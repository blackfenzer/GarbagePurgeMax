use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use crate::{FileRecord, file_version::FileVersion};
use image::GrayImage;
use image::imageops::{self, FilterType};

/// Largest Hamming distance, of 64 bits, at which two images count as similar.
/// Chosen in docs/image-similarity.md. Confirm it on real photos before release.
pub const SIMILAR_IMAGE_DISTANCE: u32 = 6;

const HASHABLE_EXTENSIONS: [&str; 8] = ["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff"];

#[derive(Debug, Default)]
pub struct SimilarImageAnalysis {
    pub groups: Vec<SimilarImageGroup>,
    pub errors: u64,
    pub first_error: Option<String>,
    pub cancelled: bool,
}

/// A heuristic match, never a verified copy. The reference image comes first.
#[derive(Debug, PartialEq, Eq)]
pub struct SimilarImageGroup {
    pub members: Vec<SimilarImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimilarImage {
    pub path: PathBuf,
    pub hash: u64,
    /// Bits that differ from the group's reference hash.
    pub distance: u32,
}

/// Whether the decoder supports this file's extension.
pub fn is_hashable_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            HASHABLE_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// One bit per horizontal neighbor pair in a 9×8 grayscale thumbnail: brightness falls to the right.
pub fn difference_hash(image: &GrayImage) -> u64 {
    let small = imageops::resize(image, 9, 8, FilterType::Triangle);
    (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .fold(0, |hash, (x, y)| {
            hash << 1 | u64::from(small.get_pixel(x, y)[0] > small.get_pixel(x + 1, y)[0])
        })
}

/// Workers used when the caller does not choose: each decodes one bounded image at a time.
pub const DEFAULT_IMAGE_WORKERS: usize = 2;

/// Decodes images read-only with the default two workers and hashes them.
/// See [`find_similar_images_with`].
pub fn find_similar_images(files: &[FileRecord], cancelled: &AtomicBool) -> SimilarImageAnalysis {
    find_similar_images_with(
        files,
        DEFAULT_IMAGE_WORKERS,
        &AtomicUsize::new(0),
        cancelled,
    )
}

/// Decodes images read-only with `workers` threads (at least one) and hashes them, counting
/// each finished image in `processed`. Each image joins the closest earlier reference within
/// `SIMILAR_IMAGE_DISTANCE`, or becomes a reference itself, so every reported distance is
/// measured to the group's reference. Cancellation discards groups.
pub fn find_similar_images_with(
    files: &[FileRecord],
    workers: usize,
    processed: &AtomicUsize,
    cancelled: &AtomicBool,
) -> SimilarImageAnalysis {
    let next = AtomicUsize::new(0);
    let hashed = Mutex::new(Vec::with_capacity(files.len()));
    let workers = workers.max(1).min(files.len());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while !cancelled.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(record) = files.get(index) else {
                        return;
                    };
                    let hash = image_hash(record, cancelled);
                    hashed.lock().expect("image hash lock").push((index, hash));
                    processed.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });

    let mut analysis = SimilarImageAnalysis::default();
    let mut hashed = hashed.into_inner().expect("image hash lock");
    hashed.sort_unstable_by_key(|(index, _)| *index);
    for (index, hash) in &hashed {
        if let Err(error) = hash
            && error.kind() != io::ErrorKind::Interrupted
        {
            analysis.errors += 1;
            analysis
                .first_error
                .get_or_insert_with(|| format!("{}: {error}", files[*index].path.display()));
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        analysis.cancelled = true;
        return analysis;
    }
    let mut groups: Vec<SimilarImageGroup> = Vec::new();
    for (index, hash) in hashed {
        if cancelled.load(Ordering::Relaxed) {
            analysis.cancelled = true;
            return analysis;
        }
        let path = files[index].path.clone();
        let hash = match hash {
            Ok(hash) => hash,
            Err(_) => continue,
        };
        // shortcut: O(images × groups), add a hash index if measured photo libraries stall here.
        let closest = groups
            .iter_mut()
            .map(|group| ((group.members[0].hash ^ hash).count_ones(), group))
            .filter(|(distance, _)| *distance <= SIMILAR_IMAGE_DISTANCE)
            .min_by_key(|(distance, _)| *distance);
        match closest {
            Some((distance, group)) => group.members.push(SimilarImage {
                path,
                hash,
                distance,
            }),
            None => groups.push(SimilarImageGroup {
                members: vec![SimilarImage {
                    path,
                    hash,
                    distance: 0,
                }],
            }),
        }
    }
    groups.retain(|group| group.members.len() > 1);
    if cancelled.load(Ordering::Relaxed) {
        analysis.cancelled = true;
        return analysis;
    }
    analysis.groups = groups;
    analysis
}

// Two workers, each with a 64 MiB decoder allocation budget and bounded dimensions.
// Decoder limits do not include every codec's internal allocation or thumbnail conversion.
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

fn image_hash(record: &FileRecord, cancelled: &AtomicBool) -> io::Result<u64> {
    let image = read_image(record, cancelled)?;
    Ok(difference_hash(&image.to_luma8()))
}

/// A bounded PNG preview, read with the same version checks and decoder limits as analysis.
pub fn image_preview(record: &FileRecord, cancelled: &AtomicBool) -> io::Result<Vec<u8>> {
    let image = read_image(record, cancelled)?;
    let mut png = io::Cursor::new(Vec::new());
    image
        .thumbnail(320, 220)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok(png.into_inner())
}

fn read_image(record: &FileRecord, cancelled: &AtomicBool) -> io::Result<image::DynamicImage> {
    let (file, version) = FileVersion::open(&record.path)?;
    version.check_record(record)?;
    if version.len > MAX_DECODE_BYTES {
        return Err(io::Error::other(
            "image exceeds the 64 MiB encoded-file limit",
        ));
    }
    let input = CancelReader {
        inner: &file,
        cancelled,
    };
    let mut reader = image::ImageReader::new(BufReader::new(input)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(io::Error::other)?;
    version.check(&record.path, &file)?;
    Ok(image)
}

struct CancelReader<'a, R> {
    inner: R,
    cancelled: &'a AtomicBool,
}

impl<R: Read> Read for CancelReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "image analysis cancelled",
            ));
        }
        self.inner.read(buffer)
    }
}

