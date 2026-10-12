//! The hub's state, as files: one directory per job (D8).
//!
//! ```text
//! <state root>/
//!   store.json              the layout's version; rewritten at every start
//!   jobs/
//!     j0000000001/
//!       job.json            the record: fields, current state, timestamps
//!       events.jsonl         one transition per line, in the order they happened
//!       log                 the job's output, both streams in arrival order
//!       files/              room for what a job keeps (M11's artefacts)
//!   index/
//!     state.json            derived: which jobs are not over yet
//! ```
//!
//! **The job directories are the only source of truth.** `index/state.json`
//! exists so that starting the hub does not have to parse the whole history:
//! it lists the jobs that have not reached a terminal state. It is derived and
//! rebuildable, and the hub checks it rather than believing it — see
//! [`JobDirStore::open`]. Every write goes through [`crate::durable`]; this
//! module never touches `std::fs` itself.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use herd_proto::{JobId, LogStream};
use serde::{Deserialize, Serialize};

use crate::durable;
use crate::job::{
    JobEvent, JobMetrics, JobRecord, JobState, JobStore, Result, StoreError, Timestamp,
    RECORD_VERSION,
};

/// The version of the directory layout above. A state root written by a newer
/// Herd is refused rather than read as if it were this one.
pub const LAYOUT_VERSION: u32 = 1;

/// The version of the derived index. It needs no compatibility: a version this
/// Herd does not know is one more reason to rebuild it.
const INDEX_VERSION: u32 = 1;

const STORE_FILE: &str = "store.json";
const JOBS_DIR: &str = "jobs";
const INDEX_DIR: &str = "index";
const INDEX_FILE: &str = "state.json";
const RECORD_FILE: &str = "job.json";
const EVENTS_FILE: &str = "events.jsonl";
const LOG_FILE: &str = "log";
const KEPT_FILES_DIR: &str = "files";

/// Digits in a job id, after the `j`. Zero-padded so that the directory
/// listing of `jobs/` is in the order the jobs were minted.
const ID_DIGITS: usize = 10;

/// The hub's job store: a directory per job under one state root.
#[derive(Debug)]
pub struct JobDirStore {
    root: PathBuf,
    live: Mutex<Live>,
}

/// What the store would otherwise have to re-read from the disk on every call.
/// Written through to `index/state.json` after every change, and never
/// consulted in place of a record: a record is always read from its file.
#[derive(Debug)]
struct Live {
    /// The next id to try. Ids are claimed by `mkdir`, so this is a hint that
    /// cannot collide even if it is wrong.
    next_sequence: u64,
    /// How many job directories exist. The index carries this number so that
    /// an index which has fallen behind the directories is detected.
    jobs_accounted: u64,
    /// The jobs that have not reached a terminal state: the queue.
    pending: BTreeSet<u64>,
}

/// `store.json`.
#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    layout_version: u32,
}

/// `index/state.json`.
#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    index_version: u32,
    jobs_accounted: u64,
    pending: Vec<JobId>,
}

/// `job.json`: the record, with the version of its shape in front of it.
#[derive(Debug, Serialize, Deserialize)]
struct RecordFile {
    record_version: u32,
    #[serde(flatten)]
    record: JobRecord,
}

/// One line of `events.jsonl`. Each line carries the version, so a line read
/// on its own — by `tail -f`, while the job runs — says what it is.
#[derive(Debug, Serialize, Deserialize)]
struct EventLine {
    record_version: u32,
    #[serde(flatten)]
    event: JobEvent,
}

/// Enough of any of the files above to read its version before trusting the
/// rest of its fields.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    #[serde(default)]
    record_version: u32,
}

impl JobDirStore {
    /// Open, or create, a state root, and recover what is in it.
    ///
    /// A root that cannot be created, cannot be written, or was written by a
    /// Herd with a different layout is refused here, naming the path: the hub
    /// must not start believing it has no state when in truth it cannot read
    /// the state it has (the same rule E-45 applies to a runner's data root).
    ///
    /// Recovery reads the names in `jobs/` — which is also where the next id
    /// comes from — and then reads only the records the index says are
    /// unfinished. The index is used only if it parses, carries a version this
    /// Herd knows, accounts for exactly as many job directories as there are,
    /// and names only jobs that exist. Otherwise it is ignored and every
    /// record is read, which is what rebuilds it.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        durable::create_dir_all(&root)
            .map_err(|error| failed("creating the state root", &root, &error))?;
        let jobs = root.join(JOBS_DIR);
        durable::create_dir(&jobs).map_err(|error| failed("creating", &jobs, &error))?;
        let index = root.join(INDEX_DIR);
        durable::create_dir(&index).map_err(|error| failed("creating", &index, &error))?;

