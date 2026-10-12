//! The requester side: submit a job, follow its log, learn its exit code.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. M1's CLI surface is `run` only; `wait` arrives with
//! M2 and `cancel` with M3.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, LogStream, Role, ToHub, PROTOCOL_VERSION};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Where herdctl looks for the hub unless told otherwise.
pub const DEFAULT_HUB_URL: &str = "ws://127.0.0.1:8787";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What a requester needs to reach a hub.
#[derive(Debug, Clone)]
pub struct Config {
    pub hub_url: String,
    /// Bearer token. Carried from M1, checked by the hub from M15.
    pub token: Option<String>,
    /// Name reported to the hub, for its logs and reports.
    pub name: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hub_url: DEFAULT_HUB_URL.to_owned(),
            token: None,
            name: None,
        }
    }
}

/// How a followed job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub job_id: JobId,
    pub exit_code: i32,
}

/// Submit one command and follow it to its end.
///
/// `on_log` is called with each chunk as it arrives, in the order the runner
/// produced it; the binary prints them, a test times them.
pub async fn run_job(
    config: &Config,
    command: Vec<String>,
    mut on_log: impl FnMut(LogStream, &str),
) -> Result<Outcome> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let (mut socket, _response) = tokio_tungstenite::connect_async(&config.hub_url)
        .await
        .with_context(|| format!("dialling {}", config.hub_url))?;

    send(
        &mut socket,
        &ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Requester,
            token: config.token.clone(),
            name: config.name.clone(),
        },
    )
    .await
    .context("sending hello")?;

    match next_message(&mut socket).await? {
        Some(FromHub::Welcome { .. }) => {}
        Some(FromHub::Refused { reason }) => bail!("the hub refused this requester: {reason}"),
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
    }

    send(&mut socket, &ToHub::SubmitJob { command })
        .await
        .context("submitting the job")?;

    let mut job_id: Option<JobId> = None;
    while let Some(message) = next_message(&mut socket).await? {
        match message {
            FromHub::JobAccepted { job_id: accepted } => job_id = Some(accepted),
            FromHub::LogChunk { stream, data, .. } => on_log(stream, &data),
            FromHub::JobFinished { job_id, exit_code } => {
                let _ = socket.close(None).await;
                return Ok(Outcome { job_id, exit_code });
            }
            FromHub::Refused { reason } => bail!("the hub refused the job: {reason}"),
            other => bail!(
                "the hub sent {}, which a requester cannot use",
                message_kind(&other)
            ),
        }
    }
    match job_id {
        Some(job_id) => bail!("the hub closed the connection while {job_id} was still running"),
        None => bail!("the hub closed the connection without accepting the job"),
    }
}

async fn send(socket: &mut Socket, message: &ToHub) -> Result<()> {
    let frame = to_frame(message).context("encoding a message")?;
    socket.send(Message::text(frame)).await.context("sending")
}

async fn next_message(socket: &mut Socket) -> Result<Option<FromHub>> {
    while let Some(frame) = socket.next().await {
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
    }
}
