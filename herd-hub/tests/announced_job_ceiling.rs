//! **The hub honours the ceiling a runner announced** (D-NEXT, invariant 2).
//!
//! C5 gave the runner a ceiling of its own and a way to refuse past it, and
//! that half is proved by `fake_hub_flood.rs`: a hand-written server floods a
//! real runner with 250 assignments and the runner holds 8 and hands 242
//! back. **That file must go on showing 242.** Its "hub" misbehaves on
//! purpose and will never read anything a runner announces; what it proves is
//! that the runner protects itself *whatever the hub does*, which is the
//! rollback case and is not what this file is about.
//!
//! What C5 left open is the cost of those 242. A refusal is a
//! `ToHub::AttemptAbandoned`, the hub requeues it through
//! `give_the_job_another_go`, and **each one spends one of that job's goes**
//! against `attempts_allowed`. On the real hub that cost is reachable in
//! ordinary operation, because the memory admission charges a job only what
//! it *declared* (`herd-hub/src/matcher.rs`, `needs_memory` of `None` costs
//! zero), so a queue of jobs that declare no memory need costs a machine
//! nothing and is placed on one runner without bound.
//!
//! So the oracle here is Alexa's, and it is not "no attempt is ever spent":
//!
//! > essais dépensés pour cause de plafond runner = 0, autres essais
//! > inchangés
//!
//! **Attempts spent because of the runner's ceiling: zero. Every other
//! attempt unchanged.** The two are told apart by *attribution*, never by a
//! total that happens to be zero: a refusal for the ceiling carries the
//! runner's own sentence — the one built by `herd_runner::jobs_at_once`,
//! which says `holds at most N job(s) at once` — and that sentence is written
//! verbatim into the record's `attempt_ended` line by `end_attempt`. A
//! timeout, a cancel or a lost runner writes a different sentence and is
//! counted as what it is. [`ceiling_refusals`] is the whole of that
//! distinction.
//!
//! The surplus is expected to **wait**, not to be sent and handed back, and
//! what it is told is asserted too: a `waiting` line, which is a
//! `JobEventKind` that already existed. **Nothing new was added to that
//! enum.** It is `#[serde(tag = "event")]` with no `#[serde(other)]` arm, so
//! a new variant would make `events.jsonl` unreadable to an earlier hub —
//! worse than a lost field, and outside what D-NEXT approved.

mod support;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use herd_hub::job::RECORD_VERSION;
use herd_proto::{JobId, PROTOCOL_VERSION};
use support::{events_of, record_of, DataRootDir, Harness, PATIENCE};

/// The machine this file's runner says it is, and the cap it says it puts on
/// every job. **Declared, never read off the host this suite runs on**: the
/// ceiling is derived from these two figures, so a runner left to read the
/// real machine would announce 28 on the development server and 6 on a 16 GiB
/// CI box, and this test would be asserting something different on each.
///
/// Four gibibytes lent, two kept as the floor, one gibibyte a job:
/// `(4096 - 2048) / 1024` is [`CEILING`], and nothing else, anywhere.
const LENT_MIB: u64 = 4 * 1024;
const FLOOR_MIB: u64 = 2 * 1024;
const PER_JOB_BYTES: u64 = 1024 << 20;
const CEILING: usize = 2;

/// How many jobs are submitted at once. More than [`CEILING`], by enough that
/// an unbounded hub would be plainly unbounded: without the hub half, all
/// [`HOW_MANY`] are assigned in one pass and `HOW_MANY - CEILING` of them
/// come straight back as abandoned attempts.
const HOW_MANY: usize = 8;

/// What each job does. Long enough that the jobs submitted together really do
/// overlap — a queue that drained one job at a time would never test a
/// ceiling — and short enough that the whole file is a few seconds.
const SLEEP_SECONDS: u64 = 1;

/// The phrase the runner's own refusal carries, from
/// `herd_runner::jobs_at_once`'s sentence. This is the attribution the oracle
/// turns on, so it is a constant with the reason beside it rather than a
/// string literal buried in an assertion.
const THE_RUNNERS_CEILING_SENTENCE: &str = "holds at most";

