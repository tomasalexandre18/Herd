//! The tests that define M2: the hub's state is files, and nothing is lost.
//!
//! What is asserted here is behaviour a requester or the first consumer can
//! see — a job that finishes across a restart, a wait that returns the right
//! code, a record that can be read while the job runs — and the records are
//! read as files, with no tool but a file read, because that is how the
//! consumer reads them (E-15, E-53).

use std::sync::Arc;

use herd_hub::{JobDirStore, JobState, JobStore, Mint, StateRootReport, RECORD_VERSION};
use herd_proto::{JobId, JobOutcome, LogStream};
use herd_runner::Session;

mod support;

use support::{
    events_of, file_state_of, job_dir, log_of, record_of, shell, wait_until, Harness, HubProcess,
};

/// The runner these tests start, carrying the token its approval minted.
///
/// An unapproved runner claims nothing (E-48), so the approval is made on the
/// state root before the hub is started on it — which is what an operator
/// really does, and what makes the approval survive the restarts this file is
/// about.
fn runner_config(
    hub_url: &str,
    token: &str,
    data_root: &support::DataRootDir,
) -> herd_runner::Config {
    herd_runner::Config {
        hub_url: hub_url.to_owned(),
        name: Some("durable-runner".to_owned()),
        token: Some(token.to_owned()),
        confinement: support::confinement(data_root),
        ..herd_runner::Config::default()
    }
}

/// E-44, the whole of it: a hub stops with a job running on its runner and a
/// job still waiting in its queue, and nothing is lost. The job finishes on
/// the runner while there is no hub at all; the result reaches the hub that
/// comes back; the queued job is handed to the runner by the new hub.
#[tokio::test]
async fn a_hub_that_restarts_loses_no_result_and_no_queued_job() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    // The operator approves the machine before the hub is started on this
    // root: an unapproved runner claims nothing (E-48), and an approval is a
    // file in the root, so it outlives every hub this test kills.
    let token = support::approve_on_disk(state.path(), "durable-runner");

    // ---- The hub that is about to die.
    let first_hub = HubProcess::start(state.path());
    // The runner holds its results outside any one connection, which is what
    // lets it redeliver after the hub comes back. M3 puts the reconnection
    // loop around this; here the test dials the second connection itself.
    let session = Session::new();
    let (data_root, confined) = support::prepared_runner("durable-runner");
    let first_link = tokio::spawn({
        let (config, session) = (
            runner_config(&first_hub.hub_url, &token, &data_root),
            session.clone(),
        );
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, session, confined).await }
    });
    first_hub.wait_for_runner().await;

    // A detached submission: the requester leaves with the id, and the job
    // outlives both it and the hub.
    let running = support::submit_job(
        &first_hub.requester(),
        shell("printf 'before the restart\\n'; sleep 1; printf 'after it\\n'; exit 7"),
    )
    .await
    .expect("the job is accepted");
    let state_dir = state.path().to_path_buf();
    wait_until("the job is running and its first line is recorded", || {
        file_state_of(&state_dir, &running) == "running"
            && log_of(&state_dir, &running).contains("before the restart")
    })
    .await;

    // ---- The hub dies, with the job running.
    first_hub.kill();
    // That this await returns at all is the proof the hub is gone: nothing of
    // it is left serving the runner's connection, so nothing of it can record
    // the result that is about to be produced.
    first_link
        .await
        .expect("the runner's link to the dead hub ends")
        .expect_err("a hub that vanished is not a connection that ended well");

    // The job finishes with no hub to tell. The runner holds the result.
    wait_until("the runner holds the result of the finished job", || {
        !session.holds_nothing()
    })
    .await;
    assert_eq!(
        file_state_of(state.path(), &running),
        "running",
        "with no hub, the record cannot have moved on"
    );

    // ---- What a hub starting on this state root recovers, before one does.
    // A job that was accepted and never dispatched is minted here too: it is
    // the queue the new hub inherits. M6 is what lets a requester put one
    // there while no runner is free; until then it is minted the way the hub
    // itself mints one.
    let (queued, recovered) = {
        let store = support::open_when_free(state.path());
        let queued = store
            .create_job(herd_hub::NewJob::new(shell("printf 'from the queue\\n'")))
            .expect("minted")
            .id;
        let recovered: Vec<(String, String)> = store
            .unfinished_jobs()
            .expect("the queue reads")
            .into_iter()
            .map(|record| (record.id.to_string(), record.state.name().to_owned()))
            .collect();
        (queued, recovered)
    };
    assert_eq!(
        recovered,
        vec![
            (running.to_string(), "interrupted".to_owned()),
            (queued.to_string(), "queued".to_owned()),
        ],
        "the queue a restarted hub inherits is what the directories say, with \
         the interrupted attempt named rather than left reading `running`"
    );
    let events = events_of(state.path(), &running);
    let named = events.last().expect("the interruption is a transition");
    assert_eq!(named["state"]["type"], "interrupted");
    assert!(
        named["state"]["reason"]
            .as_str()
            .expect("an interruption says what happened")
            .contains("the hub stopped"),
        "the record must name the interruption: {named}"
    );

    // ---- The hub comes back on the same state root, and the runner with it.
    let second_hub = HubProcess::start(state.path());
    let second_link = tokio::spawn({
        let (config, session) = (
            runner_config(&second_hub.hub_url, &token, &data_root),
            session.clone(),
        );
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, session, confined).await }
    });

    // The result is redelivered and recorded: the work is not lost.
    wait_until("the redelivered result is recorded", || {
        file_state_of(&state_dir, &running) == "finished"
    })
    .await;
    assert_eq!(
        record_of(state.path(), &running)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 7}),
        "the outcome is recorded as the runner reported it"
    );
    // And the log is whole, tail included. The job printed its last line while
    // there was no hub at all: those chunks were held by the runner, sent again
    // on the connection that came after, and written in the order they were
    // produced. This is the promise M2 could not make — it recorded the loss
    // honestly and left the recovery to M3 (D36) — and the one that matters,
    // because a build log missing its last line is a build log missing the
    // error.
    assert_eq!(
        log_of(state.path(), &running),
        "before the restart\nafter it\n",
        "the log must hold everything the job printed, including what it \
         printed while no hub existed"
    );
    assert!(
        !log_of(state.path(), &running).contains("never reached this log"),
        "nothing was lost, so nothing may be named as lost: {}",
        log_of(state.path(), &running)
    );
    // And the runner is told it may forget it.
    wait_until("the hub acknowledges the result", || {
        session.holds_nothing()
    })
    .await;

    // The inherited queue is dispatched to the runner that joined the new hub.
    wait_until("the queued job runs and ends", || {
        file_state_of(&state_dir, &queued) == "finished"
    })
    .await;
    assert_eq!(
        record_of(state.path(), &queued)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0})
    );
    assert_eq!(log_of(state.path(), &queued), "from the queue\n");

    second_link.abort();
}

