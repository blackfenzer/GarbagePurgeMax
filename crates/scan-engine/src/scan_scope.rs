//! Smart scan scope: what a broad scan leaves out, and what analysis never needs to read.
//!
//! A broad scan (a drive or home folder) does not index the regenerable internals of
//! developer projects: dependency folders, build output and Git object stores. Choosing a
//! folder inside a Git project scans it fully, nested repositories included.
//!
//! Game data, app bundles, system folders, disk images, device backups, shared developer
//! caches and online-only cloud files are indexed for size but left out of duplicate and
//! similar-image analysis: people do not clean them up file by file, they hold many
//! identical files, and reading a cloud placeholder would download it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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
/// Generic names count only beside the marker file of the tool that creates them.
pub fn project_internals(directory: &Path) -> Option<&'static str> {
    let name = directory.file_name()?.to_str()?;
    let parent = directory.parent();
    let parent_has = |marker: &str| parent.is_some_and(|parent| parent.join(marker).exists());
    let parent_name = parent.and_then(Path::file_name).and_then(OsStr::to_str);
    Some(match name {
        ".git" => "Git object store",
        // JavaScript and web tooling.
        "node_modules" | "bower_components" | "jspm_packages" => "JavaScript dependencies",
        ".next" | ".nuxt" | ".svelte-kit" | ".parcel-cache" | ".turbo" | ".vite" | ".expo"
        | ".docusaurus" | ".angular" => "Web build cache",
        "cache" if parent_name == Some(".yarn") => "Yarn package cache",
        // Python.
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".tox" | ".nox"
        | ".hypothesis" | ".eggs" | ".ipynb_checkpoints" => "Python cache",
        _ if name.ends_with(".egg-info") => "Python cache",
        ".venv" | "venv" | "env" if directory.join("pyvenv.cfg").exists() => {
            "Python virtual environment"
        }
        // Compiled languages and their package managers.
        "target" if parent_has("Cargo.toml") => "Rust build output",
        ".gradle" | ".kotlin" => "Gradle cache",
        ".cxx" => "Android native build output",
        "build" if parent_has("build.gradle") || parent_has("build.gradle.kts") => {
            "Gradle build output"
        }
        "build" if parent_has("pubspec.yaml") => "Flutter build output",
        ".dart_tool" => "Dart tool cache",
        "Pods" if parent_has("Podfile") => "CocoaPods dependencies",
        "vendor" if parent_has("composer.json") || parent_has("go.mod") => "Vendored dependencies",
        "vendor" if parent_has("Gemfile") => "Ruby gems",
        "_build" | "deps" if parent_has("mix.exs") => "Elixir build output",
        ".stack-work" => "Haskell build output",
        ".zig-cache" | "zig-cache" => "Zig build cache",
        "CMakeFiles" => "CMake build output",
        _ if name.starts_with("cmake-build-") => "CMake build output",
        ".terraform" => "Terraform providers",
        "obj" | "bin" if parent_has_extension(parent, &["csproj", "fsproj", "vbproj", "sln"]) => {
            ".NET build output"
        }
        // Game engine projects.
        "Library" | "Temp" | "Obj" | "Logs" if parent_has("ProjectSettings") => {
            "Unity generated files"
        }
        "Intermediate" | "DerivedDataCache" | "Saved"
            if parent_has_extension(parent, &["uproject"]) =>
        {
            "Unreal generated files"
        }
        ".godot" | ".import" if parent_has("project.godot") => "Godot import cache",
        _ => return None,
    })
}

fn parent_has_extension(folder: Option<&Path>, extensions: &[&str]) -> bool {
    folder
        .and_then(|folder| std::fs::read_dir(folder).ok())
        .is_some_and(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name();
                Path::new(&name)
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|extension| extensions.contains(&extension))
            })
        })
}

