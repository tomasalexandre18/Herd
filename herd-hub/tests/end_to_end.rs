//! The test that defines M1, now running against the store M2 put on disk:
//! a hub, a runner and a requester in one process, a real command, real
//! output, the real outcome (invariant 5). Everything M1 asserted must keep
//! being true of a hub whose state is files.
//!
//! The scaffolding — a hub on a free port, a state root that is removed
//! afterwards — is in `support`.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_hub::{JobState, JobStore};
use herd_proto::{
    from_frame, to_frame, FromHub, JobMetrics, JobOutcome, LogStream, Role, ToHub, PROTOCOL_VERSION,
};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{confinement, shell, Harness};

/// What a requester saw, in arrival order.
#[derive(Default)]
struct Seen {
    chunks: Vec<(LogStream, Vec<u8>)>,
    first_chunk_at: Option<Instant>,
}

impl Seen {
    fn text(&self, stream: LogStream) -> String {
        let bytes: Vec<u8> = self
            .chunks
            .iter()
            .filter(|(seen, _)| *seen == stream)
            .flat_map(|(_, data)| data.iter().copied())
            .collect();
        String::from_utf8(bytes).expect("this test's jobs write text")
    }
}

#[tokio::test]
async fn a_job_streams_its_output_and_succeeds() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let finished = support::run_job(
        &harness.requester(),
        shell("printf 'out of the runner\\n'; printf 'and its stderr\\n' >&2"),
        |stream, data| seen.chunks.push((stream, data.to_vec())),
    )
    .await
    .expect("the job runs to its end");

    assert_eq!(
        finished.outcome,
        JobOutcome::Exited { code: 0 },
        "a successful command exits 0"
    );
    assert_eq!(seen.text(LogStream::Stdout), "out of the runner\n");
    assert_eq!(seen.text(LogStream::Stderr), "and its stderr\n");

    // The hub kept the same bytes it relayed, and knows how the job ended.
    let log = String::from_utf8(
        harness
            .store
            .job_log(&finished.job_id)
            .expect("the store reads")
            .expect("the hub knows this job"),
    )
    .expect("this job wrote text");
    assert!(log.contains("out of the runner\n"), "hub log: {log:?}");
    assert!(log.contains("and its stderr\n"), "hub log: {log:?}");
    let record = harness
        .store
        .job(&finished.job_id)
        .expect("the store reads")
        .expect("the hub knows this job");
    assert_eq!(
        record.state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 0 }
        }
    );
}

/// E-15's resource fields, filled by M4: the runner measures, the hub keeps
/// the figures on the record, and the requester gets the same ones. A job
/// that wrote a known number of bytes is the only way to assert the disk
/// figure is a measurement and not a constant.
#[tokio::test]
async fn a_job_s_record_carries_what_the_runner_measured() {
    let harness = Harness::with_hub_and_runner().await;
    let written = 256 * 1024;
    let finished = support::run_job(
        &harness.requester(),
        shell(&format!(
            "dd if=/dev/zero of=./payload bs=1024 count={} 2>/dev/null; printf 'wrote it\\n'; \
             sleep 0.3",
            written / 1024
        )),
        |_, _| {},
    )
    .await
    .expect("the job runs to its end");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });

    let record = harness
        .store
        .job(&finished.job_id)
        .expect("the store reads")
        .expect("the hub knows this job");
    let measured = record.metrics.clone();
    assert_eq!(
        measured, finished.metrics,
        "the requester and the record must hold the same figures"
    );
    assert_ne!(
        measured,
        JobMetrics::default(),
        "nothing was measured at all: {measured:?}"
    );
    assert_eq!(
        measured.log_bytes, 9,
        "the log cap counts the bytes the job wrote: {measured:?}"
    );
    assert!(
        measured.wall_time_ms >= 300,
        "the job slept 300 ms, so its wall time cannot be {} ms",
        measured.wall_time_ms
    );
    let bytes_written = measured
        .bytes_written
        .expect("the workspace is always measurable");
    assert!(
        bytes_written >= written,
        "the job wrote {written} bytes and the record says {bytes_written}"
    );
    assert!(
        measured.host_load_milli.is_some(),
        "a Linux host always has a load average: {measured:?}"
    );
    let peak = measured
        .peak_memory_bytes
        .expect("a peak is readable from the cgroup or from /proc");
    assert!(peak > 0, "a process that ran held some memory");
}

