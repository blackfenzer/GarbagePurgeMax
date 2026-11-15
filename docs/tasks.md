# Storage Clinic development plan

This plan follows the [Storage Clinic product and engineering spec](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) and reflects the repository as of November 15, 2026. See [architecture](architecture.md) for implementation decisions and [progress](progress.md) for verification evidence.

## v0.1 · Read-only foundation

The release flow is **Select Folder → Scan → Analyze → Inspect Results** on Apple Silicon macOS and Windows 11 x64. Scans read the selected files and save metadata locally.

### Implemented

- [x] Establish the Rust workspace and build the native GPUI app on Apple Silicon.
- [x] Connect folder selection, metadata and permission-error reporting, background scanning, live progress, cancellation, and persistent file or folder exclusions.
- [x] Show file counts, logical size, large-file findings, and file metadata in Overview, Scan Results, and File Inspector.
- [x] Persist metadata and findings in SQLite, preserve earlier scan history on upgrade, and open saved scans with paged path search and a virtualized table.
- [x] Group same-size files and verify exact duplicates with staged SHA-256, byte comparison, and native-identity checks around reads.
- [x] Show verified duplicate groups and the evidence in the Inspector. Include fixtures for empty files, same-size differences, replacements, symlinks, hard links, and permission errors.
- [x] Confirm the native picker, an ordinary scan, and scan history after a restart on macOS.
- [x] Measure scanner traversal throughput, worker memory, and cancellation on a generated 100,000-file tree. See [benchmarks](benchmarks.md).
- [x] Add a native Windows x64 build and test workflow.

### Close before v0.1

| Order | Task | Acceptance |
| --- | --- | --- |
| 1 | [x] Add a file-type breakdown to the scan summary | Show counts and logical bytes for the current saved scan, with a fallback for files without a recognized extension. Keep categorization off the UI thread and cover partial and reopened scans. |
| 2 | [ ] Run the macOS desktop smoke test | Use a generated tree to verify picker cancel, cancel during traversal or duplicate verification, full-scan search beyond the first page, keyboard and mouse Inspector navigation, exclusions, duplicate evidence, and reopened history. Check folder and supported mounted-volume selection through the existing picker. |
| 3 | [ ] Run native Windows x64 CI and the Windows 11 desktop smoke test | Link the locked app, pass the shared fixture suite, and repeat the macOS smoke scenarios using Windows paths and a disposable folder or test volume. Record the runner, Windows version, and any platform-specific limits. |
| 4 | [ ] Measure the end-to-end flow on 100,000+ files | Include SQLite indexing, duplicate-heavy and mostly unique inputs, search and paging during review, and a cancellation check. Record per-phase timing, peak process memory, and responsiveness separately from the traversal benchmark. Record warm and cold-cache observations where reproducible. |
| 5 | [ ] Document the v0.1 release and portfolio demo | Update setup, platform limits and measured results. Capture screenshots and a short read-only workflow demo once both platforms pass. Record the build artifacts and link the architecture and benchmark evidence. |

Run final release checks on the integrated build. If the desktop automation cannot launch the development executable, run the smoke test in an interactive OS session and record what passed.

## v0.2 · Analysis and discovery

- [ ] Evaluate perceptual image hashes on a fixture set with crops, resizes, edits, and visually distinct images. Keep similarity evidence separate from verified exact matches.
- [ ] Build Similar Images and Duplicate Review views with side-by-side previews, group details, and evidence.
- [ ] Add evidence-backed candidates for old downloads, installers, archives, known temporary files, and empty items.
- [ ] Add repeatable scan profiles, richer filters and sorting, and scan export. Preserve path, type, size, age, group, confidence, risk, status, and directory semantics where applicable.
- [ ] Add incremental scan caching. Reuse results only after revalidating file identity, size, and timestamps. Measure cold and warm scans, hashing throughput, CPU and I/O load, and cache effectiveness.
- [ ] Add protected locations and conservative risk tiers. Keep unknown, personal, development, system, and cloud-placeholder data in review-only states.

## v0.3 · Reviewed cleanup and recovery

- [ ] Add Cleanup Queue and Recovery History. Build a reviewable plan with the proposed action, copy to keep, consequences, and recovery method.
- [ ] Add explicit confirmation and pre-action revalidation. Preserve at least one verified copy in every duplicate group.
- [ ] Implement recoverable cleanup through macOS Trash and Windows Recycle Bin where supported. Surface per-item capability and results.
- [ ] Journal operations and build restore and partial-failure handling. Test controlled files, interruptions, and recovery on both operating systems.
- [ ] Estimate reclaimable space separately from logical bytes. Verify actual free-space change after cleanup and report uncertainty for hard links, clones, sparse or compressed files, and cloud placeholders.

## v1.0 · Product hardening

- [ ] Complete dashboard and Inspector presentation for supported volume statistics, storage categories, findings, recent scans, and review and recovery outcomes.
- [ ] Curate per-app cache policies with platform-specific fixtures and clear evidence for each classification.
- [ ] Harden accessibility, keyboard navigation, throughput, memory limits, and error recovery on supported OS versions. Measure regressions against the same fixtures.
- [ ] Package, sign, notarize where applicable, and define the release and update process. Publish release builds, support docs, architecture diagram, reproducible benchmark results, screenshots, and a short demo.
- [ ] Evaluate deeper archive previews and workspace customization against user evidence. Keep analysis local and require opt-in for any future telemetry.

## Decisions to validate as the work reaches them

- Keep the current Rust, GPUI Kit, SQLite, and SHA-256 implementation as the baseline. Revisit worker pools, cache choices, and hashing dependencies when measurements identify a bottleneck.
- Verify the supported OS versions, image similarity method, archive formats, cache policies, recoverable cleanup mechanisms, and signing and update strategy before releasing those capabilities.
