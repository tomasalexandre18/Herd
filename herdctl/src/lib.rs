//! The requester side: submit a job, follow its log, learn how it ended.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. The surface is `run` to submit and follow, `submit`
//! to fire and leave with the id, `wait` to come back to an id later, and
//! `cancel` to ask for a job to be ended wherever it is running (E-06).

pub mod checkout;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    cap_stop_reason, from_frame, to_frame, Asked, Cap, CapabilityOffer, Declaration, FromHub,
    JobId, JobMetrics, JobOutcome, LabelSet, LogStream, Priority, Role, RunnerSwitch, Switch,
    ToHub, PROTOCOL_VERSION,
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
///
/// A job that was cancelled, and one whose runner was lost, are both "Herd
/// cannot give you a verdict" and so both 125 (D20): nobody ran the tests to
/// the end, and the one mistake a caller must not be able to make is reading
/// either of those as a failing build.
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
        JobOutcome::CouldNotStart { .. }
        | JobOutcome::Cancelled { .. }
        | JobOutcome::Impossible { .. }
        | JobOutcome::Lost { .. } => NO_VERDICT_CODE,
        JobOutcome::StoppedByCap { .. } => STOPPED_BY_CAP_CODE,
    }
}

/// Why there is no verdict, when the job's end is one of the ends that is not
/// a verdict. `None` for an end that is one.
///
/// The sentence goes on stderr beside the 125, so that a caller reading a
/// terminal can tell a cancelled job from a lost one from a build that never
/// started — the exit code cannot, and is not asked to.
pub fn no_verdict_reason(outcome: &JobOutcome) -> Option<String> {
    match outcome {
        JobOutcome::Cancelled { reason } => Some(format!("this job was cancelled: {reason}")),
        JobOutcome::Lost { reason } => {
            Some(format!("the runner executing this job was lost: {reason}"))
        }
        // Not "no machine is free": no machine could ever be. The sentence
        // says which requirement made it so, because the only thing to do
        // about one of these is change the submission (E-36).
        JobOutcome::Impossible { reason } => Some(format!(
            "no runner could ever run this job, so it was not queued for one: {reason}"
        )),
        _ => None,
    }
}

/// The one sentence `herdctl` prints on stderr for a job a runner stopped at
/// a cap. The runner is the subject: the job did not fail, it was stopped.
///
/// `detail` is the cap's own reason when its figure alone would mislead — a
/// job of four kilobytes "stopped at its disk cap of 21474836480 bytes" being
/// the case that made the field exist (D30).
pub fn stopped_by_cap_message(cap: Cap, limit: u64, detail: Option<&str>) -> String {
    format!(
        "this job has no verdict of its own: {}",
        cap_stop_reason(cap, limit, detail)
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
/// `labels` names the capabilities the job needs (E-25) — names only, because
/// a requester never names a machine (invariant 6). A job needing a capability
/// something outside Herd is holding stays queued and says so through
/// `on_note`; it is never refused for it (E-26, D38), so a caller that ignores
/// notes simply waits.
/// What a requester declares about one job's resources (E-22, E-29, E-31).
///
/// One struct rather than three arguments carried side by side: they are
/// three answers to one question — how much of a machine this job needs — and
/// the hub refuses two of them given together.
#[derive(Debug, Clone, Default)]
pub struct Declared {
    /// How much memory one build thread needs, in MiB (E-22).
    pub build_thread_mib: Option<u32>,
    /// How much memory the whole job needs, in MiB (E-29).
    pub memory_mib: Option<u64>,
    /// How much tmpfs it reserves while it runs, in MiB (E-31).
    pub tmpfs_mib: Option<u64>,
    /// When this job stops being worth starting, in seconds since the Unix
    /// epoch (E-09). Absolute, and judged on the hub's clock.
    pub deadline_unix_s: Option<i64>,
    /// The one job this one starts after (E-10). At most one, no conditions.
    pub after: Option<String>,
    /// The exit code this job's command means "what you gave me has gone" by
    /// (D76). 75 for the first consumer.
    pub requeue_exit_code: Option<i32>,
    /// The declared command this job is, read in the requester's own checkout
    /// (E-11, D106). `None` for an ad-hoc command line.
    pub declaration: Option<Declaration>,
    /// What that command declared its `run` step may take, in seconds (D108).
    pub timeout_s: Option<u64>,
}

impl Declared {
    /// What a requester asks for, as one [`herd_proto::Asked`] (E-07).
    ///
    /// One conversion, used by `run`, `submit` **and** the dry run, so that
    /// the job E-07 judges and the job a submission makes are one job.
    pub fn asked(self, command: Vec<String>, labels: LabelSet, priority: Priority) -> Asked {
        Asked {
            command,
            labels,
            priority,
            build_thread_mib: self.build_thread_mib,
            memory_mib: self.memory_mib,
            tmpfs_mib: self.tmpfs_mib,
            deadline_unix_s: self.deadline_unix_s,
            after: self.after.map(JobId::new),
            requeue_exit_code: self.requeue_exit_code,
            declaration: self.declaration,
            timeout_s: self.timeout_s,
        }
    }
}

pub async fn run_job(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
    mut on_log: impl FnMut(LogStream, &[u8]),
    mut on_note: impl FnMut(&str),
) -> Result<Finished> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: declared.asked(command, labels, priority),
        },
    )
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
            FromHub::JobNote { note, .. } => on_note(&note),
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
pub async fn submit_job(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
) -> Result<JobId> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: declared.asked(command, labels, priority),
        },
    )
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

