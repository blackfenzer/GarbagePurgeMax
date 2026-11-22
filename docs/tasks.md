# Storage Clinic development plan

This plan follows the [Storage Clinic product and engineering spec](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) and reflects the repository as of November 18, 2026. See [architecture](architecture.md) for implementation decisions and [progress](progress.md) for verification evidence.

## Grouped implementation batches

| Batch | Tasks that belong together | State and acceptance |
| --- | --- | --- |
| Dashboard and inspection | Latest saved findings, five recent scans, storage categories, Open/Review/Scan again actions, and Inspector cleanup evidence | Implemented together. Saved data loads off the UI thread at startup, when opening Overview, after a scan, and on Refresh. Partial/incomplete analysis stays visible. Inspector, Review, and CSV share the saved observation time. 67 Rust tests pass; interactive review remains pending. |
| Performance and reliability | Isolate small-file timing variation, exercise larger candidate sets and error recovery, compare memory/cancellation on the same fixtures | First pass implemented: phase diagnostics, 20,000-candidate benchmark, corrupt-storage/disappearing-file/savepoint/finalization regression tests, and image-candidate release on storage failure. File verification dominates the measured workload; earlier timing variance remains unresolved. See benchmarks. |
| UI state and evidence | Correct last-scan navigation, prevent overlapping exports, align age filters with saved time, and distinguish partial Review results from empty completed checks | Implemented and covered by age-boundary and Review-status tests; native navigation, picker, and Refresh behavior still need manual checks. This is not a completed accessibility audit. |
| Curated app cache evidence | Document default app locations, exclude adjacent profile data, and share candidate/risk evidence across Review, Inspector, and CSV | Initial Chrome and Chromium HTTP cache policies implemented with synthetic cross-platform layout and persisted-evidence tests. Other apps/locations require separately supported policies. Native Windows acceptance remains pending. |
| Platform and release validation | Validated native packages, CI artifacts, Windows runtime, macOS packaged launch, mounted volumes, accessibility, real-photo calibration, screenshots/demo, signing and updates | Package manifests, standalone verification, and CI artifact uploads are implemented; 14 packaging tests pass. Local macOS package verified. Native launch retry failed in desktop screen capture; Windows, remote CI, and other interactive/distribution gates remain open. |
| Reviewed cleanup and recovery | Queue, confirmation, revalidation, Trash/Recycle Bin actions, journals and restore | v0.3 implementation complete; native macOS Trash test blocked by permissions, Windows build/runtime and interactive acceptance pending. See cleanup-recovery.md. |

Named profiles, deeper archive previews, and customization remain conditional rather than expanding the current batches.

## v0.1 · Read-only foundation

### Current completion pass

- [x] Persist observed empty folders without inferring emptiness from exclusions or errors.
- [x] Show conservative location risks in review, Inspector, and CSV.
- [x] Cache duplicate hashes only after native identity and metadata revalidation; always byte-compare matches.
- [x] Bound image decoding and preserve cancellation and skipped-check evidence.
- [x] Harden export, cover the integrated workflow and migration upgrades, and rerun local release checks.
- [x] Record current benchmark results and exact remaining platform/release gates. See [progress](progress.md) and the [release checklist](release.md).

The foundation keeps scans read-only. v0.3 adds a separately confirmed mutation workflow at the user’s request; v1.0 distribution and conditional customization remain separate milestones.

The release flow is **Select Folder → Scan → Analyze → Inspect Results** on Apple Silicon macOS and Windows 11 x64. Scans read the selected files and save metadata locally.

### Implemented

- [x] Establish the Rust workspace and build the native GPUI app on Apple Silicon.
- [x] Connect folder selection, metadata and permission-error reporting, background scanning, live progress, cancellation, and persistent file or folder exclusions.
- [x] Show file counts, logical size, large-file findings, and file metadata in Overview, Scan Results, and File Inspector.
- [x] Persist metadata and findings in SQLite, preserve earlier scan history on upgrade, and open saved scans with paged path search and a virtualized table.
- [x] Group same-size files and verify exact duplicates with staged SHA-256, byte comparison, and native-identity checks around reads.
- [x] Show verified duplicate groups and the evidence in the Inspector. Include fixtures for empty files, same-size differences, replacements, symlinks, hard links, and permission errors.
- [x] Confirm the native picker, an ordinary scan, and scan history after a restart on macOS.
- [x] Pass the macOS desktop smoke test on the generated fixture. Mounted-volume selection is still unconfirmed.
- [x] Measure scanner traversal throughput, worker memory, and cancellation on a generated 100,000-file tree. See [benchmarks](benchmarks.md).
- [x] Add a native Windows x64 build and test workflow.

### Close before v0.1

