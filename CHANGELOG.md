# Changelog

## Unreleased

### Added

- Native GPUI Kit desktop workspace with Overview, Scan Results, and File Inspector.
- Read-only folder scans with progress, cancellation, and filesystem error counts.
- Preview of the 100 largest files, with metadata and an explanation for files of at least 1 GiB.
- Independent scanner crate and fixtures covering exclusions, permissions, symlinks, hard links, and cancellation.

### Verified

- Apple Silicon build and workspace tests.
- macOS app launch and scanning through the native folder picker.
