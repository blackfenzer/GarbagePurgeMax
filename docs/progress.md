# Development progress

## 2026-11-16 · Read-only completion and hardening

| Area | Implemented and verified |
| --- | --- |
| Empty folders | Scanner directory observations, migration v4, paged Review view, and CSV directory rows. Fixtures cover hidden/excluded contents, unreadable folders, symlinks, cancellation, partial/reopened scans, and separation from file totals. |
| Risk evidence | Protected/high/unknown review-only tiers in Review, Inspector, and CSV. Rules conservatively flag known system, app, cloud-managed, personal, and development paths. They are not complete OS/cloud-provider detection. |
| Incremental hashes | Migration v5 caches full SHA-256 hashes for candidates at least 64 KiB. Native identity, size, modification/creation/change times and algorithm must match; every duplicate still undergoes byte comparison. Tests cover restored timestamps, replacement, hard links, collisions, rollback, and reopening. |
| Reader availability | WAL allows completed history and file pages to remain readable while a 10,000-row scan holds a write transaction. This regression test failed under the old journal mode and passes with WAL. A second writer still gets a clear contention error. |
| Bounded images and review | At most two decode workers; encoded-size, dimension, and decoder-allocation limits; cancellation-aware reads; shared native version checks. Review retains at most 200 members/candidates per group and supplies 320×220 PNG thumbnails instead of full-resolution file paths. Preview errors remain visible. |
| Export and dates | Streaming CSV with risk and entry-kind evidence, unique temporary files, atomic replacement, scan-tree/app-data guards, and formula defusing. Tested failed exports, existing neighbors, Unicode names, symlink parents, and database protection. Date filters and sorting preserve nanoseconds and put unknown dates last. |
| Maintainability | GPUI scheduling stays in `scan.rs`; blocking pipeline and integrated tests live in `scan/job.rs`. Shared file-version checks now serve duplicates, images, and previews. Cache storage remains in the store crate. No new dependency was introduced. |
| Automation | Added macOS Apple Silicon CI and expanded Windows CI with rustfmt, strict Clippy, and schema tests. These workflow changes have not been run on GitHub in this session. |

### Automated checks on Apple Silicon, macOS 27.2