/// E-03: submit without following, come back by id, and get the job's own exit
/// code. The log is a file afterwards, and it is the same bytes the stream
/// delivered.
#[tokio::test]
async fn a_detached_submission_can_be_waited_for_and_its_log_read_as_a_file() {
    let harness = Harness::with_hub_and_runner().await;
    let job_id = support::submit_job(
        &harness.requester(),
        shell("printf 'one\\n'; printf 'two\\n' >&2; exit 75"),
    )
    .await
    .expect("the job is accepted");

    let finished = herdctl::wait_for_job(&harness.requester(), job_id.clone(), |_, _| {}, |_| {})
        .await
        .expect("the wait ends with the job");
    assert_eq!(finished.job_id, job_id);
    assert_eq!(
        finished.outcome,
        JobOutcome::Exited { code: 75 },
        "a wait reports the job's own code, which E-02 reads as absent or leased"
    );

    let log = log_of(harness.state_dir(), &job_id);
    assert!(log.contains("one\n"), "the log file holds stdout: {log:?}");
    assert!(log.contains("two\n"), "the log file holds stderr: {log:?}");

    // Waiting again, on a job that is already over, is answered from the
    // record: that is what makes an id enough, an hour or a restart later.
    let again = herdctl::wait_for_job(&harness.requester(), job_id.clone(), |_, _| {}, |_| {})
        .await
        .expect("a job that is over is still waitable");
    assert_eq!(again.outcome, finished.outcome);
}

/// The log file is the stream, byte for byte. A requester that followed the
/// job and a reader that opens the file afterwards see the same output.
#[tokio::test]
async fn the_log_file_equals_what_the_stream_delivered() {
    let harness = Harness::with_hub_and_runner().await;
    let mut streamed: Vec<u8> = Vec::new();
    let finished = support::run_job(
        &harness.requester(),
        // Enough output, in enough chunks, that a difference would show.
        shell("for i in $(seq 1 200); do printf 'line %s\\n' \"$i\"; printf 'err %s\\n' \"$i\" >&2; done"),
        |_stream, data| streamed.extend_from_slice(data),
    )
    .await
    .expect("the job runs to its end");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });

    let log = log_of(harness.state_dir(), &finished.job_id);
    assert_eq!(
        log.as_bytes(),
        streamed,
        "the log on disk is not the log that was streamed"
    );
    assert!(log.contains("line 200\n"), "the log is complete");
}

/// D18: a job's output is bytes. A NUL survives, a byte that is no UTF-8 at
/// all survives, and the `log` file is byte for byte what the process wrote —
/// on the wire, in the stream a requester follows, and on the hub's disk.
#[tokio::test]
async fn a_job_s_output_reaches_the_record_byte_for_byte() {
    let harness = Harness::with_hub_and_runner().await;
    let mut streamed: Vec<u8> = Vec::new();
    // Three bytes no JSON string could have carried: a NUL, 0xff, 0xfe.
    let finished = support::run_job(
        &harness.requester(),
        shell(r"printf 'a\000b\377\376c\n'"),
        |_stream, data| streamed.extend_from_slice(data),
    )
    .await
    .expect("the job runs to its end");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });

    let written: &[u8] = b"a\0b\xff\xfec\n";
    assert_eq!(
        streamed, written,
        "the stream a requester follows was rewritten"
    );
    let on_disk = std::fs::read(job_dir(harness.state_dir(), &finished.job_id).join("log"))
        .expect("the log file reads");
    assert_eq!(
        on_disk, written,
        "the log on the hub's disk is not what the process wrote"
    );
    assert!(
        !on_disk.windows(3).any(|w| w == [0xef, 0xbf, 0xbd]),
        "an invalid byte was replaced by U+FFFD: {on_disk:?}"
    );
}

/// D22: the log is always there. A job that prints nothing used to leave no
/// `log` file at all, so a consumer reading the record got ENOENT for a
/// perfectly ordinary job.
#[tokio::test]
async fn a_job_that_prints_nothing_still_has_a_log_file() {
    let harness = Harness::with_hub_and_runner().await;
    let job_id = support::submit_job(&harness.requester(), shell("exit 0"))
        .await
        .expect("the job is accepted");
    let log = job_dir(harness.state_dir(), &job_id).join("log");
    // From the job's first moment, not from its first byte of output.
    assert!(
        log.is_file(),
        "{} must exist as soon as the job does",
        log.display()
    );
    assert_eq!(std::fs::read(&log).expect("it reads").len(), 0);

    let finished = herdctl::wait_for_job(&harness.requester(), job_id, |_, _| {}, |_| {})
        .await
        .expect("the job ends");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
    assert!(log.is_file(), "and it is still there afterwards");
}

