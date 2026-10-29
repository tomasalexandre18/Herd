//! The one predicate E-41 is built on: has a named ref moved since the last
//! run? Decided here, in the requester's own checkout, and nowhere else
//! (D106, D207).
//!
//! **This is not a scheduler and must not become one.** It answers one
//! question and submits one declared command. What time of day the question
//! is asked is the consumer's own timer, in the consumer's own tree, exactly
//! as E-42's push hook is theirs: `docs/scheduling.md` says what we ship and
//! what they write.
//!
//! Why the requester's side. The hub never holds a repository and never runs
//! git (D106): a hub that clones to decide when to build is the generic-CI
//! surface invariant 1 refuses, and it would need a credential for somebody
//! else's repository. `docs/plan.md`'s M14 says the opposite and is wrong on
//! the record since D207. So the ref is read where the repository already
//! is, by the same module that reads the commit a submission carries.
//!
//! **"The last run" is a file, and a file that cannot be read is never read
//! as a ref that moved.** Every way this state file can be wrong — missing,
//! unreadable, from another format version, kept for another ref, kept for
//! another command — is a refusal naming the file, because the alternative
//! is a nightly that fires on an empty directory. Guessing "it moved" is
//! the one answer that costs five hours of somebody's machine.
//!
//! **One run at a time, enforced and not assumed.** Two timers that overlap
//! are the failure this guards: the whole of read-decide-submit-record
//! happens under an exclusive lock on a file beside the state, and the
//! second run submits nothing and says so. The lock is `flock`-shaped — the
//! kernel drops it when the process ends, however it ends — so a run that
//! is killed leaves nothing stale behind.

use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::checkout::{Checkout, Commit};

/// The format of the state file, as the file itself carries it.
///
/// Read back strictly: a file written at any other version is refused by
/// name rather than interpreted. Two versions of `herdctl` disagreeing in
/// silence about what "the last run" means is a nightly that fires twice or
/// never, and neither is discoverable from the outside.
pub const STATE_FORMAT_VERSION: u32 = 1;

/// The line a run that submitted nothing prints, because the ref had not
/// moved.
///
/// Prefixed like the three lines of `JOB_ID_PREFIX`'s family and for the
/// same reason: what a wrapper reads is a documented prefix on stderr, never
/// the wording of a sentence.
///
/// **This line is the whole of the report, and that is a deviation from
/// E-41 written down rather than papered over.** E-41 asks for the skip to
/// be reported as an event. An event comes from the hub, and the hub never
/// heard of this job: nothing was submitted, so there is nothing for it to
/// emit. The skip E-41 *can* have as an event is the other one — a job
/// submitted with `--deadline 05:00` that no machine started by then, which
/// the hub cancels and reports (E-09, D40).
pub const NOT_MOVED_PREFIX: &str = "herdctl: not-moved: ";

/// The line a run that found another run holding the state file prints.
pub const ANOTHER_RUN_PREFIX: &str = "herdctl: another-run: ";

/// The line the run that submitted prints about why it did.
pub const MOVED_PREFIX: &str = "herdctl: moved: ";

/// What one invocation is watching: one ref, for one declared command, with
/// one state file of its own.
///
/// Three fields and not one, because the state file is checked against the
/// other two. A file kept for `refs/heads/main` that is handed to a run
/// asking about `refs/heads/release` would otherwise answer a question it
/// was never asked, and the answer would be "it moved".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watching {
    /// The file that records the last run, in the requester's own tree.
    pub state: PathBuf,
    /// The ref whose having moved is the condition, in full:
    /// `refs/remotes/origin/main`.
    pub watched: String,
    /// The declared command this file answers for.
    pub command: String,
}

impl Watching {
    /// Where the lock lives: beside the state file, named for it.
    ///
    /// A file of its own rather than the state file itself, because the
    /// state file is replaced by a rename and a lock follows the inode it
    /// was taken on. Locking the state file would hand the second run a
    /// lock on a file that is no longer the state file.
    ///
    /// It is never deleted. An empty file left in the tree is cheap; a lock
    /// file removed while another run holds it is a lock that stopped
    /// meaning anything.
    pub fn lock_path(&self) -> PathBuf {
        let mut name = self.state.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        self.state.with_file_name(name)
    }

