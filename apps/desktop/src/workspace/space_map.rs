//! Space Map: a saved scan's folders by size, as a WizTree-style list and a treemap.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, div, img, px, relative,
};
use scan_engine::{VolumeSpace, volume_space};
use scan_store::{FolderEntry, FolderSummary, SavedScan};

use super::theme::{on_type_color, type_color};
use super::{Workspace, scan::format_bytes, storage};

/// Rows shown for one folder. A folder can hold tens of thousands of direct entries.
const MAX_ROWS: usize = 300;
/// Treemap blocks for one folder; smaller entries fold into the list only.
const MAX_BLOCKS: usize = 60;
const TREEMAP_WIDTH: f32 = 440.0;
const TREEMAP_HEIGHT: f32 = 320.0;
/// Largest folders whose own contents are drawn as sub-boxes inside their treemap block.
const NESTED_FOLDERS: usize = 12;
/// Sub-boxes drawn inside one block.
const NESTED_BLOCKS: usize = 24;

#[derive(Default)]
pub(super) struct SpaceMap {
    scan: Option<SavedScan>,
    pub(super) folder: PathBuf,
    pub(super) summary: Option<FolderSummary>,
    /// Position in `summary.entries` shown in the inspector.
    selected: Option<usize>,
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
    /// Logical bytes by last-modified year for the whole scan, for the Amber Deck footer.
    pub(super) years: Vec<(i32, u64)>,
    years_request: Option<Task<()>>,
    /// The scan volume's capacity and free space, for the footer's disk meter.
    disk: Option<VolumeSpace>,
    /// Sizes of the largest folders' own entries, by position in `summary.entries`.
    nested: HashMap<usize, Vec<FolderEntry>>,
    nested_request: Option<Task<()>>,
}

impl SpaceMap {
    /// Shows `scan` from its root, or keeps the current folder when it is already open.
    pub(super) fn open(&mut self, scan: Option<SavedScan>, cx: &mut Context<Workspace>) {
        let same_scan = self.scan.as_ref().map(|scan| scan.id) == scan.as_ref().map(|scan| scan.id);
        self.scan = scan;
        if !same_scan {
            self.load_years(cx);
        }
        match &self.scan {
            Some(scan) if !same_scan || self.summary.is_none() => {
                let root = scan.root.clone();
                self.open_folder(root, cx);
            }
            Some(_) => {}
            None => {
                self.request = None;
                self.summary = None;
                self.loading = false;
                cx.notify();
            }
        }
    }

    /// The id of the scan being shown, so another layout can tell when to reopen it.
    pub(super) fn scan_id(&self) -> Option<i64> {
        self.scan.as_ref().map(|scan| scan.id)
    }

    /// Loads the footer's whole-scan data: size by year and the volume's free space.
    fn load_years(&mut self, cx: &mut Context<Workspace>) {
        self.years.clear();
        self.disk = None;
        let Some(scan) = &self.scan else {
            self.years_request = None;
            return;
        };
        let scan_id = scan.id;
        let root = scan.root.clone();
        self.years_request = Some(cx.spawn(async move |workspace, cx| {
            let (years, disk) = cx
                .background_spawn(async move {
                    let years = storage::open()
                        .and_then(|store| store.modified_years(scan_id).map_err(io::Error::other));
                    (years, volume_space(&root).ok())
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                if workspace.space_map.scan_id() != Some(scan_id) {
                    return;
                }
                workspace.space_map.disk = disk;
                match years {
                    Ok(years) => workspace.space_map.years = years,
                    Err(error) => log::warn!("Could not total modified years: {error}"),
                }
                cx.notify();
            });
        }));
    }

