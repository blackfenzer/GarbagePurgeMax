# Development progress

## 2026-11-21 · Space Map and two themes

Added a **Space Map** page and a saved **Workbench / Amber Deck** theme switch, following the canvas design. `ScanStore::folder_summary` totals a folder's direct children from saved native paths (bytes, files, main type) plus the folder's type breakdown; like `file_types` it reads every file row per call, marked with a ponytail note for per-folder totals saved at scan time if large scans need it. The page shows a WizTree-style list (up to 300 rows) with % of folder bars, a squarified treemap (up to 60 blocks), breadcrumbs, Up one folder, and an inspector. Both themes are GPUI Kit dark theme configs in `apps/desktop/themes/storage-clinic.json`; the choice is saved as a `theme` file beside `scans.sqlite3`. Amber Deck currently changes colours only; its rox-style layout and footer graphs are the next task.

| Check (macOS 27.2, Rust 1.100.0) | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --locked --workspace` | **100 passed**, 0 failed, 6 ignored (new: folder totals per level, treemap tiling) |
| `sh .github/scripts/clippy-windows.sh` | Passed |
| `cargo run --locked -p storage-clinic` | Launched; no theme load errors logged |

**Not verified:** visual review of both themes and the Space Map page (no screen capture available to the agent), Windows behaviour, and folder totals speed on million-file scans. Windows CI's workspace tests failed on `0a202d2` before this work; the failing test is unknown because job logs need a GitHub login.

## 2026-11-21 · Windows CI lint repair

Windows CI had failed at Clippy since `7c4352d`, so its schema, package, test and build steps were skipped for three pushes. Two Windows-only lints were the cause: `chunks_exact(2)` in scan-store's Windows path decoding (now `as_chunks::<2>()`), and a needless `&format!` in the Windows Recycle Bin restore error. `.github/scripts/clippy-windows.sh` now reproduces the CI lint from macOS using placeholder C and resource compilers; Clippy does not link, so this checks Windows code paths without a Windows machine but proves nothing about Windows builds or behavior.

| Check (macOS 27.2, Rust 1.100.0) | Result |
| --- | --- |
| `sh .github/scripts/clippy-windows.sh` | Passed (was 2 errors) |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --locked --workspace` | **98 passed**, 0 failed, 6 ignored |

**Pending:** a Windows CI run on the pushed fix, which will also run Windows tests and packaging for the first time since v0.2.

## 2026-11-18 · Five-layout website study

Added `design-lab/` as a separate, self-contained interactive website for later design review. Workbench starts from the user's preferred dense rox analysis pattern; Harbor, Atlas, Focus and Ledger change the layout structure as well as palette. The study reviewed local rox screenshots, GPUI's official showcase and Krokiet screenshot previews. Sources, tokens, tradeoffs and suggested native mappings are recorded in `design-lab/DESIGN.md`.

The website supports sample-file search/filter/sort, inspection, kept-copy selection, queue confirmation, simulated movement and restore, cancellable sample scans, layout comparison and locally saved favorites. It does not access the filesystem or alter the Rust desktop UI. `design-lab/dist/index.html` opens directly without a server or external dependencies.

**13 Node tests passed**: domain invariants, self-contained build, all five layout renderers and all 25 navigation combinations, confirmation/restore/reset events, cancellation and references/favorites. Tests execute templates/events with a minimal DOM boundary; this is not real browser acceptance. Preview socket binding was denied, the browser tool's profile was already in use, and a separate headless Chrome launch failed. Visual/mobile/keyboard/screen-reader acceptance remains pending.

Private Site registration succeeded, but source synchronization failed with DNS resolution for `git.chatgpt-team.site`. The Site identity is retained in the prototype's `.openai/hosting.json` for a future retry. No live deployment or screenshot is claimed. The usable deliverable is the portable HTML website in the repository.



## 2026-11-18 · v0.3 reviewed cleanup and recovery

Implemented the full v0.3 feature scope in the 0.3.0 development build: persistent Cleanup Queue and paged Recovery History, selected kept copies for exact duplicates, separate move/restore confirmations, current-exclusion checks, fresh native identity/version/hash and byte validation, native macOS Trash and Windows Recycle Bin adapters, no-overwrite restore, cancellation and per-item outcomes. Scans remain read-only. Protected locations, linked paths, hard links and detected cloud placeholders are blocked. Known protected browser caches remain ineligible.

