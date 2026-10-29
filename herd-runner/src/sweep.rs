//! The collections a previous life of this runner left under `outputs/`.
//!
//! A streamed collection's bytes are renamed out of the workspace into
//! `{data root}/outputs/{job}-{attempt}` ([`crate::outputs::spool_into`]) and
//! removed again only when the hub has said every file of them is on its disk
//! ([`crate::Session::files_acknowledged`]). That acknowledgement is the
//! **one** path that removes a spool, and it needs the runner process that
//! produced the collection to still be holding it in memory. A runner that is
//! restarted holds nothing: the directory stays on the disk, no hub is ever
//! answered about it, and nothing in the tree so much as enumerates it. 125 of
//! them holding 65 GB is what that looks like after a fortnight.
//!
//! So this is the other half of [`crate::spool`], which does exactly this for
//! a *verdict*: at startup the spools are found and **brought back under the
//! offer-and-acknowledge path**, and nothing is deleted here.
//!
//! ## Why re-registering and not a reaper
//!
//! Deleting is the dangerous half. A spool is the only copy of a result until
//! the hub has it (D19), so a sweep that deleted first would be a mechanism
//! for destroying exactly what the spool exists to protect. Re-registering
//! costs nothing when the hub already has the files — it is told so, and the
//! acknowledgement removes the spool for free, through the code that was
//! always meant to remove it — and it recovers the result when the hub does
//! not.
//!
//! ## What the hub says, and what silence means
//!
//! A runner that joins is told, unprompted, about every collection this hub
//! is still owed from it (`ask_for_the_files_still_owed`, one
//! [`herd_proto::FromHub::JobFilesHeld`] each). That list is the hub's whole
//! answer, derived from its records, and it is sent **before** the hub reads
//! anything from the connection — so it is already in hand by the time the
//! answer to this runner's first health report arrives
//! ([`herd_proto::FromHub::HealthRecorded`], frames on one link being
//! ordered). That gives the runner a barrier it can read without a new frame:
//!
//! - the hub asks for a swept collection → it resumes on the ordinary path,
//!   finishes, is acknowledged, and the spool goes;
//! - the hub never mentions it → the hub is not owed it. Either it holds
//!   every byte already, or it will never take them: the job was cancelled,
//!   the attempt was written off, the record is gone, the sweeper evicted it.
//!   Each of those is **terminal**, and a spool offered for ever would be the
//!   same leak with more network.
//!
//! Terminal here means [`UNCLAIMED`], not `rm -rf`. Silence is weaker
//! evidence than a sentence: a hub whose record scan fails answers *every*
//! runner with the same silence, and a sweep that deleted on it would empty
//! every spool on the fleet over one hub fault. So the bytes are renamed,
//! named in full with their size, and never offered again — the leak stops
//! growing, the disk becomes a line in the runner's own log instead of a
//! surprise, and removing it stays a human's decision.
//!
//! ## Bounded, because this is the startup path
//!
//! A runner must start or refuse to start, and stop in five seconds
//! (invariant 2); D134 refused to put an unbounded step on the startup path
//! for exactly this reason. This walk opens nothing and hashes nothing — one
//! `readdir` per directory and one `lstat` per entry, which is what makes a
//! 65 GB sweep as cheap as an empty one — and it stops after
//! [`MOST_ENTRIES_SWEPT`] entries whatever is left. A spool the walk did not
//! finish is **not** registered: a manifest missing half its files would read
//! as a collection the hub holds in full, which is the one mistake here that
//! loses bytes.
//!
//! ## A refusal names the path and the error
//!
//! That refusal is right, and on its own it is unactionable. The first
//! consumer's upgrade to v0.4.1 refused 50 spools with it, on a machine
//! **nobody in this project may log into** (invariant 9, D161), and the
//! sentence named no file and no error — so its owner could not tell a mode
//! they can change from a file that was truncated from a disk that is going,
//! which are three different afternoons. The same run proved how much rides
//! on these lines: all 75 collections the sweep did offer were already held
//! whole by the hub, 37.6 GiB agreeing on both sides, so a sweep's own
//! sentences are the only window an operator has onto tens of gigabytes of
//! decisions.
//!
//! So [`walk`] **keeps** the first path it could not read and the error the
//! kernel gave for it ([`NotWhole`]), and the refusal carries both beside
//! what it already said. Kept, not looked for again: this is the startup
//! path, and re-walking 37 GiB of spools to find out why the first walk
//! stopped would be a worse defect than the one it explains. One line per
//! spool whatever is wrong with it, with the rest counted — a spool whose
//! every directory is shut is not fifty lines.
//!
//! What the walk does **not** do is read a file. One `lstat` per entry is the
//! bound, so a regular file nobody can open is still a whole collection as
//! far as this is concerned, and the thing that stops a walk is a *directory*
//! it may not read or traverse — which is exactly the consumer's hypothesis,
//! a pack tree a build job left owned by another user.

use std::fs;
use std::path::{Path, PathBuf};

use herd_proto::JobId;

use crate::root::DataRoot;
use crate::size::measured_bytes;

/// What a spool no hub will ever ask for is renamed to.
pub(crate) const UNCLAIMED: &str = "unclaimed";

