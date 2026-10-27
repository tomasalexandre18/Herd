//! Where one job goes, or why it goes nowhere (E-19, E-22, E-25, E-36).
//!
//! The claim step. Everything the hub decides about a job between "it is at
//! the front of the queue" and "it is on a machine" is decided here, in one
//! pure function of the queue's record and what the connected runners are
//! holding, so that a test can put a fleet in any state at all without a
//! socket, and so that the sentence an operator reads is built in one place.
//!
//! Four musts meet here, and it is worth saying which part is which.
//!
//! * **E-25, kinds under contention.** Not implemented here: the algebra is
//!   [`herd_proto::Holdings`] (M5) and this module consults it. What is here
//!   is where the holdings come from — see below — and the reading of a
//!   [`herd_proto::Contention`] as "wait" or "never".
//! * **E-22, slots and a derived `-j`.** A counted capability is slots, and
//!   the slot divisor is what makes the derived `-j` honest: a host must
//!   survive every one of its slots being full, so the memory share is
//!   divided by them ([`herd_proto::HostSize::parallelism`]).
//! * **E-19, no fallback while an eligible runner is available.** A job goes
//!   to the most *preferred* runner that can take it, and when the most
//!   preferred runner that is **available** cannot take it right now, the job
//!   **waits for that one** instead of falling back to a lesser machine that
//!   is free. Under a visible limit: [`Stickiness`].
//! * **E-36, one line per decision.** [`Placement::Claim::why`] is that line,
//!   and the only thing that writes the word `claimed`.
//!
//! ## Where the slot counts come from
//!
//! **From the records, every pass, and from nothing else.** A hub that kept a
//! counter of busy slots would have one job: to never, ever leak one. Every
//! way a job can end — exited, signalled, stopped by a cap, cancelled, lost,
//! superseded, its runner killed mid-job — would have to decrement it, and
//! the one path that forgot would halve a machine until the hub restarted,
//! silently. A hub restart would have to rebuild it, too, from exactly the
//! records this reads.
//!
//! So there is no counter. A job holds its labels on a runner for exactly as
//! long as its *record* says an attempt of it is open on that runner
//! ([`holdings`]), which is the same fact the watchdog, the requeue and the
//! restart all already work from. A leaked slot is not a bug that is hard to
//! make here; it is a state this hub cannot represent.
//!
//! ## Where a job's requirements come from
//!
//! **Off the record, through [`JobRecord::labels_needed`] and the two figures
//! beside it, and this module never asks where they came from (D74).** A
//! requester may have stated them, or a reading may have run on a machine and
//! answered them; either way they are on the record before anything here is
//! called, and a job whose reading has not answered is not offered to this
//! module at all. That is what let D74 land without moving M7 or M8: the
//! algebra, the slots and the admission are what they were.

use std::collections::BTreeMap;
use std::time::Duration;

use herd_proto::{of_which, Holdings, HostSize, JobId, LabelSet, Occupant, Parallelism};

use crate::capability::RunnerCapacity;
use crate::job::{JobRecord, JobState, Timestamp};
use crate::queue::roughly;

/// How long a job may wait for the runner it is sticking to before the hub
/// places it elsewhere (E-19).
///
/// Three hours, which is the figure the first consumer names for themselves.
/// E-19 asks for the limit to be **visible**, so it is a setting with a
/// default rather than a constant: `herd-hub --stickiness-limit`, printed at
/// startup, named in the sentence of every job that is waiting on it, and
/// named again in the line that explains the placement it eventually allows.
pub const STICKINESS_LIMIT: Duration = Duration::from_secs(3 * 60 * 60);

/// The waiting rule, as a setting (E-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stickiness {
    pub limit: Duration,
}

impl Default for Stickiness {
    fn default() -> Self {
        Self {
            limit: STICKINESS_LIMIT,
        }
    }
}

/// One connected runner, as the claim step sees it.
///
/// Built per dispatch pass. `holdings` is this runner's share of
/// [`holdings`], and it is taken by value because the pass itself admits into
/// it: two jobs offered in one pass must contend with each other, not only
/// with what was already running when the pass began.
#[derive(Debug)]
pub struct Seat {
    pub name: String,
    /// Higher is preferred (E-19). Declared by the machine, never by a
    /// requester (invariant 6).
    pub preference: i32,
    pub host: HostSize,
    /// The most jobs this runner said it holds at once, in its handshake
    /// (D181, invariant 2). `None` from a runner that did not say.
    ///
    /// **A bound beside the memory admission, never instead of it.** The two
    /// answer different questions and neither subsumes the other: admission
    /// weighs what a job *declared* it would take, so a queue of jobs that
    /// declare nothing costs a machine nothing and this is the only figure
    /// that stops them; and this one knows nothing about size, so a single
    /// job larger than the machine is still admission's to refuse. Whichever
    /// bites first is the one that bites.
    ///
    /// Never derived here. The runner's arithmetic uses the cap it puts on
    /// every job, which no frame carries, so the hub either has the runner's
    /// own figure or has none — and having none means placing exactly as it
    /// placed before this field existed.
    pub jobs_at_once: Option<u32>,
    /// What this machine last said it was **already holding**, when it
    /// handed an assignment back because it was at the ceiling above
    /// (D182). `None` is "nothing to correct", and is the ordinary state.
    ///
    /// [`Seat::reserved`]`.jobs` is the hub's count, read off the records,
    /// and there is one state in which the records are wrong by a lot: a
    /// runner whose link dropped goes on running its jobs, the watchdog ends
    /// those attempts and requeues them, and when the machine redials the
    /// hub sees a seat holding nothing. Offering it back the jobs it is
    /// still running is what this figure prevents — the two counts are
    /// compared and the larger one is the one the ceiling is held against,
    /// because the machine is the authority on what the machine is running.
    ///
    /// Only ever a **floor** under the hub's own count, never a replacement
    /// for it: within one pass `claim` goes on taking `reserved.jobs` up as
    /// jobs are offered, so two jobs offered together still contend for the
    /// last place whatever this says.
    pub it_said_it_is_holding: Option<u32>,
    /// On its way out (invariant 2). It takes no new job, and — the decision
    /// D80 records — it does not count as *available* either, so a job does
    /// not stick to a machine that is leaving.
    pub draining: bool,
    /// Where its owner's switch stands (E-33).
    ///
    /// Treated exactly as a drain is, and for D80's reason: a machine its
    /// owner has withdrawn is not *available*, so nothing waits for it and
    /// everything queued for it is claimable elsewhere in the same pass. The
    /// seat is still here rather than left out of the fleet, because a job
    /// that has nowhere to go has to be able to say that the machine it would
    /// have gone to is switched off, and why.
    pub switch: herd_proto::Switch,
    /// The job this host is being held empty for, with the capability that
    /// asked for it (E-27).
    ///
    /// Set by [`reservations`] before a pass places anything, from the records
    /// alone — never accumulated, never released (D79). While it is set, this
    /// machine claims nothing else, whatever the queue's order says: a drain
    /// that only won when the metal job happened to be at the front of the
    /// queue would lose to every build submitted behind it, which is the
    /// starvation E-27's "drains the build slot first" exists to answer.
    pub reserved_for: Option<(JobId, String)>,
    pub capacity: RunnerCapacity,
    pub holdings: Holdings,
    /// What the jobs already out here declared they would take (E-29, E-31).
    /// By value for the reason `holdings` is: two jobs offered in one pass
    /// must be weighed against each other, not only against what was already
    /// running when the pass began.
    pub reserved: Reserved,
    /// The host's one-minute load average, times 1000, as this machine last
    /// reported it (E-32).
    ///
    /// **Recorded, never a gate.** It is on the seat so that the line
    /// explaining a placement can quote it, and it is compared against nothing
    /// in this module — there is no `if load` anywhere, by design and by test.
    /// Absent when the machine could not read it, never zero.
    pub load_milli: Option<u64>,
}

/// What became of one job in one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Hand it to this runner. `why` is E-36's line; `command` is the job's
    /// command with its derived `-j` in it.
    Claim {
        runner: String,
        why: String,
        command: Vec<String>,
        /// What was derived, when anything was. On the claim so that the
        /// figure recorded and the figure passed are one value and cannot
        /// drift.
        parallelism: Option<Parallelism>,
    },
    /// It stays queued. `why` is the clause the queue's sentence carries.
    Wait { why: String },
    /// Nothing anywhere can ever run it, so it ends here (E-36). `why` names
    /// the labels or the figure that made it impossible.
    Impossible { why: String },
    /// It is over before it began, and **cancelled** is what that is (E-09,
    /// E-10, D40).
    ///
    /// Two causes, and neither is a fact about any machine: the job's deadline
    /// has passed while it was still queued, or the one job it named as its
    /// parent did not succeed. A separate variant from [`Placement::Impossible`]
    /// because the outcomes differ and must: `impossible` says nobody could
    /// ever run this, `cancelled` says nobody should now — and `herdctl` says
    /// them in different words even though D40 gives both exit 125.
    Cancel { why: String },
}

/// Where the one job this job named as its parent stands (E-10).
///
/// Resolved outside the claim step, by whoever has the store, and handed in:
/// the claim step is a pure function of a record and a fleet, and a parent
/// that had to be looked up inside it would make it a function of a disk.
/// What the *reading* of a parent's end means is [`Parent::judge`], which is
/// pure and is the one place that decides what "success" is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Parent {
    /// This job named none.
    #[default]
    None,
    /// It named one, and that job is still going: queued, assigned, running,
    /// or interrupted by a hub that died.
    NotYet { id: JobId, state: String },
    /// It named one, and that job exited 0.
    Succeeded { id: JobId },
    /// It named one, and that job ended some other way. `how` is the parent's
    /// own ending, in the words its own requester would read.
    DidNot { id: JobId, how: String },
    /// It named one this hub has no record of.
    ///
    /// Unreachable through a submission, which is refused when the parent does
    /// not exist — but a state root can lose a directory, and the one thing
    /// this must not do is read as a parent that succeeded.
    Gone { id: JobId },
}

impl Parent {
    /// What this parent says about its child: nothing, wait, or end.
    ///
    /// The whole of E-10's edge, in one function and in one place. **Success
    /// is exit 0 and nothing else.** A parent that failed, was signalled,
    /// could not start, was stopped by a cap, was cancelled, was lost or was
    /// judged impossible did not produce what its child was waiting for, and a
    /// child that ran anyway would be a test run against a build that is not
    /// there (E-12's flow).
    fn judge(&self) -> Option<Placement> {
        match self {
            Parent::None | Parent::Succeeded { .. } => None,
            Parent::NotYet { id, state } => Some(Placement::Wait {
                why: format!(
                    "it starts after {id}, which is {state}; it waits for that job to succeed \
                     and is cancelled if it does not (E-10)"
                ),
            }),
            Parent::DidNot { id, how } => Some(Placement::Cancel {
                why: format!(
                    "cancelled (parent): it starts only after {id} succeeds, and {id} {how}. \
                     One job depends on one other job's success, with no conditions and no \
                     alternative branch (E-10)"
                ),
            }),
            Parent::Gone { id } => Some(Placement::Cancel {
                why: format!(
                    "cancelled (parent): it starts only after {id} succeeds, and this hub has \
                     no record of {id} at all, so whether it succeeded cannot be known. A \
                     parent that cannot be read is never read as one that succeeded"
                ),
            }),
        }
    }
}

/// How long a job may hold a host empty waiting for it to go quiet (E-27).
///
/// Fifteen minutes, which is the figure E-27 names. Measured from the job's
/// **submission**, because that is a figure on the record and therefore
/// survives a restart and cannot leak (D79): a hub that remembered when each
/// drain began would have one more thing to rebuild, and the one it forgot
/// would hold a machine empty for ever.
pub const DRAIN_FIRST_LIMIT: Duration = Duration::from_secs(15 * 60);

/// How many requirement readings one runner may be running at once (D169).
///
/// **Two.** A reading holds no capability (D159), so until this figure
/// existed nothing at all bounded how many of them one machine could be given
/// — and the first consumer's nightly pass submitted 214 jobs in one second,
/// every one of them needing a reading first. The matcher put some two
/// hundred of them on the one runner of a one-runner fleet, on a 20 GiB,
/// eight-thread lend. The host saturated, the runner's own kvm probe timed
/// out in a loop for forty minutes, and 201 of the 214 jobs ended 125 with no
/// test run.
///
/// Two rather than one because a reading is cheap and the point is a bound,
/// not a queue of one: one reading in flight while the next is being handed
/// out keeps a fleet of readings moving at the speed of the machine. Two
/// rather than eight because the figure that matters is the one a machine
/// survives *on top of* the real work it is already running, and a reading is
/// admitted against the room but takes no slot — so nothing else is counting
/// it.
///
/// It is the hub's figure and not the runner's, like [`DRAIN_FIRST_LIMIT`]
/// and the reading cap it sits beside: a reading is the hub's own job —
/// nobody declared it, no capability file mentions it — and a figure a runner
/// announced would be a figure the hub would have to defend against a runner
/// that announced two hundred.
pub const READINGS_AT_ONCE: u32 = 2;

/// The clause that marks a sentence as **the hub holding a job back for a
/// ceiling a runner announced** — the hub's words, about the runner, and not
/// the runner's own.
///
/// It exists for the same reason `herd_runner::A_RUNNERS_OWN_CEILING` does,
/// and against it. Both sentences are about the same ceiling and both reach
/// an operator's record: this one on a `waiting` line while the job is held,
/// that one inside an `attempt_ended` line when a runner really did refuse.
/// They used to share the words *"holds at most"*, so a reader — or a test —
/// matching on that substring could not tell a job that waited (which costs
/// nothing) from a job whose go was spent (which can cost the job). **Neither
/// clause is a substring of the other, and neither sentence contains the
/// other's clause.** `herd-hub/tests/announced_job_ceiling.rs` asserts that
/// against both sentences as they are really built, so the distinction
/// cannot be lost by an edit to either wording.
pub const A_CEILING_THE_RUNNER_ANNOUNCED: &str = "it said it holds at most";

/// What the claim step needs beyond the record and the fleet.
///
/// A struct rather than three more arguments, and with a [`Default`], so that
/// the hundred-odd unit tests of the arithmetic say what they are about and
/// nothing else. Every field here is a fact somebody else looked up.
#[derive(Debug, Clone, Default)]
pub struct Placing {
    /// Where the one job this job named stands (E-10).
    pub parent: Parent,
    /// How long a drain-first job may hold a host empty (E-27).
    pub drain_first_limit: Option<Duration>,
    /// How many requirement readings one runner may run at once (D169).
    pub readings_at_once: Option<u32>,
}

impl Placing {
    fn drain_first_limit(&self) -> Duration {
        self.drain_first_limit.unwrap_or(DRAIN_FIRST_LIMIT)
    }

    /// Never zero: a cap of zero is a reading that is never placed, and every
    /// job waiting on one would wait for ever. The hub refuses the setting at
    /// startup (`Policy::refusal`); this is the second half of the same rule,
    /// for the callers that build a [`Placing`] by hand.
    fn readings_at_once(&self) -> u32 {
        self.readings_at_once.unwrap_or(READINGS_AT_ONCE).max(1)
    }
}

/// A job that fits a machine now: what was derived for it, and what it takes
/// of that machine's two admission figures (E-29, E-31).
///
/// The figures travel with the verdict rather than being recomputed at the
/// claim, so that the room the pass judged and the room the pass takes are
/// one value and cannot drift — the same reason the derived `-j` rides on the
/// claim (D82).
#[derive(Debug, Clone, Copy)]
struct Fitted {
    derived: Option<Parallelism>,
    memory_mib: u64,
    tmpfs_mib: u64,
}

/// What one runner can do with one job right now.
#[derive(Debug, Clone)]
enum Verdict {
    /// Now, with whatever was derived for it and what it takes.
    Now(Fitted),
    /// Not now, but this runner is the kind of machine that runs this job:
    /// its slot is taken, something it excludes is running, a probe outside
    /// Herd is holding a capability. A job sticks to a runner like this
    /// (E-19).
    Later(String),
    /// Not here, whatever happens here: it does not offer the label, its
    /// owner denied it, it does not say how big it is, it is draining. The
    /// job does not stick to it and does not fail for it — another machine
    /// may offer what it needs.
    NotHere(String),
    /// Not anywhere, ever (E-36).
    Impossible(String),
}

/// What every runner is holding, read from the job records alone.
///
/// A job holds its labels on the runner of its open attempt while its record
/// is in one of the three states that mean "an attempt of this is out there":
/// `assigned` (handed over, not yet started), `running`, and `interrupted` (a
/// hub died mid-job and the runner may still be running it). It stops holding
/// them the moment the record leaves those states, which is the same moment
/// every end of a job already writes — so there is no release path to forget.
///
/// **A job with no labels at all still holds a place.** It is running on that
/// machine, and `exclusive` is host-wide: it empties the host of *everything*,
/// including work that asked for nothing (D46). [`Holdings`] counts jobs for
/// that reason, so an empty label set is still an entry.
///
/// Each entry also carries **what** the job is, read off its own record
/// ([`JobRecord::occupant`]): a requirement reading is running on the machine
/// and holds none of its capabilities (D159), and no label set can say that.
pub fn holdings(records: &[JobRecord]) -> BTreeMap<String, Vec<Holding>> {
    let mut held: BTreeMap<String, Vec<Holding>> = BTreeMap::new();
    for (runner, record) in out_there(records) {
        held.entry(runner.to_owned()).or_default().push(Holding {
            job: record.id.clone(),
            labels: record.labels_needed().clone(),
            occupant: record.occupant(),
        });
    }
    held
}

