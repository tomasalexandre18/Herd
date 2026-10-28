//! D103's ordering oracle: the record's two syncs have **returned** before
//! the first byte of the event append is **issued**.
//!
//! # Why this exists
//!
//! `docs/remediation/06-junction-breakdown.md` and `08-s0-unsynced-write-cost.md`
//! both **counted** the hub's `fsync`s. Neither **ordered** them, and D103 is
//! an ordering rule: `events.jsonl` may be one transition *behind* `job.json`
//! and must never be *ahead* of it.
//!
//! `docs/remediation/10-ordering-on-ext4.md` settles what holds that rule on
//! ext4, where `08`'s ZFS transaction-group argument does not transfer: on a
//! **decisive** write nothing filesystem-specific holds it at all. What holds
//! it is that `write_file_with` has returned — both of its syncs with it —
//! before `append_line` is even called. That is a syscall-return guarantee,
//! and it is only as good as the order of the calls.
//!
//! `11-sync-prices.md` then raised the directory sync's standing rather than
//! lowering it: `sync_all` makes the record's *bytes* durable, and only
//! `sync_dir` makes its *rename* durable. A decisive write that lost its
//! directory sync could have its transition lost while the `events.jsonl`
//! line naming that transition survived — `events.jsonl` genuinely ahead of
//! `job.json`, which is exactly what D103 forbids.
//!
//! So this file is a **guard**, not a description. It stands in front of
//! ORCH-083 and ORCH-084, which propose to drop syncs elsewhere: if either of
//! them drops or reorders a sync on a path that writes a record *and* an
//! event, the assertions below go red.
//!
//! # What ORCH-083 changed here, and what it did not
//!
//! **ORCH-083 (D183 step A) has been taken.** `record_waiting`'s `waiting`
//! line no longer waits for the disk, so the one call that writes it has no
//! `AppendSynced` to observe and the oracle no longer expects one *there*.
//!
//! **Nothing that was asserted is asserted less.** The three returns D103
//! rests on are all on the record side, and all three are still required,
//! for that call as for the other sixteen: the record's `sync_all` returned
//! before its `rename` was issued, the `rename` returned before the directory
//! `fsync` was issued, and the directory `fsync` returned before the **first
//! byte** of the event line was issued. Step A removed a syscall that
//! happened *after* the last of those, and the only expectation that moved is
//! the one that named it. Twelve of the thirteen event lines still carry it;
//! which call is allowed not to is written out below rather than derived, and
//! the tripwire at the end of the file holds `store.rs` to exactly one
//! unsynced appender, so a *transition* cannot be moved onto that door
//! without this file going red twice over.
//!
//! # What it observes, and what that proves
//!
//! The observation point is [`crate::durable::watch`], five calls placed
//! inside the real `write_file_with` and the real `append_one_line`, the body
//! both `append_line` and `append_line_unsynced` run. The store
//! here is a real [`JobDirStore`] on a real directory, driven through the
//! calls the hub itself makes. **Nothing is replicated**: unlike
//! `herd-hub/tests/sync_prices.rs`, which times a faithful *copy* of
//! `record_waiting`, what is asserted here is the code that ships.
//!
//! **It proves**, for every store call that writes a record and appends an
//! event:
//!
//! 1. the record's `sync_all` returned before its `rename` was issued;
//! 2. its `rename` returned before the directory `fsync` was issued;
//! 3. the directory `fsync` returned before the **first byte** of the event
//!    line was issued;
//! 4. and the whole observed sequence of record and event operations is
//!    exactly the one named per transition, so a reordering inside one call
//!    cannot hide behind an earlier complete write — and so a line moved from
//!    one append door to the other is a change to that sequence.
//!
//! **It does not prove** that the bytes reached the platters in that order —
//! no in-process observation can, and D103 does not need it: a `fsync` that
//! has returned is durable by the syscall's own contract. It does not prove
//! anything about a path that bypasses `crate::durable`; nothing in the hub
//! does, and `durable.rs`'s module comment is the standing statement of that.
//! And it watches a `cfg(test)` build of the library, so a future
//! `#[cfg(not(test))]` divergence inside `durable.rs` would be invisible to
//! it. `docs/remediation/12-ordering-oracle.md` records the strace
//! cross-check that answers that last gap once, from outside the process.

use super::*;

use crate::durable::watch::{Op, Step};
use crate::job::Mint;
use herd_proto::{JobOutcome, Step as ProtoStep};

