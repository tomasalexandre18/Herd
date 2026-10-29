//! The hub's pack registry, through a real state root (E-12, E-17).
//!
//! `herd-hub/src/registry.rs` holds the unit tests of the registry's own
//! refusals. This file is the part that only a whole state root can say: that
//! `packs/` is a directory of the root the store opens and the store writes,
//! that the registry comes back when the store does, that the holdings do not,
//! and that the shape on the disk is the shape `LAYOUT_VERSION` 2 promises.
//!
//! **Nothing here goes through a frame.** The pack frames are numbered in
//! `herd_proto::pack` and are not wired into the hub's connection loop; that
//! is a separate step, which owns `server.rs`. These tests call the registry
//! directly, which is exactly as far as this step goes.

use std::fs;

use herd_hub::job::JobStore;
use herd_hub::registry::{Registered, Registration, Resolved};
use herd_hub::{JobDirStore, Timestamp, LAYOUT_VERSION};
use herd_proto::pack::{Holder, PackId, PackState};

/// The fixture manifest, with the two rows that make it prove anything.
///
/// Four rows: two `kind f`, one `kind d` and one `kind l`. The `d` and `l`
/// rows each carry a sha256 of something that is **not** a file's bytes, so a
/// walk of a manifest that does not filter on `kind == f` counts them as
/// objects and a completeness check built on that count looks them up under
/// `objects/` and calls a perfectly good pack incomplete. A fixture of `f`
/// rows alone proves nothing about the filter.
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

fn a_registration() -> Registration {
    let manifest = a_manifest();
    Registration {
        commit: "4f1b2c".to_owned(),
        key: "pack".to_owned(),
        announced: PackId::of_manifest_bytes(&manifest),
        manifest,
        meta: Some(b"commit 4f1b2c\nprofile debug\nscenarios 12\n".to_vec()),
        runner: "tower".to_owned(),
        job: "j0000000001".to_owned(),
        attempt: 1,
        at: Timestamp::at_unix_seconds(1_793_145_600),
    }
}

/// `packs/` is a directory of the root the store opens, in the shape
/// `LAYOUT_VERSION` 2 describes — which is the thing a version number is for.
#[test]
fn the_state_root_gains_the_directory_the_layout_version_describes() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");

    assert_eq!(LAYOUT_VERSION, 2, "and this is the number it is 2 for");
    for expected in ["packs", "packs/manifests", "packs/manifests.part"] {
        assert!(
            dir.path().join(expected).is_dir(),
            "{expected} is a directory of the root"
        );
    }
    let store_json = fs::read_to_string(dir.path().join("store.json")).expect("store.json reads");
    assert!(
        store_json.contains(&format!("\"layout_version\":{LAYOUT_VERSION}")),
        "and the root says which layout it is: {store_json}"
    );

    let pack = match store
        .packs()
        .register(a_registration())
        .expect("registered")
    {
        Registered::Fresh(pack) => pack,
        other => panic!("{other:?}"),
    };
    let at = dir.path().join("packs/manifests").join(pack.as_str());
    assert_eq!(
        fs::read(at.join("manifest.tsv")).expect("the manifest is there"),
        a_manifest(),
        "the manifest's bytes are on the disk verbatim, because the id is their sha256"
    );
    assert_eq!(
        fs::read(at.join("pack.meta")).expect("the meta is there"),
        b"commit 4f1b2c\nprofile debug\nscenarios 12\n",
    );
    let registered: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(at.join("registered.json")).expect("reads"))
            .expect("registered.json parses");
    assert_eq!(registered["record_version"], herd_hub::RECORD_VERSION);
    assert_eq!(registered["pack"], pack.as_str());
    assert_eq!(registered["commit"], "4f1b2c");
    assert_eq!(registered["key"], "pack");
    assert_eq!(registered["runner"], "tower");
    assert_eq!(registered["job"], "j0000000001");
    assert_eq!(
        registered["objects"], 2,
        "two `kind f` rows out of four: the `d` and `l` rows carry a sha256 of something that is \
         not a file's bytes, and counting them would make a good pack's object list longer than \
         it is"
    );
    assert_eq!(registered["object_bytes"], 4096 + 8192);

    // And the hub holds not one object byte of the pack (D10, invariant 3's
    // neighbour): the whole of `packs/` is the manifest, the meta and that
    // one small JSON.
    let weight: u64 = walk(&dir.path().join("packs"));
    assert!(
        weight < 4096,
        "the hub carries a pack's metadata and never its bytes; packs/ weighs {weight} bytes"
    );
    drop(store);
}

