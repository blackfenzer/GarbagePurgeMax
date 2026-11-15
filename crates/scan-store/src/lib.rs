mod encoding;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{
    Connection, OptionalExtension, Row, Transaction, TransactionBehavior, named_params, params,
    types::Type,
};
use scan_engine::{
    CleanupCandidate, FileRecord, ScanProgress, ScanSummary, SimilarImage, cleanup_candidate,
    file_type,
};

use encoding::{datetime, decode_path, encode_path, time_parts, unix_millis};

/// Applied in order. `user_version` counts how many have run.
const MIGRATIONS: [&str; 3] = [
    include_str!("../sql/001_initial.sql"),
    include_str!("../sql/002_exact_duplicates.sql"),
    include_str!("../sql/003_similar_images.sql"),
];
const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;
const LIST_SCANS: &str = include_str!("../sql/list_scans.sql");
const LIST_FILES: &str = include_str!("../sql/list_files.sql");
const DUPLICATE_CANDIDATES: &str = include_str!("../sql/duplicate_candidates.sql");
const LIST_DUPLICATE_FILES: &str = include_str!("../sql/list_duplicate_files.sql");
const LIST_DUPLICATE_GROUPS: &str = include_str!("../sql/list_duplicate_groups.sql");
const LIST_DUPLICATE_GROUP_FILES: &str = include_str!("../sql/list_duplicate_group_files.sql");
const LIST_SIMILAR_IMAGE_GROUPS: &str = include_str!("../sql/list_similar_image_groups.sql");
const LIST_SIMILAR_IMAGE_GROUP_FILES: &str =
    include_str!("../sql/list_similar_image_group_files.sql");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateEvidence {
    pub file_count: u64,
    pub hash: [u8; 32],
}

/// A duplicate or similar-image group, summarized for the review list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewGroup {
    pub group_id: u64,
    pub file_count: u64,
    pub logical_bytes: u64,
}