/// How many `self.write_record(` call sites `store.rs` holds.
///
/// Every one of them is driven below. The number is asserted against the
/// source so that a new writer cannot be added without someone deciding
/// whether the oracle has to cover it — which is the only way a list written
/// by hand stays complete.
const RECORD_WRITE_SITES: usize = 14;

/// How many `self.append_event(` call sites `store.rs` holds — the appenders
/// that **wait for the disk**. Same rule.
const EVENT_APPEND_SITES: usize = 10;

/// How many `self.append_event_unsynced(` call sites `store.rs` holds — the
/// appenders that do **not** wait for the disk (D183 step A).
///
/// **One, and it is `record_waiting`'s.** This number is the narrowness of
/// step A expressed as an assertion: a second site, on any writer, has to be
/// argued for against D103 before this moves, because the argument that
/// carries step A is about a `waiting` reason and not about a transition
/// (`docs/remediation/10-ordering-on-ext4.md` §1 F-1 to F-5, D183).
const UNSYNCED_EVENT_APPEND_SITES: usize = 1;

/// Which of the two files of a job directory an observed operation is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum On {
    /// `job.json`.
    Record,
    /// `events.jsonl`.
    Events,
}

/// One complete record write, as `durable::write_file_with` performs it.
const RECORD: &[(Op, On)] = &[
    (Op::TemporarySynced, On::Record),
    (Op::Renamed, On::Record),
    (Op::DirectorySynced, On::Record),
];

/// One appended event line, as `durable::append_line` performs it.
const EVENT: &[(Op, On)] = &[
    (Op::AppendIssued, On::Events),
    (Op::AppendSynced, On::Events),
];

/// One appended event line, as `durable::append_line_unsynced` performs it —
/// **D183 step A**, `record_waiting`'s `waiting` line and nothing else.
///
/// The first element is the same one, and it is the one D103 turns on: the
/// rule below reads `AppendIssued` against the record write beside it, and
/// that assertion is unchanged, unweakened and still made for this call. What
/// is gone is the step **after** it, because there is no longer a syscall
/// there to observe. Nothing that was asserted about the order is asserted
/// less; one operation stopped existing.
///
/// Which calls get this shape is written out per transition below, so moving a
/// *transition* onto the unsynced door would not quietly pass: the call's
/// observed sequence would hold one step where the oracle was told to expect
/// two, and the equality at the end of [`Oracle::transition`] goes red.
const EVENT_UNSYNCED: &[(Op, On)] = &[(Op::AppendIssued, On::Events)];

/// What one store call is expected to do to a job's two files, in order.
///
/// Written out per transition rather than derived, so that the expectation
/// is a statement someone made and not a restatement of whatever the code
/// happened to do.
#[derive(Clone, Copy, Debug)]
enum Part {
    /// `job.json` rewritten whole.
    Record,
    /// One line appended to `events.jsonl`, and put on the disk.
    Event,
    /// One line appended to `events.jsonl` without waiting for the disk.
    /// **Exactly one call below is allowed this**, and the tripwire at the
    /// end of the file holds `store.rs` to the same count.
    EventUnsynced,
}

fn expected(parts: &[Part]) -> Vec<(Op, On)> {
    let mut want = Vec::new();
    for part in parts {
        want.extend_from_slice(match part {
            Part::Record => RECORD,
            Part::Event => EVENT,
            Part::EventUnsynced => EVENT_UNSYNCED,
        });
    }
    want
}

/// The observed operations on one job's `job.json` and `events.jsonl`, in the
/// order they happened, with everything else dropped.
///
/// Everything else is real and is deliberately ignored: `index/state.json`,
/// the readings cache, the files of a collection under `files/` and the
/// in-place `log.seq` are all written on these paths too, and none of them is
/// a record or an event.
fn on_the_job(trace: &[Step], job_dir: &Path) -> Vec<(Op, On)> {
    trace
        .iter()
        .filter_map(|step| {
            if step.path.parent() != Some(job_dir) {
                return None;
            }
            let on = match step.path.file_name()?.to_str()? {
                RECORD_FILE => On::Record,
                EVENTS_FILE => On::Events,
                _ => return None,
            };
            Some((step.op, on))
        })
        .collect()
}

/// Where a record write had got to when an event line was issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Written {
    /// No record write has begun in this call.
    Nothing,
    /// `sync_all` on the temporary has returned; the record is not yet
    /// visible under its own name.
    BytesSynced,
    /// The rename has returned; the record is visible and its name is not yet
    /// durable.
    Renamed,
    /// The directory `fsync` has returned. **This is the only state in which
    /// an event line may be issued.**
    Durable,
}

