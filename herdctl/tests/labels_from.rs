//! A command whose requirements are read on a machine, end to end: the real
//! `herdctl` in the fixture consumer's checkout, a real hub, a real runner,
//! and the reading really running as a job of its own (D74, M10).
//!
//! What D74 asks for, and what each test here is:
//!
//! * the reading is **an ordinary job on a runner** — the one queue, the
//!   runner's caps, its own record with its own four executor steps — and the
//!   real job is then placed on the requirements it produced, which are on its
//!   record for anyone to read (E-15, E-36);
//! * the answer is **cached by (commit, command, args)**, so a second
//!   submission of those three pays no reading, and a submission of another
//!   commit pays one;
//! * the answer is read **strictly**, like the capability file (D60): an
//!   unknown key, a missing key, a duplicated key or a figure that is not a
//!   figure each **refuse the real job by name**, and it is never placed;
//! * a reading that exits non-zero, or that its cap stops, refuses the real
//!   job by name too — a job is never placed on a guess;
//! * and there is **no recursion**: the job that does the reading carries no
//!   reading of its own.
//!
//! Nothing here reads a figure or a label out of a test file: every one of
//! them comes from the fixture's own script, through the constants beside it,
//! so a test cannot agree with a wrong answer. And no consumer command is
//! named here or in the fixture: every name is invented (E-11).

use std::process::Output;
use std::time::Duration;

mod fixture;

use fixture::{
    Fixture, Harness, BRANCH, PROBE, PROBE_DECLARES_MEMORY_MIB, READ_LABELS, READ_MEMORY_MIB,
    READ_SCENARIOS, READ_TMPFS_MIB, REFUSES_CODE, RUNNER, SKETCH,
};

/// How long a reading may take in the tests that are about a reading
/// overrunning its cap. Short, and through the hub's own figure (D74), so the
/// test makes five minutes pass in two seconds rather than waiting.
const A_SHORT_READING_CAP: Duration = Duration::from_secs(2);

fn status_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("herdctl itself was killed: {:?}", output.status))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Submit the fixture's reading command, with the answer its script should
/// give, and wait for the end of it.
///
/// `--answer <mode>` travels in the command's own arguments, so it reaches the
/// reading the way a requester's argument does — and it makes each case its
/// own (commit, command, args), which is the cache key (D74).
async fn probe_answering(harness: &Harness, fixture: &Fixture, mode: &str) -> Output {
    harness
        .herdctl_in(
            fixture,
            &[
                "run",
                "--command",
                PROBE,
                "--commit",
                BRANCH,
                "--",
                "--answer",
                mode,
            ],
        )
        .await
}

/// The records of the two jobs a first submission of a reading command makes:
/// the job the requester asked for, and the reading that was run for it.
///
/// Told apart by the record itself rather than by their order: the one that
/// carries a `reading` is the job that waited, and the one its `by` names is
/// the reading.
fn the_job_and_its_reading(harness: &Harness) -> (serde_json::Value, serde_json::Value) {
    let records = harness.records();
    let waited = records
        .iter()
        .find(|record| !record["reading"].is_null())
        .unwrap_or_else(|| panic!("no record carries a reading: {records:#?}"))
        .clone();
    let by = waited["reading"]["by"]
        .as_str()
        .unwrap_or_else(|| panic!("the record names no reading job: {waited:#}"))
        .to_owned();
    let reading = records
        .iter()
        .find(|record| record["id"] == serde_json::json!(by))
        .unwrap_or_else(|| panic!("{by} has no record: {records:#?}"))
        .clone();
    (waited, reading)
}

