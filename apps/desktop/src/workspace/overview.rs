use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{Context, Div, ParentElement as _, Styled as _, div, px};

use super::scan::{ScanSession, format_bytes};
use super::{Page, Workspace};

pub(super) fn render(scan: &ScanSession, viewing_saved_scan: bool, cx: &Context<Workspace>) -> Div {
    let (heading, description) = match (&scan.active, &scan.report) {
        (Some(active), _) => (
            if active.cancelling {
                "Cancelling scan"
            } else {
                "Scanning folder"
            },
            format!(
                "{} · {} files · {} folders · {} logical size · {} skipped · {} errors",
                active.root.display(),
                active.progress.files,
                active.progress.directories,
                format_bytes(active.progress.logical_bytes),
                active.progress.skipped,
                active.progress.errors,
            ),
        ),
        (None, Some(report)) => (
            if report.summary.cancelled {
                "Last scan cancelled"
            } else {
                "Last scan"
            },
            format!(
                "{} · {} files · {} logical size",
                report.root.display(),
                report.summary.progress.files,
                format_bytes(report.summary.progress.logical_bytes),
            ),
        ),
        (None, None) => (
            "Ready to scan",
            "Choose a folder to inspect its files. Scans are read-only.".into(),
        ),
    };
    let scan_button = match &scan.active {
        Some(active) => Button::new("cancel-scan")
            .label("Cancel scan")
            .disabled(active.cancelling)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.cancel_scan(cx))),
        None => Button::new("choose-folder")
            .primary()
            .label("Choose folder")
            .disabled(scan.choosing_folder)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.choose_scan_folder(cx))),
    };
    let results_button = Button::new("open-results")
        .ghost()
        .label(if scan.report.is_some() || viewing_saved_scan {
            "View results"
        } else {
            "View sample results"
        })
        .on_click(cx.listener(|workspace, _, _, cx| {
            workspace.page = Page::Results;
            cx.notify();
        }));

    div()
        .flex()
        .flex_col()
        .gap_5()
        .p_6()
        .child(div().text_xl().font_semibold().child("Overview"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .w(px(480.0))
                .max_w_full()
                .p_5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(div().font_semibold().child(heading))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
                .children(
                    scan.error
                        .as_ref()
                        .map(|error| div().text_sm().child(error.clone())),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(scan_button)
                        .child(results_button),
                ),
        )
}
