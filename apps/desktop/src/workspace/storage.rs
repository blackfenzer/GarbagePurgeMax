use std::path::{Path, PathBuf};
use std::{fs, io};

use scan_store::ScanStore;

/// Keeps all app data (scans, exclusions, theme) in another folder, for tests and trial runs.
const DATA_DIR_OVERRIDE: &str = "STORAGE_CLINIC_DATA_DIR";

pub(super) fn database_path() -> io::Result<PathBuf> {
    if let Some(data) = std::env::var_os(DATA_DIR_OVERRIDE).filter(|data| !data.is_empty()) {
        return Ok(PathBuf::from(data).join("scans.sqlite3"));
    }
    dirs::data_local_dir()
        .map(|dir| dir.join("Storage Clinic").join("scans.sqlite3"))
        .ok_or_else(|| io::Error::other("could not find the app data directory"))
}

pub(super) fn open() -> io::Result<ScanStore> {
    open_at(&database_path()?)
}

pub(super) fn open_at(path: &Path) -> io::Result<ScanStore> {
    fs::create_dir_all(path.parent().expect("database has an app data directory"))?;
    ScanStore::open(path).map_err(io::Error::other)
}

pub(super) fn scan_exclusions(root: &Path, database: &Path) -> io::Result<Vec<PathBuf>> {
    let data = database
        .parent()
        .expect("database has an app data directory");
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let data = data.canonicalize().unwrap_or_else(|_| {
        data.parent()
            .and_then(|parent| parent.canonicalize().ok())
            .zip(data.file_name())
            .map_or_else(|| data.to_path_buf(), |(parent, name)| parent.join(name))
    });
    if root.starts_with(&data) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "choose a folder outside Storage Clinic’s data directory",
        ));
    }
    Ok(data
        .strip_prefix(&root)
        .ok()
        .map(Path::to_path_buf)
        .into_iter()
        .collect())
}