| Order | Task | Acceptance |
| --- | --- | --- |
| 1 | [x] Add a file-type breakdown to the scan summary | Show counts and logical bytes for the current saved scan, with a fallback for files without a recognized extension. Keep categorization off the UI thread and cover partial and reopened scans. |
| 2 | [x] Run the macOS desktop smoke test | Use a generated tree to verify picker cancel, cancel during traversal or duplicate verification, full-scan search beyond the first page, keyboard and mouse Inspector navigation, exclusions, duplicate evidence, and reopened history. Check folder and supported mounted-volume selection through the existing picker. |
| 3 | [ ] Run native Windows x64 CI and the Windows 11 desktop smoke test | Link the locked app, pass the shared fixture suite, and repeat the macOS smoke scenarios using Windows paths and a disposable folder or test volume. Record the runner, Windows version, and any platform-specific limits. The earlier baseline passed on `windows-2022`; the new changes and expanded workflow have not run remotely. **Needs testing:** the Windows 11 smoke test, once a Windows machine is available. |
| 4 | [x] Measure the end-to-end flow on 100,000+ files | Include SQLite indexing, duplicate-heavy and mostly unique inputs, search and paging during review, and a cancellation check. Record per-phase timing, peak process memory, and responsiveness separately from the traversal benchmark. Record warm and cold-cache observations where reproducible. |
| 5 | [ ] Document the v0.1 release and portfolio demo | Update setup, platform limits and measured results. Capture screenshots and a short read-only workflow demo once both platforms pass. Record the build artifacts and link the architecture and benchmark evidence. Setup, limits, results, and evidence links are in the README. **Needs testing first:** screenshots and the demo wait for the Windows 11 smoke test. |

### Needs testing

Run these checks before tagging v0.1. Windows hardware and mounted volumes were not available for this pass; local UI automation was blocked by the locked desktop and native pasteboard service.

- [ ] **Windows 11 x64 desktop smoke test.** Generate the tree with `cargo run --locked --release -p scan-engine --example smoke_fixture -- <new folder>`, deny read access to its `locked` folder in Properties → Security, and repeat the macOS checklist recorded in [progress](progress.md). Record the Windows version and any path or permission differences.
- [ ] **Mounted-volume selection on macOS and Windows.** Choose a disk image or USB volume in the native picker and scan it.

The current pass adds macOS CI and strengthens Windows CI with formatting, Clippy, and schema tests. Neither updated workflow has been dispatched from this session. Run final release checks on the integrated build. If the desktop automation cannot launch the development executable, run the smoke test in an interactive OS session and record what passed.

## v0.2 · Analysis and discovery

- [x] Evaluate perceptual image hashes on a fixture set with crops, resizes, edits, and visually distinct images. Keep similarity evidence separate from verified exact matches. See [image similarity](image-similarity.md): use a difference hash at ≤ 6 and confirm it on real photos.
- [x] Build Similar Images and Duplicate Review views with side-by-side previews, group details, and evidence. **Needs testing:** an interactive check of the Review page, and the similarity threshold on real photos.
- [x] Add evidence-backed candidates for old downloads, installers, archives, known temporary files, and empty files. **Needs testing:** an interactive check of the Cleanup candidates tab.
- [x] Detect empty folders. Scanner observations and migration v4 preserve folders separately from files, including partial scans. Review pages the observed empty folders; CSV exports them as directory entries. Exclusions and unreadable contents never imply emptiness. **Needs testing:** the new tab in the running app.
- [x] Add repeatable scans, richer filters and sorting, and scan export. Preserve path, type, size, age, group, confidence, risk, status, and directory semantics where applicable. Scan History repeats a scan with the current exclusions. Scan Results filters by type and age and sorts five ways. Export CSV keeps path, folder, type, size, timestamps, groups, cleanup rule, and status, and its column names state each finding's confidence. CSV now includes risk and its evidence, plus empty folders with explicit directory semantics. **Needs testing:** an interactive check of the filters, Scan again, and Export CSV.
- [ ] **Conditional / deferred:** named scan profiles, only if Scan again and persistent exclusions prove too limited. No such user evidence yet.
- [x] Add incremental full-hash caching for duplicate candidates at least 64 KiB, with native identity, size, timestamps, and change-time revalidation. Byte comparison always reruns. Metadata and image hashes remain fresh. See [benchmarks](benchmarks.md) for uncached/repeat timings, CPU time, memory, and hit counts. **Unmeasured:** privileged cold-cache runs and device-level I/O throughput.
- [x] Add conservative protected/high/unknown risk tiers to Review, Inspector, and CSV. System, application, and known cloud-managed paths are protected; personal and development paths are high risk. All states stay review-only. Path rules do not claim complete cloud-placeholder or OS protection detection.

## v0.3 · Reviewed cleanup and recovery

Implemented in the 0.3.0 development build. Scans remain read-only. See [cleanup and recovery](cleanup-recovery.md) for the contract, state transitions, and native acceptance limits.

