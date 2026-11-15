use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, Task};
use scan_engine::FileRecord;
use scan_store::{DuplicateEvidence, FileTypeTotal, SavedScan};

use super::super::{Workspace, storage};
use super::{FileRows, Results};

pub(super) const PAGE_SIZE: u32 = 100;
const SEARCH_DELAY: Duration = Duration::from_millis(175);

pub(super) struct SavedResults {
    pub scan: SavedScan,
    pub files: Vec<FileRecord>,
    pub evidence: HashMap<PathBuf, DuplicateEvidence>,
    pub duplicates_only: bool,
    pub offset: u64,
    pub has_next: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// `None` while the totals load; they cover the whole saved scan, not the current page.
    pub file_types: Option<Result<Vec<FileTypeTotal>, String>>,
    pub export_status: Option<String>,
    request: Option<Task<()>>,
    _file_types_request: Task<()>,
}

impl Results {
    pub(in crate::workspace) fn open_saved(
        &mut self,
        scan: SavedScan,
        cx: &mut Context<Workspace>,
    ) {
        let scan_id = scan.id;
        let file_types_request = cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    storage::open()?
                        .file_types(scan_id)
                        .map_err(std::io::Error::other)
                })
                .await
                .map_err(|error| format!("Could not load file types: {error}"));
            let _ = workspace.update(cx, |workspace, cx| {
                if let Some(saved) = &mut workspace.results.saved
                    && saved.scan.id == scan_id
                {
                    saved.file_types = Some(result);
                    cx.notify();
                }
            });
        });
        self.saved = Some(SavedResults {
            scan,
            files: Vec::new(),
            evidence: HashMap::new(),
            duplicates_only: false,
            offset: 0,
            has_next: false,
            loading: true,
            error: None,
            file_types: None,
            export_status: None,
            request: None,
            _file_types_request: file_types_request,
        });
        self.load_page(0, false, cx);
    }

    pub(super) fn load_page(&mut self, offset: u64, debounce: bool, cx: &mut Context<Workspace>) {
        let Some(saved) = &mut self.saved else { return };
        saved.request.take();
        saved.offset = offset;
        saved.loading = true;
        saved.error = None;
        saved.files.clear();
        saved.evidence.clear();
        saved.has_next = false;
        let scan_id = saved.scan.id;
        let duplicates_only = saved.duplicates_only;
        let query = self.input.read(cx).value().to_string();
        let executor = cx.background_executor().clone();
        saved.request = Some(cx.spawn(async move |workspace, cx| {
            if debounce {
                executor.timer(SEARCH_DELAY).await;
            }
            let result = cx
                .background_spawn(async move {
                    let store = storage::open()?;
                    let mut files = if duplicates_only {
                        store.duplicate_files(scan_id, &query, PAGE_SIZE + 1, offset)
                    } else {
                        store.files(scan_id, &query, PAGE_SIZE + 1, offset)
                    }
                    .map_err(std::io::Error::other)?;
                    let has_next = files.len() > PAGE_SIZE as usize;
                    files.truncate(PAGE_SIZE as usize);
                    let evidence = store
                        .duplicate_evidence(scan_id, &files)
                        .map_err(std::io::Error::other)?;
                    Ok::<_, std::io::Error>((files, evidence, has_next))
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let results = &mut workspace.results;
                let Some(saved) = &mut results.saved else {
                    return;
                };
                saved.loading = false;
                match result {
                    Ok((files, evidence, has_next)) => {
                        saved.has_next = has_next;
                        saved.files = files;
                        saved.evidence = evidence;
                    }
                    Err(error) => saved.error = Some(format!("Could not load scan files: {error}")),
                }
                let rows = FileRows::from_files(
                    &saved.files,
                    saved.scan.large_file_threshold,
                    "",
                    Some(&saved.evidence),
                );
                results.replace_rows(rows, cx);
                cx.notify();
            });
        }));
        self.replace_rows(FileRows::from_files(&[], 0, "", None), cx);
        cx.notify();
    }
}
