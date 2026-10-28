//! ORCH-104: what a walk of the hub's records costs, at the directory counts
//! the first consumer actually has.
//!
//! **Why this file exists.** `docs/disk-policy.md` §2.5 and §3.4 carry two
//! figures that M12's retention engine is being designed against — *"30–45 ms
//! at 3334 records"* and *"0.3–0.45 s at 33 340"* — and both are marked
//! `[D]`: **derived**, by scaling D152's 100 000-directory timings down and up
//! linearly. §4 H6 writes that assumption down as a hypothesis and says
//! nobody has run it. This file runs it.
//!
//! It also settles, or refuses to settle, **ORCH-123**: the board's claim that
//! *"about 94 % of the `T` walk is per-record CPU — not the disk and not the
//! lock"*, whose stated cause — `read_stored` parsing one record's text three
//! times — is itself recorded as *"a hypothesis with an arithmetic bound, not
//! a measurement"*. The breakdown below attributes the microseconds.
//!
//! **It measures; it asserts no product behaviour**, and every case is
//! `#[ignore]`d so the workspace suite stays the suite. Run it on purpose:
//!
//! ```sh
//! export TMPDIR=/mnt/ssd_980/herd/<somewhere with room>   # NOT the root fs
//! nix develop --command cargo test -p herd-hub --test walk_cost \
//!   --release -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Run it `--release` **and** without, because three `serde_json` parses of
//! one small document is exactly the kind of cost a debug build exaggerates,
//! and the hub the consumer runs is a release build.
//!
//! **Nothing under `herd-hub/src` is touched.** The two walks are called
//! through the public [`JobStore`] trait, and the breakdown is a *replica* of
//! `read_stored` built from public types — [`JobRecord`], [`LabelSet`] and the
//! same three `serde_json::from_str` calls in the same order over the same
//! text. The replica's total is printed beside the real walk's so that the
//! reader can see how faithful it is instead of taking it on trust; if the two
//! disagree the attribution is worth nothing and the printed ratio says so.
//!
//! **The two walks measured, and which is which.**
//!
//! | walk | what it is | what it touches per directory |
//! |---|---|---|
//! | `sweep_outputs` | the narrow pass the store already does on every completed collection (`store.rs:766`). Under budget it reads **no record at all** | one `stat` of `files.gone/`, one `opendir` of `files/` |
//! | `collections_owed` | the full record walk, on every runner join and every 200 ms `AskJobFiles` poll (`server.rs:2415`, `:4384`) | one `open`+`read` of `job.json`, then three parses of its text |
//!
//! `collections_owed` is the profile D152 timed at 0.87–1.31 s per 100 000
//! records and the profile §3.4 says a retention pass would have, *"because
//! retention must read every record to learn its priority and its end time"*.
//! It is therefore the one the engine's affordability turns on.
//!
//! **The page cache.** A cold figure here means: every record file `fsync`ed
//! when it was written, then `posix_fadvise(POSIX_FADV_DONTNEED)` over each of
//! them, so the kernel drops their clean pages. That evicts the **page
//! cache** and not the dentry or inode caches, which need `drop_caches` and
//! therefore root — so "cold" below is colder than
//! `junction_breakdown.rs`'s *"written by this process a moment ago"* and
//! still warmer than a hub restarted onto a state root it has never read. The
//! eviction is checked rather than assumed: see `the_eviction_is_real`.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::job::{JobRecord, JobState, KeptFile, NewJob, RECORD_VERSION};
use herd_hub::{durable, JobDirStore, JobStore};
use herd_proto::{JobOutcome, LabelSet};
use serde::{Deserialize, Serialize};

const RECORD_FILE: &str = "job.json";
const KEPT_FILES_DIR: &str = "files";
const OUTPUT_GONE_DIR: &str = "files.gone";

/// The two populations the brief names, with the points that make the shape
/// legible between and beyond them. 100 000 is D152's own figure, so the top
/// row of every table below meets that measurement head on instead of being
/// scaled towards it.
const POPULATIONS: [usize; 5] = [1_000, 3_334, 10_000, 33_340, 100_000];