#[tokio::test]
async fn a_failing_job_reports_its_own_exit_code() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let finished = support::run_job(
        &harness.requester(),
        shell("printf 'it went wrong\\n' >&2; exit 42"),
        |stream, data| seen.chunks.push((stream, data.to_vec())),
    )
    .await
    .expect("a failing job is still a job that ran");

    assert_eq!(
        finished.outcome,
        JobOutcome::Exited { code: 42 },
        "the job's own code reaches the requester"
    );
    assert_eq!(seen.text(LogStream::Stderr), "it went wrong\n");
    assert_eq!(
        harness.state_of(&finished.job_id),
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 42 }
        }
    );
}

/// A command a shell could not execute. 127 is what a shell would exit with
/// here, and that is exactly what must not be reported: 127 belongs to the
/// job, this failure belongs to Herd.
const NO_SUCH_COMMAND: &str = "herd-no-such-executable-in-m1";

#[tokio::test]
async fn a_job_killed_by_a_signal_is_reported_as_signalled() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    // The shell kills itself, so the runner's own child dies by the signal
    // rather than reporting a shell's 128 + 15. SIGTERM is 15 everywhere.
    let finished = support::run_job(
        &harness.requester(),
        shell("printf 'about to die\\n'; kill -TERM $$"),
        |stream, data| seen.chunks.push((stream, data.to_vec())),
    )
    .await
    .expect("a signalled job is still a job that ran");

    assert_eq!(
        finished.outcome,
        JobOutcome::Signalled { signal: 15 },
        "a killed process has no exit code to report"
    );
    assert_eq!(
        seen.text(LogStream::Stdout),
        "about to die\n",
        "what the job printed before dying still reaches the requester"
    );
    assert_eq!(
        harness.state_of(&finished.job_id),
        JobState::Finished {
            outcome: JobOutcome::Signalled { signal: 15 }
        }
    );
}

#[tokio::test]
async fn a_command_that_does_not_exist_is_reported_as_could_not_start() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let finished = support::run_job(
        &harness.requester(),
        vec![NO_SUCH_COMMAND.to_owned(), "--whatever".to_owned()],
        |stream, data| seen.chunks.push((stream, data.to_vec())),
    )
    .await
    .expect("a job that cannot start is still an answer, not an error");

    let JobOutcome::CouldNotStart { reason } = &finished.outcome else {
        panic!(
            "a missing executable must not be an exit code: {:?}",
            finished.outcome
        );
    };
    assert!(
        reason.contains(NO_SUCH_COMMAND),
        "the reason must name what could not be started: {reason}"
    );
    // The job wrote nothing, so neither did Herd on the job's behalf: the
    // reason travels in the outcome, not in the job's log.
    assert!(
        seen.chunks.is_empty(),
        "a job that never started has no output: {:?}",
        seen.chunks
    );
    assert_eq!(
        harness
            .store
            .job_log(&finished.job_id)
            .expect("the store reads")
            .as_deref(),
        Some(&b""[..])
    );
    assert_eq!(
        harness.state_of(&finished.job_id),
        JobState::Finished {
            outcome: finished.outcome.clone()
        },
        "the hub records the reason as it was reported"
    );
}

#[tokio::test]
async fn output_arrives_while_the_job_is_still_running() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let submitted_at = Instant::now();
    let finished = support::run_job(
        &harness.requester(),
        // Writes, waits, writes: if the log were delivered at exit, the first
        // chunk and the outcome would arrive together.
        shell("printf 'first\\n'; sleep 0.5; printf 'second\\n'"),
        |stream, data| {
            seen.first_chunk_at.get_or_insert_with(Instant::now);
            seen.chunks.push((stream, data.to_vec()));
        },
    )
    .await
    .expect("the job runs to its end");
    let finished_at = Instant::now();

    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
    assert_eq!(seen.text(LogStream::Stdout), "first\nsecond\n");
    let first_chunk_at = seen.first_chunk_at.expect("some output arrived");
    assert!(
        first_chunk_at.duration_since(submitted_at) < Duration::from_secs(2),
        "the first chunk took {:?} to arrive",
        first_chunk_at.duration_since(submitted_at)
    );
    assert!(
        seen.chunks[0].1.starts_with(b"first"),
        "the first chunk should be the first line, not the whole log: {:?}",
        seen.chunks[0].1
    );
    // The process slept 500 ms between its two lines, so a first chunk that
    // arrived before the process exited must predate the outcome by most of
    // that. Anything less means the output was buffered to the end.
    let lead = finished_at.duration_since(first_chunk_at);
    assert!(
        lead > Duration::from_millis(200),
        "the first chunk arrived only {lead:?} before the outcome, so it waited for the process"
    );
}

