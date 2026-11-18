//! Explicitly confirmed filesystem mutations, separate from read-only scanning.
mod evidence;
mod native;

pub use evidence::{Snapshot, inspect, validate_ancestors};
pub use native::NativeTrash;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Native OS receipt. The opaque identifier can differ from the filesystem path on Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashReceipt {
    pub identifier: PathBuf,
    pub path: PathBuf,
}

/// OS operations are injectable so interruption and failure tests touch disposable trees only.
pub trait TrashBackend {
    fn trash(&self, path: &Path) -> io::Result<TrashReceipt>;
    fn restore(&self, receipt: &TrashReceipt, destination: &Path) -> io::Result<()>;
}

pub fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Cancelled before the next filesystem action",
        ))
    } else {
        Ok(())
    }
}

/// Rechecks the queued version, current bytes, risk and distinct kept copy immediately before action.
pub fn revalidate(
    source: &Path,
    expected: &Snapshot,
    keeper: Option<(&Path, &Snapshot)>,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let current = inspect(source, cancel)?;
    if current != *expected {
        return Err(io::Error::other(
            "The queued item changed; remove it and review a fresh scan",
        ));
    }
    if let Some((path, proof)) = keeper {
        let kept = inspect(path, cancel)?;
        if kept != *proof
            || kept.identity == current.identity
            || kept.sha256 != current.sha256
            || kept.directory
            || current.directory
            || kept.logical_bytes != current.logical_bytes
        {
            return Err(io::Error::other(
                "The kept copy changed or is not a distinct identical file",
            ));
        }
        compare_files(source, path, cancel)?;
        evidence::check_version(path, proof)?;
    }
    evidence::check_version(source, expected)?;
    cancelled(cancel)
}

fn compare_files(source: &Path, keeper: &Path, cancel: &AtomicBool) -> io::Result<()> {
    use std::io::Read;
    let mut source = evidence::open(source, false)?;
    let mut keeper = evidence::open(keeper, false)?;
    let mut a = vec![0; 64 * 1024];
    let mut b = vec![0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = source.read(&mut a)?;
        if count == 0 {
            let mut tail = [0];
            if keeper.read(&mut tail)? == 0 {
                return Ok(());
            }
            return Err(io::Error::other("Kept copy length changed"));
        }
        keeper.read_exact(&mut b[..count])?;
        if a[..count] != b[..count] {
            return Err(io::Error::other("Files no longer have identical bytes"));
        }
    }
}

pub fn verify_receipt(
    receipt: &TrashReceipt,
    expected: &Snapshot,
    cancel: &AtomicBool,
) -> io::Result<()> {
    validate_ancestors(&receipt.path)?;
    // Trash is a protected location; proof verification must not apply source cleanup policy.
    let current = evidence::capture(&receipt.path, cancel)?;
    if !current.same_content(expected) {
        return Err(io::Error::other(
            "Recovery item identity or contents changed; inspect it in the OS Trash",
        ));
    }
    Ok(())
}

pub fn restore_checked(
    backend: &impl TrashBackend,
    receipt: &TrashReceipt,
    destination: &Path,
    expected: &Snapshot,
    cancel: &AtomicBool,
) -> io::Result<()> {
    verify_receipt(receipt, expected, cancel)?;
    validate_ancestors(destination)?;
    match std::fs::symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Restore destination already exists; nothing was overwritten",
            ));
        }
    }
    cancelled(cancel)?;
    backend.restore(receipt, destination)?;
    let restored = evidence::capture(destination, cancel)?;
    if !restored.same_content(expected) {
        return Err(io::Error::other(
            "Restore completed but its contents could not be verified; inspect the destination",
        ));
    }
    Ok(())
}
