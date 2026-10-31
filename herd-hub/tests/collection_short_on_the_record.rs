//! **A collection the hub gives up on is named on the job's own record**
//! (D167).
//!
//! Measured on the first consumer's instance on 2026-10-31, v1.0.0. A job
//! finished while its runner's link was down; the verdict was re-offered on
//! the next link and recorded `finished, exited 0`, and its **17 output
//! files, 42 MiB, never came home**. `collection_asked_at_the_verdict.rs`
//! is the fix for the cause. This file is about the symptom, which outlives
//! it: when some *other* cause leaves a collection short, nothing in the
//! record ever says so.
//!
//! **Stated precisely, because the overstatement is easy.** Three things
//! already existed before this file:
//!
//! * the hub printed `… is recorded and its verdict waits for its files: …`
//!   and, 30 s later, `… 's collection did not finish arriving: …`, with
//!   real figures in both — on its **own stdout**, which is not a record;
//! * the requester received both as `JobNote`s — in a client log the
//!   consumer had already purged by the time they asked us what happened;
//! * `job.json`'s `outputs.files` held all 17 paths with lengths and
//!   digests against an empty `files/`, so a job that produced nothing and a
//!   job whose outputs were dropped are two **distinguishable** states of
//!   one record.
//!
//! So the loss was *derivable* and **nothing derived it**: no event, no
//! counter, no reconciliation. That is the gap between a green nightly and a
//! silent 42 MiB, and it is what the `collection_short` line closes.
//!
//! **Why this test needs no clock.** `COLLECTION_SILENCE` is 30 s and is the
//! one real clock on this path — but it is not the only road to the give-up.
//! `where_the_collection_stands` bounds the wait on the runner *first*, in
//! its own words *"the runner is the first bound, and it needs no clock"*: a
//! machine that is not connected is sending nothing, so there is no silence
//! to time. This fixture drives that bound — the runner delivers a verdict
//! announcing a streamed collection, sends not a byte of it, and goes away —
//! and the give-up is then reached by a request rather than by a timer. No
//! sleep, no cut socket, no `tokio::time::pause`, and no wall-clock flake of
//! the ORCH-244 / ORCH-321 / ORCH-324 family.
//!
//! **And a frame-driven peer rather than the runner binary**, for
//! `collection_asked_at_the_verdict.rs`'s reason: what is under test is one
//! hub decision, and a peer that sends its bytes only when asked — here,
//! never — is a faithful model of a runner holding a collection in memory.

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// One name, dialling twice: the same runner *process* losing a link and
/// getting it back, which is the consumer's case.
const RUNNER: &str = "a-runner-that-held-a-collection";
const ONE_LIFE: &str = "the-process-that-held-the-verdict";

/// As many files as the consumer lost, small enough that each moves in one
/// chunk. The count is the thing worth keeping: the figure that was never
/// written down is *zero of seventeen*.
const FILES: usize = 17;
const BYTES: u64 = 64;

fn announced(index: usize) -> String {
    format!("evidence/screenshot-{index}.ppm")
}

