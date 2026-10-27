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

use std::fs;
use std::path::{Path, PathBuf};

use herd_proto::JobId;

use crate::root::DataRoot;
use crate::size::show_bytes;

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
            show_bytes(self.bytes),
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
            show_bytes(self.bytes),
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
    let (files, bytes, whole) = walk(path, budget);
    if !whole {
        return Found::Said(format!(
            "{} could not be read all the way through, so what this runner can see of it is \
             not the whole collection; it is NOT offered, because a part of a manifest \
             offered as the whole of one reads to the hub as a collection it already holds. \
             It is left exactly as it is",
            path.display()
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
/// Returns whether the walk finished. It does not if the budget runs out or a
/// directory will not read, and then nothing of this spool may be trusted as
/// a whole collection.
fn walk(spool: &Path, budget: &mut usize) -> (Vec<SpooledFile>, u64, bool) {
    let mut files: Vec<SpooledFile> = Vec::new();
    let mut bytes = 0u64;
    let mut whole = true;
    let mut pending = vec![(spool.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                whole = false;
                continue;
            }
        };
        for entry in entries {
            if *budget == 0 {
                return (files, bytes, false);
            }
            *budget -= 1;
            let Ok(entry) = entry else {
                whole = false;
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                whole = false;
                continue;
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
    (files, bytes, whole)
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
    #[test]
    #[cfg(unix)]
    fn a_spool_that_will_not_read_through_is_not_offered() {
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
                "a_spool_that_will_not_read_through_is_not_offered",
                "this test runs as root, and root reads a directory moded 0o000: a \
                 capable host would have proved that a spool the walk cannot finish is \
                 never offered as a whole collection",
            );
            return;
        }
        put_back.expect("the fixture puts it back");
        assert!(swept.is_empty(), "{said:?}");
        let said = said.join("\n");
        assert!(said.contains("not the whole collection"), "{said}");
        assert!(path.exists(), "nothing here deletes: {said}");
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
        let (files, _, whole) = walk(&path, &mut budget);
        assert!(!whole, "a walk that ran out of budget is not a whole one");
        assert!(files.len() < 3, "{files:?}");
        assert_eq!(budget, 0);
        // And the spool is not offered on the strength of what it saw.
        let mut budget = 2usize;
        assert!(
            matches!(one(&path, &mut budget), Found::Said(said) if said.contains("whole")),
            "a spool the walk did not finish must never be offered"
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
}
