use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use cleanup_engine::{TrashBackend, inspect, restore_checked, revalidate, verify_receipt};
use scan_engine::{cleanup_candidate, volume_space};
use scan_store::{CleanupState, QueueEntry, SavedScan, ScanStore};

pub(super) fn prepare(
    store: &ScanStore,
    scan: &SavedScan,
    source: &Path,
    keeper: Option<&Path>,
    cancel: &AtomicBool,
) -> io::Result<QueueEntry> {
    if let Some(database) = store.database_path() {
        super::super::exclusions::check_cleanup(source, database)?;
    }
    let root = scan.root.canonicalize()?;
    cleanup_engine::validate_ancestors(source)?;
    if !source.starts_with(&root) || source == root {
        return Err(io::Error::other(
            "Choose an indexed item inside the scan root, not the root itself",
        ));
    }
    if let Some(data) = store.database_path().and_then(Path::parent)
        && source.starts_with(data.canonicalize()?)
    {
        return Err(io::Error::other(
            "Storage Clinic's own data cannot be cleaned",
        ));
    }
    let record = store
        .cleanup_file(scan.id, source)
        .map_err(io::Error::other)?;
    let reason;
    if let Some(record) = &record {
        let current = fs::symlink_metadata(source)?;
        if current.len() != record.logical_bytes
            || record.modified != current.modified().ok()
            || record.created != current.created().ok()
        {
            return Err(io::Error::other(
                "The indexed metadata changed; scan this folder again",
            ));
        }
        reason = cleanup_candidate(record, scan.started_at()?).map(|candidate| candidate.evidence);
    } else if store
        .cleanup_empty_directory(scan.id, source)
        .map_err(io::Error::other)?
    {
        reason = Some(
            "Observed empty folder; emptiness will be checked again immediately before moving"
                .into(),
        );
    } else {
        return Err(io::Error::other("The item is not part of the saved scan"));
    }
    let proof = inspect(source, cancel)?;
    if proof.directory != record.is_none() {
        return Err(io::Error::other("The indexed item type changed"));
    }
    let group = store
        .cleanup_duplicate(scan.id, source)
        .map_err(io::Error::other)?;
    let (keeper, reason) = match (group, keeper) {
        (Some((group, hash)), Some(path)) => {
            let kept_group = store
                .cleanup_duplicate(scan.id, path)
                .map_err(io::Error::other)?;
            if kept_group != Some((group, hash.clone()))
                || proof.sha256.as_ref().map(|h| h.as_slice()) != Some(hash.as_slice())
            {
                return Err(io::Error::other(
                    "Choose a distinct kept copy from this exact duplicate group; changed files need a new scan",
                ));
            }
            let kept = inspect(path, cancel)?;
            revalidate(source, &proof, Some((path, &kept)), cancel)?;
            (
                Some((path.to_path_buf(), kept)),
                format!(
                    "Verified duplicate group {group}; preserve the selected copy and compare bytes again before moving"
                ),
            )
        }
        (Some(_), None) => {
            return Err(io::Error::other(
                "This file is an exact duplicate. Choose a kept copy in Exact duplicates before queueing it",
            ));
        }
        (None, Some(_)) => {
            return Err(io::Error::other(
                "The selected keeper is not supported by saved duplicate evidence",
            ));
        }
        (None, None) => (
            None,
            reason.ok_or_else(|| {
                io::Error::other(
                    "Only reviewed metadata candidates and empty folders can be queued here",
                )
            })?,
        ),
    };
    Ok(QueueEntry {
        scan_id: scan.id,
        source: source.to_path_buf(),
        proof,
        keeper,
        reason,
    })
}

#[derive(Default)]
pub(super) struct Outcome {
    pub moved: usize,
    pub failed: usize,
    pub cancelled: bool,
}