fn contents(index: usize) -> Vec<u8> {
    vec![b'a' + (index % 26) as u8; BYTES as usize]
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
        socket
            .send(Message::text(to_frame(&hello).expect("hello encodes")))
            .await
            .expect("hello is sent");
        let mut joined = Self { socket };
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

    /// Wait for this runner to be handed the job, and say which attempt.
    async fn assigned(&mut self) -> u32 {
        loop {
            match self.next().await {
                Some(FromHub::AssignJob { attempt, .. }) => return attempt,
                Some(_) => continue,
                None => panic!("the hub never handed this runner the job"),
            }
        }
    }

    /// Every frame up to and including the answer to a health report.
    ///
    /// **The barrier, and the reason this test needs no clock.** The hub
    /// handles one connection's frames in order and answers every report
    /// with `HealthRecorded`, so a report sent after the verdict bounds
    /// everything the verdict's handling will ever emit — including the
    /// record writes it makes on the way.
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

/// A requester that says it is going to fetch, which is what asks the hub to
/// hold the verdict for it (D167).
struct Requester {
    socket: Link,
}

impl Requester {
    async fn join(hub_url: &str, name: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a requester");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Requester,
            token: None,
            name: Some(name.to_owned()),
            instance: None,
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
            jobs_at_once: None,
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            mesh_endpoint: None,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("hello encodes")))
            .await
            .expect("hello is sent");
        let mut joined = Self { socket };
        match joined.next().await {
            Some(FromHub::Welcome { .. }) => joined,
            other => panic!("a requester on this hub's version is welcomed, got {other:?}"),
        }
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

    async fn wait_for(&mut self, job_id: &JobId) {
        self.socket
            .send(Message::text(
                to_frame(&ToHub::WaitForJob {
                    job_id: job_id.clone(),
                    wants_files: true,
                })
                .expect("the frame encodes"),
            ))
            .await
            .expect("the frame is sent");
    }

    /// The verdict, with every note said on the way to it.
    async fn verdict(&mut self) -> (FromHub, Vec<String>) {
        let deadline = tokio::time::Instant::now() + 3 * PATIENCE;
        let mut notes = Vec::new();
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .unwrap_or_else(|_| panic!("no verdict arrived; notes: {notes:?}"));
            match frame {
                Some(FromHub::JobNote { note, .. }) => notes.push(note),
                Some(verdict @ FromHub::JobFinished { .. }) => return (verdict, notes),
                Some(_) => continue,
                None => panic!("the hub closed the link with no verdict; notes: {notes:?}"),
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
                        BYTES,
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

/// Every `collection_short` line of a job's history, **read as raw JSON**.
///
/// The way the first consumer reads it: they parse `events.jsonl` themselves
/// and quoted six of its lines back to us. So the assertions below are about
/// the bytes on the disk and not about this crate's types — a field renamed
/// in Rust and not on the wire would pass a typed test and break theirs.
fn short_lines(
    state_dir: &std::path::Path,
    job_id: &JobId,
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    support::events_of(state_dir, job_id)
        .into_iter()
        .filter_map(|line| {
            let object = line.as_object()?.clone();
            (object.get("event")? == "collection_short").then_some(object)
        })
        .collect()
}

/// How many of the announced files are on the hub's disk at full length.
fn whole_files_on_disk(state_dir: &std::path::Path, job_id: &JobId) -> usize {
    let kept = support::job_dir(state_dir, job_id).join("files");
    (0..FILES)
        .filter(|index| {
            std::fs::metadata(kept.join(announced(*index)))
                .map(|at| at.len() == BYTES)
                .unwrap_or(false)
        })
        .count()
}

/// **The consumer's symptom, as a test.** A finished job whose collection was
/// given up on says so on its own record, with the figures.
///
/// On `3ea839c` the record carries five lines — `waiting`, three transitions
/// and one `attempt_started` — and not one of them is about files. The verdict
/// reads `finished, exited 0`, `files/` is empty, the manifest names
/// seventeen paths, and the only two durable witnesses are the hub's stdout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_given_up_on_is_named_on_the_record() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);

    // The link the job was handed out on.
    let mut first = Peer::runner(&harness.hub_url, RUNNER, &token, ONE_LIFE).await;
    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let attempt = first.assigned().await;

    // Somebody is going to fetch, which is what makes the verdict wait for
    // the collection at all (D167). Registered while the job is still
    // running, as a real `herdctl wait --outputs-to` would be.
    let mut watching = Requester::join(&harness.hub_url, "the-requester-that-watched-it").await;
    watching.wait_for(&job_id).await;

    // The link drops. The job finishes in that window, so the verdict and
    // every byte of its collection stay with the runner.
    drop(first);
    support::wait_until("the hub notices the link closed", || {
        !harness.runner_joined()
    })
    .await;

    // The next link delivers the held verdict. The hub records it and asks
    // for the collection; this peer answers with nothing at all, which is
    // what a runner whose bytes cannot cross looks like from here.
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
        "the record says the job succeeded, which is what made the loss silent"
    );
    assert!(
        short_lines(harness.state_dir(), &job_id).is_empty(),
        "nothing is given up on while the bytes may still be crossing: a collection \
         that is merely late must not be written off"
    );

    // And now the runner goes away holding every byte. **This is the bound
    // that needs no clock**: a machine that is not connected is sending
    // nothing, so `where_the_collection_stands` gives up on the next
    // question rather than timing thirty seconds of silence.
    drop(again);
    support::wait_until("the hub notices the second link closed", || {
        !harness.runner_joined()
    })
    .await;

    // The question. A `herdctl wait --outputs-to` arriving now is answered
    // off the disk, and `say_the_collection_is_short` runs before the verdict
    // goes out — so holding this verdict is the barrier that proves the write
    // happened, with no sleep anywhere.
    let mut late = Requester::join(&harness.hub_url, "the-requester-that-asked-later").await;
    late.wait_for(&job_id).await;
    let (verdict, notes) = late.verdict().await;
    let FromHub::JobFinished { outcome, .. } = &verdict else {
        unreachable!("verdict() returns only a verdict");
    };
    assert_eq!(
        outcome,
        &herd_proto::JobOutcome::Exited { code: 0 },
        "the job itself passed — which is the whole difficulty: notes {notes:?}"
    );

    // **The assertion.** One line, on the job's own history, with the
    // figures.
    let lines = short_lines(harness.state_dir(), &job_id);
    let [short] = lines.as_slice() else {
        panic!(
            "the hub gave up on {job_id}'s collection and no line of its history says so. \
             It holds {} of {FILES} announced file(s), the record reads `{}`, the manifest \
             names every missing path, and the only durable witnesses are the hub's own \
             stdout — which is how the first consumer lost 17 files and 42 MiB under a \
             verdict a nightly reads as green. The history is: {:?}",
            whole_files_on_disk(harness.state_dir(), &job_id),
            support::file_state_of(harness.state_dir(), &job_id),
            support::events_of(harness.state_dir(), &job_id)
        )
    };
    assert_eq!(
        short.get("files_here").and_then(serde_json::Value::as_u64),
        Some(0),
        "not one of them arrived: {short:?}"
    );
    assert_eq!(
        short.get("files_owed").and_then(serde_json::Value::as_u64),
        Some(FILES as u64),
        "and the manifest named seventeen: {short:?}"
    );
    assert_eq!(
        short.get("bytes_here").and_then(serde_json::Value::as_u64),
        Some(0),
        "no byte of it either: {short:?}"
    );
    assert_eq!(
        short.get("bytes_owed").and_then(serde_json::Value::as_u64),
        Some(FILES as u64 * BYTES),
        "against the total the manifest promised: {short:?}"
    );
    assert_eq!(
        short.get("runner").and_then(serde_json::Value::as_str),
        Some(RUNNER),
        "the machine that did not finish sending them: {short:?}"
    );
    // The line carries the record version, like every other line of this
    // file — the field the consumer's own paste had dropped.
    assert_eq!(
        short
            .get("record_version")
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(herd_hub::RECORD_VERSION)),
        "a line read on its own says what it is: {short:?}"
    );

    // **The prose and the figures are one derivation.** The sentence on the
    // record is the sentence the requester was given, so a reader joining
    // the two cannot be told two different things about one collection.
    let why = short
        .get("why")
        .and_then(serde_json::Value::as_str)
        .expect("the line says why");
    assert!(
        why.contains(&format!("0 of {FILES} output file(s)")),
        "the sentence carries the same figures as the fields: {why}"
    );
    assert!(
        why.contains("is not connected to this hub"),
        "and why waiting longer bought nothing: {why}"
    );
    assert!(
        notes.iter().any(|note| note.contains(why)),
        "the record keeps the very sentence the requester was given; notes: {notes:?}"
    );

    // **And once, however many readers ask.** The give-up is reached again by
    // every late `herdctl wait --outputs-to` on this job, for ever; a line
    // per reader would be a history that grows with its readers.
    let mut later_still =
        Requester::join(&harness.hub_url, "the-requester-that-asked-later-still").await;
    later_still.wait_for(&job_id).await;
    let _ = later_still.verdict().await;
    assert_eq!(
        short_lines(harness.state_dir(), &job_id).len(),
        1,
        "a second reader asking the same question does not write a second line"
    );
}

