//! `ran_commit` across two versions of Herd, in both directions, including
//! the direction we do not like (D176).
//!
//! The field is additive: a `#[serde(default)]` option on `StepRun`, a
//! `#[serde(default)]` option on the record's attempt, and an option on one
//! frame. `PROTOCOL_VERSION` stays 2 and `RECORD_VERSION` stays 1, so no
//! handshake and no state root is refused by either number, in either
//! direction. That much is the easy half and §1 asserts it.
//!
//! §2 is the half worth writing down. **A hub of an earlier version reads a
//! record carrying the field and erases it the first time it writes that
//! record back.** Serde drops what it never decoded and has nothing to
//! re-emit, so a rollback *reads* every record and *preserves* the field on
//! none of the ones it touches. The reachable paths are ordinary: a
//! `record_state` on any job still in flight, and `note_outputs_fetched`
//! (`herd-hub/src/store.rs`), which rewrites a **finished** job's whole
//! record the first time a requester fetches its outputs. For a diagnostic
//! field this is an acceptable cost; what is not acceptable is discovering
//! it, so it is asserted here rather than hoped about.
//!
//! **How an older peer is modelled.** By the types it had: a hand-written
//! `OldStepRun` and `OldAttempt` carrying exactly the fields that existed
//! before this change, with serde's ordinary defaults. That is not a
//! stand-in for the mechanism — it *is* the mechanism, because what an older
//! binary does to an unknown field is decided entirely by the shape of the
//! type it deserialises into. No `deny_unknown_fields` appears anywhere in
//! `herd-proto` or `herd-hub`, and both frame enums carry `#[serde(other)]`,
//! which is why the old shapes below have neither.
//!
//! **`herdctl` is not in this file's matrices, and that is not an
//! oversight.** It never reads a record — it reads frames — so "an old
//! `herdctl` against a new record" names an artefact that does not exist.
//! What an old `herdctl` meets is a `FromHub::JobFinished` with one more
//! key, and §3 asserts that case for what it is.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use herd_hub::job::{JobState, NewJob, RECORD_VERSION};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::{
    CommitRun, Declaration, Executor, JobId, JobOutcome, Step, StepRun, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};

/// The commit a job is submitted for here.
const DECLARED: &str = "11111111111111111111111111111111111111aa";
/// The revision the machine says it really reached. Not [`DECLARED`].
const REACHED: &str = "22222222222222222222222222222222222222bb";

// ---------------------------------------------------------------------------
// An older Herd, as its own types
// ---------------------------------------------------------------------------

/// `herd_proto::StepRun` as a build that predates D176 declares it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OldStepRun {
    step: String,
    started_unix_ms: i64,
    duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ended: Option<String>,
}

/// `herd_hub::job::Attempt` as that same build declares it: no `commit`, and
/// steps of the shape above.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OldAttempt {
    number: u32,
    runner: String,
    started_at: String,
    #[serde(default)]
    ended_at: Option<String>,
    #[serde(default)]
    requeue_code: Option<i32>,
    #[serde(default)]
    ended: Option<String>,
    why: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<OldStepRun>,
}

/// The requester-facing verdict frame as an older `herdctl` declares it.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OldFromHub {
    JobFinished {
        job_id: JobId,
        outcome: JobOutcome,
    },
    /// Every other frame. The real enum has one of these too
    /// (`#[serde(other)]`), which is half of why this change crosses a
    /// version boundary at all.
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// A store with one declared job that reported a revision
// ---------------------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
    store: Arc<JobDirStore>,
    id: JobId,
}

impl Fixture {
    /// A declared job of [`DECLARED`], one attempt, and a `fetch` step whose
    /// reported revision is whatever the caller passes.
    fn with_fetch_reporting(ran: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        let id = store
            .submit(
                NewJob::new(vec!["the-declared-line".to_owned()])
                    .declared(Some(a_declaration()), None),
            )
            .expect("the job is minted")
            .record()
            .id
            .clone();
        store
            .begin_attempt(&id, "a-runner", "the only runner", None)
            .expect("an attempt starts");
        store
            .record_step(&id, 1, fetch_reporting(ran))
            .expect("the fetch step records");
        Self {
            _dir: dir,
            store,
            id,
        }
    }