- `cargo fmt --all -- --check`: passed.
- `cargo test --locked --workspace`: **57 passed**, zero failures; three benchmarks intentionally ignored by the default test invocation.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo build --locked -p storage-clinic`: passed.
- `cargo build --locked --release -p storage-clinic`: passed; the same local symbol-stripping warning occurred. The release executable is `target/release/storage-clinic`.
- `python3 -B crates/scan-store/tests/test_schema.py`: **6 passed**.
- `cargo test --locked --release -p storage-clinic --no-run`: built successfully, with a local `rust-objcopy` missing-`libLLVM.dylib` stripping warning.

### Benchmarks, separately executed

All three ignored release benchmarks passed individually. Final 100,000-file totals: mostly unique 4.15 s first / 3.87 s repeat; duplicate-heavy 12.33 s first / 16.32 s repeat; peak process RSS approximately 17.3 MiB. Cancellation including commit returned within 85 ms in these runs. The eight-file, 256 MiB duplicate fixture improved from 0.167 s to 0.035 s with eight revalidated cache hits. See [benchmark methodology, CPU time, and limitations](benchmarks.md#integrated-hardening-pass--november-16-2026). Small-file timings are slower than the earlier recorded baseline; no broad speedup is claimed.

### Interactive and platform checks not completed

Desktop automation could not select the unbundled executable, and the Mac was locked. A direct development launch reached GPUI but failed in macOS pasteboard initialization (`NSPasteboard generalPasteboard` returned NULL). This is a blocked interactive test, not a passed launch or evidence that the new views work visually. No screenshots or demo were captured.

Skipped in this pass: Windows execution/Windows 11 UI smoke, mounted-volume selection, current macOS UI smoke, real-photo threshold calibration, full accessibility audit, cold-cache/device-I/O measurements, and signed distribution. Native invalid-UTF-8 destination creation was not supported in this Mac session; the filesystem export test uses Unicode here and reserves invalid-byte filenames for Linux. Native path serialization remains covered separately.

The v0.3 mutation/recovery features and v1.0 distribution work are **not implemented** by this pass. The application remains read-only as required by the current release scope. Named profiles and deeper archive/customization work remain conditional on user evidence.

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
| macOS smoke, first findings | The user scanned the smoke fixture: 120,023 files, 18.7 GiB logical size, 6 large files, and the expected permission error for `locked`. A second app instance then reported `database is locked` because each scan holds the write lock until it finishes. The message now explains that another window is saving a scan. Duplicate checks looked stalled in `cargo run`: debug SHA-256 verified 2 GiB at 119 MiB/s. Building `sha2` at opt-level 3 in dev raised it to 1,426 MiB/s (release 1,691 MiB/s). Rustfmt, strict Clippy, 33 Rust tests, and the app build passed. Closing the window left the process running, so relaunches added instances; the app now quits when its window closes. Automation could not click the close button (no assistive access), so that fix awaits a manual check. The rest of the smoke checklist is still pending. |
| macOS smoke test passed | The user ran the debug build on Apple Silicon against the smoke fixture. Confirmed: 120,023 files, 6 large files, the `locked` permission error, 5 verified duplicate groups (including the 10 identical `exclude-me` files), file-type totals summing to every file, paged search for `report-099`, Inspector duplicate evidence with mouse and keyboard, Duplicates only, cancellation during traversal and duplicate verification, folder exclusion, scan history after restart, and quit on window close. Mounted-volume selection was not exercised. |
| Windows x64 CI passed | GitHub Actions run 43706980591 on `windows-2022` passed the locked workspace tests and built `storage-clinic` for `x86_64-pc-windows-msvc` at commit a301c09. The Windows 11 desktop smoke test is still pending. |
| End-to-end benchmark | The release pipeline scanned and saved 100,000 files in 3.2 s for mostly unique files and 8.1 s for the duplicate-heavy input, with 2,000 verified groups. Peak process RSS was about 16.5 MiB, review queries took under 47 ms, and cancellation returned within 14 ms. See [benchmarks](benchmarks.md#end-to-end-benchmark). Cold-cache timing and frame-level responsiveness were not measured. |
| Image similarity evaluated | On 16 synthetic scenes with 12 edits each, a 64-bit difference hash at distance ≤ 6 matched 122 of 160 in-scope copies with no false matches across 20,280 pairs from different scenes. It matched every resize, brightness, contrast, JPEG, and watermark copy, but no crops of 15% or more. See [image similarity](image-similarity.md). The threshold still needs confirming on real photos. |
| Review views implemented | Scans now hash supported images on every core after duplicate verification and save similar-image groups through migration v3. A new Review page lists exact and similar groups in separate tabs with side-by-side previews and evidence. On Apple Silicon, rustfmt, strict Clippy, 37 Rust tests, 5 schema tests, and the app build passed. On the updated smoke fixture's `images/` folder, the engine found the three sunrise variants and the sunset pair, and counted one unreadable image. The Review page has not been checked interactively. |
| Cleanup candidates implemented | The engine matches saved file metadata against six review-only rules and explains each match. Review has a Cleanup candidates tab that groups them by rule and shows up to 200 cards per group. The smoke fixture now includes a temporary file, an Office lock file, an incomplete download, and old and recent files in `Downloads`. On Apple Silicon, rustfmt, strict Clippy, 39 Rust tests, the schema tests, and the app build passed. The tab has not been checked interactively, and empty folders are not detected yet. |
| Export, Scan again, filters, and sorting | Scan Results exports a saved scan to CSV, filters by type and age, and sorts five ways. Scan History repeats a scan. A security review found that leading whitespace could slip a formula past the CSV guard, so that was fixed. Adding sort expressions first slowed the first page from 0.2 ms to 55 ms. Constant ORDER BY clauses restored it: 0.2 ms first page, 31.5 ms last page, and about 46 ms search on 100,000 files. On Apple Silicon, rustfmt, strict Clippy, 41 Rust tests, the schema tests, and the app build passed. Not yet checked interactively. |
| Remaining interactive checks | Select a mounted volume through the picker on macOS. Build and run the desktop workflow on Windows 11 x64. |

## Next development session

Use [the release checklist](release.md). Run the integrated desktop on an unlocked Mac, then the updated Windows CI and Windows 11 smoke test. Check all four Review tabs, thumbnail failures, empty-folder paging, filters, Scan again, and CSV. Confirm a mounted volume and the similarity threshold on real photos. Investigate the slower duplicate-heavy benchmark before claiming performance improvements. Capture screenshots/demo only after actual interactive verification. Keep destructive cleanup and signed distribution as the later milestones recorded in [tasks](tasks.md).
