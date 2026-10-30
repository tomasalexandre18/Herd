//! What a state root holds, read off its own records (ORCH-125).
//!
//! **Why this exists at all.** Herd is shipped, not operated (D161,
//! invariant 9): nobody in this project logs into the consumer's hub, so
//! nobody here can answer "how much of what this hub holds is the same bytes
//! over again?" or "what does a job's output really weigh?" from anything
//! but a guess. A hub reporting on itself is the one way such a fact reaches
//! us: the operator runs this, reads the figures, and sends them on.
//!
//! **Five figures, and each one exists because a guess about it was wrong.**
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
//! 4. [`Packs`] — what is under `packs/`, measured. Nothing in Herd weighs
//!    those bytes against anything, and until that section existed the hub's
//!    whole pack population was held, unreported and uncounted.
//! 5. [`Bookkeeping`] — what this root's **own** bookkeeping weighs: the
//!    `job.json` records, the `events.jsonl` files and the `log` files, which
//!    every section above leaves out.
//!
//!    **Why it is not optional.** The first consumer agreed on 2026-10-30
//!    that v1.0.0 may ship without the hub's record retention, following in
//!    v1.1, on two conditions, and this is the second of them in their own
//!    words: *"`herd-hub report` continue de montrer le poids des
//!    enregistrements et des journaux, pour que nous voyions la croissance
//!    au lieu de la découvrir."* Their own measurement of their own root,
//!    which is the shape of what this has to make visible: about **346 MiB
//!    over 3334 jobs**, 0.32 % of the root, against **105.57 GiB of
//!    collected files**, 99.68 %. Sections 1 to 3 would print that root and
//!    one holding no record whatever identically, because not one of them
//!    weighs a record.
//!
//!    **And it prints its unit, because this product now has two
//!    apparent-size figures that err in opposite directions.** This one
//!    sums `metadata().len()`, so it is apparent size and a *floor* on real
//!    disk: a one-byte file costs a block, and these are almost all small
//!    files — 1443667 bytes apparent against 2850816 in blocks on one real
//!    root, about 1.97×, measured 2026-10-30. A job's `bytes_written`
//!    (**D263**, ORCH-278) is apparent size too and *overstates*, because a
//!    sparse file counts for its holes; that one killed a landing's gate the
//!    same day. Same unit, same word, opposite bias. The consumer's own
//!    figure for *this* quantity is `du -sk`, which is blocks, so a reader
//!    comparing their `du` with this total would see a root halve while
//!    nothing shrank. That is why the printed prose carries the unit, the
//!    direction of the error and the instruction — compare `du` with `du` —
//!    rather than leaving it in a comment nobody running the binary reads.
//!
//! **What it is allowed to touch.** The records, and the *lengths* of the
//! files under a job directory — those under `files/`, `files.part/` and
//! `files.gone/`, and those of the directory's own `job.json`,
//! `events.jsonl`, `log` and `log.seq`. No output byte, no log byte, no line
//! of `events.jsonl`: section 5 weighs a log and an event file and reads
//! neither of them. Every size and every digest in sections 1 and 2 comes
//! out of a `job.json`; the measurements in sections 3 to 5 are
//! `metadata().len()` and never contents.
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
// `retention.rs`'s, used rather than written again. "What a log weighs, or
// why this build cannot say" is one idea, and the rule it carries — an
// unmeasured log is UNKNOWN and never nought — is one that a second
// spelling of the same enum would be free to drift from.
use crate::retention::LogWeight;
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

/// `manifests/` under [`crate::registry::PACKS_DIR`]: the directory the hub's
/// pack registry publishes one directory per pack into.
///
/// **A second spelling of a name `registry.rs` owns, and the duplication is
/// guarded rather than hidden.** `PACKS_DIR` is `pub(crate)` and reused here;
/// this one is private to that module, and this report may not simply open a
/// [`crate::PackRegistry`] to ask it, because opening one *creates* the
/// directories and probes them for writability — the one thing a reader of a
/// live root must never do (see this module's header). So the name is written
/// again, and `the_report_walks_the_directory_the_registry_publishes_into`
/// opens a real registry on a temporary root and asserts that this is the
/// directory it made. A copy that drifts reddens that test instead of
/// silently reporting nought packs on a root holding thousands.
const PACKS_MANIFESTS_DIR: &str = "manifests";

/// `manifests.part/` beside it: where a registration is assembled before it
/// is published, and the one place an interrupted registration leaves
/// anything. The same duplication, under the same guard.
const PACKS_MANIFESTS_PART_DIR: &str = "manifests.part";

/// The files a job directory keeps **beside** its collection, by the names
/// `store.rs` writes them under: the event log, the job's own log, and the
/// hub's counter for that log.
///
/// **Written again here rather than imported**, for the reason `retention.rs`
/// writes the last two again: they are private to [`crate::store`] and that
/// module is another task's, so this one may not widen them. The duplication
/// is held by a test rather than by this comment —
/// `herd-hub/tests/state_root_report.rs` drives the store's own writer and
/// asserts section 5's figures against what *it* left, so a rename there
/// reddens that test instead of printing nought for every log and every event
/// file on an operator's root.
const EVENTS_FILE: &str = "events.jsonl";
const LOG_FILE: &str = "log";
const LOG_SEQ_FILE: &str = "log.seq";

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
    /// Every collected byte this section measured: `files/`, `files.part/`
    /// and `files.gone/` together.
    ///
    /// **Section 5's companion figure, and the one it is printed beside.**
    /// The three directories here and the files section 5 weighs partition a
    /// job directory between them — section 5 skips these three *by name* —
    /// so a byte in this total is in no other and the two can be put on
    /// adjacent lines without double-counting one.
    pub fn collected_bytes(&self) -> u64 {
        self.measured_bytes
            .saturating_add(self.parts_bytes)
            .saturating_add(self.gone_bytes)
    }

    /// How far the records and the directories are apart, as the records
    /// claiming more (`Greater`) or the disk measuring more (`Less`).
    pub fn gap(&self) -> (std::cmp::Ordering, u64) {
        (
            self.claimed_bytes.cmp(&self.measured_bytes),
            self.claimed_bytes.abs_diff(self.measured_bytes),
        )
    }
}

