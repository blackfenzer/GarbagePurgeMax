use std::env;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use scan_engine::{ScanSummary, scan_directory};

const BRANCHES: u64 = 10;
const FILES_PER_LEAF: u64 = 100;
const FILE_BYTES: [u64; 4] = [0, 128, 1024, 4096];
const FILES: u64 = BRANCHES.pow(3) * FILES_PER_LEAF;
const DIRECTORIES: u64 = 1 + BRANCHES + BRANCHES.pow(2) + BRANCHES.pow(3);
const LOGICAL_BYTES: u64 = FILES / FILE_BYTES.len() as u64
    * (FILE_BYTES[0] + FILE_BYTES[1] + FILE_BYTES[2] + FILE_BYTES[3]);
const TRAVERSAL_RUNS: u32 = 5;
const CANCELLATION_RUNS: usize = 31;

fn main() -> io::Result<()> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => benchmark(),
        [mode, fixture] if mode == "--traversal-worker" => traversal(Path::new(fixture)),
        [mode, fixture] if mode == "--cancellation-worker" => cancellation(Path::new(fixture)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "run the benchmark without arguments",
        )),
    }
}

fn benchmark() -> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let start = Instant::now();
    create_fixture(fixture.path())?;
    println!(
        "Fixture: {FILES} files, {DIRECTORIES} directories, {LOGICAL_BYTES} logical bytes, created in {:.3} s",
        start.elapsed().as_secs_f64()
    );

    run_worker("--traversal-worker", fixture.path())?;
    run_worker("--cancellation-worker", fixture.path())?;
    fixture.close()
}

fn create_fixture(root: &Path) -> io::Result<()> {
    for group in 0..BRANCHES {
        for section in 0..BRANCHES {
            for folder in 0..BRANCHES {
                let leaf = root
                    .join(format!("group-{group}"))
                    .join(format!("section-{section}"))
                    .join(format!("folder-{folder}"));
                fs::create_dir_all(&leaf)?;
                for file in 0..FILES_PER_LEAF {
                    let length = FILE_BYTES[file as usize % FILE_BYTES.len()];
                    File::create(leaf.join(format!("file-{file:03}")))?.set_len(length)?;
                }
            }
        }
    }
    Ok(())
}

fn run_worker(mode: &str, fixture: &Path) -> io::Result<()> {
    let executable = env::current_exe()?;
    let status = if cfg!(target_os = "macos") && mode == "--traversal-worker" {
        // zsh's time reserved word reports the child high-water RSS even when
        // the sandbox blocks the sysctl call made by macOS /usr/bin/time -l.
        Command::new("/bin/zsh")
            .args([
                "-fc",
                "TIMEFMT='Traversal worker peak RSS: %M KiB'; time \"$@\"",
                "scan-benchmark",
            ])
            .arg(executable)
            .arg(mode)
            .arg(fixture)
            .status()?
    } else {
        Command::new(executable).arg(mode).arg(fixture).status()?
    };
    if !status.success() {
        return Err(io::Error::other(format!("{mode} exited with {status}")));
    }
    Ok(())
}

fn traversal(fixture: &Path) -> io::Result<()> {
    let stop = AtomicBool::new(false);
    let mut elapsed = Duration::ZERO;
    for run in 1..=TRAVERSAL_RUNS {
        let start = Instant::now();
        let summary = scan_directory(fixture, &[], &stop, |_, _| {})?;
        let duration = start.elapsed();
        verify_traversal(summary)?;
        elapsed += duration;
        println!(
            "Traversal {run}/{TRAVERSAL_RUNS}: {:.3} s, {:.0} files/s",
            duration.as_secs_f64(),
            FILES as f64 / duration.as_secs_f64()
        );
    }
    println!(
        "Aggregate throughput: {:.0} files/s",
        (FILES * u64::from(TRAVERSAL_RUNS)) as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

fn verify_traversal(summary: ScanSummary) -> io::Result<()> {
    let progress = summary.progress;
    if summary.cancelled
        || progress.files != FILES
        || progress.directories != DIRECTORIES
        || progress.logical_bytes != LOGICAL_BYTES
        || progress.skipped != 0
        || progress.errors != 0
    {
        return Err(io::Error::other(format!(
            "unexpected traversal result: {summary:?}"
        )));
    }
    Ok(())
}

fn cancellation(fixture: &Path) -> io::Result<()> {
    let mut samples = Vec::with_capacity(CANCELLATION_RUNS);
    for _ in 0..CANCELLATION_RUNS {
        samples.push(cancel_after_first_file(fixture)?);
    }
    samples.sort_unstable();
    let micros = |duration: Duration| duration.as_secs_f64() * 1_000_000.0;
    println!(
        "Cancellation after first file ({CANCELLATION_RUNS} runs): {:.1} µs min, {:.1} µs median, {:.1} µs max",
        micros(samples[0]),
        micros(samples[samples.len() / 2]),
        micros(samples[samples.len() - 1])
    );
    Ok(())
}

fn cancel_after_first_file(fixture: &Path) -> io::Result<Duration> {
    let stop = AtomicBool::new(false);
    let (first_file, started) = mpsc::channel();
    let (sent_at, signal) = mpsc::channel();
    thread::scope(|scope| {
        let cancelled = &stop;
        scope.spawn(move || {
            if started.recv().is_ok() {
                let requested_at = Instant::now();
                cancelled.store(true, Ordering::Relaxed);
                let _ = sent_at.send(requested_at);
            }
        });

        let mut first_file = Some(first_file);
        let mut requested_at = None;
        let summary = scan_directory(fixture, &[], &stop, |_, file| {
            if file.is_some()
                && let Some(sender) = first_file.take()
            {
                // Pause the visitor so the scan is active when the other thread cancels it.
                if sender.send(()).is_ok() {
                    requested_at = signal.recv().ok();
                }
            }
        })?;
        let completed_at = Instant::now();
        if !summary.cancelled || summary.progress.files != 1 || summary.progress.errors != 0 {
            return Err(io::Error::other(format!(
                "unexpected cancellation result: {summary:?}"
            )));
        }
        requested_at
            .map(|instant| completed_at.duration_since(instant))
            .ok_or_else(|| io::Error::other("cancellation signal missing"))
    })
}