    fn record_path(&self) -> std::path::PathBuf {
        self.store.job_dir(&self.id).join("job.json")
    }

    fn text(&self) -> String {
        fs::read_to_string(self.record_path()).expect("the record reads")
    }

    /// What the record says about the revision this attempt ran.
    fn commit(&self) -> Option<CommitRun> {
        self.store
            .job(&self.id)
            .expect("the record reads")
            .expect("the job is there")
            .attempts
            .last()
            .and_then(|attempt| attempt.commit.clone())
    }
}

fn a_declaration() -> Declaration {
    Declaration {
        command: "a-declared-command".to_owned(),
        commit: DECLARED.to_owned(),
        args: Vec::new(),
        executor: Executor {
            up: "tool ws up {job}".to_owned(),
            fetch: "tool ws run {job} -- 'reach a revision and write it to {ran}'".to_owned(),
            run: "tool ws run {job} -- {line}".to_owned(),
            down: "tool ws down {job} --purge".to_owned(),
            env: BTreeMap::new(),
        },
    }
}

fn fetch_reporting(ran: Option<&str>) -> StepRun {
    StepRun {
        step: Step::Fetch,
        started_unix_ms: 1_791_878_153_000,
        duration_ms: 1_200,
        exit_code: Some(0),
        ended: None,
        ran_commit: ran.map(str::to_owned),
    }
}

// ---------------------------------------------------------------------------
// 1. Neither number moves, and both directions of the wire still work
// ---------------------------------------------------------------------------

/// The two numbers a compatibility break would have to move, after the
/// change.
///
/// Asserted rather than asserted-about-in-prose, because the whole claim of
/// this increment is that neither moves **for it**: a record version refuses
/// an operator's entire state root, and a handshake refuses every runner that
/// has not been restarted.
///
/// **`PROTOCOL_VERSION` has since moved, and not for `ran_commit`.** D199
/// moved it 2 → 3 for M11's pack half, so the literal below is 3 and this
/// increment's claim is untouched. The pin stays a **literal** on purpose:
/// against the constant it would be a tautology, and what it is here to catch
/// is an *accidental* move.
#[test]
fn neither_version_number_moves() {
    assert_eq!(
        PROTOCOL_VERSION, 3,
        "3 is D199's bump for M11's pack half; `ran_commit` is an additive field and moved \
         nothing"
    );
    assert_eq!(
        RECORD_VERSION, 1,
        "every existing state root still reads; D199 moves this to 2 with the hub's own pack \
         registry, and not for this field"
    );
}

/// **Old runner → new hub.** A step frame from a build that never heard of
/// the field, decoded by this one.
///
/// The absence has to arrive as *silence*, never as agreement: that is the
/// whole reason the field is an `Option` and not a `String` defaulting to
/// the declared commit.
#[test]
fn a_step_from_an_older_runner_decodes_as_having_said_nothing() {
    let older = r#"{
        "step": "fetch",
        "started_unix_ms": 1791878153000,
        "duration_ms": 1200,
        "exit_code": 0
    }"#;
    let step: StepRun = serde_json::from_str(older).expect("this build decodes an older step");
    assert_eq!(step.step, Step::Fetch);
    assert_eq!(
        step.ran_commit, None,
        "an older runner said nothing, and nothing is not the declared commit"
    );

    // And the same frame, through the real wire type.
    let frame =
        format!(r#"{{"type":"step_finished","job_id":"j0000000001","attempt":1,"step":{older}}}"#);
    match serde_json::from_str::<herd_proto::ToHub>(&frame) {
        Ok(herd_proto::ToHub::StepFinished { step, .. }) => assert_eq!(step.ran_commit, None),
        other => panic!("an older runner's step frame has to decode: {other:?}"),
    }
}

