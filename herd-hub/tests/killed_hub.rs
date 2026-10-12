//! E-44 against the real binaries: kill the hub, mid-job, with a signal.
//!
//! The in-process tests cannot reach this. Dropping a hub's runtime ends the
//! tasks that were serving a runner, but the runner in those tests is a task
//! in the *same* process, and the socket it was reading dies with a kind of
//! politeness a real kill does not have. What a killed hub does to a runner on
//! another machine is close the connection under it with a reset — and the
//! defect this file exists to forbid is the runner taking that as the end of
//! its life, exiting, killing the job it was running, and leaving the job's
//! record reading `running` for ever (D17).
//!
//! So everything here is a real process: `herd-hub`, `herd-runner` and
//! `herdctl`, as a person would run them, with the hub killed by a signal and
//! started again on the same state root and the same port.

use std::fs::File;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tokio::process::{Child, Command};

/// How long any wait here may take before it is a failure. Generous because
/// CI is slow, and never a substitute for waiting on a real condition.
const PATIENCE: Duration = Duration::from_secs(30);

/// A process the test started, killed when the test ends however it ends —
/// so a failure never leaves a hub or a runner behind.
struct Process {
    what: &'static str,
    child: Child,
}

impl Process {
    /// Whether it is still running. The whole point of D17: a runner whose hub
    /// was killed must answer `true` here.
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    async fn signal(&mut self, signal: rustix::process::Signal) {
        let pid = self.child.id().expect("a running process has a pid");
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid as i32).expect("a pid is not zero"),
            signal,
        )
        .unwrap_or_else(|e| panic!("signalling the {}: {e}", self.what));
        // Reaped here, so that the port and the state root's lock are free
        // before anything tries to take them again.
        let _ = self.child.wait().await;
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Where the workspace's binaries are, once they are known to be the ones the
/// sources say they should be.
///
/// `cargo test` does not build a workspace *binary* unless some test target
/// needs it, and this test's needs are invisible to it: it runs `herd-runner`
/// and `herdctl` as programs, not as libraries. A stale `herd-runner` from an
/// earlier build would then be the one this test drives — and a test of the
/// real binaries that silently drives yesterday's binary is worse than no
/// test at all. So the build is asked for here, once per test process, and
/// failing to build is failing the test.
fn binaries() -> &'static Path {
    static BINARIES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARIES.get_or_init(|| {
        let directory = Path::new(env!("CARGO_BIN_EXE_herd-hub"))
            .parent()
            .expect("a binary lives in a directory")
            .to_path_buf();
        let mut cargo = std::process::Command::new(env!("CARGO"));
        cargo.args([
            "build",
            "--offline",
            "-p",
            "herd-hub",
            "-p",
            "herd-runner",
            "-p",
            "herdctl",
        ]);
        // The same profile as the test itself, whichever it is.
        if directory.file_name().is_some_and(|name| name == "release") {
            cargo.arg("--release");
        }
        let built = cargo.status().expect("cargo runs");
        assert!(
            built.success(),
            "the binaries this test drives could not be built"
        );
        directory
    })
}

fn binary(name: &str) -> PathBuf {
    let path = binaries().join(name);
    assert!(
        path.is_file(),
        "{} is not built; this test runs the real binaries",
        path.display()
    );
    path
}

/// A port nothing is listening on, kept for the hub that is restarted: a
/// runner dials one URL, so the hub that comes back has to take the same one.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
    listener
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

struct Herd {
    state: TempDir,
    logs: TempDir,
    port: u16,
}

impl Herd {
    fn new() -> Self {
        Self {
            state: tempfile::tempdir().expect("a temporary state directory"),
            logs: tempfile::tempdir().expect("a temporary directory for the logs"),
            port: free_port(),
        }
    }

    fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    fn addr(&self) -> SocketAddr {
        format!("127.0.0.1:{}", self.port)
            .parse()
            .expect("a literal loopback address")
    }

