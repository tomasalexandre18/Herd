//! A Herd runner: dials the hub, executes what it is assigned under caps,
//! streams the output as it is produced, reports how the job ended and what
//! it cost.
//!
//! The runner opens the connection and **binds no listening socket at all**
//! (invariant 3). The mesh-bound fetch endpoint of D10 and D11 arrives in
//! M11; until then there is nothing to bind, and a test asserts it rather
//! than trusting it.
//!
//! **The link is assumed to be unreliable, not to be reliable.** The hub goes
//! away and comes back, the connection dies mid-chunk, the owner asks the
//! runner to stop while a build is running. Three rules carry all of it.
//!
//! 1. *Nothing a job produced is forgotten until the hub says it has it.* A
//!    result is held until `JobResultAck` (E-44), and so is every log chunk,
//!    until `LogAck` — bounded, because a runner may not grow without limit on
//!    behalf of a hub that is gone. A verdict still unacknowledged when the
//!    runner leaves is written to its data root and delivered by the next
//!    start, with no human in the loop (D34).
//! 2. *A dropped link is never the end of the runner.* It redials on a growing,
//!    jittered, capped delay, and gives up only after a bound long enough to
//!    outlast any maintenance window, saying so by name.
//! 3. *The runner says what it is doing, and listens for the answer.* A health
//!    report every ~15 s is how the hub can tell a working runner from a lost
//!    one (E-40, E-21), and the hub's answer is how the runner can tell a live
//!    link from a socket that is open onto nothing.
//!
//! Confinement is prepared once, before the first dial, and outlives every
//! connection — the data root is locked for the life of the process, so it
//! could not be prepared again anyway. It lives in modules named for the
//! thing each one owns: [`root`] for the one data root (E-45), [`privilege`]
//! for staying unprivileged (E-46), [`cgroup`] for the kernel memory ceiling
//! (E-30), [`confine`] plus [`job`] for the caps a job runs under (E-34), and
//! [`spool`] for a verdict that outlives the process holding it.
//!
//! What a runner *offers* is [`capability`]'s (M5): the capabilities it
//! detected, those its owner declared, those its owner denied, and what their
//! external probes say. Probes are read beside the redial loop rather than
//! inside a connection, because a probe is a fact about the host and goes on
//! being true while there is no hub to tell.

pub mod blockio;
pub mod capability;
pub mod cgroup;
pub mod confine;
pub mod job;
pub mod metrics;
mod outputs;
pub mod privilege;
pub mod reclaim;
pub mod room;
pub mod root;
pub mod size;
pub mod spool;
mod step;
pub mod switch;
#[doc(hidden)]
pub mod testing;

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, JobOutputs, LogStream, Role,
    RunningJob, StepRun, ToHub, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{watch, Semaphore};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub use confine::{
    jobs_at_once, Confined, Confinement, JobsAtOnce, A_RUNNERS_OWN_CEILING, DEFAULT_MEMORY_MAX,
    JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY,
};
use spool::Spooled;

/// Where a runner looks for its hub unless told otherwise.
pub const DEFAULT_HUB_URL: &str = "ws://127.0.0.1:8787";

/// How often a runner says it is alive and what it is doing (E-40).
///
/// The figure lives in `herd-proto`, because the hub's silence threshold is
/// only meaningful as a multiple of it: two crates each holding their own
/// 15 s would be two truths about one contract (invariant 4).
pub use herd_proto::HEALTH_EVERY;

/// How long a runner goes on believing a link on which the hub has said
/// nothing at all.
///
/// The same figure as the hub's own silence threshold, and for the same
/// reason: a TCP connection can stay open to a hub that no longer exists —
/// a container stopped without a FIN, a NAT table that forgot — and a runner
/// that only ever writes would hold that socket for ever, running nothing and
/// being counted as nothing. The hub answers every health report, so four
/// unanswered reports is a dead link.
pub const HUB_SILENCE_LIMIT: Duration = Duration::from_secs(60);

/// How long a draining runner gives its jobs and its link to finish.
///
/// Invariant 2 says ≤ 5 s, and this is the whole of what happens inside it:
/// every running job is told to end, the frames saying so are flushed, and
/// then the process exits whether or not that worked. Comfortably under the
/// promise on purpose — the promise is about the owner's machine being given
/// back, and a runner that negotiates its own shutdown for five seconds has
/// already broken it.
pub const DRAIN_GRACE: Duration = Duration::from_secs(3);

/// How often the drain looks at whether it is done.
const DRAIN_POLL: Duration = Duration::from_millis(20);

/// How long the last frames a drain produces are given to leave.
///
/// Carved out of [`Config::drain_grace`], never added to it: the five seconds
/// invariant 2 promises the owner is the whole of the stop, and a flush that
/// the waiting had already spent would never happen at all.
const FINAL_FLUSH: Duration = Duration::from_millis(200);

/// How much of a job's output a runner holds against an acknowledgement.
///
/// The hold is what makes a lost link cost the log nothing (D36): the chunks
/// the hub never wrote are still here and are sent again. But a runner may
/// not be made to grow without limit on behalf of a hub that is gone — the
/// first rule about a runner is that it does not destabilise its host
/// (invariant 2) — so the oldest chunks are dropped past this, and what is
/// dropped is what the hub's gap marker names (D29). 4 MiB is minutes of a
/// chatty build's output and a rounding error of the memory a build itself
/// takes.
const HELD_BYTES_PER_JOB: usize = 4 * 1024 * 1024;

/// One line of the runner's journal, written to the descriptor rather than
/// through `eprintln!`.
///
/// The same bytes to the same place; the difference is that they can be
/// counted. `eprintln!` consults libtest's per-thread output capture, which
/// is also inherited by every thread a test spawns, so under `cargo test` the
/// macro's lines never reach file descriptor 2 and no test can measure how
/// many of them a path wrote. For D151's second defect that count is the only
/// thing worth measuring — the defect was never what the line said, it was
/// that one true line was written 1497 times — so this path writes where a
/// test can divert it ([`testing::stderr_lines_about`]).
fn journal(line: &str) {
    use std::io::Write;
    // A journal that cannot be written to is not worth a panic in a runner
    // whose job is to keep running: the line is lost, the work is not.
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

/// How many written-off attempts a runner remembers having already announced
/// (D151).
///
/// Only wide enough that the floods of every job a runner could plausibly be
/// holding at once cannot push each other out and start repeating. It is a
/// ring: the oldest is forgotten, and the only cost of forgetting one is a
/// second line about an attempt whose frames stopped arriving long ago.
const REFUSALS_REMEMBERED: usize = 32;

/// How long a dead connection's writer is given to finish before it is
/// dropped. The link is over by then; what matters is not waiting for the jobs
/// that are still running, because their frames go to the next link.
const FLUSH_GRACE: Duration = Duration::from_millis(500);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Socket, Message>;
type Source = SplitStream<Socket>;
type Outbox = UnboundedSender<ToHub>;

/// How long the runner waits before dialling the hub again, and when it stops.
///
/// Growing, capped, jittered, and bounded by elapsed time rather than by a
/// number of attempts — because what has to be survived is a *duration*: a
/// hub down for a maintenance window, a server rebooting, a reverse proxy
/// being reconfigured. Counting attempts would make the bound depend on the
/// delay, which is the thing that changes.
///
/// The jitter is not decoration. Every runner of a hub that goes down redials
/// on the same schedule, and a herd of them synchronised on a 30 s boundary is
/// a thundering herd against a hub that has just come up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// The first delay, and the unit the doubling starts from.
    pub first: Duration,
    /// The longest a delay ever gets, before jitter.
    pub cap: Duration,
    /// How long the runner goes on failing before it gives up and says so.
    pub give_up_after: Duration,
}

impl Default for Backoff {
    /// 500 ms, doubling to 30 s, for a day.
    ///
    /// The three numbers answer three different questions. **500 ms** is for
    /// the common case, a hub restarted by its deployment: the runner is back
    /// before anybody notices. **30 s** is the floor on how often a down hub
    /// is knocked on — often enough that a runner rejoins within half a minute
    /// of the hub coming up, rare enough that a hundred runners cost it
    /// nothing. **24 h** is the give-up, and it is the one real choice here: a
    /// maintenance window, a reboot, a certificate renewal gone wrong, a
    /// weekend of nobody looking — all of that is hours, not days, so a day of
    /// trying outlasts every window Herd is meant to survive, while still
    /// bounding a runner pointed at a URL that will never answer. A runner
    /// that never gives up hides a typo in its configuration for ever; one
    /// that gives up in five minutes needs a human for an ordinary reboot.
    ///
    /// The 500 ms claim is only true because of [`Backoff::settled`]: it is
    /// the delay after the *first* failure, so it is what a hub restarted by
    /// its deployment gets only if the connection it ended counted as having
    /// served. Without that the figure described the first restart and
    /// nothing after it (D104).
    fn default() -> Self {
        Self {
            first: Duration::from_millis(500),
            cap: Duration::from_secs(30),
            give_up_after: Duration::from_secs(24 * 60 * 60),
        }
    }
}

impl Backoff {
    /// The delay before attempt number `failures` (1 for the first retry),
    /// doubled each time, capped, and jittered down by up to half.
    pub fn delay(&self, failures: u32) -> Duration {
        let grown = self
            .first
            .saturating_mul(2u32.saturating_pow(failures.saturating_sub(1).min(30)));
        let capped = grown.min(self.cap);
        let half = capped / 2;
        half + scale(half, jitter_fraction())
    }

    /// Whether a connection that lasted `served` has earned the counters a
    /// fresh start (D104).
    ///
    /// `None` means the attempt was never welcomed at all — a dial that was
    /// refused, or one that hung until it timed out. That is the case the
    /// backoff exists for, and it looks identical on the clock to a link that
    /// served for the same span, which is why the caller has to say which it
    /// was rather than time it.
    ///
    /// **The floor is the cap, and that is the whole argument.** A hub that
    /// accepts and dies in a loop would otherwise be answered every `first`
    /// for ever, and the give-up bound would never fire on it, because each
    /// short connection would wipe the record of the last. With the floor
    /// here, a reset can only follow a connection that lasted at least as
    /// long as the longest delay this backoff would ever have waited — so no
    /// reset can make a runner dial faster than once per cap, whatever a
    /// broken hub does.
    pub fn settled(&self, served: Option<Duration>) -> bool {
        served.is_some_and(|served| served >= self.cap)
    }

    /// What a runner says when it stops trying.
    pub fn gave_up(&self, hub_url: &str, failures: u32) -> String {
        format!(
            "giving up on {hub_url}: {failures} attempt(s) over {} failed, which is \
             longer than a runner waits for a hub to come back",
            humanise(self.give_up_after)
        )
    }
}

fn humanise(span: Duration) -> String {
    let seconds = span.as_secs();
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}

fn scale(span: Duration, fraction: f64) -> Duration {
    Duration::from_secs_f64(span.as_secs_f64() * fraction)
}

/// A fraction in `[0, 1)`, cheap and good enough to spread redials out.
///
/// Deliberately not a random-number library: the only thing asked of this is
/// that two runners dialling the same hub do not keep the same phase, and the
/// clock's nanoseconds plus a counter answer that. A dependency for it would
/// be a dependency carried into the deployment for the sake of a jitter.
fn jitter_fraction() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos() as u64);
    let mut state = nanos ^ (COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed));
    // One round of splitmix64: enough mixing that the low bits move.
    state = (state ^ (state >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    state = (state ^ (state >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    state ^= state >> 31;
    (state >> 11) as f64 / (1u64 << 53) as f64
}

/// Which build of `herd-runner` this is, as the handshake announces it
/// (E-51).
///
/// The crate's own version, not the hub's and not `herd-proto`'s: the point
/// of the figure is that the two sides of a connection can be different
/// builds, and a runner that reported a number it read from the same source
/// the hub reads would never report a gap at all.
pub fn runner_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// What a runner needs to know to join a hub and to confine what it runs.
#[derive(Debug, Clone)]
pub struct Config {
    /// `ws://host:port` inside the compose network and in tests, `wss://...`
    /// through the reverse proxy.
    pub hub_url: String,
    /// Name reported to the hub, for its logs and reports.
    pub name: Option<String>,
    /// Bearer token. Carried from M1, checked by the hub from M15.
    pub token: Option<String>,
    /// How often to report health (E-40).
    pub health_every: Duration,
    /// How long to believe a link the hub has said nothing on.
    pub hub_silence_limit: Duration,
    /// When to dial again, and when to stop.
    pub redial: Backoff,
    /// How long a drain may take before the process exits regardless.
    pub drain_grace: Duration,
    /// The one data root and every cap a job runs under (M4).
    pub confinement: Confinement,
    /// What this runner offers, each capability with its origin (M5, E-24).
    /// Resolved before the first dial, because it is what joining means.
    pub capabilities: herd_proto::CapabilityOffer,
    /// How often the external busy probes are read, and how long one may
    /// take (E-26). Injectable because a test that really waited out the
    /// default interval is a test CI drops.
    pub probes: capability::ProbeSettings,
    /// How big this host says it is: the memory and the threads Herd's jobs
    /// may use here (E-22).
    ///
    /// Read from the host by default ([`metrics::host_size`]) and overridable
    /// at startup, because `MemTotal` is the whole machine and the owner of a
    /// machine is the authority on how much of it she is lending. A figure
    /// that cannot be read stays absent, and a runner that announces no size
    /// is one no derived-`-j` job is placed on — said out loud by the hub
    /// when it joins, rather than left to be guessed from a queue that will
    /// not move.
    pub host: herd_proto::HostSize,
    /// How preferred this runner is when more than one could run a job
    /// (E-19). Higher first; zero unless its owner says otherwise.
    pub preference: i32,
    /// Where the owner's switch is kept (E-33).
    ///
    /// A path, because invariant 3 forbids anything on this machine to listen:
    /// `herd-runner switch off --reason ...` writes this file and the running
    /// runner reads it. `None` turns the whole mechanism off, which is for a
    /// test that is about something else — a runner with no switch file claims
    /// as it always did.
    pub switch_file: Option<PathBuf>,
    /// How often the switch file is read (E-33).
    ///
    /// A second by default. Injectable because a test that is about the ten
    /// seconds must be able to measure them without them being mostly this.
    pub switch_poll: Duration,
    /// How many file chunks of a streamed collection may be queued on one
    /// link at a time (D148): [`FILES_IN_FLIGHT`] unless a test says
    /// otherwise.
    ///
    /// Injectable for the reason [`Confinement::output_cap`] is. The failure
    /// this window has is that it *runs down* across collections, and
    /// reproducing that at the shipped figure costs nine real transfers of
    /// over 32 MiB each; at a window of one it costs two, and it is the same
    /// mechanism either way (D158).
    pub files_in_flight: usize,
    /// How long a collection's pump waits for room on that window before it
    /// says so on stderr (D158).
    pub files_stall_after: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hub_url: DEFAULT_HUB_URL.to_owned(),
            name: None,
            token: None,
            health_every: HEALTH_EVERY,
            hub_silence_limit: HUB_SILENCE_LIMIT,
            redial: Backoff::default(),
            drain_grace: DRAIN_GRACE,
            confinement: Confinement::with_data_root(
                root::DataRoot::default_path().unwrap_or_else(|_| "herd-data".into()),
            ),
            capabilities: herd_proto::CapabilityOffer::empty(),
            host: metrics::host_size(),
            preference: 0,
            probes: capability::ProbeSettings::default(),
            // Under the data root, which is the one directory this runner may
            // write in (E-45). Resolved by the binary, because only it knows
            // where the root really ended up.
            switch_file: None,
            switch_poll: crate::switch::Poll::default().0,
            files_in_flight: FILES_IN_FLIGHT,
            files_stall_after: FILES_STALL_AFTER,
        }
    }
}

/// The hub said no. Terminal for this runner (D13), so it is never retried: a
/// version mismatch or a refused runner does not become true by dialling
/// again.
#[derive(Debug)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the hub refused this runner: {}", self.0)
    }
}

impl std::error::Error for Refusal {}

/// The hub asked this runner to come back shortly, and why.
///
/// The opposite of a [`Refusal`], and the distinction D13 asked to be kept:
/// waiting is not being refused (D38). The commonest cause is the one M2
/// could not handle — a runner redialling faster than the hub notices its
/// previous connection is gone — and the answer is to come back, not to die.
#[derive(Debug)]
pub struct StandBy {
    pub reason: String,
    pub retry_in: Duration,
}

impl std::fmt::Display for StandBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the hub asked this runner to stand by for {:?}: {}",
            self.retry_in, self.reason
        )
    }
}

impl std::error::Error for StandBy {}

/// Why this runner would not take a job the hub assigned it.
///
/// Every one of these becomes a `ToHub::AttemptAbandoned` and nothing else:
/// the attempt is handed straight back with a reason, the job is untouched,
/// and what becomes of it — one more go somewhere else, or an end — stays the
/// hub's policy and never the runner's. That is the frame the protocol
/// already has for exactly this (`herd_proto::ToHub::AttemptAbandoned`), and
/// using it is why a runner holding its own limits needs no new variant on
/// any enum and no new version number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotTaken {
    /// The runner is stopping (invariant 2) and the hub had already sent the
    /// assignment.
    Draining,
    /// Its owner has withdrawn the machine (E-33).
    Switched(herd_proto::Switch),
    /// It is already holding as many jobs as it can hold at its own caps
    /// (invariant 2, and [`confine::jobs_at_once`]). **Not a statement about
    /// the hub**: a hub whose arithmetic is right never reaches this, and a
    /// runner that reached it says so rather than finding out what N times
    /// its memory ceiling does to its host.
    AtItsCeiling { running: usize, most: usize },
}

impl NotTaken {
    /// The sentence the hub is given, which is the whole of what a reader of
    /// the record learns about why this machine said no.
    pub(crate) fn sentence(&self, ceiling: &confine::JobsAtOnce) -> String {
        match self {
            NotTaken::Draining => "the runner is draining and never started this job".to_owned(),
            NotTaken::Switched(switch) => format!(
                "the runner is switched {} and never started this job",
                switch.sentence()
            ),
            NotTaken::AtItsCeiling { running, .. } => format!(
                "the runner is already running {running} job(s) and never started this job: {}",
                ceiling.why()
            ),
        }
    }
}

