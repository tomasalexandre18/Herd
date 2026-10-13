//! Herd's wire contract.
//!
//! Every message a hub, a runner or a requester sends lives here and nowhere
//! else (CLAUDE.md invariant 4). The crate does no I/O: it defines the message
//! types, the protocol version, and the JSON framing used on the WebSocket.
//!
//! Framing (D4): one message per WebSocket **text** frame, a serde-tagged
//! JSON object carrying its variant in `"type"`.
//!
//! One payload in here is not text: a job's output. A compiler may write
//! anything at all — a NUL byte, a stray latin-1 byte in a filename — and the
//! record Herd keeps has to be the bytes the process wrote, not a rewriting of
//! them (D18, E-15). Log payloads are therefore `Vec<u8>`, carried as base64
//! inside the JSON frame by [`bytes_as_base64`].

use std::fmt;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// The version announced in [`ToHub::Hello`] and echoed in
/// [`FromHub::Welcome`]. It is bumped whenever a message shape changes in a
/// way an older peer could not read.
pub const PROTOCOL_VERSION: u32 = 1;

/// Which side of the conversation a connecting peer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Offers capacity and executes jobs.
    Runner,
    /// Submits jobs and follows them.
    Requester,
}

/// How a log payload travels inside the JSON frame: standard base64, padded.
///
/// JSON cannot carry an arbitrary byte, and a JSON string cannot carry one
/// either: serialising a job's output as text would turn every invalid UTF-8
/// sequence into U+FFFD and lose it for good. So the bytes are encoded, and
/// what comes back out is byte for byte what went in (D18).
pub mod bytes_as_base64 {
    use data_encoding::BASE64;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        BASE64
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

/// Which of a job's two output streams a log chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// How a job ended.
///
/// A job's end is not an integer. A process can exit with a code, be killed by
/// a signal, or never start at all — and later milestones add ends that are
/// none of those: M3's lost runner, M8's refusals, M9's deadline. So the end
/// travels as a tagged object, exactly as a message does, and a new way for a
/// job to end is a new variant: purely additive, since no existing frame
/// changes shape. Every variant carries named fields for the same reason, so
/// that one of them can gain a field without breaking the others.
///
/// What this type must never do is fold Herd's own failures into the
/// consumer's exit code. The code in [`JobOutcome::Exited`] is the command's
/// own, untouched, so that 0 means pass, 1 means fail and 75 means "absent or
/// leased" (E-02) — and 127 means whatever the command meant by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobOutcome {
    /// The process ran and exited with this code, reported verbatim.
    Exited { code: i32 },
    /// The process was killed by this signal, and so has no exit code.
    Signalled { signal: i32 },
    /// Herd never got an outcome out of the process: it could not be spawned,
    /// or waiting for it failed. `reason` is one sentence for a human naming
    /// what failed — the executable that was not found, say. This is Herd
    /// failing to run the job, never the job failing.
    CouldNotStart { reason: String },
    /// The runner ended the job because it broke one of the caps its owner
    /// configured (M4). Which cap, and what its figure was, so that a human
    /// reads "stopped at its time cap of 600 s" and never just "killed".
    ///
    /// This is neither the job failing nor Herd failing: the job asked for
    /// more than the machine was willing to give it. A runner must say this
    /// rather than let a job hang or grow without end (invariant 2).
    ///
    /// `detail` is for the case where the cap's figure alone would be a true
    /// number inside a false sentence. A job whose workspace could not be
    /// measured is stopped by the disk cap (D30) and may have written four
    /// kilobytes: "stopped at its disk cap of 21474836480 bytes" is then a
    /// lie, and the real reason belongs in the record rather than only on the
    /// runner's stderr. Absent whenever the cap's own figure says it all.
    StoppedByCap {
        cap: Cap,
        limit: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A requester asked for this job to be cancelled and the runner ended it
    /// (E-06). The process was most likely killed by a signal, and reporting
    /// `signalled` would be true and useless: what a reader needs to know is
    /// that somebody asked for this, so there is no verdict to read and
    /// nothing failed.
    Cancelled { reason: String },
    /// The runner executing this job went silent and the job will not be
    /// attempted again (E-21). D15 named this variant in advance as the one
    /// M3 would have to add, and this is it: a job can end because the
    /// machine it was on stopped answering, which is neither an exit code,
    /// nor a signal, nor a failure to start.
    Lost { reason: String },
}

/// Which cap a runner enforced against a job.
///
/// A new cap is a new variant here rather than a new [`JobOutcome`] variant,
/// because every cap ends a job the same way — the runner killed it — and
/// only the figure and the sentence differ. An unknown variant is a decode
/// error naming it, never a job silently read as having exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    /// The job ran longer than the time cap. Its limit is in seconds.
    Time,
    /// A job process allocated past the memory ceiling. Its limit is in bytes.
    Memory,
    /// The job wrote more log than the cap allows. Its limit is in bytes.
    Log,
    /// The job's workspace grew past its disk budget. Its limit is in bytes.
    Disk,
}

