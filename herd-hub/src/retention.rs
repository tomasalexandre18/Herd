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

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::job::{in_days, JobRecord, JobState, Timestamp, RECORD_VERSION};
use crate::report::Unread;
use crate::store::{
    directory_bytes, JOBS_DIR, KEPT_FILES_DIR, OUTPUT_GONE_DIR, OUTPUT_PARTS_DIR, RECORD_FILE,
};
use herd_proto::{LabelSet, Priority};

/// How many records of one kind the preview names before it stops listing and
/// says how many more there are. Ten, for [`crate::report`]'s reason: a list
/// exists to be acted on, and one naming four thousand ids is a number, which
/// the count already is.
const NAMED_AT_MOST: usize = 10;

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

/// One record the preview has something to say about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The job directory's name, which is the job id.
    pub id: String,
    pub class: Priority,
    pub ended_at: Timestamp,
    /// What the whole job directory measures: `job.json`, `events.jsonl`,
    /// the log and every inline file. **What removing this record would
    /// free**, which is the only figure the question "what would it cost
    /// me?" can mean.
    pub bytes: u64,
    /// Of that, what is under `files/`, `files.part/` and `files.gone/` —
    /// the output bytes, which the budget already governs (D148, D152).
    /// Broken out because retention removes them in the same act and the
    /// budget may not, so an operator comparing the two mechanisms needs
    /// them apart.
    pub output_bytes: u64,
}

