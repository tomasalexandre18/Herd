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

/// The results this runner has produced and the hub has not acknowledged.
///
/// A job's result is the one thing a runner cannot reproduce: the process is
/// gone, and if the result is dropped the work is lost. So a result is kept
/// here from the moment the job ends until `JobResultAck` says the hub has it
/// on a disk, and every connection starts by redelivering what is left
/// (E-44). The hub records the same end twice as the same end, so a
/// redelivery that was in fact received is harmless.
///
/// This lives outside [`run`] so that it survives the connection: the caller
/// holds it, hands it to the next `run`, and M3 — which owns reconnection —
/// only has to put the loop around it. It is memory, not a file: a runner that
/// is itself killed loses the result, and that is M3's lost-attempt case, not
/// this one.
#[derive(Debug, Clone, Default)]
pub struct HeldResults {
    held: Arc<Mutex<Vec<(JobId, JobOutcome)>>>,
}

impl HeldResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many results are waiting to be acknowledged.
    pub fn len(&self) -> usize {
        self.with(|held| held.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

/// Join the hub and serve it until the connection ends, holding nothing from
/// an earlier connection.
///
/// M1 returns on the first disconnection; M3 adds reconnection with bounded
/// backoff, around [`run_holding`].
pub async fn run(config: Config) -> Result<()> {
    run_holding(config, HeldResults::new()).await
}

/// Join the hub and serve it, carrying results an earlier connection could not
/// deliver.
pub async fn run_holding(config: Config, held: HeldResults) -> Result<()> {
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
        Some(FromHub::Refused { reason }) => bail!("the hub refused this runner: {reason}"),
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
    }

    let (outbox, inbox) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_frames(sink, inbox));
    // Before anything else on this connection: what the last one could not
    // deliver. A job finished while the hub was away is still a job that ran.
    for (job_id, outcome) in held.all() {
        let _ = outbox.send(ToHub::JobFinished { job_id, outcome });
    }
    let outcome = serve_hub(&outbox, &mut source, &held).await;
    drop(outbox);
    let _ = writer.await;
    outcome
}

async fn serve_hub(outbox: &Outbox, source: &mut Source, held: &HeldResults) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            FromHub::AssignJob { job_id, command } => {
                let outbox = outbox.clone();
                let held = held.clone();
                // One task per job: a long job must not stop the runner from
                // hearing the hub.
                tokio::spawn(async move { execute(job_id, command, outbox, held).await });
            }
            FromHub::JobResultAck { job_id } => held.release(&job_id),
            FromHub::Refused { reason } => bail!("the hub refused this runner: {reason}"),
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
async fn execute(job_id: JobId, command: Vec<String>, outbox: Outbox, held: HeldResults) {
    let Some((program, arguments)) = command.split_first() else {
        report(
            &job_id,
            &outbox,
            &held,
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
                &outbox,
                &held,
                JobOutcome::CouldNotStart {
                    reason: format!("{program}: {error}"),
                },
            );
            return;
        }
    };
    let _ = outbox.send(ToHub::JobStarted {
        job_id: job_id.clone(),
    });

    // One counter for both streams, so the hub can restore the order this
    // runner saw the output in.
    let seq = Arc::new(AtomicU64::new(0));
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let pump_out = stream_output(
        stdout,
        LogStream::Stdout,
        job_id.clone(),
        Arc::clone(&seq),
        outbox.clone(),
    );
    let pump_err = stream_output(
        stderr,
        LogStream::Stderr,
        job_id.clone(),
        Arc::clone(&seq),
        outbox.clone(),
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
    report(&job_id, &outbox, &held, outcome);
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
fn report(job_id: &JobId, outbox: &Outbox, held: &HeldResults, outcome: JobOutcome) {
    held.hold(job_id.clone(), outcome.clone());
    let _ = outbox.send(ToHub::JobFinished {
        job_id: job_id.clone(),
        outcome,
    });
}

fn send_chunk(job_id: &JobId, outbox: &Outbox, seq: &AtomicU64, stream: LogStream, data: String) {
    let _ = outbox.send(ToHub::LogChunk {
        job_id: job_id.clone(),
        stream,
        seq: seq.fetch_add(1, Ordering::SeqCst),
        data,
    });
}

/// Forward one pipe to the hub as it fills, never waiting for the process.
async fn stream_output<R>(
    mut pipe: R,
    stream: LogStream,
    job_id: JobId,
    seq: Arc<AtomicU64>,
    outbox: Outbox,
) where
    R: AsyncRead + AsyncReadExt + Unpin,
{
    let mut buffer = vec![0u8; READ_CHUNK];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                // Lossy, as D4 says: a chunk boundary may split a character.
                let data = String::from_utf8_lossy(&buffer[..read]).into_owned();
                send_chunk(&job_id, &outbox, &seq, stream, data);
            }
            Err(error) => {
                send_chunk(
                    &job_id,
                    &outbox,
                    &seq,
                    LogStream::Stderr,
                    format!("herd-runner: reading the job's output failed: {error}\n"),
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
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
    }
}
