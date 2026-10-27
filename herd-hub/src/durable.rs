//! The one place in Herd that writes to the filesystem.
//!
//! The hub's state is plain files (D8), and the first consumer reads them
//! while a job runs (E-53) and re-judges them much later (E-54). A reader must
//! therefore never see a record that is half written, which means exactly one
//! discipline for replacing a file: write a temporary beside it, fsync the
//! temporary, rename it over the destination, fsync the directory. Rename is
//! atomic, so a reader sees the whole old file or the whole new one.
//!
//! Every writer in the hub goes through this module; nothing calls
//! `fs::write` on its own. The same primitive is what E-17 needs for
//! artefacts, so it is written once and tested once here.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Seek, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

/// Makes every temporary name in a process unique, so two writers to the same
/// path never share one, however they are scheduled.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// The observation point D103's ordering oracle watches from.
///
/// This module exists because counting syncs is not ordering them. `06` and
/// `08` of the remediation dossier counted; neither could say that the
/// record's `fsync` **and** its directory `fsync` had *returned* before the
/// first byte of the event append was *issued*, which is the whole of what
/// holds D103 on a decisive write (`docs/remediation/10-ordering-on-ext4.md`).
///
/// **In a product build `note` is an empty `#[inline(always)]` function and
/// `Op` is a value that is constructed and dropped.** Nothing is allocated,
/// no atomic is read and no lock is taken: the recording half is
/// `#[cfg(test)]` and is not compiled into `herd-hub` at all. The calls
/// themselves are in the real `write_file_with` and the real `append_line`,
/// so what the oracle watches is the code path the hub runs and not a replica
/// of it — which is the one thing a replica-based harness like
/// `herd-hub/tests/sync_prices.rs` cannot claim.
pub(crate) mod watch {
    use super::Synced;

    /// One durable operation of this module, named at the instant that
    /// matters for ordering.
    ///
    /// Three of the five say a syscall has **returned**; `AppendIssued` says
    /// one is about to be **issued**. That asymmetry is deliberate: D103's
    /// requirement is about returns on one side and the issue on the other,
    /// and an oracle that compared two returns would be a weaker statement.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Op {
        /// `sync_all` on the temporary that is about to become this path has
        /// returned. The path carried is the **destination**, not the
        /// temporary, because the destination is what a reader names.
        TemporarySynced,
        /// `rename` of that temporary over this path has returned: the record
        /// is now visible, and not yet durably named.
        Renamed,
        /// `sync_dir` on this path's parent has returned: the rename itself
        /// will survive the machine.
        DirectorySynced,
        /// The `write_all` of one appended line to this path is about to be
        /// issued. Nothing of the line is on the file yet.
        AppendIssued,
        /// `sync_data` on that appended line has returned.
        AppendSynced,
    }

    /// Say that a sync **returned**, holding the proof that it did.
    ///
    /// The `Synced` is the point of this signature. An observation point
    /// that could be written beside a sync could also be left behind when
    /// the sync is deleted, and then an oracle built on it would stay green
    /// through exactly the change it exists to catch. [`Synced`] is produced
    /// by nothing but the three functions that make the calls, so deleting a
    /// sync deletes the only value that can be noted for it and the file
    /// stops compiling until someone deletes the note too — at which point
    /// the oracle goes red. The red run in
    /// `docs/remediation/12-ordering-oracle.md` is that path walked once.
    #[cfg(not(test))]
    #[inline(always)]
    pub fn returned(_op: Op, _path: &std::path::Path, _proof: &Synced) {}

    /// Say that a write is about to be **issued**. There is nothing to prove
    /// yet: that is what "issued" means.
    #[cfg(not(test))]
    #[inline(always)]
    pub fn issuing(_op: Op, _path: &std::path::Path) {}

    #[cfg(test)]
    pub use recording::{armed, issuing, returned, Armed, Step};

    #[cfg(test)]
    mod recording {
        use super::{Op, Synced};
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Mutex, MutexGuard, OnceLock};

        /// One observed operation, with the path it was performed on.
        #[derive(Clone, Debug)]
        pub struct Step {
            pub op: Op,
            pub path: PathBuf,
        }

        /// Read before the lock, so that the thousands of durable writes the
        /// rest of the suite performs while no oracle is armed pay one
        /// relaxed load and nothing else.
        static ARMED: AtomicBool = AtomicBool::new(false);

        struct Watch {
            root: PathBuf,
            trace: Vec<Step>,
        }

        fn watch() -> &'static Mutex<Option<Watch>> {
            static WATCH: OnceLock<Mutex<Option<Watch>>> = OnceLock::new();
            WATCH.get_or_init(|| Mutex::new(None))
        }

        /// Only one oracle at a time, so that two armings cannot interleave
        /// their traces. Held for as long as the [`Armed`] guard lives.
        fn one_at_a_time() -> &'static Mutex<()> {
            static GATE: OnceLock<Mutex<()>> = OnceLock::new();
            GATE.get_or_init(|| Mutex::new(()))
        }

        /// Watch every durable operation under `root` until the guard is
        /// dropped.
        ///
        /// `root` is a state root of this test's own, so the operations the
        /// rest of the suite performs in parallel are filtered out by their
        /// path and cannot appear in the trace.
        pub fn armed(root: &Path) -> Armed {
            let gate = one_at_a_time()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *watch()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Watch {
                root: root.to_path_buf(),
                trace: Vec::new(),
            });
            ARMED.store(true, Ordering::SeqCst);
            Armed { _gate: gate }
        }

        /// The live watch. Dropping it stops the recording.
        pub struct Armed {
            _gate: MutexGuard<'static, ()>,
        }

        impl Armed {
            /// Everything observed since the last `take`, and start again
            /// from empty.
            pub fn take(&self) -> Vec<Step> {
                let mut held = watch()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match held.as_mut() {
                    Some(watch) => std::mem::take(&mut watch.trace),
                    None => Vec::new(),
                }
            }
        }

        impl Drop for Armed {
            fn drop(&mut self) {
                ARMED.store(false, Ordering::SeqCst);
                *watch()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            }
        }

        /// A sync returned, and here is what returned it.
        pub fn returned(op: Op, path: &Path, _proof: &Synced) {
            note(op, path);
        }

        /// A write is about to be issued.
        pub fn issuing(op: Op, path: &Path) {
            note(op, path);
        }

        fn note(op: Op, path: &Path) {
            if !ARMED.load(Ordering::SeqCst) {
                return;
            }
            let mut held = watch()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(watch) = held.as_mut() else {
                return;
            };
            if !path.starts_with(&watch.root) {
                return;
            }
            watch.trace.push(Step {
                op,
                path: path.to_path_buf(),
            });
        }
    }
}

