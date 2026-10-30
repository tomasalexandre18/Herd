//! **The real runner's barrier against a real hub whose `readdir` fails**
//! (ORCH-197 / D225, and D221's shape for the third time).
//!
//! D225 fixed the hub: a hub that cannot list its jobs claims nothing about
//! any collection and answers each one from that job's own record the moment
//! a runner offers it. The claim the fix rests on is about an **ordering**
//! between two frames on one link — that the `JobFilesHeld` answering an
//! offer arrives ahead of the `HealthRecorded` the runner's barrier reads
//! silence against — and until this file was written that ordering was
//! asserted only by `owed_scan_fails.rs`, which drives the hub as a library
//! object and the runner as a hand-written `Peer` that encodes its own
//! frames.
//!
//! ## Why a hand-driven peer could not have caught this, and must not be
//! ## "simplified" back to one
//!
//! A hand-written peer sends one chunk, then reads frames in the order it
//! chose to read them. It has no pump, no window, no health interval and no
//! barrier: the sequence it asserts is the sequence the test author typed.
//! **It cannot get the ordering wrong**, so its green says nothing about the
//! code that can.
//!
//! The code that can is `herd_runner`'s
//! `Session::the_hub_has_said_what_it_is_owed`, and it is the exact code that
//! lost the first consumer 37.6 GiB. It judges a swept collection on three
//! facts kept per connection — `swept`, `heard` and `offered_at` — set by a
//! pump that only sets `probed` once it holds a permit from the link's window
//! (`herd_runner::FILES_IN_FLIGHT`, eight), by a health interval that fires
//! on its own immediate first tick, and by whichever frames the hub happens
//! to have sent by then. None of those three exist in a hand-driven peer.
//! Before this file, the runner's barrier had **never run** against the one
//! hub state the D225 fix exists for.
//!
//! So what is composed here is the fixture from `owed_scan_fails.rs` — a real
//! `readdir` made to fail, with the *"but a record still opens"* half
//! asserted — and the harness and the scale from `spool_offer_answered.rs`:
//! both binaries as processes, the runner going through its own start-up, and
//! more collections than the link's window.
//!
//! ## The scale is not a knob
//!
//! `spool_offer_answered.rs` measured this: with **one** collection the lone
//! pump takes its permit, queues every chunk and is answered before
//! `report_health` has so much as been spawned, so the barrier arrives to
//! find nothing to judge. That fixture was green three runs running and was
//! measuring the wrong state. The condition is structural — **more
//! collections than the link's window** — which is why [`SWEPT`] is
//! `FILES_IN_FLIGHT + 4` and why anything at or below eight proves nothing.
//! The first consumer swept 75 against the same window of eight.
//!
//! ## What is asserted
//!
//! 1. the fixture bit: the hub says in its own words that it cannot tell
//!    which collections are outstanding;
//! 2. the silence at join is real — the hub never sends the unprompted
//!    *"still owes this hub files of …"* line for any of these jobs, because
//!    it could not derive the list;
//! 3. every collection is nonetheless answered **on its offer**, from that
//!    job's own record;
//! 4. and therefore **no** spool is condemned: nothing is renamed
//!    `.unclaimed`, no spool is left behind, the runner's barrier never says
//!    its sentence, and the hub ends up holding every byte of every
//!    collection whole.
//!
//! Assertion 4 is the one that would have failed on the defect this is about,
//! and assertions 1 to 3 are what make a green reading of 4 mean something:
//! without them, a hub whose listing quietly worked would pass this file by
//! taking the ordinary resumption path instead.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod a_failed_listing;
mod real_herd;

use a_failed_listing::AListingThatFails;
use real_herd::{Herd, PATIENCE};

/// The name the records will carry, and the name the restarted runner dials
/// under: the hub refuses files offered by a machine its record does not name
/// (`files_may_land`), so a test that changed it would be measuring that
/// refusal instead.
const RUNNER: &str = "a-runner-whose-hub-lost-its-listing";

/// The two files of each collection.
const PATHS: [&str; 2] = ["evidence/results.json", "evidence/beside-it.bin"];

/// How long each file is: **one chunk and a little more**, for
/// `spool_offer_answered.rs`'s reason. The window the barrier can fire in is
/// the pump's disk read — `probed` is set under the lock, before it — so a
/// fixture whose read returns in no time is a fixture that measures the one
/// case the race cannot be lost in.
const EACH: usize = (1 << 20) + 7;

