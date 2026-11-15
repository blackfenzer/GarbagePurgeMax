# Storage Clinic

A native desktop workspace for understanding disk usage and inspecting why files were flagged. The current build scans one selected folder and previews its largest files. Scans are read-only and local.

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

In the window, choose **Overview → Choose folder**. The dashboard shows scan progress and offers **Cancel scan**. On completion, **Scan Results** previews up to 100 files in descending size order, with a **Large file** finding for files at or above 1 GiB. Select a filename to inspect its metadata. A cancelled scan displays the partial results. Use **View sample results** before scanning to explore example findings.

## Development

- [`apps/desktop`](apps/desktop) contains native startup, scan job coordination, and the Overview, Results, and Inspector views.
- [`crates/scan-engine`](crates/scan-engine) streams filesystem metadata independently of GPUI. Run its fixture tests with `cargo test --locked -p scan-engine`.
- [`docs/architecture.md`](docs/architecture.md) records the folder structure, dependency decisions, and scanning boundaries.
- [`docs/progress.md`](docs/progress.md) tracks implementation and build verification.
- [`docs/tasks.md`](docs/tasks.md) tracks remaining v0.1 work. [`CHANGELOG.md`](CHANGELOG.md) records user-facing changes.
- `references/` holds optional local clones of the research projects and is ignored by Git.

This build keeps one scan in memory and shows only the first filesystem error alongside the total error count. Logical bytes are counted per path, including hard-linked names. Exact duplicate verification, SQLite history, configurable exclusions in the UI, and the searchable file index are planned for v0.1. Launching and scanning the selected folder have been verified on macOS. Interactive macOS cancellation and inspector checks, plus Windows verification, are pending.
