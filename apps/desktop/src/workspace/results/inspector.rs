use std::time::SystemTime;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    Context, Div, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div, px,
};
use scan_engine::{FileRecord, assess_risk, cleanup_candidate, file_type};
use scan_store::DuplicateEvidence;

use super::super::scan::format_bytes;
use super::Workspace;

pub(super) fn scanned_file(
    file: &FileRecord,
    threshold: u64,
    duplicate: Option<&DuplicateEvidence>,
    scanned_at: Option<SystemTime>,
    cx: &Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let name = file.path.file_name().unwrap_or(file.path.as_os_str());
    let risk = assess_risk(&file.path, file.read_only);
    let extension = file
        .path
        .extension()
        .map(|extension| extension.to_string_lossy());
    let mut content = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .font_semibold()
                .child(name.to_string_lossy().into_owned()),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(file.path.display().to_string()),
        )
        .child(
            div()
                .text_sm()
                .child(format!("{} logical size", format_bytes(file.logical_bytes))),
        )
        .child(div().text_sm().child(format!(
            "Type · {} (extension-based)",
            file_type(&file.path)
        )))
        .child(div().text_sm().child(format!(
            "Extension · {}",
            extension.as_deref().unwrap_or("None")
        )))
        .child(
            div()
                .text_sm()
                .child(format!("Modified · {}", timestamp(file.modified))),
        )
        .child(
            div()
                .text_sm()
                .child(format!("Created · {}", timestamp(file.created))),
        );

    content = content
        .child(div().text_sm().font_semibold().child(risk.tier.label()))
        .child(div().text_sm().text_color(muted).child(risk.reason));
    if file.read_only {
        content = content.child(div().text_sm().child("Read-only attribute set"));
    }
    if let Some(candidate) = scanned_at.and_then(|time| cleanup_candidate(file, time)) {
        content = content
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(format!("Review candidate · {}", candidate.kind.label())),
            )
            .child(div().text_sm().text_color(muted).child(candidate.evidence))
            .child(div().text_sm().text_color(muted).child(
                "Metadata evidence only; this does not establish that the file is safe to remove.",
            ));
    } else if scanned_at.is_none() {
        content = content.child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Cleanup evidence unavailable without a saved scan time."),
        );
    }
    if file.logical_bytes >= threshold {
        content = content
            .child(div().text_sm().font_semibold().child("Large file"))
            .child(div().text_sm().text_color(muted).child(format!(
                "The logical size meets the scan’s {} threshold.",
                format_bytes(threshold)
            )));
    }
    if let Some(duplicate) = duplicate {
        content = content
            .child(div().text_sm().font_semibold().child("Exact duplicate"))
            .child(div().text_sm().text_color(muted).child(format!(
                "{} distinct files had matching contents when scanned.",
                duplicate.file_count
            )))
            .child(div().text_xs().text_color(muted).child("SHA-256"))
            .children(duplicate.hash.chunks(16).map(|bytes| {
                div().text_xs().child(
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                )
            }));
    }
    content
}

pub(super) fn render(content: Option<Div>, cx: &Context<Workspace>) -> Div {
    let content = content.unwrap_or_else(|| {
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("Select a file to inspect its details.")
    });

    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap_4()
        .p_5()
        .border_color(cx.theme().border)
        .child(div().font_semibold().child("File Inspector"))
        .w(px(246.0))
        .border_l_1()
        .child(
            div()
                .id("file-inspector")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(content),
        )
}

fn timestamp(time: Option<SystemTime>) -> String {
    time.and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map(|time| time.strftime("%F %H:%M UTC").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
