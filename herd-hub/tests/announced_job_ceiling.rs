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
//! which says `this runner holds at most N job(s) at once` — and that
//! sentence is written verbatim into the record's `attempt_ended` line by
//! `end_attempt`. A timeout, a cancel or a lost runner writes a different
//! sentence and is counted as what it is. [`ceiling_refusals`] is the whole
//! of that distinction, and it matches on
//! `herd_runner::A_RUNNERS_OWN_CEILING` — the runner's constant, not a copy
//! of its words — which is also the clause that tells the runner's sentence
//! apart from the hub's own *"it said it holds at most N at once"*.
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

/// What the **harness's** runner announces, from `support::host()` (64 GiB
/// lent, an 8 GiB floor) and `herd_runner::DEFAULT_MEMORY_MAX` (2 GiB a job):
/// `(65536 - 8192) / 2048`. Deliberately far out of the way of every other
/// test in this suite, and asserted rather than described by
/// [`a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing`].
const THE_HARNESS_CEILING: usize = 28;

/// How many jobs that test submits. The gap between this and
/// [`THE_HARNESS_CEILING`] is its whole premise.
const HOW_MANY_WELL_INSIDE: usize = 4;

/// What each job does. Long enough that the jobs submitted together really do
/// overlap — a queue that drained one job at a time would never test a
/// ceiling — and short enough that the whole file is a few seconds.
const SLEEP_SECONDS: u64 = 1;

/// The phrase the runner's own refusal carries, **taken from the runner
/// rather than copied from it**.
///
/// This is the attribution the oracle turns on, so it cannot be a literal:
/// a literal is a second copy of the runner's wording that nothing keeps in
/// step, and an edit to `herd_runner::jobs_at_once`'s sentence would leave
/// this file matching a phrase no runner says any more — at which point
/// [`ceiling_refusals`] returns nothing for ever and the oracle reads green
/// on a fleet losing jobs.
///
/// It was `"holds at most"` and that was worse than untied: the hub's own
/// sentence ([`THE_HUBS_CEILING_SENTENCE`]) contained the same words, so the
/// substring could not say which side had spoken, which is exactly the
/// attribution this file claims to perform.
const THE_RUNNERS_CEILING_SENTENCE: &str = herd_runner::A_RUNNERS_OWN_CEILING;