impl Seen {
    /// What is not an output byte: the record, the events and the log. The
    /// evidence retention is actually about.
    pub fn record_bytes(&self) -> u64 {
        self.bytes.saturating_sub(self.output_bytes)
    }
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
    /// What they weigh, in all and in output bytes.
    pub expired_bytes: u64,
    pub expired_output_bytes: u64,
    /// Of the judged, the ones inside the span, and what they weigh.
    pub inside: usize,
    pub inside_bytes: u64,
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
    #[serde(default)]
    record_version: u32,
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
            let record_path = dir.join(RECORD_FILE);
            let text = match fs::read_to_string(&record_path) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    preview.scanned.without_record += 1;
                    continue;
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it could not be read: {error}"),
                    });
                    continue;
                }
            };
            // The version first, exactly as `read_stored` does it: a record
            // from another layout is named by its version and not by the
            // field that happened to fail.
            match serde_json::from_str::<VersionProbe>(&text) {
                Ok(probe) if probe.record_version == RECORD_VERSION => {}
                Ok(probe) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!(
                            "it carries record version {}, and this build reads version \
                             {RECORD_VERSION}",
                            probe.record_version
                        ),
                    });
                    continue;
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("it is not JSON this build can parse: {error}"),
                    });
                    continue;
                }
            }
            // Then the labels, because that is what quarantines a record
            // (D52) and a quarantined record must never reach a figure.
            match serde_json::from_str::<DurableLabels>(&text) {
                Ok(durable) => {
                    if let Err(why) = LabelSet::new(durable.labels.clone()) {
                        preview.scanned.quarantined.push((
                            name.clone(),
                            format!(
                                "its labels {:?} are not labels this hub accepts: {why}",
                                durable.labels
                            ),
                        ));
                        continue;
                    }
                }
                Err(error) => {
                    preview.scanned.unread.push(Unread {
                        path: format!("{JOBS_DIR}/{name}/{RECORD_FILE}"),
                        why: format!("its labels are not a list of names: {error}"),
                    });
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
                    continue;
                }
            };
            preview.scanned.read += 1;

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
                    class.inside += 1;
                    let (bytes, _) = weigh(&dir);
                    class.inside_bytes = class.inside_bytes.saturating_add(bytes);
                }
                Selection::Inside { left } => {
                    class.judged += 1;
                    class.inside += 1;
                    let (bytes, _) = weigh(&dir);
                    class.inside_bytes = class.inside_bytes.saturating_add(bytes);
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
                    let (bytes, output_bytes) = weigh(&dir);
                    class.expired_bytes = class.expired_bytes.saturating_add(bytes);
                    class.expired_output_bytes =
                        class.expired_output_bytes.saturating_add(output_bytes);
                    class.expired.push(Seen {
                        id: name.clone(),
                        class: parsed.priority,
                        ended_at: parsed.updated_at,
                        bytes,
                        output_bytes,
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

    /// What removing every expired record would free, over every class.
    pub fn expired_bytes(&self) -> u64 {
        self.classes
            .values()
            .fold(0u64, |sum, class| sum.saturating_add(class.expired_bytes))
    }

    /// Of that, the output bytes.
    pub fn expired_output_bytes(&self) -> u64 {
        self.classes.values().fold(0u64, |sum, class| {
            sum.saturating_add(class.expired_output_bytes)
        })
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

/// What a job directory weighs, in all and in output bytes.
///
/// Both by `metadata().len()` and never by contents: `directory_bytes` is the
/// sweeper's own walk, used on purpose, so a figure this preview prints and a
/// figure the sweeper prints about the same bytes are the same figure.
///
/// The whole directory, because the whole directory is what retention
/// removes: `job.json`, `events.jsonl`, the log, `log.seq` and the inline
/// files. The output bytes are broken out because the budget governs those
/// and may never remove the record (`docs/disk-policy.md` §3.3), so an
/// operator weighing the two mechanisms needs them apart.
fn weigh(dir: &Path) -> (u64, u64) {
    let total = directory_bytes(dir);
    let outputs = [KEPT_FILES_DIR, OUTPUT_PARTS_DIR, OUTPUT_GONE_DIR]
        .iter()
        .fold(0u64, |sum, name| {
            sum.saturating_add(directory_bytes(&dir.join(name)))
        });
    // Clamped rather than asserted: a subdirectory a later milestone adds
    // lands in the remainder, which is the harmless side, and a preview must
    // not panic on a root it was merely asked to read.
    (total, outputs.min(total))
}

/// A byte count as this preview prints one: the exact figure always, and an
/// approximation beside it once the exact figure stops being readable.
///
/// [`crate::report`]'s rule and the same function, deliberately duplicated in
/// neither direction of meaning: every number here is evidence somebody will
/// do arithmetic on, and a rounded one is how a figure gets quoted back as
/// the figure it was not.
fn said_bytes(bytes: u64) -> String {
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
            Verdict::Readable => self.write_classes(f),
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
                Ok(())
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
                Ok(())
            }
        }
    }
}

impl RetentionPreview {
    fn write_classes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "what retention would remove, by class")?;
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
            writeln!(
                f,
                "      what they weigh        {}",
                said_bytes(figures.expired_bytes)
            )?;
            writeln!(
                f,
                "        of which output      {}",
                said_bytes(figures.expired_output_bytes)
            )?;
            writeln!(
                f,
                "        the records          {}",
                said_bytes(
                    figures
                        .expired_bytes
                        .saturating_sub(figures.expired_output_bytes)
                )
            )?;
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
                    "      {JOBS_DIR}/{} {} ended {}",
                    seen.id,
                    said_bytes(seen.bytes),
                    seen.ended_at
                )?;
            }
            and_more(f, figures.expired.len(), "      ")?;
            writeln!(
                f,
                "    inside the span          {} record(s), {}",
                figures.inside,
                said_bytes(figures.inside_bytes)
            )?;
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
        writeln!(
            f,
            "  what that would free     {}",
            said_bytes(self.expired_bytes())
        )?;
        writeln!(
            f,
            "    of which output        {}",
            said_bytes(self.expired_output_bytes())
        )?;
        writeln!(
            f,
            "    the records            {}",
            said_bytes(
                self.expired_bytes()
                    .saturating_sub(self.expired_output_bytes())
            )
        )?;
        writeln!(
            f,
            "  Nothing above was removed, and this subcommand cannot remove it."
        )?;
        Ok(())
    }
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
