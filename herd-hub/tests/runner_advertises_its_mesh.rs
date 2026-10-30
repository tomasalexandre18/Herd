//! **What the real `herd-runner` puts in `Hello.mesh_endpoint`**, read off
//! the wire (invariant 3, invariant 4, D10, D11, E-12).
//!
//! Until this file, that field was `mesh_endpoint: None` in the one hello the
//! binary builds, unconditionally and whatever `--mesh-listen` it was given.
//! The consequence was not a missing line in a log: `Holder::endpoint` was
//! `None` for every real runner, `herd_runner::fetch::reachable_holder`
//! refused every holder as outbound-only, and the hub — correctly — minted no
//! capability against an address nobody had advertised. So **no pack could
//! move between two machines at all**, and the half that was missing was one
//! field.
//!
//! ## The hazard the field carries, which is why it is asserted here
//!
//! `--mesh-listen 127.0.0.1` means port `0`: the **kernel** picks the port,
//! at bind time, in a task started beside the redial loop. So on a cold start
//! the address is not known when the hello is built — and
//! `herd_proto::PROTOCOL_VERSION` 3 carries **no frame that revises a mesh
//! endpoint later** (invariant 4). A hello that guessed would therefore be
//! uncorrectable for the life of the link: the hub would mint capabilities
//! against a port nothing holds, and every peer sent there would be refused
//! for a reason no operator could see.
//!
//! The runner answers that by awaiting the bind before its first dial
//! (`herd_runner::run_until`, `endpoint::Advertised`). What is asserted below
//! is the observable consequence of that choice, and nothing about how it is
//! reached:
//!
//! 1. a runner with **no** mesh flag advertises nothing, and says so;
//! 2. a runner given `--mesh-listen 127.0.0.1` advertises a **real** port —
//!    not the nought it asked for — and a socket is really there on it;
//! 3. a **redial** advertises the same endpoint and rebinds nothing, which is
//!    the promise the frozen protocol makes the runner keep.
//!
//! ## Which side is a real process, and why that way round
//!
//! The thing under test is what the runner's own startup puts on the wire, so
//! the runner is the real `herd-runner` **binary, as a process**, built by
//! [`real_herd::binaries`] in this test's own run. The hub is a hand-written
//! listener in this test process, for the reason `silent_hub.rs` gives at
//! length: reading one field out of one frame is the one thing a hand-built
//! peer does better than the real binary, and the real hub would have to be
//! asked for the field back through a surface that does not exist.
//!
//! The file lives in `herd-hub/tests` and not beside the runner because it
//! binds a listening socket, and `no_module_of_the_runner_can_bind_a_socket`
//! (`herd-runner/tests/confinement.rs`) forbids the name `TcpListener`
//! anywhere under the `herd-runner` crate, its tests included. A stand-in for
//! a **hub** belongs beside the hub's tests, which already bind loopback
//! ports.
//!
//! ## The one address these tests may use
//!
//! Loopback, and it is a deliberate clause of `herd-runner/src/mesh.rs` and
//! not an accident: `permitted` returns `Ok` for a loopback address before
//! the routing table is consulted at all, *"because it is reachable from
//! nothing off this machine, and it is the only mesh a single workstation
//! running the two-runner end-to-end has"*. **No test here widens a guard to
//! make an address advertisable** — if a guard refused the address, the guard
//! would be right and the address wrong (D11, D222).

mod real_herd;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, ToHub, PROTOCOL_VERSION};
use real_herd::{binary, PATIENCE};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::Message;

/// The state root the hand-written hub welcomes a runner into. Any string: a
/// fresh data root has nothing to settle against it.
const STATE_ROOT: &str = "a-hub-that-only-reads-the-hello";

/// What the runner is told to listen on, in the spelling that makes the port
/// the kernel's to choose. **The whole hazard is in the missing `:PORT`.**
const BARE_LOOPBACK: &str = "127.0.0.1";

/// The redial figures these tests hand the real binary, in seconds.
///
/// The deployed pair is 500 ms doubling to 30 s
/// (`herd_runner::Backoff::default`), and a redial is what assertion 3 is
/// about — so it is shortened here rather than waited out. Nothing about the
/// code path differs: `Backoff::delay` does the same arithmetic on both
/// figures, and what is asserted is the *content* of the second hello and
/// never how long it took to arrive.
const REDIAL_FIRST_SECONDS: &str = "0.05";
const REDIAL_CAP_SECONDS: &str = "0.2";

/// Whether the hand-written hub keeps the first link or drops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstLink {
    /// Kept: one hello is all this test needs.
    Kept,
    /// Welcomed and then dropped, so that the runner redials and a **second**
    /// hello can be compared with the first. Dropped after the welcome and
    /// not before it, so the runner's own backoff treats this as a link that
    /// served rather than a dial that was refused.
    DroppedAfterTheWelcome,
}

