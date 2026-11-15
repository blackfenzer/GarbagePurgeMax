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
| Remaining interactive checks | Exercise search and keyboard navigation, and picker and scan cancellation. Build and exercise the workflow on Windows x64. |
| Planned for v0.1 | Staged exact duplicate analysis with file-identity revalidation, configurable exclusions, and platform benchmarks. |

## Next development session

1. Verify full-scan search, cancellation and keyboard navigation on macOS.
2. Add staged hashing with file-identity revalidation and surface the evidence in File Inspector.
3. Run the fixture suite, smoke tests, and benchmarks on both target platforms.