/// The populations to run, largest first dropped by `HERD_WALK_COST_MAX`.
///
/// **Why this knob exists, measured rather than guessed.** The
/// 100 000-directory fixture cost **327 s to build and ~1.3 GB** on this
/// host, and a second build of it under another agent's heavy phase was
/// heading for twenty minutes. That row is a bonus — it exists so the table
/// meets D152's own 100 000-directory figure head on — while 3 334 and
/// 33 340 are the populations the question is about. So a run that only
/// needs the answer can say `HERD_WALK_COST_MAX=33340` and finish in a
/// couple of minutes, and a run that says nothing still measures everything.
fn populations() -> Vec<usize> {
    let cap = std::env::var("HERD_WALK_COST_MAX")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let chosen: Vec<usize> = POPULATIONS.into_iter().filter(|t| *t <= cap).collect();
    if cap != usize::MAX {
        println!("  note: HERD_WALK_COST_MAX={cap}, so the populations run are {chosen:?} and the rest of POPULATIONS was NOT measured");
    }
    chosen
}

/// One record in `HEAVY_IN` carries a real manifest. The first consumer's own
/// output distribution has deciles 1–8 at 0.0 MB with the top tenth carrying
/// 95 % of the bytes (`docs/disk-policy.md`), so a corpus of identical records
/// would measure a population nobody has: it would make the per-record cost
/// uniform by construction and the deciles meaningless.
const HEAVY_IN: usize = 10;

/// How many files a heavy record's manifest names. A manifest entry is a
/// path, a length and a sha256 — about 120 bytes of JSON — and D152's own
/// prose names a *"560-file collection"*, so 240 is a middling one.
const MANIFEST_FILES: usize = 240;

/// One directory in `HOLDERS_IN` holds bytes under `files/`, which is what the
/// sweep's narrow pass pays for: D152 built 100 000 directories of which
/// 1 000 held bytes, and 1 in 10 is the heavier, more pessimistic shape.
const HOLDERS_IN: usize = 10;

/// Files in a holder's `files/`. The sweep `stat`s each one, so the count is
/// what costs, never the size.
const FILES_PER_HOLDER: usize = 12;

// ---------------------------------------------------------------------------
// A replica of `read_stored`, from public types only.
// ---------------------------------------------------------------------------

/// `herd-hub/src/store.rs:294`.
#[derive(Deserialize)]
struct VersionProbe {
    #[serde(default)]
    record_version: u32,
}

/// `herd-hub/src/store.rs:309`.
#[derive(Deserialize)]
struct DurableLabels {
    #[serde(default)]
    labels: Vec<String>,
}

/// `herd-hub/src/store.rs:276`.
#[derive(Serialize, Deserialize)]
struct RecordFile {
    record_version: u32,
    #[serde(flatten)]
    record: JobRecord,
}

/// What `read_stored` does to one record's text, in the order it does it:
/// probe the version, read the labels permissively, validate them, then parse
/// the whole record. Returns nothing — the point is the time.
fn parse_like_read_stored(text: &str) {
    let probe: VersionProbe = serde_json::from_str(text).expect("the version probe parses");
    assert_eq!(probe.record_version, RECORD_VERSION);
    let durable: DurableLabels = serde_json::from_str(text).expect("the labels parse");
    LabelSet::new(durable.labels.clone()).expect("the labels are labels");
    let file: RecordFile = serde_json::from_str(text).expect("the record parses");
    assert!(!file.record.command.is_empty());
}

/// The shape §3.4 proposes instead: one parse into a type that carries the
/// version, the labels and the record. Measured so that the saving ORCH-123
/// would buy is a figure and not a hope.
fn parse_once(text: &str) {
    let file: RecordFile = serde_json::from_str(text).expect("the record parses");
    assert_eq!(file.record_version, RECORD_VERSION);
    assert!(!file.record.command.is_empty());
}