/// How a job is being ended from outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ending {
    /// A requester asked for it (E-06): the job is over and `Cancelled` is
    /// its outcome.
    Cancelled(String),
    /// This *attempt* is over and the job is not: the runner is stopping, or
    /// the hub has written the attempt off. No outcome is invented for it —
    /// the hub is told the attempt was abandoned and decides what follows.
    Abandoned(String),
}

impl Ending {
    pub(crate) fn reason(&self) -> &str {
        match self {
            Ending::Cancelled(reason) | Ending::Abandoned(reason) => reason,
        }
    }
}

/// The one way a job is ended from outside the task running it.
///
/// Every path that stops a job early goes through here — a requester's cancel
/// (E-06), a drain (invariant 2), a hub telling the runner its attempt is
/// stale (E-21) — so there is exactly one place where a job's process tree is
/// killed, and that place is [`job::execute`], which takes the cgroup, the
/// process group and everything still carrying the job's marker with it.
#[derive(Debug, Clone)]
pub(crate) struct CancelToken {
    tx: watch::Sender<Option<Ending>>,
}

impl CancelToken {
    fn new() -> Self {
        Self {
            tx: watch::channel(None).0,
        }
    }

    /// Ask for the job to end. False if it was already asked.
    fn end(&self, ending: Ending) -> bool {
        let mut asked = false;
        self.tx.send_if_modified(|current| {
            if current.is_none() {
                *current = Some(ending);
                asked = true;
            }
            asked
        });
        asked
    }

    /// Resolves once the job has been asked to end.
    pub(crate) async fn ended(&self) -> Ending {
        let mut rx = self.tx.subscribe();
        loop {
            if let Some(ending) = rx.borrow_and_update().clone() {
                return ending;
            }
            if rx.changed().await.is_err() {
                // Nobody can ask any more; wait for the process instead.
                std::future::pending::<()>().await;
            }
        }
    }
}

/// How a job ended and what it cost, as the runner holds it until the hub
/// says it has it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ended {
    outcome: JobOutcome,
    metrics: JobMetrics,
    /// Every executor step this attempt ran, in order (D77). Empty for a job
    /// that is not a declared command, which is every job before M10.
    steps: Vec<StepRun>,
    /// What the job left under `{out}`, collected before the workspace was
    /// reclaimed ([`crate::outputs`]).
    ///
    /// Held here with the verdict, and therefore spooled with it (D34): a
    /// runner asked to stop between a job's end and the hub's acknowledgement
    /// writes the whole `job_finished` frame to its data root, so the output
    /// comes back on the next start along with the verdict it belongs to.
    /// There is no second place a result's files could be held, and that is
    /// the point of putting them on the frame.
    outputs: JobOutputs,
}

/// What one entry of [`Inner::jobs`] is about: **a job and the attempt of it
/// that this runner has in hand** (D-NEXT).
///
/// The key is both, and that is the whole of it. A runner can hold two
/// attempts of one job at once — the hub writes an attempt off while the link
/// is down, the job goes on running here, and the next attempt of it comes
/// back to this same machine the moment it redials — and keyed by job id
/// alone the second entry simply *replaced* the first. Three things followed,
/// every one of them observed: the first attempt ran with nothing counting it
/// against this machine's own ceiling, its held output was numbered as the
/// second attempt's, and handing either one back or acknowledging either one's
/// verdict let go of whichever entry happened to be there.
///
/// It does **not** make running one job twice on one machine a good idea: a
/// workspace belongs to a job and not to an attempt, so two attempts of one
/// job here still clear each other's working directory. What this key buys is
/// that the machine's own books say what is really on it, whoever decides what
/// may be taken.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InHand {
    job_id: JobId,
    attempt: u32,
}

impl InHand {
    fn of(job_id: &JobId, attempt: u32) -> Self {
        Self {
            job_id: job_id.clone(),
            attempt,
        }
    }
}

/// One job this runner has in hand, and everything the hub may still need
/// from it.
///
/// Which attempt of which job is the key it is filed under ([`InHand`]) and
/// is deliberately not repeated here: two places to read an attempt number
/// from are two numbers that can disagree.
#[derive(Debug)]
struct JobInHand {
    /// The number the next chunk of this job's output will carry. Per
    /// attempt: a second attempt is a second process and counts from zero.
    next_seq: u64,
    /// Chunks sent and not yet acknowledged, oldest first.
    unacked: VecDeque<(u64, LogStream, Vec<u8>)>,
    unacked_bytes: usize,
    /// How the job ended, once it has, until the hub acknowledges it (E-44).
    end: Option<Ended>,
    cancel: CancelToken,
    /// How many chunks of this attempt the held-bytes cap has dropped.
    ///
    /// Counted rather than announced one by one, for the reason given on
    /// [`Inner::said_stale`]: the cap bites once per chunk for as long as the
    /// link is down, so a chatty job on a broken link wrote a journal line
    /// per chunk — the same flood, on the same trigger, reached by a
    /// different path (D151). The first drop says what the cap is; the total
    /// is said once, when the job is let go.
    trimmed: u64,
}

impl JobInHand {
    fn remember(&mut self, seq: u64, stream: LogStream, data: &[u8]) {
        self.unacked_bytes += data.len();
        self.unacked.push_back((seq, stream, data.to_vec()));
        while self.unacked_bytes > HELD_BYTES_PER_JOB {
            match self.unacked.pop_front() {
                Some((dropped, _, bytes)) => {
                    self.unacked_bytes -= bytes.len();
                    self.trimmed += 1;
                    if self.trimmed == 1 {
                        eprintln!(
                            "herd-runner: chunk {dropped} is no longer held: this runner holds \
                             {HELD_BYTES_PER_JOB} bytes of a job's output against an \
                             acknowledgement and no more; further chunks it drops are counted, \
                             not announced"
                        );
                    }
                }
                None => break,
            }
        }
    }

    fn forget_through(&mut self, through_seq: u64) {
        while self
            .unacked
            .front()
            .is_some_and(|(seq, _, _)| *seq < through_seq)
        {
            if let Some((_, _, bytes)) = self.unacked.pop_front() {
                self.unacked_bytes -= bytes.len();
            }
        }
    }
}

/// The `job_finished` frame of one job's end, which is what both the link and
/// the spool carry: the protocol is the contract (invariant 4), so a verdict
/// on a disk has no shape of its own.
fn finished_frame(job_id: &JobId, attempt: u32, chunks_sent: u64, end: &Ended) -> ToHub {
    ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt,
        outcome: end.outcome.clone(),
        chunks_sent,
        metrics: end.metrics.clone(),
        steps: end.steps.clone(),
        outputs: end.outputs.clone(),
    }
}

/// What a runner keeps across connections: the jobs it has in hand, the
/// output and results the hub has not acknowledged, the verdicts a previous
/// life left on the disk, and the link they all report on.
///
/// A job's result is the one thing a runner cannot reproduce: the process is
/// gone, and if the result is dropped the work is lost. Its output is nearly
/// as bad — a build log is why anyone reads a failed job at all. So both are
/// kept here from the moment they are produced until the hub says it has them
/// on a disk, and every connection starts by sending again whatever is left
/// (E-44, D36). The hub records the same end twice as the same end and drops
/// a chunk it already has, so sending again what did in fact arrive is
/// harmless.
///
/// What is held here is memory, which a runner that is itself killed loses —
/// that is the lost-attempt case the hub requeues (E-21). What a runner that
/// is *asked* to stop holds goes to its data root instead (D34) and is picked
/// up by the next start: [`Session::spool`] and [`Session::pick_up`].
#[derive(Debug, Clone, Default)]
pub struct Session {
    inner: Arc<Mutex<Inner>>,
    /// What this runner offers and what its probes say (M5). It belongs here
    /// rather than to a connection for the same reason a held result does: it
    /// is true of the host, and it has to be told again to whichever hub is
    /// up next.
    capacity: capability::Capacity,
}

/// One attempt's collection waiting on this runner's disk for the hub
/// (D148).
///
/// **Separate from [`JobInHand`], and that is deliberate.** A verdict is
/// acknowledged when it is written and the job leaves the live list then; the
/// files are acknowledged when the last of them is fsynced, which may be
/// minutes later. Keeping the collection in `jobs` would have tied the one to
/// the other — either a runner holding a finished job open for hundreds of
/// megabytes, or a release that dropped the bytes it still owed.
#[derive(Debug)]
struct FilesOut {
    attempt: u32,
    /// Where the bytes are: the renamed `{out}`, under the data root.
    spool: PathBuf,
    /// What the verdict announced, in the order it announced it.
    manifest: Vec<herd_proto::OutputFile>,
    /// Bytes of each entry handed to the link so far.
    sent: Vec<u64>,
    /// Whether any chunk of each entry has been sent, which is how a
    /// zero-length file is told from one not started: an empty file is a real
    /// file and the hub has to be told to create it, so it gets exactly one
    /// empty chunk.
    touched: Vec<bool>,
}

impl FilesOut {
    fn new(attempt: u32, spool: PathBuf, manifest: Vec<herd_proto::OutputFile>) -> Self {
        let count = manifest.len();
        Self {
            attempt,
            spool,
            manifest,
            sent: vec![0; count],
            touched: vec![false; count],
        }
    }

    /// Start again from what the hub says it holds.
    ///
    /// Anything the hub holds complete is marked sent, and a part-written
    /// file resumes at the hub's own length — never at the runner's idea of
    /// how far it had got, which is exactly the thing a dropped link
    /// invalidated.
    fn resume_from(&mut self, held: &[herd_proto::FileHeld]) {
        for (index, entry) in self.manifest.iter().enumerate() {
            let bytes = held
                .iter()
                .find(|file| file.path == entry.path)
                .map(|file| file.bytes)
                .unwrap_or(0);
            self.sent[index] = bytes.min(entry.len);
            // An empty entry the hub already holds needs no second empty
            // chunk; one it does not hold needs its one.
            self.touched[index] = held.iter().any(|file| file.path == entry.path);
        }
    }

    /// What to send next, with the cursor advanced for it.
    ///
    /// Returns the *plan* rather than the frame so the file read happens
    /// outside the session's lock: a 1 MiB read while holding the mutex every
    /// other part of the runner contends on would make a streaming job
    /// contend with its own health report.
    fn next(&mut self) -> Option<ChunkPlan> {
        for index in 0..self.manifest.len() {
            let entry = &self.manifest[index];
            if entry.len == 0 {
                if self.touched[index] {
                    continue;
                }
                self.touched[index] = true;
                return Some(ChunkPlan {
                    attempt: self.attempt,
                    path: entry.path.clone(),
                    file: self.spool.join(&entry.path),
                    offset: 0,
                    take: 0,
                });
            }
            if self.sent[index] >= entry.len {
                continue;
            }
            let offset = self.sent[index];
            let take = herd_proto::OUTPUT_CHUNK_BYTES.min(entry.len - offset);
            self.sent[index] = offset + take;
            self.touched[index] = true;
            return Some(ChunkPlan {
                attempt: self.attempt,
                path: entry.path.clone(),
                file: self.spool.join(&entry.path),
                offset,
                take: take as usize,
            });
        }
        None
    }
}

/// One read to make and one frame to send, decided under the lock and carried
/// out outside it.
struct ChunkPlan {
    attempt: u32,
    path: String,
    file: PathBuf,
    offset: u64,
    take: usize,
}
#[derive(Debug)]
struct Inner {
    /// Who this runner *process* is, beside the name its owner gave it.
    ///
    /// Minted here because this is the scope it has to match: one Session is
    /// one runner, for as long as that runner lives, across every connection
    /// it makes. The hub compares it to tell a redial — the same process,
    /// dialling again — from a twin, a second process configured with the
    /// same name. A name cannot tell them apart, and told apart by a name
    /// alone two live twins supersede each other for ever.
    instance: String,
    /// Whichever connection to the hub is up now.
    ///
    /// A job outlives the connection it was assigned on (D17), so it must not
    /// hold that connection's own channel: a job that ends after the runner
    /// has redialled reports on the link that is up, not into the one that
    /// broke. With no link at all a frame is simply not sent — and nothing is
    /// lost by that, because everything a job produces is held here until the
    /// hub acknowledges it.
    link: Option<Outbox>,
    /// How many file chunks may be queued on the link that is up now.
    ///
    /// **The only backpressure the runner has, and it needs one.** The link's
    /// channel is unbounded, so a pump reading a 400 MiB spool as fast as the
    /// disk allows would put 400 MiB into memory on its way to a socket that
    /// is slower than the disk. A permit is taken before a chunk is queued and
    /// given back by [`write_frames`] once that chunk is on the wire, so what
    /// is in flight is bounded by [`FILES_IN_FLIGHT`] chunks whatever the two
    /// speeds are.
    ///
    /// Per **connection**, like the link beside it: chunks queued on a link
    /// that broke are dropped with it, so their permits are gone too, and a
    /// counter carried across would run down to nothing over a few redials.
    /// A new link starts with a full window and the hub says where to resume
    /// from ([`herd_proto::FromHub::JobFilesHeld`]).
    files_credit: Option<Arc<Semaphore>>,
    /// Every attempt this runner has in hand, by job **and** attempt
    /// ([`InHand`]).
    jobs: HashMap<InHand, JobInHand>,
    /// Collections whose bytes this runner still owes the hub, by job.
    ///
    /// Outlives `jobs`: a verdict is acknowledged and the job released long
    /// before hundreds of megabytes have crossed, and a runner that held the
    /// job open for its files would be a runner whose finished work never
    /// finishes.
    collections: HashMap<JobId, FilesOut>,
    /// Verdicts a previous life of this runner wrote to the data root and
    /// nobody has acknowledged yet. Not jobs: nothing of them is running, and
    /// nothing of them is counted as work this runner is doing.
    spooled: Vec<Spooled>,
    /// The runner is stopping: it takes no new job (invariant 2).
    draining: bool,
    /// When the hub last welcomed a connection of this session, if ever.
    ///
    /// Here rather than on a connection because the thing it is asked about
    /// outlives every one of them: whether *this* runner has ever been
    /// serving, and for how long, which is what tells a hub that is down
    /// from one that is up and restarting (D104).
    welcomed_at: Option<Instant>,
    /// Attempts this runner has already said out loud the hub has written
    /// off, newest last (D151).
    ///
    /// The hub answers **every** frame about a stale attempt with its own
    /// `StaleAttempt`, which is right — a frame it will not record must be
    /// refused by name. But a runner that buffered a thousand log chunks
    /// while the link was down flushes all of them the moment it is back and
    /// gets a thousand refusals, one per chunk, inside the same millisecond.
    /// Said once per chunk that is a thousand journal writes onto a host
    /// which is, by construction, the host whose IO just stalled long enough
    /// to break the link. The runner would be making the stall worse at the
    /// worst possible moment, which is the opposite of what invariant 2
    /// promises. So the sentence is said once per attempt and the rest are
    /// silent.
    ///
    /// Not on [`JobInHand`], because the job entry is dropped by the first
    /// refusal and every repeat arrives after it: the memory has to outlive
    /// the job. Bounded at [`REFUSALS_REMEMBERED`] rather than a set that
    /// grows for the life of the process — and more than one slot because two
    /// jobs' floods can interleave, and a single slot would ping-pong and
    /// flood all over again.
    said_stale: VecDeque<(JobId, u32)>,
    /// Where its owner's switch stands (E-33).
    ///
    /// Separate from `draining` above, which is the process stopping. This is
    /// slower and does less: `off` takes no new job and serves no content
    /// while **leaving the job in flight alone**, and a runner that killed
    /// its build because its owner wanted the machine back in ten seconds
    /// would have lost her exactly the work she was being polite about.
    switch: herd_proto::Switch,
    /// How long a collection's pump waits for room on the link before it says
    /// so (D158).
    ///
    /// Carried here rather than read from a constant so that a test can see a
    /// real pump wait out a real bound without waiting out the shipped one.
    /// Set from [`Config::files_stall_after`] when a connection attaches,
    /// like the window it is about.
    files_stall_after: Duration,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            instance: mint_an_instance_id(),
            link: None,
            files_credit: None,
            files_stall_after: FILES_STALL_AFTER,
            jobs: HashMap::new(),
            collections: HashMap::new(),
            spooled: Vec::new(),
            draining: false,
            welcomed_at: None,
            said_stale: VecDeque::new(),
            switch: herd_proto::Switch::On,
        }
    }
}

/// What a pump says when the link's window has not opened for it (D158).
///
/// **The sentence that would have named this outage in ten seconds.** A
/// collection waiting here is a job whose verdict is already on the hub and
/// whose bytes are not moving, which from every other vantage point looks
/// like an absence with no cause: the hub holds a manifest and no files, and
/// the requester is told what it did not get. Only the runner knows it is
/// waiting, so only the runner can say it — by job, with the figure, and
/// saying which of the two it is, because a window that has room for nobody
/// is a defect in this runner and a window that is merely full is a slow
/// socket.
fn waiting_for_room(job_id: &JobId, waited: Duration) -> String {
    format!(
        "the files of {job_id} have waited {waited:?} for room on the link to the hub and \
         none came. Their bytes are on this runner, the hub has a manifest and no file, and \
         a requester asking for them is being told it did not get them. Nothing is wrong \
         with the collection: the window this runner sends chunks through is not opening."
    )
}

/// Why a spool this runner owns the top of will nonetheless not be removed,
/// when the reason is one a reader cannot see from the error alone (D158).
///
/// `Permission denied` on a tree whose every directory is `drwxr-xr-x` reads
/// as impossible, and the hour it costs to work out goes on the one fact the
/// error does not carry: **unlinking is a write to the directory that holds
/// the name**, so a directory the runner did not create is a directory the
/// runner cannot empty, whatever its mode says. A job that moves a tree it
/// did not make into `{out}` — a content-addressed store prepared beforehand,
/// say — leaves exactly that, and no `chmod` fixes it because `chmod` is also
/// the owner's to make.
fn why_it_will_not_go(spool: &Path) -> Option<String> {
    let mine = crate::privilege::effective_uid()?;
    let (path, owner) = foreign_directory_under(spool, mine)?;
    Some(format!(
        "{} belongs to uid {owner} and this runner is uid {mine}, so it may not unlink what \
         is inside it however the directory is moded — a job that moved a tree it did not \
         create into its output directory leaves one of these behind",
        path.display()
    ))
}

