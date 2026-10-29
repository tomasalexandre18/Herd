//! A pass that submits itself when a branch has moved, through the real
//! binary: `herdctl if-moved` (E-41, D106, D207).
//!
//! What is asserted here is what a **timer** sees, because that is the only
//! caller this command will ever have: the exit code its unit read, the one
//! line on its stderr, and — the thing no unit test can show — how many jobs
//! the hub really has afterwards. The predicate itself is unit-tested in
//! `herdctl::moved` against a repository of its own; these are the five
//! sentences of the acceptance, each against a real hub, a real runner and a
//! real git repository with a real remote:
//!
//! * the ref moved → one job, and the hub's records say **one**;
//! * the ref did not move → no job at all, and a line saying so;
//! * two runs of the same state file → one of them submits nothing, and the
//!   hub still holds one job;
//! * the state file missing, unreadable or from another version → refused
//!   naming the file, and no job;
//! * the ref not there, or not a ref → refused by name, and no job.
//!
//! The state file lives **outside** the work tree, where a consumer would
//! keep it: it is the requester's own memory and not part of any commit.
//!
//! No consumer command is named here or in the fixture: every name is
//! invented (E-11).

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

mod fixture;

use fixture::{Fixture, Harness, ASSEMBLE, TRACKED};

/// The ref every test here watches: the fixture's own branch, in full.
///
/// In full, because that is what a consumer writes in a unit file and what
/// the state file records — `main` and `refs/heads/main` are one ref, and the
/// predicate stores the one spelling.
const WATCHED: &str = "refs/heads/main";

/// A hub nothing listens on, for the cases that must be refused **before**
/// anything is submitted. A test that reached a hub would be proving less:
/// here, a job is impossible, so "no job" is not a timing claim.
const NOWHERE: &str = "ws://127.0.0.1:1";

/// How long a record is waited for before the test gives up on the hub.
const PATIENCE: Duration = Duration::from_secs(20);

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

/// Where the state file goes: beside the checkout and not in it.
fn state_of(fixture: &Fixture) -> PathBuf {
    fixture.home().join("nightly.json")
}

/// Run `herdctl if-moved` in the fixture's checkout against no hub at all.
///
/// For the refusals: the working directory is the whole point — the
/// declaration and the ref are read where the repository already is (D106).
fn without_a_hub(fixture: &Fixture, arguments: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_herdctl"))
        .arg("--hub")
        .arg(NOWHERE)
        .args(arguments)
        .current_dir(fixture.root())
        // A git config of the fixture's own, so the person's identity and
        // their `~/.gitconfig` are out of the way and the refusals read the
        // same on every machine.
        .env("HOME", fixture.home())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("herdctl runs")
}

/// The arguments a nightly timer would carry: the declared command, the ref,
/// its state file, the level and the deadline E-41 asks for.
fn nightly(state: &Path) -> Vec<&str> {
    vec![
        "if-moved",
        "--command",
        ASSEMBLE,
        "--ref",
        WATCHED,
        "--state",
        state.to_str().expect("the fixture's path is utf-8"),
        "--priority",
        "nightly",
        "--deadline",
        "+2h",
        "--",
        "--verdict",
        "0",
    ]
}