    fn open_folder(&mut self, folder: PathBuf, cx: &mut Context<Workspace>) {
        let Some(scan) = &self.scan else {
            return;
        };
        let scan_id = scan.id;
        self.folder = folder.clone();
        self.summary = None;
        self.nested.clear();
        self.nested_request = None;
        self.selected = None;
        self.error = None;
        self.loading = true;
        self.request = Some(cx.spawn(async move |workspace, cx| {
            let target = folder.clone();
            let result = cx
                .background_spawn(async move {
                    storage::open()?
                        .folder_summary(scan_id, &target)
                        .map_err(io::Error::other)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let map = &mut workspace.space_map;
                if map.folder != folder {
                    return;
                }
                map.loading = false;
                match result {
                    Ok(summary) => {
                        map.summary = Some(summary);
                        map.load_nested(cx);
                    }
                    Err(error) => map.error = Some(format!("Could not total folders: {error}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Totals the largest folders' own entries in the background, so their treemap blocks
    /// can show what is inside them. One query per folder, at most `NESTED_FOLDERS`.
    fn load_nested(&mut self, cx: &mut Context<Workspace>) {
        let (Some(scan_id), Some(summary)) = (self.scan_id(), &self.summary) else {
            return;
        };
        let folder = self.folder.clone();
        let folders: Vec<(usize, PathBuf)> = summary
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.is_folder)
            .take(NESTED_FOLDERS)
            .map(|(index, entry)| (index, folder.join(&entry.name)))
            .collect();
        self.nested_request = Some(cx.spawn(async move |workspace, cx| {
            let nested = cx
                .background_spawn(async move {
                    let store = storage::open()?;
                    let mut nested = HashMap::new();
                    for (index, path) in folders {
                        let summary = store
                            .folder_summary(scan_id, &path)
                            .map_err(io::Error::other)?;
                        let mut entries = summary.entries;
                        entries.truncate(NESTED_BLOCKS);
                        nested.insert(index, entries);
                    }
                    Ok::<_, io::Error>(nested)
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                let map = &mut workspace.space_map;
                if map.folder != folder {
                    return;
                }
                match nested {
                    Ok(nested) => map.nested = nested,
                    Err(error) => log::warn!("Could not total nested folders: {error}"),
                }
                cx.notify();
            });
        }));
    }

    /// Opens a folder entry, or shows a file entry in the inspector.
    pub(super) fn choose(&mut self, index: usize, cx: &mut Context<Workspace>) {
        let Some(entry) = self
            .summary
            .as_ref()
            .and_then(|summary| summary.entries.get(index))
        else {
            return;
        };
        if entry.is_folder {
            let folder = self.folder.join(&entry.name);
            self.open_folder(folder, cx);
        } else {
            self.selected = Some(index);
            cx.notify();
        }
    }

    /// The whole page: size list beside the treemap and inspector.
    pub(super) fn render(&self, cx: &Context<Workspace>) -> Div {
        let (page, loaded) = self.frame(cx);
        let Some((scan, summary)) = loaded else {
            return page;
        };
        page.child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .flex_1()
                .min_h(px(0.0))
                .child(self.size_list(summary, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .w(px(TREEMAP_WIDTH))
                        .child(self.treemap(
                            "space-map-block",
                            summary,
                            TREEMAP_WIDTH,
                            Some(TREEMAP_HEIGHT),
                            cx,
                        ))
                        .child(self.inspector(scan, summary, cx)),
                ),
        )
    }

    /// Treemap and inspector only, for Amber Deck's left panel.
    pub(super) fn render_map(&self, cx: &Context<Workspace>) -> Div {
        let (page, loaded) = self.frame(cx);
        let Some((scan, summary)) = loaded else {
            return page;
        };
        page.child(self.treemap(
            "space-map-block",
            summary,
            TREEMAP_WIDTH,
            Some(TREEMAP_HEIGHT),
            cx,
        ))
        .child(self.inspector(scan, summary, cx))
    }

    /// The size list only, for Amber Deck's right panel.
    pub(super) fn render_tree(&self, cx: &Context<Workspace>) -> Div {
        let (page, loaded) = self.frame(cx);
        let Some((_, summary)) = loaded else {
            return page;
        };
        page.child(self.size_list(summary, cx))
    }

    /// Amber Deck's footer, after the design: a transport row (logical size up to and after
    /// the selection, the cleanup queue, disk use), graphs that grow and shrink with the
    /// footer when its divider is dragged, and a caption naming the selection.
    pub(super) fn footer(&self, queue: (usize, u64), cx: &Context<Workspace>) -> Div {
        let theme = cx.theme();
        let summary = self
            .summary
            .as_ref()
            .filter(|summary| !summary.entries.is_empty());
        let caption = match (summary, self.selected) {
            (Some(summary), Some(index)) => {
                let entry = &summary.entries[index];
                format!(
                    "{} · {} · {} files",
                    self.folder.join(&entry.name).display(),
                    format_bytes(entry.logical_bytes),
                    entry.files
                )
            }
            (Some(summary), None) => format!(
                "{} · {} · {} files",
                self.folder.display(),
                format_bytes(summary.logical_bytes),
                summary.files
            ),
            _ if self.scan.is_none() => "No scan open. Choose a folder to scan.".into(),
            _ => "Totalling folders…".into(),
        };
        let label = |text: &'static str| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .text_ellipsis()
                .child(text)
        };
        let separator = || div().w(px(1.0)).h(px(20.0)).bg(theme.border);

        // Transport. With a file selected: this folder's entries played largest first up to
        // it. Otherwise: how much of the whole scan the open folder holds.
        let (played, total) = match (summary, self.selected) {
            (Some(summary), Some(index)) => (
                summary.entries[..=index]
                    .iter()
                    .map(|entry| entry.logical_bytes)
                    .sum(),
                summary.logical_bytes,
            ),
            (Some(summary), None) => (
                summary.logical_bytes,
                self.scan
                    .as_ref()
                    .map_or(summary.logical_bytes, |scan| scan.progress.logical_bytes),
            ),
            _ => (0, 0),
        };
        let position = fraction(played, total);
        let progress = div()
            .relative()
            .flex_1()
            .min_w(px(120.0))
            .h(px(6.0))
            .rounded(px(3.0))
            .bg(theme.border)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(position))
                    .bg(theme.primary),
            )
            .child(
                div()
                    .absolute()
                    .top(px(-4.0))
                    .left(relative(position))
                    .w(px(2.0))
                    .h(px(14.0))
                    .bg(theme.foreground),
            );
        let disk = self.disk.map(|disk| {
            let used = 1.0 - fraction(disk.available_bytes, disk.capacity_bytes);
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::HardDrive).small())
                .child(
                    div()
                        .w(px(90.0))
                        .h(px(4.0))
                        .bg(theme.muted)
                        .child(div().h_full().w(relative(used)).bg(theme.primary)),
                )
                .child(format!(
                    "{:.0}% used · {} free",
                    used * 100.0,
                    format_bytes(disk.available_bytes)
                ))
        });
        let transport = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .child(div().w(px(76.0)).child(format_bytes(played)))
            .child(progress)
            .child(
                div()
                    .w(px(76.0))
                    .text_right()
                    .child(format!("-{}", format_bytes(total.saturating_sub(played)))),
            )
            .child(separator())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_semibold()
                    .text_color(theme.primary)
                    .child(Icon::new(IconName::Inbox).small())
                    .child(format!(
                        "Queue · {} · {}",
                        super::scan::count(queue.0 as u64, "item"),
                        format_bytes(queue.1)
                    )),
            )
            .children(disk.map(|disk| {
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(separator())
                    .child(disk)
            }));

        // Size by last-modified year: bar heights are fractions of the graph area.
        let largest_year = self
            .years
            .iter()
            .map(|(_, bytes)| *bytes)
            .max()
            .unwrap_or(1);
        let shown_years = active_years(&self.years);
        let mut year_bars = div().flex().items_end().gap_1().flex_1().min_h(px(0.0));
        let mut year_labels = div().flex().gap_1();
        for (year, bytes) in &shown_years {
            year_bars = year_bars.child(
                div()
                    .flex_1()
                    .max_w(px(44.0))
                    .h(relative(fraction(*bytes, largest_year).max(0.03)))
                    .bg(theme.primary),
            );
            year_labels = year_labels.child(
                div()
                    .flex_1()
                    .max_w(px(44.0))
                    .text_xs()
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child(year.to_string()),
            );
        }

        // File types in this folder.
        let mut types = div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .min_h(px(0.0))
            .overflow_hidden();
        if let Some(summary) = summary {
            let largest = summary.types.first().map_or(1, |total| total.logical_bytes);
            for total in summary.types.iter().take(6) {
                types = types.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .size(px(8.0))
                                .flex_none()
                                .bg(type_color(total.file_type)),
                        )
                        .child(div().w(px(96.0)).text_ellipsis().child(total.file_type))
                        .child(
                            div().flex_1().h(px(6.0)).bg(theme.muted).child(
                                div()
                                    .h_full()
                                    .w(relative(fraction(total.logical_bytes, largest)))
                                    .bg(type_color(total.file_type)),
                            ),
                        )
                        .child(
                            div()
                                .w(px(64.0))
                                .text_right()
                                .text_color(theme.muted_foreground)
                                .child(format_bytes(total.logical_bytes)),
                        ),
                );
            }
        }

        // The design's waveform: entries by size, centred, played up to the selection.
        let mut wave = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .flex_1()
            .min_h(px(0.0));
        if let Some(summary) = summary {
            let largest = summary.entries[0].logical_bytes.max(1);
            for (index, entry) in summary.entries.iter().take(MAX_BLOCKS).enumerate() {
                let played = self.selected.is_some_and(|selected| index <= selected);
                wave = wave.child(
                    div()
                        .id(("space-map-strip", index))
                        .flex_1()
                        .min_w(px(3.0))
                        .h(relative(fraction(entry.logical_bytes, largest).max(0.06)))
                        .rounded(px(1.0))
                        .bg(if self.selected == Some(index) {
                            theme.foreground
                        } else {
                            theme.primary
                        })
                        .opacity(if played || self.selected.is_none() {
                            1.0
                        } else {
                            0.4
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.space_map.choose(index, cx);
                        })),
                );
            }
        }

