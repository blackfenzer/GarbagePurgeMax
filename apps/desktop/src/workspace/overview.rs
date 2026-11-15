use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    tooltip::Tooltip,
};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::{Page, Workspace, exclusions::path_label, scan::format_bytes};

pub(super) fn render(workspace: &Workspace, cx: &Context<Workspace>) -> impl IntoElement {
    let scan = &workspace.scan;
    let (heading, description) = match (&scan.active, &scan.report) {
        (Some(active), _) => (
            if active.cancelling {
                "Cancelling scan"
            } else if active.analyzing {
                "Checking duplicates"
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
            .disabled(scan.choosing_folder || workspace.exclusion_settings_busy())
            .on_click(cx.listener(|workspace, _, _, cx| workspace.choose_scan_folder(cx))),
    };
    let results_button = Button::new("open-results")
        .ghost()
        .label(if scan.report.is_some() || workspace.results.is_saved() {
            "View results"
        } else {
            "View sample results"
        })
        .on_click(cx.listener(|workspace, _, _, cx| {
            workspace.page = Page::Results;
            cx.notify();
        }));

    div()
        .id("overview")
        .size_full()
        .overflow_y_scroll()
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
        .child(exclusions_card(workspace, cx))
}

fn exclusions_card(workspace: &Workspace, cx: &Context<Workspace>) -> impl IntoElement {
    let exclusions = &workspace.exclusions;
    let disabled = workspace.exclusion_controls_disabled();
    let rows = exclusions.paths.iter().enumerate().map(|(index, path)| {
        let path_to_remove = path.clone();
        let label = path_label(path);
        let tooltip = label.clone();
        div()
            .flex()
            .items_center()
            .gap_3()
            .py_1()
            .child(
                div()
                    .id(("excluded-path", index))
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .text_ellipsis()
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(label),
            )
            .child(
                Button::new(("remove-exclusion", index))
                    .ghost()
                    .small()
                    .label("Remove")
                    .disabled(disabled)
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.remove_exclusion(path_to_remove.clone(), cx);
                    })),
            )
    });

    div()
        .flex()
        .flex_col()
        .gap_3()
        .w_full()
        .max_w(px(640.0))
        .p_5()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .child(div().font_semibold().child("Scan exclusions"))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("These files and folders are skipped in future scans."),
        )
        .children(exclusions.error.as_ref().map(|error| {
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone())
        }))
        .children(exclusions.busy.then(|| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Updating scan exclusions…")
        }))
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("exclude-folder")
                        .small()
                        .label("Add folder")
                        .disabled(disabled)
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.choose_exclusion(true, cx);
                        })),
                )
                .child(
                    Button::new("exclude-file")
                        .small()
                        .label("Add file")
                        .disabled(disabled)
                        .on_click(cx.listener(|workspace, _, _, cx| {
                            workspace.choose_exclusion(false, cx);
                        })),
                ),
        )
        .children(rows)
}