/// Ask for a job to be cancelled, and come back with what the hub says
/// happened (E-06).
///
/// A requester never names a machine (invariant 6), here as anywhere: it names
/// the job, and the hub reaches whichever runner is executing it. The answer is
/// one sentence, because there are three true answers — it was queued and is
/// over, it is being ended on its runner, it was already finished — and a
/// caller that cannot tell them apart would have to guess.
/// Ask where a job would run, and for nothing to happen (E-07).
///
/// The same [`Asked`] a submission carries, so the answer is about the job a
/// submission would make. Returns the hub's lines, in order.
pub async fn would_run(config: &Config, asked: Asked) -> Result<Vec<String>> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::DryRun { asked })
        .await
        .context("asking where a job would run")?;
    match next_message(&mut socket).await? {
        Some(FromHub::WouldRun { lines }) => {
            let _ = socket.close(None).await;
            Ok(lines)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the question: {reason}"),
        Some(other) => bail!("the hub answered a dry run with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Move one runner's switch through the hub (E-33).
///
/// The orchestrator's half. The owner's half is `herd-runner switch` on the
/// machine itself, which needs no hub at all.
///
/// Answers with the hub's sentence and **whether the runner confirmed writing
/// the position down** (D99). The second half is not decoration: this call
/// blocks until the runner has the position on its own disk, or until E-33's
/// ten seconds are spent, and a caller that ignored the flag would report
/// success for a position a `kill -9` could still undo.
pub async fn set_switch(config: &Config, runner: String, switch: Switch) -> Result<(String, bool)> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::SetSwitch { runner, switch })
        .await
        .context("setting a switch")?;
    match next_message(&mut socket).await? {
        Some(FromHub::SwitchSet { said, confirmed }) => {
            let _ = socket.close(None).await;
            Ok((said, confirmed))
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused: {reason}"),
        Some(other) => bail!("the hub answered a switch with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Where every runner's switch stands (E-33).
pub async fn fleet_switches(config: &Config) -> Result<Vec<RunnerSwitch>> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::AskFleetSwitches {})
        .await
        .context("asking where the fleet's switches stand")?;
    match next_message(&mut socket).await? {
        Some(FromHub::FleetSwitches { runners }) => {
            let _ = socket.close(None).await;
            Ok(runners)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused: {reason}"),
        Some(other) => bail!("the hub answered with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// One runner's switch, as `herdctl switch` prints it (E-33).
///
/// The position, the owner's reason, and whether work is still running there —
/// because "off with a job on it" is the ordinary case and a line that did not
/// say so would read like a contradiction.
pub fn switch_line(runner: &RunnerSwitch) -> String {
    let mut line = format!("{}: {}", runner.name, runner.switch.sentence());
    line.push_str(&match runner.jobs {
        0 => ", nothing running".to_owned(),
        1 => ", 1 job running there finishes".to_owned(),
        many => format!(", {many} jobs running there finish"),
    });
    if runner.stopping {
        line.push_str("; it is also stopping (SIGTERM)");
    }
    line
}

/// A deadline as a requester types it, in seconds since the Unix epoch
/// (E-09).
///
/// Two spellings, and the relative one is resolved **here**, against this
/// machine's clock, so that what goes on the wire is an instant: `+15m` means
/// a quarter of an hour from now whatever the hub is doing, and `05:00
/// tomorrow` means that moment. The hub then judges the instant against its
/// own clock, which is the only clock in Herd that decides anything.
///
/// A deadline already in the past is **not** refused. "Start this only if you
/// can start it at once" is a real thing to ask for, E-41's nightly is
/// exactly that shape, and the job ends `cancelled (deadline)` on the first
/// pass with the reason naming both moments.
pub fn deadline_at(said: &str) -> Result<i64> {
    let said = said.trim();
    let now = time::OffsetDateTime::now_utc();
    if let Some(offset) = said.strip_prefix('+') {
        let seconds = parse_offset_seconds(offset)
            .ok_or_else(|| anyhow::anyhow!("{said:?} is not a span: try +15m, +2h or +90s"))?;
        return Ok(now.unix_timestamp() + seconds);
    }
    let parsed = time::OffsetDateTime::parse(said, &time::format_description::well_known::Rfc3339)
        .with_context(|| {
            format!(
                "{said:?} is not a moment: give it as RFC 3339 \
                 (2026-10-14T05:00:00+02:00) or as a span from now (+15m)"
            )
        })?;
    Ok(parsed.unix_timestamp())
}

/// `90s`, `15m`, `2h`, `1d` as seconds.
fn parse_offset_seconds(said: &str) -> Option<i64> {
    let (figure, unit) = said.split_at(said.len().checked_sub(1)?);
    let figure: i64 = figure.parse().ok()?;
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return None,
    };
    figure.checked_mul(scale)
}

pub async fn cancel_job(config: &Config, job_id: JobId) -> Result<String> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::CancelJob {
            job_id: job_id.clone(),
        },
    )
    .await
    .context("asking for the job to be cancelled")?;
    match next_message(&mut socket).await? {
        Some(FromHub::JobNote { note, .. }) => {
            let _ = socket.close(None).await;
            Ok(note)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the cancel: {reason}"),
        Some(other) => bail!("the hub answered a cancel with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering the cancel"),
    }
}

/// Ask for a job's priority to be raised, and come back with what the hub
/// says happened (E-18).
///
/// For the consumer's orchestrator, which is the only thing that raises a
/// job: an agent says what its job is worth at submission and does not come
/// back to argue. Upwards only — the hub refuses a request to lower one —
/// and never on a job that is over.
pub async fn raise_job(config: &Config, job_id: JobId, priority: Priority) -> Result<String> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::RaiseJob {
            job_id: job_id.clone(),
            priority,
        },
    )
    .await
    .context("asking for the job's priority to be raised")?;
    match next_message(&mut socket).await? {
        Some(FromHub::JobNote { note, .. }) => {
            let _ = socket.close(None).await;
            Ok(note)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the raise: {reason}"),
        Some(other) => bail!("the hub answered a raise with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering the raise"),
    }
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
            // A requester offers no capacity and runs nothing, so it has no
            // size and no preference: both are what the hub asks of a runner
            // (E-22, E-19).
            host: herd_proto::HostSize::default(),
            preference: 0,
            // Nor does a requester measure anything about a host (E-32).
            load_milli: None,
            // And a requester has no owner's switch: nothing is ever claimed
            // on it (E-33). `on` is what "nobody has withdrawn this" means.
            switch: Switch::On,
            // Neither belongs to a requester. It holds nothing between
            // connections — no attempt, no spooled verdict — so there is
            // nothing for the hub to tell apart and nothing to attribute to a
            // state root. A requester is one command's worth of conversation.
            instance: None,
            state_root: None,
            // Nor a cadence: a requester reports nothing, so there is no
            // silence of its own for a hub to time (E-40 is the runner's).
            health_every_ms: None,
            // A requester offers nothing: capacity is a runner's word.
            capabilities: CapabilityOffer::empty(),
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
        FromHub::StandBy { .. } => "stand_by",
        FromHub::HealthRecorded {} => "health_recorded",
        FromHub::JobAccepted { .. } => "job_accepted",
        FromHub::AssignJob { .. } => "assign_job",
        FromHub::CancelJob { .. } => "cancel_job",
        FromHub::StaleAttempt { .. } => "stale_attempt",
        FromHub::LogChunk { .. } => "log_chunk",
        FromHub::LogAck { .. } => "log_ack",
        FromHub::ResendLog { .. } => "resend_log",
        FromHub::JobLog { .. } => "job_log",
        FromHub::JobNote { .. } => "job_note",
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
        FromHub::WouldRun { .. } => "would_run",
        FromHub::SwitchSet { .. } => "switch_set",
        FromHub::FleetSwitches { .. } => "fleet_switches",
        FromHub::SetSwitch { .. } => "set_switch",
        FromHub::ReadProbesAgain { .. } => "read_probes_again",
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
                exit_code_for(&JobOutcome::StoppedByCap {
                    cap,
                    limit: 600,
                    detail: None
                }),
                124,
                "cap {} did not get the cap code",
                cap.name()
            );
        }
    }

    #[test]
    fn the_cap_sentence_names_the_cap_and_its_figure() {
        let message = stopped_by_cap_message(Cap::Memory, 2_147_483_648, None);
        assert!(message.contains("memory"), "{message}");
        assert!(message.contains("2147483648"), "{message}");
        assert!(
            message.contains("no verdict of its own"),
            "a stopped job must not read as a failing one: {message}"
        );
    }

    /// Finding 2 of M4's verification: a job that wrote four kilobytes and
    /// whose workspace could not be measured was told "the runner stopped
    /// this job at its disk cap of 21474836480 bytes". The figure was true
    /// and the sentence was a lie, and the real reason was only ever on the
    /// runner's stderr. The cap now carries it (D30).
    #[test]
    fn a_cap_whose_figure_would_mislead_says_what_really_happened() {
        let message = stopped_by_cap_message(
            Cap::Disk,
            21_474_836_480,
            Some("this runner could not measure the job's workspace"),
        );
        assert!(
            message.contains("could not measure"),
            "the requester must be told the real reason: {message}"
        );
        assert!(
            message.contains("could not hold it"),
            "and that the cap is what could not be enforced, not what was \
             reached: {message}"
        );
        assert!(
            !message.contains("stopped this job at its disk cap"),
            "the misleading sentence must not be the one printed: {message}"
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
