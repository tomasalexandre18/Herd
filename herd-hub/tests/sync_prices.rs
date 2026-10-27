//! ORCH-077: the three durable operations of `record_waiting`, priced
//! **separately**, on both filesystem classes.
//!
//! `08-s0-unsynced-write-cost.md` priced the syncs as a block: all three on
//! against all three off, x95 apart. That is the size of the prize and not a
//! guide to collecting it, because the recommendation under discussion is to
//! keep **one** of the three and drop the other two. A block figure cannot
//! say what one sync costs, and three syncs over a journalling filesystem are
//! not three independent costs: the first may commit what the second would
//! have had to.
//!
//! So this file measures the lattice. One waiting write touches exactly three
//! durable operations, named here by the lines that perform them:
//!
//! | code | operation | where |
//! |---|---|---|
//! | **R** | `file.sync_all()` on the record's temporary | `durable.rs` `write_file_with` |
//! | **D** | `sync_dir(parent)` after the rename | `durable.rs` `write_file_with` |
//! | **E** | `file.sync_data()` on the appended event line | `durable.rs` `append_line` |
//!
//! **Since ORCH-083 the hub's own waiting write performs `RD-`, not `RDE`.**
//! D183 step A dropped **E** for the `waiting` line, so `record_waiting` now
//! goes through `durable::append_line_unsynced`; `durable::append_line` and
//! its `sync_data()` still carry every **transition** line. The lattice below
//! is a replica and is unchanged — all eight subsets are still timed, which
//! is what lets this file price a step before it is taken — but the control
//! row is now compared against `RD-` (`Syncs::STORE`).
//!
//! All eight subsets of `{R, D, E}` are timed over the same waiting write, so
//! the marginal price of each sync is readable both on its own and in the
//! presence of the others. `{R, D, E}` — every sync on — is the control: it
//! must land on `JobStore::record_waiting`'s own figure, which is what says
//! the replica is the same experiment as the hub.
//!
//! **Nothing under `herd-hub/src` is touched.** `record_waiting` is timed
//! from outside through the public `JobStore` trait; the eight variants are a
//! replica, written here, over the same two files in the same order with the
//! same bytes. The replica is derived from `herd-hub/tests/unsynced_write_cost.rs`
//! (S0), whose corpus, spread and statistics are reused unchanged so that a
//! figure here and a figure there compare without reinterpretation.
//!
//! Every case is `#[ignore]`d, so `cargo test --workspace` does not run it.
//!
//! The filesystem under the measurement is `$TMPDIR`'s, and each phase prints
//! the mount it actually landed on, so a pasted run says for itself which
//! class it describes:
//!
//! ```sh
//! # ext4 on NVMe — the class the deployment runs on (ORCH-053)
//! TMPDIR=/path/on/ext4 nix develop --command cargo test -p herd-hub \
//!   --test sync_prices --release -- --ignored --nocapture --test-threads=1
//! # ZFS on a SATA mirror — the class 08's figures were taken on; its fsync is
//! # bimodal, so the per-write median needs far more samples to be a figure
//! TMPDIR=/path/on/zfs  nix develop --command cargo test -p herd-hub \
//!   --test sync_prices --release -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `HERD_SAMPLES` overrides the sample count of whichever phase is running.

mod support;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use herd_hub::durable;
use herd_hub::job::{JobEvent, JobRecord, JobState, NewJob, Timestamp, RECORD_VERSION};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::JobId;

const RECORD_FILE: &str = "job.json";
const EVENTS_FILE: &str = "events.jsonl";

/// Samples for the primitive phase, where one sample is one sync call.
const FLOOR_SAMPLES: usize = 200;
/// Samples per subset for the lattice, where one sample is one whole waiting
/// write and needs a queued job of its own.
const LATTICE_SAMPLES: usize = 60;