impl Cap {
    /// The cap's name, as it reads in a sentence and on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Cap::Time => "time",
            Cap::Memory => "memory",
            Cap::Log => "log",
            Cap::Disk => "disk",
        }
    }

    /// The unit the cap's limit is expressed in.
    pub fn unit(self) -> &'static str {
        match self {
            Cap::Time => "s",
            Cap::Memory | Cap::Log | Cap::Disk => "bytes",
        }
    }
}

/// What a runner measured while a job ran, reported with the outcome and kept
/// in the job's record (E-15).
///
/// A figure a host does not expose is absent, never zero: nobody may read
/// "0 bytes written" as a measurement that was taken. The two figures the
/// runner always knows itself are not optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobMetrics {
    /// The host's one-minute load average when the job ended, times 1000.
    /// An integer on purpose: it lands in a record a consumer compares, and
    /// a float would print two equal measurements differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_load_milli: Option<u64>,
    /// The most memory the job's processes held at once, in bytes. Read from
    /// the job's cgroup where there is one, and otherwise sampled from the
    /// job's own process, which undercounts a process tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_memory_bytes: Option<u64>,
    /// The high-water mark of the job's workspace, in bytes: the most the job
    /// had written and not deleted. This is the figure the disk budget is
    /// enforced against, not a block-layer counter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<u64>,
    /// Bytes of log the job produced, as the runner counted them.
    pub log_bytes: u64,
    /// Wall-clock milliseconds from the spawn to the outcome.
    pub wall_time_ms: u64,
}

/// A job's identity, minted by the hub.
///
/// Opaque on purpose: M1 numbers jobs in memory, M2 makes each one a directory
/// on disk, and no peer may depend on the shape of the string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One job a runner is executing, as its health report names it.
///
/// `chunks_sent` is how many chunks of that job's output the runner has handed
/// to its link since the attempt began — and therefore the number the hub's
/// own count has to agree with. The hub writes the chunks it receives and
/// counts them; when the runner says it has sent more than the hub has
/// written, output is missing, and the hub can ask for it back instead of
/// learning of the loss only when a later chunk arrives with a number that
/// does not follow. That late discovery is the one case M2 could not close:
/// chunks lost with nothing after them look exactly like a job gone quiet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningJob {
    pub job_id: JobId,
    /// Which attempt of that job this runner is executing.
    pub attempt: u32,
    pub chunks_sent: u64,
}

