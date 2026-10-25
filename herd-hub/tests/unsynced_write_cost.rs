//! S0: what a waiting write costs when it is not synchronised.
//!
//! The architecture work rests on one unmeasured premise — that taking the
//! `fsync`s out of a waiting write makes the dominant post collapse. This
//! file measures that, against the cost that post has now, on the cells and
//! by the method of `docs/remediation/06-junction-breakdown.md`, so the two
//! figures compare without reinterpretation.
//!
//! Derived from `herd-hub/tests/junction_breakdown.rs` (branch
//! `measure-junction-breakdown`): the corpus, the spread, the tracing store
//! and the junction are that file's, so that a figure here and a figure
//! there are the same measurement of two different write paths.
//!
//! **Nothing under `herd-hub/src` is touched.** `record_waiting` is a public
//! `JobStore` method, so the hub's own write path is timed from outside; the
//! unsynchronised path is a replica, written here, over the same two files
//! in the same order and the same bytes — and it is held honest by a *third*
//! path, a replica that keeps the syncs, which must land on the store's own
//! figure. What is left between the two replicas is the syncs and nothing
//! else.
//!
//! Every case is `#[ignore]`d, so `cargo test --workspace` does not run it.
//!
//! ```sh
//! nix develop --command cargo test -p herd-hub --test unsynced_write_cost \
//!   --release -- --ignored --nocapture --test-threads=1
//! ```

mod support;

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use herd_hub::durable;
use herd_hub::job::{
    Budget, ChunkLanded, CollectionOwed, JobEvent, JobMetrics, JobRecord, JobState, LogWrite,
    NewJob, Raise, Reading, Result as StoreResult, Submission, Sweep, Timestamp, RECORD_VERSION,
};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::{JobId, JobOutcome, LogStream, Priority, Requirements, StepRun};

const RECORD_FILE: &str = "job.json";
const EVENTS_FILE: &str = "events.jsonl";

// ---------------------------------------------------------------------------
// The two files a waiting write touches, as the store writes them.
// ---------------------------------------------------------------------------

/// `job.json`, with the version in front of the record — the same shape
/// `store.rs`'s private `RecordFile` has, declared again here because this
/// file reads and writes those bytes from outside the hub.
#[derive(Debug, Serialize, Deserialize)]
struct RecordFile {
    record_version: u32,
    #[serde(flatten)]
    record: JobRecord,
}

/// One line of `events.jsonl`, likewise the shape of `store.rs`'s private
/// `EventLine`.
#[derive(Debug, Serialize, Deserialize)]
struct EventLine {
    record_version: u32,
    #[serde(flatten)]
    event: JobEvent,
}

// ---------------------------------------------------------------------------
// The three write paths.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WritePath {
    /// What the hub does today: `JobStore::record_waiting`.
    Store,
    /// The replica, keeping every sync. The control that says the replica is
    /// the same experiment as the store.
    Synced,
    /// The replica with the three syncs removed: atomic rename for the
    /// record, bare append for the event line.
    Unsynced,
}

impl WritePath {
    fn label(self) -> &'static str {
        match self {
            WritePath::Store => "store  record_waiting",
            WritePath::Synced => "replica, synced    ",
            WritePath::Unsynced => "replica, UNSYNCED  ",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "store" => Some(WritePath::Store),
            "synced" => Some(WritePath::Synced),
            "unsynced" => Some(WritePath::Unsynced),
            _ => None,
        }
    }
}

