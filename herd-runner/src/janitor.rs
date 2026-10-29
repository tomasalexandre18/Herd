//! The notebook: what a job left on this host that the runner could not take
//! away.
//!
//! Herd reclaims a job's processes, its cgroup and its workspace, and says
//! what it reclaimed ([`crate::reclaim`]). What it had no way of saying is
//! what it tried to reclaim and **could not** — and that is the only half a
//! person has to act on, because it is the half that is still on their disk.
//! A runner that writes nothing there is asking the machine's owner to clean
//! up what they cannot see.
//!
//! So one file, `{data root}/left-behind`, holds one entry per thing the
//! runner tried to remove and was refused. **Nobody in this project logs into
//! the consumer's machines** (invariant 9, D161), so the entries are not
//! telemetry for us: each one is an instruction for a human on that host,
//! naming the path, naming what it weighs, and saying what to do about it. A
//! line with no figure in it is the one report nobody can act on.
//!
//! This is the second half of **D134**, which chose "publish what the reaper
//! found where a janitor can read it" over "run the consumer's `down` from
//! the startup path". Herd names the mess; the consumer owns the cleanup.
//!
//! ## Why a file and not a log line
//!
//! Both halves of the reason are about a restart. The reaper runs because the
//! runner was `SIGKILL`ed, so whatever it says is said *after* the journal
//! that would have explained it is already behind the owner; and a workspace
//! that will not go away does not go away at the next start either, so a line
//! printed once a start is a line printed for ever. A file is read when
//! somebody goes looking, survives the process that wrote it, and — this is
//! the part a log cannot do — **shrinks**: an entry whose thing is no longer
//! on the disk is taken off the list at the next start ([`reconcile`]), so
//! the list stays the length of the problem rather than the length of the
//! runner's history. A list that only grows is a list nobody reads.
//!
//! ## What is never in it
//!
//! **Nothing the runner could have reclaimed itself.** An entry is written at
//! exactly the three places a reclaim has already failed, and
//! [`write_down`] asks the disk once more before it writes: a path that is no
//! longer there is not written down at all. If Herd can clean it, Herd cleans
//! it and says nothing here.
//!
//! **Nothing under `outputs/` or `held-results/`, ever.** A spool there is
//! the only copy of a requester's evidence until the hub has acknowledged it
//! (D19, D211), and a held result is the only copy of a verdict (D34). Those
//! are not garbage, they are the thing Herd exists to protect, and they have
//! owners of their own: [`crate::sweep`] offers a spool to the hub again, and
//! [`crate::spool`] delivers a verdict at the next start. So a caller that
//! asks for such a path is **refused** here rather than trusted, and this
//! module contains no removal, rename or chmod of anything at all — it writes
//! its own file and reads the disk, and that is the whole of what it can do.
//! A test over this source asserts both.
//!
//! ## Bounded, because this runs on two paths that may not stall
//!
//! It is called when a job ends and at startup, so neither the walk nor the
//! file may grow with what a job once wrote: the measurement stops after
//! [`MOST_ENTRIES_WEIGHED`] entries and says that it did, the list stops at
//! [`MOST_ENTRIES_LISTED`] entries and says that it did, and
//! [`reconcile`] does one `lstat` per entry and no walk at all.

use std::fs;
use std::path::{Path, PathBuf};

use herd_proto::JobId;

use crate::root::DataRoot;
use crate::size::show_bytes;

/// The notebook's name, under the data root and outside `jobs/`: the startup
/// reaper clears everything under `jobs/`, so a record of what the reaper
/// could not do may not live where the reaper can erase it (D134).
pub const FILE: &str = "left-behind";

/// How many things the list holds before it stops taking new ones.
///
/// A bound rather than a rotation: the oldest entry is the one that has been
/// on the disk longest and is the least safe thing to stop naming. So the
/// list keeps what it has and says how many it could not add.
const MOST_ENTRIES_LISTED: usize = 1_000;

/// How many directory entries one measurement looks at.
///
/// The same shape of bound as [`crate::sweep::MOST_ENTRIES_SWEPT`] and for
/// the same reason: the cost of ending a job is a fixed number of syscalls
/// and never a function of what the job wrote. A walk that stops early
/// reports its figure as a floor, never as the total.
const MOST_ENTRIES_WEIGHED: usize = 50_000;

