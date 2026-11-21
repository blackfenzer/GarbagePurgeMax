use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use scan_engine::{RiskTier, assess_risk};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub identity: Vec<u8>,
    pub version: Vec<u8>,
    pub sha256: Option<[u8; 32]>,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub directory: bool,
}

impl Snapshot {
    pub fn same_content(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.sha256 == other.sha256
            && self.logical_bytes == other.logical_bytes
            && self.directory == other.directory
    }
}

/// Rejects links/reparse points in every existing ancestor, relative paths and parent traversal.
pub fn validate_ancestors(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(io::Error::other(
            "Cleanup requires an absolute path without parent traversal",
        ));
    }
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent)?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(io::Error::other(
                "A path ancestor is a link, reparse point, or not a directory",
            ));
        }
    }
    Ok(())
}

pub fn inspect(path: &Path, cancel: &AtomicBool) -> io::Result<Snapshot> {
    validate_ancestors(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if assess_risk(path, metadata.permissions().readonly()).tier == RiskTier::Protected {
        return Err(io::Error::other(
            "Protected locations and read-only items cannot be queued for cleanup",
        ));
    }
    capture(path, cancel)
}

pub(crate) fn capture(path: &Path, cancel: &AtomicBool) -> io::Result<Snapshot> {
    crate::cancelled(cancel)?;
    let before = fs::symlink_metadata(path)?;
    if is_link(&before) || !(before.is_file() || before.is_dir()) {
        return Err(io::Error::other(
            "Only regular files and observed empty directories are supported",
        ));
    }
    reject_placeholder(&before)?;
    let file = open(path, before.is_dir())?;
    let metadata = file.metadata()?;
    reject_placeholder(&metadata)?;
    let (identity, version, allocated, links) = details(&file, &metadata)?;
    if !metadata.is_dir() && links != 1 {
        return Err(io::Error::other(
            "Hard-linked files are excluded from cleanup; their physical-space ownership is ambiguous",
        ));
    }
    let sha256 = if metadata.is_dir() {
        if fs::read_dir(path)?.next().transpose()?.is_some() {
            return Err(io::Error::other("The directory is no longer empty"));
        }
        None
    } else {
        let mut reader = &file;
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            crate::cancelled(cancel)?;
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        Some(hash.finalize().into())
    };
    let after = fs::symlink_metadata(path)?;
    if is_link(&after) || metadata.modified()? != after.modified()? || metadata.len() != after.len()
    {
        return Err(io::Error::other("Item changed during validation"));
    }
    let at_path = open(path, metadata.is_dir())?;
    let (end_identity, end_version, _, _) = details(&at_path, &at_path.metadata()?)?;
    if identity != end_identity
        || version != end_version
        || details(&file, &file.metadata()?)?.1 != version
    {
        return Err(io::Error::other("Item changed during validation"));
    }
    Ok(Snapshot {
        identity,
        version,
        sha256,
        logical_bytes: if metadata.is_dir() { 0 } else { metadata.len() },
        allocated_bytes: allocated,
        directory: metadata.is_dir(),
    })
}

pub(crate) fn check_version(path: &Path, expected: &Snapshot) -> io::Result<()> {
    validate_ancestors(path)?;
    let file = open(path, expected.directory)?;
    let (identity, version, _, _) = details(&file, &file.metadata()?)?;
    if identity != expected.identity || version != expected.version {
        return Err(io::Error::other(
            "Item changed before the filesystem action",
        ));
    }
    Ok(())
}

fn reject_placeholder(metadata: &Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FILE_ATTRIBUTE_RECALL_ON_OPEN,
        };
        if metadata.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
                | FILE_ATTRIBUTE_RECALL_ON_OPEN)
            != 0
        {
            return Err(io::Error::other(
                "Cloud placeholders are excluded from cleanup",
            ));
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // Darwin SF_DATALESS: metadata exists but file contents require provider hydration.
        const SF_DATALESS: u32 = 0x40000000;
        if metadata.st_flags() & SF_DATALESS != 0 {
            return Err(io::Error::other(
                "Dataless cloud items are excluded from cleanup",
            ));
        }
    }
    let _ = metadata;
    Ok(())
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn open(path: &Path, directory: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 },
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        );
    }
    options.open(path)
}

fn append_time(bytes: &mut Vec<u8>, time: SystemTime) {
    let (negative, duration) = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(t) => (false, t),
        Err(e) => (true, e.duration()),
    };
    bytes.push(u8::from(negative));
    bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
    bytes.extend_from_slice(&duration.subsec_nanos().to_le_bytes());
}

type Details = (Vec<u8>, Vec<u8>, Option<u64>, u64);

fn details(file: &File, metadata: &Metadata) -> io::Result<Details> {
    let mut identity = Vec::new();
    let mut version = Vec::new();
    #[cfg(unix)]
    let (allocated, links) = {
        use std::os::unix::fs::MetadataExt;
        let _ = file;
        identity.extend_from_slice(&metadata.dev().to_le_bytes());
        identity.extend_from_slice(&metadata.ino().to_le_bytes());
        version.extend_from_slice(&metadata.ctime().to_le_bytes());
        version.extend_from_slice(&metadata.ctime_nsec().to_le_bytes());
        (metadata.blocks().checked_mul(512), metadata.nlink())
    };
    #[cfg(windows)]
    let (allocated, links) = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        let mut standard: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
        // Each initialized output buffer matches its information class and byte size.
        for (class, pointer, size) in [
            (
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                std::mem::size_of_val(&id),
            ),
            (
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&basic),
            ),
            (
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                std::mem::size_of_val(&standard),
            ),
        ] {
            if unsafe {
                GetFileInformationByHandleEx(file.as_raw_handle(), class, pointer, size as u32)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        if basic.FileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
                | FILE_ATTRIBUTE_RECALL_ON_OPEN)
            != 0
        {
            return Err(io::Error::other(
                "Cloud placeholders and reparse points are excluded",
            ));
        }
        identity.extend_from_slice(&id.VolumeSerialNumber.to_le_bytes());
        identity.extend_from_slice(&id.FileId.Identifier);
        version.extend_from_slice(&basic.ChangeTime.to_le_bytes());
        (
            u64::try_from(standard.AllocationSize).ok(),
            u64::from(standard.NumberOfLinks),
        )
    };
    append_time(&mut version, metadata.modified()?);
    version.extend_from_slice(&metadata.len().to_le_bytes());
    version.push(u8::from(metadata.is_dir()));
    Ok((identity, version, allocated, links))
}