        let store_file = root.join(STORE_FILE);
        if let Some(text) = read_optional(&store_file)? {
            let found: StoreFile = serde_json::from_str(&text)
                .map_err(|error| failed("reading", &store_file, &error))?;
            if found.layout_version != LAYOUT_VERSION {
                return Err(StoreError::new(format!(
                    "the state root {} has layout version {}, and this hub \
                     writes version {LAYOUT_VERSION}",
                    root.display(),
                    found.layout_version
                )));
            }
        }
        // Rewriting it at every start is also the test that the root is
        // writable, and it is where a reader learns what the layout is.
        write_json(
            &store_file,
            &StoreFile {
                layout_version: LAYOUT_VERSION,
            },
        )?;

        let store = Self {
            root,
            live: Mutex::new(Live {
                next_sequence: 1,
                jobs_accounted: 0,
                pending: BTreeSet::new(),
            }),
        };
        store.recover()?;
        Ok(store)
    }

    /// The state root, for whoever has to say where the state is.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where one job's files live. Stable for the life of the job: the first
    /// consumer reads this directory while the job runs (E-53), so nothing
    /// here ever renames or moves a job's directory.
    pub fn job_dir(&self, id: &JobId) -> PathBuf {
        self.root.join(JOBS_DIR).join(id.as_str())
    }

    fn jobs_dir(&self) -> PathBuf {
        self.root.join(JOBS_DIR)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_DIR).join(INDEX_FILE)
    }

    fn recover(&self) -> Result<()> {
        let jobs_dir = self.jobs_dir();
        let names =
            durable::names_in(&jobs_dir).map_err(|error| failed("listing", &jobs_dir, &error))?;

        let mut sequences = Vec::with_capacity(names.len());
        for name in &names {
            match sequence_of(name) {
                Some(sequence) => sequences.push(sequence),
                // Not ours, so not a job. Said out loud, never deleted: this
                // directory belongs to its owner, not to Herd.
                None => eprintln!(
                    "herd-hub: ignoring {}, which is not a job directory",
                    jobs_dir.join(name).display()
                ),
            }
        }
        sequences.sort_unstable();
        let jobs_accounted = sequences.len() as u64;
        let next_sequence = sequences.last().copied().unwrap_or(0) + 1;

        // With no job directories there is nothing to read and nothing to
        // rebuild, so a brand new state root says nothing about its index.
        let trusted = if sequences.is_empty() {
            Some(Vec::new())
        } else {
            self.usable_index(jobs_accounted, &sequences)
        };
        let to_read: Vec<u64> = match trusted {
            Some(pending) => pending,
            None => sequences.clone(),
        };

        let mut pending = BTreeSet::new();
        for sequence in to_read {
            match self.read_record(&id_of(sequence))? {
                Some(record) => {
                    if !record.state.is_terminal() {
                        pending.insert(sequence);
                    }
                }
                // A directory with no record is a mint that was interrupted
                // between claiming the id and writing the record: there is no
                // job to recover, and the id stays spent.
                None => eprintln!(
                    "herd-hub: {} holds no {RECORD_FILE}, so no job was ever recorded there",
                    self.job_dir(&id_of(sequence)).display()
                ),
            }
        }

        let mut live = self.lock()?;
        live.next_sequence = next_sequence;
        live.jobs_accounted = jobs_accounted;
        live.pending = pending;
        self.write_index(&live)
    }

    /// The pending sequences the index can be believed about, or `None` if the
    /// hub must read every record instead. Every check here answers one
    /// question: could this index be hiding a job?
    fn usable_index(&self, jobs_accounted: u64, sequences: &[u64]) -> Option<Vec<u64>> {
        let path = self.index_path();
        let rebuild = |why: String| -> Option<Vec<u64>> {
            eprintln!(
                "herd-hub: rebuilding the index at {} from the job directories: {why}",
                path.display()
            );
            None
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return rebuild("it is not there".to_owned())
            }
            Err(error) => return rebuild(format!("it cannot be read: {error}")),
        };
        let index: IndexFile = match serde_json::from_str(&text) {
            Ok(index) => index,
            Err(error) => return rebuild(format!("it cannot be read: {error}")),
        };
        if index.index_version != INDEX_VERSION {
            return rebuild(format!(
                "it has index version {}, and this hub writes version {INDEX_VERSION}",
                index.index_version
            ));
        }
        // The count is what catches an index that has fallen behind the
        // directories — a job minted just before the hub died, a job directory
        // removed by hand — without reading a single record.
        if index.jobs_accounted != jobs_accounted {
            return rebuild(format!(
                "it accounts for {} job directories and {jobs_accounted} are there",
                index.jobs_accounted
            ));
        }
        let mut pending = Vec::with_capacity(index.pending.len());
        for id in &index.pending {
            match sequence_of(id.as_str()) {
                Some(sequence) if sequences.binary_search(&sequence).is_ok() => {
                    pending.push(sequence)
                }
                _ => return rebuild(format!("it lists {id}, which is not a job here")),
            }
        }
        Some(pending)
    }

    fn write_index(&self, live: &Live) -> Result<()> {
        let index = IndexFile {
            index_version: INDEX_VERSION,
            jobs_accounted: live.jobs_accounted,
            pending: live.pending.iter().copied().map(id_of).collect(),
        };
        write_json(&self.index_path(), &index)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Live>> {
        self.live
            .lock()
            .map_err(|_| StoreError::new("the job store lock is poisoned"))
    }

    fn read_record(&self, id: &JobId) -> Result<Option<JobRecord>> {
        let path = self.job_dir(id).join(RECORD_FILE);
        let Some(text) = read_optional(&path)? else {
            return Ok(None);
        };
        let probe: VersionProbe =
            serde_json::from_str(&text).map_err(|error| failed("reading", &path, &error))?;
        if probe.record_version != RECORD_VERSION {
            return Err(StoreError::new(format!(
                "{} has record version {}, and this hub reads version {RECORD_VERSION}",
                path.display(),
                probe.record_version
            )));
        }
        let file: RecordFile =
            serde_json::from_str(&text).map_err(|error| failed("reading", &path, &error))?;
        Ok(Some(file.record))
    }

    fn write_record(&self, record: &JobRecord) -> Result<()> {
        let path = self.job_dir(&record.id).join(RECORD_FILE);
        write_json(
            &path,
            &RecordFile {
                record_version: RECORD_VERSION,
                record: record.clone(),
            },
        )
    }

    fn append_event(&self, id: &JobId, event: &JobEvent) -> Result<()> {
        let path = self.job_dir(id).join(EVENTS_FILE);
        let line = serde_json::to_string(&EventLine {
            record_version: RECORD_VERSION,
            event: event.clone(),
        })
        .map_err(|error| failed("writing", &path, &error))?;
        durable::append_line(&path, &line).map_err(|error| failed("appending to", &path, &error))
    }
}

