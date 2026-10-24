//! Experiment 6.1: the cost surface of one hub dispatch pass.
//!
//! This file measures, it does not assert a product behaviour. Every case is
//! `#[ignore]`d so the workspace suite stays the suite; run it on purpose:
//!
//! ```sh
//! cargo test -p herd-hub --test cost_surface -- --ignored --nocapture
//! ```
//!
//! What is being decided is whether taking persistence out of the reactor is
//! urgent or merely correct, and the way to decide it is to find which term
//! of `worst_turn(N, T, d, W)` dominates:
//!
//! - `N` — open jobs, the depth of the queue;
//! - `T` — **closed** job directories still sitting under `jobs/`, varied
//!   independently of `N`;
//! - `d` — the latency of one record access, which is what a persistence
//!   layer that is not a local directory would add;
//! - `W` — tokio worker threads.
//!
//! The oracle is the **worst turn**: the longest stretch over which the
//! reactor cannot answer anything. It is sampled continuously by a probe task
//! on the same runtime, never inferred from a total.
//!
//! The three hot paths this covers, all of them synchronous work reached from
//! an async context:
//!
//! 1. `dispatch_a_pass` (`herd-hub/src/server.rs:2509`), whose first act is
//!    `store.unfinished_jobs()` — O(N) records;
//! 2. `collections_owed` (`herd-hub/src/store.rs:2367`), which lists **every**
//!    directory under `jobs/` — O(N + T) records — and is reached from
//!    `ask_for_the_files_still_owed` (`server.rs:2415`) at every runner
//!    junction, four lines before the pass at `server.rs:2419`;
//! 3. the watchdog, which reads the queue every second with no condition
//!    (`attempts_in_the_air`, `server.rs:2459`, called from `server.rs:5050`).

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use herd_hub::job::{
    Budget, ChunkLanded, CollectionOwed, JobEvent, JobMetrics, JobRecord, JobState, LogWrite,
    NewJob, Raise, Reading, Result as StoreResult, Submission, Sweep,
};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::{JobId, JobOutcome, LogStream, Priority, Requirements, StepRun};

/// The file every record read opens, and the one `read_stored` parses three
/// times over (`herd-hub/src/store.rs:1142`, `:1150`, `:1160`).
const RECORD_FILE: &str = "job.json";

// ---------------------------------------------------------------------------
// A store that costs what a slower persistence layer would cost.
// ---------------------------------------------------------------------------

/// A delegating [`JobStore`] that pays `per_access` for every **record**
/// touched, and counts what it was asked for.
///
/// Built from the shape of `StoreThatHoldsTheClaimOpen`
/// (`herd-hub/src/server.rs:6928`): everything is the real [`JobDirStore`],
/// with one thing added.
///
/// The unit of cost is one record, not one trait call, and that choice is the
/// whole point. `unfinished_jobs` is a single call that reads `N` records;
/// `collections_owed` is a single call that reads `N + T`. A wrapper that
/// slept once per call would make `d` a constant and would answer the
/// question by construction. Sleeping `d` once for the whole batch rather
/// than `d` per record is deliberate too: the reactor is blocked for the same
/// total either way, and one `thread::sleep` does not add a thousand
/// scheduler wakeups to the figure being measured.
struct CostlyStore {
    inner: Arc<JobDirStore>,
    /// What one record access costs.
    per_access: Duration,
    /// How many directories sit under `jobs/`, which is what
    /// `collections_owed` walks. Fixed at construction: the corpus is fixed
    /// too, and counting it again inside the wrapper would add the very I/O
    /// being measured.
    directories: usize,
    /// Trait calls, by method name.
    calls: Mutex<BTreeMap<&'static str, u64>>,
    /// Records this wrapper has charged for.
    records: AtomicU64,
}

impl CostlyStore {
    fn new(inner: Arc<JobDirStore>, per_access: Duration, directories: usize) -> Self {
        Self {
            inner,
            per_access,
            directories,
            calls: Mutex::new(BTreeMap::new()),
            records: AtomicU64::new(0),
        }
    }

