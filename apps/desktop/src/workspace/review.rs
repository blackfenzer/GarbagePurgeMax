use std::io;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::SystemTime;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, button::Button,
};
use gpui_kit::{
    AppContext as _, Context, Div, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Task, div, img, px,
};
use scan_engine::{
    CleanupCandidate, CleanupKind, DirectoryRecord, FileRecord, SIMILAR_IMAGE_DISTANCE,
    assess_risk, image_preview, is_hashable_image,
};
use scan_store::{DuplicateEvidence, ReviewGroup, SavedScan, SimilarImageEvidence};

use super::{Workspace, scan::format_bytes, storage};

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
}

impl Review {
    pub(super) fn open(&mut self, scan: Option<SavedScan>, cx: &mut Context<Workspace>) {
        let same_scan = self.scan.as_ref().map(|scan| scan.id) == scan.as_ref().map(|scan| scan.id);
        self.scan = scan;
        if !same_scan || self.groups.is_empty() {
            self.load_groups(0, cx);
        }
    }

    fn show(&mut self, kind: ReviewKind, cx: &mut Context<Workspace>) {
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
                "Files in each group had byte-identical contents when scanned, verified with SHA-256 and a full comparison. Previews are loaded from the current files.".to_string()
            },
            ReviewKind::SimilarImages => {
                format!(
                    "Images whose difference hash is within {SIMILAR_IMAGE_DISTANCE} of 64 bits of the group's reference image. This is a heuristic match, not a verified copy. Compare the images before acting.",
                )
            },
            ReviewKind::EmptyFolders => "Folders observed with no entries, including hidden entries. Queue eligible folders for a separate confirmation; emptiness is checked again before moving.".to_string(),
            ReviewKind::CleanupCandidates => {
                "Metadata candidates need your review. Add eligible items to Cleanup Queue; nothing moves until you confirm the queue. Protected items remain blocked.".to_string()
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

        page.child(
            div()
                .text_sm()
                .text_color(muted)
                .text_ellipsis()
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
        .children(status.map(|status| div().text_sm().text_color(muted).child(status)))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .gap_4()
                .child(self.group_list(cx))
                .child(self.group_detail(cx)),
        )
    }

