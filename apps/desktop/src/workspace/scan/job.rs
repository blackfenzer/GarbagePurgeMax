use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use scan_engine::{
    FileRecord, ScanEntry, ScanProgress, ScanSummary, find_similar_images, is_hashable_image,
    scan_entries, verify_duplicate_group_cached,
};
use scan_store::ScanWriter;

use super::{LARGE_FILE_BYTES, ScanActivity, ScanReport, UPDATE_INTERVAL};
use crate::workspace::{exclusions, storage};

const PREVIEW_LIMIT: usize = 100;

pub(in crate::workspace) fn inspect_and_save(
    root: PathBuf,
    database_path: io::Result<PathBuf>,
    cancel: &AtomicBool,
    mut progress_changed: impl FnMut(ScanActivity),
) -> io::Result<ScanReport> {
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
    let mut image_files = Vec::new();
    let mut report = inspect_folder(
        root,
        &exclusions,
        cancel,
        LARGE_FILE_BYTES,
        |progress| {
            progress_changed(ScanActivity {
                progress,
                analyzing: false,
            })
        },
        |entry| {
            if let ScanEntry::File(file) = entry
                && is_hashable_image(&file.path)
            {
                image_files.push(file.clone());
            }
            if let Some(current) = &mut writer
                && let Err(error) = match entry {
                    ScanEntry::File(file) => current.insert_file(file),
                    ScanEntry::Directory(directory) => current.insert_directory(directory),
                }
            {
                save_error = Some(error.to_string());
                // Dropping the writer discards the entire incomplete database transaction.
                writer = None;
            }
        },
    )?;
    if let Some(mut writer) = writer {
        let verification = if report.summary.cancelled {
            Ok(())
        } else {
            progress_changed(ScanActivity {
                progress: report.summary.progress,
                analyzing: true,
            });
            verify_indexed_duplicates(&mut writer, &mut report.summary, cancel).and_then(|()| {
                group_similar_images(&mut writer, &image_files, &mut report.summary, cancel)
            })
        };
        let saved = verification.and_then(|()| {
            writer
                .finish(&report.summary, report.large_file_count)
                .map_err(io::Error::other)
        });
        match saved {
            Ok(saved) => report.saved = Some(saved),
            Err(error) => save_error = Some(error.to_string()),
        }
    }
    report.save_error = save_error;
    Ok(report)
}

fn verify_indexed_duplicates(
    writer: &mut ScanWriter<'_>,
    summary: &mut ScanSummary,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let mut after_size = None;
    let mut error_count = 0_u64;
    while !cancel.load(Ordering::Relaxed) {
        let files = writer
            .next_duplicate_candidates(after_size)
            .map_err(io::Error::other)?;
        let Some(first) = files.first() else {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            return writer
                .mark_duplicates_analyzed(error_count)
                .map_err(io::Error::other);
        };
        after_size = Some(first.logical_bytes);
        let analysis = verify_duplicate_group_cached(&files, cancel, writer);
        writer
            .record_cache_usage(analysis.cache_hits, analysis.cache_misses)
            .map_err(io::Error::other)?;
        error_count = error_count.saturating_add(analysis.errors);
        writer.record_duplicate_errors(error_count);
        if summary.first_error.is_none() {
            summary.first_error = analysis.first_error;
        }
        if analysis.cancelled || cancel.load(Ordering::Relaxed) {
            break;
        }
        for group in analysis.groups {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                return Ok(());
            }
            writer
                .insert_duplicate_group(&group.paths, &group.hash)
                .map_err(io::Error::other)?;
        }
    }
    summary.cancelled = true;
    Ok(())
}

/// Groups perceptually similar images after exact duplicates are saved. Decode failures
/// are counted on the scan, not reported as traversal errors.
fn group_similar_images(
    writer: &mut ScanWriter<'_>,
    image_files: &[FileRecord],
    summary: &mut ScanSummary,
    cancel: &AtomicBool,
) -> io::Result<()> {
    if summary.cancelled {
        return Ok(());
    }
    let analysis = find_similar_images(image_files, cancel);
    writer.record_image_errors(analysis.errors);
    if summary.first_error.is_none() {
        summary.first_error = analysis.first_error.clone();
    }
    if analysis.cancelled {
        summary.cancelled = true;
        return Ok(());
    }
    for group in &analysis.groups {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            return Ok(());
        }
        writer
            .insert_similar_image_group(&group.members)
            .map_err(io::Error::other)?;
    }
    writer
        .mark_similar_images_analyzed(analysis.errors)
        .map_err(io::Error::other)
}

fn inspect_folder(
    root: PathBuf,
    exclusions: &[PathBuf],
    cancel: &AtomicBool,
    large_file_threshold: u64,
    mut progress_changed: impl FnMut(ScanProgress),
    mut entry_found: impl FnMut(&ScanEntry),
) -> io::Result<ScanReport> {
    let mut largest_files = Vec::new();
    let mut large_file_count = 0;
    let mut last_update = Instant::now();
    let summary = scan_entries(&root, exclusions, cancel, |progress, entry| {
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
        summary,
        largest_files,
        large_file_count,
        saved: None,
        save_error: None,
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
        let saved = report.saved.unwrap();
        assert!(saved.cancelled);
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