/// The shallowest directory under `spool` that `mine` does not own, with its
/// owner.
///
/// Breadth first, because the shallowest is the one an operator acts on: one
/// foreign directory near the top usually holds all the others, and naming a
/// leaf six levels down would have her fix one of hundreds. Symlinks are not
/// followed, for the reason [`crate::outputs`] does not follow them. Bounded
/// at [`FOREIGN_SEARCH_LIMIT`] entries: this runs on a path that has already
/// failed, to explain it, and an explanation must not cost more than the
/// thing it explains.
#[cfg(unix)]
fn foreign_directory_under(spool: &Path, mine: u32) -> Option<(PathBuf, u32)> {
    use std::os::unix::fs::MetadataExt;

    let mut looked = 0usize;
    let mut todo = VecDeque::from([spool.to_path_buf()]);
    while let Some(dir) = todo.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            looked += 1;
            if looked > FOREIGN_SEARCH_LIMIT {
                return None;
            }
            let path = entry.path();
            let Ok(about) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !about.is_dir() {
                continue;
            }
            if about.uid() != mine {
                return Some((path, about.uid()));
            }
            todo.push_back(path);
        }
    }
    None
}

#[cfg(not(unix))]
fn foreign_directory_under(_spool: &Path, _mine: u32) -> Option<(PathBuf, u32)> {
    None
}

/// How many entries [`foreign_directory_under`] looks at before it gives up.
const FOREIGN_SEARCH_LIMIT: usize = 20_000;

/// What a position means for the machine it was just set on (E-33).
///
/// Said on the runner's own stderr, where its owner is looking, and it names
/// the job in flight because that is the thing she is most likely to be
/// worried about: `off` does not touch it.
fn what_that_position_does(switch: &herd_proto::Switch, running: usize) -> String {
    let in_flight = match running {
        0 => "nothing is running here".to_owned(),
        1 => "the 1 job running here finishes".to_owned(),
        many => format!("the {many} jobs running here finish"),
    };
    match switch {
        herd_proto::Switch::On => format!("this machine claims work again, and {in_flight}"),
        herd_proto::Switch::Off { .. } => format!(
            "nothing new is claimed and nothing is served to a peer (D11); {in_flight}, and \
             everything queued for this machine is claimable elsewhere at once"
        ),
        herd_proto::Switch::Drain { .. } => format!(
            "nothing new is claimed; {in_flight}, and this runner leaves once it holds nothing"
        ),
    }
}