/// Makes every temporary name unique, as `durable` does for the real one.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// Replace `path` with `bytes` atomically, and do **not** wait for the disk.
///
/// The temporary is written, closed and renamed over the destination. Rename
/// is atomic, so a reader sees the whole old file or the whole new one — the
/// visibility half of what `durable::write_file_with` gives. What is dropped
/// is the two `fsync`s: the bytes and the name are in the page cache when
/// this returns, which survives a killed hub and not a lost machine. This is
/// `durable::append_bytes`'s bargain (`durable.rs:74`) applied to a record.
fn write_file_unsynced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = temporary_beside(path);
    let written = (|| -> io::Result<()> {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn temporary_beside(path: &Path) -> PathBuf {
    let n = TEMPORARIES.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "record".to_owned());
    path.with_file_name(format!(".{name}.s0.{}.{n}", std::process::id()))
}

/// Append one line and do **not** wait for the disk: `durable::append_bytes`
/// with the line terminator `durable::append_line` adds.
fn append_line_unsynced(path: &Path, line: &str) -> io::Result<()> {
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(record.as_bytes())
}

/// `record_waiting`, replicated from outside the hub, with the syncs on or
/// off.
///
/// Line for line what `store.rs:2007` does — read the record, no-op unless
/// the job is queued and the cause is news, stamp it, write the record, then
/// append the event — minus the store's `Mutex<Live>`, which cannot be taken
/// from here. That difference is stated in the dossier; for a single-threaded
/// pass it changes nothing, and `WritePath::Synced` is the control that proves it.
fn replica_record_waiting(
    job_dir: &Path,
    reason: &str,
    synced: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let record_path = job_dir.join(RECORD_FILE);
    let text = fs::read(&record_path)?;
    let mut file: RecordFile = serde_json::from_slice(&text)?;
    if file.record_version != RECORD_VERSION {
        return Err(format!("record version {} is not ours", file.record_version).into());
    }
    if file.record.state != JobState::Queued || file.record.waiting_for.as_deref() == Some(reason) {
        return Ok(false);
    }
    let at = Timestamp::now();
    file.record.waiting_for = Some(reason.to_owned());
    file.record.updated_at = at;
    let bytes = serde_json::to_vec(&file)?;

    let line = serde_json::to_string(&EventLine {
        record_version: RECORD_VERSION,
        event: JobEvent::waiting(at, reason.to_owned()),
    })?;
    let events_path = job_dir.join(EVENTS_FILE);

    if synced {
        durable::write_file(&record_path, &bytes)?;
        durable::append_line(&events_path, &line)?;
    } else {
        write_file_unsynced(&record_path, &bytes)?;
        append_line_unsynced(&events_path, &line)?;
    }
    Ok(true)
}

/// One waiting write by whichever path, timed by the caller.
fn write_one(store: &JobDirStore, id: &JobId, reason: &str, by: WritePath) -> bool {
    match by {
        WritePath::Store => store
            .record_waiting(id, reason)
            .expect("the store records the wait"),
        WritePath::Synced => replica_record_waiting(&store.job_dir(id), reason, true)
            .expect("the synced replica records the wait"),
        WritePath::Unsynced => replica_record_waiting(&store.job_dir(id), reason, false)
            .expect("the unsynced replica records the wait"),
    }
}

// ---------------------------------------------------------------------------
// The corpus — `junction_breakdown.rs`'s, unchanged.
// ---------------------------------------------------------------------------

struct Corpus {
    /// Held so the state root outlives the corpus; never read.
    _dir: tempfile::TempDir,
    store: Arc<JobDirStore>,
    ids: Vec<JobId>,
}

impl Corpus {
    fn build(n: usize, t: usize) -> Self {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));

        let template = store
            .submit(NewJob::new(vec!["true".to_owned(), "template".to_owned()]))
            .expect("the template job is minted")
            .record()
            .id
            .clone();
        store
            .record_state(
                &template,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the template job finishes");
        let template_text = fs::read_to_string(store.job_dir(&template).join(RECORD_FILE))
            .expect("the template record reads");
        let digits = template.as_str().len() - 1;

        let mut ids = Vec::with_capacity(n);
        for i in 0..n {
            ids.push(
                store
                    .submit(NewJob::needing(
                        vec!["sleep".to_owned(), format!("unsynced-{i}")],
                        support::labels(&["unsynced-unobtainable"]),
                    ))
                    .expect("an open job is minted")
                    .record()
                    .id
                    .clone(),
            );
        }

        let jobs = dir.path().join("jobs");
        for i in 0..t {
            let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
            let job_dir = jobs.join(&name);
            fs::create_dir_all(&job_dir).expect("a closed job directory");
            fs::write(
                job_dir.join(RECORD_FILE),
                template_text.replace(template.as_str(), &name),
            )
            .expect("a closed job record");
        }

        Self {
            _dir: dir,
            store,
            ids,
        }
    }

    fn warm(&self) {
        let _ = self.store.unfinished_jobs();
        let _ = JobStore::collections_owed(self.store.as_ref());
    }
}

