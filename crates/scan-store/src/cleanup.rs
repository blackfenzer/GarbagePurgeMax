use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cleanup_engine::{Snapshot, TrashReceipt};
use rusqlite::{OptionalExtension, Row, TransactionBehavior, named_params, params};
use scan_engine::FileRecord;

use crate::{ScanStore, decode_path, encode_path, file_record, sqlite_integer, unix_millis};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    Queued,
    Removed,
    Moving,
    Trashed,
    Failed,
    Restoring,
    Restored,
    Recovered,
    Attention,
}

impl CleanupState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Removed => "Removed from queue",
            Self::Moving => "Moving to Trash",
            Self::Trashed => "In Trash / Recycle Bin",
            Self::Failed => "Validation failed; source untouched",
            Self::Restoring => "Restoring",
            Self::Restored => "Restored",
            Self::Recovered => "Original verified in place",
            Self::Attention => "Needs inspection",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Removed => "removed",
            Self::Moving => "moving",
            Self::Trashed => "trashed",
            Self::Failed => "failed",
            Self::Restoring => "restoring",
            Self::Restored => "restored",
            Self::Recovered => "recovered",
            Self::Attention => "attention",
        }
    }
    fn parse(key: &str) -> io::Result<Self> {
        Ok(match key {
            "queued" => Self::Queued,
            "removed" => Self::Removed,
            "moving" => Self::Moving,
            "trashed" => Self::Trashed,
            "failed" => Self::Failed,
            "restoring" => Self::Restoring,
            "restored" => Self::Restored,
            "recovered" => Self::Recovered,
            "attention" => Self::Attention,
            _ => return Err(io::Error::other("Invalid cleanup journal state")),
        })
    }
}

#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub scan_id: i64,
    pub source: PathBuf,
    pub proof: Snapshot,
    pub keeper: Option<(PathBuf, Snapshot)>,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct CleanupItem {
    pub id: i64,
    pub entry: QueueEntry,
    pub state: CleanupState,
    pub receipt: Option<TrashReceipt>,
    pub message: String,
    pub created_unix_ms: i64,
    pub available_before: Option<u64>,
    pub available_after: Option<u64>,
}

/// The OS releases this exclusive advisory lock on process exit, unlike a persisted busy flag.
pub struct CleanupSession<'a> {
    store: &'a mut ScanStore,
    _lock: File,
}

impl ScanStore {
    pub fn cleanup_items(
        &self,
        queued: bool,
        limit: u32,
        offset: u64,
    ) -> io::Result<Vec<CleanupItem>> {
        let mut query = self
            .0
            .prepare(include_str!("../sql/list_cleanup.sql"))
            .map_err(io::Error::other)?;
        let mut rows = query.query(named_params! {":queued": queued, ":limit": limit.min(200), ":offset": sqlite_integer(offset).map_err(io::Error::other)?}).map_err(io::Error::other)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().map_err(io::Error::other)? {
            items.push(read_item(row)?);
        }
        Ok(items)
    }

    pub fn cleanup_session(&mut self) -> io::Result<CleanupSession<'_>> {
        let parent = self
            .database_path()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("Cleanup requires an on-disk journal"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join("cleanup.lock"))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "Another cleanup or restore operation is active",
            ),
            std::fs::TryLockError::Error(error) => error,
        })?;
        // FULL synchronizes intent/receipts before another filesystem action is allowed.
        self.0
            .pragma_update(None, "synchronous", "FULL")
            .map_err(io::Error::other)?;
        self.0
            .execute(
                include_str!("../sql/cleanup_recover.sql"),
                [unix_millis(SystemTime::now()).map_err(io::Error::other)?],
            )
            .map_err(io::Error::other)?;
        Ok(CleanupSession {
            store: self,
            _lock: lock,
        })
    }

    pub fn cleanup_file(&self, scan_id: i64, path: &Path) -> rusqlite::Result<Option<FileRecord>> {
        self.0
            .query_row(
                include_str!("../sql/cleanup_record.sql"),
                params![scan_id, encode_path(path)],
                file_record,
            )
            .optional()
    }

    pub fn cleanup_empty_directory(&self, scan_id: i64, path: &Path) -> rusqlite::Result<bool> {
        self.0.query_row(
            include_str!("../sql/cleanup_empty_directory.sql"),
            params![scan_id, encode_path(path)],
            |row| row.get(0),
        )
    }

    pub fn cleanup_duplicate(
        &self,
        scan_id: i64,
        path: &Path,
    ) -> rusqlite::Result<Option<(u64, Vec<u8>)>> {
        self.0
            .query_row(
                include_str!("../sql/cleanup_duplicate.sql"),
                params![scan_id, encode_path(path)],
                |row| Ok((crate::unsigned_integer(row, 0)?, row.get(1)?)),
            )
            .optional()
    }
}

