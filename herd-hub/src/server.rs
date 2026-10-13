//! The hub: one listener, one runner, any number of requesters.
//!
//! Every connection is dialled by the peer (invariant 3) and starts with a
//! `Hello` whose protocol version must match. A requester's `SubmitJob` is
//! minted into a job, handed to the connected runner, and everything the
//! runner says about that job is relayed back to the requester that submitted
//! it. There is no scheduling yet: with no runner connected, a submission is
//! refused by name (M6 gives the hub a queue to put it in instead).
//!
//! Everything the hub knows is on its disk before it is acted on, through the
//! [`JobStore`] it was given. Two consequences run through this file. A result
//! is acknowledged to the runner only once it is recorded, because the runner
//! is holding it until then (E-44). And a job that is still queued when a
//! runner joins is handed to it, whether that job was submitted a second ago
//! or inherited from the hub that ran before this one.
//!
//! **M3 adds the part that deals with a runner which is not there.** A runner
//! says it is alive about every 15 s (E-40) and the hub keeps the age of that
//! report; a runner nothing has been heard from for about 60 s is lost, and
//! the jobs it was running are attempted once more (E-21). Three rules make
//! that safe rather than merely automatic:
//!
//! - **An attempt is named, numbered, and somebody's.** Every job frame
//!   carries the attempt it is about, so a runner that comes back from the
//!   dead cannot finish a job the hub has already written off, and cannot
//!   write into the record of the attempt that replaced it. And an attempt
//!   belongs to the runner it was handed to: a frame about it from any other
//!   runner is refused by name, because a number alone cannot tell a verdict
//!   from a verdict about another machine's work. The handshake carries the
//!   state root's identity for the same reason one step up — a job id means
//!   something only inside one hub's state — and a runner instance identity,
//!   so a redial can be told from a second runner wearing the same name.
//! - **Once more means once.** A job is requeued one time. A second loss is an
//!   end — `lost` — not a third attempt, because a job that kills runners
//!   would otherwise go round for ever.
//! - **A record that is over is never written to again.** Its outcome, its
//!   measurements and its log are closed together, and the attempt that
//!   produced the verdict is ended with the job — so a frame that arrives
//!   afterwards, *even from the runner the attempt was handed to*, is refused
//!   by name and reaches no writer. The rule is the store's single write
//!   entry point rather than a guard per function, because a rule enforced
//!   once per function is a rule the next function is missing.
//! - **Nothing is destroyed to make this tidy.** A result the hub cannot write
//!   is retried for as long as it takes and never dropped (D19); output the
//!   hub is missing is asked for again before the log is declared short (D29).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    a_frame_about_a_job_that_is_over_reason, foreign_state_root_reason, from_frame,
    not_this_runner_s_attempt_reason, runner_twin_reason, to_frame, unreadable_handshake_reason,
    version_mismatch_reason, CapabilityState, FromHub, Holdings, HostSize, JobId, JobMetrics,
    JobOutcome, LabelSet, Priority, Role, RunningJob, ToHub, HEALTH_EVERY, PROTOCOL_VERSION,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::capability::RunnerCapacity;
use crate::job::{JobRecord, JobState, JobStore, LogWrite, NewJob, Raise, Submission, Timestamp};
use crate::matcher::{self, Placement, Seat, Stickiness};
use crate::queue;

/// Loopback by default: the hub is reached through the reverse proxy, which
/// terminates TLS; the hub itself only ever speaks plain `ws://` (D4).
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8787";

/// How long a runner may be unheard from before it is lost (E-21).
pub const LOST_AFTER: Duration = Duration::from_secs(60);

/// The largest first frame this hub accepts, in bytes: **1 MiB**.
///
/// A figure of this project's own, named in the refusal a peer gets, rather
/// than whatever the WebSocket library happens to default to — which is where
/// "a 32 MiB frame gets a TCP reset" came from: the library's limit, the
/// library's error, and nobody to explain either.
///
/// 1 MiB is two orders of magnitude above anything a peer legitimately sends
/// *to* a hub. The largest such frame is a runner's `LogChunk`, which carries
/// 8 KiB of a job's output base64'd; the next largest is a `Hello` whose
/// capability offer would have to run to some ten thousand names of at most 64
/// characters to reach this. The hub's own outgoing frames are not bound by it
/// — a `JobLog` carries a whole job's log at once, up to the runner's log cap
/// — and need not be: this is a bound on what an unknown peer may ask of this
/// hub before it has said who it is.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// The point past which this hub stops reading a frame at all: **64 MiB**.
///
/// Two figures, because a refusal has to be *deliverable*. A peer past
/// [`MAX_FRAME_BYTES`] is told so in a sentence, which means the frame has to
/// be read to its end first: a hub that stops reading mid-frame and closes
/// leaves unread bytes in the socket, the close becomes a TCP reset, and the
/// reset throws away the very refusal it was sending. Reading to the end and
/// then refusing is what makes the sentence arrive.
///
/// That cannot be unbounded, so this is where it stops — and past it the peer
/// does get a reset, which is the honest answer to something shovelling 64 MiB
/// at a hub that has told it nothing. It is the WebSocket library's own
/// default for a whole message, so no exposure is being widened; what changes
/// is that a *single frame* that big is read rather than cut off at the
/// library's 16 MiB, so the mistake can be answered instead of dropped.
pub const FRAME_CEILING_BYTES: usize = 64 << 20;

/// How many goes at running one job the hub allows (E-21).
///
/// Two: the attempt, and one more after a loss. The second loss is an end,
/// because the alternative is a job that destroys every runner it touches
/// going round the herd for ever. M6's priorities do not change this; a third
/// attempt would be a different promise and nobody has asked for one.
pub const ATTEMPTS_ALLOWED: u32 = 2;

/// How often a job that is still waiting says so again (E-04).
///
/// Sixty seconds, because that is the figure E-04 names: "at least every
/// 60 s". A setting rather than a constant — `--wait-restatement`, printed at
/// startup — for the reason the stickiness limit is one: a test has to be
/// able to make ten minutes of waiting pass without waiting ten minutes, and
/// through the hub's own code rather than a copy of it.
pub const WAIT_RESTATEMENT: Duration = Duration::from_secs(60);

/// E-04's cadence, as a decision rather than a sleep.
///
/// The arithmetic of "at least every 60 s" is one comparison, and it is
/// pulled out of the loop that uses it for the reason the queue's aging is
/// pulled out of its own: a test has to be able to drive ten minutes of
/// waiting through **the hub's own decision**, counting the lines it would
/// have produced, rather than through a copy of it or a ten-minute sleep.
/// **The cadence is a deadline, not an interval**, and that distinction was
/// found by measuring. A loop that wakes every `tick` and fires when a job
/// was last told `cadence` ago produces gaps of up to `cadence + tick`: the
/// first by-hand ten-minute wait measured gaps of 45 s, 60 s and **61 s**
/// against a figure E-04 words as "at least every 60 s". One second over is
/// still over. So the decision fires a tick *early* — the interval lands in
/// `[cadence - tick, cadence]` and the worst gap is the cadence itself.
#[derive(Debug, Clone, Copy)]
pub struct Restating {
    pub cadence: Duration,
    /// How often the loop that uses this wakes up. Kept here because the
    /// decision has to know it: see the type's own documentation.
    pub tick: Duration,
}

impl Restating {
    /// The cadence an operator gave, with the tick that serves it.
    ///
    /// A quarter of the cadence, never under 50 ms: often enough that the
    /// deadline is met with a quarter of it to spare, rarely enough that a
    /// hub with nothing queued does not spin.
    pub fn every(cadence: Duration) -> Self {
        Self {
            cadence,
            tick: (cadence / 4).max(Duration::from_millis(50)),
        }
    }

    /// Whether a job last told `since` ago is due to be told again.
    ///
    /// `None` is a job nobody has been told about yet, which is always due:
    /// a requester that attaches to a job already waiting gets the reason at
    /// once rather than up to a cadence later.
    pub fn is_due(&self, since: Option<Duration>) -> bool {
        since.is_none_or(|since| since + self.tick >= self.cadence)
    }

    /// The most lines a follower can get out of **this clock** over a wait of
    /// `wait`.
    ///
    /// The bound E-04's flooding half needs stated rather than hoped for, and
    /// the figure a test asserts against: one at the start, and one per
    /// shortest interval the clock can produce after it. M6's news path adds
    /// to this when the sentence genuinely changes, which the sentence's own
    /// minute-rounded figures bound at about one a minute (D91).
    pub fn at_most_over(&self, wait: Duration) -> u32 {
        let shortest = self.cadence.saturating_sub(self.tick);
        if shortest.is_zero() {
            return u32::MAX;
        }
        u32::try_from(wait.as_nanos() / shortest.as_nanos()).unwrap_or(u32::MAX - 1) + 1
    }
}

/// The hub's own version, reported in `Welcome`.
/// How many goes a job gets at a capability that turned out to be taken
/// (D76). See [`Policy::requeue_budget`].
pub const REQUEUE_BUDGET: u32 = 3;

pub fn hub_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

type Socket = WebSocketStream<TcpStream>;
type Sink = SplitSink<Socket, Message>;
type Source = SplitStream<Socket>;

/// Queue of frames waiting to go out on one connection. Unbounded so that a
/// relay never blocks the task that reads from the runner.
type Outbox = UnboundedSender<FromHub>;

/// The hub's timing, in one place, so a test can make an hour pass in a
/// second and an operator can tune one number.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// A runner unheard from for this long is lost, and the jobs it was
    /// running are attempted once more (E-21).
    pub lost_after: Duration,
    /// The cadence this hub expects its runners to report at (E-40).
    ///
    /// The hub sends no health report of its own, so this is a declaration
    /// rather than a schedule — and it is load-bearing, because
    /// [`Policy::lost_after`] is only meaningful as a multiple of it. A
    /// threshold shorter than a few reports does not detect a lost runner, it
    /// detects the gap between two reports the hub asked for, and it spends a
    /// live job's attempts on that (see [`Policy::refusal`]).
    pub health_every: Duration,
    /// How many goes one job gets.
    pub attempts_allowed: u32,
    /// The first delay between two tries at writing a result the hub could
    /// not write, before the growth starts (D19).
    pub write_retry_first: Duration,
    /// The longest that delay ever gets. There is no give-up: a result is
    /// never destroyed, so the hub goes on trying.
    pub write_retry_cap: Duration,
    /// How long a queued job waits before its effective level rises one step
    /// (E-18). See [`crate::queue::AGING_STEP`].
    ///
    /// The bound it gives is `aging_step` times the number of levels between
    /// a job and the top: at the default, ninety minutes for the lowest. It
    /// is here, in the hub's one timing struct, for exactly the reason the
    /// struct exists — so that a test can make ninety minutes pass in a
    /// second *through the code the hub runs*, and an operator can tune one
    /// number.
    pub aging_step: Duration,
    /// How long a job waits for the runner it is sticking to before the hub
    /// places it on a less preferred one (E-19).
    ///
    /// E-19 asks for this limit to be a **visible setting**, so it is one:
    /// `--stickiness-limit`, three hours by default, printed when the hub
    /// starts, named in the sentence of every job waiting on it.
    pub stickiness: Stickiness,
    /// How often a job that is still waiting says so again, to whoever is
    /// following it (E-04).
    ///
    /// E-04 asks for "at least every 60 s", which has two failure modes and
    /// this figure bounds both. Too rare and a ten-minute wait is silence; a
    /// line per dispatch pass and a job waiting on one lock file fills a
    /// transcript with hundreds of identical sentences — M7 alone added
    /// several passes per second to a busy fleet. So the cadence is a figure
    /// rather than an event: a following requester is told again no oftener
    /// than this and no rarer, whatever the fleet is doing in between, and
    /// the arithmetic of the bound is `wait / restatement + 1`.
    ///
    /// It is deliberately *not* the dispatch pass's cadence. The pass is
    /// where the reason is **computed**, and it may run twenty times a minute
    /// or once an hour; this is where it is **said**.
    pub wait_restatement: Duration,
    /// How long a job may hold a host empty waiting for it to go quiet
    /// (E-27).
    ///
    /// Fifteen minutes, the figure E-27 names, and a setting for the reason
    /// the stickiness limit is one: an operator has to be able to see it, and
    /// a test has to be able to make it pass without waiting a quarter of an
    /// hour. Named in the sentence of every job that is waiting on it.
    pub drain_first_limit: Duration,
    /// How many goes a job gets at a capability that turned out to be taken
    /// (D76).
    ///
    /// Three, and **separate from [`Policy::attempts_allowed`]**, which is
    /// E-21's budget for a runner that disappears. Three because the race
    /// this bounds is small and genuine — the lease was free when the probe
    /// was read and taken by the time the job started — so one retry is
    /// usually enough and a second machine is worth trying; and bounded
    /// because a lease somebody else holds all afternoon would otherwise
    /// requeue a job for ever, which is a job nobody is told about.
    pub requeue_budget: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            lost_after: LOST_AFTER,
            health_every: HEALTH_EVERY,
            attempts_allowed: ATTEMPTS_ALLOWED,
            write_retry_first: Duration::from_millis(250),
            write_retry_cap: Duration::from_secs(5 * 60),
            aging_step: crate::queue::AGING_STEP,
            stickiness: Stickiness::default(),
            wait_restatement: WAIT_RESTATEMENT,
            drain_first_limit: crate::matcher::DRAIN_FIRST_LIMIT,
            requeue_budget: REQUEUE_BUDGET,
        }
    }
}

impl Policy {
    /// Why this timing cannot be served, if it cannot.
    ///
    /// One configuration is not merely unwise but incapable of meaning what it
    /// says: a silence threshold shorter than a few health reports. `--lost
    /// -after 8` against the 15 s cadence E-40 asks for burnt **both**
    /// attempts of a perfectly healthy job in nineteen seconds and ended it
    /// `lost` — the hub was timing the interval between two reports it had
    /// itself asked for, and D39's two attempts are spent before a job can
    /// finish. Refused here, at startup, naming both figures and the margin,
    /// because the alternative is finding it in a job's record.
    pub fn refusal(&self) -> Option<String> {
        // A restatement cadence of zero is the flooding half of E-04: every
        // turn of the loop would be due, so a requester following one job
        // waiting on one lock file would be written to as fast as the hub can
        // run. Refused at startup rather than found in a transcript.
        if self.wait_restatement.is_zero() {
            return Some(
                "--wait-restatement 0 would say why a job is waiting as fast as this hub can \
                 loop, which is not a cadence. E-04 asks for at least every 60 s; give that, \
                 or a shorter figure a test can make ten minutes pass under."
                    .to_owned(),
            );
        }
        herd_proto::silence_threshold_refusal(
            "the hub's silence threshold (--lost-after)",
            self.lost_after,
            "the health cadence it expects of its runners (--health-every)",
            self.health_every,
        )
    }

    /// How often the hub looks for a runner that has gone quiet. Often enough
    /// that the loss is noticed near the threshold rather than a threshold
    /// later, and never so often that a hub with nothing to do spins.
    fn watchdog_every(&self) -> Duration {
        (self.lost_after / 20).clamp(Duration::from_millis(50), Duration::from_secs(1))
    }

    /// How long a runner told to stand by waits before dialling again.
    fn stand_by_retry(&self) -> Duration {
        (self.lost_after / 20).clamp(Duration::from_millis(200), Duration::from_secs(5))
    }

    /// How long a connected runner's last health report may be before the
    /// hub treats the connection as one it may replace with a new one from a
    /// runner that is plainly alive.
    fn health_is_stale_after(&self) -> Duration {
        self.lost_after / 2
    }
}

/// A bound hub, not yet serving.
pub struct Hub {
    listener: TcpListener,
    state: Arc<State>,
}

impl Hub {
    /// Bind the listener. Port 0 binds a free port; ask [`Hub::local_addr`]
    /// which one it got.
    pub async fn bind(addr: SocketAddr, store: Arc<dyn JobStore>) -> Result<Self> {
        Self::bind_with_policy(addr, store, Policy::default()).await
    }

    /// The same, with the timing given: what a test and the binary's options
    /// both go through.
    pub async fn bind_with_policy(
        addr: SocketAddr,
        store: Arc<dyn JobStore>,
        policy: Policy,
    ) -> Result<Self> {
        // Before the port, because a hub that cannot keep its promises about
        // a live runner must not be reachable by one. Here rather than in the
        // binary's option parsing so that there is no way to build a hub on
        // this timing at all — a test is as able to get it wrong as an
        // operator, and the test that would prove it is the one that looks
        // right while the job underneath it is being declared lost.
        if let Some(reason) = policy.refusal() {
            bail!("{reason}");
        }
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        Ok(Self {
            listener,
            state: Arc::new(State {
                store,
                policy,
                // The clock on every attempt this hub inherited starts here: a
                // job left `running` by the hub that died is a job this hub
                // has had no word of since it began.
                started: Instant::now(),
                peers: Mutex::new(Peers::default()),
                dispatching: Mutex::new(()),
            }),
        })
    }

    /// The address actually bound.
    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has a local address")
    }

    /// A read-only handle on the hub's live peers, kept by the caller after
    /// [`Hub::serve`] has taken the hub. M13 grows this into the status
    /// surface; what is here is "is the runner there, and how long since it
    /// last said so" (E-40).
    pub fn handle(&self) -> HubHandle {
        HubHandle {
            state: Arc::clone(&self.state),
        }
    }

    /// Serve until the listener fails. Dropping the returned future releases
    /// the port and ends the hub's own four housekeeping tasks.
    pub async fn serve(self) -> Result<()> {
        // Held in a guard rather than aborted at the end, **because the end
        // is not always reached**: a caller that drops this future — every
        // end-to-end test does, by aborting the task it runs in — would
        // otherwise leave four tasks alive, each holding an `Arc<State>` and
        // through it the store's `flock` on the state root. A hub that is
        // gone would go on holding its state root against the next one, which
        // M8 turned from a theoretical leak into a real test flake: a state
        // root reported "already in use" by a hub nothing was running.
        let _housekeeping = Housekeeping(vec![
            tokio::spawn(watch_for_a_lost_runner(Arc::clone(&self.state))),
            tokio::spawn(keep_trying_to_write_results(Arc::clone(&self.state))),
            tokio::spawn(let_the_queue_age(Arc::clone(&self.state))),
            tokio::spawn(say_again_why_jobs_wait(Arc::clone(&self.state))),
        ]);
        let serving = async {
            loop {
                let (stream, peer) = self.listener.accept().await.context("accepting")?;
                let state = Arc::clone(&self.state);
                tokio::spawn(async move {
                    if let Err(error) = serve_peer(state, stream).await {
                        eprintln!("herd-hub: connection from {peer} ended: {error:#}");
                    }
                });
            }
        };
        serving.await
    }
}

/// The hub's own background tasks, ended when this goes.
///
/// One line of `Drop` instead of four `abort()` calls at the end of
/// [`Hub::serve`], and the difference is not tidiness: the four calls only
/// ran when `serve` *returned*, and a dropped future never returns. See the
/// comment at the top of `serve`.
struct Housekeeping(Vec<tokio::task::JoinHandle<()>>);

