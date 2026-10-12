//! The requester side: submit a job, follow its log, learn how it ended.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. The surface is `run` to submit and follow, `submit`
//! to fire and leave with the id, and `wait` to come back to an id later;
//! `cancel` arrives with M3.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    cap_exceeded_reason, from_frame, to_frame, Cap, FromHub, JobId, JobMetrics, JobOutcome,
    LogStream, Role, ToHub, PROTOCOL_VERSION,
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

/// How a followed job ended, which job it was, and what the runner measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub job_id: JobId,
    pub outcome: JobOutcome,
    pub metrics: JobMetrics,
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

/// `herdctl`'s exit code when a runner stopped the job at one of its caps
/// (M4): the time cap, the memory ceiling, the log cap or the disk budget.
/// 124 is `timeout(1)`'s code for exactly this, generalised to the other
/// three because they are the same event — the machine said no, so the job
/// has no verdict of its own. Which cap it was travels in the outcome and is
/// printed on stderr; 124 alone is never meant to say.
pub const STOPPED_BY_CAP_CODE: u8 = 124;

/// What `herdctl` exits with for a job that ended this way.
///
/// An exit code is worn untouched — 0, 1 and 75 included, as E-02 needs. A
/// signal becomes `128 + signal`, exactly as a shell reports one, so no
/// sentinel value is spent on it.
pub fn exit_code_for(outcome: &JobOutcome) -> u8 {
    match outcome {
        // A real process code is 0..=255. Anything else came from a peer
        // inventing one, and cannot be worn — so this is Herd with no verdict
        // to give, exactly like the cases below, and never 1: 1 is the code
        // that means "the job failed" (D20), and a lying peer must not be able
        // to make a caller read a failure that nothing established.
        JobOutcome::Exited { code } => u8::try_from(*code).unwrap_or(NO_VERDICT_CODE),
        JobOutcome::Signalled { signal } => {
            128u8.saturating_add(u8::try_from(*signal).unwrap_or(127))
        }
        JobOutcome::CouldNotStart { .. } => NO_VERDICT_CODE,
        JobOutcome::StoppedByCap { .. } => STOPPED_BY_CAP_CODE,
    }
}

/// The one sentence `herdctl` prints on stderr for a job a runner stopped at
/// a cap. The runner is the subject: the job did not fail, it was stopped.
pub fn stopped_by_cap_message(cap: Cap, limit: u64) -> String {
    format!(
        "this job has no verdict of its own: {}",
        cap_exceeded_reason(cap, limit)
    )
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

/// Why an `exited` outcome cannot be worn as an exit code, when it cannot.
///
/// A unix process exits with 0 to 255, so this is unreachable from a real
/// one; what it guards against is a peer reporting something else. The caller
/// gets [`NO_VERDICT_CODE`] and this sentence on stderr, because an exit code
/// Herd had to invent must never be mistaken for the job's own.
pub fn impossible_exit_code_reason(outcome: &JobOutcome) -> Option<String> {
    match outcome {
        JobOutcome::Exited { code } if u8::try_from(*code).is_err() => Some(format!(
            "the exit code reported for this job is {code}, which no process can exit with \
             (an exit code is 0 to 255)"
        )),
        _ => None,
    }
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
            FromHub::JobFinished {
                job_id,
                outcome,
                metrics,
            } => {
                let _ = socket.close(None).await;
                return Ok(Finished {
                    job_id,
                    outcome,
                    metrics,
                });
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
            FromHub::JobFinished {
                job_id,
                outcome,
                metrics,
            } => {
                let _ = socket.close(None).await;
                return Ok(Finished {
                    job_id,
                    outcome,
                    metrics,
                });
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

    /// D20: 1 is the code that means "the job failed". An exit code no
    /// process can have worn is Herd with no verdict to give, said as such,
    /// and never a failure a caller would act on. Unreachable from a real unix
    /// process; reachable from a peer that lies.
    #[test]
    fn an_exit_code_no_process_could_wear_gives_no_verdict_rather_than_a_failure() {
        for code in [-1, 256, 1000, i32::MIN, i32::MAX] {
            let outcome = JobOutcome::Exited { code };
            assert_eq!(
                exit_code_for(&outcome),
                NO_VERDICT_CODE,
                "exit code {code} must not be read as a verdict"
            );
            let reason = impossible_exit_code_reason(&outcome).expect("the reason is said");
            assert!(
                reason.contains(&code.to_string()),
                "the reason must name the code: {reason}"
            );
        }
        // A code a process really can wear is still its own, 125 included.
        for code in [0, 1, 125, 255] {
            assert!(
                impossible_exit_code_reason(&JobOutcome::Exited { code }).is_none(),
                "{code} is a code a process can exit with"
            );
        }
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

    /// A job the machine stopped is not a job that failed, so it does not get
    /// a verdict code. 124 is `timeout`'s, and every cap shares it.
    #[test]
    fn a_job_stopped_at_a_cap_is_the_wrapper_s_cap_code() {
        for cap in [Cap::Time, Cap::Memory, Cap::Log, Cap::Disk] {
            assert_eq!(
                exit_code_for(&JobOutcome::StoppedByCap { cap, limit: 600 }),
                124,
                "cap {} did not get the cap code",
                cap.name()
            );
        }
    }

    #[test]
    fn the_cap_sentence_names_the_cap_and_its_figure() {
        let message = stopped_by_cap_message(Cap::Memory, 2_147_483_648);
        assert!(message.contains("memory"), "{message}");
        assert!(message.contains("2147483648"), "{message}");
        assert!(
            message.contains("no verdict of its own"),
            "a stopped job must not read as a failing one: {message}"
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
