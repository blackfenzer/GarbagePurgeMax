# Architecture

The [Storage Clinic product and engineering plan](https://docs.google.com/document/d/18SDjWNdy-TYGB-RnyspMEBs-md3MumCaWI_fc94-HQc/edit) defines the read-only v0.1 release for Apple Silicon and Windows 11 x64. The foundation workflow is Select Folder → Scan → Analyze → Inspect Results. v0.3 adds Review → Queue → Confirm → Trash → Recovery as a separate mutation path.

## Repository and ownership

| Path | Owner and purpose | State |
| --- | --- | --- |
| `apps/desktop/src/main.rs` | Initializes logging, GPUI Kit and the native window. Quits when the window closes. | In source |
| `apps/desktop/src/workspace.rs` | Owns navigation and the active file selection. | In source |
| `apps/desktop/src/workspace/overview.rs` | Renders scan controls and progress. | In source |
| `apps/desktop/src/workspace/overview/saved.rs` | Loads and renders five recent saved scans, the latest findings and storage categories, and scan-specific actions. | In source |
| `apps/desktop/src/workspace/scan.rs` | Connects native folder selection and background scheduling to workspace state. | In source |
| `apps/desktop/src/workspace/scan/job.rs` | Runs the blocking scan/index/analysis pipeline and its integrated fixture tests and benchmarks. | In source |
| `apps/desktop/src/workspace/results.rs` | Renders saved file pages, the fallback preview, and example records. | In source |
| `apps/desktop/src/workspace/results/file_table.rs` | Maps virtualized rows to stable indexes in the current page or preview. | In source |
| `apps/desktop/src/workspace/results/saved.rs` | Loads persisted files and debounces search while the user types. | In source |
| `apps/desktop/src/workspace/history.rs` | Loads saved scans and opens their results. | In source |
| `apps/desktop/src/workspace/storage.rs` | Resolves the app data directory and keeps it out of scans. | In source |
| `apps/desktop/src/workspace/exclusions.rs` | Stores excluded paths and resolves them relative to the selected scan root. | In source |
| `apps/desktop/src/workspace/review.rs` | Lists exact-duplicate and similar-image groups for the open scan in separate tabs, with side-by-side previews and evidence. | In source |
| `apps/desktop/src/workspace/results/export.rs` | Writes a saved scan to CSV through a temporary file and defuses cells a spreadsheet would run as formulas. | In source |
| `apps/desktop/src/workspace/results/inspector.rs` | Presents metadata and finding evidence for the selected row. | In source |
| `crates/scan-engine/src/lib.rs` | Owns traversal, exclusions, file metadata and error reporting. | In source |
| `crates/scan-engine/src/duplicates.rs` | Verifies exact duplicates and optionally reuses revalidated full hashes. | In source |
| `crates/scan-engine/src/file_version.rs` | Shares native identity and metadata checks between duplicate analysis, image analysis, and previews. | In source |
| `crates/scan-engine/src/hash_cache.rs` | Defines the small caller-owned cache interface, with no database dependency. | In source |
| `crates/scan-engine/src/risk.rs` | Classifies conservative location risks; Protected blocks cleanup and other tiers still require review and confirmation. | In source |
| `crates/scan-engine/src/volume_space.rs` | Queries OS-reported volume capacity and available space with native paths and checked byte arithmetic. | In source |
| `crates/scan-store/src/hash_cache.rs` | Implements transactional cache reads, writes, and usage accounting. | In source |
| `crates/scan-engine/src/cleanup_candidates.rs` | Matches indexed file metadata against review-only cleanup rules and explains each match. | In source |
| `crates/scan-engine/src/app_cache.rs` | Matches narrow native Chrome/Chromium HTTP cache layouts for shared candidate and protected-risk evidence; never reads browser data. | In source |
| `crates/scan-engine/src/similar_images.rs` | Decodes supported images read-only with at most two workers, computes 64-bit difference hashes, and groups images within 6 bits of a reference image. | In source |
| `crates/scan-engine/src/file_types.rs` | Names a broad file type from the path extension, with an Other fallback. | In source |
| `crates/scan-engine/tests` | Exercises deterministic fixture trees in temporary directories. | In source |
| `crates/scan-store` | Owns the SQLite migration, transactional writer, and queries for scan history and file search. | In source |
| `.github/scripts` | Builds unsigned native packages; a separate verifier checks manifests, streaming payload hashes, architecture, archive layout, and launch metadata before atomic replacement. Tests preserve earlier artifacts on failure. | In source |

The desktop depends on scanner, cleanup-engine, and storage crates. Cleanup-engine depends on scanner risk types but has no GPUI or SQLite dependency. Storage also uses cleanup evidence and receipt types. Storage uses the scanner’s metadata types. The scanner has no SQLite or GPUI dependency. Keep file analysis and platform code in the scanner crate.

The desktop depends on engine types. The engine has no GPUI dependency. Features live with their views and state. A second caller or clearly separate maintenance responsibility should justify moving code into a shared module or crate. Keep Kit controls directly in the views so their documented focus and theme behavior remains inspectable.

## Toolkit and reference implementations

| Reference | Decision |
| --- | --- |
| [GPUI Kit](https://gpui-kit.com/docs/installation/) | Pin `gpui-kit = "=0.9.1"`. Its default backend uses the matching Zed `gpui-pre` 0.3.12 snapshot. Use Kit's sidebar, buttons, theme, and `DataTable` for virtualized rows and native selection. |
| [rox](https://github.com/zealsprince/rox) at `f95ccf4` | Follow the design principle from its panel ADR and background jobs. Keep work off the UI thread and update retained view state at controlled intervals. Rox uses a different GPUI/component revision and declares AGPL-3.0-only. Use it for research. |
| [Czkawka/Krokiet](https://github.com/qarmin/czkawka) at `1cdc6a4` | Its MIT `czkawka_core` separates UI from analysis, processes directory work and hashes with Rayon, and has distinct partial/full hash caches. The core also brings image, audio, and archive dependencies. Evaluate focused crates such as `blake3` first and compare with the core once representative scan benchmarks exist. The Krokiet app and its assets have separate license files. |
| [Zed GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) | Use the Kit-pinned snapshot for app APIs and keep GPUI versions aligned. Verify platform SDK requirements and smoke-test the exact locked versions on both target systems. |

## Scanner integration

The engine streams file and directory observations through `scan_entries`, a synchronous callback using `walkdir`. The `scan_directory` adapter keeps the file-only API for existing callers. Each directory is checked directly for any entry, including hidden, excluded, and link entries. An unreadable listing is unknown, never empty. Folder observations live separately from files and do not contribute logical bytes. It accepts exact root-relative exclusions, skips symlinks and Windows reparse points, and checks cancellation between entries. Permission and metadata errors increment a counter. The scan retains the first error for display. Metadata describes each path at the time it was observed, and a directory tree can change as a scan runs.

The desktop runs one scan on the GPUI background executor. It samples a single shared progress snapshot at 100 ms intervals and receives completion through a one-slot channel. It streams file metadata to SQLite and retains at most 200 records while selecting the 100 largest in case saving fails. The large-file threshold is 1 GiB. Exclusions are saved atomically in the app data directory and apply to future scans. The background scan reads a snapshot before walking the tree.

Image records are retained for analysis only while the database writer is available. If indexing fails, the worker drops the transaction and releases the queued image records, then continues collecting the bounded fallback preview. Existing history and a corrupt database file are never replaced as part of fallback. Scan reports retain phase durations and peak candidate-set count for debug logging and reproducible benchmarks; these diagnostics contain no paths and do not change the schema. Successful-write workloads still retain a full image list and one full same-size candidate set, so memory is not globally constant.

The analyzer reads one equal-size candidate set at a time from SQLite. It samples up to 8 KiB from each end with SHA-256, hashes remaining candidates in full, and compares matching candidates byte for byte. For files at most 16 KiB the sample covers every byte in order, so it already is the full hash; no second hash read is needed. Final byte comparison still reopens and revalidates each file. Each analysis set retains its candidates in memory. The analyzer compares available indexed size and timestamps before reading and revalidates native identity and metadata around each read. It captures file identity when verification first opens the file. A replacement that preserves all recorded metadata before that point can pass the initial check. The analyzer skips symlinks and reparse points and counts hard-linked names as one file identity. The traversal benchmark measures throughput, peak worker memory, and cancellation latency on a 100,000-file fixture.

## Saved dashboard and Inspector evidence

Overview loads its own bounded saved snapshot at startup, on entry, after scan completion, and on explicit Refresh. One background job reads the five latest committed scan records and computes file-type totals only for the newest. It uses the existing history and categorization queries; no schema or dependency changes are needed. The job handle cancels obsolete UI continuations when refreshing. Loading clears the previous snapshot so stale findings are not presented as fresh after a failed read. Running transactions remain absent from history; a cancelled scan can be the newest saved scan and is labelled as a partial index.

The dashboard distinguishes verified duplicate groups from heuristic similar-image groups, and separately shows each analysis stage's completion and skipped checks. Its Open, Review, and Scan again buttons carry the row's saved scan or native root. Opening Review also selects that same scan in Results so later sidebar navigation stays consistent. Storage totals are indexed logical bytes by extension, not physical or reclaimable space. Only the category totals and five scan records are retained; category computation still reads the newest scan's file rows off the UI thread.

File Inspector shows extension-based type and the engine's existing cleanup candidate/evidence alongside risk and duplicate evidence. `SavedScan::started_at` supplies a checked conversion of the persisted timestamp to Inspector, Review, and CSV, including times before the Unix epoch. Age-based evidence stays anchored to the scan rather than drifting when history is reopened. Unsaved previews lack a persisted observation time and explicitly show cleanup evidence as unavailable.

Results age filters also use the shared saved time, with checked duration/subtraction. An invalid cutoff clears the file page and surfaces an error; choosing Any age does not require a valid cutoff. Overview's last-scan action explicitly loads its report instead of navigating to whichever history entry Results previously showed. CSV export has one busy flag owned by Results, covering picker and write phases across scan switches; status messages remain attached to the original scan ID, while completion always releases the shared flag. Review's status rules live beside the view in `review/status.rs`, keeping partial scans, unfinished analysis, traversal skips, and per-analysis errors distinguishable. Refresh reloads the current group page and then the first group's previews through the existing background/cancellation path.

## Volume-space snapshot

After a scan finishes and commits, the background job queries capacity and space available to the caller for the selected directory's volume. macOS uses `statfs` and its 64-bit block counts (Darwin's `statvfs` counters are only 32-bit); other Unix targets use `statvfs`. Windows uses [`GetDiskFreeSpaceExW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw), preserving UTF-16 paths and adding the required trailing separator for UNC shares. Windows capacity may reflect per-user quotas. Byte multiplication is checked; zero capacity, inconsistent counters, and OS failures produce an unavailable state rather than fabricated numbers.

Overview presents this as a last-scan snapshot for the named folder's volume, separately from logical scanned bytes. It is not reclaimable space or a physical disk inventory; APFS shared space and quotas can affect the values. Statistics are session-only and are not added to saved scan history or CSV. A failed query never discards the completed scan. Already-cancelled scans skip the query. Native filesystem calls may block on an unresponsive volume; they run off the GPUI thread but cannot be interrupted mid-call. No dependency or database migration is needed.

## SQLite scan index

The schema stores scan summaries and file records in separate tables. Each record has a native path key and a display path. Preserve Unix path bytes or Windows UTF-16 code units in the key so distinct filenames survive lossy display conversion. Store file timestamps as seconds and nanoseconds, including timestamps before the Unix epoch.

The writer enables foreign keys and WAL mode on every connection and records each scan in one transaction. WAL lets readers inspect completed history while a large scan spills uncommitted pages. There is still only one writer; a second scan waits briefly and then reports contention. Completing or cancelling a scan commits its summary and available rows. Dropping an incomplete transaction rolls back its rows. History queries hide running scans. File search checks the entire saved scan for a literal path substring and folds ASCII case. File pages have a stable size order with the native key as a tiebreaker. Desktop queries run in the background with bundled SQLite through `rusqlite`.

File-type totals are computed from the saved file rows when a scan opens. They take about 15 ms for 100,000 files, so no type column is stored.

After exact duplicates are verified, the scan decodes JPEG, PNG, GIF, WebP, BMP, and TIFF files with at most two workers, hashes them, and saves similar-image groups in the same scan transaction. Each image joins the closest earlier reference within 6 bits or starts a new group, so every saved distance is measured to the reference. The v3 migration stores these groups in their own tables, apart from byte-verified duplicates. Byte-identical images appear in both views, labelled as an exact duplicate in one and a distance-0 similar image in the other. Decode failures are counted on the scan. Image analysis and previews share non-following opens, indexed-metadata checks, and native identity checks before and after decoding. Encoded input is limited to 64 MiB, dimensions to 16,384 pixels per side, and the decoder receives a 64 MiB allocation budget. The allocation budget is best-effort for codec internals, not a process RSS guarantee. Reads and grouping check cancellation. Review loads at most 200 members and generates 320×220 PNG thumbnails in the background; GPUI receives thumbnail bytes rather than live paths. A changed, missing, linked, oversized, or undecodable source produces a visible preview error. Switching groups cancels the previous preview request. See [image similarity](image-similarity.md) for how the hash and threshold were chosen.

Each store connection registers the engine's `file_type` as a SQLite function, so file pages filter by the same type names the scan summary shows. Sorting appends one of five constant `ORDER BY` clauses by result-column position, so the default largest-first order keeps using the size index. Type and age filters and other sorts read the whole saved index for each page.

Cleanup candidates are not stored. The store applies the engine's rules to a saved scan's file rows when the Review tab opens, measuring age from the scan's start time. Counts cover the complete scan, while the desktop retains at most 200 candidates per rule. Each file gets the first matching rule: empty file, known temporary name, incomplete download, installer, archive, then a file in a `Downloads` folder unmodified for 90 days. Changing a rule therefore needs no migration, and older scans gain the new candidates.

The v2 migration keeps existing scans and adds duplicate groups and membership. Older scans need a new scan to generate findings. Cancelling verification keeps groups completed before cancellation and marks verification incomplete.


## Incremental hashing and folder observations

Migration v4 adds directory observations and an index for confirmed empty folders. Cancelled scans retain only observed folders. Review pages empty folders in batches of 30; old scans require rescanning. CSV includes empty-folder rows with `entry_kind=directory` and blank logical size.

Migration v5 adds a transactional full-hash cache and per-scan hit/miss counters. Every scan still traverses and indexes fresh metadata. Samples and files smaller than 64 KiB are read directly: measurements showed caching those was slower. Full SHA-256 hashes for larger candidate files are reused only when the algorithm, native path, volume/file identity, length, modification time, creation time, and native change time all agree. The final byte comparison is always performed. Cache errors are skipped-check errors, never matches. Entries unused for more than 20 scan IDs are pruned on commit; rollback discards new cache writes. Image hashes are recomputed.

## Review-only risk and export

Risk assessment combines indexed read-only attributes with conservative, case-insensitive path-component rules. Known system, application, and cloud-managed locations are protected; known personal and development locations are high risk; everything else has unknown risk. This is path evidence, not comprehensive OS/cloud-provider detection. No tier authorizes removal. Risk and its reason appear in Review, File Inspector, and CSV.

Export streams SQLite rows and bounded folder pages into a uniquely created temporary file beside the destination. Flush and sync precede atomic replacement; failures remove the temporary file and preserve the previous destination. Exports refuse the selected tree and the app-data directory, including ordinary symlink-parent aliases, and defuse formula-like CSV cells. Native paths are retained internally; CSV paths are display text, so CSV is not a lossless native-path backup.

## Confirmed cleanup and recovery (v0.3)

`workspace/cleanup.rs` owns bounded queue/history state and confirmation controls; `cleanup/job.rs` coordinates blocking evidence, journal and native operations. `crates/cleanup-engine` owns native filesystem proof and adapters. `crates/scan-store/src/cleanup.rs` and numbered migration 006 own the durable state machine and parameterized SQL. The schema is now version 6; recovery history survives removal of scan index records.

All mutation work runs off GPUI. One app-owned OS file lock serializes cleanup/recovery across processes. SQLite WAL with synchronous FULL persists intent before native action and receipt before verification; there is no distributed transaction with the filesystem. Abandoned moving/restoring states become Attention on the next lock acquisition. No automatic retries occur. Current exclusions are read before queueing and each move, and scan/picker/exclusion controls cannot overlap cleanup in one workspace.

The queue is capped at 200; history uses 30-item pages. Snapshot hashes are streamed, UI updates are bounded, and cancellation leaves unstarted items queued. Filesystem calls already in progress cannot be interrupted. [Cleanup and recovery](cleanup-recovery.md) defines evidence, kept-copy reservations, restoration, space reporting, and acceptance limits.
