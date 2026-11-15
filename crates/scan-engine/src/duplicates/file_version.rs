use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::FileRecord;
use crate::is_link;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FileVersion {
    pub(super) identity: platform::Identity,
    pub(super) len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    changed: platform::ChangeTime,
}

impl FileVersion {
    /// Pin the final component without following links and confirm that it is
    /// the same file visible at the path before any content is read.
    pub(super) fn open(path: &Path) -> io::Result<(File, Self)> {
        let before = fs::symlink_metadata(path)?;
        regular_file(&before)?;
        let file = platform::open(path)?;
        let version = Self::from_file(&file)?;
        platform::check_before(&before, &version)?;
        version.check_path(path)?;
        Ok((file, version))
    }

    pub(super) fn check_record(self, record: &FileRecord) -> io::Result<()> {
        unchanged(
            self.len == record.logical_bytes
                && record
                    .modified
                    .is_none_or(|modified| self.modified == modified)
                && record
                    .created
                    .is_none_or(|created| self.created == Some(created)),
        )
    }

    pub(super) fn check(&self, path: &Path, file: &File) -> io::Result<()> {
        unchanged(*self == Self::from_file(file)?)?;
        self.check_path(path)
    }

    pub(super) fn check_path(&self, path: &Path) -> io::Result<()> {
        unchanged(*self == platform::at_path(path)?)
    }

    fn from_file(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        regular_file(&metadata)?;
        let (identity, changed) = platform::file_identity(file, &metadata)?;
        Self::from_metadata(&metadata, identity, changed)
    }

    fn from_metadata(
        metadata: &Metadata,
        identity: platform::Identity,
        changed: platform::ChangeTime,
    ) -> io::Result<Self> {
        Ok(Self {
            identity,
            len: metadata.len(),
            modified: metadata.modified()?,
            created: metadata.created().ok(),
            changed,
        })
    }
}

fn regular_file(metadata: &Metadata) -> io::Result<()> {
    if metadata.is_file() && !is_link(metadata) {
        Ok(())
    } else {
        Err(io::Error::other(
            "expected a regular file without symbolic links or reparse points",
        ))
    }
}

fn unchanged(matches: bool) -> io::Result<()> {
    if matches {
        Ok(())
    } else {
        Err(io::Error::other(
            "file changed since it was scanned or read",
        ))
    }
}

#[cfg(unix)]
mod platform {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    use super::*;

    pub(super) type Identity = (u64, u64);
    pub(super) type ChangeTime = (i64, i64);

    pub(super) fn open(path: &Path) -> io::Result<File> {
        // Nonblocking also prevents hanging if a FIFO replaces the file
        // between symlink_metadata and open.
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }

    pub(super) fn file_identity(
        _: &File,
        metadata: &Metadata,
    ) -> io::Result<(Identity, ChangeTime)> {
        Ok((identity(metadata), change_time(metadata)))
    }

    pub(super) fn check_before(metadata: &Metadata, version: &FileVersion) -> io::Result<()> {
        unchanged(*version == from_metadata(metadata)?)
    }

    pub(super) fn at_path(path: &Path) -> io::Result<FileVersion> {
        let metadata = fs::symlink_metadata(path)?;
        regular_file(&metadata)?;
        from_metadata(&metadata)
    }

    fn from_metadata(metadata: &Metadata) -> io::Result<FileVersion> {
        FileVersion::from_metadata(metadata, identity(metadata), change_time(metadata))
    }

    fn identity(metadata: &Metadata) -> Identity {
        (metadata.dev(), metadata.ino())
    }

    fn change_time(metadata: &Metadata) -> ChangeTime {
        (metadata.ctime(), metadata.ctime_nsec())
    }
}

#[cfg(windows)]
mod platform {
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS,
        FileBasicInfo, FileIdInfo, GetFileInformationByHandleEx,
    };

    use super::*;

    pub(super) type Identity = (u64, [u8; 16]);
    pub(super) type ChangeTime = i64;

    pub(super) fn open(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    pub(super) fn file_identity(file: &File, _: &Metadata) -> io::Result<(Identity, ChangeTime)> {
        // Each class is paired with the corresponding Win32 structure.
        let identity = unsafe { information::<FILE_ID_INFO>(file, FileIdInfo) }?;
        let basic = unsafe { information::<FILE_BASIC_INFO>(file, FileBasicInfo) }?;
        Ok((
            (identity.VolumeSerialNumber, identity.FileId.Identifier),
            basic.ChangeTime,
        ))
    }

    pub(super) fn check_before(metadata: &Metadata, version: &FileVersion) -> io::Result<()> {
        unchanged(
            version.len == metadata.len()
                && version.modified == metadata.modified()?
                && version.created == metadata.created().ok(),
        )
    }

    pub(super) fn at_path(path: &Path) -> io::Result<FileVersion> {
        FileVersion::from_file(&open(path)?)
    }

    // T must be the initialized output structure for this information class.
    unsafe fn information<T>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
        let mut value = MaybeUninit::<T>::uninit();
        let success = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                class,
                value.as_mut_ptr().cast(),
                size_of::<T>() as u32,
            )
        };
        if success == 0 {
            Err(io::Error::last_os_error())
        } else {
            // A successful query initialized the corresponding Win32 structure.
            Ok(unsafe { value.assume_init() })
        }
    }
}
