//! The hub's state, as files: one directory per job (D8).
//!
//! ```text
//! <state root>/
//!   store.json              the layout's version and this root's identity;
//!                           rewritten at every start
//!   hub.lock                held by the one hub using this root (D21)
//!   jobs/
//!     j0000000001/
//!       job.json            the record: fields, current state, timestamps
//!       events.jsonl         one line per thing that happened, in order
//!       log                 the job's output, both streams in arrival order
//!       log.seq             how many chunks of that log the hub has written,
//!                           and how many bytes they are
//!       files/              room for what a job keeps (M11's artefacts)
//!   index/
//!     state.json            derived: which jobs are not over yet
//!   readings/
//!     answers.json          derived: what every `labels_from` reading
//!                           answered, by (commit, command, args) (D74)
//! ```
//!
//! **The job directories are the only source of truth.** `index/state.json`
//! exists so that starting the hub does not have to parse the whole history:
//! it lists the jobs that have not reached a terminal state. It is derived and
//! rebuildable, and the hub checks it rather than believing it — see
//! [`JobDirStore::open`]. Every write goes through [`crate::durable`]; this
//! module never touches `std::fs` itself.
//!
//! **`log` is arrival-ordered, and that is all it promises (D29).** It is the
//! job's output as a terminal would have shown it: the chunks in the order
//! they reached the hub, byte for byte, both streams in one file. It does not
//! promise per-stream exactness — `echo OUT1; echo ERR1 >&2; echo OUT2` may
//! legitimately record `OUT1 OUT2 ERR1`, because the two pipes are drained
//! independently and a chunk's place in the file is the moment it arrived.
//! That is chosen, not accidental: a build log is read as a terminal
//! transcript, and reconstructing the true interleaving would cost a second
//! index on every chunk of a three-million-line job. The chunk numbers serve
//! gap detection — `log.seq` and the marker in [`log_gap_marker`] — and
//! nothing anywhere reorders by them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use herd_proto::{JobId, LabelSet, LogStream, Priority, Requirements, StepRun};
use serde::{Deserialize, Serialize};

use crate::durable;
use crate::job::{
    Attempt, JobEvent, JobMetrics, JobRecord, JobState, JobStore, LogWrite, NewJob, Raise, Reading,
    Result, StoreError, Submission, Timestamp, RECORD_VERSION,
};

/// The version of the directory layout above. A state root written by a newer
/// Herd is refused rather than read as if it were this one.
pub const LAYOUT_VERSION: u32 = 1;

/// The version of the derived index. It needs no compatibility: a version this
/// Herd does not know is one more reason to rebuild it.
const INDEX_VERSION: u32 = 1;

const STORE_FILE: &str = "store.json";
const JOBS_DIR: &str = "jobs";
const INDEX_DIR: &str = "index";
const INDEX_FILE: &str = "state.json";
const RECORD_FILE: &str = "job.json";
const EVENTS_FILE: &str = "events.jsonl";
const LOG_FILE: &str = "log";
/// How many chunks of a job's `log` the hub has written — which is also the
/// number it expects the next chunk to carry — and how many bytes those
/// chunks are. The hub's own bookkeeping, not part of the record: two
/// fixed-width decimals, rewritten in place beside the log they count, so
/// that a hub starting on this state root can still tell whether the chunks
/// that follow are the ones that follow, and can tell a log holding bytes it
/// never counted from one that does not.
const LOG_SEQ_FILE: &str = "log.seq";
/// The width of that number, in digits: a `u64` never needs more, and a fixed
/// width is what lets the file be written in place with nothing to truncate.
const LOG_SEQ_DIGITS: usize = 20;
const KEPT_FILES_DIR: &str = "files";

/// Where a streamed collection's part-written files wait, beside `files/` and
/// never inside it.
///
/// **A sibling, not a suffix.** A `.part` file under `files/` would be visible
/// to a consumer reading the record's directory while the job's collection is
/// still arriving (E-53), and the one promise that directory makes is that
/// what is in it is whole. So a file becomes visible by being **renamed** into
/// `files/`, after its digest has been checked, and until then it is in here
/// where nothing looks for it.
const OUTPUT_PARTS_DIR: &str = "files.part";
/// Where a collection the sweeper has taken waits while it is unlinked
/// (D152).
///
/// **D8's discipline, run backwards.** A write becomes visible by one
/// rename; a removal becomes invisible by one too. `remove_dir_all` over a
/// collection of several hundred files is not instantaneous on a loaded
/// host, and a reader arriving halfway through it would find a directory
/// holding four of twelve files with nothing saying so — a short collection
/// that reads like a whole one, which is the one failure this whole design
/// refuses. So `files/` is renamed here first, which is atomic, and the
/// unlinking happens afterwards with no lock held and nothing looking.
///
/// A directory of this name left behind is a hub that stopped mid-eviction.
/// The next sweep removes it, and the first sweep runs at startup.
const OUTPUT_GONE_DIR: &str = "files.gone";
const LOCK_FILE: &str = "hub.lock";
/// Where the answers of `labels_from` readings are kept, by their (commit,
/// command, args) key (D74). Derived, like the index: an answer that cannot be
/// read is one more reading to pay for and never a reason to refuse to start.
const READINGS_DIR: &str = "readings";
const READINGS_FILE: &str = "answers.json";
/// The version of that file. Like [`INDEX_VERSION`], a version this Herd does
/// not know is one more reason to start again with nothing in it.
const READINGS_VERSION: u32 = 1;

/// Digits in a job id, after the `j`. Zero-padded so that the directory
/// listing of `jobs/` is in the order the jobs were minted.
const ID_DIGITS: usize = 10;

/// The hub's job store: a directory per job under one state root.
#[derive(Debug)]
pub struct JobDirStore {
    root: PathBuf,
    /// How much room a job's output files have here, and how long a fresh
    /// collection is promised (D148, D152).
    budget: crate::job::Budget,
    /// Who this state root is: minted when the root is created, kept in
    /// `store.json`, never changed afterwards.
    state_root_id: String,
    /// The claim on the state root, held open for as long as the store is
    /// (D21). Never read: holding it *is* the point, and the kernel takes it
    /// back the moment this process ends, however it ends.
    _lock: fs::File,
    /// Which runners an operator has approved, and the hash of each one's
    /// token (E-48, Q8). One file in this root, written through the same
    /// writer the records go through (D8).
    roster: crate::roster::Roster,
    live: Mutex<Live>,
}

/// What the store would otherwise have to re-read from the disk on every call.
/// Written through to `index/state.json` after every change, and never
/// consulted in place of a record: a record is always read from its file.
#[derive(Debug)]
struct Live {
    /// The next id to try. Ids are claimed by `mkdir`, so this is a hint that
    /// cannot collide even if it is wrong.
    next_sequence: u64,
    /// How many job directories exist. The index carries this number so that
    /// an index which has fallen behind the directories is detected.
    jobs_accounted: u64,
    /// The jobs that have not reached a terminal state: the queue.
    pending: BTreeSet<u64>,
    /// Which open job is doing which piece of work (E-05): the deduplication
    /// key of every job that is not over, and the job doing it.
    ///
    /// **Derived from the records, never persisted on its own.** Each key is
    /// computed from the record's own command and labels
    /// ([`JobRecord::dedup_key`]), so there is no second copy to disagree
    /// with them, and it is rebuilt from the pending records at every start —
    /// which is what makes a join survive a restart (D69). A map that lived
    /// only in this process would lose the link when the hub went down, and
    /// the next identical submission would run the same build a second time.
    open_work: HashMap<String, u64>,
    /// Per job that is still producing output, how far its log has got: how
    /// many chunks the hub has written — the number the next chunk should
    /// carry — and how many bytes those chunks are. Read from `log.seq` the
    /// first time this hub sees a chunk of a job, written through to it
    /// afterwards, and forgotten when the job is over.
    log_chunks: HashMap<JobId, LogAt>,
    /// What every `labels_from` reading this store has kept answered, by its
    /// (commit, command, args) key (D74).
    ///
    /// Read from `readings/answers.json` once, at every start, and written
    /// through whenever an answer lands. It is the one thing in this store
    /// that is a **cache**: an entry that cannot be read is an entry that
    /// never existed, and the next submission of those three pays for a
    /// reading of its own. That is why an unreadable file here does not stop
    /// the hub starting, where an unreadable record does.
    readings: BTreeMap<String, Requirements>,
    /// The jobs this hub has read and will not act on, with why (D52). Filled
    /// at recovery and never added to afterwards: what quarantines a record is
    /// a judgement about rules that do not change while the hub runs.
    quarantined: BTreeMap<JobId, String>,
    /// Roughly how many bytes of whole collections this hub holds (D152).
    ///
    /// **A trigger, not an answer.** It says when it is worth looking, and
    /// the looking is what decides anything: every sweep walks
    /// `jobs/*/files` and overwrites this with what it measured. So the
    /// figure may drift — a part file thrown away for a bad digest, a
    /// directory an operator removed by hand — and the drift costs a sweep
    /// that runs a little early or a little late, never a collection
    /// deleted on a number nobody checked. That is the whole reason the
    /// expensive, authoritative thing is on the rare path and the cheap,
    /// fallible thing is on the common one.
    held_bytes: u64,
    /// Whether this hub has already said it is over budget with nothing it
    /// may remove. Cleared the moment a sweep comes back inside it.
    said_over_budget: bool,
    /// Whether a sweep is already walking. Two collections completing
    /// together would otherwise start two walks that reach the same verdict,
    /// and the second would find its victims already gone.
    sweeping: bool,
}

/// How far a job has got in its log, as the hub counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogAt {
    chunks: u64,
    bytes: u64,
}

/// `store.json`.
#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    layout_version: u32,
    /// This state root's own identity. `#[serde(default)]` so that a root
    /// written before there was one reads as a root with no identity yet and
    /// is given one, rather than failing to open.
    #[serde(default)]
    state_root_id: String,
}

/// `index/state.json`.
#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    index_version: u32,
    jobs_accounted: u64,
    pending: Vec<JobId>,
}

/// `readings/answers.json`: what every reading this store has kept answered,
/// by its (commit, command, args) key (D74).
#[derive(Debug, Serialize, Deserialize)]
struct ReadingsFile {
    readings_version: u32,
    answers: BTreeMap<String, Requirements>,
}

/// `job.json`: the record, with the version of its shape in front of it.
#[derive(Debug, Serialize, Deserialize)]
struct RecordFile {
    record_version: u32,
    #[serde(flatten)]
    record: JobRecord,
}

/// One line of `events.jsonl`. Each line carries the version, so a line read
/// on its own — by `tail -f`, while the job runs — says what it is.
#[derive(Debug, Serialize, Deserialize)]
struct EventLine {
    record_version: u32,
    #[serde(flatten)]
    event: JobEvent,
}

/// Enough of any of the files above to read its version before trusting the
/// rest of its fields.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    #[serde(default)]
    record_version: u32,
}

/// A record's labels as the disk carries them, before today's rules judge
/// them (D52).
///
/// Plain strings, read permissively on purpose. [`LabelSet`] validates on
/// deserialisation, which is right at a submission and wrong here: a single
/// job persisted under rules that have since been tightened would otherwise
/// make the whole state root unreadable, and the hub would refuse to start at
/// all. One bad record must not take the service down, and a rule nobody can
/// ever tighten again is not a rule.
#[derive(Debug, Deserialize)]
struct DurableLabels {
    #[serde(default)]
    labels: Vec<String>,
}

/// What `job.json` turned out to be.
///
/// Three outcomes rather than two, because "readable, and not something this
/// hub may act on" is its own answer (D52): it is neither a missing record nor
/// a reason to fail.
enum Stored {
    /// No `job.json` in the directory at all.
    Missing,
    /// A record this hub reads and acts on.
    Read(Box<JobRecord>),
    /// A record this hub reads and refuses to act on, with why. Named,
    /// visible, skipped — never dispatched, never failed, never silently
    /// repaired.
    Quarantined(String),
}

/// What a writer found when it opened a job: a record it may write to, or one
/// that is over.
///
/// Carries the store's lock, so a writer cannot hold the record without
/// holding the mutex it was read under, and cannot have reached the record at
/// all without the terminal test in
/// [`JobDirStore::open_unless_over`] having been made.
enum Writable<'a> {
    /// The lock, and the record as it stands. Write through this.
    Live(std::sync::MutexGuard<'a, Live>, Box<JobRecord>),
    /// The job is over, named by the state it reached. Nothing is written.
    Over(String),
}

