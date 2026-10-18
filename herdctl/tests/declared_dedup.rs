//! Two submissions of one declared command, end to end: when they are one
//! piece of work and when they are two (E-05, D156).
//!
//! The real `herdctl` binary, in the fixture consumer's own checkout, against
//! a real hub, a real runner and the four executor steps really running —
//! because this is exactly where a unit test and the lived behaviour once
//! diverged. `JobRecord::dedup_key` was the command and the labels alone, and
//! its own unit tests were over a command and a set of labels, so every one
//! of them passed while declared submissions at four different commits were
//! being answered with one commit's verdict. A test that never builds a
//! declaration cannot see that; these do.
//!
//! **Both tests are gated rather than raced.** The question "are these two
//! submissions one piece of work" is only asked while the first job is still
//! open, so each test holds its first job inside `run` — with `--wait-for`,
//! an ordinary argument both submissions carry, so the two remain
//! byte-identical — until the hub's own records say what the test is about
//! has happened. Without the gate the two submissions might not overlap at
//! all, and the test would pass against the very defect it is here for.
//!
//! No consumer command is named here or in the fixture: every name is
//! invented (E-11).

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

mod fixture;

use fixture::{Fixture, Harness, ASSEMBLE, BRANCH, TAG, TRACKED};

/// How long a test waits for the hub to have minted what it is about. Long
/// enough for a real `git fetch` on a loaded machine, short enough that a
/// defect is a failure and not a hung suite.
const PATIENCE: Duration = Duration::from_secs(60);

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn status_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("herdctl itself was killed: {:?}", output.status))
}

