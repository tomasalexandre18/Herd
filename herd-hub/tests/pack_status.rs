//! **What the hub shows of a runner's pack store** (M12 P2b, E-16, E-35),
//! against the real binaries and the real document on the disk.
//!
//! P2a put three figures on the wire and the hub discarded all three, so the
//! measurement reached nobody. What this file asserts is the other end of
//! that: a real `herd-runner` with a store planted on its disk, a real
//! `herd-hub` serving a real state root, and the figures read back out of
//! `status.json` and off `herd-hub status`'s own screen.
//!
//! ## Why the hub here is the real one
//!
//! `pack_store_health.rs` beside this file hand-writes its hub, because what
//! P2a owed was a correct frame *leaving the runner* and the real hub had
//! nothing to say about it. P2b owes the opposite — that the hub **records**
//! it, publishes it and prints it — and none of that can be read off a
//! socket. So the observation points here are the document the hub wrote and
//! the screen the shipped binary renders from it, which is also M13's rule:
//! the screen is a render of the document and of nothing else.
//!
//! ## The two states, and why they are the two
//!
//! 1. **A runner holding a known store, with a budget its owner typed.** Both
//!    figures on one line, the use through `measured_bytes` and the budget
//!    through `show_bytes` (ORCH-186).
//! 2. **A runner whose pack store will not open.** All three figures absent,
//!    which is the one silence a real binary can be made to produce — every
//!    other runner opens and measures its store at the dial, so an empty
//!    store sends a real `0`. It is also the state that matters most: P2a's
//!    own finding is that the failure mode of this mechanism is a machine
//!    holding gigabytes that says nothing at all, and on a machine nobody
//!    here may log into (D161) the hub's screen is the only place that shows.
//!
//! The document is **not state a test may checksum** (D215): a serving hub
//! rewrites it about five times a second. So every wait here is on what the
//! document *says*, never on its bytes.

mod real_herd;

use std::path::Path;
use std::time::{Duration, Instant};

use herd_hub::{Status, STATUS_FILE};
use real_herd::{Herd, PATIENCE};

/// The budget the operator types, and the same figure in bytes.
///
/// `200GB` is round in the decimal family and in no other, which is what
/// makes the renderer assertion decisive: through `show_bytes` it reads back
/// as `200 GB`, the figure its owner typed, and through `measured_bytes` it
/// would read as `200000000000 bytes (186.26 GiB)` — correct, and not the
/// number anybody wrote.
const BUDGET_TYPED: &str = "200GB";
const BUDGET_SHOWN: &str = "200 GB";

/// The lengths of the planted objects, one per pack. Three different lengths,
/// so that a total which happened to be a multiple of the count could not
/// pass for the sum.
const OBJECT_LENGTHS: [usize; 3] = [11, 500, 4_097];

/// What every runner in this file is started with.
///
/// `--no-detect` so nothing depends on whether this host has a `/dev/kvm`,
/// and two lends any host can make — a runner on a crowded workstation
/// refuses to start on the defaults, which is `real_herd`'s own measurement
/// and not a weakened assertion.
const RUNNER_OPTIONS: [&str; 3] = [
    "--no-detect",
    "--host-memory-floor=64MiB",
    "--job-memory-max=256MiB",
];

/// Plant a store on a runner's disk, in the shape a runner's **second life**
/// finds it in, and answer with what it must be measured as.
///
/// Planted rather than ingested through the store's API, for
/// `pack_store_health.rs`'s own reason: what is being measured is a runner
/// started on a disk that already holds packs, which is the ordinary case.
/// `objects/` holds files named by the sha256 of their own contents, which is
/// the only name the store counts; `manifests/<id>/manifest.tsv` is what it
/// means by holding a pack.
fn plant_a_store(data_root: &Path) -> (u64, u32) {
    let store = data_root.join(herd_runner::packstore::PACKS_DIR);
    let objects = store.join("objects");
    let manifests = store.join("manifests");
    std::fs::create_dir_all(&objects).expect("the store's objects directory");
    std::fs::create_dir_all(&manifests).expect("the store's manifests directory");

    let mut bytes = 0u64;
    for (which, length) in OBJECT_LENGTHS.iter().enumerate() {
        let content = vec![b'a' + which as u8; *length];
        let digest = herd_proto::sha256_hex(&content);
        std::fs::write(objects.join(&digest), &content).expect("a planted object");
        bytes += content.len() as u64;

        let manifest = format!(
            "{}\n# profile debug\n{digest}\t{length}\t0644\tf\tartefact\tbin/tool-{which}\n",
            herd_proto::CHATONS_PACK_FORMAT,
        );
        let manifest = manifest.into_bytes();
        let id = herd_proto::sha256_hex(&manifest);
        let at = manifests.join(&id);
        std::fs::create_dir_all(&at).expect("a planted pack directory");
        std::fs::write(at.join("manifest.tsv"), &manifest).expect("a planted manifest");
    }
    (bytes, OBJECT_LENGTHS.len() as u32)
}

