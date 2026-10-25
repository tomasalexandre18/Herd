//! C1: the five posts of one runner junction, measured on a real event.
//!
//! This file measures; it asserts no product behaviour, and every case is
//! `#[ignore]`d so the workspace suite stays the suite. Run it on purpose:
//!
//! ```sh
//! nix develop --command cargo test -p herd-hub --test junction_breakdown \
//!   --release -- --ignored --nocapture --test-threads=1
//! ```
//!
//! **Nothing under `herd-hub/src` is touched.** Every figure here is taken
//! from outside the hub, through a delegating [`JobStore`] that timestamps
//! the entry and the exit of every call the hub makes. That is enough,
//! because the three store-touching posts are public trait methods and the
//! two remaining posts are exactly the *gaps between* those calls:
//!
//! ```text
//! server.rs:2415  ask_for_the_files_still_owed -> collections_owed()   (A)
//!                 .......... gap ..........                            (D)
//!                     dispatch_queued_jobs takes `dispatching` (:2493)
//! server.rs:2510  dispatch_a_pass -> unfinished_jobs()                 (B)
//!                 .......... gaps ..........                           (E)
//!                     queue::order, matcher::holdings/reserved/place,
//!                     queue::waiting_sentence_behind
//! server.rs:2640  say_it_waits -> record_waiting() x k                 (C)
//! ```
//!
//! The pass holds `state.dispatching` from before `unfinished_jobs` to after
//! the last `record_waiting`, so **every gap after the first `unfinished_jobs`
//! is work and not waiting**, and the one gap before it is where a second
//! junction waits. That is the whole separation, and it needs no probe inside
//! the hub.
//!
//! The five posts, in Alexa's words and in the columns below:
//!
//! | post | what it is                 | what is counted                       |
//! |------|----------------------------|---------------------------------------|
//! | A    | historical reads           | time inside `collections_owed`        |
//! | B    | open-job reads             | time inside `unfinished_jobs`         |
//! | C    | durable writes             | time inside `record_waiting` & co.    |
//! | D    | lock wait                  | gap between A's exit and B's entry    |
//! | E    | dispatch computation       | every other gap inside the window     |
//!
//! A + B + C + D + E is the junction window exactly, by construction.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use herd_hub::job::{
    Budget, ChunkLanded, CollectionOwed, JobEvent, JobMetrics, JobRecord, JobState, LogWrite,
    NewJob, Raise, Reading, Result as StoreResult, Submission, Sweep,
};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::{JobId, JobOutcome, LogStream, Priority, Requirements, StepRun};

const RECORD_FILE: &str = "job.json";

// ---------------------------------------------------------------------------
// The five posts.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Post {
    /// A: reads of the closed history under `jobs/`.
    Historical,
    /// B: reads of the open jobs.
    Open,
    /// C: durable writes.
    Write,
    /// D: time a thread spent blocked on a lock it did not hold.
    LockWait,
    /// E: the dispatch computation itself.
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

/// Which post a store call belongs to. Only the calls a junction makes are
/// named; anything else lands in `Other` and is reported separately so that
/// nothing is silently folded into a post it does not belong to.
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

// ---------------------------------------------------------------------------
// A store that says when it was called, and for how long.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Call {
    thread: ThreadId,
    method: &'static str,
    enter: Instant,
    exit: Instant,
}

/// Every store call this process made, in the order they finished.
///
/// One mutex, with the vector sized up front: the push happens **after** the
/// exit timestamp is taken, so it cannot inflate a call's own duration, and
/// `noise_floor` below measures what it does to a gap.
struct Trace {
    calls: Mutex<Vec<Call>>,
    recording: AtomicBool,
    pushes: AtomicU64,
}

impl Trace {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::with_capacity(200_000)),
            recording: AtomicBool::new(false),
            pushes: AtomicU64::new(0),
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
        self.pushes.fetch_add(1, Ordering::Relaxed);
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(call);
    }
}

struct TracingStore {
    inner: Arc<JobDirStore>,
    trace: Arc<Trace>,
}

