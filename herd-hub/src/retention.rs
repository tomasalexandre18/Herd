//! What record retention *would* remove, and what it would cost (ORCH-146).
//!
//! **It removes nothing.** There is no deletion path in this module, no
//! `--apply`, no flag that a later change could invert. M12's retention
//! engine is a separate task; this is the thing that has to exist before it,
//! and it exists for one reason: once the hub acknowledges a collection the
//! runner's spool goes through that acknowledgement, so a record is the only
//! copy of a requester's evidence, and **nothing in Herd can undo its
//! removal**. A policy that destroys the only copy on a timer is owed a way
//! to be read before it runs.
//!
//! **Two questions, one walk** (ORCH-097). A state root is read by two
//! mechanisms with two different ideas of what a job directory is worth, and
//! an operator sizing a disk has to see both at once: a **span** takes the
//! whole record and everything under it, and the **output budget**'s sweep
//! takes the collected bytes under `files/` and leaves the record standing
//! (D152). So this walks the root once and answers both —
//! [`RetentionPreview::sweep`] is what a sweep would take, in the order a
//! sweep takes it, from the same `read_dir` of the same directory. That is
//! the whole of ORCH-097: *"what would this budget take, with nothing
//! removed"*.
//!
//! **And it is answered without being given a budget**, which is the one
//! design decision in it. This tool is not handed the hub's
//! `--output-budget` or its `--output-floor` — they are the serving hub's
//! arguments, not this root's, and nothing on the disk records them — so
//! instead of answering for one figure it prints the sweep's order with a
//! running total of what is freed by the time each candidate is gone. An
//! operator reads down that column until it reaches `held − budget` and has
//! their answer for **any** budget, including the one they are thinking of
//! raising it to. No new flag was added to get it, and there is therefore
//! still nothing here a later change could invert into a deletion.
//!
//! **It is also where the engine's selection lives.** [`select`] is the one
//! decision retention ever makes about one record, and the engine will call
//! it rather than re-derive it: a preview that answered a slightly different
//! question from the engine would be worse than no preview, because it would
//! be believed. [`Judged`] is the three facts that decision rests on, read
//! off a record by [`Judged::of`], so the engine and this preview cannot
//! drift.
//!
//! **No span is built into this binary.** Every span is a required argument
//! with no default, which is [`crate::report`]'s rule (D194) applied here: a
//! shipped binary stating an unrecorded policy is a number nobody can later
//! change. The 14/90 figures live in `docs/plan.md`, `docs/coverage.md` and
//! `docs/disk-policy.md`; the class map is Alexa's open question, and four
//! required spans is what lets this tool be run against any answer to it.
//!
//! **It takes no lock**, for the reasons [`crate::report`] gives at length:
//! not the store's `Mutex<Live>` — it never builds a
//! [`crate::store::JobDirStore`] — and above all not the `flock(2)` on
//! `hub.lock`, which a serving hub holds for its whole life. It opens files
//! read-only, one at a time, creates nothing and writes nothing. The figures
//! are therefore not one consistent instant, and the preview says so itself.
//!
//! **Four things it settles rather than assumes**, each with a test in
//! `herd-hub/tests/retention_preview.rs`:
//!
//! 1. `updated_at` is a stable *end* time and not a last-touch clock, so a
//!    span measured from it is a span and not an LRU.
//! 2. A clock that cannot be used against this root's own records, and a
//!    pass that would empty the root, both **refuse** and name the figures
//!    rather than report that everything is expired ([`Verdict`]).
//! 3. A span of `0` means **never expire**, said in those words.
//! 4. A record that is not over, and a quarantined record, can never be
//!    shown as expired: the first is the reaper's (ORCH-012, D134) and the
//!    second has no end time this build may trust (D52).
//!
//! **Three populations, three owners, and never one number** (ORCH-168).
//! What a job directory weighs is not one quantity: the **collected files**
//! under `files/`, `files.part/` and `files.gone/` are the output budget's
//! (D148, D152) and a sweep frees them without touching the record; the
//! **record** — `job.json`, `events.jsonl`, `log.seq` — and the **log** are
//! nobody's, and the only thing in Herd that frees one of their bytes is
//! removing the record. Nothing on the hub bounds a log at all: the only cap
//! on it is the *runner's* `--job-log-cap`, 64 MiB by default, and
//! `append_log` writes what arrives. So a record is not kilobytes merely
//! because its `job.json` is, and the argument the output budget makes about
//! itself (`herd-hub/src/main.rs`, *"a record is kilobytes and a log has a
//! cap of its own"*) is about the budget's own scope and not about this
//! figure. A preview printing one undifferentiated total could not size a
//! policy against any of the three, which is why [`Weight`] prints each with
//! its count, on both sides of every span. A log this build could not
//! measure is [`LogWeight::Unknown`] and is **named**, never nought.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::job::{in_days, JobRecord, JobState, OutputSummary, Timestamp, RECORD_VERSION};
use crate::report::Unread;
use crate::store::{
    directory_bytes, JOBS_DIR, KEPT_FILES_DIR, OUTPUT_GONE_DIR, OUTPUT_PARTS_DIR, RECORD_FILE,
};
use herd_proto::{LabelSet, Priority};
// The measured-bytes rule, one body in `herd-proto` beside the rule for a
// figure a human typed (ORCH-186). This preview's byte figures are all
// measurements or record claims, so they print exactly, with a binary
// approximation beside them and never instead of them.
use herd_proto::size::measured_bytes;

/// How many records of one kind the preview names before it stops listing and
/// says how many more there are. Ten, for [`crate::report`]'s reason: a list
/// exists to be acted on, and one naming four thousand ids is a number, which
/// the count already is.
const NAMED_AT_MOST: usize = 10;

/// How wide a label in [`write_weight`] is padded to, so that its figure
/// lands in the same column as the figures of the lines around it: the
/// per-class block is indented six and the root block four, and both put
/// their value where every other figure of this output puts it.
const LABEL_WIDTH: usize = 23;

/// The span each priority class is kept for, as one invocation was given it.
///
/// **Four spans and no default.** Not two with a map from four levels onto
/// them: that map is Alexa's open question, and a tool that baked one in
/// could not be run against the other answer. Indexed by
/// [`Priority::rank`], which is the order every other listing of the levels
/// uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spans {
    by_rank: [Duration; 4],
}

impl Spans {
    /// One span per class, highest class first — the order of
    /// [`Priority::ALL`].
    pub fn new(merger: Duration, step: Duration, nightly: Duration, producer: Duration) -> Self {
        Self {
            by_rank: [merger, step, nightly, producer],
        }
    }

    /// The span this class is kept for. [`Duration::ZERO`] means **never
    /// expires**; see [`Spans::never_expires`].
    pub fn of(&self, class: Priority) -> Duration {
        self.by_rank[class.rank() as usize]
    }

    /// Whether this class never expires, which is what a span of `0` means.
    ///
    /// **The opposite of `--output-floor 0`, deliberately.** There, zero
    /// means "no promise", so everything is evictable; the symmetric reading
    /// here would mean "delete every record now", which is a trap: a span
    /// left unset, a shell variable that expanded to nothing, a `0` typed
    /// while reaching for `90`, and the only copy of every requester's
    /// evidence is gone. The two settings are not symmetric because their
    /// failures are not: the floor's zero loses bytes a budget was going to
    /// take anyway, and this zero would lose the records. So zero is the
    /// safe end of this one, and the preview prints it in those words.
    pub fn never_expires(&self, class: Priority) -> bool {
        self.of(class).is_zero()
    }
}

/// Everything retention judges one record by, and nothing else.
///
/// A type rather than three arguments, so the engine and the preview read the
/// same three facts off a record and cannot diverge on a fourth. There is no
/// fourth: not the outcome, not whether the bytes were fetched, not their
/// size. A span is time since the job ended, full stop — the moment it is
/// anything else it is a cache eviction policy wearing retention's name, and
/// D152 has already said which way that sign points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judged {
    /// The level the record carries. Written at birth; aging never touches
    /// it (see [`JobRecord::priority`]).
    pub class: Priority,
    /// Whether the job is over. Only a job that is over has an end time.
    pub terminal: bool,
    /// When the job's state last changed, which for a job that is over is
    /// when it ended.
    ///
    /// **A stable end time and not a last-touch clock**, which is settlement
    /// 1 and is asserted in the tests: `note_outputs_fetched` writes
    /// `first_fetched_at` and not this, re-admitting an evicted collection
    /// deliberately does not touch this, and `write_output_chunk` writes no
    /// record at all. If it moved on a fetch, a span measured from it would
    /// be an LRU: evidence nobody has looked at would go **first**, which is
    /// the wrong way round by exactly the reasoning D152 used to keep an
    /// unfetched collection longer than a fetched one.
    pub ended_at: Timestamp,
}

impl Judged {
    /// The three facts, off a record this build parsed.
    ///
    /// Quarantine is not among them and cannot be: a quarantined record never
    /// becomes a [`JobRecord`] at all (`read_stored` answers
    /// `Stored::Quarantined` instead), so the only way to see one is to read
    /// the file, which is what this module's own reader does.
    pub fn of(record: &JobRecord) -> Self {
        Self {
            class: record.priority,
            terminal: record.state.is_terminal(),
            ended_at: record.updated_at,
        }
    }
}

/// Why a record is not retention's to judge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOurs {
    /// The job is not over. It belongs to the reaper (ORCH-012, D134) and to
    /// nothing here: a job that is queued, assigned, running or interrupted
    /// has no end time at all, and **retention must not become the reaper by
    /// accident** (`docs/disk-policy.md` §3.5).
    NotOver,
    /// The record is readable and this hub will not act on it (D52). It has
    /// no end time this build may trust, so it can never be *shown* to be
    /// past its span — which is the argument and not merely the precedent.
    ///
    /// Never returned by [`select`], which is only ever handed a record that
    /// parsed: a quarantined record never becomes a [`JobRecord`] at all. The
    /// reader of a state root is what answers this, and the reason is kept
    /// beside the id in [`Scanned::quarantined`].
    Quarantined,
}

/// The one decision retention ever makes about one record.
///
/// [`Selection::Expired`] is the only variant an engine may act on, and
/// [`Selection::is_expired`] is the only test it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Past its class's span, by this much. Removable.
    Expired { past_by: Duration },
    /// Inside its class's span, with this much of it left.
    Inside { left: Duration },
    /// Its class's span is `0`, which means never (see
    /// [`Spans::never_expires`]).
    NeverExpires,
    /// Not retention's to judge.
    NotOurs(NotOurs),
}

