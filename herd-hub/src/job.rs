//! What the hub records about a job, and the interface it records it through.
//!
//! These types are the shape of the files in a job's directory (D8), so they
//! are a compatibility surface: the first consumer reads a record while the
//! job runs (E-53) and re-judges it much later (E-54). Every file that carries
//! one of them carries [`RECORD_VERSION`] beside it, and a record written by a
//! newer Herd is refused by name rather than read as if it were this one.

use std::error::Error;
use std::fmt;

use herd_proto::{Declaration, JobId, JobOutcome, LabelSet, LogStream, Priority, StepRun};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a runner measured while a job ran.
///
/// There is one such type and it lives in `herd-proto` (invariant 4): the
/// figures travel from the runner to the hub in [`herd_proto::ToHub::JobFinished`]
/// and are written to the record unchanged. M2 reserved the field in the
/// record's shape and measured nothing; M4 fills it. A figure a host does not
/// expose stays `None` — never zero (D23).
pub use herd_proto::JobMetrics;

/// The version of the shape of a job's recorded data: `job.json` and every
/// line of `events.jsonl`. Bumped whenever an older reader could not read
/// what a newer Herd writes.
///
/// 2 since M6: a record carries the [`Priority`] its requester named (E-18).
/// It is a *required* field, not a defaulted one, so a record written before
/// M6 is refused by name — with its path and both version numbers — rather
/// than read as if its requester had asked for the middle level. A guessed
/// priority on a durable record is a lie a reader cannot see, and the version
/// probe in front of every record file exists for exactly this.
///
/// 3 since M7: an attempt carries the one line explaining why it was placed
/// where it was (E-36), and a record carries the per-build-thread memory its
/// requester declared, if it declared one (E-22). Both are required fields,
/// for the same reason the priority is: an attempt with no explanation is
/// precisely what E-36 forbids, and a reader must be able to tell "this job
/// asked for no derived `-j`" from "this record predates the question". A
/// pre-M7 state root is refused by name, with both figures, and no migration
/// is written for it (D73).
///
/// 4 since M8: a record carries the whole-job memory and the tmpfs its
/// requester declared (E-29, E-31). Required fields, and this time the reason
/// is load-bearing twice over: a running job's **declared** need is what
/// admission subtracts from the next job's room, read back off the record at
/// the start of every pass (D79), and a record that defaulted its two figures
/// to zero would read as a job holding nothing while it held gibibytes. A
/// pre-M8 state root is refused by name and no migration is written for it
/// (D73).
/// 5 since M9: a record carries its deadline, the one job it starts after and
/// the exit code its command means "requeue me" by (E-09, E-10, D76), and an
/// attempt says whether it ended *because of* that code. Required fields,
/// again, and the requeue code is the one that makes it load-bearing: the
/// budget for such a requeue is counted by reading the attempts off the record
/// every time, separately from E-21's two goes at a lost runner (D79), so a
/// record whose attempts could not say which kind of ending each one was would
/// let a leased machine spend the budget that exists for a runner that
/// disappears. A pre-M9 state root is refused by name and no migration is
/// written for it (D73).
///
/// 6 since M10: a record carries the declared command it is, if it is one, and
/// the time cap that command declared (E-11, D106, D108), and an attempt
/// carries one timestamped line per executor step (D77). The declaration is a
/// required field for the reason the others are — a reader must be able to
/// tell a job that was an ad-hoc command line from a record written before
/// commands were declared — and it is load-bearing beyond readability: the
/// declaration is **what a requeue onto another machine runs**, so a record
/// that lost it would re-run the job as a bare command line on a machine with
/// no workspace, and report whatever came out as the consumer's verdict. The
/// steps are `#[serde(default)]`, purely additive, and an attempt with none is
/// an attempt of a job that is not a declared command. A pre-M10 state root is
/// refused by name and no migration is written for it (D73).
pub const RECORD_VERSION: u32 = 6;

/// A moment, as the record files carry it: RFC 3339 in UTC, to the
/// nanosecond. A string a human and the consumer can both read, so that a
/// record stays meaningful with no tool but `cat`.
///
/// Wall-clock time can step backwards, so the *order* of a job's transitions
/// is the order of the lines in `events.jsonl`, never a comparison of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }

    /// How much time there is between `earlier` and this moment, and zero
    /// when there is none.
    ///
    /// Saturating, because this is wall-clock time: a host whose clock steps
    /// backwards would otherwise give a job a *negative* age, and aging
    /// computed from a negative age is aging that fires at once. A job can
    /// read as younger than it is for as long as the step lasts; it can
    /// never read as older than it is (E-18).
    pub fn since(self, earlier: Self) -> std::time::Duration {
        (self.0 - earlier.0).try_into().unwrap_or_default()
    }

    /// A moment given as seconds since the Unix epoch, as a deadline arrives
    /// on the wire (E-09).
    ///
    /// Saturating at the representable ends rather than refusing: a requester
    /// that asked for a deadline in the year 300 000 asked for "never", and a
    /// deadline so far in the past that it does not fit asked for "now". Both
    /// are answers; neither is a reason to drop a submission.
    pub fn at_unix_seconds(seconds: i64) -> Self {
        Self(
            time::OffsetDateTime::from_unix_timestamp(seconds)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
        )
    }

    /// The same moment as seconds since the Unix epoch.
    pub fn unix_seconds(self) -> i64 {
        self.0.unix_timestamp()
    }

    /// This moment, `how_long` later. For a caller that needs a moment it
    /// chooses rather than the one the clock has — a test of aging, above
    /// all, which must be able to make an hour pass without waiting one.
    pub fn after(self, how_long: std::time::Duration) -> Self {
        Self(self.0 + how_long)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self
            .0
            .format(&time::format_description::well_known::Rfc3339)
        {
            Ok(text) => f.write_str(&text),
            Err(_) => f.write_str("unformattable"),
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        time::serde::rfc3339::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        time::serde::rfc3339::deserialize(deserializer).map(Self)
    }
}

