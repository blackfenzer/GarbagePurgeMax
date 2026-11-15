use std::path::Path;

/// Shown for files whose extension is missing or not in the table below.
pub const OTHER_FILE_TYPE: &str = "Other";

/// Names a broad file type from the path extension alone. File contents are not read,
/// so a renamed file is reported by its new extension.
pub fn file_type(path: &Path) -> &'static str {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return OTHER_FILE_TYPE;
    };
    match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "heif" | "webp" | "bmp" | "tif" | "tiff"
        | "svg" | "raw" | "cr2" | "nef" | "dng" | "psd" => "Images",
        "mp4" | "mov" | "m4v" | "mkv" | "avi" | "wmv" | "webm" => "Video",
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "aiff" | "ogg" | "opus" => "Audio",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key"
        | "odt" | "ods" | "odp" | "rtf" | "txt" | "md" | "csv" | "epub" => "Documents",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "dmg" => {
            "Archives & disk images"
        }
        "app" | "exe" | "msi" | "pkg" | "dll" | "dylib" | "so" => "Apps & installers",
        "rs" | "c" | "h" | "cpp" | "hpp" | "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "kt"
        | "swift" | "go" | "rb" | "php" | "cs" | "json" | "toml" | "yaml" | "yml" | "xml"
        | "html" | "css" | "sh" => "Code & data",
        _ => OTHER_FILE_TYPE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_known_extensions_and_falls_back_to_other() {
        assert_eq!(file_type(Path::new("Photos/IMG_1.HEIC")), "Images");
        assert_eq!(
            file_type(Path::new("backup.tar.gz")),
            "Archives & disk images"
        );
        assert_eq!(file_type(Path::new("README")), OTHER_FILE_TYPE);
        assert_eq!(file_type(Path::new(".gitignore")), OTHER_FILE_TYPE);
        assert_eq!(file_type(Path::new("notes.unknown")), OTHER_FILE_TYPE);
    }
}
