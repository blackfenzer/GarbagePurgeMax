//! Colour file-type icons from Material Icon Theme (MIT; licence beside the SVGs in
//! `assets/file-icons`). Each icon is embedded and served at `file-icons/<name>.svg`.

use std::ffi::OsStr;
use std::path::Path;

/// Embedded icons by name. To add one, copy its SVG from the theme's `icons` folder.
const ICONS: &[(&str, &[u8])] = &[
    ("3d", include_bytes!("../assets/file-icons/3d.svg")),
    (
        "android",
        include_bytes!("../assets/file-icons/android.svg"),
    ),
    ("audio", include_bytes!("../assets/file-icons/audio.svg")),
    ("c", include_bytes!("../assets/file-icons/c.svg")),
    (
        "certificate",
        include_bytes!("../assets/file-icons/certificate.svg"),
    ),
    (
        "console",
        include_bytes!("../assets/file-icons/console.svg"),
    ),
    ("cpp", include_bytes!("../assets/file-icons/cpp.svg")),
    ("csharp", include_bytes!("../assets/file-icons/csharp.svg")),
    ("css", include_bytes!("../assets/file-icons/css.svg")),
    (
        "database",
        include_bytes!("../assets/file-icons/database.svg"),
    ),
    ("disc", include_bytes!("../assets/file-icons/disc.svg")),
    ("docker", include_bytes!("../assets/file-icons/docker.svg")),
    (
        "document",
        include_bytes!("../assets/file-icons/document.svg"),
    ),
    ("email", include_bytes!("../assets/file-icons/email.svg")),
    ("exe", include_bytes!("../assets/file-icons/exe.svg")),
    ("figma", include_bytes!("../assets/file-icons/figma.svg")),
    (
        "folder-base",
        include_bytes!("../assets/file-icons/folder-base.svg"),
    ),
    ("font", include_bytes!("../assets/file-icons/font.svg")),
    ("git", include_bytes!("../assets/file-icons/git.svg")),
    ("go", include_bytes!("../assets/file-icons/go.svg")),
    ("html", include_bytes!("../assets/file-icons/html.svg")),
    ("http", include_bytes!("../assets/file-icons/http.svg")),
    ("image", include_bytes!("../assets/file-icons/image.svg")),
    ("java", include_bytes!("../assets/file-icons/java.svg")),
    (
        "javascript",
        include_bytes!("../assets/file-icons/javascript.svg"),
    ),
    ("json", include_bytes!("../assets/file-icons/json.svg")),
    ("key", include_bytes!("../assets/file-icons/key.svg")),
    ("kotlin", include_bytes!("../assets/file-icons/kotlin.svg")),
    ("lock", include_bytes!("../assets/file-icons/lock.svg")),
    ("log", include_bytes!("../assets/file-icons/log.svg")),
    (
        "markdown",
        include_bytes!("../assets/file-icons/markdown.svg"),
    ),
    ("pdf", include_bytes!("../assets/file-icons/pdf.svg")),
    ("php", include_bytes!("../assets/file-icons/php.svg")),
    (
        "powerpoint",
        include_bytes!("../assets/file-icons/powerpoint.svg"),
    ),
    ("python", include_bytes!("../assets/file-icons/python.svg")),
    ("react", include_bytes!("../assets/file-icons/react.svg")),
    (
        "react_ts",
        include_bytes!("../assets/file-icons/react_ts.svg"),
    ),
    ("ruby", include_bytes!("../assets/file-icons/ruby.svg")),
    ("rust", include_bytes!("../assets/file-icons/rust.svg")),
    (
        "settings",
        include_bytes!("../assets/file-icons/settings.svg"),
    ),
    ("sketch", include_bytes!("../assets/file-icons/sketch.svg")),
    ("svg", include_bytes!("../assets/file-icons/svg.svg")),
    ("swift", include_bytes!("../assets/file-icons/swift.svg")),
    ("table", include_bytes!("../assets/file-icons/table.svg")),
    ("toml", include_bytes!("../assets/file-icons/toml.svg")),
    (
        "typescript",
        include_bytes!("../assets/file-icons/typescript.svg"),
    ),
    ("video", include_bytes!("../assets/file-icons/video.svg")),
    ("word", include_bytes!("../assets/file-icons/word.svg")),
    ("xml", include_bytes!("../assets/file-icons/xml.svg")),
    ("yaml", include_bytes!("../assets/file-icons/yaml.svg")),
    ("zip", include_bytes!("../assets/file-icons/zip.svg")),
];

const FOLDER: &str = "file-icons/folder-base.svg";
const DEFAULT: &str = "file-icons/document.svg";

/// The SVG bytes for an asset path such as `file-icons/rust.svg`.
pub(crate) fn load(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("file-icons/")?.strip_suffix(".svg")?;
    ICONS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, bytes)| *bytes)
}

/// The asset paths of every embedded icon.
pub(crate) fn paths() -> impl Iterator<Item = String> {
    ICONS
        .iter()
        .map(|(name, _)| format!("file-icons/{name}.svg"))
}

