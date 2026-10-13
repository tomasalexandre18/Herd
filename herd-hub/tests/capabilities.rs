//! E-24 and E-26 against the real binaries: `herd-hub`, `herd-runner` and
//! `herdctl` as processes, with a real lock file an operator could create with
//! `touch` and remove with `rm`.
//!
//! The in-process tests cannot stand in for this one. What E-26 promises is
//! that a capability something *outside Herd* is holding suspends a runner
//! without failing anything, and the mechanism an operator will actually reach
//! for is a path on a disk. So the probe here is a real file, the interval is
//! a real interval — shortened, because a test that waited out the default
//! would be a test CI drops — and the thing asserted is what `cat job.json`
//! says while the lock is held and after it is gone.
//!
//! Every wait is on a condition, never on a sleep, and every process is killed
//! when its test ends however it ends.

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

/// How often the runner under test reads its probes. Short, because the whole
/// point is to watch a lock file being released; the real default is two
/// seconds and nothing in CI should wait that out.
const PROBE_INTERVAL_MS: u64 = 100;

/// A process the test started, killed when the test ends however it ends.
struct Process {
    child: Child,
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
/// and `herdctl` as programs. A stale binary from an earlier build would be
/// the one driven here, and a test of the real binaries that silently drives
/// yesterday's binary is worse than no test at all.
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
        if directory.file_name().is_some_and(|name| name == "release") {
            cargo.arg("--release");
        }
        assert!(
            cargo.status().expect("cargo runs").success(),
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

/// A port that was free a moment ago.
///
/// It cannot be more than that: the hub under test is a *process*, which binds
/// the port itself, so the listener has to be let go before it starts and
/// anything else in the suite may take it in between. [`Herd::start_hub`]
/// therefore retries on another port rather than failing a test about
/// capabilities for a reason that has nothing to do with them.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("a free loopback port")
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

/// A hub, a runner with the capabilities a test gives it, and a requester.
struct Herd {
    state: TempDir,
    logs: TempDir,
    data_root: TempDir,
    /// Where a test puts the lock files its probes look for.
    run: TempDir,
    port: u16,
    hub: Option<Process>,
    runner: Option<Process>,
}

impl Herd {
    fn new() -> Self {
        Self {
            state: tempfile::tempdir().expect("a temporary state directory"),
            logs: tempfile::tempdir().expect("a temporary directory for the logs"),
            data_root: tempfile::tempdir().expect("a temporary data root"),
            run: tempfile::tempdir().expect("a temporary run directory"),
            port: free_port(),
            hub: None,
            runner: None,
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

    /// A path under this herd's run directory, for a lock file.
    fn lock_path(&self, name: &str) -> PathBuf {
        self.run.path().join(name)
    }

    fn hold(&self, name: &str) -> PathBuf {
        let path = self.lock_path(name);
        std::fs::write(&path, b"held outside herd").expect("the lock file is written");
        path
    }

    fn output(&self, which: &str, stream: &str) -> Stdio {
        Stdio::from(
            File::create(self.logs.path().join(format!("{which}-{stream}")))
                .expect("a file for a process's output"),
        )
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

    async fn start_hub(&mut self) {
        // A few attempts, because the port was only free a moment ago: if
        // something else in the suite took it in between, the hub exits
        // saying so and this picks another one. Nothing else is retried —
        // every other way the hub can fail to come up is a failure.
        for attempt in 1..=5 {
            let child = Command::new(binary("herd-hub"))
                .arg("--listen")
                .arg(self.addr().to_string())
                .arg("--state-dir")
                .arg(self.state.path())
                .stdout(self.output("hub", "out"))
                .stderr(self.output("hub", "err"))
                .kill_on_drop(true)
                .spawn()
                .expect("herd-hub starts");
            self.hub = Some(Process { child });
            if self.hub_accepts().await {
                return;
            }
            let said = self.said("hub");
            assert!(
                said.contains("Address already in use"),
                "the hub never listened on {}: {said}",
                self.addr()
            );
            eprintln!(
                "this test's hub found {} taken on attempt {attempt}; trying \
                 another port",
                self.addr()
            );
            self.hub = None;
            self.port = free_port();
        }
        panic!(
            "no free port was still free by the time the hub bound it: {}",
            self.said("hub")
        );
    }

    /// Whether the hub came up, waiting on the port rather than on a sleep.
    ///
    /// `false` means the process ended instead — which is what a port taken
    /// in the meantime looks like from here.
    async fn hub_accepts(&mut self) -> bool {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if TcpStream::connect(self.addr()).is_ok() {
                return true;
            }
            if let Some(process) = &mut self.hub {
                if matches!(process.child.try_wait(), Ok(Some(_))) {
                    return false;
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Start a runner that offers exactly what `capabilities` declares, with
    /// nothing detected from the host unless `detect` says so.
    ///
    /// Detection off by default on purpose: what this host happens to have
    /// under `/dev` must not decide whether an assertion about the algebra
    /// holds. The one test that *is* about detection turns it on.
    async fn start_runner(&mut self, file: Option<&str>, extra: &[&str]) {
        let mut command = Command::new(binary("herd-runner"));
        command
            .arg("--hub")
            .arg(self.url())
            .arg("--name")
            .arg("the-real-runner")
            .arg("--data-root")
            .arg(self.data_root.path())
            .arg("--probe-interval")
            .arg(PROBE_INTERVAL_MS.to_string());
        if let Some(contents) = file {
            let path = self.run.path().join("capabilities.json");
            std::fs::write(&path, contents).expect("the capability file is written");
            command.arg("--capabilities").arg(&path);
        }
        command.args(extra);
        let child = command
            .stdout(self.output("runner", "out"))
            .stderr(self.output("runner", "err"))
            .kill_on_drop(true)
            .spawn()
            .expect("herd-runner starts");
        self.runner = Some(Process { child });
        // The runner is in when a job that needs nothing runs. Nothing about
        // a capability is asserted by this: it is the handshake landing.
        let deadline = Instant::now() + PATIENCE;
        loop {
            let ran = self
                .herdctl(&["run", "--", "/bin/sh", "-c", "printf ready"])
                .await;
            if ran.status.success() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the runner never joined: {}",
                String::from_utf8_lossy(&ran.stderr)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
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

    /// Submit a job needing `labels` and leave with its id, as a hook would.
    async fn submit(&self, labels: &str, script: &str) -> String {
        let output = self
            .herdctl(&["submit", "--label", labels, "--", "/bin/sh", "-c", script])
            .await;
        assert!(
            output.status.success(),
            "the submission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn job_dir(&self, job_id: &str) -> PathBuf {
        self.state.path().join("jobs").join(job_id)
    }

    /// A job's record, read as the first consumer reads it: plain JSON, no
    /// tool but a file read (E-15, E-53).
    fn record(&self, job_id: &str) -> serde_json::Value {
        let path = self.job_dir(job_id).join("job.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is json: {e}", path.display()))
    }

    fn state_of(&self, job_id: &str) -> String {
        self.record(job_id)["state"]["type"]
            .as_str()
            .expect("a state has a type")
            .to_owned()
    }

    fn waiting_for(&self, job_id: &str) -> Option<String> {
        self.record(job_id)["waiting_for"]
            .as_str()
            .map(str::to_owned)
    }

    /// Every line of `events.jsonl` that records a wait, with its reason.
    fn waits_of(&self, job_id: &str) -> Vec<String> {
        let path = self.job_dir(job_id).join("events.jsonl");
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let event: serde_json::Value =
                    serde_json::from_str(line).unwrap_or_else(|e| panic!("{line} is json: {e}"));
                if event["event"] == "waiting" {
                    Some(event["reason"].as_str().unwrap_or_default().to_owned())
                } else {
                    None
                }
            })
            .collect()
    }

    async fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting until {what}.\nhub said: {}\nrunner said: {}",
                self.said("hub"),
                self.said("runner")
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// A capability file declaring one capability with one probe.
fn one_probed_capability(name: &str, probe: &str) -> String {
    format!(
        r#"{{"capabilities":[{{"name":"{name}",
            "kind":{{"type":"counted","slots":1}},
            "busy_probe":{probe}}}]}}"#
    )
}

/// The heart of E-26, driven the way an operator would drive it: a lock file
/// is held, a job that needs the capability waits with the reason on its
/// record, the lock is released, and the job runs and returns its own verdict.
///
/// Nothing is refused and nothing is failed at any point (D38).
#[tokio::test]
async fn a_held_lock_file_keeps_a_job_queued_until_it_is_released() {
    let mut herd = Herd::new();
    let lock = herd.hold("qemu.lock");
    let file = one_probed_capability(
        "qemu",
        &format!(
            r#"{{"type":"lock_file","path":"{}"}}"#,
            lock.to_string_lossy()
        ),
    );
    herd.start_hub().await;
    herd.start_runner(Some(&file), &["--no-detect"]).await;

    // The runner says what it is suspending, and so does the hub.
    herd.wait_until("the runner has read the held lock file", || {
        herd.said("runner").contains("qemu is suspended")
    })
    .await;
    assert!(
        herd.said("hub").contains("qemu is suspended on the runner"),
        "the hub must record what it was told: {}",
        herd.said("hub")
    );

    let job = herd
        .submit("qemu", "printf 'it ran after all\\n'; exit 7")
        .await;
    herd.wait_until("the job's record says why it is waiting", || {
        herd.waiting_for(&job).is_some()
    })
    .await;
    let why = herd.waiting_for(&job).expect("a reason");
    assert!(why.contains("qemu"), "{why}");
    assert!(
        why.contains(&lock.to_string_lossy().into_owned()),
        "the reason must name the lock file: {why}"
    );
    assert!(
        why.contains("not refused"),
        "a wait must not read as a refusal (D38): {why}"
    );
    assert_eq!(
        herd.state_of(&job),
        "queued",
        "a job waiting on a probe is queued, not failed and not refused"
    );
    assert_eq!(
        herd.waits_of(&job).len(),
        1,
        "the reason belongs on the record once, not once per pass: {:?}",
        herd.waits_of(&job)
    );

    // It stays queued across many probe intervals: nothing times out into a
    // failure, and nothing starts it behind the lock.
    tokio::time::sleep(Duration::from_millis(PROBE_INTERVAL_MS * 8)).await;
    assert_eq!(herd.state_of(&job), "queued");
    assert_eq!(
        herd.waits_of(&job).len(),
        1,
        "an unchanged reason must not be written again: {:?}",
        herd.waits_of(&job)
    );

    // ---- The operator releases the lock. Nothing is resubmitted.
    std::fs::remove_file(&lock).expect("the lock file is removed");

    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(7),
        "the job must run and return its own verdict: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert!(
        String::from_utf8_lossy(&waited.stdout).contains("it ran after all"),
        "{:?}",
        String::from_utf8_lossy(&waited.stdout)
    );
    let record = herd.record(&job);
    assert_eq!(record["state"]["type"], "finished");
    assert!(
        record["waiting_for"].is_null(),
        "a job that is no longer queued waits for nothing: {record}"
    );
    assert_eq!(
        record["labels"],
        serde_json::json!(["qemu"]),
        "what the job asked for is part of its record: {record}"
    );
    // The history keeps why it waited, which the record no longer carries.
    assert_eq!(herd.waits_of(&job).len(), 1);
}

/// The same promise through the other kind of probe: a command whose exit
/// code means busy. The codes are configuration — 0 busy, 1 free by default —
/// and `test -e` is exactly that shape.
#[tokio::test]
async fn a_busy_command_probe_keeps_a_job_queued_until_it_says_free() {
    let mut herd = Herd::new();
    let lock = herd.hold("leased");
    let file = one_probed_capability(
        "metal-pc",
        &format!(
            r#"{{"type":"command","command":["/bin/sh","-c","test -e {}"]}}"#,
            lock.to_string_lossy()
        ),
    );
    herd.start_hub().await;
    herd.start_runner(Some(&file), &["--no-detect"]).await;
    herd.wait_until("the runner has read the busy command probe", || {
        herd.said("runner").contains("metal-pc is suspended")
    })
    .await;
    assert!(
        herd.said("runner").contains("exited 0, which means busy"),
        "the reason must name the code it read: {}",
        herd.said("runner")
    );

    let job = herd
        .submit("metal-pc", "printf 'the lease is free\\n'")
        .await;
    herd.wait_until("the job's record says why it is waiting", || {
        herd.waiting_for(&job).is_some()
    })
    .await;
    assert_eq!(herd.state_of(&job), "queued");
    let why = herd.waiting_for(&job).expect("a reason");
    assert!(why.contains("metal-pc"), "{why}");

    std::fs::remove_file(&lock).expect("the lease is given back");
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert!(String::from_utf8_lossy(&waited.stdout).contains("the lease is free"));
}

/// A probe that cannot be read suspends its capability and nothing else.
///
/// Three ways of being broken, in one runner: a command that is not there, a
/// command that exits with a code the configuration gives no meaning to, and a
/// command that hangs. Each suspends its own capability, by name; the runner
/// goes on serving; a job that needs nothing runs to completion while all
/// three are broken; and the hub is never held up by any of them.
#[tokio::test]
async fn a_broken_probe_suspends_its_own_capability_and_hangs_nothing() {
    let mut herd = Herd::new();
    let file = r#"{"capabilities":[
        {"name":"missing-probe","kind":{"type":"shared"},
         "busy_probe":{"type":"command","command":["/nonexistent/herd-probe"]}},
        {"name":"confused-probe","kind":{"type":"shared"},
         "busy_probe":{"type":"command","command":["/bin/sh","-c","exit 7"]}},
        {"name":"hanging-probe","kind":{"type":"shared"},
         "busy_probe":{"type":"command","command":["/bin/sh","-c","sleep 60"]}},
        {"name":"fine","kind":{"type":"shared"}}
    ]}"#;
    herd.start_hub().await;
    herd.start_runner(Some(file), &["--no-detect", "--probe-timeout", "200"])
        .await;

    for (label, words) in [
        ("missing-probe", "cannot be started"),
        ("confused-probe", "exited 7"),
        ("hanging-probe", "did not answer within 200 ms"),
    ] {
        herd.wait_until(&format!("{label} is suspended"), || {
            herd.said("runner")
                .contains(&format!("{label} is suspended"))
        })
        .await;
        assert!(
            herd.said("runner").contains(words),
            "{label} must be suspended by name, not by a guess: {}",
            herd.said("runner")
        );
        let job = herd
            .submit(label, "printf 'this should not have run\\n'")
            .await;
        herd.wait_until(&format!("{job} says why it is waiting"), || {
            herd.waiting_for(&job).is_some()
        })
        .await;
        assert_eq!(
            herd.state_of(&job),
            "queued",
            "a job needing {label} must wait, not fail and not run"
        );
        let why = herd.waiting_for(&job).expect("a reason");
        assert!(why.contains(label), "{why}");
    }

    // The runner is still serving, and so is the hub: a capability whose
    // probe nobody can read suspends that capability and nothing else. This
    // is the assertion that forbids a hanging probe from taking the hub with
    // it — it runs while `sleep 60` is being started and killed every
    // interval.
    let ran = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'unaffected\\n'"])
        .await;
    assert_eq!(
        ran.status.code(),
        Some(0),
        "a job needing nothing must run while every probe is broken: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(String::from_utf8_lossy(&ran.stdout).contains("unaffected"));
    // And a capability with no probe is not suspended by its neighbours.
    let ran = herd
        .herdctl(&[
            "run",
            "--label",
            "fine",
            "--",
            "/bin/sh",
            "-c",
            "printf 'fine\\n'",
        ])
        .await;
    assert_eq!(
        ran.status.code(),
        Some(0),
        "an unprobed capability is free: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

/// A label this runner does not offer leaves the job queued with the reason,
/// and the job runs the moment a runner that does offer it arrives.
///
/// Failing such a job fast when *no* runner could ever offer the label is
/// E-36, in M7. What M5 owes is that it is never refused and never failed for
/// it (D38).
#[tokio::test]
async fn a_label_no_runner_offers_leaves_the_job_queued() {
    let mut herd = Herd::new();
    herd.start_hub().await;
    herd.start_runner(None, &["--no-detect"]).await;
    let job = herd.submit("vfio", "printf 'with vfio\\n'").await;
    herd.wait_until("the job's record says why it is waiting", || {
        herd.waiting_for(&job).is_some()
    })
    .await;
    assert_eq!(herd.state_of(&job), "queued");
    let why = herd.waiting_for(&job).expect("a reason");
    assert!(why.contains("does not offer vfio"), "{why}");
    assert!(why.contains("the-real-runner"), "{why}");

    // A runner that does offer it joins, and the job that was waiting runs
    // with nobody resubmitting it.
    drop(herd.runner.take());
    herd.start_runner(None, &["--no-detect", "--declare-capability", "vfio"])
        .await;
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert!(String::from_utf8_lossy(&waited.stdout).contains("with vfio"));
}

/// E-24 through the real binaries: a capability the host has and its owner
/// denies is reported as `kvm: present, denied`, and no job gets it.
///
/// This is the one assertion here that depends on the host, because `present`
/// means a `/dev/kvm` this runner can open. Where there is none it is skipped
/// with that as its reason (D27) rather than weakened: the exact line is also
/// asserted unconditionally in `herd-runner`'s unit tests, which do not need a
/// device to build an offer.
#[tokio::test]
async fn a_capability_the_owner_denies_is_reported_present_and_denied() {
    let mut herd = Herd::new();
    herd.start_hub().await;
    herd.start_runner(None, &["--deny-capability", "kvm"]).await;

    let has_kvm = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok();
    if has_kvm {
        assert!(
            herd.said("runner").contains("kvm: present, denied"),
            "E-24's line must read exactly `kvm: present, denied`: {}",
            herd.said("runner")
        );
        assert!(
            herd.said("hub")
                .contains("the-real-runner offers kvm: present, denied"),
            "the hub must record what it was offered: {}",
            herd.said("hub")
        );
    } else {
        herd_runner::testing::skipped(
            "a_capability_the_owner_denies_is_reported_present_and_denied \
             (the `present` half)",
            "this host has no /dev/kvm this runner can open, so there is \
             nothing for the owner's denial to override; the denial itself is \
             still asserted below, and the exact `kvm: present, denied` line \
             is asserted in herd-runner's unit tests",
        );
        assert!(
            herd.said("runner").contains("kvm: denied"),
            "the denial must be reported whether the host has the device or \
             not: {}",
            herd.said("runner")
        );
    }

    // And denial is not decoration: no job gets the capability.
    let job = herd
        .submit("kvm", "printf 'this should not have run\\n'")
        .await;
    herd.wait_until("the job's record says why it is waiting", || {
        herd.waiting_for(&job).is_some()
    })
    .await;
    assert_eq!(herd.state_of(&job), "queued");
    let why = herd.waiting_for(&job).expect("a reason");
    assert!(
        why.contains("denied"),
        "a denied capability must read as denied, not as absent: {why}"
    );
}

/// A capability file that does not read, or declares something that is not a
/// capability, stops the runner at startup naming it — the rule E-45 set for
/// a data root, applied to the other thing a runner is configured with.
///
/// A runner that started anyway would offer the wrong thing, and the first
/// anyone would hear of it is a job on a machine that cannot run it.
#[tokio::test]
async fn a_capability_file_that_does_not_read_stops_the_runner_naming_it() {
    let herd = Herd::new();
    for (name, contents, expected) in [
        ("missing.json", None, "cannot be read"),
        (
            "not-json.json",
            Some("this is not json at all"),
            "is not a capability file",
        ),
        (
            "bad-name.json",
            Some(r#"{"capabilities":[{"name":"KVM","kind":{"type":"shared"}}]}"#),
            "KVM",
        ),
        (
            "no-slots.json",
            Some(r#"{"capabilities":[{"name":"build","kind":{"type":"counted","slots":0}}]}"#),
            "counted(0)",
        ),
        (
            "unknown-key.json",
            Some(r#"{"capabilities":[],"capabilites":[]}"#),
            "is not a capability file",
        ),
        // The one that matters most: `probe` where `busy_probe` was meant.
        // Read permissively, this runner would offer `metal-pc` unprobed and
        // hand it to a job while something outside Herd held it (E-26). The
        // refusal names the key and the capability.
        (
            "misspelled-probe.json",
            Some(
                r#"{"capabilities":[{"name":"metal-pc",
                   "kind":{"type":"counted","slots":1},
                   "probe":{"type":"lock_file","path":"/run/metal-pc"}}]}"#,
            ),
            "probe",
        ),
        (
            "misspelled-probe-names-it.json",
            Some(
                r#"{"capabilities":[{"name":"metal-pc",
                   "kind":{"type":"counted","slots":1},
                   "probe":{"type":"lock_file","path":"/run/metal-pc"}}]}"#,
            ),
            "metal-pc",
        ),
        // And a name declared twice, whose second declaration would otherwise
        // drop the exclusion the first one wrote down.
        (
            "declared-twice.json",
            Some(
                r#"{"capabilities":[
                   {"name":"build","kind":{"type":"exclusive"},"excludes":["kvm"]},
                   {"name":"kvm","kind":{"type":"shared"}},
                   {"name":"build","kind":{"type":"counted","slots":9}}]}"#,
            ),
            "build is declared twice",
        ),
    ] {
        let path = herd.run.path().join(name);
        if let Some(contents) = contents {
            std::fs::write(&path, contents).expect("the fixture writes");
        }
        let output = Command::new(binary("herd-runner"))
            .arg("--hub")
            // Never dialled: the capabilities are resolved before the dial.
            .arg("ws://127.0.0.1:1")
            .arg("--data-root")
            .arg(herd.data_root.path())
            .arg("--capabilities")
            .arg(&path)
            .output()
            .await
            .expect("herd-runner runs");
        assert!(
            !output.status.success(),
            "{name} must stop the runner, not be ignored"
        );
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(
            said.contains(expected),
            "the refusal for {name} must name {expected}: {said}"
        );
    }
}

