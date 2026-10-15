//! Driving the real binaries: a hub, its runners and `herdctl`, as processes.
//!
//! `cargo test --workspace` does **not** build a workspace binary unless a
//! test target needs it as a library, and a test that runs `herd-runner` as a
//! *program* has no such need that cargo can see. A stale binary from an
//! earlier build would then be the one under test, which is worse than no
//! test at all — it reads green while proving nothing about the code in front
//! of you. So [`binaries`] asks for the build itself, once per test process,
//! and failing to build fails the test.
//!
//! Shared by the test files that need a real kill, a real restart or a real
//! exit code: `killed_hub.rs` (E-44, D17) and `priorities.rs` (E-18, E-05).
//! Not every file uses every helper here, hence the allowance below: a
//! helper that only one of them needs is still worth having in one place.
#![allow(dead_code)]

use std::fs::File;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tokio::process::{Child, Command};

/// How long any wait here may take before it is a failure. Generous because
/// CI is slow, and never a substitute for waiting on a real condition.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// How often the runners here report they are alive, in seconds.
///
/// Often enough that a test does not spend a minute proving the hub noticed.
/// The 15 s of E-40 is the default; this is the same mechanism on a shorter
/// clock. One figure, given to both sides, because the hub refuses a silence
/// threshold that is not comfortably longer than the cadence it expects — and
/// a test that handed the two sides different figures would be testing a
/// configuration nobody can run.
pub const HEALTH_EVERY_IN_THESE_TESTS: &str = "0.2";

/// A process the test started, killed when the test ends however it ends —
/// so a failure never leaves a hub or a runner behind.
pub struct Process {
    pub what: &'static str,
    pub child: Child,
}

impl Process {
    /// Whether it is still running. The whole point of D17: a runner whose hub
    /// was killed must answer `true` here.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub async fn signal(&mut self, signal: rustix::process::Signal) {
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

