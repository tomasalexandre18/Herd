//! **The pack frames, through a real hub's connection loop** (M11, D231).
//!
//! `pack_registry.rs` drives the registry by calling it. This file does not
//! call it once: every assertion here is made about what a hub *answered on a
//! socket*, because the thing that was missing was never the registry — it
//! was the seam and the dispatch. Measured on `main` at a15b52d, before any
//! of this:
//!
//! * `JobDirStore::packs()` had no production caller anywhere, so the holdings
//!   map was empty in every running hub;
//! * `ToHub::PacksHeld` had no arm in `relay_from_runner`, so a runner that
//!   sent one was answered `FromHub::Refused` and **had its link closed**;
//! * `FromHub::ReadPacksAgain` was constructed nowhere, so nothing ever asked
//!   for the announcement D231 decision 5 is built on.
//!
//! So the tests are shaped around the two things that go wrong when a frame
//! family has no arrival window of its own, which is the defect D150 paid for
//! and the one the planner named before any of this was written.
//!
//! ## The two windows, and why neither of them is `is_current`
//!
//! A pack's life touches a job at **both ends**, and `is_current` is wrong at
//! each:
//!
//! * a **fetch** arrives while the job is `assigned` — the runner is laying
//!   `{in}` out and has not reported that the process started. `is_current`
//!   is not merely the wrong window there, it is *inconstructible*: it is
//!   answered out of `State::standing_of`, which takes an attempt number, and
//!   `ToHub::AskPack` carries none;
//! * a **registration** arrives *after the verdict*, because the runner
//!   fsyncs the pack out of `{out}` before the record walk (D232-D234).
//!   `is_current` refuses exactly that, by name, as `Standing::JobIsOver`.
//!
//! One job proves both, and that is why there is one fixture: the same job,
//! read before and after its `JobFinished`, is **inside** the fetch window and
//! **inside** the registration window at one end, and **outside** the fetch
//! window while still inside the registration window at the other. A test that
//! used two jobs could not show that the two windows are different windows.
//!
//! ## What every test here also asserts, without saying so twice
//!
//! That the link is still up. A pack nobody can agree about must not cost a
//! runner its connection (D13), so every refusal below is `PackRefused` and
//! never `FromHub::Refused`, and every test that provokes one goes on to send
//! a health report and read the answer. Before this branch, each of these
//! frames closed the link.

use std::fs;
use std::path::Path;

use futures_util::{SinkExt, StreamExt};
use herd_proto::pack::{PackId, PacksHeld};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const RUNNER: &str = "a-runner-that-holds-packs";
const COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";
const KEY: &str = "pack";

/// Where this runner says it serves content to its peers.
///
/// A private mesh address, which is the only kind invariant 3 permits a
/// runner to bind, and it is here because the hub **discarded** the
/// handshake's `mesh_endpoint` until this branch: every holding it could have
/// recorded would have come back with no endpoint, which is the wire's way of
/// saying "outbound-only" and would have been a false statement about this
/// machine.
const ENDPOINT: &str = "10.42.0.7:51820";

/// What the two `kind f` rows of [`a_manifest`] declare between them.
///
/// **The figure the zero is counted against.** A pack of 12288 declared bytes
/// is registered through a frame below, and the hub must then hold 12288 of
/// them minus 12288 — the manifest that names them, yes; not one byte of the
/// objects themselves. A zero with no figure beside it is not a measurement.
const DECLARED_OBJECT_BYTES: u64 = 4096 + 8192;

/// The fixture manifest, with the rows that make it prove anything.
///
/// Two `kind f` rows, one `d` and one `l`, exactly as `pack_registry.rs`'s
/// is: the `d` and `l` rows carry a sha256 of something that is not a file's
/// bytes, so a walk that does not filter on `kind == f` counts them as
/// objects.
fn a_manifest() -> Vec<u8> {
    let mut text = String::from("# chatons pack 1\n# profile debug\n");
    text.push_str(&format!(
        "{}\t4096\t0644\tf\tbin\tbin/tool\n",
        "1".repeat(64)
    ));
    text.push_str(&format!(
        "{}\t8192\t0644\tf\tlib\tlib/libthing.rlib\n",
        "4".repeat(64)
    ));
    text.push_str(&format!("{}\t0\t0755\td\tbin\tbin\n", "2".repeat(64)));
    text.push_str(&format!(
        "{}\t7\t0777\tl\tbin\tbin/tool-latest\tbin/tool\n",
        "3".repeat(64)
    ));
    text.into_bytes()
}