/// Neither version number moves, which is the condition D-NEXT shipped under.
///
/// Asserted rather than described, because both are compatibility statements
/// about somebody else's machine: a handshake at protocol version 3 refuses
/// every runner that has not been restarted, and a hub at record version 2
/// refuses an operator's entire state root. The field this file is about is
/// `#[serde(default)]` on one frame and means, when absent, exactly what its
/// default says — which is the one shape that leaves both alone.
#[test]
fn neither_version_number_moves() {
    assert_eq!(PROTOCOL_VERSION, 2, "the handshake is unchanged");
    assert_eq!(RECORD_VERSION, 1, "every existing state root still reads");
}

/// A hub with one runner that announces a ceiling of [`CEILING`].
///
/// Built here rather than taken from the harness because the harness runner
/// declares 64 GiB and an 8 GiB floor deliberately — a ceiling that is out of
/// the way of every other test in this suite. This one needs a ceiling that
/// bites.
async fn a_hub_and_a_runner_holding_two() -> (Harness, DataRootDir, tokio::task::JoinHandle<()>) {
    let harness = Harness::with_hub().await;
    let data_root = DataRootDir::new("announced-job-ceiling");
    let mut confinement = herd_runner::Confinement::with_data_root(data_root.path());
    confinement.memory_max = PER_JOB_BYTES;
    let config = herd_runner::Config {
        hub_url: harness.hub_url.clone(),
        name: Some("capped-runner".to_owned()),
        token: Some(harness.approve("capped-runner")),
        confinement,
        host: herd_proto::HostSize::new(Some(LENT_MIB), Some(4)).with_floor(Some(FLOOR_MIB)),
        ..herd_runner::Config::default()
    };
    let running = tokio::spawn(async move {
        let _ = herd_runner::run(config).await;
    });
    let deadline = Instant::now() + PATIENCE;
    while !harness.runner_joined() {
        assert!(Instant::now() < deadline, "the runner never joined the hub");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (harness, data_root, running)
}

/// How many of this job's attempts ended **because of the runner's ceiling**,
/// read off its own record.
///
/// The attribution, and the only thing in this file that decides which
/// attempts the oracle counts. The runner's refusal sentence travels
/// unchanged: it is built by `herd_runner::jobs_at_once`, carried in
/// `ToHub::AttemptAbandoned`'s `reason`, handed to `give_the_job_another_go`
/// and written verbatim by `end_attempt` into the `attempt_ended` line. An
/// attempt that ended for a timeout, a cancel or a runner going silent
/// carries one of the hub's own sentences and is not one of these — which is
/// the point: "unchanged" cannot be asserted by a count of everything.
fn ceiling_refusals(state_dir: &std::path::Path, job_id: &JobId) -> Vec<String> {
    events_of(state_dir, job_id)
        .into_iter()
        .filter(|event| event["event"] == "attempt_ended")
        .filter_map(|event| event["reason"].as_str().map(str::to_owned))
        .filter(|reason| reason.contains(THE_RUNNERS_CEILING_SENTENCE))
        .collect()
}

/// Every `waiting` line on this job's record, in the order they were written.
fn waiting_reasons(state_dir: &std::path::Path, job_id: &JobId) -> Vec<String> {
    events_of(state_dir, job_id)
        .into_iter()
        .filter(|event| event["event"] == "waiting")
        .filter_map(|event| event["reason"].as_str().map(str::to_owned))
        .collect()
}

/// How many attempts this job was given in all, from its record.
fn attempts_of(state_dir: &std::path::Path, job_id: &JobId) -> usize {
    record_of(state_dir, job_id)["attempts"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0)
}

/// **The oracle.** Eight memory-silent jobs, a runner that announced it holds
/// two, and the figure that matters is the one the hub never spent.
///
/// Memory-silent on purpose: `herdctl::Declared::default()` declares no
/// memory need, so `admission` costs each of these jobs at zero and the
/// memory seat accounting can never be what bounds them. The announced
/// ceiling is the only thing in the hub that can, and this test fails on the
/// code as it stood before D-NEXT — the hub assigns all eight in one pass, the
/// runner takes two and hands six back, and six attempts are spent on
/// refusals that were certain before they were sent.
#[tokio::test(flavor = "multi_thread")]
async fn no_attempt_is_spent_on_a_ceiling_the_hub_was_told_about() {
    let (harness, _data_root, running) = a_hub_and_a_runner_holding_two().await;
    let requester = harness.requester();
    let state_dir = harness.state_dir().to_owned();

    // **Each command is distinct.** Two submissions of the same command join
    // one job rather than minting two (E-05), so a queue of identical sleeps
    // is a queue of one job wearing eight requesters — which paces itself
    // perfectly and would prove nothing at all. The echo is what makes them
    // eight pieces of work.
    let mut submitted = Vec::new();
    for n in 0..HOW_MANY {
        submitted.push(
            support::submit_job(
                &requester,
                support::shell(&format!("echo job {n}; sleep {SLEEP_SECONDS}")),
            )
            .await
            .expect("the hub accepts the submission"),
        );
    }
    assert_eq!(
        submitted.iter().collect::<BTreeSet<_>>().len(),
        HOW_MANY,
        "eight distinct jobs, not one job joined eight times (E-05)"
    );

    // Watched while it runs, not only afterwards: the figure under test is a
    // *peak*, and a run that ended correctly says nothing about how many
    // attempts were in the air at its busiest moment.
    let mut peak_out_there = 0usize;
    let deadline = Instant::now() + PATIENCE;
    loop {
        let out_there = submitted
            .iter()
            .filter(|id| {
                matches!(
                    harness.state_of(id),
                    herd_hub::JobState::Assigned | herd_hub::JobState::Running
                )
            })
            .count();
        peak_out_there = peak_out_there.max(out_there);
        let finished = submitted
            .iter()
            .filter(|id| harness.state_of(id).is_terminal())
            .count();
        if finished == HOW_MANY {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "only {finished} of {HOW_MANY} jobs ended inside {PATIENCE:?}; a ceiling that \
             stopped the queue instead of pacing it would look exactly like this"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert!(
        peak_out_there <= CEILING,
        "the hub had {peak_out_there} attempts out on a runner that announced {CEILING}: the \
         announcement is recorded and not acted on"
    );

    // ---- The oracle, both halves.
    let mut refusals: Vec<String> = Vec::new();
    let mut total_attempts = 0usize;
    for id in &submitted {
        refusals.extend(ceiling_refusals(&state_dir, id));
        total_attempts += attempts_of(&state_dir, id);
    }
    assert!(
        refusals.is_empty(),
        "attempts spent because of the runner's ceiling must be 0, and {} were spent: {:?}",
        refusals.len(),
        refusals
    );
    assert_eq!(
        total_attempts, HOW_MANY,
        "and every other attempt is unchanged: {HOW_MANY} jobs, one go each, which is what a \
         queue this size costs when nothing goes wrong"
    );
    for id in &submitted {
        let record = record_of(&state_dir, id);
        assert_eq!(record["state"]["type"], "finished", "{id} did not finish");
        assert_eq!(
            record["state"]["outcome"],
            serde_json::json!({"type": "exited", "code": 0}),
            "{id} ran, and ran to its own verdict rather than to a refusal"
        );
    }

    // ---- And what the surplus was told while it waited.
    let said: Vec<String> = submitted
        .iter()
        .flat_map(|id| waiting_reasons(&state_dir, id))
        .filter(|reason| reason.contains("holds at most"))
        .collect();
    assert!(
        !said.is_empty(),
        "with eight jobs and a ceiling of two, some of them have to have been told they were \
         waiting for the ceiling — a queue that moved silently would leave an operator with \
         nothing to read. All the waiting lines written: {:?}",
        submitted
            .iter()
            .flat_map(|id| waiting_reasons(&state_dir, id))
            .collect::<Vec<_>>()
    );
    let one = said.first().expect("at least one");
    assert!(
        one.contains("capped-runner") && one.contains("at most 2 at once"),
        "the sentence has to name the machine and both figures: {one}"
    );
    assert!(
        one.contains("until one of them ends"),
        "and say what ends the wait, or it is a job told it is stuck: {one}"
    );
    println!("what a queued job is told behind the ceiling:\n  {one}");

    // And no `events.jsonl` line anywhere in this run is of a kind an older
    // hub could not read. `JobEventKind` is `#[serde(tag = "event")]` with no
    // `#[serde(other)]`, so this is not a style check: a kind outside this
    // set would make the whole file undecodable to a hub rolled back.
    let known: BTreeSet<&str> = [
        "transition",
        "attempt_started",
        "attempt_ended",
        "log_gap",
        "joined",
        "priority_raised",
        "waiting",
        "outputs_removed",
    ]
    .into_iter()
    .collect();
    for id in &submitted {
        for event in events_of(&state_dir, id) {
            let kind = event["event"].as_str().expect("every line names its kind");
            assert!(
                known.contains(kind),
                "{id} recorded a {kind:?} line: a new JobEventKind variant makes events.jsonl \
                 unreadable to an earlier hub, which is outside what D-NEXT approved"
            );
        }
    }

    running.abort();
    let _ = running.await;
    drop(harness);
}

/// **A runner that said nothing is placed on exactly as it was.**
///
/// The other side of "absent means it did not say". This runner is the
/// harness's own, which announces a ceiling of 28 — far above anything this
/// test submits — so the ceiling cannot be what paces it, and the hub places
/// the whole queue at once exactly as it did before D-NEXT.
///
/// Its value is as a regression: a hub that had started *deriving* a ceiling
/// for itself, or reading silence as a small number, would pace this queue
/// too, and the field's whole contract would be broken in the one direction
/// no runner can report.
#[tokio::test(flavor = "multi_thread")]
async fn a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing() {
    let harness = Harness::with_hub_and_runner().await;
    let requester = harness.requester();
    let state_dir = harness.state_dir().to_owned();

    let mut submitted = Vec::new();
    for _ in 0..4 {
        submitted.push(
            support::submit_job(&requester, support::shell("true"))
                .await
                .expect("the hub accepts the submission"),
        );
    }
    let deadline = Instant::now() + PATIENCE;
    while submitted
        .iter()
        .any(|id| !harness.state_of(id).is_terminal())
    {
        assert!(Instant::now() < deadline, "the queue never drained");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for id in &submitted {
        assert_eq!(record_of(&state_dir, id)["state"]["type"], "finished");
        assert_eq!(
            attempts_of(&state_dir, id),
            1,
            "one go each, and no ceiling anywhere near this queue"
        );
        assert!(
            ceiling_refusals(&state_dir, id).is_empty(),
            "{id} was refused for a ceiling it was nowhere near"
        );
    }
}

/// The announcement is derived from what the runner declared, and it is the
/// **same figure the runner enforces**.
///
/// Not a second arithmetic: `serve_once` computes the ceiling once, sends
/// that value in the hello and hands the same `Arc` to `serve_hub`, which is
/// where every assignment is held against it. This asserts the derivation the
/// hub is being told about, on the two machines the first consumer has and on
/// the figures this file's runner uses, so that a change to either side shows
/// up as a failure here and not as a fleet quietly paced wrong.
#[test]
fn the_figure_announced_is_the_one_the_runner_derives() {
    let this_file = herd_proto::HostSize::new(Some(LENT_MIB), Some(4)).with_floor(Some(FLOOR_MIB));
    assert_eq!(
        herd_runner::jobs_at_once(&this_file, PER_JOB_BYTES).most(),
        CEILING
    );
    // And a host that will not say how big it is announces one, never a
    // guess and never zero (D23's rule).
    let silent = herd_proto::HostSize::default();
    assert_eq!(
        herd_runner::jobs_at_once(&silent, PER_JOB_BYTES).most(),
        1,
        "an unreadable figure is not read as plenty any more than it is read as zero"
    );
}

/// Nothing in this change let the hub invent a ceiling for itself.
///
/// The one thing a hub must not do with this field is derive it: the runner's
/// arithmetic uses the cap it puts on every job, which no frame carries, so a
/// figure the hub worked out from `host` alone would be a ceiling the runner
/// never agreed to and could not be held to. Asserted by reading the hub's
/// source, in the manner of `no_line_of_this_module_weighs_a_load_against_anything`.
#[test]
fn the_hub_never_derives_a_ceiling_of_its_own() {
    let matcher = include_str!("../src/matcher.rs");
    let server = include_str!("../src/server.rs");
    for (name, source) in [("matcher.rs", matcher), ("server.rs", server)] {
        // The hub's own code, not the tests under it: a test builds seats
        // with ceilings on them on purpose, and it is the shipped code that
        // must never fill one in.
        let shipped = source
            .split_once("#[cfg(test)]")
            .map(|(before, _)| before)
            .unwrap_or(source);
        for (number, line) in shipped.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !code.contains("jobs_at_once = Some(") && !code.contains("jobs_at_once.unwrap_or("),
                "{name}:{} fills in a ceiling the runner did not announce: {line}",
                number + 1
            );
        }
    }
}