// ---------------------------------------------------------------------------
// Statistics — `junction_breakdown.rs`'s, unchanged.
// ---------------------------------------------------------------------------

fn quantile(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let at = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[at]
}

struct Spread {
    median: Duration,
    low: Duration,
    high: Duration,
    min: Duration,
    max: Duration,
    n: usize,
}

impl Spread {
    fn of(mut samples: Vec<Duration>) -> Self {
        samples.sort_unstable();
        Self {
            median: quantile(&samples, 0.5),
            low: quantile(&samples, 0.25),
            high: quantile(&samples, 0.75),
            min: *samples.first().unwrap_or(&Duration::ZERO),
            max: *samples.last().unwrap_or(&Duration::ZERO),
            n: samples.len(),
        }
    }

    fn relative_iqr(&self) -> f64 {
        if self.median.is_zero() {
            return f64::NAN;
        }
        (self.high.as_secs_f64() - self.low.as_secs_f64()) / self.median.as_secs_f64()
    }

    fn say(&self) -> String {
        format!(
            "{:>11} [{:>10} .. {:>10}] min {:>10} max {:>10} iqr/med {:>7.1}% n={}",
            fmt(self.median),
            fmt(self.low),
            fmt(self.high),
            fmt(self.min),
            fmt(self.max),
            self.relative_iqr() * 100.0,
            self.n
        )
    }
}

fn fmt(d: Duration) -> String {
    let us = d.as_secs_f64() * 1e6;
    if us < 1.0 {
        format!("{:.0}ns", us * 1000.0)
    } else if us < 1000.0 {
        format!("{us:.1}us")
    } else if us < 1_000_000.0 {
        format!("{:.2}ms", us / 1000.0)
    } else {
        format!("{:.3}s", us / 1e6)
    }
}

fn load() -> String {
    fs::read_to_string("/proc/loadavg").unwrap_or_else(|_| "unknown".to_owned())
}

// ---------------------------------------------------------------------------
// Phase S0a: the floors, synced and not.
// ---------------------------------------------------------------------------

/// The primitives one waiting write is made of, each with its sync and
/// without it. `06`'s phase 0 measured the left-hand column; this adds the
/// right-hand one, so the difference below has a floor under it.
#[test]
#[ignore = "a bench; run it on purpose"]
fn sync_floor() {
    println!("\n== S0a: the floor under one waiting write, with and without the syncs ==");
    println!("load before this phase: {}", load().trim());
    let dir = tempfile::tempdir().expect("a temporary directory");
    let body = vec![b'x'; 1024];

    let synced_write = Spread::of(
        (0..100)
            .map(|i| {
                let path = dir.path().join(format!("synced-{i}"));
                let at = Instant::now();
                durable::write_file(&path, &body).expect("the synced write lands");
                at.elapsed()
            })
            .collect(),
    );
    let unsynced_write = Spread::of(
        (0..100)
            .map(|i| {
                let path = dir.path().join(format!("unsynced-{i}"));
                let at = Instant::now();
                write_file_unsynced(&path, &body).expect("the unsynced write lands");
                at.elapsed()
            })
            .collect(),
    );
    let synced_append = Spread::of(
        (0..100)
            .map(|_| {
                let path = dir.path().join("synced.jsonl");
                let at = Instant::now();
                durable::append_line(&path, "a line of about the size of an event line")
                    .expect("the synced append lands");
                at.elapsed()
            })
            .collect(),
    );
    let unsynced_append = Spread::of(
        (0..100)
            .map(|_| {
                let path = dir.path().join("unsynced.jsonl");
                let at = Instant::now();
                append_line_unsynced(&path, "a line of about the size of an event line")
                    .expect("the unsynced append lands");
                at.elapsed()
            })
            .collect(),
    );

    println!("  atomic replace, synced   | {}", synced_write.say());
    println!("  atomic replace, UNSYNCED | {}", unsynced_write.say());
    println!("  append line, synced      | {}", synced_append.say());
    println!("  append line, UNSYNCED    | {}", unsynced_append.say());
    println!(
        "  floor for one waiting write: synced {}, unsynced {}",
        fmt(synced_write.median + synced_append.median),
        fmt(unsynced_write.median + unsynced_append.median)
    );
}

