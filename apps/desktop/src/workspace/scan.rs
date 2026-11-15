use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use gpui_kit::{AppContext as _, Context, PathPromptOptions};
use scan_engine::{FileRecord, ScanProgress, ScanSummary, scan_directory};
use scan_store::SavedScan;

use super::{Page, Workspace, storage};

const UPDATE_INTERVAL: Duration = Duration::from_millis(100);
const PREVIEW_LIMIT: usize = 100;
pub(super) const LARGE_FILE_BYTES: u64 = 1 << 30;

#[derive(Default)]
pub(super) struct ScanSession {
    pub choosing_folder: bool,
    pub active: Option<ActiveScan>,
    pub report: Option<ScanReport>,
    pub error: Option<String>,
}

pub(super) struct ActiveScan {
    pub root: PathBuf,
    pub progress: ScanProgress,
    pub cancelling: bool,
    cancel: Arc<AtomicBool>,
}

impl Drop for ActiveScan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(super) struct ScanReport {
    pub root: PathBuf,
    pub summary: ScanSummary,
    pub largest_files: Vec<FileRecord>,
    pub large_file_count: u64,
    pub saved: Option<SavedScan>,
    pub save_error: Option<String>,
}

impl Workspace {
    pub(super) fn choose_scan_folder(&mut self, cx: &mut Context<Self>) {
        if self.scan.choosing_folder || self.scan.active.is_some() {
            return;
        }
        self.scan.choosing_folder = true;
        self.scan.error = None;
        cx.notify();

        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Scan folder".into()),
        });
        cx.spawn(async move |workspace, cx| {
            let selection = picker
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.scan.choosing_folder = false;
                match selection {
                    Ok(Some(paths)) => {
                        if let Some(root) = paths.into_iter().next() {
                            workspace.start_scan(root, cx);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        workspace.scan.error = Some(format!("Folder picker failed: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &mut self.scan.active {
            active.cancelling = true;
            active.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    fn start_scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan.active = Some(ActiveScan {
            root: root.clone(),
            progress: ScanProgress::default(),
            cancelling: false,
            cancel: Arc::clone(&cancel),
        });

        let latest = Arc::new(Mutex::new(ScanProgress::default()));
        let worker_progress = Arc::clone(&latest);
        let (send, receive) = mpsc::sync_channel(1);
        cx.background_spawn(async move {
            let result = inspect_and_save(root, storage::database_path(), &cancel, |progress| {
                *worker_progress.lock().expect("scan progress lock") = progress;
            });
            let _ = send.send(result);
        })
        .detach();

        let executor = cx.background_executor().clone();
        cx.spawn(async move |workspace, cx| {
            loop {
                let result = match receive.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(io::Error::other(
                        "Scan worker stopped before returning results",
                    ))),
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                if let Some(result) = result {
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace.scan.active = None;
                        match result {
                            Ok(report) => {
                                workspace.results.show_scan(&report, cx);
                                workspace.scan.report = Some(report);
                                workspace.page = Page::Results;
                            }
                            Err(error) => {
                                workspace.scan.error =
                                    Some(format!("Could not scan folder: {error}"));
                                workspace.page = Page::Overview;
                            }
                        }
                        cx.notify();
                    });
                    break;
                }

                let progress = *latest.lock().expect("scan progress lock");
                if workspace
                    .update(cx, |workspace, cx| {
                        if let Some(active) = &mut workspace.scan.active
                            && active.progress != progress
                        {
                            active.progress = progress;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                executor.timer(UPDATE_INTERVAL).await;
            }
        })
        .detach();
    }
}

fn inspect_and_save(
    root: PathBuf,
    database_path: io::Result<PathBuf>,
    cancel: &AtomicBool,
    progress_changed: impl FnMut(ScanProgress),
) -> io::Result<ScanReport> {
    let (mut database, exclusions) = match database_path {
        Ok(path) => {
            let exclusions = storage::scan_exclusions(&root, &path)?;
            (storage::open_at(&path), exclusions)
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
    let mut report = inspect_folder(
        root,
        &exclusions,
        cancel,
        LARGE_FILE_BYTES,
        progress_changed,
        |file| {
            if let Some(current) = &mut writer
                && let Err(error) = current.insert_file(file)
            {
                save_error = Some(error.to_string());
                // Dropping the writer discards the entire incomplete database transaction.
                writer = None;
            }
        },
    )?;
    if let Some(writer) = writer {
        match writer.finish(&report.summary, report.large_file_count) {
            Ok(saved) => report.saved = Some(saved),
            Err(error) => save_error = Some(error.to_string()),
        }
    }
    report.save_error = save_error;
    Ok(report)
}

fn inspect_folder(
    root: PathBuf,
    exclusions: &[PathBuf],
    cancel: &AtomicBool,
    large_file_threshold: u64,
    mut progress_changed: impl FnMut(ScanProgress),
    mut file_found: impl FnMut(&FileRecord),
) -> io::Result<ScanReport> {
    let mut largest_files = Vec::new();
    let mut large_file_count = 0;
    let mut last_update = Instant::now();
    let summary = scan_directory(&root, exclusions, cancel, |progress, file| {
        if let Some(file) = file {
            file_found(&file);
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

pub(super) fn format_bytes(bytes: u64) -> String {
    for (scale, label) in [
        (1 << 40, "TiB"),
        (1 << 30, "GiB"),
        (1 << 20, "MiB"),
        (1 << 10, "KiB"),
    ] {
        if bytes >= scale {
            return format!("{:.1} {label}", bytes as f64 / scale as f64);
        }
    }
    format!("{bytes} B")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

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
}