A separate `cleanup-engine` crate owns filesystem evidence and native adapters without GPUI/SQLite. The desktop coordinates background work; scan-store owns migration 006 and the durable state machine. OS locking serializes cleanup across processes; synchronous FULL persists intent before native actions and receipts before verification. Interrupted moving/restoring items become Needs inspection and never retry automatically. Recovery records outlive scan removal. Read-only reconciliation verifies a manually restored original before releasing kept-copy reservations. SQLite write failures stop filesystem actions or stop further batch work as appropriate.

Logical size, potential allocated-size upper bounds, zero expected immediate release and observed volume available-space deltas are distinct. Shared allocation, snapshots, sparse/compressed files and cloud storage remain uncertain; there is no permanent deletion or empty-Trash operation. Folder picker, scans and exclusion edits cannot overlap mutation controls in one workspace. See [the recovery contract](cleanup-recovery.md) for race and durability limits.

Verified on **macOS 27.2 / Apple Silicon, Rust 1.100.0**:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo test --locked --workspace` | **98 passed**, 0 failed; 6 ignored (5 opt-in benchmarks and 1 native Trash test) |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo build --locked -p storage-clinic` | Passed |
| `python3 -B crates/scan-store/tests/test_schema.py` | **8 passed**, including v5→v6 history preservation and cleanup constraints |
| `python3 -B .github/scripts/test_package.py` | **14 passed** |
| Release build and macOS ZIP generation | Passed; `target/packages/Storage-Clinic-0.3.0-macos-arm64.zip` |
| Standalone archive verification, expected version 0.3.0 / macos-arm64 | Passed |
| Native macOS no-overwrite restore primitive, disposable directory | Passed, including direct native collision rejection and successful verified rename |
| Opt-in real macOS Trash round trip | **Attempted, blocked**: Foundation reported permission denied moving the generated fixture; no receipt or successful round trip |

The release toolchain still reports the existing `rust-objcopy` missing `@rpath/libLLVM.dylib` warning during debug-symbol stripping. Cargo returns success and the archive's payload, architecture, manifest and launch metadata verify. Signing/notarization and packaged launch remain unverified.

New tests cover persistence, distinct keeper reservations across trashed history, source/keeper changes, late folder entries, symlink replacement, read-only/hard-linked sources, sparse allocation evidence, exact confirmation membership, cancellation, partial native failure, missing/changed receipts, lock contention, abandoned intents, occupied restore destinations, exclusions added after queueing, malformed exclusion settings, and injected intent/receipt write failures. Ordinary pipeline tests use an injected adapter that moves only disposable fixture files; they do not establish OS Trash acceptance. No new benchmark result is claimed.

**Deferred native checks:** only the Apple Silicon Rust target is installed. Windows compilation, Windows 11 Recycle Bin/restore behavior, disabled/unavailable recycling, native crash/restart, mounted volumes and the complete GUI flow still require acceptance testing. The current tools do not provide working desktop capture; the previous failure was ScreenCaptureKit -3811. The platform checklist remains open in [tasks](tasks.md) and [release](release.md). The source implementation is complete; cross-platform release acceptance is not.

## 2026-11-17 · Curated read-only browser cache evidence

Implemented the initial app cache policy task for Google Chrome stable and Chromium HTTP caches. The engine matches only documented default native layouts under macOS `/Users/<account>/Library/Caches` or Windows drive-rooted `Users/<account>/AppData/Local`; custom roots, other browsers/channels, UNC paths, and sibling profile data are excluded. Known path literals are compared without converting account, profile, or filename identity to lossy text. Parent traversal is rejected. Policy sources and exact limits are recorded in [cache policies](cache-policies.md).

Review, Inspector, and CSV use the same existing cleanup-candidate API, with App caches appended to preserve earlier group identifiers. A cache match supplies explicit metadata-only evidence and Protected risk; no browser contents, process state, deletion, or reclaimable-space claims are involved. Empty-file precedence remains unchanged. Saved scans gain the classification through metadata queries without a schema migration or reopening source files.

