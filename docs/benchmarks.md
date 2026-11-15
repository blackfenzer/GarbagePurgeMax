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