/// A requester that *follows* its job hears why it is waiting on the open
/// connection, and then gets the job's own verdict when the probe clears.
///
/// This is D38 from the caller's side: `herdctl run` neither exits with a
/// refusal nor sits silent. `Refused` is terminal and would have ended the
/// connection; a note does not.
#[tokio::test]
async fn a_following_requester_is_told_why_its_job_waits_and_then_gets_it() {
    let mut herd = Herd::new();
    let lock = herd.hold("qemu.lock");
    let file = one_probed_capability(
        "qemu",
        &format!(
            r#"{{"type":"lock_file","path":"{}"}}"#,
            lock.to_string_lossy()
        ),
    );
    herd.start_hub().await;
    herd.start_runner(Some(&file), &["--no-detect"]).await;
    herd.wait_until("the runner has read the held lock file", || {
        herd.said("runner").contains("qemu is suspended")
    })
    .await;

    // `herdctl run` is meant *not* to come back while the lock is held, so it
    // is started and watched rather than run to completion.
    let mut following = Command::new(binary("herdctl"))
        .arg("--hub")
        .arg(herd.url())
        .args([
            "run",
            "--label",
            "qemu",
            "--",
            "/bin/sh",
            "-c",
            "printf 'at last\\n'; exit 3",
        ])
        .stdout(herd.output("run", "out"))
        .stderr(herd.output("run", "err"))
        .kill_on_drop(true)
        .spawn()
        .expect("herdctl starts");

    herd.wait_until("the follower is told why its job waits", || {
        herd.said("run").contains("queued and has not started")
    })
    .await;
    assert!(
        herd.said("run").contains("qemu is suspended"),
        "the note must carry the reason: {}",
        herd.said("run")
    );
    assert!(
        matches!(following.try_wait(), Ok(None)),
        "a waiting job must not end the connection: {}",
        herd.said("run")
    );

    std::fs::remove_file(&lock).expect("the lock file is removed");
    let ended = tokio::time::timeout(PATIENCE, following.wait())
        .await
        .expect("herdctl comes back once the probe clears")
        .expect("herdctl runs");
    assert_eq!(
        ended.code(),
        Some(3),
        "the follower must get the job's own verdict: {}",
        herd.said("run")
    );
    assert!(
        herd.said("run").contains("at last"),
        "and its output: {}",
        herd.said("run")
    );
}
