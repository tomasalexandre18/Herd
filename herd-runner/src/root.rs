//! The one data root, and the per-job workspace inside it (E-45).
//!
//! A runner is given exactly one directory it may write in. It is validated
//! at startup — a root it cannot use is a refusal naming the path, never a
//! runner that joins the hub and fails on its first job — and every path the
//! runner derives afterwards is resolved through [`DataRoot`], which refuses
//! anything that would leave it.
//!
//! What this does and does not promise. Everything *Herd* writes on a job's
//! behalf is inside the root: the job's working directory, its `HOME`, its
//! `TMPDIR`, and from M11 the content store. A job's own environment is built
//! from scratch rather than inherited, so no variable from the runner's shell
//! can point its writes elsewhere. What M4 does **not** do is stop a job that
//! names an absolute path outside the root: that needs a mount namespace, and
//! until one exists the promise is "Herd writes nowhere else, and a job is
//! given nowhere else to write", which is the promise the tests assert.
//!
//! ## The runner owns the workspace, and a job cannot blind it
//!
//! A job runs as the runner's own user, so it can `chmod 000` a directory it
//! made. Two things must never follow from that: a measurement the runner
//! takes as zero, and a tree the runner then fails to delete. So every walk
//! here **restores the access the runner needs** before giving up — the
//! directories are the runner's own — and a figure that still cannot be taken
//! is returned as an error, never as a zero (D23).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use herd_proto::JobId;

/// Where a runner keeps everything, unless its owner says otherwise.
const DEFAULT_DIR: &str = "herd-runner";

/// The directory holding one subdirectory per job.
const JOBS: &str = "jobs";

/// The file one runner holds a lock on for as long as it owns the root.
const LOCK: &str = ".herd-runner.lock";

/// Where a result the hub never acknowledged is left when the runner stops.
///
/// Deliberately *not* under `jobs/`: the startup reaper clears `jobs/` of
/// everything a previous life left ([`crate::reclaim`]), and a verdict has to
/// survive exactly the restart that reaper runs in.
const HELD: &str = "held-results";

/// Which hub state root everything in this data root belongs to.
///
/// One line of text: the identity the hub announced in its welcome, and
/// nothing else. A file rather than a field in memory because the thing it
/// answers questions about is on a disk: a verdict spooled under `HELD` is
/// *for a particular hub's state*, and a job id only means something inside
/// one. A hub whose state root is recreated mints `j0000000001` again, so a
/// spooled verdict from the root that is gone is plausible and false — and
/// this line is how the runner knows which it is holding.
///
/// Text and not JSON because it has one value and the runner carries no JSON
/// writer of its own; a human reading the data root can `cat` it.
const HUB_STATE_ROOT: &str = "hub-state-root";

/// One validated data root. Holding this value is the proof that the
/// directory exists, is a directory, is writable by this runner, and that no
/// other runner owns it.
///
/// The lock is held for the life of the process: the startup reaper reclaims
/// everything left under `jobs/`, which is only safe because exactly one
/// runner owns a root at a time.
#[derive(Debug, Clone)]
pub struct DataRoot {
    path: PathBuf,
    /// Kept open and flocked. Dropping the last clone releases the lock,
    /// which is also what a runner's death does for free.
    _lock: Arc<fs::File>,
}

