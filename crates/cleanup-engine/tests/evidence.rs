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
    // Built as text: `Path::join` resolves `..` inside Windows verbatim (`\\?\`) paths,
    // which would leave no traversal to reject.
    let mut traversal = directory.clone().into_os_string();
    traversal.push(format!("{0}..{0}empty", std::path::MAIN_SEPARATOR));
    assert!(validate_ancestors(std::path::Path::new(&traversal)).is_err());
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

#[cfg(target_os = "macos")]
#[test]
fn native_restore_refuses_a_collision_even_without_the_precheck() -> io::Result<()> {
    use cleanup_engine::{NativeTrash, TrashBackend, TrashReceipt, restore_checked};
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let source = root.join("fixture-trash-item");
    let destination = root.join("occupied");
    fs::write(&source, b"original")?;
    fs::write(&destination, b"preserve destination")?;
    let cancel = AtomicBool::new(false);
    let proof = inspect(&source, &cancel)?;
    let receipt = TrashReceipt {
        identifier: source.clone(),
        path: source.clone(),
    };
    // Call the native primitive directly: an intervening destination must never be replaced.
    assert!(NativeTrash.restore(&receipt, &destination).is_err());
    assert_eq!(fs::read(&destination)?, b"preserve destination");
    assert_eq!(fs::read(&source)?, b"original");
    let unoccupied = root.join("restored");
    restore_checked(&NativeTrash, &receipt, &unoccupied, &proof, &cancel)?;
    assert_eq!(fs::read(unoccupied)?, b"original");
    assert!(!source.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn sparse_file_allocation_uses_blocks_instead_of_logical_length() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().canonicalize()?.join("sparse.tmp");
    fs::File::create(&path)?.set_len(1024 * 1024)?;
    let proof = inspect(&path, &AtomicBool::new(false))?;
    assert_eq!(proof.logical_bytes, 1024 * 1024);
    assert_eq!(
        proof.allocated_bytes,
        fs::metadata(path)?.blocks().checked_mul(512)
    );
    Ok(())
}

#[test]
fn folder_contents_are_listed_and_any_change_inside_is_rejected() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let folder = temp.path().canonicalize()?.join("project");
    fs::create_dir_all(folder.join("build/cache"))?;
    fs::write(folder.join("notes.txt"), b"abc")?;
    fs::write(folder.join("build/cache/big.bin"), vec![0; 4096])?;
    let cancel = AtomicBool::new(false);
    let (proof, contents) = cleanup_engine::inspect_contents(&folder, &cancel)?;
    let contents = contents.expect("a non-empty folder lists its contents");
    assert_eq!((contents.files, contents.folders), (2, 2));
    assert_eq!(proof.logical_bytes, 4099);
    assert_eq!(
        contents.largest[0].0,
        std::path::Path::new("build/cache/big.bin")
    );
    revalidate(&folder, &proof, None, &cancel)?;
    fs::write(folder.join("build/cache/added.txt"), b"new")?;
    assert!(revalidate(&folder, &proof, None, &cancel).is_err());
    Ok(())
}
