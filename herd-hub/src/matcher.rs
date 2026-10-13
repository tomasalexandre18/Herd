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

/// What one runner can do with one job right now.
#[derive(Debug, Clone)]
enum Verdict {
    /// Now, with whatever was derived for it.
    Now(Option<Parallelism>),
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
    for record in records {
        if !matches!(
            record.state,
            JobState::Assigned | JobState::Running | JobState::Interrupted { .. }
        ) {
            continue;
        }
        let Some(attempt) = record.attempt() else {
            continue;
        };
        if attempt.ended_at.is_some() {
            continue;
        }
        held.entry(attempt.runner.clone())
            .or_default()
            .push((record.id.clone(), record.labels.clone()));
    }
    held
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
    let Some(build_thread_mib) = record.build_thread_mib else {
        return Verdict::Now(None);
    };
    match seat
        .host
        .parallelism(build_thread_mib, slots_for(&record.labels, &seat.capacity))
    {
        Ok(derived) => Verdict::Now(Some(derived)),
        // A machine that does not say how big it is cannot be given a job
        // whose `-j` is derived from its size. Not a failure: another machine
        // may say. Never a figure invented for it.
        Err(why) => Verdict::NotHere(format!("{} {why}", seat.name)),
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
    let derived_at = |at: usize| match &verdicts[at] {
        Verdict::Now(derived) => *derived,
        _ => None,
    };
    if let Some(at) = free(true).first().copied() {
        return claim(record, &mut seats[at], derived_at(at), eligible, None);
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
                derived_at(at),
                eligible,
                Some(moved),
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
    derived: Option<Parallelism>,
    eligible: usize,
    moved: Option<String>,
) -> Placement {
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
            HostSize::new(Some(31 * 1024), Some(12)),
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
            HostSize::new(Some(17 * 1024), Some(20)),
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
        let why = waited(claim(&queued("j1", &["build"]), &mut seat, None, 1, None));
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
}
