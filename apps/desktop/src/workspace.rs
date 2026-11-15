mod exclusions;
mod history;
mod overview;
mod results;
mod review;
mod scan;
mod storage;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, IconName,
    sidebar::{Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
};
use gpui_kit::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Overview,
    Results,
    History,
    Review,
}

pub(crate) struct Workspace {
    page: Page,
    exclusions: exclusions::Exclusions,
    results: results::Results,
    scan: scan::ScanSession,
    history: history::History,
    review: review::Review,
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = SidebarMenu::new()
            .child(self.navigation_item(Page::Overview, "Overview", IconName::LayoutDashboard, cx))
            .child(self.navigation_item(Page::Results, "Scan Results", IconName::FolderOpen, cx))
            .child(self.navigation_item(Page::Review, "Review", IconName::Copy, cx))
            .child(self.navigation_item(Page::History, "Scan History", IconName::Calendar, cx));

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
                    .child(SidebarGroup::new("Storage").child(menu)),
            )
            .child(
                div().flex_1().min_w(px(0.0)).child(match self.page {
                    Page::Overview => overview::render(self, cx).into_any_element(),
                    Page::Results => self
                        .results
                        .render(self.scan.report.as_ref(), cx)
                        .into_any_element(),
                    Page::History => self
                        .history
                        .render(self.can_start_scan(), cx)
                        .into_any_element(),
                    Page::Review => self.review.render(cx).into_any_element(),
                }),
            )
    }
}

impl Workspace {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: Page::default(),
            exclusions: exclusions::Exclusions::load(cx),
            results: results::Results::new(window, cx),
            scan: scan::ScanSession::default(),
            history: history::History::default(),
            review: review::Review::default(),
        }
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
                match page {
                    Page::History => workspace.history.load(0, cx),
                    Page::Review => {
                        let scan = workspace.results.saved_scan().cloned();
                        workspace.review.open(scan, cx);
                    }
                    Page::Overview | Page::Results => {}
                }
                cx.notify();
            }))
    }
}