/// An identity for this runner process, unique among the processes a hub can
/// be talking to at one time.
///
/// The process id plus the clock, which is exactly as much as the comparison
/// needs: two runners alive at once on one host have different pids, and a
/// runner restarted onto a pid the kernel has recycled started at a different
/// nanosecond. Not a random-number library, for the reason the redial
/// jitter is not one either — a dependency carried into the deployment for
/// the sake of an identity nothing is secured by.
fn mint_an_instance_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    format!("run-{}-{nanos:x}", std::process::id())
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Who this runner process is, as its handshake announces it.
    pub fn instance(&self) -> String {
        self.with(|inner| inner.instance.clone())
    }

    /// A session offering these capabilities, with these probe settings.
    pub fn offering(
        capabilities: herd_proto::CapabilityOffer,
        probes: capability::ProbeSettings,
    ) -> Self {
        Self {
            capacity: capability::Capacity::new(capabilities, probes),
            ..Self::default()
        }
    }

    /// What this runner offers and what its probes last said.
    pub fn capacity(&self) -> &capability::Capacity {
        &self.capacity
    }

    /// Tell the hub what every probe says, as one snapshot.
    ///
    /// Sent on every connection, before anything can be assigned on it: a hub
    /// that had the offer but not the probes would believe a suspended
    /// capability free for as long as it took the next change to happen,
    /// which could be hours. Nothing to say when nothing is probed.
    pub(crate) fn announce_capabilities(&self) {
        let capabilities = self.capacity.snapshot();
        if capabilities.is_empty() {
            return;
        }
        self.send(ToHub::CapabilitiesChanged { capabilities });
    }

    /// How many results are waiting to be acknowledged.
    pub fn results_held(&self) -> usize {
        self.with(|inner| inner.jobs.values().filter(|job| job.end.is_some()).count())
    }

    /// Whether every result this runner has produced is on the hub's disk.
    pub fn holds_nothing(&self) -> bool {
        self.results_held() == 0
    }

    /// Whether this runner is draining, and so refusing work.
    pub fn draining(&self) -> bool {
        self.with(|inner| inner.draining)
    }

    /// Where its owner's switch stands (E-33).
    pub fn switch(&self) -> herd_proto::Switch {
        self.with(|inner| inner.switch.clone())
    }

    /// Whether this runner may serve content to a peer right now (D11).
    ///
    /// **The one gate the registry M11 brings must consult**, and the reason
    /// this method exists before there is anything to serve: `off` is the
    /// owner saying the machine is hers again, and an `off` that stopped
    /// claiming jobs while still pushing packs up her link would have
    /// honoured half of E-33. There is no fetch endpoint in Herd yet (M11),
    /// so what is asserted today is the gate and the line the runner prints;
    /// the endpoint is built behind this and nowhere else.
    pub fn serves_content(&self) -> bool {
        self.with(|inner| !inner.draining && inner.switch.serves_content())
    }

    /// Move the switch, and say so if it moved.
    ///
    /// Returns the sentence an owner reads, or `None` when the position was
    /// already this one — which is what keeps a file polled once a second from
    /// printing a line a second.
    pub fn set_switch(&self, switch: herd_proto::Switch) -> Option<String> {
        self.with(|inner| {
            if inner.switch == switch {
                return None;
            }
            let said = format!(
                "the owner's switch is now {} (it was {}); {}",
                switch.sentence(),
                inner.switch.sentence(),
                what_that_position_does(
                    &switch,
                    inner.jobs.values().filter(|job| job.end.is_none()).count()
                )
            );
            inner.switch = switch;
            // Said to the hub **at once**, not at the next health report:
            // E-33's ten seconds is a promise about the hub stopping to hand
            // work to this machine, and a report fifteen seconds away would
            // spend all of it.
            let health = ToHub::Health {
                jobs: running_jobs(inner),
                draining: inner.draining,
                load_milli: crate::metrics::host_load_milli(),
                switch: inner.switch.clone(),
            };
            send_on(&inner.link, health);
            Some(said)
        })
    }

    /// Whether there is a hub to say anything to right now.
    ///
    /// The drain waits for what it holds to be acknowledged — but only while
    /// there is somebody who could acknowledge it. With no link at all the
    /// verdict is going to the data root whatever happens, and spending the
    /// owner's five seconds waiting for a hub that is not there is exactly
    /// what invariant 2 forbids.
    fn linked(&self) -> bool {
        self.with(|inner| inner.link.is_some())
    }

    /// How many jobs are running right now.
    pub fn jobs_running(&self) -> usize {
        self.with(|inner| inner.jobs.values().filter(|job| job.end.is_none()).count())
    }

    /// How many verdicts from a previous life are still waiting to be
    /// delivered. Zero on an ordinary start.
    pub fn spooled_results(&self) -> usize {
        self.with(|inner| inner.spooled.len())
    }

    /// Everything held of one job's output, as text. For a test that has to
    /// read what went into a job's record when there was no hub to send it
    /// to — which is exactly the case the record must still carry.
    #[cfg(test)]
    fn held_output_of(&self, job_id: &JobId, attempt: u32) -> String {
        self.with(|inner| {
            inner
                .jobs
                .get(&InHand::of(job_id, attempt))
                .map(|job| {
                    job.unacked
                        .iter()
                        .flat_map(|(_, _, data)| data.iter().copied())
                        .collect::<Vec<u8>>()
                })
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default()
        })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut inner)
    }

    /// Take a job in hand, and give back the token that ends it.
    ///
    /// `Err` when this runner will not run it, and the variant says which of
    /// the three reasons it was: the caller reports every one of them to the
    /// hub as an abandoned attempt, and the sentence it builds is the only
    /// thing that tells the hub's operator apart "she is gaming on the PC"
    /// from "this machine is full".
    ///
    /// **The reason comes back from inside the lock**, rather than being read
    /// off the session afterwards. Three refusals now share this path and two
    /// of them can change between the refusal and the question: a runner that
    /// refused because it was full and then drained would have told the hub
    /// it was draining, which is a different fact about a different machine
    /// state and would have been wrong on the record.
    pub(crate) fn take_job(
        &self,
        job_id: &JobId,
        attempt: u32,
        most_at_once: usize,
    ) -> Result<CancelToken, NotTaken> {
        self.with(|inner| {
            if inner.draining {
                return Err(NotTaken::Draining);
            }
            // And the owner's switch, in the same place and under the same
            // lock — which is the whole of the race E-33 can lose: a job the
            // hub sent in the instant before the switch moved arrives after
            // it, and a runner that checked only `draining` would start it.
            // The hub then gives the job another go somewhere else (E-21's
            // path, an abandoned attempt), which is exactly right: nothing is
            // lost and the machine is given back.
            if !inner.switch.claims() {
                return Err(NotTaken::Switched(inner.switch.clone()));
            }
            // And this machine's own ceiling, in the same place and under the
            // same lock, for the same reason: the count has to be read and the
            // entry written without an instant in between, or two assignments
            // arriving together would both find room for the last one.
            //
            // **Jobs that are still running, not jobs still on the books.** A
            // finished job whose verdict the hub has not acknowledged holds a
            // record and some unsent bytes; it holds no process and no memory
            // ceiling, and counting it would let a slow hub shut a working
            // machine down.
            let running = inner.jobs.values().filter(|job| job.end.is_none()).count();
            if running >= most_at_once {
                return Err(NotTaken::AtItsCeiling {
                    running,
                    most: most_at_once,
                });
            }
            let cancel = CancelToken::new();
            inner.jobs.insert(
                InHand::of(job_id, attempt),
                JobInHand {
                    next_seq: 0,
                    unacked: VecDeque::new(),
                    unacked_bytes: 0,
                    end: None,
                    cancel: cancel.clone(),
                    trimmed: 0,
                },
            );
            Ok(cancel)
        })
    }

    /// Send one log chunk, numbering it and remembering it inside the lock
    /// that enqueues it.
    ///
    /// The three steps are one on purpose. Two pumps — stdout and stderr —
    /// take their numbers from one counter, and if a chunk could be numbered
    /// before another and enqueued after it, the hub would see `seq` go
    /// backwards and read a reordering as output that never arrived.
    /// Numbering here makes the order on the wire the order of the numbers, so
    /// a jump in `seq` means one thing only: chunks that are gone, which is
    /// what the hub's marker names (D29).
    pub(crate) fn send_chunk(
        &self,
        job_id: &JobId,
        attempt: u32,
        stream: LogStream,
        data: Vec<u8>,
    ) {
        self.with(|inner| {
            let Some(job) = inner.jobs.get_mut(&InHand::of(job_id, attempt)) else {
                return;
            };
            let seq = job.next_seq;
            job.next_seq += 1;
            job.remember(seq, stream, &data);
            send_on(
                &inner.link,
                ToHub::LogChunk {
                    job_id: job_id.clone(),
                    attempt,
                    stream,
                    seq,
                    data,
                },
            );
        });
    }

    /// Send something about a job on whichever link is up, and say whether
    /// there was one.
    pub(crate) fn send(&self, message: ToHub) -> bool {
        self.with(|inner| send_on(&inner.link, message))
    }

    /// Hold a job's end and report it (E-44).
    ///
    /// `steps` is every executor step this attempt ran, in order — the
    /// authoritative list, which the hub takes over the lines it already has
    /// from [`ToHub::StepFinished`] (D77). Empty for a job that is not a
    /// declared command: it ran no executor step, and an empty list says
    /// exactly that.
    ///
    /// `outputs` is what the job left under `{out}`, already capped by the
    /// runner's own figure: it is held against the acknowledgement exactly
    /// like the verdict, because a file that only exists in a workspace that
    /// has been reclaimed is a file nothing can produce again (E-44).
    /// `spool` is where the bytes are when the collection was too big to ride
    /// the verdict (D148). The verdict goes first and the bytes follow it:
    /// D20 says a side channel never delays a verdict, invariant 2 says
    /// stopping never waits for bytes, and a runner that dies mid-stream has
    /// then already delivered the verdict, so the job is readable and only
    /// its evidence is pending.
    // Eight, because every one of them is a different half of one record and
    // there is no grouping of them that a reader would not have to unpick
    // again. Same reason as the three in `job.rs`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish(
        &self,
        job_id: &JobId,
        attempt: u32,
        outcome: JobOutcome,
        metrics: JobMetrics,
        steps: Vec<StepRun>,
        outputs: JobOutputs,
        spool: Option<PathBuf>,
    ) {
        let streaming = self.with(|inner| {
            let Some(job) = inner.jobs.get_mut(&InHand::of(job_id, attempt)) else {
                return false;
            };
            let end = Ended {
                outcome,
                metrics,
                steps,
                outputs,
            };
            job.end = Some(end.clone());
            let chunks_sent = job.next_seq;
            if !send_on(
                &inner.link,
                finished_frame(job_id, attempt, chunks_sent, &end),
            ) {
                eprintln!(
                    "herd-runner: {job_id} ended while there was no hub to tell; \
                     its result is held until a hub acknowledges it"
                );
            }
            // The manifest has been announced, so the bytes may follow. Taken
            // in hand here, under the same lock that wrote the verdict, so
            // there is no instant in which a collection is promised and not
            // owed.
            match spool {
                Some(spool) if end.outputs.streamed => {
                    inner.collections.insert(
                        job_id.clone(),
                        FilesOut::new(attempt, spool, end.outputs.files.clone()),
                    );
                    true
                }
                _ => false,
            }
        });
        if streaming {
            self.pump_files(job_id.clone());
        }
    }

    /// Send a collection's bytes, a chunk at a time, until they are all on the
    /// link or the link goes away.
    ///
    /// One task per collection per connection. It holds nothing: every
    /// decision about what to send next is the session's
    /// ([`FilesOut::next`]), so a task that is replaced on a redial cannot
    /// disagree with the one that replaced it, and the file read happens
    /// outside the session's lock.
    ///
    /// **A permit is spent only by a chunk that reaches the link** (D158).
    /// The permit is held, not forgotten, until [`Session::send`] has queued
    /// the frame [`write_frames`] will hand it back for; every other way out
    /// of the loop — the collection is finished, a file will not read, the
    /// link went away — drops it, and dropping returns it. Forgetting it on
    /// acquisition cost the window one permit per **completed** collection,
    /// so the eighth collection on one connection left
    /// [`Config::files_in_flight`] at zero and the ninth waited for room
    /// that could never come: output that worked all evening and then
    /// stopped, with nothing said by anybody.
    fn pump_files(&self, job_id: JobId) {
        let session = self.clone();
        tokio::spawn(async move {
            let mut already_said = false;
            loop {
                let Some((credit, stall_after)) = session
                    .with(|inner| Some((inner.files_credit.clone()?, inner.files_stall_after)))
                else {
                    // No link: the collection stays on the disk and the hub
                    // says where to resume when there is one again.
                    return;
                };
                // The window, and the one thing that stops a fast disk
                // filling memory on its way to a slower socket. A permit
                // comes back when `write_frames` has put a chunk on the wire.
                //
                // **A window that will not open is said out loud, once per
                // collection** (D158). Waiting here is ordinary for a moment
                // and is a stopped transfer for longer than that, and a
                // stopped transfer is precisely the state that reached the
                // first consumer as a job whose verdict said `ok` and whose
                // files never came. This runner is the only party that knows
                // *why* nothing is moving — the hub sees an absence and the
                // requester sees a short collection — so it is the one that
                // has to speak, and it speaks inside `herdctl`'s own
                // `OUTPUTS_STALL_LIMIT` so the line is already in the journal
                // when the requester gives up.
                let permit = match tokio::time::timeout(stall_after, credit.acquire()).await {
                    Ok(Ok(permit)) => permit,
                    Ok(Err(_)) => return,
                    Err(_) => {
                        if !already_said {
                            already_said = true;
                            journal(&format!(
                                "herd-runner: {}",
                                waiting_for_room(&job_id, stall_after)
                            ));
                        }
                        match credit.acquire().await {
                            Ok(permit) => permit,
                            Err(_) => return,
                        }
                    }
                };
                let Some(plan) = session
                    .with(|inner| inner.collections.get_mut(&job_id).and_then(FilesOut::next))
                else {
                    return;
                };
                match read_at(&plan.file, plan.offset, plan.take) {
                    Ok(data) => {
                        if !session.send(ToHub::JobFileChunk {
                            job_id: job_id.clone(),
                            attempt: plan.attempt,
                            path: plan.path,
                            offset: plan.offset,
                            data,
                        }) {
                            return;
                        }
                        // On the link's queue, so `write_frames` owes this
                        // permit back and this task must not return it too.
                        permit.forget();
                    }
                    // The spool is this runner's own directory and nothing
                    // but a reap should ever take a file out of it, so this
                    // is a disk fault or an operator. Either way the hub
                    // cannot be given what is not there: say so once, by
                    // name, and stop — the record's manifest still names the
                    // file, so a requester asking for it is told it never
                    // arrived rather than handed a short collection (D148).
                    Err(error) => {
                        eprintln!(
                            "herd-runner: {job_id} cannot send {}: {error}. Its record names \
                             the file and this runner no longer has it.",
                            plan.file.display()
                        );
                        return;
                    }
                }
            }
        });
    }

    /// The hub has said what it holds of a collection: resume from there.
    fn files_held(&self, job_id: &JobId, attempt: u32, held: &[herd_proto::FileHeld]) {
        let resuming = self.with(|inner| match inner.collections.get_mut(job_id) {
            Some(files) if files.attempt == attempt => {
                files.resume_from(held);
                true
            }
            // A collection this runner does not hold, or holds for another
            // attempt. Not an error and not silent: the hub is waiting for
            // bytes nobody has, which is exactly the state D148 says must be
            // visible rather than a job that never completes.
            other => {
                eprintln!(
                    "herd-runner: the hub is waiting for the files of {job_id} attempt \
                     {attempt}, and this runner holds {}",
                    match other {
                        Some(files) => format!("attempt {} of it", files.attempt),
                        None => "no collection for it".to_owned(),
                    }
                );
                false
            }
        });
        if resuming {
            self.pump_files(job_id.clone());
        }
    }

    /// The hub holds the whole collection: the spool may go.
    fn files_acknowledged(&self, job_id: &JobId, attempt: u32) {
        let spool = self.with(|inner| match inner.collections.get(job_id) {
            Some(files) if files.attempt == attempt => {
                inner.collections.remove(job_id).map(|files| files.spool)
            }
            _ => None,
        });
        // Only now, after the hub has said every file is on its disk: a spool
        // removed before the acknowledgement would be the one copy of a
        // result destroyed by the mechanism that exists to keep it (D19).
        if let Some(spool) = spool {
            if let Err(error) = std::fs::remove_dir_all(&spool) {
                eprintln!(
                    "herd-runner: the hub has every file of {job_id} and {} will not go \
                     away: {error}.{} The bytes are safe on the hub; what is left here is \
                     this runner's own copy, and it will not shrink until somebody who can \
                     write there removes it.",
                    spool.display(),
                    match why_it_will_not_go(&spool) {
                        Some(why) => format!(" {why}."),
                        None => String::new(),
                    }
                );
            }
        }
    }

    /// Give an attempt back without a verdict, and let the job go.
    ///
    /// Nothing is held afterwards: the runner is not keeping a result, it is
    /// saying there is none. The hub decides what becomes of the job.
    pub(crate) fn abandon(&self, job_id: &JobId, attempt: u32, reason: &str) {
        self.with(|inner| {
            // **This attempt's entry, and only this attempt's.** Giving one
            // attempt back must not take another attempt of the same job off
            // this machine's count: that is a place the machine does not
            // have, and the next assignment would be accepted onto a host
            // already as full as it said it would ever be — invariant 2
            // broken by the bookkeeping rather than by a job.
            inner.jobs.remove(&InHand::of(job_id, attempt));
            send_on(
                &inner.link,
                ToHub::AttemptAbandoned {
                    job_id: job_id.clone(),
                    attempt,
                    reason: reason.to_owned(),
                },
            );
        });
    }

    /// The hub has written a job's verdict down ([`FromHub::JobResultAck`]):
    /// let go of the attempt whose verdict it was.
    ///
    /// **Which attempt, when the frame names none.** The acknowledgement
    /// carries a job id and nothing else, and the attempt it is about is the
    /// one *holding an end* — which is this function's own sentence read
    /// literally: the hub has **this job's end** on its disk, and an attempt
    /// that is still running has no end for an acknowledgement to be about.
    /// Before the attempt was part of the key, the entry under that id was
    /// taken out whatever it was, and once a second attempt of the same job
    /// had been assigned here that was the attempt that was *running*: it
    /// then ran with nothing counting it, nothing naming it in a health
    /// report, and a hub that wrote it off for silence while the process was
    /// working perfectly.
    fn result_acknowledged(&self, job_id: &JobId) {
        let holding_an_end: Vec<InHand> = self.with(|inner| {
            inner
                .jobs
                .iter()
                .filter(|(which, job)| &which.job_id == job_id && job.end.is_some())
                .map(|(which, _)| which.clone())
                .collect()
        });
        if holding_an_end.is_empty() {
            // Nothing in hand: a verdict acknowledged twice, or one this
            // runner let go of when its attempt was written off. Not an
            // error — and the spool is still swept, because a verdict a
            // previous life of this runner wrote is held nowhere else and
            // this is the acknowledgement that frees its file (D34).
            self.release(job_id, None);
            return;
        }
        for which in holding_an_end {
            self.release(job_id, Some(&which));
        }
    }

    /// The hub has a job's end on its disk: let the attempt that was holding
    /// it go, output and all, and remove the spool file if the verdict came
    /// from one.
    ///
    /// `entry` is that attempt, and `None` is a job this runner holds nothing
    /// of any more — the spool is swept either way, because that is where a
    /// verdict written by a previous life of this runner lives (D34).
    fn release(&self, job_id: &JobId, entry: Option<&InHand>) {
        let (delivered, trimmed): (Vec<Spooled>, u64) = self.with(|inner| {
            let trimmed = entry
                .and_then(|which| inner.jobs.remove(which))
                .map_or(0, |job| job.trimmed);
            (
                take_spooled(inner, |spooled| spooled.job_id() == job_id),
                trimmed,
            )
        });
        // The one line the per-chunk announcements were replaced by: how much
        // of this job's output the held-bytes cap dropped, said once (D151).
        if trimmed > 0 {
            eprintln!(
                "herd-runner: {trimmed} chunks of {job_id} were dropped unacknowledged: the \
                 hub was not taking them fast enough to stay inside the \
                 {HELD_BYTES_PER_JOB} bytes this runner holds per job"
            );
        }
        // Only now, after the hub has said the verdict is on its disk: a file
        // removed before the acknowledgement would be a verdict destroyed by
        // the one mechanism that exists to keep it (D19, D34).
        for spooled in delivered {
            for line in spooled.delivered() {
                eprintln!("herd-runner: {line}");
            }
        }
    }

    /// The hub has this job's output up to `through_seq`.
    fn log_acknowledged(&self, job_id: &JobId, attempt: u32, through_seq: u64) {
        self.with(|inner| {
            if let Some(job) = inner.jobs.get_mut(&InHand::of(job_id, attempt)) {
                job.forget_through(through_seq);
            }
        });
    }

    /// Send again what is held of a job's output from `from_seq` on, and then
    /// its result if one is held.
    ///
    /// The result comes after the output on purpose: the hub asks for the
    /// output *because* it would not record an end on a log it knows to be
    /// short, so the end has to arrive after what was missing.
    fn resend_log(&self, job_id: &JobId, attempt: u32, from_seq: u64) {
        self.with(|inner| {
            let Some(job) = inner.jobs.get(&InHand::of(job_id, attempt)) else {
                eprintln!(
                    "herd-runner: the hub asked for output of {job_id} from {from_seq}, and \
                     this runner holds nothing of that job any more"
                );
                // A spooled verdict is exactly that case: the process that
                // produced the output is two lives ago, and its chunks were
                // never written down — only the verdict was (D34). The hub
                // asks once and records the end on the second telling,
                // naming what is missing (D36); so the end is told again,
                // rather than both sides waiting for the other for ever.
                let again: Vec<ToHub> = inner
                    .spooled
                    .iter()
                    .filter(|spooled| spooled.job_id() == job_id && spooled.attempt() == attempt)
                    .map(|spooled| spooled.frame().clone())
                    .collect();
                for frame in again {
                    eprintln!(
                        "herd-runner: the output of {job_id} from {from_seq} is gone with the \
                         runner that produced it; its verdict is offered again so the hub \
                         can record the end and name what is missing"
                    );
                    send_on(&inner.link, frame);
                }
                return;
            };
            let held: Vec<(u64, LogStream, Vec<u8>)> = job
                .unacked
                .iter()
                .filter(|(seq, _, _)| *seq >= from_seq)
                .cloned()
                .collect();
            let (end, chunks_sent) = (job.end.clone(), job.next_seq);
            for (seq, stream, data) in held {
                send_on(
                    &inner.link,
                    ToHub::LogChunk {
                        job_id: job_id.clone(),
                        attempt,
                        stream,
                        seq,
                        data,
                    },
                );
            }
            if let Some(end) = end {
                send_on(
                    &inner.link,
                    finished_frame(job_id, attempt, chunks_sent, &end),
                );
            }
        });
    }

    /// Ask a job to end, if this runner has it at that attempt.
    /// `attempt` is not optional, and could not be: a cancellation is about
    /// one attempt of one job, and the entry it is about is the one filed
    /// under both ([`InHand`]). Before the attempt was part of the key this
    /// took an `Option<u32>` and compared the number it found — the same
    /// question, asked in a place where the answer could be wrong.
    fn end_job(&self, job_id: &JobId, attempt: u32, ending: Ending) -> bool {
        self.with(|inner| {
            let Some(job) = inner.jobs.get(&InHand::of(job_id, attempt)) else {
                return false;
            };
            job.cancel.end(ending)
        })
    }

    /// Nothing of an attempt the hub has written off is kept or finished
    /// (E-21): the job stops, and what it produced is dropped rather than
    /// delivered into the record of an attempt it is not part of.
    ///
    /// A verdict spooled for that attempt is exactly the same case — a runner
    /// coming back with the result of an attempt the hub gave up on (D39) —
    /// so its file goes too, by name, rather than being offered for ever.
    ///
    /// Returns whether this call was the one that announced the attempt. The
    /// caller in `serve_forever` has no use for it; it is how a test counts
    /// the lines a flood of refusals produces without reading the journal,
    /// and counting is the assertion that matters — a test that only checked
    /// the wording would pass again the day the loop comes back saying
    /// something else (D151).
    fn drop_stale(&self, job_id: &JobId, attempt: u32, reason: &str) -> bool {
        let (running, say_it) = self.with(|inner| {
            // Taken whether or not the job is still here: the repeats this
            // silences arrive *after* the first refusal released it.
            let say_it = !inner
                .said_stale
                .iter()
                .any(|(said, at)| said == job_id && *at == attempt);
            if say_it {
                if inner.said_stale.len() == REFUSALS_REMEMBERED {
                    inner.said_stale.pop_front();
                }
                inner.said_stale.push_back((job_id.clone(), attempt));
            }
            let Some(job) = inner.jobs.get(&InHand::of(job_id, attempt)) else {
                return (false, say_it);
            };
            job.cancel
                .end(Ending::Abandoned(format!("stale attempt: {reason}")));
            (job.end.is_none(), say_it)
        });
        // Once per attempt, not once per frame the hub refused: the hub
        // answers every buffered chunk of a dead attempt, and a runner that
        // echoed each answer would write a line per chunk onto a host whose
        // IO is already the reason the attempt died (D151).
        if say_it {
            journal(&format!(
                "herd-runner: attempt {attempt} of {job_id} is over as far as the hub is \
                 concerned ({reason}); nothing of it is sent again"
            ));
        }
        self.spool_refused(job_id, attempt, reason);
        // A job still running is left to notice the ending and clean up after
        // itself; one that had already finished is dropped here and now,
        // result included — it is not this job's attempt any more.
        if !running {
            self.release(job_id, Some(&InHand::of(job_id, attempt)));
        }
        say_it
    }

    /// A spooled verdict the hub will never record: the attempt is over, so
    /// the file is removed and what it held is said out loud by name.
    fn spool_refused(&self, job_id: &JobId, attempt: u32, reason: &str) {
        let refused: Vec<Spooled> = self.with(|inner| {
            take_spooled(inner, |spooled| {
                spooled.job_id() == job_id && spooled.attempt() == attempt
            })
        });
        for spooled in refused {
            for line in spooled.refused(reason) {
                eprintln!("herd-runner: {line}");
            }
        }
    }

    /// Note that the hub has welcomed this connection (D104).
    ///
    /// The one fact `serve_forever` cannot work out for itself: whether a
    /// connection that has just ended had ever been *serving*. A dial that
    /// hung for a minute and a link that served for a minute take the same
    /// wall-clock time, and only one of them says the hub is up.
    fn welcomed(&self) {
        self.with(|inner| inner.welcomed_at = Some(Instant::now()));
    }

    /// How long this session has been welcomed, if that welcome arrived no
    /// earlier than `began` — the moment the attempt being judged started.
    ///
    /// The `began` bound is what keeps a stale welcome from excusing a hub
    /// that has since gone: without it, a runner that served for an hour and
    /// then found the hub gone for ever would read its own old welcome as
    /// proof that every failed dial since had been a connection.
    fn served_since(&self, began: Instant) -> Option<Duration> {
        self.with(|inner| {
            inner
                .welcomed_at
                .filter(|at| *at >= began)
                .map(|at| at.elapsed())
        })
    }

    /// Everything the hub is told at the start of a connection, in order:
    /// output it may be missing, then the results it has not acknowledged,
    /// then the verdicts a previous life of this runner left on the disk.
    ///
    /// Done while holding the lock a new chunk would need, so that nothing a
    /// running job produces can slip in front of what is being sent again. A
    /// log that keeps its order across a reconnect is the whole point.
    fn attach(&self, outbox: Outbox, credit: Arc<Semaphore>, stall_after: Duration) {
        self.with(|inner| {
            inner.link = Some(outbox);
            inner.files_credit = Some(credit);
            inner.files_stall_after = stall_after;
            let held: Vec<InHand> = inner.jobs.keys().cloned().collect();
            for which in held {
                let Some(job) = inner.jobs.get(&which) else {
                    continue;
                };
                let (job_id, attempt) = (which.job_id.clone(), which.attempt);
                let chunks_sent = job.next_seq;
                let end = job.end.clone();
                let held: Vec<(u64, LogStream, Vec<u8>)> = job.unacked.iter().cloned().collect();
                for (seq, stream, data) in held {
                    send_on(
                        &inner.link,
                        ToHub::LogChunk {
                            job_id: job_id.clone(),
                            attempt,
                            stream,
                            seq,
                            data,
                        },
                    );
                }
                if let Some(end) = end {
                    send_on(
                        &inner.link,
                        finished_frame(&job_id, attempt, chunks_sent, &end),
                    );
                }
            }
            // The spool last: these verdicts are older than anything running
            // and nothing about them is urgent. What matters is only that
            // every link offers them until one of them is acknowledged (D34).
            let spooled: Vec<ToHub> = inner
                .spooled
                .iter()
                .map(|spooled| spooled.frame().clone())
                .collect();
            for frame in spooled {
                send_on(&inner.link, frame);
            }
        });
    }

    fn detach(&self) {
        self.with(|inner| {
            inner.link = None;
            // The window goes with the link. Closing it is what ends the
            // pumps: a task waiting on a permit wakes with an error rather
            // than waiting for a connection that is gone, and the next
            // connection's `JobFilesHeld` starts them again from wherever
            // the hub really got to.
            if let Some(credit) = inner.files_credit.take() {
                credit.close();
            }
        });
    }

    /// What this runner is doing, as a health report says it (E-40).
    fn health(&self) -> ToHub {
        self.with(|inner| ToHub::Health {
            jobs: running_jobs(inner),
            draining: inner.draining,
            switch: inner.switch.clone(),
            // Measured now, on every report, because it is a fact about the
            // moment (E-32). The hub records it and gates nothing on it.
            load_milli: crate::metrics::host_load_milli(),
        })
    }

    /// Refuse new work from now on, and tell every job it is over.
    ///
    /// Returns the jobs that were running, which the caller waits for —
    /// briefly, because the owner asked for the machine back and gets it
    /// (invariant 2).
    fn start_draining(&self, reason: &str) -> Vec<JobId> {
        self.with(|inner| {
            inner.draining = true;
            let mut ending = Vec::new();
            for (which, job) in inner.jobs.iter() {
                if job.end.is_none() {
                    job.cancel.end(Ending::Abandoned(reason.to_owned()));
                    ending.push(which.job_id.clone());
                }
            }
            // So that the hub stops handing out work to a runner on its way
            // out at once, rather than at the next report.
            let health = ToHub::Health {
                jobs: running_jobs(inner),
                draining: true,
                load_milli: crate::metrics::host_load_milli(),
                switch: inner.switch.clone(),
            };
            send_on(&inner.link, health);
            ending
        })
    }

    /// Pick up the verdicts a previous life of this runner could not deliver,
    /// so that the first link this one makes carries them (D34).
    ///
    /// Nothing is deleted here and nothing is assumed: each file is read and
    /// decoded, and what it holds is offered on every connection until the
    /// hub either acknowledges it or says the attempt is over. A file that
    /// cannot be decoded at all is set aside under a name that says so rather
    /// than removed — it may be the only trace of a verdict left anywhere.
    pub fn pick_up(&self, root: &root::DataRoot) -> Vec<String> {
        let (spooled, said) = spool::pick_up(root);
        self.with(|inner| inner.spooled = spooled);
        said
    }

    /// Settle, against the state root the hub has just announced, what this
    /// data root is holding — before a single frame of it is offered.
    ///
    /// A job id means something only inside one hub state. A hub whose state
    /// root is recreated hands out `j0000000001` again, and a verdict this
    /// runner spooled for the `j0000000001` of the root that is gone is then
    /// *plausible and false*: it names a job the hub has genuinely minted, for
    /// an attempt it has genuinely handed out. Offering it would put one job's
    /// exit code and one job's metrics onto another job's record, and nothing
    /// in the frame says otherwise. So the state root travels in the handshake
    /// and this is where it is used:
    ///
    /// - the recorded root and the hub's agree: the ordinary case, nothing to
    ///   do;
    /// - they differ: every spooled verdict belongs to the root that is gone.
    ///   None of it is offered, each file is set aside by name, and the new
    ///   root is written down;
    /// - nothing is recorded yet and there is a verdict in the spool: it came
    ///   from a hub this data root cannot vouch for — a planted file, a root
    ///   copied from somewhere — and the same rule applies, because
    ///   "unattributable" and "another hub's" are the same thing to a record.
    ///
    /// And one case that is **not** a refusal to serve: the new root cannot be
    /// written down at all — a read-only data root, a full disk. The runner
    /// says so loudly and **carries on**: it is admitted, it runs jobs, and it
    /// is useful. Nothing is risked by that, because a data root with no
    /// recorded root announces `None` on its next dial, `None` never equals a
    /// hub's root, and so whatever it is holding is set aside by the rule
    /// above instead of being believed. What is lost is only the ability to
    /// attribute a verdict spooled from here, which is why the line is loud.
    ///
    /// What is held in *memory* goes the same way, and for the same reason: a
    /// verdict this runner produced for the hub that is gone names a job id
    /// the new hub hands out to other jobs. It is never offered, its bytes are
    /// kept under the data root, and a job still running for that vanished
    /// attempt is ended — nothing it produces can ever be recorded anywhere,
    /// and the host is not spent on work nobody will collect (invariant 2).
    ///
    /// Every line it returns is meant to be loud, and a verdict dealt with
    /// here is dealt with **by name**: the job, the attempt, the outcome and
    /// where its bytes went.
    pub fn settle_against_state_root(&self, root: &root::DataRoot, hub_root: &str) -> Vec<String> {
        let recorded = root.hub_state_root();
        let mine = recorded.as_deref();
        if mine == Some(hub_root) {
            return Vec::new();
        }
        let mut said = Vec::new();
        let why = match mine {
            Some(theirs) => format!(
                "this data root belongs to the hub state root {theirs} and the hub has \
                 welcomed it to {hub_root}"
            ),
            None => format!(
                "this data root has never been welcomed by a hub and so belongs to no \
                 recorded state root, while the hub serves {hub_root}"
            ),
        };
        let foreign: Vec<Spooled> = self.with(|inner| take_spooled(inner, |_| true));
        if !foreign.is_empty() {
            said.push(format!(
                "{} spooled verdict(s) here cannot belong to this hub's state: {why}",
                foreign.len()
            ));
            for spooled in foreign {
                said.extend(spooled.for_a_foreign_state_root(&why));
            }
        }
        let (in_hand, running) = self.with(|inner| {
            let mut in_hand = Vec::new();
            let mut running = Vec::new();
            let mut acknowledged: Vec<InHand> = Vec::new();
            for (which, job) in inner.jobs.iter() {
                match &job.end {
                    Some(end) => {
                        in_hand.push((
                            which.job_id.clone(),
                            end.outcome.clone(),
                            finished_frame(&which.job_id, which.attempt, job.next_seq, end),
                        ));
                        acknowledged.push(which.clone());
                    }
                    None => {
                        job.cancel.end(Ending::Abandoned(why.clone()));
                        running.push(which.job_id.clone());
                    }
                }
            }
            for which in &acknowledged {
                inner.jobs.remove(which);
            }
            (in_hand, running)
        });
        if !running.is_empty() {
            said.push(format!(
                "{} job(s) still running here belong to attempts of a hub state that is \
                 gone ({why}); they are ended, because nothing they produce can be \
                 recorded anywhere: {}",
                running.len(),
                running
                    .iter()
                    .map(|job_id| job_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        said.extend(spool::write_all_for_a_foreign_state_root(
            root, in_hand, &why,
        ));
        if let Err(error) = root.remember_hub_state_root(hub_root) {
            said.push(format!(
                "COULD NOT write down which hub state root this data root belongs to: \
                 {error}. A verdict spooled here would not be attributable to {hub_root}, \
                 so the next runner on this data root will set it aside instead of \
                 delivering it. This runner goes on serving the hub: fix the data root"
            ));
        } else if recorded.is_some() {
            said.push(format!(
                "this data root now belongs to the hub state root {hub_root}: {why}"
            ));
        }
        said
    }

    /// Write every result the hub has not acknowledged into the data root,
    /// and say where each one went.
    ///
    /// Called when the runner is leaving and a held result would otherwise go
    /// with it. What is written is the `job_finished` frame itself, verbatim
    /// — the protocol is the contract (invariant 4), so the file needs no
    /// shape of its own and the next start replays it as it stands.
    ///
    /// Every line it returns is meant to be loud: this is a verdict that did
    /// not reach the hub, and the one thing that must never happen to it is
    /// for it to disappear quietly.
    pub fn spool(&self, root: &root::DataRoot) -> Vec<String> {
        let held: Vec<(JobId, JobOutcome, ToHub)> = self.with(|inner| {
            inner
                .jobs
                .iter()
                .filter_map(|(which, job)| {
                    job.end.as_ref().map(|end| {
                        (
                            which.job_id.clone(),
                            end.outcome.clone(),
                            finished_frame(&which.job_id, which.attempt, job.next_seq, end),
                        )
                    })
                })
                .collect()
        });
        spool::write_all(root, held)
    }
}

/// Take out of the spool every entry a predicate names, leaving the rest.
fn take_spooled(inner: &mut Inner, mut wanted: impl FnMut(&Spooled) -> bool) -> Vec<Spooled> {
    let mut taken = Vec::new();
    let mut kept = Vec::new();
    for spooled in inner.spooled.drain(..) {
        if wanted(&spooled) {
            taken.push(spooled);
        } else {
            kept.push(spooled);
        }
    }
    inner.spooled = kept;
    taken
}

/// The running jobs of a session, as a health report names them.
fn running_jobs(inner: &Inner) -> Vec<RunningJob> {
    inner
        .jobs
        .iter()
        .filter(|(_, job)| job.end.is_none())
        .map(|(which, job)| RunningJob {
            job_id: which.job_id.clone(),
            attempt: which.attempt,
            chunks_sent: job.next_seq,
        })
        .collect()
}

/// How many file chunks may be queued on a link at once: **8**, so 8 MiB.
///
/// Small enough that a 400 MiB collection never puts more than that in
/// memory on its way to the socket, and large enough that the pump is not
/// waiting on a round trip for every chunk — the permits come back from
/// [`write_frames`], which is on the same machine, so the window only has to
/// cover the disk-to-socket speed difference and not a network latency.
///
/// Public because it is a figure a test has to be able to exceed: the way a
/// window is got wrong is that it runs down over several collections, and a
/// test that does not know how many collections "several" is cannot write
/// that down (D158).
pub const FILES_IN_FLIGHT: usize = 8;

/// How long a collection's pump waits for room on the link before it says so:
/// **fifteen seconds** (D158).
///
/// Under `herdctl`'s own thirty-second stall bound, deliberately: the
/// requester gives up at thirty and the only party that knows *why* the bytes
/// stopped is this one, so its line has to already be in the journal when the
/// requester's appears. Long enough that an ordinary full window — a slow
/// socket behind a fast disk — never reaches it, because a line printed every
/// time a transfer is merely busy is a line nobody reads (D151).
const FILES_STALL_AFTER: Duration = Duration::from_secs(15);

/// `take` bytes of `path` starting at `offset`, and no more held than that.
///
/// The read a streamed chunk is, kept out of the session's lock and out of
/// any buffer that grows with the file: a 400 MiB collection is read 1 MiB at
/// a time, which is the whole reason the streamed carriage is cheaper than
/// the frame it replaced.
fn read_at(path: &Path, offset: u64, take: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))?;
    }
    let mut data = vec![0u8; take];
    let mut filled = 0usize;
    while filled < take {
        let read = file.read(&mut data[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    data.truncate(filled);
    Ok(data)
}

fn send_on(link: &Option<Outbox>, message: ToHub) -> bool {
    match link {
        Some(outbox) => outbox.send(message).is_ok(),
        None => false,
    }
}

/// Join the hub and keep serving it, until the owner asks this runner to stop.
///
/// Confinement is checked **before** the connection is dialled: a runner that
/// cannot confine a job must not be a runner a hub can dispatch to, and the
/// refusal must name what is wrong with the machine rather than arrive as a
/// failed job later.
///
/// A dropped connection is not the end of the runner (D17): a job that is
/// running keeps running, the runner dials again on a growing and jittered
/// delay, and everything the hub has not acknowledged is sent again on the
/// new connection. Two things do end this: a *refusal*, because the hub said
/// no and dialling again will not change its mind, and a bound on how long a
/// hub may be unreachable before a human is needed.
pub async fn run(config: Config) -> Result<()> {
    run_until(config, owner_asks_to_stop()).await
}

/// The same, with the owner's ask given as a future, so a test can make it
/// happen without a signal.
///
/// Whatever `stop` resolves to is the sentence that goes on the record of the
/// job this runner was running: the one thing a drain may not do is lose a
/// job silently.
pub async fn run_until(config: Config, stop: impl Future<Output = String>) -> Result<()> {
    // The same mistake the hub refuses on its own side, in the one direction a
    // runner can make it: believing a silent hub for less time than it takes
    // to send a few reports. The hub answers every report, so a silence limit
    // shorter than that is a runner that redials for ever on a link that is
    // perfectly alive — and jobs that keep losing the attempt they are on.
    if let Some(reason) = herd_proto::silence_threshold_refusal(
        "this runner's limit on the hub's silence (--hub-silence-limit)",
        config.hub_silence_limit,
        "its own health cadence (--health-every)",
        config.health_every,
    ) {
        bail!("{reason}");
    }
    let confined = Arc::new(
        Confined::prepare(config.confinement.clone())
            .map_err(|reason| anyhow!("this runner cannot confine a job: {reason}"))?,
    );
    // Said once, at startup, on the runner's own stderr: what it will enforce
    // and — loudly — what it has had to fall back on.
    for line in confined.report() {
        eprintln!("herd-runner: {line}");
    }

    let session = Session::offering(config.capabilities.clone(), config.probes);
    // What this runner offers, said once at startup, with each capability's
    // origin (E-24) — the `kvm: present, denied` line above all, because a
    // denial an operator cannot see is a denial they cannot trust.
    for line in capability::report(session.capacity()) {
        eprintln!("herd-runner: {line}");
    }
    // Before the first dial: whatever a previous life of this runner wrote
    // down and nobody acknowledged. A verdict on a disk is only saved once it
    // reaches the hub, and no human belongs in that loop (D34).
    for line in session.pick_up(confined.root()) {
        eprintln!("herd-runner: {line}");
    }
    let probing: tokio::task::JoinHandle<()>;
    {
        // The probes are read beside the redial loop rather than inside a
        // connection: a lock file an operator removes while the hub is down
        // must still free the capability, and the next connection announces
        // it.
        //
        // And they are read *beside the join* too, not in front of it. The
        // first sweep used to be awaited here, which made joining cost as
        // long as reading every probe took — ten probes that hung held the
        // first dial for twenty seconds, during which the runner was no use
        // to anybody. Nothing is lost by joining first: a probed capability
        // with no reading yet is suspended at both ends (D49), so the worst
        // case is a job that waits the moment it would have waited anyway,
        // and the readings arrive on this same connection as a snapshot.
        let watching = session.clone();
        probing = tokio::spawn(capability::watch(
            session.capacity().clone(),
            move |capabilities| {
                watching.send(ToHub::CapabilitiesChanged { capabilities });
            },
        ));
    }

    // Stopping the probes is part of stopping the runner, not an afterthought
    // of process exit. A probe in flight is a process group of this runner's
    // making, and dropping the task that is reading it is what kills that
    // group (`ProbeGroup`): a stop that left the task alone exited in a
    // fifth of a second and left the probe's children running, reparented to
    // init — the orphan-strewing invariant 2 forbids, and the one case the
    // timeout path did not cover.
    // The owner's switch, read beside the redial loop for the probes' reason:
    // a machine withdrawn while the hub is down is still withdrawn, and the
    // next connection announces the position in its handshake (E-33).
    //
    // Read **once, here, before the first dial**, so that a runner restarted
    // on a machine its owner switched off never claims anything at all. The
    // watcher then takes over.
    //
    // A persisted `drain` is read as `off` here and nowhere else (D164): the
    // word was said to a process that is gone, and a process holding nothing
    // has already finished and left. See `switch::at_startup` for the whole
    // of why.
    let mut a_stale_drain: Option<herd_proto::Switch> = None;
    if let Some(path) = &config.switch_file {
        match crate::switch::read(path) {
            Ok(Some(written)) => {
                let read = crate::switch::at_startup(written.clone());
                if let Some(said) = &read.converted {
                    eprintln!("herd-runner: {said}");
                    // Written back, so that there is one state and not two:
                    // the hub is about to be told `off`, and a file still
                    // saying `drain` would be a second truth the watcher
                    // reads a second later. If it cannot be written the
                    // position is still taken — and then the watcher has to
                    // be told to keep ignoring that same reading, or the
                    // restart loop comes back through the back door.
                    match crate::switch::write(path, &read.position) {
                        Ok(()) => eprintln!(
                            "herd-runner: {} now says {}, so this is read once and not every \
                             restart",
                            path.display(),
                            read.position.sentence()
                        ),
                        Err(error) => {
                            a_stale_drain = Some(written);
                            eprintln!(
                                "herd-runner: that position is in force but could not be \
                                 written down, so the file still says drain and this runner \
                                 will go on reading it as off until the file changes: {error}"
                            );
                        }
                    }
                }
                eprintln!(
                    "herd-runner: the owner's switch says {} ({}); {}",
                    read.position.sentence(),
                    path.display(),
                    what_that_position_does(&read.position, 0)
                );
                session.set_switch(read.position);
            }
            Ok(None) => {}
            Err(why) => eprintln!(
                "herd-runner: this runner starts switched {} because its switch file will not \
                 read, and an unreadable switch is never read as on: {why}",
                session.switch().sentence()
            ),
        }
    }
    let watching_the_switch = tokio::spawn(watch_the_switch(
        config.clone(),
        session.clone(),
        a_stale_drain,
    ));
    let stop_watching_the_switch = watching_the_switch.abort_handle();
    let stop_probing = probing.abort_handle();
    let serving = serve_forever(config.clone(), session.clone(), Arc::clone(&confined));
    // A `drain` position is "finish and leave" (E-33), and this is the leaving
    // half: it waits for what the runner holds to be over, and then stops the
    // process through exactly the path a `SIGTERM` stops it through. There is
    // nothing to kill by then — which is the difference between this and the
    // shutdown drain, and the whole of attack 2: a drain that killed the job
    // it was waiting for would have finished nothing.
    let left_when_empty = {
        let session = session.clone();
        let poll = config.switch_poll;
        async move {
            loop {
                let switch = session.switch();
                if switch.leaves_when_empty() && session.jobs_running() == 0 {
                    return format!(
                        "the owner's switch is {} and this runner holds nothing, so it leaves",
                        switch.sentence()
                    );
                }
                tokio::time::sleep(poll).await;
            }
        }
    };
    let stop = async move {
        tokio::select! {
            reason = stop => reason,
            reason = left_when_empty => reason,
        }
    };
    let stopping = async {
        let reason = stop.await;
        stop_watching_the_switch.abort();
        // Before the drain rather than after it: a draining runner offers
        // nothing, so a probe it is still reading is work nobody wants, and
        // the kill belongs at the start of the owner's five seconds.
        stop_probing.abort();
        drain(&config, &session, &confined, &reason).await;
    };
    // Both are polled until one of them finishes, which is what lets the
    // drain go on being served: the acknowledgement of a result the drain has
    // just produced arrives on that same connection, and a stop that stopped
    // reading could never see one — so every stop would write down a verdict
    // the hub in fact had.
    let ended = tokio::select! {
        outcome = serving => outcome,
        () = stopping => Ok(()),
    };
    // Whichever way this runner is leaving — a signal, a refusal, a run out of
    // redials — no probe of its own outlives it. Aborted *and awaited*:
    // aborting only asks for the task to be dropped, and it is the drop that
    // kills the probe's process group, so a stop that did not wait here would
    // race its own promise.
    probing.abort();
    let _ = probing.await;
    // And no switch watcher either: a task reading a file every second for a
    // runner that has left is the leak M8 found in this same shape (D93).
    watching_the_switch.abort();
    let _ = watching_the_switch.await;
    // Whichever way the runner is leaving, nothing it holds goes with it. A
    // refusal and a run out of redials end the process just as a signal does,
    // and a result that exists nowhere else is not allowed to end with it.
    for line in session.spool(confined.root()) {
        eprintln!("herd-runner: {line}");
    }
    ended
}

/// Refuse new jobs, end what is running, flush what can be flushed, and stop
/// — inside [`Config::drain_grace`] whatever happens (invariant 2).
///
/// One path, owned by two milestones at once. M3 says the runner stops inside
/// five seconds and refuses work while it is stopping; M4 says no process, no
/// cgroup and no workspace of a job outlives the runner that was watching it;
/// and D19 says a verdict is never destroyed. So the order is: refuse new
/// work and tell every job it is over, wait — briefly — for each job's own
/// task to kill its process tree, reclaim its workspace and report, then kill
/// whatever did not manage that, and let [`run_until`] write down whatever
/// the hub still has not acknowledged.
async fn drain(config: &Config, session: &Session, confined: &Arc<Confined>, reason: &str) {
    // The flush is carved out of the grace rather than added to it. A drain
    // that spent its whole budget waiting for a job would otherwise reach the
    // flush with nothing left, and the last frames it produced — the
    // workspace a stop left behind, above all — would never leave the
    // process. The owner's five seconds is the whole of it, flush included.
    let deadline = Instant::now() + config.drain_grace.saturating_sub(FINAL_FLUSH);
    let ending = session.start_draining(reason);
    // On stderr, like everything else the stop path says: these lines are
    // what an owner reads to see that the runner went quietly, and they have
    // to be in the same stream as the kills and the spool paths that follow.
    eprintln!(
        "herd-runner: draining ({reason}); {} job(s) to end, no new job accepted",
        ending.len()
    );
    // Every job was told to end. What is waited for is that each one manages
    // to say so to the hub — so that a job is never silently lost — and that
    // what it said was acknowledged, so that the spool is for verdicts the
    // hub really does not have. The wait is bounded by the owner's five
    // seconds, never by the job.
    while Instant::now() < deadline
        && (session.jobs_running() > 0 || (!session.holds_nothing() && session.linked()))
    {
        tokio::time::sleep(DRAIN_POLL).await;
    }
    let left = session.jobs_running();
    if left > 0 {
        eprintln!(
            "herd-runner: {left} job(s) did not end within {:?}; stopping anyway, because \
             the owner's machine is the owner's (invariant 2). The hub will record the \
             attempt as lost.",
            config.drain_grace
        );
    }
    // The backstop, and the part that must never be missing: a job whose own
    // task did not get that far still has a process tree, a cgroup and a
    // workspace on this host, and none of them is anybody else's to collect.
    for ended in confined.live().end_all() {
        eprintln!("herd-runner: {}", ended.said);
        if let Some(error) = &ended.workspace_left {
            // Into the job's own log, which is part of its record: the
            // runner's stderr is not somewhere a requester can read, and
            // bytes left on a host that nobody will collect are exactly what
            // invariant 2 forbids.
            session.send_chunk(
                &ended.job_id,
                ended.attempt,
                LogStream::Stderr,
                format!(
                    "herd-runner: this runner was stopped and the workspace {} of job {} \
                     could not be reclaimed; it is left on this host: {error}\n",
                    ended.workspace.display(),
                    ended.job_id
                )
                .into_bytes(),
            );
        }
    }
    // The moment the frames just produced need to leave, which the deadline
    // above kept out of the waiting. Only skipped where there is nothing to
    // flush to, so a runner with no hub at all does not sit here.
    if session.linked() {
        tokio::time::sleep(FINAL_FLUSH).await;
    }
    eprintln!("herd-runner: stopped");
}

/// Resolves when the owner asks this runner to stop, with what to call it.
///
/// A signal is how an owner asks today: `systemctl stop`, a Ctrl-C, a
/// container being told to go. `SIGKILL` cannot be waited for at all, which
/// is why there is also a reaper at startup ([`reclaim`]). The owner's
/// *switch* — off with a reason, on again, from her own desktop — is E-33 and
/// M9's; this is the stop path it will pull.
async fn owner_asks_to_stop() -> String {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("herd-runner: cannot listen for SIGTERM: {error}");
                std::future::pending::<()>().await;
                unreachable!()
            }
        };
        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("herd-runner: cannot listen for SIGINT: {error}");
                std::future::pending::<()>().await;
                unreachable!()
            }
        };
        tokio::select! {
            _ = terminate.recv() => "the runner's owner asked it to stop (SIGTERM)".to_owned(),
            _ = interrupt.recv() => "the runner's owner asked it to stop (SIGINT)".to_owned(),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "the runner's owner asked it to stop".to_owned()
    }
}

