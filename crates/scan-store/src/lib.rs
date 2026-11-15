mod encoding;

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{
    Connection, Row, Transaction, TransactionBehavior, named_params, params, types::Type,
};
use scan_engine::{FileRecord, ScanProgress, ScanSummary};

use encoding::{datetime, decode_path, encode_path, time_parts, unix_millis};

const SCHEMA: &str = include_str!("../sql/001_initial.sql");
const SCHEMA_VERSION: i64 = 1;
const LIST_SCANS: &str = include_str!("../sql/list_scans.sql");
const LIST_FILES: &str = include_str!("../sql/list_files.sql");

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
}

pub struct ScanStore(Connection);

impl ScanStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let version = schema_version(&connection)?;
        if version == 0 {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Another app instance may have migrated while this connection waited.
            match schema_version(&transaction)? {
                0 => transaction.execute_batch(SCHEMA)?,
                SCHEMA_VERSION => {}
                version => return Err(incompatible_schema(version)),
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
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        let mut statement = self.0.prepare(LIST_FILES)?;
        statement.query_map(
            named_params! {":scan_id": scan_id, ":search": search, ":limit": limit, ":offset": sqlite_integer(offset)?},
            |row| {
                Ok(FileRecord {
                    path: decode_path(row, 0)?,
                    logical_bytes: unsigned_integer(row, 2)?,
                    created: datetime(row, 3)?,
                    modified: datetime(row, 5)?,
                    read_only: row.get(7)?,
                })
            },
        )?.collect()
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
        self.transaction.execute(
            "UPDATE scans SET finished_unix_ms = ?1, status = ?2, files = ?3, directories = ?4,
                              logical_bytes = ?5, large_files = ?6, skipped = ?7, errors = ?8,
                              first_error = ?9
             WHERE id = ?10",
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