        let column = |width: Option<f32>| {
            let column = div()
                .flex()
                .flex_col()
                .gap_1()
                .min_h(px(0.0))
                .min_w(px(0.0));
            match width {
                Some(width) => column.w(px(width)).flex_shrink_0(),
                None => column.flex_1(),
            }
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .child(transport)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap_4()
                    .px_3()
                    .py_2()
                    .child(
                        column(Some(250.0))
                            .child(label("Size by last-modified year (UTC)"))
                            .child(year_bars)
                            .child(year_labels),
                    )
                    .child(
                        column(Some(240.0))
                            .child(label("Types in this folder"))
                            .child(types),
                    )
                    .child(
                        column(None)
                            .child(label(
                                "Entries by size · played up to the selection · click to open",
                            ))
                            .child(wave),
                    )
                    .when_some(summary, |row, summary| {
                        row.child(
                            div()
                                .w(px(170.0))
                                .h_full()
                                .flex_shrink_0()
                                .border_2()
                                .border_color(theme.primary)
                                .child(self.treemap(
                                    "space-map-mini-block",
                                    summary,
                                    170.0,
                                    None,
                                    cx,
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_center()
                    .line_clamp(1)
                    .child(caption),
            )
    }

    /// The header, breadcrumbs and loading state every Space Map view shares, plus the scan
    /// and folder totals once they are loaded and non-empty.
    fn frame(&self, cx: &Context<Workspace>) -> (Div, Option<(&SavedScan, &FolderSummary)>) {
        let muted = cx.theme().muted_foreground;
        let at_root = self
            .scan
            .as_ref()
            .is_none_or(|scan| self.folder == scan.root);
        let page = div().flex().flex_col().size_full().gap_3().p_6().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_xl().font_semibold().child("Space Map"))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("space-map-up")
                                .small()
                                .label("Up one folder")
                                .disabled(self.loading || at_root)
                                .on_click(cx.listener(|workspace, _, _, cx| {
                                    let map = &mut workspace.space_map;
                                    if let Some(parent) = map.folder.parent() {
                                        let parent = parent.to_path_buf();
                                        map.open_folder(parent, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new("space-map-refresh")
                                .small()
                                .label("Refresh")
                                .disabled(self.loading || self.scan.is_none())
                                .on_click(cx.listener(|workspace, _, _, cx| {
                                    let folder = workspace.space_map.folder.clone();
                                    workspace.space_map.open_folder(folder, cx);
                                })),
                        ),
                ),
        );
        let Some(scan) = &self.scan else {
            let hint = div().text_sm().text_color(muted).child(
                "Run a scan, or open one from Scan History, to see which folders use the most space.",
            );
            return (page.child(hint), None);
        };

        let page =
            page.child(self.breadcrumbs(scan, cx))
                .child(div().text_sm().text_color(muted).child(
                    "Logical sizes from the saved scan, not a live view. Nothing here moves files: \
                 use Review and Cleanup Queue for that.",
                ));
        let status = |text: String, color| div().text_sm().text_color(color).child(text);
        if self.loading {
            return (page.child(status("Totalling folders…".into(), muted)), None);
        }
        if let Some(error) = &self.error {
            return (page.child(status(error.clone(), cx.theme().danger)), None);
        }
        match &self.summary {
            Some(summary) if !summary.entries.is_empty() => (page, Some((scan, summary))),
            Some(_) => (
                page.child(status("No indexed files in this folder.".into(), muted)),
                None,
            ),
            None => (page, None),
        }
    }

    /// Drill-down path: the scan root, then each folder below it, separated by `›`.
    /// The full path wraps underneath so long names stay readable.
    fn breadcrumbs(&self, scan: &SavedScan, cx: &Context<Workspace>) -> Div {
        let theme = cx.theme();
        let mut folders: Vec<&Path> = self
            .folder
            .ancestors()
            .take_while(|folder| folder.starts_with(&scan.root))
            .collect();
        folders.reverse();
        let last = folders.len().saturating_sub(1);
        let mut crumbs = div().flex().flex_wrap().items_center().gap_1();
        for (position, folder) in folders.into_iter().enumerate() {
            let label = folder.file_name().map_or_else(
                || folder.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            let icon = if position == 0 {
                IconName::HardDrive
            } else {
                IconName::Folder
            };
            if position > 0 {
                crumbs = crumbs.child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(theme.muted_foreground),
                );
            }
            crumbs = crumbs.child(if position == last {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(px(4.0))
                    .bg(theme.secondary)
                    .text_color(theme.primary)
                    .font_semibold()
                    .child(Icon::new(icon).small())
                    .child(label)
                    .into_any_element()
            } else {
                let target = folder.to_path_buf();
                Button::new(("space-map-crumb", position))
                    .small()
                    .ghost()
                    .icon(Icon::new(icon))
                    .label(label)
                    .disabled(self.loading)
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.space_map.open_folder(target.clone(), cx);
                    }))
                    .into_any_element()
            });
        }
        div().flex().flex_col().gap_1().child(crumbs).child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .line_clamp(2)
                .child(self.folder.display().to_string()),
        )
    }

    fn size_list(&self, summary: &FolderSummary, cx: &Context<Workspace>) -> impl IntoElement {
        let theme = cx.theme();
        let header = div()
            .flex()
            .gap_3()
            .px_3()
            .py_1()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().child("Name"))
            .child(div().w(px(90.0)).text_right().child("Size"))
            .child(div().w(px(120.0)).child("% of folder"))
            .child(div().w(px(64.0)).text_right().child("Files"))
            .child(div().w(px(24.0)));
        let mut rows = div()
            .id("space-map-rows")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll();
        for (index, entry) in summary.entries.iter().take(MAX_ROWS).enumerate() {
            let share = fraction(entry.logical_bytes, summary.logical_bytes);
            let selected = self.selected == Some(index);
            let name = entry.name.clone();
            rows = rows.child(
                div()
                    .id(("space-map-row", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .min_h(px(30.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.list_active))
                    .hover(|row| row.bg(theme.list_hover))
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.space_map.choose(index, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                img(crate::file_icons::icon_path(&entry.name, entry.is_folder))
                                    .size(px(16.0))
                                    .flex_none(),
                            )
                            // Without a zero minimum width a long name pushes into the Size column.
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .when(entry.is_folder, |name| name.font_semibold())
                                    .child(entry_label(entry)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(90.0))
                            .text_right()
                            .child(format_bytes(entry.logical_bytes)),
                    )
                    .child(
                        div()
                            .w(px(120.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(8.0))
                                    .bg(theme.muted)
                                    .child(div().h_full().w(relative(share)).bg(theme.primary)),
                            )
                            .child(
                                div()
                                    .w(px(48.0))
                                    .text_right()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{:.1}%", share * 100.0)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(64.0))
                            .text_right()
                            .text_color(theme.muted_foreground)
                            .child(entry.files.to_string()),
                    )
                    .child(
                        Button::new(("space-map-trash", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Trash))
                            .tooltip("Move to Trash")
                            .on_click(cx.listener(move |workspace, _, _, cx| {
                                cx.stop_propagation();
                                if let Some(scan) = workspace.space_map.scan.clone() {
                                    let path = workspace.space_map.folder.join(&name);
                                    workspace.ask_trash(scan, path, cx);
                                }
                            })),
                    ),
            );
        }
        if summary.entries.len() > MAX_ROWS {
            rows = rows.child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{} smaller items not shown. Scan Results lists every file.",
                        summary.entries.len() - MAX_ROWS
                    )),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(340.0))
            .min_h(px(0.0))
            .text_sm()
            .border_1()
            .border_color(theme.border)
            .child(header)
            .child(rows)
    }