    /// Start a hub, with its output kept in a file the test can read.
    async fn start_hub(&self, which: &str) -> Process {
        let output = |stream: &str| {
            Stdio::from(
                File::create(self.logs.path().join(format!("{which}-{stream}")))
                    .expect("a file for the hub's output"),
            )
        };
        let child = Command::new(binary("herd-hub"))
            .arg("--listen")
            .arg(self.addr().to_string())
            .arg("--state-dir")
            .arg(self.state.path())
            .stdout(output("out"))
            .stderr(output("err"))
            .kill_on_drop(true)
            .spawn()
            .expect("herd-hub starts");
        let hub = Process { what: "hub", child };
        // Waiting on the port, not on a sleep: the hub is up when it accepts.
        let deadline = Instant::now() + PATIENCE;
        while TcpStream::connect(self.addr()).is_err() {
            assert!(
                Instant::now() < deadline,
                "the hub never listened on {}: {}",
                self.addr(),
                self.said(which)
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        hub
    }

    fn start_runner(&self) -> Process {
        let child = Command::new(binary("herd-runner"))
            .arg("--hub")
            .arg(self.url())
            .arg("--name")
            .arg("the-real-runner")
            .stdout(Stdio::from(
                File::create(self.logs.path().join("runner-out")).expect("a file"),
            ))
            .stderr(Stdio::from(
                File::create(self.logs.path().join("runner-err")).expect("a file"),
            ))
            .kill_on_drop(true)
            .spawn()
            .expect("herd-runner starts");
        Process {
            what: "runner",
            child,
        }
    }

    /// Everything one of the processes has printed so far.
    fn said(&self, which: &str) -> String {
        let mut text = String::new();
        for stream in ["out", "err"] {
            text.push_str(
                &std::fs::read_to_string(self.logs.path().join(format!("{which}-{stream}")))
                    .unwrap_or_default(),
            );
        }
        text
    }

    async fn herdctl(&self, arguments: &[&str]) -> std::process::Output {
        tokio::time::timeout(
            PATIENCE,
            Command::new(binary("herdctl"))
                .arg("--hub")
                .arg(self.url())
                .args(arguments)
                .output(),
        )
        .await
        .unwrap_or_else(|_| panic!("herdctl {arguments:?} never came back"))
        .expect("herdctl runs")
    }

    /// Submit a job and leave with its id, as a push hook would (E-03).
    async fn submit(&self, script: &str) -> String {
        let output = self
            .herdctl(&["submit", "--", "/bin/sh", "-c", script])
            .await;
        assert!(
            output.status.success(),
            "the submission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Wait until the runner has joined, by giving it something to run.
    async fn wait_for_runner(&self) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let output = self
                .herdctl(&["run", "--", "/bin/sh", "-c", "printf ready"])
                .await;
            if output.status.success() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the runner never joined: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn job_dir(&self, job_id: &str) -> PathBuf {
        self.state.path().join("jobs").join(job_id)
    }

    fn record(&self, job_id: &str) -> serde_json::Value {
        let path = self.job_dir(job_id).join("job.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is json: {e}", path.display()))
    }

    fn states_of(&self, job_id: &str) -> Vec<String> {
        let path = self.job_dir(job_id).join("events.jsonl");
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let event: serde_json::Value =
                    serde_json::from_str(line).unwrap_or_else(|e| panic!("{line} is json: {e}"));
                event["state"]["type"]
                    .as_str()
                    .expect("a state has a type")
                    .to_owned()
            })
            .collect()
    }

    fn log(&self, job_id: &str) -> String {
        std::fs::read_to_string(self.job_dir(job_id).join("log")).unwrap_or_default()
    }

    async fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting until {what}; the hub said: {}",
                self.said("first")
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The test that matters: a hub killed while a job is running loses neither
/// the runner, nor the job, nor its result (E-44, D17).
async fn a_killed_hub_loses_nothing(signal: rustix::process::Signal) {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // Long enough to still be running when the hub dies, and loud enough on
    // both sides of the gap to say what survived.
    let job = herd
        .submit("printf 'before the kill\\n'; sleep 4; printf 'after it\\n'; exit 7")
        .await;
    herd.wait_until("the job is running and its first line is recorded", || {
        herd.record(&job)["state"]["type"] == "running"
            && herd.log(&job).contains("before the kill")
    })
    .await;

    // ---- The kill.
    hub.signal(signal).await;
    assert!(!hub.alive(), "the hub is gone");
    // The runner's control connection has just been reset under it. Half a
    // second is far longer than it takes a process to die of that.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        runner.alive(),
        "the runner died with the hub ({signal:?}), so its job died too: {}",
        herd.said("runner")
    );

    // ---- The hub comes back, same port, same state root.
    let mut second = herd.start_hub("second").await;
    assert!(
        herd.said("second").contains("1 job(s) not over"),
        "the hub that comes back inherits the unfinished job: {}",
        herd.said("second")
    );
    assert!(
        herd.said("second")
            .contains("the hub stopped while this job was"),
        "an interrupted attempt must be named, not left reading `running`: {}",
        herd.said("second")
    );

    // A requester that comes back by id alone gets the job's own code and its
    // output, from a hub that never saw the job start (E-03, D16, D22).
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(7),
        "the job's own exit code must survive the kill: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert!(
        String::from_utf8_lossy(&waited.stdout).contains("before the kill"),
        "a wait must print the job's log: {:?}",
        String::from_utf8_lossy(&waited.stdout)
    );

