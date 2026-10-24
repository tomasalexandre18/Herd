//! **Experiment 6.3 (b): how many jobs one runner takes in hand when the
//! thing handing them out is not the hub.**
//!
//! Invariant 2 says a runner never destabilizes its host. The audit's
//! hypothesis is that this is today a property of the **hub's arithmetic**
//! rather than of the runner: `serve_hub` spawns a task for every `AssignJob`
//! with no condition at all (`herd-runner/src/lib.rs:2745`) and `take_job`
//! refuses only for `draining` and for the owner's switch
//! (`herd-runner/src/lib.rs:1151-1165`) — never on a count. A runner facing a
//! hub that is wrong, compromised or merely buggy would therefore accept
//! whatever it is sent.
//!
//! This **measures** that. It is a bench, not an expectation: every figure is
//! printed and nothing about the count is asserted, because asserting a
//! number would pin behaviour that is a product decision nobody has taken.
//! The test names begin `measure_` for that reason.
//!
//! ## What is kept bounded, and how
//!
//! The jobs are `sleep`: what is being measured is the *number* admitted, not
//! work. Every one carries the runner's own memory cap (M4's defaults on a
//! throwaway data root) and a `timeout_s` short enough that nothing here can
//! outlive the test even if it fails. Everything binds loopback on port 0 and
//! writes under a temporary data root. The host's one-minute load is read
//! before the flood and while it runs, and the flood is **stopped and
//! reported** rather than finished if it crosses [`LOAD_CEILING`] — the host
//! is shared with a production system.
//!
//! ## Why this file is in `herd-hub` and not beside the runner
//!
//! It binds a listening socket, and `no_module_of_the_runner_can_bind_a_socket`
//! (`herd-runner/tests/confinement.rs:1638-1668`) forbids the string
//! `TcpListener` anywhere under the `herd-runner` crate — its tests included,
//! with one exemption for the test that states the rule. That rule is right,
//! and invariant 3 is the reason: a runner dials out and binds nothing. A
//! bench that stands in for a **hub** therefore belongs beside the hub's own
//! tests, which already bind loopback ports
//! (`herd-hub/tests/unknown_frames.rs:54`). The runner under test is the real
//! library either way.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, Switch, ToHub, PROTOCOL_VERSION};
use herd_runner::testing::TempDir;
use herd_runner::{Confined, Confinement, Session};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The one-minute load average above which this bench stops and reports what
/// it has. The host is shared with a production system.
const LOAD_CEILING: f64 = 8.0;

/// How long a job is told to sleep, and the cap the assignment carries.
///
/// Long enough that every assignment of a flood is still in hand when the
/// measurement is taken, short enough that a failure here cannot leave
/// anything running for more than half a minute.
const SLEEP_SECONDS: u64 = 20;
const TIMEOUT_SECONDS: u64 = 25;

/// How long the probe may take before the runner counts as unresponsive.
const PROBE_BUDGET: Duration = Duration::from_millis(2_000);

/// How long any wait here may take before it is a failure.
const PATIENCE: Duration = Duration::from_secs(60);

type Socket = WebSocketStream<TcpStream>;

/// The host's one-minute load average, or `None` when `/proc/loadavg` cannot
/// be read — never zero, because zero is a measurement.
fn load_one_minute() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/loadavg").ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// How many `herd-job-*` cgroups exist under this process's own cgroup, and
/// whether that subtree is one this process could have made them in.
///
/// Both halves, because the count alone lies on a host with no delegation:
/// zero cgroups then means "it could make none", not "it made none", and a
/// reader of the figures must be able to tell the two apart.
fn herd_job_cgroups() -> (Option<usize>, bool) {
    let Some(mine) = std::fs::read_to_string("/proc/self/cgroup").ok() else {
        return (None, false);
    };
    let Some(relative) = mine.rsplit(':').next().map(str::trim).map(str::to_owned) else {
        return (None, false);
    };
    let root = std::path::Path::new("/sys/fs/cgroup").join(relative.trim_start_matches('/'));
    // "Could a job cgroup have been made here?" asked the only way that does
    // not lie: by making one and removing it again.
    let probe = root.join("herd-job-delegation-probe");
    let delegated = std::fs::create_dir(&probe).is_ok();
    if delegated {
        let _ = std::fs::remove_dir(&probe);
    }
    let Ok(entries) = std::fs::read_dir(&root) else {
        return (None, delegated);
    };
    let count = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("herd-job-"))
        .count();
    (Some(count), delegated)
}