    /// Where the replacement is written before it is renamed over the state
    /// file.
    fn pending_path(&self) -> PathBuf {
        let mut name = self.state.file_name().unwrap_or_default().to_os_string();
        name.push(".pending");
        self.state.with_file_name(name)
    }
}

/// What the state file holds: the last run, and what it was a run of.
///
/// JSON because it is the one serialiser this crate already writes with
/// (D166's verdict document), and because a person reading their own tree
/// should be able to see what Herd thinks happened last night without a
/// tool. It is small on purpose: this is a predicate's memory, not a
/// history. The hub's record is where a job's history lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// [`STATE_FORMAT_VERSION`], first, as the verdict document carries its
    /// own (D166).
    pub state_format_version: u32,
    /// The ref this file answers for, in full.
    #[serde(rename = "ref")]
    pub watched: String,
    /// The declared command the last run submitted.
    pub command: String,
    /// The commit the ref stood at. Forty lowercase hex: the predicate
    /// compares hashes and never names.
    pub commit: String,
    /// When the run was recorded, RFC 3339. Nothing compares it — it is
    /// there for the person reading the file.
    pub recorded_at: String,
    /// The job that run submitted, when there was one. `--init` records no
    /// job, because it submits none.
    pub job_id: Option<String>,
}

/// An exclusive hold on one state file, for as long as this value lives.
///
/// The hold is the lock on [`Watching::lock_path`]: `flock`-shaped, so it
/// goes when this process goes and leaves nothing to clean up. It exists as
/// a value so that the lock cannot be dropped between the decision and the
/// record — the compiler keeps it alive across the submission.
#[derive(Debug)]
pub struct Held {
    lock: File,
}

impl Held {
    /// The file the lock is on, kept open so nothing can close it early.
    fn of(lock: File) -> Held {
        Held { lock }
    }

    /// Named so the field is read somewhere, which is what keeps a reader
    /// from deleting it as unused: dropping it would release the lock.
    fn file(&self) -> &File {
        &self.lock
    }
}

/// What one run of the predicate decided.
#[derive(Debug)]
pub enum Decision {
    /// The ref moved, and this run holds the state file while it submits.
    Moved {
        /// The hold, which must live until the record is written.
        held: Held,
        /// The commit the state file recorded.
        was: String,
        /// Where the ref stands now: the commit the job will carry, read
        /// once so that the sha the predicate saw, the sha the job builds
        /// and the sha the file records are the same sha.
        tip: Commit,
    },
    /// The ref is where the last run left it. Nothing is submitted.
    Unmoved {
        /// Where it stands, which is also what the file records.
        commit: String,
        /// When the last run was recorded, for the line a person reads.
        since: String,
    },
    /// Another run holds the state file. Nothing is submitted, and nothing
    /// is said about whether that run will submit: it is holding the file
    /// and will record what it does.
    AnotherRun {
        /// The lock file somebody else is holding.
        lock: PathBuf,
    },
}

/// Take the hold, read the ref, read the file, and decide.
///
/// In that order, and the order is the rule.
///
/// The **lock** comes first, so that the read and the write that follows
/// are one run's. Then the **ref**, before the state file, for the reason
/// [`Checkout::resolve`] resolves a name before it looks at the tree: a
/// person who mistyped a ref must be told about the ref, even when their
/// state file is also wrong, and a hash handed to `--ref` must be refused
/// for being a hash rather than for disagreeing with a file. Then the
/// **state file**, which is the only thing left that can be wrong. The ref
/// is read exactly once, and that one sha is what the job carries and what
/// the file records.
pub fn decide(checkout: &Checkout, watching: &Watching) -> Result<Decision> {
    let Some(held) = hold(watching)? else {
        return Ok(Decision::AnotherRun {
            lock: watching.lock_path(),
        });
    };
    let tip = checkout.tip_of_ref(&watching.watched)?;
    let state = read_state(watching)?;
    if tip.hash == state.commit {
        return Ok(Decision::Unmoved {
            commit: state.commit,
            since: state.recorded_at,
        });
    }
    Ok(Decision::Moved {
        held,
        was: state.commit,
        tip,
    })
}