/// E-15's core and E-53: while the job is still running, its directory is
/// plain files that mean something — the record, the transitions so far, and
/// the output so far.
#[tokio::test]
async fn a_job_s_record_is_readable_while_it_is_still_running() {
    let harness = Harness::with_hub_and_runner().await;
    let job_id = support::submit_job(
        &harness.requester(),
        shell("printf 'working\\n'; sleep 2; printf 'done\\n'"),
    )
    .await
    .expect("the job is accepted");
    let state_dir = harness.state_dir().to_path_buf();

    wait_until("the job's output has started arriving", || {
        log_of(&state_dir, &job_id).contains("working")
    })
    .await;

    // Read while it runs, as a reader with no connection to the hub.
    let record = record_of(&state_dir, &job_id);
    assert_eq!(record["record_version"], herd_hub::RECORD_VERSION);
    assert_eq!(record["id"], job_id.as_str());
    assert_eq!(record["state"]["type"], "running");
    assert_eq!(record["command"][0], "/bin/sh");
    assert!(record["submitted_at"].is_string(), "{record}");
    assert!(
        record["metrics"]["peak_memory_bytes"].is_null(),
        "a job that is still running has no final figures: {}",
        record["metrics"]
    );
    assert_eq!(
        log_of(&state_dir, &job_id),
        "working\n",
        "the log is what the job has written so far, not what it will write"
    );

    // The transitions so far, in order, each with a moment. A line recording
    // something else — the attempt being handed to a runner, output that
    // never arrived — is not a transition, and is read as what it is.
    let events = events_of(&state_dir, &job_id);
    let states: Vec<&str> = events
        .iter()
        .filter_map(|event| event["state"]["type"].as_str())
        .collect();
    assert_eq!(states, ["queued", "assigned", "running"]);
    let attempts: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event["event"] == "attempt_started")
        .collect();
    assert_eq!(attempts.len(), 1, "one go at the job so far: {events:?}");
    assert_eq!(attempts[0]["attempt"], 1);
    assert_eq!(attempts[0]["runner"], "test-runner");
    for event in &events {
        assert_eq!(event["record_version"], herd_hub::RECORD_VERSION);
        assert!(
            event["at"].as_str().expect("a moment").ends_with('Z'),
            "a transition carries the moment it happened: {event}"
        );
    }
    // And room beside them for what a job keeps later (M11's artefacts).
    assert!(job_dir(&state_dir, &job_id).join("files").is_dir());

    let finished = herdctl::wait_for_job(&harness.requester(), job_id.clone(), |_, _| {}, |_| {})
        .await
        .expect("the job ends");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
    let events = events_of(&state_dir, &job_id);
    let states: Vec<&str> = events
        .iter()
        .filter_map(|event| event["state"]["type"].as_str())
        .collect();
    assert_eq!(
        states,
        ["queued", "assigned", "running", "finished"],
        "the end is a transition too: {events:?}"
    );
    // And the record carries the attempt whole: which runner it was on, when
    // it began, and no reason for its ending — because it ended by producing
    // the verdict, which is the answer to why.
    let record = record_of(&state_dir, &job_id);
    assert_eq!(record["attempts"].as_array().expect("attempts").len(), 1);
    assert_eq!(record["attempts"][0]["number"], 1);
    assert_eq!(record["attempts"][0]["runner"], "test-runner");
    assert!(record["attempts"][0]["ended"].is_null(), "{record}");
}

/// The index is derived, and only derived: delete it, start again, and every
/// job and the whole queue are still there, because they were never anywhere
/// but the job directories.
#[tokio::test]
async fn deleting_the_derived_index_loses_nothing() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let (finished, queued) = {
        let store = support::open_when_free(state.path());
        let finished = store
            .create_job(herd_hub::NewJob::new(vec!["true".to_owned()]))
            .expect("minted")
            .id;
        let queued = store
            .create_job(herd_hub::NewJob::new(vec!["false".to_owned()]))
            .expect("minted")
            .id;
        store
            .record_state(
                &finished,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("transition");
        store
            .append_log(&queued, LogStream::Stdout, 0, b"a queued job's log")
            .expect("appended");
        (finished, queued)
    };

    // Every way the index can be useless, one after another.
    for wreck in [
        Wreck::Deleted,
        Wreck::Corrupt,
        Wreck::Stale,
        Wreck::FromTheFuture,
        Wreck::Short,
    ] {
        wreck.apply(state.path());
        let store = support::open_when_free(state.path());
        let queue: Vec<String> = store
            .unfinished_jobs()
            .expect("the queue reads")
            .into_iter()
            .map(|record| record.id.to_string())
            .collect();
        assert_eq!(
            queue,
            vec![queued.to_string()],
            "the queue was not recovered from the directories after {wreck:?}"
        );
        assert_eq!(
            store
                .job(&finished)
                .expect("readable")
                .expect("still there")
                .state,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 }
            },
            "a finished job must survive {wreck:?} too"
        );
        assert_eq!(
            store.job_log(&queued).expect("readable").as_deref(),
            Some(&b"a queued job's log"[..])
        );
    }
}

/// The states a derived index can be found in, none of which may cost a job.
#[derive(Debug)]
enum Wreck {
    Deleted,
    Corrupt,
    /// Parses, but describes a store that no longer exists.
    Stale,
    FromTheFuture,
    /// Well formed, right about how many jobs there are, and short of a job
    /// that is not over. This is the one no count catches: believed, it would
    /// make the unfinished job vanish — out of the queue, never dispatched,
    /// never asked about again — and the hub would report "0 job(s) not over"
    /// with a straight face. Only a hand-edit reaches it today, which is
    /// exactly what a rule of "checked, never believed" is for.
    Short,
}