impl Selection {
    /// Whether retention would remove this record. The engine's one test.
    pub fn is_expired(&self) -> bool {
        matches!(self, Selection::Expired { .. })
    }
}

/// Whether retention would remove this record, at this reading of the clock.
///
/// **The selection function, and the reason this task comes before the
/// engine.** The engine walks records in id order and calls this; the preview
/// walks every record and calls this. One definition, so "what the preview
/// said would go" and "what the engine takes" are the same sentence.
///
/// The boundary is `>=`: a record is kept **for** its span, so at exactly its
/// span it has been kept for the whole of it and becomes removable. That
/// instant is reachable once and the figure is printed either way.
///
/// [`Timestamp::since`] saturates backwards, so a clock behind this record's
/// own `updated_at` makes it read as nought old and therefore *inside* its
/// span — safe, and never the other way round. Safe is not the same as
/// usable, which is what [`Verdict`] is for: a pass does not run on a clock
/// this root's own records contradict.
pub fn select(judged: Judged, spans: &Spans, now: Timestamp) -> Selection {
    if !judged.terminal {
        return Selection::NotOurs(NotOurs::NotOver);
    }
    let span = spans.of(judged.class);
    if span.is_zero() {
        return Selection::NeverExpires;
    }
    let age = now.since(judged.ended_at);
    match age.checked_sub(span) {
        Some(past_by) => Selection::Expired { past_by },
        None => Selection::Inside {
            left: span.saturating_sub(age),
        },
    }
}

/// What the walk found, and how much of it became a figure.
///
/// An unreadable record is counted and named rather than skipped, which is
/// [`crate::report::Unread`]'s rule — D27's "a thing that did not run is
/// never reported as one that did", applied to a record that was not read.
/// Here it does more than caveat the figures: **a record left out of the
/// arithmetic is a record retention must never remove**, so this is a bound
/// on the engine and not a footnote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scanned {
    /// Directories in `jobs/`.
    pub directories: usize,
    /// Of those, the ones whose `job.json` this build read as a record.
    pub read: usize,
    /// Of those, the ones with no `job.json` at all — a directory claimed
    /// for a job whose first record has not landed. A state, not a fault,
    /// and never retention's.
    pub without_record: usize,
    /// Readable and not actionable (D52). Never shown as expired.
    pub quarantined: Vec<(String, String)>,
    /// The rest, each with its reason.
    pub unread: Vec<Unread>,
}

/// What a job's log weighs, or why this build cannot say.
///
/// **The log is the population nothing bounds.** The hub appends what
/// arrives and caps nothing; the only cap is the runner's `--job-log-cap`,
/// 64 MiB a job by default, on somebody else's machine. So a log is a figure
/// worth printing in its own right, and one that could not be read is worth
/// printing as unread: a log counted as empty because it could not be
/// measured is a record whose removal frees more than the preview promised,
/// which is the direction a sizing figure must never err in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogWeight {
    /// The `log` file, by `metadata().len()`.
    Measured(u64),
    /// This build could not measure it, with the reason. **Unknown, and
    /// never nought bytes.**
    Unknown(String),
    /// There is no `log` **and** no `log.seq`, which is the counter the hub
    /// writes the moment a job is assigned. So nothing ever wrote a log byte
    /// for this record — a job cancelled while queued ends here — and nought
    /// is a measurement rather than a gap. A variant of its own and not
    /// `Measured(0)`, so that the two cannot be added up by accident.
    NeverWritten,
}

impl LogWeight {
    /// The bytes, when there are bytes to be had. `None` is unmeasured, and
    /// nothing that sums may read it as nought.
    pub fn bytes(&self) -> Option<u64> {
        match self {
            LogWeight::Measured(bytes) => Some(*bytes),
            LogWeight::Unknown(_) => None,
            LogWeight::NeverWritten => Some(0),
        }
    }
}

/// What one job directory weighs, in the three populations that own a state
/// root's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weighed {
    /// `files/`, `files.part/` and `files.gone/`: what the collection
    /// weighs. The output budget already governs these (D148, D152), and a
    /// sweep frees them without touching the record.
    pub collected_bytes: u64,
    /// How many files that is, at every depth. Beside the bytes because a
    /// byte figure with no count behind it cannot be checked, and because
    /// "names no file at all" is the first consumer's common case.
    pub collected_files: usize,
    /// Of those, what `files/` alone holds: the collection a requester can
    /// fetch, and **the only one of the three the output budget counts**
    /// (ORCH-097). Split out rather than measured again, from the same
    /// `read_dir`.
    pub kept_bytes: u64,
    /// How many files that is.
    pub kept_files: usize,
    /// `files.part/`: a collection still arriving, whose bytes are the
    /// writer's and are never a sweep candidate.
    pub arriving_bytes: u64,
    /// `files.gone/`: what a hub killed mid-eviction left behind, which a
    /// sweep gives back before it chooses anything.
    pub leftover_bytes: u64,
    /// Whether there is such a tree at all, which is the count a sweep's own
    /// sentence would carry.
    pub leftover: bool,
    /// Everything beside the log that is not a collected file: `job.json`,
    /// `events.jsonl`, `log.seq`, and any directory a later milestone adds,
    /// which lands here on purpose — no other mechanism can free it, so
    /// counting it here over-states what only a record removal can take and
    /// never under-states it.
    pub record_bytes: u64,
    /// The log, which nothing on the hub bounds.
    pub log: LogWeight,
}

impl Weighed {
    /// Every byte of this directory this build measured. `None` when the log
    /// could not be measured: `collected + record` is then a total of two of
    /// the three populations, and printing it under the name of all three is
    /// the mistake [`LogWeight::Unknown`] exists to prevent.
    pub fn measured_bytes(&self) -> Option<u64> {
        let log = self.log.bytes()?;
        Some(
            self.collected_bytes
                .saturating_add(self.record_bytes)
                .saturating_add(log),
        )
    }
}

/// What one side of a span weighs: the three populations, each with its
/// count.
///
/// One per class per side, and one more summed over the whole root
/// ([`RetentionPreview::past`], [`RetentionPreview::inside`]). Every record
/// on this side is accounted for exactly once in the log figures:
/// `logs + logs_unknown.len() + without_log == records`, which is asserted
/// rather than described.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Weight {
    /// Records on this side of the span.
    pub records: usize,
    /// What their collections weigh, and how many files that is.
    pub collected_bytes: u64,
    pub collected_files: usize,
    /// Of `records`, the ones naming no collected file at all. **The common
    /// case and not the edge**: 41 % of the first consumer's records name no
    /// file, and for every one of them the whole of what removing it frees
    /// is its record and its log.
    pub without_collected_file: usize,
    /// What the records themselves weigh: `job.json`, `events.jsonl`,
    /// `log.seq`.
    pub record_bytes: u64,
    /// The logs this build measured, and what they weigh.
    pub log_bytes: u64,
    pub logs: usize,
    /// The logs it could not measure, each named with its reason. Never
    /// folded into `log_bytes` as nought.
    pub logs_unknown: Vec<(String, String)>,
    /// Records with neither a log nor a `log.seq`: nothing ever wrote one,
    /// which is a measurement and not a gap.
    pub without_log: usize,
}

impl Weight {
    /// Add one record's three figures.
    fn add(&mut self, id: &str, weighed: &Weighed) {
        self.records += 1;
        self.collected_bytes = self.collected_bytes.saturating_add(weighed.collected_bytes);
        self.collected_files += weighed.collected_files;
        if weighed.collected_files == 0 {
            self.without_collected_file += 1;
        }
        self.record_bytes = self.record_bytes.saturating_add(weighed.record_bytes);
        match &weighed.log {
            LogWeight::Measured(bytes) => {
                self.logs += 1;
                self.log_bytes = self.log_bytes.saturating_add(*bytes);
            }
            LogWeight::Unknown(why) => self.logs_unknown.push((id.to_owned(), why.clone())),
            LogWeight::NeverWritten => self.without_log += 1,
        }
    }

    /// Another side's or another class's figures, added in.
    fn absorb(&mut self, other: &Weight) {
        self.records += other.records;
        self.collected_bytes = self.collected_bytes.saturating_add(other.collected_bytes);
        self.collected_files += other.collected_files;
        self.without_collected_file += other.without_collected_file;
        self.record_bytes = self.record_bytes.saturating_add(other.record_bytes);
        self.log_bytes = self.log_bytes.saturating_add(other.log_bytes);
        self.logs += other.logs;
        self.logs_unknown.extend(other.logs_unknown.iter().cloned());
        self.without_log += other.without_log;
    }

    /// Every byte measured on this side, over the three populations.
    ///
    /// A record whose log could not be measured contributes its collected
    /// and record bytes and no log byte, so this is a **floor** whenever
    /// `logs_unknown` is not empty — which is why the two are printed
    /// together and never apart.
    pub fn measured_bytes(&self) -> u64 {
        self.collected_bytes
            .saturating_add(self.record_bytes)
            .saturating_add(self.log_bytes)
    }
}

/// One record the preview has something to say about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The job directory's name, which is the job id.
    pub id: String,
    pub class: Priority,
    pub ended_at: Timestamp,
    /// **What removing this record would free**, split into the three
    /// populations that own it. The only figure the question "what would it
    /// cost me?" can mean, and three figures because three different things
    /// can free it — or, for two of them, nothing can.
    pub weighed: Weighed,
}

/// What one class would lose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassPreview {
    /// Every record of this class, whatever its state.
    pub records: usize,
    /// Of those, the ones not over — the reaper's, never retention's.
    pub not_over: usize,
    /// Of those, the ones this preview judged: over, readable, not
    /// quarantined.
    pub judged: usize,
    /// Of the judged, the ones past the span, oldest first.
    pub expired: Vec<Seen>,
    /// What those weigh. `past.records` is `expired.len()`.
    pub past: Weight,
    /// Of the judged, the ones inside the span, and what they weigh.
    pub inside: Weight,
    /// The soonest a record of this class that is inside its span would
    /// expire, and which one. `None` when none is inside.
    pub next_to_expire: Option<(String, Timestamp, Duration)>,
}

impl ClassPreview {
    /// The oldest and the newest `updated_at` among the records past the
    /// span, or nothing when none is.
    pub fn oldest_and_newest(&self) -> Option<(Timestamp, Timestamp)> {
        let oldest = self.expired.first()?;
        let newest = self.expired.last()?;
        Some((oldest.ended_at, newest.ended_at))
    }
}

