# Development progress

## 2026-11-15

| Status | Work |
| --- | --- |
| Completed | Read the full product specification, assessed this Mac, reviewed the local Rox and Czkawka clones, and checked GPUI Kit's pinned runtime, setup, and component APIs. |
| Verified on Apple Silicon | Built the locked GPUI Kit app using Rust 1.100.0 and Xcode Command Line Tools. The workspace tests cover traversal, filesystem permissions, exclusions, symlinks, hard-link accounting, cancellation and largest-file selection. |
| Implemented in source | Native folder picker, read-only background scan, live progress and cancellation, a 100-file preview sorted by size, 1 GiB large-file findings and metadata in File Inspector. Example results are available before the first scan. |
| macOS smoke check passed | The user launched the app from Terminal and confirmed that the native picker scanned the selected folder. |
| Remaining interactive checks | Cancel the folder picker, cancel an active scan, and verify inspector navigation by mouse and keyboard on macOS. Build and exercise the workflow on Windows x64. |
| Planned for v0.1 | SQLite indexing and history, staged exact duplicate analysis with file-identity revalidation, configurable exclusions, a virtualized searchable results table, and platform benchmarks. |

## Next development session

1. Verify cancelled scans, folder picker cancellation, and inspector navigation with mouse and keyboard on macOS.
2. Replace the in-memory preview with a SQLite-backed scan index and a virtualized table. Keep the scan engine independent of GPUI and keep scan state and errors visible in the workspace.
3. Add staged hashing and file-identity revalidation, then run the fixture suite, smoke tests, and benchmarks on both target platforms.
