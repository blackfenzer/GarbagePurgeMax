//! Starts the app binary in its off-screen UI smoke mode (see `src/workspace/ui_smoke.rs`).
//! Run with `cargo test --locked -p storage-clinic --features ui-smoke --test ui_smoke`.
//! Screenshots land in `target/ui-smoke/`.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

#[test]
fn every_page_renders_in_both_themes() -> Result<(), Box<dyn std::error::Error>> {
    let data = tempfile::tempdir()?;
    let shots = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-smoke");
    let output = Command::new(env!("CARGO_BIN_EXE_storage-clinic"))
        .env("STORAGE_CLINIC_DATA_DIR", data.path())
        .env("STORAGE_CLINIC_UI_SMOKE", &shots)
        .output()?;
    assert!(
        output.status.success(),
        "UI smoke run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