/// How many samples to take, overridable per phase.
///
/// The default is enough on a filesystem whose `fsync` has a flat cost. It is
/// **not** enough on ZFS, where `06` and `08` both found one synchronised
/// write bimodal — it lands in a transaction group or waits for the next —
/// with an `iqr/med` around 180 %. There a per-write median is not a figure
/// until the sample count is raised well past the group interval, which is
/// what `HERD_SAMPLES` is for.
fn samples(default: usize) -> usize {
    std::env::var("HERD_SAMPLES")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// The two files a waiting write touches, as the store writes them.
// ---------------------------------------------------------------------------

/// `job.json`, with the version in front of the record — the shape of
/// `store.rs`'s private `RecordFile`, declared again because this file reads
/// and writes those bytes from outside the hub.
#[derive(Debug, Serialize, Deserialize)]
struct RecordFile {
    record_version: u32,
    #[serde(flatten)]
    record: JobRecord,
}

/// One line of `events.jsonl`, likewise the shape of `store.rs`'s `EventLine`.
#[derive(Debug, Serialize, Deserialize)]
struct EventLine {
    record_version: u32,
    #[serde(flatten)]
    event: JobEvent,
}

// ---------------------------------------------------------------------------
// Which of the three syncs are on.
// ---------------------------------------------------------------------------

/// A subset of the three durable operations of one waiting write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Syncs {
    /// **R** — `sync_all()` on the record's temporary, before the rename.
    record: bool,
    /// **D** — `sync_dir(parent)`, after the rename.
    dir: bool,
    /// **E** — `sync_data()` on the appended event line.
    event: bool,
}

impl Syncs {
    const NONE: Self = Self {
        record: false,
        dir: false,
        event: false,
    };
    const ALL: Self = Self {
        record: true,
        dir: true,
        event: true,
    };

    /// Which of the three the hub's own `record_waiting` performs **today**.
    ///
    /// `RDE` until ORCH-083; **`RD-`** since — D183 step A dropped **E**, the
    /// event `fdatasync`, for the `waiting` line alone, the record keeping
    /// both of its syncs. Named here so the control below compares the
    /// replica with the subset the store actually runs, instead of with a
    /// figure someone remembers.
    const STORE: Self = Self {
        record: true,
        dir: true,
        event: false,
    };

    /// The eight subsets, in order of how many syncs they keep: the two poles
    /// `08` measured first and last, and the six it did not measure between.
    fn lattice() -> [Self; 8] {
        let s = |record, dir, event| Self { record, dir, event };
        [
            s(false, false, false),
            s(true, false, false),
            s(false, true, false),
            s(false, false, true),
            s(true, true, false),
            s(true, false, true),
            s(false, true, true),
            s(true, true, true),
        ]
    }

    /// `R D E` with a dash for each sync that is off, so a table column sorts
    /// and reads at a glance.
    fn code(self) -> String {
        format!(
            "{}{}{}",
            if self.record { 'R' } else { '-' },
            if self.dir { 'D' } else { '-' },
            if self.event { 'E' } else { '-' }
        )
    }

    fn kept(self) -> usize {
        usize::from(self.record) + usize::from(self.dir) + usize::from(self.event)
    }

    /// The same subset with one sync forced off, for a leave-one-out price.
    fn without(self, which: Which) -> Self {
        let mut next = self;
        match which {
            Which::Record => next.record = false,
            Which::Dir => next.dir = false,
            Which::Event => next.event = false,
        }
        next
    }

    /// The empty subset with one sync forced on, for an only-one price.
    fn only(which: Which) -> Self {
        Self::NONE.with(which)
    }

    fn with(self, which: Which) -> Self {
        let mut next = self;
        match which {
            Which::Record => next.record = true,
            Which::Dir => next.dir = true,
            Which::Event => next.event = true,
        }
        next
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    Record,
    Dir,
    Event,
}

impl Which {
    const ALL: [Self; 3] = [Self::Record, Self::Dir, Self::Event];

    fn label(self) -> &'static str {
        match self {
            Self::Record => "R  record fsync (sync_all on the temporary)",
            Self::Dir => "D  directory fsync (sync_dir after the rename)",
            Self::Event => "E  event fdatasync (sync_data on the line)",
        }
    }
}

// ---------------------------------------------------------------------------
// The write path, with each sync independently on or off.
// ---------------------------------------------------------------------------

/// Makes every temporary name unique, as `durable` does for the real one.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

fn temporary_beside(path: &Path) -> PathBuf {
    let n = TEMPORARIES.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "record".to_owned());
    path.with_file_name(format!(".{name}.orch077.{}.{n}", std::process::id()))
}

