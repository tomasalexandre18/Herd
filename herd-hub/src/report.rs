//! What a state root holds, read off its own records (ORCH-125).
//!
//! **Why this exists at all.** Herd is shipped, not operated (D161,
//! invariant 9): nobody in this project logs into the consumer's hub, so
//! nobody here can answer "how much of what this hub holds is the same bytes
//! over again?" or "what does a job's output really weigh?" from anything
//! but a guess. A hub reporting on itself is the one way such a fact reaches
//! us: the operator runs this, reads the figures, and sends them on.
//!
//! **Three figures, and each one exists because a guess about it was wrong.**
//!
//! 1. [`Dedup`] — `r_file`, the file-level deduplication ratio. The bytes
//!    every record names, over the bytes of their distinct `sha256`. This is
//!    the figure that decides whether deduplication is ever built, so it is
//!    printed with the four counts it rests on and never as a bare ratio.
//! 2. [`Sizes`] — per-job output size by **decile**, and the maximum. Never a
//!    mean: on the one sample we have, eight deciles are nought and the top
//!    tenth of the jobs carries almost all of the bytes, so a mean is wrong
//!    by more than an order of magnitude.
//! 3. [`Accounting`] — what the records say this root holds against what the
//!    directories measure. That difference is where an accounting defect
//!    shows up, and no other figure here would show it. It is printed as a
//!    total **and** as the jobs behind it, because a total is not something
//!    an operator can act on: the one real defect this project has seen was
//!    found by running a shell loop over a figure this report had already
//!    computed per job and printed only the sum of (ORCH-166).
//!
//! **What it is allowed to touch.** The records, and the *lengths* of the
//! files under each job's `files/`, `files.part/` and `files.gone/`. No
//! output byte, no log byte, no `events.jsonl`. Every size and every digest
//! in sections 1 and 2 comes out of a `job.json`; section 3's measurements
//! are `metadata().len()` and never contents.
//!
//! **It takes no lock.** Not the store's `Mutex<Live>` — it never builds a
//! [`crate::store::JobDirStore`] — and above all not the `flock(2)` on
//! `hub.lock`, which is held for the lifetime of the running hub and would
//! make this report either refuse to run or block the service. It opens
//! files read-only, one at a time, and creates nothing. That is why
//! [`StateRootReport::read`] takes a `&Path` and not a store: a reader that
//! could not possibly be holding the writer's lock is easier to be sure of
//! than one that promises not to take it.
//!
//! What it gives up for that, said in the report itself: the figures are not
//! one consistent instant. A hub writing this root while the walk is in
//! progress can land a record after the walk passed its directory. That is
//! the right trade — a report that can stall a production hub is worse than
//! no report — and the same trade `job.json`'s lockless read already makes
//! (ORCH-082): every record is published by `rename`, so each one is read
//! whole or not at all, and only *which* version of it was read is in doubt.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::job::{OutputSummary, Timestamp, RECORD_VERSION};
use crate::store::{
    directory_bytes, JOBS_DIR, KEPT_FILES_DIR, OUTPUT_GONE_DIR, OUTPUT_PARTS_DIR, RECORD_FILE,
};

/// How many offenders of one kind the report names before it stops listing
/// and says how many more there are. Ten, because the list exists to be
/// acted on: a defect naming ten paths is a morning's work, and one naming
/// four thousand is a number, which the count already is.
const NAMED_AT_MOST: usize = 10;

/// Enough of a `job.json` to report on it, and nothing else.
///
/// **Deliberately not [`crate::job::JobRecord`].** A record whose labels
/// today's rules would reject, or one carrying a field this build does not
/// know, must not take a *report* down: the whole point is to be runnable on
/// a live root whatever is in it. So this reads the version it must check and
/// the one field it needs, and serde ignores the rest. The field itself is
/// the real [`OutputSummary`], so there is one definition of what an output
/// manifest is and the report cannot drift from the record.
#[derive(Debug, Deserialize)]
struct RecordOutputs {
    #[serde(default)]
    record_version: u32,
    #[serde(default)]
    outputs: OutputSummary,
}

/// One job directory this build could not turn into figures, and why.
///
/// Counted and named rather than skipped: a record left out of the arithmetic
/// is a reason every figure below it is a floor, and D27's rule — a thing
/// that did not run is never reported as one that did — applies to a record
/// that was not read just as much as to an assertion that was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// The path, relative to the state root, so the line can be pasted.
    pub path: String,
    pub why: String,
}