// ---------------------------------------------------------------------------
// Phase S0b: a whole pass of N waiting writes, three ways.
// ---------------------------------------------------------------------------

/// `06`'s phase 1d, run once per write path.
///
/// A fresh corpus per path, because the first pass over a corpus is the news
/// and the second is not: the three paths must each meet an untouched queue
/// to be doing the same work. `HERD_CELLS` overrides the queue depths;
/// the default is `06`'s reproducible range, `N` >= 200.
#[test]
#[ignore = "a bench; run it on purpose"]
fn whole_pass_three_ways() {
    let cells: Vec<usize> = std::env::var("HERD_CELLS")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect::<Vec<usize>>()
        })
        .filter(|v: &Vec<usize>| !v.is_empty())
        .unwrap_or_else(|| vec![200, 1_000]);
    let t: usize = std::env::var("HERD_T")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    println!("\n== S0b: a whole pass of N waiting writes, three ways ==");
    println!("load before this phase: {}", load().trim());
    println!("T = {t} closed job directories beside the queue");

    for n in cells {
        println!("\n-- N = {n} --");
        for by in [WritePath::Store, WritePath::Synced, WritePath::Unsynced] {
            let corpus = Corpus::build(n, t);
            corpus.warm();

            let mut news = Vec::with_capacity(n);
            let at = Instant::now();
            for (i, id) in corpus.ids.iter().enumerate() {
                let one = Instant::now();
                let wrote = write_one(&corpus.store, id, &format!("waiting because cause {i}"), by);
                news.push(one.elapsed());
                assert!(wrote, "a distinct cause is news");
            }
            let whole = at.elapsed();

            // The same cause again: D171's quiet path, which must stay a
            // no-op whichever way the write is made.
            let mut quiet = Vec::with_capacity(n);
            for (i, id) in corpus.ids.iter().enumerate() {
                let one = Instant::now();
                let wrote = write_one(&corpus.store, id, &format!("waiting because cause {i}"), by);
                quiet.push(one.elapsed());
                assert!(!wrote, "the same cause is not news");
            }

            // What the hub would read back, read back: the replica must leave
            // a record the store itself can read, or it has measured nothing.
            let read = corpus
                .store
                .job(&corpus.ids[n / 2])
                .expect("the store reads the record it did not write")
                .expect("the record is there");
            assert_eq!(
                read.waiting_for.as_deref(),
                Some(format!("waiting because cause {}", n / 2).as_str()),
                "{:?} left a record the store disagrees with",
                by
            );
            let events =
                fs::read_to_string(corpus.store.job_dir(&corpus.ids[n / 2]).join(EVENTS_FILE))
                    .expect("the event file reads");
            assert!(
                events.lines().filter(|l| l.contains("waiting")).count() == 1,
                "{:?} wrote the wrong number of event lines",
                by
            );

            let news = Spread::of(news);
            let quiet = Spread::of(quiet);
            println!("{} | one write  {}", by.label(), news.say());
            println!("{} | no news    {}", by.label(), quiet.say());
            println!(
                "{} | WHOLE PASS of {n} news writes: {}  ({} per job)",
                by.label(),
                fmt(whole),
                fmt(whole / n as u32)
            );
            drop(corpus);
        }
    }
}

// ---------------------------------------------------------------------------
// Phase S0c: count the syncs, do not time them.
// ---------------------------------------------------------------------------