fn the_pack() -> PackId {
    PackId::of_manifest_bytes(&a_manifest())
}

/// A peer driven frame by frame, so a test can be a runner at one named
/// moment of its life rather than a runner that happens to be connected.
struct Peer {
    socket: Link,
}

impl Peer {
    async fn runner(hub_url: &str, name: &str, token: &str, instance: &str) -> Self {
        Self::dialling(hub_url, name, token, instance, Some(ENDPOINT.to_owned())).await
    }

    async fn dialling(
        hub_url: &str,
        name: &str,
        token: &str,
        instance: &str,
        mesh_endpoint: Option<String>,
    ) -> Self {
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
            mesh_endpoint,
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

    async fn health(&mut self) {
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
    }

    /// Every frame up to and including the answer to a health report.
    ///
    /// **The barrier, and in this file it is also the proof the link is up.**
    /// The hub answers every report with `HealthRecorded`, so a report that
    /// comes back is a link that is still carrying traffic rather than one
    /// that merely has not been closed yet. Every test that provokes a
    /// refusal ends here.
    async fn through_a_health_report(&mut self) -> Vec<FromHub> {
        self.health().await;
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .expect("the hub answers a health report inside the suite's patience")
                .expect("the link stays up, which is the whole of D13");
            let done = matches!(frame, FromHub::HealthRecorded {});
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }

    /// Announce a holding and read back the hub's answer to a health report,
    /// which is what makes the announcement's effect observable at a named
    /// moment rather than whenever the test next looks.
    async fn announces(&mut self, packs: Vec<PackId>, more: bool) {
        self.say(ToHub::PacksHeld(PacksHeld { packs, more })).await;
        if !more {
            self.through_a_health_report().await;
        }
    }
}

/// Hand this runner a job and keep it `assigned`: the attempt exists, it is
/// on this machine, and nothing has been reported about it.
///
/// That is the **fetch** window, and it is the state `is_current` cannot be
/// asked about, because the frame carries no attempt number.
async fn a_job_assigned_to(harness: &Harness, runner: &mut Peer) -> (JobId, u32) {
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
    (job_id, attempt)
}

/// End that job, which is what puts a **registration** inside its window and
/// a **fetch** outside its own.
async fn and_then_it_ends(runner: &mut Peer, job_id: &JobId, attempt: u32) {
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
    loop {
        match runner.next().await {
            Some(FromHub::JobResultAck { .. }) => break,
            Some(_) => continue,
            None => panic!("the hub never acknowledged the result"),
        }
    }
}

fn a_registration(job_id: &JobId, attempt: u32) -> ToHub {
    ToHub::RegisterPack(herd_proto::RegisterPack {
        job_id: job_id.clone(),
        attempt,
        commit: COMMIT.to_owned(),
        key: KEY.to_owned(),
        manifest: a_manifest(),
        meta: Some(b"commit cb5a2e8b\nprofile debug\n".to_vec()),
    })
}

fn an_ask(job_id: &JobId) -> ToHub {
    ToHub::AskPack(herd_proto::AskPack {
        job_id: job_id.clone(),
        commit: COMMIT.to_owned(),
        key: KEY.to_owned(),
    })
}

/// Read frames until one of them is a pack answer, so a test asserts about
/// the hub's answer and not about whatever the hub happened to say first.
async fn pack_answer(runner: &mut Peer) -> FromHub {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let frame = tokio::time::timeout_at(deadline, runner.next())
            .await
            .expect("the hub answers a pack frame inside the suite's patience")
            .expect("the link stays up: a pack refusal is not a reason to close it (D13)");
        if matches!(
            frame,
            FromHub::PackRegistered(_) | FromHub::PackRefused(_) | FromHub::PackHolders(_)
        ) {
            return frame;
        }
    }
}

fn refusal(frame: &FromHub) -> &str {
    match frame {
        FromHub::PackRefused(refused) => &refused.why,
        other => panic!("a pack refused outside its window is PackRefused, got {other:?}"),
    }
}