impl JobDirStore {
    /// Open, or create, a state root, and recover what is in it.
    ///
    /// A root that cannot be created, cannot be written, or was written by a
    /// Herd with a different layout is refused here, naming the path: the hub
    /// must not start believing it has no state when in truth it cannot read
    /// the state it has (the same rule E-45 applies to a runner's data root).
    ///
    /// Recovery reads the names in `jobs/` — which is also where the next id
    /// comes from — and then reads only the records the index says are
    /// unfinished. The index is used only if it parses, carries a version this
    /// Herd knows, accounts for exactly as many job directories as there are,
    /// names only jobs that exist, and leaves out no job whose record is not
    /// over. Otherwise it is ignored and every record is read, which is what
    /// rebuilds it.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(root, crate::job::Budget::default())
    }

    /// The same, with the output budget an operator set (D152).
    ///
    /// Separate so that every existing caller keeps the shipped figures and
    /// a test can make 200 GB happen inside a kilobyte. The budget belongs
    /// to the store because the sweeper does: the bytes it governs are this
    /// store's own files, written under this store's one lock and through
    /// the one writer D8 describes.
    ///
    /// **It sweeps once before it answers.** A hub restarted onto a volume
    /// that is already over budget — because the figure was lowered, or
    /// because the hub was down while a disk filled — has to come back
    /// inside it before it accepts the next job, not at the next verdict:
    /// waiting for the next job means the disk is full when the next job
    /// arrives, which is the outage this whole mechanism exists to prevent.
    /// It is also what establishes the running total the sweeper is later
    /// triggered by, and what clears any `files.gone` a crash left behind.
    pub fn open_with(root: impl AsRef<Path>, budget: crate::job::Budget) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        durable::create_dir_all(&root)
            .map_err(|error| failed("creating the state root", &root, &error))?;
        let jobs = root.join(JOBS_DIR);
        durable::create_dir(&jobs).map_err(|error| failed("creating", &jobs, &error))?;
        let index = root.join(INDEX_DIR);
        durable::create_dir(&index).map_err(|error| failed("creating", &index, &error))?;
        let readings = root.join(READINGS_DIR);
        durable::create_dir(&readings).map_err(|error| failed("creating", &readings, &error))?;

        // One hub per state root (D21), claimed before anything is written.
        let lock = claim(&root)?;
        // And a root the hub can actually use. Writing `store.json` below
        // proves the root itself is writable; the job directories and the
        // index are where every later write lands, and a hub that cannot
        // write them would start and then fail every submission with nothing
        // but its own log to say why.
        for directory in [&jobs, &index, &readings] {
            durable::probe_writable(directory)
                .map_err(|error| failed("writing in", directory, &error))?;
        }

        let store_file = root.join(STORE_FILE);
        // The root's identity: whatever is already in there, or a new one.
        //
        // Minted once and kept for ever, because the whole of its job is to
        // be *different* from the identity of the root that was here before.
        // A hub whose volume is recreated hands out `j0000000001` again, and
        // a runner may still be holding a verdict for the `j0000000001` of
        // the root that is gone — plausible, false, and indistinguishable
        // from the real thing without this (D42, and M2 flagged it).
        let mut state_root_id = String::new();
        if let Some(text) = read_optional(&store_file)? {
            let found: StoreFile = serde_json::from_str(&text)
                .map_err(|error| failed("reading", &store_file, &error))?;
            if found.layout_version != LAYOUT_VERSION {
                return Err(StoreError::new(format!(
                    "the state root {} has layout version {}, and this hub \
                     writes version {LAYOUT_VERSION}",
                    root.display(),
                    found.layout_version
                )));
            }
            state_root_id = found.state_root_id;
        }
        if state_root_id.is_empty() {
            state_root_id = mint_a_state_root_id();
        }
        // Rewriting it at every start is also the test that the root is
        // writable, and it is where a reader learns what the layout is.
        write_json(
            &store_file,
            &StoreFile {
                layout_version: LAYOUT_VERSION,
                state_root_id: state_root_id.clone(),
            },
        )?;

        // Who may connect, read before the first handshake can be answered.
        // A roster this hub cannot read is an error here, where it names the
        // file, rather than an empty roster that would refuse every runner
        // for a reason that blames the machines (E-48).
        let roster = crate::roster::Roster::open(&root)?;

        // The cache of readings, read once. Unreadable is empty, loudly: see
        // [`Live::readings`].
        let answers = read_the_readings(&root);
        let store = Self {
            root,
            budget,
            state_root_id,
            _lock: lock,
            roster,
            live: Mutex::new(Live {
                next_sequence: 1,
                jobs_accounted: 0,
                pending: BTreeSet::new(),
                open_work: HashMap::new(),
                log_chunks: HashMap::new(),
                readings: answers,
                quarantined: BTreeMap::new(),
                held_bytes: 0,
                said_over_budget: false,
                sweeping: false,
            }),
        };
        store.recover()?;
        // The first sweep, which is also the first measurement: see the
        // paragraph above.
        JobStore::sweep_outputs(&store)?;
        Ok(store)
    }

    /// The state root, for whoever has to say where the state is.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Who this state root is, as the handshake carries it.
    ///
    /// Not the path: a path is reused on purpose — the deployment's volume is
    /// always `/var/lib/herd` — and a path says nothing about whether the
    /// state *inside* it is the state a peer remembers. The identity is minted
    /// with the root and dies with it, which is exactly the lifetime a job id
    /// has meaning over.
    pub fn state_root_id(&self) -> &str {
        &self.state_root_id
    }

    /// The jobs this hub read and will not act on, with why, oldest first
    /// (D52).
    ///
    /// Empty on every ordinary state root. A job is here when its record is
    /// readable and what it asked for is something today's rules reject — a
    /// job minted before the rules were tightened. It is skipped, not failed
    /// and not repaired, and this is how it stays visible: the hub names each
    /// one as it starts, and whoever asks the store gets the same list.
    pub fn quarantined(&self) -> Result<Vec<(JobId, String)>> {
        Ok(self
            .lock()?
            .quarantined
            .iter()
            .map(|(id, why)| (id.clone(), why.clone()))
            .collect())
    }

    /// Where one job's files live. Stable for the life of the job: the first
    /// consumer reads this directory while the job runs (E-53), so nothing
    /// here ever renames or moves a job's directory.
    pub fn job_dir(&self, id: &JobId) -> PathBuf {
        self.root.join(JOBS_DIR).join(id.as_str())
    }

    fn jobs_dir(&self) -> PathBuf {
        self.root.join(JOBS_DIR)
    }

    /// Whether every file this job's manifest names is now whole on the disk.
    ///
    /// **`files/` only, never the part directory, and that distinction is a
    /// bug this had.** A part-written file that has reached its full length
    /// is *not* complete: its digest has not been checked and it has not been
    /// renamed, so for the hundred milliseconds it takes to hash 32 MiB it
    /// looks finished and is not. Judged off the part file, a transfer
    /// reported itself complete in that window, a requester was told its
    /// files were no longer coming, and it went looking in `files/` for
    /// something not yet there — which read as "the hub does not have it" for
    /// a collection that was seconds from arriving.
    ///
    /// So there is **one** definition of complete, here, used by every caller
    /// that needs one: the bytes are in `files/` at the manifest's length. A
    /// second definition elsewhere is what caused that, and the lesson is
    /// D79's — derive it once, so two answers cannot disagree.
    ///
    /// Read off the directory each time rather than counted as chunks land. A
    /// tally kept beside the files could disagree with them too, and the
    /// disagreement would be either a collection acknowledged before it was
    /// complete — destroying the runner's only copy — or one never
    /// acknowledged, leaving a runner holding bytes for ever.
    fn collection_is_whole(&self, id: &JobId, record: &JobRecord) -> bool {
        let kept = self.job_dir(id).join(KEPT_FILES_DIR);
        record
            .outputs
            .files
            .iter()
            .all(|file| file_length(&kept.join(&file.path)) == Some(file.bytes))
    }

    /// Add bytes that have just become whole to the running total, and sweep
    /// if that has put the hub over its budget (D152).
    ///
    /// **The one trigger, and it is an event rather than a clock.** The only
    /// thing that can put a hub over its output budget is a collection
    /// arriving, so a timer would be a second writer waking up on a quiet
    /// hub to find nothing to do — and a walk of the whole store on every
    /// verdict would be the same work done for the ninety-nine verdicts that
    /// changed nothing. Nothing is removed for merely being old: the floor
    /// is a floor, not a lifetime.
    ///
    /// Called with no lock held, because the sweep takes its own.
    fn held_more(&self, bytes: u64) -> Result<()> {
        let over = {
            let mut live = self.lock()?;
            live.held_bytes = live.held_bytes.saturating_add(bytes);
            live.held_bytes > self.budget.bytes
        };
        if over {
            JobStore::sweep_outputs(self)?;
        }
        Ok(())
    }

    /// Take one job's output bytes, with the record saying so first.
    ///
    /// Everything that makes this safe is the order of four steps:
    ///
    /// 1. the record is **re-read under the lock**, because the sweep's walk
    ///    is deliberately unlocked and a collection can have been written,
    ///    fetched or already evicted since it was measured;
    /// 2. `files/` is **renamed** to [`OUTPUT_GONE_DIR`], which is atomic,
    ///    so the collection stops being half-there the instant it stops
    ///    being there;
    /// 3. the record is written and the job's own history gets its line —
    ///    the manifest is untouched, so every path, length and digest
    ///    outlives the bytes exactly as D148 promised;
    /// 4. the unlinking happens **with no lock held**, because several
    ///    hundred `unlink` calls on a loaded host is the one part of this
    ///    that takes real time, and nothing is waiting on it.
    ///
    /// `None` when the record has changed under the walk and this is no
    /// longer a collection the sweeper may take. That is a skip and not an
    /// error: the next sweep will look again.
    fn evict(&self, id: &JobId, why: &str) -> Result<Option<u64>> {
        let dir = self.job_dir(id);
        let kept = dir.join(KEPT_FILES_DIR);
        let gone = dir.join(OUTPUT_GONE_DIR);
        let bytes = {
            let mut live = self.lock()?;
            let Some(mut record) = self.read_record(id)? else {
                return Ok(None);
            };
            if !record.state.is_terminal()
                || record.outputs.removed.is_some()
                || record.outputs.files.is_empty()
                || !self.collection_is_whole(id, &record)
            {
                return Ok(None);
            }
            let bytes = directory_bytes(&kept);
            fs::rename(&kept, &gone).map_err(|error| failed("renaming", &kept, &error))?;
            // `files/` exists in every job directory and has since M2, so it
            // is put back empty rather than left missing: a reader of the
            // record's directory sees the shape it has always had, with
            // nothing in it, and the record says why.
            durable::create_dir(&kept).map_err(|error| failed("creating", &kept, &error))?;
            let at = Timestamp::now();
            let files = record.outputs.files.len();
            record.outputs.removed = Some(crate::job::OutputsRemoved {
                at,
                why: why.to_owned(),
                bytes,
            });
            self.write_record(&record)?;
            self.append_event(id, &JobEvent::outputs_removed(at, files, bytes, why))?;
            live.held_bytes = live.held_bytes.saturating_sub(bytes);
            bytes
        };
        remove_tree(&gone)?;
        Ok(Some(bytes))
    }

    /// One sweep, with the one-at-a-time flag already taken.
    ///
    /// ## What it walks, and when
    ///
    /// `jobs/*/files` and nothing else — one `read_dir` per job directory,
    /// and a record read only for a job that actually holds bytes. A hub
    /// inside its budget reads **no records at all**: it measures, finds
    /// nothing to do, writes the figure down and stops. That is what makes
    /// the walk affordable on the rare path and the running total enough on
    /// the common one.
    ///
    /// ## The order, which is the whole decision (D152)
    ///
    /// Four keys, and the first of them outranks the other three:
    ///
    /// 1. **outside the floor before inside it.** Seven days is a promise,
    ///    so nothing inside it goes while anything outside it remains.
    /// 2. **a job that passed before a job that did not.** The consumer's
    ///    own sentence is *"results.json alone is not enough to diagnose a
    ///    red test"* — the artefacts of a green run are the ones nobody ever
    ///    opens, and the artefacts of a red one are the reason this whole
    ///    mechanism exists.
    /// 3. **fetched before never fetched.** This is where the obvious cache
    ///    answer is backwards: an LRU evicts what nothing has touched, and
    ///    here a collection nobody has fetched is evidence **nobody has
    ///    seen**, while one a requester read yesterday is already on that
    ///    requester's disk.
    /// 4. **oldest first**, which is only the tie-break. On its own it would
    ///    delete last week's one red test to keep this morning's thousand
    ///    green ones, which is the wrong thousand bytes every time.
    ///
    /// A collection still arriving is never a candidate under any of them:
    /// its bytes are the writer's, and taking them would destroy the thing
    /// the hub is in the middle of being given. The collection that
    /// *triggered* the sweep is a candidate once it is whole, and it is the
    /// last of its rank — so a hub whose budget is smaller than one
    /// collection takes that collection back the moment it completes,
    /// records why, and tells its requester with the figures. That is
    /// pathological at 200 GB and it is still the honest behaviour: the
    /// budget is a fact about a disk, and a collection that will not fit
    /// does not fit.
    fn sweep_now(&self) -> Result<crate::job::Sweep> {
        let jobs_dir = self.jobs_dir();
        let names =
            durable::names_in(&jobs_dir).map_err(|error| failed("listing", &jobs_dir, &error))?;
        let mut held = 0u64;
        let mut holders: Vec<(JobId, u64)> = Vec::new();
        let mut leftovers: Vec<PathBuf> = Vec::new();
        for name in names {
            if sequence_of(&name).is_none() {
                continue;
            }
            let dir = jobs_dir.join(&name);
            let gone = dir.join(OUTPUT_GONE_DIR);
            if gone.is_dir() {
                // A hub that stopped mid-eviction. The bytes are already
                // unreachable from `files/`, so they are nobody's evidence
                // any more — only disk to give back.
                leftovers.push(gone);
            }
            let bytes = directory_bytes(&dir.join(KEPT_FILES_DIR));
            if bytes == 0 {
                continue;
            }
            held += bytes;
            holders.push((JobId::new(name), bytes));
        }
        for gone in &leftovers {
            remove_tree(gone)?;
        }
        {
            let mut live = self.lock()?;
            live.held_bytes = held;
        }
        let budget = self.budget.bytes;
        if held <= budget {
            let swept = crate::job::Sweep {
                held,
                ..crate::job::Sweep::default()
            };
            self.say_what_the_sweep_did(&swept)?;
            return Ok(swept);
        }

        // Over budget, so the records are worth reading: this is the rare
        // path, and everything below it is about choosing well rather than
        // choosing fast.
        let now = Timestamp::now();
        struct Candidate {
            id: JobId,
            inside_the_floor: bool,
            /// Sorted on, so the field names what the order means rather
            /// than what it is: `true` goes first.
            passed: bool,
            fetched: bool,
            ended_at: Timestamp,
        }
        let mut candidates = Vec::new();
        let mut could_not_free = 0u64;
        for (id, bytes) in holders {
            let Ok(Some(record)) = self.read_record(&id) else {
                // A record that will not read is a louder problem than this
                // and recovery says so where it belongs (D52). What it is
                // not is a licence to delete the files it describes.
                could_not_free += bytes;
                continue;
            };
            let JobState::Finished { outcome } = &record.state else {
                could_not_free += bytes;
                continue;
            };
            if record.outputs.removed.is_some() || !self.collection_is_whole(&id, &record) {
                could_not_free += bytes;
                continue;
            }
            // The bytes this job holds are measured again inside `evict`,
            // under the lock, because the walk is deliberately unlocked:
            // the figure that decides how far the loop below goes has to be
            // the one the rename actually freed, not the one a walk saw a
            // moment earlier — so this candidate carries no size at all.
            candidates.push(Candidate {
                inside_the_floor: now.since(record.updated_at) < self.budget.floor,
                passed: outcome.is_success(),
                fetched: record.outputs.first_fetched_at.is_some(),
                ended_at: record.updated_at,
                id,
            });
        }
        candidates.sort_by(|a, b| {
            (a.inside_the_floor, !a.passed, !a.fetched, a.ended_at).cmp(&(
                b.inside_the_floor,
                !b.passed,
                !b.fetched,
                b.ended_at,
            ))
        });

        let mut swept = crate::job::Sweep {
            held,
            could_not_free,
            ..crate::job::Sweep::default()
        };
        for candidate in candidates {
            if swept.held <= budget {
                break;
            }
            let why = self.removal_sentence(now, candidate.inside_the_floor, candidate.ended_at);
            let Some(freed) = self.evict(&candidate.id, &why)? else {
                continue;
            };
            swept.held = swept.held.saturating_sub(freed);
            swept.freed += freed;
            swept.removed.push(candidate.id);
            if candidate.inside_the_floor {
                swept.inside_the_floor += 1;
            }
        }
        self.say_what_the_sweep_did(&swept)?;
        Ok(swept)
    }

    /// The sentence a requester is handed for a collection that is gone.
    ///
    /// **It names the figures, every time.** *"Removed"* with no date and no
    /// budget is the one report nobody can act on, and a quiet retention
    /// policy is a trap: the next thing that happens after evidence
    /// disappears without a reason is somebody concluding Herd lost it.
    fn removal_sentence(
        &self,
        at: Timestamp,
        inside_the_floor: bool,
        ended_at: Timestamp,
    ) -> String {
        let budget = herd_proto::show_bytes(self.budget.bytes);
        if !inside_the_floor {
            return format!("removed {} under the {budget} output budget", at.date());
        }
        format!(
            "removed {} under the {budget} output budget, {} old and so inside this hub's \
             {} floor: the fleet produced more than the budget in less than the floor, \
             and the budget is what the disk enforces",
            at.date(),
            crate::job::in_days(at.since(ended_at)),
            crate::job::in_days(self.budget.floor),
        )
    }

    /// One line per sweep, never one per collection (D151).
    ///
    /// A sweep that frees fifty collections is one event an operator reads
    /// once, not fifty journal writes onto a host that is already short of
    /// IO — which is precisely the defect D151 spent a day on, on both sides
    /// of the link. What is per-collection is the **record**: each evicted
    /// job's own `job.json` and `events.jsonl` say what went and why, for
    /// ever, which is where the answer to "what happened to this job's
    /// screenshots" belongs a week later.
    fn say_what_the_sweep_did(&self, swept: &crate::job::Sweep) -> Result<()> {
        let budget = herd_proto::show_bytes(self.budget.bytes);
        let floor = crate::job::in_days(self.budget.floor);
        if !swept.is_quiet() {
            println!(
                "herd-hub: the output sweeper removed {} collection(s), {}, to stay inside \
                 the {budget} budget; {} held now. Every one of those records still names \
                 every file it had, with its size and digest.",
                swept.removed.len(),
                herd_proto::show_bytes(swept.freed),
                herd_proto::show_bytes(swept.held),
            );
        }
        if swept.inside_the_floor > 0 {
            eprintln!(
                "herd-hub: {} of those collection(s) were still inside this hub's {floor} \
                 retention floor. {floor} is what this hub promises unless the fleet \
                 produces more than the {budget} budget in less than that — it just did, \
                 and the budget is what the disk enforces. Raise --output-budget, or \
                 expect this again.",
                swept.inside_the_floor,
            );
        }
        // **Said once per episode, not once per sweep** (D151). A hub that
        // is over budget with nothing it may remove is triggered again by
        // every file that lands, and a line each time would be the
        // per-frame-line defect in a new place. The flag is cleared the
        // moment a sweep comes back inside the budget, so a second episode
        // is a second line — which is right, because it is news again.
        let over = swept.held > self.budget.bytes;
        let mut live = self.lock()?;
        if over && !live.said_over_budget {
            live.said_over_budget = true;
            eprintln!(
                "herd-hub: {} of job output is held against a {budget} budget and nothing \
                 more may be removed: {} of it belongs to collections that are still \
                 arriving or whose records will not read, and a transfer in flight is \
                 never taken. The volume will stay over budget until those finish.",
                herd_proto::show_bytes(swept.held),
                herd_proto::show_bytes(swept.could_not_free),
            );
        } else if !over {
            live.said_over_budget = false;
        }
        Ok(())
    }

    fn collection_state(&self, id: &JobId, record: &JobRecord) -> crate::job::ChunkLanded {
        if self.collection_is_whole(id, record) {
            crate::job::ChunkLanded::Complete
        } else {
            crate::job::ChunkLanded::More
        }
    }

    fn readings_path(&self) -> PathBuf {
        self.root.join(READINGS_DIR).join(READINGS_FILE)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_DIR).join(INDEX_FILE)
    }

    fn recover(&self) -> Result<()> {
        let jobs_dir = self.jobs_dir();
        let names =
            durable::names_in(&jobs_dir).map_err(|error| failed("listing", &jobs_dir, &error))?;

        let mut sequences = Vec::with_capacity(names.len());
        for name in &names {
            match sequence_of(name) {
                Some(sequence) => sequences.push(sequence),
                // Not ours, so not a job. Said out loud, never deleted: this
                // directory belongs to its owner, not to Herd.
                None => eprintln!(
                    "herd-hub: ignoring {}, which is not a job directory",
                    jobs_dir.join(name).display()
                ),
            }
        }
        sequences.sort_unstable();
        let jobs_accounted = sequences.len() as u64;
        let next_sequence = sequences.last().copied().unwrap_or(0) + 1;

        // With no job directories there is nothing to read and nothing to
        // rebuild, so a brand new state root says nothing about its index.
        let trusted = if sequences.is_empty() {
            Some(Vec::new())
        } else {
            self.usable_index(jobs_accounted, &sequences)
        };
        let to_read: Vec<u64> = match trusted {
            Some(pending) => pending,
            None => sequences.clone(),
        };

        let mut pending = BTreeSet::new();
        let mut open_work: HashMap<String, u64> = HashMap::new();
        let mut interrupted = Vec::new();
        let mut quarantined = BTreeMap::new();
        for sequence in to_read {
            let id = id_of(sequence);
            match self.read_stored(&id)? {
                Stored::Read(record) => {
                    if !record.state.is_terminal() {
                        pending.insert(sequence);
                        // The deduplication link, rebuilt from the record
                        // itself (E-05, D69). Two open jobs cannot share a
                        // key — `submit` is the only minting path and it
                        // joins instead — but a hand-edited state root
                        // could, and then the *older* job is the one a
                        // submission joins, because that is the one the
                        // first submission was told about.
                        open_work
                            .entry(record.dedup_key())
                            .and_modify(|first| *first = (*first).min(sequence))
                            .or_insert(sequence);
                    }
                    // An attempt was under way when the hub that ran before
                    // this one stopped. It is named on the record below, never
                    // left reading `running` for ever (D17).
                    if matches!(record.state, JobState::Assigned | JobState::Running) {
                        interrupted.push((record.id, record.state.name()));
                    }
                }
                // A directory with no record is a mint that was interrupted
                // between claiming the id and writing the record: there is no
                // job to recover, and the id stays spent.
                Stored::Missing => eprintln!(
                    "herd-hub: {} holds no {RECORD_FILE}, so no job was ever recorded there",
                    self.job_dir(&id).display()
                ),
                // Readable and not actionable (D52). Out of the queue, so
                // nothing dispatches it; on this list and on the hub's log, so
                // nobody has to wonder where it went. The record is left
                // exactly as it is: it is not Herd's to rewrite.
                Stored::Quarantined(why) => {
                    eprintln!(
                        "herd-hub: {id} is QUARANTINED and will not be \
                         dispatched: {why}. Its record is untouched; the hub \
                         is running"
                    );
                    quarantined.insert(id, why);
                }
            }
        }

        {
            let mut live = self.lock()?;
            live.next_sequence = next_sequence;
            live.jobs_accounted = jobs_accounted;
            live.pending = pending;
            live.open_work = open_work;
            live.quarantined = quarantined;
            self.write_index(&live)?;
        }
        // After the index, and with the lock given back: naming an
        // interruption is an ordinary transition, recorded the ordinary way.
        for (id, was) in interrupted {
            let reason = format!(
                "the hub stopped while this job was {was}; the runner has not \
                 reported how it ended"
            );
            self.record_state(
                &id,
                JobState::Interrupted {
                    reason: reason.clone(),
                },
            )?;
            eprintln!("herd-hub: {id} is unfinished: {reason}");
        }
        Ok(())
    }

    /// The pending sequences the index can be believed about, or `None` if the
    /// hub must read every record instead. Every check here answers one
    /// question: could this index be hiding a job?
    fn usable_index(&self, jobs_accounted: u64, sequences: &[u64]) -> Option<Vec<u64>> {
        let path = self.index_path();
        let rebuild = |why: String| -> Option<Vec<u64>> {
            eprintln!(
                "herd-hub: rebuilding the index at {} from the job directories: {why}",
                path.display()
            );
            None
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return rebuild("it is not there".to_owned())
            }
            Err(error) => return rebuild(format!("it cannot be read: {error}")),
        };
        let index: IndexFile = match serde_json::from_str(&text) {
            Ok(index) => index,
            Err(error) => return rebuild(format!("it cannot be read: {error}")),
        };
        if index.index_version != INDEX_VERSION {
            return rebuild(format!(
                "it has index version {}, and this hub writes version {INDEX_VERSION}",
                index.index_version
            ));
        }
        // The count is what catches an index that has fallen behind the
        // directories — a job minted just before the hub died, a job directory
        // removed by hand — without reading a single record.
        if index.jobs_accounted != jobs_accounted {
            return rebuild(format!(
                "it accounts for {} job directories and {jobs_accounted} are there",
                index.jobs_accounted
            ));
        }
        let mut pending = Vec::with_capacity(index.pending.len());
        let mut listed = BTreeSet::new();
        for id in &index.pending {
            match sequence_of(id.as_str()) {
                Some(sequence) if sequences.binary_search(&sequence).is_ok() => {
                    pending.push(sequence);
                    listed.insert(sequence);
                }
                _ => return rebuild(format!("it lists {id}, which is not a job here")),
            }
        }
        // The last way an index can hide a job, and the one no count catches:
        // the right number of directories, and a `pending` list that is short.
        // Such an index would make an unfinished job vanish — out of the
        // queue, never dispatched, never asked about again — so every
        // directory it leaves out must hold a record that is over.
        //
        // Answering that costs a read of the records the index omits, which is
        // most of the history. That is the price of the rule: this index is
        // checked, never believed, and "could it be hiding a job?" is not a
        // question a file can answer about itself. Only a hand-edit can get
        // here today, which is exactly the case the rule exists for.
        for sequence in sequences.iter().copied().filter(|s| !listed.contains(s)) {
            let id = id_of(sequence);
            match self.read_stored(&id) {
                Ok(Stored::Read(record)) if !record.state.is_terminal() => {
                    return rebuild(format!(
                        "it leaves out {id}, whose record is {} and so not over",
                        describe(&record.state)
                    ))
                }
                // A quarantined record hides no job — nothing will dispatch
                // it — but it has to be *named*, at every start, and only the
                // full read names it (D52). So the index is set aside for it:
                // the cost is reading every record on a state root that holds
                // one, which is the price of a job nobody can act on never
                // going quiet.
                Ok(Stored::Quarantined(why)) => {
                    return rebuild(format!("{id} is quarantined: {why}"))
                }
                // No record at all is a mint that never finished, which the
                // full read reports on its own.
                Ok(_) => {}
                // A record this hub cannot read is not a record it may call
                // terminal. Rebuilding reads it again and refuses out loud.
                Err(error) => return rebuild(format!("{id} cannot be read: {error}")),
            }
        }
        Some(pending)
    }

    fn write_index(&self, live: &Live) -> Result<()> {
        let index = IndexFile {
            index_version: INDEX_VERSION,
            jobs_accounted: live.jobs_accounted,
            pending: live.pending.iter().copied().map(id_of).collect(),
        };
        write_json(&self.index_path(), &index)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Live>> {
        self.live
            .lock()
            .map_err(|_| StoreError::new("the job store lock is poisoned"))
    }

    /// Take the store's lock and the job's record together, refusing every
    /// write to a job that is already over.
    ///
    /// **The one gate every writer in this store goes through**, and the
    /// reason it is one gate: `record_state` had a terminal guard of its own
    /// and `record_metrics` did not, so a late frame replaced a finished
    /// job's measurements with figures nobody measured while its outcome
    /// stood, and `append_log` put a paragraph and a bogus gap marker into a
    /// log that was closed and fsynced. A rule enforced once per function is
    /// a rule that is missed by the next function; enforced here, a writer
    /// cannot even read the record it needs without passing it.
    ///
    /// What is terminal is **the job**, never an attempt. A first attempt
    /// ending is how a requeue begins (E-21, D39), and the record it ends on
    /// is `queued` a moment later — so the thing tested is `state`, which is
    /// the only thing that is final.
    ///
    /// The refusal is an error and not a silent no-op on purpose: the caller
    /// is writing something it believes in, and it has to learn that the
    /// record would not take it.
    fn open_for_writing(&self, id: &JobId) -> Result<(std::sync::MutexGuard<'_, Live>, JobRecord)> {
        match self.open_unless_over(id)? {
            Writable::Live(live, record) => Ok((live, *record)),
            Writable::Over(state) => Err(StoreError::new(format!(
                "{id} is over ({state}) and a record that has reached its end is never \
                 written to again"
            ))),
        }
    }

    /// The gate itself: the lock and the record, or word that the job is over.
    ///
    /// The terminal test lives here and nowhere else, which is the whole
    /// point of [`Self::open_for_writing`]. It is separate from it only
    /// because the two writers that pass through want different things said
    /// about the same fact. A frame about a job that is over is a *mistake*,
    /// and is refused by name. A dispatch pass that decides a job must wait,
    /// a moment after that job ended, has nothing to write and nothing to
    /// complain about — and that no-op is what closes the race
    /// [`JobStore::record_waiting`] exists to close. Neither of them writes to
    /// a record that is over, which is the rule.
    fn open_unless_over(&self, id: &JobId) -> Result<Writable<'_>> {
        let live = self.lock()?;
        let Some(record) = self.read_record(id)? else {
            return Err(StoreError::new(format!("no such job: {id}")));
        };
        if record.state.is_terminal() {
            return Ok(Writable::Over(describe(&record.state)));
        }
        Ok(Writable::Live(live, Box::new(record)))
    }

    /// Read a record, or say what else it was.
    ///
    /// The labels are read permissively first and judged by today's rules
    /// second (D52). A set the rules reject quarantines the job rather than
    /// failing the read: the alternative is a hub that will not start because
    /// of one record it minted itself under rules that have since changed.
    /// Validation stays strict where a requester is listening — at
    /// submission, through [`LabelSet`] — and that is the only place it can
    /// do any good.
    fn read_stored(&self, id: &JobId) -> Result<Stored> {
        let path = self.job_dir(id).join(RECORD_FILE);
        let Some(text) = read_optional(&path)? else {
            return Ok(Stored::Missing);
        };
        let probe: VersionProbe =
            serde_json::from_str(&text).map_err(|error| failed("reading", &path, &error))?;
        if probe.record_version != RECORD_VERSION {
            return Err(StoreError::new(format!(
                "{} has record version {}, and this hub reads version {RECORD_VERSION}",
                path.display(),
                probe.record_version
            )));
        }
        let durable: DurableLabels =
            serde_json::from_str(&text).map_err(|error| failed("reading", &path, &error))?;
        if let Err(why) = LabelSet::new(durable.labels.clone()) {
            return Ok(Stored::Quarantined(format!(
                "its labels {:?} are not labels this hub accepts: {why}",
                durable.labels
            )));
        }
        // The labels just passed the very check `LabelSet` makes when it
        // deserialises, so reading the whole record cannot fail on them.
        let file: RecordFile =
            serde_json::from_str(&text).map_err(|error| failed("reading", &path, &error))?;
        Ok(Stored::Read(Box::new(file.record)))
    }

    /// The record, for a caller that is about to act on it.
    ///
    /// A quarantined job is an error here, naming it and why: everything that
    /// reads a record through this is about to write one, and a hub that
    /// quietly reported "no such job" would make a job that is plainly on the
    /// disk invisible. Recovery, which must *skip* such a job rather than
    /// fail, reads [`Self::read_stored`] instead.
    fn read_record(&self, id: &JobId) -> Result<Option<JobRecord>> {
        match self.read_stored(id)? {
            Stored::Missing => Ok(None),
            Stored::Read(record) => Ok(Some(*record)),
            Stored::Quarantined(why) => Err(StoreError::new(format!(
                "{id} is quarantined and this hub will not act on it: {why}"
            ))),
        }
    }

    fn write_record(&self, record: &JobRecord) -> Result<()> {
        let path = self.job_dir(&record.id).join(RECORD_FILE);
        write_json(
            &path,
            &RecordFile {
                record_version: RECORD_VERSION,
                record: record.clone(),
            },
        )
    }

    /// How many chunks of this job's log the hub has written, which is the
    /// number the next chunk should carry.
    ///
    /// `None` means the hub has nothing to compare with — `log.seq` is gone or
    /// unreadable — and then a gap cannot be claimed, because a marker naming
    /// chunks that are in fact there would be its own kind of lie. The number
    /// is read from the disk once per job per hub, so that a hub which has
    /// just started on a state root still knows where the log it inherited got
    /// to, and is carried in memory from then on.
    /// It also carries how many *bytes* of the log those chunks are, and that
    /// second number exists for one failure: the hub appends a chunk's bytes
    /// and then counts it, so a hub that stops between the two leaves a log
    /// holding a chunk it never counted. The runner is holding that chunk
    /// against an acknowledgement it never got (D36) and sends it again, and
    /// without the byte count the hub would append it twice — a log with a
    /// paragraph written twice, which is as false as one with a paragraph
    /// missing. So the first time a hub looks at a job's log it compares the
    /// file's length with the length it counted, and gives back the bytes it
    /// never counted. They are not lost: they are the chunk that is about to
    /// arrive again, and if it never does, what is missing is named (D29).
    fn log_at(
        &self,
        live: &mut std::sync::MutexGuard<'_, Live>,
        id: &JobId,
    ) -> Result<Option<LogAt>> {
        if let Some(at) = live.log_chunks.get(id) {
            return Ok(Some(*at));
        }
        let path = self.job_dir(id).join(LOG_SEQ_FILE);
        let Some(text) = read_optional(&path)? else {
            eprintln!(
                "herd-hub: {} is not there, so this hub cannot tell whether chunks of \
                 {id}'s log are missing",
                path.display()
            );
            return Ok(None);
        };
        let mut numbers = text.split_whitespace();
        let chunks = match numbers.next().map(str::parse::<u64>) {
            Some(Ok(chunks)) => chunks,
            other => {
                eprintln!(
                    "herd-hub: {} does not hold a number ({other:?}), so this hub cannot tell \
                     whether chunks of {id}'s log are missing",
                    path.display()
                );
                return Ok(None);
            }
        };
        let log = self.job_dir(id).join(LOG_FILE);
        let length = fs::metadata(&log).map(|at| at.len()).unwrap_or(0);
        let counted = numbers.next().and_then(|bytes| bytes.parse::<u64>().ok());
        let bytes = match counted {
            Some(counted) if length > counted => {
                eprintln!(
                    "herd-hub: {} holds {length} bytes and this hub counted {counted}; the \
                     {} uncounted byte(s) belong to a chunk the hub never acknowledged and \
                     are dropped, so that the runner sending it again does not write it \
                     twice",
                    log.display(),
                    length - counted
                );
                durable::truncate(&log, counted)
                    .map_err(|error| failed("truncating", &log, &error))?;
                counted
            }
            // Either the count agrees with the file, or there is no count to
            // compare with. A file *shorter* than the count is a log that was
            // replaced under the hub, and nothing good comes of guessing: the
            // file is what it is, and the chunk count stands.
            Some(counted) => counted.min(length),
            None => length,
        };
        let at = LogAt { chunks, bytes };
        live.log_chunks.insert(id.clone(), at);
        Ok(Some(at))
    }

    /// Write those two numbers down, in place and in a fixed width.
    fn write_log_at(&self, id: &JobId, at: LogAt) -> Result<()> {
        let path = self.job_dir(id).join(LOG_SEQ_FILE);
        let line = format!(
            "{:0>width$} {:0>width$}\n",
            at.chunks,
            at.bytes,
            width = LOG_SEQ_DIGITS
        );
        durable::write_fixed_in_place(&path, line.as_bytes())
            .map_err(|error| failed("writing", &path, &error))
    }

    fn append_event(&self, id: &JobId, event: &JobEvent) -> Result<()> {
        let path = self.job_dir(id).join(EVENTS_FILE);
        let line = serde_json::to_string(&EventLine {
            record_version: RECORD_VERSION,
            event: event.clone(),
        })
        .map_err(|error| failed("writing", &path, &error))?;
        durable::append_line(&path, &line).map_err(|error| failed("appending to", &path, &error))
    }

    /// One transition, with the lock already held.
    fn move_to(
        &self,
        live: &mut std::sync::MutexGuard<'_, Live>,
        id: &JobId,
        state: JobState,
    ) -> Result<()> {
        let Some(mut record) = self.read_record(id)? else {
            return Err(StoreError::new(format!("no such job: {id}")));
        };
        match transition(&record.state, &state) {
            Transition::Allowed => {}
            // Nothing new said: a second restart that still cannot see an
            // attempt, a job queued again where it already was. Not an
            // ending reported twice — a record that is over refuses every
            // writer before this is reached, and the runner redelivering a
            // verdict whose acknowledgement it never saw is told so by name
            // and drops it (D42).
            Transition::AlreadyThere => return Ok(()),
            Transition::Refused => {
                return Err(StoreError::new(format!(
                    "{id} cannot go from {} to {}",
                    describe(&record.state),
                    describe(&state)
                )))
            }
        }
        let at = Timestamp::now();
        // A job that is no longer queued is no longer waiting for anything:
        // the reason goes, while the `waiting` lines of `events.jsonl` keep
        // the history of why it waited.
        if !matches!(state, JobState::Queued) {
            record.waiting_for = None;
        }
        record.state = state.clone();
        record.updated_at = at;
        // An attempt is over the moment its job is. This was the missing
        // half: the happy path never stamped `ended_at`, so the attempt that
        // produced a verdict stayed "current" for ever on a record that was
        // closed — and a frame naming that attempt and the right runner
        // passed the hub's ownership gate long after the job was done. There
        // is no *reason* to write: `ended` answers "why did this attempt end
        // without a verdict", and this attempt produced one. The moment is
        // the fact, and the transition beside it is the explanation.
        if record.state.is_terminal() {
            if let Some(attempt) = record.attempts.last_mut() {
                if attempt.ended_at.is_none() {
                    attempt.ended_at = Some(at);
                }
            }
        }
        // The record first, so that the moment a transition is readable in
        // `events.jsonl` the record agrees with it.
        self.write_record(&record)?;
        self.append_event(id, &JobEvent::transition(at, state))?;

        if record.state.is_terminal() {
            let log = self.job_dir(id).join(LOG_FILE);
            if log.exists() {
                durable::sync_file(&log).map_err(|error| failed("syncing", &log, &error))?;
            }
            if let Some(sequence) = sequence_of(id.as_str()) {
                live.pending.remove(&sequence);
                // The job is over, so it is no longer the job a second
                // identical submission joins: the next one mints and runs
                // again (E-05, D70). Removed only if the key still points at
                // *this* job, so that nothing here can unlink a different
                // job's work.
                let key = record.dedup_key();
                if live.open_work.get(&key) == Some(&sequence) {
                    live.open_work.remove(&key);
                }
            }
            // Nothing more will be appended to this log, so the hub stops
            // carrying its chunk count. `log.seq` keeps it on the disk.
            live.log_chunks.remove(id);
            self.write_index(live)?;
        }
        Ok(())
    }
    /// Mint a job for a submission nothing else is already doing.
    ///
    /// `key` is the submission's deduplication key, computed by the caller
    /// before the lock was taken: it is recorded in `open_work` under the
    /// same lock that claims the id, so there is no moment at which a job
    /// exists and is not joinable.
    fn mint(&self, new: NewJob, key: String) -> Result<JobRecord> {
        let NewJob {
            command,
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            deadline,
            after,
            requeue_exit_code,
            declaration,
            timeout_s,
            reading,
        } = new;
        let mut live = self.lock()?;
        // `mkdir` is what claims an id: the directories, not the index, decide
        // which ids are taken, so a wrong hint costs one extra attempt.
        let id = loop {
            let sequence = live.next_sequence;
            live.next_sequence += 1;
            let id = id_of(sequence);
            let dir = self.job_dir(&id);
            match durable::create_dir_exclusive(&dir) {
                Ok(()) => break id,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    eprintln!("herd-hub: {} is already taken", dir.display());
                }
                Err(error) => return Err(failed("creating", &dir, &error)),
            }
        };
        let kept = self.job_dir(&id).join(KEPT_FILES_DIR);
        durable::create_dir(&kept).map_err(|error| failed("creating", &kept, &error))?;
        // The log exists from the job's first moment, even if the job never
        // writes a byte: a consumer reading the record of a silent job must
        // find an empty file, not ENOENT (D22).
        let log = self.job_dir(&id).join(LOG_FILE);
        durable::touch(&log).map_err(|error| failed("creating", &log, &error))?;
        // And it is counted from its first moment too: "no chunk written yet"
        // has to be a number the hub can compare against, so that a first
        // chunk numbered 3 is seen for what it is — chunks 0 to 2 gone — and
        // not mistaken for a hub that has nothing to compare with.
        self.write_log_at(
            &id,
            LogAt {
                chunks: 0,
                bytes: 0,
            },
        )?;

        let at = Timestamp::now();
        let record = JobRecord {
            id: id.clone(),
            command,
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            deadline,
            after,
            requeue_exit_code,
            declaration,
            timeout_s,
            reading,
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at: at,
            updated_at: at,
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
            outputs: Default::default(),
        };
        self.write_record(&record)?;
        self.append_event(&id, &JobEvent::transition(at, JobState::Queued))?;
        // The index is written after the record it describes, always: an index
        // that has fallen behind is detected and rebuilt, while an index that
        // ran ahead would claim a job that is not there.
        let sequence = sequence_of(id.as_str()).expect("an id this store minted");
        live.pending.insert(sequence);
        // And this job is now the one an identical submission joins, under
        // the same lock that claimed its id (E-05).
        live.open_work.insert(key, sequence);
        live.jobs_accounted += 1;
        self.write_index(&live)?;
        Ok(record)
    }

    /// A second submission of work an open job is already doing joins it
    /// (E-05).
    ///
    /// The joiner gets no job of its own: it gets *this* job's id, so both
    /// callers hold one id and will be given one result out of one
    /// execution. What it may change is the level — "the merged job takes the
    /// higher priority" — and that is a raise like any other, through the one
    /// write door, recorded with its reason.
    fn join(&self, id: &JobId, asked_for: Priority) -> Result<Option<Submission>> {
        // One lock for the whole join, held across both writes. The *test*
        // that the job is still open and the writes that join it have to be
        // one step: a job that ended in between would otherwise answer this
        // submission with an error — "that record is over" — where the right
        // answer is a job of its own (D70). `None` is that answer, and
        // `submit` mints.
        let Writable::Live(_live, record) = self.open_unless_over(id)? else {
            return Ok(None);
        };
        let mut record = *record;
        let at = Timestamp::now();
        // The raise first, so that the record the caller is handed already
        // carries the level this submission bought it (E-05).
        let raise = raise_in_place(&mut record, asked_for, at);
        record.submissions += 1;
        record.updated_at = at;
        self.write_record(&record)?;
        if let Raise::Raised { from, to } = raise {
            self.append_event(
                id,
                &JobEvent::priority_raised(
                    at,
                    from,
                    to,
                    "a second identical submission asked for this level".to_owned(),
                ),
            )?;
        }
        self.append_event(id, &JobEvent::joined(at, record.submissions, asked_for))?;
        Ok(Some(Submission::Joined {
            record,
            raised: match raise {
                Raise::Raised { to, .. } => Some(to),
                Raise::AlreadyThere(_) | Raise::NotARaise { .. } => None,
            },
        }))
    }

    /// One job's level raised, with the sentence that says who raised it.
    ///
    /// Both callers of a raise come through here — the operator's
    /// [`JobStore::raise_priority`] and the join above — so that there is one
    /// place where a level moves, and it is behind the write door that
    /// refuses a job that is over (D54).
    fn raise_priority_because(&self, id: &JobId, to: Priority, why: &str) -> Result<Raise> {
        let (_live, mut record) = self.open_for_writing(id)?;
        let at = Timestamp::now();
        let raise = raise_in_place(&mut record, to, at);
        let Raise::Raised { from, to } = raise else {
            return Ok(raise);
        };
        self.write_record(&record)?;
        self.append_event(id, &JobEvent::priority_raised(at, from, to, why.to_owned()))?;
        Ok(raise)
    }
}

