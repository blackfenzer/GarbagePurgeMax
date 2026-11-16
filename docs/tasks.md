# Storage Clinic development plan

This plan follows the [Storage Clinic product and engineering spec](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) and reflects the repository as of November 16, 2026. See [architecture](architecture.md) for implementation decisions and [progress](progress.md) for verification evidence.

## v0.1 · Read-only foundation

### Current completion pass

- [x] Persist observed empty folders without inferring emptiness from exclusions or errors.
- [x] Show conservative location risks in review, Inspector, and CSV.
- [x] Cache duplicate hashes only after native identity and metadata revalidation; always byte-compare matches.
- [x] Bound image decoding and preserve cancellation and skipped-check evidence.
- [x] Harden export, cover the integrated workflow and migration upgrades, and rerun local release checks.
- [x] Record current benchmark results and exact remaining platform/release gates. See [progress](progress.md) and the [release checklist](release.md).

This pass keeps the selected tree read-only. The v0.3 actions and v1.0 distribution work below remain future milestones; conditional profiles and customization require evidence before implementation.

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

Deferred beyond this read-only release. These are implementation tasks, not skipped tests or completed features. Trash/Recycle Bin, recovery journaling, and reclaimable-space evidence require a separately validated mutation workflow.

- [ ] Add Cleanup Queue and Recovery History. Build a reviewable plan with the proposed action, copy to keep, consequences, and recovery method.
- [ ] Add explicit confirmation and pre-action revalidation. Preserve at least one verified copy in every duplicate group.
- [ ] Implement recoverable cleanup through macOS Trash and Windows Recycle Bin where supported. Surface per-item capability and results.
- [ ] Journal operations and build restore and partial-failure handling. Test controlled files, interruptions, and recovery on both operating systems.
- [ ] Estimate reclaimable space separately from logical bytes. Verify actual free-space change after cleanup and report uncertainty for hard links, clones, sparse or compressed files, and cloud placeholders.

## v1.0 · Product hardening

Current hardening includes bounded thumbnails and candidate previews, decoder limits, WAL readers, keyboard-focusable review buttons, safer export, and regression tests. Full accessibility audits, real volume statistics, app-specific cache policies, signed installers, updates, and publication remain open.

- [ ] Complete dashboard and Inspector presentation for supported volume statistics, storage categories, findings, recent scans, and review and recovery outcomes.
- [ ] Curate per-app cache policies with platform-specific fixtures and clear evidence for each classification.
- [ ] Harden accessibility, keyboard navigation, throughput, memory limits, and error recovery on supported OS versions. Measure regressions against the same fixtures.
- [ ] Package, sign, notarize where applicable, and define the release and update process. Publish release builds, support docs, architecture diagram, reproducible benchmark results, screenshots, and a short demo.
- [ ] Evaluate deeper archive previews and workspace customization against user evidence. Keep analysis local and require opt-in for any future telemetry.

## Decisions to validate as the work reaches them

- Keep the current Rust, GPUI Kit, SQLite, and SHA-256 implementation as the baseline. Revisit worker pools, cache choices, and hashing dependencies when measurements identify a bottleneck.
- Verify the supported OS versions, image similarity method, archive formats, cache policies, recoverable cleanup mechanisms, and signing and update strategy before releasing those capabilities.
