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
| Remaining interactive checks | The desktop automation rejected the unpackaged macOS executable. Full-scan search, mouse and keyboard navigation, and picker and scan cancellation still need an interactive check. Build and run the desktop workflow on Windows 11 x64. |

## Next development session

Follow the [v0.1 release gates](tasks.md#close-before-v01). The next implementation task is the saved scan’s file-type breakdown. Windows x64 build verification can proceed independently. Run final platform smoke tests and the end-to-end benchmark against the integrated build.