/// D74's shape, whole: the reading is a job, it is recorded like one, and the
/// job that waited for it is placed on what it answered.
#[tokio::test]
async fn a_reading_is_an_ordinary_job_and_the_real_job_is_placed_on_its_answer() {
    let Some(fixture) =
        Fixture::new("a_reading_is_an_ordinary_job_and_the_real_job_is_placed_on_its_answer")
    else {
        return;
    };
    let harness = Harness::new().await;
    let commit = fixture.rev_parse(BRANCH);

    let output = probe_answering(&harness, &fixture, fixture::ANSWER_GOOD).await;
    assert_eq!(
        status_code(&output),
        0,
        "the job the reading was run for must have run: {}",
        stderr(&output)
    );
    assert!(
        stdout(&output).contains(&format!("grove {PROBE}: verdict 0")),
        "the real job's own verdict must reach the caller: {}",
        stdout(&output)
    );

    // Two jobs and no more: the one that was asked for, and one reading.
    let records = harness.records();
    assert_eq!(
        records.len(),
        2,
        "a submission of a reading command makes the job and one reading: {records:#?}"
    );
    let (waited, reading) = the_job_and_its_reading(&harness);

    // The reading is a job: its own record, its own id, its own four
    // timestamped executor steps, and its own verdict (D74, D77).
    assert_eq!(
        reading["command"][0].as_str(),
        Some(
            format!(
                "sh tools/herd/labels.sh {commit} '--answer' '{}'",
                fixture::ANSWER_GOOD
            )
            .as_str()
        ),
        "the reading's command is the declared line, with the commit and the arguments \
         filled: {reading:#}"
    );
    assert_eq!(
        reading["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0}),
        "the reading ran and came off: {reading:#}"
    );
    let steps: Vec<&str> = reading["attempts"][0]["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("the reading's attempt has no steps: {reading:#}"))
        .iter()
        .map(|step| step["step"].as_str().expect("a step name"))
        .collect();
    assert_eq!(
        steps,
        vec!["up", "fetch", "run", "down"],
        "a reading runs through the four executor steps like any other declared job: \
         {reading:#}"
    );
    for step in reading["attempts"][0]["steps"]
        .as_array()
        .expect("the steps")
    {
        assert!(
            step["started_unix_ms"].as_i64().is_some_and(|at| at > 0),
            "a step line without a timestamp: {step}"
        );
    }
    // It ran on a runner, not in the hub, and the record says which (E-36).
    assert_eq!(
        reading["attempts"][0]["runner"].as_str(),
        Some(RUNNER),
        "the reading ran on the machine the hub chose: {reading:#}"
    );
    // It was confined by a cap of the hub's, not by the hours the command
    // declares for its own tests (D74, D108).
    assert_eq!(
        reading["timeout_s"].as_u64(),
        Some(herd_hub::READING_CAP.as_secs()),
        "the reading is capped by the hub's own figure for a reading: {reading:#}"
    );
    // D74's third limit, kept structurally: the job that does the reading
    // carries no reading of its own, so nothing it reaches can resolve one.
    assert!(
        reading["reading"].is_null(),
        "a command reached through labels_from must never itself resolve one: {reading:#}"
    );
    // And it is placed against the command's **declared** labels — the
    // baseline — rather than against the labels it is about to answer with.
    assert_eq!(
        reading["labels"],
        serde_json::json!(["grove-toolchain"]),
        "the reading is placed against the baseline the declaration states: {reading:#}"
    );

    // The answer is on the record of the job that waited, with where it came
    // from (E-15, E-54).
    assert_eq!(
        waited["reading"]["answer"],
        serde_json::json!({
            "labels": READ_LABELS,
            "memory_mib": READ_MEMORY_MIB,
            "tmpfs_mib": READ_TMPFS_MIB,
            "scenarios": READ_SCENARIOS,
        }),
        "what the reading answered must be on the record it was read for: {waited:#}"
    );
    let from = waited["reading"]["from"].as_str().unwrap_or_else(|| {
        panic!("the record does not say where the answer came from: {waited:#}")
    });
    assert!(
        from.contains(reading["id"].as_str().expect("the reading's id")),
        "the record must say which job read it: {from:?}"
    );

    // And the job was really placed on it: the line explaining the placement
    // names the labels the reading answered — which are not the baseline it
    // was submitted with — and the memory figure the reading gave, which is
    // not the one the command declares.
    let why = waited["attempts"][0]["why"]
        .as_str()
        .unwrap_or_else(|| panic!("the job was never placed: {waited:#}"));
    assert!(
        why.contains(&READ_LABELS.join(", ")),
        "the placement must be on the labels the reading answered: {why:?}"
    );
    assert!(
        why.contains(&herd_proto::size_mib(READ_MEMORY_MIB))
            && !why.contains(&herd_proto::size_mib(PROBE_DECLARES_MEMORY_MIB)),
        "the job must be admitted against the figure the reading gave ({}), not the one its \
         command declares ({}): {why:?}",
        herd_proto::size_mib(READ_MEMORY_MIB),
        herd_proto::size_mib(PROBE_DECLARES_MEMORY_MIB)
    );

    // The reading's own output is kept, as any job's is (E-15): the answer it
    // printed is in its log, and so is the executor's own chatter around it.
    let log = harness.log(reading["id"].as_str().expect("the reading's id"));
    assert!(
        log.contains(&format!("memory_mib={READ_MEMORY_MIB}")),
        "the reading's answer must be in its own log: {log:?}"
    );
    assert!(
        log.contains(&format!("grove: at {commit}")),
        "the reading really fetched the commit it was read for: {log:?}"
    );
}

/// D74's cache: the answer is a property of (commit, command, args), so a
/// second submission of those three pays no reading — and a submission naming
/// another commit pays one.
///
/// The absence of a second reading is **asserted**, not inferred from how long
/// anything took: the hub's own state root is read, and a reading is a job
/// directory there or it is not.
#[tokio::test]
async fn an_answer_is_cached_by_commit_command_and_arguments() {
    let Some(fixture) = Fixture::new("an_answer_is_cached_by_commit_command_and_arguments") else {
        return;
    };
    let harness = Harness::new().await;

    // One submission, which pays for a reading.
    let first = probe_answering(&harness, &fixture, fixture::ANSWER_GOOD).await;
    assert_eq!(status_code(&first), 0, "{}", stderr(&first));
    assert_eq!(
        readings_run(&harness),
        1,
        "the first submission pays for one reading: {:#?}",
        harness.records()
    );

    // And a second of the same command, commit and arguments, which pays for
    // none. The first job is over by now — `herdctl run` waits for it — so
    // this is a new job and not E-05 joining an open one.
    let second = probe_answering(&harness, &fixture, fixture::ANSWER_GOOD).await;
    assert_eq!(status_code(&second), 0, "{}", stderr(&second));
    let records = harness.records();
    assert_eq!(
        records.len(),
        3,
        "the second submission makes one job and no reading: {records:#?}"
    );
    assert_eq!(
        readings_run(&harness),
        1,
        "a second submission of one (commit, command, args) must pay no second reading: \
         {records:#?}"
    );
    // The new job carries the same answer, out of the cache, and says so:
    // nothing read it, so no job is named.
    let newest = records.last().expect("a third record");
    assert_eq!(
        newest["reading"]["answer"]["memory_mib"].as_u64(),
        Some(READ_MEMORY_MIB),
        "the cached answer must be on the new job's record: {newest:#}"
    );
    assert!(
        newest["reading"]["by"].is_null(),
        "nothing ran for the second submission, so no job reads for it: {newest:#}"
    );
    assert!(
        newest["reading"]["from"]
            .as_str()
            .is_some_and(|from| from.contains("kept by (commit, command, args)")
                || from.contains("read before")),
        "the record must say the answer came out of the cache: {newest:#}"
    );
    assert_eq!(
        newest["attempts"][0]["runner"].as_str(),
        Some(RUNNER),
        "the job was still placed and still ran: {newest:#}"
    );

    // And the commit is really part of the key: the same command with the
    // same arguments on **another** commit pays for a reading of its own.
    let another = fixture.add_third_command();
    let third = probe_answering(&harness, &fixture, fixture::ANSWER_GOOD).await;
    assert_eq!(status_code(&third), 0, "{}", stderr(&third));
    assert_eq!(
        readings_run(&harness),
        2,
        "another commit is another (commit, command, args) and pays for its own reading: \
         {:#?}",
        harness.records()
    );
    let kept = std::fs::read_to_string(harness.state.path().join("readings").join("answers.json"))
        .expect("the hub keeps its readings where the store says it does");
    assert!(
        kept.contains(&fixture.rev_parse(BRANCH)) && kept.contains(&another),
        "the cache holds one answer per commit, keyed on it: {kept}"
    );
}

/// How many readings this hub has run: job directories whose command is the
/// fixture's reading script.
///
/// Counted off the hub's own state root, which is what makes "no second
/// reading" an assertion about a fact rather than about a duration.
fn readings_run(harness: &Harness) -> usize {
    harness
        .records()
        .iter()
        .filter(|record| {
            record["command"][0]
                .as_str()
                .is_some_and(|line| line.contains("labels.sh"))
        })
        .count()
}

/// D74's first limit, all four of its ways: the answer is read strictly, and
/// an answer that is not one **refuses the real job by name**, which is never
/// placed.
#[tokio::test]
async fn an_answer_that_is_not_one_refuses_the_real_job_by_name() {
    let Some(fixture) = Fixture::new("an_answer_that_is_not_one_refuses_the_real_job_by_name")
    else {
        return;
    };
    for (mode, what_it_must_say) in [
        (fixture::ANSWER_UNKNOWN_KEY, "\"scenario\""),
        (
            fixture::ANSWER_MISSING_KEY,
            "a missing key is not a default",
        ),
        (fixture::ANSWER_TWICE, "twice"),
        (fixture::ANSWER_NOT_A_FIGURE, "not a whole number"),
    ] {
        // A harness each, so that what each case leaves behind is read on its
        // own: four cases in one state root would make "the job was never
        // placed" a sentence about four jobs.
        let harness = Harness::new().await;
        let output = probe_answering(&harness, &fixture, mode).await;
        // 125: Herd with no verdict to give, never a code a caller could read
        // as a failing test (D16, D20).
        assert_eq!(
            status_code(&output),
            125,
            "an answer that is not one is not a verdict: {}",
            stderr(&output)
        );
        let (waited, reading) = the_job_and_its_reading(&harness);
        let job_id = waited["id"].as_str().expect("the job's id");
        let said = stderr(&output);
        assert!(
            said.contains(job_id),
            "the refusal must name the job it refused ({job_id}): {said}"
        );
        assert!(
            said.contains(reading["id"].as_str().expect("the reading's id")),
            "the refusal must name the reading that was run for it: {said}"
        );
        assert!(
            said.contains(what_it_must_say),
            "the refusal must say what was wrong with the answer ({what_it_must_say}): {said}"
        );
        assert!(
            said.contains(PROBE),
            "the refusal must name the command: {said}"
        );
        // And the real job was never placed: no attempt, and an end that says
        // it could not be.
        assert_eq!(
            waited["attempts"],
            serde_json::json!([]),
            "the job must never have been placed on a guess: {waited:#}"
        );
        assert_eq!(
            waited["state"]["type"].as_str(),
            Some("finished"),
            "the job is over rather than queued for ever: {waited:#}"
        );
        assert!(
            waited["state"]["outcome"]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("never placed")),
            "the record's own end must say it: {waited:#}"
        );
        // The reading itself ran and came off — it is the *answer* that was
        // refused, and the record keeps the reading either way (E-15).
        assert_eq!(
            reading["state"]["outcome"],
            serde_json::json!({"type": "exited", "code": 0}),
            "this case is about an answer, not about a reading that failed: {reading:#}"
        );
        assert_eq!(
            harness.records().len(),
            2,
            "nothing else was minted: {:#?}",
            harness.records()
        );
    }
}

/// D74's second limit, half of it: a reading that exits non-zero refuses the
/// real job by name. A job is never placed on a guess.
#[tokio::test]
async fn a_reading_that_fails_refuses_the_real_job_by_name() {
    let Some(fixture) = Fixture::new("a_reading_that_fails_refuses_the_real_job_by_name") else {
        return;
    };
    let harness = Harness::new().await;
    let output = probe_answering(&harness, &fixture, fixture::ANSWER_REFUSES).await;
    assert_eq!(
        status_code(&output),
        125,
        "a reading that failed is not a verdict about the job's tests: {}",
        stderr(&output)
    );
    let (waited, reading) = the_job_and_its_reading(&harness);
    let said = stderr(&output);
    assert!(
        said.contains(waited["id"].as_str().expect("the job's id")),
        "the refusal must name the job: {said}"
    );
    assert!(
        said.contains(&format!("exited {REFUSES_CODE} rather than 0")),
        "the refusal must say how the reading ended: {said}"
    );
    assert_eq!(
        waited["attempts"],
        serde_json::json!([]),
        "the job must never have been placed: {waited:#}"
    );
    assert_eq!(
        reading["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": REFUSES_CODE}),
        "the reading's own end is kept: {reading:#}"
    );
}

