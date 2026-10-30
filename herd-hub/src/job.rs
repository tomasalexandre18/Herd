//! What the hub records about a job, and the interface it records it through.
//!
//! These types are the shape of the files in a job's directory (D8), so they
//! are a compatibility surface: the first consumer reads a record while the
//! job runs (E-53) and re-judges it much later (E-54). Every file that carries
//! one of them carries [`RECORD_VERSION`] beside it, and a record written by a
//! newer Herd is refused by name rather than read as if it were this one.

use std::error::Error;
use std::fmt;

use herd_proto::{
    CommitRun, Declaration, JobId, JobOutcome, LabelSet, LabelsFrom, LogStream, Occupant, Priority,
    Requirements, RunnerThatWasFull, StepRun,
};
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
/// line of `events.jsonl`. A version probe sits in front of every record
/// file, and a file whose version is not exactly this one is **not read**: the
/// store returns an error naming the path and both numbers, and because that
/// error comes out of recovery, the hub refuses to start on that state root
/// rather than read it.
///
/// **1 was the first deployed record format** (D73). It reached 6 while
/// nothing was deployed — M6's priority, M7's placement reason, M8's memory
/// and tmpfs, M9's deadline and requeue code, M10's declaration and executor
/// steps — and every one of those breaks was real, on records that only ever
/// existed in a test directory. M16 set the number back to 1, and from M16 on
/// it stopped being a development counter. `v0.4.0` and `v0.4.1` shipped at 1.
///
/// **2 is D199's move, and it is spent on M11's pack half.** `job.json` gains
/// [`JobRecord::packs_in`] and [`JobRecord::packs_out`], and the state root
/// gains the `packs/` directory the hub's registry lives in
/// ([`crate::registry`]) — which is why [`crate::LAYOUT_VERSION`] moves to 2
/// in the same commit, and why that is not optional. The two checks do not
/// behave alike: the layout's is in `JobDirStore::open_with` and refuses the
/// **root**, while this one is in `read_stored` and sets **one record** aside
/// (D218). So a build that moved this number alone would start, bind its port
/// and serve an empty queue off a root where every job was quarantined by
/// name — loud, but not a refusal, and runnable for a while before anybody
/// understood. Moving both together is what puts the refusal at the door
/// instead of inside the house. `herd-hub/tests/version_move_at_the_door.rs`
/// asserts both halves of that, including the one that would have been true
/// of the half-move.
///
/// **What moving it costs.** A record is written once and read much later
/// (E-53, E-54), so the two sides of a bump are one hub at two different
/// times: a hub at version 2 does not read what the version 1 hub wrote, and a
/// rollback to version 1 does not read what version 2 wrote. What stops being
/// readable is the operator's whole state root — every queued job, every
/// verdict the consumer has not collected yet — and the hub says so by not
/// starting, never by guessing. So a bump is a compatibility statement: it
/// needs Alexa's approval, and it ships with either a migration or an explicit
/// instruction for the operator. There is no migration, so it ships with the
/// instruction: `docs/operating.md`, *"The instruction, in order"*, which
/// [`record_version_mismatch_reason`] and the layout's own refusal both point
/// at by name. Only a field that is `#[serde(default)]` *and* means, when
/// absent, exactly what its default says is additive enough to leave the
/// number alone — and the two new fields above are in fact of that kind, so
/// what this move buys is **not** their carriage but the refusal arriving at
/// the door with the layout's.
pub const RECORD_VERSION: u32 = 2;