/// Every job record on the hub's disk, oldest id first, skipping a directory
/// whose record is not written yet.
///
/// [`Harness::records`] insists every job directory holds a readable record,
/// which is right for a test reading a finished state. These tests read while
/// jobs are being minted, and a directory claimed a moment before its
/// `job.json` lands is an ordinary state here, not a broken store.
fn records(harness: &Harness) -> Vec<serde_json::Value> {
    let jobs = harness.state.path().join("jobs");
    let mut dirs: Vec<std::path::PathBuf> = match std::fs::read_dir(&jobs) {
        Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    dirs.iter()
        .filter_map(|dir| std::fs::read_to_string(dir.join("job.json")).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect()
}

/// Wait until the hub's own records say what a test is waiting for, then open
/// the gate so the jobs can finish.
///
/// A deadline rather than a hope: a test whose premise never arrives fails
/// saying what the records held instead, which is the sentence that tells
/// whoever reads it that the deduplication key is wrong again.
async fn open_gate_once(
    harness: &Harness,
    gate: &Path,
    what: &str,
    mut arrived: impl FnMut(&[serde_json::Value]) -> bool,
) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let held = records(harness);
        if arrived(&held) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the hub never reached {what}; its records are {held:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::write(gate, b"open\n").expect("the gate opens");
}

/// The commit a record carries, which is where a declared submission keeps it
/// — and the field whose absence from the key was the defect.
fn commit_of(record: &serde_json::Value) -> String {
    record["declaration"]["commit"]
        .as_str()
        .unwrap_or_else(|| panic!("a declared job's record names its commit: {record}"))
        .to_owned()
}

fn id_of(record: &serde_json::Value) -> String {
    record["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a record has an id: {record}"))
        .to_owned()
}

/// D156, through the binaries: the same declared command at two commits is
/// two pieces of work, two jobs and two verdicts.
///
/// Both submissions name the same command with the same arguments, so the
/// line the hub records is byte-identical for both — which is the ordinary
/// declared command, since `{commit}` goes into the executor's `up` and
/// `fetch` steps and not into the line. Before D156 the second submission
/// joined the first and its caller was handed the first commit's output with
/// nothing saying anything had been substituted.
#[tokio::test]
async fn two_declared_submissions_at_two_commits_are_two_jobs() {
    let Some(fixture) = Fixture::new("two_declared_submissions_at_two_commits_are_two_jobs") else {
        return;
    };
    // Two real commits of one repository, read from the fixture's own git so
    // that no hash is written into this file.
    let first_commit = fixture.rev_parse(TAG);
    let second_commit = fixture.commit_pushed(TRACKED, "a second commit of the repository\n");
    assert_ne!(first_commit, second_commit, "the fixture made one commit");

    let harness = Harness::new().await;
    let gate = fixture.home().join("gate-two-commits");
    let gate_arg = gate.to_string_lossy().into_owned();
    // The two argument lists differ in the ref alone. Everything the key is
    // made of — the command, its arguments, the labels it resolves to — is
    // the same on both, which is the whole point.
    let asking_for_the_tag = [
        "run",
        "--command",
        ASSEMBLE,
        "--commit",
        TAG,
        "--",
        "--wait-for",
        gate_arg.as_str(),
    ];
    let asking_for_the_branch = [
        "run",
        "--command",
        ASSEMBLE,
        "--commit",
        BRANCH,
        "--",
        "--wait-for",
        gate_arg.as_str(),
    ];

    let (first, second, ()) = tokio::join!(
        harness.herdctl_in(&fixture, &asking_for_the_tag),
        harness.herdctl_in(&fixture, &asking_for_the_branch),
        open_gate_once(&harness, &gate, "two jobs", |held| held.len() >= 2),
    );

    assert_eq!(status_code(&first), 0, "{}", stderr(&first));
    assert_eq!(status_code(&second), 0, "{}", stderr(&second));

    // Two jobs on the disk, one per commit, each answering one submission.
    let held = records(&harness);
    assert_eq!(
        held.len(),
        2,
        "two commits are two jobs, and one of them was answered with the \
         other's verdict: {held:#?}"
    );
    let mut commits: Vec<String> = held.iter().map(commit_of).collect();
    commits.sort();
    let mut expected = vec![first_commit.clone(), second_commit.clone()];
    expected.sort();
    assert_eq!(commits, expected, "each job must carry its own commit");
    for record in &held {
        assert_eq!(
            record["submissions"].as_u64(),
            Some(1),
            "neither job answers a submission about another commit: {record}"
        );
    }
    assert_ne!(id_of(&held[0]), id_of(&held[1]));
    // The line really is the same on both: it is the commit, and nothing
    // else, that made these two pieces of work.
    assert_eq!(
        held[0]["command"], held[1]["command"],
        "this test is only about the commit if the two lines are identical"
    );
    assert_eq!(held[0]["labels"], held[1]["labels"]);

    // And the thing a requester actually suffers from: each caller was told
    // about **its own** commit, by the fixture's own `fetch` hook after a
    // real fetch from the fixture's own bare remote.
    let said_first = stdout(&first);
    let said_second = stdout(&second);
    assert!(
        said_first.contains(&format!("grove: at {first_commit}")),
        "the caller asking for {first_commit} was not told about it: {said_first:?}"
    );
    assert!(
        said_second.contains(&format!("grove: at {second_commit}")),
        "the caller asking for {second_commit} was not told about it: {said_second:?}"
    );
    assert!(
        !said_first.contains(&second_commit),
        "the caller asking for {first_commit} was handed {second_commit}'s \
         output: {said_first:?}"
    );
    assert!(
        !said_second.contains(&first_commit),
        "the caller asking for {second_commit} was handed {first_commit}'s \
         output: {said_second:?}"
    );
}

/// E-05's whole value, which D156 must not have cost: two submissions of one
/// declared command at **one** commit are one execution.
///
/// A push hook firing twice is the case this exists for. A fix that put the
/// commit in the key and also made every key unique would read as green on
/// the test above and would quietly run every piece of work twice.
#[tokio::test]
async fn two_declared_submissions_at_one_commit_are_one_job() {
    let Some(fixture) = Fixture::new("two_declared_submissions_at_one_commit_are_one_job") else {
        return;
    };
    let commit = fixture.rev_parse(BRANCH);

    let harness = Harness::new().await;
    let gate = fixture.home().join("gate-one-commit");
    let gate_arg = gate.to_string_lossy().into_owned();
    let asking = [
        "run",
        "--command",
        ASSEMBLE,
        "--commit",
        BRANCH,
        "--",
        "--wait-for",
        gate_arg.as_str(),
    ];

    let (first, second, ()) = tokio::join!(
        harness.herdctl_in(&fixture, &asking),
        harness.herdctl_in(&fixture, &asking),
        // The gate opens once the join has happened, which is the record
        // saying one job answers two submissions.
        open_gate_once(
            &harness,
            &gate,
            "one job answering two submissions",
            |held| { held.len() == 1 && held[0]["submissions"].as_u64() == Some(2) }
        ),
    );

    assert_eq!(status_code(&first), 0, "{}", stderr(&first));
    assert_eq!(status_code(&second), 0, "{}", stderr(&second));

    let held = records(&harness);
    assert_eq!(
        held.len(),
        1,
        "two submissions of one command at one commit must be one job: {held:#?}"
    );
    assert_eq!(held[0]["submissions"].as_u64(), Some(2));
    assert_eq!(commit_of(&held[0]), commit);
    // One execution, and both callers read it: the half of E-05 a requester
    // sees. The tool ran once, so its own verdict is the last word of both.
    let ended = format!("grove {ASSEMBLE}: verdict 0\n");
    for (who, output) in [("the first caller", &first), ("the second caller", &second)] {
        let said = stdout(output);
        assert!(
            said.contains(&format!("grove: at {commit}")),
            "{who} was not told about {commit}: {said:?}"
        );
        assert!(
            said.ends_with(&ended),
            "{who} must read the one execution's own verdict last: {said:?}"
        );
    }
}
