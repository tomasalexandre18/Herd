//! `wss://` is attempted, not pretended (M16).
//!
//! Until this milestone `tokio-tungstenite` was pulled with its default
//! features, which compile **no TLS at all**: there was not one TLS crate in
//! `Cargo.lock`, and every `wss://` dial failed with tungstenite's
//! "TLS support not compiled in." before a byte left the machine. M16's own
//! "done when" asks for a job to run over `wss://`, and M17 — the owner's PC
//! joining from another machine — cannot happen over plain `ws://` across the
//! internet.
//!
//! ## Why this lives in the hub's tests
//!
//! The endpoint below binds a listening socket, and no file of `herd-runner`
//! may so much as name one: invariant 3 is guarded there by a test that reads
//! every source file of that crate. The hub is the side of Herd that listens,
//! so its test crate is where a socket belongs. What is dialled is still the
//! runner's own dial and `herdctl`'s own dial — the hub's tests already drive
//! both.
//!
//! ## What is asserted here, and what is not
//!
//! A real handshake needs a certificate this host trusts, and the honest way
//! to get one in a test is a self-signed certificate plus a trust
//! configuration — which would mean either a new Herd flag or a certificate
//! generator in the tree, and neither is in this milestone. So what is
//! asserted is the thing that was actually broken: that the client **speaks
//! TLS**.
//!
//! The proof is not a message the client printed about itself. A TCP endpoint
//! reads the first record the client sends and reports its header: byte
//! `0x16` is a TLS handshake record, which only a client with TLS compiled in
//! and a working crypto provider can produce — `ClientConfig::builder()`
//! panics when no provider is installed, so a ClientHello on the wire is also
//! the provider's receipt. The endpoint then answers with a fatal
//! `handshake_failure` alert, and the dial must fail *with that alert*: a TLS
//! protocol failure, never again a missing feature.
//!
//! A whole handshake was completed by hand once, against a public
//! certificate and the bundled roots — `herdctl --hub wss://example.com/
//! switch` answers `HTTP error: 200 OK`, which is a verified certificate and
//! a server that will not speak WebSocket. It is not a test, because a test that needs
//! the internet is a test that fails for reasons that are not Herd's
//! (invariant 5 wants a passing test, not a flaky one).

use std::path::Path;
use std::sync::Arc;

use herd_runner::testing::TempDir;
use herd_runner::{Confined, Confinement, Session};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What tungstenite says when no TLS is compiled in. The string this whole
/// file exists to make unreachable.
const WITHOUT_TLS: &str = "TLS support not compiled in";

/// A TLS endpoint of the shortest honest kind: it reads the client's first
/// record and answers with a fatal alert.
///
/// It reports the record header it saw, so the assertion is about what the
/// client put on the wire rather than about what the client said afterwards.
struct TlsEndpoint {
    /// The `wss://` URL of this endpoint.
    url: String,
    /// The first five bytes the client sent — a TLS record header.
    first_record: tokio::sync::oneshot::Receiver<Vec<u8>>,
}

impl TlsEndpoint {
    async fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free loopback port");
        let url = format!(
            "wss://{}/",
            listener
                .local_addr()
                .expect("a bound listener has an address")
        );
        let (tell, first_record) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let Ok((mut stream, _peer)) = listener.accept().await else {
                return;
            };
            let mut header = [0u8; 5];
            let read = stream.read_exact(&mut header).await;
            // A TLS alert record: type 21, record version TLS 1.2, two bytes
            // of payload, level `fatal` (2), description `handshake_failure`
            // (40). Hand-written rather than served by a TLS library, because
            // a test that needed a TLS server to prove TLS is compiled in
            // would be proving it with the thing under test.
            let _ = stream
                .write_all(&[0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 40])
                .await;
            let _ = stream.flush().await;
            let _ = tell.send(if read.is_ok() {
                header.to_vec()
            } else {
                Vec::new()
            });
            // Held open until the client closes: a socket dropped under a
            // client still writing its ClientHello would turn the alert into
            // a reset, and the assertion would be about a closed connection
            // rather than about TLS.
            let mut drained = Vec::new();
            let _ = stream.read_to_end(&mut drained).await;
        });
        Self { url, first_record }
    }

    /// The record header the client sent, once it has sent one.
    async fn client_hello(self) -> Vec<u8> {
        self.first_record
            .await
            .expect("the endpoint reports what it read")
    }
}

