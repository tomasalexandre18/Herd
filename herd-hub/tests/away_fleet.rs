//! D140 against the real binaries: a fleet that is **away** queues a job, and
//! only an **empty roster** refuses one.
//!
//! The hub used to hold two answers to one fleet state. A job already queued
//! was told `no runner is connected to this hub` and waited; a *new*
//! submission in exactly that state was refused outright, so which answer a
//! job got depended only on whether it arrived a moment before or after the
//! last link dropped — and that wait sentence was unreachable for a newly
//! submitted job. D88's own "what we did not do" says the opposite: a job no
//! connected runner could ever fit **waits**, because a fleet is not a closed
//! world. A transient gap is that same situation with weaker evidence.
//!
//! Real processes, because what changed is what a **deployed** hub does with a
//! submission, and the thing to prove is a `herdctl` exit code: every hook
//! submitting during a hub restart, a runner restart or a brief disconnection
//! used to get 125.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with the other files that drive the real binaries.

mod real_herd;

use real_herd::Herd;

/// A job queued for an away fleet has to be able to **die**, and the sentence
/// it is told has to be the one the queue already writes.
///
/// Four facts in one test, because they are one story and a wait that cannot
/// end is a worse defect than the refusal it replaced:
///
/// * the roster is populated and no runner is connected, and the submission
///   is **accepted** rather than refused;
/// * its record carries the sentence `matcher` already had, inside M8's
///   explained wait — the position, what is ahead and why (E-04, E-36);
/// * E-04's restatement reaches a follower of it, so an empty fleet is not
///   silence;
/// * and the job runs the moment a runner joins.
#[tokio::test(flavor = "multi_thread")]
async fn a_submission_with_nobody_connected_is_queued_explained_and_runs_when_a_runner_joins() {
    let herd = Herd::new();
    // E-04's cadence, short enough to observe: a follower gets at most
    // W / this + 1 lines over a wait of W.
    let _hub = herd
        .start_hub_with("hub", &["--wait-restatement", "1"])
        .await;

    // ---- One machine approved, and nothing connected. This is a hub
    // restarting, a runner restarting, or a link that has just dropped.
    let _token = herd.approve("the-real-runner");
    let listed = herd.herdctl(&["runner", "list"]).await;
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("the-real-runner"),
        "the roster is populated: {}",
        String::from_utf8_lossy(&listed.stdout)
    );

    let submission = herd
        .submit_with(&[], "printf 'ran after the fleet came back\\n'")
        .await;
    assert!(
        submission.status.success(),
        "a fleet that is away queues a job, it does not refuse one (D88, D140): {}",
        String::from_utf8_lossy(&submission.stderr)
    );
    let job = String::from_utf8_lossy(&submission.stdout)
        .trim()
        .to_owned();

    // ---- The one sentence, not a second one. The words are `matcher`'s, and
    // they arrive inside the explained wait every other queued job gets.
    herd.wait_until("the hub says why the job is waiting", || {
        !herd.waiting_for(&job).is_empty()
    })
    .await;
    let why = herd.waiting_for(&job);
    assert!(
        why.contains("no runner is connected to this hub"),
        "a job queued a moment ago must get the same words as one queued a minute ago: {why}"
    );
    for part in ["queued 1st of 1", "nothing is ahead of it"] {
        assert!(
            why.contains(part),
            "E-04 asks for the position and what is ahead too: {why}"
        );
    }

    // ---- E-04: a queued stream reports, it does not go quiet. D118's
    // keepalive is beside this, not instead of it.
    let mut following = herd.start_wait(&job);
    herd.wait_until("the follower is told where the job stands", || {
        herd.said("wait").contains("queued and has not started")
            && herd
                .said("wait")
                .contains("no runner is connected to this hub")
    })
    .await;

    // ---- And the fleet comes back. The owner's PC woke up, or the runner
    // finished restarting: the job that was waiting runs.
    let mut runner = herd.start_runner();
    herd.wait_until("the job that waited for an empty fleet runs", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    let record = herd.record(&job);
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0}),
        "the job ran to its own verdict: {record}"
    );
    assert!(
        herd.log(&job).contains("ran after the fleet came back"),
        "and its output is the job's own: {}",
        herd.log(&job)
    );
    following.ask_it_to_stop();
    runner.ask_it_to_stop();
}

/// Every runner draining or switched off is a fleet that is **away**, not one
/// that can never work — E-33's switch is explicitly a temporary owner
/// action — so such a submission is queued with the words the queue already
/// has for it.
///
/// The switch rather than a drain, because a drain ends with the process and
/// the state would not hold still for an assertion: a runner switched off by
/// its owner stays connected, stays on the roster, and claims nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_submission_with_every_runner_switched_off_is_queued_and_explained() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // ---- The owner withdraws her machine. Temporary, by E-33's own
    // definition.
    let said = herd
        .herdctl(&[
            "switch",
            "the-real-runner",
            "off",
            "--reason",
            "somebody is gaming on it",
        ])
        .await;
    assert!(
        said.status.success(),
        "the switch failed: {}",
        String::from_utf8_lossy(&said.stderr)
    );
    // The hub's own view of the position, which is what the claim step reads:
    // asserted before the submission, so this test cannot pass by racing the
    // frame that carries it.
    let seen = herd.switches().await;
    assert!(
        seen.contains("the-real-runner: off"),
        "this hub honours the position: {seen}"
    );

    let submission = herd.submit_with(&[], "printf 'never\\n'").await;
    assert!(
        submission.status.success(),
        "a machine its owner withdrew for the evening is away, not broken (D140): {}",
        String::from_utf8_lossy(&submission.stderr)
    );
    let job = String::from_utf8_lossy(&submission.stdout)
        .trim()
        .to_owned();
    herd.wait_until("the hub says why the job is waiting", || {
        !herd.waiting_for(&job).is_empty()
    })
    .await;
    let why = herd.waiting_for(&job);
    assert!(
        why.contains("no connected runner can run it") && why.contains("switched off"),
        "the wait carries the owner's own position, which is E-33's point: {why}"
    );
    assert!(
        why.contains("queued 1st of 1"),
        "and it is M8's explained wait like any other: {why}"
    );
    runner.ask_it_to_stop();
}

