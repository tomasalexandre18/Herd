//! A Herd runner: dials the hub, executes what it is assigned under caps,
//! streams the output as it is produced, reports how the job ended and what
//! it cost.
//!
//! The runner opens the connection and **binds no listening socket at all**
//! (invariant 3). The mesh-bound fetch endpoint of D10 and D11 arrives in
//! M11; until then there is nothing to bind, and a test asserts it rather
//! than trusting it.
//!
//! The connection dropping is not the end of the runner (D17): a running job
//! keeps running, the runner redials, and a result it has not had
//! acknowledged is redelivered on the next connection. Confinement is
//! prepared once, before the first dial, and outlives every connection — the
//! data root is locked for the life of the process, so it could not be
//! prepared again anyway.
//!
//! Confinement lives in four modules, each named for the thing it owns:
//! [`root`] for the one data root (E-45), [`privilege`] for staying
//! unprivileged (E-46), [`cgroup`] for the kernel memory ceiling (E-30), and
//! [`confine`] plus [`job`] for the caps a job runs under (E-34).

pub mod cgroup;
pub mod confine;
pub mod job;
pub mod metrics;
pub mod privilege;
pub mod reclaim;
pub mod root;
pub mod size;
#[doc(hidden)]
pub mod testing;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, LogStream, Role, ToHub,
    PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub use confine::{Confined, Confinement};

/// Where a runner looks for its hub unless told otherwise.
pub const DEFAULT_HUB_URL: &str = "ws://127.0.0.1:8787";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Socket, Message>;
type Source = SplitStream<Socket>;
pub(crate) type Outbox = UnboundedSender<ToHub>;

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
    /// The one data root and every cap a job runs under (M4).
    pub confinement: Confinement,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hub_url: DEFAULT_HUB_URL.to_owned(),
            name: None,
            token: None,
            confinement: Confinement::with_data_root(
                root::DataRoot::default_path().unwrap_or_else(|_| "herd-data".into()),
            ),
        }
    }
}

/// How long the runner waits before dialling the hub again, and how many
/// times it tries before giving up.
///
/// Deliberately dumb: one fixed delay, a count, no jitter and no growth. The
/// *policy* — backoff, health reports every 15 s, 60 s of silence meaning the
/// runner is lost — is M3's, and a provisional policy here would be a thing
/// M3 has to undo. What M2 needs is only that a hub going away does not kill a
/// runner and its running job with it (D17).
const REDIAL_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
const REDIAL_ATTEMPTS: u32 = 120;

/// How long a dead connection's writer is given to finish before it is
/// dropped. The link is over by then; what matters is not waiting for the jobs
/// that are still running, because their frames go to the next link.
const FLUSH_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// How long a runner that has been asked to stop waits for the jobs it just
/// ended to report, and for a hub that is still there to acknowledge them,
/// before it writes what is left to its data root (see [`stop`]).
///
/// Well inside invariant 2's ≤ 5 s, and short on purpose: the point is not to
/// get the result delivered, it is not to lose it. M3 owns the stop's real
/// budget and the drain.
const STOP_GRACE: std::time::Duration = std::time::Duration::from_millis(2_000);
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(20);

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

/// One job's end, as the runner holds it until the hub says it has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldResult {
    pub job_id: JobId,
    pub outcome: JobOutcome,
    pub metrics: JobMetrics,
}

/// What a runner keeps across connections: the results the hub has not
/// acknowledged, and the link its running jobs report on.
///
/// A job's result is the one thing a runner cannot reproduce: the process is
/// gone, and if the result is dropped the work is lost. So a result is kept
/// here from the moment the job ends until `JobResultAck` says the hub has it
/// on a disk, and every connection starts by redelivering what is left
/// (E-44). The hub records the same end twice as the same end, so a
/// redelivery that was in fact received is harmless.
///
/// It is memory, not a file: a runner that is itself killed loses what it
/// holds, and that is M3's lost-attempt case, not this one.
#[derive(Debug, Clone, Default)]
pub struct Session {
    held: Arc<Mutex<Vec<HeldResult>>>,
    link: Link,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many results are waiting to be acknowledged.
    pub fn results_held(&self) -> usize {
        self.with(|held| held.len())
    }

    /// Whether every result this runner has produced is on the hub's disk.
    pub fn holds_nothing(&self) -> bool {
        self.results_held() == 0
    }

