# Architecture

The [Storage Clinic product and engineering plan](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) defines the read-only v0.1 release for Apple Silicon and Windows 11 x64. The current slice covers a native shell, folder scanning and large-file inspection. The release workflow is Select Folder → Scan → Analyze → Inspect Results.

## Repository and ownership

| Path | Owner and purpose | State |
| --- | --- | --- |
| `apps/desktop/src/main.rs` | Initializes logging, GPUI Kit and the native window. | In source |
| `apps/desktop/src/workspace.rs` | Owns navigation and the active file selection. | In source |
| `apps/desktop/src/workspace/overview.rs` | Renders scan controls and progress. | In source |
| `apps/desktop/src/workspace/scan.rs` | Connects native folder selection and the background scan job to workspace state. | In source |
| `apps/desktop/src/workspace/results.rs` | Renders the largest files in the latest scan or example records before the first scan. | In source |
| `apps/desktop/src/workspace/results/inspector.rs` | Presents metadata and finding evidence for the selected row. | In source |
| `crates/scan-engine/src/lib.rs` | Owns traversal, exclusions, file metadata and error reporting. | In source |
| `crates/scan-engine/src/platform` | Owns Windows and macOS file identity and other scanner-specific platform operations. | Planned |
| `crates/scan-engine/tests` | Exercises deterministic fixture trees in temporary directories. | In source |

The spec's proposed scanner, analyzer, scheduler, storage, safety, core, and platform crates describe responsibilities. The workspace has two packages with a one-way dependency from desktop to engine. Keep analysis and platform code as engine modules as they arrive. Put scan orchestration on the engine side when jobs need to coordinate traversal, analysis and persistence. Add a separate SQLite crate when the persisted scan schema and query surface exist. Add cleanup policy and recovery modules with their first real use cases.

The desktop depends on engine types. The engine has no GPUI dependency. Features live with their views and state. A second caller or clearly separate maintenance responsibility should justify moving code into a shared module or crate. Keep Kit controls directly in the views so their documented focus and theme behavior remains inspectable.

## Toolkit and reference implementations

| Reference | Decision |
| --- | --- |
| [GPUI Kit](https://gpui-kit.com/docs/installation/) | Pin `gpui-kit = "=0.9.1"`. Its default backend uses the matching Zed `gpui-pre` 0.3.12 snapshot. Use Kit's sidebar, buttons, table, and theme. Use Kit `DataTable` when real scan rows need virtualization and native selection. |
| [rox](https://github.com/zealsprince/rox) at `f95ccf4` | Follow the design principle from its panel ADR and background jobs. Keep work off the UI thread and update retained view state at controlled intervals. Rox uses a different GPUI/component revision and declares AGPL-3.0-only. Use it for research. |
| [Czkawka/Krokiet](https://github.com/qarmin/czkawka) at `1cdc6a4` | Its MIT `czkawka_core` separates UI from analysis, processes directory work and hashes with Rayon, and has distinct partial/full hash caches. The core also brings image, audio, and archive dependencies. Evaluate focused crates such as `blake3` first and compare with the core once representative scan benchmarks exist. The Krokiet app and its assets have separate license files. |
| [Zed GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) | Use the Kit-pinned snapshot for app APIs and keep GPUI versions aligned. Verify platform SDK requirements and smoke-test the exact locked versions on both target systems. |

## Scanner integration

The engine streams regular-file metadata through a synchronous callback using `walkdir`. It accepts exact root-relative exclusions, skips symlinks and Windows reparse points, and checks cancellation between entries. Permission and metadata errors increment a counter. The scan retains the first error for display. Metadata describes each path at the time it was observed, and a directory tree can change as a scan runs.

The desktop runs one scan on the GPUI background executor. It samples a single shared progress snapshot at 100 ms intervals and receives completion through a one-slot channel. It retains at most 200 records while selecting the 100 largest for preview. The prototype’s large-file threshold is 1 GiB. Adopt Tokio for the staged workflow when SQLite indexing and hashing introduce multiple stages. SQLite will store scan data in app-owned storage and serve paged results to the virtualized table.

The analyzer groups by logical size, narrows candidate sets with a partial BLAKE3 fingerprint, hashes remaining content in full, and revalidates identity and metadata around the read. Preserve native paths, record inaccessible or changed entries, and treat multiple names for one file identity as hard links. Skip symlinks and Windows reparse points by default. Expose logical bytes separately from any future physical savings estimate. Test zero-byte files, same-size nonduplicates, distinct matching files, replacement during scanning, links, permissions, and cancellation. Track cold and warm throughput, cancellation latency and peak memory on fixed fixture trees, including the spec's 100,000-file target.