impl Wreck {
    fn apply(&self, state_dir: &std::path::Path) {
        let index = state_dir.join("index").join("state.json");
        match self {
            Wreck::Deleted => std::fs::remove_file(&index).expect("the index is gone"),
            Wreck::Corrupt => {
                std::fs::write(&index, b"{\"index_version\":1,\"pending\":[").expect("truncated")
            }
            Wreck::Stale => std::fs::write(
                &index,
                br#"{"index_version":1,"jobs_accounted":1,"pending":["j0000009999"]}"#,
            )
            .expect("stale"),
            Wreck::FromTheFuture => std::fs::write(
                &index,
                br#"{"index_version":99,"jobs_accounted":2,"pending":[]}"#,
            )
            .expect("from the future"),
            Wreck::Short => std::fs::write(
                &index,
                br#"{"index_version":1,"jobs_accounted":2,"pending":[]}"#,
            )
            .expect("short of a job"),
        }
    }
}

/// A result for a job of a state root that is gone is never offered to the
/// hub that replaced it — and it does not wedge the runner either.
///
/// The hub's state root was replaced under this runner, so the verdict it is
/// holding is about a job id that now means another job: the new hub mints
/// `j0000000001` again. Offering it would have the exit code of one job land
/// on another job's record, which is why the state root's identity travels in
/// the handshake. The runner is told by name, deals with what it holds — the
/// bytes kept, never destroyed (D19) — and the connection after that is an
/// ordinary one.
///
/// Before the identity existed, this test asserted the opposite: the new hub
/// *acknowledged* a verdict for a job it had never heard of. That was only
/// safe because the test minted three jobs first so the ids could not
/// collide. With a collision — a recreated volume, which is the real case —
/// the same path wrote a fabricated verdict onto a live job.
#[tokio::test]
async fn a_result_for_a_job_of_a_state_root_that_is_gone_is_never_offered() {
    let first_root = tempfile::tempdir().expect("a temporary state directory");
    let second_root = tempfile::tempdir().expect("another one");
    // Approved on both roots, because this test is about a verdict crossing
    // from one to the other and not about who is allowed on either (E-48).
    let token = support::approve_on_disk(first_root.path(), "durable-runner");
    let on_the_second_root = support::approve_on_disk(second_root.path(), "durable-runner");

    let first_hub = HubProcess::start(first_root.path());
    let session = Session::new();
    let (data_root, confined) = support::prepared_runner("durable-runner");
    let first_link = tokio::spawn({
        let (config, session) = (
            runner_config(&first_hub.hub_url, &token, &data_root),
            session.clone(),
        );
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, session, confined).await }
    });
    first_hub.wait_for_runner().await;

    let lost = support::submit_job(&first_hub.requester(), shell("sleep 0.3; exit 3"))
        .await
        .expect("the job is accepted");
    let first_dir = first_root.path().to_path_buf();
    wait_until("the job is running", || {
        file_state_of(&first_dir, &lost) == "running"
    })
    .await;

    first_hub.kill();
    let _ = first_link.await.expect("the link to the dead hub ends");
    wait_until("the runner holds the result", || !session.holds_nothing()).await;

    // A hub on a state root that knows nothing of that job at all — and whose
    // own ids start again at `j0000000001`, which is the id the runner is
    // holding a verdict for.
    assert_eq!(
        lost.as_str(),
        "j0000000001",
        "the collision this test is about needs the first id of each root"
    );
    let second_hub = HubProcess::start(second_root.path());
    let turned_away = herd_runner::serve_once(
        runner_config(&second_hub.hub_url, &on_the_second_root, &data_root),
        session.clone(),
        Arc::clone(&confined),
    )
    .await
    .expect_err("a runner holding another state root's verdict is told so by name");
    let said = format!("{turned_away:#}");
    assert!(
        said.contains("state root") && said.contains("Deal with it and dial again"),
        "the runner has to be told what is wrong, and that it may come back: {said}"
    );

    // What it was holding is settled on that same breath: never offered, and
    // its bytes kept under the data root rather than destroyed.
    assert!(
        session.holds_nothing(),
        "a verdict for a hub that is gone is dealt with, not held for ever"
    );
    let kept: Vec<String> = std::fs::read_dir(data_root.path().join("held-results"))
        .expect("the data root has a held-results directory by now")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        kept.iter()
            .any(|name| name.ends_with(".foreign-state-root")),
        "the bytes of a verdict nobody can record are kept, not dropped: {kept:?}"
    );
    assert!(
        !job_dir(second_root.path(), &lost).exists(),
        "a job the new hub never minted must not appear on its state root"
    );

    // And the next connection is an ordinary one: the runner agrees about the
    // state root now, and runs jobs for it.
    let second_link = tokio::spawn({
        let (config, session) = (
            runner_config(&second_hub.hub_url, &on_the_second_root, &data_root),
            session.clone(),
        );
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, session, confined).await }
    });
    second_hub.wait_for_runner().await;
    let finished = support::run_job(
        &second_hub.requester(),
        shell("printf 'still working\\n'"),
        |_, _| {},
    )
    .await
    .expect("the runner still runs jobs");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
    // The sharp end of it: the new root's own `j0000000001` carries the
    // verdict of the job this hub really ran, and not the exit 3 of a job it
    // never heard of.
    assert_eq!(
        file_state_of(second_root.path(), &lost),
        "finished",
        "the new root's own first job is over"
    );
    assert_eq!(
        record_of(second_root.path(), &lost)["state"]["outcome"]["code"],
        serde_json::json!(0),
        "another state root's verdict landed on this job: {}",
        record_of(second_root.path(), &lost)
    );
    second_link.abort();
}

