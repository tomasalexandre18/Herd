//! M6 against a live hub: deduplication, the operator's raise, and the
//! refusals that keep a priority from being guessed (E-05, E-18, E-20).
//!
//! The *order* of the queue is arithmetic and is tested where it lives, in
//! `herd_hub::queue`, with simulated time; the end-to-end order of two real
//! `AssignJob` frames is in `priorities.rs`, with the real binaries. What is
//! here is everything that needs a hub, a runner and a requester talking to
//! each other over a socket: one execution answering two submissions with
//! one log, and the sentences the hub sends back when somebody asks for
//! something it will not do.

mod support;

use futures_util::{SinkExt, StreamExt};
use herd_hub::JobStore;
use herd_proto::{from_frame, to_frame, FromHub, JobOutcome, LabelSet, Priority, Role, ToHub};
use herd_proto::{JobId, PROTOCOL_VERSION};
use support::{labels, shell, Harness, PATIENCE};
use tokio_tungstenite::tungstenite::Message;

/// E-05, and the attack behind it: **two completed waits, one execution**.
/// Both callers must get the same exit code *and* the same log, and the
/// second must not hang — and exactly one process must have run.
#[tokio::test(flavor = "multi_thread")]
async fn two_identical_submissions_are_one_execution_with_one_log_for_both() {
    let harness = Harness::with_hub_and_runner().await;
    // Only stdout, and slowly enough that the second submission certainly
    // lands while the first is running: the joiner has to be given the
    // output that was produced *before* it joined, which is the half of
    // "the same log" that a naive join loses.
    let command = shell("for i in 1 2 3 4 5; do printf 'line %s\\n' \"$i\"; sleep 0.1; done");
    let requester = harness.requester();

    let first_command = command.clone();
    let first_config = requester.clone();
    let first = tokio::spawn(async move {
        let mut log = Vec::new();
        let finished = herdctl::run_job(
            &first_config,
            first_command,
            LabelSet::default(),
            Priority::Step,
            herdctl::Declared::default(),
            |_, data| log.extend_from_slice(data),
            |_| {},
        )
        .await
        .expect("the first submission runs");
        (finished, String::from_utf8(log).expect("utf-8"))
    });

    // Wait until the job is really under way before the second submission,
    // so that this is a join onto a *running* job and not a race with the
    // mint.
    support::wait_until("the first job has started", || {
        harness
            .store
            .unfinished_jobs()
            .expect("the queue reads")
            .iter()
            .any(|record| !matches!(record.state, herd_hub::JobState::Queued))
    })
    .await;

    let mut joined_notes = Vec::new();
    let mut joined_log = Vec::new();
    let joiner = herdctl::run_job(
        &requester,
        command,
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared::default(),
        |_, data| joined_log.extend_from_slice(data),
        |said| {
            if let herdctl::Said::Note(note) = said {
                joined_notes.push(note.to_owned());
            }
        },
    )
    .await
    .expect("the second submission runs");
    let joined_log = String::from_utf8(joined_log).expect("utf-8");

    let (ran, first_log) = first.await.expect("the first submission's task");

    // One id for both callers.
    assert_eq!(
        ran.job_id, joiner.job_id,
        "two identical submissions must be one job"
    );
    // The same verdict.
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(ran.outcome, joiner.outcome);
    // And the same log, byte for byte, with nothing missing from the front
    // and nothing written twice.
    assert_eq!(
        first_log, "line 1\nline 2\nline 3\nline 4\nline 5\n",
        "the submitter saw the whole log"
    );
    assert_eq!(
        joined_log, first_log,
        "the joiner must see the same log as the caller it joined"
    );
    // The joiner was told why it was given an id it did not expect.
    assert!(
        joined_notes
            .iter()
            .any(|note| note.contains("joined that job") && note.contains("One execution")),
        "the joiner should be told it joined: {joined_notes:?}"
    );
    // One execution: one attempt on the record, and the record says it
    // answers two submissions.
    let record = harness
        .store
        .job(&ran.job_id)
        .expect("the record reads")
        .expect("it is there");
    assert_eq!(record.submissions, 2);
    assert_eq!(
        record.attempts.len(),
        1,
        "one execution, not two: {:?}",
        record.attempts
    );
    // And one job on the disk, not two.
    let jobs = std::fs::read_dir(harness.state_dir().join("jobs"))
        .expect("the jobs directory reads")
        .count();
    assert_eq!(jobs, 1, "two submissions left two job directories");
}

