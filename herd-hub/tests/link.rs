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
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, LabelSet, LogStream, Priority,
    Role, RunningJob, ToHub, PROTOCOL_VERSION,
};
use herd_runner::testing::TempDir as DataRootDir;
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// How long any wait here may take before it is a failure. Generous because
/// CI is slow, and never a substitute for waiting on a real condition.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long a dial waits to find out whether it was turned away.
///
/// Short on purpose, and the one place in this file where a duration is
/// waited out rather than waited on: "the hub said nothing else" has no event
/// to wait for. A hub that turns a runner away does it in the same breath as
/// the welcome, so this is milliseconds of real work and the rest is slack
/// for a loaded CI machine.
const TURNED_AWAY_PATIENCE: Duration = Duration::from_millis(500);

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
                // Both figures move together, because the hub refuses a
                // threshold that is not comfortably longer than the cadence
                // it expects — a second of silence means nothing against
                // reports fifteen seconds apart. The deployment's shape is
                // kept: a threshold several reports long.
                health_every: lost_after / 10,
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
            LabelSet::default(),
            Priority::Step,
            herdctl::Declared::default(),
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
#[derive(Debug)]
struct Puppet {
    socket: Socket,
}

impl Puppet {
    /// A runner joining under a name, as one process.
    ///
    /// The instance identity is derived from the name, so joining twice under
    /// one name is one process dialling again — a redial — and a twin is what
    /// [`Puppet::dial`] makes.
    async fn join(hub_url: &str, name: &str) -> Self {
        Self::handshake(hub_url, name, &format!("run-{name}"), None, None).await
    }

    /// Dial saying exactly which process this is and which hub state root it
    /// believes it belongs to, and give back what the hub did with it:
    /// `Ok` when it was admitted, `Err(frame)` when it was told something
    /// instead.
    ///
    /// The welcome always comes first — a hub about to turn a runner away
    /// sends it anyway, because that frame is how the runner learns which
    /// state root it is talking to — so the answer is whatever follows it.
    async fn dial(
        hub_url: &str,
        name: &str,
        instance: &str,
        state_root: Option<&str>,
    ) -> Result<Self, Box<FromHub>> {
        Self::dial_saying(hub_url, name, instance, state_root, None).await
    }

    /// The same, announcing the cadence this runner really reports at — the
    /// figure the hub judges its own silence threshold against, because two
    /// configurations that each pass their own startup check can still be
    /// fatal together.
    async fn dial_saying(
        hub_url: &str,
        name: &str,
        instance: &str,
        state_root: Option<&str>,
        health_every: Option<Duration>,
    ) -> Result<Self, Box<FromHub>> {
        let mut puppet = Self::handshake(hub_url, name, instance, state_root, health_every).await;
        match puppet.next_within(TURNED_AWAY_PATIENCE).await {
            Some(answer) => Err(Box::new(answer)),
            None => Ok(puppet),
        }
    }