/// **An announcement replaces; `more: true` accumulates; and an empty one
/// means "I hold none".**
///
/// All three in one test on purpose, because they are one rule and the third
/// is the only way a holding is ever retracted: nothing else in the frame set
/// can say "I no longer hold that". A hub that read an empty announcement as
/// "no news" would keep naming a holder of a pack that machine deleted under
/// disk pressure, and the fetch would fail on a sentence this hub made up.
///
/// The assertions are made by **asking the hub**, through `AskPack`, so the
/// subject is the holder list the hub would really hand a job and not a
/// reading of its memory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_announcement_replaces_what_the_hub_held_and_an_empty_one_retracts_it() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;
    let (job_id, attempt) = a_job_assigned_to(&harness, &mut runner).await;

    // A pack to be a holder of. Registered through the frame, which is the
    // only thing in this build that registers anything (E-17).
    runner.say(a_registration(&job_id, attempt)).await;
    match pack_answer(&mut runner).await {
        FromHub::PackRegistered(done) => assert_eq!(done.pack, the_pack()),
        other => panic!("a post-fsync registration is accepted, got {other:?}"),
    }

    // Nobody has announced anything, so the pack is registered and held by
    // nobody — `PackState::NoHolder`, said and not inferred.
    runner.say(an_ask(&job_id)).await;
    let held_by_nobody = holders_of(&pack_answer(&mut runner).await);
    assert!(
        held_by_nobody.is_empty(),
        "a registered pack nobody announced has no holder: {held_by_nobody:?}"
    );

    // One frame of a two-frame announcement. **Nothing is applied yet**: a
    // hub that applied the first frame would replace its whole view with a
    // fragment of the truth, which is the defect `more` exists for.
    runner
        .announces(vec![the_pack(), a_second_pack()], true)
        .await;
    runner.say(an_ask(&job_id)).await;
    assert!(
        holders_of(&pack_answer(&mut runner).await).is_empty(),
        "an announcement with more to come is accumulated and not applied"
    );

    // And now it ends. The accumulated list replaces what the hub held.
    runner.announces(Vec::new(), false).await;
    runner.say(an_ask(&job_id)).await;
    let holders = holders_of(&pack_answer(&mut runner).await);
    assert_eq!(holders.len(), 1, "the runner holds it: {holders:?}");
    assert_eq!(holders[0].runner, RUNNER);
    assert_eq!(
        holders[0].endpoint.as_deref(),
        Some(ENDPOINT),
        "and with the address its handshake advertised, which the hub discarded until this \
         branch: a holder with no endpoint is the wire's way of saying outbound-only, and \
         that would be a false statement about this machine"
    );

    // **The retraction.** An empty announcement that ends is "I hold none",
    // not "nothing to say".
    runner.announces(Vec::new(), false).await;
    runner.say(an_ask(&job_id)).await;
    let after = holders_of(&pack_answer(&mut runner).await);
    assert!(
        after.is_empty(),
        "an empty announcement retracts every holding, which is the only way a holding is \
         ever retracted: {after:?}"
    );
}

/// A second id to announce beside the first, so that an announcement under
/// test carries more than one pack and the accumulation is about a list.
fn a_second_pack() -> PackId {
    PackId::of_manifest_bytes(b"# chatons pack 1\n# profile release\n")
}

fn holders_of(frame: &FromHub) -> Vec<herd_proto::pack::Holder> {
    match frame {
        FromHub::PackHolders(answer) => answer.holders.clone(),
        other => panic!("an ask about a registered key answers PackHolders, got {other:?}"),
    }
}