/// Where a job is in its life.
///
/// Tagged like a protocol message, and for the same reason: a later milestone
/// adds a state (M3's requeued attempt, M8's admission wait, M9's
/// cancellation) by adding a variant, and a state this Herd does not know is a
/// read error naming it rather than a job quietly read as something else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobState {
    /// Accepted by the hub, not yet handed to a runner.
    Queued,
    /// Sent to a runner, which has not reported starting it yet.
    Assigned,
    /// The runner reported that the process is running.
    Running,
    /// An attempt was under way and this hub stopped seeing it: the hub was
    /// killed or restarted while the job was assigned or running.
    ///
    /// Not terminal. The runner that holds the result may come back and
    /// redeliver it, which resolves the record normally (E-44); until then the
    /// job is unfinished, and what this state forbids is a job sitting in
    /// `running` for ever with nothing on its record saying why (D17). Deciding
    /// that the attempt is lost and requeueing it once is M3's.
    Interrupted { reason: String },
    /// The runner reported how the job ended. The outcome is kept whole, not
    /// flattened to a number: a persisted number would have to be migrated the
    /// first time a job ends some other way (D15).
    Finished { outcome: JobOutcome },
}

impl JobState {
    /// Whether the job is over. A terminal state is the last one a record ever
    /// has, which is what lets the store stop tracking it.
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Finished { .. })
    }

    /// The state's name as the record files spell it.
    pub fn name(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Assigned => "assigned",
            JobState::Running => "running",
            JobState::Interrupted { .. } => "interrupted",
            JobState::Finished { .. } => "finished",
        }
    }
}

/// One go at running a job on one runner.
///
/// A job is attempted once, and — when the runner it was on is lost — once
/// more (E-21). Both attempts stay on the record: a reader has to be able to
/// see that there was a first one, which runner it was on, and why it ended,
/// because "this job ran twice and the first machine died" is the single most
/// useful thing to know about a job that took twice as long as it should.
///
/// `ended` is the sentence that answers *why*, and it is set only when the
/// attempt ended without producing the job's verdict — lost, abandoned,
/// superseded. An attempt that produced the verdict needs no sentence: the
/// job's outcome is the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    /// 1 for the first, 2 for the one after a loss. Also what the runner
    /// carries on every frame about the job, so a late frame of attempt 1 is
    /// recognisable as late.
    pub number: u32,
    /// The runner the attempt was handed to, by the name it announced.
    pub runner: String,
    pub started_at: Timestamp,
    /// When this attempt stopped being the one the hub is waiting for,
    /// whichever way it ended: lost, abandoned, superseded — or finished,
    /// which is the ordinary way and was the one that used to be left unset.
    /// `None` means the attempt is under way, and nothing else does.
    pub ended_at: Option<Timestamp>,
    /// The declared requeue code this attempt ended **because of**, when that
    /// is why it ended (D76).
    ///
    /// The field that keeps two budgets apart. A job gets two goes at a runner
    /// that disappears (E-21, D39) and its own, separate budget of goes at a
    /// capability that turned out to be taken — and both are counted by
    /// reading these attempts back off the record every time, because a hub
    /// that kept counters would have two to leak instead of none (D79).
    /// Without something on the attempt saying which kind of ending it was,
    /// one leased machine answering 75 three times would leave a job with no
    /// attempts left for the case E-21 exists for.
    ///
    /// `None` on every other ending: a verdict, a loss, an abandonment.
    pub requeue_code: Option<i32>,
    /// Why this attempt ended without a verdict, when that is what happened.
    /// `None` on the attempt that produced one: the job's outcome is the
    /// answer, so there is nothing to excuse (see [`Attempt::ended_at`] for
    /// the moment, which is set either way).
    pub ended: Option<String>,
    /// The one line that explains this placement (E-36).
    ///
    /// Written once, when the attempt is numbered, and never rewritten: it is
    /// the decision as it was made, with the figures it was made from. "Every
    /// scheduling decision is explained in one line in the job's record" is
    /// E-36's whole text, and this is that line — on the attempt rather than
    /// on the job, because a job attempted twice was placed twice, and two
    /// placements have two explanations.
    ///
    /// A queued job's `waiting_for` is the other half and is not this: that
    /// one is rewritten whenever the job's place changes (M6), which is why a
    /// placement must not be recorded there.
    pub why: String,
    /// Every executor step this attempt ran, in order (D77).
    ///
    /// On the attempt rather than on the job, because a job attempted twice
    /// prepared twice — and the first consumer's `up` may pull a whole warm
    /// cache the first time a machine runs one of their jobs, which is
    /// minutes on one attempt and seconds on the next. Each line says the
    /// step's name, when it started, how long it took and what it exited
    /// with, and nothing reorders them.
    ///
    /// Filled as the attempt goes, from [`herd_proto::ToHub::StepFinished`],
    /// and taken over by the complete list the verdict carries. Empty for an
    /// attempt of a job that is not a declared command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<StepRun>,
}

/// What a requester asked for, as the hub takes it in.
///
/// One struct rather than a growing argument list: M6 adds a priority and a
/// deduplication key to a submission, and a store whose `create_job` took
/// four positional arguments would be rewritten at each one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    pub command: Vec<String>,
    /// The capabilities the job needs, by name (E-25). Canonical, because
    /// [`LabelSet`] sorts and deduplicates: M6's deduplication keys on them,
    /// and two submissions that named the same two labels in a different
    /// order are one submission.
    pub labels: LabelSet,
    /// Which of the four levels the requester asked for (E-18). No
    /// `Default`: see [`Priority`].
    pub priority: Priority,
    /// How much memory one build thread of this job needs, in MiB, when the
    /// requester asked for a derived `-j` (E-22). See
    /// [`herd_proto::ToHub::SubmitJob`].
    pub build_thread_mib: Option<u32>,
    /// How much memory the whole job needs, in MiB (E-29).
    pub memory_mib: Option<u64>,
    /// How much tmpfs it reserves while it runs, in MiB (E-31).
    pub tmpfs_mib: Option<u64>,
    /// When this job stops being worth starting (E-09). Absolute, on the
    /// hub's clock; see [`JobRecord::deadline`].
    pub deadline: Option<Timestamp>,
    /// The one job this one starts after (E-10).
    pub after: Option<JobId>,
    /// The exit code this job's command means "requeue me" by (D76).
    pub requeue_exit_code: Option<i32>,
    /// The declared command this job is, if it is one (E-11); see
    /// [`JobRecord::declaration`].
    pub declaration: Option<Declaration>,
    /// What that command declared its `run` step may take, in seconds (D108).
    pub timeout_s: Option<u64>,
}