/// The one sentence a reader gets for a record of another version.
///
/// **One sentence in one place**, because it is said at four sites — reading a
/// `job.json`, reading a line of `events.jsonl`, [`crate::report`] and
/// [`crate::retention`] — and a refusal worded four ways is four refusals an
/// operator has to learn to recognise. It lives here, beside the constant it
/// is about, for the reason `herd_proto::version_mismatch_reason` lives beside
/// `PROTOCOL_VERSION`.
///
/// `found` is an `Option` and that is the substance, not the signature: the
/// version probes read the field with a `serde` default, so a `job.json` with
/// **no** `record_version` key at all used to come back as `0` and be reported
/// as *"it carries record version 0"* — a sentence describing an absent field
/// as a field that says zero, and the sentence an operator pastes into a
/// question. Absent is said to be absent.
///
/// What it has to carry, and why each clause is in it:
///
/// - **both figures**, so a reader knows which way round it is;
/// - **what is unreadable**: this record, and — if the whole root reads this
///   way — every queued job and every uncollected verdict in it;
/// - **that the rollback is refused too**. The check is equality, not a floor,
///   so going back is as refused as coming forward. An operator who reads
///   only "this build is newer" tries the old image next, and that does not
///   work either;
/// - **where the instruction is**, in order, since the words already exist in
///   `docs/operating.md` and nobody inside a container will go looking for
///   them unassisted.
pub fn record_version_mismatch_reason(path: &std::path::Path, found: Option<u32>) -> String {
    let says = match found {
        Some(found) => format!("has record version {found}"),
        None => "carries no record version at all".to_owned(),
    };
    format!(
        "{} {says}, and this hub reads version {RECORD_VERSION}. It is set aside: this hub will \
         not dispatch it, will not rewrite it and will not repair it, so whatever it was for has \
         to be asked for again. There is no migration in either direction — the check is equality \
         and not a floor, so rolling this hub back to the version that wrote it does not read \
         what version {RECORD_VERSION} has written since. If every record in this root reads this \
         way then the root itself is from another release, and every queued job and every \
         uncollected verdict in it is unreadable to this build: do not convert it and do not \
         delete it, take the `herd-hub report --state-dir` reading off it with the old release's \
         binary first, and follow docs/operating.md, \"The instruction, in order\"",
        path.display()
    )
}

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

    /// The day this moment falls on, `YYYY-MM-DD`, for the one sentence a
    /// requester reads about something that is gone (D148, D152).
    ///
    /// A date and not the whole instant: *"removed 2026-10-30 under the
    /// 200 GB output budget"* is a sentence somebody can act on, and the
    /// nanoseconds would be noise in it. The instant is still on the record
    /// in full — this is only how it is said.
    pub fn date(self) -> String {
        let day = self.0.date();
        format!(
            "{:04}-{:02}-{:02}",
            day.year(),
            day.month() as u8,
            day.day()
        )
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

/// Which kind of ending an attempt that produced no verdict had.
///
/// **Three budgets, so three endings.** A job gets two goes at a runner that
/// disappeared (E-21, D39), its own separate budget of goes at a capability
/// that had gone under it (D76), and its own separate budget again at a
/// machine that was already as full as the ceiling it announced (D182).
/// Every one of them is counted by reading the attempts back off the record
/// (D79), so the kind of ending has to be *on* the attempt — and it has to
/// arrive here, at the one function that writes it, rather than be guessed
/// afterwards from the reason, which is prose for a human.
///
/// One parameter rather than a flag beside a code, because the three are
/// mutually exclusive and a pair of orthogonal arguments would let a caller
/// say something that cannot be true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HowItEnded {
    /// About the runner, which is the ordinary case: it stopped answering,
    /// it was asked to stop, its owner withdrew the machine, the hub wrote
    /// the attempt off. Spends one of E-21's goes.
    TheRunnerWentAway,
    /// The job exited with the requeue code it declared, so what failed was
    /// a capability and not the job (D76). Spends that budget and no other.
    TheJobAnsweredItsRequeueCode(i32),
    /// The runner handed the assignment straight back because it was already
    /// holding as many jobs as it told the hub it would hold (D182).
    /// Spends that budget and no other: the machine is here and the job is
    /// untouched.
    TheRunnerWasAtItsCeiling {
        /// What the machine said it was already holding, which is the
        /// ceiling it announced.
        holding: u32,
    },
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
    /// Whether this attempt ended because the runner was **already as full as
    /// the ceiling it announced** (D182, invariant 2).
    ///
    /// The third budget, and the same shape as [`Attempt::requeue_code`] one
    /// field up. E-21's two goes exist for *a runner that disappeared*; D76's
    /// separate budget for *a capability that had gone*; this one for a
    /// machine that is here, healthy, and holding exactly the work it said it
    /// would hold. Nothing is wrong with the job and nothing is wrong with
    /// the machine — the two sides simply disagreed about a count for a
    /// moment — so charging the job one of the goes it keeps for a runner
    /// that vanished loses work that was never in trouble. Before this field
    /// existed, a link outage longer than `lost_after` ended with every job
    /// the runner had been holding recorded `lost` with no verdict at all.
    ///
    /// Counted on its own, by [`JobRecord::goes_a_ceiling_refused`], against
    /// [`crate::CEILING_REFUSALS_ALLOWED`] — because an exemption with no
    /// bound is a job that can be refused for ever and told nobody.
    ///
    /// **The figure is the machine's own count** of what it was already
    /// holding when it refused — which is the ceiling it announced, that
    /// being why it refused. Kept rather than a bare flag because it is what
    /// `JobOutcome::RefusedAsFull` reports to a program: *"the machine
    /// configured for one job at a time refused this four times"* is
    /// something to act on and *"four machines were full"* is not.
    ///
    /// `None` on every other ending, which is what its absence means: the
    /// field is `#[serde(default)]` and is left off the record entirely when
    /// it is absent, so `RECORD_VERSION` stays 1 and a hub without it reads
    /// every record a hub with it wrote (see the rule above this struct's
    /// `RECORD_VERSION`). Never read as zero: a machine holding nothing
    /// would have taken the job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_its_ceiling: Option<u32>,

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
    /// What this attempt said about the revision it really ran, beside the
    /// commit the job is recorded against (D176).
    ///
    /// Written by the hub when the attempt's `fetch` step is recorded, from
    /// the step's own `ran_commit` and the job's own declaration — the only
    /// two places either figure exists. `None` until then, and `None` for
    /// ever on an attempt of a job that is not a declared command: an ad-hoc
    /// job declares no commit, so there is nothing for a revision to agree
    /// or disagree with.
    ///
    /// **This is the oracle, and it only ever records.** A divergence here
    /// changes no exit code and no verdict (D20); what it does is make
    /// "declared X, ran Y" a fact on the record, where before there was
    /// nothing that could contradict a verdict's commit after the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitRun>,
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
    /// The reading that will say what this job needs, when its command
    /// declares one (D74); see [`JobRecord::reading`].
    pub reading: Option<Reading>,
    /// The job this one is the requirement reading **of**, when it is one
    /// (D74, D159); see [`JobRecord::reading_for`].
    pub reading_for: Option<JobId>,
    /// The pack keys this submission asks for, as it asked for them (M11);
    /// see [`JobRecord::needs`].
    pub needs: Vec<String>,
    /// The artefacts this submission says it will leave, key → path under
    /// `{out}` (M11); see [`JobRecord::leaves`].
    pub leaves: std::collections::BTreeMap<String, String>,
    /// Whether the submitter asked for this job's end to be announced
    /// (E-37); see [`JobRecord::announce_end`].
    ///
    /// `false` for every constructor here, which is E-37's *"never per step
    /// job by default"*: the hub's own mints — a reading (D74), a requeue —
    /// ask for nothing, and the one way this is `true` is a submission that
    /// said so on the wire.
    pub announce_end: bool,
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
            reading: None,
            reading_for: None,
            needs: Vec::new(),
            leaves: std::collections::BTreeMap::new(),
            announce_end: false,
        }
    }

    /// The same, with M11's two: the pack keys this submission consumes and
    /// the artefacts it says it will leave (E-12, ORCH-148).
    ///
    /// Both are carried onto the record rather than used at the submission,
    /// because the resolve happens once per **attempt** and against the
    /// registry as it stands then.
    pub fn consuming(
        mut self,
        needs: Vec<String>,
        leaves: std::collections::BTreeMap<String, String>,
    ) -> Self {
        self.needs = needs;
        self.leaves = leaves;
        self
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

    /// The same, with the reading its command declared in place of static
    /// labels (D74).
    ///
    /// The labels it was built with are then the **baseline** the reading
    /// itself is placed against, and this job is not placed at all until the
    /// answer is on its record.
    pub fn reading(mut self, labels_from: Option<herd_proto::LabelsFrom>) -> Self {
        self.reading = labels_from.map(Reading::of);
        self
    }

    /// The same, as the requirement reading **of** that job (D74, D159).
    ///
    /// The one thing on a reading that is not an ordinary job's, and it is
    /// one field rather than a flag because naming the job it was minted for
    /// is what lets every sentence about a machine say *whose* reading is
    /// running there. Only the hub ever calls this: it is the only thing that
    /// mints a reading (D74's third limit, no recursion), and nothing on the
    /// wire can ask for one.
    pub fn reads_for(mut self, job: &JobId) -> Self {
        self.reading_for = Some(job.clone());
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

    /// The same, as a submitter who asked for this job's end to be announced
    /// (E-37).
    ///
    /// A request, carried from the submission onto the record and read back
    /// when the job ends. Nothing else in the hub ever sets it: a job the
    /// hub minted for itself is a job nobody is waiting on an announcement
    /// for.
    pub fn announcing_its_end(mut self, asked: bool) -> Self {
        self.announce_end = asked;
        self
    }

    /// The key E-05 deduplicates on: what makes two submissions *the same
    /// work*.
    ///
    /// **The command, the labels, and the commit a declaration names**, and
    /// nothing else. Not the priority — two agents asking for one build at
    /// two levels are still asking for one build, and the higher level wins
    /// (E-05). Not the time, not the requester.
    ///
    /// The commit is in the key for a **declared** submission and absent for
    /// an **ad-hoc** one, and that is not an inconsistency. An ad-hoc command
    /// line carries whatever revision it names inside itself, so its own text
    /// already tells two commits apart. A declared submission keeps the
    /// commit in [`Declaration::commit`], a field of its own, and its
    /// `command` is the resolved line — which need not mention the commit at
    /// all, and usually does not, because `{commit}` goes into the executor's
    /// `up` and `fetch` steps rather than into the line (D156).
    ///
    /// **The trap, written down so it is not walked into again: a field that
    /// moves out of a string takes its meaning out of every key built from
    /// that string.** The sentence this comment used to carry — "the commit
    /// is part of the command's meaning, and today the command is where it
    /// is" — was true of the ad-hoc submission it was written for, and M10's
    /// declarations made it false the moment the commit got a slot of its
    /// own. Nothing failed to compile and no test went red: the key simply
    /// stopped distinguishing commits, and four requesters asking about four
    /// commits were handed one commit's verdict, silently. Before giving a
    /// submission a new field, ask whether it used to be part of a string a
    /// key already reads.
    ///
    /// **Length-prefixed, never concatenated.** `["a b", "c"]` and
    /// `["a", "b c"]` are different work, and any key built by joining
    /// strings with a separator makes them equal — whatever separator is
    /// chosen, since a command may contain it. Each piece is written as its
    /// length in bytes, a colon, then the bytes, so the encoding can be
    /// parsed back and two different lists can never produce one key. The
    /// labels go through the same encoding and are already sorted and
    /// deduplicated by [`LabelSet`], so naming two labels in the other order
    /// is the same work — and so does the commit, which is one more field
    /// that must not be able to run into its neighbour.
    pub fn dedup_key(&self) -> String {
        dedup_key(&self.command, &self.labels, self.declared_commit())
    }

    /// The commit this submission's declaration names, or `None` for an
    /// ad-hoc submission, which declares nothing (E-01, M10).
    fn declared_commit(&self) -> Option<&str> {
        self.declaration
            .as_ref()
            .map(|declaration| declaration.commit.as_str())
    }
}

/// Where a job's requirements come from, when they are read on a machine
/// rather than stated by a requester (D74).
///
/// One field on the record, carrying the whole of it: what the declaration
/// said, which job is doing the reading, and what came back. A reader of
/// `job.json` alone can then answer "why is this job queued" and "where did
/// these figures come from", which is what E-15 and E-54 ask of a record.
///
/// The job this names in [`Reading::by`] is an **ordinary job**: it went
/// through the one queue, it was confined by the runner's caps, and its own
/// record carries its four executor steps (D74, D77). It carries no reading
/// of its own, and that is how "no recursion" is kept: a command reached
/// through `labels_from` cannot resolve a `labels_from`, because the hub is
/// the only thing that mints such a job and it mints it with none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    /// What the declaration said: the line that prints the requirements, and
    /// the baseline the reading is placed against (D74).
    pub labels_from: LabelsFrom,
    /// The job doing the reading, once one is.
    ///
    /// `None` on a record whose answer came out of the store's cache: nothing
    /// ran for it, which is the whole point of the cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<JobId>,
    /// What the reading answered, once it has answered.
    ///
    /// While this is `None` the job is **not placed**: a job placed on the
    /// baseline would be a job placed on a guess, and D74 forbids exactly
    /// that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Requirements>,
    /// Where the answer came from, in the record's own words (E-15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// How many readings of this job were **lost** to a machine that went
    /// away, rather than answering anything at all (D169).
    ///
    /// A reading that ran and printed something that is not an answer, or
    /// exited non-zero, or was stopped by its cap, has told this hub
    /// something about the commit, and D74's rule applies: the job is refused
    /// by name rather than placed on a guess. A reading that was **lost**
    /// told it nothing. The two were one branch until a nightly pass buried
    /// the one runner it had and 201 of 214 jobs ended 125 with no test run —
    /// every one of them refused because a reading had been lost to a busy
    /// machine, which is not a fact about the commit at all.
    ///
    /// So a lost reading is read again, and this is what bounds that: the
    /// figure is [`crate::Policy::attempts_allowed`], the same number of goes
    /// E-21 gives a job at a runner that disappears, and when it is spent the
    /// job is refused naming both. Counted on the record, like every other
    /// budget in this hub, so there is nothing to leak and nothing to rebuild
    /// after a restart (D79).
    ///
    /// Additive with a `serde` default, so [`crate::RECORD_VERSION`] does not
    /// move (D73): absent on an older record means no reading of that job was
    /// ever lost, which is exactly what an older hub could have written.
    #[serde(default, skip_serializing_if = "none_lost")]
    pub lost: u32,
}