    async fn handshake(
        hub_url: &str,
        name: &str,
        instance: &str,
        state_root: Option<&str>,
        health_every: Option<Duration>,
    ) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a connection");
        let hello = ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Runner,
            token: None,
            name: Some(name.to_owned()),
            instance: Some(instance.to_owned()),
            state_root: state_root.map(str::to_owned),
            health_every_ms: health_every.map(|every| every.as_millis() as u64),
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
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
                    ..
                }) => return (job_id, attempt, command),
                Some(_) => continue,
                None => panic!("the hub never handed this runner a job"),
            }
        }
    }

    /// Wait for the hub to acknowledge a result, which is the moment it is on
    /// the hub's disk and the runner may forget it (E-44).
    async fn wait_for_the_result_ack(&mut self, job_id: &JobId) {
        loop {
            match self.next().await {
                Some(FromHub::JobResultAck { job_id: acked }) if &acked == job_id => return,
                Some(_) => continue,
                None => panic!("the hub never acknowledged the result of {job_id}"),
            }
        }
    }

    /// The reason the hub refused the next frame about an attempt, skipping
    /// the acknowledgements of whatever came before it — and never skipping
    /// an acknowledgement of a *result*, because a result the hub says it has
    /// is a result it has written (D19).
    async fn next_refusal(&mut self) -> Option<String> {
        loop {
            match self.next().await {
                Some(FromHub::StaleAttempt { reason, .. }) => return Some(reason),
                Some(FromHub::JobResultAck { job_id }) => {
                    panic!("the hub acknowledged a verdict for {job_id} it must not have taken")
                }
                Some(_) => continue,
                None => return None,
            }
        }
    }

    async fn report_health(&mut self, jobs: Vec<RunningJob>, draining: bool) {
        self.say(ToHub::Health {
            jobs,
            draining,
            load_milli: None,
            switch: herd_proto::Switch::On,
        })
        .await;
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
    let refused = herdctl::submit_job(
        &hub.requester(),
        vec!["true".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared::default(),
    )
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
            steps: Vec::new(),
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
        steps: Vec::new(),
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
        steps: Vec::new(),
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
    let data_root = DataRootDir::new("cancel-sibling");
    let runner = tokio::spawn(herd_runner::run(herd_runner::Config {
        hub_url: hub.hub_url.clone(),
        name: Some("test-runner".to_owned()),
        token: None,
        confinement: herd_runner::Confinement::with_data_root(data_root.path()),
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
    // And the doomed job has actually printed its line, which `Running` does
    // not say: the runner reports a job started when it has *spawned* the
    // shell, and the shell has still to reach its first `printf`. Cancelling
    // on `Running` alone killed it in between on two pinned cores — the log
    // below was empty and the test failed claiming output had been lost,
    // when the job had never produced any (D101).
    hub.wait_until("the doomed job has printed its line", || {
        hub.log(&doomed).contains("working")
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
    let data_root = DataRootDir::new("drain-five-seconds");
    let (ask, asked) = tokio::sync::oneshot::channel::<String>();
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            hub_url: hub.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            token: None,
            // A data root of its own: a runner locks its root for the life of
            // the process (D32), and `Config::default()` points at the real
            // one under $XDG_STATE_HOME — which two tests would fight over
            // and a test has no business touching at all.
            confinement: herd_runner::Confinement::with_data_root(data_root.path()),
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
    let data_root = DataRootDir::new("drain-refuses");
    let (ask, asked) = tokio::sync::oneshot::channel::<String>();
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            hub_url: hub.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            token: None,
            // A data root of its own: a runner locks its root for the life of
            // the process (D32), and `Config::default()` points at the real
            // one under $XDG_STATE_HOME — which two tests would fight over
            // and a test has no business touching at all.
            confinement: herd_runner::Confinement::with_data_root(data_root.path()),
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
    let refused = herdctl::submit_job(
        &hub.requester(),
        vec!["true".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared::default(),
    )
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
            steps: Vec::new(),
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
            steps: Vec::new(),
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
                steps: Vec::new(),
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

/// **Who** a frame came from, which the attempt number alone cannot answer.
///
/// A job is running as attempt 1 on `runner-x`. Its link goes. Another
/// machine joins and says that attempt 1 of that job exited 99, with figures
/// it made up — a frame that is well formed, names a job the hub really has
/// and an attempt the hub really is waiting for, and is a lie only in its
/// author. Before this, the hub believed it: it wrote exit 99 and the
/// invented metrics onto a record that went on naming attempt 1 on
/// `runner-x`, while the real job's output sat in the log underneath.
///
/// An attempt belongs to the runner it was handed to. A log chunk goes
/// through the same gate, because a foreign paragraph in a build log is a
/// quieter version of the same falsehood.
#[tokio::test]
async fn a_verdict_from_a_runner_the_attempt_was_not_given_to_is_refused_by_name() {
    // A minute of threshold: nothing here is the watchdog's doing. The
    // attempt is live and unquestioned throughout.
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut holder = Puppet::join(&hub.hub_url, "runner-x").await;
    let job_id = hub.submit("true").await;
    let (_, attempt, _) = holder.take_a_job().await;
    holder
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    hub.wait_until("the attempt is running", || {
        hub.record(&job_id).state == JobState::Running
    })
    .await;
    let before = hub.record(&job_id);

    // `runner-x` is gone without a word, as a `kill -9` leaves it. What is
    // timed is the attempt, not the connection (D37), so the attempt is
    // untouched — and that is exactly what makes the next frame dangerous.
    drop(holder);
    hub.wait_until("the hub has noticed the link go", || {
        !hub.handle.runner_connected()
    })
    .await;

    let mut other = Puppet::join(&hub.hub_url, "runner-y").await;
    other
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 99 },
            chunks_sent: 1,
            metrics: JobMetrics {
                log_bytes: 4096,
                wall_time_ms: 77,
                ..JobMetrics::default()
            },
            steps: Vec::new(),
        })
        .await;
    match other.next().await {
        Some(FromHub::StaleAttempt { reason, .. }) => {
            assert!(
                reason.contains("runner-x") && reason.contains("runner-y"),
                "the refusal must name whose attempt it is and who spoke: {reason}"
            );
            assert!(
                reason.contains("handed to"),
                "and say why that matters: {reason}"
            );
        }
        other => panic!("a verdict from the wrong runner must be refused by name: {other:?}"),
    }
    other
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 0,
            data: b"OUTPUT OF A JOB THIS RUNNER NEVER RAN\n".to_vec(),
        })
        .await;
    assert!(
        matches!(other.next().await, Some(FromHub::StaleAttempt { .. })),
        "a log chunk from the wrong runner goes through the same gate"
    );

    // Nothing of it landed: not the outcome, not the figures, not the log.
    let after = hub.record(&job_id);
    assert_eq!(
        after.state, before.state,
        "the record took a foreign verdict"
    );
    assert_eq!(after.attempts, before.attempts);
    assert_eq!(
        after.metrics,
        JobMetrics::default(),
        "figures a runner that ran nothing invented must not be in the record"
    );
    assert!(
        !hub.log(&job_id)
            .contains("OUTPUT OF A JOB THIS RUNNER NEVER RAN"),
        "another runner wrote into this attempt's log: {}",
        hub.log(&job_id)
    );

    // And the honest path is untouched: `runner-x` itself, dialling again
    // after the gap, is believed — which is the whole of E-44's redelivery.
    drop(other);
    hub.wait_until("the hub has noticed the second link go", || {
        !hub.handle.runner_connected()
    })
    .await;
    let mut back = Puppet::join(&hub.hub_url, "runner-x").await;
    back.say(ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt,
        outcome: JobOutcome::Exited { code: 0 },
        chunks_sent: 0,
        metrics: JobMetrics::default(),
        steps: Vec::new(),
    })
    .await;
    hub.wait_until("the runner holding the attempt is believed", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;
    assert_eq!(
        hub.record(&job_id).state,
        JobState::Finished {
            outcome: JobOutcome::Exited { code: 0 }
        },
        "the verdict is the holder's, and 99 was never the job's"
    );
}

/// A runner holding the state of a hub that no longer exists.
///
/// This needs no hand-edited file and no second machine: recreate a hub's
/// state root and its job ids start again at `j0000000001`, while a runner's
/// spool from the root that is gone is still on its disk and still names
/// `j0000000001`. The handshake is where that is caught, because it is the
/// only place where both sides say what they believe before anything is
/// recorded.
#[tokio::test]
async fn a_runner_announcing_another_state_root_is_turned_away_naming_both() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mine = hub.store.state_root_id().to_owned();
    assert!(
        !mine.is_empty(),
        "a state root has an identity from its first moment"
    );

    let turned_away = *Puppet::dial(
        &hub.hub_url,
        "wsl-pc",
        "run-one",
        Some("root-of-a-hub-that-is-gone"),
    )
    .await
    .expect_err("a runner carrying another state root's state is not served");
    let FromHub::StandBy {
        reason,
        retry_in_ms,
    } = turned_away
    else {
        panic!("waiting is not being refused (D38): {turned_away:?}");
    };
    assert!(
        reason.contains(&mine) && reason.contains("root-of-a-hub-that-is-gone"),
        "both roots have to be named, or nobody can act on this: {reason}"
    );
    assert!(
        retry_in_ms > 0,
        "a runner told to stand by is told how long: {reason}"
    );

    // It was never admitted, so nothing is dispatched onto that connection: a
    // submission in this window is refused by name rather than handed to a
    // runner the hub is not serving.
    assert!(!hub.handle.runner_connected());
    let refused = herdctl::submit_job(
        &hub.requester(),
        vec!["true".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared::default(),
    )
    .await
    .expect_err("there is no runner to place a job on");
    assert!(
        refused.to_string().contains("no runner is connected"),
        "the refusal must say why: {refused:#}"
    );

    // The same runner, once it agrees about which state it is serving, is
    // welcome and is dispatched to. Being turned away is transient, which is
    // the whole difference from a refusal.
    let mut agreed = Puppet::dial(&hub.hub_url, "wsl-pc", "run-one", Some(&mine))
        .await
        .expect("a runner of this state root is welcome");
    let job_id = hub.submit("true").await;
    let (assigned, attempt, _) = agreed.take_a_job().await;
    assert_eq!(assigned, job_id);
    assert_eq!(attempt, 1);
}

/// One name, two live processes: a configuration mistake, said plainly.
///
/// Configured with one name, two runners used to supersede each other for
/// ever — forty-three dropped connections in eight seconds — because the
/// handshake carried no instance identity and the hub could not tell a redial
/// from a twin. A job dispatched in that window rode a connection that was
/// about to be cut.
///
/// So: a twin waits, with the reason; the runner that is alive and holding a
/// job is not touched; and a genuine redial still supersedes, because that is
/// the case this hub's whole reconnection story rests on (D38).
#[tokio::test]
async fn one_name_claimed_by_two_live_runners_is_said_plainly_and_nobody_is_kicked() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut incumbent = Puppet::join(&hub.hub_url, "wsl-pc").await;
    incumbent.report_health(Vec::new(), false).await;
    hub.wait_until("the incumbent has been heard from", || {
        hub.handle
            .runners()
            .first()
            .is_some_and(|runner| runner.reports_seen == 1)
    })
    .await;
    let job_id = hub.submit("sleep 1").await;
    let (assigned, attempt, _) = incumbent.take_a_job().await;
    assert_eq!(assigned, job_id);

    let twin = *Puppet::dial(&hub.hub_url, "wsl-pc", "run-the-twin", None)
        .await
        .expect_err("a name a live runner holds is not free");
    let FromHub::StandBy { reason, .. } = twin else {
        panic!("a twin waits, it is not refused and it does not kick (D38): {twin:?}");
    };
    assert!(
        reason.contains("wsl-pc")
            && reason.contains("run-the-twin")
            && reason.contains("run-wsl-pc"),
        "the reason must name the name and both instances: {reason}"
    );
    assert!(
        reason.contains("configuration mistake") && reason.contains("two names"),
        "and say plainly what to do about it: {reason}"
    );

    // The runner that was here is untouched, and still the one the hub has:
    // no flap, and the job it is running is not riding a doomed link.
    let runners = hub.handle.runners();
    assert_eq!(runners.len(), 1);
    assert_eq!(runners[0].instance.as_deref(), Some("run-wsl-pc"));
    assert_eq!(runners[0].jobs.len(), 0, "the report named no job yet");
    incumbent
        .report_health(
            vec![RunningJob {
                job_id: job_id.clone(),
                attempt,
                chunks_sent: 0,
            }],
            false,
        )
        .await;
    assert!(
        matches!(incumbent.next().await, Some(FromHub::HealthRecorded {})),
        "the incumbent's link is alive and the hub is still answering on it"
    );

    // A redial — the same process, dialling again — does supersede, and
    // finishes the job it was given before the gap.
    let mut again = Puppet::join(&hub.hub_url, "wsl-pc").await;
    assert!(
        incumbent.next().await.is_none(),
        "a superseded connection is dropped, not left half alive"
    );
    again
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: JobMetrics::default(),
            steps: Vec::new(),
        })
        .await;
    hub.wait_until("the redial's verdict is recorded", || {
        matches!(hub.record(&job_id).state, JobState::Finished { .. })
    })
    .await;
    assert_eq!(
        hub.record(&job_id).attempts.len(),
        1,
        "nothing in this was a lost attempt: {:?}",
        hub.record(&job_id).attempts
    );
}

