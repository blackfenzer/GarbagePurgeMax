use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _,
    table::{Column, TableDelegate, TableState},
};
use gpui_kit::{
    App, Context, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, px,
};
use scan_engine::FileRecord;
use scan_store::DuplicateEvidence;

use super::super::scan::format_bytes;
use super::SAMPLE_FILES;

struct FileRow {
    name: SharedString,
    search_path: String,
    finding: &'static str,
    size: SharedString,
}

pub(super) struct FileRows {
    files: Vec<FileRow>,
    matches: Vec<usize>,
}

impl FileRows {
    pub(super) fn samples() -> Self {
        let files = SAMPLE_FILES
            .iter()
            .map(|file| FileRow {
                name: file.name.into(),
                search_path: format!("{}/{}", file.folder, file.name).to_ascii_lowercase(),
                finding: file.finding,
                size: file.size.into(),
            })
            .collect();
        Self::with_query(files, "")
    }

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
                    name: name.to_string_lossy().into_owned().into(),
                    search_path: file.path.to_string_lossy().to_ascii_lowercase(),
                    finding,
                    size: format_bytes(file.logical_bytes).into(),
                }
            })
            .collect();
        Self::with_query(files, query)
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
            _ => unreachable!("unexpected file table column"),
        }
    }
}

impl TableDelegate for FileRows {
    fn columns_count(&self, _: &App) -> usize {
        3
    }

    fn rows_count(&self, _: &App) -> usize {
        self.matches.len()
    }

    fn column(&self, column: usize, _: &App) -> Column {
        match column {
            0 => Column::new("file", "File").width(px(215.)),
            1 => Column::new("finding", "Finding").width(px(108.)),
            2 => Column::new("size", "Size").width(px(82.)).text_right(),
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
        div().text_ellipsis().child(self.cell_text(row, column))
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

    #[test]
    fn search_preserves_source_indices_and_matches_folders_case_insensitively() {
        let mut rows = FileRows::samples();
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
