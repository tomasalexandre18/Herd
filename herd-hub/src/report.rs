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
// The measured-bytes rule, folded into `herd-proto` beside the rule for a
// figure a human typed (ORCH-186). Every byte figure this report prints is
// one somebody measured or one a record claims, never one an operator typed,
// so `show_bytes` appears nowhere below: exactness first and never instead,
// because every number here is evidence somebody will do arithmetic on.
use herd_proto::size::measured_bytes;

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
    /// `None` when the key is not in the JSON at all, which is **not** the
    /// same fact as a record that says version `0` and must not be reported
    /// as one. A `u32` with `#[serde(default)]` read an absent field as
    /// zero, and the refusal then said *"it carries record version 0"* —
    /// describing a field that is missing as a field with a value in it.
    /// `job.rs` draws the same line on `at_its_ceiling`: never read as zero,
    /// because zero is a thing a record can mean.
    #[serde(default)]
    record_version: Option<u32>,
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
            if parsed.record_version != Some(RECORD_VERSION) {
                report.census.unread.push(Unread {
                    path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                    why: match parsed.record_version {
                        Some(version) => format!(
                            "it carries record version {version}, and this build reads \
                             version {RECORD_VERSION}"
                        ),
                        // Said as absent, because that is a different fact
                        // about the file and sends an operator somewhere
                        // else: a version this build does not read is a
                        // record from another layout, and no version at all
                        // is a file that is probably not one of ours.
                        None => format!(
                            "it carries no record version at all, and this build reads \
                             version {RECORD_VERSION}"
                        ),
                    },
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
        // The fraction beside the names rather than a bare `and N more`.
        // Ten names on a root of 1571 unreadable records is ten names out of
        // 1571, and an operator who cannot see that cannot tell whether the
        // list is the whole defect or a sample of it. The bound itself stays
        // at [`NAMED_AT_MOST`] -- a list of 1571 paths is a number, which
        // the count already is -- so the line says where the rest are
        // instead of printing them.
        if self.census.unread.len() > NAMED_AT_MOST {
            writeln!(
                f,
                "    ^ these are the first {NAMED_AT_MOST} of {}, in job-id order. The rest are",
                self.census.unread.len()
            )?;
            writeln!(
                f,
                "      later in `ls {JOBS_DIR}/` than the last name above; this report names"
            )?;
            writeln!(f, "      no more of them and no other file lists them.")?;
        }
        // Every record unread, which until now printed no banner at all:
        // `records read 0`, up to ten names, and then sections 1 to 3 in
        // full -- deduplication ratios, distinct digest counts and budget
        // arithmetic, every one of them over zero records. That reads as an
        // answer rather than as a refusal, which is the worse failure of the
        // two, so it gets the shape the unreadable `jobs/` directory above
        // already has.
        //
        // **It stays a report and does not become a refusal.** No exit code,
        // no early return, and the sections below still print: `report`
        // exists to be runnable on a state root a hub will not open, two
        // published release pages say so, and an operator reaches for it
        // precisely when the records cannot be read. A command that refused
        // here would refuse in the one case it was built for. So this prints
        // a banner and `herd-hub report` still exits 0.
        if self.census.read == 0 && !self.census.unread.is_empty() {
            writeln!(
                f,
                "  NOT ONE RECORD COULD BE PARSED BY THIS BUILD, so every figure below is"
            )?;
            writeln!(
                f,
                "  about no records at all and about nothing else: the jobs directory was"
            )?;
            writeln!(
                f,
                "  read, the records are there, and not one of them became a figure."
            )?;
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
            measured_bytes(self.undigested_bytes)
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
                measured_bytes(*decile)
            )?;
        }
        writeln!(
            f,
            "  maximum                    {}",
            measured_bytes(self.sizes.max)
        )?;
        writeln!(
            f,
            "  over the whole population  {}",
            measured_bytes(self.sizes.bytes)
        )?;
        match self.sizes.top_decile_share() {
            Some(share) => writeln!(
                f,
                "  the largest {} record(s)    {}, {:.2}% of that",
                self.sizes.top_decile_jobs,
                measured_bytes(self.sizes.top_decile_bytes),
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
            measured_bytes(accounting.claimed_bytes)
        )?;
        writeln!(
            f,
            "  records whose bytes were removed    {} job(s), {}",
            accounting.removed_jobs,
            measured_bytes(accounting.removed_bytes)
        )?;
        writeln!(
            f,
            "  every files/ directory measures     {}",
            measured_bytes(accounting.measured_bytes)
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
                measured_bytes(gap)
            )?,
            std::cmp::Ordering::Less => writeln!(
                f,
                "  THE DIRECTORIES MEASURE {} MORE THAN THE RECORDS CLAIM",
                measured_bytes(gap)
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
                    measured_bytes(short.claimed),
                    measured_bytes(short.measured),
                    measured_bytes(short.shortfall())
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
            writeln!(f, "    {JOBS_DIR}/{name}: {}", measured_bytes(*bytes))?;
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
            measured_bytes(accounting.parts_bytes)
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
                writeln!(f, "    {JOBS_DIR}/{name}: {}", measured_bytes(*bytes))?;
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
            measured_bytes(accounting.gone_bytes)
        )?;
        Ok(())
    }
}

fn write_dedup(f: &mut fmt::Formatter<'_>, dedup: &Dedup) -> fmt::Result {
    writeln!(f, "    files with a digest    {}", dedup.files)?;
    writeln!(
        f,
        "    bytes they name        {}",
        measured_bytes(dedup.bytes)
    )?;
    writeln!(f, "    distinct sha256        {}", dedup.digests)?;
    writeln!(
        f,
        "    bytes over distinct    {}",
        measured_bytes(dedup.distinct_bytes)
    )?;
    writeln!(
        f,
        "    r_file                 {}",
        ratio_said(dedup.ratio())
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The moment the rendered text is pinned to, so an assertion is about
    /// the figures and not about when it ran: 2026-10-28T00:00:00Z.
    const PINNED: i64 = 1_793_145_600;

    /// A record naming no output at all, which is what most jobs are.
    ///
    /// **Its version is [`RECORD_VERSION`] and not a literal `1`.** A
    /// hand-written fixture that spells the version out becomes an *unread*
    /// record the day the constant moves, and a test whose whole intent is
    /// "a record this build reads" would then silently assert the opposite.
    fn readable(id: &str) -> serde_json::Value {
        serde_json::json!({
            "record_version": RECORD_VERSION,
            "id": id,
            "command": ["true"],
            "state": { "type": "finished", "outcome": { "type": "exited", "code": 0 } },
        })
    }

    /// The same record, from a layout this build does not read — whichever
    /// version this build is.
    fn from_another_layout(id: &str) -> serde_json::Value {
        let mut record = readable(id);
        record["record_version"] = serde_json::json!(RECORD_VERSION + 1);
        record
    }

    /// Write one job directory: its `job.json`, and nothing else.
    fn job_dir(root: &Path, name: &str, record: Option<serde_json::Value>) {
        let dir = root.join(JOBS_DIR).join(name);
        fs::create_dir_all(&dir).expect("the job directory is made");
        if let Some(record) = record {
            fs::write(
                dir.join(RECORD_FILE),
                serde_json::to_vec_pretty(&record).expect("the record serialises"),
            )
            .expect("the record is written");
        }
    }

    fn rendered(root: &Path) -> String {
        let mut report = StateRootReport::read(root).expect("a report, never a refusal");
        report.at = Timestamp::at_unix_seconds(PINNED);
        report.to_string()
    }

    /// **Every record unread, which the report used to print as an answer.**
    ///
    /// Before this, such a root printed `records read 0`, up to ten names
    /// with their reasons, and then sections 1 to 3 in full: deduplication
    /// ratios, distinct digest counts and budget arithmetic, every figure of
    /// it over zero records, with no banner saying so. It exits 0, so it
    /// read as an answer rather than as the refusal it is.
    ///
    /// Two things are asserted together here and the second is not a
    /// detail: the banner fires, **and this is still a report**. `read`
    /// returns `Ok`, every section still prints, and the zeros are still
    /// there to be read. `herd-hub report` exists to be runnable on a state
    /// root a hub will not open, which is exactly when records cannot be
    /// read; a version of it that refused here would refuse in the one case
    /// it was built for.
    #[test]
    fn a_root_where_no_record_could_be_read_says_so_and_is_still_a_report() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        for n in 1..=3u64 {
            let name = format!("j{n:0>10}");
            job_dir(root, &name, Some(from_another_layout(&name)));
        }

        let report = StateRootReport::read(root).expect("a report, never a refusal");
        assert_eq!(report.census.directories, 3);
        assert_eq!(report.census.read, 0, "not one record became a figure");
        assert_eq!(report.census.unread.len(), 3);

        let text = rendered(root);
        assert!(
            text.contains(
                "  NOT ONE RECORD COULD BE PARSED BY THIS BUILD, so every figure below is\n  \
                 about no records at all and about nothing else: the jobs directory was\n  \
                 read, the records are there, and not one of them became a figure.\n"
            ),
            "the all-unread banner is missing:\n{text}"
        );

        // And it is still a report: the sections print and their figures are
        // zeros an operator can read beside the reason they are zeros.
        assert!(
            text.contains("1. file-level deduplication (r_file)\n"),
            "{text}"
        );
        assert!(
            text.contains("2. per-job output size, by decile\n"),
            "{text}"
        );
        assert!(
            text.contains("3. the records against the directories\n"),
            "{text}"
        );
        assert!(
            text.contains("  population                 0 records\n"),
            "the sections still print, over nothing:\n{text}"
        );
        assert_eq!(
            report.ever.files, 0,
            "no figure came out of an unread record"
        );
        assert_eq!(report.accounting.claimed_bytes, 0);
    }

    /// The banner is about *that* case and fires on nothing else.
    ///
    /// A root whose records are simply not there yet is a state and not a
    /// fault. Without this, a condition that drifted to `read == 0` alone
    /// would start shouting at every fresh state root.
    #[test]
    fn a_root_with_no_records_yet_is_not_a_root_whose_records_failed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        job_dir(root, "j0000000001", None);
        job_dir(root, "j0000000002", None);

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.census.read, 0);
        assert_eq!(report.census.without_record, 2);
        assert!(report.census.unread.is_empty());
        let text = rendered(root);
        assert!(
            !text.contains("NOT ONE RECORD COULD BE PARSED"),
            "two directories waiting for their first record are not a failure:\n{text}"
        );
    }

    /// And a root it read prints no banner at all, which is the control for
    /// the two above.
    #[test]
    fn a_root_this_build_reads_carries_no_banner() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        job_dir(root, "j0000000001", Some(readable("j0000000001")));

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.census.read, 1);
        assert!(report.census.unread.is_empty());
        let text = rendered(root);
        assert!(!text.contains("NOT ONE RECORD COULD BE PARSED"), "{text}");
        assert!(!text.contains("COULD NOT BE READ"), "{text}");
        assert!(
            text.contains("  records read                       1\n"),
            "{text}"
        );
    }

    /// A record with **no** `record_version` key is called absent, not zero.
    ///
    /// `#[serde(default)]` on a `u32` read a missing field as `0`, and the
    /// line then said *"it carries record version 0"* — a sentence about a
    /// field with a value in it, printed about a file that has no such
    /// field. The two send an operator to different places: a version this
    /// build does not read is a record from another layout, and no version
    /// at all is a file that is probably not one of ours.
    #[test]
    fn a_record_with_no_version_field_is_named_absent_and_never_as_version_zero() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();

        // The field removed, not set to zero.
        let mut no_version = readable("j0000000001");
        no_version
            .as_object_mut()
            .expect("the record is an object")
            .remove("record_version");
        assert!(no_version.get("record_version").is_none());
        job_dir(root, "j0000000001", Some(no_version));
        // A record this build does read, so this test is about the wording
        // and not about the banner above.
        job_dir(root, "j0000000002", Some(readable("j0000000002")));
        // And one that really says zero, which must still read as a version
        // somebody wrote: zero is a thing a record can say.
        let mut zero = readable("j0000000003");
        zero["record_version"] = serde_json::json!(0);
        job_dir(root, "j0000000003", Some(zero));

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.census.read, 1);
        assert_eq!(report.census.unread.len(), 2);
        assert_eq!(
            report.census.unread[0].why,
            format!(
                "it carries no record version at all, and this build reads version \
                 {RECORD_VERSION}"
            ),
            "an absent field is said to be absent"
        );
        assert!(
            !report.census.unread[0].why.contains("version 0"),
            "the one sentence this must never print again: {}",
            report.census.unread[0].why
        );
        assert_eq!(
            report.census.unread[1].why,
            format!(
                "it carries record version 0, and this build reads version \
                 {RECORD_VERSION}"
            ),
        );
        assert!(
            rendered(root)
                .contains("jobs/j0000000001/job.json: it carries no record version at all"),
            "{}",
            rendered(root)
        );
    }

    /// Ten names out of thirteen is said to be ten of thirteen.
    ///
    /// The bound is not raised: a list of 1571 paths is a number, which the
    /// count on the line above already is. What was missing was the fraction
    /// — an operator reading ten names could not tell whether the list was
    /// the whole defect or a sample of it — and where the rest are.
    #[test]
    fn the_unread_list_says_what_fraction_of_the_pile_it_names() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        for n in 1..=13u64 {
            let name = format!("j{n:0>10}");
            job_dir(root, &name, Some(from_another_layout(&name)));
        }
        // One readable record, so the all-unread banner is not what is being
        // read here.
        job_dir(root, "j0000000014", Some(readable("j0000000014")));

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.census.read, 1);
        assert_eq!(report.census.unread.len(), 13);

        let text = rendered(root);
        assert!(
            text.contains("  records this build could not read  13\n"),
            "the count is the whole thirteen:\n{text}"
        );
        // `NAMED_AT_MOST` is 10. Written out here rather than interpolated,
        // so that changing it fails this test and somebody has to say the
        // list got longer on purpose.
        assert!(
            text.contains(
                "    ^ these are the first 10 of 13, in job-id order. The rest are\n      \
                 later in `ls jobs/` than the last name above; this report names\n      \
                 no more of them and no other file lists them.\n"
            ),
            "the fraction, and where the rest are:\n{text}"
        );
        assert!(
            !text.contains("\n    and 3 more\n"),
            "a bare count with no fraction is what this replaced:\n{text}"
        );
        for nth in 1..=10 {
            assert!(
                text.contains(&format!(
                    "jobs/j{nth:0>10}/{RECORD_FILE}: it carries record version"
                )),
                "the first ten in job-id order are named:\n{text}"
            );
        }
        for nth in 11..=13 {
            assert!(
                !text.contains(&format!("jobs/j{nth:0>10}/{RECORD_FILE}")),
                "the eleventh onwards are counted, not named:\n{text}"
            );
        }
    }
}
