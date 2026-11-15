//! Evaluates 64-bit perceptual hashes on generated images and their edited copies.
//! `cargo run --locked --release -p scan-engine --example image_similarity`
//! Results and method: docs/image-similarity.md.
use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{GrayImage, Rgb, RgbImage};

const BASE_IMAGES: u64 = 16;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const MAX_DISTANCE: u32 = 32;

type Hasher = fn(&GrayImage) -> u64;
type Edit = fn(&RgbImage) -> RgbImage;

const HASHERS: [(&str, Hasher); 3] = [
    ("Average", average_hash),
    ("Difference", difference_hash),
    ("DCT", dct_hash),
];

/// Edits a similarity feature should match. Flips and rotations are reported
/// separately because these hashes are not designed to survive them.
const EDITS: [(&str, Edit, bool); 12] = [
    ("Resize to 50%", |image| scale(image, 0.5), true),
    ("Resize to 25%", |image| scale(image, 0.25), true),
    ("Upscale to 150%", |image| scale(image, 1.5), true),
    ("Crop 5% per side", |image| crop(image, 0.05, 0.05), true),
    ("Crop 15% per side", |image| crop(image, 0.15, 0.15), true),
    ("Crop 30% from the right", crop_right, true),
    (
        "Brighten by 30",
        |image| imageops::brighten(image, 30),
        true,
    ),
    (
        "Contrast +30%",
        |image| imageops::contrast(image, 30.0),
        true,
    ),
    ("JPEG quality 40", jpeg_40, true),
    ("Watermark block", watermark, true),
    (
        "Mirror horizontally",
        |image| imageops::flip_horizontal(image),
        false,
    ),
    ("Rotate 90°", |image| imageops::rotate90(image), false),
];

struct Sample {
    source: u64,
    edit: Option<usize>,
    hashes: [u64; 3],
}