/// How many job directories there are, and how many of them became figures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    /// Directories in `jobs/`.
    pub directories: usize,
    /// Of those, the ones whose `job.json` this build read.
    pub read: usize,
    /// Of those, the ones with no `job.json` in them at all — a directory
    /// claimed for a job whose first record has not landed yet, which is a
    /// state and not a fault.
    pub without_record: usize,
    /// The rest, each with its reason.
    pub unread: Vec<Unread>,
}

/// One side of `r_file`: the copies, and the distinct bytes behind them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dedup {
    /// Files whose record carries a `sha256`.
    pub files: u64,
    /// The sum of their recorded lengths — every copy counted.
    pub bytes: u64,
    /// How many distinct `sha256` those files are.
    pub digests: u64,
    /// One length per distinct digest, summed: what one copy each would be.
    pub distinct_bytes: u64,
}

impl Dedup {
    /// `bytes / distinct_bytes`, or nothing when there is no digested byte to
    /// divide — a root holding only empty outputs is `0 / 0`, which is not a
    /// ratio of one and is not a ratio at all.
    pub fn ratio(&self) -> Option<f64> {
        if self.distinct_bytes == 0 {
            return None;
        }
        Some(self.bytes as f64 / self.distinct_bytes as f64)
    }
}

/// The distribution of per-job output size, by decile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sizes {
    /// How many job records the distribution is over: **every** record read,
    /// including the many that name no file at all. Leaving those out would
    /// be the same mistake as reporting a mean — it would describe the jobs
    /// that kept something and call it the jobs.
    pub population: usize,
    /// Of those, the ones naming no file.
    pub naming_nothing: usize,
    /// Deciles 1 to 9, by nearest rank: decile *k* is the smallest recorded
    /// size that at least `ceil(k * n / 10)` of the jobs are at or under.
    /// No interpolation, so every figure printed is a size some job really
    /// has.
    pub deciles: [u64; 9],
    pub max: u64,
    /// The sum over the whole population.
    pub bytes: u64,
    /// How many jobs sit above decile 9's rank — the top tenth, as that rank
    /// cuts it. Under ten jobs this is nought, and then the share below is
    /// nought too: a tenth of nine jobs is not a job.
    pub top_decile_jobs: usize,
    /// What those jobs carry.
    pub top_decile_bytes: u64,
}

impl Sizes {
    /// The share of the bytes the top decile carries, or nothing when there
    /// is no byte to share out.
    pub fn top_decile_share(&self) -> Option<f64> {
        if self.bytes == 0 {
            return None;
        }
        Some(self.top_decile_bytes as f64 / self.bytes as f64)
    }
}

/// One job whose record claims more bytes than its `files/` holds, with
/// both figures, so the line can be acted on without a second command.
///
/// Three fields and not two: the shortfall is `claimed - measured`, and
/// printing it beside its two terms is what lets a reader check the figure
/// rather than trust it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimShortfall {
    /// The job directory's name, relative to `jobs/`.
    pub job: String,
    /// What the record's manifest names.
    pub claimed: u64,
    /// What `files/` measures, by file length. Never nought: a job measuring
    /// nought is the extreme [`Accounting::claimed_but_absent`] already
    /// names, and naming it twice would double-count the same defect.
    pub measured: u64,
}

impl ClaimShortfall {
    /// `claimed - measured`: the bytes the record names and the directory
    /// does not have.
    pub fn shortfall(&self) -> u64 {
        self.claimed.saturating_sub(self.measured)
    }
}