/// How many `fsync`/`fdatasync` one waiting write makes, by path.
///
/// Not answerable from inside the process, so this case exists to be run
/// under `strace -c` twice with different `HERD_WRITES`, exactly as `06` did
/// it: the corpus is identical in both runs and cancels out of the
/// difference.
///
/// ```sh
/// for p in store synced unsynced; do
///   for k in 1 201; do
///     HERD_PATH=$p HERD_WRITES=$k strace -f -c -e trace=fsync,fdatasync \
///       target/release/deps/unsynced_write_cost-<hash> \
///       --exact --ignored --nocapture syncs_per_waiting_write
///   done
/// done
/// ```
#[test]
#[ignore = "a bench; run it on purpose, under strace"]
fn syncs_per_waiting_write() {
    let writes: usize = std::env::var("HERD_WRITES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let by = std::env::var("HERD_PATH")
        .ok()
        .and_then(|v| WritePath::parse(&v))
        .unwrap_or(WritePath::Store);
    let corpus = Corpus::build(writes, 0);
    corpus.warm();
    let skip = std::env::var("HERD_SKIP_WRITES").is_ok();
    for (i, id) in corpus.ids.iter().enumerate() {
        if skip {
            break;
        }
        let wrote = write_one(&corpus.store, id, &format!("cause {i}"), by);
        assert!(wrote, "a distinct cause is news");
    }
    println!("HERD_PATH={by:?} HERD_WRITES={writes}: {writes} waiting write(s) that were news");
}

// ---------------------------------------------------------------------------
// Phase S0d: one real junction, with the waiting write swapped underneath it.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Post {
    Historical,
    Open,
    Write,
    LockWait,
    Compute,
}

impl Post {
    fn label(self) -> &'static str {
        match self {
            Post::Historical => "A historical reads",
            Post::Open => "B open-job reads",
            Post::Write => "C durable writes",
            Post::LockWait => "D lock wait",
            Post::Compute => "E dispatch compute",
        }
    }
}

fn post_of(method: &str) -> Option<Post> {
    match method {
        "collections_owed" | "collection_owed" | "sweep_outputs" => Some(Post::Historical),
        "unfinished_jobs" => Some(Post::Open),
        "record_waiting"
        | "record_state"
        | "begin_attempt"
        | "end_attempt"
        | "record_metrics"
        | "record_outputs"
        | "record_step"
        | "record_reading"
        | "remember_reading"
        | "note_outputs_fetched"
        | "name_missing_log"
        | "submit"
        | "raise_priority" => Some(Post::Write),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Call {
    thread: ThreadId,
    method: &'static str,
    enter: Instant,
    exit: Instant,
}

struct Trace {
    calls: Mutex<Vec<Call>>,
    recording: AtomicBool,
}

impl Trace {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::with_capacity(200_000)),
            recording: AtomicBool::new(false),
        })
    }
    fn start(&self) {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clear();
        self.recording.store(true, Ordering::SeqCst);
    }
    fn stop(&self) -> Vec<Call> {
        self.recording.store(false, Ordering::SeqCst);
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn note(&self, call: Call) {
        if !self.recording.load(Ordering::Relaxed) {
            return;
        }
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(call);
    }
}

/// `junction_breakdown.rs`'s tracing store, with one addition: the waiting
/// write can be served by a replica instead of by the inner store, so a real
/// junction can be run over either path without a line of `herd-hub/src`
/// changing.
struct TracingStore {
    inner: Arc<JobDirStore>,
    trace: Arc<Trace>,
    waiting_by: WritePath,
}

impl TracingStore {
    fn new(inner: Arc<JobDirStore>, trace: Arc<Trace>, waiting_by: WritePath) -> Self {
        Self {
            inner,
            trace,
            waiting_by,
        }
    }

    fn around<T>(&self, method: &'static str, f: impl FnOnce() -> T) -> T {
        let enter = Instant::now();
        let value = f();
        let exit = Instant::now();
        self.trace.note(Call {
            thread: thread::current().id(),
            method,
            enter,
            exit,
        });
        value
    }
}