impl JobStore for JobDirStore {
    fn state_root_id(&self) -> &str {
        JobDirStore::state_root_id(self)
    }

    fn approve_runner(&self, name: &str) -> Result<String> {
        self.roster.approve(name)
    }

    fn remove_runner(&self, name: &str) -> Result<bool> {
        self.roster.remove(name)
    }

    fn approved_runners(&self) -> Result<Vec<crate::roster::Approval>> {
        Ok(self.roster.approvals())
    }

    fn admit_runner(&self, name: &str, token: Option<&str>) -> crate::roster::Admission {
        self.roster.admit(name, token)
    }

    fn approved_runner_count(&self) -> usize {
        self.roster.len()
    }

    /// Mint a job, or join the open one already doing this work (E-05).
    ///
    /// The whole of it is under the store's one lock, from looking the key up
    /// to writing the index: two agents submitting one build at the same
    /// moment are the case this exists for, and a check followed by a mint
    /// would let both of them through.
    ///
    /// Only a job that is **not over** is joined. There is no window where a
    /// job that has just finished is still joinable: `move_to` takes the key
    /// out under this same lock at the moment the job reaches its end.
    fn submit(&self, new: NewJob) -> Result<Submission> {
        let key = new.dedup_key();
        {
            let live = self.lock()?;
            if let Some(sequence) = live.open_work.get(&key).copied() {
                let id = id_of(sequence);
                // Read, rather than believed: the map is derived, and a
                // record that has somehow gone terminal or unreadable under
                // it must not be joined. `None` or a job that is over means
                // the map is stale, and the submission mints below.
                if let Ok(Some(record)) = self.read_record(&id) {
                    if !record.state.is_terminal() {
                        drop(live);
                        // `None` means the job reached its end between that
                        // reading and the join's own, which takes the lock
                        // again: the submission is its own job then, so it
                        // falls through to the mint below (D70).
                        if let Some(joined) = self.join(&id, new.priority)? {
                            return Ok(joined);
                        }
                    }
                }
            }
        }
        self.mint(new, key).map(Submission::Minted)
    }

    fn raise_priority(&self, id: &JobId, to: Priority) -> Result<Raise> {
        // Through the one write door, so a job that is over is refused by
        // name before anything is read off it (D54). An operator raising a
        // job that ended a second ago is told so, and nothing is written.
        self.raise_priority_because(id, to, "an operator raised it")
    }

    /// Write where this job's requirements come from, and what they are once
    /// a reading has said (D74).
    ///
    /// Through the one write door, so a job that is over refuses it by name:
    /// a reading that answers after the job it was read for was cancelled has
    /// nothing to write, and the hub is told rather than quietly writing a
    /// requirement onto a closed record. The answer is also kept in the
    /// store's cache by its own key — that is [`JobStore::remember_reading`],
    /// a separate call, because a *cached* answer landing on a record must
    /// not write the cache again.
    fn record_reading(&self, id: &JobId, reading: Reading) -> Result<()> {
        let (_live, mut record) = self.open_for_writing(id)?;
        let at = Timestamp::now();
        record.reading = Some(reading);
        record.updated_at = at;
        self.write_record(&record)
    }

    /// What a reading of these three answered, out of the cache this store
    /// holds in memory and on its disk (D74).
    fn reading_answer(&self, key: &str) -> Result<Option<Requirements>> {
        Ok(self.lock()?.readings.get(key).cloned())
    }

    /// Keep an answer under its key, in memory and on the disk, so that the
    /// next submission of those three pays no reading (D74).
    ///
    /// Idempotent: an answer already under this key is left alone. Two
    /// readings of one key can only ever have happened because the first had
    /// not finished when the second was minted, and the first answer is the
    /// one every job behind it was already told about.
    fn remember_reading(&self, key: &str, answer: &Requirements) -> Result<()> {
        let mut live = self.lock()?;
        if live.readings.contains_key(key) {
            return Ok(());
        }
        live.readings.insert(key.to_owned(), answer.clone());
        write_json(
            &self.readings_path(),
            &ReadingsFile {
                readings_version: READINGS_VERSION,
                answers: live.readings.clone(),
            },
        )
    }

    /// Write what the runner measured, and nothing else about the job.
    ///
    /// Separate from the transition because the figures are the runner's and
    /// the state is the hub's, and because the hub writes these *before* it
    /// writes `finished`: a reader that sees the terminal state is entitled to
    /// find the measurements beside it (E-15). Fsynced like every other write
    /// here, so an acknowledgement that follows it is not a lie (D19).
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> Result<()> {
        let (_live, mut record) = self.open_for_writing(id)?;
        record.metrics = metrics;
        self.write_record(&record)
    }

    /// The files first, under the same discipline as everything else here
    /// (D8: temporary, fsync, rename, fsync the directory), then the summary
    /// on the record.
    ///
    /// That order, and not the other: the record names a file only once the
    /// file is on the disk, so a reader of `job.json` never finds a path that
    /// is not there. The reverse would publish a promise before its contents.
    fn record_outputs(&self, id: &JobId, outputs: &herd_proto::JobOutputs) -> Result<()> {
        // The manifest and the inline bytes, in one scope, so that the lock
        // is given back before the sweeper is told about the bytes: it takes
        // the lock itself, and it must never be able to run against a record
        // this call has not finished writing (D152).
        let landed = {
            let (_live, mut record) = self.open_for_writing(id)?;
            let kept = self.job_dir(id).join(KEPT_FILES_DIR);
            let mut landed = 0u64;
            let mut summary = crate::job::OutputSummary {
                files: Vec::new(),
                refused: outputs.refused.clone(),
                skipped: outputs.skipped.clone(),
                streamed: outputs.streamed,
                // A verdict being recorded is a collection that has not been
                // fetched and has not been swept, whatever was on the record
                // before: these two are the sweeper's, and the sweeper has not
                // seen this job yet.
                first_fetched_at: None,
                removed: None,
            };
            for file in &outputs.files {
                // Asked before the string is joined onto anything. A walk on a
                // runner cannot produce `..`, but this is not a walk — it is a
                // path a peer sent — and the hub must never find that out by
                // writing outside the job's own directory (D131).
                let relative = match file.relative_path() {
                    Ok(relative) => relative,
                    Err(why) => {
                        summary
                            .skipped
                            .push(format!("a path this hub will not write: {why}"));
                        continue;
                    }
                };
                // The bytes on this frame have to be the file the manifest beside
                // them describes. A length and a digest that nobody compares are
                // worth nothing, and the comparison is cheap next to the write.
                if let Some(why) = file.disagreement() {
                    summary
                        .skipped
                        .push(format!("a frame this hub will not believe: {why}"));
                    continue;
                }
                // **The manifest first, the bytes if they are here.** A streamed
                // collection's verdict carries every path, length and digest and
                // none of the data: the entry is written down now, so a record is
                // complete about what the job produced from the moment the
                // verdict lands, and the bytes fill in behind it (D148). What a
                // reader of `job.json` must never find is a path that is not
                // there *and* no way to tell why — which is why the entry and
                // the file's absence are both readable rather than one being
                // inferred from the other.
                summary.files.push(crate::job::KeptFile {
                    path: file.path.clone(),
                    bytes: file.len,
                    sha256: file.sha256.clone(),
                });
                let Some(data) = &file.data else {
                    continue;
                };
                let path = kept.join(&relative);
                if let Some(parent) = path.parent() {
                    durable::create_dir_all(parent)
                        .map_err(|error| failed("creating", parent, &error))?;
                }
                durable::write_file(&path, data)
                    .map_err(|error| failed("writing", &path, &error))?;
                landed += data.len() as u64;
            }
            summary.skipped.sort();
            record.outputs = summary;
            self.write_record(&record)?;
            landed
        };
        self.held_more(landed)?;
        Ok(())
    }

    fn job_outputs(&self, id: &JobId, with_bytes: bool) -> Result<herd_proto::JobOutputs> {
        let Some(record) = JobStore::job(self, id)? else {
            return Ok(herd_proto::JobOutputs::default());
        };
        let kept = self.job_dir(id).join(KEPT_FILES_DIR);
        let mut outputs = herd_proto::JobOutputs {
            files: Vec::new(),
            refused: record.outputs.refused.clone(),
            skipped: record.outputs.skipped.clone(),
            streamed: record.outputs.streamed,
        };
        for file in &record.outputs.files {
            // The same check on the way out as on the way in: the record is a
            // file an operator may have edited, and a path read off a disk is
            // no more trustworthy than one read off a socket.
            if let Err(why) = herd_proto::check_relative(&file.path) {
                outputs.skipped.push(why);
                continue;
            }
            // **The manifest entry goes out whether the bytes exist or not**,
            // and that is the whole of "the manifest outlives the bytes"
            // (D148). A record that named twelve files and can hand over
            // eight has to say twelve, or the caller reads eight as all of
            // them. Which of them are really there is answered separately,
            // by path, so an absence can carry a reason.
            let entry = herd_proto::OutputFile::announced(
                file.path.clone(),
                file.bytes,
                file.sha256.clone(),
            );
            if !with_bytes {
                outputs.files.push(entry);
                continue;
            }
            let path = kept.join(&file.path);
            match fs::read(&path) {
                Ok(data) => outputs.files.push(herd_proto::OutputFile {
                    data: Some(data),
                    ..entry
                }),
                // Named rather than silently absent: a record that says a
                // file is there and a directory that does not have it is
                // something an operator has to see, and a requester given a
                // short list with no word about it would read it as whole.
                // The entry still goes, so the count stays honest.
                Err(error) => {
                    outputs.skipped.push(format!(
                        "{} is named on this job's record and could not be read back: {error}",
                        file.path
                    ));
                    outputs.files.push(entry);
                }
            }
        }
        Ok(outputs)
    }

