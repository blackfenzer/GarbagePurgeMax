use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use scan_engine::{FileRecord, ScanProgress, ScanSummary};
use scan_store::ScanStore;

#[test]
fn restores_partial_scan_metadata_and_paginates_literal_search()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let root = directory.path().join("files");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(&root, 75)?;
    let before_epoch = UNIX_EPOCH - Duration::from_nanos(250);
    for (name, size) in [
        ("100%_Export.txt", 50),
        ("200%_export.txt", 100),
        ("other.txt", 150),
    ] {
        writer.insert_file(&FileRecord {
            path: root.join(name),
            logical_bytes: size,
            created: None,
            modified: Some(before_epoch),
            read_only: true,
        })?;
    }
    let scan = writer.finish(
        &ScanSummary {
            progress: ScanProgress {
                files: 3,
                logical_bytes: 300,
                errors: 1,
                ..ScanProgress::default()
            },
            cancelled: true,
            first_error: Some("example read error".into()),
        },
        2,
    )?;
    drop(store);

    let store = ScanStore::open(&db)?;
    let scans = store.scans(20, 0)?;
    assert_eq!(scans.len(), 1);
    assert_eq!(scans[0].root, root);
    assert!(scans[0].cancelled);
    assert_eq!(scans[0].large_file_count, 2);
    assert_eq!(scans[0].first_error.as_deref(), Some("example read error"));
    let first_page = store.files(scan.id, "%_EXPORT", 1, 0)?;
    let second_page = store.files(scan.id, "%_EXPORT", 1, 1)?;
    assert_eq!(first_page[0].path, root.join("200%_export.txt"));
    assert_eq!(second_page[0].path, root.join("100%_Export.txt"));
    assert_eq!(second_page[0].modified, Some(before_epoch));
    assert!(second_page[0].read_only);
    assert!(store.files(scan.id, "%_EXPORT", 1, 2)?.is_empty());
    Ok(())
}

#[test]
fn dropped_writer_rolls_back_file_records_and_scan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 1)?;
    writer.insert_file(&FileRecord {
        path: PathBuf::from("partial"),
        logical_bytes: 1,
        created: None,
        modified: None,
        read_only: false,
    })?;
    drop(writer);
    let connection = rusqlite::Connection::open(&db)?;
    let count = |table| {
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
    };
    assert_eq!(count("scans")?, 0);
    assert_eq!(count("files")?, 0);
    Ok(())
}
