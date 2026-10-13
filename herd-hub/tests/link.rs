//! M3: what the hub does when a runner is not there, or is not finished being
//! there.
//!
//! Every threshold here is given to the hub rather than waited out. A runner
//! is lost after 60 s in the deployment, and a suite that really waited that
//! out would take minutes per case and be the first thing anyone deleted; so
//! the hub takes its timing as policy and these tests hand it a second. What
//! is *not* simulated is the behaviour: real sockets, the real store on a real
//! disk, the real watchdog, and — where the point is a process dying — the
//! real binaries, in `killed_hub.rs`.
//!
//! The runner in most of these is a hand-written one, because the thing being
//! tested is a runner behaving badly: going silent while holding a job, coming
//! back with the frames of an attempt that is over. A real runner cannot be
//! asked to do that, and a test that cannot make a peer misbehave cannot prove
//! the hub survives it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_hub::{Hub, HubHandle, JobDirStore, JobRecord, JobState, JobStore, Policy};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, LogStream, Role, RunningJob,
    ToHub, PROTOCOL_VERSION,
};
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// How long any wait here may take before it is a failure. Generous because
/// CI is slow, and never a substitute for waiting on a real condition.
const PATIENCE: Duration = Duration::from_secs(20);

/// A hub told to lose a runner in a second instead of a minute.
struct Quick {
    hub_url: String,
    store: Arc<JobDirStore>,
    #[allow(dead_code)]
    state: TempDir,
    handle: HubHandle,
    serving: tokio::task::JoinHandle<()>,
}

impl Quick {
    async fn start(lost_after: Duration) -> Self {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
        let hub = Hub::bind_with_policy(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
            Policy {
                lost_after,
                ..Policy::default()
            },
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        Self {
            hub_url,
            store,
            state,
            handle,
            serving,
        }
    }

    fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: None,
            name: Some("test-requester".to_owned()),
        }
    }

    fn record(&self, job_id: &JobId) -> JobRecord {
        self.store
            .job(job_id)
            .expect("the store reads")
            .expect("the hub knows this job")
    }

    fn log(&self, job_id: &JobId) -> String {
        String::from_utf8_lossy(
            &self
                .store
                .job_log(job_id)
                .expect("the store reads")
                .unwrap_or_default(),
        )
        .into_owned()
    }

    fn events(&self, job_id: &JobId) -> Vec<herd_hub::JobEvent> {
        self.store
            .job_events(job_id)
            .expect("the store reads")
            .unwrap_or_default()
    }

    /// Submit without following, as a hook would: the job runs whether anyone
    /// is listening or not.
    async fn submit(&self, command: &str) -> JobId {
        herdctl::submit_job(
            &self.requester(),
            vec!["/bin/sh".to_owned(), "-c".to_owned(), command.to_owned()],
        )
        .await
        .expect("the submission is accepted")
    }

    async fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for Quick {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A runner that does exactly what a test tells it to, and nothing else.
struct Puppet {
    socket: Socket,
}

impl Puppet {
    async fn join(hub_url: &str, name: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a connection");
        let hello = ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Runner,
            token: None,
            name: Some(name.to_owned()),
        };
        socket
            .send(Message::text(to_frame(&hello).expect("encodes")))
            .await
            .expect("hello goes out");
        let mut puppet = Self { socket };
        match puppet.next().await {
            Some(FromHub::Welcome { .. }) => puppet,
            other => panic!("the hub answered hello with {other:?}"),
        }
    }

    /// The next frame, or `None` if nothing comes for a while — which is how
    /// a test asserts that the hub said nothing.
    async fn next(&mut self) -> Option<FromHub> {
        loop {
            let frame = tokio::time::timeout(PATIENCE, self.socket.next()).await;
            match frame {
                Ok(Some(Ok(Message::Text(text)))) => {
                    return Some(from_frame(text.as_str()).expect("the hub speaks the protocol"))
                }
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
                _ => return None,
            }
        }
    }

    /// The next frame if one comes within `patience`, and `None` otherwise.
    async fn next_within(&mut self, patience: Duration) -> Option<FromHub> {
        match tokio::time::timeout(patience, self.socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                Some(from_frame(text.as_str()).expect("the hub speaks the protocol"))
            }
            _ => None,
        }
    }

    async fn say(&mut self, message: ToHub) {
        self.socket
            .send(Message::text(to_frame(&message).expect("encodes")))
            .await
            .expect("the frame goes out");
    }

    /// Wait for the hub to hand this runner a job, ignoring anything else it
    /// says on the way.
    async fn take_a_job(&mut self) -> (JobId, u32, Vec<String>) {
        loop {
            match self.next().await {
                Some(FromHub::AssignJob {
                    job_id,
                    attempt,
                    command,
                }) => return (job_id, attempt, command),
                Some(_) => continue,
                None => panic!("the hub never handed this runner a job"),
            }
        }
    }

    async fn report_health(&mut self, jobs: Vec<RunningJob>, draining: bool) {
        self.say(ToHub::Health { jobs, draining }).await;
    }
}

