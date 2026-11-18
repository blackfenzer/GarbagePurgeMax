#[cfg(test)]
use std::fs;
use std::io::{self, BufWriter, Write as _};
use std::path::Path;
use std::time::SystemTime;

use gpui_kit::{AppContext as _, Context};
use scan_engine::{assess_risk, cleanup_candidate, file_type};
use scan_store::{SavedScan, ScanStore};

use super::super::{Workspace, storage};
use super::Results;

/// Column names say how certain each finding is: duplicates are byte-verified,
/// similar images are a hash heuristic, and cleanup rules match metadata only.
const HEADER: &str = "path,folder,name,type,logical_bytes,modified_utc,created_utc,read_only,\
verified_duplicate_group,similar_image_group,similar_image_distance_bits,cleanup_rule,\
cleanup_evidence,scan_status,entry_kind,risk,risk_evidence";

impl Results {
    pub(super) fn export_csv(&mut self, cx: &mut Context<Workspace>) {
        if self.exporting {
            return;
        }
        let Some(saved) = &mut self.saved else { return };
        self.exporting = true;
        let scan = saved.scan.clone();
        let scan_id = scan.id;
        let directory = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let name = format!("storage-clinic-scan-{}.csv", scan.id);
        let prompt = cx.prompt_for_new_path(&directory, Some(&name));
        saved.export_status = Some("Choose where to save the CSV…".into());
        cx.notify();

        cx.spawn(async move |workspace, cx| {
            let set_status =
                |status: Option<String>, finished: bool, cx: &mut gpui_kit::AsyncApp| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        // The export belongs to Results, even if another scan was opened.
                        workspace.results.exporting = !finished;
                        if let Some(saved) = &mut workspace.results.saved
                            && saved.scan.id == scan_id
                        {
                            saved.export_status = status;
                        }
                        cx.notify();
                    });
                };
            let destination = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return set_status(None, true, cx),
                Ok(Err(error)) => {
                    return set_status(Some(format!("Could not export: {error}")), true, cx);
                }
                Err(error) => {
                    return set_status(Some(format!("Could not export: {error}")), true, cx);
                }
            };
            set_status(Some("Exporting…".into()), false, cx);
            let result = cx
                .background_spawn(async move {
                    let rows = export_scan(&storage::open()?, &scan, &destination)?;
                    Ok::<_, io::Error>((rows, destination))
                })
                .await;
            let status = match result {
                Ok((rows, path)) => format!("Exported {rows} items to {}", path.display()),
                Err(error) => format!("Could not export: {error}"),
            };
            set_status(Some(status), true, cx);
        })
        .detach();
    }
}

