# Changelog

## Design study — 2026-11-18

- Added a self-contained website with five distinct interactive workspace layouts, simulated review/cleanup/recovery, comparison notes and saved favorites. Thirteen Node tests pass; real-browser visual acceptance and hosted publication are blocked by this environment. Native desktop UI is unchanged.

## 0.3.0 — development, 2026-11-18

### Added

- Persistent Cleanup Queue and Recovery History; explicit move and restore confirmations, kept-copy selection, current-exclusion checks and fresh identity/content validation.
- Independent cleanup-engine with macOS Trash and Windows Recycle Bin adapters; no permanent-delete fallback and no-overwrite restore.
- SQLite migration 006 with durable operation intent, native receipts, per-item outcomes, cancellation, interrupted-operation reconciliation and cross-process cleanup locking.
- Logical-size and allocated-size evidence kept separate from zero expected immediate reclamation and observed volume-space changes. Protected paths, hard links and detected cloud placeholders are blocked.
- Disposable-fixture coverage for changed items, keeper preservation, cancellation, restore conflicts, interruptions, exclusions and journal-write failures.

Verified on Apple Silicon: 98 Rust, 8 schema and 14 packaging tests; formatting, strict Clippy, debug/release builds and the v0.3.0 archive verifier pass. The native no-overwrite restore primitive passes disposable-file tests. Native acceptance is pending: the macOS Trash fixture was blocked by filesystem permissions; Windows build/runtime and current interactive flows are unverified. See docs/progress.md.

## Earlier development work


### Added

- Review-only app cache evidence for default Chrome and Chromium HTTP cache locations on macOS and Windows. Review, Inspector, and CSV share the rules; matching locations stay protected, with sibling profile data excluded from the cache policy.
- Development package manifests with SHA-256 payload checksums and a standalone archive verifier. Packages are checked for layout, native architecture, launch metadata, and executable permissions before replacing an earlier build. CI now builds and uploads unsigned ZIP artifacts after its checks pass.
- Per-scan phase diagnostics and a reproducible 20,000-file same-size candidate benchmark, with regression coverage for corrupt storage, disappearing files, failed duplicate-group writes, and failed scan finalization.
- A saved Overview dashboard with the latest scan's findings and storage categories, up to five recent scans, and Open results, Review findings, and Scan again actions. Loads after restart and refreshes after scans without blocking GPUI.
- File Inspector now shows extension-based type and review-only cleanup evidence using the same saved observation time as Review and CSV. Incomplete analysis and partial indices remain explicit on the dashboard.
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

- Overview's last-scan button now opens the scan described by its card, even after another history entry was opened.
- CSV export allows only one picker/write operation at a time across scan navigation, and releases its busy state on cancellation, success, or failure.
- Age filters use the saved scan timestamp with checked arithmetic, preserving pre-epoch boundaries and reporting invalid dates instead of panicking or silently changing the cutoff.
- Review keeps partial-analysis and skipped-check notices visible when groups exist, uses precise empty-state messages, and provides Refresh to retry loading groups and previews.
- Scans stop retaining image candidates when database saving is unavailable, and release queued image records after an indexing write failure.
- Shared saved-scan time conversion preserves milliseconds before the Unix epoch instead of silently clamping cleanup evidence to 1970.
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

- Current Apple Silicon pass: 74 Rust tests, formatting, strict Clippy, debug/release builds, and macOS archive contents. Earlier passes also passed 6 schema and 6 packaging tests. Packaged launch and Windows execution remain untested.
- November 17 phase profiling: 100,000-file scans took 3.61 / 3.62 s for mostly unique files and 7.76 / 7.93 s for duplicate-heavy files. A 20,000-file candidate set passed three scans at 1.04–1.08 s; earlier timing variation is not considered resolved.
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