impl Drop for Housekeeping {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// What can still be asked of a hub that `serve` has taken ownership of.
#[derive(Clone)]
pub struct HubHandle {
    state: Arc<State>,
}

impl HubHandle {
    /// Whether any runner has finished its handshake and can be dispatched
    /// to.
    pub fn runner_connected(&self) -> bool {
        self.state.with_peers(|peers| !peers.runners.is_empty())
    }

    /// What the hub knows about the runners connected to it, health ages
    /// included (E-40). A set since M7, in the order their names sort.
    pub fn runners(&self) -> Vec<RunnerStatus> {
        self.state.with_peers(|peers| {
            peers
                .runners
                .values()
                .map(|runner| RunnerStatus {
                    name: runner.name.clone(),
                    instance: runner.instance.clone(),
                    joined_for: runner.joined_at.elapsed(),
                    last_health_age: runner.last_health.elapsed(),
                    reports_seen: runner.reports_seen,
                    draining: runner.draining,
                    switch: runner.switch.clone(),
                    jobs: runner.jobs.iter().map(|job| job.job_id.clone()).collect(),
                })
                .collect()
        })
    }

    /// What one connected runner offers and what its probes say (M5, E-24).
    ///
    /// Read-only, and `None` when no runner is connected. M13 grows the
    /// status document out of this; a test reads it to assert that the hub
    /// recorded what it was offered rather than inferring it. With a fleet,
    /// this is the first runner by name; [`HubHandle::capacity_of`] asks
    /// about one in particular.
    pub fn runner_capacity(&self) -> Option<RunnerCapacity> {
        self.state.with_peers(|peers| {
            peers
                .runners
                .values()
                .next()
                .map(|runner| runner.capacity.clone())
        })
    }

    /// What one named runner offers and what its probes say.
    pub fn capacity_of(&self, name: &str) -> Option<RunnerCapacity> {
        self.state.with_peers(|peers| {
            peers
                .runners
                .get(name)
                .map(|runner| runner.capacity.clone())
        })
    }
}

/// What the hub can say about one connected runner right now.
///
/// `last_health_age` is the field everything about a lost runner rests on, and
/// the one M13 puts on the status document (E-40): a number that grows is a
/// runner that has stopped talking, and there is no other way to tell that
/// from a runner with nothing to do.
#[derive(Debug, Clone)]
pub struct RunnerStatus {
    pub name: String,
    /// Which process of that name this is, when it announced one. Two live
    /// runners sharing a name is a configuration mistake the hub refuses to
    /// hide, so the identity it judged them on is visible here.
    pub instance: Option<String>,
    pub joined_for: Duration,
    pub last_health_age: Duration,
    /// How many health reports this connection has carried. Zero is a runner
    /// that has only just joined, which is not the same as one that has gone
    /// quiet.
    pub reports_seen: u64,
    pub draining: bool,
    /// Where its owner's switch stands (E-33), with her reason.
    pub switch: herd_proto::Switch,
    pub jobs: Vec<JobId>,
}

struct State {
    store: Arc<dyn JobStore>,
    policy: Policy,
    /// When this hub started serving: where the clock begins for an attempt
    /// it inherited and has heard nothing about.
    started: Instant,
    peers: Mutex<Peers>,
    /// Held for the length of one dispatch pass.
    ///
    /// M5 gives the hub a second reason to look at the queue — a capability a
    /// probe has just freed — so two passes can now want the same queued job
    /// at the same time. The `queued -> assigned` transition would catch the
    /// loser, as a refused transition, but by then it has already sent the
    /// job to a runner. One pass at a time, and the question "may this job
    /// start" is answered and acted on inside it.
    ///
    /// It also keeps a runner's probe snapshot out of the middle of a claim
    /// ([`State::update_capabilities`]). **Lock order: this before
    /// [`State::peers`], never the other way**, which is what makes holding
    /// both safe; nothing taken inside `with_peers` reaches back for this.
    dispatching: Mutex<()>,
}

/// What a runner announced about itself in its handshake, past its name.
///
/// One struct rather than three arguments carried side by side through the
/// handshake: they arrive in one frame, they are recorded together, and a
/// fourth fact about a machine should not be a fourth parameter on four
/// functions.
#[derive(Debug, Clone)]
struct Announced {
    /// What it offers, and what its probes say (M5, E-24).
    capacity: RunnerCapacity,
    /// How big its host is (E-22).
    host: HostSize,
    /// How preferred it is when more than one machine could run a job (E-19).
    preference: i32,
    /// The load it measured as it dialled, times 1000 (E-32). Recorded,
    /// never a gate; refreshed by every health report.
    load_milli: Option<u64>,
    /// Where its owner's switch stood as it dialled (E-33).
    ///
    /// In the handshake because the switch outlives the process: an owner who
    /// switched her PC off before rebooting it has not changed her mind, and a
    /// runner the hub believed `on` until its first health report would be a
    /// machine that could be claimed on in that window.
    switch: herd_proto::Switch,
}

/// What a requester gets when it attaches to a job by its id.
struct Attached {
    record: JobRecord,
    /// The output the job had already produced.
    log: Vec<u8>,
    /// Why the job is not moving, if this hub knows that it is not (D19).
    stalled: Option<String>,
}

/// One runner's connection, and what the hub knows about it.
struct ConnectedRunner {
    name: String,
    /// Which *process* this is, as its handshake announced it. What tells a
    /// redial from a twin, which a name cannot.
    instance: Option<String>,
    outbox: Outbox,
    /// What this runner offers, and what its external probes say (M5).
    ///
    /// The runner is the authority: it announces its offer in the handshake
    /// and a snapshot of its probes on every connection, so this is recorded
    /// rather than computed. It lives on the connection because it belongs to
    /// it — an offer from a machine that is no longer here is not an offer,
    /// and a redial brings its own.
    capacity: RunnerCapacity,
    /// How big this machine says it is: the input to E-22's derivation, and
    /// the one thing a job's `-j` is computed from. Announced in the
    /// handshake, because it belongs to the machine and not to the moment.
    host: HostSize,
    /// How preferred this machine is when more than one could run a job
    /// (E-19). Declared by whoever runs it; zero when it says nothing.
    preference: i32,
    /// The one-minute load average this machine last reported, times 1000
    /// (E-32).
    ///
    /// Kept because E-32 asks for the figure to be **recorded**, and refreshed
    /// on every health report because it is a fact about the moment. It is
    /// quoted on the line explaining a placement and compared against nothing:
    /// a busy host is a fact to report, never a reason to refuse a job.
    /// `None` until the first report, and for a machine that cannot read it.
    load_milli: Option<u64>,
    /// How the hub ends this connection without waiting for the socket to
    /// notice. Used when a runner that is plainly alive dials in while this
    /// connection is still registered: the new one is the real one.
    kick: watch::Sender<bool>,
    joined_at: Instant,
    last_health: Instant,
    reports_seen: u64,
    draining: bool,
    /// Where its owner's switch stands (E-33), as this runner last said.
    ///
    /// **The runner is the authority**, not the hub. `herdctl switch` asks the
    /// hub to ask the runner; the runner applies the position, writes it to
    /// its own file and says so here. One state, in the place that owns the
    /// machine — so a `set_switch` frame that never arrived leaves the hub's
    /// view wrong for one health cadence and no longer, instead of leaving two
    /// sides each sure of a different position.
    switch: herd_proto::Switch,
    jobs: Vec<RunningJob>,
}

#[derive(Default)]
struct Peers {
    /// Every runner that has finished its handshake, by the name it
    /// announced.
    ///
    /// **By name, because a name is what an attempt belongs to.** The record
    /// of every attempt carries the runner's name and nothing else, so a
    /// frame about an attempt is judged by looking the name up here; a map
    /// keyed on anything else would need a second lookup that could disagree.
    /// It is also what makes two live processes under one name a
    /// configuration mistake rather than a silent pair (the twin rule in
    /// [`State::register_runner`]) — the hub has exactly one place to put
    /// each name.
    ///
    /// Sorted, so that a fleet in the same state makes the same choice twice:
    /// the claim step breaks a tie between equally preferred runners by name
    /// (E-36 — a placement nobody can predict is one nobody can explain).
    runners: BTreeMap<String, ConnectedRunner>,
    /// When this hub last had evidence that an attempt was alive.
    ///
    /// *Per attempt*, and that is the whole of the design. "Is the runner
    /// connected" is the wrong question twice over: a different runner
    /// connecting says nothing about the attempt the dead one was running,
    /// and a runner that was killed and came back fresh is connected and
    /// healthy while the job it was running exists nowhere. So what is timed
    /// is the attempt: anything the runner says about it is evidence — a
    /// frame, or a health report naming it, which is why that report carries
    /// the jobs — and an attempt nothing has been heard about for the
    /// threshold is lost (E-21), whoever is connected.
    ///
    /// An attempt this hub inherited has no entry, and falls back to the
    /// moment the hub started: it has had no evidence at all, and the clock
    /// cannot begin before it could have been watching.
    attempt_seen: HashMap<JobId, Instant>,
    /// Who to relay a job's logs and outcome to. More than one is possible
    /// from M2: a `herdctl run` follows the job it submitted, and a
    /// `herdctl wait` on the same id attaches beside it rather than taking
    /// its place.
    followers: HashMap<JobId, Vec<Outbox>>,
    /// Per job, why the hub could not write down what it was told about it.
    ///
    /// A job whose end the hub cannot store keeps a record that reads
    /// `running` while nothing at all is happening to it (D19): the runner is
    /// holding the result, and this hub is still trying. Nothing about that is
    /// visible from the outside, which is why the reason is kept here — what a
    /// wait on such a job may not do is say nothing. Cleared the moment the
    /// hub does write the job down.
    stalled: HashMap<JobId, String>,
    /// Ends the hub was told about and could not write, with the connection
    /// that was holding each one. Retried for as long as it takes and never
    /// dropped: a stalled job is recoverable, a destroyed verdict is not
    /// (D19).
    unwritten: HashMap<JobId, (JobOutcome, JobMetrics)>,
    /// Which attempt of each unfinished job this hub is waiting for, **and
    /// which runner it was handed to**.
    ///
    /// A cache of the record, so that checking "is this frame about the
    /// attempt I am waiting for, from the runner I gave it to" does not read a
    /// file for every 8 KiB of a build log. Absent means "ask the record",
    /// which is what happens once per job after a restart.
    ///
    /// The runner's name is in here because the attempt number alone is not an
    /// identity. Attempt 1 of `j0000000001` is one attempt on one machine, and
    /// a frame about it from a different runner is a verdict — or a paragraph
    /// of log — about work that runner did not do.
    live_attempts: HashMap<JobId, (u32, String)>,
    /// Jobs whose end arrived with a log the hub knows to be short, and for
    /// which the missing output has been asked for once. A second short end
    /// is one the hub records, naming what is gone (D29).
    tail_asked: HashSet<JobId>,
    /// Per job reported short of output, how much the hub had written when it
    /// noticed. A shortfall that is still there at the next report, with
    /// nothing written in between, is a loss rather than a chunk in flight.
    short_log: HashMap<JobId, u64>,
    /// When whoever is following each queued job was last told why it is
    /// waiting (E-04).
    ///
    /// The cadence lives here rather than on the record because it is about a
    /// *stream*: the record already carries the reason, written only when it
    /// is news (M6), and E-04 asks for something else — that a requester
    /// watching a ten-minute wait is not left in silence. One instant per
    /// job, pruned when nobody is following it any more.
    wait_told: HashMap<JobId, Instant>,
}

impl State {
    fn with_peers<T>(&self, f: impl FnOnce(&mut Peers) -> T) -> T {
        let mut peers = self
            .peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut peers)
    }

    /// Register a runner, or say what to tell it instead.
    ///
    /// Three cases, and the handshake's instance identity is what separates
    /// them. **The same process dialling again** — same name, same instance —
    /// is a redial: its previous connection is one this hub has not noticed
    /// the end of yet, exactly the case that used to kill a redialling runner
    /// with a terminal `Refused` (D13), so the new connection wins and the old
    /// one is dropped. **A different process claiming the same name** is a
    /// twin, and it is told to wait: kicking the incumbent would have two live
    /// runners taking turns killing each other, which is what one name
    /// configured twice really did — forty-three dropped connections in eight
    /// seconds, with jobs dispatched onto links about to be cut. **A runner
    /// whose link has gone quiet** is reaped either way, because a link
    /// nothing comes back on is not a link and holding it would keep the name
    /// against a runner that is plainly alive.
    ///
    /// Since M7 the hub holds a **fleet**: a second machine under a second
    /// name joins beside the first, and the three cases above are all judged
    /// per name.
    fn register_runner(
        &self,
        name: &str,
        instance: Option<&str>,
        outbox: &Outbox,
        kick: watch::Sender<bool>,
        announced: Announced,
    ) -> Admitted {
        self.with_peers(|peers| {
            // Only the runner wearing this *name* is in the way. A second
            // machine under a second name is a fleet, which is what M7 is
            // for; a second process under one name is the twin the rule
            // below refuses, because an attempt belongs to a name.
            if let Some(incumbent) = peers.runners.get(name) {
                // An instance nobody announced is not an instance that
                // matches: the safe reading of "I cannot tell whether this is
                // the same process" is that it is not, because the harm is
                // one-sided — a redial told to wait comes back in a moment,
                // while a live runner kicked off its job has lost the job.
                let same_instance = instance.is_some() && incumbent.instance.as_deref() == instance;
                let quiet_for = incumbent.last_health.elapsed();
                let gone_quiet =
                    incumbent.reports_seen > 0 && quiet_for > self.policy.health_is_stale_after();
                if !same_instance && !gone_quiet {
                    // A twin is waiting for a configuration to change, or
                    // for the runner holding the name to go quiet, and asking
                    // every few seconds for that would be the flap again in
                    // a quieter costume — one line of log per second instead
                    // of one dropped job.
                    let twin = runner_twin_reason(
                        name,
                        incumbent.instance.as_deref().unwrap_or("unnamed"),
                        instance.unwrap_or("unnamed"),
                        quiet_for,
                    );
                    // On stderr, and in these words, because this is not
                    // the hub being busy: it is a configuration that
                    // cannot be served, and the owner reads it here or
                    // reads nothing.
                    eprintln!("herd-hub: {twin}");
                    return Admitted::StandBy {
                        reason: twin,
                        retry_in: self.policy.health_is_stale_after(),
                    };
                }
                let why = if same_instance {
                    format!(
                        "runner {name} (instance {}) has dialled again",
                        instance.unwrap_or("unnamed")
                    )
                } else {
                    format!("runner {name} has said nothing for {quiet_for:?}")
                };
                println!("herd-hub: dropping the connection this hub was holding, because {why}");
                let _ = incumbent.kick.send(true);
            }
            let now = Instant::now();
            peers.runners.insert(
                name.to_owned(),
                ConnectedRunner {
                    name: name.to_owned(),
                    instance: instance.map(str::to_owned),
                    outbox: outbox.clone(),
                    capacity: announced.capacity,
                    host: announced.host,
                    preference: announced.preference,
                    load_milli: announced.load_milli,
                    kick,
                    joined_at: now,
                    last_health: now,
                    reports_seen: 0,
                    draining: false,
                    switch: announced.switch.clone(),
                    jobs: Vec::new(),
                },
            );
            Admitted::Yes
        })
    }

    /// Give up a runner's place, if this connection is the one holding it.
    ///
    /// By the *connection* and not by the name: a redial has already taken
    /// the name over by the time the old connection notices it is finished,
    /// and a release by name would then throw out the live runner. Answers
    /// with the name released, so the caller can say whose link went.
    ///
    /// The clock on the attempts it was running is not touched here: a socket
    /// closing is not evidence about a job, and a runner that redials in a
    /// second finishes what it was doing. What times an attempt out is the
    /// absence of anything said about it ([`Peers::attempt_seen`]).
    fn release_runner(&self, outbox: &Outbox) -> Option<String> {
        self.with_peers(|peers| {
            let name = peers
                .runners
                .values()
                .find(|runner| runner.outbox.same_channel(outbox))
                .map(|runner| runner.name.clone())?;
            peers.runners.remove(&name);
            Some(name)
        })
    }

    /// Every runner a job may be handed to, as the claim step sees them:
    /// connected, with what it offers, what its probes say, how big it is and
    /// how preferred it is (E-19, E-22, E-25).
    ///
    /// `held` and `reserved` are what the records say is already running, per
    /// runner — the only source of a slot count, a memory holding or a tmpfs
    /// reservation there is (see `crate::matcher`, D79). A runner that is
    /// draining is in here and marked, not left out: the sentence a waiting
    /// job gets has to be able to say that the machine it would have gone to
    /// is leaving.
    fn seats(
        &self,
        held: &BTreeMap<String, Vec<(JobId, LabelSet)>>,
        reserved: &BTreeMap<String, matcher::Reserved>,
    ) -> Vec<Seat> {
        self.with_peers(|peers| {
            peers
                .runners
                .values()
                .map(|runner| {
                    let mut holdings = Holdings::new(runner.capacity.offer().clone());
                    for (job, labels) in held.get(&runner.name).into_iter().flatten() {
                        // Written down, never checked: a job the records say
                        // is out there is out there, whatever the offer says
                        // about its labels now. Checking would quietly leave
                        // such a job out of the holdings and read the host as
                        // idle on top of work that is running.
                        holdings.already_holding(job, labels.clone());
                    }
                    Seat {
                        name: runner.name.clone(),
                        preference: runner.preference,
                        host: runner.host,
                        draining: runner.draining,
                        capacity: runner.capacity.clone(),
                        holdings,
                        reserved: reserved.get(&runner.name).cloned().unwrap_or_default(),
                        load_milli: runner.load_milli,
                        switch: runner.switch.clone(),
                        // Filled by `reservations` before the pass places
                        // anything, from the records alone (D79).
                        reserved_for: None,
                    }
                })
                .collect()
        })
    }

    /// Where a job that has been claimed is sent.
    fn outbox_of(&self, name: &str) -> Option<Outbox> {
        self.with_peers(|peers| peers.runners.get(name).map(|runner| runner.outbox.clone()))
    }

    /// Take a runner's snapshot of what its probes say, if it is *the*
    /// runner. A frame from a connection whose slot has already been taken
    /// over changes nothing.
    ///
    /// Taken under [`State::dispatching`], and that is the whole of it. A
    /// dispatch pass holds that lock from the moment it asks
    /// [`State::why_not_now`] to the moment the job is claimed, and the claim
    /// is not instant: numbering an attempt writes the record, the log's
    /// counter and a line of `events.jsonl`, and fsyncs each one. A snapshot
    /// landing inside that window used to be applied at once, so the hub
    /// *knew* a capability was busy and handed out a job judged free on the
    /// reading before it — a job starting on a runner an external holder is
    /// using, which is the one thing E-26 exists to prevent. Serialised, the
    /// snapshot either precedes the pass, and the job waits, or follows it,
    /// and the next pass is judged on it. There is no third order.
    fn update_capabilities(&self, outbox: &Outbox, capabilities: Vec<CapabilityState>) -> bool {
        let _not_inside_a_claim = self
            .dispatching
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.with_peers(|peers| {
            match peers
                .runners
                .values_mut()
                .find(|runner| runner.outbox.same_channel(outbox))
            {
                Some(runner) => {
                    runner.capacity.update(capabilities);
                    true
                }
                None => false,
            }
        })
    }