/// The icon for a folder, or for a file by its name and extension, like VS Code shows.
pub(crate) fn icon_path(name: &OsStr, is_folder: bool) -> &'static str {
    if is_folder {
        return FOLDER;
    }
    let name = name.to_string_lossy().to_ascii_lowercase();
    let by_name = match name.as_str() {
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | "compose.yaml" => {
            Some("docker")
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some("git"),
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "poetry.lock" => {
            Some("lock")
        }
        _ => None,
    };
    let extension = Path::new(&name)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    let icon = match by_name {
        Some(icon) => icon,
        None => match extension {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif"
            | "ico" | "raw" | "cr2" | "nef" | "arw" | "dng" => "image",
            "svg" => "svg",
            "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "wmv" | "flv" => "video",
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "aiff" => "audio",
            "pdf" => "pdf",
            "doc" | "docx" | "rtf" | "odt" | "pages" => "word",
            "xls" | "xlsx" | "csv" | "ods" | "numbers" => "table",
            "ppt" | "pptx" | "key" | "odp" => "powerpoint",
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" => "zip",
            "dmg" | "iso" | "img" => "disc",
            "pkg" | "mpkg" | "msi" | "msix" | "exe" => "exe",
            "apk" | "aab" => "android",
            "md" | "mdx" => "markdown",
            "json" | "jsonc" => "json",
            "yaml" | "yml" => "yaml",
            "xml" | "plist" => "xml",
            "toml" => "toml",
            "html" | "htm" => "html",
            "css" | "scss" | "sass" | "less" => "css",
            "js" | "mjs" | "cjs" => "javascript",
            "ts" | "mts" | "cts" => "typescript",
            "jsx" => "react",
            "tsx" => "react_ts",
            "rs" => "rust",
            "py" | "pyw" => "python",
            "go" => "go",
            "java" | "jar" => "java",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" => "cpp",
            "cs" => "csharp",
            "swift" => "swift",
            "kt" | "kts" => "kotlin",
            "rb" => "ruby",
            "php" => "php",
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => "console",
            "sql" | "db" | "sqlite" | "sqlite3" => "database",
            "ttf" | "otf" | "woff" | "woff2" => "font",
            "lock" => "lock",
            "log" => "log",
            "ini" | "conf" | "cfg" | "env" | "properties" => "settings",
            "pem" | "crt" | "cer" => "certificate",
            "p12" | "pfx" | "gpg" | "asc" => "key",
            "eml" | "msg" => "email",
            "fig" => "figma",
            "sketch" => "sketch",
            "obj" | "fbx" | "blend" | "stl" | "glb" | "gltf" | "usdz" => "3d",
            "http" | "rest" => "http",
            _ => return DEFAULT,
        },
    };
    ICON_PATHS
        .iter()
        .find(|path| {
            path.strip_prefix("file-icons/")
                .and_then(|rest| rest.strip_suffix(".svg"))
                == Some(icon)
        })
        .copied()
        .unwrap_or(DEFAULT)
}

/// Static asset paths for `ICONS`, so lookups return `&'static str` without allocating.
const ICON_PATHS: &[&str] = &[
    "file-icons/3d.svg",
    "file-icons/android.svg",
    "file-icons/audio.svg",
    "file-icons/c.svg",
    "file-icons/certificate.svg",
    "file-icons/console.svg",
    "file-icons/cpp.svg",
    "file-icons/csharp.svg",
    "file-icons/css.svg",
    "file-icons/database.svg",
    "file-icons/disc.svg",
    "file-icons/docker.svg",
    "file-icons/document.svg",
    "file-icons/email.svg",
    "file-icons/exe.svg",
    "file-icons/figma.svg",
    "file-icons/folder-base.svg",
    "file-icons/font.svg",
    "file-icons/git.svg",
    "file-icons/go.svg",
    "file-icons/html.svg",
    "file-icons/http.svg",
    "file-icons/image.svg",
    "file-icons/java.svg",
    "file-icons/javascript.svg",
    "file-icons/json.svg",
    "file-icons/key.svg",
    "file-icons/kotlin.svg",
    "file-icons/lock.svg",
    "file-icons/log.svg",
    "file-icons/markdown.svg",
    "file-icons/pdf.svg",
    "file-icons/php.svg",
    "file-icons/powerpoint.svg",
    "file-icons/python.svg",
    "file-icons/react.svg",
    "file-icons/react_ts.svg",
    "file-icons/ruby.svg",
    "file-icons/rust.svg",
    "file-icons/settings.svg",
    "file-icons/sketch.svg",
    "file-icons/svg.svg",
    "file-icons/swift.svg",
    "file-icons/table.svg",
    "file-icons/toml.svg",
    "file-icons/typescript.svg",
    "file-icons/video.svg",
    "file-icons/word.svg",
    "file-icons/xml.svg",
    "file-icons/yaml.svg",
    "file-icons/zip.svg",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_names_and_extensions_to_embedded_icons() {
        let icon = |name: &str| icon_path(OsStr::new(name), false);
        assert_eq!(icon("main.rs"), "file-icons/rust.svg");
        assert_eq!(icon("Photo.HEIC"), "file-icons/image.svg");
        assert_eq!(icon("Dockerfile"), "file-icons/docker.svg");
        assert_eq!(icon("Cargo.lock"), "file-icons/lock.svg");
        assert_eq!(icon("notes"), DEFAULT);
        assert_eq!(icon_path(OsStr::new("src"), true), FOLDER);
        for path in ICON_PATHS {
            assert!(
                load(path).is_some_and(|bytes| bytes.starts_with(b"<svg")),
                "{path}"
            );
        }
        assert_eq!(ICON_PATHS.len(), ICONS.len());
    }
}
