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

use herd_hub::{Hub, JobStore, MemoryJobStore};
use tokio::process::Command;
use tokio::task::JoinHandle;

/// A hub with one runner joined, and the means to run `herdctl` against it.
struct Harness {
    hub_url: String,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    async fn new() -> Self {
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
                name: Some("exit-code-runner".to_owned()),
                token: None,
            };
            async move {
                let _ = herd_runner::run(config).await;
            }
        });
        // Wait on the hub's own state, not on a sleep.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Self {
            hub_url,
            tasks: vec![serving, running],
        }
    }

    /// Run `herdctl run -- /bin/sh -c <script>` and collect what a caller sees.
    async fn herdctl_shell(&self, script: &str) -> Output {
        self.herdctl(&["/bin/sh", "-c", script]).await
    }

    async fn herdctl(&self, command: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&self.hub_url)
            .arg("run")
            .arg("--")
            .args(command)
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
    for code in [0, 1, 42, 75, 125, 127] {
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
    let output = harness.herdctl_shell("kill -TERM $$").await;
    assert_eq!(
        status_code(&output),
        143,
        "SIGTERM is 15, so herdctl must exit 143: {}",
        stderr(&output)
    );
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
