mod inspector;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
};
use gpui_kit::{
    Context, Div, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::Workspace;
use super::scan::{LARGE_FILE_BYTES, ScanReport, format_bytes};

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

pub(super) fn render(
    report: Option<&ScanReport>,
    selected_file: Option<usize>,
    cx: &Context<Workspace>,
) -> Div {
    let (description, rows, detail) = match report {
        Some(report) => {
            let summary = &report.summary;
            let progress = summary.progress;
            let status = if summary.cancelled {
                "Cancelled scan · "
            } else {
                ""
            };
            let description = format!(
                "{status}{} files · {} logical size · {} large · {} skipped · {} errors · showing {} largest",
                progress.files,
                format_bytes(progress.logical_bytes),
                report.large_file_count,
                progress.skipped,
                progress.errors,
                report.largest_files.len()
            );
            let rows = report
                .largest_files
                .iter()
                .map(|file| {
                    let name = file.path.file_name().unwrap_or(file.path.as_os_str());
                    let finding = if file.logical_bytes >= LARGE_FILE_BYTES {
                        "Large file"
                    } else {
                        "—"
                    };
                    (
                        SharedString::from(name.to_string_lossy().into_owned()),
                        finding,
                        SharedString::from(format_bytes(file.logical_bytes)),
                    )
                })
                .collect::<Vec<_>>();
            let detail = selected_file
                .and_then(|index| report.largest_files.get(index))
                .map(|file| inspector::scanned_file(file, cx));
            (description, rows, detail)
        }
        None => {
            let rows = SAMPLE_FILES
                .iter()
                .map(|file| (file.name.into(), file.finding, file.size.into()))
                .collect();
            let detail = selected_file
                .and_then(|index| SAMPLE_FILES.get(index))
                .map(|file| inspector::sample_file(file, cx));
            (
                format!("Sample data · {} files", SAMPLE_FILES.len()),
                rows,
                detail,
            )
        }
    };
    let finding_width = px(112.0);
    let size_width = px(80.0);
    let header = TableHeader::new().child(
        TableRow::new()
            .child(TableHead::new().child("File"))
            .child(TableHead::new().w(finding_width).child("Finding"))
            .child(TableHead::new().w(size_width).text_right().child("Size")),
    );
    let rows = rows
        .into_iter()
        .enumerate()
        .map(|(index, (name, finding, size))| {
            TableRow::new()
                .child(
                    TableCell::new().min_w(px(0.0)).overflow_hidden().child(
                        Button::new(("scan-file", index))
                            .ghost()
                            .small()
                            .max_w_full()
                            .label(name)
                            .selected(selected_file == Some(index))
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                workspace.selected_file = Some(index);
                                cx.notify();
                            })),
                    ),
                )
                .child(TableCell::new().w(finding_width).child(finding))
                .child(TableCell::new().w(size_width).text_right().child(size))
        });
    let muted = cx.theme().muted_foreground;
    let details = report.map(|report| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(report.root.display().to_string()),
            )
            .children(report.summary.first_error.as_ref().map(|error| {
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("First scan error: {error}"))
            }))
    });

    div()
        .flex()
        .size_full()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap_4()
                .p_6()
                .child(div().text_xl().font_semibold().child("Scan Results"))
                .child(div().text_sm().text_color(muted).child(description))
                .children(details)
                .child(
                    div()
                        .id("scan-results-list")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .child(
                            Table::new()
                                .child(header)
                                .child(TableBody::new().children(rows)),
                        ),
                ),
        )
        .child(inspector::render(detail, cx))
}
