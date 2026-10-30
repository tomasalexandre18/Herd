//! **What a hub says about a collection when it cannot read its own records**
//! (ORCH-197).
//!
//! A runner that joins is told, unprompted, about every collection this hub
//! is still owed from it, and the answer to its first health report closes
//! that list: a collection the hub has not named by then is one the hub is
//! not waiting for, and the runner sets those bytes aside. The list comes
//! from **one** `readdir` of the jobs directory
//! (`JobStore::collections_owed`), and until this file was written a listing
//! that failed made the hub answer *every* runner with the same silence. One
//! directory, the whole fleet: every runner read a genuine silence and every
//! one of them correctly condemned its spools. 37.6 GiB of the first
//! consumer's results were already lost to a nearby mistake, and this is the
//! remaining way a healthy collection can still be set aside.
//!
//! The fixture is the listing itself, not a mock: the jobs directory is left
//! with execute permission and no read permission, which is what a `chmod`,
//! an ACL or a tree another user owns does to a hub. `fs::read_dir` then
//! fails with `EACCES` while `open("jobs/<id>/job.json")` still succeeds —
//! and that gap is the whole of the fix. The hub cannot enumerate, so it
//! claims nothing about any collection; each one a runner *offers* is
//! answered from that job's own record, which the failed listing says nothing
//! about.
//!
//! Three things are asserted here against the real hub, over a real socket:
//!
//! 1. a collection the hub is still owed is **named on its offer**, with the
//!    attempt and the holdings the join list would have carried, and named
//!    once however many chunks the offer is;
//! 2. the hub **goes on serving** while its listing is broken — the runner is
//!    welcomed, its health report is answered, and the collection lands and
//!    is acknowledged, so the bytes are recovered rather than condemned;
//! 3. a collection the hub **already holds** is still not claimed as owed,
//!    because the answer is read from the record and not invented. A hub that
//!    cannot tell what it holds must not say it holds nothing, and it must
//!    not say it is owed something either.
//!
//! The runner side of this contract needs no change: `JobFilesHeld` is the
//! frame it already resumes on, and it arrives ahead of the answer to any
//! health report queued after the offer, frames on one link being ordered.
//!
//! **That last sentence is an ordering, and nothing here can measure one.**
//! The hub is a library object and the runner is a hand-written `Peer` that
//! encodes its own frames, so the sequence this file asserts is the sequence
//! this file chose: it cannot get the ordering wrong, and its green says
//! nothing about the code that can. The real
//! `Session::the_hub_has_said_what_it_is_owed` — pumps, a window of eight
//! permits, a health interval — is driven against this same fixture by
//! `owed_scan_fails_against_the_real_runner.rs`, with both binaries as
//! processes and more collections than the window. The permission fixture
//! itself is shared with it, in `a_failed_listing/mod.rs`, so there is one
//! definition of the absence rather than two.

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod a_failed_listing;
mod support;

use a_failed_listing::AListingThatFails;
use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The one file each job leaves. Two chunks, so that the answer to an offer
/// can be shown to belong to the **first** of them and to arrive once.
const PATH: &str = "evidence/results.json";
const BYTES: usize = 64;
const HALF: usize = BYTES / 2;

/// A peer driven frame by frame, so a test can be a *restarted* runner rather
/// than a runner that happens to reconnect. The same shape as
/// `spools_resumed.rs`'s, which is the file this one is the broken-disk half
/// of.
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

    /// Every frame up to and including the answer to a health report — the
    /// barrier the runner's terminal rule for a spool is built on.
    ///
    /// That the answer still comes is half of "it must not stop serving": a
    /// hub that went quiet here would be dropped by its runner after four
    /// unanswered reports (`herd_runner::HUB_SILENCE_LIMIT`), and the fleet
    /// would redial for ever instead of working.
    async fn through_a_health_report(&mut self) -> Vec<FromHub> {
        self.say(ToHub::Health {
            jobs: Vec::new(),
            draining: false,
            load_milli: None,
            switch: herd_proto::Switch::On,
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

    /// Offer one chunk of a collection and collect what the hub says back,
    /// until it says the thing this test is waiting for.
    async fn offer(&mut self, job_id: &JobId, attempt: u32, offset: u64, data: Vec<u8>) {
        self.say(ToHub::JobFileChunk {
            job_id: job_id.clone(),
            attempt,
            path: PATH.to_owned(),
            offset,
            data,
        })
        .await;
    }

    /// Frames until one of them satisfies `what`, with the suite's patience
    /// and a sentence that says what was missing.
    async fn until(&mut self, what: impl Fn(&FromHub) -> bool, missing: &str) -> Vec<FromHub> {
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = match tokio::time::timeout_at(deadline, self.next()).await {
                Ok(Some(frame)) => frame,
                Ok(None) => panic!("the link went down waiting for {missing}: {seen:?}"),
                Err(_) => panic!("the hub never {missing}; it said {seen:?}"),
            };
            let done = what(&frame);
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }
}

/// Run one job to a verdict that announces a streamed collection, and say
/// which attempt produced it. Not a byte of it follows here.
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

fn files_held_for<'a>(seen: &'a [FromHub], job_id: &JobId) -> Vec<&'a FromHub> {
    seen.iter()
        .filter(|frame| matches!(frame, FromHub::JobFilesHeld { job_id: of, .. } if of == job_id))
        .collect()
}

