use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use scan_engine::{ScanProgress, scan_directory};

#[test]
fn streams_metadata_and_skips_excluded_subtrees() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path();
    fs::create_dir(path.join("nested"))?;
    fs::create_dir(path.join("excluded"))?;
    fs::write(path.join("nested/track.wav"), b"12345")?;
    fs::write(path.join("empty.txt"), b"")?;
    fs::write(path.join("excluded/private.txt"), b"secret")?;

    let mut files = Vec::new();
    let mut updates = Vec::new();
    let summary = scan_directory(
        path,
        &[PathBuf::from("excluded")],
        &AtomicBool::new(false),
        |progress, file| {
            updates.push(progress);
            files.extend(file);
        },
    )?;

    files.sort_by_key(|file| file.logical_bytes);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, path.join("empty.txt"));
    assert_eq!(files[1].path, path.join("nested/track.wav"));
    assert_eq!(files[1].logical_bytes, 5);
    assert_eq!(
        files[1].modified,
        fs::metadata(&files[1].path)?.modified().ok()
    );
    assert_eq!(fs::read(&files[1].path)?, b"12345");
    assert!(!summary.cancelled);
    assert_eq!(summary.progress, *updates.last().unwrap());
    assert_eq!(
        summary.progress,
        ScanProgress {
            files: 2,
            directories: 2,
            logical_bytes: 5,
            skipped: 1,
            errors: 0,
        }
    );
    Ok(())
}

#[test]
fn cancels_after_first_file() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("first"), b"data")?;
    fs::write(root.path().join("second"), b"data")?;
    let cancelled = AtomicBool::new(false);

    let summary = scan_directory(root.path(), &[], &cancelled, |_, file| {
        if file.is_some() {
            cancelled.store(true, Ordering::Relaxed);
        }
    })?;

    assert!(summary.cancelled);
    assert_eq!(summary.progress.files, 1);
    Ok(())
}

#[test]
fn reports_invalid_roots() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let file = root.path().join("file.txt");
    fs::write(&file, b"data")?;
    let cancelled = AtomicBool::new(false);

    let error = scan_directory(&file, &[], &cancelled, |_, _| {}).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let error =
        scan_directory(&root.path().join("missing"), &[], &cancelled, |_, _| {}).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    Ok(())
}

#[test]
fn reports_logical_bytes_per_path_for_hard_links() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let original = root.path().join("original");
    fs::write(&original, b"same")?;
    fs::hard_link(&original, root.path().join("alias"))?;

    let summary = scan_directory(root.path(), &[], &AtomicBool::new(false), |_, _| {})?;
    assert_eq!(summary.progress.files, 2);
    assert_eq!(summary.progress.logical_bytes, 8);
    Ok(())
}

#[cfg(unix)]
#[test]
fn ignores_symlink_targets_and_rejects_a_symlink_root() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(outside.path().join("external.txt"), b"outside")?;
    let target = root.path().join("external-folder");
    symlink(outside.path(), &target)?;
    symlink(root.path(), root.path().join("cycle"))?;
    fs::write(root.path().join("local.txt"), b"local")?;
    let cancelled = AtomicBool::new(false);

    let summary = scan_directory(root.path(), &[], &cancelled, |_, _| {})?;
    assert_eq!(summary.progress.files, 1);
    assert_eq!(summary.progress.skipped, 2);
    let error = scan_directory(&target, &[], &cancelled, |_, _| {}).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let error = scan_directory(&target.join("."), &[], &cancelled, |_, _| {}).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    Ok(())
}

#[cfg(unix)]
#[test]
fn continues_after_an_unreadable_subdirectory() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir()?;
    let blocked = root.path().join("blocked");
    fs::create_dir(&blocked)?;
    fs::write(blocked.join("private.txt"), b"private")?;
    fs::write(root.path().join("visible.txt"), b"visible")?;
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))?;
    let can_read = fs::read_dir(&blocked).is_ok();
    let result = scan_directory(root.path(), &[], &AtomicBool::new(false), |_, _| {});
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700))?;

    // Elevated test users can read mode-000 directories.
    if !can_read {
        let summary = result?;
        assert_eq!(summary.progress.files, 1);
        assert_eq!(summary.progress.errors, 1);
        assert!(summary.first_error.unwrap().contains("blocked"));
    }
    Ok(())
}

