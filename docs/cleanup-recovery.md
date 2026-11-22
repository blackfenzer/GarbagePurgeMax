# Reviewed cleanup and recovery — v0.3

Scans, analysis and previews remain read-only. Cleanup is a separate, explicitly confirmed workflow. Storage Clinic never permanently deletes files, empties Trash or automatically retries interrupted actions. This is a development implementation, pending native platform acceptance.

## Review and execute

1. Open a saved scan in Review. In Exact duplicates select **Keep this copy**, then queue unwanted copies. Metadata candidates and observed empty folders can also be queued. Similar-image matches alone never qualify.
2. Inspect Cleanup Queue: original paths, reasons, kept copies, logical sizes and allocation evidence. Remove any unwanted action. The queue persists across restarts and holds at most 200 items.
3. Choose **Review confirmation**, read the consequences, then **Confirm move to Trash / Recycle Bin**. Confirmation covers the exact current queue; another instance changing membership invalidates it.
4. Open Recovery History for individual outcomes. Restore requires its own confirmation. Cancellation stops before the next action; a native call already executing must finish and be recorded.

**Move to Trash** in Files, a Tree row or the Space Map inspector handles one chosen file or folder directly. A folder's contents are listed first (size, file and folder counts, largest files, links and read-only items). Confirming journals that one item and moves only it; anything already queued keeps waiting for its own confirmation. It is restored from Recovery History like any queued item.

The scan index is historical evidence, not a live filesystem view. Rescan after cleanup or restore to refresh Results. No real user files are touched by automated fixture tests.

## Eligibility and proof

Queue preparation requires a saved file/empty-directory observation, unchanged indexed metadata, a path inside the scanned root, and current readable exclusion settings. The scan root and app data are ineligible. Before every move, current exclusions, native identity/version, size, SHA-256, risk and directory emptiness are checked again. A chosen non-empty folder instead carries a digest of every entry's relative path, kind, size, modified time and (on Unix) inode, taken without following links; any change inside after the preview refuses the move, and the Trash copy must match the same digest. Cloud placeholders anywhere inside are refused; hard links and read-only items inside are listed, not blocked, and risk is assessed for the chosen folder. Hashing streams 64 KiB buffers and checks cancellation. Symlinks/reparse points and linked ancestors are rejected; files must have exactly one hard link. Protected/read-only locations and detected cloud placeholders are blocked. macOS checks Darwin SF_DATALESS; Windows checks offline/recall/reparse attributes before content reads. Path-based protection and these attributes are conservative, not exhaustive provider detection.

Exact-duplicate cleanup also requires a selected distinct member of the saved verified group, matching the saved SHA-256 and fresh byte comparison. The journal reserves kept paths and identities across queued, moving, trashed, restoring and uncertain operations. A kept copy cannot become a cleanup target until those references are restored/reconciled or safely removed before action. External applications can still edit files: close applications using the selected items.

OS Trash calls accept paths. Checks immediately before a call and receipt verification afterward cannot make the entire operation atomic against hostile concurrent renames or writes. This workflow is not a security boundary against an attacker controlling the tree. A discrepancy produces Needs inspection rather than a success claim.

## Journal and interruptions

Migration 006 adds operation records independent of scan foreign keys. Native paths remain BLOBs; serialized evidence holds identity, version, content hash and allocation. An app-owned OS file lock serializes cleanup/restore across processes and releases on process exit. SQLite WAL with synchronous FULL records intent before any native action. Failure to save intent prevents action; failure to save a receipt/outcome stops the batch.

| State | Meaning and next step |
| --- | --- |
| Queued | Reviewed evidence only; no mutation yet. Remove or confirm. |
| Moving | Durable intent exists; native outcome may be incomplete. |
| In Trash / Recycle Bin | Saved receipt and moved contents verified. Restore while still retained. |
| Validation failed | No native action was invoked for this item. Rescan and review. |
| Restoring | Durable restore intent exists; outcome may be incomplete. |
| Restored | Original identity/content verified at the original path. |
| Needs inspection | Native failure, interrupted operation or unverifiable result. Inspect both original and OS Trash. |
| Original verified in place | Read-only reconciliation proved original identity/content and absence of a saved Trash item. |
| Removed from queue | No filesystem action performed. |

The filesystem and SQLite cannot share one transaction. A crash between native movement and receipt persistence may leave an item only discoverable through the OS Trash. After acquiring the exclusive lock, abandoned Moving/Restoring records become Needs inspection; they never resume automatically. Other claimed but unstarted items may also require read-only reconciliation. If restored manually, **Verify original location** checks identity/content and the saved Trash location before clearing the active reservation. It does not blindly dismiss unresolved history.

History uses 30-item pages. Records outlive scan deletion. Keep the app-data journal and Trash contents if recovery is needed; a journal alone is not a backup.

## Native adapters and restore

macOS uses Foundation `NSFileManager` Trash, retains its resulting native URL, and restores with atomic `renameatx_np(RENAME_EXCL)`. Windows uses `IFileOperation` on an owned STA thread, requests recycling, rejects a delete callback without recycling flags, and records the returned Shell item identifier and filesystem path. No permanent-delete fallback is provided. Native rejection or missing receipt is shown per item and treated as uncertain; the application does not claim every volume supports recycling.

Restore verifies the retained identity and contents, requires an existing non-linked parent, and refuses an occupied destination. macOS has an atomic no-replace primitive. Windows requests collision renaming rather than overwriting; a racing collision that causes a different destination is reported as Needs inspection with the resulting path. There is no cross-volume copy fallback. Missing/changed Trash items, removed parent directories and permission failures remain visible. Emptying OS Trash independently destroys the recovery source.

## Space reporting

Moving into Trash normally retains allocation, so the queue reports **0 B expected immediate release**, separately from logical bytes. Allocated bytes come from Unix blocks × 512 or Windows allocation metadata and form a potential upper bound after OS Trash is emptied. Clones, snapshots, sparse/compressed files, metadata, quotas and provider-managed storage make actual reclamation uncertain. Hard links and detected placeholders are blocked instead of assigned speculative savings. A missing or overflowing allocation total is unknown.

Per-item before/after available-volume readings are observations, not proof cleanup caused the difference. Other applications can allocate/free space concurrently. Storage Clinic never empties Trash to measure reclamation.

## Verification and remaining acceptance

The ordinary workspace suite uses disposable directories and an injected Trash adapter to test queue persistence, kept-copy reservations, changed sources/keepers, directory changes, exclusions, cancellation, partial failures, missing/modified receipts, lock contention, interrupted intents, journal-write failures, and restore conflicts. The macOS native no-overwrite rename primitive is separately tested inside a disposable directory.

The opt-in real native round-trip test is:

```sh
cargo test --locked -p cleanup-engine native_trash_round_trip -- --ignored --nocapture
```

On November 18 the macOS attempt was rejected by filesystem permissions while moving the generated fixture. It did not return a receipt, and no successful native Trash/restore round trip is claimed. Windows target compilation/runtime, GUI acceptance, mounted volumes, native crash/restart tests and disabled Recycle Bin behavior remain pending. Only the Apple Silicon Rust target is installed here. See [progress](progress.md) for executed checks and [release checklist](release.md) for the manual fixture procedure. Do not treat simulated-backend success as native acceptance.
