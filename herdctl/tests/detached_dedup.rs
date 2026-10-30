//! The **detached** submission of a declared command, twice in a row: E-42's
//! own shape, through the real binaries as processes.
//!
//! E-42 is *"a push hook submits the producer job"*. The hook is the
//! consumer's; Herd's side is `herdctl submit`, which leaves with the job's id
//! and follows nothing — `id=$(herdctl submit --command <producer> --commit
//! "$sha")` is the whole of what a hook needs. What E-42 asks of it is that a
//! producer pushed twice in a row be **one** job, and that the second
//! submission be answered with that job's id rather than mint a second
//! execution of the same commit.
//!
//! `declared_dedup.rs` already asserts E-05's key over the **attached** path
//! (`herdctl run`), which is where D156's defect was found and fixed. Nothing
//! asserted it over the detached one until this file, and the two paths are
//! not the same code: `run` follows the job it submitted and reads the hub's
//! sentences, while `submit` takes `JobAccepted` and closes the socket. A
//! deduplication that held for a follower and not for a hook would be a
//! defect nobody could see, and E-42's caller is the hook.
//!
//! **What makes these pass is the hub's derived key, not the declared one.**
//! The fixture's producer declares `dedup_key = "assemble-{commit}-{args}"`,
//! `herdctl` parses and validates it, and then nothing carries it: it is not
//! a field on `herd_proto::Asked` (ORCH-148). What joins the second
//! submission is `JobRecord::dedup_key` — the command, the labels and the
//! declared commit (D156) — which for this case answers the same question the
//! declared key would have. So these tests are about the behaviour E-42 asks
//! for and not about the field, and they would stay green if the declared
//! field were refused at parse time rather than carried.
//!
//! **Gated rather than raced**, as `declared_dedup.rs` is: the question "are
//! these two submissions one piece of work" is only asked while the first job
//! is still open, so the first job is held inside its run with `--wait-for` —
//! an ordinary argument both submissions carry, so the two stay
//! byte-identical — until this test has read what it is about. Unlike the
//! attached tests, the two submissions here are **sequential and not
//! concurrent**, and that is not a convenience: `submit` returns once the hub
//! has said `JobAccepted`, so by the time the second one leaves, the first
//! job's record exists by construction. There is no window to race.
//!
//! No consumer command is named here or in the fixture: every name is
//! invented (E-11).

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

mod fixture;

use fixture::{Fixture, Harness, ASSEMBLE, BRANCH, TAG, TRACKED};

/// How long a test waits for the hub's records to say what it is about. Long
/// enough for a real `git fetch` and four real executor steps on a loaded
/// machine, short enough that a defect is a failure and not a hung suite.
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

/// Submit one declared command at one ref and come back with the id it
/// printed, without waiting for the job.
///
/// Exactly what a hook does, down to reading the id off stdout: the id alone
/// is on stdout and Herd's own prose is on stderr (E-02), so a hook needs no
/// parsing at all.
async fn submit_detached(harness: &Harness, fixture: &Fixture, at: &str, gate: &Path) -> String {
    let gate = gate.to_string_lossy().into_owned();
    let output = harness
        .herdctl_in(
            fixture,
            &[
                "submit",
                "--command",
                ASSEMBLE,
                "--commit",
                at,
                "--",
                "--wait-for",
                gate.as_str(),
            ],
        )
        .await;
    assert_eq!(status_code(&output), 0, "{}", stderr(&output));
    let id = stdout(&output).trim().to_owned();
    assert!(
        !id.is_empty(),
        "a detached submission prints its job's id on stdout and nothing else: {:?}",
        stdout(&output)
    );
    id
}