/// **A registration is accepted after the verdict, and that is the window.**
///
/// The runner fsyncs its pack out of `{out}` before the record walk
/// (D232-D234), so this frame routinely arrives about a job that is over.
/// `is_current` refuses exactly that as `Standing::JobIsOver`, which is right
/// for a log chunk and wrong here, and `a_pack_may_be_registered` is the
/// predicate that says so.
///
/// **And the case outside the window, in the same test:** a registration
/// naming an attempt that is not the one on the record. Refused by name, with
/// the number that is, and the link stays up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pack_is_registered_after_the_verdict_and_a_wrong_attempt_is_refused_by_name() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;
    let (job_id, attempt) = a_job_assigned_to(&harness, &mut runner).await;
    and_then_it_ends(&mut runner, &job_id, attempt).await;

    // Inside the window, on a job that is over.
    runner.say(a_registration(&job_id, attempt)).await;
    match pack_answer(&mut runner).await {
        FromHub::PackRegistered(done) => {
            assert_eq!(done.pack, the_pack(), "the id is the sha256 of the bytes");
            assert_eq!(done.commit, COMMIT);
            assert_eq!(done.key, KEY);
        }
        other => panic!(
            "a pack fsynced before the record walk arrives after the verdict and must be \
             registered, got {other:?}"
        ),
    }

    // Outside it: an attempt this record does not name.
    let mut wrong = a_registration(&job_id, attempt + 7);
    if let ToHub::RegisterPack(frame) = &mut wrong {
        frame.key = "another-key".to_owned();
    }
    runner.say(wrong).await;
    let answer = pack_answer(&mut runner).await;
    let why = refusal(&answer);
    assert!(
        why.contains(&format!("attempt {} of {job_id}", attempt + 7)),
        "the refusal names the attempt that was sent: {why}"
    );
    assert!(
        why.contains(&format!("attempt {attempt} is")),
        "and the attempt the record really holds, so the sentence can be acted on: {why}"
    );
    assert!(
        why.contains("(commit, key)"),
        "and why it matters, which is that a superseded attempt's pack would put a second id \
         on one key: {why}"
    );

    // And the link is still carrying traffic. Before this branch the frame
    // above was answered `FromHub::Refused`, which closes it (D13).
    runner.through_a_health_report().await;
}

/// **A fetch is refused once the job is over, and that is the other window.**
///
/// A pack fetched for a job whose verdict is written would be laid out under
/// an `{in}` nothing will ever read. `is_current` cannot be asked: `AskPack`
/// carries no attempt number. So `a_pack_may_be_fetched` reads the record,
/// and the refusal says what the job is rather than what its key is.
///
/// **This is the shape D150 cost.** There, `is_current` answered `JobIsOver`
/// for file chunks, the runner streamed 32, the hub read all 32 and wrote
/// none, and a requester was told its collection was not there with no error
/// anywhere. A fetch answered outside its window the same way would have a
/// runner move hundreds of megabytes into a directory nothing reads, and say
/// nothing either.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fetch_for_a_job_that_is_over_is_refused_by_name_and_not_answered_with_a_key() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;
    let (job_id, attempt) = a_job_assigned_to(&harness, &mut runner).await;

    // Inside the window first, so that the test distinguishes "refused
    // because of the window" from "refused always". The job is `assigned`:
    // the runner is laying `{in}` out and has reported nothing.
    runner.say(an_ask(&job_id)).await;
    match pack_answer(&mut runner).await {
        // Nothing is registered for the key, which is a **wait with a reason
        // on it** and not a refusal (D13): `pack` and `manifest` both absent
        // is the frame's own way of saying so, rather than an empty holder
        // list a runner would have to read as either that or `NoHolder`.
        FromHub::PackHolders(answer) => {
            assert!(
                answer.pack.is_none() && answer.manifest.is_none(),
                "nothing registered is said, not inferred: {answer:?}"
            );
            assert_eq!(answer.commit, COMMIT);
            assert_eq!(answer.key, KEY);
        }
        other => panic!("inside its window a fetch is answered, not refused: {other:?}"),
    }

    and_then_it_ends(&mut runner, &job_id, attempt).await;

    runner.say(an_ask(&job_id)).await;
    let answer = pack_answer(&mut runner).await;
    let why = refusal(&answer);
    assert!(
        why.contains(&job_id.to_string()) && why.contains("finished"),
        "the refusal names the job and the state that put it outside the window: {why}"
    );
    assert!(
        why.contains("{in}"),
        "and what would have happened to the bytes, which is the sentence that makes it \
         actionable rather than merely true: {why}"
    );
    runner.through_a_health_report().await;
}

