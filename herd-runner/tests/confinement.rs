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
//! beginning `HERD-SKIPPED:`, CI runs the suite with `--nocapture`, and
//! `scripts/report-skips.sh` turns each of those lines into a warning on the
//! run. So the difference between "asserted" and "not asserted here" is
//! visible on the pull request rather than buried.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::{Hub, JobStore, MemoryJobStore};
use herd_proto::{Cap, JobOutcome, LogStream};
use herd_runner::testing::TempDir;
use herd_runner::{Confined, Confinement};
use tokio::task::JoinHandle;

/// A hub with one runner joined, confined as the test asks.
struct Harness {
    hub_url: String,
    data_root: TempDir,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    async fn new(label: &str, shape: impl FnOnce(&mut Confinement)) -> Self {
        let data_root = TempDir::new(label);
        let mut confinement = test_confinement(data_root.path());
        shape(&mut confinement);
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::new(MemoryJobStore::new()) as Arc<dyn JobStore>,
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
                    LogStream::Stdout => out.push_str(data),
                    LogStream::Stderr => err.push_str(data),
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
/// does not. Asked by preparing a confinement exactly as a runner does.
fn kernel_ceiling_or_reason(data_root: &Path) -> Result<(), String> {
    let confined = Confined::prepare(test_confinement(data_root))
        .expect("an unprivileged process with a writable root can confine");
    match confined.ceiling().fallback_reason() {
        None => Ok(()),
        Some(why) => Err(why.to_owned()),
    }
}

/// Print a skip so that nobody can mistake it for an assertion that ran.
/// `scripts/report-skips.sh` turns the marker into a warning in CI.
fn skipped(test: &str, why: &str) {
    println!("HERD-SKIPPED: {test}: {why}");
    eprintln!("HERD-SKIPPED: {test}: {why}");
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
    let real_ceiling = kernel_ceiling_or_reason(harness.root());

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
    let store = Arc::new(MemoryJobStore::new());
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

// ------------------------------------------------------------- helpers ----

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