    fn write_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<crate::job::ChunkLanded> {
        use crate::job::ChunkLanded;

        let Some(record) = JobStore::job(self, id)? else {
            return Ok(ChunkLanded::Refused(format!("no such job: {id}")));
        };
        // A path no verdict announced is never written. The manifest is the
        // promise and this is the hub keeping to it: without the check, a
        // peer could put any relative path it liked under a job's record,
        // and the record would then describe something the job never left.
        let Some(entry) = record.outputs.files.iter().find(|file| file.path == path) else {
            return Ok(ChunkLanded::Refused(format!(
                "{path} is not on {id}'s manifest, so this hub will not write it: a verdict \
                 announces every file of a collection before a byte of it moves"
            )));
        };
        if let Err(why) = herd_proto::check_relative(path) {
            return Ok(ChunkLanded::Refused(why));
        }
        let kept = self.job_dir(id).join(KEPT_FILES_DIR);
        let parts = self.job_dir(id).join(OUTPUT_PARTS_DIR);
        let whole = kept.join(path);
        let part = parts.join(path);
        // Already here and whole: a chunk for a file the hub finished is a
        // runner that was told so after it had already sent more, which is
        // ordinary on a resumed transfer and not an error.
        if file_length(&whole) == Some(entry.bytes) {
            return Ok(self.collection_state(id, &record));
        }
        let held = file_length(&part).unwrap_or(0);
        // **The offset is checked, not trusted.** A chunk that does not begin
        // where the hub's own bytes end would splice two different reads of
        // one path into a file that is a plausible mixture of both — and the
        // digest would then fail for a reason nothing could explain.
        if offset != held {
            return Ok(ChunkLanded::Refused(format!(
                "{path}: this hub holds {held} byte(s) and the chunk starts at {offset}"
            )));
        }
        if held + data.len() as u64 > entry.bytes {
            return Ok(ChunkLanded::Refused(format!(
                "{path}: the manifest says {} byte(s) and this chunk would make it {}",
                entry.bytes,
                held + data.len() as u64
            )));
        }
        if let Some(parent) = part.parent() {
            durable::create_dir_all(parent).map_err(|error| failed("creating", parent, &error))?;
        }
        durable::append_bytes(&part, data).map_err(|error| failed("writing", &part, &error))?;
        if held + data.len() as u64 != entry.bytes {
            return Ok(ChunkLanded::More);
        }
        // The last byte landed, so the promise is now checkable. **Verified
        // before the rename**, which is what makes `files/` a directory of
        // whole, verified files and nothing else: a reader of it during a
        // transfer sees finished files or no file, never one it could parse
        // and be wrong about (E-53).
        let real = match digest_of(&part) {
            Ok(digest) => digest,
            Err(error) => return Err(failed("reading back", &part, &error)),
        };
        if !entry.sha256.is_empty() && real != entry.sha256 {
            // Thrown away rather than kept: the bytes are not the file the
            // verdict promised, and the runner still has the real ones. The
            // sentence names both digests, because "it did not match" with no
            // figures is the one report nobody can act on.
            let _ = fs::remove_file(&part);
            return Ok(ChunkLanded::Refused(format!(
                "{path}: the manifest says sha256 {} and what arrived hashes to {real}; the \
                 {} byte(s) received have been thrown away rather than written",
                entry.sha256, entry.bytes
            )));
        }
        if let Some(parent) = whole.parent() {
            durable::create_dir_all(parent).map_err(|error| failed("creating", parent, &error))?;
        }
        durable::sync_file(&part).map_err(|error| failed("syncing", &part, &error))?;
        fs::rename(&part, &whole).map_err(|error| failed("renaming", &part, &error))?;
        if let Some(parent) = whole.parent() {
            durable::sync_dir(parent).map_err(|error| failed("syncing", parent, &error))?;
        }
        let landed = self.collection_state(id, &record);
        // **Counted at the rename and not at the chunk**, so the running
        // total and the sweeper's own walk measure the same thing: whole,
        // verified files in `files/`. Part-written bytes are a transfer in
        // flight — real on the disk, outside the budget, and never evicted
        // (see [`crate::job::Budget::bytes`]).
        self.held_more(entry.bytes)?;
        Ok(landed)
    }

    fn output_holdings(&self, id: &JobId) -> Result<Vec<herd_proto::FileHeld>> {
        let Some(record) = JobStore::job(self, id)? else {
            return Ok(Vec::new());
        };
        let kept = self.job_dir(id).join(KEPT_FILES_DIR);
        let parts = self.job_dir(id).join(OUTPUT_PARTS_DIR);
        let mut held = Vec::new();
        for file in &record.outputs.files {
            if herd_proto::check_relative(&file.path).is_err() {
                continue;
            }
            // Whole first, then part-written, then nothing at all — and a
            // path that is in neither place is simply absent from this list,
            // which is how the runner is told to send it from the beginning.
            let bytes = match file_length(&kept.join(&file.path)) {
                Some(bytes) => bytes,
                None => match file_length(&parts.join(&file.path)) {
                    Some(bytes) => bytes,
                    None => continue,
                },
            };
            held.push(herd_proto::FileHeld {
                path: file.path.clone(),
                bytes,
            });
        }
        Ok(held)
    }

    fn read_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        take: usize,
    ) -> Result<Option<Vec<u8>>> {
        use std::io::{Read, Seek, SeekFrom};

        if herd_proto::check_relative(path).is_err() {
            return Ok(None);
        }
        let at = self.job_dir(id).join(KEPT_FILES_DIR).join(path);
        let mut file = match fs::File::open(&at) {
            Ok(file) => file,
            // Not there is an answer, not an error: the record may name a
            // file whose bytes never arrived or are gone, and the caller's
            // job is to say which.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failed("opening", &at, &error)),
        };
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .map_err(|error| failed("seeking in", &at, &error))?;
        }
        let mut data = vec![0u8; take];
        let mut filled = 0usize;
        while filled < take {
            let read = file
                .read(&mut data[filled..])
                .map_err(|error| failed("reading", &at, &error))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        data.truncate(filled);
        Ok(Some(data))
    }

    fn clear_output_parts(&self, id: &JobId) -> Result<()> {
        let parts = self.job_dir(id).join(OUTPUT_PARTS_DIR);
        if !parts.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&parts).map_err(|error| failed("removing", &parts, &error))
    }

    /// Why a queued job is not starting, on its record and in its history.
    ///
    /// Written only when it is news, for two reasons. The hub tries to
    /// dispatch a queued job on every event that could have changed the
    /// answer, so an unconditional write would make `events.jsonl` unreadable
    /// and spin the disk of a hub with nothing to do. And a reason that has
    /// not changed is not something that happened.
    ///
    /// A job that is no longer queued is not waiting for anything, so this is
    /// a no-op on it: that closes the race where a dispatch pass decides a job
    /// must wait a moment after another pass has started it. A job that has
    /// *ended* in that same window is the same no-op and not an error, which
    /// is why this writer takes [`JobDirStore::open_unless_over`] rather than
    /// the refusing door above it — it still passes the one terminal test,
    /// and it still writes nothing to a record that is over.
    fn record_waiting(&self, id: &JobId, reason: &str) -> Result<bool> {
        let Writable::Live(_live, mut record) = self.open_unless_over(id)? else {
            return Ok(false);
        };
        if record.state != JobState::Queued || record.waiting_for.as_deref() == Some(reason) {
            return Ok(false);
        }
        let at = Timestamp::now();
        record.waiting_for = Some(reason.to_owned());
        record.updated_at = at;
        // The record first, so that the moment the line is readable in
        // `events.jsonl` the record agrees with it — the same order every
        // other write here uses.
        self.write_record(&record)?;
        self.append_event(id, &JobEvent::waiting(at, reason.to_owned()))?;
        Ok(true)
    }

    fn record_state(&self, id: &JobId, state: JobState) -> Result<()> {
        let (mut live, _record) = self.open_for_writing(id)?;
        self.move_to(&mut live, id, state)
    }

    /// Record an attempt, number it, and start its output from zero.
    ///
    /// Everything under one lock, because these three facts have to agree: the
    /// record says which attempt is current, the transition says the job is
    /// assigned, and the chunk count says this attempt has produced nothing
    /// yet. A second attempt numbers its chunks from 0 again — it is a new
    /// process with a new counter — so without the reset the hub would read
    /// its first chunk as a gap, and with the reset alone it would overwrite
    /// nothing: the log keeps everything, and a line says where the second
    /// attempt begins.
    fn begin_attempt(
        &self,
        id: &JobId,
        runner: &str,
        why: &str,
        parallelism: Option<u32>,
    ) -> Result<u32> {
        let (mut live, record) = self.open_for_writing(id)?;
        let number = record.attempt_number() + 1;
        let at = Timestamp::now();
        if number > 1 {
            // The log is one file for the whole job, and a reader has to be
            // able to tell the attempt that was lost from the one that ran.
            let path = self.job_dir(id).join(LOG_FILE);
            durable::append_bytes(&path, new_attempt_marker(number).as_bytes())
                .map_err(|error| failed("appending to", &path, &error))?;
        }
        // A second attempt numbers its chunks from zero again, and its output
        // goes on the end of the log the first attempt left.
        let length = fs::metadata(self.job_dir(id).join(LOG_FILE))
            .map(|at| at.len())
            .unwrap_or(0);
        let at_log = LogAt {
            chunks: 0,
            bytes: length,
        };
        live.log_chunks.insert(id.clone(), at_log);
        self.write_log_at(id, at_log)?;
        self.move_to(&mut live, id, JobState::Assigned)?;
        // Read again: `move_to` wrote the record, and this adds to what it
        // wrote rather than to the copy read before it.
        let Some(mut record) = self.read_record(id)? else {
            return Err(StoreError::new(format!("no such job: {id}")));
        };
        record.attempts.push(Attempt {
            number,
            runner: runner.to_owned(),
            started_at: at,
            ended_at: None,
            requeue_code: None,
            ended: None,
            why: why.to_owned(),
            steps: Vec::new(),
        });
        // The figure travels with the job, not only inside the attempt's
        // sentence: whatever reads this job's requirements reads the N it was
        // given (D82).
        record.parallelism = parallelism;
        record.updated_at = at;
        self.write_record(&record)?;
        self.append_event(id, &JobEvent::attempt_started(at, number, runner, why))?;
        Ok(number)
    }

    fn record_step(&self, id: &JobId, attempt: u32, step: StepRun) -> Result<()> {
        let (_live, mut record) = self.open_for_writing(id)?;
        let Some(entry) = record
            .attempts
            .iter_mut()
            .find(|entry| entry.number == attempt)
        else {
            return Err(StoreError::new(format!(
                "{id} has no attempt {attempt} to record a {} step on",
                step.step
            )));
        };
        // Replaced rather than appended twice: the complete list arrives
        // again with the verdict, and four steps are four steps however many
        // times the runner says so. The order is the order they ran in, which
        // is the order the first of each arrived in.
        match entry.steps.iter_mut().find(|held| held.step == step.step) {
            Some(held) => *held = step,
            None => entry.steps.push(step),
        }
        record.updated_at = Timestamp::now();
        self.write_record(&record)
    }

    fn end_attempt(
        &self,
        id: &JobId,
        attempt: u32,
        reason: &str,
        requeue_code: Option<i32>,
    ) -> Result<()> {
        let (_live, mut record) = self.open_for_writing(id)?;
        let Some(entry) = record
            .attempts
            .iter_mut()
            .find(|entry| entry.number == attempt)
        else {
            return Err(StoreError::new(format!(
                "{id} has no attempt {attempt} to end"
            )));
        };
        // The first reason is the true one: the attempt ended once.
        if entry.ended.is_some() {
            return Ok(());
        }
        let at = Timestamp::now();
        entry.ended_at = Some(at);
        entry.ended = Some(reason.to_owned());
        // Which kind of ending this was, which is what keeps D76's budget
        // apart from E-21's: both are counted by reading these attempts back,
        // and an attempt that could not say would be counted by both.
        entry.requeue_code = requeue_code;
        record.updated_at = at;
        self.write_record(&record)?;
        self.append_event(id, &JobEvent::attempt_ended(at, attempt, reason))
    }

    fn chunks_recorded(&self, id: &JobId) -> Result<u64> {
        let mut live = self.lock()?;
        Ok(self.log_at(&mut live, id)?.map_or(0, |at| at.chunks))
    }

    fn name_missing_log(&self, id: &JobId, first_missing: u64, last_missing: u64) -> Result<()> {
        if last_missing < first_missing {
            return Ok(());
        }
        let (mut live, _record) = self.open_for_writing(id)?;
        let path = self.job_dir(id).join(LOG_FILE);
        durable::append_bytes(
            &path,
            log_gap_marker(first_missing, last_missing).as_bytes(),
        )
        .map_err(|error| failed("appending to", &path, &error))?;
        self.append_event(
            id,
            &JobEvent::log_gap(Timestamp::now(), first_missing, last_missing),
        )?;
        eprintln!(
            "herd-hub: {} log chunk(s) of {id} are gone for good (chunks {first_missing} to \
             {last_missing}); the gap is named in the log and on the record",
            last_missing - first_missing + 1
        );
        // Counted as if they had been written, so that whatever comes next
        // follows the marker instead of being read as a second gap.
        let length = fs::metadata(&path).map(|at| at.len()).unwrap_or(0);
        let at_log = LogAt {
            chunks: last_missing.saturating_add(1),
            bytes: length,
        };
        live.log_chunks.insert(id.clone(), at_log);
        self.write_log_at(id, at_log)
    }

    /// Append one chunk, and name whatever is missing before it.
    ///
    /// `stream` is not written down: the log is one file in arrival order
    /// (D29, and the module's own words). `seq` is, because it is the only way
    /// the hub can tell that chunks are gone — a control connection that
    /// dropped under a running job takes with it everything the runner wrote
    /// while there was no hub, and the runner spends those numbers all the
    /// same. A number that is not the one expected is therefore a gap, and it
    /// is named in the log at the place where the bytes are missing, and on
    /// the record. Either side of the marker the log stays byte for byte what
    /// the job wrote.
    ///
    /// A number *below* the one expected is the other half of the same
    /// bookkeeping: the runner holds every chunk until the hub acknowledges
    /// it, so a chunk already written arrives again whenever the link died
    /// between the write and the acknowledgement. Writing it twice would put a
    /// paragraph of the job's output in the log twice, which is as false as
    /// losing it, so it is dropped and said to be dropped.
    fn append_log(
        &self,
        id: &JobId,
        _stream: LogStream,
        seq: u64,
        data: &[u8],
    ) -> Result<LogWrite> {
        let (mut live, _record) = self.open_for_writing(id)?;
        let path = self.job_dir(id).join(LOG_FILE);
        let so_far = self.log_at(&mut live, id)?;
        let expected = so_far.map(|at| at.chunks);
        if expected.is_some_and(|expected| seq < expected) {
            return Ok(LogWrite::AlreadyRecorded);
        }
        let mut bytes = so_far.map_or(0, |at| at.bytes);
        if let Some(expected) = expected.filter(|expected| seq > *expected) {
            let (first, last) = (expected, seq - 1);
            let marker = log_gap_marker(first, last);
            durable::append_bytes(&path, marker.as_bytes())
                .map_err(|error| failed("appending to", &path, &error))?;
            bytes += marker.len() as u64;
            self.append_event(id, &JobEvent::log_gap(Timestamp::now(), first, last))?;
            eprintln!(
                "herd-hub: {} log chunk(s) of {id} never reached its log (chunks {first} to \
                 {last}); the gap is named in the log and on the record",
                last - first + 1
            );
        }
        durable::append_bytes(&path, data)
            .map_err(|error| failed("appending to", &path, &error))?;
        // Counted only now, and only if the bytes went in. A chunk the hub
        // could not write is a chunk that is missing, and the next one must
        // show it missing rather than paper over it.
        let at_log = LogAt {
            chunks: expected.unwrap_or(0).max(seq.saturating_add(1)),
            bytes: bytes + data.len() as u64,
        };
        live.log_chunks.insert(id.clone(), at_log);
        self.write_log_at(id, at_log)?;
        Ok(LogWrite::Recorded)
    }

    fn quarantine_reason(&self, id: &JobId) -> Result<Option<String>> {
        let _live = self.lock()?;
        match self.read_stored(id)? {
            Stored::Quarantined(why) => Ok(Some(why)),
            Stored::Read(_) | Stored::Missing => Ok(None),
        }
    }

    fn job(&self, id: &JobId) -> Result<Option<JobRecord>> {
        let _live = self.lock()?;
        self.read_record(id)
    }

    fn job_log(&self, id: &JobId) -> Result<Option<Vec<u8>>> {
        let _live = self.lock()?;
        if !self.job_dir(id).is_dir() {
            return Ok(None);
        }
        let path = self.job_dir(id).join(LOG_FILE);
        Ok(Some(read_optional_bytes(&path)?.unwrap_or_default()))
    }

    fn job_events(&self, id: &JobId) -> Result<Option<Vec<JobEvent>>> {
        let _live = self.lock()?;
        if !self.job_dir(id).is_dir() {
            return Ok(None);
        }
        let path = self.job_dir(id).join(EVENTS_FILE);
        let Some(text) = read_optional(&path)? else {
            return Ok(Some(Vec::new()));
        };
        let mut events = Vec::new();
        for (number, line) in text.split_inclusive('\n').enumerate() {
            if !line.ends_with('\n') {
                // The last line has no terminator, so it is an append that did
                // not finish. A transition is only a transition once its line
                // is whole; this one is not there yet.
                eprintln!(
                    "herd-hub: {}:{} is not a whole line, so it is not an event yet",
                    path.display(),
                    number + 1
                );
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let probe: VersionProbe = serde_json::from_str(line)
                .map_err(|error| failed_line("reading", &path, number + 1, &error))?;
            if probe.record_version != RECORD_VERSION {
                return Err(StoreError::new(format!(
                    "{}:{} has record version {}, and this hub reads version {RECORD_VERSION}",
                    path.display(),
                    number + 1,
                    probe.record_version
                )));
            }
            let parsed: EventLine = serde_json::from_str(line)
                .map_err(|error| failed_line("reading", &path, number + 1, &error))?;
            events.push(parsed.event);
        }
        Ok(Some(events))
    }

    fn unfinished_jobs(&self) -> Result<Vec<JobRecord>> {
        let live = self.lock()?;
        let mut records = Vec::with_capacity(live.pending.len());
        for sequence in live.pending.iter().copied() {
            // `read_stored`, because the queue is what the hub acts on and a
            // quarantined job is not in it: recovery keeps it out of
            // `pending`, and reading one here would be a hub that cannot even
            // list its queue (D52).
            if let Stored::Read(record) = self.read_stored(&id_of(sequence))? {
                records.push(*record);
            }
        }
        Ok(records)
    }

    fn output_budget(&self) -> crate::job::Budget {
        self.budget
    }

    fn note_outputs_fetched(&self, id: &JobId) -> Result<()> {
        let _live = self.lock()?;
        let Some(mut record) = self.read_record(id)? else {
            return Ok(());
        };
        if record.outputs.files.is_empty() || record.outputs.first_fetched_at.is_some() {
            return Ok(());
        }
        // **Not through `open_for_writing`**, and that is deliberate. The
        // one gate refuses every write to a job that is over, because a late
        // frame about a finished job is a mistake. This is not a frame about
        // the job: it is the hub writing down something that can only ever
        // happen *after* the job is over, about the record's own files and
        // nothing else. The state, the outcome, the metrics and the manifest
        // are untouched here, which is what keeps that gate meaning what it
        // means — the same licence `write_output_chunk` already takes, for
        // the same reason (D148, D152).
        record.outputs.first_fetched_at = Some(Timestamp::now());
        self.write_record(&record)
    }

    fn sweep_outputs(&self) -> Result<crate::job::Sweep> {
        {
            let mut live = self.lock()?;
            if live.sweeping {
                return Ok(crate::job::Sweep::default());
            }
            live.sweeping = true;
        }
        let swept = self.sweep_now();
        if let Ok(mut live) = self.lock() {
            live.sweeping = false;
        }
        swept
    }

    fn collections_owed(&self) -> Result<Vec<crate::job::CollectionOwed>> {
        let mut owed = Vec::new();
        // A scan of the job directories, like the one that rebuilds the queue
        // at startup. It runs when a runner dials, not per job, so the cost
        // is paid once per redial — and the alternative is a list of
        // outstanding transfers kept in memory, which is the thing that can
        // disagree with the disk.
        for name in durable::names_in(&self.jobs_dir())
            .map_err(|error| failed("listing", &self.jobs_dir(), &error))?
        {
            let id = JobId::new(name);
            // A record that will not read is not a collection to chase. It is
            // a louder problem than this, and recovery says so where it
            // belongs (D52); here it is simply skipped.
            let Ok(Some(record)) = JobStore::job(self, &id) else {
                continue;
            };
            if !record.outputs.streamed || record.outputs.files.is_empty() {
                continue;
            }
            // **A collection the sweeper took is not one anybody still owes**
            // (D152). Without this the two mechanisms would fight: an
            // evicted collection fails `collection_is_whole` for exactly the
            // same reason an unfinished one does, so the hub would ask its
            // runner to send the bytes again and sweep them again, for ever
            // — and a requester asking for it would be told to keep waiting
            // for something that is never coming back.
            if record.outputs.removed.is_some() {
                continue;
            }
            // The one definition of complete, shared with every other
            // caller: in `files/`, at the manifest's length, verified.
            // **Not** off the holdings below — a part file at full length is
            // not a finished file, and judging it so told requesters their
            // collection had stopped coming in the window where its digest
            // was still being checked.
            if self.collection_is_whole(&id, &record) {
                continue;
            }
            let held = self.output_holdings(&id)?;
            // Whoever ran the attempt that produced it. No attempt means a
            // record nothing can be asked of, which is not a state a streamed
            // collection can honestly be in — skipped rather than guessed at.
            let Some(attempt) = record.attempts.last() else {
                continue;
            };
            owed.push(crate::job::CollectionOwed {
                job_id: id,
                attempt: attempt.number,
                runner: attempt.runner.clone(),
                held,
            });
        }
        Ok(owed)
    }
}

/// What the log says at the place where chunks are missing.
///
/// One line, on its own, unmistakably the hub speaking rather than the job.
/// It names how many chunks are gone and which, so that a log short of a
/// quarter of its lines is a log that says it is short of a quarter of its
/// lines — the point of the whole exercise. Nothing either side of it is
/// touched, so the bytes before and after are still exactly what the job
/// wrote (D29).
pub fn log_gap_marker(first_missing: u64, last_missing: u64) -> String {
    let chunks = last_missing - first_missing + 1;
    format!(
        "\n[herd-hub] {chunks} log chunk(s) never reached this log and are missing here: \
         chunks {first_missing} to {last_missing} of this job's output. Everything before \
         and after this line is byte for byte what the job wrote.\n"
    )
}

/// What the log says where a second attempt's output begins.
///
/// A requeued job's log is one file holding two runs of the same command
/// (E-21), and a reader who cannot see the seam would read the first
/// attempt's half-finished output as part of the second's. One line, the hub
/// speaking, in the same voice as [`log_gap_marker`].
pub fn new_attempt_marker(attempt: u32) -> String {
    format!(
        "\n[herd-hub] attempt {attempt} of this job starts here. Everything above is an \
         earlier attempt, which ended without a verdict; the record names why.\n"
    )
}

/// What a transition from one state to another is.
enum Transition {
    Allowed,
    /// Already recorded, and recording it again is harmless.
    AlreadyThere,
    Refused,
}

/// The job state machine, in one place.
///
/// A job goes forward only — minted queued, handed to a runner, running, over
/// — with one exception, and it is deliberate: a job whose runner was lost
/// goes back to `queued` for its second and last attempt (E-21). That edge is
/// recorded like any other, so the record shows the job going round once
/// rather than quietly starting again.
fn transition(from: &JobState, to: &JobState) -> Transition {
    use JobState::{Assigned, Finished, Interrupted, Queued, Running};
    match (from, to) {
        (Queued, Assigned | Running | Finished { .. })
        | (Assigned, Running | Finished { .. })
        | (Running, Finished { .. })
        // Requeued for one more attempt. Whether the job is allowed a second
        // attempt is the hub's policy, not the store's; what the store
        // enforces is that the move is written down.
        | (Assigned | Running | Interrupted { .. }, Queued)
        // An attempt the hub stopped seeing (D17). A queued job is not in it:
        // nothing was under way, so nothing was interrupted.
        | (Assigned | Running, Interrupted { .. })
        // And the way out of it: the runner comes back, and the result it was
        // holding resolves the record exactly as it would have before.
        | (Interrupted { .. }, Running | Finished { .. }) => Transition::Allowed,
        (Finished { outcome: had }, Finished { outcome: got }) if had == got => {
            Transition::AlreadyThere
        }
        // A second restart with the same attempt still missing says nothing
        // new, and a record is not a journal of the hub's own restarts.
        (Interrupted { .. }, Interrupted { .. }) | (Queued, Queued) => Transition::AlreadyThere,
        _ => Transition::Refused,
    }
}

