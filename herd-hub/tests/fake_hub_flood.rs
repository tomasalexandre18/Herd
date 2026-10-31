//! **How many jobs one runner takes in hand when the thing handing them out
//! is not the hub.**
//!
//! Invariant 2 says a runner never destabilizes its host. When this file was
//! written as experiment 6.3 (b) it could only *measure*, because the
//! behaviour it found was nobody's decision: `serve_hub` spawned a task for
//! every `AssignJob` with no condition at all and `take_job` refused for
//! `draining` and for the owner's switch and **never on a count**, so 50
//! assignments produced 50 jobs and 250 produced 250 — at both figures, with
//! no refusal at either (§2 of
//! `docs/remediation/04b-integrity-and-containment.md`). That bound was a
//! property of the **hub's arithmetic** and not of the runner.
//!
//! It is now the runner's. A runner holds at most as many jobs at once as its
//! own declared lendable memory can hold at its own declared per-job ceiling
//! (`herd_runner::jobs_at_once`), and hands every assignment past that back
//! as a `ToHub::AttemptAbandoned` — the frame the protocol already has for an
//! attempt given up without a verdict, so that nothing new had to appear on
//! any enum and neither version number moved. So this file asserts, and these
//! are the assertions: **past its ceiling the runner refuses, its health
//! report says so, and the host never sees 250 workspaces.**
//!
//! ## What is kept bounded, and how
//!
//! The jobs are `sleep`: what is being measured is the *number* admitted, not
//! work. Every one carries the runner's own memory cap and a `timeout_s`
//! short enough that nothing here can outlive the test even if it fails.
//! Everything binds loopback on port 0 and writes under a temporary data
//! root. The host's one-minute load is read before the flood and while it
//! runs, and if it crosses [`LOAD_CEILING`] the flood is **skipped,
//! D27-fashion** — the host is shared with a production system. That is true
//! of both readings: a host already busy is skipped before anything starts,
//! and a host that becomes busy mid-flood is cut short, printed, and skipped
//! as well. **A cut-short flood is never asserted on.** It has read fewer
//! abandonments than the test expects, and asserting the full figure on a
//! truncated reading would make a busy host look like a broken runner.
//!
//! **The host this runs on is declared, not read.** The ceiling is derived
//! from what the runner says about its machine, so a bench that let it read
//! the real one would assert a different number on every host. It announces
//! [`LENT_MIB`] less a [`FLOOR_MIB`] floor and caps every job at
//! [`PER_JOB_BYTES`], which is [`EXPECTED_CEILING`] jobs and nothing else,
//! anywhere.
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
use herd_proto::{
    from_frame, to_frame, FromHub, HostSize, JobId, Role, Switch, ToHub, PROTOCOL_VERSION,
};
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