/// Write down what this run submitted, replacing the last run.
///
/// Called **after** the hub has acknowledged the submission, with the hold
/// still in hand, which is the honest order and has one residual window:
/// if this write fails, a job was submitted and the file does not say so, so
/// the next run will submit the same commit again. That is why the failure
/// is loud and names the file and the job (the caller's business), and why
/// it is this way round: recording first would mean a hub that is down turns
/// into a commit that is silently never built, and a nightly that never runs
/// and says nothing is worse than one that runs twice.
pub fn record(held: &Held, watching: &Watching, commit: &str, job_id: Option<&str>) -> Result<()> {
    // Read, so that the hold is observably alive at the moment of the write
    // and no future edit can drop it earlier.
    let _ = held.file();
    write_state(
        watching,
        &State {
            state_format_version: STATE_FORMAT_VERSION,
            watched: watching.watched.clone(),
            command: watching.command.clone(),
            commit: commit.to_owned(),
            recorded_at: now(),
            job_id: job_id.map(str::to_owned),
        },
        Replacing::Existing,
    )
}

/// Start a state file at the ref's current tip, submitting nothing.
///
/// The deliberate act that answers "how does the first run ever happen?".
/// There is no "treat a missing file as moved" because that is the guess
/// this module exists to refuse, and there is no silent reset: a file that
/// is already there refuses, naming itself.
pub fn begin(checkout: &Checkout, watching: &Watching) -> Result<Commit> {
    let Some(held) = hold(watching)? else {
        bail!(
            "another run holds {}: nothing was written, because two runs starting the same \
             state file would each write a different tip",
            watching.lock_path().display()
        );
    };
    let _ = held.file();
    let tip = checkout.tip_of_ref(&watching.watched)?;
    write_state(
        watching,
        &State {
            state_format_version: STATE_FORMAT_VERSION,
            watched: watching.watched.clone(),
            command: watching.command.clone(),
            commit: tip.hash.clone(),
            recorded_at: now(),
            job_id: None,
        },
        Replacing::Nothing,
    )?;
    Ok(tip)
}

/// Whether a write may replace a state file that is already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Replacing {
    /// A record of a run that happened: the last run is what it replaces.
    Existing,
    /// A first file: one that is already there is refused, so that `--init`
    /// can never be the thing that forgot last night.
    Nothing,
}

/// How many readings of "somebody else has it" it takes to believe it, and
/// how long between them.
///
/// **One reading is not evidence, and that is measured rather than
/// defensive.** A `flock` belongs to the open file description, so a `fork`
/// lends it to the child until the child `exec`s — and this module runs
/// `git` under this very lock, as does the submission after it. In a
/// process that forks on other threads, a single `EWOULDBLOCK` can
/// therefore come out of a window microseconds wide in which nobody held
/// anything: measured on this host at **1 run in 10** of
/// `cargo test -p herdctl --lib moved::`, where ten tests fork `git`
/// continuously, with `/proc/self/fd` showing **no** descriptor on the
/// lock file at the moment of the refusal.
///
/// A real holder holds it across the whole of decide-submit-record, orders
/// of magnitude longer than that window, so waiting half a second before
/// believing "another run" costs a nightly nothing and makes the sentence
/// mean what it says. It weakens nothing: a reading that survives ten
/// attempts over half a second is a run, and nothing here ever ends up
/// taking the lock twice.
const HOLD_ATTEMPTS: u32 = 10;
const HOLD_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// Take the exclusive hold, or learn that somebody else has it.
///
/// `None` is "another run holds it" and never an error: a nightly whose
/// previous run is still going is an ordinary event, and the run that is
/// turned away has done its whole job by turning away.
fn hold(watching: &Watching) -> Result<Option<Held>> {
    let path = watching.lock_path();
    let lock = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| {
            format!(
                "could not open {} to hold the state file: `herdctl if-moved` takes an \
                 exclusive lock beside its state file so that two timers that overlap cannot \
                 submit the same pass twice",
                path.display()
            )
        })?;
    for attempt in 1..=HOLD_ATTEMPTS {
        match lock.try_lock() {
            Ok(()) => return Ok(Some(Held::of(lock))),
            // The same descriptor, asked again: the window this waits out
            // closes at somebody else's `exec`.
            Err(TryLockError::WouldBlock) if attempt < HOLD_ATTEMPTS => {
                std::thread::sleep(HOLD_PAUSE);
            }
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(why)) => {
                return Err(anyhow::Error::new(why).context(format!(
                    "could not lock {}: a state file that cannot be held is never read, \
                     because two runs reading it at once is the one failure this guards",
                    path.display()
                )))
            }
        }
    }
    // Unreachable: the last attempt returns in one arm or the other. Said
    // as "another run" rather than as an `unreachable!`, because a panic in
    // a nightly's first step is the worst of the three answers.
    Ok(None)
}