/// The configuration mistake, which must **not** wait for ever: a state root
/// on which no runner is approved refuses a submission, by name.
///
/// This is the whole of what still refuses, and it is a fact the hub holds on
/// its disk rather than a guess about the moment — it counts the roster out
/// loud at startup in these same words. A job queued against an empty roster
/// would wait for nobody coming.
#[tokio::test(flavor = "multi_thread")]
async fn a_submission_with_an_empty_roster_is_refused_by_name() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    assert!(
        herd.said("hub")
            .contains("0 runner(s) are approved on this state root"),
        "the hub says so at startup: {}",
        herd.said("hub")
    );

    let refused = herd.submit_with(&[], "printf 'never\\n'").await;
    assert!(
        !refused.status.success(),
        "a roster with nothing on it is a configuration mistake, not a fleet that is away"
    );
    assert_eq!(
        refused.status.code(),
        Some(125),
        "a refused submission has no verdict to report (D20)"
    );
    let why = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        why.contains("0 runner(s) are approved on this state root"),
        "the refusal names the state the hub is in: {why}"
    );
    assert!(
        why.contains("herdctl runner approve"),
        "and what to do about it: {why}"
    );
    // No id was ever printed, so a falling-back requester has D139's handle
    // on it: this invocation left nothing it can name.
    assert!(
        !why.contains(herdctl::JOB_ID_PREFIX),
        "a refused submission names no job: {why}"
    );
    // And which of D139's three states this is: the refusal is the hub
    // saying it minted nothing, so running the work again is safe. Found
    // while writing this test, and fixed with it — every `Refused` on the
    // submission path now precedes the minting, which is what makes the
    // line provable (D142).
    assert!(
        why.contains(herdctl::NO_JOB_LINE) && !why.contains(herdctl::JOB_UNKNOWN_LINE),
        "a refused submission leaves no job, and must not send a requester \
         looking for one: {why}"
    );

    // And a dry run says the same thing, which is E-07's own requirement: a
    // dry run answering "it would wait" about the one state a submission
    // refuses would be two answers to one fleet state again.
    let judged = herd
        .herdctl(&["would-run", "--", "/bin/sh", "-c", "true"])
        .await;
    let said = String::from_utf8_lossy(&judged.stdout).into_owned();
    assert!(
        said.contains("it would be refused at submission")
            && said.contains("0 runner(s) are approved on this state root"),
        "a dry run must predict the refusal, not a wait: {said}"
    );
}

/// **E-09's deadline is the only bound on a job nothing can run**, which D88
/// says in so many words — so it has to reach a job waiting for an away
/// fleet, or "waits" means "waits for ever".
///
/// Judged in `matcher::place` before anything about the fleet, including an
/// empty one, and the pass that judges it is fired by the queue's own ageing
/// loop: no runner has to be connected for a deadline to fire.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_still_cancels_a_job_waiting_for_an_away_fleet() {
    let herd = Herd::new();
    // An ageing step of four seconds, which is a re-judging pass a second.
    // Nothing else can provoke one here: there is no runner to join, no job to
    // end, and no second submission — which is precisely the state this test
    // is about, and read against the default step a quarter of it is clamped
    // to a minute (D100's lesson, applied before it bit).
    let _hub = herd.start_hub_with("hub", &["--aging-step", "4"]).await;
    let _token = herd.approve("the-real-runner");

    let job = herd
        .submit_ok(&["--deadline", "+2s"], "printf 'never\\n'")
        .await;
    herd.wait_until("the deadline cancels the job", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    let outcome = herd.record(&job)["state"]["outcome"].clone();
    assert_eq!(outcome["type"], "cancelled", "{outcome}");
    let reason = outcome["reason"].as_str().unwrap_or_default();
    assert!(
        reason.starts_with("cancelled (deadline)"),
        "E-09's own words, and D88's only bound on a job nothing can run: {reason}"
    );
}

/// And a cancel still reaches such a job, which is the other half of the same
/// question: a requester that changed its mind must not be told the hub has
/// nothing to cancel because nothing is connected.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_still_reaches_a_job_waiting_for_an_away_fleet() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _token = herd.approve("the-real-runner");

    let job = herd.submit_with(&[], "printf 'never\\n'").await;
    assert!(job.status.success());
    let job = String::from_utf8_lossy(&job.stdout).trim().to_owned();
    herd.wait_until("the hub says why the job is waiting", || {
        !herd.waiting_for(&job).is_empty()
    })
    .await;

    let cancelled = herd.herdctl(&["cancel", &job]).await;
    assert!(
        cancelled.status.success(),
        "the cancel failed: {}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
    let record = herd.record(&job);
    assert_eq!(
        record["state"]["type"], "finished",
        "a queued job is cancelled where it stands, with no runner to ask: {record}"
    );
    let outcome = record["state"]["outcome"].clone();
    assert_eq!(outcome["type"], "cancelled", "{outcome}");
    let reason = outcome["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("cancelled this job before it started"),
        "and it says who ended it: {reason}"
    );
}
