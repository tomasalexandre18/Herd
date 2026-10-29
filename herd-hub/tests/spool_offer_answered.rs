//! **The sentence that would free a swept spool, and the runner that is no
//! longer listening when it arrives** (ORCH-092's sequel).
//!
//! The premise everybody worked from was that a hub holding a collection in
//! full has *no way to say so*: the acknowledgement happened once, on a
//! connection that no longer exists, so a restarted runner can only read the
//! hub's silence. That is what
//! `herd-runner`'s `the_hub_has_said_what_it_is_owed` is built on, and it is
//! wrong. The hub **does** say it, every time, in answer to the very offer
//! the startup sweep makes:
//!
//! * `herd-hub/src/store.rs`, `write_output_chunk`'s *already here and whole*
//!   arm neither writes nor refuses a chunk of a file it already holds — it
//!   answers with the state of the collection, which is `Complete`;
//! * `herd-hub/src/server.rs` sends `FromHub::JobFilesAck` on `Complete` and
//!   prints `has every file of its collection`;
//! * `herd-runner`'s `files_acknowledged` is the one path that removes a
//!   spool, and D19 authorises exactly that.
//!
//! The runner condemns the spool before that answer can arrive. The barrier
//! treats the first `HealthRecorded` as proof the hub has said everything,
//! which is sound for the unprompted `JobFilesHeld` list — sent before the
//! hub reads a frame — and is **not** proof about the answer to an offer,
//! because the offer leaves after the health report: `offer_swept_collections`
//! only spawns the pump tasks, `report_health` queues `ToHub::Health` on the
//! interval's immediate first tick, and each pump sets `probed` under the lock
//! *before* its disk read. `heard` is set only by `JobFilesHeld`, which is the
//! one frame the hub by construction does not send for a collection it holds
//! whole. So `swept && probed && !heard` are all true the instant
//! `HealthRecorded` lands, the spool is renamed `.unclaimed`, and the loss is
//! then sealed in silence: `files_acknowledged` on an entry the barrier has
//! already removed is a no-op that prints nothing.
//!
//! **Why this file exists rather than a fix.** Both halves of this already
//! have a passing test and neither test runs the startup sequence — D211's
//! lesson, for the third time. `spools_resumed.rs` asserts the hub's half
//! against a hand-driven peer and is green today; `herd-runner`'s
//! `a_swept_spool_no_hub_asks_for_is_set_aside_and_never_offered_again` calls
//! the barrier directly and faithfully asserts the outcome that cost the first
//! consumer 37.6 GiB. Nothing anywhere ran a **real runner's start-up** against
//! a **real hub that holds a collection whole**. That gap is the defect's
//! hiding place, and this is the test that stands in it.
//!
//! Real binaries on both sides, for one reason beyond fidelity: their output
//! is captured, so the test can read what each of them *said*. The failure
//! this file produces therefore carries the three measurements that decide
//! how narrow the fix is — how many times the hub acknowledged the re-offer,
//! whether the runner's pump ever complained that its files had gone, and the
//! runner's own sentence about setting the spool aside.
//!
//! ## What the consumer's logs do and do not show
//!
//! Measured on the first consumer's 0.4.1 container, started 2026-10-29
//! ~19:23Z: **244** `has every file of its collection` lines over **75**
//! distinct job ids — the 75 the sweep offered — three times for 56 of them
//! and four times for 19, which is once per reconnection of the runner. The
//! hub said the sentence between three and four times per job and not one byte
//! was freed. Repetition cannot save it: the spool is `.unclaimed` after the
//! first reconnection and is never offered again, so there is no later offer
//! for a later acknowledgement to answer. The 37.6 GiB agreed on both sides,
//! and the hub held every byte of it whole — nothing was ever at risk but the
//! runner's ability to let go of its copy.
//!
//! What their logs do **not** show is any `cannot send … No such file or
//! directory`: zero, across the sweep. The prediction was that `set_aside`
//! renames the directory out from under a pump still reading it, so each
//! `.unclaimed` should have an ENOENT beside it. **This test reproduces that
//! absence too** — five runs on this workstation, 4 to 9 collections condemned
//! each time, and not one `cannot send` in the runner's own output — and the
//! reason is in the barrier rather than in a second swallowed error. The map
//! entry is removed **inside** the lock; the rename happens after it, in the
//! loop that follows. The pump's one and only read site sits behind
//! `inner.collections.get_mut(&job_id)` under that same lock, so a pump that
//! wakes after the barrier finds no entry and returns — never reaching
//! `read_at`, never touching the renamed path. There is no instant in which
//! the entry exists and the directory is gone. So the pump leaves through its
//! ordinary "nothing more to send" door, and the silence is the barrier's,
//! not the disk's.
//!
//! ## What decides it, which the first fixture here got wrong
//!
//! The first version of this test swept **one** collection and was green three
//! runs running: every chunk acknowledged, the spool deleted, the barrier
//! arriving to find nothing to condemn. That is a true measurement of the
//! wrong state. A pump sets `probed` only once it holds a permit from the
//! link's window (`herd_runner::FILES_IN_FLIGHT`, eight), so a lone pump takes
//! its permit, queues every chunk and is answered before `report_health` has
//! even been spawned. The consumer swept **75** against that window of eight:
//! the pumps that wait take their permits back *after* `ToHub::Health` is on
//! the queue, and each then sets `probed` and starts a disk read inside the
//! window the first `HealthRecorded` closes. More collections than the window
//! is therefore the condition, and it is why this file sweeps [`SWEPT`] of
//! them and not one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod real_herd;

