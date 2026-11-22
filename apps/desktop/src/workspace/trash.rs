//! Move to Trash from Files, Tree and Space Map. The item is inspected first; a folder's
//! contents are listed so you can see what goes with it. One confirmation then records the
//! item in the cleanup journal and moves it, so it appears in Recovery History for restore.

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cleanup_engine::{FolderContents, NativeTrash};
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, div, hsla, px,
};
use scan_engine::assess_risk;
use scan_store::{QueueEntry, SavedScan};

use super::{Page, Workspace, cleanup, scan::format_bytes, storage};

enum Stage {
    Inspecting,
    Ready {
        entry: Box<QueueEntry>,
        contents: Option<FolderContents>,
    },
    Moving,
    Finished {
        moved: bool,
        message: String,
    },
}

pub(super) struct TrashPrompt {
    path: PathBuf,
    stage: Stage,
    cancel: Arc<AtomicBool>,
    _task: Option<Task<()>>,
}

impl Drop for TrashPrompt {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Workspace {
    /// Opens the Move to Trash dialog for `path` and starts inspecting it in the background.
    pub(super) fn ask_trash(&mut self, scan: SavedScan, path: PathBuf, cx: &mut Context<Self>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let blocked = self.cleanup.busy
            || self.scan.active.is_some()
            || self.scan.analysis.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy();
        let mut prompt = TrashPrompt {
            path: path.clone(),
            stage: Stage::Inspecting,
            cancel: Arc::clone(&cancel),
            _task: None,
        };
        if blocked {
            prompt.stage = Stage::Finished {
                moved: false,
                message:
                    "Wait for the current scan, analysis or cleanup to finish, then try again."
                        .into(),
            };
        } else {
            prompt._task = Some(cx.spawn(async move |workspace, cx| {
                let result = cx
                    .background_spawn(async move {
                        cleanup::job::prepare_chosen(&storage::open()?, &scan, &path, &cancel)
                    })
                    .await;
                let _ = workspace.update(cx, |workspace, cx| {
                    if let Some(prompt) = &mut workspace.trash {
                        prompt.stage = match result {
                            Ok((entry, contents)) => Stage::Ready {
                                entry: Box::new(entry),
                                contents,
                            },
                            Err(error) => Stage::Finished {
                                moved: false,
                                message: format!("Can't move this item: {error}"),
                            },
                        };
                        cx.notify();
                    }
                });
            }));
        }
        self.trash = Some(prompt);
        cx.notify();
    }