fn is_files_held_for(job_id: &JobId) -> impl Fn(&FromHub) -> bool + '_ {
    move |frame| matches!(frame, FromHub::JobFilesHeld { job_id: of, .. } if of == job_id)
}

fn is_files_ack_for(job_id: &JobId) -> impl Fn(&FromHub) -> bool + '_ {
    move |frame| matches!(frame, FromHub::JobFilesAck { job_id: of, .. } if of == job_id)
}

/// Let the hub notice the machine is gone, so that the next life of it is not
/// asked to stand by as its own twin.
async fn after_the_runner_is_gone(harness: &Harness, runner: Peer) {
    drop(runner);
    support::wait_until("the hub notices the runner is gone", || {
        !harness.runner_joined()
    })
    .await;
}

/// **The fix, end to end.** A hub that cannot list its jobs says nothing
/// about any collection at join — it has nothing to say — and answers each
/// one from that collection's own record the moment the runner offers it.
///
/// Before this, the silence was the hub's whole answer and the runner was
/// right to read it as "nothing of yours is wanted". Both halves are
/// asserted: the silence at join, which is what the old code turned into a
/// lie by completing it, and the answer on the offer, which is what makes the
/// silence no longer the whole sentence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hub_that_cannot_list_its_jobs_answers_each_collection_it_is_offered() {
    let harness = Harness::with_hub().await;
    let token = harness.approve("a-runner-whose-hub-lost-its-listing");
    let mut runner = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-hub-lost-its-listing",
        &token,
        "the-life-that-ran-it",
    )
    .await;
    let (job_id, attempt, bytes) = a_job_promising_a_collection(&harness, &mut runner).await;
    after_the_runner_is_gone(&harness, runner).await;

    let Some(broken) = AListingThatFails::over(harness.state_dir()) else {
        herd_runner::testing::skipped(
            "a_hub_that_cannot_list_its_jobs_answers_each_collection_it_is_offered",
            AListingThatFails::WHY_IT_WOULD_BE_SKIPPED,
        );
        return;
    };
    broken.but_a_record_still_opens(&job_id);

    let mut restarted = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-hub-lost-its-listing",
        &token,
        "the-life-that-swept-it-up",
    )
    .await;
    // The hub is still serving: it welcomed this runner (above) and it
    // answers its health report (here). What it does *not* do is claim
    // anything about any collection.
    let at_join = restarted.through_a_health_report().await;
    assert!(
        files_held_for(&at_join, &job_id).is_empty(),
        "a hub that cannot read its records has nothing to say about a collection: {at_join:?}"
    );

    // The offer. One chunk of a two-chunk collection, which is how a swept
    // spool re-enters the ordinary path.
    restarted
        .offer(&job_id, attempt, 0, bytes[..HALF].to_vec())
        .await;
    let answered = restarted
        .until(
            is_files_held_for(&job_id),
            "answered the offer of a collection it is owed, although its listing failed \
             and that offer named the one record it had to read (ORCH-197)",
        )
        .await;
    let held = match answered.last() {
        Some(FromHub::JobFilesHeld {
            attempt: of_attempt,
            held,
            ..
        }) => {
            assert_eq!(
                *of_attempt, attempt,
                "the attempt that produced it, read from its own record"
            );
            held.clone()
        }
        other => unreachable!("matched by the wait: {other:?}"),
    };
    assert!(
        held.is_empty(),
        "not a byte of it had arrived when the offer came, and the hub says so rather than \
         guessing: {held:?}"
    );

    // And the bytes land. This is the half that makes the fix worth having:
    // the collection is **recovered** on a hub whose listing is broken,
    // rather than renamed `.unclaimed` on a runner's disk.
    restarted
        .offer(&job_id, attempt, HALF as u64, bytes[HALF..].to_vec())
        .await;
    let acknowledged = restarted
        .until(
            is_files_ack_for(&job_id),
            "acknowledged a collection that arrived in full while its listing was broken",
        )
        .await;
    assert!(
        files_held_for(&acknowledged, &job_id).is_empty(),
        "a collection is answered once per connection, not once per chunk: {acknowledged:?}"
    );

    // Still serving, after all of it, on the same link.
    let after = restarted.through_a_health_report().await;
    assert!(
        after
            .iter()
            .any(|f| matches!(f, FromHub::HealthRecorded {})),
        "the link must stay alive through a broken listing: {after:?}"
    );
}

