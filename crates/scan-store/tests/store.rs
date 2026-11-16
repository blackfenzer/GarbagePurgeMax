use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::Connection;
use scan_engine::{CleanupKind, FileRecord, ScanProgress, ScanSummary, SimilarImage};
use scan_store::{
    DuplicateEvidence, FileFilter, FileSort, FileTypeTotal, ReviewGroup, ScanStore,
    SimilarImageEvidence,
};

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
        assert!(!legacy.similar_images_analyzed);
        assert_eq!(legacy.similar_group_count, 0);
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

#[test]
fn lists_review_groups_and_keeps_similarity_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("scans.sqlite3");
    let mut store = ScanStore::open(&db)?;
    let mut writer = store.begin_scan(directory.path(), 1_000)?;
    let files = [
        file("small-a.txt", 5),
        file("small-b.txt", 5),
        file("large-a.bin", 40),
        file("large-b.bin", 40),
        file("photo.jpg", 30),
        file("photo-small.jpg", 10),
    ];
    for record in &files {
        writer.insert_file(record)?;
    }
    writer.insert_duplicate_group(&paths(&files[..2]), &[1; 32])?;
    writer.insert_duplicate_group(&paths(&files[2..4]), &[2; 32])?;
    let high_bit_hash = u64::MAX - 1;
    writer.insert_similar_image_group(&[
        SimilarImage {
            path: files[4].path.clone(),
            hash: high_bit_hash,
            distance: 0,
        },
        SimilarImage {
            path: files[5].path.clone(),
            hash: high_bit_hash ^ 0b101,
            distance: 2,
        },
    ])?;
    writer.mark_similar_images_analyzed(1)?;
    let scan = writer.finish(&summary(6, false), 0)?;
    assert!(scan.similar_images_analyzed);
    drop(store);

    let store = ScanStore::open(&db)?;
    let saved = &store.scans(1, 0)?[0];
    assert_eq!((saved.similar_group_count, saved.image_error_count), (1, 1));
    let group = |group_id, file_count, logical_bytes| ReviewGroup {
        group_id,
        file_count,
        logical_bytes,
    };
    assert_eq!(
        store.duplicate_groups(scan.id, 10, 0)?,
        [group(2, 2, 80), group(1, 2, 10)]
    );
    assert_eq!(store.duplicate_groups(scan.id, 1, 1)?, [group(1, 2, 10)]);
    assert_eq!(
        paths(&store.duplicate_group_files(scan.id, 2, 10)?),
        paths(&files[2..4])
    );
    assert_eq!(
        store.similar_image_groups(scan.id, 10, 0)?,
        [group(1, 2, 40)]
    );
    let members = store.similar_image_group_files(scan.id, 1, 10)?;
    assert_eq!(members[0].0.path, files[4].path);
    assert_eq!(
        members[1].1,
        SimilarImageEvidence {
            difference_hash: high_bit_hash ^ 0b101,
            distance: 2
        }
    );
    assert!(
        store
            .duplicate_evidence(scan.id, &[file("photo.jpg", 30)])?
            .is_empty()
    );

    let exported = store.exported_files(scan.id)?;
    assert_eq!(exported.len(), files.len());
    let row = |name: &str| {
        exported
            .iter()
            .find(|row| row.file.path == Path::new(name))
            .unwrap()
    };
    assert_eq!(row("large-a.bin").duplicate_group, Some(2));
    assert_eq!(row("large-a.bin").similar_image, None);
    assert_eq!(row("photo-small.jpg").similar_image, Some((1, 2)));
    assert_eq!(row("photo-small.jpg").duplicate_group, None);
    Ok(())
}

#[test]
fn lists_cleanup_candidates_largest_first() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut store = ScanStore::open(&directory.path().join("scans.sqlite3"))?;
    let mut writer = store.begin_scan(directory.path(), 1_000)?;
    for record in [
        file("empty", 0),
        file("notes.txt", 7),
        file("old.zip", 40),
        file("Tool.dmg", 90),
    ] {
        writer.insert_file(&record)?;
    }
    let scan = writer.finish(&summary(4, false), 0)?;
    let candidates = store.cleanup_candidates(scan.id, UNIX_EPOCH)?;
    let kinds: Vec<_> = candidates.iter().map(|(_, found)| found.kind).collect();
    assert_eq!(
        kinds,
        [
            CleanupKind::Installer,
            CleanupKind::Archive,
            CleanupKind::EmptyFile
        ]
    );
    Ok(())
}

