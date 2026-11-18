# Storage Clinic engineering guidelines

Use [the development plan](docs/tasks.md) for milestone scope, [the architecture](docs/architecture.md) for module boundaries, and [the progress notes](docs/progress.md) for verified results. Keep scans read-only and local. v0.3 mutations belong exclusively to the explicitly confirmed cleanup and recovery workflow.

## Folder ownership

| Path | Put this work here |
| --- | --- |
| `apps/desktop/src/main.rs` | App startup, assets, and the native window. |
| `apps/desktop/src/workspace.rs` | Navigation and workspace-level state. |
| `apps/desktop/src/workspace/` | Views and their state, native dialogs, and background job coordination. Keep supporting code with its feature, as in `results/`. |
| `crates/scan-engine/src/` | Traversal, metadata, file identity, and analysis. Keep platform-specific code beside the feature it supports. |
| `crates/scan-engine/tests/` and `crates/scan-engine/examples/` | Generated filesystem fixtures and reproducible scanner benchmarks. |
| `crates/cleanup-engine/src/` | Pre-action evidence, native Trash/Recycle Bin adapters, and no-overwrite restore. No GPUI or SQLite. |
| `crates/scan-store/src/` and `sql/` | SQLite connections, transactions, data encoding, ordered migrations, and parameterized queries. |
| `docs/` and `.github/workflows/` | Architecture, tasks, benchmarks, and build automation. |

Keep dependencies moving from desktop to the engine and store. The store can use scanner and cleanup evidence types, and the scanner stays independent of GPUI and SQLite. Add a module when it gives a feature or platform adapter a clear home. Extract shared code when callers have the same responsibility and invariants. Add a crate when it has a meaningful dependency boundary or genuinely independent callers. Put new SQL beside the existing migrations and queries. Keep local references and build output under Git-ignored paths.

## Readable Rust

- Use domain names that describe the work, such as `scan_root`, `logical_bytes`, `duplicate_candidates`, and `verified_matches`. Include units where the type leaves them ambiguous.
- Start with a direct function and explicit data flow. Use guard clauses for early exits, pattern matching for distinct states, and short iterator chains where they read clearly. Split a function when a named operation has its own responsibility or invariant.
- Keep helpers private by default. Prefer typed state and structures with named fields when callers need to understand several values together. Keep types with their owner and expose the smallest API callers need.
- Explain the reason for unusual code. Document platform assumptions, transaction boundaries, safety invariants, and tradeoffs that the code alone cannot show.
- Keep changes focused on the requested behavior. Follow local conventions, format with Rustfmt, and update the plan or architecture when a change alters scope or ownership.

## Filesystem and desktop boundaries

- Open the selected tree read-only during scans. Only confirmed cleanup may move revalidated items to OS Trash; never permanently delete or empty Trash. Record durable intent and per-item receipts; never automatically retry interrupted mutations. Keep database, settings, and temporary files in app-owned or test-owned locations. Keep excluded paths, symlinks, and reparse points outside traversal according to the scanner contract.
- Carry paths as `Path` or `PathBuf`. Convert to display text only for presentation or search. Retain native path identity through storage, and account for hard links when counting distinct files.
- Keep traversal, hashing, and SQLite I/O off the GPUI thread. Keep work and UI updates bounded, preserve cancellation checkpoints, and make partial results and skipped entries visible.
- Use parameterized SQL and transactions for persistent changes. Add a numbered migration and verify upgrades from databases produced by earlier versions.
- Keep verified matches, heuristics, and skipped checks distinguishable. Label logical sizes accurately and keep physical-space estimates tied to evidence.

## Verification

Run the checks that cover the change and record the OS and outcome. The baseline workspace commands are:

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p storage-clinic
```

Run `python3 -B crates/scan-store/tests/test_schema.py` for SQL changes and `python3 -B .github/scripts/test_package.py` for packaging changes. Use synthetic fixtures for filesystem and cleanup tests. Record build, automated tests, benchmarks, and interactive smoke results separately. Update [the task plan](docs/tasks.md), [progress notes](docs/progress.md), and [changelog](CHANGELOG.md) where the change affects those records.