/// The attack: a priority that is not one of the four must be refused **by
/// name**, with a frame the caller can read — never clamped to the nearest
/// level and never quietly defaulted.
#[tokio::test(flavor = "multi_thread")]
async fn a_priority_that_is_not_one_of_the_four_is_refused_by_name() {
    let harness = Harness::with_hub_and_runner().await;
    // A *name* that is not one of the four: the refusal lists all four, so
    // the caller does not have to go looking for them. Case matters, and so
    // does a trailing space — a level is one of four names, not something
    // tidied up until it matches one.
    for bad in [
        r#""urgent""#,
        r#""URGENT""#,
        r#""Merger""#,
        r#""merger ""#,
        r#""""#,
    ] {
        let reason = refusal_of_a_submission_with_priority(&harness.hub_url, bad).await;
        for level in Priority::ALL {
            assert!(
                reason.contains(level.name()),
                "the refusal of {bad} should name {}: {reason}",
                level.name()
            );
        }
    }
    // A priority that is not even a name — a number, say, which is how a
    // queue with levels an operator can invent would spell one — is refused
    // too, naming what it got.
    for bad in ["3", "null", "[]", "{}"] {
        let reason = refusal_of_a_submission_with_priority(&harness.hub_url, bad).await;
        assert!(
            reason.contains("invalid type") || reason.contains("invalid value"),
            "the refusal of {bad} should say what was wrong with it: {reason}"
        );
    }
    // And a submission with no priority at all is refused too, rather than
    // given a level nobody asked for.
    let mut socket = requester_socket(&harness.hub_url).await;
    socket
        .send(Message::text(
            r#"{"type":"submit_job","asked":{"command":["true"]}}"#.to_owned(),
        ))
        .await
        .expect("the frame goes out");
    match next(&mut socket).await {
        Some(FromHub::Refused { reason }) => assert!(
            reason.contains("priority"),
            "the refusal should name the missing field: {reason}"
        ),
        other => panic!("a submission with no priority got {other:?}"),
    }
    // Nothing of any of that was minted.
    assert!(
        harness
            .store
            .unfinished_jobs()
            .expect("the queue reads")
            .is_empty(),
        "a refused submission must leave no job behind"
    );
}

/// E-18's operator raise, and the two things it will not do.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_raises_a_queued_job_and_can_never_lower_one() {
    let harness = Harness::with_hub_and_runner().await;
    // A job that waits, so that there is something to reorder: the runner
    // this harness starts offers no capability called `kvm`, so the job
    // stays queued with its reason on its record (E-26, D38).
    let job_id = herdctl::submit_job(
        &harness.requester(),
        shell("printf 'should not run yet\\n'"),
        labels(&["kvm"]),
        Priority::Producer,
        herdctl::Declared::default(),
    )
    .await
    .expect("a job needing a capability nobody offers is accepted, never refused");

    let note = herdctl::raise_job(&harness.requester(), job_id.clone(), Priority::Merger)
        .await
        .expect("the raise is answered");
    assert_eq!(
        note,
        "this job was at priority producer and is now at merger"
    );

    let note = herdctl::raise_job(&harness.requester(), job_id.clone(), Priority::Merger)
        .await
        .expect("the raise is answered");
    assert!(note.contains("already at priority merger"), "{note}");

    let note = herdctl::raise_job(&harness.requester(), job_id.clone(), Priority::Step)
        .await
        .expect("the raise is answered");
    assert!(
        note.contains("only ever raised, never lowered"),
        "a request to lower a job must be refused in words: {note}"
    );

    let record = harness
        .store
        .job(&job_id)
        .expect("the record reads")
        .expect("it is there");
    assert_eq!(record.priority, Priority::Merger);
    assert_eq!(record.state, herd_hub::JobState::Queued);
}