/// Whether the figures may be believed at all.
///
/// **Settlement 2, and the one place this tool refuses.** The two refusals
/// below are not symmetric with each other and neither is a threshold: both
/// are read off this root's own records against the spans the operator typed,
/// so there is no figure here for anyone to inherit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The clock agrees with the root and the pass would leave something
    /// behind. The figures are what retention would select.
    Readable,
    /// The clock is **behind** this root's own newest record, which this hub
    /// wrote with its own clock. So the clock has gone backwards since that
    /// write, and [`Timestamp::since`] would saturate and make every record
    /// read as younger than it is. Nothing here is wrong in the destructive
    /// direction — a young record is kept — but a pass run on a clock the
    /// records contradict is a pass whose figures mean nothing, so it does
    /// not run.
    ClockBehindTheRoot {
        newest: Timestamp,
        /// How far ahead of the clock that record is.
        ahead_by: Duration,
    },
    /// **Every** judgeable record is past its span, so the pass would empty
    /// this root of records.
    ///
    /// This is the forward-clock refusal, and it is the only shape that
    /// catches one. A clock stepped forward by a year is **not** detectable
    /// from a state root: a year-old newest record is what an idle hub and a
    /// broken clock both look like, and nothing on the disk tells them
    /// apart. What *is* detectable is the consequence they share — a pass
    /// that takes everything — and since nothing in Herd can undo the
    /// removal, that consequence is the thing to refuse. A hub that really
    /// has been idle longer than its shortest span is refused too, which
    /// costs it one explicit instruction to the engine and is the cheap side
    /// of this trade.
    WouldEmptyTheRoot {
        /// How many records would go: every one on this root that can
        /// expire at all. A class whose span is `0` is left out of this, and
        /// has to be: its records can never be emptied, so counting them
        /// would let one such class mask a clock that has stepped forward
        /// under every other class.
        expirable: usize,
        /// The newest record on the whole root, its class, and how far past
        /// its own span it reads. The clock's own witness: if even this one
        /// is expired, nothing on this root is not.
        newest: String,
        newest_class: Priority,
        newest_at: Timestamp,
        newest_past_by: Duration,
    },
}

impl Verdict {
    /// Whether the per-class figures may be printed. A refusal prints the
    /// clock, the spans and the census, and **not** a list of what would be
    /// removed: a refusal that still printed "everything is expired" would
    /// be the output it exists to prevent.
    pub fn is_readable(&self) -> bool {
        matches!(self, Verdict::Readable)
    }

    /// One line for an exit status, or nothing when the preview stands.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Verdict::Readable => None,
            Verdict::ClockBehindTheRoot { newest, ahead_by } => Some(format!(
                "this hub's clock reads earlier than its own newest record ({newest}, {} \
                 ahead): no retention pass may be judged against it, and nothing was \
                 previewed",
                said_span(*ahead_by),
            )),
            Verdict::WouldEmptyTheRoot {
                expirable,
                newest,
                newest_at,
                newest_past_by,
                ..
            } => Some(format!(
                "every one of the {expirable} record(s) that can expire on this root is \
                 past its span -- the newest ({newest}, {newest_at}) by {} -- so a pass \
                 would empty this root of records, and nothing was previewed",
                said_span(*newest_past_by),
            )),
        }
    }
}

/// What the output budget's sweep would take from this root, and in which
/// order (ORCH-097).
///
/// **The other question, from the same walk.** A span takes a whole job
/// directory; a sweep takes the bytes under `files/` and leaves the record,
/// its log and its manifest exactly where they are (D152) — so the manifest
/// outlives the bytes and a requester asking for an evicted collection is
/// handed a sentence rather than a shrug. The two populations are not
/// comparable and are never summed here.
///
/// **Class-blind on purpose, because the sweep is.** The output budget does
/// not look at a record's priority at all, so this is one list and not four:
/// a `producer` record pinned to a span of `0` — one that *never* expires —
/// can still be the first collection a sweep takes, and on a real root it
/// usually is, because it is the oldest. That is not a contradiction between
/// the two mechanisms; it is the difference between them, and seeing it is
/// the reason both are printed on one page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepPreview {
    /// What `files/` holds, over every job directory a sweep counts. **The
    /// figure a budget is compared against**, and the one `held_bytes` the
    /// store keeps a running total of.
    pub held: u64,
    /// How many job directories hold any of it.
    pub holders: usize,
    /// Every holder a sweep could take, in the order it would take them.
    pub order: Vec<Candidate>,
    /// Every holder it could not, with the reason. **A bound on the engine
    /// and not a footnote**: these bytes are inside `held` and no sweep can
    /// give them back, which is the shape of ORCH-124's known leak.
    pub withheld: Vec<Withheld>,
    /// `files.part/`: a collection still arriving. Its bytes are the
    /// writer's, are not in [`SweepPreview::held`], and are never a
    /// candidate under any key.
    pub arriving: u64,
    /// How many directories are mid-collection.
    pub arrivals: usize,
    /// `files.gone/`: what a hub killed mid-eviction left behind. A sweep
    /// removes these **before** it chooses anything, because they are
    /// already unreachable from `files/` and so are nobody's evidence — only
    /// disk to give back. Not in [`SweepPreview::held`] either.
    pub leftover: u64,
    /// How many such trees there are.
    pub leftovers: usize,
    /// Bytes under the `files/` of a directory in `jobs/` whose name is not
    /// a job id. A sweep ignores such a directory entirely — it belongs to
    /// whoever put it there, not to Herd — so these bytes are named here and
    /// counted nowhere else.
    pub not_a_job: u64,
    /// How many such directories hold bytes.
    pub not_jobs: usize,
}

impl SweepPreview {
    /// What a sweep could free if it took every candidate: the floor of what
    /// any budget can recover, and the ceiling of what this one can.
    pub fn freeable(&self) -> u64 {
        self.order.iter().map(|one| one.bytes).sum()
    }

    /// Held and unfreeable, whatever the budget.
    pub fn withheld_bytes(&self) -> u64 {
        self.withheld.iter().map(|one| one.bytes).sum()
    }
}

/// One collection a sweep would take, with the three keys it is ordered on
/// and the running total it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    /// What `files/` holds **now**. The sweeper measures this again under
    /// its own lock inside the removal, on purpose, so this is the figure a
    /// sweep would start from and not a promise about the byte.
    pub bytes: u64,
    pub files: usize,
    /// Whether the job succeeded. `store.rs`'s second key: *"a job that
    /// passed before a job that did not"*, because the artefacts of a green
    /// run are the ones nobody ever opens.
    pub passed: bool,
    /// Whether a requester has ever fetched these bytes. The third key, and
    /// the one where the obvious cache answer is backwards: an unfetched
    /// collection is evidence **nobody has seen**, so it goes last.
    pub fetched: bool,
    /// When the job ended. The tie-break, and the only one of the three that
    /// an LRU would have used first.
    pub ended_at: Timestamp,
    /// How old that is at the clock reading this preview was given.
    pub age: Duration,
    /// What is freed by the time this one is gone, this one included. **The
    /// column that answers the budget's question without being given a
    /// budget**: read down it until it reaches `held − budget`.
    pub freed_by_then: u64,
}

/// One collection whose bytes a sweep counts and cannot free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    pub id: String,
    pub bytes: u64,
    pub why: WhyWithheld,
}

/// Why a sweep would leave bytes it has counted.
///
/// The four `continue`s of `store.rs`'s sweep, in its own order, each one
/// adding the bytes to `could_not_free` instead of taking them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhyWithheld {
    /// The record would not read, or this hub will not act on it: another
    /// layout's version, labels today's rules reject (D52), a `job.json`
    /// that is not there. **Not a licence to delete the files it
    /// describes.**
    NoReadableRecord(String),
    /// The job is not over. Its bytes are an attempt's, not a record's.
    NotOver,
    /// A sweep has already taken these bytes and the record says so, which
    /// is the one case where `files/` holding bytes is a leftover rather
    /// than a collection.
    AlreadyRemoved,
    /// `files/` does not hold what the record's manifest names, at the
    /// lengths it names: a collection that is not whole is one the hub is
    /// either still owed or has lost, and either way the bytes are not a
    /// sweep's to count as recoverable.
    NotWhole,
}

impl WhyWithheld {
    /// The sentence this reason is printed as.
    pub fn said(&self) -> String {
        match self {
            WhyWithheld::NoReadableRecord(why) => format!("no record this hub will act on: {why}"),
            WhyWithheld::NotOver => {
                "the job is not over, so these bytes are an attempt's and not a record's".to_owned()
            }
            WhyWithheld::AlreadyRemoved => {
                "the record says a sweep has already taken this collection".to_owned()
            }
            WhyWithheld::NotWhole => {
                "files/ does not hold what the manifest names, at the lengths it names".to_owned()
            }
        }
    }
}

/// What retention would remove from one state root, and what it would cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPreview {
    pub root: PathBuf,
    /// The clock reading every figure here was judged against. **A field and
    /// not a call to the clock inside [`fmt::Display`]**, so the rendered
    /// text is a function of its inputs and a test can assert it line for
    /// line — and so that the one number an operator must be able to check
    /// is printed rather than implied.
    pub now: Timestamp,
    pub spans: Spans,
    /// Said rather than swallowed when `jobs/` itself could not be read: a
    /// preview of nought records on a root holding thousands would be the
    /// worst output this tool could produce.
    pub jobs_dir_error: Option<String>,
    pub scanned: Scanned,
    /// The newest `updated_at` on the whole root, with the job it belongs
    /// to: the clock's own witness. `None` on a root with no readable
    /// record.
    pub newest: Option<(String, Priority, Timestamp)>,
    pub verdict: Verdict,
    /// One entry per class, in [`Priority::ALL`] order.
    pub classes: BTreeMap<u8, ClassPreview>,
    /// The output budget's question, from the same walk (ORCH-097).
    ///
    /// **Printed under a refusal too**, unlike the per-class figures. The two
    /// refusals in [`Verdict`] are about the clock and the spans, and the
    /// sweep's order is read off the records alone: the only clock-derived
    /// figure in it is [`Candidate::age`], and the block says so where a
    /// refusal has been printed above it. A clock an operator cannot trust is
    /// exactly the moment they still need to know what their disk is holding.
    pub sweep: SweepPreview,
}

/// Enough of a `job.json` to judge it by, and nothing else.
///
/// **Deliberately not [`JobRecord`]**, for [`crate::report`]'s reason: a
/// record carrying a field this build does not know, or labels today's rules
/// reject, must not take a *preview* down — the whole point is to be runnable
/// on a live root whatever is in it. The three fields are required and not
/// `#[serde(default)]`: a record with no priority or no `updated_at` is a
/// record this build cannot judge, and it belongs in the census, never in a
/// figure.
#[derive(Debug, Deserialize)]
struct Judgeable {
    priority: Priority,
    state: JobState,
    updated_at: Timestamp,
}