    /// Every capability a probe is currently suspending, for the hub's log:
    /// the runner it is on, the capability, and what said so.
    ///
    /// The runner's name is in here because with a fleet it has to be. The
    /// line used to read "kvm is suspended on the runner", which was true of
    /// a hub that held one and is a lie about a hub that holds several.
    fn suspended_capabilities(&self) -> Vec<(String, String, String)> {
        self.with_peers(|peers| {
            peers
                .runners
                .values()
                .flat_map(|runner| {
                    runner
                        .capacity
                        .suspended()
                        .into_iter()
                        .map(|(label, why)| (runner.name.clone(), label, why))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
    }

    /// Where a message about an attempt goes: the runner the attempt was
    /// handed to, draining or not.
    ///
    /// By name, because that is what an attempt belongs to. An
    /// acknowledgement or a cancel sent to "whichever runner is connected"
    /// was a frame about one machine's work delivered to another, which is
    /// the defect naming the holder exists to prevent.
    fn runner_holding(&self, job_id: &JobId) -> Option<Outbox> {
        let holder = self.with_peers(|peers| {
            peers
                .live_attempts
                .get(job_id)
                .map(|(_, holder)| holder.clone())
        });
        let holder = match holder {
            Some(holder) => holder,
            // Nothing cached: after a restart the record is the authority.
            None => self
                .store
                .job(job_id)
                .ok()
                .flatten()
                .and_then(|record| record.attempt().map(|attempt| attempt.runner.clone()))?,
        };
        self.outbox_of(&holder)
    }

    /// Where every connected runner's switch stands (E-33).
    ///
    /// E-33 asks for the position and its reason to be **visible**. The status
    /// document is M13's; this is the answer `herdctl switch` prints, and it is
    /// read straight off the fleet so there is nothing for it to disagree with.
    fn fleet_switches(&self) -> Vec<herd_proto::RunnerSwitch> {
        self.with_peers(|peers| {
            peers
                .runners
                .values()
                .map(|runner| herd_proto::RunnerSwitch {
                    name: runner.name.clone(),
                    switch: runner.switch.clone(),
                    jobs: runner.jobs.len() as u32,
                    stopping: runner.draining,
                })
                .collect()
        })
    }

    /// Take a runner's switch, if it is *the* runner on this connection
    /// (E-33).
    fn record_switch(&self, outbox: &Outbox, switch: herd_proto::Switch) -> Option<String> {
        self.with_peers(|peers| {
            let runner = peers
                .runners
                .values_mut()
                .find(|runner| runner.outbox.same_channel(outbox))?;
            if runner.switch == switch {
                return None;
            }
            let said = format!(
                "runner {} is now switched {} (it was {})",
                runner.name,
                switch.sentence(),
                runner.switch.sentence()
            );
            runner.switch = switch;
            Some(said)
        })
    }

    /// Why a submission cannot be placed anywhere at all, if it cannot.
    ///
    /// About the *fleet*, not about one job: a job that no connected runner
    /// can take is queued with its own sentence and is not refused (D38).
    /// This is the one case a submission is answered with a refusal — there
    /// is no machine here to run anything.
    fn why_no_runner(&self) -> Option<String> {
        self.with_peers(|peers| {
            if peers.runners.is_empty() {
                return Some("no runner is connected".to_owned());
            }
            if peers
                .runners
                .values()
                .all(|runner| runner.draining || !runner.switch.claims())
            {
                return Some(format!(
                    "every runner connected to this hub takes no new job: {}",
                    peers
                        .runners
                        .values()
                        .map(|runner| {
                            if runner.draining {
                                format!("{} is draining", runner.name)
                            } else {
                                // The owner's own words, which is the whole
                                // point of E-33's reason: a submission refused
                                // because somebody is gaming on the PC should
                                // say so.
                                format!("{} is switched {}", runner.name, runner.switch.sentence())
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            None
        })
    }

    fn record_health(
        &self,
        outbox: &Outbox,
        jobs: Vec<RunningJob>,
        draining: bool,
        load_milli: Option<u64>,
    ) {
        self.with_peers(|peers| {
            if let Some(runner) = peers
                .runners
                .values_mut()
                .find(|runner| runner.outbox.same_channel(outbox))
            {
                runner.last_health = Instant::now();
                runner.reports_seen += 1;
                runner.draining = draining;
                runner.jobs = jobs;
                // A figure that could not be read this time does not erase
                // the last one that could: an absent load is unknown, and the
                // most recent figure the machine managed to give is still the
                // most recent figure there is (E-32).
                if load_milli.is_some() {
                    runner.load_milli = load_milli;
                }
            }
        });
    }

    fn follow(&self, id: JobId, outbox: &Outbox) {
        self.with_peers(|peers| peers.followers.entry(id).or_default().push(outbox.clone()));
    }

    fn followers_of(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| peers.followers.get(id).cloned().unwrap_or_default())
    }

    /// Record one chunk of a job's output and take the list of who is
    /// following that job, in one breath.
    ///
    /// The two happen under one lock on purpose. A requester attaching to a
    /// running job is given the output recorded so far and registered as a
    /// follower under the same lock ([`State::attach`]), so every chunk is
    /// carried by exactly one of the two paths: it is in the log a waiter
    /// reads, or it is relayed to it, never both and never neither.
    fn record_chunk(
        &self,
        id: &JobId,
        stream: herd_proto::LogStream,
        seq: u64,
        data: &[u8],
    ) -> (crate::job::Result<LogWrite>, Vec<Outbox>) {
        self.with_peers(|peers| {
            let written = self.store.append_log(id, stream, seq, data);
            let followers = peers.followers.get(id).cloned().unwrap_or_default();
            (written, followers)
        })
    }

    /// Remember that the hub cannot write a job down, and take the list of who
    /// is waiting for it.
    fn note_a_stall(
        &self,
        id: &JobId,
        reason: String,
        outcome: &JobOutcome,
        metrics: &JobMetrics,
    ) -> Vec<Outbox> {
        self.with_peers(|peers| {
            peers.stalled.insert(id.clone(), reason);
            peers
                .unwritten
                .insert(id.clone(), (outcome.clone(), metrics.clone()));
            peers.followers.get(id).cloned().unwrap_or_default()
        })
    }

    /// The hub wrote the job down after all: it is moving again.
    fn stall_is_over(&self, id: &JobId) {
        self.with_peers(|peers| {
            peers.stalled.remove(id);
            peers.unwritten.remove(id);
        });
    }

    /// Attach a requester to a job by its id: its record, the output recorded
    /// so far, why the job is not moving if it is not, and — unless the job is
    /// already over — a place in the list of followers, all under one lock.
    ///
    /// Reading the record and registering have to be one step too: between
    /// them the job could reach its end, and a requester that registered
    /// afterwards would wait for a frame that has already been sent.
    fn attach(&self, id: &JobId, outbox: &Outbox) -> crate::job::Result<Option<Attached>> {
        self.with_peers(|peers| {
            let Some(record) = self.store.job(id)? else {
                return Ok(None);
            };
            let log = self.store.job_log(id)?.unwrap_or_default();
            let stalled = peers.stalled.get(id).cloned();
            if !record.state.is_terminal() {
                peers
                    .followers
                    .entry(id.clone())
                    .or_default()
                    .push(outbox.clone());
            }
            Ok(Some(Attached {
                record,
                log,
                stalled,
            }))
        })
    }

    fn stop_following(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| peers.followers.remove(id).unwrap_or_default())
    }

    /// A requester went away: nobody is waiting for its jobs any more.
    fn forget_follower(&self, outbox: &Outbox) {
        self.with_peers(|peers| {
            for following in peers.followers.values_mut() {
                following.retain(|f| !f.same_channel(outbox));
            }
            peers.followers.retain(|_, following| !following.is_empty());
        });
    }

    /// Where a frame a runner sent about `attempt` of `id` stands: is it the
    /// attempt this hub is waiting for, and is this the runner it was handed
    /// to (E-21)?
    ///
    /// The record is the authority; the cache is only there so that a build
    /// log does not cost one file read per chunk.
    ///
    /// Both halves are needed, and the second one is the one that was
    /// missing. An attempt number says *which go* at a job this is, and two
    /// runners can both be holding the number 1: the hub's, which is live on
    /// the machine it dispatched to, and a verdict from a state root that was
    /// recreated, which happens to carry the same number for the same id. The
    /// number alone cannot tell them apart. The runner the attempt was handed
    /// to can.
    ///
    /// # This gate is not authentication, and must not be read as one
    ///
    /// A runner's name is **self-asserted**. Until M15 there is no token to
    /// check (E-48), so any peer that can reach the hub's port can announce
    /// any name — and while the legitimate runner is *down*, a hand-crafted
    /// peer wearing its name is, to this gate, that runner: it can write a
    /// false verdict, fabricated metrics and forged log lines onto the job
    /// that runner was given. What this gate does stop is a *second* peer
    /// talking about an attempt while the real holder is alive, which is
    /// M3's twin rule, and what D14 does is keep the port on loopback by
    /// default. Neither is a credential. The gate is exactly as strong as
    /// M15's token, which does not exist yet; nothing here should be taken
    /// for proof of who is speaking.
    fn standing_of(&self, id: &JobId, attempt: u32, speaker: &str) -> crate::job::Result<Standing> {
        if let Some((number, holder)) =
            self.with_peers(|peers| peers.live_attempts.get(id).cloned())
        {
            if number != attempt {
                return Ok(Standing::Superseded);
            }
            if holder != speaker {
                return Ok(Standing::AnotherRunner { holder });
            }
            return Ok(Standing::Current);
        }
        let Some(record) = self.store.job(id)? else {
            return Ok(Standing::Superseded);
        };
        // A job that is over is over for everyone, including the runner it
        // was really running on. Its outcome, its measurements and its log
        // were closed together, and a frame that arrives afterwards is
        // refused by name — said first and separately from "that attempt is
        // not the current one", because it is a different thing to be told:
        // the record is final, not merely moved on.
        if record.state.is_terminal() {
            return Ok(Standing::JobIsOver {
                state: record.state.name().to_owned(),
            });
        }
        if !record.attempt_is_current(attempt) {
            return Ok(Standing::Superseded);
        }
        let holder = record
            .attempt()
            .map(|attempt| attempt.runner.clone())
            .unwrap_or_default();
        if holder != speaker {
            return Ok(Standing::AnotherRunner { holder });
        }
        self.with_peers(|peers| peers.live_attempts.insert(id.clone(), (attempt, holder)));
        Ok(Standing::Current)
    }

    fn attempt_is_over(&self, id: &JobId) {
        self.with_peers(|peers| {
            peers.live_attempts.remove(id);
            peers.tail_asked.remove(id);
            peers.attempt_seen.remove(id);
            peers.short_log.remove(id);
        });
    }

    /// Word of an attempt: it is alive, whatever else is happening.
    fn attempt_was_heard_of(&self, id: &JobId) {
        self.with_peers(|peers| peers.attempt_seen.insert(id.clone(), Instant::now()));
    }

    /// How long since anything was heard about each attempt that is under
    /// way, for the jobs named.
    fn unheard_of_for(&self, jobs: &[JobId]) -> Vec<(JobId, Duration)> {
        self.with_peers(|peers| {
            jobs.iter()
                .map(|id| {
                    let seen = peers.attempt_seen.get(id).copied().unwrap_or(self.started);
                    (id.clone(), seen.elapsed())
                })
                .collect()
        })
    }
}

/// What the hub does with a runner that has just said hello.
enum Admitted {
    Yes,
    StandBy { reason: String, retry_in: Duration },
}

/// Where a frame about an attempt stands with the hub.
enum Standing {
    /// The attempt this hub is waiting for, from the runner holding it.
    Current,
    /// An attempt that is over: replaced by a later one, or ended by name.
    Superseded,
    /// A live attempt, but somebody else's: `holder` has it.
    AnotherRunner { holder: String },
    /// The job itself has reached its end, so there is no attempt of it to be
    /// current. Nothing is written to a record that is over — not its
    /// outcome, not its metrics, not its log.
    JobIsOver { state: String },
}

/// What a peer really said, with a blank answer read as no answer.
///
/// `Some("")` is not an identity, and neither is `Some("   ")`. Both used to
/// satisfy every "did it say?" test in this file by being `Some`, which let a
/// peer announce itself as nobody and then match every other nobody's
/// attempt. A name is also trimmed rather than taken as given, so that one
/// runner spelled with a trailing space is not a second runner.
fn said(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

async fn serve_peer(state: Arc<State>, stream: TcpStream) -> Result<()> {
    // Log chunks are small and must not wait for a full segment.
    let _ = stream.set_nodelay(true);
    let socket = tokio_tungstenite::accept_async_with_config(stream, Some(frame_limits()))
        .await
        .context("websocket handshake")?;
    let (mut sink, mut source) = socket.split();

    // The handshake is read as text first, because a handshake this hub
    // cannot *decode* still deserves a sentence. A capability offer is
    // validated as it decodes (E-24), so one mistake in a runner's
    // configuration file — a name that is not a label, one capability
    // offered twice — makes its whole `Hello` undecodable. Treated like any
    // other unreadable frame that cost the connection its socket and put the
    // reason on the hub's stdout, where the runner's owner never sees it.
    // It is a terminal no of exactly D13's shape: the offer is on the
    // runner's disk and dialling again will say the same thing.
    //
    // And "could not read it" covers the frame as well as the text in it. A
    // first frame that is binary, or larger than this hub reads, used to come
    // back out of here as an error and cost the peer its socket with nothing
    // written on it — the same silence, through the one input the text path
    // did not cover.
    let text = match first_frame(&mut source).await {
        FirstFrame::Said(text) => text,
        FirstFrame::Unreadable(what_went_wrong) => {
            let reason = unreadable_handshake_reason(&what_went_wrong);
            eprintln!("herd-hub: {reason}");
            return refuse(&mut sink, reason).await;
        }
        FirstFrame::Gone => bail!("the peer closed before saying hello"),
    };
    let hello = match from_frame::<ToHub>(&text) {
        Ok(hello) => hello,
        Err(error) => {
            let reason = unreadable_handshake_reason(&error.to_string());
            eprintln!("herd-hub: {reason}");
            return refuse(&mut sink, reason).await;
        }
    };
    let (role, peer_name, peer_instance, peer_state_root, peer_health_every, announced) =
        match hello {
            ToHub::Hello {
                protocol_version,
                role,
                token,
                name,
                instance,
                state_root,
                health_every_ms,
                capabilities,
                host,
                preference,
                load_milli,
                switch,
            } => {
                if protocol_version != PROTOCOL_VERSION {
                    return refuse(&mut sink, version_mismatch_reason(protocol_version)).await;
                }
                // The token is carried from M1 and validated from M15 (D6, Q8).
                let _ = token;
                // Trimmed once, here, so that every reader below — the ownership
                // gate, the twin rule, the capability registry's sentences — is
                // looking at the same name.
                let name = said(name);
                (
                    role,
                    // A blank identity is no identity. `Some("")` — or a name of
                    // spaces — used to walk straight through the refusal below,
                    // because it *has* an instance and *has* a name as far as an
                    // `Option` is concerned, and the peer then wore a blank
                    // effective name: one that matches every other blank-named
                    // peer's attempt, which is the exact hole naming an attempt's
                    // runner was meant to close. So whitespace is stripped and
                    // what is left empty is absent, which is the case the hub
                    // already refuses.
                    //
                    // The offer carries the same trimmed name, not the one as
                    // given: the sentence a human reads about a capability names
                    // the runner, and " wsl-pc " is not a second machine there
                    // either.
                    name.clone().unwrap_or_default(),
                    said(instance),
                    said(state_root),
                    health_every_ms.map(Duration::from_millis),
                    Announced {
                        capacity: RunnerCapacity::new(name, capabilities),
                        host,
                        preference,
                        load_milli,
                        switch,
                    },
                )
            }
            other => {
                let reason = format!(
                    "the first message must be hello, not {}",
                    message_kind(&other)
                );
                return refuse(&mut sink, reason).await;
            }
        };

    // A runner has to be a *particular* runner, because that is what an
    // attempt belongs to. With an empty name, the record's `runner` field says
    // nothing and two different machines both match it — so a runner that
    // gave no name wears its instance identity, which is the next best thing
    // it has, and one that gave neither is refused by name. The refusal is
    // terminal (D13) on purpose: dialling again with the same silence changes
    // nothing. Our own runner always announces an instance, so this reaches
    // only a peer that identifies itself not at all.
    let peer_name = match (role, peer_name.is_empty(), &peer_instance) {
        (Role::Runner, true, Some(instance)) => instance.clone(),
        (Role::Runner, true, None) => {
            return refuse(
                &mut sink,
                "a runner must say what it is called, or at least which process it is: an \
                 attempt belongs to the runner it was handed to, and a runner with no name \
                 and no instance cannot be told from any other"
                    .to_owned(),
            )
            .await
        }
        _ => peer_name,
    };

    // The hub's own state root goes out with the welcome whatever happens
    // next, including to a peer this hub is about to turn away: that string is
    // how a runner holding a verdict for a hub whose state is gone finds out,
    // and a turned-away peer that was not told would come back holding the
    // same thing for ever.
    let hub_state_root = state.store.state_root_id().to_owned();
    send(
        &mut sink,
        &FromHub::Welcome {
            protocol_version: PROTOCOL_VERSION,
            hub_version: hub_version().to_owned(),
            state_root: hub_state_root.clone(),
        },
    )
    .await?;

    // Two configurations that are each legal on their own side and fatal
    // together. The hub refuses, at startup, a silence threshold that does
    // not outlast three of the reports it *expects*; the runner refuses one
    // that does not outlast three of the reports it *sends*. Both checks pass
    // for `herd-hub --health-every 1 --lost-after 3` against `herd-runner
    // --health-every 10 --hub-silence-limit 30`, and that pair burnt both
    // attempts of a quiet `sleep 30` in about six seconds and ended it
    // `lost`: the hub was timing the gap between two reports the runner never
    // promised. So the runner's real cadence crosses the handshake and the
    // pairing is judged here, by its figures, before a job can be placed on
    // it.
    //
    // `Refused` and not `StandBy`: a cadence is settled when the process
    // starts and dialling again announces the same one, so a runner told to
    // wait would wait for ever on a configuration only a human can change —
    // exactly the terminal "no" D13 keeps this frame for, and the same answer
    // a version mismatch gets. A refusal that names both figures is also what
    // reaches the operator, where an endless stand-by is a quiet runner.
    //
    // A peer that announces no cadence is served as before: the hub refuses
    // figures it has been given, never ones it has guessed.
    if let (Role::Runner, Some(cadence)) = (role, peer_health_every) {
        if let Some(reason) = herd_proto::silence_threshold_refusal(
            "the hub's silence threshold (--lost-after)",
            state.policy.lost_after,
            "the cadence this runner really reports at (--health-every)",
            cadence,
        ) {
            eprintln!("herd-hub: runner {peer_name} is refused: {reason}");
            return refuse(&mut sink, reason).await;
        }
    }

    // A runner carrying another state root's state is not served on this
    // connection. Not because the machine is bad — it is fine, and it will be
    // welcome a second from now — but because everything it is holding is
    // about jobs whose ids mean something else here, and there is no way to
    // tell such a frame from an honest one once it arrives. It is told to
    // stand by, with both roots named, and it comes back having dealt with
    // what it holds (D38: waiting is not being refused).
    //
    // Only runners: a requester holds nothing between connections, so there
    // is nothing of a vanished state for it to carry.
    //
    // A runner that announces **no** root at all is admitted, and that is the
    // behaviour and not an oversight — M3's note had it standing by until it
    // could record one, which is not what happens and not what should. A
    // runner whose data root cannot be written to is still a machine that can
    // build: it says so loudly on its own stderr (`COULD NOT write down which
    // hub state root this data root belongs to`), and because `None` never
    // equals this hub's root, everything it is holding from a previous life
    // is set aside rather than believed. Turning it away instead would cost a
    // working runner and protect nothing.
    let foreign_root = match (role, &peer_state_root) {
        (Role::Runner, Some(theirs)) if theirs != &hub_state_root => {
            Some(foreign_state_root_reason(&hub_state_root, theirs))
        }
        _ => None,
    };

    let (outbox, inbox) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_frames(sink, inbox));
    let outcome = match (role, foreign_root) {
        (_, Some(reason)) => {
            eprintln!("herd-hub: runner {peer_name} is asked to stand by: {reason}");
            let _ = outbox.send(FromHub::StandBy {
                reason,
                retry_in_ms: state.policy.stand_by_retry().as_millis() as u64,
            });
            Ok(())
        }
        (Role::Runner, None) => {
            serve_runner(
                &state,
                &peer_name,
                peer_instance.as_deref(),
                &outbox,
                &mut source,
                announced,
            )
            .await
        }
        (Role::Requester, None) => serve_requester(&state, &outbox, &mut source).await,
    };
    drop(outbox); // ends the writer, which then closes the socket
    let _ = writer.await;
    outcome
}

async fn serve_runner(
    state: &State,
    name: &str,
    instance: Option<&str>,
    outbox: &Outbox,
    source: &mut Source,
    announced: Announced,
) -> Result<()> {
    let (host, preference) = (announced.host, announced.preference);
    // What it offers, said out loud with every capability's origin: a denial
    // nobody can see is a denial nobody can trust (E-24).
    for line in announced.capacity.report() {
        println!("herd-hub: {line}");
    }
    // And how big it says it is, because that is what every `-j` on this
    // machine is derived from (E-22) — and a machine that says nothing is a
    // machine no derived job can be placed on, which an operator should read
    // here rather than infer from a queue that will not move.
    match (host.memory_mib, host.threads) {
        (Some(memory), Some(threads)) => println!(
            "herd-hub: {name} offers {memory} MiB and {threads} threads, preference {preference}"
        ),
        _ => println!(
            "herd-hub: {name} does not say {}, so no job whose -j has to be derived can be \
             placed on it (preference {preference})",
            host.what_is_missing().unwrap_or("how big it is")
        ),
    }
    let (kick, mut kicked) = watch::channel(false);
    match state.register_runner(name, instance, outbox, kick, announced) {
        Admitted::Yes => {}
        // Not a refusal: a runner told to wait comes back, and one that is
        // refused dies (D13). Telling a redialling runner "a runner is
        // already connected" with a terminal frame is how M2 killed runners.
        Admitted::StandBy { reason, retry_in } => {
            println!("herd-hub: a runner is asked to stand by: {reason}");
            let _ = outbox.send(FromHub::StandBy {
                reason,
                retry_in_ms: retry_in.as_millis() as u64,
            });
            return Ok(());
        }
    }
    println!("herd-hub: runner {name} has joined");
    // Whatever happens from here, the runner slot is given back before this
    // returns: a failure that kept the slot would mean no runner could ever
    // join this hub again.
    let outcome = match dispatch_queued_jobs(state) {
        Ok(()) => relay_from_runner(state, name, outbox, source, &mut kicked).await,
        Err(error) => Err(error),
    };
    if let Some(gone) = state.release_runner(outbox) {
        println!(
            "herd-hub: runner {gone} is gone; an attempt it was running is lost if it is \
             not back within {:?} (E-21)",
            state.policy.lost_after
        );
        // A job that was sticking to this runner is no longer sticking to it:
        // it was waiting for a machine that is now not *available*, which is
        // exactly when E-19 lets it move. One pass, now, so that the job
        // moves within seconds of the link going rather than at the next
        // thing that happens to wake the queue.
        if let Err(error) = dispatch_queued_jobs(state) {
            eprintln!("herd-hub: judging the queue after {gone} left failed: {error:#}");
        }
        // Whoever is waiting hears that the link went, and that the job has
        // not been given up on. Not a refusal, and the followers keep their
        // place: the runner may redeliver the result it is holding, and if it
        // does not the job is attempted again on this same connection's watch.
        for job in attempts_in_the_air(state) {
            for follower in state.followers_of(&job) {
                let _ = follower.send(FromHub::JobNote {
                    job_id: job.clone(),
                    note: format!(
                        "the runner executing this job lost its link to the hub. If it does \
                         not come back within {:?} the job is attempted again, and this wait \
                         carries on either way.",
                        state.policy.lost_after
                    ),
                });
            }
        }
    }
    outcome
}

/// The jobs whose attempt is under way and whose runner is not here.
fn attempts_in_the_air(state: &State) -> Vec<JobId> {
    state
        .store
        .unfinished_jobs()
        .unwrap_or_default()
        .into_iter()
        .filter(|record| {
            matches!(
                record.state,
                JobState::Assigned | JobState::Running | JobState::Interrupted { .. }
            ) && record.attempt().is_some_and(|a| a.ended_at.is_none())
        })
        .map(|record| record.id)
        .collect()
}

/// Hand the runner every queued job it can take, and leave the rest queued
/// with the reason why.
///
/// Called whenever the answer could have changed: a runner joins, a runner
/// says a probe has changed its mind, a job is submitted. After a restart the
/// queue is whatever the job directories say it is, and a job that was queued
/// when the hub went down is simply a job waiting for a runner — as is a job
/// that was requeued after a loss (E-21). Nobody need be following them: the
/// requester that submitted them may be long gone, which is exactly the
/// detached case (E-03), and the result is recorded whether anyone is
/// listening or not.
///
/// Only *queued* jobs are dispatched. A job that is assigned or running
/// belongs to an attempt that is not over, and a job left `interrupted` by a
/// hub that died belongs to an attempt whose runner may still redeliver its
/// result — deciding that such an attempt is lost is the watchdog's, on the
/// clock, never a guess made here.
fn dispatch_queued_jobs(state: &State) -> Result<()> {
    let _one_pass = state
        .dispatching
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    dispatch_a_pass(state)
}

/// One pass over the queue, in order, with the claim lock already held.
///
/// Separate from [`dispatch_queued_jobs`] only so that the submission path —
/// which has to take the same lock around a little more than this — cannot
/// take it twice and deadlock. **Every** dispatch in this hub goes through
/// here: there is one queue and one way out of it (E-20). A submission does
/// not hand its own job to a runner on the side, and a merger's job is not
/// pushed ahead through a lane of its own — it is simply first in this order
/// (E-43).
fn dispatch_a_pass(state: &State) -> Result<()> {
    let unfinished = state
        .store
        .unfinished_jobs()
        .context("reading the queue the hub recovered")?;
    // What every runner is already holding, read from those same records and
    // from nothing else: there is no counter of busy slots in this hub, so
    // there is none to leak and none to rebuild after a restart (D79).
    let held = matcher::holdings(&unfinished);
    // And what every runner has reserved, from the same records and the same
    // filter: the memory a running job *declared* it needs and the tmpfs it
    // *declared* it would take (E-29, E-31). There is no reservation table in
    // this hub, so there is none to release on any of the eight ways a job
    // ends and none to rebuild after a restart (D79).
    let reserved = matcher::reserved(&unfinished);
    let mut seats = state.seats(&held, &reserved);
    // One moment for the whole pass, so that the order cannot change halfway
    // through it, and the moment the figures in the sentences are computed
    // from is the moment the order was.
    let now = Timestamp::now();
    let queue = queue::order(unfinished.clone(), now, state.policy.aging_step);
    // Which hosts are being held empty for which job (E-27), decided before a
    // single job is placed. Before, because a reservation found while walking
    // the queue in priority order would be found only after everything ahead
    // of the drain-first job had been claimed — and a stream of builds would
    // then starve a metal trial for ever, which is the attack E-27's
    // "drains the build slot first" exists to answer. Derived from these same
    // records, so there is nothing to leak and nothing to rebuild (D79).
    let held_empty = matcher::reservations(
        &queue,
        &seats,
        now,
        state.policy.drain_first_limit,
    );
    for seat in seats.iter_mut() {
        seat.reserved_for = held_empty.get(&seat.name).cloned();
    }
    let total = queue.len();
    for (place, record) in queue.iter().enumerate() {
        // The one job this one named, if it named one (E-10). Looked up here
        // rather than inside the claim step, which is a pure function of a
        // record and a fleet: a parent that is finished is not in the queue at
        // all, so finding it is a read of the store and the claim step does no
        // I/O. `unfinished` first, because that is already in hand.
        let placing = matcher::Placing {
            parent: parent_of(state, record, &unfinished),
            drain_first_limit: Some(state.policy.drain_first_limit),
        };
        // The whole claim decision, for this job, against the fleet as this
        // pass found it — and `seats` is carried *through* the loop, so a job
        // this pass has just placed is contending with the next one (E-25).
        match matcher::place(record, &mut seats, now, state.policy.stickiness, &placing) {
            Placement::Claim {
                runner,
                why,
                command,
                parallelism,
            } => {
                if !dispatch(state, record, &runner, &why, command, parallelism)? {
                    // The link went between the judgement and the frame. The
                    // job is still queued, its labels are still taken on a
                    // seat that is gone, and the next pass — which a
                    // released runner triggers — judges it afresh.
                    let standing = queue::standing(record, now, state.policy.aging_step);
                    let sentence = queue::waiting_sentence_behind(
                        place + 1,
                        total,
                        &standing,
                        &queue::what_is_ahead(&queue, place, now, state.policy.aging_step),
                        &format!(
                            "it is waiting because runner {runner}'s connection ended while \
                             this pass was placing it there"
                        ),
                    );
                    say_it_waits(state, &record.id, &sentence)?;
                }
            }
            // This job waits, and the queue behind it does not. **That is
            // E-19's answer** to the head-of-line question M6 left open: a
            // job sticking to a busy runner it prefers holds nothing else up,
            // because the machine it is waiting for is not the machine the
            // job behind it would go to. The cost is that a lower-priority
            // job can take a free lesser runner while a higher-priority one
            // waits for its preferred machine — which is what E-19 asks for,
            // bounded by the stickiness limit it asks to be visible.
            Placement::Wait { why } => {
                let standing = queue::standing(record, now, state.policy.aging_step);
                let sentence = queue::waiting_sentence_behind(
                    place + 1,
                    total,
                    &standing,
                    &queue::what_is_ahead(&queue, place, now, state.policy.aging_step),
                    &format!("it is waiting because {why}"),
                );
                say_it_waits(state, &record.id, &sentence)?;
            }
            // Nothing anywhere can ever run it, so it ends here rather than
            // waiting for a machine that cannot exist (E-36). Terminal, and
            // the one place in the claim step that is: everything else about
            // a job that cannot start now is a wait (D38).
            Placement::Impossible { why } => {
                end_the_job(
                    state,
                    record,
                    JobOutcome::Impossible {
                        reason: why.clone(),
                    },
                    &format!("can never run: {why}"),
                )?;
            }
            // Over before it began: its deadline passed while it was queued,
            // or the one job it named did not succeed (E-09, E-10). Terminal
            // like `Impossible`, and a **different outcome** on purpose:
            // `cancelled` is what D40 calls an end nobody chose to run, and
            // `herdctl` says it in different words.
            Placement::Cancel { why } => {
                end_the_job(
                    state,
                    record,
                    JobOutcome::Cancelled {
                        reason: why.clone(),
                    },
                    &why,
                )?;
            }
        }
    }
    Ok(())
}

/// Where the one job this job named as its parent stands (E-10).
///
/// Read from the records: the queue this pass already holds first, then the
/// store for a parent that is over. A parent this hub has no record of is
/// [`matcher::Parent::Gone`] and is never read as one that succeeded.
fn parent_of(state: &State, record: &JobRecord, unfinished: &[JobRecord]) -> matcher::Parent {
    let Some(parent_id) = record.after.clone() else {
        return matcher::Parent::None;
    };
    let parent = unfinished
        .iter()
        .find(|other| other.id == parent_id)
        .cloned()
        .or_else(|| state.store.job(&parent_id).ok().flatten());
    let Some(parent) = parent else {
        return matcher::Parent::Gone { id: parent_id };
    };
    match &parent.state {
        JobState::Finished { outcome } => match outcome {
            JobOutcome::Exited { code: 0 } => matcher::Parent::Succeeded { id: parent_id },
            other => matcher::Parent::DidNot {
                id: parent_id,
                how: how_it_ended(other),
            },
        },
        state => matcher::Parent::NotYet {
            id: parent_id,
            state: state.name().to_owned(),
        },
    }
}

/// A parent's ending, in the words its own requester would have read.
///
/// Said here rather than borrowed from `herdctl`, because the hub writes it
/// onto a *child's* record and a record is read with no tool but `cat`.
fn how_it_ended(outcome: &JobOutcome) -> String {
    match outcome {
        JobOutcome::Exited { code } => format!("exited {code} rather than 0"),
        JobOutcome::Signalled { signal } => format!("was killed by signal {signal}"),
        JobOutcome::CouldNotStart { reason } => format!("could not be started: {reason}"),
        JobOutcome::StoppedByCap { cap, limit, .. } => {
            format!("was stopped by its {cap:?} cap of {limit}")
        }
        JobOutcome::Cancelled { reason } => format!("was cancelled: {reason}"),
        JobOutcome::Lost { reason } => format!("was lost: {reason}"),
        JobOutcome::Impossible { reason } => format!("could never have run: {reason}"),
    }
}

/// End a job the claim step ended, with the reason on its record and in the
/// stream of whoever is waiting for it (E-36, E-09, E-10).
///
/// One door for the three ends the claim step can decide: no machine could
/// ever run it (`impossible`, E-36), its deadline passed while it waited
/// (`cancelled`, E-09) and the one job it named did not succeed (`cancelled`,
/// E-10). The outcome is the caller's because the three are not the same end,
/// and a reader of a record must be able to tell them apart (D15).
fn end_the_job(
    state: &State,
    record: &JobRecord,
    outcome: JobOutcome,
    said: &str,
) -> Result<()> {
    state
        .store
        .record_state(
            &record.id,
            JobState::Finished {
                outcome: outcome.clone(),
            },
        )
        .with_context(|| format!("ending {}", record.id))?;
    eprintln!("herd-hub: {} {said}", record.id);
    for follower in state.stop_following(&record.id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: record.id.clone(),
            outcome: outcome.clone(),
            metrics: JobMetrics::default(),
        });
    }
    Ok(())
}

/// Re-judge the queue on a clock, so that aging happens even when nothing
/// else does (E-18).
///
/// Every other dispatch pass is triggered by something: a runner joins, a
/// probe changes its mind, a job ends, a job is submitted. That is enough for
/// the *order* to be right — the order only matters at the moment a job can
/// start, and every one of those moments is one of those events. It is not
/// enough for the **sentence** on a waiting job's record, which carries
/// figures: a job queued on a hub where nothing happens for an hour would
/// still be saying it rises to the next level in two minutes, and a figure
/// that is an hour stale is worse than no figure at all.
///
/// So the queue is also re-judged on a clock. A quarter of the aging step,
/// never oftener than once a second and never rarer than once a minute — so
/// at the default step it is a pass a minute, and a figure in a sentence is
/// never more than a minute stale. A pass costs one read of the queue, and
/// writes nothing unless a sentence has actually changed (`record_waiting`
/// writes only what is news). A hub with nothing queued does not even read:
/// there is nothing to age.
async fn let_the_queue_age(state: Arc<State>) {
    let every =
        (state.policy.aging_step / 4).clamp(Duration::from_secs(1), Duration::from_secs(60));
    loop {
        tokio::time::sleep(every).await;
        let anything_queued = state
            .store
            .unfinished_jobs()
            .map(|queue| queue.iter().any(|record| record.state == JobState::Queued))
            .unwrap_or(false);
        if !anything_queued {
            continue;
        }
        if let Err(error) = dispatch_queued_jobs(&state) {
            eprintln!("herd-hub: re-judging the queue failed: {error:#}");
        }
    }
}

/// Say again, on a clock, why every followed job is still waiting (E-04).
///
/// E-04 asks for a queued stream to report "at least every 60 s", and the two
/// ways to fail that are opposite. Saying it only when the sentence *changes*
/// — which is what the record does, and rightly, because a record is history —
/// leaves a requester watching a job stuck on one lock file in silence for an
/// hour. Saying it on every dispatch pass floods: a pass runs when a runner
/// joins, when a probe changes its mind, when a job ends, when one is
/// submitted, and once a minute besides, so a busy fleet would write a
/// requester hundreds of identical lines.
///
/// So this is a clock of its own, and it reads the sentence the dispatch pass
/// already wrote on the record rather than judging anything itself: one task,
/// one figure (`--wait-restatement`), and a bound an operator can compute —
/// over a wait of `W` a follower gets at most `W / restatement + 1` lines.
///
/// It reads only the records of jobs somebody is following, so a hub nobody is
/// watching does no work here at all.
async fn say_again_why_jobs_wait(state: Arc<State>) {
    // Checked oftener than the cadence, so that "at least every 60 s" is met
    // rather than approached: a loop that woke exactly every 60 s would drift
    // past it by however long a pass of it takes.
    let restating = Restating::every(state.policy.wait_restatement);
    loop {
        tokio::time::sleep(restating.tick).await;
        let followed = state.with_peers(|peers| {
            // And the pruning, under the same lock: a job nobody follows any
            // more leaves nothing behind in this map.
            peers.wait_told.retain(|job, _| {
                peers
                    .followers
                    .get(job)
                    .is_some_and(|followers| !followers.is_empty())
            });
            peers
                .followers
                .iter()
                .filter(|(_, followers)| !followers.is_empty())
                .map(|(job, _)| job.clone())
                .collect::<Vec<_>>()
        });
        for job_id in followed {
            // Only a job that is *queued* and has a reason on it. A running
            // job is not waiting, and a queued job the hub has not yet had a
            // reason for says nothing rather than saying nothing useful.
            let Ok(Some(record)) = state.store.job(&job_id) else {
                continue;
            };
            if record.state != JobState::Queued {
                continue;
            }
            let Some(reason) = record.waiting_for else {
                continue;
            };
            let due = state.with_peers(|peers| {
                let due = restating.is_due(peers.wait_told.get(&job_id).map(|told| told.elapsed()));
                if due {
                    peers.wait_told.insert(job_id.clone(), Instant::now());
                }
                due
            });
            if !due {
                continue;
            }
            let note = format!("this job is queued and has not started: {reason}");
            for follower in state.followers_of(&job_id) {
                let _ = follower.send(FromHub::JobNote {
                    job_id: job_id.clone(),
                    note: note.clone(),
                });
            }
        }
    }
}

/// Record why a queued job is not starting, and tell whoever is waiting for
/// it.
///
/// The record first and unconditionally, so that a job nobody is following
/// still says on its disk why nothing is happening to it (E-53). The note goes
/// out only when the reason is *news*, because a requester following a job
/// that waits an hour on one lock file wants one sentence, not one per
/// dispatch pass — M8 owns the periodic re-statement E-04 asks for.
fn say_it_waits(state: &State, job_id: &JobId, reason: &str) -> Result<()> {
    let news = state
        .store
        .record_waiting(job_id, reason)
        .with_context(|| format!("recording why {job_id} is waiting"))?;
    if !news {
        return Ok(());
    }
    println!("herd-hub: {job_id} stays queued: {reason}");
    // News resets the cadence: a requester told the reason this second does
    // not want the same sentence again a second later because a clock
    // happened to come round (E-04's flooding half).
    state.with_peers(|peers| peers.wait_told.insert(job_id.clone(), Instant::now()));
    let note = format!("this job is queued and has not started: {reason}");
    for follower in state.followers_of(job_id) {
        let _ = follower.send(FromHub::JobNote {
            job_id: job_id.clone(),
            note: note.clone(),
        });
    }
    Ok(())
}

/// Hand one job to the runner, recording the attempt before the frame goes
/// out. False when there is no runner to hand it to.
fn dispatch(
    state: &State,
    record: &JobRecord,
    runner_name: &str,
    why: &str,
    command: Vec<String>,
    parallelism: Option<herd_proto::Parallelism>,
) -> Result<bool> {
    let job_id = &record.id;
    let Some(runner) = state.outbox_of(runner_name) else {
        return Ok(false);
    };
    // The explanation and the derived figure go on the record **before** the
    // frame goes out, in the one write the attempt is numbered by. A hub that
    // sent the job first and explained afterwards would have a window in
    // which a job was running with no line saying why it was there (E-36) —
    // and, worse for M10, a `-j` that reached the machine before it reached
    // the record.
    let attempt = state
        .store
        .begin_attempt(
            job_id,
            runner_name,
            why,
            parallelism.map(|derived| derived.threads),
        )
        .with_context(|| format!("recording an attempt on {job_id}"))?;
    state.with_peers(|peers| {
        peers
            .live_attempts
            .insert(job_id.clone(), (attempt, runner_name.to_owned()))
    });
    // Handing it out is the first word about it: the clock on this attempt
    // starts here, not at whatever the hub last heard about the job before.
    state.attempt_was_heard_of(job_id);
    if runner
        .send(FromHub::AssignJob {
            job_id: job_id.clone(),
            attempt,
            command,
        })
        .is_err()
    {
        return Ok(false);
    }
    println!("herd-hub: {job_id} is attempt {attempt} on runner {runner_name}: {why}");
    Ok(true)
}

/// Serve one runner's connection. `name` is what that connection announced in
/// its handshake, and every frame about an attempt is judged against it: an
/// attempt belongs to the runner it was handed to.
async fn relay_from_runner(
    state: &State,
    name: &str,
    outbox: &Outbox,
    source: &mut Source,
    kicked: &mut watch::Receiver<bool>,
) -> Result<()> {
    loop {
        let message = tokio::select! {
            // The hub has accepted a newer connection from this runner; this
            // one is a socket onto a process that is gone, or onto a link
            // nothing comes back on.
            _ = kicked.changed() => return Ok(()),
            message = next_message(source) => match message? {
                Some(message) => message,
                None => return Ok(()),
            },
        };
        match message {
            ToHub::Health {
                jobs,
                draining,
                load_milli,
                switch,
            } => {
                // A report is two different claims. "I am alive" is this
                // runner's to make, and it is recorded whatever else the
                // report says. "I am running these jobs" is a claim about
                // *attempts*, and an attempt belongs to the runner it was
                // handed to — so each entry is judged before it is kept, and
                // an entry about somebody else's attempt is refused by name
                // rather than landing in the hub's picture of this runner.
                let mut its_own = Vec::new();
                for job in jobs {
                    if mind_a_reported_job(state, name, outbox, &job) {
                        its_own.push(job);
                    }
                }
                state.record_health(outbox, its_own, draining, load_milli);
                // And the owner's switch, which rides on every report (E-33).
                // A position that moved is said out loud **and** re-judges the
                // queue at once: `off` means the jobs waiting for this machine
                // are claimable elsewhere now, not at the next thing that
                // happens to wake the hub (D80). That is where E-33's ten
                // seconds is actually spent.
                if let Some(said) = state.record_switch(outbox, switch) {
                    println!("herd-hub: {said}");
                    dispatch_queued_jobs(state)?;
                }
                // The answer is what tells the runner this link is alive: a
                // socket can stay open onto a hub that is gone.
                let _ = outbox.send(FromHub::HealthRecorded {});
            }
            ToHub::JobStarted { job_id, attempt } => {
                if !is_current(state, name, outbox, &job_id, attempt) {
                    continue;
                }
                // A failure here is the hub's disk, not the runner's doing, so
                // it is said out loud and the connection goes on (D19) — and
                // whoever is waiting for the job is told, because a record
                // that cannot move is not something a wait can see by itself.
                match state.store.record_state(&job_id, JobState::Running) {
                    Ok(()) => state.stall_is_over(&job_id),
                    Err(error) => {
                        eprintln!("herd-hub: recording {job_id} as running failed: {error}");
                        let note = stalled_note(&error.to_string());
                        for follower in state.followers_of(&job_id) {
                            let _ = follower.send(FromHub::JobNote {
                                job_id: job_id.clone(),
                                note: note.clone(),
                            });
                        }
                    }
                }
            }
            // What the runner's external probes say (E-26). Recording it is
            // half of it; the other half is that a probe which has just
            // *freed* a capability is the one event that can make a queued
            // job runnable with nothing else having happened, so the queue is
            // walked again here. That is what turns "the job waits" into "the
            // job runs when the lock is released" without anyone resubmitting
            // it.
            ToHub::CapabilitiesChanged { capabilities } => {
                if !state.update_capabilities(outbox, capabilities) {
                    continue;
                }
                for (runner, label, reason) in state.suspended_capabilities() {
                    println!("herd-hub: {label} is suspended on {runner}: {reason}");
                }
                dispatch_queued_jobs(state)?;
            }
            ToHub::LogChunk {
                job_id,
                attempt,
                stream,
                seq,
                data,
            } => {
                if !is_current(state, name, outbox, &job_id, attempt) {
                    continue;
                }
                let (written, followers) = state.record_chunk(&job_id, stream, seq, &data);
                match written {
                    // Acknowledged only once it is on the disk, so that a
                    // chunk the runner still holds is a chunk the hub does not
                    // have (D36) — the same rule as a result, for the same
                    // reason.
                    Ok(LogWrite::Recorded) => {
                        let _ = outbox.send(FromHub::LogAck {
                            job_id: job_id.clone(),
                            attempt,
                            through_seq: seq + 1,
                        });
                        // Relayed whether or not anyone is listening: a
                        // requester watching the job must not go blind
                        // because the hub's disk did.
                        for follower in followers {
                            let _ = follower.send(FromHub::LogChunk {
                                job_id: job_id.clone(),
                                stream,
                                seq,
                                data: data.clone(),
                            });
                        }
                    }
                    // Already written: this is the runner sending again what
                    // it was never acknowledged for. Acknowledged, not
                    // relayed — whoever is following has seen it once.
                    Ok(LogWrite::AlreadyRecorded) => {
                        let _ = outbox.send(FromHub::LogAck {
                            job_id: job_id.clone(),
                            attempt,
                            through_seq: seq + 1,
                        });
                    }
                    Err(error) => {
                        eprintln!("herd-hub: recording a log chunk of {job_id} failed: {error}");
                        for follower in followers {
                            let _ = follower.send(FromHub::LogChunk {
                                job_id: job_id.clone(),
                                stream,
                                seq,
                                data: data.clone(),
                            });
                        }
                    }
                }
            }
            ToHub::JobFinished {
                job_id,
                attempt,
                outcome,
                chunks_sent,
                metrics,
            } => {
                if !is_current(state, name, outbox, &job_id, attempt) {
                    continue;
                }
                // A result for a job this hub has no record of — a state root
                // that was replaced under it, say. There is nothing to write
                // it to, and refusing it would wedge the runner, which would
                // redeliver it on every connection for ever. So it is
                // acknowledged and said out loud.
                let known = match state.store.job(&job_id) {
                    Ok(None) => {
                        eprintln!(
                            "herd-hub: a runner reported how {job_id} ended, and this hub \
                             has no record of that job: {outcome:?} is dropped"
                        );
                        let _ = outbox.send(FromHub::JobResultAck { job_id });
                        continue;
                    }
                    // Not knowing whether there is a record is not knowing
                    // anything: the result stays with the runner (D19).
                    Err(error) => {
                        eprintln!("herd-hub: reading the record of {job_id} failed: {error}");
                        continue;
                    }
                    Ok(Some(record)) => record,
                };
                // The declared requeue code, before anything is recorded as a
                // verdict (D76). This is **not** how the job ended: the
                // capability it was given had gone, which is a race between a
                // probe and a run, and the answer is a bounded requeue with
                // the capability re-probed — never a 75 handed to the
                // requester as a result.
                if is_the_requeue_code(&known, &outcome) {
                    requeue_for_a_lost_capability(state, outbox, &known, attempt).await?;
                    continue;
                }
                // The log before the end. A job that ended having written more
                // chunks than this hub holds is the lost-tail case, and the
                // end is not recorded on a log known to be short: the missing
                // output is asked for first, once. Asked twice it is gone, and
                // then it is named (D29) rather than waited for for ever.
                if !the_log_is_whole(state, outbox, &job_id, attempt, chunks_sent) {
                    continue;
                }
                // Recorded first, acknowledged second. The runner is holding
                // this result until the acknowledgement arrives, so the order
                // of these two steps is the whole of E-44's promise: nothing
                // tells the runner to forget a result that is not yet on a
                // disk.
                if !record_the_end(state, &job_id, &outcome, &metrics).await {
                    // The hub could not write it. Nothing is acknowledged, so
                    // the runner keeps holding it; the hub goes on trying in
                    // the background and never destroys it (D19).
                    continue;
                }
                state.attempt_is_over(&job_id);
                let _ = outbox.send(FromHub::JobResultAck {
                    job_id: job_id.clone(),
                });
                for follower in state.stop_following(&job_id) {
                    let _ = follower.send(FromHub::JobFinished {
                        job_id: job_id.clone(),
                        outcome: outcome.clone(),
                        metrics: metrics.clone(),
                    });
                }
                // A job that ends gives back whatever it was holding, and
                // something queued may have been waiting for exactly that
                // (E-22, E-25). Before M7 this trigger was not needed —
                // nothing was counted, so a queued job was never waiting for
                // a slot — and without it a one-slot machine would run one
                // job and then stand still until the next thing happened to
                // wake the queue.
                dispatch_queued_jobs(state)?;
            }
            ToHub::AttemptAbandoned {
                job_id,
                attempt,
                reason,
            } => {
                if !is_current(state, name, outbox, &job_id, attempt) {
                    continue;
                }
                println!("herd-hub: attempt {attempt} of {job_id} was abandoned: {reason}");
                if let Err(error) = give_the_job_another_go(state, &job_id, attempt, &reason) {
                    eprintln!("herd-hub: {job_id} could not be requeued: {error:#}");
                }
            }
            other => {
                let _ = outbox.send(FromHub::Refused {
                    reason: format!("a runner may not send {}", message_kind(&other)),
                });
                return Ok(());
            }
        }
    }
}

/// Whether a frame about an attempt is one the hub will act on, telling the
/// runner by name when it is not (E-21).
///
/// `from` is the name the connection announced in its handshake, and it is
/// half the gate: an attempt belongs to the runner it was handed to, so a
/// verdict, a log chunk or a health report about it from anybody else is
/// refused and said out loud. Without that half, a `job_finished` frame
/// naming a live attempt is believed whoever sends it — the hub records the
/// exit code, writes metrics it was handed, and leaves the record naming the
/// runner that was really running the job.
fn is_current(state: &State, from: &str, outbox: &Outbox, job_id: &JobId, attempt: u32) -> bool {
    match state.standing_of(job_id, attempt, from) {
        // Every frame about the attempt that is current — a start, a chunk, an
        // end, a health report naming it — is word that the attempt is alive,
        // and that is what the watchdog times (E-21).
        Ok(Standing::Current) => {
            state.attempt_was_heard_of(job_id);
            true
        }
        Ok(Standing::Superseded) => {
            let reason = format!(
                "attempt {attempt} of {job_id} is not the attempt this hub is waiting for; \
                 nothing of it is recorded"
            );
            eprintln!("herd-hub: {reason}");
            let _ = outbox.send(FromHub::StaleAttempt {
                job_id: job_id.clone(),
                attempt,
                reason,
            });
            false
        }
        // A record that is over. Refused by name, with the state it reached:
        // whoever sent this is holding something about a job whose story is
        // told, and the one thing that must not happen is for it to be
        // appended to a log a reader has already been given as complete.
        Ok(Standing::JobIsOver { state }) => {
            let reason = a_frame_about_a_job_that_is_over_reason(job_id, &state, attempt);
            eprintln!("herd-hub: {reason}");
            let _ = outbox.send(FromHub::StaleAttempt {
                job_id: job_id.clone(),
                attempt,
                reason,
            });
            false
        }
        // Somebody else's attempt. Refused by name, and the clock on the real
        // attempt is untouched: a frame from the wrong runner is not evidence
        // that the attempt is alive, and letting it count would have a
        // bystander keep a dead attempt from ever being requeued.
        Ok(Standing::AnotherRunner { holder }) => {
            let reason = not_this_runner_s_attempt_reason(job_id, attempt, &holder, from);
            eprintln!("herd-hub: {reason}");
            let _ = outbox.send(FromHub::StaleAttempt {
                job_id: job_id.clone(),
                attempt,
                reason,
            });
            false
        }
        // Not knowing is not a licence to write: the frame is left alone and
        // the runner keeps holding whatever it was about.
        Err(error) => {
            eprintln!("herd-hub: reading which attempt {job_id} is on failed: {error}");
            false
        }
    }
}

/// What the hub does about one job a health report mentions: tell the runner
/// if the attempt is not its to report on, and ask for output the hub is
/// short of (E-40). False when the entry is refused, so that the hub's
/// picture of this runner holds only the attempts it was really given.
///
/// A shortfall at the instant of a report is the ordinary case, not a loss: a
/// chunk is in flight, or is being written, every time a busy job is reported
/// on. So the hub asks only for a shortfall that has *survived* a report —
/// nothing written between two of them, with the runner saying both times
/// that it has sent more. Asking on sight would have the hub ask for output
/// it is in the middle of recording, every fifteen seconds, for every running
/// job.
fn mind_a_reported_job(state: &State, from: &str, outbox: &Outbox, job: &RunningJob) -> bool {
    if !is_current(state, from, outbox, &job.job_id, job.attempt) {
        return false;
    }
    let recorded = match state.store.chunks_recorded(&job.job_id) {
        Ok(recorded) => recorded,
        Err(error) => {
            eprintln!(
                "herd-hub: cannot tell how much of {}'s log is written: {error}",
                job.job_id
            );
            return true;
        }
    };
    if recorded >= job.chunks_sent {
        state.with_peers(|peers| peers.short_log.remove(&job.job_id));
        return true;
    }
    let stuck = state.with_peers(|peers| {
        peers
            .short_log
            .insert(job.job_id.clone(), recorded)
            .is_some_and(|before| before == recorded)
    });
    if !stuck {
        return true;
    }
    println!(
        "herd-hub: runner has sent {} chunk(s) of {} and this hub still has {}; asking \
         for the rest",
        job.chunks_sent, job.job_id, recorded
    );
    let _ = outbox.send(FromHub::ResendLog {
        job_id: job.job_id.clone(),
        attempt: job.attempt,
        from_seq: recorded,
    });
    true
}

/// Whether the job's log holds everything the attempt says it wrote.
///
/// This is the case M2 could not close. Chunks lost with nothing after them
/// leave the log short with nothing to notice it by — the next chunk is what
/// reveals a gap, and there is no next chunk. The end of the job carries the
/// attempt's total, so the hub can see the shortfall at the one moment it
/// still can: it asks for the output back before recording an end, and only
/// if it is asked for in vain does the log say it is short.
fn the_log_is_whole(
    state: &State,
    outbox: &Outbox,
    job_id: &JobId,
    attempt: u32,
    chunks_sent: u64,
) -> bool {
    let recorded = match state.store.chunks_recorded(job_id) {
        Ok(recorded) => recorded,
        Err(error) => {
            eprintln!("herd-hub: cannot tell how much of {job_id}'s log is written: {error}");
            return true;
        }
    };
    if recorded >= chunks_sent {
        return true;
    }
    let asked_before = state.with_peers(|peers| !peers.tail_asked.insert(job_id.clone()));
    if !asked_before {
        println!(
            "herd-hub: {job_id} ended having written {chunks_sent} chunk(s) and this hub \
             holds {recorded}; asking the runner for the tail before recording the end"
        );
        let _ = outbox.send(FromHub::ResendLog {
            job_id: job_id.clone(),
            attempt,
            from_seq: recorded,
        });
        return false;
    }
    // Asked, and the runner does not have it. The log is short, and says so.
    if let Err(error) = state
        .store
        .name_missing_log(job_id, recorded, chunks_sent - 1)
    {
        eprintln!("herd-hub: naming the missing tail of {job_id} failed: {error}");
    }
    true
}

/// Write a job's end to the disk, retrying briefly, and say whether it is
/// written.
///
/// A result is the one thing a runner cannot reproduce, and the hub failing to
/// store it is the hub's problem, never the runner's: it is never acknowledged
/// unless it is written, the runner is never dropped for it, and the reason is
/// said in full so that an operator can act on it (D19).
///
/// The tries here are the fast path — three of them, inside a second — for the
/// failure that passes while the frame is in hand. What M2 did next was give
/// up and leave the resolution to a link bouncing, which is not a policy; what
/// happens now is that the end is handed to
/// [`keep_trying_to_write_results`], which goes on trying on a growing delay
/// and never stops. There is no give-up, because the only alternative to
/// writing a result eventually is losing it.
async fn record_the_end(
    state: &State,
    job_id: &JobId,
    outcome: &JobOutcome,
    metrics: &JobMetrics,
) -> bool {
    const FAST_TRIES: u32 = 3;
    let end = JobState::Finished {
        outcome: outcome.clone(),
    };
    let mut why = String::new();
    for attempt in 1..=FAST_TRIES {
        // The measurements go down before the transition, so that a reader
        // who sees `finished` is entitled to find the figures beside it
        // (E-15) — and inside the same try, because a hub that could not
        // write what a runner measured must not acknowledge the result
        // either. Failing to store either half is the hub's problem, never
        // the runner's, and never a reason to drop it (D19).
        match state
            .store
            .record_metrics(job_id, metrics.clone())
            .and_then(|()| state.store.record_state(job_id, end.clone()))
        {
            Ok(()) => {
                state.stall_is_over(job_id);
                return true;
            }
            Err(error) => {
                why = error.to_string();
                eprintln!(
                    "herd-hub: recording the outcome of {job_id} failed \
                     (try {attempt} of {FAST_TRIES}): {error}"
                );
                if attempt < FAST_TRIES {
                    tokio::time::sleep(state.policy.write_retry_first).await;
                }
            }
        }
    }
    eprintln!(
        "herd-hub: {job_id} ended with {outcome:?} and this hub cannot write it. \
         The result is NOT acknowledged: the runner still holds it, and this hub \
         goes on trying to write it for as long as it takes. Make the state root \
         writable."
    );
    // The record will go on reading `running`, or `assigned`, with nothing
    // happening to it at all. Whoever is waiting for this job hears why, and
    // so does whoever waits for it later: being mute about a job that is
    // going nowhere is not a policy (D19).
    let note = stalled_note(&why);
    for follower in state.note_a_stall(job_id, why, outcome, metrics) {
        let _ = follower.send(FromHub::JobNote {
            job_id: job_id.clone(),
            note: note.clone(),
        });
    }
    false
}

/// Go on trying to write the ends this hub could not write, for as long as it
/// takes (D19).
///
/// The delay grows to a cap and stays there; there is no attempt count and no
/// give-up. A full disk is fixed in minutes or in days, and either way the
/// outcome in memory here is the only copy besides the one the runner is
/// holding — so the one thing this task may never do is stop.
async fn keep_trying_to_write_results(state: Arc<State>) {
    let mut delay = state.policy.write_retry_first;
    loop {
        tokio::time::sleep(delay).await;
        let pending: Vec<(JobId, JobOutcome, JobMetrics)> = state.with_peers(|peers| {
            peers
                .unwritten
                .iter()
                .map(|(id, (outcome, metrics))| (id.clone(), outcome.clone(), metrics.clone()))
                .collect()
        });
        if pending.is_empty() {
            delay = state.policy.write_retry_first;
            continue;
        }
        let mut wrote_something = false;
        for (job_id, outcome, metrics) in pending {
            let end = JobState::Finished {
                outcome: outcome.clone(),
            };
            match state
                .store
                .record_metrics(&job_id, metrics.clone())
                .and_then(|()| state.store.record_state(&job_id, end))
            {
                Ok(()) => {
                    wrote_something = true;
                    state.stall_is_over(&job_id);
                    state.attempt_is_over(&job_id);
                    println!(
                        "herd-hub: {job_id} is written at last: {outcome:?}. The runner is \
                         told it may forget it."
                    );
                    if let Some(runner) = state.runner_holding(&job_id) {
                        let _ = runner.send(FromHub::JobResultAck {
                            job_id: job_id.clone(),
                        });
                    }
                    for follower in state.stop_following(&job_id) {
                        let _ = follower.send(FromHub::JobFinished {
                            job_id: job_id.clone(),
                            outcome: outcome.clone(),
                            metrics: metrics.clone(),
                        });
                    }
                    // Written at last, so what it was holding is given back
                    // at last: whatever was waiting for that slot can go.
                    if let Err(error) = dispatch_queued_jobs(&state) {
                        eprintln!(
                            "herd-hub: judging the queue after {job_id} was written failed: \
                             {error:#}"
                        );
                    }
                }
                Err(error) => {
                    eprintln!("herd-hub: {job_id} still cannot be written: {error}");
                }
            }
        }
        delay = if wrote_something {
            state.policy.write_retry_first
        } else {
            (delay * 2).min(state.policy.write_retry_cap)
        };
    }
}

/// Watch for attempts nothing is being heard about, and give their jobs one
/// more go (E-21).
///
/// What is timed is **an attempt**, never "is a runner connected". Three cases
/// made that the only model that works. A runner killed and replaced by
/// another leaves its attempt dead while the hub has a connected, healthy
/// runner. A runner killed and restarted comes back itself, healthy, with no
/// memory of the job it was running — and it is the *job* that is gone, not
/// the machine. And a hub that restarts inherits attempts it has never heard
/// a word about. So the clock is per attempt, anything the runner says about
/// it is evidence, and a health report naming it is evidence too, which is
/// what that list in the report is for.
///
/// A connected runner that has stopped reporting is also dropped here, which
/// is a separate thing: a link nothing comes back on is not a link, and
/// holding it would keep the slot and refuse the runner that is trying to
/// take its place.
async fn watch_for_a_lost_runner(state: Arc<State>) {
    let mut ticker = tokio::time::interval(state.policy.watchdog_every());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;

        // Every connection nothing has come back on for the threshold:
        // dropped, so that the name is free for the runner that is dialling.
        let silent_links = state.with_peers(|peers| {
            let silent: Vec<(String, Duration)> = peers
                .runners
                .values()
                .filter(|runner| runner.last_health.elapsed() > state.policy.lost_after)
                .map(|runner| (runner.name.clone(), runner.last_health.elapsed()))
                .collect();
            for (name, _) in &silent {
                if let Some(runner) = peers.runners.remove(name) {
                    let _ = runner.kick.send(true);
                }
            }
            silent
        });
        for (name, quiet_for) in silent_links {
            eprintln!(
                "herd-hub: dropping the connection to runner {name}: nothing has come back \
                 on it for {quiet_for:?}"
            );
        }

        let in_the_air = attempts_in_the_air(&state);
        if in_the_air.is_empty() {
            continue;
        }
        for (job_id, unheard_for) in state.unheard_of_for(&in_the_air) {
            if unheard_for <= state.policy.lost_after {
                continue;
            }
            let Ok(Some(record)) = state.store.job(&job_id) else {
                continue;
            };
            let attempt = record.attempt_number();
            if attempt == 0 {
                continue;
            }
            let runner = record
                .attempt()
                .map(|attempt| attempt.runner.clone())
                .unwrap_or_default();
            let reason = format!(
                "runner {runner} stopped answering: nothing about attempt {attempt} of this \
                 job for {unheard_for:?}, so it is not coming back"
            );
            if let Err(error) = give_the_job_another_go(&state, &job_id, attempt, &reason) {
                eprintln!("herd-hub: {job_id} could not be requeued: {error:#}");
            }
        }
    }
}

/// End an attempt that produced no verdict, and give the job one more go —
/// or, if it has had its goes, end it by name (E-21).
///
/// The job goes back to where it was in the queue: priority levels and aging
/// arrive in M6, and until then "its existing place" is what the durable queue
/// already gives, oldest first.
fn give_the_job_another_go(
    state: &State,
    job_id: &JobId,
    attempt: u32,
    reason: &str,
) -> Result<()> {
    let Some(record) = state
        .store
        .job(job_id)
        .with_context(|| format!("reading the record of {job_id}"))?
    else {
        return Ok(());
    };
    if record.state.is_terminal() {
        return Ok(());
    }
    // Only the attempt that is current can be given up on, and only once. Two
    // callers reach here — the runner abandoning an attempt and the watchdog
    // declaring it lost — and they can arrive about the same attempt at the
    // same moment. Without this, the second one would requeue the job *again*,
    // off the back of an attempt that has already been replaced, and a job
    // allowed two goes would get three.
    if !record.attempt_is_current(attempt) {
        return Ok(());
    }
    // How many goes a *runner* has taken away from this job already — read off
    // the record, not off the attempt number (D79, D76). An attempt that ended
    // because the job answered its declared requeue code is not one of these:
    // a leased machine must not spend the goes that exist for a machine that
    // disappears, and before this was counted separately three 75s left a job
    // with nothing left for the case E-21 is about.
    let goes_taken = record.goes_a_runner_took();
    state
        .store
        .end_attempt(job_id, attempt, reason, None)
        .with_context(|| format!("ending attempt {attempt} of {job_id}"))?;
    state.attempt_is_over(job_id);
    // Whatever that attempt wrote and the hub never received is gone with it:
    // the runner held it in memory. Said in the log, at the place where it is
    // missing, rather than left to be noticed by its absence (D29).
    name_what_that_attempt_lost(
        state,
        job_id,
        record
            .attempt()
            .map_or("", |attempt| attempt.runner.as_str()),
    );

    if goes_taken + 1 < state.policy.attempts_allowed {
        state
            .store
            .record_state(job_id, JobState::Queued)
            .with_context(|| format!("requeueing {job_id}"))?;
        println!(
            "herd-hub: {job_id} is queued again for attempt {} of {} at a runner that went \
             away: {reason}",
            goes_taken + 1,
            state.policy.attempts_allowed
        );
        for follower in state.followers_of(job_id) {
            let _ = follower.send(FromHub::JobNote {
                job_id: job_id.clone(),
                note: format!(
                    "attempt {attempt} of this job ended without a verdict ({reason}); it is \
                     queued again as attempt {}. This wait carries on.",
                    attempt + 1
                ),
            });
        }
        // There may be a runner here already — the one that replaced the one
        // that went, or another machine of the fleet entirely — in which case
        // the second attempt starts at once. It goes through the one dispatch
        // pass like everything else (E-20): a requeued job is placed by the
        // claim step, with its own explanation, not handed back to the
        // machine it was on.
        dispatch_queued_jobs(state)?;
        return Ok(());
    }

    // Out of goes. An end, and a named one: not an exit code, because no
    // process gave one (D15).
    let outcome = JobOutcome::Lost {
        reason: format!(
            "{reason}; this was attempt {} of {} at a runner that went away, so the job is not \
             attempted again (E-21). A go spent on a capability that had gone under it would \
             not have counted towards these (D76)",
            goes_taken + 1,
            state.policy.attempts_allowed
        ),
    };
    state
        .store
        .record_state(
            job_id,
            JobState::Finished {
                outcome: outcome.clone(),
            },
        )
        .with_context(|| format!("recording {job_id} as lost"))?;
    eprintln!("herd-hub: {job_id} is lost: {reason}");
    for follower in state.stop_following(job_id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            // Whatever the record holds, which for a job no runner ever
            // reported on is nothing at all — and nothing is what is sent,
            // never a zero (D23).
            metrics: record.metrics.clone(),
        });
    }
    Ok(())
}

/// Whether this is the job's own declared "the capability I was given went
/// away" code, rather than a verdict (D76).
///
/// Only an [`JobOutcome::Exited`] can be: a signal, a cap breach and a
/// failure to start are not exit codes, and a job whose requester declared no
/// such code has every code of its own passed through untouched (D20).
fn is_the_requeue_code(record: &JobRecord, outcome: &JobOutcome) -> bool {
    match (record.requeue_exit_code, outcome) {
        (Some(declared), JobOutcome::Exited { code }) => *code == declared,
        _ => false,
    }
}

/// Give a job another go at a capability that turned out to be taken, or end
/// it naming the code and the machines that answered it (D76).
///
/// On its **own** budget. E-21's two goes exist for a runner that disappears,
/// and a leased machine answering 75 three times must not spend them — so the
/// two are counted from different attempts of the same record
/// ([`JobRecord::requeues_spent`], [`JobRecord::goes_a_runner_took`]) and
/// neither can reach the other's.
///
/// The result is acknowledged, because the runner is holding it and it is not
/// a verdict anybody wants: the attempt is over, not the job.
async fn requeue_for_a_lost_capability(
    state: &State,
    outbox: &Outbox,
    record: &JobRecord,
    attempt: u32,
) -> Result<()> {
    let job_id = &record.id;
    let code = record.requeue_exit_code.unwrap_or_default();
    let runner = record
        .attempt()
        .map(|attempt| attempt.runner.clone())
        .unwrap_or_default();
    let spent = record.requeues_spent();
    let reason = format!(
        "it exited {code} on {runner}, the code this job declares for a capability that has \
         gone (--requeue-exit-code); that is not a verdict, so the attempt ends here"
    );
    state
        .store
        .end_attempt(job_id, attempt, &reason, Some(code))
        .with_context(|| format!("ending attempt {attempt} of {job_id}"))?;
    state.attempt_is_over(job_id);
    let _ = outbox.send(FromHub::JobResultAck {
        job_id: job_id.clone(),
    });
    // The reading that let this job through is known to be stale: the
    // capability was free when the probe last said so and taken by the time
    // the job ran. Asked for again before the next pass, so the budget is not
    // spent three times against one stale answer.
    let _ = outbox.send(FromHub::ReadProbesAgain {
        why: format!("{job_id} exited {code} on {runner}, so what it was given is held by \
                      something outside Herd after all"),
    });
    if spent + 1 < state.policy.requeue_budget {
        state
            .store
            .record_state(job_id, JobState::Queued)
            .with_context(|| format!("requeueing {job_id}"))?;
        println!(
            "herd-hub: {job_id} is queued again, having spent {} of its {} goes at a \
             capability that went away: {reason}",
            spent + 1,
            state.policy.requeue_budget
        );
        for follower in state.followers_of(job_id) {
            let _ = follower.send(FromHub::JobNote {
                job_id: job_id.clone(),
                note: format!(
                    "{reason}. It is queued again, having spent {} of its {} goes at a \
                     capability that went away, and the capability is being read again. This \
                     wait carries on.",
                    spent + 1,
                    state.policy.requeue_budget
                ),
            });
        }
        dispatch_queued_jobs(state)?;
        return Ok(());
    }
    // The budget is spent. An end, naming the code and every machine that
    // answered it — which is the sentence an operator acts on: it says the
    // lease is held, and by how many machines it has been tried.
    let answered = record.runners_that_requeued_it();
    let outcome = JobOutcome::Lost {
        reason: format!(
            "every go at a capability this job needs came back {code}, the code it declares \
             for a capability that has gone: {} of {} goes spent, on {}. Something outside \
             Herd is holding what it needs, and Herd has no verdict for it",
            spent + 1,
            state.policy.requeue_budget,
            if answered.is_empty() {
                runner.clone()
            } else {
                answered.join(", ")
            }
        ),
    };
    state
        .store
        .record_state(
            job_id,
            JobState::Finished {
                outcome: outcome.clone(),
            },
        )
        .with_context(|| format!("recording {job_id} as out of requeues"))?;
    eprintln!("herd-hub: {job_id} has spent its requeue budget: {reason}");
    for follower in state.stop_following(job_id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            metrics: record.metrics.clone(),
        });
    }
    Ok(())
}

/// Name the output an attempt took with it, if the hub knows there was any.
///
/// The last health report said how many chunks the runner had sent; what the
/// hub has written is what it has. The difference went with the runner, and a
/// log short of it says so (D29).
///
/// `holder` is the runner the attempt was handed to, and the report is only
/// read if it came from that runner: a report naming a job somebody else is
/// running would otherwise put a gap on that job's record on the strength of
/// a number the machine running it never said.
fn name_what_that_attempt_lost(state: &State, job_id: &JobId, holder: &str) {
    let sent = state.with_peers(|peers| {
        peers
            .runners
            .get(holder)
            .into_iter()
            .flat_map(|runner| runner.jobs.iter())
            .find(|job| &job.job_id == job_id)
            .map(|job| job.chunks_sent)
    });
    let Some(sent) = sent else {
        return;
    };
    let Ok(recorded) = state.store.chunks_recorded(job_id) else {
        return;
    };
    if recorded < sent {
        if let Err(error) = state.store.name_missing_log(job_id, recorded, sent - 1) {
            eprintln!("herd-hub: naming what {job_id} lost failed: {error}");
        }
    }
}

/// What a wait is told about a job the hub cannot write down.
///
/// Neither a refusal nor a verdict: the result exists, the runner is holding
/// it, and this hub cannot put it on its disk (D19). One sentence in one
/// place, so a test can rely on it.
fn stalled_note(why: &str) -> String {
    format!(
        "this job is not over and the hub cannot write it down: {why}. \
         Nothing is happening to its record until that is fixed; the runner \
         holds the result and redelivers it on its next connection."
    )
}

/// What a requester asking about a quarantined job is told, if it is one.
///
/// D52 quarantines a record whose labels today's rules reject: the job is on
/// the disk, named at every start, skipped, and never acted on. Every store
/// method that would act on it errors — and an error here used to travel up
/// out of the connection, so the requester got **no frame at all**: the hub
/// put the reason on its own stderr, the socket went away, and `herdctl` said
/// "the hub closed the connection while j… was still running", which is false
/// twice over. The job is not running and the connection did not just happen
/// to close.
///
/// So it is a refusal, of exactly D13's shape — terminal, last, carrying the
/// sentence a human reads — which is what `herdctl` turns into 125 with the
/// reason on stderr (D20). A quarantine is terminal by construction: waiting
/// longer will not lift it, and only a change to the rules or to the record
/// can.
fn quarantine_refusal(state: &State, job_id: &JobId) -> Result<Option<String>> {
    let quarantined = state
        .store
        .quarantine_reason(job_id)
        .with_context(|| format!("reading the record of {job_id}"))?;
    Ok(quarantined.map(|why| {
        format!(
            "{job_id} is quarantined and this hub will not act on it: {why}. \
             It is skipped rather than failed or repaired: its record is left \
             exactly as it is, and nothing will dispatch it, cancel it or \
             report an end for it on this state root."
        )
    }))
}

async fn serve_requester(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    let outcome = accept_submissions(state, outbox, source).await;
    state.forget_follower(outbox);
    outcome
}

/// What a requester asked for, as the store takes it in.
///
/// One function, used by the submission **and** by the dry run, so that the
/// job E-07 judges and the job a submission makes are the same job down to the
/// last field. Two conversions would be the lie E-07 must not tell.
fn new_job(asked: herd_proto::Asked) -> NewJob {
    NewJob::at(asked.command, asked.labels, asked.priority)
        .with_build_thread_mib(asked.build_thread_mib)
        .with_admission(asked.memory_mib, asked.tmpfs_mib)
        .with_m9(
            asked.deadline_unix_s.map(Timestamp::at_unix_seconds),
            asked.after,
            asked.requeue_exit_code,
        )
}

/// Why the job a submission named as its parent will not do, if it will not
/// (E-10).
///
/// One rule: it has to exist. Everything else about a parent — whether it
/// succeeded, whether it is still going — is judged every pass by the claim
/// step, because it is a fact that changes.
fn why_that_parent_will_not_do(state: &State, after: Option<&JobId>) -> Result<Option<String>> {
    let Some(parent) = after else {
        return Ok(None);
    };
    let known = state
        .store
        .job(parent)
        .with_context(|| format!("reading the record of {parent}"))?;
    if known.is_some() {
        return Ok(None);
    }
    Ok(Some(format!(
        "this job starts after {parent}, and this hub has no record of {parent}. A parent has \
         to exist before the job that depends on it is submitted: ids are minted here, in \
         order, so a job can only ever depend on one older than itself — which is also why no \
         job can name itself and no two jobs can name each other (E-10)"
    )))
}

/// Where a job **would** run, said in lines, with nothing happening (E-07).
///
/// The whole answer comes out of one dispatch pass that is thrown away. The
/// fleet is built exactly as [`dispatch_a_pass`] builds it, the queue is
/// ordered by exactly the same function, every job ahead of the hypothetical
/// one is placed against the same seats so that the slots and the room it
/// would really find are the slots and the room it finds here — and then the
/// seats, which are values, go out of scope.
///
/// **Nothing is written.** No id is minted (the record carries one that cannot
/// be minted), no state is recorded, no reservation is taken anywhere but in
/// these local seats, and no event is appended. The claim lock is held for the
/// same reason a real pass holds it: a probe snapshot landing halfway through
/// would make the answer describe two different fleets.
fn dry_run(state: &State, asked: herd_proto::Asked) -> Result<Vec<String>> {
    let _one_pass = state
        .dispatching
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let unfinished = state
        .store
        .unfinished_jobs()
        .context("reading the queue a dry run is judged against")?;
    let held = matcher::holdings(&unfinished);
    let reserved = matcher::reserved(&unfinished);
    let mut seats = state.seats(&held, &reserved);
    let now = Timestamp::now();
    let hypothetical = JobRecord::hypothetical(new_job(asked), now);
    let mut queue = queue::order(unfinished.clone(), now, state.policy.aging_step);
    // Inserted where the queue's own ordering function puts it, so "what is
    // queued ahead" is what would really be ahead of it (E-07 asks for that
    // in so many words).
    queue.push(hypothetical.clone());
    let queue = queue::order(queue, now, state.policy.aging_step);
    let held_empty = matcher::reservations(&queue, &seats, now, state.policy.drain_first_limit);
    for seat in seats.iter_mut() {
        seat.reserved_for = held_empty.get(&seat.name).cloned();
    }
    let mut lines = Vec::new();
    let mut verdict = None;
    for (place, record) in queue.iter().enumerate() {
        let placing = matcher::Placing {
            parent: parent_of(state, record, &unfinished),
            drain_first_limit: Some(state.policy.drain_first_limit),
        };
        if record.id == hypothetical.id {
            let eligible = matcher::eligible(record, &seats, now, &placing);
            lines.push(if eligible.is_empty() {
                "would run on: nothing connected to this hub could run it".to_owned()
            } else {
                format!(
                    "would run on: {}",
                    eligible
                        .iter()
                        .map(|(runner, labels)| format!("{runner} ({labels})"))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            });
            lines.push(format!(
                "queued {} of {}; {}",
                place + 1,
                queue.len(),
                queue::what_is_ahead(&queue, place, now, state.policy.aging_step)
            ));
            verdict = Some(matcher::place(
                record,
                &mut seats,
                now,
                state.policy.stickiness,
                &placing,
            ));
            break;
        }
        // Everything ahead of it, placed against these same seats and
        // discarded: a dry run that judged an empty fleet would promise a
        // slot the jobs in front of it are about to take.
        let _ = matcher::place(record, &mut seats, now, state.policy.stickiness, &placing);
    }
    lines.push(match verdict {
        Some(Placement::Claim { why, .. }) => {
            format!("it would start now, {why}")
        }
        Some(Placement::Wait { why }) => format!("it would wait: {why}"),
        Some(Placement::Impossible { why }) => {
            format!("it would be refused and never run: {why}")
        }
        Some(Placement::Cancel { why }) => format!("it would end at once: {why}"),
        // Unreachable: the hypothetical job is in the queue this loop walks.
        None => "this hub could not judge it".to_owned(),
    });
    lines.push(
        "nothing ran: no job id was minted, nothing was written to this hub's state, no slot \
         and no reservation was taken (E-07)"
            .to_owned(),
    );
    Ok(lines)
}

/// Move one runner's switch, and say what happened (E-33).
///
/// The hub asks; the runner is the authority. It applies the position, writes
/// it to its own file so a restart keeps it, and says so on its next report —
/// which is what makes the two ways of setting a switch leave one state
/// behind.
fn set_a_switch(state: &State, runner: &str, switch: herd_proto::Switch) -> String {
    let why = format!("set through this hub to {}", switch.sentence());
    let found = state.with_peers(|peers| {
        peers.runners.get(runner).map(|connected| {
            let _ = connected.outbox.send(FromHub::SetSwitch {
                switch: switch.clone(),
                why: why.clone(),
            });
            connected.switch.clone()
        })
    });
    match found {
        Some(was) => format!(
            "runner {runner} is asked to switch to {} (it was {was}); it honours this within \
             ten seconds and its own report is what confirms it (E-33)",
            switch.sentence()
        ),
        None => format!(
            "no runner called {runner} is connected to this hub, so its switch cannot be \
             moved from here. An owner sets it on the machine itself, with \
             `herd-runner switch`, which works whether this hub is up or not"
        ),
    }
}

async fn accept_submissions(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    loop {
        // A frame this hub cannot *decode* gets a sentence, not a dropped
        // socket (D13, extended past the handshake by D58). The mistake this
        // closes is a requester naming a priority that is not one of the
        // four: `--priority urgnet` reached here as a frame with an unknown
        // variant in it, and the connection used to die with the reason on
        // the hub's own stdout, where the person who typed it never looks.
        let text = match next_text(source).await? {
            Some(text) => text,
            None => break,
        };
        let message = match from_frame::<ToHub>(&text) {
            Ok(message) => message,
            Err(error) => {
                let reason = format!("this hub could not read what you sent: {error}");
                eprintln!("herd-hub: a requester sent a frame that would not read: {error}");
                let _ = outbox.send(FromHub::Refused { reason });
                return Ok(());
            }
        };
        match message {
            ToHub::SubmitJob { asked } => {
                if asked.command.is_empty() {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "a job needs a command to run".to_owned(),
                    });
                    return Ok(());
                }
                if let Some(reason) = state.why_no_runner() {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                // The one job it named has to exist **already** (E-10).
                // Refused here, at the submission, rather than left to be
                // cancelled later, because a requester naming a job that is
                // not there has made a mistake and is standing right here to
                // be told — and because this is what makes a cycle
                // unrepresentable: ids are minted in order by this hub, so a
                // job can only ever name one older than itself, and a job
                // cannot name itself at all.
                if let Some(reason) = why_that_parent_will_not_do(state, asked.after.as_ref())? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                let submission = state
                    .store
                    .submit(new_job(asked))
                    .context("taking a submission in")?;
                let job_id = submission.record().id.clone();
                // JobAccepted first, so the requester has the id before any
                // chunk of its log can reach it — and for a submission that
                // joined, before the sentence explaining the id it was given.
                let _ = outbox.send(FromHub::JobAccepted {
                    job_id: job_id.clone(),
                });
                match &submission {
                    Submission::Minted(_) => state.follow(job_id.clone(), outbox),
                    Submission::Joined { record, raised } => {
                        println!(
                            "herd-hub: a submission joined {job_id}, which is {} and \
                             answers {} submission(s) now",
                            record.state.name(),
                            record.submissions
                        );
                        let mut note = format!(
                            "this submission asked for work {job_id} is already doing, \
                             so it joined that job rather than running it twice: same \
                             command, same labels. One execution, one result, for both \
                             callers"
                        );
                        if let Some(to) = raised {
                            note.push_str(&format!(
                                ". It was the higher priority, so {job_id} is now at \
                                 priority {to}"
                            ));
                        }
                        let _ = outbox.send(FromHub::JobNote {
                            job_id: job_id.clone(),
                            note,
                        });
                        // And then exactly what a `wait` on that id would
                        // do: the output it has already produced, a place
                        // in the followers under the same lock, and — if it
                        // ended in the moment since the join — its end. A
                        // joiner that was told nothing until the next chunk
                        // arrived would be a joiner whose log is short of
                        // everything that happened before it joined.
                        answer_a_wait(state, outbox, job_id.clone())?;
                    }
                }
                // And then the job goes through exactly the same gate every
                // other queued job goes through — the one queue, in its own
                // order (E-20). It starts, or it stays queued with its
                // reason on its record and a note on this connection: a
                // submission the runner cannot serve yet is not a submission
                // that failed (D38).
                {
                    let _one_pass = state
                        .dispatching
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    dispatch_a_pass(state)?;
                }
                // A runner that went away between the check above and the
                // pass leaves the job queued with nowhere to go, and the
                // requester is told rather than left watching a job nothing
                // will pick up. Read from the record, because the pass may
                // well have started it.
                let still_queued = state
                    .store
                    .job(&job_id)
                    .with_context(|| format!("reading the record of {job_id}"))?
                    .is_some_and(|record| record.state == JobState::Queued);
                if still_queued {
                    if let Some(reason) = state.why_no_runner() {
                        state.stop_following(&job_id);
                        let _ = outbox.send(FromHub::Refused { reason });
                        return Ok(());
                    }
                }
            }
            // E-07, and the whole of what it must not do: judge, say, and
            // change nothing.
            ToHub::DryRun { asked } => {
                if asked.command.is_empty() {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "a dry run needs a command to judge".to_owned(),
                    });
                    return Ok(());
                }
                let lines = match why_that_parent_will_not_do(state, asked.after.as_ref())? {
                    Some(reason) => vec![format!("it would be refused at submission: {reason}")],
                    None => dry_run(state, asked)?,
                };
                let _ = outbox.send(FromHub::WouldRun { lines });
            }
            ToHub::SetSwitch { runner, switch } => {
                let said = set_a_switch(state, &runner, switch);
                println!("herd-hub: {said}");
                let _ = outbox.send(FromHub::SwitchSet { said });
            }
            ToHub::AskFleetSwitches {} => {
                let _ = outbox.send(FromHub::FleetSwitches {
                    runners: state.fleet_switches(),
                });
            }
            ToHub::RaiseJob { job_id, priority } => {
                if let Some(reason) = quarantine_refusal(state, &job_id)? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                raise_job(state, outbox, &job_id, priority)?;
            }
            ToHub::CancelJob { job_id } => {
                if let Some(reason) = quarantine_refusal(state, &job_id)? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                cancel_job(state, outbox, &job_id)?;
            }
            ToHub::WaitForJob { job_id } => {
                if let Some(reason) = quarantine_refusal(state, &job_id)? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                if !answer_a_wait(state, outbox, job_id)? {
                    return Ok(());
                }
            }
            other => {
                let _ = outbox.send(FromHub::Refused {
                    reason: format!("a requester may not send {}", message_kind(&other)),
                });
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Tell this connection where a job stands, and keep it following until the
/// job ends.
///
/// What `WaitForJob` does — and what a submission that *joined* an open job
/// does too (E-05), because the two are the same question: "I hold this id,
/// tell me everything about it from here on". Writing it once is what
/// guarantees a joiner is given the log the job produced before it joined,
/// and is answered at once if the job ended in the meantime.
///
/// `false` means the job does not exist and the connection has been refused.
fn answer_a_wait(state: &State, outbox: &Outbox, job_id: JobId) -> Result<bool> {
    let attached = state
        .attach(&job_id, outbox)
        .with_context(|| format!("reading the record of {job_id}"))?;
    let Some(Attached {
        record,
        log,
        stalled,
    }) = attached
    else {
        // A terminal no, as D13 means it: this id will not come
        // into being by waiting for it.
        let _ = outbox.send(FromHub::Refused {
            reason: format!("no such job: {job_id}"),
        });
        return Ok(false);
    };
    // The output the job has already produced, before anything
    // else about it. A detached requester has no other way to get
    // a job's log through the CLI (D22).
    if !log.is_empty() {
        let _ = outbox.send(FromHub::JobLog {
            job_id: job_id.clone(),
            data: log,
        });
    }
    let metrics = record.metrics.clone();
    let waiting_for = record.waiting_for.clone();
    match record.state {
        // Answered from the record, which is the point of writing
        // one: a wait works an hour later, on another machine,
        // across a restart of the hub. The measurements come from
        // the record too, so a wait an hour later reads the same
        // figures the requester that followed the job read (E-15).
        JobState::Finished { outcome } => {
            let _ = outbox.send(FromHub::JobFinished {
                job_id,
                outcome,
                metrics,
            });
        }
        // Waiting for a job whose attempt was interrupted is
        // waiting for a runner that may never come back, so the
        // wait says so rather than being silent (D17). It is not a
        // refusal: the runner may still redeliver, and if it does
        // not the job is attempted again — either way this same
        // connection carries the answer.
        JobState::Interrupted { reason } => {
            let _ = outbox.send(FromHub::JobNote {
                job_id,
                note: format!(
                    "this job is not over and its last attempt was \
                     interrupted: {reason}. If the runner comes back, \
                     its result arrives here; if it does not, the job is \
                     attempted again."
                ),
            });
        }
        // Queued, assigned or running — and if this hub knows that
        // the attempt is not progressing because it cannot write
        // the job down, the wait is told so rather than left
        // watching a record that says `running` and never moves.
        _ => {
            if let Some(why) = stalled {
                let _ = outbox.send(FromHub::JobNote {
                    job_id: job_id.clone(),
                    note: stalled_note(&why),
                });
            }
            // And a job that is queued because a capability it
            // needs is suspended says so to a wait that attaches
            // an hour later, from its record: `herdctl wait`
            // must not sit silent on a job nothing is happening
            // to (E-26, E-04).
            if let Some(waiting_for) = waiting_for {
                let _ = outbox.send(FromHub::JobNote {
                    job_id,
                    note: format!(
                        "this job is queued and has not started: \
                         {waiting_for}"
                    ),
                });
            }
        }
    }
    Ok(true)
}

/// Raise one job's level, as an operator may (E-18).
///
/// Four answers and each is said in words, because an operator who asked for
/// a job to go first has to know which of them happened: it moved, it was
/// there already, the level asked for is *below* the one it has, or the job
/// is over and a record that has reached its end is never written to again
/// (D54). Only the last is a refusal; the rest are notes about a job that is
/// in exactly the state the operator can now see.
///
/// A raise that moved the job runs a dispatch pass, because the order of the
/// queue has just changed and the raise would otherwise take effect only at
/// the next thing that happened to the hub.
fn raise_job(state: &State, outbox: &Outbox, job_id: &JobId, to: Priority) -> Result<()> {
    let raised = match state.store.raise_priority(job_id, to) {
        Ok(raised) => raised,
        // The store refuses a job that is over, and one that does not exist,
        // by name. Both are terminal answers to this request (D13): asking
        // again will say the same thing.
        Err(error) => {
            let _ = outbox.send(FromHub::Refused {
                reason: format!("{job_id} cannot be raised to {to}: {error}"),
            });
            return Ok(());
        }
    };
    let note = match raised {
        Raise::Raised { from, to } => {
            println!("herd-hub: {job_id} was raised from {from} to {to} by an operator");
            format!("this job was at priority {from} and is now at {to}")
        }
        Raise::AlreadyThere(at) => {
            format!("this job is already at priority {at}; nothing was changed")
        }
        Raise::NotARaise { from, asked } => format!(
            "this job is at priority {from}, which is above {asked}: a job's priority \
             is only ever raised, never lowered, so nothing was changed"
        ),
    };
    let _ = outbox.send(FromHub::JobNote {
        job_id: job_id.clone(),
        note,
    });
    if matches!(raised, Raise::Raised { .. }) {
        dispatch_queued_jobs(state)?;
    }
    Ok(())
}

/// Cancel one job, wherever it is (E-06).
///
/// Three cases, and each is answered rather than assumed. A job nobody has
/// started is ended here and now, because there is no runner to ask. A job
/// being executed is cancelled on its runner, scoped to that one attempt, so
/// a sibling job on the same machine is untouched; the runner reports the end
/// and the record gets it the ordinary way. A job that is already over is not
/// a failure to report — it is simply too late, said in one sentence.
fn cancel_job(state: &State, outbox: &Outbox, job_id: &JobId) -> Result<()> {
    let Some(record) = state
        .store
        .job(job_id)
        .with_context(|| format!("reading the record of {job_id}"))?
    else {
        let _ = outbox.send(FromHub::Refused {
            reason: format!("no such job: {job_id}"),
        });
        return Ok(());
    };
    let note = match record.state {
        JobState::Finished { .. } => {
            format!("{job_id} is already over, so there was nothing to cancel")
        }
        JobState::Queued => {
            let reason = "a requester cancelled this job before it started".to_owned();
            state
                .store
                .record_state(
                    job_id,
                    JobState::Finished {
                        outcome: JobOutcome::Cancelled {
                            reason: reason.clone(),
                        },
                    },
                )
                .with_context(|| format!("recording {job_id} as cancelled"))?;
            state.attempt_is_over(job_id);
            for follower in state.stop_following(job_id) {
                let _ = follower.send(FromHub::JobFinished {
                    job_id: job_id.clone(),
                    outcome: JobOutcome::Cancelled {
                        reason: reason.clone(),
                    },
                    // A job cancelled before it started cost no machine
                    // anything, so there is nothing measured to send.
                    metrics: JobMetrics::default(),
                });
            }
            format!("{job_id} was queued and is cancelled")
        }
        _ => {
            let attempt = record.attempt_number();
            let reason = "a requester cancelled this job".to_owned();
            match state.runner_holding(job_id) {
                Some(runner) => {
                    let _ = runner.send(FromHub::CancelJob {
                        job_id: job_id.clone(),
                        attempt,
                        reason: reason.clone(),
                    });
                    format!(
                        "attempt {attempt} of {job_id} is being ended on the runner \
                         executing it"
                    )
                }
                // Nothing to propagate to. The attempt is already in the air,
                // and the watchdog is what resolves it; inventing an end here
                // would claim a job was cancelled that is still running
                // somewhere.
                None => format!(
                    "{job_id} is on a runner that is not connected; it is cancelled as soon \
                     as the runner is back, and lost if it never is"
                ),
            }
        }
    };
    println!("herd-hub: {note}");
    let _ = outbox.send(FromHub::JobNote {
        job_id: job_id.clone(),
        note,
    });
    Ok(())
}

/// Pump one connection's outbox onto its socket, then close it.
async fn write_frames(mut sink: Sink, mut inbox: UnboundedReceiver<FromHub>) {
    while let Some(message) = inbox.recv().await {
        match to_frame(&message) {
            Ok(frame) => {
                if sink.send(Message::text(frame)).await.is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("herd-hub: a message would not encode: {error}"),
        }
    }
    let _ = sink.close().await;
}

async fn send(sink: &mut Sink, message: &FromHub) -> Result<()> {
    let frame = to_frame(message).context("encoding a message")?;
    sink.send(Message::text(frame)).await.context("sending")
}

/// Say no, in one frame, and close. Refusal is never silent.
async fn refuse(sink: &mut Sink, reason: String) -> Result<()> {
    send(sink, &FromHub::Refused { reason }).await?;
    let _ = sink.close().await;
    Ok(())
}

/// The next protocol message, or `None` once the peer is done.
async fn next_message(source: &mut Source) -> Result<Option<ToHub>> {
    let Some(text) = next_text(source).await? else {
        return Ok(None);
    };
    let message =
        from_frame(&text).map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
    Ok(Some(message))
}

/// What this hub's accepted sockets are allowed to read: the ceiling, not the
/// bound.
///
/// Set here rather than left to the library's default so that both figures are
/// this project's own and can be read off one line.
/// [`FRAME_CEILING_BYTES`] is where reading stops; [`MAX_FRAME_BYTES`] is what
/// the hub accepts, and it is enforced by [`first_frame`], which has a
/// refusal to send and so has to have the whole frame in hand.
fn frame_limits() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(FRAME_CEILING_BYTES))
        .max_frame_size(Some(FRAME_CEILING_BYTES))
}

/// What the peer's first frame was, as far as this hub could read it.
///
/// Three outcomes and no fourth, because every one of them has to end in a
/// sentence or a clean close — never in an error travelling up out of the
/// connection, which is what left a peer with a reset socket.
enum FirstFrame {
    /// The text of the frame, undecoded.
    Said(String),
    /// Nothing this hub can read, and what is wrong with it.
    Unreadable(String),
    /// The peer closed, or went away, without saying anything.
    Gone,
}

/// Read the first frame, reporting what was wrong with it rather than failing.
///
/// Every way a first frame can be unreadable lands in
/// [`FirstFrame::Unreadable`] with a sentence: a binary frame (D4 says the
/// protocol carries text), a frame past [`MAX_FRAME_BYTES`], a truncated one,
/// a WebSocket-level protocol error. The caller refuses with it, which is the
/// one thing a peer can act on — and the figure travels, so an owner whose
/// tool sends too much learns what it has to stay under instead of guessing
/// from a reset socket.
async fn first_frame(source: &mut Source) -> FirstFrame {
    loop {
        let frame = match source.next().await {
            None => return FirstFrame::Gone,
            Some(Ok(frame)) => frame,
            Some(Err(error)) => return FirstFrame::Unreadable(unreadable_frame_reason(&error)),
        };
        match frame {
            // Past the bound, and refused with the figure. The frame was read
            // to its end — that is what [`FRAME_CEILING_BYTES`] is for —
            // because a refusal nobody receives is the reset this replaces.
            Message::Text(text) if text.len() > MAX_FRAME_BYTES => {
                return FirstFrame::Unreadable(oversized_frame_reason(text.len()))
            }
            Message::Text(text) => return FirstFrame::Said(text.to_string()),
            Message::Close(_) => return FirstFrame::Gone,
            Message::Binary(data) => {
                return FirstFrame::Unreadable(format!(
                    "the protocol carries text frames only (D4), and this peer's first \
                     frame was {} byte(s) of binary",
                    data.len()
                ))
            }
            // A ping before the handshake is ordinary liveness, not a message.
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
}

/// What a peer whose first frame is too big is told, with both figures.
fn oversized_frame_reason(size: usize) -> String {
    format!(
        "this peer's first frame is {size} bytes, and this hub accepts a first \
         frame of at most {MAX_FRAME_BYTES} bytes"
    )
}

/// Why a frame would not read, in words.
///
/// The library's own text for a frame past the ceiling names the sizes it
/// compared but not whose ceiling it is, so it is said here: past
/// [`FRAME_CEILING_BYTES`] the hub stops reading, and this sentence is the one
/// a peer gets if it is still there to read it.
fn unreadable_frame_reason(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Capacity(why) => format!(
            "this frame is past the {FRAME_CEILING_BYTES} bytes this hub reads at all, so \
             it was not read ({why}); a first frame may carry at most \
             {MAX_FRAME_BYTES} bytes"
        ),
        other => format!("this frame would not read: {other}"),
    }
}

/// The text of the next frame that carries any, undecoded.
///
/// Separate from [`next_message`] for one caller: the handshake, which has to
/// be able to say *why* it could not be read rather than drop the connection
/// (D13, and see [`unreadable_handshake_reason`]).
async fn next_text(source: &mut Source) -> Result<Option<String>> {
    while let Some(frame) = source.next().await {
        match frame.context("reading a frame")? {
            Message::Text(text) => return Ok(Some(text.to_string())),
            Message::Close(_) => return Ok(None),
            Message::Binary(_) => bail!("the protocol carries text frames only (D4)"),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
    Ok(None)
}

/// A message's wire tag, for refusal texts.
fn message_kind(message: &ToHub) -> &'static str {
    match message {
        ToHub::Hello { .. } => "hello",
        ToHub::CapabilitiesChanged { .. } => "capabilities_changed",
        ToHub::SubmitJob { .. } => "submit_job",
        ToHub::RaiseJob { .. } => "raise_job",
        ToHub::CancelJob { .. } => "cancel_job",
        ToHub::WaitForJob { .. } => "wait_for_job",
        ToHub::JobStarted { .. } => "job_started",
        ToHub::LogChunk { .. } => "log_chunk",
        ToHub::JobFinished { .. } => "job_finished",
        ToHub::Health { .. } => "health",
        ToHub::AttemptAbandoned { .. } => "attempt_abandoned",
        ToHub::DryRun { .. } => "dry_run",
        ToHub::SetSwitch { .. } => "set_switch",
        ToHub::AskFleetSwitches {} => "ask_fleet_switches",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc as std_mpsc;

    use herd_proto::{
        Availability, BusyProbe, Capability, CapabilityOffer, CapabilityOrigins, LabelSet,
        OfferedCapability,
    };

    use super::*;
    use crate::job::Mint;
    use crate::job::{JobEvent, LogWrite, NewJob, Result as StoreResult};
    use crate::store::JobDirStore;
    use herd_proto::LogStream;

    /// A real store with one method held open on command.
    ///
    /// Everything is the real [`JobDirStore`] — the records, the fsyncs, the
    /// transitions — except that `begin_attempt` can be made to sit inside the
    /// claim before it writes anything. That is how the window this test is
    /// about is forced open: in production it is as long as three fsyncs, and
    /// a test that hoped to hit three fsyncs would be a test that passes by
    /// luck.
    struct StoreThatHoldsTheClaimOpen {
        inner: JobDirStore,
        /// Raised by the dispatch pass when it is inside the window.
        in_the_window: Arc<AtomicBool>,
        /// What the pass waits for before it finishes claiming.
        go_on: Mutex<Option<std_mpsc::Receiver<()>>>,
    }

    impl JobStore for StoreThatHoldsTheClaimOpen {
        fn begin_attempt(
            &self,
            id: &JobId,
            runner: &str,
            why: &str,
            parallelism: Option<u32>,
        ) -> StoreResult<u32> {
            self.in_the_window.store(true, Ordering::SeqCst);
            if let Some(go_on) = self
                .go_on
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                let _ = go_on.recv();
            }
            self.inner.begin_attempt(id, runner, why, parallelism)
        }

        fn state_root_id(&self) -> &str {
            self.inner.state_root_id()
        }
        fn submit(&self, new: NewJob) -> StoreResult<crate::job::Submission> {
            self.inner.submit(new)
        }

        fn raise_priority(&self, id: &JobId, to: Priority) -> StoreResult<crate::job::Raise> {
            self.inner.raise_priority(id, to)
        }
        fn record_waiting(&self, id: &JobId, reason: &str) -> StoreResult<bool> {
            self.inner.record_waiting(id, reason)
        }
        fn record_state(&self, id: &JobId, state: JobState) -> StoreResult<()> {
            self.inner.record_state(id, state)
        }
        fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> StoreResult<()> {
            self.inner.record_metrics(id, metrics)
        }
        fn end_attempt(
            &self,
            id: &JobId,
            attempt: u32,
            reason: &str,
            requeue_code: Option<i32>,
        ) -> StoreResult<()> {
            self.inner.end_attempt(id, attempt, reason, requeue_code)
        }
        fn append_log(
            &self,
            id: &JobId,
            stream: LogStream,
            seq: u64,
            data: &[u8],
        ) -> StoreResult<LogWrite> {
            self.inner.append_log(id, stream, seq, data)
        }
        fn chunks_recorded(&self, id: &JobId) -> StoreResult<u64> {
            self.inner.chunks_recorded(id)
        }
        fn name_missing_log(
            &self,
            id: &JobId,
            first_missing: u64,
            last_missing: u64,
        ) -> StoreResult<()> {
            self.inner.name_missing_log(id, first_missing, last_missing)
        }
        fn job(&self, id: &JobId) -> StoreResult<Option<JobRecord>> {
            self.inner.job(id)
        }
        fn quarantine_reason(&self, id: &JobId) -> StoreResult<Option<String>> {
            self.inner.quarantine_reason(id)
        }
        fn job_log(&self, id: &JobId) -> StoreResult<Option<Vec<u8>>> {
            self.inner.job_log(id)
        }
        fn job_events(&self, id: &JobId) -> StoreResult<Option<Vec<JobEvent>>> {
            self.inner.job_events(id)
        }
        fn unfinished_jobs(&self) -> StoreResult<Vec<JobRecord>> {
            self.inner.unfinished_jobs()
        }
    }

    /// A runner offering `kvm` behind a lock-file probe, read free.
    fn offering_kvm_free() -> RunnerCapacity {
        let offer = CapabilityOffer::new(vec![OfferedCapability::new(
            Capability::shared("kvm").probed_by(BusyProbe::lock_file("/run/herd/kvm")),
            CapabilityOrigins::probed(),
        )])
        .expect("a valid offer");
        let mut capacity = RunnerCapacity::new(Some("the-runner".to_owned()), offer);
        capacity.update(vec![herd_proto::CapabilityState::new(
            "kvm",
            Availability::Free,
        )]);
        capacity
    }

    /// E-26, in the one place it is hardest to see: a probe's snapshot and a
    /// claim judged against the reading before it must not overlap.
    ///
    /// The harm is specific. `offer_job` asks `why_not_now`, is told the
    /// capability is free, and only then claims the job — and claiming it
    /// writes the record, the log counter and a line of `events.jsonl`, each
    /// fsynced. A `capabilities_changed` saying `kvm` is busy used to be
    /// applied the instant it arrived, so for the length of those writes the
    /// hub held two incompatible facts at once and acted on the stale one: a
    /// job dispatched onto a capability the hub already knew an external
    /// holder had taken.
    ///
    /// **How the window is forced open.** Not by timing: the store's
    /// `begin_attempt` is wrapped so that it raises a flag the moment the
    /// pass is inside the claim and then blocks until this test says go on.
    /// Everything else is the real store. The snapshot is delivered from a
    /// second thread *while the pass is in there*, which is the race, and the
    /// thread records — the instant its own call returns — whether the claim
    /// had finished. Under the defect that answer is "no": the snapshot was
    /// applied mid-claim. There is nothing to wait out and nothing to sleep
    /// for.
    #[test]
    fn a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim() {
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let in_the_window = Arc::new(AtomicBool::new(false));
        let (go_on, wait_for_go_on) = std_mpsc::channel();
        let store = Arc::new(StoreThatHoldsTheClaimOpen {
            inner: JobDirStore::open(dir.path()).expect("the state root opens"),
            in_the_window: Arc::clone(&in_the_window),
            go_on: Mutex::new(Some(wait_for_go_on)),
        });
        let labels = LabelSet::new(["kvm"]).expect("valid labels");
        store
            .create_job(NewJob::needing(vec!["true".to_owned()], labels.clone()))
            .expect("minted");

        let state = Arc::new(State {
            store: Arc::clone(&store) as Arc<dyn JobStore>,
            policy: Policy::default(),
            started: Instant::now(),
            peers: Mutex::new(Peers::default()),
            dispatching: Mutex::new(()),
        });
        let (outbox, _inbox) = mpsc::unbounded_channel();
        let (kick, _kicked) = watch::channel(false);
        assert!(matches!(
            state.register_runner(
                "the-runner",
                Some("run-1"),
                &outbox,
                kick,
                Announced {
                    capacity: offering_kvm_free(),
                    host: HostSize::default(),
                    preference: 0,
                    load_milli: None,
                    switch: herd_proto::Switch::On,
                },
            ),
            Admitted::Yes
        ));
        assert_eq!(
            state.seats(&BTreeMap::new(), &BTreeMap::new())[0]
                .capacity
                .why_not_now(&labels),
            None,
            "the reading this claim is about to be judged on is free"
        );

        // The pass. It stops inside the claim and waits to be let go.
        let claim_is_over = Arc::new(AtomicBool::new(false));
        let pass = std::thread::spawn({
            let (state, claim_is_over) = (Arc::clone(&state), Arc::clone(&claim_is_over));
            move || {
                dispatch_queued_jobs(&state).expect("the pass runs");
                claim_is_over.store(true, Ordering::SeqCst);
            }
        });
        while !in_the_window.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }

        // The snapshot, delivered from outside while the claim is open.
        let snapshot = std::thread::spawn({
            let (state, claim_is_over) = (Arc::clone(&state), Arc::clone(&claim_is_over));
            let outbox = outbox.clone();
            move || {
                state.update_capabilities(
                    &outbox,
                    vec![herd_proto::CapabilityState::new(
                        "kvm",
                        Availability::busy("the lock file /run/herd/kvm is there"),
                    )],
                );
                // Read *after* the snapshot was applied, which is the whole
                // assertion: nothing may have been applied while a claim was
                // still open.
                claim_is_over.load(Ordering::SeqCst)
            }
        });

        // Let the claim finish. Both orders of this and the line above are
        // fine: the snapshot thread is either already blocked on the lock or
        // about to be, and neither can be applied before the pass is done.
        go_on.send(()).expect("the pass is still there");
        pass.join().expect("the pass ends");
        let claim_was_over = snapshot.join().expect("the snapshot lands");

        assert!(
            claim_was_over,
            "a probe's snapshot was applied while a claim judged on the \
             reading before it was still open: the hub knew the capability \
             was busy and dispatched anyway (E-26)"
        );
        // And the claim itself stands, judged on the reading that held when
        // it was made. The snapshot governs the next pass, not this one.
        let record = store
            .unfinished_jobs()
            .expect("the queue reads")
            .into_iter()
            .next()
            .expect("the job is still there");
        assert_eq!(record.state, JobState::Assigned);
        assert_eq!(
            state.seats(&BTreeMap::new(), &BTreeMap::new())[0]
                .capacity
                .why_not_now(&labels)
                .as_deref()
                .map(|_| "suspended"),
            Some("suspended"),
            "and the snapshot is in force for whatever comes next"
        );
    }

    /// **Attack 8.** E-04 asks for "at least every 60 s", and that has two
    /// failure modes: a reason that stops, and a reason that floods.
    ///
    /// Ten simulated minutes are driven through the hub's own decision — the
    /// one [`say_again_why_jobs_wait`] calls — at the cadence of a busy
    /// fleet's dispatch passes, two seconds apart. The two assertions are
    /// the two failure modes: there is never a gap longer than the cadence,
    /// and the whole ten minutes produce no more lines than the bound.
    #[test]
    fn ten_simulated_minutes_of_waiting_leave_periodic_reasons_and_no_flood() {
        let restating = Restating::every(WAIT_RESTATEMENT);
        let ten_minutes = Duration::from_secs(10 * 60);

        let mut said_at: Vec<Duration> = Vec::new();
        let mut last_told: Option<Duration> = None;
        let mut elapsed = Duration::ZERO;
        while elapsed <= ten_minutes {
            if restating.is_due(last_told) {
                said_at.push(elapsed);
                last_told = Some(Duration::ZERO);
            }
            elapsed += restating.tick;
            last_told = last_told.map(|since| since + restating.tick);
        }

        // It does not stop, and — this is the half measuring found — **no gap
        // is longer than the cadence at all**, not "the cadence plus a tick".
        // A loop firing when a job was last told `cadence` ago gives gaps of
        // up to `cadence + tick`, and a by-hand ten-minute wait measured one
        // of 61 s against a figure E-04 words as "at least every 60 s".
        assert_eq!(said_at.first(), Some(&Duration::ZERO), "{said_at:?}");
        for pair in said_at.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                gap <= restating.cadence,
                "a gap of {gap:?} is longer than E-04's 60 s: {said_at:?}"
            );
        }
        assert!(
            ten_minutes - *said_at.last().expect("at least one") <= restating.cadence,
            "it stopped before the ten minutes were up: {said_at:?}"
        );

        // And it does not flood. The clock's own bound over ten minutes is
        // fourteen lines; a line per dispatch pass on a busy fleet would be
        // hundreds.
        let bound = restating.at_most_over(ten_minutes);
        assert_eq!(bound, 14);
        assert!(
            said_at.len() as u32 <= bound,
            "{} lines over ten minutes, bound {bound}: {said_at:?}",
            said_at.len()
        );
        assert_eq!(said_at.len(), 14, "{said_at:?}");
    }

    /// A hub whose `serve` future is **dropped** lets its state root go.
    ///
    /// Found as a flake rather than by reasoning, and worth a test of its own
    /// because of how it reads when it happens: `the state root … is already
    /// in use by herd-hub, process N`, naming a hub that nothing is running.
    /// `serve` spawns housekeeping tasks that each hold an `Arc<State>`, and
    /// through it the store's `flock`; the aborts used to live at the end of
    /// `serve`, which a dropped future never reaches. M8's fourth task made
    /// it bite often enough to catch — at about one run in six — and the
    /// aborts now live in a `Drop`.
    #[tokio::test]
    async fn a_dropped_serve_gives_the_state_root_back() {
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"))
                as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        // Let the housekeeping tasks start, then take the future away, as
        // every end-to-end harness in this workspace does.
        tokio::time::sleep(Duration::from_millis(50)).await;
        serving.abort();

        // The root is free again. Polled rather than asserted at once:
        // aborting a task is a request, and the `Arc` it holds goes when the
        // runtime reaps it.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match JobDirStore::open(dir.path()) {
                Ok(_) => break,
                Err(why) => assert!(
                    Instant::now() < deadline,
                    "the hub is gone and its state root is still locked: {why}"
                ),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// **Attack 11, measured.** The `dispatching` lock is held for a whole
    /// pass over the whole queue, including every claim's and every waiting
    /// line's fsyncs — and M8 adds admission arithmetic inside the same pass.
    ///
    /// This is a measurement rather than a bound: it prints the wall-clock
    /// figure for a pass over a large queue, so that the cost is a number in
    /// the record instead of a worry. The assertion is deliberately loose —
    /// it is there to catch something going quadratic, not to pin a figure a
    /// slow CI box cannot meet.
    #[test]
    fn one_pass_over_a_large_queue_is_measured_and_the_figure_is_printed() {
        const QUEUED: usize = 400;
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        for which in 0..QUEUED {
            store
                .create_job(
                    NewJob::new(vec!["true".to_owned(), format!("{which}")])
                        .with_admission(Some(2048), Some(1024)),
                )
                .expect("minted");
        }
        let state = Arc::new(State {
            store: Arc::clone(&store) as Arc<dyn JobStore>,
            policy: Policy::default(),
            started: Instant::now(),
            peers: Mutex::new(Peers::default()),
            dispatching: Mutex::new(()),
        });

        // The first pass writes a waiting line on every record: 400 records,
        // 400 `events.jsonl` lines, every one fsynced. That is the expensive
        // pass, and the one an operator would meet after a restart.
        let began = Instant::now();
        dispatch_queued_jobs(&state).expect("the pass runs");
        let writing = began.elapsed();
        // The second writes nothing, because no sentence is news. That is
        // what a steady-state pass costs.
        let began = Instant::now();
        dispatch_queued_jobs(&state).expect("the pass runs again");
        let steady = began.elapsed();

        println!(
            "herd-hub: one dispatch pass over {QUEUED} queued jobs took {writing:?} writing a \
             waiting line for each, and {steady:?} when none of them was news. The \
             `dispatching` lock is held for all of it."
        );
        assert!(
            writing < Duration::from_secs(120),
            "a pass over {QUEUED} jobs took {writing:?}, which is not a cost this design can \
             carry: something is quadratic"
        );
        assert!(steady < writing, "{steady:?} against {writing:?}");
    }
}
