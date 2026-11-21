//! Smart scan scope: what a broad scan leaves out, and what analysis never needs to read.
//!
//! A broad scan (a drive or home folder) does not index the regenerable internals of
//! developer projects: dependency folders, build output and Git object stores. Choosing a
//! folder inside a Git project scans it fully, nested repositories included.
//!
//! Game libraries, app bundles and system folders are always indexed for size, but their
//! files are left out of duplicate and similar-image analysis: people do not clean up
//! game assets or system files by hand, and they hold many identical files.

use std::ffi::OsStr;
use std::path::Path;

/// How much of a chosen folder to index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanScope {
    /// Index every file.
    Everything,
    /// Skip project dependencies and build output, unless the chosen folder is inside a
    /// Git project, which is then scanned in full.
    #[default]
    Smart,
}

impl ScanScope {
    /// Whether this scan of `root` skips project internals.
    pub fn skips_project_internals(self, root: &Path) -> bool {
        self == Self::Smart && !inside_git_project(root)
    }
}

/// The chosen folder, or a folder above it, holds a `.git` entry.
pub fn inside_git_project(root: &Path) -> bool {
    root.ancestors().any(|folder| folder.join(".git").exists())
}

/// Why a broad scan leaves `directory` out, if it is a regenerable project folder.
/// Dependency folders are recognised by name plus, where the name is generic, a marker file.
pub fn project_internals(directory: &Path) -> Option<&'static str> {
    let name = directory.file_name()?.to_str()?;
    let parent_has = |marker: &str| {
        directory
            .parent()
            .is_some_and(|parent| parent.join(marker).exists())
    };
    Some(match name {
        ".git" => "Git object store",
        "node_modules" => "JavaScript dependencies",
        ".next" | ".nuxt" | ".svelte-kit" | ".parcel-cache" | ".turbo" => "Web build cache",
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".tox" => "Python cache",
        ".venv" | "venv" | "env" if directory.join("pyvenv.cfg").exists() => {
            "Python virtual environment"
        }
        "target" if parent_has("Cargo.toml") => "Rust build output",
        ".gradle" => "Gradle cache",
        "build" if parent_has("build.gradle") || parent_has("build.gradle.kts") => {
            "Gradle build output"
        }
        ".dart_tool" => "Dart tool cache",
        "Pods" if parent_has("Podfile") => "CocoaPods dependencies",
        "vendor" if parent_has("composer.json") || parent_has("go.mod") => "Vendored dependencies",
        ".terraform" => "Terraform providers",
        "obj" | "bin" if has_dotnet_project(directory.parent()) => ".NET build output",
        _ => return None,
    })
}

fn has_dotnet_project(folder: Option<&Path>) -> bool {
    folder
        .and_then(|folder| std::fs::read_dir(folder).ok())
        .is_some_and(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name();
                let extension = Path::new(&name).extension().and_then(OsStr::to_str);
                matches!(extension, Some("csproj" | "fsproj" | "vbproj" | "sln"))
            })
        })
}

/// Why `path`'s contents are left out of duplicate and similar-image analysis.
/// Only folders below the scan root count, so choosing a game folder directly analyses it.
pub fn analysis_exemption(path: &Path, root: &Path) -> Option<&'static str> {
    let inside = path.strip_prefix(root).ok()?;
    let mut folders = inside.parent()?.components();
    folders.find_map(|folder| {
        let name = folder.as_os_str().to_str()?;
        let lower = name.to_ascii_lowercase();
        Some(match lower.as_str() {
            "steamapps"
            | "epic games"
            | "gog galaxy"
            | "gog games"
            | "xboxgames"
            | "riot games"
            | "ea games"
            | "origin games"
            | "ubisoft game launcher"
            | "battle.net" => "Game library",
            "windows" | "program files" | "program files (x86)" | "programdata" | "system" => {
                "System or installed software"
            }
            _ if lower.ends_with(".app") => "App bundle",
            _ => return None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn recognises_project_internals_by_name_and_marker() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("app");
        fs::create_dir_all(project.join("target"))?;
        fs::create_dir_all(project.join("vendor"))?;
        fs::create_dir_all(project.join(".venv"))?;
        assert_eq!(
            project_internals(&project.join("node_modules")),
            Some("JavaScript dependencies")
        );
        assert_eq!(
            project_internals(&project.join(".git")),
            Some("Git object store")
        );
        assert_eq!(
            project_internals(&project.join("target")),
            None,
            "no Cargo.toml yet"
        );
        assert_eq!(
            project_internals(&project.join("vendor")),
            None,
            "plain vendor folder"
        );
        assert_eq!(
            project_internals(&project.join(".venv")),
            None,
            "no pyvenv.cfg"
        );
        fs::write(project.join("Cargo.toml"), "")?;
        fs::write(project.join("go.mod"), "")?;
        fs::write(project.join(".venv/pyvenv.cfg"), "")?;
        assert_eq!(
            project_internals(&project.join("target")),
            Some("Rust build output")
        );
        assert_eq!(
            project_internals(&project.join("vendor")),
            Some("Vendored dependencies")
        );
        assert_eq!(
            project_internals(&project.join(".venv")),
            Some("Python virtual environment")
        );
        assert_eq!(project_internals(&project.join("src")), None);

        assert!(ScanScope::Smart.skips_project_internals(temp.path()));
        fs::create_dir(project.join(".git"))?;
        assert!(
            !ScanScope::Smart.skips_project_internals(&project),
            "chosen project scans fully"
        );
        assert!(!ScanScope::Smart.skips_project_internals(&project.join("target")));
        assert!(!ScanScope::Everything.skips_project_internals(temp.path()));
        Ok(())
    }

    #[test]
    fn exempts_games_and_system_folders_below_the_root_only() {
        let root = Path::new("/Volumes/Data");
        let game = root.join("SteamLibrary/steamapps/common/Game/textures/a.png");
        assert_eq!(analysis_exemption(&game, root), Some("Game library"));
        assert_eq!(
            analysis_exemption(
                &root.join("Tools/Editor.app/Contents/Resources/icon.png"),
                root
            ),
            Some("App bundle")
        );
        assert_eq!(analysis_exemption(&root.join("Photos/a.png"), root), None);
        let chosen = Path::new("/Volumes/Data/SteamLibrary/steamapps/common/Game");
        assert_eq!(
            analysis_exemption(&chosen.join("textures/a.png"), chosen),
            None
        );
    }
}
