use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use scan_engine::{
    FileRecord, ScanEntry, ScanProgress, ScanScope, analysis_exemption, find_similar_images_with,
    is_cloud_placeholder, scan_entries_with_scope, verify_duplicate_group_cached, volume_space,
};
use scan_store::SavedScan;

use super::{LARGE_FILE_BYTES, ScanActivity, ScanReport, ScanTimings, UPDATE_INTERVAL};
use crate::workspace::{exclusions, storage};

const PREVIEW_LIMIT: usize = 100;
/// Folders a smart scan left out that the report lists by path; the count stays exact.
const SCOPED_OUT_LIMIT: usize = 200;

/// Live counters of a running analysis, read by the UI while the worker writes them.
#[derive(Default)]
pub(in crate::workspace) struct AnalysisProgress {
    /// False while exact duplicates are checked, true once images are being compared.
    pub comparing_images: AtomicBool,
    pub candidates_total: AtomicU64,
    pub candidates_checked: AtomicU64,
    pub duplicate_groups: AtomicU64,
    /// Candidates skipped because they sit in a game library, app bundle or system folder.
    pub exempt_files: AtomicU64,
    pub images_total: AtomicUsize,
    pub images_done: AtomicUsize,
}

pub(in crate::workspace) struct AnalysisReport {
    pub saved: SavedScan,
    pub cancelled: bool,
    pub first_error: Option<String>,
}

/// Indexes `root` and commits the scan right away, so results can be browsed while
/// duplicate and similar-image analysis runs separately with `analyze_saved`.
pub(in crate::workspace) fn index_and_save(
    root: PathBuf,
    database_path: io::Result<PathBuf>,
    scope: ScanScope,
    cancel: &AtomicBool,
    mut progress_changed: impl FnMut(ScanActivity),
) -> io::Result<ScanReport> {
    let started = Instant::now();
    let (mut database, exclusions) = match database_path {
        Ok(path) => {
            let mut paths = storage::scan_exclusions(&root, &path)?;
            paths.extend(exclusions::for_scan(&root, &path)?);
            (storage::open_at(&path), paths)
        }
        Err(error) => (Err(error), Vec::new()),
    };
    let mut save_error = database.as_ref().err().map(ToString::to_string);
    let mut writer =
        database
            .as_mut()
            .ok()
            .and_then(|store| match store.begin_scan(&root, LARGE_FILE_BYTES) {
                Ok(writer) => Some(writer),
                Err(error) => {
                    save_error = Some(error.to_string());
                    None
                }
            });
    let setup = started.elapsed();
    let started = Instant::now();
    let mut scoped_out = Vec::new();
    let mut scoped_out_count = 0_u64;
    let mut report = inspect_folder(
        root,
        &exclusions,
        scope,
        cancel,
        LARGE_FILE_BYTES,
        |progress| {
            progress_changed(ScanActivity {
                progress,
                analyzing: false,
            })
        },
        |entry| {
            let saved = match entry {
                ScanEntry::File(file) => writer.as_mut().map(|current| current.insert_file(file)),
                ScanEntry::Directory(directory) => writer
                    .as_mut()
                    .map(|current| current.insert_directory(directory)),
                ScanEntry::ScopedOut { path, reason } => {
                    scoped_out_count += 1;
                    if scoped_out.len() < SCOPED_OUT_LIMIT {
                        scoped_out.push((path.clone(), *reason));
                    }
                    None
                }
            };
            if let Some(Err(error)) = saved {
                save_error = Some(error.to_string());
                // Dropping the writer discards the entire incomplete database transaction.
                writer = None;
            }
        },
    )?;
    report.timings.setup = setup;
    report.timings.indexing = started.elapsed();
    report.scoped_out = scoped_out;
    report.scoped_out_count = scoped_out_count;
    if let Some(writer) = writer {
        let started = Instant::now();
        match writer.finish(&report.summary, report.large_file_count) {
            Ok(saved) => report.saved = Some(saved),
            Err(error) => save_error = Some(error.to_string()),
        }
        report.timings.commit = started.elapsed();
    }
    report.save_error = save_error;
    log::debug!("Scan phases: {:?}", report.timings);
    Ok(report)
}

