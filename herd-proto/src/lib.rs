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
    JobStarted { job_id: JobId },
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
        stream: LogStream,
        seq: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Runner reports how the job ended.
    JobFinished { job_id: JobId, outcome: JobOutcome },
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
    /// The runner is asked to execute this job.
    AssignJob { job_id: JobId, command: Vec<String> },
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
    /// How the job ended, relayed to the requester that submitted it.
    JobFinished { job_id: JobId, outcome: JobOutcome },
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