    fn group_list(&self, cx: &Context<Workspace>) -> Div {
        let muted = cx.theme().muted_foreground;
        let rows = self.groups.iter().map(|group| {
            let group_id = group.group_id;
            let row = div()
                .flex()
                .flex_col()
                .p_2()
                .rounded_md()
                .child(
                    Button::new(("review-group", group_id))
                        .small()
                        .label(self.group_title(group_id))
                        .selected(self.selected_group == Some(group_id))
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.review.select_group(group_id, cx)
                        })),
                )
                .child(div().text_xs().text_color(muted).child(format!(
                    "{} items · {}",
                    group.file_count,
                    format_bytes(group.logical_bytes)
                )));
            if self.selected_group == Some(group_id) {
                row.bg(cx.theme().accent)
            } else {
                row
            }
        });
        let previous = self.offset.saturating_sub(u64::from(PAGE_SIZE));
        let next = self.offset + u64::from(PAGE_SIZE);

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(200.))
            .gap_2()
            .child(
                div()
                    .id("review-groups")
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
                    )
            }))
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

    /// Group members side by side: preview, location, size, and the evidence for the match.
    fn group_detail(&self, cx: &Context<Workspace>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        if self.kind == ReviewKind::EmptyFolders {
            return div()
                .id("empty-folder-detail")
                .flex_1()
                .min_w(px(0.))
                .overflow_y_scroll()
                .children(
                    self.empty_folders
                        .iter()
                        .enumerate()
                        .map(|(index, folder)| {
                            let risk = assess_risk(&folder.path, false);
                            let path = folder.path.clone();
                            let scan = self.scan.clone();
                            div()
                                .p_3()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(div().text_sm().child(folder.path.display().to_string()))
                                .child(div().text_xs().child(risk.tier.label()))
                                .child(div().text_xs().text_color(muted).child(risk.reason))
                                .child(
                                    Button::new(("queue-folder", index))
                                        .small()
                                        .label("Add to cleanup queue")
                                        .disabled(
                                            risk.tier == scan_engine::RiskTier::Protected
                                                || self.loading,
                                        )
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
                                )
                        }),
                );
        }
        let members: &[Member] = match (self.kind, self.selected_group) {
            (ReviewKind::CleanupCandidates, Some(group_id)) => self
                .candidate_groups
                .get(group_id as usize - 1)
                .map_or(&[], Vec::as_slice),
            _ => &self.members,
        };
        let total = self
            .groups
            .iter()
            .find(|group| Some(group.group_id) == self.selected_group)
            .map_or(members.len() as u64, |group| group.file_count);
        let hidden = total.saturating_sub(MAX_CARDS as u64);
        let cards = members
            .iter()
            .take(MAX_CARDS)
            .enumerate()
            .map(|(index, member)| {
                let file = &member.file;
                let path = file.path.clone();
                let keep_path = file.path.clone();
                let scan = self.scan.clone();
                let keeper = self.keeper.clone();
                let exact = self.kind == ReviewKind::ExactDuplicates;
                let actionable = exact || self.kind == ReviewKind::CleanupCandidates;
                let evidence = &member.evidence;
                let risk = assess_risk(&file.path, file.read_only);
                let name = file
                    .path
                    .file_name()
                    .unwrap_or(file.path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let folder = file
                    .path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default();
                // GPUI receives a bounded thumbnail, never a live filesystem path.
                let preview = if let Some(preview) = &member.preview {
                    img(Arc::clone(preview))
                        .object_fit(ObjectFit::Contain)
                        .size_full()
                        .into_any_element()
                } else {
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(
                            member
                                .preview_error
                                .clone()
                                .unwrap_or_else(|| "No preview".into()),
                        )
                        .into_any_element()
                };
                let evidence = match evidence {
                    Some(Evidence::Exact(evidence)) => vec![
                        "Exact duplicate".to_string(),
                        format!("{} files with identical contents", evidence.file_count),
                        format!("SHA-256 {}…", hex(&evidence.hash[..8])),
                    ],
                    Some(Evidence::Similar(evidence)) if index == 0 => vec![
                        "Reference image".to_string(),
                        format!("Difference hash {:016x}", evidence.difference_hash),
                    ],
                    Some(Evidence::Similar(evidence)) => vec![
                        "Similar image".to_string(),
                        format!("{} of 64 bits differ from the reference", evidence.distance),
                        format!("Difference hash {:016x}", evidence.difference_hash),
                    ],
                    Some(Evidence::Cleanup(candidate)) => vec![
                        candidate.kind.label().to_string(),
                        candidate.evidence.clone(),
                    ],
                    None => vec!["No saved evidence for this file".to_string()],
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(220.))
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .h(px(150.))
                            .mb_2()
                            .rounded_md()
                            .bg(cx.theme().secondary)
                            .overflow_hidden()
                            .child(preview),
                    )
                    .child(div().text_sm().font_semibold().text_ellipsis().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(folder),
                    )
                    .child(
                        div()
                            .text_xs()
                            .child(format!("{} logical size", format_bytes(file.logical_bytes))),
                    )
                    .child(div().text_xs().font_semibold().child(risk.tier.label()))
                    .child(div().text_xs().text_color(muted).child(risk.reason))
                    .children(evidence.into_iter().enumerate().map(|(line, text)| {
                        let row = div().text_xs();
                        if line == 0 {
                            row.font_semibold().child(text)
                        } else {
                            row.text_color(muted).child(text)
                        }
                    }))
                    .children(exact.then(|| {
                        Button::new(("keep-duplicate", index))
                            .small()
                            .label(if self.keeper.as_ref() == Some(&file.path) {
                                "Kept copy selected"
                            } else {
                                "Keep this copy"
                            })
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.review.keeper = Some(keep_path.clone());
                                cx.notify();
                            }))
                    }))
                    .children(actionable.then(|| {
                        Button::new(("queue-review-file", index))
                            .small()
                            .label("Add to cleanup queue")
                            .disabled(
                                self.loading
                                    || risk.tier == scan_engine::RiskTier::Protected
                                    || (exact
                                        && (keeper.is_none() || keeper.as_ref() == Some(&path))),
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
                            }))
                    }))
            });

        div()
            .id("review-detail")
            .flex_1()
            .min_w(px(0.))
            .overflow_y_scroll()
            .children((hidden > 0).then(|| {
                div().mb_3().text_sm().text_color(muted).child(format!(
                    "Showing the first {MAX_CARDS} of {} files. Export CSV includes all indexed files.",
                    total
                ))
            }))
            .child(div().flex().flex_wrap().gap_3().children(cards))
    }
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