    // And the record is whole: the end as the runner reported it, the
    // interruption named before it, the log still there.
    let record = herd.record(&job);
    assert_eq!(record["state"]["type"], "finished");
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 7})
    );
    let states = herd.states_of(&job);
    assert_eq!(
        states,
        ["queued", "assigned", "running", "interrupted", "finished"],
        "every transition is on the record, in order"
    );
    assert!(
        herd.log(&job).contains("before the kill"),
        "the log the hub wrote before it died: {:?}",
        herd.log(&job)
    );

    // The runner is still a runner, on the hub that came back.
    let after = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'still here\\n'"])
        .await;
    assert_eq!(after.status.code(), Some(0), "{}", herd.said("runner"));
    assert_eq!(String::from_utf8_lossy(&after.stdout), "still here\n");

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_killed_outright_mid_job_loses_nothing() {
    a_killed_hub_loses_nothing(rustix::process::Signal::KILL).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_asked_to_stop_mid_job_loses_nothing() {
    a_killed_hub_loses_nothing(rustix::process::Signal::TERM).await;
}

/// D19, with the real binaries: a hub that cannot write a result does not
/// destroy it, and does not drop the runner that is holding it.
///
/// `chmod 500` on a running job's directory is the cheapest honest stand-in
/// for a full disk: every write into that directory fails, and the result the
/// runner is about to report has nowhere to go.
#[tokio::test(flavor = "multi_thread")]
async fn a_result_the_hub_cannot_write_is_kept_until_it_can() {
    use std::os::unix::fs::PermissionsExt;

    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 1; exit 3").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // Nothing can be written in the job's directory from here.
    let dir = herd.job_dir(&job);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("read-only");
    if herd_hub::durable::probe_writable(&dir).is_ok() {
        // Running as a user permissions do not apply to; there is nothing to
        // keep from being written.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable");
        return;
    }

    herd.wait_until("the hub has failed to record the outcome", || {
        herd.said("first").contains("cannot write it")
    })
    .await;
    // The record could not move on, the hub said so in full, and — this is
    // the point — the runner was not dropped for the hub's own failure.
    assert_eq!(herd.record(&job)["state"]["type"], "running");
    assert!(
        herd.said("first").contains("NOT acknowledged"),
        "the hub must say the result is still the runner's: {}",
        herd.said("first")
    );
    assert!(
        runner.alive(),
        "the runner was dropped: {}",
        herd.said("runner")
    );
    let sibling = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'still serving\\n'"])
        .await;
    assert_eq!(
        sibling.status.code(),
        Some(0),
        "the runner's connection did not survive: {}",
        String::from_utf8_lossy(&sibling.stderr)
    );

    // The disk comes back, and so does the hub. The runner still holds the
    // result and redelivers it: a stalled job is recoverable, a destroyed
    // outcome would not have been.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable again");
    hub.signal(rustix::process::Signal::TERM).await;
    let mut second = herd.start_hub("second").await;
    herd.wait_until("the held result is recorded at last", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    assert_eq!(
        herd.record(&job)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 3}),
        "the outcome the runner held is the outcome recorded"
    );

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}
