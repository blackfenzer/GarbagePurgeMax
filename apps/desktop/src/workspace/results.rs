mod export;
mod file_table;
mod inspector;
mod saved;

use gpui_kit::base::StyledExt as _;
use std::io;
use std::time::{Duration, SystemTime};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IndexPath, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
    table::{DataTable, TableDelegate as _, TableEvent, TableState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, ParentElement as _, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};

use super::Workspace;
use scan_engine::{FILE_TYPES, ScanProgress};
use scan_store::{FileFilter, FileSort};

type Options = SelectState<SearchableVec<&'static str>>;

const ALL_TYPES: &str = "All types";
/// Ages are measured from the scan's start, like cleanup candidates.
const AGE_OPTIONS: [(&str, Option<u64>); 3] = [
    ("Any age", None),
    ("Older than 30 days", Some(30)),
    ("Older than 1 year", Some(365)),
];
const SORT_OPTIONS: [(&str, FileSort); 5] = [
    ("Largest first", FileSort::Largest),
    ("Smallest first", FileSort::Smallest),
    ("Newest first", FileSort::Newest),
    ("Oldest first", FileSort::Oldest),
    ("Name", FileSort::Name),
];

use super::scan::{LARGE_FILE_BYTES, ScanReport, format_bytes};
use file_table::FileRows;
pub(super) use file_table::RowAction;
use saved::{PAGE_SIZE, SavedResults};

struct SampleFile {
    name: &'static str,
    folder: &'static str,
    size: &'static str,
    finding: &'static str,
    explanation: &'static str,
}

const SAMPLE_FILES: &[SampleFile] = &[
    SampleFile {
        name: "studio-backup.zip",
        folder: "Example / Archives",
        size: "2.4 GiB",
        finding: "Large file",
        explanation: "This archive exceeds the example 1 GiB large-file threshold.",
    },
    SampleFile {
        name: "coast-export.png",
        folder: "Example / Design",
        size: "28.6 MiB",
        finding: "Exact duplicate",
        explanation: "In this example, both images have the same size and matching full-content hashes.",
    },
    SampleFile {
        name: "coast-export-copy.png",
        folder: "Example / Downloads",
        size: "28.6 MiB",
        finding: "Exact duplicate",
        explanation: "In this example, both images have the same size and matching full-content hashes.",
    },
];

pub(super) struct Results {
    input: Entity<InputState>,
    table: Entity<TableState<FileRows>>,
    saved: Option<SavedResults>,
    type_filter: Entity<Options>,
    age_filter: Entity<Options>,
    sort_order: Entity<Options>,
    exporting: bool,
    /// The saved file shown in the details dialog, by position in `saved.files`.
    detail: Option<usize>,
    /// Lets the table's row buttons reach the workspace.
    workspace: WeakEntity<Workspace>,
    _subscriptions: Vec<Subscription>,
}

impl Results {
    pub(super) fn is_saved(&self) -> bool {
        self.saved.is_some()
    }

    pub(super) fn saved_scan(&self) -> Option<&scan_store::SavedScan> {
        self.saved.as_ref().map(|saved| &saved.scan)
    }