/// The last run, or a refusal naming the file.
///
/// Every arm here is a refusal rather than a default, which is the whole
/// design: the cheapest wrong answer in this module is "there is no file, so
/// assume it moved".
fn read_state(watching: &Watching) -> Result<State> {
    let path = &watching.state;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => bail!(
            "{} does not exist, and a missing state file is never read as a ref that moved: \
             start one with `herdctl if-moved --init --command {} --ref {} --state {}`, which \
             records where {} stands now and submits nothing",
            path.display(),
            watching.command,
            watching.watched,
            path.display(),
            watching.watched
        ),
        Err(why) => bail!(
            "{} cannot be read ({why}): a state file that cannot be read is never read as a \
             ref that moved, so nothing was submitted",
            path.display()
        ),
    };
    let state: State = serde_json::from_str(&text).map_err(|why| {
        anyhow::anyhow!(
            "{} is not a state file `herdctl if-moved` wrote ({why}): it is never read as a \
             ref that moved. Look at it, and start again with `herdctl if-moved --init` once \
             you have moved it out of the way",
            path.display()
        )
    })?;
    if state.state_format_version != STATE_FORMAT_VERSION {
        bail!(
            "{} was written at state format version {}, and this herdctl writes {}: a file \
             from another version is never read as a ref that moved, because the two versions \
             need not mean the same thing by it. Move it out of the way and start again with \
             `herdctl if-moved --init`",
            path.display(),
            state.state_format_version,
            STATE_FORMAT_VERSION
        );
    }
    if !is_whole_hash(&state.commit) {
        bail!(
            "{} records commit {:?}, which is not forty lowercase hex: the predicate compares \
             hashes, so a file that holds anything else is never read as a ref that moved",
            path.display(),
            state.commit
        );
    }
    if state.watched != watching.watched {
        bail!(
            "{} records ref {:?}, and this run asks about {:?}: one state file answers for one \
             ref, so give each ref its own --state rather than have one of them read as moved \
             every time the other does",
            path.display(),
            state.watched,
            watching.watched
        );
    }
    if state.command != watching.command {
        bail!(
            "{} records command {:?}, and this run submits {:?}: one state file answers for \
             one command, so give each command its own --state rather than have the second \
             command never run",
            path.display(),
            state.command,
            watching.command
        );
    }
    Ok(state)
}