/// Sent to the hub by a runner or a requester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToHub {
    /// First frame on every connection, from either role.
    Hello {
        protocol_version: u32,
        role: Role,
        /// Bearer token. Carried from M1, validated from M15 (D6, Q8).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        /// Human-readable name of the peer, for logs and reports.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Requester asks for work to be done.
    SubmitJob { command: Vec<String> },
    /// Requester asks to be told how a job that already exists ends.
    ///
    /// This is what makes a submission detachable: `herdctl submit` leaves
    /// with the id and nothing else, and a later `herdctl wait <id>` — or
    /// another machine, or the same script an hour afterwards — attaches to the
    /// job by its id alone. A job that is already over is answered at once
    /// from the hub's record, which is why the hub's state has to be durable
    /// for this message to mean anything.
    WaitForJob { job_id: JobId },
    /// Runner reports that it has spawned the job's process.
    JobStarted { job_id: JobId, attempt: u32 },
    /// Runner reports output. `seq` counts chunks per job, across both
    /// streams, from 0, and the runner numbers a chunk as it enqueues it, so
    /// the numbers arrive in the order they were given out.
    ///
    /// What `seq` is for is **gap detection**, not reordering: the hub knows
    /// the number of the last chunk it wrote, and a number that is not its
    /// successor means the chunks in between never arrived — a control
    /// connection that dropped, most often. The hub says so in the job's log
    /// rather than leaving the log quietly short (D29). The single `log` file
    /// is arrival-ordered and promises nothing about which stream a byte came
    /// from, so nothing anywhere reorders by `seq`.
    ///
    /// `data` is the bytes the process wrote, unaltered (D18).
    LogChunk {
        job_id: JobId,
        /// Which attempt produced it. A job can be attempted twice (E-21) and
        /// the second attempt's output is not the first's, so a chunk whose
        /// attempt is over is refused by name rather than written into the
        /// record of the attempt that is current.
        attempt: u32,
        stream: LogStream,
        seq: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Runner reports how the job ended, how much output that attempt
    /// produced in all, and what it measured while it ran.
    ///
    /// `chunks_sent` is what closes the lost-tail case: when the job ends the
    /// hub learns the exact number of chunks the attempt wrote, so a log that
    /// is short says so at once instead of waiting for a chunk that is never
    /// coming. The hub asks for the missing ones back before it records the
    /// end, and names them in the log only once they are truly gone.
    JobFinished {
        job_id: JobId,
        attempt: u32,
        outcome: JobOutcome,
        chunks_sent: u64,
        /// Default for a peer that measured nothing; never silently zero for
        /// a figure that was simply not readable (see [`JobMetrics`]).
        #[serde(default)]
        metrics: JobMetrics,
    },
    /// Runner reports that it is alive and what it is doing (E-40).
    ///
    /// Sent about every 15 s, and the mechanism everything about a lost runner
    /// rests on: the hub keeps the age of the last report, a runner silent for
    /// about 60 s is lost and its running jobs are requeued once (E-21), and
    /// the age is what M13 surfaces. `draining` says the runner is stopping
    /// and takes no new job (invariant 2), so the hub stops handing work to a
    /// runner on its way out the moment it says so.
    Health {
        jobs: Vec<RunningJob>,
        draining: bool,
    },
    /// Runner gives an attempt back without a verdict, and says why.
    ///
    /// Not an outcome: the job did not end, this *attempt* did. A runner asked
    /// to stop while a job is running abandons it rather than inventing a
    /// verdict for it (invariant 2), and a runner handed a job while it is
    /// draining abandons it before it starts. What becomes of the job — one
    /// more attempt, or an end — is the hub's policy and never the runner's.
    AttemptAbandoned {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// Requester asks for a job to be cancelled (E-06).
    ///
    /// The hub propagates it to whichever runner holds the attempt; a
    /// requester never names a machine (invariant 6).
    CancelJob { job_id: JobId },
}

/// Sent by the hub to a runner or a requester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromHub {
    /// The handshake succeeded.
    Welcome {
        protocol_version: u32,
        hub_version: String,
    },
    /// The hub declines to go on, and says why. Always the last frame.
    Refused { reason: String },
    /// The submitted job exists and will be followed on this connection.
    JobAccepted { job_id: JobId },
    /// The runner is asked to execute this job, as attempt `attempt` of it.
    ///
    /// The number is the hub's, minted when the attempt is recorded, and the
    /// runner carries it on everything it says about that job afterwards. It
    /// is what lets the hub tell a frame of the attempt it is waiting for from
    /// a frame of an attempt it has already given up on (E-21).
    AssignJob {
        job_id: JobId,
        attempt: u32,
        command: Vec<String>,
    },
    /// The runner is asked to end a job it is executing, and told why.
    ///
    /// A requester asked for it (E-06), or the hub has decided this attempt is
    /// over. Scoped to one attempt of one job: a sibling job on the same
    /// runner is not touched by it.
    CancelJob {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// The runner said something about an attempt that is over, and nothing of
    /// it was recorded.
    ///
    /// A runner that was gone long enough to be declared lost comes back still
    /// holding the frames of its attempt — a result, output, the lot — and the
    /// job it was running may already be on its second attempt. None of that
    /// may touch the record (E-21), and the runner may not be left guessing
    /// why nothing happens: it is told by name, and it ends whatever it is
    /// still doing for that attempt. Not a refusal — `Refused` is terminal
    /// (D13) and this connection goes on.
    StaleAttempt {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// Every chunk of this job's output numbered below `through_seq` is on the
    /// hub's disk, and the runner may forget it.
    ///
    /// The same promise as [`FromHub::JobResultAck`], for output: a runner
    /// holds the chunks it has sent until they are recorded, so a link that
    /// dies under a running job costs the log nothing. Sent after the chunk is
    /// appended, never before.
    LogAck {
        job_id: JobId,
        attempt: u32,
        through_seq: u64,
    },
    /// The hub is missing this job's output from `from_seq` onwards, and asks
    /// the runner to send it again.
    ///
    /// What the runner still holds it resends under the numbers it first gave
    /// them; what it no longer holds is genuinely gone, and the hub then names
    /// it in the log rather than leaving the log quietly short (D29).
    ResendLog {
        job_id: JobId,
        attempt: u32,
        from_seq: u64,
    },
    /// Not now, and not never: dial again in `retry_in_ms`.
    ///
    /// A runner redialling into a hub that has not yet noticed its previous
    /// connection is gone used to be told "a runner is already connected" and,
    /// `Refused` being terminal (D13), died of it. So that case is not a
    /// refusal at all: the hub says stand by, names what it is waiting for,
    /// and the runner comes back. D13's own warning, honoured — a peer that
    /// waits is not a peer that was refused.
    StandBy { reason: String, retry_in_ms: u64 },
    /// The hub has the runner's health report, and is itself still there.
    ///
    /// The answer matters as much as the report: a TCP connection can stay
    /// open to a hub that is gone, and a runner that only ever writes would
    /// never find out. Hearing nothing at all from the hub for as long as the
    /// hub would take to call the *runner* lost is a dead link, and the runner
    /// redials.
    HealthRecorded {},
    /// Output relayed to the requester that submitted the job, as bytes
    /// (D18). `seq` is the runner's own numbering, carried through for the
    /// same reason as in [`ToHub::LogChunk`]: telling a gap from a reordering.
    LogChunk {
        job_id: JobId,
        stream: LogStream,
        seq: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// The output a job had already produced when a requester attached to it.
    ///
    /// `herdctl wait` attaches to a job by its id alone, often long after the
    /// job ran, so the stream it would have followed is over. This frame is
    /// what the hub has on its disk: both streams in the order they arrived,
    /// which is how the `log` file keeps them, so the split between stdout and
    /// stderr is not in it. Sent once, before anything else about that job
    /// (D22).
    JobLog {
        job_id: JobId,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Something a requester following a job should know, on a connection that
    /// stays open.
    ///
    /// Not a refusal: `Refused` is terminal (D13), and a note is the opposite
    /// — the job is still there and this connection goes on. M2 sends a note
    /// for each of the two ways a job that is waited for can be going
    /// nowhere: an attempt that was interrupted and whose runner has not come
    /// back (D17), and an attempt whose end the hub cannot write to its disk
    /// (D19). Waiting for such a job is never silent. M8's explained waits are
    /// this frame's real home.
    JobNote { job_id: JobId, note: String },
    /// How the job ended, relayed to the requester that submitted it, with
    /// what the runner measured while it ran.
    JobFinished {
        job_id: JobId,
        outcome: JobOutcome,
        #[serde(default)]
        metrics: JobMetrics,
    },
    /// The hub has the job's outcome on its disk, and the runner may forget it.
    ///
    /// A result is the one thing a runner cannot simply resend from its
    /// memory of the world: the process is gone, so if the result is lost the
    /// work is lost. A runner therefore holds a finished job's outcome until
    /// this frame arrives and redelivers it otherwise (E-44). The hub sends it
    /// only after the record is written and fsynced, never before: an
    /// acknowledgement of something still in memory would be a lie the next
    /// power cut collects on.
    JobResultAck { job_id: JobId },
}

/// The one sentence a cap refusal reads as, so that the runner, the record
/// and `herdctl` all say the same thing and a test can rely on it. It names
/// the cap and its figure, as every Herd refusal must.
pub fn cap_exceeded_reason(cap: Cap, limit: u64) -> String {
    format!(
        "the runner stopped this job at its {} cap of {limit} {}",
        cap.name(),
        cap.unit()
    )
}

/// The same sentence, with the reason a cap carries when its figure alone
/// would mislead ([`JobOutcome::StoppedByCap`]'s `detail`).
///
/// Without a detail this is exactly [`cap_exceeded_reason`]. With one it says
/// the cap could not be *held to* rather than that the job reached it, so
/// nobody reads a job that wrote four kilobytes as a job that filled a disk.
pub fn cap_stop_reason(cap: Cap, limit: u64, detail: Option<&str>) -> String {
    match detail {
        None => cap_exceeded_reason(cap, limit),
        Some(detail) => format!(
            "the runner stopped this job because it could not hold it to its {} cap of \
             {limit} {}: {detail}",
            cap.name(),
            cap.unit()
        ),
    }
}

/// The wording of a version refusal, so that every peer reads one sentence and
/// a test can rely on it. Names both versions, as the handshake must.
pub fn version_mismatch_reason(announced: u32) -> String {
    format!(
        "protocol version mismatch: the hub speaks version {PROTOCOL_VERSION}, \
         the peer announced version {announced}"
    )
}

/// Encode one message as the text of one WebSocket frame.
pub fn to_frame<M: Serialize>(message: &M) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

/// Decode the text of one WebSocket frame into one message.
pub fn from_frame<M: DeserializeOwned>(frame: &str) -> Result<M, serde_json::Error> {
    serde_json::from_str(frame)
}