/// **A terminal record is immutable, and that is the whole of this test.**
///
/// The job ran, the runner reported how it ended, the hub wrote it down and
/// answered. Then the very same runner — right name, right attempt number,
/// nothing forged about its identity at all — sends a log chunk and a second
/// verdict about that job. Before this, both got through: the chunk landed in
/// a log that had been closed and fsynced, carrying a gap marker that claimed
/// four chunks of the job's output had been lost, and the metrics were
/// replaced by figures nobody measured, with two of the real ones dropped.
/// The outcome itself held, which is exactly what made it easy to miss.
///
/// Nothing is written to a job that is over. The attempt that produced the
/// verdict is over with it — that is the stamp the happy path never made —
/// and the frame is refused by name.
#[tokio::test]
async fn nothing_is_written_to_a_job_that_is_over_not_even_by_its_own_runner() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    let mut runner = Puppet::join(&hub.hub_url, "runner-x").await;
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
            data: b"the output this job really produced\n".to_vec(),
        })
        .await;
    let measured = JobMetrics {
        log_bytes: 35,
        wall_time_ms: 12,
        host_load_milli: Some(700),
        bytes_written: Some(4096),
        ..JobMetrics::default()
    };
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 1,
            metrics: measured.clone(),
            steps: Vec::new(),
        })
        .await;
    // Waited on the acknowledgement, not on a poll: when it arrives the end
    // is on the hub's disk and this connection has nothing left pending.
    runner.wait_for_the_result_ack(&job_id).await;
    assert!(matches!(
        hub.record(&job_id).state,
        JobState::Finished { .. }
    ));

    // The record as a reader has now been given it: this is what may never
    // change again.
    let closed = hub.record(&job_id);
    let log_when_closed = hub.log(&job_id);
    let events_when_closed = hub.events(&job_id).len();
    assert!(
        closed.attempts[0].ended_at.is_some(),
        "the attempt that produced the verdict is over too: {closed:?}"
    );
    assert!(
        closed.attempts[0].ended.is_none(),
        "and it has nothing to excuse: {closed:?}"
    );
    assert!(
        !closed.attempt_is_current(attempt),
        "so no frame about it is the one the hub is waiting for: {closed:?}"
    );

    // And now the late frames, from the runner the attempt really was handed
    // to. The chunk number is deliberately 5 — four ahead of what the log
    // holds — because what used to happen was worse than the chunk itself:
    // the hub wrote a gap marker saying four chunks of this job's output had
    // been lost, onto a log that was complete.
    runner
        .say(ToHub::LogChunk {
            job_id: job_id.clone(),
            attempt,
            stream: LogStream::Stdout,
            seq: 5,
            data: b"a paragraph this job never wrote\n".to_vec(),
        })
        .await;
    let refused = runner.next_refusal().await;
    let Some(reason) = &refused else {
        panic!("a frame about a job that is over is refused by name: {refused:?}");
    };
    assert!(
        reason.contains(job_id.as_str()) && reason.contains("is over"),
        "the refusal names the job and that its record is final: {reason}"
    );

    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 77 },
            chunks_sent: 6,
            metrics: JobMetrics {
                log_bytes: 999,
                wall_time_ms: 999,
                ..JobMetrics::default()
            },
            steps: Vec::new(),
        })
        .await;
    let refused = runner.next_refusal().await;
    assert!(
        refused
            .as_deref()
            .is_some_and(|reason| reason.contains("is over")),
        "and so is a second verdict: {refused:?}"
    );

    let after = hub.record(&job_id);
    assert_eq!(
        after, closed,
        "the record of a job that is over never moves"
    );
    assert_eq!(
        after.metrics, measured,
        "the measurements are the ones the job's own attempt reported"
    );
    assert_eq!(
        hub.log(&job_id),
        log_when_closed,
        "and its log is byte for byte what the job wrote"
    );
    assert!(
        !hub.log(&job_id).contains("never reached this log"),
        "a complete log must never be given a gap marker: {}",
        hub.log(&job_id)
    );
    assert_eq!(
        hub.events(&job_id).len(),
        events_when_closed,
        "nothing was appended to its history either: {:?}",
        hub.events(&job_id)
    );
}