impl DataRoot {
    /// Where the data root lives when the owner names none: the XDG state
    /// directory, which is per-user and never a system path. With neither
    /// `XDG_STATE_HOME` nor `HOME` set there is no sensible guess, and a
    /// guess is the one thing a data root must not be — so it is an error
    /// saying which variable to set.
    pub fn default_path() -> Result<PathBuf, String> {
        if let Some(state) = non_empty_var("XDG_STATE_HOME") {
            return Ok(PathBuf::from(state).join(DEFAULT_DIR));
        }
        if let Some(home) = non_empty_var("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("state")
                .join(DEFAULT_DIR));
        }
        Err(
            "no data root was given and neither HERD_DATA_ROOT, XDG_STATE_HOME \
             nor HOME is set, so there is nowhere to put one"
                .to_owned(),
        )
    }

    /// Validate a data root, creating it if it does not exist yet.
    ///
    /// Every failure names the path, because the only useful thing to say
    /// about a root a runner cannot use is which one it was.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let given = path.as_ref();
        if given.as_os_str().is_empty() {
            return Err("the data root is an empty path".to_owned());
        }
        // Asked before the directory is created, because `create_dir_all` on
        // a path that exists and is not a directory fails with "File
        // exists" — which names neither the problem nor the fix, and left
        // the `is_dir()` refusal below unreachable for the commonest way of
        // getting this wrong.
        if let Ok(metadata) = fs::symlink_metadata(given) {
            if !metadata.is_dir() && !resolves_to_a_directory(given) {
                return Err(format!(
                    "the data root {} is not a directory: it is {}",
                    given.display(),
                    if metadata.is_file() {
                        "a file"
                    } else if metadata.is_symlink() {
                        "a symlink to something that is not a directory"
                    } else {
                        "not a regular file either"
                    }
                ));
            }
        }
        fs::create_dir_all(given).map_err(|error| {
            format!(
                "the data root {} cannot be created: {error}",
                given.display()
            )
        })?;
        // Canonical from here on: a root reached through a symlink would make
        // "inside the root" a question about the link rather than the disk.
        let path = fs::canonicalize(given).map_err(|error| {
            format!(
                "the data root {} cannot be resolved: {error}",
                given.display()
            )
        })?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("the data root {} cannot be read: {error}", path.display()))?;
        if !metadata.is_dir() {
            return Err(format!(
                "the data root {} is not a directory",
                path.display()
            ));
        }
        if path.parent().is_none() {
            return Err(format!(
                "the data root {} is the filesystem root, which a runner may not own",
                path.display()
            ));
        }
        // Writability is tested by writing, not by reading permission bits:
        // a read-only mount, a full disk and a wrong owner all look the same
        // from the bits and all have to be refused here rather than later.
        let probe = path.join(".herd-writable");
        fs::write(&probe, b"herd").map_err(|error| {
            format!("the data root {} is not writable: {error}", path.display())
        })?;
        let _ = fs::remove_file(&probe);
        fs::create_dir_all(path.join(JOBS)).map_err(|error| {
            format!(
                "the data root {} cannot hold a jobs directory: {error}",
                path.display()
            )
        })?;
        let lock = lock_exclusively(&path.join(LOCK))?;
        Ok(Self {
            path,
            _lock: Arc::new(lock),
        })
    }

    /// The validated root itself.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where one directory per job lives. The startup reaper reads it.
    pub fn jobs_dir(&self) -> PathBuf {
        self.path.join(JOBS)
    }

    /// Where results the hub never acknowledged are left when this runner is
    /// told to stop.
    pub fn held_results_dir(&self) -> PathBuf {
        self.path.join(HELD)
    }

    /// Which hub state root this data root's durable things belong to, if a
    /// hub has ever said.
    ///
    /// `None` on a data root that has never been welcomed by a hub — a first
    /// start — and that is a meaningful answer rather than a missing one: a
    /// runner with no recorded state root is holding nothing it can attribute
    /// to one, so anything it finds in `held-results/` came from somewhere it
    /// cannot vouch for.
    pub fn hub_state_root(&self) -> Option<String> {
        let path = self.path.join(HUB_STATE_ROOT);
        let text = fs::read_to_string(path).ok()?;
        let id = text.trim().to_owned();
        (!id.is_empty()).then_some(id)
    }

    /// Write down which hub state root this data root now belongs to.
    ///
    /// Fsynced, because the question it answers is asked after a crash: a
    /// verdict in `held-results/` and a state root recorded only in a page
    /// cache would be a verdict nobody can attribute.
    pub fn remember_hub_state_root(&self, state_root: &str) -> Result<(), String> {
        let path = self.path.join(HUB_STATE_ROOT);
        fs::write(&path, format!("{state_root}\n"))
            .and_then(|()| fs::File::open(&path).and_then(|file| file.sync_all()))
            .map_err(|error| {
                format!(
                    "the hub's state root could not be written to {}: {error}",
                    path.display()
                )
            })
    }

    /// The file one unacknowledged result is written to, named for its job.
    ///
    /// The directory is created on the way, because this is called at the one
    /// moment where failing to write would destroy a verdict.
    pub fn held_result_path(&self, job_id: &JobId) -> Result<PathBuf, String> {
        let name = directory_name(job_id)?;
        let directory = self.held_results_dir();
        fs::create_dir_all(&directory).map_err(|error| {
            format!(
                "the directory {} for unacknowledged results cannot be created: {error}",
                directory.display()
            )
        })?;
        self.resolve(Path::new(HELD).join(format!("{name}.json")))
    }

    /// Resolve a path that must stay inside the root, refusing anything that
    /// would leave it and naming the path it refused.
    ///
    /// This is the single gate every derived path goes through, so that
    /// "nothing outside the root" is one function with one test rather than a
    /// habit repeated at each call site.
    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf, String> {
        let relative = relative.as_ref();
        let mut resolved = self.path.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(format!(
                        "{} is outside the data root {}",
                        relative.display(),
                        self.path.display()
                    ))
                }
            }
        }
        if !resolved.starts_with(&self.path) {
            return Err(format!(
                "{} is outside the data root {}",
                relative.display(),
                self.path.display()
            ));
        }
        Ok(resolved)
    }

    /// Make this job's workspace: its working directory, its `HOME` and its
    /// `TMPDIR`, all inside the root, all empty.
    pub fn open_workspace(&self, job_id: &JobId) -> Result<Workspace, String> {
        let name = directory_name(job_id)?;
        let base = self.resolve(Path::new(JOBS).join(&name))?;
        // A leftover from a previous life of the same id would make the disk
        // budget and the measurements lie, so the workspace starts empty.
        if base.exists() {
            reclaim_directory(&base, &self.path).map_err(|error| {
                format!(
                    "the workspace {} is left over and cannot be cleared: {error}",
                    base.display()
                )
            })?;
        }
        let work = base.join("work");
        let home = base.join("home");
        let tmp = base.join("tmp");
        for directory in [&work, &home, &tmp] {
            fs::create_dir_all(directory).map_err(|error| {
                format!(
                    "the workspace directory {} cannot be created: {error}",
                    directory.display()
                )
            })?;
        }
        Ok(Workspace {
            root: self.path.clone(),
            exchange_in: base.join("in"),
            exchange_out: base.join("out"),
            base,
            work,
            home,
            tmp,
        })
    }
}

