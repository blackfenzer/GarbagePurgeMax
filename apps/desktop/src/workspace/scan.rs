use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, PathPromptOptions};
use scan_engine::{FileRecord, ScanProgress, ScanSummary};
use scan_store::SavedScan;

use super::{Page, Workspace, storage};

mod job;
pub(super) use job::inspect_and_save;

const UPDATE_INTERVAL: Duration = Duration::from_millis(100);
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
    pub analyzing: bool,
    pub cancelling: bool,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ScanActivity {
    progress: ScanProgress,
    analyzing: bool,
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
        if !self.can_start_scan() {
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

    /// Repeats a saved scan's folder with the current exclusions.
    pub(super) fn scan_again(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.can_start_scan() {
            return;
        }
        self.scan.error = None;
        self.page = Page::Overview;
        self.start_scan(root, cx);
        cx.notify();
    }

    pub(super) fn can_start_scan(&self) -> bool {
        !self.scan.choosing_folder && self.scan.active.is_none() && !self.exclusion_settings_busy()
    }

    fn start_scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan.active = Some(ActiveScan {
            root: root.clone(),
            progress: ScanProgress::default(),
            analyzing: false,
            cancelling: false,
            cancel: Arc::clone(&cancel),
        });

        let latest = Arc::new(Mutex::new(ScanActivity::default()));
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

                let activity = *latest.lock().expect("scan progress lock");
                if workspace
                    .update(cx, |workspace, cx| {
                        if let Some(active) = &mut workspace.scan.active
                            && (active.progress != activity.progress
                                || active.analyzing != activity.analyzing)
                        {
                            active.progress = activity.progress;
                            active.analyzing = activity.analyzing;
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