/// One thing the runner tried to take away and was refused.
#[derive(Debug, Clone)]
pub struct Leftover {
    /// The thing itself, on this host. The one field that is not prose.
    path: PathBuf,
    /// Whose it was, in words: a job and its attempt, or a runner that is
    /// gone.
    left_by: String,
    /// The error the runner got, verbatim. A reclaim fails for reasons a
    /// person needs to read — a mode nobody can change, a mount that is read
    /// only, a file where a directory should be — and a summary of it is a
    /// summary of the one sentence that says what to try.
    why: String,
}

impl Leftover {
    /// A workspace of a job this runner ran and could not clear afterwards.
    pub fn workspace_of(path: &Path, job_id: &JobId, attempt: u32, why: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            left_by: format!("job {job_id}, attempt {attempt}"),
            why: why.to_owned(),
        }
    }

    /// A workspace left by a runner that was killed, which this runner's
    /// startup reaper found and could not clear either.
    pub fn workspace_of_a_past_life(path: &Path, why: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            left_by: "a runner that is gone; found by this runner's startup reaper".to_owned(),
            why: why.to_owned(),
        }
    }
}

/// Write one thing down, or say on stderr why it could not be written.
///
/// **Never on the critical path.** This returns nothing and fails at nothing:
/// a notebook that cannot be written must not stop a job from running or a
/// verdict from reaching the hub. A janitor's notebook is worth a line on
/// stderr and not one byte of a job's outcome, so every error here ends in
/// [`crate::journal`]'s descriptor and nowhere else.
///
/// Three things are refused before anything is written, and each of them is
/// the module doc's promise made mechanical:
///
/// 1. a path that is not under this data root — the runner owns nothing above
///    it and names nothing it does not own;
/// 2. a path under `outputs/` or `held-results/`, which hold the only copy of
///    a result and of a verdict and are never garbage (D19, D34, D211);
/// 3. a path that is no longer on the disk — which is how "nothing the runner
///    could have reclaimed itself" is enforced here rather than trusted of
///    the caller.
///
/// A path the list already names is **left exactly as it was**, timestamp and
/// figure included. An entry is about a thing and not about an event: the
/// startup reaper meets the same unremovable workspace at every start, and a
/// list that rewrote the entry each time would say "first seen" of the last
/// boot and would re-walk the tree to say it. The earliest sighting, with
/// what it weighed then, is both the truer record and the cheaper one.
pub fn write_down(root: &DataRoot, left: &Leftover) {
    if let Err(refusal) = acceptable(root, &left.path) {
        crate::journal(&format!(
            "herd-runner: NOT writing {} down as left behind: {refusal}",
            left.path.display()
        ));
        return;
    }
    // Asked of the disk, not of the caller. A reclaim that failed and then
    // succeeded — a process that exited and released the last open file, a
    // mount that came back — must leave no entry at all.
    if fs::symlink_metadata(&left.path)
        .err()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return;
    }
    let path = root.left_behind_path();
    let found = read_blocks(&path);
    let was = found.len();
    let mut kept = still_here(found);
    let shrank = kept.len() != was;
    if kept.iter().any(|block| block.path == left.path) {
        // Already named. The only thing that may have changed is that
        // something else on the list has been cleared, and that is worth the
        // one write it costs.
        if shrank {
            rewrite(&path, &kept);
        }
        return;
    }
    if kept.len() >= MOST_ENTRIES_LISTED {
        crate::journal(&format!(
            "herd-runner: {} already names {MOST_ENTRIES_LISTED} things left on this host, \
             which is its bound, so {} is NOT in it. Clear what is listed and the next one \
             that will not go away is written down again",
            path.display(),
            left.path.display()
        ));
        if shrank {
            rewrite(&path, &kept);
        }
        return;
    }
    let weight = weigh(&left.path);
    let at = now();
    let mut text = String::new();
    text.push_str(&format!("left-behind: {}\n", left.path.display()));
    text.push_str(&format!("holds: {}, measured {at}\n", weight.sentence()));
    text.push_str(&format!("left-by: {}\n", left.left_by));
    text.push_str(&format!("first-seen: {at}\n"));
    text.push_str(&format!("why: {}\n", left.why.trim()));
    text.push_str(&advice(&weight));
    kept.push(Block {
        path: left.path.clone(),
        text,
    });
    rewrite(&path, &kept);
}