/// Proof that one `fsync`-class syscall **returned**.
///
/// Zero-sized, and produced by nothing but [`sync_all_of`], [`sync_data_of`]
/// and [`sync_dir_of`], each of which makes the call before it hands one
/// back. [`watch::returned`] takes one, so the note that says a sync
/// happened cannot outlive the sync: delete the call and the note no longer
/// compiles. That is what makes D103's ordering oracle a guard rather than a
/// description — see `herd-hub/src/store/ordering_oracle.rs`.
pub(crate) struct Synced(());

/// `fsync` an open file's bytes and metadata.
fn sync_all_of(file: &File) -> io::Result<Synced> {
    file.sync_all()?;
    Ok(Synced(()))
}

/// `fdatasync` an open file's data.
fn sync_data_of(file: &File) -> io::Result<Synced> {
    file.sync_data()?;
    Ok(Synced(()))
}

/// [`sync_dir`], with the proof that it returned.
fn sync_dir_of(path: &Path) -> io::Result<Synced> {
    sync_dir(path)?;
    Ok(Synced(()))
}

/// Replace `path` with `bytes`, atomically.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_file_with(path, |file| file.write_all(bytes))
}

/// Replace `path` with whatever `fill` writes, atomically.
///
/// `fill` writes into a temporary file; only once it has returned `Ok` and the
/// bytes are on the disk does the temporary take the destination's name. If
/// `fill` fails, the destination keeps the contents it had and no temporary is
/// left behind — a failed write is a write that did not happen.
pub fn write_file_with(
    path: &Path,
    fill: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = parent_of(path)?;
    let temporary = temporary_beside(path);
    let written = (|| -> io::Result<()> {
        let mut file = File::create(&temporary)?;
        fill(&mut file)?;
        // The bytes, before the name: a rename that beats its own contents to
        // the disk would publish an empty file.
        let bytes = sync_all_of(&file)?;
        watch::returned(watch::Op::TemporarySynced, path, &bytes);
        drop(file);
        fs::rename(&temporary, path)?;
        watch::issuing(watch::Op::Renamed, path);
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
        return written;
    }
    // The name, too, has to survive: without this the rename itself can be
    // lost while the file's contents are safe.
    let name = sync_dir_of(parent)?;
    watch::returned(watch::Op::DirectorySynced, path, &name);
    Ok(())
}

/// Whether an appended line is on the disk when the append returns.
///
/// Both answers are correct for some line of `events.jsonl`, and which line
/// gets which is D183's step A: a **transition** is a fact about where a job
/// stands and waits for the disk; a **`waiting` reason** is an explanation of
/// why a queued job is not starting and does not.
enum Durably {
    /// `fdatasync` before returning.
    Yes,
    /// The bytes are in the page cache and visible to every reader; a power
    /// cut can lose them, and a `kill -9` cannot, because the cache is the
    /// kernel's.
    No,
}

/// Append one line, as one write, and put it on the disk before returning.
///
/// Used for the transition lines of `events.jsonl`, where a transition must be
/// readable the moment it is recorded. `line` must not contain a newline of
/// its own; the terminator is added here so that a line is never published
/// without one.
pub fn append_line(path: &Path, line: &str) -> io::Result<()> {
    append_one_line(path, line, Durably::Yes)
}

/// Append one line, as one write, without waiting for the disk.
///
/// **D183 step A.** One caller: the `waiting` line of `record_waiting`
/// (`store.rs`). It is the cheap third of a waiting write — 907 µs of
/// 3.00 ms on ext4/NVMe (`docs/remediation/11-sync-prices.md` §6) — and it
/// is the only one of the three that can be dropped without touching D103.
///
/// Why it cannot touch D103 is arithmetic rather than judgement: the record's
/// `sync_all` **and** its directory `fsync` have returned before the first
/// byte of this line is issued (`write_file_with` above), so a line written
/// here can only ever be *missing* beside a record that is whole, never
/// present beside a record that is not. D103 forbids `events.jsonl` being
/// **ahead** of `job.json` and permits it being behind; this moves the pair
/// in the permitted direction only. `herd-hub/src/store/ordering_oracle.rs`
/// is the standing proof of the premise, not a description of it.
///
/// What it costs: after a power cut, `events.jsonl` may be missing a
/// `waiting` line whose reason `job.json` still names. Already reachable
/// before this (`store.rs`'s `move_to` and `raise` both clear the reason and
/// leave the line), never read by recovery, and rewritten by the next
/// dispatch pass (D171). It is **not** for a transition line, and
/// [`append_line`] above is what every transition still goes through.
pub fn append_line_unsynced(path: &Path, line: &str) -> io::Result<()> {
    append_one_line(path, line, Durably::No)
}

fn append_one_line(path: &Path, line: &str, durably: Durably) -> io::Result<()> {
    debug_assert!(!line.contains('\n'), "one line per append");
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    // The last instant before the first byte of this line is issued. The
    // ordering oracle (`store::ordering_oracle`) reads this point against the
    // record write that D103 requires to be complete before it. It is noted
    // on both paths: dropping the sync below drops nothing of the order.
    watch::issuing(watch::Op::AppendIssued, path);
    file.write_all(record.as_bytes())?;
    match durably {
        Durably::Yes => {
            let line = sync_data_of(&file)?;
            watch::returned(watch::Op::AppendSynced, path, &line);
        }
        Durably::No => {}
    }
    Ok(())
}

/// Append bytes to a job's log, without waiting for the disk.
///
/// A log is a stream, not a record: it is appended to thousands of times while
/// a job runs, and E-53 asks only that a reader can follow it. The bytes are
/// visible to any reader as soon as this returns; [`sync_file`] is what makes
/// them survive the machine, and the store calls it when the job ends.
pub fn append_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(bytes)
}