/// Enough of a `job.json` to judge its **collected bytes** by: what a sweep
/// asks of a record and nothing else.
///
/// **Parsed separately from [`Judgeable`], out of the same text, and that is
/// deliberate.** Every field here is `#[serde(default)]` through
/// [`OutputSummary`]'s own attributes, so a record with no `outputs` key at
/// all — which is every job that kept nothing — reads as a job that holds no
/// collection rather than as a record this build cannot read. Folding these
/// fields into `Judgeable` would have made a record whose `outputs` this
/// build cannot parse vanish from the **span** figures too, which is a figure
/// nobody asked to change. Two parses of one string, one read of the disk.
#[derive(Debug, Deserialize)]
struct Sweepable {
    state: JobState,
    #[serde(default)]
    outputs: OutputSummary,
}

/// A record's labels as the disk carries them, before today's rules judge
/// them — the one thing that decides whether a record is quarantined (D52,
/// `store.rs`'s `DurableLabels`).
#[derive(Debug, Deserialize)]
struct DurableLabels {
    #[serde(default)]
    labels: Vec<String>,
}

/// Just the version, read before anything else, so a record from another
/// layout is named by its version rather than by whichever field happened to
/// fail first.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    /// `None` when the key is absent, never `0`. The same line
    /// [`crate::report`]'s `RecordOutputs` draws, for the same reason: a
    /// `u32` with a serde default reads a missing field as zero and the
    /// preview then says *"it carries record version 0"* about a record that
    /// carries no version at all.
    #[serde(default)]
    record_version: Option<u32>,
}

impl RetentionPreview {
    /// Walk a state root and answer. Takes no lock, writes nothing, removes
    /// nothing, reads no output byte and no log byte.
    ///
    /// `now` is given rather than read here so that the whole of this value
    /// is a function of its inputs; the subcommand passes
    /// [`Timestamp::now`], and that reading is printed.
    ///
    /// The only error is a root that is not a readable directory: everything
    /// after that is reported rather than raised, because a preview that
    /// refuses on one bad record is a preview nobody can run on the root
    /// that has one.
    pub fn read(root: &Path, spans: Spans, now: Timestamp) -> io::Result<Self> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a directory", root.display()),
            ));
        }
        let mut preview = Self {
            root: root.to_path_buf(),
            now,
            spans,
            jobs_dir_error: None,
            scanned: Scanned::default(),
            newest: None,
            verdict: Verdict::Readable,
            classes: BTreeMap::new(),
            sweep: SweepPreview::default(),
        };
        for class in Priority::ALL {
            preview
                .classes
                .insert(class.rank(), ClassPreview::default());
        }

        // Sorted, so that every list this preview prints is in one order
        // whatever the directory hands back. `read_dir` has none, and these
        // lists are ids an operator pastes into a command.
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
                preview.jobs_dir_error = Some(format!("{}: {error}", jobs_dir.display()));
                Vec::new()
            }
        };
        names.sort();
        preview.scanned.directories = names.len();

        let mut inside_soonest: BTreeMap<u8, (String, Timestamp, Duration)> = BTreeMap::new();

        for name in &names {
            let dir = jobs_dir.join(name);
            // **Weighed once, for both questions.** One `read_dir` of this
            // job directory answers the span's three populations and the
            // budget's one, and it is taken before the record is read so
            // that a directory whose record cannot be read is still counted
            // against the budget — which is what the sweep does, and is the
            // half ORCH-124's leak lives in.
            let weighed = weigh(&dir);
            let holding = sweep_counts(&mut preview.sweep, name, &weighed);
            let record_path = dir.join(RECORD_FILE);
            let text = match fs::read_to_string(&record_path) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    preview.scanned.without_record += 1;
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(format!(
                            "there is no {RECORD_FILE} here at all"
                        )),
                    );
                    continue;
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it could not be read: {error}"),
                    });
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(format!("it could not be read: {error}")),
                    );
                    continue;
                }
            };
            // The version first, exactly as `read_stored` does it: a record
            // from another layout is named by its version and not by the
            // field that happened to fail.
            match serde_json::from_str::<VersionProbe>(&text) {
                Ok(probe) if probe.record_version == Some(RECORD_VERSION) => {}
                Ok(probe) => {
                    let why = match probe.record_version {
                        Some(version) => format!(
                            "it carries record version {version}, and this build \
                             reads version {RECORD_VERSION}"
                        ),
                        None => format!(
                            "it carries no record version at all, and this build \
                             reads version {RECORD_VERSION}"
                        ),
                    };
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: why.clone(),
                    });
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(why),
                    );
                    continue;
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it is not JSON this build can parse: {error}"),
                    });
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(format!(
                            "it is not JSON this build can parse: {error}"
                        )),
                    );
                    continue;
                }
            }
            // Then the labels, because that is what quarantines a record
            // (D52) and a quarantined record must never reach a figure.
            match serde_json::from_str::<DurableLabels>(&text) {
                Ok(durable) => {
                    if let Err(why) = LabelSet::new(durable.labels.clone()) {
                        let said = format!(
                            "its labels {:?} are not labels this hub accepts: {why}",
                            durable.labels
                        );
                        preview
                            .scanned
                            .quarantined
                            .push((name.clone(), said.clone()));
                        // Quarantined is `could_not_free` in the sweep too:
                        // `read_record` answers an error for one, so the
                        // bytes are counted and never taken (D52).
                        withhold(
                            &mut preview.sweep,
                            name,
                            holding,
                            WhyWithheld::NoReadableRecord(said),
                        );
                        continue;
                    }
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("its labels are not a list of names: {error}"),
                    });
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(format!(
                            "its labels are not a list of names: {error}"
                        )),
                    );
                    continue;
                }
            }
            let parsed: Judgeable = match serde_json::from_str(&text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("this build cannot judge it: {error}"),
                    });
                    withhold(
                        &mut preview.sweep,
                        name,
                        holding,
                        WhyWithheld::NoReadableRecord(format!(
                            "this build cannot judge it: {error}"
                        )),
                    );
                    continue;
                }
            };
            preview.scanned.read += 1;

            // The budget's question, settled here and not in the match
            // below: a sweep does not look at a record's class or its span,
            // so the answer is the same whichever side of a span the record
            // falls on, and it is wanted even for a record that is not over.
            if let Some(bytes) = holding {
                match sweep_judge(&dir, &text) {
                    Ok((passed, fetched)) => preview.sweep.order.push(Candidate {
                        id: name.clone(),
                        bytes,
                        files: weighed.kept_files,
                        passed,
                        fetched,
                        ended_at: parsed.updated_at,
                        age: now.since(parsed.updated_at),
                        // Filled once the order is known, below: a running
                        // total has no value until there is an order to run
                        // along.
                        freed_by_then: 0,
                    }),
                    Err(why) => withhold(&mut preview.sweep, name, holding, why),
                }
            }

            // The clock's witness, over every readable record and not only
            // the judgeable ones: a record written in the future is evidence
            // about the clock whether or not its job is over.
            match &preview.newest {
                Some((_, _, at)) if *at >= parsed.updated_at => {}
                _ => {
                    preview.newest = Some((name.clone(), parsed.priority, parsed.updated_at));
                }
            }

            let rank = parsed.priority.rank();
            let class = preview
                .classes
                .get_mut(&rank)
                .expect("every rank has an entry");
            class.records += 1;
            let judged = Judged {
                class: parsed.priority,
                terminal: parsed.state.is_terminal(),
                ended_at: parsed.updated_at,
            };
            match select(judged, &spans, now) {
                Selection::NotOurs(_) => {
                    class.not_over += 1;
                    continue;
                }
                Selection::NeverExpires => {
                    // Counted as judged and as inside: the class was looked
                    // at, and nothing of it is ever selected. Leaving it out
                    // of `judged` would make the "would empty the root" test
                    // below read a class that can never be emptied as one
                    // that was.
                    class.judged += 1;
                    class.inside.add(name, &weighed);
                }
                Selection::Inside { left } => {
                    class.judged += 1;
                    class.inside.add(name, &weighed);
                    let sooner = match inside_soonest.get(&rank) {
                        Some((_, _, known)) => left < *known,
                        None => true,
                    };
                    if sooner {
                        inside_soonest.insert(rank, (name.clone(), parsed.updated_at, left));
                    }
                }
                Selection::Expired { .. } => {
                    class.judged += 1;
                    class.past.add(name, &weighed);
                    class.expired.push(Seen {
                        id: name.clone(),
                        class: parsed.priority,
                        ended_at: parsed.updated_at,
                        weighed,
                    });
                }
            }
        }

        for (rank, soonest) in inside_soonest {
            if let Some(class) = preview.classes.get_mut(&rank) {
                class.next_to_expire = Some(soonest);
            }
        }
        // Oldest first, and by the end time rather than by the id: a span is
        // about when a job ended, and id order is only an approximation of
        // that (a long job ends after a later short one).
        for class in preview.classes.values_mut() {
            class
                .expired
                .sort_by(|a, b| (a.ended_at, &a.id).cmp(&(b.ended_at, &b.id)));
        }
        // The sweep's own order, on the three keys a record carries
        // (`store.rs`'s 2, 3 and 4), with the id as a fourth so that this
        // listing is the same listing twice over. **The floor is the key
        // that is not here**: it is `--output-floor`, the serving hub's
        // argument, and nothing on this disk records it — so the order
        // below is the sweep's within one floor band, and a floor of F
        // stably moves every candidate younger than F to the end, keeping
        // this order among themselves. The block says so where it prints.
        preview.sweep.order.sort_by(|a, b| {
            (!a.passed, !a.fetched, a.ended_at, &a.id)
                .cmp(&(!b.passed, !b.fetched, b.ended_at, &b.id))
        });
        let mut freed = 0u64;
        for candidate in &mut preview.sweep.order {
            freed = freed.saturating_add(candidate.bytes);
            candidate.freed_by_then = freed;
        }
        preview.verdict = preview.judge_the_clock();
        Ok(preview)
    }

    /// Every record past its span, over every class.
    pub fn expired(&self) -> usize {
        self.classes.values().map(|class| class.expired.len()).sum()
    }

    /// Every record this preview judged, over every class.
    pub fn judged(&self) -> usize {
        self.classes.values().map(|class| class.judged).sum()
    }

    /// Of those, the ones that can expire at all: a class whose span is `0`
    /// contributes none.
    ///
    /// **The population "would this empty the root?" is asked of**, and the
    /// reason it is not [`RetentionPreview::judged`]: one class pinned to
    /// `0` would otherwise be enough to hide a clock that had stepped
    /// forward under all three others.
    pub fn expirable(&self) -> usize {
        Priority::ALL
            .iter()
            .filter(|class| !self.spans.never_expires(**class))
            .filter_map(|class| self.classes.get(&class.rank()))
            .map(|figures| figures.judged)
            .sum()
    }

    /// What removing every expired record would free, over every class and
    /// split into the three populations.
    pub fn past(&self) -> Weight {
        let mut total = Weight::default();
        for class in self.classes.values() {
            total.absorb(&class.past);
        }
        total
    }

    /// And what the records inside their spans weigh, the same way. Printed
    /// beside the other side on purpose: under the direction this figure is
    /// wanted for, a span is a guard rather than a trigger, so what is
    /// *kept* is as much a sizing figure as what would go.
    pub fn inside(&self) -> Weight {
        let mut total = Weight::default();
        for class in self.classes.values() {
            total.absorb(&class.inside);
        }
        total
    }

    /// The two refusals, in the order that matters: a clock the records
    /// contradict makes every figure meaningless, so it is tested first.
    fn judge_the_clock(&self) -> Verdict {
        let Some((newest_id, newest_class, newest_at)) = &self.newest else {
            return Verdict::Readable;
        };
        if *newest_at > self.now {
            return Verdict::ClockBehindTheRoot {
                newest: *newest_at,
                ahead_by: newest_at.since(self.now),
            };
        }
        let expirable = self.expirable();
        if expirable > 0 && self.expired() == expirable {
            // The newest record on the root is the clock's witness, and by
            // the test above it is expired like all the rest. Its own figure
            // is what the refusal names: a reader has to be able to see that
            // even the most recent thing this hub did is past its span.
            let past_by = match select(
                Judged {
                    class: *newest_class,
                    terminal: true,
                    ended_at: *newest_at,
                },
                &self.spans,
                self.now,
            ) {
                Selection::Expired { past_by } => past_by,
                // The newest record may be one that is not over, or of a
                // class that never expires, while every *judgeable* record
                // is expired. Then the honest figure is its age, which is
                // what the sentence is about.
                _ => self.now.since(*newest_at),
            };
            return Verdict::WouldEmptyTheRoot {
                expirable,
                newest: newest_id.clone(),
                newest_class: *newest_class,
                newest_at: *newest_at,
                newest_past_by: past_by,
            };
        }
        Verdict::Readable
    }
}

