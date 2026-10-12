//! The requester side: submit a job, follow its log, learn how it ended.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. The surface is `run` to submit and follow, `submit`
//! to fire and leave with the id, and `wait` to come back to an id later;
//! `cancel` arrives with M3.

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

/// `herdctl`'s exit code when there is no verdict to report: Herd could not
/// run the job, lost it before it finished, refused it, or could not be
/// reached at all.
///
/// 125 is the convention of a tool that wraps another one (`env`, `timeout`):
/// the wrapper failed, not the command. It is one code for one meaning —
/// *Herd cannot tell you whether this job passed* — because a script that
/// cannot tell "the tests failed" from "the hub was down" will read a Herd
/// failure as a verdict, which is the one mistake E-02 and E-43 cannot
/// survive (D20). The job's own code is worn only when there is a real
/// verdict. A job could exit 125 by itself, which is why the reason is also
/// printed on stderr, and why the wire carries a typed outcome rather than a
/// number.
pub const NO_VERDICT_CODE: u8 = 125;

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
        JobOutcome::CouldNotStart { .. } => NO_VERDICT_CODE,
    }
}

/// The one sentence `herdctl` prints on stderr when Herd could not run the
/// job. Herd is the subject on purpose: the job did not fail, it never ran.
/// A function, so the wording lives in one place and a test can rely on it.
pub fn could_not_start_message(reason: &str) -> String {
    format!("herd could not run this job, so it never started: {reason}")
}

/// The one sentence `herdctl` prints on stderr when it has no verdict to give:
/// the hub could not be reached, it refused the job, or the job was lost
/// before it finished. Herd is the subject, as above, and the exit code is
/// [`NO_VERDICT_CODE`] — never the job's, because there is no job's code to
/// wear (D20).
pub fn no_verdict_message(reason: &str) -> String {
    format!("herd cannot give you a verdict for this job: {reason}")
}

/// Submit one command and follow it to its end.
///
/// `on_log` is called with each chunk as it arrives, in the order the runner
/// produced it, carrying the bytes the process wrote (D18); the binary prints
/// them, a test times them.
pub async fn run_job(
    config: &Config,
    command: Vec<String>,
    mut on_log: impl FnMut(LogStream, &[u8]),
) -> Result<Finished> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::SubmitJob { command })
        .await
        .context("submitting the job")?;

    let mut job_id: Option<JobId> = None;
    while let Some(message) = next_message(&mut socket).await? {
        match message {
            FromHub::JobAccepted { job_id: accepted } => job_id = Some(accepted),
            FromHub::LogChunk { stream, data, .. } => on_log(stream, &data),
            // A job this requester has just submitted has produced nothing
            // before it attached, so neither of these is expected here; both
            // are still shown rather than swallowed, the recorded log as the
            // job's own output and a note as Herd's own words (stderr).
            FromHub::JobLog { data, .. } => on_log(LogStream::Stdout, &data),
            FromHub::JobNote { note, .. } => {
                on_log(LogStream::Stderr, format!("herdctl: {note}\n").as_bytes())
            }
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

/// Submit one command and leave, with the id the hub minted.
///
/// The job runs whether anyone is listening or not — the hub records it — so
/// this is how a push hook or a script fires work and gets on with its day
/// (E-03, E-42). The id is the only handle needed afterwards:
/// [`wait_for_job`] takes it, and so does the job's directory on the hub's
/// disk.
pub async fn submit_job(config: &Config, command: Vec<String>) -> Result<JobId> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::SubmitJob { command })
        .await
        .context("submitting the job")?;
    match next_message(&mut socket).await? {
        Some(FromHub::JobAccepted { job_id }) => {
            let _ = socket.close(None).await;
            Ok(job_id)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the job: {reason}"),
        Some(other) => bail!(
            "the hub answered a submission with {}",
            message_kind(&other)
        ),
        None => bail!("the hub closed the connection without accepting the job"),
    }
}

/// Wait for a job that already exists, by id, and learn how it ended.
///
/// Works on a job that is still queued, one that is running, and one that
/// ended before this call was made: the answer comes from the hub's record
/// either way.
///
/// The job's output comes with it (D22). `on_log` is called with whatever the
/// job had already written when this wait attached — one call, both streams
/// interleaved as the hub's `log` file keeps them, so it is reported as
/// stdout — and then with every chunk that arrives afterwards on its own
/// stream. A remote requester has no other way to read a detached job's
/// output.
///
/// `on_note` is called with anything the hub says about the job itself: in M2
/// exactly one sentence, for a job whose attempt was interrupted and whose
/// result may never come back (D17). It is Herd speaking, not the job, so the
/// binary puts it on stderr.
pub async fn wait_for_job(
    config: &Config,
    job_id: JobId,
    mut on_log: impl FnMut(LogStream, &[u8]),
    mut on_note: impl FnMut(&str),
) -> Result<Finished> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::WaitForJob {
            job_id: job_id.clone(),
        },
    )
    .await
    .context("asking to be told how the job ends")?;

    while let Some(message) = next_message(&mut socket).await? {
        match message {
            FromHub::JobFinished { job_id, outcome } => {
                let _ = socket.close(None).await;
                return Ok(Finished { job_id, outcome });
            }
            FromHub::JobLog { data, .. } => on_log(LogStream::Stdout, &data),
            FromHub::LogChunk { stream, data, .. } => on_log(stream, &data),
            FromHub::JobNote { note, .. } => on_note(&note),
            FromHub::Refused { reason } => bail!("the hub refused the wait: {reason}"),
            other => bail!(
                "the hub sent {}, which a requester cannot use",
                message_kind(&other)
            ),
        }
    }
    bail!("the hub closed the connection while {job_id} was still running")
}

/// Dial the hub and get through the handshake.
async fn join(config: &Config) -> Result<Socket> {
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
        Some(FromHub::Welcome { .. }) => Ok(socket),
        Some(FromHub::Refused { reason }) => bail!("the hub refused this requester: {reason}"),
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
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
        FromHub::JobLog { .. } => "job_log",
        FromHub::JobNote { .. } => "job_note",
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
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