/// Adds the containing volume's capacity to a finished scan. Optional OS statistics must
/// never discard a successful scan or delay a cancelled one; call this on a background worker.
pub(in crate::workspace) fn record_volume_space(report: &mut ScanReport, cancel: &AtomicBool) {
    if !report.summary.cancelled && !cancel.load(Ordering::Relaxed) {
        let started = Instant::now();
        report.volume_space = Some(volume_space(&report.root).map_err(|error| error.to_string()));
        report.timings.volume_query = started.elapsed();
    }
}

/// Finds exact duplicates, then similar images, in a saved scan, replacing any earlier
/// results. `workers` threads decode images; duplicate checks read one file at a time,
/// which suits spinning disks. Groups found before a cancellation are kept.
pub(in crate::workspace) fn analyze_saved(
    scan: &SavedScan,
    database_path: &Path,
    workers: usize,
    cancel: &AtomicBool,
    progress: &AnalysisProgress,
    timings: &mut ScanTimings,
) -> io::Result<AnalysisReport> {
    let mut store = storage::open_at(database_path)?;
    let images = store.image_files(scan.id).map_err(io::Error::other)?;
    let candidates = store
        .duplicate_candidate_count(scan.id)
        .map_err(io::Error::other)?;
    progress
        .candidates_total
        .store(candidates, Ordering::Relaxed);
    let mut writer = store.begin_analysis(scan).map_err(io::Error::other)?;
    let mut first_error = None;

    // Exact duplicates, one size group at a time.
    let mut after_size = None;
    let mut error_count = 0_u64;
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let started = Instant::now();
        let files = writer
            .next_duplicate_candidates(after_size)
            .map_err(io::Error::other);
        timings.candidate_queries += started.elapsed();
        let mut files = files?;
        let Some(first) = files.first() else {
            writer
                .mark_duplicates_analyzed(error_count)
                .map_err(io::Error::other)?;
            break;
        };
        after_size = Some(first.logical_bytes);
        let size_group = files.len() as u64;
        files.retain(|file| skip_in_analysis(&file.path, &scan.root).is_none());
        progress
            .exempt_files
            .fetch_add(size_group - files.len() as u64, Ordering::Relaxed);
        timings.largest_candidate_set = timings.largest_candidate_set.max(files.len());
        if files.len() > 1 {
            let started = Instant::now();
            let analysis = verify_duplicate_group_cached(&files, cancel, &mut writer);
            timings.duplicate_verification += started.elapsed();
            let started = Instant::now();
            writer
                .record_cache_usage(analysis.cache_hits, analysis.cache_misses)
                .map_err(io::Error::other)?;
            error_count = error_count.saturating_add(analysis.errors);
            writer.record_duplicate_errors(error_count);
            first_error = first_error.or(analysis.first_error);
            if analysis.cancelled || cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            for group in analysis.groups {
                writer
                    .insert_duplicate_group(&group.paths, &group.hash)
                    .map_err(io::Error::other)?;
                progress.duplicate_groups.fetch_add(1, Ordering::Relaxed);
            }
            timings.duplicate_writes += started.elapsed();
        }
        progress
            .candidates_checked
            .fetch_add(size_group, Ordering::Relaxed);
    }

    // Similar images, with the same exemptions as duplicates.
    if !cancelled {
        progress.comparing_images.store(true, Ordering::Relaxed);
        let images: Vec<FileRecord> = images
            .into_iter()
            .filter(|image| skip_in_analysis(&image.path, &scan.root).is_none())
            .collect();
        progress.images_total.store(images.len(), Ordering::Relaxed);
        let started = Instant::now();
        let analysis = find_similar_images_with(&images, workers, &progress.images_done, cancel);
        timings.image_analysis = started.elapsed();
        writer.record_image_errors(analysis.errors);
        first_error = first_error.or(analysis.first_error.clone());
        if analysis.cancelled {
            cancelled = true;
        } else {
            for group in &analysis.groups {
                writer
                    .insert_similar_image_group(&group.members)
                    .map_err(io::Error::other)?;
            }
            writer
                .mark_similar_images_analyzed(analysis.errors)
                .map_err(io::Error::other)?;
        }
    }
    let started = Instant::now();
    let saved = writer.finish_analysis().map_err(io::Error::other)?;
    timings.commit += started.elapsed();
    Ok(AnalysisReport {
        saved,
        cancelled,
        first_error,
    })
}