/// Bring the list up to date with the disk, and say what is still on it.
///
/// Called once at startup, beside the reaper whose failures put things on it.
/// Two jobs, both of them the reason the file is worth more than a log line:
/// an entry whose thing is no longer here is **taken off the list**, so the
/// list cannot grow for ever and become one more file nobody opens; and what
/// is left is named in the startup report, so an owner who never opens the
/// file is still told that it has something in it.
///
/// One `lstat` per entry and no walk: this is the startup path, and a runner
/// must start or refuse to start (invariant 2).
pub fn reconcile(root: &DataRoot) -> Vec<String> {
    let path = root.left_behind_path();
    let before = read_blocks(&path);
    if before.is_empty() {
        // No file, or a file with nothing in it. The ordinary case, and
        // silence is the right report for it: this runner has left nothing
        // behind that it could not take away.
        return Vec::new();
    }
    let kept = still_here(before.clone());
    let cleared = before.len() - kept.len();
    let mut said = Vec::new();
    if cleared != 0 {
        if let Err(error) = rewrite_reporting(&path, &kept) {
            said.push(format!(
                "COULD NOT take {cleared} finished entr(ies) off {}: {error}. What it names \
                 is still right about the disk, it is only longer than it needs to be",
                path.display()
            ));
        } else {
            said.push(format!(
                "{cleared} thing(s) named in {} are no longer on this host, so their \
                 entries were taken off the list",
                path.display()
            ));
        }
    }
    if kept.is_empty() {
        return said;
    }
    said.push(format!(
        "{} names {} thing(s) a job left on this host that a runner could NOT take away. \
         Nothing else will ever collect them: read that file and clear what it names",
        path.display(),
        kept.len()
    ));
    // A few by name, so the startup report is useful on its own, and the
    // count above for the rest: the file is the list, this is the pointer to
    // it, and a startup report is not a place to print a thousand paths.
    const NAMED_AT_STARTUP: usize = 5;
    for block in kept.iter().take(NAMED_AT_STARTUP) {
        said.push(format!("  still here: {}", block.first_two_lines()));
    }
    if kept.len() > NAMED_AT_STARTUP {
        said.push(format!(
            "  and {} more, every one of them in {}",
            kept.len() - NAMED_AT_STARTUP,
            path.display()
        ));
    }
    said
}

/// Why a path may not be named in the notebook, if it may not.
fn acceptable(root: &DataRoot, path: &Path) -> Result<(), String> {
    if !path.starts_with(root.path()) {
        return Err(format!(
            "it is not under the data root {}, and this runner names nothing it does not \
             own",
            root.path().display()
        ));
    }
    if path == root.path() {
        return Err(
            "it is the data root itself, which is the owner's and not a leftover".to_owned(),
        );
    }
    if path == root.left_behind_path() {
        return Err("it is this list itself".to_owned());
    }
    // The two directories that hold the only copy of something. A list that
    // called one of them garbage would be a mechanism for destroying exactly
    // what Herd holds them for, and the owner's hands are the ones that would
    // do it (D19, D34, D211). Asked of the data root rather than of a name
    // spelled a second time here: one of these directories moving while this
    // guard stays behind is how a promise like this goes quietly wrong.
    for (directory, what, owner) in [
        (
            root.outputs_dir(),
            "an unacknowledged collection, which is the only copy of a requester's \
             evidence until the hub holds every file of it",
            "crate::sweep::pick_up offers it to the hub again at every start",
        ),
        (
            root.held_results_dir(),
            "an unacknowledged verdict, which is the only copy of how a job ended",
            "crate::spool::pick_up delivers it at the next start",
        ),
    ] {
        if path.starts_with(&directory) {
            return Err(format!(
                "it is under {}, which holds {what}. It is not a leftover and it has an \
                 owner already: {owner}",
                directory.display()
            ));
        }
    }
    Ok(())
}