// ---------------------------------------------------------------------------
// The corpus.
// ---------------------------------------------------------------------------

/// A state root holding `t` closed job directories, built the way
/// `cost_surface.rs` and `junction_breakdown.rs` build theirs — one real
/// record minted through the store, then stamped — so the three campaigns
/// measure the same shape of disk.
///
/// **No real state root is read or copied.** Nothing here looks at the
/// consumer's hub volume or at their runner's data root, which is
/// `drwx------ herd:herd` and nobody's business here (invariant 9, D161).
struct Corpus {
    /// Held so the state root outlives the measurement, read by nothing.
    _dir: tempfile::TempDir,
    store: Arc<JobDirStore>,
    jobs: PathBuf,
    /// The synthetic directory names, in id order.
    names: Vec<String>,
    heavy: usize,
    holders: usize,
}

impl Corpus {
    fn build(t: usize) -> Self {
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
        let plain = fs::read_to_string(store.job_dir(&template).join(RECORD_FILE))
            .expect("the template record reads");
        let heavy_text = inflate(&plain);
        let digits = template.as_str().len() - 1;

        let jobs = dir.path().join("jobs");
        let mut names = Vec::with_capacity(t);
        let mut heavy = 0usize;
        let mut holders = 0usize;
        for i in 0..t {
            let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
            let job_dir = jobs.join(&name);
            fs::create_dir_all(&job_dir).expect("a closed job directory");
            let text = if i % HEAVY_IN == 0 {
                heavy += 1;
                &heavy_text
            } else {
                &plain
            };
            // `fsync`ed on purpose: a page that is still dirty is a page
            // `POSIX_FADV_DONTNEED` will not drop, so without this the cold
            // figures below would silently be warm ones.
            write_synced(
                &job_dir.join(RECORD_FILE),
                text.replace(template.as_str(), &name).as_bytes(),
            );
            if i % HOLDERS_IN == 0 {
                holders += 1;
                let kept = job_dir.join(KEPT_FILES_DIR);
                fs::create_dir_all(&kept).expect("a kept-files directory");
                for f in 0..FILES_PER_HOLDER {
                    fs::write(kept.join(format!("part-{f}.bin")), [b'x'; 1024])
                        .expect("a kept file");
                }
            }
            names.push(name);
        }

        Self {
            _dir: dir,
            store,
            jobs,
            names,
            heavy,
            holders,
        }
    }

    fn directories(&self) -> usize {
        fs::read_dir(&self.jobs)
            .expect("the jobs directory lists")
            .count()
    }

    fn record_path(&self, name: &str) -> PathBuf {
        self.jobs.join(name).join(RECORD_FILE)
    }

    /// Record bytes on the disk, which is what a cold walk has to fetch.
    fn record_bytes(&self) -> u64 {
        self.names
            .iter()
            .filter_map(|name| fs::metadata(self.record_path(name)).ok())
            .map(|meta| meta.len())
            .sum()
    }

    /// Drop the clean page-cache pages of every record file.
    ///
    /// The dentry and inode caches stay warm: dropping those needs
    /// `/proc/sys/vm/drop_caches`, which needs root, and this host also runs
    /// the first consumer's production — a global cache drop is their incident
    /// and not ours to cause. So the cold figures below are a **page-cache**
    /// cold and are labelled as one.
    fn evict(&self) {
        for name in &self.names {
            if let Ok(file) = File::open(self.record_path(name)) {
                let _ = rustix::fs::fadvise(&file, 0, None, rustix::fs::Advice::DontNeed);
            }
        }
    }

    /// Read everything once, so no warm figure below is a first-touch figure.
    fn warm(&self) {
        let _ = JobStore::collections_owed(self.store.as_ref());
        let _ = self.store.sweep_outputs();
    }

    fn say(&self) -> String {
        format!(
            "{} dirs, {} heavy records, {} holders, {} of records",
            self.directories(),
            self.heavy,
            self.holders,
            show(self.record_bytes()),
        )
    }
}