    /// Report how a job ended, and hold on to it until the hub says it has it.
    ///
    /// The holding comes first, on purpose: between the send and an
    /// acknowledgement the result exists nowhere else, and the hub may be
    /// gone. A job that never ran sends no `JobStarted` and no log chunk — it
    /// wrote nothing, and Herd's own failure belongs in the outcome rather
    /// than in the job's output — but its end is held exactly like any other.
    pub(crate) fn report(&self, job_id: &JobId, outcome: JobOutcome, metrics: JobMetrics) {
        self.hold(HeldResult {
            job_id: job_id.clone(),
            outcome: outcome.clone(),
            metrics: metrics.clone(),
        });
        if !self.link.send(ToHub::JobFinished {
            job_id: job_id.clone(),
            outcome,
            metrics,
        }) {
            // No hub to tell. The result is held, and the next connection
            // begins by redelivering it (E-44, D17).
            eprintln!(
                "herd-runner: {job_id} ended while there was no hub to tell; \
                 its result is held until a hub acknowledges it"
            );
        }
    }

    /// Write every result the hub has not acknowledged into the data root,
    /// and say where each one went.
    ///
    /// Called when the runner is leaving and a held result would otherwise go
    /// with it (see [`stop`]). What is written is the `job_finished` frame
    /// itself, verbatim — the protocol is the contract (invariant 4), so the
    /// file needs no shape of its own and M3 can replay it as it stands.
    ///
    /// Every line it returns is meant to be loud: this is a verdict that did
    /// not reach the hub, and the one thing that must never happen to it is
    /// for it to disappear quietly.
    pub fn spool(&self, root: &root::DataRoot) -> Vec<String> {
        let held = self.all();
        if held.is_empty() {
            return Vec::new();
        }
        let mut said = vec![format!(
            "{} result(s) were never acknowledged by a hub; they are NOT lost, \
             they are being written under {}",
            held.len(),
            root.held_results_dir().display()
        )];
        for result in held {
            let frame = to_frame(&ToHub::JobFinished {
                job_id: result.job_id.clone(),
                outcome: result.outcome.clone(),
                metrics: result.metrics.clone(),
            });
            // Whatever goes wrong, the outcome itself goes on stderr: a
            // verdict in a log a human can read beats a verdict nowhere.
            match (root.held_result_path(&result.job_id), frame) {
                (Ok(path), Ok(frame)) => match std::fs::write(&path, frame.as_bytes())
                    .and_then(|()| std::fs::File::open(&path).and_then(|file| file.sync_all()))
                {
                    Ok(()) => said.push(format!(
                        "the result of {} is held in {} and has not reached a hub: {:?}",
                        result.job_id,
                        path.display(),
                        result.outcome
                    )),
                    Err(error) => said.push(format!(
                        "COULD NOT write the unacknowledged result of {} to {}: {error}. \
                         It ended with {:?} and that is the only record of it",
                        result.job_id,
                        path.display(),
                        result.outcome
                    )),
                },
                (path, frame) => said.push(format!(
                    "COULD NOT write the unacknowledged result of {} ({}{}). It ended \
                     with {:?} and that is the only record of it",
                    result.job_id,
                    path.err().unwrap_or_default(),
                    frame.err().map(|e| e.to_string()).unwrap_or_default(),
                    result.outcome
                )),
            }
        }
        said
    }

    fn hold(&self, result: HeldResult) {
        self.with(|held| {
            held.retain(|kept| kept.job_id != result.job_id);
            held.push(result);
        });
    }

    fn release(&self, job_id: &JobId) {
        self.with(|held| held.retain(|kept| &kept.job_id != job_id));
    }

    fn all(&self) -> Vec<HeldResult> {
        self.with(|held| held.clone())
    }

    fn with<T>(&self, f: impl FnOnce(&mut Vec<HeldResult>) -> T) -> T {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut held)
    }
}

/// Whichever connection to the hub is up now.
///
/// A job outlives the connection it was assigned on (D17), so it must not hold
/// that connection's own channel: a job that ends after the runner has
/// redialled has to report on the link that is up, not into the one that
/// broke. With no link at all a frame is dropped — a log chunk written while
/// no hub existed is gone, which is M2's line and M3's problem — while a
/// *result* is held by [`Session`] until it is acknowledged, so it is
/// redelivered rather than dropped.
#[derive(Debug, Clone, Default)]
pub(crate) struct Link {
    current: Arc<Mutex<Option<Outbox>>>,
}

impl Link {
    fn attach(&self, outbox: Outbox) {
        *self.lock() = Some(outbox);
    }

    fn detach(&self) {
        *self.lock() = None;
    }