/// `durable::write_file`, with its two syncs each independently skippable.
///
/// Byte for byte the same sequence as `durable.rs`'s `write_file_with` —
/// temporary beside the destination, fill, `sync_all`, rename, `sync_dir` —
/// with `sync_all` performed only if `syncs.record` and `sync_dir` only if
/// `syncs.dir`. The rename is never skipped: atomicity is the visibility
/// guarantee and is not one of the three costs being priced.
fn write_file_selectively(path: &Path, bytes: &[u8], syncs: Syncs) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("the record has no directory to live in"))?;
    let temporary = temporary_beside(path);
    let written = (|| -> io::Result<()> {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        if syncs.record {
            file.sync_all()?;
        }
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
        return written;
    }
    if syncs.dir {
        durable::sync_dir(parent)?;
    }
    Ok(())
}

/// `durable::append_line`, with its one sync skippable.
fn append_line_selectively(path: &Path, line: &str, syncs: Syncs) -> io::Result<()> {
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(record.as_bytes())?;
    if syncs.event {
        file.sync_data()?;
    }
    Ok(())
}

/// `record_waiting`, replicated from outside the hub, with any subset of its
/// three syncs.
///
/// Line for line what `store.rs`'s `record_waiting` does — read the record,
/// no-op unless the job is queued and the cause is news, stamp it, write the
/// record, then append the event — minus the store's `Mutex<Live>`, which
/// cannot be taken from here. For a single-threaded pass that changes
/// nothing, and the `RDE` row of the lattice is the control that says so.
fn replica_record_waiting(
    job_dir: &Path,
    reason: &str,
    syncs: Syncs,
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

    // The record first, then the event line: `store.rs`'s order, so that the
    // moment the line is readable the record agrees with it.
    write_file_selectively(&record_path, &bytes, syncs)?;
    append_line_selectively(&job_dir.join(EVENTS_FILE), &line, syncs)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// The corpus — `unsynced_write_cost.rs`'s, narrowed to what is needed here.
// ---------------------------------------------------------------------------

/// A state root holding `n` queued jobs, each of which can take exactly one
/// waiting write that is news.
struct Corpus {
    /// Held so the state root outlives the corpus; never read.
    _dir: tempfile::TempDir,
    store: Arc<JobDirStore>,
    ids: Vec<JobId>,
}

impl Corpus {
    fn build(n: usize) -> Self {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        let mut ids = Vec::with_capacity(n);
        for i in 0..n {
            ids.push(
                store
                    .submit(NewJob::needing(
                        vec!["sleep".to_owned(), format!("orch077-{i}")],
                        support::labels(&["orch077-unobtainable"]),
                    ))
                    .expect("an open job is minted")
                    .record()
                    .id
                    .clone(),
            );
        }
        Self {
            _dir: dir,
            store,
            ids,
        }
    }

    fn warm(&self) {
        let _ = self.store.unfinished_jobs();
    }

    fn root(&self) -> PathBuf {
        self._dir.path().to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// Statistics — `junction_breakdown.rs`'s and `unsynced_write_cost.rs`'s,
// unchanged, so a spread here reads as a spread there.
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

/// A signed difference between two medians, as a duration with its sign — a
/// marginal price that comes out negative is noise and must print as such
/// rather than underflowing to zero.
fn difference(bigger: Duration, smaller: Duration) -> String {
    if bigger >= smaller {
        fmt(bigger - smaller)
    } else {
        format!("-{}", fmt(smaller - bigger))
    }
}

fn load() -> String {
    fs::read_to_string("/proc/loadavg").unwrap_or_else(|_| "unknown".to_owned())
}

/// The mount a path landed on, with its filesystem type, device and options.
///
/// The whole measurement is a property of this line, so every phase prints
/// it: a figure from this file without its mount beside it says nothing, and
/// the two classes ORCH-077 compares differ only here. The options are
/// reported verbatim because `barrier=0` would invalidate every figure below
/// and is visible nowhere else.
fn mount_of(path: &Path) -> String {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (source, target, fstype, options) =
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
                _ => continue,
            };
        if !real.starts_with(target) {
            continue;
        }
        let depth = target.len();
        if best.as_ref().is_none_or(|(best, _)| depth > *best) {
            best = Some((
                depth,
                format!("{target} — {fstype} on {source} — {options}"),
            ));
        }
    }
    best.map_or_else(|| "unknown".to_owned(), |(_, said)| said)
}

/// The header every phase opens with, so one pasted run is self-describing.
fn say_where(root: &Path) {
    println!("  state root : {}", root.display());
    println!("  mount      : {}", mount_of(root));
    println!("  load before: {}", load().trim());
}

// ---------------------------------------------------------------------------
// Phase 1: each sync syscall on its own, with the write excluded.
// ---------------------------------------------------------------------------

/// The bare price of each of the three sync calls, with the write that
/// precedes it kept out of the timer.
///
/// This is the figure H-1 is about: whether one `fsync` on this class is tens
/// of microseconds or whole milliseconds. `08` reported `write`+`sync_all`
/// composed (1.19 ms on ZFS) and so could not separate the syscall from the
/// write under it; here the timer opens after the bytes are in the file and
/// closes when the sync returns.
///
/// **D is measured twice, and the pair is the point.** A directory sync that
/// follows a record sync may cost nothing, because the first call can already
/// have committed what the second would have had to write — on a filesystem
/// with one journal or one transaction group, the three syncs are not three
/// independent costs. So D is priced both in the real order (after R) and
/// alone (no R before it), and the two numbers say whether the ordering is
/// load-bearing. E likewise.
#[test]
#[ignore = "a bench; run it on purpose"]
fn each_sync_priced_alone() {
    println!("\n== ORCH-077 phase 1: each sync call on its own ==");
    let dir = tempfile::tempdir().expect("a temporary directory");
    say_where(dir.path());
    let count = samples(FLOOR_SAMPLES);
    println!("  {count} samples each; the preceding write is outside the timer");

    let body = vec![b'x'; 1024];
    let parent = dir.path();

    // R — sync_all on a freshly written 1 KiB temporary.
    let record_sync = Spread::of(
        (0..count)
            .map(|i| {
                let path = parent.join(format!("r-{i}"));
                let mut file = File::create(&path).expect("the temporary is created");
                file.write_all(&body).expect("the bytes are written");
                let at = Instant::now();
                file.sync_all().expect("the record sync lands");
                at.elapsed()
            })
            .collect(),
    );

    // D, in the real order — a record sync, a rename, then the directory sync.
    let dir_sync_after_record = Spread::of(
        (0..count)
            .map(|i| {
                let temporary = parent.join(format!("d-after-tmp-{i}"));
                let path = parent.join(format!("d-after-{i}"));
                let mut file = File::create(&temporary).expect("the temporary is created");
                file.write_all(&body).expect("the bytes are written");
                file.sync_all().expect("the record sync lands");
                drop(file);
                fs::rename(&temporary, &path).expect("the rename lands");
                let at = Instant::now();
                durable::sync_dir(parent).expect("the directory sync lands");
                at.elapsed()
            })
            .collect(),
    );

    // D, alone — the same rename with no record sync before it, so the
    // directory sync has the record's bytes still owed underneath it.
    let dir_sync_alone = Spread::of(
        (0..count)
            .map(|i| {
                let temporary = parent.join(format!("d-alone-tmp-{i}"));
                let path = parent.join(format!("d-alone-{i}"));
                let mut file = File::create(&temporary).expect("the temporary is created");
                file.write_all(&body).expect("the bytes are written");
                drop(file);
                fs::rename(&temporary, &path).expect("the rename lands");
                let at = Instant::now();
                durable::sync_dir(parent).expect("the directory sync lands");
                at.elapsed()
            })
            .collect(),
    );

    // E — sync_data on an appended event line, on a file that keeps growing
    // exactly as `events.jsonl` does.
    let line = "a line of about the size of an event line\n";
    let events = parent.join("events.jsonl");
    let event_sync = Spread::of(
        (0..count)
            .map(|_| {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&events)
                    .expect("the event log opens");
                file.write_all(line.as_bytes())
                    .expect("the line is written");
                let at = Instant::now();
                file.sync_data().expect("the event sync lands");
                at.elapsed()
            })
            .collect(),
    );

    println!("\n  sync call                                  | median and spread");
    println!("  {}", "-".repeat(100));
    println!(
        "  R  record fsync, sync_all on a 1 KiB file  | {}",
        record_sync.say()
    );
    println!(
        "  D  directory fsync, in the real order      | {}",
        dir_sync_after_record.say()
    );
    println!(
        "  D  directory fsync, with no R before it    | {}",
        dir_sync_alone.say()
    );
    println!(
        "  E  event fdatasync on an appended line     | {}",
        event_sync.say()
    );
    println!(
        "\n  the three, in the real order, summed: {}",
        fmt(record_sync.median + dir_sync_after_record.median + event_sync.median)
    );
    println!(
        "  what keeping R alone would pay      : {}",
        fmt(record_sync.median)
    );
    println!(
        "  D's price depends on R before it    : {} after R against {} alone",
        fmt(dir_sync_after_record.median),
        fmt(dir_sync_alone.median)
    );
}