/// The rule itself, over one call's trace.
///
/// Stated once, applied to every transition: an event line may be issued only
/// against a record write that is `Durable`, and a record write only ever
/// goes `Nothing` → `BytesSynced` → `Renamed` → `Durable`.
///
/// `records_expected` is false for the two calls that append an event and
/// write no record at all (the log-gap markers); there the rule is vacuous
/// and saying so is the point.
fn check_the_rule(what: &str, trace: &[Step], records_expected: bool) {
    let mut written: HashMap<PathBuf, Written> = HashMap::new();
    for (index, step) in trace.iter().enumerate() {
        let Some(name) = step.path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        match (step.op, name) {
            (Op::TemporarySynced, RECORD_FILE) => {
                let at = written.entry(step.path.clone()).or_insert(Written::Nothing);
                assert!(
                    matches!(at, Written::Nothing | Written::Durable),
                    "{what}: step {index} began a second write of {} while the first had \
                     only reached {at:?}",
                    step.path.display()
                );
                *at = Written::BytesSynced;
            }
            (Op::Renamed, RECORD_FILE) => {
                let at = written.entry(step.path.clone()).or_insert(Written::Nothing);
                assert_eq!(
                    *at,
                    Written::BytesSynced,
                    "{what}: step {index} renamed {} over its destination while the \
                     record's own fsync had not returned — the bytes can be lost under \
                     the new name",
                    step.path.display()
                );
                *at = Written::Renamed;
            }
            (Op::DirectorySynced, RECORD_FILE) => {
                let at = written.entry(step.path.clone()).or_insert(Written::Nothing);
                assert_eq!(
                    *at,
                    Written::Renamed,
                    "{what}: step {index} fsynced the directory of {} out of order",
                    step.path.display()
                );
                *at = Written::Durable;
            }
            (Op::AppendIssued, EVENTS_FILE) => {
                if !records_expected {
                    continue;
                }
                let record = step.path.with_file_name(RECORD_FILE);
                let at = written.get(&record).copied().unwrap_or(Written::Nothing);
                assert_eq!(
                    at,
                    Written::Durable,
                    "D103 VIOLATED — {what}: the first byte of an event line was issued \
                     while the record write of {} had only reached {at:?}. An event line \
                     is durable the moment append_line returns; a record whose fsync or \
                     whose directory fsync has not returned is not. events.jsonl can \
                     therefore end up ahead of job.json, which D103 forbids.\n\
                     The trace of this call:\n{}",
                    record.display(),
                    describe(trace)
                );
            }
            _ => {}
        }
    }
    if records_expected {
        assert!(
            written.values().any(|at| *at == Written::Durable),
            "{what}: no complete record write was observed at all\n{}",
            describe(trace)
        );
    }
    for (path, at) in &written {
        assert_eq!(
            *at,
            Written::Durable,
            "{what}: the write of {} was left at {at:?} when the call returned",
            path.display()
        );
    }
}

/// A trace, one operation per line, for a failure message.
fn describe(trace: &[Step]) -> String {
    trace
        .iter()
        .enumerate()
        .map(|(index, step)| {
            format!(
                "  {index:>2}. {:?} {}\n",
                step.op,
                step.path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        })
        .collect()
}

/// The oracle, applied to one call.
struct Oracle<'a> {
    watch: &'a crate::durable::watch::Armed,
    /// The directory of the job being followed, learned from the first
    /// `job.json` the mint writes — because the mint is itself one of the
    /// transitions under test, and its id does not exist before it runs.
    job_dir: std::cell::RefCell<Option<PathBuf>>,
}

impl Oracle<'_> {
    /// Run `call`, then hold its trace against the rule and against the
    /// sequence `parts` names.
    fn transition<T>(&self, what: &str, parts: &[Part], call: impl FnOnce() -> T) -> T {
        // Anything the fixture did before this call belongs to the call
        // before it.
        let _ = self.watch.take();
        let outcome = call();
        let trace = self.watch.take();
        let records_expected = parts.iter().any(|part| matches!(part, Part::Record));
        check_the_rule(what, &trace, records_expected);
        let mut held = self.job_dir.borrow_mut();
        if held.is_none() {
            *held = trace
                .iter()
                .find(|step| step.path.file_name() == Some(RECORD_FILE.as_ref()))
                .and_then(|step| step.path.parent().map(Path::to_path_buf));
        }
        let job_dir = held.clone().unwrap_or_else(|| {
            panic!(
                "{what}: no job.json was written, so there is no job \
                                       directory to follow\n{}",
                describe(&trace)
            )
        });
        assert_eq!(
            on_the_job(&trace, &job_dir),
            expected(parts),
            "{what}: the record and event operations of this call are not the sequence \
             the oracle was told to expect\nobserved:\n{}",
            describe(&trace)
        );
        outcome
    }
}