impl TracingStore {
    fn new(inner: Arc<JobDirStore>, trace: Arc<Trace>) -> Self {
        Self { inner, trace }
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
        self.around("record_waiting", || self.inner.record_waiting(id, reason))
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
        how: herd_hub::job::HowItEnded,
    ) -> StoreResult<()> {
        self.around("end_attempt", || {
            self.inner.end_attempt(id, attempt, reason, how)
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

// ---------------------------------------------------------------------------
// The corpus.
// ---------------------------------------------------------------------------

/// A state root holding `n` open jobs nothing can run and `t` closed job
/// directories. Built exactly as `cost_surface.rs` builds its corpus, so the
/// two campaigns measure the same shape of disk.
struct Corpus {
    dir: tempfile::TempDir,
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
                        vec!["sleep".to_owned(), format!("breakdown-{i}")],
                        support::labels(&["breakdown-unobtainable"]),
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

        Self { dir, store, ids }
    }

    fn directories(&self) -> usize {
        fs::read_dir(self.dir.path().join("jobs"))
            .expect("the jobs directory lists")
            .count()
    }

    /// Read everything once, so no figure below is a first-touch figure.
    fn warm(&self) {
        let _ = self.store.unfinished_jobs();
        let _ = JobStore::collections_owed(self.store.as_ref());
    }
}

// ---------------------------------------------------------------------------
// Statistics: a median is not enough when the question is "how noisy".
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

    /// The interquartile range as a fraction of the median: the one number
    /// that says whether a post is a figure or a guess.
    fn relative_iqr(&self) -> f64 {
        if self.median.is_zero() {
            return f64::NAN;
        }
        (self.high.as_secs_f64() - self.low.as_secs_f64()) / self.median.as_secs_f64()
    }

    fn say(&self) -> String {
        format!(
            "{:>11} [{:>10} .. {:>10}] min {:>10} max {:>10} iqr/med {:>6.1}% n={}",
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

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

// ---------------------------------------------------------------------------
// Phase 0: the noise, before any signal.
// ---------------------------------------------------------------------------

/// What this host costs before the hub costs anything.
///
/// Five floors, because each post rests on a different one:
///
/// - the clock and the trace, which every figure in this file pays twice;
/// - `fsync`, which post C is made of;
/// - a warm record read, which posts A and B are made of;
/// - an uncontended `std::sync::Mutex` round trip, which post D is measured
///   against;
/// - the first-touch penalty, which says whether "warm" is a real state.
#[test]
#[ignore = "a bench; run it on purpose"]
fn noise_floor() {
    println!("\n== phase 0: the floors under the five posts ==");
    println!("state root filesystem: see `df -T` in the report; this is $TMPDIR");

    // 1. The clock.
    let mut clock = Vec::new();
    for _ in 0..200 {
        let at = Instant::now();
        let b = Instant::now();
        clock.push(b.saturating_duration_since(at));
    }
    println!(
        "  Instant::now() pair           {}",
        Spread::of(clock).say()
    );

    // 2. The trace: what one recorded call costs on top of the call.
    let trace = Trace::new();
    trace.start();
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
    let tracing = TracingStore::new(Arc::clone(&store), Arc::clone(&trace));
    let mut traced = Vec::new();
    let mut bare = Vec::new();
    for _ in 0..200 {
        let at = Instant::now();
        let _ = tracing.unfinished_jobs();
        traced.push(at.elapsed());
        let at = Instant::now();
        let _ = store.unfinished_jobs();
        bare.push(at.elapsed());
    }
    let _ = trace.stop();
    let traced = Spread::of(traced);
    let bare = Spread::of(bare);
    println!("  empty unfinished_jobs, bare   {}", bare.say());
    println!("  the same through the trace    {}", traced.say());
    println!(
        "  => trace overhead per call    {}",
        fmt(traced.median.saturating_sub(bare.median))
    );

    // 3. fsync, the three a `record_waiting` pays (store.rs:2019-2021 ->
    //    durable::write_file_with, which syncs the file and then the
    //    directory, and durable::append_line, which syncs the data).
    let scratch = dir.path().join("floor");
    fs::create_dir_all(&scratch).expect("a scratch directory");
    let mut file_sync = Vec::new();
    let mut dir_sync = Vec::new();
    let mut append_sync = Vec::new();
    for i in 0..100 {
        let path = scratch.join(format!("f{i}"));
        let at = Instant::now();
        let mut f = fs::File::create(&path).expect("creates");
        f.write_all(b"{\"a\":1}").expect("writes");
        f.sync_all().expect("syncs");
        file_sync.push(at.elapsed());

        let at = Instant::now();
        fs::File::open(&scratch)
            .expect("opens")
            .sync_all()
            .expect("syncs");
        dir_sync.push(at.elapsed());

        let at = Instant::now();
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(scratch.join("events"))
            .expect("opens");
        f.write_all(b"line\n").expect("writes");
        f.sync_data().expect("syncs");
        append_sync.push(at.elapsed());
    }
    let file_sync = Spread::of(file_sync);
    let dir_sync = Spread::of(dir_sync);
    let append_sync = Spread::of(append_sync);
    println!("  create+write+sync_all         {}", file_sync.say());
    println!("  sync_dir (open+sync_all)      {}", dir_sync.say());
    println!("  append+sync_data              {}", append_sync.say());
    println!(
        "  => the fsync floor under one record_waiting: {}",
        fmt(file_sync.median + dir_sync.median + append_sync.median)
    );

    // 4. A warm record read, which is what A and B are made of.
    let corpus = Corpus::build(200, 0);
    corpus.warm();
    let one = corpus.ids[100].clone();
    let mut read = Vec::new();
    for _ in 0..200 {
        let at = Instant::now();
        let _ = corpus.store.job(&one);
        read.push(at.elapsed());
    }
    println!("  one warm record read          {}", Spread::of(read).say());

    // 5. An uncontended mutex round trip: the unit post D is counted in.
    let lock = Mutex::new(0u64);
    let mut taken = Vec::new();
    for _ in 0..200 {
        let at = Instant::now();
        {
            let mut held = lock.lock().unwrap_or_else(|p| p.into_inner());
            *held += 1;
        }
        taken.push(at.elapsed());
    }
    println!(
        "  uncontended mutex round trip  {}",
        Spread::of(taken).say()
    );

    // 6. First touch against warm: is "warm" a real state on this host?
    println!("\n  first touch vs warm, collections_owed:");
    for t in [1_000usize, 20_000] {
        let corpus = Corpus::build(50, t);
        let at = Instant::now();
        let _ = JobStore::collections_owed(corpus.store.as_ref());
        let first = at.elapsed();
        let warm = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let _ = JobStore::collections_owed(corpus.store.as_ref());
                    at.elapsed()
                })
                .collect(),
        );
        println!(
            "    T={t:<6} first {:>10}  warm {:>10}  first/warm {:.2}",
            fmt(first),
            fmt(warm),
            first.as_secs_f64() / warm.as_secs_f64()
        );
    }
    println!(
        "  note: the page cache cannot be dropped without root, so \"cold\" here \
         means \"written by this process a moment ago\", which is the warmest \
         cold there is. A hub restarted onto an old state root is colder."
    );
}

// ---------------------------------------------------------------------------
// Phase 1: each post alone, swept.
// ---------------------------------------------------------------------------

/// Posts A, B and C alone, each against the axis that moves it.
///
/// Solo and single-threaded, so there is no lock wait in these figures by
/// construction: this is the *work* half of the separation Alexa asked for,
/// and phase 2 measures the *waiting* half against it.
#[test]
#[ignore = "a bench; run it on purpose"]
fn each_post_alone() {
    println!("\n== phase 1a: post A, historical reads, against T ==");
    println!("     N      T    dirs | collections_owed");
    for (n, t) in [
        (50usize, 0usize),
        (50, 1_000),
        (50, 5_000),
        (50, 20_000),
        (1_000, 0),
        (1_000, 20_000),
    ] {
        let corpus = Corpus::build(n, t);
        corpus.warm();
        let dirs = corpus.directories();
        let samples: Vec<Duration> = (0..7)
            .map(|_| {
                let at = Instant::now();
                let _ = JobStore::collections_owed(corpus.store.as_ref());
                at.elapsed()
            })
            .collect();
        let spread = Spread::of(samples);
        println!(
            "{n:>6} {t:>6} {dirs:>7} | {}  ({:>7} per dir)",
            spread.say(),
            fmt(spread.median / dirs.max(1) as u32)
        );
    }

    println!("\n== phase 1b: post B, open-job reads, against N (T = 0) ==");
    println!("     N | unfinished_jobs");
    for n in [50usize, 200, 1_000, 4_000] {
        let corpus = Corpus::build(n, 0);
        corpus.warm();
        let samples: Vec<Duration> = (0..7)
            .map(|_| {
                let at = Instant::now();
                let _ = corpus.store.unfinished_jobs();
                at.elapsed()
            })
            .collect();
        let spread = Spread::of(samples);
        println!(
            "{n:>6} | {}  ({:>7} per open job)",
            spread.say(),
            fmt(spread.median / n.max(1) as u32)
        );
    }

    println!("\n== phase 1c: post B is flat in T ==");
    println!("     N      T | unfinished_jobs");
    for t in [0usize, 20_000] {
        let corpus = Corpus::build(200, t);
        corpus.warm();
        let samples: Vec<Duration> = (0..7)
            .map(|_| {
                let at = Instant::now();
                let _ = corpus.store.unfinished_jobs();
                at.elapsed()
            })
            .collect();
        println!("{:>6} {t:>6} | {}", 200, Spread::of(samples).say());
    }

    println!("\n== phase 1d: post C, durable writes, against N ==");
    println!("  one record_waiting that is news, and one that is not (D171)");
    println!("     N |         news per write |     no news per write | whole pass (news)");
    for n in [50usize, 200, 1_000] {
        let corpus = Corpus::build(n, 0);
        corpus.warm();

        // News: a distinct cause per job, which is what a pass that found a
        // different fleet than the last one writes.
        let mut news = Vec::with_capacity(n);
        let at = Instant::now();
        for (i, id) in corpus.ids.iter().enumerate() {
            let one = Instant::now();
            let wrote = corpus
                .store
                .record_waiting(id, &format!("it is waiting because cause {i}"))
                .expect("the wait records");
            news.push(one.elapsed());
            assert!(wrote, "a distinct cause is news");
        }
        let whole = at.elapsed();

        // No news: the same cause again, which is what every pass after the
        // first writes while the fleet does not change (D171).
        let mut quiet = Vec::with_capacity(n);
        for (i, id) in corpus.ids.iter().enumerate() {
            let one = Instant::now();
            let wrote = corpus
                .store
                .record_waiting(id, &format!("it is waiting because cause {i}"))
                .expect("the wait records");
            quiet.push(one.elapsed());
            assert!(!wrote, "the same cause is not news");
        }

        let news = Spread::of(news);
        let quiet = Spread::of(quiet);
        println!("{n:>6} | {}", news.say());
        println!("       | {}  <- no news", quiet.say());
        println!(
            "       | whole pass of {n} news writes: {}  ({} per job)",
            fmt(whole),
            fmt(whole / n as u32)
        );
    }

    println!("\n== phase 1e: post E, the dispatch computation, against N ==");
    println!("  the O(N) and O(N log N) parts of dispatch_a_pass that are public:");
    println!("  queue::order + matcher::holdings + matcher::reserved + one");
    println!("  standing/what_is_ahead/waiting_sentence_behind per record.");
    println!("     N |      order |   holdings |   reserved |  sentences |      total");
    for n in [50usize, 200, 1_000, 4_000] {
        let corpus = Corpus::build(n, 0);
        corpus.warm();
        let records = corpus.store.unfinished_jobs().expect("the queue reads");
        let now = herd_hub::job::Timestamp::now();
        let step = herd_hub::AGING_STEP;

        let order = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let ordered = herd_hub::queue::order(records.clone(), now, step);
                    let took = at.elapsed();
                    std::hint::black_box(ordered);
                    took
                })
                .collect(),
        );
        let queue = herd_hub::queue::order(records.clone(), now, step);
        let holdings = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    std::hint::black_box(herd_hub::matcher::holdings(&records));
                    at.elapsed()
                })
                .collect(),
        );
        let reserved = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    std::hint::black_box(herd_hub::matcher::reserved(&records));
                    at.elapsed()
                })
                .collect(),
        );
        let sentences = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    for (place, record) in queue.iter().enumerate() {
                        let standing = herd_hub::queue::standing(record, now, step);
                        let ahead = herd_hub::queue::what_is_ahead(&queue, place, now, step);
                        std::hint::black_box(herd_hub::queue::waiting_sentence_behind(
                            place + 1,
                            queue.len(),
                            &standing,
                            &ahead,
                            "it is waiting because no runner offers what it needs",
                        ));
                    }
                    at.elapsed()
                })
                .collect(),
        );
        println!(
            "{n:>6} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
            fmt(order),
            fmt(holdings),
            fmt(reserved),
            fmt(sentences),
            fmt(order + holdings + reserved + sentences)
        );
    }
    println!(
        "  (`matcher::place` is left out: it needs a `Seat`, which only a live\n   \
         fleet builds. Phase 2 measures post E whole, on a real junction, and\n   \
         this table is the cross-check on its bulk.)"
    );
}