/// The registry is on the disk and the holdings are not.
#[test]
fn the_registry_survives_a_restart_and_the_holdings_do_not() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let pack = match store
        .packs()
        .register(a_registration())
        .expect("registered")
    {
        Registered::Fresh(pack) => pack,
        other => panic!("{other:?}"),
    };
    store.packs().announced(
        "tower",
        Some("100.64.0.9:7777".to_owned()),
        vec![pack.clone()],
        false,
    );
    // An outbound-only holder beside it: a runner with no endpoint is not a
    // degraded holder, it is why D10 has a push path.
    store
        .packs()
        .announced("laptop", None, vec![pack.clone()], false);
    assert_eq!(
        store.packs().holders_of(&pack),
        vec![
            Holder {
                runner: "laptop".to_owned(),
                endpoint: None
            },
            Holder {
                runner: "tower".to_owned(),
                endpoint: Some("100.64.0.9:7777".to_owned())
            },
        ]
    );
    assert_eq!(
        store.packs().state_of("4f1b2c", "pack").expect("a state"),
        Some((pack.clone(), PackState::Registered))
    );
    drop(store);

    let store = JobDirStore::open(dir.path()).expect("the state root opens again");
    let resolved = store.packs().resolve("4f1b2c", "pack").expect("resolves");
    let Resolved::Pack(assigned) = resolved else {
        panic!("{resolved:?}");
    };
    assert_eq!(assigned.pack, pack, "the registry came back");
    assert_eq!(
        assigned.manifest,
        a_manifest(),
        "with the manifest's bytes verbatim"
    );
    assert_eq!(
        PackId::of_manifest_bytes(&assigned.manifest),
        pack,
        "which still hash to the id, because nothing reprinted them"
    );
    assert!(
        assigned.holders.is_empty(),
        "and no holder: holdings are volatile and re-announced after Welcome, so a hub that has \
         just restarted offers no holder that may not be there"
    );
    assert_eq!(
        store.packs().state_of("4f1b2c", "pack").expect("a state"),
        Some((pack, PackState::NoHolder)),
        "which is a named state and not a silence"
    );
    assert!(
        store.packs().holdings().is_empty(),
        "there is no holdings file to have read"
    );
    assert!(
        !dir.path().join("packs/holdings.json").exists(),
        "and none was ever written"
    );
    drop(store);
}

/// A `(commit, key)` the disk holds two ids for is refused by name, at the
/// resolve and at the next registration, and never a newest-wins.
#[test]
fn two_ids_for_one_commit_and_key_are_refused_by_name_through_the_store() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let first = match store
        .packs()
        .register(a_registration())
        .expect("registered")
    {
        Registered::Fresh(pack) => pack,
        other => panic!("{other:?}"),
    };

    // The same (commit, key), a different manifest — and therefore a
    // different pack, since the id is the manifest's sha256. The registry
    // refuses this through its own API, so the only way a root holds it is
    // somebody else having written it: that is the case this test builds.
    let mut other = a_manifest();
    other
        .extend_from_slice(format!("{}\t64\t0644\tf\tbin\tbin/extra\n", "5".repeat(64)).as_bytes());
    let second = PackId::of_manifest_bytes(&other);
    let at = dir.path().join("packs/manifests").join(second.as_str());
    fs::create_dir_all(&at).expect("made");
    fs::write(at.join("manifest.tsv"), &other).expect("written");
    fs::write(
        at.join("registered.json"),
        serde_json::to_vec(&serde_json::json!({
            "record_version": herd_hub::RECORD_VERSION,
            "pack": second.as_str(),
            "commit": "4f1b2c",
            "key": "pack",
            "runner": "elsewhere",
            "job": "j0000000002",
            "attempt": 1,
            "registered_at": "2026-10-29T00:00:00Z",
            "manifest_bytes": other.len(),
            "objects": 3,
            "object_bytes": 12352,
        }))
        .expect("serialises"),
    )
    .expect("written");
    drop(store);

    let store = JobDirStore::open(dir.path()).expect(
        "the root still opens: this is one key's \
                                                       problem and not the root's",
    );
    let resolved = store.packs().resolve("4f1b2c", "pack").expect("answered");
    let Resolved::Refused(why) = resolved else {
        panic!("two ids is a refusal, not a pick: {resolved:?}");
    };
    assert!(why.contains(first.as_str()), "{why}");
    assert!(why.contains(second.as_str()), "{why}");
    assert!(why.contains("refuses to pick one"), "{why}");
    assert!(
        why.contains("identical outputs give the identical pack"),
        "the refusal says why two ids cannot be one pack: {why}"
    );
    assert!(
        why.contains("hand some later job the wrong artefact"),
        "and what picking one would cost: {why}"
    );
    // A third registration for the same key is refused too.
    let outcome = store.packs().register(a_registration()).expect("answered");
    assert!(
        outcome.refusal().is_some(),
        "not even the id that was there first wins: {outcome:?}"
    );
    assert_eq!(store.packs().conflicts().len(), 1);
    // And the root is still serving jobs.
    let submission = store
        .submit(herd_hub::NewJob::new(vec!["true".to_owned()]))
        .expect("a job is still minted");
    assert!(store.job(&submission.record().id).expect("reads").is_some());
    drop(store);
}