    /// A clickable treemap laid out for `width` × `height`; it stretches to its container's width.
    /// `id` keeps block ids distinct when one window shows two treemaps.
    fn treemap(
        &self,
        id: &'static str,
        summary: &FolderSummary,
        width: f32,
        fixed_height: Option<f32>,
        cx: &Context<Workspace>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let entries = &summary.entries[..summary.entries.len().min(MAX_BLOCKS)];
        let sizes: Vec<u64> = entries.iter().map(|entry| entry.logical_bytes).collect();
        // Without a fixed height the map fills its container; lay it out for a typical one.
        let height = fixed_height.unwrap_or(width * 0.7);
        let map = div().relative().w_full().bg(theme.muted).overflow_hidden();
        let mut map = match fixed_height {
            // Keep its height inside a crowded column instead of shrinking away.
            Some(height) => map.h(px(height)).flex_shrink_0(),
            None => map.h_full(),
        };
        for block in treemap(&sizes, width / height) {
            let entry = &entries[block.index];
            let index = block.index;
            let (block_width, block_height) = (block.width * width, block.height * height);
            let label = BoxLabel::fits(block_width, block_height);
            let nested = self
                .nested
                .get(&index)
                .filter(|_| block_width >= 56.0 && block_height >= 72.0)
                .map(|entries| nested_blocks(entries, block_width, block_height, label));
            map = map.child(
                div()
                    .id((id, index))
                    .absolute()
                    .left(relative(block.x))
                    .top(relative(block.y))
                    .w(relative(block.width))
                    .h(relative(block.height))
                    .bg(type_color(entry.main_type))
                    .border_1()
                    .border_color(theme.background)
                    .when(self.selected == Some(index), |block| {
                        block.border_2().border_color(theme.foreground)
                    })
                    .p_1()
                    .overflow_hidden()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(on_type_color())
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.space_map.choose(index, cx);
                    }))
                    .child(label.render(entry, true))
                    .children(nested),
            );
        }
        map
    }

    fn inspector(
        &self,
        scan: &SavedScan,
        summary: &FolderSummary,
        cx: &Context<Workspace>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let selected = self.selected.and_then(|index| summary.entries.get(index));
        let (name, path, bytes, files) = match selected {
            Some(entry) => (
                entry_label(entry),
                self.folder.join(&entry.name),
                entry.logical_bytes,
                entry.files,
            ),
            None => (
                self.folder.file_name().map_or_else(
                    || self.folder.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                self.folder.clone(),
                summary.logical_bytes,
                summary.files,
            ),
        };
        let fact = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p_2()
                .bg(theme.muted)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(div().font_semibold().child(value))
        };
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .child(div().font_semibold().text_ellipsis().child(name))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .line_clamp(3)
                    .child(path.display().to_string()),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(fact("Logical size", format_bytes(bytes)))
                    .child(fact("Files", files.to_string()))
                    .child(fact(
                        "Of scan",
                        format!(
                            "{:.1}%",
                            fraction(bytes, scan.progress.logical_bytes) * 100.0
                        ),
                    )),
            );
        // The scan root itself can't be moved; anything inside it can.
        if path != scan.root {
            let scan = scan.clone();
            panel = panel.child(
                div().flex().child(
                    Button::new("space-map-inspector-trash")
                        .small()
                        .outline()
                        .icon(Icon::new(IconName::Trash))
                        .label("Move to Trash")
                        .on_click(cx.listener(move |workspace, _, _, cx| {
                            workspace.ask_trash(scan.clone(), path.clone(), cx);
                        })),
                ),
            );
        }
        if selected.is_none() {
            let largest = summary.types.first().map_or(1, |total| total.logical_bytes);
            for total in summary.types.iter().take(5) {
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .child(div().size(px(8.0)).bg(type_color(total.file_type)))
                        .child(div().w(px(150.0)).child(total.file_type))
                        .child(
                            div().flex_1().h(px(6.0)).bg(theme.muted).child(
                                div()
                                    .h_full()
                                    .w(relative(fraction(total.logical_bytes, largest)))
                                    .bg(type_color(total.file_type)),
                            ),
                        )
                        .child(
                            div()
                                .w(px(70.0))
                                .text_right()
                                .text_color(theme.muted_foreground)
                                .child(format_bytes(total.logical_bytes)),
                        ),
                );
            }
        }
        panel
    }
}

