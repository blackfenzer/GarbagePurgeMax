//! Amber Deck: a top scan bar, two tabbed panels and a footer of graphs,
//! laid out like a music player's deck instead of a sidebar.

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::{AnyElement, Context, Div, IntoElement, ParentElement as _, Styled as _, div, px};

use super::{Page, Workspace, overview};

/// Left panel tabs: the views that explain where space goes.
const LEFT_TABS: [(Page, &str); 3] = [
    (Page::SpaceMap, "Map"),
    (Page::Overview, "Overview"),
    (Page::Review, "Review"),
];
/// Right panel tabs: the lists. `Workspace::page` chooses among these.
const RIGHT_TABS: [(Page, &str); 5] = [
    (Page::Results, "Files"),
    (Page::SpaceMap, "Tree"),
    (Page::Cleanup, "Queue"),
    (Page::Recovery, "Recovery"),
    (Page::History, "History"),
];

pub(super) fn render(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let right = if RIGHT_TABS.iter().any(|(page, _)| *page == workspace.page) {
        workspace.page
    } else {
        Page::Results
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .child(top_bar(workspace, cx))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(0.0))
                .gap_3()
                .p_3()
                .child(panel(workspace, Side::Left, workspace.deck_left, cx))
                .child(panel(workspace, Side::Right, right, cx)),
        )
        .child(workspace.space_map.footer(cx))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

fn top_bar(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let scan = &workspace.scan;
    let (heading, description) = overview::scan_status(workspace);
    let saved = workspace.results.saved_scan();
    let control = match &scan.active {
        Some(active) => Button::new("deck-cancel-scan")
            .small()
            .label("Cancel scan")
            .disabled(active.cancelling)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.cancel_scan(cx))),
        None => Button::new("deck-choose-folder")
            .small()
            .primary()
            .label("Choose folder")
            .disabled(scan.choosing_folder || workspace.exclusion_settings_busy())
            .on_click(cx.listener(|workspace, _, _, cx| workspace.choose_scan_folder(cx))),
    };
    let scan_again = Button::new("deck-scan-again")
        .small()
        .label("Scan again")
        .disabled(saved.is_none() || !workspace.can_start_scan())
        .on_click(cx.listener(|workspace, _, _, cx| {
            if let Some(root) = workspace.results.saved_scan().map(|scan| scan.root.clone()) {
                workspace.scan_again(root, cx);
            }
        }));

    let mut bar = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.title_bar)
        .child(div().font_semibold().child("Storage Clinic"))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(240.0))
                .child(div().font_semibold().child(heading))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .text_ellipsis()
                        .child(description),
                ),
        );
    if let Some(saved) = saved {
        bar = bar.child(chip(
            format!(
                "{} duplicate groups · {} similar-image groups",
                saved.duplicate_group_count, saved.similar_group_count
            ),
            cx,
        ));
    }
    bar.child(control)
        .child(scan_again)
        .child(workspace.theme_buttons(false, cx))
}

fn chip(text: String, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    div()
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border)
        .rounded(px(3.0))
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text)
}

fn panel(workspace: &Workspace, side: Side, current: Page, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    let tabs = match side {
        Side::Left => &LEFT_TABS[..],
        Side::Right => &RIGHT_TABS[..],
    };
    let mut strip = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border);
    for (position, (page, label)) in tabs.iter().copied().enumerate() {
        let id = match side {
            Side::Left => ("deck-left-tab", position),
            Side::Right => ("deck-right-tab", position),
        };
        strip = strip.child(
            Button::new(id)
                .small()
                .ghost()
                .label(label)
                .selected(page == current)
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    match side {
                        Side::Left => workspace.deck_left = page,
                        Side::Right => workspace.page = page,
                    }
                    workspace.load_page(page, cx);
                })),
        );
    }
    let body: AnyElement = match (side, current) {
        (Side::Left, Page::SpaceMap) => workspace.space_map.render_map(cx).into_any_element(),
        (Side::Right, Page::SpaceMap) => workspace.space_map.render_tree(cx).into_any_element(),
        (_, page) => workspace.page_view(page, cx),
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .border_1()
        .border_color(theme.border)
        .rounded(px(4.0))
        .bg(theme.sidebar)
        .overflow_hidden()
        .child(strip)
        .child(div().flex_1().min_h(px(0.0)).child(body))
}