/// Whether a reading has lost none, so the field stays off every record that
/// has nothing to say with it.
fn none_lost(lost: &u32) -> bool {
    *lost == 0
}

impl Reading {
    /// The reading a submission asked for, with nothing read yet.
    pub fn of(labels_from: LabelsFrom) -> Self {
        Self {
            labels_from,
            by: None,
            answer: None,
            from: None,
            lost: 0,
        }
    }

    /// The same, now being read by this job.
    pub fn read_by(mut self, job: &JobId) -> Self {
        self.by = Some(job.clone());
        self
    }

    /// The same, answered, with where the answer came from.
    pub fn answered(mut self, answer: Requirements, from: impl Into<String>) -> Self {
        self.answer = Some(answer);
        self.from = Some(from.into());
        self
    }

    /// The same, after the job that was reading it was **lost**: counted, and
    /// waiting to be read again (D169).
    ///
    /// `by` goes back to `None`, which is the whole mechanism: that is the
    /// state a job starts in, and the pass that writes the loss down mints a
    /// fresh reading for it exactly as it minted the first — through the
    /// cache first, so a reading that was lost after another job's reading
    /// of the same (commit, command, args) had already answered costs
    /// nothing at all.
    pub fn after_a_loss(mut self) -> Self {
        self.lost = self.lost.saturating_add(1);
        self.by = None;
        self
    }

    /// Whether this job may be read again, having lost [`Reading::lost`]
    /// readings already (D169).
    ///
    /// `budget` is [`crate::Policy::attempts_allowed`]: a job gets as many
    /// goes at a reading lost to a machine that went away as it gets at a
    /// machine that went away under it. Bounded because the alternative is a
    /// job that reads for ever on a fleet that is down, which is a job
    /// nobody is told about — the same argument D76 makes for its own
    /// separate budget, and the reason this is a count on the record rather
    /// than an unbounded retry.
    pub fn may_be_read_again(&self, budget: u32) -> bool {
        self.lost < budget
    }
}

/// The key a reading's answer is cached under: the commit, the command and
/// the arguments, and nothing else (D74).
///
/// It is a property of those three alone — the first consumer's `--labels`
/// reads the arguments and the scenario table compiled into that commit, and
/// nothing on the machine — so two submissions of the same three are one
/// reading, and without the cache every submission would pay one.
///
/// Encoded exactly as [`dedup_key`] is, and for the same reason: length
/// before bytes, so that no two different triples can ever produce one key.
/// This is M6's key family, which is what D74 asks for.
pub fn reading_key(commit: &str, command: &str, args: &[String]) -> String {
    let mut key = String::new();
    key.push_str("commit ");
    push_counted(&mut key, commit);
    key.push_str(" command ");
    push_counted(&mut key, command);
    key.push_str(" args ");
    for arg in args {
        push_counted(&mut key, arg);
    }
    key
}

/// The key E-05 deduplicates on, for a command, a label set and a declared
/// commit that are already somewhere — on a record, say. See
/// [`NewJob::dedup_key`], which is where the whole of it is written down.
///
/// `commit` is `None` for an ad-hoc submission. The field is then written
/// nowhere in the key rather than as an empty piece, so an ad-hoc key still
/// means exactly what it has always meant; and a declared key is that same
/// key with one more counted field after it, which no ad-hoc key can equal.
pub fn dedup_key(command: &[String], labels: &LabelSet, commit: Option<&str>) -> String {
    let mut key = String::new();
    key.push_str("command ");
    for piece in command {
        push_counted(&mut key, piece);
    }
    key.push_str(" labels ");
    for label in labels.iter() {
        push_counted(&mut key, label);
    }
    if let Some(commit) = commit {
        key.push_str(" commit ");
        push_counted(&mut key, commit);
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
    /// Where this job's requirements come from, when they are read on a
    /// machine rather than stated by its requester (D74).
    ///
    /// The one field a reader looks at to understand a job that is queued
    /// behind a reading: it carries the line, the job doing the reading and
    /// the answer. While the answer is absent the job is not placed, and
    /// [`JobRecord::labels_needed`], [`JobRecord::memory_need_mib`] and
    /// [`JobRecord::tmpfs_need_mib`] all read the answer in preference to
    /// what the submission carried — which is the whole of D74's consequence
    /// for M7 and M8: they consume the requirements **on the record** and
    /// stay where they are.
    ///
    /// `None` for every job whose requirements its requester stated, which is
    /// every job before D74. `#[serde(default)]`, because a record written
    /// before this field existed is a record of such a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<Reading>,
    /// The job this one is the requirement reading **of**, when it is one
    /// (D74, D159).
    ///
    /// The mirror of [`JobRecord::reading`], and the two can never both be
    /// set: a command reached through `labels_from` cannot resolve one of its
    /// own, because the hub is the only thing that mints such a job and mints
    /// it with none (D74's third limit).
    ///
    /// It exists because the claim step has to be able to tell a reading from
    /// a build, and nothing else on the record says so: a reading is an
    /// ordinary job in every other respect, which is D74's whole design. A
    /// reading **holds no capability** — no counted slot, no host-wide
    /// machine, no exclusion — and is still admitted against the room it runs
    /// in (D119) and still bounded by the hub's reading cap and the runner's
    /// own (D108). That is D159, and this field is how the arithmetic knows.
    ///
    /// Written at the mint and never changed, so a hub that restarts rebuilds
    /// the right holdings from the records: a reading read back as an
    /// ordinary job would be handed a slot nobody gave it.
    ///
    /// `#[serde(default)]`, because a record written before D159 is a record
    /// of a job that is not a reading — or, for the one in flight across the
    /// upgrade, of a reading that holds its slot until it ends, which is five
    /// minutes at most ([`crate::READING_CAP`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_for: Option<JobId>,
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
    /// `#[serde(default)]`, so the record of a job not yet attempted reads as
    /// a job with no attempt rather than as a read error. Purely additive in
    /// that sense, which is why M3 added it without moving
    /// [`RECORD_VERSION`] — the one kind of change still allowed to leave the
    /// frozen number alone (D73).
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    /// What the runner measured while the job ran (E-15). Reserved in the
    /// record's shape by M2 and filled by M4: every figure is unset until the
    /// runner reports it, which it does once, with the outcome.
    pub metrics: JobMetrics,
    /// What the job left under its `{out}` directory, as the record
    /// summarises it: the paths and sizes, never the bytes. The bytes are the
    /// files in `files/` beside this record, which is the directory D8
    /// reserved for exactly them (E-15's "kept files").
    ///
    /// `#[serde(default)]`, and that is why [`RECORD_VERSION`] does not move
    /// for it: a record written before this field reads as a job that kept
    /// nothing, which is precisely what such a job did. The live state root
    /// on the deployed hub is readable unchanged, and `files/` has existed and
    /// been empty in every job directory since M2 — so a job directory does
    /// not even gain a *name*, only contents it was already shaped for.
    #[serde(default, skip_serializing_if = "OutputSummary::is_silent")]
    pub outputs: OutputSummary,
    /// The pack keys this job asks for, as its requester asked for them
    /// (M11, E-12).
    ///
    /// **The question, where [`JobRecord::packs_in`] is the answer.** It is
    /// on the record for the reason [`JobRecord::declaration`] is: an attempt
    /// is resolved against the registry as it stands *when that attempt goes
    /// out*, and a requeue onto another machine (E-21) resolves again — so
    /// the keys have to outlive the submission connection that named them.
    /// Until this field existed, [`herd_proto::Asked::needs`] arrived on the
    /// wire and was dropped at the submission, which is why
    /// `FromHub::AssignJob`'s `packs_in` could only ever be sent empty
    /// (ORCH-234).
    ///
    /// `#[serde(default)]`, for the reason [`JobRecord::packs_in`] gives:
    /// absent means **this job asks for no pack**, which is every job on the
    /// deployed root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    /// What this job says it will leave, key → path under `{out}`
    /// (M11, E-12).
    ///
    /// The question [`JobRecord::packs_out`] answers, and on the record for
    /// the same reason `needs` is: `FromHub::AssignJob`'s `packs_out` is
    /// handed back on **every** attempt, so a requeue leaves the same
    /// artefacts under the same keys, and the record is the only thing that
    /// knows them both.
    ///
    /// `#[serde(default)]`, as above.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub leaves: std::collections::BTreeMap<String, String>,
    /// The packs this job's `needs` resolved to, by the key that names them
    /// under `{in}` (E-12).
    ///
    /// To be written when the job is assigned, because that is when the hub
    /// resolves a `(commit, key)` to an id, and never changed afterwards:
    /// what a job consumed is part of what it was, and a re-run on another
    /// commit is another job. A requeue onto another machine (E-21) resolves
    /// again and rewrites this, exactly as it re-derives the `-j`.
    ///
    /// **Written by nothing on this build, and that is a measured gap and
    /// not the design.** The resolve itself now happens where the assignment
    /// is built and travels on `FromHub::AssignJob`'s `packs_in`
    /// (ORCH-234's frame half), so a runner is told which packs its job
    /// consumes; what is missing is the *recording*. Putting it here means a
    /// write at the moment `begin_attempt` numbers the attempt — the one
    /// write that moment is allowed — which is a `JobStore` signature and
    /// five implementors, and was deliberately left out of the branch that
    /// filled the frame. So a reader of `job.json` cannot yet tell which
    /// pack an attempt was handed, and `herd-hub report` cannot either.
    ///
    /// The **id** and not the manifest: the manifest's bytes live once, in
    /// `packs/manifests/<id>/manifest.tsv`, and the id is the sha256 of
    /// exactly those bytes. Putting a manifest here would put a second copy of
    /// a pack's identity in a file that is rewritten on every transition.
    ///
    /// `#[serde(default)]`, and absent means exactly what the default says:
    /// **this job consumed no pack.** At record version 2 that is
    /// unambiguous — the version in front of the file already says the hub
    /// that wrote it knew about packs — which is the condition this constant's
    /// own doc sets for a field to be additive. The field is therefore not
    /// what [`RECORD_VERSION`] moved for; see that doc for what was.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub packs_in: std::collections::BTreeMap<String, herd_proto::pack::PackId>,
    /// The packs this job produced, by its declaration's `leaves` key
    /// (E-12, E-17).
    ///
    /// Written when a runner's post-fsync registration frame is accepted, so a
    /// key appears here only once the pack exists somewhere whole. A job that
    /// declared `leaves` and whose key is absent here is a job whose pack was
    /// refused or never registered, which is a visible state and not a
    /// silence — and the reason is in `events.jsonl`.
    ///
    /// **This is where a pack's provenance lives**, together with
    /// `herd-hub report`: no pack field goes on the verdict document, because
    /// `VERDICT_FORMAT_VERSION` and `OUTPUT_FORMAT_VERSION` are frozen as whole
    /// literal text in `herdctl/tests/frozen_verdict.rs` and
    /// `herd-proto/tests/frozen_formats.rs`, and a field there would move a
    /// fourth public version D199 never authorised.
    ///
    /// `#[serde(default)]`, for the reason [`JobRecord::packs_in`] is.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub packs_out: std::collections::BTreeMap<String, herd_proto::pack::PackId>,
    /// Whether this job's **submitter asked for its end to be announced**
    /// (E-37).
    ///
    /// **A request and never a result.** It says what was asked for at
    /// submission time; it says nothing at all about whether an announcement
    /// was made, could be made, or reached anybody. A hub with no
    /// `--notify-sink` configured writes this field exactly as a hub with one
    /// does and announces nothing, because the sink is the operator's setting
    /// and this is the requester's question. Whether a particular event left
    /// the machine is the hub's own stderr's business, not the record's: an
    /// event is news, sent once and never retried
    /// ([`crate::notify`]), so a record claiming delivery would be a record
    /// claiming something nobody checked.
    ///
    /// Written at birth and never changed, like [`JobRecord::labels`]: what a
    /// submitter asked for is part of what the job was. A requeue (D76) and a
    /// second attempt (E-21) re-run the same record and so carry the same
    /// asking; a second identical submission that *joins* this job (E-05) does
    /// not change it, which is the honest answer — one execution has one end,
    /// and E-05's joiner is told how it went on its own connection.
    ///
    /// **It is read exactly once per job**, at the job's terminal transition,
    /// and only by a hub that has a sink: see `say_how_it_ended` in
    /// `crate::server`. So the acceptance criterion holds by construction —
    /// a day of step jobs nobody asked about is a day of no reads and no
    /// events.
    ///
    /// `#[serde(default)]`, and absent means exactly what the default says:
    /// **nobody asked**. That is true of every record written before this
    /// field existed — there was no way to ask — which is the condition
    /// [`RECORD_VERSION`]'s own doc sets for a field to be additive, so this
    /// field does not move that number. `herd-hub/src/store.rs`'s unit tests
    /// assert both halves: a record carrying the asking reads back as asking,
    /// and a `job.json` with no such key reads back as not asking rather than
    /// failing to parse.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub announce_end: bool,
}

/// What a job left behind, as `job.json` says it.
///
/// The summary and not the files, so a reader of the record alone sees what
/// is there and how big it is without reading any of it (E-53), and so that a
/// record stays the kilobytes D8 promised whatever a job wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSummary {
    /// Every file kept, by its path relative to `files/`, with its size.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<KeptFile>,
    /// Why nothing was kept although the job left something: the runner's
    /// output cap, with the figures (D131). Never set beside a non-empty
    /// `files` — over the cap is all or nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// One sentence per thing under `{out}` that was deliberately not kept: a
    /// symlink, a fifo, a name that is not UTF-8, a path this hub would not
    /// join onto its own state root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// Whether the bytes came after the verdict rather than on it (D148).
    ///
    /// On the record so an operator can tell which carriage a job used, and
    /// so a hub that restarts knows a collection it is still owed bytes for
    /// is one it should ask about. **Not for a requester**: what a requester
    /// gets is the same collection either way.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streamed: bool,
    /// When a requester first fetched these bytes, if one ever did (D152).
    ///
    /// **The first fetch and not the last**, because the only question the
    /// sweeper asks of it is whether anybody has ever looked — and a figure
    /// written once per collection costs one record write for ever, where a
    /// last-read stamp would cost one per fetch on a path that already
    /// polls. A collection nobody has opened is evidence nobody has seen,
    /// and that is why it is kept longer than one that has been read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_fetched_at: Option<Timestamp>,
    /// When the hub's sweeper removed these bytes, and under which figures
    /// (D152).
    ///
    /// **Set on the record that keeps the manifest**, which is what gives
    /// "the manifest outlives the bytes" an answer rather than a shrug:
    /// [`OutputSummary::files`] still names every path, length and digest,
    /// and this says why none of them is on the disk. A requester asking for
    /// an evicted collection is handed this sentence, with its date and its
    /// budget in it, instead of "not on this hub's disk".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<OutputsRemoved>,
}