impl JobStore for TracingStore {
    fn state_root_id(&self) -> &str {
        self.inner.state_root_id()
    }
    fn approve_runner(&self, name: &str) -> StoreResult<String> {
        self.around("approve_runner", || self.inner.approve_runner(name))
    }
    fn remove_runner(&self, name: &str) -> StoreResult<bool> {
        self.around("remove_runner", || self.inner.remove_runner(name))
    }
    fn approved_runners(&self) -> StoreResult<Vec<herd_hub::roster::Approval>> {
        self.around("approved_runners", || self.inner.approved_runners())
    }
    fn admit_runner(&self, name: &str, token: Option<&str>) -> herd_hub::roster::Admission {
        self.around("admit_runner", || self.inner.admit_runner(name, token))
    }
    fn approved_runner_count(&self) -> usize {
        self.inner.approved_runner_count()
    }
    fn submit(&self, new: NewJob) -> StoreResult<Submission> {
        self.around("submit", || self.inner.submit(new))
    }
    fn raise_priority(&self, id: &JobId, to: Priority) -> StoreResult<Raise> {
        self.around("raise_priority", || self.inner.raise_priority(id, to))
    }
    fn record_waiting(&self, id: &JobId, reason: &str) -> StoreResult<bool> {
        self.around("record_waiting", || match self.waiting_by {
            WritePath::Store => self.inner.record_waiting(id, reason),
            by => Ok(replica_record_waiting(
                &self.inner.job_dir(id),
                reason,
                by == WritePath::Synced,
            )
            .expect("the replica records the wait")),
        })
    }
    fn record_state(&self, id: &JobId, state: JobState) -> StoreResult<()> {
        self.around("record_state", || self.inner.record_state(id, state))
    }
    fn record_reading(&self, id: &JobId, reading: Reading) -> StoreResult<()> {
        self.around("record_reading", || self.inner.record_reading(id, reading))
    }
    fn reading_answer(&self, key: &str) -> StoreResult<Option<Requirements>> {
        self.around("reading_answer", || self.inner.reading_answer(key))
    }
    fn remember_reading(&self, key: &str, answer: &Requirements) -> StoreResult<()> {
        self.around("remember_reading", || {
            self.inner.remember_reading(key, answer)
        })
    }
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> StoreResult<()> {
        self.around("record_metrics", || self.inner.record_metrics(id, metrics))
    }
    fn record_outputs(&self, id: &JobId, outputs: &herd_proto::JobOutputs) -> StoreResult<()> {
        self.around("record_outputs", || self.inner.record_outputs(id, outputs))
    }
    fn job_outputs(&self, id: &JobId, with_bytes: bool) -> StoreResult<herd_proto::JobOutputs> {
        self.around("job_outputs", || self.inner.job_outputs(id, with_bytes))
    }
    fn write_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> StoreResult<ChunkLanded> {
        self.around("write_output_chunk", || {
            self.inner.write_output_chunk(id, path, offset, data)
        })
    }
    fn output_holdings(&self, id: &JobId) -> StoreResult<Vec<herd_proto::FileHeld>> {
        self.around("output_holdings", || self.inner.output_holdings(id))
    }
    fn collections_owed(&self) -> StoreResult<Vec<CollectionOwed>> {
        self.around("collections_owed", || self.inner.collections_owed())
    }
    fn collection_owed(&self, id: &JobId) -> StoreResult<Option<CollectionOwed>> {
        self.around("collection_owed", || self.inner.collection_owed(id))
    }
    fn output_budget(&self) -> Budget {
        self.inner.output_budget()
    }
    fn note_outputs_fetched(&self, id: &JobId) -> StoreResult<()> {
        self.around("note_outputs_fetched", || {
            self.inner.note_outputs_fetched(id)
        })
    }
    fn sweep_outputs(&self) -> StoreResult<Sweep> {
        self.around("sweep_outputs", || self.inner.sweep_outputs())
    }
    fn read_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        take: usize,
    ) -> StoreResult<Option<Vec<u8>>> {
        self.around("read_output_chunk", || {
            self.inner.read_output_chunk(id, path, offset, take)
        })
    }
    fn clear_output_parts(&self, id: &JobId) -> StoreResult<()> {
        self.around("clear_output_parts", || self.inner.clear_output_parts(id))
    }
    fn begin_attempt(
        &self,
        id: &JobId,
        runner: &str,
        why: &str,
        parallelism: Option<u32>,
    ) -> StoreResult<u32> {
        self.around("begin_attempt", || {
            self.inner.begin_attempt(id, runner, why, parallelism)
        })
    }
    fn record_step(&self, id: &JobId, attempt: u32, step: StepRun) -> StoreResult<()> {
        self.around("record_step", || self.inner.record_step(id, attempt, step))
    }
    fn end_attempt(
        &self,
        id: &JobId,
        attempt: u32,
        reason: &str,
        requeue_code: Option<i32>,
    ) -> StoreResult<()> {
        self.around("end_attempt", || {
            self.inner.end_attempt(id, attempt, reason, requeue_code)
        })
    }
    fn append_log(
        &self,
        id: &JobId,
        stream: LogStream,
        seq: u64,
        data: &[u8],
    ) -> StoreResult<LogWrite> {
        self.around("append_log", || {
            self.inner.append_log(id, stream, seq, data)
        })
    }
    fn chunks_recorded(&self, id: &JobId) -> StoreResult<u64> {
        self.around("chunks_recorded", || self.inner.chunks_recorded(id))
    }
    fn name_missing_log(
        &self,
        id: &JobId,
        first_missing: u64,
        last_missing: u64,
    ) -> StoreResult<()> {
        self.around("name_missing_log", || {
            self.inner.name_missing_log(id, first_missing, last_missing)
        })
    }
    fn job(&self, id: &JobId) -> StoreResult<Option<JobRecord>> {
        self.around("job", || self.inner.job(id))
    }
    fn quarantine_reason(&self, id: &JobId) -> StoreResult<Option<String>> {
        self.around("quarantine_reason", || self.inner.quarantine_reason(id))
    }
    fn job_log(&self, id: &JobId) -> StoreResult<Option<Vec<u8>>> {
        self.around("job_log", || self.inner.job_log(id))
    }
    fn job_events(&self, id: &JobId) -> StoreResult<Option<Vec<JobEvent>>> {
        self.around("job_events", || self.inner.job_events(id))
    }
    fn unfinished_jobs(&self) -> StoreResult<Vec<JobRecord>> {
        self.around("unfinished_jobs", || self.inner.unfinished_jobs())
    }
}

