use std::io;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{AppContext as _, Context, Div, ParentElement as _, Styled as _, Task, div, px};
use scan_store::{FileTypeTotal, SavedScan, ScanStore};

use crate::workspace::{Page, Workspace, scan::format_bytes, storage};

const RECENT_SCAN_LIMIT: u32 = 5;

#[derive(Default)]
pub(in crate::workspace) struct SavedOverview {
    pub(super) data: Option<Snapshot>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    request: Option<Task<()>>,
}

pub(super) struct Snapshot {
    pub scans: Vec<SavedScan>,
    pub file_types: Vec<FileTypeTotal>,
}

impl SavedOverview {
    pub(in crate::workspace) fn load(&mut self, cx: &mut Context<Workspace>) {
        self.request.take();
        self.loading = true;
        self.error = None;
        self.data = None;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move { load_snapshot(&storage::open()?) })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let overview = &mut workspace.overview;
                overview.loading = false;
                match result {
                    Ok(data) => overview.data = Some(data),
                    Err(error) => {
                        overview.error = Some(format!("Could not load saved overview: {error}"))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn load_snapshot(store: &ScanStore) -> io::Result<Snapshot> {
    let scans = store
        .scans(RECENT_SCAN_LIMIT, 0)
        .map_err(io::Error::other)?;
    let file_types = scans
        .first()
        .map(|scan| store.file_types(scan.id))
        .transpose()
        .map_err(io::Error::other)?
        .unwrap_or_default();
    Ok(Snapshot { scans, file_types })
}

pub(super) fn render(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let overview = &workspace.overview;
    let muted = cx.theme().muted_foreground;
    let mut card = div()
        .flex()
        .flex_col()
        .gap_3()
        .w_full()
        .max_w(px(800.))
        .p_5()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().font_semibold().child("Saved scans"))
                .child(
                    Button::new("refresh-overview")
                        .ghost()
                        .small()
                        .label("Refresh")
                        .disabled(overview.loading)
                        .on_click(cx.listener(|workspace, _, _, cx| workspace.overview.load(cx))),
                ),
        );
    if overview.loading {
        return card.child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Loading saved scans…"),
        );
    }
    if let Some(error) = &overview.error {
        return card.child(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone()),
        );
    }
    let Some(data) = &overview.data else {
        return card;
    };
    let Some(latest) = data.scans.first() else {
        return card.child(
            div()
                .text_sm()
                .text_color(muted)
                .child("No saved scans yet. Choose a folder to begin."),
        );
    };
    card = card
        .child(div().font_semibold().child("Latest saved findings"))
        .child(div().text_sm().child(latest.root.display().to_string()))
        .child(div().text_sm().text_color(muted).child(scan_description(latest)))
        .children(findings(latest).into_iter().map(|line| div().text_sm().child(line)))
        .children(latest.first_error.as_ref().map(|error| div().text_sm().text_color(muted).child(format!("First issue: {error}"))))
        .child(scan_actions(latest, workspace.can_start_scan(), cx))
        .child(div().font_semibold().child("Storage categories · indexed logical size"))
        .child(div().text_sm().text_color(muted).child("Types are based on extensions. Partial scans include only indexed files; hard-linked names count separately."))
        .children(data.file_types.iter().map(|total| div().text_sm().child(format!(
            "{} · {} files · {}", total.file_type, total.files, format_bytes(total.logical_bytes)))))
        .children(data.file_types.is_empty().then(|| div().text_sm().text_color(muted).child("No indexed files in this scan.")))
        .children((data.scans.len() > 1).then(|| div().font_semibold().child("Earlier saved scans")));
    for scan in data.scans.iter().skip(1) {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().child(scan.root.display().to_string()))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(scan_description(scan)),
                )
                .child(scan_actions(scan, workspace.can_start_scan(), cx)),
        );
    }
    card.child(
        Button::new("overview-history")
            .ghost()
            .small()
            .label("All scan history")
            .on_click(cx.listener(|workspace, _, _, cx| {
                workspace.page = Page::History;
                workspace.history.load(0, cx);
            })),
    )
}

fn scan_actions(scan: &SavedScan, can_scan: bool, cx: &Context<Workspace>) -> Div {
    let to_open = scan.clone();
    let to_review = scan.clone();
    let root = scan.root.clone();
    div()
        .flex()
        .gap_2()
        .child(
            Button::new(("overview-open", scan.id as u64))
                .small()
                .label("Open results")
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    workspace.results.open_saved(to_open.clone(), cx);
                    workspace.page = Page::Results;
                    cx.notify();
                })),
        )
        .child(
            Button::new(("overview-review", scan.id as u64))
                .small()
                .ghost()
                .label("Review findings")
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    // Keep Review and Results on the same scan for subsequent sidebar navigation.
                    workspace.results.open_saved(to_review.clone(), cx);
                    workspace.review.open(Some(to_review.clone()), cx);
                    workspace.page = Page::Review;
                    cx.notify();
                })),
        )
        .child(
            Button::new(("overview-repeat", scan.id as u64))
                .small()
                .ghost()
                .label("Scan again")
                .disabled(!can_scan)
                .on_click(
                    cx.listener(move |workspace, _, _, cx| workspace.scan_again(root.clone(), cx)),
                ),
        )
}