/// **And a collection that comes home whole is never called short.**
///
/// The counterpart, so the line is not simply written for every job with a
/// streamed collection: the same runner, the same manifest, and this time the
/// bytes follow the ask. Nothing on the record says anything was lost,
/// because nothing was.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_that_comes_home_whole_is_not_called_short() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);

    let mut first = Peer::runner(&harness.hub_url, RUNNER, &token, ONE_LIFE).await;
    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let attempt = first.assigned().await;
    let mut watching = Requester::join(&harness.hub_url, "the-requester-that-gets-them").await;
    watching.wait_for(&job_id).await;

    drop(first);
    support::wait_until("the hub notices the link closed", || {
        !harness.runner_joined()
    })
    .await;

    let mut again = Peer::runner(&harness.hub_url, RUNNER, &token, ONE_LIFE).await;
    again.say(the_held_verdict(&job_id, attempt)).await;
    let _ = again.through_a_health_report().await;
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
    let (_verdict, notes) = watching.verdict().await;
    assert_eq!(
        whole_files_on_disk(harness.state_dir(), &job_id),
        FILES,
        "every announced file is on the hub's disk at its length; notes: {notes:?}"
    );
    assert!(
        short_lines(harness.state_dir(), &job_id).is_empty(),
        "a collection that arrived whole leaves no line saying it did not: {:?}",
        support::events_of(harness.state_dir(), &job_id)
    );
}