/// How much of an entry's name a treemap box can show, step by step as it gets smaller:
/// name and size, then the name, then only its file-type icon, then nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoxLabel {
    NameAndSize,
    Name,
    Icon,
    Blank,
}

impl BoxLabel {
    fn fits(width: f32, height: f32) -> Self {
        if width >= 70.0 && height >= 34.0 {
            Self::NameAndSize
        } else if width >= 44.0 && height >= 18.0 {
            Self::Name
        } else if width >= 18.0 && height >= 18.0 {
            Self::Icon
        } else {
            Self::Blank
        }
    }

    /// Room the label takes at the top of its box.
    fn height(self) -> f32 {
        match self {
            Self::NameAndSize => 44.0,
            Self::Name => 24.0,
            Self::Icon | Self::Blank => 3.0,
        }
    }

    fn render(self, entry: &FolderEntry, bold: bool) -> gpui_kit::AnyElement {
        let name = || {
            div()
                .text_ellipsis()
                .when(bold, |name| name.font_semibold())
                .child(entry_label(entry))
        };
        match self {
            Self::NameAndSize => div()
                .child(name())
                .child(format_bytes(entry.logical_bytes))
                .into_any_element(),
            Self::Name => name().into_any_element(),
            Self::Icon => img(crate::file_icons::icon_path(&entry.name, entry.is_folder))
                .size(px(14.0))
                .into_any_element(),
            Self::Blank => div().into_any_element(),
        }
    }
}