impl NewJob {
    /// A job that needs no capability in particular, at the ordinary level.
    ///
    /// `new` and `needing` are for the callers that are not a requester —
    /// the store's and the hub's own tests, which are about something other
    /// than the level. Every path a real submission takes carries the level
    /// the requester named: the wire field is required, and
    /// [`NewJob::at`] is what the hub calls.
    pub fn new(command: Vec<String>) -> Self {
        Self::at(command, LabelSet::default(), Priority::Step)
    }

    /// A job that needs these capabilities, at the ordinary level.
    pub fn needing(command: Vec<String>, labels: LabelSet) -> Self {
        Self::at(command, labels, Priority::Step)
    }

    /// A job as a requester asked for it: its command, its capabilities and
    /// its level.
    pub fn at(command: Vec<String>, labels: LabelSet, priority: Priority) -> Self {
        Self {
            command,
            labels,
            priority,
            build_thread_mib: None,
            memory_mib: None,
            tmpfs_mib: None,
            deadline: None,
            after: None,
            requeue_exit_code: None,
            declaration: None,
            timeout_s: None,
        }
    }

    /// The same, with M9's three: when it stops being worth starting, the one
    /// job it starts after, and the code its command means "requeue me" by
    /// (E-09, E-10, D76).
    pub fn with_m9(
        mut self,
        deadline: Option<Timestamp>,
        after: Option<JobId>,
        requeue_exit_code: Option<i32>,
    ) -> Self {
        self.deadline = deadline;
        self.after = after;
        self.requeue_exit_code = requeue_exit_code;
        self
    }

    /// The same, as the declared command a requester named (E-11): the four
    /// executor steps it runs through, and the time cap its command declared
    /// (D106, D108).
    pub fn declared(mut self, declaration: Option<Declaration>, timeout_s: Option<u64>) -> Self {
        self.declaration = declaration;
        self.timeout_s = timeout_s;
        self
    }

    /// The same, with the memory one build thread needs, so that the hub
    /// derives this job's `-j` from whatever machine it lands on (E-22).
    pub fn with_build_thread_mib(mut self, mib: Option<u32>) -> Self {
        self.build_thread_mib = mib;
        self
    }

    /// The whole-job memory and the tmpfs the requester declared (E-29,
    /// E-31). Both optional and neither read as zero when absent.
    pub fn with_admission(mut self, memory_mib: Option<u64>, tmpfs_mib: Option<u64>) -> Self {
        self.memory_mib = memory_mib;
        self.tmpfs_mib = tmpfs_mib;
        self
    }

    /// The key E-05 deduplicates on: what makes two submissions *the same
    /// work*.
    ///
    /// The command and the labels, and nothing else. Not the priority — two
    /// agents asking for one build at two levels are still asking for one
    /// build, and the higher level wins (E-05). Not the time, not the
    /// requester. The commit will join it when a submission names one
    /// (E-01, M10): it is part of the command's meaning, and today the
    /// command is where it is.
    ///
    /// **Length-prefixed, never concatenated.** `["a b", "c"]` and
    /// `["a", "b c"]` are different work, and any key built by joining
    /// strings with a separator makes them equal — whatever separator is
    /// chosen, since a command may contain it. Each piece is written as its
    /// length in bytes, a colon, then the bytes, so the encoding can be
    /// parsed back and two different lists can never produce one key. The
    /// labels go through the same encoding and are already sorted and
    /// deduplicated by [`LabelSet`], so naming two labels in the other order
    /// is the same work.
    pub fn dedup_key(&self) -> String {
        dedup_key(&self.command, &self.labels)
    }
}

/// The key E-05 deduplicates on, for a command and a label set that are
/// already somewhere — on a record, say. See [`NewJob::dedup_key`].
pub fn dedup_key(command: &[String], labels: &LabelSet) -> String {
    let mut key = String::new();
    key.push_str("command ");
    for piece in command {
        push_counted(&mut key, piece);
    }
    key.push_str(" labels ");
    for label in labels.iter() {
        push_counted(&mut key, label);
    }
    key
}

/// One piece of a key, as its byte length and its bytes.
fn push_counted(key: &mut String, piece: &str) {
    key.push_str(&piece.len().to_string());
    key.push(':');
    key.push_str(piece);
}