/// Writes every indexed file as CSV and returns the row count. The file is written beside
/// the destination and renamed at the end, so a failure never leaves a partial export.
fn export_scan(store: &ScanStore, scan: &SavedScan, destination: &Path) -> io::Result<u64> {
    let directory = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory = directory.canonicalize()?;
    // Export must not write into the selected tree, including through a directory symlink.
    let scan_root = scan
        .root
        .canonicalize()
        .unwrap_or_else(|_| scan.root.clone());
    if directory.starts_with(&scan_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "choose an export location outside the scanned folder",
        ));
    }
    if let Some(data) = store.database_path().and_then(Path::parent)
        && directory.starts_with(data.canonicalize()?)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "choose an export location outside Storage Clinic’s data directory",
        ));
    }
    let mut partial = tempfile::NamedTempFile::new_in(&directory)?;
    let scanned_at = scan.started_at()?;
    let status = if scan.cancelled {
        "cancelled"
    } else {
        "completed"
    };
    let count;
    {
        let mut out = BufWriter::new(partial.as_file_mut());
        writeln!(out, "{HEADER}")?;
        let files = store.visit_exported_files(scan.id, |row| {
            let file = &row.file;
            let risk = assess_risk(&file.path, file.read_only);
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
                "file".to_string(),
                risk.tier.label().to_string(),
                risk.reason.to_string(),
            ];
            let line: Vec<_> = cells.iter().map(|cell| csv_cell(cell)).collect();
            writeln!(out, "{}", line.join(","))
        })?;
        let mut directories = 0;
        loop {
            let page = store
                .empty_directories(scan.id, 256, directories)
                .map_err(io::Error::other)?;
            if page.is_empty() {
                break;
            }
            for folder in page {
                let risk = assess_risk(&folder.path, false);
                let mut cells: [String; 17] = std::array::from_fn(|_| String::new());
                cells[0] = folder.path.display().to_string();
                cells[1] = folder
                    .path
                    .parent()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                cells[2] = folder
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                cells[3] = "Folder".into();
                cells[11] = "Empty folders".into();
                cells[12] = "No entries observed when scanned".into();
                cells[13] = status.into();
                cells[14] = "directory".into();
                cells[15] = risk.tier.label().into();
                cells[16] = risk.reason.into();
                writeln!(
                    out,
                    "{}",
                    cells
                        .iter()
                        .map(|cell| csv_cell(cell))
                        .collect::<Vec<_>>()
                        .join(",")
                )?;
                directories += 1;
            }
        }
        out.flush()?;
        count = files + directories;
    }
    partial.as_file().sync_all()?;
    partial.persist(destination).map_err(|error| error.error)?;
    Ok(count)
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

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn saved_cache_evidence_is_shared_by_review_and_csv_without_reading_browser_data()
    -> Result<(), Box<dyn std::error::Error>> {
        use scan_engine::{CleanupKind, FileRecord, RiskTier, ScanProgress, ScanSummary};

        // Synthetic records only: never create or inspect an actual browser profile.
        #[cfg(target_os = "macos")]
        let profile = Path::new("/Users/storage-clinic-test/Library/Caches/Google/Chrome/Default");
        #[cfg(windows)]
        let profile = Path::new(
            r"C:\Users\storage-clinic-test\AppData\Local\Google\Chrome\User Data\Default",
        );
        let fixture = tempfile::tempdir()?;
        let database = fixture.path().join("app-data/scans.sqlite3");
        let mut store = storage::open_at(&database)?;
        let mut writer = store.begin_scan(profile, 1_000)?;
        let cache = FileRecord {
            path: profile.join("Cache/Cache_Data/data_0"),
            logical_bytes: 128,
            created: None,
            modified: None,
            read_only: false,
        };
        writer.insert_file(&cache)?;
        writer.insert_file(&FileRecord {
            path: profile.join("Bookmarks"),
            ..cache.clone()
        })?;
        let saved = writer.finish(
            &ScanSummary {
                progress: ScanProgress {
                    files: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
            0,
        )?;
        drop(store);
        let reopened = storage::open_at(&database)?;
        let candidates = reopened.cleanup_candidates(saved.id, saved.started_at()?)?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0.path, cache.path);
        assert_eq!(candidates[0].1.kind, CleanupKind::AppCache);
        assert_eq!(assess_risk(&cache.path, false).tier, RiskTier::Protected);
        let destination = fixture.path().join("cache.csv");
        assert_eq!(export_scan(&reopened, &saved, &destination)?, 2);
        let csv = fs::read_to_string(destination)?;
        let cache_row = csv.lines().find(|line| line.contains("data_0")).unwrap();
        assert!(
            cache_row.contains(",App caches,Path matches Google Chrome"),
            "{cache_row}"
        );
        assert!(cache_row.contains("Metadata evidence only"), "{cache_row}");
        assert!(
            cache_row.contains(",Protected · cleanup blocked,"),
            "{cache_row}"
        );
        let bookmarks = csv.lines().find(|line| line.contains("Bookmarks")).unwrap();
        assert!(!bookmarks.contains("App caches"), "{bookmarks}");
        Ok(())
    }

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
            empty.contains(",Empty files,0 bytes when scanned,completed,file,Unknown risk"),
            "{empty}"
        );
        assert_eq!(csv_cell(" \t=cmd()"), "' \t=cmd()");
        assert_eq!(csv_cell("\tplain"), "'\tplain");
        assert_eq!(csv_cell("report-1.txt"), "report-1.txt");
        let copy = lines.iter().find(|line| line.contains("copy.txt")).unwrap();
        assert!(copy.contains(",false,1,,,"), "{copy}");
        Ok(())
    }
    #[test]
    fn export_preserves_neighbors_and_refuses_the_scan_tree() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let tree = root.path().join("tree");
        fs::create_dir_all(tree.join("empty"))?;
        fs::write(tree.join("keep.txt"), b"keep")?;
        let database = root.path().join("app-data/scans.sqlite3");
        let report = inspect_and_save(
            tree.clone(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let scan = report.saved.unwrap();
        let store = storage::open_at(&database)?;
        let destination = root.path().join("export.csv");
        fs::write(root.path().join("export.csv.part"), b"unrelated")?;
        fs::write(&destination, b"previous export")?;
        assert_eq!(export_scan(&store, &scan, &destination)?, 2);
        assert_eq!(fs::read(root.path().join("export.csv.part"))?, b"unrelated");
        let csv = fs::read_to_string(&destination)?;
        assert!(
            csv.contains(",Empty folders,No entries observed when scanned,completed,directory,")
        );
        assert_eq!(
            export_scan(&store, &scan, &tree.join("keep.txt"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(tree.join("keep.txt"))?, b"keep");
        assert!(export_scan(&store, &scan, &database).is_err());
        assert_eq!(store.scans(10, 0).map_err(io::Error::other)?.len(), 1);
        let before = fs::read_dir(root.path())?.count();
        assert!(export_scan(&store, &scan, &root.path().join("app-data")).is_err());
        assert_eq!(
            fs::read_dir(root.path())?.count(),
            before,
            "failed export leaves no temporary file"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn export_preserves_native_destination_names_and_checks_symlink_parents() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir()?;
        let tree = root.path().join("tree");
        fs::create_dir(&tree)?;
        let database = root.path().join("app-data/scans.sqlite3");
        let report = inspect_and_save(
            tree.clone(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?;
        let scan = report.saved.unwrap();
        let store = storage::open_at(&database)?;
        #[cfg(not(target_os = "linux"))]
        let destination = root.path().join("export-雪.csv");
        #[cfg(target_os = "linux")]
        let destination = {
            use std::os::unix::ffi::OsStrExt;
            root.path()
                .join(std::ffi::OsStr::from_bytes(b"export-\xff.csv"))
        };
        assert_eq!(export_scan(&store, &scan, &destination)?, 1);
        assert!(destination.is_file());
        symlink(&tree, root.path().join("alias"))?;
        assert!(export_scan(&store, &scan, &root.path().join("alias/result.csv")).is_err());
        Ok(())
    }
}