/// A folder's own entries as sub-boxes inside its block, under its label. Each is shaded
/// by size, the largest lightest, and labelled as far as its size allows.
fn nested_blocks(
    entries: &[FolderEntry],
    width: f32,
    height: f32,
    label: BoxLabel,
) -> impl IntoElement {
    let top = label.height();
    let (inner_width, inner_height) = (width - 6.0, (height - top - 3.0).max(1.0));
    let sizes: Vec<u64> = entries.iter().map(|entry| entry.logical_bytes).collect();
    let largest = sizes.iter().copied().max().unwrap_or(1).max(1);
    let mut area = div()
        .absolute()
        .left(px(3.0))
        .right(px(3.0))
        .bottom(px(3.0))
        .top(px(top));
    for block in treemap(&sizes, inner_width / inner_height) {
        let entry = &entries[block.index];
        // Kept light enough that the label ink stays readable on either palette.
        let shade = 0.3 * (1.0 - fraction(entry.logical_bytes, largest));
        let label = BoxLabel::fits(block.width * inner_width, block.height * inner_height);
        area = area.child(
            div()
                .absolute()
                .left(relative(block.x))
                .top(relative(block.y))
                .w(relative(block.width))
                .h(relative(block.height))
                .p(px(2.0))
                .overflow_hidden()
                .bg(gpui_kit::hsla(0.0, 0.0, 0.0, shade))
                .border_1()
                .border_color(gpui_kit::hsla(0.0, 0.0, 0.0, 0.35))
                .child(label.render(entry, false)),
        );
    }
    area
}