/// The machine this bench's runner says it is, and the cap it says it puts on
/// every job. Declared rather than read, so that the ceiling derived from
/// them is the same number on every host this suite runs on.
///
/// Four gibibytes lent, two kept back, 256 MiB a job: eight jobs and no more.
/// A small ceiling on purpose — the assertion is that there *is* one, and
/// every job past it costs a workspace nobody wanted on a shared host.
const LENT_MIB: u64 = 4 * 1024;
const FLOOR_MIB: u64 = 2 * 1024;
const PER_JOB_BYTES: u64 = 256 << 20;
const EXPECTED_CEILING: usize = 8;

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
        let ToHub::Hello {
            role: Role::Runner,
            jobs_at_once,
            ..
        } = &hello
        else {
            panic!("the peer that dialled is not a runner: {hello:?}");
        };
        // **The runner announced its ceiling, and this hub is about to ignore
        // it on purpose.** Both halves matter.
        //
        // That it announced it, and announced *this* figure: the hello
        // carries the same number `serve_hub` holds every assignment against,
        // because `serve_once` derives it once and uses the one value for
        // both. If those two ever came apart, a hub honouring the
        // announcement would place to a ceiling the runner does not have.
        assert_eq!(
            *jobs_at_once,
            Some(EXPECTED_CEILING as u32),
            "the runner must announce the ceiling it enforces, not a second arithmetic"
        );
        // And that ignoring it changes nothing about what follows. This is
        // the **rollback case** in miniature — a hub older than D181, or a hub
        // that is simply not one — and the whole point of C5's first half is
        // that the runner is not weakened by it. Everything below goes on
        // measuring 8 held and the rest handed back, and it must: a fleet
        // whose hub ignores the field pays in attempts exactly what it paid
        // before the field existed, which is the acknowledged cost and not a
        // regression. What a hub that *honours* it costs instead is measured
        // against the real hub, in `announced_job_ceiling.rs`.
        let welcome = FromHub::Welcome {
            protocol_version: PROTOCOL_VERSION,
            hub_version: herd_runner::runner_version().to_owned(),
            state_root: "fake-hub-state-root".to_owned(),
            version_note: None,
            keeper_for: Vec::new(),
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
            packs_in: Vec::new(),
            packs_out: std::collections::BTreeMap::new(),
            need: None,
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

/// What one flood found, for the test that asked for it to assert on.
struct Flood {
    /// Job workspaces open on the runner when the flood had settled, and the
    /// most there were at any moment of it.
    workspaces: usize,
    peak_workspaces: usize,
    /// How many jobs the runner's own health report said it was running.
    health: Option<usize>,
    /// How many attempts it handed straight back, and one of the sentences it
    /// handed them back with.
    abandoned: usize,
    a_refusal: Option<String>,
    /// How long the control link took to answer, or `None` if it did not.
    probe: Option<Duration>,
}

/// One flood, or `None` when the host was too busy to measure anything on.
async fn flood(label: &str, how_many: usize) -> Option<Flood> {
    let before = load_one_minute();
    if before.is_some_and(|load| load > LOAD_CEILING) {
        // A `HERD-SKIPPED` now, where the first version of this file printed
        // a plain line. It asserted nothing then; it asserts a ceiling now,
        // and an assertion that did not run must never read as one that
        // passed (D27). **This skip is conditional on the host's load**, so
        // it is not part of the suite's standing skip reference: on a quiet
        // host it never appears.
        herd_runner::testing::skipped(
            label,
            &format!(
                "the host's one-minute load is {before:?}, over this bench's ceiling of \
                 {LOAD_CEILING}; the host is shared with a production system"
            ),
        );
        return None;
    }

    let hub = FakeHub::bind().await;
    let data_root = TempDir::new(label);
    let mut confinement = Confinement::with_data_root(data_root.path());
    // A trivial job needs no room, and a small ceiling is the honest shape of
    // "every job runs under an explicit memory cap" (invariant 2). It is also
    // half of the arithmetic under test: this figure and the memory the host
    // below says it lends are what the runner's own ceiling is derived from.
    confinement.memory_max = PER_JOB_BYTES;
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
        // Declared, never read from the machine this test runs on: the
        // ceiling is derived from these two figures, so reading them would
        // make the assertion a different one on every host.
        host: HostSize::new(Some(LENT_MIB), Some(4)).with_floor(Some(FLOOR_MIB)),
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
    let mut abandoned = 0usize;
    let mut a_refusal = None;
    while Instant::now() < settle {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(200), socket.next()).await
        {
            match from_frame::<ToHub>(text.as_str()) {
                Ok(ToHub::Health { jobs, .. }) => health = Some(jobs.len()),
                // The whole of how a refusal is said. Nothing new on the
                // wire: this is the frame a runner has always used to give an
                // attempt back without inventing a verdict for it, and the
                // hub's own policy decides what becomes of the job.
                Ok(ToHub::AttemptAbandoned { reason, .. }) => {
                    abandoned += 1;
                    a_refusal.get_or_insert(reason);
                }
                _ => {}
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

    println!("--- a hub that is not one, flooding a runner: {label} ---");
    println!("assignments sent: {how_many} in {handed_out:?}");
    println!("job workspaces open on the runner: {workspaces} (peak {peak_workspaces})");
    println!("attempts the runner handed straight back: {abandoned}");
    match &a_refusal {
        Some(reason) => println!("what it said when it refused: {reason}"),
        None => println!("the runner refused nothing"),
    }
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

    // **A flood cut short is a skip, not a smaller flood.** The settle loop
    // above is where all `how_many - EXPECTED_CEILING` abandonments are read,
    // so a loop that broke early has read fewer of them — and
    // [`a_flood_past_the_ceiling_is_held`] asserts the exact figure. Before
    // this, the cut-short path handed those truncated figures on and the
    // assertion turned a busy host into a **red** test. That is the opposite
    // of what this file's own D27 paragraph promises, and the opposite of
    // what the guard is for: the guard exists to protect the host, not to
    // invent failures on it. An assertion that did not run is printed as a
    // skip and never counted as a pass (D27).
    if let Some(load) = bit {
        herd_runner::testing::skipped(
            label,
            &format!(
                "the host's one-minute load reached {load} mid-flood, over this bench's \
                 ceiling of {LOAD_CEILING}, so the flood was cut short before it had read \
                 every abandonment; the figures it did have are printed above and are not \
                 asserted on. The host is shared with a production system"
            ),
        );
        return None;
    }

    Some(Flood {
        workspaces,
        peak_workspaces,
        health: health.or(reading.jobs_on_health),
        abandoned,
        a_refusal,
        probe: reading.probe,
    })
}

/// Everything one flood has to show, at whatever figure it was handed.
///
/// Three claims, and the third is the one invariant 2 is about: the runner
/// refuses past its own ceiling, it tells the truth about itself while doing
/// it, and the host never sees more workspaces than the runner said it could
/// hold.
fn a_flood_past_the_ceiling_is_held(found: &Flood, how_many: usize) {
    // Every [`Flood`] that reaches here is a complete one: a flood the load
    // guard cut short never becomes a `Flood` at all, it becomes a
    // `HERD-SKIPPED` line and a `None` (D27).
    assert!(
        found.peak_workspaces <= EXPECTED_CEILING,
        "the host saw {} job workspaces at once and this runner said it holds \
         {EXPECTED_CEILING}: a runner's limits are its own, not the hub's arithmetic \
         (invariant 2)",
        found.peak_workspaces
    );
    assert_eq!(
        found.workspaces, EXPECTED_CEILING,
        "and it should be holding exactly its ceiling, not fewer: a runner that refused \
         work it had room for would be a different defect with the same shape"
    );
    assert_eq!(
        found.health,
        Some(EXPECTED_CEILING),
        "its own health report has to say what it is really running: a hub reading a runner \
         that under-reports cannot place anything correctly"
    );
    assert_eq!(
        found.abandoned,
        how_many - EXPECTED_CEILING,
        "every assignment past the ceiling has to come back as an abandoned attempt, so the \
         hub can place the job somewhere else: a job the runner simply dropped would wait \
         for a watchdog that is minutes away"
    );
    let refusal = found
        .a_refusal
        .as_deref()
        .expect("the runner says why it refused");
    assert!(
        refusal.contains(herd_runner::A_RUNNERS_OWN_CEILING) && refusal.contains("floor"),
        "the refusal has to carry the arithmetic, or nobody can tell a full machine from a \
         broken one: {refusal}"
    );
    assert!(
        found.probe.is_some_and(|took| took < PROBE_BUDGET),
        "and the control link stays answerable throughout: {:?}",
        found.probe
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fifty_assignments_from_a_hub_that_is_not_one_stop_at_this_runner_s_own_ceiling() {
    let Some(found) = flood("flood-fifty", 50).await else {
        return;
    };
    a_flood_past_the_ceiling_is_held(&found, 50);
}

/// The figure the audit measured at 250 of 250 admitted
/// (`docs/remediation/04b-integrity-and-containment.md` §2.2). Kept at 250
/// because the defect was that the number did not matter.
#[tokio::test(flavor = "multi_thread")]
async fn two_hundred_and_fifty_assignments_from_a_hub_that_is_not_one_stop_at_the_same_ceiling() {
    let Some(found) = flood("flood-two-fifty", 250).await else {
        return;
    };
    a_flood_past_the_ceiling_is_held(&found, 250);
}