/// **One listing, the whole fleet — and each runner answered on its own
/// offer.** The defect's shape is that the failure is not per runner: two
/// machines with two different collections were both told nothing. Both are
/// still told nothing at join here, because that is honest, and both are
/// answered when they offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_failed_listing_silences_every_runner_and_each_is_still_answered() {
    let harness = Harness::with_hub().await;
    let first_token = harness.approve("the-first-machine");
    let second_token = harness.approve("the-second-machine");

    let mut first = Peer::runner(
        &harness.hub_url,
        "the-first-machine",
        &first_token,
        "the-life-that-ran-it",
    )
    .await;
    let (first_job, first_attempt, first_bytes) =
        a_job_promising_a_collection(&harness, &mut first).await;
    after_the_runner_is_gone(&harness, first).await;

    let mut second = Peer::runner(
        &harness.hub_url,
        "the-second-machine",
        &second_token,
        "the-life-that-ran-it",
    )
    .await;
    let (second_job, second_attempt, second_bytes) =
        a_job_promising_a_collection(&harness, &mut second).await;
    after_the_runner_is_gone(&harness, second).await;

    let Some(broken) = AListingThatFails::over(harness.state_dir()) else {
        herd_runner::testing::skipped(
            "one_failed_listing_silences_every_runner_and_each_is_still_answered",
            AListingThatFails::WHY_IT_WOULD_BE_SKIPPED,
        );
        return;
    };
    broken.but_a_record_still_opens(&first_job);

    // Both machines come back, and the one broken directory is silent to both
    // of them at join. That is the fleet-wide half, measured rather than
    // argued: nothing about either collection, from one `readdir`.
    let mut first = Peer::runner(
        &harness.hub_url,
        "the-first-machine",
        &first_token,
        "the-life-that-swept-it-up",
    )
    .await;
    let mut second = Peer::runner(
        &harness.hub_url,
        "the-second-machine",
        &second_token,
        "the-life-that-swept-it-up",
    )
    .await;
    for (peer, job_id) in [(&mut first, &first_job), (&mut second, &second_job)] {
        let at_join = peer.through_a_health_report().await;
        assert!(
            files_held_for(&at_join, job_id).is_empty(),
            "one failed listing is silent to every runner: {at_join:?}"
        );
    }

    // And each is answered about its **own** collection, from its own record.
    for (peer, job_id, attempt, bytes) in [
        (&mut first, &first_job, first_attempt, &first_bytes),
        (&mut second, &second_job, second_attempt, &second_bytes),
    ] {
        peer.offer(job_id, attempt, 0, bytes[..HALF].to_vec()).await;
        peer.until(
            is_files_held_for(job_id),
            "answered this runner's own offer while its listing was broken",
        )
        .await;
        peer.offer(job_id, attempt, HALF as u64, bytes[HALF..].to_vec())
            .await;
        peer.until(
            is_files_ack_for(job_id),
            "acknowledged this runner's collection once it was whole",
        )
        .await;
    }
}

/// **The other direction, which is the harder half of "do not lie".** A hub
/// that cannot enumerate must not say it holds nothing — and it must not say
/// it is owed something either. A collection it already holds in full is not
/// claimed on the offer: the answer comes from the record, so `None` here is
/// a read fact and not a guess, and the re-offer is acknowledged on the
/// ordinary path, which is the one thing that removes a spool.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_this_hub_already_holds_is_not_claimed_as_owed() {
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
    runner.offer(&job_id, attempt, 0, bytes.clone()).await;
    runner
        .until(
            is_files_ack_for(&job_id),
            "acknowledged a collection it had in full",
        )
        .await;
    after_the_runner_is_gone(&harness, runner).await;

    let Some(broken) = AListingThatFails::over(harness.state_dir()) else {
        herd_runner::testing::skipped(
            "a_collection_this_hub_already_holds_is_not_claimed_as_owed",
            AListingThatFails::WHY_IT_WOULD_BE_SKIPPED,
        );
        return;
    };
    broken.but_a_record_still_opens(&job_id);

    let mut restarted = Peer::runner(
        &harness.hub_url,
        "a-runner-whose-files-landed",
        &token,
        "the-life-that-swept-it-up",
    )
    .await;
    let _ = restarted.through_a_health_report().await;
    restarted.offer(&job_id, attempt, 0, bytes).await;
    let seen = restarted
        .until(
            is_files_ack_for(&job_id),
            "acknowledged a collection it holds in full when it was offered again",
        )
        .await;
    assert!(
        files_held_for(&seen, &job_id).is_empty(),
        "a collection this hub holds in full is not one it is owed, and a broken listing is \
         no reason to say otherwise: {seen:?}"
    );

    // The record is unharmed by the whole exercise: one file, at its length.
    let record = support::record_of(harness.state_dir(), &job_id);
    assert_eq!(
        record["outputs"]["files"]
            .as_array()
            .map(|files| files.len()),
        Some(1),
        "{record}"
    );
}
