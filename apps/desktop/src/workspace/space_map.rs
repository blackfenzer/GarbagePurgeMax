//! Space Map: a saved scan's folders by size, as a WizTree-style list and a treemap.

use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, button::Button,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, div, px, relative,
};
use scan_store::{FolderEntry, FolderSummary, SavedScan};

use super::theme::{on_type_color, type_color};
use super::{Workspace, scan::format_bytes, storage};

/// Rows shown for one folder. A folder can hold tens of thousands of direct entries.
const MAX_ROWS: usize = 300;
/// Treemap blocks for one folder; smaller entries fold into the list only.
const MAX_BLOCKS: usize = 60;
const TREEMAP_WIDTH: f32 = 440.0;
const TREEMAP_HEIGHT: f32 = 320.0;

#[derive(Default)]
pub(super) struct SpaceMap {
    scan: Option<SavedScan>,
    folder: PathBuf,
    summary: Option<FolderSummary>,
    /// Position in `summary.entries` shown in the inspector.
    selected: Option<usize>,
    loading: bool,
    error: Option<String>,
    request: Option<Task<()>>,
}

impl SpaceMap {
    /// Shows `scan` from its root, or keeps the current folder when it is already open.
    pub(super) fn open(&mut self, scan: Option<SavedScan>, cx: &mut Context<Workspace>) {
        let same_scan = self.scan.as_ref().map(|scan| scan.id) == scan.as_ref().map(|scan| scan.id);
        self.scan = scan;
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

    fn open_folder(&mut self, folder: PathBuf, cx: &mut Context<Workspace>) {
        let Some(scan) = &self.scan else {
            return;
        };
        let scan_id = scan.id;
        self.folder = folder.clone();
        self.summary = None;
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
                    Ok(summary) => map.summary = Some(summary),
                    Err(error) => map.error = Some(format!("Could not total folders: {error}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens a folder entry, or shows a file entry in the inspector.
    fn choose(&mut self, index: usize, cx: &mut Context<Workspace>) {
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

    pub(super) fn render(&self, cx: &Context<Workspace>) -> Div {
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
            return page.child(div().text_sm().text_color(muted).child(
                "Run a scan, or open one from Scan History, to see which folders use the most space.",
            ));
        };

        let page =
            page.child(self.breadcrumbs(scan, cx))
                .child(div().text_sm().text_color(muted).child(
                    "Logical sizes from the saved scan, not a live view. Nothing here moves files: \
                 use Review and Cleanup Queue for that.",
                ));
        if self.loading {
            return page.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Totalling folders…"),
            );
        }
        if let Some(error) = &self.error {
            return page.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        let Some(summary) = &self.summary else {
            return page;
        };
        if summary.entries.is_empty() {
            return page.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("No indexed files in this folder."),
            );
        }

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
                        .child(self.treemap(summary, cx))
                        .child(self.inspector(scan, summary, cx)),
                ),
        )
    }

    fn breadcrumbs(&self, scan: &SavedScan, cx: &Context<Workspace>) -> Div {
        let mut folders: Vec<&Path> = self
            .folder
            .ancestors()
            .take_while(|folder| folder.starts_with(&scan.root))
            .collect();
        folders.reverse();
        let last = folders.len().saturating_sub(1);
        let mut crumbs = div().flex().flex_wrap().items_center().gap_1();
        for (position, folder) in folders.into_iter().enumerate() {
            let label = if position == 0 {
                folder.display().to_string()
            } else {
                folder
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            };
            let target = folder.to_path_buf();
            crumbs = crumbs.child(
                Button::new(("space-map-crumb", position))
                    .small()
                    .label(label)
                    .selected(position == last)
                    .disabled(self.loading)
                    .on_click(cx.listener(move |workspace, _, _, cx| {
                        workspace.space_map.open_folder(target.clone(), cx);
                    })),
            );
        }
        crumbs
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
            .child(div().w(px(170.0)).child("% of folder"))
            .child(div().w(px(80.0)).text_right().child("Files"));
        let mut rows = div()
            .id("space-map-rows")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll();
        for (index, entry) in summary.entries.iter().take(MAX_ROWS).enumerate() {
            let share = fraction(entry.logical_bytes, summary.logical_bytes);
            let selected = self.selected == Some(index);
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
                            .child(div().size(px(10.0)).bg(type_color(entry.main_type)))
                            .child(
                                div()
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
                            .w(px(170.0))
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
                            .w(px(80.0))
                            .text_right()
                            .text_color(theme.muted_foreground)
                            .child(entry.files.to_string()),
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
            .min_w(px(460.0))
            .min_h(px(0.0))
            .text_sm()
            .border_1()
            .border_color(theme.border)
            .child(header)
            .child(rows)
    }

    fn treemap(&self, summary: &FolderSummary, cx: &Context<Workspace>) -> impl IntoElement {
        let theme = cx.theme();
        let entries = &summary.entries[..summary.entries.len().min(MAX_BLOCKS)];
        let sizes: Vec<u64> = entries.iter().map(|entry| entry.logical_bytes).collect();
        let mut map = div()
            .relative()
            .w_full()
            .h(px(TREEMAP_HEIGHT))
            .bg(theme.muted)
            .overflow_hidden();
        for block in treemap(&sizes, TREEMAP_WIDTH / TREEMAP_HEIGHT) {
            let entry = &entries[block.index];
            let index = block.index;
            let labelled =
                block.width * TREEMAP_WIDTH >= 70.0 && block.height * TREEMAP_HEIGHT >= 34.0;
            map = map.child(
                div()
                    .id(("space-map-block", index))
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
                    .when(labelled, |block| {
                        block
                            .child(
                                div()
                                    .font_semibold()
                                    .text_ellipsis()
                                    .child(entry_label(entry)),
                            )
                            .child(format_bytes(entry.logical_bytes))
                    }),
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
                    .text_ellipsis()
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