/// A state as a refusal names it: the outcome included, because "finished" on
/// its own does not say which end was already recorded.
fn describe(state: &JobState) -> String {
    match state {
        JobState::Finished { outcome } => format!(
            "finished {}",
            serde_json::to_string(outcome).unwrap_or_else(|_| "unprintable".to_owned())
        ),
        other => other.name().to_owned(),
    }
}

/// A job's id for one sequence number: zero-padded so that `ls` sorts the
/// directory in the order the jobs were minted. Opaque to every peer (D15's
/// `JobId` is a string and nothing may read its shape).
fn id_of(sequence: u64) -> JobId {
    JobId::new(format!("j{sequence:0>width$}", width = ID_DIGITS))
}

/// The sequence number in a job id, if the name is one of ours.
fn sequence_of(name: &str) -> Option<u64> {
    let digits = name.strip_prefix('j')?;
    if digits.len() != ID_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A fresh identity for a state root: random, short, and printable.
///
/// Random because the one thing asked of it is that two state roots never
/// share one — including a root created a second after the one it replaced, on
/// the same path, by the same deployment. A counter or a timestamp would
/// collide exactly there, which is the case this exists for. Sixteen bytes
/// from the kernel, hex, with a prefix so that a reader of a log or a frame
/// knows what kind of thing it is looking at.
///
/// Printable because it travels in a JSON frame and lands in an operator's
/// terminal: the hub's refusal names both roots, and a reader who cannot read
/// the names cannot act on the refusal.
fn mint_a_state_root_id() -> String {
    let mut bytes = [0u8; 16];
    // A kernel that cannot give sixteen random bytes is not a kernel this hub
    // can be sure of, so the fallback is loud rather than silent — and still
    // an identity, because starting without one would make every spooled
    // verdict believable again.
    let filled =
        match rustix::rand::getrandom(&mut bytes[..], rustix::rand::GetRandomFlags::empty()) {
            Ok(filled) => filled,
            Err(error) => {
                eprintln!(
                    "herd-hub: this host would not give random bytes for the state root's \
                 identity ({error}); falling back to the clock and this process's id, \
                 which is weaker"
                );
                0
            }
        };
    if filled < bytes.len() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        bytes[0..8].copy_from_slice(&nanos.to_be_bytes());
        bytes[8..12].copy_from_slice(&std::process::id().to_be_bytes());
    }
    let mut id = String::with_capacity(5 + bytes.len() * 2);
    id.push_str("root-");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

/// Claim a state root for this hub, or refuse naming who holds it.
///
/// Two hubs on one state root are two hubs minting ids from the same
/// directories and two derived indexes disagreeing about the queue — a
/// corruption with no error message, which is the worst kind. The claim is
/// `flock` on `hub.lock`, so it costs nothing to hold and is released by the
/// kernel even when a hub is killed; the file's contents say who is holding
/// it, because a lock that cannot name its holder leaves an operator with
/// nothing to act on (D21).
fn claim(root: &Path) -> Result<fs::File> {
    let path = root.join(LOCK_FILE);
    let mut file = match durable::lock_exclusive(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            let holder = read_optional(&path)
                .ok()
                .flatten()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "another herd-hub, which left no name".to_owned());
            return Err(StoreError::new(format!(
                "the state root {} is already in use by {holder}; \
                 one hub per state root",
                root.display()
            )));
        }
        Err(error) => return Err(failed("locking", &path, &error)),
    };
    let holder = format!(
        "herd-hub, process {} since {}\n",
        std::process::id(),
        Timestamp::now()
    );
    durable::write_in_place(&mut file, holder.as_bytes())
        .map_err(|error| failed("writing", &path, &error))?;
    Ok(file)
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("reading", path, &error)),
    }
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("reading", path, &error)),
    }
}

/// The cache of readings this state root holds, or nothing at all (D74).
///
/// **Never an error.** A cache is derived: a file that does not parse, or that
/// a newer Herd wrote, is a cache this hub starts without — every submission
/// then pays for a reading of its own, which is slow and correct. Said on
/// stderr rather than swallowed, because "the hub is paying for a reading
/// every time" is a thing an operator wants to know.
fn read_the_readings(root: &Path) -> BTreeMap<String, Requirements> {
    let path = root.join(READINGS_DIR).join(READINGS_FILE);
    let text = match read_optional(&path) {
        Ok(Some(text)) => text,
        Ok(None) => return BTreeMap::new(),
        Err(why) => {
            eprintln!(
                "herd-hub: {} could not be read ({why}), so this hub starts with no cached \
                 reading and every submission of a command with a labels_from pays for one \
                 until it is read again",
                path.display()
            );
            return BTreeMap::new();
        }
    };
    match serde_json::from_str::<ReadingsFile>(&text) {
        Ok(file) if file.readings_version == READINGS_VERSION => file.answers,
        Ok(file) => {
            eprintln!(
                "herd-hub: {} carries readings version {} and this hub writes \
                 {READINGS_VERSION}, so its answers are left alone and read again",
                path.display(),
                file.readings_version
            );
            BTreeMap::new()
        }
        Err(why) => {
            eprintln!(
                "herd-hub: {} is not a cache of readings ({why}), so this hub starts with \
                 none and reads them again",
                path.display()
            );
            BTreeMap::new()
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_vec(value).map_err(|error| failed("writing", path, &error))?;
    durable::write_file(path, &text).map_err(|error| failed("writing", path, &error))
}

/// How long a file is, or nothing at all if it is not there.
///
/// Absent and zero are two different answers and this keeps them apart: an
/// empty file is a real file a collection may contain, and reading its
/// absence as "0 bytes held" would tell a runner to resume a file it had
/// never started.
/// Every byte under a directory, counting nothing that is not a regular
/// file.
///
/// Read off the disk rather than off the manifest, because the budget is
/// about the volume and not about what a record claims: a file the record
/// forgot still fills the disk, and a file the record names and the disk
/// does not have fills nothing. A directory that is not there is nought
/// bytes and not an error — most job directories hold no output at all, and
/// that is the common case this walk has to be cheap for.
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

/// Unlink a directory and everything in it, with the path in the error.
///
/// A failure here is a real one and is reported: the bytes are already
/// invisible to every reader, so nothing is *wrong* — but a sweeper that
/// could not actually free the disk has to say so, or the next sweep finds
/// the same figure and the operator finds a full volume with a hub that
/// claimed to be tidying it.
fn remove_tree(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failed("removing", path, &error)),
    }
}

fn file_length(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .filter(|at| at.is_file())
        .map(|at| at.len())
}

/// The sha256 of a file on this hub's disk, read a piece at a time.
///
/// Never `fs::read` then hash: the file may be hundreds of megabytes, and
/// holding it whole to check it would put back the cost the streamed carriage
/// exists to remove.
fn digest_of(path: &Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut rolling = herd_proto::Sha256Rolling::new();
    let mut buffer = vec![0u8; 64 << 10];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(rolling.finish());
        }
        rolling.update(&buffer[..read]);
    }
}

fn failed(what: &str, path: &Path, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::new(format!("{what} {} failed: {error}", path.display()))
}

fn failed_line(what: &str, path: &Path, line: usize, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::new(format!("{what} {}:{line} failed: {error}", path.display()))
}