/// Give a template record a real manifest, so that the corpus has a skewed
/// record-size distribution rather than one size repeated.
fn inflate(plain: &str) -> String {
    let mut file: RecordFile = serde_json::from_str(plain).expect("the template record parses");
    file.record.outputs.files = (0..MANIFEST_FILES)
        .map(|i| KeptFile {
            path: format!("results/group-{i:04}/results.json"),
            bytes: 4_096 + i as u64,
            sha256: format!("{:064x}", i as u128 * 0x9e37_79b9_7f4a_7c15),
        })
        .collect();
    serde_json::to_string(&file).expect("the inflated record serialises")
}

fn write_synced(path: &Path, bytes: &[u8]) {
    let mut file = File::create(path).expect("a record file");
    file.write_all(bytes).expect("the record is written");
    file.sync_data().expect("the record is on the disk");
}

// ---------------------------------------------------------------------------
// Statistics. A mean over a skewed population has already misled this
// project by more than an order of magnitude, so every distribution here is
// printed as deciles and the mean is printed beside them to be compared, not
// instead of them.
// ---------------------------------------------------------------------------

fn nanos(samples: &[Duration]) -> Vec<u128> {
    let mut all: Vec<u128> = samples.iter().map(Duration::as_nanos).collect();
    all.sort_unstable();
    all
}

fn decile(sorted: &[u128], d: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let at = ((sorted.len() - 1) * d) / 10;
    sorted[at]
}

fn median(samples: &[Duration]) -> Duration {
    let sorted = nanos(samples);
    Duration::from_nanos(decile(&sorted, 5) as u64)
}

fn total(samples: &[Duration]) -> Duration {
    samples.iter().sum()
}

/// One line of deciles, in microseconds, for a per-record distribution.
fn deciles_line(label: &str, samples: &[Duration]) -> String {
    let sorted = nanos(samples);
    if sorted.is_empty() {
        return format!("{label:<22} (no samples)");
    }
    let us = |n: u128| n as f64 / 1_000.0;
    let mean = sorted.iter().sum::<u128>() as f64 / sorted.len() as f64 / 1_000.0;
    let mut line = format!("{label:<22}");
    for d in 1..=9 {
        line.push_str(&format!(" {:>7.2}", us(decile(&sorted, d))));
    }
    line.push_str(&format!(
        " |{:>8.2} {:>9.2} {:>7.2}",
        us(sorted[sorted.len() - 1]),
        mean,
        us(sorted[sorted.len() - 1]) / us(decile(&sorted, 5)).max(1e-9)
    ));
    line
}

fn deciles_header(what: &str) -> String {
    format!(
        "{:<22}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8} |{:>8} {:>9} {:>7}\n  {}",
        what,
        "D1",
        "D2",
        "D3",
        "D4",
        "D5",
        "D6",
        "D7",
        "D8",
        "D9",
        "max",
        "mean",
        "max/D5",
        "(microseconds per record)"
    )
}

fn fmt(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s >= 1.0 {
        format!("{s:.3} s")
    } else if s >= 0.001 {
        format!("{:.2} ms", s * 1_000.0)
    } else {
        format!("{:.1} us", s * 1_000_000.0)
    }
}

fn show(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    format!("{mb:.1} MiB")
}

fn load1() -> f64 {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| {
            text.split_whitespace()
                .next()
                .and_then(|first| first.parse().ok())
        })
        .unwrap_or(f64::NAN)
}

/// Every table says what the host was doing while it was made. On a shared
/// workstation a figure with no load beside it is not a figure.
fn host(phase: &str) {
    println!("  [host] {phase}: 1-minute load {:.2}", load1());
}

// ---------------------------------------------------------------------------
// `directory_bytes`, replicated. `herd-hub/src/store.rs:3043`.
// ---------------------------------------------------------------------------

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            bytes += directory_bytes(&entry.path());
        } else if kind.is_file() {
            bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        }
    }
    bytes
}

