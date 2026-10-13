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

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    a_frame_about_a_job_that_is_over_reason, foreign_state_root_reason, from_frame,
    not_this_runner_s_attempt_reason, runner_twin_reason, to_frame, version_mismatch_reason,
    FromHub, JobId, JobMetrics, JobOutcome, Role, RunningJob, ToHub, HEALTH_EVERY,
    PROTOCOL_VERSION,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::job::{JobRecord, JobState, JobStore, LogWrite};

/// Loopback by default: the hub is reached through the reverse proxy, which
/// terminates TLS; the hub itself only ever speaks plain `ws://` (D4).
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8787";

/// How long a runner may be unheard from before it is lost (E-21).
pub const LOST_AFTER: Duration = Duration::from_secs(60);

/// How many goes at running one job the hub allows (E-21).
///
/// Two: the attempt, and one more after a loss. The second loss is an end,
/// because the alternative is a job that destroys every runner it touches
/// going round the herd for ever. M6's priorities do not change this; a third
/// attempt would be a different promise and nobody has asked for one.
pub const ATTEMPTS_ALLOWED: u32 = 2;

/// The hub's own version, reported in `Welcome`.
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
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            lost_after: LOST_AFTER,
            health_every: HEALTH_EVERY,
            attempts_allowed: ATTEMPTS_ALLOWED,
            write_retry_first: Duration::from_millis(250),
            write_retry_cap: Duration::from_secs(5 * 60),
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
    /// the port and ends the hub's own two housekeeping tasks.
    pub async fn serve(self) -> Result<()> {
        let watching = tokio::spawn(watch_for_a_lost_runner(Arc::clone(&self.state)));
        let writing = tokio::spawn(keep_trying_to_write_results(Arc::clone(&self.state)));
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
        let outcome = serving.await;
        watching.abort();
        writing.abort();
        outcome
    }
}

/// What can still be asked of a hub that `serve` has taken ownership of.
#[derive(Clone)]
pub struct HubHandle {
    state: Arc<State>,
}

impl HubHandle {
    /// Whether a runner has finished its handshake and can be dispatched to.
    pub fn runner_connected(&self) -> bool {
        self.state.with_peers(|peers| peers.runner.is_some())
    }

