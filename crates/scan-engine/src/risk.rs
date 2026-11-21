use std::path::Path;

use crate::app_cache::app_cache_origin;
use crate::system_files::{SystemFileKind, system_file};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskTier {
    Protected,
    High,
    Unknown,
    /// OS metadata that is recreated automatically, such as `.DS_Store`.
    Low,
}

impl RiskTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Protected => "Protected · cleanup blocked",
            Self::High => "High risk · review carefully",
            Self::Unknown => "Unknown risk · review carefully",
            Self::Low => "Low risk · recreated by the system",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskAssessment {
    pub tier: RiskTier,
    pub reason: &'static str,
}

/// Conservative path evidence, not an OS permission check or proof a file is disposable.
/// No extension, age, empty size, or duplicate match grants permission to remove anything.
pub fn assess_risk(path: &Path, read_only: bool) -> RiskAssessment {
    let names: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let contains = |known: &[&str]| names.iter().any(|name| known.contains(&name.as_str()));
    let protected = if read_only {
        Some("The indexed read-only attribute is set.")
    } else if app_cache_origin(path).is_some() {
        Some("A browser cache location may still contain data in use by the application.")
    } else if contains(&[
        "system",
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "library",
        "usr",
        "bin",
        "sbin",
        "etc",
        "applications",
        ".trash",
        "$recycle.bin",
        "system volume information",
    ]) {
        Some("A system or application location may contain required files.")
    } else if contains(&[
        "cloudstorage",
        "mobile documents",
        "icloud drive",
        "dropbox",
        "google drive",
        "googledrive",
    ]) || names
        .iter()
        .any(|name| name.starts_with("onedrive") || name.ends_with(".icloud"))
    {
        Some("A cloud-managed location may contain placeholders or synchronized data.")
    } else {
        None
    };
    if let Some(reason) = protected {
        return RiskAssessment {
            tier: RiskTier::Protected,
            reason,
        };
    }
    // Known OS files outrank folder rules: `.localized` in Documents must stay, while
    // `.DS_Store` anywhere outside a protected location is only cosmetic state.
    if let Some(file) = system_file(path) {
        return RiskAssessment {
            tier: match file.kind {
                SystemFileKind::Managed => RiskTier::Protected,
                SystemFileKind::Regenerated => RiskTier::Low,
            },
            reason: file.description,
        };
    }
    let high_risk = if contains(&[
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        "target",
        "vendor",
        ".venv",
        "venv",
        ".idea",
        ".vscode",
        "src",
    ]) {
        Some("Development files may be required by a project or its tools.")
    } else if contains(&[
        "documents",
        "pictures",
        "photos",
        "desktop",
        "music",
        "movies",
        "videos",
    ]) {
        Some("A personal-data location may hold irreplaceable files.")
    } else {
        None
    };
    match high_risk {
        Some(reason) => RiskAssessment {
            tier: RiskTier::High,
            reason,
        },
        None => RiskAssessment {
            tier: RiskTier::Unknown,
            reason: "Location and metadata do not establish whether this item is safe to remove.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_locations_before_considering_disposable_names() {
        for path in [
            "/System/cache.tmp",
            "C:/Windows/empty",
            "/Users/me/OneDrive - Work/old.zip",
            "/Users/me/photo.icloud",
        ] {
            assert_eq!(
                assess_risk(Path::new(path), false).tier,
                RiskTier::Protected,
                "{path}"
            );
        }
        for path in [
            "/project/.git/empty",
            "/project/src/cache.tmp",
            "/Users/me/Documents/old.zip",
        ] {
            assert_eq!(
                assess_risk(Path::new(path), false).tier,
                RiskTier::High,
                "{path}"
            );
        }
        assert_eq!(
            assess_risk(Path::new("/Downloads/old.zip"), false).tier,
            RiskTier::Unknown
        );
        assert_eq!(
            assess_risk(Path::new("empty"), true).tier,
            RiskTier::Protected
        );
        assert_eq!(
            assess_risk(Path::new("/Systematic/myfile"), false).tier,
            RiskTier::Unknown
        );
        let managed = assess_risk(Path::new("/Users/me/Documents/.localized"), false);
        assert_eq!(managed.tier, RiskTier::Protected);
        assert!(managed.reason.contains("display-name"));
        assert_eq!(
            assess_risk(Path::new("/Users/me/Documents/.DS_Store"), false).tier,
            RiskTier::Low
        );
        assert_eq!(
            assess_risk(Path::new("/System/Library/.DS_Store"), false).tier,
            RiskTier::Protected,
            "protected locations still win"
        );
    }
}
