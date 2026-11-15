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

### Fixed

- Scan History uses a sidebar icon included in the app’s default assets.

### Verified

- Apple Silicon build and workspace tests.
- macOS app launch and scanning through the native folder picker.
- Saved scan history after restarting the macOS app.