/// The log and the hub's own counter for it, by the names a job directory
/// carries them under.
///
/// **Written again here rather than imported.** They are private to
/// [`crate::store`], and `store.rs` is another task's exclusively, so this
/// module may not widen them. The duplication is held by a test rather than
/// by a comment: `the_preview_reads_what_the_store_itself_wrote` appends a
/// real log line through the store's own writer and asserts the figure, so a
/// rename in `store.rs` fails there instead of printing nought for every log
/// on an operator's root.
const LOG_FILE: &str = "log";
const LOG_SEQ_FILE: &str = "log.seq";

/// What a job directory weighs, in the three populations retention removes
/// in one act.
///
/// Measured **positively**, population by population, and not by subtracting
/// one from a total: a figure the log could not supply must leave the log's
/// place empty rather than silently move its bytes into the record's.
///
/// By `metadata().len()` and never by contents, and by `directory_bytes` for
/// the subdirectories: that is the sweeper's own walk, used on purpose, so a
/// figure this preview prints and a figure the sweeper prints about the same
/// bytes are the same figure.
///
/// The log is the one file measured or named: it is the population nothing
/// bounds, and the only one where a missing figure changes what an operator
/// would decide. Every other length falls back to nought the way
/// `directory_bytes` does, because an unreadable `job.json` is already
/// counted and named by the walk above.
fn weigh(dir: &Path) -> Weighed {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            return Weighed {
                collected_bytes: 0,
                collected_files: 0,
                kept_bytes: 0,
                kept_files: 0,
                arriving_bytes: 0,
                leftover_bytes: 0,
                leftover: false,
                record_bytes: 0,
                log: LogWeight::Unknown(format!(
                    "the job directory itself could not be read, so none of its three \
                     populations was measured: {error}"
                )),
            }
        }
    };
    let mut collected_bytes = 0u64;
    let mut collected_files = 0usize;
    let mut kept_bytes = 0u64;
    let mut kept_files = 0usize;
    let mut arriving_bytes = 0u64;
    let mut leftover_bytes = 0u64;
    let mut leftover = false;
    let mut record_bytes = 0u64;
    let mut log: Option<LogWeight> = None;
    let mut counter_is_there = false;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            let path = entry.path();
            if [KEPT_FILES_DIR, OUTPUT_PARTS_DIR, OUTPUT_GONE_DIR].contains(&name.as_str()) {
                let bytes = directory_bytes(&path);
                let files = files_in(&path);
                collected_bytes = collected_bytes.saturating_add(bytes);
                collected_files += files;
                // The same measurement, kept apart by which mechanism owns
                // it: the budget's sweep counts `files/`, never the part
                // file it is still being given, and gives a `files.gone/`
                // back before it chooses anything.
                match name.as_str() {
                    KEPT_FILES_DIR => {
                        kept_bytes = kept_bytes.saturating_add(bytes);
                        kept_files += files;
                    }
                    OUTPUT_PARTS_DIR => {
                        arriving_bytes = arriving_bytes.saturating_add(bytes);
                    }
                    _ => {
                        leftover_bytes = leftover_bytes.saturating_add(bytes);
                        leftover = true;
                    }
                }
            } else {
                record_bytes = record_bytes.saturating_add(directory_bytes(&path));
            }
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        if name == LOG_FILE {
            log = Some(match entry.metadata() {
                Ok(at) => LogWeight::Measured(at.len()),
                Err(error) => LogWeight::Unknown(format!(
                    "{LOG_FILE} is there and its length could not be read: {error}"
                )),
            });
            continue;
        }
        if name == LOG_SEQ_FILE {
            counter_is_there = true;
        }
        record_bytes =
            record_bytes.saturating_add(entry.metadata().map(|at| at.len()).unwrap_or(0));
    }
    let log = match log {
        Some(log) => log,
        // The counter is this hub's own record that a log was being
        // written, so a log missing from beside it is a figure that is
        // unknown rather than nought. Without the counter the job never
        // reached an attempt, and then nought is a measurement.
        None if counter_is_there => LogWeight::Unknown(format!(
            "there is no {LOG_FILE}, and the {LOG_SEQ_FILE} beside it is this hub's own \
             record that a log was being counted, so its bytes are unknown and not nought"
        )),
        None => LogWeight::NeverWritten,
    };
    Weighed {
        collected_bytes,
        collected_files,
        kept_bytes,
        kept_files,
        arriving_bytes,
        leftover_bytes,
        leftover,
        record_bytes,
        log,
    }
}

/// How wide a job id is, after its `j`.
///
/// **`store.rs`'s `ID_DIGITS` written again here**, like `LOG_FILE` above and
/// for the same reason: it is private there and `store.rs` is another task's
/// exclusively. The duplication is held by a figure rather than by a comment
/// — `a_directory_in_jobs_that_is_not_a_job_holds_bytes_a_sweep_ignores`
/// mints a real job through the store itself and asserts that its id lands
/// among the candidates while a directory beside it does not.
const ID_DIGITS: usize = 10;

/// Whether a name in `jobs/` is a job id this hub minted.
///
/// A sweep asks this first and skips everything else: a directory somebody
/// else put in `jobs/` belongs to whoever put it there, so its bytes are
/// neither held nor freeable. Said out loud on its own line rather than
/// quietly folded into the total.
fn looks_like_a_job_id(name: &str) -> bool {
    let Some(digits) = name.strip_prefix('j') else {
        return false;
    };
    digits.len() == ID_DIGITS && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Count one job directory into the sweep's figures, and answer with what
/// `files/` holds if a sweep would count it against the budget at all.
///
/// The order of the three tests is `store.rs`'s own: the name first, then the
/// `files.gone/` a killed sweep left — which is given back before anything is
/// chosen — and only then the bytes. A directory whose `files/` is there and
/// empty is not a holder, exactly as it is not one there.
fn sweep_counts(sweep: &mut SweepPreview, name: &str, weighed: &Weighed) -> Option<u64> {
    if !looks_like_a_job_id(name) {
        if weighed.kept_bytes > 0 {
            sweep.not_a_job = sweep.not_a_job.saturating_add(weighed.kept_bytes);
            sweep.not_jobs += 1;
        }
        return None;
    }
    if weighed.arriving_bytes > 0 {
        sweep.arriving = sweep.arriving.saturating_add(weighed.arriving_bytes);
        sweep.arrivals += 1;
    }
    if weighed.leftover {
        sweep.leftover = sweep.leftover.saturating_add(weighed.leftover_bytes);
        sweep.leftovers += 1;
    }
    if weighed.kept_bytes == 0 {
        return None;
    }
    sweep.held = sweep.held.saturating_add(weighed.kept_bytes);
    sweep.holders += 1;
    Some(weighed.kept_bytes)
}

/// Record bytes a sweep counts and cannot free, with the reason.
///
/// Does nothing for a directory holding nothing: a reason is only worth
/// printing about bytes.
fn withhold(sweep: &mut SweepPreview, name: &str, holding: Option<u64>, why: WhyWithheld) {
    if let Some(bytes) = holding {
        sweep.withheld.push(Withheld {
            id: name.to_owned(),
            bytes,
            why,
        });
    }
}

/// What a sweep would make of one holder: its two record-borne sort keys, or
/// the reason its bytes are not a sweep's to free.
///
/// The four refusals are `store.rs`'s four `continue`s, in its order, and
/// each one puts the bytes into `could_not_free` there and into
/// [`SweepPreview::withheld`] here.
fn sweep_judge(dir: &Path, text: &str) -> Result<(bool, bool), WhyWithheld> {
    let parsed: Sweepable = match serde_json::from_str(text) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(WhyWithheld::NoReadableRecord(format!(
                "this build cannot read what it kept: {error}"
            )))
        }
    };
    let JobState::Finished { outcome } = &parsed.state else {
        return Err(WhyWithheld::NotOver);
    };
    if parsed.outputs.removed.is_some() {
        return Err(WhyWithheld::AlreadyRemoved);
    }
    if !collection_is_whole(dir, &parsed.outputs) {
        return Err(WhyWithheld::NotWhole);
    }
    Ok((
        outcome.is_success(),
        parsed.outputs.first_fetched_at.is_some(),
    ))
}