// ---------------------------------------------------------------------------
// Phase 2: all eight subsets, over a whole waiting write.
// ---------------------------------------------------------------------------

/// Every subset of the three syncs, over the real `record_waiting` write, plus
/// the store's own figure as the control.
///
/// A fresh corpus per subset, because the first waiting write on a job is the
/// news and a second with the same cause is a no-op: every subset must meet an
/// untouched queue to be doing the same work. One write per job, one job per
/// sample.
///
/// The row that matters for the recommendation under discussion is `R--`
/// against `RDE`: what one waiting write costs if the record sync is kept and
/// the other two are dropped, against what it costs today.
#[test]
#[ignore = "a bench; run it on purpose"]
fn the_eight_subsets_of_one_waiting_write() {
    println!("\n== ORCH-077 phase 2: all eight subsets of one waiting write ==");
    let probe = tempfile::tempdir().expect("a temporary directory");
    say_where(probe.path());
    drop(probe);
    let count = samples(LATTICE_SAMPLES);
    println!("  {count} samples per subset, one queued job per sample");

    let mut medians: Vec<(Syncs, Duration)> = Vec::with_capacity(8);
    let mut rows: Vec<(String, usize, Spread)> = Vec::with_capacity(9);

    for syncs in Syncs::lattice() {
        let corpus = Corpus::build(count);
        if syncs == Syncs::NONE {
            println!("  corpus root: {}", mount_of(&corpus.root()));
        }
        corpus.warm();
        let samples: Vec<Duration> = corpus
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let job_dir = corpus.store.job_dir(id);
                let reason = format!("waiting because cause {i}");
                let at = Instant::now();
                let wrote = replica_record_waiting(&job_dir, &reason, syncs)
                    .expect("the replica records the wait");
                let took = at.elapsed();
                assert!(wrote, "a distinct cause is news");
                took
            })
            .collect();
        let spread = Spread::of(samples);
        medians.push((syncs, spread.median));
        rows.push((syncs.code(), syncs.kept(), spread));
    }

    // The control: the hub's own write path, on its own fresh corpus.
    //
    // **Since ORCH-083 (D183 step A) it keeps two syncs, not three**, so the
    // row it must land on is `RD-` and no longer `RDE`. The replica is
    // deliberately left with all eight subsets — the lattice is the
    // measurement and dropping a column from it would make this file say less
    // than it did — so the comparison below names the store's own subset
    // rather than assuming it.
    let corpus = Corpus::build(count);
    corpus.warm();
    let store_samples: Vec<Duration> = corpus
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let reason = format!("waiting because cause {i}");
            let at = Instant::now();
            let wrote = corpus
                .store
                .record_waiting(id, &reason)
                .expect("the store records the wait");
            let took = at.elapsed();
            assert!(wrote, "a distinct cause is news");
            took
        })
        .collect();
    let store = Spread::of(store_samples);

    println!("\n  subset | kept | one waiting write, median and spread");
    println!("  {}", "-".repeat(104));
    for (code, kept, spread) in &rows {
        println!("  {code:<6} |  {kept}   | {}", spread.say());
    }
    println!(
        "  {:<6} |  {}   | {}",
        "store",
        Syncs::STORE.kept(),
        store.say()
    );

    let median_of = |want: Syncs| {
        medians
            .iter()
            .find(|(syncs, _)| *syncs == want)
            .map(|(_, median)| *median)
            .expect("every subset was measured")
    };
    let none = median_of(Syncs::NONE);
    let all = median_of(Syncs::ALL);

    println!("\n  -- the control --");
    println!(
        "  {} replica {} against the store's {}: the replica is the same experiment",
        Syncs::STORE.code(),
        fmt(median_of(Syncs::STORE)),
        fmt(store.median)
    );
    println!(
        "  RDE, which the store performed before ORCH-083: {}",
        fmt(all)
    );

    println!("\n  -- each sync's marginal price, two ways --");
    println!("  sync                                        | added to nothing | added last to the other two");
    println!("  {}", "-".repeat(104));
    for which in Which::ALL {
        let only = median_of(Syncs::only(which));
        let without = median_of(Syncs::ALL.without(which));
        println!(
            "  {:<43} | {:>16} | {:>27}",
            which.label(),
            difference(only, none),
            difference(all, without)
        );
    }
    println!(
        "\n  Two columns that disagree mean the syncs are not independent: the\n  \
         first call pays for what a later one would have had to write."
    );

    println!("\n  -- the recommendation under discussion --");
    let keep_record_only = median_of(Syncs::only(Which::Record));
    println!("  all three syncs (today)          : {}", fmt(all));
    println!(
        "  the record sync alone (R--)      : {}",
        fmt(keep_record_only)
    );
    println!("  no sync at all (---)             : {}", fmt(none));
    let whole_prize = all.saturating_sub(none);
    let collected = all.saturating_sub(keep_record_only);
    println!(
        "  dropping D and E collects {} of the {} whole prize{}",
        fmt(collected),
        fmt(whole_prize),
        if whole_prize.is_zero() {
            String::new()
        } else {
            format!(
                " — {:.1} %",
                collected.as_secs_f64() / whole_prize.as_secs_f64() * 100.0
            )
        }
    );
    println!("  load after this phase: {}", load().trim());
}