/// How many collections the hub is owed, and **why the number is not a
/// knob**: see the module header. Four over the link's window is the smallest
/// fixture that has the shape.
const SWEPT: usize = herd_runner::FILES_IN_FLIGHT + 4;

/// What the hub prints when its own listing fails — the fixture's own
/// receipt, so a run that silently had a working `readdir` cannot read as a
/// pass.
const CANNOT_TELL: &str = "cannot tell which collections are outstanding";

/// What the hub prints when it answers an offer from that job's record, which
/// is the D225 path this file exercises.
const ANSWERED_FROM_THE_RECORD: &str =
    "that job's own record answered what the scan of all of them could not";

/// What the runner prints when its barrier condemns a collection — the
/// sentence that must not appear.
const CONDEMNED: &str = "neither asked for it nor acknowledged it";

/// The collection, path by path, with bytes that differ between the files so
/// that each carries a digest of its own.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    PATHS
        .iter()
        .enumerate()
        .map(|(which, path)| (*path, vec![b'a' + which as u8; EACH]))
        .collect()
}

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The life of the runner that *ran* the jobs, driven frame by frame.
///
/// Hand-driven only for the first life, and only to put the hub in the state
/// this test is about: a record naming a streamed collection, and **not one
/// byte of it** in `files/`. The life that matters is the second one, and
/// that one is the real `herd-runner` binary going through its own start-up.
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
            // Numbered by the pack half and left absent here: this stand-in
            // runner serves nothing to a peer, and absent is what
            // outbound-only looks like on the wire (invariant 3, D11).
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

    async fn until(&mut self, what: &str, mut wanted: impl FnMut(&FromHub) -> bool) -> FromHub {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .unwrap_or_else(|_| panic!("the hub never said {what} inside {PATIENCE:?}"))
                .unwrap_or_else(|| panic!("the link went away before the hub said {what}"));
            if wanted(&frame) {
                return frame;
            }
        }
    }

    /// One health report, so the hub has seen a report from *this* instance.
    ///
    /// Not decoration: the twin rule lets a second process under one name in
    /// only when the incumbent's link has gone quiet, and a runner that never
    /// reported cannot be judged quiet (`reports_seen > 0`).
    async fn through_a_health_report(&mut self) {
        self.say(ToHub::Health {
            jobs: Vec::new(),
            draining: false,
            load_milli: None,
            switch: herd_proto::Switch::On,
            // This stand-in holds no pack store, so it reports no figure
            // about one rather than a zero that would read as an empty store.
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        })
        .await;
        self.until("it had recorded a health report", |frame| {
            matches!(frame, FromHub::HealthRecorded {})
        })
        .await;
    }
}

/// Run one job to a verdict that announces a streamed collection, and leave
/// with the attempt that produced it. **Not a byte of it follows**, which is
/// what makes the hub *owed* this collection rather than a holder of it.
async fn a_collection_the_hub_is_owed(herd: &Herd, peer: &mut Peer) -> (JobId, u32) {
    let job_id = JobId::new(herd.submit("true").await);
    let assigned = peer
        .until("it had assigned the job", |frame| {
            matches!(frame, FromHub::AssignJob { .. })
        })
        .await;
    let FromHub::AssignJob { attempt, .. } = assigned else {
        unreachable!("matched above");
    };
    peer.say(ToHub::JobFinished {
        job_id: job_id.clone(),
        attempt,
        outcome: herd_proto::JobOutcome::Exited { code: 0 },
        chunks_sent: 0,
        metrics: herd_proto::JobMetrics::default(),
        steps: Vec::new(),
        outputs: herd_proto::JobOutputs {
            files: files()
                .iter()
                .map(|(path, bytes)| {
                    herd_proto::OutputFile::announced(
                        (*path).to_owned(),
                        bytes.len() as u64,
                        herd_proto::sha256_hex(bytes),
                    )
                })
                .collect(),
            refused: None,
            skipped: Vec::new(),
            streamed: true,
        },
    })
    .await;
    peer.until("it had acknowledged the result", |frame| {
        matches!(frame, FromHub::JobResultAck { .. })
    })
    .await;
    (job_id, attempt)
}

