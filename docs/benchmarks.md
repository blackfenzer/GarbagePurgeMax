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
TIMEFMT='Peak RSS: %M KiB' time <executable> --ignored --exact workspace::scan::job::tests::benchmark_mostly_unique_end_to_end --nocapture
TIMEFMT='Peak RSS: %M KiB' time <executable> --ignored --exact workspace::scan::job::tests::benchmark_duplicate_heavy_end_to_end --nocapture
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


## Integrated hardening pass · November 16, 2026

Measured on the same Apple Silicon/macOS 27.2 development machine with Rust 1.100.0, in separate release-test processes. All three benchmark assertions passed. The first scan follows fixture creation and is **not** a cold filesystem-cache measurement. No privileged cache purge or device-level I/O tracing was performed.

| Metric | Mostly unique, 100,000 files | Duplicate-heavy, 100,000 files |
| --- | ---: | ---: |
| Traversal + SQLite index, first | 0.70 s | 0.73 s |
| Verification + commit, first | 3.45 s | 11.59 s |
| Total, first | 4.15 s | 12.33 s |
| Total, repeat | 3.87 s | 16.32 s |
| Full-hash cache hits / misses | 0 / 0 | 0 / 0 |
| First results page | 0.1 ms | 0.1 ms |
| Last results page | 31.2 ms | 32.9 ms |
| Path search, first / last page | 43.7 / 45.3 ms | 54.8 / 54.8 ms |
| File-type totals | 14.2 ms | 22.2 ms |
| Cancel during indexing | 35.0 ms | 52.9 ms |
| Cancel during verification | 77.3 ms | 84.9 ms |
| Peak process RSS | 17,536 KiB | 17,712 KiB |
| Whole-process CPU, user / system | 1.86 / 16.39 s | 4.01 / 33.78 s |

CPU and RSS include fixture creation, both scans, queries, cancellation checks, and fixture removal. They are not phase-specific CPU or disk-bandwidth measurements. Timing variation and the slower duplicate-heavy repeat remain a performance concern; these results do not establish a speedup over the earlier baseline. The integrated build adds directory observations, WAL durability, bounded review data, and cache bookkeeping. Their individual contributions to the difference have not been isolated.

These small-file fixtures contain files below 64 KiB, so they deliberately bypass the cache. An initial implementation caching samples regressed the mostly-unique test to 4.38 s first / 4.52 s repeat with 100,000 cache hits. Restricting caching to large full hashes avoids that needless database traffic. A pre-WAL measurement after this change was 3.47 / 3.45 s; the table above is the final integrated measurement.

### Large-file cache effectiveness

Run the third ignored benchmark in its own process:

```sh
TIMEFMT='Peak RSS: %M KiB; CPU: %U user, %S system' time <executable> --ignored --exact workspace::scan::job::tests::benchmark_large_cached_duplicates --nocapture
```

Eight separately written 32 MiB files (256 MiB total) contain identical bytes. Both passes must find one verified duplicate group, and the repeat must reuse all eight full hashes while still byte-comparing the files.

| Metric | First scan | Repeat scan |
| --- | ---: | ---: |
| Total | 0.167 s | 0.035 s |
| Full-hash cache hits | 0 | 8 |
| Full-hash cache misses | 8 | 0 |

The repeat was about 4.8× faster on this warm synthetic input. Peak process RSS was 45,104 KiB, including the 32 MiB fixture-generation buffer; CPU time was 0.12 s user / 0.14 s system. This is not a general throughput guarantee for real disks or photo libraries.

The release test build succeeded with a local Rust-toolchain warning: `rust-objcopy` could not load `libLLVM.dylib` while stripping symbols. The executables still built and all benchmark assertions ran. `/usr/bin/time -l` could not read `kern.clockrate` in the sandbox, so final resource measurements use Zsh's built-in `time` instead.