/// The records against the disk: figure 3.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accounting {
    /// Jobs whose record names files and does **not** say the bytes were
    /// removed: the collections this root still claims to hold.
    pub claiming_jobs: usize,
    /// The bytes those records name.
    pub claimed_bytes: u64,
    /// Jobs whose record says the bytes are gone (`outputs.removed`), and
    /// the bytes their manifests still name — the manifest outliving its
    /// bytes, which is D148's whole point and not a defect.
    pub removed_jobs: usize,
    pub removed_bytes: u64,
    /// What every job's `files/` measures, by file length.
    pub measured_bytes: u64,
    /// Jobs whose record claims bytes and whose `files/` measures nought.
    pub claimed_but_absent: Vec<String>,
    /// Jobs whose record claims **more** than their `files/` measures, while
    /// that directory still measures something: the name, the claim and the
    /// measurement.
    ///
    /// **The middle of the range, and the state a defect actually leaves.**
    /// The two lists either side of this one catch the extremes — nothing
    /// where bytes were claimed, and bytes where nothing was claimed — and
    /// between 2026-10-28 and today the one real accounting defect this
    /// project has seen sat in neither: a job whose `files/` measured 143 MB
    /// against a record claiming 559 MB. The gap total named it as a number
    /// and nothing named it as a job, so the operator found it with a shell
    /// loop over a figure this list had already computed. That is the whole
    /// reason this field exists.
    ///
    /// **Largest shortfall first, ties by job name — and that order is
    /// deliberately not the order of its neighbours.** Every other list in
    /// this report is in the walk's sorted job order, because every other
    /// list is an all-or-nothing state where the order carries nothing. This
    /// one is a *magnitude*, and only [`NAMED_AT_MOST`] of it is printed: in
    /// job order the names that fit would be the ten lowest ids, which is
    /// arbitrary with respect to the question being asked, and an operator
    /// would have to re-sort by hand — the very thing (a shell loop over
    /// figures the report already had) this list exists to stop. Do not
    /// "fix" it back to job order. The tie-break keeps the text a function
    /// of its inputs, so the whole-text assertion holds.
    pub claiming_more_than_held: Vec<ClaimShortfall>,
    /// Jobs whose record says the bytes went and whose `files/` still
    /// measures some, with what it measures.
    pub removed_but_present: Vec<(String, u64)>,
    /// `files.part/` directories — a collection that started and did not
    /// finish, which is real disk and deliberately outside the output budget
    /// — how many, what they measure, and **which jobs**.
    pub parts_dirs: usize,
    pub parts_bytes: u64,
    /// Those directories by job, with what each measures.
    pub parts_jobs: Vec<(String, u64)>,
    /// `files.gone/` directories — a sweep that was killed between the
    /// rename and the unlink (D152) — and what they measure. Non-zero is
    /// disk the next sweep will free, not disk anything accounts for.
    pub gone_dirs: usize,
    pub gone_bytes: u64,
}

impl Accounting {
    /// How far the records and the directories are apart, as the records
    /// claiming more (`Greater`) or the disk measuring more (`Less`).
    pub fn gap(&self) -> (std::cmp::Ordering, u64) {
        (
            self.claimed_bytes.cmp(&self.measured_bytes),
            self.claimed_bytes.abs_diff(self.measured_bytes),
        )
    }
}

/// A `sha256` two records give two different lengths.
///
/// The same bytes cannot be two lengths, so this is an accounting defect and
/// is named with both figures. The larger is what the arithmetic above uses,
/// which makes `r_file` the lower of the two claims it could have made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestConflict {
    pub digest: String,
    pub smaller: u64,
    pub larger: u64,
}

/// Everything this report says, as values, so that a test can assert the
/// whole of its text and a caller can read a figure without parsing one.
#[derive(Debug, Clone, PartialEq)]
pub struct StateRootReport {
    pub root: PathBuf,
    /// When the walk began. **A field and not a call to the clock inside
    /// [`fmt::Display`]**, so the rendered text is a function of its inputs
    /// and can be asserted line for line.
    pub at: Timestamp,
    /// Said rather than swallowed when `jobs/` itself could not be read: a
    /// report of nought jobs on a root holding thousands would be the worst
    /// output this tool could produce.
    pub jobs_dir_error: Option<String>,
    pub census: Census,
    /// `r_file` over every record this root holds, bytes still there or not.
    /// The historical figure: what deduplication would have saved over
    /// everything this hub has ever been given.
    pub ever: Dedup,
    /// `r_file` over the collections whose bytes the records still claim.
    /// The figure for today's disk.
    pub held: Dedup,
    /// Files whose record carries no digest, and their bytes. Left out of
    /// both sides of both ratios: a byte with no digest can neither be
    /// matched against another nor be called distinct from it.
    pub undigested_files: u64,
    pub undigested_bytes: u64,
    pub conflicts: Vec<DigestConflict>,
    pub sizes: Sizes,
    pub accounting: Accounting,
}