/// Wait until the hub holds `want` records, then give them.
///
/// A record is written as the job is minted, and `herdctl` returns as soon
/// as the hub has acknowledged the id, so the two are a few microseconds
/// apart; waiting rather than reading once is what keeps that from being a
/// flake. It never waits for *more* than `want` — that would be waiting for
/// the failure this file is about.
async fn records_settle_at(harness: &Harness, want: usize) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let held = harness.records();
        if held.len() >= want {
            return held;
        }
        assert!(
            Instant::now() < deadline,
            "the hub never reached {want} record(s); it holds {held:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The whole of E-41's predicate through the binary, against a real hub: a
/// ref that moved is submitted **once**, and a ref that has not moved is a
/// line on stderr and no job.
///
/// The count on the hub is what makes this the acceptance rather than a
/// re-run of the unit tests: "it submits once" is a statement about how many
/// jobs exist, and only the hub can be asked that.
#[tokio::test]
async fn a_branch_that_moved_is_submitted_once_and_one_that_did_not_is_not() {
    let Some(fixture) =
        Fixture::new("a_branch_that_moved_is_submitted_once_and_one_that_did_not_is_not")
    else {
        return;
    };
    let harness = Harness::new().await;
    let state = state_of(&fixture);
    let first = fixture.rev_parse("HEAD");

    // The deliberate first act: the state file starts where the ref stands,
    // and nothing is submitted. There is no "no file yet, so it must have
    // moved".
    let begun = harness
        .herdctl_in(
            &fixture,
            &[
                "if-moved",
                "--init",
                "--command",
                ASSEMBLE,
                "--ref",
                WATCHED,
                "--state",
                state.to_str().expect("the fixture's path is utf-8"),
            ],
        )
        .await;
    assert_eq!(status_code(&begun), 0, "{}", stderr(&begun));
    assert!(
        stderr(&begun).contains(&first),
        "--init must say where the ref stands: {}",
        stderr(&begun)
    );
    assert!(
        harness.records().is_empty(),
        "--init submits nothing: {:#?}",
        harness.records()
    );

    // Nothing has moved, so nothing is submitted and the night is reported
    // in one line of this command's own. There is no hub event for it: the
    // hub never heard of this pass.
    let nothing = harness.herdctl_in(&fixture, &nightly(&state)).await;
    assert_eq!(
        status_code(&nothing),
        0,
        "a night with nothing to build is not a failure: {}",
        stderr(&nothing)
    );
    assert!(
        stderr(&nothing).contains(herdctl::moved::NOT_MOVED_PREFIX),
        "the skip is this command's own line: {}",
        stderr(&nothing)
    );
    assert!(
        stdout(&nothing).is_empty(),
        "nothing was submitted, so there is no id: {:?}",
        stdout(&nothing)
    );
    assert!(
        harness.records().is_empty(),
        "a ref that has not moved mints nothing: {:#?}",
        harness.records()
    );

    // The branch moves, as a consumer's `main` does when something lands.
    let second = fixture.commit_pushed(TRACKED, "the branch moved\n");
    let submitted = harness.herdctl_in(&fixture, &nightly(&state)).await;
    assert_eq!(status_code(&submitted), 0, "{}", stderr(&submitted));
    assert!(
        stderr(&submitted).contains(herdctl::moved::MOVED_PREFIX)
            && stderr(&submitted).contains(&first)
            && stderr(&submitted).contains(&second),
        "the line must name where the ref was and where it is: {}",
        stderr(&submitted)
    );
    let id = stdout(&submitted).trim().to_owned();
    assert!(!id.is_empty(), "a submission prints its id on stdout");

    let held = records_settle_at(&harness, 1).await;
    assert_eq!(held.len(), 1, "one move, one job: {held:#?}");
    assert_eq!(
        held[0]["declaration"]["commit"].as_str(),
        Some(second.as_str()),
        "the job must carry the commit the predicate read: {:#?}",
        held[0]
    );
    // The level on the record is the **declaration's**, not the timer's:
    // this fixture's command fixes its own level, and a declared command
    // that does overrules `--priority` exactly as it does for `herdctl
    // submit`. What is asserted is that `if-moved` goes through that same
    // door and gets that same rule, rather than a second one of its own.
    assert_eq!(
        held[0]["priority"].as_str(),
        Some("producer"),
        "a declared command's own level must overrule the timer's: {:#?}",
        held[0]
    );
    // The deadline E-41 asks for, on the record, as the moment the hub
    // will judge: a string, because that is how a record spells a
    // timestamp. It is here because the brief's claim — "the deadline and
    // the exclusive label it needs already exist in the submission" — is a
    // claim about this door, and an untested claim about a door is a
    // sentence.
    let deadline = held[0]["deadline"].as_str().unwrap_or_default();
    assert!(
        deadline.len() >= "2026-10-29T18:50:29Z".len(),
        "E-41's deadline is an ordinary submission's deadline and must be on the record: {:#?}",
        held[0]
    );

    // And once means once. The ref is where the last run left it, so the
    // next night submits nothing and the hub still holds exactly one job.
    let again = harness.herdctl_in(&fixture, &nightly(&state)).await;
    assert_eq!(status_code(&again), 0, "{}", stderr(&again));
    assert!(
        stderr(&again).contains(herdctl::moved::NOT_MOVED_PREFIX),
        "a recorded run is not submitted a second time: {}",
        stderr(&again)
    );
    assert_eq!(
        harness.records().len(),
        1,
        "the pass was submitted twice, which is the double nightly this guards: {:#?}",
        harness.records()
    );
}

/// Two runs racing one state file: the second submits nothing, and the hub
/// gets no second job.
///
/// The race is **established and not hoped for**: this test takes the same
/// exclusive lock a run takes and holds it across the invocation, which is
/// exactly the state a second timer finds while the first is still
/// submitting. Nothing is timed, nothing sleeps, and the assertion cannot
/// pass by luck.
#[tokio::test]
async fn a_second_run_of_the_same_pass_submits_nothing() {
    let Some(fixture) = Fixture::new("a_second_run_of_the_same_pass_submits_nothing") else {
        return;
    };
    let harness = Harness::new().await;
    let state = state_of(&fixture);
    let watching = herdctl::moved::Watching {
        state: state.clone(),
        watched: WATCHED.to_owned(),
        command: ASSEMBLE.to_owned(),
    };

    let begun = harness
        .herdctl_in(
            &fixture,
            &[
                "if-moved",
                "--init",
                "--command",
                ASSEMBLE,
                "--ref",
                WATCHED,
                "--state",
                state.to_str().expect("the fixture's path is utf-8"),
            ],
        )
        .await;
    assert_eq!(status_code(&begun), 0, "{}", stderr(&begun));
    // The ref really moved, so the only thing standing between this run and
    // a submission is the other run's hold.
    fixture.commit_pushed(TRACKED, "the branch moved while a run was going\n");

    let held_by_us = std::fs::File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(watching.lock_path())
        .expect("the lock file opens");
    held_by_us.lock().expect("this test holds the lock");

    let turned_away = harness.herdctl_in(&fixture, &nightly(&state)).await;
    assert_eq!(
        status_code(&turned_away),
        0,
        "a run that found another going is not a failure: {}",
        stderr(&turned_away)
    );
    assert!(
        stderr(&turned_away).contains(herdctl::moved::ANOTHER_RUN_PREFIX),
        "it must say that another run holds the pass: {}",
        stderr(&turned_away)
    );
    assert!(
        stdout(&turned_away).is_empty(),
        "nothing was submitted, so there is no id: {:?}",
        stdout(&turned_away)
    );
    assert!(
        harness.records().is_empty(),
        "the second run of an overlapping pair minted a job: {:#?}",
        harness.records()
    );

    // With the hold released, the same run is an ordinary submission: what
    // the lock did was postpone it, not lose it.
    drop(held_by_us);
    let submitted = harness.herdctl_in(&fixture, &nightly(&state)).await;
    assert_eq!(status_code(&submitted), 0, "{}", stderr(&submitted));
    let records = records_settle_at(&harness, 1).await;
    assert_eq!(records.len(), 1, "one move, one job: {records:#?}");
}

/// Every way the state file can be wrong is a refusal naming the file, and
/// never a guess that the ref moved.
///
/// Against no hub at all, so that "nothing was submitted" is not a claim
/// about timing: a job here is impossible.
#[test]
fn a_state_file_that_cannot_be_trusted_refuses_and_submits_nothing() {
    let Some(fixture) =
        Fixture::new("a_state_file_that_cannot_be_trusted_refuses_and_submits_nothing")
    else {
        return;
    };
    let state = state_of(&fixture);
    let named = state.display().to_string();

    // Missing.
    let missing = without_a_hub(&fixture, &nightly(&state));
    assert_eq!(status_code(&missing), 125, "{}", stderr(&missing));
    assert!(
        stderr(&missing).contains(&named) && stderr(&missing).contains("--init"),
        "a missing state file must refuse, naming itself and how to start one: {}",
        stderr(&missing)
    );
    assert!(
        stderr(&missing).contains(herdctl::NO_JOB_LINE),
        "and say plainly that nothing was submitted: {}",
        stderr(&missing)
    );
    assert!(!state.exists(), "a refusal writes no state file of its own");

    // Unreadable.
    std::fs::write(&state, "{ not json\n").expect("the test writes it");
    let unreadable = without_a_hub(&fixture, &nightly(&state));
    assert_eq!(status_code(&unreadable), 125, "{}", stderr(&unreadable));
    assert!(
        stderr(&unreadable).contains(&named),
        "an unreadable state file must refuse, naming itself: {}",
        stderr(&unreadable)
    );

    // From another version.
    let older = herdctl::moved::STATE_FORMAT_VERSION - 1;
    std::fs::write(
        &state,
        format!(
            r#"{{"state_format_version":{older},"ref":"{WATCHED}","command":"{ASSEMBLE}",
            "commit":"{}","recorded_at":"2026-10-28T03:00:00Z","job_id":null}}"#,
            "0".repeat(40)
        ),
    )
    .expect("the test writes it");
    let old = without_a_hub(&fixture, &nightly(&state));
    assert_eq!(status_code(&old), 125, "{}", stderr(&old));
    assert!(
        stderr(&old).contains(&named) && stderr(&old).contains(&format!("version {older}")),
        "a state file from another version must refuse, naming the file and the version: {}",
        stderr(&old)
    );

    // Started twice: `--init` never replaces a file, because that is how
    // last night gets forgotten.
    std::fs::remove_file(&state).expect("the test clears it");
    let begin = [
        "if-moved",
        "--init",
        "--command",
        ASSEMBLE,
        "--ref",
        WATCHED,
        "--state",
        state.to_str().expect("the fixture's path is utf-8"),
    ];
    let first = without_a_hub(&fixture, &begin);
    assert_eq!(status_code(&first), 0, "{}", stderr(&first));
    let held = std::fs::read_to_string(&state).expect("the state file");
    let twice = without_a_hub(&fixture, &begin);
    assert_eq!(status_code(&twice), 125, "{}", stderr(&twice));
    assert!(
        stderr(&twice).contains(&named) && stderr(&twice).contains("already exists"),
        "a second --init must refuse, naming the file: {}",
        stderr(&twice)
    );
    assert_eq!(
        std::fs::read_to_string(&state).expect("the state file"),
        held,
        "and must not have touched it"
    );
}