impl JobStore for JobDirStore {
    fn create_job(&self, command: Vec<String>) -> Result<JobRecord> {
        let mut live = self.lock()?;
        // `mkdir` is what claims an id: the directories, not the index, decide
        // which ids are taken, so a wrong hint costs one extra attempt.
        let id = loop {
            let sequence = live.next_sequence;
            live.next_sequence += 1;
            let id = id_of(sequence);
            let dir = self.job_dir(&id);
            match durable::create_dir_exclusive(&dir) {
                Ok(()) => break id,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    eprintln!("herd-hub: {} is already taken", dir.display());
                }
                Err(error) => return Err(failed("creating", &dir, &error)),
            }
        };
        let kept = self.job_dir(&id).join(KEPT_FILES_DIR);
        durable::create_dir(&kept).map_err(|error| failed("creating", &kept, &error))?;

        let at = Timestamp::now();
        let record = JobRecord {
            id: id.clone(),
            command,
            state: JobState::Queued,
            submitted_at: at,
            updated_at: at,
            metrics: JobMetrics::default(),
        };
        self.write_record(&record)?;
        self.append_event(
            &id,
            &JobEvent {
                at,
                state: JobState::Queued,
            },
        )?;
        // The index is written after the record it describes, always: an index
        // that has fallen behind is detected and rebuilt, while an index that
        // ran ahead would claim a job that is not there.
        let sequence = sequence_of(id.as_str()).expect("an id this store minted");
        live.pending.insert(sequence);
        live.jobs_accounted += 1;
        self.write_index(&live)?;
        Ok(record)
    }

    fn record_state(&self, id: &JobId, state: JobState) -> Result<()> {
        let mut live = self.lock()?;
        let Some(mut record) = self.read_record(id)? else {
            return Err(StoreError::new(format!("no such job: {id}")));
        };
        match transition(&record.state, &state) {
            Transition::Allowed => {}
            // A runner redelivering a result it was never acknowledged for
            // reports the same end again. That is the durability handshake
            // working, not a second ending.
            Transition::AlreadyThere => return Ok(()),
            Transition::Refused => {
                return Err(StoreError::new(format!(
                    "{id} cannot go from {} to {}",
                    describe(&record.state),
                    describe(&state)
                )))
            }
        }
        let at = Timestamp::now();
        record.state = state.clone();
        record.updated_at = at;
        // The record first, so that the moment a transition is readable in
        // `events.jsonl` the record agrees with it.
        self.write_record(&record)?;
        self.append_event(id, &JobEvent { at, state })?;

        if record.state.is_terminal() {
            let log = self.job_dir(id).join(LOG_FILE);
            if log.exists() {
                durable::sync_file(&log).map_err(|error| failed("syncing", &log, &error))?;
            }
            if let Some(sequence) = sequence_of(id.as_str()) {
                live.pending.remove(&sequence);
            }
            self.write_index(&live)?;
        }
        Ok(())
    }

    fn append_log(&self, id: &JobId, _stream: LogStream, _seq: u64, data: &str) -> Result<()> {
        let _live = self.lock()?;
        let dir = self.job_dir(id);
        if !dir.is_dir() {
            return Err(StoreError::new(format!("no such job: {id}")));
        }
        let path = dir.join(LOG_FILE);
        durable::append_bytes(&path, data.as_bytes())
            .map_err(|error| failed("appending to", &path, &error))
    }

    fn job(&self, id: &JobId) -> Result<Option<JobRecord>> {
        let _live = self.lock()?;
        self.read_record(id)
    }

    fn job_log(&self, id: &JobId) -> Result<Option<String>> {
        let _live = self.lock()?;
        if !self.job_dir(id).is_dir() {
            return Ok(None);
        }
        let path = self.job_dir(id).join(LOG_FILE);
        Ok(Some(read_optional(&path)?.unwrap_or_default()))
    }

    fn job_events(&self, id: &JobId) -> Result<Option<Vec<JobEvent>>> {
        let _live = self.lock()?;
        if !self.job_dir(id).is_dir() {
            return Ok(None);
        }
        let path = self.job_dir(id).join(EVENTS_FILE);
        let Some(text) = read_optional(&path)? else {
            return Ok(Some(Vec::new()));
        };
        let mut events = Vec::new();
        for (number, line) in text.split_inclusive('\n').enumerate() {
            if !line.ends_with('\n') {
                // The last line has no terminator, so it is an append that did
                // not finish. A transition is only a transition once its line
                // is whole; this one is not there yet.
                eprintln!(
                    "herd-hub: {}:{} is not a whole line, so it is not an event yet",
                    path.display(),
                    number + 1
                );
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let probe: VersionProbe = serde_json::from_str(line)
                .map_err(|error| failed_line("reading", &path, number + 1, &error))?;
            if probe.record_version != RECORD_VERSION {
                return Err(StoreError::new(format!(
                    "{}:{} has record version {}, and this hub reads version {RECORD_VERSION}",
                    path.display(),
                    number + 1,
                    probe.record_version
                )));
            }
            let parsed: EventLine = serde_json::from_str(line)
                .map_err(|error| failed_line("reading", &path, number + 1, &error))?;
            events.push(parsed.event);
        }
        Ok(Some(events))
    }

    fn unfinished_jobs(&self) -> Result<Vec<JobRecord>> {
        let live = self.lock()?;
        let mut records = Vec::with_capacity(live.pending.len());
        for sequence in live.pending.iter().copied() {
            if let Some(record) = self.read_record(&id_of(sequence))? {
                records.push(record);
            }
        }
        Ok(records)
    }
}