    /// Charge for `records` record accesses and note the call.
    fn charge(&self, what: &'static str, records: usize) {
        *self
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(what)
            .or_insert(0) += 1;
        self.records.fetch_add(records as u64, Ordering::Relaxed);
        if !self.per_access.is_zero() && records > 0 {
            thread::sleep(self.per_access * records as u32);
        }
    }

    fn calls_to(&self, what: &str) -> u64 {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(what)
            .copied()
            .unwrap_or(0)
    }
}

impl JobStore for CostlyStore {
    fn state_root_id(&self) -> &str {
        self.inner.state_root_id()
    }
    fn approve_runner(&self, name: &str) -> StoreResult<String> {
        self.charge("approve_runner", 1);
        self.inner.approve_runner(name)
    }
    fn remove_runner(&self, name: &str) -> StoreResult<bool> {
        self.charge("remove_runner", 1);
        self.inner.remove_runner(name)
    }
    fn approved_runners(&self) -> StoreResult<Vec<herd_hub::roster::Approval>> {
        self.charge("approved_runners", 1);
        self.inner.approved_runners()
    }
    fn admit_runner(&self, name: &str, token: Option<&str>) -> herd_hub::roster::Admission {
        self.charge("admit_runner", 1);
        self.inner.admit_runner(name, token)
    }
    fn approved_runner_count(&self) -> usize {
        self.inner.approved_runner_count()
    }
    fn submit(&self, new: NewJob) -> StoreResult<Submission> {
        self.charge("submit", 1);
        self.inner.submit(new)
    }
    fn raise_priority(&self, id: &JobId, to: Priority) -> StoreResult<Raise> {
        self.charge("raise_priority", 1);
        self.inner.raise_priority(id, to)
    }
    fn record_waiting(&self, id: &JobId, reason: &str) -> StoreResult<bool> {
        self.charge("record_waiting", 1);
        self.inner.record_waiting(id, reason)
    }
    fn record_state(&self, id: &JobId, state: JobState) -> StoreResult<()> {
        self.charge("record_state", 1);
        self.inner.record_state(id, state)
    }
    fn record_reading(&self, id: &JobId, reading: Reading) -> StoreResult<()> {
        self.charge("record_reading", 1);
        self.inner.record_reading(id, reading)
    }
    fn reading_answer(&self, key: &str) -> StoreResult<Option<Requirements>> {
        self.charge("reading_answer", 1);
        self.inner.reading_answer(key)
    }
    fn remember_reading(&self, key: &str, answer: &Requirements) -> StoreResult<()> {
        self.charge("remember_reading", 1);
        self.inner.remember_reading(key, answer)
    }
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> StoreResult<()> {
        self.charge("record_metrics", 1);
        self.inner.record_metrics(id, metrics)
    }
    fn record_outputs(&self, id: &JobId, outputs: &herd_proto::JobOutputs) -> StoreResult<()> {
        self.charge("record_outputs", 1);
        self.inner.record_outputs(id, outputs)
    }
    fn job_outputs(&self, id: &JobId, with_bytes: bool) -> StoreResult<herd_proto::JobOutputs> {
        self.charge("job_outputs", 1);
        self.inner.job_outputs(id, with_bytes)
    }
    fn write_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> StoreResult<ChunkLanded> {
        self.charge("write_output_chunk", 1);
        self.inner.write_output_chunk(id, path, offset, data)
    }
    fn output_holdings(&self, id: &JobId) -> StoreResult<Vec<herd_proto::FileHeld>> {
        self.charge("output_holdings", 1);
        self.inner.output_holdings(id)
    }
    fn collections_owed(&self) -> StoreResult<Vec<CollectionOwed>> {
        // O(T): one record read per directory under `jobs/`, closed ones
        // included (`herd-hub/src/store.rs:2367`).
        self.charge("collections_owed", self.directories);
        self.inner.collections_owed()
    }
    fn collection_owed(&self, id: &JobId) -> StoreResult<Option<CollectionOwed>> {
        self.charge("collection_owed", 1);
        self.inner.collection_owed(id)
    }
    fn output_budget(&self) -> Budget {
        self.inner.output_budget()
    }
    fn note_outputs_fetched(&self, id: &JobId) -> StoreResult<()> {
        self.charge("note_outputs_fetched", 1);
        self.inner.note_outputs_fetched(id)
    }
    fn sweep_outputs(&self) -> StoreResult<Sweep> {
        self.charge("sweep_outputs", self.directories);
        self.inner.sweep_outputs()
    }
    fn read_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        take: usize,
    ) -> StoreResult<Option<Vec<u8>>> {
        self.charge("read_output_chunk", 1);
        self.inner.read_output_chunk(id, path, offset, take)
    }
    fn clear_output_parts(&self, id: &JobId) -> StoreResult<()> {
        self.charge("clear_output_parts", 1);
        self.inner.clear_output_parts(id)
    }
    fn begin_attempt(
        &self,
        id: &JobId,
        runner: &str,
        why: &str,
        parallelism: Option<u32>,
    ) -> StoreResult<u32> {
        self.charge("begin_attempt", 1);
        self.inner.begin_attempt(id, runner, why, parallelism)
    }
    fn record_step(&self, id: &JobId, attempt: u32, step: StepRun) -> StoreResult<()> {
        self.charge("record_step", 1);
        self.inner.record_step(id, attempt, step)
    }
    fn end_attempt(
        &self,
        id: &JobId,
        attempt: u32,
        reason: &str,
        requeue_code: Option<i32>,
    ) -> StoreResult<()> {
        self.charge("end_attempt", 1);
        self.inner.end_attempt(id, attempt, reason, requeue_code)
    }
    fn append_log(
        &self,
        id: &JobId,
        stream: LogStream,
        seq: u64,
        data: &[u8],
    ) -> StoreResult<LogWrite> {
        self.charge("append_log", 1);
        self.inner.append_log(id, stream, seq, data)
    }
    fn chunks_recorded(&self, id: &JobId) -> StoreResult<u64> {
        self.charge("chunks_recorded", 1);
        self.inner.chunks_recorded(id)
    }
    fn name_missing_log(
        &self,
        id: &JobId,
        first_missing: u64,
        last_missing: u64,
    ) -> StoreResult<()> {
        self.charge("name_missing_log", 1);
        self.inner.name_missing_log(id, first_missing, last_missing)
    }
    fn job(&self, id: &JobId) -> StoreResult<Option<JobRecord>> {
        self.charge("job", 1);
        self.inner.job(id)
    }
    fn quarantine_reason(&self, id: &JobId) -> StoreResult<Option<String>> {
        self.charge("quarantine_reason", 1);
        self.inner.quarantine_reason(id)
    }
    fn job_log(&self, id: &JobId) -> StoreResult<Option<Vec<u8>>> {
        self.charge("job_log", 1);
        self.inner.job_log(id)
    }
    fn job_events(&self, id: &JobId) -> StoreResult<Option<Vec<JobEvent>>> {
        self.charge("job_events", 1);
        self.inner.job_events(id)
    }
    fn unfinished_jobs(&self) -> StoreResult<Vec<JobRecord>> {
        // O(N): one record read per *open* job, from `live.pending`
        // (`herd-hub/src/store.rs:2312`). Charged on what it returns.
        let records = self.inner.unfinished_jobs()?;
        self.charge("unfinished_jobs", records.len());
        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// The corpus: N open jobs and T closed job directories, varied independently.
// ---------------------------------------------------------------------------

/// A state root holding `n` open jobs and `t` closed job directories.
///
/// The two are built by different means on purpose. The open jobs go through
/// `submit`, because they have to end up in the store's in-memory `pending`
/// list — that list is what `unfinished_jobs` walks. The closed ones are
/// written straight onto the disk as directories holding a finished
/// `job.json`, because what `collections_owed` walks is the **directory**,
/// and a closed job is exactly a directory the pending list has forgotten.
/// Minting twenty thousand jobs through `submit` and finishing each would
/// measure the same thing an hour later.
struct Corpus {
    dir: tempfile::TempDir,
    store: Arc<JobDirStore>,
    n: usize,
    t: usize,
}

impl Corpus {
    fn build(n: usize, t: usize) -> Self {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));

        // One finished job, to copy. Its record is a real record: minted by
        // the store, moved to a terminal state through the store.
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
        let template_path = store.job_dir(&template).join(RECORD_FILE);
        let template_text = fs::read_to_string(&template_path).expect("the template record reads");
        let digits = template.as_str().len() - 1;

        // N open jobs. Distinct commands, because two identical submissions
        // are one job (E-05) and a queue of one is not a queue of a thousand.
        for i in 0..n {
            store
                .submit(NewJob::needing(
                    vec!["sleep".to_owned(), format!("queued-{i}")],
                    support::labels(&["cost-surface-unobtainable"]),
                ))
                .expect("an open job is minted");
        }

        // T closed job directories, numbered far above anything the store
        // will mint, so nothing collides.
        let jobs = dir.path().join("jobs");
        for i in 0..t {
            let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
            let job_dir = jobs.join(&name);
            fs::create_dir_all(&job_dir).expect("a closed job directory");
            let text = template_text.replace(template.as_str(), &name);
            fs::write(job_dir.join(RECORD_FILE), text).expect("a closed job record");
        }

        Self { dir, store, n, t }
    }

    /// How many directories sit under `jobs/`: the open jobs, the template,
    /// and the closed ones.
    fn directories(&self) -> usize {
        fs::read_dir(self.dir.path().join("jobs"))
            .expect("the jobs directory lists")
            .count()
    }

    fn costly(&self, per_access: Duration) -> Arc<CostlyStore> {
        Arc::new(CostlyStore::new(
            Arc::clone(&self.store),
            per_access,
            self.directories(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Counting what the disk was asked for.
// ---------------------------------------------------------------------------

/// Read syscalls and bytes read by this process, from `/proc/self/io`.
///
/// Process-wide, so every figure taken from it is only honest while this
/// process is doing one thing. Every case below runs in a single `#[test]`
/// for that reason, and the reactor cases — which are deliberately
/// concurrent — do not use it.
fn io_counters() -> (u64, u64) {
    let text = fs::read_to_string("/proc/self/io").unwrap_or_default();
    let field = |name: &str| -> u64 {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.trim().parse().ok())
            .unwrap_or(0)
    };
    (field("syscr:"), field("rchar:"))
}

/// One measurement of a synchronous call: how long, how many read syscalls,
/// how many bytes.
struct Cost {
    wall: Duration,
    reads: u64,
    bytes: u64,
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    let (reads_before, bytes_before) = io_counters();
    let at = Instant::now();
    let value = f();
    let wall = at.elapsed();
    let (reads_after, bytes_after) = io_counters();
    (
        value,
        Cost {
            wall,
            reads: reads_after.saturating_sub(reads_before),
            bytes: bytes_after.saturating_sub(bytes_before),
        },
    )
}

/// The median of several runs, which is what the tables report: the host is
/// shared, so one sample is a sample of whatever else was running.
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

// ---------------------------------------------------------------------------
// The oracle: the worst turn.
// ---------------------------------------------------------------------------

/// The longest stretch over which the reactor could not answer, sampled
/// continuously.
///
/// A probe task on the same runtime asks for a 500 µs timer over and over and
/// records how late it was. That lateness **is** the thing being measured: a
/// synchronous call made from an async task holds a worker thread, and while
/// every worker is held nothing on that runtime runs — no timer, no frame, no
/// health report. Sampling it rather than dividing a total matters because
/// one 900 ms pass and nine 1 ms passes have the same total and are not the
/// same hub.
///
/// `callers` is how many async tasks make the synchronous call at once. One
/// is the quiet case. `workers` is the case the thirteen call sites of
/// `dispatch_queued_jobs` make reachable: a redial storm, a watchdog tick and
/// a submission landing together.
fn worst_turn(
    workers: usize,
    callers: usize,
    rounds: usize,
    work: Arc<dyn Fn() + Send + Sync>,
) -> Duration {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .expect("a runtime of its own");

    let worst = Arc::new(Mutex::new(Duration::ZERO));
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    let probe_worst = Arc::clone(&worst);
    let probe_running = Arc::clone(&running);
    runtime.spawn(async move {
        let tick = Duration::from_micros(500);
        while probe_running.load(Ordering::Relaxed) {
            let at = Instant::now();
            tokio::time::sleep(tick).await;
            let late = at.elapsed().saturating_sub(tick);
            let mut worst = probe_worst
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if late > *worst {
                *worst = late;
            }
        }
    });

    // Let the probe settle, then forget what it saw while it was settling.
    thread::sleep(Duration::from_millis(50));
    *worst
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Duration::ZERO;

    runtime.block_on(async {
        for _ in 0..rounds {
            // Every caller enters the synchronous call at the same moment,
            // which is what a redial storm looks like from the reactor's side.
            let gate = Arc::new(Barrier::new(callers));
            let mut handles = Vec::with_capacity(callers);
            for _ in 0..callers {
                let work = Arc::clone(&work);
                let gate = Arc::clone(&gate);
                handles.push(tokio::spawn(async move {
                    gate.wait();
                    work();
                }));
            }
            for handle in handles {
                let _ = handle.await;
            }
            // A breath between rounds, so the probe gets a turn and the next
            // round measures a stall and not the tail of the last one.
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    running.store(false, Ordering::Relaxed);
    let seen = *worst
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    runtime.shutdown_timeout(Duration::from_secs(2));
    seen
}

/// The noise floor of the probe on this host, with nothing blocking it.
fn idle_worst_turn(workers: usize) -> Duration {
    worst_turn(workers, 1, 10, Arc::new(|| {}))
}

// ---------------------------------------------------------------------------
// Case 1: what one pass costs, axis by axis.
// ---------------------------------------------------------------------------

/// `N` and `T` against the two reads every pass begins with.
///
/// Not the cartesian product. The two calls being measured touch disjoint
/// sets — `unfinished_jobs` walks the store's in-memory pending list
/// (`store.rs:2312`), `collections_owed` walks the directory
/// (`store.rs:2367`) — so there is no interaction term for a product to
/// find. One sweep of `N` at `T = 0`, one sweep of `T` at the shallowest `N`,
/// and one corner at both maxima to check that the two sweeps add up, says
/// everything the product would.
#[test]
#[ignore = "an experiment, not an assertion: run it on purpose"]
fn n_and_t_against_one_pass() {
    println!("\n== case 1: cost of the two reads a pass begins with, d = 0 ==");
    println!(
        "{:>6} {:>7} {:>7} | {:>12} {:>8} {:>10} | {:>12} {:>8} {:>10}",
        "N", "T", "dirs", "unfinished", "reads", "bytes", "owed", "reads", "bytes"
    );

    let mut grid: Vec<(usize, usize)> = Vec::new();
    for n in [50_usize, 200, 400, 1000] {
        grid.push((n, 0));
    }
    for t in [1000_usize, 5000, 20000] {
        grid.push((50, t));
    }
    grid.push((1000, 20000));

    for (n, t) in grid {
        let corpus = Corpus::build(n, t);
        let dirs = corpus.directories();

        // Warm, then measure. A cold page cache is a different experiment;
        // what a hub pays on its own state root is a warm one.
        let _ = corpus.store.unfinished_jobs();
        let _ = JobStore::collections_owed(corpus.store.as_ref());

        let (_, unfinished_cost) = measure(|| {
            corpus
                .store
                .unfinished_jobs()
                .expect("the queue reads")
                .len()
        });
        let unfinished_wall = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let _ = corpus.store.unfinished_jobs();
                    at.elapsed()
                })
                .collect(),
        );

        let (_, owed_cost) = measure(|| {
            JobStore::collections_owed(corpus.store.as_ref())
                .expect("the collections read")
                .len()
        });
        let owed_wall = median(
            (0..5)
                .map(|_| {
                    let at = Instant::now();
                    let _ = JobStore::collections_owed(corpus.store.as_ref());
                    at.elapsed()
                })
                .collect(),
        );

        println!(
            "{:>6} {:>7} {:>7} | {:>12?} {:>8} {:>10} | {:>12?} {:>8} {:>10}",
            corpus.n,
            corpus.t,
            dirs,
            unfinished_wall,
            unfinished_cost.reads,
            unfinished_cost.bytes,
            owed_wall,
            owed_cost.reads,
            owed_cost.bytes
        );
        let _ = unfinished_cost.wall;
        let _ = owed_cost.wall;
    }
    println!(
        "note: `read_stored` parses the same text three times (store.rs:1142, :1150, :1160), \
         so serde parses = 3 x records read."
    );
}

// ---------------------------------------------------------------------------
// Case 2: the worst turn, against N, T and W.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "an experiment, not an assertion: run it on purpose"]
fn worst_turn_against_n_t_and_w() {
    println!("\n== case 2: worst turn, d = 0 ==");
    for workers in [2_usize, 4] {
        println!(
            "  idle noise floor, W = {workers}: {:?}",
            idle_worst_turn(workers)
        );
    }
    println!(
        "{:>6} {:>7} {:>3} {:>8} | {:>14} | {:>14}",
        "N", "T", "W", "callers", "watchdog read", "junction"
    );

    for (n, t) in [(50_usize, 0_usize), (1000, 0), (50, 20000), (1000, 20000)] {
        let corpus = Corpus::build(n, t);
        let _ = corpus.store.unfinished_jobs();
        let _ = JobStore::collections_owed(corpus.store.as_ref());

        for workers in [2_usize, 4] {
            for callers in [1_usize, workers] {
                if callers == 1 && workers == 2 {
                    // Reported once per (N, T); the quiet case does not
                    // depend on how many workers are idle beside it.
                }
                let store = Arc::clone(&corpus.store);
                // What the watchdog does every second, unconditionally
                // (server.rs:2459, called from server.rs:5050).
                let watchdog: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = store.unfinished_jobs();
                });
                let watchdog_turn = worst_turn(workers, callers, 5, watchdog);

                // What a runner junction does: the collections it is still
                // owed (server.rs:2415), then the pass (server.rs:2419),
                // both synchronous, the pass behind the one `dispatching`
                // lock every pass in this hub takes (server.rs:2493).
                let store = Arc::clone(&corpus.store);
                let dispatching = Arc::new(Mutex::new(()));
                let junction: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = JobStore::collections_owed(store.as_ref());
                    let _one_pass = dispatching
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let _ = store.unfinished_jobs();
                });
                let junction_turn = worst_turn(workers, callers, 5, junction);

                println!(
                    "{:>6} {:>7} {:>3} {:>8} | {:>14?} | {:>14?}",
                    n, t, workers, callers, watchdog_turn, junction_turn
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Case 3: the d axis.
// ---------------------------------------------------------------------------

/// `d` at a fixed, deliberately small corpus.
///
/// Small because `d` is charged per record and the product is the point:
/// 20 ms against the 20 000 closed directories of case 1 would be a
/// seven-minute stall, which is a figure to compute and not a figure to sit
/// through. `N = 200, T = 0` keeps every cell under five seconds and the
/// coefficient is the same one.
#[test]
#[ignore = "an experiment, not an assertion: run it on purpose"]
fn d_against_the_worst_turn() {
    println!("\n== case 3: d, per record access, N = 200, T = 0, W = 4 ==");
    let corpus = Corpus::build(200, 0);
    let _ = corpus.store.unfinished_jobs();
    println!(
        "{:>6} {:>8} {:>9} | {:>14} | {:>14}",
        "d (ms)", "callers", "records", "watchdog read", "junction"
    );

    for d_ms in [0_u64, 1, 5, 20] {
        for callers in [1_usize, 4] {
            let store = corpus.costly(Duration::from_millis(d_ms));
            let reading = Arc::clone(&store);
            let watchdog: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = reading.unfinished_jobs();
            });
            let watchdog_turn = worst_turn(4, callers, 1, watchdog);

            let store = corpus.costly(Duration::from_millis(d_ms));
            let dispatching = Arc::new(Mutex::new(()));
            let reading = Arc::clone(&store);
            let junction: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = reading.collections_owed();
                let _one_pass = dispatching
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let _ = reading.unfinished_jobs();
            });
            let junction_turn = worst_turn(4, callers, 1, junction);

            println!(
                "{:>6} {:>8} {:>9} | {:>14?} | {:>14?}",
                d_ms,
                callers,
                store.records.load(Ordering::Relaxed),
                watchdog_turn,
                junction_turn
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Case 4: how often a live hub pays each of those costs.
// ---------------------------------------------------------------------------

/// Against a live hub, with a real runner dialling in: how many records each
/// junction and each watchdog tick asks the store for.
///
/// Case 1 says what one call costs; this says how often the hub makes it, and
/// that is the half of the answer no table of durations can supply. The jobs
/// in the queue need a capability nothing offers, so the queue stays deep and
/// nothing is ever placed — which is both what a deep queue is, and what
/// keeps this experiment from spawning fifty processes on a shared host.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "an experiment, not an assertion: run it on purpose"]
async fn how_often_a_live_hub_walks_the_disk() {
    println!("\n== case 4: calls a live hub makes, N = 50, T = 5000 ==");
    let corpus = Corpus::build(50, 5000);
    let store = corpus.costly(Duration::ZERO);
    let token = store
        .approve_runner("cost-surface-runner")
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

    let before_owed = store.calls_to("collections_owed");
    let before_queue = store.calls_to("unfinished_jobs");

    let data_root = support::DataRootDir::new("cost-surface");
    let config = herd_runner::Config {
        hub_url,
        name: Some("cost-surface-runner".to_owned()),
        token: Some(token),
        confinement: support::confinement(&data_root),
        ..herd_runner::Config::default()
    };
    let running = tokio::spawn(async move {
        let _ = herd_runner::run(config).await;
    });

    let deadline = Instant::now() + support::PATIENCE;
    while !handle.runner_connected() {
        assert!(Instant::now() < deadline, "the runner never joined the hub");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    println!(
        "one junction: collections_owed x{}, unfinished_jobs x{}, records charged {}",
        store.calls_to("collections_owed") - before_owed,
        store.calls_to("unfinished_jobs") - before_queue,
        store.records.load(Ordering::Relaxed)
    );

    let settled_owed = store.calls_to("collections_owed");
    let settled_queue = store.calls_to("unfinished_jobs");
    let records_settled = store.records.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_secs(5)).await;
    println!(
        "five idle seconds: collections_owed x{}, unfinished_jobs x{}, records charged {}",
        store.calls_to("collections_owed") - settled_owed,
        store.calls_to("unfinished_jobs") - settled_queue,
        store.records.load(Ordering::Relaxed) - records_settled
    );
    println!(
        "queue is still {} deep and nothing was placed",
        store.unfinished_jobs().expect("the queue reads").len()
    );

    running.abort();
    serving.abort();
    drop(data_root);
}
