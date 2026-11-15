# Storage Clinic instructions for Claude Code

Read [AGENTS.md](AGENTS.md) at the start of each session. It is the shared coding guide for folder ownership, readable Rust, filesystem safety, and verification.

- Start from [docs/tasks.md](docs/tasks.md) and check the working tree before editing. Use [docs/architecture.md](docs/architecture.md) to choose the owning module.
- Keep GPUI concerns in `apps/desktop`, filesystem analysis in `crates/scan-engine`, and SQLite changes in `crates/scan-store`.
- Keep changes focused, give domain values descriptive names, and make state transitions and side effects easy to follow. Keep v0.1 scans read-only and use synthetic test data.
- When delegating, give each task a clear file owner. Integrate dependent changes in order and verify the combined result.
- Before handing off, record what actually ran, which platform checks remain, and the next concrete task in [docs/progress.md](docs/progress.md).