/// Why a job's output bytes are gone, kept on its record for ever (D152).
///
/// Kilobytes beside the manifest it explains, so there is never a reason to
/// delete it: a record that had files says so, and says what became of them,
/// for as long as the record exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputsRemoved {
    pub at: Timestamp,
    /// One sentence naming the figures: the date, the budget, and the floor
    /// when the floor had to be broken. A reason with no figures in it is
    /// the one report nobody can act on (E-04's style).
    pub why: String,
    /// How many bytes went. The manifest's own lengths add up to this, and
    /// it is written down anyway: an operator reading one record should not
    /// have to sum twelve numbers to learn what the sweep freed.
    pub bytes: u64,
}

impl OutputSummary {
    /// Whether there is nothing to write down, which is every job that leaves
    /// no output: the field is then absent from `job.json` entirely, so the
    /// record of such a job is byte-identical to the one it was before this
    /// existed.
    pub fn is_silent(&self) -> bool {
        self.files.is_empty() && self.refused.is_none() && self.skipped.is_empty()
    }
}

/// How much of the hub's volume a job's output files may hold, and for how
/// long they are safe from the sweeper (D148, D152).
///
/// **Two figures and no third.** Invariant 1 says a retention policy is not
/// a storage product: there is no tier, no class, no per-job override and no
/// expression — an operator says how much room there is and how long a fresh
/// collection is promised, and the order in which things go when those two
/// disagree is Herd's and not configurable. A knob that let one job's
/// evidence outrank another's would be the first line of a policy language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Bytes of **whole, verified** collections the hub may hold across
    /// every job's `files/` directory.
    ///
    /// Not the state root: a record is kilobytes and a log has a cap of its
    /// own (D137), and counting them here would make one number answer for
    /// three different things that fail in three different ways.
    ///
    /// Part-written bytes are deliberately outside it. They belong to a
    /// transfer in flight, they cannot be evicted without destroying the
    /// thing that is arriving, and they are bounded by the per-job output
    /// cap times the number of runners — so the disk may sit that much above
    /// the budget while collections cross, and nothing pretends otherwise.
    pub bytes: u64,
    /// How long a collection is promised before the sweeper may take it.
    ///
    /// Seven days (D148). **It is a floor and not a lifetime**: nothing is
    /// removed merely for being older, and a collection outlives the floor
    /// by as long as the budget allows. Zero turns the promise off, which is
    /// what a test uses to make an ordinary eviction happen in a second.
    pub floor: std::time::Duration,
}

/// The hub's output budget: 200 GB (D148, chosen by Alexa on 2026-10-16).
///
/// **A chosen figure, never a derived one.** No measurement produced it, and
/// the arithmetic that would — a few hundred megabytes times the number of
/// jobs a day that really leave that much — needs a distribution only the
/// first consumer has. Every other byte figure in Herd is computed from what
/// a mechanism can carry; this one is a decision, and D148 says at length
/// why it must not be read as anything else.
pub const OUTPUT_BUDGET: u64 = 200_000_000_000;

