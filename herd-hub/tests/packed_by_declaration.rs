//! **The hub's charge is never below the runner's grant** (ORCH-312,
//! invariant 2, invariant 4).
//!
//! A job is charged the ceiling its runner will grant it — the declared sum
//! with the tmpfs inside it and the margin on top — bounded by
//! `C_hub := (lent − floor) / at_once`, this hub's only upper bound on the
//! runner's own `--job-memory-max`, which no frame carries (D180). A job
//! costed at **nothing** is charged all of `C_hub`, because all of that cap
//! is what the runner grants it. On a runner that announced no count there is
//! no `C_hub`, and the bound on one job is the whole room instead.
//!
//! `herd-hub/src/matcher.rs` says, about the first bound `admission` applies,
//! that *"the two sides have to be counting the same set"* (D159, D169). This
//! file is the other half of that sentence: the two sides have to be counting
//! a **charge** over that set that the runner can honour, and after ORCH-312
//! they do, because they charge through one function —
//! `herd_proto::charged_ceiling_mib`. Never *below* the grant rather than
//! equal to it, and the difference is deliberate: a reading with no figure is
//! charged nothing on purpose (see the last test here), and a runner that
//! announced no count is charged conservatively because there is no cap to
//! clamp by. Over-charging costs throughput; under-charging costs attempts,
//! which is what D181 measured.
//!
//! The claim, said once:
//!
//! > For every host size, every floor and every `--job-memory-max`, and for
//! > every job, what this hub charges that job is at least what that job's
//! > runner grants it. So every set the hub admits against `lent − floor` is
//! > a set whose granted ceilings also fit in `lent − floor`: the runner
//! > refuses nothing, no attempt is spent on a refusal, and there is no
//! > refusal storm of the shape D181 measured.
//!
//! **The oracle is the runner's own function**, `herd_runner::job_ceiling` —
//! the one that writes `memory.max` — and not a restatement of it here. A
//! test that re-derived the grant would agree with itself for ever.
//!
//! **And the detector is proved on a known-bad fixture.** The property below
//! is run twice: once over the charge the hub really makes, which must pass,
//! and once over the charge the hub made *before* ORCH-312 — the declaration
//! alone, no margin, no tmpfs, no floor, nothing for an uncosted job — which
//! must fail, with the triple that fails it named. A property test that
//! passes over the defect it was written for has proved nothing, and this one
//! says so out loud.

use herd_hub::matcher::charged_against_the_room;

/// The grid. Hosts from a laptop to the development server, floors from
/// generous to none, caps from the smallest that is a cap to larger than the
/// host — every one of them a figure some real runner could be started with.
const LENT_MIB: [u64; 6] = [1024, 2048, 8192, 17408, 20480, 31744];
const FLOOR_MIB: [u64; 5] = [0, 1024, 2048, 3072, 6144];
const CAP_MIB: [u64; 7] = [16, 256, 2048, 4096, 7168, 8192, 65536];

/// The jobs. One of each kind the hub costs differently, with the two that
/// found defects by reading: the consumer's own declaration with its tmpfs,
/// and a job declaring less than a shell needs.
fn jobs() -> Vec<(&'static str, Option<herd_proto::JobNeed>)> {
    vec![
        ("costed, nothing on tmpfs", need(1792, 0)),
        ("the consumer's own, with tmpfs", need(1792, 2560)),
        ("costed above any cap here", need(60_000, 8192)),
        ("a declaration smaller than a shell", need(1, 0)),
        ("costed at nothing (`submit -- true`)", None),
    ]
}

fn need(memory_mib: u64, tmpfs_mib: u64) -> Option<herd_proto::JobNeed> {
    Some(herd_proto::JobNeed {
        memory_mib,
        tmpfs_mib,
    })
}

