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

In the window, choose **Overview → Choose folder**. The dashboard shows scan progress and offers **Cancel scan**. **Scan Results** lists the indexed files in descending size order, 100 per page, and flags files of at least 1 GiB. Use **Search file paths** to search the full saved scan by filename or folder. Click a row or focus the table and use the arrow keys to inspect metadata. **Scan History** opens past scans, including partial results from cancelled scans.

The app saves file paths and metadata locally under `~/Library/Application Support/Storage Clinic` on macOS or `%LOCALAPPDATA%\Storage Clinic` on Windows. If saving fails, the results view keeps the 100 largest files in a temporary preview.

## Development

- [`apps/desktop`](apps/desktop) contains native startup, scan job coordination, and the Overview, Results, History, and Inspector views.
- [`crates/scan-engine`](crates/scan-engine) streams filesystem metadata independently of GPUI. Run its fixture tests with `cargo test --locked -p scan-engine`.
- [`crates/scan-store`](crates/scan-store) manages the SQLite schema, scan writes, and paged queries. Run its tests with `cargo test --locked -p scan-store`.
- [`docs/architecture.md`](docs/architecture.md) records the folder structure, dependency decisions, and scanning boundaries.
- [`docs/progress.md`](docs/progress.md) tracks implementation and build verification.
- [`docs/tasks.md`](docs/tasks.md) tracks remaining v0.1 work. [`CHANGELOG.md`](CHANGELOG.md) records user-facing changes.
- `references/` holds optional local clones of the research projects and is ignored by Git.

The app shows the first filesystem error alongside the total error count. Logical bytes are counted per path, including hard-linked names. Exact duplicate verification and configurable exclusions are planned for v0.1. Launching, scanning the selected folder, and reopening a saved scan after restart have been verified on macOS. Interactive macOS search, cancellation and navigation checks, plus Windows verification, are pending.
