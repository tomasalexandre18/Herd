//! The pack half's wire surface, at [`PROTOCOL_VERSION`] 3.
//!
//! Every type and every frame payload M11's pack half and M12's pack-side
//! ride-along carry (D199's one bump), asserted from **outside** the crate:
//! each one round-trips, the two version refusals say what they cost, the id
//! is computed over verbatim bytes, and a pack frame this build has never
//! heard of reaches the catch-all rather than killing a link.
//!
//! ## The stand-in enums, and why they are still here
//!
//! When these payloads were frozen they were **not yet arms of [`ToHub`] and
//! [`FromHub`]**: adding a variant to either breaks
//! `herd-hub/src/server.rs`'s exhaustive `message_kind` match and every
//! construction site in `herd-runner/src/lib.rs`, `herdctl/src/lib.rs` and
//! the hub's own test files, none of which that step owned. So the wire shape
//! was pinned through stand-in enums declared here, on the argument that an
//! internally tagged enum with a newtype variant whose inner type is a struct
//! serialises to exactly the same bytes as a struct variant carrying those
//! fields.
//!
//! **The variants have landed, and the stand-ins are kept so the argument is
//! checked rather than believed.** Every assertion frozen against them still
//! runs, and `the_bytes_are_the_same_on_the_real_enum` now encodes the real
//! arm beside the stand-in's and compares the two byte strings — once per
//! payload per case, every absent optional included. If one byte ever
//! differs, the stand-in was wrong and everything frozen through it is
//! suspect, which is why the stand-in is not deleted now that it is
//! redundant: a frozen shape with nothing comparing it to the real one is a
//! comment.

