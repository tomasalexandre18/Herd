//! The tests that define M4: a real hub, a real runner, real commands, and
//! the real kernel (invariant 5).
//!
//! ## Skipping, and why it is loud
//!
//! Some assertions here need a capability the host may not have — cgroup v2
//! delegation above all, which a GitHub Actions runner does not give. An
//! assertion that cannot run is **skipped with its reason printed**, never
//! quietly passed and never failed: a red suite on a host that is merely
//! different teaches people to ignore the suite. Every skip prints a line
//! beginning `HERD-SKIPPED:` **and appends it to `$HERD_SKIP_LOG`**, because
//! `cargo test` hides the stdout of a passing test and a skip nobody can see
//! is a skip that reads as a pass. `scripts/test.sh` prints the log as a
//! summary, and `scripts/report-skips.sh` turns each line into a warning on
//! the CI run. So the difference between "asserted" and "not asserted here"
//! is visible from the terminal and on the pull request.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::{Hub, JobDirStore, JobStore};
use herd_proto::{Cap, JobOutcome, LogStream};
use herd_runner::metrics::DEFAULT_TMPFS_PATH;
use herd_runner::testing::TempDir;
use herd_runner::{Confined, Confinement};
use tokio::task::JoinHandle;

/// A hub with one runner joined, confined as the test asks.
struct Harness {
    hub_url: String,
    data_root: PathBuf,
    /// The data root when this harness made its own, removed with it.
    /// `None` when the test handed one in: the one test whose data root has
    /// to sit inside a directory of its own keeps that directory itself
    /// ([`TheOnlyWayIn`]).
    _own_root: Option<TempDir>,
    /// The hub's state root, which M2 made files: one per harness, removed
    /// with it, so no confinement test reads another's records.
    _state: TempDir,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    async fn new(label: &str, shape: impl FnOnce(&mut Confinement)) -> Self {
        let own = TempDir::new(label);
        let data_root = own.path().to_path_buf();
        Self::rooted_at(data_root, Some(own), label, shape).await
    }

    /// The same harness with its data root **given** rather than made: a
    /// directory the test owns, so that what encloses the data root can be
    /// part of what the test asserts ([`TheOnlyWayIn`]).
    async fn rooted_at(
        data_root: PathBuf,
        own_root: Option<TempDir>,
        label: &str,
        shape: impl FnOnce(&mut Confinement),
    ) -> Self {
        let state = TempDir::new(&format!("{label}-state"));
        let mut confinement = test_confinement(&data_root);
        shape(&mut confinement);
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
        // An unapproved runner claims nothing (E-48), so this harness
        // approves the one runner it starts and hands it the token that
        // approval minted.
        let token = store
            .approve_runner(&format!("m4-{label}"))
            .expect("the state root mints a token");
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            store as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        let running = tokio::spawn({
            let config = herd_runner::Config {
                hub_url: hub_url.clone(),
                name: Some(format!("m4-{label}")),
                token: Some(token),
                confinement,
                ..herd_runner::Config::default()
            };
            async move {
                if let Err(error) = herd_runner::run(config).await {
                    eprintln!("herd-runner (test): {error:#}");
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Self {
            hub_url,
            data_root,
            _own_root: own_root,
            _state: state,
            tasks: vec![serving, running],
        }
    }

    fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: None,
            name: Some("m4-requester".to_owned()),
        }
    }

    /// Run one shell script as a job and collect everything about its end.
    async fn run(&self, script: &str) -> Ran {
        let mut out = String::new();
        let mut err = String::new();
        // Everything Herd said about this job while it was happening. Kept
        // because a job *delayed* is a job the hub explained a wait for, and
        // an assertion that nothing was delayed has to be able to look.
        let mut notes = Vec::new();
        let finished = herdctl::run_job(
            &self.requester(),
            shell(script),
            // No capability in particular: M4's caps are numbers a runner
            // enforces, never labels (M5 keeps them apart).
            herd_proto::LabelSet::default(),
            herd_proto::Priority::Step,
            herdctl::Declared::default(),
            |stream, data| match stream {
                LogStream::Stdout => out.push_str(&String::from_utf8_lossy(data)),
                LogStream::Stderr => err.push_str(&String::from_utf8_lossy(data)),
            },
            |said| {
                if let herdctl::Said::Note(note) = said {
                    notes.push(note.to_owned());
                }
            },
        )
        .await
        .expect("the hub answers with an outcome");
        Ran {
            outcome: finished.outcome,
            metrics: finished.metrics,
            out,
            err,
            notes,
        }
    }

    fn root(&self) -> &Path {
        &self.data_root
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

struct Ran {
    outcome: JobOutcome,
    metrics: herd_proto::JobMetrics,
    out: String,
    err: String,
    /// What Herd said about this job, in order. Every wait the hub makes a
    /// job serve is explained in one of these (E-04).
    notes: Vec<String>,
}

fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// Caps a test can work with: generous enough that nothing trips by
/// accident, and each one overridden by the test that is about it.
fn test_confinement(data_root: &Path) -> Confinement {
    let mut confinement = Confinement::with_data_root(data_root);
    confinement.timeout = Duration::from_secs(60);
    confinement.memory_max = 512 << 20;
    confinement.log_cap = 4 << 20;
    confinement.disk_budget = 64 << 20;
    // A test host may itself be a delegated cgroup scope; where it is not,
    // every ceiling assertion below skips with that as its reason.
    confinement.cgroup_root = std::env::var_os("HERD_TEST_CGROUP_ROOT").map(PathBuf::from);
    confinement
}

/// Whether this host enforces the real kernel ceiling, and why not if it
/// does not. Asked by preparing a confinement exactly as a runner does, in a
/// data root of its own: a root belongs to one runner at a time, so this must
/// never borrow the root a harness is using.
fn kernel_ceiling_or_reason() -> Result<(), String> {
    let temp = TempDir::new("ceiling-question");
    let confined = Confined::prepare(test_confinement(temp.path()))
        .expect("an unprivileged process with a writable root can confine");
    match confined.ceiling().fallback_reason() {
        None => Ok(()),
        Some(why) => Err(why.to_owned()),
    }
}

/// Print a skip so that nobody can mistake it for an assertion that ran, and
/// write it down where `cargo test` cannot hide it: `scripts/test.sh` prints
/// the file's contents as a summary, and `scripts/report-skips.sh` turns each
/// line into a warning in CI.
fn skipped(test: &str, why: &str) {
    herd_runner::testing::skipped(test, why);
}

// ---------------------------------------------------------------- E-45 ----

/// Everything a job is given to write in is inside the one data root: its
/// working directory, its `HOME`, its `TMPDIR`. The job itself writes to each
/// of them, so the assertion is that the writes worked *and* that the places
/// they worked in are inside the root.
#[tokio::test]
async fn a_job_works_and_writes_only_inside_the_data_root() {
    let harness = Harness::new("inside-root", |_| {}).await;
    let ran = harness
        .run(
            "pwd; echo \"$HOME\"; echo \"$TMPDIR\"; \
             echo work > ./in-the-workspace && \
             echo home > \"$HOME/in-the-home\" && \
             echo tmp > \"$TMPDIR/in-the-tmp\"",
        )
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the job could not write where it was told to: {}",
        ran.err
    );
    // The paths are compared as they were printed: the workspace is already
    // gone by the time the outcome arrives, which is itself the point of the
    // next test. `DataRoot` canonicalises the root, so what the job printed
    // is canonical too and a prefix comparison is exact.
    let root = harness.root().canonicalize().expect("the root resolves");
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines.len(), 3, "expected three paths, got {:?}", ran.out);
    for (what, line) in ["working directory", "HOME", "TMPDIR"]
        .into_iter()
        .zip(lines)
    {
        assert!(
            Path::new(line).starts_with(&root),
            "the job's {what} is outside the data root: {line} is not under {}",
            root.display()
        );
    }
}

/// A job whose place inside the data root cannot be made fails by name,
/// carrying the path — never silently running somewhere else instead. This is
/// E-45's refusal seen from the job's side: the runner started fine, and the
/// root stopped being usable afterwards, which is the case a startup check
/// cannot catch.
#[tokio::test]
async fn a_job_that_cannot_be_given_a_place_in_the_root_fails_naming_the_path() {
    let harness = Harness::new("unusable-root", |_| {}).await;
    let jobs = harness.root().join("jobs");
    // Read and execute, but not write: the runner can look inside and cannot
    // make a workspace there.
    set_mode(&jobs, 0o500);
    let ran = harness.run("echo this should never run").await;
    set_mode(&jobs, 0o700);

    let JobOutcome::CouldNotStart { reason } = &ran.outcome else {
        panic!(
            "a job with nowhere to run is not a job that ran: {:?}",
            ran.outcome
        );
    };
    assert!(
        reason.contains(&jobs.display().to_string()),
        "the reason must name the path inside the data root: {reason}"
    );
    assert_eq!(ran.out, "", "a job that never started wrote nothing");
}

/// The startup refusal: a data root the runner cannot use stops it before it
/// ever joins a hub, and the refusal names the path.
#[tokio::test]
async fn a_runner_with_a_bad_data_root_refuses_to_start_naming_it() {
    let temp = TempDir::new("bad-root");
    let file = temp.path().join("this-is-a-file");
    std::fs::write(&file, b"not a directory").expect("the fixture writes");
    let error = herd_runner::run(herd_runner::Config {
        // Never reached: confinement is checked before the hub is dialled.
        hub_url: "ws://127.0.0.1:1".to_owned(),
        name: None,
        token: None,
        confinement: Confinement::with_data_root(&file),
        ..herd_runner::Config::default()
    })
    .await
    .expect_err("a file is not a data root");
    let said = format!("{error:#}");
    assert!(
        said.contains(&file.display().to_string()),
        "the refusal must name the path: {said}"
    );
    assert!(
        said.contains("cannot confine"),
        "the refusal must say what it is about: {said}"
    );
}

/// The workspace is the job's, and it goes away with the job: a runner that
/// kept every job's tree would fill its host's disk, which is the thing
/// invariant 2 forbids above all.
#[tokio::test]
async fn a_job_s_workspace_is_gone_once_the_job_has_ended() {
    let harness = Harness::new("workspace-gone", |_| {}).await;
    let ran = harness
        .run("dd if=/dev/zero of=./big bs=1024 count=2048 2>/dev/null")
        .await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    let jobs = harness.root().join("jobs");
    let left: Vec<PathBuf> = std::fs::read_dir(&jobs)
        .expect("the jobs directory is readable")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(
        left.is_empty(),
        "the job left its workspace behind: {left:?}"
    );
}

/// Nothing of the runner's own environment reaches a job: not its token, not
/// its `TMPDIR`, not anything its owner happened to export.
#[tokio::test]
async fn a_job_inherits_nothing_from_the_runner_s_environment() {
    std::env::set_var("HERD_M4_SECRET", "this must not reach a job");
    let harness = Harness::new("clean-env", |_| {}).await;
    let ran = harness.run("echo \"[${HERD_M4_SECRET-unset}]\"").await;
    std::env::remove_var("HERD_M4_SECRET");
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(
        ran.out.trim(),
        "[unset]",
        "the runner's environment leaked into the job: {:?}",
        ran.out
    );
}

// ---------------------------------------------------------------- E-34 ----

/// A job is a guest on someone's machine, so it runs niced. Field 19 of
/// `/proc/<pid>/stat` is the nice value, read by the job about itself.
///
/// The configured niceness is a floor rather than a value: an unprivileged
/// process cannot lower its own niceness, so a runner its owner started at
/// nice 15 keeps its jobs at 15 rather than pulling them up to 7. The test
/// therefore compares against the runner's own niceness, which is also what
/// makes it pass on a build machine that is itself niced.
///
/// IO niceness is set in the same breath (the idle class) and is deliberately
/// not asserted: Linux exposes a process's IO priority through no file, only
/// through `ioprio_get`, and a test that shelled out to `ionice` would be
/// asserting that `ionice` is installed.
#[tokio::test]
async fn a_job_runs_niced() {
    let harness = Harness::new("niced", |confinement| confinement.nice = 7).await;
    let ran = harness.run("cut -d' ' -f19 /proc/self/stat").await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.err);
    let job_nice: i32 = ran
        .out
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("nice is not a number ({error}): {:?}", ran.out));
    let expected = own_nice().max(7);
    assert_eq!(
        job_nice,
        expected,
        "the job runs at nice {job_nice}; this runner is at nice {} and asked for 7",
        own_nice()
    );
    assert!(job_nice >= 7, "a job must never run less nice than asked");
}

/// A job that leaves a background child behind must not outlive its own
/// death. The job prints the grandchild's pid and exits straight away; the
/// grandchild would otherwise sit there for five minutes.
#[tokio::test]
async fn a_background_child_does_not_outlive_the_job_that_spawned_it() {
    let harness = Harness::new("grandchild", |_| {}).await;
    let ran = harness.run("sleep 300 & echo $!; exit 0").await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.err);
    let pid: u32 = ran
        .out
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("the job printed no pid ({error}): {:?}", ran.out));

    // The outcome is reported after the kill, so by now the grandchild is
    // either gone or was never ours.
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_a_live_sleep(pid) {
        assert!(
            Instant::now() < deadline,
            "the grandchild {pid} outlived the job that spawned it"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A job past its time cap ends by name. Never a hang: a runner that lets a
/// wedged job sit forever has given its host away.
#[tokio::test]
async fn a_job_past_its_time_cap_is_stopped_by_name() {
    let harness = Harness::new("time-cap", |confinement| {
        confinement.timeout = Duration::from_secs(1);
    })
    .await;
    let started = Instant::now();
    let ran = harness.run("printf 'starting\\n'; sleep 300").await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Time,
            limit: 1,
            detail: None
        },
        "a job past its time cap must name the cap: {:?}",
        ran.outcome
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the cap took {:?} to end a 1 s job",
        started.elapsed()
    );
    assert_eq!(
        ran.out, "starting\n",
        "what the job printed before being stopped still reaches the requester"
    );
}

/// A job that floods its log is stopped at the cap, and the log the hub kept
/// is exactly as long as the cap says.
#[tokio::test]
async fn a_job_flooding_its_log_is_stopped_at_the_log_cap() {
    let cap = 64 * 1024;
    let harness = Harness::new("log-cap", |confinement| confinement.log_cap = cap).await;
    let ran = harness
        .run("while :; do dd if=/dev/zero bs=4096 count=256 2>/dev/null | tr '\\0' 'x'; done")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Log,
            limit: cap,
            detail: None
        },
        "a flooding job must name the log cap: {:?}",
        ran.outcome
    );
    let streamed = ran.out.len() + ran.err.len();
    assert!(
        streamed as u64 <= cap,
        "{streamed} bytes reached the requester with a cap of {cap}"
    );
    assert!(
        streamed as u64 > cap / 2,
        "only {streamed} bytes of a {cap} cap arrived, so the cap tripped on nothing"
    );
}

/// A job that fills its workspace is stopped at its disk budget, with the
/// figure in the outcome.
#[tokio::test]
async fn a_job_filling_its_workspace_is_stopped_at_its_disk_budget() {
    let budget = 8 << 20;
    let harness = Harness::new("disk-budget", |confinement| {
        confinement.disk_budget = budget;
    })
    .await;
    let ran = harness
        .run("i=0; while :; do dd if=/dev/zero of=./fill-$i bs=1048576 count=4 2>/dev/null; i=$((i+1)); done")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Disk,
            limit: budget,
            detail: None
        },
        "a job filling its workspace must name the disk budget: {:?}",
        ran.outcome
    );
    let written = ran
        .metrics
        .bytes_written
        .expect("the workspace is always measurable");
    assert!(
        written > budget,
        "the budget tripped at {written} bytes, which is not past {budget}"
    );
}

/// What the job prints once it has made its own hiding place unreadable to
/// itself, and what it prints when it could not.
///
/// The job writes **nothing** in the second case and exits 9. A run whose
/// premise was already gone before the bytes existed is void, and a void run
/// has no reason to put a hundred megabytes on anybody's disk.
const HIDDEN_FROM_THE_JOB_ITSELF: &str = "this job cannot read its own hiding place";
const STILL_READABLE_BY_THE_JOB: &str = "this job can still read its own hiding place";