/// D74's second limit, the other half: a reading that does not finish inside
/// its cap refuses the real job by name — and it is the runner's cap that
/// stops it, which is what "confined like any other job" means (M4, D108).
#[tokio::test]
async fn a_reading_that_overruns_its_cap_refuses_the_real_job_by_name() {
    let Some(fixture) =
        Fixture::new("a_reading_that_overruns_its_cap_refuses_the_real_job_by_name")
    else {
        return;
    };
    let harness = Harness::with_policy(herd_hub::Policy {
        reading_cap: A_SHORT_READING_CAP,
        ..herd_hub::Policy::default()
    })
    .await;
    let output = probe_answering(&harness, &fixture, fixture::ANSWER_NEVER_ENDS).await;
    assert_eq!(
        status_code(&output),
        125,
        "a reading that was stopped is not a verdict about the job's tests: {}",
        stderr(&output)
    );
    let (waited, reading) = the_job_and_its_reading(&harness);
    let said = stderr(&output);
    assert!(
        said.contains(waited["id"].as_str().expect("the job's id")),
        "the refusal must name the job: {said}"
    );
    assert!(
        said.contains(&format!(
            "stopped by its Time cap of {}",
            A_SHORT_READING_CAP.as_secs()
        )),
        "the refusal must say that the reading's cap stopped it: {said}"
    );
    assert_eq!(
        waited["attempts"],
        serde_json::json!([]),
        "the job must never have been placed on a reading that never answered: {waited:#}"
    );
    assert_eq!(
        reading["state"]["outcome"]["type"].as_str(),
        Some("stopped_by_cap"),
        "the reading was stopped by the runner's enforcement of its cap: {reading:#}"
    );
    // The reading was really confined by the figure the hub gave it, and the
    // record says whose figure it was (D108).
    assert_eq!(
        reading["timeout_s"].as_u64(),
        Some(A_SHORT_READING_CAP.as_secs()),
        "the cap on the reading is the hub's own figure for a reading: {reading:#}"
    );
}