impl<R: Seek> Seek for CancelReader<'_, R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// The difference hash reads horizontal brightness changes, so the distinct
    /// scene darkens from left to right while the original brightens.
    fn scene(darkening: bool) -> RgbImage {
        RgbImage::from_fn(320, 240, |x, y| {
            let brightening = x * 255 / 320;
            let shade = if darkening {
                255 - brightening
            } else {
                brightening
            } as u8;
            let in_square = (100..180).contains(&x) && (60..140).contains(&y);
            Rgb(if in_square { [250, 40, 40] } else { [shade; 3] })
        })
    }

    fn record(path: &Path) -> FileRecord {
        let metadata = std::fs::metadata(path).unwrap();
        FileRecord {
            path: path.into(),
            logical_bytes: metadata.len(),
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
            read_only: false,
        }
    }

    #[test]
    fn groups_a_resized_copy_and_reports_undecodable_files() -> io::Result<()> {
        let folder = tempfile::tempdir()?;
        let original = folder.path().join("original.png");
        let smaller = folder.path().join("smaller.jpg");
        let distinct = folder.path().join("distinct.png");
        let broken = folder.path().join("broken.jpg");
        let save = |image: &RgbImage, path: &Path| image.save(path).map_err(io::Error::other);
        save(&scene(false), &original)?;
        save(
            &imageops::resize(&scene(false), 160, 120, FilterType::Triangle),
            &smaller,
        )?;
        save(&scene(true), &distinct)?;
        std::fs::write(&broken, b"not an image")?;

        let paths = [original.clone(), broken, distinct, smaller.clone()];
        let files: Vec<_> = paths.iter().map(|path| record(path)).collect();
        let analysis = find_similar_images(&files, &AtomicBool::new(false));
        assert_eq!(analysis.errors, 1);
        assert_eq!(analysis.groups.len(), 1);
        let members = &analysis.groups[0].members;
        assert_eq!(members[0].path, original);
        assert_eq!(members[0].distance, 0);
        assert_eq!(members[1].path, smaller);
        assert!(members[1].distance <= SIMILAR_IMAGE_DISTANCE);
        assert!(is_hashable_image(Path::new("photo.JPEG")));
        assert!(!is_hashable_image(Path::new("photo.heic")));
        Ok(())
    }

    #[test]
    fn cancellation_discards_groups() {
        let analysis = find_similar_images(&[], &AtomicBool::new(true));
        assert!(analysis.cancelled);
        assert!(analysis.groups.is_empty());
    }
    #[test]
    fn rejects_changed_and_oversized_images_and_honors_read_cancellation() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("photo.png");
        scene(false).save(&path).map_err(io::Error::other)?;
        let indexed = record(&path);
        std::fs::write(&path, b"replacement")?;
        assert!(image_hash(&indexed, &AtomicBool::new(false)).is_err());
        image::GrayImage::new(MAX_DIMENSION + 1, 1)
            .save(&path)
            .map_err(io::Error::other)?;
        assert!(image_hash(&record(&path), &AtomicBool::new(false)).is_err());
        let cancel = AtomicBool::new(true);
        let mut input = CancelReader {
            inner: io::Cursor::new(b"image"),
            cancelled: &cancel,
        };
        assert_eq!(
            input.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn refuses_image_symlinks_after_indexing() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("photo.png");
        let target = root.path().join("target.png");
        scene(false).save(&path).map_err(io::Error::other)?;
        let indexed = record(&path);
        std::fs::rename(&path, &target)?;
        std::os::unix::fs::symlink(&target, &path)?;
        let analysis = find_similar_images(&[indexed], &AtomicBool::new(false));
        assert_eq!(analysis.errors, 1);
        assert!(analysis.groups.is_empty());
        Ok(())
    }
    #[test]
    fn preview_is_bounded_and_refuses_a_changed_source() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("large.png");
        image::RgbImage::new(2000, 1500)
            .save(&path)
            .map_err(io::Error::other)?;
        let indexed = record(&path);
        let png = image_preview(&indexed, &AtomicBool::new(false))?;
        let preview = image::load_from_memory(&png).map_err(io::Error::other)?;
        assert!(preview.width() <= 320 && preview.height() <= 220);
        std::fs::write(&path, b"changed")?;
        assert!(image_preview(&indexed, &AtomicBool::new(false)).is_err());
        Ok(())
    }
}