/// Dial, serve, redial, until a refusal or the give-up bound.
///
/// **What resets the backoff, and why it is not a clean close (D104).** Both
/// counters below — how many failures in a row, and when the run of them
/// started — are about *a hub that cannot be reached*. A connection that was
/// welcomed and then served is the opposite of that, however it ended, so it
/// is what clears them. It used to be a clean WebSocket close instead, which
/// measurement showed is a case that never happens: a hub stopped with
/// `SIGTERM`, which is how a deployment stops it, ends the link as a reset.
/// Both counters were therefore monotonic for the life of the process, which
/// walked the delay to its cap for ever and — worse — let the give-up bound
/// kill a runner that had been serving happily for a day.
///
/// **A connection has to have lasted the cap to count.** Otherwise a hub that
/// accepts and dies in a loop is answered by a dial every `first`, which is
/// the thundering herd the backoff exists to prevent, and the give-up bound
/// never fires on a hub that is broken rather than absent. With the floor at
/// the cap, the reset can never make this runner dial faster than once per
/// cap — which is the slowest the backoff was ever going to settle at anyway,
/// so the floor costs nothing and bounds everything.
async fn serve_forever(config: Config, session: Session, confined: Arc<Confined>) -> Result<()> {
    let mut failures = 0u32;
    let mut failing_since: Option<Instant> = None;
    loop {
        let began = Instant::now();
        let outcome = serve_once(config.clone(), session.clone(), Arc::clone(&confined)).await;
        // Whether this attempt was ever a connection, and for how long. Read
        // before anything is decided, because every arm below wants it and
        // one of them returns.
        let settled = config.redial.settled(session.served_since(began));
        if settled {
            failures = 0;
            failing_since = None;
        }
        let delay = match outcome {
            // The hub closed the connection. Nothing says the runner is done,
            // so it waits and joins the hub that comes back.
            Ok(()) => config.redial.delay(failures.max(1)),
            Err(error) if error.downcast_ref::<Refusal>().is_some() => return Err(error),
            // Asked to come back, which is not a failure at all: the budget
            // for a hub that cannot be reached is not spent on a hub that
            // answered (D13, D38).
            Err(error) => match error.downcast_ref::<StandBy>() {
                Some(stand_by) => {
                    println!("herd-runner: {stand_by}");
                    stand_by.retry_in
                }
                None => {
                    failures += 1;
                    let since = *failing_since.get_or_insert_with(Instant::now);
                    eprintln!("herd-runner: the link to the hub ended: {error:#}");
                    if since.elapsed() >= config.redial.give_up_after {
                        return Err(error.context(config.redial.gave_up(&config.hub_url, failures)));
                    }
                    let delay = config.redial.delay(failures);
                    println!(
                        "herd-runner: dialling {} again in {:?} (attempt {})",
                        config.hub_url,
                        delay,
                        failures + 1
                    );
                    delay
                }
            },
        };
        tokio::time::sleep(delay).await;
    }
}