/// E-40: the hub keeps the age of each runner's last health report, which is
/// the only way to tell a runner with nothing to do from one that has stopped
/// answering. M13 puts this on the status document; M3 puts it in the state.
#[tokio::test]
async fn a_runner_s_health_report_and_its_age_are_in_the_hub_s_state() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut runner = Puppet::join(&hub.hub_url, "wsl-pc").await;

    // Before any report: the runner is here, and the hub says it has heard
    // nothing from it yet — which is not the same as a runner gone quiet.
    let joined = hub.handle.runners();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].name, "wsl-pc");
    assert_eq!(joined[0].reports_seen, 0);
    assert!(!joined[0].draining);

    runner.report_health(Vec::new(), false).await;
    assert!(
        matches!(runner.next().await, Some(FromHub::HealthRecorded {})),
        "the hub answers a health report, so the runner knows the link is alive"
    );
    hub.wait_until("the hub has recorded a health report", || {
        hub.handle
            .runners()
            .first()
            .is_some_and(|r| r.reports_seen == 1)
    })
    .await;
    let seen = hub.handle.runners();
    assert!(
        seen[0].last_health_age < Duration::from_secs(5),
        "the age of the last report must be fresh: {:?}",
        seen[0].last_health_age
    );
    assert!(seen[0].joined_for < Duration::from_secs(60));

    // A job this runner is executing is in the report too, with how much
    // output it has produced: that number is what lets the hub tell a quiet
    // job from one whose output is being lost.
    let job_id = hub.submit("true").await;
    let (assigned, attempt, _) = runner.take_a_job().await;
    assert_eq!(assigned, job_id);
    runner
        .report_health(
            vec![RunningJob {
                job_id: job_id.clone(),
                attempt,
                chunks_sent: 0,
            }],
            false,
        )
        .await;
    hub.wait_until("the report names the job", || {
        hub.handle
            .runners()
            .first()
            .is_some_and(|r| r.jobs == vec![job_id.clone()])
    })
    .await;

    // And a runner that says it is draining is one the hub stops giving work
    // to, at once rather than at the next report (invariant 2).
    runner.report_health(Vec::new(), true).await;
    hub.wait_until("the hub knows the runner is draining", || {
        hub.handle.runners().first().is_some_and(|r| r.draining)
    })
    .await;
    let refused = herdctl::submit_job(&hub.requester(), vec!["true".to_owned()])
        .await
        .expect_err("a draining runner takes no new job");
    assert!(
        refused.to_string().contains("draining"),
        "the refusal must say why: {refused:#}"
    );
}

