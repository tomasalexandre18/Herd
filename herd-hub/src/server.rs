//! The hub: one listener, one runner, any number of requesters.
//!
//! Every connection is dialled by the peer (invariant 3) and starts with a
//! `Hello` whose protocol version must match. A requester's `SubmitJob` is
//! minted into a job, handed to the connected runner, and everything the
//! runner says about that job is relayed back to the requester that submitted
//! it. With no runner connected a submission is **queued and explained**, not
//! refused: the one fleet state a submission is refused in is a state root on
//! which no runner was ever approved, which is a configuration mistake and
//! not a fleet that is away (D88, D140, [`State::why_no_runner_is_approved`]).
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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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
    JobOutcome, Priority, Role, RunningJob, ToHub, HEALTH_EVERY, PROTOCOL_VERSION,
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

/// How long a held verdict waits on a collection that has gone **silent**
/// before it is handed over anyway: **30 s** (D167).
///
/// A verdict whose streamed collection is still crossing is not given to a
/// requester yet, because a requester that fetches on it gets a short
/// directory. That wait has to be bounded, and the bound is on *silence* and
/// not on duration — a few hundred megabytes on a slow link is slow and not
/// broken, so a deadline on the transfer itself would make the fleet's
/// biggest jobs the ones that fail. This is the same shape as every other
/// liveness figure in Herd, and the same figure the requester's own fetch
/// uses (`herdctl::OUTPUTS_STALL_LIMIT`, D150), on purpose: two different
/// silences would mean a requester and a hub disagreeing about when a
/// transfer had stopped.
///
/// Twice the runner's own stall sentence (15 s) so that the machine sending
/// the bytes gets to say what went wrong before the hub speaks for it; half
/// [`LOST_AFTER`] so that a runner that has genuinely died is noticed by the
/// watchdog at about the same time rather than long after. A constant and not
/// a flag: nothing an operator can tune makes this better, and a figure
/// derived from two others must not drift away from them.
pub const COLLECTION_SILENCE: Duration = Duration::from_secs(30);

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
///
/// Taken from [`herd_proto::FRAME_CEILING`] rather than written again here:
/// the output cap is **derived** from what a frame can carry (D131), so the
/// figure has to be one figure in one place or a cap would be computed
/// against a ceiling the hub no longer has.
pub const FRAME_CEILING_BYTES: usize = herd_proto::FRAME_CEILING as usize;

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