/// What a thing weighs, as a figure that is never a silent zero.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Weight {
    bytes: u64,
    files: u64,
    /// Paths the walk was refused or could not read, so the two figures above
    /// are a floor and the sentence says so.
    unread: u64,
    /// The walk stopped at [`MOST_ENTRIES_WEIGHED`], same consequence.
    truncated: bool,
}

impl Weight {
    /// The figure, in a clause that can be dropped into a sentence. Always a
    /// number: a floor when the walk was incomplete, and a plain `0 bytes in
    /// 0 file(s)` for a directory tree that really is empty, which is a figure
    /// a person can act on and not the absence of one.
    fn sentence(&self) -> String {
        let floor = self.unread != 0 || self.truncated;
        let mut said = format!(
            "{}{} in {} file(s)",
            if floor { "at least " } else { "" },
            show_bytes(self.bytes),
            self.files
        );
        if self.unread != 0 {
            said.push_str(&format!(
                ", with {} path(s) this runner could not read and so could not add up",
                self.unread
            ));
        }
        if self.truncated {
            said.push_str(&format!(
                ", and the walk stopped at its bound of {MOST_ENTRIES_WEIGHED} entries"
            ));
        }
        said
    }
}

/// Add up what is at a path, following no symlink and restoring no mode.
///
/// Apparent sizes, like every other figure the runner takes about a tree
/// ([`crate::root::measure_tree`]): what the job put there, not how the
/// filesystem stores it. Nothing is chmodded on the way — the reclaim that
/// just failed already restored every mode the runner owns and was refused
/// anyway, so a second attempt here would be the same refusal with more
/// syscalls, and a path that cannot be read is counted as unread rather than
/// as empty (D23).
fn weigh(path: &Path) -> Weight {
    let mut weight = Weight::default();
    let mut budget = MOST_ENTRIES_WEIGHED;
    let Ok(metadata) = fs::symlink_metadata(path) else {
        weight.unread += 1;
        return weight;
    };
    if !metadata.is_dir() {
        weight.bytes = metadata.len();
        weight.files = 1;
        return weight;
    }
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            weight.unread += 1;
            continue;
        };
        for entry in entries {
            if budget == 0 {
                weight.truncated = true;
                return weight;
            }
            budget -= 1;
            let Ok(entry) = entry else {
                weight.unread += 1;
                continue;
            };
            // `DirEntry::metadata` does not follow a symlink, so a link to
            // something huge outside this tree counts as the link it is.
            let Ok(metadata) = entry.metadata() else {
                weight.unread += 1;
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                weight.files += 1;
                weight.bytes = weight.bytes.saturating_add(metadata.len());
            }
        }
    }
    weight
}

/// What the person on this host is to do about it, with the figure in it.
///
/// Two fields and no more, because an instruction nobody finishes reading is
/// not an instruction. The first says what to do, the second says what it
/// costs not to. Wrapped, with the continuation indented, because this is
/// read in a terminal; a continuation line carries no field name and so is
/// never mistaken for the start of an entry ([`read_blocks`]).
fn advice(weight: &Weight) -> String {
    format!(
        "do: remove this path as the user that owns what is in it. This runner\n\
         \u{20}    put back every mode it owns and was refused anyway, so it will not\n\
         \u{20}    try again.\n\
         cost-of-not: {} stay on this disk, and no runner will ever take them\n\
         \u{20}    away.\n",
        show_bytes(weight.bytes)
    )
}

/// One entry of the list: the path it is about, and its text exactly as it
/// will be written back.
///
/// Kept verbatim on purpose. The figure in an entry is dated in the entry
/// itself, so a rewrite does not have to re-measure a tree it is only
/// re-listing — which is what keeps [`reconcile`] one `lstat` per entry on
/// the startup path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    path: PathBuf,
    text: String,
}

impl Block {
    /// The path and the figure, for the startup report.
    fn first_two_lines(&self) -> String {
        let mut lines = self.text.lines();
        let first = lines.next().unwrap_or_default();
        let second = lines.next().unwrap_or_default();
        format!("{first} — {second}")
    }
}