/// Write a fixed-width value over a small file, in place, without waiting for
/// the disk.
///
/// For the one thing the hub rewrites on the byte path: the number of the last
/// log chunk it wrote, which is what lets it see that chunks are missing. The
/// atomic-rename discipline above costs a temporary, two syncs and a directory
/// sync per write — which the log's own appends deliberately do not pay — and
/// a rename per chunk would also leave a temporary behind every time a `kill
/// -9` landed in the middle of one.
///
/// So this is one write at offset 0, and every caller writes the same number
/// of bytes, so there is nothing to truncate and no shorter old value can show
/// through. The value is in the page cache when this returns, which is what a
/// killed hub needs: the kernel keeps it. A machine that loses power can lose
/// it, and then the hub has no number to compare with — which it says, rather
/// than assuming nothing is missing.
pub fn write_fixed_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.rewind()?;
    file.write_all(bytes)
}

/// Put an existing file's contents on the disk.
pub fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_data()
}

/// Make sure a file exists, even empty.
///
/// A job's `log` is created the moment the job starts, so that a consumer
/// reading the record of a job that printed nothing finds an empty file rather
/// than `ENOENT` (D22). Nothing is written and an existing file is left
/// exactly as it is.
pub fn touch(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    OpenOptions::new().create(true).append(true).open(path)?;
    sync_dir(parent_of(path)?)
}