/// What the hub knows about one job: the contents of its `job.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub command: Vec<String>,
    /// The capabilities this job needs (E-25). Written at birth and never
    /// changed: what a job asked for is part of what it was.
    #[serde(default)]
    pub labels: LabelSet,
    /// The level this job is weighed at (E-18).
    ///
    /// What the requester asked for, raised since by an operator (E-18's
    /// raise) and by nothing else. **Aging is not written here**: a job's
    /// effective level is computed from this one and the job's age every time
    /// the queue is ordered, so there is no periodic write, nothing to lose
    /// in a crash, and no record claiming a level a requester never asked
    /// for. See `crate::queue`.
    ///
    /// Required, not `#[serde(default)]`: a record with no priority is a
    /// record from before M6 and is refused by its version, never read as if
    /// its requester had asked for the middle level.
    pub priority: Priority,
    /// How much memory one build thread of this job needs, in MiB, when its
    /// requester asked for its `-j` to be derived (E-22). `None` when it
    /// asked for nothing of the kind.
    ///
    /// Required, not `#[serde(default)]`, for the reason the priority is: a
    /// reader has to be able to tell a job that wanted no derivation from a
    /// record written before the question existed. The figure is the
    /// requester's; the `-j` derived from it is the field below.
    pub build_thread_mib: Option<u32>,
    /// How much memory this whole job declares it needs, in MiB (E-29).
    ///
    /// The figure admission weighs, and — once the job is out on a runner —
    /// the figure the **next** job's admission subtracts, read back off this
    /// record at the start of every pass (D79). It is the *declaration* and
    /// never a measurement: a job admitted a second ago has allocated nothing
    /// yet, so costing what a job uses would let a second job in beside it
    /// and let both then grow. A job that exceeds its declaration is stopped
    /// by M4's cap, which is what makes the declaration the honest unit.
    ///
    /// Mutually exclusive with [`JobRecord::build_thread_mib`], which is the
    /// per-thread figure a build declares: the whole-job need of such a job is
    /// that figure times the `-j` it was given.
    pub memory_mib: Option<u64>,
    /// How much room on its runner's tmpfs this job reserves while it runs,
    /// in MiB (E-31).
    ///
    /// Reserved before the job starts and released by the write that ends the
    /// job, because the reservation *is* this field on a record whose attempt
    /// is open — there is no counter to decrement and so none to leak (D79).
    /// `Some(0)` is a real declaration and so is absence: the first
    /// consumer's `build` and `check` need no tmpfs at all.
    pub tmpfs_mib: Option<u64>,
    /// The `-j` this job was actually given, derived from the machine the hub
    /// placed it on (E-22). `None` until it is placed, and `None` for ever on
    /// a job that asked for no derivation.
    ///
    /// **Written at the claim, before the job is handed over**, and therefore
    /// before anything can read what the job needs. That ordering is
    /// load-bearing rather than cosmetic: the first consumer's `--labels`
    /// reading computes its memory and tmpfs figures *for the N it was given*
    /// (M10, D82), so a design that chose N after reading the requirements
    /// would be circular — N from memory, memory from a reading that needs N.
    /// N first, requirements second, placement third.
    ///
    /// A requeue onto a differently sized machine (E-21) derives its own
    /// figure and rewrites this one; the line on each attempt keeps the
    /// arithmetic of that attempt.
    pub parallelism: Option<u32>,
    /// When this job stops being worth starting (E-09).
    ///
    /// **Absolute, and on the hub's clock.** The requester's CLI turns a
    /// human's `05:00` or `+15m` into an instant and sends the instant; the
    /// hub records it as it received it and judges it against its own clock,
    /// which is the only clock in Herd that decides anything. An offset from
    /// the submission would have moved the consumer's 05:00 every time the
    /// submission itself was late, and a deadline read against the
    /// requester's clock would be a job cancelled because a laptop's time
    /// zone was wrong.
    ///
    /// It bounds **waiting** and nothing else. A queued job past it ends
    /// `cancelled`, with the reason naming the figures *and what it was
    /// waiting for when the moment came*; a job that has started is never
    /// touched by it (M4's time cap is what bounds a running job, exit 124).
    /// It is also the only bound D88 leaves on a job no connected runner could
    /// ever fit.
    ///
    /// Required, not `#[serde(default)]`, for the reason the priority is: a
    /// reader must be able to tell a job whose requester asked for no deadline
    /// from a record written before deadlines existed.
    pub deadline: Option<Timestamp>,
    /// The one job this one starts after (E-10).
    ///
    /// At most one, and the edge is "after it **succeeds**": a parent that
    /// ends any other way ends this job `cancelled`, naming the parent and how
    /// it ended. One edge and no conditions, because two would be a graph and
    /// a graph with conditions on it is a workflow language (invariant 1).
    ///
    /// The parent has to exist **already** when this job is submitted, which
    /// is what makes a cycle unrepresentable rather than merely refused: ids
    /// are minted by the hub in order, so a job can only ever name one that is
    /// older than itself.
    pub after: Option<JobId>,
    /// The exit code this job's command means "the capability I was given went
    /// away under me" by (D76). 75 for the first consumer.
    ///
    /// Not a verdict: such a code causes a bounded requeue with the capability
    /// re-probed. Absent means every code the command exits with is its own
    /// verdict, passed through untouched (D20).
    pub requeue_exit_code: Option<i32>,
    /// The declared command this job is, when it is one (E-11, D106).
    ///
    /// Written at birth from what `herdctl` resolved in the requester's own
    /// checkout, and never changed: the commit, the arguments and the four
    /// executor hooks are part of what this job *was*. It is handed back with
    /// every attempt, which is the whole reason it is on the record rather
    /// than held by the runner that first saw it — a job requeued onto another
    /// machine (E-21) has to run the same four steps on the same commit, and
    /// nothing else in Herd remembers them.
    ///
    /// `None` is an ad-hoc command line: [`JobRecord::command`] is then the
    /// whole of what runs. Required, not `#[serde(default)]`, so that a reader
    /// can tell the two apart from a record written before M10.
    pub declaration: Option<Declaration>,
    /// How long this job's `run` step may take, in seconds, as its command
    /// declared (E-11, D108).
    ///
    /// The runner caps the step at the smaller of this and its own figure: the
    /// consumer says what its command needs, the owner of the machine says
    /// what her machine lends, and neither raises the other. `None` is a job
    /// bounded by the runner's figure alone, which is every job before M10.
    pub timeout_s: Option<u64>,
    /// How many submissions this one execution answers (E-05).
    ///
    /// 1 for an ordinary job; 2 after a second identical submission joined
    /// it; 3 after a third. It is the operator-visible proof that two
    /// requesters got one result id out of one run, and `events.jsonl` keeps
    /// the moment each of them joined.
    #[serde(default = "one")]
    pub submissions: u32,
    pub state: JobState,
    /// Why the job has not started, while it has not.
    ///
    /// A waiting job is still `queued` — waiting is not being refused (D38),
    /// and the state machine must not grow a variant for it or every later
    /// milestone would have to handle "queued but differently". The reason
    /// lives beside the state so that a reader of `job.json` alone sees why
    /// nothing is happening (E-53), and it is cleared the moment the job
    /// leaves the queue. `events.jsonl` keeps the history of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<String>,
    /// When the hub minted the job.
    pub submitted_at: Timestamp,
    /// When its state last changed.
    pub updated_at: Timestamp,
    /// Every go at running this job, in order (E-21). Empty while the job is
    /// only queued, one entry in the ordinary case, two when the first
    /// runner was lost.
    ///
    /// `#[serde(default)]`, so a record written before M3 reads as a job with
    /// no attempt recorded rather than as a read error. [`RECORD_VERSION`]
    /// stays 1 for the same reason D15 and D18 kept `PROTOCOL_VERSION` at 1:
    /// the field is purely additive, an older reader ignores it, and nothing
    /// is deployed to read it wrongly.
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    /// What the runner measured while the job ran (E-15). Reserved in the
    /// record's shape by M2 and filled by M4: every figure is unset until the
    /// runner reports it, which it does once, with the outcome.
    pub metrics: JobMetrics,
}