On Apple Silicon/macOS 27.2, **78 Rust tests passed**, with five benchmark tests ignored by the normal invocation. New tests cover both platform layouts, rejected siblings and channels, absolute/native paths, parent traversal, non-UTF-8 names and empty-file precedence, and a reopened synthetic SQLite scan whose Review candidates and CSV agree. The added native-name assertions also passed individually. Rustfmt, strict Clippy, and debug/release builds passed. The rebuilt macOS ZIP passed the new archive verifier. The preceding **14 packaging** and **6 schema** checks remain applicable; no package or SQL implementation changed in this batch. The known local symbol-stripping warning remains.

Windows layout tests ran here, but Windows native path execution and real browser installations are unverified. Interactive acceptance remains blocked by the capture failure recorded below. No additional benchmark run or performance claim was made for these metadata rules. Broader application support needs separately curated evidence; reviewed cleanup/recovery and signed distribution remain future milestones, not completed work.

## 2026-11-17 · Package verification and CI artifacts

Development ZIPs now include a version/platform manifest and SHA-256 payload hashes. The packager validates the completed archive before atomic replacement. A standalone `verify_package.py` checks the exact file layout, duplicate entries, regular-file types, streaming hashes, native executable architecture, and macOS launch metadata/execute permissions without extracting or executing the app. Metadata reads and hash buffers are bounded. These integrity checks do not provide signing or publisher authentication.

Both native workflows now build and verify release ZIPs after their existing checks and upload artifacts named with the platform and source revision, retained for 14 days. The upload action is pinned to its official v4.6.2 commit. Workflow YAML parses locally; remote execution is still unverified. Shell access to GitHub failed DNS resolution, so no remote CI run is claimed.

Verified on Apple Silicon/macOS 27.2: **14 packaging tests** and **6 SQLite schema tests** passed. New package tests cover modified payloads, malformed/oversized manifests, duplicate/unexpected files, wrong architecture/version, invalid launch metadata even with a matching checksum, missing execute permission, symlinks, invalid ZIP data, and preservation of earlier artifacts on validation failure. The release build and standalone verification of `target/packages/Storage-Clinic-0.1.0-macos-arm64.zip` passed. The previously documented `rust-objcopy` missing-`libLLVM.dylib` stripping warning remains. The unchanged Rust application retains the preceding 74-test, Rustfmt, strict Clippy, and debug-build results; those checks and benchmarks were not repeated for Python/workflow-only changes.

Retried native testing through the extracted app bundle. Desktop automation failed with ScreenCaptureKit error -3811 (audio/video capture failure), so packaged launch and all current visual/keyboard checks remain unverified. Process inventory was also unavailable (`sysmond` service missing). No screenshots or demo were fabricated. Windows hardware/runtime, mounted volumes, real-photo calibration, accessibility, signed distribution, and later mutation/recovery work remain open as detailed in the task and release checklists.

## 2026-11-17 · UI state and evidence hardening

Source review found that Overview's last-scan card could navigate to a different previously opened history entry, CSV export accepted overlapping requests, and the Results age filter still clamped negative scan timestamps. Fixed the navigation target, added a Results-owned export busy flag across picker/write phases and scan switches, and routed age filtering through the shared saved timestamp with checked arithmetic and visible errors.

Review now preserves incomplete-analysis and skipped-check notices even when saved groups exist. Empty messages distinguish unavailable/incomplete analysis, partial indices, checked files/images with no groups, metadata candidates, and observed folders. A Refresh button reloads the current group page and its initial previews using the existing background tasks. Status rules and their tests live in `workspace/review/status.rs`.

Verified on Apple Silicon/macOS 27.2: **74 Rust tests passed** (five benchmarks ignored by the default invocation), Rustfmt, strict Clippy, and the debug build. New tests exercise age cutoffs through real SQLite queries at pre-epoch nanosecond boundaries, overflow rejection, and Review notices for incomplete groups, completed empty checks, and skipped/error counts. Existing CSV I/O tests still pass. No schema, dependency, or packaging-script changes were needed.

The release build passed, and the rebuilt macOS ZIP passed integrity, launch-metadata, executable-permission, and current-binary equality checks. The existing local `rust-objcopy` missing-`libLLVM.dylib` stripping warning remains. Native navigation, repeated-click export/picker behavior, Review Refresh, keyboard/accessibility, Windows, and mounted-volume checks remain untested; their exact scenarios are added to the release checklist. This pass is source-level UI hardening, not an interactive smoke test or accessibility certification. Benchmarks were not repeated because scanning and verification algorithms are unchanged.