impl StateRootReport {
    /// Walk a state root and answer. Takes no lock, writes nothing, reads no
    /// payload byte.
    ///
    /// The only error is a root that is not a readable directory: everything
    /// after that is reported rather than raised, because a report that
    /// refuses on one bad record is a report nobody can run on the root that
    /// has one.
    pub fn read(root: &Path) -> io::Result<Self> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a directory", root.display()),
            ));
        }
        let mut report = Self {
            root: root.to_path_buf(),
            at: Timestamp::now(),
            jobs_dir_error: None,
            census: Census::default(),
            ever: Dedup::default(),
            held: Dedup::default(),
            undigested_files: 0,
            undigested_bytes: 0,
            conflicts: Vec::new(),
            sizes: Sizes::default(),
            accounting: Accounting::default(),
        };

        // Sorted, so that the lists this report prints are in one order
        // whatever the directory hands back. `read_dir` has none.
        let jobs_dir = root.join(JOBS_DIR);
        let mut names = match fs::read_dir(&jobs_dir) {
            Ok(entries) => {
                let mut names = Vec::new();
                for entry in entries.flatten() {
                    if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                        names.push(entry.file_name().to_string_lossy().into_owned());
                    }
                }
                names
            }
            Err(error) => {
                report.jobs_dir_error = Some(format!("{}: {error}", jobs_dir.display()));
                Vec::new()
            }
        };
        names.sort();
        report.census.directories = names.len();

        // One length per digest, the largest any record gave it, and the
        // conflicts found on the way.
        let mut ever_digests: BTreeMap<String, u64> = BTreeMap::new();
        let mut held_digests: BTreeMap<String, u64> = BTreeMap::new();
        let mut conflicts: BTreeMap<String, DigestConflict> = BTreeMap::new();
        let mut per_job: Vec<u64> = Vec::with_capacity(names.len());
        let mut naming_nothing = 0usize;

        for name in &names {
            let dir = jobs_dir.join(name);
            let record_path = dir.join(RECORD_FILE);
            let text = match fs::read_to_string(&record_path) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    report.census.without_record += 1;
                    continue;
                }
                Err(error) => {
                    report.census.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it could not be read: {error}"),
                    });
                    continue;
                }
            };
            let parsed: RecordOutputs = match serde_json::from_str(&text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    report.census.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it is not a record this build can parse: {error}"),
                    });
                    continue;
                }
            };
            if parsed.record_version != RECORD_VERSION {
                report.census.unread.push(Unread {
                    path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                    why: format!(
                        "it carries record version {}, and this build reads version \
                         {RECORD_VERSION}",
                        parsed.record_version
                    ),
                });
                continue;
            }
            report.census.read += 1;

            let outputs = &parsed.outputs;
            let still_held = outputs.removed.is_none();
            let mut job_bytes = 0u64;
            for file in &outputs.files {
                job_bytes = job_bytes.saturating_add(file.bytes);
                if file.sha256.is_empty() {
                    report.undigested_files += 1;
                    report.undigested_bytes = report.undigested_bytes.saturating_add(file.bytes);
                    continue;
                }
                report.ever.files += 1;
                report.ever.bytes = report.ever.bytes.saturating_add(file.bytes);
                note_digest(&mut ever_digests, &mut conflicts, &file.sha256, file.bytes);
                if still_held {
                    report.held.files += 1;
                    report.held.bytes = report.held.bytes.saturating_add(file.bytes);
                    note_digest(&mut held_digests, &mut conflicts, &file.sha256, file.bytes);
                }
            }

            if outputs.files.is_empty() {
                naming_nothing += 1;
            }
            per_job.push(job_bytes);

            // Figure 3. `directory_bytes` is the sweeper's own walk, on
            // purpose: if this report measured by a different rule, the gap
            // it prints would be partly its own doing.
            let measured = directory_bytes(&dir.join(KEPT_FILES_DIR));
            report.accounting.measured_bytes =
                report.accounting.measured_bytes.saturating_add(measured);
            if still_held {
                if !outputs.files.is_empty() {
                    report.accounting.claiming_jobs += 1;
                    report.accounting.claimed_bytes =
                        report.accounting.claimed_bytes.saturating_add(job_bytes);
                    if job_bytes > 0 && measured == 0 {
                        report.accounting.claimed_but_absent.push(name.clone());
                    } else if job_bytes > measured {
                        // `measured > 0` holds here: the arm above took the
                        // zero case. The middle of the range, which is where
                        // a real accounting defect sits.
                        report
                            .accounting
                            .claiming_more_than_held
                            .push(ClaimShortfall {
                                job: name.clone(),
                                claimed: job_bytes,
                                measured,
                            });
                    }
                }
            } else {
                report.accounting.removed_jobs += 1;
                report.accounting.removed_bytes =
                    report.accounting.removed_bytes.saturating_add(job_bytes);
                if measured > 0 {
                    report
                        .accounting
                        .removed_but_present
                        .push((name.clone(), measured));
                }
            }
            let parts = dir.join(OUTPUT_PARTS_DIR);
            if parts.is_dir() {
                let parts_bytes = directory_bytes(&parts);
                report.accounting.parts_dirs += 1;
                report.accounting.parts_bytes =
                    report.accounting.parts_bytes.saturating_add(parts_bytes);
                report
                    .accounting
                    .parts_jobs
                    .push((name.clone(), parts_bytes));
            }
            let gone = dir.join(OUTPUT_GONE_DIR);
            if gone.is_dir() {
                report.accounting.gone_dirs += 1;
                report.accounting.gone_bytes = report
                    .accounting
                    .gone_bytes
                    .saturating_add(directory_bytes(&gone));
            }
        }

        report.ever.digests = ever_digests.len() as u64;
        report.ever.distinct_bytes = ever_digests
            .values()
            .fold(0u64, |sum, bytes| sum.saturating_add(*bytes));
        report.held.digests = held_digests.len() as u64;
        report.held.distinct_bytes = held_digests
            .values()
            .fold(0u64, |sum, bytes| sum.saturating_add(*bytes));
        report.conflicts = conflicts.into_values().collect();
        // Largest shortfall first, ties by job name. The only list here not
        // in the walk's job order, and the reason is on the field: it is a
        // magnitude that gets truncated, so the names that fit have to be
        // the ones that matter. `sort_by` and not `sort_unstable_by`:
        // nothing here is hot, and the comparison is total anyway, so the
        // order is a function of the inputs either way.
        report
            .accounting
            .claiming_more_than_held
            .sort_by(|left, right| {
                right
                    .shortfall()
                    .cmp(&left.shortfall())
                    .then_with(|| left.job.cmp(&right.job))
            });
        report.sizes = summarise(per_job, naming_nothing);
        Ok(report)
    }
}