// ---------------------------------------------------------------------------
// 1. The table the engine needs: both walks, cold and warm, against T.
// ---------------------------------------------------------------------------

/// What a walk costs at 1 000, 3 334, 10 000, 33 340 and 100 000 directories.
///
/// Cold and warm, both walks, five warm runs each. The last column is the
/// per-directory cost, which is where linearity lives: a walk that is linear
/// in `T` has a flat per-directory column, and the row where that column
/// starts climbing is the row where linearity stops.
#[test]
#[ignore = "a bench; run it on purpose"]
fn what_a_walk_costs_against_the_directory_count() {
    println!("\n== ORCH-104 phase 1: both walks against T ==");
    println!(
        "  cold = every record fsync'ed, then posix_fadvise(DONTNEED) over all \
         of them; dentry and inode caches stay warm (no root on this host)"
    );
    for t in populations() {
        let at = Instant::now();
        let corpus = Corpus::build(t);
        println!(
            "\n  T={t}: {} (built in {})",
            corpus.say(),
            fmt(at.elapsed())
        );
        host("before the cold walks");

        // Cold, each walk against its own eviction, so neither warms the
        // other. `sweep_outputs` first: it reads no record, so whatever it
        // warms is directory metadata, which the eviction cannot drop anyway.
        corpus.evict();
        let at = Instant::now();
        let swept = corpus.store.sweep_outputs().expect("the sweep walks");
        let sweep_cold = at.elapsed();
        corpus.evict();
        let at = Instant::now();
        let owed = JobStore::collections_owed(corpus.store.as_ref()).expect("the walk reads");
        let owed_cold = at.elapsed();

        corpus.warm();
        host("after warming");
        let sweep_warm: Vec<Duration> = (0..5)
            .map(|_| {
                let at = Instant::now();
                let _ = corpus.store.sweep_outputs();
                at.elapsed()
            })
            .collect();
        let owed_warm: Vec<Duration> = (0..5)
            .map(|_| {
                let at = Instant::now();
                let _ = JobStore::collections_owed(corpus.store.as_ref());
                at.elapsed()
            })
            .collect();
        host("after the warm walks");

        let sweep_warm_med = median(&sweep_warm);
        let owed_warm_med = median(&owed_warm);
        println!(
            "    sweep_outputs      cold {:>10}   warm {:>10}   cold/warm {:>5.2}   \
             per dir warm {:>8.2} us   (held {}, {} owed)",
            fmt(sweep_cold),
            fmt(sweep_warm_med),
            sweep_cold.as_secs_f64() / sweep_warm_med.as_secs_f64(),
            sweep_warm_med.as_secs_f64() * 1e6 / t as f64,
            show(swept.held),
            owed.len(),
        );
        println!(
            "    collections_owed   cold {:>10}   warm {:>10}   cold/warm {:>5.2}   \
             per dir warm {:>8.2} us   per dir cold {:>8.2} us",
            fmt(owed_cold),
            fmt(owed_warm_med),
            owed_cold.as_secs_f64() / owed_warm_med.as_secs_f64(),
            owed_warm_med.as_secs_f64() * 1e6 / t as f64,
            owed_cold.as_secs_f64() * 1e6 / t as f64,
        );
    }
}

// ---------------------------------------------------------------------------
// 2. What dominates: readdir, the per-record open+read, or the parse.
// ---------------------------------------------------------------------------