/// One job's private corner of the data root, removed when the job ends.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The data root this workspace lives in: the boundary above which the
    /// runner owns nothing and restores nothing.
    root: PathBuf,
    base: PathBuf,
    work: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    /// `{in}` and `{out}`, named but not created until a declared command
    /// asks for them ([`Workspace::open_exchange`]).
    exchange_in: PathBuf,
    exchange_out: PathBuf,
}

impl Workspace {
    /// The job's working directory, and the only one it is told about.
    pub fn work_dir(&self) -> &Path {
        &self.work
    }

    /// Everything under this job, workspace included.
    pub fn base_dir(&self) -> &Path {
        &self.base
    }

    /// The `HOME` the job is given. Inside the root, so a tool that insists
    /// on writing to the home directory writes here.
    pub fn home_dir(&self) -> &Path {
        &self.home
    }

    /// The `TMPDIR` the job is given, for the same reason.
    pub fn tmp_dir(&self) -> &Path {
        &self.tmp
    }

    /// What a declared command's steps consume, for `{in}` (D107).
    ///
    /// Empty this milestone — nothing fills it until the content store (M11)
    /// — and that is the whole of what it is: a directory that exists, inside
    /// the root, so a hook naming `{in}` names somewhere real.
    pub fn in_dir(&self) -> &Path {
        &self.exchange_in
    }

    /// What a declared command's steps leave behind, for `{out}` (D107).
    pub fn out_dir(&self) -> &Path {
        &self.exchange_out
    }

    /// Make `{in}` and `{out}`, or say which one could not be made.
    ///
    /// Separate from [`DataRoot::open_workspace`] because only a declared
    /// command has steps to exchange anything between: an ad-hoc job is
    /// Herd's own command line and gets what it always got (E-11's reason for
    /// keeping both shapes). Both live under the job's own base directory,
    /// which [`DataRoot::resolve`] already put inside the data root, so there
    /// is one gate for "nothing outside the root" and not a second habit here
    /// (E-45).
    pub fn open_exchange(&self) -> Result<(), String> {
        for directory in [&self.exchange_in, &self.exchange_out] {
            fs::create_dir_all(directory).map_err(|error| {
                format!(
                    "the directory {} cannot be created: {error}",
                    directory.display()
                )
            })?;
        }
        Ok(())
    }

    /// How many bytes the job has written and not deleted, or why the figure
    /// could not be taken. Apparent sizes, summed, following no symlink: the
    /// figure the disk budget is about is what the job put there, not how the
    /// filesystem stores it.
    ///
    /// An error here is not a detail. The disk budget is enforced against
    /// this number, so a workspace that cannot be measured is a budget that
    /// cannot be enforced, and the job ends rather than being called a
    /// success (D23's "absent rather than zero", applied to the one figure a
    /// job can interfere with). Whether it ends at the *cap* or as a setup
    /// failure is [`Unmeasurable::refused`]'s answer (D166).
    pub fn measure(&self) -> Result<u64, Unmeasurable> {
        measure_tree(&self.base, &self.root)
    }

    /// Remove the workspace, restoring whatever access the job took away.
    ///
    /// The runner made this tree and owns every inode in it, so "permission
    /// denied" is never the last word: the modes are put back and the removal
    /// is tried again. A failure is still reported to the caller rather than
    /// swallowed — a workspace that will not go away is a disk filling up,
    /// which is the one thing invariant 2 forbids above all.
    pub fn reclaim(&self) -> Result<(), String> {
        reclaim_directory(&self.base, &self.root)
    }
}