/// E-21, the whole of it: a runner that stops answering has its job attempted
/// once more, and the record names both attempts — which runner each was on,
/// and why the first one ended.
#[tokio::test]
async fn a_silent_runner_s_job_is_attempted_again_and_the_record_names_both() {
    let hub = Quick::start(Duration::from_secs(1)).await;
    let mut first = Puppet::join(&hub.hub_url, "runner-a").await;
    let job_id = hub.submit("true").await;
    let (assigned, attempt, command) = first.take_a_job().await;
    assert_eq!(assigned, job_id);
    assert_eq!(attempt, 1);
    assert_eq!(command, ["/bin/sh", "-c", "true"]);

    first
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    first
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"the first attempt\n".to_vec(),
        })
        .await;
    hub.wait_until(
        "the first attempt is running and its output is recorded",
        || {
            hub.record(&job_id).state == JobState::Running
                && hub.log(&job_id).contains("the first attempt")
        },
    )
    .await;

    // And now it says nothing at all. Not a disconnection — the socket is
    // still open, which is the hard case: only the absence of health reports
    // says this runner is gone.
    hub.wait_until("the job is queued again", || {
        hub.record(&job_id).state == JobState::Queued
    })
    .await;

    let requeued = hub.record(&job_id);
    assert_eq!(requeued.attempts.len(), 1, "{requeued:?}");
    assert_eq!(requeued.attempts[0].runner, "runner-a");
    let why = requeued.attempts[0]
        .ended
        .clone()
        .expect("the attempt that was lost says why it ended");
    assert!(
        why.contains("stopped answering"),
        "the reason must name the silence: {why}"
    );
    assert!(requeued.attempts[0].ended_at.is_some());

    // A second runner picks it up as attempt 2 and finishes it.
    let mut second = Puppet::join(&hub.hub_url, "runner-b").await;
    let (again, second_attempt, _) = second.take_a_job().await;
    assert_eq!(again, job_id);
    assert_eq!(
        second_attempt, 2,
        "a requeued job is attempt 2, not 1 again"
    );
    second
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt: second_attempt,
        })
        .await;
    second
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt: second_attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"the second attempt\n".to_vec(),
        })
        .await;
    second
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt: second_attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 1,
            metrics: JobMetrics::default(),
        })
        .await;
    hub.wait_until("the second attempt is recorded", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;

    // The record a reader gets: two goes, named, in order, with the first
    // one's ending explained and the second one's verdict kept.
    let done = hub.record(&job_id);
    assert_eq!(
        done.state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 0 }
        }
    );
    assert_eq!(done.attempts.len(), 2, "{done:?}");
    assert_eq!(done.attempts[0].number, 1);
    assert_eq!(done.attempts[1].number, 2);
    assert_eq!(done.attempts[1].runner, "runner-b");
    assert!(
        done.attempts[1].ended.is_none(),
        "the attempt that produced the verdict needs no excuse: {done:?}"
    );

    // Both attempts' output is in the one log, with a line saying where the
    // second begins: a reader must not take the first attempt's half-finished
    // output for the second's.
    let log = hub.log(&job_id);
    assert!(log.contains("the first attempt"), "{log}");
    assert!(log.contains("the second attempt"), "{log}");
    assert!(
        log.contains("attempt 2 of this job starts here"),
        "the log must say where the second attempt begins: {log}"
    );
    assert!(
        log.find("the first attempt") < log.find("attempt 2 of this job starts here"),
        "and say it in the right place: {log}"
    );

    // And the events carry the same story for a reader of the record alone.
    let events = hub.events(&job_id);
    let states: Vec<&str> = events
        .iter()
        .filter_map(|event| event.state().map(|state| state.name()))
        .collect();
    assert_eq!(
        states,
        ["queued", "assigned", "running", "queued", "assigned", "running", "finished"],
        "the job went round once, and every step is on the record: {events:?}"
    );
}