/// Write a position down and apply it, wherever it came from (E-33).
///
/// One function for both halves of E-33 — the file an owner writes and the
/// frame an orchestrator sends — because the state they leave has to be one
/// state. A file that cannot be written is said out loud and the position is
/// still applied: the owner asked for her machine back, and a read-only data
/// root is not a reason to go on claiming work. What is lost in that case is
/// only that a restart would not remember, and the line says so — and, since
/// D99, the hub is told so rather than left to read a health report as proof
/// of something it is not.
///
/// **The file goes first, and that order is the fix D99 records.** Applying a
/// position tells the hub at once (`Session::set_switch` sends a health report
/// in the same breath), and the hub stopping to claim is what `herdctl switch`
/// used to take as its answer. Written afterwards, there was a window — a
/// second and a half on a two-core host — in which the owner had been told
/// success, the hub honoured the position, and a `kill -9` brought the machine
/// back on its previous one. The owner's own path (`herd-runner switch`) was
/// always this way round: the command writes the file and the running runner
/// reads it. Now both are.
///
/// Returns what to tell the hub: in force and durable, or in force and not.
fn apply_a_switch(
    config: &Config,
    session: &Session,
    switch: herd_proto::Switch,
    why: &str,
) -> herd_proto::SwitchPersistence {
    let persistence = match &config.switch_file {
        Some(path) => match crate::switch::write(path, &switch) {
            Ok(()) => herd_proto::SwitchPersistence::Written {
                path: path.display().to_string(),
            },
            Err(error) => {
                eprintln!(
                    "herd-runner: this switch is in force now, but it could not be written \
                     down, so a restart of this runner would come back {}: {error}",
                    herd_proto::Switch::default().sentence()
                );
                herd_proto::SwitchPersistence::NotWritten { why: error }
            }
        },
        // A runner started with no switch file keeps no position across a
        // restart at all, and that is a configuration rather than a failure —
        // but it is still not the durability an owner is promised, so it is not
        // reported as one.
        None => herd_proto::SwitchPersistence::NotWritten {
            why: "this runner was started with no switch file, so no position it is given \
                  outlives its process"
                .to_owned(),
        },
    };
    if let Some(said) = session.set_switch(switch) {
        eprintln!("herd-runner: {said} ({why})");
    }
    persistence
}

/// Watch the switch file for ever, applying what it says (E-33).
///
/// Beside the redial loop, like the probes, and for the same reason: an owner
/// switching her machine off while the hub is down must still be obeyed, and
/// the next connection announces the position in its handshake.
///
/// `a_stale_drain` is the reading startup converted to `off` and could not
/// write back (D164). A reading equal to it is the same stale word and is
/// skipped; anything else is the owner speaking now, is honoured, and clears
/// the staleness — including a fresh `drain`, which is still "finish and
/// leave" and still obeyed.
async fn watch_the_switch(
    config: Config,
    session: Session,
    mut a_stale_drain: Option<herd_proto::Switch>,
) {
    let Some(path) = config.switch_file.clone() else {
        return;
    };
    let mut said_it_will_not_read: Option<String> = None;
    loop {
        match crate::switch::read(&path) {
            Ok(read) => {
                said_it_will_not_read = None;
                // An absent file is a machine nobody has withdrawn, and that
                // is how a switch is turned back on with `rm`.
                let switch = read.unwrap_or_default();
                if a_stale_drain.as_ref() == Some(&switch) {
                    tokio::time::sleep(config.switch_poll).await;
                    continue;
                }
                a_stale_drain = None;
                if let Some(said) = session.set_switch(switch) {
                    eprintln!("herd-runner: {said} (from {})", path.display());
                }
            }
            // **The position is left where it is.** A file that will not read
            // is not a runner that claims: the one reading that could do harm
            // is treating a mangled switch file as `on`, which would be a
            // withdrawn machine quietly taking work again.
            Err(why) => {
                if said_it_will_not_read.as_deref() != Some(why.as_str()) {
                    eprintln!(
                        "herd-runner: the switch stays {} because its file will not read, and \
                         an unreadable switch is never read as on: {why}",
                        session.switch().sentence()
                    );
                    said_it_will_not_read = Some(why);
                }
            }
        }
        tokio::time::sleep(config.switch_poll).await;
    }
}

/// Join the hub and serve it until this one connection ends.
///
/// Public because a test needs one connection it can watch, and because
/// [`run`] is only this in a loop. The confinement is the caller's, not this
/// connection's: the data root is locked for the life of the process, so one
/// `Confined` serves every connection a runner ever makes.
pub async fn serve_once(config: Config, session: Session, confined: Arc<Confined>) -> Result<()> {
    // Herd's own frame ceiling, not the library's: its `max_frame_size`
    // default is 16 MiB, which nobody here chose, and a runner must be able
    // to read anything the hub may legitimately send. Stated on this socket
    // for the same reason the hub states it on the ones it accepts — one
    // number for the whole system, readable off one line.
    let ceiling = usize::try_from(herd_proto::FRAME_CEILING).unwrap_or(usize::MAX);
    let limits = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(ceiling))
        .max_frame_size(Some(ceiling));
    let (socket, _response) =
        tokio_tungstenite::connect_async_with_config(&config.hub_url, Some(limits), false)
            .await
            .with_context(|| format!("dialling {}", config.hub_url))?;
    let (mut sink, mut source) = socket.split();

    // This machine's own ceiling, worked out once from what this machine
    // itself declared and from nothing the hub has said: the memory it lends,
    // the floor it keeps, the cap it puts on every job. It is computed here,
    // before the handshake, so that **the figure announced and the figure
    // enforced are one value** and cannot drift — the same `Arc` is sent in
    // the hello below and handed to `serve_hub`, which is where it is held
    // against every assignment.
    let ceiling = Arc::new(confine::jobs_at_once(
        &config.host,
        confined.caps().memory_max,
    ));

    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        // Which build this is, beside which protocol it speaks (E-51). The
        // protocol version says the hub can read these frames; this says
        // whether the two sides agree on what they mean, which is the thing
        // a fleet drifting apart gets wrong.
        peer_version: runner_version().to_owned(),
        role: Role::Runner,
        token: config.token.clone(),
        name: config.name.clone(),
        // Which process this is, so the hub can tell this runner dialling
        // again from a second runner wearing its name.
        instance: Some(session.instance()),
        // How big this machine is and how preferred it is: the two things
        // the hub's claim step needs about a machine that it cannot work out
        // for itself (E-22, E-19).
        host: config.host,
        // And the ceiling derived from that host, so the hub stops placing
        // work this runner is certain to hand straight back (D-NEXT).
        // Announced, never asked for: the runner enforces this figure
        // whatever the hub does with it.
        //
        // **A hub that ignores it is not free, and the version to measure
        // that against is `v0.3.7`, the one in the field.** On `v0.3.7` this
        // runner had no ceiling at all, so a hub placing a queue of
        // memory-silent jobs on it cost *no* refusals — it ran them all, two
        // deep or ten, which is the invariant 2 defect this change closes.
        // A fleet whose runners are upgraded before its hub pays refusals it
        // never paid before, and `attempts_allowed` is 2, so a job refused
        // twice ends with no verdict. The order is therefore **hub first**,
        // and nothing here detects it: `Welcome` carries no echo of this
        // field, the protocol version deliberately does not move, and a
        // runner cannot tell a hub that honoured its announcement from one
        // that never read it (`docs/remediation/09-runner-self-limits.md`
        // §7.6).
        jobs_at_once: Some(u32::try_from(ceiling.most()).unwrap_or(u32::MAX)),
        preference: config.preference,
        // And where its owner's switch stands, which is announced rather than
        // assumed: a runner restarted while switched off has not changed its
        // owner's mind, and a hub that believed it `on` until the first health
        // report could claim on it inside that window (E-33).
        switch: session.switch(),
        // And which hub state the durable things under this data root belong
        // to, so that nothing spooled for a hub that no longer exists is
        // believed by the hub that replaced it.
        state_root: confined.root().hub_state_root(),
        // And how often this runner will really say it is alive. The hub's
        // silence threshold is only meaningful as a multiple of *this*
        // figure, and a hub cannot check a pairing it cannot see: a hub
        // expecting a report every second and a runner sending one every ten
        // are each self-consistent and together declare a healthy job lost
        // (E-40, E-21).
        health_every_ms: Some(config.health_every.as_millis() as u64),
        // The session's, not the config's: one runner has one offer, and the
        // session is what survives a reconnect with it.
        capabilities: session.capacity().offer().clone(),
        // The load as this runner dials, so that a job placed before the
        // first health report still has a figure on its line (E-32).
        load_milli: crate::metrics::host_load_milli(),
    };
    sink.send(Message::text(to_frame(&hello).context("encoding hello")?))
        .await
        .context("sending hello")?;

    match next_message(&mut source, config.hub_silence_limit).await? {
        Some(FromHub::Welcome {
            state_root,
            version_note,
            ..
        }) => {
            // What the hub has to say about the gap between its build and
            // this one (E-51). On this machine's stderr, because the person
            // who can move this runner to another version is the person
            // reading it here; the hub says it on its own side too.
            if let Some(note) = &version_note {
                eprintln!("herd-runner: {note}");
            }
            // This connection is serving: what ends it from here on is a link
            // that was up, which is not the same thing as a hub that cannot
            // be reached (D104).
            session.welcomed();
            // Before the link is attached and so before anything is offered
            // on it: what this data root is holding has to be settled against
            // the state root that has just welcomed it.
            for line in session.settle_against_state_root(confined.root(), &state_root) {
                eprintln!("herd-runner: {line}");
            }
        }
        Some(FromHub::Refused { reason }) => return Err(Refusal(reason).into()),
        Some(FromHub::StandBy {
            reason,
            retry_in_ms,
        }) => {
            return Err(StandBy {
                reason,
                retry_in: Duration::from_millis(retry_in_ms),
            }
            .into())
        }
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
    }

    let (outbox, inbox) = mpsc::unbounded_channel();
    // A fresh window for a fresh connection (see [`Inner::files_credit`]):
    // chunks queued on the link that broke went with it, so their permits
    // would never have come back and a counter carried across redials would
    // run down to nothing.
    let credit = Arc::new(Semaphore::new(config.files_in_flight));
    let mut writer = tokio::spawn(write_frames(sink, inbox, Arc::clone(&credit)));
    // Before anything else on this connection: what the last one could not
    // deliver. A job finished while the hub was away is still a job that ran,
    // and output the hub never wrote is still output the job produced.
    session.attach(outbox, credit, config.files_stall_after);
    // And what every probe says, before this connection can be dispatched on.
    session.announce_capabilities();
    let reporting = tokio::spawn(report_health(session.clone(), config.health_every));
    let outcome = serve_hub(
        &config,
        &session,
        &mut source,
        confined,
        ceiling,
        config.hub_silence_limit,
    )
    .await;
    reporting.abort();
    session.detach();
    // The writer is holding a queue for a hub that may be gone, and the jobs
    // still running hold senders of their own, so waiting for it to end by
    // itself would mean waiting for those jobs. They report on the next link.
    if tokio::time::timeout(FLUSH_GRACE, &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
    outcome
}

/// Say what this runner is doing, for as long as this connection lasts (E-40).
async fn report_health(session: Session, every: Duration) {
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if !session.send(session.health()) {
            return;
        }
    }
}

