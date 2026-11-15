# Storage Clinic

A native desktop workspace for understanding disk usage and inspecting why files were flagged. The current build scans a selected folder and keeps a local history of scan metadata. Scans are read-only for the selected files.

## Build and run

Install the current stable Rust toolchain via [rustup](https://rustup.rs/). GPUI Kit's documented baseline is Rust 1.92. A first build needs access to the Cargo registry.

| Target | Build prerequisites |
| --- | --- |
| macOS on Apple Silicon | macOS 15 or later and Xcode Command Line Tools. Check `xcode-select -p`. |
| Windows 11 x64 | MSVC Rust target (`x86_64-pc-windows-msvc`), Visual Studio 2022 C++ Build Tools with a Windows SDK, and CMake on `PATH`. |

Run these from the repository root:

```sh
rustc --version
cargo --version
cargo fmt --all -- --check
cargo check --locked --workspace
cargo test --locked --workspace
cargo run --locked -p storage-clinic
```

The workspace lockfile pins the resolved dependencies. On macOS, GPUI Kit 0.9.1 compiles its Metal shaders at runtime. This build used Apple’s Command Line Tools without full Xcode. If a Homebrew `rustup` install exposes `rustup` but `cargo` is missing from your shell, run `export PATH="$(dirname "$(rustup which cargo)"):$PATH"` before the commands above. Set `RUST_LOG` to change the log level.

In the window, choose **Overview → Choose folder**. The dashboard shows scan progress and offers **Cancel scan**. **Scan Results** lists the indexed files in descending size order, 100 per page, flags files of at least 1 GiB, and totals files and logical size by type. Use **Search file paths** to search the full saved scan by filename or folder. Click a row or focus the table and use the arrow keys to inspect metadata. **Scan History** opens past scans, including partial results from cancelled scans.

Open **Review** to compare groups from the scan open in Scan Results. **Exact duplicates** shows byte-identical files with their SHA-256 evidence. **Similar images** shows JPEG, PNG, GIF, WebP, BMP, and TIFF images that look alike, side by side, with each image's difference from the group's reference image. Similar images are a heuristic and are never treated as copies. **Cleanup candidates** groups empty files, known temporary files, incomplete downloads, installers, archives, and files in a Downloads folder unmodified for 90 days, each with the reason it matched. Candidates are for review only.

Use **Scan exclusions** on Overview to select files or folders to skip in future scans. After indexing, Storage Clinic checks same-size files for exact duplicates. Use **Duplicates only** in Scan Results to review matches and see their SHA-256 hashes in File Inspector. Reopen older scans for their indexed files, or run a new scan to add duplicate findings.

The app saves file paths and metadata locally under `~/Library/Application Support/Storage Clinic` on macOS or `%LOCALAPPDATA%\Storage Clinic` on Windows. If saving fails, the results view keeps the 100 largest files in a temporary preview.

## Development

- [`apps/desktop`](apps/desktop) contains native startup, scan job coordination, and the Overview, Results, History, and Inspector views.
- [`crates/scan-engine`](crates/scan-engine) streams filesystem metadata independently of GPUI. Run its fixture tests with `cargo test --locked -p scan-engine`.
- [`crates/scan-store`](crates/scan-store) manages the SQLite schema, scan writes, and paged queries. Run its tests with `cargo test --locked -p scan-store`.
- [`docs/architecture.md`](docs/architecture.md) records the folder structure, dependency decisions, and scanning boundaries.
- [`docs/progress.md`](docs/progress.md) tracks implementation and build verification.
- [`docs/tasks.md`](docs/tasks.md) tracks release scope and acceptance from v0.1 through v1.0. [`AGENTS.md`](AGENTS.md) documents folder ownership and coding guidelines. [`CHANGELOG.md`](CHANGELOG.md) records user-facing changes.
- [`docs/benchmarks.md`](docs/benchmarks.md) describes how to reproduce the traversal and end-to-end benchmarks and interpret their results.
- [`docs/image-similarity.md`](docs/image-similarity.md) compares perceptual image hashes for the planned Similar Images view.
- `references/` holds optional local clones of the research projects and is ignored by Git.

To generate the synthetic tree used for the desktop smoke test, run `cargo run --locked --release -p scan-engine --example smoke_fixture -- <new folder>`. On macOS, run `chmod 755 <folder>/locked` before deleting it.

## v0.1 status and limits

| Platform | Build and tests | Desktop smoke test |
| --- | --- | --- |
| macOS 27.2, Apple Silicon | Passed locally: rustfmt, strict Clippy, 33 Rust tests, and the schema tests | Passed on 120,023 generated files |
| Windows x64 | Passed on GitHub Actions `windows-2022` | Needs testing on Windows 11 |

On a 100,000-file tree, the release pipeline scanned, indexed, and verified duplicates in 3.2 s for mostly unique files and 8.1 s for a duplicate-heavy tree. Peak memory was about 16.5 MiB, review queries took under 47 ms, and cancellation returned within 14 ms. Traversal alone averaged 267,804 files/s. See [benchmarks](docs/benchmarks.md) for the method and the [architecture](docs/architecture.md) for the design.

- Scans read files and never change them. Cleanup arrives in v0.3.
- File types come from extensions only. Duplicates are exact byte matches. Similar images use a 64-bit difference hash, so heavy crops, mirrored, and rotated copies are not found, and HEIC and RAW photos are not read.
- Logical sizes include every hard-linked name. Duplicate verification counts each file identity once. Physical space on disk is not estimated yet.
- Each scan holds the database write lock until it finishes, so a second app instance cannot save a scan at the same time. The app quits when its window closes.
- Older scans need a new scan to add duplicate findings. The app shows the first scan error alongside the error counts.
- Release builds are not packaged, signed, or notarized yet. Run the app with `cargo run`.
