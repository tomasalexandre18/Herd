//! A whole pass submitted at once, against one runner, end to end (D169).
//!
//! The shape of the first consumer's nightly, in miniature: a batch of jobs
//! submitted in one second, every one of them needing its requirements read
//! on a machine first, and one runner to read them all on. On 0.3.5 that put
//! some two hundred readings on that machine at once — a reading holds no
//! slot (D159), so nothing anywhere counted them — the host saturated, and
//! 201 of 214 jobs ended 125 with no test run. A second pass the same day
//! reached 255 readings at once and a host load of 399.
//!
//! The two things that has to mean, and the two tests here:
//!
//! * **at most `--readings-at-once` of them run on one runner at a time**,
//!   read off the records rather than off a log, and the rest *queue* — no
//!   job is refused and none ends `lost`;
//! * **a reading that was lost does not refuse the job it was read for**: it
//!   told this hub nothing about the commit, so the job is read again, which
//!   is what turns the incident's 125s back into results.
//!
//! The hub and the runner are the real code with their own state and data
//! roots, the requester is the real binary, and every job runs the fixture
//! consumer's own four executor steps on a real commit in a real repository.

use std::time::{Duration, Instant};

mod fixture;

use fixture::{Fixture, Harness, ANSWER_SLOW, BRANCH, SURVEY};

/// How many jobs this pass submits at once.
///
/// Four times the cap, so that the bound has to hold over several waves and
/// not merely over the first — and small enough that a suite nobody waits
/// for stays one nobody waits for. The shape is the consumer's; the figure
/// is not.
const A_PASS: usize = 8;

/// How long the whole pass may take before it is a failure. Generous because
/// CI is slow, and never a substitute for waiting on a real condition.
const PATIENCE: Duration = Duration::from_secs(120);

/// The cadence the test about a lost reading runs both sides at.
///
/// The 15 s of E-40 is the default; this is the same mechanism on a shorter
/// clock, so that a machine going away is noticed in a second rather than in
/// a minute.
const A_SHORT_CADENCE: Duration = Duration::from_millis(200);

/// What the hub is told, in all these tests.
///
/// The cap is passed explicitly even though it is the default: a test that
/// asserts "at most two" against a figure it did not state would start
/// passing for the wrong reason the day the default moved.
fn two_readings_at_once() -> herd_hub::Policy {
    herd_hub::Policy {
        readings_at_once: 2,
        ..herd_hub::Policy::default()
    }
}