/// The years worth a bar: at least 2% of the busiest year's bytes, the latest ten of them,
/// so a few stray old files don't stretch the graph across decades.
fn active_years(years: &[(i32, u64)]) -> Vec<&(i32, u64)> {
    let busiest = years.iter().map(|(_, bytes)| *bytes).max().unwrap_or(0);
    let mut active: Vec<_> = years
        .iter()
        .filter(|(_, bytes)| *bytes * 50 >= busiest)
        .collect();
    let skip = active.len().saturating_sub(10);
    active.drain(..skip);
    active
}

fn entry_label(entry: &FolderEntry) -> String {
    entry.name.to_string_lossy().into_owned()
}

fn fraction(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) as f32
    }
}

/// A treemap block, in fractions of the map's width and height.
#[derive(Debug)]
struct Block {
    index: usize,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Squarified treemap of `sizes`, which must be sorted largest first. Zero sizes get no block.
fn treemap(sizes: &[u64], aspect: f32) -> Vec<Block> {
    let total: f64 = sizes.iter().map(|&size| size as f64).sum();
    if total == 0.0 {
        return Vec::new();
    }
    let aspect = f64::from(aspect);
    let (mut x, mut y, mut width, mut height) = (0.0, 0.0, aspect, 1.0);
    let areas: Vec<(usize, f64)> = sizes
        .iter()
        .enumerate()
        .filter(|(_, size)| **size > 0)
        .map(|(index, &size)| (index, size as f64 / total * aspect))
        .collect();
    let mut blocks = Vec::with_capacity(areas.len());
    let mut start = 0;
    while start < areas.len() {
        let side = f64::min(width, height);
        let mut end = start + 1;
        let mut best = worst_ratio(&areas[start..end], side);
        while end < areas.len() {
            let next = worst_ratio(&areas[start..=end], side);
            if next > best {
                break;
            }
            best = next;
            end += 1;
        }
        let row = &areas[start..end];
        let thickness = row.iter().map(|(_, area)| area).sum::<f64>() / side;
        let mut offset = 0.0;
        for &(index, area) in row {
            let length = area / thickness;
            let (left, top, block_width, block_height) = if width >= height {
                (x, y + offset, thickness, length)
            } else {
                (x + offset, y, length, thickness)
            };
            blocks.push(Block {
                index,
                x: (left / aspect) as f32,
                y: top as f32,
                width: (block_width / aspect) as f32,
                height: block_height as f32,
            });
            offset += length;
        }
        if width >= height {
            x += thickness;
            width -= thickness;
        } else {
            y += thickness;
            height -= thickness;
        }
        start = end;
    }
    blocks
}

/// The worst aspect ratio in a row laid along `side`; lower is squarer.
fn worst_ratio(row: &[(usize, f64)], side: f64) -> f64 {
    let sum: f64 = row.iter().map(|(_, area)| area).sum();
    let largest = row.iter().map(|(_, area)| *area).fold(0.0, f64::max);
    let smallest = row
        .iter()
        .map(|(_, area)| *area)
        .fold(f64::INFINITY, f64::min);
    f64::max(
        side * side * largest / (sum * sum),
        sum * sum / (side * side * smallest),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_labels_shrink_step_by_step_and_years_skip_stray_files() {
        assert_eq!(BoxLabel::fits(120.0, 60.0), BoxLabel::NameAndSize);
        assert_eq!(BoxLabel::fits(60.0, 20.0), BoxLabel::Name);
        assert_eq!(BoxLabel::fits(24.0, 24.0), BoxLabel::Icon);
        assert_eq!(BoxLabel::fits(10.0, 40.0), BoxLabel::Blank);
        let mut years: Vec<(i32, u64)> = (2000..2026).map(|year| (year, 1_000)).collect();
        years[0].1 = 10;
        let active = active_years(&years);
        assert_eq!(active.len(), 10);
        assert_eq!(active.first().map(|(year, _)| *year), Some(2016));
        assert!(
            active_years(&[(1999, 1), (2024, 1_000)])
                .iter()
                .all(|(year, _)| *year == 2024)
        );
    }

    #[test]
    fn treemap_tiles_the_whole_map_without_overlap() {
        let sizes = [400, 300, 120, 80, 50, 30, 15, 5, 0];
        let blocks = treemap(&sizes, 1.4);
        assert_eq!(blocks.len(), 8, "zero-sized entries get no block");
        let area: f32 = blocks.iter().map(|block| block.width * block.height).sum();
        assert!((area - 1.0).abs() < 1e-4, "blocks cover the map: {area}");
        for block in &blocks {
            assert!(block.x >= -1e-6 && block.y >= -1e-6);
            assert!(block.x + block.width <= 1.0 + 1e-4 && block.y + block.height <= 1.0 + 1e-4);
            let share = sizes[block.index] as f32 / 1000.0;
            assert!((block.width * block.height - share).abs() < 1e-4);
        }
        for (position, first) in blocks.iter().enumerate() {
            for second in &blocks[position + 1..] {
                let overlap_x =
                    (first.x + first.width).min(second.x + second.width) - first.x.max(second.x);
                let overlap_y =
                    (first.y + first.height).min(second.y + second.height) - first.y.max(second.y);
                assert!(
                    overlap_x <= 1e-4 || overlap_y <= 1e-4,
                    "{first:?} overlaps {second:?}"
                );
            }
        }
        assert!(treemap(&[0, 0], 1.0).is_empty());
    }
}