/// A ref that is not there has not moved; a hash is not a ref at all; and a
/// pass that names no ref is refused before the state file is opened.
#[test]
fn a_ref_that_cannot_move_is_refused_by_name() {
    let Some(fixture) = Fixture::new("a_ref_that_cannot_move_is_refused_by_name") else {
        return;
    };
    let state = state_of(&fixture);
    let path = state
        .to_str()
        .expect("the fixture's path is utf-8")
        .to_owned();

    // Started properly, so that the only thing wrong in the two runs below
    // is the ref.
    let begun = without_a_hub(
        &fixture,
        &[
            "if-moved",
            "--init",
            "--command",
            ASSEMBLE,
            "--ref",
            WATCHED,
            "--state",
            &path,
        ],
    );
    assert_eq!(status_code(&begun), 0, "{}", stderr(&begun));

    let nowhere = without_a_hub(
        &fixture,
        &[
            "if-moved",
            "--command",
            ASSEMBLE,
            "--ref",
            "refs/heads/nowhere",
            "--state",
            &path,
        ],
    );
    assert_eq!(status_code(&nowhere), 125, "{}", stderr(&nowhere));
    assert!(
        stderr(&nowhere).contains("refs/heads/nowhere"),
        "a ref that does not exist must be refused by name: {}",
        stderr(&nowhere)
    );

    let hash = fixture.rev_parse("HEAD");
    let frozen = without_a_hub(
        &fixture,
        &[
            "if-moved",
            "--command",
            ASSEMBLE,
            "--ref",
            &hash,
            "--state",
            &path,
        ],
    );
    assert_eq!(status_code(&frozen), 125, "{}", stderr(&frozen));
    assert!(
        stderr(&frozen).contains(&hash) && stderr(&frozen).contains("not a ref"),
        "a hash can never move and must be refused for what it is: {}",
        stderr(&frozen)
    );

    // And a pass with no ref at all: the fixture's declaration names no
    // schedule, so there is nothing for `--ref` to default to.
    let unsaid = without_a_hub(
        &fixture,
        &["if-moved", "--command", ASSEMBLE, "--state", &path],
    );
    assert_eq!(status_code(&unsaid), 125, "{}", stderr(&unsaid));
    assert!(
        stderr(&unsaid).contains("if_moved") && stderr(&unsaid).contains("--ref"),
        "it must say both ways of naming the ref: {}",
        stderr(&unsaid)
    );
}