/// The breakdown, and the verdict on ORCH-123.
///
/// Four quantities, each measured and none inferred:
///
/// * the one `readdir` of `jobs/`,
/// * the per-record `open`+`read_to_string`, which is the disk half,
/// * the per-record three parses plus `LabelSet::new`, which is the CPU half,
/// * a per-record `stat`, for scale — the thing the sweep's cheap pass pays.
///
/// Their sum is printed against the real `collections_owed`'s wall time. The
/// replica is only worth reading if those two agree, so the ratio is printed
/// rather than assumed, and the per-record distributions are printed as
/// deciles because the corpus is deliberately skewed (one record in ten
/// carries a manifest).
#[test]
#[ignore = "a bench; run it on purpose"]
fn what_dominates_a_record_walk() {
    println!("\n== ORCH-104 phase 2: what dominates a record walk ==");
    for t in populations() {
        let corpus = Corpus::build(t);
        println!("\n  T={t}: {}", corpus.say());
        host("before the cold breakdown");

        // **Cold first, before anything warms the corpus.** The question
        // "what dominates" has two answers and they are not the same one: on
        // a cold walk the disk is paying, on a warm walk the CPU is, and a
        // design that saw only the warm answer would be sized against the
        // cheaper of the two.
        corpus.evict();
        let mut cold_reads = Vec::with_capacity(t);
        let mut cold_parses = Vec::with_capacity(t);
        for name in &corpus.names {
            let path = corpus.record_path(name);
            let at = Instant::now();
            let text = fs::read_to_string(&path).expect("the record reads");
            cold_reads.push(at.elapsed());
            let at = Instant::now();
            parse_like_read_stored(&text);
            cold_parses.push(at.elapsed());
        }
        let cold_read_total = total(&cold_reads);
        let cold_parse_total = total(&cold_parses);
        println!(
            "    COLD  open+read {:>10} ({:>5.1} %)   parse x3 {:>10} ({:>5.1} %)",
            fmt(cold_read_total),
            100.0 * cold_read_total.as_secs_f64()
                / (cold_read_total + cold_parse_total).as_secs_f64(),
            fmt(cold_parse_total),
            100.0 * cold_parse_total.as_secs_f64()
                / (cold_read_total + cold_parse_total).as_secs_f64(),
        );
        println!("  {}", deciles_header("per-record, cold"));
        println!("  {}", deciles_line("open+read cold", &cold_reads));
        println!("  {}", deciles_line("parse x3 cold", &cold_parses));

        corpus.warm();
        host("before the warm breakdown");

        // The real walk, for the replica to be checked against.
        let real: Vec<Duration> = (0..5)
            .map(|_| {
                let at = Instant::now();
                let _ = JobStore::collections_owed(corpus.store.as_ref());
                at.elapsed()
            })
            .collect();
        let real = median(&real);

        // The one readdir.
        let listing: Vec<Duration> = (0..5)
            .map(|_| {
                let at = Instant::now();
                let names = durable::names_in(&corpus.jobs).expect("the listing reads");
                assert!(names.len() >= t);
                at.elapsed()
            })
            .collect();
        let listing = median(&listing);

        // Per record, one pass: read, then parse the text that read returned.
        let mut reads = Vec::with_capacity(t);
        let mut parses = Vec::with_capacity(t);
        let mut both = Vec::with_capacity(t);
        for name in &corpus.names {
            let path = corpus.record_path(name);
            let at = Instant::now();
            let text = fs::read_to_string(&path).expect("the record reads");
            let read = at.elapsed();
            let at = Instant::now();
            parse_like_read_stored(&text);
            let parse = at.elapsed();
            reads.push(read);
            parses.push(parse);
            both.push(read + parse);
        }

        // A per-record `stat`, and the sweep's own per-directory work, for
        // scale: this is what a walk costs when it does not read records.
        let mut stats = Vec::with_capacity(t);
        let mut sweep_each = Vec::with_capacity(t);
        for name in &corpus.names {
            let dir = corpus.jobs.join(name);
            let at = Instant::now();
            let _ = fs::metadata(dir.join(RECORD_FILE));
            stats.push(at.elapsed());
            let at = Instant::now();
            let _ = dir.join(OUTPUT_GONE_DIR).is_dir();
            let _ = directory_bytes(&dir.join(KEPT_FILES_DIR));
            sweep_each.push(at.elapsed());
        }

        // And the one-parse shape §3.4 proposes.
        let mut once = Vec::with_capacity(t);
        for name in &corpus.names {
            let text = fs::read_to_string(corpus.record_path(name)).expect("the record reads");
            let at = Instant::now();
            parse_once(&text);
            once.push(at.elapsed());
        }
        host("after the warm breakdown");

        let read_total = total(&reads);
        let parse_total = total(&parses);
        let replica = listing + read_total + parse_total;
        let per_record = read_total + parse_total;
        println!(
            "    readdir(jobs/)            {:>10}   {:>5.1} % of the replica",
            fmt(listing),
            100.0 * listing.as_secs_f64() / replica.as_secs_f64()
        );
        println!(
            "    per-record open+read      {:>10}   {:>5.1} % of the replica",
            fmt(read_total),
            100.0 * read_total.as_secs_f64() / replica.as_secs_f64()
        );
        println!(
            "    per-record parse x3       {:>10}   {:>5.1} % of the replica",
            fmt(parse_total),
            100.0 * parse_total.as_secs_f64() / replica.as_secs_f64()
        );
        println!(
            "    ---- per-record total     {:>10}   {:>5.1} % of the replica  \
             <- ORCH-123 claims ~94 %",
            fmt(per_record),
            100.0 * per_record.as_secs_f64() / replica.as_secs_f64()
        );
        println!(
            "    replica sum               {:>10}   against the real walk {:>10}  \
             replica/real {:.2}",
            fmt(replica),
            fmt(real),
            replica.as_secs_f64() / real.as_secs_f64()
        );
        println!(
            "    parse share of per-record {:>9.1} %   (the lever ORCH-123 names)",
            100.0 * parse_total.as_secs_f64() / per_record.as_secs_f64()
        );
        println!(
            "    one parse instead of 3    {:>10}   would save {:>10} of the walk \
             ({:>4.1} %)",
            fmt(total(&once)),
            fmt(parse_total.saturating_sub(total(&once))),
            100.0 * (parse_total.saturating_sub(total(&once))).as_secs_f64()
                / replica.as_secs_f64()
        );
        println!(
            "    per-record stat only      {:>10}   sweep's per-dir work {:>10}",
            fmt(total(&stats)),
            fmt(total(&sweep_each))
        );

        println!("\n  {}", deciles_header("per-record distribution"));
        println!("  {}", deciles_line("open+read", &reads));
        println!("  {}", deciles_line("parse x3", &parses));
        println!("  {}", deciles_line("read+parse", &both));
        println!("  {}", deciles_line("one parse", &once));
        println!("  {}", deciles_line("stat only", &stats));
        println!("  {}", deciles_line("sweep per dir", &sweep_each));
    }
}