/// Why analysis leaves `path` unread: an exempt location, or an online-only cloud file
/// that reading would download.
fn skip_in_analysis(path: &Path, root: &Path) -> Option<&'static str> {
    analysis_exemption(path, root)
        .or_else(|| is_cloud_placeholder(path).then_some("Online-only cloud file"))
}

/// Indexing then analysis in one call, as the app ran them before analysis became separate.
#[cfg(test)]
pub(in crate::workspace) fn inspect_and_save(
    root: PathBuf,
    database_path: io::Result<PathBuf>,
    cancel: &AtomicBool,
    mut progress_changed: impl FnMut(ScanActivity),
) -> io::Result<ScanReport> {
    let database = database_path.as_ref().ok().cloned();
    let mut report = index_and_save(
        root,
        database_path,
        ScanScope::Everything,
        cancel,
        &mut progress_changed,
    )?;
    let (Some(scan), Some(database)) = (report.saved.clone(), database) else {
        record_volume_space(&mut report, cancel);
        return Ok(report);
    };
    if report.summary.cancelled {
        return Ok(report);
    }
    progress_changed(ScanActivity {
        progress: report.summary.progress,
        analyzing: true,
    });
    let analysis = analyze_saved(
        &scan,
        &database,
        scan_engine::DEFAULT_IMAGE_WORKERS,
        cancel,
        &AnalysisProgress::default(),
        &mut report.timings,
    )?;
    report.summary.cancelled |= analysis.cancelled;
    if report.summary.first_error.is_none() {
        report.summary.first_error = analysis.first_error;
    }
    report.saved = Some(analysis.saved);
    record_volume_space(&mut report, cancel);
    Ok(report)
}

fn inspect_folder(
    root: PathBuf,
    exclusions: &[PathBuf],
    scope: ScanScope,
    cancel: &AtomicBool,
    large_file_threshold: u64,
    mut progress_changed: impl FnMut(ScanProgress),
    mut entry_found: impl FnMut(&ScanEntry),
) -> io::Result<ScanReport> {
    let mut largest_files = Vec::new();
    let mut large_file_count = 0;
    let mut last_update = Instant::now();
    let summary = scan_entries_with_scope(&root, exclusions, scope, cancel, |progress, entry| {
        if let Some(entry) = &entry {
            entry_found(entry);
        }
        if let Some(ScanEntry::File(file)) = entry {
            large_file_count += u64::from(file.logical_bytes >= large_file_threshold);
            largest_files.push(file);
            if largest_files.len() == PREVIEW_LIMIT * 2 {
                retain_largest(&mut largest_files);
            }
        }
        if last_update.elapsed() >= UPDATE_INTERVAL {
            progress_changed(progress);
            last_update = Instant::now();
        }
    })?;
    retain_largest(&mut largest_files);
    Ok(ScanReport {
        root,
        volume_space: None,
        summary,
        largest_files,
        large_file_count,
        saved: None,
        save_error: None,
        timings: ScanTimings::default(),
        scoped_out: Vec::new(),
        scoped_out_count: 0,
    })
}

fn retain_largest(files: &mut Vec<FileRecord>) {
    let largest_first = |left: &FileRecord, right: &FileRecord| {
        right
            .logical_bytes
            .cmp(&left.logical_bytes)
            .then_with(|| left.path.cmp(&right.path))
    };
    if files.len() > PREVIEW_LIMIT {
        files.select_nth_unstable_by(PREVIEW_LIMIT, largest_first);
        files.truncate(PREVIEW_LIMIT);
    }
    files.sort_unstable_by(largest_first);
}

#[cfg(test)]
mod tests {
    use super::*;
    use scan_store::SavedScan;
    use std::fs::File;
    use std::time::Duration;