/// What a spool produced for another hub's state root is renamed to.
///
/// The same word [`crate::spool`] keeps a verdict under, for the same reason:
/// a job id means something only inside one hub state, so bytes produced for
/// a state root that is gone can never be attached to a record again — and
/// offering them to the hub that replaced it would write one job's output
/// onto another job's record.
pub(crate) const FOREIGN: &str = "foreign-state-root";

/// How many directory entries one sweep looks at, over every spool together.
///
/// A bound rather than a budget of time, so that the cost of a start is a
/// fixed number of syscalls and not a function of what a job once wrote.
pub(crate) const MOST_ENTRIES_SWEPT: usize = 50_000;

/// One file of a spool, as the disk has it.
///
/// The two figures the resumption needs and no more: the path the manifest
/// announced it under, and how long it is. The digest is not here and is not
/// wanted — the hub verifies a collection against the manifest **on its own
/// record**, which is the only copy of it that counts, and a runner that
/// re-announced one would be offering a second opinion about bytes it is
/// merely holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpooledFile {
    pub path: String,
    pub len: u64,
}

/// One attempt's collection, found on the disk at startup.
#[derive(Debug, Clone)]
pub struct Swept {
    path: PathBuf,
    job_id: JobId,
    attempt: u32,
    files: Vec<SpooledFile>,
    bytes: u64,
}

impl Swept {
    pub fn job_id(&self) -> &JobId {
        &self.job_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What it holds, in the order the walk sorted it.
    #[cfg(test)]
    pub fn files(&self) -> &[SpooledFile] {
        &self.files
    }

    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn into_files(self) -> Vec<SpooledFile> {
        self.files
    }

    /// The line a spool handed back to the hub's own path is worth.
    pub fn offered(&self) -> String {
        format!(
            "picked up the collection of {} (attempt {}, {} in {} file(s)) from {}: it was \
             never acknowledged, and it is offered to the hub again",
            self.job_id,
            self.attempt,
            measured_bytes(self.bytes),
            self.files.len(),
            self.path.display()
        )
    }

    /// These bytes were produced for a hub state root that is gone, so no hub
    /// can record them: they are not offered, and they are kept under a name
    /// that says why.
    pub fn for_a_foreign_state_root(&self, why: &str) -> String {
        format!(
            "the collection of {} (attempt {}, {} in {} file(s)) was produced for another \
             hub's state root ({why}); no hub can record it now, so it is NOT offered. {}",
            self.job_id,
            self.attempt,
            measured_bytes(self.bytes),
            self.files.len(),
            set_aside(&self.path, FOREIGN)
        )
    }
}

/// Every collection a previous life of this runner left under `outputs/`, and
/// what to say about everything in there that is not one.
///
/// Nothing is deleted and nothing is opened. One bad entry is named and the
/// sweep carries on: a spool with a name nobody can parse must not be able to
/// stop a runner from starting, nor to hide the 124 beside it.
pub fn pick_up(root: &DataRoot) -> (Vec<Swept>, Vec<String>) {
    let directory = root.outputs_dir();
    let mut swept = Vec::new();
    let mut said = Vec::new();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        // No directory at all is the ordinary case: this runner has never
        // streamed a collection, or every one it streamed was acknowledged.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (swept, said),
        Err(error) => {
            return (
                swept,
                vec![format!(
                    "COULD NOT read {} for collections a previous runner left: {error}. If \
                     one is in there, it is not being offered and its disk is not coming \
                     back",
                    directory.display()
                )],
            )
        }
    };
    let mut budget = MOST_ENTRIES_SWEPT;
    let mut set_aside_already = 0usize;
    let mut names: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.path()),
            Err(error) => said.push(format!(
                "an entry of {} could not be read and is left alone: {error}",
                directory.display()
            )),
        }
    }
    // One order, whatever the filesystem's is, so a log reads the same twice.
    names.sort();
    for path in names {
        match one(&path, &mut budget) {
            Found::Spool(spool) => swept.push(spool),
            Found::SetAside => set_aside_already += 1,
            Found::Said(line) => said.push(line),
        }
    }
    if set_aside_already > 0 {
        said.push(format!(
            "{set_aside_already} spool(s) under {} have already been set aside under a \
             .{UNCLAIMED} or .{FOREIGN} name: no hub will ever ask for them, this runner \
             does not offer them, and removing them is yours to do",
            directory.display()
        ));
    }
    if budget == 0 {
        said.push(format!(
            "this sweep of {} stopped after {MOST_ENTRIES_SWEPT} entries, which is its \
             bound: a runner's start is a fixed number of syscalls and never a function of \
             what a job once wrote. Whatever it had not reached is left exactly as it is \
             and the next start looks again",
            directory.display()
        ));
    }
    (swept, said)
}

/// What one entry of `outputs/` turned out to be.
enum Found {
    Spool(Swept),
    /// Already dealt with by an earlier sweep, and counted rather than named
    /// one line at a time.
    SetAside,
    /// Left exactly as it is, with the sentence saying why.
    Said(String),
}