use herd_proto::pack::{
    check_peer_protocol_version, decode_peer_header, encode_peer_header,
    peer_version_mismatch_reason, take_peer_body, AskPack, AskTransferCapability, AssignedPack,
    ExpectTransfer, FetchCapability, FetchPath, Holder, ManifestKind, PackHolders, PackId,
    PackManifest, PackRefused, PackRegistered, PackState, PackTransferred, PackUnavailable,
    PacksHeld, PeerFramingRefusal, PeerHeaderRead, PeerRefusal, PeerReply, PeerRequest, PinPacks,
    ReadPacksAgain, RegisterPack, TransferCapability, TransferDirection, CHATONS_PACK_FORMAT,
    MANIFEST_MAX_BYTES, PACK_CAPABILITY_LIFETIME, PACK_CHUNK_BYTES, PACK_ENDPOINT_READ_BYTES,
};
use herd_proto::{
    frame_type, from_frame, to_frame, version_mismatch_reason, FromHub, JobId, ToHub,
    FRAME_CEILING, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The fixture: a manifest in the consumer's own shape, with a `d` row and an
// `l` row
// ---------------------------------------------------------------------------

/// A `manifest.tsv` as the consumer's tool writes one (issue #16, measured
/// 2026-10-13), carrying **two `f` rows, one `d` row and one `l` row**.
///
/// The `d` and the `l` are the point. Both carry a sha256 column, of
/// something that is not a file's bytes, so a completeness check that forgot
/// to filter on `kind == f` would look them up under `objects/`, find
/// nothing, and report a perfectly good pack as incomplete. **A fixture of
/// `f` rows alone would let that code pass**, which is why this one is not
/// one.
fn a_manifest() -> Vec<u8> {
    [
        CHATONS_PACK_FORMAT,
        "# profile debug",
        "1111111111111111111111111111111111111111111111111111111111111111\t12\t0755\tf\tbin\tbin/tool",
        "2222222222222222222222222222222222222222222222222222222222222222\t4096\t0755\td\tbin\tbin",
        "3333333333333333333333333333333333333333333333333333333333333333\t7\t0777\tl\tbin\tbin/tool-latest\tbin/tool",
        "4444444444444444444444444444444444444444444444444444444444444444\t900\t0644\tf\tdata\tshare/table.bin",
    ]
    .join("\n")
    .into_bytes()
}

fn a_pack() -> PackId {
    PackId::of_manifest_bytes(&a_manifest())
}

fn a_job() -> JobId {
    JobId::new("j0000000042")
}

// ---------------------------------------------------------------------------
// 1. The identity rule
// ---------------------------------------------------------------------------

/// **`<id>` is the sha256 of `manifest.tsv`'s bytes**, computed over the bytes
/// and over nothing derived from them (issue #16).
#[test]
fn a_pack_id_is_the_sha256_of_the_manifest_bytes_verbatim() {
    let bytes = a_manifest();
    let manifest = PackManifest::parse(bytes.clone()).expect("the fixture parses");
    assert_eq!(
        manifest.bytes(),
        bytes.as_slice(),
        "the bytes come back exactly as they went in, or the pack is a different pack"
    );
    assert_eq!(manifest.id(), &PackId::of_manifest_bytes(&bytes));
    assert_eq!(
        manifest.id().as_str(),
        &herd_proto::sha256_hex(&bytes),
        "the same digest function the rest of the protocol uses (invariant 4)"
    );
    assert_eq!(manifest.id().as_str().len(), 64, "lowercase hex sha256");
}

/// The cost of "verbatim", asserted: two manifests with the same rows and one
/// byte of difference are **two packs**.
///
/// This is the assertion that makes the no-re-serialisation rule a rule. A
/// parse cannot tell these apart, so any code path that reprinted a manifest
/// would mint an id for a pack nobody holds — and the only defence is that
/// there is no such path.
#[test]
fn one_byte_of_difference_is_a_different_pack() {
    let plain = PackManifest::parse(a_manifest()).expect("parses");
    let mut with_newline = a_manifest();
    with_newline.push(b'\n');
    let padded = PackManifest::parse(with_newline).expect("parses");
    assert_eq!(
        plain.entries().len(),
        padded.entries().len(),
        "the same rows: a parse cannot tell them apart"
    );
    assert_ne!(plain.id(), padded.id(), "and yet two packs, by one byte");
}

/// Only the `kind f` rows name objects — the filter the `d` and `l` rows are
/// in the fixture to prove.
#[test]
fn only_the_kind_f_rows_name_an_object() {
    let manifest = PackManifest::parse(a_manifest()).expect("parses");
    assert_eq!(
        manifest.entries().len(),
        4,
        "two files, a directory, a link"
    );
    assert_eq!(
        manifest
            .entries()
            .iter()
            .filter(|entry| entry.kind == ManifestKind::Dir)
            .count(),
        1,
        "the fixture must carry a d row or it proves nothing"
    );
    assert_eq!(
        manifest
            .entries()
            .iter()
            .filter(|entry| entry.kind == ManifestKind::Link)
            .count(),
        1,
        "and an l row"
    );

    let needed = manifest.objects_needed();
    assert_eq!(needed.len(), 2, "only the two f rows: {needed:?}");
    assert_eq!(needed.get(&"1".repeat(64)), Some(&12));
    assert_eq!(needed.get(&"4".repeat(64)), Some(&900));
    assert!(
        !needed.contains_key(&"2".repeat(64)),
        "a directory's sha256 is not a file's bytes, and looking it up under objects/ would \
         report a good pack as incomplete"
    );
    assert!(!needed.contains_key(&"3".repeat(64)), "nor is a symlink's");
    assert_eq!(
        manifest.headers(),
        ["profile debug"],
        "carried, never interpreted"
    );
}

/// The hub never takes a pack's identity on a runner's word.
#[test]
fn an_announced_id_is_checked_against_the_bytes_and_names_both() {
    let manifest = PackManifest::parse(a_manifest()).expect("parses");
    assert_eq!(
        manifest.disagreement_with(manifest.id()),
        None,
        "the id the hub computes is the id the pack has"
    );
    let said = manifest
        .disagreement_with(&PackId::as_announced("0".repeat(64)))
        .expect("a mismatch is named, not guessed between");
    assert!(said.contains(manifest.id().as_str()), "{said}");
    assert!(said.contains(&"0".repeat(64)), "{said}");
    assert!(
        said.contains("two different packs"),
        "the refusal has to say what the mismatch means: {said}"
    );
}

/// A manifest too large for a frame is **refused whole**, never truncated.
#[test]
fn a_manifest_past_the_cap_is_refused_rather_than_truncated() {
    let mut bytes = a_manifest();
    bytes.resize(MANIFEST_MAX_BYTES as usize + 1, b'\n');
    let said = PackManifest::parse(bytes).expect_err("past the cap");
    assert!(said.contains("truncated"), "{said}");
}

/// The format header is the consumer's, read and **refused by name** rather
/// than interpreted (invariant 1).
#[test]
fn a_pack_format_herd_was_not_taught_is_refused_naming_both() {
    let said = PackManifest::parse(b"# chatons pack 2\n".to_vec()).expect_err("refused");
    assert!(said.contains("chatons pack 2"), "{said}");
    assert!(said.contains(CHATONS_PACK_FORMAT), "{said}");
}

// ---------------------------------------------------------------------------
// 2. The constants
// ---------------------------------------------------------------------------

/// [`PACK_CHUNK_BYTES`] is derived from the endpoint's read buffer, **not**
/// from [`FRAME_CEILING`] — which D148 forbids moving and which the peer leg
/// does not use at all.
#[test]
fn the_pack_chunk_is_derived_from_the_endpoint_and_not_from_the_frame_ceiling() {
    assert_eq!(PACK_CHUNK_BYTES, 8 * PACK_ENDPOINT_READ_BYTES);
    assert_eq!(PACK_ENDPOINT_READ_BYTES, 128 << 10);
    const {
        assert!(
            PACK_CHUNK_BYTES < FRAME_CEILING,
            "and it is not the ceiling, nor a fraction anybody computed from it"
        )
    };
}

/// A capability has to outlive a whole pack, or a slow link becomes a refusal
/// nobody can explain.
#[test]
fn a_capability_outlives_the_consumers_measured_pack() {
    // Their figure, 2026-10-13: 401.6 MiB in 557 files.
    let measured_bytes = 401.6_f64 * 1024.0 * 1024.0;
    let seconds = PACK_CAPABILITY_LIFETIME.as_secs_f64();
    let floor = measured_bytes / seconds;
    assert!(
        floor < 2.0 * 1024.0 * 1024.0,
        "the lifetime implies a sustained floor of {floor:.0} B/s, which is faster than two \
         machines of one fleet can be relied on to be"
    );
    assert!(
        PACK_CAPABILITY_LIFETIME > herd_proto::HEALTH_EVERY,
        "and a transfer must never have to race a health report"
    );
}

/// [`MANIFEST_MAX_BYTES`] is a bound read twice: against the frame and
/// against the consumer's own pack.
#[test]
fn the_manifest_cap_fits_a_frame_and_dwarfs_the_measured_pack() {
    // Base64 is four thirds, and `pack.meta` and the frame ride beside it.
    const {
        assert!(
            MANIFEST_MAX_BYTES * 4 / 3 < FRAME_CEILING / 2,
            "a manifest at the cap must leave most of the frame for everything else"
        )
    };
    // Their 557 entries are about 42 KiB, some 77 bytes an entry.
    const {
        assert!(
            MANIFEST_MAX_BYTES / 77 > 100_000,
            "room for a hundred thousand entries against the 557 they measured"
        )
    };
}

// ---------------------------------------------------------------------------
// 3. The two version refusals
// ---------------------------------------------------------------------------

/// The control-plane refusal names **3**, names the peer's number, and says
/// what the mismatch costs — because the person reading it is the one who can
/// act on it.
#[test]
fn the_version_refusal_names_three_and_the_peers_and_says_what_it_costs() {
    assert_eq!(PROTOCOL_VERSION, 3, "D199's one bump, for M11's pack half");
    let said = version_mismatch_reason(2);
    assert!(said.contains("version 3"), "it names the hub's: {said}");
    assert!(said.contains("version 2"), "and the peer's: {said}");
    assert!(
        said.contains("cannot work at all"),
        "a refused peer is not degraded, it is stopped: {said}"
    );
    assert!(
        said.contains("machine by machine"),
        "the fleet cost, which is the part a pair of numbers cannot say: {said}"
    );
    assert!(
        said.contains("together"),
        "equality and not a floor, so hub and peers move together: {said}"
    );
    assert!(
        said.contains("roll the hub back"),
        "and either direction works, which an operator has to be told: {said}"
    );

    // A peer *newer* than the hub reads the same sentence, because the test
    // is equality: nothing here is a floor.
    let newer = version_mismatch_reason(PROTOCOL_VERSION + 1);
    assert!(newer.contains("version 4"), "{newer}");
    assert!(newer.contains("cannot work at all"), "{newer}");
}

/// The peer wire has **no version of its own** and refuses by name at the
/// hub's number.
#[test]
fn the_peer_refusal_names_the_same_number_and_sends_the_operator_to_the_handshake() {
    let said = peer_version_mismatch_reason(2);
    assert!(said.contains("version 3"), "{said}");
    assert!(said.contains("version 2"), "{said}");
    assert!(
        said.contains("no version of its own"),
        "one version, not two: {said}"
    );
    assert!(
        said.contains("handshake"),
        "a peer refused here will be refused at the hub too, and saying so saves a second \
         investigation: {said}"
    );
}

// ---------------------------------------------------------------------------
// 4. An unknown pack frame reaches the catch-all, in all three directions
// ---------------------------------------------------------------------------

/// A pack frame of a type this build has never heard of **decodes**, into the
/// catch-all, in each of the three directions D148 numbered — and the tag
/// comes back off the text so the log line names it.
///
/// Three readings of one catch-all, and the behaviour behind each of them
/// lives in a crate this step does not own: the **hub** refuses the peer by
/// name, a **runner** shrugs, a **requester** ends its run (D124, D128). What
/// `herd-proto` owns is that all three *decode* rather than failing to, which
/// is the property the bump to 2 bought and the reason a pack frame added
/// after this one is not an outage.
#[test]
fn an_unknown_pack_frame_reaches_the_catch_all_in_all_three_directions() {
    // (1) runner → hub: a pack frame a future Herd sends.
    let to_hub = r#"{"type":"pack_rehomed","pack":"abc","peer":"wsl-pc"}"#;
    assert_eq!(
        from_frame::<ToHub>(to_hub).expect("an unknown ToHub decodes rather than erroring"),
        ToHub::Unknown,
        "the hub reads it, and refuses the peer by name rather than going silent"
    );
    assert_eq!(
        frame_type(to_hub).as_deref(),
        Some("pack_rehomed"),
        "the tag is read back off the text, or the refusal cannot name the frame"
    );

    // (2) hub → runner, and (3) hub → requester: one frame, one decode, two
    // behaviours that are not this crate's.
    let from_hub = r#"{"type":"pack_rehomed","pack":"abc","keeper":"wsl-pc"}"#;
    let decoded: FromHub =
        from_frame(from_hub).expect("an unknown FromHub decodes rather than erroring");
    assert_eq!(
        decoded,
        FromHub::Unknown,
        "a runner shrugs at it and a requester ends its run, and neither takes the decode-error \
         path that D128 recorded as a redial loop"
    );
    assert_eq!(frame_type(from_hub).as_deref(), Some("pack_rehomed"));
}

/// **The one-line defect this step was sent to fix.**
///
/// `FromHub::Welcome::version_note` was the only `Option` on either frame
/// enum without `#[serde(default)]`, so a `Welcome` with no `version_note`
/// key was a serde **missing-field error** and not a readable frame. That
/// error is outside the catch-all: it kills the link, the runner redials, and
/// it reads the same undecodable frame again — D128's loop, on the one frame
/// a runner cannot get past.
#[test]
fn a_welcome_without_a_version_note_decodes() {
    let welcome = format!(
        r#"{{"type":"welcome","protocol_version":{PROTOCOL_VERSION},"hub_version":"0.4.1",
        "state_root":"root-abc"}}"#
    );
    let decoded: FromHub = from_frame(&welcome).expect(
        "a welcome missing that key must decode; a missing-field error here is D128's redial loop",
    );
    match decoded {
        FromHub::Welcome {
            version_note,
            state_root,
            ..
        } => {
            assert_eq!(version_note, None, "absent means the two builds agree");
            assert_eq!(state_root, "root-abc", "and the rest of the frame is read");
        }
        other => panic!("that is a welcome: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5. The frame payloads, round-tripped at their final wire shape
// ---------------------------------------------------------------------------

/// The six new `ToHub` pack frames, in the shape they will have as arms of
/// [`ToHub`].
///
/// An internally tagged enum with a newtype variant whose inner type is a
/// struct serialises to the same bytes as a struct variant carrying those
/// fields, so what is asserted here is the wire and not the stand-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PackToHub {
    PacksHeld(PacksHeld),
    RegisterPack(RegisterPack),
    AskPack(AskPack),
    AskTransferCapability(AskTransferCapability),
    PackTransferred(PackTransferred),
    PackUnavailable(PackUnavailable),
    #[serde(other)]
    Unknown,
}

/// The seven new `FromHub` pack frames, likewise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PackFromHub {
    PackHolders(PackHolders),
    TransferCapability(TransferCapability),
    ExpectTransfer(ExpectTransfer),
    PackRegistered(PackRegistered),
    PackRefused(PackRefused),
    ReadPacksAgain(ReadPacksAgain),
    PinPacks(PinPacks),
    #[serde(other)]
    Unknown,
}

