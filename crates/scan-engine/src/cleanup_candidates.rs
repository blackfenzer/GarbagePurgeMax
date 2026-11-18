use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::{FileRecord, app_cache::app_cache_origin};

/// A download untouched for this long when scanned is reported as old.
pub const OLD_DOWNLOAD_DAYS: u64 = 90;

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

/// Why a file may be worth reviewing for cleanup. Candidates are review-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CleanupKind {
    EmptyFile,
    TemporaryFile,
    IncompleteDownload,
    Installer,
    Archive,
    OldDownload,
    AppCache,
}

impl CleanupKind {
    pub const ALL: [Self; 7] = [
        Self::EmptyFile,
        Self::TemporaryFile,
        Self::IncompleteDownload,
        Self::Installer,
        Self::Archive,
        Self::OldDownload,
        Self::AppCache,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::EmptyFile => "Empty files",
            Self::TemporaryFile => "Temporary files",
            Self::IncompleteDownload => "Incomplete downloads",
            Self::Installer => "Installers",
            Self::Archive => "Archives",
            Self::OldDownload => "Old downloads",
            Self::AppCache => "App caches",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupCandidate {
    pub kind: CleanupKind,
    /// What in the file's name, size, location, or age matched the rule.
    pub evidence: String,
}

/// Applies the first matching rule, from most to least certain. Uses only indexed
/// metadata, measured against the scan's start time so the evidence stays stable.
pub fn cleanup_candidate(file: &FileRecord, scanned_at: SystemTime) -> Option<CleanupCandidate> {
    let candidate = |kind, evidence: String| Some(CleanupCandidate { kind, evidence });
    if file.logical_bytes == 0 {
        return candidate(CleanupKind::EmptyFile, "0 bytes when scanned".into());
    }
    if let Some(browser) = app_cache_origin(&file.path) {
        return candidate(
            CleanupKind::AppCache,
            format!(
                "Path matches {browser}'s default HTTP cache layout. Metadata evidence only; the browser may still use this file. Review only."
            ),
        );
    }
    let name = file.path.file_name()?.to_str()?;
    if let Some(origin) = temporary_file_origin(name) {
        return candidate(CleanupKind::TemporaryFile, format!("Named like {origin}"));
    }
    let extension = file
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let evidence = |what| format!("The .{extension} extension marks {what}");
    match extension.as_str() {
        "crdownload" | "part" | "partial" | "opdownload" => {
            return candidate(
                CleanupKind::IncompleteDownload,
                evidence("an unfinished browser download"),
            );
        }
        "dmg" | "pkg" | "mpkg" | "msi" | "msix" | "appx" => {
            return candidate(
                CleanupKind::Installer,
                evidence("an installer or disk image"),
            );
        }
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" => {
            return candidate(
                CleanupKind::Archive,
                evidence("an archive. Its contents may already be extracted"),
            );
        }
        _ => {}
    }
    let age_days = scanned_at
        .duration_since(file.modified?)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        / SECONDS_PER_DAY;
    if age_days >= OLD_DOWNLOAD_DAYS && in_downloads_folder(&file.path) {
        return candidate(
            CleanupKind::OldDownload,
            format!("In a Downloads folder and unmodified for {age_days} days when scanned"),
        );
    }
    None
}

fn temporary_file_origin(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        ".ds_store" => "macOS Finder folder settings",
        "thumbs.db" | "ehthumbs.db" => "a Windows thumbnail cache",
        _ if name.starts_with("~$") => "an Office lock file",
        _ if name.starts_with(".~lock.") => "a LibreOffice lock file",
        _ if lower.ends_with(".tmp") || lower.ends_with(".temp") => "a temporary file",
        _ if lower.ends_with(".swp") && name.starts_with('.') => "a Vim swap file",
        _ => return None,
    })
}

fn in_downloads_folder(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("Downloads"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(path: &str, logical_bytes: u64, age_days: u64, scanned_at: SystemTime) -> FileRecord {
        FileRecord {
            path: PathBuf::from(path),
            logical_bytes,
            created: None,
            modified: Some(scanned_at - Duration::from_secs(age_days * SECONDS_PER_DAY)),
            read_only: false,
        }
    }

    #[test]
    fn applies_rules_in_order_with_evidence() {
        let now = SystemTime::now();
        let kind = |path, bytes, age| {
            cleanup_candidate(&file(path, bytes, age, now), now).map(|candidate| candidate.kind)
        };
        assert_eq!(
            kind("/a/Downloads/empty.dmg", 0, 400),
            Some(CleanupKind::EmptyFile)
        );
        assert_eq!(kind("/a/.DS_Store", 6, 0), Some(CleanupKind::TemporaryFile));
        assert_eq!(
            kind("/a/~$report.docx", 162, 0),
            Some(CleanupKind::TemporaryFile)
        );
        assert_eq!(
            kind("/a/video.mp4.crdownload", 9, 0),
            Some(CleanupKind::IncompleteDownload)
        );
        assert_eq!(
            kind("/a/Downloads/Tool.DMG", 9, 400),
            Some(CleanupKind::Installer)
        );
        assert_eq!(kind("/a/backup.tar.gz", 9, 0), Some(CleanupKind::Archive));
        assert_eq!(
            kind("/a/downloads/notes.txt", 9, 90),
            Some(CleanupKind::OldDownload)
        );
        assert_eq!(kind("/a/downloads/notes.txt", 9, 89), None);
        assert_eq!(
            kind("/a/Downloads", 9, 400),
            None,
            "only files inside Downloads"
        );
        assert_eq!(kind("/a/Documents/notes.txt", 9, 400), None);

        let old = cleanup_candidate(&file("/a/Downloads/x.pdf", 9, 214, now), now).unwrap();
        assert_eq!(
            old.evidence,
            "In a Downloads folder and unmodified for 214 days when scanned"
        );
    }
}
