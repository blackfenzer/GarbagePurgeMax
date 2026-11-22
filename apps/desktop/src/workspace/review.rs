use std::io;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::SystemTime;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, button::Button,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Div, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Task, div, img, px,
};
use scan_engine::{
    CleanupCandidate, CleanupKind, DirectoryRecord, FileRecord, RiskAssessment, RiskTier,
    SIMILAR_IMAGE_DISTANCE, assess_risk, image_preview, is_hashable_image,
};
use scan_store::{DuplicateEvidence, ReviewGroup, SavedScan, SimilarImageEvidence};

use super::{
    Page, Workspace,
    scan::{count, format_bytes},
    storage,
};

mod status;

const PAGE_SIZE: u32 = 30;
/// Cards shown for one group. Candidate groups can hold thousands of files.
const MAX_CARDS: usize = 200;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ReviewKind {
    #[default]
    ExactDuplicates,
    SimilarImages,
    CleanupCandidates,
    EmptyFolders,
}

type GroupsAndCandidates = (Vec<ReviewGroup>, Vec<Vec<Member>>, Vec<DirectoryRecord>);

/// A group member and the saved evidence for its match.
struct Member {
    file: FileRecord,
    evidence: Option<Evidence>,
    preview: Option<Arc<Image>>,
    preview_error: Option<String>,
}

impl Member {
    fn new(file: FileRecord, evidence: Option<Evidence>) -> Self {
        Self {
            file,
            evidence,
            preview: None,
            preview_error: None,
        }
    }
}

struct PreviewRequest(Arc<AtomicBool>);
impl Drop for PreviewRequest {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

enum Evidence {
    Exact(DuplicateEvidence),
    Similar(SimilarImageEvidence),
    Cleanup(CleanupCandidate),
}

/// Groups for the scan open in Scan Results. Exact, similar, and cleanup groups stay in separate tabs.
#[derive(Default)]
pub(super) struct Review {
    scan: Option<SavedScan>,
    kind: ReviewKind,
    groups: Vec<ReviewGroup>,
    offset: u64,
    has_next: bool,
    selected_group: Option<u64>,
    members: Vec<Member>,
    /// Cleanup candidates by `CleanupKind::ALL` position. Group ids are positions plus one.
    candidate_groups: Vec<Vec<Member>>,
    empty_folders: Vec<DirectoryRecord>,
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
    preview_request: Option<PreviewRequest>,
    keeper: Option<PathBuf>,
    /// The last Queue result, shown here so several items can be queued without leaving.
    pub(super) queue_note: Option<String>,
}

impl Review {
    pub(super) fn open(&mut self, scan: Option<SavedScan>, cx: &mut Context<Workspace>) {
        let same_scan = self.scan.as_ref().map(|scan| scan.id) == scan.as_ref().map(|scan| scan.id);
        self.scan = scan;
        if !same_scan || self.groups.is_empty() {
            self.load_groups(0, cx);
        }
    }

    pub(super) fn show(&mut self, kind: ReviewKind, cx: &mut Context<Workspace>) {
        if self.kind != kind {
            self.kind = kind;
            self.load_groups(0, cx);
        }
    }

