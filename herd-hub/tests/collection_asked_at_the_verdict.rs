//! **A verdict that arrives after the join scan still gets its collection
//! asked for** (D148, D167).
//!
//! Measured on the first consumer's instance on 2026-10-31, v1.0.0. Their
//! runner's link dropped for 71 s at host load 172. A job finished inside
//! that window, so the runner held its verdict (`herd-runner/src/lib.rs`
//! `Session::finish`, the "ended while there was no hub to tell" line). On
//! the next link the verdict was re-offered and the hub recorded
//! `finished, exited 0` — and its **17 output files, 42 MiB, never came
//! home**. The requester saw `herdctl` exit 2 with 17 *"did NOT get"* lines
//! and the record said the job had succeeded.
//!
//! **The race, which is the whole defect.** The two halves of a streamed
//! result come home by two different mechanisms, and on a reconnect they
//! diverge:
//!
//! * the **verdict** is re-offered by the runner, unconditionally, on every
//!   link until something acknowledges it (D34). It needs nothing of the hub.
//! * the **bytes** are hub-driven. `pump_files` returns on its first
//!   statement when there is no link, and `Session::attach` deliberately does
//!   not restart it — the runner's own words at
//!   `herd-runner/src/lib.rs:3322-3324` are *"the next connection's
//!   `JobFilesHeld` starts them again"*. So the collection moves only when
//!   the hub asks.
//!
//! The hub asked at join, from `ask_for_the_files_still_owed`, whose own
//! comment says it is *"sent before the hub reads a frame"*. At that instant
//! the record carried **no manifest at all** — the held verdict had not
//! landed yet — so `collection_owed`'s first substantive test
//! (`!record.outputs.streamed || record.outputs.files.is_empty()`) answered
//! `None`, the hub named nothing, and the pump was never started.
//! Milliseconds later the re-offered verdict wrote the 17-file manifest.
//! **Nothing ever asked again**: `look_at_a_held_verdict` sends no
//! `JobFilesHeld`, it gives up and reports. After `COLLECTION_SILENCE` the
//! verdict was released naming every missing path — D167 working exactly as
//! specified, on a collection nobody had asked for.
//!
//! So the ask has to happen where the manifest becomes known, which is the
//! `ToHub::JobFinished` arm, the moment `record_the_end` has written it. That
//! makes the join scan's ordering structurally irrelevant.
//!
//! **Why a frame-driven peer and not the runner binary.** What is under test
//! is one hub decision, and the runner's half of it is settled by its own
//! source: for a collection held in memory there is exactly one restarter,
//! `Session::files_held` on `FromHub::JobFilesHeld`, because the other —
//! `offer_swept_collections` — filters `files.swept`, true only for a
//! collection read off the disk at startup. A peer that sends its bytes only
//! when asked is therefore a faithful model of the real runner here, and it
//! is **deterministic**: no clock, no sleep, no cut socket to simulate. The
//! barrier that makes "the hub did not ask" readable is the suite's existing
//! one — the answer to a health report sent *after* the verdict, since frames
//! on one link are ordered, so anything the verdict's handling emits is in
//! front of it.

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// One name, dialling twice: the same runner *process* losing a link and
/// getting it back, which is the consumer's case. Not a restart — a restarted
/// process holds nothing in memory and takes the swept path instead, which
/// `spools_resumed.rs` is about.
const RUNNER: &str = "a-runner-whose-link-dropped";
const ONE_LIFE: &str = "the-process-that-held-the-verdict";

/// As many files as the consumer lost, small enough that each moves in one
/// chunk. The count is the thing worth keeping: the assertion that failed in
/// production is *zero of seventeen*.
const FILES: usize = 17;
const BYTES: usize = 64;

fn announced(index: usize) -> String {
    format!("evidence/screenshot-{index}.ppm")
}

fn contents(index: usize) -> Vec<u8> {
    vec![b'a' + (index % 26) as u8; BYTES]
}

/// A peer driven frame by frame, so the test decides exactly when the verdict
/// is offered and whether the bytes follow.
struct Peer {
    socket: Link,
}

