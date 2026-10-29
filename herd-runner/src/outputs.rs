//! Collecting what a job left under `{out}`, before the workspace goes.
//!
//! A job's `{out}` directory lives inside its workspace, and the workspace is
//! reclaimed when the job ends — success, failure or death ([`crate::job`]).
//! So the collection happens at exactly one point: after everything of the
//! job has been killed and before [`crate::root::Workspace::reclaim`], on
//! **every** ending that produced a workspace. D108 fixed a workspace leaking
//! on failure for this class of reason, and the same reasoning runs the other
//! way here: a job that fails is very often the one whose output you most
//! want, so there is no success condition on this and never will be.
//!
//! ## What is collected, and what is not
//!
//! Everything under `{out}` that is a **regular file**, with its path
//! relative to `{out}` preserved. Four deliberate exclusions, each of which
//! earns a sentence in [`JobOutputs::skipped`] rather than silence:
//!
//! * **A symlink is never followed, wherever it points.** Not "not followed
//!   if it leaves the workspace" — not followed at all. Herd cannot tell a
//!   link to `./results.json` from a link to `/etc/shadow` without
//!   resolving it, resolving it is a `TOCTOU` race against a job that is
//!   still being killed, and the one safe rule is the one with no exception
//!   in it. `symlink_metadata` is therefore the only `stat` in this file.
//! * **An empty directory is not represented.** A directory exists in the
//!   record because a file under it does; there is no file to carry, so
//!   there is nothing to carry. A consumer that needs an empty directory
//!   creates it.
//! * **Anything that is not a regular file and not a directory** — a fifo, a
//!   socket, a device — is skipped by its type, *before* anything is opened.
//!   Opening a fifo nobody is writing blocks for ever, which would hang the
//!   job's own report; testing the type first is what makes that impossible
//!   rather than unlikely.
//! * **A name that is not UTF-8**, because a path on the wire is a JSON
//!   string. Rare, and named rather than dropped.
//!
//! A hard link is a regular file and is collected; two names for one inode
//! come back as two files, which is what the directory says they are.
//!
//! ## What a job killed mid-write leaves behind
//!
//! A half-written `results.json`, and it is collected as it stands. That is
//! acceptable, and the alternative is worse: Herd cannot know what the
//! consumer's writer meant to write, so the only honest choice is between the
//! bytes that are there and no bytes at all — and a job stopped at its time
//! cap, which is a case someone is actively debugging, is precisely when the
//! partial file is the evidence. What makes it safe to read is that the
//! verdict says the job was capped, cancelled or killed: a consumer parses
//! `results.json` on the strength of the verdict, not on the strength of the
//! file being present. The consumer's own `{out}` writer should rename into
//! place if it wants all-or-nothing, which costs Herd nothing to support and
//! is theirs to decide.
//!
//! ## Measured first, then the carriage, then read
//!
//! The walk takes every file's size and charges [`OutputFile::weight`] for
//! each — bytes, path and [`herd_proto::PER_FILE_OVERHEAD`] — and not one
//! byte is read until the whole collection has been weighed. That order is
//! what makes the carriage decision possible (D148): a requester declares
//! nothing about size, because nobody knows in advance whether a run leaves
//! 1 MiB or 400 MiB, so the choice is made **here**, off the figures, at the
//! only moment when everything is measured and nothing is read.
//!
//! * at or under [`herd_proto::INLINE_OUTPUT_MAX`] the files are read into
//!   the verdict frame, exactly as they always were;
//! * above it each file is read **once, to hash it**, the manifest goes on
//!   the verdict, and `{out}` is **renamed** out of the workspace so the
//!   bytes outlive the reclaim and can be streamed afterwards.
//!
//! Over the job's whole cap, **no file comes back** and the refusal names the
//! cap and the real figure, because a truncated `results.json` that still
//! parses is the worst possible outcome: it reads like the truth.
//!
//! ## What the manifest promises
//!
//! A length and a sha256 per file, announced on the verdict before a byte
//! moves. A file whose size changed between the walk and the hash is **left
//! out and named** rather than announced at a figure the stream would then
//! contradict: a manifest describes bytes that have stopped changing, and a
//! file still being written by a background child of a job the runner is
//! killing has not.

use std::fs;
use std::path::{Path, PathBuf};

use herd_proto::{JobOutputs, OutputFile, PER_FILE_OVERHEAD};