/// What a transition from one state to another is.
enum Transition {
    Allowed,
    /// Already recorded, and recording it again is harmless.
    AlreadyThere,
    Refused,
}

/// The job state machine, in one place.
///
/// A job goes forward only: minted queued, handed to a runner, running, over.
/// M3 adds a second attempt after a lost runner, which is a new edge recorded
/// here rather than a record quietly overwritten.
fn transition(from: &JobState, to: &JobState) -> Transition {
    use JobState::{Assigned, Finished, Queued, Running};
    match (from, to) {
        (Queued, Assigned | Running | Finished { .. })
        | (Assigned, Running | Finished { .. })
        | (Running, Finished { .. }) => Transition::Allowed,
        (Finished { outcome: had }, Finished { outcome: got }) if had == got => {
            Transition::AlreadyThere
        }
        _ => Transition::Refused,
    }
}

/// A state as a refusal names it: the outcome included, because "finished" on
/// its own does not say which end was already recorded.
fn describe(state: &JobState) -> String {
    match state {
        JobState::Finished { outcome } => format!(
            "finished {}",
            serde_json::to_string(outcome).unwrap_or_else(|_| "unprintable".to_owned())
        ),
        other => other.name().to_owned(),
    }
}

/// A job's id for one sequence number: zero-padded so that `ls` sorts the
/// directory in the order the jobs were minted. Opaque to every peer (D15's
/// `JobId` is a string and nothing may read its shape).
fn id_of(sequence: u64) -> JobId {
    JobId::new(format!("j{sequence:0>width$}", width = ID_DIGITS))
}