/// Take the exclusive lock on `path`, creating the file, without waiting.
///
/// This is `flock(2)`, which means the kernel owns the lock: it is released
/// when the holding process dies, however it dies, so a hub that was killed
/// never leaves a lock behind for the next one to break by hand. A lock
/// another holder has is reported as [`ErrorKind::WouldBlock`], and holding it
/// is holding the returned file open.
pub fn lock_exclusive(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(io::Error::from)?;
    Ok(file)
}

/// Replace a held file's contents in place, without renaming it.
///
/// The lock of [`lock_exclusive`] belongs to that file, so the atomic-rename
/// discipline cannot be used on it: a rename would leave the lock on an inode
/// nobody can find. The file says who holds the lock, which no reader needs to
/// see whole, so writing it in place is the honest exception.
pub fn write_in_place(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(bytes)?;
    file.sync_data()
}

/// Whether a directory can be written in, said now rather than at the first
/// job that needs it.
///
/// A state root whose `jobs/` cannot be written is a hub that starts happily
/// and then fails every submission, with only its own log to explain why
/// (D21). The probe is a file created and removed again, because permission is
/// not the only way a directory refuses a write — a full or read-only
/// filesystem refuses one too.
pub fn probe_writable(dir: &Path) -> io::Result<()> {
    let probe = dir.join(format!(".probe-{}", process::id()));
    let written = File::create(&probe).and_then(|mut file| file.write_all(b"herd"));
    let _ = fs::remove_file(&probe);
    written
}

/// Create one directory, and make its existence survive a power cut. An
/// already existing directory is not an error: creating a job's directory is
/// how an id is claimed, so [`create_dir_exclusive`] is what minting uses.
pub fn create_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => sync_dir(parent_of(path)?),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// Create one directory, failing if it already exists.
///
/// `mkdir` is atomic and exclusive, which is what lets the job directories
/// themselves arbitrate job ids: a claim either wins or is told it lost, with
/// no window in between.
pub fn create_dir_exclusive(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    sync_dir(parent_of(path)?)
}

/// Create a directory and every parent it needs.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    sync_dir(path)
}

/// Put a directory's list of names on the disk.
pub fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// The names in a directory, with anything this module could have left behind
/// filtered out, so a caller never has to know what a temporary looks like.
pub fn names_in(path: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            names.push(name);
        }
    }
    Ok(names)
}

fn parent_of(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("{} has no directory to write it in", path.display()),
        )
    })
}

