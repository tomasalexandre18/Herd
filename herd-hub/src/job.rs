//! What the hub records about a job, and the interface it records it through.
//!
//! These types are the shape of the files in a job's directory (D8), so they
//! are a compatibility surface: the first consumer reads a record while the
//! job runs (E-53) and re-judges it much later (E-54). Every file that carries
//! one of them carries [`RECORD_VERSION`] beside it, and a record written by a
//! newer Herd is refused by name rather than read as if it were this one.

use std::error::Error;
use std::fmt;

use herd_proto::{JobId, JobOutcome, LogStream};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a runner measured while a job ran.
///
/// There is one such type and it lives in `herd-proto` (invariant 4): the
/// figures travel from the runner to the hub in [`herd_proto::ToHub::JobFinished`]
/// and are written to the record unchanged. M2 reserved the field in the
/// record's shape and measured nothing; M4 fills it. A figure a host does not
/// expose stays `None` — never zero (D23).
pub use herd_proto::JobMetrics;

/// The version of the shape of a job's recorded data: `job.json` and every
/// line of `events.jsonl`. Bumped whenever an older reader could not read
/// what a newer Herd writes.
pub const RECORD_VERSION: u32 = 1;

/// A moment, as the record files carry it: RFC 3339 in UTC, to the
/// nanosecond. A string a human and the consumer can both read, so that a
/// record stays meaningful with no tool but `cat`.
///
/// Wall-clock time can step backwards, so the *order* of a job's transitions
/// is the order of the lines in `events.jsonl`, never a comparison of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self
            .0
            .format(&time::format_description::well_known::Rfc3339)
        {
            Ok(text) => f.write_str(&text),
            Err(_) => f.write_str("unformattable"),
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        time::serde::rfc3339::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        time::serde::rfc3339::deserialize(deserializer).map(Self)
    }
}

/// Where a job is in its life.
///
/// Tagged like a protocol message, and for the same reason: a later milestone
/// adds a state (M3's requeued attempt, M8's admission wait, M9's
/// cancellation) by adding a variant, and a state this Herd does not know is a
/// read error naming it rather than a job quietly read as something else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobState {
    /// Accepted by the hub, not yet handed to a runner.
    Queued,
    /// Sent to a runner, which has not reported starting it yet.
    Assigned,
    /// The runner reported that the process is running.
    Running,
    /// An attempt was under way and this hub stopped seeing it: the hub was
    /// killed or restarted while the job was assigned or running.
    ///
    /// Not terminal. The runner that holds the result may come back and
    /// redeliver it, which resolves the record normally (E-44); until then the
    /// job is unfinished, and what this state forbids is a job sitting in
    /// `running` for ever with nothing on its record saying why (D17). Deciding
    /// that the attempt is lost and requeueing it once is M3's.
    Interrupted { reason: String },
    /// The runner reported how the job ended. The outcome is kept whole, not
    /// flattened to a number: a persisted number would have to be migrated the
    /// first time a job ends some other way (D15).
    Finished { outcome: JobOutcome },
}

impl JobState {
    /// Whether the job is over. A terminal state is the last one a record ever
    /// has, which is what lets the store stop tracking it.
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Finished { .. })
    }

    /// The state's name as the record files spell it.
    pub fn name(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Assigned => "assigned",
            JobState::Running => "running",
            JobState::Interrupted { .. } => "interrupted",
            JobState::Finished { .. } => "finished",
        }
    }
}