/// Why `path` is left out of duplicate and similar-image analysis. Folder rules apply only
/// below the scan root, so choosing a game folder directly analyses it.
pub fn analysis_exemption(path: &Path, root: &Path) -> Option<&'static str> {
    let inside = path.strip_prefix(root).ok()?;
    if let Some(container) = path
        .extension()
        .and_then(OsStr::to_str)
        .and_then(container_file)
    {
        return Some(container);
    }
    if path.file_name().is_some_and(|name| name == "Docker.raw") {
        return Some("Virtual machine or disk image");
    }
    if regenerable_cache(path).is_some() {
        return Some("Regenerable developer cache");
    }
    let folders: Vec<String> = inside
        .parent()?
        .components()
        .filter_map(|folder| folder.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .collect();
    folders.iter().enumerate().find_map(|(position, lower)| {
        let next = folders.get(position + 1).map(String::as_str);
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
            | "battle.net"
            | "rockstar games"
            | "heroic"
            | "lutris"
            | ".minecraft"
            | "shadercache"
            | "shader cache" => "Game library",
            // Wine, CrossOver and Proton prefixes hold a whole Windows installation.
            "drive_c" => "Windows compatibility prefix",
            "mobilesync" if next == Some("backup") => "Device backup",
            "windows" | "program files" | "program files (x86)" | "programdata" | "system" => {
                "System or installed software"
            }
            _ if lower.ends_with(".app") => "App bundle",
            _ if lower.ends_with(".photoslibrary") => "Photos library",
            _ if lower.ends_with(".sparsebundle") => "Virtual machine or disk image",
            _ => return None,
        })
    })
}

/// Single files too large and too unique to be worth hashing for duplicates.
fn container_file(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "vmdk" | "vhd" | "vhdx" | "qcow2" | "vdi" | "hdd" | "sparseimage" => {
            Some("Virtual machine or disk image")
        }
        _ => None,
    }
}

/// A shared developer cache outside any project, which its tool can download or rebuild.
/// Returns the cache's name and its top folder.
pub fn regenerable_cache(path: &Path) -> Option<(&'static str, PathBuf)> {
    const CACHES: &[(&[&str], &str)] = &[
        (&[".npm", "_cacache"], "npm cache"),
        (&[".pnpm-store"], "pnpm store"),
        (&["library", "pnpm", "store"], "pnpm store"),
        (&["library", "caches", "yarn"], "Yarn cache"),
        (&["appdata", "local", "yarn", "cache"], "Yarn cache"),
        (&[".cargo", "registry"], "Cargo registry"),
        (&[".cargo", "git"], "Cargo Git checkouts"),
        (&[".gradle", "caches"], "Gradle caches"),
        (&[".m2", "repository"], "Maven repository"),
        (&["library", "caches", "pip"], "pip cache"),
        (&[".cache", "pip"], "pip cache"),
        (&["appdata", "local", "pip", "cache"], "pip cache"),
        (&["go", "pkg", "mod"], "Go module cache"),
        (&["library", "caches", "homebrew"], "Homebrew downloads"),
        (
            &["library", "developer", "xcode", "deriveddata"],
            "Xcode DerivedData",
        ),
        (
            &["library", "developer", "xcode", "ios devicesupport"],
            "Xcode device support",
        ),
        (
            &["library", "developer", "coresimulator", "caches"],
            "Simulator caches",
        ),
        (
            &["library", "android", "sdk", "system-images"],
            "Android system images",
        ),
        (
            &["appdata", "local", "android", "sdk", "system-images"],
            "Android system images",
        ),
        (
            &["android", "sdk", "system-images"],
            "Android system images",
        ),
    ];
    let components: Vec<_> = path.components().collect();
    let lower: Vec<String> = components
        .iter()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    CACHES.iter().find_map(|(pattern, label)| {
        let start = lower
            .windows(pattern.len())
            .position(|window| window.iter().zip(pattern.iter()).all(|(a, b)| a == b))?;
        // The cache must hold the path, not be the path itself.
        let end = start + pattern.len();
        (end < components.len()).then(|| (*label, components[..end].iter().collect()))
    })
}