/// An interrupted registration leaves nothing a reader can see as a pack, and
/// the next start removes it by name.
///
/// Built the way a crash builds it: the files a registration writes, in the
/// directory it writes them in, with the one `rename` that publishes them
/// never having happened.
#[test]
fn an_interrupted_registration_is_not_readable_and_is_cleared_at_the_next_start() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let pack = a_registration().announced;
    drop(store);

    let staged = dir
        .path()
        .join("packs/manifests.part")
        .join(format!("4242-{pack}-0"));
    fs::create_dir_all(&staged).expect("made");
    fs::write(staged.join("manifest.tsv"), a_manifest()).expect("written");
    fs::write(staged.join("pack.meta"), b"commit 4f1b2c\n").expect("written");
    fs::write(
        staged.join("registered.json"),
        serde_json::to_vec(&serde_json::json!({
            "record_version": herd_hub::RECORD_VERSION,
            "pack": pack.as_str(),
            "commit": "4f1b2c",
            "key": "pack",
            "runner": "tower",
            "job": "j0000000001",
            "attempt": 1,
            "registered_at": "2026-10-29T00:00:00Z",
            "manifest_bytes": a_manifest().len(),
            "objects": 2,
            "object_bytes": 12288,
        }))
        .expect("serialises"),
    )
    .expect("written");

    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    assert_eq!(
        store.packs().resolve("4f1b2c", "pack").expect("answered"),
        Resolved::Nothing,
        "an interrupted registration resolves to nothing at all: not a pack, not a refusal, and \
         not a half-pack"
    );
    assert!(
        store.packs().manifest_bytes(&pack).expect("read").is_none(),
        "and the id finds no manifest"
    );
    assert!(store.packs().entries().is_empty());
    assert!(store.packs().quarantined().is_empty());
    assert!(
        !staged.exists(),
        "the residue is removed, because nothing of it was ever readable: {}",
        staged.display()
    );

    // The control, which is what makes every assertion above about the
    // interruption rather than about a registry that never works: the same
    // registration, published, is readable at once.
    let outcome = store
        .packs()
        .register(a_registration())
        .expect("registered");
    assert_eq!(outcome, Registered::Fresh(pack.clone()));
    assert!(matches!(
        store.packs().resolve("4f1b2c", "pack").expect("answered"),
        Resolved::Pack(_)
    ));
    assert_eq!(
        store
            .packs()
            .manifest_bytes(&pack)
            .expect("read")
            .as_deref(),
        Some(&a_manifest()[..])
    );
    drop(store);
}

/// Every byte under a directory, counting nothing that is not a regular file.
fn walk(at: &std::path::Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = fs::read_dir(at) else {
        return 0;
    };
    for entry in entries.flatten() {
        let kind = entry.file_type().expect("a file type");
        if kind.is_dir() {
            total += walk(&entry.path());
        } else if kind.is_file() {
            total += entry.metadata().expect("a size").len();
        }
    }
    total
}