/// How long a `labels_from` reading's `run` step may take (D74, D108).
///
/// Five minutes. A reading builds and boots nothing — the first consumer's
/// `--labels` reads its arguments and a table compiled into the commit — so
/// this is a cap that a healthy reading never approaches and a hung one
/// cannot sit under for an afternoon. It is the hub's figure because a
/// reading is the hub's own job: nobody declared it, and the declaration's
/// `timeout_s` is the figure of the command whose *tests* run, which is hours.
/// The runner's own cap still bounds it and is never raised by it (D108).
pub const READING_CAP: Duration = Duration::from_secs(5 * 60);

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
    /// How long a follower of a job that is not over may hear nothing at all
    /// before the hub speaks to it ([`herd_proto::FromHub::StillThere`]).
    ///
    /// Fifteen seconds, [`herd_proto::STILL_THERE_EVERY`]. A *running* job
    /// that prints nothing gives its follower no frames whatever — no output,
    /// no queue reason, no verdict until the end — and through a proxy with an
    /// idle timeout that connection is closed under the follower, which
    /// `herdctl` reads as the stream ending with no verdict: exit 125 on a
    /// job that succeeded.
    ///
    /// A setting for the reason every other figure here is one: a test has to
    /// be able to prove a quiet job keeps its follower warm without running a
    /// job for a minute, and through the hub's own clock rather than a copy of
    /// it. Zero is refused.
    pub still_there_every: Duration,
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
    /// How long the `run` step of a `labels_from` reading may take (D74).
    ///
    /// See [`READING_CAP`]. A setting for the reason the others are: a test
    /// has to be able to make a reading overrun its cap without waiting five
    /// minutes, and through the hub's own code rather than a copy of it.
    pub reading_cap: Duration,
    /// How many requirement readings one runner may be running at once
    /// (D169).
    ///
    /// See [`crate::matcher::READINGS_AT_ONCE`]. A setting for the reason the
    /// others are: an operator with a bigger machine may want more of them,
    /// and a test has to be able to prove the bound holds without submitting
    /// two hundred jobs. Zero is refused — it would be a reading that is
    /// never placed, and every job waiting on one would wait for ever.
    pub readings_at_once: u32,
    /// How long a verdict held for a still-crossing collection waits on
    /// **silence** before it is handed over anyway (D167).
    ///
    /// See [`COLLECTION_SILENCE`]. A setting for the reason every other
    /// figure here is one: a test has to be able to prove that a runner which
    /// dies mid-collection never leaves a `wait` hanging, without waiting
    /// half a minute to find out, and through the hub's own clock rather than
    /// a copy of it.
    pub collection_silence: Duration,
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
            still_there_every: herd_proto::STILL_THERE_EVERY,
            drain_first_limit: crate::matcher::DRAIN_FIRST_LIMIT,
            requeue_budget: REQUEUE_BUDGET,
            reading_cap: READING_CAP,
            readings_at_once: crate::matcher::READINGS_AT_ONCE,
            collection_silence: COLLECTION_SILENCE,
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
        // And the same mistake for the liveness cadence: zero would be due on
        // every turn of the loop, so a follower of one quiet job would be
        // written to as fast as the hub can run. A frame nobody prints is not
        // a frame nobody pays for.
        if self.still_there_every.is_zero() {
            return Some(
                "--still-there-every 0 would keep a follower's link warm as fast as this hub \
                 can loop, which is not a cadence. Give 15 s, or a shorter figure a test can \
                 make a quiet minute pass under."
                    .to_owned(),
            );
        }
        // And a reading cap of zero, which is not a tight bound but an
        // absent one: no reading is ever placed, so every job whose
        // requirements are read on a machine waits for ever and nothing says
        // why. Refused here rather than found in a queue that never moves.
        if self.readings_at_once == 0 {
            return Some(
                "--readings-at-once 0 would place no requirement reading on any runner \
                 at all, so every job whose command declares labels_from would wait for \
                 ever. Give 1 if you want them strictly one at a time."
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

/// What this hub requires of the peers that dial it (E-48, Q8).
///
/// Separate from [`Policy`], which is timings an operator tunes: this is who
/// is allowed here. A runner's half of it is not in this struct at all — it
/// lives in the state root, because an approval has to survive a restart
/// (D8). What is here is the one thing the hub is *given* at startup rather
/// than keeping: the single shared requester token.
#[derive(Debug, Default)]
pub struct Authority {
    /// The requester token this hub was started with, as a salted hash.
    ///
    /// `None` is a hub that authenticates no requester, which is the default
    /// and is only safe where D14 puts the hub: loopback, behind the reverse
    /// proxy that M16 adds. The binary says so in one loud line at startup,
    /// because the failure mode of an open hub is that nothing goes wrong
    /// until something does.
    pub requester_token: Option<crate::roster::SharedSecret>,
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
        Self::bind_with(addr, store, policy, Authority::default()).await
    }

    /// The same, saying what this hub requires of the peers that dial it
    /// (E-48, Q8).
    ///
    /// What a runner must carry is not a parameter here: it is in the state
    /// root, where an approval belongs. This is the shared requester token,
    /// which the hub is handed at startup and never writes down.
    pub async fn bind_with(
        addr: SocketAddr,
        store: Arc<dyn JobStore>,
        policy: Policy,
        authority: Authority,
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
                authority,
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
    /// Who is allowed here (E-48, Q8). The runners' half is in the store,
    /// because an approval outlives the process; this is the requester token
    /// the hub was handed at startup.
    authority: Authority,
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
    /// Which **build** it is, as its handshake said (E-51, D166).
    ///
    /// Kept because a fleet listing has to be able to say it. The hub already
    /// judges this string at the handshake — refusing a major gap, noting a
    /// minor one on its own stderr — and that note goes nowhere an operator
    /// working through `herdctl` will ever see it.
    version: String,
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
    /// Whether this connection was left in the job's list of followers.
    ///
    /// False for a job that is already over, which is the ordinary case: the
    /// caller answers such a wait itself and there is nothing more to come.
    /// The caller needs to know, because a verdict held for a collection
    /// still crossing (D167) *is* something more to come, and it must be
    /// registered exactly once.
    following: bool,
}

/// One runner's connection, and what the hub knows about it.
struct ConnectedRunner {
    name: String,
    /// Which **build** this is, as its handshake announced it (E-51, D166).
    /// On the connection because a build is a fact about the process, and a
    /// redial brings its own — which is the whole point after an upgrade.
    version: String,
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
    /// What this runner last said it *did* with a position the hub asked it
    /// for (E-33, D99).
    ///
    /// The channel `herdctl switch` waits on. It is separate from `switch`
    /// above because the two answer different questions: `switch` is where the
    /// position stands, and this is whether the runner has it on its own disk.
    /// A health report proves the first and not the second — the runner applies
    /// a position it could not write, and rightly — so a requester told
    /// "success" on the strength of a health report was being told something
    /// nobody had established.
    ///
    /// `None` is "nothing acknowledged since the last ask". Cleared by
    /// [`set_a_switch`] before it asks, so that a stale acknowledgement of the
    /// same position can never answer a new question.
    applied: watch::Sender<Option<Acknowledged>>,
    jobs: Vec<RunningJob>,
}

impl ConnectedRunner {
    /// What a fleet listing says about this machine besides its name (D166).
    ///
    /// Built in one place for both listings — `herdctl runner list` and
    /// `herdctl switch` — because an operator must not have to learn which of
    /// the two commands tells the truth about a runner.
    ///
    /// The suspensions are `Some` even when the list is empty, and that is
    /// the point of the `Option`: this hub is connected to the machine and is
    /// answering the question. An *absent* list means a hub that cannot
    /// answer it, which is a different thing and must not read as a clean
    /// bill of health.
    fn facts(&self) -> herd_proto::RunnerFacts {
        herd_proto::RunnerFacts {
            version: Some(self.version.clone()),
            suspended: Some(
                self.capacity
                    .suspended()
                    .into_iter()
                    .map(|(label, why)| herd_proto::SuspendedCapability { label, why })
                    .collect(),
            ),
        }
    }
}

/// What a runner said it did with a position the hub asked it for (E-33, D99).
#[derive(Debug, Clone)]
struct Acknowledged {
    switch: herd_proto::Switch,
    persistence: herd_proto::SwitchPersistence,
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
    followers: HashMap<JobId, Vec<Follower>>,
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
    ///
    /// The job's **kept files** are held here with its outcome, and they have
    /// to be: the runner's workspace is already reclaimed by the time this
    /// frame exists, so these bytes and the runner's copy of them are the only
    /// two in the world. A retry that wrote the verdict and forgot the files
    /// would destroy half a record D19 says is never destroyed. Bounded by
    /// the output cap times the number of stalled jobs, which is the same
    /// bargain as holding the outcome itself.
    unwritten: HashMap<JobId, (JobOutcome, JobMetrics, herd_proto::JobOutputs)>,
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
    /// Attempts the watchdog has already said are slow rather than lost
    /// (D169).
    ///
    /// The watchdog wakes twenty times per threshold, so a busy runner's slow
    /// attempt would otherwise produce a line every three seconds for as long
    /// as it ran — which is the shape of log nobody reads. Said once per
    /// attempt and then kept quiet, and cleared by [`State::attempt_is_over`]
    /// with everything else about an attempt that is done.
    slowness_said: HashSet<JobId>,
    /// When whoever is following each queued job was last told why it is
    /// waiting (E-04).
    ///
    /// The cadence lives here rather than on the record because it is about a
    /// *stream*: the record already carries the reason, written only when it
    /// is news (M6), and E-04 asks for something else — that a requester
    /// watching a ten-minute wait is not left in silence. One instant per
    /// job, pruned when nobody is following it any more.
    wait_told: HashMap<JobId, Instant>,
    /// When the hub last wrote **anything at all** to the followers of each
    /// job.
    ///
    /// What [`FromHub::StillThere`] is decided on, and it has to be "anything
    /// at all" rather than "a note": a connection that has just carried eight
    /// kilobytes of a build log is not a connection a proxy is about to close,
    /// and one that has carried nothing since the job started is.
    ///
    /// Per *job* rather than per connection, because a job is the only thing
    /// [`Peers::followers`] is keyed on and because every frame the hub writes
    /// to a follower goes to all the followers of one job in the same breath.
    /// Kept beside [`Peers::wait_told`] and pruned by the same loop, for the
    /// same reason: it is a fact about a stream, not about a job.
    spoke_to_followers: HashMap<JobId, Instant>,
    /// Why each queued job is waiting, as the last dispatch pass judged it
    /// (D171).
    ///
    /// Two strings per job, because a waiting sentence is two different
    /// things wearing one coat. The **cause** is a fact about this job — the
    /// capability it needs is suspended, the reading it waits for has not
    /// answered, the runner it is sticking to is busy — and it changes
    /// rarely. The **sentence** wraps that cause in a snapshot of the whole
    /// queue: "queued 74th of 184", and the three jobs ahead of it by name.
    /// Those figures move whenever *any* job moves, which on a deep queue is
    /// every pass.
    ///
    /// So the cause is what the record is written for (`record_waiting`,
    /// news-only since M6) and the sentence is kept here, in memory, like
    /// every other fact about a stream in this struct: a requester that asks
    /// is told today's figures, and nothing is fsynced to say that a job is
    /// still waiting for the reason it was waiting for a moment ago.
    ///
    /// Pruned by the pass that fills it, against the queue it has just
    /// walked, so it is never larger than the queue and nothing survives the
    /// job it is about.
    waiting: HashMap<JobId, Waiting>,
    /// Verdicts this hub has **recorded** and not yet handed to a requester,
    /// because the collection they announce is still crossing (D167).
    ///
    /// The end is on the disk before an entry appears here, which is the
    /// whole point: recording the end is what frees the job's slot on its
    /// runner — the dispatch pass reads occupancy off the records and from
    /// nothing else (D79, `matcher::out_there`) — so a machine with one build
    /// slot is never held for the minute or two a few hundred megabytes take
    /// to arrive. What is held is only the sentence a requester reads, and it
    /// is held because a requester that fetches the moment it is told
    /// `finished` otherwise gets a short collection and no word about it.
    ///
    /// In memory and never on the disk, deliberately: a record says what
    /// happened, and "whose requester has not been told yet" is a fact about
    /// a stream. An entry lost to a hub restart costs nothing — the record is
    /// terminal and complete, and the next `herdctl wait` re-derives the
    /// whole question off the disk in [`answer_a_wait`].
    held_verdicts: HashMap<JobId, HeldVerdict>,
}

/// One requester following one job, and what it will do about that job's
/// output files (D167).
///
/// The flag is per **follower** and not per job, because two requesters can
/// follow one job and answer differently: a `herdctl run` that asked for no
/// files is told the verdict the moment it is recorded, exactly as it always
/// was, while a `herdctl wait --outputs-to` beside it waits for the
/// collection it is about to fetch. Absent on the wire means *wants them*,
/// so a requester built before the field keeps the safe behaviour
/// ([`herd_proto::ToHub::WaitForJob`]).
struct Follower {
    outbox: Outbox,
    wants_files: bool,
}

/// What one queued job's wait is, as the last pass judged it (D171).
struct Waiting {
    /// What was news about this job when its record was last written: its
    /// cause, and the level the queue weighs it at.
    ///
    /// `None` until a pass has written one, which is what makes the first
    /// judgement of a job news whatever a hub that died before it had
    /// written one had said. Set only once the write has gone through, so a
    /// disk that refused it is tried again by the next pass rather than
    /// remembered as done.
    recorded: Option<String>,
    /// The whole sentence the last pass built, cause and figures together.
    ///
    /// This is what a requester is told, and the reason it is held at all:
    /// the record carries the sentence that held when the cause last
    /// changed, and on a queue that is draining that sentence's position is
    /// stale within seconds.
    sentence: String,
}

/// A verdict waiting for its collection (D167).
struct HeldVerdict {
    outcome: JobOutcome,
    metrics: JobMetrics,
    /// When a byte of this collection last landed on this hub. The bound is
    /// on silence, not on duration ([`COLLECTION_SILENCE`]).
    moved: Instant,
}

impl Peers {
    /// The followers of one job, remembering that the hub is about to write
    /// to them.
    ///
    /// One function because the two halves must not drift. Every path that
    /// asks for a follower list asks in order to *send*, and the liveness
    /// clock is exactly "nothing was sent": a list handed out without the
    /// timestamp would be a frame the clock never saw, and the hub would then
    /// tell a busy connection it was quiet.
    fn followers_to_write_to(&mut self, id: &JobId) -> Vec<Outbox> {
        let followers: Vec<Outbox> = self
            .followers
            .get(id)
            .map(|following| following.iter().map(|f| f.outbox.clone()).collect())
            .unwrap_or_default();
        if !followers.is_empty() {
            self.spoke_to_followers.insert(id.clone(), Instant::now());
        }
        followers
    }

    /// A follower has just joined, and whoever registered it is about to say
    /// something to it — the id it was given, the log so far, where the job
    /// was placed.
    ///
    /// So the clock starts here rather than at the first frame the loop
    /// happens to see. Without it the first turn of the loop would find a
    /// follower nothing was ever written to and keep a link warm that was
    /// warm already.
    fn joined(&mut self, id: &JobId) {
        self.spoke_to_followers.insert(id.clone(), Instant::now());
    }
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
                    version: announced.version,
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
                    // Nothing has been asked of this connection yet, so
                    // nothing has been acknowledged on it. A redial starts
                    // empty on purpose: an acknowledgement is about one
                    // process, and the process may be a different one.
                    applied: watch::channel(None).0,
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
        held: &BTreeMap<String, Vec<matcher::Holding>>,
        reserved: &BTreeMap<String, matcher::Reserved>,
    ) -> Vec<Seat> {
        self.with_peers(|peers| {
            peers
                .runners
                .values()
                .map(|runner| {
                    let mut holdings = Holdings::new(runner.capacity.offer().clone());
                    for holding in held.get(&runner.name).into_iter().flatten() {
                        // Written down, never checked: a job the records say
                        // is out there is out there, whatever the offer says
                        // about its labels now. Checking would quietly leave
                        // such a job out of the holdings and read the host as
                        // idle on top of work that is running. What it *is*
                        // comes off the record with it (D159), so a hub that
                        // restarts does not read a reading as a build and
                        // hand its slot away twice.
                        holdings.already_holding(
                            &holding.job,
                            holding.labels.clone(),
                            holding.occupant,
                        );
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
                    facts: runner.facts(),
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

    /// Hand a runner's acknowledgement to whoever is waiting for it (D99).
    ///
    /// By the *connection*, like every other frame a runner sends: an
    /// acknowledgement is about one process, and a redial under the same name
    /// is a different one. Nobody waiting is the ordinary case — an owner who
    /// set her own switch file asked nothing — and dropping it then costs
    /// nothing, because the position itself travels on the health report.
    fn record_switch_applied(&self, outbox: &Outbox, acknowledged: Acknowledged) {
        self.with_peers(|peers| {
            if let Some(runner) = peers
                .runners
                .values()
                .find(|runner| runner.outbox.same_channel(outbox))
            {
                runner.applied.send_replace(Some(acknowledged));
            }
        })
    }

    /// Why a submission cannot be placed anywhere **ever**, if it cannot
    /// (D140).
    ///
    /// About the *fleet*, not about one job, and it is the one state a
    /// submission is answered with a refusal: **no runner is approved on this
    /// state root** — never was, or was and was removed. That is a
    /// configuration mistake, the hub counts it out loud at startup in these
    /// same words, and the roster is on its disk — so this is a fact the hub
    /// holds, not a guess about the moment. A job queued against an empty
    /// roster would wait for nobody coming.
    ///
    /// **A fleet that is merely away is not this, and is not refused.**
    /// Nobody connected this second, or every runner draining or switched
    /// off, is a job that **waits**, with the sentence the one queue already
    /// gives it (D38, D88, D140). D88's reason is that a fleet is not a closed
    /// world — the owner's PC is asleep at night and its 17 GiB come back in
    /// the morning — and a transient gap is that same situation with *weaker*
    /// evidence; E-33's switch is explicitly a temporary owner action. Both
    /// used to be refused here, which made the wait sentence
    /// [`crate::matcher`] already carried unreachable for a newly submitted
    /// job: which answer a job got depended only on whether it arrived a
    /// moment before or after the last link dropped, and every hook
    /// submitting during a hub restart, a runner restart or a brief
    /// disconnection got exit 125.
    ///
    /// What bounds a job that waits for an away fleet is **E-09's deadline**,
    /// which is where D88 leaves it: judged in [`crate::matcher::place`]
    /// before anything about the fleet, including an empty one. A cancel
    /// reaches it like any other queued job, and E-04's restatement keeps its
    /// stream saying where it stands.
    fn why_no_runner_is_approved(&self) -> Option<String> {
        if self.store.approved_runner_count() > 0 {
            return None;
        }
        Some(
            "0 runner(s) are approved on this state root, so nothing submitted here could \
             ever start. That is a configuration mistake rather than a fleet that is away, \
             and a job queued against an empty roster would wait for nobody coming — so it \
             is refused here instead of queued. `herdctl runner approve <name>` mints an \
             approval and `herdctl runner list` says who is approved"
                .to_owned(),
        )
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

    fn follow(&self, id: JobId, outbox: &Outbox, wants_files: bool) {
        self.with_peers(|peers| {
            peers.joined(&id);
            peers.followers.entry(id).or_default().push(Follower {
                outbox: outbox.clone(),
                wants_files,
            });
        });
    }

    /// Who is following this job, and the hub is about to write to them.
    ///
    /// Taking the list is what marks the connection as having spoken
    /// ([`Peers::followers_to_write_to`]), so this is not a getter: every
    /// call site here sends on what it gets back, and the liveness clock
    /// depends on that staying true.
    fn followers_to_tell(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| peers.followers_to_write_to(id))
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
            let followers = peers.followers_to_write_to(id);
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
        outputs: &herd_proto::JobOutputs,
    ) -> Vec<Outbox> {
        self.with_peers(|peers| {
            peers.stalled.insert(id.clone(), reason);
            peers.unwritten.insert(
                id.clone(),
                (outcome.clone(), metrics.clone(), outputs.clone()),
            );
            peers.followers_to_write_to(id)
        })
    }

    /// Why a queued job is waiting, with the figures the **last pass** found
    /// (E-04, D171).
    ///
    /// Preferred over the record's own `waiting_for` everywhere a requester
    /// is told, and for one reason: the record is written when the *cause*
    /// changes, so a job whose cause has held for twenty minutes carries the
    /// position it had twenty minutes ago. A queue drains; an ordinal does
    /// not survive it. `None` for a job this hub has not judged since it
    /// started, and then the record is the only answer there is.
    fn why_it_waits(&self, id: &JobId) -> Option<String> {
        self.with_peers(|peers| peers.waiting.get(id).map(|w| w.sentence.clone()))
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
    fn attach(
        &self,
        id: &JobId,
        outbox: &Outbox,
        wants_files: bool,
    ) -> crate::job::Result<Option<Attached>> {
        self.with_peers(|peers| {
            let Some(record) = self.store.job(id)? else {
                return Ok(None);
            };
            let log = self.store.job_log(id)?.unwrap_or_default();
            let stalled = peers.stalled.get(id).cloned();
            // **A job whose verdict this hub is holding is still followed**
            // (D167). Its record is terminal, so without this clause a
            // `herdctl wait` arriving while the collection crosses would be
            // registered nowhere and would never be sent the verdict it is
            // held back from. It is not enough for the caller to answer this
            // connection directly: the collection completes on another task
            // entirely, and the list of followers is where that task looks.
            //
            // Only for a requester that wants the files: one that does not
            // is answered now and has nothing more to come.
            let held_back = wants_files && peers.held_verdicts.contains_key(id);
            let following = !record.state.is_terminal() || held_back;
            if following {
                peers.joined(id);
                peers
                    .followers
                    .entry(id.clone())
                    .or_default()
                    .push(Follower {
                        outbox: outbox.clone(),
                        wants_files,
                    });
            }
            Ok(Some(Attached {
                record,
                log,
                stalled,
                following,
            }))
        })
    }

    fn stop_following(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| {
            peers
                .followers
                .remove(id)
                .unwrap_or_default()
                .into_iter()
                .map(|follower| follower.outbox)
                .collect()
        })
    }

    /// Take the followers of this job whose answer about its files is
    /// `wants_files`, leaving the others where they are (D167).
    ///
    /// What lets one verdict go to some followers now and wait for the rest:
    /// a `herdctl run` that asked for no files is told the moment the end is
    /// recorded, and the `herdctl wait --outputs-to` beside it keeps waiting
    /// for the collection it is about to fetch.
    fn take_the_followers_that(&self, id: &JobId, wants_files: bool) -> Vec<Outbox> {
        self.with_peers(|peers| {
            let Some(following) = peers.followers.get_mut(id) else {
                return Vec::new();
            };
            let mut taken = Vec::new();
            following.retain(|follower| {
                if follower.wants_files == wants_files {
                    taken.push(follower.outbox.clone());
                    false
                } else {
                    true
                }
            });
            if following.is_empty() {
                peers.followers.remove(id);
            }
            if !taken.is_empty() {
                peers.spoke_to_followers.insert(id.clone(), Instant::now());
            }
            taken
        })
    }

    /// Whether anybody following this job is going to fetch its files, and
    /// is therefore worth holding its verdict for.
    fn anybody_wants_the_files(&self, id: &JobId) -> bool {
        self.with_peers(|peers| {
            peers
                .followers
                .get(id)
                .is_some_and(|following| following.iter().any(|f| f.wants_files))
        })
    }

    /// Keep this job's verdict back: it is recorded, and the collection it
    /// announced is still crossing (D167).
    ///
    /// Idempotent on the clock it carries. A verdict already held keeps the
    /// instant it was last making progress, so a second look at the same job
    /// does not reset the silence bound — which is exactly how a wedged
    /// transfer would wait for ever.
    fn hold_the_verdict(&self, id: &JobId, outcome: &JobOutcome, metrics: &JobMetrics) {
        self.with_peers(|peers| {
            peers
                .held_verdicts
                .entry(id.clone())
                .or_insert(HeldVerdict {
                    outcome: outcome.clone(),
                    metrics: metrics.clone(),
                    moved: Instant::now(),
                });
        });
    }

    /// A byte of this job's collection landed, so its held verdict is
    /// waiting on something that is working (D167).
    fn the_collection_moved(&self, id: &JobId) {
        self.with_peers(|peers| {
            if let Some(held) = peers.held_verdicts.get_mut(id) {
                held.moved = Instant::now();
            }
        });
    }

    /// Take this job's held verdict, if one is held.
    fn take_the_held_verdict(&self, id: &JobId) -> Option<(JobOutcome, JobMetrics)> {
        self.with_peers(|peers| {
            peers
                .held_verdicts
                .remove(id)
                .map(|held| (held.outcome, held.metrics))
        })
    }

    /// Every job whose verdict is held, and how long its collection has been
    /// silent, for the clock that decides whether holding it is still worth
    /// anything.
    fn verdicts_held(&self) -> Vec<(JobId, Duration)> {
        self.with_peers(|peers| {
            peers
                .held_verdicts
                .iter()
                .map(|(id, held)| (id.clone(), held.moved.elapsed()))
                .collect()
        })
    }

    /// A requester went away: nobody is waiting for its jobs any more.
    fn forget_follower(&self, outbox: &Outbox) {
        self.with_peers(|peers| {
            for following in peers.followers.values_mut() {
                following.retain(|f| !f.outbox.same_channel(outbox));
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
            peers.slowness_said.remove(id);
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

    /// What the runner holding this attempt says about it **now**, rather
    /// than when the hub last heard anything (D169).
    ///
    /// The per-attempt clock is the right instrument for a machine that is
    /// gone and the wrong one for a machine that is merely slow, and the two
    /// are not told apart by a timer. A health report names every job its
    /// runner is holding and nothing else, so a runner that is connected is
    /// a runner that has already answered the question: either its last
    /// report names this attempt, in which case the attempt is running and
    /// the machine is busy, or it does not, in which case the attempt is
    /// gone from a machine that is plainly here.
    ///
    /// `unheard_for` is the age of the clock the watchdog is about to act on,
    /// and the report is only evidence if it is **newer** than that: a report
    /// composed before the job was handed out says nothing about it. Reports
    /// older than the threshold cannot arise here — the watchdog drops a link
    /// nothing comes back on before it looks at any attempt — but the
    /// comparison is made rather than assumed, because the one case it would
    /// get wrong is a job requeued onto a runner whose report is in flight,
    /// and that would spend a go on a machine that did nothing wrong.
    fn what_the_runner_says(
        &self,
        runner: &str,
        job_id: &JobId,
        attempt: u32,
        unheard_for: Duration,
    ) -> WhatTheRunnerSays {
        let word = self.with_peers(|peers| {
            peers.runners.get(runner).map(|connected| TheRunnersWord {
                reports: connected.reports_seen,
                reported: connected.last_health.elapsed(),
                names_it: connected
                    .jobs
                    .iter()
                    .any(|held| &held.job_id == job_id && held.attempt == attempt),
            })
        });
        what_a_runner_says(word, unheard_for)
    }

    /// Whether this is the first time the watchdog has found this attempt
    /// slow. See [`Peers::slowness_said`].
    fn first_word_of_slowness(&self, job_id: &JobId) -> bool {
        self.with_peers(|peers| peers.slowness_said.insert(job_id.clone()))
    }
}

/// What the runner holding an attempt last said about it (D169).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhatTheRunnerSays {
    /// It is connected and its last report names the attempt: the attempt is
    /// alive, and the machine is busy rather than gone.
    StillRunningIt { reported: Duration },
    /// It is connected and has not reported on this link yet, so it has not
    /// been asked the question. Not evidence of anything, and bounded: a
    /// connection nothing comes back on is dropped by the link check at
    /// `lost_after`, and then there is no runner and the clock decides.
    NotYetSaying,
    /// It is connected and its last report does not name the attempt: the
    /// attempt is over on a machine that is here to say so.
    NotAnyMore { reported: Duration },
    /// Nobody of that name is connected. Nothing to go on but the clock,
    /// which is what the clock is for (E-21).
    NothingAtAll,
}

/// What this hub knows about the runner an attempt was handed to, at the
/// moment the watchdog is about to act on the clock (D169).
///
/// `None` for a runner that is not connected. Gathered by
/// [`State::what_the_runner_says`] and judged by [`what_a_runner_says`],
/// which are two functions so that the judgement is a pure one a test can
/// put in any state at all without a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TheRunnersWord {
    /// How many health reports this connection has had.
    reports: u64,
    /// How long ago the last of them was recorded.
    reported: Duration,
    /// Whether that report named this attempt.
    names_it: bool,
}

/// Whether the clock may be believed about an attempt, given what its runner
/// has said (D169).
///
/// The watchdog times an *attempt*, and that is still right: a runner killed
/// and restarted is connected and healthy while the job it was running
/// exists nowhere. What was wrong is believing the clock when the machine is
/// on the line and has just said otherwise. A health report names every job
/// its runner is holding and nothing else, so a connected runner has already
/// answered the question — and an answer beats a timer that was only ever a
/// stand-in for one.
///
/// A report is only evidence if it is **newer** than the hub's last word
/// about the attempt: a report composed before the job was handed out says
/// nothing about it, and reading it as "the runner does not have this" would
/// spend a go on a machine that did nothing wrong.
fn what_a_runner_says(word: Option<TheRunnersWord>, unheard_for: Duration) -> WhatTheRunnerSays {
    let Some(word) = word else {
        return WhatTheRunnerSays::NothingAtAll;
    };
    if word.reports == 0 {
        return WhatTheRunnerSays::NotYetSaying;
    }
    if word.reported >= unheard_for {
        return WhatTheRunnerSays::NotYetSaying;
    }
    if word.names_it {
        WhatTheRunnerSays::StillRunningIt {
            reported: word.reported,
        }
    } else {
        WhatTheRunnerSays::NotAnyMore {
            reported: word.reported,
        }
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

/// The word a sentence about the far side uses for it.
fn peer_kind(role: Role) -> &'static str {
    match role {
        Role::Runner => "runner",
        Role::Requester => "requester",
    }
}

/// Why this peer may not be served, or `None` if it may (E-48, Q8).
///
/// A **runner** is admitted by the roster in the state root: an operator
/// approved its name, which minted a token, and the hub kept the hash. Four
/// answers, three of which are a no, and each no names what to do.
///
/// A **requester** is admitted by the one shared token the hub was started
/// with, when it was started with one. A hub with no requester token
/// authenticates no requester — see [`Authority::requester_token`] for why
/// that is the default and what says so out loud.
fn admission_refusal(
    state: &State,
    role: Role,
    peer_name: &str,
    token: Option<&str>,
) -> Option<String> {
    match role {
        Role::Runner => match state.store.admit_runner(peer_name, token) {
            crate::roster::Admission::Approved => None,
            crate::roster::Admission::NotApproved => {
                Some(herd_proto::unapproved_runner_reason(peer_name))
            }
            crate::roster::Admission::NoToken => {
                Some(herd_proto::missing_runner_token_reason(peer_name))
            }
            crate::roster::Admission::WrongToken => {
                Some(herd_proto::wrong_runner_token_reason(peer_name))
            }
        },
        Role::Requester => {
            let required = state.authority.requester_token.as_ref()?;
            match token {
                None => Some(herd_proto::missing_requester_token_reason()),
                Some(token) if required.matches(token) => None,
                Some(_) => Some(herd_proto::wrong_requester_token_reason()),
            }
        }
    }
}

async fn serve_peer(state: Arc<State>, stream: TcpStream) -> Result<()> {
    // Log chunks are small and must not wait for a full segment.
    let _ = stream.set_nodelay(true);
    let socket =
        match tokio_tungstenite::accept_async_with_config(stream, Some(frame_limits())).await {
            Ok(socket) => socket,
            Err(error) => bail!("{}", unfinished_handshake_reason(&error)),
        };
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
    #[allow(clippy::type_complexity)]
    let (
        role,
        peer_name,
        peer_instance,
        peer_state_root,
        peer_health_every,
        announced,
        peer_token,
        version_note,
    ) = match hello {
        ToHub::Hello {
            protocol_version,
            peer_version,
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
            // And then which *build* is speaking, which the protocol
            // version does not say (E-51). A major version apart is
            // refused here, before this peer is registered or given
            // anything: two builds a major apart can read each other's
            // frames and still disagree about what they commit to, which
            // is a worse thing to discover from a job's record than from
            // a refusal. A minor gap rides out in the welcome.
            let gap = herd_proto::version_gap(peer_kind(role), hub_version(), &peer_version);
            let version_note = match gap {
                Some(gap) if gap.refuse => {
                    eprintln!("herd-hub: {}", gap.reason);
                    return refuse(&mut sink, gap.reason).await;
                }
                Some(gap) => {
                    eprintln!("herd-hub: {}", gap.reason);
                    Some(gap.reason)
                }
                None => None,
            };
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
                    version: peer_version,
                    capacity: RunnerCapacity::new(name, capabilities),
                    host,
                    preference,
                    load_milli,
                    switch,
                },
                said(token),
                version_note,
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

    // And then: is this peer allowed here at all (E-48, Q8)?
    //
    // **Before the welcome**, unlike the checks below it, and that is the
    // point: a peer this hub has never approved gets one frame, the refusal,
    // and learns nothing else about the hub — not its state root, not its
    // version. The checks that come after the welcome are about
    // configurations that cannot be served; this one is about a machine that
    // may not be served, which is a different answer and deserves a
    // different shape.
    //
    // Terminal (D13) on purpose, in all four of its forms. An unapproved
    // runner, one carrying no token, one carrying the wrong token and one
    // whose approval was removed are each waiting for a person to do
    // something, and a runner told to *stand by* for that would come back
    // every few seconds for ever with nobody reading the reason. The sentence
    // carries the command that fixes it, because the person reading it on the
    // runner's stderr is usually the person who can type it.
    if let Some(reason) = admission_refusal(&state, role, &peer_name, peer_token.as_deref()) {
        eprintln!("herd-hub: {reason}");
        return refuse(&mut sink, reason).await;
    }

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
            // A build gap this hub is willing to serve, said where the far
            // side's owner will read it (E-51). `None` is the ordinary case.
            version_note,
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
    // Before anything else is asked of this connection: the collections this
    // hub is still owed bytes for (D148).
    //
    // **Sent unprompted, the way `ResendLog` is**, and for the same reason:
    // the hub is the side that knows what it is missing, so asking would be a
    // round trip spent learning what it could simply say. A runner that has
    // just redialled with 300 MiB spooled needs one frame to know where to
    // pick up, and a part-written file resumes at the hub's own length rather
    // than at the runner's idea of how far it had got — which is exactly the
    // figure a dropped link invalidated.
    ask_for_the_files_still_owed(state, name, outbox);
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
            for follower in state.followers_to_tell(&job) {
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
    let mut unfinished = state
        .store
        .unfinished_jobs()
        .context("reading the queue the hub recovered")?;
    // Before a single job is placed: every job whose requirements are read on
    // a machine gets its reading minted, or its answer written onto its
    // record, or is refused by name (D74). Done here, ahead of the ordering,
    // so that a reading minted now is in *this* pass's queue and goes out to
    // a runner at once — and so that a job whose answer has just landed is
    // placed in the same pass that read it.
    if settle_readings(state, &unfinished)? {
        unfinished = state
            .store
            .unfinished_jobs()
            .context("reading the queue again, after the readings were settled")?;
    }
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
    let held_empty = matcher::reservations(&queue, &seats, now, state.policy.drain_first_limit);
    for seat in seats.iter_mut() {
        seat.reserved_for = held_empty.get(&seat.name).cloned();
    }
    let total = queue.len();
    // Every job this pass found still waiting, so that what the hub holds in
    // memory about waits ([`Peers::waiting`]) is emptied of the jobs that are
    // no longer in the queue. Derived from the pass that has just walked it,
    // which is the only thing that knows the answer exactly — so there is no
    // eviction policy and nothing to leak (D79's habit, applied to a map).
    let mut said_it_waits: HashSet<JobId> = HashSet::new();
    for (place, record) in queue.iter().enumerate() {
        // A job whose requirements are still being read is not placed, by
        // anything, ever: there is nothing to place it against but the
        // baseline its reading is running on, and that baseline is not its
        // own (D74). [`settle_readings`] has already done everything that can
        // be done for it this pass, so what is left is to say so.
        if record.awaits_a_reading() {
            let standing = queue::standing(record, now, state.policy.aging_step);
            let because = format!("it is waiting because {}", waiting_for_a_reading(record));
            let sentence = queue::waiting_sentence_behind(
                place + 1,
                total,
                &standing,
                &queue::what_is_ahead(&queue, place, now, state.policy.aging_step),
                &because,
            );
            said_it_waits.insert(record.id.clone());
            say_it_waits(state, &record.id, &standing, &because, &sentence)?;
            continue;
        }
        // The one job this one named, if it named one (E-10). Looked up here
        // rather than inside the claim step, which is a pure function of a
        // record and a fleet: a parent that is finished is not in the queue at
        // all, so finding it is a read of the store and the claim step does no
        // I/O. `unfinished` first, because that is already in hand.
        let placing = matcher::Placing {
            parent: parent_of(state, record, &unfinished),
            drain_first_limit: Some(state.policy.drain_first_limit),
            readings_at_once: Some(state.policy.readings_at_once),
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
                    let because = format!(
                        "it is waiting because runner {runner}'s connection ended while this \
                         pass was placing it there"
                    );
                    let sentence = queue::waiting_sentence_behind(
                        place + 1,
                        total,
                        &standing,
                        &queue::what_is_ahead(&queue, place, now, state.policy.aging_step),
                        &because,
                    );
                    said_it_waits.insert(record.id.clone());
                    say_it_waits(state, &record.id, &standing, &because, &sentence)?;
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
                let because = format!("it is waiting because {why}");
                let sentence = queue::waiting_sentence_behind(
                    place + 1,
                    total,
                    &standing,
                    &queue::what_is_ahead(&queue, place, now, state.policy.aging_step),
                    &because,
                );
                said_it_waits.insert(record.id.clone());
                say_it_waits(state, &record.id, &standing, &because, &sentence)?;
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
    // A job that is no longer waiting is a job this hub has nothing to
    // remember about a wait. Pruned here, against the pass that has just
    // judged the whole queue, so a claimed job, a cancelled one and one that
    // was found impossible all go in the same breath.
    state.with_peers(|peers| peers.waiting.retain(|id, _| said_it_waits.contains(id)));
    Ok(())
}

/// Settle every reading the queue is waiting on, and say whether anything was
/// written (D74).
///
/// The whole of D74's mechanism, in one place and under the claim lock the
/// pass already holds. For each queued job whose command declared a
/// `labels_from`:
///
/// * **no reading yet** — the cache is asked first, because the answer is a
///   property of (commit, command, args) and nothing else, so a second
///   submission of those three must not pay for a second reading. On a miss,
///   an **ordinary job** is minted for it: the one queue, the baseline labels
///   the command declared, the runner's caps, its own record and its own four
///   executor steps. It carries **no reading of its own**, which is how "a
///   command reached through `labels_from` never resolves one" is kept — the
///   hub is the only thing that mints such a job.
/// * **a reading that has not ended** — nothing to do; the job waits, and the
///   pass says so.
/// * **a reading that ended well** — its output is read strictly
///   ([`herd_proto::Requirements::read`]), the answer goes into the cache and
///   onto the job's record, and the pass places the job on it.
/// * **a reading that ended any other way, or whose answer is not one** — the
///   job is **refused by name**, with the reading's id and what was wrong in
///   the sentence. A job is never placed on a guess.
///
/// Every write goes through the store's one door, so a job cancelled while
/// its reading ran is an error naming it rather than a requirement written
/// onto a closed record — and the pass goes on to the next job instead of
/// failing.
fn settle_readings(state: &State, unfinished: &[JobRecord]) -> Result<bool> {
    let mut written = false;
    for record in unfinished {
        if record.state != JobState::Queued || !record.awaits_a_reading() {
            continue;
        }
        let Some(reading) = record.reading.clone() else {
            continue;
        };
        // A reading with no declaration cannot be: the submission gate
        // refuses one ([`why_that_reading_will_not_do`]). A record that
        // carries it anyway is said rather than guessed at.
        let Some(declaration) = record.declaration.clone() else {
            end_the_job(
                state,
                record,
                JobOutcome::Impossible {
                    reason: format!(
                        "{} is refused: its requirements come from a reading ({}) and its \
                         record carries no declared command, so there is no commit, no \
                         command and no arguments to read them for (D74)",
                        record.id, reading.labels_from.line
                    ),
                },
                "has a reading and no declaration",
            )?;
            written = true;
            continue;
        };
        let key =
            crate::job::reading_key(&declaration.commit, &declaration.command, &declaration.args);
        match reading.by.clone() {
            None => {
                read_what_it_needs(state, record, reading, &declaration, &key)?;
                written = true;
            }
            Some(by) => {
                let read = state
                    .store
                    .job(&by)
                    .with_context(|| format!("reading the record of {by}"))?;
                let Some(read) = read else {
                    end_the_job(
                        state,
                        record,
                        JobOutcome::Impossible {
                            reason: refused_for_want_of_requirements(
                                record,
                                &declaration,
                                &by,
                                &format!("this hub has no record of {by} at all"),
                            ),
                        },
                        "its reading is gone",
                    )?;
                    written = true;
                    continue;
                };
                let JobState::Finished { outcome } = &read.state else {
                    // Still going. The job waits, and the pass says so.
                    continue;
                };
                if !outcome.is_success() {
                    // A reading that was **lost** answered nothing, so there
                    // is nothing about this commit to refuse the job with: it
                    // is read again (D169). Told apart from every other way a
                    // reading can fail, which are facts about the commit and
                    // do refuse it — that one branch, covering both, is what
                    // turned 201 of one nightly's 214 jobs into exit 125 on a
                    // host that was merely buried.
                    if matches!(outcome, JobOutcome::Lost { .. }) {
                        let again = reading.clone().after_a_loss();
                        if again.may_be_read_again(state.policy.attempts_allowed) {
                            println!(
                                "herd-hub: {by} was reading what {} needs and was lost \
                                 before it answered, so nothing was learnt about command \
                                 {} of commit {}; it is read again (reading {} of {}, \
                                 D169)",
                                record.id,
                                declaration.command,
                                declaration.commit,
                                again.lost + 1,
                                state.policy.attempts_allowed
                            );
                            // In **this** pass, not the next one. A record
                            // merely set back to "nothing is reading it" is
                            // a job that waits for whatever wakes the hub
                            // next, which on a quiet fleet is the
                            // restatement cadence — a minute of a nightly
                            // nobody can account for, which is what the
                            // end-to-end test measured before this line.
                            read_what_it_needs(state, record, again, &declaration, &key)?;
                            written = true;
                            continue;
                        }
                        end_the_job(
                            state,
                            record,
                            JobOutcome::Impossible {
                                reason: refused_for_want_of_requirements(
                                    record,
                                    &declaration,
                                    &by,
                                    &format!(
                                        "it {}, and that is the {} reading of this job lost \
                                         to a machine that went away — the same number of \
                                         goes E-21 gives a job at a runner that disappears \
                                         (D169). Nothing is wrong with the command: the \
                                         fleet could not keep a reading of it alive",
                                        how_it_ended(outcome),
                                        again.lost
                                    ),
                                ),
                            },
                            "its readings keep being lost",
                        )?;
                        written = true;
                        continue;
                    }
                    // D74's second limit: a reading that exits non-zero, or
                    // that its cap stopped, refuses the job. Both arrive
                    // here, and the sentence says which it was in the words a
                    // requester reads everywhere else.
                    end_the_job(
                        state,
                        record,
                        JobOutcome::Impossible {
                            reason: refused_for_want_of_requirements(
                                record,
                                &declaration,
                                &by,
                                &format!("it {}", how_it_ended(outcome)),
                            ),
                        },
                        "its reading did not come off",
                    )?;
                    written = true;
                    continue;
                }
                let log = state
                    .store
                    .job_log(&by)
                    .with_context(|| format!("reading the output of {by}"))?
                    .unwrap_or_default();
                match herd_proto::Requirements::read(&log) {
                    // D74's first limit: the answer is read strictly, like
                    // the capability file (D60).
                    Err(why) => {
                        end_the_job(
                            state,
                            record,
                            JobOutcome::Impossible {
                                reason: refused_for_want_of_requirements(
                                    record,
                                    &declaration,
                                    &by,
                                    &why,
                                ),
                            },
                            "its reading did not answer",
                        )?;
                        written = true;
                    }
                    Ok(answer) => {
                        // The cache before the record, so that a hub which
                        // stops between the two re-reads nothing: the next
                        // pass finds the answer under its key and writes it
                        // onto the record without a second job.
                        if let Err(error) = state.store.remember_reading(&key, &answer) {
                            eprintln!(
                                "herd-hub: {by} answered what {} needs and this hub could \
                                 not keep the answer ({error}); the job is placed on it all \
                                 the same and the next submission of the same commit, \
                                 command and arguments will pay for a reading",
                                record.id
                            );
                        }
                        println!(
                            "herd-hub: {} is what {by} read for command {} of commit {}, and \
                             {} is placed on it",
                            answer.line(),
                            declaration.command,
                            declaration.commit,
                            record.id
                        );
                        let said = format!("read on a runner by {by}: {}", answer.line());
                        if let Err(error) = state
                            .store
                            .record_reading(&record.id, reading.answered(answer, said))
                        {
                            eprintln!(
                                "herd-hub: {by} answered what {} needs and that could not be \
                                 written onto its record: {error}",
                                record.id
                            );
                            continue;
                        }
                        written = true;
                    }
                }
            }
        }
    }
    Ok(written)
}

/// Give this job an answer, or a job that will find one (D74).
///
/// The cache first and always, which is what makes a second submission of
/// one (commit, command, args) free; and otherwise an ordinary job that
/// reads them.
///
/// One function rather than the body of one `match` arm, because two things
/// now reach it: a job whose requirements have never been read, and a job
/// whose reading was lost before it answered anything (D169). The second
/// must go through the cache exactly as the first does — a reading lost
/// after another job's reading of the same three had already answered costs
/// nothing at all — and two copies of that would be two answers to one
/// question.
fn read_what_it_needs(
    state: &State,
    record: &JobRecord,
    reading: crate::job::Reading,
    declaration: &herd_proto::Declaration,
    key: &str,
) -> Result<()> {
    if let Some(answer) = state
        .store
        .reading_answer(key)
        .with_context(|| format!("asking the cache what {} needs", record.id))?
    {
        let said = format!(
            "read before, and kept by (commit, command, args): {}",
            answer.line()
        );
        println!(
            "herd-hub: {} needs no reading: command {} of commit {} was read before and the \
             answer is {}",
            record.id,
            declaration.command,
            declaration.commit,
            answer.line()
        );
        if let Err(error) = state
            .store
            .record_reading(&record.id, reading.answered(answer, said))
        {
            eprintln!(
                "herd-hub: {} could not be given the cached answer to its reading: {error}",
                record.id
            );
        }
        return Ok(());
    }
    let new = reading_job(state, record, &reading, declaration);
    let submission = state
        .store
        .submit(new)
        .with_context(|| format!("minting the reading {} waits for", record.id))?;
    let by = submission.record().id.clone();
    println!(
        "herd-hub: {by} reads what {} needs — command {} of commit {}, against the baseline \
         labels {} its declaration states (D74)",
        record.id,
        declaration.command,
        declaration.commit,
        if reading.labels_from.baseline.is_empty() {
            "none".to_owned()
        } else {
            reading.labels_from.baseline.to_string()
        }
    );
    if let Err(error) = state.store.record_reading(&record.id, reading.read_by(&by)) {
        eprintln!(
            "herd-hub: {by} was minted to read what {} needs and that could not be written \
             onto its record: {error}",
            record.id
        );
    }
    Ok(())
}

/// The job that reads what another job needs: an ordinary job, and the one
/// thing that makes it one is that nothing here is special (D74).
///
/// What it takes from the job it reads for, and why each one:
///
/// * the **declaration**, so it runs the consumer's own four executor steps
///   on the same commit — their reading reads their tree, and `up` and
///   `fetch` are what put the tree on a machine (D77, D106);
/// * the **baseline labels** the command declared, which is what it is placed
///   against (D74) — never the labels the reading is about to answer;
/// * the **level** of the job that waits for it, so that a nightly's reading
///   does not go round the queue in front of a merger's job (E-18);
/// * the **deadline**, so that a reading of a job that is no longer worth
///   starting is no longer worth running either (E-09).
///
/// What it does not take: the parent (E-10 — a reading needs the tree and
/// nothing a parent makes), the requeue code (the consumer's 75 is about
/// their tests), the figures (it builds and boots nothing) and — the one that
/// matters — **a reading of its own**. Hence no recursion.
fn reading_job(
    state: &State,
    record: &JobRecord,
    reading: &crate::job::Reading,
    declaration: &herd_proto::Declaration,
) -> NewJob {
    NewJob::at(
        vec![reading.labels_from.line.clone()],
        reading.labels_from.baseline.clone(),
        record.priority,
    )
    .with_m9(record.deadline, None, None)
    .declared(
        Some(declaration.clone()),
        Some(state.policy.reading_cap.as_secs()),
    )
    // Said out loud rather than left to the default: this is D74's third
    // limit, and it is kept here or nowhere.
    .reading(None)
    // And this is what makes it a reading in the claim step's eyes (D159):
    // it holds no counted slot, empties no host-wide machine and waits for
    // no probe, while still being matched on the labels a runner offers and
    // has not denied, and still admitted against the room (D119). The one
    // place a reading is ever marked as one — the hub is the only thing that
    // mints one, which is also how D74's no-recursion limit is kept.
    .reads_for(&record.id)
}

/// Why a job whose requirements could not be read is refused, **by name**
/// (D74, E-36).
///
/// It names the job, the command and the commit, the reading that was run for
/// it, and what was wrong with the answer — because the only thing to do
/// about one of these is to fix the reading, and whoever reads this sentence
/// is the person who can.
fn refused_for_want_of_requirements(
    record: &JobRecord,
    declaration: &herd_proto::Declaration,
    by: &JobId,
    what_went_wrong: &str,
) -> String {
    format!(
        "{} is refused and was never placed: what command {} of commit {} needs is read on a \
         runner, {by} ran that reading, and {what_went_wrong}. A job is never placed on \
         requirements nobody read (D74)",
        record.id, declaration.command, declaration.commit
    )
}

/// Why a job waiting for its requirements is waiting, in the words its record
/// carries (D74, E-04).
fn waiting_for_a_reading(record: &JobRecord) -> String {
    let Some(reading) = record.reading.as_ref() else {
        return "its requirements are not known".to_owned();
    };
    match &reading.by {
        Some(by) => format!(
            "what it needs is being read on a runner by {by} ({}), and a job is placed on what \
             a reading answered rather than on a guess (D74)",
            reading.labels_from.line
        ),
        None => format!(
            "what it needs has to be read on a runner first ({}), and this hub has not minted \
             that reading yet (D74)",
            reading.labels_from.line
        ),
    }
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
        JobState::Finished { outcome } if outcome.is_success() => {
            matcher::Parent::Succeeded { id: parent_id }
        }
        JobState::Finished { outcome } => matcher::Parent::DidNot {
            id: parent_id,
            how: how_it_ended(outcome),
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
        JobOutcome::SetupFailed { reason } => {
            format!("ran on a machine that could not do its part: {reason}")
        }
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
fn end_the_job(state: &State, record: &JobRecord, outcome: JobOutcome, said: &str) -> Result<()> {
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
    // Whatever the record holds, which for a job the claim step ended is
    // nothing: no runner ever made it a workspace. Read rather than assumed,
    // so that there is one answer to "what did this job leave?" and not one
    // per place a job can end.
    let kept = read_the_outputs(state, &record.id);
    let commit = the_commit_it_ran(state, &record.id);
    for follower in state.stop_following(&record.id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: record.id.clone(),
            outcome: outcome.clone(),
            metrics: JobMetrics::default(),
            outputs: kept.clone(),
            commit: commit.clone(),
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

/// Keep every followed job's stream from going silent: say again why a queued
/// job is waiting (E-04), and breathe on any job that is not over and is
/// saying nothing.
///
/// Two deadlines, **one timer**, and that is deliberate. They are the same
/// question asked twice — *is this connection still carrying anything?* — and
/// a second task would be a second place to get the pruning of
/// [`Peers::wait_told`] and [`Peers::spoke_to_followers`] wrong. [`Restating`]
/// already separates the cadence a deadline is measured against from the tick
/// a loop wakes on, so one loop waking on the shorter of the two ticks serves
/// both figures with each keeping its own cadence.
///
/// E-04's half is explained below. The liveness half is this: a job that is
/// executing and printing nothing produces no frames at all, so it carries no
/// bytes for as long as the job is quiet, and a reverse proxy with an idle
/// timeout closes it under the follower — which `herdctl` reads as the stream
/// ending with no verdict and reports as 125 on a job that succeeded.
///
/// The two are **not** "queued" and "running": the breath covers every state
/// that is not terminal, queued included. E-04's restatement is paced by a
/// figure an operator chooses — 60 s by default, four breaths — and a job the
/// hub has not yet found a reason for is restated never, so a queued stream is
/// not warm by itself either. What keeps the two from doubling up is the
/// clock: a note the hub has just written *is* traffic, so it postpones the
/// next breath by a cadence.
///
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
    // The liveness deadline, with a cadence of its own. The loop wakes on
    // whichever tick is shorter, so neither figure is served by a loop too slow
    // for it: at the defaults the liveness cadence (15 s) is a quarter of the
    // restatement's (60 s), so the loop ticks four times as often as it used
    // to and each deadline still fires inside its own `[cadence - tick,
    // cadence]`.
    let breathing = Restating::every(state.policy.still_there_every);
    let tick = restating.tick.min(breathing.tick);
    loop {
        tokio::time::sleep(tick).await;
        // **The verdicts this hub is holding, first** (D167). They belong on
        // this clock and not on one of their own for the reason the two
        // deadlines above share it: a held verdict's note *is* the traffic
        // that keeps its follower's link warm, and the loop below gives a job
        // whose record is terminal neither a restatement nor a breath — which
        // was right when a follower was always dropped at the verdict and
        // would now leave a requester waiting for files behind a proxy with
        // an idle timeout.
        for (job_id, silent_for) in state.verdicts_held() {
            look_at_a_held_verdict(&state, &job_id, silent_for, &breathing);
        }
        let followed = state.with_peers(|peers| {
            let followed: Vec<JobId> = peers
                .followers
                .iter()
                .filter(|(_, followers)| !followers.is_empty())
                .map(|(job, _)| job.clone())
                .collect();
            // And the pruning, under the same lock: a job nobody follows any
            // more leaves nothing behind in either map.
            let followed_now: HashSet<&JobId> = followed.iter().collect();
            peers.wait_told.retain(|job, _| followed_now.contains(job));
            peers
                .spoke_to_followers
                .retain(|job, _| followed_now.contains(job));
            followed
        });
        for job_id in followed {
            // **Both deadlines are judged before the record is read**, and
            // that order is deliberate: the two clocks live in memory, the
            // record lives on a disk, and a tick on which neither deadline is
            // due has no question to ask the disk. Reading first would mean
            // one file read per followed job per *tick* — and the tick is now
            // the shorter of two cadences, so the cost of the second deadline
            // would land on every hub whether anything was following a quiet
            // job or not.
            let (wait_due, breath_due) = state.with_peers(|peers| {
                (
                    restating.is_due(peers.wait_told.get(&job_id).map(|told| told.elapsed())),
                    breathing.is_due(
                        peers
                            .spoke_to_followers
                            .get(&job_id)
                            .map(|spoke| spoke.elapsed()),
                    ),
                )
            });
            if !wait_due && !breath_due {
                continue;
            }
            let Ok(Some(record)) = state.store.job(&job_id) else {
                continue;
            };
            // A job that is **over** gets neither of the two: its stream is
            // finished, and the hub has nothing left to say about it. A
            // follower is dropped at the verdict, so this covers the window
            // between the end being recorded and the list being emptied —
            // small, and the one place a frame after a verdict could come
            // from.
            if record.state.is_terminal() {
                continue;
            }
            // E-04. Only a job that is *queued* and has a reason on it: a
            // running job is not waiting, and a queued job the hub has not
            // yet had a reason for says nothing rather than saying nothing
            // useful.
            if wait_due && record.state == JobState::Queued {
                // The sentence the last pass built, which carries this
                // minute's position, before the one the record carries from
                // whenever the cause last changed (D171). E-04 asks for the
                // figures, and a restatement is exactly where a stale one
                // would be read.
                if let Some(reason) = state.why_it_waits(&job_id).or(record.waiting_for) {
                    // The clock is reset where the decision is taken, under
                    // the lock, so two passes cannot both find it due.
                    state
                        .with_peers(|peers| peers.wait_told.insert(job_id.clone(), Instant::now()));
                    let note = format!("this job is queued and has not started: {reason}");
                    for follower in state.followers_to_tell(&job_id) {
                        let _ = follower.send(FromHub::JobNote {
                            job_id: job_id.clone(),
                            note: note.clone(),
                        });
                    }
                }
            }
            // The breath, for **every** job that is not over, queued included. A
            // queued job is not safe by itself: its reason is restated at a
            // cadence an operator chooses (60 s by default, four times this
            // one), and a job the hub has not yet found a reason for is
            // restated never — either way a link that carries nothing.
            //
            // Judged again here rather than reused from above, because a note
            // that has just gone out is itself something the connection
            // carried, and a job that is both due at once wants the note and
            // not a breath behind it.
            let breath_due = breath_due
                && state.with_peers(|peers| {
                    breathing.is_due(
                        peers
                            .spoke_to_followers
                            .get(&job_id)
                            .map(|spoke| spoke.elapsed()),
                    )
                });
            if !breath_due {
                continue;
            }
            // Taking the list is what records that the hub spoke, so the next
            // breath is a cadence after this one.
            for follower in state.followers_to_tell(&job_id) {
                let _ = follower.send(FromHub::StillThere {
                    job_id: job_id.clone(),
                });
            }
        }
    }
}

/// Record why a queued job is not starting, and tell whoever is waiting for
/// it.
///
/// A job nobody is following still says on its disk why nothing is happening
/// to it (E-53), and the note goes out only when the reason is *news*,
/// because a requester following a job that waits an hour on one lock file
/// wants one sentence, not one per dispatch pass — M8 owns the periodic
/// re-statement E-04 asks for.
///
/// **News is what changed about this job, not what changed around it**
/// (D171). The whole `sentence` is three things: the job's **cause**
/// (`because` — the reading it waits for, the capability that is suspended,
/// the machine it sticks to), the job's own **standing** (the level it was
/// submitted at and the level the queue weighs it at now, E-18), and its
/// **place among the others** — `74th of 184`, and the three jobs ahead by
/// name.
///
/// Only the first two are facts about this job, and only they are news. The
/// place changes whenever any *other* job is submitted, started or ended, so
/// comparing whole sentences made every pass news for every queued job, and
/// each one cost a rewritten `job.json` and an appended `events.jsonl` line,
/// both fsynced, on a thread the runtime needed. A hundred and eighty-four
/// of those per pass is what stopped the hub answering (D171).
///
/// The countdown to the next level is deliberately *not* in the key either:
/// it is a clock, not an event, and it would make every job a write a
/// minute. The level it counts down to **is** in the key, so a job that ages
/// says so on its record the pass it happens.
///
/// Nothing is lost: the whole sentence is kept in [`Peers::waiting`] and is
/// what every reader is given, so E-04's figures stay as fresh as the last
/// pass without the disk hearing about it.
fn say_it_waits(
    state: &State,
    job_id: &JobId,
    standing: &queue::Standing,
    because: &str,
    sentence: &str,
) -> Result<()> {
    let news_of_this_job = format!(
        "{} weighed as {}; {because}",
        standing.declared, standing.effective
    );
    let news = state.with_peers(|peers| {
        let waiting = peers
            .waiting
            .entry(job_id.clone())
            .or_insert_with(|| Waiting {
                recorded: None,
                sentence: sentence.to_owned(),
            });
        waiting.sentence = sentence.to_owned();
        waiting.recorded.as_deref() != Some(news_of_this_job.as_str())
    });
    if !news {
        return Ok(());
    }
    let written = state
        .store
        .record_waiting(job_id, sentence)
        .with_context(|| format!("recording why {job_id} is waiting"))?;
    // Remembered once it is on the disk and not before: a write that failed
    // propagates out of here, and the next pass must judge this news all
    // over again rather than believe it was written down.
    state.with_peers(|peers| {
        if let Some(waiting) = peers.waiting.get_mut(job_id) {
            waiting.recorded = Some(news_of_this_job);
        }
    });
    if !written {
        return Ok(());
    }
    println!("herd-hub: {job_id} stays queued: {sentence}");
    // News resets the cadence: a requester told the reason this second does
    // not want the same sentence again a second later because a clock
    // happened to come round (E-04's flooding half).
    state.with_peers(|peers| peers.wait_told.insert(job_id.clone(), Instant::now()));
    let note = format!("this job is queued and has not started: {sentence}");
    for follower in state.followers_to_tell(job_id) {
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
            // Off the record, on every attempt, rather than remembered by
            // the runner that saw it first: a job requeued onto another
            // machine (E-21) runs the same four steps on the same commit,
            // and the record is the only thing that holds them (E-11, D106).
            declaration: record.declaration.clone(),
            timeout_s: record.timeout_s,
        })
        .is_err()
    {
        return Ok(false);
    }
    // And whoever is following it is told where it went, before a byte of its
    // output can arrive: that line is the first line of the output E-02
    // promises, so it cannot come second (D109).
    for follower in state.followers_to_tell(job_id) {
        let _ = follower.send(FromHub::RanOn {
            job_id: job_id.clone(),
            runner: runner_name.to_owned(),
            why: why.to_owned(),
        });
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
    // Lives exactly as long as this connection, which is what makes the
    // runner's name part of its key without being in it, and what frees it
    // with no eviction policy when the runner goes (D151).
    let mut said = AlreadySaid::new();
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
                    if mind_a_reported_job(state, name, outbox, &mut said, &job) {
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
            // What the runner did with a position this hub asked it for, and
            // the one thing `herdctl switch` may read as success (E-33, D99).
            ToHub::SwitchApplied {
                switch,
                persistence,
            } => {
                // It is evidence about the position too, so the hub's own view
                // moves on it rather than waiting for the health report that
                // is on its way: the two carry the same position and the
                // earlier one wins.
                if let Some(said) = state.record_switch(outbox, switch.clone()) {
                    println!("herd-hub: {said}");
                    dispatch_queued_jobs(state)?;
                }
                match &persistence {
                    herd_proto::SwitchPersistence::Written { path } => println!(
                        "herd-hub: runner {name} has written its switch ({}) down in {path}",
                        switch.sentence()
                    ),
                    // On stderr, because it is the half of E-33 that did not
                    // happen: the machine is withdrawn and a restart of it
                    // would come back claiming.
                    herd_proto::SwitchPersistence::NotWritten { why } => eprintln!(
                        "herd-hub: runner {name} holds its switch ({}) in memory only, so a \
                         restart of it would forget the position: {why}",
                        switch.sentence()
                    ),
                }
                state.record_switch_applied(
                    outbox,
                    Acknowledged {
                        switch,
                        persistence,
                    },
                );
            }
            ToHub::JobStarted { job_id, attempt } => {
                if !is_current(state, name, outbox, &mut said, &job_id, attempt) {
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
                        for follower in state.followers_to_tell(&job_id) {
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
                if !is_current(state, name, outbox, &mut said, &job_id, attempt) {
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
            // One piece of one file of a streamed collection (D148).
            //
            // The verdict announced the manifest, so everything needed to
            // judge this chunk is already on the record: which paths exist,
            // how long each is, what each must hash to. The store does the
            // judging and the writing; what happens here is the one decision
            // that belongs to the link — acknowledging the collection when
            // the last file lands, which is what lets the runner drop its
            // spool.
            ToHub::JobFileChunk {
                job_id,
                attempt,
                path,
                offset,
                data,
            } => {
                // **Not `is_current`, and the difference is the design.**
                // Every other frame about a job is about an attempt that is
                // *running*, so `is_current` refuses one about a job that is
                // over — which is right for a log chunk and exactly wrong
                // here: the verdict goes first and the bytes follow (D148),
                // so a file chunk is *always* about a job that has ended.
                // Checked by `is_current`, every chunk of every collection
                // was silently dropped and answered with a `StaleAttempt`.
                //
                // What has to be true instead is that this runner produced
                // the collection this hub is waiting for: the attempt on the
                // record, from the machine that ran it.
                if let Err(why) = files_may_land(state, name, &job_id, attempt) {
                    eprintln!("herd-hub: a file chunk of {job_id} is refused: {why}");
                    continue;
                }
                match state
                    .store
                    .write_output_chunk(&job_id, &path, offset, &data)
                {
                    // A byte landed, so a verdict held for this collection is
                    // waiting on something that is working: the bound is on
                    // silence, not on how long a transfer takes (D167).
                    Ok(crate::job::ChunkLanded::More) => {
                        state.the_collection_moved(&job_id);
                    }
                    Ok(crate::job::ChunkLanded::Complete) => {
                        if let Err(error) = state.store.clear_output_parts(&job_id) {
                            eprintln!(
                                "herd-hub: {job_id} has every file and its part directory \
                                 will not go away: {error}"
                            );
                        }
                        let _ = outbox.send(FromHub::JobFilesAck {
                            job_id: job_id.clone(),
                            attempt,
                        });
                        println!("herd-hub: {job_id} has every file of its collection");
                        // And the verdict this hub was holding for exactly
                        // this moment (D167). Here rather than on a clock,
                        // because the only thing that can complete a
                        // collection is a chunk arriving.
                        the_collection_is_whole(state, &job_id);
                    }
                    // A chunk that cannot be written is said out loud and the
                    // collection is **not** acknowledged, so the runner goes
                    // on holding it: the hub has not got the bytes, and
                    // pretending otherwise is the one way a result gets
                    // destroyed (D19).
                    //
                    // The hub also tells the runner what it really holds, so
                    // a disagreement about an offset resolves itself instead
                    // of becoming a loop of refusals.
                    Ok(crate::job::ChunkLanded::Refused(why)) => {
                        eprintln!("herd-hub: a file chunk of {job_id} was not written: {why}");
                        if let Ok(held) = state.store.output_holdings(&job_id) {
                            let _ = outbox.send(FromHub::JobFilesHeld {
                                job_id: job_id.clone(),
                                attempt,
                                held,
                            });
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "herd-hub: writing a file chunk of {job_id} failed: {error}. Its \
                             runner still holds the collection."
                        );
                    }
                }
            }
            ToHub::StepFinished {
                job_id,
                attempt,
                step,
            } => {
                if !is_current(state, name, outbox, &mut said, &job_id, attempt) {
                    continue;
                }
                // Said out loud and recorded, and a failure to record it is
                // said rather than retried: a step line is neither output nor
                // a verdict, the complete list arrives again with the end,
                // and nothing about a running job may hang on writing one
                // (D19's rule is about results, and this is not one).
                println!("herd-hub: {job_id} attempt {attempt} {}", step.line());
                if let Err(error) = state.store.record_step(&job_id, attempt, step) {
                    eprintln!("herd-hub: recording an executor step of {job_id} failed: {error}");
                }
            }
            ToHub::JobFinished {
                job_id,
                attempt,
                outcome,
                chunks_sent,
                metrics,
                steps,
                outputs,
            } => {
                if !is_current(state, name, outbox, &mut said, &job_id, attempt) {
                    continue;
                }
                // The steps before the end: the attempt's four lines are part
                // of the record this verdict is written into, and a reader
                // that found the verdict first would see a job that ended
                // with no preparation behind it (D77). Each one replaces the
                // live line of the same name, so this is the complete list
                // whatever arrived while the job ran.
                for step in steps {
                    if let Err(error) = state.store.record_step(&job_id, attempt, step) {
                        eprintln!(
                            "herd-hub: recording an executor step of {job_id} failed: {error}"
                        );
                    }
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
                if !record_the_end(state, &job_id, &outcome, &metrics, &outputs).await {
                    // The hub could not write it. Nothing is acknowledged, so
                    // the runner keeps holding it; the hub goes on trying in
                    // the background and never destroys it (D19).
                    continue;
                }
                state.attempt_is_over(&job_id);
                let _ = outbox.send(FromHub::JobResultAck {
                    job_id: job_id.clone(),
                });
                // Off the record, not out of the frame that just arrived, so
                // a requester following live and a `herdctl wait` an hour
                // later are given the same bytes by the same code: one answer
                // to "what did this job leave?", which is the disk (D131).
                //
                // **And not yet, if the bytes are still coming** (D167). A
                // streamed collection follows the verdict by design, so this
                // used to report a job finished while its files were still
                // arriving — and a requester fetching on that signal got a
                // short collection. The record is already written, which is
                // what frees the slot; what waits is the sentence.
                tell_or_hold_the_verdict(state, &job_id, &outcome, &metrics);
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
                if !is_current(state, name, outbox, &mut said, &job_id, attempt) {
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

/// One line of the hub's journal, written to the descriptor rather than
/// through `eprintln!`.
///
/// The same bytes to the same place; the difference is that they can be
/// counted. `eprintln!` consults libtest's per-thread output capture, which
/// std also hands to every thread a test spawns, so under `cargo test` the
/// macro's lines never reach file descriptor 2 and no test can measure how
/// many of them a path wrote. For D151 that count is the whole assertion —
/// the defect was never what the line said, it was that one true line was
/// written once per frame — so this path writes where a test can divert it.
///
/// The twin of `herd_runner::journal`, and deliberately a twin rather than
/// something shared: a logging helper is not a protocol message, so it has no
/// business in `herd-proto`, and four lines in each binary is cheaper than a
/// crate that exists to hold them.
fn journal(line: &str) {
    use std::io::Write;
    // A journal that cannot be written to is not worth bringing a hub down
    // over: the line is lost, the service is not.
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

/// Why a frame about an attempt was refused, to the precision the journal
/// needs to avoid repeating itself (D151).
///
/// Only the *kind*, not the sentence: two frames refused for the same reason
/// about the same attempt produce the same line, and the second one is worth
/// nothing to a reader. A change of kind is worth a line, which is why this
/// is part of the key rather than collapsed away — an attempt that was
/// superseded and whose job then ended is refused for a genuinely different
/// reason the second time, and saying the first reason for ever would make
/// the hub's journal wrong rather than merely quiet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Superseded,
    JobIsOver,
    AnotherRunner,
    Unreadable,
}

/// How many refusals one connection remembers having already written down
/// (D151).
///
/// A connection holds a handful of a runner's attempts at a time, so this is
/// wide enough that the refusals in flight cannot push each other out; the
/// ring exists for the long tail, a link that stays up for days and
/// accumulates written-off attempts one at a time. Forgetting the oldest
/// costs at most a second line about an attempt whose frames stopped arriving
/// long ago.
const REFUSALS_REMEMBERED: usize = 32;

/// The refusals this **one connection** has already written to the journal.
///
/// Per connection, and that is the whole design rather than an accident of
/// where it is declared. The hub refuses frames from every runner it serves,
/// so a memory of "attempts already announced" has to be keyed by runner as
/// well as by attempt or two runners would silence each other. Living inside
/// [`relay_from_runner`], which serves exactly one connection, makes the
/// runner's name part of the key implicitly and for free — and gives the
/// bound a structural half the runner's own ring cannot have: when the
/// connection ends the memory goes with it, so nothing survives a runner that
/// is gone.
///
/// A reconnect therefore allows one line again per attempt. That is right,
/// not a leak: a runner that dialled back and is still flushing a dead
/// attempt is a new fact about a link that is flapping, and exactly what
/// somebody reading a stalled host wants to see once.
struct AlreadySaid {
    said: VecDeque<(JobId, u32, Refusal)>,
}

impl AlreadySaid {
    fn new() -> Self {
        Self {
            said: VecDeque::new(),
        }
    }

    /// Write `line`, unless this connection has already written one about
    /// this attempt for this reason. Returns whether it wrote.
    fn say_once(&mut self, job_id: &JobId, attempt: u32, why: Refusal, line: &str) -> bool {
        let key = (job_id.clone(), attempt, why);
        if self.said.contains(&key) {
            return false;
        }
        if self.said.len() == REFUSALS_REMEMBERED {
            self.said.pop_front();
        }
        self.said.push_back(key);
        journal(line);
        true
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
///
/// `said` is this connection's memory of what it has already written down,
/// so that a burst of refused frames about one attempt leaves one line and
/// not one per frame (D151).
fn is_current(
    state: &State,
    from: &str,
    outbox: &Outbox,
    said: &mut AlreadySaid,
    job_id: &JobId,
    attempt: u32,
) -> bool {
    // Every arm below refuses by name on the wire **every time**, and is
    // quiet in the journal after the first: the frame the runner gets back is
    // what stops it resending, so silencing that would break E-21, while
    // silencing the repeated journal line costs a reader nothing and costs a
    // stalled host 1496 writes it did not need to make.
    let mut refused = |why: Refusal, reason: String| {
        said.say_once(job_id, attempt, why, &format!("herd-hub: {reason}"));
        let _ = outbox.send(FromHub::StaleAttempt {
            job_id: job_id.clone(),
            attempt,
            reason,
        });
        false
    };
    match state.standing_of(job_id, attempt, from) {
        // Every frame about the attempt that is current — a start, a chunk, an
        // end, a health report naming it — is word that the attempt is alive,
        // and that is what the watchdog times (E-21).
        Ok(Standing::Current) => {
            state.attempt_was_heard_of(job_id);
            true
        }
        Ok(Standing::Superseded) => refused(
            Refusal::Superseded,
            format!(
                "attempt {attempt} of {job_id} is not the attempt this hub is waiting for; \
                 nothing of it is recorded"
            ),
        ),
        // A record that is over. Refused by name, with the state it reached:
        // whoever sent this is holding something about a job whose story is
        // told, and the one thing that must not happen is for it to be
        // appended to a log a reader has already been given as complete.
        Ok(Standing::JobIsOver { state }) => refused(
            Refusal::JobIsOver,
            a_frame_about_a_job_that_is_over_reason(job_id, &state, attempt),
        ),
        // Somebody else's attempt. Refused by name, and the clock on the real
        // attempt is untouched: a frame from the wrong runner is not evidence
        // that the attempt is alive, and letting it count would have a
        // bystander keep a dead attempt from ever being requeued.
        Ok(Standing::AnotherRunner { holder }) => refused(
            Refusal::AnotherRunner,
            not_this_runner_s_attempt_reason(job_id, attempt, &holder, from),
        ),
        // Not knowing is not a licence to write: the frame is left alone and
        // the runner keeps holding whatever it was about. Said once too — a
        // store that cannot be read is read no better on the second frame of
        // a burst, and this arm floods on exactly the same trigger.
        Err(error) => {
            said.say_once(
                job_id,
                attempt,
                Refusal::Unreadable,
                &format!("herd-hub: reading which attempt {job_id} is on failed: {error}"),
            );
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
fn mind_a_reported_job(
    state: &State,
    from: &str,
    outbox: &Outbox,
    said: &mut AlreadySaid,
    job: &RunningJob,
) -> bool {
    if !is_current(state, from, outbox, said, &job.job_id, job.attempt) {
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
/// Tell a runner that has just joined where to resume every collection this
/// hub is still owed (D148).
///
/// Only its own: a collection belongs to the attempt that produced it, and
/// telling one runner about another's would have it look for a spool it never
/// had. One that *is* missing its spool — a runner restarted from a cleaned
/// data root — says so by name, which is the visible no-holder state D148
/// insists on instead of a job whose evidence never arrives and nobody
/// notices.
fn ask_for_the_files_still_owed(state: &State, name: &str, outbox: &Outbox) {
    let owed = match state.store.collections_owed() {
        Ok(owed) => owed,
        Err(error) => {
            eprintln!("herd-hub: cannot tell which collections are outstanding: {error}");
            return;
        }
    };
    for collection in owed {
        if collection.runner != name {
            continue;
        }
        println!(
            "herd-hub: {} still owes this hub files of {}; asking it to resume",
            name, collection.job_id
        );
        let _ = outbox.send(FromHub::JobFilesHeld {
            job_id: collection.job_id,
            attempt: collection.attempt,
            held: collection.held,
        });
    }
}

/// Hand a requester the files it asked for, and name the ones that are gone.
///
/// **This is the function that makes an absent output answerable rather than
/// empty** (D148). The manifest outlives the bytes: a record keeps every
/// path, length and digest for as long as it exists, and only the bytes are
/// ever removed — by a runner that never came back today, by M12's sweeper
/// later. So a path on the manifest with no file behind it is reported as
/// what it is, with its size and a reason, and a requester never receives a
/// short collection it would read as a whole one.
///
/// Read off the disk a chunk at a time. Nothing here holds a file, which is
/// the whole reason this frame exists instead of the verdict carrying it.
/// Whether a file chunk is one this hub will write, or why it is not.
///
/// The counterpart to [`is_current`] for the one kind of frame that is
/// legitimately about a job that has ended (D148). Three things have to hold,
/// and each of them is a different mistake being prevented:
///
/// * the record says this collection **streamed** — otherwise a peer could
///   write files into the record of a job whose output came inline, or of one
///   that left none at all;
/// * the attempt is the one on the record — a second attempt's files are not
///   the first's, which is why every file frame carries the number;
/// * the runner is the one that ran that attempt. There is no authentication
///   before M15, so this is not a credential; it is the same honesty the
///   attempt check is, and it keeps one machine's collection out of another
///   machine's hands by name rather than by trust.
fn files_may_land(
    state: &State,
    from: &str,
    job_id: &JobId,
    attempt: u32,
) -> std::result::Result<(), String> {
    let record = match state.store.job(job_id) {
        Ok(Some(record)) => record,
        Ok(None) => return Err(format!("no such job: {job_id}")),
        Err(error) => return Err(format!("{job_id}'s record will not read: {error}")),
    };
    if !record.outputs.streamed {
        return Err(format!(
            "{job_id}'s record names no streamed collection, so this hub is expecting no \
             file of it"
        ));
    }
    // A collection the sweeper has taken is not one this hub will take
    // back (D152). A runner redialling with a spool of something that was
    // evicted while it was away would otherwise refill the disk the sweep
    // had just freed, and the sweep would take it again on the next
    // verdict: two correct mechanisms in a loop.
    if let Some(removed) = &record.outputs.removed {
        return Err(format!(
            "{job_id}'s collection was {}, so this hub will not write it again",
            removed.why
        ));
    }
    let Some(ran) = record.attempts.last() else {
        return Err(format!(
            "{job_id} has no attempt that could have produced files"
        ));
    };
    if ran.number != attempt {
        return Err(format!(
            "attempt {attempt} of {job_id} did not produce its collection; attempt {} did",
            ran.number
        ));
    }
    if ran.runner != from {
        return Err(format!(
            "{from} is offering files of {job_id}, which ran on {}",
            ran.runner
        ));
    }
    Ok(())
}

fn send_the_files(state: &State, job_id: &JobId, paths: &[String], outbox: &Outbox) {
    let manifest = match state.store.job_outputs(job_id, false) {
        Ok(outputs) => outputs,
        Err(error) => {
            eprintln!("herd-hub: the manifest of {job_id} could not be read: {error}");
            let _ = outbox.send(FromHub::JobFilesSent {
                job_id: job_id.clone(),
                absent: Vec::new(),
            });
            return;
        }
    };
    // **What the hub is still expecting, read once.** This is what tells an
    // absence the requester should wait for from one it should give up on,
    // and the two look identical in a list of names: the verdict goes first
    // and the bytes follow (D148), so a requester asking the instant its job
    // ended is asking for something genuinely in flight. Told "gone" it
    // would report a lost result for a job whose evidence was seconds away.
    // Read, not guessed: an error here would make a collection still
    // crossing look like one that is never coming, which is the one
    // confusion this figure exists to prevent. So it is said out loud rather
    // than defaulted away.
    let owed = match state.store.collections_owed() {
        Ok(owed) => owed.into_iter().find(|owed| &owed.job_id == job_id),
        Err(error) => {
            eprintln!("herd-hub: cannot tell whether {job_id}'s files are still coming: {error}");
            None
        }
    };
    let coming = owed.is_some();
    // **Why the bytes are gone, when they are gone on purpose** (D152). The
    // sweeper writes one sentence on the record, with the date and the
    // budget in it, and this is where a requester reads it: "not on this
    // hub's disk" is true of an evicted collection and useless, because the
    // reader's next question is whether Herd lost it.
    let removed = match state.store.job(job_id) {
        Ok(Some(record)) => record.outputs.removed.map(|removed| removed.why),
        _ => None,
    };
    let held_of = |path: &str| -> u64 {
        owed.as_ref()
            .and_then(|owed| owed.held.iter().find(|file| file.path == path))
            .map(|file| file.bytes)
            .unwrap_or(0)
    };
    let mut absent = Vec::new();
    let mut sent = 0usize;
    for file in &manifest.files {
        // An empty `paths` means the whole collection; anything else is a
        // requester saying what it is missing, which is how a `--outputs-to`
        // that died is simply run again without fetching what it already has.
        if !paths.is_empty() && !paths.iter().any(|wanted| wanted == &file.path) {
            continue;
        }
        if herd_proto::check_relative(&file.path).is_err() {
            absent.push(herd_proto::AbsentFile {
                path: file.path.clone(),
                len: file.len,
                why: "this hub will not read a path that climbs out of the job's directory"
                    .to_owned(),
                held: 0,
                still_coming: false,
            });
            continue;
        }
        match send_one_file(state, job_id, &file.path, outbox) {
            Ok(()) => sent += 1,
            Err(why) => absent.push(herd_proto::AbsentFile {
                path: file.path.clone(),
                len: file.len,
                why: match (&removed, coming) {
                    (Some(removed), _) => removed.clone(),
                    (None, true) => {
                        "its runner has not finished sending it to this hub yet".to_owned()
                    }
                    (None, false) => why,
                },
                held: held_of(&file.path),
                // Never: the sweeper is the one absence that is final by
                // construction, and telling a requester to wait for it
                // would be thirty seconds of silence and then a worse
                // sentence (D150's stall bound).
                still_coming: coming && removed.is_none(),
            }),
        }
    }
    // **Somebody has now read this collection** (D152). Written once ever,
    // and only when bytes really went: a `--outputs-to` polls while a
    // transfer crosses, and a round that handed over nothing is not a read.
    // The sweeper keeps a collection nobody has opened longer than one
    // that has been, so this figure decides what survives.
    if sent > 0 {
        if let Err(error) = state.store.note_outputs_fetched(job_id) {
            eprintln!("herd-hub: {job_id} was fetched and the record will not say so: {error}");
        }
    }
    let _ = outbox.send(FromHub::JobFilesSent {
        job_id: job_id.clone(),
        absent,
    });
}

/// One file, in chunks, or why it could not be sent.
fn send_one_file(
    state: &State,
    job_id: &JobId,
    path: &str,
    outbox: &Outbox,
) -> std::result::Result<(), String> {
    let take = herd_proto::OUTPUT_CHUNK_BYTES as usize;
    let mut offset = 0u64;
    loop {
        let data = match state.store.read_output_chunk(job_id, path, offset, take) {
            Ok(Some(data)) => data,
            Ok(None) => {
                return Err("named on this job's record and not on this hub's disk".to_owned())
            }
            Err(error) => return Err(format!("could not be read back: {error}")),
        };
        let read = data.len();
        // A zero-length file still gets one frame, because the requester has
        // to be told to create it: an empty `results.json` is a real file and
        // not the absence of one. Past the first frame, nothing read means
        // the end.
        if read == 0 && offset > 0 {
            return Ok(());
        }
        if outbox
            .send(FromHub::JobFile {
                job_id: job_id.clone(),
                path: path.to_owned(),
                offset,
                data,
            })
            .is_err()
        {
            // The requester went away mid-collection. Not an error about the
            // file: there is nobody left to tell.
            return Ok(());
        }
        offset += read as u64;
        if read < take {
            return Ok(());
        }
    }
}

/// Where a job's streamed collection stands, for the one decision that
/// depends on it: may this job's verdict go to its requester yet? (D167)
enum TheCollection {
    /// Nothing is outstanding. Either the job left no files, or they rode the
    /// verdict, or every one of them is on this hub at its declared length —
    /// so a requester that fetches on the verdict gets the whole set.
    Whole,
    /// Bytes are genuinely crossing and the machine sending them is here.
    /// Carries how far it has got, for the requester to read while it waits.
    Crossing(String),
    /// Bytes are outstanding and waiting longer buys nothing: the runner has
    /// gone, or nothing has landed for [`COLLECTION_SILENCE`]. Carries what
    /// is missing and why, because the requester is about to be told the job
    /// is over with a collection that is short.
    StoppedShort(String),
}

/// How far a collection that is still crossing has got, in the figures a
/// requester needs: files here of files promised, bytes here of bytes
/// promised, and which machine is sending them.
fn how_far_the_collection_got(record: &JobRecord, owed: &crate::job::CollectionOwed) -> String {
    let promised: u64 = record.outputs.files.iter().map(|file| file.bytes).sum();
    let here: u64 = owed.held.iter().map(|file| file.bytes).sum();
    format!(
        "{} of {} output file(s) and {} of {} have reached this hub from {}",
        owed.held.len(),
        record.outputs.files.len(),
        herd_proto::show_bytes(here),
        herd_proto::show_bytes(promised),
        owed.runner,
    )
}

/// Ask the disk where a job's collection stands.
///
/// `silent_for` is how long the verdict has already been held with nothing
/// landing, or `None` the first time the question is asked — nothing can have
/// gone silent before the hub started listening for it.
///
/// **A store that will not answer is never a reason to hold a verdict**, and
/// that is the same ruling [`read_the_outputs`] takes: the end is recorded,
/// somebody is waiting for it, and an unreadable disk is said out loud rather
/// than turned into a wait with no end. The absences then answer for
/// themselves on the fetch, which is where a requester can act on them.
fn where_the_collection_stands(
    state: &State,
    job_id: &JobId,
    silent_for: Option<Duration>,
) -> TheCollection {
    let owed = match state.store.collection_owed(job_id) {
        Ok(None) => return TheCollection::Whole,
        Ok(Some(owed)) => owed,
        Err(error) => {
            eprintln!(
                "herd-hub: whether {job_id}'s files have all arrived cannot be read, so its \
                 verdict is not held back for them: {error}"
            );
            return TheCollection::Whole;
        }
    };
    let Ok(Some(record)) = state.store.job(job_id) else {
        return TheCollection::Whole;
    };
    let got = how_far_the_collection_got(&record, &owed);
    // **The runner is the first bound, and it needs no clock.** A machine
    // that is not connected is sending nothing, so there is no silence to
    // time: a verdict held for it would wait out the whole bound to learn
    // what is already known. This is also what answers a `herdctl wait` an
    // hour later on a collection whose runner never came back — at once,
    // rather than after half a minute of nothing.
    if state.outbox_of(&owed.runner).is_none() {
        return TheCollection::StoppedShort(format!(
            "{got}, and {} is not connected to this hub",
            owed.runner
        ));
    }
    // And the second: a runner that is here and sending nothing.
    if silent_for.is_some_and(|silent| silent >= state.policy.collection_silence) {
        return TheCollection::StoppedShort(format!(
            "{got}, and no byte of it has landed for {:?}",
            state.policy.collection_silence
        ));
    }
    TheCollection::Crossing(got)
}

/// Hand a job's verdict to everybody waiting for it, and stop following it.
///
/// The one place a requester-facing [`FromHub::JobFinished`] is built for a
/// job a runner reported on, so that the files on it are read off the disk
/// once and by one piece of code (D131).
fn tell_them_how_it_ended(
    state: &State,
    job_id: &JobId,
    outcome: &JobOutcome,
    metrics: &JobMetrics,
) {
    hand_them_the_verdict(
        state,
        job_id,
        outcome,
        metrics,
        state.stop_following(job_id),
    );
}

/// The same verdict to a chosen list of followers (D167).
///
/// Nothing is read off the disk for an empty list, which is the common case
/// on the path that hands the verdict to the followers wanting no files:
/// usually there are none, and a manifest read per verdict for nobody would
/// be a cost paid by every job in the fleet.
fn hand_them_the_verdict(
    state: &State,
    job_id: &JobId,
    outcome: &JobOutcome,
    metrics: &JobMetrics,
    to: Vec<Outbox>,
) {
    if to.is_empty() {
        return;
    }
    let kept = read_the_outputs(state, job_id);
    let commit = the_commit_it_ran(state, job_id);
    for follower in to {
        let _ = follower.send(FromHub::JobFinished {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            metrics: metrics.clone(),
            outputs: kept.clone(),
            commit: commit.clone(),
        });
    }
}

/// Tell a job's requesters how it ended, **or hold that sentence back until
/// its collection is whole** (D167).
///
/// This is the whole of the defect it fixes. A streamed collection follows
/// the verdict by design (D148), so for the minute or two a few hundred
/// megabytes take to cross, "this job finished successfully" and "this job's
/// evidence is on the hub" were two different facts reported as one — and a
/// requester that fetched on the first got a short directory with 195 of 657
/// files named as absent, while a second fetch minutes later got all of
/// them. The fix is not to make the transfer faster or the fetch cleverer: it
/// is that **a requester is told a job finished once there is something whole
/// to fetch**.
///
/// What is deliberately *not* delayed is the record. The end is on the disk
/// before this is called, and that is what frees the job's slot on its runner
/// — a dispatch pass reads occupancy off the records and from nothing else
/// (D79, [`crate::matcher`]). Holding the record back instead would be the
/// same sentence arriving correctly and a one-slot machine standing idle for
/// two minutes after its work was done, once per job.
///
/// **Only the followers that said they want the files wait**, which is what
/// [`Follower::wants_files`] is for. A `herdctl run` with no `--outputs-to`
/// receives no byte of the collection and never asks about it, so it is told
/// the moment the end is recorded — D148's promise, kept — while a `herdctl
/// wait --outputs-to` beside it waits for the collection it is about to
/// fetch. A requester that does not mention the field is read as wanting
/// them, so an older `herdctl` keeps the safe behaviour rather than being
/// handed the early verdict by the upgrade that fixed this.
fn tell_or_hold_the_verdict(
    state: &State,
    job_id: &JobId,
    outcome: &JobOutcome,
    metrics: &JobMetrics,
) {
    // **A follower that said it wants no files is never held** (D167). The
    // verdict is the only thing it is waiting for, so it is told the moment
    // the end is recorded — which is what `herdctl run` with no
    // `--outputs-to` has always done and what D148 promised it would keep
    // doing. Taken first, so what is left to hold is exactly the followers
    // the hold is for.
    hand_them_the_verdict(
        state,
        job_id,
        outcome,
        metrics,
        state.take_the_followers_that(job_id, false),
    );
    if !state.anybody_wants_the_files(job_id) {
        return;
    }
    match where_the_collection_stands(state, job_id, None) {
        TheCollection::Whole => tell_them_how_it_ended(state, job_id, outcome, metrics),
        TheCollection::StoppedShort(why) => {
            say_the_collection_is_short(state, job_id, &why);
            tell_them_how_it_ended(state, job_id, outcome, metrics);
        }
        TheCollection::Crossing(got) => {
            state.hold_the_verdict(job_id, outcome, metrics);
            println!("herd-hub: {job_id} is recorded and its verdict waits for its files: {got}");
            say_the_collection_is_crossing(state, job_id, &got);
        }
    }
}

/// The sentence a requester reads while its verdict is held.
///
/// Said the way a queued job's `waiting_for` says itself (E-04): a wait with
/// no explanation is the thing being fixed here as much as the short
/// directory was. It goes out when the verdict is first held and again on the
/// clock, so a requester that waits sixty seconds for files has four lines
/// saying which files and from where, rather than a minute of silence after a
/// job it watched finish.
fn say_the_collection_is_crossing(state: &State, job_id: &JobId, got: &str) {
    let note = format!(
        "this job has ended and its files are still arriving: {got}. Its verdict follows \
         when the collection is whole, so that a fetch on it gets every file."
    );
    for follower in state.followers_to_tell(job_id) {
        let _ = follower.send(FromHub::JobNote {
            job_id: job_id.clone(),
            note: note.clone(),
        });
    }
}

/// The sentence a requester reads when a collection is **not** going to
/// complete, said before the verdict rather than instead of it.
///
/// A wait that cannot be rewarded has to end, and it has to end saying why:
/// the verdict goes out next, the fetch that follows names every missing path
/// off the manifest, and `herdctl` exits non-zero rather than 0 on a
/// collection it was asked for and did not get. What must never happen is the
/// third thing — a `wait` that hangs with nothing said (D167).
fn say_the_collection_is_short(state: &State, job_id: &JobId, why: &str) {
    eprintln!("herd-hub: {job_id}'s collection did not finish arriving: {why}");
    let note = format!(
        "this job's files did not all arrive: {why}. Its verdict follows now, and every \
         file this hub does not have is named on the record."
    );
    for follower in state.followers_to_tell(job_id) {
        let _ = follower.send(FromHub::JobNote {
            job_id: job_id.clone(),
            note: note.clone(),
        });
    }
}

/// The collection is whole: let the verdict go (D167).
///
/// Called where the last byte lands, which is the event rather than a clock —
/// the only thing that can complete a collection is a chunk arriving, so a
/// timer would be a second mechanism waking up to find nothing to do (D152's
/// own reasoning about the sweeper).
fn the_collection_is_whole(state: &State, job_id: &JobId) {
    let Some((outcome, metrics)) = state.take_the_held_verdict(job_id) else {
        return;
    };
    println!("herd-hub: {job_id}'s collection is whole, so its verdict goes to its requesters");
    tell_them_how_it_ended(state, job_id, &outcome, &metrics);
}

/// One look at one held verdict, on the clock that keeps a followed job's
/// stream alive (D167).
///
/// Four answers, and each of them ends the hold or restates it:
///
/// * nobody is following any more, so there is nobody to hold it for — the
///   entry goes, and a later `herdctl wait` derives the whole question off
///   the disk again in [`answer_a_wait`];
/// * the collection is whole, which the chunk path normally catches first;
///   this is the belt for a chunk that landed before the verdict did, or for
///   a hub that restarted mid-transfer;
/// * it cannot complete — the runner is gone, or nothing has landed for
///   [`COLLECTION_SILENCE`] — so the verdict goes out with the absence named;
/// * it is still crossing, and the requester is told so again.
fn look_at_a_held_verdict(
    state: &State,
    job_id: &JobId,
    silent_for: Duration,
    breathing: &Restating,
) {
    if !state.anybody_wants_the_files(job_id) {
        let _ = state.take_the_held_verdict(job_id);
        return;
    }
    match where_the_collection_stands(state, job_id, Some(silent_for)) {
        TheCollection::Whole => the_collection_is_whole(state, job_id),
        TheCollection::StoppedShort(why) => {
            let Some((outcome, metrics)) = state.take_the_held_verdict(job_id) else {
                return;
            };
            say_the_collection_is_short(state, job_id, &why);
            tell_them_how_it_ended(state, job_id, &outcome, &metrics);
        }
        // Restated on the liveness cadence and not on every tick, for the
        // reason that cadence exists: the note *is* traffic, so it keeps the
        // link warm through a proxy with an idle timeout at the same time as
        // it explains the wait. One mechanism, one figure, and a bound a
        // reader can compute — a sixty-second wait is four lines.
        TheCollection::Crossing(got) => {
            let due = state.with_peers(|peers| {
                breathing.is_due(
                    peers
                        .spoke_to_followers
                        .get(job_id)
                        .map(|spoke| spoke.elapsed()),
                )
            });
            if due {
                say_the_collection_is_crossing(state, job_id, &got);
            }
        }
    }
}

/// What a job's record holds under `files/`, for the frame a requester gets.
///
/// A read that fails is **not** a reason to withhold the verdict: the files
/// are part of the record and the verdict is the answer somebody is waiting
/// for, so a store that will not give them up is said out loud and the job
/// still ends. Nothing is invented in their place — the requester is given no
/// file and the sentence is on the hub's stderr, where an operator reads it.
///
/// **The manifest always, the bytes only if they fit on the frame.** A
/// streamed collection's record holds hundreds of megabytes and this frame is
/// the verdict, so what goes on it is the manifest — which is also exactly
/// what the runner's own verdict carried. A requester that wants the files
/// then asks ([`herd_proto::ToHub::AskJobFiles`]) and gets them a chunk at a
/// time; one that does not never pays for them. The inline carriage keeps its
/// single round trip, because its bytes are already here and nothing is
/// missing to ask for (D137, D148).
fn read_the_outputs(state: &State, job_id: &JobId) -> herd_proto::JobOutputs {
    let inline = match state.store.job_outputs(job_id, false) {
        Ok(outputs) => !outputs.streamed,
        // Unreadable either way, and the arm below says so with the error.
        Err(_) => true,
    };
    match state.store.job_outputs(job_id, inline) {
        Ok(outputs) => outputs,
        Err(error) => {
            eprintln!(
                "herd-hub: the files kept for {job_id} could not be read back, so its \
                 requester is given none: {error}"
            );
            herd_proto::JobOutputs::default()
        }
    }
}

/// What the attempt that produced this job's verdict said about the revision
/// it really ran, read off the record (D-NEXT).
///
/// Off the record and not out of the frame being handled, for
/// [`read_the_outputs`]' own reason: a `herdctl wait` attaching an hour after
/// the job ran has to be told the same thing as the requester who followed
/// it, and the record is the only place both of them can read.
///
/// The **last** attempt, because a job requeued onto another machine (E-21)
/// prepared twice and the verdict is the last attempt's. `None` for a job
/// that is not a declared command, for one no runner ever reached, and for
/// one whose record cannot be read — none of which is a claim that the tree
/// was at the declared commit.
fn the_commit_it_ran(state: &State, job_id: &JobId) -> Option<herd_proto::CommitRun> {
    match state.store.job(job_id) {
        Ok(record) => record?
            .attempts
            .last()
            .and_then(|attempt| attempt.commit.clone()),
        Err(error) => {
            eprintln!(
                "herd-hub: the revision {job_id} ran could not be read back, so its \
                 requester is told nothing about it: {error}"
            );
            None
        }
    }
}

async fn record_the_end(
    state: &State,
    job_id: &JobId,
    outcome: &JobOutcome,
    metrics: &JobMetrics,
    outputs: &herd_proto::JobOutputs,
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
        //
        // The job's kept files go down in the same try and for the same
        // reason (D131). They are part of the record a `finished` state
        // promises, and a hub that acknowledged a verdict whose
        // `results.json` it had failed to write would have told the runner to
        // forget the only copy of it — the workspace is already reclaimed by
        // the time this frame exists.
        match state
            .store
            .record_metrics(job_id, metrics.clone())
            .and_then(|()| state.store.record_outputs(job_id, outputs))
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
    for follower in state.note_a_stall(job_id, why, outcome, metrics, outputs) {
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
        let pending: Vec<(JobId, JobOutcome, JobMetrics, herd_proto::JobOutputs)> = state
            .with_peers(|peers| {
                peers
                    .unwritten
                    .iter()
                    .map(|(id, (outcome, metrics, outputs))| {
                        (
                            id.clone(),
                            outcome.clone(),
                            metrics.clone(),
                            outputs.clone(),
                        )
                    })
                    .collect()
            });
        if pending.is_empty() {
            delay = state.policy.write_retry_first;
            continue;
        }
        let mut wrote_something = false;
        for (job_id, outcome, metrics, outputs) in pending {
            let end = JobState::Finished {
                outcome: outcome.clone(),
            };
            // The same three writes in the same order as the first try, kept
            // deliberately identical: the files are part of the record this
            // transition promises, so a hub that recovered its disk and wrote
            // a verdict without them would have lost half a record D19 says
            // is never lost.
            match state
                .store
                .record_metrics(&job_id, metrics.clone())
                .and_then(|()| state.store.record_outputs(&job_id, &outputs))
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
                    // Held back if its collection is still crossing, exactly
                    // as on the first try: a verdict written late is still a
                    // verdict a requester may fetch on (D167).
                    tell_or_hold_the_verdict(&state, &job_id, &outcome, &metrics);
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
            // Before the clock is believed: what the machine itself says
            // (D169). A runner that is here and reporting has already
            // answered "is this attempt still yours", and its answer beats a
            // timer that was only ever a stand-in for it.
            let reason = match state.what_the_runner_says(&runner, &job_id, attempt, unheard_for) {
                // Here, and holding it. The attempt is not lost, no go is
                // spent, and nothing is requeued: the machine is busy, and a
                // job taken away from a busy machine is a job that has to be
                // run again somewhere. What bounds it is the cap the job
                // runs under on that machine — the hub's reading cap for a
                // reading, the declared or the owner's time cap for anything
                // else (invariant 2) — and, if the machine stops answering
                // altogether, the link check above.
                WhatTheRunnerSays::StillRunningIt { reported } => {
                    say_the_attempt_is_slow(
                        &state,
                        &record,
                        &runner,
                        attempt,
                        unheard_for,
                        reported,
                    );
                    continue;
                }
                // Here, and not yet asked: the link is new, or nothing has
                // come back on it yet. Nothing is decided on a machine that
                // has not spoken, and the wait is bounded by the link check
                // above — a connection nothing comes back on is dropped at
                // the threshold, and then there is no runner and the clock
                // decides.
                WhatTheRunnerSays::NotYetSaying => continue,
                // Here, and not holding it. Said in those words rather than
                // "stopped answering", which would be untrue of a machine
                // that is on the line: this is a runner that was restarted,
                // or that dropped the attempt without the hub hearing why.
                WhatTheRunnerSays::NotAnyMore { reported } => format!(
                    "runner {runner} is connected and the health report it sent {reported:?} \
                     ago does not name attempt {attempt} of this job, and nothing has been \
                     heard about that attempt for {unheard_for:?}: the attempt is gone from a \
                     machine that is still here"
                ),
                // Nothing to go on but the clock, which is what it is for.
                WhatTheRunnerSays::NothingAtAll => format!(
                    "runner {runner} stopped answering: nothing about attempt {attempt} of \
                     this job for {unheard_for:?}, so it is not coming back"
                ),
            };
            if let Err(error) = give_the_job_another_go(&state, &job_id, attempt, &reason) {
                eprintln!("herd-hub: {job_id} could not be requeued: {error:#}");
            }
        }
    }
}

/// Say, once, that an attempt is slow on a machine that is still here
/// (D169).
///
/// The half of the fix that is not arithmetic. A job the hub has stopped
/// hearing about is a job somebody is waiting on, and "nothing is written in
/// your transcript for four minutes" is the state the first consumer read as
/// a hang. So the silence is named — whose machine, how long, and what
/// bounds it — and the job is **not** taken away from the runner that is
/// running it.
///
/// Once per attempt: the watchdog wakes twenty times per threshold, and the
/// same sentence twenty times a minute is a sentence nobody reads.
fn say_the_attempt_is_slow(
    state: &State,
    record: &JobRecord,
    runner: &str,
    attempt: u32,
    unheard_for: Duration,
    reported: Duration,
) {
    if !state.first_word_of_slowness(&record.id) {
        return;
    }
    let what = if record.is_a_reading() {
        "this requirement reading"
    } else {
        "this job"
    };
    let bound = if record.is_a_reading() {
        format!(
            "its {:?} reading cap (--reading-cap)",
            state.policy.reading_cap
        )
    } else {
        "the time cap it runs under on that machine".to_owned()
    };
    let note = format!(
        "nothing has been heard about attempt {attempt} of {what} for {unheard_for:?}, and \
         runner {runner} is connected and said {reported:?} ago that it is still running it. \
         A busy machine is not a lost one: the attempt keeps its go, nothing is requeued, \
         and what ends it is {bound} or the runner going silent altogether. This wait \
         carries on."
    );
    println!("herd-hub: {}: {note}", record.id);
    for follower in state.followers_to_tell(&record.id) {
        let _ = follower.send(FromHub::JobNote {
            job_id: record.id.clone(),
            note: note.clone(),
        });
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
        for follower in state.followers_to_tell(job_id) {
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
    let kept = read_the_outputs(state, job_id);
    let commit = the_commit_it_ran(state, job_id);
    for follower in state.stop_following(job_id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            // Whatever the record holds, which for a job no runner ever
            // reported on is nothing at all — and nothing is what is sent,
            // never a zero (D23). The same for its files: a runner that went
            // away reported no verdict, so it reported no output either.
            metrics: record.metrics.clone(),
            outputs: kept.clone(),
            commit: commit.clone(),
        });
    }
    // A job that is over is a job that is holding nothing, and something may
    // have been waiting for exactly that — a slot, the room, or, since D169,
    // a reading of its own to be minted now that the one it was waiting for
    // is written off. The other end of this function already judges the
    // queue after a requeue; the end that gives up had been leaving it to
    // whatever happened to wake the hub next, which on a quiet fleet is the
    // restatement cadence. A reading read again a minute later is a minute
    // of a nightly nobody can account for.
    if let Err(error) = dispatch_queued_jobs(state) {
        eprintln!("herd-hub: judging the queue after {job_id} was lost failed: {error:#}");
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
        why: format!(
            "{job_id} exited {code} on {runner}, so what it was given is held by \
                      something outside Herd after all"
        ),
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
        for follower in state.followers_to_tell(job_id) {
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
    let kept = read_the_outputs(state, job_id);
    let commit = the_commit_it_ran(state, job_id);
    for follower in state.stop_following(job_id) {
        let _ = follower.send(FromHub::JobFinished {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            metrics: record.metrics.clone(),
            outputs: kept.clone(),
            commit: commit.clone(),
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
        .declared(asked.declaration, asked.timeout_s)
        // The reading its command declared, if it declared one: the labels
        // above are then the baseline that reading is placed against, and
        // this job is placed on nothing until the answer is on its record
        // (D74).
        .reading(asked.labels_from)
}

/// Why a declared command this hub was sent will not do, if it will not
/// (E-11).
///
/// The hub **trusts** the requester for the declaration: it holds no
/// repository and cannot check the file against the commit, and D106 states
/// that cost out loud. What it can check is that the declaration is a thing a
/// runner could execute at all, and it does, because each of these would
/// otherwise become a job that runs something nobody declared:
///
/// * a commit that is not forty lowercase hex — the hash `herdctl` resolved is
///   what the workspace fetches, and anything else is a ref that would be
///   resolved on a machine, which is what E-01 forbids;
/// * an empty hook — four named steps run in a fixed order, and a step with
///   no line is a step that silently does not happen;
/// * a `run` hook with no `{line}` in it — the declared command line would
///   never reach the shell, and the job would report the exit code of their
///   workspace tool as a verdict about their tests.
///
/// Refused at the submission, where a requester is standing to be told
/// (E-36's rule for every refusal here).
fn why_that_declaration_will_not_do(asked: &herd_proto::Asked) -> Option<String> {
    if let Some(reason) = why_that_reading_will_not_do(asked) {
        return Some(reason);
    }
    let declaration = asked.declaration.as_ref()?;
    let commit = &declaration.commit;
    if commit.len() != 40
        || !commit
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    {
        return Some(format!(
            "this submission says command {} runs commit {commit:?}, which is not forty \
             lowercase hex: a submission carries the hash its own checkout resolved, never a \
             ref for a machine to resolve",
            declaration.command
        ));
    }
    for step in herd_proto::Step::ALL {
        if declaration.executor.hook(step).trim().is_empty() {
            return Some(format!(
                "this submission's {} step is an empty line: the four steps run in a fixed \
                 order and a step with no line is a step that would silently not happen",
                step
            ));
        }
    }
    if !declaration
        .executor
        .hook(herd_proto::Step::Run)
        .contains(herd_proto::LINE_TOKEN)
    {
        return Some(format!(
            "this submission's run step does not carry {}, so command {}'s own line would \
             never be run and whatever the step exited with would be reported as its verdict",
            herd_proto::LINE_TOKEN,
            declaration.command
        ));
    }
    None
}

/// Why the `labels_from` reading a submission carries will not do, if it will
/// not (D74).
///
/// Three rules, and each of them is a job that would otherwise run something
/// nobody declared, or be placed on a figure nobody chose:
///
/// * a reading with no declaration around it — the reading runs the
///   consumer's four executor steps on a commit, and the cache is keyed on
///   (commit, command, args), none of which exists without one;
/// * an empty line, which is a reading that would answer nothing and refuse
///   every job of that command;
/// * a line carrying `{j}`. This one is the sharp one. The figure is a
///   property of the machine the hub chooses, derived as it chooses one
///   (E-22, D82), and a reading is placed **before** the job whose figure it
///   would be — so there is no N to put in it, and an answer obtained for the
///   reading's own machine would be cached under (commit, command, args) as
///   if it were a property of those three, which D82 says it is not. Refused
///   by name rather than run with a brace in it or cached as a half-truth.
///   D74 and D82 together leave this open; M18 settles it with the consumer.
fn why_that_reading_will_not_do(asked: &herd_proto::Asked) -> Option<String> {
    let labels_from = asked.labels_from.as_ref()?;
    let Some(declaration) = asked.declaration.as_ref() else {
        return Some(format!(
            "this submission carries a labels_from reading ({}) and no declared command: a \
             reading runs the four executor steps of a command on a commit, and there is \
             neither here",
            labels_from.line
        ));
    };
    if labels_from.line.trim().is_empty() {
        return Some(format!(
            "command {}'s labels_from is an empty line: a reading that runs nothing answers \
             nothing, and every job of that command would then be refused for want of \
             requirements",
            declaration.command
        ));
    }
    if labels_from.line.contains(herd_proto::PARALLELISM_TOKEN) {
        return Some(format!(
            "command {}'s labels_from line carries {}: that figure is a property of the \
             machine the hub chooses and is derived as it chooses one (E-22), and the reading \
             is placed before the job whose figure it would be — so there is none to put in \
             it, and an answer that depended on one could not be cached by (commit, command, \
             args) as D74 requires. Declare a reading that does not ask for it",
            declaration.command,
            herd_proto::PARALLELISM_TOKEN
        ));
    }
    None
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
    let Some(labels_from) = asked.labels_from.clone() else {
        return judge_one(state, new_job(asked));
    };
    // A command whose requirements are read cannot be judged as itself: what
    // it needs is not known here, and judging it against the baseline its
    // reading runs on would answer about a job nobody submitted (D74). So the
    // answer is about what would really happen next — the cache, or the
    // reading.
    let key = asked.declaration.as_ref().map(|declaration| {
        crate::job::reading_key(&declaration.commit, &declaration.command, &declaration.args)
    });
    let cached = match &key {
        Some(key) => state
            .store
            .reading_answer(key)
            .context("asking the cache what a dry run's job would need")?,
        None => None,
    };
    if let Some(answer) = cached {
        let mut lines = vec![format!(
            "its requirements are already read and kept by (commit, command, args), so no \
             reading would run: {} (D74)",
            answer.line()
        )];
        let mut new = new_job(asked);
        new.reading = new
            .reading
            .map(|reading| reading.answered(answer, "the cache"));
        lines.extend(judge_one(state, new)?);
        return Ok(lines);
    }
    let mut lines = vec![format!(
        "what it needs is not known yet: an ordinary job on a runner reads it first ({}), \
         against the baseline labels {} its command declares, and this job is then placed on \
         what that answered (D74). What follows is where that reading would run",
        labels_from.line,
        if labels_from.baseline.is_empty() {
            "none".to_owned()
        } else {
            labels_from.baseline.to_string()
        }
    )];
    let hypothetical_record = JobRecord::hypothetical(new_job(asked.clone()), Timestamp::now());
    let reading = reading_job(
        state,
        &hypothetical_record,
        &crate::job::Reading::of(labels_from),
        // Judged with the declaration the submission carries, so the reading
        // this answers about is the reading a submission would really mint.
        &asked
            .declaration
            .unwrap_or_else(|| herd_proto::Declaration {
                command: String::new(),
                commit: String::new(),
                args: Vec::new(),
                executor: herd_proto::Executor::default(),
            }),
    );
    lines.extend(judge_one(state, reading)?);
    Ok(lines)
}

/// Where one job — the one a submission would make, or the reading it would
/// mint first — would run, said in lines (E-07).
fn judge_one(state: &State, new: NewJob) -> Result<Vec<String>> {
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
    let hypothetical = JobRecord::hypothetical(new, now);
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
            readings_at_once: Some(state.policy.readings_at_once),
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

/// Move one runner's switch, and say what happened (E-33, D99).
///
/// The hub asks; the runner is the authority. It writes the position to its own
/// file so a restart keeps it, applies it, and acknowledges both — which is
/// what makes the two ways of setting a switch leave one state behind.
///
/// **This waits for that acknowledgement, and that is D99.** Until D99 it
/// returned the moment the ask had been *sent*, and `herdctl switch` printed
/// success on the strength of it. There was then a window — measured at a
/// second and a half on a two-core host — in which the owner had been told her
/// machine was off, the hub honoured it, and the machine's own disk said
/// nothing: a `kill -9` inside it brought the runner back on its previous
/// position. For a command whose whole purpose is that an owner can take her
/// machine back, that was the wrong answer.
///
/// The wait is bounded by [`herd_proto::SWITCH_HONOURED_WITHIN`], E-33's own
/// ten seconds, and what comes back when it expires is **not** success: it is
/// the position the hub holds, the fact that the runner has confirmed nothing,
/// and the runner's name. Answers with the sentence and whether it is success.
async fn set_a_switch(state: &State, runner: &str, switch: herd_proto::Switch) -> (String, bool) {
    let why = format!("set through this hub to {}", switch.sentence());
    let asked = state.with_peers(|peers| {
        peers.runners.get(runner).map(|connected| {
            // Cleared before the ask, so that an acknowledgement of this same
            // position from an earlier `herdctl switch` cannot answer this one.
            connected.applied.send_replace(None);
            let acknowledgements = connected.applied.subscribe();
            let _ = connected.outbox.send(FromHub::SetSwitch {
                switch: switch.clone(),
                why: why.clone(),
            });
            (connected.switch.clone(), acknowledgements)
        })
    });
    let Some((was, mut acknowledgements)) = asked else {
        return (
            format!(
                "no runner called {runner} is connected to this hub, so its switch cannot be \
                 moved from here, and nothing about it has changed. An owner sets it on the \
                 machine itself, with `herd-runner switch`, which works whether this hub is up \
                 or not"
            ),
            false,
        );
    };
    let heard = tokio::time::timeout(herd_proto::SWITCH_HONOURED_WITHIN, async {
        loop {
            let seen = acknowledgements.borrow_and_update().clone();
            match seen {
                // Only an acknowledgement of *this* position answers: a runner
                // whose owner moved her own switch file in the same second
                // acknowledges nothing, but its health report does carry a
                // position, and that position is not an answer to this ask.
                Some(ack) if ack.switch == switch => return Some(ack),
                _ => {}
            }
            // The sender is dropped when the runner's connection is released,
            // so a runner that went away ends this wait instead of spending
            // ten seconds on a link that no longer exists.
            if acknowledgements.changed().await.is_err() {
                return None;
            }
        }
    })
    .await;
    let position = switch.sentence();
    match heard {
        Ok(Some(Acknowledged {
            persistence: herd_proto::SwitchPersistence::Written { path },
            ..
        })) => (
            format!(
                "runner {runner} is switched {position} (it was {was}) and has written the \
                 position down in {path}, so {runner} comes back on it even if it is killed \
                 now (E-33)"
            ),
            true,
        ),
        Ok(Some(Acknowledged {
            persistence: herd_proto::SwitchPersistence::NotWritten { why },
            ..
        })) => (
            format!(
                "runner {runner} is switched {position} (it was {was}) and this hub honours \
                 that now. What is NOT true: {runner} could not write the position down, so a \
                 {runner} restarted would come back {}. Why it could not: {why}",
                herd_proto::Switch::default().sentence()
            ),
            false,
        ),
        // The link went while this was waiting. Said as what it is, because
        // the position may well have been written before the runner went — and
        // "may well have been" is not an answer anyone can act on.
        Ok(None) => (
            format!(
                "runner {runner} was asked to switch to {position} (it was {was}) and its link \
                 to this hub ended before it answered, so whether {runner} wrote the position \
                 down is not known here. It is announced in the handshake, so `herdctl switch` \
                 says where {runner} stands once it dials again"
            ),
            false,
        ),
        Err(_) => {
            // Exactly what is and is not true, and the hub's half is read
            // rather than assumed: the runner is the authority on the
            // position (D98), so what this hub honours is what it has been
            // told, and on this path it may have been told nothing at all.
            let seen = state.with_peers(|peers| {
                peers
                    .runners
                    .get(runner)
                    .map(|connected| connected.switch.sentence())
            });
            let honouring = match seen.as_deref() {
                Some(now) if now == position => format!(
                    "this hub honours {position} on {runner} now — nothing new is claimed there"
                ),
                Some(now) => format!(
                    "this hub still sees {runner} as {now}, so it has not been told the \
                     position took effect either"
                ),
                None => format!("{runner} is no longer connected to this hub"),
            };
            (
                format!(
                    "runner {runner} was asked to switch to {position} (it was {was}) and \
                     {honouring}. What is NOT true: {runner} has not confirmed writing the \
                     position down, within the {:?} E-33 gives it. So if {runner} is killed \
                     before it writes, it comes back {was}. Ask again, or set it on the \
                     machine itself with `herd-runner switch`, which needs no hub",
                    herd_proto::SWITCH_HONOURED_WITHIN
                ),
                false,
            )
        }
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
            ToHub::SubmitJob { asked, wants_files } => {
                if asked.command.is_empty() {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "a job needs a command to run".to_owned(),
                    });
                    return Ok(());
                }
                if let Some(reason) = why_that_declaration_will_not_do(&asked) {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                // The one fleet state a submission is refused in: a roster
                // with nothing on it at all (D140). A fleet that is merely
                // away queues the job, and the pass below explains it.
                if let Some(reason) = state.why_no_runner_is_approved() {
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
                // The claim lock is taken **before** the job exists, and held
                // until this submission has had its place in the followers
                // and its pass. Not for the pass's sake — that took the lock
                // on its own before — but for the window between the record
                // becoming visible and this connection being registered to
                // follow it. A job is dispatchable the instant `submit`
                // returns, and a pass fired by anything else (a runner
                // joining, a job of another test ending, the watchdog) would
                // dispatch it in that window, send E-02's line to a follower
                // list this requester is not in yet, and leave it with a job
                // whose output has no first line (D109). The window is
                // microseconds wide and a suite submits thousands of jobs,
                // which is exactly the shape of a defect that reads as
                // flakiness. One record write is added to a lock that already
                // carries a whole pass's fsyncs.
                let _one_pass = state
                    .dispatching
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                    Submission::Minted(_) => state.follow(job_id.clone(), outbox, wants_files),
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
                        answer_a_wait(state, outbox, job_id.clone(), wants_files)?;
                    }
                }
                // And then the job goes through exactly the same gate every
                // other queued job goes through — the one queue, in its own
                // order (E-20). It starts, or it stays queued with its
                // reason on its record and a note on this connection: a
                // submission the runner cannot serve yet is not a submission
                // that failed (D38). The lock this needs is the one taken
                // above, around the submission itself, so the pass runs here
                // without taking it again — and nothing has been able to
                // dispatch this job between the two.
                dispatch_a_pass(state)?;
                drop(_one_pass);
                // And there is nothing after the pass. A runner that went
                // away between the roster check and the pass leaves the job
                // **queued**, with the pass's own sentence on its record and
                // on this connection — not refused (D140). This is where the
                // second answer to one fleet state used to live: a submission
                // that landed a moment after the last link dropped was
                // refused while the job in front of it, queued a moment
                // earlier, waited. The requester keeps its place in the
                // followers, E-04's restatement says where the job stands at
                // least every 60 s, E-09's deadline bounds it, and a cancel
                // reaches it.
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
                // Judged by the same function a submission is, so that a dry
                // run of a declaration a submission would refuse says so
                // rather than answering about a job nobody can submit (E-07).
                if let Some(reason) = why_that_declaration_will_not_do(&asked) {
                    let _ = outbox.send(FromHub::WouldRun {
                        lines: vec![format!("it would be refused at submission: {reason}")],
                    });
                    return Ok(());
                }
                // The roster, judged exactly as a submission judges it
                // (D140), through the sentence E-07 already has for a
                // declaration a submission would refuse. Without this the
                // dry run would answer "it would wait" about the one fleet
                // state a submission refuses — two answers to one fleet
                // state, which is the defect D140 is about, in a new place.
                let lines = match state
                    .why_no_runner_is_approved()
                    .or(why_that_parent_will_not_do(state, asked.after.as_ref())?)
                {
                    Some(reason) => vec![format!("it would be refused at submission: {reason}")],
                    None => dry_run(state, asked)?,
                };
                let _ = outbox.send(FromHub::WouldRun { lines });
            }
            ToHub::SetSwitch { runner, switch } => {
                let (said, confirmed) = set_a_switch(state, &runner, switch).await;
                println!("herd-hub: {said}");
                let _ = outbox.send(FromHub::SwitchSet { said, confirmed });
            }
            ToHub::AskFleetSwitches {} => {
                let _ = outbox.send(FromHub::FleetSwitches {
                    runners: state.fleet_switches(),
                });
            }
            ToHub::ApproveRunner { runner } => approve_a_runner(state, outbox, &runner),
            ToHub::RemoveRunner { runner } => remove_a_runner(state, outbox, &runner),
            ToHub::AskApprovedRunners {} => {
                let approvals = match state.store.approved_runners() {
                    Ok(approvals) => approvals,
                    Err(error) => {
                        let _ = outbox.send(FromHub::Refused {
                            reason: format!(
                                "this hub cannot read which runners it approved: {error}"
                            ),
                        });
                        return Ok(());
                    }
                };
                // Keyed by name, carrying what the listing says about each
                // connected machine: its build and what it is not handing out
                // (D166). A name that is not in here is a runner that is not
                // connected, and that is still the only question `connected`
                // answers.
                let connected = state.with_peers(|peers| {
                    peers
                        .runners
                        .iter()
                        .map(|(name, runner)| (name.clone(), runner.facts()))
                        .collect::<BTreeMap<String, herd_proto::RunnerFacts>>()
                });
                let _ = outbox.send(FromHub::ApprovedRunners {
                    runners: approvals
                        .into_iter()
                        .map(|approval| herd_proto::ApprovedRunner {
                            connected: connected.contains_key(&approval.name),
                            // Empty for a machine that is not here: it offers
                            // nothing, so it suspends nothing, and a stale
                            // list would be a report about a runner that left.
                            facts: connected.get(&approval.name).cloned().unwrap_or_default(),
                            name: approval.name,
                            approved_unix_s: approval.approved_unix_s,
                        })
                        .collect(),
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
            ToHub::WaitForJob {
                job_id,
                wants_files,
            } => {
                if let Some(reason) = quarantine_refusal(state, &job_id)? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                if !answer_a_wait(state, outbox, job_id, wants_files)? {
                    return Ok(());
                }
            }
            // A requester asking for the files of a job whose manifest it
            // already has (D148).
            //
            // **Only the streamed carriage ever gets here**, because only it
            // leaves a requester with something missing: under the inline
            // threshold the bytes were on the verdict, so nothing is absent
            // and nothing is asked for — D137's single round trip, untouched.
            //
            // Answered off the disk, so one piece of code serves a job that
            // ended an hour ago and one ending now: there is one answer to
            // "what did this job leave" and it is the record.
            ToHub::AskJobFiles { job_id, paths } => {
                if let Some(reason) = quarantine_refusal(state, &job_id)? {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                send_the_files(state, &job_id, &paths, outbox);
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
/// `wants_files` is this requester's own answer about the job's output files
/// (D167): one that is going to fetch them has its verdict held until they
/// have all arrived, and one that is not is answered at once.
///
/// `false` means the job does not exist and the connection has been refused.
fn answer_a_wait(state: &State, outbox: &Outbox, job_id: JobId, wants_files: bool) -> Result<bool> {
    let attached = state
        .attach(&job_id, outbox, wants_files)
        .with_context(|| format!("reading the record of {job_id}"))?;
    let Some(Attached {
        record,
        log,
        stalled,
        following,
    }) = attached
    else {
        // A terminal no, as D13 means it: this id will not come
        // into being by waiting for it.
        let _ = outbox.send(FromHub::Refused {
            reason: format!("no such job: {job_id}"),
        });
        return Ok(false);
    };
    // Where it ran, before its output: a `herdctl wait` on a job that is
    // already running, or long over, prints the same first line a requester
    // that followed it from the start printed (E-02, D109). Off the record's
    // own attempt, so the sentence is the one that was recorded (E-36), and
    // the latest attempt because that is where the job is now.
    if let Some(attempt) = record.attempts.last() {
        let _ = outbox.send(FromHub::RanOn {
            job_id: job_id.clone(),
            runner: attempt.runner.clone(),
            why: attempt.why.clone(),
        });
    }
    // The output the job has already produced, before anything
    // else about it. A detached requester has no other way to get
    // a job's log through the CLI (D22).
    //
    // **In frames of a bounded size, and that is not a refinement.** This
    // sent the whole log in one frame, and a job's log cap is 64 MiB while
    // the frame ceiling is 64 MiB too — so past about 48 MiB of output,
    // base64 built a frame that could not go through the socket, and a
    // `herdctl wait` on a job that had passed got a closed link and exit 125
    // (D148). Several frames concatenate to exactly what one carried, so the
    // frame itself did not have to change.
    for piece in log.chunks(herd_proto::JOB_LOG_FRAME_BYTES) {
        let _ = outbox.send(FromHub::JobLog {
            job_id: job_id.clone(),
            data: piece.to_vec(),
        });
    }
    let metrics = record.metrics.clone();
    // The live sentence first, the record second (D171): a wait that attaches
    // to a job queued behind two hundred others is told where it is now, not
    // where it was when its cause was last written down.
    let waiting_for = state
        .why_it_waits(&record.id)
        .or(record.waiting_for.clone());
    match record.state {
        // Answered from the record, which is the point of writing
        // one: a wait works an hour later, on another machine,
        // across a restart of the hub. The measurements come from
        // the record too, so a wait an hour later reads the same
        // figures the requester that followed the job read (E-15).
        JobState::Finished { outcome } => {
            // **A verdict whose collection is still crossing waits here
            // too** (D167). The record is terminal and says the job passed,
            // and a `herdctl wait --outputs-to` answered from it while the
            // bytes were still arriving is the very case the first consumer
            // measured: a fetch on that answer gets a short directory. So
            // this wait is registered, told why it is waiting, and answered
            // when the collection is whole — or when waiting has stopped
            // being worth anything.
            //
            // Derived off the disk rather than from the entry in memory,
            // because this path is the one that has to work after a hub
            // restart and for a requester that was never connected while the
            // job ran.
            //
            // **And only for a requester that said it wants the files.** One
            // that did not is asking a different question — "how did it
            // end?" — and the answer to that has been on the disk since the
            // verdict was recorded.
            let stands = if wants_files {
                where_the_collection_stands(state, &job_id, None)
            } else {
                TheCollection::Whole
            };
            match stands {
                TheCollection::Crossing(got) => {
                    // Exactly once: `State::attach` registers this
                    // connection when a verdict is already held, and this is
                    // the first hold when it is not.
                    state.hold_the_verdict(&job_id, &outcome, &metrics);
                    if !following {
                        state.follow(job_id.clone(), outbox, wants_files);
                    }
                    say_the_collection_is_crossing(state, &job_id, &got);
                }
                stands => {
                    if let TheCollection::StoppedShort(why) = &stands {
                        say_the_collection_is_short(state, &job_id, why);
                    }
                    // And the files it left, off the same record. This is
                    // what makes the whole mechanism work for a requester
                    // that was never connected while the job ran: `herdctl
                    // wait <id>` an hour later gets the verdict and the bytes
                    // in one frame, so there is no fetch command to add and
                    // no second round trip (D131).
                    let kept = read_the_outputs(state, &job_id);
                    let commit = the_commit_it_ran(state, &job_id);
                    let verdict = FromHub::JobFinished {
                        job_id: job_id.clone(),
                        outcome,
                        metrics,
                        outputs: kept,
                        commit,
                    };
                    // Whoever `attach` left following — this connection, if a
                    // verdict was held a moment ago and has since been let go
                    // — and then this connection, which is the ordinary case
                    // and leaves no follower behind on a job that is over.
                    for follower in state.stop_following(&job_id) {
                        if !follower.same_channel(outbox) {
                            let _ = follower.send(verdict.clone());
                        }
                    }
                    let _ = outbox.send(verdict);
                }
            }
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

/// Approve one runner, and hand the token back once (E-48, Q8).
///
/// **One operator action**, which is the whole of what Q8 asks for: the name
/// goes in, the token comes out, and the machine is in the fleet as soon as
/// it dials with it. Nothing is generated on the runner, nothing is
/// exchanged, no certificate and no second round trip — those are the things
/// invariant 1 keeps Herd away from.
///
/// The hub's own stdout says that the approval happened and never what was
/// minted: the token travels on this one connection to the operator who asked
/// for it, and a hub that printed it would put a credential in the container's
/// log for as long as the log is kept.
fn approve_a_runner(state: &State, outbox: &Outbox, runner: &str) {
    match state.store.approve_runner(runner) {
        Ok(token) => {
            println!("herd-hub: runner {runner} is approved and a new token was minted for it");
            let _ = outbox.send(FromHub::RunnerApproved {
                runner: runner.to_owned(),
                token,
                said: format!(
                    "runner {runner} is approved on this hub. Its token is printed once and \
                     this hub keeps only a hash of it: give it to that machine as --token or \
                     HERD_TOKEN. Approving {runner} again mints a new token and revokes this \
                     one"
                ),
            });
        }
        // Terminal (D13): a name that is not a name, or a state root that
        // cannot be written, are both answers that asking again repeats.
        Err(error) => {
            let _ = outbox.send(FromHub::Refused {
                reason: format!("runner {runner} was not approved: {error}"),
            });
        }
    }
}

/// Remove one runner, revoke its token, and tell the machine on the
/// connection it is holding (E-48).
///
/// The telling is the half that is easy to leave out. A revocation that only
/// took effect at the next dial would leave a runner Herd no longer trusts
/// holding a link, claiming jobs, for as long as its process lives — which on
/// a machine somebody has just taken out of the fleet could be weeks. So the
/// refusal goes out on its own connection and the connection is dropped,
/// which is exactly what the hub already does to a runner it supersedes.
///
/// A job that runner is executing is **not** cancelled here. It is running on
/// a machine that was trusted when it started, its verdict is spooled and
/// will be refused like any other verdict from a runner that is not admitted,
/// and the hub's own loss detection ends the attempt. Killing work on the way
/// out would be a removal that destroys a build nobody asked to lose.
fn remove_a_runner(state: &State, outbox: &Outbox, runner: &str) {
    let removed = match state.store.remove_runner(runner) {
        Ok(removed) => removed,
        Err(error) => {
            let _ = outbox.send(FromHub::Refused {
                reason: format!("runner {runner} was not removed: {error}"),
            });
            return;
        }
    };
    if !removed {
        let _ = outbox.send(FromHub::RunnerRemoved {
            said: format!(
                "no runner called {runner} is approved on this hub, so there was no token to \
                 revoke and nothing was changed"
            ),
        });
        return;
    }
    let reason = herd_proto::revoked_runner_reason(runner);
    let told = state.with_peers(|peers| {
        peers.runners.get(runner).is_some_and(|connected| {
            let _ = connected.outbox.send(FromHub::Refused {
                reason: reason.clone(),
            });
            let _ = connected.kick.send(true);
            true
        })
    });
    println!("herd-hub: {reason}");
    let _ = outbox.send(FromHub::RunnerRemoved {
        said: if told {
            format!(
                "runner {runner} is removed and its token is revoked; it was connected and has \
                 been told, and claims nothing from here on"
            )
        } else {
            format!(
                "runner {runner} is removed and its token is revoked; it was not connected, \
                 and it is refused by name if it dials again"
            )
        },
    });
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
                    // anything, so there is nothing measured to send — and
                    // nothing left behind either: it never had a workspace,
                    // so no `fetch` of it ever reported a revision.
                    metrics: JobMetrics::default(),
                    outputs: herd_proto::JobOutputs::default(),
                    commit: None,
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
    let message: ToHub =
        from_frame(&text).map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
    // **A frame type this hub does not know is refused by name, not ignored**
    // (D148). Said here because this is where the text is: `Unknown` is a
    // unit variant, so the tag is discarded on the way to it, and the one
    // thing the peer needs told is which of its frames this hub cannot read.
    //
    // The opposite of the runner's answer to the same situation, deliberately.
    // A runner ignoring a frame it does not know loses nothing, because a
    // frame it has never heard of is work it is not needed for. A *request*
    // the hub cannot read is work that will never be done, and a peer told
    // nothing would wait for it.
    if message == ToHub::Unknown {
        eprintln!(
            "herd-hub: a peer sent a frame of type {:?}, which this hub does not know (it \
             speaks protocol {PROTOCOL_VERSION})",
            herd_proto::frame_type(&text).unwrap_or_else(|| "(none)".to_owned())
        );
    }
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

/// Why a websocket handshake did not finish, in words an operator can act on.
///
/// The common cause is not a broken peer: it is something that connected to
/// the port and went away without saying who it was — a deployment's TCP
/// health check, a reverse proxy's probe, or a runner dialling a hub that is
/// still coming up, which retries on its own backoff and gets in (that is what
/// its 500 ms first delay is for). **There is nothing here for this hub to
/// refuse**, which is the question this wording answers: the peer never said
/// anything, so there is no name to turn away and nothing to record against
/// it. Said plainly, so that a line on every health check is not read as a
/// fault — and so that the case which *is* one, a peer that spoke and spoke
/// wrongly, still arrives in the library's own words.
fn unfinished_handshake_reason(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::error::ProtocolError;
    use tokio_tungstenite::tungstenite::Error;
    let went_away = match error {
        Error::Protocol(ProtocolError::HandshakeIncomplete) => true,
        Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
        ),
        _ => false,
    };
    if went_away {
        format!(
            "a peer connected and went away before finishing its websocket handshake, so \
             this hub never learnt who it was and has nothing to answer ({error}). A TCP \
             health check looks exactly like this, and so does a runner dialling while this \
             hub is still coming up; it is worth reading only if it repeats for a peer that \
             should be getting in"
        )
    } else {
        format!("this peer's websocket handshake would not complete: {error}")
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
        ToHub::JobFileChunk { .. } => "job_file_chunk",
        ToHub::AskJobFiles { .. } => "ask_job_files",
        ToHub::Unknown => "a frame of a type this hub does not know",
        ToHub::Hello { .. } => "hello",
        ToHub::CapabilitiesChanged { .. } => "capabilities_changed",
        ToHub::SubmitJob { .. } => "submit_job",
        ToHub::RaiseJob { .. } => "raise_job",
        ToHub::CancelJob { .. } => "cancel_job",
        ToHub::WaitForJob { .. } => "wait_for_job",
        ToHub::JobStarted { .. } => "job_started",
        ToHub::LogChunk { .. } => "log_chunk",
        ToHub::JobFinished { .. } => "job_finished",
        ToHub::StepFinished { .. } => "step_finished",
        ToHub::Health { .. } => "health",
        ToHub::AttemptAbandoned { .. } => "attempt_abandoned",
        ToHub::DryRun { .. } => "dry_run",
        ToHub::SwitchApplied { .. } => "switch_applied",
        ToHub::SetSwitch { .. } => "set_switch",
        ToHub::AskFleetSwitches {} => "ask_fleet_switches",
        ToHub::ApproveRunner { .. } => "approve_runner",
        ToHub::RemoveRunner { .. } => "remove_runner",
        ToHub::AskApprovedRunners {} => "ask_approved_runners",
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
        /// Raised when the claim has written everything it writes — **inside
        /// the `dispatching` lock**, which is what makes this test's answer a
        /// fact rather than a race (D100).
        claim_is_over: Arc<AtomicBool>,
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
            let attempt = self.inner.begin_attempt(id, runner, why, parallelism);
            // Here, and not when the pass thread comes back. The pass still
            // holds `dispatching` at this point, so a snapshot — which takes
            // that lock — cannot be applied before this is raised. Raised by
            // the pass thread *after* `dispatch_queued_jobs` returned, it was
            // raised after the lock had already been released, and on a
            // two-core host the snapshot thread routinely got in between: the
            // test then reported the product dispatching onto a busy
            // capability when the product had done nothing of the kind. It
            // failed in seven of nine workspace suite runs on `taskset -c 0,1`
            // against unmodified `main`, and in none of five runs of this
            // target on its own — it needs the contention to show (D100).
            self.claim_is_over.store(true, Ordering::SeqCst);
            attempt
        }

        fn state_root_id(&self) -> &str {
            self.inner.state_root_id()
        }
        fn approve_runner(&self, name: &str) -> StoreResult<String> {
            self.inner.approve_runner(name)
        }
        fn remove_runner(&self, name: &str) -> StoreResult<bool> {
            self.inner.remove_runner(name)
        }
        fn approved_runners(&self) -> StoreResult<Vec<crate::roster::Approval>> {
            self.inner.approved_runners()
        }
        fn admit_runner(&self, name: &str, token: Option<&str>) -> crate::roster::Admission {
            self.inner.admit_runner(name, token)
        }
        fn approved_runner_count(&self) -> usize {
            self.inner.approved_runner_count()
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
        fn record_reading(&self, id: &JobId, reading: crate::job::Reading) -> StoreResult<()> {
            self.inner.record_reading(id, reading)
        }
        fn reading_answer(&self, key: &str) -> StoreResult<Option<herd_proto::Requirements>> {
            self.inner.reading_answer(key)
        }
        fn remember_reading(
            &self,
            key: &str,
            answer: &herd_proto::Requirements,
        ) -> StoreResult<()> {
            self.inner.remember_reading(key, answer)
        }
        fn record_state(&self, id: &JobId, state: JobState) -> StoreResult<()> {
            self.inner.record_state(id, state)
        }
        fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> StoreResult<()> {
            self.inner.record_metrics(id, metrics)
        }
        fn record_outputs(&self, id: &JobId, outputs: &herd_proto::JobOutputs) -> StoreResult<()> {
            self.inner.record_outputs(id, outputs)
        }
        fn job_outputs(&self, id: &JobId, with_bytes: bool) -> StoreResult<herd_proto::JobOutputs> {
            self.inner.job_outputs(id, with_bytes)
        }
        fn write_output_chunk(
            &self,
            id: &JobId,
            path: &str,
            offset: u64,
            data: &[u8],
        ) -> StoreResult<crate::job::ChunkLanded> {
            self.inner.write_output_chunk(id, path, offset, data)
        }
        fn output_holdings(&self, id: &JobId) -> StoreResult<Vec<herd_proto::FileHeld>> {
            self.inner.output_holdings(id)
        }
        fn collections_owed(&self) -> StoreResult<Vec<crate::job::CollectionOwed>> {
            self.inner.collections_owed()
        }
        fn collection_owed(&self, id: &JobId) -> StoreResult<Option<crate::job::CollectionOwed>> {
            self.inner.collection_owed(id)
        }
        fn output_budget(&self) -> crate::job::Budget {
            self.inner.output_budget()
        }
        fn note_outputs_fetched(&self, id: &JobId) -> StoreResult<()> {
            self.inner.note_outputs_fetched(id)
        }
        fn sweep_outputs(&self) -> StoreResult<crate::job::Sweep> {
            self.inner.sweep_outputs()
        }
        fn read_output_chunk(
            &self,
            id: &JobId,
            path: &str,
            offset: u64,
            take: usize,
        ) -> StoreResult<Option<Vec<u8>>> {
            self.inner.read_output_chunk(id, path, offset, take)
        }
        fn clear_output_parts(&self, id: &JobId) -> StoreResult<()> {
            self.inner.clear_output_parts(id)
        }
        fn record_step(
            &self,
            id: &JobId,
            attempt: u32,
            step: herd_proto::StepRun,
        ) -> StoreResult<()> {
            self.inner.record_step(id, attempt, step)
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
    ///
    /// "The claim had finished" is raised by the instrumented store **inside**
    /// the lock the pass holds, and that detail is the test (D100). Raised by
    /// the pass thread once `dispatch_queued_jobs` had returned, it was raised
    /// after the lock was released, and a snapshot let in during that gap read
    /// it as `false` — a green product reported red in seven of nine workspace
    /// suite runs on two pinned cores.
    #[test]
    fn a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim() {
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let in_the_window = Arc::new(AtomicBool::new(false));
        let claim_is_over = Arc::new(AtomicBool::new(false));
        let (go_on, wait_for_go_on) = std_mpsc::channel();
        let store = Arc::new(StoreThatHoldsTheClaimOpen {
            inner: JobDirStore::open(dir.path()).expect("the state root opens"),
            in_the_window: Arc::clone(&in_the_window),
            claim_is_over: Arc::clone(&claim_is_over),
            go_on: Mutex::new(Some(wait_for_go_on)),
        });
        let labels = LabelSet::new(["kvm"]).expect("valid labels");
        store
            .create_job(NewJob::needing(vec!["true".to_owned()], labels.clone()))
            .expect("minted");

        let state = Arc::new(State {
            store: Arc::clone(&store) as Arc<dyn JobStore>,
            policy: Policy::default(),
            authority: Authority::default(),
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
                    version: hub_version().to_owned(),
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

        // The pass. It stops inside the claim and waits to be let go. The
        // store raises `claim_is_over` itself, under the lock, rather than
        // this thread raising it on the way out.
        let pass = std::thread::spawn({
            let state = Arc::clone(&state);
            move || dispatch_queued_jobs(&state).expect("the pass runs")
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
            authority: Authority::default(),
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
        // The second writes nothing, because no job's cause is news. That is
        // what a steady-state pass costs — and since D171 it is also what a
        // pass over a queue that has *moved* costs, which is the figure that
        // matters: the one this pair measures held on v0.3.6 only because
        // nothing between the two passes changed.
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

    /// How many `waiting` lines this job's `events.jsonl` holds.
    ///
    /// Read off the disk rather than counted through an instrumented store,
    /// because the disk is what the first consumer measured: one of their
    /// queued jobs had **94** of these lines, every one of them the same
    /// explanation with a different ordinal in it (D171).
    fn waiting_lines(store: &JobDirStore, id: &JobId) -> usize {
        let path = store.job_dir(id).join("events.jsonl");
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains(r#""event":"waiting""#))
            .count()
    }

    /// **The incident, as a count.** A queue that *moves* must not cost a
    /// durable write on every job that is standing still in it (D171).
    ///
    /// The sentence a pass writes on a queued record carries the queue's
    /// figures — `queued 76th of 120`, and the jobs ahead by name — so one
    /// job arriving changes the sentence of every job already waiting while
    /// changing the reason for none of them. `record_waiting` compared
    /// sentences, found news every time, and rewrote `job.json` and appended
    /// to `events.jsonl` for all of them, each fsynced.
    ///
    /// Twelve jobs arrive here, one per pass. The watched job's cause never
    /// changes, so its history must hold the **one** line its first
    /// judgement wrote. Before D171 it held thirteen, and on the first
    /// consumer's 184-job queue that arithmetic is what filled a tokio
    /// worker with uninterruptible disk wait for forty minutes.
    #[test]
    fn a_queue_that_moves_writes_nothing_about_the_jobs_standing_still_in_it() {
        const QUEUED: usize = 120;
        const ARRIVALS: usize = 12;
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        for which in 0..QUEUED {
            store
                .create_job(NewJob::new(vec!["true".to_owned(), format!("{which}")]))
                .expect("minted");
        }
        let state = Arc::new(State {
            store: Arc::clone(&store) as Arc<dyn JobStore>,
            policy: Policy::default(),
            authority: Authority::default(),
            started: Instant::now(),
            peers: Mutex::new(Peers::default()),
            dispatching: Mutex::new(()),
        });

        // The first judgement of a job is news and is written: a job nobody
        // is following still says on its disk why nothing is happening to it
        // (E-53). That line is the one this test allows.
        dispatch_queued_jobs(&state).expect("the pass runs");
        let watched = JobId::new("j0000000076".to_owned());
        assert_eq!(
            waiting_lines(&store, &watched),
            1,
            "the first pass to judge a job writes why it waits"
        );

        // And now the queue moves, exactly as the first consumer's did: jobs
        // keep arriving behind it. Nothing about *why* the watched job waits
        // changes at all — there is still no runner — but its sentence does,
        // because the total in it does.
        for arrival in 0..ARRIVALS {
            store
                .create_job(NewJob::new(vec![
                    "true".to_owned(),
                    format!("arrival {arrival}"),
                ]))
                .expect("minted");
            dispatch_queued_jobs(&state).expect("the pass runs");
        }

        let lines = waiting_lines(&store, &watched);
        assert_eq!(
            lines, 1,
            "{watched} is waiting for the reason it has waited for all along, and this hub \
             wrote that down {lines} times over {ARRIVALS} passes: a queue that moves must \
             not cost a record rewrite and an fsynced history line for every job standing \
             still in it (D171)"
        );

        // E-04 is kept, and this is where: the figures a requester is told
        // are **this** pass's, not the ones that held when the cause was
        // last written down.
        let fresh = state.why_it_waits(&watched).expect("the pass judged it");
        assert!(
            fresh.contains(&format!("of {}", QUEUED + ARRIVALS)),
            "a requester asking why its job waits is told the queue as it is now: {fresh}"
        );
        // And the record carries the sentence that held when the cause was
        // news, which is what a hub that restarts reads back. Said here so
        // the trade is written down where it can be seen.
        let recorded = store
            .job(&watched)
            .expect("the record reads")
            .expect("the job is there")
            .waiting_for
            .expect("a reason");
        assert!(
            recorded.contains(&format!("of {QUEUED}")),
            "the record keeps the sentence its cause was written with: {recorded}"
        );
    }

    /// **The symptom, on a runtime.** A hub standing a deep queue must still
    /// have a worker to answer with (D171).
    ///
    /// The first consumer's hub stopped answering `herdctl runner list`
    /// entirely — three tries at 30 s and one at 180 s, all `rc 124` — while
    /// using 1.8 % of a CPU. Their thread dump said why: of thirteen
    /// threads, one was always in `D`, uninterruptible disk wait, and the
    /// rest were waiting behind it. A dispatch pass is a blocking function
    /// called straight from async callers, so a pass that spends seconds in
    /// fsync is a tokio worker that spends seconds unavailable.
    ///
    /// This stands two passes against a two-worker runtime — the hub has a
    /// dozen callers that each start one, and they serialize on
    /// `dispatching` — and asks the runtime, from outside it, to poll a task
    /// that does nothing at all. Under the defect no worker is free to do
    /// it for as long as the passes run.
    ///
    /// **The figures, measured on the host this was written on**: 17.3 s on
    /// v0.3.6, 0.23 s with D171. The bound is two seconds — a requester's
    /// patience, deliberately not a figure about disks — which leaves either
    /// side of it an order of magnitude of room on a slower box.
    #[test]
    fn a_deep_queue_leaves_the_runtime_a_worker_to_answer_with() {
        const QUEUED: usize = 300;
        const PASSES: usize = 4;
        const WORKERS: usize = 2;
        const PATIENCE: Duration = Duration::from_secs(2);

        let dir = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        for which in 0..QUEUED {
            store
                .create_job(NewJob::new(vec!["true".to_owned(), format!("{which}")]))
                .expect("minted");
        }
        let state = Arc::new(State {
            store: Arc::clone(&store) as Arc<dyn JobStore>,
            policy: Policy::default(),
            authority: Authority::default(),
            started: Instant::now(),
            peers: Mutex::new(Peers::default()),
            dispatching: Mutex::new(()),
        });
        // Outside the measurement: the pass that judges every job for the
        // first time writes a line for each, and is allowed to.
        dispatch_queued_jobs(&state).expect("the first pass runs");

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKERS)
            .enable_all()
            .build()
            .expect("a runtime");
        // One blocking pass loop per worker, each making the queue move
        // between its passes, which is what every caller of
        // `dispatch_queued_jobs` does: it is a synchronous function and it
        // holds the worker that called it.
        let mut passes = Vec::new();
        for caller in 0..WORKERS {
            let (state, store) = (Arc::clone(&state), Arc::clone(&store));
            passes.push(runtime.spawn(async move {
                for arrival in 0..PASSES {
                    // Distinct work per caller: two submissions of the same
                    // piece of work coalesce, and this test needs the queue
                    // to grow.
                    store
                        .create_job(NewJob::new(vec![
                            "true".to_owned(),
                            format!("arrival {caller}.{arrival}"),
                        ]))
                        .expect("minted");
                    dispatch_queued_jobs(&state).expect("the pass runs");
                }
            }));
        }

        // The requester, asked for from this thread rather than from a task,
        // so that nothing about the measurement can be starved by the thing
        // it is measuring. Twenty of them, because the first could be lucky
        // and land before a pass has begun.
        let mut worst = Duration::ZERO;
        for _ in 0..20 {
            let began = Instant::now();
            let (answered, answer) = std_mpsc::channel();
            runtime.spawn(async move {
                let _ = answered.send(());
            });
            match answer.recv_timeout(Duration::from_secs(60)) {
                Ok(()) => worst = worst.max(began.elapsed()),
                Err(_) => {
                    worst = Duration::from_secs(60);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for pass in passes {
            runtime.block_on(pass).expect("the passes end");
        }

        println!(
            "herd-hub: with {QUEUED} jobs queued and {WORKERS} passes running, the slowest \
             turn the runtime gave a task that does nothing was {worst:?}."
        );
        assert!(
            worst < PATIENCE,
            "a task that does nothing waited {worst:?} for a turn while {WORKERS} dispatch \
             passes ran over a {QUEUED}-job queue: the hub has no worker left to answer a \
             requester or a health report with, which is how it stopped answering anything \
             (D171)"
        );
    }

    /// What the hub can check about a declaration without a repository, and
    /// each of the three things it must not let past (E-11, D106).
    ///
    /// The hub trusts the requester for the *content* of the declaration — it
    /// holds no repository and cannot check the file against the commit, which
    /// D106 states as its cost. These three are different: each one is a
    /// declaration no runner could execute as declared, and each would become
    /// a job that ran something nobody declared.
    #[test]
    fn a_declaration_no_runner_could_execute_is_refused_by_name() {
        let hook = |run: &str| herd_proto::Executor {
            up: "tool ws up {job}".to_owned(),
            fetch: "tool ws fetch {job}".to_owned(),
            run: run.to_owned(),
            down: "tool ws down {job}".to_owned(),
            env: Default::default(),
        };
        let asked = |commit: &str, executor: herd_proto::Executor| herd_proto::Asked {
            command: vec!["a-line".to_owned()],
            labels: LabelSet::default(),
            priority: Priority::Step,
            build_thread_mib: None,
            memory_mib: None,
            tmpfs_mib: None,
            deadline_unix_s: None,
            after: None,
            requeue_exit_code: None,
            declaration: Some(herd_proto::Declaration {
                command: "a-declared-command".to_owned(),
                commit: commit.to_owned(),
                args: Vec::new(),
                executor,
            }),
            labels_from: None,
            timeout_s: None,
        };
        const GOOD: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";

        assert_eq!(
            why_that_declaration_will_not_do(&asked(GOOD, hook("tool ws run {job} -- {line}"))),
            None,
            "a declaration a runner could execute is let through"
        );

        // A ref rather than the hash the requester's own checkout resolved:
        // resolving it would mean resolving it on a machine, which is what
        // E-01 forbids.
        let said =
            why_that_declaration_will_not_do(&asked("main", hook("tool ws run {job} -- {line}")))
                .expect("a ref is not a commit this hub will take");
        assert!(
            said.contains("main") && said.contains("forty"),
            "the refusal names what it was sent and what it needed: {said}"
        );

        // A step with no line is a step that would silently not happen, and
        // the four steps run with no conditions at all (D77).
        let said =
            why_that_declaration_will_not_do(&asked(GOOD, hook("   "))).expect("an empty hook");
        assert!(
            said.contains("run") && said.contains("empty"),
            "the refusal names the step: {said}"
        );

        // The sharpest one: a `run` hook with no {line} would run their
        // workspace tool and report *its* exit code as a verdict about their
        // tests (D20).
        let said = why_that_declaration_will_not_do(&asked(GOOD, hook("tool ws run {job}")))
            .expect("a run hook that would never run the line");
        assert!(
            said.contains(herd_proto::LINE_TOKEN) && said.contains("a-declared-command"),
            "the refusal names the token and the command: {said}"
        );

        // And a job that is not a declared command is not judged by this at
        // all: every job before M10 is one, and so is every `herdctl run --`.
        let mut ad_hoc = asked(GOOD, hook("tool ws run {job} -- {line}"));
        ad_hoc.declaration = None;
        assert_eq!(why_that_declaration_will_not_do(&ad_hoc), None);

        // And the three a `labels_from` reading can earn (D74).
        let reading = |line: &str| {
            let mut asked = asked(GOOD, hook("tool ws run {job} -- {line}"));
            asked.labels_from = Some(herd_proto::LabelsFrom {
                line: line.to_owned(),
                baseline: LabelSet::of("build").expect("a label"),
            });
            asked
        };
        assert_eq!(
            why_that_declaration_will_not_do(&reading("tool test --labels")),
            None,
            "a reading a runner could run is let through"
        );

        // A reading with no declaration around it: there is no commit, no
        // command and no arguments, which is the whole of the cache key and
        // the whole of what a runner would check out.
        let mut orphan = reading("tool test --labels");
        orphan.declaration = None;
        let said =
            why_that_declaration_will_not_do(&orphan).expect("a reading with no declared command");
        assert!(
            said.contains("tool test --labels") && said.contains("no declared command"),
            "the refusal names the line and what is missing: {said}"
        );

        // An empty reading would answer nothing, and every job of that
        // command would then be refused for want of requirements.
        let said = why_that_declaration_will_not_do(&reading("  ")).expect("an empty reading");
        assert!(
            said.contains("a-declared-command") && said.contains("empty line"),
            "the refusal names the command: {said}"
        );

        // The sharp one: a reading asking for the figure the hub derives from
        // the machine it chooses. There is no N when a reading runs — it is
        // placed before the job whose figure it would be — and an answer that
        // depended on one could not be a property of (commit, command, args),
        // which is what D74 caches it as. D82 anticipates this; M18 settles
        // it with the consumer.
        let said = why_that_declaration_will_not_do(&reading(&format!(
            "tool test --labels -j {}",
            herd_proto::PARALLELISM_TOKEN
        )))
        .expect("a reading that asks for a derived -j");
        assert!(
            said.contains(herd_proto::PARALLELISM_TOKEN)
                && said.contains("a-declared-command")
                && said.contains("cached by (commit, command, args)"),
            "the refusal names the token, the command and why: {said}"
        );
    }

    /// The hub's half of D151: a burst of frames about one written-off
    /// attempt leaves **one** line in the journal, not one line per frame.
    ///
    /// The same field failure as the runner's, from the other end of the
    /// link. The hub answers every frame of a dead attempt with its own
    /// `StaleAttempt` — which is correct and is not touched here — and it
    /// also wrote `herd-hub: {reason}` beside each one, so the 1497 buffered
    /// chunks of `j0000000057` cost the hub 1497 writes into Docker's
    /// logging stack while the host it shares with the runner was stalled at
    /// 96 %.
    ///
    /// The assertion is the **count of writes**, not the wording: a test on
    /// the sentence would pass again the day the per-frame line comes back
    /// phrased differently. The refusal *on the wire* is counted too, and
    /// must stay at one per frame — silencing that would stop the runner
    /// learning its attempt is dead, which is E-21 and not a logging
    /// question.
    #[tokio::test]
    async fn a_burst_of_refused_frames_leaves_one_line_in_the_hub_s_journal() {
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let state = State {
            store: Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"))
                as Arc<dyn JobStore>,
            policy: Policy::default(),
            authority: Authority::default(),
            started: Instant::now(),
            peers: Mutex::new(Peers::default()),
            dispatching: Mutex::new(()),
        };
        let (outbox, mut inbox) = mpsc::unbounded_channel();
        // A job this hub has no record of is exactly what a written-off
        // attempt looks like from here: superseded, nothing to record.
        let job_id = JobId::new("j0000000057");
        let mut said = AlreadySaid::new();

        let frames = 1497;
        let (refused, written) = herd_runner::testing::stderr_lines_about("j0000000057", || {
            (0..frames)
                .filter(|_| !is_current(&state, "the-runner", &outbox, &mut said, &job_id, 1))
                .count()
        });

        assert_eq!(
            written.len(),
            1,
            "{frames} refused frames about one attempt must write one journal line, not {}: \
             {written:#?}",
            written.len()
        );
        assert_eq!(
            refused, frames,
            "every one of them must still be refused: the quiet is in the journal, not on \
             the wire"
        );
        let mut told = 0;
        while inbox.try_recv().is_ok() {
            told += 1;
        }
        assert_eq!(
            told, frames,
            "and the runner must be told every time, or it never learns the attempt is dead"
        );

        // Another attempt of the same job is a different fact and is worth
        // its own line: the quiet is per attempt, not per job.
        let (_, written) = herd_runner::testing::stderr_lines_about("j0000000057", || {
            is_current(&state, "the-runner", &outbox, &mut said, &job_id, 2)
        });
        assert_eq!(
            written.len(),
            1,
            "silencing the repeats must not silence another attempt: {written:#?}"
        );
    }

    /// What one connection remembers is bounded, and it is freed with the
    /// connection rather than by an eviction policy — so the only bound that
    /// has to be proved is the ring's (D151).
    #[test]
    fn what_a_connection_remembers_about_refusals_is_bounded() {
        let mut said = AlreadySaid::new();
        herd_runner::testing::stderr_lines_about("", || {
            for n in 0..(REFUSALS_REMEMBERED * 4) {
                said.say_once(
                    &JobId::new(format!("j{n:010}")),
                    1,
                    Refusal::Superseded,
                    "herd-hub: written off",
                );
            }
        });
        assert_eq!(
            said.said.len(),
            REFUSALS_REMEMBERED,
            "the ring must stop at {REFUSALS_REMEMBERED}, not grow with the connection"
        );
    }

    // ----------- a busy runner is not a lost one (E-21, D169) ----

    /// A runner that is here and says it is still running the attempt is not
    /// a runner that stopped answering, whatever the clock says.
    ///
    /// This is the half of the nightly incident that turned a buried host
    /// into 201 jobs ending 125: an attempt nothing had been heard about for
    /// the threshold was ended with "it is not coming back" while the machine
    /// was on the line and running it, and the runner's own frames about it
    /// were then refused as superseded.
    #[test]
    fn a_runner_that_says_it_is_running_the_attempt_is_not_a_lost_one() {
        let says = what_a_runner_says(
            Some(TheRunnersWord {
                reports: 12,
                reported: Duration::from_secs(1),
                names_it: true,
            }),
            Duration::from_secs(90),
        );
        assert!(
            matches!(says, WhatTheRunnerSays::StillRunningIt { .. }),
            "{says:?}"
        );
    }

    /// And the other direction, which is what the clock was standing in for:
    /// a runner that is here and does **not** name the attempt has answered
    /// the question, so the attempt is over and the job gets its next go at
    /// once rather than a threshold later.
    #[test]
    fn a_runner_that_no_longer_names_the_attempt_has_lost_it() {
        let says = what_a_runner_says(
            Some(TheRunnersWord {
                reports: 12,
                reported: Duration::from_secs(1),
                names_it: false,
            }),
            Duration::from_secs(90),
        );
        assert!(
            matches!(says, WhatTheRunnerSays::NotAnyMore { .. }),
            "{says:?}"
        );
    }

    /// A report that is older than the hub's last word about the attempt is
    /// not evidence about it: it was composed before the job was handed out.
    /// Reading it as "the runner does not have this" would spend a go on a
    /// machine that did nothing wrong.
    #[test]
    fn a_report_older_than_the_last_word_about_the_attempt_says_nothing() {
        let stale = what_a_runner_says(
            Some(TheRunnersWord {
                reports: 12,
                reported: Duration::from_secs(90),
                names_it: false,
            }),
            Duration::from_secs(61),
        );
        assert_eq!(stale, WhatTheRunnerSays::NotYetSaying);
        // Nor is a connection nothing has come back on yet — a runner that
        // has just redialled has not been asked anything.
        let fresh_link = what_a_runner_says(
            Some(TheRunnersWord {
                reports: 0,
                reported: Duration::from_millis(5),
                names_it: false,
            }),
            Duration::from_secs(90),
        );
        assert_eq!(fresh_link, WhatTheRunnerSays::NotYetSaying);
    }

    /// And the case E-21 exists for is untouched: no runner of that name is
    /// connected, so there is nothing to go on but the clock.
    #[test]
    fn a_runner_that_is_not_there_leaves_the_clock_to_decide() {
        assert_eq!(
            what_a_runner_says(None, Duration::from_secs(90)),
            WhatTheRunnerSays::NothingAtAll
        );
    }

    /// A reading cap of zero is not a tight bound but an absent one: no
    /// reading is ever placed, so every job whose command declares
    /// `labels_from` waits for ever. Refused at startup, where the message
    /// names the figure the operator typed.
    #[test]
    fn a_hub_told_to_run_no_reading_at_all_refuses_to_start() {
        let policy = Policy {
            readings_at_once: 0,
            ..Policy::default()
        };
        let refusal = policy.refusal().expect("a cap of zero is refused");
        assert!(refusal.contains("--readings-at-once 0"), "{refusal}");
        assert!(Policy::default().refusal().is_none());
    }
}