/// The sequence number in a job id, if the name is one of ours.
fn sequence_of(name: &str) -> Option<u64> {
    let digits = name.strip_prefix('j')?;
    if digits.len() != ID_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("reading", path, &error)),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_vec(value).map_err(|error| failed("writing", path, &error))?;
    durable::write_file(path, &text).map_err(|error| failed("writing", path, &error))
}

fn failed(what: &str, path: &Path, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::new(format!("{what} {} failed: {error}", path.display()))
}

fn failed_line(what: &str, path: &Path, line: usize, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::new(format!("{what} {}:{line} failed: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use herd_proto::JobOutcome;
    use tempfile::TempDir;

    /// A store in a directory that goes away with the test.
    fn store() -> (TempDir, JobDirStore) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let store = JobDirStore::open(dir.path()).expect("the state root opens");
        (dir, store)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} reads: {e}", path.display()))
    }

    #[test]
    fn a_minted_job_is_queued_and_has_a_fresh_id() {
        let (_dir, store) = store();
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

    /// The layout a reader outside Herd depends on (E-53, E-54), pinned.
    #[test]
    fn a_job_is_a_directory_of_plain_files_carrying_their_version() {
        let (dir, store) = store();
        let job = store
            .create_job(vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "true".to_owned(),
            ])
            .expect("minted");
        store
            .append_log(&job.id, LogStream::Stdout, 0, "half a line")
            .expect("appended");

        let job_dir = dir.path().join(JOBS_DIR).join(job.id.as_str());
        let mut names = durable::names_in(&job_dir).expect("the job directory lists");
        names.sort();
        assert_eq!(names, vec!["events.jsonl", "files", "job.json", "log"]);
        assert!(job_dir.join(KEPT_FILES_DIR).is_dir(), "room for kept files");

        let record: serde_json::Value =
            serde_json::from_str(&read(&job_dir.join(RECORD_FILE))).expect("job.json is json");
        assert_eq!(record["record_version"], RECORD_VERSION);
        assert_eq!(record["id"], job.id.as_str());
        assert_eq!(record["command"][2], "true");
        assert_eq!(record["state"]["type"], "queued");
        assert!(
            record["submitted_at"]
                .as_str()
                .expect("a timestamp")
                .ends_with('Z'),
            "timestamps are RFC 3339 in UTC: {}",
            record["submitted_at"]
        );
        // M4's fields are in the shape from the start, and say "not measured".
        for field in [
            "load_before",
            "load_after",
            "memory_peak_bytes",
            "bytes_written",
        ] {
            assert!(
                record["metrics"][field].is_null(),
                "metrics.{field} should be present and unset: {}",
                record["metrics"]
            );
        }

        // The log is the job's own bytes, nothing else: it is read by tools
        // that know nothing of Herd.
        assert_eq!(read(&job_dir.join(LOG_FILE)), "half a line");
        // And the layout says which layout it is.
        let store_file: serde_json::Value =
            serde_json::from_str(&read(&dir.path().join(STORE_FILE))).expect("store.json is json");
        assert_eq!(store_file["layout_version"], LAYOUT_VERSION);
    }

    #[test]
    fn transitions_and_log_chunks_land_on_the_record() {
        let (_dir, store) = store();
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
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 3 },
                },
            )
            .expect("transition");

        let record = store.job(&job.id).expect("readable").expect("still there");
        assert_eq!(
            record.state,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 3 }
            }
        );
        assert_eq!(
            store.job_log(&job.id).expect("readable").as_deref(),
            Some("first-second"),
            "both streams interleave in arrival order"
        );
        assert!(
            record.updated_at >= record.submitted_at,
            "the record says when it last changed"
        );
    }

    /// Transitions are present, ordered and timestamped — the order being the
    /// order of the lines, not an ordering of the clock.
    #[test]
    fn every_transition_is_recorded_in_order_with_its_moment() {
        let (dir, store) = store();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        for state in [
            JobState::Assigned,
            JobState::Running,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        ] {
            store.record_state(&job.id, state).expect("transition");
        }
        let events = store
            .job_events(&job.id)
            .expect("readable")
            .expect("still there");
        let names: Vec<&str> = events.iter().map(|event| event.state.name()).collect();
        assert_eq!(names, ["queued", "assigned", "running", "finished"]);
        assert_eq!(
            events.last().expect("four events").state,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 }
            },
            "the end is recorded whole, outcome included"
        );

        let lines = read(
            &dir.path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(EVENTS_FILE),
        );
        assert_eq!(lines.lines().count(), 4, "one line per transition: {lines}");
        for line in lines.lines() {
            let event: serde_json::Value = serde_json::from_str(line).expect("a line is json");
            assert_eq!(event["record_version"], RECORD_VERSION);
            assert!(
                event["at"].as_str().expect("a moment").contains('T'),
                "{line}"
            );
        }
    }

    /// A line that was being appended when the machine stopped is not an
    /// event: a reader sees the transitions that are whole and no more.
    #[test]
    fn an_unfinished_line_is_not_read_as_a_transition() {
        let (dir, store) = store();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(EVENTS_FILE);
        let torn = format!("{}{{\"record_version\":1,\"at\":\"2026", read(&path));
        durable::write_file(&path, torn.as_bytes()).expect("the torn file is in place");

        let events = store
            .job_events(&job.id)
            .expect("readable")
            .expect("still there");
        assert_eq!(events.len(), 1, "only the whole line is an event");
        assert_eq!(events[0].state, JobState::Queued);
    }

    #[test]
    fn every_way_a_job_can_end_is_recorded_whole() {
        let (_dir, store) = store();
        for outcome in [
            JobOutcome::Exited { code: 42 },
            JobOutcome::Signalled { signal: 15 },
            JobOutcome::CouldNotStart {
                reason: "no-such-tool: not found".to_owned(),
            },
        ] {
            let job = store.create_job(vec!["true".to_owned()]).expect("minted");
            store
                .record_state(
                    &job.id,
                    JobState::Finished {
                        outcome: outcome.clone(),
                    },
                )
                .expect("transition");
            assert_eq!(
                store
                    .job(&job.id)
                    .expect("readable")
                    .expect("still there")
                    .state,
                JobState::Finished { outcome }
            );
        }
    }

    #[test]
    fn an_unknown_job_is_an_error_not_a_silent_no_op() {
        let (_dir, store) = store();
        let ghost = JobId::new("j0000000404");
        assert!(store.record_state(&ghost, JobState::Running).is_err());
        assert!(store.append_log(&ghost, LogStream::Stdout, 0, "x").is_err());
        assert_eq!(store.job(&ghost).expect("readable"), None);
        assert_eq!(store.job_log(&ghost).expect("readable"), None);
        assert_eq!(store.job_events(&ghost).expect("readable"), None);
    }

    /// The same end, twice, is a runner redelivering a result it was never
    /// acknowledged for. A *different* end is a bug, and says so.
    #[test]
    fn the_same_end_twice_is_accepted_and_a_different_one_is_not() {
        let (_dir, store) = store();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        let end = JobState::Finished {
            outcome: JobOutcome::Exited { code: 7 },
        };
        store.record_state(&job.id, end.clone()).expect("the end");
        store
            .record_state(&job.id, end)
            .expect("the same end again is the redelivery handshake");
        let error = store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect_err("a job does not end twice, differently");
        assert!(error.to_string().contains("cannot go from"), "{error}");
        assert!(error.to_string().contains("code\":7"), "{error}");
    }

    #[test]
    fn a_job_never_goes_backwards() {
        let (_dir, store) = store();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        store
            .record_state(&job.id, JobState::Running)
            .expect("transition");
        let error = store
            .record_state(&job.id, JobState::Queued)
            .expect_err("M2 has no requeue; M3 adds one as its own transition");
        assert!(
            error.to_string().contains("from running to queued"),
            "a refused transition names both states: {error}"
        );
    }

    #[test]
    fn the_queue_is_every_job_that_is_not_over_oldest_first() {
        let (_dir, store) = store();
        let first = store.create_job(vec!["first".to_owned()]).expect("minted");
        let second = store.create_job(vec!["second".to_owned()]).expect("minted");
        let third = store.create_job(vec!["third".to_owned()]).expect("minted");
        store
            .record_state(
                &second.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("transition");
        let queue: Vec<String> = store
            .unfinished_jobs()
            .expect("readable")
            .into_iter()
            .map(|record| record.id.to_string())
            .collect();
        assert_eq!(queue, vec![first.id.to_string(), third.id.to_string()]);
    }

    #[test]
    fn a_state_root_that_cannot_be_created_is_refused_with_its_path() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let in_the_way = dir.path().join("a-file");
        durable::write_file(&in_the_way, b"not a directory").expect("written");
        let wanted = in_the_way.join("state");
        let error = JobDirStore::open(&wanted).expect_err("there is a file in the way");
        assert!(
            error.to_string().contains(&wanted.display().to_string()),
            "the refusal must name the path: {error}"
        );
    }

    #[test]
    fn a_state_root_from_a_newer_herd_is_refused_naming_both_versions() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        {
            let _store = JobDirStore::open(dir.path()).expect("the state root opens");
        }
        durable::write_file(&dir.path().join(STORE_FILE), br#"{"layout_version":99}"#)
            .expect("written");
        let error = JobDirStore::open(dir.path()).expect_err("99 is not this layout");
        assert!(error.to_string().contains("99"), "{error}");
        assert!(
            error.to_string().contains(&LAYOUT_VERSION.to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_record_from_a_newer_herd_is_refused_rather_than_misread() {
        let (dir, store) = store();
        let job = store.create_job(vec!["true".to_owned()]).expect("minted");
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(RECORD_FILE);
        durable::write_file(&path, br#"{"record_version":2,"id":"j0000000001"}"#).expect("written");
        let error = store
            .job(&job.id)
            .expect_err("version 2 is not readable here");
        assert!(error.to_string().contains("record version 2"), "{error}");
    }

    /// Nothing but the job directories decides which ids exist, so a store
    /// whose index is gone still mints an id that collides with nothing.
    #[test]
    fn ids_come_from_the_directories_and_are_never_reused() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let first = {
            let store = JobDirStore::open(dir.path()).expect("opens");
            store
                .create_job(vec!["true".to_owned()])
                .expect("minted")
                .id
        };
        fs::remove_dir_all(dir.path().join(INDEX_DIR)).expect("the index is gone");
        let store = JobDirStore::open(dir.path()).expect("opens without an index");
        let second = store
            .create_job(vec!["true".to_owned()])
            .expect("minted")
            .id;
        assert_ne!(first, second);
        assert!(second.as_str() > first.as_str(), "{first} then {second}");
    }
}