/// D52: a job on the disk whose labels today's rules reject is quarantined by
/// name, and the hub starts.
///
/// A label set validates when it is deserialised, which is exactly right at a
/// submission — a requester is listening and can be told — and exactly wrong
/// on the durable path, where nobody is. Read strictly, one record carrying a
/// label the rules no longer accept made `JobDirStore::open` fail, which makes
/// the hub refuse to start at all: a service down because of a single job it
/// minted itself. It would also mean `check_label` could never be tightened
/// again after anything was deployed, since every job already on the disk
/// would have to pass the new rules.
///
/// So the labels are read permissively and the job is quarantined: named,
/// visible, skipped, its record untouched. Everything else on that state root
/// goes on working, which is the half this test spends most of its words on.
#[tokio::test]
async fn a_record_whose_labels_the_rules_reject_is_quarantined_not_fatal() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let (queued, rejected) = {
        let store = JobDirStore::open(state.path()).expect("the state root opens");
        let queued = store
            .create_job(herd_hub::NewJob::new(shell("printf 'from the queue\\n'")))
            .expect("minted")
            .id;
        let rejected = store
            .create_job(herd_hub::NewJob::new(shell("printf 'never dispatched\\n'")))
            .expect("minted")
            .id;
        (queued, rejected)
    };

    // The record a tightening of the rules would leave behind, planted by
    // hand because that is the only way to make one today: `BUILD` is not a
    // label, and `LabelSet` would refuse to build it.
    let path = job_dir(state.path(), &rejected).join("job.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the record reads"))
            .expect("the record is json");
    record["labels"] = serde_json::json!(["BUILD"]);
    std::fs::write(
        &path,
        serde_json::to_string(&record).expect("the record serializes"),
    )
    .expect("the record is planted");

    // ---- The store opens. This is the whole point: one bad record is not a
    // dead hub.
    let store = JobDirStore::open(state.path())
        .expect("a record whose labels the rules reject must not stop the hub");
    let quarantined = store.quarantined().expect("the quarantine list reads");
    assert_eq!(
        quarantined.len(),
        1,
        "exactly the one job is quarantined: {quarantined:?}"
    );
    assert_eq!(quarantined[0].0, rejected, "and it is named");
    assert!(
        quarantined[0].1.contains("BUILD"),
        "the reason must name what was rejected: {}",
        quarantined[0].1
    );

    // Skipped: out of the queue, so nothing will ever dispatch it, while the
    // job beside it is still there to be run.
    let queue: Vec<String> = store
        .unfinished_jobs()
        .expect("the queue reads")
        .into_iter()
        .map(|record| record.id.to_string())
        .collect();
    assert_eq!(
        queue,
        vec![queued.to_string()],
        "a quarantined job is not in the queue, and nothing else is missing \
         from it"
    );
    // And asking for it answers with the quarantine rather than pretending it
    // is not there.
    let refused = store
        .job(&rejected)
        .expect_err("the hub will not act on a quarantined job");
    let said = refused.to_string();
    assert!(said.contains(rejected.as_str()), "{said}");
    assert!(said.contains("quarantined"), "{said}");
    assert!(said.contains("BUILD"), "{said}");
    // Its record is left exactly as it was found: a job nobody can act on is
    // not a job Herd may rewrite.
    assert_eq!(
        record_of(state.path(), &rejected)["labels"],
        serde_json::json!(["BUILD"])
    );
    drop(store);

    // ---- And the hub itself serves, on that same state root, with the
    // quarantined job sitting in it.
    let token = support::approve_on_disk(state.path(), "durable-runner");
    let hub = HubProcess::start(state.path());
    let (data_root, confined) = support::prepared_runner("quarantine-runner");
    let link = tokio::spawn({
        let config = runner_config(&hub.hub_url, &token, &data_root);
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, Session::new(), confined).await }
    });
    hub.wait_for_runner().await;
    let state_dir = state.path().to_path_buf();
    wait_until("the inherited queue runs and ends", || {
        file_state_of(&state_dir, &queued) == "finished"
    })
    .await;
    assert_eq!(log_of(state.path(), &queued), "from the queue\n");
    // A fresh submission is accepted and runs too.
    let finished = support::run_job(&hub.requester(), shell("exit 5"), |_, _| {})
        .await
        .expect("the hub serves");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 5 });
    // The quarantined job never ran and never changed.
    assert_eq!(file_state_of(&state_dir, &rejected), "queued");
    assert_eq!(log_of(state.path(), &rejected), "");
    link.abort();
}