#[test]
fn filters_by_type_and_age_and_sorts_pages() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut store = ScanStore::open(&directory.path().join("scans.sqlite3"))?;
    let mut writer = store.begin_scan(directory.path(), 1_000)?;
    let day = Duration::from_secs(24 * 60 * 60);
    for (name, bytes, age_days) in [
        ("b-photo.jpg", 30, 400),
        ("A-photo.png", 10, 5),
        ("notes.txt", 20, 400),
        ("clip.mov", 40, 1),
    ] {
        writer.insert_file(&FileRecord {
            modified: Some(UNIX_EPOCH + day * (1_000 - age_days)),
            ..file(name, bytes)
        })?;
    }
    let scan = writer.finish(&summary(4, false), 0)?;
    let names = |filter: FileFilter| -> Result<Vec<_>, rusqlite::Error> {
        Ok(store
            .filtered_files(scan.id, &filter, 10, 0)?
            .into_iter()
            .map(|file| file.path.display().to_string())
            .collect())
    };

    let images = FileFilter {
        file_type: Some("Images"),
        ..FileFilter::default()
    };
    assert_eq!(names(images.clone())?, ["b-photo.jpg", "A-photo.png"]);
    let by_name = FileFilter {
        sort: FileSort::Name,
        ..images
    };
    assert_eq!(names(by_name)?, ["A-photo.png", "b-photo.jpg"]);
    let older_than_a_year = FileFilter {
        modified_before: Some(UNIX_EPOCH + day * (1_000 - 365)),
        sort: FileSort::Smallest,
        ..FileFilter::default()
    };
    assert_eq!(names(older_than_a_year)?, ["notes.txt", "b-photo.jpg"]);
    let newest = FileFilter {
        sort: FileSort::Newest,
        ..FileFilter::default()
    };
    assert_eq!(names(newest)?[0], "clip.mov");
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

#[test]
fn upgrades_every_previous_schema_and_rolls_back_directory_and_cache_writes()
-> Result<(), Box<dyn std::error::Error>> {
    use scan_engine::{DirectoryRecord, HashCache, HashStage};
    let migrations = [
        include_str!("../sql/001_initial.sql"),
        include_str!("../sql/002_exact_duplicates.sql"),
        include_str!("../sql/003_similar_images.sql"),
        include_str!("../sql/004_directories.sql"),
    ];
    for version in 1..=migrations.len() {
        let root = tempfile::tempdir()?;
        let database = root.path().join("scan.sqlite3");
        let connection = Connection::open(&database)?;
        for migration in &migrations[..version] {
            connection.execute_batch(migration)?;
        }
        drop(connection);
        let mut store = ScanStore::open(&database)?;
        let mut writer = store.begin_scan(root.path(), 100)?;
        writer.insert_directory(&DirectoryRecord {
            path: root.path().join("empty"),
            empty: Some(true),
        })?;
        writer.insert_directory(&DirectoryRecord {
            path: root.path().join("unknown"),
            empty: None,
        })?;
        writer.store(Path::new("file"), b"version", HashStage::Full, &[42; 32])?;
        assert_eq!(
            writer.load(Path::new("file"), b"changed", HashStage::Full)?,
            None
        );
        assert_eq!(
            writer.load(Path::new("file"), b"version", HashStage::Full)?,
            Some([42; 32])
        );
        drop(writer);
        assert!(store.empty_directories(1, 10, 0)?.is_empty());
        let connection = Connection::open(&database)?;
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM hash_cache", [], |row| row
                .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?,
            5
        );
    }
    Ok(())
}