/// One go at running a job on one runner.
///
/// A job is attempted once, and — when the runner it was on is lost — once
/// more (E-21). Both attempts stay on the record: a reader has to be able to
/// see that there was a first one, which runner it was on, and why it ended,
/// because "this job ran twice and the first machine died" is the single most
/// useful thing to know about a job that took twice as long as it should.
///
/// `ended` is the sentence that answers *why*, and it is set only when the
/// attempt ended without producing the job's verdict — lost, abandoned,
/// superseded. An attempt that produced the verdict needs no sentence: the
/// job's outcome is the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    /// 1 for the first, 2 for the one after a loss. Also what the runner
    /// carries on every frame about the job, so a late frame of attempt 1 is
    /// recognisable as late.
    pub number: u32,
    /// The runner the attempt was handed to, by the name it announced.
    pub runner: String,
    pub started_at: Timestamp,
    /// When this attempt stopped being the one the hub is waiting for,
    /// whichever way it ended: lost, abandoned, superseded — or finished,
    /// which is the ordinary way and was the one that used to be left unset.
    /// `None` means the attempt is under way, and nothing else does.
    pub ended_at: Option<Timestamp>,
    /// Why this attempt ended without a verdict, when that is what happened.
    /// `None` on the attempt that produced one: the job's outcome is the
    /// answer, so there is nothing to excuse (see [`Attempt::ended_at`] for
    /// the moment, which is set either way).
    pub ended: Option<String>,
}

/// What the hub knows about one job: the contents of its `job.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub command: Vec<String>,
    pub state: JobState,
    /// When the hub minted the job.
    pub submitted_at: Timestamp,
    /// When its state last changed.
    pub updated_at: Timestamp,
    /// Every go at running this job, in order (E-21). Empty while the job is
    /// only queued, one entry in the ordinary case, two when the first
    /// runner was lost.
    ///
    /// `#[serde(default)]`, so a record written before M3 reads as a job with
    /// no attempt recorded rather than as a read error. [`RECORD_VERSION`]
    /// stays 1 for the same reason D15 and D18 kept `PROTOCOL_VERSION` at 1:
    /// the field is purely additive, an older reader ignores it, and nothing
    /// is deployed to read it wrongly.
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    /// What the runner measured while the job ran (E-15). Reserved in the
    /// record's shape by M2 and filled by M4: every figure is unset until the
    /// runner reports it, which it does once, with the outcome.
    pub metrics: JobMetrics,
}

impl JobRecord {
    /// The attempt that is current, if one is.
    pub fn attempt(&self) -> Option<&Attempt> {
        self.attempts.last()
    }

    /// The number of the attempt that is current: 0 before the first one, so
    /// that "which attempt is this frame about" always has an answer.
    pub fn attempt_number(&self) -> u32 {
        self.attempts.last().map_or(0, |attempt| attempt.number)
    }

    /// Whether a frame a runner sent about `attempt` concerns the attempt the
    /// hub is waiting for.
    ///
    /// False for an attempt that has been superseded by a later one, and false
    /// for one that has ended — a runner declared lost and come back may not
    /// finish what the hub has already written off (E-21), and neither may
    /// anybody go on talking about an attempt that produced its job's verdict.
    ///
    /// What is tested is [`Attempt::ended_at`] and not [`Attempt::ended`].
    /// `ended` is a sentence, and the attempt that produces a verdict has
    /// nothing to excuse — so testing it made every normally-finished job's
    /// attempt read as current for ever, which is how a late frame bearing
    /// the *correct* runner name got through the ownership gate.
    pub fn attempt_is_current(&self, attempt: u32) -> bool {
        self.attempts
            .last()
            .is_some_and(|current| current.number == attempt && current.ended_at.is_none())
    }
}

/// One line of `events.jsonl`: a moment, and what happened at it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobEvent {
    pub at: Timestamp,
    #[serde(flatten)]
    pub what: JobEventKind,
}

impl JobEvent {
    /// A transition, which is what most lines are.
    pub fn transition(at: Timestamp, state: JobState) -> Self {
        Self {
            at,
            what: JobEventKind::Transition { state },
        }
    }

    /// Output that is missing from the job's log, named on the record.
    pub fn log_gap(at: Timestamp, first_missing: u64, last_missing: u64) -> Self {
        Self {
            at,
            what: JobEventKind::LogGap {
                first_missing,
                last_missing,
            },
        }
    }

    /// An attempt handed to a runner.
    pub fn attempt_started(at: Timestamp, attempt: u32, runner: &str) -> Self {
        Self {
            at,
            what: JobEventKind::AttemptStarted {
                attempt,
                runner: runner.to_owned(),
            },
        }
    }

