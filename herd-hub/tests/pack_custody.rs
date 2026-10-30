//! **What the real `herd-runner` binary keeps, and what it lets its budget
//! take** (M12, E-16).
//!
//! The milestone's own clause, as a test: *a runner given `keeper_for` keeps,
//! under a budget that would otherwise take it, a pack an unpinned runner
//! loses*. Two runners, the shipped binary both times, started as processes
//! on two identical disks with one identical `--pack-budget`. The only
//! difference between the two runs is **one field of one frame** — the
//! `keeper_for` of the welcome — and the difference in outcome is read off
//! the wire, in what each runner then announces that it holds.
//!
//! ## Why the hub here is hand-written
//!
//! The real hub sends `keeper_for: Vec::new()` at `herd-hub/src/server.rs`
//! and sends no `FromHub::PinPacks` at all: choosing a keeper is M12's
//! keeper policy, which is the hub's half and is not written yet. A real hub
//! therefore cannot make a runner a keeper, and a test that waited for one
//! would be testing the policy and not the custody. So the observation point
//! is a peer at the other end of the socket, which is also the honest one:
//! what this step owes is a **runner that honours what it was told**, and
//! that is exactly what is read here.
//!
//! Same division, and the same reasoning, as `pack_store_health.rs` beside
//! it.
//!
//! ## Why it lives beside the hub's tests and not the runner's
//!
//! It binds a listening socket, and
//! `no_module_of_the_runner_can_bind_a_socket`
//! (`herd-runner/tests/confinement.rs`) forbids the string `TcpListener`
//! anywhere under the `herd-runner` crate, its tests included — invariant 3:
//! a runner dials out and binds nothing. A test standing in for a **hub**
//! belongs here.
//!
//! ## What is planted, and why the budget is the figure it is
//!
//! Three packs of one object each, **1 000 bytes every one of them**, all
//! filed under one key family — planted on the disk in the shape a runner's
//! *second life* finds it in, because what is measured is a runner started
//! on a disk that already holds packs. 3 000 bytes in all, against a budget
//! of 1 500: two packs have to go to cover that and one has to stay.
//!
//! **One length and not three, deliberately.** The eviction order is by age,
//! and a file's mtime on Linux comes from a coarse clock whose granularity
//! is a scheduler tick: three packs planted in one loop can be filed in the
//! same instant, and the id then breaks the tie. Three packs of three sizes
//! would make the surviving *count* depend on which one went first, which is
//! a flake and not a measurement. Three equal packs make it the budget's
//! answer and nobody else's: any two of them cover the overrun and no one of
//! them does. **Which** pack goes is `packstore`'s own unit measurement,
//! where a fixture can plant the mtimes it needs.
//!
//! So an unpinned runner comes under its budget holding **one** pack, and a
//! keeper of that family holds **three** and says that it could not come
//! under it.

use std::fs::File;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::pack::PinPacks;
use herd_proto::{from_frame, to_frame, FromHub, PackId, ToHub};
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Message;

mod real_herd;

use real_herd::{binary, PATIENCE};

/// The key family every planted pack is filed under, and the one a keeper is
/// named for here.
const A_KEY_FAMILY: &str = "pack";

/// The budget, as an operator types it on the runner's command line: half of
/// what is planted, rounded so that two of the three packs cover it.
const BUDGET_TYPED: &str = "1500";

/// How many packs are planted, and how long the one object of each is. One
/// length for all three, for the reason this file's own doc gives.
const PLANTED_PACKS: usize = 3;
const OBJECT_LENGTH: usize = 1_000;

/// How many packs an unpinned runner is left holding: one, because two of
/// three had to go to cover 1 500 bytes of overrun and no one of them would
/// have.
const LEFT_UNPINNED: usize = 1;

/// Plant [`PLANTED_PACKS`] packs of [`OBJECT_LENGTH`] bytes, every one of
/// them filed under [`A_KEY_FAMILY`].
///
/// The key goes down through the store's own `remember_key`, which is what
/// `leave_packs` calls on a real job's artefact: a fixture that wrote
/// `pack.key` itself could pass while the two spellings disagreed.
fn plant_a_keyed_store(data_root: &Path) -> usize {
    let store = data_root.join(herd_runner::packstore::PACKS_DIR);
    let objects = store.join("objects");
    let manifests = store.join("manifests");
    std::fs::create_dir_all(&objects).expect("the store's objects directory");
    std::fs::create_dir_all(&manifests).expect("the store's manifests directory");

    for which in 0..PLANTED_PACKS {
        // As long as it says and distinct from its neighbours', so three
        // equal-sized packs are still three different packs.
        let content = vec![b'a' + which as u8; OBJECT_LENGTH];
        let digest = herd_proto::sha256_hex(&content);
        std::fs::write(objects.join(&digest), &content).expect("a planted object");
        // The directory's name **is** the sha256 of the manifest beside it,
        // which is the store's identity rule.
        let manifest = format!(
            "{}\n# profile debug\n{digest}\t{OBJECT_LENGTH}\t0644\tf\tartefact\tbin/tool-{which}\n",
            herd_proto::CHATONS_PACK_FORMAT,
        )
        .into_bytes();
        let at = manifests.join(herd_proto::sha256_hex(&manifest));
        std::fs::create_dir_all(&at).expect("a planted pack directory");
        std::fs::write(at.join("manifest.tsv"), &manifest).expect("a planted manifest");
    }

    let store = herd_runner::packstore::PackStore::open(data_root).expect("the store opens");
    let held = store.held_packs().expect("the store lists what it holds");
    assert_eq!(
        held.len(),
        PLANTED_PACKS,
        "the fixture plants one pack per object"
    );
    for id in &held {
        store
            .remember_key(id, A_KEY_FAMILY)
            .expect("the key family goes down beside the manifest");
    }
    held.len()
}