/// **New runner → old hub.** A step frame from this build, decoded by the
/// shape the older hub had.
///
/// It decodes, every field it knows survives, and the one it does not know
/// is dropped on the floor — which is exactly what "additive" means on the
/// wire and is why `PROTOCOL_VERSION` does not move.
#[test]
fn a_step_from_this_runner_decodes_on_an_older_hub_minus_the_new_field() {
    let frame = serde_json::to_string(&fetch_reporting(Some(REACHED))).expect("it encodes");
    assert!(
        frame.contains(REACHED),
        "this build puts the revision on the wire: {frame}"
    );
    let old: OldStepRun = serde_json::from_str(&frame).expect("an older hub decodes it");
    assert_eq!(old.step, "fetch");
    assert_eq!(old.duration_ms, 1_200);
    assert_eq!(old.exit_code, Some(0));
    assert!(
        !serde_json::to_string(&old)
            .expect("it re-encodes")
            .contains("ran_commit"),
        "the older hub cannot re-emit a field it never decoded"
    );
}

/// A runner that reports nothing puts **no key at all** on the wire, so an
/// older hub receives byte for byte what it received before the change.
///
/// `skip_serializing_if` is what makes that true, and a test is what keeps
/// it true: without it, every frame in the fleet would grow a `null` on the
/// day this ships, for the benefit of nobody.
#[test]
fn a_runner_with_nothing_to_report_sends_exactly_what_it_sent_before() {
    let silent = serde_json::to_string(&fetch_reporting(None)).expect("it encodes");
    assert!(
        !silent.contains("ran_commit"),
        "silence puts nothing on the wire: {silent}"
    );
}

// ---------------------------------------------------------------------------
// 2. The record, and the direction we do not like
// ---------------------------------------------------------------------------

/// The record carries both strings, and says which is which.
///
/// This is the oracle: before it, "this verdict is about commit X" rested on
/// the assumption that a `fetch` exiting 0 left the tree at X, and nothing
/// on the record could contradict it afterwards.
#[test]
fn a_divergence_is_a_fact_on_the_record_and_not_a_refusal() {
    let fixture = Fixture::with_fetch_reporting(Some(REACHED));
    let commit = fixture.commit().expect("the attempt says something");
    assert_eq!(commit.declared, DECLARED);
    assert_eq!(commit.ran.as_deref(), Some(REACHED));
    assert!(commit.diverged(), "the two strings differ");
    assert!(
        commit.line().contains(DECLARED) && commit.line().contains(REACHED),
        "the record's sentence names both: {}",
        commit.line()
    );

    // Recorded, not refused: the job is where it was and nothing about it
    // has failed. A divergence that stopped a job would be a new failure
    // mode shipping on the same day as the field that finds it.
    let record = fixture
        .store
        .job(&fixture.id)
        .expect("reads")
        .expect("there")
        .state
        .name()
        .to_owned();
    assert_eq!(record, "assigned", "the job carries on exactly as before");

    // And both strings really are on the disk, in one file, for a reader
    // with nothing but `cat`.
    let text = fixture.text();
    assert!(text.contains(DECLARED) && text.contains(REACHED), "{text}");
}

/// Silence is written down as silence.
///
/// `"ran": null` and not an absent key: an absent key and a key that is
/// there and empty read the same to a careless reader, and the one thing
/// this field must never do is let a runner's silence pass for agreement.
#[test]
fn a_runner_that_said_nothing_is_recorded_as_having_said_nothing() {
    let fixture = Fixture::with_fetch_reporting(None);
    let commit = fixture.commit().expect("the attempt still says something");
    assert_eq!(commit.declared, DECLARED);
    assert_eq!(commit.ran, None);
    assert!(!commit.diverged(), "silence is not a divergence");
    assert!(
        commit.line().contains("did not report a revision"),
        "the record says it in words: {}",
        commit.line()
    );
    assert!(
        fixture.text().contains("\"ran\":null"),
        "silence is an explicit null on the disk: {}",
        fixture.text()
    );
}