use crate::root::read_dir_restoring;
use crate::size::{measured_bytes, show_bytes};

/// One file found under `{out}`, before its bytes are read.
struct Found {
    relative: String,
    path: PathBuf,
    bytes: u64,
}

impl Found {
    /// What it will cost the cap once it is an [`OutputFile`], charged before
    /// anything is read.
    fn weight(&self) -> u64 {
        self.bytes + self.relative.len() as u64 + PER_FILE_OVERHEAD
    }
}

/// A collection, and where its bytes are if they are not in it.
pub(crate) struct Collected {
    /// What the verdict carries: the manifest always, the bytes when they
    /// fit on the frame.
    pub outputs: JobOutputs,
    /// Where the bytes wait, when they are travelling by the streamed
    /// carriage. `None` for an inline collection, a refusal, or nothing at
    /// all — in each of those cases the verdict is the whole of the delivery
    /// and there is nothing for a spool to hold.
    pub spool: Option<PathBuf>,
}

impl Collected {
    fn verdict_only(outputs: JobOutputs) -> Self {
        Self {
            outputs,
            spool: None,
        }
    }

    /// Nothing collected and nothing to collect: an abandoned attempt.
    pub(crate) fn nothing() -> Self {
        Self::verdict_only(JobOutputs::default())
    }
}

/// Where one attempt's streamed bytes wait for the hub.
///
/// Under the data root, so the rename out of the workspace stays a rename
/// (see [`spool_into`]), and keyed by **attempt** as well as by job: a second
/// attempt's output is not the first's, which is the same reason every file
/// frame carries an attempt number.
pub(crate) fn spool_path(root: &Path, job_id: &herd_proto::JobId, attempt: u32) -> PathBuf {
    root.join(SPOOL_DIR).join(format!("{job_id}-{attempt}"))
}

/// The directory every attempt's spool sits under, inside the data root.
pub(crate) const SPOOL_DIR: &str = "outputs";

/// Collect what the job left under `out_dir`, or say why nothing came back.
///
/// `root` is the runner's data root: the boundary passed to the mode-restoring
/// walk, which may take back the runner's own access inside it and must never
/// touch anything above it (E-45).
///
/// `cap` of 0 switches the collection off entirely — an owner who wants her
/// machine to return nothing but a verdict — and says so rather than
/// reporting an empty `{out}`.
///
/// `spool_at` is where the bytes are moved to if they are too big to ride the
/// verdict. It must be on the same filesystem as `out_dir` — both are under
/// the runner's data root — because the move is a **rename**: see
/// [`spool_into`].
pub(crate) fn collect(out_dir: &Path, root: &Path, cap: u64, spool_at: &Path) -> Collected {
    if !out_dir.exists() {
        // An ad-hoc job: no declared command, so no `{out}` was ever made.
        // Nothing to say, and nothing on the wire.
        return Collected::verdict_only(JobOutputs::default());
    }
    let (found, mut skipped) = walk(out_dir, root);
    if cap == 0 {
        if found.is_empty() && skipped.is_empty() {
            return Collected::verdict_only(JobOutputs::default());
        }
        return Collected::verdict_only(JobOutputs::refusing(
            format!(
                "this runner returns no output file: its output cap is 0 bytes, and the job \
                 left {} file(s) under its output directory. Raise --job-output-cap on the \
                 machine if you want its results back.",
                found.len()
            ),
            skipped,
        ));
    }
    let weight: u64 = found.iter().map(Found::weight).sum();
    if weight > cap {
        return Collected::verdict_only(JobOutputs::refusing(
            over_the_cap(cap, weight, found.len()),
            skipped,
        ));
    }

    // **Measured, then the carriage, then a single byte is read.** The choice
    // is made off the walk's figures, which is what lets it be made at all: a
    // requester declares nothing about size (D148), so somebody has to decide
    // here, and the only honest moment is after everything is measured and
    // before anything is read. Inline reads the files into the frame, exactly
    // as it always did; streamed reads each one once to hash it and leaves
    // the bytes on the disk.
    if weight <= herd_proto::INLINE_OUTPUT_MAX {
        return Collected::verdict_only(inline(found, skipped));
    }

    let mut files = Vec::with_capacity(found.len());
    for file in &found {
        match hash_file(&file.path) {
            Ok((len, sha256)) if len == file.bytes => {
                files.push(OutputFile::announced(file.relative.clone(), len, sha256))
            }
            // Measured at one size and read at another: the job's own
            // background child still writing while the runner killed it. The
            // manifest has to describe bytes that will not move again, so
            // this file is named and left out rather than announced at a
            // length the stream would then disagree with — which would fail
            // at the far end, for a reason the far end could not explain.
            Ok((len, _)) => skipped.push(format!(
                "{} was {} byte(s) when it was measured and {len} when it was read, so it \
                 is not in this record: a manifest has to describe bytes that have stopped \
                 changing",
                file.relative, file.bytes
            )),
            Err(error) => skipped.push(format!(
                "{} was measured and then could not be read: {error}",
                file.relative
            )),
        }
    }
    // Nothing left to stream — every file went away between the walk and the
    // read — so there is nothing for a spool to hold and nothing for the
    // streamed carriage to do. The skipped lines are the whole report.
    if files.is_empty() {
        return Collected::verdict_only(JobOutputs {
            skipped,
            ..JobOutputs::default()
        });
    }
    match spool_into(out_dir, spool_at) {
        Ok(()) => Collected {
            outputs: JobOutputs {
                files,
                refused: None,
                skipped,
                streamed: true,
            },
            spool: Some(spool_at.to_path_buf()),
        },
        // The bytes could not be put somewhere that outlives the workspace,
        // so they are about to be destroyed by the reclaim and there is no
        // honest way to announce them. Named with the figures, because
        // "nothing came back" needs a reason beside it either way.
        Err(error) => Collected::verdict_only(JobOutputs::refusing(
            not_spooled(weight, found.len(), &error),
            skipped,
        )),
    }
}