#[derive(Default, Clone)]
struct Breakdown {
    posts: BTreeMap<Post, Duration>,
    other: Duration,
    window: Duration,
    calls: BTreeMap<&'static str, u64>,
}

fn break_down(calls: &[Call], settled: Instant) -> Option<Breakdown> {
    let opening = calls
        .iter()
        .position(|c| c.method == "collections_owed" && c.exit <= settled)?;
    let thread = calls[opening].thread;
    let mine: Vec<Call> = calls
        .iter()
        .copied()
        .filter(|c| c.thread == thread && c.enter >= calls[opening].enter && c.exit <= settled)
        .collect();
    if mine.is_empty() {
        return None;
    }
    let start = mine[0].enter;
    let end = mine.last().expect("at least one call").exit;

    let mut out = Breakdown {
        window: end.saturating_duration_since(start),
        ..Default::default()
    };
    let mut seen_queue_read = false;
    let mut previous_exit = start;
    for call in &mine {
        *out.calls.entry(call.method).or_insert(0) += 1;
        let gap = call.enter.saturating_duration_since(previous_exit);
        let bucket = if seen_queue_read {
            Post::Compute
        } else {
            Post::LockWait
        };
        *out.posts.entry(bucket).or_default() += gap;
        if call.method == "unfinished_jobs" {
            seen_queue_read = true;
        }
        let inside = call.exit.saturating_duration_since(call.enter);
        match post_of(call.method) {
            Some(post) => *out.posts.entry(post).or_default() += inside,
            None => out.other += inside,
        }
        previous_exit = call.exit;
    }
    Some(out)
}