#[test]
fn empty_folder_pages_survive_partial_scans_without_changing_file_totals()
-> Result<(), Box<dyn std::error::Error>> {
    use scan_engine::DirectoryRecord;
    let root = tempfile::tempdir()?;
    let database = root.path().join("scan.sqlite3");
    let mut store = ScanStore::open(&database)?;
    let mut writer = store.begin_scan(root.path(), 100)?;
    for (name, empty) in [
        ("a", Some(true)),
        ("b", Some(true)),
        ("unknown", None),
        ("not-empty", Some(false)),
    ] {
        writer.insert_directory(&DirectoryRecord {
            path: root.path().join(name),
            empty,
        })?;
    }
    let saved = writer.finish(
        &ScanSummary {
            cancelled: true,
            progress: ScanProgress {
                directories: 4,
                ..ScanProgress::default()
            },
            ..ScanSummary::default()
        },
        0,
    )?;
    drop(store);
    let store = ScanStore::open(&database)?;
    assert_eq!(
        store.empty_directories(saved.id, 1, 0)?[0].path,
        root.path().join("a")
    );
    assert_eq!(
        store.empty_directories(saved.id, 1, 1)?[0].path,
        root.path().join("b")
    );
    assert!(store.empty_directories(saved.id, 1, 2)?.is_empty());
    assert!(store.file_types(saved.id)?.is_empty());
    assert_eq!(store.scans(10, 0)?[0].progress.files, 0);
    Ok(())
}

#[test]
fn age_filter_and_time_sort_preserve_nanoseconds_and_put_unknown_dates_last()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut store = ScanStore::open(&root.path().join("scan.sqlite3"))?;
    let mut writer = store.begin_scan(root.path(), 100)?;
    for (name, modified) in [
        ("early", Some(UNIX_EPOCH + Duration::from_nanos(1))),
        ("late", Some(UNIX_EPOCH + Duration::from_nanos(3))),
        ("unknown", None),
    ] {
        writer.insert_file(&FileRecord {
            modified,
            ..file(name, 1)
        })?;
    }
    let scan = writer.finish(&summary(3, false), 0)?;
    let names = |filter| -> Result<Vec<_>, rusqlite::Error> {
        Ok(store
            .filtered_files(scan.id, &filter, 10, 0)?
            .into_iter()
            .map(|file| file.path)
            .collect())
    };
    assert_eq!(
        names(FileFilter {
            modified_before: Some(UNIX_EPOCH + Duration::from_nanos(2)),
            ..FileFilter::default()
        })?,
        vec![PathBuf::from("early")]
    );
    assert_eq!(
        names(FileFilter {
            sort: FileSort::Oldest,
            ..FileFilter::default()
        })?,
        vec![
            PathBuf::from("early"),
            PathBuf::from("late"),
            PathBuf::from("unknown")
        ]
    );
    assert_eq!(
        names(FileFilter {
            sort: FileSort::Newest,
            ..FileFilter::default()
        })?,
        vec![
            PathBuf::from("late"),
            PathBuf::from("early"),
            PathBuf::from("unknown")
        ]
    );
    Ok(())
}

#[test]
fn history_and_pages_remain_readable_while_a_large_scan_is_writing()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let database = root.path().join("scan.sqlite3");
    let mut store = ScanStore::open(&database)?;
    let mut first = store.begin_scan(root.path(), 1_000)?;
    first.insert_file(&file("saved.txt", 10))?;
    let saved = first.finish(&summary(1, false), 0)?;
    let mut active = store.begin_scan(root.path(), 1_000)?;
    for index in 0..10_000 {
        active.insert_file(&file(&format!("{}/{index}.txt", "folder/".repeat(40)), 10))?;
    }
    let reader = ScanStore::open(&database)?;
    assert_eq!(reader.scans(10, 0)?.len(), 1);
    assert_eq!(reader.files(saved.id, "saved", 100, 0)?.len(), 1);
    drop(active);
    assert_eq!(reader.scans(10, 0)?.len(), 1);
    Ok(())
}

#[test]
fn partial_scans_retain_analysis_error_counts() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let database = root.path().join("scan.sqlite3");
    let mut store = ScanStore::open(&database)?;
    let mut writer = store.begin_scan(root.path(), 100)?;
    writer.record_duplicate_errors(2);
    writer.record_image_errors(3);
    writer.finish(
        &ScanSummary {
            cancelled: true,
            first_error: Some("unreadable image".into()),
            ..ScanSummary::default()
        },
        0,
    )?;
    drop(store);
    let saved = ScanStore::open(&database)?.scans(1, 0)?.remove(0);
    assert_eq!(saved.duplicate_error_count, 2);
    assert_eq!(saved.image_error_count, 3);
    assert!(!saved.duplicates_analyzed && !saved.similar_images_analyzed);
    Ok(())
}
