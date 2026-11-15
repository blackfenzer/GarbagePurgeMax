# Development tasks

## Completed

- [x] Build the native GPUI Kit shell on Apple Silicon.
- [x] Add a read-only scanner and exercise it with filesystem fixtures.
- [x] Connect folder selection, scan progress, cancellation, and file inspection.
- [x] Confirm that the macOS app scans the folder selected in the native picker.
- [x] Add a virtualized results table and search within the current scan preview.
- [x] Add and test the SQLite schema, scan history query, and paginated file search.
- [x] Add bundled `rusqlite` and persist streamed scan records to an app-owned SQLite database.
- [x] Open saved scans and search their file metadata from the results view.
- [x] Verify saved scan history across a macOS app restart.

## Remaining for v0.1

- [ ] Exercise picker and scan cancellation, full-scan search, and mouse and keyboard navigation on macOS.
- [ ] Connect configurable scan exclusions.
- [ ] Verify exact duplicates with staged hashing and file-identity revalidation.
- [ ] Build and smoke-test the read-only workflow on Windows x64.
- [ ] Measure scan throughput, peak memory, and cancellation latency on a representative fixture tree.