/// Every job record on the hub's disk, oldest id first, skipping a directory
/// whose record is not written yet.
///
/// [`Harness::records`] insists every job directory holds a readable record,
/// which is right for a test reading a finished state. This one reads while
/// jobs are being minted, and a directory claimed a moment before its
/// `job.json` lands is an ordinary state here, not a broken store.
fn records(harness: &Harness) -> Vec<serde_json::Value> {
    let jobs = harness.state.path().join("jobs");
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(&jobs) {
        Ok(entries) => entries
            .map(|entry| entry.expect("a job directory").path())
            .collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    dirs.iter()
        .filter_map(|dir| std::fs::read_to_string(dir.join("job.json")).ok())
        .map(|text| serde_json::from_str(&text).expect("the record is json"))
        .collect()
}

/// Wait until the hub's own records say what this test is waiting for.
///
/// A deadline rather than a hope: a test whose premise never arrives fails
/// saying what the records held instead, which is the sentence that tells
/// whoever reads it that a hook's second push is no longer being joined.
async fn until(harness: &Harness, what: &str, ready: impl Fn(&[serde_json::Value]) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let held = records(harness);
        if ready(&held) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waited {PATIENCE:?} for {what} and the hub's records held {held:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Whether a record's state is the last one it will have.
fn is_over(record: &serde_json::Value) -> bool {
    record["state"]["type"] == serde_json::json!("finished")
}

/// The commit a record carries, which is where a declared submission keeps it.
fn commit_of(record: &serde_json::Value) -> String {
    record["declaration"]["commit"]
        .as_str()
        .unwrap_or_else(|| panic!("a declared job's record carries its commit: {record}"))
        .to_owned()
}

fn id_of(record: &serde_json::Value) -> String {
    record["id"]
        .as_str()
        .expect("a record carries its id")
        .to_owned()
}

/// E-42, the half of it that is Herd's: a producer pushed twice at one commit
/// is one job, and the second detached submission is answered with that job's
/// id.
///
/// The case is a hook firing twice — a second push of the same commit, a
/// re-run of a hook that was interrupted, two mirrors of one push. Each one
/// leaves with an id, both ids are the same id, and the one execution's own
/// verdict is on the record that id names.
#[tokio::test]
async fn two_detached_submissions_at_one_commit_are_one_job() {
    let Some(fixture) = Fixture::new("two_detached_submissions_at_one_commit_are_one_job") else {
        return;
    };
    let commit = fixture.rev_parse(BRANCH);

    let harness = Harness::new().await;
    let gate = fixture.home().join("gate-one-commit-detached");

    // The first push's hook.
    let first_id = submit_detached(&harness, &fixture, BRANCH, &gate).await;
    // Established, not assumed: the first job exists and is not over, which
    // is the only state in which the question this test asks is asked at all.
    until(&harness, "the first job's record", |held| {
        held.len() == 1 && !is_over(&held[0])
    })
    .await;

    // The second push's hook, byte-identical to the first.
    let second_id = submit_detached(&harness, &fixture, BRANCH, &gate).await;

    assert_eq!(
        second_id, first_id,
        "a second detached submission of one producer at one commit must be \
         answered with the first job's id, not a second job's"
    );

    until(&harness, "one job answering two submissions", |held| {
        held.len() == 1 && held[0]["submissions"].as_u64() == Some(2)
    })
    .await;

    let held = records(&harness);
    assert_eq!(
        held.len(),
        1,
        "two detached submissions of one command at one commit must be one \
         job: {held:#?}"
    );
    assert_eq!(held[0]["submissions"].as_u64(), Some(2));
    assert_eq!(id_of(&held[0]), first_id);
    assert_eq!(commit_of(&held[0]), commit);

    // Let the one execution finish, and read what it left on the hub's disk
    // the way a reader outside Herd does (E-15): one record, one outcome, one
    // verdict — which is the result **both** hooks hold the id of.
    std::fs::write(&gate, b"open\n").expect("the gate opens");
    until(&harness, "the one job to be over", |held| {
        held.len() == 1 && is_over(&held[0])
    })
    .await;

    let over = records(&harness);
    assert_eq!(over.len(), 1, "one execution, not two: {over:#?}");
    assert_eq!(
        over[0]["state"]["outcome"],
        serde_json::json!({ "type": "exited", "code": 0 }),
        "the one execution's own verdict is what both hooks' id answers: {}",
        over[0]["state"]
    );
    assert_eq!(over[0]["submissions"].as_u64(), Some(2));
    let logged = harness.log(&first_id);
    assert!(
        logged.contains(&format!("grove {ASSEMBLE}: verdict 0")),
        "the one execution really ran the declared command: {logged:?}"
    );
}

/// The detector, proven on work that must **not** be joined: two detached
/// submissions of one producer at two commits are two jobs.
///
/// Without this, the test above would read green against a `herdctl submit`
/// that joined every submission to whatever was open — which is the defect
/// D156 found on the attached path, where four requesters asking about four
/// commits were handed one commit's verdict. A hook fires on a *new* commit;
/// a key that cannot tell two commits apart is worse than no key at all.
#[tokio::test]
async fn two_detached_submissions_at_two_commits_are_two_jobs() {
    let Some(fixture) = Fixture::new("two_detached_submissions_at_two_commits_are_two_jobs") else {
        return;
    };
    // Two real commits of one repository, read from the fixture's own git so
    // that no hash is written into this file.
    let first_commit = fixture.rev_parse(TAG);
    let second_commit = fixture.commit_pushed(TRACKED, "a second commit of the repository\n");
    assert_ne!(first_commit, second_commit, "the fixture made one commit");

    let harness = Harness::new().await;
    let gate = fixture.home().join("gate-two-commits-detached");

    // The two submissions differ in the ref alone. Everything the key is made
    // of besides the commit — the command, its arguments, the labels it
    // resolves to — is the same on both, which is the whole point.
    let first_id = submit_detached(&harness, &fixture, TAG, &gate).await;
    until(&harness, "the first job's record", |held| {
        held.len() == 1 && !is_over(&held[0])
    })
    .await;
    let second_id = submit_detached(&harness, &fixture, BRANCH, &gate).await;

    assert_ne!(
        second_id, first_id,
        "a producer at a new commit is new work: it must not be answered with \
         the job of another commit"
    );

    until(&harness, "two jobs", |held| held.len() == 2).await;
    // Nothing is held any more: both jobs may finish whenever the machine
    // gets to them, and what this test is about is already on the disk.
    std::fs::write(&gate, b"open\n").expect("the gate opens");

    let held = records(&harness);
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
    // The line really is the same on both: it is the commit, and nothing
    // else, that made these two pieces of work.
    assert_eq!(
        held[0]["command"], held[1]["command"],
        "this test is only about the commit if the two lines are identical"
    );
    assert_eq!(held[0]["labels"], held[1]["labels"]);
}