/// Write the state file whole, or leave the one that is there untouched.
///
/// Written beside itself and renamed over, so that a reader never sees half
/// a file: a torn state file is one more way to arrive at "I cannot read
/// this", and the refusal above would then be about a crash of ours.
fn write_state(watching: &Watching, state: &State, replacing: Replacing) -> Result<()> {
    let path = &watching.state;
    if replacing == Replacing::Nothing && path.exists() {
        bail!(
            "{} already exists: `--init` starts a state file and never replaces one, because \
             replacing it is how last night's run gets forgotten. Read it, and move it out of \
             the way yourself if that is what you mean",
            path.display()
        );
    }
    let mut text = serde_json::to_string_pretty(state).context("the state file's own JSON")?;
    text.push('\n');
    let pending = watching.pending_path();
    let mut file =
        File::create(&pending).with_context(|| format!("could not write {}", pending.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("could not write {}", pending.display()))?;
    // Durable before it is visible: a machine that loses power between the
    // two must find either the last run or this one, never a file of zeroes
    // where a commit was.
    file.sync_all()
        .with_context(|| format!("could not flush {}", pending.display()))?;
    drop(file);
    std::fs::rename(&pending, path).with_context(|| {
        format!(
            "could not put {} in place as {}",
            pending.display(),
            path.display()
        )
    })?;
    Ok(())
}

/// Now, RFC 3339, as the verdict document spells a time.
fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_owned())
}