/// Put the spool on the runner's disk, exactly where the runner's own
/// start-up sweep looks for it.
///
/// **A fixture for the disk, never a stand-in for the start-up path.** What
/// is under test begins after this: the real `herd-runner` reads this
/// directory itself, with its own `sweep::pick_up`, its own
/// `settle_against_state_root`, its own offer and its own health report, in
/// its own order.
fn a_spool_on_the_runner_s_disk(root: &Path, job_id: &JobId, attempt: u32) -> PathBuf {
    let spool = root.join("outputs").join(format!("{job_id}-{attempt}"));
    for (path, bytes) in files() {
        let at = spool.join(path);
        std::fs::create_dir_all(at.parent().expect("every file is under the spool"))
            .expect("the fixture makes the spool");
        std::fs::write(&at, &bytes).expect("the fixture writes the spooled bytes");
    }
    spool
}

/// The name a spool nothing will ever acknowledge is renamed to.
///
/// Spelled out rather than taken from `herd_runner::sweep::UNCLAIMED`, which
/// is `pub(crate)`: this is an integration test and sees the crate as a
/// consumer does. What is asserted here is that **no** such directory
/// appears.
fn set_aside_as_unclaimed(root: &Path, job_id: &JobId, attempt: u32) -> PathBuf {
    root.join("outputs")
        .join(format!("{job_id}-{attempt}.unclaimed"))
}