/// The attack: raising a job that is over. Refused by name, through the
/// store's one write door, and nothing written (D54).
#[tokio::test(flavor = "multi_thread")]
async fn raising_a_job_that_is_over_is_refused_by_name() {
    let harness = Harness::with_hub_and_runner().await;
    let finished = support::run_job(&harness.requester(), shell("exit 7"), |_, _| {})
        .await
        .expect("the job runs");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 7 });

    let error = herdctl::raise_job(
        &harness.requester(),
        finished.job_id.clone(),
        Priority::Merger,
    )
    .await
    .expect_err("a job that is over cannot be raised");
    let said = error.to_string();
    assert!(said.contains(finished.job_id.as_str()), "{said}");
    assert!(said.contains("is over"), "{said}");

    let record = harness
        .store
        .job(&finished.job_id)
        .expect("the record reads")
        .expect("it is there");
    assert_eq!(
        record.priority,
        Priority::Step,
        "the level on a record that is over is the one it had"
    );
}

/// Raising a job this hub never minted is a terminal no with the id in it
/// (D13), not a hang and not a job brought into being.
#[tokio::test(flavor = "multi_thread")]
async fn raising_a_job_that_does_not_exist_is_refused_with_its_id() {
    let harness = Harness::with_hub_and_runner().await;
    let error = herdctl::raise_job(
        &harness.requester(),
        JobId::new("j0000004242"),
        Priority::Merger,
    )
    .await
    .expect_err("there is no such job");
    assert!(error.to_string().contains("j0000004242"), "{error}");
}

/// The attack: a job dispatched twice. A reorder touches the claim path, and
/// E-21's record allows exactly two attempts — so a job that could be
/// claimed by two passes at once would burn both of them on one run.
///
/// Twelve jobs at four levels submitted from twelve connections at once, so
/// that the passes they trigger overlap: every job must end with exactly one
/// attempt, and every one of their commands must have run exactly once.
#[tokio::test(flavor = "multi_thread")]
async fn no_job_is_claimed_twice_when_passes_overlap() {
    let harness = Harness::with_hub_and_runner().await;
    let mut submissions = Vec::new();
    for (number, priority) in (0..12).map(|n| (n, Priority::ALL[n % 4])) {
        let config = harness.requester();
        submissions.push(tokio::spawn(async move {
            support::run_job_at(
                &config,
                shell(&format!("printf 'job %s ran\\n' {number}")),
                priority,
            )
            .await
            .unwrap_or_else(|e| panic!("job {number} runs: {e}"))
        }));
    }
    for (number, submission) in submissions.into_iter().enumerate() {
        let finished = submission.await.expect("the submission's task");
        assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
        let record = harness
            .store
            .job(&finished.job_id)
            .expect("the record reads")
            .expect("it is there");
        assert_eq!(
            record.attempts.len(),
            1,
            "job {number} ({}) was attempted {} times: {:?}",
            finished.job_id,
            record.attempts.len(),
            record.attempts
        );
        // And the log holds its line once, so the command itself ran once.
        let log = support::log_of(harness.state_dir(), &finished.job_id);
        assert_eq!(
            log.matches(&format!("job {number} ran")).count(),
            1,
            "job {number} ran more than once: {log:?}"
        );
    }
}