/// **A holding is forgotten when the link drops, and re-announced on the
/// rejoin.** The two halves of D231 decision 5, measured as one sequence.
///
/// `PackRegistry::runner_gone` and `forget_all_holdings` had no caller
/// outside the registry's own tests on `main`, and not for want of a call
/// site: the two places that know a runner is gone are in `server.rs` and
/// could not reach the registry at all. The registry itself is untouched by
/// either — the manifests stay on the disk — and only the in-memory statement
/// "this machine has a complete copy" goes, which is the whole of why
/// holdings are volatile.
///
/// The second runner is a **second life of the same name**, because that is
/// the sequence a hub really sees: a machine rebooting, not a machine
/// disappearing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_link_forgets_what_a_runner_held_and_the_rejoin_says_it_again() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-life-that-held-it").await;
    let (job_id, attempt) = a_job_assigned_to(&harness, &mut runner).await;
    runner.say(a_registration(&job_id, attempt)).await;
    match pack_answer(&mut runner).await {
        FromHub::PackRegistered(_) => {}
        other => panic!("registered, got {other:?}"),
    }
    runner.announces(vec![the_pack()], false).await;
    runner.say(an_ask(&job_id)).await;
    assert_eq!(
        holders_of(&pack_answer(&mut runner).await).len(),
        1,
        "the hub records the holding before the link goes"
    );

    // The machine goes away holding every byte of it.
    drop(runner);
    support::wait_until("the hub notices the runner is gone", || {
        !harness.runner_joined()
    })
    .await;

    let mut back = Peer::runner(&harness.hub_url, RUNNER, &token, "the-life-that-came-back").await;
    // The record is still there, so the key still resolves: what must have
    // gone is the holder, and only the holder.
    back.say(an_ask(&job_id)).await;
    let after = pack_answer(&mut back).await;
    match &after {
        FromHub::PackHolders(answer) => {
            assert_eq!(
                answer.pack.as_ref(),
                Some(&the_pack()),
                "the registry survives the link: the manifest is on the disk"
            );
            assert!(
                answer.holders.is_empty(),
                "and the holding does not: a holder that is not connected is not a holder, \
                 and a hub that kept it would name a machine a fetcher cannot dial: {:?}",
                answer.holders
            );
        }
        // The job is over by now in the rejoin's eyes only if the record says
        // so; it does not — the attempt was never ended — so a refusal here
        // is a real failure and is reported as one.
        other => panic!("the key still resolves after a rejoin, got {other:?}"),
    }

    // And the rejoin says it again, which is the half that makes the
    // forgetting safe rather than merely correct.
    back.announces(vec![the_pack()], false).await;
    back.say(an_ask(&job_id)).await;
    let again = holders_of(&pack_answer(&mut back).await);
    assert_eq!(again.len(), 1, "re-announced: {again:?}");
    assert_eq!(again[0].runner, RUNNER);
}