/// Fold one file's digest into a tally, and remember a length that disagrees
/// with the one already there.
fn note_digest(
    into: &mut BTreeMap<String, u64>,
    conflicts: &mut BTreeMap<String, DigestConflict>,
    digest: &str,
    bytes: u64,
) {
    match into.get_mut(digest) {
        None => {
            into.insert(digest.to_owned(), bytes);
        }
        Some(known) if *known == bytes => {}
        Some(known) => {
            let smaller = (*known).min(bytes);
            let larger = (*known).max(bytes);
            *known = larger;
            let entry = conflicts
                .entry(digest.to_owned())
                .or_insert_with(|| DigestConflict {
                    digest: digest.to_owned(),
                    smaller,
                    larger,
                });
            entry.smaller = entry.smaller.min(smaller);
            entry.larger = entry.larger.max(larger);
        }
    }
}

/// The deciles, the maximum and the top decile's share of one population of
/// per-job sizes.
fn summarise(mut per_job: Vec<u64>, naming_nothing: usize) -> Sizes {
    per_job.sort_unstable();
    let n = per_job.len();
    let mut sizes = Sizes {
        population: n,
        naming_nothing,
        deciles: [0; 9],
        max: per_job.last().copied().unwrap_or(0),
        bytes: per_job
            .iter()
            .fold(0u64, |sum, bytes| sum.saturating_add(*bytes)),
        top_decile_jobs: 0,
        top_decile_bytes: 0,
    };
    if n == 0 {
        return sizes;
    }
    for k in 1u32..=9 {
        sizes.deciles[(k - 1) as usize] = per_job[rank(n, k) - 1];
    }
    let above = rank(n, 9);
    sizes.top_decile_jobs = n - above;
    sizes.top_decile_bytes = per_job[above..]
        .iter()
        .fold(0u64, |sum, bytes| sum.saturating_add(*bytes));
    sizes
}