/// And the hub's own clause for the same ceiling, likewise taken from the
/// hub. Used where this file wants a sentence the **hub** wrote — a
/// `waiting` line — and never where it wants the runner's.
const THE_HUBS_CEILING_SENTENCE: &str = herd_hub::matcher::A_CEILING_THE_RUNNER_ANNOUNCED;

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

    // **Equality, not `<=`.** A one-sided bound here passes at zero, and zero
    // is what it would read if `harness.state_of` ever stopped returning
    // `Assigned | Running` for a job that is out there — the assertion the
    // whole file is named for would then measure nothing while staying green.
    // The lower half is as certain as the upper one: [`HOW_MANY`] jobs are
    // submitted at once against a ceiling of [`CEILING`], each sleeps
    // [`SLEEP_SECONDS`], and this loop samples every 20 ms, so a run in which
    // the ceiling was never once full is a run in which the queue was not
    // placed at all.
    println!("peak attempts out on the runner at once: {peak_out_there}");
    assert_eq!(
        peak_out_there, CEILING,
        "the hub's busiest moment had {peak_out_there} attempts out on a runner that announced \
         {CEILING}. More means the announcement is recorded and not acted on; fewer means this \
         test never filled the ceiling it claims to measure — a queue of {HOW_MANY} jobs that \
         never once put {CEILING} on the machine was paced by something else, or `state_of` no \
         longer says when a job is out there"
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
        .filter(|reason| reason.contains(THE_HUBS_CEILING_SENTENCE))
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

/// **A queue far inside an announced ceiling costs nothing**: four jobs on
/// the harness runner, which announces [`THE_HARNESS_CEILING`].
///
/// It used to be headed *"a runner that said nothing is placed on exactly as
/// it was"*, and that was not what it did: the runner in it announces 28.
/// **No end-to-end test in this suite exercises `jobs_at_once: None`, and
/// none can** — our runner derives a ceiling from what its own machine
/// declared and always announces one, floored at 1, so there is no way to
/// start a real `herd-runner` that says nothing. A hub speaking to a peer
/// that is silent is covered where it can be: by `herd-proto`'s
/// `round_trip`, which decodes a `main`-shaped hello to `None`, and by the
/// matcher's own unit tests, whose seats announce nothing by default and
/// which include
/// `a_runner_that_announced_no_ceiling_is_placed_on_exactly_as_it_was`.
///
/// What this test is for, now that it says so: a hub that started deriving a
/// ceiling of its own, or reading a *large* announcement as a small number,
/// would pace this queue. The 28 is therefore asserted rather than described
/// — a doc comment naming a figure that the test never reads is the same
/// defect one level up, and the gap between 28 and four is the whole
/// premise.
#[tokio::test(flavor = "multi_thread")]
async fn a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing() {
    // Asserted, not assumed: this is what makes "well inside" true. It is
    // derived from `support::host()` and the harness's own confinement, so a
    // change to either shows up here instead of quietly turning this test
    // into a test of a ceiling that bites.
    let announced = herd_runner::jobs_at_once(&support::host(), herd_runner::DEFAULT_MEMORY_MAX);
    assert_eq!(
        announced.most(),
        THE_HARNESS_CEILING,
        "the harness runner announces {} and this test submits {HOW_MANY_WELL_INSIDE}: if the \
         two ever meet, this stops being a test of a queue nothing paces. Its sentence: {}",
        announced.most(),
        announced.why()
    );
    // "Well inside" is the premise, and it is arithmetic rather than a
    // heading. A `const` block, so the day the two figures meet this file
    // stops compiling instead of quietly testing something else.
    const _: () = assert!(THE_HARNESS_CEILING > HOW_MANY_WELL_INSIDE);

    let harness = Harness::with_hub_and_runner().await;
    let requester = harness.requester();
    let state_dir = harness.state_dir().to_owned();

    let mut submitted = Vec::new();
    for _ in 0..HOW_MANY_WELL_INSIDE {
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
///
/// **What this test used to miss, measured.** It looked for
/// `jobs_at_once = Some(` and `jobs_at_once.unwrap_or(` — an *assignment* and
/// an unwrap. The hub does neither: it builds `Announced` with field-init
/// shorthand (`herd-hub/src/server.rs`), so the spelling a hub would really
/// use to invent a ceiling has a **colon**. The review of PR #82 mutated that
/// line to `jobs_at_once: Some(999),` and this test passed. What it matches
/// now is any line of shipped hub code that puts a `Some(` beside the field
/// name at all, in whatever punctuation, plus the `.or(`/`.unwrap_or(` shapes
/// that fill in a silence — the only lines the shipped hub needs are the
/// field's two declarations and the places it is moved along unchanged, and
/// none of those names `Some`.
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
        // **And it really read the hub.** The split above is on the first
        // `#[cfg(test)]` in the file, which is the tests module today; a
        // single `#[cfg(test)]` helper added near the top of either file
        // would shrink the scanned region to a handful of lines, or to none,
        // and every assertion below would go on passing over nothing. Both
        // files are thousands of lines of shipped code and will stay so while
        // this field exists.
        let scanned = shipped.lines().count();
        assert!(
            scanned > 1000,
            "{name}: only {scanned} lines were scanned for an invented ceiling. The region is \
             everything before the first `#[cfg(test)]`, so a test helper moved up the file \
             empties this guard without failing it"
        );
        let mut found_the_field = false;
        for (number, line) in shipped.lines().enumerate() {
            // Everything before a `//`. This also truncates a `//` inside a
            // string literal, which costs a little reach and never a false
            // failure; there is no such line in either file today.
            let code = line.split("//").next().unwrap_or("");
            for (at, _) in code.match_indices("jobs_at_once") {
                found_the_field = true;
                let rest = &code[at + "jobs_at_once".len()..];
                // What the hub does legitimately: declare the field, read it
                // (`if let Some(at_once) = seat.jobs_at_once`), and move it
                // along unchanged. What it must never do is put a value
                // *into* it. So the shape under test is "the field, then a
                // `:` or an `=`, then something built here" — which is where
                // `Some(` would appear, in whatever punctuation. A `Some(`
                // to the **left** of the field name is a read and is fine.
                let filled_in = rest
                    .strip_prefix(':')
                    .or_else(|| rest.strip_prefix(" ="))
                    .or_else(|| rest.strip_prefix('='))
                    .filter(|after| !after.starts_with('='));
                if let Some(after) = filled_in {
                    assert!(
                        !after.contains("Some("),
                        "{name}:{} fills in a ceiling the runner did not announce: {line}",
                        number + 1
                    );
                }
                // And the silence must not be read as a number by any of the
                // shapes that turn a `None` into one.
                for turning_silence_into_a_figure in [
                    ".unwrap_or(",
                    ".or(",
                    ".map_or(",
                    ".unwrap_or_else(",
                    ".get_or_insert",
                ] {
                    assert!(
                        !rest.starts_with(turning_silence_into_a_figure),
                        "{name}:{} reads a runner's silence as a ceiling: {line}",
                        number + 1
                    );
                }
            }
        }
        // And the field is still named in both files, so a rename that left
        // this test matching a word the hub no longer uses fails here rather
        // than passing silently over code it stopped understanding.
        assert!(
            found_the_field,
            "{name} no longer mentions `jobs_at_once` anywhere in its shipped code: this guard \
             is now watching a field that moved"
        );
    }
}

/// **The runner's ceiling sentence and the hub's cannot be mistaken for each
/// other**, asserted on the two sentences as they are really built.
///
/// [`ceiling_refusals`] is the oracle's whole attribution, and it works by
/// matching words. Both sides have a sentence about this ceiling and both
/// reach the same record: the hub's on a `waiting` line, which costs the job
/// nothing, and the runner's inside an `attempt_ended` line, which spent one
/// of that job's goes. Until this branch both contained *"holds at most"*,
/// so the substring could not say which had spoken. This is the assertion
/// that keeps them apart — against the real sentences, not against the two
/// constants, because a wording edit that reintroduced the ambiguity would
/// leave the constants looking fine.
#[test]
fn the_runners_ceiling_sentence_is_not_the_hubs() {
    let runner = herd_runner::jobs_at_once(
        &herd_proto::HostSize::new(Some(LENT_MIB), Some(4)).with_floor(Some(FLOOR_MIB)),
        PER_JOB_BYTES,
    );
    let runner_said = runner.why().to_owned();
    // A host that says nothing takes the other branch of the same function,
    // and a refusal from such a runner is no less a ceiling refusal.
    let silent_said = herd_runner::jobs_at_once(&herd_proto::HostSize::default(), PER_JOB_BYTES)
        .why()
        .to_owned();
    // The hub's, built the way `admission` builds it. Not the real call —
    // this file already exercises that end to end — but the same constant,
    // which is what the matching turns on.
    let hub_said = format!(
        "on capped-runner, {CEILING} job(s) already out there and {THE_HUBS_CEILING_SENTENCE} \
         {CEILING} at once, so it takes nothing more until one of them ends"
    );

    for said in [&runner_said, &silent_said] {
        assert!(
            said.contains(THE_RUNNERS_CEILING_SENTENCE),
            "every branch of the runner's arithmetic has to carry its own attribution, and \
             this one does not: {said}"
        );
        assert!(
            !said.contains(THE_HUBS_CEILING_SENTENCE),
            "a runner's sentence that contains the hub's clause makes a refusal \
             indistinguishable from a wait: {said}"
        );
    }
    assert!(
        hub_said.contains(THE_HUBS_CEILING_SENTENCE)
            && !hub_said.contains(THE_RUNNERS_CEILING_SENTENCE),
        "and the hub's sentence must not carry the runner's: {hub_said}"
    );
    assert!(
        !THE_RUNNERS_CEILING_SENTENCE.contains(THE_HUBS_CEILING_SENTENCE)
            && !THE_HUBS_CEILING_SENTENCE.contains(THE_RUNNERS_CEILING_SENTENCE),
        "neither clause may be a substring of the other: {THE_RUNNERS_CEILING_SENTENCE:?} and \
         {THE_HUBS_CEILING_SENTENCE:?}"
    );
}

// ---------------------------------------------------------------------------
// The link that drops while the machine is full.
// ---------------------------------------------------------------------------

/// How long the link is down in the test that stays in this suite.
///
/// Longer than [`LOST_AFTER_HERE`], because the whole sequence begins with
/// the hub's watchdog firing. Alexa's condition names **60 s**, which is the
/// deployed `herd_hub::LOST_AFTER`; the figure is a setting
/// (`--lost-after`/`HERD_LOST_AFTER`, `herd-hub/src/main.rs`) and this test
/// drives the same watchdog through it, because a suite that waited out a
/// minute per run would be a suite nobody runs. The 60 s shape was run by
/// hand once and its output is in the pull request; what is kept here is the
/// mechanism, not the clock.
const THE_OUTAGE: Duration = Duration::from_secs(4);

/// The hub's silence threshold for this test, and the cadence its runner
/// really reports at. Three to one, which is `herd_proto::SILENCE_MARGIN`:
/// anything tighter is refused at the handshake, and rightly.
const LOST_AFTER_HERE: Duration = Duration::from_secs(3);
const HEALTH_EVERY_HERE: Duration = Duration::from_secs(1);

/// How long each job runs. Long enough to still be running when the link
/// comes back — which is the whole premise, since the defect is the runner
/// holding jobs the hub has already written off — and short enough that the
/// run is seconds.
const SLEEP_ACROSS_THE_OUTAGE: u64 = 6;

/// How long this one test may take. Its own figure rather than [`PATIENCE`]:
/// an outage, a watchdog, a redial and a whole second go at every job do not
/// fit inside a bound written for a trivial queue, and a test that borrowed
/// that bound would fail for the clock rather than for the defect.
const PATIENCE_ACROSS_AN_OUTAGE: Duration = Duration::from_secs(120);

/// A TCP relay between the runner and the hub that the test can **cut**.
///
/// A real outage, not a simulated one. The sockets are shut down under both
/// peers, every redial while the cut lasts is answered and dropped, and
/// nothing but the clock tells either side what happened. It cannot be done
/// by stopping the hub: the defect needs the runner's jobs to go on running
/// while the hub's watchdog writes their attempts off, so both processes have
/// to stay alive and only the wire between them may go.
///
/// Blocking sockets on their own threads rather than `tokio::io`, because
/// `herd-hub` does not build tokio's `io-util` and a relay is not a reason to
/// widen what the crate compiles.
struct ALinkTheTestCanCut {
    url: String,
    state: std::sync::Arc<std::sync::Mutex<Wire>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct Wire {
    cut: bool,
    open: Vec<std::sync::Arc<std::net::TcpStream>>,
}

impl ALinkTheTestCanCut {
    fn in_front_of(hub_url: &str) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};

        let hub: std::net::SocketAddr = hub_url
            .trim_start_matches("ws://")
            .parse()
            .expect("the harness hands out ws://host:port");
        let listening =
            std::net::TcpListener::bind("127.0.0.1:0").expect("a free loopback port for the relay");
        let addr = listening.local_addr().expect("the relay knows its port");
        listening
            .set_nonblocking(true)
            .expect("a listener can be polled");
        let state = Arc::new(Mutex::new(Wire::default()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || loop {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let from_runner = match listening.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => return,
                };
                if from_runner.set_nonblocking(false).is_err() {
                    continue;
                }
                // Taken before the dial out, so that a cut arriving in the
                // middle of this cannot leave a pair of sockets nobody holds.
                let mut wire = state.lock().unwrap_or_else(|p| p.into_inner());
                if wire.cut {
                    // The outage: the dial is answered by the kernel and then
                    // nothing, which is what a link that is down looks like
                    // from a runner that keeps trying.
                    drop(from_runner);
                    continue;
                }
                let Ok(to_hub) = std::net::TcpStream::connect(hub) else {
                    continue;
                };
                let from_runner = Arc::new(from_runner);
                let to_hub = Arc::new(to_hub);
                wire.open.push(Arc::clone(&from_runner));
                wire.open.push(Arc::clone(&to_hub));
                drop(wire);
                pump(Arc::clone(&from_runner), Arc::clone(&to_hub));
                pump(to_hub, from_runner);
            });
        }
        Self {
            url: format!("ws://{addr}"),
            state,
            stop,
        }
    }

    /// Cut the link: every socket either way is shut down, and every redial
    /// is dropped until [`ALinkTheTestCanCut::restore`].
    fn cut(&self) {
        let mut wire = self.state.lock().unwrap_or_else(|p| p.into_inner());
        wire.cut = true;
        for socket in wire.open.drain(..) {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }

    fn restore(&self) {
        let mut wire = self.state.lock().unwrap_or_else(|p| p.into_inner());
        wire.cut = false;
    }
}