/// Read every file into the frame: the carriage that has always existed.
fn inline(found: Vec<Found>, mut skipped: Vec<String>) -> JobOutputs {
    let mut files = Vec::with_capacity(found.len());
    for file in found {
        match fs::read(&file.path) {
            Ok(data) => files.push(OutputFile::collected(file.relative, data)),
            // Measured a moment ago and unreadable now: a background child
            // of the job deleting its own output while the runner was killing
            // it, or a mode no restore could take back. Named, because "it
            // was there when I looked" is the fact somebody needs.
            Err(error) => skipped.push(format!(
                "{} was measured and then could not be read: {error}",
                file.relative
            )),
        }
    }
    JobOutputs {
        files,
        refused: None,
        skipped,
        streamed: false,
    }
}

/// How long a file is and what it hashes to, read once and held nowhere.
///
/// Both figures come out of the same pass, which is the point: a length taken
/// from `stat` and a digest taken from a read are two statements about a file
/// that may have changed in between, and the manifest has to carry two that
/// agree.
fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut rolling = herd_proto::Sha256Rolling::new();
    let mut buffer = vec![0u8; 64 << 10];
    let mut len = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        rolling.update(&buffer[..read]);
        len += read as u64;
    }
    Ok((len, rolling.finish()))
}

/// Move `{out}` out of the workspace, so the bytes outlive the reclaim.
///
/// **A rename, not a copy, and that is why the spool lives under the data
/// root.** Both directories are inside it, so they are on one filesystem and
/// this is a directory entry being moved: constant time, no second copy of
/// 400 MiB, and nothing to half-finish. A copy would have doubled the disk a
/// large collection needs at exactly the moment the machine is already
/// holding it once.
///
/// What is moved is the whole directory, including the things the walk
/// skipped — a symlink it would not follow, a fifo it would not open.
/// Nothing ever opens them: the stream reads the manifest's paths and only
/// those, and the spool is removed whole once the hub has the collection.
fn spool_into(out_dir: &Path, spool_at: &Path) -> std::io::Result<()> {
    if let Some(parent) = spool_at.parent() {
        fs::create_dir_all(parent)?;
    }
    // A spool left by an earlier attempt of the same job is not this
    // attempt's output, and renaming onto a non-empty directory fails. Take
    // it away first: a previous attempt's files are never the current one's
    // (the reason every file frame carries an attempt).
    if spool_at.exists() {
        fs::remove_dir_all(spool_at)?;
    }
    fs::rename(out_dir, spool_at)
}