- [x] Add Cleanup Queue and Recovery History with proposed actions, original/kept paths, consequences, recovery method, and bounded pagination.
- [x] Require explicit confirmation, exact queue membership, current exclusions and pre-action identity/content/risk revalidation. Reserve a distinct byte-verified kept copy across active cleanup history.
- [x] Implement native macOS Trash and Windows Recycle Bin adapters without a permanent-deletion fallback. Show per-item validation errors, receipts, and uncertain outcomes. **Native acceptance pending below.**
- [x] Persist durable intents, receipts and outcomes; implement no-overwrite restore, cancellation, partial failures, and read-only reconciliation. Inject interruption and journal-write failures on disposable fixtures.
- [x] Separate logical bytes, potential allocation upper bounds, expected immediate release (zero), and observed free-space change. Block hard links and detected placeholders; disclose clone/compression/sparse/cloud uncertainty.
- [x] Move a chosen file or folder to Trash from Files, Tree or Space Map after a contents preview, journaled and restorable like the queue.
- [ ] Complete native macOS Trash/restore and interactive acceptance, including a non-empty folder moved from Files or Tree. The disposable native test was attempted and rejected by filesystem permissions; no successful native round trip is claimed.
- [ ] Compile and run native Windows cleanup/recovery acceptance, including collisions, unavailable Recycle Bin, permissions, interruptions, and mounted volumes. Deferred on this Mac at the user's request.

## v1.0 · Product hardening

Current hardening includes bounded thumbnails and candidate previews, decoder limits, WAL readers, keyboard-focusable review buttons, safer export, and regression tests. Overview now shows OS-reported volume capacity and available space for the latest scan in this session. Initial app cache policies cover Chrome and Chromium HTTP caches. Full accessibility audits, broader policy coverage, signed installers, updates, and publication remain open.

- [x] Show volume capacity and space available to the caller in Overview after a scan, distinct from logical folder size. Query off the UI thread, label the snapshot, and preserve results on query failure. macOS native query and regression tests pass; Windows execution and visual review remain pending.
- [x] Complete the read-only dashboard and Inspector presentation: volume snapshot, latest saved findings, storage categories, recent scans, direct navigation, and metadata-based cleanup evidence. Automated data checks pass; visual and keyboard checks remain pending.
- [ ] Add dashboard and Inspector review-action/recovery outcomes when the v0.3 mutation workflow exists.
- [x] Curate initial read-only app cache policies with platform-specific fixtures and clear evidence: default Chrome and Chromium HTTP cache locations. [Policy scope and source evidence](cache-policies.md) explicitly exclude sibling profile data and custom locations. Windows native execution remains unverified; additional applications need their own curated policies.
- [ ] Harden accessibility, keyboard navigation, throughput, memory limits, and error recovery on supported OS versions. Measure regressions against the same fixtures.
- [x] Harden source-level UI state: last-scan navigation, single active CSV export, checked saved-time age filters, Review completion/error notices, and a Refresh action. Interactive acceptance remains on the release checklist.
- [x] Build unsigned development ZIPs for native macOS arm64 and Windows x64 hosts, with layout, architecture, and failure-preservation tests. The macOS archive is built; packaged launch and actual Windows packaging still need platform testing.
- [x] Verify completed package manifests, SHA-256 payload hashes, layout, architecture, and launch metadata before atomic replacement. Configure CI to upload development ZIPs after checks pass. Local archive and 14 packaging tests pass; remote artifact production still needs a workflow run.
- [x] Add a Space Map page (WizTree-style folder sizes, treemap, inspector) and a saved Workbench / Amber Deck theme choice, from the [Space Map design](https://claude.ai/artifact/TxcKwHdjUeTQtyE3YqUPv2). Folder totals and treemap layout have tests; visual review of both themes remains pending.
- [x] Build the Amber Deck layout: top scan bar, tabbed left/right panels, and footer graphs (size by modified year, folder size strip, mini treemap, caption). Visual review remains pending.
- [x] Add an off-screen UI smoke test that scans a synthetic folder, opens every page in both themes, checks Space Map navigation, and saves screenshots for review (`--features ui-smoke --test ui_smoke`). macOS only; not yet in CI.
- [ ] Run the UI smoke test in macOS CI once GitHub runners are confirmed to render with Metal, and drive it with simulated clicks instead of direct method calls.
- [x] Smart scan scope for broad scans; analysis separated from indexing with progress, cancel, re-run and worker count; bento Overview; resizable Amber Deck. Real-drive measurements remain.
- [ ] Add a Quick scan comparison on a real 40 GB+ folder: indexing time with smart scope on and off, and analysis time by worker count, in a release build.
- [ ] Sign, notarize where applicable, and define the release and update process. Publish release builds, support docs, architecture diagram, reproducible benchmark results, screenshots, and a short demo.
- [ ] Evaluate deeper archive previews and workspace customization against user evidence. Keep analysis local and require opt-in for any future telemetry.

## Decisions to validate as the work reaches them

- Keep the current Rust, GPUI Kit, SQLite, and SHA-256 implementation as the baseline. Revisit worker pools, cache choices, and hashing dependencies when measurements identify a bottleneck.
- Verify the supported OS versions, image similarity method, archive formats, cache policies, recoverable cleanup mechanisms, and signing and update strategy before releasing those capabilities.