/// `store.rs`'s `collection_is_whole`, over the manifest this module parsed:
/// in `files/`, at the manifest's length, by a path this hub would join onto
/// its own state root.
///
/// **Not the digest**, which `store.rs` does not check here either: the
/// question is whether the sweep would find a collection to take, and it
/// asks exactly this.
fn collection_is_whole(dir: &Path, outputs: &OutputSummary) -> bool {
    let kept = dir.join(KEPT_FILES_DIR);
    outputs.files.iter().all(|file| {
        herd_proto::check_relative(&file.path).is_ok()
            && fs::metadata(kept.join(&file.path))
                .ok()
                .filter(|at| at.is_file())
                .map(|at| at.len())
                == Some(file.bytes)
    })
}

/// How many files a directory holds, at every depth.
///
/// Beside `directory_bytes`'s sum and in the same shape, because a byte
/// figure with no count behind it cannot be checked by hand — and because
/// "names no collected file at all" is the first consumer's common case and
/// a count is the only way to see it.
fn files_in(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut files = 0;
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => files += files_in(&entry.path()),
            Ok(kind) if kind.is_file() => files += 1,
            _ => {}
        }
    }
    files
}

/// A span or an age, said in days **and** in seconds.
///
/// `in_days` alone reads "1 day(s)" for thirty-six hours, which is the one
/// figure in a retention sentence a reader cannot check against the span they
/// typed. Exactness first and never instead, as with the bytes.
fn said_span(how_long: Duration) -> String {
    format!("{} ({} s)", in_days(how_long), how_long.as_secs())
}

/// A class's span, or the words a span of `0` means.
fn said_span_setting(spans: &Spans, class: Priority) -> String {
    if spans.never_expires(class) {
        return "0: these records NEVER EXPIRE".to_owned();
    }
    said_span(spans.of(class))
}

/// A span as a command line gives one: a figure and a unit, `14d`, `36h`,
/// `90m`, `30s` — or a bare `0`, which means never.
///
/// **The unit is required**, which is the whole point. Every other duration
/// this binary takes is a bare number of seconds, and a retention span typed
/// in that shape is the mistake that cannot be recovered from: `14` meaning
/// fourteen seconds would select every record on the root. A required suffix
/// makes that typo unrepresentable rather than merely unlikely.
///
/// `0` is accepted bare because it has no unit: nought seconds and nought
/// days are the same nought, and here it means never (see
/// [`Spans::never_expires`]).
pub fn parse_span(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    if text == "0" {
        return Ok(Duration::ZERO);
    }
    let (figure, unit) = text.split_at(
        text.find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| format!("{text:?} carries no unit: write 14d, 36h, 90m or 30s"))?,
    );
    if figure.is_empty() {
        return Err(format!(
            "{text:?} begins with no figure: write 14d, 36h, 90m or 30s"
        ));
    }
    let figure: u64 = figure
        .parse()
        .map_err(|_| format!("{figure:?} is not a whole number of {unit}"))?;
    let seconds = match unit {
        "d" => 86_400,
        "h" => 3_600,
        "m" => 60,
        "s" => 1,
        other => {
            return Err(format!(
                "{other:?} is not a unit this reads: write d, h, m or s (or a bare 0, \
                 which means never)"
            ))
        }
    };
    figure
        .checked_mul(seconds)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("{text:?} is more time than this can hold"))
}

impl fmt::Display for RetentionPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "herd-hub retention preview")?;
        writeln!(f, "  state root  {}", self.root.display())?;
        writeln!(f, "  clock read  {}", self.now)?;
        match &self.newest {
            Some((id, class, at)) => writeln!(
                f,
                "  newest      {JOBS_DIR}/{id}, a {}, last changed {at}",
                class.name()
            )?,
            None => writeln!(f, "  newest      no readable record on this root")?,
        }
        writeln!(f)?;
        writeln!(
            f,
            "  IT REMOVES NOTHING. This subcommand has no deletion path and no flag that"
        )?;
        writeln!(
            f,
            "  could be given one: it reads records and file lengths and exits. It takes"
        )?;
        writeln!(
            f,
            "  no lock of any kind, so a hub may be writing this root while it is read --"
        )?;
        writeln!(
            f,
            "  each record is read whole or not at all, but the figures together are not"
        )?;
        writeln!(
            f,
            "  one instant. The clock reading above is the one every figure below was"
        )?;
        writeln!(f, "  judged against.")?;
        writeln!(f)?;

        writeln!(f, "spans, as this invocation was given them")?;
        for class in Priority::ALL {
            writeln!(
                f,
                "  {:<9} {}",
                class.name(),
                said_span_setting(&self.spans, class)
            )?;
        }
        writeln!(
            f,
            "    Every one of these is this invocation's and nothing else's: no span is"
        )?;
        writeln!(
            f,
            "    built into this binary and none is defaulted (D194). A span of 0 means"
        )?;
        writeln!(
            f,
            "    that class NEVER EXPIRES -- not \"remove it now\" -- which is the"
        )?;
        writeln!(
            f,
            "    opposite of what --output-floor 0 means, because the floor's zero loses"
        )?;
        writeln!(f, "    bytes and this one would lose the records.")?;
        writeln!(f)?;

        writeln!(f, "records")?;
        if let Some(error) = &self.jobs_dir_error {
            writeln!(
                f,
                "  THE JOBS DIRECTORY COULD NOT BE READ, so every figure below is about"
            )?;
            writeln!(f, "  no records at all and about nothing else: {error}")?;
        }
        writeln!(
            f,
            "  job directories                    {}",
            self.scanned.directories
        )?;
        writeln!(
            f,
            "  records read                       {}",
            self.scanned.read
        )?;
        writeln!(
            f,
            "  directories with no record yet     {}",
            self.scanned.without_record
        )?;
        writeln!(
            f,
            "  quarantined, never expired         {}",
            self.scanned.quarantined.len()
        )?;
        for (id, why) in self.scanned.quarantined.iter().take(NAMED_AT_MOST) {
            writeln!(f, "    {JOBS_DIR}/{id}: {why}")?;
        }
        and_more(f, self.scanned.quarantined.len(), "    ")?;
        writeln!(
            f,
            "  records this build could not read  {}",
            self.scanned.unread.len()
        )?;
        for unread in self.scanned.unread.iter().take(NAMED_AT_MOST) {
            writeln!(f, "    {}: {}", unread.path, unread.why)?;
        }
        and_more(f, self.scanned.unread.len(), "    ")?;
        writeln!(
            f,
            "    A record this build cannot read has no readable end time, so it can"
        )?;
        writeln!(
            f,
            "    never be shown to be past its span -- and therefore retention may never"
        )?;
        writeln!(f, "    remove it. This list is a bound on the engine.")?;
        writeln!(f)?;

        match &self.verdict {
            Verdict::Readable => self.write_classes(f)?,
            Verdict::ClockBehindTheRoot { newest, ahead_by } => {
                writeln!(f, "REFUSED: THE CLOCK IS BEHIND THIS ROOT'S OWN RECORDS")?;
                writeln!(f)?;
                writeln!(f, "  clock read                 {}", self.now)?;
                writeln!(f, "  newest record last changed {newest}")?;
                writeln!(f, "  that record is ahead by    {}", said_span(*ahead_by))?;
                writeln!(f)?;
                writeln!(
                    f,
                    "  This hub wrote that record with its own clock, so the clock has gone"
                )?;
                writeln!(
                    f,
                    "  backwards since. An age measured against it saturates at nought, which"
                )?;
                writeln!(
                    f,
                    "  makes every record read as younger than it is -- safe, and still"
                )?;
                writeln!(
                    f,
                    "  unusable: a pass judged on a clock the records contradict proves"
                )?;
                writeln!(
                    f,
                    "  nothing. No per-class figure is printed and nothing was previewed."
                )?;
            }
            Verdict::WouldEmptyTheRoot {
                expirable,
                newest,
                newest_class,
                newest_at,
                newest_past_by,
            } => {
                writeln!(f, "REFUSED: A PASS WOULD EMPTY THIS ROOT OF RECORDS")?;
                writeln!(f)?;
                writeln!(f, "  records that can expire    {expirable}")?;
                writeln!(f, "  of those, past their span  {expirable} -- every one")?;
                writeln!(
                    f,
                    "  the newest record          {JOBS_DIR}/{newest}, a {}, last changed \
                     {newest_at}",
                    newest_class.name()
                )?;
                writeln!(
                    f,
                    "  it is past its span by     {}",
                    said_span(*newest_past_by)
                )?;
                writeln!(f, "  clock read                 {}", self.now)?;
                writeln!(f)?;
                writeln!(
                    f,
                    "  Even the most recent thing this hub recorded is past its span, so a"
                )?;
                writeln!(
                    f,
                    "  pass would take every record there is. Two different things look"
                )?;
                writeln!(
                    f,
                    "  exactly like this from the disk alone -- a clock that stepped forward,"
                )?;
                writeln!(
                    f,
                    "  and a hub idle for longer than its shortest span -- and nothing here"
                )?;
                writeln!(
                    f,
                    "  can tell them apart. Nothing in Herd can undo the removal either, so"
                )?;
                writeln!(
                    f,
                    "  this refuses instead of guessing. Check the clock reading above"
                )?;
                writeln!(
                    f,
                    "  against the date, then check the spans: a span typed in the wrong"
                )?;
                writeln!(
                    f,
                    "  unit lands here too. No per-class figure is printed and nothing was"
                )?;
                writeln!(f, "  previewed.")?;
            }
        }
        writeln!(f)?;
        self.write_sweep(f)
    }
}