/// Why a tree could not be measured, and whose problem that is (D166).
///
/// The sentence was always there; what was missing was the one bit beside it.
/// A workspace that cannot be measured is a disk budget that cannot be
/// enforced either way, so the job ends either way — but *why* it could not be
/// measured decides what the job's end is called. A path the runner is
/// **refused** is a fact about how the machine is set up, which is a person's
/// job to fix and never the requester's; anything else is the runner failing
/// at its own work, and D30's reading — the cap it could not be held to —
/// stays right for it.
///
/// So the kind rides with the sentence rather than being re-derived by
/// matching on its words, which is the one way this could have been done
/// wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmeasurable {
    /// The sentence a human reads, naming the path.
    pub why: String,
    /// The runner was not allowed to read a path inside the tree.
    pub refused: bool,
}

impl Unmeasurable {
    pub(crate) fn of(why: String, error: &io::Error) -> Self {
        Self {
            why,
            refused: error.kind() == io::ErrorKind::PermissionDenied,
        }
    }
}

impl fmt::Display for Unmeasurable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.why)
    }
}

/// Sum the apparent size of every regular file under a directory, restoring
/// the access the walk needs, and refusing to call an unreadable tree empty.
fn measure_tree(path: &Path, up_to: &Path) -> Result<u64, Unmeasurable> {
    let mut total = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match read_dir_restoring(&directory, up_to) {
            Ok(Some(entries)) => entries,
            // Gone while we walked: a job deleting its own files as it goes is
            // ordinary, and what is no longer there holds no bytes.
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            // DirEntry::metadata does not follow symlinks, so a link to a
            // huge file outside the root counts as the link it is, and a
            // link loop cannot be walked.
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Unmeasurable::of(
                        format!("{} cannot be measured: {error}", entry.path().display()),
                        &error,
                    ))
                }
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

/// Read a directory the runner owns, putting back the access a job removed.
///
/// `Ok(None)` means the directory is gone, which is not a failure to measure.
///
/// Shared with [`crate::outputs`], which walks `{out}` for the same reason
/// the measurement walks the workspace: a job must not be able to `chmod` its
/// way out of a figure the runner enforces, nor out of the record its own
/// output belongs in.
pub(crate) fn read_dir_restoring(
    directory: &Path,
    up_to: &Path,
) -> Result<Option<Vec<fs::DirEntry>>, Unmeasurable> {
    match collect_dir(directory) {
        Ok(entries) => return Ok(Some(entries)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {}
    }
    // The runner created this directory and every directory above it up to
    // its data root, so it can always take its own access back. A job that
    // chmods its way out of being measured — its own files, or the parents
    // they sit in — gets measured anyway.
    restore_ancestors(directory, up_to);
    restore_access(directory);
    match collect_dir(directory) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        // The restoration is the runner taking back access to a tree it
        // created. What survives it is a tree whose ownership is not the
        // runner's at all — the first consumer's jobs deposit theirs under
        // another user — and `chmod` on an inode somebody else owns is
        // refused however many times it is tried. That is the case D166 is
        // about, and `Unmeasurable::refused` is how it is told apart here
        // rather than by reading this sentence's words later.
        Err(error) => Err(Unmeasurable::of(
            format!(
                "{} cannot be read even after its mode was restored: {error}",
                directory.display()
            ),
            &error,
        )),
    }
}

fn collect_dir(directory: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        entries.push(entry?);
    }
    Ok(entries)
}

/// Remove a directory the runner owns, whatever a job did to its modes.
///
/// `up_to` is the data root: the modes are restored inside `base` and on
/// every directory between `base` and `up_to`, because a job can `chmod 000`
/// its workspace's *parent* just as easily as its own files, and the runner
/// owns every one of those directories. Nothing above `up_to` is ever
/// touched — the runner owns its data root and nothing else on the host.
pub(crate) fn reclaim_directory(base: &Path, up_to: &Path) -> Result<(), String> {
    let mut last = match remove_tree(base) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    // Two passes, because restoring the modes top-down can only reach what it
    // can already open: the first pass opens the way for the second.
    for _ in 0..2 {
        restore_ancestors(base, up_to);
        restore_tree(base);
        match remove_tree(base) {
            Ok(()) => return Ok(()),
            Err(error) => last = error,
        }
    }
    Err(format!(
        "{} could not be removed even after its modes were restored: {last}",
        base.display()
    ))
}

fn remove_tree(base: &Path) -> io::Result<()> {
    match fs::remove_dir_all(base) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Put the runner's access back on every directory from `base`'s parent up
/// to `up_to` inclusive, deepest first. Does nothing unless `up_to` really is
/// an ancestor of `base`: this must never walk out of the data root.
fn restore_ancestors(base: &Path, up_to: &Path) {
    if !base.starts_with(up_to) {
        return;
    }
    let mut here = base.parent();
    while let Some(directory) = here {
        restore_access(directory);
        if directory == up_to {
            return;
        }
        here = directory.parent();
    }
}

/// Put back, on every directory under `base`, the access the runner needs to
/// walk it and to unlink what is in it. Best effort and top-down: a directory
/// has to be opened before its children can be reached.
fn restore_tree(base: &Path) {
    let mut pending = vec![base.to_path_buf()];
    while let Some(directory) = pending.pop() {
        restore_access(&directory);
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            // Directories only, by lstat: a file needs no mode of its own to
            // be unlinked, and a symlink's target is none of our business.
            if entry.metadata().map(|data| data.is_dir()).unwrap_or(false) {
                pending.push(entry.path());
            }
        }
    }
}