/// A collection a runner carried inline, so the bytes are under `files/` and
/// the collection is whole — which is what the sweeper's eviction needs.
fn carried(path: &str, data: &[u8]) -> herd_proto::JobOutputs {
    herd_proto::JobOutputs {
        files: vec![herd_proto::OutputFile::collected(
            path.to_owned(),
            data.to_vec(),
        )],
        ..herd_proto::JobOutputs::default()
    }
}

/// **The oracle.** Every store call that writes a record, appends an event,
/// or both, driven once, with the order asserted after each one.
///
/// One test and not seventeen, deliberately: the observation point is a
/// process-wide recording, the fixture is one job carried through its whole
/// life, and a call's trace means nothing without the state the calls before
/// it left. The `what` string names the transition in every failure.
#[test]
fn every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let watch = crate::durable::watch::armed(dir.path());

    // --- 1. a job is minted (store.rs `submit`) ---
    let work = || {
        NewJob::at(
            vec!["true".to_owned()],
            LabelSet::default(),
            Priority::Producer,
        )
    };
    let oracle = Oracle {
        watch: &watch,
        job_dir: std::cell::RefCell::new(None),
    };
    let minted = oracle.transition(
        "submit — a job is minted",
        &[Part::Record, Part::Event],
        || store.create_job(work()).expect("minted"),
    );
    let id = minted.id.clone();
    assert_eq!(
        oracle.job_dir.borrow().as_deref(),
        Some(dir.path().join(JOBS_DIR).join(id.as_str()).as_path()),
        "the oracle is following the job the mint just made"
    );

    // --- 2. a second identical submission joins, and raises (store.rs `join`) ---
    // One record write, then *two* event lines: the raise and the join.
    let joined = oracle.transition(
        "join — a second identical submission joins and raises",
        &[Part::Record, Part::Event, Part::Event],
        || {
            store
                .submit(NewJob::at(
                    vec!["true".to_owned()],
                    LabelSet::default(),
                    Priority::Nightly,
                ))
                .expect("the submission is answered")
        },
    );
    assert!(
        matches!(joined, Submission::Joined { .. }),
        "the fixture needs a join, not a second job: {joined:?}"
    );

    // --- 3. an operator raises the level (store.rs `raise_priority_because`) ---
    oracle.transition(
        "raise_priority — an operator raises the level",
        &[Part::Record, Part::Event],
        || store.raise_priority(&id, Priority::Step).expect("raised"),
    );

    // --- 4. why a queued job is not starting (store.rs `record_waiting`) ---
    // **The one unsynced appender** (D183 step A, ORCH-083). The record's
    // three operations are still required, in order, and the first byte of
    // the line still may not be issued until the directory `fsync` has
    // returned — which is the whole of what D103 needs and all this call ever
    // proved. What is gone is the `fdatasync` that followed it.
    oracle.transition(
        "record_waiting — why a queued job is not starting",
        &[Part::Record, Part::EventUnsynced],
        || {
            assert!(
                store
                    .record_waiting(&id, "no runner has the labels this job asks for")
                    .expect("recorded"),
                "the first reason is news"
            );
        },
    );

    // --- 5. where the requirements came from (store.rs `record_reading`) ---
    // A record write with no event of its own: the rule still binds the
    // record's own two syncs to each other and to the rename.
    oracle.transition(
        "record_reading — a reading is written on the record",
        &[Part::Record],
        || {
            store
                .record_reading(
                    &id,
                    crate::job::Reading::of(herd_proto::LabelsFrom {
                        line: "sh tools/labels.sh".to_owned(),
                        baseline: LabelSet::default(),
                    }),
                )
                .expect("recorded");
        },
    );

    // --- 6. an attempt begins (store.rs `begin_attempt`) ---
    // **Two pairs in one call**: `move_to(Assigned)` writes the record and
    // appends the transition, then the attempt is added and `attempt_started`
    // appended. The sequence below is what forbids the second append from
    // sliding above the second record write.
    let attempt = oracle.transition(
        "begin_attempt — the job is assigned and an attempt is opened",
        &[Part::Record, Part::Event, Part::Record, Part::Event],
        || {
            store
                .begin_attempt(&id, "test-runner", "the only runner", None)
                .expect("the attempt is recorded")
        },
    );
    assert_eq!(attempt, 1);

    // --- 7. one transition (store.rs `move_to`, through `record_state`) ---
    oracle.transition(
        "record_state — one transition, assigned to running",
        &[Part::Record, Part::Event],
        || {
            store
                .record_state(&id, JobState::Running)
                .expect("transition")
        },
    );

    // --- 8. one executor step (store.rs `record_step`) ---
    oracle.transition(
        "record_step — one executor step on the attempt",
        &[Part::Record],
        || {
            store
                .record_step(
                    &id,
                    attempt,
                    StepRun {
                        step: ProtoStep::Run,
                        started_unix_ms: 1_791_878_153_000,
                        duration_ms: 12,
                        exit_code: Some(0),
                        ended: None,
                        ran_commit: None,
                    },
                )
                .expect("recorded");
        },
    );

    // --- 9. a gap in the log, found by a chunk arriving out of turn ---
    // **An event with no record write**, which is why the rule is told not to
    // expect one: the gap is a fact about the log, not a transition.
    store
        .append_log(&id, LogStream::Stdout, 0, b"the first chunk")
        .expect("appended");
    oracle.transition(
        "append_log — a chunk arrives past a gap, and the gap is named",
        &[Part::Event],
        || {
            store
                .append_log(&id, LogStream::Stdout, 2, b"the third chunk")
                .expect("appended");
        },
    );

    // --- 10. a gap the hub gives up on (store.rs `name_missing_log`) ---
    oracle.transition(
        "name_missing_log — chunks that are gone for good are named",
        &[Part::Event],
        || {
            store.name_missing_log(&id, 3, 4).expect("the gap is named");
        },
    );

    // --- 11. what the runner measured (store.rs `record_metrics`) ---
    oracle.transition(
        "record_metrics — the runner's figures land on the record",
        &[Part::Record],
        || {
            store
                .record_metrics(&id, JobMetrics::default())
                .expect("recorded");
        },
    );

    // --- 12. the collection (store.rs `record_outputs`) ---
    // The files under `files/` go through `write_file` too; `on_the_job`
    // drops them, and the rule ignores them because they are neither the
    // record nor the events.
    oracle.transition(
        "record_outputs — a collection carried inline",
        &[Part::Record],
        || {
            store
                .record_outputs(&id, &carried("files/results.json", b"the truth"))
                .expect("recorded");
        },
    );

    // --- 13. the attempt ends (store.rs `end_attempt`) ---
    oracle.transition(
        "end_attempt — the attempt is closed with its reason",
        &[Part::Record, Part::Event],
        || {
            store
                .end_attempt(
                    &id,
                    attempt,
                    "the runner went away",
                    crate::job::HowItEnded::TheRunnerWentAway,
                )
                .expect("ended");
        },
    );

    // --- 14. the job ends (store.rs `move_to`, terminal) ---
    oracle.transition(
        "record_state — the terminal transition",
        &[Part::Record, Part::Event],
        || {
            store
                .record_state(
                    &id,
                    JobState::Finished {
                        outcome: JobOutcome::Exited { code: 0 },
                    },
                )
                .expect("finished");
        },
    );

    // --- 15. the collection is fetched (store.rs `note_outputs_fetched`) ---
    oracle.transition(
        "note_outputs_fetched — the first fetch is stamped",
        &[Part::Record],
        || {
            store.note_outputs_fetched(&id).expect("stamped");
        },
    );

    // --- 16. the sweeper takes the collection (store.rs `evict`) ---
    // Called where the sweeper calls it, under its own name: putting the hub
    // over its budget to reach it would make the fixture about the budget.
    oracle.transition(
        "evict — the sweeper takes a collection and says so",
        &[Part::Record, Part::Event],
        || {
            let bytes = store
                .evict(&id, "the hub is over its output budget")
                .expect("the sweep reads");
            assert!(bytes.is_some(), "the collection was there to take");
        },
    );

    // --- 17. the bytes arrive again (store.rs `readmit_to_the_budget`) ---
    // The mark step 16 left, cleared: a collection whose bytes are back in
    // `files/` is the sweeper's again (ORCH-124). Called under its own name,
    // as `evict` is, because reaching it through a chunk would make the
    // fixture about the streamed carriage instead. **One record write and no
    // event line**: the `outputs_removed` line step 16 appended stays true —
    // the bytes were removed that day — and `JobEventKind` may not grow a
    // variant without making `events.jsonl` undecodable to an earlier hub.
    oracle.transition(
        "readmit_to_the_budget — an evicted collection is countable again",
        &[Part::Record],
        || {
            store
                .readmit_to_the_budget(&id)
                .expect("the mark is cleared");
        },
    );

    drop(watch);

    // The record and the history agree at the end, which is the state D103
    // is about.
    let events = store
        .job_events(&id)
        .expect("the history reads")
        .expect("there is one");
    // Thirteen lines from eleven call sites: `move_to` is reached three times
    // (assigned, running, finished) and the join appends two of its own.
    assert_eq!(
        events.len(),
        13,
        "one line per event append the oracle drove: {events:#?}"
    );
    // The unsynced line is one of the thirteen, and it reads back like any
    // other: step A gave up survival of a power cut, not visibility. E-53
    // asks a reader to be able to follow a job while it runs, and that is
    // served by the page cache, which is what an unsynced append fills.
    assert!(
        events.iter().any(|event| matches!(
            &event.what,
            crate::job::JobEventKind::Waiting { reason }
                if reason == "no runner has the labels this job asks for"
        )),
        "the unsynced waiting line is not readable: {events:#?}"
    );
    let record = store
        .job(&id)
        .expect("the record reads")
        .expect("there is one");
    assert!(record.state.is_terminal());
    // Step 16 took the bytes and step 17 put the collection back into the
    // budget's accounting, so the live mark is gone while the history still
    // carries the line that says the bytes went (ORCH-124).
    assert!(record.outputs.removed.is_none());
    assert!(
        events
            .iter()
            .any(|event| matches!(&event.what, crate::job::JobEventKind::OutputsRemoved { .. })),
        "the eviction is not on the history: {events:#?}"
    );
}