/// What `submissions` is on a record that predates the field. One, because
/// one submission is what every record ever written meant.
fn one() -> u32 {
    1
}

impl JobRecord {
    /// The key this job would be joined on (E-05), derived from its own
    /// fields.
    ///
    /// Derived rather than stored, on purpose: a key kept beside the fields
    /// it is made of is a second copy that can disagree with them, and the
    /// disagreement would silently join two submissions that are not the
    /// same work. The hub's index of open keys is rebuilt from the records at
    /// every start, through this.
    pub fn dedup_key(&self) -> String {
        dedup_key(&self.command, &self.labels)
    }

    /// How much memory this job declares it needs, in MiB, given the `-j` it
    /// has been or would be given (E-29).
    ///
    /// Three answers, and the third is the one that matters. `Ok(Some(mib))`
    /// is a declared figure. `Ok(None)` is a job that declares no memory need
    /// at all, which is a real state — `herdctl submit -- true` — and is
    /// admitted on its capabilities alone. `Err` is a need that **cannot be
    /// costed**: a job declaring what one build thread needs, with no `-j` to
    /// multiply it by. That is never read as zero, here or anywhere: a job out
    /// on a runner whose holding cannot be costed stops the hub admitting
    /// anything more there, by name, rather than reading as a machine holding
    /// nothing.
    pub fn memory_need_mib(
        &self,
        threads: Option<u32>,
    ) -> std::result::Result<Option<u64>, String> {
        if let Some(mib) = self.memory_mib {
            return Ok(Some(mib));
        }
        let Some(per_thread) = self.build_thread_mib else {
            return Ok(None);
        };
        match threads {
            Some(threads) => Ok(Some(u64::from(per_thread) * u64::from(threads))),
            None => Err(format!(
                "it needs {per_thread} MiB per build thread and no -j is known for it, so what \
                 it holds cannot be costed"
            )),
        }
    }

    /// The same, for a job that is already out on a runner: the `-j` on its
    /// own record is the figure it was given (D82).
    pub fn memory_held_mib(&self) -> std::result::Result<Option<u64>, String> {
        self.memory_need_mib(self.parallelism)
    }

    /// The tmpfs this job reserves, in MiB (E-31). Absent is no reservation.
    pub fn tmpfs_need_mib(&self) -> u64 {
        self.tmpfs_mib.unwrap_or(0)
    }

    /// The record a job **would** have, for a dry run to be judged on (E-07).
    ///
    /// Not minted and never written. Its id is
    /// [`JobRecord::DRY_RUN_ID`], which is not a shape this hub's counter can
    /// produce, so a record carrying it cannot be mistaken for one of this
    /// hub's jobs by anything that reads a state root — and an id that *could*
    /// collide would be the one way a dry run could touch a real job.
    ///
    /// Everything else is what the store would have written: the same
    /// [`NewJob`], queued, submitted now, no attempt, no `-j`.
    pub fn hypothetical(new: NewJob, now: Timestamp) -> Self {
        Self {
            id: JobId::new(Self::DRY_RUN_ID),
            command: new.command,
            labels: new.labels,
            priority: new.priority,
            build_thread_mib: new.build_thread_mib,
            memory_mib: new.memory_mib,
            tmpfs_mib: new.tmpfs_mib,
            deadline: new.deadline,
            after: new.after,
            requeue_exit_code: new.requeue_exit_code,
            declaration: new.declaration,
            timeout_s: new.timeout_s,
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at: now,
            updated_at: now,
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
        }
    }

    /// The id a job that is only being asked about carries (E-07).
    ///
    /// Deliberately not of the form this hub mints (`j` and ten digits), so
    /// that it can never name a real job: the one way a dry run could change
    /// anything is by being mistaken for one.
    pub const DRY_RUN_ID: &'static str = "(a dry run, never minted)";

    /// How many goes at this job a runner has taken away with it (E-21, D39).
    ///
    /// Read off the attempts every time it is asked, never counted (D79), and
    /// it counts only the attempts that ended with no verdict for a reason
    /// that is *about the runner*: lost, abandoned, superseded. An attempt
    /// that ended because the job answered its declared requeue code is not
    /// one of these, which is the whole of D76's "separate budget" — a leased
    /// machine answering 75 must not spend the goes that exist for a machine
    /// that disappears.
    pub fn goes_a_runner_took(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| attempt.ended_at.is_some() && attempt.requeue_code.is_none())
            .count() as u32
    }

    /// How much of the requeue budget this job has spent (D76).
    ///
    /// Derived from the attempts, like everything else a pass needs: there is
    /// no counter, so a hub restart rebuilds nothing and a crash mid-requeue
    /// cannot leave a budget that has quietly grown back.
    pub fn requeues_spent(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| attempt.requeue_code.is_some())
            .count() as u32
    }

    /// The runners that answered this job's declared requeue code, in order,
    /// each named once (D76).
    pub fn runners_that_requeued_it(&self) -> Vec<String> {
        let mut named: Vec<String> = Vec::new();
        for attempt in &self.attempts {
            if attempt.requeue_code.is_some() && !named.contains(&attempt.runner) {
                named.push(attempt.runner.clone());
            }
        }
        named
    }

    /// The attempt that is current, if one is.
    pub fn attempt(&self) -> Option<&Attempt> {
        self.attempts.last()
    }

    /// The number of the attempt that is current: 0 before the first one, so
    /// that "which attempt is this frame about" always has an answer.
    pub fn attempt_number(&self) -> u32 {
        self.attempts.last().map_or(0, |attempt| attempt.number)
    }

    /// Whether a frame a runner sent about `attempt` concerns the attempt the
    /// hub is waiting for.
    ///
    /// False for an attempt that has been superseded by a later one, and false
    /// for one that has ended — a runner declared lost and come back may not
    /// finish what the hub has already written off (E-21), and neither may
    /// anybody go on talking about an attempt that produced its job's verdict.
    ///
    /// What is tested is [`Attempt::ended_at`] and not [`Attempt::ended`].
    /// `ended` is a sentence, and the attempt that produces a verdict has
    /// nothing to excuse — so testing it made every normally-finished job's
    /// attempt read as current for ever, which is how a late frame bearing
    /// the *correct* runner name got through the ownership gate.
    pub fn attempt_is_current(&self, attempt: u32) -> bool {
        self.attempts
            .last()
            .is_some_and(|current| current.number == attempt && current.ended_at.is_none())
    }
}

