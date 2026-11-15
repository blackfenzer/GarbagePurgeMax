use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::Connection;
use scan_engine::{FileRecord, ScanProgress, ScanSummary};
use scan_store::{DuplicateEvidence, FileTypeTotal, ScanStore};

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
    let files = [file("partial-one", 1), file("partial-two", 1)];
    for file in &files {
        writer.insert_file(file)?;
    }
    writer.insert_duplicate_group(&paths(&files), &[42; 32])?;
    writer.mark_duplicates_analyzed(1)?;
    drop(writer);
    let connection = Connection::open(&db)?;
    let count = |table| {
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
    };
    assert_eq!(count("scans")?, 0);
    assert_eq!(count("files")?, 0);
    assert_eq!(count("duplicate_groups")?, 0);
    assert_eq!(count("duplicate_members")?, 0);
    Ok(())
}

#[test]
fn migrates_legacy_history_when_opened_concurrently() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let connection = Connection::open(&db)?;
    connection.execute_batch(include_str!("../sql/001_initial.sql"))?;
    let key = |value: &str| -> Vec<u8> {
        if cfg!(windows) {
            value.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            value.as_bytes().to_vec()
        }
    };
    connection.execute(
        "INSERT INTO scans (id, root_key, display_root, started_unix_ms, finished_unix_ms,
                            files, status, large_file_threshold)
         VALUES (1, ?1, 'legacy', 1, 2, 1, 'completed', 42)",
        [key("legacy")],
    )?;
    connection.execute(
        "INSERT INTO files (scan_id, path_key, display_path, logical_bytes, read_only)
         VALUES (1, ?1, 'old.txt', 32, 0)",
        [key("old.txt")],
    )?;
    drop(connection);

    let start = Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let open = || {
            start.wait();
            let store = ScanStore::open(&db)?;
            store.scans(10, 0)
        };
        let first = scope.spawn(open);
        let second = scope.spawn(open);
        (first.join().unwrap(), second.join().unwrap())
    });
    for scans in [first?, second?] {
        assert_eq!(scans.len(), 1);
        let legacy = &scans[0];
        assert_eq!(legacy.root, Path::new("legacy"));
        assert_eq!(legacy.progress.files, 1);
        assert!(!legacy.duplicates_analyzed);
        assert_eq!(legacy.duplicate_group_count, 0);
        assert_eq!(legacy.duplicate_error_count, 0);
    }
    let store = ScanStore::open(&db)?;
    let files = store.files(1, "", 10, 0)?;
    assert_eq!(paths(&files), [PathBuf::from("old.txt")]);
    assert!(store.duplicate_evidence(1, &files)?.is_empty());
    assert!(store.duplicate_files(1, "", 10, 0)?.is_empty());
    Ok(())
}

#[test]
fn walks_size_candidates_and_pages_verified_members() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 20)?;
    let empty = [file("empty-a", 0), file("empty-b", 0)];
    let singleton = file("size-single", 1);
    let larger = [
        file("copy%_A", 2),
        file("copy%_b", 2),
        file("same-size-different-content", 2),
    ];
    for file in empty.iter().chain([&singleton]).chain(&larger) {
        writer.insert_file(file)?;
    }
    assert_eq!(
        paths(&writer.next_duplicate_candidates(None)?),
        paths(&empty)
    );
    assert_eq!(
        paths(&writer.next_duplicate_candidates(Some(0))?),
        paths(&larger)
    );
    assert!(writer.next_duplicate_candidates(Some(2))?.is_empty());

    let invalid = [larger[0].path.clone(), PathBuf::from("not indexed")];
    assert!(writer.insert_duplicate_group(&invalid, &[34; 32]).is_err());
    writer.insert_duplicate_group(&paths(&larger[..2]), &[34; 32])?;
    writer.insert_duplicate_group(&paths(&empty), &[42; 32])?;
    writer.mark_duplicates_analyzed(3)?;
    let scan = writer.finish(&summary(6, false), 0)?;
    assert!(scan.duplicates_analyzed);
    assert_eq!(scan.duplicate_group_count, 2);
    assert_eq!(scan.duplicate_error_count, 3);
    drop(store);

    let store = ScanStore::open(&db)?;
    let scans = store.scans(10, 0)?;
    assert!(scans[0].duplicates_analyzed);
    assert_eq!(scans[0].duplicate_group_count, 2);
    assert_eq!(scans[0].duplicate_error_count, 3);
    let first = store.duplicate_files(scan.id, "COPY%_", 1, 0)?;
    let second = store.duplicate_files(scan.id, "COPY%_", 1, 1)?;
    assert_eq!(paths(&first), paths(&larger[..1]));
    assert_eq!(paths(&second), paths(&larger[1..2]));
    assert!(store.duplicate_files(scan.id, "COPY%_", 1, 2)?.is_empty());
    assert_eq!(
        paths(&store.duplicate_files(scan.id, "", 10, 2)?),
        paths(&empty)
    );
    let page = store.files(scan.id, "", 10, 0)?;
    let evidence = store.duplicate_evidence(scan.id, &page)?;
    assert_eq!(evidence.len(), 4);
    assert_eq!(
        evidence.get(&larger[0].path),
        Some(&DuplicateEvidence {
            file_count: 2,
            hash: [34; 32],
        })
    );
    assert_eq!(
        evidence.get(&empty[0].path),
        Some(&DuplicateEvidence {
            file_count: 2,
            hash: [42; 32],
        })
    );
    assert!(store.duplicate_evidence(scan.id + 1, &page)?.is_empty());
    Ok(())
}

