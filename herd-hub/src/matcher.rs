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

use std::collections::BTreeMap;
use std::time::Duration;

use herd_proto::{Holdings, HostSize, JobId, LabelSet, Parallelism};

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
    /// On its way out (invariant 2). It takes no new job, and — the decision
    /// D80 records — it does not count as *available* either, so a job does
    /// not stick to a machine that is leaving.
    pub draining: bool,
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
pub fn holdings(records: &[JobRecord]) -> BTreeMap<String, Vec<(JobId, LabelSet)>> {
    let mut held: BTreeMap<String, Vec<(JobId, LabelSet)>> = BTreeMap::new();
    for (runner, record) in out_there(records) {
        held.entry(runner.to_owned())
            .or_default()
            .push((record.id.clone(), record.labels.clone()));
    }
    held
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
    fn take(&mut self, memory_mib: u64, tmpfs_mib: u64) {
        self.memory_mib = self.memory_mib.saturating_add(memory_mib);
        self.tmpfs_mib = self.tmpfs_mib.saturating_add(tmpfs_mib);
        self.jobs += 1;
    }

    /// The clause naming what is already taken here (E-04).
    fn sentence(&self) -> String {
        match self.jobs {
            0 => "nothing is running there".to_owned(),
            1 => format!(
                "1 job running there holds {} of memory and {} of tmpfs",
                herd_proto::size_mib(self.memory_mib),
                herd_proto::size_mib(self.tmpfs_mib)
            ),
            many => format!(
                "{many} jobs running there hold {} of memory and {} of tmpfs",
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
            Ok(memory) => here.take(memory.unwrap_or(0), record.tmpfs_need_mib()),
            Err(why) => {
                here.take(0, record.tmpfs_need_mib());
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
fn what_it_takes(job_labels: &LabelSet, seat: &Seat) -> String {
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
fn judge(record: &JobRecord, seat: &Seat) -> Verdict {
    if seat.draining {
        return Verdict::NotHere(format!(
            "{} is draining and takes no new job, so nothing waits for it",
            seat.name
        ));
    }
    // What M5's algebra decides: the kinds, under contention with what is
    // already running here (E-25).
    let contention = seat.holdings.check(&record.labels);
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
    if let Some(held_up) = seat.capacity.held_up_by(&record.labels) {
        return if held_up.clears_with_time {
            Verdict::Later(held_up.reason)
        } else {
            Verdict::NotHere(held_up.reason)
        };
    }
    if let Err(contention) = contention {
        let reason = format!("on {}, {contention}", seat.name);
        return if contention.clears_with_time() {
            Verdict::Later(reason)
        } else {
            Verdict::NotHere(reason)
        };
    }
    // And what M7 adds: the parallelism one slot of this machine gets (E-22).
    // **Before admission, always**, because the memory a build needs is that
    // figure times its per-thread declaration and N is chosen first (D82).
    let derived = match record.build_thread_mib {
        None => None,
        Some(build_thread_mib) => match seat
            .host
            .parallelism(build_thread_mib, slots_for(&record.labels, &seat.capacity))
        {
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
        } => Verdict::Now(Fitted {
            derived,
            memory_mib,
            tmpfs_mib,
        }),
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
) -> Placement {
    // A requirement the record does not carry is refused by name, never read
    // as zero and placed anywhere (D74's rule, and the defect behind it): a
    // job asking for a derived `-j` with nowhere to put it would be recorded
    // as `-j 7` and run with its tool's own default.
    if let Some(why) = nothing_can_ever_run_this(record) {
        return Placement::Impossible { why };
    }
    if seats.is_empty() {
        return Placement::Wait {
            why: "no runner is connected to this hub".to_owned(),
        };
    }
    let verdicts: Vec<Verdict> = seats.iter().map(|seat| judge(record, seat)).collect();
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
        return claim(record, &mut seats[at], fitted_at(at), eligible, None);
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
            return claim(record, &mut seats[at], fitted_at(at), eligible, Some(moved));
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
) -> Placement {
    let Fitted {
        derived,
        memory_mib,
        tmpfs_mib,
    } = fitted;
    let takes = what_it_takes(&record.labels, seat);
    let labels = if record.labels.is_empty() {
        "none".to_owned()
    } else {
        record.labels.to_string()
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
    if let Err(contention) = seat.holdings.admit(&record.id, record.labels.clone()) {
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
    seat.reserved.take(memory_mib, tmpfs_mib);
    let command = match derived {
        Some(derived) => herd_proto::with_parallelism(&record.command, derived.threads),
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
    match herd_proto::parallelism_tokens(&record.command) {
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
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
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
            draining: false,
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
        ));
        assert_eq!(first.0, "build-host");
        assert!(first.1.contains("build slot 1 of 2"), "{}", first.1);
        let second = placed(place(
            &queued("j2", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
        ));
        assert!(second.1.contains("build slot 2 of 2"), "{}", second.1);
        let third = waited(place(
            &queued("j3", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
        );
        assert!(matches!(counted, Placement::Claim { .. }));
        let exclusive = waited(place(
            &queued("j2", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
        );
        assert!(matches!(exclusive, Placement::Claim { .. }));
        let counted = waited(place(
            &queued("j2", &["build"]),
            &mut fleet,
            now(),
            Stickiness::default(),
        ));
        assert!(counted.contains("holds igpu-vfio"), "{counted}");
        let needs_nothing = waited(place(
            &queued("j3", &[]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
                Stickiness::default()
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
                Stickiness::default()
            ),
            Placement::Claim { .. }
        ));
        let other_way = waited(place(
            &queued("j2", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
                Stickiness::default()
            ),
            Placement::Claim { .. }
        ));
        let back = waited(place(
            &queued("j2", &["gpu-venus"]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
                    Stickiness::default()
                ),
                Placement::Claim { .. }
            ));
            let blocked = waited(place(
                &queued("j2", &[second]),
                &mut fleet,
                now(),
                Stickiness::default(),
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
                Stickiness::default()
            ),
            Placement::Claim { .. }
        ));
        assert!(
            matches!(
                place(
                    &queued("j2", &["c"]),
                    &mut fleet,
                    now(),
                    Stickiness::default()
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
        ));
        assert!(why.contains("-j 6"), "two slots halve the share: {why}");
        assert!(why.contains("shared between 2 slots"), "{why}");
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
        let why = impossible(place(&zero, &mut fleet, now(), Stickiness::default()));
        assert!(why.contains("0 MiB"), "{why}");

        let mut no_token = a_build("j2", 1792);
        no_token.command = vec!["cargo".to_owned(), "xtask".to_owned(), "pack".to_owned()];
        let why = impossible(place(&no_token, &mut fleet, now(), Stickiness::default()));
        assert!(why.contains("{j}") && why.contains("1792"), "{why}");

        let mut twice = a_build("j3", 1792);
        twice.command = vec!["make".to_owned(), "-j{j}".to_owned(), "J={j}".to_owned()];
        let why = impossible(place(&twice, &mut fleet, now(), Stickiness::default()));
        assert!(why.contains("2 times"), "{why}");

        // And a job that asks for nothing derived is untouched by all of it.
        assert!(matches!(
            place(
                &queued("j4", &["build"]),
                &mut fleet,
                now(),
                Stickiness::default()
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
        let waiting = place(&second, &mut fleet, second.submitted_at, stickiness);
        assert!(matches!(waiting, Placement::Wait { .. }), "{waiting:?}");
        second.waiting_for = None;
        let (runner, why) = placed(place(&second, &mut fleet, moment, stickiness));
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
        ));
        assert!(why.contains("denied"), "{why}");

        // And an empty fleet is a wait, never an end.
        let why = waited(place(
            &queued("j5", &["build"]),
            &mut [],
            now(),
            Stickiness::default(),
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
        seat.holdings
            .already_holding(&JobId::new("j-elsewhere"), labels(&["build"]));
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
            why: "claimed by wsl-pc: labels build".to_owned(),
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
        seat.holdings
            .already_holding(&JobId::new("j-old"), labels(&["a-label-nobody-offers"]));
        let mut fleet = vec![seat];
        let why = waited(place(
            &queued("j1", &["igpu-vfio"]),
            &mut fleet,
            now(),
            Stickiness::default(),
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
            why: format!("claimed by {runner}"),
        }];
        record
    }

    /// A fleet of one seat, with what the records say is already out on it.
    fn fleet_with(mut seat: Seat, running: &[JobRecord]) -> Vec<Seat> {
        let held = holdings(running);
        for (job, labels) in held.get(&seat.name).into_iter().flatten() {
            seat.holdings.already_holding(job, labels.clone());
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
        let (runner, why) = placed(place(&first, &mut fleet, now(), Stickiness::default()));
        assert_eq!(runner, "build-host");
        assert!(
            why.contains("admitted 14 GiB of memory, 11 GiB free of 25 GiB after it"),
            "{why}"
        );
        let why = waited(place(&second, &mut fleet, now(), Stickiness::default()));
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
        let (_, why) = placed(place(&first, &mut fleet, now(), Stickiness::default()));
        assert!(
            why.contains("reserved 5 GiB in tmpfs, 3 GiB free of 8 GiB"),
            "{why}"
        );
        let why = waited(place(&second, &mut fleet, now(), Stickiness::default()));
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
        ));
        assert_eq!(runner, "big-host");
        // With that machine gone, the job waits rather than ending.
        let mut fleet = vec![their_server()];
        let placement = place(
            &a_test("j3", 1024, 16 * 1024),
            &mut fleet,
            now(),
            Stickiness::default(),
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
        let why = impossible(place(&zero, &mut fleet, now(), Stickiness::default()));
        assert!(
            why.contains("0 MiB of memory, which is not a figure"),
            "{why}"
        );

        let mut both = a_build("j2", 1792);
        both.memory_mib = Some(8192);
        let why = impossible(place(&both, &mut fleet, now(), Stickiness::default()));
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
        ));
        let mut hammered = vec![their_server()];
        hammered[0].load_milli = Some(50_000);
        let (busy_runner, busy_why) = placed(place(
            &a_test("j1", 8192, 2048),
            &mut hammered,
            now(),
            Stickiness::default(),
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
}