impl CleanupSession<'_> {
    pub fn enqueue(&mut self, entry: &QueueEntry) -> io::Result<i64> {
        let transaction = self
            .store
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        let count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM cleanup_items WHERE state = 'queued'",
                [],
                |row| row.get(0),
            )
            .map_err(io::Error::other)?;
        if count >= 200 {
            return Err(io::Error::other(
                "The cleanup queue is limited to 200 reviewed items",
            ));
        }
        let source = encode_path(&entry.source);
        let keeper = entry.keeper.as_ref().map(|(path, _)| encode_path(path));
        let keeper_identity = entry.keeper.as_ref().map(|(_, proof)| &proof.identity);
        let conflict: bool = transaction
            .query_row(
                include_str!("../sql/cleanup_conflicts.sql"),
                params![source, entry.proof.identity, keeper, keeper_identity],
                |row| row.get(0),
            )
            .map_err(io::Error::other)?;
        if conflict {
            return Err(io::Error::other(
                "This item is already active or is a kept copy for another cleanup item",
            ));
        }
        let now = unix_millis(SystemTime::now()).map_err(io::Error::other)?;
        transaction
            .execute(
                include_str!("../sql/cleanup_enqueue.sql"),
                params![
                    entry.scan_id,
                    source,
                    entry.proof.identity,
                    serde_json::to_string(&entry.proof)?,
                    keeper,
                    keeper_identity,
                    entry
                        .keeper
                        .as_ref()
                        .map(|(_, proof)| serde_json::to_string(proof))
                        .transpose()?,
                    entry.reason,
                    now
                ],
            )
            .map_err(io::Error::other)?;
        let id = transaction.last_insert_rowid();
        transaction.commit().map_err(io::Error::other)?;
        Ok(id)
    }

    pub fn remove(&mut self, id: i64) -> io::Result<()> {
        self.transition(
            id,
            CleanupState::Queued,
            CleanupState::Removed,
            "Removed without changing the source",
        )
    }

    /// Atomically claims exactly the reviewed queue, never a newly added or changed selection.
    pub fn claim(&mut self, ids: &[i64]) -> io::Result<Vec<CleanupItem>> {
        let items = self.store.cleanup_items(true, 200, 0)?;
        let expected: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        let current: std::collections::BTreeSet<_> = items.iter().map(|item| item.id).collect();
        if expected != current || expected.len() != ids.len() || ids.is_empty() {
            return Err(io::Error::other(
                "The queue changed; review and confirm it again",
            ));
        }
        self.mark_moving(ids)?;
        Ok(items)
    }

    /// Claims one item confirmed on its own, such as a file moved to Trash from Files. The
    /// rest of the queue stays queued for its own confirmation.
    pub fn claim_one(&mut self, id: i64) -> io::Result<CleanupItem> {
        let item = self
            .store
            .cleanup_items(true, 200, 0)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| io::Error::other("The confirmed item is no longer queued"))?;
        self.mark_moving(&[id])?;
        Ok(item)
    }

    fn mark_moving(&mut self, ids: &[i64]) -> io::Result<()> {
        let transaction = self
            .store
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io::Error::other)?;
        for id in ids {
            let changed = transaction
                .execute(
                    include_str!("../sql/cleanup_claim.sql"),
                    params![
                        id,
                        unix_millis(SystemTime::now()).map_err(io::Error::other)?
                    ],
                )
                .map_err(io::Error::other)?;
            if changed != 1 {
                return Err(io::Error::other("Queue state changed"));
            }
        }
        transaction.commit().map_err(io::Error::other)?;
        Ok(())
    }

    pub fn transition(
        &mut self,
        id: i64,
        from: CleanupState,
        to: CleanupState,
        message: &str,
    ) -> io::Result<()> {
        use CleanupState::*;
        if !matches!(
            (from, to),
            (Queued, Removed)
                | (Moving, Queued | Failed | Attention | Trashed)
                | (Trashed | Attention, Restoring)
                | (Restoring, Restored | Attention)
                | (Attention | Trashed, Recovered)
        ) {
            return Err(io::Error::other("Invalid cleanup state transition"));
        }
        let changed = self
            .store
            .0
            .execute(
                include_str!("../sql/cleanup_transition.sql"),
                params![
                    id,
                    from.key(),
                    to.key(),
                    message,
                    unix_millis(SystemTime::now()).map_err(io::Error::other)?
                ],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "Cleanup state changed in another operation",
            ));
        }
        Ok(())
    }

    /// Persist the native receipt before verification so later failures still have a recovery path.
    pub fn receipt(&mut self, id: i64, receipt: &TrashReceipt) -> io::Result<()> {
        let changed = self
            .store
            .0
            .execute(
                include_str!("../sql/cleanup_receipt.sql"),
                params![
                    id,
                    encode_path(&receipt.identifier),
                    encode_path(&receipt.path)
                ],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other("Could not save the native Trash receipt"));
        }
        Ok(())
    }

    pub fn space_observation(
        &mut self,
        id: i64,
        before: Option<u64>,
        after: Option<u64>,
    ) -> io::Result<()> {
        self.store
            .0
            .execute(
                include_str!("../sql/cleanup_space.sql"),
                params![
                    id,
                    before.and_then(|n| i64::try_from(n).ok()),
                    after.and_then(|n| i64::try_from(n).ok())
                ],
            )
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub fn restore_item(&self, id: i64) -> io::Result<CleanupItem> {
        let mut statement = self
            .store
            .0
            .prepare(include_str!("../sql/cleanup_item.sql"))
            .map_err(io::Error::other)?;
        let mut rows = statement.query([id]).map_err(io::Error::other)?;
        read_item(
            rows.next()
                .map_err(io::Error::other)?
                .ok_or_else(|| io::Error::other("Recovery item not found"))?,
        )
    }
}