/// One line of `events.jsonl`: a moment, and what happened at it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobEvent {
    pub at: Timestamp,
    #[serde(flatten)]
    pub what: JobEventKind,
}

impl JobEvent {
    /// A transition, which is what most lines are.
    pub fn transition(at: Timestamp, state: JobState) -> Self {
        Self {
            at,
            what: JobEventKind::Transition { state },
        }
    }

    /// Output that is missing from the job's log, named on the record.
    pub fn log_gap(at: Timestamp, first_missing: u64, last_missing: u64) -> Self {
        Self {
            at,
            what: JobEventKind::LogGap {
                first_missing,
                last_missing,
            },
        }
    }

    /// An attempt handed to a runner, and the one line explaining why that
    /// runner (E-36).
    pub fn attempt_started(at: Timestamp, attempt: u32, runner: &str, why: &str) -> Self {
        Self {
            at,
            what: JobEventKind::AttemptStarted {
                attempt,
                runner: runner.to_owned(),
                why: why.to_owned(),
            },
        }
    }

    /// An attempt that ended without producing the job's verdict, and why.
    pub fn attempt_ended(at: Timestamp, attempt: u32, reason: &str) -> Self {
        Self {
            at,
            what: JobEventKind::AttemptEnded {
                attempt,
                reason: reason.to_owned(),
            },
        }
    }

    /// Why a queued job is not starting, named on the record (E-26, E-04).
    pub fn waiting(at: Timestamp, reason: String) -> Self {
        Self {
            at,
            what: JobEventKind::Waiting { reason },
        }
    }

    /// A second identical submission joined this job rather than minting a
    /// new one (E-05).
    pub fn joined(at: Timestamp, submissions: u32, asked_for: Priority) -> Self {
        Self {
            at,
            what: JobEventKind::Joined {
                submissions,
                asked_for,
            },
        }
    }

    /// The job's level moved up, and why.
    pub fn priority_raised(at: Timestamp, from: Priority, to: Priority, why: String) -> Self {
        Self {
            at,
            what: JobEventKind::PriorityRaised { from, to, why },
        }
    }

    /// The state the job moved to, when this line is a transition, and `None`
    /// when it records something else.
    pub fn state(&self) -> Option<&JobState> {
        match &self.what {
            JobEventKind::Transition { state } => Some(state),
            JobEventKind::LogGap { .. }
            | JobEventKind::AttemptStarted { .. }
            | JobEventKind::AttemptEnded { .. }
            | JobEventKind::Joined { .. }
            | JobEventKind::PriorityRaised { .. }
            | JobEventKind::Waiting { .. } => None,
        }
    }
}

/// What one line of `events.jsonl` records.
///
/// A job's history is not only its transitions. Output that never reached the
/// hub is part of what happened to the job, and a log that is a quarter short
/// with nothing on the record saying so is a record that lies by omission. So
/// a line says which kind it is, in a `event` field, and a reader that wants
/// the transitions reads the `transition` lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEventKind {
    /// The job moved to a new state.
    Transition { state: JobState },
    /// The job was handed to a runner, as this attempt of it (E-21), with the
    /// one line that explains the placement (E-36).
    ///
    /// **One of these per placement, and exactly one.** It is what "a day's
    /// decisions are greppable" means: `grep claimed` over the state root
    /// gives one line per job that ran, with the runner, the labels, the
    /// slots and any derived `-j` in it. Nothing else in the record writes
    /// that word.
    AttemptStarted {
        attempt: u32,
        runner: String,
        why: String,
    },
    /// That attempt ended without the job's verdict, and this is why: the
    /// runner went silent, it was asked to stop, the hub gave the attempt up.
    /// The job itself may go on — requeued once — or end here; the transition
    /// line that follows says which.
    AttemptEnded { attempt: u32, reason: String },
    /// Chunks of the job's output never made it into the log, and the log
    /// names the gap at the point where they are missing.
    ///
    /// Every chunk carries a per-job sequence number (D4), which is there for
    /// exactly this: a number that is not the successor of the last one the
    /// hub wrote means the chunks in between are gone — the control
    /// connection dropped under a running job, or the hub could not write
    /// one. M3 holds the chunks on the runner until the hub acknowledges
    /// them and asks for them again, so a link that drops costs the log
    /// nothing; this line is for what is missing even so — chunks the runner
    /// no longer holds, or a runner that never came back. A silently short
    /// log is worse than a short log that says so.
    LogGap {
        /// The number of the first chunk that is missing.
        first_missing: u64,
        /// The number of the last one, inclusive.
        last_missing: u64,
    },
    /// A second submission asked for work this job is already doing, and
    /// joined it instead of minting a job of its own (E-05).
    ///
    /// On the record because one execution answering two requesters is the
    /// single most surprising thing about a job id a caller did not expect
    /// to be given: the line says when it happened, how many submissions the
    /// job now answers, and which level the joiner asked for — which is what
    /// explains the `priority_raised` line that may follow it.
    Joined {
        submissions: u32,
        asked_for: Priority,
    },
    /// The job's level moved up, and why: an operator raised it (E-18), or a
    /// submission that joined it asked for more than it had (E-05).
    ///
    /// Only ever upwards, and only ever written here — aging is not a write.
    PriorityRaised {
        from: Priority,
        to: Priority,
        why: String,
    },
    /// The job is queued and the hub tried to start it and could not, and
    /// this is the sentence why (E-26, E-04).
    ///
    /// Not a state and not a failure. A capability something outside Herd is
    /// holding suspends it, and the job that needs it waits — with this line
    /// on its record, so that "nothing is happening" is never silent and
    /// never has to be said with `Refused`, which is terminal (D13, D38).
    /// Written only when the reason *changes*, so a job that waits an hour
    /// for one lock file leaves one line rather than a thousand.
    Waiting { reason: String },
}