/// E-20: everything is in the one queue, whatever its level — and a job that
/// waits says where it is in that queue, what it is weighed at and when that
/// changes, in the sentence an operator actually reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_job_says_where_it_stands_with_its_figures() {
    let harness = Harness::with_hub_and_runner().await;
    let mut queued = Vec::new();
    for priority in [Priority::Producer, Priority::Step, Priority::Merger] {
        queued.push(
            herdctl::submit_job(
                &harness.requester(),
                shell(&format!("printf '{} waits\\n'", priority.name())),
                labels(&["kvm"]),
                priority,
                herdctl::Declared::default(),
            )
            .await
            .expect("accepted, never refused"),
        );
    }
    // Every one of them is in the one queue the hub recovers, none of them
    // has started, and each says why in a sentence with its figures.
    //
    // Waited for: a submission is answered before the dispatch pass that
    // judges it has run, so the reason is written a moment *after* the id
    // comes back. Read on sight, this panicked with "{job_id} is queued and
    // says nothing about why" — the hub's other true sentence — on a host
    // slow enough to put the pass behind the answer (D101).
    let queued_now = queued.clone();
    let store = std::sync::Arc::clone(&harness.store);
    support::wait_until("every queued job says why it waits", move || {
        queued_now.iter().all(|job_id| {
            store
                .job(job_id)
                .expect("the record reads")
                .expect("it is there")
                .waiting_for
                .is_some()
        })
    })
    .await;
    let waiting = harness.store.unfinished_jobs().expect("the queue reads");
    assert_eq!(waiting.len(), 3);
    for (job_id, priority) in
        queued
            .iter()
            .zip([Priority::Producer, Priority::Step, Priority::Merger])
    {
        let record = harness
            .store
            .job(job_id)
            .expect("the record reads")
            .expect("it is there");
        let said = record
            .waiting_for
            .unwrap_or_else(|| panic!("{job_id} is queued and says nothing about why"));
        assert!(
            said.contains(&format!("at priority {}", priority.name())),
            "{said}"
        );
        assert!(said.contains("queued "), "{said}");
        assert!(
            said.contains("it is waiting because") && said.contains("kvm"),
            "the sentence must name what it is waiting for: {said}"
        );
        if priority == Priority::Merger {
            assert!(
                said.contains("at the highest priority there is"),
                "a merger has nothing to rise to: {said}"
            );
        } else {
            assert!(
                said.contains("rising to"),
                "a job below the top says when it rises: {said}"
            );
        }
    }
}

/// What the hub says to a submission whose `priority` is `written`, which is
/// put into the frame verbatim so that a test can send what no typed client
/// could.
async fn refusal_of_a_submission_with_priority(hub_url: &str, written: &str) -> String {
    let mut socket = requester_socket(hub_url).await;
    socket
        .send(Message::text(format!(
            r#"{{"type":"submit_job","asked":{{"command":["true"],"priority":{written}}}}}"#
        )))
        .await
        .expect("the frame goes out");
    match next(&mut socket).await {
        Some(FromHub::Refused { reason }) => {
            assert!(
                reason.contains("could not read"),
                "the refusal should say it could not read the frame: {reason}"
            );
            reason
        }
        other => panic!("a submission naming the priority {written} got {other:?}"),
    }
}

/// A requester connection, through the handshake, with nothing submitted.
async fn requester_socket(
    hub_url: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
        .await
        .expect("the hub accepts a connection");
    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: herdctl::requester_version().to_owned(),
        role: Role::Requester,
        token: None,
        name: Some("a-requester-with-its-own-frames".to_owned()),
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: Default::default(),
        host: herd_proto::HostSize::default(),
        preference: 0,
        load_milli: None,
        switch: herd_proto::Switch::On,
    };
    socket
        .send(Message::text(to_frame(&hello).expect("encodes")))
        .await
        .expect("the handshake goes out");
    match next(&mut socket).await {
        Some(FromHub::Welcome { .. }) => socket,
        other => panic!("the hub answered the handshake with {other:?}"),
    }
}

/// The next frame the hub sends, decoded.
async fn next(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Option<FromHub> {
    let message = tokio::time::timeout(PATIENCE, socket.next())
        .await
        .expect("the hub answers")?
        .expect("the frame reads");
    let text = message.into_text().expect("the hub speaks text");
    Some(from_frame(&text).unwrap_or_else(|e| panic!("{text} decodes: {e}")))
}