/// The same payload, as the arm of the **real** [`ToHub`] it is now.
///
/// `None` only for the catch-all, which has no payload to carry.
fn the_real_to_hub(message: &PackToHub) -> Option<ToHub> {
    Some(match message.clone() {
        PackToHub::PacksHeld(payload) => ToHub::PacksHeld(payload),
        PackToHub::RegisterPack(payload) => ToHub::RegisterPack(payload),
        PackToHub::AskPack(payload) => ToHub::AskPack(payload),
        PackToHub::AskTransferCapability(payload) => ToHub::AskTransferCapability(payload),
        PackToHub::PackTransferred(payload) => ToHub::PackTransferred(payload),
        PackToHub::PackUnavailable(payload) => ToHub::PackUnavailable(payload),
        PackToHub::Unknown => return None,
    })
}

/// The same, from the hub.
fn the_real_from_hub(message: &PackFromHub) -> Option<FromHub> {
    Some(match message.clone() {
        PackFromHub::PackHolders(payload) => FromHub::PackHolders(payload),
        PackFromHub::TransferCapability(payload) => FromHub::TransferCapability(payload),
        PackFromHub::ExpectTransfer(payload) => FromHub::ExpectTransfer(payload),
        PackFromHub::PackRegistered(payload) => FromHub::PackRegistered(payload),
        PackFromHub::PackRefused(payload) => FromHub::PackRefused(payload),
        PackFromHub::ReadPacksAgain(payload) => FromHub::ReadPacksAgain(payload),
        PackFromHub::PinPacks(payload) => FromHub::PinPacks(payload),
        PackFromHub::Unknown => return None,
    })
}

fn round_trip_to_hub(message: PackToHub, tag: &str) -> String {
    let wire = to_frame(&message).expect("a pack frame encodes");
    assert!(
        wire.contains(&format!("\"type\":\"{tag}\"")),
        "the tag is the frame's name on the wire: {wire}"
    );
    assert_eq!(
        from_frame::<PackToHub>(&wire).expect("and decodes"),
        message,
        "a field that silently fails to round-trip is a lost field: {wire}"
    );
    the_bytes_are_the_same_on_the_real_enum(&wire, the_real_to_hub(&message));
    wire
}

fn round_trip_from_hub(message: PackFromHub, tag: &str) -> String {
    let wire = to_frame(&message).expect("a pack frame encodes");
    assert!(
        wire.contains(&format!("\"type\":\"{tag}\"")),
        "the tag is the frame's name on the wire: {wire}"
    );
    assert_eq!(
        from_frame::<PackFromHub>(&wire).expect("and decodes"),
        message,
        "a field that silently fails to round-trip is a lost field: {wire}"
    );
    the_bytes_are_the_same_on_the_real_enum(&wire, the_real_from_hub(&message));
    wire
}