/// A hub that is up, welcomes a runner, and keeps every hello it was sent.
struct HandWrittenHub {
    url: String,
    /// One entry per dial, in order: what that hello's `mesh_endpoint` was.
    /// `None` is the field absent on the wire, which is the outbound-only
    /// runner.
    hellos: Arc<Mutex<Vec<Option<String>>>>,
    _serving: tokio::task::JoinHandle<()>,
}

impl HandWrittenHub {
    async fn bind(first_link: FirstLink) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free loopback port");
        let url = format!(
            "ws://{}",
            listener.local_addr().expect("a bound listener has one")
        );
        let hellos = Arc::new(Mutex::new(Vec::new()));
        let dials = Arc::new(AtomicUsize::new(0));
        let serving = tokio::spawn({
            let hellos = Arc::clone(&hellos);
            let dials = Arc::clone(&dials);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let which = dials.fetch_add(1, Ordering::SeqCst);
                    let hellos = Arc::clone(&hellos);
                    tokio::spawn(async move {
                        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                            return;
                        };
                        while let Some(Ok(frame)) = socket.next().await {
                            let Message::Text(text) = frame else { continue };
                            // One frame of the handshake is read, and
                            // everything else a runner announces — health,
                            // capabilities, the packs it holds — is read off
                            // the socket and left unanswered. Nothing here is
                            // about what a hub answers.
                            let Ok(ToHub::Hello { mesh_endpoint, .. }) =
                                from_frame::<ToHub>(text.as_str())
                            else {
                                continue;
                            };
                            // Recorded **before** the welcome, so a hub that
                            // failed to answer could not be read as a runner
                            // that failed to say.
                            hellos
                                .lock()
                                .expect("no panic holds this lock")
                                .push(mesh_endpoint);
                            let welcome = to_frame(&FromHub::Welcome {
                                protocol_version: PROTOCOL_VERSION,
                                hub_version: env!("CARGO_PKG_VERSION").to_owned(),
                                state_root: STATE_ROOT.to_owned(),
                                version_note: None,
                                keeper_for: Vec::new(),
                            })
                            .expect("a welcome encodes");
                            if socket.send(Message::text(welcome)).await.is_err() {
                                return;
                            }
                            if first_link == FirstLink::DroppedAfterTheWelcome && which == 0 {
                                // The link goes. The runner concludes the hub
                                // closed it and redials, which is the act
                                // assertion 3 is about.
                                return;
                            }
                        }
                    });
                }
            }
        });
        Self {
            url,
            hellos,
            _serving: serving,
        }
    }

    fn hellos(&self) -> Vec<Option<String>> {
        self.hellos
            .lock()
            .expect("no panic holds this lock")
            .clone()
    }

    /// Wait until `wanted` hellos have arrived, and hand them all back.
    ///
    /// Bounded by [`real_herd::PATIENCE`], and the sentence on the way out
    /// names what the runner had printed by then: a hello that never comes is
    /// a startup that stopped somewhere, and the runner's own words are where
    /// that is readable.
    async fn hellos_until(&self, wanted: usize, runner: &RealRunner) -> Vec<Option<String>> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let seen = self.hellos();
            if seen.len() >= wanted {
                return seen;
            }
            assert!(
                Instant::now() < deadline,
                "only {} of {wanted} hello(s) arrived in {PATIENCE:?}. The runner said:\n{}",
                seen.len(),
                runner.said()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The real `herd-runner`, as a process, with both streams on disk so they
/// can be read while it is still running.
struct RealRunner {
    child: Child,
    logs: tempfile::TempDir,
    /// Held for the life of the runner: a runner locks its data root (D32),
    /// and this is what removes the directory afterwards.
    _data_root: tempfile::TempDir,
}

impl RealRunner {
    /// Started against `hub_url` with `mesh` as its `--mesh-listen`, when it
    /// has one, and nothing else that bears on what it advertises.
    fn start(hub_url: &str, label: &str, mesh: Option<&str>) -> Self {
        let logs = tempfile::tempdir().expect("a temporary directory for the runner's output");
        let data_root = tempfile::tempdir().expect("a temporary data root");
        let mut command = Command::new(binary("herd-runner"));
        command
            .arg("--hub")
            .arg(hub_url)
            .arg("--name")
            .arg(label)
            .arg("--data-root")
            .arg(data_root.path())
            .arg("--redial-first")
            .arg(REDIAL_FIRST_SECONDS)
            .arg("--redial-cap")
            .arg(REDIAL_CAP_SECONDS);
        if let Some(mesh) = mesh {
            command.arg("--mesh-listen").arg(mesh);
        }
        let child = command
            .stdout(Stdio::from(
                std::fs::File::create(logs.path().join("out")).expect("a file"),
            ))
            .stderr(Stdio::from(
                std::fs::File::create(logs.path().join("err")).expect("a file"),
            ))
            // So that a test which fails or panics still takes its runner with
            // it: nothing of this file's making outlives it.
            .kill_on_drop(true)
            .spawn()
            .expect("herd-runner starts");
        Self {
            child,
            logs,
            _data_root: data_root,
        }
    }

    fn logs(&self) -> PathBuf {
        self.logs.path().to_path_buf()
    }

    /// Everything the runner has printed so far, both streams together.
    fn said(&self) -> String {
        let mut text = String::new();
        for stream in ["out", "err"] {
            text.push_str(&std::fs::read_to_string(self.logs().join(stream)).unwrap_or_default());
        }
        text
    }

    /// Ends the process this test started, by its own pid and no other, and
    /// reaps it so nothing is left stopped behind the test.
    async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

/// **A runner with no mesh flag advertises nothing** (D10, invariant 3).
///
/// The outbound-only runner, which is the default and the common case: it
/// dials out, runs every job, listens on nothing, and is refused as a
/// *holder* by name — `herd-runner/src/fetch.rs`'s
/// *"every one of the N runner(s) holding pack P is outbound-only"*, and the
/// hub's own matching refusal. That refusal is the right answer and not a
/// degradation, which is why its premise is asserted here rather than assumed
/// from the absence of a flag.
#[tokio::test]
async fn a_runner_with_no_mesh_flag_advertises_nothing() {
    let hub = HandWrittenHub::bind(FirstLink::Kept).await;
    let runner = RealRunner::start(&hub.url, "outbound-only", None);
    let hellos = hub.hellos_until(1, &runner).await;
    assert_eq!(
        hellos[0], None,
        "a runner given no {} advertised an address: {hellos:?}",
        "--mesh-listen"
    );
    // And it says so in its own voice, because "this machine serves nothing"
    // is a fact an operator should not have to infer from a line that is not
    // there.
    let said = runner.said();
    assert!(
        said.contains("outbound-only"),
        "the runner must say it is outbound-only: {said}"
    );
    runner.stop().await;
}

/// **A runner advertises the port it really bound, and the same one on a
/// redial** (invariant 3, invariant 4, D10, D11).
///
/// Three clauses in one test, because split up each could pass while the trio
/// did not — a runner could advertise a plausible port it never bound, or
/// bind the right one and advertise a different one on its second hello.
///
/// * the advertised port is **not** the `0` the flag asked for;
/// * a socket is really there on it, reached from this test process;
/// * and the **second** hello, after the hub drops the first link, carries
///   the same endpoint, with the same socket still open — so nothing rebound
///   and no peer was ever sent to a port that had moved.
#[tokio::test]
async fn a_runner_advertises_the_port_it_bound_and_the_same_one_on_a_redial() {
    let hub = HandWrittenHub::bind(FirstLink::DroppedAfterTheWelcome).await;
    let runner = RealRunner::start(&hub.url, "a-holder", Some(BARE_LOOPBACK));
    let hellos = hub.hellos_until(2, &runner).await;

    let first = hellos[0]
        .clone()
        .unwrap_or_else(|| panic!("the first hello advertised nothing: {}", runner.said()));
    let at: std::net::SocketAddr = first
        .parse()
        .unwrap_or_else(|_| panic!("{first} is not an address a peer could dial"));
    assert_eq!(
        at.ip().to_string(),
        BARE_LOOPBACK,
        "the runner advertised an address it was not given: {first}"
    );
    assert_ne!(
        at.port(),
        0,
        "the runner advertised the nought it asked for rather than the port the kernel gave \
         it, which is a port no peer can dial: {first}"
    );
    // A socket is really there. This is the clause that separates an
    // advertisement from a sentence about one, and it is made from outside
    // the runner's process.
    tokio::net::TcpStream::connect(at)
        .await
        .unwrap_or_else(|error| {
            panic!("the runner advertised {first} and nothing is listening there: {error}")
        });

    // The redial. The same endpoint, because the protocol has no frame that
    // revises one — and the same socket, because nothing rebound.
    assert_eq!(
        hellos[1].as_deref(),
        Some(first.as_str()),
        "the runner's second hello advertised a different endpoint from its first: {hellos:?}"
    );
    tokio::net::TcpStream::connect(at)
        .await
        .unwrap_or_else(|error| {
            panic!("the socket at {first} did not survive the redial: {error}")
        });

    // And the runner's own startup line carries the same figure, so that the
    // address an operator reads and the address the hub is told cannot drift
    // apart.
    let said = runner.said();
    assert!(
        said.contains(&format!("serves pack content at {first}")),
        "the runner must say what its handshake advertises, and say the same address: {said}"
    );
    runner.stop().await;
}
