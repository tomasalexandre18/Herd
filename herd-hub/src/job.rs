//! The job lifecycle, behind one small interface.
//!
//! M1 keeps jobs in memory. M2 puts them on disk — one directory per job,
//! `job.json` by atomic rename, `events.jsonl` appended, the log as a file
//! (D8) — by writing a second [`JobStore`] and changing the line in
//! `main.rs` that builds one. Nothing in the server knows which it got.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Mutex;

use herd_proto::{JobId, LogStream};

/// Where a job is in its life. M2 gives each transition a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Accepted by the hub, not yet handed to a runner.
    Queued,
    /// Sent to a runner, which has not reported starting it yet.
    Assigned,
    /// The runner reported that the process is running.
    Running,
    /// The runner reported the outcome.
    Finished { exit_code: i32 },
}

/// What the hub knows about one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: JobId,
    pub command: Vec<String>,
    pub state: JobState,
}

/// A job store failed. In M1 only a poisoned lock can cause one; from M2 this
/// carries filesystem failures, which is why the interface is fallible.
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

/// Everything the hub does to a job's state. Deliberately boring and
/// synchronous: an implementation that writes small files can do it inline.
pub trait JobStore: Send + Sync + 'static {
    /// Mint a job, assign its id, and record it as [`JobState::Queued`].
    fn create_job(&self, command: Vec<String>) -> Result<JobRecord>;

    /// Record a transition. Unknown ids are an error, not a silent no-op.
    fn record_state(&self, id: &JobId, state: JobState) -> Result<()>;

    /// Append one chunk of a job's output in the order the runner produced it.
    fn append_log(&self, id: &JobId, stream: LogStream, seq: u64, data: &str) -> Result<()>;

    /// The job's record, if the store has one.
    fn job(&self, id: &JobId) -> Result<Option<JobRecord>>;

    /// The job's output so far, both streams interleaved as they arrived.
    fn job_log(&self, id: &JobId) -> Result<Option<String>>;
}

/// The M1 store: everything in memory, lost on restart. M2 replaces it.
#[derive(Debug, Default)]
pub struct MemoryJobStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    minted: u64,
    jobs: HashMap<JobId, StoredJob>,
}

#[derive(Debug)]
struct StoredJob {
    record: JobRecord,
    log: String,
}

impl MemoryJobStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_inner<T>(&self, f: impl FnOnce(&mut Inner) -> Result<T>) -> Result<T> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| StoreError::new("the job store lock is poisoned"))?;
        f(&mut inner)
    }
}

impl JobStore for MemoryJobStore {
    fn create_job(&self, command: Vec<String>) -> Result<JobRecord> {
        self.with_inner(|inner| {
            inner.minted += 1;
            let record = JobRecord {
                id: JobId::new(format!("j{}", inner.minted)),
                command,
                state: JobState::Queued,
            };
            inner.jobs.insert(
                record.id.clone(),
                StoredJob {
                    record: record.clone(),
                    log: String::new(),
                },
            );
            Ok(record)
        })
    }

    fn record_state(&self, id: &JobId, state: JobState) -> Result<()> {
        self.with_inner(|inner| match inner.jobs.get_mut(id) {
            Some(job) => {
                job.record.state = state;
                Ok(())
            }
            None => Err(StoreError::new(format!("no such job: {id}"))),
        })
    }

    fn append_log(&self, id: &JobId, _stream: LogStream, _seq: u64, data: &str) -> Result<()> {
        self.with_inner(|inner| match inner.jobs.get_mut(id) {
            Some(job) => {
                job.log.push_str(data);
                Ok(())
            }
            None => Err(StoreError::new(format!("no such job: {id}"))),
        })
    }

    fn job(&self, id: &JobId) -> Result<Option<JobRecord>> {
        self.with_inner(|inner| Ok(inner.jobs.get(id).map(|job| job.record.clone())))
    }

    fn job_log(&self, id: &JobId) -> Result<Option<String>> {
        self.with_inner(|inner| Ok(inner.jobs.get(id).map(|job| job.log.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_job_is_queued_and_has_a_fresh_id() {
        let store = MemoryJobStore::new();
        let first = store.create_job(vec!["true".to_owned()]).expect("minted");
        let second = store.create_job(vec!["false".to_owned()]).expect("minted");
        assert_ne!(first.id, second.id);
        assert_eq!(first.state, JobState::Queued);
        assert_eq!(
            store.job(&first.id).expect("readable"),
            Some(first.clone()),
            "a minted job is readable straight away"
        );
    }

    #[test]
    fn transitions_and_log_chunks_land_on_the_record() {
        let store = MemoryJobStore::new();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        store
            .record_state(&job.id, JobState::Running)
            .expect("transition");
        store
            .append_log(&job.id, LogStream::Stdout, 0, "first")
            .expect("append");
        store
            .append_log(&job.id, LogStream::Stderr, 1, "-second")
            .expect("append");
        store
            .record_state(&job.id, JobState::Finished { exit_code: 3 })
            .expect("transition");

        let record = store.job(&job.id).expect("readable").expect("still there");
        assert_eq!(record.state, JobState::Finished { exit_code: 3 });
        assert_eq!(
            store.job_log(&job.id).expect("readable").as_deref(),
            Some("first-second"),
            "both streams interleave in arrival order"
        );
    }

    #[test]
    fn an_unknown_job_is_an_error_not_a_silent_no_op() {
        let store = MemoryJobStore::new();
        let ghost = JobId::new("j404");
        assert!(store.record_state(&ghost, JobState::Running).is_err());
        assert!(store.append_log(&ghost, LogStream::Stdout, 0, "x").is_err());
        assert_eq!(store.job(&ghost).expect("readable"), None);
    }
}