async fn serve_hub(
    config: &Config,
    session: &Session,
    source: &mut Source,
    confined: Arc<Confined>,
    // This machine's own ceiling, worked out by `serve_once` from what this
    // machine itself declared — the memory it lends, the floor it keeps, the
    // cap it puts on every job — and from nothing the hub has said. Handed in
    // rather than recomputed because it is the **same value** the hello
    // announced (D-NEXT): a figure announced and a figure enforced that were
    // derived twice could differ, and the one that matters is this one.
    //
    // It is a property of the host and of its configuration, neither of which
    // a connection can change. **Held whatever the hub does with the
    // announcement**: a runner that learned its limits from the peer handing
    // out the work would have no limits at all the day that peer is wrong
    // (invariant 2).
    ceiling: Arc<confine::JobsAtOnce>,
    silence_limit: Duration,
) -> Result<()> {
    while let Some(message) = next_message(source, silence_limit).await? {
        match message {
            FromHub::AssignJob {
                job_id,
                attempt,
                command,
                declaration,
                timeout_s,
            } => {
                let session = session.clone();
                let confined = Arc::clone(&confined);
                let ceiling = Arc::clone(&ceiling);
                // A declared command runs through the consumer's four
                // executor steps; an ad-hoc one is the whole of what runs.
                // Both go through [`job::execute`], which is where the caps,
                // the log, the cancellation and the verdict live (D108).
                let declared_timeout = timeout_s.map(Duration::from_secs);
                // One task per job: a long job must not stop the runner from
                // hearing the hub, and must not end when the link does.
                //
                // **The task is spawned for every assignment and the ceiling
                // is held inside it**, at `take_job`, rather than here. The
                // count has to be read under the same lock that writes the
                // job down or two assignments arriving together would both
                // find room for the last place; a check here would be that
                // race with a wider window. What a refused assignment costs
                // is one task that sends one frame and ends.
                tokio::spawn(async move {
                    job::execute(
                        job_id,
                        attempt,
                        command,
                        declaration,
                        declared_timeout,
                        session,
                        confined,
                        ceiling,
                    )
                    .await
                });
            }
            FromHub::JobResultAck { job_id } => session.result_acknowledged(&job_id),
            FromHub::JobFilesHeld {
                job_id,
                attempt,
                held,
            } => session.files_held(&job_id, attempt, &held),
            FromHub::JobFilesAck { job_id, attempt } => {
                session.files_acknowledged(&job_id, attempt)
            }
            FromHub::LogAck {
                job_id,
                attempt,
                through_seq,
            } => session.log_acknowledged(&job_id, attempt, through_seq),
            FromHub::ResendLog {
                job_id,
                attempt,
                from_seq,
            } => session.resend_log(&job_id, attempt, from_seq),
            FromHub::CancelJob {
                job_id,
                attempt,
                reason,
            } => {
                if session.end_job(&job_id, attempt, Ending::Cancelled(reason.clone())) {
                    println!("herd-runner: ending {job_id} because {reason}");
                } else {
                    // Already over, or never here. Not an error: a cancel
                    // racing the end of a job is the ordinary case.
                    println!(
                        "herd-runner: asked to end attempt {attempt} of {job_id}, which this \
                         runner is not running"
                    );
                }
            }
            FromHub::StaleAttempt {
                job_id,
                attempt,
                reason,
            } => {
                session.drop_stale(&job_id, attempt, &reason);
            }
            FromHub::HealthRecorded {} => {}
            // The orchestrator's half of E-33, arriving through the hub. It is
            // written down **and then** applied, so that the two ways of
            // setting a switch leave one state behind: an owner who later looks
            // at her machine sees the position an agent set, and a runner
            // restarted afterwards comes back on it.
            //
            // And it is acknowledged, because whoever typed `herdctl switch` is
            // waiting for exactly this and nothing else can stand in for it
            // (D99).
            FromHub::SetSwitch { switch, why } => {
                let persistence = apply_a_switch(config, session, switch.clone(), &why);
                session.send(ToHub::SwitchApplied {
                    switch,
                    persistence,
                });
            }
            // A capability this runner handed out turned out to be taken
            // (D76). The reading that allowed it is known to be stale, so it
            // is taken again now rather than at the next interval.
            FromHub::ReadProbesAgain { why } => {
                eprintln!("herd-runner: reading every probe again now: {why}");
                if session.capacity().sweep().await {
                    session.announce_capabilities();
                }
            }
            FromHub::Refused { reason } => return Err(Refusal(reason).into()),
            FromHub::StandBy {
                reason,
                retry_in_ms,
            } => {
                return Err(StandBy {
                    reason,
                    retry_in: Duration::from_millis(retry_in_ms),
                }
                .into())
            }
            // Already named where the text was, in `next_message`. Nothing
            // here, and nothing is the behaviour: the link stays up and the
            // runner goes on serving (D148).
            FromHub::Unknown => {}
            other => {
                eprintln!(
                    "herd-runner: ignoring {}, which is not a runner's business",
                    message_kind(&other)
                );
            }
        }
    }
    Ok(())
}

