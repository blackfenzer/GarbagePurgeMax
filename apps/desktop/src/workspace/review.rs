use std::io;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, button::Button,
};
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Task, div,
    img, px,
};
use scan_engine::{
    CleanupCandidate, CleanupKind, FileRecord, SIMILAR_IMAGE_DISTANCE, is_hashable_image,
};
use scan_store::{DuplicateEvidence, ReviewGroup, SavedScan, SimilarImageEvidence};

use super::{Workspace, scan::format_bytes, storage};

const PAGE_SIZE: u32 = 30;
/// Cards shown for one group. Candidate groups can hold thousands of files.
const MAX_CARDS: usize = 200;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ReviewKind {
    #[default]
    ExactDuplicates,
    SimilarImages,
    CleanupCandidates,
}

type GroupsAndCandidates = (Vec<ReviewGroup>, Vec<Vec<Member>>);

/// A group member and the saved evidence for its match.
type Member = (FileRecord, Option<Evidence>);

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
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
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
        self.groups.clear();
        self.members.clear();
        self.candidate_groups.clear();
        self.selected_group = None;
        self.error = None;
        let Some(scan) = &self.scan else {
            self.loading = false;
            cx.notify();
            return;
        };
        let scan_id = scan.id;
        let scanned_at =
            UNIX_EPOCH + Duration::from_millis(u64::try_from(scan.started_unix_ms).unwrap_or(0));
        self.loading = true;
        let kind = self.kind;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    load_groups(kind, scan_id, offset, scanned_at).map_err(io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let review = &mut workspace.review;
                review.loading = false;
                match result {
                    Ok((mut groups, candidate_groups)) => {
                        review.candidate_groups = candidate_groups;
                        review.has_next = groups.len() > PAGE_SIZE as usize;
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
        let Some(scan_id) = self.scan.as_ref().map(|scan| scan.id) else {
            return;
        };
        if self.kind == ReviewKind::CleanupCandidates {
            // Candidates arrive with their groups, so selection needs no query.
            self.selected_group = Some(group_id);
            cx.notify();
            return;
        }
        self.request.take();
        self.selected_group = Some(group_id);
        self.members.clear();
        self.loading = true;
        let kind = self.kind;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    load_members(kind, scan_id, group_id).map_err(io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let review = &mut workspace.review;
                if review.selected_group != Some(group_id) {
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
        let page = div()
            .flex()
            .flex_col()
            .size_full()
            .gap_3()
            .p_6()
            .child(div().text_xl().font_semibold().child("Review"));
        let Some(scan) = &self.scan else {
            return page.child(div().text_sm().text_color(muted).child(
                "Run a scan, or open one from Scan History, to review its duplicate and similar-image groups.",
            ));
        };

        let (analyzed, explanation) = match self.kind {
            ReviewKind::ExactDuplicates => (
                scan.duplicates_analyzed || scan.duplicate_group_count > 0,
                "Every file in a group has byte-identical contents, verified with SHA-256 and a full comparison.".to_string(),
            ),
            ReviewKind::SimilarImages => (
                scan.similar_images_analyzed,
                format!(
                    "Images whose difference hash is within {SIMILAR_IMAGE_DISTANCE} of 64 bits of the group's reference image. This is a heuristic match, not a verified copy. Compare the images before acting.",
                ),
            ),
            ReviewKind::CleanupCandidates => (
                true,
                "Files matching a cleanup rule, judged from the names, sizes, folders, and dates saved with the scan. Candidates are for review only. Nothing is moved or deleted.".to_string(),
            ),
        };
        let status = if self.loading {
            Some("Loading…".to_string())
        } else if let Some(error) = &self.error {
            Some(error.clone())
        } else if !analyzed {
            Some("This scan has no results for this check. Run a new scan to add them.".into())
        } else if self.groups.is_empty() {
            Some("No groups found in this scan.".into())
        } else {
            None
        };
        let image_errors = (self.kind == ReviewKind::SimilarImages && scan.image_error_count > 0)
            .then(|| format!("{} images could not be read.", scan.image_error_count));

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
                )),
        )
        .child(div().text_sm().text_color(muted).child(explanation))
        .children(image_errors.map(|text| div().text_sm().text_color(muted).child(text)))
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
                .id(("review-group", group_id))
                .flex()
                .flex_col()
                .p_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|row| row.bg(cx.theme().secondary))
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    workspace.review.select_group(group_id, cx)
                }))
                .child(div().text_sm().child(self.group_title(group_id)))
                .child(div().text_xs().text_color(muted).child(format!(
                    "{} files · {}",
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
        let members: &[Member] = match (self.kind, self.selected_group) {
            (ReviewKind::CleanupCandidates, Some(group_id)) => self
                .candidate_groups
                .get(group_id as usize - 1)
                .map_or(&[], Vec::as_slice),
            _ => &self.members,
        };
        let hidden = members.len().saturating_sub(MAX_CARDS);
        let cards = members
            .iter()
            .take(MAX_CARDS)
            .enumerate()
            .map(|(index, (file, evidence))| {
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
                // GPUI loads and decodes the preview off the UI thread.
                let preview = if is_hashable_image(&file.path) {
                    img(file.path.clone())
                        .object_fit(ObjectFit::Contain)
                        .size_full()
                        .into_any_element()
                } else {
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("No preview")
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
                    .children(evidence.into_iter().enumerate().map(|(line, text)| {
                        let row = div().text_xs();
                        if line == 0 {
                            row.font_semibold().child(text)
                        } else {
                            row.text_color(muted).child(text)
                        }
                    }))
            });

        div()
            .id("review-detail")
            .flex_1()
            .min_w(px(0.))
            .overflow_y_scroll()
            .children((hidden > 0).then(|| {
                div().mb_3().text_sm().text_color(muted).child(format!(
                    "Showing the largest {MAX_CARDS} of {} files.",
                    members.len()
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
        ),
        ReviewKind::SimilarImages => (
            store.similar_image_groups(scan_id, PAGE_SIZE + 1, offset)?,
            Vec::new(),
        ),
        ReviewKind::CleanupCandidates => {
            let mut by_kind: Vec<Vec<Member>> =
                CleanupKind::ALL.iter().map(|_| Vec::new()).collect();
            for (file, candidate) in store.cleanup_candidates(scan_id, scanned_at)? {
                let index = CleanupKind::ALL
                    .iter()
                    .position(|kind| *kind == candidate.kind)
                    .expect("every kind is listed");
                by_kind[index].push((file, Some(Evidence::Cleanup(candidate))));
            }
            let groups = by_kind
                .iter()
                .enumerate()
                .filter(|(_, members)| !members.is_empty())
                .map(|(index, members)| ReviewGroup {
                    group_id: index as u64 + 1,
                    file_count: members.len() as u64,
                    logical_bytes: members.iter().map(|(file, _)| file.logical_bytes).sum(),
                })
                .collect();
            (groups, by_kind)
        }
    })
}

fn load_members(
    kind: ReviewKind,
    scan_id: i64,
    group_id: u64,
) -> Result<Vec<Member>, Box<dyn std::error::Error + Send + Sync>> {
    let store = storage::open()?;
    Ok(match kind {
        ReviewKind::ExactDuplicates => {
            let files = store.duplicate_group_files(scan_id, group_id)?;
            let mut evidence = store.duplicate_evidence(scan_id, &files)?;
            files
                .into_iter()
                .map(|file| {
                    let found = evidence.remove(&file.path).map(Evidence::Exact);
                    (file, found)
                })
                .collect()
        }
        ReviewKind::SimilarImages => store
            .similar_image_group_files(scan_id, group_id)?
            .into_iter()
            .map(|(file, evidence)| (file, Some(Evidence::Similar(evidence))))
            .collect(),
        ReviewKind::CleanupCandidates => Vec::new(),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