    pub(super) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search file paths"));
        let table = cx.new(|cx| {
            TableState::new(FileRows::samples(), window, cx)
                .col_selectable(false)
                .col_movable(false)
        });
        let search = cx.subscribe(&input, |workspace, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value();
                if workspace.results.saved.is_some() {
                    workspace.results.load_page(0, true, cx);
                } else {
                    workspace.results.filter(&query, cx);
                }
                cx.notify();
            }
        });
        let selection = cx.subscribe(&table, |workspace, table, event: &TableEvent, cx| {
            // Amber Deck has no room for the inspector, so a click opens the details dialog.
            if let TableEvent::SelectRow(row) = event
                && workspace.theme == super::theme::AppTheme::AmberDeck
                && workspace.results.saved.is_some()
            {
                workspace.results.detail = table.read(cx).delegate().source_index(*row);
            }
            if matches!(event, TableEvent::SelectRow(_) | TableEvent::ClearSelection) {
                cx.notify();
            }
        });

        let options =
            |labels: Vec<&'static str>, window: &mut Window, cx: &mut Context<Workspace>| {
                cx.new(|cx| {
                    SelectState::new(
                        SearchableVec::new(labels),
                        Some(IndexPath::new(0)),
                        window,
                        cx,
                    )
                })
            };
        let type_filter = options(
            std::iter::once(ALL_TYPES).chain(FILE_TYPES).collect(),
            window,
            cx,
        );
        let age_filter = options(AGE_OPTIONS.map(|(label, _)| label).to_vec(), window, cx);
        let sort_order = options(SORT_OPTIONS.map(|(label, _)| label).to_vec(), window, cx);
        let mut subscriptions = vec![search, selection];
        for state in [&type_filter, &age_filter, &sort_order] {
            subscriptions.push(cx.subscribe(
                state,
                |workspace, _, _: &SelectEvent<SearchableVec<&'static str>>, cx| {
                    if workspace.results.saved.is_some() {
                        workspace.results.load_page(0, false, cx);
                    }
                },
            ));
        }

        Self {
            input,
            table,
            saved: None,
            type_filter,
            age_filter,
            sort_order,
            exporting: false,
            detail: None,
            workspace: cx.entity().downgrade(),
            _subscriptions: subscriptions,
        }
    }

    /// The search box, dropdowns, and Duplicates only toggle as a store query.
    fn file_filter(
        &self,
        scan: &scan_store::SavedScan,
        duplicates_only: bool,
        cx: &App,
    ) -> io::Result<FileFilter> {
        let row = |state: &Entity<Options>| {
            state
                .read(cx)
                .selected_index(cx)
                .map_or(0, |index| index.row)
        };
        let age_days = AGE_OPTIONS
            .get(row(&self.age_filter))
            .and_then(|(_, days)| *days);
        Ok(FileFilter {
            search: self.input.read(cx).value().to_string(),
            file_type: row(&self.type_filter)
                .checked_sub(1)
                .and_then(|index| FILE_TYPES.get(index).copied()),
            modified_before: age_cutoff(scan, age_days)?,
            sort: SORT_OPTIONS
                .get(row(&self.sort_order))
                .map_or(FileSort::Largest, |(_, sort)| *sort),
            duplicates_only,
        })
    }

    fn sort_label(&self, cx: &App) -> &'static str {
        let row = self
            .sort_order
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        SORT_OPTIONS
            .get(row)
            .map_or(SORT_OPTIONS[0].0, |(label, _)| label)
    }

    pub(super) fn show_scan(&mut self, report: &ScanReport, cx: &mut Context<Workspace>) {
        if let Some(scan) = &report.saved {
            self.open_saved(scan.clone(), cx);
            return;
        }
        self.saved = None;
        let query = self.input.read(cx).value();
        let rows = FileRows::from_files(&report.largest_files, LARGE_FILE_BYTES, &query, None);
        self.replace_rows(rows, cx);
    }

    fn replace_rows(&mut self, rows: FileRows, cx: &mut Context<Workspace>) {
        self.detail = None;
        let rows = if self.saved.is_some() {
            let workspace = self.workspace.clone();
            rows.with_actions(std::rc::Rc::new(move |action, index, _, cx| {
                let _ =
                    workspace.update(cx, |workspace, cx| workspace.file_action(action, index, cx));
            }))
        } else {
            rows
        };
        self.table.update(cx, |table, cx| {
            let columns = table.delegate().columns_count(cx);
            *table.delegate_mut() = rows;
            // The table caches its columns; reload them when the action column comes or goes.
            if table.delegate().columns_count(cx) != columns {
                table.refresh(cx);
            }
            table.clear_selection(cx);
            if table.delegate().counts().0 > 0 {
                table.scroll_to_row(0, cx);
            }
        });
    }

    fn filter(&self, query: &str, cx: &mut Context<Workspace>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().filter(query);
            table.clear_selection(cx);
            if table.delegate().counts().0 > 0 {
                table.scroll_to_row(0, cx);
            }
        });
    }

    /// `stacked` puts the inspector under the table, for Amber Deck's narrower panel.
    pub(super) fn render(
        &self,
        report: Option<&ScanReport>,
        stacked: bool,
        cx: &Context<Workspace>,
    ) -> Div {
        let table = self.table.read(cx);
        let rows = table.delegate();
        let selected = table.selected_row().and_then(|row| rows.source_index(row));
        let (matches, files) = rows.counts();
        let muted = cx.theme().muted_foreground;
        let (summary, scope, root, first_error, detail) = match (&self.saved, report) {
            (Some(saved), _) => {
                let scan = &saved.scan;
                let scope = if saved.loading {
                    "Loading files…".into()
                } else if let Some(error) = &saved.error {
                    error.clone()
                } else if saved.files.is_empty() {
                    String::new()
                } else {
                    format!(
                        "Files {}–{} · {}",
                        saved.offset + 1,
                        saved.offset + saved.files.len() as u64,
                        self.sort_label(cx)
                    )
                };
                let detail = selected
                    .and_then(|index| saved.files.get(index))
                    .map(|file| {
                        inspector::scanned_file(
                            file,
                            scan.large_file_threshold,
                            saved.evidence.get(&file.path),
                            scan.started_at().ok(),
                            cx,
                        )
                    });
                (
                    summary_line(scan.progress, scan.cancelled, scan.large_file_count),
                    scope,
                    Some(scan.root.as_path()),
                    scan.first_error.as_deref(),
                    detail,
                )
            }
            (None, Some(report)) => {
                let summary = &report.summary;
                let description =
                    summary_line(summary.progress, summary.cancelled, report.large_file_count);
                let scope = format!("{} of {} preview files · Largest first", matches, files,);
                let detail = selected
                    .and_then(|index| report.largest_files.get(index))
                    .map(|file| inspector::scanned_file(file, LARGE_FILE_BYTES, None, None, cx));
                (
                    description,
                    scope,
                    Some(report.root.as_path()),
                    summary.first_error.as_deref(),
                    detail,
                )
            }
            (None, None) => {
                let detail = selected
                    .and_then(|index| SAMPLE_FILES.get(index))
                    .map(|file| inspector::sample_file(file, cx));
                let scope = format!("{} of {} sample files", matches, files);
                ("Sample data".into(), scope, None, None, detail)
            }
        };
        let selected_file = self.saved.as_ref().and_then(|saved| {
            let file = selected.and_then(|index| saved.files.get(index))?;
            Some((saved.scan.clone(), file.path.clone()))
        });
        let save_error = report
            .filter(|_| self.saved.is_none())
            .and_then(|report| report.save_error.as_ref());
        let show_table = self
            .saved
            .as_ref()
            .is_none_or(|saved| !saved.loading && saved.error.is_none());
        let duplicate_summary = self.saved.as_ref().map(|saved| {
            let scan = &saved.scan;
            let mut text = if scan.duplicates_analyzed {
                format!(
                    "{} · {} cached hashes reused · {} cache misses",
                    super::scan::count(scan.duplicate_group_count, "duplicate group"),
                    scan.hash_cache_hits,
                    scan.hash_cache_misses
                )
            } else if scan.cancelled {
                format!(
                    "Duplicate check incomplete · {} verified groups",
                    scan.duplicate_group_count
                )
            } else {
                "Run a new scan to check for duplicates".into()
            };
            if scan.duplicate_error_count > 0 {
                text.push_str(&format!(
                    " · {} files skipped during verification",
                    scan.duplicate_error_count
                ));
            }
            text
        });
        let file_type_summary = self.saved.as_ref().map(|saved| match &saved.file_types {
            None => "Loading file types…".to_string(),
            Some(Err(error)) => error.clone(),
            Some(Ok(totals)) => {
                let label = if saved.scan.cancelled {
                    "Indexed file types"
                } else {
                    "File types"
                };
                let totals = totals
                    .iter()
                    .map(|total| {
                        format!(
                            "{} {} ({})",
                            total.file_type,
                            total.files,
                            format_bytes(total.logical_bytes)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!("{label}: {totals}")
            }
        });
        let pagination = self
            .saved
            .as_ref()
            .filter(|saved| saved.offset > 0 || saved.has_next)
            .map(|saved| {
                let previous = saved.offset.saturating_sub(u64::from(PAGE_SIZE));
                let next = saved.offset + u64::from(PAGE_SIZE);
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("previous-files")
                            .small()
                            .label("Previous")
                            .disabled(saved.offset == 0 || saved.loading)
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.results.load_page(previous, false, cx)
                            })),
                    )
                    .child(
                        Button::new("next-files")
                            .small()
                            .label("Next")
                            .disabled(!saved.has_next || saved.loading)
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.results.load_page(next, false, cx)
                            })),
                    )
            });

        // Stacked (Amber Deck): every part takes its natural height and the panel scrolls.
        div()
            .flex()
            .map(|page| {
                if stacked {
                    page.flex_col().w_full()
                } else {
                    page.size_full()
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .when(!stacked, |column| column.flex_1())
                    .min_w(px(0.))
                    .gap_3()
                    .p_6()
                    .child(div().text_xl().font_semibold().child("Scan Results"))
                    .child(div().text_sm().text_color(muted).child(summary))
                    .children(root.map(|root| {
                        // Long scan roots wrap instead of being cut off.
                        div()
                            .text_sm()
                            .text_color(muted)
                            .line_clamp(3)
                            .child(root.display().to_string())
                    }))
                    .children(first_error.map(|error| {
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("First scan error: {error}"))
                    }))
                    .children(save_error.map(|error| {
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("Could not save scan: {error}"))
                    }))
                    .children(
                        duplicate_summary
                            .into_iter()
                            .chain(file_type_summary)
                            .chain(
                                self.saved
                                    .as_ref()
                                    .and_then(|saved| saved.export_status.clone()),
                            )
                            .map(|message| div().text_sm().text_color(muted).child(message)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(180.)).child(
                                    Input::new(&self.input)
                                        .small()
                                        .cleanable(true)
                                        .aria_label("Search file paths"),
                                ),
                            )
                            .children(self.saved.as_ref().map(|_| {
                                Button::new("export-csv")
                                    .small()
                                    .label(if self.exporting {
                                        "Export in progress…"
                                    } else {
                                        "Export CSV"
                                    })
                                    .disabled(self.exporting)
                                    .on_click(cx.listener(|workspace, _, _, cx| {
                                        workspace.results.export_csv(cx)
                                    }))
                            }))
                            .children(
                                self.saved
                                    .as_ref()
                                    .filter(|saved| saved.scan.duplicate_group_count > 0)
                                    .map(|saved| {
                                        Button::new("duplicates-only")
                                            .small()
                                            .label("Duplicates only")
                                            .selected(saved.duplicates_only)
                                            .on_click(cx.listener(|workspace, _, _, cx| {
                                                if let Some(saved) = &mut workspace.results.saved {
                                                    saved.duplicates_only = !saved.duplicates_only;
                                                    workspace.results.load_page(0, false, cx);
                                                }
                                            }))
                                    }),
                            )
                            .children(self.saved.as_ref().map(|_| {
                                Button::new("trash-file")
                                    .small()
                                    .icon(Icon::new(IconName::Trash))
                                    .label("Move to Trash")
                                    .tooltip(
                                        "Move the selected file to Trash, after a confirmation",
                                    )
                                    .disabled(selected_file.is_none())
                                    .on_click(cx.listener(move |workspace, _, _, cx| {
                                        if let Some((scan, path)) = selected_file.clone() {
                                            workspace.ask_trash(scan, path, cx);
                                        }
                                    }))
                            })),
                    )
                    .children(self.saved.as_ref().map(|_| {
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                div().w(px(190.)).child(
                                    Select::new(&self.type_filter)
                                        .small()
                                        .accessibility_label("File type"),
                                ),
                            )
                            .child(
                                div().w(px(170.)).child(
                                    Select::new(&self.age_filter)
                                        .small()
                                        .accessibility_label("Last modified"),
                                ),
                            )
                            .child(
                                div().w(px(150.)).child(
                                    Select::new(&self.sort_order)
                                        .small()
                                        .accessibility_label("Sort order"),
                                ),
                            )
                    }))
                    .children(
                        (!scope.is_empty()).then(|| div().text_sm().text_color(muted).child(scope)),
                    )
                    .children(show_table.then(|| {
                        // Fills the page when there is room; in a narrow or scrolling panel it
                        // keeps about eight rows and the panel scrolls instead.
                        div()
                            .flex()
                            .flex_col()
                            .map(|table| {
                                if stacked {
                                    table.h(px(440.))
                                } else {
                                    table.flex_1().min_h(px(320.))
                                }
                            })
                            .child(
                                DataTable::new(&self.table)
                                    .small()
                                    .accessibility_label("Scan files"),
                            )
                    }))
                    .children(pagination),
            )
            // Stacked, the details dialog takes the inspector's place.
            .when(!stacked, |page| page.child(inspector::render(detail, cx)))
    }

    /// The saved file at `index` and its scan, for Move to Trash.
    fn saved_file(&self, index: usize) -> Option<(scan_store::SavedScan, std::path::PathBuf)> {
        let saved = self.saved.as_ref()?;
        Some((saved.scan.clone(), saved.files.get(index)?.path.clone()))
    }

    /// The details dialog for the file picked in the table, or nothing.
    pub(super) fn detail_modal(&self, cx: &Context<Workspace>) -> Option<AnyElement> {
        let index = self.detail?;
        let saved = self.saved.as_ref()?;
        let file = saved.files.get(index)?;
        let close = Button::new("file-detail-close")
            .outline()
            .label("Close")
            .on_click(cx.listener(|workspace, _, _, cx| {
                workspace.results.detail = None;
                cx.notify();
            }));
        let trash = Button::new("file-detail-trash")
            .danger()
            .icon(Icon::new(IconName::Trash))
            .label("Move to Trash")
            .on_click(cx.listener(move |workspace, _, _, cx| {
                workspace.file_action(RowAction::Trash, index, cx)
            }));
        Some(super::trash::modal(
            "file-detail",
            IconName::Info,
            "File details",
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(inspector::scanned_file(
                    file,
                    saved.scan.large_file_threshold,
                    saved.evidence.get(&file.path),
                    saved.scan.started_at().ok(),
                    cx,
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap_2()
                        .child(close)
                        .child(trash),
                ),
            cx,
        ))
    }
}