impl RetentionPreview {
    fn write_classes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "what retention would remove, by class")?;
        writeln!(f)?;
        writeln!(
            f,
            "  THREE POPULATIONS, THREE OWNERS, kept apart below because sizing a policy"
        )?;
        writeln!(
            f,
            "  against their sum is sizing it against nothing. The collected files are the"
        )?;
        writeln!(
            f,
            "  output budget's (D148, D152): a sweep frees them and leaves the record. The"
        )?;
        writeln!(
            f,
            "  record and its log are nobody's -- the only thing in Herd that frees one of"
        )?;
        writeln!(
            f,
            "  their bytes is removing the record -- and nothing on this hub bounds a log"
        )?;
        writeln!(
            f,
            "  at all: the only cap is the runner's --job-log-cap, 64 MiB a job by default,"
        )?;
        writeln!(
            f,
            "  on the runner's own machine. A record is not kilobytes because its job.json"
        )?;
        writeln!(
            f,
            "  is. A log this build could not measure is printed UNKNOWN and named, never"
        )?;
        writeln!(f, "  as nought.")?;
        writeln!(f)?;
        for class in Priority::ALL {
            let Some(figures) = self.classes.get(&class.rank()) else {
                continue;
            };
            writeln!(f, "  {}", class.name())?;
            writeln!(
                f,
                "    span                     {}",
                said_span_setting(&self.spans, class)
            )?;
            writeln!(f, "    records of this class    {}", figures.records)?;
            writeln!(
                f,
                "      not over, so not ours  {} (the reaper's, ORCH-012 and D134)",
                figures.not_over
            )?;
            writeln!(f, "      judged                 {}", figures.judged)?;
            writeln!(
                f,
                "    past the span            {} record(s)",
                figures.expired.len()
            )?;
            write_weight(f, &figures.past, "      ")?;
            match figures.oldest_and_newest() {
                Some((oldest, newest)) => {
                    writeln!(f, "      oldest                 {oldest}")?;
                    writeln!(f, "      newest                 {newest}")?;
                }
                None => {
                    writeln!(
                        f,
                        "      oldest                 none: no record of this class is past \
                         its span"
                    )?;
                    writeln!(f, "      newest                 none")?;
                }
            }
            for seen in figures.expired.iter().take(NAMED_AT_MOST) {
                writeln!(
                    f,
                    "      {JOBS_DIR}/{}  {}  ended {}",
                    seen.id,
                    said_three(&seen.weighed),
                    seen.ended_at
                )?;
            }
            and_more(f, figures.expired.len(), "      ")?;
            writeln!(
                f,
                "    inside the span          {} record(s)",
                figures.inside.records
            )?;
            write_weight(f, &figures.inside, "      ")?;
            match &figures.next_to_expire {
                Some((id, at, left)) => writeln!(
                    f,
                    "      next to expire         {JOBS_DIR}/{id}, ended {at}, {} left",
                    said_span(*left)
                )?,
                None if self.spans.never_expires(class) => writeln!(
                    f,
                    "      next to expire         never: this class's span is 0"
                )?,
                None => writeln!(
                    f,
                    "      next to expire         none: no record of this class is inside \
                     its span"
                )?,
            }
            writeln!(f)?;
        }
        writeln!(
            f,
            "  in all                   {} of the {} record(s) that can expire are past \
             their span",
            self.expired(),
            self.expirable()
        )?;
        writeln!(
            f,
            "    records judged         {} (a class whose span is 0 is judged and never \
             expires)",
            self.judged()
        )?;
        writeln!(f, "  what that would free, over the whole root")?;
        write_weight(f, &self.past(), "    ")?;
        writeln!(f, "  what it would keep, over the whole root")?;
        write_weight(f, &self.inside(), "    ")?;
        writeln!(
            f,
            "  Nothing above was removed, and this subcommand cannot remove it."
        )?;
        Ok(())
    }

    /// The output budget's question, printed beside the span's (ORCH-097).
    ///
    /// **Printed whatever the verdict**, because the order in it is read off
    /// the records and not off the clock: a refusal above is about the spans,
    /// and an operator whose clock is wrong still has a disk filling up.
    fn write_sweep(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = LABEL_WIDTH;
        writeln!(
            f,
            "what the output budget's sweep would take, and in which order"
        )?;
        writeln!(f)?;
        writeln!(
            f,
            "  A SWEEP AND A SPAN TAKE DIFFERENT THINGS, so the two figures are never summed"
        )?;
        writeln!(
            f,
            "  here. A sweep frees the collected bytes under files/ and leaves the record,"
        )?;
        writeln!(
            f,
            "  its log and its manifest standing, which is what lets the manifest outlive"
        )?;
        writeln!(
            f,
            "  the bytes (D152); a span takes the whole directory. A sweep also does not"
        )?;
        writeln!(
            f,
            "  look at a record's class or its span at all, so this is one list and not"
        )?;
        writeln!(
            f,
            "  four: a class whose span is 0 -- one that NEVER EXPIRES above -- can still"
        )?;
        writeln!(
            f,
            "  be the first collection a sweep takes, and on an old root it usually is."
        )?;
        if !self.verdict.is_readable() {
            writeln!(f)?;
            writeln!(
                f,
                "  The refusal above is about the clock and the spans. Every figure in this"
            )?;
            writeln!(
                f,
                "  block but one is read off the records and the file lengths alone: the one"
            )?;
            writeln!(
                f,
                "  judged against that clock reading is the age beside each candidate."
            )?;
        }
        writeln!(f)?;
        writeln!(
            f,
            "  {:<width$}{} in {} collection(s)",
            "held under files/",
            measured_bytes(self.sweep.held),
            self.sweep.holders
        )?;
        writeln!(
            f,
            "  {:<width$}{} in {} collection(s)",
            "  a sweep could free",
            measured_bytes(self.sweep.freeable()),
            self.sweep.order.len()
        )?;
        writeln!(
            f,
            "  {:<width$}{} in {} collection(s)",
            "  it could not free",
            measured_bytes(self.sweep.withheld_bytes()),
            self.sweep.withheld.len()
        )?;
        for one in self.sweep.withheld.iter().take(NAMED_AT_MOST) {
            writeln!(
                f,
                "    {JOBS_DIR}/{}  {}  {}",
                one.id,
                measured_bytes(one.bytes),
                one.why.said()
            )?;
        }
        and_more(f, self.sweep.withheld.len(), "    ")?;
        writeln!(
            f,
            "  {:<width$}{} in {} collection(s), the writer's and never a candidate",
            "still arriving",
            measured_bytes(self.sweep.arriving),
            self.sweep.arrivals
        )?;
        writeln!(
            f,
            "  {:<width$}{} in {} tree(s), given back before anything is chosen",
            "a killed sweep left",
            measured_bytes(self.sweep.leftover),
            self.sweep.leftovers
        )?;
        writeln!(
            f,
            "  {:<width$}{} in {} directory(s), not a job id and so not a sweep's at all",
            "not a job directory",
            measured_bytes(self.sweep.not_a_job),
            self.sweep.not_jobs
        )?;
        writeln!(f)?;
        writeln!(
            f,
            "  NO BUDGET WAS GIVEN AND NONE IS NEEDED. The hub's --output-budget is the"
        )?;
        writeln!(
            f,
            "  serving hub's own argument and no byte of this root records it, so rather"
        )?;
        writeln!(
            f,
            "  than answer for one figure this answers for every one: read down the \"freed"
        )?;
        writeln!(
            f,
            "  by then\" column below until it reaches held minus the budget, and what is"
        )?;
        writeln!(
            f,
            "  above that line is what that budget would take. The budget an operator is"
        )?;
        writeln!(
            f,
            "  thinking of raising to is answered by the same column, at no further cost."
        )?;
        writeln!(f)?;
        writeln!(
            f,
            "  THE FLOOR IS THE ONE KEY THAT IS NOT HERE. A sweep's first key is"
        )?;
        writeln!(
            f,
            "  --output-floor: outside the floor before inside it, because the floor is a"
        )?;
        writeln!(
            f,
            "  promise and nothing inside it goes while anything outside it remains. That"
        )?;
        writeln!(
            f,
            "  figure is the serving hub's too, so the order below is the sweep's within"
        )?;
        writeln!(
            f,
            "  one floor band. Under a floor of F, every candidate younger than F moves to"
        )?;
        writeln!(
            f,
            "  the end of this list and keeps this order among themselves -- the partition"
        )?;
        writeln!(
            f,
            "  is stable -- and the freed column re-runs along what comes out. The age"
        )?;
        writeln!(f, "  beside each candidate is printed for that arithmetic.")?;
        writeln!(f)?;
        writeln!(
            f,
            "  the order a sweep takes them in, oldest-evidence-last by three keys: a job"
        )?;
        writeln!(
            f,
            "  that passed before one that did not, fetched before never fetched, then"
        )?;
        writeln!(f, "  oldest first")?;
        if self.sweep.order.is_empty() {
            writeln!(
                f,
                "    none: no job directory on this root holds a collection a sweep could \
                 take"
            )?;
        }
        for candidate in self.sweep.order.iter().take(NAMED_AT_MOST) {
            writeln!(
                f,
                "    {JOBS_DIR}/{}  {} in {} file(s)  freed by then {}  {}, {}, ended {}, \
                 age {}",
                candidate.id,
                measured_bytes(candidate.bytes),
                candidate.files,
                measured_bytes(candidate.freed_by_then),
                if candidate.passed {
                    "passed"
                } else {
                    "did not pass"
                },
                if candidate.fetched {
                    "fetched"
                } else {
                    "never fetched"
                },
                candidate.ended_at,
                said_span(candidate.age),
            )?;
        }
        and_more(f, self.sweep.order.len(), "    ")?;
        writeln!(
            f,
            "  Nothing above was removed either, and this subcommand cannot remove it: the"
        )?;
        writeln!(
            f,
            "  two questions on this page are one walk, and the walk opens files read-only."
        )?;
        Ok(())
    }
}

/// One side of a span, population by population.
///
/// **Three figures and three counts, never one number.** A sweep can free
/// the first; nothing in Herd can free the other two but removing the
/// record, and nothing anywhere bounds the third. The unknown logs are
/// printed immediately under the measured ones so that the measured figure
/// is never read as a total when it is a floor.
fn write_weight(f: &mut fmt::Formatter<'_>, weight: &Weight, indent: &str) -> fmt::Result {
    let width = LABEL_WIDTH;
    writeln!(
        f,
        "{indent}{:<width$}{} in {} file(s)",
        "collected files",
        measured_bytes(weight.collected_bytes),
        weight.collected_files
    )?;
    writeln!(
        f,
        "{indent}{:<width$}{} record(s)",
        "  naming no file", weight.without_collected_file
    )?;
    writeln!(
        f,
        "{indent}{:<width$}{} in {} record(s)",
        "the records",
        measured_bytes(weight.record_bytes),
        weight.records
    )?;
    writeln!(
        f,
        "{indent}{:<width$}{} in {} log(s)",
        "the logs",
        measured_bytes(weight.log_bytes),
        weight.logs
    )?;
    writeln!(
        f,
        "{indent}{:<width$}{} log(s): UNKNOWN, never nought",
        "  could not be read",
        weight.logs_unknown.len()
    )?;
    let named = format!("{indent}    ");
    for (id, why) in weight.logs_unknown.iter().take(NAMED_AT_MOST) {
        writeln!(f, "{named}{JOBS_DIR}/{id}: {why}")?;
    }
    and_more(f, weight.logs_unknown.len(), &named)?;
    writeln!(
        f,
        "{indent}{:<width$}{} record(s)",
        "  no log ever written", weight.without_log
    )?;
    writeln!(
        f,
        "{indent}{:<width$}{}",
        "measured in all",
        measured_bytes(weight.measured_bytes())
    )?;
    Ok(())
}

