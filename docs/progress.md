# Development progress

## 2026-11-15

| Status | Work |
| --- | --- |
| Completed | Read the full product specification, assessed this Mac, reviewed the local Rox and Czkawka clones, and checked GPUI Kit's pinned runtime, setup, and component APIs. |
| Verified on Apple Silicon | Built the locked GPUI Kit app using Rust 1.100.0 and Xcode Command Line Tools. The workspace tests cover traversal, filesystem permissions, exclusions, symlinks, hard-link accounting, cancellation and largest-file selection. |
| Implemented in source | Native folder picker, read-only background scan, live progress and cancellation, a 100-file preview sorted by size, 1 GiB large-file findings and metadata in File Inspector. Example results are available before the first scan. |
| macOS smoke check passed | The user launched the app from Terminal and confirmed that the native picker scanned the selected folder. |
| SQLite persistence verified | Cached and built bundled `rusqlite`. The workspace tests reopen a database, search for a file outside the preview, recover cancelled scan metadata, and confirm rollback when a writer is dropped. |
| Results and History views implemented | The virtualized table presents up to 100 rows per page. Search covers every indexed file in a saved scan. Scan History opens past scans, including cancelled ones. An unsaved scan shows an error and a temporary preview. |
| Scan History smoke check passed | The user confirmed the saved scan reopened after restarting the macOS app. The History sidebar now uses GPUI Kit’s bundled calendar icon to resolve the reported missing SVG. |
| Scan exclusions and exact duplicates | Added persistent path exclusions, staged SHA-256 and byte comparison, migration of existing history, and duplicate evidence in Scan Results and File Inspector. The Apple Silicon build, strict Clippy, 30 Rust tests, and 4 SQLite schema tests passed. |
| Traversal benchmark | The release scan engine averaged 267,804 files/s with 2,272 KiB peak worker RSS and 6.5 µs median cancellation latency on the synthetic 100,000-file fixture. See [method and results](benchmarks.md). |
| Windows CI ready | Added a Windows x64 MSVC workflow that builds the desktop executable and runs the workspace tests. Native Windows execution is pending. |
| File-type breakdown | Saved scans show counts and logical bytes per type, from file extensions, with an Other fallback. Totals load off the UI thread and cover partial and reopened scans. On Apple Silicon, rustfmt, strict Clippy, 32 Rust tests, and the app build passed. Not yet checked in the running app. |
| Smoke fixture | Added `cargo run --locked --release -p scan-engine --example smoke_fixture -- <new folder>`. It writes 120,023 readable files: 120,000 searchable `report-*` files in six types, small duplicate and same-size groups, six sparse 2 GiB identical `.dmg` files for large-file findings and cancellable verification, an `exclude-me` folder, and an unreadable `locked` folder on Unix. Generated on macOS in 6.6 s, 60 KiB on disk. Run `chmod 755 <folder>/locked` before deleting it. |
| macOS smoke, first findings | The user scanned the smoke fixture: 120,023 files, 18.7 GiB logical size, 6 large files, and the expected permission error for `locked`. A second app instance then reported `database is locked` because each scan holds the write lock until it finishes. The message now explains that another window is saving a scan. Duplicate checks looked stalled in `cargo run`: debug SHA-256 verified 2 GiB at 119 MiB/s. Building `sha2` at opt-level 3 in dev raised it to 1,426 MiB/s (release 1,691 MiB/s). Rustfmt, strict Clippy, 33 Rust tests, and the app build passed. The rest of the smoke checklist is still pending. |
| Remaining interactive checks | The desktop automation rejected the unpackaged macOS executable. Full-scan search, mouse and keyboard navigation, and picker and scan cancellation still need an interactive check. Build and run the desktop workflow on Windows 11 x64. |

## Next development session

Follow the [v0.1 release gates](tasks.md#close-before-v01). The next task is the macOS desktop smoke test, including the new file-type line in Scan Results. Windows x64 build verification can proceed independently. Run final platform smoke tests and the end-to-end benchmark against the integrated build.