/// How many packs this store holds, read off the disk.
fn packs_on_the_disk(data_root: &Path) -> usize {
    herd_runner::packstore::PackStore::open(data_root)
        .expect("the store opens")
        .held_packs()
        .expect("the store lists what it holds")
        .len()
}

/// Start the real runner on a planted store with [`BUDGET_TYPED`], welcome it
/// with this `keeper_for`, and come back with the holdings it announces, what
/// its disk holds afterwards, and everything it said.
///
/// The announcement is read rather than guessed at because it is the one
/// place a hub learns what survived: the frame set replaces, so the complete
/// announcement — the frames up to and including `more: false` — *is* this
/// runner's holdings as it now has them. And it arrives **after** the pass,
/// which is the order `serve_once` keeps so that a hub is never left
/// believing a holder of a pack that has just gone.
async fn a_runner_welcomed_as(
    keeper_for: Vec<String>,
    pin_after: Option<PinPacks>,
) -> (Vec<PackId>, usize, String) {
    let data_root = tempfile::tempdir().expect("a temporary data root");
    let planted = plant_a_keyed_store(data_root.path());
    // Read before the frame is moved into the hub below: whether this run
    // has a pin line to wait for at all.
    let pinned_later = pin_after.is_some();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free loopback port");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    // A hub that welcomes with the custody under test, answers every health
    // report, and hands back the first complete announcement. It answers
    // rather than falling silent so that nothing the runner does here is a
    // reaction to a hub that stopped reading.
    let hub = tokio::spawn(async move {
        let (stream, _from) = listener.accept().await.expect("the runner dials in");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("the websocket handshake");
        let hello = socket.next().await.expect("the runner says hello");
        let hello = hello.expect("the hello arrives whole");
        let hello: ToHub =
            from_frame(hello.to_text().expect("a text frame")).expect("the hello decodes");
        assert!(
            matches!(hello, ToHub::Hello { .. }),
            "the first frame is the handshake, not {hello:?}"
        );
        socket
            .send(Message::text(
                to_frame(&FromHub::Welcome {
                    protocol_version: herd_proto::PROTOCOL_VERSION,
                    hub_version: "a hand-written hub".to_owned(),
                    state_root: "s-the-hand-written-hub".to_owned(),
                    version_note: None,
                    keeper_for,
                })
                .expect("the welcome encodes"),
            ))
            .await
            .expect("the welcome is sent");
        if let Some(pins) = pin_after {
            socket
                .send(Message::text(
                    to_frame(&FromHub::PinPacks(pins)).expect("the pin frame encodes"),
                ))
                .await
                .expect("the pin frame is sent");
        }
        let mut announced: Vec<PackId> = Vec::new();
        let mut complete = false;
        let mut reports_after = 0;
        loop {
            let frame = socket
                .next()
                .await
                .expect("the runner keeps talking")
                .expect("the frame arrives whole");
            let Ok(text) = frame.to_text() else { continue };
            let Ok(message) = from_frame::<ToHub>(text) else {
                continue;
            };
            match message {
                ToHub::PacksHeld(held) => {
                    if !complete {
                        announced.extend(held.packs);
                        complete = !held.more;
                    }
                }
                ToHub::Health { .. } => {
                    socket
                        .send(Message::text(
                            to_frame(&FromHub::HealthRecorded {}).expect("the answer encodes"),
                        ))
                        .await
                        .expect("the report is answered");
                    if complete {
                        reports_after += 1;
                        // Two answered reports past the announcement: the
                        // link has carried frames in both directions since,
                        // which is as much as a peer can know about frames
                        // the runner has read.
                        if reports_after == 2 {
                            return announced;
                        }
                    }
                }
                _ => continue,
            }
        }
    });

    let logs = tempfile::tempdir().expect("a temporary directory for the logs");
    let out = logs.path().join("runner-out");
    let err = logs.path().join("runner-err");
    let mut child = Command::new(binary("herd-runner"))
        .arg("--hub")
        .arg(format!("ws://{addr}"))
        .arg("--name")
        .arg("the-runner-under-a-pack-budget")
        .arg("--data-root")
        .arg(data_root.path())
        .arg("--pack-budget")
        .arg(BUDGET_TYPED)
        .arg("--health-every")
        .arg("0.2")
        .arg("--no-detect")
        .stdout(Stdio::from(
            File::create(&out).expect("a file for the runner's stdout"),
        ))
        .stderr(Stdio::from(
            File::create(&err).expect("a file for the runner's stderr"),
        ))
        .kill_on_drop(true)
        .spawn()
        .expect("herd-runner starts");

    let announced = tokio::time::timeout(PATIENCE, hub).await;
    let said = || {
        format!(
            "{}{}",
            std::fs::read_to_string(&out).unwrap_or_default(),
            std::fs::read_to_string(&err).unwrap_or_default()
        )
    };
    let announced = announced
        .unwrap_or_else(|_| {
            panic!(
                "a real runner announces what it holds. It said:\n{}",
                said()
            )
        })
        .expect("the hand-written hub ends");
    // Read before the child is killed, so that what is asserted on is what
    // the runner had already written down.
    let mut everything = said();
    // And the pin line, which is written when the frame is **read** and not
    // when it was sent: waited for on that condition rather than on a clock,
    // and only by the run that sent one.
    if pinned_later {
        let until = std::time::Instant::now() + PATIENCE;
        while !everything.contains("the hub pins") && std::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(20)).await;
            everything = said();
        }
    }
    let _ = child.start_kill();
    assert!(
        everything.contains(&format!("in {planted} pack(s)")),
        "the runner's own startup line names the pack count it measured, before any pass. \
         It said:\n{everything}"
    );
    (announced, packs_on_the_disk(data_root.path()), everything)
}

