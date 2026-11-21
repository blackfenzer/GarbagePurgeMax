mod cleanup;
mod deck;
mod exclusions;
mod history;
mod overview;
mod results;
mod review;
mod scan;
mod space_map;
mod storage;
mod theme;
#[cfg(feature = "ui-smoke")]
pub(crate) mod ui_smoke;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _,
    button::Button,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};
use theme::AppTheme;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Overview,
    SpaceMap,
    Results,
    History,
    Review,
    Cleanup,
    Recovery,
}

pub(crate) struct Workspace {
    page: Page,
    exclusions: exclusions::Exclusions,
    results: results::Results,
    scan: scan::ScanSession,
    history: history::History,
    overview: overview::SavedOverview,
    review: review::Review,
    cleanup: cleanup::Cleanup,
    space_map: space_map::SpaceMap,
    theme: AppTheme,
    /// The page in Amber Deck's left panel; `page` drives its right panel.
    deck_left: Page,
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.theme == AppTheme::AmberDeck {
            // Amber Deck always shows Space Map, so follow the scan open in Scan Results.
            if self.space_map.scan_id() != self.results.saved_scan().map(|scan| scan.id) {
                let scan = self.results.saved_scan().cloned();
                self.space_map.open(scan, cx);
            }
            return deck::render(self, cx).into_any_element();
        }
        let menu = SidebarMenu::new()
            .child(self.navigation_item(Page::Overview, "Overview", IconName::LayoutDashboard, cx))
            .child(self.navigation_item(Page::SpaceMap, "Space Map", IconName::ChartPie, cx))
            .child(self.navigation_item(Page::Results, "Scan Results", IconName::FolderOpen, cx))
            .child(self.navigation_item(Page::Review, "Review", IconName::Copy, cx))
            .child(self.navigation_item(Page::History, "Scan History", IconName::Calendar, cx))
            .child(self.navigation_item(Page::Cleanup, "Cleanup Queue", IconName::FolderOpen, cx))
            .child(self.navigation_item(
                Page::Recovery,
                "Recovery History",
                IconName::Calendar,
                cx,
            ));

        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Sidebar::new("workspace-sidebar")
                    .w(px(184.0))
                    .header(
                        SidebarHeader::new().child(div().font_semibold().child("Storage Clinic")),
                    )
                    .child(SidebarGroup::new("Storage").child(menu))
                    .footer(self.theme_switch(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(self.page_view(self.page, cx)),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = AppTheme::saved();
        theme.apply(cx);
        let mut overview = overview::SavedOverview::default();
        overview.load(cx);
        Self {
            space_map: space_map::SpaceMap::default(),
            theme,
            deck_left: Page::SpaceMap,
            page: Page::default(),
            exclusions: exclusions::Exclusions::load(cx),
            results: results::Results::new(window, cx),
            scan: scan::ScanSession::default(),
            history: history::History::default(),
            overview,
            review: review::Review::default(),
            cleanup: cleanup::Cleanup::default(),
        }
    }

    /// One page's content. Both layouts show pages through this.
    fn page_view(&self, page: Page, cx: &Context<Self>) -> AnyElement {
        let cleanup_blocked = self.scan.active.is_some()
            || self.scan.choosing_folder
            || self.exclusion_settings_busy();
        match page {
            Page::Overview => overview::render(self, cx).into_any_element(),
            Page::SpaceMap => self.space_map.render(cx).into_any_element(),
            Page::Results => self
                .results
                .render(self.scan.report.as_ref(), cx)
                .into_any_element(),
            Page::History => self
                .history
                .render(self.can_start_scan(), cx)
                .into_any_element(),
            Page::Review => self.review.render(cx).into_any_element(),
            Page::Cleanup => self
                .cleanup
                .render(false, cleanup_blocked, cx)
                .into_any_element(),
            Page::Recovery => self
                .cleanup
                .render(true, cleanup_blocked, cx)
                .into_any_element(),
        }
    }

    /// Loads what `page` shows when it is opened.
    fn load_page(&mut self, page: Page, cx: &mut Context<Self>) {
        match page {
            Page::History => self.history.load(0, cx),
            Page::Review => {
                let scan = self.results.saved_scan().cloned();
                self.review.open(scan, cx);
            }
            Page::Overview => self.overview.load(cx),
            Page::SpaceMap => {
                let scan = self.results.saved_scan().cloned();
                self.space_map.open(scan, cx);
            }
            Page::Results => {}
            Page::Cleanup | Page::Recovery => self.cleanup.load(0, cx),
        }
        cx.notify();
    }

    /// The theme choices, stacked in the sidebar or in a row in Amber Deck's top bar.
    fn theme_buttons(&self, stacked: bool, cx: &Context<Self>) -> gpui_kit::Div {
        let mut choices = if stacked {
            div().flex().flex_col().gap_1().w_full()
        } else {
            div().flex().gap_1()
        };
        for theme in AppTheme::ALL {
            choices = choices.child(
                Button::new(theme.label())
                    .small()
                    .when(stacked, |button| button.w_full())
                    .label(theme.label())
                    .selected(self.theme == theme)
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.theme = theme;
                        theme.apply(cx);
                        cx.notify();
                    })),
            );
        }
        choices
    }

    fn theme_switch(&self, cx: &Context<Self>) -> SidebarFooter {
        SidebarFooter::new().child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w_full()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Theme"),
                )
                .child(self.theme_buttons(true, cx)),
        )
    }

    fn navigation_item(
        &self,
        page: Page,
        label: &'static str,
        icon: IconName,
        cx: &Context<Self>,
    ) -> SidebarMenuItem {
        SidebarMenuItem::new(label)
            .icon(icon)
            .active(self.page == page)
            .on_click(cx.listener(move |workspace, _, _, cx| {
                workspace.page = page;
                workspace.load_page(page, cx);
            }))
    }
}