/// What this hub holds under `packs/`: figure 4 (M12, E-16, ORCH-256).
///
/// **Why it is here at all.** `docs/disk-policy.md` §0.7 makes it a
/// requirement and not a preference that whatever Herd keeps appears in a
/// report with its size and counts toward the space calculation. Until this
/// existed the words `pack` and `manifest` appeared nowhere in this file: the
/// hub's whole pack population was held, unreported and uncounted, which is
/// §0.7's 65 GB precedent one directory over. Nothing here removes anything —
/// **who owns the growth of `packs/` is ORCH-215, which is Alexa's question**,
/// and this report deliberately does not pre-empt it. Reporting what is held
/// needs no permission; removing it does.
///
/// **It is measured, never claimed.** There is no manifest of manifests to
/// read, so every figure below is [`directory_bytes`] over a real directory —
/// the sweeper's own walk, for the reason figure 3 uses it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packs {
    /// Whether `packs/` is on this root at all.
    ///
    /// A hub creates it at every start, so `false` is a root no hub of
    /// layout 2 has ever served — which is a different fact from a `packs/`
    /// that is there and empty, and the two must not print the same.
    pub present: bool,
    /// Directories under `packs/manifests/`: the packs this hub has
    /// published, one directory each.
    pub registered: usize,
    /// What those directories measure, by file length.
    pub registered_bytes: u64,
    /// Entries under `packs/manifests/` that are **not** directories.
    ///
    /// Counted because a file where a pack directory should be is somebody
    /// else's doing, and a count of nought is the answer an operator wants to
    /// read rather than infer.
    pub not_directories: usize,
    /// Said rather than swallowed when `packs/manifests/` itself could not be
    /// read: a report of nought packs on a root holding thousands is the
    /// worst output this tool could produce, which is the rule
    /// [`StateRootReport::jobs_dir_error`] already lives by.
    pub manifests_error: Option<String>,
    /// `packs/manifests.part/` directories — a registration interrupted
    /// between its writes and its one `rename(2)` — and what they measure.
    ///
    /// Real disk that nothing reads: the pack does not resolve and its id
    /// finds no manifest. The next `PackRegistry::open` removes it by name,
    /// so a non-zero figure here is residue a restart frees, not a
    /// population.
    pub interrupted: usize,
    pub interrupted_bytes: u64,
    /// Everything under `packs/`, whatever is in it.
    ///
    /// **The total is measured over the whole directory and not summed from
    /// the two figures above**, so that a child this build has never heard of
    /// is still counted. A later directory under `packs/` would otherwise be
    /// invisible here, which is exactly how the population this section
    /// exists for came to be uncounted in the first place.
    pub bytes: u64,
}

impl Packs {
    /// Bytes under `packs/` that neither `manifests/` nor `manifests.part/`
    /// accounts for: the figure that says this section has stopped seeing
    /// everything.
    pub fn unaccounted_bytes(&self) -> u64 {
        self.bytes
            .saturating_sub(self.registered_bytes)
            .saturating_sub(self.interrupted_bytes)
    }
}

/// What this root's own bookkeeping weighs: figure 5 (ORCH-282).
///
/// **The population every section above leaves out.** Sections 1 to 3 are
/// about the collected output and nothing else; section 4 is about `packs/`.
/// A `job.json`, an `events.jsonl` and a `log` are in none of them, and
/// nothing in Herd bounds any of the three: the output budget governs
/// collections (D148, D152) and a sweep frees them without touching a
/// record, while the only thing that frees a record's or a log's byte is
/// removing the record. So a root whose collections were all swept still
/// grows, and until this section existed no figure this project can see said
/// by how much.
///
/// **Over every job directory, read or not.** The arithmetic above stops at
/// a record this build cannot parse; these bytes are on the disk whatever
/// the record says, so a directory whose `job.json` is from another layout
/// is weighed and counted here. That makes [`Bookkeeping::directories`] the
/// census's `directories` and not its `read`, which is why the two counts
/// are printed together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookkeeping {
    /// Job directories this section weighed: every one of them.
    pub directories: usize,
    /// Of those, the ones holding a `job.json`, and what those weigh.
    pub records: usize,
    pub record_bytes: u64,
    /// The `events.jsonl` files, and what they weigh.
    pub events: usize,
    pub event_bytes: u64,
    /// The logs this build measured, and what they weigh.
    pub logs: usize,
    pub log_bytes: u64,
    /// The logs it could not measure, each named with its reason. **Never
    /// folded into `log_bytes` as nought**: a log counted as empty because it
    /// could not be measured understates the only population here that has no
    /// bound at all, which is the direction a growth figure must never err in.
    pub logs_unknown: Vec<(String, String)>,
    /// Directories with neither a `log` nor a `log.seq`: nothing ever wrote a
    /// log byte for them, which is a measurement and not a gap.
    pub without_log: usize,
    /// Everything else in a job directory that is not one of the three
    /// collection directories: `log.seq` today, and whatever a later
    /// milestone puts beside it.
    ///
    /// A catch-all on purpose, for [`Packs::bytes`]'s reason. No mechanism
    /// frees these bytes either, so counting a file nothing here names
    /// over-states what only a record removal takes and never under-states
    /// it — and a figure growing on this line is how a directory nobody
    /// named gets noticed.
    pub other_bytes: u64,
    /// Job directories that would not open at all, each with its reason. The
    /// figures are then a floor, said and not swallowed (D27's shape: a
    /// directory that was not read is never reported as one that weighed
    /// nothing).
    pub unreadable: Vec<Unread>,
    /// The heaviest single directory and what it weighed, as the walk's job
    /// order breaks ties.
    ///
    /// **Why there is an order statistic here at all and only one.** This
    /// section prints a *total*, because the question it answers — what does
    /// the bookkeeping weigh — is a question about a sum. But a total alone
    /// cannot be told apart from a different root with the same total, and
    /// the skew is real: a log has no bound on the hub and the runner's own
    /// default cap is 64 MiB, so one directory can be the whole of the
    /// difference. The maximum is the cheapest figure that says so. It is
    /// **not** a mean, which section 2's own prose rules out for this
    /// distribution and which would misread a directory here by the same
    /// order of magnitude.
    ///
    /// A directory whose log is [`LogWeight::Unknown`] contributes no log
    /// byte to this comparison, so it can lose the maximum to a smaller
    /// directory that could be read whole. `logs_unknown` names it.
    pub largest: Option<(String, u64)>,
}

impl Bookkeeping {
    /// Every byte this section measured.
    ///
    /// A **floor** whenever `logs_unknown` is not empty, which is why the two
    /// are printed together and never apart.
    pub fn bytes(&self) -> u64 {
        self.record_bytes
            .saturating_add(self.event_bytes)
            .saturating_add(self.log_bytes)
            .saturating_add(self.other_bytes)
    }

    /// This bookkeeping's share of itself plus the collected output beside it
    /// ([`Accounting::collected_bytes`]), or nothing when the two are nought
    /// and there is no byte to share out.
    ///
    /// The share and not the ratio, because the share is how the first
    /// consumer asked the question and read their own answer: 0.32 % against
    /// 99.68 %.
    pub fn share_of(&self, collected: u64) -> Option<f64> {
        let whole = self.bytes().saturating_add(collected);
        if whole == 0 {
            return None;
        }
        Some(self.bytes() as f64 / whole as f64)
    }

