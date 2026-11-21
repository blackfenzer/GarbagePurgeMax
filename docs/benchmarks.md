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

## Small-file follow-up · November 16, 2026

For files at most 16 KiB the sample already hashes every byte in order. Reusing it removes a redundant open/read/hash while preserving the final byte comparison and version checks. Zero-use cache counters also skip SQL updates.

Run the focused benchmark with:

```sh
cargo test --locked --release -p scan-engine benchmark_complete_sample_reuse -- --ignored --nocapture
```

Six rounds alternate the strategy order over the same 1,000 files of 8 KiB. Both check native identity around reads and assert the exact SHA-256. Cumulative hashing-stage time was **0.262 s with the second read** and **0.134 s with sample reuse**. Fixture creation, SQLite, grouping, and final byte comparison are outside this focused measurement; it does not measure total scan speed.

Separate duplicate-heavy 100,000-file runs on the same Mac passed their assertions but varied substantially:

| Metric | Before follow-up | After follow-up |
| --- | ---: | ---: |
| Fixture creation | 6.3 s | 9.2 s |
| Index, first scan | 0.90 s | 1.03 s |
| Verification + commit, first | 10.80 s | 16.73 s |
| Total, first scan | 11.70 s | 17.75 s |
| Total, repeat scan | 9.70 s | 16.15 s |
| Cancellation, indexing / verification | 48.3 / 75.8 ms | 49.7 / 74.7 ms |
| Peak process RSS | 16,640 KiB | 16,752 KiB |
| Whole-process CPU, user / system | 3.38 / 27.68 s | 4.01 / 34.99 s |

Fixture creation also slowed, showing environmental variability beyond the changed hashing stage. These unpaired end-to-end timings do not establish a speedup or isolate the cause of the difference. The earlier small-file performance regression remains open; neither privileged cold-cache measurements nor device-level I/O tracing was performed.

## Phase profiling and larger candidate sets · November 17, 2026

The scan report now records setup, traversal/indexing, candidate-query, duplicate-verification, duplicate-write, image-analysis, commit, and volume-query wall times, plus the largest same-size candidate set. Release benchmarks print these fields. For an ordinary development run, use `RUST_LOG=storage_clinic=debug cargo run --locked -p storage-clinic`. Diagnostics contain durations/counts, not paths, and are not saved to SQLite. Verification includes native version checks, sample/full hashing, optional cache access, and byte comparison; these substeps are not individually timed. Small bookkeeping and cleanup costs are outside the named phase totals.

Measured on Apple Silicon/macOS 27.2 using the existing 100,000-file fixtures in separate sequential release processes. No compiler or other benchmark was launched concurrently. First scans follow fixture creation; these are warm-cache observations, not cold-disk measurements.

| Metric | Mostly unique | Duplicate-heavy |
| --- | ---: | ---: |
| Total, first / repeat | 3.61 / 3.62 s | 7.76 / 7.93 s |
| Indexing, first / repeat | 0.687 / 0.676 s | 0.673 / 0.675 s |
| Candidate queries, first / repeat | 0.061 / 0.062 s | 0.082 / 0.085 s |
| Duplicate verification, first / repeat | 2.797 / 2.825 s | 6.508 / 6.668 s |
| Duplicate writes, first / repeat | <0.001 / <0.001 s | 0.364 / 0.366 s |
| Commit, first / repeat | 0.056 / 0.059 s | 0.124 / 0.132 s |
| Largest same-size set | 100 files | 100 files |
| Cancellation, indexing / verification | 44.9 / 73.8 ms | 45.0 / 87.3 ms |
| Peak process RSS | 17,728 KiB | 16,848 KiB |
| Whole-process CPU, user / system | 1.76 / 16.01 s | 2.71 / 23.40 s |

Image and volume queries took less than 0.1 ms on these fixtures; there are no decodable image inputs. Page/search queries remained below 49 ms. CPU/RSS cover fixture generation, scans, query/cancellation checks, and removal; they are not phase-specific CPU or disk-I/O statistics.

File verification accounts for about 84% of the duplicate-heavy scan. Candidate SQL queries are a small fraction on this workload, so this result does not justify a new index or migration. The previous 17.75 / 16.15 s result did not reproduce. Since only diagnostics and the unavailable-storage memory path changed, the difference is not evidence of a normal-path speedup or proof that the variability is resolved.

### One large candidate set

Run the new ignored test in its own release-test process, using the executable path printed by `cargo test --locked --release -p storage-clinic --no-run`:

```sh
TIMEFMT='Peak RSS: %M KiB; CPU: %U user, %S system' time <executable> --ignored --exact workspace::scan::job::tests::benchmark_large_candidate_set --nocapture
```

The fixture writes 20,000 separate files of 4 KiB in one directory. Half are byte-identical; the remainder have distinct leading bytes. All files enter one size bucket. Each of three scans must persist exactly one verified group of 10,000 distinct files and report zero duplicate errors.

| Pass | Total | Candidate queries | File verification |
| --- | ---: | ---: | ---: |
| 1 | 1.054 s | 7.79 ms | 0.866 s |
| 2 | 1.076 s | 7.88 ms | 0.861 s |
| 3 | 1.037 s | 7.93 ms | 0.849 s |

Peak process RSS was 26,800 KiB; whole-process CPU was 0.50 s user / 4.42 s system. This establishes behavior for the tested 20,000-member bucket, not a general memory ceiling: candidates and image records still scale with input size. Large photo libraries, adversarial hash collisions, cold-cache/device-I/O tracing, and GUI frame timing need separate measurements. The storage-failure cleanup of retained image records does not affect these successful-save measurements.

## Space Map folder totals

Synthetic 1,000,000-file scan (20 top-level folders × 400 subfolders), release build, macOS 27.2 on Apple Silicon. Run with `cargo test --release --locked -p scan-store --test store folder_summary_speed -- --ignored --nocapture`.

| Folder opened | Files beneath | Time |
| --- | --- | --- |
| Scan root | 1,000,000 | 350 ms |
| Top-level folder | 50,000 | 19 ms |
| Leaf folder | 2,500 | 1.2 ms |