impl Workspace {
    /// A Files row button: show the file's details, or start Move to Trash.
    pub(super) fn file_action(&mut self, action: RowAction, index: usize, cx: &mut Context<Self>) {
        match action {
            RowAction::Inspect => self.results.detail = Some(index),
            RowAction::Trash => {
                self.results.detail = None;
                if let Some((scan, path)) = self.results.saved_file(index) {
                    self.ask_trash(scan, path, cx);
                }
            }
        }
        cx.notify();
    }
}

fn age_cutoff(scan: &scan_store::SavedScan, days: Option<u64>) -> io::Result<Option<SystemTime>> {
    let Some(days) = days else { return Ok(None) };
    let seconds = days.checked_mul(24 * 60 * 60).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Age filter duration is too large",
        )
    })?;
    scan.started_at()?
        .checked_sub(Duration::from_secs(seconds))
        .map(Some)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Age filter date is outside the supported range",
            )
        })
}

fn summary_line(progress: ScanProgress, cancelled: bool, large_file_count: u64) -> String {
    let status = if cancelled { "Cancelled scan · " } else { "" };
    format!(
        "{status}{} files · {} logical size · {} large · {} skipped · {} errors",
        progress.files,
        format_bytes(progress.logical_bytes),
        large_file_count,
        progress.skipped,
        progress.errors
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use scan_engine::{FileRecord, ScanSummary};
    use scan_store::{FileFilter, ScanStore};

    #[test]
    fn age_filter_uses_saved_time_and_preserves_pre_epoch_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let mut store = ScanStore::open(&fixture.path().join("scans.sqlite3"))?;
        let mut writer = store.begin_scan(fixture.path(), 100)?;
        let cutoff =
            SystemTime::UNIX_EPOCH - Duration::from_millis(1) - Duration::from_secs(30 * 86400);
        for (name, modified) in [
            ("older", cutoff - Duration::from_nanos(1)),
            ("boundary", cutoff),
            ("newer", cutoff + Duration::from_nanos(1)),
        ] {
            writer.insert_file(&FileRecord {
                path: fixture.path().join(name),
                logical_bytes: 1,
                created: None,
                modified: Some(modified),
                read_only: false,
            })?;
        }
        let mut scan = writer.finish(&ScanSummary::default(), 0)?;
        scan.started_unix_ms = -1;
        assert_eq!(age_cutoff(&scan, Some(30))?, Some(cutoff));
        let filter = FileFilter {
            modified_before: age_cutoff(&scan, Some(30))?,
            ..Default::default()
        };
        let matches = store.filtered_files(scan.id, &filter, 10, 0)?;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path.file_name().unwrap(), "older");
        assert_eq!(age_cutoff(&scan, None)?, None);
        assert!(age_cutoff(&scan, Some(u64::MAX)).is_err());
        Ok(())
    }
}
