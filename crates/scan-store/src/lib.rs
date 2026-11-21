mod cleanup;
mod encoding;
mod hash_cache;
pub use cleanup::{CleanupItem, CleanupSession, CleanupState, QueueEntry};

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::functions::FunctionFlags;
use rusqlite::{
    Connection, OptionalExtension, Row, Transaction, TransactionBehavior, named_params, params,
    types::Type,
};
use scan_engine::{
    CleanupCandidate, DirectoryRecord, FileRecord, ScanProgress, ScanSummary, SimilarImage,
    cleanup_candidate, file_type,
};

use encoding::{datetime, decode_path, encode_path, time_parts, unix_millis};

/// Applied in order. `user_version` counts how many have run.
const MIGRATIONS: [&str; 6] = [
    include_str!("../sql/001_initial.sql"),
    include_str!("../sql/002_exact_duplicates.sql"),
    include_str!("../sql/003_similar_images.sql"),
    include_str!("../sql/004_directories.sql"),
    include_str!("../sql/005_hash_cache.sql"),
    include_str!("../sql/006_cleanup.sql"),
];
const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;
const LIST_SCANS: &str = include_str!("../sql/list_scans.sql");
const LIST_FILES: &str = include_str!("../sql/list_files.sql");
const DUPLICATE_CANDIDATES: &str = include_str!("../sql/duplicate_candidates.sql");
const LIST_DUPLICATE_FILES: &str = include_str!("../sql/list_duplicate_files.sql");
const LIST_DUPLICATE_GROUPS: &str = include_str!("../sql/list_duplicate_groups.sql");
const LIST_DUPLICATE_GROUP_FILES: &str = include_str!("../sql/list_duplicate_group_files.sql");
const LIST_SIMILAR_IMAGE_GROUPS: &str = include_str!("../sql/list_similar_image_groups.sql");
const EXPORT_FILES: &str = include_str!("../sql/export_files.sql");
const LIST_SIMILAR_IMAGE_GROUP_FILES: &str =
    include_str!("../sql/list_similar_image_group_files.sql");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateEvidence {
    pub file_count: u64,
    pub hash: [u8; 32],
}

/// Order for file pages. Ties fall back to largest first, then native path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileSort {
    #[default]
    Largest,
    Smallest,
    Newest,
    Oldest,
    Name,
}

/// Narrows a file page. The default matches every file, largest first.
#[derive(Clone, Debug, Default)]
pub struct FileFilter {
    /// Literal, case-insensitive path substring.
    pub search: String,
    /// One of `scan_engine::FILE_TYPES`.
    pub file_type: Option<&'static str>,
    /// Only files last modified before this time.
    pub modified_before: Option<SystemTime>,
    pub sort: FileSort,
    pub duplicates_only: bool,
}

/// A duplicate or similar-image group, summarized for the review list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewGroup {
    pub group_id: u64,
    pub file_count: u64,
    pub logical_bytes: u64,
}

/// One indexed file with the groups it belongs to, for export.
#[derive(Debug)]
pub struct ExportedFile {
    pub file: FileRecord,
    /// Byte-verified duplicate group.
    pub duplicate_group: Option<u64>,
    /// Heuristic similar-image group and the distance from its reference image.
    pub similar_image: Option<(u64, u32)>,
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

/// One direct child of a folder, totalled over every indexed file beneath it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderEntry {
    pub name: OsString,
    pub is_folder: bool,
    pub files: u64,
    pub logical_bytes: u64,
    /// The file type holding the most logical bytes beneath this entry.
    pub main_type: &'static str,
}

/// A shared developer cache found in a scan, such as the npm cache or Xcode DerivedData.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegenerableCache {
    pub label: &'static str,
    pub root: PathBuf,
    pub files: u64,
    pub logical_bytes: u64,
}

/// A folder's saved totals: its direct children and its file types, each largest first.
/// Folders without indexed files, such as empty folders, do not appear.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FolderSummary {
    pub files: u64,
    pub logical_bytes: u64,
    pub entries: Vec<FolderEntry>,
    pub types: Vec<FileTypeTotal>,
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
    pub hash_cache_hits: u64,
    pub hash_cache_misses: u64,
}