/// What a runner of this size announces, and what it would grant this job, in
/// MiB — read off the runner's own two functions and nothing else.
fn the_runner(
    lent: u64,
    floor: u64,
    cap_mib: u64,
    need: Option<herd_proto::JobNeed>,
) -> (u32, u64) {
    let host = herd_proto::HostSize::new(Some(lent), Some(8)).with_floor(Some(floor));
    let cap_bytes = cap_mib << 20;
    let announced = herd_runner::jobs_at_once(&host, cap_bytes);
    let granted = herd_runner::job_ceiling(cap_bytes, need);
    (
        u32::try_from(announced.most()).expect("a count that fits"),
        granted.charged_mib(),
    )
}

/// One charge, for the property to be run over: the hub's, or a hub's that is
/// wrong on purpose.
type Charge = fn(Option<herd_proto::JobNeed>, herd_hub::matcher::TheRoom) -> u64;

/// What the hub really charges (`matcher::charged_against_the_room`, for a job
/// that is not a reading).
fn as_the_hub_charges(need: Option<herd_proto::JobNeed>, room: herd_hub::matcher::TheRoom) -> u64 {
    let (memory_mib, tmpfs_mib) = match need {
        Some(need) => (Some(need.memory_mib), need.tmpfs_mib),
        None => (None, 0),
    };
    charged_against_the_room(memory_mib, tmpfs_mib, room, false)
}

/// **The known-bad fixture.** What the hub charged before ORCH-312: the
/// declared whole-job memory figure, with no margin, no tmpfs inside the
/// ceiling, no floor under a tiny declaration, and nothing at all for a job
/// costed at nothing. Every one of those four is an under-charge, and the
/// property has to fail on it.
fn as_the_hub_charged_before(
    need: Option<herd_proto::JobNeed>,
    _room: herd_hub::matcher::TheRoom,
) -> u64 {
    need.map_or(0, |need| need.memory_mib)
}

/// Run the property over one charge, and give back the triples it failed on.
///
/// **Walked over the triples where the runner's division did not hit its
/// `.max(1)` floor**, which is where the claim holds and is stated to hold.
/// A host that lends less after its floor than it caps a job at announces one
/// place, and the hub then charges that one job the whole of `lent − floor`
/// while the runner grants it the cap: the charge *is* below the grant there,
/// on purpose, and what bounds the machine is that one job is all either side
/// admits. That branch has a test of its own —
/// [`a_host_too_small_for_one_ceiling_admits_one_job_on_both_sides`] — and
/// leaving it inside this walk would have made this property false rather
/// than made that machine safer.
fn where_the_charge_falls_short(charge: Charge) -> Vec<String> {
    let mut short = Vec::new();
    for (lent, floor, cap_mib, admissible) in triples() {
        if admissible < cap_mib {
            continue;
        }
        for (what, need) in jobs() {
            let (at_once, granted_mib) = the_runner(lent, floor, cap_mib, need);
            let charged = charge(need, the_hubs_room(lent, floor, at_once));
            if charged < granted_mib {
                short.push(format!(
                    "{lent} MiB lent, {floor} kept, {cap_mib} a job (admits {admissible}, \
                     announces {at_once}): {what} is charged {charged} MiB and granted \
                     {granted_mib} MiB"
                ));
            }
        }
    }
    short
}

/// Every `(lent, floor, cap, lent − floor)` of the grid, with the pairings
/// that are not a machine left out.
fn triples() -> Vec<(u64, u64, u64, u64)> {
    let mut all = Vec::new();
    for lent in LENT_MIB {
        for floor in FLOOR_MIB {
            if floor >= lent {
                continue;
            }
            for cap_mib in CAP_MIB {
                all.push((lent, floor, cap_mib, lent - floor));
            }
        }
    }
    all
}

/// What this hub weighs a job against on a runner of this size that announced
/// this count — `matcher::the_room`, called as `admission` calls it.
fn the_hubs_room(lent: u64, floor: u64, at_once: u32) -> herd_hub::matcher::TheRoom {
    herd_hub::matcher::the_room(
        &herd_proto::HostSize::new(Some(lent), Some(8)).with_floor(Some(floor)),
        Some(at_once),
    )
}

