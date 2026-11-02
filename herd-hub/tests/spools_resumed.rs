//! **What a hub does about a collection when its runner comes back**
//! (ORCH-092).
//!
//! The first consumer's runner is holding 65 GB in `outputs/`: 125 spools,
//! every one of them less than a week old. A spool is the renamed `{out}` of
//! a streamed collection, and exactly one thing in the tree removes one — the
//! hub saying it has every file (`files_acknowledged`), which needs the
//! runner **process** that produced the collection to still be holding it in
//! memory. A runner that is restarted holds nothing, so the directory stays
//! on the disk and nothing so much as enumerates it again.
//!
//! The runner's startup sweep brings those spools back under the
//! offer-and-acknowledge path rather than deleting them, and the whole design
//! rests on three facts about the hub that are asserted here against the real
//! one, over a real socket, rather than read off its source:
//!
//! 1. a collection the hub is **still owed** is asked for, unprompted, the
//!    moment the runner joins — that is the path a swept spool re-enters, and
//!    it needs no new frame, which matters because `PROTOCOL_VERSION` is
//!    frozen at 2 (D73);
//! 2. a collection the hub **already holds in full** is never *asked for*
//!    again — it is not owed, so the unprompted list at join says nothing
//!    about it — while a chunk of it offered anyway is neither written twice
//!    nor refused: the store answers with the state of the whole collection
//!    and the hub acknowledges it, which is the sentence a swept spool's offer
//!    is freed by (D19). The barrier that makes the *silence at join* readable
//!    is the answer to the first health report — the owed list is sent before
//!    the hub reads a frame, and frames on one link are ordered.
//! 3. that acknowledgement is said **once per collection per link**,
//!    never once per chunk: a collection that is already whole reaches the
//!    hub's `Complete` arm on every straggler still crossing the wire, and
//!    saying it each time sent the first consumer's runner 61 frames for one
//!    job inside one second and OOM-killed their hub twice (2026-11-02). Per
//!    link, so a restarted runner's offer is still answered — the
//!    de-duplication must not cost the one sentence that frees a spool.
//!
//! Each is asserted with a runner that **rejoins**, because that is the state
//! the sweep exists for: the second connection is a restarted runner as far as
//! the hub can tell.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The one file the job leaves, big enough that the carriage is the streamed
/// one and small enough that the test moves it in a single chunk.
const PATH: &str = "evidence/results.json";
const BYTES: usize = 64;

/// A peer driven frame by frame, so the test can be a *restarted* runner
/// rather than a runner that happens to reconnect.
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
    /// **The barrier the runner's terminal rule is built on.** The hub sends
    /// one `JobFilesHeld` per outstanding collection the instant a runner
    /// joins, before it reads anything from the connection, and it answers
    /// every report with `HealthRecorded`. So everything the hub is owed is
    /// in this list, and a collection that is not in it is one the hub is not
    /// waiting for.
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

/// Run one job to a verdict that announces a streamed collection, and say
/// which attempt produced it.
///
/// The runner says nothing about the bytes here: whether they follow is what
/// each test is about.
async fn a_job_promising_a_collection(
    harness: &Harness,
    runner: &mut Peer,
) -> (JobId, u32, Vec<u8>) {
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
    let bytes = vec![b'e'; BYTES];
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: herd_proto::JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: herd_proto::JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs {
                files: vec![herd_proto::OutputFile::announced(
                    PATH.to_owned(),
                    BYTES as u64,
                    herd_proto::sha256_hex(&bytes),
                )],
                refused: None,
                skipped: Vec::new(),
                streamed: true,
            },
        })
        .await;
    loop {
        match runner.next().await {
            Some(FromHub::JobResultAck { .. }) => break,
            Some(_) => continue,
            None => panic!("the hub never acknowledged the result"),
        }
    }
    (job_id, attempt, bytes)
}

fn files_held_for<'a>(seen: &'a [FromHub], job_id: &JobId) -> Option<&'a FromHub> {
    seen.iter()
        .find(|frame| matches!(frame, FromHub::JobFilesHeld { job_id: of, .. } if of == job_id))
}