/// Submit one `survey` job and come back with its id, without waiting.
async fn submit_a_survey(harness: &Harness, fixture: &Fixture, nth: usize) -> String {
    let nth = nth.to_string();
    let output = harness
        .herdctl_in(
            fixture,
            &[
                "submit",
                "--command",
                SURVEY,
                "--commit",
                BRANCH,
                "--",
                "--answer",
                ANSWER_SLOW,
                "--group",
                &nth,
            ],
        )
        .await;
    assert!(
        output.status.success(),
        "the submission failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Every record on the hub's disk whose state is final.
fn is_over(record: &serde_json::Value) -> bool {
    record["state"]["type"] == serde_json::json!("finished")
}

/// Wait until every record on the hub's disk is over, readings included.
async fn until_the_pass_is_over(harness: &Harness, jobs: usize) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let records = harness.records();
        let over = records.iter().filter(|record| is_over(record)).count();
        if records.len() >= jobs && over == records.len() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{over} of {} records are over after {PATIENCE:?}: {records:#?}",
            records.len()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait until a requirement reading is really running on the machine, on the
/// attempt named, so that what goes away goes away *under* it.
async fn until_a_reading_is_running(harness: &Harness, attempt: usize) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let running = harness.records().iter().any(|record| {
            !record["reading_for"].is_null()
                && record["state"]["type"] == serde_json::json!("running")
                && record["attempts"].as_array().map(Vec::len) == Some(attempt)
        });
        if running {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "attempt {attempt} of a reading never started: {:#?}",
            harness.records()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A moment off a record, as a value that can be compared with another.
fn moment(value: &serde_json::Value) -> herd_hub::Timestamp {
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("{value} is not a moment this hub wrote: {e}"))
}

/// When each requirement reading was running: its attempt's two moments.
///
/// Read off the records and off nothing else, which is also what the claim
/// step counts (D79): a reading holds its place for exactly as long as its
/// record says an attempt of it is open, so the overlap computed here is the
/// overlap the hub itself was reasoning about.
fn when_the_readings_ran(
    harness: &Harness,
) -> Vec<(String, herd_hub::Timestamp, herd_hub::Timestamp)> {
    harness
        .records()
        .iter()
        .filter(|record| !record["reading_for"].is_null())
        .map(|record| {
            let id = record["id"]
                .as_str()
                .expect("a record has an id")
                .to_owned();
            let attempt = &record["attempts"][0];
            assert!(
                !attempt.is_null(),
                "every reading here ran: {record:#}, attempts {:#}",
                record["attempts"]
            );
            let ended = &attempt["ended_at"];
            assert!(
                !ended.is_null(),
                "every reading here is over, so its attempt is closed: {record:#}"
            );
            (id, moment(&attempt["started_at"]), moment(ended))
        })
        .collect()
}

/// The most readings that were ever running at once, and which they were.
///
/// Counted at each reading's own start, which is where a count can only go
/// up: an interval that is open at the end never counts a reading whose
/// attempt had closed at that very instant.
fn the_most_at_once(
    ran: &[(String, herd_hub::Timestamp, herd_hub::Timestamp)],
) -> (usize, Vec<String>) {
    let mut worst = (0, Vec::new());
    for (_, at, _) in ran {
        let together: Vec<String> = ran
            .iter()
            .filter(|(_, started, ended)| started <= at && at < ended)
            .map(|(id, _, _)| id.clone())
            .collect();
        if together.len() > worst.0 {
            worst = (together.len(), together);
        }
    }
    worst
}

/// **The incident.** Eight jobs submitted at once against one runner: at most
/// two of their readings ever run together, the other six queue, and every
/// one of the eight jobs ends with its own verdict.
///
/// Before D169 this ran all eight readings at once — nothing counted them —
/// which is how two hundred of them landed on one machine.
#[tokio::test]
async fn a_pass_submitted_at_once_runs_at_most_two_readings_on_one_runner() {
    let Some(fixture) =
        Fixture::new("a_pass_submitted_at_once_runs_at_most_two_readings_on_one_runner")
    else {
        return;
    };
    fixture.add_a_command_nothing_coalesces();
    let harness = Harness::with_policy(two_readings_at_once()).await;

    let mut submitted = Vec::new();
    for nth in 0..A_PASS {
        submitted.push(submit_a_survey(&harness, &fixture, nth).await);
    }
    assert_eq!(
        submitted.len(),
        A_PASS,
        "nothing here deduplicates: {submitted:?}"
    );
    until_the_pass_is_over(&harness, A_PASS * 2).await;

    // One reading per job, and no more: the cap queues them, it does not
    // refuse them and it does not make the hub mint a second one.
    let ran = when_the_readings_ran(&harness);
    assert_eq!(
        ran.len(),
        A_PASS,
        "every job was read for exactly once: {ran:#?}"
    );

    let (most, together) = the_most_at_once(&ran);
    assert!(
        most <= 2,
        "{most} requirement readings ran on the one runner at once ({together:?}), and this \
         hub was told at most 2 (--readings-at-once). The records: {:#?}",
        harness.records()
    );
    // And the bound is a bound and not an accident of timing: with eight
    // readings of a second each and two at a time, two of them really did
    // overlap.
    assert_eq!(
        most, 2,
        "the readings never overlapped at all, so this test proved nothing about a cap: \
         {ran:#?}"
    );

    // Every job ended with its own verdict. Not one `lost`, not one
    // `impossible`: a reading over the cap waits, and waiting is never a
    // failure.
    for record in harness.records() {
        assert_eq!(
            record["state"]["outcome"]["type"],
            serde_json::json!("exited"),
            "a pass submitted at once ends in verdicts, never in losses: {record:#}"
        );
        assert_eq!(
            record["state"]["outcome"]["code"],
            serde_json::json!(0),
            "{record:#}"
        );
    }
}

/// **The other half of the incident.** A reading that was *lost* — its
/// machine went away under it — told this hub nothing about the commit, so
/// the job it was read for is read again rather than refused.
///
/// Before D169 one branch covered every way a reading can fail, and a
/// reading lost to a busy machine refused its job with "a job is never
/// placed on requirements nobody read". That sentence is true of a reading
/// that answered badly and false of one that never answered, and it is what
/// turned 201 of one nightly's 214 jobs into exit 125.
#[tokio::test]
async fn a_reading_lost_with_its_runner_is_read_again_and_the_job_runs() {
    let Some(fixture) =
        Fixture::new("a_reading_lost_with_its_runner_is_read_again_and_the_job_runs")
    else {
        return;
    };
    fixture.add_a_command_nothing_coalesces();
    // The shipped budgets — a job gets two goes at a runner that goes away
    // (E-21), and a job gets two readings — because what is under test is
    // what the consumer's own hub does. Only the clock is shortened, for the
    // reason every figure in `Policy` is a setting: a test must not wait a
    // minute to find out.
    //
    // Both sides are given the same cadence, because the hub refuses a
    // threshold that cannot outlast three of the reports the runner says it
    // will send — and a test that handed the two sides different figures
    // would be testing a configuration nobody can run.
    let mut harness = Harness::with_all_of(
        herd_hub::Policy {
            lost_after: Duration::from_secs(1),
            health_every: A_SHORT_CADENCE,
            ..two_readings_at_once()
        },
        herd_hub::Budget::default(),
        |_| {},
        |config| config.health_every = A_SHORT_CADENCE,
    )
    .await;

    let job = submit_a_survey(&harness, &fixture, 0).await;

    // The machine goes away under the reading as many times as E-21 gives
    // the reading goes, so the reading really is **lost** and not merely
    // attempted again. It comes back each time, because a second reading has
    // to have somewhere to run.
    for attempt in 1..=herd_hub::ATTEMPTS_ALLOWED as usize {
        until_a_reading_is_running(&harness, attempt).await;
        harness.the_runner_goes_away().await;
        harness.the_runner_comes_back().await;
    }

    until_the_pass_is_over(&harness, 2).await;

    let records = harness.records();
    let waited = records
        .iter()
        .find(|record| record["id"] == serde_json::json!(job))
        .unwrap_or_else(|| panic!("{job} has no record: {records:#?}"));
    assert_eq!(
        waited["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0}),
        "a reading lost with its machine is read again, and the job then runs: {waited:#}. \
         Every record: {records:#?}"
    );
    // And the record says what happened to it, in the figures a reader needs:
    // one reading lost, and the answer the second one gave.
    assert_eq!(
        waited["reading"]["lost"],
        serde_json::json!(1),
        "the record counts the reading that was lost: {waited:#}"
    );
    assert!(
        !waited["reading"]["answer"].is_null(),
        "the second reading answered, and the answer is on the record: {waited:#}"
    );
    // Two readings were run for one job, and the first of them is the one
    // that was lost.
    let readings: Vec<&serde_json::Value> = records
        .iter()
        .filter(|record| !record["reading_for"].is_null())
        .collect();
    assert_eq!(readings.len(), 2, "the job was read twice: {records:#?}");
    assert_eq!(
        readings[0]["state"]["outcome"]["type"],
        serde_json::json!("lost"),
        "the first reading went with its machine: {:#}",
        readings[0]
    );
}
