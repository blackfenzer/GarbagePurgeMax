# Changelog

## Footer stats and nested treemap — 2026-11-22

- Treemap boxes are rounded and float on the page with soft shadows and a fixed colour gradient; a folder's sub-boxes float together in one rounded holder in the app background colour instead of having borders.
- Each theme has its own file-type palette: Ember for Amber Deck, Slate jewel for Workbench.
- Treemap boxes label themselves as far as they fit: name and size, name, icon, then nothing; sub-boxes are named too.
- The theme switch is a swatch button with a theme menu, the year graph shows only active years, and the footer's progress bar shows the open folder's share of the scan.
- Amber Deck's footer adds a transport row (size up to and after the selection, cleanup queue, disk use) and a file-types chart; its graphs scale when the footer is resized.
- Space Map treemap blocks for the largest folders show their contents as sub-boxes shaded by size.
- Scan Results no longer shows sample files before the first scan.

## Move to Trash and Review redesign — 2026-11-22

- Files rows have Details and Move to Trash buttons; in Amber Deck clicking a row opens a details dialog.
- Move to Trash from Files, Tree rows and the Space Map inspector, in both themes. Folders show what is inside before anything moves, and the move is refused if their contents change first. Every move is journaled and restorable from Recovery History.
- Review shows each group as aligned rows with full paths, size, modified date and a coloured risk chip; Empty folders is a single list. Queueing no longer leaves Review.
- Amber Deck panels have a gap between them, and the Files panel no longer clips its toolbar or table.

## Amber Deck design review — 2026-11-22

- Added colour file-type icons from Material Icon Theme (MIT) to Space Map and the Results file list.
- Risk evidence now recognises OS-generated files: `.localized`, `desktop.ini`, Spotlight and Time Machine data, paging files and registry hives are Protected; `.DS_Store`, AppleDouble `._` files and thumbnail caches are a new Low-risk tier and cleanup candidates.
- Fixed buttons that open another page doing nothing in Amber Deck.
- Clearer active tabs and selected buttons, stronger borders, icons throughout, `›` breadcrumbs in Space Map, and wrapping long paths.

## Smart scope and background analysis — 2026-11-21

- Smart scope also skips web, mobile, Ruby, Elixir, Haskell, Zig, CMake and game-engine (Unity, Unreal, Godot) generated folders; analysis never reads online-only cloud files, disk images, device backups, Photos libraries, Wine prefixes or more game data.
- Added a Regenerable developer caches card (npm, Cargo, Gradle, Maven, pip, Go, Homebrew, Xcode, Android and more).
- Added smart scan scope: broad scans skip project dependencies, build output and Git object stores; a chosen Git project scans in full; game libraries, app bundles and system folders are sized but not analysed for duplicates.
- Duplicate and similar-image analysis now runs after the index is saved, with live progress, cancel, run again, a configurable image-worker count and an auto-analyze switch.
- Rebuilt Overview as a bento dashboard with charts and stat tiles.
- Amber Deck panels and footer are resizable.

## Space Map and themes — 2026-11-21

- Added Space Map: a saved scan's folders by logical size with % of folder, file counts, main file type, a squarified treemap, breadcrumbs and an inspector. It reads saved paths only and moves nothing.
- Added the Amber Deck layout: a top scan bar with Choose folder / Cancel, Scan again and the theme switch; a left panel (Map, Overview, Review) and a right panel (Files, Tree, Queue, Recovery, History); and a footer with size by last-modified year, a clickable strip of the current folder's entries, a small treemap and a caption.
- Added a sidebar theme switch between Workbench (dark slate and lavender) and Amber Deck (near-black and amber). The choice is saved next to the scan database.
- Added an off-screen UI smoke test with screenshots, and `STORAGE_CLINIC_DATA_DIR` to keep app data in another folder.
- Space Map totals subfolders from a path-key range: 19 ms instead of 198 ms for a top-level folder of a 1M-file scan.
- Fixed Amber Deck's collapsed Map treemap, oversized footer and year bars, and the chip's plural.
- Fixed two Windows-only Clippy errors that had failed Windows CI since `7c4352d`, and added `.github/scripts/clippy-windows.sh` to lint Windows code from macOS.

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
