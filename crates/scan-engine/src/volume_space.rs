use std::io;
use std::path::Path;

/// OS-reported space for the volume containing a directory, not that directory's size.
/// Windows capacity can be limited by the caller's quota. Neither value estimates
/// reclaimable space, and shared volumes may change immediately after the query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeSpace {
    pub capacity_bytes: u64,
    pub available_bytes: u64,
}

impl VolumeSpace {
    fn from_bytes(capacity_bytes: u64, available_bytes: u64) -> io::Result<Self> {
        if capacity_bytes == 0 || available_bytes > capacity_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The filesystem did not report usable capacity and available space",
            ));
        }
        Ok(Self {
            capacity_bytes,
            available_bytes,
        })
    }
}

/// Query read-only filesystem statistics. Call off the UI thread: mounted or
/// network filesystems can block. The final component must be an ordinary directory.
pub fn volume_space(root: &Path) -> io::Result<VolumeSpace> {
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir() || crate::is_link(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Volume statistics require a directory without a link or reparse point",
        ));
    }
    platform::query(root)
}

#[cfg(unix)]
mod platform {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    #[cfg(target_os = "macos")]
    pub(super) fn query(root: &Path) -> io::Result<VolumeSpace> {
        let path = CString::new(root.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        // Darwin statvfs exposes 32-bit block counts; statfs retains large-volume sizes.
        let mut stats = MaybeUninit::<libc::statfs>::uninit();
        // The path is NUL-terminated and stats points to a writable statfs structure.
        if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // A successful statfs call initializes the output structure.
        let stats = unsafe { stats.assume_init() };
        from_blocks(stats.f_blocks, stats.f_bavail, u64::from(stats.f_bsize))
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn query(root: &Path) -> io::Result<VolumeSpace> {
        let path = CString::new(root.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut stats = MaybeUninit::<libc::statvfs>::uninit();
        // The path is NUL-terminated and stats points to a writable statvfs structure.
        if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // A successful statvfs call initializes the output structure.
        let stats = unsafe { stats.assume_init() };
        // Native counter widths differ among Unix platforms.
        #[allow(clippy::unnecessary_cast)]
        from_blocks(
            stats.f_blocks as u64,
            stats.f_bavail as u64,
            stats.f_frsize as u64,
        )
    }

    fn from_blocks(
        capacity: u64,
        available: u64,
        allocation_bytes: u64,
    ) -> io::Result<VolumeSpace> {
        let bytes = |blocks: u64| {
            blocks.checked_mul(allocation_bytes).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Filesystem byte count overflow")
            })
        };
        VolumeSpace::from_bytes(bytes(capacity)?, bytes(available)?)
    }

    #[cfg(test)]
    #[test]
    fn converts_allocation_blocks_without_overflow() -> io::Result<()> {
        assert_eq!(
            from_blocks(10, 3, 4096)?,
            VolumeSpace {
                capacity_bytes: 40960,
                available_bytes: 12288
            }
        );
        assert!(from_blocks(u64::MAX, 1, 4096).is_err());
        assert!(from_blocks(10, u64::MAX, 4096).is_err());
        assert!(from_blocks(10, 3, 0).is_err());
        assert_eq!(
            from_blocks(1_u64 << 34, 1_u64 << 33, 4096)?.capacity_bytes,
            1_u64 << 46
        );
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    use super::*;

    pub(super) fn query(root: &Path) -> io::Result<VolumeSpace> {
        let path = directory_name(root)?;
        let mut capacity = 0;
        let mut available = 0;
        // The path is terminated UTF-16; both output pointers refer to writable u64s.
        // Total free space is deliberately omitted: quotas may restrict the caller.
        let success = unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                &mut capacity,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        VolumeSpace::from_bytes(capacity, available)
    }

    fn directory_name(root: &Path) -> io::Result<Vec<u16>> {
        let mut path: Vec<_> = root.as_os_str().encode_wide().collect();
        if path.is_empty() || path.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid volume path",
            ));
        }
        // GetDiskFreeSpaceExW requires a trailing backslash for UNC share names.
        if path.last() == Some(&47) {
            *path.last_mut().expect("nonempty directory name") = 92;
        } else if path.last() != Some(&92) {
            path.push(92);
        }
        path.push(0);
        Ok(path)
    }

    #[cfg(test)]
    #[test]
    fn preserves_native_paths_and_terminates_unc_directories() -> io::Result<()> {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        for text in [
            r"C:\folder",
            r"\\server\share",
            r"\\?\C:\folder",
            "C:\\โฟลเดอร์",
        ] {
            let expected: Vec<_> = format!("{text}\\\0").encode_utf16().collect();
            assert_eq!(directory_name(Path::new(text))?, expected);
        }
        let native = OsString::from_wide(&[67, 58, 92, 0xD800]);
        for text in [r"\\server\share\", r"\\server\share/"] {
            let expected: Vec<_> = "\\\\server\\share\\\0".encode_utf16().collect();
            assert_eq!(directory_name(Path::new(text))?, expected);
        }
        assert_eq!(
            directory_name(Path::new(&native))?,
            [67, 58, 92, 0xD800, 92, 0]
        );
        assert!(directory_name(Path::new("bad\0path")).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_or_inconsistent_space_but_accepts_full_volumes() -> io::Result<()> {
        assert!(VolumeSpace::from_bytes(0, 0).is_err());
        assert!(VolumeSpace::from_bytes(100, 101).is_err());
        assert_eq!(VolumeSpace::from_bytes(100, 0)?.available_bytes, 0);
        assert_eq!(
            VolumeSpace::from_bytes(u64::MAX, u64::MAX)?.capacity_bytes,
            u64::MAX
        );
        Ok(())
    }

    #[test]
    fn queries_a_real_directory_and_rejects_missing_or_file_paths() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("ข้อมูล");
        std::fs::create_dir(&directory)?;
        let space = volume_space(&directory)?;
        assert!(space.capacity_bytes > 0);
        assert!(space.available_bytes <= space.capacity_bytes);
        assert!(volume_space(&root.path().join("missing")).is_err());
        let file = root.path().join("file");
        std::fs::write(&file, b"unchanged")?;
        assert_eq!(
            volume_space(&file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(std::fs::read(file)?, b"unchanged");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symbolic_link_roots() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let link = root.path().join("alias");
        std::os::unix::fs::symlink(root.path(), &link)?;
        assert_eq!(
            volume_space(&link).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        Ok(())
    }
}