    /// Fold one directory's figures in. `job` is the directory's name under
    /// `jobs/`.
    fn add(&mut self, job: &str, one: &OneDirectory) {
        self.directories += 1;
        if let Some(why) = &one.unreadable {
            self.unreadable.push(Unread {
                path: format!("{JOBS_DIR}/{job}"),
                why: why.clone(),
            });
        }
        if one.has_record {
            self.records += 1;
        }
        self.record_bytes = self.record_bytes.saturating_add(one.record_bytes);
        if one.has_events {
            self.events += 1;
        }
        self.event_bytes = self.event_bytes.saturating_add(one.event_bytes);
        match &one.log {
            LogWeight::Measured(bytes) => {
                self.logs += 1;
                self.log_bytes = self.log_bytes.saturating_add(*bytes);
            }
            LogWeight::Unknown(why) => self.logs_unknown.push((job.to_owned(), why.clone())),
            LogWeight::NeverWritten => self.without_log += 1,
        }
        self.other_bytes = self.other_bytes.saturating_add(one.other_bytes);
        // `>=` on the incumbent, so that the walk's sorted job order breaks a
        // tie for the earlier name and the rendered text stays a function of
        // its inputs.
        let measured = one.bytes();
        match &self.largest {
            Some((_, most)) if *most >= measured => {}
            _ => self.largest = Some((job.to_owned(), measured)),
        }
    }
}

/// What one job directory's own files weigh, before they are folded into
/// [`Bookkeeping`].
///
/// Measured **positively**, file by file, and never by taking a collection
/// away from a total for the directory: a figure the log could not supply
/// must leave the log's place empty rather than silently move its bytes into
/// the record's.
#[derive(Debug)]
struct OneDirectory {
    /// `job.json`, by length, and whether there was one at all. A directory
    /// claimed for a job whose first record has not landed yet has none, and
    /// that is a state and not a fault.
    record_bytes: u64,
    has_record: bool,
    /// `events.jsonl`, by length, and whether there was one.
    event_bytes: u64,
    has_events: bool,
    /// The `log`: measured, or named as unmeasured.
    log: LogWeight,
    /// `log.seq`, and anything else beside the three collection directories.
    other_bytes: u64,
    /// Why this directory would not open, when it would not.
    unreadable: Option<String>,
}

impl OneDirectory {
    /// This directory's measured bookkeeping. An unmeasured log contributes
    /// nothing, so the figure is a floor for exactly the directories
    /// [`Bookkeeping::logs_unknown`] names.
    fn bytes(&self) -> u64 {
        self.record_bytes
            .saturating_add(self.event_bytes)
            .saturating_add(self.log.bytes().unwrap_or(0))
            .saturating_add(self.other_bytes)
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
    /// What this hub holds under `packs/`: figure 4, and the one population
    /// in this state root that no section above accounts for.
    pub packs: Packs,
    /// What this root's own records, event files and logs weigh: figure 5.
    pub bookkeeping: Bookkeeping,
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
            packs: Packs::default(),
            bookkeeping: Bookkeeping::default(),
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
            // Figure 5, taken before anything below can `continue`. A
            // directory whose record this build cannot parse still holds its
            // record, its events and its log on the disk, and a bookkeeping
            // figure that skipped it would be the one figure in this report
            // that under-states what the root weighs.
            report.bookkeeping.add(name, &weigh_one_directory(&dir));
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
        report.packs = read_the_packs(root);
        Ok(report)
    }
}

/// Measure what this hub holds under `packs/`: figure 4.
///
/// A directory walk and nothing else — no `PackRegistry::open`, which would
/// create directories, probe them for writability and remove residue on a
/// root this report is only allowed to read. So a pack is **one directory
/// under `packs/manifests/`**, which is the registry's own publication unit,
/// and its weight is the length of the files in it.
///
/// Nothing here is a refusal: a `packs/` that is not there is a root no hub
/// of layout 2 has served, which is a state, and a `manifests/` that cannot
/// be read is said in [`Packs::manifests_error`] so that nought packs is
/// never printed as an answer when it is really a failure to look.
fn read_the_packs(root: &Path) -> Packs {
    let directory = root.join(crate::registry::PACKS_DIR);
    let mut packs = Packs {
        present: directory.is_dir(),
        // The whole directory, measured once. Deliberately not the sum of
        // the two figures below: see [`Packs::bytes`].
        bytes: directory_bytes(&directory),
        ..Packs::default()
    };
    if !packs.present {
        return packs;
    }

    let manifests = directory.join(PACKS_MANIFESTS_DIR);
    match fs::read_dir(&manifests) {
        Ok(entries) => {
            for entry in entries.flatten() {
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => {
                        packs.registered += 1;
                        packs.registered_bytes = packs
                            .registered_bytes
                            .saturating_add(directory_bytes(&entry.path()));
                    }
                    // A file, a symlink or an entry whose kind would not read:
                    // not a pack, and counted rather than ignored.
                    _ => packs.not_directories += 1,
                }
            }
        }
        // A `manifests/` that is simply not there is not an error: a hub
        // creates it at open, so this is a `packs/` somebody made by hand or
        // one a very early build left. Nought packs is then the true answer.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            packs.manifests_error = Some(format!("{}: {error}", manifests.display()));
        }
    }

    let part = directory.join(PACKS_MANIFESTS_PART_DIR);
    if let Ok(entries) = fs::read_dir(&part) {
        for entry in entries.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                packs.interrupted += 1;
                packs.interrupted_bytes = packs
                    .interrupted_bytes
                    .saturating_add(directory_bytes(&entry.path()));
            }
        }
    }
    packs
}
/// Weigh one job directory's own files: figure 5, for one directory.
///
/// **The three collection directories are skipped by name**, not subtracted
/// from a total and not detected by any other rule. That is what makes this
/// figure and [`Accounting::collected_bytes`] disjoint rather than merely
/// intended to be: a byte under `files/`, `files.part/` or `files.gone/` is
/// never read by this function at all, and every other byte in the directory
/// is.
///
/// Lengths by `metadata().len()`, following no symlink, and
/// [`directory_bytes`] for a subdirectory this build does not know — the
/// sweeper's own walk, used on purpose so that two figures about the same
/// bytes are the same figure.
///
/// The log is the one file measured *or named*: it is the population nothing
/// bounds, and the only one where a missing figure would change what an
/// operator decides. Every other length falls back to nought the way
/// `directory_bytes` does, because a `job.json` that cannot be read is
/// already counted and named by the census.
fn weigh_one_directory(dir: &Path) -> OneDirectory {
    let mut one = OneDirectory {
        record_bytes: 0,
        has_record: false,
        event_bytes: 0,
        has_events: false,
        log: LogWeight::NeverWritten,
        other_bytes: 0,
        unreadable: None,
    };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            let why = format!(
                "the job directory itself could not be read, so none of its own files was \
                 weighed: {error}"
            );
            // Unknown and not nought, for the same reason a log that would
            // not stat is: nothing here was looked at.
            one.log = LogWeight::Unknown(why.clone());
            one.unreadable = Some(why);
            return one;
        }
    };
    let mut log: Option<LogWeight> = None;
    let mut counter_is_there = false;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            // Section 3's three, and this is the skip the two totals rest on.
            if [KEPT_FILES_DIR, OUTPUT_PARTS_DIR, OUTPUT_GONE_DIR].contains(&name.as_str()) {
                continue;
            }
            one.other_bytes = one
                .other_bytes
                .saturating_add(directory_bytes(&entry.path()));
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        match name.as_str() {
            RECORD_FILE => {
                one.has_record = true;
                one.record_bytes = entry.metadata().map(|at| at.len()).unwrap_or(0);
            }
            EVENTS_FILE => {
                one.has_events = true;
                one.event_bytes = entry.metadata().map(|at| at.len()).unwrap_or(0);
            }
            LOG_FILE => {
                log = Some(match entry.metadata() {
                    Ok(at) => LogWeight::Measured(at.len()),
                    Err(error) => LogWeight::Unknown(format!(
                        "{LOG_FILE} is there and its length could not be read: {error}"
                    )),
                });
            }
            _ => {
                if name == LOG_SEQ_FILE {
                    counter_is_there = true;
                }
                one.other_bytes = one
                    .other_bytes
                    .saturating_add(entry.metadata().map(|at| at.len()).unwrap_or(0));
            }
        }
    }
    one.log = match log {
        Some(log) => log,
        // The counter is this hub's own record that a log was being written,
        // so a log missing from beside it is a figure that is unknown rather
        // than nought. Without the counter the job never reached an attempt,
        // and then nought is a measurement.
        None if counter_is_there => LogWeight::Unknown(format!(
            "there is no {LOG_FILE}, and the {LOG_SEQ_FILE} beside it is this hub's own \
             record that a log was being counted, so its bytes are unknown and not nought"
        )),
        None => LogWeight::NeverWritten,
    };
    one
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
            "  out of a record, and the measurements in sections 3 to 5 are file"
        )?;
        writeln!(f, "  lengths, a log's own length among them.")?;
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
        writeln!(f)?;

        write_packs(f, &self.packs)?;
        writeln!(f)?;
        write_bookkeeping(f, &self.bookkeeping, self.accounting.collected_bytes())?;
        Ok(())
    }
}

