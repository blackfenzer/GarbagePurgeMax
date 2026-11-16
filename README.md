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

In the window, choose **Overview → Choose folder**. The dashboard shows scan progress and offers **Cancel scan**. **Scan Results** lists the indexed files in descending size order, 100 per page, flags files of at least 1 GiB, and totals files and logical size by type. Use **Search file paths** to search the full saved scan by filename or folder, and the dropdowns to filter by file type or age and change the sort order. **Export CSV** saves every indexed file and observed empty folder, with type, size, timestamps, groups, cleanup evidence, risk, and scan status. Choose a destination outside the scanned folder and app-data directory. Directory rows have a blank logical size. **Scan again** in Scan History repeats a scan of the same folder with the current exclusions. Click a row or focus the table and use the arrow keys to inspect metadata. **Scan History** opens past scans, including partial results from cancelled scans.

Open **Review** to compare groups from the scan open in Scan Results. **Exact duplicates** shows byte-identical files with their SHA-256 evidence. **Similar images** shows JPEG, PNG, GIF, WebP, BMP, and TIFF images that look alike, side by side, with each image's difference from the group's reference image. Similar images are a heuristic and are never treated as copies. **Cleanup candidates** groups empty files, known temporary files, incomplete downloads, installers, archives, and files in a Downloads folder unmodified for 90 days, each with the reason it matched. **Empty folders** lists folders observed without any entries, including hidden ones. All findings show conservative risk labels and remain review-only. Review retains at most 200 cards per group; CSV includes all indexed files. Image cards use bounded thumbnails, and report missing or changed sources.

Use **Scan exclusions** on Overview to select files or folders to skip in future scans. After indexing, Storage Clinic checks same-size files for exact duplicates. Use **Duplicates only** in Scan Results to review matches and see their SHA-256 hashes in File Inspector. Reopen older scans for their indexed files, or run a new scan to add duplicate findings.

Repeat scans reuse full duplicate hashes for files at least 64 KiB only after revalidating native identity and timestamps. Every duplicate match is still compared byte for byte. Cache hit/miss counts appear in Scan Results.

The app saves file paths and metadata locally under `~/Library/Application Support/Storage Clinic` on macOS or `%LOCALAPPDATA%\Storage Clinic` on Windows. If saving fails, the results view keeps the 100 largest files in a temporary preview.

After a scan, Overview shows the containing volume's OS-reported capacity and available space. This is a snapshot from the latest scan in this session, separate from folder logical size; it may reflect quotas and shared storage. It is not a reclaimable-space estimate or a live reading. Unavailable statistics leave scan results intact, and cancelled scans skip the query. Volume snapshots are not saved in history or CSV.

## Unsigned development package

Run `python3 -B .github/scripts/package.py` on Apple Silicon macOS, or `python -B .github/scripts/package.py` on Windows x64. It builds the locked release executable and writes a ZIP under Cargo's `target/packages` directory. The macOS ZIP contains `Storage Clinic.app`; the Windows ZIP contains `storage-clinic.exe`. Extract before opening. Packages use the same local history as development builds.

The macOS archive has been built and checked, but its launch still needs an interactive test. Windows packaging has layout tests only. These packages are unsigned and not notarized; if the OS blocks them, use the source build workflow. See [release checks](docs/release.md).

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
| macOS 27.2, Apple Silicon | Current pass: rustfmt, strict Clippy, 63 Rust tests, debug/release builds; earlier schema and packaging checks passed | Earlier baseline passed; current UI changes and packaged launch need an unlocked interactive session |
| Windows x64 | Earlier baseline passed on `windows-2022`; current changes await CI | Needs testing on Windows 11 |

The current 100,000-file release measurements took 4.15 s for mostly unique files and 12.33 s for duplicate-heavy files on the first pass, with about 17.3 MiB peak process RSS. Repeats took 3.87 s and 16.32 s respectively; the slower duplicate-heavy repeat remains a performance concern. Review queries stayed below 55 ms and cancellation returned within 85 ms in these runs. A separate 256 MiB duplicate fixture improved from 0.167 s to 0.035 s using revalidated full hashes. See [benchmarks](docs/benchmarks.md) for the method, earlier baseline, and limits, and [architecture](docs/architecture.md) for the design.

- Scans read files and never change them. Cleanup arrives in v0.3.
- File types come from extensions only. Duplicates are exact byte matches. Similar images use a 64-bit difference hash, so heavy crops, mirrored, and rotated copies are not found, and HEIC and RAW photos are not read.
- Logical sizes include every hard-linked name. Duplicate verification counts each file identity once. Physical space on disk is not estimated yet.
- WAL keeps saved results readable during indexing. Each scan holds the database write lock until it finishes, so a second app instance cannot save a scan at the same time. The app quits when its window closes.
- Older scans need a new scan to add duplicate findings. The app shows the first scan error alongside the error counts.
- Images over 64 MiB encoded size or 16,384 pixels per side are skipped. Decoder allocations have a best-effort 64 MiB budget per worker, with at most two workers. The similarity threshold still needs testing on real photos.
- Protected-location labels are conservative path rules, not complete OS or cloud-provider detection.
- Unsigned development ZIPs can be built locally. Signed distribution, notarization, and updates remain open.


## Release and demo checklist

See [the release checklist](docs/release.md) for exact checks, the read-only demo sequence, and outstanding platform and distribution gates. This repository is a working read-only analysis application; Trash/Recycle Bin actions and recovery are future milestones, not enabled capabilities.