// ---------------------------------------------------------------------------
// Phase 3: a whole pass, so the figure compares with `08`'s junction.
// ---------------------------------------------------------------------------

/// `08`'s phase S0b, narrowed to the three subsets that carry the decision.
///
/// The per-write medians above are not the figure to multiply by `N`. On ZFS
/// one synchronised write is bimodal, so its mean is well above its median
/// and a pass costs more than `N` times the median; `06` and `08` both say to
/// report the pass and not the write. This phase therefore times the whole
/// pass end to end, which is what `08`'s **9.970 s at `N` = 1 000** is, so
/// that figure and this one are the same measurement on two filesystems.
///
/// `HERD_CELLS` overrides the queue depths, as in `08`.
#[test]
#[ignore = "a bench; run it on purpose"]
fn a_whole_pass_of_waiting_writes() {
    let cells: Vec<usize> = std::env::var("HERD_CELLS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect::<Vec<usize>>()
        })
        .filter(|cells: &Vec<usize>| !cells.is_empty())
        .unwrap_or_else(|| vec![1_000]);

    println!("\n== ORCH-077 phase 3: a whole pass of N waiting writes ==");
    let probe = tempfile::tempdir().expect("a temporary directory");
    say_where(probe.path());
    drop(probe);
    println!("  comparable with 08's S0b, whose N = 1 000 pass was 9.970 s on ZFS");

    for n in cells {
        println!("\n  -- N = {n} --");
        println!("  subset | whole pass  | per job     | one write, median and spread");
        println!("  {}", "-".repeat(104));
        for syncs in [Syncs::ALL, Syncs::only(Which::Record), Syncs::NONE] {
            // A fresh corpus per subset: the first waiting write on a job is
            // the news, so each subset must meet an untouched queue.
            let corpus = Corpus::build(n);
            corpus.warm();
            let mut each = Vec::with_capacity(n);
            let pass = Instant::now();
            for (i, id) in corpus.ids.iter().enumerate() {
                let job_dir = corpus.store.job_dir(id);
                let reason = format!("waiting because cause {i}");
                let one = Instant::now();
                let wrote = replica_record_waiting(&job_dir, &reason, syncs)
                    .expect("the replica records the wait");
                each.push(one.elapsed());
                assert!(wrote, "a distinct cause is news");
            }
            let whole = pass.elapsed();
            let spread = Spread::of(each);
            println!(
                "  {:<6} | {:>11} | {:>11} | {}",
                syncs.code(),
                fmt(whole),
                fmt(whole / n as u32),
                spread.say()
            );
        }
    }
    println!("\n  load after this phase: {}", load().trim());
}