/// **The runner's half of D225, against the hub state the fix exists for.**
///
/// A hub that is owed twelve collections and cannot list its own jobs says
/// nothing about any of them at join. Every one of them is therefore resumed
/// on the strength of the answer to its *offer* alone — the one frame
/// ordering the fix depends on — and the real runner's barrier must not
/// condemn a single spool while that is happening.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_runner_condemns_nothing_when_the_hub_cannot_list_its_jobs() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let token = herd.approve(RUNNER);

    // The life that ran the jobs, and handed over not one byte of any of
    // them.
    let mut peer = Peer::runner(&herd.url(), RUNNER, &token, "the-life-that-ran-it").await;
    let mut collections = Vec::new();
    for _ in 0..SWEPT {
        collections.push(a_collection_the_hub_is_owed(&herd, &mut peer).await);
    }
    peer.through_a_health_report().await;
    // The machine goes away. In the first consumer's case it was a container
    // restart; here it is the socket closing, which is the same thing to a
    // hub and to a spool.
    drop(peer);

    // **The premise, measured and not assumed**: the hub holds nothing of any
    // of these collections, so each one is genuinely owed and the join list
    // would have named every one of them.
    for (job_id, _) in &collections {
        let kept = herd.job_dir(job_id.as_str()).join("files");
        for (path, _) in files() {
            assert!(
                !kept.join(path).exists(),
                "the hub must be owed {} before this test means anything",
                kept.join(path).display()
            );
        }
    }

    // The disk a restarted runner wakes up on: the spools, and a data root
    // that belongs to this hub's state — without that, every collection is
    // set aside as another root's and the test would be measuring
    // `settle_against_state_root` instead.
    herd.pretend_the_root_was_welcomed("runner");
    let root = herd.runner_root("runner");
    let swept: Vec<(JobId, u32, PathBuf, PathBuf)> = collections
        .iter()
        .map(|(job_id, attempt)| {
            (
                job_id.clone(),
                *attempt,
                a_spool_on_the_runner_s_disk(&root, job_id, *attempt),
                set_aside_as_unclaimed(&root, job_id, *attempt),
            )
        })
        .collect();

    // And the hub's listing, broken after the last submission and before the
    // runner that matters joins. Submitting a job needs a `mkdir` in this
    // directory; everything from here on needs only to traverse it.
    let Some(broken) = AListingThatFails::over(herd.state.path()) else {
        herd_runner::testing::skipped(
            "a_real_runner_condemns_nothing_when_the_hub_cannot_list_its_jobs",
            AListingThatFails::WHY_IT_WOULD_BE_SKIPPED,
        );
        return;
    };
    broken.but_a_record_still_opens(&swept[0].0);

    // The real binary, through its own start-up: sweep, dial, offer, report
    // health, read the answers, judge.
    let _runner = herd.start_runner_called(RUNNER, "runner");
    herd.wait_until("the restarted runner has joined the hub", || {
        herd.said("hub").matches("has joined").count() >= 2
    })
    .await;
    herd.wait_until("the runner has dealt with every spool it swept", || {
        swept
            .iter()
            .all(|(_, _, spool, aside)| !spool.exists() || aside.exists())
    })
    .await;
    // The rename and the acknowledgement race each other on the same link, so
    // a verdict read the instant the last directory moved could miss a frame
    // that was already in flight. One health cadence of these tests (0.2 s) is
    // many round trips on loopback.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let hub_said = herd.said("hub");
    let runner_said = herd.said("runner");

    // 1. **The fixture bit.** Without this line the hub's listing worked, the
    //    ordinary resumption path ran, and a green reading below would be
    //    about nothing.
    assert!(
        hub_said.contains(CANNOT_TELL),
        "this test is about a hub whose own listing fails, and that hub never said so — so \
         nothing here is evidence about the fix.\n--- the hub said:\n{hub_said}"
    );

    // 2. **The silence at join is real.** The unprompted list is the thing the
    //    barrier is sound for; a hub that could derive it would resume these
    //    collections the old way and this file would be measuring that.
    for (job_id, _, _, _) in &swept {
        assert!(
            !hub_said.contains(&format!("still owes this hub files of {job_id}")),
            "a hub that cannot enumerate must claim nothing at join, so the only thing that \
             can save {job_id} is the answer to its own offer: {hub_said}"
        );
    }

    // 3. **And every collection is answered on its offer, from its own
    //    record** — once each, which is what D225 added.
    let answered_from_the_record = hub_said.matches(ANSWERED_FROM_THE_RECORD).count();
    assert_eq!(
        answered_from_the_record, SWEPT,
        "each of the {SWEPT} collections is answered once, from the one record its offer \
         names: {hub_said}"
    );

    // The measurements, counted before the assertion so that a failure
    // carries them rather than needing a second run.
    let condemned = runner_said.matches(CONDEMNED).count();
    let set_aside: Vec<String> = swept
        .iter()
        .filter(|(_, _, _, aside)| aside.exists())
        .map(|(_, _, _, aside)| aside.display().to_string())
        .collect();
    let still_held: Vec<String> = swept
        .iter()
        .filter(|(_, _, spool, _)| spool.exists())
        .map(|(_, _, spool, _)| spool.display().to_string())
        .collect();

    // Said whichever way the assertion goes, because the figures are the
    // finding and not only the diagnosis: a green run of a race on one host
    // is evidence about that host, and the counts are what make it readable.
    println!(
        "owed-scan-fails-real-runner: {SWEPT} collection(s) owed and swept, {} set aside, \
         {} still spooled, {answered_from_the_record} answered from their own records, \
         {condemned} condemned by the barrier",
        set_aside.len(),
        still_held.len(),
    );

    // 4. **Nothing condemned.** This is the assertion the defect would have
    //    failed, and the one the hand-driven peer could not make.
    assert!(
        set_aside.is_empty() && still_held.is_empty() && condemned == 0,
        "THE RUNNER CONDEMNED A COLLECTION THIS HUB IS OWED: {} of {SWEPT} set aside, {} of \
         {SWEPT} still spooled, {condemned} barrier line(s).\n\
         \n\
         The hub could not list its jobs, so it said nothing at join and answered \
         {answered_from_the_record} of the {SWEPT} offers from the records themselves. A \
         collection set aside here is one whose `JobFilesHeld` lost the race against the \
         `HealthRecorded` the barrier reads silence against — the ordering the whole D225 \
         fix rests on. Set aside: {:#?}\nStill spooled: {:#?}\n\
         \n\
         --- the hub said:\n{hub_said}\n--- the runner said:\n{runner_said}",
        set_aside.len(),
        still_held.len(),
        set_aside,
        still_held,
    );

    // And the bytes landed, which is what makes the fix worth having: every
    // collection is **recovered** on a hub whose listing is broken, rather
    // than renamed `.unclaimed` on a runner's disk.
    for (job_id, _, _, _) in &swept {
        let kept = herd.job_dir(job_id.as_str()).join("files");
        for (path, bytes) in files() {
            let whole = kept.join(path);
            assert_eq!(
                std::fs::read(&whole).ok(),
                Some(bytes),
                "a collection answered on its offer arrives in full: {} \
                 \n--- the hub said:\n{hub_said}",
                whole.display()
            );
        }
    }
}
