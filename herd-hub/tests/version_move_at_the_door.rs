//! Why `RECORD_VERSION` and `LAYOUT_VERSION` had to move in the same commit,
//! asserted rather than argued (D199).
//!
//! The two checks live in different places and do not behave alike, and that
//! is the whole of it:
//!
//! | | where | what it does |
//! |---|---|---|
//! | `LAYOUT_VERSION` | `JobDirStore::open_with`, reading `store.json` | returns `Err`, which reaches `main()` **before the port is bound** |
//! | `RECORD_VERSION` | `JobDirStore::read_stored`, reading one `job.json` | sets that **one record** aside and serves the rest (D218, V10) |
//!
//! So a build that had moved the record's number **alone** would have started
//! on the consumer's existing root, bound its port, answered a handshake and
//! served an empty queue, with every job on the disk quarantined by name. Loud
//! — the names are printed at startup and counted on the line that says the
//! hub is listening — but **not a refusal**, and an operator could run it like
//! that for a while before understanding that nothing was going to come out of
//! it.
//!
//! The first test below is that half-moved build, simulated exactly: a root
//! whose `store.json` says layout 2 and whose records say record 1. It
//! **passes** — the hub opens — and that is the finding, not a defect. The
//! second test is the pair this commit actually ships: the same records under
//! the layout the old release wrote, refused at the door, by name, with both
//! figures and the instruction.
//!
//! Neither test is about the pack registry. They are about the two numbers,
//! and they go red the moment either one moves without the other.

use std::fs;
use std::path::Path;

use herd_hub::job::JobStore;
use herd_hub::{JobDirStore, LAYOUT_VERSION, RECORD_VERSION};

/// A `job.json` carrying a record version, and the two fields every reader of
/// one expects to find.
fn record(id: &str, version: u32) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "record_version": version,
        "id": id,
        "command": ["true"],
        "labels": [],
        "priority": "step",
        "state": { "type": "queued" },
        "submitted_at": "2026-10-29T00:00:00Z",
        "updated_at": "2026-10-29T00:00:00Z",
        "build_thread_mib": serde_json::Value::Null,
        "memory_mib": serde_json::Value::Null,
        "tmpfs_mib": serde_json::Value::Null,
        "parallelism": serde_json::Value::Null,
        "deadline": serde_json::Value::Null,
        "after": serde_json::Value::Null,
        "requeue_exit_code": serde_json::Value::Null,
        "declaration": serde_json::Value::Null,
        "timeout_s": serde_json::Value::Null,
        "reading": serde_json::Value::Null,
        "metrics": {},
    }))
    .expect("the record serialises")
}

/// A state root somebody else's release wrote: a `store.json` at `layout`, and
/// three queued jobs at `record`.
fn a_root(at: &Path, layout: u32, record_version: u32) {
    fs::create_dir_all(at.join("jobs")).expect("room for the jobs");
    fs::write(
        at.join("store.json"),
        serde_json::to_vec(&serde_json::json!({
            "layout_version": layout,
            "state_root_id": "root-from-another-release",
            "ids_minted": 3,
        }))
        .expect("store.json serialises"),
    )
    .expect("store.json is written");
    for n in 1..=3 {
        let id = format!("j{n:0>10}");
        let dir = at.join("jobs").join(&id);
        fs::create_dir_all(&dir).expect("room for the job");
        fs::write(dir.join("job.json"), record(&id, record_version))
            .expect("the record is written");
    }
}