// ---------------------------------------------------------------------------
// Phase 1f: post D, measured as waiting and not as slowness.
// ---------------------------------------------------------------------------

/// The store's own `Mutex<Live>`, measured against an identical workload that
/// does not take it.
///
/// `collections_owed` takes and releases the store lock **once per directory**
/// (`store.rs:2378` -> `collection_owed` -> `JobStore::job` -> `self.lock()`,
/// `store.rs:2255`), while `unfinished_jobs` holds it for its whole O(N) read
/// (`store.rs:2313`). So the two contend, and the question is how much of
/// that is the lock rather than the disk.
///
/// The control is the same I/O without the mutex: `durable::names_in` plus one
/// `fs::read_to_string` per directory, which is what `collection_owed` reads.
/// If the control scales with the callers and the store does not, the gap is
/// the lock.
#[test]
#[ignore = "a bench; run it on purpose"]
fn lock_wait_under_contention() {
    println!("\n== phase 1f: post D, the store's Mutex<Live> ==");
    for (n, t) in [(1_000usize, 0usize), (50, 20_000)] {
        let corpus = Corpus::build(n, t);
        corpus.warm();
        let jobs = corpus.dir.path().join("jobs");
        println!("\n  N = {n}, T = {t}, {} directories", corpus.directories());
        println!("  callers |  through the store |  without the mutex | store/control");

        let mut store_one = Duration::ZERO;
        let mut control_one = Duration::ZERO;
        for callers in [1usize, 2, 4, 8] {
            let through = concurrent_slowest(callers, 3, {
                let store = Arc::clone(&corpus.store);
                Arc::new(move || {
                    let _ = JobStore::collections_owed(store.as_ref());
                })
            });
            let control = concurrent_slowest(callers, 3, {
                let jobs = jobs.clone();
                Arc::new(move || {
                    let names = herd_hub::durable::names_in(&jobs).expect("lists");
                    for name in &names {
                        let _ = fs::read_to_string(jobs.join(name).join(RECORD_FILE));
                    }
                })
            });
            if callers == 1 {
                store_one = through;
                control_one = control;
            }
            println!(
                "  {callers:>7} | {:>18} | {:>18} | x{:.2} vs x{:.2}",
                fmt(through),
                fmt(control),
                through.as_secs_f64() / store_one.as_secs_f64(),
                control.as_secs_f64() / control_one.as_secs_f64()
            );
        }
        println!(
            "  => at 4 callers, the lock wait inside one collections_owed is\n     \
             (store at 4) - (store at 1) minus whatever the control also lost."
        );

        // The mix a real hub makes: one junction walking the history while the
        // watchdog reads the queue (server.rs:2459, ticked from :5050).
        println!("\n  the real mix: collections_owed beside unfinished_jobs");
        let solo_owed = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let _ = JobStore::collections_owed(corpus.store.as_ref());
                    at.elapsed()
                })
                .collect(),
        );
        let solo_queue = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let _ = corpus.store.unfinished_jobs();
                    at.elapsed()
                })
                .collect(),
        );
        let gate = Arc::new(Barrier::new(2));
        let store_a = Arc::clone(&corpus.store);
        let gate_a = Arc::clone(&gate);
        let owed_hand = thread::spawn(move || {
            gate_a.wait();
            let at = Instant::now();
            let _ = JobStore::collections_owed(store_a.as_ref());
            at.elapsed()
        });
        let store_b = Arc::clone(&corpus.store);
        let gate_b = Arc::clone(&gate);
        let queue_hand = thread::spawn(move || {
            gate_b.wait();
            let at = Instant::now();
            let _ = store_b.unfinished_jobs();
            at.elapsed()
        });
        let owed_together = owed_hand.join().expect("the walk returns");
        let queue_together = queue_hand.join().expect("the queue read returns");
        println!(
            "    collections_owed alone {:>10}  beside a queue read {:>10}  (+{})",
            fmt(solo_owed),
            fmt(owed_together),
            fmt(owed_together.saturating_sub(solo_owed))
        );
        println!(
            "    unfinished_jobs  alone {:>10}  beside a history walk {:>9}  (+{})",
            fmt(solo_queue),
            fmt(queue_together),
            fmt(queue_together.saturating_sub(solo_queue))
        );
    }
}