/// E-07 of a command whose requirements are read: a dry run cannot say where
/// such a job would run, because nothing knows yet — so it says so, and
/// answers about the reading that would run first. And it changes nothing.
#[tokio::test]
async fn a_dry_run_says_the_reading_would_run_first_and_changes_nothing() {
    let Some(fixture) =
        Fixture::new("a_dry_run_says_the_reading_would_run_first_and_changes_nothing")
    else {
        return;
    };
    let harness = Harness::new().await;
    let output = harness
        .herdctl_in(
            &fixture,
            &[
                "would-run",
                "--command",
                PROBE,
                "--commit",
                BRANCH,
                "--",
                "--answer",
                fixture::ANSWER_GOOD,
            ],
        )
        .await;
    assert_eq!(status_code(&output), 0, "{}", stderr(&output));
    let said = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        said.contains("what it needs is not known yet"),
        "a dry run must not claim to know what a job it cannot judge needs: {said}"
    );
    assert!(
        said.contains("labels.sh") && said.contains("would run on"),
        "it must answer about the reading that would run first: {said}"
    );
    assert!(
        harness.records().is_empty(),
        "a dry run mints nothing, least of all a reading: {:#?}",
        harness.records()
    );
}

/// The offer that reproduces the incident of 2026-10-19: **one** slot of the
/// label the reading command declares as its baseline (D159).
///
/// Everything else about the fixture's offer is unchanged. One slot is the
/// setting the first consumer wants to go back to, and it is what turned one
/// reading into a convoy of thirteen jobs: the reading carried the declared
/// command's labels, so it had to take that single slot, and it queued
/// forty-six minutes behind a long build.
const ONE_SLOT_OF_THE_BASELINE: &str = r#"[
    {"name":"grove-toolchain","kind":{"type":"counted","slots":1},"origins":{"declared":true}},
    {"name":"linux","kind":{"type":"shared"},"origins":{"declared":true}}
]"#;

