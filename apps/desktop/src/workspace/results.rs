mod file_table;
mod inspector;
mod saved;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::Button,
    input::{Input, InputEvent, InputState},
    table::{DataTable, TableEvent, TableState},
};
use gpui_kit::{
    AppContext as _, Context, Div, Entity, ParentElement as _, Styled as _, Subscription, Window,
    div, px,
};

use super::Workspace;
use scan_engine::ScanProgress;

use super::scan::{LARGE_FILE_BYTES, ScanReport, format_bytes};
use file_table::FileRows;
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
    _subscriptions: [Subscription; 2],
}

impl Results {
    pub(super) fn is_saved(&self) -> bool {
        self.saved.is_some()
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
        let selection = cx.subscribe(&table, |_, _, event: &TableEvent, cx| {
            if matches!(event, TableEvent::SelectRow(_) | TableEvent::ClearSelection) {
                cx.notify();
            }
        });

        Self {
            input,
            table,
            saved: None,
            _subscriptions: [search, selection],
        }
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

    fn replace_rows(&self, rows: FileRows, cx: &mut Context<Workspace>) {
        self.table.update(cx, |table, cx| {
            *table.delegate_mut() = rows;
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

    pub(super) fn render(&self, report: Option<&ScanReport>, cx: &Context<Workspace>) -> Div {
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
                        "Files {}–{} · Largest first",
                        saved.offset + 1,
                        saved.offset + saved.files.len() as u64
                    )
                };
                let detail = selected
                    .and_then(|index| saved.files.get(index))
                    .map(|file| {
                        inspector::scanned_file(
                            file,
                            scan.large_file_threshold,
                            saved.evidence.get(&file.path),
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
                    .map(|file| inspector::scanned_file(file, LARGE_FILE_BYTES, None, cx));
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
        let save_error = report
            .filter(|_| self.saved.is_none())
            .and_then(|report| report.save_error.as_ref());
        let show_table = self
            .saved
            .as_ref()
            .is_none_or(|saved| !saved.loading && saved.error.is_none());
        let duplicate_summary = self.saved.as_ref().map(|saved| {
            let scan = &saved.scan;
            if scan.duplicates_analyzed {
                let mut text = format!("{} duplicate groups", scan.duplicate_group_count);
                if scan.duplicate_error_count > 0 {
                    text.push_str(&format!(
                        " · {} files skipped during verification",
                        scan.duplicate_error_count
                    ));
                }
                text
            } else if scan.cancelled {
                format!(
                    "Duplicate check incomplete · {} verified groups",
                    scan.duplicate_group_count
                )
            } else {
                "Run a new scan to check for duplicates".into()
            }
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

        div()
            .flex()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_3()
                    .p_6()
                    .child(div().text_xl().font_semibold().child("Scan Results"))
                    .child(div().text_sm().text_color(muted).child(summary))
                    .children(root.map(|root| {
                        div()
                            .text_sm()
                            .text_color(muted)
                            .text_ellipsis()
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
                            .map(|message| div().text_sm().text_color(muted).child(message)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(0.)).child(
                                    Input::new(&self.input)
                                        .small()
                                        .cleanable(true)
                                        .aria_label("Search file paths"),
                                ),
                            )
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
                            ),
                    )
                    .children(
                        (!scope.is_empty()).then(|| div().text_sm().text_color(muted).child(scope)),
                    )
                    .children(show_table.then(|| {
                        div().flex().flex_col().flex_1().min_h(px(0.)).child(
                            DataTable::new(&self.table)
                                .small()
                                .accessibility_label("Scan files"),
                        )
                    }))
                    .children(pagination),
            )
            .child(inspector::render(detail, cx))
    }
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