/// **The one way in**: the directory this test's data root sits in, kept as
/// the witness that tells *this* runner's restoration apart from somebody
/// else's.
///
/// ## Why a witness is needed at all
///
/// This suite runs inside a Herd job (D258), and the runner carrying that
/// job walks everything under the job's base every
/// `herd_runner::confine::POLL_INTERVAL` — 200 ms — to hold it to its own disk
/// budget. `$TMPDIR` is inside that base, so every temporary directory this
/// file makes is inside the walked tree, and a directory that walk cannot
/// list has access handed back to it by `herd_runner::root`'s
/// `read_dir_restoring` → `restore_access`: `set_permissions(mode | 0o700)`.
///
/// That is the enclosing runner doing exactly what invariant 2 and D166
/// require, so the fixture is the side that has to be honest about it. Until
/// 2026-10-30 this test wrote its bytes in the open, ran `chmod 300 h` and
/// then `sleep 6`: a six-second premise against a 200 ms restorer. The
/// hiding place was readable again long before the runner under test
/// measured it, the cap tripped on bytes in plain view, and the assertion
/// held either way — it would have held with `restore_access` deleted from
/// the product, which is the one thing it exists to catch. `0o700` where the
/// job set `0o300` is the fingerprint.
///
/// ## What this directory proves
///
/// The data root lives inside it, so **the only path from the host to the
/// job's hiding place runs through here**. And `read_dir_restoring` restores
/// the **ancestors first** — `restore_ancestors`, every directory up to the
/// walking runner's own data root — and only then the directory it failed
/// on. The enclosing runner's data root is above `$TMPDIR`, so this
/// directory is one of those ancestors: nothing outside can give the job's
/// hiding place its access back without giving this directory its access
/// back in the same call.
///
/// The runner under test never touches it: `restore_ancestors` stops at
/// `up_to`, which is the data root *inside* here.
///
/// So the mode of this one directory, read after the job has ended, says
/// whether the measurement that judged the job was taken on the premise the
/// test is named for. A measurement taken on a premise that did not hold is
/// void and taken again from nothing, [`Self::ATTEMPTS`] times, and the last
/// void is **red** with [`Self::WOULD_NOT_HOLD`] — red and not skipped,
/// because a skip would move the suite's skip list, and that list is
/// compared and not counted (D27, D187).
struct TheOnlyWayIn {
    enclosure: TempDir,
}

/// What the enclosure's mode says about the attempt that just ran. Three
/// modes, three different facts, which is why the witness is a mode and not
/// a boolean.
#[derive(Debug, PartialEq, Eq)]
enum WhatTheWayInSays {
    /// `0o300`: the job blinded it and nothing outside this runner has
    /// opened it since, so the runner under test is what made the hidden
    /// bytes measurable. This is the only state an assertion is judged on.
    ThePremiseHeld,
    /// `0o700`, which is `0o300 | 0o700` and the fingerprint of
    /// `restore_access`: an enclosing walk came through, so it restored the
    /// job's hiding place in the same call, and this measurement is evidence
    /// about that walk and none about this runner.
    SomethingOutsideOpenedIt,
    /// `0o755`, the mode this fixture sets and nothing else leaves: the job
    /// never reached its own `chmod`, so there was no premise to measure
    /// against — the disk cap stopped it before it had one, or its `mkdir`
    /// failed.
    TheJobNeverBlindedIt,
    /// Moded `0o300` and listing anyway: a process file permissions do not
    /// apply to, where no mode hides anything from anybody and this
    /// assertion cannot be made at all.
    APermissionThatDoesNotApply,
}

impl TheOnlyWayIn {
    /// The mode this fixture sets, the one the job sets, and the one a
    /// restoration leaves. Three values that cannot be each other, which is
    /// the whole mechanism: `0o755` is set here and never produced by
    /// `mode | 0o700`, and the umask is given no say in it.
    const AS_MADE: u32 = 0o755;
    const BLINDED: u32 = 0o300;
    const RESTORED: u32 = Self::BLINDED | 0o700;

    /// How many measurements are taken before this host is called one that
    /// will not hold the premise at all.
    ///
    /// Eight, against a window of a few hundred milliseconds and a walk that
    /// comes past about as often: a single collision is ordinary and eight
    /// in a row is not a number this project waits for. The bound exists so
    /// that a host whose enclosing walk opens everything faster than a job
    /// can hide anything is red with a sentence rather than looping.
    const ATTEMPTS: usize = 8;

    /// What this test says instead of passing, when every attempt was undone.
    const WOULD_NOT_HOLD: &'static str =
        "every attempt at this test's premise was undone before the runner under test could \
         be judged on it, so nothing here is evidence about a job that hides what it wrote. \
         See TheOnlyWayIn: the runner carrying this suite restores access on any directory \
         under the job it cannot list, every 200 ms and its ancestors first, and 0o700 where \
         the job set 0o300 is the fingerprint. A job that never blinded it at all is a disk \
         cap that stopped the job before its premise existed; an enclosure moded 0o300 that \
         lists anyway is a process file permissions do not apply to, and then no mode hides \
         anything here";

    /// A fresh enclosure, at the mode this fixture made it with.
    fn around(label: &str) -> Self {
        let it = Self {
            enclosure: TempDir::new(&format!("{label}-enclosure")),
        };
        it.hold_again();
        it
    }

    /// Where the harness's data root goes: inside, so that the only path
    /// from the host to the job's hiding place runs through this directory.
    fn data_root(&self) -> PathBuf {
        let root = self.enclosure.path().join("root");
        std::fs::create_dir_all(&root).expect("a data root inside the enclosure");
        root
    }

    fn path(&self) -> &Path {
        self.enclosure.path()
    }

    /// What the enclosure says, asked at the moment a measurement is judged
    /// and never assumed from the job having run.
    fn asked(&self) -> WhatTheWayInSays {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::symlink_metadata(self.path())
            .expect("the enclosure is this test's own directory")
            .permissions()
            .mode()
            & 0o777;
        match mode {
            Self::BLINDED => match std::fs::read_dir(self.path()) {
                Ok(_) => WhatTheWayInSays::APermissionThatDoesNotApply,
                // It fails *because of the mode*, which is the premise. Any
                // other error is a listing that failed for a reason nobody
                // here arranged — a descriptor limit, a tree that moved —
                // and reading it as the premise would put a green on
                // something nobody built.
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    WhatTheWayInSays::ThePremiseHeld
                }
                Err(error) => panic!(
                    "{} would not list for a reason this fixture did not arrange: {error:?}",
                    self.path().display()
                ),
            },
            Self::RESTORED => WhatTheWayInSays::SomethingOutsideOpenedIt,
            Self::AS_MADE => WhatTheWayInSays::TheJobNeverBlindedIt,
            other => panic!(
                "{} is moded {other:o}, which is neither this fixture's mode, nor the job's, \
                 nor a restoration's",
                self.path().display()
            ),
        }
    }

    /// Put the enclosure back to the mode it was made with, before the next
    /// attempt.
    ///
    /// Not a retry of an assertion: the *job* is what blinds this directory,
    /// and this is what keeps "the job never blinded it" a state of its own
    /// rather than one that reads as a restoration.
    fn hold_again(&self) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(self.path(), std::fs::Permissions::from_mode(Self::AS_MADE))
            .expect("the enclosure's mode is this test's to set");
    }
}

impl Drop for TheOnlyWayIn {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        // Before the temporary directory's own drop, which would otherwise
        // be a removal that cannot list what it is removing.
        let _ = std::fs::set_permissions(
            self.enclosure.path(),
            std::fs::Permissions::from_mode(Self::AS_MADE),
        );
    }
}

/// Defect 1 of the M4 review, end to end: a job that hides what it wrote
/// behind a directory the runner cannot enter used to be reported
/// `bytes_written: Some(0)`, `Exited { code: 0 }` — with the 100 MB still on
/// the host afterwards, because the removal failed on the same directory.
///
/// **The premise is the whole test, so it is built to be checked and never
/// assumed** ([`TheOnlyWayIn`]). Three things make this an assertion about
/// the product rather than about whichever process last touched a mode:
///
/// 1. the bytes are written **into** the hidden directory, so no measurement
///    anywhere can reach them without a restoration — the old script wrote
///    them in the open and hid them afterwards, which leaves the figure true
///    whether or not anything was ever hidden;
/// 2. the job says in its own log that its hiding place really was
///    unreadable **to the job itself**, and writes nothing if it was not;
/// 3. the enclosure says afterwards whether anything outside this runner
///    opened a directory under it, which is the only other way those bytes
///    could have been counted.
///
/// ## Proved both ways, inside a Herd job, 2026-10-30
///
/// With `root::restore_access` still called and robbed of its effect —
/// `mode | 0o700` written as `mode`, in a worktree and never committed —
/// this test is **red** three times out of three, alone and in its whole
/// file, on its **first** clause (job `j0000000159`): the outcome is
/// `SetupFailed`, naming `…/work/h cannot be read even after its mode was
/// restored: Permission denied`, which is D166's refusal where
/// `StoppedByCap { cap: Disk }` is asserted. With the product as it is,
/// green three times out of three (`j0000000158`).
///
/// And the test **as it was** — `chmod 300 h; sleep 6` — is **green** three
/// times out of three against that same ineffective `restore_access`
/// (`j0000000160`). That green is the whole reason this fixture exists.
///
/// The witness is not a theory either: in `j0000000159`'s third run the
/// enclosure read `SomethingOutsideOpenedIt` on two attempts before a third
/// was judged — the enclosing runner's walk coming past, and being caught
/// at it.
#[tokio::test]
async fn a_job_cannot_hide_what_it_wrote_from_its_disk_budget() {
    let budget = 8 << 20;
    let way_in = TheOnlyWayIn::around("blinded-budget");
    let harness = Harness::rooted_at(way_in.data_root(), None, "blinded-budget", |confinement| {
        confinement.disk_budget = budget;
    })
    .await;
    // The enclosure is blinded first and the hiding place second: in that
    // order an enclosing walk that is already on its way down cannot reach
    // the hiding place without having left its mark above it.
    let script = format!(
        "mkdir h || exit 9; \
         chmod {blinded:o} '{enclosure}' || exit 9; \
         chmod {blinded:o} h || exit 9; \
         if [ -r h ]; then echo '{readable}'; exit 9; fi; \
         echo '{hidden}'; \
         dd if=/dev/zero of=h/big bs=1048576 count=100 2>/dev/null; \
         exit 0",
        blinded = TheOnlyWayIn::BLINDED,
        enclosure = way_in.path().display(),
        readable = STILL_READABLE_BY_THE_JOB,
        hidden = HIDDEN_FROM_THE_JOB_ITSELF,
    );
    let ran = {
        let mut judged = None;
        for attempt_at_the_premise in 1..=TheOnlyWayIn::ATTEMPTS {
            way_in.hold_again();
            let ran = harness.run(&script).await;
            let said = way_in.asked();
            if !ran.out.contains(HIDDEN_FROM_THE_JOB_ITSELF)
                || said != WhatTheWayInSays::ThePremiseHeld
            {
                // Said out loud, because a measurement thrown away is a
                // measurement nobody made: `--nocapture` shows how often
                // this host undoes the premise, and the figure belongs in
                // the record rather than in somebody's reasoning.
                println!(
                    "a_job_cannot_hide_what_it_wrote_from_its_disk_budget: attempt \
                     {attempt_at_the_premise} at the premise is void and is taken again from \
                     nothing: the enclosure said {said:?}"
                );
                assert!(
                    attempt_at_the_premise < TheOnlyWayIn::ATTEMPTS,
                    "{}. The enclosure said {said:?}, the job said {out:?}, and it ended \
                     {outcome:?}",
                    TheOnlyWayIn::WOULD_NOT_HOLD,
                    out = ran.out,
                    outcome = ran.outcome,
                );
                continue;
            }
            judged = Some(ran);
            break;
        }
        judged.expect("the loop above either measures or says why it could not")
    };
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Disk,
            limit: budget,
            detail: None
        },
        "a job that blinds the runner must still be held to its budget: {:?} / {}",
        ran.outcome,
        ran.err
    );
    let written = ran
        .metrics
        .bytes_written
        .expect("the runner owns the workspace, so it can always measure it");
    assert!(
        written > budget,
        "the budget tripped at {written} bytes, which is not past {budget}"
    );
    assert!(nothing_left_in(harness.root()).is_empty());
}

/// The same budget, breached by a job that exits at once. Nothing polls fast
/// enough to catch this one while it runs, so the last measurement — taken
/// after the process is gone — is what has to catch it.
#[tokio::test]
async fn a_job_that_fills_its_workspace_and_exits_at_once_is_still_stopped() {
    let budget = 8 << 20;
    let harness = Harness::new("quick-fill", |confinement| {
        confinement.disk_budget = budget;
    })
    .await;
    let ran = harness
        .run("dd if=/dev/zero of=./big bs=1048576 count=100 2>/dev/null; exit 0")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Disk,
            limit: budget,
            detail: None
        },
        "exiting first does not buy a job its budget back: {:?}",
        ran.outcome
    );
    assert!(nothing_left_in(harness.root()).is_empty());
}

/// Defect 2: a job that locks its workspace shut keeps its own verdict, and
/// leaves nothing on the host. The modes it set are the runner's to undo.
#[tokio::test]
async fn a_workspace_a_job_locked_shut_is_still_reclaimed() {
    let harness = Harness::new("locked-shut", |_| {}).await;
    let ran = harness
        .run("mkdir d; echo payload > d/f; chmod 000 d; chmod 500 .; exit 0")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the job's own verdict is real and stays: {:?}",
        ran.outcome
    );
    let left = nothing_left_in(harness.root());
    assert!(
        left.is_empty(),
        "the job kept its workspace by changing its modes: {left:?}"
    );
}

/// The same defect one directory up: a job can `chmod 000` its workspace's
/// *parents* — the `jobs/` directory and the data root itself — as easily as
/// its own files. The runner owns those too, so neither the reclaim nor the
/// next job may be stopped by them.
#[tokio::test]
async fn a_job_cannot_lock_the_runner_out_of_its_own_data_root() {
    let harness = Harness::new("locked-root", |_| {}).await;
    let ran = harness
        .run("echo payload > ./f; chmod 000 ../..; chmod 000 ../../..; exit 0")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "{:?} / {}",
        ran.outcome,
        ran.err
    );
    // The jobs directory is readable again, so this call is itself part of
    // the assertion: it panics if the runner left it shut.
    let left = nothing_left_in(harness.root());
    assert!(left.is_empty(), "the job kept its workspace: {left:?}");

    // And the runner can still be given work.
    let after = harness.run("echo the runner lived").await;
    assert_eq!(
        after.outcome,
        JobOutcome::Exited { code: 0 },
        "{}",
        after.err
    );
    assert_eq!(after.out.trim(), "the runner lived");
}

/// Defect 3, the worst of them: a job that breaches the log cap and exits
/// promptly was reported `Exited { code: 0 }` with the log silently cut. A
/// truncated build reported green is the worst thing a build service can do.
#[tokio::test]
async fn a_job_that_breaches_the_log_cap_and_exits_at_once_names_the_cap() {
    let cap = 1024;
    let harness = Harness::new("log-cap-quick", |confinement| confinement.log_cap = cap).await;
    let ran = harness
        .run("dd if=/dev/zero bs=4096 count=1 2>/dev/null | tr '\\0' 'x'; exit 0")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Log,
            limit: cap,
            detail: None
        },
        "a cut log is never a pass, whatever the process exited with: {:?}",
        ran.outcome
    );
    let streamed = (ran.out.len() + ran.err.len()) as u64;
    assert!(
        streamed <= cap,
        "{streamed} bytes reached the requester with a cap of {cap}"
    );
}

/// The other half of defect 3: a job that keeps writing after the cap has
/// been reached gets `SIGPIPE` when the runner stops reading. That signal is
/// an artefact of the cap, not the job's verdict, and must never be the
/// outcome.
#[tokio::test]
async fn a_job_killed_by_the_pipe_the_log_cap_closed_still_names_the_cap() {
    let cap = 1024;
    let harness = Harness::new("log-cap-sigpipe", |confinement| confinement.log_cap = cap).await;
    let ran = harness
        .run("while :; do dd if=/dev/zero bs=4096 count=64 2>/dev/null | tr '\\0' 'x'; done")
        .await;
    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: Cap::Log,
            limit: cap,
            detail: None
        },
        "SIGPIPE from the cap's own doing must not leak out as the job's \
         verdict: {:?}",
        ran.outcome
    );
}

/// Defect 4: a job's process that gave itself a new session escapes
/// `killpg` by definition, and used to outlive its job — reparented to init,
/// on a host where the runner is not a delegated unit, which is the default
/// shape. Under a cgroup the cgroup kill answers it; without one, the marker
/// sweep does. Either way this assertion is real on every host.
#[tokio::test]
async fn a_job_s_child_in_its_own_session_does_not_outlive_the_job() {
    // A sleep nobody else on this host would be running, so that finding one
    // is finding ours.
    let marker = &a_sleep_nobody_else_is_sleeping(1);
    let harness = Harness::new("setsid-child", |_| {}).await;
    let ran = harness
        .run(&format!(
            "setsid sleep {marker} >/dev/null 2>&1 & sleep 0.3; exit 0"
        ))
        .await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.err);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let survivors = processes_with_argument(marker);
        if survivors.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{survivors:?} outlived the job that started them, in a session of \
             their own where killpg cannot reach"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Finding 2 of M4's verification, end to end: a job whose workspace cannot