/// `06`'s phase 2, run once per write path: a hub on a loopback port, a real
/// `herd_runner::run` dialling it, the five posts read off the store's trace
/// — and the waiting write served either by the store or by the replica.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a bench; run it on purpose"]
async fn one_real_junction_unsynced() {
    let rounds: usize = std::env::var("HERD_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let cells: Vec<usize> = std::env::var("HERD_CELLS")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect::<Vec<usize>>()
        })
        .filter(|v: &Vec<usize>| !v.is_empty())
        .unwrap_or_else(|| vec![200, 1_000]);

    println!("\n== S0d: one real junction, the waiting write swapped underneath it ==");
    println!("load before this phase: {}", load().trim());
    println!("{rounds} junctions per cell, medians. T = 0.");

    for n in cells {
        for by in [WritePath::Store, WritePath::Unsynced] {
            let mut windows = Vec::new();
            let mut per_post: BTreeMap<Post, Vec<Duration>> = BTreeMap::new();
            let mut calls_seen: BTreeMap<&'static str, u64> = BTreeMap::new();
            for _ in 0..rounds {
                let Some(b) = one_junction(n, by).await else {
                    println!("N={n} {} : a round never settled", by.label());
                    continue;
                };
                windows.push(b.window);
                for (post, took) in b.posts {
                    per_post.entry(post).or_default().push(took);
                }
                for (m, k) in b.calls {
                    *calls_seen.entry(m).or_insert(0) += k;
                }
            }
            if windows.is_empty() {
                continue;
            }
            let window = Spread::of(windows);
            println!("\n-- N = {n}, {} --", by.label());
            for (post, samples) in per_post {
                let s = Spread::of(samples);
                let per_job = if post == Post::Write {
                    format!("  ({} per job)", fmt(s.median / n as u32))
                } else {
                    String::new()
                };
                println!("  {:<20} {}{}", post.label(), s.say(), per_job);
            }
            println!("  {:<20} {}", "junction window", window.say());
            println!("  calls: {calls_seen:?}");
        }
    }
}

async fn one_junction(n: usize, by: WritePath) -> Option<Breakdown> {
    let corpus = Corpus::build(n, 0);
    corpus.warm();
    let trace = Trace::new();
    let store = Arc::new(TracingStore::new(
        Arc::clone(&corpus.store),
        Arc::clone(&trace),
        by,
    ));
    let token = store
        .approve_runner("unsynced-runner")
        .expect("the state root mints a token");

    let hub = herd_hub::Hub::bind_with_policy(
        "127.0.0.1:0".parse().expect("a literal loopback address"),
        Arc::clone(&store) as Arc<dyn JobStore>,
        herd_hub::Policy::default(),
    )
    .await
    .expect("the hub binds a free port");
    let hub_url = format!("ws://{}", hub.local_addr());
    let handle = hub.handle();
    let serving = tokio::spawn(async move {
        let _ = hub.serve().await;
    });

    let data_root = support::DataRootDir::new("unsynced");
    let config = herd_runner::Config {
        hub_url,
        name: Some("unsynced-runner".to_owned()),
        token: Some(token),
        confinement: support::confinement(&data_root),
        ..herd_runner::Config::default()
    };

    trace.start();
    let running = tokio::spawn(async move {
        let _ = herd_runner::run(config).await;
    });

    let deadline = Instant::now() + support::PATIENCE;
    while !handle.runner_connected() {
        if Instant::now() >= deadline {
            running.abort();
            serving.abort();
            return None;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let deadline = Instant::now() + Duration::from_secs(300);
    let settled = loop {
        let done = {
            let held = trace.calls.lock().unwrap_or_else(|p| p.into_inner());
            held.iter().filter(|c| c.method == "record_waiting").count()
        };
        if done >= n {
            break Instant::now();
        }
        if Instant::now() >= deadline {
            running.abort();
            serving.abort();
            let _ = trace.stop();
            return None;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };

    let calls = trace.stop();
    running.abort();
    serving.abort();
    drop(data_root);
    let out = break_down(&calls, settled);

    // The replica must leave a record the hub itself reads back, or the
    // junction measured a write that did not happen.
    let read = corpus
        .store
        .job(&corpus.ids[n / 2])
        .expect("the record reads")
        .expect("the record is there");
    assert!(
        read.waiting_for.is_some(),
        "{by:?} left a job with no recorded wait"
    );
    out
}