/// A temporary in the destination's own directory — rename is only atomic
/// within one filesystem — hidden by a leading dot so that a reader listing
/// the directory never trips over it.
fn temporary_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = TEMPORARIES.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.tmp-{}-{unique}", process::id()))
}
/// Cut a file back to a length, without waiting for the disk.
///
/// One caller, and one reason: a job's log holding the bytes of a chunk the
/// hub appended and never counted, which the runner is about to send again
/// (see `JobDirStore::log_at`). Appending it twice would put a paragraph of
/// the job's output in the log twice, so the uncounted bytes come off first.
pub fn truncate(path: &Path, length: u64) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .truncate(false)
        .open(path)?
        .set_len(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Everything a reader could see at `path`, as bytes.
    fn read(path: &Path) -> Vec<u8> {
        let mut bytes = Vec::new();
        File::open(path)
            .expect("the destination exists")
            .read_to_end(&mut bytes)
            .expect("the destination reads");
        bytes
    }

    #[test]
    fn a_written_file_holds_exactly_what_was_written() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("record.json");
        write_file(&path, b"{\"first\":true}").expect("written");
        assert_eq!(read(&path), b"{\"first\":true}");
        // A rewrite replaces, and a shorter one leaves no tail of the longer.
        write_file(&path, b"{}").expect("rewritten");
        assert_eq!(read(&path), b"{}");
    }

    /// The property the whole store rests on: a write that fails partway
    /// leaves the previous record whole. The destination is never opened for
    /// writing, so there is no moment at which it is half a record.
    #[test]
    fn a_write_that_fails_partway_leaves_the_previous_record_intact() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("record.json");
        write_file(&path, b"{\"state\":\"running\"}").expect("written");

        let error = write_file_with(&path, |file| {
            file.write_all(b"{\"state\":\"fin")?;
            Err(io::Error::other("the writer was interrupted here"))
        })
        .expect_err("an interrupted write is reported");
        assert!(error.to_string().contains("interrupted"), "{error}");

        assert_eq!(
            read(&path),
            b"{\"state\":\"running\"}",
            "the half-written record was published"
        );
        assert_eq!(
            names_in(dir.path()).expect("the directory lists"),
            vec!["record.json".to_owned()],
            "a failed write left litter behind"
        );
    }

    /// A reader racing writers only ever sees one whole version. The contents
    /// differ in length on purpose: a torn read would almost certainly land
    /// between two of these sizes and be caught.
    #[test]
    fn a_reader_racing_writers_never_sees_a_half_written_record() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("record.json");
        let versions: Vec<Vec<u8>> = (0..4)
            .map(|n| {
                format!("{{\"n\":{n},\"pad\":\"{}\"}}", "x".repeat(1 << (12 + n))).into_bytes()
            })
            .collect();
        write_file(&path, &versions[0]).expect("written");

        let stop = Arc::new(AtomicBool::new(false));
        let writer = std::thread::spawn({
            let (path, versions, stop) = (path.clone(), versions.clone(), Arc::clone(&stop));
            move || {
                for _ in 0..50 {
                    for version in &versions {
                        write_file(&path, version).expect("written");
                    }
                }
                stop.store(true, Ordering::SeqCst);
            }
        });

        let mut reads = 0u32;
        while !stop.load(Ordering::SeqCst) {
            let seen = read(&path);
            assert!(
                versions.contains(&seen),
                "a reader saw {} bytes that are no version of the record",
                seen.len()
            );
            reads += 1;
        }
        writer.join().expect("the writer finished");
        assert!(reads > 0, "the reader never got to look");
    }

    #[test]
    fn appended_lines_arrive_whole_and_in_order() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("events.jsonl");
        for n in 0..3 {
            append_line(&path, &format!("{{\"n\":{n}}}")).expect("appended");
        }
        assert_eq!(
            String::from_utf8(read(&path)).expect("utf-8"),
            "{\"n\":0}\n{\"n\":1}\n{\"n\":2}\n"
        );
    }

    /// The unsynced append writes the same bytes in the same shape as the
    /// synced one. Only the waiting for the disk differs, and nothing a
    /// reader can see does — which is what makes D183 step A invisible to
    /// `job_events` and to the first consumer.
    #[test]
    fn an_unsynced_append_writes_exactly_what_a_synced_one_writes() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let synced = dir.path().join("synced.jsonl");
        let unsynced = dir.path().join("unsynced.jsonl");
        for n in 0..3 {
            let line = format!("{{\"n\":{n}}}");
            append_line(&synced, &line).expect("appended");
            append_line_unsynced(&unsynced, &line).expect("appended");
        }
        assert_eq!(read(&synced), read(&unsynced));
        assert_eq!(
            String::from_utf8(read(&unsynced)).expect("utf-8"),
            "{\"n\":0}\n{\"n\":1}\n{\"n\":2}\n"
        );
    }

    #[test]
    fn a_claimed_directory_cannot_be_claimed_twice() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let claim = dir.path().join("j0000000001");
        create_dir_exclusive(&claim).expect("the first claim wins");
        assert_eq!(
            create_dir_exclusive(&claim)
                .expect_err("the second claim loses")
                .kind(),
            ErrorKind::AlreadyExists
        );
        // Creating it as a directory that may already exist is not a claim.
        create_dir(&claim).expect("already there is not a failure");
    }

    #[test]
    fn listing_a_directory_hides_what_the_writer_left_in_it() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        write_file(&dir.path().join("job.json"), b"{}").expect("written");
        File::create(dir.path().join(".job.json.tmp-1-1")).expect("a leftover temporary");
        assert_eq!(
            names_in(dir.path()).expect("the directory lists"),
            vec!["job.json".to_owned()]
        );
    }

    /// Writing where no directory exists is an error carrying the path, not a
    /// panic: the hub refuses to start on a state root it cannot write.
    #[test]
    fn writing_into_a_missing_directory_names_the_path() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("no-such-directory/job.json");
        let error = write_file(&path, b"{}").expect_err("there is nowhere to write it");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }
}