/// The list above is complete only while `store.rs` has the writers it had
/// when it was written.
///
/// This is a **tripwire, not a proof**: it counts the call sites in the source
/// text. A fifteenth `write_record` or a twelfth event appender makes it red,
/// and whoever added it then decides whether the oracle has to cover it. It
/// cannot tell a covered site from an uncovered one, and a site renamed or
/// reached through a new helper would slip past it — which is why it carries
/// the reason in its message rather than a bare number.
///
/// Since D183 step A it counts the event appenders in **two** classes, and
/// the second one is pinned at one. That is the narrowness of step A made
/// mechanical: the door that does not wait for the disk cannot acquire a
/// second user without this test saying so.
#[test]
fn no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it() {
    let source = include_str!("../store.rs");
    assert_eq!(
        source.matches("self.write_record(").count(),
        RECORD_WRITE_SITES,
        "store.rs no longer has {RECORD_WRITE_SITES} record writers. Every one of them is \
         driven by the ordering oracle above; a new one has to be driven too, or named \
         here as deliberately out of scope, before this number moves."
    );
    assert_eq!(
        source.matches("self.append_event(").count(),
        EVENT_APPEND_SITES,
        "store.rs no longer has {EVENT_APPEND_SITES} event appenders. D103 binds every one \
         of them to the record write beside it; a new one has to be driven by the ordering \
         oracle above before this number moves."
    );
    assert_eq!(
        source.matches("self.append_event_unsynced(").count(),
        UNSYNCED_EVENT_APPEND_SITES,
        "store.rs no longer has exactly {UNSYNCED_EVENT_APPEND_SITES} appender that does not \
         wait for the disk. D183 authorises step A for `record_waiting`'s `waiting` line and \
         for nothing else: a `waiting` reason is not a state transition (10 §1 F-1), is \
         already permitted to disagree with the record (F-2), is never read on recovery \
         (F-3) and is rewritten by the next dispatch pass (D171). None of that is true of a \
         transition, a verdict or an attempt, so a second site here is a decision and not a \
         refactor."
    );
    // The two doors together still account for every event line. Counted on
    // the shared prefix, so an appender cannot be added under a third name
    // that neither exact pattern above would catch.
    assert_eq!(
        source.matches("self.append_event").count(),
        EVENT_APPEND_SITES + UNSYNCED_EVENT_APPEND_SITES,
        "store.rs no longer has {} event appenders in total. Step A split them into a synced \
         door and an unsynced one; it did not change how many there are, and a third door \
         would be invisible to the two exact counts above.",
        EVENT_APPEND_SITES + UNSYNCED_EVENT_APPEND_SITES
    );
}
