use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSFileManager, NSURL};

use crate::{NativeTrash, TrashBackend, TrashReceipt};

impl TrashBackend for NativeTrash {
    fn trash(&self, path: &Path) -> io::Result<TrashReceipt> {
        autoreleasepool(|_| {
            let name = CString::new(path.as_os_str().as_bytes())?;
            // Foundation reads the NUL-terminated native path during this call.
            let url = unsafe {
                NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                    NonNull::new(name.as_ptr().cast_mut()).unwrap(),
                    path.is_dir(),
                    None,
                )
            };
            let mut result = None;
            NSFileManager::defaultManager()
                .trashItemAtURL_resultingItemURL_error(&url, Some(&mut result))
                .map_err(|error| io::Error::other(error.to_string()))?;
            let result = result.ok_or_else(|| {
                io::Error::other(
                    "Moved to Trash but no recovery URL was returned; restore using Finder",
                )
            })?;
            // The URL owns this representation; copy it before the autorelease pool ends.
            let bytes = unsafe { CStr::from_ptr(result.fileSystemRepresentation().as_ptr()) };
            let path = Path::new(OsStr::from_bytes(bytes.to_bytes())).to_path_buf();
            Ok(TrashReceipt {
                identifier: path.clone(),
                path,
            })
        })
    }

    fn restore(&self, receipt: &TrashReceipt, destination: &Path) -> io::Result<()> {
        let source = CString::new(receipt.path.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        // RENAME_EXCL makes the no-overwrite check atomic, including an intervening new file.
        if unsafe {
            libc::renameatx_np(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