/// The measurement this commit exists to make unnecessary: with the layout
/// matching, a root of the **old record version** opens, and the hub is
/// serving nothing.
///
/// Read it as the counterfactual it is. It is green on purpose, and what it
/// proves is the cost of the arrangement this commit does **not** ship.
#[test]
fn a_record_only_move_would_have_started_a_hub_on_a_wholly_quarantined_root() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    // The one thing a half-moved build would have had: a layout this hub
    // accepts, over records it cannot read.
    a_root(dir.path(), LAYOUT_VERSION, RECORD_VERSION - 1);

    let store = JobDirStore::open(dir.path()).expect(
        "this is the finding: with the layout matching, the hub OPENS on a root whose every \
         record it cannot read, and nothing stops it binding its port",
    );
    let quarantined = store.quarantined().expect("the quarantine lists");
    assert_eq!(
        quarantined.len(),
        3,
        "every job on the disk is set aside, one at a time: {quarantined:?}"
    );
    for (id, why) in &quarantined {
        assert!(
            why.contains(&format!("record version {}", RECORD_VERSION - 1)),
            "{id}: {why}"
        );
        assert!(
            why.contains(&format!("reads version {RECORD_VERSION}")),
            "{id}: {why}"
        );
    }
    // And the queue is empty, which is the shape an operator would have seen:
    // a hub that answers, and has no work.
    let unfinished = store.unfinished_jobs().expect("the queue reads");
    assert!(
        unfinished.is_empty(),
        "an empty queue off a root holding three queued jobs: {unfinished:?}"
    );
    drop(store);
}

/// What this commit ships instead: the same records, under the layout the
/// release that wrote them used. Refused at the door.
#[test]
fn moving_both_numbers_puts_the_refusal_at_the_door_with_the_instruction() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    // A real root from the release before this one: layout 1, record 1 — the
    // two numbers v0.4.0 and v0.4.1 shipped at.
    a_root(dir.path(), LAYOUT_VERSION - 1, RECORD_VERSION - 1);

    let error = JobDirStore::open(dir.path())
        .expect_err("a root of another layout is refused, and the hub does not start");
    let said = error.to_string();

    // Both figures, so a reader knows which way round it is.
    assert!(
        said.contains(&format!("layout version {}", LAYOUT_VERSION - 1)),
        "{said}"
    );
    assert!(
        said.contains(&format!("writes version {LAYOUT_VERSION}")),
        "{said}"
    );
    assert!(
        said.contains(&dir.path().display().to_string()),
        "and the root it is about: {said}"
    );

    // The irreversible half. Each of these clauses is in the sentence because
    // an operator who does not read it loses something she cannot get back.
    for clause in [
        // there is nothing to wait for and nothing to run
        "no migration tool",
        // the old image is not the way out either
        "equality and not a floor",
        // what crossing it costs
        "every queued job and every uncollected verdict",
        // the step that cannot be taken twice, and in which order
        "take the reading off this root FIRST",
        "with the OLD release's binary",
        "herd-hub report --state-dir",
        // the instruction that is not "convert it"
        "do not convert it and do not delete it",
        "new empty volume",
        // and the thing that will happen next, so it is not read as a fault
        "every runner to be refused once",
        // where the whole of it is written down
        "docs/operating.md",
        "The instruction, in order",
    ] {
        assert!(
            said.contains(clause),
            "the refusal must carry {clause:?}, and says: {said}"
        );
    }

    // And it did not touch the root it refused: `docs/operating.md`'s
    // instruction is that the root is moved aside, not converted, so a hub
    // that refuses one must not have written in it — not even the directory
    // its own layout wants.
    assert!(
        !dir.path().join("packs").exists(),
        "a refused root gains no packs/ directory: the smallest possible conversion of \
         somebody else's state is still a conversion"
    );
    let store_json = fs::read_to_string(dir.path().join("store.json")).expect("still readable");
    assert!(
        store_json.contains(&format!("\"layout_version\":{}", LAYOUT_VERSION - 1)),
        "and store.json still says what it said: {store_json}"
    );
}

/// The numbers themselves, pinned as literals.
///
/// Literals on purpose: written against the constants they would be
/// tautologies and would catch nothing, and what they are here to catch is an
/// *accidental* move of either — which, after this commit, means a move that
/// does not move the other.
#[test]
fn both_numbers_are_where_d199_put_them() {
    assert_eq!(
        RECORD_VERSION, 2,
        "D199: the record gains packs_in and packs_out"
    );
    assert_eq!(
        LAYOUT_VERSION, 2,
        "D199: the state root gains the packs/ directory the registry lives in. If you are here \
         because one of these two is red, the other one has to move in the same commit — see \
         this file's own doc comment for what a half-move does"
    );
    assert_eq!(
        herd_proto::PROTOCOL_VERSION,
        3,
        "and the third of D199's triple, which moved with the pack frames and is not touched here"
    );
}