/// Whether `path` is an online-only cloud file (iCloud Drive, OneDrive, Dropbox, Google
/// Drive): reading its contents would make the provider download it.
pub fn is_cloud_placeholder(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // Darwin SF_DATALESS: metadata exists but contents need provider hydration.
        const SF_DATALESS: u32 = 0x4000_0000;
        metadata.st_flags() & SF_DATALESS != 0
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        metadata.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = metadata;
        false
    }
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

    #[test]
    fn recognises_game_engine_and_language_folders() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let unity = temp.path().join("Shooter");
        fs::create_dir_all(unity.join("ProjectSettings"))?;
        let unreal = temp.path().join("Racer");
        fs::create_dir_all(&unreal)?;
        fs::write(unreal.join("Racer.uproject"), "")?;
        let ruby = temp.path().join("site");
        fs::create_dir_all(&ruby)?;
        fs::write(ruby.join("Gemfile"), "")?;
        assert_eq!(
            project_internals(&unity.join("Library")),
            Some("Unity generated files")
        );
        assert_eq!(
            project_internals(&temp.path().join("Library")),
            None,
            "macOS ~/Library"
        );
        assert_eq!(
            project_internals(&unreal.join("DerivedDataCache")),
            Some("Unreal generated files")
        );
        assert_eq!(project_internals(&ruby.join("vendor")), Some("Ruby gems"));
        assert_eq!(
            project_internals(&ruby.join(".yarn/cache")),
            Some("Yarn package cache")
        );
        assert_eq!(project_internals(&ruby.join("cache")), None);
        assert_eq!(
            project_internals(&ruby.join("app.egg-info")),
            Some("Python cache")
        );
        assert_eq!(
            project_internals(&ruby.join("cmake-build-debug")),
            Some("CMake build output")
        );
        assert_eq!(
            project_internals(&ruby.join("dist")),
            None,
            "generic output stays visible"
        );
        Ok(())
    }

    #[test]
    fn exempts_containers_backups_caches_and_prefixes() {
        let root = Path::new("/Users/you");
        let exempt = |relative: &str| analysis_exemption(&root.join(relative), root);
        assert_eq!(
            exempt("VMs/win11.vhdx"),
            Some("Virtual machine or disk image")
        );
        assert_eq!(
            exempt("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw"),
            Some("Virtual machine or disk image")
        );
        assert_eq!(
            exempt("Library/Application Support/MobileSync/Backup/abc/00/0011"),
            Some("Device backup")
        );
        assert_eq!(
            exempt("Pictures/Photos Library.photoslibrary/resources/derivatives/a.jpg"),
            Some("Photos library")
        );
        assert_eq!(
            exempt("Games/wine/drive_c/Program Files/x.dll"),
            Some("Windows compatibility prefix")
        );
        assert_eq!(
            exempt(".npm/_cacache/content-v2/sha512/ab/cd"),
            Some("Regenerable developer cache")
        );
        assert_eq!(exempt("Documents/report.pdf"), None);
    }

    #[test]
    fn names_shared_developer_caches_and_their_roots() {
        let (label, root) = regenerable_cache(Path::new(
            "/Users/you/Library/Developer/Xcode/DerivedData/App-abc/Build/x.o",
        ))
        .unwrap();
        assert_eq!(label, "Xcode DerivedData");
        assert_eq!(
            root,
            Path::new("/Users/you/Library/Developer/Xcode/DerivedData")
        );
        let (label, root) = regenerable_cache(Path::new(
            "/home/you/.cargo/registry/cache/index/crate.crate",
        ))
        .unwrap();
        assert_eq!(label, "Cargo registry");
        assert_eq!(root, Path::new("/home/you/.cargo/registry"));
        assert!(regenerable_cache(Path::new("/Users/you/.cargo/registry")).is_none());
        assert!(regenerable_cache(Path::new("/Users/you/Documents/go/notes.txt")).is_none());
    }
}