/// How long a collection is promised before the sweeper may take it: seven
/// days (D148).
pub const OUTPUT_FLOOR: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: OUTPUT_BUDGET,
            floor: OUTPUT_FLOOR,
        }
    }
}

/// A span as a sentence about retention: days, or hours under a day.
///
/// The floor is spoken of in days, so both it and an age compared against
/// it are said in days. `{:?}` on a `Duration` of seven days prints
/// `604800s`, which is the one figure in the sentence a reader cannot check
/// against the promise they were given — every other setting the hub prints
/// is seconds or minutes, where the debug form reads fine.
pub fn in_days(how_long: std::time::Duration) -> String {
    let hours = how_long.as_secs() / 3600;
    if hours >= 24 {
        return format!("{} day(s)", hours / 24);
    }
    format!("{hours} hour(s)")
}

/// What one run of the sweeper did (D152).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sweep {
    /// The jobs whose bytes were removed, in the order they went.
    pub removed: Vec<JobId>,
    /// How many bytes that freed.
    pub freed: u64,
    /// What the hub holds after it, as the sweep's own walk measured.
    pub held: u64,
    /// How many of the removed collections were still inside the floor.
    ///
    /// Non-zero is the case D148 insists is never silent: the fleet produced
    /// more than the budget in less than the floor, and the operator is told
    /// on the day, in a sentence naming both figures.
    pub inside_the_floor: usize,
    /// Bytes the sweeper could not free although it is still over budget:
    /// collections still arriving, which it must never take.
    pub could_not_free: u64,
}

impl Sweep {
    /// Whether this sweep did nothing at all, which is every sweep on a hub
    /// that is inside its budget.
    pub fn is_quiet(&self) -> bool {
        self.removed.is_empty()
    }
}

/// One collection a runner still owes this hub, as the records describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionOwed {
    pub job_id: JobId,
    /// Which attempt produced it: a second attempt's files are not the
    /// first's, which is why every file frame carries this.
    pub attempt: u32,
    /// Which runner ran that attempt, so one runner is never told about
    /// another's collection.
    pub runner: String,
    /// What this hub holds of it, ready to go on
    /// [`herd_proto::FromHub::JobFilesHeld`].
    pub held: Vec<herd_proto::FileHeld>,
}

/// What one streamed chunk completed, as the store saw it.
///
/// Three outcomes rather than a `bool`, because the hub does three different
/// things with them and a caller that could not tell them apart would have to
/// guess: say nothing, acknowledge the collection, or name a file that will
/// not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkLanded {
    /// Written, and this file is not finished yet.
    More,
    /// This file is complete, verified and in `files/`, and so is every other
    /// file the manifest named: the collection is done and the runner may
    /// drop its spool.
    Complete,
    /// This chunk is not what the manifest promised and was not written.
    ///
    /// An offset that does not continue what the hub holds, a path no verdict
    /// announced, or a digest that did not match when the last byte landed.
    /// Each is named rather than retried silently: a transfer that cannot
    /// make progress has to be visible, because the alternative is a
    /// collection that never completes and a job whose record nobody can
    /// explain.
    Refused(String),
}