/// One open attempt on one runner, as a dispatch pass reads it off the
/// records: which job, what it was placed against, and what kind of work it
/// is (D159).
#[derive(Debug, Clone)]
pub struct Holding {
    pub job: JobId,
    pub labels: LabelSet,
    pub occupant: Occupant,
}

/// Every job the records say has an attempt out on a runner, with that
/// runner's name.
///
/// One filter, used by everything that has to know what a machine is already
/// carrying: the labels ([`holdings`]) and the memory and tmpfs
/// ([`reserved`]). Two copies of this test would be two answers to one
/// question, and the first job to be counted by one and not the other would
/// be a machine that reads idle while work runs on it.
fn out_there(records: &[JobRecord]) -> impl Iterator<Item = (&str, &JobRecord)> {
    records.iter().filter_map(|record| {
        if !matches!(
            record.state,
            JobState::Assigned | JobState::Running | JobState::Interrupted { .. }
        ) {
            return None;
        }
        let attempt = record.attempt()?;
        if attempt.ended_at.is_some() {
            return None;
        }
        Some((attempt.runner.as_str(), record))
    })
}

/// What the jobs already out on one runner **declared** they would take
/// (E-29, E-31).
///
/// Declared, never measured, and that distinction is the whole of it: a job
/// admitted a second ago has allocated nothing yet, so a hub that costed real
/// usage would let a second job in beside it and watch both grow into the
/// host's swap. A job that exceeds its declaration is already stopped by M4's
/// memory cap (exit 124), which is what makes the declaration the honest unit
/// to add up.
///
/// Derived from the records at the start of every pass, exactly as the slot
/// counts are (D79). There is no reservation table, so there is nothing to
/// release when a job exits, is signalled, is stopped by a cap, is cancelled,
/// is lost, is superseded, has its runner killed with `kill -9`, or has its
/// hub killed and restarted — a reservation is a field on a record whose
/// attempt is open, and every one of those endings is already the write that
/// closes it.
#[derive(Debug, Clone, Default)]
pub struct Reserved {
    /// The sum of the declared memory needs, in MiB.
    pub memory_mib: u64,
    /// The sum of the declared tmpfs reservations, in MiB.
    pub tmpfs_mib: u64,
    /// How many jobs those two sums are over.
    pub jobs: u32,
    /// How many of those jobs are requirement readings (D74, D159).
    ///
    /// A reading declares no memory and no tmpfs — it builds and boots
    /// nothing, so `reading_job` gives it no figure to declare — and it is
    /// still weighed by admission like any job, which is what D119 asks and
    /// what D159 deliberately leaves alone: a reading is slot-free, never
    /// admission-free. It is counted here so that the sentence a short job
    /// reads can break the count down: "2 jobs running there hold 4 GiB" says
    /// nothing about which of the two is a `--labels` line.
    pub readings: u32,
    /// Jobs out there whose declared memory **cannot be costed** from their
    /// record, with the reason, by job.
    ///
    /// A record in this state is a bug somewhere else — a build handed out
    /// with no `-j` written on it — and the one thing it must not do is read
    /// as a job holding nothing. While one is here, nothing more that costs
    /// memory is admitted on that machine, and the sentence says which job and
    /// why.
    pub uncostable: Vec<(JobId, String)>,
}

impl Reserved {
    /// Take this job's declared need, as the claim does.
    fn take(&mut self, memory_mib: u64, tmpfs_mib: u64, occupant: Occupant) {
        self.memory_mib = self.memory_mib.saturating_add(memory_mib);
        self.tmpfs_mib = self.tmpfs_mib.saturating_add(tmpfs_mib);
        self.jobs += 1;
        if !occupant.holds_capabilities() {
            self.readings += 1;
        }
    }

    /// The clause naming what is already taken here (E-04).
    fn sentence(&self) -> String {
        let of_which = of_which(usize::try_from(self.readings).unwrap_or(usize::MAX));
        match self.jobs {
            0 => "nothing is running there".to_owned(),
            1 => format!(
                "1 job running there holds {} of memory and {} of tmpfs{of_which}",
                herd_proto::size_mib(self.memory_mib),
                herd_proto::size_mib(self.tmpfs_mib)
            ),
            many => format!(
                "{many} jobs running there hold {} of memory and {} of tmpfs{of_which}",
                herd_proto::size_mib(self.memory_mib),
                herd_proto::size_mib(self.tmpfs_mib)
            ),
        }
    }
}

/// What every runner has reserved, read from the job records alone (D79).
pub fn reserved(records: &[JobRecord]) -> BTreeMap<String, Reserved> {
    let mut taken: BTreeMap<String, Reserved> = BTreeMap::new();
    for (runner, record) in out_there(records) {
        let here = taken.entry(runner.to_owned()).or_default();
        match record.memory_held_mib() {
            Ok(memory) => here.take(
                memory.unwrap_or(0),
                record.tmpfs_need_mib(),
                record.occupant(),
            ),
            Err(why) => {
                here.take(0, record.tmpfs_need_mib(), record.occupant());
                here.uncostable.push((record.id.clone(), why));
            }
        }
    }
    taken
}

/// How many jobs may hold a slot of this job's labels at once on this runner.
///
/// The smallest holder count among the capabilities it asks for, or one when
/// it asks for nothing counted. That is the number of jobs *like this one*
/// that can be running here together, and therefore the number the host's
/// memory share has to be divided between (E-22): two build slots on a 31 GiB
/// server are `-j 6` each, not `-j 12` each, because the second build is not
/// hypothetical — it is what the second slot is for.
fn slots_for(job_labels: &LabelSet, capacity: &RunnerCapacity) -> u32 {
    job_labels
        .iter()
        .filter_map(|label| capacity.offer().available(label))
        .filter_map(|capability| capability.kind.holders_allowed())
        .min()
        .unwrap_or(1)
        .max(1)
}

/// The clause naming what this job takes on this runner (E-36).
///
/// One phrase per label that is governed by a count, with the figures: a
/// claim line that said "labels build" and no more would not say whether the
/// machine now has a slot left.
///
/// A requirement reading takes **none** of them and the line says so in as
/// many words (D159), so that the record of a reading cannot be read as a
/// record of something that held the machine. The one thing it does take is
/// a place among the readings that machine may run at once, and the line says
/// which place of how many (D169) — a figure a human cannot break down is a
/// figure that gets broken down by hand, at night.
fn what_it_takes(
    job_labels: &LabelSet,
    seat: &Seat,
    occupant: Occupant,
    readings_at_once: u32,
) -> String {
    if !occupant.holds_capabilities() {
        return format!(
            "it is a requirement reading and holds none of them — no slot, no host-wide \
             machine, nothing emptied — because it builds and boots nothing; it is matched on \
             the labels {} offers and has not denied, and nothing about contention (D159); it \
             is reading {} of the {readings_at_once} this hub runs there at once \
             (--readings-at-once, D169)",
            seat.name,
            seat.holdings.readings_running() + 1
        );
    }
    let mut clauses = Vec::new();
    for label in job_labels.iter() {
        let Some(capability) = seat.capacity.offer().available(label) else {
            continue;
        };
        match capability.kind.holders_allowed() {
            Some(1) if capability.kind.is_host_wide() => clauses.push(format!(
                "{label} is host-wide and this runner is idle, so nothing else runs here \
                 until this ends"
            )),
            Some(slots) => clauses.push(format!(
                "{label} slot {} of {slots}",
                seat.holdings.holders_of(label) + 1
            )),
            None => clauses.push(format!("{label} is shared")),
        }
    }
    if clauses.is_empty() {
        return "it needs no capability".to_owned();
    }
    clauses.join(", ")
}

/// What admission says about one job on one machine (E-29, E-31).
#[derive(Debug, Clone)]
enum Admission {
    /// It fits, and here is what it takes: memory, then tmpfs, in MiB.
    Fits { memory_mib: u64, tmpfs_mib: u64 },
    /// The room is there on this machine but something running has it. The
    /// job **waits** — short is never a failure (E-31) — and it sticks to this
    /// machine, because a job ending here is what frees the room.
    ShortNow(String),
    /// Not here, whatever happens here: this machine could never hold this
    /// job even empty, or it does not say what it has. The job does not stick
    /// to it, which is the whole point — a job told it is "waiting for
    /// build-host" when build-host could never hold it waits for ever for
    /// something that cannot happen.
    NotHere(String),
}

/// Whether this job fits on this machine, with every figure in the sentence
/// (E-04, E-29, E-31).
///
/// `threads` is the `-j` already derived for this seat, because the need is a
/// function of it for a job that declared a per-thread figure, and N is chosen
/// before any requirement is read (D82).
fn admission(record: &JobRecord, seat: &Seat, threads: Option<u32>) -> Admission {
    // **First, and before any figure about size**: the count the runner
    // itself put on how many jobs it will hold at once (D181, invariant 2).
    //
    // Here rather than anywhere else because this is the function that reads
    // [`Seat::reserved`], and `reserved.jobs` is already the count this needs
    // — the number of attempts out on this machine that have not ended, taken
    // off the records by the one `out_there` filter every other figure about
    // a machine is taken off (D79), and taken *again* by `claim` inside the
    // pass so that two jobs offered together contend for the last place. A
    // second counter kept beside it would be a second answer to one question,
    // and the first job counted by one and not the other would be a machine
    // the hub reads as having room on top of work that is running.
    //
    // Before the memory arithmetic because it is the cheaper and the blunter
    // of the two, and because the sentence a waiting job should read is the
    // one that is actually holding it up: a machine at its own ceiling is
    // full whatever its memory says, and a job told how many mebibytes are
    // free on a machine that will not take it anyway has been told the wrong
    // thing. It does not replace what follows — a job too big for this
    // machine is still refused below, on a machine holding nothing at all.
    //
    // Counted over **every** job out there, readings included (D159, D169),
    // because the runner's own ceiling counts every job it is running and the
    // two sides have to be counting the same set. A ceiling the hub applied
    // to a smaller set than the runner does would let the runner refuse work
    // the hub thought it had room for, which is the whole defect this closes.
    if let Some(at_once) = seat.jobs_at_once {
        if at_once == 0 {
            // A runner that says it holds nothing is not a machine this job
            // waits for. `NotHere` and not `ShortNow`, because `ShortNow`
            // sticks the job to the machine until the stickiness limit, and a
            // machine whose ceiling is zero has no job that could end there
            // to free the place: the wait it would start is one nothing on
            // that machine can finish. Our own runner never announces zero —
            // `jobs_at_once` is floored at one — so this is about a peer that
            // is not ours, and the reading that costs nothing is the right
            // one.
            return Admission::NotHere(format!(
                "{} says it holds 0 jobs at once, so nothing can ever be placed there and \
                 nothing waits for it",
                seat.name
            ));
        }
        // The hub's count, and the machine's own where the machine has
        // contradicted it (D182). The larger wins, because the only way
        // the two differ is the hub's being stale: a runner that redialled
        // after an outage is holding jobs whose attempts the watchdog ended
        // while the link was down, so the records read that seat as empty
        // and the machine knows it is full. Taking the larger is also what
        // makes this correction cost nothing in the ordinary case, where the
        // machine has said nothing and the figure is the hub's alone.
        let out_there = seat
            .reserved
            .jobs
            .max(seat.it_said_it_is_holding.unwrap_or(0));
        if out_there >= at_once {
            return Admission::ShortNow(format!(
                "on {}, {out_there} job(s) already out there and \
                 {A_CEILING_THE_RUNNER_ANNOUNCED} {at_once} at once, so it takes nothing more \
                 until one of them ends: {}{}",
                seat.name,
                seat.reserved.sentence(),
                match seat.it_said_it_is_holding {
                    // Said out loud, because it is the one figure here that
                    // did not come off the records: an operator reading
                    // "0 job(s) out there and it holds at most 2" would be
                    // reading a contradiction otherwise.
                    Some(holding) if holding > seat.reserved.jobs => format!(
                        " — {holding} of them by the machine's own count, which it gave when it \
                         last handed an assignment back; this hub's records knew of {}",
                        seat.reserved.jobs
                    ),
                    _ => String::new(),
                }
            ));
        }
    }
    // A job already out there whose holding cannot be costed stops this
    // machine admitting anything that costs memory. Named, never read as
    // zero: the alternative is a machine that reads idle on top of a build.
    let needs_memory = match record.memory_need_mib(threads) {
        Ok(need) => need,
        // Unreachable for a queued job: `nothing_can_ever_run_this` refuses a
        // per-thread figure with no `{j}`, and a seat that derives no `-j`
        // has already answered `NotHere`. Kept because the one thing it must
        // not do is cost the job at zero.
        Err(why) => return Admission::NotHere(format!("{}: {why}", record.id)),
    };
    if needs_memory.is_some_and(|need| need > 0) {
        if let Some((job, why)) = seat.reserved.uncostable.first() {
            return Admission::NotHere(format!(
                "{} is holding {job} and {why}, so nothing more is admitted there until it ends",
                seat.name
            ));
        }
    }
    let memory_mib = match needs_memory {
        None => 0,
        Some(need) => match seat.host.admissible_memory_mib() {
            // A figure that cannot be read is not read as zero, and is not
            // read as plenty either (D23's rule, and M4's `Some(0)` defect).
            None => {
                return Admission::NotHere(format!(
                    "{} does not say {}, so a {} job cannot be admitted there",
                    seat.name,
                    match (seat.host.memory_mib, seat.host.memory_floor_mib) {
                        (None, None) => "how much memory it lends or how much it keeps back",
                        (None, _) => "how much memory it lends",
                        _ => "how much memory it keeps back for itself",
                    },
                    herd_proto::size_mib(need)
                ))
            }
            Some(admissible) => {
                let free = admissible.saturating_sub(seat.reserved.memory_mib);
                if need > admissible {
                    return Admission::NotHere(format!(
                        "{} admits at most {} of memory ({} lent, {} kept as its own floor) and \
                         this job needs {}, so it can never fit there however empty it gets",
                        seat.name,
                        herd_proto::size_mib(admissible),
                        herd_proto::size_mib(seat.host.memory_mib.unwrap_or(0)),
                        herd_proto::size_mib(seat.host.memory_floor_mib.unwrap_or(0)),
                        herd_proto::size_mib(need)
                    ));
                }
                if need > free {
                    return Admission::ShortNow(format!(
                        "on {}, {} free of its {} memory admission, {} needed: {}",
                        seat.name,
                        herd_proto::size_mib(free),
                        herd_proto::size_mib(admissible),
                        herd_proto::size_mib(need),
                        seat.reserved.sentence()
                    ));
                }
                need
            }
        },
    };
    let tmpfs_mib = record.tmpfs_need_mib();
    if tmpfs_mib > 0 {
        let Some(capacity) = seat.host.tmpfs_mib else {
            return Admission::NotHere(format!(
                "{} does not say how much tmpfs it has, so a {} reservation cannot be taken \
                 there — a figure that cannot be read is not read as zero",
                seat.name,
                herd_proto::size_mib(tmpfs_mib)
            ));
        };
        if tmpfs_mib > capacity {
            return Admission::NotHere(format!(
                "{} has {} of tmpfs in all and this job needs {}, so no reservation there can \
                 ever cover it",
                seat.name,
                herd_proto::size_mib(capacity),
                herd_proto::size_mib(tmpfs_mib)
            ));
        }
        let free = capacity.saturating_sub(seat.reserved.tmpfs_mib);
        if tmpfs_mib > free {
            return Admission::ShortNow(format!(
                "on {}, {} free in tmpfs, {} needed ({} of tmpfs there in all): {}",
                seat.name,
                herd_proto::size_mib(free),
                herd_proto::size_mib(tmpfs_mib),
                herd_proto::size_mib(capacity),
                seat.reserved.sentence()
            ));
        }
    }
    Admission::Fits {
        memory_mib,
        tmpfs_mib,
    }
}

