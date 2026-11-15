//! Builds the synthetic tree for the desktop smoke test. Run with a new folder path:
//! `cargo run --locked --release -p scan-engine --example smoke_fixture -- <folder>`
//! Its `images/` folder holds one similar-image group of three and one exact image duplicate.
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

    write_images(&root.join("images"))?;
    write_unreadable_folder(&root.join("locked"))?;
    println!("Smoke fixture ready at {}", root.display());
    Ok(())
}

/// Two scenes: one with resized, recompressed, and brightened copies (one similar group),
/// one with a byte-identical copy (an exact duplicate), plus a file that cannot be decoded.
fn write_images(folder: &Path) -> io::Result<()> {
    use image::imageops::{self, FilterType};
    use image::{Rgb, RgbImage};

    fs::create_dir(folder)?;
    let scene = |darkening: bool| {
        RgbImage::from_fn(640, 480, |x, y| {
            let shade = (x * 255 / 640) as u8;
            let shade = if darkening { 255 - shade } else { shade };
            let in_circle = x.abs_diff(320).pow(2) + y.abs_diff(240).pow(2) < 120 * 120;
            Rgb(if in_circle {
                [40, 120, 220]
            } else {
                [shade, shade / 2, 90]
            })
        })
    };
    let save =
        |image: &RgbImage, name: &str| image.save(folder.join(name)).map_err(io::Error::other);
    let sunrise = scene(false);
    save(&sunrise, "sunrise.png")?;
    save(
        &imageops::resize(&sunrise, 320, 240, FilterType::Lanczos3),
        "sunrise-small.jpg",
    )?;
    save(&imageops::brighten(&sunrise, 30), "sunrise-brighter.png")?;
    let sunset = scene(true);
    save(&sunset, "sunset.png")?;
    save(&sunset, "sunset copy.png")?;
    fs::write(folder.join("not-really.png"), "not an image")
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