/// One file of a job's record, as the record names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptFile {
    /// Relative to the job's `files/` directory, directories preserved:
    /// `files/results.json` under `{out}` is `files/results.json` here.
    pub path: String,
    pub bytes: u64,
    /// The file's sha256, lowercase hex, as the runner announced it.
    ///
    /// **This is what makes the manifest outlive the bytes** (D148). The
    /// record keeps a path, a size and a digest for every file a job
    /// produced, and those three are kilobytes however many hundreds of
    /// megabytes they describe — so there is never a reason to delete them,
    /// and only the bytes are ever removed. A requester asking for something
    /// already gone is then told which files, how big, and why, instead of
    /// being handed a short directory it would read as a whole one; and a
    /// judge re-reading an old result knows what it was looking for (E-54).
    ///
    /// Empty on a record written before this existed, which is the honest
    /// answer for one: nobody computed a digest for those bytes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
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
    ///
    /// **Derived is also why D156 needed no migration.** Putting the commit
    /// in the key changed what is computed from a record, not what is
    /// written in one: every record already carries its whole
    /// [`JobRecord::declaration`], so an existing `job.json` keys correctly
    /// under the new rule with no field added, no default, and no bump of
    /// [`RECORD_VERSION`].
    pub fn dedup_key(&self) -> String {
        dedup_key(
            &self.command,
            &self.labels,
            self.declaration
                .as_ref()
                .map(|declaration| declaration.commit.as_str()),
        )
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
        // What a reading answered wins over what the submission carried, and
        // over the per-thread derivation with it: the reading is the thing
        // that knows, and it computed its figure for the `-j` the job was
        // given (D74, D82).
        if let Some(read) = self.requirements() {
            return Ok(Some(read.memory_mib));
        }
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

    /// The tmpfs this job reserves, in MiB (E-31). Absent is no reservation,
    /// and a reading's answer takes the place of the declaration's figure
    /// (D74).
    pub fn tmpfs_need_mib(&self) -> u64 {
        match self.requirements() {
            Some(read) => read.tmpfs_mib,
            None => self.tmpfs_mib.unwrap_or(0),
        }
    }

    /// What a reading answered about this job, once it has (D74).
    ///
    /// `None` for every job whose requirements its requester stated, **and**
    /// for one whose reading has not answered yet. The two are told apart by
    /// [`JobRecord::awaits_a_reading`], and a caller that confused them would
    /// place a job on its baseline.
    pub fn requirements(&self) -> Option<&Requirements> {
        self.reading.as_ref()?.answer.as_ref()
    }

    /// What this job **is**, for the claim step's arithmetic (D159).
    ///
    /// [`herd_proto::Occupant::Reading`] for a `labels_from` requirement
    /// reading the hub minted, and [`herd_proto::Occupant::Job`] for
    /// everything else. The one place the distinction is read off a record,
    /// so that a second reading of it cannot drift from this one.
    pub fn occupant(&self) -> Occupant {
        match self.reading_for {
            Some(_) => Occupant::Reading,
            None => Occupant::Job,
        }
    }

    /// Whether this job is the requirement reading of another (D74, D159).
    pub fn is_a_reading(&self) -> bool {
        self.reading_for.is_some()
    }

    /// Whether this job is waiting for a reading to say what it needs (D74).
    ///
    /// A job that is is never placed: not by the claim step, not by a dry
    /// run, not by anything. There is nothing to place it against.
    pub fn awaits_a_reading(&self) -> bool {
        self.reading.is_some() && self.requirements().is_none()
    }

    /// The capabilities this job needs: what a reading answered, or what its
    /// submission carried (E-25, D74).
    ///
    /// **The one thing M7's matching and M8's admission read**, which is how
    /// both of them consume a reading's answer without moving in the plan:
    /// [`JobRecord::labels`] stays what the submission said for ever — it is
    /// what the job was, and M6's deduplication key is computed over it — and
    /// this is what the job *needs*.
    pub fn labels_needed(&self) -> &LabelSet {
        match self.requirements() {
            Some(read) => &read.labels,
            None => &self.labels,
        }
    }

    /// The words the requester typed, as they were quoted into this job's
    /// command line before it was minted (D107, D139).
    ///
    /// What the hub's own `{j}` pass has to hold out of itself: the arguments
    /// are already *in* [`JobRecord::command`], and a pass over the whole of
    /// it would turn an argument that says `{j}` into a thread count. Empty
    /// for an ad-hoc command line, which no requester's arguments were spliced
    /// into — the whole of it is then the text its submitter wrote, and the
    /// hub fills `{j}` across all of it as it always has.
    pub fn requester_args(&self) -> &[String] {
        match &self.declaration {
            Some(declaration) => &declaration.args,
            None => &[],
        }
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
            reading: new.reading,
            reading_for: new.reading_for,
            needs: new.needs,
            leaves: new.leaves,
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at: now,
            updated_at: now,
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
            outputs: OutputSummary::default(),
            packs_in: Default::default(),
            packs_out: Default::default(),
            announce_end: new.announce_end,
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
    ///
    /// Nor is an attempt a runner handed back **because it was already as
    /// full as the ceiling it announced** (D182). That machine did not
    /// disappear: it is on the line, healthy, running exactly what it said it
    /// would run, and the job it declined is untouched. Charging it a go cost
    /// whole queues their verdicts on an ordinary link blip — the runner's
    /// jobs go on running across a dropped link, the hub's watchdog requeues
    /// them, and the moment the runner redials the hub hands the same jobs
    /// straight back to the machine that is still holding them. That refusal
    /// was the job's last go. It has a bound of its own,
    /// [`JobRecord::goes_a_ceiling_refused`].
    pub fn goes_a_runner_took(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| {
                attempt.ended_at.is_some()
                    && attempt.requeue_code.is_none()
                    && attempt.at_its_ceiling.is_none()
            })
            .count() as u32
    }

    /// How many times this job has been handed back by a runner that was
    /// already at the ceiling it announced (D182).
    ///
    /// The bound on the exemption above, and derived from the attempts for
    /// the reason every other budget here is: a counter would be a second
    /// answer to a question the record already answers, and one more thing
    /// to leak across a restart (D79).
    pub fn goes_a_ceiling_refused(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| attempt.at_its_ceiling.is_some())
            .count() as u32
    }

    /// The runners that refused this job for a ceiling of their own, in
    /// order, each named once.
    ///
    /// For the one sentence an operator reads about a job that ran out of
    /// this budget: *which* machines kept saying they were full is the whole
    /// of what there is to act on.
    pub fn runners_that_were_full(&self) -> Vec<RunnerThatWasFull> {
        let mut named: Vec<RunnerThatWasFull> = Vec::new();
        for attempt in &self.attempts {
            let Some(holding) = attempt.at_its_ceiling else {
                continue;
            };
            if named.iter().any(|full| full.name == attempt.runner) {
                continue;
            }
            named.push(RunnerThatWasFull {
                name: attempt.runner.clone(),
                holding: Some(holding),
            });
        }
        named
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

    /// The sweeper took this job's output bytes, and why (D152).
    pub fn outputs_removed(at: Timestamp, files: usize, bytes: u64, why: &str) -> Self {
        Self {
            at,
            what: JobEventKind::OutputsRemoved {
                files: files as u64,
                bytes,
                why: why.to_owned(),
            },
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
            | JobEventKind::Waiting { .. }
            | JobEventKind::OutputsRemoved { .. } => None,
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
    /// The hub's sweeper removed this job's output bytes to stay inside the
    /// output budget (D152).
    ///
    /// **On the job's own history, which is where an irreversible deletion
    /// belongs.** The hub's journal gets one line per *sweep* and not one
    /// per collection — D151's lesson about a path reached in bulk — so the
    /// only place that answers "what happened to this job's screenshots" a
    /// week later is here, beside the manifest that still names them.
    OutputsRemoved {
        /// How many files the manifest named. They are all still on the
        /// record; none of them is on the disk.
        files: u64,
        bytes: u64,
        /// The same sentence [`OutputsRemoved`](crate::job::OutputsRemoved)
        /// carries, with the date and the budget in it.
        why: String,
    },
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

    /// Approve one runner by name and mint its token (E-48, Q8).
    ///
    /// On this trait rather than beside it because the approvals are part of
    /// the state this store keeps, written through the one writer the records
    /// go through (D8): a hub with a second way to persist would be a hub
    /// with a second way to lose things. Answers with the token, which is the
    /// only time it is readable — what is kept is a hash.
    fn approve_runner(&self, name: &str) -> Result<String>;

    /// Remove one runner, revoking its token. `false` if it was not approved.
    fn remove_runner(&self, name: &str) -> Result<bool>;

    /// Who is approved here, by name, for `herdctl runner list`.
    fn approved_runners(&self) -> Result<Vec<crate::roster::Approval>>;

    /// Whether a runner announcing this name and this token may be served,
    /// and which way it may not be (E-48).
    fn admit_runner(&self, name: &str, token: Option<&str>) -> crate::roster::Admission;

    /// How many runners are approved, for the line the hub prints at startup.
    fn approved_runner_count(&self) -> usize;

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

    /// Record where this job's requirements come from, and what they are once
    /// they are known (D74).
    ///
    /// The one write that moves a job from "waiting for its reading" to
    /// placeable, so it is the one write that must be on the disk before the
    /// job is handed anywhere: a hub that placed the job and then failed to
    /// write the answer would have a running job whose record cannot say what
    /// it was admitted against. Refused on a job that is over, like every
    /// other write here.
    fn record_reading(&self, id: &JobId, reading: Reading) -> Result<()>;

    /// What a reading of this (commit, command, args) answered, if this store
    /// has it (D74).
    ///
    /// `None` is "pay for a reading", never "needs nothing": a store that
    /// cannot read its own cache answers `None` and the reading runs again,
    /// which costs a job and never a wrong figure.
    fn reading_answer(&self, key: &str) -> Result<Option<Requirements>>;

    /// Keep a reading's answer under its (commit, command, args) key, so that
    /// the next submission of those three pays no reading (D74).
    fn remember_reading(&self, key: &str, answer: &Requirements) -> Result<()>;

    /// Record what the runner measured (E-15). Separate from the transition
    /// because a runner reports figures the hub never computes, and because a
    /// store that keeps no measurements is still a correct store.
    fn record_metrics(&self, id: &JobId, metrics: JobMetrics) -> Result<()>;

    /// Keep the files a job left under `{out}`, and say so on its record
    /// (D131).
    ///
    /// Written **before** the transition to `finished`, inside the same try,
    /// for the reason the measurements are: a reader who sees `finished` is
    /// entitled to find the record's files beside it (E-15), and a hub that
    /// could not write them must not acknowledge the verdict either — the
    /// runner keeps holding both until it can (D19, E-44).
    ///
    /// A path this store will not join onto its own directory is **dropped
    /// and named on the record**, not an error: a verdict must never be lost
    /// over a malformed path, and a path that cannot be written down is a
    /// fact about one file rather than about the job.
    ///
    /// **It is dropped from the manifest too, and that is the one case that
    /// is** (D175). The manifest is the list of paths this store joins onto
    /// its own directory — `write_output_chunk` resolves a chunk against it,
    /// `job_outputs` reads a file back by it, the sweeper measures by it — so
    /// a path that must never be joined must never be on it. The `skipped`
    /// sentence is then the only place that file exists on the record, which
    /// is the price of not writing an unjoinable path down. **The exit code a
    /// requester reads for this case was never decided**: it is 0 today
    /// because the manifest has nothing to count short, and whether a path
    /// the hub refuses should also cost a requester its zero is open.
    ///
    /// **Bytes that contradict their own manifest entry are a different
    /// case, and the entry survives them** (D175). The entry is written
    /// down before the bytes are judged, so a refused frame leaves a file
    /// that is announced and not held — the same shape as a streamed file
    /// still crossing — and a requester counting its promises against what it
    /// holds finds it short. This is the rule for every other absence here;
    /// the digest case was the one exception and it produced a false success.
    fn record_outputs(&self, id: &JobId, outputs: &herd_proto::JobOutputs) -> Result<()>;
    /// The files this job's record holds, read back off the disk.
    ///
    /// What a `herdctl wait` that attached long after the job ran is given,
    /// which is why it comes off the disk and not out of memory: the hub may
    /// have restarted twice since, and the record is the only thing that
    /// still knows. `Default` for a job that kept nothing, and for a job this
    /// store has never heard of — the caller has already dealt with that case
    /// through [`JobStore::job`].
    ///
    /// `with_bytes` decides whether the data comes with the manifest.
    /// **`false` is not a cheaper version of `true`**: it is the answer to a
    /// different question. A verdict frame carries the manifest of a streamed
    /// collection and must not carry hundreds of megabytes; a requester that
    /// then asks for the files gets them a chunk at a time. Reading the bytes
    /// to throw them away would be the one place this design reintroduced the
    /// cost it exists to remove.
    fn job_outputs(&self, id: &JobId, with_bytes: bool) -> Result<herd_proto::JobOutputs>;

    /// Write one chunk of a streamed output file, and say what that completed.
    ///
    /// The hub's half of [`herd_proto::ToHub::JobFileChunk`]. Appends to a
    /// part-file beside the job's `files/` directory and, when the manifest's
    /// length is reached, verifies the digest the verdict announced before
    /// renaming the file into place under D8's discipline.
    ///
    /// **`files/` therefore only ever holds whole, verified files**, which is
    /// what keeps E-53's promise intact while a collection is still arriving:
    /// a consumer reading the directory sees finished files or nothing, never
    /// a half-written one it could parse.
    fn write_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<ChunkLanded>;

    /// How much of each file of a job's manifest this hub holds.
    ///
    /// What [`herd_proto::FromHub::JobFilesHeld`] carries, and the whole of
    /// the resumption design on the runner's side: the hub is the side that
    /// knows what it has, so a runner redialling with 300 MiB spooled is told
    /// where to pick up rather than having to ask, and a part-written file
    /// resumes at the hub's own length instead of the runner's idea of how
    /// far it had got — which is exactly the figure a dropped link
    /// invalidated.
    ///
    /// Derived from the disk every time, never stored: a count kept beside
    /// the files could disagree with them, and the disagreement would be a
    /// collection that never completes (D79's reasoning, applied to bytes).
    fn output_holdings(&self, id: &JobId) -> Result<Vec<herd_proto::FileHeld>>;

    /// Every collection this hub was promised and has not finished
    /// receiving.
    ///
    /// **Derived from the records, never kept in memory** (D79's rule,
    /// applied to bytes): a list of outstanding transfers held beside the
    /// files could disagree with them, and the disagreement would be either a
    /// runner holding a spool nobody ever asks for again or a hub waiting for
    /// bytes that are already on its disk. Rebuilt by looking, so a hub that
    /// restarted twice mid-transfer picks up exactly where its disk says it
    /// is.
    ///
    /// A **finished** job, note: the verdict goes first and the files follow
    /// (D148), so these are never in [`JobStore::unfinished_jobs`]. That is
    /// the point — a job is readable and complete about what it produced long
    /// before its evidence has all crossed.
    fn collections_owed(&self) -> Result<Vec<CollectionOwed>>;

    /// The same question about **one** job: is this hub still owed any of
    /// this collection?
    ///
    /// `None` means there is nothing left to wait for, and it says so for
    /// every reason there is — the job left no files, they rode the verdict,
    /// they are all here at their declared length, or the sweeper took them.
    /// `Some` means bytes are genuinely outstanding, and names the runner
    /// that owes them.
    ///
    /// **One definition, asked two ways** (D79). [`JobStore::collections_owed`]
    /// is this answer over every record, which is what a runner's redial
    /// needs; this is what a *verdict* needs, because a verdict is about one
    /// job and walking the whole store to answer for one of them is the work
    /// D152 refused to do once per verdict. Both read the same completeness
    /// test off the same disk, so the two cannot disagree (D167).
    fn collection_owed(&self, id: &JobId) -> Result<Option<CollectionOwed>>;

    /// What this store was told its output budget is, for the line the hub
    /// prints at startup and for a test that wants to read it back.
    fn output_budget(&self) -> Budget;

    /// Write down that a requester has fetched this job's collection, if
    /// none ever had (D152).
    ///
    /// **A no-op after the first time**, which is what keeps it off the hot
    /// path: a `--outputs-to` polls while a collection crosses, and a
    /// last-read stamp would be a record write per poll. What the sweeper
    /// needs is the boolean, and the boolean only ever changes once.
    fn note_outputs_fetched(&self, id: &JobId) -> Result<()>;

    /// Bring the hub inside its output budget, and say what that cost
    /// (D152).
    ///
    /// Run where bytes land and once at startup, never on a timer: the only
    /// thing that can put a hub over its budget is a collection arriving, so
    /// a clock would be a second writer waking up to find nothing to do. It
    /// walks `jobs/*/files` when it runs and believes the walk, so the
    /// running total it is triggered by can drift without ever producing a
    /// wrong eviction — see [`Budget`] and the order in D152.
    ///
    /// A sweep that finds another one already walking answers with
    /// [`Sweep::default`] and does nothing, so a caller reading `held` off
    /// it is reading "no figure" and not "nothing held". Nothing but a
    /// test ever reads it.
    fn sweep_outputs(&self) -> Result<Sweep>;

    /// Read `take` bytes of one of a job's kept files, starting at `offset`.
    ///
    /// `None` when the record names the file and the disk does not have it,
    /// which is a fact a requester is **told** rather than one the hub
    /// smooths over: the manifest outlives the bytes, so a path with nothing
    /// behind it is an answer and not a gap (D148).
    ///
    /// A chunk at a time, and through the store rather than off a path the
    /// server built itself: where a job's files live is the store's to know,
    /// and a server that joined its own paths onto the state root would be a
    /// second opinion about the layout (D8).
    fn read_output_chunk(
        &self,
        id: &JobId,
        path: &str,
        offset: u64,
        take: usize,
    ) -> Result<Option<Vec<u8>>>;

    /// Forget whatever is left of a part-written collection.
    ///
    /// Called when every file is in place, so the part directory does not
    /// outlive the transfer it belonged to.
    fn clear_output_parts(&self, id: &JobId) -> Result<()>;

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
    fn end_attempt(&self, id: &JobId, attempt: u32, reason: &str, how: HowItEnded) -> Result<()>;

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

    /// This store's pack registry, when it keeps one (E-12, E-17).
    ///
    /// **`None` means "this store keeps no pack registry", and never "the
    /// registry it keeps is empty".** The distinction is the whole reason the
    /// method answers an `Option` rather than a `&PackRegistry`. A hub whose
    /// store answers `None` cannot register a pack, cannot resolve one and
    /// cannot record a holding, so every pack frame it is sent is refused
    /// **by name** saying exactly that. A hub whose store answers `Some` with
    /// an empty registry is a working pack authority that has been told about
    /// no pack yet, and the frames it is sent are answered on their merits.
    /// Reading the first as the second is how a runner comes to be told
    /// "nothing is registered for that key" by a hub that could never have
    /// registered anything — the shape of D150's silence, in a second place.
    ///
    /// Every implementor but [`crate::store::JobDirStore`] answers `None`,
    /// which is why the default body is here rather than on each of them: the
    /// registry lives on a state root's disk, and a store with no root — every
    /// test double in `server.rs` and in `herd-hub/tests/` — has nowhere to
    /// keep one. The default is what let the seam be added without moving a
    /// single implementor, which is why it was chosen over a field on the
    /// hub's `State`, where every construction site would have grown an
    /// argument (ORCH-230).
    fn packs(&self) -> Option<&crate::registry::PackRegistry> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";

    fn labels(names: &[&str]) -> LabelSet {
        LabelSet::new(names.iter().map(|name| (*name).to_owned())).expect("valid labels")
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// An answer, as a reading would have printed it.
    fn answer() -> Requirements {
        Requirements {
            labels: labels(&["kvm", "metal"]),
            memory_mib: 4096,
            tmpfs_mib: 8192,
            scenarios: 12,
        }
    }

    /// A job whose command declares a reading, as a submission makes one: its
    /// labels are the baseline, and nothing is read yet.
    fn waiting_for_a_reading() -> JobRecord {
        let new = NewJob::at(
            vec!["their-tool test".to_owned()],
            labels(&["build"]),
            Priority::Step,
        )
        .with_admission(Some(512), Some(16))
        .reading(Some(LabelsFrom {
            line: "their-tool test --labels".to_owned(),
            baseline: labels(&["build"]),
        }));
        JobRecord::hypothetical(new, Timestamp::now())
    }

    /// The cache key is the three things D74 names and nothing else, and no
    /// two different triples can produce one key.
    #[test]
    fn a_reading_is_keyed_on_the_commit_the_command_and_the_arguments() {
        let key = reading_key(A_COMMIT, "test", &args(&["--only", "posix"]));
        assert_eq!(
            key,
            format!("commit 40:{A_COMMIT} command 4:test args 6:--only5:posix")
        );
        // The same three, again: a second submission of one triple finds the
        // first one's answer.
        assert_eq!(
            key,
            reading_key(A_COMMIT, "test", &args(&["--only", "posix"]))
        );
        // And each of the three really keys it.
        let another_commit = A_COMMIT.replace("cb5a", "0000");
        assert_ne!(
            key,
            reading_key(&another_commit, "test", &args(&["--only", "posix"]))
        );
        assert_ne!(
            key,
            reading_key(A_COMMIT, "test-pack", &args(&["--only", "posix"]))
        );
        assert_ne!(key, reading_key(A_COMMIT, "test", &args(&["--only"])));
        // Length before bytes, like M6's key: two argument lists that would
        // be one string joined on a separator are two keys (D74 asks for the
        // same key family, and this is what makes it one).
        assert_ne!(
            reading_key(A_COMMIT, "test", &args(&["a b", "c"])),
            reading_key(A_COMMIT, "test", &args(&["a", "b c"]))
        );
    }

    /// Before the answer: the job needs the requirements nobody has read yet,
    /// so it is not placeable, and what it holds is still what it was
    /// submitted with.
    #[test]
    fn a_job_waiting_for_a_reading_is_not_placed_on_its_baseline() {
        let record = waiting_for_a_reading();
        assert!(record.awaits_a_reading());
        assert_eq!(record.requirements(), None);
        assert_eq!(
            record.labels_needed(),
            &labels(&["build"]),
            "the baseline is what it carries, and awaits_a_reading is what stops it being \
             placed on it"
        );
    }

    /// After the answer: M7's matching and M8's admission read the
    /// requirements **off the record**, so the answer takes the place of what
    /// the submission carried — labels, memory and tmpfs alike (D74).
    #[test]
    fn an_answer_takes_the_place_of_what_the_submission_carried() {
        let mut record = waiting_for_a_reading();
        record.reading = record
            .reading
            .map(|reading| reading.answered(answer(), "read on a runner by j2"));
        assert!(!record.awaits_a_reading());
        assert_eq!(record.labels_needed(), &labels(&["kvm", "metal"]));
        assert_eq!(record.memory_need_mib(None), Ok(Some(4096)));
        assert_eq!(record.tmpfs_need_mib(), 8192);
        // And the labels it was submitted with are still on the record: that
        // is what the job *was*, and M6's deduplication key is over it.
        assert_eq!(record.labels, labels(&["build"]));
        assert_eq!(
            record.dedup_key(),
            dedup_key(
                &record.command,
                &record.labels,
                record
                    .declaration
                    .as_ref()
                    .map(|declaration| declaration.commit.as_str())
            )
        );
    }

    /// A job with no reading is every job before D74, and nothing about it
    /// changes.
    #[test]
    fn a_job_whose_requester_stated_its_requirements_is_untouched() {
        let record = JobRecord::hypothetical(
            NewJob::at(vec!["true".to_owned()], labels(&["build"]), Priority::Step)
                .with_admission(Some(512), Some(16)),
            Timestamp::now(),
        );
        assert!(!record.awaits_a_reading());
        assert_eq!(record.requirements(), None);
        assert_eq!(record.labels_needed(), &labels(&["build"]));
        assert_eq!(record.memory_need_mib(None), Ok(Some(512)));
        assert_eq!(record.tmpfs_need_mib(), 16);
    }

    /// A reading that was **lost** answered nothing, so the job is read
    /// again — and the going back to `by: None` is the whole mechanism: that
    /// is the state a job starts in, and the next pass mints a fresh reading
    /// exactly as it minted the first (D169).
    #[test]
    fn a_reading_lost_before_it_answered_is_read_again_and_counted() {
        let reading = Reading::of(herd_proto::LabelsFrom {
            line: "sh tools/labels.sh".to_owned(),
            baseline: labels(&["build"]),
        })
        .read_by(&JobId::new("j0000000002".to_owned()));
        assert_eq!(reading.lost, 0);

        let again = reading.after_a_loss();
        assert_eq!(again.lost, 1);
        assert_eq!(again.by, None, "a lost reading leaves nothing to wait for");
        assert!(again.answer.is_none());
    }

    /// And it is bounded, because a job that reads for ever on a fleet that
    /// is down is a job nobody is told about. The figure is
    /// [`crate::Policy::attempts_allowed`]: as many goes at a reading lost to
    /// a machine that went away as E-21 gives the job at the machine itself.
    #[test]
    fn a_job_whose_readings_keep_being_lost_runs_out_of_goes() {
        let budget = crate::ATTEMPTS_ALLOWED;
        let mut reading = Reading::of(herd_proto::LabelsFrom {
            line: "sh tools/labels.sh".to_owned(),
            baseline: labels(&["build"]),
        });
        // One reading is running; every turn of this loop loses it, exactly
        // as `settle_readings` does: count the loss, then ask whether there
        // is a go left for another.
        let mut read = 1;
        loop {
            reading = reading.after_a_loss();
            if !reading.may_be_read_again(budget) {
                break;
            }
            read += 1;
            assert!(read <= budget, "the budget must bound this loop");
        }
        assert_eq!(read, budget, "a job gets {budget} readings, and no more");
        assert_eq!(reading.lost, budget);
    }

    /// The count is additive: a record written by a hub that never heard of
    /// it reads as a job none of whose readings was lost, which is exactly
    /// what that hub could have written — so [`RECORD_VERSION`] does not move
    /// (D73).
    #[test]
    fn a_record_with_no_lost_count_reads_as_a_job_that_lost_none() {
        let older = r#"{
            "labels_from": {"line": "sh tools/labels.sh", "baseline": ["build"]}
        }"#;
        let reading: Reading = serde_json::from_str(older).expect("an older reading reads");
        assert_eq!(reading.lost, 0);
        assert!(reading.may_be_read_again(crate::ATTEMPTS_ALLOWED));
        // And a reading that has lost none writes nothing about it, so a
        // record this hub writes is one an older hub can still read.
        let written = serde_json::to_string(&reading).expect("a reading writes");
        assert!(!written.contains("lost"), "{written}");
        assert!(serde_json::to_string(&reading.after_a_loss())
            .expect("a reading writes")
            .contains("\"lost\":1"));
    }
}

#[cfg(test)]
mod a_ceiling_refusal_on_the_record {
    use super::*;

    /// An attempt as a hub **without** this field writes and reads one: the
    /// same struct minus `at_its_ceiling`, which is what a rollback is.
    #[derive(Debug, Serialize, Deserialize)]
    struct AttemptAsAnOlderHubReadsIt {
        number: u32,
        runner: String,
        started_at: Timestamp,
        ended_at: Option<Timestamp>,
        requeue_code: Option<i32>,
        ended: Option<String>,
        why: String,
        #[serde(default)]
        steps: Vec<StepRun>,
        #[serde(default)]
        commit: Option<CommitRun>,
    }

    fn ended(number: u32, runner: &str, how: Option<&str>) -> Attempt {
        let at = Timestamp::now();
        Attempt {
            number,
            runner: runner.to_owned(),
            started_at: at,
            ended_at: Some(at),
            requeue_code: None,
            at_its_ceiling: None,
            ended: how.map(str::to_owned),
            why: "claimed by somebody".to_owned(),
            steps: Vec::new(),
            commit: None,
        }
    }

    /// **Neither version number moves for the field.** Asserted here as well
    /// as in `herd-hub/tests/announced_job_ceiling.rs`, because this is the
    /// file whose rule the new field has to satisfy: `#[serde(default)]`,
    /// absent meaning exactly what the default says, and therefore additive.
    ///
    /// **Two assertions, and the second is about the protocol and not about
    /// the record.** Both numbers have since moved at D199, for M11's pack
    /// half and not for this field: `PROTOCOL_VERSION` 2 → 3 with the pack
    /// frames, `RECORD_VERSION` 1 → 2 with the hub's pack registry and
    /// `LAYOUT_VERSION`. The claim this test makes is the one it always made —
    /// *nothing about an announced job ceiling moved either number* — and the
    /// literals are what makes it a claim. Both stay **literals**: an
    /// assertion written against the constant it is pinning catches nothing.
    #[test]
    fn the_record_version_does_not_move() {
        assert_eq!(
            RECORD_VERSION, 2,
            "2 is D199's move for the hub's pack registry and the `packs/` directory beside it; \
             this field is additive and moved nothing"
        );
        assert_eq!(
            herd_proto::PROTOCOL_VERSION,
            3,
            "3 is D199's bump for M11's pack half, and not for this field"
        );
    }

    /// A record written by a hub **with** the field is read by one without
    /// it, and the other way round. Both directions, because a rollback is
    /// one of them and an upgrade is the other, and a state root that only
    /// survives one of those is a state root an operator loses.
    #[test]
    fn a_record_crosses_the_field_in_both_directions() {
        let mut full = ended(1, "capped-runner", Some("the runner was full"));
        full.at_its_ceiling = Some(2);
        let written = serde_json::to_string(&full).expect("an attempt writes");
        assert!(
            written.contains("\"at_its_ceiling\":2"),
            "the machine's own count is on the record when it refused: {written}"
        );

        // Forwards: a hub without the field reads what a hub with it wrote,
        // and everything it does know is unchanged.
        let older: AttemptAsAnOlderHubReadsIt =
            serde_json::from_str(&written).expect("an older hub reads it");
        assert_eq!(older.number, 1);
        assert_eq!(older.runner, "capped-runner");
        assert_eq!(older.ended.as_deref(), Some("the runner was full"));
        assert_eq!(older.requeue_code, None);

        // Backwards: a hub with the field reads what a hub without it wrote,
        // and reads the absence as `false` — which is what every attempt
        // ever written before this field meant.
        let written_before = serde_json::to_string(&older).expect("an older attempt writes");
        assert!(
            !written_before.contains("at_its_ceiling"),
            "the older shape carries no such key: {written_before}"
        );
        let read_back: Attempt =
            serde_json::from_str(&written_before).expect("this hub reads an older attempt");
        assert_eq!(
            read_back.at_its_ceiling, None,
            "an attempt written before this field existed is not a ceiling refusal"
        );

        // And the key is off the wire entirely when the flag is not set, so
        // no existing record grows a byte.
        let plain = serde_json::to_string(&ended(1, "wsl-pc", Some("lost"))).expect("writes");
        assert!(
            !plain.contains("at_its_ceiling"),
            "an ordinary attempt is written exactly as it was: {plain}"
        );
    }

    /// **The three budgets are counted apart**, which is the whole of the
    /// change: a refusal at a machine's own ceiling spends none of E-21's
    /// goes, none of D76's, and one of its own.
    #[test]
    fn a_ceiling_refusal_spends_its_own_budget_and_no_other() {
        let mut record =
            JobRecord::hypothetical(NewJob::new(vec!["true".to_owned()]), Timestamp::now());
        let mut refused = ended(1, "capped-runner", Some("the runner was full"));
        refused.at_its_ceiling = Some(2);
        let mut again = ended(2, "capped-runner", Some("the runner was full"));
        again.at_its_ceiling = Some(2);
        let mut requeued = ended(3, "metal-pc", Some("it exited 75"));
        requeued.requeue_code = Some(75);
        let went_away = ended(4, "wsl-pc", Some("the runner stopped answering"));
        record.attempts = vec![refused, again, requeued, went_away];

        assert_eq!(
            record.goes_a_runner_took(),
            1,
            "only the attempt that ended at a runner that went away"
        );
        assert_eq!(record.requeues_spent(), 1, "and only the 75 (D76)");
        assert_eq!(
            record.goes_a_ceiling_refused(),
            2,
            "and both refusals at a machine's own ceiling, on their own budget"
        );
        assert_eq!(
            record.runners_that_were_full(),
            vec![RunnerThatWasFull {
                name: "capped-runner".to_owned(),
                holding: Some(2),
            }],
            "named once, with the figure the machine itself gave"
        );
    }
}