impl SavedScan {
    /// The saved observation time, shared by review, Inspector, and export rules.
    pub fn started_at(&self) -> io::Result<SystemTime> {
        let duration = Duration::from_millis(self.started_unix_ms.unsigned_abs());
        let time = if self.started_unix_ms < 0 {
            SystemTime::UNIX_EPOCH.checked_sub(duration)
        } else {
            SystemTime::UNIX_EPOCH.checked_add(duration)
        };
        time.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Scan time is outside the supported range",
            )
        })
    }
}

pub struct ScanStore(Connection);

impl ScanStore {
    /// The app-owned database location, used to keep exports away from persistent state.
    pub fn database_path(&self) -> Option<&Path> {
        self.0.path().map(Path::new)
    }

    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        // Lets file queries filter by the same type names the scan summary shows.
        connection.create_scalar_function(
            "file_type",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(file_type(Path::new(&context.get::<String>(0)?))),
        )?;
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
        // A large scan can spill pages before committing; WAL keeps saved history readable.
        connection.pragma_update(None, "journal_mode", "WAL")?;
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
            hash_cache_hits: 0,
            hash_cache_misses: 0,
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
                        hash_cache_hits: unsigned_integer(row, 19)?,
                        hash_cache_misses: unsigned_integer(row, 20)?,
                    })
                },
            )?
            .collect()
    }

    pub fn empty_directories(
        &self,
        scan_id: i64,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<DirectoryRecord>> {
        let mut statement = self.0.prepare_cached(
            "SELECT path_key FROM directories WHERE scan_id = ?1 AND empty = 1 ORDER BY path_key LIMIT ?2 OFFSET ?3"
        )?;
        statement
            .query_map(params![scan_id, limit, sqlite_integer(offset)?], |row| {
                Ok(DirectoryRecord {
                    path: decode_path(row, 0)?,
                    empty: Some(true),
                })
            })?
            .collect()
    }

    pub fn files(
        &self,
        scan_id: i64,
        search: &str,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let filter = FileFilter {
            search: search.into(),
            ..FileFilter::default()
        };
        self.filtered_files(scan_id, &filter, limit, offset)
    }

    pub fn duplicate_files(
        &self,
        scan_id: i64,
        search: &str,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let filter = FileFilter {
            search: search.into(),
            duplicates_only: true,
            ..FileFilter::default()
        };
        self.filtered_files(scan_id, &filter, limit, offset)
    }

    // ponytail: filters and non-size sorts scan the whole saved index per page (tens of
    // milliseconds at 100,000 files). Add indexes or a stored type column if this shows up.
    pub fn filtered_files(
        &self,
        scan_id: i64,
        filter: &FileFilter,
        limit: u32,
        offset: u64,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let query = if filter.duplicates_only {
            LIST_DUPLICATE_FILES
        } else {
            LIST_FILES
        };
        let (modified_before, modified_before_nanos) = time_parts(filter.modified_before)?;
        // Result-column positions (3 = size, 6 = modified, 2 = display path, 1 = native key)
        // keep each order index-friendly in both queries. Every string here is a constant.
        let order = match filter.sort {
            FileSort::Largest => "3 DESC, 1",
            FileSort::Smallest => "3, 1",
            FileSort::Newest => "6 DESC, 7 DESC, 3 DESC, 1",
            FileSort::Oldest => "modified_unix_seconds IS NULL, 6, 7, 3 DESC, 1",
            FileSort::Name => "2 COLLATE NOCASE, 3 DESC, 1",
        };
        let query = format!("{query}ORDER BY {order}\nLIMIT :limit OFFSET :offset");
        let mut statement = self.0.prepare_cached(&query)?;
        statement
            .query_map(
                named_params! {
                    ":scan_id": scan_id,
                    ":search": filter.search,
                    ":file_type": filter.file_type.unwrap_or(""),
                    ":modified_before": modified_before,
                    ":modified_before_nanos": modified_before_nanos,
                    ":limit": limit,
                    ":offset": sqlite_integer(offset)?
                },
                file_record,
            )?
            .collect()
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
        limit: u32,
    ) -> rusqlite::Result<Vec<FileRecord>> {
        let mut statement = self.0.prepare_cached(LIST_DUPLICATE_GROUP_FILES)?;
        statement
            .query_map(
                named_params! {":scan_id": scan_id, ":group_id": sqlite_integer(group_id)?, ":limit": limit},
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
        limit: u32,
    ) -> rusqlite::Result<Vec<(FileRecord, SimilarImageEvidence)>> {
        let mut statement = self.0.prepare_cached(LIST_SIMILAR_IMAGE_GROUP_FILES)?;
        statement
            .query_map(
                named_params! {":scan_id": scan_id, ":group_id": sqlite_integer(group_id)?, ":limit": limit},
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

    /// Streams every indexed file and its evidence in stable size order.
    pub fn visit_exported_files(
        &self,
        scan_id: i64,
        mut visit: impl FnMut(ExportedFile) -> io::Result<()>,
    ) -> io::Result<u64> {
        let mut statement = self.0.prepare(EXPORT_FILES).map_err(io::Error::other)?;
        let mut rows = statement
            .query(named_params! {":scan_id": scan_id})
            .map_err(io::Error::other)?;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(io::Error::other)? {
            let record = (|| -> rusqlite::Result<ExportedFile> {
                Ok(ExportedFile {
                    file: file_record(row)?,
                    duplicate_group: optional_unsigned(row, 8)?,
                    similar_image: optional_unsigned(row, 9)?
                        .map(|group| row.get(10).map(|distance| (group, distance)))
                        .transpose()?,
                })
            })()
            .map_err(io::Error::other)?;
            visit(record)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn exported_files(&self, scan_id: i64) -> io::Result<Vec<ExportedFile>> {
        let mut files = Vec::new();
        self.visit_exported_files(scan_id, |file| {
            files.push(file);
            Ok(())
        })?;
        Ok(files)
    }

    /// Streams metadata candidates largest first; callers may retain a bounded preview.
    pub fn visit_cleanup_candidates(
        &self,
        scan_id: i64,
        scanned_at: SystemTime,
        mut visit: impl FnMut(FileRecord, CleanupCandidate),
    ) -> rusqlite::Result<()> {
        let query = format!("{LIST_FILES}ORDER BY 3 DESC, 1");
        let mut statement = self.0.prepare_cached(&query)?;
        let mut rows = statement.query(named_params! {":scan_id": scan_id, ":search": "", ":file_type": "", ":modified_before": Option::<i64>::None, ":modified_before_nanos": Option::<u32>::None})?;
        while let Some(row) = rows.next()? {
            let file = file_record(row)?;
            if let Some(candidate) = cleanup_candidate(&file, scanned_at) {
                visit(file, candidate);
            }
        }
        Ok(())
    }

    pub fn cleanup_candidates(
        &self,
        scan_id: i64,
        scanned_at: SystemTime,
    ) -> rusqlite::Result<Vec<(FileRecord, CleanupCandidate)>> {
        let mut candidates = Vec::new();
        self.visit_cleanup_candidates(scan_id, scanned_at, |file, candidate| {
            candidates.push((file, candidate))
        })?;
        Ok(candidates)
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
        Ok(largest_types_first(totals))
    }

    /// Reopens a saved scan's index for duplicate and similar-image analysis. Earlier groups,
    /// counts and analysis flags are cleared in the same transaction, so analysis can run
    /// again; nothing is visible to readers until `ScanWriter::finish_analysis` commits.
    pub fn begin_analysis(&mut self, scan: &SavedScan) -> rusqlite::Result<ScanWriter<'_>> {
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for statement in [
            "DELETE FROM duplicate_groups WHERE scan_id = ?1",
            "DELETE FROM similar_image_groups WHERE scan_id = ?1",
            "UPDATE scans SET duplicates_analyzed = 0, duplicate_group_count = 0,
                              duplicate_error_count = 0, similar_images_analyzed = 0,
                              similar_group_count = 0, image_error_count = 0,
                              hash_cache_hits = 0, hash_cache_misses = 0
             WHERE id = ?1",
        ] {
            transaction.execute(statement, [scan.id])?;
        }
        let scan = SavedScan {
            duplicates_analyzed: false,
            duplicate_group_count: 0,
            duplicate_error_count: 0,
            similar_images_analyzed: false,
            similar_group_count: 0,
            image_error_count: 0,
            hash_cache_hits: 0,
            hash_cache_misses: 0,
            ..scan.clone()
        };
        Ok(ScanWriter { transaction, scan })
    }

    /// Indexed files that share their size with another file: the duplicate candidates.
    pub fn duplicate_candidate_count(&self, scan_id: i64) -> rusqlite::Result<u64> {
        self.0
            .prepare_cached(
                "SELECT count(*) FROM files
                 WHERE scan_id = ?1 AND logical_bytes IN (
                     SELECT logical_bytes FROM files WHERE scan_id = ?1
                     GROUP BY logical_bytes HAVING count(*) > 1
                 )",
            )?
            .query_row([scan_id], |row| unsigned_integer(row, 0))
    }

    /// Indexed files with an image extension the similar-image check can decode.
    pub fn image_files(&self, scan_id: i64) -> rusqlite::Result<Vec<FileRecord>> {
        let mut statement = self.0.prepare_cached(
            "SELECT path_key, display_path, logical_bytes, created_unix_seconds,
                    created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
             FROM files WHERE scan_id = ?1 ORDER BY path_key",
        )?;
        let mut images = Vec::new();
        for file in statement.query_map([scan_id], file_record)? {
            let file = file?;
            if scan_engine::is_hashable_image(&file.path) {
                images.push(file);
            }
        }
        Ok(images)
    }

    /// Totals the direct children of `folder` from saved native paths, for the Space Map.
    /// Cancelled scans cover the indexed part.
    // ponytail: reads every file row beneath `folder` per call (about 0.3 s at the root of a
    // 1M-file scan in a release build); save per-folder totals during the scan if that grows.
    pub fn folder_summary(&self, scan_id: i64, folder: &Path) -> rusqlite::Result<FolderSummary> {
        #[derive(Default)]
        struct ChildTotals {
            is_folder: bool,
            files: u64,
            logical_bytes: u64,
            bytes_by_type: HashMap<&'static str, u64>,
        }

        let mut children: HashMap<OsString, ChildTotals> = HashMap::new();
        let mut types: HashMap<&'static str, FileTypeTotal> = HashMap::new();
        // Every path beneath `folder` starts with its encoded form plus a separator, so a key
        // range on the primary key reads only those rows. The separator's last encoded byte is
        // never 0xFF, so incrementing it gives the exclusive upper bound.
        let lower = encode_path(&folder.join(""));
        let mut upper = lower.clone();
        if let Some(last) = upper.last_mut() {
            *last += 1;
        }
        let mut statement = self.0.prepare_cached(
            "SELECT path_key, logical_bytes FROM files
             WHERE scan_id = ?1 AND path_key >= ?2 AND path_key < ?3",
        )?;
        let mut rows = statement.query(params![scan_id, lower, upper])?;
        while let Some(row) = rows.next()? {
            let path = decode_path(row, 0)?;
            let Ok(inside) = path.strip_prefix(folder) else {
                continue;
            };
            let mut components = inside.components();
            let Some(first) = components.next() else {
                continue;
            };
            let logical_bytes = unsigned_integer(row, 1)?;
            let file_type = file_type(&path);

            let child = children.entry(first.as_os_str().to_owned()).or_default();
            child.is_folder |= components.next().is_some();
            child.files += 1;
            child.logical_bytes += logical_bytes;
            *child.bytes_by_type.entry(file_type).or_default() += logical_bytes;

            let total = types.entry(file_type).or_insert(FileTypeTotal {
                file_type,
                files: 0,
                logical_bytes: 0,
            });
            total.files += 1;
            total.logical_bytes += logical_bytes;
        }

        let mut entries: Vec<_> = children
            .into_iter()
            .map(|(name, child)| FolderEntry {
                main_type: child
                    .bytes_by_type
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(left.0)))
                    .map_or(scan_engine::OTHER_FILE_TYPE, |(file_type, _)| file_type),
                name,
                is_folder: child.is_folder,
                files: child.files,
                logical_bytes: child.logical_bytes,
            })
            .collect();
        entries.sort_unstable_by(|left, right| {
            right
                .logical_bytes
                .cmp(&left.logical_bytes)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(FolderSummary {
            files: entries.iter().map(|entry| entry.files).sum(),
            logical_bytes: entries.iter().map(|entry| entry.logical_bytes).sum(),
            entries,
            types: largest_types_first(types),
        })
    }

    /// Shared developer caches in the scan, largest first. Their tools can rebuild them.
    // ponytail: reads every file row per call, like `file_types`; record caches during
    // the scan if large scans make Overview slow.
    pub fn regenerable_caches(&self, scan_id: i64) -> rusqlite::Result<Vec<RegenerableCache>> {
        let mut caches: Vec<RegenerableCache> = Vec::new();
        let mut statement = self
            .0
            .prepare_cached("SELECT path_key, logical_bytes FROM files WHERE scan_id = ?1")?;
        let mut rows = statement.query([scan_id])?;
        while let Some(row) = rows.next()? {
            let path = decode_path(row, 0)?;
            let Some((label, root)) = scan_engine::regenerable_cache(&path) else {
                continue;
            };
            let logical_bytes = unsigned_integer(row, 1)?;
            match caches.iter_mut().find(|cache| cache.root == root) {
                Some(cache) => {
                    cache.files += 1;
                    cache.logical_bytes += logical_bytes;
                }
                None => caches.push(RegenerableCache {
                    label,
                    root,
                    files: 1,
                    logical_bytes,
                }),
            }
        }
        caches.sort_unstable_by_key(|cache| std::cmp::Reverse(cache.logical_bytes));
        Ok(caches)
    }

    /// Logical bytes by UTC year of last modification, oldest year first.
    /// Files without a saved modification time are left out.
    pub fn modified_years(&self, scan_id: i64) -> rusqlite::Result<Vec<(i32, u64)>> {
        let mut statement = self.0.prepare_cached(
            "SELECT CAST(strftime('%Y', modified_unix_seconds, 'unixepoch') AS INTEGER) AS year,
                    sum(logical_bytes)
             FROM files
             WHERE scan_id = ?1 AND modified_unix_seconds IS NOT NULL
             GROUP BY year HAVING year IS NOT NULL ORDER BY year",
        )?;
        statement
            .query_map([scan_id], |row| {
                Ok((row.get(0)?, unsigned_integer(row, 1)?))
            })?
            .collect()
    }
}

fn largest_types_first(totals: HashMap<&'static str, FileTypeTotal>) -> Vec<FileTypeTotal> {
    let mut totals: Vec<_> = totals.into_values().collect();
    totals.sort_unstable_by(|left, right| {
        right
            .logical_bytes
            .cmp(&left.logical_bytes)
            .then_with(|| left.file_type.cmp(right.file_type))
    });
    totals
}

pub struct ScanWriter<'a> {
    transaction: Transaction<'a>,
    scan: SavedScan,
}

impl ScanWriter<'_> {
    pub fn insert_directory(&mut self, directory: &DirectoryRecord) -> rusqlite::Result<()> {
        self.transaction.prepare_cached(
            "INSERT INTO directories (scan_id, path_key, display_path, empty) VALUES (?1, ?2, ?3, ?4)"
        )?.execute(params![self.scan.id, encode_path(&directory.path), directory.path.to_string_lossy(), directory.empty])?;
        Ok(())
    }

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

    pub fn record_duplicate_errors(&mut self, error_count: u64) {
        self.scan.duplicate_error_count = error_count;
    }

    pub fn record_image_errors(&mut self, error_count: u64) {
        self.scan.image_error_count = error_count;
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

    /// Commits analysis started with `ScanStore::begin_analysis`. Groups found before a
    /// cancellation are kept; only completed passes are marked analyzed.
    pub fn finish_analysis(self) -> rusqlite::Result<SavedScan> {
        self.transaction.execute(
            "UPDATE scans SET duplicate_group_count = ?1, similar_group_count = ?2,
                              duplicate_error_count = ?3, image_error_count = ?4
             WHERE id = ?5",
            params![
                sqlite_integer(self.scan.duplicate_group_count)?,
                sqlite_integer(self.scan.similar_group_count)?,
                sqlite_integer(self.scan.duplicate_error_count)?,
                sqlite_integer(self.scan.image_error_count)?,
                self.scan.id,
            ],
        )?;
        self.transaction.commit()?;
        Ok(self.scan)
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
                              similar_group_count = ?13, duplicate_error_count = ?15, image_error_count = ?16
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
                sqlite_integer(self.scan.duplicate_error_count)?,
                sqlite_integer(self.scan.image_error_count)?,
            ],
        )?;
        // Bound stale cache history without deleting scan evidence. Active versions are refreshed on hits.
        self.transaction.execute(
            "DELETE FROM hash_cache WHERE last_scan_id < ?1",
            [self.scan.id.saturating_sub(20)],
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

fn optional_unsigned(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    let value: Option<i64> = row.get(column)?;
    value.map(|_| unsigned_integer(row, column)).transpose()
}

fn unsigned_integer(row: &Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