/// be measured at all is stopped by the disk cap (D30) — and the record says
/// **why**, rather than naming a figure the job never came near.
///
/// The reproduction is the one `root.rs` uses for an unmeasurable tree: the
/// job replaces its own workspace directory with a *file*. `read_dir` then
/// fails with `ENOTDIR`, which no mode can fix, so the runner genuinely
/// cannot take the figure. Before this, the requester was told "the runner
/// stopped this job at its disk cap of 21474836480 bytes" for a job that had
/// written a few bytes, and the true reason went to the runner's stderr where
/// no requester can read it.
#[tokio::test]
async fn an_unmeasurable_workspace_says_why_in_the_job_s_own_record() {
    let budget = 64 << 20;
    let harness = Harness::new("unmeasurable-workspace", |confinement| {
        confinement.disk_budget = budget;
    })
    .await;
    let ran = harness
        .run(
            "base=$(dirname \"$PWD\"); printf 'wrecking it\\n'; cd /; rm -rf \"$base\"; \
             : > \"$base\"; exit 0",
        )
        .await;
    let JobOutcome::StoppedByCap { cap, limit, detail } = &ran.outcome else {
        panic!(
            "a workspace the runner cannot measure is a budget it cannot enforce, and \
             that is never a job that passed: {:?} / {}",
            ran.outcome, ran.err
        )
    };
    assert_eq!(*cap, Cap::Disk, "{:?}", ran.outcome);
    assert_eq!(*limit, budget, "the cap's own figure is still the cap's");
    let detail = detail
        .as_ref()
        .expect("the record must carry the true reason, not only the cap's figure");
    assert!(
        detail.contains("could not measure") && detail.contains("workspace"),
        "the reason must say the workspace could not be measured: {detail}"
    );
    // D30's other half: the figure is absent, never a zero.
    assert!(
        ran.metrics.bytes_written.is_none(),
        "a figure that could not be taken must be absent: {:?}",
        ran.metrics
    );
    // And the requester is told the same thing in words, not only in a field.
    let said = herdctl::stopped_by_cap_message(*cap, *limit, detail.as_str().into());
    assert!(said.contains("could not measure"), "{said}");
}

/// Finding 6 of M4's verification, from the other end: a workspace the runner
/// cannot reclaim is in the job's **record**, not only on the runner's
/// stderr. Same reproduction, and the job's log is part of its record.
#[tokio::test]
async fn a_workspace_that_cannot_be_reclaimed_is_in_the_job_s_log() {
    let harness = Harness::new("unreclaimable-workspace", |_| {}).await;
    let ran = harness
        .run("base=$(dirname \"$PWD\"); cd /; rm -rf \"$base\"; : > \"$base\"; exit 0")
        .await;
    assert!(
        ran.err.contains("could not be reclaimed") && ran.err.contains("left on this host"),
        "bytes left on a host nobody will collect must reach the requester, not \
         only the runner's stderr: {:?} / {}",
        ran.outcome,
        ran.err
    );
}

/// Defect 9: the whole metric path, so that a measured zero and a figure
/// that could not be taken can never be confused.
///
/// A job that wrote nothing reports `Some(0)` — it was measured, and it was
/// nothing. A job that wrote and then deleted reports the high-water mark,
/// which is what the disk budget is about. The unmeasurable case reports
/// `None`, and is asserted in `root.rs`'s unit tests, where a tree can be
/// made genuinely unreadable rather than merely locked.
#[tokio::test]
async fn a_measured_zero_is_not_an_unmeasured_figure() {
    let harness = Harness::new("metrics", |_| {}).await;

    let quiet = harness.run("exit 0").await;
    assert_eq!(quiet.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(
        quiet.metrics.bytes_written,
        Some(0),
        "a job that wrote nothing was measured, and the measurement is zero"
    );
    assert!(
        quiet.metrics.host_load_milli.is_some(),
        "/proc/loadavg is readable on Linux"
    );

    // Written, held long enough for the watchdog to see it, then deleted.
    let deleted = harness
        .run("dd if=/dev/zero of=./big bs=1048576 count=20 2>/dev/null; sleep 1; rm ./big")
        .await;
    assert_eq!(
        deleted.outcome,
        JobOutcome::Exited { code: 0 },
        "{}",
        deleted.err
    );
    let written = deleted
        .metrics
        .bytes_written
        .expect("a measurable workspace is never absent");
    assert!(
        written >= 20 << 20,
        "the high-water mark lost what the job deleted: {written} bytes"
    );
}

// ---------------------------------------------------------------- E-46 ----

/// Defect 10: on this server the runner's user is in the `docker` group,
/// which is root through `/run/docker.sock` whatever the process's uid says.
/// A deployment that is not really unprivileged must say so at startup.
///
/// The host is asked with `id -Gn`, not with the code under test, so this
/// cannot pass by agreeing with itself.
#[tokio::test]
async fn a_root_equivalent_user_is_named_at_startup() {
    let temp = TempDir::new("root-equivalent");
    let confined = Confined::prepare(test_confinement(temp.path())).expect("confinable");
    let report = confined.report().join("\n");

    let groups = std::process::Command::new("id")
        .arg("-Gn")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    // The group that shares its name with the escalation tool is left out on
    // purpose: `privilege.rs` is the only file of this crate allowed to write
    // that word, and a test may not be the exception that breaks its own
    // rule. `wheel` and the container-runtime groups are enough.
    let root_equivalent: Vec<&str> = groups
        .split_whitespace()
        .filter(|name| {
            [
                "docker", "podman", "lxd", "incus", "libvirt", "libvirtd", "wheel",
            ]
            .contains(name)
        })
        .collect();
    if root_equivalent.is_empty() {
        skipped(
            "a_root_equivalent_user_is_named_at_startup",
            "this user is in no root-equivalent group, so there is nothing for the \
             startup report to warn about here",
        );
        return;
    }
    for group in root_equivalent {
        assert!(
            report.contains(group) && report.contains("WARNING"),
            "the startup report must name the {group} group as root-equivalent: \
             {report}"
        );
    }
    assert!(
        report.contains("not true of this deployment"),
        "the warning must say plainly that E-46 does not hold here: {report}"
    );
}

// ---------------------------------------------------------------- E-30 ----

/// The milestone's hardest promise: an allocation past the ceiling ends the
/// job **and only the job**. A sibling job runs through the whole thing and
/// the runner goes on taking work afterwards.
///
/// Two assertions, and they are not equally available. That the job dies and
/// nothing else does is asserted on every host. That the kernel killed it by
/// name — `stopped_by_cap { memory }` out of the job's own cgroup — needs
/// cgroup v2 delegation, and is skipped with its reason where there is none.
#[tokio::test]
async fn an_allocation_past_the_ceiling_kills_the_job_and_only_the_job() {
    let harness = Harness::new("ceiling", |confinement| {
        confinement.memory_max = 64 << 20;
        confinement.timeout = Duration::from_secs(30);
    })
    .await;
    let real_ceiling = kernel_ceiling_or_reason();

    // The hog doubles a shell variable, which is an allocation in the job's
    // own process and reaches gibibytes in a few dozen turns.
    let hog = harness.run("a=x; while :; do a=\"$a$a\"; done; echo survived");
    let sibling = harness.run("sleep 2; echo the sibling lived");
    let (hog, sibling) = tokio::join!(hog, sibling);

    assert_eq!(
        sibling.outcome,
        JobOutcome::Exited { code: 0 },
        "the sibling job did not survive its neighbour's ceiling: {:?} / {}",
        sibling.outcome,
        sibling.err
    );
    assert_eq!(sibling.out.trim(), "the sibling lived");
    assert_ne!(
        hog.outcome,
        JobOutcome::Exited { code: 0 },
        "the hog was allowed past the ceiling: {:?}",
        hog.outcome
    );
    assert!(
        !hog.out.contains("survived"),
        "the hog ran past the ceiling: {:?}",
        hog.out
    );

    match real_ceiling {
        Ok(()) => assert_eq!(
            hog.outcome,
            JobOutcome::StoppedByCap {
                cap: Cap::Memory,
                limit: 64 << 20,
                detail: None
            },
            "the kernel killed the job's cgroup, so the outcome must name the \
             memory ceiling rather than a bare signal: {:?}",
            hog.outcome
        ),
        Err(why) => skipped(
            "an_allocation_past_the_ceiling_kills_the_job_and_only_the_job \
             (the named memory outcome)",
            &format!(
                "{why} — the job was still stopped and its sibling still survived, \
                 but by the RLIMIT_AS fallback, which reports the job's own failure \
                 rather than stopped_by_cap{{memory}}"
            ),
        ),
    }

    // The runner itself is unharmed: it takes the next job.
    let after = harness.run("echo the runner lived").await;
    assert_eq!(after.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(after.out.trim(), "the runner lived");
}

/// Whichever mechanism this host has, the runner says which one at startup.
/// A fallback that is not said out loud is the failure mode this test exists
/// to forbid.
#[tokio::test]
async fn the_runner_says_which_memory_ceiling_it_enforces() {
    let temp = TempDir::new("ceiling-report");
    let confined = Confined::prepare(test_confinement(temp.path())).expect("confinable");
    let report = confined.report().join("\n");
    match confined.ceiling().fallback_reason() {
        None => assert!(
            report.contains("one cgroup v2 subtree per job"),
            "a host with the real ceiling must say so: {report}"
        ),
        Some(why) => {
            assert!(report.contains("FALLING BACK"), "{report}");
            assert!(report.contains(why), "{report}");
            skipped(
                "the_runner_says_which_memory_ceiling_it_enforces \
                 (the cgroup branch of the report)",
                why,
            );
        }
    }
}

// ------------------------------------------- E-31 against E-29 (D-NEXT) ----

/// How much tmpfs the subject job writes, and the ceiling it writes under.
/// Three times the ceiling, so a job that survives has survived by a margin
/// no accounting slack can explain.
const SHMEM_CEILING: u64 = 64 << 20;
const SHMEM_WRITTEN_MIB: u64 = 192;

/// What a delegated scope would settle, said in every skip of the test below.
const WHAT_WOULD_SETTLE_THE_CHARGE: &str = "a delegated scope — `systemd-run \
     --user --scope -p Delegate=yes -p MemoryMax=12G <the test binary> \
     --test-threads 1`, the recipe in docs/runner-confinement.md — would settle \
     whether a tmpfs byte a job writes is charged to that job's own memory.max";

/// A directory on the host's tmpfs, removed whatever happens.
///
/// **Not a [`TempDir`]**, which is rooted at `std::env::temp_dir()`: under the
/// gate that is the nix-shell's directory on ordinary disk, and a file there is
/// page cache, which is reclaimable and so measures nothing about this
/// question. This one is rooted at [`DEFAULT_TMPFS_PATH`], the very tmpfs
/// E-31's admission figure is measured from.
///
/// It is made and removed by the **test**, never by the job: the point of the
/// test is that the job may be killed mid-write, and a job killed mid-write
/// runs no cleanup of its own. Without this `Drop` a charged verdict would
/// leave tens of mebibytes on the host's shared tmpfs after every run.
struct OnTheHostsTmpfs {
    dir: PathBuf,
}

impl OnTheHostsTmpfs {
    /// `Err` with the reason when this host lends no tmpfs a job could write
    /// in — a premise that did not hold, never a verdict.
    fn new(label: &str) -> Result<Self, String> {
        // The capacity, which is what E-31 announces. A tmpfs too small for
        // the quantity cannot answer the question either way: a write that
        // stopped at ENOSPC is not a write that stopped at a ceiling.
        let room = herd_runner::metrics::tmpfs_room_mib(DEFAULT_TMPFS_PATH)?;
        if room < SHMEM_WRITTEN_MIB * 2 {
            return Err(format!(
                "{DEFAULT_TMPFS_PATH} has {room} MiB of capacity and this test writes \
                 {SHMEM_WRITTEN_MIB} MiB into it, so on a tmpfs this small an ENOSPC \
                 and a memory ceiling could not be told apart"
            ));
        }
        let name = format!("herd-test-{label}-{}", std::process::id());
        let dir = Path::new(DEFAULT_TMPFS_PATH).join(name);
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(error) = std::fs::create_dir(&dir) {
            return Err(format!(
                "{DEFAULT_TMPFS_PATH} has room but is not writable here: {error}"
            ));
        }
        Ok(Self { dir })
    }

    fn file(&self) -> PathBuf {
        self.dir.join("written-by-the-job")
    }
}

impl Drop for OnTheHostsTmpfs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// **The question a per-job ceiling turns on: is a tmpfs byte a job writes
/// charged to that job's own `memory.max`?**
///
/// Herd admits `tmpfs_mib` (E-31) separately from `memory_mib` (E-29), as two
/// independent quantities. They are **not** independent. Shmem pages are
/// charged to the cgroup of the task that touches them, and a job's cgroup is
/// created with `memory.swap.max = 0` (`herd-runner/src/cgroup.rs`), so the
/// charge can be neither swapped out nor dropped. A job that reserves tmpfs
/// and uses it is therefore killed at its `memory_mib` however much tmpfs it
/// declared — so a per-job ceiling set to `memory_mib + margin` would
/// OOM-kill, on its first day, every job that reserves tmpfs and fills it. The
/// first consumer has already paid this once, and their kernel's own memcg
/// OOM report for job j0000000069 is the real-world positive this fixture
/// reproduces: on the job cgroup `herd-job-1785059-6…`, `usage 8388608kB,
/// limit 8388608kB … shmem 4219834368` — 4.031 GiB anonymous plus 3.930 GiB
/// shmem is 7.961 GiB against an 8.000 GiB ceiling, which closes. The killed
/// `qemu`'s own `shmem-rss` read **0** while its cgroup carried nearly 4 GiB
/// of shmem: the charge is accounted at the **cgroup**, not at the process,
/// which is why a per-process reading cannot see it (their report, their
/// figures, their machine — invariant 9).
///
/// **Two things that report does not settle, and this test does.** Their
/// tmpfs is theirs, `/mnt/ssd_980/chatons/ram`, a mount **outside** Herd's
/// `--host-tmpfs` path, so it proves the kernel's charging rule and not
/// Herd's own bookkeeping; this fixture writes into [`DEFAULT_TMPFS_PATH`],
/// which is the mount E-31 measures. And a formula is not the whole cure:
/// that job declared 1792 MiB of memory and reserved 2.5 GiB of tmpfs, and
/// used 2.30× and 1.57× of those, so a ceiling correctly computed as
/// `memory_mib + tmpfs_mib + margin` would still have killed it. Counting
/// tmpfs in the ceiling is **necessary and not sufficient**; the declarations
/// were also wrong, which is the consumer's half.
///
/// ## Three outcomes, each visible, none silent
///
/// - **charged** — the subject job is `stopped_by_cap{memory}`. That is the
///   measured answer and what this test asserts.
/// - **not charged** — the subject writes all of it and prints `survived`.
///   A **failure** here, and a loud one: it would mean the kernel's accounting
///   had changed under Herd, and that every ceiling arithmetic resting on the
///   paragraph above has to be redone.
/// - **the premise did not hold** — a skip with its reason, never a pass.
///   Four separate ways, checked in order, because each would otherwise
///   produce a job that survives for a reason unrelated to the question.
///
/// ## How each way the premise can fail is closed
///
/// 1. **No tmpfs.** Herd names a job no tmpfs at all: `job_environment`
///    (`herd-runner/src/confine.rs`) points `TMPDIR`, `TMP`, `TEMP` and
///    `XDG_RUNTIME_DIR` at the job's workspace on the data root, which is
///    ordinary disk. A test writing to `$TMPDIR` would measure page cache and
///    nothing else. What a job *can* reach is [`DEFAULT_TMPFS_PATH`] by
///    absolute path, because M4 gives a job no mount namespace and says so
///    (`herd-runner/src/root.rs`: "What M4 does **not** do is stop a job that
///    names an absolute path outside the root"). That is also exactly the
///    tmpfs E-31 measures its own figure from, so this measures the quantity
///    the admission arithmetic actually uses.
/// 2. **No kernel ceiling.** Without a delegated cgroup v2 the ceiling is
///    `RLIMIT_AS` on the first process (`herd-runner/src/job.rs`), which is
///    per-process, bounds no tree, and which `dd` writing to a file never
///    trips because the bytes never enter its address space. On such a host
///    the subject survives for the wrong reason, so
///    [`kernel_ceiling_or_reason`] is asked **before** any verdict is read.
/// 3. **A ceiling that does not bite.** Delegation can be present and the
///    ceiling still fail to kill at this scale. So the same ceiling is first
///    shown to kill the same quantity of **anonymous** memory: a known-bad
///    fixture, in the test, on every run. Only a control that died licenses
///    reading the subject at all — otherwise a surviving subject means
///    nothing, the detector having been dead rather than the charge absent.
/// 4. **A job that never wrote.** Exit 90, 91 or 92 says the job could not
///    create its file, that its write failed of itself, or that the host has
///    no `dd`. Each is a skip, never a survival.
///
/// A fifth hazard is closed by construction rather than by a check: the
/// runner's workspace walk calls `set_permissions(mode | 0o700)` every 200 ms
/// on a directory it cannot list, and its trigger is duration, not
/// concurrency. It cannot reach this fixture, because every walk in
/// `herd-runner/src/root.rs` starts at the workspace and `restore_ancestors`
/// returns early unless the path is under the root it was given. The fixture
/// is on `/dev/shm`, outside the data root entirely.
#[tokio::test]
async fn a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling() {
    const TEST: &str = "a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling";

    // Premise 1: a tmpfs this host lends, that a job could write in.
    let fixture = match OnTheHostsTmpfs::new("shmem-charge") {
        Ok(fixture) => fixture,
        Err(why) => {
            skipped(TEST, &format!("{why} — {WHAT_WOULD_SETTLE_THE_CHARGE}"));
            return;
        }
    };

    // Premise 2: the ceiling is the kernel's, not RLIMIT_AS on one process.
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            TEST,
            &format!(
                "{why} — so `memory.max` bounds no tree here, and a job writing \
                 {SHMEM_WRITTEN_MIB} MiB of tmpfs would survive the RLIMIT_AS fallback \
                 whether or not its shmem was charged: the premise of this measurement \
                 did not hold and nothing about the charge was checked. \
                 {WHAT_WOULD_SETTLE_THE_CHARGE}"
            ),
        );
        return;
    }

    let harness = Harness::new("shmem-charge", |confinement| {
        confinement.memory_max = SHMEM_CEILING;
        confinement.timeout = Duration::from_secs(30);
    })
    .await;

    // Premise 3, the known-bad fixture: the same ceiling and the same
    // quantity, as *anonymous* memory. `dd` with one big block allocates the
    // whole buffer and faults every page of it, so this is the same number of
    // bytes the subject writes, charged the one way nobody disputes.
    let control_script = format!(
        "command -v dd >/dev/null 2>&1 || exit 92; \
         dd if=/dev/zero of=/dev/null bs={SHMEM_WRITTEN_MIB}M count=1 2>/dev/null \
         || exit 91; \
         echo survived"
    );
    let control = harness.run(&control_script).await;
    if control.outcome == (JobOutcome::Exited { code: 0 }) || control.out.contains("survived") {
        skipped(
            TEST,
            &format!(
                "the control arm survived: {SHMEM_WRITTEN_MIB} MiB of ANONYMOUS memory under \
                 a {} MiB ceiling ended {:?}, so this host's ceiling does not kill at this \
                 scale and a surviving tmpfs arm would prove nothing about the charge — the \
                 detector was not shown to fire, so its silence is not a result. \
                 {WHAT_WOULD_SETTLE_THE_CHARGE}",
                SHMEM_CEILING >> 20,
                control.outcome
            ),
        );
        return;
    }

    // The subject: the same ceiling and the same quantity, written into the
    // host's tmpfs instead of into anonymous memory.
    let target = fixture.file();
    let subject_script = format!(
        "command -v dd >/dev/null 2>&1 || exit 92; \
         : > \"{target}\" 2>/dev/null || exit 90; \
         dd if=/dev/zero of=\"{target}\" bs=1M count={SHMEM_WRITTEN_MIB} 2>/dev/null \
         || exit 91; \
         echo survived",
        target = target.display()
    );
    let subject = harness.run(&subject_script).await;

    // Premise 4: a job that never wrote a byte says so by its exit code.
    for (code, why) in [
        (90, "the job could not create its file on the tmpfs"),
        (
            91,
            "the job's write failed of itself: out of room, or `dd` killed by a \
             signal while the shell outlived it, which happens only where \
             memory.oom.group did not take",
        ),
        (92, "this host's job environment has no dd, so nothing was written"),
    ] {
        if subject.outcome == (JobOutcome::Exited { code }) {
            skipped(
                TEST,
                &format!(
                    "{why} (exit {code}), so none of the {SHMEM_WRITTEN_MIB} MiB existed to \
                     be charged and this measurement was void: {}",
                    subject.err
                ),
            );
            return;
        }
    }

    // The verdict, three-way and explicit.
    match &subject.outcome {
        // Charged: the measured answer, and the one this test asserts.
        JobOutcome::StoppedByCap {
            cap: Cap::Memory,
            limit,
            ..
        } => {
            assert_eq!(
                *limit,
                SHMEM_CEILING,
                "the job was stopped by a memory ceiling that is not the one it was given"
            );
            assert!(
                !subject.out.contains("survived"),
                "a job stopped by its memory cap cannot also have printed `survived`: {:?}",
                subject.out
            );
        }
        // Not charged: the two admitted quantities would really be
        // independent, and every sentence above is wrong.
        JobOutcome::Exited { code: 0 } => panic!(
            "NOT CHARGED: a job under a {} MiB ceiling wrote {SHMEM_WRITTEN_MIB} MiB into \
             {DEFAULT_TMPFS_PATH} and lived, while the same quantity of anonymous memory \
             under the same ceiling ended {:?}. Shmem is no longer charged to the job that \
             touches it, so E-29 and E-31 ARE independent and every ceiling arithmetic \
             that assumed otherwise has to be redone. The job said {:?}",
            SHMEM_CEILING >> 20,
            control.outcome,
            subject.out
        ),
        // Dead, but not by the memory cap: a disk-budget kill, a timeout or a
        // bare signal answers a different question, and is never read as
        // charged.
        other => panic!(
            "AMBIGUOUS: the job that wrote {SHMEM_WRITTEN_MIB} MiB of tmpfs under a {} MiB \
             memory ceiling ended {other:?}, which is neither `stopped_by_cap{{memory}}` nor \
             a clean exit, so it says nothing about whether that shmem was charged to it. \
             The job said out {:?} / err {:?}",
            SHMEM_CEILING >> 20,
            subject.out,
            subject.err
        ),
    }

    // And the runner is unharmed by either arm: it takes the next job.
    let after = harness.run("echo the runner lived").await;
    assert_eq!(after.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(after.out.trim(), "the runner lived");
}

// ------------------------------------------- CPU, IO and pid bounds ----

/// What a delegated scope would prove, said in every skip below, so a reader
/// of a green run knows exactly what was not checked and how to check it.
const WHAT_DELEGATION_WOULD_PROVE: &str = "a delegated scope — \
     `systemd-run --user --scope -p Delegate=yes cargo test ...` — would prove that \
     a job's own cgroup really carries cpu.weight, cpu.max, io.weight and pids.max, \
     read back by the job itself out of /sys/fs/cgroup, and that pids.max stops a \
     job from forking without end";

/// Ask the job itself what its cgroup says, which is the only reading that
/// cannot agree with the code under test by accident.
fn read_own_cgroup() -> String {
    "d=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup); \
     for f in memory.max cpu.weight cpu.max io.weight io.max pids.max; do \
       printf '%s=%s\\n' \"$f\" \"$(cat \"$d/$f\" 2>/dev/null | tr '\\n' ' ' || echo MISSING)\"; \
     done"
        .to_owned()
}

/// Pick one `name=value` line out of what that script printed.
fn control(out: &str, name: &str) -> String {
    out.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("the job printed no {name}: {out}"))
        .trim()
        .to_owned()
}