/// **The path a swept spool re-enters.** A runner that promised a collection
/// and went away before sending it is asked for it again, unprompted, the
/// moment it comes back — which is why the startup sweep re-registers rather
/// than reaping, and why it needs no new frame on a frozen protocol (D73).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hub_asks_a_rejoining_runner_for_a_collection_it_is_still_owed() {
    let harness = Harness::with_hub().await;
    let token = harness.approve("a-runner-that-is-restarted");
    let mut runner = Peer::runner(
        &harness.hub_url,
        "a-runner-that-is-restarted",
        &token,
        "the-life-that-ran-it",
    )
    .await;
    let (job_id, attempt, _bytes) = a_job_promising_a_collection(&harness, &mut runner).await;
    // The machine goes away holding every byte it promised. Its spool is on
    // its disk and the memory that knew about it is gone.
    drop(runner);
    // The hub has to notice the machine is gone before another life of it
    // dials: a runner whose twin the hub still believes connected is asked to
    // stand by, and a test that raced that would be measuring the twin rule.
    support::wait_until("the hub notices the runner is gone", || {
        !harness.runner_joined()
    })
    .await;

    let mut restarted = Peer::runner(
        &harness.hub_url,
        "a-runner-that-is-restarted",
        &token,
        "the-life-that-swept-it-up",
    )
    .await;
    let seen = restarted.through_a_health_report().await;
    let asked = files_held_for(&seen, &job_id).unwrap_or_else(|| {
        panic!("the hub must ask a rejoining runner for what it is owed: {seen:?}")
    });
    let FromHub::JobFilesHeld {
        attempt: of_attempt,
        held,
        ..
    } = asked
    else {
        unreachable!("matched above");
    };
    assert_eq!(*of_attempt, attempt, "the attempt that produced it");
    assert!(
        held.is_empty(),
        "not a byte of it arrived, and the hub says so rather than guessing: {held:?}"
    );
}

/// **The crux, and the reason the sweep's terminal rule reads silence.**
///
/// A collection the hub already holds in full is never asked for again — it
/// is not owed, so a rejoining runner is told nothing about it — and a chunk
/// of it offered anyway is **refused**, not written twice and not
/// acknowledged. So there is no sentence a runner can get that says "you may
/// drop this spool": the acknowledgement happened once, on a connection that
/// no longer exists. What the runner has instead is the absence of the
/// question, bounded by the answer to its first health report.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_the_hub_already_holds_is_never_asked_for_again() {
    let harness = Harness::with_hub().await;
    let token = harness.approve("a-runner-whose-files-landed");
    let mut runner = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-files-landed",
        &token,
        "the-life-that-ran-it",
    )
    .await;
    let (job_id, attempt, bytes) = a_job_promising_a_collection(&harness, &mut runner).await;
    runner
        .say(ToHub::JobFileChunk {
            job_id: job_id.clone(),
            attempt,
            path: PATH.to_owned(),
            offset: 0,
            data: bytes.clone(),
        })
        .await;
    loop {
        match runner.next().await {
            Some(FromHub::JobFilesAck { job_id: of, .. }) => {
                assert_eq!(of, job_id);
                break;
            }
            Some(_) => continue,
            None => panic!("the hub never acknowledged a collection it had in full"),
        }
    }
    // The acknowledgement arrived on a connection this runner is about to
    // lose — in the real case it never arrives at all, because the process
    // was already gone.
    drop(runner);
    // The hub has to notice the machine is gone before another life of it
    // dials: a runner whose twin the hub still believes connected is asked to
    // stand by, and a test that raced that would be measuring the twin rule.
    support::wait_until("the hub notices the runner is gone", || {
        !harness.runner_joined()
    })
    .await;

    let mut restarted = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-files-landed",
        &token,
        "the-life-that-swept-it-up",
    )
    .await;
    let seen = restarted.through_a_health_report().await;
    assert!(
        files_held_for(&seen, &job_id).is_none(),
        "a collection the hub holds in full is not owed, so it is never mentioned again: \
         {seen:?}"
    );

    // **And offering it again is free.** The store sees a file already whole
    // in `files/` and answers with the state of the collection without
    // reading, writing or refusing the chunk (`write_output_chunk`, the
    // "already here and whole" arm) — so the hub acknowledges it a second
    // time and the runner may drop its spool through the one path that ever
    // removes one. This is what lets the startup sweep re-register a spool
    // instead of reaping it: the recovery costs one frame when the hub
    // already has the bytes, and recovers the result when it does not.
    restarted
        .say(ToHub::JobFileChunk {
            job_id: job_id.clone(),
            attempt,
            path: PATH.to_owned(),
            offset: 0,
            data: bytes,
        })
        .await;
    match tokio::time::timeout(Duration::from_secs(5), restarted.next()).await {
        Ok(Some(FromHub::JobFilesAck { job_id: of, .. })) => assert_eq!(of, job_id),
        other => panic!(
            "a collection the hub holds in full must be acknowledged again when it is \
             offered again, or a restarted runner could never learn that its spool may \
             go: {other:?}"
        ),
    }

    // The record is unharmed by the re-offer: one file, at its length.
    let record = support::record_of(harness.state_dir(), &job_id);
    assert_eq!(
        record["outputs"]["files"]
            .as_array()
            .map(|files| files.len()),
        Some(1),
        "{record}"
    );
}

/// How many straggler chunks the test sends after the collection is already
/// whole.
///
/// **More than one, deliberately.** A single repeat could be read as a frame
/// that raced the completion; several cannot, and the field's own figure was 61
/// for one job inside one second. Each of these takes the store's *already here
/// and whole* arm, which answers `Complete` without writing a byte — so each
/// one is a chance for the hub to say the same sentence again.
const STRAGGLERS: usize = 5;