    /// An attempt that ended without producing the job's verdict, and why.
    pub fn attempt_ended(at: Timestamp, attempt: u32, reason: &str) -> Self {
        Self {
            at,
            what: JobEventKind::AttemptEnded {
                attempt,
                reason: reason.to_owned(),
            },
        }
    }

    /// The state the job moved to, when this line is a transition, and `None`
    /// when it records something else.
    pub fn state(&self) -> Option<&JobState> {
        match &self.what {
            JobEventKind::Transition { state } => Some(state),
            JobEventKind::LogGap { .. }
            | JobEventKind::AttemptStarted { .. }
            | JobEventKind::AttemptEnded { .. } => None,
        }
    }
}

/// What one line of `events.jsonl` records.
///
/// A job's history is not only its transitions. Output that never reached the
/// hub is part of what happened to the job, and a log that is a quarter short
/// with nothing on the record saying so is a record that lies by omission. So
/// a line says which kind it is, in a `event` field, and a reader that wants
/// the transitions reads the `transition` lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEventKind {
    /// The job moved to a new state.
    Transition { state: JobState },
    /// The job was handed to a runner, as this attempt of it (E-21).
    AttemptStarted { attempt: u32, runner: String },
    /// That attempt ended without the job's verdict, and this is why: the
    /// runner went silent, it was asked to stop, the hub gave the attempt up.
    /// The job itself may go on — requeued once — or end here; the transition
    /// line that follows says which.
    AttemptEnded { attempt: u32, reason: String },
    /// Chunks of the job's output never made it into the log, and the log
    /// names the gap at the point where they are missing.
    ///
    /// Every chunk carries a per-job sequence number (D4), which is there for
    /// exactly this: a number that is not the successor of the last one the
    /// hub wrote means the chunks in between are gone — the control
    /// connection dropped under a running job, or the hub could not write
    /// one. M3 holds the chunks on the runner until the hub acknowledges
    /// them and asks for them again, so a link that drops costs the log
    /// nothing; this line is for what is missing even so — chunks the runner
    /// no longer holds, or a runner that never came back. A silently short
    /// log is worse than a short log that says so.
    LogGap {
        /// The number of the first chunk that is missing.
        first_missing: u64,
        /// The number of the last one, inclusive.
        last_missing: u64,
    },
}

/// A job store failed, with the path or the id that failed in the message: the
/// hub's state is files, and a filesystem failure that does not name what it
/// was touching cannot be acted on.
#[derive(Debug)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for StoreError {}

pub type Result<T> = std::result::Result<T, StoreError>;

/// What became of a log chunk the store was handed.
///
/// A runner holds the chunks it has sent until the hub acknowledges them, so
/// the hub does receive the same chunk twice: once on the link that died
/// before the acknowledgement got out, once on the link that came after. The
/// second one must not be appended — a log with a paragraph written twice is
/// as wrong as a log with a paragraph missing — and the hub must not relay it
/// to a requester that already saw it either, which is why the answer comes
/// back rather than being swallowed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogWrite {
    /// New output, now in the log.
    Recorded,
    /// The store already had this chunk, and nothing was written.
    AlreadyRecorded,
}