/// Section 4, as a function of its own.
///
/// **A function and not a block, because it returns early.** A root with no
/// `packs/` prints the banner and stops describing packs, and while this was
/// the last thing the report printed that early return ended the whole
/// output. Section 5 follows it now, so a `return` here has to leave section
/// 4 and nothing else: on a root no hub of layout 2 has served — which is
/// every root a test builds by hand — the bookkeeping figures would
/// otherwise have silently vanished.
fn write_packs(f: &mut fmt::Formatter<'_>, packs: &Packs) -> fmt::Result {
    writeln!(f, "4. the packs this hub holds")?;
    writeln!(f)?;
    writeln!(
        f,
        "  What is under packs/, measured by file length. Section 3 above is about"
    )?;
    writeln!(
        f,
        "  job output and accounts for none of this: the output budget is a bound on"
    )?;
    writeln!(
        f,
        "  collections, and nothing in Herd weighs these bytes against anything"
    )?;
    writeln!(
        f,
        "  today. They are counted here because docs/disk-policy.md section 0.7 asks"
    )?;
    writeln!(
        f,
        "  that whatever is kept appear in a report with its size. Nothing is"
    )?;
    writeln!(
        f,
        "  removed: who owns the growth of this directory is a question for whoever"
    )?;
    writeln!(f, "  runs this hub, and it is open (ORCH-215).")?;
    writeln!(f)?;
    if !packs.present {
        writeln!(
            f,
            "  THERE IS NO packs/ DIRECTORY ON THIS ROOT, so this hub has never"
        )?;
        writeln!(
            f,
            "  served it at layout 2 or later. That is not an empty registry: an"
        )?;
        writeln!(f, "  empty one is a directory with nothing in it.")?;
        return Ok(());
    }
    if let Some(error) = &packs.manifests_error {
        writeln!(
            f,
            "  THE MANIFESTS DIRECTORY COULD NOT BE READ, so the pack count below is"
        )?;
        writeln!(f, "  about no packs at all and not about none: {error}")?;
    }
    writeln!(
        f,
        "  packs registered                    {} pack(s), {}",
        packs.registered,
        measured_bytes(packs.registered_bytes)
    )?;
    if packs.not_directories > 0 {
        writeln!(
            f,
            "    and {} entry/ies under {}/{PACKS_MANIFESTS_DIR}/ that are not",
            packs.not_directories,
            crate::registry::PACKS_DIR
        )?;
        writeln!(
            f,
            "    directories, so they are not packs and are no part of the count above"
        )?;
    }
    writeln!(
        f,
        "  registrations interrupted           {} directory/ies, {}",
        packs.interrupted,
        measured_bytes(packs.interrupted_bytes)
    )?;
    if packs.interrupted > 0 {
        writeln!(
            f,
            "    Written and never renamed into place, so nothing reads them: the"
        )?;
        writeln!(
            f,
            "    pack does not resolve and its id finds no manifest. The next start of"
        )?;
        writeln!(f, "    a hub on this root removes them by name.")?;
    }
    writeln!(
        f,
        "  everything under {}/             {}",
        crate::registry::PACKS_DIR,
        measured_bytes(packs.bytes)
    )?;
    // The total is measured over the whole directory, so a child nothing
    // above names is still in it. Saying the difference out loud is what
    // makes that true rather than merely intended: a figure here is a
    // directory this section has stopped describing.
    let unaccounted = packs.unaccounted_bytes();
    if unaccounted > 0 {
        writeln!(
            f,
            "  neither of the two above            {}",
            measured_bytes(unaccounted)
        )?;
        writeln!(
            f,
            "    Bytes under {}/ that this section does not describe. The total is",
            crate::registry::PACKS_DIR
        )?;
        writeln!(
            f,
            "    measured over the whole directory on purpose, so that a directory"
        )?;
        writeln!(
            f,
            "    added beside the two above is counted before anything names it."
        )?;
    }
    Ok(())
}