/// Memory was the only control a job's cgroup carried, and `nice 10` plus the
/// idle IO class was everything else standing between a job and the host.
/// Four controls are written now, and this reads all four back out of the
/// kernel — from inside the job, through `/proc/self/cgroup`.
#[tokio::test]
async fn a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds() {
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            "a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds",
            &format!("{why} — {WHAT_DELEGATION_WOULD_PROVE}"),
        );
        return;
    }
    let harness = Harness::new("controls", |confinement| {
        // Each one different from its default, so a value read back is this
        // test's value and not a figure the kernel or the default supplied.
        confinement.cpu_weight = 37;
        confinement.cpu_max =
            Some(herd_runner::cgroup::CpuQuota::from_cores(1.5).expect("1.5 cores is a quota"));
        confinement.io_weight = 23;
        confinement.pids_max = 64;
    })
    .await;
    let ran = harness.run(&read_own_cgroup()).await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the job could not read its own cgroup: {} / {}",
        ran.out,
        ran.err
    );
    assert_eq!(control(&ran.out, "memory.max"), (512 << 20).to_string());
    assert_eq!(control(&ran.out, "cpu.weight"), "37");
    assert_eq!(
        control(&ran.out, "cpu.max"),
        "150000 100000",
        "a declared quota is a quota and a period, in microseconds: {}",
        ran.out
    );
    // `io.weight` reads back as "default N", with a line per device where an
    // operator set one, so the figure is matched inside it.
    let io = control(&ran.out, "io.weight");
    assert!(
        io.contains("23"),
        "the job's cgroup must carry the io.weight it was given: {io}"
    );
    assert_eq!(control(&ran.out, "pids.max"), "64");
}

/// What a delegated scope **with its data root on a block device** would
/// prove about the hard IO ceiling, said in every skip below.
///
/// Two conditions and not one, because `io.max` is per-device: a scope with
/// its temporary directory on ZFS proves nothing here, and the skip has to
/// say so rather than reading as "no delegation" (D120).
const WHAT_A_CEILING_WOULD_PROVE: &str = "a delegated scope whose temporary directory is on a \
     block device — `TMPDIR=/mnt/ssd_980/tmp systemd-run --user --scope -p Delegate=yes \
     cargo test ...`, and note that Delegate=yes delegates only to the scope's single \
     process (D117) — would prove that a job's own cgroup carries the io.max ceiling it was \
     given, read back by the job itself out of /sys/fs/cgroup, and that the line names the \
     device the data root is on";

/// Whether a hard IO ceiling could be written for a data root on the
/// filesystem this suite's temporary directories land on, and why not if it
/// could not. Asked before a harness is built, because a runner that refuses
/// a ceiling refuses to start at all — which is the point of the refusal.
fn io_ceiling_or_reason() -> Result<String, String> {
    let probe = TempDir::new("io-ceiling-question");
    herd_runner::blockio::ceiling_device(probe.path()).map(|device| device.device)
}

/// The ceiling the consumer's freeze actually needs: a **hard** per-device
/// cap, not the share `io.weight` gives. Written for the device the data root
/// is on, and read back out of the kernel by the job itself.
///
/// `io.weight` decides who gets the disk when two cgroups want it and caps
/// nothing, so one job alone saturates an idle disk; `io.max` holds whatever
/// else is asking. The job printing its own `io.max` is the only reading that
/// cannot agree with the code under test by accident.
#[tokio::test]
async fn a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given() {
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            "a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given",
            &format!("{why} — {WHAT_A_CEILING_WOULD_PROVE}"),
        );
        return;
    }
    let device = match io_ceiling_or_reason() {
        Ok(device) => device,
        Err(why) => {
            skipped(
                "a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given",
                &format!("{why} — {WHAT_A_CEILING_WOULD_PROVE}"),
            );
            return;
        }
    };
    let harness = Harness::new("io-ceiling", |confinement| {
        confinement.io_max = Some(
            herd_runner::cgroup::IoCeiling::parse("rbps=100MB,wbps=50MB,riops=3000,wiops=1500")
                .expect("four limits are a ceiling"),
        );
    })
    .await;
    let ran = harness.run(&read_own_cgroup()).await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the job could not read its own cgroup: {} / {}",
        ran.out,
        ran.err
    );
    let io_max = control(&ran.out, "io.max");
    // The device first: io.max is per-device, and a line naming another
    // device — or none — would be a ceiling the kernel keeps and never
    // applies to this job's writes.
    assert!(
        io_max.contains(&device),
        "the ceiling must name the device the data root is on ({device}): {io_max}"
    );
    for limit in [
        "rbps=100000000",
        "wbps=50000000",
        "riops=3000",
        "wiops=1500",
    ] {
        assert!(
            io_max.contains(limit),
            "the job's cgroup must carry {limit}: {io_max}"
        );
    }
}

/// And with no ceiling declared, `io.max` is **empty**: absent by default is
/// a real absence, not a figure somebody guessed. A default throughput would
/// be right on the machine it was chosen on and wrong on every other.
#[tokio::test]
async fn no_declared_ceiling_leaves_io_max_empty() {
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            "no_declared_ceiling_leaves_io_max_empty",
            &format!("{why} — {WHAT_A_CEILING_WOULD_PROVE}"),
        );
        return;
    }
    let harness = Harness::new("no-ceiling", |confinement| {
        confinement.io_max = None;
    })
    .await;
    let ran = harness.run(&read_own_cgroup()).await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    let io_max = control(&ran.out, "io.max");
    assert!(
        io_max.is_empty() || io_max == "MISSING",
        "a job with no declared ceiling must have no io.max line at all, and certainly not \
         a zero: {io_max:?}"
    );
    // The weight is still there, and it is the thing that is not a cap.
    assert!(control(&ran.out, "io.weight").contains("10"), "{}", ran.out);
}

/// And with no quota declared, `cpu.max` is the kernel's own `max`: absent by
/// default is a real absence, not a quota equal to the machine.
#[tokio::test]
async fn no_declared_quota_leaves_cpu_max_at_the_kernel_s_max() {
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            "no_declared_quota_leaves_cpu_max_at_the_kernel_s_max",
            &format!("{why} — {WHAT_DELEGATION_WOULD_PROVE}"),
        );
        return;
    }
    let harness = Harness::new("no-quota", |confinement| {
        confinement.cpu_max = None;
    })
    .await;
    let ran = harness.run(&read_own_cgroup()).await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    assert!(
        control(&ran.out, "cpu.max").starts_with("max"),
        "a job with no declared quota must have no quota: {}",
        ran.out
    );
    // The weight is still there: yielding under contention is the default,
    // and it costs an idle machine nothing.
    assert_eq!(control(&ran.out, "cpu.weight"), "10");
}

/// The bound the consumer's second incident needed: a job that forks without
/// end runs out of **its own** pids and the host keeps its own. The job is
/// asked to reach well past its ceiling and must not get there.
#[tokio::test]
async fn a_job_cannot_fork_past_its_own_pid_ceiling() {
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(
            "a_job_cannot_fork_past_its_own_pid_ceiling",
            &format!("{why} — {WHAT_DELEGATION_WOULD_PROVE}"),
        );
        return;
    }
    let harness = Harness::new("pid-ceiling", |confinement| {
        confinement.pids_max = 16;
        confinement.timeout = Duration::from_secs(20);
    })
    .await;
    // Not a fork bomb: a counted number of children, well past the ceiling
    // and far short of anything that could trouble the host even if the
    // ceiling did nothing. The last line is only reached if all of them
    // started.
    let ran = harness
        .run(
            "i=0; while [ $i -lt 200 ]; do sleep 20 & i=$((i+1)); done; \
             echo every child started",
        )
        .await;
    assert!(
        !ran.out.contains("every child started"),
        "a job with pids.max 16 started two hundred children: {} / {}",
        ran.out,
        ran.err
    );
    // And the runner is unharmed: it takes the next job.
    let after = harness.run("echo the runner lived").await;
    assert_eq!(after.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(after.out.trim(), "the runner lived");
}

/// Where a test may put a data root when the suite's own temporary directory
/// is on a filesystem the assertion is not about.
///
/// `/tmp` is ZFS on this project's server, and a ZFS dataset is a place where
/// no cgroup IO knob can bite (D120) — so the assertion that a weighted
/// device *is* reported as weighted had nowhere to stand and skipped. An
/// operator who has a writable directory on a block-device filesystem points
/// this at it; without it the assertions below say what they could not check.
fn scratch_root() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("HERD_TEST_SCRATCH")?);
    path.is_dir().then_some(path)
}

/// Every mount point in this process's mount table, with its filesystem type.
fn mount_points() -> Vec<(PathBuf, String)> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let point = line.split_whitespace().nth(4)?;
            let kind = line.split(" - ").nth(1)?.split_whitespace().next()?;
            Some((PathBuf::from(point), kind.to_owned()))
        })
        .collect()
}

/// D120: whether `io.weight` bites is a fact about **the device the job's
/// directory is on**, and not about the host's disks in general. D116 read
/// the host's IO schedulers, found no bfq anywhere, and called this server's
/// NVMe inert — while blk-iocost weights it per cgroup.
///
/// Two devices, two different answers, asserted against this host's real
/// `/proc/self/mountinfo`, `/sys/fs/cgroup/io.cost.*` and request queues. No
/// cgroup and no writable directory are needed for either: the verdict is
/// read, not written.
#[test]
fn the_io_weight_verdict_follows_the_device_the_job_works_on() {
    const TEST: &str = "the_io_weight_verdict_follows_the_device_the_job_works_on";
    let mounts = mount_points();

    // Half one: a ZFS dataset, where no cgroup IO knob reaches and the
    // sentence must say *that* rather than a generic "inert" — an operator
    // told "inert" would go looking for a scheduler to change, and there is
    // none that would help.
    match mounts.iter().find(|(_, kind)| kind == "zfs") {
        Some((point, _)) => {
            let reach = herd_runner::blockio::io_weight_reach(point);
            assert!(!reach.bites, "no cgroup IO knob reaches ZFS: {reach:?}");
            let why = reach.why;
            assert!(why.contains("ZFS"), "the ZFS answer must name ZFS: {why}");
            assert!(
                why.contains("io.max"),
                "it must also say that the other IO lever cannot reach it: {why}"
            );
        }
        None => skipped(
            TEST,
            "this host has no ZFS mount — a host with one would prove that a data root on \
             a ZFS dataset is reported as reached by no cgroup IO knob at all, rather than \
             as generically inert (D120)",
        ),
    }

    // Half two: a device the block layer really does weight. The evidence is
    // read here, independently of the code under test: a cost model or an
    // enabled qos line in /sys/fs/cgroup, or a request queue set to bfq.
    let weighted_devices = std::fs::read_to_string("/sys/fs/cgroup/io.cost.model")
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("model=") && !line.contains("rbps=0 "))
        .count();
    let candidates: Vec<PathBuf> = scratch_root()
        .into_iter()
        .chain(
            mounts
                .iter()
                .filter(|(_, kind)| matches!(kind.as_str(), "ext4" | "xfs" | "btrfs"))
                .map(|(point, _)| point.clone()),
        )
        .collect();
    let biting = candidates
        .iter()
        .map(|path| (path, herd_runner::blockio::io_weight_reach(path)))
        .find(|(_, reach)| reach.bites);
    match (biting, weighted_devices) {
        (Some((path, reach)), _) => {
            let why = reach.why;
            assert!(
                why.contains(&path.display().to_string()),
                "the answer must name the directory it is about: {why}"
            );
            assert!(
                why.contains("blk-iocost") || why.contains("bfq"),
                "and which of the two honours the weight there: {why}"
            );
            assert!(
                why.contains("device ") || why.contains("scheduler is bfq"),
                "and the device the job's IO is weighted on: {why}"
            );
        }
        (None, 0) => skipped(
            TEST,
            "no device on this host offers bfq or a blk-iocost cost model, so there is no \
             directory here whose io.weight could bite — a host with one (this project's \
             own server: blk-iocost with ctrl=auto on nvme0n1) would prove that a data \
             root on it is reported as weighted rather than inert",
        ),
        (None, _) => panic!(
            "this host weights IO on {weighted_devices} device(s) and yet no directory \
             among {candidates:?} was reported as weighted — which is the D116 defect \
             over again"
        ),
    }
}