/// E-21's other half, and the one that costs a record if it is wrong: a runner
/// that comes back after being written off finishes **nothing** of the attempt
/// it lost.
///
/// The frames it is still holding — output, a result — are about an attempt the
/// hub has ended. They are refused by name, and they must not land in the
/// record of the attempt that replaced it: a job that is running now must not
/// be reported finished by a runner that was told hours ago to forget it.
#[tokio::test]
async fn a_returning_runner_finishes_nothing_of_the_attempt_it_lost() {
    let hub = Quick::start(Duration::from_secs(1)).await;
    let mut lost = Puppet::join(&hub.hub_url, "runner-a").await;
    let job_id = hub.submit("true").await;
    let (_, first_attempt, _) = lost.take_a_job().await;
    lost.say(ToHub::JobStarted {
        job_id: job_id.clone(),
        attempt: first_attempt,
    })
    .await;
    hub.wait_until("the attempt is running", || {
        hub.record(&job_id).state == JobState::Running
    })
    .await;
    hub.wait_until("the silent runner's job is queued again", || {
        hub.record(&job_id).state == JobState::Queued
    })
    .await;

    // The runner comes back — the same machine, a new connection — and is
    // handed the job again, as attempt 2. It is still holding everything it
    // never managed to deliver about attempt 1.
    let mut back = Puppet::join(&hub.hub_url, "runner-a").await;
    let (_, second_attempt, _) = back.take_a_job().await;
    assert_eq!(second_attempt, 2);
    let before = hub.record(&job_id);

    back.say(ToHub::LogChunk {
        job_id: job_id.clone(),
        attempt: first_attempt,
        stream: LogStream::Stdout,
        seq: 0,
        data: b"OUTPUT OF THE ATTEMPT THAT WAS LOST\n".to_vec(),
    })
    .await;
    match back.next().await {
        Some(FromHub::StaleAttempt {
            job_id: named,
            attempt,
            reason,
        }) => {
            assert_eq!(named, job_id);
            assert_eq!(attempt, first_attempt);
            assert!(
                reason.contains("not the attempt this hub is waiting for"),
                "the refusal must say what it is: {reason}"
            );
        }
        other => panic!("output of an attempt that is over must be refused by name: {other:?}"),
    }

    back.say(ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt: first_attempt,
        outcome: JobOutcome::Exited { code: 99 },
        chunks_sent: 1,
        metrics: JobMetrics::default(),
    })
    .await;
    assert!(
        matches!(back.next().await, Some(FromHub::StaleAttempt { .. })),
        "a result from an attempt that is over must be refused too"
    );

    // Nothing moved. Not the state, not the attempts, not the log.
    let after = hub.record(&job_id);
    assert_eq!(
        after.state, before.state,
        "a stale frame changed the record's state"
    );
    assert_eq!(after.attempts, before.attempts);
    assert!(
        !hub.log(&job_id)
            .contains("OUTPUT OF THE ATTEMPT THAT WAS LOST"),
        "the lost attempt's output must not be written into the new attempt's \
         log: {}",
        hub.log(&job_id)
    );

    // And the attempt that is current still works: the stale frames did not
    // poison the connection either.
    back.say(ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt: second_attempt,
        outcome: JobOutcome::Exited { code: 0 },
        chunks_sent: 0,
        metrics: JobMetrics::default(),
    })
    .await;
    hub.wait_until("the attempt that is current ends normally", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;
    assert_eq!(
        hub.record(&job_id).state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 0 }
        },
        "the verdict is the current attempt's, never the lost one's"
    );
}

