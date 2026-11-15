# Development tasks

## Completed

- [x] Build the native GPUI Kit shell on Apple Silicon.
- [x] Add a read-only scanner and exercise it with filesystem fixtures.
- [x] Connect folder selection, scan progress, cancellation, and file inspection.
- [x] Confirm that the macOS app scans the folder selected in the native picker.

## In progress

- [ ] Persist scan history and streamed file metadata to an app-owned SQLite database.
- [ ] Open saved scans and search their file metadata from the results view.

## Remaining for v0.1

- [ ] Exercise picker and scan cancellation and mouse and keyboard navigation on macOS.
- [ ] Connect configurable exclusions and virtualize the results table.
- [ ] Verify exact duplicates with staged hashing and file-identity revalidation.
- [ ] Build and smoke-test the read-only workflow on Windows x64.
- [ ] Measure scan throughput, peak memory, and cancellation latency on a representative fixture tree.