fn read_item(row: &Row<'_>) -> io::Result<CleanupItem> {
    (|| -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
        let keeper = row
            .get::<_, Option<Vec<u8>>>(4)?
            .map(|_| {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>((
                    decode_path(row, 4)?,
                    serde_json::from_str(&row.get::<_, String>(5)?)?,
                ))
            })
            .transpose()?;
        let receipt = row
            .get::<_, Option<Vec<u8>>>(8)?
            .map(|_| {
                Ok::<_, rusqlite::Error>(TrashReceipt {
                    identifier: decode_path(row, 8)?,
                    path: decode_path(row, 9)?,
                })
            })
            .transpose()?;
        Ok(CleanupItem {
            id: row.get(0)?,
            entry: QueueEntry {
                scan_id: row.get(1)?,
                source: decode_path(row, 2)?,
                proof: serde_json::from_str(&row.get::<_, String>(3)?)?,
                keeper,
                reason: row.get(6)?,
            },
            state: CleanupState::parse(&row.get::<_, String>(7)?)?,
            receipt,
            message: row.get(10)?,
            created_unix_ms: row.get(11)?,
            available_before: row
                .get::<_, Option<i64>>(12)?
                .map(u64::try_from)
                .transpose()?,
            available_after: row
                .get::<_, Option<i64>>(13)?
                .map(u64::try_from)
                .transpose()?,
        })
    })()
    .map_err(io::Error::other)
}
