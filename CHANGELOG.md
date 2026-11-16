# Changelog

## Unreleased

### Added

- Overview volume capacity and available-space snapshots, queried in the background with native paths. Unavailable statistics preserve completed scan results; cancelled scans skip the query. Values remain separate from logical folder sizes and reclaimable space.
- Unsigned native development ZIP generation for macOS arm64 and Windows x64, with package layout, architecture, and failure-preservation tests in CI.
- Observed empty-folder records, a paged Review tab, and explicit directory rows in CSV.
- Protected, high, and unknown risk labels with reasons in Review, Inspector, and CSV; every tier remains review-only.
- Incremental full SHA-256 caching for duplicate candidates of at least 64 KiB, validated against native file identity and metadata. Matches still undergo byte comparison.
- Cache hit/miss counters, database migrations v4/v5, upgrade and rollback tests, and a large-file cache benchmark.
- macOS CI and expanded Windows formatting, Clippy, and schema checks.

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
- CSV export of a saved scan, with formula-like cells defused for spreadsheets.
- A Scan again button in Scan History.
- File type and age filters and five sort orders in Scan Results.

### Fixed

- Duplicate candidates at most 16 KiB reuse their complete sample hash, avoiding a redundant read while retaining final byte comparison and identity checks. Zero-use cache counters no longer issue SQL updates.
- Large scans no longer block saved-history readers: the local index uses SQLite WAL.
- Image analysis and previews reject changed files and links, enforce decode limits, and honor cancellation. Review uses bounded thumbnails and reports preview failures.
- CSV export streams rows, uses unique temporary files, preserves unrelated `.part` files, and refuses the scan tree and app-data directory.
- Time filtering and sorting preserve nanoseconds; unknown modification times sort last.
- Cancelled scans retain analysis error counts. Review group buttons support keyboard focus.
- Background scan work and integrated tests now live separately from GPUI scheduling.

- Closing the window quits the app instead of leaving a windowless process running.
- A scan started while another app window is saving now explains why it could not be saved.
- Development builds verify duplicates at near-release speed.
- Scan History uses a sidebar icon included in the app’s default assets.

### Verified

- Current Apple Silicon pass: 63 Rust tests, formatting, strict Clippy, debug/release builds, and macOS archive contents. The preceding pass also passed 6 schema and 6 packaging tests. Packaged launch and Windows execution remain untested.
- A focused alternating-order benchmark confirms less work in small-file hashing; full-scan measurements remain variable and do not establish an end-to-end speedup.
- Three separately executed release benchmarks, including eight revalidated full-hash cache hits on a 256 MiB fixture. Current small-file timings and the earlier baseline are recorded separately in `docs/benchmarks.md`.

- Apple Silicon build and workspace tests.
- macOS app launch and scanning through the native folder picker.
- Saved scan history after restarting the macOS app.
- The macOS desktop smoke test on 120,023 generated files: picker and scan cancellation, paged search, Inspector navigation, exclusions, duplicate evidence, file-type totals, and reopened history.
- The Windows x64 build and workspace tests on GitHub Actions `windows-2022`.
- End-to-end release timings on 100,000 files: 3.2 s for mostly unique files and 8.1 s for duplicate-heavy files, with about 16.5 MiB peak memory. See [benchmarks](docs/benchmarks.md#end-to-end-benchmark).

### Needs testing

- The Windows 11 desktop smoke test and mounted-volume selection.
- The Review page's four tabs, the Scan Results filters, Scan again, and Export CSV in the running app.
- The similar-image threshold on real photos.