/// **Zero payload bytes on the hub, counted against the figure the pack
/// declares.**
///
/// The hub is the pack's registry and authority and never carries a byte of
/// one (D10, D11, E-12). That is an invariant about this build, so it is
/// measured rather than asserted in prose: the manifest registered below
/// declares [`DECLARED_OBJECT_BYTES`] of objects in two `kind f` rows, and
/// after a registration driven through a frame the hub's **whole state root**
/// holds the manifest, the meta and the registration record and nothing else.
///
/// Counted over the whole root and not over `packs/`, deliberately: a hub
/// that wrote an object into a job directory, a spool or a temporary file
/// would pass a check that only looked where the packs are kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_registers_a_pack_and_holds_not_one_byte_of_its_objects() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;
    let (job_id, attempt) = a_job_assigned_to(&harness, &mut runner).await;

    let before = bytes_under(harness.state_dir());
    runner.say(a_registration(&job_id, attempt)).await;
    match pack_answer(&mut runner).await {
        FromHub::PackRegistered(_) => {}
        other => panic!("registered, got {other:?}"),
    }
    let after = bytes_under(harness.state_dir());

    let manifest = a_manifest().len() as u64;
    let meta = b"commit cb5a2e8b\nprofile debug\n".len() as u64;
    let grew = after.saturating_sub(before);
    println!(
        "the pack declares {DECLARED_OBJECT_BYTES} bytes of objects in 2 rows; the hub's \
         state root grew by {grew} bytes, of which {manifest} are the manifest and {meta} \
         the meta"
    );
    assert!(
        grew >= manifest + meta,
        "the manifest and the meta are on the disk verbatim, because the id is the sha256 of \
         exactly those bytes: grew {grew}, manifest {manifest}, meta {meta}"
    );
    // `registered.json` is the only other file a registration writes, and it
    // is a few hundred bytes of names and figures. The bound is what makes
    // the zero a count: there is no room under it for 12288 bytes of
    // objects, and no room for even the smaller of the two rows.
    let room_for_an_object = grew - manifest - meta;
    assert!(
        room_for_an_object < 4096,
        "whatever else a registration wrote, it is smaller than the smallest object the \
         manifest declares, so not one object is in it: {room_for_an_object} bytes beyond \
         the manifest and the meta"
    );

    // And the objects are nowhere by name either: a content-addressed store
    // names a file for its digest, and the hub has no file named for one of
    // these.
    for digest in ["1".repeat(64), "4".repeat(64)] {
        assert!(
            !names_anywhere_under(harness.state_dir(), &digest),
            "no file of the hub's state root is named for the object {digest}: the objects \
             live on the runners and the hub holds none of them"
        );
    }

    // The same statement from the registry's own side, through the seam the
    // hub reaches it by: the pack resolves, its manifest comes back, and
    // `PackState::Incomplete` is never computed here because holdings are per
    // manifest and the hub has no information about blobs.
    runner.say(an_ask(&job_id)).await;
    match pack_answer(&mut runner).await {
        FromHub::PackHolders(answer) => {
            let carried = answer.manifest.as_ref().map(Vec::len).unwrap_or_default() as u64;
            println!(
                "the hub's answer carries {carried} bytes — the manifest — against \
                 {DECLARED_OBJECT_BYTES} declared"
            );
            assert_eq!(
                answer.manifest.as_deref(),
                Some(a_manifest().as_slice()),
                "verbatim: a re-serialised manifest would be a different pack"
            );
            assert!(
                carried < DECLARED_OBJECT_BYTES,
                "and what rides the wire is the manifest and not the pack: {carried} against \
                 {DECLARED_OBJECT_BYTES}"
            );
        }
        other => panic!("the key resolves, got {other:?}"),
    }
}

/// Every byte of every file under a directory, recursively.
fn bytes_under(root: &Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            total += bytes_under(&entry.path());
        } else if let Ok(data) = entry.metadata() {
            total += data.len();
        }
    }
    total
}