#[test]
fn observes_empty_folders_without_treating_excluded_children_as_empty() -> io::Result<()> {
    use scan_engine::{ScanEntry, scan_entries};
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("parent/empty"))?;
    fs::create_dir(root.path().join("hidden-only"))?;
    fs::write(root.path().join("hidden-only/.hidden"), b"")?;
    fs::create_dir(root.path().join("excluded-only"))?;
    fs::write(root.path().join("excluded-only/secret"), b"secret")?;
    fs::create_dir(root.path().join("skip"))?;
    let mut directories = Vec::new();
    let summary = scan_entries(
        root.path(),
        &["excluded-only/secret".into(), "skip".into()],
        &AtomicBool::new(false),
        |_, entry| {
            if let Some(ScanEntry::Directory(directory)) = entry {
                directories.push(directory);
            }
        },
    )?;
    let empty: Vec<_> = directories
        .iter()
        .filter(|directory| directory.empty == Some(true))
        .map(|directory| directory.path.clone())
        .collect();
    assert_eq!(empty, [root.path().join("parent/empty")]);
    assert_eq!(summary.progress.files, 1);
    assert_eq!(summary.progress.directories, directories.len() as u64);
    assert_eq!(summary.progress.skipped, 2);
    assert!(
        !directories
            .iter()
            .any(|directory| directory.path.ends_with("skip"))
    );
    assert_eq!(
        fs::read(root.path().join("excluded-only/secret"))?,
        b"secret"
    );
    Ok(())
}

#[test]
fn cancelled_directory_scan_does_not_infer_unvisited_empty_folders() -> io::Result<()> {
    use scan_engine::{ScanEntry, scan_entries};
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("unvisited"))?;
    let cancel = AtomicBool::new(false);
    let mut directories = Vec::new();
    let summary = scan_entries(root.path(), &[], &cancel, |_, entry| {
        if let Some(ScanEntry::Directory(directory)) = entry {
            directories.push(directory);
            cancel.store(true, Ordering::Relaxed);
        }
    })?;
    assert!(summary.cancelled);
    assert_eq!(directories.len(), 1);
    assert_eq!(directories[0].empty, Some(false));
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_or_link_only_folders_are_never_reported_empty() -> io::Result<()> {
    use scan_engine::{ScanEntry, scan_entries};
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("blocked"))?;
    fs::write(root.path().join("blocked/secret"), b"keep")?;
    fs::create_dir(root.path().join("links"))?;
    symlink("missing-target", root.path().join("links/broken"))?;
    fs::set_permissions(
        root.path().join("blocked"),
        fs::Permissions::from_mode(0o000),
    )?;
    let mut empty = Vec::new();
    let result = scan_entries(root.path(), &[], &AtomicBool::new(false), |_, entry| {
        if let Some(ScanEntry::Directory(directory)) = entry
            && directory.empty == Some(true)
        {
            empty.push(directory.path);
        }
    });
    fs::set_permissions(
        root.path().join("blocked"),
        fs::Permissions::from_mode(0o700),
    )?;
    result?;
    assert!(empty.is_empty());
    Ok(())
}

#[test]
fn smart_scope_skips_project_internals_unless_the_project_is_chosen() -> std::io::Result<()> {
    use scan_engine::{ScanScope, scan_entries_with_scope};
    use std::sync::atomic::AtomicBool;

    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let project = home.join("code/app");
    for (path, bytes) in [
        ("code/app/src/main.rs", 10),
        ("code/app/Cargo.toml", 1),
        ("code/app/target/debug/app", 500),
        ("code/app/node_modules/left-pad/index.js", 20),
        ("code/app/.git/objects/ab", 30),
        ("Documents/notes.txt", 5),
    ] {
        let path = home.join(path);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, vec![0; bytes])?;
    }
    let scan = |root: &std::path::Path| -> std::io::Result<(Vec<String>, Vec<&'static str>)> {
        let mut files = Vec::new();
        let mut reasons = Vec::new();
        scan_entries_with_scope(
            root,
            &[],
            ScanScope::Smart,
            &AtomicBool::new(false),
            |_, entry| match entry {
                Some(scan_engine::ScanEntry::File(file)) => files.push(
                    file.path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                ),
                Some(scan_engine::ScanEntry::ScopedOut { reason, .. }) => reasons.push(reason),
                _ => {}
            },
        )?;
        files.sort();
        reasons.sort();
        Ok((files, reasons))
    };

    let (files, reasons) = scan(&home)?;
    assert_eq!(
        files,
        [
            "Documents/notes.txt",
            "code/app/Cargo.toml",
            "code/app/src/main.rs"
        ]
    );
    assert_eq!(
        reasons,
        [
            "Git object store",
            "JavaScript dependencies",
            "Rust build output"
        ]
    );

    let (files, reasons) = scan(&project)?;
    assert_eq!(
        files.len(),
        5,
        "a chosen Git project is scanned in full: {files:?}"
    );
    assert!(reasons.is_empty());
    Ok(())
}
