//! **What the real `herd-runner` binary says about its pack store, read off
//! the wire** (M12 P2a, E-16).
//!
//! Until this file the three figures were `None` at the build site, so the
//! only thing a test could have asserted about them is that they were absent.
//! They are filled now, and the question this file answers is the one the
//! plan's *Done when* asks: against the **real binaries**, does a runner
//! holding a known store report that store, and does a runner given no
//! `--pack-budget` send `None` for the budget and a real figure for the other
//! two?
//!
//! ## Why the hub here is hand-written
//!
//! The real hub **discards all three fields** — `herd-hub/src/server.rs`
//! binds and drops them in its `ToHub::Health` arm, which is P2b's job to
//! change — so a real hub has nothing to say about them and reading its
//! journal would measure P2b, not this. A peer that decodes the frame is
//! therefore the observation point, and it is the honest one: what P2a owes
//! is a **correct frame leaving the runner**, and that is exactly what is
//! read here. The runner under test is the shipped binary, started as a
//! process, with its own data root and its own `--pack-budget` or none.
//!
//! This is the same division `capabilities.rs`
//! `a_refused_runner_prints_the_reason_before_it_stops` makes, and for the
//! same reason: a frame the real hub cannot be made to produce — or, here,
//! cannot be made to show — is tested from the other end of the socket.
//!
//! ## Why it lives beside the hub's tests and not the runner's
//!
//! It binds a listening socket, and
//! `no_module_of_the_runner_can_bind_a_socket`
//! (`herd-runner/tests/confinement.rs`) forbids the string `TcpListener`
//! anywhere under the `herd-runner` crate, its tests included — invariant 3:
//! a runner dials out and binds nothing. A test standing in for a **hub**
//! belongs here, which is `fake_hub_flood.rs`'s own reasoning.
//!
//! ## What is planted, and what the figures therefore are
//!
//! The store is planted on the disk in its own shape rather than ingested
//! through the store's API, because what is being measured is a runner
//! started on a disk that already holds packs — a runner's second life, which
//! is the ordinary case. `objects/` holds files named by the sha256 of their
//! own contents, which is the name `check_object_digest` accepts and the only
//! name `PackStore` counts; `manifests/<id>/manifest.tsv` is what the store
//! means by holding a pack. So the two figures asserted are the **summed
//! lengths of the planted objects** and the **number of planted manifests**,
//! both known to this file by construction and neither read back out of the
//! code under test.

use std::fs::File;
use std::path::Path;
use std::process::Stdio;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, ToHub};
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Message;

mod real_herd;

use real_herd::{binary, PATIENCE};

/// The budget the operator types in the test that gives one. A round figure
/// in bytes, and deliberately **not** a multiple of anything the store
/// holds: it has to be impossible to mistake for a measurement.
const BUDGET: u64 = 7_000_000_003;

/// What `--pack-budget` is given, in the form an operator types it. The flag
/// takes a size and `7000000003` is the same figure as [`BUDGET`], written
/// the way `herdctl` and the runner's other size flags are written.
const BUDGET_TYPED: &str = "7000000003";

/// How many packs are planted. Small on purpose: the chunking of the
/// *announcement* is `packs_announced.rs`'s measurement and not this file's,
/// and a count this file can hold in one assertion is what it needs.
const PLANTED_PACKS: usize = 3;

/// The lengths of the planted objects, one per pack. Three different
/// lengths, so that a total which happened to be a multiple of the count
/// could not pass for the sum.
const OBJECT_LENGTHS: [usize; PLANTED_PACKS] = [11, 500, 4_097];