/// Whether anything under a directory is named for this string.
fn names_anywhere_under(root: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(name) {
            return true;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && names_anywhere_under(&entry.path(), name)
        {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// The assignment side (ORCH-234)
// ---------------------------------------------------------------------------
//
// Measured on `main` at 26e83cf, before this: `FromHub::AssignJob` was built
// with `packs_in: Vec::new()` and `packs_out: BTreeMap::new()` at the one
// place it is built, and `Asked.needs`/`Asked.leaves` were dropped at
// `new_job` — they reached no record and no frame. So the hub's resolve, which
// `an_announcement_replaces_…` above proves works, was reachable **only** by
// a runner that asked for a pack it had never been offered.
//
// Both tests below are about an assignment and not about a pack answer, so
// neither goes through `pack_answer`: the subject is the frame that carries
// the work.

/// A requester, driven frame by frame, so a test can submit an `Asked` this
/// build's `herdctl` has no flag for.
///
/// `needs` and `leaves` ride the wire (protocol 3) and no requester surface
/// fills them yet — that is a separate row. Submitting the frame directly
/// measures the **hub's** behaviour, which is what ORCH-234 is about, and
/// adds no flag anybody then has to keep.
struct Requester {
    socket: Link,
}

impl Requester {
    async fn joining(hub_url: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a requester");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Requester,
            token: None,
            name: Some("a-requester-that-asks-for-a-pack".to_owned()),
            instance: None,
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
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
            other => panic!("a requester is welcomed, got {other:?}"),
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

    /// Submit a declared job on [`COMMIT`] that asks for `needs` and promises
    /// `leaves`, and leave with the id the hub minted.
    async fn submits(
        &mut self,
        needs: Vec<String>,
        leaves: std::collections::BTreeMap<String, String>,
    ) -> JobId {
        let asked = herd_proto::Asked {
            command: shell("true"),
            labels: Default::default(),
            priority: herd_proto::Priority::Step,
            build_thread_mib: None,
            memory_mib: None,
            tmpfs_mib: None,
            deadline_unix_s: None,
            after: None,
            requeue_exit_code: None,
            // A declaration, because a pack is registered for a
            // `(commit, key)` pair and only a declared command names a commit
            // (E-11, D106). Its hooks are the shape
            // `why_that_declaration_will_not_do` admits.
            declaration: Some(herd_proto::Declaration {
                command: "a-command-that-needs-a-pack".to_owned(),
                commit: COMMIT.to_owned(),
                args: Vec::new(),
                executor: herd_proto::Executor {
                    up: "tool ws up {job}".to_owned(),
                    fetch: "tool ws fetch {job}".to_owned(),
                    run: "tool ws run {job} -- {line}".to_owned(),
                    down: "tool ws down {job}".to_owned(),
                    env: Default::default(),
                },
            }),
            labels_from: None,
            timeout_s: None,
            needs,
            leaves,
        };
        self.socket
            .send(Message::text(
                to_frame(&ToHub::SubmitJob {
                    asked,
                    wants_files: false,
                    announce_end: false,
                })
                .expect("the submission encodes"),
            ))
            .await
            .expect("the submission is sent");
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .expect("the hub answers a submission inside the suite's patience")
                .expect("the link stays up");
            match frame {
                FromHub::JobAccepted { job_id } => return job_id,
                FromHub::Refused { reason } => panic!("the submission was refused: {reason}"),
                _ => continue,
            }
        }
    }

    /// Every note the hub sends about this job until one of them says `what`.
    async fn note_saying(&mut self, what: &str) -> String {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        let mut seen: Vec<String> = Vec::new();
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .unwrap_or_else(|_| {
                    panic!("no note said {what:?} inside the suite's patience; heard {seen:?}")
                })
                .expect("the link stays up: a job that waits is not a refusal (D13)");
            if let FromHub::JobNote { note, .. } = frame {
                if note.contains(what) {
                    return note;
                }
                seen.push(note);
            }
        }
    }
}

/// Register [`the_pack`] for `COMMIT`/`KEY` and announce that this runner
/// holds it, so that a later job's `needs` has something to resolve to.
///
/// One job is spent doing it, and it is **ended** first: a registration
/// arrives after the verdict (D232-D234), and ending it also gives the seat
/// back, so the job under test is placed on this same runner.
async fn a_pack_this_runner_holds(harness: &Harness, runner: &mut Peer) {
    let (job_id, attempt) = a_job_assigned_to(harness, runner).await;
    and_then_it_ends(runner, &job_id, attempt).await;
    runner.say(a_registration(&job_id, attempt)).await;
    match pack_answer(runner).await {
        FromHub::PackRegistered(done) => assert_eq!(done.pack, the_pack()),
        other => panic!("a post-fsync registration is accepted, got {other:?}"),
    }
    runner.announces(vec![the_pack()], false).await;
}

/// Read assignments until one is for this job, and answer with its packs.
async fn assignment_for(
    runner: &mut Peer,
    job_id: &JobId,
) -> (
    Vec<herd_proto::pack::AssignedPack>,
    std::collections::BTreeMap<String, String>,
) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let frame = tokio::time::timeout_at(deadline, runner.next())
            .await
            .expect("the hub assigns the job inside the suite's patience")
            .expect("the link stays up");
        if let FromHub::AssignJob {
            job_id: assigned,
            packs_in,
            packs_out,
            ..
        } = frame
        {
            if &assigned == job_id {
                return (packs_in, packs_out);
            }
        }
    }
}