/// `callers` threads enter `work` at the same moment; the slowest one's wall,
/// medianed over `rounds`.
fn concurrent_slowest(
    callers: usize,
    rounds: usize,
    work: Arc<dyn Fn() + Send + Sync>,
) -> Duration {
    let mut seen = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let gate = Arc::new(Barrier::new(callers));
        let hands: Vec<_> = (0..callers)
            .map(|_| {
                let work = Arc::clone(&work);
                let gate = Arc::clone(&gate);
                thread::spawn(move || {
                    gate.wait();
                    let at = Instant::now();
                    work();
                    at.elapsed()
                })
            })
            .collect();
        seen.push(
            hands
                .into_iter()
                .map(|h| h.join().expect("the caller returns"))
                .max()
                .expect("at least one caller"),
        );
    }
    median(seen)
}

// ---------------------------------------------------------------------------
// Phase 2: one real junction, broken into five.
// ---------------------------------------------------------------------------

/// What one junction's trace adds up to.
#[derive(Default, Clone)]
struct Breakdown {
    posts: BTreeMap<Post, Duration>,
    other: Duration,
    window: Duration,
    calls: BTreeMap<&'static str, u64>,
}

/// Cut a trace into the five posts, for the thread that walked the history.
///
/// The window opens at the `collections_owed` the junction makes
/// (`server.rs:2415`) and closes at the last store call that same thread made
/// before `settled`. Everything in between is either inside a store call —
/// which names its post — or between two of them:
///
/// - the one gap between `collections_owed` and the first `unfinished_jobs`
///   is where `dispatch_queued_jobs` takes `state.dispatching`
///   (`server.rs:2493`), so it is post D;
/// - every later gap is inside the pass with that lock already held, so it is
///   post E.
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