    fn load_groups(&mut self, offset: u64, cx: &mut Context<Workspace>) {
        self.request.take();
        self.preview_request.take();
        self.groups.clear();
        self.members.clear();
        self.candidate_groups.clear();
        self.empty_folders.clear();
        self.offset = offset;
        self.has_next = false;
        self.selected_group = None;
        self.keeper = None;
        self.error = None;
        let Some(scan) = &self.scan else {
            self.loading = false;
            cx.notify();
            return;
        };
        let scan_id = scan.id;
        let scanned_at = scan.started_at();
        self.loading = true;
        let kind = self.kind;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    load_groups(kind, scan_id, offset, scanned_at?).map_err(io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let review = &mut workspace.review;
                review.loading = false;
                match result {
                    Ok((mut groups, candidate_groups, mut folders)) => {
                        review.candidate_groups = candidate_groups;
                        review.has_next =
                            groups.len() > PAGE_SIZE as usize || folders.len() > PAGE_SIZE as usize;
                        folders.truncate(PAGE_SIZE as usize);
                        review.empty_folders = folders;
                        groups.truncate(PAGE_SIZE as usize);
                        review.offset = offset;
                        let first = groups.first().map(|group| group.group_id);
                        review.groups = groups;
                        if let Some(group_id) = first {
                            review.select_group(group_id, cx);
                        }
                    }
                    Err(error) => review.error = Some(format!("Could not load groups: {error}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_group(&mut self, group_id: u64, cx: &mut Context<Workspace>) {
        self.keeper = None;
        let Some(scan_id) = self.scan.as_ref().map(|scan| scan.id) else {
            return;
        };
        self.error = None;
        if matches!(
            self.kind,
            ReviewKind::CleanupCandidates | ReviewKind::EmptyFolders
        ) {
            // Candidates arrive with their groups, so selection needs no query.
            self.selected_group = Some(group_id);
            cx.notify();
            return;
        }
        self.request.take();
        self.preview_request.take();
        self.selected_group = Some(group_id);
        self.members.clear();
        self.loading = true;
        let kind = self.kind;
        let cancel = Arc::new(AtomicBool::new(false));
        self.preview_request = Some(PreviewRequest(Arc::clone(&cancel)));
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    load_members(kind, scan_id, group_id, &cancel).map_err(io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let review = &mut workspace.review;
                if review.selected_group != Some(group_id)
                    || review.kind != kind
                    || review.scan.as_ref().map(|scan| scan.id) != Some(scan_id)
                {
                    return;
                }
                review.loading = false;
                match result {
                    Ok(members) => review.members = members,
                    Err(error) => review.error = Some(format!("Could not load group: {error}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render(&self, cx: &Context<Workspace>) -> Div {
        let muted = cx.theme().muted_foreground;
        let page = div().flex().flex_col().size_full().gap_3().p_6().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_xl().font_semibold().child("Review"))
                .child(
                    Button::new("refresh-review")
                        .small()
                        .label("Refresh")
                        .disabled(self.loading || self.scan.is_none())
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            let offset = workspace.review.offset;
                            workspace.review.load_groups(offset, cx);
                        })),
                ),
        );
        let Some(scan) = &self.scan else {
            return page.child(div().text_sm().text_color(muted).child(
                "Run a scan, or open one from Scan History, to review its duplicate and similar-image groups.",
            ));
        };

        let explanation = match self.kind {
            ReviewKind::ExactDuplicates => {
                "Files in each group had byte-identical contents when scanned, verified with SHA-256 and a full comparison. Pick the copy to keep, then queue the others.".to_string()
            },
            ReviewKind::SimilarImages => {
                format!(
                    "Images whose difference hash is within {SIMILAR_IMAGE_DISTANCE} of 64 bits of the group's reference image. This is a heuristic match, not a verified copy. Compare the images before acting.",
                )
            },
            ReviewKind::EmptyFolders => "Folders observed with no entries, including hidden entries. Emptiness is checked again before anything moves.".to_string(),
            ReviewKind::CleanupCandidates => {
                "Metadata candidates need your review. Queue the ones you want gone; nothing moves until you confirm the queue. Protected items stay blocked.".to_string()
            },
        };
        let status = if self.loading {
            Some("Loading…".to_string())
        } else if let Some(error) = &self.error {
            Some(error.clone())
        } else if self.groups.is_empty() {
            Some(status::empty_message(self.kind, scan).into())
        } else {
            None
        };

        let tab = |id: &'static str, label: String, kind: ReviewKind| {
            Button::new(id)
                .small()
                .label(label)
                .selected(self.kind == kind)
                .on_click(cx.listener(move |workspace, _, _, cx| workspace.review.show(kind, cx)))
        };
        let body = if self.kind == ReviewKind::EmptyFolders {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .gap_2()
                .child(self.empty_folders(cx))
                .children(self.pagination(cx))
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .gap_3()
                .child(self.group_list(cx))
                .child(self.group_detail(cx))
        };

        page.child(
            div()
                .text_sm()
                .text_color(muted)
                .line_clamp(2)
                .child(scan.root.display().to_string()),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(tab(
                    "exact-duplicates",
                    format!("Exact duplicates · {}", scan.duplicate_group_count),
                    ReviewKind::ExactDuplicates,
                ))
                .child(tab(
                    "similar-images",
                    format!("Similar images · {}", scan.similar_group_count),
                    ReviewKind::SimilarImages,
                ))
                .child(tab(
                    "cleanup-candidates",
                    "Cleanup candidates".into(),
                    ReviewKind::CleanupCandidates,
                ))
                .child(tab(
                    "empty-folders",
                    "Empty folders".into(),
                    ReviewKind::EmptyFolders,
                )),
        )
        .child(div().text_sm().text_color(muted).child(explanation))
        .children(
            status::notice(self.kind, scan)
                .map(|text| div().text_sm().text_color(muted).child(text)),
        )
        .children(self.queue_note.as_ref().map(|note| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded(px(4.))
                .bg(cx.theme().muted)
                .text_sm()
                .child(Icon::new(IconName::Inbox).text_color(cx.theme().primary))
                .child(div().flex_1().min_w(px(200.)).child(note.clone()))
                .child(
                    Button::new("review-open-queue")
                        .small()
                        .outline()
                        .label("Open Cleanup Queue")
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.show_page(Page::Cleanup);
                            workspace.load_page(Page::Cleanup, cx);
                        })),
                )
        }))
        .children(status.map(|status| div().text_sm().text_color(muted).child(status)))
        .child(body)
    }

    fn pagination(&self, cx: &Context<Workspace>) -> Option<Div> {
        if self.offset == 0 && !self.has_next {
            return None;
        }
        let previous = self.offset.saturating_sub(u64::from(PAGE_SIZE));
        let next = self.offset + u64::from(PAGE_SIZE);
        Some(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("previous-groups")
                        .small()
                        .label("Previous")
                        .disabled(self.loading || self.offset == 0)
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.review.load_groups(previous, cx)
                        })),
                )
                .child(
                    Button::new("next-groups")
                        .small()
                        .label("Next")
                        .disabled(self.loading || !self.has_next)
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.review.load_groups(next, cx)
                        })),
                ),
        )
    }

    /// Groups as a wrapping row of compact tiles above the table, so the table keeps the full
    /// width; the selected one carries the accent bar.
    fn group_list(&self, cx: &Context<Workspace>) -> Div {
        let theme = cx.theme();
        let rows = self.groups.iter().map(|group| {
            let group_id = group.group_id;
            let selected = self.selected_group == Some(group_id);
            div()
                .id(("review-group", group_id))
                .flex()
                .flex_col()
                .min_w(px(150.))
                .gap_1()
                .px_3()
                .py_2()
                .border_l_2()
                .rounded(px(4.))
                .cursor_pointer()
                .map(|row| {
                    if selected {
                        row.border_color(theme.primary).bg(theme.list_active)
                    } else {
                        row.border_color(gpui_kit::transparent_black())
                            .hover(|row| row.bg(theme.list_hover))
                    }
                })
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    workspace.review.select_group(group_id, cx)
                }))
                .child(
                    div()
                        .text_sm()
                        .when(selected, |title| {
                            title.font_semibold().text_color(theme.primary)
                        })
                        .child(self.group_title(group_id)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "{} · {}",
                            count(group.file_count, "file"),
                            format_bytes(group.logical_bytes)
                        )),
                )
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(rows)
            .children(self.pagination(cx))
    }

    fn group_title(&self, group_id: u64) -> String {
        match self.kind {
            ReviewKind::EmptyFolders => "Empty folders".into(),
            ReviewKind::CleanupCandidates => CleanupKind::ALL
                .get(group_id as usize - 1)
                .map_or("Candidates", |kind| kind.label())
                .to_string(),
            _ => format!("Group {group_id}"),
        }
    }

    /// Empty folders as one full-width list: full path, risk and a small Queue button.
    fn empty_folders(&self, cx: &Context<Workspace>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self
            .empty_folders
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                let risk = assess_risk(&folder.path, false);
                let path = folder.path.clone();
                let scan = self.scan.clone();
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .hover(|row| row.bg(theme.list_hover))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .flex_1()
                            .min_w(px(NAME_MIN_WIDTH))
                            .overflow_hidden()
                            .child(
                                img(crate::file_icons::icon_path(
                                    folder.path.file_name().unwrap_or(folder.path.as_os_str()),
                                    true,
                                ))
                                .size(px(16.))
                                .flex_none(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_sm()
                                    .child(folder.path.display().to_string()),
                            ),
                    )
                    .child(risk_cell(risk, cx))
                    .child(
                        div().w(px(ACTION_WIDTH)).child(
                            Button::new(("queue-folder", index))
                                .small()
                                .icon(Icon::new(IconName::Inbox))
                                .label("Queue")
                                .disabled(risk.tier == RiskTier::Protected || self.loading)
                                .on_click(cx.listener(move |workspace, _, _, cx| {
                                    if let Some(scan) = &scan {
                                        workspace.queue_cleanup(
                                            scan.clone(),
                                            path.clone(),
                                            None,
                                            cx,
                                        );
                                    }
                                })),
                        ),
                    )
            });
        div()
            .id("empty-folder-detail")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme.border)
            .rounded(px(4.))
            .child(column_header(&["Folder", "Risk", ""], false, cx))
            .children(rows)
    }

    /// The selected group as aligned rows, so sizes, dates, risk and full paths line up for
    /// comparison. Images show a thumbnail.
    fn group_detail(&self, cx: &Context<Workspace>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let members: &[Member] = match (self.kind, self.selected_group) {
            (ReviewKind::CleanupCandidates, Some(group_id)) => self
                .candidate_groups
                .get(group_id as usize - 1)
                .map_or(&[], Vec::as_slice),
            _ => &self.members,
        };
        let group = self
            .groups
            .iter()
            .find(|group| Some(group.group_id) == self.selected_group);
        let total = group.map_or(members.len() as u64, |group| group.file_count);
        let hidden = total.saturating_sub(MAX_CARDS as u64);
        let exact = self.kind == ReviewKind::ExactDuplicates;
        let thumbnails = matches!(
            self.kind,
            ReviewKind::ExactDuplicates | ReviewKind::SimilarImages
        );
        let summary = group.map(|group| match self.kind {
            ReviewKind::ExactDuplicates => {
                let each = group.logical_bytes / group.file_count.max(1);
                format!(
                    "{} copies of {} · keeping one frees up to {}",
                    group.file_count,
                    format_bytes(each),
                    format_bytes(group.logical_bytes.saturating_sub(each))
                )
            }
            _ => format!(
                "{} · {}",
                count(group.file_count, "file"),
                format_bytes(group.logical_bytes)
            ),
        });
        let rows = members
            .iter()
            .take(MAX_CARDS)
            .enumerate()
            .map(|(index, member)| {
                let file = &member.file;
                let path = file.path.clone();
                let keep_path = file.path.clone();
                let scan = self.scan.clone();
                let keeper = self.keeper.clone();
                let kept = keeper.as_ref() == Some(&file.path);
                let actionable = exact || self.kind == ReviewKind::CleanupCandidates;
                let risk = assess_risk(&file.path, file.read_only);
                let name = file.path.file_name().unwrap_or(file.path.as_os_str());
                let evidence = match &member.evidence {
                    Some(Evidence::Exact(evidence)) => {
                        format!("SHA-256 {}…", hex(&evidence.hash[..8]))
                    }
                    Some(Evidence::Similar(evidence)) if index == 0 => {
                        format!("Reference image · hash {:016x}", evidence.difference_hash)
                    }
                    Some(Evidence::Similar(evidence)) => {
                        format!("{} of 64 bits differ from the reference", evidence.distance)
                    }
                    Some(Evidence::Cleanup(candidate)) => candidate.evidence.clone(),
                    None => "No saved evidence for this file".into(),
                };
                // GPUI receives a bounded thumbnail, never a live filesystem path.
                let thumbnail = thumbnails.then(|| {
                    let content = match &member.preview {
                        Some(preview) => img(Arc::clone(preview))
                            .object_fit(ObjectFit::Contain)
                            .size_full()
                            .into_any_element(),
                        None => img(crate::file_icons::icon_path(name, false))
                            .size(px(24.))
                            .into_any_element(),
                    };
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_none()
                        .size(px(THUMB_SIZE))
                        .rounded(px(4.))
                        .bg(theme.secondary)
                        .overflow_hidden()
                        .child(content)
                });
                let mut actions = div().flex().flex_wrap().gap_2().pt_1();
                if exact {
                    actions = actions.child(
                        Button::new(("keep-duplicate", index))
                            .small()
                            .icon(Icon::new(if kept {
                                IconName::CircleCheck
                            } else {
                                IconName::Check
                            }))
                            .label(if kept { "Kept copy" } else { "Keep this" })
                            .selected(kept)
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.review.keeper = Some(keep_path.clone());
                                cx.notify();
                            })),
                    );
                }
                if actionable {
                    actions = actions.child(
                        Button::new(("queue-review-file", index))
                            .small()
                            .icon(Icon::new(IconName::Inbox))
                            .label("Queue")
                            .disabled(
                                self.loading
                                    || risk.tier == RiskTier::Protected
                                    || (exact && (keeper.is_none() || kept)),
                            )
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                if let Some(scan) = &scan {
                                    workspace.queue_cleanup(
                                        scan.clone(),
                                        path.clone(),
                                        if exact { keeper.clone() } else { None },
                                        cx,
                                    );
                                }
                            })),
                    );
                }
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .map(|row| {
                        if kept {
                            row.bg(theme.list_active)
                        } else {
                            row.hover(|row| row.bg(theme.list_hover))
                        }
                    })
                    .children(thumbnail)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_w(px(NAME_MIN_WIDTH))
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        img(crate::file_icons::icon_path(name, false))
                                            .size(px(16.))
                                            .flex_none(),
                                    )
                                    // One line; the full name is in the wrapped path below.
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_sm()
                                            .font_semibold()
                                            .child(name.to_string_lossy().into_owned()),
                                    ),
                            )
                            // The full path wraps instead of being cut off.
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(file.path.display().to_string()),
                            )
                            .child(div().text_xs().text_color(muted).child(evidence))
                            // Actions sit under the file they act on, so the columns stay narrow.
                            .child(actions),
                    )
                    .child(
                        div()
                            .w(px(SIZE_WIDTH))
                            .text_sm()
                            .text_right()
                            .child(format_bytes(file.logical_bytes)),
                    )
                    .child(
                        div()
                            .w(px(DATE_WIDTH))
                            .text_xs()
                            .text_color(muted)
                            .child(date(file.modified)),
                    )
                    .child(risk_cell(risk, cx))
            });

        div()
            .id("review-detail")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .gap_2()
            .overflow_scroll()
            .children(summary.map(|summary| div().text_sm().font_semibold().child(summary)))
            .children((exact && self.keeper.is_none() && !members.is_empty()).then(|| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Choose Keep this on the copy to keep; the others can then be queued.")
            }))
            .children((hidden > 0).then(|| {
                div().text_sm().text_color(muted).child(format!(
                    "Showing the first {MAX_CARDS} of {total} files. Export CSV includes all indexed files.",
                ))
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(TABLE_MIN_WIDTH))
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(4.))
                    .child(column_header(
                        &["File", "Size", "Modified", "Risk"],
                        thumbnails,
                        cx,
                    ))
                    .children(rows),
            )
    }
}