/// V10: one `job.json` a power cut tore in half must not stop the hub from
/// **binding its port**.
///
/// This was live on the first consumer's machine and nobody there could
/// repair it. An unreadable record was an `Err` out of `read_stored`, and
/// that `Err` travelled up through `recover`, `open_with` and `main` — which
/// reaches `Hub::bind_with` seventy-six lines later. So the hub exited before
/// the port ever opened, the container restarted straight back into the same
/// wall, and every healthy job on that root was held hostage by one damaged
/// one. Herd is shipped, not operated (invariant 9, D161): from outside the
/// container there is nothing an operator can do about it.
///
/// So the damaged record is set aside exactly as a record the label rules
/// reject already was, and this test asserts the three things that follow:
/// the hub binds and serves, the healthy jobs on either side of the damaged
/// one both run, and a requester asking about the damaged one is told what
/// happened to it.
#[tokio::test]
async fn a_torn_record_is_set_aside_and_the_hub_still_binds_its_port() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let (first, torn, last) = {
        let store = JobDirStore::open(state.path()).expect("the state root opens");
        let mut minted = Vec::new();
        for what in ["the first", "the torn one", "the last"] {
            minted.push(
                store
                    .create_job(herd_hub::NewJob::new(shell(&format!("printf '{what}\\n'"))))
                    .expect("minted")
                    .id,
            );
        }
        (minted[0].clone(), minted[1].clone(), minted[2].clone())
    };

    // The middle record, cut where a power cut would have left it. Only the
    // middle one, so a loss on either side of it shows.
    let path = job_dir(state.path(), &torn).join("job.json");
    let whole = std::fs::read(&path).expect("the record reads");
    let cut = whole.len() * 2 / 3;
    std::fs::write(&path, &whole[..cut]).expect("the record is torn");

    let token = support::approve_on_disk(state.path(), "durable-runner");
    // **The assertion the defect was about.** `HubProcess::start` opens the
    // state root and then binds a port, in that order, as `main()` does.
    // Before V10 it never reached the bind.
    let hub = HubProcess::start(state.path());
    assert_ne!(
        hub.addr.port(),
        0,
        "the hub bound a port it can be reached on"
    );

    let (data_root, confined) = support::prepared_runner("torn-record-runner");
    let link = tokio::spawn({
        let config = runner_config(&hub.hub_url, &token, &data_root);
        let confined = Arc::clone(&confined);
        async move { herd_runner::serve_once(config, Session::new(), confined).await }
    });
    hub.wait_for_runner().await;

    // Both healthy jobs run: the damaged one cost neither of them.
    let state_dir = state.path().to_path_buf();
    let (one, other) = (first.clone(), last.clone());
    wait_until("the two healthy jobs run and end", || {
        file_state_of(&state_dir, &one) == "finished"
            && file_state_of(&state_dir, &other) == "finished"
    })
    .await;
    assert_eq!(log_of(state.path(), &first), "the first\n");
    assert_eq!(log_of(state.path(), &last), "the last\n");

    // A requester asking about the damaged one gets a sentence saying what
    // happened to it, which is the one thing "no such job" could never say:
    // the job is on the disk, and the hub is refusing to act on it.
    let said = herdctl::cancel_job(&hub.requester(), torn.clone())
        .await
        .expect_err("the hub will not cancel a job it will not act on")
        .to_string();
    assert!(said.contains(torn.as_str()), "{said}");
    assert!(said.contains("quarantined"), "{said}");
    assert!(
        !said.contains("no such job"),
        "a job whose record is on the disk is never 'no such job': {said}"
    );

    // And the damaged bytes are exactly as they were found: a record nobody
    // can read is not Herd's to guess at or to tidy away.
    assert_eq!(
        std::fs::read(&path).expect("the record is still there"),
        whole[..cut].to_vec(),
        "the torn record was rewritten"
    );
    link.abort();
}

// ---------------------------------------------------------------------------
// The quarantine, one shape of damage at a time (V10, D218, ORCH-183)
// ---------------------------------------------------------------------------
//
// The test above proves the sentence an operator cares about — the hub binds
// its port and the healthy jobs run — on one shape of damage, a record cut
// mid-write. These five hold the rest of the shapes, and they are here rather
// than in `crash_points.rs` for one reason: every case in that file is
// `#[ignore]`d, so the 308-state census that asserts `0 refuse the whole
// state root` never runs under `./scripts/test.sh`. The strongest evidence
// for the fix was outside the gate. This is it inside the gate.
//
// **Every fixture is a record the store's own writer wrote, then damaged.**
// Never a hand-written string that looks torn: a string the test invented
// proves the reader agrees with the test, and nothing about what `create_job`
// puts on the disk.
//
// **And every one is armed first.** Each test opens the *intact* root and
// asserts `quarantined()` is empty before it damages anything. A detector
// that cannot be shown to be silent on a sound root cannot be trusted when it
// is silent on a damaged one — three detectors in this project reported a
// false zero in one day for exactly that reason.

/// Enough of a `job.json` to read its version: the same shape `store.rs`
/// probes with, restated here so a test can prove *which* of the store's
/// three parses rejected a fixture.
#[derive(serde::Deserialize)]
struct VersionProbe {
    #[serde(default)]
    record_version: u32,
}

/// Mint `whats.len()` real jobs on a fresh root and give back their ids.
///
/// The store is dropped before this returns, so the `flock(2)` on `hub.lock`
/// is free and the caller can reopen the root.
fn mint_on(root: &std::path::Path, whats: &[&str]) -> Vec<JobId> {
    let store = JobDirStore::open(root).expect("the state root opens");
    whats
        .iter()
        .map(|what| {
            store
                .create_job(herd_hub::NewJob::new(shell(&format!("printf '{what}\\n'"))))
                .expect("minted")
                .id
        })
        .collect()
}

/// The arming check, as its own named step: an intact root quarantines
/// nothing.
///
/// Called before each fixture is damaged. If this ever passed trivially — a
/// `quarantined()` that returned an empty list whatever was on the disk — the
/// five assertions below would be worthless, and they would still be green.
fn armed(root: &std::path::Path) {
    let store = JobDirStore::open(root).expect("the intact state root opens");
    let quarantined = store.quarantined().expect("the quarantine lists");
    assert!(
        quarantined.is_empty(),
        "the arming check: an intact root this store wrote itself must quarantine \
         nothing, or the assertions below prove nothing. Got {quarantined:?}"
    );
}

/// Open the root and give back its quarantine list, oldest first.
fn quarantine_on(root: &std::path::Path) -> Vec<(JobId, String)> {
    let store = JobDirStore::open(root).expect("the damaged state root still opens");
    store.quarantined().expect("the quarantine lists")
}

/// Bytes a record never holds: a `job.json` overwritten mid-record with
/// something that is not UTF-8 at all.
///
/// This is the shape a partially-rewritten block leaves, and it fails
/// *before* any parse: `fs::read_to_string` will not hand back the bytes. The
/// hub treats that like every other unreadable record — one job's problem, not
/// the root's.
#[test]
fn a_record_overwritten_with_non_utf8_bytes_quarantines_only_itself() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let ids = mint_on(state.path(), &["the healthy one", "the damaged one"]);
    armed(state.path());

    let path = job_dir(state.path(), &ids[1]).join("job.json");
    let mut bytes = std::fs::read(&path).expect("the record reads");
    // Mid-record, not at the front: the opening brace and the version field
    // survive, which is what a torn block really looks like.
    let at = bytes.len() / 2;
    bytes[at..at + 4].copy_from_slice(&[0xf0, 0x9f, 0xff, 0xfe]);
    std::fs::write(&path, &bytes).expect("the record is overwritten");

    let quarantined = quarantine_on(state.path());
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(quarantined[0].0, ids[1]);
    assert!(
        quarantined[0].1.contains(&path.display().to_string()),
        "the quarantine names the file an operator has to look at: {}",
        quarantined[0].1
    );
    assert_eq!(
        std::fs::read(&path).expect("the record is still there"),
        bytes,
        "a record nobody can read is not Herd's to rewrite"
    );
}