impl Peer {
    async fn runner(hub_url: &str, name: &str, token: &str, instance: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a runner");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Runner,
            token: Some(token.to_owned()),
            name: Some(name.to_owned()),
            instance: Some(instance.to_owned()),
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize {
                memory_mib: Some(8192),
                threads: Some(4),
                memory_floor_mib: Some(1024),
                tmpfs_mib: Some(4096),
            },
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            jobs_at_once: None,
            mesh_endpoint: None,
        };
        let mut joined = Self {
            socket: {
                socket
                    .send(Message::text(to_frame(&hello).expect("hello encodes")))
                    .await
                    .expect("hello is sent");
                socket
            },
        };
        match joined.next().await {
            Some(FromHub::Welcome { .. }) => joined,
            other => panic!("an approved runner is welcomed, got {other:?}"),
        }
    }

    async fn say(&mut self, message: ToHub) {
        self.socket
            .send(Message::text(
                to_frame(&message).expect("the frame encodes"),
            ))
            .await
            .expect("the frame is sent");
    }

    async fn next(&mut self) -> Option<FromHub> {
        loop {
            let frame = self.socket.next().await?.expect("the frame is readable");
            if frame.is_close() {
                return None;
            }
            let Ok(text) = frame.to_text() else { continue };
            if text.is_empty() {
                continue;
            }
            return Some(from_frame(text).expect("the hub speaks the protocol"));
        }
    }

    /// Every frame up to and including the answer to a health report.
    ///
    /// **The barrier, and the reason this test needs no clock.** The hub
    /// handles one connection's frames in order and answers every report with
    /// `HealthRecorded`. So a report sent after the verdict bounds everything
    /// the verdict's handling will ever emit: a `JobFilesHeld` that is not in
    /// this list is one the hub did not send.
    async fn through_a_health_report(&mut self) -> Vec<FromHub> {
        self.say(ToHub::Health {
            jobs: Vec::new(),
            draining: false,
            load_milli: None,
            switch: herd_proto::Switch::On,
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        })
        .await;
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .expect("the hub answers a health report inside the suite's patience")
                .expect("the link stays up");
            let done = matches!(frame, FromHub::HealthRecorded {});
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }
}

/// The verdict a runner holds while there is no hub to tell: exited 0, with a
/// streamed collection announced and not a byte of it sent.
fn the_held_verdict(job_id: &JobId, attempt: u32) -> ToHub {
    ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt,
        outcome: herd_proto::JobOutcome::Exited { code: 0 },
        chunks_sent: 0,
        metrics: herd_proto::JobMetrics::default(),
        steps: Vec::new(),
        outputs: herd_proto::JobOutputs {
            files: (0..FILES)
                .map(|index| {
                    herd_proto::OutputFile::announced(
                        announced(index),
                        BYTES as u64,
                        herd_proto::sha256_hex(&contents(index)),
                    )
                })
                .collect(),
            refused: None,
            skipped: Vec::new(),
            streamed: true,
        },
    }
}

fn files_held_for<'a>(seen: &'a [FromHub], job_id: &JobId) -> Option<&'a FromHub> {
    seen.iter()
        .find(|frame| matches!(frame, FromHub::JobFilesHeld { job_id: of, .. } if of == job_id))
}

/// How many of the announced files are on the hub's disk at full length, as a
/// reader outside Herd would count them.
fn whole_files_on_disk(state_dir: &std::path::Path, job_id: &JobId) -> usize {
    let kept = support::job_dir(state_dir, job_id).join("files");
    (0..FILES)
        .filter(|index| {
            std::fs::metadata(kept.join(announced(*index)))
                .map(|at| at.len() == BYTES as u64)
                .unwrap_or(false)
        })
        .count()
}

