use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use crate::FileRecord;
use crate::is_link;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileVersion {
    pub(crate) identity: platform::Identity,
    pub(crate) len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    changed: platform::ChangeTime,
}

impl FileVersion {
    pub(crate) fn cache_key(self) -> Vec<u8> {
        let mut key = b"SCV1".to_vec();
        #[cfg(unix)]
        {
            key.extend_from_slice(b"unix");
            key.extend_from_slice(&self.identity.0.to_le_bytes());
            key.extend_from_slice(&self.identity.1.to_le_bytes());
            key.extend_from_slice(&self.changed.0.to_le_bytes());
            key.extend_from_slice(&self.changed.1.to_le_bytes());
        }
        #[cfg(windows)]
        {
            key.extend_from_slice(b"win");
            key.extend_from_slice(&self.identity.0.to_le_bytes());
            key.extend_from_slice(&self.identity.1);
            key.extend_from_slice(&self.changed.to_le_bytes());
        }
        key.extend_from_slice(&self.len.to_le_bytes());
        for time in [Some(self.modified), self.created] {
            key.push(u8::from(time.is_some()));
            if let Some(time) = time {
                let (before_epoch, duration) = match time.duration_since(SystemTime::UNIX_EPOCH) {
                    Ok(duration) => (false, duration),
                    Err(error) => (true, error.duration()),
                };
                key.push(u8::from(before_epoch));
                key.extend_from_slice(&duration.as_secs().to_le_bytes());
                key.extend_from_slice(&duration.subsec_nanos().to_le_bytes());
            }
        }
        key
    }

    /// Pin the final component without following links and confirm that it is
    /// the same file visible at the path before any content is read.
    pub(crate) fn open(path: &Path) -> io::Result<(File, Self)> {
        let before = fs::symlink_metadata(path)?;
        regular_file(&before)?;
        let file = platform::open(path)?;
        let version = Self::from_file(&file)?;
        platform::check_before(&before, &version)?;
        version.check_path(path)?;
        Ok((file, version))
    }

    pub(crate) fn check_record(self, record: &FileRecord) -> io::Result<()> {
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

    pub(crate) fn check(&self, path: &Path, file: &File) -> io::Result<()> {
        unchanged(*self == Self::from_file(file)?)?;
        self.check_path(path)
    }

    pub(crate) fn check_path(&self, path: &Path) -> io::Result<()> {
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

    pub(crate) type Identity = (u64, u64);
    pub(crate) type ChangeTime = (i64, i64);

    pub(crate) fn open(path: &Path) -> io::Result<File> {
        // Nonblocking also prevents hanging if a FIFO replaces the file
        // between symlink_metadata and open.
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }

    pub(crate) fn file_identity(
        _: &File,
        metadata: &Metadata,
    ) -> io::Result<(Identity, ChangeTime)> {
        Ok((identity(metadata), change_time(metadata)))
    }

    pub(crate) fn check_before(metadata: &Metadata, version: &FileVersion) -> io::Result<()> {
        unchanged(*version == from_metadata(metadata)?)
    }

    pub(crate) fn at_path(path: &Path) -> io::Result<FileVersion> {
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

    pub(crate) type Identity = (u64, [u8; 16]);
    pub(crate) type ChangeTime = i64;

    pub(crate) fn open(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    pub(crate) fn file_identity(file: &File, _: &Metadata) -> io::Result<(Identity, ChangeTime)> {
        // Each class is paired with the corresponding Win32 structure.
        let identity = unsafe { information::<FILE_ID_INFO>(file, FileIdInfo) }?;
        let basic = unsafe { information::<FILE_BASIC_INFO>(file, FileBasicInfo) }?;
        Ok((
            (identity.VolumeSerialNumber, identity.FileId.Identifier),
            basic.ChangeTime,
        ))
    }

    pub(crate) fn check_before(metadata: &Metadata, version: &FileVersion) -> io::Result<()> {
        unchanged(
            version.len == metadata.len()
                && version.modified == metadata.modified()?
                && version.created == metadata.created().ok(),
        )
    }

    pub(crate) fn at_path(path: &Path) -> io::Result<FileVersion> {
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