/// A peer on the wrong protocol version is refused by name, whichever side of
/// this hub's number it is on.
///
/// **Both directions, and the stale one is the one that happens.** A peer
/// announcing a *newer* version is a machine somebody upgraded early; a peer
/// announcing an *older* one is every runner and every `herdctl` in the fleet
/// on the morning after a bump, which is exactly the cost D148's move to
/// version 2 is paid in. The refusal has to name both figures so one line
/// says which side to move — otherwise a fleet goes quiet machine by machine
/// with nothing saying why (D73, E-51).
#[tokio::test]
async fn a_version_mismatch_is_refused_naming_both_versions() {
    for announced in [PROTOCOL_VERSION + 1, PROTOCOL_VERSION - 1] {
        a_peer_announcing_is_refused(announced).await;
    }
}

async fn a_peer_announcing_is_refused(announced: u32) {
    let harness = Harness::with_hub().await;

    for role in [Role::Runner, Role::Requester] {
        let (mut socket, _response) = tokio_tungstenite::connect_async(&harness.hub_url)
            .await
            .expect("the hub accepts the connection");
        let hello = ToHub::Hello {
            protocol_version: announced,
            peer_version: herd_runner::runner_version().to_owned(),
            role,
            token: None,
            name: Some("from-the-future".to_owned()),
            instance: Some("run-from-the-future".to_owned()),
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
            // This stand-in holds no job and says so by not saying (D-NEXT).
            jobs_at_once: None,
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("hello encodes")))
            .await
            .expect("hello is sent");

        let frame = socket
            .next()
            .await
            .expect("the hub answers")
            .expect("the frame is readable");
        let text = frame.to_text().expect("the answer is a text frame");
        let answer: FromHub = from_frame(text).expect("the answer is a protocol message");
        let FromHub::Refused { reason } = answer else {
            panic!("a mismatched version must be refused, got {answer:?}");
        };
        assert!(
            reason.contains(&format!("version {PROTOCOL_VERSION}")),
            "the refusal must name the hub's version: {reason}"
        );
        assert!(
            reason.contains(&format!("version {announced}")),
            "the refusal must name the peer's version: {reason}"
        );
    }
}

/// The one fleet state a submission is refused in: a state root on which no
/// runner is approved (D140).
///
/// Not "no runner is connected" any more. A fleet that is merely away queues
/// the job with the sentence the queue already had for it, because D88 says a
/// fleet is not a closed world; an **empty roster** is a configuration
/// mistake, and a job queued against it would wait for nobody coming. Which
/// of the two a hub is in is a fact on its disk, not a guess about the moment.
/// `away_fleet.rs` drives the other half against the real binaries.
#[tokio::test]
async fn a_submission_with_an_empty_roster_is_refused_by_name() {
    let harness = Harness::with_hub().await;
    let error = support::run_job(&harness.requester(), shell("true"), |_, _| {})
        .await
        .expect_err("nothing submitted to a hub with an empty roster could ever start");
    let said = format!("{error:#}");
    assert!(
        said.contains("0 runner(s) are approved on this state root"),
        "the refusal must name the state the hub is in: {said}"
    );
    assert!(
        said.contains("herdctl runner approve"),
        "and what to do about it: {said}"
    );
}

/// And with the roster populated and nothing connected, the same submission
/// is **queued** — the state that used to decide a job's fate by whether it
/// arrived a moment before or after the last link dropped (D88, D140).
#[tokio::test]
async fn a_submission_with_the_roster_populated_and_nobody_connected_is_queued() {
    let harness = Harness::with_hub().await;
    let _token = harness.approve("the-machine-that-is-away");
    let job = herdctl::submit_job(
        &harness.requester(),
        shell("true"),
        herd_proto::LabelSet::default(),
        herd_proto::Priority::Step,
        herdctl::Declared::default(),
    )
    .await
    .expect("a fleet that is away queues a job, it does not refuse one");
    let record = harness
        .store
        .job(&job)
        .expect("the record reads")
        .expect("the submission minted a record");
    assert_eq!(record.state, JobState::Queued);
    let why = record.waiting_for.unwrap_or_default();
    assert!(
        why.contains("no runner is connected to this hub"),
        "and it gets the one sentence the queue already had, not a second one: {why}"
    );
}