/// **The assertion that makes the stand-in a proof rather than a hope.**
///
/// Every payload in this file was frozen through the stand-in enums above
/// *before* the real [`ToHub`] and [`FromHub`] carried it, on the argument
/// that an internally tagged enum's newtype variant serialises to exactly
/// the same bytes as a struct variant carrying the same fields. The variants
/// have landed, so the argument is now checkable: the real arm is encoded
/// beside the stand-in's and the two byte strings are compared.
///
/// It runs inside the two round-trip helpers, so it fires once per payload
/// per case rather than once per enum — thirteen frames, each in every shape
/// any test in this file gives it, including every absent optional. A single
/// byte of difference anywhere fails here and names both strings.
fn the_bytes_are_the_same_on_the_real_enum<M>(stand_in_wire: &str, real: Option<M>)
where
    M: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
{
    let Some(real) = real else {
        return;
    };
    let real_wire = to_frame(&real).expect("the real arm encodes");
    assert_eq!(
        real_wire, stand_in_wire,
        "the stand-in froze a wire shape the real enum does not emit; the stand-in was wrong \
         and every byte frozen through it is suspect"
    );
    assert_eq!(
        from_frame::<M>(stand_in_wire).expect("the real enum decodes the stand-in's own text"),
        real,
        "the bytes agree and the decode does not, which is a field read into the wrong place"
    );
}