/// Drain the link's queue onto the socket, and give a file chunk's window
/// permit back once it is really on the wire.
///
/// The permit is returned **here** and nowhere earlier, which is what makes
/// [`Inner::files_credit`] a window on the socket rather than on the channel:
/// returning it when the chunk was queued would have bounded nothing, because
/// queueing is the fast half.
async fn write_frames(mut sink: Sink, mut inbox: UnboundedReceiver<ToHub>, credit: Arc<Semaphore>) {
    while let Some(message) = inbox.recv().await {
        let was_a_chunk = matches!(message, ToHub::JobFileChunk { .. });
        match to_frame(&message) {
            Ok(frame) => {
                if sink.send(Message::text(frame)).await.is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("herd-runner: a message would not encode: {error}"),
        }
        if was_a_chunk {
            credit.add_permits(1);
        }
    }
    let _ = sink.close().await;
}

/// The next message from the hub, or an error if the hub says nothing at all
/// for too long.
///
/// The timeout is the runner's half of E-21: the hub answers every health
/// report, so a link that has carried nothing for as long as the hub's own
/// silence threshold is a link onto a hub that is not there. Redialling is
/// free; holding a socket open onto nothing is not.
async fn next_message(source: &mut Source, silence_limit: Duration) -> Result<Option<FromHub>> {
    loop {
        let frame = match tokio::time::timeout(silence_limit, source.next()).await {
            Ok(Some(frame)) => frame.context("reading a frame")?,
            Ok(None) => return Ok(None),
            Err(_) => bail!(
                "the hub has said nothing for {silence_limit:?}, so this link is dead \
                 even though its socket is open"
            ),
        };
        match frame {
            Message::Text(text) => {
                let message: FromHub = from_frame(text.as_str())
                    .map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
                // **A frame from a newer hub, ignored by name, on the link
                // that stays up** (D148). This used to be the `?` above: the
                // decode failed, the error killed the link, and the runner
                // redialled straight into the same frame — it looped rather
                // than shrugged, which is D128's recorded defect and the
                // reason `PROTOCOL_VERSION` 2 was worth spending.
                //
                // Said here rather than in the handler because this is where
                // the **text** is: `Unknown` is a unit variant, so the tag is
                // discarded on the way to it, and a line that cannot name the
                // type is a line nobody can act on.
                if message == FromHub::Unknown {
                    eprintln!(
                        "herd-runner: ignoring a frame of type {:?}, which this build does \
                         not know (it speaks protocol {PROTOCOL_VERSION})",
                        herd_proto::frame_type(text.as_str())
                            .unwrap_or_else(|| "(none)".to_owned())
                    );
                }
                return Ok(Some(message));
            }
            Message::Close(_) => return Ok(None),
            Message::Binary(_) => bail!("the protocol carries text frames only (D4)"),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
}

fn message_kind(message: &FromHub) -> &'static str {
    match message {
        FromHub::JobFilesHeld { .. } => "job_files_held",
        FromHub::JobFilesAck { .. } => "job_files_ack",
        FromHub::JobFile { .. } => "job_file",
        FromHub::JobFilesSent { .. } => "job_files_sent",
        // Named rather than left to a wildcard, because the one thing this
        // function is for is saying what arrived — and `Unknown` is the case
        // where that matters most. The real type is read off the frame text
        // in `next_message`, which is the only place that still has it.
        FromHub::Unknown => "a frame type this build does not know",
        FromHub::Welcome { .. } => "welcome",
        FromHub::Refused { .. } => "refused",
        FromHub::StandBy { .. } => "stand_by",
        FromHub::HealthRecorded {} => "health_recorded",
        FromHub::JobAccepted { .. } => "job_accepted",
        FromHub::AssignJob { .. } => "assign_job",
        FromHub::CancelJob { .. } => "cancel_job",
        FromHub::StaleAttempt { .. } => "stale_attempt",
        FromHub::LogChunk { .. } => "log_chunk",
        FromHub::LogAck { .. } => "log_ack",
        FromHub::ResendLog { .. } => "resend_log",
        FromHub::JobLog { .. } => "job_log",
        FromHub::JobNote { .. } => "job_note",
        FromHub::StillThere { .. } => "still_there",
        FromHub::RanOn { .. } => "ran_on",
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
        FromHub::WouldRun { .. } => "would_run",
        FromHub::SwitchSet { .. } => "switch_set",
        FromHub::FleetSwitches { .. } => "fleet_switches",
        FromHub::RunnerApproved { .. } => "runner_approved",
        FromHub::RunnerRemoved { .. } => "runner_removed",
        FromHub::ApprovedRunners { .. } => "approved_runners",
        FromHub::SetSwitch { .. } => "set_switch",
        FromHub::ReadProbesAgain { .. } => "read_probes_again",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the policy, which is the part a reader has to be able to
    /// check: it grows, it is capped, and it never comes back the same twice.
    #[test]
    fn the_redial_delay_grows_to_a_cap_and_is_jittered() {
        let backoff = Backoff {
            first: Duration::from_millis(500),
            cap: Duration::from_secs(30),
            give_up_after: Duration::from_secs(60),
        };
        // Never above the cap, never below half of what is asked for: a delay
        // jittered to nearly nothing would be no backoff at all.
        for failures in 1..=20 {
            let delay = backoff.delay(failures);
            assert!(
                delay <= backoff.cap,
                "{failures}: {delay:?} is above the cap"
            );
            let grown = backoff
                .first
                .saturating_mul(2u32.saturating_pow((failures - 1).min(30)))
                .min(backoff.cap);
            assert!(
                delay >= grown / 2,
                "{failures}: {delay:?} is less than half of {grown:?}"
            );
        }
        // It grows: the tenth delay is at the cap, the first is not.
        assert!(backoff.delay(1) < backoff.cap / 2);
        assert!(backoff.delay(10) >= backoff.cap / 2);
        // And two runners do not keep the same phase.
        let delays: Vec<Duration> = (0..8).map(|_| backoff.delay(10)).collect();
        assert!(
            delays.iter().any(|delay| *delay != delays[0]),
            "a jittered delay that never varies is not jittered: {delays:?}"
        );
    }

    /// What earns a fresh start, and what does not (D104).
    #[test]
    fn only_a_connection_that_lasted_the_cap_clears_the_backoff() {
        let backoff = Backoff {
            first: Duration::from_millis(500),
            cap: Duration::from_secs(30),
            give_up_after: Duration::from_secs(60),
        };
        // An attempt that was never welcomed is the case the backoff is for,
        // however long it took to fail. A dial that hangs for a minute and a
        // link that serves for a minute are the same span and opposite facts.
        assert!(!backoff.settled(None));
        // A connection shorter than the cap does not count: a hub that
        // accepts and dies in a loop must still be backed off, and must still
        // reach the give-up bound.
        assert!(!backoff.settled(Some(Duration::ZERO)));
        assert!(!backoff.settled(Some(backoff.cap - Duration::from_millis(1))));
        // At the cap and beyond it does.
        assert!(backoff.settled(Some(backoff.cap)));
        assert!(backoff.settled(Some(Duration::from_secs(60 * 60))));
        // And the property that makes the floor defensible: a reset can never
        // let this runner dial more often than the cap already allowed. Even
        // in the worst case — a hub that serves for exactly the floor and then
        // dies, for ever — each round costs a connection of `cap` before the
        // delay returns to `first`.
        assert!(
            backoff.cap >= backoff.delay(1),
            "the floor is the slower of the two"
        );
    }

    /// A welcome from an earlier connection does not excuse this attempt
    /// (D104).
    #[test]
    fn a_stale_welcome_is_not_read_as_a_connection_that_served() {
        let session = Session::new();
        let before = Instant::now();
        session.welcomed();
        // Judged against a moment at or before the welcome: this attempt was
        // the one that got in.
        let served = session
            .served_since(before)
            .expect("the welcome belongs to this attempt");
        assert!(served < Duration::from_secs(1), "{served:?}");
        // Judged against a later moment — a dial that failed after that
        // connection had ended — it says nothing, which is the point: a
        // runner that served for an hour and then lost its hub for good must
        // not read its own old welcome as proof that every dial since was a
        // connection.
        assert_eq!(session.served_since(Instant::now()), None);
        // And a session that has never been welcomed has nothing to say
        // either way.
        assert_eq!(Session::new().served_since(before), None);
    }

    #[test]
    fn giving_up_names_the_hub_and_how_long_it_tried() {
        let backoff = Backoff::default();
        let said = backoff.gave_up("ws://hub.example:8787", 2931);
        assert!(said.contains("ws://hub.example:8787"), "{said}");
        assert!(said.contains("2931"), "{said}");
        assert!(said.contains("24h"), "{said}");
    }

    /// A runner holds output against an acknowledgement, and the hold is
    /// bounded: the oldest goes first, and what is dropped is said out loud.
    #[test]
    fn held_output_is_bounded_and_drops_the_oldest_first() {
        let mut job = JobInHand {
            next_seq: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            end: None,
            cancel: CancelToken::new(),
            trimmed: 0,
        };
        let chunk = vec![b'x'; 1024 * 1024];
        for seq in 0..8 {
            job.remember(seq, LogStream::Stdout, &chunk);
        }
        assert!(
            job.unacked_bytes <= HELD_BYTES_PER_JOB,
            "{} bytes held",
            job.unacked_bytes
        );
        let oldest = job.unacked.front().expect("something is held").0;
        assert!(oldest > 0, "the oldest chunks must be the ones dropped");
        let newest = job.unacked.back().expect("something is held").0;
        assert_eq!(newest, 7, "the newest chunk is always held");
    }

    #[test]
    fn an_acknowledged_chunk_is_forgotten_and_the_rest_is_kept() {
        let mut job = JobInHand {
            next_seq: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            end: None,
            cancel: CancelToken::new(),
            trimmed: 0,
        };
        for seq in 0..5 {
            job.remember(seq, LogStream::Stdout, b"hello");
        }
        job.forget_through(3);
        let left: Vec<u64> = job.unacked.iter().map(|(seq, _, _)| *seq).collect();
        assert_eq!(left, vec![3, 4]);
        assert_eq!(job.unacked_bytes, 10);
    }

    /// One ask, one ending: a cancel racing a drain does not end a job twice,
    /// and the first reason is the one the record gets.
    #[tokio::test]
    async fn a_job_is_only_ever_ended_once() {
        let token = CancelToken::new();
        assert!(token.end(Ending::Cancelled("a requester asked".to_owned())));
        assert!(!token.end(Ending::Abandoned("draining".to_owned())));
        assert_eq!(
            token.ended().await,
            Ending::Cancelled("a requester asked".to_owned())
        );
    }

    /// An attempt the hub has written off is announced **once**, however many
    /// frames of it the hub refuses (D151).
    ///
    /// The shape this is about: the runner buffers a job's output while the
    /// link is down, flushes all of it the instant the link is back, and the
    /// hub — correctly — answers every single chunk with its own
    /// `StaleAttempt`. On the real failure that found this, that was 1497
    /// refusals inside the same millisecond, and the runner echoed every one
    /// of them to the journal of a host whose IO was already stalled at 96 %.
    ///
    /// So the assertion is the **count**, not the wording. A test that
    /// checked the sentence would go green again the day somebody
    /// reintroduces the loop with different words; a test that counts cannot.
    #[tokio::test]
    async fn a_stale_attempt_is_announced_once_however_many_frames_it_buffered() {
        let session = Session::new();
        let job_id = JobId::new("j0000000057");
        let _cancel = session
            .take_job(&job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");

        // The buffered output, as the runner holds it with no link up. These
        // are the items that each draw a refusal out of the hub.
        let buffered = 1497;
        for _ in 0..buffered {
            session.send_chunk(
                &job_id,
                1,
                LogStream::Stdout,
                b"a line of a build\n".to_vec(),
            );
        }

        // One refusal per buffered item, which is what the hub really sends —
        // and the journal is read back, so what is counted is the writing
        // itself and not some return value a reintroduced loop could leave
        // untouched.
        let (returned, written) = testing::stderr_lines_about("j0000000057", || {
            (0..buffered)
                .filter(|_| {
                    session.drop_stale(
                        &job_id,
                        1,
                        "attempt 1 of j0000000057 is not the attempt this hub is waiting for; \
                         nothing of it is recorded",
                    )
                })
                .count()
        });
        assert_eq!(
            written.len(),
            1,
            "{buffered} refusals of one attempt must write one journal line, not {}: {written:#?}",
            written.len()
        );
        assert_eq!(
            returned, 1,
            "and exactly one call may claim to have said it"
        );

        // A different attempt of the same job is a different fact and is
        // still worth a line: the silence is per attempt, not per job.
        assert!(
            session.drop_stale(&job_id, 2, "a second attempt, written off too"),
            "silencing the repeats must not silence another attempt"
        );
    }

    /// The memory that keeps the repeats quiet is bounded: a runner that has
    /// seen more written-off attempts than it remembers must still not be
    /// able to grow that list for ever (invariant 2 again, in memory rather
    /// than in journal writes).
    #[tokio::test]
    async fn the_attempts_already_announced_are_remembered_in_a_bounded_ring() {
        let session = Session::new();
        // Diverted only to keep a hundred-odd true lines out of the suite's
        // own output; what this test is about is the length of the ring.
        testing::stderr_lines_about("", || {
            for n in 0..(REFUSALS_REMEMBERED * 4) {
                session.drop_stale(&JobId::new(format!("j{n:010}")), 1, "written off");
            }
        });
        let held = session.with(|inner| inner.said_stale.len());
        assert_eq!(
            held, REFUSALS_REMEMBERED,
            "the ring must stop at {REFUSALS_REMEMBERED}, not grow to {held}"
        );
    }

    #[tokio::test]
    async fn a_draining_runner_takes_no_new_job() {
        let session = Session::new();
        session.start_draining("the owner asked");
        assert!(session.draining());
        assert_eq!(
            session.take_job(&JobId::new("j1"), 1, 4).err(),
            Some(NotTaken::Draining),
            "a draining runner must refuse a job the hub had already sent, and say which \
             refusal it was"
        );
    }

    /// **The half of invariant 2 that had no local defence.** A runner's
    /// bound on how much work lands on its host was, until this, a property
    /// of the hub's arithmetic: `take_job` refused for a drain and for the
    /// owner's switch and for no count at all, so 250 assignments from a hub
    /// that was not one produced 250 jobs
    /// (`herd-hub/tests/fake_hub_flood.rs`, and §2 of
    /// `docs/remediation/04b-integrity-and-containment.md`).
    ///
    /// Here at the lock, where the count is read and the job written down
    /// without an instant in between.
    #[tokio::test]
    async fn a_runner_at_its_own_ceiling_refuses_and_says_what_the_figures_were() {
        let session = Session::new();
        let most = 3;
        for n in 1..=most {
            session
                .take_job(&JobId::new(format!("j{n:010}")), 1, most)
                .expect("a runner under its ceiling takes the job");
        }
        assert_eq!(
            session.take_job(&JobId::new("j0000000004"), 1, most).err(),
            Some(NotTaken::AtItsCeiling {
                running: most,
                most
            }),
            "a runner holding all it can hold must refuse the next one, whatever the hub \
             believes about it"
        );
    }

    /// A finished job whose verdict nobody has acknowledged holds a record
    /// and some unsent bytes. It holds no process and no memory ceiling, so
    /// it is not what the ceiling counts — a hub slow to acknowledge must not
    /// be able to shut a working machine down.
    #[tokio::test]
    async fn a_result_waiting_for_an_acknowledgement_does_not_hold_a_place() {
        let session = Session::new();
        let finished = JobId::new("j0000000001");
        session
            .take_job(&finished, 1, 1)
            .expect("the first job is taken");
        session.finish(
            &finished,
            JobOutcome::Exited { code: 0 },
            JobMetrics::default(),
            Vec::new(),
            herd_proto::JobOutputs::default(),
            None,
        );
        assert_eq!(session.results_held(), 1, "the verdict is still held");
        assert!(
            session.take_job(&JobId::new("j0000000002"), 1, 1).is_ok(),
            "a machine whose one job has ended has its place back, acknowledged or not"
        );
    }

    /// **Two attempts of one job on one machine are two things, and the
    /// machine's own count has to say two.**
    ///
    /// Reachable wherever an attempt outlives the hub's opinion of it: the
    /// link drops, the hub's watchdog writes the attempt off and queues the
    /// job again, the job goes on running here, and the next attempt of it
    /// comes back to this same machine the moment it redials. Keyed by job
    /// id alone, the second assignment *replaced* the first one's entry —
    /// so the first went on running with nothing counting it, nothing
    /// naming it in a health report, and its held output numbered as if it
    /// belonged to the attempt that replaced it.
    ///
    /// What this asserts is only the bookkeeping. Whether a runner should
    /// take a second attempt of a job it is already running is a separate
    /// question with a separate answer; this is about a map that cannot
    /// hold the truth either way.
    #[tokio::test]
    async fn two_attempts_of_one_job_are_two_entries_in_the_machines_own_count() {
        let session = Session::new();
        let job_id = JobId::new("j0000000401");
        let _first = session
            .take_job(&job_id, 1)
            .expect("a fresh session is not draining");
        let _second = session
            .take_job(&job_id, 2)
            .expect("a fresh session is not draining");
        assert_eq!(
            session.jobs_running(),
            2,
            "this machine is running two processes and its own count says {}: a count that \
             is short is a host that accepts work it has no room for (invariant 2)",
            session.jobs_running()
        );
        // And the health report names both, each under its own number. The
        // hub times an *attempt*, so a report that leaves one out is how a
        // perfectly healthy job gets written off for silence.
        let reported: Vec<u32> = session.with(|inner| {
            let mut attempts: Vec<u32> = running_jobs(inner)
                .into_iter()
                .filter(|job| job.job_id == job_id)
                .map(|job| job.attempt)
                .collect();
            attempts.sort_unstable();
            attempts
        });
        assert_eq!(
            reported,
            vec![1, 2],
            "both attempts of {job_id} are running here and the health report names {reported:?}"
        );
    }

    /// **Handing one attempt back must not let go of another one.**
    ///
    /// The first of the three consequences that were really observed: the
    /// runner refuses attempt 2 of a job, says so to the hub, and the
    /// *running* attempt 1 disappears from its own count. The place the
    /// machine does not have is then handed to the next assignment, which
    /// is invariant 2 broken by the bookkeeping rather than by a job.
    #[tokio::test]
    async fn handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running() {
        let session = Session::new();
        let job_id = JobId::new("j0000000402");
        let first = session
            .take_job(&job_id, 1)
            .expect("a fresh session is not draining");
        let _second = session
            .take_job(&job_id, 2)
            .expect("a fresh session is not draining");

        session.abandon(
            &job_id,
            2,
            "this machine is already as full as it said it would be",
        );

        assert_eq!(
            session.jobs_running(),
            1,
            "attempt 1 of {job_id} is still running on this machine and its own count says {}",
            session.jobs_running()
        );
        let reported: Vec<u32> = session.with(|inner| {
            running_jobs(inner)
                .into_iter()
                .filter(|job| job.job_id == job_id)
                .map(|job| job.attempt)
                .collect()
        });
        assert_eq!(
            reported,
            vec![1],
            "the health report must name the attempt that is running and not the one that \
             was handed back: {reported:?}"
        );
        // And it is attempt 1's token that is still live, not a token the
        // abandonment left behind: this is the process that is working.
        assert!(
            first.tx.borrow().is_none(),
            "handing attempt 2 back must not end attempt 1"
        );
    }

    /// **An acknowledgement lets go of the attempt whose result it is
    /// about, and of no other.**
    ///
    /// The third consequence, and the one that cost a job: `JobResultAck`
    /// carries a job id and no attempt number, so an acknowledgement for a
    /// result the hub has just written took out whatever entry stood under
    /// that id — which, once a second attempt of the same job had been
    /// assigned here, was the attempt that was *running*. It then ran with
    /// nothing counting it and nothing naming it in a health report, and
    /// the hub wrote it off for silence while the process was working
    /// perfectly.
    ///
    /// The attempt an acknowledgement is about is the one **holding an
    /// end**: that is this function's own sentence — the hub has this job's
    /// end on its disk — and a job that is running has no end for an
    /// acknowledgement to be about.
    #[tokio::test]
    async fn an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other() {
        let session = Session::new();
        let job_id = JobId::new("j0000000403");
        let _first = session
            .take_job(&job_id, 1)
            .expect("a fresh session is not draining");
        session.finish(
            &job_id,
            1,
            JobOutcome::Exited { code: 0 },
            JobMetrics::default(),
            Vec::new(),
            herd_proto::JobOutputs::default(),
            None,
        );
        assert_eq!(
            session.results_held(),
            1,
            "attempt 1 has a verdict and this runner holds it against an acknowledgement (E-44)"
        );

        // The hub wrote attempt 1 off while the link was down and has since
        // sent attempt 2 of the same job here. Taking it must not throw the
        // verdict of attempt 1 away: a result nothing acknowledged is a
        // result only this machine has (D19).
        let second = session
            .take_job(&job_id, 2)
            .expect("a fresh session is not draining");
        assert_eq!(
            session.results_held(),
            1,
            "taking attempt 2 of {job_id} destroyed the unacknowledged verdict of attempt 1"
        );

        // And now the acknowledgement arrives. It is about the verdict, so
        // it lets the verdict go — and leaves the attempt that is running
        // exactly where it is.
        session.result_acknowledged(&job_id);
        assert_eq!(
            session.results_held(),
            0,
            "the acknowledged verdict is let go of"
        );
        assert_eq!(
            session.jobs_running(),
            1,
            "attempt 2 of {job_id} is running here and an acknowledgement about attempt 1 \
             must not take it off this machine's count: {} left",
            session.jobs_running()
        );
        assert!(
            second.tx.borrow().is_none(),
            "and the running attempt's own token is untouched"
        );
    }

    /// Finding 6 of M4's verification: on the **stop** path a workspace left
    /// behind was written to the runner's stderr and nowhere a requester can
    /// read, while the job-end path put it in the job's own record.
    ///
    /// The drain's backstop ends whatever a job's own task did not, and that
    /// is where the reclaim can fail with nobody watching. Bytes left on a
    /// host nobody will collect are what invariant 2 forbids above all, so
    /// the sentence goes into the job's log, which is part of its record.
    ///
    /// The workspace is wrecked the way `root.rs` wrecks an unmeasurable
    /// tree: its directory is replaced by a *file*, so `remove_dir_all`
    /// fails with `ENOTDIR` and no mode can fix it. The pid on the live list
    /// is one that has really exited — a live pid would make the drain's kill
    /// signal a process group, and this test is not about killing anything.
    #[tokio::test]
    // The lock is held across the awaits on purpose: it exists to keep every
    // other reaper in this binary out for the whole of this test, and a lock
    // released at the first await would keep nobody out at all.
    #[allow(clippy::await_holding_lock)]
    async fn a_workspace_the_stop_leaves_behind_reaches_the_job_s_record() {
        let _reaping = testing::one_reaper_at_a_time();
        let temp = testing::TempDir::new("stop-leftover");
        let confined = Arc::new(
            Confined::prepare(Confinement::with_data_root(temp.path()))
                .expect("a throwaway data root is confinable"),
        );
        let session = Session::new();
        let job_id = JobId::new("j0000000009");
        let _cancel = session
            .take_job(&job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");

        let workspace = confined
            .root()
            .open_workspace(&job_id)
            .expect("a workspace is made");
        std::fs::remove_dir_all(workspace.base_dir()).expect("the fixture clears it");
        std::fs::write(workspace.base_dir(), b"not a directory").expect("the fixture writes");

        let mut gone = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("the fixture starts");
        let dead = gone.id();
        let _ = gone.wait();
        let token = confined.live().add(
            job_id.clone(),
            1,
            reclaim::JobTag::mint(&job_id),
            dead,
            None,
            workspace.clone(),
        );

        let config = Config {
            drain_grace: Duration::from_millis(50),
            ..Config::default()
        };
        drain(&config, &session, &confined, "the owner asked it to stop").await;
        confined.live().remove(token);

        let record = session.held_output_of(&job_id, 1);
        assert!(
            record.contains("could not be reclaimed") && record.contains("left on this host"),
            "a workspace the stop left behind must be in the job's own record, not \
             only on the runner's stderr: {record:?}"
        );
        assert!(
            record.contains(&workspace.base_dir().display().to_string()),
            "and it must name the path that is still there: {record:?}"
        );
        // The fixture's file, so the temporary directory can be removed.
        let _ = std::fs::remove_file(workspace.base_dir());
    }

    /// The give-up, which is the part of the policy that must not be silent.
    ///
    /// A runner pointed at a hub that will never answer has to stop and say
    /// so, naming the hub: the alternative is a process that dials a typo for
    /// ever. The bound is a day in the deployment, so the behaviour is proved
    /// with the bound set to nothing — the question is whether the give-up
    /// happens and what it says, not how long the wait is.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // See above: the lock is the point.
    async fn a_runner_gives_up_on_a_hub_that_will_never_answer_and_says_so() {
        let _reaping = testing::one_reaper_at_a_time();
        let temp = testing::TempDir::new("give-up");
        let confined = Arc::new(
            Confined::prepare(Confinement::with_data_root(temp.path()))
                .expect("a throwaway data root is confinable"),
        );
        let config = Config {
            // Port 1 on loopback: nothing of ours is ever there, and a dial
            // fails at once rather than hanging.
            hub_url: "ws://127.0.0.1:1".to_owned(),
            redial: Backoff {
                first: Duration::from_millis(1),
                cap: Duration::from_millis(1),
                give_up_after: Duration::ZERO,
            },
            ..Config::default()
        };
        let error = tokio::time::timeout(
            Duration::from_secs(10),
            serve_forever(config, Session::new(), confined),
        )
        .await
        .expect("a runner with no budget left must not go on dialling")
        .expect_err("a hub that cannot be reached is an error, not a silence");
        let said = format!("{error:#}");
        assert!(said.contains("ws://127.0.0.1:1"), "{said}");
        assert!(said.contains("giving up"), "{said}");
    }

    /// M3 noted that a runner whose data root cannot record which hub state
    /// root it belongs to stands by until it can. It does not — it says so
    /// loudly and goes on serving — and that is the better behaviour, so the
    /// note was wrong and not the code. A runner with an unwritable data
    /// root is still a machine that can build; what it loses is only the
    /// ability to attribute a verdict it spools, and because the identity it
    /// then announces is `None`, which is never a hub's root, everything it
    /// holds is set aside rather than believed.
    #[test]
    fn a_data_root_that_cannot_record_the_hub_s_state_root_says_so_and_carries_on() {
        let temp = crate::testing::TempDir::new("state-root-unwritable");
        let root = root::DataRoot::open(temp.path()).expect("a throwaway root");
        // The one way this fails on a root the runner would not have refused
        // outright: the file's own name is taken by something that is not a
        // file (a read-only root is refused at startup, naming it).
        std::fs::create_dir(temp.path().join("hub-state-root"))
            .expect("the fixture takes the name");

        let said = Session::new()
            .settle_against_state_root(&root, "root-0123456789abcdef")
            .join("\n");
        assert!(
            said.contains("COULD NOT write down which hub state root"),
            "the failure is loud, and that is the whole of the mitigation: {said}"
        );
        assert!(
            said.contains("goes on serving"),
            "and it says what it is doing about it: {said}"
        );
        assert_eq!(
            root.hub_state_root(),
            None,
            "so the next dial announces no root at all, and is admitted"
        );
    }

    /// A collection, on the disk and in the session, ready to be pumped.
    ///
    /// Small on purpose: these tests are about the **window**, not about
    /// moving bytes, and a one-chunk collection exercises every edge of the
    /// window that a four-hundred-megabyte one does.
    fn a_collection_to_send(
        session: &Session,
        job_id: &JobId,
        spool: &Path,
    ) -> herd_proto::JobOutputs {
        let bytes = b"the one file of a streamed collection".to_vec();
        std::fs::create_dir_all(spool).expect("a spool directory");
        std::fs::write(spool.join("results.json"), &bytes).expect("the spooled file");
        let _cancel = session
            .take_job(job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");
        herd_proto::JobOutputs {
            files: vec![herd_proto::OutputFile {
                path: "results.json".to_owned(),
                len: bytes.len() as u64,
                sha256: herd_proto::sha256_hex(&bytes),
                data: None,
            }],
            streamed: true,
            ..Default::default()
        }
    }

    /// **The outage of 2026-10-19, in one assertion** (D158).
    ///
    /// Every chunk of a collection takes a permit out of the link's window
    /// and [`write_frames`] puts it back when the chunk is on the wire. The
    /// pump also took one for the acquisition that *finds the collection
    /// finished*, and nothing ever gave that one back — so each completed
    /// collection shrank the window by one, the eighth left it at zero, and
    /// the ninth waited for room for ever while the hub held a manifest with
    /// no files behind it.
    ///
    /// Asserted on the window itself rather than on a ninth transfer,
    /// because the window is the invariant: **a collection that has gone
    /// through gives back everything it borrowed.** A test written as "nine
    /// collections still work" would pass again the day the window is
    /// widened to nine.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_finished_collection_gives_the_whole_window_back() {
        let temp = crate::testing::TempDir::new("window-returned");
        let session = Session::new();
        let credit = Arc::new(Semaphore::new(FILES_IN_FLIGHT));
        let (outbox, mut inbox) = mpsc::unbounded_channel();
        session.attach(outbox, Arc::clone(&credit), Duration::from_millis(50));

        for number in 1..=3 {
            let job_id = JobId::new(format!("j000000021{number}"));
            let spool = temp.path().join(format!("spool-{number}"));
            let outputs = a_collection_to_send(&session, &job_id, &spool);
            session.finish(
                &job_id,
                1,
                JobOutcome::Exited { code: 0 },
                JobMetrics::default(),
                Vec::new(),
                outputs,
                Some(spool),
            );
            // Stand in for `write_frames`: drain the link and give a permit
            // back for every chunk that really reached it, which is the
            // contract the pump is written against.
            let mut chunks = 0;
            while chunks == 0 {
                while let Ok(message) = inbox.try_recv() {
                    if matches!(message, ToHub::JobFileChunk { .. }) {
                        chunks += 1;
                        credit.add_permits(1);
                    }
                }
                tokio::task::yield_now().await;
            }
            // The hub has it all, so the pump finds nothing more to send and
            // ends — which is the moment the permit used to disappear.
            session.files_acknowledged(&job_id, 1);
            let deadline = Instant::now() + Duration::from_secs(5);
            while credit.available_permits() < FILES_IN_FLIGHT {
                assert!(
                    Instant::now() < deadline,
                    "collection {number} left the window at {} of {FILES_IN_FLIGHT} permits: \
                     a window that shrinks per collection runs out, and the collection it \
                     runs out on waits for ever",
                    credit.available_permits()
                );
                tokio::task::yield_now().await;
            }
        }
    }

    /// A collection that cannot get room on the link says so, by name, once
    /// (D158).
    ///
    /// The silence was half the defect: the runner waited, the hub had
    /// nothing to report because nothing arrived, and the requester was told
    /// only that it did not get its files. One line, from the one party that
    /// knows why.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_collection_with_no_room_on_the_link_is_not_silent_about_it() {
        let temp = crate::testing::TempDir::new("window-stalled");
        let session = Session::new();
        let job_id = JobId::new("j0000000214");
        let spool = temp.path().join("spool");
        let outputs = a_collection_to_send(&session, &job_id, &spool);
        // A link whose window has no room in it at all, which is what eight
        // leaked permits left behind.
        let (outbox, _inbox) = mpsc::unbounded_channel();
        session.attach(
            outbox,
            Arc::new(Semaphore::new(0)),
            Duration::from_millis(50),
        );

        let (_, said) = crate::testing::stderr_lines_about("j0000000214", || {
            session.finish(
                &job_id,
                1,
                JobOutcome::Exited { code: 0 },
                JobMetrics::default(),
                Vec::new(),
                outputs,
                Some(spool.clone()),
            );
            // The pump is on another worker; this thread waits out several
            // times the bound so that a line which is never written is a
            // failure and not a race.
            std::thread::sleep(Duration::from_millis(600));
        });
        assert_eq!(
            said.len(),
            1,
            "a stalled collection says so exactly once, not never and not per chunk: {said:#?}"
        );
        assert!(
            said[0].contains("room on the link"),
            "and the line says what it is waiting for: {}",
            said[0]
        );
    }

    /// The sentence a reader gets when a spool will not go away, and the one
    /// fact the `errno` does not carry (D158).
    #[test]
    fn a_directory_the_runner_does_not_own_is_named_as_why_a_spool_stays() {
        let temp = crate::testing::TempDir::new("foreign-spool");
        let spool = temp.path().join("j0000000211-1");
        std::fs::create_dir_all(spool.join("packs/abc/objects")).expect("a spool tree");
        std::fs::write(spool.join("packs/abc/objects/one"), b"an object").expect("an object");

        let mine = crate::privilege::effective_uid().expect("a unix uid");
        assert_eq!(
            foreign_directory_under(&spool, mine),
            None,
            "a tree this runner made itself is one it can empty, so there is nothing to say"
        );
        // The real shape, which cannot be built in a test without root: a
        // directory somebody else owns. Asked from the point of view of
        // another uid, every one of these is foreign, and the walk has to
        // find the shallowest.
        let (named, owner) = foreign_directory_under(&spool, mine.wrapping_add(1))
            .expect("a directory this uid does not own");
        assert_eq!(owner, mine);
        assert_eq!(
            named,
            spool.join("packs"),
            "the shallowest one, because that is the one an operator fixes"
        );
    }
}
