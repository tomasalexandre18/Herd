//! `herdctl`'s exit code is outward-facing: a requester's script reads it, so
//! it is pinned here by running the real binary against a real hub and a real
//! runner, and reading the status the shell would have read.
//!
//! The hub binds port 0 and the test discovers the port it got, so the suite
//! never collides with a port in use. Every spawned task is aborted when the
//! harness drops and every child is waited for, so no test leaves a process
//! or a bound port behind.

use std::process::Output;
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::{Hub, JobDirStore, JobStore};
// Two throwaway directories with the same name: the hub's state root and the
// runner's data root are different things and a test must not share them.
use herd_runner::testing::TempDir as DataRootDir;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::task::JoinHandle;

/// A hub with one runner joined, and the means to run `herdctl` against it.
struct Harness {
    hub_url: String,
    /// The hub's state, removed with the harness: a test leaves no files.
    _state: TempDir,
    tasks: Vec<JoinHandle<()>>,
    /// The runner's data root (E-45), removed when the harness drops.
    _data_root: Option<DataRootDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::with_caps(|_| {}).await
    }

    /// A harness whose runner confines jobs as the caller asks, so that a cap
    /// can be made small enough to trip in a test.
    async fn with_caps(shape: impl FnOnce(&mut herd_runner::Confinement)) -> Self {
        Self::start(Some(shape)).await
    }

    /// A hub with nothing to run jobs on, for the refusals that are Herd's own
    /// failure rather than a job's verdict (D20).
    async fn with_no_runner() -> Self {
        Self::start(None::<fn(&mut herd_runner::Confinement)>).await
    }

    async fn start(shape: Option<impl FnOnce(&mut herd_runner::Confinement)>) -> Self {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = JobDirStore::open(state.path()).expect("the state root opens");
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::new(store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        let mut tasks = vec![serving];
        let mut data_root = None;
        if let Some(shape) = shape {
            // One data root per runner, thrown away with the harness: a
            // runner locks its root for the life of the process (E-45), so
            // two tests sharing one would refuse to start.
            let root = DataRootDir::new("exit-code");
            let mut confinement = herd_runner::Confinement::with_data_root(root.path());
            shape(&mut confinement);
            tasks.push(tokio::spawn({
                let config = herd_runner::Config {
                    hub_url: hub_url.clone(),
                    name: Some("exit-code-runner".to_owned()),
                    token: None,
                    confinement,
                };
                async move {
                    let _ = herd_runner::run(config).await;
                }
            }));
            data_root = Some(root);
            // Wait on the hub's own state, not on a sleep.
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.runner_connected() {
                assert!(Instant::now() < deadline, "the runner never joined the hub");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        Self {
            hub_url,
            _state: state,
            tasks,
            _data_root: data_root,
        }
    }

    /// Run `herdctl run -- /bin/sh -c <script>` and collect what a caller sees.
    async fn herdctl_shell(&self, script: &str) -> Output {
        self.herdctl(&["/bin/sh", "-c", script]).await
    }

    async fn herdctl(&self, command: &[&str]) -> Output {
        let mut arguments = vec!["run", "--"];
        arguments.extend_from_slice(command);
        self.herdctl_args(&arguments).await
    }

    /// Run `herdctl` with whatever arguments, as a caller would.
    async fn herdctl_args(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&self.hub_url)
            .args(arguments)
            .output()
            .await
            .expect("herdctl runs")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn status_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("herdctl itself was killed: {:?}", output.status))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The job's own code, untouched. E-02 reads 0 as pass, 1 as fail and 75 as
/// "absent or leased", so none of them may be rewritten on the way out.
#[tokio::test]
async fn herdctl_wears_the_job_s_own_exit_code() {
    let harness = Harness::new().await;
    for code in [0, 1, 42, 75, 125, 127, 255] {
        let output = harness
            .herdctl_shell(&format!("printf 'done\\n'; exit {code}"))
            .await;
        assert_eq!(
            status_code(&output),
            code,
            "herdctl rewrote exit code {code}: {}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), "done\n", "the job's output is herdctl's");
        // 125 is also herdctl's own code for a job it could not run, and 127
        // is what a shell exits with for a command it could not execute. Both
        // are the job's to choose, and neither makes herdctl claim a failure
        // of its own.
        assert!(
            !stderr(&output).contains(&herdctl::could_not_start_message("")),
            "exit code {code} was read as Herd failing to run the job: {}",
            stderr(&output)
        );
    }
}

/// A signalled job needs no sentinel: `128 + signal` is what a shell reports.
#[tokio::test]
async fn herdctl_exits_a_hundred_and_twenty_eight_plus_the_signal() {
    let harness = Harness::new().await;
    for (signal, number, code) in [("TERM", 15, 143), ("KILL", 9, 137)] {
        let output = harness.herdctl_shell(&format!("kill -{signal} $$")).await;
        assert_eq!(
            status_code(&output),
            code,
            "SIG{signal} is {number}, so herdctl must exit {code}: {}",
            stderr(&output)
        );
    }
}

/// Herd failing to run the job is 125, the wrapper's own code, and the reason
/// is said on stderr. Never on stdout: stdout is the job's output alone.
#[tokio::test]
async fn herdctl_exits_125_and_says_on_stderr_that_herd_could_not_run_the_job() {
    let harness = Harness::new().await;
    let missing = "herd-no-such-executable-in-m1";
    let output = harness.herdctl(&[missing]).await;

    assert_eq!(
        status_code(&output),
        125,
        "a job Herd could not run is not a job that failed: {}",
        stderr(&output)
    );
    let said = stderr(&output);
    assert!(
        said.contains(&herdctl::could_not_start_message("")),
        "the message must blame Herd, not the job: {said}"
    );
    assert!(
        said.contains(missing),
        "the message must name what could not be started: {said}"
    );
    assert_eq!(
        stdout(&output),
        "",
        "stdout belongs to the job, which never ran"
    );
}

/// A detached submission prints the id and nothing else, and a later `wait`
/// exits with the job's own code — the same code `run` would have exited
/// with (D16). This is the pair a push hook uses (E-03, E-42).
#[tokio::test]
async fn submit_prints_an_id_and_wait_exits_with_the_job_s_code() {
    let harness = Harness::new().await;
    for code in [0, 1, 75] {
        let submitted = harness
            .herdctl_args(&[
                "submit",
                "--",
                "/bin/sh",
                "-c",
                &format!("printf 'detached\\n'; exit {code}"),
            ])
            .await;
        assert_eq!(
            status_code(&submitted),
            0,
            "a submission that was accepted is a success: {}",
            stderr(&submitted)
        );
        let job_id = stdout(&submitted).trim().to_owned();
        assert!(!job_id.is_empty(), "submit must print the job's id");
        assert_eq!(
            stdout(&submitted),
            format!("{job_id}\n"),
            "stdout carries the id alone, so a script can read it"
        );

        let waited = harness.herdctl_args(&["wait", &job_id]).await;
        assert_eq!(
            status_code(&waited),
            code,
            "a wait must wear the job's own code: {}",
            stderr(&waited)
        );
    }
}

/// D22: a wait prints the job's output, the way `run` does. Without this a
/// requester on another machine cannot get a detached job's log through the
/// CLI at all — only by reading the hub's state directory, which it cannot
/// reach.
#[tokio::test]
async fn wait_prints_the_job_s_log() {
    let harness = Harness::new().await;
    let submitted = harness
        .herdctl_args(&[
            "submit",
            "--",
            "/bin/sh",
            "-c",
            "printf 'on stdout\\n'; printf 'on stderr\\n' >&2; exit 4",
        ])
        .await;
    let job_id = stdout(&submitted).trim().to_owned();

    let waited = harness.herdctl_args(&["wait", &job_id]).await;
    assert_eq!(status_code(&waited), 4, "{}", stderr(&waited));
    // A wait that attaches while the job is still running gets whatever was
    // recorded before it arrived, and then the live chunks on their own
    // streams — so between the two, all of the job's output.
    let said = format!("{}{}", stdout(&waited), stderr(&waited));
    assert!(said.contains("on stdout\n"), "stdout is missing: {said:?}");
    assert!(said.contains("on stderr\n"), "stderr is missing: {said:?}");

    // And again, on a job that was over before this wait began. Now it all
    // comes from the record, whose `log` keeps both streams in arrival order
    // and does not say which was which, so it is reported as the job's output.
    let again = harness.herdctl_args(&["wait", &job_id]).await;
    assert_eq!(status_code(&again), 4, "{}", stderr(&again));
    let recorded = stdout(&again);
    assert!(
        recorded.contains("on stdout\n") && recorded.contains("on stderr\n"),
        "a wait on a job that is over must still print its log: {recorded:?}"
    );
}

/// Waiting on an id that was never minted is a refusal naming it, not a wait
/// that never ends — and it is 125, because there is no verdict to give (D20).
#[tokio::test]
async fn waiting_for_a_job_that_does_not_exist_says_so() {
    let harness = Harness::new().await;
    let output = harness.herdctl_args(&["wait", "j0000009999"]).await;
    assert_eq!(status_code(&output), 125, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no such job: j0000009999"),
        "the refusal must name the id: {}",
        stderr(&output)
    );
}

/// D20: every way Herd itself can fail to produce a verdict exits 125, with
/// the reason on stderr and nothing on stdout. A script that cannot tell "the
/// tests failed" from "the hub was down" would read the second as the first,
/// which is exactly what E-02 and E-43 rest on not happening.
#[tokio::test]
async fn herd_s_own_failures_exit_125_and_never_1() {
    // A hub that cannot be reached at all.
    let nowhere = Command::new(env!("CARGO_BIN_EXE_herdctl"))
        .args(["--hub", "ws://127.0.0.1:1", "run", "--", "/bin/true"])
        .output()
        .await
        .expect("herdctl runs");
    assert_eq!(
        status_code(&nowhere),
        125,
        "a hub that cannot be dialled is not a job that failed: {}",
        stderr(&nowhere)
    );
    assert_eq!(stdout(&nowhere), "", "stdout belongs to the job");
    assert!(
        stderr(&nowhere).contains(&herdctl::no_verdict_message("")),
        "the reason must blame Herd: {}",
        stderr(&nowhere)
    );

    // A hub with nothing to run the job on.
    let harness = Harness::with_no_runner().await;
    for arguments in [
        vec!["run", "--", "/bin/true"],
        vec!["submit", "--", "/bin/true"],
    ] {
        let output = harness.herdctl_args(&arguments).await;
        assert_eq!(
            status_code(&output),
            125,
            "`{}` with no runner must not look like a failing job: {}",
            arguments.join(" "),
            stderr(&output)
        );
        assert_eq!(stdout(&output), "", "stdout belongs to the job");
        assert!(
            stderr(&output).contains("no runner is connected"),
            "the reason must say what Herd could not do: {}",
            stderr(&output)
        );
    }
}

/// A cap breach is 124 — `timeout(1)`'s code, generalised (D25) — with the
/// cap named on stderr, and this is read off the real binary because a
/// requester's script reads it.
///
/// Three of the four caps can be made to trip on any host. The memory
/// ceiling's named outcome needs cgroup v2 delegation and is asserted in
/// `herd-runner`'s own suite, which skips it loudly where it cannot run.
#[tokio::test]
async fn herdctl_exits_124_for_every_cap_a_test_can_trip() {
    // The log cap, breached by a job that then exits 0 straight away: the
    // cap has to outrank the job's own status, or a truncated log reads as a
    // build that passed.
    let harness = Harness::with_caps(|caps| caps.log_cap = 1024).await;
    let output = harness
        .herdctl_shell("dd if=/dev/zero bs=4096 count=1 2>/dev/null | tr '\\0' 'x'; exit 0")
        .await;
    assert_eq!(
        status_code(&output),
        124,
        "a job whose log was cut is not a job that passed: {}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("log cap"), "{}", stderr(&output));
    assert_eq!(
        stdout(&output).len(),
        1024,
        "the log the requester got must be exactly the cap"
    );
    drop(harness);

    // The time cap.
    let harness = Harness::with_caps(|caps| caps.timeout = Duration::from_secs(1)).await;
    let output = harness.herdctl_shell("sleep 120").await;
    assert_eq!(status_code(&output), 124, "{}", stderr(&output));
    assert!(stderr(&output).contains("time cap"), "{}", stderr(&output));
    drop(harness);

    // The disk budget, breached behind a directory the job made unreadable:
    // a budget the runner cannot measure is still a budget it enforces.
    let harness = Harness::with_caps(|caps| caps.disk_budget = 8 << 20).await;
    let output = harness
        .herdctl_shell(
            "mkdir h; dd if=/dev/zero of=h/big bs=1048576 count=40 2>/dev/null; \
             chmod 300 h; exit 0",
        )
        .await;
    assert_eq!(status_code(&output), 124, "{}", stderr(&output));
    assert!(stderr(&output).contains("disk cap"), "{}", stderr(&output));
}
