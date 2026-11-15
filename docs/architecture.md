# Architecture

The [Storage Clinic product and engineering plan](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) defines the read-only v0.1 release for Apple Silicon and Windows 11 x64. The release workflow is Select Folder → Scan → Analyze → Inspect Results.

## Repository and ownership

| Path | Owner and purpose | State |
| --- | --- | --- |
| `apps/desktop/src/main.rs` | Initializes logging, GPUI Kit and the native window. | In source |
| `apps/desktop/src/workspace.rs` | Owns navigation and the active file selection. | In source |
| `apps/desktop/src/workspace/overview.rs` | Renders scan controls and progress. | In source |
| `apps/desktop/src/workspace/scan.rs` | Connects native folder selection and the background scan job to workspace state. | In source |
| `apps/desktop/src/workspace/results.rs` | Renders saved file pages, the fallback preview, and example records. | In source |
| `apps/desktop/src/workspace/results/file_table.rs` | Maps virtualized rows to stable indexes in the current page or preview. | In source |
| `apps/desktop/src/workspace/results/saved.rs` | Loads persisted files and debounces search while the user types. | In source |
| `apps/desktop/src/workspace/history.rs` | Loads saved scans and opens their results. | In source |
| `apps/desktop/src/workspace/storage.rs` | Resolves the app data directory and keeps it out of scans. | In source |
| `apps/desktop/src/workspace/exclusions.rs` | Stores excluded paths and resolves them relative to the selected scan root. | In source |
| `apps/desktop/src/workspace/results/inspector.rs` | Presents metadata and finding evidence for the selected row. | In source |
| `crates/scan-engine/src/lib.rs` | Owns traversal, exclusions, file metadata and error reporting. | In source |
| `crates/scan-engine/src/duplicates.rs` | Verifies exact duplicates. Its platform code revalidates file identity before and after reads. | In source |
| `crates/scan-engine/tests` | Exercises deterministic fixture trees in temporary directories. | In source |
| `crates/scan-store` | Owns the SQLite migration, transactional writer, and queries for scan history and file search. | In source |

The desktop depends on the scanner and storage crates. Storage uses the scanner’s metadata types. The scanner has no SQLite or GPUI dependency. Keep file analysis and platform code in the scanner crate.

The desktop depends on engine types. The engine has no GPUI dependency. Features live with their views and state. A second caller or clearly separate maintenance responsibility should justify moving code into a shared module or crate. Keep Kit controls directly in the views so their documented focus and theme behavior remains inspectable.

## Toolkit and reference implementations

| Reference | Decision |
| --- | --- |
| [GPUI Kit](https://gpui-kit.com/docs/installation/) | Pin `gpui-kit = "=0.9.1"`. Its default backend uses the matching Zed `gpui-pre` 0.3.12 snapshot. Use Kit's sidebar, buttons, theme, and `DataTable` for virtualized rows and native selection. |
| [rox](https://github.com/zealsprince/rox) at `f95ccf4` | Follow the design principle from its panel ADR and background jobs. Keep work off the UI thread and update retained view state at controlled intervals. Rox uses a different GPUI/component revision and declares AGPL-3.0-only. Use it for research. |
| [Czkawka/Krokiet](https://github.com/qarmin/czkawka) at `1cdc6a4` | Its MIT `czkawka_core` separates UI from analysis, processes directory work and hashes with Rayon, and has distinct partial/full hash caches. The core also brings image, audio, and archive dependencies. Evaluate focused crates such as `blake3` first and compare with the core once representative scan benchmarks exist. The Krokiet app and its assets have separate license files. |
| [Zed GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) | Use the Kit-pinned snapshot for app APIs and keep GPUI versions aligned. Verify platform SDK requirements and smoke-test the exact locked versions on both target systems. |

## Scanner integration

The engine streams regular-file metadata through a synchronous callback using `walkdir`. It accepts exact root-relative exclusions, skips symlinks and Windows reparse points, and checks cancellation between entries. Permission and metadata errors increment a counter. The scan retains the first error for display. Metadata describes each path at the time it was observed, and a directory tree can change as a scan runs.

The desktop runs one scan on the GPUI background executor. It samples a single shared progress snapshot at 100 ms intervals and receives completion through a one-slot channel. It streams file metadata to SQLite and retains at most 200 records while selecting the 100 largest in case saving fails. The large-file threshold is 1 GiB. Exclusions are saved atomically in the app data directory and apply to future scans. The background scan reads a snapshot before walking the tree.

The analyzer reads one equal-size candidate set at a time from SQLite. It samples up to 8 KiB from each end with SHA-256, hashes remaining candidates in full, and compares matching candidates byte for byte. Each analysis set retains its candidates in memory. The analyzer compares available indexed size and timestamps before reading and revalidates native identity and metadata around each read. It captures file identity when verification first opens the file. A replacement that preserves all recorded metadata before that point can pass the initial check. The analyzer skips symlinks and reparse points and counts hard-linked names as one file identity. The traversal benchmark measures throughput, peak worker memory, and cancellation latency on a 100,000-file fixture.

## SQLite scan index

The schema stores scan summaries and file records in separate tables. Each record has a native path key and a display path. Preserve Unix path bytes or Windows UTF-16 code units in the key so distinct filenames survive lossy display conversion. Store file timestamps as seconds and nanoseconds, including timestamps before the Unix epoch.

The writer enables foreign keys on every connection and records each scan in one transaction. Completing or cancelling a scan commits its summary and available rows. Dropping an incomplete transaction rolls back its rows. History queries hide running scans. File search checks the entire saved scan for a literal path substring and folds ASCII case. File pages have a stable size order with the native key as a tiebreaker. Desktop queries run in the background with bundled SQLite through `rusqlite`.

The v2 migration keeps existing scans and adds duplicate groups and membership. Older scans need a new scan to generate findings. Cancelling verification keeps groups completed before cancellation and marks verification incomplete.
