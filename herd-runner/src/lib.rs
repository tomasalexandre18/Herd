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

pub mod cgroup;
pub mod confine;
pub mod job;
pub mod metrics;
pub mod privilege;
pub mod reclaim;
pub mod root;
pub mod size;
pub mod spool;
#[doc(hidden)]
pub mod testing;

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, LogStream, Role, RunningJob,
    ToHub, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub use confine::{Confined, Confinement};
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
}

/// One job this runner has in hand, and everything the hub may still need
/// from it.
#[derive(Debug)]
struct JobInHand {
    attempt: u32,
    /// The number the next chunk of this job's output will carry. Per
    /// attempt: a second attempt is a second process and counts from zero.
    next_seq: u64,
    /// Chunks sent and not yet acknowledged, oldest first.
    unacked: VecDeque<(u64, LogStream, Vec<u8>)>,
    unacked_bytes: usize,
    /// How the job ended, once it has, until the hub acknowledges it (E-44).
    end: Option<Ended>,
    cancel: CancelToken,
}

impl JobInHand {
    fn remember(&mut self, seq: u64, stream: LogStream, data: &[u8]) {
        self.unacked_bytes += data.len();
        self.unacked.push_back((seq, stream, data.to_vec()));
        while self.unacked_bytes > HELD_BYTES_PER_JOB {
            match self.unacked.pop_front() {
                Some((dropped, _, bytes)) => {
                    self.unacked_bytes -= bytes.len();
                    eprintln!(
                        "herd-runner: chunk {dropped} is no longer held: this runner holds \
                         {HELD_BYTES_PER_JOB} bytes of a job's output against an \
                         acknowledgement and no more"
                    );
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
    jobs: HashMap<JobId, JobInHand>,
    /// Verdicts a previous life of this runner wrote to the data root and
    /// nobody has acknowledged yet. Not jobs: nothing of them is running, and
    /// nothing of them is counted as work this runner is doing.
    spooled: Vec<Spooled>,
    /// The runner is stopping: it takes no new job (invariant 2).
    draining: bool,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            instance: mint_an_instance_id(),
            link: None,
            jobs: HashMap::new(),
            spooled: Vec::new(),
            draining: false,
        }
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
    fn held_output_of(&self, job_id: &JobId) -> String {
        self.with(|inner| {
            inner
                .jobs
                .get(job_id)
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
    /// `None` when this runner will not run it: it is draining and the hub
    /// had already sent the assignment, which the caller reports as an
    /// abandoned attempt.
    pub(crate) fn take_job(&self, job_id: &JobId, attempt: u32) -> Option<CancelToken> {
        self.with(|inner| {
            if inner.draining {
                return None;
            }
            let cancel = CancelToken::new();
            inner.jobs.insert(
                job_id.clone(),
                JobInHand {
                    attempt,
                    next_seq: 0,
                    unacked: VecDeque::new(),
                    unacked_bytes: 0,
                    end: None,
                    cancel: cancel.clone(),
                },
            );
            Some(cancel)
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
    pub(crate) fn send_chunk(&self, job_id: &JobId, stream: LogStream, data: Vec<u8>) {
        self.with(|inner| {
            let Some(job) = inner.jobs.get_mut(job_id) else {
                return;
            };
            let seq = job.next_seq;
            job.next_seq += 1;
            job.remember(seq, stream, &data);
            let attempt = job.attempt;
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
    pub(crate) fn finish(&self, job_id: &JobId, outcome: JobOutcome, metrics: JobMetrics) {
        self.with(|inner| {
            let Some(job) = inner.jobs.get_mut(job_id) else {
                return;
            };
            let end = Ended { outcome, metrics };
            job.end = Some(end.clone());
            let (attempt, chunks_sent) = (job.attempt, job.next_seq);
            if !send_on(
                &inner.link,
                finished_frame(job_id, attempt, chunks_sent, &end),
            ) {
                eprintln!(
                    "herd-runner: {job_id} ended while there was no hub to tell; \
                     its result is held until a hub acknowledges it"
                );
            }
        });
    }

    /// Give an attempt back without a verdict, and let the job go.
    ///
    /// Nothing is held afterwards: the runner is not keeping a result, it is
    /// saying there is none. The hub decides what becomes of the job.
    pub(crate) fn abandon(&self, job_id: &JobId, attempt: u32, reason: &str) {
        self.with(|inner| {
            inner.jobs.remove(job_id);
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

    /// The hub has this job's end on its disk: let the job go, output and all,
    /// and remove the spool file if the verdict came from one.
    fn release(&self, job_id: &JobId) {
        let delivered: Vec<Spooled> = self.with(|inner| {
            inner.jobs.remove(job_id);
            take_spooled(inner, |spooled| spooled.job_id() == job_id)
        });
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
            if let Some(job) = inner.jobs.get_mut(job_id) {
                if job.attempt == attempt {
                    job.forget_through(through_seq);
                }
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
            let Some(job) = inner.jobs.get(job_id) else {
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
            if job.attempt != attempt {
                return;
            }
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
    fn end_job(&self, job_id: &JobId, attempt: Option<u32>, ending: Ending) -> bool {
        self.with(|inner| {
            let Some(job) = inner.jobs.get(job_id) else {
                return false;
            };
            if attempt.is_some_and(|attempt| attempt != job.attempt) {
                return false;
            }
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
    fn drop_stale(&self, job_id: &JobId, attempt: u32, reason: &str) {
        let running = self.with(|inner| {
            let Some(job) = inner.jobs.get(job_id) else {
                return false;
            };
            if job.attempt != attempt {
                return false;
            }
            job.cancel
                .end(Ending::Abandoned(format!("stale attempt: {reason}")));
            job.end.is_none()
        });
        eprintln!(
            "herd-runner: attempt {attempt} of {job_id} is over as far as the hub is \
             concerned ({reason}); nothing of it is sent again"
        );
        self.spool_refused(job_id, attempt, reason);
        // A job still running is left to notice the ending and clean up after
        // itself; one that had already finished is dropped here and now,
        // result included — it is not this job's attempt any more.
        if !running {
            self.release(job_id);
        }
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

    /// Everything the hub is told at the start of a connection, in order:
    /// output it may be missing, then the results it has not acknowledged,
    /// then the verdicts a previous life of this runner left on the disk.
    ///
    /// Done while holding the lock a new chunk would need, so that nothing a
    /// running job produces can slip in front of what is being sent again. A
    /// log that keeps its order across a reconnect is the whole point.
    fn attach(&self, outbox: Outbox) {
        self.with(|inner| {
            inner.link = Some(outbox);
            let ids: Vec<JobId> = inner.jobs.keys().cloned().collect();
            for job_id in ids {
                let Some(job) = inner.jobs.get(&job_id) else {
                    continue;
                };
                let attempt = job.attempt;
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
        self.with(|inner| inner.link = None);
    }

    /// What this runner is doing, as a health report says it (E-40).
    fn health(&self) -> ToHub {
        self.with(|inner| ToHub::Health {
            jobs: running_jobs(inner),
            draining: inner.draining,
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
            for (job_id, job) in inner.jobs.iter() {
                if job.end.is_none() {
                    job.cancel.end(Ending::Abandoned(reason.to_owned()));
                    ending.push(job_id.clone());
                }
            }
            // So that the hub stops handing out work to a runner on its way
            // out at once, rather than at the next report.
            let health = ToHub::Health {
                jobs: running_jobs(inner),
                draining: true,
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
            for (job_id, job) in inner.jobs.iter() {
                match &job.end {
                    Some(end) => in_hand.push((
                        job_id.clone(),
                        end.outcome.clone(),
                        finished_frame(job_id, job.attempt, job.next_seq, end),
                    )),
                    None => {
                        job.cancel.end(Ending::Abandoned(why.clone()));
                        running.push(job_id.clone());
                    }
                }
            }
            for (job_id, _, _) in &in_hand {
                inner.jobs.remove(job_id);
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
                 {error}. A verdict spooled here would not be attributable to {hub_root}"
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
                .filter_map(|(job_id, job)| {
                    job.end.as_ref().map(|end| {
                        (
                            job_id.clone(),
                            end.outcome.clone(),
                            finished_frame(job_id, job.attempt, job.next_seq, end),
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
        .map(|(job_id, job)| RunningJob {
            job_id: job_id.clone(),
            attempt: job.attempt,
            chunks_sent: job.next_seq,
        })
        .collect()
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

    let session = Session::new();
    // Before the first dial: whatever a previous life of this runner wrote
    // down and nobody acknowledged. A verdict on a disk is only saved once it
    // reaches the hub, and no human belongs in that loop (D34).
    for line in session.pick_up(confined.root()) {
        eprintln!("herd-runner: {line}");
    }

    let serving = serve_forever(config.clone(), session.clone(), Arc::clone(&confined));
    let stopping = async {
        let reason = stop.await;
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
async fn serve_forever(config: Config, session: Session, confined: Arc<Confined>) -> Result<()> {
    let mut failures = 0u32;
    let mut failing_since: Option<Instant> = None;
    loop {
        let delay = match serve_once(config.clone(), session.clone(), Arc::clone(&confined)).await {
            // The hub closed the connection. Nothing says the runner is done,
            // so it waits and joins the hub that comes back.
            Ok(()) => {
                failures = 0;
                failing_since = None;
                config.redial.delay(1)
            }
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

/// Join the hub and serve it until this one connection ends.
///
/// Public because a test needs one connection it can watch, and because
/// [`run`] is only this in a loop. The confinement is the caller's, not this
/// connection's: the data root is locked for the life of the process, so one
/// `Confined` serves every connection a runner ever makes.
pub async fn serve_once(config: Config, session: Session, confined: Arc<Confined>) -> Result<()> {
    let (socket, _response) = tokio_tungstenite::connect_async(&config.hub_url)
        .await
        .with_context(|| format!("dialling {}", config.hub_url))?;
    let (mut sink, mut source) = socket.split();

    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        role: Role::Runner,
        token: config.token.clone(),
        name: config.name.clone(),
        // Which process this is, so the hub can tell this runner dialling
        // again from a second runner wearing its name.
        instance: Some(session.instance()),
        // And which hub state the durable things under this data root belong
        // to, so that nothing spooled for a hub that no longer exists is
        // believed by the hub that replaced it.
        state_root: confined.root().hub_state_root(),
    };
    sink.send(Message::text(to_frame(&hello).context("encoding hello")?))
        .await
        .context("sending hello")?;

    match next_message(&mut source, config.hub_silence_limit).await? {
        Some(FromHub::Welcome { state_root, .. }) => {
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
    let mut writer = tokio::spawn(write_frames(sink, inbox));
    // Before anything else on this connection: what the last one could not
    // deliver. A job finished while the hub was away is still a job that ran,
    // and output the hub never wrote is still output the job produced.
    session.attach(outbox);
    let reporting = tokio::spawn(report_health(session.clone(), config.health_every));
    let outcome = serve_hub(&session, &mut source, confined, config.hub_silence_limit).await;
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
    session: &Session,
    source: &mut Source,
    confined: Arc<Confined>,
    silence_limit: Duration,
) -> Result<()> {
    while let Some(message) = next_message(source, silence_limit).await? {
        match message {
            FromHub::AssignJob {
                job_id,
                attempt,
                command,
            } => {
                let session = session.clone();
                let confined = Arc::clone(&confined);
                // One task per job: a long job must not stop the runner from
                // hearing the hub, and must not end when the link does.
                tokio::spawn(async move {
                    job::execute(job_id, attempt, command, session, confined).await
                });
            }
            FromHub::JobResultAck { job_id } => session.release(&job_id),
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
                if session.end_job(&job_id, Some(attempt), Ending::Cancelled(reason.clone())) {
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
            } => session.drop_stale(&job_id, attempt, &reason),
            FromHub::HealthRecorded {} => {}
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

async fn write_frames(mut sink: Sink, mut inbox: UnboundedReceiver<ToHub>) {
    while let Some(message) = inbox.recv().await {
        match to_frame(&message) {
            Ok(frame) => {
                if sink.send(Message::text(frame)).await.is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("herd-runner: a message would not encode: {error}"),
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
                let message = from_frame(text.as_str())
                    .map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
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
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
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
            attempt: 1,
            next_seq: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            end: None,
            cancel: CancelToken::new(),
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
            attempt: 1,
            next_seq: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            end: None,
            cancel: CancelToken::new(),
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

    #[tokio::test]
    async fn a_draining_runner_takes_no_new_job() {
        let session = Session::new();
        session.start_draining("the owner asked");
        assert!(session.draining());
        assert!(
            session.take_job(&JobId::new("j1"), 1).is_none(),
            "a draining runner must refuse a job the hub had already sent"
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
            .take_job(&job_id, 1)
            .expect("a fresh session is not draining");

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

        let record = session.held_output_of(&job_id);
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
}