/// Once more means once (E-21). A job whose second runner is also lost ends —
/// `lost`, by name, with the reason — rather than going round the herd for
/// ever.
#[tokio::test]
async fn a_second_loss_ends_the_job_rather_than_starting_a_third_attempt() {
    let hub = Quick::start(Duration::from_secs(1)).await;
    let job_id = {
        let mut first = Puppet::join(&hub.hub_url, "runner-a").await;
        let job_id = hub.submit("true").await;
        let (_, attempt, _) = first.take_a_job().await;
        first
            .say(ToHub::JobStarted {
                job_id: job_id.clone(),
                attempt,
            })
            .await;
        hub.wait_until("the first attempt is queued again", || {
            hub.record(&job_id).state == JobState::Queued
        })
        .await;
        job_id
    };

    let mut second = Puppet::join(&hub.hub_url, "runner-b").await;
    let (_, attempt, _) = second.take_a_job().await;
    assert_eq!(attempt, 2);
    second
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    // And this one goes silent too.
    hub.wait_until(
        "the job ends rather than being attempted a third time",
        || matches!(hub.record(&job_id).state, JobState::Finished { .. }),
    )
    .await;

    let done = hub.record(&job_id);
    let JobState::Finished {
        outcome: JobOutcome::Lost { reason },
    } = &done.state
    else {
        panic!("a job out of attempts ends `lost`, not as an exit code: {done:?}");
    };
    assert!(
        reason.contains("attempt 2 of 2"),
        "the reason must say the job is not attempted again: {reason}"
    );
    assert!(reason.contains("stopped answering"), "and why: {reason}");
    assert_eq!(done.attempts.len(), 2);
    assert!(
        done.attempts.iter().all(|attempt| attempt.ended.is_some()),
        "both attempts ended without a verdict, and both say so: {done:?}"
    );
    // A requester gets no verdict out of this, and 125 rather than a failure
    // it would act on (D20).
    assert_eq!(
        herdctl::exit_code_for(&JobOutcome::Lost {
            reason: reason.clone()
        }),
        125
    );
}

/// E-06: a cancel reaches the runner and ends the job within ten seconds,
/// while a sibling job on the same runner runs to its own end untouched.
///
/// The real runner, not a puppet: the point is that the job's process really
/// stops. The kill itself is hardened into a process-tree kill in M4; what is
/// asserted here is that the cancel arrives, that it is scoped to one job, and
/// that the end is recorded as a cancellation rather than as a signal nobody
/// asked about.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_one_job_within_ten_seconds_and_a_sibling_runs_on() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let runner = tokio::spawn(herd_runner::run(herd_runner::Config {
        hub_url: hub.hub_url.clone(),
        name: Some("test-runner".to_owned()),
        token: None,
        ..herd_runner::Config::default()
    }));
    hub.wait_until("the runner joins", || hub.handle.runner_connected())
        .await;

    // One job that would run for a long time, and one that ends by itself.
    let doomed = hub.submit("printf 'working\\n'; sleep 300").await;
    let sibling = hub
        .submit("printf 'start\\n'; sleep 2; printf 'done\\n'")
        .await;
    hub.wait_until("both jobs are running", || {
        hub.record(&doomed).state == JobState::Running
            && hub.record(&sibling).state == JobState::Running
    })
    .await;

    let asked_at = Instant::now();
    let said = herdctl::cancel_job(&hub.requester(), doomed.clone())
        .await
        .expect("the cancel is accepted");
    assert!(
        said.contains("ended on the runner"),
        "the requester is told what is happening: {said}"
    );
    hub.wait_until("the cancelled job is over", || {
        matches!(hub.record(&doomed).state, JobState::Finished { .. })
    })
    .await;
    let took = asked_at.elapsed();
    assert!(
        took < Duration::from_secs(10),
        "a cancel must end the job within 10 s (E-06); this took {took:?}"
    );

    // Ended as a cancellation, which is not a verdict and not a signal
    // nobody asked for.
    let record = hub.record(&doomed);
    let JobState::Finished {
        outcome: JobOutcome::Cancelled { reason },
    } = &record.state
    else {
        panic!("a cancelled job ends `cancelled`: {record:?}");
    };
    assert!(reason.contains("requester"), "{reason}");
    assert!(
        hub.log(&doomed).contains("working"),
        "what the job printed before it was cancelled is still its log: {}",
        hub.log(&doomed)
    );

    // And the sibling was never touched: same runner, same moment, its own
    // verdict.
    hub.wait_until("the sibling finishes on its own", || {
        matches!(hub.record(&sibling).state, JobState::Finished { .. })
    })
    .await;
    assert_eq!(
        hub.record(&sibling).state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 0 }
        },
        "a cancel is scoped to one job"
    );
    assert!(
        hub.log(&sibling).contains("done"),
        "the sibling ran to its end: {}",
        hub.log(&sibling)
    );
    runner.abort();
}

