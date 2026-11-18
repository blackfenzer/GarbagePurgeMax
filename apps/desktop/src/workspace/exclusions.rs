use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Context, PathPromptOptions};

use super::{Workspace, storage};

const FILE_HEADER: &[u8] = b"SCE1";

pub(super) struct Exclusions {
    pub paths: Vec<PathBuf>,
    pub busy: bool,
    pub error: Option<String>,
}

impl Exclusions {
    pub(super) fn load(cx: &mut Context<Workspace>) -> Self {
        load_or_edit(cx, "load", |database| read_paths(&settings_path(&database)));
        Self {
            paths: Vec::new(),
            busy: true,
            error: None,
        }
    }
}

impl Workspace {
    pub(super) fn exclusion_settings_busy(&self) -> bool {
        self.exclusions.busy
    }

    pub(super) fn exclusion_controls_disabled(&self) -> bool {
        self.exclusion_settings_busy()
            || self.scan.choosing_folder
            || self.scan.active.is_some()
            || self.cleanup.busy
    }

    pub(super) fn choose_exclusion(&mut self, folder: bool, cx: &mut Context<Self>) {
        if self.exclusion_controls_disabled() {
            return;
        }
        self.exclusions.busy = true;
        cx.notify();

        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(
                if folder {
                    "Exclude folder"
                } else {
                    "Exclude file"
                }
                .into(),
            ),
        });
        cx.spawn(async move |workspace, cx| {
            let selection = picker
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
                .map(|paths| paths.and_then(|paths| paths.into_iter().next()));
            let _ = workspace.update(cx, |workspace, cx| {
                match selection {
                    Ok(Some(path)) => {
                        workspace.exclusions.error = None;
                        load_or_edit(cx, "save", move |database| {
                            let path = normalize_selection(&path)?;
                            edit(&database, |paths| paths.push(path))
                        });
                    }
                    Ok(None) => workspace.exclusions.busy = false,
                    Err(error) => {
                        workspace.exclusions.busy = false;
                        workspace.exclusions.error = Some(format!("Path picker failed: {error}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn remove_exclusion(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.exclusion_controls_disabled() {
            return;
        }
        self.exclusions.busy = true;
        self.exclusions.error = None;
        load_or_edit(cx, "save", move |database| {
            edit(&database, |paths| paths.retain(|entry| entry != &path))
        });
        cx.notify();
    }
}

fn load_or_edit(
    cx: &mut Context<Workspace>,
    action: &'static str,
    operation: impl FnOnce(PathBuf) -> io::Result<Vec<PathBuf>> + Send + 'static,
) {
    cx.spawn(async move |workspace, cx| {
        let result = cx
            .background_spawn(async move { operation(storage::database_path()?) })
            .await;
        let _ = workspace.update(cx, |workspace, cx| {
            workspace.exclusions.busy = false;
            match result {
                Ok(paths) => {
                    workspace.exclusions.paths = paths;
                    workspace.exclusions.error = None;
                }
                Err(error) => {
                    workspace.exclusions.error =
                        Some(format!("Could not {action} scan exclusions: {error}"))
                }
            }
            cx.notify();
        });
    })
    .detach();
}

pub(super) fn for_scan(root: &Path, database: &Path) -> io::Result<Vec<PathBuf>> {
    let paths = read_paths(&settings_path(database)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not load scan exclusions: {error}"),
        )
    })?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let root = root.canonicalize()?;
    let mut relative_paths = Vec::new();
    for path in paths {
        if root.starts_with(&path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the selected folder is covered by an exclusion for {}",
                    path_label(&path)
                ),
            ));
        }
        if let Ok(relative) = path.strip_prefix(&root) {
            relative_paths.push(relative.to_path_buf());
        }
    }
    Ok(relative_paths)
}

/// Re-read current exclusions before queueing and immediately before a cleanup action.
pub(super) fn check_cleanup(source: &Path, database: &Path) -> io::Result<()> {
    for excluded in read_paths(&settings_path(database))? {
        if source.starts_with(&excluded) {
            return Err(io::Error::other(
                "This item is covered by a current exclusion",
            ));
        }
    }
    Ok(())
}

pub(super) fn path_label(path: &Path) -> String {
    let text = path.display().to_string();
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        if let Some(Component::Prefix(prefix)) = path.components().next() {
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => return text[4..].to_owned(),
                Prefix::VerbatimUNC(_, _) => return format!(r"\\{}", &text[8..]),
                _ => {}
            }
        }
    }
    text
}