fn main() {
    let mut samples = Vec::new();
    let mut hash_time = [0_f64; 3];
    for source in 0..BASE_IMAGES {
        let base = scene(source);
        let mut images = vec![(None, base.clone())];
        images.extend(
            EDITS
                .iter()
                .enumerate()
                .map(|(index, (_, edit, _))| (Some(index), edit(&base))),
        );
        for (edit, image) in images {
            let gray = imageops::grayscale(&image);
            let mut hashes = [0; 3];
            for (index, (_, hasher)) in HASHERS.iter().enumerate() {
                let started = Instant::now();
                hashes[index] = hasher(&gray);
                hash_time[index] += started.elapsed().as_secs_f64();
            }
            samples.push(Sample {
                source,
                edit,
                hashes,
            });
        }
    }

    // Positives: each base against its own edits. Negatives: every pair from different sources.
    let positive = |edit: usize| {
        samples
            .iter()
            .filter(move |sample| sample.edit == Some(edit))
            .map(|sample| {
                (
                    sample,
                    &samples[(sample.source * (EDITS.len() as u64 + 1)) as usize],
                )
            })
    };
    let negatives: Vec<_> = samples
        .iter()
        .enumerate()
        .flat_map(|(index, left)| samples[index + 1..].iter().map(move |right| (left, right)))
        .filter(|(left, right)| left.source != right.source)
        .collect();
    let distance = |algorithm: usize, left: &Sample, right: &Sample| {
        (left.hashes[algorithm] ^ right.hashes[algorithm]).count_ones()
    };

    println!(
        "{} base images, {} edited copies, {} cross-source pairs\n",
        BASE_IMAGES,
        BASE_IMAGES as usize * EDITS.len(),
        negatives.len()
    );
    // The strictest useful threshold: one below the closest pair from different sources.
    let thresholds: Vec<u32> = (0..HASHERS.len())
        .map(|algorithm| {
            negatives
                .iter()
                .map(|(left, right)| distance(algorithm, left, right))
                .min()
                .unwrap_or(MAX_DISTANCE)
                .saturating_sub(1)
        })
        .collect();
    let matched = |algorithm: usize, edit: usize, threshold: u32| {
        positive(edit)
            .filter(|(copy, original)| distance(algorithm, copy, original) <= threshold)
            .count()
    };

    println!(
        "Largest distance from the original (of 64 bits) · copies matched with no false matches\n"
    );
    println!("| Edit | Average | Difference | DCT |\n| --- | ---: | ---: | ---: |");
    for (edit, (label, _, in_scope)) in EDITS.iter().enumerate() {
        let cells: Vec<_> = (0..HASHERS.len())
            .map(|algorithm| {
                let worst = positive(edit)
                    .map(|(copy, original)| distance(algorithm, copy, original))
                    .max()
                    .unwrap_or(0);
                let hits = matched(algorithm, edit, thresholds[algorithm]);
                format!("{worst} · {hits}/{BASE_IMAGES}")
            })
            .collect();
        let scope = if *in_scope { "" } else { " (out of scope)" };
        println!("| {label}{scope} | {} |", cells.join(" | "));
    }

    let in_scope: Vec<_> = (0..EDITS.len()).filter(|&edit| EDITS[edit].2).collect();
    let in_scope_total = in_scope.len() * BASE_IMAGES as usize;
    println!(
        "\nThreshold sweep: in-scope copies matched of {in_scope_total}, and false matches of {}\n",
        negatives.len()
    );
    println!("| Threshold | Average | Difference | DCT |\n| ---: | ---: | ---: | ---: |");
    for threshold in [4, 6, 8, 10, 12, 16] {
        let cells: Vec<_> = (0..HASHERS.len())
            .map(|algorithm| {
                let hits: usize = in_scope
                    .iter()
                    .map(|&edit| matched(algorithm, edit, threshold))
                    .sum();
                let false_matches = negatives
                    .iter()
                    .filter(|(left, right)| distance(algorithm, left, right) <= threshold)
                    .count();
                format!("{hits} · {false_matches} false")
            })
            .collect();
        println!("| ≤ {threshold} | {} |", cells.join(" | "));
    }

    let cells: Vec<_> = (0..HASHERS.len())
        .map(|algorithm| {
            format!(
                "≤ {} · {:.0} µs",
                thresholds[algorithm],
                hash_time[algorithm] / samples.len() as f64 * 1e6
            )
        })
        .collect();
    println!(
        "\n| Zero-false-match threshold · mean hash time | {} |",
        cells.join(" | ")
    );
}

/// A gradient background with overlapping shapes and light noise, seeded per image.
fn scene(seed: u64) -> RgbImage {
    let mut random = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let colors = [random.color(), random.color()];
    let direction = seed % 3;
    let mut image = RgbImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let t = match direction {
            0 => x as f32 / WIDTH as f32,
            1 => y as f32 / HEIGHT as f32,
            _ => (x + y) as f32 / (WIDTH + HEIGHT) as f32,
        };
        Rgb(std::array::from_fn(|channel| {
            (colors[0][channel] as f32 * (1.0 - t) + colors[1][channel] as f32 * t) as u8
        }))
    });
    for _ in 0..8 {
        let color = random.color();
        let (cx, cy) = (random.below(WIDTH), random.below(HEIGHT));
        let radius = 30 + random.below(120);
        let circle = random.below(2) == 0;
        for y in cy.saturating_sub(radius)..(cy + radius).min(HEIGHT) {
            for x in cx.saturating_sub(radius)..(cx + radius).min(WIDTH) {
                let (dx, dy) = (x.abs_diff(cx), y.abs_diff(cy));
                if !circle || dx * dx + dy * dy <= radius * radius {
                    image.put_pixel(x, y, Rgb(color));
                }
            }
        }
    }
    for pixel in image.pixels_mut() {
        let noise = random.below(13) as i16 - 6;
        pixel.0 = pixel
            .0
            .map(|value| (value as i16 + noise).clamp(0, 255) as u8);
    }
    image
}

