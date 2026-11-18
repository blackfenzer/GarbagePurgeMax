use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use cleanup_engine::{TrashBackend, TrashReceipt};
use scan_store::{CleanupState, SavedScan, ScanStore};

use super::job;
use crate::workspace::{scan::inspect_and_save, storage};

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    tree: PathBuf,
    database: PathBuf,
    scan: SavedScan,
}

impl Fixture {
    fn new() -> io::Result<Self> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let tree = root.join("tree");
        fs::create_dir(&tree)?;
        fs::write(tree.join("a.bin"), b"verified duplicate")?;
        fs::write(tree.join("b.bin"), b"verified duplicate")?;
        fs::write(tree.join("first.tmp"), b"temporary one")?;
        fs::write(tree.join("second.tmp"), b"temporary two, different size")?;
        fs::create_dir(tree.join("empty"))?;
        let database = root.join("app-data/scans.sqlite3");
        let scan = inspect_and_save(
            tree.clone(),
            Ok(database.clone()),
            &AtomicBool::new(false),
            |_| {},
        )?
        .saved
        .unwrap();
        Ok(Self {
            _temporary: temporary,
            root,
            tree,
            database,
            scan,
        })
    }
    fn store(&self) -> io::Result<ScanStore> {
        storage::open_at(&self.database)
    }
    fn queue(&self, source: &str, keeper: Option<&str>) -> io::Result<i64> {
        let mut store = self.store()?;
        let keeper = keeper.map(|name| self.tree.join(name));
        let entry = job::prepare(
            &store,
            &self.scan,
            &self.tree.join(source),
            keeper.as_deref(),
            &AtomicBool::new(false),
        )?;
        store.cleanup_session()?.enqueue(&entry)
    }
    fn backend(&self) -> io::Result<TestTrash> {
        let directory = self.root.join("test-trash");
        fs::create_dir_all(&directory)?;
        Ok(TestTrash {
            directory,
            calls: AtomicUsize::new(0),
            fail: None,
            lose_receipt: false,
        })
    }
}

struct TestTrash {
    directory: PathBuf,
    calls: AtomicUsize,
    fail: Option<String>,
    lose_receipt: bool,
}
impl TrashBackend for TestTrash {
    fn trash(&self, source: &Path) -> io::Result<TrashReceipt> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self
            .fail
            .as_deref()
            .is_some_and(|name| source.file_name() == Some(std::ffi::OsStr::new(name)))
        {
            return Err(io::Error::other("Injected native failure"));
        }
        let path = self.directory.join(source.file_name().unwrap());
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test collision",
            ));
        }
        fs::rename(source, &path)?;
        if self.lose_receipt {
            return Err(io::Error::other(
                "Injected interruption after move, before receipt",
            ));
        }
        Ok(TrashReceipt {
            identifier: path.clone(),
            path,
        })
    }
    fn restore(&self, receipt: &TrashReceipt, destination: &Path) -> io::Result<()> {
        if destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test collision",
            ));
        }
        fs::rename(&receipt.path, destination)
    }
}

#[test]
fn duplicate_cleanup_restores_without_overwriting_and_keeps_history() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("a.bin", Some("b.bin"))?;
    let backend = fixture.backend()?;
    let cancel = AtomicBool::new(false);
    let result = job::execute(&mut fixture.store()?, &[id], &backend, &cancel, |_, _| {})?;
    assert_eq!(result.moved, 1);
    assert!(!fixture.tree.join("a.bin").exists());
    assert_eq!(fs::read(fixture.tree.join("b.bin"))?, b"verified duplicate");
    let history = fixture.store()?.cleanup_items(false, 30, 0)?;
    assert_eq!(history[0].state, CleanupState::Trashed);
    assert!(history[0].receipt.is_some());
    fs::write(fixture.tree.join("a.bin"), b"new user data")?;
    assert!(job::restore(&mut fixture.store()?, id, &backend, &cancel).is_err());
    assert_eq!(fs::read(fixture.tree.join("a.bin"))?, b"new user data");
    assert_eq!(
        fs::read(backend.directory.join("a.bin"))?,
        b"verified duplicate"
    );
    fs::remove_file(fixture.tree.join("a.bin"))?;
    job::restore(&mut fixture.store()?, id, &backend, &cancel)?;
    assert_eq!(fs::read(fixture.tree.join("a.bin"))?, b"verified duplicate");
    assert_eq!(
        fixture.store()?.cleanup_items(false, 30, 0)?[0].state,
        CleanupState::Restored
    );
    Ok(())
}

