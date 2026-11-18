use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cleanup_engine::NativeTrash;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, button::Button};
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, div, px,
};
use scan_store::{CleanupItem, CleanupState, SavedScan};

use super::{Page, Workspace, scan::format_bytes, storage};

mod job;
#[cfg(test)]
mod tests;

const HISTORY_PAGE: u32 = 30;

#[derive(Default)]
pub(super) struct Cleanup {
    queue: Vec<CleanupItem>,
    history: Vec<CleanupItem>,
    history_offset: u64,
    has_next: bool,
    pub(super) busy: bool,
    confirm: bool,
    restore_confirm: Option<i64>,
    status: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    request: Option<Task<()>>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Cleanup {
    pub(super) fn load(&mut self, offset: u64, cx: &mut Context<Workspace>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.confirm = false;
        self.restore_confirm = None;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut store = storage::open()?;
                    // An active process owns the lock; only abandoned intents are reconciled.
                    match store.cleanup_session() {
                        Ok(session) => drop(session),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(error) => return Err(error),
                    }
                    Ok::<_, std::io::Error>((
                        store.cleanup_items(true, 200, 0)?,
                        store.cleanup_items(false, HISTORY_PAGE + 1, offset)?,
                    ))
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let cleanup = &mut workspace.cleanup;
                cleanup.busy = false;
                match result {
                    Ok((queue, mut history)) => {
                        cleanup.queue = queue;
                        cleanup.has_next = history.len() > HISTORY_PAGE as usize;
                        history.truncate(HISTORY_PAGE as usize);
                        cleanup.history = history;
                        cleanup.history_offset = offset;
                    }
                    Err(error) => {
                        cleanup.status = Some(format!("Could not load cleanup history: {error}"))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render(&self, history: bool, scan_active: bool, cx: &Context<Workspace>) -> Div {
        let muted = cx.theme().muted_foreground;
        let mut page = div().flex().flex_col().size_full().gap_3().p_6()
            .child(div().flex().justify_between().items_center()
                .child(div().text_xl().font_semibold().child(if history { "Recovery History" } else { "Cleanup Queue" }))
                .child(Button::new("refresh-cleanup").small().label("Refresh").disabled(self.busy)
                    .on_click(cx.listener(|workspace, _, _, cx| { let offset = workspace.cleanup.history_offset; workspace.cleanup.load(offset, cx); }))))
            .child(div().text_sm().text_color(muted).child("Scans are read-only. Only explicitly confirmed queue items move to OS Trash / Recycle Bin. Protected items and hard links are blocked. Restore is available until the OS trash is emptied."))
            .children(self.status.as_ref().map(|status| div().text_sm().child(status.clone())))
            .children(self.busy.then(|| div().text_sm().child("Working in the background…")))
            .children(self.cancel.as_ref().map(|_| Button::new("cancel-cleanup").small().label("Stop after current action")
                .on_click(cx.listener(|workspace, _, _, cx| { if let Some(cancel) = &workspace.cleanup.cancel { cancel.store(true, Ordering::Relaxed); } workspace.cleanup.status = Some("Cancellation requested; the current native action will be recorded.".into()); cx.notify(); })).into_any_element()));
        if !history {
            let bytes = self.queue.iter().fold(0_u64, |sum, item| {
                sum.saturating_add(item.entry.proof.logical_bytes)
            });
            let allocated = self.queue.iter().try_fold(0_u64, |sum, item| {
                sum.checked_add(item.entry.proof.allocated_bytes?)
            });
            page = page.child(div().text_sm().child(format!("{} items · {} logical size · immediate space release expected: 0 B", self.queue.len(), format_bytes(bytes))))
                .child(div().text_sm().text_color(muted).child(format!("Potential allocation upper bound after OS trash is emptied: {}. Shared clones, compression, sparse files, metadata and cloud storage make actual reclamation uncertain. This app never empties Trash.", allocated.map(format_bytes).unwrap_or_else(|| "unknown".into()))));
            if self.confirm {
                page = page.child(div().flex().flex_col().gap_2().p_3().border_1().border_color(cx.theme().border)
                    .child(div().font_semibold().child(format!("Confirm moving these {} reviewed items?", self.queue.len())))
                    .child(div().text_sm().child("Each listed original path will disappear. Keep paths remain in place. Close applications using these files. Recovery depends on the OS Trash and the saved receipts; interrupted actions may require manual inspection."))
                    .child(Button::new("confirm-cleanup").label("Confirm move to Trash / Recycle Bin").disabled(self.busy || scan_active)
                        .on_click(cx.listener(|workspace, _, _, cx| workspace.execute_cleanup(cx))))
                    .child(Button::new("cancel-confirm-cleanup").small().label("Back to review").on_click(cx.listener(|workspace, _, _, cx| { workspace.cleanup.confirm = false; cx.notify(); }))));
            } else {
                page = page.child(
                    Button::new("review-cleanup-confirmation")
                        .label("Review confirmation")
                        .disabled(
                            self.busy
                                || scan_active
                                || self.queue.is_empty()
                                || !NativeTrash::supported(),
                        )
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.cleanup.confirm = true;
                            cx.notify();
                        })),
                );
            }
        }
        let items = if history { &self.history } else { &self.queue };
        let rows = items.iter().map(|item| {
            let id = item.id;
            let mut row = div().flex().flex_col().gap_2().p_3().border_b_1().border_color(cx.theme().border)
                .child(div().font_semibold().child(item.entry.source.display().to_string()))
                .child(div().text_sm().child(format!("{} · {} logical · scan {}", item.state.label(), format_bytes(item.entry.proof.logical_bytes), item.entry.scan_id)))
                .child(div().text_sm().text_color(muted).child(item.entry.reason.clone()))
                .child(div().text_sm().child(item.entry.keeper.as_ref().map_or_else(|| "No duplicate keeper: this is a metadata candidate or empty directory.".into(), |(path, _)| format!("Keep this copy: {}", path.display()))))
                .children((!item.message.is_empty()).then(|| div().text_sm().child(item.message.clone())));
            if let (Some(before), Some(after)) = (item.available_before, item.available_after) {
                row = row.child(div().text_xs().text_color(muted).child(format!("Observed volume available-space change: {:+} bytes (not attributable to cleanup alone).", i128::from(after) - i128::from(before))));
            }
            if !history {
                row = row.child(Button::new(("remove-queued", id as u64)).small().label("Remove from queue").disabled(self.busy)
                    .on_click(cx.listener(move |workspace, _, _, cx| workspace.remove_cleanup(id, cx))));
            } else if let Some(receipt) = &item.receipt {
                row = row.child(div().text_xs().text_color(muted).child(format!("Recovery location: {}", receipt.path.display())));
                if matches!(item.state, CleanupState::Trashed | CleanupState::Attention) {
                    let confirming = self.restore_confirm == Some(id);
                    row = row.child(Button::new(("restore-cleanup", id as u64)).small()
                        .label(if confirming { "Confirm restore to original path" } else { "Review restore" }).disabled(self.busy || scan_active)
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            if workspace.cleanup.restore_confirm == Some(id) { workspace.restore_cleanup(id, cx); }
                            else { workspace.cleanup.restore_confirm = Some(id); cx.notify(); }
                        })));
                    if confirming {
                        row = row.child(div().text_sm().child("Restore the item to the original path shown above. An existing destination will not be overwritten."))
                            .child(Button::new(("cancel-restore", id as u64)).small().label("Cancel restore")
                                .on_click(cx.listener(|workspace, _, _, cx| { workspace.cleanup.restore_confirm = None; cx.notify(); })));
                    }
                }
            }
            if history && matches!(item.state, CleanupState::Attention | CleanupState::Trashed) {
                row = row.child(Button::new(("reconcile-cleanup", id as u64)).small().label("Verify original location").disabled(self.busy || scan_active)
                    .on_click(cx.listener(move |workspace, _, _, cx| workspace.reconcile_cleanup(id, cx))));
            }
            row
        });
        page = page.child(div().id("cleanup-items").flex_1().min_h(px(0.)).overflow_y_scroll().children(rows)
            .children(items.is_empty().then(|| div().text_sm().child(if history { "No recovery history yet." } else { "Add files or empty folders from Review. For exact duplicates, select a kept copy first." }))));
        if history {
            let previous = self.history_offset.saturating_sub(u64::from(HISTORY_PAGE));
            let next = self.history_offset + u64::from(HISTORY_PAGE);
            page = page.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("previous-recovery")
                            .small()
                            .label("Previous")
                            .disabled(self.busy || self.history_offset == 0)
                            .on_click(cx.listener(move |w, _, _, cx| w.cleanup.load(previous, cx))),
                    )
                    .child(
                        Button::new("next-recovery")
                            .small()
                            .label("Next")
                            .disabled(self.busy || !self.has_next)
                            .on_click(cx.listener(move |w, _, _, cx| w.cleanup.load(next, cx))),
                    ),
            );
        }
        page
    }
}