/// Valid JSON, a version this build reads, and a field the record requires
/// **gone** — and the one place `report` and the hub disagree.
///
/// The hub parses the whole `RecordFile`, so a missing `state` is a record it
/// cannot read and it quarantines. `report` parses `RecordOutputs`, which
/// needs `record_version` and `outputs` and lets serde ignore everything
/// else, so the **same bytes** are counted in `census.read` as a record it
/// read. `report` is the *laxer* reader of the two and says nothing about it:
/// a clean `report` is evidence about damaged bytes, not a promise that every
/// record the hub opens is one it can use.
///
/// Pinned here because it is the kind of divergence that is invisible until
/// somebody acts on the wrong one of the two numbers.
#[test]
fn a_record_missing_a_required_field_is_quarantined_and_report_counts_it_read() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let ids = mint_on(state.path(), &["the only one"]);
    armed(state.path());

    let path = job_dir(state.path(), &ids[0]).join("job.json");
    let text = std::fs::read_to_string(&path).expect("the record reads");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("the record is json");
    assert!(
        value
            .as_object_mut()
            .expect("a record is an object")
            .remove("state")
            .is_some(),
        "the fixture must really remove a field the record had"
    );
    let damaged = serde_json::to_string(&value).expect("the fixture serialises");
    std::fs::write(&path, &damaged).expect("the record is rewritten without `state`");

    // **The second arming check.** The fixture must still get *past* the
    // version probe, or this test would be proving the version arm fires and
    // calling it the third parse. The probe is the store's first of three
    // reads of the same text.
    let probe: VersionProbe = serde_json::from_str(&damaged).expect(
        "the fixture still parses as the version probe: if it did not, the quarantine \
         below would be the version arm and not the record parse",
    );
    assert_eq!(
        probe.record_version, RECORD_VERSION,
        "the fixture carries this build's own record version, so the version arm \
         cannot be what sets it aside"
    );

    let quarantined = quarantine_on(state.path());
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(quarantined[0].0, ids[0]);
    let why = &quarantined[0].1;
    assert!(
        why.contains("not a record this hub can read"),
        "the third parse is what rejected it: {why}"
    );
    assert!(
        !why.contains("record version"),
        "the version arm must not be what fired: {why}"
    );
    assert!(
        why.contains("state"),
        "the sentence names the field that is missing: {why}"
    );

    // And the divergence itself, on the very same bytes.
    let report = StateRootReport::read(state.path()).expect("the report reads the root");
    assert_eq!(
        report.census.read, 1,
        "`report` counts this record as read while the hub quarantines it: \
         `RecordOutputs` needs only `record_version` and `outputs`, both \
         `#[serde(default)]`, so `report` is the laxer reader of the two. {:?}",
        report.census
    );
    assert!(
        report.census.unread.is_empty(),
        "`report` says nothing at all about it: {:?}",
        report.census.unread
    );
}

/// `job.json` is not a file any more: a directory stands where the record
/// was.
///
/// Not a crash shape — it is what a restore, a mount or a hand-repair can
/// leave — and the hub must treat it as one job's problem like any other.
/// `read_to_string` on a directory is `EISDIR`, which is the "could not get
/// the bytes at all" arm rather than a parse failure.
#[test]
fn a_record_replaced_by_a_directory_quarantines_only_that_job() {
    let state = tempfile::tempdir().expect("a temporary state directory");
    let ids = mint_on(state.path(), &["the healthy one", "the replaced one"]);
    armed(state.path());

    let path = job_dir(state.path(), &ids[1]).join("job.json");
    std::fs::remove_file(&path).expect("the record is removed");
    std::fs::create_dir(&path).expect("a directory stands in its place");

    let quarantined = quarantine_on(state.path());
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(quarantined[0].0, ids[1]);
    assert!(
        quarantined[0].1.contains("could not be read"),
        "the bytes were never got, which is its own arm: {}",
        quarantined[0].1
    );
    assert!(
        path.is_dir(),
        "the hub repaired nothing: what it found is what is still there"
    );
}

/// A record the filesystem will not hand back: mode `0o000`.
///
/// The shape the first consumer is most likely to meet, because it needs no
/// crash at all — a restore under the wrong owner, or an `fchmod` from
/// somebody's repair script. It is `EACCES` at the read, so the hub sets the
/// job aside carrying the errno and serves the rest of the root.
///
/// **Skipped loudly as root** (D27). Root reads a file moded `0o000`, so on a
/// root-run suite the record would parse, nothing would be quarantined, and
/// the assertion would be a false zero dressed as a pass.
#[test]
#[cfg(unix)]
fn a_record_the_filesystem_will_not_hand_back_quarantines_only_that_job() {
    use std::os::unix::fs::PermissionsExt;

    let state = tempfile::tempdir().expect("a temporary state directory");
    let ids = mint_on(state.path(), &["the healthy one", "the shut one"]);
    armed(state.path());

    let path = job_dir(state.path(), &ids[1]).join("job.json");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .expect("the fixture shuts the record");
    if rustix::process::geteuid().is_root() {
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        herd_runner::testing::skipped(
            "a_record_the_filesystem_will_not_hand_back_quarantines_only_that_job",
            "this test runs as root, and root reads a file moded 0o000: the record \
             would parse, nothing would be quarantined, and a green assertion here \
             would be a false zero. An unprivileged host would have proved that a \
             record whose bytes the filesystem refuses is set aside with its errno \
             while the rest of the root is served. The same sentence is proved \
             without a mode by \
             a_record_replaced_by_a_directory_quarantines_only_that_job",
        );
        return;
    }

    let quarantined = quarantine_on(state.path());
    // Put the mode back before any assertion can panic out of the test, so
    // the temporary directory is still removable.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("the fixture opens it again");

    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(quarantined[0].0, ids[1]);
    assert!(
        quarantined[0].1.contains("could not be read"),
        "the quarantine carries what the kernel said: {}",
        quarantined[0].1
    );
    assert!(
        quarantined[0].1.contains(&path.display().to_string()),
        "and the path it said it about: {}",
        quarantined[0].1
    );
}

