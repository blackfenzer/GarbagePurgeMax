use std::fs;
use std::io;
use std::sync::atomic::AtomicBool;

use cleanup_engine::{inspect, revalidate, validate_ancestors};

#[test]
fn changed_bytes_and_identity_are_rejected_even_when_size_matches() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().canonicalize()?.join("item.tmp");
    fs::write(&path, b"abc")?;
    let cancel = AtomicBool::new(false);
    let proof = inspect(&path, &cancel)?;
    fs::write(&path, b"xyz")?;
    assert!(revalidate(&path, &proof, None, &cancel).is_err());
    fs::remove_file(&path)?;
    fs::write(&path, b"abc")?;
    assert!(revalidate(&path, &proof, None, &cancel).is_err());
    Ok(())
}

#[test]
fn hard_links_read_only_files_and_cancelled_checks_are_blocked() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let path = root.join("item.tmp");
    fs::write(&path, b"abc")?;
    assert!(inspect(&path, &AtomicBool::new(true)).is_err());
    fs::hard_link(&path, root.join("alias.tmp"))?;
    assert!(inspect(&path, &AtomicBool::new(false)).is_err());
    fs::remove_file(root.join("alias.tmp"))?;
    let original = fs::metadata(&path)?.permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly)?;
    let rejected = inspect(&path, &AtomicBool::new(false)).is_err();
    fs::set_permissions(&path, original)?;
    assert!(rejected);
    Ok(())
}

#[test]
fn directories_must_stay_empty_and_paths_must_be_absolute() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let directory = root.join("empty");
    fs::create_dir(&directory)?;
    let cancel = AtomicBool::new(false);
    let proof = inspect(&directory, &cancel)?;
    assert!(proof.directory);
    assert_eq!(proof.logical_bytes, 0);
    fs::write(directory.join(".hidden"), b"keep")?;
    assert!(revalidate(&directory, &proof, None, &cancel).is_err());
    assert!(validate_ancestors(std::path::Path::new("relative")).is_err());
    assert!(validate_ancestors(&directory.join("../empty")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_link_ancestors_are_rejected() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    fs::create_dir(root.join("real"))?;
    fs::write(root.join("real/file.tmp"), b"keep")?;
    std::os::unix::fs::symlink(root.join("real"), root.join("link"))?;
    assert!(inspect(&root.join("link/file.tmp"), &AtomicBool::new(false)).is_err());
    Ok(())
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires an interactive OS Trash / Recycle Bin and moves only a generated fixture"]
fn native_trash_round_trip() -> io::Result<()> {
    use cleanup_engine::{NativeTrash, TrashBackend, restore_checked, verify_receipt};
    let temp = tempfile::tempdir()?;
    let path = temp
        .path()
        .canonicalize()?
        .join("storage-clinic-native-trash-fixture.txt");
    fs::write(
        &path,
        b"disposable Storage Clinic native round-trip fixture",
    )?;
    let cancel = AtomicBool::new(false);
    let proof = inspect(&path, &cancel)?;
    let receipt = NativeTrash.trash(&path)?;
    eprintln!("Native fixture recovery path: {}", receipt.path.display());
    assert!(!path.exists());
    verify_receipt(&receipt, &proof, &cancel)?;
    restore_checked(&NativeTrash, &receipt, &path, &proof, &cancel)?;
    assert_eq!(
        fs::read(path)?,
        b"disposable Storage Clinic native round-trip fixture"
    );
    Ok(())
}
