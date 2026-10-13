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

use herd_proto::JobOutcome;
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
    /// The data root the runner of this herd is given (E-45). One per herd,
    /// because a runner locks its root for the life of the process: two of
    /// these tests running at once would otherwise be two runners fighting
    /// over one directory, and the second would refuse to start.
    data_root: TempDir,
    port: u16,
}

impl Herd {
    fn new() -> Self {
        Self {
            state: tempfile::tempdir().expect("a temporary state directory"),
            logs: tempfile::tempdir().expect("a temporary directory for the logs"),
            data_root: tempfile::tempdir().expect("a temporary data root"),
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
            // Its own data root, and the real one: this is the binary the
            // server runs, confinement included (M4).
            .arg("--data-root")
            .arg(self.data_root.path())
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

    /// Start a `herdctl wait` and leave it running, with what it says kept in
    /// a file: a wait that is *meant* not to end cannot be run to completion.
    fn start_wait(&self, job_id: &str) -> Process {
        let child = Command::new(binary("herdctl"))
            .arg("--hub")
            .arg(self.url())
            .args(["wait", job_id])
            .stdout(Stdio::from(
                File::create(self.logs.path().join("wait-out")).expect("a file"),
            ))
            .stderr(Stdio::from(
                File::create(self.logs.path().join("wait-err")).expect("a file"),
            ))
            .kill_on_drop(true)
            .spawn()
            .expect("herdctl starts");
        Process {
            what: "wait",
            child,
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

    /// The transitions on a job's record, in order. A line of `events.jsonl`
    /// that records something else — output that never arrived — is not one.
    fn states_of(&self, job_id: &str) -> Vec<String> {
        let path = self.job_dir(job_id).join("events.jsonl");
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let event: serde_json::Value =
                    serde_json::from_str(line).unwrap_or_else(|e| panic!("{line} is json: {e}"));
                event["state"]["type"].as_str().map(str::to_owned)
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

/// D29, and the one thing a record may never do: a log that is short of
/// output says so, at the place where the output is missing.
///
/// Twelve `kill -9`s of the hub during one job is how the defect was found.
/// The hub came back each time, the runner redialled and went on streaming,
/// and everything the job had printed while there was no hub was simply gone —
/// a quarter of the lines, with the record reading `finished{exited 5}` and
/// nothing anywhere saying a byte was missing. Getting those chunks back is
/// M3's work; saying they are not there is M2's, and that is what the chunk
/// numbers have been carried for since D4.
#[tokio::test(flavor = "multi_thread")]
async fn a_log_that_lost_chunks_to_twelve_killed_hubs_says_where() {
    const KILLS: u32 = 12;

    let herd = Herd::new();
    let mut hub = herd.start_hub("hub-0").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // A line every 50 ms until the test says stop, so output is always being
    // produced when a hub is killed and always resumes when one comes back.
    // `END` makes a whole line recognisable: a lost chunk can cut a line in
    // half, and a half line must not be mistaken for a line of its own.
    let stop = herd.logs.path().join("stop");
    let job = herd
        .submit(&format!(
            "i=0; while [ ! -f '{}' ]; do i=$((i+1)); printf 'line %s END\\n' \"$i\"; \
             sleep 0.05; done; exit 5",
            stop.display()
        ))
        .await;
    herd.wait_until("the job is running and its output is arriving", || {
        herd.record(&job)["state"]["type"] == "running" && herd.log(&job).contains("line 1 END")
    })
    .await;

    // A whole line of the job's own, as the log keeps it. A line a lost chunk
    // cut in half is not one.
    let whole_lines = || herd.log(&job).matches(" END\n").count();

    for round in 1..=KILLS {
        // Enough output between two kills that the log has substance on both
        // sides of every gap, as the job that found this had.
        let so_far = whole_lines();
        herd.wait_until("more of the job's output reaches the hub", || {
            whole_lines() >= so_far + 5
        })
        .await;
        hub.signal(rustix::process::Signal::KILL).await;
        hub = herd.start_hub(&format!("hub-{round}")).await;
    }
    // The output resuming is what makes the last gap visible: the first chunk
    // after a gap is the one whose number does not follow.
    let before_the_end = whole_lines();
    herd.wait_until("the output resumes on the hub that came back", || {
        whole_lines() > before_the_end
    })
    .await;

    // The job ends, on the twelfth hub, and keeps its own exit code.
    File::create(&stop).expect("the stop file");
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(5),
        "the job's own code must survive twelve kills: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert_eq!(herd.record(&job)["state"]["type"], "finished");

    // ---- What the log says about itself. Every jump in the job's own line
    // numbers must have a marker between the two lines that name the jump:
    // that is the whole of it, a log that is short is a log that says so.
    let log = herd.log(&job);
    let mut last: Option<u64> = None;
    let mut announced = false;
    let mut gaps = 0u32;
    let mut lines = 0u32;
    for line in log.lines() {
        if line.contains("never reached this log") {
            announced = true;
            continue;
        }
        let Some(number) = line
            .strip_prefix("line ")
            .and_then(|rest| rest.strip_suffix(" END"))
            .and_then(|digits| digits.parse::<u64>().ok())
        else {
            // A line a lost chunk cut in half, or the blank line the marker
            // stands on. Neither is one of the job's lines.
            continue;
        };
        lines += 1;
        if let Some(last) = last {
            assert!(number > last, "the log went backwards at {number}: {log}");
            if number != last + 1 {
                assert!(
                    announced,
                    "lines {} to {} are missing from the log and nothing says so: \
                     a record that is short of output and silent about it is the \
                     defect this test exists for",
                    last + 1,
                    number - 1
                );
                gaps += 1;
            }
        }
        announced = false;
        last = Some(number);
    }
    assert!(lines > 0, "the log holds none of the job's lines: {log}");
    assert!(
        gaps > 0,
        "twelve killed hubs cost this job no output at all, so nothing was \
         proved; {lines} lines, log: {log}"
    );

    // And the record carries the gaps too, as events of their own kind: a
    // reader of the record alone must not have to parse the log to learn that
    // it is incomplete.
    let markers = log.matches("never reached this log").count();
    assert!(
        markers >= gaps as usize,
        "{gaps} gap(s) in the job's lines and only {markers} marker(s)"
    );
    let path = herd.job_dir(&job).join("events.jsonl");
    let events = std::fs::read_to_string(&path).expect("the events read");
    let recorded = events
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["event"] == "log_gap")
        .count();
    assert_eq!(
        recorded, markers,
        "every marker in the log is an event on the record: {events}"
    );
    assert_eq!(
        herd.states_of(&job).first().map(String::as_str),
        Some("queued"),
        "and the transitions are still the transitions: {events}"
    );

    // Twelve kills, and nothing half written left behind in the job's
    // directory either.
    let litter: Vec<String> = std::fs::read_dir(herd.job_dir(&job))
        .expect("the job directory lists")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(
        litter.is_empty(),
        "a kill left a temporary behind: {litter:?}"
    );

    hub.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// D19's other half, with the real binaries: a wait on a job the hub cannot
/// write down says so, instead of saying nothing at all.
///
/// The record of such a job reads `running` and stays there — the runner holds
/// the result, this hub has stopped trying to store it — and a `herdctl wait`
/// on it used to print absolutely nothing, with no way for a requester to tell
/// a stalled job from a slow one. How long a hub tries before giving up is
/// M3's policy; being mute about it was never a policy.
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_on_a_job_the_hub_cannot_write_down_says_so() {
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

    // Nothing can be written in the job's directory from here, so the outcome
    // the runner is about to report has nowhere to go.
    let dir = herd.job_dir(&job);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("read-only");
    if herd_hub::durable::probe_writable(&dir).is_ok() {
        // Running as a user permissions do not apply to; there is nothing to
        // keep from being written.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable");
        return;
    }

    herd.wait_until("the hub has given up writing the outcome", || {
        herd.said("first").contains("cannot write it")
    })
    .await;
    assert_eq!(
        herd.record(&job)["state"]["type"],
        "running",
        "the record cannot have moved on"
    );

    // A requester comes back to the job by its id. The job is not over, and
    // nothing is going to happen to it: the wait has to say that much.
    let mut waiting = herd.start_wait(&job);
    let said = async {
        loop {
            if herd.said("wait").contains("cannot write it down") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), said)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the wait said nothing about a job the hub cannot write down: {:?}",
                herd.said("wait")
            )
        });
    assert!(
        herd.said("wait").contains(&job),
        "the note must carry the reason, which names the file: {:?}",
        herd.said("wait")
    );
    assert!(
        herd.said("wait").contains("redelivers it"),
        "and what happens next: {:?}",
        herd.said("wait")
    );
    // Still waiting: no verdict has been invented for a job that has one, in
    // the runner's memory, that the hub could not store.
    assert!(waiting.alive(), "the wait must not have answered anything");
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(herd.logs.path().join("wait-out")).unwrap()),
        "working\n",
        "stdout stays the job's own output; the note is Herd speaking, on stderr"
    );

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable again");
    waiting.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// D17's other half: when nothing comes back, the job stays unfinished but is
/// *named* as interrupted, and a wait on it says so instead of hanging mutely
/// for ever.
///
/// The hub and the runner are both killed, so the result exists nowhere: the
/// runner held it in memory and the runner is gone. M2 does not decide that
/// such an attempt is lost — the ~60 s silence threshold and the requeue are
/// M3's — so the wait does go on waiting. What it may not do is wait in
/// silence, which is what it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_on_an_interrupted_job_says_so_rather_than_hanging_mutely() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 30").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // Both ends of the attempt, gone. Nothing can report this job's end.
    hub.signal(rustix::process::Signal::KILL).await;
    runner.signal(rustix::process::Signal::KILL).await;

    let mut second = herd.start_hub("second").await;
    let mut waiting = herd.start_wait(&job);
    let said = async {
        loop {
            if herd.said("wait").contains("interrupted") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), said)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the wait said nothing about a job whose attempt was interrupted: {:?}",
                herd.said("wait")
            )
        });
    assert!(
        herd.said("wait")
            .contains("the hub stopped while this job was"),
        "the note must carry the reason the record holds: {:?}",
        herd.said("wait")
    );
    // Still waiting, as M2 means it to: no verdict has been invented.
    assert!(waiting.alive(), "the wait must not have answered anything");
    assert_eq!(
        herd.record(&job)["state"]["type"],
        "interrupted",
        "the record names the interruption rather than reading `running`"
    );

    waiting.signal(rustix::process::Signal::TERM).await;
    second.signal(rustix::process::Signal::TERM).await;
}