const THUMB_SIZE: f32 = 56.;
const NAME_MIN_WIDTH: f32 = 150.;
/// Below this the detail scrolls sideways rather than squeezing the columns.
const TABLE_MIN_WIDTH: f32 = 480.;
const SIZE_WIDTH: f32 = 72.;
const DATE_WIDTH: f32 = 76.;
const RISK_WIDTH: f32 = 112.;
const ACTION_WIDTH: f32 = 100.;

/// Column labels matching the row widths: the first label takes the remaining space.
fn column_header(labels: &[&'static str], thumbnail: bool, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let widths: &[f32] = if labels.len() == 3 {
        &[RISK_WIDTH, ACTION_WIDTH]
    } else {
        &[SIZE_WIDTH, DATE_WIDTH, RISK_WIDTH]
    };
    let mut header = div()
        .flex()
        .gap_3()
        .px_3()
        .py_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .when(thumbnail, |header| header.child(div().w(px(THUMB_SIZE))))
        .child(div().flex_1().min_w(px(NAME_MIN_WIDTH)).child(labels[0]));
    for (label, width) in labels[1..].iter().zip(widths) {
        header = header.child(
            div()
                .w(px(*width))
                .when(*label == "Size", |cell| cell.text_right())
                .child(*label),
        );
    }
    header
}