/// Invariant 2: the owner asks the runner to stop, and gets the machine back
/// inside five seconds even though a job is running — and what became of that
/// job is on the record by name, not silently lost.
#[tokio::test(flavor = "multi_thread")]
async fn a_busy_runner_asked_to_stop_drains_inside_five_seconds() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let (ask, asked) = tokio::sync::oneshot::channel::<String>();
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            hub_url: hub.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            token: None,
            ..herd_runner::Config::default()
        },
        async move {
            asked
                .await
                .unwrap_or_else(|_| "the test went away".to_owned())
        },
    ));
    hub.wait_until("the runner joins", || hub.handle.runner_connected())
        .await;
    let job_id = hub.submit("printf 'working\\n'; sleep 300").await;
    hub.wait_until("the job is running", || {
        hub.record(&job_id).state == JobState::Running
    })
    .await;

    let asked_at = Instant::now();
    ask.send("the runner's owner asked it to stop".to_owned())
        .expect("the runner is listening");
    let stopped = tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .expect("a runner asked to stop must be gone inside five seconds (invariant 2)");
    assert!(stopped.is_ok(), "the runner stopped cleanly: {stopped:?}");
    let took = asked_at.elapsed();
    assert!(
        took <= Duration::from_secs(5),
        "invariant 2 promises five seconds; this took {took:?}"
    );

    // The job it was running is named on the record, with the reason, and
    // queued for another go — the one thing a drain may not do is lose a job
    // in silence.
    hub.wait_until("the drained job's attempt is ended by name", || {
        hub.record(&job_id)
            .attempts
            .first()
            .is_some_and(|attempt| attempt.ended.is_some())
    })
    .await;
    let record = hub.record(&job_id);
    let why = record.attempts[0].ended.clone().expect("a reason");
    assert!(
        why.contains("asked it to stop"),
        "the record must say the owner asked: {why}"
    );
    assert_eq!(
        record.state,
        JobState::Queued,
        "a job whose runner stopped waits for another runner: {record:?}"
    );
}

/// The other half of a drain: a job submitted while the runner is on its way
/// out is refused by name, rather than handed to a runner that is leaving.
#[tokio::test(flavor = "multi_thread")]
async fn a_draining_runner_refuses_a_job_submitted_in_the_meantime() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let (ask, asked) = tokio::sync::oneshot::channel::<String>();
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            hub_url: hub.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            token: None,
            ..herd_runner::Config::default()
        },
        async move {
            asked
                .await
                .unwrap_or_else(|_| "the test went away".to_owned())
        },
    ));
    hub.wait_until("the runner joins", || hub.handle.runner_connected())
        .await;
    // Busy on purpose: a drain with a job running is the case invariant 2 is
    // about, and the case where refusing new work matters most.
    let running = hub.submit("sleep 300").await;
    hub.wait_until("the job is running", || {
        hub.record(&running).state == JobState::Running
    })
    .await;

    ask.send("the runner's owner asked it to stop".to_owned())
        .expect("the runner is listening");
    // The runner says it is draining before it goes, so the hub knows at once.
    hub.wait_until("the hub hears that the runner is draining", || {
        hub.handle
            .runners()
            .first()
            .is_some_and(|runner| runner.draining)
            || !hub.handle.runner_connected()
    })
    .await;
    let refused = herdctl::submit_job(&hub.requester(), vec!["true".to_owned()])
        .await
        .expect_err("a draining runner takes no new job");
    let said = format!("{refused:#}");
    assert!(
        said.contains("draining") || said.contains("no runner is connected"),
        "the refusal must name the reason: {said}"
    );
    let _ = runner.await;
}