/// Section 5: the records, the event files and the logs, beside the
/// collected output section 3 measured.
///
/// `collected` is [`Accounting::collected_bytes`] and is passed in rather
/// than recomputed, so that the share this prints is a share of the very
/// figure printed four lines above it.
fn write_bookkeeping(
    f: &mut fmt::Formatter<'_>,
    book: &Bookkeeping,
    collected: u64,
) -> fmt::Result {
    writeln!(f, "5. the records, the events and the logs")?;
    writeln!(f)?;
    writeln!(
        f,
        "  What this root spends on knowing what it holds, as against what it holds."
    )?;
    writeln!(
        f,
        "  Sections 1 to 3 are about the collected output and not one of them weighs"
    )?;
    writeln!(
        f,
        "  a job.json, an events.jsonl or a log. Nothing in Herd bounds any of the"
    )?;
    writeln!(
        f,
        "  three: the output budget governs collections, a sweep frees them without"
    )?;
    writeln!(
        f,
        "  touching a record, and the only thing that frees a record's byte or a"
    )?;
    writeln!(
        f,
        "  log's is removing the record. So a root whose collections were all swept"
    )?;
    writeln!(f, "  still grows, and this is the figure that shows it.")?;
    writeln!(f)?;
    writeln!(
        f,
        "  One total, and it is a total: a mean over these directories would misread"
    )?;
    writeln!(
        f,
        "  one the way section 2 says a mean misreads a job, because a single large"
    )?;
    writeln!(
        f,
        "  log can be the whole of the difference. So the heaviest directory is"
    )?;
    writeln!(
        f,
        "  printed beside the total and the three classes are named apart, so that a"
    )?;
    writeln!(
        f,
        "  total which has grown can be attributed without a second command."
    )?;
    writeln!(f)?;
    // **Apparent size, said in the output and not only in a doc comment.**
    // The first consumer's own figure for this very quantity is `du -sk`
    // without `--apparent-size`, which is allocated blocks; this one sums
    // `metadata().len()`, which is apparent size. On a comparable root this
    // section therefore prints roughly half of what their `du` told them,
    // with nothing having shrunk, and the natural reading of that is
    // "records got smaller" and then "retention is less urgent than we
    // thought". The orchestrator made the mirror-image mistake on
    // 2026-10-30 with both measurements in front of it. An operator will
    // have less, so the output carries the unit, the direction of the
    // error, the one measurement behind it, and the instruction that
    // follows from it.
    writeln!(
        f,
        "  Apparent size, summed from file lengths, and that is a FLOOR on what"
    )?;
    writeln!(
        f,
        "  these files occupy. A one-byte file costs a whole block, and a"
    )?;
    writeln!(
        f,
        "  bookkeeping population is almost all small files: on one real root this"
    )?;
    writeln!(
        f,
        "  figure read 1443667 bytes while the same files held 2850816 bytes of"
    )?;
    writeln!(
        f,
        "  blocks -- about 1.97 times more disk than the figure says (measured"
    )?;
    writeln!(
        f,
        "  2026-10-30, find -printf '%b' against find -printf '%s'). That factor is"
    )?;
    writeln!(
        f,
        "  a property of that root's file sizes and its block size and is not a"
    )?;
    writeln!(
        f,
        "  constant to apply anywhere else. So compare du with du, or apparent with"
    )?;
    writeln!(
        f,
        "  apparent, and never one against the other: a du -sk taken before a"
    )?;
    writeln!(
        f,
        "  migration and this figure taken after it would show a root halving while"
    )?;
    writeln!(f, "  nothing at all had shrunk.")?;
    writeln!(f)?;
    writeln!(
        f,
        "  The three collection directories are skipped by name, so no byte is in"
    )?;
    writeln!(
        f,
        "  both this total and section 3's. Nothing under packs/ is in either."
    )?;
    writeln!(f)?;
    // D257's rule, one level down from a version number: at version 1 an
    // absence says two things at once, and a nought printed bare here would
    // say both of them. A root this report found nothing to weigh on and a
    // root whose bookkeeping really weighs nothing are different facts, and
    // the second does not exist -- a `job.json` is never nought bytes -- so
    // the banner is the whole answer rather than a caveat on one.
    if book.directories == 0 {
        writeln!(
            f,
            "  THERE IS NO JOB DIRECTORY ON THIS ROOT TO WEIGH, so every figure below"
        )?;
        writeln!(
            f,
            "  is nought because there was nothing to weigh, and not because this root's"
        )?;
        writeln!(
            f,
            "  bookkeeping weighs nothing. The two are different facts and the second"
        )?;
        writeln!(f, "  one cannot happen: a record is never nought bytes.")?;
    } else if book.records == 0 {
        writeln!(
            f,
            "  NOT ONE JOB DIRECTORY ON THIS ROOT HOLDS A job.json, so the record figure"
        )?;
        writeln!(
            f,
            "  below is nought because there was no record to weigh, and not because a"
        )?;
        writeln!(
            f,
            "  record weighs nothing. Every directory here is one claimed for a job"
        )?;
        writeln!(f, "  whose first record has not landed.")?;
    }
    writeln!(
        f,
        "  job directories weighed             {}",
        book.directories
    )?;
    writeln!(
        f,
        "  the records (job.json)              {} in {} record(s)",
        measured_bytes(book.record_bytes),
        book.records
    )?;
    writeln!(
        f,
        "  the event files (events.jsonl)      {} in {} file(s)",
        measured_bytes(book.event_bytes),
        book.events
    )?;
    writeln!(
        f,
        "  the logs                            {} in {} log(s)",
        measured_bytes(book.log_bytes),
        book.logs
    )?;
    writeln!(
        f,
        "    could not be read                 {} log(s): UNKNOWN, never nought",
        book.logs_unknown.len()
    )?;
    for (job, why) in book.logs_unknown.iter().take(NAMED_AT_MOST) {
        writeln!(f, "      {JOBS_DIR}/{job}: {why}")?;
    }
    if let Some(more) = book.logs_unknown.len().checked_sub(NAMED_AT_MOST) {
        if more > 0 {
            writeln!(f, "      and {more} more")?;
        }
    }
    writeln!(
        f,
        "    no log ever written               {} directory/ies",
        book.without_log
    )?;
    writeln!(
        f,
        "  anything else beside them           {}",
        measured_bytes(book.other_bytes)
    )?;
    writeln!(
        f,
        "    {LOG_SEQ_FILE} today, and whatever a later milestone puts in a job"
    )?;
    writeln!(
        f,
        "    directory. Counted before anything names it, because no mechanism frees"
    )?;
    writeln!(f, "    it either.")?;
    writeln!(
        f,
        "  directories that would not open     {} directory/ies",
        book.unreadable.len()
    )?;
    for unreadable in book.unreadable.iter().take(NAMED_AT_MOST) {
        writeln!(f, "    {}: {}", unreadable.path, unreadable.why)?;
    }
    if let Some(more) = book.unreadable.len().checked_sub(NAMED_AT_MOST) {
        if more > 0 {
            writeln!(f, "    and {more} more")?;
        }
    }
    writeln!(
        f,
        "  the bookkeeping in all              {}",
        measured_bytes(book.bytes())
    )?;
    if !book.logs_unknown.is_empty() {
        writeln!(
            f,
            "    A FLOOR AND NOT A TOTAL: the {} log(s) named above are in no figure",
            book.logs_unknown.len()
        )?;
        writeln!(
            f,
            "    here, because an unmeasured log counted as nought would understate the"
        )?;
        writeln!(f, "    one population on this root that has no bound.")?;
    }
    match &book.largest {
        Some((job, bytes)) => writeln!(
            f,
            "  the heaviest one directory          {} in {JOBS_DIR}/{job}",
            measured_bytes(*bytes)
        )?,
        None => writeln!(
            f,
            "  the heaviest one directory          there is no directory to be the \
             heaviest"
        )?,
    }
    writeln!(
        f,
        "  the collected output beside it      {}",
        measured_bytes(collected)
    )?;
    match book.share_of(collected) {
        Some(share) => writeln!(
            f,
            "  the bookkeeping is                  {:.2}% of those two together",
            share * 100.0
        )?,
        None => writeln!(
            f,
            "  the bookkeeping is                  neither figure above is a byte, so \
             there is no share"
        )?,
    }
    writeln!(
        f,
        "    The collected figure is section 3's three directories, over the records"
    )?;
    writeln!(
        f,
        "    that section could read; this bookkeeping is over every job directory,"
    )?;
    writeln!(
        f,
        "    read or not. Where the two counts differ the share is approximate by"
    )?;
    writeln!(f, "    that much, and the census above says by how many.")?;
    Ok(())
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

    /// Plant one pack under `packs/manifests/`, in the shape the registry
    /// publishes, and answer with the bytes it weighs.
    ///
    /// Not a `PackRegistry::open` and a real registration: what this section
    /// measures is a **directory on a disk**, and a test that went through the
    /// registry's API would prove the API agrees with itself. The identity
    /// rule is honoured anyway — the directory's name is the sha256 of the
    /// manifest beside it — so nothing here is a shape the hub would refuse.
    fn plant_a_pack(root: &Path, which: u8, filler: usize) -> u64 {
        let manifest = format!("# pack {which}\n{}\n", "x".repeat(filler));
        let manifest = manifest.into_bytes();
        let id = herd_proto::sha256_hex(&manifest);
        let at = root
            .join(crate::registry::PACKS_DIR)
            .join(PACKS_MANIFESTS_DIR)
            .join(&id);
        fs::create_dir_all(&at).expect("a planted pack directory");
        fs::write(at.join("manifest.tsv"), &manifest).expect("a planted manifest");
        manifest.len() as u64
    }

    /// **The population, with its size** (§0.7, ORCH-256).
    ///
    /// Three packs of three different weights, so that a total which happened
    /// to be a multiple of the count could not pass for the sum, and every
    /// figure is known to this test by construction.
    #[test]
    fn the_report_names_the_pack_population_with_its_size() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        job_dir(root, "j0000000001", Some(readable("j0000000001")));
        let planted: u64 = [3usize, 97, 1_000]
            .iter()
            .enumerate()
            .map(|(which, filler)| plant_a_pack(root, which as u8, *filler))
            .sum();

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert!(report.packs.present, "packs/ is on this root");
        assert_eq!(report.packs.registered, 3, "three directories, three packs");
        assert_eq!(
            report.packs.registered_bytes, planted,
            "the figure is the planted bytes and nothing else"
        );
        assert_eq!(
            report.packs.bytes, planted,
            "the whole of packs/ is the three packs, measured over the directory"
        );
        assert_eq!(report.packs.unaccounted_bytes(), 0);
        assert_eq!(report.packs.interrupted, 0);
        assert_eq!(report.packs.not_directories, 0);

        let text = rendered(root);
        assert!(
            text.contains("4. the packs this hub holds\n"),
            "the section is there:\n{text}"
        );
        assert!(
            text.contains(&format!(
                "  packs registered                    3 pack(s), {}\n",
                measured_bytes(planted)
            )),
            "the population and its size, on one line:\n{text}"
        );
        assert!(
            text.contains("ORCH-215"),
            "the line says removal is somebody's open question and not this \
             report's doing:\n{text}"
        );
    }

    /// An empty registry and a root with no `packs/` at all are two different
    /// facts, and the report says which.
    ///
    /// Nought packs printed on a root a hub has never served would be the
    /// same sentence as nought packs on one serving a fleet, and an operator
    /// could not tell them apart — the mistake section 3's banners already
    /// exist to avoid.
    #[test]
    fn a_root_with_no_packs_directory_is_not_an_empty_registry() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        job_dir(root, "j0000000001", Some(readable("j0000000001")));

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert!(!report.packs.present);
        assert_eq!(report.packs.registered, 0);
        let text = rendered(root);
        assert!(
            text.contains("THERE IS NO packs/ DIRECTORY ON THIS ROOT"),
            "a root no hub of layout 2 served says so:\n{text}"
        );
        assert!(
            !text.contains("packs registered                    0 pack(s)"),
            "and it does not print nought packs as the answer:\n{text}"
        );

        // The empty registry: the directory is there with nothing in it.
        fs::create_dir_all(
            root.join(crate::registry::PACKS_DIR)
                .join(PACKS_MANIFESTS_DIR),
        )
        .expect("an empty registry");
        let report = StateRootReport::read(root).expect("the report reads this root");
        assert!(report.packs.present);
        assert_eq!(report.packs.registered, 0);
        let text = rendered(root);
        assert!(
            !text.contains("THERE IS NO packs/ DIRECTORY"),
            "an empty registry is not an absent one:\n{text}"
        );
        assert!(
            text.contains("  packs registered                    0 pack(s), 0 bytes\n"),
            "and nought is the honest figure here:\n{text}"
        );
    }

    /// An interrupted registration is real disk and is named as residue, not
    /// as population.
    #[test]
    fn a_registration_that_was_interrupted_is_counted_apart_from_the_packs() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        let planted = plant_a_pack(root, 0, 11);
        let residue = root
            .join(crate::registry::PACKS_DIR)
            .join(PACKS_MANIFESTS_PART_DIR)
            .join("staging-7");
        fs::create_dir_all(&residue).expect("the residue directory");
        fs::write(residue.join("manifest.tsv"), vec![b'z'; 40]).expect("residue bytes");

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.packs.registered, 1, "one published pack");
        assert_eq!(report.packs.registered_bytes, planted);
        assert_eq!(report.packs.interrupted, 1);
        assert_eq!(report.packs.interrupted_bytes, 40);
        assert_eq!(
            report.packs.bytes,
            planted + 40,
            "the total holds both, because both are disk"
        );
        assert_eq!(
            report.packs.unaccounted_bytes(),
            0,
            "and the two figures together account for the total"
        );
        let text = rendered(root);
        assert!(
            text.contains("  registrations interrupted           1 directory/ies, 40 bytes\n"),
            "the residue is named with its size:\n{text}"
        );
        assert!(
            text.contains("removes them by name"),
            "and the line says a restart frees it:\n{text}"
        );
    }

    /// A directory under `packs/` that this build does not name is still in
    /// the total, and the difference is printed.
    ///
    /// The property the total exists for: the pack population went uncounted
    /// for a whole milestone because nothing measured the directory it was
    /// in. A figure summed from the children this build happens to know would
    /// repeat that, one directory further down.
    #[test]
    fn bytes_under_packs_that_no_figure_names_are_still_in_the_total() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        let planted = plant_a_pack(root, 0, 11);
        let stranger = root.join(crate::registry::PACKS_DIR).join("objects");
        fs::create_dir_all(&stranger).expect("a directory nothing here names");
        fs::write(stranger.join("blob"), vec![b'q'; 500]).expect("its bytes");

        let report = StateRootReport::read(root).expect("the report reads this root");
        assert_eq!(report.packs.registered_bytes, planted);
        assert_eq!(report.packs.bytes, planted + 500);
        assert_eq!(report.packs.unaccounted_bytes(), 500);
        let text = rendered(root);
        assert!(
            text.contains("  neither of the two above            500 bytes\n"),
            "bytes no figure names are named as such:\n{text}"
        );
    }

    /// **The guard on the one name this file spells again.**
    ///
    /// [`PACKS_MANIFESTS_DIR`] is `registry.rs`'s `MANIFESTS_DIR`, written out
    /// here because that constant is private to that module and this report
    /// may not open a registry to ask. So a real [`crate::PackRegistry`] is
    /// opened on a temporary root — which is exactly what this report refuses
    /// to do on a live one — and the directory it makes is compared against
    /// the path this file walks. If the two ever drift, this reddens instead
    /// of the report quietly answering nought.
    #[test]
    fn the_report_walks_the_directory_the_registry_publishes_into() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        crate::registry::PackRegistry::open(root).expect("a registry opens on an empty root");

        let walked = root
            .join(crate::registry::PACKS_DIR)
            .join(PACKS_MANIFESTS_DIR);
        assert!(
            walked.is_dir(),
            "the registry made {}, which is the directory this report walks",
            walked.display()
        );
        // And the report agrees with it on the same root: present, and empty.
        let report = StateRootReport::read(root).expect("the report reads this root");
        assert!(report.packs.present);
        assert_eq!(report.packs.registered, 0);
        assert!(report.packs.manifests_error.is_none());
    }

    // ------------------------------------------------------------------
    // Figure 5: the records, the events and the logs (ORCH-282).
    // ------------------------------------------------------------------

    /// Every regular file under `jobs/`, by length, walked by the **test**
    /// and not by the code under test.
    ///
    /// The whole of section 5's disjointness claim is checked against this:
    /// the bookkeeping plus section 3's collected bytes must come to exactly
    /// this figure, so a byte counted twice and a byte counted nowhere both
    /// redden the same assertion. A sum computed with the report's own
    /// `directory_bytes` would have agreed with the report for the report's
    /// own reasons.
    fn every_byte_under_jobs(root: &Path) -> u64 {
        fn walk(at: &Path) -> u64 {
            let Ok(entries) = fs::read_dir(at) else {
                return 0;
            };
            let mut bytes = 0u64;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    bytes += walk(&path);
                } else if let Ok(at) = fs::metadata(&path) {
                    bytes += at.len();
                }
            }
            bytes
        }
        walk(&root.join(JOBS_DIR))
    }

    /// One job directory with a record, an event file, a log, the hub's
    /// counter for it, and a collected file: all five of the things a job
    /// directory holds, so that the classification can be asserted and not
    /// just the sum.
    fn a_whole_job_dir(root: &Path, name: &str, events: usize, log: usize, kept: usize) {
        job_dir(root, name, Some(readable(name)));
        let dir = root.join(JOBS_DIR).join(name);
        fs::write(dir.join(EVENTS_FILE), vec![b'e'; events]).expect("the event file");
        fs::write(dir.join(LOG_FILE), vec![b'l'; log]).expect("the log");
        fs::write(dir.join(LOG_SEQ_FILE), vec![b'0'; 20]).expect("the counter");
        let files = dir.join(KEPT_FILES_DIR);
        fs::create_dir_all(&files).expect("room for the collection");
        fs::write(files.join("out.bin"), vec![b'o'; kept]).expect("the collected file");
    }

    /// **The condition v1.0.0 ships on**: the records, the events and the
    /// logs are weighed, with their counts, beside the collected output and
    /// not inside it.
    ///
    /// Three things at once, and the third is the one the first consumer
    /// asked for:
    ///
    /// 1. Each class is weighed and counted **apart**, so a total that grew
    ///    can be attributed.
    /// 2. The bookkeeping and section 3's collected figure **partition**
    ///    `jobs/`: their sum is every byte under it, walked independently by
    ///    the test. A byte counted twice fails this, and so does one counted
    ///    nowhere.
    /// 3. The two are printed **beside** each other with the bookkeeping's
    ///    share, which is how the growth gets seen rather than discovered.
    #[test]
    fn the_report_weighs_the_records_the_events_and_the_logs_beside_the_output() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        a_whole_job_dir(root, "j0000000001", 700, 9_000, 300);
        a_whole_job_dir(root, "j0000000002", 700, 9_000, 300);
        // The heaviest, by its log alone.
        a_whole_job_dir(root, "j0000000003", 700, 50_000, 300);

        let report = StateRootReport::read(root).expect("a report, never a refusal");
        let book = &report.bookkeeping;
        assert_eq!(book.directories, 3);
        assert_eq!(book.records, 3);
        assert_eq!(book.events, 3);
        assert_eq!(book.event_bytes, 3 * 700);
        assert_eq!(book.logs, 3);
        assert_eq!(book.log_bytes, 9_000 + 9_000 + 50_000);
        assert!(book.logs_unknown.is_empty());
        assert_eq!(book.without_log, 0);
        // `log.seq` and nothing else, at 20 bytes a directory.
        assert_eq!(book.other_bytes, 3 * 20);
        assert!(book.unreadable.is_empty());
        // The records are real JSON, so their length is read rather than
        // asserted as a literal: what matters is that it is the `job.json`
        // lengths and no other file's.
        let records: u64 = (1..=3)
            .map(|n| {
                fs::metadata(
                    root.join(JOBS_DIR)
                        .join(format!("j{n:0>10}"))
                        .join(RECORD_FILE),
                )
                .expect("the record is there")
                .len()
            })
            .sum();
        assert_eq!(book.record_bytes, records);
        assert_eq!(
            book.bytes(),
            records + 3 * 700 + 68_000 + 3 * 20,
            "the total is its four classes and nothing else"
        );

        // Section 3 measured the collection and only the collection.
        assert_eq!(report.accounting.collected_bytes(), 3 * 300);

        // The partition, against a walk the test did itself.
        assert_eq!(
            book.bytes() + report.accounting.collected_bytes(),
            every_byte_under_jobs(root),
            "every byte under jobs/ is in exactly one of the two totals"
        );

        // The heaviest directory, and it is the one with the big log.
        assert_eq!(
            book.largest,
            Some((
                "j0000000003".to_owned(),
                fs::metadata(root.join(JOBS_DIR).join("j0000000003").join(RECORD_FILE))
                    .expect("the record is there")
                    .len()
                    + 700
                    + 50_000
                    + 20
            ))
        );

        let text = rendered(root);
        assert!(
            text.contains("5. the records, the events and the logs\n"),
            "section 5 is missing:\n{text}"
        );
        assert!(
            text.contains(
                "  the event files (events.jsonl)      2100 bytes (2.05 KiB) in 3 file(s)\n"
            ),
            "the event files are weighed with their count:\n{text}"
        );
        assert!(
            text.contains(
                "  the logs                            68000 bytes (66.41 KiB) in 3 log(s)\n"
            ),
            "the logs are weighed with their count:\n{text}"
        );
        assert!(
            text.contains("  the collected output beside it      900 bytes\n"),
            "the output figure is printed beside it:\n{text}"
        );
        assert!(
            text.contains("  the heaviest one directory          "),
            "the maximum is printed, so the total cannot pass for an even root:\n{text}"
        );
        assert!(
            text.contains("% of those two together\n"),
            "the share is printed:\n{text}"
        );
        // Rule 2 of this file, said in the output and not only here.
        assert!(
            text.contains("  One total, and it is a total:"),
            "a total says it is one:\n{text}"
        );
    }

    /// A log this build could not measure is **UNKNOWN**, the total says it
    /// is a floor, and no figure reads it as nought.
    ///
    /// The case is reached the way an operator reaches it: a `log.seq`, which
    /// is this hub's own record that a log was being counted, with no `log`
    /// beside it. Nought there would understate the one population on a state
    /// root that nothing bounds.
    #[test]
    fn a_log_that_could_not_be_measured_is_unknown_and_never_nought() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        job_dir(root, "j0000000001", Some(readable("j0000000001")));
        fs::write(
            root.join(JOBS_DIR).join("j0000000001").join(LOG_SEQ_FILE),
            vec![b'0'; 20],
        )
        .expect("the counter with no log beside it");

        let report = StateRootReport::read(root).expect("a report, never a refusal");
        let book = &report.bookkeeping;
        assert_eq!(book.logs, 0, "nothing was measured");
        assert_eq!(book.log_bytes, 0);
        assert_eq!(
            book.without_log, 0,
            "this is not a directory where nothing ever wrote a log"
        );
        assert_eq!(book.logs_unknown.len(), 1);
        assert_eq!(book.logs_unknown[0].0, "j0000000001");

        let text = rendered(root);
        assert!(
            text.contains(
                "    could not be read                 1 log(s): UNKNOWN, never nought\n"
            ),
            "the unmeasured log is named as unknown:\n{text}"
        );
        assert!(
            text.contains(
                "    A FLOOR AND NOT A TOTAL: the 1 log(s) named above are in no figure\n"
            ),
            "the total says it is a floor:\n{text}"
        );
    }

    /// **D257 on this root's figure.** A root with no record says so, rather
    /// than printing a nought that means two things at once.
    ///
    /// Two roots, two sentences, because the two facts send an operator
    /// somewhere else: a root with no job directory at all, and one whose
    /// directories are claimed and whose first records have not landed.
    #[test]
    fn a_root_with_no_record_says_so_rather_than_weighing_nought() {
        let empty = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir_all(empty.path().join(JOBS_DIR)).expect("an empty jobs directory");
        let text = rendered(empty.path());
        assert!(
            text.contains(
                "  THERE IS NO JOB DIRECTORY ON THIS ROOT TO WEIGH, so every figure below\n"
            ),
            "an empty root says there was nothing to weigh:\n{text}"
        );
        assert!(
            text.contains("  one cannot happen: a record is never nought bytes.\n"),
            "and says why the other reading is impossible:\n{text}"
        );

        let claimed = tempfile::tempdir().expect("a temporary directory");
        job_dir(claimed.path(), "j0000000001", None);
        job_dir(claimed.path(), "j0000000002", None);
        let report = StateRootReport::read(claimed.path()).expect("a report");
        assert_eq!(report.bookkeeping.directories, 2);
        assert_eq!(report.bookkeeping.records, 0);
        assert_eq!(report.bookkeeping.bytes(), 0);
        let text = rendered(claimed.path());
        assert!(
            text.contains(
                "  NOT ONE JOB DIRECTORY ON THIS ROOT HOLDS A job.json, so the record figure\n"
            ),
            "a root whose records have not landed says so:\n{text}"
        );
        assert!(
            !text.contains("  THERE IS NO JOB DIRECTORY ON THIS ROOT TO WEIGH"),
            "and it is not the other sentence:\n{text}"
        );
    }

    /// A directory whose record this build cannot parse is **still weighed**.
    ///
    /// Sections 1 to 3 stop at such a record, and rightly: there is no claim
    /// to reconcile. Its bytes are on the disk all the same, and a growth
    /// figure that skipped them would be the one figure in this report that
    /// understates what the root holds. So the count here is the census's
    /// `directories` and not its `read`, and that is asserted rather than
    /// described.
    #[test]
    fn a_record_this_build_cannot_read_is_still_weighed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path();
        a_whole_job_dir(root, "j0000000001", 700, 9_000, 300);
        let other = root.join(JOBS_DIR).join("j0000000002");
        job_dir(
            root,
            "j0000000002",
            Some(from_another_layout("j0000000002")),
        );
        fs::write(other.join(EVENTS_FILE), vec![b'e'; 1_100]).expect("its event file");
        fs::write(other.join(LOG_FILE), vec![b'l'; 4_000]).expect("its log");

        let report = StateRootReport::read(root).expect("a report, never a refusal");
        assert_eq!(report.census.directories, 2);
        assert_eq!(report.census.read, 1, "section 3 reads one of the two");
        assert_eq!(report.census.unread.len(), 1);
        let book = &report.bookkeeping;
        assert_eq!(book.directories, 2, "section 5 weighs both");
        assert_eq!(book.records, 2, "including the record it cannot parse");
        assert_eq!(book.event_bytes, 700 + 1_100);
        assert_eq!(book.log_bytes, 9_000 + 4_000);
    }
}