/// A job store failed, with the path or the id that failed in the message: the
/// hub's state is files, and a filesystem failure that does not name what it
/// was touching cannot be acted on.
#[derive(Debug)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for StoreError {}

pub type Result<T> = std::result::Result<T, StoreError>;

/// What became of a log chunk the store was handed.
///
/// A runner holds the chunks it has sent until the hub acknowledges them, so
/// the hub does receive the same chunk twice: once on the link that died
/// before the acknowledgement got out, once on the link that came after. The
/// second one must not be appended — a log with a paragraph written twice is
/// as wrong as a log with a paragraph missing — and the hub must not relay it
/// to a requester that already saw it either, which is why the answer comes
/// back rather than being swallowed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogWrite {
    /// New output, now in the log.
    Recorded,
    /// The store already had this chunk, and nothing was written.
    AlreadyRecorded,
}

/// Minting one job, for a caller that is not a requester.
///
/// The hub's own path is [`JobStore::submit`], which may join (E-05), and
/// everything a requester sends goes through it. A test that is about
/// something else — a transition, a log, a restart — wants one job per call
/// and would be quietly testing a *join* if two of its jobs happened to
/// carry the same command and labels. So this says so: it goes through the
/// one door, and a submission that joined is an error naming the job it
/// joined, never a record handed back as if it were new.
pub trait Mint {
    fn create_job(&self, new: NewJob) -> Result<JobRecord>;
}

impl<S: JobStore + ?Sized> Mint for S {
    fn create_job(&self, new: NewJob) -> Result<JobRecord> {
        match self.submit(new)? {
            Submission::Minted(record) => Ok(record),
            Submission::Joined { record, .. } => Err(StoreError::new(format!(
                "this submission joined {}, which is already doing that work, so no \
                 job was minted; a caller that needs two jobs has to ask for two \
                 different pieces of work",
                record.id
            ))),
        }
    }
}

/// What became of a submission (E-05).
#[derive(Debug, Clone, PartialEq)]
pub enum Submission {
    /// A job of its own, minted now.
    Minted(JobRecord),
    /// It joined a job that is already doing this work, and the record is
    /// that job's. Both callers hold one id and will be given one result.
    ///
    /// **Only an open job is ever joined.** A submission whose work was done
    /// an hour ago mints a new job and runs again: see D70 — handing back a
    /// finished verdict would make a rerun impossible to ask for, and would
    /// make a second caller wear a `cancelled` or `lost` 125 for a job it
    /// never submitted (D40).
    Joined {
        record: JobRecord,
        /// The level the joined job was raised to by this submission, when
        /// the joiner asked for more than the job had (E-05: the higher
        /// priority wins).
        raised: Option<Priority>,
    },
}

impl Submission {
    /// The record either way: the job this submission is now about.
    pub fn record(&self) -> &JobRecord {
        match self {
            Submission::Minted(record) | Submission::Joined { record, .. } => record,
        }
    }
}

/// What became of a request to raise a job's level (E-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raise {
    /// It moved, and these are the two levels.
    Raised { from: Priority, to: Priority },
    /// The job is already at this level or above it. Nothing was written,
    /// and nothing is wrong.
    AlreadyThere(Priority),
    /// The level asked for is *below* the one the job has. Refused rather
    /// than done: lowering a job by hand is how something gets starved on
    /// purpose, and E-18 offers a raise and nothing else.
    NotARaise { from: Priority, asked: Priority },
}