#[test]
fn saves_completed_groups_from_cancelled_analysis() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 20)?;
    let members = [file("copy-a", 1), file("copy-b", 1)];
    for file in &members {
        writer.insert_file(file)?;
    }
    writer.insert_duplicate_group(&paths(&members), &[42; 32])?;
    writer.mark_duplicates_analyzed(0)?;
    let scan = writer.finish(&summary(2, true), 0)?;
    assert!(scan.cancelled);
    assert!(!scan.duplicates_analyzed);
    assert_eq!(scan.duplicate_group_count, 1);
    assert!(!store.scans(10, 0)?[0].duplicates_analyzed);
    assert_eq!(
        paths(&store.duplicate_files(scan.id, "", 10, 0)?),
        paths(&members)
    );
    Ok(())
}

#[test]
fn distinguishes_completed_analysis_with_zero_duplicates() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 20)?;
    writer.mark_duplicates_analyzed(0)?;
    writer.finish(&summary(0, false), 0)?;
    let scans = store.scans(10, 0)?;
    assert!(scans[0].duplicates_analyzed);
    assert_eq!(scans[0].duplicate_group_count, 0);
    Ok(())
}

#[test]
fn totals_file_types_for_a_reopened_partial_scan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 1)?;
    for record in [
        file("a.JPG", 10),
        file("b.png", 5),
        file("README", 3),
        file("c.unknown", 4),
        file("d.pdf", 1),
    ] {
        writer.insert_file(&record)?;
    }
    let scan = writer.finish(&summary(5, true), 0)?;
    drop(store);

    let totals = ScanStore::open(&db)?.file_types(scan.id)?;
    let total = |file_type, files, logical_bytes| FileTypeTotal {
        file_type,
        files,
        logical_bytes,
    };
    assert_eq!(
        totals,
        [
            total("Images", 2, 15),
            total("Other", 2, 7),
            total("Documents", 1, 1)
        ]
    );
    Ok(())
}

#[test]
fn explains_a_scan_blocked_by_another_window() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut saving = ScanStore::open(&db)?;
    let mut blocked = ScanStore::open(&db)?;
    let _writer = saving.begin_scan(directory.path(), 1)?;
    let Err(error) = blocked.begin_scan(directory.path(), 1) else {
        panic!("a second writer started while the first scan was saving");
    };
    assert_eq!(
        error.to_string(),
        "another Storage Clinic window is saving a scan. Close it and scan again"
    );
    Ok(())
}

fn file(path: &str, logical_bytes: u64) -> FileRecord {
    FileRecord {
        path: path.into(),
        logical_bytes,
        created: None,
        modified: None,
        read_only: false,
    }
}

fn paths(files: &[FileRecord]) -> Vec<PathBuf> {
    files.iter().map(|file| file.path.clone()).collect()
}

fn summary(files: u64, cancelled: bool) -> ScanSummary {
    ScanSummary {
        progress: ScanProgress {
            files,
            ..ScanProgress::default()
        },
        cancelled,
        first_error: None,
    }
}
