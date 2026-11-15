mod overview;
mod results;
mod scan;

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
}

#[derive(Default)]
pub(crate) struct Workspace {
    page: Page,
    selected_file: Option<usize>,
    scan: scan::ScanSession,
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = SidebarMenu::new()
            .child(self.navigation_item(Page::Overview, "Overview", IconName::LayoutDashboard, cx))
            .child(self.navigation_item(Page::Results, "Scan Results", IconName::FolderOpen, cx));

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
            .child(div().flex_1().min_w(px(0.0)).child(match self.page {
                Page::Overview => overview::render(&self.scan, cx),
                Page::Results => results::render(self.scan.report.as_ref(), self.selected_file, cx),
            }))
    }
}

impl Workspace {
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
                cx.notify();
            }))
    }
}