/// `io.weight` is the one control the cgroup core cannot tell the truth
/// about: the write succeeds on every host, and only bfq or blk-iocost
/// honours it. A runner that stayed quiet about that would be claiming a
/// bound it does not have, which is exactly what D23 forbids — and a runner
/// that stayed quiet when it *does* have the bound would leave an operator
/// unable to tell which of its two disks the job should work on (D120).
#[tokio::test]
async fn an_io_weight_that_cannot_bite_is_said_out_loud() {
    let temp = TempDir::new("io-weight-report");
    // On a host whose temporary directory is ZFS, the data root this test
    // would use by default is the one place where nothing can ever bite. A
    // scratch root on a block-device filesystem is what lets the other half
    // of the report be asserted.
    let data_root = scratch_root().map_or_else(
        || temp.path().to_path_buf(),
        |scratch| scratch.join(format!("herd-io-weight-report-{}", std::process::id())),
    );
    std::fs::create_dir_all(&data_root).expect("a data root under the scratch root");
    let confined = Confined::prepare(test_confinement(&data_root)).expect("confinable");
    let report = confined.report().join("\n");
    let no_cgroup = confined
        .ceiling()
        .fallback_reason()
        .map(|why| why.to_owned());
    let Some(why) = confined.io_weight_inert().map(|why| why.to_owned()) else {
        // bfq or blk-iocost: the weight really bites for a job working here,
        // and the report has to say so plainly and name the device — moving
        // the data root is what moves this answer (D120).
        assert!(
            !report.contains("WRITTEN BUT INERT"),
            "a job here is weighted, so the report must not claim otherwise: {report}"
        );
        let plainly = confined.io_weight_reach().why.clone();
        match no_cgroup {
            Some(no_cgroup) => {
                skipped(
                    "an_io_weight_that_cannot_bite_is_said_out_loud \
                     (the weighted-device branch)",
                    &format!(
                        "{no_cgroup} — a job's directory here IS on a weighted device \
                         ({plainly}), and inside a delegated scope the startup report \
                         would have to carry that line instead of the inert one"
                    ),
                );
            }
            None => assert!(
                report.contains("enforced for jobs here") && report.contains(&plainly),
                "an io.weight that does bite must say so, with the device: {report}"
            ),
        }
        let _ = std::fs::remove_dir_all(&data_root);
        return;
    };
    let _ = std::fs::remove_dir_all(&data_root);
    if let Some(no_cgroup) = no_cgroup {
        // With no cgroup at all the report already says io.weight is not
        // enforced; the inert sentence would be a second reason for the same
        // thing.
        assert!(report.contains("NOT ENFORCED AT ALL"), "{report}");
        skipped(
            "an_io_weight_that_cannot_bite_is_said_out_loud",
            &format!(
                "{no_cgroup} — a job's directory here is on a device whose io.weight is inert too \
                 ({why}), and inside a delegated scope the startup report would have to \
                 carry the WRITTEN BUT INERT line for it"
            ),
        );
        return;
    }
    assert!(
        report.contains("WRITTEN BUT INERT") && report.contains(&why),
        "an io.weight the block layer ignores must be shouted, with its reason: {report}"
    );
    assert!(
        report.contains("io.max"),
        "the report must name the lever that would work here: {report}"
    );
}

// --------------------------------------------- the room it is in (D119) ----

/// D119: the figure the hub admits against is the room in the cgroup the
/// runner is **actually in**, not the machine's `MemTotal`. Asserted against
/// a cgroup the kernel really bounds — a child of this process's own cgroup
/// with a `memory.max` of 512 MiB — so that the walk, the parse and the
/// subtraction are checked against the kernel's own files rather than against
/// a fixture.
///
/// The unit tests in `room` prove the walk over a tree a test builds, and
/// that a host with no enclosing limit keeps its `MemTotal` untouched. This
/// one proves the remaining half, and it needs the right to make a cgroup.
#[test]
fn a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced() {
    const TEST: &str = "a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced";
    /// What this assertion is for, said in every skip: a reader of a green
    /// run has to know what was not checked and how to check it.
    const WOULD_PROVE: &str = "a host that may make a cgroup — \
         `systemd-run --user --scope -p Delegate=yes cargo test ...`, or any runner \
         under a systemd unit with Delegate=yes — would prove that a real \
         enclosing memory.max read out of /sys/fs/cgroup lowers the memory this \
         runner announces, which is what stops a slice from being OOM-killed while \
         the hub believes it admitted carefully (D119)";
    let ceiling_bytes: u64 = 512 * (1 << 20);
    // A host that cannot give a job a cgroup of its own cannot be given a
    // cgroup with a ceiling to stand in either, and the reason is the same
    // sentence.
    if let Err(why) = kernel_ceiling_or_reason() {
        skipped(TEST, &format!("{why} — {WOULD_PROVE}"));
        return;
    }
    // Prepared first, and kept alive, for its side effects on a host that
    // does delegate: it moves this process out of the base cgroup into a leaf
    // of its own and hands the memory controller down to the base's children.
    // cgroup v2 refuses both to a cgroup that still holds processes, so
    // without this there is no child here that could carry a `memory.max` —
    // which is the trap D117 names, met from the test's side.
    let temp = TempDir::new("room-ceiling");
    let _confined = Confined::prepare(test_confinement(temp.path())).expect("confinable");
    let mount = Path::new(herd_runner::room::CGROUP_MOUNT);
    let Some(own) = std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .as_deref()
        .and_then(herd_runner::room::own_cgroup)
    else {
        skipped(
            TEST,
            &format!("this host has no cgroup v2 line in /proc/self/cgroup — {WOULD_PROVE}"),
        );
        return;
    };
    // A sibling of the leaf this process stands in, which is where a cgroup
    // with a ceiling can exist: the leaf itself holds a process and so can
    // have no controller-enabled children of its own.
    let Some(base) = Path::new(&own).parent().map(|at| at.to_owned()) else {
        skipped(
            TEST,
            &format!("this process is in the root cgroup {own} — {WOULD_PROVE}"),
        );
        return;
    };
    let name = format!("herd-room-test-{}", std::process::id());
    let cgroup = base.join(&name);
    let directory = mount.join(cgroup.strip_prefix("/").unwrap_or(&cgroup));
    let _ = std::fs::remove_dir(&directory);
    if let Err(error) = std::fs::create_dir(&directory) {
        skipped(
            TEST,
            &format!(
                "this process may not create a cgroup under {} ({error}) — {WOULD_PROVE}",
                mount
                    .join(base.strip_prefix("/").unwrap_or(&base))
                    .display()
            ),
        );
        return;
    }
    let written = std::fs::write(directory.join("memory.max"), ceiling_bytes.to_string());
    let found = herd_runner::room::enclosing(mount, &cgroup.display().to_string());
    let _ = std::fs::remove_dir(&directory);
    if let Err(error) = written {
        skipped(
            TEST,
            &format!(
                "a cgroup was creatable at {} but its memory.max was not writable \
                 ({error}), so the memory controller is not handed down there — \
                 {WOULD_PROVE}",
                directory.display()
            ),
        );
        return;
    }
    let found = found.expect("a cgroup with a memory.max of 512 MiB encloses that path");
    assert_eq!(found.ceiling_bytes, ceiling_bytes, "{found:?}");
    assert_eq!(found.knob, "memory.max");
    assert!(
        found.room_mib() <= 512,
        "the room under a 512 MiB ceiling is at most 512 MiB: {found:?}"
    );
    // And that figure is what the runner would announce, in place of the
    // machine's.
    let host = herd_runner::metrics::total_memory_mib();
    let (announced, said) = herd_runner::room::announced_memory_mib(host, Some(found.clone()));
    assert_eq!(announced, Some(found.room_mib()));
    let said = said.expect("a figure the kernel took down is never announced silently");
    assert!(
        said.contains(&name) && said.contains("memory.max"),
        "{said}"
    );
}

// ------------------------------------------- the one inbound socket ----

