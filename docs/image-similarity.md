# Image similarity evaluation

This evaluation chooses a perceptual hash for the v0.2 Similar Images view. Reproduce it from the repository root:

```sh
cargo run --locked --release -p scan-engine --example image_similarity
```

## Method

The example generates 16 synthetic 640 × 480 scenes from a fixed seed. Each scene has a gradient background, eight overlapping circles or rectangles, and light pixel noise. It makes 12 edited copies of each scene. Ten edits are in scope for a "similar image" finding: three resizes, three crops, brightness, contrast, JPEG quality 40, and a white watermark block. Mirroring and 90° rotation are reported as out of scope.

Each image is converted to grayscale and hashed to 64 bits three ways:

- **Average hash:** an 8 × 8 thumbnail, with each pixel compared to the thumbnail mean.
- **Difference hash:** a 9 × 8 thumbnail, with each pixel compared to its right-hand neighbor.
- **DCT hash (pHash):** the low-frequency 8 × 8 corner of a 32 × 32 discrete cosine transform, compared to its median, excluding the DC term.

Similarity is the Hamming distance between two hashes. Matches compare each original with its own edits. False matches are any of the 20,280 pairs from different scenes, including edited copies. Hash time covers grayscale resize and hashing, not file decoding.

## Results

Measured on Apple Silicon with macOS 27.2 and Rust 1.100.0. Each cell gives the largest distance from the original, then how many of the 16 copies matched at that hash's strictest threshold with no false matches. The thresholds are ≤ 6 for average, ≤ 7 for difference, and ≤ 13 for DCT.

| Edit | Average | Difference | DCT |
| --- | ---: | ---: | ---: |
| Resize to 50% | 0 · 16/16 | 1 · 16/16 | 0 · 16/16 |
| Resize to 25% | 0 · 16/16 | 1 · 16/16 | 0 · 16/16 |
| Upscale to 150% | 0 · 16/16 | 1 · 16/16 | 0 · 16/16 |
| Crop 5% per side | 8 · 14/16 | 9 · 12/16 | 18 · 12/16 |
| Crop 15% per side | 23 · 1/16 | 26 · 0/16 | 38 · 0/16 |
| Crop 30% from the right | 27 · 1/16 | 35 · 0/16 | 34 · 0/16 |
| Brighten by 30 | 3 · 16/16 | 3 · 16/16 | 4 · 16/16 |
| Contrast +30% | 8 · 15/16 | 7 · 16/16 | 8 · 16/16 |
| JPEG quality 40 | 1 · 16/16 | 6 · 16/16 | 2 · 16/16 |
| Watermark block | 17 · 8/16 | 6 · 16/16 | 30 · 10/16 |
| Mirror horizontally (out of scope) | 46 · 1/16 | 47 · 0/16 | 36 · 0/16 |
| Rotate 90° (out of scope) | 42 · 0/16 | 43 · 0/16 | 40 · 0/16 |

Threshold sweep, showing in-scope copies matched of 160 and false matches of 20,280:

| Threshold | Average | Difference | DCT |
| ---: | ---: | ---: | ---: |
| ≤ 4 | 111 · 0 | 110 · 0 | 93 · 0 |
| ≤ 6 | 119 · 0 | 122 · 0 | 100 · 0 |
| ≤ 8 | 128 · 15 | 126 · 1 | 108 · 0 |
| ≤ 10 | 132 · 61 | 128 · 1 | 112 · 0 |
| ≤ 12 | 139 · 146 | 130 · 25 | 118 · 0 |
| ≤ 16 | 149 · 412 | 134 · 223 | 124 · 9 |

Mean hash time was 275 µs for average, 271 µs for difference, and 306 µs for DCT. Decoding a full-size photo costs far more than any of these.

## Decision

Use the **difference hash** with a starting threshold of **≤ 6**. It matched 122 of 160 in-scope copies with no false matches: every resize, brightness, contrast, JPEG, and watermark copy, and most 5% crops. It handled the watermark best and is the simplest to implement.

- All three hashes are global, so crops beyond about 5%, mirroring, and rotation are not found. Matching those needs feature-based methods. Leave them out of v0.2 rather than raise the threshold, which adds false matches quickly.
- The DCT hash keeps a wider margin against false matches, with none until ≤ 12, but it found fewer copies at every threshold. Revisit it if real photos show false matches with the difference hash.
- The scenes are synthetic. Before release, confirm the threshold on a local folder of real photos and their edited copies. Real photo sets can include near-identical burst shots that are different photos.

## Keeping similarity separate from exact matches

When the view is built, store similarity evidence apart from exact duplicates and label it clearly.

- Save hashes and candidate pairs in their own tables, with the hash algorithm, threshold, and distance. Do not write them to `duplicate_groups`.
- Label findings "Similar image", with the distance as evidence. Keep "Exact duplicate" for byte-verified groups only.
- Similar images are review-only. Cleanup must never treat them as copies of each other.
- Decoding needs the `image` crate as a normal dependency of the scanner, with only the formats the app supports. Today it is a dev-dependency used by this evaluation.