/// Give the runner read, write and search on one directory it owns.
#[cfg(unix)]
fn restore_access(directory: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = fs::symlink_metadata(directory) else {
        return;
    };
    if !metadata.is_dir() {
        return;
    }
    let mode = metadata.permissions().mode();
    let _ = fs::set_permissions(directory, fs::Permissions::from_mode(mode | 0o700));
}

#[cfg(not(unix))]
fn restore_access(_directory: &Path) {}

/// How long the data root's lock is retried before a refusal is believed, and
/// how often. Long enough to outlast a `fork` reaching its `exec` by orders of
/// magnitude; short enough that a genuinely occupied root is reported at once.
const LOCK_PATIENCE: std::time::Duration = std::time::Duration::from_millis(500);
const LOCK_RETRY_EVERY: std::time::Duration = std::time::Duration::from_millis(10);

/// Take the root's lock, or refuse naming the runner that holds it.
///
/// One runner per data root, for the same reason as one hub per state root
/// (D21): the startup reaper reclaims everything left under `jobs/`, and
/// "left" can only mean "left by a runner that is gone" if no second runner
/// is using the same directory. The lock dies with the process, so a runner
/// that was killed leaves the root free.
///
/// # Why this is not one try
///
/// `flock` belongs to an *open file description*, and `fork` duplicates every
/// one of those. So between a `fork` and its `exec` — every job this runner
/// spawns, and every external busy probe it reads (M5) — a child briefly
/// holds this lock too, even though the descriptor is close-on-exec and goes
/// the instant the child execs. A runner starting in that window would be
/// told the root belongs to a process that has already let go.
///
/// Microseconds, and not worth a design: the lock is retried for
/// [`LOCK_PATIENCE`] before the refusal is believed. A root a live runner
/// really holds stays held for the whole window, so the refusal that matters
/// still happens — later by a fraction of a second, on a path that ends the
/// process anyway.
#[cfg(unix)]
fn lock_exclusively(path: &Path) -> Result<fs::File, String> {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("the lock file {} cannot be opened: {error}", path.display()))?;
    let give_up_at = std::time::Instant::now() + LOCK_PATIENCE;
    let locked = loop {
        // Safe: flock on an open descriptor, with no allocation involved.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break true;
        }
        if std::time::Instant::now() >= give_up_at {
            break false;
        }
        std::thread::sleep(LOCK_RETRY_EVERY);
    };
    if !locked {
        let holder = fs::read_to_string(path).unwrap_or_default();
        let holder = holder.trim();
        let holder = if holder.is_empty() {
            "another runner".to_owned()
        } else {
            format!("the runner with pid {holder}")
        };
        return Err(format!(
            "the data root {} is already owned by {holder}: one runner per data \
             root, so that a restarted runner can reclaim what a dead one left \
             behind. Give this runner its own --data-root.",
            path.parent().unwrap_or(path).display()
        ));
    }
    let _ = file.set_len(0);
    let _ = write!(file, "{}", std::process::id());
    let _ = file.flush();
    Ok(file)
}

#[cfg(not(unix))]
fn lock_exclusively(path: &Path) -> Result<fs::File, String> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|error| format!("the lock file {} cannot be opened: {error}", path.display()))
}

/// A job's id as one directory name.
///
/// Ids are opaque (`JobId`), and an opaque string reaching the filesystem is
/// exactly how `../..` ends up in a path. So the name is built from the
/// characters that are safe and nothing else, and an id that has none of them
/// is refused naming itself.
fn directory_name(job_id: &JobId) -> Result<String, String> {
    let id = job_id.as_str();
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '_') {
        return Err(format!(
            "the job id {id:?} gives no usable directory name inside the data root"
        ));
    }
    Ok(name)
}

/// Whether a path leads to a directory once links are followed. A symlink to
/// a directory is a perfectly good data root; a symlink to a file is not.
fn resolves_to_a_directory(path: &Path) -> bool {
    fs::metadata(path)
        .map(|data| data.is_dir())
        .unwrap_or(false)
}

fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[test]
    fn a_fresh_root_is_created_and_holds_a_jobs_directory() {
        let temp = TempDir::new("root-fresh");
        let root = DataRoot::open(temp.path().join("state")).expect("a fresh root is usable");
        assert!(root.path().join(JOBS).is_dir());
        assert!(
            root.path().is_absolute(),
            "a root is canonical: {}",
            root.path().display()
        );
    }

    /// E-45's refusal: every reason names the path, because that is the only
    /// thing worth saying about a root that cannot be used.
    #[test]
    fn a_root_that_is_a_file_is_refused_naming_it() {
        let temp = TempDir::new("root-file");
        let file = temp.path().join("not-a-directory");
        fs::write(&file, b"i am a file").expect("the fixture writes");
        let error = DataRoot::open(&file).expect_err("a file is not a data root");
        assert!(
            error.contains(&file.display().to_string()),
            "the refusal must name the path: {error}"
        );
        // And it names what is wrong. `create_dir_all` answered this with
        // "cannot be created: File exists", which says neither the problem
        // nor the fix — and docs/runner-confinement.md claimed the `is_dir`
        // refusal below, which was unreachable for the commonest mistake.
        assert!(
            error.contains("is not a directory") && error.contains("a file"),
            "the refusal must say it is a file, not that something exists: {error}"
        );
    }

    /// A symlink is judged by what it points at: to a directory it is a
    /// usable root, to a file it is refused like the file it is.
    #[test]
    #[cfg(unix)]
    fn a_root_reached_through_a_symlink_is_judged_by_its_target() {
        let temp = TempDir::new("root-symlink");
        let file = temp.path().join("a-file");
        fs::write(&file, b"i am a file").expect("the fixture writes");
        let to_file = temp.path().join("link-to-file");
        std::os::unix::fs::symlink(&file, &to_file).expect("the fixture links");
        let error = DataRoot::open(&to_file).expect_err("a link to a file is not a root");
        assert!(error.contains("not a directory"), "{error}");

        let directory = temp.path().join("a-directory");
        fs::create_dir(&directory).expect("the fixture makes it");
        let to_directory = temp.path().join("link-to-directory");
        std::os::unix::fs::symlink(&directory, &to_directory).expect("the fixture links");
        let root = DataRoot::open(&to_directory).expect("a link to a directory is a root");
        assert_eq!(
            root.path(),
            fs::canonicalize(&directory).expect("canonical").as_path(),
            "a root is canonical, so being reached through a link changes nothing after"
        );
    }

    #[test]
    fn a_root_that_cannot_be_created_is_refused_naming_it() {
        let temp = TempDir::new("root-under-file");
        let file = temp.path().join("blocker");
        fs::write(&file, b"i am a file").expect("the fixture writes");
        let under = file.join("root");
        let error = DataRoot::open(&under).expect_err("there is a file in the way");
        assert!(
            error.contains(&under.display().to_string()),
            "the refusal must name the path: {error}"
        );
    }

    #[test]
    fn an_empty_root_path_is_refused() {
        let error = DataRoot::open("").expect_err("an empty path is not a root");
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn the_filesystem_root_is_not_a_data_root() {
        let error = DataRoot::open("/").expect_err("a runner may not own /");
        assert!(error.contains("filesystem root"), "{error}");
    }

    /// The gate every derived path goes through. A path that would leave the
    /// root is refused, and the refusal names it.
    #[test]
    fn a_path_leaving_the_root_is_refused_naming_it() {
        let temp = TempDir::new("resolve");
        let root = DataRoot::open(temp.path()).expect("usable");
        for escape in ["../etc/passwd", "jobs/../../etc/passwd", "/etc/passwd"] {
            let error = root
                .resolve(escape)
                .expect_err("an escaping path must be refused");
            assert!(
                error.contains(escape) && error.contains(&root.path().display().to_string()),
                "the refusal must name both the path and the root: {error}"
            );
        }
        let inside = root
            .resolve("jobs/j1/work")
            .expect("a path inside resolves");
        assert!(inside.starts_with(root.path()));
    }

    /// A job id is opaque, so it is treated as hostile: an id shaped like a
    /// path cannot reach outside the root through the workspace.
    #[test]
    fn a_job_id_shaped_like_a_path_cannot_escape_the_root() {
        let temp = TempDir::new("hostile-id");
        let root = DataRoot::open(temp.path()).expect("usable");
        let workspace = root
            .open_workspace(&JobId::new("../../escaped"))
            .expect("a hostile id is sanitised, not fatal");
        assert!(
            workspace.base_dir().starts_with(root.path()),
            "{} escaped the root",
            workspace.base_dir().display()
        );
        let error = directory_name(&JobId::new("///")).expect_err("no usable name");
        assert!(error.contains("///"), "{error}");
    }

    #[test]
    fn a_workspace_is_empty_three_directories_and_goes_away_again() {
        let temp = TempDir::new("workspace");
        let root = DataRoot::open(temp.path()).expect("usable");
        let job = JobId::new("j7");
        let workspace = root.open_workspace(&job).expect("made");
        for directory in [
            workspace.work_dir(),
            workspace.home_dir(),
            workspace.tmp_dir(),
        ] {
            assert!(directory.is_dir(), "{}", directory.display());
            assert!(directory.starts_with(root.path()));
        }
        assert_eq!(workspace.measure(), Ok(0), "a fresh workspace is empty");

        fs::write(workspace.work_dir().join("output"), vec![0u8; 5_000]).expect("the job writes");
        assert!(workspace.measure().expect("measurable") >= 5_000);

        // A second workspace for the same id starts empty again.
        let again = root.open_workspace(&job).expect("made again");
        assert_eq!(again.measure(), Ok(0));

        again.reclaim().expect("reclaimed");
        assert!(!again.base_dir().exists());
        again.reclaim().expect("reclaiming twice is not a failure");
    }

    /// `{in}` and `{out}` are made only when a declared command asks for them,
    /// and they are inside the one data root like everything else a job is
    /// given (E-45, D107).
    #[test]
    fn the_exchange_directories_are_asked_for_and_are_inside_the_root() {
        let temp = TempDir::new("exchange");
        let root = DataRoot::open(temp.path()).expect("usable");
        let workspace = root
            .open_workspace(&JobId::new("j8"))
            .expect("a workspace is made");
        assert!(
            !workspace.in_dir().exists() && !workspace.out_dir().exists(),
            "an ad-hoc job gets what it always got, and nothing more"
        );

        workspace
            .open_exchange()
            .expect("the two directories are made");
        for directory in [workspace.in_dir(), workspace.out_dir()] {
            assert!(directory.is_dir(), "{}", directory.display());
            assert!(
                directory.starts_with(root.path()),
                "{} has to be inside the data root {}",
                directory.display(),
                root.path().display()
            );
            assert!(directory.starts_with(workspace.base_dir()));
        }
        assert_eq!(
            workspace.measure(),
            Ok(0),
            "two empty directories hold no bytes of anybody's budget"
        );
        // Asked for twice is not a failure: an attempt that was given back
        // and tried again finds them already there.
        workspace
            .open_exchange()
            .expect("asking twice is not an error");

        workspace.reclaim().expect("reclaimed");
        assert!(!workspace.in_dir().exists() && !workspace.out_dir().exists());
    }

    /// Defect 1 of the M4 review, as a unit: a job that hides what it wrote
    /// behind a directory the runner cannot enter is measured anyway, and a
    /// tree that truly cannot be read is an error rather than a zero.
    #[test]
    #[cfg(unix)]
    fn a_directory_a_job_made_unreadable_is_still_measured() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new("blinded");
        let root = DataRoot::open(temp.path()).expect("usable");
        let workspace = root.open_workspace(&JobId::new("blinded")).expect("made");
        let hidden = workspace.work_dir().join("hidden");
        fs::create_dir(&hidden).expect("the job makes a directory");
        fs::write(hidden.join("big"), vec![7u8; 300_000]).expect("the job writes in it");
        // Write and search but no read: `read_dir` fails with EACCES, which is
        // exactly what used to be counted as zero bytes.
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o300)).expect("chmod");

        let measured = workspace
            .measure()
            .expect("the runner owns the directory, so it can take its access back");
        assert!(
            measured >= 300_000,
            "the hidden file was not counted: {measured} bytes"
        );
        // And the mode the walk restored does not stop the removal either.
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o000)).expect("chmod");
        workspace
            .reclaim()
            .expect("an owned tree is always reclaimable");
        assert!(!workspace.base_dir().exists());
    }

    /// Defect 2: the modes a job leaves behind never keep a workspace alive.
    #[test]
    #[cfg(unix)]
    fn a_workspace_a_job_locked_shut_is_still_reclaimed() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new("locked-shut");
        let root = DataRoot::open(temp.path()).expect("usable");
        let workspace = root.open_workspace(&JobId::new("locked")).expect("made");
        let deep = workspace.work_dir().join("d");
        fs::create_dir(&deep).expect("the job makes a directory");
        fs::write(deep.join("f"), b"payload").expect("the job writes");
        fs::set_permissions(&deep, fs::Permissions::from_mode(0o000)).expect("chmod 000 d");
        fs::set_permissions(workspace.work_dir(), fs::Permissions::from_mode(0o500))
            .expect("chmod 500 .");

        workspace
            .reclaim()
            .expect("the runner owns the workspace unconditionally");
        assert!(
            !workspace.base_dir().exists(),
            "{} survived",
            workspace.base_dir().display()
        );
    }

    /// Which of the two endings a failed measurement is turns on the error's
    /// **kind** and never on the words of its sentence (D166).
    #[test]
    fn a_refused_measurement_is_told_from_any_other_by_the_kind_of_its_error() {
        let refused = Unmeasurable::of(
            "/srv/herd/out/packs cannot be read".to_owned(),
            &io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert!(refused.refused, "{refused}");
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::InvalidData,
            io::ErrorKind::Other,
        ] {
            let other = Unmeasurable::of("whatever".to_owned(), &io::Error::from(kind));
            assert!(
                !other.refused,
                "{kind:?} is the runner failing at its own work, which is still D30's cap"
            );
        }
    }

    /// And the same judgement on a **real** refusal, reached the way the
    /// consumer's jobs reach it: a path the runner cannot take its access
    /// back on, because what is in the way is not a mode it owns.
    ///
    /// No second user is needed to make one. The runner restores modes
    /// between the workspace and its data root and **nothing above the data
    /// root**, which is the right line to draw — a runner owns its data root
    /// and nothing else on the host — so a directory above it that the walk
    /// cannot search is an access that stays refused however many times the
    /// restoration runs. That is the same answer the consumer's `server:users`
    /// trees give for a different reason, and it is the answer this is about.
    #[test]
    #[cfg(unix)]
    fn a_path_the_runner_cannot_take_its_access_back_on_is_refused_and_not_capped() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new("refused");
        let outside = temp.path().join("not-ours");
        let inside = outside.join("data-root");
        fs::create_dir_all(&inside).expect("the fixture makes both");
        let root = DataRoot::open(&inside).expect("usable");
        let workspace = root.open_workspace(&JobId::new("j1197")).expect("made");
        fs::write(workspace.work_dir().join("pack"), vec![3u8; 4_096]).expect("the job writes");

        // Above the data root, so nothing in the walk's restoration reaches
        // it. Put back at once, whatever the assertions below do.
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o000)).expect("chmod 000");
        let measured = workspace.measure();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).expect("chmod back");

        match measured {
            Err(refused) => {
                assert!(
                    refused.refused,
                    "an access the runner cannot take back is a setup failure, not a budget: \
                     {refused}"
                );
                assert!(
                    refused
                        .why
                        .contains(&workspace.work_dir().display().to_string())
                        || refused
                            .why
                            .contains(&workspace.base_dir().display().to_string()),
                    "the sentence names the path: {refused}"
                );
            }
            // Running as a user the mode does not bind — root — which is a
            // host this assertion cannot be made on rather than one it
            // passes on (D27).
            Ok(measured) => crate::testing::skipped(
                "a_path_the_runner_cannot_take_its_access_back_on_is_refused_and_not_capped",
                &format!(
                    "a directory at mode 000 did not stop this user from walking it \
                     ({measured} bytes were measured through it), which is what running as \
                     root looks like — a non-root user would prove that a refused path is \
                     reported as a setup failure and never as a disk cap"
                ),
            ),
        }
    }

    /// A measurement is taken, or it is an error. It is never a zero that
    /// means "could not look" (D23).
    #[test]
    fn a_tree_that_cannot_be_read_is_an_error_not_a_zero() {
        let temp = TempDir::new("unmeasurable");
        let file = temp.path().join("i-am-a-file");
        fs::write(&file, b"x").expect("the fixture writes");
        // A file where a directory is expected: read_dir fails with ENOTDIR,
        // which no chmod can fix, so the figure must be absent.
        let error = measure_tree(&file, temp.path()).expect_err("a file is not a tree to walk");
        assert!(error.why.contains(&file.display().to_string()), "{error}");
        // ENOTDIR is the runner failing at its own work, not a path somebody
        // else owns: D30's reading — the cap that could not be held to —
        // stays right for it, and D166's does not apply.
        assert!(
            !error.refused,
            "a directory that is not a directory is not an access that was refused: {error}"
        );
        // A path that is simply not there holds nothing, which is a real zero.
        assert_eq!(
            measure_tree(&temp.path().join("never-made"), temp.path()),
            Ok(0)
        );
    }

    /// One runner per data root, so that the startup reaper can tell what a
    /// dead runner left from what a live one is using.
    #[test]
    fn a_second_runner_is_refused_the_same_data_root_naming_the_holder() {
        let temp = TempDir::new("one-owner");
        let first = DataRoot::open(temp.path()).expect("the first runner owns it");
        let started = std::time::Instant::now();
        let error = DataRoot::open(temp.path()).expect_err("the second is refused");
        assert!(
            error.contains(&std::process::id().to_string()),
            "the refusal must name the holder: {error}"
        );
        assert!(error.contains("--data-root"), "{error}");
        // The lock is retried before the refusal is believed, because a
        // `fork` holds it until its `exec` (see `lock_exclusively`). What that
        // must not become is a wait: an occupied root is reported while
        // somebody is still watching the terminal.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "an occupied root took {:?} to be reported",
            started.elapsed()
        );
        drop(first);
        // The lock dies with its holder, so the root is free again.
        DataRoot::open(temp.path()).expect("a released root is usable again");
    }
}
