//! E-51: the handshake says which *build* is speaking, and the hub answers a
//! minor gap with a warning and a major one with a refusal.
//!
//! The protocol version, which M1 already refuses on, says whether two peers
//! can read each other's frames. It says nothing about whether they agree on
//! what the frames *mean*: a runner built three releases ago reads every field
//! of today's `AssignJob` and may still confine a job differently or spool a
//! verdict in a shape the hub has stopped reading, all inside one protocol
//! version. So the build travels too.
//!
//! Every peer here is a hand-built handshake rather than the real runner,
//! because the figure under test is a compile-time constant of the real
//! runner: there is no way to ask `herd-runner` to claim it is version 2.
//! What is asserted is what the hub does with the figure, and — for the minor
//! case — that the connection really is *served*: a job is placed on it.

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, Role, ToHub, PROTOCOL_VERSION};
use support::{shell, Harness, PATIENCE};
use tokio_tungstenite::tungstenite::Message;

/// How long a test waits to find out that the hub said nothing more.
///
/// The one duration here that is waited out rather than waited on: "no other
/// frame came" has no event to wait for. The hub answers a handshake in the
/// same breath, so this is milliseconds of real work and the rest is slack.
const NOTHING_ELSE_CAME: Duration = Duration::from_millis(500);

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Dial as a runner announcing `peer_version`, and come back with the socket
/// and the hub's first frame.
async fn dial_announcing(harness: &Harness, name: &str, peer_version: &str) -> (Socket, FromHub) {
    let (mut socket, _response) = tokio_tungstenite::connect_async(&harness.hub_url)
        .await
        .expect("the hub accepts a connection");
    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: peer_version.to_owned(),
        role: Role::Runner,
        // Approved, so that nothing in this file is refused for being an
        // unknown machine: the only thing under test is the version.
        token: Some(harness.approve(name)),
        name: Some(name.to_owned()),
        instance: Some(format!("run-{name}")),
        state_root: None,
        health_every_ms: None,
        capabilities: Default::default(),
        host: herd_proto::HostSize::default(),
        preference: 0,
        load_milli: None,
        switch: herd_proto::Switch::On,
    };
    socket
        .send(Message::text(to_frame(&hello).expect("encodes")))
        .await
        .expect("hello goes out");
    let first = next_frame(&mut socket)
        .await
        .expect("the hub answers a handshake rather than going quiet");
    (socket, first)
}

