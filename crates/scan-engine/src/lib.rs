use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use walkdir::WalkDir;

mod duplicates;
mod file_types;
mod similar_images;

pub use duplicates::{DuplicateAnalysis, DuplicateGroup, verify_duplicate_group};
pub use file_types::{OTHER_FILE_TYPE, file_type};
pub use similar_images::{
    SIMILAR_IMAGE_DISTANCE, SimilarImage, SimilarImageAnalysis, SimilarImageGroup, difference_hash,
    find_similar_images, is_hashable_image,
};

#[derive(Debug)]
pub struct FileRecord {
    pub path: PathBuf,
    pub logical_bytes: u64,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files: u64,
    pub directories: u64,
    pub logical_bytes: u64,
    pub skipped: u64,
    pub errors: u64,
}

#[derive(Debug, Default)]
pub struct ScanSummary {
    pub progress: ScanProgress,
    pub cancelled: bool,
    pub first_error: Option<String>,
}

/// Streams regular-file metadata from a directory. Exclusions are relative to `root`.
/// Logical bytes are counted per path, including separate names for hard-linked files.
/// The visitor runs on the caller's thread. Cancellation is checked between entries.
pub fn scan_directory(
    root: &Path,
    exclusions: &[PathBuf],
    cancelled: &AtomicBool,
    mut visit: impl FnMut(ScanProgress, Option<FileRecord>),
) -> io::Result<ScanSummary> {
    let mut summary = ScanSummary::default();
    if cancelled.load(Ordering::Relaxed) {
        summary.cancelled = true;
        return Ok(summary);
    }

    // Strip a trailing separator or `/.` so symlink_metadata checks the root itself.
    let root: PathBuf = root.components().collect();
    let root_metadata = fs::symlink_metadata(&root)?;
    if !root_metadata.is_dir() || is_link(&root_metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a directory that is not a symbolic link or reparse point",
        ));
    }

    let mut entries = WalkDir::new(&root)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }

        let Some(entry) = entries.next() else { break };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.depth() == 0 => return Err(error.into()),
            Err(error) => {
                summary.record_error(error.to_string());
                visit(summary.progress, None);
                continue;
            }
        };
        let path = entry.path();
        let directory = entry.file_type().is_dir();
        if entry.depth() != 0
            && path
                .strip_prefix(&root)
                .is_ok_and(|relative| exclusions.iter().any(|excluded| excluded == relative))
        {
            if directory {
                entries.skip_current_dir();
            }
            summary.progress.skipped += 1;
            visit(summary.progress, None);
            continue;
        }

        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if entry.depth() == 0 => return Err(error),
            Err(error) => {
                if directory {
                    entries.skip_current_dir();
                }
                summary.record_error(format!("{}: {error}", path.display()));
                visit(summary.progress, None);
                continue;
            }
        };

        if is_link(&metadata) || metadata.is_dir() != directory {
            if directory {
                entries.skip_current_dir();
            }
            summary.progress.skipped += 1;
            visit(summary.progress, None);
        } else if directory {
            summary.progress.directories += 1;
            visit(summary.progress, None);
        } else if metadata.is_file() {
            let record = FileRecord {
                path: entry.into_path(),
                logical_bytes: metadata.len(),
                created: metadata.created().ok(),
                modified: metadata.modified().ok(),
                read_only: metadata.permissions().readonly(),
            };
            summary.progress.files += 1;
            summary.progress.logical_bytes = summary
                .progress
                .logical_bytes
                .saturating_add(record.logical_bytes);
            visit(summary.progress, Some(record));
        } else {
            summary.progress.skipped += 1;
            visit(summary.progress, None);
        }
    }
    Ok(summary)
}

impl ScanSummary {
    fn record_error(&mut self, message: String) {
        self.progress.errors += 1;
        self.first_error.get_or_insert(message);
    }
}

#[cfg(windows)]
fn is_link(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &Metadata) -> bool {
    metadata.is_symlink()
}