/// One record's three populations, on the line that names it.
fn said_three(weighed: &Weighed) -> String {
    format!(
        "files {}, record {}, log {}",
        measured_bytes(weighed.collected_bytes),
        measured_bytes(weighed.record_bytes),
        match &weighed.log {
            LogWeight::Measured(bytes) => measured_bytes(*bytes),
            LogWeight::Unknown(_) => "UNKNOWN".to_owned(),
            LogWeight::NeverWritten => "none written".to_owned(),
        }
    )
}

/// "and N more", at the indentation of the list it follows, when a list was
/// cut off at [`NAMED_AT_MOST`].
fn and_more(f: &mut fmt::Formatter<'_>, total: usize, indent: &str) -> fmt::Result {
    if let Some(more) = total.checked_sub(NAMED_AT_MOST) {
        if more > 0 {
            writeln!(f, "{indent}and {more} more")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(86_400);

    /// An absent `record_version` reads as **absent**, never as `0`.
    ///
    /// The preview's refusal quotes this field, and a `u32` with a serde
    /// default made it say *"it carries record version 0"* about a record
    /// carrying no version at all — a sentence about a field with a value in
    /// it. `report.rs` draws the same line on its own `RecordOutputs`, and
    /// `job.rs` draws it on `at_its_ceiling`: zero is a thing a record can
    /// say, so absence may not borrow it.
    #[test]
    fn an_absent_record_version_is_absent_and_not_zero() {
        let absent: VersionProbe = serde_json::from_str("{}").expect("an empty object parses");
        assert_eq!(absent.record_version, None, "absent is absent");
        let zero: VersionProbe =
            serde_json::from_str(r#"{"record_version":0}"#).expect("a zero parses");
        assert_eq!(
            zero.record_version,
            Some(0),
            "a record that really says 0 still says a version"
        );
        let ours: VersionProbe =
            serde_json::from_str(&format!(r#"{{"record_version":{RECORD_VERSION}}}"#))
                .expect("this build's own version parses");
        assert_eq!(ours.record_version, Some(RECORD_VERSION));
    }

    fn spans() -> Spans {
        Spans::new(90 * DAY, 14 * DAY, 14 * DAY, 14 * DAY)
    }

    fn at(unix: i64) -> Timestamp {
        Timestamp::at_unix_seconds(unix)
    }

    /// Settlement 3, at the one place the meaning is decided.
    #[test]
    fn a_span_of_zero_means_never_and_not_now() {
        let never = Spans::new(Duration::ZERO, 14 * DAY, Duration::ZERO, 14 * DAY);
        assert!(never.never_expires(Priority::Merger));
        assert!(never.never_expires(Priority::Nightly));
        assert!(!never.never_expires(Priority::Step));
        let ancient = Judged {
            class: Priority::Merger,
            terminal: true,
            ended_at: at(0),
        };
        assert_eq!(
            select(ancient, &never, at(100_000 * 86_400)),
            Selection::NeverExpires,
            "a span of 0 keeps a record a century old"
        );
    }

    /// The boundary, said once in a test so it cannot drift: kept **for** the
    /// span, removable at it.
    #[test]
    fn the_span_boundary_is_inclusive_at_exactly_the_span() {
        let spans = spans();
        let ended = at(1_000_000);
        let judged = Judged {
            class: Priority::Step,
            terminal: true,
            ended_at: ended,
        };
        let one_second_short = at(1_000_000 + 14 * 86_400 - 1);
        assert_eq!(
            select(judged, &spans, one_second_short),
            Selection::Inside {
                left: Duration::from_secs(1)
            }
        );
        assert_eq!(
            select(judged, &spans, at(1_000_000 + 14 * 86_400)),
            Selection::Expired {
                past_by: Duration::ZERO
            }
        );
    }

    /// Settlement 4's half that lives in the selection itself.
    #[test]
    fn a_job_that_is_not_over_is_never_selected() {
        let spans = spans();
        let long_ago = Judged {
            class: Priority::Nightly,
            terminal: false,
            ended_at: at(0),
        };
        assert!(!select(long_ago, &spans, at(10_000 * 86_400)).is_expired());
    }

    /// A clock behind a record makes it read young, never old.
    #[test]
    fn a_backward_clock_can_only_make_a_record_read_young() {
        let spans = spans();
        let judged = Judged {
            class: Priority::Step,
            terminal: true,
            ended_at: at(1_000_000),
        };
        assert_eq!(
            select(judged, &spans, at(0)),
            Selection::Inside { left: 14 * DAY },
            "the age saturates at nought, so the whole span is left"
        );
    }

    /// The three populations, measured positively, off a directory whose
    /// every length is one this test chose.
    #[test]
    fn a_job_directory_weighs_three_populations_and_not_one_number() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let at = dir.path();
        fs::write(at.join(RECORD_FILE), vec![b'r'; 100]).expect("a record");
        fs::write(at.join("events.jsonl"), vec![b'e'; 30]).expect("some events");
        fs::write(at.join(LOG_SEQ_FILE), vec![b'0'; 41]).expect("the counter");
        fs::write(at.join(LOG_FILE), vec![b'l'; 7_000]).expect("a log");
        fs::create_dir_all(at.join(KEPT_FILES_DIR).join("deep")).expect("a collection");
        fs::write(at.join(KEPT_FILES_DIR).join("one.bin"), vec![b'1'; 5]).expect("a file");
        fs::write(
            at.join(KEPT_FILES_DIR).join("deep").join("two.bin"),
            vec![b'2'; 6],
        )
        .expect("a nested file");
        fs::create_dir_all(at.join(OUTPUT_PARTS_DIR)).expect("a part");
        fs::write(at.join(OUTPUT_PARTS_DIR).join("half.bin"), vec![b'h'; 9])
            .expect("an arriving file");

        let weighed = weigh(at);
        assert_eq!(weighed.collected_bytes, 20, "5 + 6 + 9");
        assert_eq!(weighed.collected_files, 3, "at every depth");
        assert_eq!(weighed.record_bytes, 171, "100 + 30 + 41, and not the log");
        assert_eq!(weighed.log, LogWeight::Measured(7_000));
        assert_eq!(weighed.measured_bytes(), Some(7_191));
    }

    /// A missing log is never nought. Which of the two kinds of absence it
    /// is comes from the hub's own counter beside it.
    #[test]
    fn a_log_that_cannot_be_measured_is_unknown_and_never_nought() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        // A job that was assigned -- `begin_attempt` writes `log.seq` --
        // whose log is not there. Its bytes are unknown.
        let assigned = dir.path().join("assigned");
        fs::create_dir_all(&assigned).expect("a job directory");
        fs::write(assigned.join(RECORD_FILE), vec![b'r'; 512]).expect("a record");
        fs::write(assigned.join(LOG_SEQ_FILE), vec![b'0'; 41]).expect("the counter");
        let weighed = weigh(&assigned);
        match &weighed.log {
            LogWeight::Unknown(why) => assert!(
                why.contains("unknown and not nought"),
                "the reason says so: {why}"
            ),
            other => panic!("a log with a counter and no file is unknown, not {other:?}"),
        }
        assert_eq!(
            weighed.log.bytes(),
            None,
            "nothing that sums may read it as nought"
        );
        assert_eq!(weighed.measured_bytes(), None, "and nor may the total");
        assert_eq!(weighed.record_bytes, 553, "512 + 41, measured either way");

        // A job that never ran: no log and no counter either. Nought is a
        // measurement here, and a variant of its own so the two cannot be
        // added up by accident.
        let never = dir.path().join("never");
        fs::create_dir_all(&never).expect("a job directory");
        fs::write(never.join(RECORD_FILE), vec![b'r'; 512]).expect("a record");
        let weighed = weigh(&never);
        assert_eq!(weighed.log, LogWeight::NeverWritten);
        assert_eq!(weighed.log.bytes(), Some(0));
        assert_eq!(weighed.measured_bytes(), Some(512));
    }

    /// Every record on one side is in exactly one of the three log figures,
    /// so an operator can check the counts against the record count.
    #[test]
    fn every_record_lands_in_exactly_one_log_figure() {
        let mut weight = Weight::default();
        weight.add(
            "measured",
            &Weighed {
                collected_bytes: 8,
                collected_files: 2,
                // All of it under `files/`, which is the ordinary shape: the
                // sweep's split of the same measurement.
                kept_bytes: 8,
                kept_files: 2,
                arriving_bytes: 0,
                leftover_bytes: 0,
                leftover: false,
                record_bytes: 100,
                log: LogWeight::Measured(64 << 20),
            },
        );
        weight.add(
            "unknown",
            &Weighed {
                collected_bytes: 0,
                collected_files: 0,
                kept_bytes: 0,
                kept_files: 0,
                arriving_bytes: 0,
                leftover_bytes: 0,
                leftover: false,
                record_bytes: 100,
                log: LogWeight::Unknown("it is not there".to_owned()),
            },
        );
        weight.add(
            "never",
            &Weighed {
                collected_bytes: 0,
                collected_files: 0,
                kept_bytes: 0,
                kept_files: 0,
                arriving_bytes: 0,
                leftover_bytes: 0,
                leftover: false,
                record_bytes: 100,
                log: LogWeight::NeverWritten,
            },
        );
        assert_eq!(weight.records, 3);
        assert_eq!(
            weight.logs + weight.logs_unknown.len() + weight.without_log,
            weight.records,
            "every record is in exactly one of the three"
        );
        assert_eq!(
            weight.without_collected_file, 2,
            "the common case, and counted"
        );
        assert_eq!(weight.collected_files, 2);
        assert_eq!(weight.log_bytes, 64 << 20, "the unknown one adds nothing");
        assert_eq!(weight.measured_bytes(), 8 + 300 + (64 << 20));
        assert_eq!(weight.logs_unknown[0].0, "unknown", "and it is named");
    }

    #[test]
    fn a_span_needs_a_unit_and_zero_needs_none() {
        assert_eq!(parse_span("14d"), Ok(14 * DAY));
        assert_eq!(parse_span("90d"), Ok(90 * DAY));
        assert_eq!(parse_span("36h"), Ok(Duration::from_secs(129_600)));
        assert_eq!(parse_span("90m"), Ok(Duration::from_secs(5_400)));
        assert_eq!(parse_span("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_span(" 0 "), Ok(Duration::ZERO));
        // The mistake this shape exists to make unrepresentable: a bare
        // figure, read as seconds, would select every record on the root.
        assert!(parse_span("14").is_err(), "a bare figure carries no unit");
        assert!(parse_span("d").is_err());
        assert!(parse_span("14w").is_err());
        assert!(parse_span("-1d").is_err());
        assert!(parse_span("").is_err());
    }
}