    fn confirm_trash(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = &mut self.trash else {
            return;
        };
        let Stage::Ready { entry, .. } = std::mem::replace(&mut prompt.stage, Stage::Moving) else {
            return;
        };
        let cancel = Arc::clone(&prompt.cancel);
        self.cleanup.busy = true;
        prompt._task = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut store = storage::open()?;
                    let outcome =
                        cleanup::job::execute_chosen(&mut store, &entry, &NativeTrash, &cancel)?;
                    if outcome.moved == 1 {
                        return Ok((true, "Moved to Trash. Restore it from Recovery History; scan again to refresh sizes.".to_string()));
                    }
                    // The journal holds the reason the move was refused.
                    let reason = store
                        .cleanup_items(false, 1, 0)?
                        .into_iter()
                        .next()
                        .map_or_else(String::new, |item| item.message);
                    Ok::<_, std::io::Error>((false, format!("Not moved. {reason}")))
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.cleanup.busy = false;
                workspace.cleanup.load(0, cx);
                if let Some(prompt) = &mut workspace.trash {
                    let (moved, message) =
                        result.unwrap_or_else(|error| (false, format!("Not moved: {error}")));
                    prompt.stage = Stage::Finished { moved, message };
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The previewed folder's file count, or the dialog's message when it is not ready.
    #[cfg(feature = "ui-smoke")]
    pub(super) fn trash_preview(&self) -> Result<Option<u64>, String> {
        match self.trash.as_ref().map(|prompt| &prompt.stage) {
            Some(Stage::Ready { contents, .. }) => {
                Ok(contents.as_ref().map(|contents| contents.files))
            }
            Some(Stage::Finished { message, .. }) => Err(message.clone()),
            _ => Err("not ready".into()),
        }
    }

    /// The dialog over the whole window, or nothing when it is closed.
    pub(super) fn trash_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let prompt = self.trash.as_ref()?;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let name = prompt.path.file_name().map_or_else(
            || prompt.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let fact = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(96.0))
                .p_2()
                .rounded(px(4.0))
                .bg(theme.muted)
                .child(div().text_xs().text_color(muted).child(label))
                .child(div().font_semibold().child(value))
        };
        let close = Button::new("trash-close")
            .outline()
            .label("Cancel")
            .on_click(cx.listener(|workspace, _, _, cx| {
                workspace.trash = None;
                cx.notify();
            }));

        let mut body = div().flex().flex_col().gap_3();
        let mut actions = div().flex().flex_wrap().justify_end().gap_2();
        match &prompt.stage {
            Stage::Inspecting => {
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("Checking the item and listing what is inside…"),
                );
                actions = actions.child(close);
            }
            Stage::Moving => {
                body = body.child(div().text_sm().child("Moving to Trash…"));
            }
            Stage::Finished { moved, message } => {
                let moved = *moved;
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .text_sm()
                        .child(
                            Icon::new(if moved {
                                IconName::CircleCheck
                            } else {
                                IconName::TriangleAlert
                            })
                            .text_color(theme.primary),
                        )
                        .child(div().flex_1().min_w(px(0.0)).child(message.clone())),
                );
                if moved {
                    actions = actions.child(
                        Button::new("trash-open-recovery")
                            .outline()
                            .label("Open Recovery History")
                            .on_click(cx.listener(|workspace, _, _, cx| {
                                workspace.trash = None;
                                workspace.show_page(Page::Recovery);
                                workspace.load_page(Page::Recovery, cx);
                            })),
                    );
                }
                actions =
                    actions.child(Button::new("trash-done").primary().label("Close").on_click(
                        cx.listener(|workspace, _, _, cx| {
                            workspace.trash = None;
                            cx.notify();
                        }),
                    ));
            }
            Stage::Ready { entry, contents } => {
                let risk = assess_risk(&entry.source, false);
                let mut facts = div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(fact("Size", format_bytes(entry.proof.logical_bytes)));
                if let Some(contents) = contents {
                    facts = facts
                        .child(fact("Files", contents.files.to_string()))
                        .child(fact("Folders", contents.folders.to_string()));
                } else {
                    facts = facts.child(fact(
                        "Kind",
                        if entry.proof.directory {
                            "Empty folder"
                        } else {
                            "File"
                        }
                        .into(),
                    ));
                }
                body = body.child(facts).child(
                    div()
                        .text_sm()
                        .child(div().font_semibold().child(risk.tier.label()))
                        .child(div().text_color(muted).child(risk.reason)),
                );
                if let Some(contents) = contents {
                    let mut largest = div()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(theme.border)
                        .rounded(px(4.0))
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .bg(theme.muted)
                                .child("Largest files inside"),
                        );
                    for (path, bytes) in &contents.largest {
                        let name = path.file_name().unwrap_or(path.as_os_str());
                        largest = largest.child(
                            div()
                                .flex()
                                .items_start()
                                .gap_2()
                                .px_2()
                                .py_1()
                                .border_t_1()
                                .border_color(theme.border)
                                .text_xs()
                                .child(
                                    gpui_kit::img(crate::file_icons::icon_path(name, false))
                                        .size(px(14.0))
                                        .flex_none(),
                                )
                                // Paths wrap so deep folders stay readable.
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .child(path.display().to_string()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(muted)
                                        .child(format_bytes(*bytes)),
                                ),
                        );
                    }
                    body = body.child(largest);
                    let mut notes = Vec::new();
                    if contents.links > 0 {
                        notes.push(format!(
                            "{} links move as links; the files they point to stay.",
                            contents.links
                        ));
                    }
                    if contents.read_only > 0 {
                        notes.push(format!("{} read-only items inside.", contents.read_only));
                    }
                    body = body.children(
                        notes
                            .into_iter()
                            .map(|note| div().text_xs().text_color(muted).child(note)),
                    );
                }
                body = body.child(div().text_xs().text_color(muted).child(
                    "It goes to the Trash / Recycle Bin and is listed in Recovery History, where you can restore it. If anything here changes before it moves, nothing is moved.",
                ));
                actions = actions.child(close).child(
                    Button::new("trash-confirm")
                        .danger()
                        .icon(Icon::new(IconName::Trash))
                        .label("Move to Trash")
                        .on_click(cx.listener(|workspace, _, _, cx| workspace.confirm_trash(cx))),
                );
            }
        }

        let is_folder =
            matches!(&prompt.stage, Stage::Ready { entry, .. } if entry.proof.directory);
        let card = div()
            .id("trash-dialog")
            .flex()
            .flex_col()
            .gap_3()
            .w(px(560.0))
            .max_w(gpui_kit::relative(0.92))
            .max_h(gpui_kit::relative(0.85))
            .p_5()
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_lg()
                    .font_semibold()
                    .child(Icon::new(IconName::Trash).text_color(theme.primary))
                    .child("Move to Trash?"),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        gpui_kit::img(crate::file_icons::icon_path(
                            prompt.path.file_name().unwrap_or(prompt.path.as_os_str()),
                            is_folder,
                        ))
                        .size(px(20.0))
                        .flex_none(),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(div().font_semibold().child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(prompt.path.display().to_string()),
                            ),
                    ),
            )
            .child(body)
            .child(actions);

        Some(
            div()
                .id("trash-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0.0, 0.0, 0.0, 0.6))
                .occlude()
                .child(card)
                .into_any_element(),
        )
    }
}