/// **One acknowledgement per collection per link — and the first one is never
/// swallowed** (the 2026-11-02 flood).
///
/// `ChunkLanded::Complete` is not an edge. The store answers it for every chunk
/// of a file it already holds whole, so once a collection is complete each
/// chunk still crossing the wire comes back to the arm that acknowledges it.
/// With no memory of having said it, the hub said it again every time: the
/// first consumer's runner was sent the frame 61 times for one job inside one
/// second, 232 times across four collections it held nothing for, and that hub
/// was OOM-killed twice in a night.
///
/// Both halves are asserted here, because a de-duplication that swallowed the
/// *first* ack would be the worse defect: `files_acknowledged` on the runner is
/// the only path in the tree that frees a collection (D19), so an ack that
/// never goes out strands the bytes on the runner's disk for ever.
///
/// And the register is **per connection**, like the link's window and its
/// credit:
/// `a_collection_the_hub_already_holds_is_never_asked_for_again` above is the
/// same sentence on a *second* link, and it must keep arriving there — that is
/// the sentence the startup sweep's offer is answered with.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_whole_collection_is_acknowledged_once_per_link_and_never_not_at_all() {
    let harness = Harness::with_hub().await;
    let token = harness.approve("a-runner-whose-chunks-straggle");
    let mut runner = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-chunks-straggle",
        &token,
        "the-life-that-ran-it",
    )
    .await;
    let (job_id, attempt, bytes) = a_job_promising_a_collection(&harness, &mut runner).await;

    // The chunk that completes the collection. This acknowledgement is the one
    // that may never be lost.
    runner
        .say(ToHub::JobFileChunk {
            job_id: job_id.clone(),
            attempt,
            path: PATH.to_owned(),
            offset: 0,
            data: bytes.clone(),
        })
        .await;
    loop {
        match tokio::time::timeout(PATIENCE, runner.next()).await {
            Ok(Some(FromHub::JobFilesAck { job_id: of, .. })) => {
                assert_eq!(of, job_id, "the collection that completed");
                break;
            }
            Ok(Some(_)) => continue,
            other => panic!(
                "a collection that just became whole must be acknowledged, or its runner \
                 can never free it: {other:?}"
            ),
        }
    }

    // And now the stragglers: the same bytes again, for a file the hub already
    // holds whole.
    for _ in 0..STRAGGLERS {
        runner
            .say(ToHub::JobFileChunk {
                job_id: job_id.clone(),
                attempt,
                path: PATH.to_owned(),
                offset: 0,
                data: bytes.clone(),
            })
            .await;
    }
    // **Read against the health report rather than against a clock.** One
    // link, one order: the hub handles these chunks before the report that
    // follows them, so anything they produced is in this list, ahead of the
    // `HealthRecorded` that ends it. A sleep would measure the host's speed.
    let after = runner.through_a_health_report().await;
    let said_again: Vec<&FromHub> = after
        .iter()
        .filter(|frame| matches!(frame, FromHub::JobFilesAck { job_id: of, .. } if *of == job_id))
        .collect();
    assert!(
        said_again.is_empty(),
        "{STRAGGLERS} straggler chunk(s) of a collection this hub already acknowledged drew \
         {} more acknowledgement(s) on the same link. This is the frame the first \
         consumer's runner received 61 times in one second, on a hub that was then \
         OOM-killed twice: {after:?}",
        said_again.len()
    );
    assert!(
        after
            .iter()
            .any(|frame| matches!(frame, FromHub::HealthRecorded {})),
        "the link has to stay alive through the stragglers, or the silence above is the \
         silence of a dropped connection: {after:?}"
    );

    // **The other side of the boundary: a new link says it again.** The set is
    // per connection, so the offer a restarted runner makes is still answered
    // — which is the only way a swept spool is ever freed.
    drop(runner);
    support::wait_until("the hub notices the runner is gone", || {
        !harness.runner_joined()
    })
    .await;
    let mut restarted = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-chunks-straggle",
        &token,
        "the-life-that-swept-it-up",
    )
    .await;
    let _ = restarted.through_a_health_report().await;
    restarted
        .say(ToHub::JobFileChunk {
            job_id: job_id.clone(),
            attempt,
            path: PATH.to_owned(),
            offset: 0,
            data: bytes,
        })
        .await;
    match tokio::time::timeout(PATIENCE, restarted.next()).await {
        Ok(Some(FromHub::JobFilesAck { job_id: of, .. })) => {
            assert_eq!(of, job_id, "the collection offered again")
        }
        other => panic!(
            "the acknowledgement is de-duplicated per connection, not for ever: a link that \
             has never been told its collection is whole must be told, or a swept spool \
             can never go (D19): {other:?}"
        ),
    }
}