/// Invariant 3, asserted against the running process rather than against a
/// promise: **every** listening socket the real runner binary holds is the
/// mesh address its configuration named, and there is **at most one**.
///
/// This is the shape the old assertion said it would take. It used to read
/// "the runner holds no socket in the listening state", with its own note
/// that *"the mesh-bound fetch endpoint of D10 and D11 arrives in M11; when
/// it does, this test becomes 'no socket on a default or public interface'
/// and keeps its shape"*. It now says something stricter than that note
/// promised: not merely "not a public interface" but **that one address and
/// no other**, counted, so a second listener anywhere in the process is a
/// failure even when it is bound somewhere respectable.
///
/// Three properties in one walk, and the middle one is the new part:
///
/// 1. with **no** mesh address configured the permitted set is empty, so the
///    assertion is exactly the old one, letter for letter;
/// 2. with one configured, the permitted set is that one address, and the
///    walk finds **exactly** that one: the fetch endpoint binds it, through
///    `mesh::MeshListener` and from the one module permitted to name a
///    listener. The configured runner also proves the other half of the rule
///    — the address was **accepted** at startup (loopback, on purpose — see
///    `mesh.rs`) and the process joined the hub rather than refusing to
///    start;
/// 3. the walk is first made to find a socket planted on purpose, because an
///    empty result from a walk that can find nothing is not evidence of
///    anything.
///
/// The refusals themselves — a wildcard, an address of the default-route
/// interface, a globally routable one — are asserted in `mesh.rs`'s own unit
/// tests against fixed host fixtures and against this host's real routing,
/// and the two that need no host at all are asserted below against the real
/// binary.
#[tokio::test]
async fn every_listening_socket_of_the_runner_is_its_mesh_address() {
    const TEST: &str = "every_listening_socket_of_the_runner_is_its_mesh_address";
    if !cfg!(target_os = "linux") {
        skipped(
            TEST,
            "this host is not Linux, so /proc/net/tcp cannot be read",
        );
        return;
    }
    // The detector, before the claim: a listener bound by this test, found by
    // the same walk that is about to report the runner's. A zero is not a
    // result until the walk is known to be able to see something.
    let planted = std::net::TcpListener::bind("127.0.0.1:0").expect("the fixture binds a socket");
    let mesh: std::net::SocketAddr = planted
        .local_addr()
        .expect("a bound listener has an address");
    let found = listening_sockets_of(std::process::id());
    assert!(
        found.iter().any(|held| same_socket(*held, mesh)),
        "the walk missed the listener this test bound at {mesh}, so an empty result for the \
         runner would have proved nothing: {found:?}"
    );
    // And the address the runner is now given is the one this test just let
    // go of: free, loopback, and nothing else on this host answers there.
    drop(planted);

    let data_root = TempDir::new("no-listener");
    let state = TempDir::new("no-listener-state");
    let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
    let hub = Hub::bind(
        "127.0.0.1:0".parse().expect("a literal loopback address"),
        Arc::clone(&store) as Arc<dyn JobStore>,
    )
    .await
    .expect("the hub binds a free port");
    let hub_url = format!("ws://{}", hub.local_addr());
    let handle = hub.handle();
    let serving = tokio::spawn(async move {
        let _ = hub.serve().await;
    });

    // A name, and the token its approval minted: an unapproved runner claims
    // nothing (E-48), and a runner that gave no name would wear a process
    // identity nobody could have approved in advance.
    let token = store
        .approve_runner("no-listener")
        .expect("the state root mints a token");
    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg(&hub_url)
        .arg("--name")
        .arg("no-listener")
        .env("HERD_TOKEN", token)
        .arg("--data-root")
        .arg(data_root.path())
        // Configured with a mesh address, which is the part of this test that
        // is new: a runner that would not be allowed to serve anything would
        // prove the old assertion and not this one. Loopback, because it is
        // the only address the rule permits on a workstation with one network
        // card, and because its acceptance is a deliberate clause of the rule
        // rather than an accident of it (`mesh.rs`).
        .arg("--mesh-listen")
        .arg(mesh.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the real runner binary runs");
    let pid = runner.id().expect("a spawned process has a pid");

    let deadline = Instant::now() + Duration::from_secs(20);
    while !handle.runner_connected() {
        assert!(
            Instant::now() < deadline,
            "the runner never joined the hub (pid {pid})"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Checked while a job is running, because that is when a runner would
    // plausibly have opened something.
    let requester = herdctl::Config {
        hub_url,
        token: None,
        name: Some("no-listener-requester".to_owned()),
    };
    // Held by two files rather than by a sleep: the job says when it has
    // started and waits to be let go. A 400 ms sleep in front of the check
    // was a guess that the job had started by then — and a host slow enough
    // to lose that guess ran the whole assertion against a runner with
    // nothing in flight, which is precisely the state the comment above says
    // it must not be in. It would have passed (D101).
    let started = data_root.path().join("the-job-has-started");
    let release = data_root.path().join("the-job-may-end");
    let script = format!(
        ": > {}; while [ ! -e {} ]; do sleep 0.05; done",
        started.display(),
        release.display()
    );
    let job = tokio::spawn(async move {
        let _ = herdctl::run_job(
            &requester,
            shell(&script),
            herd_proto::LabelSet::default(),
            herd_proto::Priority::Step,
            herdctl::Declared::default(),
            |_, _| {},
            |_| {},
        )
        .await;
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    while !started.exists() {
        assert!(
            Instant::now() < deadline,
            "the job never started, so there was nothing to check the runner against"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let listening = listening_sockets_of(pid);
    // Every one of them is the configured mesh address…
    for held in &listening {
        assert!(
            same_socket(*held, mesh),
            "the runner is listening on {held}, which is not the mesh address {mesh} its \
             configuration named: invariant 3 permits one private mesh address and nothing \
             else, never a default or public interface (D11). All of them: {listening:?}"
        );
    }
    // …and there is at most one, so a second listener is a failure even when
    // it is bound somewhere the rule would have permitted.
    assert!(
        listening.len() <= 1,
        "the runner holds {} listening sockets ({listening:?}); invariant 3 permits one, on \
         the single configured mesh address",
        listening.len()
    );
    // And there is exactly one, which is the line the endpoint step came here
    // to change. It used to read `listening.is_empty()`, with its own note
    // that the step which binds would have to change it rather than silently
    // satisfy a weaker claim: "at most one" is true of a runner that binds
    // nothing, so a broken endpoint would have passed the two assertions
    // above. This is the half that fails when the socket is missing.
    assert_eq!(
        listening.len(),
        1,
        "this runner was configured with a mesh address and holds {} listening socket(s) \
         ({listening:?}); the fetch endpoint binds exactly one, on that address, and nothing \
         else of this runner ever listens (invariant 3, D10, D11)",
        listening.len()
    );

    std::fs::write(&release, b"checked").expect("the job is let go");
    let _ = job.await;
    let _ = runner.kill().await;
    serving.abort();
}

/// Start the real binary with one `--mesh-listen` value and give back what it
/// said and whether it lived.
///
/// Nothing is bound and no hub is needed: the mesh address is judged before
/// the dial, exactly as a bad data root is (E-45, D11), so a refusal is this
/// process exiting non-zero with the reason on its stderr.
fn startup_with_mesh_listen(label: &str, address: &str) -> (bool, String) {
    let data_root = TempDir::new(label);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        // A hub that is not there. Reached only if the address was accepted,
        // which is not what these cases are about.
        .arg("--hub")
        .arg("ws://127.0.0.1:1")
        .arg("--name")
        .arg(label)
        .arg("--data-root")
        .arg(data_root.path())
        .arg("--mesh-listen")
        .arg(address)
        // A dial this process never waits for: the refusal happens before it.
        .arg("--give-up-after")
        .arg("0")
        .output()
        .expect("the real runner binary runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), said)
}

/// D11's first condition against the real binary: a wildcard is refused at
/// startup, by name, before anything is dialled.
///
/// A wildcard is the one value that would satisfy a naive reading of "bind
/// the mesh address" and break invariant 3 completely: `0.0.0.0` binds the
/// default-route interface along with every other one, which is exactly the
/// interface a port forwarded on a router lands on.
#[test]
fn a_wildcard_mesh_address_refuses_the_runners_startup() {
    for address in ["0.0.0.0:9440", "[::]:9440"] {
        let (lived, said) = startup_with_mesh_listen("mesh-wildcard", address);
        assert!(!lived, "{address} was accepted: {said}");
        assert!(said.contains("--mesh-listen"), "{address}: {said}");
        assert!(said.contains("wildcard"), "{address}: {said}");
        assert!(said.contains("invariant 3"), "{address}: {said}");
    }
}

/// And the third: a globally routable address is refused at startup, by name.
/// Host-independent on purpose — `8.8.8.8` is public wherever this runs, and
/// no interface of this machine has to hold it for the rule to refuse it.
#[test]
fn a_globally_routable_mesh_address_refuses_the_runners_startup() {
    let (lived, said) = startup_with_mesh_listen("mesh-public", "8.8.8.8:9440");
    assert!(!lived, "a public address was accepted: {said}");
    assert!(said.contains("--mesh-listen"), "{said}");
    assert!(said.contains("globally routable"), "{said}");
    // The second half of a refusal an operator can act on: what to do about
    // it. D10 keeps a path for a machine that serves nothing.
    assert!(said.contains("outbound-only"), "{said}");
}

/// An advertised address with nothing listening on it is a promise this
/// runner cannot keep, so it is refused rather than ignored.
#[test]
fn an_advertised_mesh_address_without_a_listen_address_refuses_the_startup() {
    let data_root = TempDir::new("mesh-advertise-alone");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg("ws://127.0.0.1:1")
        .arg("--name")
        .arg("mesh-advertise-alone")
        .arg("--data-root")
        .arg(data_root.path())
        .arg("--mesh-advertise")
        .arg("10.9.0.3:9440")
        .arg("--give-up-after")
        .arg("0")
        .output()
        .expect("the real runner binary runs");
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "it was accepted: {said}");
    assert!(said.contains("--mesh-advertise"), "{said}");
    assert!(said.contains("--mesh-listen"), "{said}");
}

/// The module that may name the one listener type invariant 3 permits. One
/// file, by name, and the test below is where the name lives.
const MAY_NAME_A_LISTENER: &str = "mesh.rs";

/// The same promise read off the code, so that it holds on every host and not
/// only where `/proc` can be walked: one named module of the runner may reach
/// for the one listener type D11 permits, and no other module may reach for
/// anything that binds at all.
///
/// **This guard is stronger than the one it replaces, not weaker.** It used
/// to forbid `TcpListener`, `UdpSocket`, `UnixListener` and `UnixDatagram`
/// everywhere in the crate. That was a true statement about a runner with no
/// fetch endpoint, and the moment one is built it becomes a statement nobody
/// can keep — and the usual way it stops being kept is an `#[allow]`, a
/// renamed import or a path spelled `net::TcpListener` so the substring no
/// longer matches. What replaces it is:
///
/// - `TcpListener` is permitted in **exactly one** file, named here, and
///   asserted to really be named there — so the permit cannot rot into a
///   name nobody uses while the real listener grows somewhere else;
/// - `UdpSocket`, `UnixListener` and `UnixDatagram` stay forbidden
///   **everywhere**, that one file included: a runner's peer path is one TCP
///   endpoint on one mesh address, and nothing in Herd has any business
///   opening a datagram socket or a Unix socket;
/// - every other file of the crate is forbidden all four, as before.
///
/// "One named module" is a tighter statement than "nowhere" plus an exception
/// nobody wrote down, which is what the first step of the pack half would
/// otherwise have created: the exception is now a file name in a test, and
/// moving the listener to a second module is a red suite rather than a
/// reviewer's good memory.
#[test]
fn no_module_of_the_runner_can_bind_a_socket() {
    // Forbidden everywhere, the permitted module included.
    let forbidden = ["UdpSocket", "UnixListener", "UnixDatagram"];
    // Forbidden everywhere but the one module named above.
    let permitted_once = "TcpListener";
    // The detector, before the claim: the same matching the walk uses, made
    // to fire on a line that is known to be bad. A source walk that matched
    // nothing would pass this test on a crate that bound a socket in every
    // module.
    assert_eq!(
        names_in(
            code_of("    let socket = tokio::net::UdpSocket::bind(addr).await?;"),
            &forbidden
        ),
        vec!["UdpSocket"],
        "the matching this test is built on does not fire on a line that binds"
    );
    assert!(
        names_in(
            code_of("    // UdpSocket is what this module must never reach for"),
            &forbidden
        )
        .is_empty(),
        "a name inside a comment is not a module reaching for it"
    );

    let sources = herd_runner::testing::source_files();
    assert!(sources.len() > 5, "the walk found {} files", sources.len());
    let mut checked = 0usize;
    let mut named_the_listener: Vec<String> = Vec::new();
    for (path, text) in sources {
        // This file is the test of the rule, so it is allowed to name what it
        // forbids; nothing else in the crate is.
        if path
            .file_name()
            .is_some_and(|name| name == "confinement.rs")
        {
            continue;
        }
        let may_name_a_listener = path
            .file_name()
            .is_some_and(|name| name == MAY_NAME_A_LISTENER);
        checked += 1;
        for (number, line) in text.lines().enumerate() {
            let code = code_of(line);
            let reached_for = names_in(code, &forbidden);
            assert!(
                reached_for.is_empty(),
                "{}:{} names {reached_for:?}: a runner dials out, and the one socket it may \
                 ever bind is a TCP endpoint on its mesh address (invariant 3, D11): {line}",
                path.display(),
                number + 1
            );
            if code.contains(permitted_once) {
                assert!(
                    may_name_a_listener,
                    "{}:{} names {permitted_once}: the one module of this crate that may is \
                     {MAY_NAME_A_LISTENER}, which owns the mesh-address rule and the single \
                     endpoint invariant 3 permits (D11). Bind through \
                     `mesh::MeshListener` from there rather than reaching for the type \
                     here: {line}",
                    path.display(),
                    number + 1
                );
                named_the_listener.push(path.display().to_string());
            }
        }
    }
    assert!(checked > 4, "only {checked} files were checked");
    // The permit is a door with a name on it, so the name must lead
    // somewhere: a `MAY_NAME_A_LISTENER` nothing matches would quietly become
    // "forbidden everywhere" again, and the next listener would go in
    // whichever file was convenient.
    assert!(
        !named_the_listener.is_empty(),
        "no file of this crate names {permitted_once}, so the permit for \
         {MAY_NAME_A_LISTENER} points at nothing. Either that module owns the one listener \
         invariant 3 permits, or this test's permit should be removed"
    );
    assert!(
        named_the_listener
            .iter()
            .all(|path| path.ends_with(MAY_NAME_A_LISTENER)),
        "{permitted_once} is named outside {MAY_NAME_A_LISTENER}: {named_the_listener:?}"
    );
}

/// What is left of a source line once its trailing comment is cut off, which
/// is the only part that can reach for a type. A doc comment or a whole-line
/// comment leaves nothing.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// Which of `names` the code on this line reaches for. Separated out, with
/// `code_of`, so that the matching the guard above rests on can itself be
/// made to fire on a line known to be bad and to stay silent on a comment.
fn names_in<'a>(code: &str, names: &[&'a str]) -> Vec<&'a str> {
    names
        .iter()
        .copied()
        .filter(|name| code.contains(name))
        .collect()
}

// ---------------------------------------------- no Docker socket (D78) ----

/// D78's last clause, and one of M16's deliverables: while a job is
/// **genuinely running**, no open descriptor of the runner — and none of the
/// job's own process — resolves to a container-runtime socket.
///
/// The claim is about what Herd opens, never about what the host forbids, so
/// nothing here consults a group: this server's user is in `docker` because
/// the hub runs under compose, which means the socket is sitting there
/// writable, and the assertion reads the same on a host where it is not.
/// Where the host cannot prove a part of the claim — no such socket on it at
/// all, or none this user could have opened anyway — that part is skipped out
/// loud with what a host that could run it would have proved (D27).
///
/// The paths come from `privilege::runtime_sockets`, the list the production
/// code already warns about, so a path added there is a path this test starts
/// watching rather than a second list to keep in step.
///
/// Only the job's own process is walked, not its children: a child inherits
/// its descriptors from that process, which is checked, and a job that opened
/// the socket itself would be the consumer's own code doing it rather than
/// Herd (E-45 is what bounds that, and this is D78).
#[tokio::test]
async fn no_descriptor_of_the_runner_or_its_job_reaches_a_runtime_socket() {
    const TEST: &str = "no_descriptor_of_the_runner_or_its_job_reaches_a_runtime_socket";
    if !cfg!(target_os = "linux") {
        skipped(
            TEST,
            "this host is not Linux, so /proc/<pid>/fd cannot be walked",
        );
        return;
    }
    let watched = watched_runtime_sockets();
    assert!(
        !watched.is_empty(),
        "privilege::runtime_sockets() named nothing, so this test would watch nothing"
    );
    // Before anything is asserted about the runner: the walk is made to find
    // a descriptor planted on purpose, so that an empty result below is a
    // fact about the runner and not about a walk that finds nothing ever.
    the_walk_finds_a_planted_descriptor(TEST);
    say_what_this_host_cannot_prove(TEST, &watched);

    let data_root = TempDir::new("no-docker-fd");
    let state = TempDir::new("no-docker-fd-state");
    let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
    let hub = Hub::bind(
        "127.0.0.1:0".parse().expect("a literal loopback address"),
        Arc::clone(&store) as Arc<dyn JobStore>,
    )
    .await
    .expect("the hub binds a free port");
    let hub_url = format!("ws://{}", hub.local_addr());
    let handle = hub.handle();
    let serving = tokio::spawn(async move {
        let _ = hub.serve().await;
    });

    // The real binary, because the claim is about a runner *process*: an
    // in-process runner would share this test's own descriptors and prove
    // nothing about either of them. A name of its own, and the token its
    // approval minted (E-48), so that no other test's reaper or restart ever
    // takes this runner's job for a leftover of its own.
    let token = store
        .approve_runner("no-docker-fd")
        .expect("the state root mints a token");
    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg(&hub_url)
        .arg("--name")
        .arg("no-docker-fd")
        .env("HERD_TOKEN", token)
        .arg("--data-root")
        .arg(data_root.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the real runner binary runs");
    let runner_pid = runner.id().expect("a spawned process has a pid");

    let deadline = Instant::now() + Duration::from_secs(20);
    while !handle.runner_connected() {
        assert!(
            Instant::now() < deadline,
            "the runner never joined the hub (pid {runner_pid})"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Held open by two files, as the no-listener test holds its own: the job
    // says when it has started and then waits to be let go, so the walk
    // happens with the job in flight. A window with no job in it has no
    // descriptors to find and would pass on its own (D101).
    let started = data_root.path().join("the-job-has-started");
    let release = data_root.path().join("the-job-may-end");
    let script = format!(
        ": > {}; while [ ! -e {} ]; do sleep 0.05; done",
        started.display(),
        release.display()
    );
    let requester = herdctl::Config {
        hub_url,
        token: None,
        name: Some("no-docker-fd-requester".to_owned()),
    };
    let job = tokio::spawn({
        let command = shell(&script);
        async move {
            let _ = herdctl::run_job(
                &requester,
                command,
                herd_proto::LabelSet::default(),
                herd_proto::Priority::Step,
                herdctl::Declared::default(),
                |_, _| {},
                |_| {},
            )
            .await;
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    while !started.exists() {
        assert!(
            Instant::now() < deadline,
            "the job never started, so there was nothing to walk the runner against"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The job's own process is the shell the runner executed, and the whole
    // script is one of its arguments — unique, because the paths in it are in
    // a data root of this process's own.
    let job_pids = processes_with_argument(&script);
    assert_eq!(
        job_pids.len(),
        1,
        "expected exactly one live process to be running the job's script, found \
         {job_pids:?}"
    );

    let mut unaccounted = Vec::new();
    for (whose, pid) in [("the runner", runner_pid), ("the job", job_pids[0])] {
        let found = runtime_socket_descriptors_of(pid, &watched);
        assert!(
            found.on_a_runtime_socket.is_empty(),
            "{whose} (pid {pid}) holds a descriptor on a container-runtime socket \
             while a job runs, which D78 says never happens: {}",
            found.on_a_runtime_socket.join("; ")
        );
        unaccounted.extend(
            found
                .unix_sockets_nothing_could_account_for
                .iter()
                .map(|descriptor| format!("{whose} (pid {pid}) {descriptor}")),
        );
    }
    if !unaccounted.is_empty() {
        skipped(
            TEST,
            &format!(
                "these AF_UNIX descriptors could be checked by path and by nothing else, \
                 because the kernel's socket diagnostics did not account for them and \
                 procfs publishes the path of a bound socket but never the peer of a \
                 connected one: {}. A host where `ss -xa` answers for every socket would \
                 have proved each of these is not dialled at a container-runtime socket \
                 either",
                unaccounted.join("; ")
            ),
        );
    }

    std::fs::write(&release, b"walked").expect("the job is let go");
    let _ = job.await;
    let _ = runner.kill().await;
    serving.abort();
}

// ------------------------------------- a runner's own death (defect 5) ----

/// A hub, and a *real* runner binary joined to it, so that a signal can be
/// sent to a process rather than to a task.
struct RealRunner {
    hub_url: String,
    process: tokio::process::Child,
    stderr: JoinHandle<String>,
}

/// The name the real runner in these tests answers to, and therefore the name
/// its approval is minted for (E-48).
const REAL_RUNNER_NAME: &str = "the-runner-under-test";

impl RealRunner {
    async fn join(
        hub_url: String,
        handle: herd_hub::HubHandle,
        data_root: &Path,
        token: &str,
    ) -> Self {
        let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
            .arg("--hub")
            .arg(&hub_url)
            .arg("--name")
            .arg(REAL_RUNNER_NAME)
            .env("HERD_TOKEN", token)
            .arg("--data-root")
            .arg(data_root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("the real runner binary runs");
        let mut pipe = process.stderr.take().expect("stderr was piped");
        // Read as it comes: the startup report is what the reaper's own
        // account of itself lands in, and it must not fill a pipe buffer.
        let stderr = tokio::spawn(async move {
            let mut said = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut said).await;
            String::from_utf8_lossy(&said).into_owned()
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the real runner never joined");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Self {
            hub_url,
            process,
            stderr,
        }
    }

    fn pid(&self) -> u32 {
        self.process.id().expect("a spawned process has a pid")
    }

    /// Submit a job and stop following it: these tests are about what happens
    /// to the job when the runner dies, so nobody waits for a verdict.
    fn submit(&self, script: &str) -> JoinHandle<()> {
        let requester = herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: None,
            name: Some("runner-death-requester".to_owned()),
        };
        let command = shell(script);
        tokio::spawn(async move {
            let _ = herdctl::run_job(
                &requester,
                command,
                herd_proto::LabelSet::default(),
                herd_proto::Priority::Step,
                herdctl::Declared::default(),
                |_, _| {},
                |_| {},
            )
            .await;
        })
    }
}

/// A hub on a free port, with its serving task and the state root it keeps
/// its records in (M2). The caller holds the directory: dropping it removes
/// the records, so no test of confinement leaves a state root behind.
async fn hub_on_a_free_port() -> (String, herd_hub::HubHandle, JoinHandle<()>, TempDir, String) {
    let state = TempDir::new("hub-state");
    let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
    // The runner these tests start is approved here, before the hub serves:
    // an unapproved runner claims nothing (E-48). One name, so that a runner
    // started again on the same data root is the same machine coming back.
    let token = store
        .approve_runner(REAL_RUNNER_NAME)
        .expect("the state root mints a token");
    let hub = Hub::bind(
        "127.0.0.1:0".parse().expect("a literal loopback address"),
        store as Arc<dyn JobStore>,
    )
    .await
    .expect("the hub binds a free port");
    let hub_url = format!("ws://{}", hub.local_addr());
    let handle = hub.handle();
    let serving = tokio::spawn(async move {
        let _ = hub.serve().await;
    });
    (hub_url, handle, serving, state, token)
}

/// A runner asked to stop ends its jobs. Without this, `systemctl stop` or a
/// Ctrl-C left the job's processes running, reparented to init, with the
/// workspace on disk and nobody to collect either.
#[tokio::test]
async fn a_runner_told_to_stop_ends_its_jobs_rather_than_orphaning_them() {
    let marker = &a_sleep_nobody_else_is_sleeping(2);
    let data_root = TempDir::new("sigterm");
    let (hub_url, handle, serving, _hub_state, token) = hub_on_a_free_port().await;
    let mut runner = RealRunner::join(hub_url, handle, data_root.path(), &token).await;
    let job = runner.submit(&format!(
        "setsid sleep {marker} >/dev/null 2>&1 & sleep 120"
    ));
    wait_for_a_process_with(marker).await;

    send_signal(runner.pid(), libc::SIGTERM);
    let ended = tokio::time::timeout(Duration::from_secs(10), runner.process.wait())
        .await
        .expect("a runner asked to stop does not hang")
        .expect("the runner's own exit is readable");
    assert!(ended.success(), "a runner told to stop left by {ended:?}");

    let survivors = processes_with_argument(marker);
    assert!(
        survivors.is_empty(),
        "{survivors:?} outlived the runner that was watching them"
    );
    let left = nothing_left_in(data_root.path());
    assert!(left.is_empty(), "the stopped runner left {left:?} on disk");
    let said = runner.stderr.await.unwrap_or_default();
    // What it says is M3's drain, which is where M4's stop now lives: the
    // signal that asked, how many jobs it had to end, and that it is over.
    // The job itself is ended by its own task — that is what produces an
    // honest `attempt_abandoned` rather than a verdict nobody earned — and
    // the backstop in `LiveJobs::end_all` only speaks when a task did not
    // get there.
    assert!(
        said.contains("SIGTERM") && said.contains("draining"),
        "a runner must say that it is stopping and what asked it to: {said}"
    );
    assert!(
        said.contains("job(s) to end") && said.contains("herd-runner: stopped"),
        "and that it ended what it was running, and is gone: {said}"
    );
    job.abort();
    serving.abort();
}

/// `SIGKILL` cannot be handled, so the cleanup has to happen in the next
/// life: a runner reclaims the processes, the cgroups and the workspaces its
/// predecessor could not, and says what it reclaimed.
#[tokio::test]
async fn a_restarted_runner_reclaims_what_a_killed_one_left() {
    let marker = &a_sleep_nobody_else_is_sleeping(3);
    let data_root = TempDir::new("sigkill");
    let (hub_url, handle, serving, _hub_state, token) = hub_on_a_free_port().await;
    let first = RealRunner::join(hub_url.clone(), handle.clone(), data_root.path(), &token).await;
    let job = first.submit(&format!(
        "setsid sleep {marker} >/dev/null 2>&1 & sleep 120"
    ));
    wait_for_a_process_with(marker).await;
    let workspaces = nothing_left_in(data_root.path());
    assert_eq!(
        workspaces.len(),
        1,
        "the job should have exactly one workspace while it runs: {workspaces:?}"
    );

    send_signal(first.pid(), libc::SIGKILL);
    let mut first = first;
    let _ = first.process.wait().await;
    job.abort();
    // The premise, asserted rather than assumed: a killed runner cannot clean
    // up after itself, so its workspace is still there. Only the runner
    // holding this root's lock can reclaim it, so this is not racy — unlike
    // the job's *processes*, which any other runner starting on this host
    // will reap, that being the very mechanism under test. (Their reaping is
    // asserted deterministically in `reclaim.rs`'s unit tests.)
    assert!(
        !nothing_left_in(data_root.path()).is_empty(),
        "a killed runner cannot have reclaimed its own workspace"
    );
    // The hub holds one runner slot in M1, so the second runner can only join
    // once the hub has noticed the first one is gone.
    let deadline = Instant::now() + Duration::from_secs(10);
    while handle.runner_connected() {
        assert!(
            Instant::now() < deadline,
            "the hub never let go of the killed runner's slot"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let second = RealRunner::join(hub_url, handle, data_root.path(), &token).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !processes_with_argument(marker).is_empty() {
        assert!(
            Instant::now() < deadline,
            "the restarted runner left {:?} running",
            processes_with_argument(marker)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let left = nothing_left_in(data_root.path());
    assert!(
        left.is_empty(),
        "the restarted runner left its predecessor's workspaces: {left:?}"
    );

    let mut second = second;
    send_signal(second.pid(), libc::SIGTERM);
    let _ = tokio::time::timeout(Duration::from_secs(10), second.process.wait()).await;
    let said = second.stderr.await.unwrap_or_default();
    assert!(
        said.contains("reaped at startup"),
        "a reaper that says nothing is a reaper nobody can trust: {said}"
    );
    assert!(
        said.contains("reclaimed the workspace"),
        "the report must name what it reclaimed: {said}"
    );
    serving.abort();
}

// ------------------------------------------------------------- helpers ----

#[cfg(unix)]
fn send_signal(pid: u32, signal: i32) {
    // Safe: one signal to one pid this test spawned itself.
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

#[cfg(not(unix))]
fn send_signal(_pid: u32, _signal: i32) {}

/// Every job workspace still on disk under a data root.
fn nothing_left_in(data_root: &Path) -> Vec<PathBuf> {
    let jobs = data_root.join("jobs");
    std::fs::read_dir(&jobs)
        .unwrap_or_else(|error| panic!("{} cannot be read: {error}", jobs.display()))
        .flatten()
        .map(|entry| entry.path())
        .collect()
}

/// Every live process whose command line carries this argument. Used to find
/// a job's detached children: a sleep of 444441 seconds is nobody else's.
/// A sleep duration **this test process** alone is sleeping.
///
/// The sweep below finds a process by one of its arguments, so the argument
/// has to belong to this process and to nothing else. A constant does not:
/// `busy_probe.rs` declared two of these same figures, and a second checkout
/// of this repository running its own suite declares all of them — either
/// one turns the sweep into an assertion about somebody else`s leavings
/// (D101).
fn a_sleep_nobody_else_is_sleeping(which: u32) -> String {
    (1_000_000 + std::process::id() * 10 + which).to_string()
}

fn processes_with_argument(needle: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if cmdline
            .split(|byte| *byte == 0)
            .any(|argument| argument == needle.as_bytes())
        {
            found.push(pid);
        }
    }
    found
}

async fn wait_for_a_process_with(needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while processes_with_argument(needle).is_empty() {
        assert!(
            Instant::now() < deadline,
            "the fixture never started a process carrying {needle}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("the mode of {} cannot be set: {error}", path.display()));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

/// This test process's own niceness, read the same way the job reads its own.
fn own_nice() -> i32 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            stat.split_whitespace()
                .nth(18)
                .and_then(|nice| nice.parse().ok())
        })
        .unwrap_or(0)
}

/// Whether a pid is a live `sleep`. The command name is checked as well as
/// the pid, because a pid alone could have been reused by something else
/// between the job's death and this read.
fn is_a_live_sleep(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|comm| comm.trim() == "sleep")
        .unwrap_or(false)
}

/// Every local address a process holds a listening socket on, parsed, so that
/// a caller can compare one against the address a configuration named.
fn listening_sockets_of(pid: u32) -> Vec<std::net::SocketAddr> {
    let mut inodes = HashSet::new();
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        panic!("the runner's descriptors cannot be read; the test asserts nothing");
    };
    for entry in entries.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            let target = target.to_string_lossy().into_owned();
            if let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|rest| rest.strip_suffix(']'))
            {
                inodes.insert(inode.to_owned());
            }
        }
    }

    let mut listening = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // local_address is field 1, st is field 3, inode is field 9.
            // State 0A is TCP_LISTEN.
            if fields.len() > 9 && fields[3] == "0A" && inodes.contains(fields[9]) {
                let address = proc_net_address(fields[1]).unwrap_or_else(|| {
                    panic!(
                        "{table} printed a local address this test cannot read: {}",
                        fields[1]
                    )
                });
                listening.push(address);
            }
        }
    }
    listening
}

/// The `local_address` column of `/proc/net/tcp` and `/proc/net/tcp6`, which
/// is not a printed address but the kernel's own bytes in hex.
///
/// Eight hex digits for IPv4 and thirty-two for IPv6, each four-byte group in
/// the host's byte order, then a colon and the port in hex. `0100007F:1F90`
/// is `127.0.0.1:8080`, and reading it as `1.0.0.127` is the mistake this
/// function exists to make once rather than at every call site.
fn proc_net_address(field: &str) -> Option<std::net::SocketAddr> {
    let (host, port) = field.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let word = |group: &[u8]| u32::from_str_radix(std::str::from_utf8(group).ok()?, 16).ok();
    match host.len() {
        8 => {
            let raw = word(host.as_bytes())?;
            let address = std::net::Ipv4Addr::from(raw.to_le_bytes());
            Some(std::net::SocketAddr::new(address.into(), port))
        }
        32 => {
            let mut octets = [0u8; 16];
            for (index, group) in host.as_bytes().chunks(8).enumerate() {
                octets[index * 4..index * 4 + 4].copy_from_slice(&word(group)?.to_le_bytes());
            }
            let address = std::net::Ipv6Addr::from(octets);
            Some(std::net::SocketAddr::new(address.into(), port))
        }
        _ => None,
    }
}

/// Whether two sockets are the same endpoint, `::ffff:` and all.
///
/// A listener on `127.0.0.1` can be reported in `/proc/net/tcp6` as
/// `::ffff:127.0.0.1` when the socket was opened on a dual-stack descriptor,
/// and a comparison that read those as two different addresses would have
/// reported a permitted listener as a forbidden one.
fn same_socket(held: std::net::SocketAddr, against: std::net::SocketAddr) -> bool {
    let plain = |address: std::net::SocketAddr| match address.ip() {
        std::net::IpAddr::V6(inner) => match inner.to_ipv4_mapped() {
            Some(mapped) => std::net::SocketAddr::new(mapped.into(), address.port()),
            None => address,
        },
        std::net::IpAddr::V4(_) => address,
    };
    plain(held) == plain(against)
}

/// Every path a descriptor must never resolve to: the container-runtime
/// sockets the runner's own code lists (D78), under both names each of them
/// has on this host.
fn watched_runtime_sockets() -> Vec<PathBuf> {
    both_names_of(
        herd_runner::privilege::runtime_sockets()
            .iter()
            .map(PathBuf::from),
    )
}

/// Each path as it is written and as it canonicalises, deduplicated, so that
/// `/var/run/docker.sock` and `/run/docker.sock` — two names for one socket
/// on most hosts — are one thing to compare against, whichever name a
/// descriptor happens to carry.
fn both_names_of(paths: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut watched: Vec<PathBuf> = Vec::new();
    for listed in paths {
        let canonical = listed.canonicalize().unwrap_or_else(|_| listed.clone());
        for name in [listed, canonical] {
            if !watched.contains(&name) {
                watched.push(name);
            }
        }
    }
    watched
}

/// What a walk of one process's open descriptors found about D78's claim.
#[derive(Debug)]
struct FoundDescriptors {
    /// Descriptors that reach a watched socket, each as one line naming which
    /// descriptor and how it got there. Any one of these is a failure.
    on_a_runtime_socket: Vec<String>,
    /// Descriptors on an AF_UNIX socket that this host could not be made to
    /// account for: procfs names the path of a socket that is **bound** and
    /// never the peer of one that is **connected**, so a connected descriptor
    /// needs the kernel's socket diagnostics, and those are not everywhere.
    /// Not a failure — it is what this host cannot answer, and the caller
    /// says so out loud rather than quietly (D27).
    unix_sockets_nothing_could_account_for: Vec<String>,
}

/// Walk `/proc/<pid>/fd`, resolve every link, and compare what it reached.
///
/// Three shapes, because a descriptor reaches a unix socket in three ways and
/// `/proc/<pid>/fd` alone shows only the first:
///
/// 1. a descriptor on the socket **file** resolves to its path, so the path
///    itself is compared, canonicalised;
/// 2. a descriptor on a **bound** socket resolves to `socket:[inode]` and
///    names no path — `/proc/net/unix` holds the path against that inode;
/// 3. a descriptor **connected** to a socket resolves to `socket:[inode]`
///    too, and procfs publishes no peer for it at all. The kernel gives the
///    accepted end of a connection the listener's own address, so the peer's
///    line in `/proc/net/unix` carries the path that was dialled — and the
///    peer's inode comes from the kernel's socket diagnostics, which is what
///    `ss -x` reads. Where those cannot be asked, such a descriptor is
///    reported as unaccounted for instead of being passed over.
///
/// `/proc/net/unix` lists every AF_UNIX socket on the host and nothing else,
/// which is also how an inode of one is told from a TCP socket's.
fn runtime_socket_descriptors_of(pid: u32, watched: &[PathBuf]) -> FoundDescriptors {
    let unix_sockets = unix_socket_paths();
    let peers = unix_socket_peers();
    let mut found = FoundDescriptors {
        on_a_runtime_socket: Vec::new(),
        unix_sockets_nothing_could_account_for: Vec::new(),
    };
    let watches = |path: &str| {
        let named = PathBuf::from(path);
        let canonical = named.canonicalize().unwrap_or_else(|_| named.clone());
        watched.contains(&named) || watched.contains(&canonical)
    };
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).unwrap_or_else(|error| {
        panic!(
            "the descriptors of pid {pid} cannot be read ({error}), so this test is \
             asserting nothing"
        )
    });
    for entry in entries.flatten() {
        let fd = entry.file_name().to_string_lossy().into_owned();
        // A descriptor closed while the directory was being read: gone, so
        // held by nothing.
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let shown = target.to_string_lossy().into_owned();
        let Some(inode) = shown
            .strip_prefix("socket:[")
            .and_then(|rest| rest.strip_suffix(']'))
        else {
            // Shape 1: an ordinary descriptor, which names where it points.
            let canonical = target.canonicalize().unwrap_or_else(|_| target.clone());
            if watched.contains(&target) || watched.contains(&canonical) {
                found.on_a_runtime_socket.push(format!(
                    "fd {fd} -> {shown} (canonically {})",
                    canonical.display()
                ));
            }
            continue;
        };
        let Some(bound_to) = unix_sockets.get(inode) else {
            // Not AF_UNIX at all, so none of these paths.
            continue;
        };
        // Shape 2: this socket's own name, where it has one.
        if !bound_to.is_empty() {
            if watches(bound_to) {
                found
                    .on_a_runtime_socket
                    .push(format!("fd {fd} -> socket:[{inode}] bound to {bound_to}"));
            }
            continue;
        }
        // Shape 3: whatever it is connected to, named by its peer.
        let Some(peers) = peers.as_ref() else {
            found
                .unix_sockets_nothing_could_account_for
                .push(format!("fd {fd} -> socket:[{inode}]"));
            continue;
        };
        let Some(peer) = peers.get(inode) else {
            found
                .unix_sockets_nothing_could_account_for
                .push(format!("fd {fd} -> socket:[{inode}]"));
            continue;
        };
        // Nobody on the other end — a socket that is listening, or one nobody
        // has dialled — or a peer with no name of its own: either way this
        // descriptor reaches no path, and its own name was compared above.
        let Some(dialled) = unix_sockets.get(peer).filter(|path| !path.is_empty()) else {
            continue;
        };
        if watches(dialled) {
            found.on_a_runtime_socket.push(format!(
                "fd {fd} -> socket:[{inode}] connected to socket:[{peer}], which is \
                 {dialled}"
            ));
        }
    }
    found
}

/// The path of every AF_UNIX socket on this host, by inode, empty for the
/// ones the kernel publishes no path for. `/proc/net/unix` is `Num RefCount
/// Protocol Flags Type St Inode Path`, behind one header line.
fn unix_socket_paths() -> HashMap<String, String> {
    let mut by_inode = HashMap::new();
    let Ok(text) = std::fs::read_to_string("/proc/net/unix") else {
        return by_inode;
    };
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 7 {
            continue;
        }
        by_inode.insert(
            fields[6].to_owned(),
            fields.get(7).copied().unwrap_or_default().to_owned(),
        );
    }
    by_inode
}

/// The peer inode of every connected AF_UNIX socket, by inode, or `None`
/// where this host cannot be asked.
///
/// Asked of `ss -xa`, which reads the kernel's socket diagnostics over
/// netlink: that is the only place a connected socket's peer appears, and
/// without it a descriptor dialled at a socket is a descriptor with no name.
/// A socket with no peer — listening, or never dialled — is in the table with
/// `*` where its peer would be, which is enough to account for it.
fn unix_socket_peers() -> Option<HashMap<String, String>> {
    let shown = std::process::Command::new("ss").arg("-xa").output().ok()?;
    if !shown.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&shown.stdout).into_owned();
    let mut by_inode = HashMap::new();
    for line in text.lines() {
        // netid state recv-q send-q local-address local-inode peer-address
        // peer-inode [process], where an address is a path or `*` and an
        // inode of 0 means there is nobody on the other end. The header line
        // has no netid of this shape and is left out by that alone.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 || !fields[0].starts_with("u_") {
            continue;
        }
        let (Ok(inode), Ok(peer)) = (fields[5].parse::<u64>(), fields[7].parse::<u64>()) else {
            continue;
        };
        by_inode.insert(
            inode.to_string(),
            if peer == 0 {
                String::new()
            } else {
                peer.to_string()
            },
        );
    }
    // An empty table is a tool that answered without answering; treating it
    // as an answer would turn "nothing could be asked" into "nothing found".
    if by_inode.is_empty() {
        return None;
    }
    Some(by_inode)
}

/// The parts of D78's claim this host may be unable to prove, each skipped
/// out loud with what a host that could would have proved (D27). The
/// assertion itself runs either way: it is about what the runner opens, not
/// about what the host allows.
fn say_what_this_host_cannot_prove(test: &str, watched: &[PathBuf]) {
    let present: Vec<String> = watched
        .iter()
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect();
    if present.is_empty() {
        skipped(
            test,
            &format!(
                "none of the {} container-runtime socket paths the runner's code lists \
                 exists on this host, so an empty walk cannot tell a runner that never \
                 opens the Docker socket from a host that has no socket to open. A host \
                 running Docker would have proved the first",
                watched.len()
            ),
        );
        return;
    }
    if !present
        .iter()
        .any(|path| this_user_can_write(Path::new(path)))
    {
        skipped(
            test,
            &format!(
                "the container-runtime sockets on this host ({}) are not writable by \
                 this user, so a descriptor on one was out of reach anyway. A host whose \
                 user is in the docker group — this server is one, because the hub runs \
                 under compose — would have proved the runner declines a socket it could \
                 have used",
                present.join(", ")
            ),
        );
    }
}

/// Whether this user may write a path, asked of the kernel rather than
/// guessed from the mode bits — the same question `privilege` asks, asked
/// here about the host instead of about the runner.
#[cfg(unix)]
fn this_user_can_write(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Safe: access(2) reads one path and returns an integer.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn this_user_can_write(_path: &Path) -> bool {
    false
}

/// The whole of `sun_path` in a `sockaddr_un`, its terminating NUL included:
/// 108 bytes on Linux, and nothing about it is negotiable. `bind(2)` fails
/// with `ENAMETOOLONG` on a path one byte over it — a failure about where a
/// fixture put its socket, never about the runner.
#[cfg(unix)]
const SUN_PATH_BYTES: usize = 108;

/// The room `ShortDir` keeps inside `SUN_PATH_BYTES` for the name of
/// whatever is bound in it, so that a base can be judged short enough before
/// any particular socket is named.
#[cfg(unix)]
const LONGEST_BOUND_NAME: usize = 24;

/// A directory short enough to bind a Unix domain socket in, chosen by this
/// fixture rather than inherited from the environment, and removed on drop.
///
/// Why not `TempDir`: it lives under `std::env::temp_dir()`, and every agent
/// in this project runs with a scratchpad `TMPDIR` of about 131 bytes, onto
/// which `nix develop` nests a prefix of its own. A socket bound there
/// overruns `sun_path` and the test goes red for a reason that has nothing to
/// do with confinement. Measured on 2026-10-29: a **28**-byte `TMPDIR` inside
/// a nested `nix develop` became 62, and that was already enough to tip the
/// total over — so the trigger is the total length of the bound path, and a
/// short `TMPDIR` is no protection.
///
/// So the fixture takes that total out of the environment's hands: it binds
/// under the first base it finds that is short enough, is a directory, and
/// this user can write to — the per-user runtime directory, then `/tmp` — and
/// falls back to the ambient temporary directory only where there is none. In
/// that last case the assertion at the bind site says so with the figure,
/// instead of leaving `bind(2)` to fail and look like a defect.
#[cfg(unix)]
struct ShortDir {
    path: PathBuf,
}

#[cfg(unix)]
impl ShortDir {
    /// A fresh directory named after `label`, unique per process and call.
    fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let leaf = format!("herd-{label}-{}-{unique}", std::process::id());
        let ambient = std::env::temp_dir();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        // What a base may spend: `sun_path` less the '/' before the bound
        // name, the name itself, and its terminating NUL. Written as the
        // base's room rather than as a sum against the ceiling, because
        // `x + 1 <= y` is `clippy::int_plus_one` and the subtraction says
        // the same thing in the fixture's own terms.
        let room_for_a_base = SUN_PATH_BYTES - LONGEST_BOUND_NAME - 2;
        let mut chosen = ambient.join(leaf.as_str());
        for base in runtime
            .iter()
            .map(PathBuf::as_path)
            .chain([Path::new("/tmp"), ambient.as_path()])
        {
            let candidate = base.join(leaf.as_str());
            let fits = candidate.as_os_str().len() <= room_for_a_base;
            if fits && base.is_dir() && this_user_can_write(base) {
                chosen = candidate;
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&chosen);
        std::fs::create_dir_all(&chosen).expect("the fixture's short directory is creatable");
        Self { path: chosen }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for ShortDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The walk run against descriptors this test plants on purpose, so that an
/// empty result about the runner is a fact about the runner rather than about
/// a walk that could never find anything.
///
/// Planted in *this* test process and against a stand-in path in a temporary
/// directory of its own: no production path is touched and the real assertion
/// stays exactly as strict as it was. One plant per shape the walk claims to
/// see — the file, under the name of a **symlink** to it, which is the
/// `/var/run` against `/run` case the canonicalisation exists for; a **bound**
/// socket, which names no path in `/proc/<pid>/fd`; and a socket
/// **connected** to one, which names no path anywhere and is found through
/// its peer. Each is then closed and the walk must stop reporting it, because
/// a walk that always finds something is no better than one that never does.
#[cfg(unix)]
fn the_walk_finds_a_planted_descriptor(test: &str) {
    let temp = TempDir::new("planted-descriptor");
    let me = std::process::id();

    let real = temp.path().join("stand-in.sock");
    std::fs::write(&real, b"a stand-in for a socket file").expect("the fixture writes");
    let by_another_name = temp.path().join("by-another-name.sock");
    std::os::unix::fs::symlink(&real, &by_another_name).expect("the fixture links");
    let watched = both_names_of(std::iter::once(by_another_name));
    let held = std::fs::File::open(&real).expect("the fixture opens its own stand-in");
    let found = runtime_socket_descriptors_of(me, &watched);
    assert!(
        found
            .on_a_runtime_socket
            .iter()
            .any(|line| line.starts_with("fd ") && line.contains(&real.display().to_string())),
        "the walk missed the descriptor this test planted on {}, or did not name it and \
         where it resolved: {found:?}",
        real.display()
    );
    drop(held);
    let after = runtime_socket_descriptors_of(me, &watched);
    assert!(
        after.on_a_runtime_socket.is_empty(),
        "the walk reports a descriptor that is closed: {after:?}"
    );

    // This one is *bound*, so its path has to fit in `sun_path`: it goes in a
    // directory of the fixture's own rather than in `temp`, which inherits
    // whatever `TMPDIR` the suite was launched with.
    let short = ShortDir::new("planted-socket");
    let planted = short.path().join("planted.sock");
    assert!(
        planted.as_os_str().len() < SUN_PATH_BYTES,
        "the fixture is about to bind a path of {} bytes, and `sun_path` holds \
         {SUN_PATH_BYTES} including its terminating NUL: bind(2) would fail with \
         ENAMETOOLONG, and this test would be red about where its own socket went \
         rather than about the runner. Every short base this fixture knows was \
         refused or too long, so it fell back to the ambient temporary directory: {}",
        planted.as_os_str().len(),
        planted.display()
    );
    let watched = both_names_of(std::iter::once(planted.clone()));
    let listener =
        std::os::unix::net::UnixListener::bind(&planted).expect("the fixture binds a socket");
    let found = runtime_socket_descriptors_of(me, &watched);
    assert!(
        found
            .on_a_runtime_socket
            .iter()
            .any(|line| line.starts_with("fd ") && line.contains(&planted.display().to_string())),
        "the walk missed the socket this test bound at {}, which only /proc/net/unix \
         names: {found:?}",
        planted.display()
    );

    // The third shape, and the only one neither `/proc/<pid>/fd` nor
    // `/proc/net/unix` can answer: a descriptor *connected* to the socket,
    // which is how a Docker client would hold it.
    let dialled =
        std::os::unix::net::UnixStream::connect(&planted).expect("the fixture dials its socket");
    let accepted = listener.accept().expect("the fixture accepts its own dial");
    if unix_socket_peers().is_some() {
        let found = runtime_socket_descriptors_of(me, &watched);
        assert!(
            found
                .on_a_runtime_socket
                .iter()
                .any(|line| line.contains("connected to")),
            "the walk missed the descriptor this test dialled at {}, so a descriptor \
             connected to a container-runtime socket would not have been found either: \
             {found:?}",
            planted.display()
        );
    } else {
        skipped(
            test,
            &format!(
                "the kernel's socket diagnostics cannot be asked on this host — `ss -xa` \
                 is missing or answered nothing — and they are the only place a connected \
                 AF_UNIX socket's peer appears. A descriptor the runner had *dialled* at \
                 {} would not have been found by this walk, and this test planted one to \
                 prove it. A host with iproute2 would have proved that shape too",
                planted.display()
            ),
        );
    }
    drop(accepted);
    drop(dialled);
    drop(listener);
    std::fs::remove_file(&planted).expect("the fixture removes its socket");
    let after = runtime_socket_descriptors_of(me, &watched);
    assert!(
        after.on_a_runtime_socket.is_empty(),
        "the walk reports a socket that is closed: {after:?}"
    );
}

/// Linux-only; the test that calls it has already returned on any other host.
#[cfg(not(unix))]
fn the_walk_finds_a_planted_descriptor(_test: &str) {}

// ------------------------------------------------- pressure (E-32) ----

/// The figure travels with the verdict, which is the whole shape of D119's
/// answer: the first consumer asked for PSI to **brake** admission, and E-32
/// — their own expectation — says a load figure is recorded and never a
/// criterion. So it is measured, it reaches the record, and it decides
/// nothing.
#[tokio::test]
async fn a_job_s_record_carries_the_pressure_it_ran_under() {
    let harness = Harness::new("pressure", |_| {}).await;
    let ran = harness.run("echo measured").await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    if !Path::new("/proc/pressure").is_dir() {
        skipped(
            "a_job_s_record_carries_the_pressure_it_ran_under",
            "this kernel has no /proc/pressure (CONFIG_PSI), so there is no figure to \
             record — a kernel with PSI would prove that the host's cpu, memory and io \
             stall figures travel with the job's outcome. The figure is absent here, which \
             is asserted, rather than reported as a zero",
        );
        assert_eq!(
            ran.metrics.pressure, None,
            "a host with no PSI must report no figure at all, never a row of zeroes"
        );
        return;
    }
    let pressure = ran
        .metrics
        .pressure
        .expect("a kernel with PSI records the figure with the job");
    // The host's three are there, and each one is a share of time.
    for (name, figure) in pressure.named() {
        assert!(
            figure <= 100_000,
            "{name} is {figure} thousandths of a percent, which is not a share of time"
        );
    }
    assert!(
        pressure.host_cpu_milli.is_some()
            && pressure.host_memory_milli.is_some()
            && pressure.host_io_milli.is_some(),
        "{pressure:?}"
    );
    // The job's own three come out of its own cgroup, so they are there
    // exactly where there was a cgroup to read.
    match kernel_ceiling_or_reason() {
        Ok(()) => assert!(
            pressure.job_cpu_milli.is_some()
                && pressure.job_memory_milli.is_some()
                && pressure.job_io_milli.is_some(),
            "a job with a cgroup of its own carries its own stall figures too: {pressure:?}"
        ),
        Err(why) => skipped(
            "a_job_s_record_carries_the_pressure_it_ran_under",
            &format!(
                "{why}, so this job had no cgroup of its own and only the host's three \
                 figures could be asserted — a delegated scope (`systemd-run --user \
                 --scope -p Delegate=yes cargo test ...`) would prove that the job's own \
                 cpu.pressure, memory.pressure and io.pressure are read out of its own \
                 cgroup and recorded with it"
            ),
        ),
    }
}

/// **The guarantee E-32 asks for, and the important test of the two.**
///
/// Nothing is ever refused or delayed because this host is under pressure. A
/// hub that silently stopped dispatching under load would be
/// indistinguishable from a hub that had died, which is a worse failure than
/// the queueing it would prevent (D119).
///
/// Asserted by making the host really stall and then asking for work anyway:
/// a job that keeps every thread busy and writes synchronously runs
/// *alongside* a second job, and that second job is admitted, runs, explains
/// no wait and returns its verdict. The recorded figure is what proves the
/// host was in fact stalled while that happened — so this is not an
/// assertion about an idle machine.
#[tokio::test]
async fn pressure_refuses_nothing_and_delays_nothing() {
    let harness = Harness::new("pressure-gates-nothing", |confinement| {
        confinement.timeout = Duration::from_secs(30);
    })
    .await;
    // The hog: every thread spinning, and a synchronous write loop under it,
    // for about two seconds. Bounded on purpose — a runner must never be the
    // reason its host falls over, test or not (invariant 2) — and inside its
    // own job's caps, so the kernel stops it whatever it does.
    let hog = harness.run(
        "i=0; while [ $i -lt 8 ]; do (end=$(( $(date +%s) + 2 )); \
           while [ $(date +%s) -lt $end ]; do :; done) & i=$((i+1)); done; \
         j=0; while [ $j -lt 40 ]; do dd if=/dev/zero of=./churn bs=256k count=4 \
           conv=fsync 2>/dev/null; j=$((j+1)); done; wait; echo hog done",
    );
    // And the question, asked while that is happening.
    let asking = harness.run("echo admitted under pressure");
    let (hog, asked) = tokio::join!(hog, asking);
    assert_eq!(
        hog.outcome,
        JobOutcome::Exited { code: 0 },
        "the hog itself must finish inside its caps: {} / {}",
        hog.out,
        hog.err
    );
    // Refused nothing: the job ran and returned its own verdict.
    assert_eq!(
        asked.outcome,
        JobOutcome::Exited { code: 0 },
        "a job asked for while this host was stalled was not run: {} / {}",
        asked.out,
        asked.err
    );
    assert!(
        asked.out.contains("admitted under pressure"),
        "{}",
        asked.out
    );
    // Delayed nothing: every wait the hub makes a job serve is explained in a
    // note (E-04), and there is no note here about load, pressure or a
    // stalled host — because no such reason exists anywhere in Herd.
    for note in &asked.notes {
        let said = note.to_lowercase();
        assert!(
            !said.contains("pressure") && !said.contains("stall"),
            "a job was made to wait for pressure, which E-32 forbids: {note}"
        );
    }
    // And the figure that proves the host really was stalled while all that
    // was allowed to happen. Where it could not be read, or where this host
    // would not stall at all, the skip says what a stalling host would have
    // shown — never a weakened claim.
    let highest = asked
        .metrics
        .pressure
        .map(|pressure| {
            pressure
                .named()
                .into_iter()
                .map(|(_, figure)| figure)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    if highest == 0 {
        skipped(
            "pressure_refuses_nothing_and_delays_nothing",
            "this host recorded no stall at all while eight spinning threads and a \
             synchronous write loop ran, so the job above was admitted on a machine that \
             was never under pressure: it proves that nothing is refused or delayed, but \
             not that the figure it would have been refused for was non-zero. A host with \
             CONFIG_PSI that can be made to stall would have shown both at once",
        );
    }
}

/// And the same guarantee read off the code rather than off one run: **no
/// module anywhere in Herd compares a pressure figure against anything.**
///
/// The behavioural test above proves one job was not refused on one host.
/// This one proves there is no code that could refuse one: the figure is
/// read in `metrics`, carried in `herd-proto`'s `JobMetrics`, handed to the
/// record once in `job`, and mentioned nowhere a decision is taken. It is
/// the shape of `no_module_but_this_one_can_reach_an_escalation_tool`, and
/// it is here because E-32 is a promise about Herd and not about a Tuesday.
#[test]
fn nothing_in_herd_reads_a_pressure_figure_to_decide_anything() {
    // Where a decision about a job is taken: admission, placement, claiming,
    // queueing. Every one of these is a file the figure must not appear in.
    let deciders = [
        "herd-hub/src/matcher.rs",
        "herd-hub/src/queue.rs",
        "herd-hub/src/server.rs",
        "herd-runner/src/room.rs",
        "herd-runner/src/capability.rs",
        "herd-runner/src/switch.rs",
    ];
    let Some(workspace) = workspace_root() else {
        skipped(
            "nothing_in_herd_reads_a_pressure_figure_to_decide_anything",
            "the workspace root could not be found from this test's manifest directory, so \
             the hub's own sources could not be read — a checkout would have proved that no \
             module that decides anything about a job so much as mentions the figure",
        );
        return;
    };
    let mut read = 0;
    for decider in deciders {
        let path = workspace.join(decider);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        read += 1;
        for (number, line) in text.lines().enumerate() {
            // A comment saying the figure gates nothing is the opposite of a
            // gate, and is exactly what should be written there.
            let code = line.split("//").next().unwrap_or_default();
            assert!(
                !code.contains("pressure"),
                "{}:{} reads a pressure figure where a job is decided about, which E-32 \
                 forbids: {line}",
                path.display(),
                number + 1
            );
        }
    }
    assert!(
        read >= 4,
        "only {read} of the {} files that decide about a job could be read, so this \
         assertion did not cover what it claims",
        deciders.len()
    );
}

/// The workspace root, found by walking up from this crate until `Cargo.toml`
/// names a workspace. `CARGO_MANIFEST_DIR` is the crate, and the hub is a
/// sibling of it.
fn workspace_root() -> Option<PathBuf> {
    let mut here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if std::fs::read_to_string(here.join("Cargo.toml"))
            .is_ok_and(|text| text.contains("[workspace]"))
        {
            return Some(here);
        }
        if !here.pop() {
            return None;
        }
    }
}