/// A coloured risk chip with the reason underneath.
fn risk_cell(risk: RiskAssessment, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (label, color) = match risk.tier {
        RiskTier::Protected => ("Protected", theme.danger),
        RiskTier::High => ("High risk", theme.warning),
        RiskTier::Unknown => ("Unknown risk", theme.muted_foreground),
        RiskTier::Low => ("Low risk", theme.success),
    };
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap_1()
        .w(px(RISK_WIDTH))
        .child(
            div()
                .px_2()
                .rounded(px(4.))
                .border_1()
                .border_color(color)
                .text_color(color)
                .text_xs()
                .font_semibold()
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(risk.reason),
        )
}

fn date(time: Option<SystemTime>) -> String {
    time.and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map(|time| time.strftime("%F").to_string())
        .unwrap_or_else(|| "—".into())
}

fn load_groups(
    kind: ReviewKind,
    scan_id: i64,
    offset: u64,
    scanned_at: SystemTime,
) -> Result<GroupsAndCandidates, Box<dyn std::error::Error + Send + Sync>> {
    let store = storage::open()?;
    Ok(match kind {
        ReviewKind::ExactDuplicates => (
            store.duplicate_groups(scan_id, PAGE_SIZE + 1, offset)?,
            Vec::new(),
            Vec::new(),
        ),
        ReviewKind::SimilarImages => (
            store.similar_image_groups(scan_id, PAGE_SIZE + 1, offset)?,
            Vec::new(),
            Vec::new(),
        ),
        ReviewKind::EmptyFolders => {
            let folders = store.empty_directories(scan_id, PAGE_SIZE + 1, offset)?;
            let groups = if folders.is_empty() {
                Vec::new()
            } else {
                vec![ReviewGroup {
                    group_id: 1,
                    file_count: folders.len().min(PAGE_SIZE as usize) as u64,
                    logical_bytes: 0,
                }]
            };
            (groups, Vec::new(), folders)
        }
        ReviewKind::CleanupCandidates => {
            let mut by_kind: Vec<Vec<Member>> =
                CleanupKind::ALL.iter().map(|_| Vec::new()).collect();
            let mut totals: Vec<ReviewGroup> = CleanupKind::ALL
                .iter()
                .enumerate()
                .map(|(index, _)| ReviewGroup {
                    group_id: index as u64 + 1,
                    file_count: 0,
                    logical_bytes: 0,
                })
                .collect();
            store.visit_cleanup_candidates(scan_id, scanned_at, |file, candidate| {
                let index = CleanupKind::ALL
                    .iter()
                    .position(|kind| *kind == candidate.kind)
                    .expect("every kind is listed");
                totals[index].file_count += 1;
                totals[index].logical_bytes = totals[index]
                    .logical_bytes
                    .saturating_add(file.logical_bytes);
                if by_kind[index].len() < MAX_CARDS {
                    by_kind[index].push(Member::new(file, Some(Evidence::Cleanup(candidate))));
                }
            })?;
            let groups = totals
                .into_iter()
                .filter(|group| group.file_count > 0)
                .collect();
            (groups, by_kind, Vec::new())
        }
    })
}