fn scan_description(scan: &SavedScan) -> String {
    let date = jiff::Timestamp::from_millisecond(scan.started_unix_ms)
        .map(|time| time.strftime("%F %H:%M UTC").to_string())
        .unwrap_or_else(|_| "Date unavailable".into());
    format!(
        "{date} · {} · {} files · {} logical size",
        if scan.cancelled {
            "Cancelled · partial index"
        } else {
            "Completed"
        },
        scan.progress.files,
        format_bytes(scan.progress.logical_bytes)
    )
}

fn findings(scan: &SavedScan) -> Vec<String> {
    let analysis = |count, complete, errors| {
        format!(
            "{count} groups · {} · {errors} skipped checks",
            if complete {
                "analysis finished"
            } else {
                "analysis incomplete"
            }
        )
    };
    vec![
        format!(
            "Large files · {} at least {}",
            scan.large_file_count,
            format_bytes(scan.large_file_threshold)
        ),
        format!(
            "Exact duplicates · {}",
            analysis(
                scan.duplicate_group_count,
                scan.duplicates_analyzed,
                scan.duplicate_error_count
            )
        ),
        format!(
            "Similar images (heuristic) · {}",
            analysis(
                scan.similar_group_count,
                scan.similar_images_analyzed,
                scan.image_error_count
            )
        ),
        format!(
            "Traversal · {} skipped entries · {} errors",
            scan.progress.skipped, scan.progress.errors
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use scan_engine::{FileRecord, ScanProgress, ScanSummary};
    use std::path::Path;

    #[test]
    fn empty_history_has_no_fabricated_summary() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let store = ScanStore::open(&fixture.path().join("scans.sqlite3"))?;
        let snapshot = load_snapshot(&store)?;
        assert!(snapshot.scans.is_empty());
        assert!(snapshot.file_types.is_empty());
        Ok(())
    }

    #[test]
    fn reopens_five_recent_scans_with_categories_from_the_latest_partial_index()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let database = fixture.path().join("scans.sqlite3");
        let mut store = ScanStore::open(&database)?;
        let mut ids = Vec::new();
        for index in 0..7 {
            let root = fixture.path().join(format!("scan-{index}"));
            let mut writer = store.begin_scan(&root, 100)?;
            let latest = index == 6;
            writer.insert_file(&FileRecord {
                path: root.join(if latest { "photo.png" } else { "notes.txt" }),
                logical_bytes: if latest { 200 } else { 10 },
                created: None,
                modified: None,
                read_only: false,
            })?;
            if !latest {
                writer.mark_duplicates_analyzed(0)?;
                writer.mark_similar_images_analyzed(0)?;
            }
            let summary = ScanSummary {
                progress: ScanProgress {
                    files: 1,
                    logical_bytes: if latest { 200 } else { 10 },
                    ..Default::default()
                },
                cancelled: latest,
                first_error: None,
            };
            ids.push(writer.finish(&summary, u64::from(latest))?.id);
        }
        drop(store);
        let store = ScanStore::open(&database)?;
        let snapshot = load_snapshot(&store)?;
        assert_eq!(
            snapshot
                .scans
                .iter()
                .map(|scan| scan.id)
                .collect::<Vec<_>>(),
            ids.into_iter().rev().take(5).collect::<Vec<_>>()
        );
        let latest = &snapshot.scans[0];
        assert!(latest.cancelled);
        assert_eq!(latest.large_file_count, 1);
        assert_eq!(snapshot.file_types.len(), 1);
        assert_eq!(
            snapshot.file_types[0].file_type,
            scan_engine::file_type(Path::new("photo.png"))
        );
        assert_eq!(snapshot.file_types[0].files, 1);
        assert_eq!(snapshot.file_types[0].logical_bytes, 200);
        assert!(scan_description(latest).contains("partial index"));
        let summary = findings(latest).join("\n");
        assert!(summary.contains("analysis incomplete"));
        assert!(!summary.contains("analysis finished"));
        assert!(summary.contains("heuristic"));
        assert!(
            findings(&snapshot.scans[1])
                .join("\n")
                .contains("analysis finished")
        );
        Ok(())
    }

    #[test]
    fn running_scan_does_not_replace_saved_overview() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let database = fixture.path().join("scans.sqlite3");
        let mut store = ScanStore::open(&database)?;
        let saved = store
            .begin_scan(fixture.path(), 100)?
            .finish(&ScanSummary::default(), 0)?;
        let reader = ScanStore::open(&database)?;
        let _running = store.begin_scan(&fixture.path().join("running"), 100)?;
        let snapshot = load_snapshot(&reader)?;
        assert_eq!(snapshot.scans.len(), 1);
        assert_eq!(snapshot.scans[0].id, saved.id);
        assert!(snapshot.file_types.is_empty());
        Ok(())
    }
}