/// Everything the hub does to a job's state.
///
/// Deliberately boring and synchronous: the writes are small files, and the
/// hub's correctness depends on a transition being on the disk before the
/// hub acts on it — an acknowledgement to a runner, above all.
/// **Every writer here refuses a job that is over.** A terminal record is the
/// last thing a job ever has — its outcome, its measurements and its log are
/// closed together — so `record_state`, `record_metrics`, `begin_attempt`,
/// `end_attempt`, `append_log` and `name_missing_log` all answer a finished
/// job with an error naming it, and the hub refuses such a frame by name
/// before it ever gets here. What is terminal is the *job*: a first attempt
/// ending is how a requeue begins (E-21), and that record is `queued` a
/// moment later.
pub trait JobStore: Send + Sync + 'static {
    /// Who the state this store keeps belongs to, as the handshake carries it
    /// (see `JobDirStore::state_root_id`).
    ///
    /// On the trait because the hub puts it in `Welcome`, and because a job id
    /// only means anything inside one state: `j0000000001` of a root that was
    /// recreated is a different job with the same name. A store with no
    /// durable identity of its own answers with something stable for its own
    /// lifetime; what it must never do is answer with a string another store
    /// could also answer with.
    fn state_root_id(&self) -> &str;

    /// Mint a job, assign its id, and record it as [`JobState::Queued`].
    fn create_job(&self, command: Vec<String>) -> Result<JobRecord>;

    /// Record a transition, with the moment it happened. Unknown ids are an
    /// error, not a silent no-op, and so is a job that is already over — a
    /// runner redelivering a result whose acknowledgement it never saw is
    /// told by name that the record holds its end already, and drops it
    /// (D42), rather than writing the same ending twice.
    fn record_state(&self, id: &JobId, state: JobState) -> Result<()>;

    /// Record what the runner measured (E-15). Separate from the transition
    /// because a runner reports figures the hub never computes, and because a
    /// store that keeps no measurements is still a correct store.
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> Result<()>;

    /// Record the start of an attempt on a job, and give it its number.
    ///
    /// One call so that the attempt, the transition to
    /// [`JobState::Assigned`] and the reset of the job's chunk count are one
    /// step: a second attempt numbers its output from zero again, and a hub
    /// that had recorded the attempt without resetting the count would read
    /// the new attempt's first chunk as a gap.
    fn begin_attempt(&self, id: &JobId, runner: &str) -> Result<u32>;

    /// Record that an attempt ended without the job's verdict, and why.
    ///
    /// Idempotent: ending an attempt that is already ended keeps the first
    /// reason, because the first reason is the true one and a record is not a
    /// journal of the hub's second thoughts.
    fn end_attempt(&self, id: &JobId, attempt: u32, reason: &str) -> Result<()>;

    /// Append one chunk of a job's output in the order the runner produced it,
    /// as the bytes the process wrote (D18): a job's output is not text.
    ///
    /// `seq` is the runner's per-attempt chunk number, and the store uses it
    /// for two things. A number *above* the one expected means the chunks in
    /// between are missing, so the log gets a named gap and the record a
    /// [`JobEventKind::LogGap`]. A number *below* it is a chunk the store has
    /// already written — a runner resending what it was never acknowledged
    /// for — and writing it twice would corrupt the log with duplicated
    /// output, so it is reported as [`LogWrite::AlreadyRecorded`] and
    /// dropped.
    fn append_log(&self, id: &JobId, stream: LogStream, seq: u64, data: &[u8]) -> Result<LogWrite>;

    /// How many chunks of this job's output the store has written for the
    /// attempt under way: the number the next chunk should carry.
    ///
    /// What the hub compares against the runner's own count, so that output
    /// lost with nothing behind it is still noticed (E-40's health report
    /// carries the runner's number).
    fn chunks_recorded(&self, id: &JobId) -> Result<u64>;

    /// Name output that is missing and is not coming back, at the point in
    /// the log where it should have been.
    ///
    /// The hub calls this once it knows the chunks are gone for good: the
    /// runner was asked for them and does not have them, or it is never
    /// coming back. Up to that point a short log is a log waiting for a
    /// resend, not a log with a hole in it, and claiming a gap that then
    /// fills in would be its own kind of lie.
    fn name_missing_log(&self, id: &JobId, first_missing: u64, last_missing: u64) -> Result<()>;

    /// The job's record, if the store has one.
    fn job(&self, id: &JobId) -> Result<Option<JobRecord>>;

    /// The job's output so far, both streams interleaved as they arrived, and
    /// byte for byte what the process wrote.
    fn job_log(&self, id: &JobId) -> Result<Option<Vec<u8>>>;

    /// The job's transitions, in the order they were recorded.
    fn job_events(&self, id: &JobId) -> Result<Option<Vec<JobEvent>>>;

    /// Every job that has not reached a terminal state, oldest first. This is
    /// the queue: after a restart it is what the hub has left to do.
    fn unfinished_jobs(&self) -> Result<Vec<JobRecord>>;
}