/// How many job workspaces this runner has open under its data root.
///
/// The same tag names a job's cgroup, its workspace and its marker
/// (`herd-runner/src/reclaim.rs:84`), so on a host with no delegated cgroup
/// this is the observable that says how many jobs are really in hand.
fn job_workspaces(data_root: &std::path::Path) -> usize {
    let jobs = data_root.join("jobs");
    std::fs::read_dir(jobs)
        .map(|entries| entries.filter_map(|entry| entry.ok()).count())
        .unwrap_or(0)
}

/// A hub that is not one: it speaks the protocol and hands out whatever this
/// file tells it to.
struct FakeHub {
    url: String,
    listening: TcpListener,
}

impl FakeHub {
    async fn bind() -> Self {
        let listening = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let url = format!(
            "ws://{}",
            listening.local_addr().expect("the bound address")
        );
        Self { url, listening }
    }

    /// Accept the runner's dial, answer its hello, and hand back the socket.
    async fn welcome_a_runner(&self) -> Socket {
        let (stream, _peer) = self.listening.accept().await.expect("the runner dials");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("the runner speaks websocket");
        let hello = loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    break from_frame::<ToHub>(text.as_str())
                        .expect("the runner speaks the protocol")
                }
                Some(Ok(_)) => continue,
                other => panic!("the runner said {other:?} instead of hello"),
            }
        };
        assert!(
            matches!(
                hello,
                ToHub::Hello {
                    role: Role::Runner,
                    ..
                }
            ),
            "the peer that dialled is not a runner: {hello:?}"
        );
        let welcome = FromHub::Welcome {
            protocol_version: PROTOCOL_VERSION,
            hub_version: herd_runner::runner_version().to_owned(),
            state_root: "fake-hub-state-root".to_owned(),
            version_note: None,
        };
        socket
            .send(Message::text(to_frame(&welcome).expect("encodes")))
            .await
            .expect("the welcome goes out");
        socket
    }
}

/// The id of the nth job this bench hands out, in the hub's own shape.
fn a_job_id(n: usize) -> JobId {
    JobId::new(format!("j{n:010}"))
}

/// Hand out `how_many` assignments as fast as the socket takes them.
async fn assign(socket: &mut Socket, how_many: usize) {
    for n in 1..=how_many {
        let frame = FromHub::AssignJob {
            job_id: a_job_id(n),
            attempt: 1,
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                format!("sleep {SLEEP_SECONDS}"),
            ],
            declaration: None,
            timeout_s: Some(TIMEOUT_SECONDS),
        };
        socket
            .send(Message::text(to_frame(&frame).expect("encodes")))
            .await
            .expect("the assignment goes out");
    }
}

/// What the runner says about itself: the jobs on its latest health report,
/// and how long the control link took to answer a probe.
struct Reading {
    jobs_on_health: Option<usize>,
    probe: Option<Duration>,
}

