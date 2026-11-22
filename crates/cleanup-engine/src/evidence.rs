use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
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
    if !path.is_absolute() || has_traversal(path) {
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

fn has_traversal(path: &Path) -> bool {
    let parsed = path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir));
    // Verbatim paths (`\\?\C:\…`, as `canonicalize` returns on Windows) keep `..` and `/`
    // inside ordinary components, so check each segment of the raw text as well.
    #[cfg(windows)]
    let parsed = parsed
        || path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| matches!(byte, b'/' | b'\\'))
            .any(|segment| segment == b".." || segment == b".");
    parsed
}

/// What a non-empty folder held when it was inspected: shown before it moves, and fixed by
/// the snapshot's digest so the move is refused if anything inside changes first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderContents {
    pub files: u64,
    pub folders: u64,
    /// Links and reparse points inside; they move as links, never followed.
    pub links: u64,
    pub read_only: u64,
    pub logical_bytes: u64,
    /// The largest files by path relative to the folder, largest first.
    pub largest: Vec<(PathBuf, u64)>,
}

const LARGEST_SHOWN: usize = 8;

pub fn inspect(path: &Path, cancel: &AtomicBool) -> io::Result<Snapshot> {
    inspect_contents(path, cancel).map(|(snapshot, _)| snapshot)
}

/// Like `inspect`, also listing a non-empty folder's contents for confirmation.
pub fn inspect_contents(
    path: &Path,
    cancel: &AtomicBool,
) -> io::Result<(Snapshot, Option<FolderContents>)> {
    validate_ancestors(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if assess_risk(path, metadata.permissions().readonly()).tier == RiskTier::Protected {
        return Err(io::Error::other(
            "Protected locations and read-only items cannot be queued for cleanup",
        ));
    }
    capture_contents(path, cancel)
}

pub(crate) fn capture(path: &Path, cancel: &AtomicBool) -> io::Result<Snapshot> {
    capture_contents(path, cancel).map(|(snapshot, _)| snapshot)
}

fn capture_contents(
    path: &Path,
    cancel: &AtomicBool,
) -> io::Result<(Snapshot, Option<FolderContents>)> {
    crate::cancelled(cancel)?;
    let before = fs::symlink_metadata(path)?;
    if is_link(&before) || !(before.is_file() || before.is_dir()) {
        return Err(io::Error::other(
            "Only regular files and folders are supported, not links",
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
    let mut contents = None;
    let sha256 = if metadata.is_dir() {
        // An empty folder has no digest, so one that gains entries no longer matches its proof.
        if fs::read_dir(path)?.next().transpose()?.is_none() {
            None
        } else {
            let (digest, listed) = walk_folder(path, cancel)?;
            contents = Some(listed);
            Some(digest)
        }
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
    let snapshot = Snapshot {
        identity,
        version,
        sha256,
        logical_bytes: match &contents {
            Some(contents) => contents.logical_bytes,
            None if metadata.is_dir() => 0,
            None => metadata.len(),
        },
        // A folder's own blocks say nothing about the space its contents hold.
        allocated_bytes: if contents.is_some() { None } else { allocated },
        directory: metadata.is_dir(),
    };
    Ok((snapshot, contents))
}

/// Walks a folder without following links. The digest covers every entry's relative path,
/// kind, size, modified time and (on Unix) inode, so any addition, removal, edit or swap
/// inside changes it. Moving the folder keeps all of these, so the Trash copy still matches.
fn walk_folder(root: &Path, cancel: &AtomicBool) -> io::Result<([u8; 32], FolderContents)> {
    let mut hash = Sha256::new();
    let mut contents = FolderContents::default();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let mut names = fs::read_dir(root.join(&relative))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        for name in names {
            crate::cancelled(cancel)?;
            let path = relative.join(name);
            let metadata = fs::symlink_metadata(root.join(&path))?;
            reject_placeholder(&metadata)?;
            let kind = if is_link(&metadata) {
                contents.links += 1;
                b'l'
            } else if metadata.is_dir() {
                contents.folders += 1;
                pending.push(path.clone());
                b'd'
            } else {
                contents.files += 1;
                contents.logical_bytes = contents.logical_bytes.saturating_add(metadata.len());
                b'f'
            };
            if metadata.permissions().readonly() {
                contents.read_only += 1;
            }
            let mut record = path.as_os_str().as_encoded_bytes().to_vec();
            record.extend_from_slice(&[0, kind]);
            record.extend_from_slice(&metadata.len().to_le_bytes());
            append_time(&mut record, metadata.modified()?);
            #[cfg(unix)]
            record.extend_from_slice(&std::os::unix::fs::MetadataExt::ino(&metadata).to_le_bytes());
            hash.update(&record);
            if kind == b'f' {
                let largest = &mut contents.largest;
                if largest.len() < LARGEST_SHOWN
                    || largest
                        .last()
                        .is_some_and(|(_, bytes)| *bytes < metadata.len())
                {
                    largest.push((path, metadata.len()));
                    largest.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
                    largest.truncate(LARGEST_SHOWN);
                }
            }
        }
    }
    Ok((hash.finalize().into(), contents))
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
