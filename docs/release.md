# Read-only release checklist

The app currently supports selecting a folder, scanning/indexing it locally, reviewing exact duplicates and similar images, inspecting metadata and conservative risk, finding metadata-based cleanup candidates and observed empty folders, reopening/repeating scans, filtering results, and exporting CSV. It does not move, delete, restore, or automatically clean files.

## Automated validation

Run from the repository root on each target OS:

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p storage-clinic
python3 -B crates/scan-store/tests/test_schema.py
python3 -B .github/scripts/test_package.py
```

On Windows use `python` if `python3` is unavailable. The lockfile is required. The macOS and Windows workflows run these checks, but the modified workflows still need a remote run. Local results are in [progress](progress.md), with independent release measurements in [benchmarks](benchmarks.md).

The local optimized build also passed with `cargo build --locked --release -p storage-clinic`; its executable is `target/release/storage-clinic`.

## Unsigned development packages

Run `python3 -B .github/scripts/package.py` on Apple Silicon macOS or `python -B .github/scripts/package.py` on Windows x64. The script builds the locked release binary, uses Cargo's reported artifact path, validates its architecture, and atomically writes `target/packages/Storage-Clinic-<version>-<platform>.zip` (under the configured Cargo target directory). Packaging failure preserves an existing archive.

The macOS ZIP contains `Storage Clinic.app` with launch metadata and executable permissions. The Windows ZIP contains `storage-clinic.exe`. Extract the ZIP before opening. Both include a README explaining read-only behavior and the shared app-data location. No signing, notarization, installer registration, or update service is included. An OS may block downloaded unsigned applications; use the source workflow rather than disabling security settings.

The Apple Silicon package was built locally and its ZIP contents checked against the compiled executable. Both layouts, target validation, and failure preservation have automated tests. Packaged launch has not been tested; Windows runtime dependencies and native packaging must be checked on Windows before distribution.

Development executable: `target/debug/storage-clinic` on macOS, `target/debug/storage-clinic.exe` on Windows (or the equivalent target-specific Cargo directory). Run it in an unlocked interactive OS session with `cargo run --locked -p storage-clinic`. A successful compile does not verify native picker, rendering, or OS service availability.

## Disposable fixture and demo sequence

Generate a new tree outside the app-data directory:

```sh
cargo run --locked --release -p scan-engine --example smoke_fixture -- <new-folder>
```

The fixture contains searchable files, duplicates, same-size differences, sparse large disk images, sample images, metadata candidates, empty folders, exclusions, and an unreadable folder. Restore permission on its `locked` directory before removing the fixture on macOS; on Windows set up denied read access manually for that permission check.

1. Cancel the native picker, then select the fixture and start a scan. Verify visible progress and cancellation in both indexing and analysis; reopen the partial scan.
2. Run to completion. Confirm large files, counts, type totals, skipped entries/errors, and full-index search beyond the first 100 rows. Inspect rows with mouse and keyboard, change all filters and sorts, and page both directions.
3. Open Review. Inspect exact and similar groups separately, load thumbnails, and confirm that replacing or removing an image produces a visible preview failure after reloading the group. Use the keyboard to select groups.
4. Inspect Cleanup candidates and their risk reasons. Check Empty folders, including a protected/personal path. Confirm a folder containing only an excluded/hidden entry is not empty. Create more than 30 empty folders to check paging.
5. Export outside the scanned tree. Confirm file and directory kinds, logical sizes, risk/evidence, partial/completed status, group IDs, and formula-like names. Confirm export refuses the scanned tree and app-data directory.
6. Add an exclusion, scan again from History, and compare results. Repeat a large-duplicate scan and confirm cache hits without treating hard-link aliases as separate duplicate copies.
7. While a scan runs, open a completed scan and search its saved results. Close/restart the app and reopen both completed and partial history.
8. Repeat with a mounted test volume. On Windows, also check native paths, permission errors, and reparse points on Windows 11 x64.
9. After completion, return to Overview and inspect Volume space. Confirm capacity/available bytes are labelled as a last-scan snapshot, separate from folder logical size. Repeat with a cancelled scan and confirm no space reading is shown. Test a quota-limited account and mounted volume where available; unavailable statistics must leave scan results usable.
10. Restart and inspect Saved scans on Overview. Confirm the latest findings and type totals match that saved scan, recent rows are newest first and limited to five including the latest, and an incomplete analysis does not claim zero verified findings. Exercise Refresh, Open results, Review findings, Scan again, and All scan history with keyboard and mouse. After opening Review from a row, switch to Results and confirm the same scan is selected.
11. Select an archive, an empty file, and an old download in File Inspector. Compare cleanup evidence and risk with Review and CSV. Age evidence must stay anchored to the saved scan date after reopening; an unsaved preview should report cleanup evidence as unavailable.

Record platform/version, exact build revision, fixture, result, and any differences. Capture screenshots of Overview, Results/Inspector, and each Review tab plus a short Select → Scan → Review → Export demo only after these steps actually run. No screenshots or demo are claimed for the current pass.

## Outstanding gates

| Gate | Current status |
| --- | --- |
| Local automated tests and debug build | Passed on macOS; see progress |
| Unsigned development packages | macOS archive built and checked; launch and Windows native packaging pending |
| Integrated macOS UI | Blocked during this pass by the locked desktop/native pasteboard service |
| Windows CI and Windows 11 UI | Earlier baseline CI passed; current changes and Windows 11 UI untested |
| Mounted volumes and real-photo calibration | Not tested |
| Accessibility audit and performance regression investigation | Still required |
| Signed installers, notarization, updates, and publication | Not implemented; require a distribution design and signing identities |
| Trash/Recycle Bin, cleanup plans, journals, recovery, reclaimable space | Future v0.3 work; no destructive controls are enabled |

Do not tag a cross-platform release based only on this machine's build. Preserve current database versions when testing migrations. The database includes native path encodings and is local to its operating system, not a portable backup format. A history backup should be taken with the app closed so WAL data has been checkpointed.