async fn next_frame(socket: &mut Socket) -> Option<FromHub> {
    loop {
        match tokio::time::timeout(PATIENCE, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                return Some(from_frame(text.as_str()).expect("the hub speaks the protocol"))
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

/// A minor gap is served, and said out loud with both versions in one line
/// (E-51).
///
/// "Served" is asserted the only way it means anything: a job is submitted and
/// the hub places it on this runner. A test that stopped at the welcome would
/// pass just as well against a hub that warned and then quietly refused to
/// dispatch.
#[tokio::test(flavor = "multi_thread")]
async fn a_minor_version_mismatch_warns_with_both_versions_and_still_works() {
    let harness = Harness::with_hub().await;
    let hub_version = herd_hub::hub_version();
    // One minor ahead of the hub, which is the real case: a machine updated
    // before the hub was.
    let ours = bump_minor(hub_version);

    let (mut socket, first) = dial_announcing(&harness, "wsl-pc", &ours).await;
    let FromHub::Welcome { version_note, .. } = first else {
        panic!("a minor gap is served, so the first frame is a welcome, not {first:?}");
    };
    let note = version_note.expect("a minor gap is said out loud, not swallowed");
    assert!(
        note.contains(hub_version) && note.contains(&ours),
        "the warning must name both versions, or a person reading one line \
         cannot tell which side to move: {note}"
    );
    assert!(
        note.contains("major versions agree"),
        "and why it is served anyway: {note}"
    );

    // And it really is a runner: the hub places a job on it.
    support::wait_until("the hub counts this runner as joined", || {
        harness.runner_joined()
    })
    .await;
    let requester = harness.requester();
    let submitted = tokio::spawn(async move {
        support::submit_job(&requester, shell("printf 'ran\\n'"))
            .await
            .expect("the submission is taken")
    });
    let assigned = next_frame(&mut socket)
        .await
        .expect("the hub hands the job to the runner it just warned");
    match assigned {
        FromHub::AssignJob { job_id, .. } => {
            assert_eq!(
                job_id,
                submitted.await.expect("the submission came back"),
                "the job the hub placed is the job that was submitted"
            );
        }
        other => panic!("a runner a minor version apart must be dispatched to, not {other:?}"),
    }
}

/// A major gap is refused, in one line naming both versions, and the refusal
/// is the whole of what that peer gets (E-51, D13).
#[tokio::test(flavor = "multi_thread")]
async fn a_major_version_mismatch_is_refused_with_both_versions() {
    let harness = Harness::with_hub().await;
    let hub_version = herd_hub::hub_version();
    let ours = bump_major(hub_version);

    let (mut socket, first) = dial_announcing(&harness, "wsl-pc", &ours).await;
    let FromHub::Refused { reason } = first else {
        panic!("a major gap is a terminal no (D13), not {first:?}");
    };
    assert!(
        reason.contains(hub_version) && reason.contains(&ours),
        "the refusal must name both versions: {reason}"
    );
    assert!(
        reason.contains("major version apart"),
        "and say what the gap is: {reason}"
    );
    assert!(
        reason.contains("Move the runner") || reason.contains("move the hub"),
        "and what to do about it: {reason}"
    );

    // Nothing else, and no runner: a peer the hub refuses is not in the
    // fleet, and the refusal is the last frame on that connection.
    match tokio::time::timeout(NOTHING_ELSE_CAME, socket.next()).await {
        Ok(None) | Ok(Some(Ok(Message::Close(_)))) | Err(_) => {}
        other => panic!("the refusal must be the last frame, not followed by {other:?}"),
    }
    assert!(
        !harness.runner_joined(),
        "a runner the hub refused must not be counted in its fleet"
    );
}

/// A version string the hub cannot read is served with a note, never refused.
///
/// Turning a working machine away over the shape of a string would be a hub
/// refusing what it merely does not understand. The credential is what says a
/// peer is allowed here; the version only says whether to expect the same
/// behaviour, and "I cannot tell" is what the note says.
#[tokio::test(flavor = "multi_thread")]
async fn a_version_the_hub_cannot_read_is_served_and_said_so() {
    let harness = Harness::with_hub().await;
    let (_socket, first) = dial_announcing(&harness, "wsl-pc", "built-from-a-branch").await;
    let FromHub::Welcome { version_note, .. } = first else {
        panic!("an unreadable version is served, not {first:?}");
    };
    let note = version_note.expect("and said out loud");
    assert!(
        note.contains(herd_hub::hub_version()) && note.contains("built-from-a-branch"),
        "naming both, as every version line does: {note}"
    );
    assert!(
        note.contains("nothing about the gap between the two builds could be judged"),
        "and saying plainly that nothing was judged: {note}"
    );
}

/// Two peers of the same build say nothing about versions at all.
///
/// The ordinary case, and worth pinning: a note on every connection would be
/// a line nobody reads, which is how the one that matters gets missed.
#[tokio::test(flavor = "multi_thread")]
async fn two_peers_of_one_build_say_nothing_about_versions() {
    let harness = Harness::with_hub().await;
    let (_socket, first) = dial_announcing(&harness, "wsl-pc", herd_hub::hub_version()).await;
    let FromHub::Welcome { version_note, .. } = first else {
        panic!("the ordinary case is a welcome, not {first:?}");
    };
    assert_eq!(
        version_note, None,
        "two peers of one build have nothing to be told about versions"
    );
}

/// `0.1.0` -> `0.2.0`: one minor ahead, whatever the hub's own figure is.
fn bump_minor(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().expect("a version has a major");
    let minor: u64 = parts
        .next()
        .expect("a version has a minor")
        .parse()
        .expect("the minor is a number");
    format!("{major}.{}.0", minor + 1)
}

/// `0.1.0` -> `1.1.0`: one major ahead.
fn bump_major(version: &str) -> String {
    let mut parts = version.split('.');
    let major: u64 = parts
        .next()
        .expect("a version has a major")
        .parse()
        .expect("the major is a number");
    let rest: Vec<&str> = parts.collect();
    format!("{}.{}", major + 1, rest.join("."))
}
