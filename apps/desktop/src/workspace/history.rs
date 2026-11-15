use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, div, px,
};
use scan_store::SavedScan;

use super::{Page, Workspace, scan::format_bytes, storage};

const PAGE_SIZE: u32 = 20;

#[derive(Default)]
pub(super) struct History {
    scans: Vec<SavedScan>,
    offset: u64,
    has_next: bool,
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
}

impl History {
    pub(super) fn load(&mut self, offset: u64, cx: &mut Context<Workspace>) {
        self.request.take();
        self.loading = true;
        self.error = None;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    storage::open()?
                        .scans(PAGE_SIZE + 1, offset)
                        .map_err(std::io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let history = &mut workspace.history;
                history.loading = false;
                match result {
                    Ok(mut scans) => {
                        history.has_next = scans.len() > PAGE_SIZE as usize;
                        scans.truncate(PAGE_SIZE as usize);
                        history.scans = scans;
                        history.offset = offset;
                    }
                    Err(error) => history.error = Some(format!("Could not load history: {error}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render(&self, cx: &Context<Workspace>) -> Div {
        let muted = cx.theme().muted_foreground;
        let previous = self.offset.saturating_sub(u64::from(PAGE_SIZE));
        let next = self.offset + u64::from(PAGE_SIZE);
        let status = if self.loading {
            Some("Loading scans…")
        } else if let Some(error) = self.error.as_deref() {
            Some(error)
        } else if self.scans.is_empty() {
            Some("No saved scans yet")
        } else {
            None
        };
        let rows = self.scans.iter().map(|scan| {
            let date = jiff::Timestamp::from_millisecond(scan.started_unix_ms)
                .map(|started| started.strftime("%F %H:%M UTC").to_string())
                .unwrap_or_else(|_| "Date unavailable".into());
            let status = if scan.cancelled {
                "Cancelled"
            } else {
                "Completed"
            };
            let scan_to_open = scan.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .p_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .gap_1()
                        .child(div().text_ellipsis().child(scan.root.display().to_string()))
                        .child(div().text_sm().text_color(muted).child(format!(
                            "{date} · {status} · {} files · {} logical size",
                            scan.progress.files,
                            format_bytes(scan.progress.logical_bytes),
                        ))),
                )
                .child(
                    Button::new(("open-saved-scan", scan.id as u64))
                        .label("Open")
                        .small()
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.results.open_saved(scan_to_open.clone(), cx);
                            workspace.page = Page::Results;
                            cx.notify();
                        })),
                )
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_3()
            .p_6()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_xl().font_semibold().child("Scan History"))
                    .child(
                        Button::new("refresh-history")
                            .ghost()
                            .small()
                            .label("Refresh")
                            .disabled(self.loading)
                            .on_click(
                                cx.listener(|workspace, _, _, cx| workspace.history.load(0, cx)),
                            ),
                    ),
            )
            .children(
                status.map(|status| div().text_sm().text_color(muted).child(status.to_owned())),
            )
            .child(
                div()
                    .id("saved-scans")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .children((self.offset > 0 || self.has_next).then(|| {
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("previous-scans")
                            .small()
                            .label("Previous")
                            .disabled(self.loading || self.offset == 0)
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.history.load(previous, cx)
                            })),
                    )
                    .child(
                        Button::new("next-scans")
                            .small()
                            .label("Next")
                            .disabled(self.loading || !self.has_next)
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.history.load(next, cx)
                            })),
                    )
            }))
    }
}