/// The 1-based nearest rank of decile `k` in a population of `n`:
/// `ceil(k * n / 10)`, never below 1 and never above `n`.
fn rank(n: usize, k: u32) -> usize {
    let n = n as u128;
    let rank = (u128::from(k) * n).div_ceil(10);
    (rank.max(1) as usize).min(n as usize)
}

/// A byte count as this report prints one: the exact figure always, and an
/// approximation beside it once the exact figure stops being readable.
///
/// Exactness first and never instead: every number here is evidence somebody
/// will do arithmetic on, and a rounded one is how a figure gets quoted back
/// as the figure it was not.
fn bytes_said(bytes: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if bytes < KIB {
        return format!("{bytes} bytes");
    }
    let (scale, unit) = if bytes >= GIB {
        (GIB, "GiB")
    } else if bytes >= MIB {
        (MIB, "MiB")
    } else {
        (KIB, "KiB")
    };
    format!("{bytes} bytes ({:.2} {unit})", bytes as f64 / scale as f64)
}

/// A ratio, or the sentence that says there was nothing to divide.
fn ratio_said(ratio: Option<f64>) -> String {
    match ratio {
        Some(ratio) => format!("{ratio:.2}"),
        None => "undefined: no record names a digested byte".to_owned(),
    }
}

impl fmt::Display for StateRootReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "herd-hub state-root report")?;
        writeln!(f, "  state root  {}", self.root.display())?;
        writeln!(f, "  taken at    {}", self.at)?;
        writeln!(f)?;
        writeln!(
            f,
            "  A point-in-time snapshot. It takes no lock of any kind, so a hub may be"
        )?;
        writeln!(
            f,
            "  writing this root while it is read: each record is read whole or not at"
        )?;
        writeln!(
            f,
            "  all, but the figures together are not one instant. It reads no output"
        )?;
        writeln!(
            f,
            "  byte and no log byte -- every size and digest in sections 1 and 2 comes"
        )?;
        writeln!(
            f,
            "  out of a record, and section 3's measurements are file lengths."
        )?;
        writeln!(f)?;

        writeln!(f, "records")?;
        if let Some(error) = &self.jobs_dir_error {
            writeln!(
                f,
                "  THE JOBS DIRECTORY COULD NOT BE READ, so every figure below is about"
            )?;
            writeln!(f, "  no jobs at all and about nothing else: {error}")?;
        }
        writeln!(
            f,
            "  job directories                    {}",
            self.census.directories
        )?;
        writeln!(
            f,
            "  records read                       {}",
            self.census.read
        )?;
        writeln!(
            f,
            "  directories with no record yet     {}",
            self.census.without_record
        )?;
        writeln!(
            f,
            "  records this build could not read  {}",
            self.census.unread.len()
        )?;
        for unread in self.census.unread.iter().take(NAMED_AT_MOST) {
            writeln!(f, "    {}: {}", unread.path, unread.why)?;
        }
        if let Some(more) = self.census.unread.len().checked_sub(NAMED_AT_MOST) {
            if more > 0 {
                writeln!(f, "    and {more} more")?;
            }
        }
        writeln!(f)?;

        writeln!(f, "1. file-level deduplication (r_file)")?;
        writeln!(f)?;
        writeln!(
            f,
            "  The bytes the records name, over the bytes of their distinct sha256:"
        )?;
        writeln!(
            f,
            "  what one copy per digest would weigh against what every copy weighs."
        )?;
        writeln!(
            f,
            "  Read it with the counts under it. A ratio on its own says nothing about"
        )?;
        writeln!(f, "  how many files or how many bytes it is over.")?;
        writeln!(f)?;
        writeln!(
            f,
            "  over every record this root holds, bytes still there or not"
        )?;
        write_dedup(f, &self.ever)?;
        writeln!(
            f,
            "  over the collections whose bytes these records still claim"
        )?;
        write_dedup(f, &self.held)?;
        writeln!(
            f,
            "  files whose record carries no digest  {} ({})",
            self.undigested_files,
            bytes_said(self.undigested_bytes)
        )?;
        writeln!(
            f,
            "    Written before a record carried one. Left out of both sides of both"
        )?;
        writeln!(
            f,
            "    ratios: a byte with no digest can neither be matched against another"
        )?;
        writeln!(f, "    nor be called distinct from it.")?;
        writeln!(
            f,
            "  digests given two different lengths   {}",
            self.conflicts.len()
        )?;
        if !self.conflicts.is_empty() {
            writeln!(
                f,
                "    An accounting defect: the same bytes cannot be two lengths. The"
            )?;
            writeln!(
                f,
                "    larger is what the figures above use, which makes r_file the lower"
            )?;
            writeln!(f, "    of the two claims it could have made.")?;
            for conflict in self.conflicts.iter().take(NAMED_AT_MOST) {
                writeln!(
                    f,
                    "    {}: {} bytes and {} bytes",
                    conflict.digest, conflict.smaller, conflict.larger
                )?;
            }
            if let Some(more) = self.conflicts.len().checked_sub(NAMED_AT_MOST) {
                if more > 0 {
                    writeln!(f, "    and {more} more")?;
                }
            }
        }
        writeln!(f)?;

        writeln!(f, "2. per-job output size, by decile")?;
        writeln!(f)?;
        writeln!(
            f,
            "  Deciles and the maximum, never a mean: this distribution is skewed far"
        )?;
        writeln!(
            f,
            "  enough that a mean over it misreads a job by more than an order of"
        )?;
        writeln!(
            f,
            "  magnitude. Decile k is the smallest size at least ceil(k * n / 10) of"
        )?;
        writeln!(
            f,
            "  the jobs are at or under -- a size some job really has, not an"
        )?;
        writeln!(
            f,
            "  interpolation -- and the population is every record read, including the"
        )?;
        writeln!(f, "  ones that name no file.")?;
        writeln!(f)?;
        writeln!(
            f,
            "  population                 {} records",
            self.sizes.population
        )?;
        writeln!(
            f,
            "    of those, naming no file {}",
            self.sizes.naming_nothing
        )?;
        for (index, decile) in self.sizes.deciles.iter().enumerate() {
            writeln!(
                f,
                "  decile {}                   {}",
                index + 1,
                bytes_said(*decile)
            )?;
        }
        writeln!(
            f,
            "  maximum                    {}",
            bytes_said(self.sizes.max)
        )?;
        writeln!(
            f,
            "  over the whole population  {}",
            bytes_said(self.sizes.bytes)
        )?;
        match self.sizes.top_decile_share() {
            Some(share) => writeln!(
                f,
                "  the largest {} record(s)    {}, {:.2}% of that",
                self.sizes.top_decile_jobs,
                bytes_said(self.sizes.top_decile_bytes),
                share * 100.0
            )?,
            None => writeln!(
                f,
                "  the largest {} record(s)    no record names a byte, so there is no \
                 share to give",
                self.sizes.top_decile_jobs
            )?,
        }
        writeln!(f)?;

        writeln!(f, "3. the records against the directories")?;
        writeln!(f)?;
        writeln!(
            f,
            "  What the records say this root holds against what the directories"
        )?;
        writeln!(
            f,
            "  measure. A manifest outliving its bytes is by design (D148): only the"
        )?;
        writeln!(
            f,
            "  bytes are ever removed, and such a record is counted apart below rather"
        )?;
        writeln!(f, "  than against the disk.")?;
        writeln!(f)?;
        let accounting = &self.accounting;
        writeln!(
            f,
            "  records still claiming their bytes  {} job(s), {}",
            accounting.claiming_jobs,
            bytes_said(accounting.claimed_bytes)
        )?;
        writeln!(
            f,
            "  records whose bytes were removed    {} job(s), {}",
            accounting.removed_jobs,
            bytes_said(accounting.removed_bytes)
        )?;
        writeln!(
            f,
            "  every files/ directory measures     {}",
            bytes_said(accounting.measured_bytes)
        )?;
        let (order, gap) = accounting.gap();
        match order {
            std::cmp::Ordering::Equal => writeln!(
                f,
                "  the two agree to the byte, which is what a sound accounting looks like"
            )?,
            std::cmp::Ordering::Greater => writeln!(
                f,
                "  THE RECORDS CLAIM {} MORE THAN THE DIRECTORIES MEASURE",
                bytes_said(gap)
            )?,
            std::cmp::Ordering::Less => writeln!(
                f,
                "  THE DIRECTORIES MEASURE {} MORE THAN THE RECORDS CLAIM",
                bytes_said(gap)
            )?,
        }
        writeln!(
            f,
            "  claiming bytes, files/ measures 0   {} job(s)",
            accounting.claimed_but_absent.len()
        )?;
        for name in accounting.claimed_but_absent.iter().take(NAMED_AT_MOST) {
            writeln!(f, "    {JOBS_DIR}/{name}")?;
        }
        if let Some(more) = accounting
            .claimed_but_absent
            .len()
            .checked_sub(NAMED_AT_MOST)
        {
            if more > 0 {
                writeln!(f, "    and {more} more")?;
            }
        }
        writeln!(
            f,
            "  claiming more than files/ holds     {} job(s)",
            accounting.claiming_more_than_held.len()
        )?;
        if !accounting.claiming_more_than_held.is_empty() {
            writeln!(
                f,
                "    Bytes were claimed and some of them are there: neither extreme the"
            )?;
            writeln!(
                f,
                "    lists either side of this one name. Largest shortfall first, so the"
            )?;
            writeln!(
                f,
                "    names that fit are the ones that matter, and both figures are on"
            )?;
            writeln!(
                f,
                "    each line, so the shortfall can be checked rather than trusted."
            )?;
            for short in accounting
                .claiming_more_than_held
                .iter()
                .take(NAMED_AT_MOST)
            {
                writeln!(
                    f,
                    "    {JOBS_DIR}/{}: claims {}, files/ measures {}, short by {}",
                    short.job,
                    bytes_said(short.claimed),
                    bytes_said(short.measured),
                    bytes_said(short.shortfall())
                )?;
            }
            if let Some(more) = accounting
                .claiming_more_than_held
                .len()
                .checked_sub(NAMED_AT_MOST)
            {
                if more > 0 {
                    writeln!(f, "    and {more} more")?;
                }
            }
        }
        writeln!(
            f,
            "  bytes removed, files/ still holds   {} job(s)",
            accounting.removed_but_present.len()
        )?;
        for (name, bytes) in accounting.removed_but_present.iter().take(NAMED_AT_MOST) {
            writeln!(f, "    {JOBS_DIR}/{name}: {}", bytes_said(*bytes))?;
        }
        if let Some(more) = accounting
            .removed_but_present
            .len()
            .checked_sub(NAMED_AT_MOST)
        {
            if more > 0 {
                writeln!(f, "    and {more} more")?;
            }
        }
        writeln!(
            f,
            "  {OUTPUT_PARTS_DIR}/ unfinished collection   {} job(s), {}",
            accounting.parts_dirs,
            bytes_said(accounting.parts_bytes)
        )?;
        if !accounting.parts_jobs.is_empty() {
            writeln!(
                f,
                "    A collection that started and did not finish. This report cannot"
            )?;
            writeln!(
                f,
                "    tell whether anything is still coming: a runner may be streaming"
            )?;
            writeln!(
                f,
                "    into it right now, or it may be what one that died left behind."
            )?;
            for (name, bytes) in accounting.parts_jobs.iter().take(NAMED_AT_MOST) {
                writeln!(f, "    {JOBS_DIR}/{name}: {}", bytes_said(*bytes))?;
            }
            if let Some(more) = accounting.parts_jobs.len().checked_sub(NAMED_AT_MOST) {
                if more > 0 {
                    writeln!(f, "    and {more} more")?;
                }
            }
        }
        writeln!(
            f,
            "  {OUTPUT_GONE_DIR}/ a sweep left behind     {} job(s), {}",
            accounting.gone_dirs,
            bytes_said(accounting.gone_bytes)
        )?;
        Ok(())
    }
}

fn write_dedup(f: &mut fmt::Formatter<'_>, dedup: &Dedup) -> fmt::Result {
    writeln!(f, "    files with a digest    {}", dedup.files)?;
    writeln!(f, "    bytes they name        {}", bytes_said(dedup.bytes))?;
    writeln!(f, "    distinct sha256        {}", dedup.digests)?;
    writeln!(
        f,
        "    bytes over distinct    {}",
        bytes_said(dedup.distinct_bytes)
    )?;
    writeln!(
        f,
        "    r_file                 {}",
        ratio_said(dedup.ratio())
    )?;
    Ok(())
}