pub(super) fn execute(
    store: &mut ScanStore,
    ids: &[i64],
    backend: &impl TrashBackend,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> io::Result<Outcome> {
    let database = store.database_path().map(Path::to_path_buf);
    let mut session = store.cleanup_session()?;
    let items = session.claim(ids)?;
    let mut outcome = Outcome::default();
    for (index, item) in items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            for pending in &items[index..] {
                session.transition(
                    pending.id,
                    CleanupState::Moving,
                    CleanupState::Queued,
                    "Cancelled before this item; confirm again to run it",
                )?;
            }
            outcome.cancelled = true;
            break;
        }
        let entry = &item.entry;
        let parent = entry
            .source
            .parent()
            .ok_or_else(|| io::Error::other("Missing parent directory"))?;
        let before = volume_space(parent).ok().map(|space| space.available_bytes);
        let kept = entry
            .keeper
            .as_ref()
            .map(|(path, proof)| (path.as_path(), proof));
        let validation = database
            .as_ref()
            .map_or(Ok(()), |path| {
                super::super::exclusions::check_cleanup(&entry.source, path)
            })
            .and_then(|()| revalidate(&entry.source, &entry.proof, kept, cancel));
        if let Err(error) = validation {
            if error.kind() == io::ErrorKind::Interrupted {
                for pending in &items[index..] {
                    session.transition(
                        pending.id,
                        CleanupState::Moving,
                        CleanupState::Queued,
                        "Cancelled during validation; source untouched",
                    )?;
                }
                outcome.cancelled = true;
                break;
            }
            session.transition(
                item.id,
                CleanupState::Moving,
                CleanupState::Failed,
                &error.to_string(),
            )?;
            outcome.failed += 1;
            progress(index + 1, items.len());
            continue;
        }
        // Intent is already durable. Once native execution starts, any error may mean a partial action.
        match backend.trash(&entry.source) {
            Ok(receipt) => {
                session.receipt(item.id, &receipt)?;
                // Cancellation does not skip recording/verifying an action already performed.
                let verification = verify_receipt(&receipt, &entry.proof, &AtomicBool::new(false));
                let gone = fs::symlink_metadata(&entry.source)
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
                match verification {
                    Ok(()) if gone => {
                        session.transition(
                            item.id,
                            CleanupState::Moving,
                            CleanupState::Trashed,
                            "Recoverable in OS Trash / Recycle Bin until it is emptied",
                        )?;
                        outcome.moved += 1;
                    }
                    result => {
                        let message = result.err().map_or_else(|| "Source still exists after the native action; inspect both locations".into(), |e| e.to_string());
                        session.transition(
                            item.id,
                            CleanupState::Moving,
                            CleanupState::Attention,
                            &message,
                        )?;
                        outcome.failed += 1;
                    }
                }
            }
            Err(error) => {
                session.transition(item.id, CleanupState::Moving, CleanupState::Attention, &format!("Native action could not be confirmed: {error}. Inspect OS Trash before any further action."))?;
                outcome.failed += 1;
            }
        }
        let after = volume_space(parent).ok().map(|space| space.available_bytes);
        session.space_observation(item.id, before, after)?;
        progress(index + 1, items.len());
    }
    Ok(outcome)
}

pub(super) fn restore(
    store: &mut ScanStore,
    id: i64,
    backend: &impl TrashBackend,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let mut session = store.cleanup_session()?;
    let item = session.restore_item(id)?;
    if !matches!(item.state, CleanupState::Trashed | CleanupState::Attention) {
        return Err(io::Error::other("This history item cannot be restored"));
    }
    let receipt = item.receipt.as_ref().ok_or_else(|| {
        io::Error::other("No durable receipt: inspect and restore this item through the OS Trash")
    })?;
    // Verify before recording restore intent; a missing or changed receipt leaves history usable.
    verify_receipt(receipt, &item.entry.proof, cancel)?;
    session.transition(id, item.state, CleanupState::Restoring, "Restore requested")?;
    match restore_checked(
        backend,
        receipt,
        &item.entry.source,
        &item.entry.proof,
        cancel,
    ) {
        Ok(()) => session.transition(
            id,
            CleanupState::Restoring,
            CleanupState::Restored,
            "Original path and contents restored without overwriting another item",
        ),
        Err(error) => {
            session.transition(
                id,
                CleanupState::Restoring,
                CleanupState::Attention,
                &format!("Restore needs inspection: {error}"),
            )?;
            Err(error)
        }
    }
}

pub(super) fn reconcile(store: &mut ScanStore, id: i64, cancel: &AtomicBool) -> io::Result<()> {
    let mut session = store.cleanup_session()?;
    let item = session.restore_item(id)?;
    if !matches!(item.state, CleanupState::Attention | CleanupState::Trashed) {
        return Err(io::Error::other(
            "Only interrupted or trashed items need reconciliation",
        ));
    }
    if let Some(receipt) = &item.receipt {
        match fs::symlink_metadata(&receipt.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err(io::Error::other(
                    "The saved Trash location still exists; inspect both locations first",
                ));
            }
        }
    }
    let original = cleanup_engine::TrashReceipt {
        identifier: item.entry.source.clone(),
        path: item.entry.source.clone(),
    };
    verify_receipt(&original, &item.entry.proof, cancel)?;
    session.transition(
        id,
        item.state,
        CleanupState::Recovered,
        "Verified the original identity and contents in place; no filesystem action performed",
    )
}
