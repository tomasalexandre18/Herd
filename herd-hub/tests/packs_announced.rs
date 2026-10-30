//! **The real runner announces what it holds, to a real hub** (ORCH-232,
//! D231 decision 5).
//!
//! This is the one measurement of the holdings mechanism that is not made
//! against a hand-driven peer. `pack_frames.rs` drives five tests through a
//! socket with a peer that encodes its own frames, and what such a peer
//! proves is that **a hub told about a holding records it** — the sequence it
//! asserts is the sequence the test author typed. Nothing, until this file,
//! measured whether the `herd-runner` binary says anything at all.
//!
//! Measured on `main` at `6d10427`, before this: `herd-runner` constructed
//! `ToHub::PacksHeld` in **no place**, so every running hub's holdings map
//! was empty whatever was on a runner's disk. The hub cannot ask either — it
//! does not send `FromHub::ReadPacksAgain` (ORCH-233 carries the figures that
//! withdrew it) — so the announcement has to be unprompted or there is none.
//!
//! ## What is asserted, and why each needs two real processes
//!
//! 1. **A runner whose store holds packs announces them, unprompted, on its
//!    own join.** Nothing asks. The frame has no acknowledgement in the frame
//!    set, so the hub's own journal line is the observation point, and it is
//!    read out of the hub *process*'s stderr.
//! 2. **The announcement is cut into frames and the hub accumulates them.**
//!    The hub says "so far and more to come" for a `more: true` frame and
//!    "which replaces what this hub had for it" for the one that applies, so
//!    the two sentences together are the chunking measured from the far end
//!    of a socket rather than from the function that cut it.
//! 3. **A runner that holds nothing says so.** The empty announcement is the
//!    frame set's only retraction, and the hub names it in its own words:
//!    *every holding it had is forgotten*. No announcement at all would leave
//!    a hub offering a holder that has dropped everything.
//!
//! ## The scale is not a knob
//!
//! [`PLANTED`] is `PACKS_HELD_PER_FRAME + 4`. A fixture at or below the
//! window is green while measuring nothing, because it never cuts — the same
//! trap `spool_offer_answered.rs` was caught by with one collection against a
//! window of eight, and the same `+ 4`. Four over the window forces exactly
//! two frames, so the `more: true` path is walked and the remainder is not a
//! whole window either.
//!
//! ## What is planted, and what is not
//!
//! Each planted pack is a manifest directory in the store's own shape: a
//! 64-hex name that **is** the sha256 of the `manifest.tsv` beside it, which
//! is the identity rule. The objects are not planted, and that is deliberate
//! rather than a shortcut: the manifest is written last, so its presence is
//! what the store means by holding a pack, and this file measures the
//! announcement and not completeness. What it does mean is stated as a
//! finding in the handoff rather than left for a reader to discover.

use std::path::Path;

use herd_runner::packstore::{PACKS_DIR, PACKS_HELD_PER_FRAME};

mod real_herd;

use real_herd::Herd;

/// How many packs are planted on the runner's disk: four over the window, for
/// the reason the module doc gives.
const PLANTED: usize = PACKS_HELD_PER_FRAME + 4;

/// Plant `how_many` pack manifests in a runner's store, and come back with
/// their ids.
///
/// Each one's directory name is the sha256 of its own `manifest.tsv`, so the
/// store is not being handed a name it would refuse the moment anything read
/// the manifest back.
fn plant_packs(data_root: &Path, how_many: usize) -> Vec<String> {
    let manifests = data_root.join(PACKS_DIR).join("manifests");
    std::fs::create_dir_all(&manifests).expect("the store's manifests directory");
    let mut ids = Vec::new();
    for which in 0..how_many {
        let manifest = format!(
            "{}\n# profile debug\n{}\t{}\t0644\tf\tartefact\tbin/tool-{which}\n",
            herd_proto::CHATONS_PACK_FORMAT,
            herd_proto::sha256_hex(format!("the bytes of pack {which}").as_bytes()),
            24 + which,
        );
        let bytes = manifest.into_bytes();
        let id = herd_proto::sha256_hex(&bytes);
        let at = manifests.join(&id);
        std::fs::create_dir_all(&at).expect("a planted pack directory");
        std::fs::write(at.join("manifest.tsv"), &bytes).expect("a planted manifest");
        ids.push(id);
    }
    ids.sort();
    ids
}

/// **Unprompted, cut into frames, and the hub holds the set back until the
/// last one.**
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_runner_announces_what_it_holds_and_the_hub_records_every_frame_of_it() {
    let herd = Herd::new();
    let ids = plant_packs(&herd.runner_root("runner"), PLANTED);
    assert_eq!(ids.len(), PLANTED, "the fixture is the size it claims");

    let mut hub = herd.start_hub("hub").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // The frame that **applies** the announcement, in the hub's own words.
    //
    // **The hub's figure is the last frame's, not the holdings' — and that is
    // the hub's line and not a miscount here.** `relay_from_runner` reads
    // `announcement.packs.len()` off the frame in hand, so a chunked
    // announcement of 516 is journalled as *holds 4 pack(s)* while the
    // registry accumulates all 516. Measured here, 2026-10-30: the hub
    // printed `512 so far and more to come` and then `holds 4 pack(s)`. The
    // expectation is written against what the hub really says rather than
    // against what it should say, and the line it should say is a finding in
    // this task's handoff — fixing it is a `herd-hub/src` change and this
    // task's write path is `herd-runner/src`.
    let applied = format!(
        "runner the-real-runner holds {} pack(s), which replaces what this hub had for it",
        PLANTED - PACKS_HELD_PER_FRAME
    );
    herd.wait_until("the hub applies what the runner announced", || {
        herd.said("hub").contains(&applied)
    })
    .await;

    // And the frame before it, which the hub accumulated and did not apply.
    let accumulated = format!(
        "runner the-real-runner is announcing its packs, {PACKS_HELD_PER_FRAME} so far and more \
         to come"
    );
    assert!(
        herd.said("hub").contains(&accumulated),
        "the announcement was cut and the hub held the first frame back. Expected \
         {accumulated:?}\n{}",
        herd.everything_said()
    );

    // Nothing asked for it: the hub has no frame that does, and this is the
    // assertion that says the runner volunteered rather than answered.
    assert!(
        !herd.said("runner").contains("read_packs_again"),
        "the runner answered an ask instead of volunteering\n{}",
        herd.everything_said()
    );
    // And the runner says what it did, in its own voice and with the cut.
    let said = format!("this runner holds {PLANTED} pack(s), announced in 2 frame(s)");
    assert!(
        herd.said("runner").contains(&said),
        "the runner names what it announced. Expected {said:?}\n{}",
        herd.everything_said()
    );

    runner.ask_it_to_stop();
    hub.ask_it_to_stop();
}

/// **An empty store announces that it is empty**, which is the retraction and
/// is not the same as saying nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_runner_that_holds_no_pack_says_so_and_the_hub_forgets_what_it_had() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("hub").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let retracted =
        "runner the-real-runner holds no pack; every holding it had is forgotten".to_owned();
    herd.wait_until("the hub is told the runner holds nothing", || {
        herd.said("hub").contains(&retracted)
    })
    .await;
    assert!(
        herd.said("runner")
            .contains("this runner holds no pack, and says so"),
        "the runner says it is announcing an emptiness rather than skipping it\n{}",
        herd.everything_said()
    );

    runner.ask_it_to_stop();
    hub.ask_it_to_stop();
}
