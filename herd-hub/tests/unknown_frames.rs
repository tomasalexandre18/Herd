//! What each of Herd's three peers does with a frame it has never heard of
//! (D148), and why the three answers are different on purpose.
//!
//! This is the half of `PROTOCOL_VERSION` 2 that is not about output at all.
//! Both frame enums now carry a `#[serde(other)]` catch-all, so an unknown
//! `type` **decodes** instead of failing to — and what each peer then does
//! with the value is the whole point:
//!
//! * a **runner** logs it and ignores it, because a frame it does not know is
//!   work it is not needed for. Before this, the decode error killed the link
//!   and the runner redialled *into the same undecodable frame*: it looped
//!   rather than shrugged. D128 recorded that and did not fix it, and it is
//!   the reason spending a protocol break on an enum arm was worth it;
//! * a **requester** ends its run, because one that cannot read what the hub
//!   is telling it must not report a verdict (D124). The `bail!` is right and
//!   it stays; what the catch-all buys is a named frame type in the sentence;
//! * the **hub** refuses the peer by name, because a request it cannot read
//!   is work that will never be done and a peer told nothing would wait for
//!   it.
//!
//! The runner and the requester are the real ones, against a hand-written
//! hub — a test that cannot make a hub send something from the future cannot
//! prove anything about meeting one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub, PROTOCOL_VERSION};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

mod support;

/// A frame from a Herd two releases from now: a plausible M11 registry frame,
/// with a payload, of a type nothing in this build has heard of.
const FROM_THE_FUTURE: &str =
    r#"{"type":"pack_capability","blob":"e3b0c442","peer":"pc","expires_in_ms":30000}"#;

/// A hand-written hub: it completes the handshake, says something from the
/// future, and then counts how many times it is dialled.
struct HubFromTheFuture {
    url: String,
    dials: Arc<AtomicUsize>,
    /// Health reports seen after the unknown frame was sent — the proof that
    /// the link is still carrying traffic rather than merely still open.
    reports: Arc<AtomicUsize>,
    _serving: tokio::task::JoinHandle<()>,
}