fn normalize_selection(path: &Path) -> io::Result<PathBuf> {
    let metadata = path.symlink_metadata()?;
    #[cfg(windows)]
    let is_link = {
        use std::os::windows::fs::MetadataExt as _;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    };
    #[cfg(not(windows))]
    let is_link = metadata.is_symlink();

    // Keep the selected name if it is a link so the rule does not target its destination.
    if is_link && let Some((parent, name)) = path.parent().zip(path.file_name()) {
        return Ok(parent.canonicalize()?.join(name));
    }
    path.canonicalize()
}

fn settings_path(database: &Path) -> PathBuf {
    database.with_file_name("exclusions.bin")
}

fn edit(database: &Path, change: impl FnOnce(&mut Vec<PathBuf>)) -> io::Result<Vec<PathBuf>> {
    let settings = settings_path(database);
    let directory = settings
        .parent()
        .expect("settings have an app data directory");
    fs::create_dir_all(directory)?;

    // Serialize read-modify-write between app instances. Readers see an atomic replacement.
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("exclusions.lock"))?;
    lock.lock()?;
    let mut paths = read_paths(&settings)?;
    change(&mut paths);
    paths.sort_unstable();
    paths.dedup();

    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(FILE_HEADER)?;
    for path in &paths {
        let bytes = encode_path(path);
        let length = u32::try_from(bytes.len()).map_err(io::Error::other)?;
        file.write_all(&length.to_le_bytes())?;
        file.write_all(&bytes)?;
    }
    file.as_file().sync_all()?;
    file.persist(settings).map_err(|error| error.error)?;
    Ok(paths)
}

fn read_paths(settings: &Path) -> io::Result<Vec<PathBuf>> {
    let bytes = match fs::read(settings) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid exclusion settings file",
        )
    };
    let mut data = bytes.strip_prefix(FILE_HEADER).ok_or_else(invalid)?;
    let mut paths = Vec::new();
    while !data.is_empty() {
        let (length, entries) = data.split_first_chunk::<4>().ok_or_else(invalid)?;
        let length = u32::from_le_bytes(*length) as usize;
        let (entry, remaining) = entries.split_at_checked(length).ok_or_else(invalid)?;
        let path = PathBuf::from(decode_path(entry).ok_or_else(invalid)?);
        if !path.is_absolute() {
            return Err(invalid());
        }
        paths.push(path);
        data = remaining;
    }
    Ok(paths)
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_path(bytes: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt as _;

    (!bytes.contains(&0)).then(|| OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn decode_path(bytes: &[u8]) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt as _;

    let (chunks, remainder) = bytes.as_chunks::<2>();
    let words: Vec<_> = chunks.iter().copied().map(u16::from_le_bytes).collect();
    (remainder.is_empty() && !words.contains(&0)).then(|| OsString::from_wide(&words))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_saved_exclusions_against_each_scan_root() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("app-data/scans.sqlite3");
        let root = temp.path().join("scans");
        fs::create_dir_all(root.join("skip/nested"))?;
        assert!(for_scan(&root, &database)?.is_empty());

        let root = root.canonicalize()?;
        let excluded_folder = root.join("skip");
        let excluded_file = root.join("one.txt");
        let unrelated = temp.path().canonicalize()?.join("scans-other/file.txt");
        let paths = edit(&database, |paths| {
            paths.extend([excluded_file.clone(), excluded_folder.clone(), unrelated]);
            paths.push(excluded_file.clone());
        })?;
        assert_eq!(paths.len(), 3);
        assert_eq!(read_paths(&settings_path(&database))?, paths);
        assert_eq!(
            for_scan(&root, &database)?,
            [PathBuf::from("one.txt"), PathBuf::from("skip")],
        );
        for path in [&excluded_folder, &excluded_folder.join("nested")] {
            assert_eq!(
                for_scan(path, &database).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
            );
        }

        edit(&database, |paths| {
            paths.retain(|path| path != &excluded_folder)
        })?;
        assert_eq!(for_scan(&root, &database)?, [PathBuf::from("one.txt")]);
        Ok(())
    }

    #[test]
    fn reports_corrupt_settings_and_preserves_the_existing_file() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("scans.sqlite3");
        let settings = settings_path(&database);
        let corrupt = b"SCE1\x05\x00\x00\x00a";
        fs::write(&settings, corrupt)?;
        assert_eq!(
            for_scan(temp.path(), &database).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
        );
        assert_eq!(
            edit(&database, Vec::clear).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
        );
        assert_eq!(fs::read(&settings)?, corrupt);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn round_trips_non_utf8_paths() -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt as _;

        let temp = tempfile::tempdir()?;
        let database = temp.path().join("scans.sqlite3");
        let root = temp.path().canonicalize()?;
        let name = std::ffi::OsStr::from_bytes(b"file-\xff\n.txt");
        edit(&database, |paths| paths.push(root.join(name)))?;
        assert_eq!(for_scan(&root, &database)?, [PathBuf::from(name)]);
        Ok(())
    }
}