/// One real junction: a hub on a loopback port, a real runner dialling it,
/// and the five posts read off the store's own trace.
///
/// `N` open jobs that nothing can run, so the junction's pass places nothing
/// and writes `N` waits — which is both the deepest half of the cost and the
/// cheapest experiment to run on a shared host: no job is ever executed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a bench; run it on purpose"]
async fn one_real_junction_broken_down() {
    let rounds: usize = std::env::var("HERD_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    println!("\n== phase 2: one real runner junction, five posts, {rounds} rounds ==");

    let grid = [
        (50usize, 0usize),
        (200, 0),
        (1_000, 0),
        (50, 5_000),
        (50, 20_000),
        (1_000, 20_000),
    ];
    // `HERD_ONLY_T0=1` keeps the three cells that vary `N` alone: the cheap
    // grid a reproducibility run repeats when the host is only briefly quiet.
    let only_t0 = std::env::var("HERD_ONLY_T0").is_ok();
    for (n, t) in grid {
        if only_t0 && t != 0 {
            continue;
        }
        let mut per_post: BTreeMap<Post, Vec<Duration>> = BTreeMap::new();
        let mut windows: Vec<Duration> = Vec::new();
        let mut others: Vec<Duration> = Vec::new();
        let mut call_counts: BTreeMap<&'static str, u64> = BTreeMap::new();

        for _round in 0..rounds {
            let Some(one) = one_junction(n, t).await else {
                println!("{n:>6} {t:>6} | a round produced no trace; skipped");
                continue;
            };
            for (post, took) in &one.posts {
                per_post.entry(*post).or_default().push(*took);
            }
            windows.push(one.window);
            others.push(one.other);
            for (method, count) in &one.calls {
                *call_counts.entry(method).or_insert(0) += count;
            }
        }

        println!("\n  --- N = {n}, T = {t} ---");
        println!("    store calls per junction, summed over {rounds} rounds: {call_counts:?}");
        let mut accounted = Duration::ZERO;
        for post in [
            Post::Historical,
            Post::Open,
            Post::Write,
            Post::LockWait,
            Post::Compute,
        ] {
            let samples = per_post.remove(&post).unwrap_or_default();
            if samples.is_empty() {
                println!("    {:<22} (never reached)", post.label());
                continue;
            }
            let spread = Spread::of(samples);
            accounted += spread.median;
            println!("    {:<22} {}", post.label(), spread.say());
        }
        let others = Spread::of(others);
        let windows = Spread::of(windows);
        println!("    {:<22} {}", "  unclassified calls", others.say());
        println!("    {:<22} {}", "= junction window", windows.say());
        println!(
            "    (medians sum to {} against a window of {}; medians of parts \
             need not sum to the median of the whole)",
            fmt(accounted + others.median),
            fmt(windows.median)
        );
    }
}

/// One junction, traced. `None` if the pass never settled.
async fn one_junction(n: usize, t: usize) -> Option<Breakdown> {
    let corpus = Corpus::build(n, t);
    corpus.warm();
    let trace = Trace::new();
    let store = Arc::new(TracingStore::new(
        Arc::clone(&corpus.store),
        Arc::clone(&trace),
    ));
    let token = store
        .approve_runner("breakdown-runner")
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

    let data_root = support::DataRootDir::new("breakdown");
    let config = herd_runner::Config {
        hub_url,
        name: Some("breakdown-runner".to_owned()),
        token: Some(token),
        confinement: support::confinement(&data_root),
        ..herd_runner::Config::default()
    };

    // Recording starts here, so the corpus's own writes are not in the trace.
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

    // The pass is over when it has said so to every job it left queued: one
    // `record_waiting` each (`server.rs:2640` -> `store.rs:2007`). Counted
    // **in the trace** rather than by reading a record, because a reader
    // takes the store's `Mutex<Live>` (`store.rs:2255`) and would therefore
    // be measuring itself: a 1 ms poll against an 8 ms write is a third
    // caller in an experiment about two.
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
    break_down(&calls, settled)
}

// ---------------------------------------------------------------------------
// Phase 3: two runners at once, which is the only way post D is not zero.
// ---------------------------------------------------------------------------

/// Two runners dialling together, so that the second one's pass has to wait
/// for the first one's.
///
/// This is the measurement post D exists for: `state.dispatching`
/// (`server.rs:2493`) is held from before `unfinished_jobs` until after the
/// last `record_waiting`, so the second junction's gap between its
/// `collections_owed` and its `unfinished_jobs` **is** the time it spent
/// blocked. Read off the trace, from outside, with nothing added to the hub.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a bench; run it on purpose"]
async fn two_runners_at_once() {
    let rounds: usize = std::env::var("HERD_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    println!("\n== phase 3: two junctions at once; post D on the second ==");
    println!("     N      T | first junction's window | second's lock wait | second's window");

    for (n, t) in [(50usize, 0usize), (200, 0), (1_000, 0), (200, 20_000)] {
        let mut first_window = Vec::new();
        let mut second_wait = Vec::new();
        let mut second_window = Vec::new();

        for _round in 0..rounds {
            let corpus = Corpus::build(n, t);
            corpus.warm();
            let trace = Trace::new();
            let store = Arc::new(TracingStore::new(
                Arc::clone(&corpus.store),
                Arc::clone(&trace),
            ));
            let one = store
                .approve_runner("breakdown-one")
                .expect("a token for the first runner");
            let two = store
                .approve_runner("breakdown-two")
                .expect("a token for the second runner");

            let hub = herd_hub::Hub::bind_with_policy(
                "127.0.0.1:0".parse().expect("a literal loopback address"),
                Arc::clone(&store) as Arc<dyn JobStore>,
                herd_hub::Policy::default(),
            )
            .await
            .expect("the hub binds a free port");
            let hub_url = format!("ws://{}", hub.local_addr());
            let serving = tokio::spawn(async move {
                let _ = hub.serve().await;
            });

            let root_one = support::DataRootDir::new("breakdown-one");
            let root_two = support::DataRootDir::new("breakdown-two");
            let config_one = herd_runner::Config {
                hub_url: hub_url.clone(),
                name: Some("breakdown-one".to_owned()),
                token: Some(one),
                confinement: support::confinement(&root_one),
                ..herd_runner::Config::default()
            };
            let config_two = herd_runner::Config {
                hub_url,
                name: Some("breakdown-two".to_owned()),
                token: Some(two),
                confinement: support::confinement(&root_two),
                ..herd_runner::Config::default()
            };

            trace.start();
            let running_one = tokio::spawn(async move {
                let _ = herd_runner::run(config_one).await;
            });
            let running_two = tokio::spawn(async move {
                let _ = herd_runner::run(config_two).await;
            });

            // Two junctions have happened when two threads have each walked
            // the history and then read the queue.
            let deadline = Instant::now() + Duration::from_secs(180);
            let mut taken: Option<Vec<Call>> = None;
            loop {
                let calls = {
                    let held = trace.calls.lock().unwrap_or_else(|p| p.into_inner());
                    held.clone()
                };
                let junctions = junction_threads(&calls);
                if junctions.len() >= 2 {
                    // Give the slower pass room to finish its writes.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let held = trace.calls.lock().unwrap_or_else(|p| p.into_inner());
                    taken = Some(held.clone());
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let _ = trace.stop();
            running_one.abort();
            running_two.abort();
            serving.abort();
            drop(root_one);
            drop(root_two);

            let Some(calls) = taken else { continue };
            let mut windows = junction_windows(&calls);
            if windows.len() < 2 {
                continue;
            }
            windows.sort_by_key(|w| w.0);
            first_window.push(windows[0].1);
            second_wait.push(windows[1].2);
            second_window.push(windows[1].1);
        }

        if first_window.is_empty() {
            println!("{n:>6} {t:>6} | no round produced two junctions");
            continue;
        }
        println!(
            "{n:>6} {t:>6} | {:>23} | {:>18} | {}",
            fmt(median(first_window)),
            fmt(median(second_wait.clone())),
            fmt(median(second_window))
        );
        println!(
            "         second's lock wait, spread: {}",
            Spread::of(second_wait).say()
        );
    }
}

/// The threads that made a `collections_owed` followed by an `unfinished_jobs`
/// — that is, the threads a junction ran on.
fn junction_threads(calls: &[Call]) -> Vec<ThreadId> {
    let mut found = Vec::new();
    for (at, call) in calls.iter().enumerate() {
        if call.method != "collections_owed" {
            continue;
        }
        let has_pass = calls[at..]
            .iter()
            .any(|c| c.thread == call.thread && c.method == "unfinished_jobs");
        if has_pass && !found.contains(&call.thread) {
            found.push(call.thread);
        }
    }
    found
}

/// Per junction: when it opened, how long its window was, and how long it
/// spent between the history walk and the queue read — which, with the pass
/// lock taken in between, is how long it was blocked.
fn junction_windows(calls: &[Call]) -> Vec<(Instant, Duration, Duration)> {
    let mut out = Vec::new();
    for thread in junction_threads(calls) {
        let mine: Vec<&Call> = calls.iter().filter(|c| c.thread == thread).collect();
        let Some(owed) = mine.iter().find(|c| c.method == "collections_owed") else {
            continue;
        };
        let Some(queue) = mine
            .iter()
            .find(|c| c.method == "unfinished_jobs" && c.enter >= owed.exit)
        else {
            continue;
        };
        let last = mine
            .iter()
            .filter(|c| c.enter >= owed.enter)
            .map(|c| c.exit)
            .max()
            .unwrap_or(queue.exit);
        out.push((
            owed.enter,
            last.saturating_duration_since(owed.enter),
            queue.enter.saturating_duration_since(owed.exit),
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// A third method on post C: count the syncs rather than time them.
// ---------------------------------------------------------------------------

/// How many `fsync`/`fdatasync` calls one `record_waiting` makes.
///
/// Not answerable from inside the process, so this case exists to be run
/// twice under `strace -c` with `HERD_WRITES` set to two different values;
/// the difference over the difference is the answer, and the corpus the two
/// runs build is identical and cancels:
///
/// ```sh
/// for k in 1 101; do
///   HERD_WRITES=$k strace -f -c -e trace=fsync,fdatasync \
///     target/release/deps/junction_breakdown-<hash> \
///     --exact --ignored --nocapture syncs_per_record_waiting
/// done
/// ```
#[test]
#[ignore = "a bench; run it on purpose, under strace"]
fn syncs_per_record_waiting() {
    let writes: usize = std::env::var("HERD_WRITES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let corpus = Corpus::build(writes, 0);
    corpus.warm();
    // `HERD_SKIP_WRITES=1` builds the identical corpus and makes no write:
    // the control run whose syscalls cancel the corpus out of the answer.
    let skip = std::env::var("HERD_SKIP_WRITES").is_ok();
    for (i, id) in corpus.ids.iter().enumerate() {
        if skip {
            break;
        }
        let wrote = corpus
            .store
            .record_waiting(id, &format!("cause {i}"))
            .expect("the wait records");
        assert!(wrote, "a distinct cause is news");
    }
    println!("HERD_WRITES={writes}: {writes} record_waiting call(s) that were news");
}
