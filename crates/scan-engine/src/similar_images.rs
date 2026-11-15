use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

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

/// Decodes images read-only on every core and hashes them. Each image joins the closest
/// earlier reference within `SIMILAR_IMAGE_DISTANCE`, or becomes a reference itself, so
/// every reported distance is measured to the group's reference. Cancellation discards groups.
pub fn find_similar_images(paths: &[PathBuf], cancelled: &AtomicBool) -> SimilarImageAnalysis {
    let next = AtomicUsize::new(0);
    let hashed = Mutex::new(Vec::with_capacity(paths.len()));
    let workers = thread::available_parallelism().map_or(1, usize::from);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while !cancelled.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { return };
                    let hash = image_hash(path);
                    hashed.lock().expect("image hash lock").push((index, hash));
                }
            });
        }
    });

    let mut analysis = SimilarImageAnalysis::default();
    if cancelled.load(Ordering::Relaxed) {
        analysis.cancelled = true;
        return analysis;
    }
    let mut hashed = hashed.into_inner().expect("image hash lock");
    hashed.sort_unstable_by_key(|(index, _)| *index);
    let mut groups: Vec<SimilarImageGroup> = Vec::new();
    for (index, hash) in hashed {
        let path = paths[index].clone();
        let hash = match hash {
            Ok(hash) => hash,
            Err(error) => {
                analysis.errors += 1;
                analysis
                    .first_error
                    .get_or_insert_with(|| format!("{}: {error}", path.display()));
                continue;
            }
        };
        // ponytail: compares each image with every reference, O(images × groups).
        // Index the hashes (a BK-tree) if large photo libraries stall here.
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
    analysis.groups = groups;
    analysis
}

fn image_hash(path: &Path) -> io::Result<u64> {
    let image = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(io::Error::other)?;
    Ok(difference_hash(&image.to_luma8()))
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
        let analysis = find_similar_images(&paths, &AtomicBool::new(false));
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
        let paths = [PathBuf::from("missing.png")];
        let analysis = find_similar_images(&paths, &AtomicBool::new(true));
        assert!(analysis.cancelled);
        assert!(analysis.groups.is_empty());
    }
}
