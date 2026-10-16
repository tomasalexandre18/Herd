//! Test support, shared between this crate's unit tests and its end-to-end
//! tests. Not part of the runner's interface; it exists so that neither kind
//! of test grows its own half-right temporary-directory handling, and so that
//! no test can leave a data root behind on the machine it ran on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Where a skipped assertion writes itself down, so that the command people
/// actually run can print a summary of what it did not check.
///
/// `cargo test` swallows the stdout of a passing test, which made D27's
/// `HERD-SKIPPED` line invisible under `cargo test --workspace`: the suite
/// read as 75 green assertions when some of them had not run at all. A file
/// survives that, and `scripts/test.sh` prints what is in it.
pub const SKIP_LOG_VARIABLE: &str = "HERD_SKIP_LOG";

/// Record an assertion that could not be made on this host.
///
/// Three places at once, on purpose: stdout and stderr for a human reading
/// the run and for `scripts/report-skips.sh` in CI, and the skip log for
/// `scripts/test.sh`, which is the only one `cargo test` cannot hide.
pub fn skipped(test: &str, why: &str) {
    let line = format!("HERD-SKIPPED: {test}: {why}");
    println!("{line}");
    eprintln!("{line}");
    let Some(path) = std::env::var_os(SKIP_LOG_VARIABLE) else {
        return;
    };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        // One `write`, newline included, and not `writeln!`: the macro puts
        // the text and the `\n` on the wire as separate calls, and two tests
        // skipping at the same moment interleaved between them — two
        // assertions landing on one line of the log. `scripts/test.sh` counts
        // lines, so the header under-reported: 13 skipped assertions
        // announced as 12. A count of what was *not* checked that is itself
        // wrong is the exact thing D27 exists to prevent, so the whole line
        // goes in one append, which is atomic on a regular file (D151).
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

/// Serialises the tests that plant, sweep for, or reap Herd job markers.
///
/// The startup reaper walks `/proc` and kills every process carrying a marker
/// whose runner is gone ([`crate::reclaim`]) — and it runs inside
/// `Confined::prepare`, which several tests call. So a test that deliberately
/// plants an orphan to prove the reaper finds it was racing every other test
/// that prepared a confinement: the other reaper killed the orphan first, and
/// the planted process was a zombie with an empty `environ` by the time the
/// assertion looked. That is a flake, and a flake erodes invariant 5, so it
/// is held still rather than retried.
///
/// Every test in this crate that prepares a confinement or sweeps `/proc`
/// takes this first. Poisoning is ignored: one failing test must not turn
/// every other one into a second failure with a different reason.
pub fn one_reaper_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run `body` with this process's stderr diverted to a file, and give back
/// every line of it that mentions `about`.
///
/// For the one kind of assertion that cannot be made any other way: *how many
/// lines* a code path writes to the journal. D151 is about a runner that said
/// one true thing 1497 times onto a host whose IO had already stalled, and a
/// test that counted a return value rather than the writing itself would go
/// green again the moment somebody put the loop back with a `eprintln!` of
/// its own. So this counts the writes.
///
/// `about` is what keeps it honest under `cargo test`'s thread pool: fd 2 is
/// process-wide, so another test writing at the same moment lands in the same
/// capture. Filtering on a marker the caller owns — a job id minted for the
/// one test — leaves a count that is the caller's and nobody else's.
///
/// Stderr is restored whatever `body` does, including panicking: `dup2` back
/// happens before the panic is resumed, or a failing assertion would take the
/// rest of the run's output down with it.
///
/// Only lines written to the descriptor are seen, which is why the path under
/// test writes there ([`crate::journal`]): `eprintln!` consults libtest's
/// output capture, and that capture is per-thread **and inherited by every
/// thread a test spawns**, so there is no thread from which the macro's
/// output can be made to reach fd 2 inside a test binary.
#[cfg(unix)]
pub fn stderr_lines_about<T>(about: &str, body: impl FnOnce() -> T) -> (T, Vec<String>) {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let temp = TempDir::new("captured-stderr");
    let path = temp.path().join("stderr");
    let file = std::fs::File::create(&path).expect("a capture file opens");

    // Safe: three fd calls on fds this function owns or is restoring.
    let saved = unsafe { libc::dup(libc::STDERR_FILENO) };
    assert!(saved >= 0, "stderr must be duplicable to be captured");
    assert!(
        unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } >= 0,
        "stderr must be divertible to be captured"
    );

    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));

    let _ = std::io::stderr().flush();
    unsafe {
        libc::dup2(saved, libc::STDERR_FILENO);
        libc::close(saved);
    }

    let written = std::fs::read_to_string(&path).unwrap_or_default();
    let lines = written
        .lines()
        .filter(|line| line.contains(about))
        .map(ToOwned::to_owned)
        .collect();
    match out {
        Ok(out) => (out, lines),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// A directory under the system temporary directory, removed on drop.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A fresh directory named after `label`, unique per process and call.
    pub fn new(label: &str) -> Self {
        let unique = NEXT.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("herd-test-{label}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory is creatable");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Every Rust source file of this crate, as (path, text).
///
/// Used by the tests that are about the code itself rather than about its
/// behaviour — the one that forbids reaching for an escalation tool, and the
/// one that forbids binding a socket. `CARGO_MANIFEST_DIR` is set by cargo
/// for both unit and end-to-end tests, so the walk starts from the crate
/// rather than from whatever directory the suite was launched in.
pub fn source_files() -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    found.push((path, text));
                }
            }
        }
    }
    found
}