// ---------------------------------------------------------------------------
// 3. Is the eviction real? A cold figure nobody checked is a warm one.
// ---------------------------------------------------------------------------

/// Whether `posix_fadvise(DONTNEED)` actually drops the pages on this host.
///
/// One file, read three times: once right after eviction, then twice more.
/// If the first read is no slower than the second, the eviction did nothing
/// and every cold figure in phase 1 is in truth a warm one — which is a thing
/// to print, not a thing to hope about.
#[test]
#[ignore = "a bench; run it on purpose"]
fn the_eviction_is_real() {
    println!("\n== ORCH-104 phase 3: is the page-cache eviction real? ==");
    let corpus = Corpus::build(2_000);
    println!("  {}", corpus.say());
    host("before the check");

    let mut after_eviction = Vec::new();
    let mut warm = Vec::new();
    for name in corpus.names.iter().take(500) {
        let path = corpus.record_path(name);
        if let Ok(file) = File::open(&path) {
            let _ = rustix::fs::fadvise(&file, 0, None, rustix::fs::Advice::DontNeed);
        }
        let at = Instant::now();
        let _ = fs::read_to_string(&path);
        after_eviction.push(at.elapsed());
        let at = Instant::now();
        let _ = fs::read_to_string(&path);
        warm.push(at.elapsed());
    }
    println!("\n  {}", deciles_header("one record's open+read"));
    println!("  {}", deciles_line("after DONTNEED", &after_eviction));
    println!("  {}", deciles_line("immediately again", &warm));
    println!(
        "\n  median after eviction {} against warm {}: ratio {:.2}",
        fmt(median(&after_eviction)),
        fmt(median(&warm)),
        median(&after_eviction).as_secs_f64() / median(&warm).as_secs_f64()
    );
    println!(
        "  a ratio near 1.0 means the eviction did nothing on this host and \
         every \"cold\" figure above is warm."
    );
}