/// The claim, over the grid.
#[test]
fn the_hubs_charge_is_never_below_the_runners_grant() {
    let short = where_the_charge_falls_short(as_the_hub_charges);
    assert!(
        short.is_empty(),
        "the hub charges less than the runner grants on {} of the grid's triples, so it would \
         admit sets the runner refuses — which is D181's refusal storm. First few:\n{}",
        short.len(),
        short.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// **And the detector fires.** The same property over the charge this hub made
/// before ORCH-312 has to fail, or the test above is green over nothing.
///
/// Both of the under-charges found by *reading* rather than by measuring are
/// named here, so that a future edit that reintroduced either one fails on a
/// sentence that says which: the tmpfs-plus-margin ceiling (the consumer's own
/// job, granted 4787 MiB and charged 1792), and the 16 MiB floor under a
/// declaration smaller than a shell needs.
#[test]
fn the_charge_this_hub_made_before_is_caught_by_that_property() {
    let short = where_the_charge_falls_short(as_the_hub_charged_before);
    assert!(
        !short.is_empty(),
        "the property passed over the defect it was written for, so it is proving nothing"
    );
    let all = short.join("\n");
    assert!(
        all.contains("the consumer's own, with tmpfs"),
        "the tmpfs-inside-the-ceiling under-charge has to be one of the ones it catches:\n{all}"
    );
    assert!(
        all.contains("a declaration smaller than a shell"),
        "and so has the floor under a tiny declaration:\n{all}"
    );
    assert!(
        all.contains("costed at nothing"),
        "and a job costed at nothing, which the runner grants the whole cap:\n{all}"
    );
}

/// **No set the hub admits is a set whose grants do not fit**, which is the
/// consequence the claim exists for, asserted rather than left as an
/// implication.
///
/// Walked over the grid: jobs are admitted one at a time against
/// `lent − floor` and never more than the count the runner announced, exactly
/// as `admission` admits them, and after each one the sum of what the
/// **runner** would grant is checked.
///
/// The bound checked is `max(lent − floor, cap)` and not `lent − floor`
/// alone, because of the one liberty both sides take and take identically:
/// `jobs_at_once`'s `.max(1)`. A machine too small to hold one job at its own
/// ceiling holds one anyway — the runner admits its first job whatever the
/// sum says, and the hub announces one place — so the permitted total on such
/// a machine is one cap, which is what it was before any of this and is the
/// most either side ever permitted.
#[test]
fn no_set_the_hub_admits_is_a_set_the_runner_cannot_hold() {
    let mut sets_that_filled = 0;
    for (lent, floor, cap_mib, admissible) in triples() {
        for (what, need) in jobs() {
            let (at_once, granted_mib) = the_runner(lent, floor, cap_mib, need);
            let charged = as_the_hub_charges(need, the_hubs_room(lent, floor, at_once));
            let mut charged_so_far = 0_u64;
            let mut granted_so_far = 0_u64;
            let mut admitted = 0_u32;
            while charged_so_far + charged <= admissible && admitted < at_once {
                charged_so_far += charged;
                granted_so_far += granted_mib;
                admitted += 1;
            }
            if admitted == 0 {
                // The `.max(1)` branch: neither side admits a set, both
                // admit one job, and that is the next test's subject.
                continue;
            }
            sets_that_filled += 1;
            assert!(
                granted_so_far <= admissible.max(cap_mib),
                "the hub admitted {admitted} job(s) charged {charged_so_far} MiB on a machine \
                 admitting {admissible}, and the runner would grant {granted_so_far} MiB of \
                 ceilings for them: {what} on {lent} lent, {floor} kept, {cap_mib} a job"
            );
        }
    }
    assert!(
        sets_that_filled > 100,
        "only {sets_that_filled} sets were walked, so this is asserting over almost nothing"
    );
}

/// **The `.max(1)` branch, stated rather than skipped.** A host that lends
/// less after its floor than it caps a job at announces **one** place, so the
/// hub admits one job and no more — and one job is what the runner holds
/// there too, whatever the sum says, because a runner that ran nothing would
/// not be the thing invariant 2 asks for.
///
/// So on such a machine the hub charges **less** than the runner grants, and
/// that is safe for one reason only: the set it can admit has one member in
/// it. The brief's own proof says so in as many words, and this is where it
/// is checked rather than asserted.
#[test]
fn a_host_too_small_for_one_ceiling_admits_one_job_on_both_sides() {
    let mut branches = 0;
    for (lent, floor, cap_mib, admissible) in triples() {
        if admissible >= cap_mib {
            continue;
        }
        branches += 1;
        let (at_once, _) = the_runner(lent, floor, cap_mib, None);
        assert_eq!(
            at_once, 1,
            "a host lending {admissible} MiB after its floor and capping a job at {cap_mib} \
             has to announce one place ({lent} lent, {floor} kept)"
        );
        // And the hub's own cap is that whole admission, so a second job of
        // any size whatever is refused by the count before the sum is
        // consulted: `admission` checks the announced count first.
        assert_eq!(
            the_hubs_room(lent, floor, at_once).per_job_cap_mib,
            Some(admissible)
        );
    }
    assert!(
        branches > 5,
        "only {branches} triples reached the `.max(1)` branch, so this is asserting over \
         almost nothing"
    );
}

/// **A requirement reading is charged nothing, and that is deliberate** —
/// with the bound that makes it safe named in the same test (D159, D169).
///
/// `reading_job` gives a reading no figure: it builds and boots nothing, and
/// no figure is invented for one. But the **runner** cannot tell a reading
/// from `herdctl submit -- true` — both arrive with no `JobNeed` and both are
/// granted the whole of `--job-memory-max` — so for a reading, and a reading
/// alone, the hub charges less than the runner grants. What keeps that safe is
/// the count the runner announced, which `admission` still applies as a hard
/// bound: at most `at_once` jobs out there, and `at_once × cap ≤ lent − floor`
/// by the runner's own arithmetic.
///
/// This test is the record of that trade. If the count bound is ever dropped
/// in favour of the sum alone, this test is where it has to be answered.
#[test]
fn a_reading_is_charged_nothing_and_the_announced_count_is_what_bounds_it() {
    let room = the_hubs_room(8192, 1024, 1);
    assert_eq!(room.per_job_cap_mib, Some(7168));
    let reading = charged_against_the_room(None, 0, room, true);
    assert_eq!(reading, 0, "no figure is invented for a reading");
    // And the same job, if it were not a reading, is charged the whole cap —
    // which is what the runner grants it either way.
    let not_a_reading = charged_against_the_room(None, 0, room, false);
    assert_eq!(not_a_reading, 7168);
    let (_, granted) = the_runner(8192, 1024, 7168, None);
    assert_eq!(
        granted, 7168,
        "the runner grants a reading the whole cap, because nothing on the frame says it is one"
    );
    // The bound that covers the difference, on every triple of the grid:
    // `at_once × cap` never exceeds what the host admits, except on
    // `jobs_at_once`'s own `.max(1)` branch, where one job is what both sides
    // allow.
    for lent in LENT_MIB {
        for floor in FLOOR_MIB {
            if floor >= lent {
                continue;
            }
            let admissible = lent - floor;
            for cap_mib in CAP_MIB {
                let (at_once, _) = the_runner(lent, floor, cap_mib, None);
                if at_once == 1 && cap_mib > admissible {
                    continue;
                }
                assert!(
                    u64::from(at_once) * cap_mib <= admissible,
                    "{at_once} × {cap_mib} MiB is more than the {admissible} MiB this host \
                     admits, so the count would not bound what the readings are not charged"
                );
            }
        }
    }
}