#[test]
fn queue_cannot_schedule_a_kept_copy_or_bypass_duplicate_review() -> io::Result<()> {
    let fixture = Fixture::new()?;
    assert!(fixture.queue("a.bin", None).is_err());
    assert!(fixture.queue("a.bin", Some("a.bin")).is_err());
    let id = fixture.queue("a.bin", Some("b.bin"))?;
    assert!(fixture.queue("b.bin", Some("a.bin")).is_err());
    assert!(fixture.queue("a.bin", Some("b.bin")).is_err());
    fixture.store()?.cleanup_session()?.remove(id)?;
    fixture.queue("b.bin", Some("a.bin"))?;
    assert!(fixture.tree.join("a.bin").exists());
    assert!(fixture.tree.join("b.bin").exists());
    Ok(())
}

#[test]
fn changed_sources_keepers_and_nonempty_folders_never_reach_native_trash() -> io::Result<()> {
    for change in ["source", "keeper", "directory"] {
        let fixture = Fixture::new()?;
        let id = if change == "directory" {
            fixture.queue("empty", None)?
        } else {
            fixture.queue("a.bin", Some("b.bin"))?
        };
        match change {
            "source" => fs::write(fixture.tree.join("a.bin"), b"new contents")?,
            "keeper" => fs::remove_file(fixture.tree.join("b.bin"))?,
            _ => fs::write(fixture.tree.join("empty/new.txt"), b"new contents")?,
        }
        let backend = fixture.backend()?;
        let result = job::execute(
            &mut fixture.store()?,
            &[id],
            &backend,
            &AtomicBool::new(false),
            |_, _| {},
        )?;
        assert_eq!(result.failed, 1);
        assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            fixture.store()?.cleanup_items(false, 30, 0)?[0].state,
            CleanupState::Failed
        );
    }
    Ok(())
}

#[test]
fn cancellation_and_changed_confirmation_leave_unstarted_sources_untouched() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let first = fixture.queue("first.tmp", None)?;
    let second = fixture.queue("second.tmp", None)?;
    let backend = fixture.backend()?;
    assert!(
        job::execute(
            &mut fixture.store()?,
            &[first],
            &backend,
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    let cancelled = AtomicBool::new(true);
    assert!(
        job::execute(
            &mut fixture.store()?,
            &[first, second],
            &backend,
            &cancelled,
            |_, _| {}
        )?
        .cancelled
    );
    assert_eq!(fixture.store()?.cleanup_items(true, 200, 0)?.len(), 2);
    assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    cancelled.store(false, Ordering::Relaxed);
    let outcome = job::execute(
        &mut fixture.store()?,
        &[first, second],
        &backend,
        &cancelled,
        |_, _| cancelled.store(true, Ordering::Relaxed),
    )?;
    assert_eq!(outcome.moved, 1);
    assert!(outcome.cancelled);
    assert_eq!(fixture.store()?.cleanup_items(true, 200, 0)?.len(), 1);
    Ok(())
}

#[test]
fn partial_native_failure_preserves_other_outcomes_and_empty_folder_restore() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let first = fixture.queue("first.tmp", None)?;
    let folder = fixture.queue("empty", None)?;
    let mut backend = fixture.backend()?;
    backend.fail = Some("first.tmp".into());
    let cancel = AtomicBool::new(false);
    let result = job::execute(
        &mut fixture.store()?,
        &[first, folder],
        &backend,
        &cancel,
        |_, _| {},
    )?;
    assert_eq!((result.moved, result.failed), (1, 1));
    assert!(fixture.tree.join("first.tmp").exists());
    assert!(!fixture.tree.join("empty").exists());
    job::restore(&mut fixture.store()?, folder, &backend, &cancel)?;
    assert!(fixture.tree.join("empty").is_dir());
    job::reconcile(&mut fixture.store()?, first, &cancel)?;
    assert_eq!(
        fixture
            .store()?
            .cleanup_items(false, 30, 0)?
            .iter()
            .find(|item| item.id == first)
            .unwrap()
            .state,
        CleanupState::Recovered
    );
    Ok(())
}