use real_herd::{Herd, PATIENCE};

/// The name the record will carry, and the name the restarted runner dials
/// under: the hub refuses files offered by a machine its record does not name
/// (`files_may_land`), so a test that changed it would be measuring that
/// refusal instead.
const RUNNER: &str = "a-runner-that-comes-back";

/// What the hub prints when a chunk completes a collection it holds — the
/// sentence this whole file is about.
const ACKNOWLEDGED: &str = "has every file of its collection";

/// **More than one chunk's worth, and the difference is the measurement.**
///
/// A collection the hub already holds is acknowledged on **every** chunk of a
/// re-offer: the *already here and whole* arm answers with the state of the
/// whole collection without writing anything, and that state is `Complete`
/// from the first chunk on. So a collection that fits in one chunk would make
/// "the pump sent everything" and "the pump sent one chunk and stopped"
/// indistinguishable — both are one chunk and one acknowledgement. Two files
/// of [`EACH`] are [`chunks_in_the_collection`] chunks, and the count the hub
/// prints says which of three things happened: none of them (the pump never
/// read at all), one (it read, sent, and found its entry gone), or all of them
/// (it finished, and the acknowledgement raced the barrier and won).
const PATHS: [&str; 2] = ["evidence/results.json", "evidence/beside-it.bin"];

/// How long each file of the collection is: **one chunk and a little more**.
///
/// Not a knob, and the first version of this test got it wrong. A collection
/// of 54-byte files is not the collection this defect was measured on: the
/// first consumer's are around 500 MiB, so every read the pump makes is a
/// whole `herd_proto::OUTPUT_CHUNK_BYTES` off the disk. The window the barrier
/// can fire in is exactly that read — `probed` is set under the lock, before
/// it — so a fixture whose read returns in no time is a fixture that measures
/// the one case the race cannot be lost in. Just over a chunk is the smallest
/// collection that makes the runner do what it does in production: a full
/// chunk read, and more than one of them per file.
const EACH: usize = (1 << 20) + 7;

/// The collection, path by path, with bytes that differ between the files so
/// that each carries a digest of its own.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    PATHS
        .iter()
        .enumerate()
        .map(|(which, path)| (*path, vec![b'a' + which as u8; EACH]))
        .collect()
}