/// Wait until the document says something about the fleet, and answer with
/// it.
///
/// On what it says and never on its bytes: the publisher writes on change
/// (D215) and a serving hub changes something several times a second.
async fn until(herd: &Herd, what: &str, mut ready: impl FnMut(&Status) -> bool) -> Status {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Ok(Some(status)) = Status::read(herd.state.path()) {
            if ready(&status) {
                return status;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the status document never showed {what} inside {PATIENCE:?}. It last \
             said:\n{}\n{}",
            Status::read(herd.state.path())
                .ok()
                .flatten()
                .map(|status| status.to_string())
                .unwrap_or_else(|| "(no document at all)".to_owned()),
            herd.everything_said()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `status.json`'s own bytes, as a value, so that an **absent** key can be
/// told from a null one.
///
/// The typed `Status` cannot answer that question: `Option<u64>` is `None`
/// either way. The whole point of these three fields is that absence means
/// "the runner did not say", and a renderer reading `null` as `0` would draw
/// a flat line for a figure nobody measured — so the JSON is read as JSON.
fn the_document_as_json(herd: &Herd) -> serde_json::Value {
    let bytes = std::fs::read(herd.state.path().join(STATUS_FILE))
        .unwrap_or_else(|error| panic!("reading {STATUS_FILE}: {error}"));
    serde_json::from_slice(&bytes).expect("the document is JSON")
}

/// What the shipped `herd-hub status` prints against this root.
async fn the_screen(herd: &Herd) -> String {
    let output = tokio::process::Command::new(real_herd::binary("herd-hub"))
        .arg("status")
        .arg(herd.state.path())
        .output()
        .await
        .expect("herd-hub status runs");
    assert!(
        output.status.success(),
        "herd-hub status failed against a serving hub: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The one line of that screen this file is about.
fn the_pack_store_line(screen: &str) -> &str {
    screen
        .lines()
        .find(|line| line.contains("its pack store:"))
        .unwrap_or_else(|| panic!("the screen has no pack-store line:\n{screen}"))
}

/// **The plan's clause, end to end: one runner's use against its budget, with
/// both figures, driven from the document and nothing else.**
#[tokio::test(flavor = "multi_thread")]
async fn the_hub_shows_a_real_runners_pack_use_against_its_budget() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;

    // Planted before the runner starts, so the store is one it finds rather
    // than one it made: its figures are read at the dial.
    let root = herd.runner_root("runner");
    std::fs::create_dir_all(&root).expect("the runner's data root");
    let (planted_bytes, planted_packs) = plant_a_store(&root);
    assert!(planted_bytes > 0, "the fixture plants a real store");

    let mut options: Vec<&str> = RUNNER_OPTIONS.to_vec();
    let budget = format!("--pack-budget={BUDGET_TYPED}");
    options.push(&budget);
    let _runner = herd.start_runner_with("the-real-runner", "runner", "runner", &options);

    let status = until(&herd, "a runner that has said what it holds", |status| {
        status
            .runners
            .iter()
            .any(|runner| runner.pack_store_bytes.is_some())
    })
    .await;
    let runner = &status.runners[0];
    assert_eq!(runner.name, "the-real-runner");
    assert_eq!(
        runner.pack_store_bytes,
        Some(planted_bytes),
        "the document carries the store's own bytes, measured by the runner"
    );
    assert_eq!(
        runner.packs_held,
        Some(planted_packs),
        "and the count of packs beside them"
    );
    assert_eq!(
        runner.pack_store_budget_bytes,
        Some(200_000_000_000),
        "and the budget its owner typed, unchanged"
    );

    // The JSON itself, because a `Some` in the typed document would also be
    // satisfied by a hub that invented it. These are the keys the wire uses.
    let json = the_document_as_json(&herd);
    let said = &json["runners"][0];
    assert_eq!(said["pack_store_bytes"], serde_json::json!(planted_bytes));
    assert_eq!(said["packs_held"], serde_json::json!(planted_packs));
    assert_eq!(
        said["pack_store_budget_bytes"],
        serde_json::json!(200_000_000_000u64)
    );

    // And the screen, from the shipped binary against the live root.
    let screen = the_screen(&herd).await;
    let line = the_pack_store_line(&screen);
    assert_eq!(
        line,
        format!(
            "    its pack store: {} of {BUDGET_SHOWN} allowed, in {planted_packs} pack(s)",
            herd_proto::size::measured_bytes(planted_bytes)
        ),
        "the use, the budget and the count, on one line: {screen}"
    );
    // ORCH-186's half: a figure its owner typed must read back in the family
    // it was typed in, and a measurement must carry its exact count.
    assert!(
        !line.contains("186.26 GiB") && !line.contains("200000000000"),
        "the budget went through the measured renderer: {line}"
    );
    assert!(
        line.contains(&planted_bytes.to_string()),
        "the use must carry its exact figure, not only an approximation: {line}"
    );
}

/// **A runner that said nothing prints `did not say`, and never `0`.**
///
/// The store is made impossible to open — a regular file where `packs/` must
/// be a directory — so `PackStore::open`'s `create_dir_all` refuses, nothing
/// is remembered, and the runner sends all three figures absent. That is
/// P2a's documented third state and the one real binaries can be made to
/// show: a runner with an *empty* store opens and measures it at the dial and
/// honestly sends `0`.
///
/// The runner goes on serving, which is asserted too: an error about the pack
/// store is not a reason to drop a machine out of the fleet.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_whose_pack_store_will_not_open_is_shown_as_having_said_nothing() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;

    let root = herd.runner_root("runner");
    std::fs::create_dir_all(&root).expect("the runner's data root");
    // A file, not a directory. `create_dir_all` of `packs/objects` under it
    // cannot succeed, and the store is never opened.
    std::fs::write(
        root.join(herd_runner::packstore::PACKS_DIR),
        b"not a directory",
    )
    .expect("a pack store that cannot be opened");

    let options: Vec<&str> = RUNNER_OPTIONS.to_vec();
    let _runner = herd.start_runner_with("the-real-runner", "runner", "runner", &options);

    // Waited for on the health reports, not on the pack fields: what is being
    // asserted is that they are *absent*, so the wait has to be on something
    // else that proves the runner is reporting at all.
    let status = until(&herd, "a runner reporting its health", |status| {
        status
            .runners
            .iter()
            .any(|runner| runner.health_reports_seen > 0)
    })
    .await;
    let runner = &status.runners[0];
    assert_eq!(runner.pack_store_bytes, None, "{runner:?}");
    assert_eq!(runner.pack_store_budget_bytes, None, "{runner:?}");
    assert_eq!(runner.packs_held, None, "{runner:?}");

    // Absent from the document, not null: the property P2a proved on the wire,
    // held all the way to the file an operator's renderer reads.
    let json = the_document_as_json(&herd);
    let said = &json["runners"][0];
    for field in ["pack_store_bytes", "pack_store_budget_bytes", "packs_held"] {
        assert!(
            said.get(field).is_none(),
            "{field} must be absent from {STATUS_FILE}, never null: {said}"
        );
    }

    let screen = the_screen(&herd).await;
    let line = the_pack_store_line(&screen);
    assert_eq!(
        line, "    its pack store: it did not say",
        "a runner that said nothing is shown as having said nothing: {screen}"
    );
    assert!(
        !line.contains("0 bytes") && !line.contains("0 pack(s)"),
        "and never as a nought, which is the measurement an empty store \
         makes: {line}"
    );

    // The runner it happened to is still in the fleet and still reporting: an
    // error about a pack store is not a reason to lose a machine.
    assert!(
        screen.contains("runners connected: 1") && screen.contains("the-real-runner"),
        "the machine is still here: {screen}"
    );
    // And it said so once, in its own voice, where an operator of *that*
    // machine would see it — which the hub's screen cannot show.
    assert!(
        herd.said("runner").contains("is NOT announced to the hub"),
        "the runner says on its own stderr that it could not read its store: {}",
        herd.said("runner")
    );
}