/// Every entry of the file, in the order it has them.
///
/// The only field parsed is `left-behind:`, which is the only one anything
/// but a person reads. Everything above the first one — the header, and
/// anything a person added — is dropped, because the header is regenerated
/// and the file says not to edit it.
fn read_blocks(path: &Path) -> Vec<Block> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut blocks: Vec<Block> = Vec::new();
    for line in text.lines() {
        if let Some(named) = line.strip_prefix("left-behind: ") {
            blocks.push(Block {
                path: PathBuf::from(named),
                text: format!("{line}\n"),
            });
        } else if let Some(block) = blocks.last_mut() {
            block.text.push_str(line);
            block.text.push('\n');
        }
    }
    for block in &mut blocks {
        block.text = format!("{}\n", block.text.trim_end());
    }
    blocks
}

/// Only the entries whose thing is still on this host.
///
/// This is the whole of "the list shrinks". `symlink_metadata` and not
/// `metadata`: a dangling symlink is still a thing on the disk, and following
/// one would ask about something that is not this runner's.
fn still_here(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .filter(|block| fs::symlink_metadata(&block.path).is_ok())
        .collect()
}

/// Write the whole file, saying on stderr if it could not be written.
fn rewrite(path: &Path, blocks: &[Block]) {
    if let Err(error) = rewrite_reporting(path, blocks) {
        crate::journal(&format!(
            "herd-runner: COULD NOT write {}: {error}. What a job left on this host is then \
             only in this journal, and this journal does not survive a restart — but no job \
             and no verdict depends on this file",
            path.display()
        ));
    }
}

/// The whole file at once, fsynced.
///
/// Whole and not appended, because the shrinking is the point: an entry comes
/// off the list by not being written again. Fsynced for the reason the held
/// verdicts are ([`crate::spool`]): the question this file answers is asked
/// after a crash, and a list that is only in a page cache is a list that the
/// crash takes with it.
fn rewrite_reporting(path: &Path, blocks: &[Block]) -> std::io::Result<()> {
    let mut text = header(blocks.len());
    for block in blocks {
        text.push('\n');
        text.push_str(&block.text);
    }
    fs::write(path, text.as_bytes())?;
    fs::File::open(path).and_then(|file| file.sync_all())
}

/// What a person reads first, which has to say what the file is for, that it
/// is complete, and that acting on it is what makes it shorter.
fn header(entries: usize) -> String {
    format!(
        "# What jobs left on this host that a Herd runner could NOT take away.\n\
         #\n\
         # Herd reclaims a job's processes, its cgroup and its workspace. Each entry\n\
         # below is one it tried to reclaim and was refused, so it is still on this\n\
         # disk right now and nothing in Herd will ever collect it. Herd names the\n\
         # mess; clearing it is yours (D134).\n\
         #\n\
         # Clear what an entry names and the entry goes away by itself, at this\n\
         # runner's next start. Nothing here is a thing Herd could have removed\n\
         # itself, and nothing here is ever an output spool or a held result: those\n\
         # are the only copy of a job's evidence, they are not garbage, and this\n\
         # runner refuses to name one.\n\
         #\n\
         # Written whole by herd-runner, newest entry last. Do not edit it; act on\n\
         # it. Entries now: {entries}.\n"
    )
}