/// The sentence a collection over the cap is refused with.
///
/// E-04's style: the limit **and** the measurement, so a reader can see how
/// far over it is rather than only that it is over. It also says where the
/// large case belongs, because "it does not fit" with no second sentence
/// reads as "Herd cannot do this".
///
/// **The two figures are said by two formatters, and that is deliberate.**
/// The cap is what someone typed after `--job-output-cap`, and it reads back
/// in the family they typed it in or they cannot tell the runner took the
/// number they meant. The weight is what this runner measured, and
/// [`crate::size::measured_bytes`] prints it exactly, because a measured
/// figure whose unit depends on what it happens to divide by is how
/// `535691 kB` came to be one line in seventy-five.
pub(crate) fn over_the_cap(cap: u64, weight: u64, count: usize) -> String {
    format!(
        "this job's output is not in its record: {} in {count} file(s), and this runner's \
         output cap is {}. Nothing was truncated — a cut-down results file that still parses \
         reads like the truth — so no file came back at all. Raise --job-output-cap on the \
         machine, up to its disk budget, if a job of this size is meant to hand its output \
         back.",
        measured_bytes(weight),
        show_bytes(cap)
    )
}

/// The sentence a collection that could not be moved out of the workspace is
/// refused with.
///
/// **Its own function so the line can be pinned.** Staging a failing rename
/// in a test means arranging the filesystem to refuse one, and the sentence a
/// reader sees would then be checked only through whatever errno that
/// arrangement produced. The figures are the part somebody downstream reads,
/// so they are asserted directly.
///
/// The weight is a measurement — this runner walked those files — so it is
/// said as one, exactly, with the approximation beside it.
pub(crate) fn not_spooled(weight: u64, count: usize, error: &std::io::Error) -> String {
    format!(
        "this job's output is not in its record: {} in {count} file(s) had to be moved \
         out of the workspace before it was reclaimed, and that failed: {error}. Nothing \
         was truncated and nothing came back.",
        measured_bytes(weight)
    )
}