/// The lost tail, closed. A job's last output never reaches the hub, and
/// nothing follows it: the hub learns of the shortfall from the end of the
/// job, asks for it back, and gets it (D36).
#[tokio::test]
async fn output_lost_with_nothing_behind_it_is_asked_for_and_recovered() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut runner = Puppet::join(&hub.hub_url, "runner-a").await;
    let job_id = hub.submit("true").await;
    let (_, attempt, _) = runner.take_a_job().await;
    runner
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"the first line\n".to_vec(),
        })
        .await;
    hub.wait_until("the first line is recorded", || {
        hub.log(&job_id).contains("the first line")
    })
    .await;

    // Chunk 1 is never sent — it is the tail a dead link swallowed — and the
    // job ends having produced two. M2 would have recorded the end on a log
    // one line short, with nothing anywhere saying so, because the gap is
    // only visible from the chunk that follows it and there is none.
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 2,
            metrics: JobMetrics::default(),
        })
        .await;
    let asked = loop {
        match runner.next().await {
            Some(FromHub::ResendLog {
                job_id: named,
                from_seq,
                ..
            }) => {
                assert_eq!(named, job_id);
                break from_seq;
            }
            Some(FromHub::LogAck { .. }) | Some(FromHub::HealthRecorded {}) => continue,
            other => panic!(
                "the hub must ask for the output it is missing, not record a short log: {other:?}"
            ),
        }
    };
    assert_eq!(asked, 1, "it asks from the first chunk it does not have");
    assert!(
        !matches!(hub.record(&job_id).state, JobState::Finished { .. }),
        "the end must not be recorded on a log the hub knows to be short"
    );

    // The runner still holds it, as a real one does, and sends it again with
    // the number it first gave it.
    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 1,
            data: b"the last line\n".to_vec(),
        })
        .await;
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 2,
            metrics: JobMetrics::default(),
        })
        .await;
    hub.wait_until("the end is recorded once the log is whole", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;
    let log = hub.log(&job_id);
    assert_eq!(
        log, "the first line\nthe last line\n",
        "the log must hold the tail, in order, with no marker: {log}"
    );
    assert!(
        !log.contains("never reached"),
        "nothing was lost, so nothing is named as lost: {log}"
    );
}

/// And when it cannot be recovered, the log says so rather than being quietly
/// short (D29): the hub asks once, and a runner that no longer holds the
/// output gets its end recorded with the gap named.
#[tokio::test]
async fn output_that_cannot_be_recovered_is_named_in_the_log() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut runner = Puppet::join(&hub.hub_url, "runner-a").await;
    let job_id = hub.submit("true").await;
    let (_, attempt, _) = runner.take_a_job().await;
    runner
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"all that is left\n".to_vec(),
        })
        .await;
    hub.wait_until("the first chunk is recorded", || {
        hub.log(&job_id).contains("all that is left")
    })
    .await;

    // Three chunks were produced and the hub has one. Asked for the rest, this
    // runner has nothing: it dropped them, as a real runner does once it is
    // holding more output than it is allowed to hold.
    for _ in 0..2 {
        runner
            .say(ToHub::JobFinished {
                job_id: job_id.clone(),
                attempt,
                outcome: JobOutcome::Exited { code: 3 },
                chunks_sent: 3,
                metrics: JobMetrics::default(),
            })
            .await;
        // Drain whatever the hub says back, so the second end is a second ask.
        let _ = runner.next_within(Duration::from_secs(2)).await;
    }

    hub.wait_until("the end is recorded even though the log is short", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;
    assert_eq!(
        hub.record(&job_id).state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 3 }
        },
        "a short log must not cost the job its verdict"
    );
    let log = hub.log(&job_id);
    assert!(
        log.contains("never reached this log"),
        "output that is gone must be named where it is missing: {log}"
    );
    assert!(
        log.contains("chunks 1 to 2"),
        "and which chunks they were: {log}"
    );
    let gaps = hub
        .events(&job_id)
        .iter()
        .filter(|event| event.state().is_none())
        .count();
    assert!(
        gaps >= 1,
        "a reader of the record alone must see that the log is short"
    );
}