/// How long these tests wait for the hub's own records to say what they are
/// about before failing with what the records held instead.
const PATIENCE: Duration = Duration::from_secs(60);

/// The record of the reading, found by the field that says it is one.
fn the_reading(records: &[serde_json::Value]) -> Option<&serde_json::Value> {
    records
        .iter()
        .find(|record| !record["reading_for"].is_null())
}

/// Wait until the reading has run to its end **while the job holding the
/// machine's one slot is still running**, then open the gate that lets the
/// holder finish.
///
/// This is the whole assertion, and it is made from the hub's own records at
/// one moment rather than from two clocks: while the gate is shut the holder
/// cannot end, so a reading that has reached a verdict beside a holder whose
/// attempt has no `ended_at` is a reading that ran on a machine whose only
/// slot was taken. Before D159 this moment never arrived — the reading sat in
/// the queue behind the slot — and the test fails with the records that show
/// it waiting.
async fn open_the_gate_once_a_reading_has_run_beside_the_holder(
    harness: &Harness,
    gate: &std::path::Path,
    holder: &str,
) {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        let records = harness.records();
        let held = records
            .iter()
            .find(|record| record["id"] == serde_json::json!(holder));
        if let (Some(held), Some(reading)) = (held, the_reading(&records)) {
            let holder_running = held["state"]["type"] == serde_json::json!("running")
                && held["attempts"][0]["ended_at"].is_null();
            let reading_is_over =
                reading["state"]["outcome"]["type"] == serde_json::json!("exited");
            if reading_is_over {
                assert!(
                    holder_running,
                    "the holder of the one slot must still be running when the reading \
                     ends, or this test proves nothing about them overlapping: {held:#}"
                );
                assert_eq!(
                    reading["state"]["outcome"],
                    serde_json::json!({"type": "exited", "code": 0}),
                    "the reading ran beside the holder and came off: {reading:#}"
                );
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no reading ever ran while {holder} held the machine's one slot; the hub's \
             records are {records:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::write(gate, b"open\n").expect("the gate opens");
}

/// **The incident of 2026-10-19, end to end, and it cannot happen again**
/// (D159).
///
/// A real hub, a real runner offering **one** slot of the baseline label, a
/// real job holding that slot for as long as this test says, and a real
/// `labels_from` reading of a command whose declared labels are that same
/// label. The reading runs beside the holder instead of queueing behind it,
/// and the job it was read for is then placed on the figures it answered.
///
/// What each half is for:
///
/// * the holder is the **third** command, which declares no artefact and
///   needs none, so the only thing it and the reading have in common is the
///   machine's one slot — nothing about a carriage can be what let the
///   reading through;
/// * the gate is what makes it an assertion rather than a race (D156): the
///   holder cannot end until this test writes the gate, so "the reading ended
///   while the holder was running" is a fact about the records and not about
///   which process the machine happened to finish first.
#[tokio::test]
async fn a_reading_runs_beside_a_real_job_on_a_machine_with_one_slot() {
    let Some(fixture) = Fixture::new("a_reading_runs_beside_a_real_job_on_a_machine_with_one_slot")
    else {
        return;
    };
    // A command of its own to hold the slot with: it leaves nothing and needs
    // nothing, so the slot is the only thing contended here.
    fixture.add_third_command();
    let harness = Harness::with_all_of(
        herd_hub::Policy::default(),
        herd_hub::Budget::default(),
        |_| {},
        |config| {
            config.capabilities = serde_json::from_str(ONE_SLOT_OF_THE_BASELINE)
                .expect("one counted slot of the baseline label");
        },
    )
    .await;
    let gate = fixture.home().join("gate-one-slot");
    let gate_arg = gate.to_string_lossy().into_owned();

    // The job that takes the one slot, submitted without waiting for it: it
    // stands in for the long build that the reading queued forty-six minutes
    // behind.
    let holder = harness
        .herdctl_in(
            &fixture,
            &[
                "submit",
                "--command",
                SKETCH,
                "--commit",
                BRANCH,
                "--",
                "--wait-for",
                gate_arg.as_str(),
            ],
        )
        .await;
    assert_eq!(status_code(&holder), 0, "{}", stderr(&holder));
    let holder_id = stdout(&holder).trim().to_owned();
    assert!(
        holder_id.starts_with('j'),
        "herdctl submit prints the job id: {holder_id:?}"
    );

    // Then the reading command, whose declared baseline is the very label
    // that job is holding.
    let (asked, ()) = tokio::join!(
        probe_answering(&harness, &fixture, fixture::ANSWER_GOOD),
        open_the_gate_once_a_reading_has_run_beside_the_holder(&harness, &gate, &holder_id),
    );
    assert_eq!(
        status_code(&asked),
        0,
        "the job the reading was read for must still run: {}",
        stderr(&asked)
    );

    // And the job really got its figures, from the reading that ran beside
    // the holder (E-15).
    let (waited, reading) = the_job_and_its_reading(&harness);
    assert_eq!(
        waited["reading"]["answer"],
        serde_json::json!({
            "labels": READ_LABELS,
            "memory_mib": READ_MEMORY_MIB,
            "tmpfs_mib": READ_TMPFS_MIB,
            "scenarios": READ_SCENARIOS,
        }),
        "the answer must be on the record of the job it was read for: {waited:#}"
    );
    // The reading's own record says whose reading it was, and the line
    // explaining its placement says it held none of the machine (D159).
    assert_eq!(
        reading["reading_for"].as_str(),
        waited["id"].as_str(),
        "a reading names the job it was minted for: {reading:#}"
    );
    let why = reading["attempts"][0]["why"]
        .as_str()
        .unwrap_or_else(|| panic!("the reading was never placed: {reading:#}"));
    assert!(
        why.contains("requirement reading") && why.contains("holds none of them"),
        "the placement must say the reading took nothing: {why:?}"
    );
    // And the cap is still the hub's own five minutes, unchanged by any of
    // this (D74, D108).
    assert_eq!(
        reading["timeout_s"].as_u64(),
        Some(herd_hub::READING_CAP.as_secs()),
        "the reading is still bounded by the hub's figure: {reading:#}"
    );
}
