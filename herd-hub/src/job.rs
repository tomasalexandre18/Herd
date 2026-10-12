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

/// What a job cost the machine it ran on.
///
/// The fields are part of the record's shape from M2 so that the layout the
/// consumer reads does not change when M4 starts filling them; M2 collects
/// nothing, so they are written as nulls and mean "not measured".
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JobMetrics {
    /// The runner's load average just before the job started.
    pub load_before: Option<f64>,
    /// The runner's load average just after it ended.
    pub load_after: Option<f64>,
    /// The highest memory the job's process tree held, in bytes.
    pub memory_peak_bytes: Option<u64>,
    /// How much the job wrote under the runner's data root, in bytes.
    pub bytes_written: Option<u64>,
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
    /// Filled in by M4. Present and unset here on purpose.
    pub metrics: JobMetrics,
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

    /// The state the job moved to, when this line is a transition, and `None`
    /// when it records something else.
    pub fn state(&self) -> Option<&JobState> {
        match &self.what {
            JobEventKind::Transition { state } => Some(state),
            JobEventKind::LogGap { .. } => None,
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
    /// Chunks of the job's output never made it into the log, and the log
    /// names the gap at the point where they are missing.
    ///
    /// Every chunk carries a per-job sequence number (D4), which is there for
    /// exactly this: a number that is not the successor of the last one the
    /// hub wrote means the chunks in between are gone — the control
    /// connection dropped under a running job, or the hub could not write
    /// one. Getting them back is M3's (the runner would have to hold its
    /// output as it holds a result); saying they are missing is M2's, because
    /// a silently short log is worse than a short log that says so.
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

/// Everything the hub does to a job's state.
///
/// Deliberately boring and synchronous: the writes are small files, and the
/// hub's correctness depends on a transition being on the disk before the
/// hub acts on it — an acknowledgement to a runner, above all.
pub trait JobStore: Send + Sync + 'static {
    /// Mint a job, assign its id, and record it as [`JobState::Queued`].
    fn create_job(&self, command: Vec<String>) -> Result<JobRecord>;

    /// Record a transition, with the moment it happened. Unknown ids are an
    /// error, not a silent no-op. Reporting the same end twice is accepted:
    /// that is a runner redelivering a result whose acknowledgement it never
    /// saw, not a second ending.
    fn record_state(&self, id: &JobId, state: JobState) -> Result<()>;

    /// Append one chunk of a job's output in the order the runner produced it,
    /// as the bytes the process wrote (D18): a job's output is not text.
    ///
    /// `seq` is the runner's per-job chunk number, and the store uses it to
    /// notice that chunks are missing: a number that is not the successor of
    /// the last one written leaves a named gap in the log and a
    /// [`JobEventKind::LogGap`] on the record. A store that threw `seq` away
    /// would make a quarter of a log disappear in silence.
    fn append_log(&self, id: &JobId, stream: LogStream, seq: u64, data: &[u8]) -> Result<()>;

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
