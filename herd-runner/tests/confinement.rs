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
use herd_runner::testing::TempDir;
use herd_runner::{Confined, Confinement};
use tokio::task::JoinHandle;

/// A hub with one runner joined, confined as the test asks.
struct Harness {
    hub_url: String,
    data_root: TempDir,
    /// The hub's state root, which M2 made files: one per harness, removed
    /// with it, so no confinement test reads another's records.
    _state: TempDir,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    async fn new(label: &str, shape: impl FnOnce(&mut Confinement)) -> Self {
        let data_root = TempDir::new(label);
        let state = TempDir::new(&format!("{label}-state"));
        let mut confinement = test_confinement(data_root.path());
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
            |_note| {},
        )
        .await
        .expect("the hub answers with an outcome");
        Ran {
            outcome: finished.outcome,
            metrics: finished.metrics,
            out,
            err,
        }
    }

    fn root(&self) -> &Path {
        self.data_root.path()
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

/// Defect 1 of the M4 review, end to end: a job that hides what it wrote
/// behind a directory the runner cannot enter used to be reported
/// `bytes_written: Some(0)`, `Exited { code: 0 }` — with the 100 MB still on
/// the host afterwards, because the removal failed on the same directory.
#[tokio::test]
async fn a_job_cannot_hide_what_it_wrote_from_its_disk_budget() {
    let budget = 8 << 20;
    let harness = Harness::new("blinded-budget", |confinement| {
        confinement.disk_budget = budget;
    })
    .await;
    let ran = harness
        .run(
            "mkdir h; dd if=/dev/zero of=h/big bs=1048576 count=100 2>/dev/null; \
             chmod 300 h; sleep 6",
        )
        .await;
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
     for f in memory.max cpu.weight cpu.max io.weight pids.max; do \
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

/// `io.weight` is the one control the cgroup core cannot tell the truth
/// about: the write succeeds on every host, and only bfq or blk-iocost
/// honours it. A runner that stayed quiet about that would be claiming a
/// bound it does not have, which is exactly what D23 forbids.
#[tokio::test]
async fn an_io_weight_that_cannot_bite_is_said_out_loud() {
    let temp = TempDir::new("io-weight-report");
    let confined = Confined::prepare(test_confinement(temp.path())).expect("confinable");
    let report = confined.report().join("\n");
    let Some(why) = confined.io_weight_inert().map(|why| why.to_owned()) else {
        // bfq or blk-iocost: the weight really bites here, and the report
        // says nothing about it on purpose.
        assert!(
            !report.contains("WRITTEN BUT INERT"),
            "this host weights IO, so the report must not claim otherwise: {report}"
        );
        return;
    };
    if let Some(no_cgroup) = confined.ceiling().fallback_reason() {
        // With no cgroup at all the report already says io.weight is not
        // enforced; the inert sentence would be a second reason for the same
        // thing.
        assert!(report.contains("NOT ENFORCED AT ALL"), "{report}");
        skipped(
            "an_io_weight_that_cannot_bite_is_said_out_loud",
            &format!(
                "{no_cgroup} — this host's io.weight is inert for a second reason too \
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

// ------------------------------------------------- no inbound socket ----

/// Invariant 3, asserted against the running process rather than against a
/// promise: the real runner binary, joined to a hub and running a job, holds
/// no socket in the listening state.
///
/// The mesh-bound fetch endpoint of D10 and D11 arrives in M11; when it does,
/// this test becomes "no socket on a default or public interface" and keeps
/// its shape.
#[tokio::test]
async fn the_runner_binds_no_listening_socket() {
    if !cfg!(target_os = "linux") {
        skipped(
            "the_runner_binds_no_listening_socket",
            "this host is not Linux, so /proc/net/tcp cannot be read",
        );
        return;
    }
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
    assert!(
        listening.is_empty(),
        "the runner is listening on {listening:?}; invariant 3 says it dials out \
         and binds nothing (the mesh endpoint of D10/D11 arrives in M11)"
    );

    std::fs::write(&release, b"checked").expect("the job is let go");
    let _ = job.await;
    let _ = runner.kill().await;
    serving.abort();
}

/// The same promise read off the code, so that it holds on every host and not
/// only where `/proc` can be walked: no module of the runner reaches for
/// anything that binds.
#[test]
fn no_module_of_the_runner_can_bind_a_socket() {
    let forbidden = ["TcpListener", "UdpSocket", "UnixListener", "UnixDatagram"];
    let sources = herd_runner::testing::source_files();
    assert!(sources.len() > 5, "the walk found {} files", sources.len());
    let mut checked = 0usize;
    for (path, text) in sources {
        // This file is the test of the rule, so it is allowed to name what it
        // forbids; nothing else in the crate is.
        if path
            .file_name()
            .is_some_and(|name| name == "confinement.rs")
        {
            continue;
        }
        checked += 1;
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for name in forbidden {
                assert!(
                    !code.contains(name),
                    "{}:{} names {name}: a runner dials out and binds nothing \
                     (invariant 3): {line}",
                    path.display(),
                    number + 1
                );
            }
        }
    }
    assert!(checked > 4, "only {checked} files were checked");
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

/// Every local address a process holds a listening socket on.
fn listening_sockets_of(pid: u32) -> Vec<String> {
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
                listening.push(format!("{table} {}", fields[1]));
            }
        }
    }
    listening
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

    let planted = temp.path().join("planted.sock");
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