/// Plant a store on a runner's disk and come back with what it should be
/// measured as: the summed object lengths and the pack count.
fn plant_a_store(data_root: &Path) -> (u64, u64) {
    let store = data_root.join(herd_runner::packstore::PACKS_DIR);
    let objects = store.join("objects");
    let manifests = store.join("manifests");
    std::fs::create_dir_all(&objects).expect("the store's objects directory");
    std::fs::create_dir_all(&manifests).expect("the store's manifests directory");

    let mut bytes = 0u64;
    for (which, length) in OBJECT_LENGTHS.iter().enumerate() {
        // The content is as long as it says and distinct from its
        // neighbours', so the digest is distinct and the length is the
        // figure this file asserts.
        let content = vec![b'a' + which as u8; *length];
        let digest = herd_proto::sha256_hex(&content);
        std::fs::write(objects.join(&digest), &content).expect("a planted object");
        bytes += content.len() as u64;

        // And one pack naming it. The directory's name **is** the sha256 of
        // the manifest beside it, which is the store's identity rule: a name
        // that did not hold would be refused the moment anything read it
        // back.
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
    (bytes, PLANTED_PACKS as u64)
}

/// The three figures of the **first** health report a real runner sends,
/// with the runner's own startup line beside them.
///
/// One report and not an average of several: the figures are a fact about the
/// store and the first report is where an operator's hub reads them.
async fn the_first_health_report(options: &[&str]) -> (ToHub, String) {
    let data_root = tempfile::tempdir().expect("a temporary data root");
    let (bytes, packs) = plant_a_store(data_root.path());
    assert!(bytes > 0 && packs > 0, "the fixture plants a real store");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free loopback port");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    // A hub that welcomes, answers every report, and hands the first one
    // back. It answers rather than falling silent so that nothing the runner
    // does here is a reaction to a hub that stopped reading.
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
                    keeper_for: Vec::new(),
                })
                .expect("the welcome encodes"),
            ))
            .await
            .expect("the welcome is sent");
        // Everything before the first report — the capabilities, the
        // holdings — is skipped rather than asserted on: what each of those
        // frames says is another file's measurement, and a test that
        // asserted an ordering here would go red the day a frame is added
        // in front of it (ORCH-233's own lesson).
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
            if matches!(message, ToHub::Health { .. }) {
                return message;
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
        .arg("the-pack-holding-runner")
        .arg("--data-root")
        .arg(data_root.path())
        // Short, because what is being waited for is one report and not a
        // cadence; and `--no-detect` because nothing here is about probes.
        .arg("--health-every")
        .arg("0.2")
        .arg("--no-detect")
        .args(options)
        .stdout(Stdio::from(
            File::create(&out).expect("a file for the runner's stdout"),
        ))
        .stderr(Stdio::from(
            File::create(&err).expect("a file for the runner's stderr"),
        ))
        .kill_on_drop(true)
        .spawn()
        .expect("herd-runner starts");

    let report = tokio::time::timeout(PATIENCE, hub).await;
    // Said before any assertion, so that a timeout is diagnosable from the
    // runner's own words rather than from a bare elapsed-time failure.
    let said = format!(
        "{}{}",
        std::fs::read_to_string(&out).unwrap_or_default(),
        std::fs::read_to_string(&err).unwrap_or_default()
    );
    let report = report
        .unwrap_or_else(|_| panic!("a real runner sends a health report. It said:\n{said}"))
        .expect("the hand-written hub ends");

    let _ = child.start_kill();
    // The figures the fixture planted, restated in the panic message of
    // whichever assertion the caller makes, by being in the sentence here.
    assert!(
        said.contains(&format!("in {packs} pack(s)")),
        "the runner's own startup line names the pack count it measured. It said:\n{said}"
    );
    (report, said)
}

/// **A runner holding a known store, with a budget: all three figures, and
/// each of them the fixture's own.**
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runner_holding_a_known_store_reports_all_three_figures() {
    let expected_bytes: u64 = OBJECT_LENGTHS.iter().map(|length| *length as u64).sum();
    let (report, said) = the_first_health_report(&["--pack-budget", BUDGET_TYPED]).await;
    let ToHub::Health {
        pack_store_bytes,
        pack_store_budget_bytes,
        packs_held,
        ..
    } = report
    else {
        panic!("the frame waited for is a health report");
    };
    assert_eq!(
        pack_store_bytes,
        Some(expected_bytes),
        "the bytes are the planted objects' summed lengths, not a round number and not \
         absent. The runner said:\n{said}"
    );
    assert_eq!(
        packs_held,
        Some(PLANTED_PACKS as u32),
        "the count is the planted manifests'. The runner said:\n{said}"
    );
    assert_eq!(
        pack_store_budget_bytes,
        Some(BUDGET),
        "the budget is the one the operator typed, in bytes. The runner said:\n{said}"
    );
}

/// **The same runner with no `--pack-budget`: `None` for the budget, and a
/// real figure for the other two.**
///
/// The clause the plan writes out, and the one a default would have made
/// unmeetable. Absent is not nought here and it is not "unlimited" either: it
/// is a hub with nothing to weigh the usage against, which is why the other
/// two must still arrive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runner_with_no_pack_budget_still_says_what_it_holds() {
    let expected_bytes: u64 = OBJECT_LENGTHS.iter().map(|length| *length as u64).sum();
    let (report, said) = the_first_health_report(&[]).await;
    let ToHub::Health {
        pack_store_bytes,
        pack_store_budget_bytes,
        packs_held,
        ..
    } = report
    else {
        panic!("the frame waited for is a health report");
    };
    assert_eq!(
        pack_store_budget_bytes, None,
        "nobody said what this store may hold, so the report says nothing — never nought, \
         which is a store that may hold nothing. The runner said:\n{said}"
    );
    assert_eq!(
        pack_store_bytes,
        Some(expected_bytes),
        "and what it holds is a measurement, which it still makes. The runner said:\n{said}"
    );
    assert_eq!(
        packs_held,
        Some(PLANTED_PACKS as u32),
        "both of them. The runner said:\n{said}"
    );
}