/// Why an image was grouped: its difference hash and distance from the group's reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimilarImageEvidence {
    pub difference_hash: u64,
    pub distance: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTypeTotal {
    pub file_type: &'static str,
    pub files: u64,
    pub logical_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SavedScan {
    pub id: i64,
    pub root: PathBuf,
    pub started_unix_ms: i64,
    pub progress: ScanProgress,
    pub cancelled: bool,
    pub first_error: Option<String>,
    pub large_file_count: u64,
    pub large_file_threshold: u64,
    pub duplicates_analyzed: bool,
    pub duplicate_group_count: u64,
    pub duplicate_error_count: u64,
    pub similar_images_analyzed: bool,
    pub similar_group_count: u64,
    pub image_error_count: u64,
}

pub struct ScanStore(Connection);

impl ScanStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let version = schema_version(&connection)?;
        if (0..SCHEMA_VERSION).contains(&version) {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Another app instance may have migrated while this connection waited.
            let version = schema_version(&transaction)?;
            if !(0..=SCHEMA_VERSION).contains(&version) {
                return Err(incompatible_schema(version));
            }
            for migration in &MIGRATIONS[version as usize..] {
                transaction.execute_batch(migration)?;
            }
            transaction.commit()?;
        } else if version != SCHEMA_VERSION {
            return Err(incompatible_schema(version));
        }
        Ok(Self(connection))
    }

    pub fn begin_scan(
        &mut self,
        root: &Path,
        large_file_threshold: u64,
    ) -> rusqlite::Result<ScanWriter<'_>> {
        // Each scan holds the write lock until it finishes, so a second app instance waits here.
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(code, _)
                    if code.code == rusqlite::ErrorCode::DatabaseBusy =>
                {
                    rusqlite::Error::SqliteFailure(
                        code,
                        Some(
                            "another Storage Clinic window is saving a scan. Close it and scan again"
                                .into(),
                        ),
                    )
                }
                error => error,
            })?;
        let started_unix_ms = unix_millis(SystemTime::now())?;
        transaction.execute(
            "INSERT INTO scans (root_key, display_root, started_unix_ms, status, large_file_threshold)
             VALUES (?1, ?2, ?3, 'running', ?4)",
            params![encode_path(root), root.to_string_lossy(), started_unix_ms, sqlite_integer(large_file_threshold)?],
        )?;
        let scan = SavedScan {
            id: transaction.last_insert_rowid(),
            root: root.into(),
            started_unix_ms,
            progress: ScanProgress::default(),
            cancelled: false,
            first_error: None,
            large_file_count: 0,
            large_file_threshold,
            duplicates_analyzed: false,
            duplicate_group_count: 0,
            duplicate_error_count: 0,
            similar_images_analyzed: false,
            similar_group_count: 0,
            image_error_count: 0,
        };
        Ok(ScanWriter { transaction, scan })
    }

    pub fn scans(&self, limit: u32, offset: u64) -> rusqlite::Result<Vec<SavedScan>> {
        let mut statement = self.0.prepare(LIST_SCANS)?;
        statement
            .query_map(
                named_params! {":limit": limit, ":offset": sqlite_integer(offset)?},
                |row| {
                    Ok(SavedScan {
                        id: row.get(0)?,
                        root: decode_path(row, 1)?,
                        started_unix_ms: row.get(2)?,
                        cancelled: row.get::<_, String>(4)? == "cancelled",
                        progress: ScanProgress {
                            files: unsigned_integer(row, 5)?,
                            directories: unsigned_integer(row, 6)?,
                            logical_bytes: unsigned_integer(row, 7)?,
                            skipped: unsigned_integer(row, 10)?,
                            errors: unsigned_integer(row, 11)?,
                        },
                        large_file_count: unsigned_integer(row, 8)?,
                        large_file_threshold: unsigned_integer(row, 9)?,
                        first_error: row.get(12)?,
                        duplicates_analyzed: row.get(13)?,
                        duplicate_group_count: unsigned_integer(row, 14)?,
                        duplicate_error_count: unsigned_integer(row, 15)?,
                        similar_images_analyzed: row.get(16)?,
                        similar_group_count: unsigned_integer(row, 17)?,
                        image_error_count: unsigned_integer(row, 18)?,
                    })
                },
            )?
            .collect()
    }

    pub fn files(
        &self,
        scan_id: i64,
        search: &str,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        self.file_page(LIST_FILES, scan_id, search, limit, offset)
    }

    pub fn duplicate_files(
        &self,
        scan_id: i64,
        search: &str,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        self.file_page(LIST_DUPLICATE_FILES, scan_id, search, limit, offset)
    }

    pub fn duplicate_evidence(
        &self,
        scan_id: i64,
        files: &[FileRecord],
    ) -> rusqlite::Result<HashMap<PathBuf, DuplicateEvidence>> {
        let mut evidence = HashMap::new();
        let mut statement = self.0.prepare_cached(
            "SELECT file_count, hash
             FROM duplicate_members
             JOIN duplicate_groups USING (scan_id, group_id)
             WHERE duplicate_members.scan_id = ?1 AND path_key = ?2",
        )?;
        for file in files {
            let entry = statement
                .query_row(params![scan_id, encode_path(&file.path)], |row| {
                    Ok(DuplicateEvidence {
                        file_count: unsigned_integer(row, 0)?,
                        hash: row.get(1)?,
                    })
                })
                .optional()?;
            if let Some(entry) = entry {
                evidence.insert(file.path.clone(), entry);
            }
        }
        Ok(evidence)
    }

    /// Exact duplicate groups, largest total logical size first.
    pub fn duplicate_groups(
        &self,
        scan_id: i64,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<ReviewGroup>> {
        self.review_groups(LIST_DUPLICATE_GROUPS, scan_id, limit, offset)
    }

    pub fn duplicate_group_files(
        &self,
        scan_id: i64,
        group_id: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let mut statement = self.0.prepare_cached(LIST_DUPLICATE_GROUP_FILES)?;
        statement
            .query_map(
                named_params! {":scan_id": scan_id, ":group_id": sqlite_integer(group_id)?},
                file_record,
            )?
            .collect()
    }

    /// Similar-image groups, largest total logical size first.
    pub fn similar_image_groups(
        &self,
        scan_id: i64,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<ReviewGroup>> {
        self.review_groups(LIST_SIMILAR_IMAGE_GROUPS, scan_id, limit, offset)
    }

    /// Group members with their evidence, reference image first.
    pub fn similar_image_group_files(
        &self,
        scan_id: i64,
        group_id: u64,
    ) -> rusqlite::Result<Vec<(FileRecord, SimilarImageEvidence)>> {
        let mut statement = self.0.prepare_cached(LIST_SIMILAR_IMAGE_GROUP_FILES)?;
        statement
            .query_map(
                named_params! {":scan_id": scan_id, ":group_id": sqlite_integer(group_id)?},
                |row| {
                    let hash: i64 = row.get(8)?;
                    Ok((
                        file_record(row)?,
                        SimilarImageEvidence {
                            // SQLite integers are signed. The hash keeps its 64 bits.
                            difference_hash: hash as u64,
                            distance: row.get(9)?,
                        },
                    ))
                },
            )?
            .collect()
    }

    fn review_groups(
        &self,
        query: &str,
        scan_id: i64,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<ReviewGroup>> {
        let mut statement = self.0.prepare_cached(query)?;
        statement
            .query_map(
                named_params! {
                    ":scan_id": scan_id,
                    ":limit": limit,
                    ":offset": sqlite_integer(offset)?
                },
                |row| {
                    Ok(ReviewGroup {
                        group_id: unsigned_integer(row, 0)?,
                        file_count: unsigned_integer(row, 1)?,
                        logical_bytes: unsigned_integer(row, 2)?,
                    })
                },
            )?
            .collect()
    }

    /// Files matching a cleanup rule, largest first. Rules run on the saved metadata, so
    /// older scans gain candidates when the rules change.
    // ponytail: classifies every file row per call, like `file_types`; store the kind if slow.
    pub fn cleanup_candidates(
        &self,
        scan_id: i64,
        scanned_at: SystemTime,
    ) -> rusqlite::Result<Vec<(FileRecord, CleanupCandidate)>> {
        Ok(self
            .file_page(LIST_FILES, scan_id, "", u32::MAX, 0)?
            .into_iter()
            .filter_map(|file| cleanup_candidate(&file, scanned_at).map(|found| (file, found)))
            .collect())
    }

    /// Totals indexed files by type, largest first. Cancelled scans cover the indexed part.
    // ponytail: reads every file row per call; store a type column if large scans show it.
    pub fn file_types(&self, scan_id: i64) -> rusqlite::Result<Vec<FileTypeTotal>> {
        let mut totals: HashMap<&'static str, FileTypeTotal> = HashMap::new();
        let mut statement = self
            .0
            .prepare_cached("SELECT path_key, logical_bytes FROM files WHERE scan_id = ?1")?;
        let mut rows = statement.query([scan_id])?;
        while let Some(row) = rows.next()? {
            let file_type = file_type(&decode_path(row, 0)?);
            let total = totals.entry(file_type).or_insert(FileTypeTotal {
                file_type,
                files: 0,
                logical_bytes: 0,
            });
            total.files += 1;
            total.logical_bytes += unsigned_integer(row, 1)?;
        }
        let mut totals: Vec<_> = totals.into_values().collect();
        totals.sort_unstable_by(|left, right| {
            right
                .logical_bytes
                .cmp(&left.logical_bytes)
                .then_with(|| left.file_type.cmp(right.file_type))
        });
        Ok(totals)
    }

    fn file_page(
        &self,
        query: &str,
        scan_id: i64,
        search: &str,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let mut statement = self.0.prepare_cached(query)?;
        statement
            .query_map(
                named_params! {
                    ":scan_id": scan_id,
                    ":search": search,
                    ":limit": limit,
                    ":offset": sqlite_integer(offset)?
                },
                file_record,
            )?
            .collect()
    }
}

pub struct ScanWriter<'a> {
    transaction: Transaction<'a>,
    scan: SavedScan,
}

impl ScanWriter<'_> {
    pub fn insert_file(&mut self, file: &FileRecord) -> rusqlite::Result<()> {
        let (created_seconds, created_nanos) = time_parts(file.created)?;
        let (modified_seconds, modified_nanos) = time_parts(file.modified)?;
        self.transaction
            .prepare_cached(
                "INSERT INTO files (scan_id, path_key, display_path, logical_bytes,
                                created_unix_seconds, created_subsec_nanos, modified_unix_seconds,
                                modified_subsec_nanos, read_only)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?
            .execute(params![
                self.scan.id,
                encode_path(&file.path),
                file.path.to_string_lossy(),
                sqlite_integer(file.logical_bytes)?,
                created_seconds,
                created_nanos,
                modified_seconds,
                modified_nanos,
                file.read_only,
            ])?;
        Ok(())
    }

    pub fn next_duplicate_candidates(
        &self,
        after_size: Option<u64>,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let after_size = after_size.map_or(-1, |size| i64::try_from(size).unwrap_or(i64::MAX));
        let mut statement = self.transaction.prepare_cached(DUPLICATE_CANDIDATES)?;
        statement
            .query_map(params![self.scan.id, after_size], file_record)?
            .collect()
    }

    pub fn insert_duplicate_group(
        &mut self,
        paths: &[PathBuf],
        hash: &[u8; 32],
    ) -> rusqlite::Result<()> {
        let group_id = self.scan.duplicate_group_count + 1;
        let group_key = sqlite_integer(group_id)?;
        let savepoint = self.transaction.savepoint()?;
        savepoint.execute(
            "INSERT INTO duplicate_groups (scan_id, group_id, file_count, hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                self.scan.id,
                group_key,
                sqlite_integer(paths.len() as u64)?,
                hash.as_slice(),
            ],
        )?;
        {
            let mut statement = savepoint.prepare_cached(
                "INSERT INTO duplicate_members (scan_id, path_key, group_id, logical_bytes)
                 VALUES (?1, ?2, ?3, (
                     SELECT logical_bytes FROM files WHERE scan_id = ?1 AND path_key = ?2
                 ))",
            )?;
            for path in paths {
                statement.execute(params![self.scan.id, encode_path(path), group_key])?;
            }
        }
        savepoint.commit()?;
        self.scan.duplicate_group_count = group_id;
        Ok(())
    }

    pub fn mark_duplicates_analyzed(&mut self, error_count: u64) -> rusqlite::Result<()> {
        self.transaction.execute(
            "UPDATE scans SET duplicates_analyzed = 1, duplicate_error_count = ?1 WHERE id = ?2",
            params![sqlite_integer(error_count)?, self.scan.id],
        )?;
        self.scan.duplicates_analyzed = true;
        self.scan.duplicate_error_count = error_count;
        Ok(())
    }

    pub fn insert_similar_image_group(&mut self, members: &[SimilarImage]) -> rusqlite::Result<()> {
        let group_id = self.scan.similar_group_count + 1;
        let group_key = sqlite_integer(group_id)?;
        let savepoint = self.transaction.savepoint()?;
        savepoint.execute(
            "INSERT INTO similar_image_groups (scan_id, group_id, file_count) VALUES (?1, ?2, ?3)",
            params![
                self.scan.id,
                group_key,
                sqlite_integer(members.len() as u64)?
            ],
        )?;
        {
            let mut statement = savepoint.prepare_cached(
                "INSERT INTO similar_image_members
                     (scan_id, path_key, group_id, position, difference_hash, distance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (position, member) in members.iter().enumerate() {
                statement.execute(params![
                    self.scan.id,
                    encode_path(&member.path),
                    group_key,
                    sqlite_integer(position as u64)?,
                    // SQLite integers are signed. Keep all 64 hash bits.
                    member.hash as i64,
                    member.distance,
                ])?;
            }
        }
        savepoint.commit()?;
        self.scan.similar_group_count = group_id;
        Ok(())
    }

    pub fn mark_similar_images_analyzed(&mut self, error_count: u64) -> rusqlite::Result<()> {
        self.transaction.execute(
            "UPDATE scans SET similar_images_analyzed = 1, image_error_count = ?1 WHERE id = ?2",
            params![sqlite_integer(error_count)?, self.scan.id],
        )?;
        self.scan.similar_images_analyzed = true;
        self.scan.image_error_count = error_count;
        Ok(())
    }

    pub fn finish(
        mut self,
        summary: &ScanSummary,
        large_file_count: u64,
    ) -> rusqlite::Result<SavedScan> {
        let progress = summary.progress;
        let status = if summary.cancelled {
            "cancelled"
        } else {
            "completed"
        };
        self.scan.duplicates_analyzed &= !summary.cancelled;
        self.scan.similar_images_analyzed &= !summary.cancelled;
        self.transaction.execute(
            "UPDATE scans SET finished_unix_ms = ?1, status = ?2, files = ?3, directories = ?4,
                              logical_bytes = ?5, large_files = ?6, skipped = ?7, errors = ?8,
                              first_error = ?9, duplicates_analyzed = ?10,
                              duplicate_group_count = ?11, similar_images_analyzed = ?12,
                              similar_group_count = ?13
             WHERE id = ?14",
            params![
                unix_millis(SystemTime::now())?,
                status,
                sqlite_integer(progress.files)?,
                sqlite_integer(progress.directories)?,
                sqlite_integer(progress.logical_bytes)?,
                sqlite_integer(large_file_count)?,
                sqlite_integer(progress.skipped)?,
                sqlite_integer(progress.errors)?,
                summary.first_error,
                self.scan.duplicates_analyzed,
                sqlite_integer(self.scan.duplicate_group_count)?,
                self.scan.similar_images_analyzed,
                sqlite_integer(self.scan.similar_group_count)?,
                self.scan.id,
            ],
        )?;
        self.transaction.commit()?;
        self.scan.progress = progress;
        self.scan.cancelled = summary.cancelled;
        self.scan.first_error.clone_from(&summary.first_error);
        self.scan.large_file_count = large_file_count;
        Ok(self.scan)
    }
}

fn file_record(row: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        path: decode_path(row, 0)?,
        logical_bytes: unsigned_integer(row, 2)?,
        created: datetime(row, 3)?,
        modified: datetime(row, 5)?,
        read_only: row.get(7)?,
    })
}

fn schema_version(connection: &Connection) -> rusqlite::Result<i64> {
    connection.pragma_query_value(None, "user_version", |row: &Row<'_>| row.get(0))
}

fn incompatible_schema(version: i64) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Integer,
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported scan database version {version}"),
        )
        .into(),
    )
}

fn sqlite_integer(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))
}

fn unsigned_integer(row: &Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
