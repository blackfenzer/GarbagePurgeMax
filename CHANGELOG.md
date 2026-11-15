# Changelog

## Unreleased

### Added

- Native GPUI Kit desktop workspace with Overview, Scan Results, and File Inspector.
- Read-only folder scans with progress, cancellation, and filesystem error counts.
- File metadata and an explanation for files of at least 1 GiB.
- Independent scanner crate and fixtures covering exclusions, permissions, symlinks, hard links, and cancellation.
- A local SQLite index with scan history, paginated file-path search, and a virtualized results table with keyboard row navigation.
- A 100-file preview with an error message if a scan cannot be saved.
- Per-file and per-folder exclusions that persist across app restarts.
- Exact duplicate findings with a duplicate-only view, verified group size, and SHA-256 evidence in File Inspector.
- A reproducible 100,000-file traversal benchmark and a Windows x64 build and test workflow.
- A file-type breakdown with counts and logical size for saved scans, labelled as indexed totals for cancelled scans.
- A generated smoke-test fixture for the desktop workflow.
- An end-to-end benchmark of scanning, indexing, duplicate verification, review queries, and cancellation on 100,000 files.
- Similar-image detection for JPEG, PNG, GIF, WebP, BMP, and TIFF files using a difference hash, stored apart from exact duplicates.
- A Review page with Exact duplicates and Similar images tabs, side-by-side previews, and the evidence for each match.
- Review-only cleanup candidates for empty files, temporary files, incomplete downloads, installers, archives, and old downloads, each with its evidence.

### Fixed

- Closing the window quits the app instead of leaving a windowless process running.
- A scan started while another app window is saving now explains why it could not be saved.
- Development builds verify duplicates at near-release speed.
- Scan History uses a sidebar icon included in the app’s default assets.

### Verified

- Apple Silicon build and workspace tests.
- macOS app launch and scanning through the native folder picker.
- Saved scan history after restarting the macOS app.
- The macOS desktop smoke test on 120,023 generated files: picker and scan cancellation, paged search, Inspector navigation, exclusions, duplicate evidence, file-type totals, and reopened history.
- The Windows x64 build and workspace tests on GitHub Actions `windows-2022`.
- End-to-end release timings on 100,000 files: 3.2 s for mostly unique files and 8.1 s for duplicate-heavy files, with about 16.5 MiB peak memory. See [benchmarks](docs/benchmarks.md#end-to-end-benchmark).

### Needs testing

- The Windows 11 desktop smoke test and mounted-volume selection.
- The Review page's three tabs in the running app.
- The similar-image threshold on real photos.