    #[test]
    fn corrupt_storage_preserves_database_and_bounds_the_image_file_preview() -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        std::fs::create_dir(&root)?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        std::fs::create_dir(database.parent().unwrap())?;
        let original = b"corrupt database fixture; do not replace";
        std::fs::write(&database, original)?;
        for index in 0..250 {
            std::fs::write(root.join(format!("image-{index}.png")), b"not an image")?;
        }
        let report = inspect_and_save(root, Ok(database.clone()), &AtomicBool::new(false), |_| {})?;
        assert!(report.save_error.is_some());
        assert!(report.saved.is_none());
        assert_eq!(report.summary.progress.files, 250);
        assert_eq!(report.largest_files.len(), PREVIEW_LIMIT);
        assert_eq!(report.timings.largest_candidate_set, 0);
        assert_eq!(report.timings.image_analysis, Duration::ZERO);
        assert_eq!(std::fs::read(database)?, original);
        Ok(())
    }

    #[test]
    fn analysis_skips_game_libraries_but_indexes_them() -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("drive");
        let database = fixture.path().join("app-data/scans.sqlite3");
        for path in [
            "SteamLibrary/steamapps/common/Game/a.pak",
            "SteamLibrary/steamapps/common/Game/b.pak",
            "Backups/a.pak",
            "Backups/b.pak",
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, b"same bytes")?;
        }
        let cancel = AtomicBool::new(false);
        let report = index_and_save(
            root.clone(),
            Ok(database.clone()),
            ScanScope::Smart,
            &cancel,
            |_| {},
        )?;
        let scan = report.saved.expect("index saved");
        assert_eq!(
            scan.progress.files, 4,
            "game files are still indexed for size"
        );
        let progress = AnalysisProgress::default();
        let analysis = analyze_saved(
            &scan,
            &database,
            2,
            &cancel,
            &progress,
            &mut ScanTimings::default(),
        )?;
        assert!(!analysis.cancelled);
        assert_eq!(
            analysis.saved.duplicate_group_count, 1,
            "only the backup copies are grouped"
        );
        assert_eq!(progress.exempt_files.load(Ordering::Relaxed), 2);
        assert_eq!(progress.candidates_checked.load(Ordering::Relaxed), 4);
        Ok(())
    }

    #[test]
    fn disappearing_files_preserve_index_and_other_analysis_groups() -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        std::fs::create_dir(&root)?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        for name in ["a", "b", "missing"] {
            std::fs::write(root.join(format!("{name}.txt")), b"same")?;
            image::RgbImage::new(8, 8)
                .save(root.join(format!("{name}.png")))
                .map_err(io::Error::other)?;
        }
        let report = inspect_and_save(
            root.clone(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |activity| {
                if activity.analyzing {
                    std::fs::remove_file(root.join("missing.txt")).unwrap();
                    std::fs::remove_file(root.join("missing.png")).unwrap();
                }
            },
        )?;
        assert!(report.save_error.is_none());
        let saved = report.saved.unwrap();
        assert_eq!(saved.progress.files, 6);
        assert_eq!(saved.duplicate_group_count, 2);
        assert_eq!(saved.duplicate_error_count, 2);
        assert_eq!(saved.similar_group_count, 1);
        assert_eq!(saved.image_error_count, 1);
        assert!(saved.duplicates_analyzed && saved.similar_images_analyzed);
        // Analysis problems are reported for the session; the saved row keeps indexing errors.
        assert!(
            report
                .summary
                .first_error
                .as_ref()
                .unwrap()
                .contains("missing")
        );
        let reopened = storage::open_at(&database)?;
        assert_eq!(
            reopened
                .files(saved.id, "", 10, 0)
                .map_err(io::Error::other)?
                .len(),
            6
        );
        assert_eq!(
            reopened.scans(1, 0).map_err(io::Error::other)?[0].duplicate_error_count,
            2
        );
        Ok(())
    }

    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn benchmark_large_candidate_set() -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        std::fs::create_dir(&root)?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        const FILES: u64 = 20_000;
        for index in 0..FILES {
            let mut bytes = vec![0_u8; 4096];
            if index >= FILES / 2 {
                bytes[..8].copy_from_slice(&index.to_le_bytes());
            }
            std::fs::write(root.join(format!("file-{index:05}")), bytes)?;
        }
        for pass in 1..=3 {
            let started = Instant::now();
            let report = inspect_and_save(
                root.clone(),
                Ok(database.clone()),
                &AtomicBool::new(false),
                |_| {},
            )?;
            println!(
                "Large candidate set, pass {pass}: {:.3} s · {:?}",
                started.elapsed().as_secs_f64(),
                report.timings
            );
            assert!(report.save_error.is_none());
            assert_eq!(report.timings.largest_candidate_set, FILES as usize);
            let saved = report.saved.unwrap();
            assert_eq!(saved.progress.files, FILES);
            assert_eq!(saved.duplicate_group_count, 1);
            assert_eq!(saved.duplicate_error_count, 0);
            let store = storage::open_at(&database)?;
            assert_eq!(
                store
                    .duplicate_groups(saved.id, 10, 0)
                    .map_err(io::Error::other)?[0]
                    .file_count,
                FILES / 2
            );
        }
        Ok(())
    }

    #[test]
    fn previews_the_largest_files_and_counts_threshold_matches() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let threshold = (PREVIEW_LIMIT * 2 + 10) as u64;
        for size in 0..threshold {
            File::create(root.path().join(format!("file-{size}")))?.set_len(size)?;
        }
        let threshold_file = root.path().join("archive.zip");
        File::create(&threshold_file)?.set_len(threshold)?;

        let report = inspect_folder(
            root.path().to_path_buf(),
            &[],
            ScanScope::Everything,
            &AtomicBool::new(false),
            threshold,
            |_| {},
            |_| {},
        )?;
        assert_eq!(report.summary.progress.files, threshold + 1);
        assert_eq!(report.large_file_count, 1);
        assert_eq!(report.largest_files.len(), PREVIEW_LIMIT);
        assert_eq!(report.largest_files[0].path, threshold_file);
        assert_eq!(
            report.largest_files[PREVIEW_LIMIT - 1].logical_bytes,
            threshold - PREVIEW_LIMIT as u64 + 1
        );
        Ok(())
    }

    #[test]
    fn reopens_a_scan_and_finds_a_file_outside_the_preview() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("app-data/scans.sqlite3");
        for index in 0..PREVIEW_LIMIT {
            std::fs::write(root.path().join(format!("file-{index}")), b"data")?;
        }
        let needle = root.path().join("find-me.txt");
        File::create(&needle)?;

        let report = inspect_and_save(
            root.path().into(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        assert!(report.save_error.is_none(), "{:?}", report.save_error);
        assert_eq!(report.summary.progress.files, PREVIEW_LIMIT as u64 + 1);
        assert_eq!(report.summary.progress.skipped, 1);
        assert!(!report.largest_files.iter().any(|file| file.path == needle));

        let reopened = storage::open_at(&database)?;
        let history = reopened.scans(10, 0).map_err(io::Error::other)?;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, report.saved.unwrap().id);
        let files = reopened
            .files(history[0].id, "FIND-ME", 100, 0)
            .map_err(io::Error::other)?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, needle);
        Ok(())
    }

    #[test]
    fn retains_preview_if_storage_is_unavailable() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("keep.txt"), "keep")?;
        let report = inspect_and_save(
            root.path().into(),
            Err(io::Error::other("no storage")),
            &AtomicBool::new(false),
            |_| {},
        )?;
        assert!(report.saved.is_none());
        assert_eq!(report.save_error.as_deref(), Some("no storage"));
        assert_eq!(report.largest_files.len(), 1);
        assert!(report.volume_space.as_ref().is_some_and(Result::is_ok));
        Ok(())
    }

    #[test]
    fn unavailable_volume_statistics_do_not_discard_a_completed_scan() -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        std::fs::create_dir(&root)?;
        let report = inspect_and_save(
            root.clone(),
            Ok(fixture.path().join("app-data/scans.sqlite3")),
            &AtomicBool::new(false),
            |activity| {
                if activity.analyzing {
                    // Simulate a selected directory disappearing after indexing.
                    std::fs::remove_dir(&root).expect("remove empty fixture");
                }
            },
        )?;
        assert!(!report.summary.cancelled);
        assert!(report.save_error.is_none());
        assert!(report.saved.is_some());
        assert!(report.volume_space.as_ref().is_some_and(Result::is_err));
        Ok(())
    }

    #[test]
    fn persists_exact_duplicate_findings_for_the_inspector() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("app-data/scans.sqlite3");
        for (name, contents) in [
            ("original.txt", "same"),
            ("copy.txt", "same"),
            ("distinct.txt", "else"),
        ] {
            std::fs::write(root.path().join(name), contents)?;
        }

        let report = inspect_and_save(
            root.path().into(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        assert!(report.save_error.is_none(), "{:?}", report.save_error);
        let saved = report.saved.unwrap();
        assert!(saved.duplicates_analyzed);
        assert_eq!(saved.duplicate_group_count, 1);
        assert_eq!(saved.duplicate_error_count, 0);

        let store = storage::open_at(&database)?;
        let files = store
            .duplicate_files(saved.id, "", 10, 0)
            .map_err(io::Error::other)?;
        assert_eq!(files.len(), 2);
        let evidence = store
            .duplicate_evidence(saved.id, &files)
            .map_err(io::Error::other)?;
        let original = &evidence[&root.path().join("original.txt")];
        let copy = &evidence[&root.path().join("copy.txt")];
        assert_eq!(original.file_count, 2);
        assert_eq!(original.hash, copy.hash);
        assert!(!evidence.contains_key(&root.path().join("distinct.txt")));
        Ok(())
    }

    #[test]
    fn persists_similar_images_apart_from_exact_duplicates() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("app-data/scans.sqlite3");
        let scene = image::RgbImage::from_fn(240, 160, |x, y| {
            let in_square = (60..140).contains(&x) && (40..120).contains(&y);
            image::Rgb(if in_square {
                [220, 30, 30]
            } else {
                [(x + y) as u8; 3]
            })
        });
        let smaller =
            image::imageops::resize(&scene, 120, 80, image::imageops::FilterType::Triangle);
        scene
            .save(root.path().join("original.png"))
            .map_err(io::Error::other)?;
        smaller
            .save(root.path().join("smaller.png"))
            .map_err(io::Error::other)?;
        std::fs::write(root.path().join("renamed.png"), "not an image")?;

        let report = inspect_and_save(
            root.path().into(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let saved = report.saved.expect("scan saved");
        assert!(saved.similar_images_analyzed);
        assert_eq!(saved.similar_group_count, 1);
        assert_eq!(saved.image_error_count, 1);
        assert_eq!(saved.duplicate_group_count, 0);
        let store = storage::open_at(&database)?;
        let members = store
            .similar_image_group_files(saved.id, 1, 10)
            .map_err(io::Error::other)?;
        assert_eq!(members.len(), 2);
        Ok(())
    }

    #[test]
    fn cancelling_duplicate_verification_keeps_the_file_index() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("app-data/scans.sqlite3");
        std::fs::write(root.path().join("one"), "same")?;
        std::fs::write(root.path().join("two"), "same")?;
        let cancel = AtomicBool::new(false);
        let report = inspect_and_save(
            root.path().into(),
            Ok(database.clone()),
            &cancel,
            |activity| {
                if activity.analyzing {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        )?;
        assert!(report.summary.cancelled);
        assert!(report.volume_space.is_none());
        let saved = report.saved.unwrap();
        // The index was saved as complete; only its analysis was cancelled.
        assert!(!saved.cancelled);
        assert!(!saved.duplicates_analyzed);
        assert!(!saved.similar_images_analyzed);
        assert_eq!(saved.duplicate_group_count, 0);
        let store = storage::open_at(&database)?;
        assert_eq!(
            store
                .files(saved.id, "", 10, 0)
                .map_err(io::Error::other)?
                .len(),
            2
        );
        Ok(())
    }

    #[test]
    fn repeat_scans_revalidate_cached_hashes_and_persist_empty_folders() -> io::Result<()> {
        use std::fs::{self, FileTimes};
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        fs::create_dir_all(root.join("empty"))?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        let original = vec![b'a'; 128 * 1024];
        fs::write(root.join("a"), &original)?;
        fs::write(root.join("b"), &original)?;
        let run = || -> io::Result<SavedScan> {
            let report = inspect_and_save(
                root.clone(),
                Ok(database.clone()),
                &AtomicBool::new(false),
                |_| {},
            )?;
            assert!(report.save_error.is_none(), "{:?}", report.save_error);
            Ok(report.saved.unwrap())
        };
        let first = run()?;
        assert_eq!(first.duplicate_group_count, 1);
        assert_eq!(first.hash_cache_hits, 0);
        assert_eq!(first.hash_cache_misses, 2);
        let repeat = run()?;
        assert_eq!(repeat.duplicate_group_count, 1);
        assert_eq!(repeat.hash_cache_hits, 2);
        assert_eq!(repeat.hash_cache_misses, 0);
        let store = storage::open_at(&database)?;
        assert_eq!(
            store
                .empty_directories(repeat.id, 10, 0)
                .map_err(io::Error::other)?[0]
                .path,
            root.join("empty")
        );
        let history = store.scans(10, 0).map_err(io::Error::other)?;
        assert_eq!(history[0].hash_cache_hits, 2);
        drop(store);

        // Same length and restored modification time must not reuse the old hash.
        let modified = fs::metadata(root.join("b"))?.modified()?;
        let mut changed_contents = original.clone();
        changed_contents[64 * 1024] = b'b';
        fs::write(root.join("b"), &changed_contents)?;
        File::options()
            .write(true)
            .open(root.join("b"))?
            .set_times(FileTimes::new().set_modified(modified))?;
        let changed = run()?;
        assert_eq!(changed.duplicate_group_count, 0);
        assert!(changed.hash_cache_misses > 0);
        fs::write(root.join("replacement"), &original)?;
        fs::remove_file(root.join("b"))?;
        fs::rename(root.join("replacement"), root.join("b"))?;
        File::options()
            .write(true)
            .open(root.join("b"))?
            .set_times(FileTimes::new().set_modified(modified))?;
        let replaced = run()?;
        assert_eq!(replaced.duplicate_group_count, 1);
        assert!(replaced.hash_cache_misses > 0);
        fs::remove_file(root.join("b"))?;
        fs::hard_link(root.join("a"), root.join("b"))?;
        assert_eq!(run()?.duplicate_group_count, 0);
        assert_eq!(fs::read(root.join("a"))?, original);
        Ok(())
    }

    // End-to-end benchmarks over the app's scan, index, and verification pipeline.
    // Run one at a time so peak RSS belongs to one input; see docs/benchmarks.md.
    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn benchmark_mostly_unique_end_to_end() -> io::Result<()> {
        end_to_end_benchmark(false)
    }

    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn benchmark_duplicate_heavy_end_to_end() -> io::Result<()> {
        end_to_end_benchmark(true)
    }

    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn benchmark_large_cached_duplicates() -> io::Result<()> {
        use std::fs;
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        fs::create_dir(&root)?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        let contents = vec![42_u8; 32 * 1024 * 1024];
        for index in 0..8 {
            fs::write(root.join(format!("copy-{index}.bin")), &contents)?;
        }
        drop(contents);
        for pass in ["first", "repeat"] {
            let started = Instant::now();
            let report = inspect_and_save(
                root.clone(),
                Ok(database.clone()),
                &AtomicBool::new(false),
                |_| {},
            )?;
            let saved = report.saved.expect("saved benchmark");
            assert_eq!(saved.duplicate_group_count, 1);
            assert_eq!(saved.duplicate_error_count, 0);
            println!(
                "Large duplicates ({pass}): {:.3} s · {} hits · {} misses",
                started.elapsed().as_secs_f64(),
                saved.hash_cache_hits,
                saved.hash_cache_misses
            );
            if pass == "repeat" {
                assert_eq!(saved.hash_cache_hits, 8);
                assert_eq!(saved.hash_cache_misses, 0);
            }
        }
        Ok(())
    }

    const BENCHMARK_FOLDERS: u64 = 1_000;
    const BENCHMARK_FILES_PER_FOLDER: u64 = 100;
    const BENCHMARK_FILES: u64 = BENCHMARK_FOLDERS * BENCHMARK_FILES_PER_FOLDER;

    /// Each folder holds 100 files of one size (4,096 + folder bytes), so both inputs give
    /// 1,000 same-size candidate buckets. Mostly unique files differ in their first bytes;
    /// duplicate-heavy folders hold two contents of 50 identical files each.
    fn end_to_end_benchmark(duplicate_heavy: bool) -> io::Result<()> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("tree");
        let database = fixture.path().join("app-data/scans.sqlite3");
        let started = Instant::now();
        for folder in 0..BENCHMARK_FOLDERS {
            let directory = root.join(format!("folder-{folder:04}"));
            std::fs::create_dir_all(&directory)?;
            for file in 0..BENCHMARK_FILES_PER_FOLDER {
                let mut contents = vec![0_u8; 4_096 + folder as usize];
                if duplicate_heavy {
                    contents.fill(b'a' + (file % 2) as u8);
                } else {
                    contents[..8].copy_from_slice(&(folder * 1_000 + file).to_le_bytes());
                }
                std::fs::write(directory.join(format!("report-{file:03}.txt")), contents)?;
            }
        }
        let expected_groups = if duplicate_heavy {
            2 * BENCHMARK_FOLDERS
        } else {
            0
        };
        println!(
            "Input: {} · fixture written in {:.1} s",
            if duplicate_heavy {
                "duplicate-heavy"
            } else {
                "mostly unique"
            },
            started.elapsed().as_secs_f64()
        );

        // The first pass follows fixture creation, so file data is likely still cached.
        let mut scan_id = 0;
        for pass in ["first", "repeat"] {
            let cancel = AtomicBool::new(false);
            let started = Instant::now();
            let mut indexed = None;
            let report =
                inspect_and_save(root.clone(), Ok(database.clone()), &cancel, |activity| {
                    if activity.analyzing && indexed.is_none() {
                        indexed = Some(started.elapsed());
                    }
                })?;
            let total = started.elapsed();
            println!("Scan phases ({pass}): {:?}", report.timings);
            assert!(report.save_error.is_none(), "{:?}", report.save_error);
            let saved = report.saved.expect("benchmark scan saved");
            assert_eq!(saved.progress.files, BENCHMARK_FILES);
            assert_eq!(saved.duplicate_group_count, expected_groups);
            let indexed = indexed.expect("verification phase reported");
            println!(
                "Scan ({pass}): traversal + SQLite index {:.2} s · verification + commit {:.2} s · total {:.2} s",
                indexed.as_secs_f64(),
                (total - indexed).as_secs_f64(),
                total.as_secs_f64()
            );
            println!(
                "Hash cache ({pass}): {} hits · {} misses",
                saved.hash_cache_hits, saved.hash_cache_misses
            );
            scan_id = saved.id;
        }

        let store = storage::open_at(&database)?;
        let last_page = BENCHMARK_FILES - 100;
        timed("First page", || store.files(scan_id, "", 101, 0))?;
        timed("Last page", || store.files(scan_id, "", 101, last_page))?;
        timed("Search folder-0999", || {
            store.files(scan_id, "folder-0999", 101, 0)
        })?;
        timed("Search report, last page", || {
            store.files(scan_id, "report", 101, last_page)
        })?;
        timed("Duplicates first page", || {
            store.duplicate_files(scan_id, "", 101, 0)
        })?;
        timed("File types", || store.file_types(scan_id))?;

        for during_verification in [false, true] {
            let cancel = AtomicBool::new(false);
            let mut cancelled_at = None;
            let report =
                inspect_and_save(root.clone(), Ok(database.clone()), &cancel, |activity| {
                    let ready = if during_verification {
                        activity.analyzing
                    } else {
                        activity.progress.files >= BENCHMARK_FILES / 2
                    };
                    if ready && cancelled_at.is_none() {
                        cancel.store(true, Ordering::Relaxed);
                        cancelled_at = Some(Instant::now());
                    }
                })?;
            let latency = cancelled_at.expect("cancellation was requested").elapsed();
            assert!(report.summary.cancelled);
            println!(
                "Cancel during {}: returned in {:.1} ms after {} indexed files",
                if during_verification {
                    "verification"
                } else {
                    "indexing"
                },
                millis(latency),
                report.summary.progress.files
            );
        }
        Ok(())
    }

    fn timed<T, E>(label: &str, query: impl FnOnce() -> Result<Vec<T>, E>) -> io::Result<()>
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let started = Instant::now();
        let rows = query().map_err(io::Error::other)?.len();
        println!(
            "{label}: {rows} rows in {:.1} ms",
            millis(started.elapsed())
        );
        Ok(())
    }

    fn millis(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1_000.0
    }
}