## 2026-11-17 · Performance diagnostics and failure recovery

Added wall-clock diagnostics for setup, indexing, candidate queries, duplicate verification/writes, image analysis, commit, and the volume query, plus the largest same-size candidate set. Release benchmarks print them; ordinary runs can enable `RUST_LOG=storage_clinic=debug`. Timings do not claim device throughput or phase-specific CPU measurements.

The worker now stops accumulating image records if saving is unavailable and releases the queued records after an indexing write failure. Added synthetic regression tests for corrupted storage (preserve original bytes and retain a bounded preview), disappearing text/image files after indexing (preserve the full index and remaining valid groups), duplicate-group savepoint rollback, and failed scan finalization (preserve earlier history and discard uncommitted records).

Automated checks on Apple Silicon/macOS 27.2: **71 Rust tests passed**, five benchmarks ignored by the default suite. Rustfmt, strict Clippy, and debug/release app builds pass. No SQL or package-script change was made, so those suites were not repeated. The rebuilt unsigned macOS ZIP passed integrity, launch-metadata, executable-permission, and current-binary equality checks, separately from launch. The existing local `rust-objcopy` missing-`libLLVM.dylib` stripping warning remains.

Three separately executed release benchmark scenarios passed. Mostly unique 100,000-file scans took 3.61 / 3.62 s; duplicate-heavy scans took 7.76 / 7.93 s. File verification accounts for about 84% of the duplicate-heavy first scan, versus 82 ms for candidate queries. The new 20,000-file same-size candidate set found one 10,000-member duplicate group on all three passes (1.054 / 1.076 / 1.037 s, 26,800 KiB peak process RSS). See [phase measurements](benchmarks.md#phase-profiling-and-larger-candidate-sets--november-17-2026).

Earlier full-scan slowdowns did not reproduce; no normal-path performance fix is claimed. Current measurements cover warm synthetic inputs, not cold caches, device I/O, large photo libraries, or UI frame times. The finalization test injects a constraint failure, not real disk exhaustion. GUI/Windows/mounted-volume checks remain deferred. Next performance work should focus on file verification substeps or representative photo workloads, rather than changing SQLite indexes without evidence.

## 2026-11-16 · Grouped dashboard and inspection batch

Grouped the remaining work in [the task plan](tasks.md#grouped-implementation-batches), then implemented the read-only dashboard and inspection batch together:

- Overview loads the latest saved findings, extension-based storage categories, and up to five recent scans. Loading happens off GPUI at startup, on opening Overview, after a scan, and on Refresh. Loading/error/empty states are explicit; historical data is distinct from the session-only volume snapshot.
- Each row offers Open results, Review findings, and Scan again. Review actions select the same scan in Results. Scan again respects the existing active-scan and exclusion-settings guards. History remains available through All scan history.
- Summaries label cancelled indices as partial and distinguish unfinished duplicate/image analysis from finished checks with no groups. Counts of traversal issues and skipped analysis checks remain visible. Storage categories cover only the latest saved scan and represent logical bytes, including hard-linked names.
- File Inspector shows type and existing metadata-based cleanup evidence beside risk and duplicate evidence. Inspector, Review, and CSV now share a checked saved-time conversion, preserving pre-epoch milliseconds rather than clamping them. Unsaved previews explicitly lack cleanup evidence.

On Apple Silicon/macOS 27.2: **67 Rust tests passed**, four benchmark tests ignored by the normal suite; Rustfmt, strict Clippy, and the debug build passed. New tests cover empty history, reopening and bounding recent scans, category totals from the latest partial index, incomplete-analysis labels, history during an active write transaction, and saved timestamps on both sides of the epoch. Existing Review/export/scanner tests pass with the shared timestamp API.

No SQL, migration, dependency, or packaging-script changes were needed; schema/package test suites were not repeated. The release build passed and the rebuilt ZIP passed integrity, launch-metadata, executable-permission, and current-binary equality checks. The existing local `rust-objcopy` missing-`libLLVM.dylib` stripping warning remains. Interactive UI, keyboard/accessibility, Windows, and mounted-volume checks remain deferred. End-to-end performance benchmarks were not repeated, and no responsiveness measurements are claimed. Next engineering batch: isolate performance variation and exercise larger candidate/error-recovery workloads; signing and mutation/recovery remain separate work.

## 2026-11-16 · Overview volume-space snapshot

Implemented the next read-only dashboard task: Overview shows capacity and space available to the caller for the selected directory's volume after a scan. The engine owns native `statfs`/`statvfs` and `GetDiskFreeSpaceExW` adapters; GPUI only renders the worker's result. macOS uses 64-bit block counters to avoid the narrower Darwin `statvfs` representation. No dependency or schema change was needed.

The card identifies the scan root, labels the numbers as a snapshot, and explains quota/shared-storage limits. It does not estimate reclaimable space. Query failures preserve completed results; already-cancelled scans skip the query. Snapshots remain session-only rather than appearing as historical or live measurements. Native calls on unavailable/network volumes can still block their background worker until the OS returns.

Verified on Apple Silicon/macOS 27.2: **63 Rust tests passed**, four benchmark tests ignored by the normal suite; Rustfmt, strict Clippy, and the debug app build passed. New tests cover allocation arithmetic beyond 32-bit counts, overflow, full/invalid volumes, native Unicode directory paths, missing paths, regular files, symlinks, and preserving a completed scan after its selected directory disappears. Existing cancellation and storage-fallback tests now verify volume-query behavior. Windows-only tests cover UNC/extended paths and unpaired UTF-16 code units, but have not run here.

The release build and rebuilt macOS package passed; ZIP integrity, launch metadata, executable permissions, and equality with the release binary were verified separately from launch. The existing local `rust-objcopy` missing-`libLLVM.dylib` stripping warning remains. SQL and packaging code are unchanged, so their six-test suites were not repeated. End-to-end benchmarks were not rerun; earlier measurements do not include this query. Interactive checks remain deferred at the user's request. Native Windows queries, mounted volumes, quota behavior, and the new Overview card still need platform testing. Next dashboard work: saved findings/recent-scan presentation; cleanup and recovery remain outside this read-only release.

## 2026-11-16 · Small-file hashing and unsigned packages

- Duplicate candidates at most 16 KiB now reuse the sample's complete SHA-256 instead of reopening and hashing twice. Final byte comparison and file-version revalidation remain mandatory. A boundary regression covers empty files through 16 KiB + 1, including a changed middle byte outside the sample. Cache usage skips SQL writes when both counters are zero.
- `.github/scripts/package.py` builds a native release binary and creates an unsigned macOS application ZIP or Windows executable ZIP. It uses Cargo's artifact path, checks binary architecture, preserves an existing ZIP on failure, and cleans staging files. Six Python tests cover both layouts and validation/failure paths; both CI workflows now run them.
- Built `target/packages/Storage-Clinic-0.1.0-macos-arm64.zip` on Apple Silicon. Checked ZIP integrity, plist launch metadata, executable permissions, and embedded binary equality. The linked macOS libraries are system libraries/frameworks. This is archive validation, not a successful packaged launch.

Automated checks on macOS 27.2: **58 Rust tests passed**, **6 schema tests passed**, **6 packaging tests passed**, Rustfmt, strict Clippy, and debug/release app builds passed. Four benchmarks are ignored in the default suite. This follow-up separately ran the focused hashing benchmark and the duplicate-heavy integrated benchmark before and after the change; the other two integrated benchmarks were run in the preceding hardening pass. The local release build still reports the previously recorded missing-`libLLVM.dylib` symbol-stripping warning.

The focused hashing stage took 0.262 s with rereading versus 0.134 s with reuse over six alternating rounds of 1,000 files. Integrated timings varied substantially (11.70 / 9.70 s before, 17.75 / 16.15 s after); fixture creation also slowed, so the end-to-end regression is not considered resolved. See [measurements](benchmarks.md#small-file-follow-up--november-16-2026).

Deferred at the user's request: interactive app testing. Packaged launch, Windows native packaging/execution, mounted volumes, real-photo calibration, accessibility, signing, and notarization remain unverified or outstanding. The application stays read-only; cleanup and recovery remain future work.

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