/// What this runner can do with this job right now.
fn judge(record: &JobRecord, seat: &Seat, placing: &Placing, now: Timestamp) -> Verdict {
    if seat.draining {
        return Verdict::NotHere(format!(
            "{} is draining and takes no new job, so nothing waits for it",
            seat.name
        ));
    }
    // The owner's switch, judged exactly as the drain is and for D80's reason:
    // a machine its owner has withdrawn is not available, so this job does not
    // stick to it and is free to go elsewhere in this same pass (E-33).
    if !seat.switch.claims() {
        return Verdict::NotHere(format!(
            "{} is switched {} by its owner and claims nothing, so nothing waits for it \
             (E-33)",
            seat.name,
            seat.switch.sentence()
        ));
    }
    // A host being held empty for another job takes nothing, whatever its
    // slots say (E-27). Judged here, before the capability algebra, because
    // the reservation is about the *host* and not about any one capability —
    // and a job told "the build slot is free" on a machine that is being
    // emptied would be a job claimed into the middle of a drain.
    if let Some((held_for, label)) = &seat.reserved_for {
        if held_for != &record.id {
            return Verdict::NotHere(format!(
                "{} is being emptied for {held_for}, which needs {label} and starts only on a \
                 quiet host, so nothing new is claimed there (E-27)",
                seat.name
            ));
        }
    }
    // What this job *is*, which three of the gates below turn on (D159).
    let occupant = record.occupant();
    // What M5's algebra decides: the kinds, under contention with what is
    // already running here (E-25). A requirement reading is judged on what
    // this runner offers and denies and on nothing about contention, which
    // [`Holdings::check`] is where that is written down.
    let contention = seat.holdings.check(record.labels_needed(), occupant);
    // **The impossible case is judged first, before anything about the
    // moment.** A label set that contradicts itself is a fact about the
    // submission, and a job asking for two labels that exclude each other —
    // one of which a lock file outside Herd happens to be holding — would
    // otherwise be told it is waiting for that lock, for ever, for a job that
    // could never have run whatever the lock did. Order matters here and
    // nowhere else in this function.
    if let Err(contention) = &contention {
        if contention.is_impossible() {
            return Verdict::Impossible(contention.reason());
        }
    }
    // What M5 decided: offered and not suspended. A label this runner does
    // not offer is not about the moment; a probe outside Herd holding one is
    // (D48).
    //
    // **And that is exactly where a requirement reading is let through**
    // (D159). The two halves of this judgement are the two halves of D159's
    // distinction, already told apart by `clears_with_time` and already named
    // by `held_up_by`'s own doc comment: `false` is the *machine* — a label
    // it does not offer, a label its owner denied — which is eligibility, and
    // a reading is held to it, because a reading placed on a machine that
    // could not run the command reads the wrong machine's capabilities and
    // answers with figures that mean nothing. `true` is the *moment* — a
    // probe outside Herd holding the capability, or a probe that cannot be
    // read at all — which is availability, and a reading is given none of the
    // capability it would be about: it runs a `--labels` line, which does not
    // touch the GPU the lock protects.
    //
    // The failure direction decides it. A reading that runs while a probe
    // says busy costs one `--labels` line on a machine something else is
    // using. A reading that waits for a probe nobody can read waits for ever,
    // and every job its answer unblocks waits behind it — which is the
    // incident D159 was written for, in a different costume. The cheap
    // mistake is the one to make.
    if let Some(held_up) = seat.capacity.held_up_by(record.labels_needed()) {
        if held_up.clears_with_time {
            if occupant.holds_capabilities() {
                return Verdict::Later(held_up.reason);
            }
        } else {
            return Verdict::NotHere(held_up.reason);
        }
    }
    if let Err(contention) = contention {
        let reason = format!("on {}, {contention}", seat.name);
        return if contention.clears_with_time() {
            Verdict::Later(reason)
        } else {
            Verdict::NotHere(reason)
        };
    }
    // And the one thing a reading *does* take of a machine: a place among the
    // readings it is running (D169).
    //
    // D159 is kept whole — a reading holds no slot, empties no host and waits
    // for no probe — and this is not one of those. It is a bound on the
    // kind's own concurrency, read off the same holdings every other figure
    // here is read off (D79), so there is no counter to leak and a reading
    // that ends any of the eight ways a job ends gives its place back by
    // ending. Without it nothing bounded readings at all: 214 jobs submitted
    // in one second put some two hundred of them on one runner, and the host
    // they were placed on stopped being a host (D169).
    //
    // `Later` and never `NotHere`: a reading over the cap is **queued**, not
    // refused and not sent to a machine that suits it less. That is also what
    // keeps it off the dead-runner timer — a reading that is waiting here has
    // no attempt in the air, so there is nothing for the watchdog to count.
    if !occupant.holds_capabilities() {
        let at_once = placing.readings_at_once();
        let already = u32::try_from(seat.holdings.readings_running()).unwrap_or(u32::MAX);
        if already >= at_once {
            return Verdict::Later(format!(
                "on {}, {already} requirement reading(s) are already running and this hub \
                 runs at most {at_once} there at once (--readings-at-once); this reading \
                 takes no slot and no capability, and a machine given every reading at once \
                 is a machine that stops answering (D169)",
                seat.name
            ));
        }
    }
    // And what M7 adds: the parallelism one slot of this machine gets (E-22).
    // **Before admission, always**, because the memory a build needs is that
    // figure times its per-thread declaration and N is chosen first (D82).
    let derived = match record.build_thread_mib {
        None => None,
        Some(build_thread_mib) => match seat.host.parallelism(
            build_thread_mib,
            slots_for(record.labels_needed(), &seat.capacity),
        ) {
            Ok(derived) => Some(derived),
            // A machine that does not say how big it is cannot be given a job
            // whose `-j` is derived from its size. Not a failure: another
            // machine may say. Never a figure invented for it.
            Err(why) => return Verdict::NotHere(format!("{} {why}", seat.name)),
        },
    };
    // And what M8 adds: whether the room is there (E-29, E-31).
    match admission(record, seat, derived.map(|derived| derived.threads)) {
        Admission::Fits {
            memory_mib,
            tmpfs_mib,
        } => {
            // And last, E-27's drain-first: a job whose capability its
            // machine's operator declared `drain_first` starts only when
            // nothing else of this runner's is running. Last on purpose — a
            // job whose lease is held, or which does not fit, has a better
            // reason to be waiting than an empty host it could not use
            // anyway, and the sentence an operator reads should be that one.
            if let Some(label) = needs_a_quiet_host(record, seat) {
                if seat.holdings.running() > 0 {
                    return Verdict::Later(quiet_host_sentence(
                        record,
                        seat,
                        &label,
                        placing.drain_first_limit(),
                        now,
                    ));
                }
            }
            Verdict::Now(Fitted {
                derived,
                memory_mib,
                tmpfs_mib,
            })
        }
        // Short now is a wait, never a failure (E-31), and the job sticks to
        // this machine because a job ending here is what frees the room.
        Admission::ShortNow(why) => Verdict::Later(why),
        // More than this machine could ever give, or a machine that does not
        // say: the job must *not* stick to it. That is the distinction E-31
        // turns on — "wait, never fail" is about room that will come back,
        // not about a machine that can never have it.
        Admission::NotHere(why) => Verdict::NotHere(why),
    }
}

/// Which runners could run this job, now or later, and which of its labels
/// each of them answers for (E-07).
///
/// The *eligible* set, in E-19's sense: available machines of this job's
/// labels, whether or not one is free this second. Built from the same
/// [`judge`] the claim uses, so a dry run cannot name a machine the real pass
/// would refuse — the reverse of the defect E-07 is most easily broken by.
pub fn eligible(
    record: &JobRecord,
    seats: &[Seat],
    now: Timestamp,
    placing: &Placing,
) -> Vec<(String, String)> {
    seats
        .iter()
        .filter(|seat| {
            matches!(
                judge(record, seat, placing, now),
                Verdict::Now(_) | Verdict::Later(_)
            )
        })
        .map(|seat| {
            let answered: Vec<&str> = record
                .labels_needed()
                .iter()
                .filter(|label| seat.capacity.offer().available(label).is_some())
                .collect();
            let said = if answered.is_empty() {
                "no capability needed".to_owned()
            } else {
                answered.join(", ")
            };
            (seat.name.clone(), said)
        })
        .collect()
}

/// Which capability of this job's, on this runner, asks for a quiet host
/// (E-27).
///
/// `None` for every ordinary job, which is the point: nothing in Herd reads a
/// label's spelling, and `metal-pc` only behaves differently from `build`
/// because the PC's own capability file says `"drain_first": true` about it
/// (D86). A label the runner does not offer cannot ask for anything.
///
/// **And `None` for a requirement reading, always** (D159). Asking for a
/// quiet host is the strongest thing a job can do to a machine — E-27 lets it
/// hold one empty for fifteen minutes — and a reading that builds and boots
/// nothing must not be able to. It is one function rather than two checks
/// because both callers need it: [`judge`], which would make the reading wait
/// for the host to empty, and [`reservations`], which would empty the host
/// *for* it. A reading still **respects** a host being emptied for somebody
/// else, which is [`Seat::reserved_for`] and is judged before this: a stream
/// of readings allowed to slip onto a draining machine would starve the drain
/// for ever, which is the attack E-27 exists to answer.
fn needs_a_quiet_host(record: &JobRecord, seat: &Seat) -> Option<String> {
    if record.is_a_reading() {
        return None;
    }
    record.labels_needed().iter().find_map(|label| {
        seat.capacity
            .offer()
            .available(label)
            .filter(|capability| capability.drain_first)
            .map(|_| label.to_owned())
    })
}

/// Whether a drain-first job may still hold a host empty, and for how much
/// longer (E-27).
///
/// Measured from the job's submission, because that is the one moment about
/// this job that is written down (D79).
fn may_still_hold_a_host(record: &JobRecord, limit: Duration, now: Timestamp) -> bool {
    now.since(record.submitted_at) < limit
}

/// What a job waiting for a quiet host says, with its figures (E-04, E-27).
fn quiet_host_sentence(
    record: &JobRecord,
    seat: &Seat,
    label: &str,
    limit: Duration,
    now: Timestamp,
) -> String {
    let waited = now.since(record.submitted_at);
    let running = seat.holdings.running();
    // What those jobs are, when some of them are requirement readings (D159):
    // a drain that is waiting on a `--labels` line is a drain whose operator
    // should be told so, and the figure alone never says it.
    let of_which = of_which(seat.holdings.readings_running());
    if may_still_hold_a_host(record, limit, now) {
        format!(
            "{} is being emptied for it: {label} starts only on a quiet host and {running} \
             job(s) are still running there{of_which}, so {} claims nothing new until they \
             end or until the {} drain-first limit is reached (--drain-first-limit); it has \
             waited {}",
            seat.name,
            seat.name,
            roughly(limit),
            roughly(waited)
        )
    } else {
        format!(
            "{label} starts only on a quiet host and {running} job(s) are still running on \
             {}{of_which}; the {} this job may hold a machine empty for is spent after {}, so \
             {} claims other work again and this job now waits for its host to go quiet on \
             its own. The drain did not win: raise this job's priority, or stop submitting \
             work to {} (--drain-first-limit)",
            seat.name,
            roughly(limit),
            roughly(waited),
            seat.name,
            seat.name
        )
    }
}

/// Which hosts are being held empty for which job (E-27).
///
/// Read from the queued records and the fleet, at the start of every pass, and
/// from nothing else — the same rule as the slots and the reservations (D79).
/// A hub that remembered its drains would have a third thing to rebuild after
/// a restart, and the one it forgot would be a machine held empty for a job
/// that ended an hour ago.
///
/// Computed **before** the pass places anything, which is what makes the drain
/// win: a reservation discovered while walking the queue in priority order
/// would be found only after everything ahead of the metal job had already
/// been claimed, and a stream of builds would then starve it for ever. The
/// queue order still decides *which* job owns a contested host — the first one
/// in it.
pub fn reservations(
    queued: &[JobRecord],
    seats: &[Seat],
    now: Timestamp,
    limit: Duration,
) -> BTreeMap<String, (JobId, String)> {
    let mut held: BTreeMap<String, (JobId, String)> = BTreeMap::new();
    for record in queued {
        if !may_still_hold_a_host(record, limit, now) {
            continue;
        }
        for seat in seats {
            if seat.draining || !seat.switch.claims() {
                continue;
            }
            let Some(label) = needs_a_quiet_host(record, seat) else {
                continue;
            };
            held.entry(seat.name.clone())
                .or_insert_with(|| (record.id.clone(), label));
        }
    }
    held
}

