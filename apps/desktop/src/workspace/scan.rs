use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, PathPromptOptions};
use scan_engine::{FileRecord, ScanProgress, ScanSummary, VolumeSpace};
use scan_store::SavedScan;

use super::{Page, Workspace, storage};

mod job;
#[cfg(test)]
pub(super) use job::inspect_and_save;
pub(super) use job::{
    AnalysisProgress, AnalysisReport, analyze_saved, index_and_save, record_volume_space,
};

const UPDATE_INTERVAL: Duration = Duration::from_millis(100);
pub(super) const LARGE_FILE_BYTES: u64 = 1 << 30;

#[derive(Default)]
pub(super) struct ScanSession {
    pub choosing_folder: bool,
    pub active: Option<ActiveScan>,
    pub report: Option<ScanReport>,
    pub error: Option<String>,
    /// Duplicate and similar-image analysis of a saved scan, running after indexing.
    pub analysis: Option<ActiveAnalysis>,
    /// How the last analysis ended, shown until the next one starts.
    pub analysis_note: Option<String>,
}

pub(super) struct ActiveAnalysis {
    pub root: PathBuf,
    pub workers: usize,
    pub progress: Arc<AnalysisProgress>,
    pub cancelling: bool,
    cancel: Arc<AtomicBool>,
}

impl Drop for ActiveAnalysis {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
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
    pub volume_space: Option<Result<VolumeSpace, String>>,
    pub summary: ScanSummary,
    pub largest_files: Vec<FileRecord>,
    pub large_file_count: u64,
    pub saved: Option<SavedScan>,
    pub save_error: Option<String>,
    pub timings: ScanTimings,
    /// Folders a smart scan left unindexed (first 200) and their exact count.
    pub scoped_out: Vec<(PathBuf, &'static str)>,
    pub scoped_out_count: u64,
}

/// Wall-clock diagnostics for one scan; never interpreted as disk throughput.
#[derive(Debug, Default)]
pub(super) struct ScanTimings {
    pub setup: Duration,
    pub indexing: Duration,
    pub candidate_queries: Duration,
    pub duplicate_verification: Duration,
    pub duplicate_writes: Duration,
    pub image_analysis: Duration,
    pub commit: Duration,
    pub volume_query: Duration,
    pub largest_candidate_set: usize,
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
        self.show_page(Page::Overview);
        self.start_scan(root, cx);
        cx.notify();
    }

    /// Runs duplicate and similar-image analysis on a saved scan, replacing earlier results.
    pub(super) fn start_analysis(&mut self, scan: SavedScan, cx: &mut Context<Self>) {
        if !self.can_start_scan() {
            return;
        }
        let database = match storage::database_path() {
            Ok(database) => database,
            Err(error) => {
                self.scan.analysis_note = Some(format!("Could not start analysis: {error}"));
                cx.notify();
                return;
            }
        };
        let workers = self.preferences.analysis_workers;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AnalysisProgress::default());
        self.scan.analysis_note = None;
        self.scan.analysis = Some(ActiveAnalysis {
            root: scan.root.clone(),
            workers,
            progress: Arc::clone(&progress),
            cancelling: false,
            cancel: Arc::clone(&cancel),
        });

        let (send, receive) = mpsc::sync_channel(1);
        cx.background_spawn(async move {
            let mut timings = ScanTimings::default();
            let result = analyze_saved(&scan, &database, workers, &cancel, &progress, &mut timings);
            log::debug!("Analysis phases: {timings:?}");
            let _ = send.send(result);
        })
        .detach();

        let executor = cx.background_executor().clone();
        cx.spawn(async move |workspace, cx| {
            loop {
                let result = match receive.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(io::Error::other(
                        "Analysis worker stopped before returning results",
                    ))),
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                let finished = workspace.update(cx, |workspace, cx| {
                    if let Some(result) = result {
                        workspace.finish_analysis(result, cx);
                        true
                    } else {
                        // Counters change on the worker; redraw to show them.
                        cx.notify();
                        false
                    }
                });
                if finished.unwrap_or(true) {
                    break;
                }
                executor.timer(UPDATE_INTERVAL).await;
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cancel_analysis(&mut self, cx: &mut Context<Self>) {
        if let Some(analysis) = &mut self.scan.analysis {
            analysis.cancelling = true;
            analysis.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    fn finish_analysis(&mut self, result: io::Result<AnalysisReport>, cx: &mut Context<Self>) {
        self.scan.analysis = None;
        match result {
            Ok(report) => {
                let problem = report
                    .first_error
                    .as_ref()
                    .map(|error| format!(" First unreadable item: {error}"))
                    .unwrap_or_default();
                self.scan.analysis_note = Some(
                    if report.cancelled {
                        "Analysis cancelled. Groups found so far are kept; run it again to finish."
                            .into()
                    } else {
                        format!(
                            "Analysis complete: {}, {}.",
                            count(report.saved.duplicate_group_count, "duplicate group"),
                            count(report.saved.similar_group_count, "similar-image group")
                        )
                    } + &problem,
                );
                if let Some(saved) = self
                    .scan
                    .report
                    .as_mut()
                    .and_then(|report| report.saved.as_mut())
                    && saved.id == report.saved.id
                {
                    *saved = report.saved.clone();
                }
                if self.results.saved_scan().map(|scan| scan.id) == Some(report.saved.id) {
                    self.results.open_saved(report.saved.clone(), cx);
                }
                self.review.open(Some(report.saved), cx);
                self.overview.load(cx);
            }
            Err(error) => self.scan.analysis_note = Some(format!("Analysis failed: {error}")),
        }
        cx.notify();
    }

    pub(super) fn can_start_scan(&self) -> bool {
        !self.scan.choosing_folder
            && self.scan.active.is_none()
            && self.scan.analysis.is_none()
            && !self.exclusion_settings_busy()
            && !self.cleanup.busy
    }

    fn start_scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.can_start_scan() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan.active = Some(ActiveScan {
            root: root.clone(),
            progress: ScanProgress::default(),
            analyzing: false,
            cancelling: false,
            cancel: Arc::clone(&cancel),
        });

        let scope = self.preferences.scope();
        let latest = Arc::new(Mutex::new(ScanActivity::default()));
        let worker_progress = Arc::clone(&latest);
        let (send, receive) = mpsc::sync_channel(1);
        cx.background_spawn(async move {
            let result =
                index_and_save(root, storage::database_path(), scope, &cancel, |progress| {
                    *worker_progress.lock().expect("scan progress lock") = progress;
                });
            let result = result.map(|mut report| {
                record_volume_space(&mut report, &cancel);
                report
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
                                let analyze = report.saved.clone().filter(|_| {
                                    !report.summary.cancelled && workspace.preferences.auto_analyze
                                });
                                workspace.scan.report = Some(report);
                                workspace.overview.load(cx);
                                workspace.show_page(Page::Results);
                                if let Some(saved) = analyze {
                                    // The scan has finished, so this starts once `active` clears.
                                    workspace.scan.active = None;
                                    workspace.start_analysis(saved, cx);
                                }
                            }
                            Err(error) => {
                                workspace.scan.error =
                                    Some(format!("Could not scan folder: {error}"));
                                workspace.show_page(Page::Overview);
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

/// `number` and `noun`, with the noun made plural unless the number is one.
pub(super) fn count(number: u64, noun: &str) -> String {
    if number == 1 {
        format!("1 {noun}")
    } else {
        format!("{number} {noun}s")
    }
}