impl Drop for ALinkTheTestCanCut {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.cut();
    }
}

/// One direction of the relay, on its own thread, ending both sockets when
/// either end does.
fn pump(from: std::sync::Arc<std::net::TcpStream>, to: std::sync::Arc<std::net::TcpStream>) {
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = match (&*from).read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            if (&*to).write_all(&buffer[..read]).is_err() {
                break;
            }
        }
        let _ = from.shutdown(std::net::Shutdown::Both);
        let _ = to.shutdown(std::net::Shutdown::Both);
    });
}

/// **A link that drops while the runner is at its ceiling loses no job.**
///
/// Alexa's condition of merge, of 2026-10-25:
///
/// > un test hub réel qui coupe le lien 60 s avec un runner à son plafond,
/// > 0 job mort, 0 job sans verdict.
///
/// The sequence, every step of which was read on this branch before it was
/// written down:
///
/// 1. A runner announcing [`CEILING`] is running [`CEILING`] jobs. It is full
///    on both sides' arithmetic, and the two agree.
/// 2. The link drops. The jobs keep running — `herd-runner`'s own comment is
///    explicit that they hold senders of their own and report on the next
///    link — so the runner's count stays at its ceiling.
/// 3. After `lost_after` the hub's watchdog ends every attempt out on that
///    machine and requeues each job. The hub's count for the runner is now
///    **0**, and the runner's is still [`CEILING`].
/// 4. The runner redials. The hub dispatches to it on the hello, before it
///    has read one frame from it (`herd-hub/src/server.rs`), and nothing
///    avoids the machine a job has just come off.
/// 5. The runner is still holding the originals, so it refuses — and before
///    this branch's fix each refusal spent one of that job's two goes, which
///    was the second and last. Every job ended `lost`, with no verdict, on a
///    link blip and a fleet that was working perfectly.
///
/// The oracle is Alexa's two figures and nothing else: **no job dead, no job
/// without a verdict**. It says nothing about how long the detour took or how
/// many attempts it cost — a job requeued by a watchdog has genuinely lost an
/// attempt to a runner that went away, which is E-21 working — only that the
/// work came back with an answer.
/// A hub whose watchdog a test can outlast, a runner holding [`CEILING`],
/// and a link between them the test can cut.
///
/// `policy` is the hub's, already carrying this file's timings, so a caller
/// has only to change what its own case is about.
async fn a_capped_runner_behind_a_cuttable_link(
    label: &'static str,
    policy: herd_hub::Policy,
) -> (
    Harness,
    ALinkTheTestCanCut,
    DataRootDir,
    tokio::task::JoinHandle<()>,
) {
    let harness = Harness::with_hub_timed(policy).await;
    let link = ALinkTheTestCanCut::in_front_of(&harness.hub_url);
    let data_root = DataRootDir::new(label);
    let mut confinement = herd_runner::Confinement::with_data_root(data_root.path());
    confinement.memory_max = PER_JOB_BYTES;
    let config = herd_runner::Config {
        // Through the relay, which is the only thing this test adds to an
        // ordinary runner.
        hub_url: link.url.clone(),
        name: Some("capped-runner".to_owned()),
        token: Some(harness.approve("capped-runner")),
        confinement,
        host: herd_proto::HostSize::new(Some(LENT_MIB), Some(4)).with_floor(Some(FLOOR_MIB)),
        health_every: HEALTH_EVERY_HERE,
        // So that the redial after the outage is prompt. The shape under test
        // is what the hub does with a runner that has just come back, not how
        // long a runner waits before coming back.
        redial: herd_runner::Backoff {
            first: Duration::from_millis(100),
            cap: Duration::from_millis(400),
            ..herd_runner::Backoff::default()
        },
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
    (harness, link, data_root, running)
}

/// This file's timings, as a hub policy a caller can go on changing.
fn a_watchdog_a_test_can_outlast() -> herd_hub::Policy {
    herd_hub::Policy {
        lost_after: LOST_AFTER_HERE,
        health_every: HEALTH_EVERY_HERE,
        ..herd_hub::Policy::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_cut_while_the_runner_is_full_loses_no_job() {
    let (harness, link, _data_root, running) = a_capped_runner_behind_a_cuttable_link(
        "link-cut-at-the-ceiling",
        a_watchdog_a_test_can_outlast(),
    )
    .await;

    let requester = harness.requester();
    let state_dir = harness.state_dir().to_owned();
    // Exactly as many jobs as the machine holds: the premise is a runner **at
    // its ceiling**, not one with a queue behind it. Each command distinct,
    // because two submissions of one command are one job (E-05).
    let mut submitted = Vec::new();
    for n in 0..CEILING {
        submitted.push(
            support::submit_job(
                &requester,
                support::shell(&format!(
                    "echo across the outage {n}; sleep {SLEEP_ACROSS_THE_OUTAGE}"
                )),
            )
            .await
            .expect("the hub accepts the submission"),
        );
    }

    // Waited for, never slept over: the cut has to land on a machine that is
    // really full, and a sleep that ended early would cut a link with nothing
    // on it.
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
        if out_there == CEILING {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "only {out_there} of {CEILING} jobs reached the runner, so the link would be cut \
             on a machine that was not at its ceiling"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // ---- The outage.
    println!("cutting the link for {THE_OUTAGE:?}, with the runner at its ceiling of {CEILING}");
    link.cut();
    tokio::time::sleep(THE_OUTAGE).await;
    // The hub must really have given up on them, or this test cut a link and
    // measured nothing: the whole sequence begins with the watchdog.
    let written_off = submitted
        .iter()
        .filter(|id| !matches!(harness.state_of(id), herd_hub::JobState::Running))
        .count();
    assert_eq!(
        written_off, CEILING,
        "after {THE_OUTAGE:?} with no link and a silence threshold of {LOST_AFTER_HERE:?}, the \
         hub's watchdog has to have written off every attempt out on that machine; {written_off} \
         of {CEILING} were. Without that this test never reaches the defect it is named for"
    );
    link.restore();
    println!("the link is back; the runner is still running its {CEILING} original job(s)");

    // ---- Alexa's oracle, and nothing else.
    let deadline = Instant::now() + PATIENCE_ACROSS_AN_OUTAGE;
    loop {
        let unfinished = submitted
            .iter()
            .filter(|id| !harness.state_of(id).is_terminal())
            .count();
        if unfinished == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{unfinished} of {CEILING} jobs never ended at all inside \
             {PATIENCE_ACROSS_AN_OUTAGE:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let mut dead: Vec<String> = Vec::new();
    for id in &submitted {
        let record = record_of(&state_dir, id);
        let outcome = &record["state"]["outcome"];
        if outcome["type"] != "exited" || outcome["code"] != 0 {
            dead.push(format!("{id}: {outcome}"));
        }
    }
    assert!(
        dead.is_empty(),
        "0 job dead and 0 job without a verdict is the condition this change merges under, and \
         {} of {CEILING} came back without one: {dead:?}. Every one of these was a runnable job \
         on a healthy machine; what killed it was a link blip and a refusal that spent its last \
         go",
        dead.len()
    );
    println!("every one of the {CEILING} jobs came back with its own verdict");

    running.abort();
    let _ = running.await;
    drop(link);
    drop(harness);
}

/// **The exemption is bounded, and the bound says so on the record**
/// (D-NEXT).
///
/// A refusal at a machine's own ceiling spends none of the job's goes at a
/// runner that went away — that is the test above. An exemption with no
/// bound at all would be a job that can be handed back for ever with nobody
/// told, which is the one outcome worse than a job that ends. So it has a
/// budget of its own, `herd_hub::CEILING_REFUSALS_ALLOWED`, and this is what
/// happens when a job reaches it.
///
/// Driven at a budget of **one** rather than by provoking eight refusals: a
/// test that spent the real figure would be testing the number instead of
/// the behaviour, and the number is a setting precisely so that a test does
/// not have to. The same outage as above, and the one refusal it produces is
/// the whole budget.
#[tokio::test(flavor = "multi_thread")]
async fn a_job_refused_past_the_bound_ends_and_names_it() {
    let (harness, link, _data_root, running) = a_capped_runner_behind_a_cuttable_link(
        "link-cut-bound",
        herd_hub::Policy {
            ceiling_refusals_allowed: 1,
            ..a_watchdog_a_test_can_outlast()
        },
    )
    .await;
    let requester = harness.requester();
    let state_dir = harness.state_dir().to_owned();

    let mut submitted = Vec::new();
    for n in 0..CEILING {
        submitted.push(
            support::submit_job(
                &requester,
                support::shell(&format!(
                    "echo at the bound {n}; sleep {SLEEP_ACROSS_THE_OUTAGE}"
                )),
            )
            .await
            .expect("the hub accepts the submission"),
        );
    }
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
        if out_there == CEILING {
            break;
        }
        assert!(Instant::now() < deadline, "the runner never filled up");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    link.cut();
    tokio::time::sleep(THE_OUTAGE).await;
    link.restore();

    let deadline = Instant::now() + PATIENCE_ACROSS_AN_OUTAGE;
    while submitted
        .iter()
        .any(|id| !harness.state_of(id).is_terminal())
    {
        assert!(Instant::now() < deadline, "the jobs never ended at all");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // At least one of them met the bound, and what it was told is the point:
    // not "a runner went away", which would be false and would send an
    // operator looking at a machine that never failed, but the budget, the
    // figure, and the machine whose count never agreed with the hub's.
    let ended_at_the_bound: Vec<String> = submitted
        .iter()
        .map(|id| record_of(&state_dir, id))
        .filter(|record| record["state"]["outcome"]["type"] == "lost")
        .filter_map(|record| {
            record["state"]["outcome"]["reason"]
                .as_str()
                .map(str::to_owned)
        })
        .filter(|reason| reason.contains("as full as the ceiling they announced"))
        .collect();
    assert!(
        !ended_at_the_bound.is_empty(),
        "with a budget of one and an outage that produces a refusal, a job has to reach the \
         bound; what the {CEILING} jobs ended as: {:?}",
        submitted
            .iter()
            .map(|id| record_of(&state_dir, id)["state"]["outcome"].clone())
            .collect::<Vec<_>>()
    );
    let said = ended_at_the_bound.first().expect("at least one");
    assert!(
        said.contains("1 time(s) of the 1 allowed"),
        "the sentence has to carry both figures, or it is a job told it is over and not why: \
         {said}"
    );
    assert!(
        said.contains("capped-runner"),
        "and name the machine whose count never agreed: {said}"
    );
    assert!(
        said.contains("None of those refusals spent"),
        "and say that the goes it still had were not what ran out: {said}"
    );
    println!("what a job that reaches the bound is told:\n  {said}");

    running.abort();
    let _ = running.await;
    drop(link);
    drop(harness);
}
