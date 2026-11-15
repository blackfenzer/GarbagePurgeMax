# Scan engine benchmark

Run the release example from the repository root.

```sh
cargo run --locked --release -p scan-engine --example scan_benchmark
```

The example creates a temporary tree with 100,000 files across 1,000 leaf directories. Each leaf contains 25 files of each logical size: 0, 128, 1,024, and 4,096 bytes. It sets each file's length without writing payload data, for 131,200,000 logical bytes across the tree. The directory count is 1,111, including the root. The example removes the tree after both workers exit.

- **Traversal throughput** times five calls to `scan_directory` with an empty exclusion list and a visitor that drops each streamed record. The aggregate is 500,000 files divided by the total scan time. Fixture creation, result printing, and cleanup are outside that interval. The filesystem cache starts in the state left by fixture creation.
- **Peak process memory** is the maximum resident set size of the separate traversal worker as reported by the macOS Zsh timer. It includes worker startup and all five passes. The parent creates the fixture, and another worker measures cancellation. On other platforms the example reports throughput and cancellation latency.
- **Cancellation latency** measures 31 runs from immediately before another thread sets the cancellation flag to the return of `scan_directory`. The visitor pauses on its first file record until it receives confirmation that the flag was set, guaranteeing the scan is in progress. The measurement includes waking the visitor and returning from the scanner.

The example exercises the scan engine's metadata traversal. Desktop workflows, SQLite writes, and duplicate hashing need separate measurements.

## macOS release run

Measured on Apple Silicon with macOS 27.2, APFS, and Rust 1.100.0.

| Metric | Result |
| --- | ---: |
| First traversal pass | 285,599 files/s |
| Range across five passes | 249,222–285,599 files/s |
| Aggregate traversal throughput | 267,804 files/s |
| Peak traversal worker RSS | 2,272 KiB |
| Cancellation, minimum | 4.4 µs |
| Cancellation, median | 6.5 µs |
| Cancellation, maximum | 10.6 µs |

# End-to-end benchmark

These ignored tests run the desktop app's own scan pipeline: traversal, SQLite indexing, staged duplicate verification, and the commit. Build the release test binary, then run each input on its own so the peak RSS belongs to it.

```sh
cargo test --locked --release -p storage-clinic --no-run
# Use the "Executable unittests src/main.rs" path that cargo prints.
TIMEFMT='Peak RSS: %M KiB' time <executable> --ignored --exact workspace::scan::tests::benchmark_mostly_unique_end_to_end --nocapture
TIMEFMT='Peak RSS: %M KiB' time <executable> --ignored --exact workspace::scan::tests::benchmark_duplicate_heavy_end_to_end --nocapture
```

Each input has 100,000 files in 1,000 folders. Files in a folder share one size, 4,096 bytes plus the folder number, so both inputs produce 1,000 same-size candidate buckets and about 460 MB of written data. In the mostly unique input, every file differs in its first 8 bytes. In the duplicate-heavy input, each folder holds two contents of 50 identical files, for 2,000 verified groups. The database and fixture live in a temporary directory.

- **Scan phases** split at the moment the pipeline reports that duplicate verification has started. The second phase includes the SQLite commit.
- **Review queries** time the same store calls that Scan Results uses, each requesting 101 rows.
- **Cancellation** sets the flag from the progress callback, once 50,000 files are indexed or verification starts, and times until the pipeline returns with the partial scan saved.
- **Peak RSS** covers the whole test process: fixture creation, two scans, the queries, and both cancellations.

## macOS release run

Measured on Apple Silicon with macOS 27.2, APFS, and Rust 1.100.0. The first scan follows fixture creation, so file data is likely cached, and the repeat scan matched it. A cold-cache run needs a privileged cache purge and was not recorded.

| Metric | Mostly unique | Duplicate-heavy |
| --- | ---: | ---: |
| Traversal and SQLite index | 0.52 s | 0.50 s |
| Duplicate verification and commit | 2.65 s | 7.58 s |
| Total scan, first pass | 3.17 s | 8.08 s |
| Total scan, repeat pass | 3.10 s | 8.08 s |
| Verified duplicate groups | 0 | 2,000 |
| First results page | 0.2 ms | 0.2 ms |
| Last results page, offset 99,900 | 31.1 ms | 31.0 ms |
| Path search, first page | 44.8 ms | 45.5 ms |
| Path search, last page | 46.2 ms | 46.1 ms |
| Duplicates-only first page | 0.1 ms | 0.1 ms |
| File-type totals | 14.5 ms | 14.3 ms |
| Cancel during indexing | 8.3 ms | 8.3 ms |
| Cancel during verification | 13.5 ms | 11.7 ms |
| Peak process RSS | 16,656 KiB | 16,864 KiB |

These queries run off the UI thread. In the interactive smoke test on 120,023 files, the window stayed responsive during scanning, paging, and search, but frame times were not measured.