fn scale(image: &RgbImage, factor: f32) -> RgbImage {
    let width = (image.width() as f32 * factor) as u32;
    let height = (image.height() as f32 * factor) as u32;
    imageops::resize(image, width, height, FilterType::Lanczos3)
}

fn crop(image: &RgbImage, horizontal: f32, vertical: f32) -> RgbImage {
    let (x, y) = (
        (image.width() as f32 * horizontal) as u32,
        (image.height() as f32 * vertical) as u32,
    );
    imageops::crop_imm(image, x, y, image.width() - 2 * x, image.height() - 2 * y).to_image()
}

fn crop_right(image: &RgbImage) -> RgbImage {
    let width = (image.width() as f32 * 0.7) as u32;
    imageops::crop_imm(image, 0, 0, width, image.height()).to_image()
}

fn jpeg_40(image: &RgbImage) -> RgbImage {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 40)
        .encode_image(image)
        .expect("encode JPEG");
    image::load_from_memory(&bytes)
        .expect("decode JPEG")
        .to_rgb8()
}

fn watermark(image: &RgbImage) -> RgbImage {
    let mut marked = image.clone();
    let (width, height) = (image.width() / 4, image.height() / 6);
    for y in image.height() - height - 10..image.height() - 10 {
        for x in image.width() - width - 10..image.width() - 10 {
            marked.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }
    marked
}

/// One bit per pixel of an 8×8 thumbnail: brighter than the thumbnail's mean.
fn average_hash(image: &GrayImage) -> u64 {
    let small = imageops::resize(image, 8, 8, FilterType::Triangle);
    let mean = small.pixels().map(|pixel| u32::from(pixel[0])).sum::<u32>() / 64;
    bits(small.pixels().map(|pixel| u32::from(pixel[0]) > mean))
}

/// One bit per horizontal neighbor pair in a 9×8 thumbnail: brightness falls to the right.
fn difference_hash(image: &GrayImage) -> u64 {
    let small = imageops::resize(image, 9, 8, FilterType::Triangle);
    bits((0..8).flat_map(|y| {
        let small = &small;
        (0..8).map(move |x| small.get_pixel(x, y)[0] > small.get_pixel(x + 1, y)[0])
    }))
}

/// The low-frequency 8×8 corner of a 32×32 DCT, compared with its median (pHash).
fn dct_hash(image: &GrayImage) -> u64 {
    const SIZE: usize = 32;
    let small = imageops::resize(image, SIZE as u32, SIZE as u32, FilterType::Triangle);
    let pixels: Vec<f64> = small.pixels().map(|pixel| f64::from(pixel[0])).collect();
    let cosine: Vec<f64> = (0..8 * SIZE)
        .map(|index| {
            let (frequency, position) = (index / SIZE, index % SIZE);
            (std::f64::consts::PI / SIZE as f64 * (position as f64 + 0.5) * frequency as f64).cos()
        })
        .collect();
    let mut coefficients = [0.0; 64];
    for v in 0..8 {
        for u in 0..8 {
            coefficients[v * 8 + u] = (0..SIZE * SIZE)
                .map(|index| {
                    let (y, x) = (index / SIZE, index % SIZE);
                    pixels[index] * cosine[u * SIZE + x] * cosine[v * SIZE + y]
                })
                .sum();
        }
    }
    // The DC term tracks overall brightness, so it is left out of the median.
    let mut sorted = coefficients[1..].to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    bits(coefficients.iter().map(|&coefficient| coefficient > median))
}

fn bits(values: impl Iterator<Item = bool>) -> u64 {
    values.fold(0, |hash, bit| hash << 1 | u64::from(bit))
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: u32) -> u32 {
        (self.next() % u64::from(limit)) as u32
    }

    fn color(&mut self) -> [u8; 3] {
        std::array::from_fn(|_| self.below(256) as u8)
    }
}