/// **A `needs` key is resolved where the assignment is built, and the
/// manifest and the holder ride with the work** (M11, E-12, ORCH-234).
///
/// The assertion the fields exist for: the common case costs **no round
/// trip**. A runner handed this frame has the manifest — kilobytes (D10) —
/// and a holder to fetch from, so it lays `{in}/<key>` out without saying a
/// word to anybody, and `ToHub::AskPack` is left for the case the hub had
/// nothing resolved.
///
/// **Verbatim, and that is not a detail**: a pack's id is the sha256 of its
/// `manifest.tsv`, so a manifest that went out re-serialised would be a
/// different pack, and a runner comparing the two would refuse its own
/// inputs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_whose_needs_resolve_is_assigned_the_manifest_the_holder_and_its_leaves() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;
    a_pack_this_runner_holds(&harness, &mut runner).await;

    let mut requester = Requester::joining(&harness.hub_url).await;
    let leaves = std::collections::BTreeMap::from([("built".to_owned(), "artefacts".to_owned())]);
    let job_id = requester
        .submits(vec![KEY.to_owned()], leaves.clone())
        .await;
    let (packs_in, packs_out) = assignment_for(&mut runner, &job_id).await;

    assert_eq!(
        packs_in.len(),
        1,
        "one `needs` key resolves to one pack on the assignment: {packs_in:?}"
    );
    let assigned = &packs_in[0];
    assert_eq!(
        assigned.key,
        KEY,
        "named by the key the job asked for, which is what names the directory under {}",
        herd_proto::IN_TOKEN
    );
    assert_eq!(assigned.pack, the_pack());
    assert_eq!(
        assigned.manifest,
        a_manifest(),
        "the manifest's bytes, verbatim: the id is their sha256"
    );
    assert_eq!(
        assigned.meta.as_deref(),
        Some(&b"commit cb5a2e8b\nprofile debug\n"[..]),
        "and `pack.meta` with it, also verbatim"
    );
    assert_eq!(
        assigned
            .holders
            .iter()
            .map(|holder| (holder.runner.as_str(), holder.endpoint.as_deref()))
            .collect::<Vec<_>>(),
        vec![(RUNNER, Some(ENDPOINT))],
        "with the holder to fetch from and the address it serves on, so the common path costs \
         no round trip"
    );
    assert_eq!(
        packs_out, leaves,
        "and what the job is to leave, handed back with the attempt for the reason the \
         declaration is: a requeue has to leave the same artefacts under the same keys"
    );
}

/// **A `needs` key nothing is registered for is a job that waits, by name,
/// and never a `Refused`** (D13).
///
/// Three claims in one test because they are one behaviour: the job is **not**
/// assigned, the sentence names the **key and the commit**, and the link is
/// still up at the end. `FromHub::Refused` is terminal, and a pack that is
/// not there yet is the ordinary case of a fleet building one commit — the
/// job that leaves the pack simply has not registered it. A hub that refused
/// here would cost a requester its connection for being early.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_needing_a_pack_nothing_registered_waits_by_name_and_is_not_assigned() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let mut runner = Peer::runner(&harness.hub_url, RUNNER, &token, "the-only-life").await;

    let mut requester = Requester::joining(&harness.hub_url).await;
    let job_id = requester
        .submits(vec![KEY.to_owned()], Default::default())
        .await;

    // The hub's own sentence, read where a requester reads it.
    let note = requester
        .note_saying(&format!("nothing is registered for {COMMIT}/{KEY}"))
        .await;
    assert!(
        note.contains("this job is queued and has not started"),
        "it is a wait and not a verdict: {note}"
    );
    assert!(
        note.contains(&format!("empty {}/{KEY}", herd_proto::IN_TOKEN)),
        "and it says what running it anyway would have meant: {note}"
    );

    // And the runner was handed nothing. Read after the note, so this is not
    // a race with a dispatch that had not happened yet: the hub had already
    // walked this job in a pass and said why it stayed.
    runner.health().await;
    let seen = runner.through_a_health_report().await;
    assert!(
        !seen
            .iter()
            .any(|frame| matches!(frame, FromHub::AssignJob { .. })),
        "a job whose pack does not resolve is not handed to a runner: {seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|frame| matches!(frame, FromHub::Refused { .. })),
        "and nobody's link is closed over it (D13): {seen:?}"
    );
    // The job is still the hub's to place, not finished and not refused.
    let record = herd_hub::JobStore::job(harness.store.as_ref(), &job_id)
        .expect("the record is readable")
        .expect("the job the hub minted is on the disk");
    assert_eq!(
        record.needs,
        vec![KEY.to_owned()],
        "the keys are on the record, which is what a requeue resolves again from"
    );
}