#[test]
fn lost_receipt_requires_manual_restore_and_explicit_reconciliation() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    let mut backend = fixture.backend()?;
    backend.lose_receipt = true;
    let cancel = AtomicBool::new(false);
    let result = job::execute(&mut fixture.store()?, &[id], &backend, &cancel, |_, _| {})?;
    assert_eq!(result.failed, 1);
    assert_eq!(
        fixture.store()?.cleanup_items(false, 30, 0)?[0].state,
        CleanupState::Attention
    );
    assert!(job::restore(&mut fixture.store()?, id, &backend, &cancel).is_err());
    assert!(job::reconcile(&mut fixture.store()?, id, &cancel).is_err());
    fs::rename(
        backend.directory.join("first.tmp"),
        fixture.tree.join("first.tmp"),
    )?;
    job::reconcile(&mut fixture.store()?, id, &cancel)?;
    assert_eq!(
        fixture.store()?.cleanup_items(false, 30, 0)?[0].state,
        CleanupState::Recovered
    );
    Ok(())
}

#[test]
fn interrupted_intent_is_not_retried_and_exclusive_lock_prevents_a_second_worker() -> io::Result<()>
{
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    let mut first = fixture.store()?;
    let mut second = fixture.store()?;
    let mut session = first.cleanup_session()?;
    session.claim(&[id])?;
    assert!(second.cleanup_session().is_err());
    drop(session);
    drop(first);
    drop(second.cleanup_session()?);
    let history = second.cleanup_items(false, 30, 0)?;
    assert_eq!(history[0].state, CleanupState::Attention);
    assert!(fixture.tree.join("first.tmp").exists());
    job::reconcile(&mut second, id, &AtomicBool::new(false))?;
    Ok(())
}

#[test]
fn missing_or_modified_trash_content_is_not_restored() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    let backend = fixture.backend()?;
    let cancel = AtomicBool::new(false);
    job::execute(&mut fixture.store()?, &[id], &backend, &cancel, |_, _| {})?;
    fs::write(
        backend.directory.join("first.tmp"),
        b"modified trash contents",
    )?;
    assert!(job::restore(&mut fixture.store()?, id, &backend, &cancel).is_err());
    assert!(!fixture.tree.join("first.tmp").exists());
    fs::remove_file(backend.directory.join("first.tmp"))?;
    assert!(job::restore(&mut fixture.store()?, id, &backend, &cancel).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn link_replacement_is_rejected_before_action() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    fs::remove_file(fixture.tree.join("first.tmp"))?;
    std::os::unix::fs::symlink(
        fixture.tree.join("second.tmp"),
        fixture.tree.join("first.tmp"),
    )?;
    let backend = fixture.backend()?;
    job::execute(
        &mut fixture.store()?,
        &[id],
        &backend,
        &AtomicBool::new(false),
        |_, _| {},
    )?;
    assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    assert!(fixture.tree.join("second.tmp").exists());
    Ok(())
}