/// Output lost in the middle of a job, with the link still up, is asked for
/// and recovered — but not on sight.
///
/// A health report always arrives with a chunk or two in flight, so a
/// shortfall at that instant means nothing. What means something is a
/// shortfall that is still there at the next report with nothing written in
/// between: then the output really is missing, and the hub asks for it. Asking
/// on sight would have the hub ask every running job for output it is in the
/// middle of writing, every fifteen seconds, for ever.
#[tokio::test]
async fn output_missing_between_two_health_reports_is_asked_for() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut runner = Puppet::join(&hub.hub_url, "runner-a").await;
    let job_id = hub.submit("true").await;
    let (_, attempt, _) = runner.take_a_job().await;
    runner
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"first\n".to_vec(),
        })
        .await;
    hub.wait_until("the first chunk is recorded", || {
        hub.log(&job_id).contains("first")
    })
    .await;

    // Chunk 1 is never sent: the link swallowed it. The first report that
    // says so is not acted on — a chunk in flight looks exactly like this.
    let reported = vec![RunningJob {
        job_id: job_id.clone(),
        attempt,
        chunks_sent: 2,
    }];
    runner.report_health(reported.clone(), false).await;
    let mut asked = None;
    for _ in 0..2 {
        match runner.next_within(Duration::from_millis(500)).await {
            Some(FromHub::ResendLog { from_seq, .. }) => asked = Some(from_seq),
            Some(_) => continue,
            None => break,
        }
    }
    assert_eq!(
        asked, None,
        "a shortfall seen once is a chunk in flight, not a loss"
    );

    // The second says the same thing, and the hub has written nothing since.
    runner.report_health(reported, false).await;
    let asked = loop {
        match runner.next_within(PATIENCE).await {
            Some(FromHub::ResendLog { from_seq, .. }) => break from_seq,
            Some(_) => continue,
            None => panic!("a shortfall that survives a report must be asked for"),
        }
    };
    assert_eq!(asked, 1);

    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 1,
            data: b"second\n".to_vec(),
        })
        .await;
    hub.wait_until("the missing chunk arrives", || {
        hub.log(&job_id).contains("second")
    })
    .await;
    assert_eq!(
        hub.log(&job_id),
        "first\nsecond\n",
        "recovered in place, with no marker: {}",
        hub.log(&job_id)
    );
}

/// E-06's other case: a job nobody has started is cancelled by the hub
/// itself, because there is no runner to ask.
///
/// Reached here the way it is reached in reality — a job requeued after its
/// runner was lost, waiting for one that can take it — which is also the
/// state a requester is most likely to want out of.
#[tokio::test]
async fn a_job_waiting_for_a_runner_is_cancelled_by_the_hub_itself() {
    let hub = Quick::start(Duration::from_secs(1)).await;
    let job_id = {
        let mut lost = Puppet::join(&hub.hub_url, "runner-a").await;
        let job_id = hub.submit("true").await;
        let (_, attempt, _) = lost.take_a_job().await;
        lost.say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
        // And then silence, until the hub gives up on the attempt and puts
        // the job back in the queue with no runner left to hand it to.
        hub.wait_until("the job is waiting for a runner", || {
            hub.record(&job_id).state == JobState::Queued
        })
        .await;
        job_id
    };
    hub.wait_until("the runner that went quiet is dropped", || {
        !hub.handle.runner_connected()
    })
    .await;

    let said = herdctl::cancel_job(&hub.requester(), job_id.clone())
        .await
        .expect("the cancel is accepted");
    assert!(
        said.contains("was queued and is cancelled"),
        "the requester is told what happened: {said}"
    );
    let record = hub.record(&job_id);
    let JobState::Finished {
        outcome: JobOutcome::Cancelled { reason },
    } = &record.state
    else {
        panic!("a queued job is cancelled by the hub, there and then: {record:?}");
    };
    assert!(
        reason.contains("before it started"),
        "and the record says it never ran: {reason}"
    );
}
