//! Amber Deck: a top scan bar, two tabbed panels and a footer of graphs,
//! laid out like a music player's deck instead of a sidebar.

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::{Page, Workspace, overview, scan::count};

/// Left panel tabs: the views that explain where space goes.
pub(super) const LEFT_TABS: [(Page, &str, IconName); 3] = [
    (Page::SpaceMap, "Map", IconName::Map),
    (Page::Overview, "Overview", IconName::LayoutDashboard),
    (Page::Review, "Review", IconName::Copy),
];
/// Right panel tabs: the lists.
const RIGHT_TABS: [(Page, &str, IconName); 5] = [
    (Page::Results, "Files", IconName::Files),
    (Page::SpaceMap, "Tree", IconName::ListTree),
    (Page::Cleanup, "Queue", IconName::Inbox),
    (Page::Recovery, "Recovery", IconName::ArchiveRestore),
    (Page::History, "History", IconName::Clock),
];

pub(super) fn render(workspace: &Workspace, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .child(top_bar(workspace, cx))
        .child(
            // Drag the dividers to resize the panels and the footer; sizes last for the session.
            div().flex_1().min_h(px(0.0)).child(
                v_resizable("deck-rows")
                    .child(
                        resizable_panel().child(
                            div().size_full().p_3().child(
                                h_resizable("deck-panels")
                                    .child(
                                        resizable_panel().size_range(px(320.0)..px(4000.0)).child(
                                            panel(workspace, Side::Left, workspace.deck_left, cx),
                                        ),
                                    )
                                    .child(
                                        resizable_panel().size_range(px(360.0)..px(4000.0)).child(
                                            panel(workspace, Side::Right, workspace.deck_right, cx),
                                        ),
                                    ),
                            ),
                        ),
                    )
                    .child(
                        resizable_panel()
                            .size(px(170.0))
                            .size_range(px(96.0)..px(480.0))
                            .child(workspace.space_map.footer(cx)),
                    ),
            ),
        )
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
            .icon(Icon::new(IconName::CircleStop))
            .label("Cancel scan")
            .disabled(active.cancelling)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.cancel_scan(cx))),
        None => Button::new("deck-choose-folder")
            .primary()
            .icon(Icon::new(IconName::FolderOpen))
            .label("Choose folder")
            .disabled(!workspace.can_start_scan())
            .on_click(cx.listener(|workspace, _, _, cx| workspace.choose_scan_folder(cx))),
    };
    let scan_again = Button::new("deck-scan-again")
        .outline()
        .icon(Icon::new(IconName::RotateCcw))
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
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.title_bar)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .font_semibold()
                .child(Icon::new(IconName::HardDrive).text_color(theme.primary))
                .child("Storage Clinic"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w(px(240.0))
                .child(div().font_semibold().child(heading))
                // Long paths wrap onto a second line instead of running off the bar.
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_clamp(2)
                        .child(description),
                ),
        );
    if let Some(saved) = saved {
        bar = bar
            .child(chip(
                IconName::Copy,
                count(saved.duplicate_group_count, "duplicate group"),
                cx,
            ))
            .child(chip(
                IconName::Images,
                count(saved.similar_group_count, "similar-image group"),
                cx,
            ));
    }
    bar.child(control)
        .child(scan_again)
        .child(workspace.theme_buttons(false, cx))
}

fn chip(icon: IconName, text: String, cx: &Context<Workspace>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border)
        .rounded(px(4.0))
        .text_xs()
        .child(Icon::new(icon).small().text_color(theme.primary))
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
        .items_end()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.title_bar);
    for (position, (page, label, icon)) in tabs.iter().cloned().enumerate() {
        let active = page == current;
        let id = match side {
            Side::Left => ("deck-left-tab", position),
            Side::Right => ("deck-right-tab", position),
        };
        // The active tab is amber with an underline; others are muted until hovered.
        strip = strip.child(
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .min_h(px(36.0))
                .border_b_2()
                .cursor_pointer()
                .map(|tab| {
                    if active {
                        tab.border_color(theme.primary)
                            .text_color(theme.primary)
                            .font_semibold()
                    } else {
                        tab.border_color(gpui_kit::transparent_black())
                            .text_color(theme.muted_foreground)
                            .hover(|tab| tab.text_color(theme.foreground).bg(theme.secondary))
                    }
                })
                .child(Icon::new(icon).small())
                .child(label)
                .on_click(cx.listener(move |workspace, _, _, cx| {
                    workspace.page = page;
                    match side {
                        Side::Left => workspace.deck_left = page,
                        Side::Right => workspace.deck_right = page,
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
        .size_full()
        .min_w(px(0.0))
        .border_1()
        .border_color(theme.border)
        .rounded(px(6.0))
        .bg(theme.sidebar)
        .overflow_hidden()
        .child(strip)
        // The panel scrolls; the page inside takes its natural height, so long maps, inspectors
        // and tables extend the scroll instead of being clipped or squeezed.
        .child(
            div()
                .id(match side {
                    Side::Left => "deck-left-body",
                    Side::Right => "deck-right-body",
                })
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(div().w_full().child(body)),
        )
}
