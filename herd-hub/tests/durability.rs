//! The tests that define M2: the hub's state is files, and nothing is lost.
//!
//! What is asserted here is behaviour a requester or the first consumer can
//! see — a job that finishes across a restart, a wait that returns the right
//! code, a record that can be read while the job runs — and the records are
//! read as files, with no tool but a file read, because that is how the
//! consumer reads them (E-15, E-53).

use std::sync::Arc;

use herd_hub::{JobDirStore, JobState, JobStore};
use herd_proto::{JobOutcome, LogStream};
use herd_runner::Session;

mod support;

use support::{
    events_of, file_state_of, job_dir, log_of, record_of, shell, wait_until, Harness, HubProcess,
};

fn runner_config(hub_url: &str, data_root: &support::DataRootDir) -> herd_runner::Config {
    herd_runner::Config {
        hub_url: hub_url.to_owned(),
        name: Some("durable-runner".to_owned()),
        token: None,
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

    // ---- The hub that is about to die.
    let first_hub = HubProcess::start(state.path());
    // The runner holds its results outside any one connection, which is what
    // lets it redeliver after the hub comes back. M3 puts the reconnection
    // loop around this; here the test dials the second connection itself.
    let session = Session::new();
    let (data_root, confined) = support::prepared_runner("durable-runner");
    let first_link = tokio::spawn({
        let (config, session) = (
            runner_config(&first_hub.hub_url, &data_root),
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
            runner_config(&second_hub.hub_url, &data_root),
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

    let first_hub = HubProcess::start(first_root.path());
    let session = Session::new();
    let (data_root, confined) = support::prepared_runner("durable-runner");
    let first_link = tokio::spawn({
        let (config, session) = (
            runner_config(&first_hub.hub_url, &data_root),
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
        runner_config(&second_hub.hub_url, &data_root),
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
            runner_config(&second_hub.hub_url, &data_root),
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
    let hub = HubProcess::start(state.path());
    let (data_root, confined) = support::prepared_runner("quarantine-runner");
    let link = tokio::spawn({
        let config = runner_config(&hub.hub_url, &data_root);
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