/// Probe the control link, draining health reports on the way.
///
/// `SetSwitch { On }` is the probe because it is the one frame a runner
/// **acknowledges** (`herd-runner/src/lib.rs:2810-2816`), and `On` is the
/// position it is already in, so the probe changes nothing. A runner whose
/// read loop is starved cannot answer it.
async fn probe(socket: &mut Socket) -> Reading {
    let mut jobs_on_health = None;
    let began = Instant::now();
    let ask = FromHub::SetSwitch {
        switch: Switch::On,
        why: "a probe, asking for the position this runner is already in".to_owned(),
    };
    socket
        .send(Message::text(to_frame(&ask).expect("encodes")))
        .await
        .expect("the probe goes out");
    loop {
        let left = match PROBE_BUDGET.checked_sub(began.elapsed()) {
            Some(left) => left,
            None => {
                return Reading {
                    jobs_on_health,
                    probe: None,
                }
            }
        };
        match tokio::time::timeout(left, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                match from_frame::<ToHub>(text.as_str()).expect("the runner speaks the protocol") {
                    ToHub::Health { jobs, .. } => jobs_on_health = Some(jobs.len()),
                    ToHub::SwitchApplied { .. } => {
                        return Reading {
                            jobs_on_health,
                            probe: Some(began.elapsed()),
                        }
                    }
                    _ => continue,
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(_) => {
                return Reading {
                    jobs_on_health,
                    probe: None,
                }
            }
            Err(_) => {
                return Reading {
                    jobs_on_health,
                    probe: None,
                }
            }
        }
    }
}

/// Cancel everything this bench handed out, and wait for the workspaces to
/// go. Nothing is killed by name: the runner ends its own jobs.
async fn cancel_everything(socket: &mut Socket, how_many: usize, data_root: &std::path::Path) {
    for n in 1..=how_many {
        let frame = FromHub::CancelJob {
            job_id: a_job_id(n),
            attempt: 1,
            reason: "the bench is over".to_owned(),
        };
        if socket
            .send(Message::text(to_frame(&frame).expect("encodes")))
            .await
            .is_err()
        {
            break;
        }
    }
    let deadline = Instant::now() + PATIENCE;
    while job_workspaces(data_root) > 0 && Instant::now() < deadline {
        // The socket has to go on being read or the runner's writes block.
        let _ = tokio::time::timeout(Duration::from_millis(50), socket.next()).await;
    }
}

/// One flood, measured.
async fn flood(label: &str, how_many: usize) {
    let before = load_one_minute();
    if before.is_some_and(|load| load > LOAD_CEILING) {
        // A plain line and not a `HERD-SKIPPED`: this file asserts nothing
        // about the count, so there is no assertion to skip (D27), and a skip
        // here would move the suite's skip reference for no reason.
        println!(
            "--- experiment 6.3 (b): {label} --- NOT RUN: the host's one-minute load is \
             {before:?}, over this bench's ceiling of {LOAD_CEILING}; the host is shared with a \
             production system"
        );
        return;
    }

    let hub = FakeHub::bind().await;
    let data_root = TempDir::new(label);
    let mut confinement = Confinement::with_data_root(data_root.path());
    // A trivial job needs no room, and a small ceiling is the honest shape of
    // "every job runs under an explicit memory cap" (invariant 2).
    confinement.memory_max = 256 << 20;
    let confined = Arc::new(Confined::prepare(confinement.clone()).expect("a throwaway data root"));
    let config = herd_runner::Config {
        hub_url: hub.url.clone(),
        name: Some(label.to_owned()),
        token: None,
        // Often enough that a reading is never more than a moment stale.
        health_every: Duration::from_millis(500),
        hub_silence_limit: Duration::from_secs(120),
        confinement,
        switch_file: None,
        ..herd_runner::Config::default()
    };
    let running = tokio::spawn(herd_runner::serve_once(
        config,
        Session::new(),
        Arc::clone(&confined),
    ));

    let mut socket = hub.welcome_a_runner().await;
    let handing_out = Instant::now();
    assign(&mut socket, how_many).await;
    let handed_out = handing_out.elapsed();

    // Let the runner get them all started and report on itself at least
    // once, watching the host's load the whole time. Long enough to cover
    // several health reports at the cadence set above, and cut short the
    // instant the load crosses the ceiling.
    let settle = Instant::now() + Duration::from_secs(5);
    let mut bit = None;
    let mut health = None;
    let mut peak_workspaces = 0;
    while Instant::now() < settle {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(200), socket.next()).await
        {
            if let Ok(ToHub::Health { jobs, .. }) = from_frame::<ToHub>(text.as_str()) {
                health = Some(jobs.len());
            }
        }
        peak_workspaces = peak_workspaces.max(job_workspaces(data_root.path()));
        if let Some(load) = load_one_minute() {
            if load > LOAD_CEILING {
                bit = Some(load);
                break;
            }
        }
    }

    let workspaces = job_workspaces(data_root.path());
    let (cgroups, delegated) = herd_job_cgroups();
    let during = load_one_minute();
    let reading = probe(&mut socket).await;

    println!("--- experiment 6.3 (b): {label} ---");
    println!("assignments sent: {how_many} in {handed_out:?}");
    println!("job workspaces open on the runner: {workspaces} (peak {peak_workspaces})");
    match (cgroups, delegated) {
        (Some(count), true) => println!("herd-job cgroups: {count}"),
        (count, false) => println!(
            "herd-job cgroups: {count:?}, and NOT A MEASUREMENT — this host delegates no cgroup \
             v2 subtree to this session, so a runner here can make none and the workspace count \
             above is what stands in for it"
        ),
        (None, true) => println!("herd-job cgroups: the delegated subtree would not be read"),
    }
    println!(
        "jobs on the runner's own health report: {:?}",
        health.or(reading.jobs_on_health)
    );
    match reading.probe {
        Some(took) => {
            println!("the runner answered a control probe in {took:?} (budget {PROBE_BUDGET:?})")
        }
        None => println!("the runner did NOT answer a control probe inside {PROBE_BUDGET:?}"),
    }
    println!("host one-minute load: before {before:?}, during {during:?}");
    if let Some(load) = bit {
        println!(
            "THE LOAD GUARD BIT: one-minute load reached {load}, over {LOAD_CEILING}; the flood \
             was cut short and these figures are what it had"
        );
    }

    cancel_everything(&mut socket, how_many, data_root.path()).await;
    drop(socket);
    running.abort();
    let _ = running.await;
    println!(
        "workspaces left after cancelling: {}",
        job_workspaces(data_root.path())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn measure_fifty_assignments_from_a_hub_that_is_not_one() {
    flood("flood-fifty", 50).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn measure_two_hundred_and_fifty_assignments_from_a_hub_that_is_not_one() {
    flood("flood-two-fifty", 250).await;
}