/// Everything the hub does to a job's state.
///
/// Deliberately boring and synchronous: the writes are small files, and the
/// hub's correctness depends on a transition being on the disk before the
/// hub acts on it — an acknowledgement to a runner, above all.
/// **Every writer here refuses a job that is over.** A terminal record is the
/// last thing a job ever has — its outcome, its measurements and its log are
/// closed together — so `record_state`, `record_metrics`, `begin_attempt`,
/// `end_attempt`, `append_log` and `name_missing_log` all answer a finished
/// job with an error naming it, and the hub refuses such a frame by name
/// before it ever gets here. What is terminal is the *job*: a first attempt
/// ending is how a requeue begins (E-21), and that record is `queued` a
/// moment later.
pub trait JobStore: Send + Sync + 'static {
    /// Who the state this store keeps belongs to, as the handshake carries it
    /// (see `JobDirStore::state_root_id`).
    ///
    /// On the trait because the hub puts it in `Welcome`, and because a job id
    /// only means anything inside one state: `j0000000001` of a root that was
    /// recreated is a different job with the same name. A store with no
    /// durable identity of its own answers with something stable for its own
    /// lifetime; what it must never do is answer with a string another store
    /// could also answer with.
    fn state_root_id(&self) -> &str;

    /// Take a submission in: mint a job for it, or join it to the one
    /// already doing that work (E-05).
    ///
    /// One method, because the decision and the mint have to be one step. A
    /// store that answered "is there an open job with this key?" and then
    /// minted would let two submissions arriving together both mint, which
    /// is the defect deduplication exists to prevent — and the two
    /// submissions that race are exactly the two agents building one commit
    /// that E-05 is about.
    ///
    /// Only an **open** job is joined: see [`Submission`].
    fn submit(&self, new: NewJob) -> Result<Submission>;

    /// Raise one job's level, as an operator may (E-18).
    ///
    /// Upwards only, and never on a job that is over: both are answered by
    /// name — the first here, the second by the store's one write door,
    /// which refuses every write to a terminal record (D54). A level that is
    /// already at least the one asked for is not an error and not a write.
    fn raise_priority(&self, id: &JobId, to: Priority) -> Result<Raise>;

    /// Record why a queued job is not starting, and say whether that was
    /// news.
    ///
    /// `false` means the reason is already the one on the record, or the job
    /// is not queued any more — nothing was written. The hub calls this on
    /// every dispatch attempt, so writing unconditionally would turn a job
    /// waiting on one lock file into an `events.jsonl` nobody can read.
    fn record_waiting(&self, id: &JobId, reason: &str) -> Result<bool>;

    /// Record a transition, with the moment it happened. Unknown ids are an
    /// error, not a silent no-op, and so is a job that is already over — a
    /// runner redelivering a result whose acknowledgement it never saw is
    /// told by name that the record holds its end already, and drops it
    /// (D42), rather than writing the same ending twice.
    fn record_state(&self, id: &JobId, state: JobState) -> Result<()>;

    /// Record what the runner measured (E-15). Separate from the transition
    /// because a runner reports figures the hub never computes, and because a
    /// store that keeps no measurements is still a correct store.
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> Result<()>;

    /// Record the start of an attempt on a job, and give it its number.
    ///
    /// One call so that the attempt, the transition to
    /// [`JobState::Assigned`] and the reset of the job's chunk count are one
    /// step: a second attempt numbers its output from zero again, and a hub
    /// that had recorded the attempt without resetting the count would read
    /// the new attempt's first chunk as a gap.
    /// `why` is the one line explaining the placement, which goes on the
    /// attempt and into `events.jsonl` with it (E-36). It is taken here
    /// rather than written afterwards because an attempt whose explanation
    /// could be written separately is an attempt that can exist without one.
    /// `parallelism` is the `-j` derived for the machine this attempt is
    /// going to, or `None` when the job asked for no derivation. It is
    /// written onto the **record** rather than only into `why`, because it
    /// has to travel with the job: the first consumer's requirements reading
    /// (M10) is a function of the arguments it is given, `-j N` among them,
    /// so N has to exist and be written down before anything reads what the
    /// job needs (D82).
    fn begin_attempt(
        &self,
        id: &JobId,
        runner: &str,
        why: &str,
        parallelism: Option<u32>,
    ) -> Result<u32>;

    /// Record one executor step of an attempt, with its figures (D77).
    ///
    /// Called as each step ends, so that a job stopped at its cap or lost
    /// with its runner still shows the preparation it did — the case the
    /// timestamps exist for, since the first consumer's `up` may pull a whole
    /// warm cache before anything of theirs runs.
    ///
    /// Appends, and **replaces a step of the same name** rather than keeping
    /// two: the complete list arrives again with the verdict, and a record
    /// that held `up` twice would read as a job that prepared twice in one
    /// attempt. A step for an attempt the record does not hold is an error,
    /// not a silent no-op, for the reason every other write here is.
    fn record_step(&self, id: &JobId, attempt: u32, step: StepRun) -> Result<()>;

    /// Record that an attempt ended without the job's verdict, and why.
    ///
    /// Idempotent: ending an attempt that is already ended keeps the first
    /// reason, because the first reason is the true one and a record is not a
    /// journal of the hub's second thoughts.
    fn end_attempt(
        &self,
        id: &JobId,
        attempt: u32,
        reason: &str,
        requeue_code: Option<i32>,
    ) -> Result<()>;

    /// Append one chunk of a job's output in the order the runner produced it,
    /// as the bytes the process wrote (D18): a job's output is not text.
    ///
    /// `seq` is the runner's per-attempt chunk number, and the store uses it
    /// for two things. A number *above* the one expected means the chunks in
    /// between are missing, so the log gets a named gap and the record a
    /// [`JobEventKind::LogGap`]. A number *below* it is a chunk the store has
    /// already written — a runner resending what it was never acknowledged
    /// for — and writing it twice would corrupt the log with duplicated
    /// output, so it is reported as [`LogWrite::AlreadyRecorded`] and
    /// dropped.
    fn append_log(&self, id: &JobId, stream: LogStream, seq: u64, data: &[u8]) -> Result<LogWrite>;

    /// How many chunks of this job's output the store has written for the
    /// attempt under way: the number the next chunk should carry.
    ///
    /// What the hub compares against the runner's own count, so that output
    /// lost with nothing behind it is still noticed (E-40's health report
    /// carries the runner's number).
    fn chunks_recorded(&self, id: &JobId) -> Result<u64>;

    /// Name output that is missing and is not coming back, at the point in
    /// the log where it should have been.
    ///
    /// The hub calls this once it knows the chunks are gone for good: the
    /// runner was asked for them and does not have them, or it is never
    /// coming back. Up to that point a short log is a log waiting for a
    /// resend, not a log with a hole in it, and claiming a gap that then
    /// fills in would be its own kind of lie.
    fn name_missing_log(&self, id: &JobId, first_missing: u64, last_missing: u64) -> Result<()>;

    /// The job's record, if the store has one.
    fn job(&self, id: &JobId) -> Result<Option<JobRecord>>;

    /// Why this store will not act on this job, if it will not (D52).
    ///
    /// `None` for every ordinary job, including one that does not exist.
    /// `Some(why)` for a quarantined record: the job is on the disk, it is
    /// readable, and what it asked for is something today's rules reject.
    ///
    /// Asked *before* acting rather than discovered while acting. Every other
    /// method here answers a quarantined id with an error, which is right —
    /// they are all about to write something — but an error is not an answer
    /// a requester can be given: the hub that treated it as one dropped the
    /// connection without a frame, and `herdctl` told the caller its job was
    /// still running. This is how the hub says "no, and here is why" instead
    /// (D13).
    fn quarantine_reason(&self, id: &JobId) -> Result<Option<String>>;

    /// The job's output so far, both streams interleaved as they arrived, and
    /// byte for byte what the process wrote.
    fn job_log(&self, id: &JobId) -> Result<Option<Vec<u8>>>;

    /// The job's transitions, in the order they were recorded.
    fn job_events(&self, id: &JobId) -> Result<Option<Vec<JobEvent>>>;

    /// Every job that has not reached a terminal state, oldest first. This is
    /// the queue: after a restart it is what the hub has left to do.
    fn unfinished_jobs(&self) -> Result<Vec<JobRecord>>;
}