fn load_members(
    kind: ReviewKind,
    scan_id: i64,
    group_id: u64,
    cancelled: &AtomicBool,
) -> Result<Vec<Member>, Box<dyn std::error::Error + Send + Sync>> {
    let store = storage::open()?;
    let mut members: Vec<Member> = match kind {
        ReviewKind::ExactDuplicates => {
            let files = store.duplicate_group_files(scan_id, group_id, MAX_CARDS as u32)?;
            let mut evidence = store.duplicate_evidence(scan_id, &files)?;
            files
                .into_iter()
                .map(|file| {
                    let found = evidence.remove(&file.path).map(Evidence::Exact);
                    Member::new(file, found)
                })
                .collect()
        }
        ReviewKind::SimilarImages => store
            .similar_image_group_files(scan_id, group_id, MAX_CARDS as u32)?
            .into_iter()
            .map(|(file, evidence)| Member::new(file, Some(Evidence::Similar(evidence))))
            .collect(),
        ReviewKind::CleanupCandidates | ReviewKind::EmptyFolders => Vec::new(),
    };
    for member in &mut members {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if is_hashable_image(&member.file.path) {
            match image_preview(&member.file, cancelled) {
                Ok(png) => {
                    member.preview = Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
                }
                Err(error) => member.preview_error = Some(format!("Preview unavailable: {error}")),
            }
        }
    }
    Ok(members)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