/// Where M4's stop meets M2's held result, with both real binaries: a runner
/// told to stop while it is holding a verdict no hub has acknowledged does
/// **not** take that verdict with it.
///
/// The two promises pull against each other. M4 says a runner asked to stop
/// ends its running jobs — not doing so is how a `sleep` outlives its runner,
/// reparented to init, with its cgroup and its workspace left for nobody. M2
/// says a finished job's outcome is held until the hub acknowledges it,
/// because a destroyed verdict is the one thing Herd never does (D19, E-44) —
/// and the job this stop ends produces exactly such an outcome, with no hub
/// left to take it.
///
/// So the runner writes what it still holds into its data root, one file per
/// job, and names each path on its way out. The file is the `job_finished`
/// frame itself, so nothing has to be reinterpreted to replay it (invariant
/// 4). A verdict on a disk is recoverable; a verdict in the memory of a
/// process that has exited is not.
///
/// **What this leaves to M3**: reading these files back on the next start, so
/// that the hub learns of them with no human in the loop, and the ≤ 5 s stop
/// of invariant 2 with its full drain. This is the minimum honest thing — the
/// verdict survives, and the runner says where it is.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_stopped_while_holding_a_result_leaves_it_on_disk() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 120").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // The hub goes first, so there is nothing left to acknowledge anything.
    hub.signal(rustix::process::Signal::KILL).await;
    assert!(!hub.alive(), "the hub is gone");
    herd.wait_until("the runner has noticed the hub is gone", || {
        herd.said("runner").contains("the link to the hub ended")
    })
    .await;

    // And now the runner is told to stop, with a job still running under it.
    runner.signal(rustix::process::Signal::TERM).await;

    let said = herd.said("runner");
    assert!(
        said.contains("SIGTERM received"),
        "a runner must say it is stopping: {said}"
    );
    let held = herd.data_root.path().join("held-results");
    let spooled: Vec<PathBuf> = std::fs::read_dir(&held)
        .unwrap_or_else(|error| {
            panic!(
                "a runner that stopped holding a result must leave it in {}: \
                 {error}. It said: {said}",
                held.display()
            )
        })
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(
        spooled.len(),
        1,
        "exactly the one unacknowledged result belongs there: {spooled:?}"
    );
    let frame = std::fs::read_to_string(&spooled[0]).expect("the held result reads");
    let message: herd_proto::ToHub = herd_proto::from_frame(&frame).unwrap_or_else(|error| {
        panic!("a held result is a frame; {frame} did not decode: {error}")
    });
    match message {
        herd_proto::ToHub::JobFinished {
            job_id, outcome, ..
        } => {
            assert_eq!(job_id.as_str(), job, "the frame names the job it is about");
            assert!(
                matches!(
                    outcome,
                    JobOutcome::Signalled { .. } | JobOutcome::StoppedByCap { .. }
                ),
                "the job was ended by the stop, so that is what the verdict says: \
                 {outcome:?}"
            );
        }
        other => panic!("a held result is a job_finished frame, not {other:?}"),
    }
    // Impossible to miss: a verdict that reached no hub is said out loud, with
    // the path, never left for someone to discover.
    assert!(
        said.contains("never acknowledged") && said.contains(&held.display().to_string()),
        "the runner must say where the verdict went: {said}"
    );
    // And nothing invented a verdict the hub was never told: the record is
    // not finished. It reads `running` because this hub was killed outright
    // and never got to name the interruption; the next hub on this state root
    // does that, and M3 is what will hand it the file above (D17).
    assert_ne!(
        herd.record(&job)["state"]["type"],
        "finished",
        "nothing may claim a verdict the hub was never told"
    );
}
