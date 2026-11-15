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

Use **Scan exclusions** on Overview to select files or folders to skip in future scans. After indexing, Storage Clinic checks same-size files for exact duplicates. Use **Duplicates only** in Scan Results to review matches and see their SHA-256 hashes in File Inspector. Reopen older scans for their indexed files, or run a new scan to add duplicate findings.

The app saves file paths and metadata locally under `~/Library/Application Support/Storage Clinic` on macOS or `%LOCALAPPDATA%\Storage Clinic` on Windows. If saving fails, the results view keeps the 100 largest files in a temporary preview.

## Development

- [`apps/desktop`](apps/desktop) contains native startup, scan job coordination, and the Overview, Results, History, and Inspector views.
- [`crates/scan-engine`](crates/scan-engine) streams filesystem metadata independently of GPUI. Run its fixture tests with `cargo test --locked -p scan-engine`.
- [`crates/scan-store`](crates/scan-store) manages the SQLite schema, scan writes, and paged queries. Run its tests with `cargo test --locked -p scan-store`.
- [`docs/architecture.md`](docs/architecture.md) records the folder structure, dependency decisions, and scanning boundaries.
- [`docs/progress.md`](docs/progress.md) tracks implementation and build verification.
- [`docs/tasks.md`](docs/tasks.md) tracks release scope and acceptance from v0.1 through v1.0. [`AGENTS.md`](AGENTS.md) documents folder ownership and coding guidelines. [`CHANGELOG.md`](CHANGELOG.md) records user-facing changes.
- [`docs/benchmarks.md`](docs/benchmarks.md) describes how to reproduce the traversal benchmark and interpret its results.
- `references/` holds optional local clones of the research projects and is ignored by Git.

The app shows the first scan error alongside the error counts. Logical bytes include hard-linked names. Duplicate verification counts distinct file identities. It reads file content to establish a match and checks identity and metadata around those reads. Native Windows build and GUI verification are pending.
