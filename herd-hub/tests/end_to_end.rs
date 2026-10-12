//! The test that defines M1: a hub, a runner and a requester in one process,
//! a real command, real output, the real outcome (invariant 5).
//!
//! Everything binds port 0 and discovers the port it got, so the suite never
//! collides with a port in use. Every spawned task is aborted when its
//! harness drops, and jobs are spawned with `kill_on_drop`, so no test leaves
//! a process or a bound port behind.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_hub::{Hub, JobState, JobStore, MemoryJobStore};
use herd_proto::{
    from_frame, to_frame, FromHub, JobOutcome, LogStream, Role, ToHub, PROTOCOL_VERSION,
};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

/// A hub, optionally a runner, and the means to reach them.
struct Harness {
    hub_url: String,
    store: Arc<MemoryJobStore>,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    /// A hub on a free loopback port, with nothing connected to it yet.
    async fn with_hub() -> Self {
        let store = Arc::new(MemoryJobStore::new());
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        let harness = Self {
            hub_url,
            store,
            tasks: vec![serving],
        };
        // Nothing else can be asked of the hub until it is actually serving,
        // but a handle proves the test holds the same state the server does.
        assert!(!handle.runner_connected());
        harness
    }

    /// A hub with one runner joined and ready to be dispatched to.
    async fn with_hub_and_runner() -> Self {
        let store = Arc::new(MemoryJobStore::new());
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        let running = tokio::spawn({
            let config = herd_runner::Config {
                hub_url: hub_url.clone(),
                name: Some("test-runner".to_owned()),
                // Carried, not checked: M15 gives tokens meaning.
                token: Some("not-validated-in-m1".to_owned()),
            };
            async move {
                let _ = herd_runner::run(config).await;
            }
        });
        // Wait on the hub's own state, not on a sleep: the submission below is
        // only meaningful once the runner is registered.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Self {
            hub_url,
            store,
            tasks: vec![serving, running],
        }
    }

    /// What the hub itself believes became of a job.
    fn state_of(&self, job_id: &herd_proto::JobId) -> JobState {
        self.store
            .job(job_id)
            .expect("the store reads")
            .expect("the hub knows this job")
            .state
    }

    fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: Some("not-validated-in-m1".to_owned()),
            name: Some("test-requester".to_owned()),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// What a requester saw, in arrival order.
#[derive(Default)]
struct Seen {
    chunks: Vec<(LogStream, String)>,
    first_chunk_at: Option<Instant>,
}

impl Seen {
    fn text(&self, stream: LogStream) -> String {
        self.chunks
            .iter()
            .filter(|(seen, _)| *seen == stream)
            .map(|(_, data)| data.as_str())
            .collect()
    }
}

fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

#[tokio::test]
async fn a_job_streams_its_output_and_succeeds() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let finished = herdctl::run_job(
        &harness.requester(),
        shell("printf 'out of the runner\\n'; printf 'and its stderr\\n' >&2"),
        |stream, data| seen.chunks.push((stream, data.to_owned())),
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
    let log = harness
        .store
        .job_log(&finished.job_id)
        .expect("the store reads")
        .expect("the hub knows this job");
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

#[tokio::test]
async fn a_failing_job_reports_its_own_exit_code() {
    let harness = Harness::with_hub_and_runner().await;
    let mut seen = Seen::default();
    let finished = herdctl::run_job(
        &harness.requester(),
        shell("printf 'it went wrong\\n' >&2; exit 42"),
        |stream, data| seen.chunks.push((stream, data.to_owned())),
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
    let finished = herdctl::run_job(
        &harness.requester(),
        shell("printf 'about to die\\n'; kill -TERM $$"),
        |stream, data| seen.chunks.push((stream, data.to_owned())),
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
    let finished = herdctl::run_job(
        &harness.requester(),
        vec![NO_SUCH_COMMAND.to_owned(), "--whatever".to_owned()],
        |stream, data| seen.chunks.push((stream, data.to_owned())),
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
        Some("")
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
    let finished = herdctl::run_job(
        &harness.requester(),
        // Writes, waits, writes: if the log were delivered at exit, the first
        // chunk and the outcome would arrive together.
        shell("printf 'first\\n'; sleep 0.5; printf 'second\\n'"),
        |stream, data| {
            seen.first_chunk_at.get_or_insert_with(Instant::now);
            seen.chunks.push((stream, data.to_owned()));
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
        seen.chunks[0].1.starts_with("first"),
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

#[tokio::test]
async fn a_version_mismatch_is_refused_naming_both_versions() {
    let harness = Harness::with_hub().await;
    let announced = PROTOCOL_VERSION + 1;

    for role in [Role::Runner, Role::Requester] {
        let (mut socket, _response) = tokio_tungstenite::connect_async(&harness.hub_url)
            .await
            .expect("the hub accepts the connection");
        let hello = ToHub::Hello {
            protocol_version: announced,
            role,
            token: None,
            name: Some("from-the-future".to_owned()),
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

#[tokio::test]
async fn a_submission_with_no_runner_is_refused_by_name() {
    let harness = Harness::with_hub().await;
    let error = herdctl::run_job(&harness.requester(), shell("true"), |_, _| {})
        .await
        .expect_err("there is nothing to run the job on");
    assert!(
        error.to_string().contains("no runner is connected"),
        "a wait with no runner must be named, not silent: {error:#}"
    );
}

#[tokio::test]
async fn a_second_runner_is_refused_in_m1() {
    let harness = Harness::with_hub_and_runner().await;
    let error = herd_runner::run(herd_runner::Config {
        hub_url: harness.hub_url.clone(),
        name: Some("the-second-runner".to_owned()),
        token: None,
    })
    .await
    .expect_err("M1 accepts one runner");
    assert!(
        error.to_string().contains("already connected"),
        "the refusal must say why: {error:#}"
    );
}