impl HubFromTheFuture {
    async fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free loopback port");
        let url = format!(
            "ws://{}",
            listener.local_addr().expect("a bound listener has one")
        );
        let dials = Arc::new(AtomicUsize::new(0));
        let reports = Arc::new(AtomicUsize::new(0));
        let serving = tokio::spawn({
            let dials = Arc::clone(&dials);
            let reports = Arc::clone(&reports);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    dials.fetch_add(1, Ordering::SeqCst);
                    let reports = Arc::clone(&reports);
                    tokio::spawn(async move {
                        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                            return;
                        };
                        // The handshake, and nothing clever about it: this
                        // hub speaks the same protocol version, so nothing
                        // here is a version mismatch. What follows is a frame
                        // a *newer build of the same version* could send,
                        // which is exactly the case the catch-all is for.
                        while let Some(Ok(frame)) = socket.next().await {
                            let Message::Text(text) = frame else { continue };
                            match from_frame::<ToHub>(text.as_str()) {
                                Ok(ToHub::Hello { .. }) => {
                                    let welcome = to_frame(&FromHub::Welcome {
                                        protocol_version: PROTOCOL_VERSION,
                                        hub_version: "9.9.9".to_owned(),
                                        state_root: "future".to_owned(),
                                        version_note: None,
                                    })
                                    .expect("a welcome encodes");
                                    if socket.send(Message::text(welcome)).await.is_err() {
                                        return;
                                    }
                                    if socket.send(Message::text(FROM_THE_FUTURE)).await.is_err() {
                                        return;
                                    }
                                }
                                // Every report is answered, as the real hub
                                // does: a runner that hears nothing at all
                                // redials on its own silence limit, and that
                                // would be a second reason to dial twice.
                                Ok(ToHub::Health { .. }) => {
                                    reports.fetch_add(1, Ordering::SeqCst);
                                    let said =
                                        to_frame(&FromHub::HealthRecorded {}).expect("encodes");
                                    if socket.send(Message::text(said)).await.is_err() {
                                        return;
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
                }
            }
        });
        Self {
            url,
            dials,
            reports,
            _serving: serving,
        }
    }
}

/// **A runner that meets a frame from the future ignores it and stays.**
///
/// The assertion that matters is the dial count, and it has to be measured
/// against [`herd_runner::run`] rather than `serve_once`: `run` is the
/// redial loop, and the defect D128 recorded only exists inside it — one
/// connection failing is not a loop, coming straight back into the same frame
/// is. The backoff is a few milliseconds here, so a runner that was going to
/// loop would have looped dozens of times in the window below.
///
/// One dial, and three health reports to show the link is not merely still
/// open but still working.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runner_ignores_a_frame_from_the_future_and_does_not_redial() {
    let hub = HubFromTheFuture::bind().await;
    let data_root = herd_runner::testing::TempDir::new("unknown-frames");
    let config = herd_runner::Config {
        hub_url: hub.url.clone(),
        name: Some("future-runner".to_owned()),
        health_every: Duration::from_millis(100),
        confinement: support::confinement(&data_root),
        // A redial that is nearly instant, so looping would be unmissable.
        redial: herd_runner::Backoff {
            first: Duration::from_millis(5),
            cap: Duration::from_millis(5),
            give_up_after: Duration::from_secs(60),
        },
        ..herd_runner::Config::default()
    };
    let serving = tokio::spawn(async move {
        let _ = herd_runner::run(config).await;
    });

    // Long enough for several health cadences, so a runner that was going to
    // redial has had many chances to.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let dials = hub.dials.load(Ordering::SeqCst);
    let reports = hub.reports.load(Ordering::SeqCst);
    serving.abort();
    drop(data_root);

    assert_eq!(
        dials, 1,
        "a frame it cannot read must not make a runner redial: {dials} dials in 1.2 s is \
         D128's loop, not a shrug"
    );
    assert!(
        reports >= 3,
        "and the link has to still be working, not merely still open: {reports} health \
         report(s) after the unknown frame"
    );
}

/// **A requester that meets one ends its run, and names it.**
///
/// The opposite answer to the runner's, deliberately: a requester that cannot
/// understand what the hub is telling it must not go on to report a verdict
/// (D124), because a guess there is a false statement about whether somebody's
/// tests passed. What the catch-all adds is the **name** — this used to be a
/// serde error about an unknown variant, and is now a sentence an operator can
/// act on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_requester_refuses_a_frame_from_the_future_and_names_it() {
    let hub = HubFromTheFuture::bind().await;
    let config = herdctl::Config {
        hub_url: hub.url.clone(),
        ..herdctl::Config::default()
    };
    let error = herdctl::wait_for_job(&config, JobId::new("j0000000001"), |_, _| {}, |_| {})
        .await
        .expect_err("a requester must not report a verdict it cannot read");
    let said = format!("{error:#}");
    assert!(
        said.contains("pack_capability"),
        "the sentence must name the frame type: {said}"
    );
    assert!(
        said.contains("rebuild herdctl") || said.contains("does not know"),
        "and say what to do about it: {said}"
    );
}

/// **A hub that meets one refuses the peer by name.**
///
/// Not ignored, and that is the third answer. A *request* the hub cannot read
/// is work that will never be done, so a peer told nothing would wait for it
/// for ever — which is the failure a catch-all is supposed to prevent, not a
/// cheaper version of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_refuses_a_frame_it_does_not_know_rather_than_ignoring_it() {
    let harness = support::Harness::with_hub().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(&harness.hub_url)
        .await
        .expect("a peer dials the hub");
    let hello = to_frame(&ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: "test".to_owned(),
        role: Role::Requester,
        token: None,
        name: Some("a-peer-from-the-future".to_owned()),
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: Default::default(),
        host: herd_proto::HostSize::default(),
        preference: 0,
        load_milli: None,
        switch: herd_proto::Switch::On,
    })
    .expect("a hello encodes");
    socket
        .send(Message::text(hello))
        .await
        .expect("the hello goes");
    // The welcome, before anything else.
    let welcome = socket.next().await.expect("an answer").expect("a frame");
    let Message::Text(text) = welcome else {
        panic!("the protocol carries text")
    };
    assert!(
        matches!(
            from_frame::<FromHub>(text.as_str()),
            Ok(FromHub::Welcome { .. })
        ),
        "the handshake completes first: {text}"
    );

    socket
        .send(Message::text(
            r#"{"type":"deploy_to_production","target":"prod"}"#,
        ))
        .await
        .expect("the unknown frame goes");
    let answer = socket.next().await.expect("an answer").expect("a frame");
    let Message::Text(text) = answer else {
        panic!("the protocol carries text")
    };
    let FromHub::Refused { reason } = from_frame::<FromHub>(text.as_str()).expect("it decodes")
    else {
        panic!("the hub must refuse a frame it cannot read, not ignore it: {text}")
    };
    assert!(
        reason.contains("does not know"),
        "and the refusal has to say so: {reason}"
    );
}