/// Walk `out_dir`, taking every regular file's size and naming everything
/// that is not one.
fn walk(out_dir: &Path, root: &Path) -> (Vec<Found>, Vec<String>) {
    let mut found: Vec<Found> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    // Breadth first over a queue of (directory, its path relative to
    // `{out}`), so a path is built from the names that were walked and never
    // from stripping a prefix off an absolute path.
    let mut pending = vec![(out_dir.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let entries = match read_dir_restoring(&directory, root) {
            Ok(Some(entries)) => entries,
            // Gone while we walked: a job deleting its own files as it is
            // killed is ordinary, and what is no longer there holds nothing.
            Ok(None) => continue,
            Err(error) => {
                skipped.push(format!(
                    "a directory under the output could not be read: {error}"
                ));
                continue;
            }
        };
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                skipped.push(format!(
                    "{} has a name that is not UTF-8, and a path on the wire is a JSON string",
                    entry.path().display()
                ));
                continue;
            };
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            // Never `entry.metadata()`, which would follow a symlink: the one
            // `stat` in this file does not resolve links, so a link out of
            // the workspace cannot be read through and a link loop cannot be
            // walked.
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    skipped.push(format!("{relative} could not be looked at: {error}"));
                    continue;
                }
            };
            let kind = metadata.file_type();
            if kind.is_symlink() {
                // Wherever it points. Herd cannot tell a harmless link from
                // one aimed at `/etc/shadow` without resolving it, and
                // resolving it is a race against a job that is still being
                // killed — so the rule has no exception and this is where
                // someone would be tempted to add one.
                skipped.push(format!(
                    "{relative} is a symlink, and a symlink is never followed: a link out of \
                     the workspace would put a file this job never wrote into its record"
                ));
                continue;
            }
            if kind.is_dir() {
                pending.push((entry.path(), relative));
                continue;
            }
            if !kind.is_file() {
                // By its type, before anything is opened: a fifo nobody
                // writes would block the open for ever and hold up the job's
                // own report.
                skipped.push(format!(
                    "{relative} is not a regular file ({kind:?}) and holds no bytes to keep"
                ));
                continue;
            }
            // Checked here as well as at the hub, because a path this walk
            // cannot name is a path the hub would refuse anyway, and it is
            // cheaper to say so beside the file it is about.
            if let Err(why) = herd_proto::check_relative(&relative) {
                skipped.push(format!("{relative} is not a path a record can hold: {why}"));
                continue;
            }
            found.push(Found {
                relative,
                path: entry.path(),
                bytes: metadata.len(),
            });
        }
    }
    // One order, whatever the filesystem's own is, so two runs of one job
    // produce the same record and a test can read the list.
    found.sort_by(|a, b| a.relative.cmp(&b.relative));
    skipped.sort();
    (found, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    /// A `{out}` under a throwaway data root, as a job's workspace holds one.
    struct Out {
        temp: TempDir,
    }

    impl Out {
        fn new(name: &str) -> Out {
            let temp = TempDir::new(name);
            fs::create_dir_all(temp.path().join("jobs/j1/out")).expect("an output directory");
            Out { temp }
        }

        fn root(&self) -> PathBuf {
            self.temp.path().to_path_buf()
        }

        fn out(&self) -> PathBuf {
            self.temp.path().join("jobs/j1/out")
        }

        fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
            let path = self.out().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("the parent directory");
            }
            fs::write(&path, bytes).expect("the file is written");
            path
        }

        fn collect(&self, cap: u64) -> JobOutputs {
            self.collected(cap).outputs
        }

        /// The whole result, spool and all, for the tests that are about the
        /// carriage rather than about what was walked.
        fn collected(&self, cap: u64) -> Collected {
            collect(&self.out(), &self.root(), cap, &self.spool())
        }

        fn spool(&self) -> PathBuf {
            self.root().join("outputs/j1-1")
        }
    }

    /// The whole of what the first consumer asks for: a `results.json` under
    /// `{out}/files/` comes back **as** `files/results.json`.
    #[test]
    fn a_file_under_a_directory_keeps_its_relative_path() {
        let out = Out::new("outputs-paths");
        out.write("files/results.json", br#"{"ok":true}"#);
        out.write("files/deep/inner.txt", b"inner");
        out.write("top.txt", b"top");
        let collected = out.collect(1 << 20);
        assert_eq!(collected.refused, None, "{collected:?}");
        assert_eq!(
            collected
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["files/deep/inner.txt", "files/results.json", "top.txt"],
            "the paths are relative to {{out}} and keep their directories"
        );
        let results = collected
            .files
            .iter()
            .find(|file| file.path == "files/results.json")
            .expect("the results file is there");
        assert_eq!(results.bytes(), br#"{"ok":true}"#);
    }

    /// Over the cap: no file, and a sentence with both figures in it.
    #[test]
    fn over_the_cap_nothing_comes_back_and_the_refusal_carries_the_figures() {
        let out = Out::new("outputs-cap");
        out.write("files/results.json", &vec![b'x'; 4096]);
        let cap = 1024;
        let collected = out.collect(cap);
        assert!(
            collected.files.is_empty(),
            "over the cap is all or nothing, never a truncation: {collected:?}"
        );
        let refusal = collected.refused.expect("a refusal with figures");
        assert!(
            refusal.contains(&show_bytes(cap)),
            "the refusal must name the cap: {refusal}"
        );
        assert!(
            refusal.contains("4 KiB") || refusal.contains("bytes"),
            "the refusal must name the actual size: {refusal}"
        );
        assert!(
            refusal.contains("1 file(s)"),
            "and how many files it was: {refusal}"
        );
        // It used to have to name M11 as somewhere else to go. M11 **is**
        // this — the streamed carriage above the inline threshold — so what
        // the sentence has to name now is the flag and what bounds it (D148).
        assert!(
            refusal.contains("--job-output-cap"),
            "and the flag that raises it: {refusal}"
        );
        assert!(
            refusal.contains("disk budget"),
            "and what bounds that flag, since there is no longer a frame to point at: \
             {refusal}"
        );
    }

    /// The empty-file case the per-file charge exists for: a thousand files
    /// weighing nothing at all still weigh something against the cap, so a
    /// frame no byte cap bounded cannot be built.
    #[test]
    fn a_crowd_of_empty_files_is_charged_and_refused() {
        let out = Out::new("outputs-crowd");
        for n in 0..100 {
            out.write(&format!("files/{n}.empty"), b"");
        }
        let collected = out.collect(100 * PER_FILE_OVERHEAD / 2);
        assert!(
            collected.files.is_empty(),
            "empty files are not free: {collected:?}"
        );
        assert!(collected.refused.is_some());
        // And the same crowd fits under a cap that really does cover them.
        let collected = out.collect(100 * (PER_FILE_OVERHEAD + 32));
        assert_eq!(collected.refused, None, "{collected:?}");
        assert_eq!(collected.files.len(), 100);
    }

    /// A symlink is never followed, wherever it points — and the one that
    /// matters is the one pointing out of the workspace.
    #[test]
    fn a_symlink_out_of_the_workspace_is_not_followed() {
        let out = Out::new("outputs-symlink");
        let secret = out.temp.path().join("not-the-job-s.txt");
        fs::write(&secret, b"this must never reach a record").expect("a file outside {out}");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, out.out().join("files-stolen.txt"))
                .expect("a symlink out of the workspace");
            std::os::unix::fs::symlink("/etc/hostname", out.out().join("absolute.txt"))
                .expect("a symlink to an absolute path");
            // A loop, which a walk that resolved links would never leave.
            std::os::unix::fs::symlink(out.out(), out.out().join("loop")).expect("a loop");
        }
        out.write("files/results.json", b"mine");
        let collected = out.collect(1 << 20);
        assert_eq!(
            collected
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["files/results.json"],
            "only the regular file is collected: {collected:?}"
        );
        for file in &collected.files {
            assert_ne!(
                file.bytes(),
                b"this must never reach a record",
                "a symlink was followed"
            );
        }
        #[cfg(unix)]
        {
            assert_eq!(
                collected.skipped.len(),
                3,
                "each link is named rather than silently dropped: {:?}",
                collected.skipped
            );
            assert!(
                collected
                    .skipped
                    .iter()
                    .all(|said| said.contains("symlink is never followed")),
                "{:?}",
                collected.skipped
            );
        }
    }

    /// Anything that is not a regular file: named by its type, and never
    /// opened — a fifo nobody writes would block the open for ever.
    #[test]
    #[cfg(unix)]
    fn a_fifo_is_named_by_its_type_and_never_opened() {
        let out = Out::new("outputs-fifo");
        let fifo = out.out().join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            crate::testing::skipped(
                "a_fifo_is_named_by_its_type_and_never_opened",
                "mkfifo is not on this host's PATH, so a non-regular file under {out} could \
                 not be made: a capable host would have proved that a fifo is named by its \
                 type and never opened, which is what stops a job hanging its own report",
            );
            return;
        }
        out.write("files/results.json", b"mine");
        let collected = out.collect(1 << 20);
        assert_eq!(
            collected
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["files/results.json"]
        );
        assert!(
            collected
                .skipped
                .iter()
                .any(|said| said.contains("pipe") && said.contains("not a regular file")),
            "{:?}",
            collected.skipped
        );
    }

    /// An empty directory is nothing to carry, and says nothing.
    #[test]
    fn an_empty_directory_is_not_represented_and_is_not_an_error() {
        let out = Out::new("outputs-empty-dir");
        fs::create_dir_all(out.out().join("files/nothing/here")).expect("empty directories");
        let collected = out.collect(1 << 20);
        assert!(collected.files.is_empty(), "{collected:?}");
        assert_eq!(collected.refused, None);
        assert!(
            collected.skipped.is_empty(),
            "an empty directory is not a complaint: {:?}",
            collected.skipped
        );
        assert!(
            collected.is_silent(),
            "and nothing of it goes on the wire: {collected:?}"
        );
    }

    /// A job with no `{out}` at all — every ad-hoc job — collects nothing and
    /// says nothing.
    #[test]
    fn a_job_with_no_output_directory_is_silent() {
        let temp = TempDir::new("outputs-none");
        let collected = collect(
            &temp.path().join("jobs/j1/out"),
            temp.path(),
            1 << 20,
            &temp.path().join("outputs/j1-1"),
        );
        assert!(collected.outputs.is_silent(), "{:?}", collected.outputs);
        assert!(collected.spool.is_none(), "nothing to spool");
    }

    /// A cap of 0 is an owner saying "a verdict and nothing else", and it
    /// says so rather than reading as an empty output directory.
    #[test]
    fn a_cap_of_nothing_refuses_by_name_rather_than_looking_empty() {
        let out = Out::new("outputs-cap-zero");
        out.write("files/results.json", b"mine");
        let collected = out.collect(0);
        assert!(collected.files.is_empty());
        let refusal = collected.refused.expect("a refusal by name");
        assert!(refusal.contains("output cap is 0 bytes"), "{refusal}");
        // With nothing there, a cap of 0 still says nothing at all.
        let empty = Out::new("outputs-cap-zero-empty");
        assert!(empty.collect(0).is_silent());
    }

    /// A directory the job chmodded shut is still walked: the runner made
    /// every inode under `{out}` and takes its own access back, exactly as
    /// the disk measurement does.
    #[test]
    #[cfg(unix)]
    fn a_directory_the_job_closed_is_still_collected() {
        use std::os::unix::fs::PermissionsExt;
        let out = Out::new("outputs-chmod");
        out.write("files/results.json", b"mine");
        fs::set_permissions(out.out().join("files"), fs::Permissions::from_mode(0o000))
            .expect("the job shuts its own directory");
        let collected = out.collect(1 << 20);
        assert_eq!(
            collected
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["files/results.json"],
            "a job cannot chmod its way out of its own record: {collected:?}"
        );
    }

    /// The refusal a reader of a verdict actually reads, pinned as text.
    ///
    /// **Two figures, two formatters, and that is the point of this test.**
    /// The cap was typed after `--job-output-cap` and reads back in the
    /// family it was typed in; the weight was measured by this runner's own
    /// walk and reads as an exact count with an approximation beside it.
    /// 535691000 is the first consumer's figure, the one that printed as
    /// `535691 kB` because it happens to divide by 1000 and by nothing
    /// larger.
    ///
    /// This is a literal expectation and not a round trip on purpose: a test
    /// that re-derived this line from the formatters would stay green through
    /// exactly the change that breaks whoever parses our output. **If it goes
    /// red, do not paste the new line over the expectation** — that repair is
    /// always wrong here, because the expectation is the contract.
    #[test]
    fn the_cap_refusal_says_a_typed_cap_in_its_family_and_a_measured_weight_in_bytes() {
        assert_eq!(
            over_the_cap(200_000_000_000, 535_691_000, 75),
            "this job's output is not in its record: 535691000 bytes (510.87 MiB) in 75 \
             file(s), and this runner's output cap is 200 GB. Nothing was truncated — a \
             cut-down results file that still parses reads like the truth — so no file came \
             back at all. Raise --job-output-cap on the machine, up to its disk budget, if \
             a job of this size is meant to hand its output back.",
            "the weight is measured and stays in bytes; the cap is typed and stays in the \
             family it was typed in. Do not paste new output over this expectation."
        );
    }

    /// The class the defect belongs to, at this call site: a cap and a weight
    /// that both divide exactly by 1000. Only the typed one may take a
    /// decimal unit.
    #[test]
    fn a_weight_that_divides_by_a_thousand_keeps_its_bytes_in_the_cap_refusal() {
        let refusal = over_the_cap(64 << 20, 2_000, 1);
        assert!(
            refusal.contains("2000 bytes (1.95 KiB)"),
            "a measured weight is exact whatever it divides by: {refusal}"
        );
        assert!(
            refusal.contains("output cap is 64 MiB"),
            "and the typed cap is unchanged: {refusal}"
        );
        for weight in [1_000u64, 1_000_000, 535_691_000, 1_000_000_000] {
            let refusal = over_the_cap(2_000_000_000_000, weight, 3);
            assert!(
                refusal.contains(&format!("record: {weight} bytes")),
                "the exact count comes first, always: {refusal}"
            );
            assert!(
                !refusal.contains(&format!("{} kB", weight / 1_000)),
                "and never the decimal unit the figure happens to divide by: {refusal}"
            );
        }
    }

    /// And the sentence for bytes that could not be moved out of the
    /// workspace, which carries the same measured figure.
    ///
    /// Literal, for the same reason as above. **Do not paste new output over
    /// it.**
    #[test]
    fn the_unspooled_refusal_says_its_measured_weight_in_bytes() {
        let error = std::io::Error::other("the data root went away");
        assert_eq!(
            not_spooled(535_691_000, 75, &error),
            "this job's output is not in its record: 535691000 bytes (510.87 MiB) in 75 \
             file(s) had to be moved out of the workspace before it was reclaimed, and \
             that failed: the data root went away. Nothing was truncated and nothing came \
             back.",
            "nobody typed this weight, so it is said exactly. Do not paste new output over \
             this expectation."
        );
        assert!(
            !not_spooled(2_000, 1, &error).contains("2 kB"),
            "nor at any other size that divides by a thousand"
        );
    }
}