/// Dial and take the hub's first answer, whatever it is.
///
/// [`Puppet::dial`] insists on the welcome, because a peer that has said who
/// it is gets one even when the hub is about to turn it away. A peer that has
/// *not* is refused before the welcome, so this is the only way to read that
/// refusal.
async fn the_hub_s_first_answer(hub_url: &str, name: &str, instance: &str) -> FromHub {
    let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
        .await
        .expect("the hub accepts a connection");
    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        role: Role::Runner,
        token: None,
        name: Some(name.to_owned()),
        instance: Some(instance.to_owned()),
        state_root: None,
        health_every_ms: None,
        // A peer whose identity the hub is about to refuse offers nothing.
        capabilities: Default::default(),
        host: herd_proto::HostSize::default(),
        preference: 0,
        load_milli: None,
        switch: herd_proto::Switch::On,
    };
    socket
        .send(Message::text(to_frame(&hello).expect("encodes")))
        .await
        .expect("hello goes out");
    match tokio::time::timeout(PATIENCE, socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            from_frame(text.as_str()).expect("the hub speaks the protocol")
        }
        other => panic!("the hub said nothing a peer can read: {other:?}"),
    }
}
/// A blank identity is no identity (and `Some("")` is blank).
///
/// M3 refuses a runner that announces neither a name nor an instance, because
/// an attempt belongs to the runner it was handed to and a nameless peer
/// cannot be told from any other. `instance: ""` walked straight through that
/// refusal — it *has* an instance as far as an `Option` is concerned — and the
/// peer ended up wearing a blank effective name, which matches every other
/// blank-named peer's attempt. Two such peers were enough: the first took a
/// dispatch, the second wrote `exited 77` and a forged line onto it.
#[tokio::test]
async fn a_peer_whose_name_and_instance_are_blank_is_refused_like_a_nameless_one() {
    let hub = Quick::start(Duration::from_secs(60)).await;

    for (name, instance) in [("", ""), ("   ", "\t"), ("", " ")] {
        let turned_away = the_hub_s_first_answer(&hub.hub_url, name, instance).await;
        let FromHub::Refused { reason } = turned_away else {
            panic!("a blank identity will not change by dialling again (D13): {turned_away:?}");
        };
        assert!(
            reason.contains("no name") && reason.contains("no instance"),
            "the refusal says what is missing: {reason}"
        );
        assert!(
            !hub.handle.runner_connected(),
            "and such a peer is never admitted, so nothing is ever dispatched to it"
        );
    }

    // There is therefore no blank-named runner for a second one to write onto:
    // a job submitted in this window is refused by name, not placed on a peer
    // the hub is not serving.
    let refused = herdctl::submit_job(
        &hub.requester(),
        vec!["true".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared::default(),
    )
    .await
    .expect_err("there is no runner to place a job on");
    assert!(
        refused.to_string().contains("no runner is connected"),
        "the refusal must say why: {refused:#}"
    );

    // A runner with no name but a real instance still wears it: that is the
    // case M3 kept, and it is not this one.
    let nameless = Puppet::dial(&hub.hub_url, "", "run-3fa91c", None)
        .await
        .expect("a runner that says which process it is has an identity");
    drop(nameless);
    hub.wait_until("the hub has that runner", || {
        hub.handle
            .runners()
            .first()
            .is_some_and(|runner| runner.name == "run-3fa91c")
    })
    .await;
}

/// Two timings, each legal on its own side, that together kill a healthy job.
///
/// The hub refuses at startup a silence threshold that does not outlast three
/// of the reports it *expects*; the runner refuses one that does not outlast
/// three of the reports it *sends*. Both pass for a hub losing a runner after
/// 3 s while expecting a report every second, against a runner that really
/// reports every ten — and that pair spent both attempts of a quiet job in
/// about six seconds and ended it `lost`. Only the pairing is wrong, so the
/// runner's real cadence crosses the handshake and the pairing is refused by
/// its figures.
#[tokio::test]
async fn a_runner_whose_real_cadence_cannot_satisfy_the_hub_is_refused_by_figures() {
    let hub = Quick::start(Duration::from_secs(3)).await;

    let turned_away = *Puppet::dial_saying(
        &hub.hub_url,
        "wsl-pc",
        "run-one",
        None,
        Some(Duration::from_secs(10)),
    )
    .await
    .expect_err("a cadence that cannot satisfy this hub's threshold is not served");
    let FromHub::Refused { reason } = turned_away else {
        panic!("a cadence does not change by dialling again, so this is terminal: {turned_away:?}");
    };
    assert!(
        reason.contains("3s") && reason.contains("10s") && reason.contains("30s"),
        "both figures and the margin they need are named: {reason}"
    );
    assert!(
        !hub.handle.runner_connected(),
        "and no job is ever placed on a runner the hub cannot keep its promises about"
    );

    // The same runner, reporting often enough, is welcome and is dispatched
    // to: what was refused is a pairing, not a machine.
    let mut fast = Puppet::dial_saying(
        &hub.hub_url,
        "wsl-pc",
        "run-two",
        None,
        Some(Duration::from_millis(300)),
    )
    .await
    .expect("a runner whose cadence the threshold outlasts three times over");
    let job_id = hub.submit("true").await;
    let (assigned, attempt, _) = fast.take_a_job().await;
    assert_eq!(assigned, job_id);
    assert_eq!(attempt, 1);
}

/// A declared command reaches the runner as a declaration, and the next
/// attempt of it is handed the same one (E-11, D106, E-21).
///
/// What this is really about is the requeue. The declaration is held on the
/// record, not by the runner that first saw it, so that a job whose runner
/// disappears runs the same four steps on the same commit on the next
/// machine. A hub that kept it in memory beside the live attempt would pass
/// for attempt 1 and hand attempt 2 a bare command line — a job that ran
/// something nobody declared, reported as if it had not.
#[tokio::test]
async fn a_declared_command_is_handed_over_whole_on_every_attempt() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    const COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";
    let declaration = herd_proto::Declaration {
        command: "a-declared-command".to_owned(),
        commit: COMMIT.to_owned(),
        args: vec!["--only".to_owned(), "one".to_owned()],
        executor: herd_proto::Executor {
            up: format!("tool ws up {{job}} {COMMIT}"),
            fetch: format!("tool ws run {{job}} -- 'git switch --detach {COMMIT}'"),
            run: "tool ws run {job} -- {line}".to_owned(),
            down: "tool ws down {job} --purge".to_owned(),
            env: std::collections::BTreeMap::from([(
                "RESULTS".to_owned(),
                "{out}/results.json".to_owned(),
            )]),
        },
    };
    // A runner is connected before the submission: the hub refuses a job it
    // has no machine for at all, so without this the test would be about
    // that instead.
    let mut holder = Puppet::join(&hub.hub_url, "runner-x").await;
    let job_id = herdctl::submit_job(
        &hub.requester(),
        vec!["a-line --only one".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared {
            declaration: Some(declaration.clone()),
            timeout_s: Some(2700),
            ..herdctl::Declared::default()
        },
    )
    .await
    .expect("the submission is accepted");

    let first = loop {
        match holder.next().await {
            Some(FromHub::AssignJob {
                job_id: assigned,
                attempt,
                command,
                declaration: carried,
                timeout_s,
            }) => break (assigned, attempt, command, carried, timeout_s),
            Some(_) => continue,
            None => panic!("the hub never handed this runner the job"),
        }
    };
    let (assigned, attempt, command, carried, timeout_s) = first;
    assert_eq!(assigned, job_id);
    assert_eq!(attempt, 1);
    assert_eq!(
        command,
        vec!["a-line --only one".to_owned()],
        "a declared job's command is the line going into the run hook's {{line}}"
    );
    assert_eq!(
        carried.as_ref(),
        Some(&declaration),
        "the four hooks, the commit and the arguments, unchanged"
    );
    assert_eq!(timeout_s, Some(2700), "and the cap the command declared");

    // The runner gives the attempt back without a verdict, and E-21 gives
    // the job one more go. Nothing in the hub holds a declaration anywhere
    // but the record, so what the next attempt is handed is what the record
    // says — and this is the assertion that would fail if a hub ever kept it
    // beside the live attempt.
    holder
        .say(ToHub::AttemptAbandoned {
            job_id: job_id.clone(),
            attempt,
            reason: "this runner is draining".to_owned(),
        })
        .await;
    let second = loop {
        match holder.next().await {
            Some(FromHub::AssignJob {
                attempt,
                declaration: carried,
                timeout_s,
                ..
            }) => break (attempt, carried, timeout_s),
            Some(_) => continue,
            None => panic!("the hub never handed the job out a second time"),
        }
    };
    let (attempt, carried, timeout_s) = second;
    assert_eq!(attempt, 2, "the second go");
    assert_eq!(
        carried.as_ref(),
        Some(&declaration),
        "the second attempt is handed the same four hooks and the same commit"
    );
    assert_eq!(timeout_s, Some(2700));

    // And it is on the record, which is where a reader with no Herd running
    // finds what the job was (E-53, E-54).
    let record = hub.record(&job_id);
    assert_eq!(record.declaration.as_ref(), Some(&declaration));
    assert_eq!(record.timeout_s, Some(2700));
}

/// A declaration no runner could execute is refused at the submission, with
/// the requester standing there to be told (E-11, E-36).
#[tokio::test]
async fn a_run_hook_that_would_never_run_the_line_is_refused_at_submission() {
    let hub = Quick::start(Duration::from_secs(60)).await;
    // A runner has to be connected, or the refusal would be about that
    // instead and this test would pass for the wrong reason.
    let _runner = Puppet::join(&hub.hub_url, "runner-x").await;
    let refused = herdctl::submit_job(
        &hub.requester(),
        vec!["a-line".to_owned()],
        LabelSet::default(),
        Priority::Step,
        herdctl::Declared {
            declaration: Some(herd_proto::Declaration {
                command: "a-declared-command".to_owned(),
                commit: "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68".to_owned(),
                args: Vec::new(),
                executor: herd_proto::Executor {
                    up: "tool ws up {job}".to_owned(),
                    fetch: "tool ws fetch {job}".to_owned(),
                    // No {line}: their workspace tool would run and its own
                    // exit code would be reported as a verdict about the
                    // consumer's tests.
                    run: "tool ws run {job}".to_owned(),
                    down: "tool ws down {job}".to_owned(),
                    env: std::collections::BTreeMap::new(),
                },
            }),
            ..herdctl::Declared::default()
        },
    )
    .await
    .expect_err("a run hook with no {line} is refused");
    let said = format!("{refused:#}");
    assert!(
        said.contains("{line}") && said.contains("a-declared-command"),
        "the refusal names the token and the command: {said}"
    );
}