/// How many chunks a complete offer of the collection is, which is the ceiling
/// the acknowledgement count below is read against.
fn chunks_in_the_collection() -> usize {
    PATHS.len() * EACH.div_ceil(herd_proto::OUTPUT_CHUNK_BYTES as usize)
}

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The life of the runner that *ran* the job, driven frame by frame.
///
/// Hand-driven only for the first life, and only to put the hub in the state
/// this test is about — a record naming a streamed collection, and every byte
/// of it whole in `files/`. The life that matters is the second one, and that
/// one is the real `herd-runner` binary going through its own start-up: a
/// stand-in for the start-up path is precisely how this shipped, twice.
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

    /// Wait for one frame of a shape, inside the suite's patience.
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
    /// Not decoration. The twin rule lets a second process under one name in
    /// only when the incumbent's link has gone quiet, and a runner that never
    /// reported cannot be judged quiet (`reports_seen > 0`). The ordinary path
    /// is that dropping this peer closes the socket and the name is free at
    /// once; this is the fallback, so that a slow unregister costs the test a
    /// moment instead of the whole of `PATIENCE`.
    async fn through_a_health_report(&mut self) {
        self.say(ToHub::Health {
            jobs: Vec::new(),
            draining: false,
            load_milli: None,
            switch: herd_proto::Switch::On,
        })
        .await;
        self.until("it had recorded a health report", |frame| {
            matches!(frame, FromHub::HealthRecorded {})
        })
        .await;
    }
}

/// Run one job to a verdict announcing a streamed collection, hand the hub
/// every byte of it, and leave with the attempt that produced it.
async fn a_collection_the_hub_holds_whole(herd: &Herd, peer: &mut Peer) -> (JobId, u32) {
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
    // And the bytes, which is what makes this the state the test is about: a
    // hub that holds the collection **whole**. A chunk at a time, in the
    // runner's own `OUTPUT_CHUNK_BYTES`, because the hub checks every offset
    // against what it already holds and will not splice two reads of one path.
    let chunk = herd_proto::OUTPUT_CHUNK_BYTES as usize;
    for (path, bytes) in files() {
        for (which, slice) in bytes.chunks(chunk).enumerate() {
            peer.say(ToHub::JobFileChunk {
                job_id: job_id.clone(),
                attempt,
                path: path.to_owned(),
                offset: (which * chunk) as u64,
                data: slice.to_vec(),
            })
            .await;
        }
    }
    peer.until(
        "it had acknowledged the collection",
        |frame| matches!(frame, FromHub::JobFilesAck { job_id: of, .. } if *of == job_id),
    )
    .await;
    peer.through_a_health_report().await;
    (job_id, attempt)
}

/// Put the spool back on the runner's disk, exactly where the runner's own
/// start-up sweep looks for it.
///
/// **A fixture for the disk, never a stand-in for the start-up path.** What is
/// under test begins after this: the real `herd-runner` reads this directory
/// itself, with its own `sweep::pick_up`, its own
/// `settle_against_state_root`, its own offer and its own health report, in
/// its own order. All this does is leave the bytes a restarted runner finds —
/// `{data root}/outputs/{job}-{attempt}/…`, the name
/// `herd_runner::outputs::spool_path` writes and the sweep parses — because
/// the ordinary run of the ordinary path has already, correctly, deleted them.
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
/// consumer does. The one in the runner's own unit tests is the authority on
/// the constant; what is asserted here is that **no** such directory appears.
fn set_aside_as_unclaimed(root: &Path, job_id: &JobId, attempt: u32) -> PathBuf {
    root.join("outputs")
        .join(format!("{job_id}-{attempt}.unclaimed"))
}

/// How many collections the sweep finds, and **why the number is not a knob**.
///
/// The first version of this test swept one collection and came out green,
/// three runs in a row, with every chunk of it acknowledged and the spool
/// deleted. That is a true result and it is not the first consumer's state:
/// they swept **75**, against a link whose window is
/// `herd_runner::FILES_IN_FLIGHT` — eight. The window is what decides the
/// race. A pump only sets `probed` when it has a permit, so with one
/// collection the single pump takes its permit, queues every chunk and is
/// acknowledged before `report_health` has so much as been spawned: the
/// barrier arrives to find nothing left to condemn. With more pumps than
/// permits, the ones that are still waiting take their permits back *after*
/// `ToHub::Health` has been queued, and each of them then sets `probed` and
/// starts a disk read inside the window the first `HealthRecorded` closes.
///
/// So the condition is structural and measured: **more collections than the
/// link's window**. Four over it is the smallest fixture that has the shape;
/// 75 against 8 is what it looked like on the consumer's runner.
const SWEPT: usize = herd_runner::FILES_IN_FLIGHT + 4;