/// An ad-hoc job declares no commit, so there is nothing for a revision to
/// agree or disagree with, and the record says nothing rather than
/// inventing a comparison.
#[test]
fn a_job_that_declared_no_commit_records_no_comparison() {
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let id = store
        .submit(NewJob::new(vec!["true".to_owned()]))
        .expect("minted")
        .record()
        .id
        .clone();
    store
        .begin_attempt(&id, "a-runner", "the only runner", None)
        .expect("an attempt starts");
    store
        .record_step(&id, 1, fetch_reporting(Some(REACHED)))
        .expect("the step records");
    assert!(
        store
            .job(&id)
            .expect("reads")
            .expect("there")
            .attempts
            .last()
            .expect("one attempt")
            .commit
            .is_none(),
        "no declaration, no comparison"
    );
}

/// **The one that costs something: a rollback reads, and does not preserve.**
///
/// A hub of an earlier version reads this record perfectly well — that is
/// the half `RECORD_VERSION` staying at 1 buys, and it is the important half
/// — and then erases both new fields the first time it writes the record
/// back. Modelled on the real `job.json` the real store just wrote, through
/// the attempt shape that hub had.
///
/// Every path that writes a record again is a path that costs the field:
/// `record_state` on a job still in flight, and `note_outputs_fetched` on a
/// job that is already over, the first time a requester fetches its outputs.
/// Nothing here can be fixed by a version number, because a version number
/// that refused the record would cost the operator the whole state root
/// instead.
#[test]
fn an_older_hub_reads_the_field_and_erases_it_on_its_next_write() {
    let fixture = Fixture::with_fetch_reporting(Some(REACHED));
    let text = fixture.text();
    let whole: serde_json::Value = serde_json::from_str(&text).expect("the record is json");

    // It reads. Nothing about the record is unreadable to the older shape,
    // which is what the frozen `RECORD_VERSION` is worth.
    let read: Vec<OldAttempt> = serde_json::from_value(whole["attempts"].clone())
        .expect("an older hub reads a record this build wrote");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].steps.len(), 1, "every step it knew about is there");
    assert_eq!(read[0].steps[0].step, "fetch");
    assert_eq!(read[0].why, "the only runner");

    // And then it writes it back, and the revision is gone — both on the
    // step and on the attempt.
    let written = serde_json::to_string(&read).expect("the older hub re-encodes");
    assert!(
        !written.contains(REACHED),
        "this assertion failing would be good news; it does not fail: {written}"
    );
    assert!(!written.contains("ran_commit"), "{written}");
    assert!(!written.contains("\"commit\""), "{written}");

    // What that costs, said as the thing a reader would see: a record that
    // *had* an answer now has none, and a later hub reading it back cannot
    // tell it from a runner that never said anything. The divergence is not
    // wrong afterwards — it is absent, which is the one state this whole
    // change exists to tell apart from agreement.
    let after: Vec<OldAttempt> =
        serde_json::from_str(&written).expect("the erased record still reads");
    assert_eq!(after[0].steps[0].duration_ms, 1_200, "everything else kept");
}

/// The same loss, through the real store rather than through a model: a
/// record this build wrote keeps its field across **this** build's own
/// rewrites. The asymmetry is the point — the field survives every write by
/// a hub that knows it, and no write by one that does not.
#[test]
fn a_hub_that_knows_the_field_keeps_it_across_a_rewrite() {
    let fixture = Fixture::with_fetch_reporting(Some(REACHED));
    fixture
        .store
        .record_state(&fixture.id, JobState::Running)
        .expect("the record is written again");
    assert_eq!(
        fixture.commit().and_then(|commit| commit.ran).as_deref(),
        Some(REACHED),
        "a rewrite by a hub that knows the field keeps it"
    );
}

