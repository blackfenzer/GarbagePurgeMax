//! Builds the synthetic tree for the desktop smoke test. Run with a new folder path:
//! `cargo run --locked --release -p scan-engine --example smoke_fixture -- <folder>`
use std::env;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

const SEARCH_FOLDERS: u64 = 1_200;
const SEARCH_FILES_PER_FOLDER: u64 = 100;
const SEARCH_EXTENSIONS: [&str; 6] = [".txt", ".jpg", ".mp4", ".zip", ".rs", ""];
/// Sparse identical files: over the 1 GiB large-file threshold, and slow enough to hash
/// that duplicate verification can be cancelled by hand. They use no disk blocks.
const LARGE_COPIES: u64 = 6;
const LARGE_FILE_BYTES: u64 = 2 << 30;

fn main() -> io::Result<()> {
    let [root] = env::args_os()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pass one output folder"))?;
    let root = PathBuf::from(root);
    // Refuse to write into an existing folder so the fixture never mixes with real files.
    fs::create_dir(&root)?;

    for folder in 0..SEARCH_FOLDERS {
        let directory = root.join(format!(
            "search/batch-{:02}/folder-{folder:04}",
            folder / 100
        ));
        fs::create_dir_all(&directory)?;
        for file in 0..SEARCH_FILES_PER_FOLDER {
            let extension = SEARCH_EXTENSIONS[file as usize % SEARCH_EXTENSIONS.len()];
            File::create(directory.join(format!("report-{file:03}{extension}")))?
                .set_len(folder * SEARCH_FILES_PER_FOLDER + file)?;
        }
    }

    let duplicates = root.join("duplicates");
    fs::create_dir(&duplicates)?;
    for (name, contents) in [
        ("original.txt", "same contents"),
        ("copy of original.txt", "same contents"),
        ("same size, different.txt", "else contents"),
        ("photo.jpg", "jpeg-ish bytes"),
        ("photo (1).jpg", "jpeg-ish bytes"),
        ("empty-a", ""),
        ("empty-b", ""),
    ] {
        fs::write(duplicates.join(name), contents)?;
    }

    let large = root.join("large");
    fs::create_dir(&large)?;
    for copy in 0..LARGE_COPIES {
        File::create(large.join(format!("disk-image-{copy}.dmg")))?.set_len(LARGE_FILE_BYTES)?;
    }

    let excluded = root.join("exclude-me");
    fs::create_dir(&excluded)?;
    for file in 0..10 {
        fs::write(
            excluded.join(format!("hidden-by-exclusion-{file}.txt")),
            "skip",
        )?;
    }

    write_unreadable_folder(&root.join("locked"))?;
    println!("Smoke fixture ready at {}", root.display());
    Ok(())
}

#[cfg(unix)]
fn write_unreadable_folder(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::create_dir(folder)?;
    fs::write(folder.join("secret.txt"), "unreadable")?;
    // Restore with `chmod 755` before deleting the fixture.
    fs::set_permissions(folder, fs::Permissions::from_mode(0o000))
}

#[cfg(not(unix))]
fn write_unreadable_folder(folder: &Path) -> io::Result<()> {
    // ponytail: Windows ACL denial needs extra APIs; deny read on this folder by hand.
    fs::create_dir(folder)?;
    fs::write(folder.join("secret.txt"), "deny read access to this folder")
}