/// Tear the **last** record on a root, leaving the damage on the highest id,
/// and give it back: mint three, tear `j0000000003`, reopen, `create_job`.
fn torn_highest_of_three(root: &std::path::Path) -> (Vec<JobId>, Vec<u8>) {
    let ids = mint_on(root, &["the first", "the second", "the last"]);
    assert_eq!(ids[2].as_str(), "j0000000003", "the fixture's own premise");
    armed(root);
    let path = job_dir(root, &ids[2]).join("job.json");
    let whole = std::fs::read(&path).expect("the record reads");
    let cut = whole[..whole.len() * 2 / 3].to_vec();
    std::fs::write(&path, &cut).expect("the record is torn");
    (ids, cut)
}

/// **The one that matters for later.** A torn record holding the highest id
/// does not cost that id to the next job — on a root with a high-water mark
/// *and* on a root without one.
///
/// `recover` takes the next id from the higher of two things: `store.json`'s
/// `ids_minted` mark, and the highest **directory name** in `jobs/`.
/// `sequence_of` reads the name and never the record, so a quarantined
/// record's id stays spent only while its directory keeps its name.
///
/// **Two arms, because only one of them is load-bearing, and it is the one a
/// modern root hides.** On a root this build wrote, the mark alone would hold
/// the line even if the directory vanished. On a root written *before* the
/// mark existed — `store.rs`'s own words for that set are "every root in
/// existence", the first consumer's included — the directories are the whole
/// truth, and there the directory name is the only thing standing between the
/// next job and an id another job already has.
///
/// So this is the test that goes red the day somebody decides a set-aside
/// record should be moved or renamed — `j0000000003.bad`, or into a
/// `quarantine/` folder. The second arm is the one that catches it.
#[test]
fn a_torn_record_holding_the_highest_id_does_not_cost_that_id() {
    // ---- Arm one: a root this build wrote, mark and directories agreeing.
    let state = tempfile::tempdir().expect("a temporary state directory");
    let (ids, cut) = torn_highest_of_three(state.path());
    let path = job_dir(state.path(), &ids[2]).join("job.json");

    let store = JobDirStore::open(state.path()).expect("the state root opens");
    let quarantined = store.quarantined().expect("the quarantine lists");
    assert_eq!(
        quarantined.len(),
        1,
        "the torn record is on the quarantine list under its own id. An empty list \
         here means recovery never saw the directory at all — which is what a \
         RENAMED quarantine directory looks like, because `sequence_of` reads the \
         name and `j0000000003.bad` is not a name it knows. Read arm two's message \
         for what that costs. Got {quarantined:?}"
    );
    assert_eq!(quarantined[0].0, ids[2]);

    let next = store
        .create_job(herd_hub::NewJob::new(shell("printf 'after the damage\\n'")))
        .expect("the hub still mints on a root holding a quarantined record")
        .id;
    assert_eq!(
        next.as_str(),
        "j0000000004",
        "a quarantined record must not give its id back"
    );
    assert!(
        job_dir(state.path(), &ids[2]).is_dir(),
        "the quarantined record's directory is still named {}",
        ids[2]
    );
    assert_eq!(
        std::fs::read(&path).expect("the torn record is still there"),
        cut,
        "the torn record was rewritten"
    );
    drop(store);

    // ---- Arm two, and this is the assertion with something behind it: a
    // root with no high-water mark, where `jobs/` is the only record of what
    // has been handed out.
    let legacy = tempfile::tempdir().expect("a second temporary state directory");
    let (ids, _) = torn_highest_of_three(legacy.path());
    let store_file = legacy.path().join("store.json");
    let text = std::fs::read_to_string(&store_file).expect("store.json reads");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("store.json is json");
    assert!(
        value
            .as_object_mut()
            .expect("store.json is an object")
            .remove("ids_minted")
            .is_some(),
        "the fixture must really remove the mark, or this arm is arm one again"
    );
    std::fs::write(
        &store_file,
        serde_json::to_string(&value).expect("the fixture serialises"),
    )
    .expect("store.json is rewritten without its mark");

    let store = JobDirStore::open(legacy.path()).expect("a root with no mark opens");
    let quarantined = store.quarantined().expect("the quarantine lists");
    assert_eq!(
        quarantined.len(),
        1,
        "the torn record is still the one set aside. An empty list is a renamed \
         quarantine directory: see the id assertion below for the price. Got \
         {quarantined:?}"
    );
    let next = store
        .create_job(herd_hub::NewJob::new(shell("printf 'after the damage\\n'")))
        .expect("the hub still mints")
        .id;
    assert_eq!(
        next.as_str(),
        "j0000000004",
        "On a root with no high-water mark the DIRECTORY NAME is the only thing \
         that says j0000000003 was ever handed out: `sequence_of` reads the name, \
         never the record. If this line is red because the new id is j0000000003, \
         something now renames or moves a quarantined record's directory. That \
         drops the directory out of the id sequence, and on this root — every \
         root written before `ids_minted`, the first consumer's included — one id \
         is then handed to two jobs, one of them a job whose record is still \
         sitting in jobs/j0000000003."
    );
    assert!(
        job_dir(legacy.path(), &ids[2]).is_dir(),
        "and the directory that carries that id is still named {}",
        ids[2]
    );
}