/// `more` is **required**, and it is the one field in this surface that could
/// not have been added later: a holding too large for one frame has to be
/// chunkable the day the frame is numbered, or an older hub reads the first
/// frame as the whole of it — and since an announcement **replaces** the
/// hub's view rather than adding to it, reading a fragment as the whole would
/// drop every pack the runner really holds.
///
/// The replace rule is also how a holding is **retracted**: a runner that has
/// deleted a pack announces what it holds now, without it. An empty
/// announcement with `more: false` is therefore a real statement — "I hold
/// none" — and not an absence, which is why the flag is on the wire even when
/// it is false.
#[test]
fn packs_held_round_trips_and_carries_its_continuation_flag() {
    let wire = round_trip_to_hub(
        PackToHub::PacksHeld(PacksHeld {
            packs: vec![a_pack(), PackId::as_announced("b".repeat(64))],
            more: true,
        }),
        "packs_held",
    );
    assert!(wire.contains(r#""more":true"#), "{wire}");

    // And `false` is on the wire too: an announcement that is complete says
    // so, rather than leaving "and that is all of them" to an absence.
    let last = round_trip_to_hub(
        PackToHub::PacksHeld(PacksHeld {
            packs: Vec::new(),
            more: false,
        }),
        "packs_held",
    );
    assert!(
        last.contains(r#""more":false"#),
        "a required field is never skipped: {last}"
    );
}

/// The manifest rides **verbatim**, base64, and comes back byte for byte.
#[test]
fn register_pack_carries_the_manifest_bytes_unchanged() {
    let manifest = a_manifest();
    let wire = round_trip_to_hub(
        PackToHub::RegisterPack(RegisterPack {
            job_id: a_job(),
            attempt: 2,
            commit: "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68".to_owned(),
            key: "pack".to_owned(),
            manifest: manifest.clone(),
            meta: Some(b"commit=cb5a2e8b\nprofile=debug\n".to_vec()),
        }),
        "register_pack",
    );
    let PackToHub::RegisterPack(back) = from_frame::<PackToHub>(&wire).expect("decodes") else {
        panic!("that is a register_pack");
    };
    assert_eq!(
        back.manifest, manifest,
        "the id is the sha256 of these bytes, so a byte lost on the wire is a different pack"
    );
    assert_eq!(
        PackId::of_manifest_bytes(&back.manifest),
        a_pack(),
        "and the hub recomputes the id from what it received"
    );
    assert!(
        !wire.contains("chatons pack"),
        "it is base64 on the frame, not raw text: {wire}"
    );

    // A pack with no `pack.meta` is a pack, not an error: `meta` is absent
    // from the wire rather than an empty string, which would be a meta file
    // of zero bytes.
    let bare = round_trip_to_hub(
        PackToHub::RegisterPack(RegisterPack {
            job_id: a_job(),
            attempt: 1,
            commit: "c".repeat(40),
            key: "pack".to_owned(),
            manifest,
            meta: None,
        }),
        "register_pack",
    );
    assert!(!bare.contains(r#""meta":"#), "{bare}");
}

#[test]
fn ask_pack_round_trips() {
    round_trip_to_hub(
        PackToHub::AskPack(AskPack {
            job_id: a_job(),
            commit: "c".repeat(40),
            key: "pack".to_owned(),
        }),
        "ask_pack",
    );
}

/// `peer` absent means **the hub names one** — the outbound-only case — and
/// `objects` empty means the whole pack. Both absences are on the wire as
/// absences, not as empty strings or empty lists that mean something else.
#[test]
fn ask_transfer_capability_round_trips_in_both_directions_and_both_absences() {
    let whole = round_trip_to_hub(
        PackToHub::AskTransferCapability(AskTransferCapability {
            job_id: a_job(),
            pack: a_pack(),
            peer: None,
            direction: TransferDirection::Push,
            objects: Vec::new(),
        }),
        "ask_transfer_capability",
    );
    assert!(whole.contains(r#""direction":"push""#), "{whole}");
    assert!(
        !whole.contains(r#""peer":"#),
        "absent, so the hub names one: {whole}"
    );
    assert!(
        !whole.contains(r#""objects":"#),
        "absent, so the whole pack: {whole}"
    );

    let some = round_trip_to_hub(
        PackToHub::AskTransferCapability(AskTransferCapability {
            job_id: a_job(),
            pack: a_pack(),
            peer: Some("wsl-pc".to_owned()),
            direction: TransferDirection::Fetch,
            objects: vec!["1".repeat(64)],
        }),
        "ask_transfer_capability",
    );
    assert!(some.contains(r#""direction":"fetch""#), "{some}");
    assert!(some.contains("wsl-pc"), "{some}");
}

/// All three of D10's paths are named on the wire, and the same-host one
/// carries no peer and zero bytes.
#[test]
fn pack_transferred_names_which_of_the_three_paths_it_took() {
    for (path, tag) in [
        (FetchPath::SameHost, "same_host"),
        (FetchPath::Peer, "peer"),
        (FetchPath::ViaPeer, "via_peer"),
    ] {
        let wire = round_trip_to_hub(
            PackToHub::PackTransferred(PackTransferred {
                job_id: a_job(),
                pack: a_pack(),
                peer: (path != FetchPath::SameHost).then(|| "wsl-pc".to_owned()),
                path,
                bytes: if path == FetchPath::SameHost {
                    0
                } else {
                    421_138_432
                },
                objects_fetched: 557,
                objects_held: 3,
                ms: 4_200,
            }),
            "pack_transferred",
        );
        assert!(wire.contains(&format!(r#""path":"{tag}""#)), "{wire}");
    }

    let same_host = to_frame(&PackToHub::PackTransferred(PackTransferred {
        job_id: a_job(),
        pack: a_pack(),
        peer: None,
        path: FetchPath::SameHost,
        bytes: 0,
        objects_fetched: 0,
        objects_held: 557,
        ms: 11,
    }))
    .expect("encodes");
    assert!(
        same_host.contains(r#""bytes":0"#),
        "the zero D10's same-host clause rests on has to be on the wire to be asserted: \
         {same_host}"
    );
    assert!(
        !same_host.contains(r#""peer":"#),
        "no peer, no network byte: {same_host}"
    );
}

/// A pack that cannot be got is **said**, with a reason, and `pack` is absent
/// when nothing is registered at all — there is no id to name.
#[test]
fn pack_unavailable_round_trips_with_and_without_an_id() {
    let nothing = round_trip_to_hub(
        PackToHub::PackUnavailable(PackUnavailable {
            job_id: a_job(),
            pack: None,
            commit: "c".repeat(40),
            key: "pack".to_owned(),
            why: "no pack is registered for this commit and key".to_owned(),
        }),
        "pack_unavailable",
    );
    assert!(
        !nothing.contains(r#""pack":"#),
        "no id to name; the tag is pack_unavailable, so the key is what is absent: {nothing}"
    );

    round_trip_to_hub(
        PackToHub::PackUnavailable(PackUnavailable {
            job_id: a_job(),
            pack: Some(a_pack()),
            commit: "c".repeat(40),
            key: "pack".to_owned(),
            why: "every holder of it is gone".to_owned(),
        }),
        "pack_unavailable",
    );
}

/// `pack` and `manifest` both absent is **nothing is registered**, said
/// rather than answered with an empty holder list a runner would have to read
/// as either.
#[test]
fn pack_holders_round_trips_and_distinguishes_nothing_registered_from_no_holder() {
    let unregistered = round_trip_from_hub(
        PackFromHub::PackHolders(PackHolders {
            job_id: a_job(),
            commit: "c".repeat(40),
            key: "pack".to_owned(),
            pack: None,
            manifest: None,
            meta: None,
            holders: Vec::new(),
        }),
        "pack_holders",
    );
    assert!(!unregistered.contains(r#""manifest":"#), "{unregistered}");
    assert!(
        !unregistered.contains(r#""holders":"#),
        "nothing registered is said by three absences, not by an empty list: {unregistered}"
    );

    let registered = round_trip_from_hub(
        PackFromHub::PackHolders(PackHolders {
            job_id: a_job(),
            commit: "c".repeat(40),
            key: "pack".to_owned(),
            pack: Some(a_pack()),
            manifest: Some(a_manifest()),
            meta: None,
            holders: vec![
                Holder {
                    runner: "wsl-pc".to_owned(),
                    endpoint: Some("100.71.4.9:7711".to_owned()),
                },
                Holder {
                    runner: "outbound-only".to_owned(),
                    endpoint: None,
                },
            ],
        }),
        "pack_holders",
    );
    assert!(registered.contains("outbound-only"), "{registered}");
    // An outbound-only holder puts no endpoint on the wire, which is how a
    // fetcher knows it must ask the hub for a push instead of dialling.
    let PackFromHub::PackHolders(back) = from_frame(&registered).expect("decodes") else {
        panic!("that is a pack_holders");
    };
    assert_eq!(back.holders[1].endpoint, None);
    assert_eq!(
        back.manifest.as_deref(),
        Some(a_manifest().as_slice()),
        "verbatim, both ways"
    );
}

#[test]
fn transfer_capability_round_trips_with_its_instant_and_its_object_list() {
    let wire = round_trip_from_hub(
        PackFromHub::TransferCapability(TransferCapability {
            job_id: a_job(),
            pack: a_pack(),
            peer: "wsl-pc".to_owned(),
            endpoint: "100.71.4.9:7711".to_owned(),
            token: "t-ephemeral-7f3a".to_owned(),
            expires_unix_s: 1_700_000_300,
            direction: TransferDirection::Fetch,
            objects: vec!["1".repeat(64), "4".repeat(64)],
        }),
        "transfer_capability",
    );
    assert!(
        wire.contains("1700000300"),
        "an instant, not an offset: {wire}"
    );
}

/// The D209 frame, **numbered on neither enum before this** (ORCH-159).
///
/// It carries no `job_id`: the holder is not a party to the job that wants
/// the pack, and a job id it could do nothing with would be the hub telling
/// one runner about another's work.
#[test]
fn expect_transfer_round_trips_and_names_no_job() {
    let wire = round_trip_from_hub(
        PackFromHub::ExpectTransfer(ExpectTransfer {
            pack: a_pack(),
            peer: "the-pc".to_owned(),
            token: "t-ephemeral-7f3a".to_owned(),
            expires_unix_s: 1_700_000_300,
            direction: TransferDirection::Push,
            objects: Vec::new(),
        }),
        "expect_transfer",
    );
    assert!(!wire.contains(r#""job_id":"#), "{wire}");
    assert!(
        !wire.contains(r#""objects":"#),
        "empty is the whole pack: {wire}"
    );
}

#[test]
fn pack_registered_round_trips() {
    round_trip_from_hub(
        PackFromHub::PackRegistered(PackRegistered {
            job_id: a_job(),
            pack: a_pack(),
            commit: "c".repeat(40),
            key: "pack".to_owned(),
        }),
        "pack_registered",
    );
}

/// Non-terminal, and deliberately **not** `Refused`, which closes the link
/// (D13).
#[test]
fn pack_refused_is_its_own_frame_and_not_the_terminal_one() {
    let wire = round_trip_from_hub(
        PackFromHub::PackRefused(PackRefused {
            job_id: a_job(),
            pack: Some(a_pack()),
            why: "this commit and key already have a different registered id".to_owned(),
        }),
        "pack_refused",
    );
    assert!(
        !wire.contains(r#""type":"refused""#),
        "a refusal about one pack must not be the frame that drops the link: {wire}"
    );
    // And `Refused` stays what it is: one reason, no pack, terminal.
    assert_eq!(
        from_frame::<FromHub>(r#"{"type":"refused","reason":"no"}"#).expect("decodes"),
        FromHub::Refused {
            reason: "no".to_owned()
        }
    );
}

/// Holdings are volatile, so a restarted hub has to ask again — the same
/// shape as `ReadProbesAgain`, so an operator who knows one knows both.
#[test]
fn read_packs_again_round_trips_and_matches_its_sibling() {
    round_trip_from_hub(
        PackFromHub::ReadPacksAgain(ReadPacksAgain {
            why: "this hub restarted and holds no holding".to_owned(),
        }),
        "read_packs_again",
    );
    let sibling = to_frame(&FromHub::ReadProbesAgain {
        why: "this hub restarted".to_owned(),
    })
    .expect("encodes");
    assert!(
        sibling.contains(r#""why":"#),
        "one shape for \"tell me again\", and it carries the reason: {sibling}"
    );
}

/// M12's keeper pinning, numbered now so the keeper half needs no version of
/// its own.
#[test]
fn pin_packs_round_trips() {
    let wire = round_trip_from_hub(
        PackFromHub::PinPacks(PinPacks {
            keys: vec!["pack".to_owned()],
            keep_per_key: 3,
        }),
        "pin_packs",
    );
    assert!(wire.contains(r#""keep_per_key":3"#), "{wire}");
}

// ---------------------------------------------------------------------------
// 6. The types that are not frames
// ---------------------------------------------------------------------------

/// What rides on `AssignJob` so the common path costs **no round trip**.
#[test]
fn an_assigned_pack_round_trips_with_its_manifest_verbatim() {
    let assigned = AssignedPack {
        key: "pack".to_owned(),
        pack: a_pack(),
        manifest: a_manifest(),
        meta: Some(b"profile=debug\n".to_vec()),
        holders: vec![Holder {
            runner: "wsl-pc".to_owned(),
            endpoint: Some("100.71.4.9:7711".to_owned()),
        }],
    };
    let wire = serde_json::to_string(&assigned).expect("encodes");
    let back: AssignedPack = serde_json::from_str(&wire).expect("decodes");
    assert_eq!(back, assigned);
    assert_eq!(back.manifest, a_manifest(), "verbatim, or a different pack");
    assert_eq!(PackId::of_manifest_bytes(&back.manifest), back.pack);

    // Empty is said by being absent, and `holders: []` on arrival is
    // `PackState::NoHolder` rather than an unknown.
    let nobody = AssignedPack {
        holders: Vec::new(),
        meta: None,
        ..assigned
    };
    let wire = serde_json::to_string(&nobody).expect("encodes");
    assert!(!wire.contains(r#""holders":"#), "{wire}");
    assert!(!wire.contains(r#""meta":"#), "{wire}");
    let back: AssignedPack = serde_json::from_str(&wire).expect("decodes");
    assert!(back.holders.is_empty());
}

#[test]
fn a_fetch_capability_round_trips() {
    let capability = FetchCapability {
        token: "t-ephemeral-7f3a".to_owned(),
        pack: a_pack(),
        peer: "the-pc".to_owned(),
        direction: TransferDirection::Fetch,
        objects: vec!["1".repeat(64)],
        expires_unix_s: 1_700_000_300,
    };
    let wire = serde_json::to_string(&capability).expect("encodes");
    assert_eq!(
        serde_json::from_str::<FetchCapability>(&wire).expect("decodes"),
        capability
    );
}

/// The peer wire: a request, a reply, a refusal. It never touches the hub
/// (D10) and still lives in `herd-proto` (invariant 4).
#[test]
fn the_peer_wire_round_trips_and_carries_the_hubs_version() {
    let request = PeerRequest {
        protocol_version: PROTOCOL_VERSION,
        token: "t-ephemeral-7f3a".to_owned(),
        pack: a_pack(),
        object: "1".repeat(64),
        offset: PACK_CHUNK_BYTES,
    };
    let wire = serde_json::to_string(&request).expect("encodes");
    assert!(
        wire.contains(r#""protocol_version":3"#),
        "the number is on the peer wire so a mismatch is refused by name: {wire}"
    );
    assert_eq!(
        serde_json::from_str::<PeerRequest>(&wire).expect("decodes"),
        request
    );

    let reply = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: 12,
        object_length: 12,
    };
    let wire = serde_json::to_string(&reply).expect("encodes");
    assert!(
        !wire.contains("data"),
        "the bytes left the struct: a reply is a header, and the body follows it raw: {wire}"
    );
    assert_eq!(
        serde_json::from_str::<PeerReply>(&wire).expect("decodes"),
        reply
    );

    let refusal = PeerRefusal {
        why: peer_version_mismatch_reason(2),
    };
    let wire = serde_json::to_string(&refusal).expect("encodes");
    assert_eq!(
        serde_json::from_str::<PeerRefusal>(&wire).expect("decodes"),
        refusal
    );
}

/// The three named visible states, each on the wire as a name and never as an
/// absence somebody has to infer.
#[test]
fn every_pack_state_and_every_enum_round_trips_by_name() {
    for (state, tag) in [
        (PackState::Registered, "registered"),
        (PackState::Incomplete, "incomplete"),
        (PackState::NoHolder, "no_holder"),
    ] {
        let wire = serde_json::to_string(&state).expect("encodes");
        assert_eq!(wire, format!("\"{tag}\""));
        assert_eq!(
            serde_json::from_str::<PackState>(&wire).expect("decodes"),
            state
        );
    }
    for (direction, tag) in [
        (TransferDirection::Fetch, "fetch"),
        (TransferDirection::Push, "push"),
    ] {
        assert_eq!(
            serde_json::to_string(&direction).expect("encodes"),
            format!("\"{tag}\"")
        );
    }
    for (kind, letter) in [
        (ManifestKind::File, "f"),
        (ManifestKind::Dir, "d"),
        (ManifestKind::Link, "l"),
    ] {
        assert_eq!(kind.letter(), letter);
        assert_eq!(ManifestKind::from_letter(letter), Some(kind));
    }
    assert_eq!(
        ManifestKind::from_letter("x"),
        None,
        "a kind this build has not been taught is refused by its caller, never read as \"not a \
         file\""
    );
}

/// A `PackId` is a string on the wire and nothing else, so a record or a
/// manifest holding one is readable by eye.
#[test]
fn a_pack_id_is_transparent_on_the_wire() {
    let id = a_pack();
    let wire = serde_json::to_string(&id).expect("encodes");
    assert_eq!(wire, format!("\"{id}\""));
    assert_eq!(serde_json::from_str::<PackId>(&wire).expect("decodes"), id);
}

// ---------------------------------------------------------------------------
// 7. The peer leg's framing: one JSON line, then the bytes
// ---------------------------------------------------------------------------
//
// The control plane has no choice about base64 (D4's text frames, D18's
// byte-exact output). This leg does: it touches no hub, and it is the one leg
// that moves hundreds of megabytes — the consumer's measured 401.6 MiB pack
// would be about 535 MiB base64'd. So the header is JSON and the body is not,
// and every way the two can disagree is refused **by its own name**, each
// proven below by a case that fires it.

/// A reply header, a body, and the next header behind it — read exactly.
///
/// The assertion that matters is the third one: after `take_peer_body` the
/// remainder begins the *next* header and not one byte before or after it. A
/// framing that is off by one is a framing that works on the first chunk of
/// every transfer and on no other.
#[test]
fn a_replys_body_is_read_exactly_and_the_next_header_follows_it() {
    let body: Vec<u8> = (0..4096u32).map(|byte| byte as u8).collect();
    let first = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: body.len() as u64,
        object_length: 12_288,
    };
    let (line, promised) = encode_peer_header(&first).expect("a reply header encodes");
    assert_eq!(
        promised,
        body.len() as u64,
        "the number the caller writes is the number the line carries, or the stream desynchronises"
    );
    assert_eq!(
        line.last(),
        Some(&b'\n'),
        "one line, ended by one newline: {}",
        String::from_utf8_lossy(&line)
    );
    assert_eq!(
        line.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "and exactly one, or the body starts somewhere else"
    );

    let second = PeerReply {
        object: "1".repeat(64),
        offset: body.len() as u64,
        length: 8_192,
        object_length: 12_288,
    };
    let (next_line, _) = encode_peer_header(&second).expect("encodes");

    // What arrives on the socket: line, body, and the beginning of the next
    // exchange, all in one buffer, because a stream has no message boundaries
    // of its own.
    let mut arrived = line.clone();
    arrived.extend_from_slice(&body);
    arrived.extend_from_slice(&next_line);

    let read = decode_peer_header::<PeerReply>(&arrived).expect("the header reads");
    let PeerHeaderRead::Read(header) = read else {
        panic!("the whole line is here, so it is not incomplete");
    };
    assert_eq!(header.header, first);
    assert_eq!(header.body_bytes, body.len() as u64);
    assert_eq!(
        header.body_at,
        line.len(),
        "the body starts past the newline"
    );

    let (read_body, rest) =
        take_peer_body(&arrived[header.body_at..], header.body_bytes).expect("the body is here");
    assert_eq!(
        read_body,
        body.as_slice(),
        "byte for byte, or a different object"
    );
    assert_eq!(
        rest, next_line,
        "the remainder is the next header exactly, with nothing of the body left in it"
    );
    let PeerHeaderRead::Read(next) = decode_peer_header::<PeerReply>(rest).expect("and it reads")
    else {
        panic!("that is the second header");
    };
    assert_eq!(next.header, second);
}

/// Half a line is **not** a refusal.
///
/// The distinction a stream forces: a reader that closed the connection
/// whenever a read ended inside a header would close a healthy one on every
/// chunk boundary. It becomes a refusal only once more unterminated header
/// has arrived than one read of the endpoint's buffer could hold.
#[test]
fn a_header_still_arriving_is_incomplete_and_not_refused() {
    let reply = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: 16,
        object_length: 16,
    };
    let (line, _) = encode_peer_header(&reply).expect("encodes");
    for cut in [0, 1, line.len() / 2, line.len() - 1] {
        assert_eq!(
            decode_peer_header::<PeerReply>(&line[..cut]).expect("not a refusal"),
            PeerHeaderRead::Incomplete,
            "{cut} bytes of a {} byte line is a short read, not a bad peer",
            line.len()
        );
    }
    assert!(matches!(
        decode_peer_header::<PeerReply>(&line).expect("the whole line reads"),
        PeerHeaderRead::Read(_)
    ));
}

/// A header longer than one read of the endpoint's buffer is refused, and the
/// bound is [`PACK_ENDPOINT_READ_BYTES`] rather than a number invented here.
///
/// Both sides: a peer that *sent* one is refused before the line goes out,
/// and a peer that is *receiving* one stops waiting for a newline that is
/// never coming. Without the second, an endless line is an endless buffer,
/// which is invariant 2 on the serving runner's host.
#[test]
fn a_header_past_one_read_of_the_buffer_is_refused_on_both_sides() {
    let flood = vec![b'{'; PACK_ENDPOINT_READ_BYTES as usize];
    let refusal =
        decode_peer_header::<PeerReply>(&flood).expect_err("no newline and no room left for one");
    assert_eq!(
        refusal,
        PeerFramingRefusal::HeaderPastTheReadBuffer {
            bytes: PACK_ENDPOINT_READ_BYTES as usize
        }
    );
    let said = refusal.to_string();
    assert!(
        said.contains(&PACK_ENDPOINT_READ_BYTES.to_string()),
        "the refusal names the bound it applied: {said}"
    );
    assert!(
        said.contains("one line of JSON"),
        "and says what a header is: {said}"
    );

    // One byte short of the bound is still only a short read.
    assert_eq!(
        decode_peer_header::<PeerReply>(&flood[..flood.len() - 1]).expect("not a refusal"),
        PeerHeaderRead::Incomplete
    );

    // And a header this peer would have sent is caught before it is sent.
    let enormous = PeerRefusal {
        why: "x".repeat(PACK_ENDPOINT_READ_BYTES as usize),
    };
    let refused = encode_peer_header(&enormous).expect_err("a line that long is not one line");
    assert!(matches!(
        refused,
        PeerFramingRefusal::HeaderPastTheReadBuffer { .. }
    ));
}

/// A body that stopped short is discarded, by name, and the fetch resumes at
/// the last whole chunk.
///
/// The failure this closes is the one the consumer met on the job half: bytes
/// that never came, with nothing anywhere saying so. An object is its
/// sha256, so a short body is not a short object — it is nothing.
#[test]
fn a_truncated_body_is_refused_and_says_what_was_promised() {
    let reply = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: 4_096,
        object_length: 4_096,
    };
    let (line, promised) = encode_peer_header(&reply).expect("encodes");
    let mut arrived = line.clone();
    arrived.extend_from_slice(&[7u8; 4_000]);

    let PeerHeaderRead::Read(header) =
        decode_peer_header::<PeerReply>(&arrived).expect("the header is whole")
    else {
        panic!("the header is whole");
    };
    let refusal = take_peer_body(&arrived[header.body_at..], promised)
        .expect_err("4 000 of 4 096 is not a body");
    assert_eq!(
        refusal,
        PeerFramingRefusal::BodyTruncated {
            promised: 4_096,
            arrived: 4_000
        }
    );
    let said = refusal.to_string();
    assert!(said.contains("4096") && said.contains("4000"), "{said}");
    assert!(
        said.contains("discarded"),
        "a partial object is never written: {said}"
    );
}

/// A length that disagrees with what arrived is refused **at the next
/// header**, which is the one place the disagreement is visible.
///
/// A sender that wrote more bytes than its header promised cannot be caught
/// by counting the body — the promised count is there, so the body reads
/// fine. What catches it is that the surplus is still in the buffer where the
/// next header should be, and `take_peer_body` hands it back as the remainder
/// rather than consuming it. The next `decode_peer_header` then refuses it by
/// name instead of writing somebody's stray bytes into an object.
#[test]
fn a_length_that_disagrees_with_what_arrived_is_refused_at_the_next_header() {
    let reply = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: 64,
        object_length: 128,
    };
    let (line, promised) = encode_peer_header(&reply).expect("encodes");
    let mut arrived = line.clone();
    arrived.extend_from_slice(&[3u8; 80]); // 16 bytes more than it said
    arrived.push(b'\n');

    let PeerHeaderRead::Read(header) =
        decode_peer_header::<PeerReply>(&arrived).expect("the header is whole")
    else {
        panic!("the header is whole");
    };
    let (body, rest) =
        take_peer_body(&arrived[header.body_at..], promised).expect("64 bytes are here");
    assert_eq!(body.len(), 64, "exactly what was promised and no more");
    assert_eq!(
        rest.len(),
        17,
        "the surplus is left where the next header goes"
    );

    let refusal = decode_peer_header::<PeerReply>(rest)
        .expect_err("sixteen bytes of nothing are not a header");
    assert!(
        matches!(refusal, PeerFramingRefusal::HeaderUnreadable { .. }),
        "{refusal:?}"
    );
    let said = refusal.to_string();
    assert!(
        said.contains("lost count of the stream"),
        "it says why nothing later on this connection can be trusted: {said}"
    );
}

/// A reply promising more than one chunk is refused, on the way out and on
/// the way in.
#[test]
fn a_body_past_the_chunk_is_refused_on_both_sides() {
    let greedy = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: PACK_CHUNK_BYTES + 1,
        object_length: PACK_CHUNK_BYTES * 4,
    };
    let refused = encode_peer_header(&greedy).expect_err("more than a chunk");
    assert_eq!(
        refused,
        PeerFramingRefusal::BodyPastTheChunk {
            promised: PACK_CHUNK_BYTES + 1
        }
    );
    assert!(
        refused.to_string().contains(&PACK_CHUNK_BYTES.to_string()),
        "the refusal names the cap: {refused}"
    );

    // And a peer that sent one anyway is refused at the header, before a byte
    // of it is held.
    let line = format!(
        r#"{{"object":"{}","offset":0,"length":{},"object_length":{}}}"#,
        "1".repeat(64),
        PACK_CHUNK_BYTES + 1,
        PACK_CHUNK_BYTES * 4
    );
    let mut arrived = line.into_bytes();
    arrived.push(b'\n');
    assert_eq!(
        decode_peer_header::<PeerReply>(&arrived).expect_err("refused at the header"),
        PeerFramingRefusal::BodyPastTheChunk {
            promised: PACK_CHUNK_BYTES + 1
        }
    );

    // Exactly one chunk is fine: the cap is a cap and not a fence.
    let at_the_cap = PeerReply {
        object: "1".repeat(64),
        offset: 0,
        length: PACK_CHUNK_BYTES,
        object_length: PACK_CHUNK_BYTES,
    };
    assert_eq!(
        encode_peer_header(&at_the_cap)
            .expect("one whole chunk is allowed")
            .1,
        PACK_CHUNK_BYTES
    );
}

/// A header whose chunk ends past the object it names is refused as a
/// **disagreement with itself**, before any body is waited for.
#[test]
fn a_chunk_that_ends_past_its_object_is_refused_as_a_disagreement() {
    let impossible = PeerReply {
        object: "1".repeat(64),
        offset: 900,
        length: 200,
        object_length: 1_000,
    };
    let refused = encode_peer_header(&impossible).expect_err("900 + 200 > 1 000");
    assert_eq!(
        refused,
        PeerFramingRefusal::BodyPastTheObject {
            offset: 900,
            length: 200,
            object_length: 1_000
        }
    );
    let said = refused.to_string();
    assert!(
        said.contains("100 bytes past it"),
        "the refusal does the arithmetic for the person reading it: {said}"
    );
    assert!(
        said.contains("same sha256"),
        "and names what it means: two peers, two objects, one name: {said}"
    );

    // A chunk that ends exactly at the object's end is the last chunk, not a
    // disagreement — the boundary case the refusal must not swallow.
    let last = PeerReply {
        object: "1".repeat(64),
        offset: 800,
        length: 200,
        object_length: 1_000,
    };
    assert!(encode_peer_header(&last).is_ok(), "that is the last chunk");
}

/// A peer announcing a version that is not this one's is refused by name,
/// with the handshake's own sentence.
#[test]
fn a_peer_on_another_protocol_version_is_refused_by_name() {
    assert_eq!(check_peer_protocol_version(PROTOCOL_VERSION), Ok(()));
    let refused = check_peer_protocol_version(PROTOCOL_VERSION - 1)
        .expect_err("the check is equality, not a floor");
    assert_eq!(
        refused,
        PeerFramingRefusal::ProtocolVersion {
            announced: PROTOCOL_VERSION - 1
        }
    );
    assert_eq!(
        refused.to_string(),
        peer_version_mismatch_reason(PROTOCOL_VERSION - 1),
        "one sentence about one contract, written once (invariant 4)"
    );
    assert!(
        refused
            .to_string()
            .contains("refused at the hub's handshake too"),
        "and it sends the operator where the fix is: {refused}"
    );
}

/// A newline **inside** a header's own text does not end the line.
///
/// The whole framing rests on this, so it is asserted rather than assumed:
/// JSON escapes a newline in a string as `\n`, two characters. The fixture is
/// the refusal a peer really sends, because that is the field most likely to
/// carry one.
#[test]
fn a_refusal_carrying_a_newline_is_still_one_line() {
    let refusal = PeerRefusal {
        why: format!(
            "this peer holds none of that pack\nand its store was swept at 04:00\n{}",
            peer_version_mismatch_reason(2)
        ),
    };
    let (line, body) = encode_peer_header(&refusal).expect("encodes");
    assert_eq!(body, 0, "a refusal carries no bytes behind it");
    assert_eq!(
        line.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "the only raw newline is the one that ends the line"
    );
    let PeerHeaderRead::Read(read) =
        decode_peer_header::<PeerRefusal>(&line).expect("and reads back")
    else {
        panic!("one whole line");
    };
    assert_eq!(read.header, refusal, "with its newlines intact");
    assert_eq!(read.body_bytes, 0);
}

/// A request carries no body either, so a reader never waits for one.
#[test]
fn a_request_carries_its_version_and_no_body() {
    let request = PeerRequest {
        protocol_version: PROTOCOL_VERSION,
        token: "t-ephemeral-7f3a".to_owned(),
        pack: a_pack(),
        object: "1".repeat(64),
        offset: PACK_CHUNK_BYTES,
    };
    let (line, body) = encode_peer_header(&request).expect("encodes");
    assert_eq!(body, 0);
    let PeerHeaderRead::Read(read) = decode_peer_header::<PeerRequest>(&line).expect("reads")
    else {
        panic!("one whole line");
    };
    assert_eq!(read.header, request);
    assert_eq!(read.body_bytes, 0);
    assert_eq!(read.body_at, line.len(), "the next header starts here");
}
