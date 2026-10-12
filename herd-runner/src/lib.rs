//! A Herd runner: dials the hub, executes what it is assigned, streams the
//! output as it is produced, reports how the job ended.
//!
//! The runner opens the connection and needs nothing inbound (invariant 3).
//! M1 runs a job with no caps of any kind; confinement, memory and time caps
//! are M4's, and until they exist a runner must only be pointed at a hub its
//! owner trusts.

use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobOutcome, LogStream, Role, ToHub, PROTOCOL_VERSION,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Where a runner looks for its hub unless told otherwise.
pub const DEFAULT_HUB_URL: &str = "ws://127.0.0.1:8787";

/// Output is read in chunks this size; a chunk is sent as soon as the process
/// has written anything, which is what makes a log arrive while it is running.
const READ_CHUNK: usize = 8 * 1024;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Socket, Message>;
type Source = SplitStream<Socket>;
type Outbox = UnboundedSender<ToHub>;

/// What a runner needs to know to join a hub.
#[derive(Debug, Clone)]
pub struct Config {
    /// `ws://host:port` inside the compose network and in tests, `wss://...`
    /// through the reverse proxy.
    pub hub_url: String,
    /// Name reported to the hub, for its logs and reports.
    pub name: Option<String>,
    /// Bearer token. Carried from M1, checked by the hub from M15.
    pub token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hub_url: DEFAULT_HUB_URL.to_owned(),
            name: None,
            token: None,
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
    held: Arc<Mutex<Vec<(JobId, JobOutcome)>>>,
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

    fn hold(&self, job_id: JobId, outcome: JobOutcome) {
        self.with(|held| {
            held.retain(|(id, _)| id != &job_id);
            held.push((job_id, outcome));
        });
    }

    fn release(&self, job_id: &JobId) {
        self.with(|held| held.retain(|(id, _)| id != job_id));
    }

    fn all(&self) -> Vec<(JobId, JobOutcome)> {
        self.with(|held| held.clone())
    }