// ---------------------------------------------------------------------------
// 4. The order the walk reads in, which nothing has measured.
// ---------------------------------------------------------------------------

/// Directory order against id order, cold and warm.
///
/// **Why this phase exists.** In phase 2 the replica's *warm* total tracks
/// the real `collections_owed` to within a few percent, and its *cold* total
/// is well under it. The two walks do the same syscalls over the same files,
/// so the difference is not what they read — it is the order. The store walks
/// whatever `durable::names_in` hands back, which on ext4 is the directory's
/// **hash order** and so effectively random against the disk; the replica
/// walks `corpus.names`, which is **id order** and therefore creation order,
/// and creation order is laid out sequentially.
///
/// That is worth a measurement of its own rather than a sentence, because
/// `docs/disk-policy.md` §3.4 proposes an **id-ordered** retention pass for a
/// different reason — it can stop early — and if id order is also the cheaper
/// order per record on a cold cache, the proposal earns an argument nobody
/// has made for it.
#[test]
#[ignore = "a bench; run it on purpose"]
fn the_order_a_walk_reads_in() {
    println!("\n== ORCH-104 phase 4: directory order against id order ==");
    for t in [3_334usize, 33_340] {
        let corpus = Corpus::build(t);
        println!("\n  T={t}: {}", corpus.say());

        // Exactly what the store sees, in the order the store sees it.
        let listed = durable::names_in(&corpus.jobs).expect("the listing reads");
        let mut sorted = listed.clone();
        sorted.sort();
        let agree = listed
            .iter()
            .zip(sorted.iter())
            .filter(|(a, b)| a == b)
            .count();
        println!(
            "    readdir order against sorted id order: {agree} of {} names in the \
             same position",
            listed.len()
        );

        let read_all = |order: &[String]| -> Duration {
            let at = Instant::now();
            for name in order {
                let _ = fs::read_to_string(corpus.jobs.join(name).join(RECORD_FILE));
            }
            at.elapsed()
        };

        host("before the cold pair");
        corpus.evict();
        let cold_listed = read_all(&listed);
        corpus.evict();
        let cold_sorted = read_all(&sorted);
        // And again the other way round, because whichever runs first on a
        // cold cache also warms the dentries it touches.
        corpus.evict();
        let cold_sorted_first = read_all(&sorted);
        corpus.evict();
        let cold_listed_second = read_all(&listed);

        corpus.warm();
        host("before the warm pair");
        let warm_listed = median(&(0..3).map(|_| read_all(&listed)).collect::<Vec<_>>());
        let warm_sorted = median(&(0..3).map(|_| read_all(&sorted)).collect::<Vec<_>>());

        println!(
            "    COLD  readdir order {:>10} then id order {:>10}   id/readdir {:.2}",
            fmt(cold_listed),
            fmt(cold_sorted),
            cold_sorted.as_secs_f64() / cold_listed.as_secs_f64()
        );
        println!(
            "    COLD  id order first {:>9} then readdir  {:>10}   readdir/id {:.2}",
            fmt(cold_sorted_first),
            fmt(cold_listed_second),
            cold_listed_second.as_secs_f64() / cold_sorted_first.as_secs_f64()
        );
        println!(
            "    WARM  readdir order {:>10}      id order {:>10}   id/readdir {:.2}",
            fmt(warm_listed),
            fmt(warm_sorted),
            warm_sorted.as_secs_f64() / warm_listed.as_secs_f64()
        );
    }
}