/// D13, and the defect it warned about: a runner the hub has no room for is
/// told to **stand by**, not refused.
///
/// M2 answered a second runner with `Refused`, which is terminal, and a
/// runner that is refused exits. The commonest way to hit that was not a
/// second machine at all — it was one runner redialling faster than the hub
/// noticed its previous connection had gone, so the runner died of its own
/// reconnection. A peer that waits is not a peer that was refused.
///
/// Since M7 the hub holds a **fleet**, so "no room" is no longer about a
/// second machine: a second name joins beside the first. What is still
/// refused room is a **twin** — a second process wearing a name another live
/// process is answering to — because an attempt belongs to a name and two
/// live twins would take turns kicking each other off it. That is the case
/// this test drives, and the answer it must get is still a stand-by.
#[tokio::test]
async fn a_twin_of_a_live_runner_is_told_to_stand_by_rather_than_refused() {
    let harness = Harness::with_hub_and_runner().await;
    let (data_root, confined) = support::prepared_runner("second-runner");
    let error = herd_runner::serve_once(
        herd_runner::Config {
            hub_url: harness.hub_url.clone(),
            // The name the harness's own runner is answering to, from a
            // different process: a twin.
            name: Some("test-runner".to_owned()),
            token: Some(harness.approve("test-runner")),
            confinement: confinement(&data_root),
            ..herd_runner::Config::default()
        },
        herd_runner::Session::new(),
        confined,
    )
    .await
    .expect_err("a live runner's name is not free");
    let stand_by = error
        .downcast_ref::<herd_runner::StandBy>()
        .expect("a runner with no room is asked to wait, never refused (D13)");
    assert!(
        stand_by.reason.contains("test-runner"),
        "the answer must name what the hub is waiting for: {stand_by}"
    );
    assert!(
        error.downcast_ref::<herd_runner::Refusal>().is_none(),
        "a stand-by must not be a refusal: a refused runner exits"
    );
    assert!(
        stand_by.retry_in > std::time::Duration::ZERO,
        "and it must say when to come back: {stand_by}"
    );
}

/// The same hub, the same runner name, a new connection: the hub drops the
/// connection it was holding and takes the new one.
///
/// This is the case that killed runners in M2. A runner whose link dropped
/// without the hub noticing — a kill, a NAT timeout — redials within
/// milliseconds, and the hub still has the old connection registered. Told
/// "a runner is already connected" with a terminal frame, the runner exited
/// and its host stopped being part of the herd until someone noticed.
#[tokio::test]
async fn the_same_runner_dialling_again_replaces_its_own_dead_connection() {
    // **One `Session`, dialling twice.** The redial the hub must take is the
    // *same process* coming back, which it tells apart from a twin by the
    // instance id — and that id belongs to the `Session`, minted once and
    // shared by every connection that session makes.
    //
    // This test used to start from [`Harness::with_hub_and_runner`] and dial
    // a fresh `Session::new()` beside it, which mints a *different* instance:
    // a twin, which the test just above proves is told to stand by. The
    // assertion that followed — that a job ran — was then satisfied by the
    // harness's own runner, which had never been replaced by anything. It
    // passed without the case under test ever happening (D101).
    let harness = Harness::with_hub().await;
    let (data_root, confined) = support::prepared_runner("same-runner-again");
    let session = herd_runner::Session::new();
    let config = herd_runner::Config {
        hub_url: harness.hub_url.clone(),
        name: Some("test-runner".to_owned()),
        token: Some(harness.approve("test-runner")),
        confinement: confinement(&data_root),
        ..herd_runner::Config::default()
    };
    let first = tokio::spawn({
        let (config, session, confined) = (
            config.clone(),
            session.clone(),
            std::sync::Arc::clone(&confined),
        );
        async move { herd_runner::serve_once(config, session, confined).await }
    });
    support::wait_until("the first connection has joined", || {
        harness.runner_joined()
    })
    .await;

    // The link goes from under it, and the same process dials again at once
    // — the same name and, because the `Session` is the same, the same
    // instance. That is the redial; a fresh `Session` here would mint a new
    // instance and be the *twin* of the test above, which the hub rightly
    // tells to stand by.
    first.abort();
    let again = tokio::spawn({
        let (config, session, confined) = (config, session, confined);
        async move { herd_runner::serve_once(config, session, confined).await }
    });

    // There is no other runner left to answer for it: this hub's own runner
    // is the connection that just dialled, and a job runs on it or on
    // nothing. That is what the old shape of this test could not say — it
    // dialled beside a harness runner that stayed connected throughout, so
    // the job it submitted was answered by the machine that had never been
    // replaced (D101).
    //
    // The hub superseding a connection it is *still holding* is pinned
    // deterministically in `link.rs`, with a hand-written peer that does not
    // close: here the close is real, so whether the hub has already noticed
    // it by the time the redial lands is a race either way, and both of its
    // outcomes are this assertion.
    let finished = support::run_job(&harness.requester(), shell("printf 'back\\n'"), |_, _| {})
        .await
        .expect("the runner that dialled again is the hub's runner now");
    assert_eq!(finished.outcome, JobOutcome::Exited { code: 0 });
    assert!(
        !again.is_finished(),
        "the redial was turned away rather than taken"
    );
    again.abort();
}