// ---------------------------------------------------------------------------
// 3. The requester's end
// ---------------------------------------------------------------------------

/// **New hub → old `herdctl`.** The verdict frame grew one key, and the
/// shape an older `herdctl` had still decodes it.
///
/// This is 04b §3.5's second matrix, with its artefact corrected: the cell's
/// answer was right and `herdctl` never reads a record, so what is really
/// being asserted is a *frame* crossing a version boundary.
#[test]
fn an_older_herdctl_decodes_a_verdict_carrying_the_new_key() {
    let verdict = herd_proto::FromHub::JobFinished {
        job_id: JobId::new("j0000000001"),
        outcome: JobOutcome::Exited { code: 0 },
        metrics: herd_proto::JobMetrics::default(),
        outputs: herd_proto::JobOutputs::default(),
        commit: Some(CommitRun {
            declared: DECLARED.to_owned(),
            ran: Some(REACHED.to_owned()),
        }),
    };
    let frame = serde_json::to_string(&verdict).expect("it encodes");
    assert!(frame.contains(REACHED), "the hub says it: {frame}");
    match serde_json::from_str::<OldFromHub>(&frame) {
        Ok(OldFromHub::JobFinished { job_id, outcome }) => {
            assert_eq!(job_id, JobId::new("j0000000001"));
            assert_eq!(outcome, JobOutcome::Exited { code: 0 });
        }
        other => panic!("an older herdctl has to read this frame: {other:?}"),
    }
}

/// **Old hub → new `herdctl`.** A verdict frame with no such key at all.
///
/// `None`, and `None` is "nobody said" — which is what the requester-facing
/// document turns into `null` rather than into `false`.
#[test]
fn a_verdict_from_an_older_hub_leaves_the_question_unanswered() {
    let older = r#"{"type":"job_finished","job_id":"j0000000001",
                    "outcome":{"type":"exited","code":0}}"#;
    match serde_json::from_str::<herd_proto::FromHub>(older) {
        Ok(herd_proto::FromHub::JobFinished { commit, .. }) => assert_eq!(
            commit, None,
            "an older hub said nothing, which is not the same as saying it matched"
        ),
        other => panic!("this build has to read an older hub's verdict: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. The consumer's side of the same boundary
// ---------------------------------------------------------------------------

/// **A declaration naming `{ran}`, handed to a runner too old to fill it.**
///
/// The failure mode has to be *nothing happening*, because on the day this
/// ships a consumer may well update its declaration before every machine in
/// the fleet has been restarted. An older runner leaves the brace in the
/// hook, the shell treats it as the ordinary filename it is, the hook
/// succeeds, and the runner reports no revision — which is the state this
/// change was careful to keep harmless.
///
/// Run with a real shell, because the claim is about what `sh` does with
/// `> '{ran}'` and not about what anybody believes it does. It is also the
/// argument against passing the path in an environment variable instead: an
/// unset variable makes the same line `> ""`, and every job of that consumer
/// would fail its `fetch` on every machine that had not been restarted.
#[test]
fn a_hook_naming_ran_on_a_runner_too_old_to_fill_it_still_succeeds() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("printf '%s' deadbeef > '{ran}'")
        .current_dir(dir.path())
        .status()
        .expect("a shell runs");
    assert!(
        status.success(),
        "an unfilled {{ran}} must not fail a consumer's fetch: {status:?}"
    );
    assert!(
        dir.path().join("{ran}").exists(),
        "the brace became a filename, which is the whole of the damage"
    );

    // And the counter-case, which is the one that decided the design: the
    // same line with an environment variable nobody set.
    let unset = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("printf '%s' deadbeef > \"$HERD_RAN_COMMIT_FILE\"")
        .current_dir(dir.path())
        .status()
        .expect("a shell runs");
    assert!(
        !unset.success(),
        "an unset variable fails the step, which is why {{ran}} is a token and not one"
    );
}
