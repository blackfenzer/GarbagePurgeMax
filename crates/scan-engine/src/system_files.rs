//! Files and folders that operating systems create on their own, by exact name.
//!
//! Some are regenerated metadata that is safe to lose (Finder view settings, thumbnail
//! caches); others are managed by the OS and must stay (folder display names, Spotlight
//! indexes, paging files). Names are matched case-insensitively against the final component.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemFileKind {
    /// Recreated automatically when missing; removing it only resets cosmetic state.
    Regenerated,
    /// Maintained by the OS; removing it can change behaviour or break the volume.
    Managed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemFile {
    pub kind: SystemFileKind,
    /// What the file is, shown as evidence.
    pub description: &'static str,
}

const REGENERATED: &[(&str, &str)] = &[
    (".ds_store", "macOS Finder folder view settings"),
    ("thumbs.db", "Windows thumbnail cache"),
    ("ehthumbs.db", "Windows Media Center thumbnail cache"),
    ("ehthumbs_vista.db", "Windows thumbnail cache"),
    (".directory", "KDE folder view settings"),
    (".apdisk", "macOS AirPort disk sharing record"),
];

const MANAGED: &[(&str, &str)] = &[
    (
        ".localized",
        "macOS folder display-name marker; removing it changes how Finder names the folder",
    ),
    (
        "desktop.ini",
        "Windows folder customisation (name, icon, view)",
    ),
    ("icon\r", "macOS custom folder icon"),
    (".volumeicon.icns", "macOS custom volume icon"),
    (".spotlight-v100", "macOS Spotlight search index"),
    (".fseventsd", "macOS file-system event log"),
    (".trashes", "macOS per-volume Trash"),
    (".temporaryitems", "macOS temporary items folder"),
    (".documentrevisions-v100", "macOS document version history"),
    (".mobilebackups", "macOS Time Machine local snapshots"),
    (
        ".metadata_never_index",
        "macOS marker that disables Spotlight indexing",
    ),
    (
        ".com.apple.timemachine.donotpresent",
        "macOS Time Machine marker",
    ),
    (".pkinstallsandboxmanager", "macOS installer sandbox"),
    ("pagefile.sys", "Windows paging file"),
    ("hiberfil.sys", "Windows hibernation file"),
    ("swapfile.sys", "Windows swap file"),
    ("dumpstack.log.tmp", "Windows crash dump log"),
    ("ntuser.dat", "Windows user registry hive"),
    ("usrclass.dat", "Windows user registry hive"),
    ("$recycle.bin", "Windows Recycle Bin"),
    (
        "system volume information",
        "Windows restore points and volume data",
    ),
    ("bootmgr", "Windows boot manager"),
    ("lost+found", "Linux file-system recovery folder"),
];

/// Recognises an OS-generated file or folder by its name.
pub fn system_file(path: &Path) -> Option<SystemFile> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_lowercase();
    let lookup = |table: &[(&str, &'static str)], kind| {
        table
            .iter()
            .find(|(known, _)| *known == lower)
            .map(|(_, description)| SystemFile { kind, description })
    };
    if let Some(managed) = lookup(MANAGED, SystemFileKind::Managed) {
        return Some(managed);
    }
    // Registry hive transaction logs: NTUSER.DAT.LOG1, ntuser.dat{…}.TM.blf, and similar.
    if lower.starts_with("ntuser.dat") || lower.starts_with("usrclass.dat") {
        return Some(SystemFile {
            kind: SystemFileKind::Managed,
            description: "Windows user registry hive log",
        });
    }
    if lower.starts_with(".trash-") {
        return Some(SystemFile {
            kind: SystemFileKind::Managed,
            description: "Linux per-volume Trash",
        });
    }
    if let Some(regenerated) = lookup(REGENERATED, SystemFileKind::Regenerated) {
        return Some(regenerated);
    }
    // AppleDouble: macOS writes `._name` beside files copied to non-Mac drives.
    if name.starts_with("._") && name.len() > 2 {
        return Some(SystemFile {
            kind: SystemFileKind::Regenerated,
            description: "macOS AppleDouble metadata for a file copied to a non-Mac drive",
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_system_files_by_name() {
        let kind = |path: &str| system_file(Path::new(path)).map(|file| file.kind);
        assert_eq!(
            kind("/Users/me/Downloads/.DS_Store"),
            Some(SystemFileKind::Regenerated)
        );
        assert_eq!(
            kind("E:/Photos/._IMG_0001.JPG"),
            Some(SystemFileKind::Regenerated)
        );
        assert_eq!(
            kind("E:/Photos/Thumbs.db"),
            Some(SystemFileKind::Regenerated)
        );
        assert_eq!(
            kind("/Users/me/Documents/.localized"),
            Some(SystemFileKind::Managed)
        );
        assert_eq!(kind("D:/Music/desktop.ini"), Some(SystemFileKind::Managed));
        assert_eq!(
            kind("C:/Users/me/NTUSER.DAT.LOG1"),
            Some(SystemFileKind::Managed)
        );
        assert_eq!(
            kind("/Volumes/USB/.Spotlight-V100"),
            Some(SystemFileKind::Managed)
        );
        assert_eq!(
            kind("/Volumes/USB/.Trash-1000"),
            Some(SystemFileKind::Managed)
        );
        assert_eq!(kind("/Users/me/notes.txt"), None);
        assert_eq!(kind("/Users/me/._"), None);
    }
}