/// Both sentences a TLS failure must satisfy: it names the alert the peer
/// sent, and it is not the missing-feature message.
fn assert_tls_was_spoken(said: &str) {
    assert!(
        !said.contains(WITHOUT_TLS),
        "this dial did not even try TLS; the client is built without it: {said}"
    );
    assert!(
        said.to_lowercase().contains("alert"),
        "a dial that reached a TLS endpoint must fail with what that endpoint \
         said — a fatal alert — and not with anything else: {said}"
    );
}

/// `herdctl` dials `wss://`, reaches TLS, and fails with the peer's alert.
#[tokio::test]
async fn herdctl_dials_wss_and_fails_on_the_peer_s_alert() {
    let endpoint = TlsEndpoint::bind().await;
    let config = herdctl::Config {
        hub_url: endpoint.url.clone(),
        token: None,
        name: Some("m16-tls".to_owned()),
    };
    let error = herdctl::fleet_switches(&config)
        .await
        .expect_err("an endpoint that sends a fatal alert is not a hub");
    let said = format!("{error:#}");
    assert!(
        said.contains(&endpoint.url),
        "the failure must name what was dialled: {said}"
    );
    assert_tls_was_spoken(&said);

    let header = endpoint.client_hello().await;
    assert_eq!(
        header.first().copied(),
        Some(0x16),
        "the first record a TLS client sends is a handshake record (0x16); \
         this client sent {header:?}"
    );
    assert_eq!(
        header.get(1).copied(),
        Some(0x03),
        "a TLS record carries a 0x03xx version; this client sent {header:?}"
    );
}

/// And so does a runner, through its own dial — the one M17 needs.
#[tokio::test]
async fn a_runner_dials_wss_and_fails_on_the_peer_s_alert() {
    let endpoint = TlsEndpoint::bind().await;
    let temp = TempDir::new("tls-runner");
    let confined = {
        let _reaping = herd_runner::testing::one_reaper_at_a_time();
        Arc::new(
            Confined::prepare(Confinement::with_data_root(temp.path()))
                .expect("an unprivileged process with a writable root can confine"),
        )
    };
    let config = herd_runner::Config {
        hub_url: endpoint.url.clone(),
        name: Some("m16-tls-runner".to_owned()),
        ..herd_runner::Config::default()
    };
    let error = herd_runner::serve_once(config, Session::new(), confined)
        .await
        .expect_err("an endpoint that sends a fatal alert is not a hub");
    let said = format!("{error:#}");
    assert!(
        said.contains(&endpoint.url),
        "the failure must name what was dialled: {said}"
    );
    assert_tls_was_spoken(&said);

    let header = endpoint.client_hello().await;
    assert_eq!(
        header.first().copied(),
        Some(0x16),
        "a runner's dial must put a TLS handshake record on the wire; it sent \
         {header:?}"
    );
}

/// The lock file is where the absence was visible: no `rustls`, no
/// `native-tls`, no `openssl` of any kind meant `wss://` could not work
/// whatever the code said. It is also where a well-meaning cleanup of the
/// manifests would show up first.
#[test]
fn the_lock_file_carries_rustls_and_no_system_tls() {
    let lock = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace")
        .join("Cargo.lock");
    let text = std::fs::read_to_string(&lock)
        .unwrap_or_else(|error| panic!("{} cannot be read: {error}", lock.display()));
    for needed in ["name = \"rustls\"", "name = \"webpki-roots\""] {
        assert!(
            text.contains(needed),
            "{} has no {needed}, so this workspace has no TLS and no root \
             certificates to verify a hub against",
            lock.display()
        );
    }
    // The other half of the choice: no system library, so a runner drops onto
    // a machine without development headers and the Nix packaging stays plain.
    for forbidden in ["name = \"openssl-sys\"", "name = \"native-tls\""] {
        assert!(
            !text.contains(forbidden),
            "{} pulls {forbidden}: TLS here is rustls, which needs no system \
             library",
            lock.display()
        );
    }
}
