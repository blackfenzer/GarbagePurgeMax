use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    table::{Column, TableDelegate, TableState},
};
use gpui_kit::{
    App, Context, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, img, px,
};
use scan_engine::FileRecord;
use scan_store::DuplicateEvidence;

use super::super::scan::format_bytes;

struct FileRow {
    name: SharedString,
    /// The file-type icon's asset path.
    icon: &'static str,
    search_path: String,
    finding: &'static str,
    size: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::workspace) enum RowAction {
    Inspect,
    Trash,
}

/// Runs a row button's action for the file at a source index.
pub(super) type RowActions = Rc<dyn Fn(RowAction, usize, &mut Window, &mut App)>;

pub(super) struct FileRows {
    files: Vec<FileRow>,
    matches: Vec<usize>,
    /// Saved scans get Details and Move to Trash buttons in a last column.
    actions: Option<RowActions>,
}

impl FileRows {
    pub(super) fn from_files(
        records: &[FileRecord],
        large_file_threshold: u64,
        query: &str,
        evidence: Option<&HashMap<PathBuf, DuplicateEvidence>>,
    ) -> Self {
        let files = records
            .iter()
            .map(|file| {
                let name = file.path.file_name().unwrap_or(file.path.as_os_str());
                let finding = if evidence.is_some_and(|evidence| evidence.contains_key(&file.path))
                {
                    "Exact duplicate"
                } else if file.logical_bytes >= large_file_threshold {
                    "Large file"
                } else {
                    "—"
                };
                FileRow {
                    icon: crate::file_icons::icon_path(name, false),
                    name: name.to_string_lossy().into_owned().into(),
                    search_path: file.path.to_string_lossy().to_ascii_lowercase(),
                    finding,
                    size: format_bytes(file.logical_bytes).into(),
                }
            })
            .collect();
        Self::with_query(files, query)
    }

    pub(super) fn with_actions(mut self, actions: RowActions) -> Self {
        self.actions = Some(actions);
        self
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        (self.matches.len(), self.files.len())
    }

    pub(super) fn source_index(&self, visible_row: usize) -> Option<usize> {
        self.matches.get(visible_row).copied()
    }

    fn with_query(files: Vec<FileRow>, query: &str) -> Self {
        let mut rows = Self {
            files,
            matches: Vec::new(),
            actions: None,
        };
        rows.filter(query);
        rows
    }

    pub(super) fn filter(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();
        self.matches.clear();
        self.matches.extend(
            self.files
                .iter()
                .enumerate()
                .filter_map(|(index, file)| file.search_path.contains(&query).then_some(index)),
        );
    }

    fn cell_text(&self, row: usize, column: usize) -> SharedString {
        let file = &self.files[self.matches[row]];
        match column {
            0 => file.name.clone(),
            1 => file.finding.into(),
            2 => file.size.clone(),
            _ => SharedString::default(),
        }
    }
}

impl TableDelegate for FileRows {
    fn columns_count(&self, _: &App) -> usize {
        if self.actions.is_some() { 4 } else { 3 }
    }

    fn rows_count(&self, _: &App) -> usize {
        self.matches.len()
    }

    fn column(&self, column: usize, _: &App) -> Column {
        match column {
            0 => Column::new("file", "File").width(px(215.)),
            1 => Column::new("finding", "Finding").width(px(108.)),
            2 => Column::new("size", "Size").width(px(82.)).text_right(),
            3 => Column::new("actions", "").width(px(76.)),
            _ => unreachable!("unexpected file table column"),
        }
    }

    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if column == 3
            && let (Some(actions), Some(&source)) = (&self.actions, self.matches.get(row))
        {
            let button = |action: RowAction, icon: IconName, tooltip: &'static str| {
                let actions = Rc::clone(actions);
                Button::new((
                    if action == RowAction::Trash {
                        "file-row-trash"
                    } else {
                        "file-row-inspect"
                    },
                    row,
                ))
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
                .on_click(move |_, window, cx| {
                    // Keep the click from also selecting the row underneath.
                    cx.stop_propagation();
                    actions(action, source, window, cx);
                })
            };
            return div()
                .flex()
                .items_center()
                .gap_1()
                .child(button(RowAction::Inspect, IconName::Info, "Details"))
                .child(button(RowAction::Trash, IconName::Trash, "Move to Trash"))
                .into_any_element();
        }
        let icon = (column == 0)
            .then(|| self.matches.get(row).map(|&index| self.files[index].icon))
            .flatten();
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w(px(0.0))
            .children(icon.map(|icon| img(icon).size(px(16.0)).flex_none()))
            .child(
                div()
                    .min_w(px(0.0))
                    .text_ellipsis()
                    .child(self.cell_text(row, column)),
            )
            .into_any_element()
    }

    fn cell_label(&self, row: usize, column: usize, _: &App) -> Option<SharedString> {
        Some(self.cell_text(row, column))
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let message = if self.files.is_empty() {
            "No files found"
        } else {
            "No matching files"
        };
        div()
            .p_4()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(message)
    }
}

#[cfg(test)]
mod tests {
    use super::FileRows;
    use scan_engine::FileRecord;

    #[test]
    fn search_preserves_source_indices_and_matches_folders_case_insensitively() {
        let files: Vec<FileRecord> = [
            "/Example/Archives/studio-backup.zip",
            "/Example/Design/coast-export.png",
            "/Example/Downloads/coast-export-copy.png",
        ]
        .into_iter()
        .map(|path| FileRecord {
            path: path.into(),
            logical_bytes: 1,
            created: None,
            modified: None,
            read_only: false,
        })
        .collect();
        let mut rows = FileRows::from_files(&files, u64::MAX, "", None);
        rows.filter("DOWNLOADS");
        assert_eq!(rows.matches, [2]);
        assert_eq!(rows.cell_text(0, 0).as_ref(), "coast-export-copy.png");
        rows.filter("COAST");
        assert_eq!(rows.matches, [1, 2]);
        rows.filter("%");
        assert!(rows.matches.is_empty());
        rows.filter("");
        assert_eq!(rows.matches, [0, 1, 2]);
    }
}