/// Exactly forty lowercase hex characters, and nothing else.
fn is_whole_hash(hash: &str) -> bool {
    hash.len() == 40
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Output, Stdio};
    use tempfile::TempDir;

    /// A real git repository with two commits on `main` and a tag, built
    /// here: every state these tests assert on is one the fixture put there
    /// with real `git init` and `git commit`, so "the ref moved" means a ref
    /// that really moved (D100, D101).
    struct Fixture {
        dir: TempDir,
    }

    impl Fixture {
        /// `None` means git is missing and the test has skipped loudly.
        fn new(test: &str) -> Option<Fixture> {
            if !git_is_here() {
                // D27: an assertion that could not run must be impossible to
                // mistake for one that passed.
                herd_runner::testing::skipped(
                    test,
                    "git is not on this host's PATH, so nothing about a ref having moved \
                     could be read",
                );
                return None;
            }
            let dir = TempDir::new().expect("a temporary directory for the fixture");
            for made in ["home", "work"] {
                std::fs::create_dir_all(dir.path().join(made)).expect("the fixture's directories");
            }
            let fixture = Fixture { dir };
            fixture.run(&["init", "-q", "--initial-branch=main", "."]);
            fixture.write("README.md", "what the predicate reads\n");
            fixture.run(&["add", "-A"]);
            fixture.run(&["commit", "-qm", "the first commit"]);
            Some(fixture)
        }

        fn work(&self) -> PathBuf {
            self.dir.path().join("work")
        }

        fn checkout(&self) -> Checkout {
            Checkout::found_from(&self.work()).expect("the fixture's own checkout")
        }

        /// One more commit on `main`, which is how the ref moves.
        fn commit(&self, what: &str) -> String {
            self.write("README.md", what);
            self.run(&["add", "-A"]);
            self.run(&["commit", "-qm", what]);
            self.said(&["rev-parse", "HEAD"])
        }

        fn write(&self, rel: &str, text: &str) {
            std::fs::write(self.work().join(rel), text).expect("the fixture writes a file");
        }

        /// One git command in the fixture, with a config of its own: the
        /// host's `~/.gitconfig` and `/etc/gitconfig` are out of the way, so
        /// these tests say the same thing on every machine.
        fn run(&self, args: &[&str]) -> Output {
            let output = Command::new("git")
                .arg("-C")
                .arg(self.work())
                .args(args)
                .env_clear()
                .env(
                    "PATH",
                    std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
                )
                .env("HOME", self.dir.path().join("home"))
                .env("LC_ALL", "C")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Herd Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@herd.invalid")
                .env("GIT_COMMITTER_NAME", "Herd Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@herd.invalid")
                .stdin(Stdio::null())
                .output()
                .expect("git runs in the fixture");
            assert!(
                output.status.success(),
                "the fixture's own git failed: git {}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            output
        }

        /// What the fixture's own git says, trimmed: the independent reading
        /// a test compares the module against.
        fn said(&self, args: &[&str]) -> String {
            String::from_utf8_lossy(&self.run(args).stdout)
                .trim()
                .to_owned()
        }

        /// What this fixture watches, with its state file beside the
        /// repository rather than in it.
        fn watching(&self) -> Watching {
            Watching {
                state: self.dir.path().join("nightly.json"),
                watched: "refs/heads/main".to_owned(),
                command: "nightly".to_owned(),
            }
        }
    }

    fn git_is_here() -> bool {
        match Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// The refusal, as a reader would see it.
    ///
    /// The accepted case prints **what** was accepted, because the three
    /// decisions are not interchangeable: a test that expected a refusal and
    /// got `AnotherRun` has a lock problem, and one that got `Moved` has a
    /// predicate problem, and a bare "it was accepted" sends the reader to
    /// the wrong one of the two.
    fn refusal<T: std::fmt::Debug>(result: Result<T>) -> String {
        match result {
            Ok(what) => panic!("this was meant to be refused and was accepted: {what:?}"),
            Err(why) => format!("{why:#}"),
        }
    }

    /// The whole of the happy path, and the word "once" in it: the second
    /// run of the same state file, with nothing having moved in between,
    /// submits nothing.
    #[test]
    fn a_ref_that_moved_is_submitted_once_and_not_again() {
        let Some(fixture) = Fixture::new("a_ref_that_moved_is_submitted_once_and_not_again") else {
            return;
        };
        let watching = fixture.watching();
        let first = fixture.said(&["rev-parse", "HEAD"]);
        let begun = begin(&fixture.checkout(), &watching).expect("--init records the tip");
        assert_eq!(begun.hash, first);

        // Nothing has moved yet.
        match decide(&fixture.checkout(), &watching).expect("the predicate answers") {
            Decision::Unmoved { commit, .. } => assert_eq!(commit, first),
            other => panic!("a ref that has not moved decided {other:?}"),
        }

        let second = fixture.commit("the ref moves\n");
        assert_ne!(second, first);
        let held = match decide(&fixture.checkout(), &watching).expect("the predicate answers") {
            Decision::Moved { held, was, tip } => {
                assert_eq!(was, first, "it must say where the ref was");
                assert_eq!(tip.hash, second, "and the tip it read is the job's commit");
                held
            }
            other => panic!("a ref that moved decided {other:?}"),
        };
        record(&held, &watching, &second, Some("job-7")).expect("the run is recorded");
        drop(held);

        // And once means once: the same tip, a second run, no submission.
        match decide(&fixture.checkout(), &watching).expect("the predicate answers") {
            Decision::Unmoved { commit, .. } => assert_eq!(commit, second),
            other => panic!("a recorded run decided {other:?}"),
        }
        let written: State =
            serde_json::from_str(&std::fs::read_to_string(&watching.state).expect("the file"))
                .expect("the state file is JSON this module reads");
        assert_eq!(written.commit, second);
        assert_eq!(written.job_id.as_deref(), Some("job-7"));
        assert_eq!(written.state_format_version, STATE_FORMAT_VERSION);
    }

    /// The failure E-41's nightly must never have: two runs, one state file,
    /// and the pass submitted twice. The second run is turned away while the
    /// first still holds the file, and it is turned away **before** it can
    /// decide that the ref moved.
    #[test]
    fn two_runs_racing_one_state_file_do_not_both_submit() {
        let Some(fixture) = Fixture::new("two_runs_racing_one_state_file_do_not_both_submit")
        else {
            return;
        };
        let watching = fixture.watching();
        begin(&fixture.checkout(), &watching).expect("--init records the tip");
        fixture.commit("the ref moves while two timers overlap\n");

        // The first run holds the file, as it does across a submission.
        let first = decide(&fixture.checkout(), &watching).expect("the first run answers");
        assert!(
            matches!(first, Decision::Moved { .. }),
            "the first run must see the move: {first:?}"
        );
        let second = decide(&fixture.checkout(), &watching).expect("the second run answers");
        match second {
            Decision::AnotherRun { lock } => assert_eq!(lock, watching.lock_path()),
            other => panic!("the second run of an overlapping pair decided {other:?}"),
        }

        // And once the first has recorded and let go, the second run of the
        // night is an ordinary skip rather than a second submission.
        let Decision::Moved { held, tip, .. } = first else {
            unreachable!("asserted above");
        };
        record(&held, &watching, &tip.hash, Some("job-1")).expect("the first run is recorded");
        drop(held);
        match decide(&fixture.checkout(), &watching).expect("the predicate answers") {
            Decision::Unmoved { commit, .. } => assert_eq!(commit, tip.hash),
            other => panic!("the run after a recorded one decided {other:?}"),
        }
    }

    /// Two threads through the same door, which is the shape two systemd
    /// timers have: exactly one of them is let through, and the one that is
    /// holds the file until the other has asked.
    ///
    /// Deterministic, and that is what the second barrier is for: whichever
    /// thread takes the lock waits there **holding** it, so the other one
    /// decides while it is held. Without it, a thread that let go early
    /// would hand the lock straight to the second one and both would read a
    /// move -- the double submission, arrived at by the test rather than by
    /// the code.
    #[test]
    fn only_one_of_two_concurrent_runs_is_let_through() {
        let Some(fixture) = Fixture::new("only_one_of_two_concurrent_runs_is_let_through") else {
            return;
        };
        let watching = fixture.watching();
        begin(&fixture.checkout(), &watching).expect("--init records the tip");
        fixture.commit("the ref moves under two threads\n");

        let start = std::sync::Barrier::new(2);
        let decided = std::sync::Barrier::new(2);
        let moved = std::sync::atomic::AtomicUsize::new(0);
        let turned_away = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    let checkout = fixture.checkout();
                    start.wait();
                    let decision = decide(&checkout, &watching).expect("each run answers");
                    match &decision {
                        Decision::Moved { .. } => {
                            moved.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Decision::AnotherRun { .. } => {
                            turned_away.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Decision::Unmoved { .. } => {}
                    }
                    // The hold lives until both have asked, which is what a
                    // run does with it across a submission.
                    decided.wait();
                    drop(decision);
                });
            }
        });
        assert_eq!(
            moved.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "exactly one of two overlapping runs may submit: a second one is the double \
             nightly this lock exists to prevent"
        );
        assert_eq!(
            turned_away.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "and the run that was not let through must have been told so"
        );
    }

    #[test]
    fn a_missing_state_file_refuses_and_names_itself() {
        let Some(fixture) = Fixture::new("a_missing_state_file_refuses_and_names_itself") else {
            return;
        };
        let watching = fixture.watching();
        let why = refusal(decide(&fixture.checkout(), &watching));
        assert!(
            why.contains(&watching.state.display().to_string()),
            "the refusal must name the file: {why}"
        );
        assert!(why.contains("--init"), "and say how to start one: {why}");
        assert!(
            !watching.state.exists(),
            "a refusal writes no state file of its own"
        );
    }

    #[test]
    fn an_unreadable_state_file_refuses_and_names_itself() {
        let Some(fixture) = Fixture::new("an_unreadable_state_file_refuses_and_names_itself")
        else {
            return;
        };
        let watching = fixture.watching();
        std::fs::write(&watching.state, "not json at all\n").expect("the fixture writes it");
        let why = refusal(decide(&fixture.checkout(), &watching));
        assert!(
            why.contains(&watching.state.display().to_string()),
            "the refusal must name the file: {why}"
        );
    }

    #[test]
    fn a_state_file_from_another_version_refuses_and_names_both() {
        let Some(fixture) =
            Fixture::new("a_state_file_from_another_version_refuses_and_names_both")
        else {
            return;
        };
        let watching = fixture.watching();
        let older = STATE_FORMAT_VERSION - 1;
        std::fs::write(
            &watching.state,
            format!(
                r#"{{"state_format_version":{older},"ref":"refs/heads/main",
                "command":"nightly","commit":"{}","recorded_at":"2026-10-28T03:00:00Z",
                "job_id":null}}"#,
                "0".repeat(40)
            ),
        )
        .expect("the fixture writes it");
        let why = refusal(decide(&fixture.checkout(), &watching));
        assert!(
            why.contains(&watching.state.display().to_string())
                && why.contains(&format!("version {older}"))
                && why.contains(&STATE_FORMAT_VERSION.to_string()),
            "the refusal must name the file and both versions: {why}"
        );
    }

    #[test]
    fn a_state_file_kept_for_another_ref_or_command_refuses() {
        let Some(fixture) = Fixture::new("a_state_file_kept_for_another_ref_or_command_refuses")
        else {
            return;
        };
        let watching = fixture.watching();
        begin(&fixture.checkout(), &watching).expect("--init records the tip");
        // A ref that really exists, so the **only** thing wrong below is
        // that the file answers for another one: a second ref the fixture
        // had not created would be refused for not existing, and the test
        // would pass on a premise it had not established (D100).
        fixture.run(&["branch", "release"]);

        let elsewhere = Watching {
            watched: "refs/heads/release".to_owned(),
            ..watching.clone()
        };
        let why = refusal(decide(&fixture.checkout(), &elsewhere));
        assert!(
            why.contains("refs/heads/main") && why.contains("refs/heads/release"),
            "the refusal must name both refs: {why}"
        );

        let other = Watching {
            command: "produce".to_owned(),
            ..watching.clone()
        };
        let why = refusal(decide(&fixture.checkout(), &other));
        assert!(
            why.contains("nightly") && why.contains("produce"),
            "the refusal must name both commands: {why}"
        );
    }

    /// A ref that does not exist has not moved, and is never read as having
    /// moved.
    #[test]
    fn a_ref_that_does_not_exist_is_refused_by_name() {
        let Some(fixture) = Fixture::new("a_ref_that_does_not_exist_is_refused_by_name") else {
            return;
        };
        let watching = Watching {
            watched: "refs/heads/nowhere".to_owned(),
            ..fixture.watching()
        };
        // The state file is there, so the only thing wrong is the ref.
        std::fs::write(
            &watching.state,
            serde_json::to_string(&State {
                state_format_version: STATE_FORMAT_VERSION,
                watched: watching.watched.clone(),
                command: watching.command.clone(),
                commit: "0".repeat(40),
                recorded_at: "2026-10-28T03:00:00Z".to_owned(),
                job_id: None,
            })
            .expect("the fixture's own JSON"),
        )
        .expect("the fixture writes it");
        let why = refusal(decide(&fixture.checkout(), &watching));
        assert!(
            why.contains("refs/heads/nowhere"),
            "the refusal must name the ref: {why}"
        );
        assert!(
            why.contains("has not moved") || why.contains("not a ref"),
            "and say that a ref which is not there has not moved: {why}"
        );
    }

    /// A hash names one commit for ever, so there is nothing for the
    /// predicate to compare: refused, rather than submitted every night.
    #[test]
    fn a_commit_hash_is_not_a_ref_that_can_move() {
        let Some(fixture) = Fixture::new("a_commit_hash_is_not_a_ref_that_can_move") else {
            return;
        };
        let hash = fixture.said(&["rev-parse", "HEAD"]);
        let why = refusal(fixture.checkout().tip_of_ref(&hash));
        assert!(
            why.contains(&hash) && why.contains("not a ref"),
            "the refusal must name it and say what it is: {why}"
        );
    }

    #[test]
    fn init_starts_a_file_and_never_replaces_one() {
        let Some(fixture) = Fixture::new("init_starts_a_file_and_never_replaces_one") else {
            return;
        };
        let watching = fixture.watching();
        begin(&fixture.checkout(), &watching).expect("--init records the tip");
        let first = std::fs::read_to_string(&watching.state).expect("the file");
        let why = refusal(begin(&fixture.checkout(), &watching));
        assert!(
            why.contains(&watching.state.display().to_string()) && why.contains("already exists"),
            "a second --init must refuse, naming the file: {why}"
        );
        assert_eq!(
            std::fs::read_to_string(&watching.state).expect("the file"),
            first,
            "and must not have touched it"
        );
    }
}