/// **A runner the hub names no keeper for comes under its budget, and loses
/// packs to do it.**
///
/// The control, and the half that proves the budget is really spent: without
/// it, a keeper keeping everything would be indistinguishable from a gc that
/// never runs. This is also the assertion that closes "nothing calls
/// `evict_to_budget`" — a pass that had no caller could not take these two
/// packs away.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unpinned_runner_loses_packs_to_come_under_its_budget() {
    let (announced, on_the_disk, said) = a_runner_welcomed_as(Vec::new(), None).await;
    assert_eq!(
        announced.len(),
        LEFT_UNPINNED,
        "three packs planted, 4 608 bytes, a budget of 4 200: the two oldest go and the \
         newest stays. It said:\n{said}"
    );
    assert_eq!(
        on_the_disk,
        announced.len(),
        "and what it announces is what is on its disk, which is the invariant an \
         announcement after the pass exists for. It said:\n{said}"
    );
    assert!(
        said.contains("pack(s) and 2 object(s) taken away"),
        "and it says what it took and what that freed, in its own voice. It said:\n{said}"
    );
}

/// **A keeper keeps, under the same budget, the packs the unpinned runner
/// lost** (the milestone's own clause).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_keeper_keeps_every_pack_of_a_pinned_key_under_the_same_budget() {
    let (announced, on_the_disk, said) =
        a_runner_welcomed_as(vec![A_KEY_FAMILY.to_owned()], None).await;
    assert_eq!(
        announced.len(),
        PLANTED_PACKS,
        "one field of one frame is the whole difference from the run above, and this runner \
         keeps all three. It said:\n{said}"
    );
    assert!(
        announced.len() > LEFT_UNPINNED,
        "which is more than the unpinned runner was left holding, or this file is \
         measuring nothing. It said:\n{said}"
    );
    assert_eq!(
        on_the_disk,
        announced.len(),
        "and they are on its disk, not merely announced. It said:\n{said}"
    );
    assert!(
        said.contains("makes this runner a keeper for key(s) \"pack\""),
        "it says what it keeps, on the machine whose disk it is. It said:\n{said}"
    );
    assert!(
        said.contains("still over its budget") && said.contains("pinned here as a keeper's"),
        "and it says that it could not come under its budget and why, rather than staying \
         over it in silence. It said:\n{said}"
    );
}

/// **A `PinPacks` arriving after the welcome changes the set, and says so
/// once.**
///
/// The second half of the clause. The frame arrives *after* this
/// connection's pass has already run, so what it changes is what the next
/// one may take — which is why what is asserted here is the line and the
/// count of it, and not a pack that moved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pin_packs_frame_after_the_welcome_changes_the_set_and_says_so_once() {
    let (_announced, _on_the_disk, said) = a_runner_welcomed_as(
        vec![A_KEY_FAMILY.to_owned()],
        Some(PinPacks {
            keys: vec!["reference".to_owned()],
            keep_per_key: 2,
        }),
    )
    .await;
    let lines: Vec<&str> = said
        .lines()
        .filter(|line| line.contains("the hub pins"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "one frame, one line — not one per key and not one per pack. It said:\n{said}"
    );
    let line = lines[0];
    assert!(
        line.contains("\"reference\"")
            && line.contains("newest 2 pack(s)")
            && line.contains(A_KEY_FAMILY),
        "and the line names what is kept now, how many of each, and what was kept before: \
         {line}"
    );
}
