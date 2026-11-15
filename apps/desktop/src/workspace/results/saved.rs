use std::time::Duration;

use gpui_kit::{AppContext as _, Context, Task};
use scan_engine::FileRecord;
use scan_store::SavedScan;

use super::super::{Workspace, storage};
use super::{FileRows, Results};

pub(super) const PAGE_SIZE: u32 = 100;
const SEARCH_DELAY: Duration = Duration::from_millis(175);

pub(super) struct SavedResults {
    pub scan: SavedScan,
    pub files: Vec<FileRecord>,
    pub offset: u64,
    pub has_next: bool,
    pub loading: bool,
    pub error: Option<String>,
    request: Option<Task<()>>,
}

impl Results {
    pub(in crate::workspace) fn open_saved(
        &mut self,
        scan: SavedScan,
        cx: &mut Context<Workspace>,
    ) {
        self.saved = Some(SavedResults {
            scan,
            files: Vec::new(),
            offset: 0,
            has_next: false,
            loading: true,
            error: None,
            request: None,
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
        saved.has_next = false;
        let scan_id = saved.scan.id;
        let query = self.input.read(cx).value().to_string();
        let executor = cx.background_executor().clone();
        saved.request = Some(cx.spawn(async move |workspace, cx| {
            if debounce {
                executor.timer(SEARCH_DELAY).await;
            }
            let result = cx
                .background_spawn(async move {
                    storage::open()?
                        .files(scan_id, &query, PAGE_SIZE + 1, offset)
                        .map_err(std::io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let results = &mut workspace.results;
                let Some(saved) = &mut results.saved else {
                    return;
                };
                saved.loading = false;
                match result {
                    Ok(mut files) => {
                        saved.has_next = files.len() > PAGE_SIZE as usize;
                        files.truncate(PAGE_SIZE as usize);
                        saved.files = files;
                    }
                    Err(error) => saved.error = Some(format!("Could not load scan files: {error}")),
                }
                let rows = FileRows::from_files(&saved.files, saved.scan.large_file_threshold, "");
                results.replace_rows(rows, cx);
                cx.notify();
            });
        }));
        self.replace_rows(FileRows::from_files(&[], 0, ""), cx);
        cx.notify();
    }
}