/// **A real runner's start-up against a real hub that holds its collections
/// whole: every spool must go.**
///
/// RED on `main`, and that is the point of it. The hub answers the sweep's
/// offer with `JobFilesAck` — the sentence D19 says a spool may go on — and
/// the runner has condemned the spool before the answer can arrive, so the
/// bytes are renamed `.unclaimed` and the acknowledgement lands on an entry
/// that is no longer there. 37.6 GiB on the first consumer's runner, with the
/// hub holding every byte of it.
///
/// It was `#[ignore]`d with its reason while the defect stood, so the suite
/// printed it as a skip and never as a pass (D27). **The fix removed the
/// attribute, and from here this is the regression test for the rule it
/// established**: a swept collection may be set aside only on a silence the
/// hub has already had the chance to break — its whole offer on the link, and
/// a health report queued after that offer's last chunk answered — so a hub
/// that holds the collection whole now always gets to say so before the
/// runner can condemn it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_swept_spool_the_hub_acknowledges_is_deleted_and_none_set_aside() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let token = herd.approve(RUNNER);

    // The life that ran the jobs, and handed over every byte of every one.
    let mut peer = Peer::runner(&herd.url(), RUNNER, &token, "the-life-that-ran-it").await;
    let mut collections = Vec::new();
    for _ in 0..SWEPT {
        collections.push(a_collection_the_hub_holds_whole(&herd, &mut peer).await);
    }
    // The machine goes away. In the first consumer's case it was a container
    // restart; here it is the socket closing, which is the same thing to a
    // hub and to a spool.
    drop(peer);

    // **The premise, measured and not assumed.** The hub holds every
    // collection whole on its own state root: this test is about a runner that
    // cannot let go of a copy, never about bytes at risk.
    for (job_id, _) in &collections {
        let kept = herd.job_dir(job_id.as_str()).join("files");
        for (path, bytes) in files() {
            let whole = kept.join(path);
            assert_eq!(
                std::fs::read(&whole).ok(),
                Some(bytes),
                "the hub must hold {} whole before this test means anything",
                whole.display()
            );
        }
    }
    // **Once each**, and the asymmetry with the re-offer below is the
    // measurement this test turns on. A collection *arriving* completes on its
    // last chunk: the chunks before it leave the collection `More`, so there
    // is one acknowledgement however many files it has. A collection
    // *re-offered* is already whole, so **every** chunk of it takes the
    // `already here and whole` arm and is answered `Complete` — one
    // acknowledgement each. So after the restart, the number of these lines is
    // the number of chunks that reached the hub, which is what says how far
    // the pumps got.
    let acknowledged_before = herd.said("hub").matches(ACKNOWLEDGED).count();
    assert_eq!(
        acknowledged_before,
        SWEPT,
        "each collection is acknowledged once as it arrives, on the chunk that completes \
         it: {}",
        herd.said("hub")
    );

    // The disk a restarted runner wakes up on: the spools, and a data root
    // that belongs to this hub's state — without the welcome, every collection
    // is set aside as another root's and the test would be measuring that
    // rule instead (`settle_against_state_root`).
    herd.pretend_the_root_was_welcomed("runner");
    let root = herd.runner_root("runner");
    let swept: Vec<(JobId, PathBuf, PathBuf)> = collections
        .iter()
        .map(|(job_id, attempt)| {
            (
                job_id.clone(),
                a_spool_on_the_runner_s_disk(&root, job_id, *attempt),
                set_aside_as_unclaimed(&root, job_id, *attempt),
            )
        })
        .collect();

    // And the real binary, through its own start-up: sweep, dial, offer,
    // report health, read the answers.
    let _runner = herd.start_runner_called(RUNNER, "runner");
    herd.wait_until("the restarted runner has joined the hub", || {
        herd.said("hub").matches("has joined").count() >= 2
    })
    .await;
    herd.wait_until("the runner has dealt with every spool it swept", || {
        swept
            .iter()
            .all(|(_, spool, aside)| !spool.exists() || aside.exists())
    })
    .await;
    // The rename and the acknowledgement race each other on the same link, so
    // a verdict read the instant the last directory moved could miss a frame
    // that was already in flight. One health cadence of these tests (0.2 s) is
    // many round trips on loopback.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // **The hub never asked for any of them**, which is the other half of the
    // premise: a collection it holds whole is not owed, so the unprompted list
    // that the barrier is sound for says nothing about these.
    let hub_said = herd.said("hub");
    for (job_id, _, _) in &swept {
        assert!(
            !hub_said.contains(&format!("still owes this hub files of {job_id}")),
            "a collection the hub holds whole is never asked for again; if it were, this \
             test would be measuring the resumption path and not the offer: {hub_said}"
        );
    }

    // The measurements the fix's shape depends on. Counted here so that the
    // failure below carries them, rather than needing a second run.
    let acknowledged_for_the_offer = hub_said.matches(ACKNOWLEDGED).count() - acknowledged_before;
    let runner_said = herd.said("runner");
    let could_not_send = runner_said.matches("cannot send").count();
    let barrier_lines = runner_said
        .lines()
        .filter(|line| line.contains("neither asked for it nor acknowledged it"))
        .count();
    let set_aside: Vec<String> = swept
        .iter()
        .filter(|(_, _, aside)| aside.exists())
        .map(|(_, _, aside)| aside.display().to_string())
        .collect();
    let still_held: Vec<String> = swept
        .iter()
        .filter(|(_, spool, _)| spool.exists())
        .map(|(_, spool, _)| spool.display().to_string())
        .collect();

    // Said whichever way the assertion goes, because the figures are the
    // finding and not only the diagnosis: a green run on one host is evidence
    // about the race on that host, and the counts are what make it readable.
    println!(
        "spool-offer-repro: {SWEPT} collection(s) swept, {} set aside, {} still spooled, \
         {acknowledged_for_the_offer} acknowledgement(s) of the re-offers (up to {} \
         chunks each), {could_not_send} `cannot send` line(s), {barrier_lines} barrier \
         line(s)",
        set_aside.len(),
        still_held.len(),
        chunks_in_the_collection(),
    );

    assert!(
        set_aside.is_empty() && still_held.is_empty(),
        "SWEPT SPOOLS THE HUB ACKNOWLEDGED ARE STILL ON THE DISK: {} of {SWEPT} set \
         aside, {} of {SWEPT} still spooled.\n\
         \n\
         The hub answered the sweep's offers {acknowledged_for_the_offer} time(s) with \
         `{ACKNOWLEDGED}` — the sentence D19 says a spool may go on — and the runner had \
         already condemned them. Set aside: {:#?}\nStill spooled: {:#?}\n\
         \n\
         What the pumps did, which decides how narrow the fix is. A re-offered collection \
         is already whole, so the hub answers EVERY chunk of it `Complete`: that count is \
         the number of chunks that reached the hub, against up to {} per collection. \
         {could_not_send} `cannot send` line(s) in the runner's own output, and \
         {barrier_lines} collection(s) condemned by the barrier in its own words. A \
         condemned collection with no `cannot send` beside it is a pump that stopped at \
         its map lookup — the barrier removes the entry under the lock before it renames \
         the directory — and not a swallowed disk error.\n\
         \n\
         --- the hub said:\n{hub_said}\n--- the runner said:\n{runner_said}",
        set_aside.len(),
        still_held.len(),
        set_aside,
        still_held,
        chunks_in_the_collection(),
    );
}