    /// Signal it and come straight back, without reaping it.
    ///
    /// For `SIGSTOP` and `SIGCONT`, which a process does not exit on: a paused
    /// process is how a test makes a peer that is plainly there and answers
    /// nothing, which is the only honest way to let an acknowledgement time out
    /// (D99). Waiting for it, as [`Process::signal`] does, would never return.
    pub fn signal_without_waiting(&mut self, signal: rustix::process::Signal) {
        let pid = self.child.id().expect("a running process has a pid");
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid as i32).expect("a pid is not zero"),
            signal,
        )
        .unwrap_or_else(|e| panic!("signalling the {}: {e}", self.what));
    }

    /// Signal it and come straight back, so the caller can time how long it
    /// takes to go. Invariant 2 is a promise about exactly that duration.
    pub fn ask_it_to_stop(&mut self) {
        let pid = self.child.id().expect("a running process has a pid");
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid as i32).expect("a pid is not zero"),
            rustix::process::Signal::TERM,
        )
        .unwrap_or_else(|e| panic!("signalling the {}: {e}", self.what));
    }

    /// How long it took to be gone, and how it went.
    pub async fn gone(&mut self) -> std::process::ExitStatus {
        self.child
            .wait()
            .await
            .unwrap_or_else(|e| panic!("waiting for the {}: {e}", self.what))
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
pub fn binaries() -> &'static Path {
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

pub fn binary(name: &str) -> PathBuf {
    let path = binaries().join(name);
    assert!(
        path.is_file(),
        "{} is not built; this test runs the real binaries",
        path.display()
    );
    path
}

/// The health cadence these tests run both sides at, unless the test under
/// way is about the cadence itself — in which case it gives its own, and this
/// adds nothing: the binaries refuse the same option twice, and rightly.
pub fn cadence_unless_given(options: &[&str]) -> Vec<String> {
    if options.contains(&"--health-every") {
        return Vec::new();
    }
    vec![
        "--health-every".to_owned(),
        HEALTH_EVERY_IN_THESE_TESTS.to_owned(),
    ]
}

/// A port nothing is listening on, kept for the hub that is restarted: a
/// runner dials one URL, so the hub that comes back has to take the same one.
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
    listener
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

pub struct Herd {
    pub state: TempDir,
    pub logs: TempDir,
    /// The data root the runner of this herd is given (E-45). One per herd,
    /// because a runner locks its root for the life of the process: two of
    /// these tests running at once would otherwise be two runners fighting
    /// over one directory, and the second would refuse to start.
    pub data_root: TempDir,
    pub port: u16,
    /// The token each runner of this herd carries, so one name is approved
    /// once however many times it is started (E-48).
    approved: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl Herd {
    pub fn new() -> Self {
        Self {
            state: tempfile::tempdir().expect("a temporary state directory"),
            logs: tempfile::tempdir().expect("a temporary directory for the logs"),
            data_root: tempfile::tempdir().expect("a temporary data root"),
            port: free_port(),
            approved: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Approve one runner through the real command, and come back with the
    /// token it printed (E-48).
    ///
    /// `herdctl runner approve <name>`, as an operator types it once per
    /// machine, against the hub that is already running: an unapproved runner
    /// claims nothing, so every runner these tests start goes through this
    /// first. The token is on stdout alone, which is what makes this one line.
    ///
    /// Blocking rather than `async`, so that starting a runner stays the one
    /// synchronous line it has always been at forty-odd call sites. The hub is
    /// a separate process, so there is nothing here for a runtime to deadlock
    /// on.
    pub fn approve(&self, name: &str) -> String {
        let mut approved = self
            .approved
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        approved
            .entry(name.to_owned())
            .or_insert_with(|| match TcpStream::connect(self.addr()) {
                // The hub is up: the operator's own command, over the wire.
                Ok(_) => {
                    let output = std::process::Command::new(binary("herdctl"))
                        .arg("--hub")
                        .arg(self.url())
                        .args(["runner", "approve", name])
                        .output()
                        .expect("herdctl runs");
                    assert!(
                        output.status.success(),
                        "approving {name} failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    assert!(!token.is_empty(), "an approval prints the token it minted");
                    token
                }
                // No hub yet, which some of these tests want on purpose: a
                // runner started against a port nothing is listening on.
                // Approving is then done on the state root itself, which is
                // also what a deployment does before it starts the hub for
                // the first time — the approval is a file in the root, so the
                // hub reads it when it comes up.
                Err(_) => {
                    use herd_hub::JobStore;
                    herd_hub::JobDirStore::open(self.state.path())
                        .expect("the state root opens")
                        .approve_runner(name)
                        .expect("the state root mints a token")
                }
            })
            .clone()
    }

    /// Forget which runners this herd approved (E-48).
    ///
    /// For the one case where the approvals really are gone: a state root that
    /// has been **recreated**, which is the volume somebody rebuilt. The
    /// tokens minted against the old root mean nothing to the new one, so the
    /// next runner this harness starts is approved again — as an operator
    /// would have to.
    pub fn forget_approvals(&self) {
        self.approved
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    pub fn addr(&self) -> SocketAddr {
        format!("127.0.0.1:{}", self.port)
            .parse()
            .expect("a literal loopback address")
    }

    /// Start a hub, with its output kept in a file the test can read.
    pub async fn start_hub(&self, which: &str) -> Process {
        // A minute, as the deployment has it: these tests are about a hub and
        // a runner dying, not about the threshold.
        self.start_hub_losing_runners_after(which, 60.0).await
    }

    /// The same, with the hub told how long a runner may be unheard from.
    ///
    /// The threshold is the hub's policy rather than a constant for exactly
    /// this reason: a test of what happens 60 s after a runner dies would take
    /// a minute and be the first thing deleted, while the behaviour it proves
    /// is the behaviour whatever the number is.
    pub async fn start_hub_losing_runners_after(&self, which: &str, seconds: f64) -> Process {
        self.start_hub_with(which, &["--lost-after", &seconds.to_string()])
            .await
    }

    /// A hub with whatever options the test wants, and its output in files.
    ///
    /// The health cadence goes with the threshold, because the hub refuses a
    /// threshold that is not comfortably longer than the cadence it expects
    /// its runners to report at: a three-second silence means nothing against
    /// reports fifteen seconds apart. The runners these tests start report
    /// every 0.2 s, so the hub is told the same, and the deployment's shape —
    /// a threshold many reports long — is kept.
    pub async fn start_hub_with(&self, which: &str, options: &[&str]) -> Process {
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
            // Unless the test is itself about the cadence: the binary
            // refuses the same option twice, and rightly.
            .args(cadence_unless_given(options))
            .args(options)
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

    pub fn start_runner(&self) -> Process {
        self.start_runner_called("the-real-runner", "runner")
    }

    /// The data root of one named runner of this herd, and where its spooled
    /// verdicts land (D34).
    pub fn runner_root(&self, which: &str) -> PathBuf {
        self.data_root.path().join(which)
    }

    /// A runner under a name of its own, with its output in files of its own —
    /// so that a test can have two of them, and read what each said.
    pub fn start_runner_called(&self, name: &str, which: &str) -> Process {
        self.start_runner_rooted(name, which, which)
    }

    /// The same, with the data root named separately from the output files, so
    /// that a runner can be started again on the root a previous one left —
    /// which is how a spooled verdict gets delivered (D34).
    pub fn start_runner_rooted(&self, name: &str, which: &str, root: &str) -> Process {
        self.start_runner_with(name, which, root, &[])
    }

    /// A runner carrying **no token at all**: the machine nobody has approved
    /// yet (E-48).
    ///
    /// The one way into this harness that does not approve first, because the
    /// thing M15 has to prove is what happens to a machine an operator has
    /// not let in.
    pub fn start_runner_unapproved(&self, name: &str, which: &str) -> Process {
        self.spawn_runner(name, which, which, &[], None)
    }

    /// The same, with whatever options the test wants — its own cadence above
    /// all, because the pairing of the two sides' timings is itself something
    /// the hub judges.
    pub fn start_runner_with(
        &self,
        name: &str,
        which: &str,
        root: &str,
        options: &[&str],
    ) -> Process {
        let token = self.approve(name);
        self.spawn_runner(name, which, root, options, Some(token))
    }

    fn spawn_runner(
        &self,
        name: &str,
        which: &str,
        root: &str,
        options: &[&str],
        token: Option<String>,
    ) -> Process {
        let mut command = Command::new(binary("herd-runner"));
        // The token this machine's approval minted, when it has one (E-48).
        // Through the environment rather than the command line, as a
        // deployment passes it: a token on argv is readable out of `/proc` by
        // every user of the host. `None` is the machine nobody approved.
        if let Some(token) = token {
            command.env("HERD_TOKEN", token);
        }
        let child = command
            .arg("--hub")
            .arg(self.url())
            .arg("--name")
            .arg(name)
            // A data root of its own, and the real one: this is the binary
            // the server runs, confinement included (M4). One per runner,
            // because a runner locks its root for the life of the process
            // (D32), and these tests run two at a time.
            .arg("--data-root")
            .arg(self.runner_root(root))
            // The same cadence the hub is told to expect, unless the test is
            // itself about the cadence.
            .args(cadence_unless_given(options))
            .args(options)
            .stdout(Stdio::from(
                File::create(self.logs.path().join(format!("{which}-out"))).expect("a file"),
            ))
            .stderr(Stdio::from(
                File::create(self.logs.path().join(format!("{which}-err"))).expect("a file"),
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
    pub fn said(&self, which: &str) -> String {
        let mut text = String::new();
        for stream in ["out", "err"] {
            text.push_str(
                &std::fs::read_to_string(self.logs.path().join(format!("{which}-{stream}")))
                    .unwrap_or_default(),
            );
        }
        text
    }

    pub async fn herdctl(&self, arguments: &[&str]) -> std::process::Output {
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
    pub async fn submit(&self, script: &str) -> String {
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
    pub async fn wait_for_runner(&self) {
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
    pub fn start_wait(&self, job_id: &str) -> Process {
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

    pub fn job_dir(&self, job_id: &str) -> PathBuf {
        self.state.path().join("jobs").join(job_id)
    }

    /// The identity of the hub's state root, read from its own file as a
    /// runner learns it from the handshake.
    pub fn state_root_id(&self) -> String {
        let path = self.state.path().join("store.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
        let store: serde_json::Value = serde_json::from_str(&text).expect("store.json is json");
        store["state_root_id"]
            .as_str()
            .expect("a state root has an identity")
            .to_owned()
    }

    /// Make a runner's data root believe it belongs to this hub's state root,
    /// as it would after one welcome.
    pub fn pretend_the_root_was_welcomed(&self, which: &str) {
        let root = self.runner_root(which);
        std::fs::create_dir_all(&root).expect("the fixture makes the data root");
        std::fs::write(root.join("hub-state-root"), self.state_root_id())
            .expect("the fixture writes which hub state root it belongs to");
    }

    pub fn record(&self, job_id: &str) -> serde_json::Value {
        let path = self.job_dir(job_id).join("job.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is json: {e}", path.display()))
    }

    /// The transitions on a job's record, in order. A line of `events.jsonl`
    /// that records something else — output that never arrived — is not one.
    pub fn states_of(&self, job_id: &str) -> Vec<String> {
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

    pub fn log(&self, job_id: &str) -> String {
        std::fs::read_to_string(self.job_dir(job_id).join("log")).unwrap_or_default()
    }

    pub async fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting until {what}.\n{}",
                self.everything_said()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Everything every process of this herd has printed, named.
    ///
    /// The diagnostic above used to read `self.said("first")`, which is the
    /// hub of the tests that call their first hub `first` and **empty** for
    /// every other test in the file. A timeout in one of those printed its
    /// sentence and then nothing at all, which is the worst a failure can
    /// do: it costs a rerun to learn what the suite already knew (D101).
    pub fn everything_said(&self) -> String {
        let mut whos: Vec<String> = std::fs::read_dir(self.logs.path())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix("-out").map(str::to_owned)
            })
            .collect();
        whos.sort();
        let mut text = String::new();
        for who in whos {
            text.push_str(&format!("--- {who} said:\n{}", self.said(&who)));
        }
        text
    }
}

impl Herd {
    /// Submit a job that needs one capability, at one of the four levels,
    /// and leave with its id (E-18, E-25).
    ///
    /// The capability is what makes the job *wait*: a runner whose probe says
    /// the capability is held takes no job needing it, which is the only
    /// honest way to make a queue of real jobs stand still until a test says
    /// otherwise (E-26, D38).
    pub async fn submit_needing(&self, label: &str, priority: &str, script: &str) -> String {
        let output = self
            .herdctl(&[
                "submit",
                "--label",
                label,
                "--priority",
                priority,
                "--",
                "/bin/sh",
                "-c",
                script,
            ])
            .await;
        assert!(
            output.status.success(),
            "the submission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Herd {
    /// Submit a job that declares what it needs of a machine (E-29, E-31),
    /// and leave with its id.
    ///
    /// The two figures are what admission weighs: a whole-job memory need and
    /// a tmpfs reservation. Either may be left out, which is a job that
    /// declares nothing of the kind rather than a job that needs zero.
    pub async fn submit_reserving(
        &self,
        memory_mib: Option<u64>,
        tmpfs_mib: Option<u64>,
        script: &str,
    ) -> String {
        let mut arguments: Vec<String> = vec!["submit".to_owned()];
        if let Some(memory_mib) = memory_mib {
            arguments.push("--memory-mib".to_owned());
            arguments.push(memory_mib.to_string());
        }
        if let Some(tmpfs_mib) = tmpfs_mib {
            arguments.push("--tmpfs-mib".to_owned());
            arguments.push(tmpfs_mib.to_string());
        }
        for word in ["--", "/bin/sh", "-c", script] {
            arguments.push(word.to_owned());
        }
        let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let output = self.herdctl(&borrowed).await;
        assert!(
            output.status.success(),
            "the submission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Why a job is not starting, as its own record says it (E-04, E-53).
    pub fn waiting_for(&self, job_id: &str) -> String {
        self.record(job_id)["waiting_for"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }
}

impl Herd {
    /// Submit a job with whatever options the test wants, and leave with its
    /// id. The M9 options — a deadline, a parent, a requeue code — go through
    /// here.
    pub async fn submit_with(&self, options: &[&str], script: &str) -> std::process::Output {
        let mut arguments: Vec<&str> = vec!["submit"];
        arguments.extend_from_slice(options);
        arguments.extend_from_slice(&["--", "/bin/sh", "-c", script]);
        self.herdctl(&arguments).await
    }

    /// The same, asserting it was accepted.
    pub async fn submit_ok(&self, options: &[&str], script: &str) -> String {
        let output = self.submit_with(options, script).await;
        assert!(
            output.status.success(),
            "the submission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Where the owner's switch file of one named runner is (E-33).
    pub fn switch_file(&self, root: &str) -> PathBuf {
        self.runner_root(root).join("switch")
    }

    /// Set a switch on the machine itself, as an owner does (E-33). Returns
    /// how long the command took, which is the owner's half of the ten
    /// seconds.
    pub async fn switch_on_the_machine(&self, root: &str, arguments: &[&str]) -> Duration {
        let began = Instant::now();
        let output = Command::new(binary("herd-runner"))
            .arg("--data-root")
            .arg(self.runner_root(root))
            .arg("switch")
            .args(arguments)
            .output()
            .await
            .expect("herd-runner switch runs");
        let took = began.elapsed();
        assert!(
            output.status.success(),
            "herd-runner switch {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        took
    }

    /// What `herdctl switch` says about the fleet (E-33).
    pub async fn switches(&self) -> String {
        let output = self.herdctl(&["switch"]).await;
        assert!(
            output.status.success(),
            "herdctl switch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Every byte of this hub's state root, as a sorted list of paths and
    /// contents (E-07).
    ///
    /// What a dry run must not change. Paths **and** bytes, because "nothing
    /// was written" has two halves: no file appeared and no file moved.
    pub fn state_bytes(&self) -> Vec<(String, Vec<u8>)> {
        fn walk(at: &Path, under: &Path, into: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(at)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect();
            entries.sort();
            for entry in entries {
                let shown = entry
                    .strip_prefix(under)
                    .unwrap_or(&entry)
                    .display()
                    .to_string();
                if entry.is_dir() {
                    into.push((format!("{shown}/"), Vec::new()));
                    walk(&entry, under, into);
                } else {
                    into.push((shown, std::fs::read(&entry).unwrap_or_default()));
                }
            }
        }
        let mut bytes = Vec::new();
        walk(self.state.path(), self.state.path(), &mut bytes);
        bytes
    }
}
