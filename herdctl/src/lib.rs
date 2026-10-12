//! The requester side: submit a job, follow its log, learn how it ended.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. M1's CLI surface is `run` only; `wait` arrives with
//! M2 and `cancel` with M3.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobOutcome, LogStream, Role, ToHub, PROTOCOL_VERSION,
};
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

/// How a followed job ended, and which job it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub job_id: JobId,
    pub outcome: JobOutcome,
}

/// `herdctl`'s exit code when Herd could not run the job at all. 125 is the
/// convention of a tool that wraps another one (`env`, `timeout`): the wrapper
/// failed, not the command. A job could exit 125 by itself, which is why the
/// reason is also printed, and why the wire carries the variant rather than a
/// number.
pub const COULD_NOT_START_CODE: u8 = 125;

/// What `herdctl` exits with for a job that ended this way.
///
/// An exit code is worn untouched — 0, 1 and 75 included, as E-02 needs. A
/// signal becomes `128 + signal`, exactly as a shell reports one, so no
/// sentinel value is spent on it.
pub fn exit_code_for(outcome: &JobOutcome) -> u8 {
    match outcome {
        // A real process code is 0..=255. Anything else came from a peer
        // inventing one, and cannot be worn.
        JobOutcome::Exited { code } => u8::try_from(*code).unwrap_or(1),
        JobOutcome::Signalled { signal } => {
            128u8.saturating_add(u8::try_from(*signal).unwrap_or(127))
        }
        JobOutcome::CouldNotStart { .. } => COULD_NOT_START_CODE,
    }
}

/// The one sentence `herdctl` prints on stderr when Herd could not run the
/// job. Herd is the subject on purpose: the job did not fail, it never ran.
/// A function, so the wording lives in one place and a test can rely on it.
pub fn could_not_start_message(reason: &str) -> String {
    format!("herd could not run this job, so it never started: {reason}")
}

/// Submit one command and follow it to its end.
///
/// `on_log` is called with each chunk as it arrives, in the order the runner
/// produced it; the binary prints them, a test times them.
pub async fn run_job(
    config: &Config,
    command: Vec<String>,
    mut on_log: impl FnMut(LogStream, &str),
) -> Result<Finished> {
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
            FromHub::JobFinished { job_id, outcome } => {
                let _ = socket.close(None).await;
                return Ok(Finished { job_id, outcome });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_code_is_worn_untouched() {
        // E-02 reads these three: pass, fail, absent or leased. 127 is a code
        // a command may choose, and it must not mean anything to herdctl.
        for code in [0, 1, 42, 75, 125, 127, 255] {
            assert_eq!(
                exit_code_for(&JobOutcome::Exited { code }),
                u8::try_from(code).expect("a code in range"),
                "exit code {code} was not worn as it was"
            );
        }
    }

    #[test]
    fn a_signal_becomes_the_shell_s_hundred_and_twenty_eight_plus_it() {
        assert_eq!(exit_code_for(&JobOutcome::Signalled { signal: 15 }), 143);
        assert_eq!(exit_code_for(&JobOutcome::Signalled { signal: 9 }), 137);
    }

    #[test]
    fn a_job_that_never_started_is_the_wrapper_s_own_code() {
        assert_eq!(
            exit_code_for(&JobOutcome::CouldNotStart {
                reason: "no-such-tool: not found".to_owned()
            }),
            125
        );
    }

    #[test]
    fn the_could_not_start_sentence_blames_herd_and_carries_the_reason() {
        let message = could_not_start_message("no-such-tool: not found");
        assert!(
            message.starts_with("herd could not run this job"),
            "{message}"
        );
        assert!(message.contains("no-such-tool: not found"), "{message}");
    }
}
