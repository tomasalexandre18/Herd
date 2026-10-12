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

use std::collections::HashSet;
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
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::new(JobDirStore::open(state.path()).expect("the state root opens"))
                as Arc<dyn JobStore>,
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
                token: None,
                confinement,
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
        let finished =
            herdctl::run_job(
                &self.requester(),
                shell(script),
                |stream, data| match stream {
                    LogStream::Stdout => out.push_str(&String::from_utf8_lossy(data)),
                    LogStream::Stderr => err.push_str(&String::from_utf8_lossy(data)),
                },
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
            limit: 1
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
            limit: cap
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
            limit: budget
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
            limit: budget
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
            limit: budget
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
            limit: cap
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
            limit: cap
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
    let marker = "444441";
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
                limit: 64 << 20
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

    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg(&hub_url)
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
    let job = tokio::spawn(async move {
        let _ = herdctl::run_job(&requester, shell("sleep 1.5"), |_, _| {}).await;
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    let listening = listening_sockets_of(pid);
    assert!(
        listening.is_empty(),
        "the runner is listening on {listening:?}; invariant 3 says it dials out \
         and binds nothing (the mesh endpoint of D10/D11 arrives in M11)"
    );

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

// ------------------------------------- a runner's own death (defect 5) ----

/// A hub, and a *real* runner binary joined to it, so that a signal can be
/// sent to a process rather than to a task.
struct RealRunner {
    hub_url: String,
    process: tokio::process::Child,
    stderr: JoinHandle<String>,
}

impl RealRunner {
    async fn join(hub_url: String, handle: herd_hub::HubHandle, data_root: &Path) -> Self {
        let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
            .arg("--hub")
            .arg(&hub_url)
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
            let _ = herdctl::run_job(&requester, command, |_, _| {}).await;
        })
    }
}

/// A hub on a free port, with its serving task and the state root it keeps
/// its records in (M2). The caller holds the directory: dropping it removes
/// the records, so no test of confinement leaves a state root behind.
async fn hub_on_a_free_port() -> (String, herd_hub::HubHandle, JoinHandle<()>, TempDir) {
    let state = TempDir::new("hub-state");
    let hub = Hub::bind(
        "127.0.0.1:0".parse().expect("a literal loopback address"),
        Arc::new(JobDirStore::open(state.path()).expect("the state root opens"))
            as Arc<dyn JobStore>,
    )
    .await
    .expect("the hub binds a free port");
    let hub_url = format!("ws://{}", hub.local_addr());
    let handle = hub.handle();
    let serving = tokio::spawn(async move {
        let _ = hub.serve().await;
    });
    (hub_url, handle, serving, state)
}

/// A runner asked to stop ends its jobs. Without this, `systemctl stop` or a
/// Ctrl-C left the job's processes running, reparented to init, with the
/// workspace on disk and nobody to collect either.
#[tokio::test]
async fn a_runner_told_to_stop_ends_its_jobs_rather_than_orphaning_them() {
    let marker = "444442";
    let data_root = TempDir::new("sigterm");
    let (hub_url, handle, serving, _hub_state) = hub_on_a_free_port().await;
    let mut runner = RealRunner::join(hub_url, handle, data_root.path()).await;
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
    assert!(
        said.contains("SIGTERM received") && said.contains("ended job"),
        "a runner must say what it ended on its way out: {said}"
    );
    job.abort();
    serving.abort();
}

/// `SIGKILL` cannot be handled, so the cleanup has to happen in the next
/// life: a runner reclaims the processes, the cgroups and the workspaces its
/// predecessor could not, and says what it reclaimed.
#[tokio::test]
async fn a_restarted_runner_reclaims_what_a_killed_one_left() {
    let marker = "444443";
    let data_root = TempDir::new("sigkill");
    let (hub_url, handle, serving, _hub_state) = hub_on_a_free_port().await;
    let first = RealRunner::join(hub_url.clone(), handle.clone(), data_root.path()).await;
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
    // The premise, asserted rather than assumed: a killed runner orphans
    // everything, which is why the next one has to clean up.
    assert!(
        !processes_with_argument(marker).is_empty(),
        "nothing was orphaned, so this test is not testing the reaper"
    );
    assert!(!nothing_left_in(data_root.path()).is_empty());
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

    let second = RealRunner::join(hub_url, handle, data_root.path()).await;
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
        said.contains("reclaimed the workspace") && said.contains("orphaned job process"),
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