/// This moment, in the one timestamp format the rest of Herd prints (the
/// hub's records and `herdctl`'s output are both RFC 3339).
///
/// To the second, with the nanoseconds dropped. The question this date
/// answers is "how long has this been sitting on my disk?", which nine digits
/// of precision do not help with and do make harder to read.
fn now() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "an unformattable moment".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    /// A data root to leave things behind in.
    fn a_root(label: &str) -> (TempDir, DataRoot) {
        let temp = TempDir::new(label);
        let root = DataRoot::open(temp.path()).expect("a throwaway data root opens");
        (temp, root)
    }

    /// A directory under `jobs/` holding files of exactly these lengths.
    fn a_tree(root: &DataRoot, name: &str, lengths: &[usize]) -> PathBuf {
        let path = root.jobs_dir().join(name);
        fs::create_dir_all(path.join("work")).expect("the fixture makes a tree");
        for (nth, length) in lengths.iter().enumerate() {
            fs::write(
                path.join("work").join(format!("f{nth}")),
                vec![b'x'; *length],
            )
            .expect("the fixture writes a file");
        }
        path
    }

    fn notebook(root: &DataRoot) -> String {
        fs::read_to_string(root.left_behind_path()).unwrap_or_default()
    }

    /// The entry is a path **and** a figure, and the figure is arithmetic
    /// anybody can check: three files of 3 000, 2 000 and 2 000 bytes.
    ///
    /// A line with no figure in it is the one report nobody can act on — the
    /// owner cannot tell a directory worth clearing from one that is not —
    /// so every line of the entry is asserted to carry a number.
    #[test]
    fn what_was_left_behind_is_named_with_what_it_weighs() {
        let (_temp, root) = a_root("janitor-figure");
        let left = a_tree(&root, "j0000000023-1", &[3_000, 2_000, 2_000]);
        write_down(
            &root,
            &Leftover::workspace_of(&left, &JobId::new("j0000000023"), 1, "Permission denied"),
        );

        let text = notebook(&root);
        assert!(
            text.contains(&format!("left-behind: {}", left.display())),
            "the entry has to name the path somebody must go to: {text}"
        );
        assert!(
            text.contains(&format!("holds: {} in 3 file(s)", show_bytes(7_000))),
            "and what it weighs, by arithmetic: {text}"
        );
        assert!(
            text.contains("left-by: job j0000000023, attempt 1"),
            "and whose it was: {text}"
        );
        assert!(
            text.contains("why: Permission denied"),
            "and the error the runner got, verbatim: {text}"
        );
        assert!(
            text.contains(&format!(
                "cost-of-not: {} stay on this disk",
                show_bytes(7_000)
            )),
            "and what not acting costs, in bytes: {text}"
        );
        for line in text
            .lines()
            .filter(|line| line.starts_with("holds:") || line.starts_with("cost-of-not:"))
        {
            assert!(
                line.chars().any(|c| c.is_ascii_digit()),
                "a line of this file with no figure in it is one nobody can act on: {line}"
            );
        }
    }

    /// Durability is the whole reason this is a file and not a log line, so
    /// it is proved through the runner's **own** startup path twice over —
    /// `Confined::prepare`, which is what runs the reaper — and not through a
    /// stand-in for it (D211's lesson).
    ///
    /// The leftover is a workspace whose directory is a **file**, so
    /// `remove_dir_all` fails with `ENOTDIR` and no mode can fix it: the one
    /// shape of unreclaimable workspace an unprivileged test can make.
    #[test]
    fn the_list_survives_the_restart_that_is_the_whole_reason_for_it() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("janitor-restart");
        let planted = temp.path().join("jobs").join("j0000000024-1");
        fs::create_dir_all(temp.path().join("jobs")).expect("the fixture makes jobs/");
        fs::write(&planted, vec![b'w'; 4_096]).expect("the fixture wrecks a workspace");

        let first = crate::confine::Confined::prepare(crate::confine::Confinement::with_data_root(
            temp.path(),
        ))
        .expect("a throwaway data root is confinable");
        // Through the data root rather than through the fixture's own path: a
        // data root is canonicalised when it is opened, and a temporary
        // directory reached through a symlink would otherwise be two spellings
        // of one place.
        let wrecked = first.root().jobs_dir().join("j0000000024-1");
        let said = first.report().join("\n");
        assert!(
            said.contains("COULD NOT reclaim the leftover workspace"),
            "the reaper has to have tried and failed, or this test proves nothing: {said}"
        );
        let after_one_start = notebook(first.root());
        assert!(
            after_one_start.contains(&format!("left-behind: {}", wrecked.display())),
            "what the reaper could not take away has to be written down: {after_one_start}"
        );
        // The lock on the data root is held for the life of the value, so the
        // restart is this drop and the next prepare.
        drop(first);

        let second = crate::confine::Confined::prepare(
            crate::confine::Confinement::with_data_root(temp.path()),
        )
        .expect("the same data root opens again");
        let said = second.report().join("\n");
        assert!(
            said.contains("left behind:") && said.contains(&wrecked.display().to_string()),
            "a second start has to say what is still on this host: {said}"
        );
        let after_two_starts = notebook(second.root());
        assert_eq!(
            after_two_starts.matches("left-behind: ").count(),
            1,
            "one thing is one entry however many times a runner meets it: \
             {after_two_starts}"
        );
        let block_of = |text: &str| {
            text.split("left-behind: ")
                .nth(1)
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(
            block_of(&after_one_start),
            block_of(&after_two_starts),
            "and the entry keeps the moment it was first seen, not the moment of the \
             last boot"
        );
        // The fixture's file, so the temporary directory can be removed.
        let _ = fs::remove_file(&planted);
    }

    /// The list has to shrink, or it becomes one more file nobody opens.
    #[test]
    fn an_entry_whose_thing_the_owner_has_cleared_comes_off_the_list() {
        let (_temp, root) = a_root("janitor-cleared");
        let left = a_tree(&root, "j0000000025-1", &[1_000]);
        write_down(
            &root,
            &Leftover::workspace_of_a_past_life(&left, "Read-only file system"),
        );
        assert!(
            notebook(&root).contains("left-behind: "),
            "it is on the list"
        );

        // The owner does what the entry told them to.
        fs::remove_dir_all(&left).expect("the owner can remove their own directory");

        let said = reconcile(&root).join("\n");
        assert!(
            said.contains("no longer on this host"),
            "a start that finds a cleared thing has to say so: {said}"
        );
        let text = notebook(&root);
        assert!(
            !text.contains(&left.display().to_string()),
            "and the entry has to be gone: {text}"
        );
        assert!(
            text.contains("Entries now: 0"),
            "the header counts what is left: {text}"
        );
        assert!(
            reconcile(&root).is_empty(),
            "and an empty list is silence, not a line about nothing"
        );
    }

    /// Strictly what the runner **cannot** reclaim. If Herd can clean it,
    /// Herd cleans it and says nothing here — so a path that is not on the
    /// disk by the time the entry would be written is never written down,
    /// whatever the caller believed when it asked.
    #[test]
    fn nothing_the_runner_could_reclaim_itself_is_ever_on_the_list() {
        let (_temp, root) = a_root("janitor-reclaimed");
        let workspace = root
            .open_workspace(&JobId::new("j0000000026"))
            .expect("a workspace is made");
        let base = workspace.base_dir().to_path_buf();
        workspace
            .reclaim()
            .expect("the runner reclaims its own workspace");

        write_down(
            &root,
            &Leftover::workspace_of(&base, &JobId::new("j0000000026"), 1, "a stale error"),
        );
        assert_eq!(
            notebook(&root),
            "",
            "a workspace the runner did take away is not a thing anybody has to clear"
        );
        assert!(
            !root.left_behind_path().exists(),
            "and a runner that left nothing behind writes no file at all"
        );
    }

    /// A spool is the only copy of a requester's evidence until the hub holds
    /// it (D19, D211), and a held result the only copy of a verdict (D34).
    /// Neither is garbage, so neither can be named here — and the refusal is
    /// in this module rather than in the habits of its callers.
    #[test]
    fn a_spool_and_a_held_result_can_never_be_named_as_garbage() {
        let (_temp, root) = a_root("janitor-spool");
        let spool = root.outputs_dir().join("j0000000027-1");
        fs::create_dir_all(&spool).expect("the fixture makes a spool");
        fs::write(spool.join("pack"), vec![b'p'; 2_048]).expect("the fixture fills it");
        let held = root.held_results_dir().join("j0000000027.json");
        fs::create_dir_all(root.held_results_dir()).expect("the fixture makes held-results/");
        fs::write(&held, b"{}").expect("the fixture writes a verdict");

        for path in [&spool, &held] {
            write_down(
                &root,
                &Leftover::workspace_of_a_past_life(path, "whatever a caller believed"),
            );
        }
        assert_eq!(
            notebook(&root),
            "",
            "the list names neither, and so can never send an owner to delete one"
        );
        assert!(spool.join("pack").exists(), "and nothing here removed them");
        assert!(held.exists());

        // Nor anything outside the root this runner owns.
        write_down(
            &root,
            &Leftover::workspace_of_a_past_life(Path::new("/etc"), "nothing good"),
        );
        assert_eq!(notebook(&root), "");
    }

    /// A janitor's notebook is not on the critical path: a file that cannot
    /// be written costs a line on stderr and nothing else. Here the path is
    /// occupied by a **directory**, which no `write` can replace.
    #[test]
    fn a_notebook_that_cannot_be_written_costs_the_caller_nothing() {
        let (_temp, root) = a_root("janitor-unwritable");
        fs::create_dir_all(root.left_behind_path()).expect("the fixture blocks the path");
        let left = a_tree(&root, "j0000000028-1", &[512]);

        // Returns, like every other call: there is nothing for a caller to
        // handle, which is what keeps it off a job's path.
        write_down(
            &root,
            &Leftover::workspace_of(&left, &JobId::new("j0000000028"), 1, "Permission denied"),
        );
        assert!(
            root.left_behind_path().is_dir(),
            "nothing here removes or renames what is in the way"
        );
        assert!(
            reconcile(&root).is_empty(),
            "and a notebook that cannot even be read is silence, not a refusal to start"
        );
    }

    /// The list is bounded, because a runner may not write a file whose
    /// length is a function of how long it has been unlucky. The bound keeps
    /// what it has — the oldest entry is the one that has been on the disk
    /// longest — and says what it could not add.
    #[test]
    fn the_list_stops_at_its_bound_and_keeps_what_it_has() {
        let (_temp, root) = a_root("janitor-bound");
        let mut text = String::new();
        for nth in 0..MOST_ENTRIES_LISTED {
            let path = a_tree(&root, &format!("filler-{nth}"), &[]);
            text.push_str(&format!(
                "\nleft-behind: {}\nholds: 0 bytes in 0 file(s)\n",
                path.display()
            ));
        }
        fs::write(root.left_behind_path(), &text).expect("the fixture seeds a full list");
        let one_too_many = a_tree(&root, "j0000000029-1", &[64]);

        write_down(
            &root,
            &Leftover::workspace_of(&one_too_many, &JobId::new("j0000000029"), 1, "refused"),
        );
        let after = notebook(&root);
        assert!(
            !after.contains(&one_too_many.display().to_string()),
            "the bound holds: the {MOST_ENTRIES_LISTED} already listed are not pushed out \
             for this one"
        );
        assert_eq!(
            after.matches("left-behind: ").count(),
            MOST_ENTRIES_LISTED,
            "and nothing already named was lost"
        );
    }

    /// What a figure means when the walk could not finish. A floor is said to
    /// be a floor; it is never passed off as a total, and never as a zero
    /// (D23).
    #[test]
    fn a_figure_the_walk_could_not_finish_is_a_floor_and_says_so() {
        let complete = Weight {
            bytes: 7_000,
            files: 3,
            unread: 0,
            truncated: false,
        };
        assert_eq!(complete.sentence(), "7 kB in 3 file(s)");

        let refused = Weight {
            unread: 2,
            ..complete.clone()
        };
        assert!(
            refused.sentence().starts_with("at least 7 kB in 3 file(s)")
                && refused.sentence().contains("2 path(s)"),
            "{}",
            refused.sentence()
        );

        let truncated = Weight {
            truncated: true,
            ..complete
        };
        assert!(
            truncated.sentence().starts_with("at least ")
                && truncated
                    .sentence()
                    .contains(&MOST_ENTRIES_WEIGHED.to_string()),
            "{}",
            truncated.sentence()
        );

        // And the empty tree, which is a figure and not the absence of one.
        assert_eq!(Weight::default().sentence(), "0 bytes in 0 file(s)");
    }

    /// This module reads the disk and writes its own file. It removes,
    /// renames and chmods **nothing** — which is what makes "your list can
    /// never destroy a spool" a property of the code and not of its callers'
    /// good manners.
    ///
    /// Asserted over the source above the tests, because the fixtures below
    /// do remove their own temporary directories.
    #[test]
    fn this_module_takes_nothing_away() {
        let marker = "#[cfg(test)]";
        let mine = crate::testing::source_files()
            .into_iter()
            .find(|(path, _)| path.ends_with("janitor.rs"))
            .map(|(_, text)| text)
            .expect("this module's own source is readable");
        let (code, _) = mine
            .split_once(marker)
            .expect("this module ends in its tests");
        for forbidden in [
            "remove_file",
            "remove_dir",
            "remove_dir_all",
            "rename(",
            "set_permissions",
            "symlink(",
            "cgroup.kill",
        ] {
            assert!(
                !code.contains(forbidden),
                "the notebook may name what is left behind and never take anything away, \
                 and this source reaches for {forbidden}"
            );
        }
    }
}