impl Workspace {
    pub(super) fn queue_cleanup(
        &mut self,
        scan: SavedScan,
        source: PathBuf,
        keeper: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.cleanup.busy
            || self.scan.active.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy()
        {
            return;
        }
        self.page = Page::Cleanup;
        self.cleanup.busy = true;
        self.cleanup.confirm = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.cancel = Some(cancel.clone());
        self.cleanup.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut store = storage::open()?;
                    let entry = job::prepare(&store, &scan, &source, keeper.as_deref(), &cancel)?;
                    cleanup_engine::cancelled(&cancel)?;
                    store.cleanup_session()?.enqueue(&entry)?;
                    Ok::<_, std::io::Error>(
                        "Added to the review queue. No file was moved.".to_string(),
                    )
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| workspace.finish_cleanup(result, cx));
        }));
        cx.notify();
    }

    fn remove_cleanup(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.cleanup.busy {
            return;
        }
        self.cleanup.busy = true;
        self.cleanup.confirm = false;
        self.cleanup.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    storage::open()?.cleanup_session()?.remove(id)?;
                    Ok::<_, std::io::Error>("Removed from queue; source unchanged.".into())
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| workspace.finish_cleanup(result, cx));
        }));
    }

    fn execute_cleanup(&mut self, cx: &mut Context<Self>) {
        if !self.cleanup.confirm
            || self.cleanup.busy
            || self.scan.active.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy()
        {
            return;
        }
        let ids: Vec<_> = self.cleanup.queue.iter().map(|item| item.id).collect();
        self.cleanup.busy = true;
        self.cleanup.confirm = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.cancel = Some(cancel.clone());
        self.cleanup.status = Some("Revalidating the confirmed queue…".into());
        self.cleanup.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = job::execute(
                        &mut storage::open()?,
                        &ids,
                        &NativeTrash,
                        &cancel,
                        |_, _| {},
                    )?;
                    Ok(format!(
                        "{} moved; {} failed or need inspection. {}",
                        result.moved,
                        result.failed,
                        if result.cancelled {
                            "Cancelled items remain queued."
                        } else {
                            "See Recovery History for each outcome."
                        }
                    ))
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.page = Page::Recovery;
                workspace.finish_cleanup(result, cx);
            });
        }));
        cx.notify();
    }

    fn restore_cleanup(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.cleanup.busy
            || self.scan.active.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy()
            || self.cleanup.restore_confirm != Some(id)
        {
            return;
        }
        self.cleanup.busy = true;
        self.cleanup.restore_confirm = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.cancel = Some(cancel.clone());
        self.cleanup.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    job::restore(&mut storage::open()?, id, &NativeTrash, &cancel)?;
                    Ok("Restored and verified at the original path.".into())
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| workspace.finish_cleanup(result, cx));
        }));
        cx.notify();
    }

    fn finish_cleanup(&mut self, result: std::io::Result<String>, cx: &mut Context<Self>) {
        self.cleanup.busy = false;
        self.cleanup.cancel = None;
        self.cleanup.status =
            Some(result.unwrap_or_else(|error| format!("Cleanup / recovery: {error}")));
        self.cleanup.load(0, cx);
        cx.notify();
    }

    fn reconcile_cleanup(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.cleanup.busy
            || self.scan.active.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy()
        {
            return;
        }
        self.cleanup.busy = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cleanup.cancel = Some(cancel.clone());
        self.cleanup.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    job::reconcile(&mut storage::open()?, id, &cancel)?;
                    Ok("Original location verified. No file was changed.".into())
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| workspace.finish_cleanup(result, cx));
        }));
        cx.notify();
    }
}