/// **The consumer's defect, as a test.**
///
/// A job whose verdict was produced with the link down and delivered by the
/// runner's next link. The hub's join scan on that link ran before the frame
/// and found nothing owed, because the manifest did not exist yet. The
/// question is whether the hub asks for the collection once it does.
///
/// On `7309b16` it does not, and the assertion that fails is the production
/// one: zero of seventeen files, with the job already `finished`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verdict_delivered_on_a_new_link_has_its_collection_asked_for() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);

    // The link the job was handed out on.
    let mut first = Peer::runner(&harness.hub_url, RUNNER, &token, ONE_LIFE).await;
    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let attempt = loop {
        match first.next().await {
            Some(FromHub::AssignJob { attempt, .. }) => break attempt,
            Some(_) => continue,
            None => panic!("the hub never handed this runner the job"),
        }
    };

    // The link drops. The job finishes in that window, so the verdict and
    // every byte of its collection stay with the runner, which is still
    // running and still holding them in memory.
    drop(first);
    support::wait_until("the hub notices the link closed", || {
        !harness.runner_joined()
    })
    .await;

    // The next link. **The hub's owed-collections scan runs here, before it
    // reads a frame** — against a record that carries no manifest, so it
    // names nothing. That is the race, and from here on the collection's only
    // hope is that something else asks.
    let mut again = Peer::runner(&harness.hub_url, RUNNER, &token, ONE_LIFE).await;
    again.say(the_held_verdict(&job_id, attempt)).await;
    let seen = again.through_a_health_report().await;

    assert!(
        seen.iter()
            .any(|frame| matches!(frame, FromHub::JobResultAck { job_id: of } if *of == job_id)),
        "the hub takes a verdict re-offered on a new link: {seen:?}"
    );
    assert_eq!(
        support::file_state_of(harness.state_dir(), &job_id),
        "finished",
        "the record says the job succeeded, which is what makes a lost collection silent"
    );

    let asked = files_held_for(&seen, &job_id).unwrap_or_else(|| {
        panic!(
            "the hub recorded {job_id} as finished with a manifest of {FILES} streamed \
             file(s) and never asked its runner for one of them. It holds {} of {FILES} \
             on disk, the runner is on the line, and nothing will ask again: the join \
             scan already ran, no chunk will arrive to trigger the fallback, and the \
             held verdict will be released naming every missing path. This is the first \
             consumer's 2026-10-31 loss of 17 files. Frames the hub sent: {seen:?}",
            whole_files_on_disk(harness.state_dir(), &job_id)
        )
    });
    let FromHub::JobFilesHeld {
        attempt: of_attempt,
        held,
        ..
    } = asked
    else {
        unreachable!("matched above");
    };
    assert_eq!(*of_attempt, attempt, "the attempt that produced the files");
    assert!(
        held.is_empty(),
        "not a byte of it arrived, and the hub says so rather than guessing: {held:?}"
    );

    // **On the same connection, with no second redial.** This peer is the one
    // the verdict arrived on; the test never dials a third time. That is the
    // whole gain: the bytes come home on the link that brought the verdict,
    // instead of waiting for a redial nobody will make.
    for index in 0..FILES {
        again
            .say(ToHub::JobFileChunk {
                job_id: job_id.clone(),
                attempt,
                path: announced(index),
                offset: 0,
                data: contents(index),
            })
            .await;
    }
    loop {
        match again.next().await {
            Some(FromHub::JobFilesAck { job_id: of, .. }) => {
                assert_eq!(of, job_id);
                break;
            }
            Some(_) => continue,
            None => panic!("the hub never acknowledged the collection it had asked for"),
        }
    }
    assert!(
        harness.runner_joined(),
        "the collection came home on the link the verdict arrived on"
    );
    assert_eq!(
        whole_files_on_disk(harness.state_dir(), &job_id),
        FILES,
        "every announced file is on the hub's disk at its length"
    );
}

/// **And the ask is not sent where there is nothing to ask for.**
///
/// The counterpart, so the new frame is not simply unconditional: a verdict
/// that announces no streamed collection — the ordinary job, which is almost
/// every job — gets no `JobFilesHeld`, and the hub does not read the record
/// again to find that out.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verdict_with_no_streamed_collection_is_asked_nothing() {
    let harness = Harness::with_hub().await;
    let token = harness.approve("a-runner-whose-job-left-nothing");
    let mut runner = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-job-left-nothing",
        &token,
        "its-one-life",
    )
    .await;
    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let attempt = loop {
        match runner.next().await {
            Some(FromHub::AssignJob { attempt, .. }) => break attempt,
            Some(_) => continue,
            None => panic!("the hub never handed this runner the job"),
        }
    };
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: herd_proto::JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: herd_proto::JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs::default(),
        })
        .await;
    let seen = runner.through_a_health_report().await;
    assert!(
        files_held_for(&seen, &job_id).is_none(),
        "a job that left no streamed collection is asked for nothing: {seen:?}"
    );
}