fn one(path: &Path, budget: &mut usize) -> Found {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Found::Said(format!(
            "{} has a name that is not UTF-8, so it is not a spool this runner wrote \
             ({{job}}-{{attempt}} is ASCII); it is left exactly as it is",
            path.display()
        ));
    };
    if name.ends_with(&format!(".{UNCLAIMED}")) || name.ends_with(&format!(".{FOREIGN}")) {
        return Found::SetAside;
    }
    // Never `metadata()`: a symlink here is not followed, for the reason a
    // symlink under `{out}` is not (E-45). What it points at is not this
    // runner's to walk and certainly not its to remove.
    let kind = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Found::Said(format!(
                "{} went away while this runner was looking at it; nothing is held for it",
                path.display()
            ))
        }
        Err(error) => {
            return Found::Said(format!(
                "{} could not be looked at ({error}), so this runner cannot tell whether it \
                 is a collection somebody is waiting for; it is left exactly as it is",
                path.display()
            ))
        }
    };
    if !kind.is_dir() {
        return Found::Said(format!(
            "{} is {} and not a directory, so it is not a spool this runner wrote; it is \
             left exactly as it is",
            path.display(),
            if kind.is_symlink() {
                "a symlink, which is never followed".to_owned()
            } else {
                format!("{kind:?}")
            }
        ));
    }
    let Some((job_id, attempt)) = parse_name(name) else {
        return Found::Said(format!(
            "{} is a directory whose name is not {{job}}-{{attempt}}, so this runner cannot \
             tell which collection it would be; it is not offered to any hub and it is left \
             exactly as it is",
            path.display()
        ));
    };
    let (files, bytes, not_whole) = walk(path, budget);
    if let Some(not_whole) = not_whole {
        return Found::Said(format!(
            "{} could not be read all the way through, so what this runner can see of it is \
             not the whole collection; it is NOT offered, because a part of a manifest \
             offered as the whole of one reads to the hub as a collection it already holds. \
             It is left exactly as it is. {}",
            path.display(),
            not_whole.sentence()
        ));
    }
    if files.is_empty() {
        return Found::Said(format!(
            "{} holds no regular file, so there is nothing in it any record names; it is \
             not offered and it is left exactly as it is",
            path.display()
        ));
    }
    Found::Spool(Swept {
        path: path.to_path_buf(),
        job_id,
        attempt,
        files,
        bytes,
    })
}

/// Split `{job}-{attempt}` the way [`crate::outputs::spool_path`] joined it.
///
/// At the **last** hyphen, because a job id is the hub's to shape and this
/// runner only ever appended `-{attempt}` to it; and the attempt must be a
/// number, which is what keeps `j7-results` from reading as a spool.
fn parse_name(name: &str) -> Option<(JobId, u32)> {
    let (job, attempt) = name.rsplit_once('-')?;
    if job.is_empty() {
        return None;
    }
    let attempt: u32 = attempt.parse().ok()?;
    Some((JobId::new(job), attempt))
}