    /// What the hub knows about the runners connected to it, health ages
    /// included (E-40). One entry at most until M5, which holds a set.
    pub fn runners(&self) -> Vec<RunnerStatus> {
        self.state.with_peers(|peers| {
            peers
                .runner
                .iter()
                .map(|runner| RunnerStatus {
                    name: runner.name.clone(),
                    instance: runner.instance.clone(),
                    joined_for: runner.joined_at.elapsed(),
                    last_health_age: runner.last_health.elapsed(),
                    reports_seen: runner.reports_seen,
                    draining: runner.draining,
                    jobs: runner.jobs.iter().map(|job| job.job_id.clone()).collect(),
                })
                .collect()
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
    pub jobs: Vec<JobId>,
}

struct State {
    store: Arc<dyn JobStore>,
    policy: Policy,
    /// When this hub started serving: where the clock begins for an attempt
    /// it inherited and has heard nothing about.
    started: Instant,
    peers: Mutex<Peers>,
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
    /// How the hub ends this connection without waiting for the socket to
    /// notice. Used when a runner that is plainly alive dials in while this
    /// connection is still registered: the new one is the real one.
    kick: watch::Sender<bool>,
    joined_at: Instant,
    last_health: Instant,
    reports_seen: u64,
    draining: bool,
    jobs: Vec<RunningJob>,
}

#[derive(Default)]
struct Peers {
    /// M1 accepts exactly one runner. M5 onwards holds a set.
    runner: Option<ConnectedRunner>,
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
    /// nothing comes back on is not a link and holding it would keep the slot
    /// against a runner that is plainly alive.
    fn register_runner(
        &self,
        name: &str,
        instance: Option<&str>,
        outbox: &Outbox,
        kick: watch::Sender<bool>,
    ) -> Admitted {
        self.with_peers(|peers| {
            if let Some(incumbent) = &peers.runner {
                let same_name = !name.is_empty() && incumbent.name == name;
                // An instance nobody announced is not an instance that
                // matches: the safe reading of "I cannot tell whether this is
                // the same process" is that it is not, because the harm is
                // one-sided — a redial told to wait comes back in a moment,
                // while a live runner kicked off its job has lost the job.
                let same_instance = instance.is_some() && incumbent.instance.as_deref() == instance;
                let quiet_for = incumbent.last_health.elapsed();
                let gone_quiet =
                    incumbent.reports_seen > 0 && quiet_for > self.policy.health_is_stale_after();
                if !(same_name && same_instance) && !gone_quiet {
                    // How soon it is worth coming back differs by case. A
                    // runner waiting for a slot may have it in a moment; a
                    // twin is waiting for a configuration to change, or for
                    // the runner holding the name to go quiet, and asking
                    // every few seconds for that would be the flap again in
                    // a quieter costume — one line of log per second instead
                    // of one dropped job.
                    let retry_in = if same_name {
                        self.policy.health_is_stale_after()
                    } else {
                        self.policy.stand_by_retry()
                    };
                    let reason = if same_name {
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
                        twin
                    } else {
                        format!(
                            "runner {} is connected and was heard from {:?} ago; this hub \
                             accepts one runner until M5",
                            incumbent.name, quiet_for
                        )
                    };
                    return Admitted::StandBy { reason, retry_in };
                }
                let why = if same_name && same_instance {
                    format!(
                        "runner {name} (instance {}) has dialled again",
                        instance.unwrap_or("unnamed")
                    )
                } else {
                    format!(
                        "runner {} has said nothing for {:?}",
                        incumbent.name, quiet_for
                    )
                };
                println!("herd-hub: dropping the connection this hub was holding, because {why}");
                let _ = incumbent.kick.send(true);
            }
            let now = Instant::now();
            peers.runner = Some(ConnectedRunner {
                name: name.to_owned(),
                instance: instance.map(str::to_owned),
                outbox: outbox.clone(),
                kick,
                joined_at: now,
                last_health: now,
                reports_seen: 0,
                draining: false,
                jobs: Vec::new(),
            });
            Admitted::Yes
        })
    }

    /// Give up the runner slot, if this connection is the one holding it.
    ///
    /// The clock on the attempts it was running is not touched here: a socket
    /// closing is not evidence about a job, and a runner that redials in a
    /// second finishes what it was doing. What times an attempt out is the
    /// absence of anything said about it ([`Peers::attempt_seen`]).
    fn release_runner(&self, outbox: &Outbox) -> bool {
        self.with_peers(|peers| {
            if !peers
                .runner
                .as_ref()
                .is_some_and(|runner| runner.outbox.same_channel(outbox))
            {
                return false;
            }
            peers.runner = None;
            true
        })
    }

    /// The runner a job may be handed to: connected, and not on its way out
    /// (invariant 2).
    fn runner_for_dispatch(&self) -> Option<(String, Outbox)> {
        self.with_peers(|peers| {
            peers
                .runner
                .as_ref()
                .filter(|runner| !runner.draining)
                .map(|runner| (runner.name.clone(), runner.outbox.clone()))
        })
    }

    /// Whichever runner is connected, draining or not: where an
    /// acknowledgement or a cancel goes.
    fn runner(&self) -> Option<Outbox> {
        self.with_peers(|peers| peers.runner.as_ref().map(|runner| runner.outbox.clone()))
    }

    /// Why a submission cannot be placed, if it cannot.
    fn why_no_runner(&self) -> Option<String> {
        self.with_peers(|peers| match &peers.runner {
            None => Some("no runner is connected".to_owned()),
            Some(runner) if runner.draining => Some(format!(
                "runner {} is draining and takes no new job",
                runner.name
            )),
            Some(_) => None,
        })
    }

    fn record_health(&self, outbox: &Outbox, jobs: Vec<RunningJob>, draining: bool) {
        self.with_peers(|peers| {
            if let Some(runner) = peers
                .runner
                .as_mut()
                .filter(|runner| runner.outbox.same_channel(outbox))
            {
                runner.last_health = Instant::now();
                runner.reports_seen += 1;
                runner.draining = draining;
                runner.jobs = jobs;
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
    let socket = tokio_tungstenite::accept_async(stream)
        .await
        .context("websocket handshake")?;
    let (mut sink, mut source) = socket.split();

    let Some(hello) = next_message(&mut source).await? else {
        bail!("the peer closed before saying hello");
    };
    let (role, peer_name, peer_instance, peer_state_root, peer_health_every) = match hello {
        ToHub::Hello {
            protocol_version,
            role,
            token,
            name,
            instance,
            state_root,
            health_every_ms,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                return refuse(&mut sink, version_mismatch_reason(protocol_version)).await;
            }
            // The token is carried from M1 and validated from M15 (D6, Q8).
            let _ = token;
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
                said(name).unwrap_or_default(),
                said(instance),
                said(state_root),
                health_every_ms.map(Duration::from_millis),
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
) -> Result<()> {
    let (kick, mut kicked) = watch::channel(false);
    match state.register_runner(name, instance, outbox, kick) {
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
    if state.release_runner(outbox) {
        println!(
            "herd-hub: runner {name} is gone; an attempt it was running is lost if it is \
             not back within {:?} (E-21)",
            state.policy.lost_after
        );
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

/// Hand whatever is queued to the runner that can take it.
///
/// After a restart the queue is whatever the job directories say it is, and a
/// job that was queued when the hub went down is simply a job waiting for a
/// runner — as is a job that was requeued after a loss (E-21). Nobody need be
/// following them: the requester that submitted them may be long gone, which
/// is exactly the detached case (E-03), and the result is recorded whether
/// anyone is listening or not.
///
/// Only *queued* jobs are dispatched. A job that is assigned or running
/// belongs to an attempt that is not over, and a job left `interrupted` by a
/// hub that died belongs to an attempt whose runner may still redeliver its
/// result — deciding that such an attempt is lost is the watchdog's, on the
/// clock, never a guess made here.
fn dispatch_queued_jobs(state: &State) -> Result<()> {
    let waiting = state
        .store
        .unfinished_jobs()
        .context("reading the queue the hub recovered")?;
    for record in waiting {
        if record.state != JobState::Queued {
            continue;
        }
        if !dispatch(state, &record.id, record.command.clone())? {
            return Ok(());
        }
    }
    Ok(())
}

/// Hand one job to the runner, recording the attempt before the frame goes
/// out. False when there is no runner to hand it to.
fn dispatch(state: &State, job_id: &JobId, command: Vec<String>) -> Result<bool> {
    let Some((runner_name, runner)) = state.runner_for_dispatch() else {
        return Ok(false);
    };
    let attempt = state
        .store
        .begin_attempt(job_id, &runner_name)
        .with_context(|| format!("recording an attempt on {job_id}"))?;
    state.with_peers(|peers| {
        peers
            .live_attempts
            .insert(job_id.clone(), (attempt, runner_name.clone()))
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
    println!("herd-hub: {job_id} is attempt {attempt} on runner {runner_name}");
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
            ToHub::Health { jobs, draining } => {
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
                state.record_health(outbox, its_own, draining);
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
                match state.store.job(&job_id) {
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
                    Ok(Some(_)) => {}
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
                    if let Some(runner) = state.runner() {
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

        // A connection nothing has come back on for the threshold: dropped,
        // so that the slot is free for the runner that is dialling.
        let silent_link = state.with_peers(|peers| {
            let silent = peers
                .runner
                .as_ref()
                .filter(|runner| runner.last_health.elapsed() > state.policy.lost_after)
                .map(|runner| (runner.name.clone(), runner.last_health.elapsed()));
            if silent.is_some() {
                if let Some(runner) = &peers.runner {
                    let _ = runner.kick.send(true);
                }
                peers.runner = None;
            }
            silent
        });
        if let Some((name, quiet_for)) = silent_link {
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
    state
        .store
        .end_attempt(job_id, attempt, reason)
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

    if attempt < state.policy.attempts_allowed {
        state
            .store
            .record_state(job_id, JobState::Queued)
            .with_context(|| format!("requeueing {job_id}"))?;
        println!(
            "herd-hub: {job_id} is queued again for attempt {} of {}: {reason}",
            attempt + 1,
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
        // that went — in which case the second attempt starts at once.
        dispatch(state, job_id, record.command.clone())?;
        return Ok(());
    }

    // Out of goes. An end, and a named one: not an exit code, because no
    // process gave one (D15).
    let outcome = JobOutcome::Lost {
        reason: format!(
            "{reason}; this was attempt {attempt} of {}, so the job is not attempted again",
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
            .runner
            .as_ref()
            .filter(|runner| runner.name == holder)
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

async fn serve_requester(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    let outcome = accept_submissions(state, outbox, source).await;
    state.forget_follower(outbox);
    outcome
}

async fn accept_submissions(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            ToHub::SubmitJob { command } => {
                if command.is_empty() {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "a job needs a command to run".to_owned(),
                    });
                    return Ok(());
                }
                if let Some(reason) = state.why_no_runner() {
                    let _ = outbox.send(FromHub::Refused { reason });
                    return Ok(());
                }
                let job = state
                    .store
                    .create_job(command.clone())
                    .context("minting a job")?;
                // JobAccepted first, so the requester has the id before any
                // chunk of its log can reach it.
                let _ = outbox.send(FromHub::JobAccepted {
                    job_id: job.id.clone(),
                });
                state.follow(job.id.clone(), outbox);
                if !dispatch(state, &job.id, command)? {
                    state.stop_following(&job.id);
                    let _ = outbox.send(FromHub::Refused {
                        reason: "the runner disconnected before the job was assigned".to_owned(),
                    });
                    return Ok(());
                }
            }
            ToHub::CancelJob { job_id } => {
                cancel_job(state, outbox, &job_id)?;
            }
            ToHub::WaitForJob { job_id } => {
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
                    return Ok(());
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
                                job_id,
                                note: stalled_note(&why),
                            });
                        }
                    }
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
            match state.runner() {
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
    while let Some(frame) = source.next().await {
        match frame.context("reading a frame")? {
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
    Ok(None)
}

/// A message's wire tag, for refusal texts.
fn message_kind(message: &ToHub) -> &'static str {
    match message {
        ToHub::Hello { .. } => "hello",
        ToHub::SubmitJob { .. } => "submit_job",
        ToHub::CancelJob { .. } => "cancel_job",
        ToHub::WaitForJob { .. } => "wait_for_job",
        ToHub::JobStarted { .. } => "job_started",
        ToHub::LogChunk { .. } => "log_chunk",
        ToHub::JobFinished { .. } => "job_finished",
        ToHub::Health { .. } => "health",
        ToHub::AttemptAbandoned { .. } => "attempt_abandoned",
    }
}