#[test]
fn failed_intent_write_prevents_all_filesystem_actions() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    let connection = rusqlite::Connection::open(&fixture.database)?;
    connection.execute_batch("CREATE TRIGGER block_cleanup_intent BEFORE UPDATE OF state ON cleanup_items WHEN NEW.state = 'moving' BEGIN SELECT RAISE(ABORT, 'injected journal failure'); END;")?;
    let backend = fixture.backend()?;
    assert!(
        job::execute(
            &mut fixture.store()?,
            &[id],
            &backend,
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.store()?.cleanup_items(true, 200, 0)?.len(), 1);
    assert!(fixture.tree.join("first.tmp").exists());
    Ok(())
}

#[test]
fn receipt_write_failure_stops_the_batch_and_preserves_recoverable_data()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let first = fixture.queue("first.tmp", None)?;
    let second = fixture.queue("second.tmp", None)?;
    let connection = rusqlite::Connection::open(&fixture.database)?;
    connection.execute_batch("CREATE TRIGGER block_cleanup_receipt BEFORE UPDATE OF receipt_key ON cleanup_items BEGIN SELECT RAISE(ABORT, 'injected receipt failure'); END;")?;
    let backend = fixture.backend()?;
    assert!(
        job::execute(
            &mut fixture.store()?,
            &[first, second],
            &backend,
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(backend.calls.load(Ordering::Relaxed), 1);
    // Queue is newest-first: the second item moved, the first was never touched.
    assert!(fixture.tree.join("first.tmp").exists());
    assert!(backend.directory.join("second.tmp").exists());
    let mut reopened = fixture.store()?;
    drop(reopened.cleanup_session()?);
    let history = reopened.cleanup_items(false, 30, 0)?;
    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .all(|item| item.state == CleanupState::Attention)
    );
    fs::rename(
        backend.directory.join("second.tmp"),
        fixture.tree.join("second.tmp"),
    )?;
    job::reconcile(&mut reopened, first, &AtomicBool::new(false))?;
    job::reconcile(&mut reopened, second, &AtomicBool::new(false))?;
    assert_eq!(
        fs::read(fixture.tree.join("second.tmp"))?,
        b"temporary two, different size"
    );
    Ok(())
}

#[test]
fn kept_copy_remains_reserved_after_trashing_and_restoration_releases_it() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("a.bin", Some("b.bin"))?;
    let backend = fixture.backend()?;
    let cancel = AtomicBool::new(false);
    job::execute(&mut fixture.store()?, &[id], &backend, &cancel, |_, _| {})?;
    assert!(fixture.queue("b.bin", Some("a.bin")).is_err());
    job::restore(&mut fixture.store()?, id, &backend, &cancel)?;
    fixture.queue("b.bin", Some("a.bin"))?;
    Ok(())
}

#[test]
fn current_exclusions_block_queueing_and_previously_confirmed_items() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    let source = fixture.tree.join("first.tmp");
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        source.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let bytes: Vec<_> = {
        use std::os::windows::ffi::OsStrExt;
        source
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    let mut settings = b"SCE1".to_vec();
    settings.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    settings.extend(bytes);
    fs::write(fixture.database.with_file_name("exclusions.bin"), settings)?;
    assert!(fixture.queue("first.tmp", None).is_err());
    let backend = fixture.backend()?;
    let outcome = job::execute(
        &mut fixture.store()?,
        &[id],
        &backend,
        &AtomicBool::new(false),
        |_, _| {},
    )?;
    assert_eq!(outcome.failed, 1);
    assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    assert!(source.exists());
    Ok(())
}

#[test]
fn unreadable_exclusion_settings_fail_closed_before_filesystem_actions() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let id = fixture.queue("first.tmp", None)?;
    fs::write(
        fixture.database.with_file_name("exclusions.bin"),
        b"invalid",
    )?;
    assert!(fixture.queue("second.tmp", None).is_err());
    let backend = fixture.backend()?;
    let outcome = job::execute(
        &mut fixture.store()?,
        &[id],
        &backend,
        &AtomicBool::new(false),
        |_, _| {},
    )?;
    assert_eq!(outcome.failed, 1);
    assert_eq!(backend.calls.load(Ordering::Relaxed), 0);
    assert!(fixture.tree.join("first.tmp").exists());
    Ok(())
}
