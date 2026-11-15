use std::fs::{self, File};
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{AppContext as _, Context};
use scan_engine::{cleanup_candidate, file_type};
use scan_store::{SavedScan, ScanStore};

use super::super::{Workspace, storage};
use super::Results;

/// Column names say how certain each finding is: duplicates are byte-verified,
/// similar images are a hash heuristic, and cleanup rules match metadata only.
const HEADER: &str = "path,folder,name,type,logical_bytes,modified_utc,created_utc,read_only,\
verified_duplicate_group,similar_image_group,similar_image_distance_bits,cleanup_rule,\
cleanup_evidence,scan_status";

impl Results {
    pub(super) fn export_csv(&mut self, cx: &mut Context<Workspace>) {
        let Some(saved) = &mut self.saved else { return };
        let scan = saved.scan.clone();
        let directory = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let name = format!("storage-clinic-scan-{}.csv", scan.id);
        let prompt = cx.prompt_for_new_path(&directory, Some(&name));
        saved.export_status = Some("Choose where to save the CSV…".into());
        cx.notify();

        cx.spawn(async move |workspace, cx| {
            let set_status = |status: Option<String>, cx: &mut gpui_kit::AsyncApp| {
                let _ = workspace.update(cx, |workspace, cx| {
                    if let Some(saved) = &mut workspace.results.saved {
                        saved.export_status = status;
                        cx.notify();
                    }
                });
            };
            let destination = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return set_status(None, cx),
                Ok(Err(error)) => {
                    return set_status(Some(format!("Could not export: {error}")), cx);
                }
                Err(error) => return set_status(Some(format!("Could not export: {error}")), cx),
            };
            set_status(Some("Exporting…".into()), cx);
            let result = cx
                .background_spawn(async move {
                    let rows = export_scan(&storage::open()?, &scan, &destination)?;
                    Ok::<_, io::Error>((rows, destination))
                })
                .await;
            let status = match result {
                Ok((rows, path)) => format!("Exported {rows} files to {}", path.display()),
                Err(error) => format!("Could not export: {error}"),
            };
            set_status(Some(status), cx);
        })
        .detach();
    }
}

/// Writes every indexed file as CSV and returns the row count. The file is written beside
/// the destination and renamed at the end, so a failure never leaves a partial export.
fn export_scan(store: &ScanStore, scan: &SavedScan, destination: &Path) -> io::Result<u64> {
    let rows = store.exported_files(scan.id).map_err(io::Error::other)?;
    let scanned_at =
        UNIX_EPOCH + Duration::from_millis(u64::try_from(scan.started_unix_ms).unwrap_or(0));
    let status = if scan.cancelled {
        "cancelled"
    } else {
        "completed"
    };
    let partial = PathBuf::from(format!("{}.part", destination.display()));
    let written = (|| {
        let mut out = BufWriter::new(File::create(&partial)?);
        writeln!(out, "{HEADER}")?;
        for row in &rows {
            let file = &row.file;
            let candidate = cleanup_candidate(file, scanned_at);
            let optional = |value: Option<String>| value.unwrap_or_default();
            let cells = [
                file.path.display().to_string(),
                optional(
                    file.path
                        .parent()
                        .map(|folder| folder.display().to_string()),
                ),
                optional(
                    file.path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                ),
                file_type(&file.path).to_string(),
                file.logical_bytes.to_string(),
                timestamp(file.modified),
                timestamp(file.created),
                file.read_only.to_string(),
                optional(row.duplicate_group.map(|group| group.to_string())),
                optional(row.similar_image.map(|(group, _)| group.to_string())),
                optional(row.similar_image.map(|(_, distance)| distance.to_string())),
                optional(
                    candidate
                        .as_ref()
                        .map(|found| found.kind.label().to_string()),
                ),
                optional(candidate.map(|found| found.evidence)),
                status.to_string(),
            ];
            let line: Vec<_> = cells.iter().map(|cell| csv_cell(cell)).collect();
            writeln!(out, "{}", line.join(","))?;
        }
        out.into_inner()
            .map_err(|error| error.into_error())?
            .sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, destination)?;
    Ok(rows.len() as u64)
}

fn timestamp(time: Option<SystemTime>) -> String {
    time.and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map(|time| time.to_string())
        .unwrap_or_default()
}

/// Quotes a CSV cell when needed. A cell whose first non-space character is `=`, `+`,
/// `-`, or `@`, or that starts with a tab or carriage return, gets an apostrophe so
/// spreadsheets show a file name instead of running it as a formula.
fn csv_cell(value: &str) -> String {
    let formula_like =
        value.starts_with(['\t', '\r']) || value.trim_start().starts_with(['=', '+', '-', '@']);
    let value = if formula_like {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    use crate::workspace::scan::inspect_and_save;

    #[test]
    fn exports_every_file_with_quoted_and_defused_cells() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("app-data/scans.sqlite3");
        let scanned = root.path().join("scanned");
        fs::create_dir(&scanned)?;
        // Windows forbids `"` in file names, so quote doubling is checked on `csv_cell` below.
        fs::write(scanned.join("=sum(1),x.txt"), "same")?;
        fs::write(scanned.join("copy.txt"), "same")?;
        fs::write(scanned.join("empty"), "")?;

        let report = inspect_and_save(
            scanned,
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let scan = report.saved.expect("scan saved");
        let destination = root.path().join("export.csv");
        let rows = export_scan(&storage::open_at(&database)?, &scan, &destination)?;
        assert_eq!(rows, 3);
        assert!(!root.path().join("export.csv.part").exists());

        let csv = fs::read_to_string(&destination)?;
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines[0], HEADER);
        assert_eq!(lines.len(), 4);
        // The name cell is defused with an apostrophe, then quoted for its comma.
        assert!(csv.contains(",\"'=sum(1),x.txt\","), "{csv}");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        let empty = lines.iter().find(|line| line.contains("empty,")).unwrap();
        assert!(
            empty.ends_with(",Empty files,0 bytes when scanned,completed"),
            "{empty}"
        );
        assert_eq!(csv_cell(" \t=cmd()"), "' \t=cmd()");
        assert_eq!(csv_cell("\tplain"), "'\tplain");
        assert_eq!(csv_cell("report-1.txt"), "report-1.txt");
        let copy = lines.iter().find(|line| line.contains("copy.txt")).unwrap();
        assert!(copy.contains(",false,1,,,"), "{copy}");
        Ok(())
    }
}