/// Every regular file under a spool, with the path the manifest would have
/// announced it under.
///
/// The same rules the collection was made with ([`crate::outputs`]): regular
/// files only, no symlink followed, a name that is not UTF-8 or a path a
/// record cannot hold left out — because those were left out of the manifest
/// too, and a file the hub was never promised is one it refuses by name.
///
/// Returns why the walk did not finish, if it did not: the budget ran out or
/// something would not read, and then nothing of this spool may be trusted as
/// a whole collection. [`None`] is a whole one.
fn walk(spool: &Path, budget: &mut usize) -> (Vec<SpooledFile>, u64, Option<NotWhole>) {
    let mut files: Vec<SpooledFile> = Vec::new();
    let mut bytes = 0u64;
    let mut stopped = NotWhole::default();
    let mut pending = vec![(spool.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                stopped.could_not_read(directory, &error);
                continue;
            }
        };
        for entry in entries {
            if *budget == 0 {
                stopped.bound = true;
                return (files, bytes, Some(stopped));
            }
            *budget -= 1;
            let entry = match entry {
                Ok(entry) => entry,
                // No path of its own to give: `readdir` failed part way
                // through, so the directory being read is the whole of what
                // is known about where.
                Err(error) => {
                    stopped.could_not_read(directory.clone(), &error);
                    continue;
                }
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) => {
                    stopped.could_not_read(entry.path(), &error);
                    continue;
                }
            };
            let kind = metadata.file_type();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                pending.push((entry.path(), relative));
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            if herd_proto::check_relative(&relative).is_err() {
                continue;
            }
            bytes += metadata.len();
            files.push(SpooledFile {
                path: relative,
                len: metadata.len(),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    (files, bytes, stopped.if_any())
}

/// Why [`walk`] cannot say it saw the whole of a spool — with the path and
/// the error that say so.
///
/// Both halves are what the refusal was missing. The first consumer read 50
/// of those refusals and could act on none of them, because the two things
/// the walk knew at that moment — *which* path, and *what* the kernel said
/// about it — were thrown away one line before the sentence was written.
#[derive(Debug, Default)]
struct NotWhole {
    /// The sweep's entry bound ran out inside this spool, so the rest of it
    /// was never looked at. Nothing here is a fault of the disk, and the next
    /// start looks again.
    bound: bool,
    /// The first path under the spool that would not read, if one would not.
    unreadable: Option<Unreadable>,
}

/// One path the walk could not read, kept for the refusal to name.
#[derive(Debug)]
struct Unreadable {
    /// The path itself. The one field that is not prose.
    path: PathBuf,
    /// The error the kernel gave, verbatim — `Permission denied (os error
    /// 13)`, and not one word of this project's own about it. The errno *is*
    /// the diagnosis: `EACCES` is a mode or an owner somebody can change,
    /// `EIO` is a disk, `ENOENT` is a tree that moved under the walk, and a
    /// sentence that summarised them into "could not be read" is how all
    /// three became one unanswerable report.
    why: String,
    /// How many further paths under the same spool would not read either.
    /// Counted and not kept: a spool whose every directory is shut is worth
    /// one line and a number, not fifty lines.
    and_more: usize,
}

impl NotWhole {
    /// Keep one unreadable path and count the rest.
    ///
    /// The one kept is the first in **path order**, not the first the
    /// filesystem happened to hand over: `readdir` order is not stable
    /// between two starts over the same disk, and a refusal that named a
    /// different file at each start is one nobody can compare with the last
    /// one. The same reason [`pick_up`] sorts the names it sweeps.
    fn could_not_read(&mut self, path: PathBuf, error: &std::io::Error) {
        let why = error.to_string();
        match &mut self.unreadable {
            None => {
                self.unreadable = Some(Unreadable {
                    path,
                    why,
                    and_more: 0,
                })
            }
            Some(first) => {
                first.and_more += 1;
                if path < first.path {
                    first.path = path;
                    first.why = why;
                }
            }
        }
    }

    /// [`None`] when the walk did get through after all.
    fn if_any(self) -> Option<Self> {
        (self.bound || self.unreadable.is_some()).then_some(self)
    }

    /// What to tell the person on that host: the path, the error in the
    /// kernel's own words, and one sentence on what to do about it.
    ///
    /// [`crate::janitor`]'s shape, for [`crate::janitor`]'s audience — the
    /// owner of a machine this project never logs into, who needs a path, a
    /// figure and an action, and for whom a line with none of the three is
    /// the one line nobody can act on.
    fn sentence(&self) -> String {
        let mut said = Vec::new();
        if let Some(first) = &self.unreadable {
            let and_more = if first.and_more > 0 {
                format!(
                    ", and {} further path(s) under it would not read either",
                    first.and_more
                )
            } else {
                String::new()
            };
            said.push(format!(
                "The first path it could not read is {}: {}{and_more}. do: make that path \
                 readable to the user this runner runs as — a job that left its output \
                 owned by another user is what this usually is. This sweep reads and never \
                 repairs: it has changed no mode, no owner and no name here, and the next \
                 start offers the whole collection as soon as the path reads",
                first.path.display(),
                first.why
            ));
        }
        if self.bound {
            said.push(format!(
                "The sweep's bound of {MOST_ENTRIES_SWEPT} entries ran out inside it, so the \
                 rest of it was never looked at and no path in it is at fault. do: nothing — \
                 the next start looks again, and every collection the hub acknowledges \
                 shortens the walk"
            ));
        }
        said.join(" ")
    }
}

/// Move a spool out of the way under a name that says why, keeping every
/// byte, and say where they went.
///
/// A rename, so it is constant time whatever the collection weighs, and the
/// swept name no longer parses as `{job}-{attempt}` — which is what stops the
/// next start picking it up again and offering what nobody will take.
pub(crate) fn set_aside(path: &Path, suffix: &str) -> String {
    let mut aside = path.as_os_str().to_os_string();
    aside.push(".");
    aside.push(suffix);
    let aside = PathBuf::from(aside);
    match fs::rename(path, &aside) {
        Ok(()) => format!("Its bytes are kept as {}", aside.display()),
        Err(error) => format!(
            "Its bytes could NOT even be moved aside ({error}), so they are still in {}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    /// A data root with a spool in it, as a runner that streamed a collection
    /// and was then restarted leaves one.
    fn a_root(name: &str) -> (TempDir, DataRoot) {
        let temp = TempDir::new(name);
        let root = DataRoot::open(temp.path()).expect("a throwaway root");
        (temp, root)
    }

    fn a_spool(root: &DataRoot, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let path = root.outputs_dir().join(name);
        for (relative, bytes) in files {
            let file = path.join(relative);
            fs::create_dir_all(file.parent().expect("a parent")).expect("the directories");
            fs::write(&file, bytes).expect("the file");
        }
        fs::create_dir_all(&path).expect("the spool directory");
        path
    }

    /// The whole point: a spool left by a runner that was restarted is found,
    /// and found with the two figures the resumption needs.
    #[test]
    fn a_spool_a_previous_runner_left_is_picked_up_with_its_files() {
        let (_temp, root) = a_root("sweep-picked-up");
        a_spool(
            &root,
            "j0000000007-2",
            &[("results.json", b"{\"ok\":true}"), ("logs/run.txt", b"hi")],
        );
        let (swept, said) = pick_up(&root);
        assert_eq!(swept.len(), 1, "{said:?}");
        assert_eq!(swept[0].job_id(), &JobId::new("j0000000007"));
        assert_eq!(swept[0].attempt(), 2);
        assert_eq!(
            swept[0].files(),
            &[
                SpooledFile {
                    path: "logs/run.txt".to_owned(),
                    len: 2
                },
                SpooledFile {
                    path: "results.json".to_owned(),
                    len: 11
                },
            ],
            "the paths are the ones the manifest announced, in one order"
        );
        assert_eq!(swept[0].bytes(), 13);
        assert!(
            swept[0].offered().contains("j0000000007"),
            "picking a collection up is not something to do quietly: {:?}",
            swept[0].offered()
        );
        // And nothing was deleted by the looking.
        assert!(swept[0].path().exists());
    }

    /// A name that is not `{job}-{attempt}`, an entry that is a file, and one
    /// that is a symlink: each is named, none is touched, and **none of them
    /// stops the sweep** — the spool beside them is still found.
    #[test]
    fn a_bad_entry_is_named_and_left_and_the_sweep_carries_on() {
        let (_temp, root) = a_root("sweep-bad-entries");
        let outputs = root.outputs_dir();
        fs::create_dir_all(&outputs).expect("the outputs directory");
        fs::create_dir_all(outputs.join("not-a-spool")).expect("a directory with no attempt");
        fs::create_dir_all(outputs.join("j1-notanumber")).expect("an attempt that is not one");
        fs::write(outputs.join("j2-1"), b"a file where a spool should be").expect("a file");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", outputs.join("j3-1")).expect("a symlink");
        a_spool(&root, "j0000000009-1", &[("results.json", b"mine")]);

        let (swept, said) = pick_up(&root);
        assert_eq!(
            swept.len(),
            1,
            "one bad entry must never hide the spool beside it: {said:?}"
        );
        assert_eq!(swept[0].job_id(), &JobId::new("j0000000009"));
        let said = said.join("\n");
        for named in ["not-a-spool", "j1-notanumber", "j2-1"] {
            assert!(said.contains(named), "{named} is not named: {said}");
        }
        assert!(said.contains("left exactly as it is"), "{said}");
        // Named, and still there: nothing here deletes.
        assert!(outputs.join("not-a-spool").exists());
        assert!(outputs.join("j2-1").exists());
        #[cfg(unix)]
        {
            assert!(said.contains("never followed"), "{said}");
            assert!(outputs.join("j3-1").symlink_metadata().is_ok());
        }
    }

    /// A spool with no regular file in it is not a collection to offer, and
    /// it is still not this sweep's to remove.
    #[test]
    fn a_spool_holding_nothing_is_named_rather_than_offered() {
        let (_temp, root) = a_root("sweep-empty");
        let path = root.outputs_dir().join("j0000000011-1");
        fs::create_dir_all(path.join("empty")).expect("an empty spool");
        let (swept, said) = pick_up(&root);
        assert!(swept.is_empty(), "{said:?}");
        let said = said.join("\n");
        assert!(said.contains("no regular file"), "{said}");
        assert!(path.exists(), "nothing here deletes: {said}");
    }

    /// A directory under a spool that will not read: what the walk can see is
    /// **not** the collection, and a part of a manifest offered as the whole
    /// of one would read to the hub as a collection it already holds.
    ///
    /// And — the first consumer's 50 refusals, which named neither — the
    /// refusal names **that path** and **that errno**. Built with a mode the
    /// test removes itself, because an output tree owned by another user is
    /// the consumer's own hypothesis for what their 50 are.
    #[test]
    #[cfg(unix)]
    fn a_spool_that_will_not_read_through_is_not_offered_and_says_which_path() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, root) = a_root("sweep-unreadable");
        let path = a_spool(
            &root,
            "j0000000013-1",
            &[("open.txt", b"here"), ("shut/inner.txt", b"not here")],
        );
        fs::set_permissions(path.join("shut"), fs::Permissions::from_mode(0o000))
            .expect("the fixture shuts it");
        let (swept, said) = pick_up(&root);
        let put_back = fs::set_permissions(path.join("shut"), fs::Permissions::from_mode(0o700));
        if crate::privilege::effective_uid() == Some(0) {
            crate::testing::skipped(
                "a_spool_that_will_not_read_through_is_not_offered_and_says_which_path",
                "this test runs as root, and root reads a directory moded 0o000: a \
                 capable host would have proved that a spool the walk cannot finish is \
                 never offered as a whole collection, and that the refusal names the \
                 path and the errno. The sentence itself is still proved without a \
                 mode by a_refusal_names_the_errno_in_the_kernels_own_words",
            );
            return;
        }
        put_back.expect("the fixture puts it back");
        assert!(swept.is_empty(), "{said:?}");
        let said = said.join("\n");
        assert!(said.contains("not the whole collection"), "{said}");
        // The two figures the operator could not get out of v0.4.1: which
        // path, and what the kernel said about it.
        assert!(
            said.contains(&path.join("shut").display().to_string()),
            "the refusal must name the path it could not read: {said}"
        );
        assert!(
            said.contains("Permission denied"),
            "the refusal must carry the errno's own words, which are what tell a mode \
             apart from a failing disk: {said}"
        );
        assert!(
            said.contains("do: make that path readable"),
            "a line with no action in it is one nobody acts on: {said}"
        );
        // One path named, no second one invented.
        assert!(
            !said.contains("further path(s)"),
            "there was only one unreadable path here: {said}"
        );
        // The refusal still refuses, and the bytes are still exactly as they
        // were: this walk reads, it never repairs.
        assert!(path.exists(), "nothing here deletes: {said}");
        assert_eq!(
            fs::read(path.join("open.txt")).expect("the readable file is untouched"),
            b"here"
        );
        assert_eq!(
            fs::symlink_metadata(path.join("shut"))
                .expect("the shut directory is still there")
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "only this test's own `put_back` may have changed that mode; the sweep \
             chmods nothing"
        );
    }

    /// Several unreadable paths under one spool: the **first in path order**
    /// is named, the rest are counted, and the spool is one line and not
    /// fifty. A startup report is not a place to print a tree.
    #[test]
    #[cfg(unix)]
    fn several_unreadable_paths_name_the_first_and_count_the_rest() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, root) = a_root("sweep-unreadable-many");
        let path = a_spool(
            &root,
            "j0000000014-1",
            &[
                ("a-shut/inner.txt", b"no"),
                ("b-shut/inner.txt", b"no"),
                ("c-shut/inner.txt", b"no"),
                ("open.txt", b"yes"),
            ],
        );
        let shut = ["a-shut", "b-shut", "c-shut"];
        for name in shut {
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o000))
                .expect("the fixture shuts it");
        }
        let (swept, said) = pick_up(&root);
        let put_back: Vec<_> = shut
            .iter()
            .map(|name| fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o700)))
            .collect();
        if crate::privilege::effective_uid() == Some(0) {
            crate::testing::skipped(
                "several_unreadable_paths_name_the_first_and_count_the_rest",
                "this test runs as root, and root reads a directory moded 0o000: a \
                 capable host would have proved that one spool with three unreadable \
                 directories is one line naming the first and counting two more",
            );
            return;
        }
        for outcome in put_back {
            outcome.expect("the fixture puts it back");
        }
        assert!(swept.is_empty(), "{said:?}");
        // One line for this spool, whatever is wrong inside it.
        let about: Vec<&String> = said
            .iter()
            .filter(|line| line.contains("j0000000014-1"))
            .collect();
        assert_eq!(about.len(), 1, "one spool is one line: {said:?}");
        let said = about[0];
        assert!(
            said.contains(&path.join("a-shut").display().to_string()),
            "the first in path order is the one named: {said}"
        );
        assert!(
            said.contains("and 2 further path(s)"),
            "the others are counted, so the line says the problem is bigger than the \
             path it names: {said}"
        );
        assert!(
            !said.contains("c-shut"),
            "counted, not listed — fifty lines for one spool is how a startup report \
             stops being read: {said}"
        );
    }

    /// The sentence itself, without a mode: an errno this host can produce at
    /// any uid, including root. This is what keeps the claim "the refusal
    /// carries the path and the error" proved on a host where the two tests
    /// above can only skip.
    #[test]
    fn a_refusal_names_the_errno_in_the_kernels_own_words() {
        let temp = TempDir::new("sweep-errno-words");
        let missing = temp.path().join("a-path-that-is-not-there");
        let error = fs::read_dir(&missing).expect_err("nothing is there to read");
        let mut stopped = NotWhole::default();
        stopped.could_not_read(missing.clone(), &error);
        let said = stopped.sentence();
        assert!(said.contains(&missing.display().to_string()), "{said}");
        assert!(
            said.contains("No such file or directory") && said.contains("os error 2"),
            "the kernel's own words and its number, not a summary of them: {said}"
        );
        assert!(said.contains("do: "), "{said}");

        // A second one does not replace the first unless it sorts before it,
        // because a refusal an owner cannot compare with yesterday's is one
        // they cannot follow.
        let later = temp.path().join("z-later");
        stopped.could_not_read(later, &error);
        let earlier = temp.path().join("A-earlier");
        stopped.could_not_read(earlier.clone(), &error);
        let said = stopped.sentence();
        assert!(said.contains(&earlier.display().to_string()), "{said}");
        assert!(said.contains("and 2 further path(s)"), "{said}");
    }

    /// The other half of the same change, and the one that must not move: a
    /// spool that reads through is still offered, with no reason attached to
    /// anything. Adding a reason to a refusal must not change **who** is
    /// refused.
    #[test]
    fn a_spool_that_reads_through_is_still_offered_with_nothing_said() {
        let (_temp, root) = a_root("sweep-still-offered");
        let path = a_spool(
            &root,
            "j0000000015-3",
            &[("results.json", b"{}"), ("packs/one.pack", b"bytes")],
        );
        let mut budget = MOST_ENTRIES_SWEPT;
        let (files, bytes, not_whole) = walk(&path, &mut budget);
        assert!(
            not_whole.is_none(),
            "a spool nothing is wrong with has no reason to give: {not_whole:?}"
        );
        assert_eq!(files.len(), 2, "{files:?}");
        assert_eq!(bytes, 7);
        let (swept, said) = pick_up(&root);
        assert_eq!(swept.len(), 1, "{said:?}");
        assert!(said.is_empty(), "nothing to complain about: {said:?}");
        let offered = swept[0].offered();
        assert!(
            offered.contains("it is offered to the hub again"),
            "{offered}"
        );
    }

    /// A spool already set aside is never picked up again: that is what makes
    /// the terminal state terminal, rather than a rename a later start undoes.
    #[test]
    fn a_spool_already_set_aside_is_counted_and_never_offered_again() {
        let (_temp, root) = a_root("sweep-set-aside");
        let path = a_spool(&root, "j0000000017-1", &[("results.json", b"mine")]);
        let said = set_aside(&path, UNCLAIMED);
        assert!(said.contains("kept as"), "{said}");
        assert!(!path.exists());

        let (swept, said) = pick_up(&root);
        assert!(swept.is_empty(), "{said:?}");
        let said = said.join("\n");
        assert!(said.contains("1 spool(s)"), "{said}");
        assert!(said.contains("removing them is yours"), "{said}");
        // And the bytes are still there, which is the whole difference
        // between setting aside and deleting.
        assert_eq!(
            fs::read(
                root.outputs_dir()
                    .join(format!("j0000000017-1.{UNCLAIMED}"))
                    .join("results.json")
            )
            .expect("the bytes are kept"),
            b"mine"
        );
    }

    /// No `outputs/` at all — a runner that has never streamed a collection —
    /// is silence, not a complaint.
    #[test]
    fn a_data_root_with_no_outputs_directory_says_nothing() {
        let (_temp, root) = a_root("sweep-none");
        let (swept, said) = pick_up(&root);
        assert!(swept.is_empty());
        assert!(said.is_empty(), "{said:?}");
    }

    /// The bound: a sweep is a fixed number of syscalls, and what it did not
    /// reach is left alone rather than half-offered.
    #[test]
    fn the_sweep_stops_at_its_bound_and_says_so() {
        let (_temp, root) = a_root("sweep-bounded");
        let mut budget = 2usize;
        let path = a_spool(
            &root,
            "j0000000019-1",
            &[("a.txt", b"a"), ("b.txt", b"b"), ("c.txt", b"c")],
        );
        let (files, _, not_whole) = walk(&path, &mut budget);
        let not_whole = not_whole.expect("a walk that ran out of budget is not a whole one");
        assert!(not_whole.bound);
        assert!(
            not_whole.unreadable.is_none(),
            "the bound is not a fault of any path, and must not accuse one: {not_whole:?}"
        );
        assert!(files.len() < 3, "{files:?}");
        assert_eq!(budget, 0);
        // And the spool is not offered on the strength of what it saw — with
        // the bound named as the reason, so that an owner does not go looking
        // for a broken disk that is not there.
        let mut budget = 2usize;
        let Found::Said(said) = one(&path, &mut budget) else {
            panic!("a spool the walk did not finish must never be offered");
        };
        assert!(said.contains("whole"), "{said}");
        assert!(
            said.contains(&MOST_ENTRIES_SWEPT.to_string()) && said.contains("no path in it"),
            "the bound must say it was the bound: {said}"
        );
    }

    /// `{job}-{attempt}` splits at the last hyphen, because the job id is the
    /// hub's to shape and this runner only ever appended the attempt.
    #[test]
    fn a_name_is_split_at_the_last_hyphen_and_the_attempt_is_a_number() {
        assert_eq!(
            parse_name("j0000000007-2"),
            Some((JobId::new("j0000000007"), 2))
        );
        assert_eq!(
            parse_name("a-hub-that-hyphenates-11"),
            Some((JobId::new("a-hub-that-hyphenates"), 11))
        );
        assert_eq!(parse_name("j7"), None);
        assert_eq!(parse_name("j7-"), None);
        assert_eq!(parse_name("-1"), None);
        assert_eq!(parse_name("j7-results"), None);
        assert_eq!(
            parse_name("j7-4294967296"),
            None,
            "an attempt is a u32, and a number this runner never wrote is not one"
        );
        // Splitting at the *last* hyphen is what makes this total: there is
        // no name `spool_path` can produce that this reads as another job.
        assert_eq!(parse_name("j7--1"), Some((JobId::new("j7-"), 1)));
    }

    /// **The startup reaper and this sweep, composed, in the order
    /// `run_until` composes them.** The regression behind the first
    /// consumer's blocked upgrade (ORCH-164).
    ///
    /// Both halves were correct alone and both had a green test of their own:
    /// `reclaim::tests` asserted the reaper removed a spool, and
    /// `a_spool_a_previous_runner_left_is_picked_up_with_its_files` above
    /// asserts it is picked up. Neither ran the startup sequence, so nothing
    /// noticed that they walked the same directory and that the reaper went
    /// first — `remove_dir_all` on every entry under `outputs/` with no guard
    /// at all, twenty-eight lines before `pick_up_collections` read the same
    /// directory and found it empty. v0.4.0 shipped the sweep dead on
    /// arrival.
    ///
    /// So this test calls the runner's **own** two startup entry points and
    /// re-implements neither: `Confined::prepare` (`lib.rs:2634`), which is
    /// what calls `reclaim_a_past_life`, and `Session::pick_up_collections`
    /// (`lib.rs:2662`), which is this sweep. A test that stood in for either
    /// would reproduce nothing, which is exactly how the defect got out.
    #[test]
    fn the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("sweep-vs-reaper");
        // A spool on the disk before anything starts, as a runner that
        // streamed a collection and was then `SIGKILL`ed leaves one.
        let spool = temp
            .path()
            .join(crate::outputs::SPOOL_DIR)
            .join("j0000001561-2");
        let file = spool.join("packs/e49deee1/objects/one.pack");
        fs::create_dir_all(file.parent().expect("a parent")).expect("the spool directories");
        fs::write(&file, b"the bytes a requester is owed").expect("the spooled file");

        // Startup, step one: the confinement, which reaps a past life.
        let confined = crate::confine::Confined::prepare(
            crate::confine::Confinement::with_data_root(temp.path()),
        )
        .expect("a throwaway data root confines");
        // What the reaper said, read off the startup report the runner really
        // prints rather than off a return value only a test looks at.
        let reaped: Vec<String> = confined
            .report()
            .into_iter()
            .filter(|line| line.starts_with("reaped at startup:"))
            .collect();

        assert!(
            spool.exists(),
            "the reaper removed {}, so the sweep that exists to re-adopt it reads an \
             empty directory. The reaper said: {reaped:?}",
            spool.display()
        );
        assert_eq!(
            fs::read(&file).expect("the spooled bytes survive the reaper"),
            b"the bytes a requester is owed",
            "a spool is the one copy of a requester's evidence until the hub \
             acknowledges it (D19)"
        );
        // And it does not *claim* to have reclaimed it either: the sentence
        // that explained the deletion was itself false — "no runner can offer
        // a spool it did not make" — and the sweep below is the runner that
        // does. Asserted on the spool rather than on the report being empty,
        // because this host's own cgroups and stray processes are a shared
        // machine's business and no part of this claim.
        assert!(
            !reaped
                .iter()
                .any(|line| line.contains("j0000001561") || line.contains("of output left in")),
            "the reaper has no business under outputs/ and nothing to say about it: \
             {reaped:?}"
        );

        // Startup, step two: the sweep, on the same data root the confinement
        // opened — and then the offer, so that what survived is shown reaching
        // the hub's own resumption path and not merely sitting on a disk.
        confined
            .root()
            .remember_hub_state_root("s-the-one-that-made-these-ids")
            .expect("the data root records a state root");
        let session = crate::Session::new();
        let said = session.pick_up_collections(confined.root()).join("\n");
        assert!(said.is_empty(), "nothing to complain about: {said}");
        let offered = session
            .settle_against_state_root(confined.root(), "s-the-one-that-made-these-ids")
            .join("\n");
        assert!(
            offered.contains("picked up the collection") && offered.contains("j0000001561"),
            "a collection the hub is still owed must survive the startup that is \
             supposed to offer it again: {offered}"
        );
    }

    /// A spool as the sweep hands one to the lines below, with the figures
    /// chosen rather than walked.
    fn a_swept(path: PathBuf, bytes: u64) -> Swept {
        Swept {
            path,
            job_id: JobId::new("j0000000007"),
            attempt: 2,
            files: vec![
                SpooledFile {
                    path: "files/results.json".to_owned(),
                    len: 535_690_000,
                },
                SpooledFile {
                    path: "files/log.txt".to_owned(),
                    len: 1_000,
                },
            ],
            bytes,
        }
    }

    /// **The line the first consumer read, pinned as text.**
    ///
    /// 535691000 is their own figure: one offer line in seventy-five said
    /// `535691 kB` where the other seventy-four said bytes, because that one
    /// total happens to be an exact multiple of 1000 and of nothing larger.
    /// The number was right and the unit moved, which is worse than either.
    ///
    /// The expectation is spelled out and is **not** a round trip through the
    /// formatter. A round trip would stay green through the one change that
    /// matters here — the writer and the reader moving together — which is
    /// precisely what breaks somebody parsing this line. **If this goes red,
    /// do not paste the new line over the expectation:** that is the repair
    /// that is always wrong, because the expectation is the bug report.
    #[test]
    fn the_offer_line_says_a_measured_weight_in_bytes_not_in_a_round_kb() {
        let swept = a_swept(
            PathBuf::from("/var/lib/herd/outputs/j0000000007-2"),
            535_691_000,
        );
        assert_eq!(
            swept.offered(),
            "picked up the collection of j0000000007 (attempt 2, 535691000 bytes \
             (510.87 MiB) in 2 file(s)) from /var/lib/herd/outputs/j0000000007-2: it was \
             never acknowledged, and it is offered to the hub again",
            "nobody typed this weight, so it is said exactly. Do not paste new output over \
             this expectation."
        );
    }

    /// The whole class, not the one case: every figure here divides exactly
    /// by 1000, which is what used to pull the line into `kB`.
    #[test]
    fn no_measured_weight_changes_unit_because_it_divides_by_a_thousand() {
        for (bytes, said) in [
            (1_000u64, "1000 bytes"),
            (2_000, "2000 bytes (1.95 KiB)"),
            (1_000_000, "1000000 bytes (976.56 KiB)"),
            (535_691_000, "535691000 bytes (510.87 MiB)"),
            (1_000_000_000, "1000000000 bytes (953.67 MiB)"),
        ] {
            let offered =
                a_swept(PathBuf::from("/var/lib/herd/outputs/j0000000007-2"), bytes).offered();
            assert!(
                offered.contains(&format!("attempt 2, {said} in 2 file(s)")),
                "a measured weight is exact whatever it divides by: {offered}"
            );
            assert!(
                !offered.contains(&format!("{} kB", bytes / 1_000)),
                "and never takes the decimal unit it happens to divide by: {offered}"
            );
        }
    }

    /// The other line that carries a measured weight: bytes produced for a
    /// hub state root that is gone. Pinned as text for the same reason, the
    /// one interpolation being the throwaway directory this test made.
    #[test]
    fn the_foreign_state_root_line_says_a_measured_weight_in_bytes() {
        let (_temp, root) = a_root("sweep-foreign-measured");
        let path = a_spool(&root, "j0000000007-2", &[("files/log.txt", b"a few bytes")]);
        let swept = a_swept(path.clone(), 535_691_000);
        assert_eq!(
            swept.for_a_foreign_state_root("its state root is 0f0f"),
            format!(
                "the collection of j0000000007 (attempt 2, 535691000 bytes (510.87 MiB) in \
                 2 file(s)) was produced for another hub's state root (its state root is \
                 0f0f); no hub can record it now, so it is NOT offered. Its bytes are kept \
                 as {}.foreign-state-root",
                path.display()
            ),
            "nobody typed this weight either. Do not paste new output over this \
             expectation."
        );
    }
}