    /// Send on the link that is up, and say whether there was one.
    pub(crate) fn send(&self, message: ToHub) -> bool {
        match self.lock().as_ref() {
            Some(outbox) => outbox.send(message).is_ok(),
            None => false,
        }
    }

    /// Send one log chunk, numbering it inside the lock that enqueues it.
    ///
    /// The number and the enqueueing are one step on purpose. Two pumps —
    /// stdout and stderr — take their numbers from one counter, and if a
    /// chunk could be numbered before another and enqueued after it, the hub
    /// would see `seq` go backwards and read a reordering as output that
    /// never arrived. Numbering here makes the order on the wire the order of
    /// the numbers, so a jump in `seq` means one thing only: chunks that are
    /// gone, which is what the hub's gap marker names (D29).
    ///
    /// A chunk produced while there is no link is dropped, and its number is
    /// spent all the same: that spent number is exactly how the hub learns
    /// afterwards that something is missing.
    pub(crate) fn send_chunk(
        &self,
        job_id: &JobId,
        stream: LogStream,
        seq: &AtomicU64,
        data: Vec<u8>,
    ) -> bool {
        let link = self.lock();
        let message = ToHub::LogChunk {
            job_id: job_id.clone(),
            stream,
            seq: seq.fetch_add(1, Ordering::SeqCst),
            data,
        };
        match link.as_ref() {
            Some(outbox) => outbox.send(message).is_ok(),
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Outbox>> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Join the hub and keep serving it, across the hub going away and coming
/// back.
///
/// Confinement is checked **before** the connection is dialled: a runner that
/// cannot confine a job must not be a runner a hub can dispatch to, and the
/// refusal must name what is wrong with the machine rather than arrive as a
/// failed job later.
///
/// The connection dropping is not the end of the runner (D17): a job that is
/// running keeps running, the runner dials again on a fixed delay, and the
/// result of a job that ended while there was no hub is redelivered on the new
/// connection until the hub acknowledges it. A *refusal* is different and
/// ends here: the hub said no, and dialling again would not change its mind.
pub async fn run(config: Config) -> Result<()> {
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
    // The stop is watched around the whole redial loop, not inside one
    // connection: a runner asked to stop while there is no hub at all still
    // has jobs to end and, possibly, a verdict nobody has taken yet.
    // Pinned, because the stop goes on polling it: the acknowledgement of a
    // result the stop has just produced arrives on that same connection, and
    // a stop that dropped the reader could never see it.
    let mut serving = std::pin::pin!(async {
        let mut failures = 0u32;
        loop {
            match serve_once(config.clone(), session.clone(), Arc::clone(&confined)).await {
                // The hub closed the connection. Nothing says the runner is
                // done, so it waits and joins the hub that comes back.
                Ok(()) => failures = 0,
                Err(error) if error.downcast_ref::<Refusal>().is_some() => return Err(error),
                Err(error) => {
                    failures += 1;
                    eprintln!("herd-runner: the link to the hub ended: {error:#}");
                    if failures >= REDIAL_ATTEMPTS {
                        return Err(error.context(format!(
                            "giving up on {} after {REDIAL_ATTEMPTS} attempts",
                            config.hub_url
                        )));
                    }
                }
            }
            tokio::time::sleep(REDIAL_DELAY).await;
        }
    });
    let ended = tokio::select! {
        ended = &mut serving => ended,
        signal = stop_requested() => {
            stop(signal?, &session, &confined, serving).await;
            Ok(())
        }
    };
    // Whichever way the runner is leaving, nothing it holds goes with it. A
    // refusal and a run out of redials end the process just as a signal does,
    // and a result that exists nowhere else is not allowed to end with it.
    for line in session.spool(confined.root()) {
        eprintln!("herd-runner: {line}");
    }
    ended
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
    };
    sink.send(Message::text(to_frame(&hello).context("encoding hello")?))
        .await
        .context("sending hello")?;

    match next_message(&mut source).await? {
        Some(FromHub::Welcome { .. }) => {}
        Some(FromHub::Refused { reason }) => return Err(Refusal(reason).into()),
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
    }

    let (outbox, inbox) = mpsc::unbounded_channel();
    let mut writer = tokio::spawn(write_frames(sink, inbox));
    session.link.attach(outbox);
    // Before anything else on this connection: what the last one could not
    // deliver. A job finished while the hub was away is still a job that ran.
    for held in session.all() {
        session.link.send(ToHub::JobFinished {
            job_id: held.job_id,
            outcome: held.outcome,
            metrics: held.metrics,
        });
    }
    let outcome = serve_hub(&session, &mut source, &confined).await;
    session.link.detach();
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

/// Wait for the signals that mean "stop": the one `systemctl stop` and docker
/// send, and the one a terminal sends. `SIGKILL` cannot be waited for at all,
/// which is why there is also a reaper at startup ([`reclaim`]).
#[cfg(unix)]
async fn stop_requested() -> Result<&'static str> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut terminate = signal(SignalKind::terminate()).context("listening for SIGTERM")?;
    let mut interrupt = signal(SignalKind::interrupt()).context("listening for SIGINT")?;
    tokio::select! {
        _ = terminate.recv() => Ok("SIGTERM"),
        _ = interrupt.recv() => Ok("SIGINT"),
    }
}

#[cfg(not(unix))]
async fn stop_requested() -> Result<&'static str> {
    std::future::pending().await
}

/// End the running jobs, then make sure no verdict leaves with the process.
///
/// Two promises meet here and they pull in opposite directions. M4 says a
/// runner that is asked to stop ends its jobs: not doing so is how a `sleep`
/// survives its runner, reparented to init, with its cgroup and its workspace
/// left for nobody. M2 says a result is held until the hub acknowledges it,
/// because a destroyed verdict is the one thing Herd never does (D19, E-44) —
/// and the jobs this stop ends produce exactly such results, a second before
/// the process goes away.
///
/// So the order is: end the jobs, give their ends a bounded moment to reach a
/// hub that is still there and be acknowledged, and then write whatever is
/// still held to the data root, one file per job, naming each path on stderr.
/// A verdict on a disk is recoverable; a verdict in the memory of a process
/// that has exited is not.
///
/// **What M3 owns.** The ≤ 5 s stop of invariant 2, refusing new jobs while
/// draining, and *reading these files back* on the next start so the hub is
/// told without a human in the loop. This is the minimum honest thing: the
/// verdict survives and says where it is.
async fn stop(
    signal: &'static str,
    session: &Session,
    confined: &Arc<Confined>,
    serving: std::pin::Pin<&mut impl std::future::Future<Output = Result<()>>>,
) {
    eprintln!(
        "herd-runner: {signal} received, ending {} running job(s) before leaving",
        confined.live().count()
    );
    for line in confined.live().end_all() {
        eprintln!("herd-runner: {line}");
    }
    // The ended jobs are still reporting: each one's task has to notice its
    // process is gone, measure, hold its outcome and send it. So the hub goes
    // on being served for a moment — that is where the acknowledgement comes
    // from, and a stop that stopped reading could never see one, which would
    // make every stop write a file for a result the hub in fact had. Bounded
    // and short, because invariant 2 is about leaving quickly.
    let settled = async {
        loop {
            if confined.live().count() == 0 && session.holds_nothing() {
                return;
            }
            tokio::time::sleep(STOP_POLL).await;
        }
    };
    let _ = tokio::time::timeout(STOP_GRACE, async move {
        tokio::select! {
            _ = serving => {}
            () = settled => {}
        }
    })
    .await;
    // Anything the hub managed to assign inside that moment is ended too.
    // Refusing a job while draining is M3's (invariant 2); what must not
    // happen meanwhile is a job started here and left behind.
    for line in confined.live().end_all() {
        eprintln!("herd-runner: {line}");
    }
    // Whatever is still held is written by [`run`], on the way out: one place
    // for it, so that a refusal and a stop lose a verdict in exactly the same
    // way — which is to say, not at all.
}

async fn serve_hub(session: &Session, source: &mut Source, confined: &Arc<Confined>) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            FromHub::AssignJob { job_id, command } => {
                let session = session.clone();
                let confined = Arc::clone(confined);
                // One task per job: a long job must not stop the runner from
                // hearing the hub, and must not end when the link does.
                tokio::spawn(async move { job::execute(job_id, command, session, confined).await });
            }
            FromHub::JobResultAck { job_id } => session.release(&job_id),
            FromHub::Refused { reason } => return Err(Refusal(reason).into()),
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

async fn next_message(source: &mut Source) -> Result<Option<FromHub>> {
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

fn message_kind(message: &FromHub) -> &'static str {
    match message {
        FromHub::Welcome { .. } => "welcome",
        FromHub::Refused { .. } => "refused",
        FromHub::JobAccepted { .. } => "job_accepted",
        FromHub::AssignJob { .. } => "assign_job",
        FromHub::LogChunk { .. } => "log_chunk",
        FromHub::JobLog { .. } => "job_log",
        FromHub::JobNote { .. } => "job_note",
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
    }
}
