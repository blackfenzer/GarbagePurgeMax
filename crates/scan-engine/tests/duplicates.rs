use std::fs::{self, File, FileTimes};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, UNIX_EPOCH};

use scan_engine::{FileRecord, scan_directory, verify_duplicate_group};

fn index(root: &Path) -> io::Result<Vec<FileRecord>> {
    let mut records = Vec::new();
    scan_directory(root, &[], &AtomicBool::new(false), |_, record| {
        records.extend(record);
    })?;
    Ok(records)
}

#[test]
fn groups_byte_exact_matches_and_empty_files_by_size() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let first = vec![b'a'; 160 * 1024];
    let mut different = first.clone();
    different[81 * 1024] = b'b';
    fs::write(root.join("first"), &first)?;
    fs::write(root.join("copy"), &first)?;
    fs::write(root.join("same-size-sample-match"), &different)?;
    fs::write(root.join("empty-a"), [])?;
    fs::write(root.join("empty-b"), [])?;
    fs::write(root.join("other-size"), b"other")?;

    let result = verify_duplicate_group(&index(root)?, &AtomicBool::new(false));
    assert_eq!(result.errors, 0);
    assert!(!result.cancelled);
    assert_eq!(result.groups.len(), 2);
    assert_eq!(
        result.groups[0].paths,
        [root.join("copy"), root.join("first")],
    );
    assert_eq!(
        result.groups[1].paths,
        [root.join("empty-a"), root.join("empty-b")],
    );
    let empty_hash = result.groups[1]
        .hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        empty_hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    Ok(())
}

#[test]
fn counts_only_one_path_per_hard_link_identity() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let original = root.join("original");
    let alias = root.join("alias");
    fs::write(&original, b"twin")?;
    fs::hard_link(&original, &alias)?;

    let cancel = AtomicBool::new(false);
    let result = verify_duplicate_group(&index(root)?, &cancel);
    assert!(result.groups.is_empty());
    assert_eq!(result.errors, 0);

    let separate = root.join("separate");
    fs::write(&separate, b"twin")?;
    let result = verify_duplicate_group(&index(root)?, &cancel);
    assert_eq!(result.errors, 0);
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].paths, [alias, separate]);
    Ok(())
}

#[test]
fn records_missing_and_changed_files_and_keeps_remaining_matches() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    for name in ["a-missing", "b-modified", "c-matching", "d-matching"] {
        fs::write(root.join(name), b"match")?;
    }

    let records = index(root)?;
    fs::remove_file(root.join("a-missing"))?;
    let modified = root.join("b-modified");
    fs::write(&modified, b"other")?;
    File::options().write(true).open(modified)?.set_times(
        FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_500_000_000)),
    )?;

    let result = verify_duplicate_group(&records, &AtomicBool::new(false));
    assert_eq!(result.errors, 2);
    assert!(result.first_error.unwrap().contains("a-missing"));
    assert_eq!(result.groups.len(), 1);
    assert_eq!(
        result.groups[0].paths,
        [root.join("c-matching"), root.join("d-matching")],
    );
    Ok(())
}

#[test]
fn does_not_open_candidates_if_cancelled() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    fs::write(root.join("a"), b"same")?;
    fs::write(root.join("b"), b"same")?;
    let records = index(root)?;
    fs::remove_file(root.join("a"))?;

    let result = verify_duplicate_group(&records, &AtomicBool::new(true));
    assert!(result.cancelled);
    assert!(result.groups.is_empty());
    assert_eq!(result.errors, 0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_a_path_replaced_by_a_symbolic_link() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let root = directory.path();
    fs::write(root.join("a"), b"same")?;
    fs::write(root.join("b"), b"same")?;
    let records = index(root)?;

    let elsewhere = tempfile::tempdir()?;
    let target = elsewhere.path().join("target");
    fs::write(&target, b"same")?;
    fs::remove_file(root.join("a"))?;
    symlink(&target, root.join("a"))?;

    let result = verify_duplicate_group(&records, &AtomicBool::new(false));
    assert_eq!(result.errors, 1);
    assert!(result.first_error.unwrap().contains("symbolic links"));
    assert!(result.groups.is_empty());
    assert_eq!(fs::read(target)?, b"same");
    Ok(())
}

#[cfg(unix)]
#[test]
fn preserves_native_paths_and_reports_permission_errors() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let fixture = root.join("a");
    let blocked = root.join("b-unreadable");
    let native = root.join("c-雪");
    fs::write(&fixture, b"same")?;
    fs::write(&blocked, b"same")?;
    fs::write(&native, b"same")?;
    let records = index(root)?;

    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))?;
    let can_read = File::open(&blocked).is_ok();
    let result = verify_duplicate_group(&records, &AtomicBool::new(false));
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o600))?;

    assert_eq!(result.errors, u64::from(!can_read));
    if !can_read {
        assert!(result.first_error.unwrap().contains("b-unreadable"));
    }
    assert_eq!(result.groups.len(), 1);
    assert!(result.groups[0].paths.contains(&native));
    assert!(result.groups[0].paths.contains(&fixture));
    Ok(())
}