/// Where this job goes now, among these runners.
///
/// `now` and the limit are arguments rather than read from a clock here, for
/// the reason `crate::queue` gives: a test makes three hours pass through the
/// code the hub runs, not through a copy of it.
pub fn place(
    record: &JobRecord,
    seats: &mut [Seat],
    now: Timestamp,
    stickiness: Stickiness,
    placing: &Placing,
) -> Placement {
    // A requirement the record does not carry is refused by name, never read
    // as zero and placed anywhere (D74's rule, and the defect behind it): a
    // job asking for a derived `-j` with nowhere to put it would be recorded
    // as `-j 7` and run with its tool's own default.
    //
    // **First, before the clock and before the parent.** A job whose own
    // figures contradict each other is a bug a human has to fix, and a job
    // cancelled by its deadline would have hidden it — the operator would
    // read "cancelled (deadline)" and never learn that the submission was
    // malformed. This is M7's own lesson in a new place: judge the fact about
    // the submission before the fact about the moment.
    if let Some(why) = nothing_can_ever_run_this(record) {
        return Placement::Impossible { why };
    }
    // Then the clock (E-09). **Before anything about the fleet**, including
    // an empty fleet: a deadline is the bound D88 leaves on a job no connected
    // runner could ever fit, and a bound that only fired when a machine
    // happened to be connected would not be one. Judged against the pass's
    // single `now`, so a job is either claimed in this pass or expired in it,
    // never both.
    if let Some(why) = past_its_deadline(record, now) {
        return Placement::Cancel { why };
    }
    // And then the one parent, if it named one (E-10).
    if let Some(placement) = placing.parent.judge() {
        return placement;
    }
    if seats.is_empty() {
        return Placement::Wait {
            why: "no runner is connected to this hub".to_owned(),
        };
    }
    let verdicts: Vec<Verdict> = seats
        .iter()
        .map(|seat| judge(record, seat, placing, now))
        .collect();
    let is_eligible = |at: usize| matches!(verdicts[at], Verdict::Now(_) | Verdict::Later(_));

    // The runners this job is entitled to wait for: the ones that are
    // available — connected, not draining — and are machines of its labels,
    // whether or not they are free this second (E-19).
    let eligible = (0..seats.len()).filter(|at| is_eligible(*at)).count();
    let Some(best) = (0..seats.len())
        .filter(|at| is_eligible(*at))
        .map(|at| seats[at].preference)
        .max()
    else {
        // Nobody here can run it, now or later. Either it is impossible
        // anywhere — and then it ends (E-36) — or the fleet simply has no
        // machine for it today, and a machine that has it may dial in
        // tomorrow, so it waits.
        if let Some(why) = verdicts.iter().find_map(|verdict| match verdict {
            Verdict::Impossible(why) => Some(why.clone()),
            _ => None,
        }) {
            return Placement::Impossible { why };
        }
        let why = reasons(&verdicts, |verdict| match verdict {
            Verdict::NotHere(why) => Some(why.clone()),
            _ => None,
        });
        return Placement::Wait {
            why: format!("no connected runner can run it: {why}"),
        };
    };

    // Among the most preferred eligible runners, one that is free now takes
    // it. Ties go by name, so that the same fleet makes the same choice
    // twice: a placement nobody can predict is a placement nobody can
    // explain.
    let free = |only_at_best: bool| {
        let mut free: Vec<usize> = (0..seats.len())
            .filter(|at| matches!(verdicts[*at], Verdict::Now(_)))
            .filter(|at| !only_at_best || seats[*at].preference == best)
            .collect();
        free.sort_by(|left, right| {
            seats[*right]
                .preference
                .cmp(&seats[*left].preference)
                .then_with(|| seats[*left].name.cmp(&seats[*right].name))
        });
        free
    };
    let fitted_at = |at: usize| match &verdicts[at] {
        Verdict::Now(fitted) => *fitted,
        // Only ever asked of a seat `free` returned, which is a `Now`.
        _ => Fitted {
            derived: None,
            memory_mib: 0,
            tmpfs_mib: 0,
        },
    };
    if let Some(at) = free(true).first().copied() {
        return claim(
            record,
            &mut seats[at],
            fitted_at(at),
            eligible,
            None,
            placing,
        );
    }

    // The most preferred eligible runners are all busy. E-19: the job waits
    // for them while they are available, and does not fall back to a lesser
    // machine — up to the visible limit.
    let waited = now.since(record.submitted_at);
    let sticking = (0..seats.len())
        .filter(|at| matches!(verdicts[*at], Verdict::Later(_)) && seats[*at].preference == best)
        .map(|at| seats[at].name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let because = (0..seats.len())
        .filter(|at| seats[*at].preference == best)
        .filter_map(|at| match &verdicts[at] {
            Verdict::Later(why) => Some(why.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("; and ");
    let elsewhere = free(false);
    if waited >= stickiness.limit {
        if let Some(at) = elsewhere.first().copied() {
            let moved = format!(
                "after waiting {} for {sticking} (preference {best}), past the {} stickiness \
                 limit (--stickiness-limit)",
                roughly(waited),
                roughly(stickiness.limit)
            );
            return claim(
                record,
                &mut seats[at],
                fitted_at(at),
                eligible,
                Some(moved),
                placing,
            );
        }
    }
    let mut why = format!("it is waiting for {sticking}: {because}");
    match elsewhere.first().copied() {
        None => why.push_str("; no other eligible runner is free either"),
        Some(at) => why.push_str(&format!(
            "; {} could take it and is less preferred ({} against {best}), so this job does \
             not move there (E-19)",
            seats[at].name, seats[at].preference
        )),
    }
    why.push_str(&format!(
        ". It moves to another eligible runner only when {sticking} stops being available, \
         or after {} in the queue (--stickiness-limit); it has waited {}",
        roughly(stickiness.limit),
        roughly(waited)
    ));
    Placement::Wait { why }
}

/// Why this job is over before it started, if its deadline has passed (E-09).
///
/// Only ever asked of a job the pass found **queued**: the queue is queued
/// jobs and nothing else (`queue::order`), so a job that has started cannot
/// reach here at all. That is the design rather than a happy accident — E-09
/// cancels an *unstarted* job, and a running job past its deadline is M4's
/// time cap's business and nobody else's (exit 124).
///
/// The sentence carries three things, and the third is the one an operator
/// needs most: the moment that passed, how long ago, and **what the job was
/// waiting for when it passed**. A job cancelled by a deadline it only missed
/// because the one machine of its labels was switched off is a correct
/// cancellation and a baffling one, unless the reason says so.
fn past_its_deadline(record: &JobRecord, now: Timestamp) -> Option<String> {
    let deadline = record.deadline?;
    if now <= deadline {
        return None;
    }
    let mut why = format!(
        "cancelled (deadline): it had not started by {deadline}, which is {} ago, and a \
         deadline cancels a job that has not started (E-09). It was submitted {} before that \
         moment",
        roughly(now.since(deadline)),
        roughly(deadline.since(record.submitted_at))
    );
    match &record.waiting_for {
        Some(waiting) => why.push_str(&format!(". When the moment came it was: {waiting}")),
        None => why.push_str(
            ". Nothing had been recorded about why it was waiting, which means no dispatch \
             pass had judged it yet",
        ),
    }
    Some(why)
}

/// Every reason of one kind, in one clause.
///
/// Joined rather than reduced to the first, because a job that cannot run has
/// as many reasons as the fleet has machines, and the one an operator needs is
/// not predictably the first.
fn reasons(verdicts: &[Verdict], of: impl Fn(&Verdict) -> Option<String>) -> String {
    verdicts
        .iter()
        .filter_map(of)
        .collect::<Vec<_>>()
        .join("; and ")
}
/// Take the labels for this job on this runner and build E-36's line.
fn claim(
    record: &JobRecord,
    seat: &mut Seat,
    fitted: Fitted,
    eligible: usize,
    moved: Option<String>,
    placing: &Placing,
) -> Placement {
    let Fitted {
        derived,
        memory_mib,
        tmpfs_mib,
    } = fitted;
    let occupant = record.occupant();
    let takes = what_it_takes(
        record.labels_needed(),
        seat,
        occupant,
        placing.readings_at_once(),
    );
    let labels = if record.labels_needed().is_empty() {
        "none".to_owned()
    } else {
        record.labels_needed().to_string()
    };
    let mut why = format!(
        "claimed by {}: labels {labels}; {takes}",
        seat.name, // the runner first, as E-36's own example has it
    );
    if let Some(derived) = derived {
        why.push_str(&format!("; {}", derived.sentence()));
    }
    // What admission gave it, with the room that is left afterwards (E-29,
    // E-31, E-36): a claim line that did not say would not let an operator
    // work out why the next job waited.
    why.push_str(&format!("; {}", admitted_sentence(seat, &fitted)));
    // And the load, which is recorded here and gates nothing (E-32).
    why.push_str(&format!("; {}", load_sentence(seat)));
    why.push_str(&format!(
        "; preference {}, {}",
        seat.preference,
        match eligible {
            1 => "the only eligible runner".to_owned(),
            n => format!("the most preferred of {n} eligible runners"),
        }
    ));
    if let Some(moved) = moved {
        why.push_str(&format!("; {moved}"));
    }
    // Taken under the same lock the pass holds, and taken *here*, so that the
    // next job offered in this pass contends with this one (D79): an
    // exclusive job and a counted job arriving in one pass must not both be
    // admitted.
    // A requirement reading is written down here too, holding nothing: it is
    // running on this machine, so a host-wide job waits for it and the next
    // sentence about this machine can name it (D159).
    if let Err(contention) =
        seat.holdings
            .admit(&record.id, record.labels_needed().clone(), occupant)
    {
        // The judgement above said this was free. Only a bug can get here,
        // and the one thing it must not do is place the job anyway.
        return Placement::Wait {
            why: format!(
                "{} could not be given its labels on {} after all: {contention}",
                record.id, seat.name
            ),
        };
    }
    // And the room, taken *here*, inside the same pass, so that the next job
    // offered is weighed against this one and not only against what was
    // running when the pass began (E-32's host-wide bound). The slot version
    // of this is M7's; these are the two figures M8 adds.
    seat.reserved.take(memory_mib, tmpfs_mib, occupant);
    let command = match derived {
        Some(derived) => {
            herd_proto::with_parallelism(&record.command, record.requester_args(), derived.threads)
        }
        None => record.command.clone(),
    };
    Placement::Claim {
        runner: seat.name.clone(),
        why,
        command,
        parallelism: derived,
    }
}

/// What admission gave this job, and what is left afterwards (E-29, E-31).
///
/// Taken from the seat **before** the claim takes the room, so the figures
/// read as the arithmetic an operator would do: this much was free, this much
/// went to this job, this much remains.
fn admitted_sentence(seat: &Seat, fitted: &Fitted) -> String {
    let mut said = Vec::new();
    match seat.host.admissible_memory_mib() {
        Some(admissible) => said.push(format!(
            "admitted {} of memory, {} free of {} after it ({} lent, {} kept as its own floor)",
            herd_proto::size_mib(fitted.memory_mib),
            herd_proto::size_mib(
                admissible
                    .saturating_sub(seat.reserved.memory_mib)
                    .saturating_sub(fitted.memory_mib)
            ),
            herd_proto::size_mib(admissible),
            herd_proto::size_mib(seat.host.memory_mib.unwrap_or(0)),
            herd_proto::size_mib(seat.host.memory_floor_mib.unwrap_or(0)),
        )),
        // Only reachable for a job that declares no memory need at all: one
        // that does is refused on a machine with no figures.
        None => said.push(
            "it declares no memory need, and this machine does not say what it admits".to_owned(),
        ),
    }
    match (fitted.tmpfs_mib, seat.host.tmpfs_mib) {
        (0, _) => said.push("it reserves no tmpfs".to_owned()),
        (reserved, Some(capacity)) => said.push(format!(
            "reserved {} in tmpfs, {} free of {} after it",
            herd_proto::size_mib(reserved),
            herd_proto::size_mib(
                capacity
                    .saturating_sub(seat.reserved.tmpfs_mib)
                    .saturating_sub(reserved)
            ),
            herd_proto::size_mib(capacity)
        )),
        // Unreachable: a tmpfs need on a machine with no tmpfs figure is
        // refused by name in `admission`.
        (reserved, None) => said.push(format!(
            "reserved {} in tmpfs on a machine that does not say how much it has",
            herd_proto::size_mib(reserved)
        )),
    }
    said.join("; ")
}

/// The host's load, recorded on the placement and gating nothing (E-32).
///
/// It is written into the line for one reason: E-32 asks for the figure to be
/// *recorded* and explicitly not used as a gate, and a figure nobody can see
/// is indistinguishable from a figure nobody kept. There is no comparison
/// against it anywhere in this module.
fn load_sentence(seat: &Seat) -> String {
    match seat.load_milli {
        Some(milli) => format!(
            "load {}.{:02} there, recorded and not a gate (E-32)",
            milli / 1000,
            milli % 1000 / 10
        ),
        None => "it does not say what its load is, which gates nothing either (E-32)".to_owned(),
    }
}

/// Why no machine could ever run this job, if none could (E-36).
///
/// Two of these, and both are about the job's own requirements rather than
/// about any machine: a per-thread memory figure of zero, and a job that asks
/// for a derived `-j` with nowhere in its command to put it. Both are
/// refusals a human has to fix, so both are terminal — a job that waited for
/// them would wait for ever (D13, D38).
///
/// A label nobody offers is **not** here: that is a fleet that has no such
/// machine *today*, and the job waits. The one label set that is impossible
/// by its own contents — two labels that exclude each other — is found by the
/// algebra, per runner, in [`judge`].
fn nothing_can_ever_run_this(record: &JobRecord) -> Option<String> {
    // Two figures about one question. A build declares what one of its
    // threads needs and the hub multiplies by the `-j` it derived; a test
    // declares what the whole job needs. A job saying both would be admitted
    // against one figure and capped against the other, and which one is which
    // must not be a guess.
    if let (Some(per_thread), Some(whole)) = (record.build_thread_mib, record.memory_mib) {
        return Some(format!(
            "this job says one of its build threads needs {per_thread} MiB and also that the \
             whole job needs {whole} MiB; those are two answers to one question, so a job \
             declares one or the other and never both"
        ));
    }
    // Zero where a figure was meant. Terminal for exactly the reason a
    // per-thread figure of zero is: a job of no memory is not a job that
    // needs none, it is a declaration nobody can admit against, and a human
    // has to fix it (D38 — a job that waited for this would wait for ever).
    // A *tmpfs* figure of zero is not this: `build` and `check` really need
    // no tmpfs, and 0 is how they say so.
    if record.memory_mib == Some(0) {
        return Some(
            "this job says the whole of it needs 0 MiB of memory, which is not a figure: a job \
             that needs no memory declares nothing rather than zero"
                .to_owned(),
        );
    }
    let mib = record.build_thread_mib?;
    if mib == 0 {
        return Some(
            "this job says one of its build threads needs 0 MiB, which is not a figure: a \
             job asking for a derived -j has to say how much memory one thread needs"
                .to_owned(),
        );
    }
    match herd_proto::parallelism_tokens(&record.command, record.requester_args()) {
        1 => None,
        0 => Some(format!(
            "this job asks for a -j derived from {mib} MiB per build thread, and its command \
             has no {} for the figure to go in, so the figure would be recorded and never \
             passed",
            herd_proto::PARALLELISM_TOKEN
        )),
        many => Some(format!(
            "this job's command carries {} {} times; one figure goes in one place, so which \
             one it is must not be a guess",
            herd_proto::PARALLELISM_TOKEN,
            many
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Attempt, JobMetrics, NewJob};
    use herd_proto::{
        Availability, BusyProbe, Capability, CapabilityOffer, CapabilityOrigins, CapabilityState,
        JobOutcome, OfferedCapability, Priority,
    };

    fn labels(names: &[&str]) -> LabelSet {
        LabelSet::new(names.iter().map(|n| (*n).to_owned())).expect("valid labels")
    }

    /// A queued record as the store would have written it. No disk, no clock:
    /// the claim step is arithmetic over a record and a fleet.
    fn queued(id: &str, want: &[&str]) -> JobRecord {
        let new = NewJob::at(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            labels(want),
            Priority::Step,
        );
        JobRecord {
            id: JobId::new(id.to_owned()),
            command: new.command,
            labels: new.labels,
            priority: new.priority,
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
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
            outputs: Default::default(),
        }
    }

    /// The same, asking for a derived `-j` through a command that carries the
    /// token (E-22).
    fn a_build(id: &str, mib: u32) -> JobRecord {
        let mut record = queued(id, &["build"]);
        record.command = vec![
            "cargo".to_owned(),
            "xtask".to_owned(),
            "pack".to_owned(),
            "-j".to_owned(),
            "{j}".to_owned(),
        ];
        record.build_thread_mib = Some(mib);
        record
    }

    fn offering(entries: Vec<OfferedCapability>) -> CapabilityOffer {
        CapabilityOffer::new(entries).expect("a valid offer")
    }

    fn declared(capability: Capability) -> OfferedCapability {
        OfferedCapability::new(capability, CapabilityOrigins::declared())
    }

    /// The first consumer's own fleet, as issue #16 describes it: their
    /// server with two build slots, two `gpu-venus` and an `igpu-vfio` that
    /// is exclusive host-wide, and the owner's PC with one build slot, no
    /// kvm, and a preference that makes builds belong there.
    fn their_server() -> Seat {
        seat(
            "build-host",
            0,
            // 31 GiB lent, 6 GiB kept as their floor, 8 GiB of tmpfs: the
            // three figures E-29 and E-31 name for that machine.
            HostSize::new(Some(31 * 1024), Some(12))
                .with_floor(Some(6 * 1024))
                .with_tmpfs(Some(8 * 1024)),
            offering(vec![
                declared(Capability::counted("build", 2)),
                declared(Capability::shared("kvm")),
                declared(
                    Capability::counted("gpu-venus", 2)
                        .excluding(["igpu-vfio"])
                        .expect("valid labels"),
                ),
                declared(Capability::exclusive("igpu-vfio")),
                declared(Capability::shared("reference")),
            ]),
        )
    }

    fn her_pc() -> Seat {
        seat(
            "wsl-pc",
            10,
            // 17 GiB lent, 3 GiB kept, and 2 GiB of tmpfs — less than the
            // server, which is what makes the two machines differ in M8.
            HostSize::new(Some(17 * 1024), Some(20))
                .with_floor(Some(3 * 1024))
                .with_tmpfs(Some(2 * 1024)),
            offering(vec![
                declared(Capability::counted("build", 1)),
                declared(Capability::shared("rig-cpu")),
            ]),
        )
    }

    fn seat(name: &str, preference: i32, host: HostSize, offer: CapabilityOffer) -> Seat {
        Seat {
            name: name.to_owned(),
            preference,
            host,
            it_said_it_is_holding: None,
            // Silent, on every seat these tests build but the ones that are
            // about the ceiling. That is deliberate and it is itself the
            // assertion D181's "absent is not a figure" needs: a runner that
            // said nothing is placed on exactly as it was before this field
            // existed, and every test in this module that passed without it
            // has to go on passing unchanged.
            jobs_at_once: None,
            draining: false,
            switch: herd_proto::Switch::On,
            reserved_for: None,
            capacity: RunnerCapacity::new(Some(name.to_owned()), offer.clone()),
            holdings: Holdings::new(offer),
            reserved: Reserved::default(),
            // A plausible figure, deliberately present on every seat these
            // tests build: E-32 says load is recorded and never a gate, and a
            // fleet whose load were always absent could not prove it.
            load_milli: Some(420),
        }
    }

    /// The claim, as a runner name and the one line explaining it. Owned,
    /// because a test asserts on the sentence of a placement it does not keep.
    fn placed(placement: Placement) -> (String, String) {
        match placement {
            Placement::Claim { runner, why, .. } => (runner, why),
            other => panic!("expected a claim, got {other:?}"),
        }
    }

    fn waited(placement: Placement) -> String {
        match placement {
            Placement::Wait { why } => why,
            other => panic!("expected a wait, got {other:?}"),
        }
    }

    fn impossible(placement: Placement) -> String {
        match placement {
            Placement::Impossible { why } => why,
            other => panic!("expected an impossibility, got {other:?}"),
        }
    }

    fn now() -> Timestamp {
        Timestamp::now()
    }

    // ---------------------------------------------------------------- E-25

    /// Two counted jobs serialise and the third waits, on the figures of the
    /// consumer's own server: `build` is two slots there.
    #[test]
    fn counted_slots_serialise_and_the_sentence_carries_the_figures() {
        let mut fleet = vec![their_server()];
        let first = placed(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(first.0, "build-host");
        assert!(first.1.contains("build slot 1 of 2"), "{}", first.1);
        let second = placed(place(
            &queued("j2", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(second.1.contains("build slot 2 of 2"), "{}", second.1);
        let third = waited(place(
            &queued("j3", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            third.contains("build has 2 slot(s) and 2 are taken"),
            "the wait must carry the figures (E-04): {third}"
        );
    }

    /// **Attack 1.** `exclusive` is host-wide (D46): it must not overlap
    /// anything at all on that machine, and nothing may start on top of it —
    /// and the two must not both be admitted in one pass, which is what
    /// carrying the seats through the pass is for.
    #[test]
    fn an_exclusive_job_never_overlaps_anything_in_one_pass() {
        // Counted first, exclusive second.
        let mut fleet = vec![their_server()];
        let counted = place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        assert!(matches!(counted, Placement::Claim { .. }));
        let exclusive = waited(place(
            &queued("j2", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            exclusive.contains("host-wide") && exclusive.contains("running 1 other job"),
            "an exclusive job must not join a counted one: {exclusive}"
        );

        // Exclusive first, counted second — and a job that asks for nothing
        // at all is just as refused, because host-wide means the host.
        let mut fleet = vec![their_server()];
        let exclusive = place(
            &queued("j1", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        assert!(matches!(exclusive, Placement::Claim { .. }));
        let counted = waited(place(
            &queued("j2", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(counted.contains("holds igpu-vfio"), "{counted}");
        let needs_nothing = waited(place(
            &queued("j3", &[]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            needs_nothing.contains("holds igpu-vfio"),
            "a job needing no capability still may not run beside a host-wide \
             one (D46): {needs_nothing}"
        );
    }

    /// And a probe that has said nothing yet holds an exclusive job off just
    /// as a busy one would: a reading nobody has made is not "free" (D48).
    #[test]
    fn an_exclusive_job_waits_for_a_probe_in_flight() {
        let offer = offering(vec![declared(
            Capability::exclusive("igpu-vfio").probed_by(BusyProbe::lock_file("/run/igd")),
        )]);
        let mut fleet = vec![seat("gpu-host", 0, HostSize::default(), offer)];
        let why = waited(place(
            &queued("j1", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("has not reported a reading"), "{why}");
        // The reading arrives and says free: it runs.
        fleet[0]
            .capacity
            .update(vec![CapabilityState::new("igpu-vfio", Availability::Free)]);
        assert!(matches!(
            place(
                &queued("j1", &["igpu-vfio"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ),
            Placement::Claim { .. }
        ));
    }

    /// **Attack 2.** Their real case, both ways round, and the third
    /// capability that must stay free: exclusion closes **symmetrically** and
    /// is **never transitive** (D47).
    #[test]
    fn exclusion_closes_symmetrically_and_never_transitively() {
        // The pair as they declare it, both ways round. Worth reading
        // closely, because it is **not** the declared exclusion that does the
        // work here: `igpu-vfio` is `exclusive`, so it is host-wide, and
        // host-wide already blocks every direction — a venus job keeps it off
        // by existing, and it keeps a venus job off for the same reason.
        // Their `gpu-venus excludes igpu-vfio` is therefore inert on this
        // offer, and that is worth them knowing: what really keeps the two
        // apart when something *outside* Herd holds the iGPU is the busy
        // probe on it (E-26), not the exclusion list.
        let mut fleet = vec![their_server()];
        assert!(matches!(
            place(
                &queued("j1", &["gpu-venus"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ),
            Placement::Claim { .. }
        ));
        let other_way = waited(place(
            &queued("j2", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            other_way.contains("igpu-vfio is host-wide")
                && other_way.contains("running 1 other job"),
            "holding gpu-venus must keep igpu-vfio off: {other_way}"
        );
        let mut fleet = vec![their_server()];
        assert!(matches!(
            place(
                &queued("j1", &["igpu-vfio"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ),
            Placement::Claim { .. }
        ));
        let back = waited(place(
            &queued("j2", &["gpu-venus"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(back.contains("holds igpu-vfio"), "{back}");

        // Symmetry on a pair where neither side is host-wide, so it is the
        // closure being tested and not exclusivity: a excludes b, declared
        // once, holds in both directions.
        let pair = offering(vec![
            declared(
                Capability::counted("a", 2)
                    .excluding(["b"])
                    .expect("valid labels"),
            ),
            declared(Capability::counted("b", 2)),
            declared(Capability::counted("c", 2)),
        ]);
        for (first, second) in [("a", "b"), ("b", "a")] {
            let mut fleet = vec![seat("pair-host", 0, HostSize::default(), pair.clone())];
            assert!(matches!(
                place(
                    &queued("j1", &[first]),
                    &mut fleet,
                    now(),
                    Stickiness::default(),
                    &Placing::default(),
                ),
                Placement::Claim { .. }
            ));
            let blocked = waited(place(
                &queued("j2", &[second]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
            assert!(
                blocked.contains(second) && blocked.contains(first),
                "{first} and {second} exclude each other in both directions: {blocked}"
            );
        }

        // And not transitively. b excludes c as well, so a-b and b-c are
        // declared and a-c is not: holding a leaves c free.
        let chain = offering(vec![
            declared(
                Capability::counted("a", 2)
                    .excluding(["b"])
                    .expect("valid labels"),
            ),
            declared(
                Capability::counted("b", 2)
                    .excluding(["c"])
                    .expect("valid labels"),
            ),
            declared(Capability::counted("c", 2)),
        ]);
        let mut fleet = vec![seat("chain-host", 0, HostSize::default(), chain)];
        assert!(matches!(
            place(
                &queued("j1", &["a"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ),
            Placement::Claim { .. }
        ));
        assert!(
            matches!(
                place(
                    &queued("j2", &["c"]),
                    &mut fleet,
                    now(),
                    Stickiness::default(),
                    &Placing::default(),
                ),
                Placement::Claim { .. }
            ),
            "a excludes b and b excludes c; nothing was said about a and c (D47)"
        );
    }

    // ---------------------------------------------------------------- E-22

    /// **Attack 3.** The figure E-22 asks to see, recorded, in the command,
    /// and never zero.
    #[test]
    fn the_derived_figure_is_recorded_and_is_the_one_the_job_is_given() {
        let mut fleet = vec![her_pc()];
        let placement = place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        let (runner, why) = placed(placement.clone());
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("-j 7"), "E-22's own figure: {why}");
        assert!(why.contains("1792 MiB per build thread"), "{why}");
        assert!(why.contains("17408 MiB and 20 threads"), "{why}");
        match &placement {
            Placement::Claim {
                command,
                parallelism,
                ..
            } => {
                assert_eq!(
                    command.last().map(String::as_str),
                    Some("7"),
                    "the figure recorded must be the figure passed: {command:?}"
                );
                assert_eq!(parallelism.expect("derived").threads, 7);
            }
            other => panic!("{other:?}"),
        }
        // Their server runs two builds at once, so each slot gets half the
        // share: 31 GiB, 12 threads, two slots.
        let mut fleet = vec![their_server()];
        let (_, why) = placed(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("-j 6"), "two slots halve the share: {why}");
        assert!(why.contains("shared between 2 slots"), "{why}");
    }

    /// **The hub's own instance of D107's defect, closed (D139, D140).**
    ///
    /// `{j}` is filled here, *after* `herdctl` quoted the requester's
    /// arguments into the line (D107) — so a pass over the whole command
    /// turned an argument that said `{j}` into a thread count, and the count
    /// read two tokens where the declaration wrote one, which ends the job by
    /// name. The declared `{j}` is filled and the argument is not: D107's
    /// property is that a value a substitution pasted in is never itself
    /// scanned for tokens.
    #[test]
    fn a_parallelism_token_a_requester_typed_in_an_argument_stays_text() {
        let args = vec!["--only".to_owned(), "{j}".to_owned()];
        let mut record = a_build("j1", 1792);
        // The line as `herdctl` resolved it, before the job was minted: one
        // declared `{j}`, and the arguments quoted in beside it.
        record.command = vec![format!(
            "cargo xtask pack -j {} {}",
            herd_proto::PARALLELISM_TOKEN,
            herd_proto::args_as_line(&args)
        )];
        record.declaration = Some(herd_proto::Declaration {
            command: "fixture".to_owned(),
            commit: "a".repeat(40),
            args,
            executor: herd_proto::Executor::default(),
        });
        let mut fleet = vec![her_pc()];
        let placement = place(
            &record,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        match &placement {
            Placement::Claim { command, .. } => assert_eq!(
                command,
                &vec!["cargo xtask pack -j 7 '--only' '{j}'".to_owned()],
                "the declared token is a figure and the requester's is three characters"
            ),
            other => panic!("one declared token is one place for the figure: {other:?}"),
        }
    }

    /// **Attack 8 and 10.** A machine that does not say how big it is takes
    /// no job whose `-j` has to be derived, and the sentence names the figure
    /// that is missing. Nothing is read as zero, and the job is not failed —
    /// another machine may say.
    #[test]
    fn a_runner_that_does_not_say_how_big_it_is_takes_no_derived_job() {
        let silent = seat(
            "silent-host",
            0,
            HostSize::default(),
            offering(vec![declared(Capability::counted("build", 1))]),
        );
        let mut fleet = vec![silent];
        let why = waited(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("how much memory or how many threads"),
            "the missing figure is named: {why}"
        );
        assert!(!why.contains("-j 0"), "{why}");
        // Half a figure is just as missing.
        let mut fleet = vec![seat(
            "half-host",
            0,
            HostSize::new(Some(17408), None),
            offering(vec![declared(Capability::counted("build", 1))]),
        )];
        let why = waited(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("how many threads"), "{why}");
    }

    /// **Attack 10.** A requirement the record carries only half of is
    /// refused by name, and refused *for good*: waiting cannot fix a
    /// submission. A figure of zero, a command with nowhere to put the
    /// figure, and a command with two places for it.
    #[test]
    fn a_job_whose_requirements_cannot_be_met_by_any_machine_fails_fast() {
        let mut fleet = vec![her_pc()];
        let mut zero = a_build("j1", 0);
        zero.build_thread_mib = Some(0);
        let why = impossible(place(
            &zero,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("0 MiB"), "{why}");

        let mut no_token = a_build("j2", 1792);
        no_token.command = vec!["cargo".to_owned(), "xtask".to_owned(), "pack".to_owned()];
        let why = impossible(place(
            &no_token,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("{j}") && why.contains("1792"), "{why}");

        let mut twice = a_build("j3", 1792);
        twice.command = vec!["make".to_owned(), "-j{j}".to_owned(), "J={j}".to_owned()];
        let why = impossible(place(
            &twice,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("2 times"), "{why}");

        // And a job that asks for nothing derived is untouched by all of it.
        assert!(matches!(
            place(
                &queued("j4", &["build"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ),
            Placement::Claim { .. }
        ));
    }

    // ---------------------------------------------------------------- E-19

    /// **Attack 5.** A job waits for the machine it is sticking to while that
    /// machine is *available*, even when a lesser machine is free — and the
    /// sentence says so, with the visible limit in it.
    #[test]
    fn a_job_waits_for_its_preferred_runner_rather_than_falling_back() {
        let mut fleet = vec![their_server(), her_pc()];
        // The first build goes to the PC: it is the preferred machine.
        let (runner, why) = placed(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("preference 10"), "{why}");
        assert!(why.contains("of 2 eligible runners"), "{why}");
        // The second one waits for it, although the server has two free
        // slots. That is E-19, and it is the whole point.
        let why = waited(place(
            &queued("j2", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("waiting for wsl-pc"), "{why}");
        assert!(
            why.contains("build-host could take it and is less preferred"),
            "the sentence has to name the machine it is not going to: {why}"
        );
        assert!(
            why.contains("3h") && why.contains("--stickiness-limit"),
            "E-19's limit must be visible in the sentence: {why}"
        );
    }

    /// And it moves the moment its preferred runner stops being available.
    /// Two ways for that to happen, and the decision D80 is the second one:
    /// the runner is gone, or the runner is draining.
    #[test]
    fn it_moves_when_the_preferred_runner_is_no_longer_available() {
        // Gone: the fleet is just the server.
        let mut fleet = vec![their_server()];
        let (runner, why) = placed(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "build-host");
        assert!(why.contains("the only eligible runner"), "{why}");

        // Draining: the PC is connected, and a job does not wait for a
        // machine that is leaving (invariant 2, D80).
        let mut pc = her_pc();
        pc.draining = true;
        let mut fleet = vec![their_server(), pc];
        let (runner, _) = placed(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(
            runner, "build-host",
            "a draining runner is not available, so nothing sticks to it"
        );
    }

    /// The safety limit is a limit: past it the job moves, and the line
    /// explaining the placement says it moved and why (E-19, E-36).
    #[test]
    fn past_the_visible_limit_the_job_moves_and_the_line_says_so() {
        let mut fleet = vec![their_server(), her_pc()];
        let busy = place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        assert!(matches!(busy, Placement::Claim { .. }));

        let stickiness = Stickiness {
            limit: Duration::from_secs(3 * 60 * 60),
        };
        let mut second = queued("j2", &["build"]);
        // Submitted three hours and a minute ago. Simulated, through the same
        // code the hub runs: `now` is an argument.
        let moment = second
            .submitted_at
            .after(Duration::from_secs(3 * 60 * 60 + 60));
        let waiting = place(
            &second,
            &mut fleet,
            second.submitted_at,
            stickiness,
            &Placing::default(),
        );
        assert!(matches!(waiting, Placement::Wait { .. }), "{waiting:?}");
        second.waiting_for = None;
        let (runner, why) = placed(place(
            &second,
            &mut fleet,
            moment,
            stickiness,
            &Placing::default(),
        ));
        assert_eq!(runner, "build-host");
        assert!(why.contains("past the 3h stickiness limit"), "{why}");
        assert!(why.contains("after waiting 3h01m for wsl-pc"), "{why}");
    }

    // ---------------------------------------------------------------- E-36

    /// **Attack 6, both directions.** "Nobody can ever serve this" ends a
    /// job; "nobody can serve this now" queues it. The two must never be
    /// confused, so both are asserted here, including the case that looks
    /// most like the other one: the only machine that offers the label is not
    /// connected.
    #[test]
    fn nobody_ever_is_terminal_and_nobody_now_is_a_wait() {
        // Impossible: two labels that exclude each other, on a machine that
        // offers both and says so.
        let mut fleet = vec![their_server()];
        let why = impossible(place(
            &queued("j1", &["gpu-venus", "igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("gpu-venus") && why.contains("igpu-vfio"),
            "the reason must name both labels: {why}"
        );
        assert!(why.contains("no runner can give it"), "{why}");

        // A label nothing here offers: a wait, because a machine that offers
        // it may dial in. This is the case that must never be read as the one
        // above.
        let why = waited(place(
            &queued("j2", &["metal-dell"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("does not offer metal-dell"), "{why}");

        // The same label, offered by a machine that is simply not connected
        // right now — which from the hub's seat is indistinguishable from the
        // line above, and must be a wait for the same reason.
        let mut with_pc = vec![their_server(), her_pc()];
        let why = waited(place(
            &queued("j3", &["metal-dell"]),
            &mut with_pc,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("does not offer metal-dell"), "{why}");

        // A label whose owner denied it: still a wait, and the sentence says
        // denied rather than absent (E-24).
        let denying = seat(
            "laptop",
            0,
            HostSize::default(),
            offering(vec![OfferedCapability::new(
                Capability::shared("kvm"),
                CapabilityOrigins {
                    probed: true,
                    declared: false,
                    denied: true,
                },
            )]),
        );
        let mut fleet = vec![denying];
        let why = waited(place(
            &queued("j4", &["kvm"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("denied"), "{why}");

        // And an empty fleet is a wait, never an end.
        let why = waited(place(
            &queued("j5", &["build"]),
            &mut [],
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("no runner is connected"), "{why}");
    }

    /// **Attack 7.** One line per placement, and one line means one line:
    /// no newline in it, the word `claimed` in it once, and every figure an
    /// operator would ask for.
    #[test]
    fn the_explanation_is_one_line_with_the_figures_in_it() {
        let mut fleet = vec![her_pc()];
        let (_, why) = placed(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(!why.contains('\n'), "one line: {why}");
        assert_eq!(why.matches("claimed by").count(), 1, "{why}");
        for figure in ["wsl-pc", "build", "slot 1 of 1", "-j 7", "preference 10"] {
            assert!(why.contains(figure), "{figure} is missing from: {why}");
        }
    }

    /// **Attack 9, the arithmetic half.** The claim re-checks under the lock
    /// rather than trusting the judgement that led to it. The lock itself is
    /// the hub's and is tested against the real thing in `server.rs`; what is
    /// tested here is that the second check exists and that failing it leaves
    /// the job queued rather than placing it anyway.
    #[test]
    fn the_claim_rechecks_and_a_stale_judgement_places_nothing() {
        let mut seat = her_pc();
        // The judgement said free; by the time the claim is written the one
        // slot has gone to somebody else.
        seat.holdings.already_holding(
            &JobId::new("j-elsewhere"),
            labels(&["build"]),
            Occupant::Job,
        );
        let why = waited(claim(
            &queued("j1", &["build"]),
            &mut seat,
            Fitted {
                derived: None,
                memory_mib: 0,
                tmpfs_mib: 0,
            },
            1,
            None,
            &Placing::default(),
        ));
        assert!(
            why.contains("could not be given its labels"),
            "a claim whose second check fails must place nothing: {why}"
        );
    }

    // ------------------------------------------------- holdings from records

    /// **Attack 4, the bookkeeping half.** What a runner holds is read from
    /// the records, so every way a job can end releases its slot by the same
    /// write that ends it — and a hub that restarts reads the same thing.
    #[test]
    fn a_slot_is_held_by_an_open_attempt_and_by_nothing_else() {
        let mut running = queued("j1", &["build"]);
        running.state = JobState::Running;
        running.attempts = vec![Attempt {
            number: 1,
            runner: "wsl-pc".to_owned(),
            started_at: Timestamp::now(),
            ended_at: None,
            ended: None,
            requeue_code: None,
            at_its_ceiling: None,
            why: "claimed by wsl-pc: labels build".to_owned(),
            steps: Vec::new(),
            commit: None,
        }];
        let held = holdings(std::slice::from_ref(&running));
        assert_eq!(held["wsl-pc"].len(), 1);

        // Every end releases it, whichever end it is — and there is no path
        // to forget, because the release *is* the record's state.
        for state in [
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
            JobState::Finished {
                outcome: JobOutcome::Signalled { signal: 9 },
            },
            JobState::Finished {
                outcome: JobOutcome::Cancelled {
                    reason: "a requester asked".to_owned(),
                },
            },
            JobState::Finished {
                outcome: JobOutcome::Lost {
                    reason: "silent".to_owned(),
                },
            },
            JobState::Finished {
                outcome: JobOutcome::StoppedByCap {
                    cap: herd_proto::Cap::Time,
                    limit: 600,
                    detail: None,
                },
            },
            JobState::Finished {
                outcome: JobOutcome::Impossible {
                    reason: "two labels".to_owned(),
                },
            },
            JobState::Queued,
        ] {
            let mut ended = running.clone();
            ended.state = state.clone();
            assert!(
                holdings(&[ended]).is_empty(),
                "a job that is {} holds no slot",
                state.name()
            );
        }

        // An attempt that ended while the job carries on — the requeue of
        // E-21 — holds nothing either, even before the next one starts.
        let mut requeued = running.clone();
        requeued.attempts[0].ended_at = Some(Timestamp::now());
        requeued.attempts[0].ended = Some("the runner went silent".to_owned());
        assert!(holdings(&[requeued]).is_empty());

        // And a job the hub was interrupted in the middle of still holds:
        // the machine may well still be running it.
        let mut interrupted = running.clone();
        interrupted.state = JobState::Interrupted {
            reason: "the hub restarted".to_owned(),
        };
        assert_eq!(holdings(&[interrupted])["wsl-pc"].len(), 1);
    }

    /// A job holding a label its runner no longer offers still holds the
    /// host: written down, never re-checked. Otherwise the host would read as
    /// idle and something host-wide would be handed out on top of it.
    #[test]
    fn a_holding_the_offer_no_longer_explains_still_empties_the_host() {
        let mut seat = their_server();
        seat.holdings.already_holding(
            &JobId::new("j-old"),
            labels(&["a-label-nobody-offers"]),
            Occupant::Job,
        );
        let mut fleet = vec![seat];
        let why = waited(place(
            &queued("j1", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("running 1 other job"), "{why}");
    }

    // ----------------------------------------------- E-29, E-31, E-32 (M8)

    /// A job declaring a whole-job memory need and a tmpfs reservation, as
    /// the first consumer's `test` command does once `--labels` has answered.
    fn a_test(id: &str, memory_mib: u64, tmpfs_mib: u64) -> JobRecord {
        let mut record = queued(id, &["build"]);
        record.memory_mib = Some(memory_mib);
        record.tmpfs_mib = Some(tmpfs_mib);
        record
    }

    /// The same job, as its record reads once it is out on a runner: the
    /// state and the open attempt that make it *hold* what it declared.
    fn out_on(mut record: JobRecord, runner: &str) -> JobRecord {
        record.state = JobState::Running;
        record.attempts = vec![Attempt {
            number: 1,
            runner: runner.to_owned(),
            started_at: Timestamp::now(),
            ended_at: None,
            ended: None,
            requeue_code: None,
            at_its_ceiling: None,
            why: format!("claimed by {runner}"),
            steps: Vec::new(),
            commit: None,
        }];
        record
    }

    /// A fleet of one seat, with what the records say is already out on it.
    fn fleet_with(mut seat: Seat, running: &[JobRecord]) -> Vec<Seat> {
        let held = holdings(running);
        for holding in held.get(&seat.name).into_iter().flatten() {
            seat.holdings
                .already_holding(&holding.job, holding.labels.clone(), holding.occupant);
        }
        seat.reserved = reserved(running).remove(&seat.name).unwrap_or_default();
        vec![seat]
    }

    /// **Attack 1.** Two jobs that each fit alone are not both admitted in
    /// one pass. The seats are carried through the pass, so the second job is
    /// weighed against the first — which is the only thing standing between
    /// invariant 2 and a host that swaps.
    #[test]
    fn two_jobs_that_each_fit_alone_do_not_both_fit_in_one_pass() {
        // 31 GiB lent less a 6 GiB floor is 25 GiB admitted; two 14 GiB jobs
        // are 28. Both ask for `build`, of which that machine has two slots,
        // so nothing but the memory can be what stops the second.
        let mut fleet = vec![their_server()];
        let first = a_test("j1", 14 * 1024, 0);
        let second = a_test("j2", 14 * 1024, 0);
        let (runner, why) = placed(place(
            &first,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "build-host");
        assert!(
            why.contains("admitted 14 GiB of memory, 11 GiB free of 25 GiB after it"),
            "{why}"
        );
        let why = waited(place(
            &second,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("11 GiB free of its 25 GiB memory admission, 14 GiB needed"),
            "the second job of one pass must be weighed against the first: {why}"
        );
        assert!(
            why.contains("1 job running there holds 14 GiB of memory"),
            "{why}"
        );

        // And the same for tmpfs: 8 GiB there, two 5 GiB reservations.
        let mut fleet = vec![their_server()];
        let first = a_test("j1", 1024, 5 * 1024);
        let second = a_test("j2", 1024, 5 * 1024);
        let (_, why) = placed(place(
            &first,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("reserved 5 GiB in tmpfs, 3 GiB free of 8 GiB"),
            "{why}"
        );
        let why = waited(place(
            &second,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("3 GiB free in tmpfs, 5 GiB needed (8 GiB of tmpfs there in all)"),
            "{why}"
        );
    }

    /// **Attack 2.** A job admitted a second ago has allocated nothing yet,
    /// and is costed at every byte it **declared** all the same. A hub that
    /// looked at real usage would let a second job in beside it and watch
    /// both grow.
    #[test]
    fn a_job_admitted_and_still_idle_is_costed_at_what_it_declared() {
        // `assigned` is the state of a job handed over and not yet started:
        // nothing of it is running, nothing of it has allocated a page.
        let mut idle = out_on(a_test("j1", 20 * 1024, 6 * 1024), "build-host");
        idle.state = JobState::Assigned;
        let taken = reserved(std::slice::from_ref(&idle));
        assert_eq!(taken["build-host"].memory_mib, 20 * 1024);
        assert_eq!(taken["build-host"].tmpfs_mib, 6 * 1024);

        let mut fleet = fleet_with(their_server(), std::slice::from_ref(&idle));
        let why = waited(place(
            &a_test("j2", 6 * 1024, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("5 GiB free of its 25 GiB memory admission, 6 GiB needed"),
            "a job that has allocated nothing still holds what it declared: {why}"
        );
    }

    /// E-04's own example sentence, from end to end: a job that is short of
    /// tmpfs is told how much is free and how much it needs, in those words.
    #[test]
    fn a_refusal_carries_the_figures_e04_asks_for() {
        // 8 GiB of tmpfs there, 6 960 MiB of it declared by a job that is
        // running: 1 229 MiB free, which is E-04's `1.2 GiB`.
        let running = out_on(a_test("j1", 1024, 6960), "build-host");
        let mut fleet = fleet_with(their_server(), std::slice::from_ref(&running));
        let why = waited(place(
            &a_test("j2", 1024, 2048),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("1.2 GiB free in tmpfs, 2 GiB needed"),
            "E-04's own example, in E-04's own words: {why}"
        );
    }

    /// **Attack 4.** "Short of tmpfs now" and "more than this host can ever
    /// give" are different answers, and the difference is whether the job
    /// **sticks** to the machine. A job told it is waiting for a machine that
    /// could never hold it waits for ever for something that cannot happen —
    /// which is M7's defect in a new place.
    #[test]
    fn short_now_waits_for_the_machine_and_too_big_ever_does_not() {
        // Short now: 8 GiB there, 6 GiB taken, 4 GiB wanted. It waits *for
        // build-host*, because a job ending there is what frees the room.
        let running = out_on(a_test("j1", 1024, 6 * 1024), "build-host");
        let mut fleet = fleet_with(their_server(), std::slice::from_ref(&running));
        let why = waited(place(
            &a_test("j2", 1024, 4 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("it is waiting for build-host"), "{why}");
        assert!(why.contains("2 GiB free in tmpfs, 4 GiB needed"), "{why}");

        // More than it has in all: the job does not wait for that machine,
        // and the sentence says *ever* rather than *not now*.
        let mut fleet = vec![their_server()];
        let why = waited(place(
            &a_test("j3", 1024, 16 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("build-host has 8 GiB of tmpfs in all and this job needs 16 GiB"),
            "{why}"
        );
        assert!(why.contains("can ever cover it"), "{why}");
        assert!(
            !why.contains("waiting for build-host"),
            "it must not stick to a machine that could never hold it: {why}"
        );

        // And the same job is kept alive — never failed — when the only
        // machine that could ever fit it is not connected. A fleet is not a
        // closed world: the owner's PC is asleep at night (E-31, D88).
        let fits_it = seat(
            "big-host",
            0,
            HostSize::new(Some(64 * 1024), Some(32))
                .with_floor(Some(8 * 1024))
                .with_tmpfs(Some(32 * 1024)),
            offering(vec![declared(Capability::counted("build", 1))]),
        );
        let mut whole_fleet = vec![their_server(), fits_it];
        let (runner, _) = placed(place(
            &a_test("j3", 1024, 16 * 1024),
            &mut whole_fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "big-host");
        // With that machine gone, the job waits rather than ending.
        let mut fleet = vec![their_server()];
        let placement = place(
            &a_test("j3", 1024, 16 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        );
        assert!(
            matches!(placement, Placement::Wait { .. }),
            "a job no connected machine can hold waits; it is not failed: {placement:?}"
        );
    }

    /// **Attack 7.** A figure that is unknown is said to be unknown. A runner
    /// that announces no floor, or no tmpfs, takes no job that needs the
    /// figure — by name, never on the assumption that the room is there and
    /// never as `Some(0)`.
    #[test]
    fn an_unknown_admission_figure_refuses_by_name_and_is_never_read_as_zero() {
        // No floor announced: the hub does not know what this host keeps.
        let no_floor = seat(
            "silent-floor",
            0,
            HostSize::new(Some(31 * 1024), Some(12)).with_tmpfs(Some(8 * 1024)),
            offering(vec![declared(Capability::counted("build", 1))]),
        );
        let mut fleet = vec![no_floor];
        let why = waited(place(
            &a_test("j1", 2048, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("silent-floor does not say how much memory it keeps back"),
            "{why}"
        );

        // No tmpfs figure announced, and a job that needs a reservation.
        let no_tmpfs = seat(
            "silent-tmpfs",
            0,
            HostSize::new(Some(31 * 1024), Some(12)).with_floor(Some(6 * 1024)),
            offering(vec![declared(Capability::counted("build", 1))]),
        );
        let mut fleet = vec![no_tmpfs];
        let why = waited(place(
            &a_test("j1", 2048, 2048),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("silent-tmpfs does not say how much tmpfs it has"),
            "{why}"
        );
        assert!(why.contains("is not read as zero"), "{why}");
        // But a job that reserves nothing is placed there all the same: an
        // absent tmpfs figure is not a machine that can run nothing.
        let mut fleet = vec![seat(
            "silent-tmpfs",
            0,
            HostSize::new(Some(31 * 1024), Some(12)).with_floor(Some(6 * 1024)),
            offering(vec![declared(Capability::counted("build", 1))]),
        )];
        let (runner, why) = placed(place(
            &a_test("j2", 2048, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "silent-tmpfs");
        assert!(why.contains("it reserves no tmpfs"), "{why}");
    }

    /// A whole-job memory figure of zero is not a figure, and a job that says
    /// two different things about its memory is refused rather than admitted
    /// against whichever the code happens to read first.
    #[test]
    fn a_memory_declaration_that_is_not_a_figure_fails_fast() {
        let mut zero = queued("j1", &["build"]);
        zero.memory_mib = Some(0);
        let mut fleet = vec![their_server()];
        let why = impossible(place(
            &zero,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("0 MiB of memory, which is not a figure"),
            "{why}"
        );

        let mut both = a_build("j2", 1792);
        both.memory_mib = Some(8192);
        let why = impossible(place(
            &both,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("two answers to one question"), "{why}");
    }

    /// A build's whole-job need is its per-thread figure times the `-j` the
    /// machine gave it, which is what makes D82's order load-bearing: N
    /// first, the requirement second.
    #[test]
    fn a_build_is_admitted_against_its_per_thread_figure_times_the_derived_j() {
        // Their server: 31 GiB, 12 threads, two build slots, so `-j 6` and
        // 6 x 1 792 = 10 752 MiB of declared need.
        let mut fleet = vec![their_server()];
        let (_, why) = placed(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("-j 6"), "{why}");
        assert!(why.contains("admitted 10.5 GiB of memory"), "{why}");
        // Both of their build slots fit at once, which is the arithmetic the
        // first consumer's own fleet depends on: 2 x 10.5 GiB under 25 GiB.
        let (_, why) = placed(place(
            &a_build("j2", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("admitted 10.5 GiB of memory"), "{why}");
        assert!(why.contains("build slot 2 of 2"), "{why}");

        // And her PC derives `-j 7` on one slot: 7 x 1 792 = 12 544 MiB,
        // against 17 GiB less a 3 GiB floor. It fits, and barely.
        let mut fleet = vec![her_pc()];
        let (_, why) = placed(place(
            &a_build("j3", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("-j 7"), "{why}");
        assert!(why.contains("admitted 12.2 GiB of memory"), "{why}");
    }

    /// **Attack 3, 9 and 10.** A reservation is a field on a record whose
    /// attempt is open, so every way a job ends releases it by the same write
    /// that ends it — and a hub that restarts reads what the hub before it
    /// would have read. There is nothing to leak and nothing to rebuild.
    #[test]
    fn a_reservation_is_released_by_every_way_a_job_ends() {
        let running = out_on(a_test("j1", 8192, 4096), "build-host");
        assert_eq!(
            reserved(std::slice::from_ref(&running))["build-host"].tmpfs_mib,
            4096
        );

        for state in [
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
            JobState::Finished {
                outcome: JobOutcome::Signalled { signal: 9 },
            },
            JobState::Finished {
                outcome: JobOutcome::StoppedByCap {
                    cap: herd_proto::Cap::Memory,
                    limit: 8192,
                    detail: None,
                },
            },
            JobState::Finished {
                outcome: JobOutcome::Cancelled {
                    reason: "a requester asked".to_owned(),
                },
            },
            JobState::Finished {
                outcome: JobOutcome::Lost {
                    reason: "the runner went silent".to_owned(),
                },
            },
            JobState::Finished {
                outcome: JobOutcome::CouldNotStart {
                    reason: "the workspace could not be made".to_owned(),
                },
            },
            JobState::Finished {
                outcome: JobOutcome::Impossible {
                    reason: "two labels".to_owned(),
                },
            },
            JobState::Queued,
        ] {
            let mut ended = running.clone();
            ended.state = state.clone();
            assert!(
                reserved(&[ended]).is_empty(),
                "a job that is {} reserves nothing",
                state.name()
            );
        }

        // **Attack 9**: a reservation taken for a job that never started. The
        // runner died between the claim and the start, so the attempt is
        // closed and the job is queued again — and the room is back.
        let mut never_started = running.clone();
        never_started.state = JobState::Queued;
        never_started.attempts[0].ended_at = Some(Timestamp::now());
        never_started.attempts[0].ended = Some("the runner went before it started".to_owned());
        assert!(reserved(&[never_started]).is_empty());

        // **Attack 10**: a hub killed mid-job leaves the record
        // `interrupted`, and the machine may well still be running it — so it
        // still holds every byte it declared, read off the same record the
        // new hub reads.
        let mut interrupted = running.clone();
        interrupted.state = JobState::Interrupted {
            reason: "this hub was killed while the job was running".to_owned(),
        };
        let after_a_restart = reserved(std::slice::from_ref(&interrupted));
        assert_eq!(after_a_restart["build-host"].memory_mib, 8192);
        assert_eq!(after_a_restart["build-host"].tmpfs_mib, 4096);
        assert_eq!(after_a_restart["build-host"].jobs, 1);
    }

    /// A job out there whose declared memory cannot be costed — a build with
    /// no `-j` on its record — stops the machine admitting anything more,
    /// **by name**. Read as zero it would be a machine that looks idle on top
    /// of a running build.
    #[test]
    fn a_holding_that_cannot_be_costed_is_named_and_never_counted_as_zero() {
        let mut uncostable = out_on(a_build("j1", 1792), "build-host");
        uncostable.parallelism = None;
        let taken = reserved(std::slice::from_ref(&uncostable));
        assert_eq!(taken["build-host"].memory_mib, 0);
        assert_eq!(taken["build-host"].uncostable.len(), 1);

        let mut fleet = fleet_with(their_server(), std::slice::from_ref(&uncostable));
        let why = waited(place(
            &a_test("j2", 1024, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("build-host is holding j1"), "{why}");
        assert!(why.contains("cannot be costed"), "{why}");
    }

    /// **Attack 5's neighbour.** A floor at or past the memory lent admits
    /// nothing, says so with both figures, and above all does not wrap round.
    /// The runner refuses that pairing where it is written; this is what the
    /// hub does if one arrives anyway.
    #[test]
    fn a_floor_past_the_memory_admits_nothing_and_does_not_wrap_round() {
        let absurd = seat(
            "all-floor",
            0,
            HostSize::new(Some(4096), Some(8))
                .with_floor(Some(8192))
                .with_tmpfs(Some(1024)),
            offering(vec![declared(Capability::counted("build", 1))]),
        );
        let mut fleet = vec![absurd];
        let why = waited(place(
            &a_test("j1", 512, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("all-floor admits at most 0 MiB of memory (4 GiB lent, 8 GiB kept"),
            "{why}"
        );
    }

    /// **Attack 6.** E-32: the load figure is recorded and is never a gate.
    /// The same job goes to the same machine at a load of 0.1 and at a load
    /// of 50, and the figure is on the line either way.
    #[test]
    fn load_is_recorded_on_the_placement_and_gates_nothing() {
        let mut quiet = vec![their_server()];
        quiet[0].load_milli = Some(100);
        let (calm_runner, calm_why) = placed(place(
            &a_test("j1", 8192, 2048),
            &mut quiet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        let mut hammered = vec![their_server()];
        hammered[0].load_milli = Some(50_000);
        let (busy_runner, busy_why) = placed(place(
            &a_test("j1", 8192, 2048),
            &mut hammered,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(
            calm_runner, busy_runner,
            "a busy host is a fact to report, never a reason to refuse (E-32)"
        );
        assert!(
            calm_why.contains("load 0.10 there, recorded and not a gate"),
            "{calm_why}"
        );
        assert!(
            busy_why.contains("load 50.00 there, recorded and not a gate"),
            "{busy_why}"
        );
        // And the two lines differ in nothing but that figure.
        assert_eq!(
            calm_why.replace("load 0.10", "load X"),
            busy_why.replace("load 50.00", "load X")
        );

        // A machine that cannot read its load is placed on all the same, and
        // the absence is said rather than printed as zero.
        let mut silent = vec![their_server()];
        silent[0].load_milli = None;
        let (runner, why) = placed(place(
            &a_test("j1", 8192, 2048),
            &mut silent,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "build-host");
        assert!(why.contains("does not say what its load is"), "{why}");
    }

    /// **Attack 6, the other half.** Nothing in this module compares anything
    /// against a load.
    ///
    /// Read off this module's own source, which catches the shape the defect
    /// would actually take — `if seat.load_milli > something` — and not every
    /// shape it could conceivably take. A reviewer is still the backstop; what
    /// this stops is the obvious version being added without anyone noticing.
    #[test]
    fn no_line_of_this_module_weighs_a_load_against_anything() {
        for (number, line) in include_str!("matcher.rs").lines().enumerate() {
            // Code only: a comment that names the forbidden shape in order to
            // forbid it is not the forbidden shape.
            if line.trim_start().starts_with("//") || !line.contains("load_milli") {
                continue;
            }
            for comparison in [" < ", " > ", " <= ", " >= ", ".cmp(", ".max(", ".min("] {
                assert!(
                    !line.contains(comparison),
                    "line {} weighs a load against something, and E-32 forbids it: {line}",
                    number + 1
                );
            }
        }
    }
    // ---------------------------------------------------------------- M9

    fn cancelled(placement: Placement) -> String {
        match placement {
            Placement::Cancel { why } => why,
            other => panic!("expected a cancellation, got {other:?}"),
        }
    }

    /// A runner whose owner has switched it off is not available (E-33, D80).
    ///
    /// Not here, rather than later: the job must not stick to a machine that
    /// has been withdrawn, and the sentence has to carry her reason, because
    /// "the PC is busy" and "she is gaming" are different facts.
    #[test]
    fn a_switched_off_runner_takes_nothing_and_nothing_waits_for_it() {
        let mut off = her_pc();
        off.switch = herd_proto::Switch::Off {
            reason: "she is gaming".to_owned(),
        };
        let mut fleet = vec![their_server(), off];
        // The PC is preferred (10 against 0) and would normally take a build
        // under E-19. Switched off, the build goes to the server at once.
        let (runner, _) = placed(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(
            runner, "build-host",
            "a job must not stick to a machine its owner has withdrawn (D80)"
        );
        // And with nothing else to take it, the wait names the position and
        // the reason she typed.
        let mut only_the_pc = vec![fleet.pop().expect("the pc is last")];
        let why = waited(place(
            &queued("j2", &["build"]),
            &mut only_the_pc,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("wsl-pc is switched off (she is gaming)"),
            "the sentence must carry the owner's own words: {why}"
        );
        assert!(why.contains("E-33"), "{why}");
    }

    /// A queued job past its deadline is cancelled, and the sentence says
    /// what it was waiting for when the moment came (E-09, attack 11).
    #[test]
    fn a_queued_job_past_its_deadline_is_cancelled_and_says_what_it_waited_for() {
        let mut off = her_pc();
        off.switch = herd_proto::Switch::Off {
            reason: "she is gaming".to_owned(),
        };
        let mut fleet = vec![off];
        let mut record = queued("j1", &["build"]);
        // What the previous pass wrote on it: the machine of its labels was
        // switched off.
        record.waiting_for =
            Some("queued 1st of 1 … wsl-pc is switched off (she is gaming)".to_owned());
        record.deadline = Some(record.submitted_at.after(Duration::from_secs(60)));
        let moment = record.submitted_at.after(Duration::from_secs(5 * 60));
        let why = cancelled(place(
            &record,
            &mut fleet,
            moment,
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.starts_with("cancelled (deadline)"), "{why}");
        assert!(
            why.contains("4m ago"),
            "the sentence carries the figures: {why}"
        );
        assert!(
            why.contains("she is gaming"),
            "a job cancelled by a deadline it missed because its machine was off must say \
             so, not merely say 'deadline': {why}"
        );
    }

    /// The deadline fires with no fleet at all, which is the bound D88 leaves
    /// on a job no connected runner could ever fit (E-09).
    #[test]
    fn a_deadline_fires_on_an_empty_fleet_which_is_d88_s_only_bound() {
        let mut record = queued("j1", &["build"]);
        record.deadline = Some(record.submitted_at);
        let why = cancelled(place(
            &record,
            &mut [],
            record.submitted_at.after(Duration::from_secs(1)),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.starts_with("cancelled (deadline)"), "{why}");
    }

    /// A deadline already past at submission fires on the first pass, and one
    /// exactly at the moment of the pass does not: the comparison is strict,
    /// so a job and its deadline arriving together is a job that may start.
    #[test]
    fn a_deadline_in_the_past_fires_at_once_and_one_on_the_dot_does_not() {
        let mut record = queued("j1", &["build"]);
        let moment = record.submitted_at;
        record.deadline = Some(moment.after(Duration::from_secs(0)));
        let mut fleet = vec![their_server()];
        // On the dot: not past, so the job is placed.
        assert!(matches!(
            place(
                &record,
                &mut fleet,
                moment,
                Stickiness::default(),
                &Placing::default()
            ),
            Placement::Claim { .. }
        ));
        // A nanosecond later it is past, and the pass's single `now` is what
        // decides — so a job is either claimed in a pass or expired in it,
        // never both.
        let mut fleet = vec![their_server()];
        let why = cancelled(place(
            &record,
            &mut fleet,
            moment.after(Duration::from_nanos(1)),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.starts_with("cancelled (deadline)"), "{why}");
    }

    /// A job whose own figures contradict each other says *that*, even when
    /// its deadline has also passed — M7's lesson, in a new place: the fact
    /// about the submission is judged before the fact about the moment, or a
    /// malformed job would be reported as a late one and the bug would never
    /// be found.
    #[test]
    fn a_contradictory_submission_outranks_its_own_deadline() {
        let mut record = a_build("j1", 1792);
        record.memory_mib = Some(4096);
        record.deadline = Some(record.submitted_at);
        let mut fleet = vec![their_server()];
        let why = impossible(place(
            &record,
            &mut fleet,
            record.submitted_at.after(Duration::from_secs(60)),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("two answers to one question"), "{why}");
    }

    /// E-10's one edge, every way a parent can end.
    ///
    /// Success is exit 0 and nothing else. Every other ending cancels the
    /// child **by name**, so that an operator reading the child's record
    /// learns which job let it down and how.
    #[test]
    fn a_parent_that_did_not_succeed_cancels_its_child_by_name() {
        let mut fleet = vec![their_server()];
        let child = queued("j2", &["build"]);
        // Succeeded: the child is placed like anything else.
        assert!(matches!(
            place(
                &child,
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing {
                    parent: Parent::Succeeded {
                        id: JobId::new("j1")
                    },
                    ..Placing::default()
                }
            ),
            Placement::Claim { .. }
        ));
        // Still going: it waits, naming the parent and where it is.
        let why = waited(place(
            &child,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing {
                parent: Parent::NotYet {
                    id: JobId::new("j1"),
                    state: "running".to_owned(),
                },
                ..Placing::default()
            },
        ));
        assert!(
            why.contains("it starts after j1, which is running"),
            "{why}"
        );
        // Every other ending: cancelled, naming the parent and how it went.
        for how in [
            "exited 1 rather than 0",
            "was killed by signal 9",
            "was cancelled: its requester asked",
            "was lost: runner wsl-pc stopped answering",
            "was stopped by its Time cap of 7200",
            "could never have run: nothing offers metal-dell",
        ] {
            let why = cancelled(place(
                &child,
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing {
                    parent: Parent::DidNot {
                        id: JobId::new("j1"),
                        how: how.to_owned(),
                    },
                    ..Placing::default()
                },
            ));
            assert!(why.starts_with("cancelled (parent)"), "{why}");
            assert!(why.contains("j1"), "{why}");
            assert!(
                why.contains(how),
                "the child says how its parent went: {why}"
            );
        }
        // And a parent nothing can be read about is never read as one that
        // succeeded.
        let why = cancelled(place(
            &child,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing {
                parent: Parent::Gone {
                    id: JobId::new("j1"),
                },
                ..Placing::default()
            },
        ));
        assert!(why.contains("no record of j1 at all"), "{why}");
    }

    /// A drain-first capability empties its host before the job starts, and
    /// the host claims nothing new while it waits (E-27).
    #[test]
    fn a_drain_first_job_waits_for_a_quiet_host_and_reserves_it() {
        let trial = queued("j9", &["metal-pc"]);
        let build = queued("j1", &["build"]);
        let mut pc = her_pc_with_a_metal_lease();
        // A build is already running there.
        pc.holdings
            .already_holding(&JobId::new("j0"), labels(&["build"]), Occupant::Job);
        pc.reserved.jobs = 1;
        let queue = vec![build.clone(), trial.clone()];
        let seats = vec![pc];
        let reserved = reservations(&queue, &seats, now(), DRAIN_FIRST_LIMIT);
        assert_eq!(
            reserved
                .get("wsl-pc")
                .map(|(id, label)| (id.to_string(), label.clone())),
            Some(("j9".to_owned(), "metal-pc".to_owned())),
            "the host must be reserved for the trial even though a build is ahead of it in \
             the queue: a drain that lost to queue order would starve (E-27)"
        );
        let mut seats = seats;
        for seat in seats.iter_mut() {
            seat.reserved_for = reserved.get(&seat.name).cloned();
        }
        // The trial waits for the host to go quiet, with the figures.
        let why = waited(place(
            &trial,
            &mut seats,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("wsl-pc is being emptied for it"), "{why}");
        assert!(why.contains("1 job(s) are still running there"), "{why}");
        assert!(why.contains("15m drain-first limit"), "{why}");
        // And the build behind it is **not** claimed there, which is the
        // whole of the drain: builds that kept arriving would otherwise keep
        // the host busy for ever.
        let why = waited(place(
            &build,
            &mut seats,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("wsl-pc is being emptied for j9"),
            "a host being emptied must claim nothing new (E-27): {why}"
        );
    }

    /// A quiet host runs the trial at once.
    #[test]
    fn a_drain_first_job_starts_when_its_host_is_quiet() {
        let trial = queued("j9", &["metal-pc"]);
        let mut seats = vec![her_pc_with_a_metal_lease()];
        let reserved = reservations(
            std::slice::from_ref(&trial),
            &seats,
            now(),
            DRAIN_FIRST_LIMIT,
        );
        for seat in seats.iter_mut() {
            seat.reserved_for = reserved.get(&seat.name).cloned();
        }
        let (runner, _) = placed(place(
            &trial,
            &mut seats,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
    }

    /// A drain that did not win says so, and gives the machine back (E-27).
    ///
    /// Bounded, because a host held empty for a job that never comes is a
    /// machine lent for nothing. Past the limit the reservation is gone, the
    /// host takes other work again, and the trial's sentence says the drain
    /// did not win rather than going on claiming it is about to.
    #[test]
    fn a_drain_that_times_out_gives_the_machine_back_and_says_so() {
        let trial = queued("j9", &["metal-pc"]);
        let build = queued("j1", &["build"]);
        let mut pc = her_pc_with_a_metal_lease();
        pc.holdings
            .already_holding(&JobId::new("j0"), labels(&["build"]), Occupant::Job);
        pc.reserved.jobs = 1;
        let mut seats = vec![pc];
        let too_late = trial
            .submitted_at
            .after(DRAIN_FIRST_LIMIT + Duration::from_secs(60));
        let reserved = reservations(
            &[build.clone(), trial.clone()],
            &seats,
            too_late,
            DRAIN_FIRST_LIMIT,
        );
        assert!(
            reserved.is_empty(),
            "past its limit a drain-first job holds no host empty: {reserved:?}"
        );
        for seat in seats.iter_mut() {
            seat.reserved_for = reserved.get(&seat.name).cloned();
        }
        let why = waited(place(
            &trial,
            &mut seats,
            too_late,
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("The drain did not win"),
            "a drain that lost must say so rather than promising: {why}"
        );
        assert!(why.contains("--drain-first-limit"), "{why}");
    }

    /// A withdrawn or draining machine is never held empty for anything: a
    /// reservation on a machine that claims nothing is a machine lent twice
    /// over for nothing.
    #[test]
    fn nothing_holds_a_withdrawn_machine_empty() {
        let trial = queued("j9", &["metal-pc"]);
        for seat in [
            {
                let mut seat = her_pc_with_a_metal_lease();
                seat.switch = herd_proto::Switch::Off {
                    reason: "she is gaming".to_owned(),
                };
                seat
            },
            {
                let mut seat = her_pc_with_a_metal_lease();
                seat.draining = true;
                seat
            },
        ] {
            let reserved = reservations(
                std::slice::from_ref(&trial),
                &[seat],
                now(),
                DRAIN_FIRST_LIMIT,
            );
            assert!(reserved.is_empty(), "{reserved:?}");
        }
    }

    /// Her PC, with the metal lease of E-27 declared on it: counted(1) with a
    /// probe (D76), and **drain-first declared in her own file** (D86) —
    /// nothing about the spelling `metal-pc` does anything here.
    fn her_pc_with_a_metal_lease() -> Seat {
        let mut seat = seat(
            "wsl-pc",
            10,
            HostSize::new(Some(17 * 1024), Some(20))
                .with_floor(Some(3 * 1024))
                .with_tmpfs(Some(2 * 1024)),
            offering(vec![
                declared(Capability::counted("build", 1)),
                declared(
                    Capability::lease(
                        "metal-pc",
                        BusyProbe::Command {
                            command: vec![
                                "cargo".to_owned(),
                                "xtask".to_owned(),
                                "metal".to_owned(),
                                "pc".to_owned(),
                                "--lease-status".to_owned(),
                            ],
                            busy_exit_code: 10,
                            free_exit_code: 0,
                        },
                    )
                    .draining_first(),
                ),
            ]),
        );
        // The lease read free, which is what a probe says when nobody outside
        // Herd holds the machine (D49: an unread probe suspends, so a test of
        // the drain has to say the lease was read).
        seat.capacity
            .update(vec![CapabilityState::new("metal-pc", Availability::Free)]);
        seat
    }

    /// Nothing in Herd reads the `metal-` prefix (E-27, invariant 1).
    ///
    /// The same label, declared without `drain_first`, behaves as any counted
    /// capability does: the trial runs beside a build. What makes a metal
    /// trial empty the PC is the flag in the PC's own file, and this test is
    /// what keeps that true.
    #[test]
    fn the_metal_prefix_means_nothing_without_the_declared_flag() {
        let trial = queued("j9", &["metal-pc"]);
        let mut pc = seat(
            "wsl-pc",
            10,
            HostSize::new(Some(17 * 1024), Some(20))
                .with_floor(Some(3 * 1024))
                .with_tmpfs(Some(2 * 1024)),
            offering(vec![
                declared(Capability::counted("build", 1)),
                declared(Capability::counted("metal-pc", 1)),
            ]),
        );
        pc.holdings
            .already_holding(&JobId::new("j0"), labels(&["build"]), Occupant::Job);
        pc.reserved.jobs = 1;
        let mut seats = vec![pc];
        let reserved = reservations(
            std::slice::from_ref(&trial),
            &seats,
            now(),
            DRAIN_FIRST_LIMIT,
        );
        assert!(
            reserved.is_empty(),
            "a label called metal-anything means nothing to Herd: {reserved:?}"
        );
        for seat in seats.iter_mut() {
            seat.reserved_for = reserved.get(&seat.name).cloned();
        }
        assert!(matches!(
            place(
                &trial,
                &mut seats,
                now(),
                Stickiness::default(),
                &Placing::default()
            ),
            Placement::Claim { .. }
        ));
    }

    /// The two budgets are counted from different attempts of one record
    /// (D76, D79) — a leased machine must not spend the goes that exist for a
    /// machine that disappears.
    #[test]
    fn the_requeue_budget_and_the_lost_runner_budget_are_counted_apart() {
        let mut record = queued("j1", &["metal-pc"]);
        record.requeue_exit_code = Some(75);
        let attempt = |number: u32, runner: &str, requeue_code: Option<i32>| Attempt {
            number,
            runner: runner.to_owned(),
            started_at: record.submitted_at,
            ended_at: Some(record.submitted_at),
            requeue_code,
            ended: Some("ended".to_owned()),
            at_its_ceiling: None,
            why: "claimed by somebody".to_owned(),
            commit: None,
            steps: Vec::new(),
        };
        record.attempts = vec![
            attempt(1, "wsl-pc", Some(75)),
            attempt(2, "build-host", Some(75)),
        ];
        assert_eq!(record.requeues_spent(), 2);
        assert_eq!(
            record.goes_a_runner_took(),
            0,
            "two machines answering 75 must leave both of E-21's goes unspent"
        );
        assert_eq!(
            record.runners_that_requeued_it(),
            vec!["wsl-pc".to_owned(), "build-host".to_owned()]
        );
        // And the other way round: a lost runner spends no requeue budget.
        record.attempts.push(attempt(3, "wsl-pc", None));
        assert_eq!(record.goes_a_runner_took(), 1);
        assert_eq!(record.requeues_spent(), 2);
        // An attempt still under way counts for neither.
        let mut running = attempt(4, "build-host", None);
        running.ended_at = None;
        running.ended = None;
        record.attempts.push(running);
        assert_eq!(record.goes_a_runner_took(), 1);
        assert_eq!(record.requeues_spent(), 2);
    }

    // ------------------------------- a requirement reading (D159) ----

    /// A record of the job the hub mints to read what another job needs
    /// (D74): an ordinary job in every respect but one — it is marked as the
    /// reading of a job, and the claim step reads that off it (D159).
    fn a_reading(id: &str, baseline: &[&str], reads_for: &str) -> JobRecord {
        let mut record = queued(id, baseline);
        record.reading_for = Some(JobId::new(reads_for.to_owned()));
        assert!(record.is_a_reading());
        assert_eq!(record.occupant(), Occupant::Reading);
        record
    }

    /// **The incident, through the claim step.** Her PC has one build slot.
    /// A build is holding it; a reading whose baseline is `build` is claimed
    /// anyway, and the line explaining the claim says it holds nothing.
    #[test]
    fn a_reading_is_claimed_on_a_machine_whose_only_slot_is_taken() {
        let build = out_on(a_build("j0", 1792), "wsl-pc");
        let mut fleet = fleet_with(her_pc(), std::slice::from_ref(&build));
        // The ordinary job behind it still waits, which is the half of this
        // that must not change.
        let why = waited(place(
            &queued("j1", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("build has 1 slot(s) and 1 are taken"), "{why}");
        // The reading does not.
        let (runner, why) = placed(place(
            &a_reading("j2", &["build"], "j1"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("requirement reading"), "{why}");
        assert!(why.contains("holds none of them"), "{why}");
        assert!(
            !why.contains("slot 1 of 1"),
            "a reading must not be reported as taking a slot: {why}"
        );
    }

    /// And the other direction, which is what the forty-six minutes were: a
    /// reading already running does not stop the real job taking the slot.
    #[test]
    fn a_reading_running_does_not_hold_the_slot_against_a_real_job() {
        let reading = out_on(a_reading("j0", &["build"], "j1"), "wsl-pc");
        let mut fleet = fleet_with(her_pc(), std::slice::from_ref(&reading));
        let (runner, why) = placed(place(
            &a_build("j1", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("build slot 1 of 1"), "{why}");
    }

    /// Eligibility is not waived: a reading waits for a runner that offers
    /// its baseline, because a reading placed on a machine which could not
    /// run the command reads the wrong machine's capabilities.
    #[test]
    fn a_reading_still_waits_for_a_runner_that_offers_its_baseline() {
        let mut fleet = vec![her_pc()];
        let why = waited(place(
            &a_reading("j2", &["kvm"], "j1"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("does not offer kvm"), "{why}");
    }

    /// A probe outside Herd holding a capability — or a probe nobody can read
    /// at all — holds off a job and **not** a reading (D159).
    ///
    /// The failure direction decides it. A reading that runs while the lock
    /// is held costs one `--labels` line, which does not touch the thing the
    /// lock protects. A reading that waits for a probe that cannot be read
    /// waits for ever, and every job its answer unblocks waits behind it.
    #[test]
    fn a_suspended_probe_holds_off_a_job_and_not_a_reading() {
        for suspension in [
            Availability::busy("the lock file /run/metal-pc is there"),
            Availability::unknown("the probe cannot be started"),
        ] {
            let mut fleet = vec![her_pc_with_a_metal_lease()];
            fleet[0]
                .capacity
                .update(vec![CapabilityState::new("metal-pc", suspension.clone())]);
            let why = waited(place(
                &queued("j1", &["metal-pc"]),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
            assert!(why.contains("suspended on wsl-pc"), "{why}");
            let (runner, _) = placed(place(
                &a_reading("j2", &["metal-pc"], "j1"),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
            assert_eq!(runner, "wsl-pc", "a reading runs anyway: {suspension:?}");
        }
    }

    /// A reading never asks for a quiet host, so it neither holds a machine
    /// empty for itself (E-27's fifteen minutes) nor waits for one to empty.
    #[test]
    fn a_reading_never_holds_a_host_empty_and_never_waits_for_a_quiet_one() {
        let reading = a_reading("j2", &["metal-pc"], "j1");
        let build = out_on(a_build("j0", 1792), "wsl-pc");
        let mut fleet = fleet_with(her_pc_with_a_metal_lease(), std::slice::from_ref(&build));
        // Nothing is held empty for it.
        let held = reservations(
            std::slice::from_ref(&reading),
            &fleet,
            now(),
            DRAIN_FIRST_LIMIT,
        );
        assert!(
            held.is_empty(),
            "a reading must not be able to empty a machine: {held:?}"
        );
        // And it is claimed while a build is still running there, which a
        // real `metal-pc` job would have waited for.
        let (runner, _) = placed(place(
            &reading,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        let trial = queued("j3", &["metal-pc"]);
        let why = waited(place(
            &trial,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing {
                parent: Parent::None,
                drain_first_limit: Some(DRAIN_FIRST_LIMIT),
                readings_at_once: None,
            },
        ));
        assert!(why.contains("quiet host"), "{why}");
        // And the drain's own sentence names the reading among the jobs it
        // is waiting for (D159).
        assert!(why.contains("requirement reading"), "{why}");
    }

    /// A reading still respects a host being emptied for somebody else. A
    /// stream of readings allowed onto a draining machine would starve the
    /// drain for ever, which is the attack E-27 exists to answer — and the
    /// wait is bounded by the same fifteen minutes everything else is.
    #[test]
    fn a_reading_respects_a_host_being_emptied_for_another_job() {
        let trial = queued("j1", &["metal-pc"]);
        let mut fleet = vec![her_pc_with_a_metal_lease()];
        let held = reservations(
            std::slice::from_ref(&trial),
            &fleet,
            now(),
            DRAIN_FIRST_LIMIT,
        );
        for seat in fleet.iter_mut() {
            seat.reserved_for = held.get(&seat.name).cloned();
        }
        let why = waited(place(
            &a_reading("j2", &["build"], "j3"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("is being emptied for j1"), "{why}");
    }

    /// **Slot-free, never admission-free** (D119, D159). A reading runs a
    /// process on a machine, so the room there still has to cover it: one
    /// that declares a figure bigger than what is free waits exactly as a job
    /// does, and the sentence carries the figures.
    #[test]
    fn a_reading_is_still_admitted_against_the_room_it_runs_in() {
        // 17 GiB lent less a 3 GiB floor is 14 GiB admitted on her PC.
        let sitting = out_on(a_test("j0", 10 * 1024, 0), "wsl-pc");
        let mut fleet = fleet_with(her_pc(), std::slice::from_ref(&sitting));
        let mut reading = a_reading("j2", &["build"], "j1");
        reading.memory_mib = Some(8 * 1024);
        let why = waited(place(
            &reading,
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("free of its"), "{why}");
        assert!(why.contains("8 GiB needed"), "{why}");
    }

    /// And what a short job is told, when some of the room is held by a
    /// reading: the count of jobs is broken down (D159).
    #[test]
    fn a_short_job_is_told_how_much_of_the_room_a_reading_holds() {
        let mut reading = a_reading("j0", &["build"], "j1");
        reading.memory_mib = Some(10 * 1024);
        let reading = out_on(reading, "wsl-pc");
        let mut fleet = fleet_with(her_pc(), std::slice::from_ref(&reading));
        let why = waited(place(
            &a_test("j2", 8 * 1024, 0),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("1 job running there holds 10 GiB of memory"),
            "{why}"
        );
        assert!(why.contains("of which 1 is a requirement reading"), "{why}");
    }

    // --------------------- how many readings a runner takes (D169) ----

    /// **The incident, through the claim step.** Two readings are running on
    /// the one machine of the fleet; a third is **queued**, not claimed and
    /// not refused, and the sentence says the figure and the flag that sets
    /// it.
    ///
    /// Before D169 nothing counted them at all: a reading holds no slot
    /// (D159), so the two already there were invisible to every gate in
    /// `judge` and the third — and the two hundredth — was claimed.
    #[test]
    fn a_third_reading_waits_for_one_of_the_two_running_to_end() {
        let running = vec![
            out_on(a_reading("j0", &["build"], "j10"), "wsl-pc"),
            out_on(a_reading("j1", &["build"], "j11"), "wsl-pc"),
        ];
        let mut fleet = fleet_with(her_pc(), &running);
        let why = waited(place(
            &a_reading("j2", &["build"], "j12"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("2 requirement reading(s) are already running"),
            "{why}"
        );
        assert!(why.contains("at most 2 there at once"), "{why}");
        assert!(why.contains("--readings-at-once"), "{why}");
    }

    /// And the queue moves: when one of the two ends, the third is claimed.
    /// A reading over the cap waits, it is never refused.
    #[test]
    fn the_third_reading_is_claimed_once_one_of_the_two_has_ended() {
        let running = vec![out_on(a_reading("j0", &["build"], "j10"), "wsl-pc")];
        let mut fleet = fleet_with(her_pc(), &running);
        let (runner, why) = placed(place(
            &a_reading("j2", &["build"], "j12"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("it is reading 2 of the 2"), "{why}");
    }

    /// **The 214 submitted in one second, in miniature.** The seats are
    /// carried through a pass, so readings offered in one pass contend with
    /// each other and not only with what was running when it began: four
    /// readings through one pass are two claims and two waits, never four
    /// claims.
    #[test]
    fn readings_offered_in_one_pass_contend_with_each_other() {
        let mut fleet = vec![her_pc()];
        let mut claimed = 0;
        let mut queued_behind = 0;
        for n in 0..4 {
            match place(
                &a_reading(&format!("j{n}"), &["build"], &format!("j1{n}")),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ) {
                Placement::Claim { .. } => claimed += 1,
                Placement::Wait { why } => {
                    assert!(why.contains("--readings-at-once"), "{why}");
                    queued_behind += 1;
                }
                other => panic!("a reading is claimed or queued, never {other:?}"),
            }
        }
        assert_eq!((claimed, queued_behind), (2, 2));
    }

    /// The figure is a setting, so an operator with a bigger machine can
    /// raise it and a test can make the bound bite at one.
    #[test]
    fn the_number_of_readings_at_once_is_a_setting() {
        let running = vec![out_on(a_reading("j0", &["build"], "j10"), "wsl-pc")];
        let mut fleet = fleet_with(her_pc(), &running);
        let why = waited(place(
            &a_reading("j2", &["build"], "j12"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing {
                readings_at_once: Some(1),
                ..Placing::default()
            },
        ));
        assert!(
            why.contains("1 requirement reading(s) are already running"),
            "{why}"
        );
        assert!(why.contains("at most 1 there at once"), "{why}");
        // And the other way: four at once claims the second.
        let (runner, _) = placed(place(
            &a_reading("j2", &["build"], "j12"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing {
                readings_at_once: Some(4),
                ..Placing::default()
            },
        ));
        assert_eq!(runner, "wsl-pc");
    }

    /// D159 is kept whole: the cap is about readings and about nothing else.
    /// A real job takes the slot it is entitled to with that machine's
    /// readings at their cap.
    #[test]
    fn the_cap_holds_back_readings_and_nothing_else() {
        let running = vec![
            out_on(a_reading("j0", &[], "j10"), "wsl-pc"),
            out_on(a_reading("j1", &[], "j11"), "wsl-pc"),
        ];
        let mut fleet = fleet_with(her_pc(), &running);
        let (runner, why) = placed(place(
            &a_build("j2", 1792),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(runner, "wsl-pc");
        assert!(why.contains("build slot 1 of 1"), "{why}");
    }

    // ------------------------------------------- D181: the announced ceiling

    /// A runner that announced it holds `at_once` jobs, and nothing else
    /// changed about it.
    fn their_server_holding_at_most(at_once: u32) -> Seat {
        let mut seat = their_server();
        seat.jobs_at_once = Some(at_once);
        seat
    }

    /// The same, for the machine with the preference on it.
    fn her_pc_holding_at_most(at_once: u32) -> Seat {
        let mut seat = her_pc();
        seat.jobs_at_once = Some(at_once);
        seat
    }

    /// A job that declares **no memory need at all** — which is the whole
    /// reason this field exists. `admission` costs such a job at zero
    /// (`needs_memory` is `None`, so `memory_mib` is 0), so the memory seat
    /// accounting never stops a queue of them, however long it is.
    fn costs_nothing(id: &str) -> JobRecord {
        queued(id, &[])
    }

    /// The same, declaring a whole-job memory need — and still asking for no
    /// capability, so that nothing but the two bounds under test can be what
    /// holds it up.
    fn costing(id: &str, memory_mib: u64) -> JobRecord {
        let mut record = queued(id, &[]);
        record.memory_mib = Some(memory_mib);
        record
    }

    /// The defect, stated as a test: without a ceiling the hub places an
    /// unbounded number of memory-silent jobs on one machine.
    ///
    /// Kept as the first of these on purpose. It is what a runner that says
    /// nothing still gets — today's behaviour, unchanged, which is what
    /// "absent means it did not say" has to mean in the placement and not
    /// only in the doc comment.
    #[test]
    fn a_runner_that_announced_no_ceiling_is_placed_on_exactly_as_it_was() {
        let mut fleet = vec![their_server()];
        assert_eq!(fleet[0].jobs_at_once, None);
        let mut running = Vec::new();
        for n in 0..40 {
            let job = costs_nothing(&format!("j{n}"));
            let (runner, _) = placed(place(
                &job,
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
            assert_eq!(
                runner, "build-host",
                "job {n} of a memory-silent queue: nothing in the hub bounds this, which is \
                 the defect D181 answers and not something to be fixed by silence"
            );
            running.push(job);
        }
        assert_eq!(fleet[0].reserved.jobs, 40);
        assert_eq!(
            fleet[0].reserved.memory_mib, 0,
            "and the forty of them are costed at nothing, which is why the memory admission \
             could never have been what stopped them"
        );
    }

    /// The same queue against a runner that *did* say. It fills to the figure
    /// it announced and not one job past it, and the job behind reads a
    /// sentence with the arithmetic in it.
    #[test]
    fn a_runner_at_the_ceiling_it_announced_takes_nothing_more_and_says_so() {
        let mut fleet = vec![their_server_holding_at_most(3)];
        for n in 0..3 {
            let (runner, _) = placed(place(
                &costs_nothing(&format!("j{n}")),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
            assert_eq!(runner, "build-host", "job {n} is inside the ceiling");
        }
        // The fourth, in the same pass: the count the ceiling is held
        // against is taken again by `claim`, so two jobs offered together
        // contend for the last place instead of both finding it free.
        let why = waited(place(
            &costs_nothing("j3"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("3 job(s) already out there and it said it holds at most 3 at once"),
            "the sentence has to carry both figures, or an operator cannot tell a full \
             machine from a broken one: {why}"
        );
        assert!(
            why.contains("takes nothing more until one of them ends"),
            "and it has to say what ends the wait: {why}"
        );
        assert_eq!(
            fleet[0].reserved.jobs, 3,
            "and the refused job took no place on the way past"
        );
    }

    /// **A bound beside the memory one, never instead of it.** Whichever
    /// bites first is the one that bites, and the sentence names that one.
    #[test]
    fn the_ceiling_and_the_memory_admission_are_two_bounds_and_either_may_bite() {
        // The ceiling bites first: two jobs of 1 GiB on a machine admitting
        // 25 GiB, which has room for twenty more and will take none.
        let mut fleet = vec![their_server_holding_at_most(2)];
        for n in 0..2 {
            placed(place(
                &costing(&format!("j{n}"), 1024),
                &mut fleet,
                now(),
                Stickiness::default(),
                &Placing::default(),
            ));
        }
        let why = waited(place(
            &costing("j2", 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("it said it holds at most 2 at once"),
            "the ceiling is what is holding this job up and the sentence must say so, not \
             quote 23 GiB of free memory at a machine that will not take it: {why}"
        );

        // The memory bites first: one 20 GiB job on a machine that announced
        // room for eight, and the second 20 GiB job does not fit although
        // seven places are open.
        let mut fleet = vec![their_server_holding_at_most(8)];
        placed(place(
            &costing("j0", 20 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        let why = waited(place(
            &costing("j1", 20 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("5 GiB free of its 25 GiB memory admission"),
            "a ceiling with places left must not stop the memory admission doing its job: \
             {why}"
        );

        // And a job larger than the machine is still *impossible* there,
        // not a wait: the ceiling knows nothing about size and must not
        // turn a refusal that can never clear into one that waits for ever.
        let mut fleet = vec![their_server_holding_at_most(8)];
        let why = waited(place(
            &costing("j0", 40 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("can never fit there however empty it gets"),
            "{why}"
        );
    }

    /// A peer that announces a ceiling of zero is a machine nothing waits
    /// for, not a machine every job waits at for ever.
    ///
    /// Our own runner never announces zero — `herd_runner::jobs_at_once` is
    /// floored at one, deliberately, because invariant 2 asks for a cap and
    /// not a wall — so this is about a peer that is not ours. It still has
    /// to be answered: `ShortNow` sticks a job to the machine it is waiting
    /// for, and on a machine whose ceiling is zero there is no job that
    /// could end there to free the place.
    #[test]
    fn a_ceiling_of_zero_is_a_machine_nothing_waits_for() {
        let mut fleet = vec![their_server_holding_at_most(0), her_pc()];
        let (runner, _) = placed(place(
            &costs_nothing("j0"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(
            runner, "wsl-pc",
            "the job goes to the other machine in this same pass rather than sticking to \
             one that has told it there is nothing to wait for"
        );
        let mut alone = vec![their_server_holding_at_most(0)];
        let why = waited(place(
            &costs_nothing("j1"),
            &mut alone,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("says it holds 0 jobs at once") && why.contains("nothing waits for it"),
            "{why}"
        );
    }

    /// The ceiling counts **every** job out there, requirement readings
    /// included, because the runner's own ceiling does.
    ///
    /// D159 keeps a reading out of the slot algebra and D169 caps readings
    /// separately; neither makes a reading weightless on the machine it runs
    /// on. A hub counting a smaller set than the runner would read room that
    /// is not there and place work the runner is certain to hand back, which
    /// is the whole defect this closes.
    #[test]
    fn the_ceiling_counts_readings_too_because_the_runner_s_does() {
        let running = vec![
            out_on(a_reading("j0", &[], "j10"), "build-host"),
            out_on(a_reading("j1", &[], "j11"), "build-host"),
        ];
        let mut fleet = fleet_with(their_server_holding_at_most(2), &running);
        assert_eq!(fleet[0].reserved.jobs, 2);
        assert_eq!(fleet[0].reserved.readings, 2);
        let why = waited(place(
            &costs_nothing("j2"),
            &mut fleet,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(
            why.contains("2 job(s) already out there and it said it holds at most 2 at once"),
            "two readings fill a ceiling of two, because two readings fill the runner's: {why}"
        );
    }

    /// A machine at its ceiling is still **eligible**: it is busy, not
    /// unsuitable. That is what keeps the job waiting for the machine it
    /// prefers instead of falling to a lesser one — and what bounds the wait
    /// is the stickiness limit, exactly as it is for memory.
    #[test]
    fn a_job_held_by_a_ceiling_waits_for_that_machine_and_then_moves() {
        let running: Vec<JobRecord> = (0..2)
            .map(|n| out_on(costs_nothing(&format!("j{n}")), "wsl-pc"))
            .collect();
        let mut her = fleet_with(her_pc_holding_at_most(2), &running);
        her.push(their_server());
        // Her PC is preferred (10 against 0) and full, so the job waits for
        // it rather than taking the free server.
        let why = waited(place(
            &costs_nothing("j9"),
            &mut her,
            now(),
            Stickiness::default(),
            &Placing::default(),
        ));
        assert!(why.contains("it is waiting for wsl-pc"), "{why}");
        assert!(why.contains("it said it holds at most 2 at once"), "{why}");
        assert!(
            why.contains("build-host could take it and is less preferred"),
            "{why}"
        );

        // And past the stickiness limit it moves, which is what stops a
        // ceiling from being a place a job can sit behind for ever.
        let mut her = fleet_with(her_pc_holding_at_most(2), &running);
        her.push(their_server());
        let waited_too_long = costs_nothing("j9");
        let later = waited_too_long
            .submitted_at
            .after(Stickiness::default().limit + Duration::from_secs(60));
        let (runner, why) = placed(place(
            &waited_too_long,
            &mut her,
            later,
            Stickiness::default(),
            &Placing::default(),
        ));
        assert_eq!(
            runner, "build-host",
            "past the stickiness limit the job takes the less preferred machine: {why}"
        );
    }
}