/// Move one record's level up, in memory, and say what that came to.
///
/// **The one place a level changes.** Both callers go through it — the
/// operator's raise and the raise a joining submission buys (E-05, E-18) —
/// so that "only ever upwards" is one rule in one function rather than two
/// copies that can drift. It writes nothing: the caller holds the store's
/// lock and the write door's verdict, and persists the record and the line
/// that explains it.
fn raise_in_place(record: &mut JobRecord, to: Priority, at: Timestamp) -> Raise {
    let from = record.priority;
    if from == to {
        return Raise::AlreadyThere(from);
    }
    if from.outranks(to) {
        return Raise::NotARaise { from, asked: to };
    }
    record.priority = to;
    record.updated_at = at;
    // A level that changed changes why the job is waiting, and the sentence
    // on the record names the old level. Cleared rather than rewritten here:
    // the next dispatch pass writes the true one, and `events.jsonl` keeps
    // the one that was.
    record.waiting_for = None;
    Raise::Raised { from, to }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobEventKind;
    use crate::job::Mint;
    use herd_proto::{Declaration, Executor, JobOutcome, Step};
    use tempfile::TempDir;

    /// A store in a directory that goes away with the test.
    fn store() -> (TempDir, JobDirStore) {
        make_a_store()
    }

    /// The same thing under a name a local `store` binding cannot shadow, for
    /// the tests that need two stores at once.
    fn make_a_store() -> (TempDir, JobDirStore) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let store = JobDirStore::open(dir.path()).expect("the state root opens");
        (dir, store)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} reads: {e}", path.display()))
    }

    /// The commit a declared submission carries: forty hex, resolved in the
    /// requester's own checkout (D106).
    const A_COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";

    /// A second commit of the same repository, for the submissions that must
    /// **not** be one piece of work (D156).
    const ANOTHER_COMMIT: &str = "3f7d1a90c6e84b25df0a7c13e95b6482da10c7fe";

    /// A declared command as a submission carries it, shaped like the first
    /// consumer's and named like nothing of theirs (E-11).
    fn a_declaration() -> Declaration {
        Declaration {
            command: "a-declared-command".to_owned(),
            commit: A_COMMIT.to_owned(),
            args: vec!["--only".to_owned(), "one".to_owned()],
            executor: Executor {
                up: format!("tool ws up {{job}} {A_COMMIT}"),
                fetch: format!("tool ws run {{job}} -- 'git switch --detach {A_COMMIT}'"),
                run: "tool ws run {job} -- {line}".to_owned(),
                down: "tool ws down {job} --purge".to_owned(),
                env: BTreeMap::from([("RESULTS".to_owned(), "{out}/results.json".to_owned())]),
            },
        }
    }

    fn step(step: Step, duration_ms: u64, exit_code: Option<i32>) -> StepRun {
        StepRun {
            step,
            started_unix_ms: 1_791_878_153_000,
            duration_ms,
            exit_code,
            ended: exit_code
                .is_none()
                .then(|| "stopped at the time cap, 2700 s".to_owned()),
        }
    }

    /// The identity a job id only means anything inside.
    ///
    /// Minted with the root, kept in `store.json`, unchanged by a restart —
    /// and *different* for a root created where one was deleted, which is the
    /// case it exists for: both roots hand out `j0000000001`, and a runner
    /// holding a verdict for one of them must not have it recorded on the
    /// other.
    #[test]
    fn a_state_root_s_identity_is_minted_once_and_dies_with_the_root() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let first = JobDirStore::open(dir.path()).expect("the state root opens");
        let id = first.state_root_id().to_owned();
        assert!(
            !id.is_empty(),
            "a state root has an identity from the start"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&read(&dir.path().join(STORE_FILE)))
                .expect("store.json is json")["state_root_id"]
                .as_str(),
            Some(id.as_str()),
            "and it is on the disk, not only in memory"
        );
        drop(first);

        let again = JobDirStore::open(dir.path()).expect("the same root opens again");
        assert_eq!(
            again.state_root_id(),
            id,
            "a restart is the same state, so it is the same identity"
        );
        drop(again);

        fs::remove_dir_all(dir.path()).expect("the root is deleted");
        let recreated = JobDirStore::open(dir.path()).expect("a root is created in its place");
        assert_ne!(
            recreated.state_root_id(),
            id,
            "a recreated root is another state, and its ids mean other jobs"
        );
    }

    #[test]
    fn a_minted_job_is_queued_and_has_a_fresh_id() {
        let (_dir, store) = store();
        let first = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let second = store
            .create_job(NewJob::new(vec!["false".to_owned()]))
            .expect("minted");
        assert_ne!(first.id, second.id);
        assert_eq!(first.state, JobState::Queued);
        assert_eq!(
            store.job(&first.id).expect("readable"),
            Some(first.clone()),
            "a minted job is readable straight away"
        );
    }

    /// The layout a reader outside Herd depends on (E-53, E-54), pinned.
    #[test]
    fn a_job_is_a_directory_of_plain_files_carrying_their_version() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "true".to_owned(),
            ]))
            .expect("minted");
        store
            .append_log(&job.id, LogStream::Stdout, 0, b"half a line")
            .expect("appended");

        let job_dir = dir.path().join(JOBS_DIR).join(job.id.as_str());
        let mut names = durable::names_in(&job_dir).expect("the job directory lists");
        names.sort();
        assert_eq!(
            names,
            vec!["events.jsonl", "files", "job.json", "log", "log.seq"]
        );
        assert!(job_dir.join(KEPT_FILES_DIR).is_dir(), "room for kept files");

        let record: serde_json::Value =
            serde_json::from_str(&read(&job_dir.join(RECORD_FILE))).expect("job.json is json");
        assert_eq!(record["record_version"], RECORD_VERSION);
        assert_eq!(record["id"], job.id.as_str());
        assert_eq!(record["command"][2], "true");
        assert_eq!(record["state"]["type"], "queued");
        assert!(
            record["submitted_at"]
                .as_str()
                .expect("a timestamp")
                .ends_with('Z'),
            "timestamps are RFC 3339 in UTC: {}",
            record["submitted_at"]
        );
        // The runner's figures (M4) say "not measured" on a job that has not
        // run: a figure nobody took is absent, never a zero (D23, E-15).
        for field in [
            "host_load_milli",
            "peak_memory_bytes",
            "bytes_written",
            "pressure",
        ] {
            assert!(
                record["metrics"][field].is_null(),
                "metrics.{field} should be unset, not zero: {}",
                record["metrics"]
            );
        }

        // The log is the job's own bytes, nothing else: it is read by tools
        // that know nothing of Herd.
        assert_eq!(read(&job_dir.join(LOG_FILE)), "half a line");
        // And the layout says which layout it is.
        let store_file: serde_json::Value =
            serde_json::from_str(&read(&dir.path().join(STORE_FILE))).expect("store.json is json");
        assert_eq!(store_file["layout_version"], LAYOUT_VERSION);
    }

    #[test]
    fn transitions_and_log_chunks_land_on_the_record() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        store
            .record_state(&job.id, JobState::Running)
            .expect("transition");
        store
            .append_log(&job.id, LogStream::Stdout, 0, b"first")
            .expect("append");
        store
            .append_log(&job.id, LogStream::Stderr, 1, b"-second")
            .expect("append");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 3 },
                },
            )
            .expect("transition");

        let record = store.job(&job.id).expect("readable").expect("still there");
        assert_eq!(
            record.state,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 3 }
            }
        );
        assert_eq!(
            store.job_log(&job.id).expect("readable").as_deref(),
            Some(&b"first-second"[..]),
            "both streams interleave in arrival order"
        );
        assert!(
            record.updated_at >= record.submitted_at,
            "the record says when it last changed"
        );
    }

    /// A log that is missing chunks says so, in the log, at the place where
    /// they are missing — and the record says so too.
    ///
    /// This is the mechanism; `killed_hub.rs` drives it through twelve real
    /// `kill -9`s, which is how its absence was found.
    #[test]
    fn chunks_that_never_arrived_leave_a_named_gap_in_the_log() {
        use crate::job::JobEventKind;

        let (dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        store
            .append_log(&job.id, LogStream::Stdout, 0, b"first\n")
            .expect("appended");
        // Chunks 1, 2 and 3 are gone: the link dropped under the running job,
        // and the runner spent those numbers on output no hub ever saw.
        store
            .append_log(&job.id, LogStream::Stdout, 4, b"fifth\n")
            .expect("appended");

        let log = String::from_utf8(
            store
                .job_log(&job.id)
                .expect("readable")
                .expect("there is a log"),
        )
        .expect("this job's output is text");
        assert!(
            log.starts_with("first\n") && log.ends_with("fifth\n"),
            "the bytes either side of the marker are the job's own: {log:?}"
        );
        assert!(
            log.contains("3 log chunk(s) never reached this log"),
            "the log must say how much is missing: {log:?}"
        );
        assert!(
            log.contains("chunks 1 to 3"),
            "and which chunks they were: {log:?}"
        );

        let events = store
            .job_events(&job.id)
            .expect("readable")
            .expect("still there");
        let gap = events
            .iter()
            .find_map(|event| match &event.what {
                JobEventKind::LogGap { .. } => Some(&event.what),
                _ => None,
            })
            .expect("the gap is on the record, not only in the log");
        assert_eq!(
            gap,
            &JobEventKind::LogGap {
                first_missing: 1,
                last_missing: 3
            }
        );

        // And the count is on the disk, so the hub that starts next on this
        // state root still knows where the log it inherited got to — which is
        // the whole of the killed-hub case.
        drop(store);
        let store = JobDirStore::open(dir.path()).expect("the state root opens again");
        store
            .append_log(&job.id, LogStream::Stdout, 7, b"eighth\n")
            .expect("appended");
        let log = String::from_utf8(
            store
                .job_log(&job.id)
                .expect("readable")
                .expect("there is a log"),
        )
        .expect("text");
        assert!(
            log.contains("chunks 5 to 6"),
            "a hub that has just started must still see the gap: {log:?}"
        );
        assert!(log.ends_with("eighth\n"), "{log:?}");
    }

    /// Transitions are present, ordered and timestamped — the order being the
    /// order of the lines, not an ordering of the clock.
    #[test]
    fn every_transition_is_recorded_in_order_with_its_moment() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        for state in [
            JobState::Assigned,
            JobState::Running,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        ] {
            store.record_state(&job.id, state).expect("transition");
        }
        let events = store
            .job_events(&job.id)
            .expect("readable")
            .expect("still there");
        let names: Vec<&str> = events
            .iter()
            .map(|event| event.state().expect("a transition").name())
            .collect();
        assert_eq!(names, ["queued", "assigned", "running", "finished"]);
        assert_eq!(
            events.last().expect("four events").state(),
            Some(&JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 }
            }),
            "the end is recorded whole, outcome included"
        );

        let lines = read(
            &dir.path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(EVENTS_FILE),
        );
        assert_eq!(lines.lines().count(), 4, "one line per transition: {lines}");
        for line in lines.lines() {
            let event: serde_json::Value = serde_json::from_str(line).expect("a line is json");
            assert_eq!(event["record_version"], RECORD_VERSION);
            assert!(
                event["at"].as_str().expect("a moment").contains('T'),
                "{line}"
            );
        }
    }

    /// A line that was being appended when the machine stopped is not an
    /// event: a reader sees the transitions that are whole and no more.
    #[test]
    fn an_unfinished_line_is_not_read_as_a_transition() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(EVENTS_FILE);
        let torn = format!("{}{{\"record_version\":1,\"at\":\"2026", read(&path));
        durable::write_file(&path, torn.as_bytes()).expect("the torn file is in place");

        let events = store
            .job_events(&job.id)
            .expect("readable")
            .expect("still there");
        assert_eq!(events.len(), 1, "only the whole line is an event");
        assert_eq!(events[0].state(), Some(&JobState::Queued));
    }

    #[test]
    fn every_way_a_job_can_end_is_recorded_whole() {
        let (_dir, store) = store();
        for outcome in [
            JobOutcome::Exited { code: 42 },
            JobOutcome::Signalled { signal: 15 },
            JobOutcome::CouldNotStart {
                reason: "no-such-tool: not found".to_owned(),
            },
        ] {
            let job = store
                .create_job(NewJob::new(vec!["true".to_owned()]))
                .expect("minted");
            store
                .record_state(
                    &job.id,
                    JobState::Finished {
                        outcome: outcome.clone(),
                    },
                )
                .expect("transition");
            assert_eq!(
                store
                    .job(&job.id)
                    .expect("readable")
                    .expect("still there")
                    .state,
                JobState::Finished { outcome }
            );
        }
    }

    /// E-15 wants the runner's measurements in the record, on the disk, where
    /// the first consumer reads them. A job that has not ended has none, and
    /// that is "not measured", never zero (D23).
    #[test]
    fn a_runner_s_measurements_land_on_the_record() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        assert_eq!(
            job.metrics,
            JobMetrics::default(),
            "a job that has not run has nothing measured"
        );
        let metrics = JobMetrics {
            host_load_milli: Some(1_500),
            peak_memory_bytes: Some(64 * 1024 * 1024),
            bytes_written: Some(4_096),
            log_bytes: 19,
            wall_time_ms: 250,
            // E-32's figure, which is recorded like every other and gates
            // nothing (D119): what matters of it here is that it survives
            // the trip through job.json.
            pressure: Some(herd_proto::Pressure {
                host_io_milli: Some(4_200),
                ..herd_proto::Pressure::default()
            }),
        };
        store
            .record_metrics(&job.id, metrics.clone())
            .expect("recorded");
        assert_eq!(
            store
                .job(&job.id)
                .expect("readable")
                .expect("still there")
                .metrics,
            metrics,
            "the figures must survive the trip through job.json"
        );
        // And nothing else about the job moved.
        assert_eq!(
            store.job(&job.id).expect("readable").expect("there").state,
            JobState::Queued
        );
    }

    #[test]
    fn an_unknown_job_is_an_error_not_a_silent_no_op() {
        let (_dir, store) = store();
        let ghost = JobId::new("j0000000404");
        assert!(store.record_state(&ghost, JobState::Running).is_err());
        assert!(store
            .append_log(&ghost, LogStream::Stdout, 0, b"x")
            .is_err());
        assert!(store.record_metrics(&ghost, JobMetrics::default()).is_err());
        assert_eq!(store.job(&ghost).expect("readable"), None);
        assert_eq!(store.job_log(&ghost).expect("readable"), None);
        assert_eq!(store.job_events(&ghost).expect("readable"), None);
    }

    /// A record that has reached its end is never written to again, by
    /// anyone, with anything — the same end included.
    ///
    /// The redelivery handshake E-44 rests on is settled a level up now: the
    /// hub refuses a frame about a job that is over *by name*, and the runner
    /// drops what it was holding (D42). The store used to accept the identical
    /// end as a no-op, and that acceptance was the thin end of the wedge the
    /// verification drove in — a late frame that gets as far as a writer is a
    /// late frame that writes.
    #[test]
    fn a_record_that_is_over_refuses_every_end_including_its_own() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let end = JobState::Finished {
            outcome: JobOutcome::Exited { code: 7 },
        };
        store.record_state(&job.id, end.clone()).expect("the end");
        for again in [
            end,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        ] {
            let error = store
                .record_state(&job.id, again)
                .expect_err("a job that is over does not end a second time");
            // Naming what the record holds, not merely refusing: an operator
            // reading this has to see which verdict stood.
            assert!(error.to_string().contains("is over"), "{error}");
            assert!(error.to_string().contains("code\":7"), "{error}");
        }
    }

    /// Nothing at all is written to a job that is over: not its state, not
    /// its measurements, not an attempt, not a byte of its log, not a gap
    /// marker on that log.
    ///
    /// Every writer of [`JobStore`] is here on purpose. The defect this
    /// forbids was found because `record_state` had a guard of its own and
    /// `record_metrics` did not — so a forged frame replaced a finished job's
    /// real figures with 999s and a forged chunk went into a closed log with
    /// a gap marker claiming four chunks had been lost. A new writer belongs
    /// in this list the day it is added.
    #[test]
    fn a_record_that_is_over_refuses_every_writer() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let runner = "wsl-pc";
        let attempt = store
            .begin_attempt(&job.id, runner, "claimed by wsl-pc: labels none", None)
            .expect("attempt 1");
        store
            .append_log(&job.id, LogStream::Stdout, 0, b"the real output\n")
            .expect("the attempt's own output");
        let measured = JobMetrics {
            log_bytes: 16,
            ..JobMetrics::default()
        };
        store
            .record_metrics(&job.id, measured.clone())
            .expect("what the runner measured");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the end");

        let refusals: Vec<(&str, StoreError)> = vec![
            (
                "a state",
                store
                    .record_state(&job.id, JobState::Running)
                    .expect_err("state"),
            ),
            (
                "measurements",
                store
                    .record_metrics(
                        &job.id,
                        JobMetrics {
                            log_bytes: 999,
                            wall_time_ms: 999,
                            ..JobMetrics::default()
                        },
                    )
                    .expect_err("metrics"),
            ),
            (
                "an attempt",
                store
                    .begin_attempt(&job.id, "somebody-else", "claimed by somebody-else", None)
                    .expect_err("begin"),
            ),
            (
                "the end of an attempt",
                store
                    .end_attempt(&job.id, attempt, "a late sentence", None)
                    .expect_err("end attempt"),
            ),
            (
                "output",
                store
                    .append_log(&job.id, LogStream::Stdout, 1, b"forged\n")
                    .expect_err("append"),
            ),
            (
                "a gap marker",
                store.name_missing_log(&job.id, 1, 4).expect_err("gap"),
            ),
        ];
        for (what, error) in refusals {
            assert!(
                error.to_string().contains("is over")
                    && error.to_string().contains(job.id.as_str()),
                "writing {what} to a job that is over must be refused by name: {error}"
            );
        }

        // M5's writer is on this list too, and it is the one that says `false`
        // instead of erroring: a dispatch pass that decides a job must wait, a
        // moment after that job ended, has nothing to write and nothing to
        // complain about — the no-op is what closes that race (D50). It
        // passes the same terminal test as everybody else, and the proof that
        // it wrote nothing is the untouched record just below.
        assert!(
            !store
                .record_waiting(&job.id, "a lock file nobody holds any more")
                .expect("a job that is over is simply not waiting for anything"),
            "it is news to nobody"
        );

        // And the record is exactly what the job's own attempt left.
        let record = store.job(&job.id).expect("readable").expect("there");
        assert_eq!(record.metrics, measured, "{record:?}");
        assert_eq!(
            record.waiting_for, None,
            "and nothing put a waiting reason on a record that is over: {record:?}"
        );
        assert_eq!(
            record.state,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 }
            }
        );
        assert_eq!(record.attempts.len(), 1, "{record:?}");
        assert!(
            record.attempts[0].ended_at.is_some() && record.attempts[0].ended.is_none(),
            "the attempt that produced the verdict is over, with nothing to excuse: {record:?}"
        );
        assert_eq!(
            store.job_log(&job.id).expect("readable").expect("there"),
            b"the real output\n",
            "the log of a job that is over is byte for byte what the job wrote"
        );
    }

    /// A job goes forward, with exactly one way back: the requeue M3 adds for
    /// a job whose runner was lost (E-21). A job that is over is over.
    #[test]
    fn a_job_goes_backwards_only_to_be_attempted_again() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        store
            .record_state(&job.id, JobState::Running)
            .expect("transition");
        // The one edge backwards, and it is recorded like any other.
        store
            .record_state(&job.id, JobState::Queued)
            .expect("a lost attempt is requeued once (E-21)");
        assert_eq!(
            store.job(&job.id).expect("readable").expect("there").state,
            JobState::Queued
        );
        // And not from an end. A verdict is not undone by requeueing.
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the second attempt ends");
        let error = store
            .record_state(&job.id, JobState::Queued)
            .expect_err("a finished job is never queued again");
        assert!(
            error.to_string().contains("is over") && error.to_string().contains("code\":0"),
            "the refusal names the job and the end it reached: {error}"
        );
    }

    #[test]
    fn the_queue_is_every_job_that_is_not_over_oldest_first() {
        let (_dir, store) = store();
        let first = store
            .create_job(NewJob::new(vec!["first".to_owned()]))
            .expect("minted");
        let second = store
            .create_job(NewJob::new(vec!["second".to_owned()]))
            .expect("minted");
        let third = store
            .create_job(NewJob::new(vec!["third".to_owned()]))
            .expect("minted");
        store
            .record_state(
                &second.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("transition");
        let queue: Vec<String> = store
            .unfinished_jobs()
            .expect("readable")
            .into_iter()
            .map(|record| record.id.to_string())
            .collect();
        assert_eq!(queue, vec![first.id.to_string(), third.id.to_string()]);
    }

    #[test]
    fn a_state_root_that_cannot_be_created_is_refused_with_its_path() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let in_the_way = dir.path().join("a-file");
        durable::write_file(&in_the_way, b"not a directory").expect("written");
        let wanted = in_the_way.join("state");
        let error = JobDirStore::open(&wanted).expect_err("there is a file in the way");
        assert!(
            error.to_string().contains(&wanted.display().to_string()),
            "the refusal must name the path: {error}"
        );
    }

    #[test]
    fn a_state_root_from_a_newer_herd_is_refused_naming_both_versions() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        {
            let _store = JobDirStore::open(dir.path()).expect("the state root opens");
        }
        durable::write_file(&dir.path().join(STORE_FILE), br#"{"layout_version":99}"#)
            .expect("written");
        let error = JobDirStore::open(dir.path()).expect_err("99 is not this layout");
        assert!(error.to_string().contains("99"), "{error}");
        assert!(
            error.to_string().contains(&LAYOUT_VERSION.to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_record_from_a_newer_herd_is_refused_rather_than_misread() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(RECORD_FILE);
        // One past whatever this Herd reads, computed rather than written
        // down, so the test does not have to be edited every time the number
        // moves (it has moved three times: M6, M7, M8).
        let newer = RECORD_VERSION + 1;
        durable::write_file(
            &path,
            format!(r#"{{"record_version":{newer},"id":"j0000000001"}}"#).as_bytes(),
        )
        .expect("written");
        let error = store
            .job(&job.id)
            .expect_err("a newer version is not readable here");
        assert!(
            error
                .to_string()
                .contains(&format!("record version {newer}")),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .contains(&format!("reads version {RECORD_VERSION}")),
            "both figures, so a reader knows which way round it is: {error}"
        );
    }

    /// D21: two hubs on one state root would mint ids from the same
    /// directories and write two disagreeing indexes. The second one is
    /// refused, naming the root and the hub that holds it.
    #[test]
    fn a_second_hub_on_one_state_root_is_refused_naming_the_holder() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let first = JobDirStore::open(dir.path()).expect("the first hub claims the root");
        let error = JobDirStore::open(dir.path()).expect_err("the second hub is refused");
        let said = error.to_string();
        assert!(
            said.contains(&dir.path().display().to_string()),
            "the refusal must name the state root: {said}"
        );
        assert!(
            said.contains(&format!("process {}", std::process::id())),
            "the refusal must name who holds it: {said}"
        );

        // And the claim is given back when the hub that held it is gone, so a
        // restart never has to break a lock by hand.
        drop(first);
        let _again = JobDirStore::open(dir.path()).expect("the root is free again");
    }

    /// A state root whose `jobs/` cannot be written used to start a hub that
    /// then failed every submission, with only its own log to say why (D21).
    #[test]
    fn a_state_root_whose_jobs_directory_cannot_be_written_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("a temporary directory");
        {
            let _store = JobDirStore::open(dir.path()).expect("the state root opens");
        }
        let jobs = dir.path().join(JOBS_DIR);
        fs::set_permissions(&jobs, fs::Permissions::from_mode(0o500)).expect("read-only");
        if durable::probe_writable(&jobs).is_ok() {
            // Running as a user permissions do not apply to; there is nothing
            // to refuse and nothing to assert.
            fs::set_permissions(&jobs, fs::Permissions::from_mode(0o700)).expect("writable again");
            return;
        }

        let error = JobDirStore::open(dir.path()).expect_err("a hub cannot work in that root");
        let said = error.to_string();
        fs::set_permissions(&jobs, fs::Permissions::from_mode(0o700)).expect("writable again");
        assert!(
            said.contains(&jobs.display().to_string()),
            "the refusal must name the directory it cannot write: {said}"
        );
    }

    /// Nothing but the job directories decides which ids exist, so a store
    /// whose index is gone still mints an id that collides with nothing.
    #[test]
    fn ids_come_from_the_directories_and_are_never_reused() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let first = {
            let store = JobDirStore::open(dir.path()).expect("opens");
            store
                .create_job(NewJob::new(vec!["true".to_owned()]))
                .expect("minted")
                .id
        };
        fs::remove_dir_all(dir.path().join(INDEX_DIR)).expect("the index is gone");
        let store = JobDirStore::open(dir.path()).expect("opens without an index");
        // A different command, because the first job is still open and an
        // identical submission would join it rather than mint (E-05).
        let second = store
            .create_job(NewJob::new(vec!["false".to_owned()]))
            .expect("minted")
            .id;
        assert_ne!(first, second);
        assert!(second.as_str() > first.as_str(), "{first} then {second}");
    }

    // --- M6: the one queue, its priorities and its deduplication ---

    /// A job carries the level its requester asked for, and the record says
    /// so in a word a person can read.
    #[test]
    fn a_record_carries_the_priority_its_requester_asked_for() {
        let (dir, store) = store();
        for priority in Priority::ALL {
            let job = store
                .create_job(NewJob::at(
                    vec![priority.name().to_owned()],
                    LabelSet::default(),
                    priority,
                ))
                .expect("minted");
            assert_eq!(job.priority, priority);
            assert_eq!(job.submissions, 1);
            let path = dir
                .path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(RECORD_FILE);
            let written: serde_json::Value =
                serde_json::from_str(&read(&path)).expect("the record is json");
            assert_eq!(written["priority"], priority.name());
            assert_eq!(written["record_version"], RECORD_VERSION);
        }
    }

    /// E-05: the second submission of work that is already under way joins
    /// it. One execution, one id, and the record says it answers two.
    #[test]
    fn a_second_identical_submission_joins_the_open_job() {
        let (_dir, store) = store();
        let work = || {
            NewJob::at(
                vec!["cargo".to_owned(), "build".to_owned()],
                LabelSet::new(["build"]).expect("a label"),
                Priority::Step,
            )
        };
        let first = match store.submit(work()).expect("the first submission") {
            Submission::Minted(record) => record,
            other => panic!("the first submission should mint: {other:?}"),
        };
        let second = store.submit(work()).expect("the second submission");
        match &second {
            Submission::Joined { record, raised } => {
                assert_eq!(record.id, first.id, "both callers hold one id");
                assert_eq!(record.submissions, 2);
                assert_eq!(*raised, None, "the same level is no raise");
            }
            other => panic!("the second submission should join: {other:?}"),
        }
        // A third one too, and the count keeps up.
        let third = store.submit(work()).expect("the third submission");
        assert_eq!(third.record().submissions, 3);
        // One job on the disk, not three.
        assert_eq!(
            store.unfinished_jobs().expect("the queue reads").len(),
            1,
            "three submissions of one piece of work are one job"
        );
        // And the join is on the record's history, with the level the joiner
        // asked for.
        let events = store
            .job_events(&first.id)
            .expect("the events read")
            .expect("a job this store minted has events");
        let joins: Vec<_> = events
            .iter()
            .filter(|event| matches!(event.what, JobEventKind::Joined { .. }))
            .collect();
        assert_eq!(joins.len(), 2, "two joins, one line each: {events:?}");
    }

    /// A declared submission, as the hub receives one: a resolved line that
    /// says nothing about the commit — which is the ordinary declared
    /// command, since `{commit}` goes into the executor's `up` and `fetch`
    /// steps — and the commit in a field of its own (D156).
    fn declared_work(commit: &str) -> NewJob {
        let declaration = Declaration {
            commit: commit.to_owned(),
            ..a_declaration()
        };
        NewJob::at(
            vec!["tool check --all".to_owned()],
            LabelSet::new(["build"]).expect("a label"),
            Priority::Step,
        )
        .declared(Some(declaration), None)
    }

    /// E-05 still holds for a declared command: a push hook firing twice at
    /// one commit must not run the work twice. D156 put the commit in the
    /// key and must not have made every key unique.
    #[test]
    fn two_declared_submissions_at_one_commit_are_one_job() {
        let (_dir, store) = store();
        let first = match store.submit(declared_work(A_COMMIT)).expect("the first") {
            Submission::Minted(record) => record,
            other => panic!("the first submission should mint: {other:?}"),
        };
        match store.submit(declared_work(A_COMMIT)).expect("the second") {
            Submission::Joined { record, .. } => {
                assert_eq!(record.id, first.id, "both callers hold one id");
                assert_eq!(record.submissions, 2);
            }
            other => panic!("the second submission should join: {other:?}"),
        }
        assert_eq!(
            store.unfinished_jobs().expect("the queue reads").len(),
            1,
            "one declared command at one commit is one job"
        );
    }

    /// The defect D156 fixes, at the level the hub actually decides it: the
    /// same declared command at two commits joins nothing, and each
    /// submission gets a job carrying its **own** commit.
    ///
    /// Before D156 the key was the command and the labels alone, so these two
    /// submissions were byte-identical work and the second was handed the
    /// first's verdict — a wrong answer presented as a right one.
    #[test]
    fn two_declared_submissions_at_two_commits_are_two_jobs() {
        let (_dir, store) = store();
        let first = match store.submit(declared_work(A_COMMIT)).expect("the first") {
            Submission::Minted(record) => record,
            other => panic!("the first submission should mint: {other:?}"),
        };
        let second = match store
            .submit(declared_work(ANOTHER_COMMIT))
            .expect("the second")
        {
            Submission::Minted(record) => record,
            Submission::Joined { record, .. } => panic!(
                "a submission for {ANOTHER_COMMIT} joined {}, which is running {A_COMMIT}",
                record.id
            ),
        };
        assert_ne!(first.id, second.id);
        assert_eq!(first.submissions, 1);
        assert_eq!(second.submissions, 1);
        // The resolved line really is the same on both: it is the commit,
        // and nothing else, that made them two pieces of work.
        assert_eq!(first.command, second.command);
        assert_eq!(first.labels, second.labels);
        let commit_of = |record: &JobRecord| {
            record
                .declaration
                .as_ref()
                .expect("a declared submission")
                .commit
                .clone()
        };
        assert_eq!(commit_of(&first), A_COMMIT);
        assert_eq!(commit_of(&second), ANOTHER_COMMIT);
        assert_eq!(
            store.unfinished_jobs().expect("the queue reads").len(),
            2,
            "two commits are two jobs"
        );
    }

    /// The key is derived from the record, so the link the hub rebuilds at
    /// start keys on the commit too — and D156 therefore needed nothing of
    /// the records on disk (no field, no default, no [`RECORD_VERSION`]).
    #[test]
    fn a_recovered_declared_job_is_keyed_on_its_own_commit() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let first = {
            let store = JobDirStore::open(dir.path()).expect("the state root opens");
            store
                .create_job(declared_work(A_COMMIT))
                .expect("the first job")
        };
        // A hub that comes back reads the records and rebuilds the links.
        let store = JobDirStore::open(dir.path()).expect("the state root reopens");
        match store
            .submit(declared_work(A_COMMIT))
            .expect("the same work")
        {
            Submission::Joined { record, .. } => assert_eq!(record.id, first.id),
            other => panic!("the same commit should join across a restart: {other:?}"),
        }
        match store
            .submit(declared_work(ANOTHER_COMMIT))
            .expect("other work")
        {
            Submission::Minted(record) => assert_ne!(record.id, first.id),
            Submission::Joined { record, .. } => panic!(
                "a submission for {ANOTHER_COMMIT} joined the recovered {}",
                record.id
            ),
        }
    }

    /// E-05: "the merged job takes the higher priority" — and only ever the
    /// higher one.
    #[test]
    fn a_joining_submission_raises_the_job_but_never_lowers_it() {
        let (_dir, store) = store();
        let work = |priority| {
            NewJob::at(
                vec!["cargo".to_owned(), "build".to_owned()],
                LabelSet::default(),
                priority,
            )
        };
        let first = store
            .create_job(work(Priority::Producer))
            .expect("the first submission mints");
        match store.submit(work(Priority::Merger)).expect("joins") {
            Submission::Joined { record, raised } => {
                assert_eq!(raised, Some(Priority::Merger));
                assert_eq!(record.priority, Priority::Merger);
            }
            other => panic!("expected a join: {other:?}"),
        }
        // And a later joiner asking for less leaves it where it is.
        match store.submit(work(Priority::Nightly)).expect("joins") {
            Submission::Joined { record, raised } => {
                assert_eq!(raised, None);
                assert_eq!(record.priority, Priority::Merger);
            }
            other => panic!("expected a join: {other:?}"),
        }
        assert_eq!(
            store
                .job(&first.id)
                .expect("the record reads")
                .expect("it is there")
                .priority,
            Priority::Merger
        );
    }

    /// The attack: a submission must never join work that is *over*, however
    /// that work ended. A stale verdict handed to a caller that never
    /// submitted the first job is a verdict about a different request — and
    /// for `cancelled` and `lost` it is a 125 worn for somebody else's
    /// cancellation (D40, D70).
    #[test]
    fn a_submission_never_joins_a_job_that_is_over() {
        for ending in [
            JobOutcome::Exited { code: 0 },
            JobOutcome::Exited { code: 1 },
            JobOutcome::Cancelled {
                reason: "a requester asked for it".to_owned(),
            },
            JobOutcome::Lost {
                reason: "its runner went silent".to_owned(),
            },
        ] {
            let (_dir, store) = store();
            let work = || {
                NewJob::at(
                    vec!["cargo".to_owned(), "build".to_owned()],
                    LabelSet::default(),
                    Priority::Step,
                )
            };
            let first = store.create_job(work()).expect("minted");
            store
                .record_state(
                    &first.id,
                    JobState::Finished {
                        outcome: ending.clone(),
                    },
                )
                .expect("the job ends");
            let again = store.submit(work()).expect("the second submission");
            match &again {
                Submission::Minted(record) => assert_ne!(
                    record.id, first.id,
                    "a submission after {ending:?} must be its own job"
                ),
                Submission::Joined { record, .. } => panic!(
                    "a submission joined {} which had ended {ending:?}",
                    record.id
                ),
            }
            // The new job is queued and waiting for a runner, not carrying
            // the old job's end.
            assert_eq!(again.record().state, JobState::Queued);
            assert_eq!(again.record().submissions, 1);
        }
    }

    /// The narrow race under D70: the open-work index says a job is doing
    /// this work, and the job reaches its end between that reading and the
    /// join. The submission must become a job of its own — never an error
    /// saying "that record is over", which is a submission lost to a
    /// coincidence of timing.
    #[test]
    fn a_stale_open_work_link_mints_rather_than_failing() {
        let (_dir, store) = store();
        let work = || {
            NewJob::at(
                vec!["cargo".to_owned(), "build".to_owned()],
                LabelSet::default(),
                Priority::Step,
            )
        };
        let first = store.create_job(work()).expect("minted");
        // The job ends, and the index is left pointing at it by hand: this
        // is the state the race leaves for the few microseconds it lasts.
        store
            .record_state(
                &first.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the job ends");
        {
            let mut live = store.lock().expect("the lock");
            live.open_work.insert(
                first.dedup_key(),
                sequence_of(first.id.as_str()).expect("an id this store minted"),
            );
        }
        match store.submit(work()).expect("the submission is answered") {
            Submission::Minted(record) => assert_ne!(record.id, first.id),
            Submission::Joined { record, .. } => {
                panic!("a submission joined {}, which is over", record.id)
            }
        }
    }

    /// The deduplication link is derived from the records, so a hub that
    /// comes back finds it again (D69). Without this the restart would run
    /// one piece of work twice.
    #[test]
    fn a_join_survives_a_restart_of_the_hub() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let work = || {
            NewJob::at(
                vec!["cargo".to_owned(), "build".to_owned()],
                LabelSet::new(["build", "kvm"]).expect("labels"),
                Priority::Nightly,
            )
        };
        let first = {
            let store = JobDirStore::open(dir.path()).expect("opens");
            store.create_job(work()).expect("minted").id
        };
        // Including the case where the index has to be rebuilt from the
        // directories: the link is read off the records either way.
        for attempt in 0..2 {
            if attempt == 1 {
                fs::remove_dir_all(dir.path().join(INDEX_DIR)).expect("the index is gone");
            }
            let store = JobDirStore::open(dir.path()).expect("reopens");
            match store.submit(work()).expect("the submission") {
                Submission::Joined { record, .. } => assert_eq!(record.id, first),
                Submission::Minted(record) => panic!(
                    "a hub that came back minted {} for work {first} is already doing",
                    record.id
                ),
            }
        }
    }

    /// The attack: raising a job that is over must be refused by name at the
    /// store's one write door, never written (D54).
    #[test]
    fn the_priority_of_a_job_that_is_over_cannot_be_raised() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Producer,
            ))
            .expect("minted");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the job ends");
        let error = store
            .raise_priority(&job.id, Priority::Merger)
            .expect_err("a record that has reached its end is never written to again");
        let said = error.to_string();
        assert!(said.contains(job.id.as_str()), "{said}");
        assert!(said.contains("is over"), "{said}");
        // And nothing was written: the level is the one the requester asked
        // for, and the history has no raise on it.
        let record = store
            .job(&job.id)
            .expect("the record reads")
            .expect("it is there");
        assert_eq!(record.priority, Priority::Producer);
        let events = store
            .job_events(&job.id)
            .expect("the events read")
            .expect("there are events");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.what, JobEventKind::PriorityRaised { .. })),
            "{events:?}"
        );
    }

    /// A raise only raises, and says so (E-18).
    #[test]
    fn a_raise_only_ever_raises() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Nightly,
            ))
            .expect("minted");
        assert_eq!(
            store
                .raise_priority(&job.id, Priority::Nightly)
                .expect("ok"),
            Raise::AlreadyThere(Priority::Nightly)
        );
        assert_eq!(
            store
                .raise_priority(&job.id, Priority::Producer)
                .expect("ok"),
            Raise::NotARaise {
                from: Priority::Nightly,
                asked: Priority::Producer
            }
        );
        assert_eq!(
            store.raise_priority(&job.id, Priority::Merger).expect("ok"),
            Raise::Raised {
                from: Priority::Nightly,
                to: Priority::Merger
            }
        );
        let record = store.job(&job.id).expect("reads").expect("it is there");
        assert_eq!(record.priority, Priority::Merger);
        // One raise on the history, not three.
        let raises = store
            .job_events(&job.id)
            .expect("reads")
            .expect("events")
            .into_iter()
            .filter(|event| matches!(event.what, JobEventKind::PriorityRaised { .. }))
            .count();
        assert_eq!(raises, 1);
    }

    /// Raising a job nobody minted is refused by name, not silently created.
    #[test]
    fn raising_a_job_that_does_not_exist_is_refused_by_name() {
        let (_dir, store) = store();
        let error = store
            .raise_priority(&JobId::new("j0000009999"), Priority::Merger)
            .expect_err("there is no such job");
        assert!(error.to_string().contains("j0000009999"), "{error}");
    }

    /// Everything that is not over is in the one queue (E-20), whatever its
    /// level, and the store hands it over oldest first — the order
    /// `crate::queue` then sorts.
    #[test]
    fn every_job_that_is_not_over_is_in_the_one_queue() {
        let (_dir, store) = store();
        let mut minted = Vec::new();
        for priority in Priority::ALL {
            minted.push(
                store
                    .create_job(NewJob::at(
                        vec![format!("job-at-{}", priority.name())],
                        LabelSet::default(),
                        priority,
                    ))
                    .expect("minted"),
            );
        }
        let queue = store.unfinished_jobs().expect("the queue reads");
        assert_eq!(queue.len(), 4);
        assert_eq!(
            queue
                .iter()
                .map(|record| record.priority)
                .collect::<Vec<_>>(),
            Priority::ALL.to_vec(),
            "the store's order is the order they were minted in"
        );
        // One of them ends, and it leaves the queue — nothing else moves.
        store
            .record_state(
                &minted[1].id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("it ends");
        let queue = store.unfinished_jobs().expect("the queue reads");
        assert_eq!(
            queue
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>(),
            vec![
                minted[0].id.clone(),
                minted[2].id.clone(),
                minted[3].id.clone()
            ]
        );
    }

    /// A declared command on the record, and the four step lines of its
    /// attempt (E-11, D77).
    ///
    /// The declaration is written at birth and read back off the disk, because
    /// it is what a requeue onto another machine runs: a record that lost it
    /// would re-run the job as a bare command line with no workspace.
    #[test]
    fn a_declared_command_and_its_step_lines_survive_a_write_and_a_read() {
        let (_dir, store) = store();
        let declared = store
            .create_job(
                NewJob::new(vec!["a-line --only one".to_owned()])
                    .declared(Some(a_declaration()), Some(2700)),
            )
            .expect("minted");
        let read_back = store
            .job(&declared.id)
            .expect("readable")
            .expect("a record was written");
        assert_eq!(
            read_back.declaration.as_ref().map(|d| d.commit.as_str()),
            Some(A_COMMIT),
            "the commit the requester resolved is on the record"
        );
        assert_eq!(
            read_back
                .declaration
                .as_ref()
                .map(|d| d.executor.run.as_str()),
            Some("tool ws run {job} -- {line}"),
            "and so are the four hooks, with the tokens the runner fills left alone"
        );
        assert_eq!(read_back.timeout_s, Some(2700));

        let attempt = store
            .begin_attempt(&declared.id, "runner-x", "the only runner", None)
            .expect("an attempt is numbered");
        for step in [
            step(Step::Up, 95_400, Some(0)),
            step(Step::Fetch, 1_200, Some(0)),
            step(Step::Run, 17_500, Some(1)),
            step(Step::Down, 400, Some(0)),
        ] {
            store
                .record_step(&declared.id, attempt, step)
                .expect("the step is recorded");
        }
        let lines: Vec<String> = store
            .job(&declared.id)
            .expect("readable")
            .expect("a record")
            .attempts[0]
            .steps
            .iter()
            .map(|ran| ran.line())
            .collect();
        assert_eq!(
            lines,
            [
                "up: 95.400 s, exit 0",
                "fetch: 1.200 s, exit 0",
                "run: 17.500 s, exit 1",
                "down: 0.400 s, exit 0",
            ],
            "one line per step, in the order they ran, with the figures D77 asks for"
        );
    }

    /// A step said twice is one step: the complete list arrives again with the
    /// verdict, and a record holding `up` twice would read as a job that
    /// prepared twice in one attempt.
    #[test]
    fn a_step_said_again_replaces_the_line_it_already_had() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let attempt = store
            .begin_attempt(&job.id, "runner-x", "the only runner", None)
            .expect("an attempt is numbered");
        store
            .record_step(&job.id, attempt, step(Step::Up, 95_400, None))
            .expect("recorded");
        store
            .record_step(&job.id, attempt, step(Step::Up, 95_400, Some(0)))
            .expect("recorded again");
        let steps = store
            .job(&job.id)
            .expect("readable")
            .expect("a record")
            .attempts[0]
            .steps
            .clone();
        assert_eq!(steps.len(), 1, "one step, not two: {steps:?}");
        assert_eq!(
            steps[0].exit_code,
            Some(0),
            "and it is the later line that stands"
        );
    }

    /// A step for an attempt the record does not hold is an error, not a
    /// silent no-op: the hub says so rather than losing the line.
    #[test]
    fn a_step_of_an_attempt_that_is_not_there_is_refused_by_name() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::new(vec!["true".to_owned()]))
            .expect("minted");
        let refused = store
            .record_step(&job.id, 7, step(Step::Up, 10, Some(0)))
            .expect_err("there is no attempt 7");
        let said = refused.to_string();
        assert!(
            said.contains(job.id.as_str()) && said.contains('7') && said.contains("up"),
            "the refusal names the job, the attempt and the step: {said}"
        );
    }

    /// What a reading answered is kept **in the job store**, under the
    /// (commit, command, args) key, and it outlives the hub that read it
    /// (D74).
    ///
    /// The restart is the point: without it the cache would be a map in one
    /// process, and the first submission after a deployment would pay for
    /// every reading again.
    #[test]
    fn a_reading_s_answer_is_kept_by_its_key_and_survives_a_restart() {
        let dir = TempDir::new().expect("a temporary directory");
        let answer = Requirements {
            labels: LabelSet::new(["kvm", "metal"]).expect("valid labels"),
            memory_mib: 4096,
            tmpfs_mib: 8192,
            scenarios: 12,
        };
        let key = crate::job::reading_key(A_COMMIT, "test", &["--only".to_owned()]);
        let another = crate::job::reading_key(A_COMMIT, "test", &[]);
        {
            let store = JobDirStore::open(dir.path()).expect("the state root opens");
            assert_eq!(
                store.reading_answer(&key).expect("the cache reads"),
                None,
                "a store that has read nothing answers nothing, never a default"
            );
            store
                .remember_reading(&key, &answer)
                .expect("the answer is kept");
            assert_eq!(
                store.reading_answer(&key).expect("the cache reads"),
                Some(answer.clone())
            );
            assert_eq!(
                store.reading_answer(&another).expect("the cache reads"),
                None,
                "another argument list is another reading"
            );
        }
        // The same state root, a new hub: the answer is still there, so the
        // next submission of those three pays for no reading.
        let store = JobDirStore::open(dir.path()).expect("the state root opens again");
        assert_eq!(
            store.reading_answer(&key).expect("the cache reads"),
            Some(answer)
        );

        // And a cache that cannot be read is an empty cache, not a hub that
        // will not start: the readings are derived, and the cost of losing
        // them is paying for them again.
        drop(store);
        std::fs::write(
            dir.path().join("readings").join("answers.json"),
            "this is not json",
        )
        .expect("the cache is made unreadable");
        let store = JobDirStore::open(dir.path()).expect("an unreadable cache is not a refusal");
        assert_eq!(store.reading_answer(&key).expect("the cache reads"), None);
    }

    // --- D131: a job's declared outputs are part of its record ---

    /// A collection as a runner reports one, with the first consumer's shape:
    /// a file under a directory.
    fn collected(files: &[(&str, &[u8])]) -> herd_proto::JobOutputs {
        herd_proto::JobOutputs {
            files: files
                .iter()
                .map(|(path, data)| {
                    herd_proto::OutputFile::collected((*path).to_owned(), data.to_vec())
                })
                .collect(),
            ..herd_proto::JobOutputs::default()
        }
    }

    /// The same collection by the other carriage: the manifest alone, with
    /// every entry announced and none carried.
    fn announced(files: &[(&str, &[u8])]) -> herd_proto::JobOutputs {
        herd_proto::JobOutputs {
            files: files
                .iter()
                .map(|(path, data)| {
                    herd_proto::OutputFile::announced(
                        (*path).to_owned(),
                        data.len() as u64,
                        herd_proto::sha256_hex(data),
                    )
                })
                .collect(),
            streamed: true,
            ..herd_proto::JobOutputs::default()
        }
    }

    /// A job minted, handed to a runner, and ready to have a collection
    /// recorded against it.
    ///
    /// The attempt is not decoration: a collection belongs to the attempt
    /// that produced it, and `collections_owed` has to know **which runner**
    /// to ask for the bytes. A record with a streamed collection and no
    /// attempt is a record that contradicts itself, which is why the store
    /// skips one rather than guessing at a runner.
    fn a_job(store: &JobDirStore) -> JobRecord {
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        store
            .begin_attempt(&job.id, "test-runner", "the only runner", None)
            .expect("the attempt is recorded");
        job
    }

    /// The streamed carriage, end to end through the store: a manifest with no
    /// bytes, then the bytes, then a complete and verified collection.
    ///
    /// **The property that matters is the last assertion**: what a requester
    /// reads back is byte-identical to what the inline carriage would have
    /// given it. Everything above it is the mechanism; that line is the
    /// promise (D148).
    #[test]
    fn a_streamed_collection_lands_the_same_bytes_the_inline_one_would() {
        let (dir, streamed_store) = store();
        let store = &streamed_store;
        let job = a_job(store);
        let big: Vec<u8> = (0..(3 * herd_proto::OUTPUT_CHUNK_BYTES as usize + 7))
            .map(|n| (n % 251) as u8)
            .collect();
        let files: &[(&str, &[u8])] = &[
            ("files/results.json", br#"{"ok":true}"#),
            ("files/deep/shot.png", &big),
            ("files/empty.marker", b""),
        ];
        store
            .record_outputs(&job.id, &announced(files))
            .expect("the manifest is recorded");

        // The manifest is on the record from the moment the verdict landed,
        // and **no byte is on the disk yet**. That is the one thing the
        // inline carriage never does, and it is deliberate: a record is
        // complete about what a job produced before its evidence has crossed.
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        let record = store.job(&job.id).expect("reads").expect("there");
        assert!(record.outputs.streamed, "the record says which carriage");
        assert_eq!(record.outputs.files.len(), 3);
        assert!(
            record
                .outputs
                .files
                .iter()
                .all(|file| !file.sha256.is_empty()),
            "every entry carries a digest, which is what outlives the bytes"
        );
        assert!(
            !kept.join("files/results.json").exists(),
            "nothing is written until its bytes arrive"
        );

        // Nothing held, so the runner is told to send everything from the
        // beginning: a path absent from the holdings is exactly that
        // instruction.
        assert!(store
            .output_holdings(&job.id)
            .expect("holdings read")
            .is_empty());

        // The bytes, a chunk at a time, in the order a runner sends them.
        let mut landed = crate::job::ChunkLanded::More;
        for (path, data) in files {
            if data.is_empty() {
                landed = store
                    .write_output_chunk(&job.id, path, 0, b"")
                    .expect("an empty file is a file");
                continue;
            }
            for (index, piece) in data
                .chunks(herd_proto::OUTPUT_CHUNK_BYTES as usize)
                .enumerate()
            {
                let offset = index as u64 * herd_proto::OUTPUT_CHUNK_BYTES;
                landed = store
                    .write_output_chunk(&job.id, path, offset, piece)
                    .expect("a chunk lands");
            }
        }
        assert_eq!(
            landed,
            crate::job::ChunkLanded::Complete,
            "the last chunk of the last file completes the collection"
        );

        // On the disk, under the same paths the inline carriage uses.
        assert_eq!(
            fs::read(kept.join("files/deep/shot.png")).expect("the big file is there"),
            big
        );
        assert_eq!(
            fs::read(kept.join("files/empty.marker")).expect("the empty file is there"),
            Vec::<u8>::new(),
            "an empty file is a real file, not the absence of one"
        );

        // **And the promise.** Recorded by the other carriage into a second
        // store, read back the same way, compared field by field: the two
        // collections a requester is handed are equal.
        let (_other_dir, other) = make_a_store();
        let twin = a_job(&other);
        other
            .record_outputs(&twin.id, &collected(files))
            .expect("the inline collection is kept");
        let by_frame = other.job_outputs(&twin.id, true).expect("reads back");
        let by_stream = store.job_outputs(&job.id, true).expect("reads back");
        assert_eq!(
            by_stream.files, by_frame.files,
            "a requester cannot tell which carriage it got: that is the whole of D148's \
             'a requester declares nothing'"
        );
        assert_eq!(by_stream.refused, by_frame.refused);
        assert_eq!(by_stream.skipped, by_frame.skipped);
        // The *record* differs in exactly one field, which is the one an
        // operator reads and a requester does not.
        assert!(by_stream.streamed && !by_frame.streamed);
    }

    /// A part-written file is resumable, and the hub is the side that says
    /// from where.
    #[test]
    fn a_half_sent_file_resumes_at_the_length_the_hub_holds() {
        let (dir, store) = store();
        let job = a_job(&store);
        let big: Vec<u8> = (0..(2 * herd_proto::OUTPUT_CHUNK_BYTES as usize))
            .map(|n| (n % 97) as u8)
            .collect();
        store
            .record_outputs(&job.id, &announced(&[("files/shot.png", &big)]))
            .expect("recorded");

        // One chunk of two, as a link that dropped mid-transfer leaves it.
        store
            .write_output_chunk(
                &job.id,
                "files/shot.png",
                0,
                &big[..herd_proto::OUTPUT_CHUNK_BYTES as usize],
            )
            .expect("the first chunk lands");
        assert_eq!(
            store.output_holdings(&job.id).expect("holdings"),
            vec![herd_proto::FileHeld {
                path: "files/shot.png".to_owned(),
                bytes: herd_proto::OUTPUT_CHUNK_BYTES,
            }],
            "the hub says how far it got, which is what the runner resumes from"
        );
        // And the file is **not** visible under `files/` while it is partial:
        // a consumer reading the record's directory during a transfer sees
        // whole files or none (E-53).
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        assert!(!kept.join("files/shot.png").exists());

        // A chunk that does not continue where the hub is, refused by name
        // rather than spliced into a file that would then fail its digest for
        // a reason nobody could see.
        let crate::job::ChunkLanded::Refused(why) = store
            .write_output_chunk(&job.id, "files/shot.png", 0, b"again from the top")
            .expect("the store answers")
        else {
            panic!("an offset that does not continue must be refused");
        };
        assert!(why.contains("starts at 0"), "{why}");

        // The rest, from where the hub really is.
        let landed = store
            .write_output_chunk(
                &job.id,
                "files/shot.png",
                herd_proto::OUTPUT_CHUNK_BYTES,
                &big[herd_proto::OUTPUT_CHUNK_BYTES as usize..],
            )
            .expect("the second chunk lands");
        assert_eq!(landed, crate::job::ChunkLanded::Complete);
        assert_eq!(fs::read(kept.join("files/shot.png")).expect("there"), big);
    }

    /// Bytes that do not hash to what the verdict promised are thrown away,
    /// named, and never renamed into place.
    #[test]
    fn a_file_that_fails_its_digest_is_refused_and_not_kept() {
        let (dir, store) = store();
        let job = a_job(&store);
        store
            .record_outputs(&job.id, &announced(&[("files/results.json", b"the truth")]))
            .expect("recorded");
        let crate::job::ChunkLanded::Refused(why) = store
            .write_output_chunk(&job.id, "files/results.json", 0, b"not truth")
            .expect("the store answers")
        else {
            panic!("a digest that does not match must be refused");
        };
        assert!(why.contains("sha256"), "{why}");
        assert!(
            why.contains(&herd_proto::sha256_hex(b"the truth")),
            "the refusal names what was promised: {why}"
        );
        assert!(
            why.contains(&herd_proto::sha256_hex(b"not truth")),
            "and what arrived, because 'it did not match' with no figures is unactionable: \
             {why}"
        );
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        assert!(
            !kept.join("files/results.json").exists(),
            "a file that failed its digest is never in files/"
        );
        // And the collection is still outstanding, so the runner goes on
        // holding it: the hub has not got the bytes and must not pretend it
        // has (D19).
        let owed = store.collections_owed().expect("owed reads");
        assert_eq!(owed.len(), 1, "{owed:?}");
    }

    /// A path no verdict announced is never written, whatever a peer sends.
    #[test]
    fn a_path_the_manifest_does_not_name_is_refused() {
        let (_dir, store) = store();
        let job = a_job(&store);
        store
            .record_outputs(&job.id, &announced(&[("files/results.json", b"ok")]))
            .expect("recorded");
        let crate::job::ChunkLanded::Refused(why) = store
            .write_output_chunk(&job.id, "files/surprise.sh", 0, b"rm -rf /")
            .expect("the store answers")
        else {
            panic!("a path off the manifest must be refused");
        };
        assert!(why.contains("files/surprise.sh"), "{why}");
        assert!(why.contains("manifest"), "{why}");
    }

    /// An outstanding collection is found by looking, names its runner, and
    /// stops being outstanding when the bytes are all there.
    ///
    /// **A finished job**, and that is the point: the verdict goes first and
    /// the files follow (D148), so an outstanding collection is never in
    /// `unfinished_jobs` and a scan that only looked there would wait for
    /// bytes nobody was ever going to be asked for.
    #[test]
    fn a_collection_still_owed_is_derived_from_the_records_and_names_its_runner() {
        let (_dir, store) = store();
        let job = a_job(&store);
        store
            .record_outputs(&job.id, &announced(&[("files/results.json", b"ok")]))
            .expect("recorded");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("the job is over, and its files are not");

        let owed = store.collections_owed().expect("the scan reads");
        assert_eq!(owed.len(), 1, "{owed:?}");
        assert_eq!(owed[0].job_id, job.id);
        assert_eq!(
            owed[0].runner, "test-runner",
            "so one runner is never asked for another's collection"
        );
        assert_eq!(owed[0].attempt, 1);
        assert!(
            owed[0].held.is_empty(),
            "nothing held means send it from the beginning"
        );

        // An inline collection is never owed anything: its bytes arrived with
        // the verdict, so a hub that chased it would be chasing itself.
        let inline = a_job(&store);
        store
            .record_outputs(&inline.id, &collected(&[("files/results.json", b"ok")]))
            .expect("kept");
        assert_eq!(
            store.collections_owed().expect("reads").len(),
            1,
            "only the streamed one is outstanding"
        );

        // And once the bytes are there it stops being outstanding, read off
        // the disk rather than off a counter that could disagree with it.
        store
            .write_output_chunk(&job.id, "files/results.json", 0, b"ok")
            .expect("the chunk lands");
        assert!(
            store.collections_owed().expect("reads").is_empty(),
            "a complete collection is owed nothing"
        );
    }

    /// The manifest outlives the bytes: a record whose files are gone still
    /// says what they were.
    ///
    /// The case reached here is the one that exists today — a runner that
    /// never came back, so bytes that never arrived. M12's sweeper will reach
    /// the same state from the other direction, which is why there is one
    /// answer for both.
    #[test]
    fn a_record_whose_bytes_are_gone_still_names_every_file_it_had() {
        let (dir, store) = store();
        let job = a_job(&store);
        let files: &[(&str, &[u8])] = &[
            ("files/results.json", br#"{"verdict":"ok"}"#),
            ("files/shot.png", b"pretend this is 300 MiB"),
        ];
        store
            .record_outputs(&job.id, &collected(files))
            .expect("kept");
        // Taken away behind the record's back, which is exactly what
        // retention will do on purpose.
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        fs::remove_file(kept.join("files/shot.png")).expect("removed");

        let back = store.job_outputs(&job.id, true).expect("reads back");
        assert_eq!(
            back.files.len(),
            2,
            "two files on the record means two entries, or a reader takes one for all of \
             them: {back:?}"
        );
        let gone = back
            .files
            .iter()
            .find(|file| file.path == "files/shot.png")
            .expect("still named");
        assert!(!gone.is_held(), "its bytes are not there");
        assert_eq!(gone.len, 23, "and its size is still known");
        assert!(
            !gone.sha256.is_empty(),
            "and what it hashed to, which is what a judge re-reading it needs (E-54)"
        );
        assert!(
            back.skipped
                .iter()
                .any(|said| said.contains("files/shot.png")),
            "and the absence is said out loud: {:?}",
            back.skipped
        );
    }

    /// The whole promise, on the disk and back: `files/results.json` is
    /// written at `files/results.json` under the job's own `files/`
    /// directory, byte for byte, and comes back under that same path.
    #[test]
    fn a_kept_file_lands_under_the_job_s_own_directory_with_its_path_kept() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        // Bytes that are not text, for the reason a log's are not (D18): a
        // results file is UTF-8 only if the consumer made it so.
        let odd: Vec<u8> = vec![0, 1, 0xff, 0xfe, b'{', b'}'];
        store
            .record_outputs(
                &job.id,
                &collected(&[
                    ("files/results.json", br#"{"ok":true}"#),
                    ("files/deep/under.bin", &odd),
                ]),
            )
            .expect("the files are kept");

        // On the disk, where a reader outside Herd finds them (E-15, D8).
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        assert_eq!(
            fs::read(kept.join("files/results.json")).expect("the results file is there"),
            br#"{"ok":true}"#.to_vec(),
            "the path is preserved, not flattened"
        );
        assert_eq!(
            fs::read(kept.join("files/deep/under.bin")).expect("the nested file is there"),
            odd,
            "and the bytes are the bytes"
        );

        // On the record, as the summary a reader of `job.json` alone sees.
        let record = store
            .job(&job.id)
            .expect("the record reads")
            .expect("there");
        assert_eq!(
            record
                .outputs
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.bytes))
                .collect::<Vec<_>>(),
            [("files/results.json", 11), ("files/deep/under.bin", 6)]
        );

        // And back out, which is what a requester attaching later is given.
        let back = store.job_outputs(&job.id, true).expect("they read back");
        assert_eq!(back.refused, None);
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
        assert_eq!(
            back.files
                .iter()
                .map(|file| (file.path.as_str(), file.bytes().to_vec()))
                .collect::<Vec<_>>(),
            [
                ("files/results.json", br#"{"ok":true}"#.to_vec()),
                ("files/deep/under.bin", odd),
            ]
        );
    }

    /// A refusal is recorded as a refusal, with no file beside it.
    ///
    /// The thing being pinned is that the two never appear together: a caller
    /// that found a short list of files *and* a sentence saying the cap was
    /// exceeded would have a partial result set reading like a whole one.
    #[test]
    fn a_refusal_is_kept_with_its_figures_and_no_file() {
        let (_dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        let refusal = "this job's output is not in its record: 1 GiB in 3 file(s), and this \
                       runner's output cap is 8 MiB";
        store
            .record_outputs(
                &job.id,
                &herd_proto::JobOutputs::refusing(
                    refusal.to_owned(),
                    vec!["files/link is a symlink, and a symlink is never followed".to_owned()],
                ),
            )
            .expect("the refusal is recorded");
        let back = store.job_outputs(&job.id, true).expect("it reads back");
        assert!(back.files.is_empty(), "{back:?}");
        assert_eq!(back.refused.as_deref(), Some(refusal));
        assert_eq!(back.skipped.len(), 1, "{back:?}");
        let record = store.job(&job.id).expect("reads").expect("there");
        assert_eq!(record.outputs.refused.as_deref(), Some(refusal));
        assert!(record.outputs.files.is_empty());
    }

    /// A path that would climb out of the job's directory is **dropped and
    /// named**, and the verdict is not lost over it.
    ///
    /// The walk on a runner cannot produce one; a frame can, because a frame
    /// is a string a peer sent. What this pins is the pair of answers: the
    /// hub writes nothing outside the job's own directory, and
    /// `record_outputs` still returns `Ok` — because the next write in that
    /// chain is the verdict, and a malformed path must never cost a record
    /// its outcome (D19's rule, read the other way round).
    #[test]
    fn a_path_climbing_out_of_the_record_is_dropped_and_named_and_costs_no_verdict() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        store
            .record_outputs(
                &job.id,
                &collected(&[
                    ("../../escaped.json", b"this must never be written"),
                    ("/etc/herd-was-here", b"nor this"),
                    ("files/results.json", b"this one is fine"),
                ]),
            )
            .expect("a bad path is not an error: the verdict comes after this write");

        // Nothing outside the job's own directory, and nothing at all under
        // the two names.
        assert!(
            !dir.path().join("escaped.json").exists()
                && !dir.path().join(JOBS_DIR).join("escaped.json").exists(),
            "a path with `..` in it was written somewhere"
        );
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        assert_eq!(
            fs::read(kept.join("files/results.json")).expect("the good file is still kept"),
            b"this one is fine".to_vec(),
            "one bad path does not cost the files beside it"
        );

        // And each refusal is named on the record, so an operator sees it.
        let record = store.job(&job.id).expect("reads").expect("there");
        assert_eq!(
            record.outputs.files.len(),
            1,
            "only the good path is kept: {:?}",
            record.outputs.files
        );
        assert_eq!(
            record.outputs.skipped.len(),
            2,
            "both bad paths are named: {:?}",
            record.outputs.skipped
        );
        assert!(
            record
                .outputs
                .skipped
                .iter()
                .any(|said| said.contains("escaped.json")),
            "{:?}",
            record.outputs.skipped
        );
    }

    /// A job that kept nothing has no `outputs` in its record at all.
    ///
    /// This is the property that makes the whole change additive to the
    /// frozen record format (D73): every record written before the field
    /// existed reads as a job that kept nothing, and every job that keeps
    /// nothing writes a record byte-identical to the one it would have
    /// written before. [`RECORD_VERSION`] therefore does not move.
    #[test]
    fn a_job_that_keeps_nothing_writes_no_outputs_field_and_reads_back_silent() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        store
            .record_outputs(&job.id, &herd_proto::JobOutputs::default())
            .expect("nothing is recorded");
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(RECORD_FILE);
        let written: serde_json::Value =
            serde_json::from_str(&read(&path)).expect("the record is json");
        assert!(
            written.get("outputs").is_none(),
            "a silent collection must leave the record exactly as it was: {written}"
        );
        assert_eq!(written["record_version"], RECORD_VERSION);
        assert!(store
            .job_outputs(&job.id, true)
            .expect("it reads back")
            .is_silent());

        // And a record written without the field — which is every record on
        // the live state root — still reads.
        let older = written
            .to_string()
            .replace(",\"outputs\":", ",\"ignored\":");
        fs::write(&path, &older).expect("an older-shaped record is written");
        let record = store
            .job(&job.id)
            .expect("a record with no outputs field reads")
            .expect("there");
        assert!(record.outputs.is_silent());
    }

    /// A file the record names and the directory does not have is **named and
    /// still listed**, never a short list read as a whole one.
    ///
    /// The second half of that sentence is new, and it is the manifest
    /// outliving the bytes (D148). This test used to assert the entry was
    /// **dropped** and only a `skipped` line kept — which was the best
    /// available answer when a collection was one frame, and is the wrong one
    /// now: a caller that reads `files` to know what a job produced would
    /// read a dropped entry as a file the job never wrote. So the entry
    /// stays, with its size and its digest, and `data` being absent is what
    /// says the bytes are gone.
    #[test]
    fn a_file_named_on_the_record_and_missing_on_the_disk_is_said_rather_than_dropped() {
        let (dir, store) = store();
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned()],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("minted");
        store
            .record_outputs(&job.id, &collected(&[("files/results.json", b"mine")]))
            .expect("kept");
        fs::remove_file(
            dir.path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(KEPT_FILES_DIR)
                .join("files/results.json"),
        )
        .expect("somebody removed it from under the record");
        let back = store.job_outputs(&job.id, true).expect("it still reads");
        assert_eq!(back.files.len(), 1, "the entry stays: {back:?}");
        assert!(
            !back.files[0].is_held(),
            "and absent bytes are what says the file is gone, not a missing entry"
        );
        assert_eq!(back.files[0].len, 4, "its size is still on the record");
        assert_eq!(back.skipped.len(), 1, "{back:?}");
        assert!(
            back.skipped[0].contains("files/results.json")
                && back.skipped[0].contains("could not be read back"),
            "{:?}",
            back.skipped
        );
    }

    // ---------------------------------------------------------------- D152
    // The output budget and its sweeper.
    //
    // **These tests assert what is kept at least as hard as what goes.**
    // Deleting a requester's evidence is irreversible, so a sweeper that
    // evicts one collection too many is a worse defect than one that evicts
    // none at all — and a test that only counted what was freed would pass
    // for both.

    /// A store whose budget and floor are the test's own.
    fn store_holding(bytes: u64, floor: std::time::Duration) -> (TempDir, JobDirStore) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let budget = crate::job::Budget { bytes, floor };
        let store = JobDirStore::open_with(dir.path(), budget).expect("the state root opens");
        (dir, store)
    }

    /// A finished job holding one file of `bytes` bytes, by the inline
    /// carriage, with the outcome the test asked for.
    fn a_collection(store: &JobDirStore, bytes: usize, outcome: JobOutcome) -> JobId {
        let job = a_job(store);
        let data = vec![b'x'; bytes];
        store
            .record_outputs(&job.id, &collected(&[("files/results.json", &data)]))
            .expect("kept");
        store
            .record_state(&job.id, JobState::Finished { outcome })
            .expect("finished");
        job.id
    }

    /// Whether this job's bytes are still on the disk.
    fn bytes_are_there(dir: &TempDir, id: &JobId) -> bool {
        dir.path()
            .join(JOBS_DIR)
            .join(id.as_str())
            .join(KEPT_FILES_DIR)
            .join("files/results.json")
            .exists()
    }

    /// Make a record look older than it is, the way a week looks.
    ///
    /// Through the file, because there is no API for it and there must not
    /// be: nothing in Herd may move a job's own moments. The floor is judged
    /// against `updated_at`, which is when the job ended.
    fn aged_by(dir: &TempDir, id: &JobId, how_long: std::time::Duration) {
        let path = dir
            .path()
            .join(JOBS_DIR)
            .join(id.as_str())
            .join(RECORD_FILE);
        let mut record: serde_json::Value =
            serde_json::from_str(&read(&path)).expect("the record parses");
        let was = Timestamp::now().after(std::time::Duration::ZERO);
        let then = Timestamp::at_unix_seconds(was.unix_seconds() - how_long.as_secs() as i64);
        record["updated_at"] = serde_json::Value::String(then.to_string());
        fs::write(&path, serde_json::to_string(&record).expect("rewrites")).expect("written");
    }

    /// Nothing is removed while the hub is inside its budget — and nothing
    /// is removed for being old, either.
    ///
    /// The floor is a floor and not a lifetime (D148): a collection from
    /// last month is still there if there is room for it, because the only
    /// thing that makes room worth taking is a shortage of it.
    #[test]
    fn a_hub_inside_its_budget_removes_nothing_however_old_the_bytes_are() {
        let (dir, store) = store_holding(1 << 20, crate::job::OUTPUT_FLOOR);
        let old = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        aged_by(&dir, &old, std::time::Duration::from_secs(400 * 24 * 3600));

        let swept = store.sweep_outputs().expect("sweeps");
        assert!(
            swept.is_quiet(),
            "a hub with room to spare removes nothing: {swept:?}"
        );
        assert!(
            bytes_are_there(&dir, &old),
            "a collection from last year is still evidence while there is room for it"
        );
        assert_eq!(swept.held, 1024, "and the sweep measured what is there");
    }

    /// **The eviction order, which is the decision D152 is about.**
    ///
    /// Four collections, all the same size, all outside the floor, and only
    /// room for one. What goes is the passing job that somebody has already
    /// read; what survives is the failing job nobody has opened. Oldest
    /// first is the tie-break and never the rule — on its own it would have
    /// taken the red one, which is the collection the first consumer asked
    /// for this whole mechanism to keep.
    #[test]
    fn the_sweeper_takes_a_read_green_collection_before_an_unread_red_one() {
        // No floor, so the order alone decides: the floor's own rank is the
        // next test.
        let (dir, store) = store_holding(4096, std::time::Duration::ZERO);
        // Minted oldest first, so `updated_at` orders them the way they are
        // written here — and so a pure oldest-first sweeper would take
        // `green_read` and then `green_unread` and then `red_read`.
        let green_read = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        let green_unread = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        let red_read = a_collection(&store, 1024, JobOutcome::Exited { code: 1 });
        let red_unread = a_collection(&store, 1024, JobOutcome::Exited { code: 1 });
        for fetched in [&green_read, &red_read] {
            store.note_outputs_fetched(fetched).expect("noted");
        }
        // One more collection, which is what puts the hub over its budget.
        let newcomer = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });

        // Recording the newcomer swept already — the budget is enforced
        // where bytes land — so this asserts the state that left, not a
        // sweep of its own.
        let swept = store.sweep_outputs().expect("sweeps");
        assert!(
            swept.is_quiet(),
            "the hub is back inside its budget, so a second sweep has nothing to do: \
             {swept:?}"
        );
        assert!(
            !bytes_are_there(&dir, &green_read),
            "a passing job whose evidence has already been read goes first"
        );
        // And every one of these is what the sweeper had to leave alone.
        for kept in [&green_unread, &red_read, &red_unread, &newcomer] {
            assert!(
                bytes_are_there(&dir, kept),
                "{kept} must still be here: only one collection had to go, and the order \
                 says which"
            );
        }
        // The manifest of the one that went is untouched, which is the
        // whole of "the manifest outlives the bytes" (D148).
        let record = store
            .job(&green_read)
            .expect("reads")
            .expect("the record is still there");
        assert_eq!(
            record.outputs.files.len(),
            1,
            "the record still names every file it had: {record:?}"
        );
        assert_eq!(record.outputs.files[0].bytes, 1024);
        assert!(
            !record.outputs.files[0].sha256.is_empty(),
            "with its digest, which is what a judge re-reading it needs (E-54)"
        );
        let removed = record
            .outputs
            .removed
            .expect("and the record says what became of the bytes");
        assert_eq!(removed.bytes, 1024);
        assert!(
            removed.why.contains("removed") && removed.why.contains("4 KiB"),
            "the sentence names the date and the budget: {}",
            removed.why
        );
        assert!(
            removed.why.contains(&Timestamp::now().date()),
            "and the day it happened: {}",
            removed.why
        );
        // The job's own history carries it too, because that is where an
        // irreversible deletion belongs a week later (D151: the journal
        // gets one line per *sweep*).
        let events = store
            .job_events(&green_read)
            .expect("reads")
            .expect("the job has a history");
        let line = events
            .iter()
            .find_map(|event| match &event.what {
                JobEventKind::OutputsRemoved { files, bytes, why } => Some((*files, *bytes, why)),
                _ => None,
            })
            .expect("one line says the bytes went");
        assert_eq!((line.0, line.1), (1, 1024));
        assert_eq!(line.2, &removed.why, "the same sentence in both places");
    }

    /// Nothing inside the floor goes while anything outside it remains.
    ///
    /// Seven days is a promise, so it outranks every other key in the order:
    /// a *failing* collection from last month is taken before a *passing*
    /// one from this morning, which is the one place the outcome does not
    /// win.
    #[test]
    fn the_floor_outranks_the_rest_of_the_order() {
        let (dir, store) = store_holding(2048, crate::job::OUTPUT_FLOOR);
        let old_red = a_collection(&store, 1024, JobOutcome::Exited { code: 1 });
        aged_by(
            &dir,
            &old_red,
            std::time::Duration::from_secs(30 * 24 * 3600),
        );
        let fresh_green = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        let newcomer = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });

        assert!(
            !bytes_are_there(&dir, &old_red),
            "a collection past the floor goes before one inside it, whatever it says"
        );
        assert!(
            bytes_are_there(&dir, &fresh_green),
            "and this morning's is kept, because the floor is what it is"
        );
        assert!(bytes_are_there(&dir, &newcomer));
        let removed = store
            .job(&old_red)
            .expect("reads")
            .expect("record")
            .outputs
            .removed
            .expect("removed");
        assert!(
            !removed.why.contains("floor"),
            "nothing was promised about this one, so the sentence does not mention the \
             floor: {}",
            removed.why
        );
    }

    /// When the floor and the budget disagree, the budget wins and the hub
    /// says so, naming both figures.
    ///
    /// This is the one place a quiet policy would be a trap: a floor that
    /// broke silently is worse than no floor, because the next thing that
    /// happens is a full disk nobody saw coming (D148).
    #[test]
    fn the_budget_breaks_the_floor_and_the_sentence_names_both_figures() {
        let (dir, store) = store_holding(2048, crate::job::OUTPUT_FLOOR);
        // Everything here is minutes old, so the floor covers all of it.
        let first = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        let second = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        let third = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });

        assert!(
            !bytes_are_there(&dir, &first),
            "the budget is what the disk enforces, so the floor is broken rather than the \
             volume filled"
        );
        assert!(bytes_are_there(&dir, &second) && bytes_are_there(&dir, &third));
        let removed = store
            .job(&first)
            .expect("reads")
            .expect("record")
            .outputs
            .removed
            .expect("removed");
        assert!(
            removed.why.contains("floor")
                && removed.why.contains("2 KiB")
                && removed.why.contains("hour(s)"),
            "the sentence names the budget, the floor and how far inside it this was: {}",
            removed.why
        );
        // And the sweep reports it, which is what the hub's own line is
        // built from.
        let swept = store.sweep_outputs().expect("sweeps");
        assert!(swept.is_quiet(), "there is nothing left to do: {swept:?}");
    }

    /// A collection still arriving is never taken, however far over budget
    /// the hub is.
    ///
    /// Those bytes belong to the writer. Evicting them would destroy the
    /// thing the hub is in the middle of being given — and the runner, told
    /// nothing, would go on sending into a directory that keeps being
    /// deleted.
    #[test]
    fn a_collection_still_arriving_is_never_swept() {
        // A budget below what has already landed, so the sweeper really
        // does go looking rather than stopping at the first measurement.
        let (dir, store) = store_holding(4, std::time::Duration::ZERO);
        let files: &[(&str, &[u8])] = &[
            ("files/here.json", b"complete"),
            ("files/coming.bin", &[b'y'; 2048]),
        ];
        let job = a_job(&store);
        store
            .record_outputs(&job.id, &announced(files))
            .expect("the manifest lands");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("finished");
        // One of the two files arrives; the other never does.
        store
            .write_output_chunk(&job.id, "files/here.json", 0, b"complete")
            .expect("written");

        let swept = store.sweep_outputs().expect("sweeps");
        assert!(
            swept.is_quiet(),
            "a half-arrived collection is not the sweeper's to take: {swept:?}"
        );
        assert!(
            dir.path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(KEPT_FILES_DIR)
                .join("files/here.json")
                .exists(),
            "and the half that did arrive is still here"
        );
        assert_eq!(
            swept.could_not_free, 8,
            "the figure is reported rather than hidden, because an operator looking at a \
             volume that will not come down has to be able to see why: {swept:?}"
        );
    }

    /// Eviction takes the bytes and nothing else: the log, the history and
    /// every other field of the record are what they were.
    #[test]
    fn eviction_takes_the_bytes_and_leaves_the_rest_of_the_record() {
        let (dir, store) = store_holding(1024, std::time::Duration::ZERO);
        let job = a_job(&store);
        store
            .append_log(&job.id, LogStream::Stdout, 0, b"a line of output\n")
            .expect("logged");
        let data = vec![b'x'; 1024];
        store
            .record_outputs(&job.id, &collected(&[("files/results.json", &data)]))
            .expect("kept");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("finished");
        let before = store.job(&job.id).expect("reads").expect("record");
        // Something bigger arrives and the budget has to give.
        let _newcomer = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });

        assert!(!bytes_are_there(&dir, &job.id), "the bytes went");
        assert_eq!(
            store.job_log(&job.id).expect("reads"),
            Some(b"a line of output\n".to_vec()),
            "the log is not output files and has a cap of its own (D137): the sweeper \
             does not touch it"
        );
        let after = store.job(&job.id).expect("reads").expect("record");
        assert_eq!(after.state, before.state, "the verdict is untouched");
        assert_eq!(after.metrics, before.metrics);
        assert_eq!(
            after.outputs.files, before.outputs.files,
            "and so is every entry of the manifest"
        );
        // The directory is still shaped the way every job directory has been
        // since M2: `files/`, empty.
        let kept = dir
            .path()
            .join(JOBS_DIR)
            .join(job.id.as_str())
            .join(KEPT_FILES_DIR);
        assert!(
            kept.is_dir(),
            "files/ is put back empty, never left missing"
        );
        assert_eq!(
            durable::names_in(&kept).expect("lists").len(),
            0,
            "and there is nothing in it"
        );
        assert!(
            !dir.path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(OUTPUT_GONE_DIR)
                .exists(),
            "and nothing is left half-unlinked"
        );
    }

    /// A collection the sweeper took is not one a runner still owes.
    ///
    /// Without this the two mechanisms fight: an evicted collection fails
    /// the completeness test for the same reason an unfinished one does, so
    /// the hub would ask for the bytes again, sweep them again, and tell a
    /// requester to keep waiting for something that is never coming.
    #[test]
    fn an_evicted_collection_is_not_owed_by_anybody() {
        let (dir, store) = store_holding(1024, std::time::Duration::ZERO);
        let data = [b'z'; 1024];
        let job = a_job(&store);
        store
            .record_outputs(&job.id, &announced(&[("files/shot.png", &data)]))
            .expect("the manifest lands");
        store
            .record_state(
                &job.id,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("finished");
        store
            .write_output_chunk(&job.id, "files/shot.png", 0, &data)
            .expect("written");
        assert!(bytes_are_there_at(&dir, &job.id, "files/shot.png"));
        assert!(
            store.collections_owed().expect("reads").is_empty(),
            "a complete collection is owed by nobody"
        );

        let _newcomer = a_collection(&store, 1024, JobOutcome::Exited { code: 0 });
        assert!(!bytes_are_there_at(&dir, &job.id, "files/shot.png"));
        assert!(
            store.collections_owed().expect("reads").is_empty(),
            "and an evicted one is owed by nobody either, or the hub would ask its runner \
             to refill the disk the sweep just freed"
        );
    }

    fn bytes_are_there_at(dir: &TempDir, id: &JobId, path: &str) -> bool {
        dir.path()
            .join(JOBS_DIR)
            .join(id.as_str())
            .join(KEPT_FILES_DIR)
            .join(path)
            .exists()
    }

    /// A hub that stopped mid-eviction gives the disk back when it starts.
    ///
    /// `files.gone` is the rename that makes a removal atomic; a directory
    /// of that name on the disk is a hub that was killed between the rename
    /// and the unlink, and its bytes are nobody's evidence any more.
    #[test]
    fn a_half_finished_eviction_is_finished_at_the_next_start() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let budget = crate::job::Budget {
            bytes: 1 << 20,
            floor: std::time::Duration::ZERO,
        };
        let gone = {
            let store = JobDirStore::open_with(dir.path(), budget).expect("the state root opens");
            let job = a_job(&store);
            let left_behind = dir
                .path()
                .join(JOBS_DIR)
                .join(job.id.as_str())
                .join(OUTPUT_GONE_DIR);
            fs::create_dir_all(&left_behind).expect("made");
            fs::write(left_behind.join("orphan.bin"), [0u8; 4096]).expect("written");
            left_behind
        };
        assert!(gone.exists(), "the fixture really left one behind");

        let store = JobDirStore::open_with(dir.path(), budget).expect("the state root reopens");
        assert!(
            !gone.exists(),
            "a start finishes what an eviction began, so the disk comes back"
        );
        assert_eq!(
            store.sweep_outputs().expect("sweeps").held,
            0,
            "and those bytes were never counted as evidence"
        );
    }

    /// A requester that has read a collection is written down once and only
    /// once.
    #[test]
    fn a_fetch_is_written_down_once_and_never_again() {
        let (_dir, store) = store_holding(1 << 20, crate::job::OUTPUT_FLOOR);
        let id = a_collection(&store, 16, JobOutcome::Exited { code: 0 });
        assert!(store
            .job(&id)
            .expect("reads")
            .expect("record")
            .outputs
            .first_fetched_at
            .is_none());
        store.note_outputs_fetched(&id).expect("noted");
        let first = store
            .job(&id)
            .expect("reads")
            .expect("record")
            .outputs
            .first_fetched_at
            .expect("written down");
        store.note_outputs_fetched(&id).expect("noted again");
        assert_eq!(
            store
                .job(&id)
                .expect("reads")
                .expect("record")
                .outputs
                .first_fetched_at,
            Some(first),
            "the second fetch writes nothing: what the sweeper needs is the boolean, and a \
             record write per poll on a path that polls is what this avoids"
        );
    }
}