    fn with<T>(&self, f: impl FnOnce(&mut Vec<(JobId, JobOutcome)>) -> T) -> T {
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
struct Link {
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
    fn send(&self, message: ToHub) -> bool {
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
    fn send_chunk(
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
/// The connection dropping is not the end of the runner (D17): a job that is
/// running keeps running, the runner dials again on a fixed delay, and the
/// result of a job that ended while there was no hub is redelivered on the new
/// connection until the hub acknowledges it. A *refusal* is different and
/// ends here: the hub said no, and dialling again would not change its mind.
pub async fn run(config: Config) -> Result<()> {
    let session = Session::new();
    let mut failures = 0u32;
    loop {
        match serve_once(config.clone(), session.clone()).await {
            // The hub closed the connection. Nothing says the runner is done,
            // so it waits and joins the hub that comes back.
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
}

/// Join the hub and serve it until this one connection ends.
///
/// Public because a test needs one connection it can watch, and because
/// [`run`] is only this in a loop.
pub async fn serve_once(config: Config, session: Session) -> Result<()> {
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
    for (job_id, outcome) in session.all() {
        session.link.send(ToHub::JobFinished { job_id, outcome });
    }
    let outcome = serve_hub(&session, &mut source).await;
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

async fn serve_hub(session: &Session, source: &mut Source) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            FromHub::AssignJob { job_id, command } => {
                let session = session.clone();
                // One task per job: a long job must not stop the runner from
                // hearing the hub, and must not end when the link does.
                tokio::spawn(async move { execute(job_id, command, session).await });
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

/// Run one job: announce it, stream both streams, report how it ended.
async fn execute(job_id: JobId, command: Vec<String>, session: Session) {
    let Some((program, arguments)) = command.split_first() else {
        report(
            &job_id,
            &session,
            JobOutcome::CouldNotStart {
                reason: "a job needs a command to run".to_owned(),
            },
        );
        return;
    };

    let mut child = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            // Names the executable, so the reason reads as Herd failing to run
            // this command rather than the command failing.
            report(
                &job_id,
                &session,
                JobOutcome::CouldNotStart {
                    reason: format!("{program}: {error}"),
                },
            );
            return;
        }
    };
    session.link.send(ToHub::JobStarted {
        job_id: job_id.clone(),
    });

    // One counter for both streams, and it outlives every connection this job
    // reports on: the hub counts chunks per job, so a number must not start
    // again at 0 because the link was redialled under a running job (D17).
    let seq = Arc::new(AtomicU64::new(0));
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let pump_out = stream_output(
        stdout,
        LogStream::Stdout,
        job_id.clone(),
        Arc::clone(&seq),
        session.link.clone(),
    );
    let pump_err = stream_output(
        stderr,
        LogStream::Stderr,
        job_id.clone(),
        Arc::clone(&seq),
        session.link.clone(),
    );
    // Both pipes are drained to end of file before the outcome is reported, so
    // no output can arrive after JobFinished.
    tokio::join!(pump_out, pump_err);

    let outcome = match child.wait().await {
        Ok(status) => outcome_of(status),
        // The process started but Herd never learned how it ended, so it has
        // no code and no signal to report. M3's "lost" is where this belongs;
        // until then it is Herd failing to run the job, said in full.
        Err(error) => JobOutcome::CouldNotStart {
            reason: format!("{program}: waiting for the process failed: {error}"),
        },
    };
    report(&job_id, &session, outcome);
}

/// How a finished process ended, in the protocol's terms. A unix status is an
/// exit code or a signal, never both.
#[cfg(unix)]
fn outcome_of(status: ExitStatus) -> JobOutcome {
    use std::os::unix::process::ExitStatusExt;

    match status.code() {
        Some(code) => JobOutcome::Exited { code },
        // Killed, and `signal` is then set. Signal 0 is one no platform ever
        // delivers, so it reads as "killed, by something unnamed".
        None => JobOutcome::Signalled {
            signal: status.signal().unwrap_or(0),
        },
    }
}

/// Elsewhere a process only ever has an exit code.
#[cfg(not(unix))]
fn outcome_of(status: ExitStatus) -> JobOutcome {
    JobOutcome::Exited {
        code: status.code().unwrap_or(0),
    }
}

/// Report how a job ended, and hold on to it until the hub says it has it.
///
/// The holding comes first, on purpose: between the send and an
/// acknowledgement the result exists nowhere else, and the hub may be gone.
/// A job that never ran sends no `JobStarted` and no log chunk — it wrote
/// nothing, and Herd's own failure belongs in the outcome rather than in the
/// job's output — but its end is held exactly like any other.
fn report(job_id: &JobId, session: &Session, outcome: JobOutcome) {
    session.hold(job_id.clone(), outcome.clone());
    if !session.link.send(ToHub::JobFinished {
        job_id: job_id.clone(),
        outcome,
    }) {
        // No hub to tell. The result is held, and the next connection begins
        // by redelivering it (E-44, D17).
        eprintln!(
            "herd-runner: {job_id} ended while there was no hub to tell; \
             its result is held until a hub acknowledges it"
        );
    }
}

/// Forward one pipe to the hub as it fills, never waiting for the process.
async fn stream_output<R>(
    mut pipe: R,
    stream: LogStream,
    job_id: JobId,
    seq: Arc<AtomicU64>,
    link: Link,
) where
    R: AsyncRead + AsyncReadExt + Unpin,
{
    let mut buffer = vec![0u8; READ_CHUNK];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                // The bytes as they were read: a job's output is not text, and a
                // chunk boundary may fall in the middle of a character (D18).
                link.send_chunk(&job_id, stream, &seq, buffer[..read].to_vec());
            }
            Err(error) => {
                link.send_chunk(
                    &job_id,
                    LogStream::Stderr,
                    &seq,
                    format!("herd-runner: reading the job's output failed: {error}\n").into_bytes(),
                );
                break;
            }
        }
    }
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
