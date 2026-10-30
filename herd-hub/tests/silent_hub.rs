//! **A hub that is up and says nothing**, and the sentence the runner says
//! about it — the conclusion `HUB_SILENCE_LIMIT` exists to produce, which
//! until this file was asserted by nothing anywhere.
//!
//! `herd_runner::HUB_SILENCE_LIMIT` is 60 s, and from it the runner concludes
//! that a link whose socket is open carries no hub at all: *"the hub has said
//! nothing for 60s, so this link is dead even though its socket is open"*
//! (`herd-runner/src/lib.rs`, `next_message`). A search for that sentence
//! across every test directory in this workspace returned nothing, which is
//! why this file exists. What the consumer meets on the day it breaks is a
//! fleet gone quiet, in one of two directions and both bad: every runner in a
//! redial loop against a hub that is perfectly alive, or every runner pinned
//! to a hub that is gone, running nothing and counted as nothing.
//!
//! ## The shape of the absence, which is the whole fixture
//!
//! Not a hub that is killed — `killed_hub.rs` covers that, and a killed hub
//! closes the connection under the runner with a reset. Not a socket that is
//! closed either: `Ok(None)` out of `next_message` is a *different*
//! conclusion down a different arm (the hub closed the connection), and a
//! runner that only ever got that answer would still hold a dead link for
//! ever. This is the third case and the one nothing could reach: the
//! connection stays open, the TCP stack is happy, the handshake completed,
//! the runner's own health reports keep leaving — and **no frame ever comes
//! back**. A container stopped without a FIN and a NAT table that forgot both
//! look exactly like this from the runner's end.
//!
//! ## Which side is a real process, and why that way round
//!
//! The thing under test is the runner's own code, so **the runner is the real
//! `herd-runner` binary, as a process**, built by [`real_herd::binaries`] in
//! this test's own run so that no stale binary can stand in for it. The
//! silent party is a **hand-written listener in this test process**: it binds
//! loopback, completes the WebSocket handshake, answers `Hello` with
//! `Welcome`, counts every health report that arrives — and answers none of
//! them. The real hub cannot be the silent party, because answering is
//! exactly what it does: making it stop would mean a production code path for
//! a hub that goes mute, which is not something to add in order to test
//! something else. Saying nothing is the one thing a hand-built peer does
//! better than the real binary, so that is the half it plays.
//!
//! The two sides are therefore asymmetric on purpose, and that asymmetry is
//! stated here rather than left to be assumed: a reader who needs the real
//! hub as well needs a different test, not this one.
//!
//! ## Why this file is in `herd-hub/tests` and not beside the runner
//!
//! It binds a listening socket, and
//! `no_module_of_the_runner_can_bind_a_socket`
//! (`herd-runner/tests/confinement.rs`) forbids the string `TcpListener`
//! anywhere under the `herd-runner` crate, its tests included — invariant 3,
//! and it is right: a runner dials out and binds nothing. A stand-in for a
//! **hub** therefore belongs beside the hub's tests, which already bind
//! loopback ports (`unknown_frames.rs`, `fake_hub_flood.rs`). The runner
//! under test is the real one either way.
//!
//! ## The figures, and where they come from
//!
//! The threshold is this test's, not the clock's: it is passed to the real
//! binary on its command line, and then **read back out of the runner's own
//! words** — the conclusion names the duration it applied, so a runner that
//! applied some other figure fails the assertion rather than passing it
//! slowly. The deployed 60 s is read back too, out of the one surface the
//! runner publishes it on (`herd-runner --help`), and compared with the
//! constant: see [`the_runner_publishes_the_deployed_silence_limit`]. Neither
//! figure is typed twice.
//!
//! **What the shortened threshold costs in honesty, and what it buys.** At
//! the deployed 60 s this test would take a minute of wall clock per
//! assertion; `announced_job_ceiling.rs` meets the same wall and answers it
//! with an `#[ignore]`d full-length twin at 171 s. Here the threshold is a
//! plain runtime figure — `config.hub_silence_limit` is handed to
//! `next_message` and nothing about the code path differs between 3 s and
//! 60 s — so a shortened threshold in the gate exercises the same arithmetic
//! on the same branch, and the deployed figure is asserted separately out of
//! `--help`. An `#[ignore]`d 60 s twin would add a test that nobody runs and
//! whose skip this branch's `gate.sh` does not report by name, which is
//! worse than no twin at all. So: shortened in the gate, deployed figure
//! asserted from the binary, and the cost stated here.
//!
//! **One thing that would silently defeat the limit, checked while here.**
//! `next_message` restarts its timeout on every frame it loops past,
//! `Message::Ping` and `Message::Pong` included, so a WebSocket-level
//! keepalive on either side would reset the clock and make this conclusion
//! unreachable. Neither peer sends pings — `grep -n "Ping\|Pong"` over
//! `herd-runner/src` and `herd-hub/src` finds only the three arms that skip
//! them — and `tokio-tungstenite` only answers pings it receives, so with
//! nobody sending one the silence is a silence. This test would notice if
//! that changed: the conclusion would simply never come.

mod real_herd;

use std::path::{Path, PathBuf};
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

/// The silence limit this test gives the real runner, in seconds.
///
/// **It and [`HEALTH_EVERY_SECONDS`] are the deployed pair divided by 20**,
/// and that is deliberate: the audit that found this gap names *cadence ×
/// limit* as the scale-sensitive quantity, so the ratio is what has to
/// survive the shortening. 60 / 15 is 4, and 3 / 0.75 is 4, so the runner
/// here lets exactly four of its own unanswered reports pass before it
/// concludes — the same number of reports the deployed figures let pass, on
/// a clock twenty times faster. The margin against
/// `herd_proto::silence_threshold_refusal` scales with it (3 s against the
/// 2.25 s it needs, as 60 s against 45 s), and
/// [`the_runner_publishes_the_deployed_silence_limit`] asserts the binary
/// really does accept this pair.
const SILENCE_LIMIT_SECONDS: f64 = 3.0;

/// How often the runner under test says it is alive, in seconds: the
/// deployed `HEALTH_EVERY` divided by the same 20.
const HEALTH_EVERY_SECONDS: f64 = 0.75;

/// The state root the hand-written hub welcomes a runner into. Any string:
/// the runner settles a fresh data root against it and has nothing to settle.
const STATE_ROOT: &str = "a-hub-that-says-nothing";

/// The silence limit as a `Duration`, built the way the real binary builds it
/// from the same command-line figure (`herd-runner/src/main.rs`, `seconds`).
///
/// This is what makes the conclusion below a read-back rather than a
/// restatement: the runner prints `{silence_limit:?}` of whatever it really
/// applied, and this is the only place the expected text comes from.
fn silence_limit() -> Duration {
    Duration::try_from_secs_f64(SILENCE_LIMIT_SECONDS).expect("a positive, finite figure")
}

/// Whether the hand-written hub answers what the runner sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answers {
    /// The fixture: every frame is read, counted, and left unanswered.
    Nothing,
    /// The control: health reports are answered, as the real hub answers
    /// them. Proves the conclusion below is about silence and not about this
    /// harness.
    Health,
}

/// A hub that is up: it binds, it completes the handshake, it welcomes a
/// runner — and then it is as quiet as it was told to be.
struct HandWrittenHub {
    url: String,
    /// How many times the runner has dialled. The second one is the act this
    /// file exists to assert: a conclusion that is printed and not acted on
    /// leaves the fleet exactly as quiet.
    dials: Arc<AtomicUsize>,
    /// Health reports that arrived — proof the link was carrying traffic
    /// outward while nothing came back, rather than merely being open.
    reports: Arc<AtomicUsize>,
    /// When the **first** connection was welcomed, which is the instant the
    /// runner's own silence clock starts from.
    first_welcome: Arc<Mutex<Option<Instant>>>,
    _serving: tokio::task::JoinHandle<()>,
}

impl HandWrittenHub {
    async fn bind(answers: Answers) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a free loopback port");
        let url = format!(
            "ws://{}",
            listener.local_addr().expect("a bound listener has one")
        );
        let dials = Arc::new(AtomicUsize::new(0));
        let reports = Arc::new(AtomicUsize::new(0));
        let first_welcome = Arc::new(Mutex::new(None));
        let serving = tokio::spawn({
            let dials = Arc::clone(&dials);
            let reports = Arc::clone(&reports);
            let first_welcome = Arc::clone(&first_welcome);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    dials.fetch_add(1, Ordering::SeqCst);
                    let reports = Arc::clone(&reports);
                    let first_welcome = Arc::clone(&first_welcome);
                    tokio::spawn(async move {
                        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                            return;
                        };
                        while let Some(Ok(frame)) = socket.next().await {
                            let Message::Text(text) = frame else { continue };
                            match from_frame::<ToHub>(text.as_str()) {
                                Ok(ToHub::Hello { .. }) => {
                                    let welcome = to_frame(&FromHub::Welcome {
                                        protocol_version: PROTOCOL_VERSION,
                                        hub_version: env!("CARGO_PKG_VERSION").to_owned(),
                                        state_root: STATE_ROOT.to_owned(),
                                        version_note: None,
                                    })
                                    .expect("a welcome encodes");
                                    if socket.send(Message::text(welcome)).await.is_err() {
                                        return;
                                    }
                                    // The runner's silence clock starts here,
                                    // and only the first link's start is kept:
                                    // that is the one the conclusion is about.
                                    let mut when =
                                        first_welcome.lock().expect("no panic holds this lock");
                                    when.get_or_insert_with(Instant::now);
                                }
                                Ok(ToHub::Health { .. }) => {
                                    reports.fetch_add(1, Ordering::SeqCst);
                                    if answers == Answers::Health {
                                        let said = to_frame(&FromHub::HealthRecorded {})
                                            .expect("an acknowledgement encodes");
                                        if socket.send(Message::text(said)).await.is_err() {
                                            return;
                                        }
                                    }
                                    // `Answers::Nothing` is the fixture, and
                                    // this is the whole of it: the report is
                                    // read off the socket and counted, and
                                    // nothing goes back.
                                }
                                // Capabilities, swept collections, anything
                                // else a runner announces: read and ignored.
                                // A hub that says nothing says nothing to
                                // these either.
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
            first_welcome,
            _serving: serving,
        }
    }

    fn dials(&self) -> usize {
        self.dials.load(Ordering::SeqCst)
    }

    fn reports(&self) -> usize {
        self.reports.load(Ordering::SeqCst)
    }

    fn first_welcome(&self) -> Option<Instant> {
        *self.first_welcome.lock().expect("no panic holds this lock")
    }
}

/// The real `herd-runner`, as a process, with its two streams on disk so they
/// can be read while it is still running.
struct RealRunner {
    child: Child,
    logs: tempfile::TempDir,
    /// Held for the life of the runner: a runner locks its data root (D32),
    /// and this is what removes the directory afterwards.
    _data_root: tempfile::TempDir,
}

impl RealRunner {
    /// Started against `hub_url` with the pair of figures this file is about
    /// and nothing else: no job ever runs here, so no memory or timeout
    /// option changes what is being asserted.
    fn start(hub_url: &str, label: &str) -> Self {
        let logs = tempfile::tempdir().expect("a temporary directory for the runner's output");
        let data_root = tempfile::tempdir().expect("a temporary data root");
        let child = Command::new(binary("herd-runner"))
            .arg("--hub")
            .arg(hub_url)
            .arg("--name")
            .arg(label)
            .arg("--data-root")
            .arg(data_root.path())
            .arg("--health-every")
            .arg(HEALTH_EVERY_SECONDS.to_string())
            .arg("--hub-silence-limit")
            .arg(SILENCE_LIMIT_SECONDS.to_string())
            .stdout(Stdio::from(
                std::fs::File::create(logs.path().join("out")).expect("a file"),
            ))
            .stderr(Stdio::from(
                std::fs::File::create(logs.path().join("err")).expect("a file"),
            ))
            // So that a test which fails or panics still takes its runner
            // with it: nothing of this file's making outlives it.
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

    fn said(&self) -> String {
        said_by(self.logs().as_path())
    }

    /// Ends the process this test started, by its own pid and no other, and
    /// reaps it so nothing is left stopped behind the test.
    async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

/// Everything the runner has printed so far, both streams together: the
/// conclusion goes to stderr and the redial to stdout, and the assertions
/// here are about the pair.
fn said_by(logs: &Path) -> String {
    let mut text = String::new();
    for stream in ["out", "err"] {
        text.push_str(&std::fs::read_to_string(logs.join(stream)).unwrap_or_default());
    }
    text
}

/// Wait for something the runner has to do, failing with what it said rather
/// than hanging on the very regression this file exists to catch.
async fn wait_until(what: &str, runner: &mut RealRunner, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        if let Ok(Some(status)) = runner.child.try_wait() {
            panic!(
                "the runner ended ({status}) before {what}; it said:\n{}",
                runner.said()
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "{what} did not happen within {PATIENCE:?}; the runner said:\n{}",
        runner.said()
    );
}

/// **A runner against a hub that is up and says nothing concludes the link is
/// dead, and dials again.**
///
/// Two assertions, and the second is the one the consumer feels. The
/// conclusion is the runner's own sentence, naming the threshold it really
/// applied; the act is a **second connection arriving at the listener**,
/// which is the difference between a fleet that recovers when the hub comes
/// back and a fleet pinned to a hub that is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runner_whose_hub_says_nothing_concludes_the_link_is_dead_and_dials_again() {
    let hub = HandWrittenHub::bind(Answers::Nothing).await;
    let mut runner = RealRunner::start(&hub.url, "the-real-runner");
    let logs = runner.logs();

    // It got in: the handshake completed and the link is serving. Without
    // this, a conclusion about silence could be a conclusion about a hub that
    // was never reached, which is D104's distinction and a different arm.
    wait_until("the runner joins the hand-written hub", &mut runner, || {
        hub.dials() >= 1 && hub.first_welcome().is_some()
    })
    .await;
    let welcomed = hub
        .first_welcome()
        .expect("the wait above is what this returns");

    // The sentence, with the figure read back out of it. A runner that
    // applied the deployed 60 s, or any figure but the one its command line
    // gave it, does not print this line.
    let conclusion = format!(
        "the hub has said nothing for {:?}, so this link is dead even though its socket is open",
        silence_limit()
    );
    wait_until(
        "the runner says a silent link is dead, naming the limit it was given",
        &mut runner,
        || said_by(&logs).contains(&conclusion),
    )
    .await;
    let concluded = Instant::now();

    // And it waited for the whole of it. Polled, so this reading can only be
    // later than the truth — which is what makes it a floor worth asserting:
    // a runner that gave up early could not clear it.
    let waited = concluded.saturating_duration_since(welcomed);
    assert!(
        waited >= silence_limit(),
        "the runner concluded after {waited:?}, which is less than the {:?} it was given: a \
         silence limit that fires early is a fleet redialling against a hub that is alive",
        silence_limit()
    );

    // The link was carrying traffic outward the whole time it carried nothing
    // back. This is what makes the fixture a *silent hub* rather than a
    // broken socket.
    assert!(
        hub.reports() >= 1,
        "the runner's health reports must have been arriving while nothing came back; the hub \
         counted {} and the runner said:\n{}",
        hub.reports(),
        runner.said()
    );

    // The act. Printed first —
    wait_until(
        "the runner says it is dialling the same hub again",
        &mut runner,
        || said_by(&logs).contains(&format!("dialling {} again in", hub.url)),
    )
    .await;
    // — and then done, which is the half a log line cannot stand in for.
    wait_until(
        "a second connection reaches the hand-written hub",
        &mut runner,
        || hub.dials() >= 2,
    )
    .await;

    runner.stop().await;
}

/// **The control: a hub that answers keeps the same runner on the same
/// link.**
///
/// Without this, the assertion above could be about anything — a handshake
/// this harness gets wrong, a runner that redials on a timer, a data root it
/// cannot lock. Here the one and only difference is that the health reports
/// are answered, and the conclusion must then be **unreachable**: no such
/// sentence anywhere in the runner's output, and one dial, after twice the
/// limit has passed. It is also the red run of the test above made
/// permanent — the detector is shown to be silent only when there is nothing
/// to detect.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hub_that_answers_keeps_the_same_runner_on_the_same_link() {
    let hub = HandWrittenHub::bind(Answers::Health).await;
    let mut runner = RealRunner::start(&hub.url, "the-real-runner");

    wait_until("the runner joins the answering hub", &mut runner, || {
        hub.dials() >= 1 && hub.first_welcome().is_some()
    })
    .await;

    // Twice the threshold: whatever was going to fire has had two chances to.
    let twice_over = silence_limit() * 2;
    tokio::time::sleep(twice_over).await;

    let said = runner.said();
    assert!(
        !said.contains("has said nothing for"),
        "a hub that answers is not a silent hub, and {twice_over:?} of it must produce no such \
         conclusion:\n{said}"
    );
    assert_eq!(
        hub.dials(),
        1,
        "and the runner must still be on its first link after {twice_over:?}; it said:\n{said}"
    );
    // At this cadence that is a dozen reports and acknowledgements: the link
    // was working, not merely open.
    assert!(
        hub.reports() >= 3,
        "the answering hub must have answered real reports, not an idle socket; it counted {}",
        hub.reports()
    );

    runner.stop().await;
}

/// **The deployed figure, read out of the binary rather than retyped.**
///
/// `--hub-silence-limit` is declared with `default_value_t =
/// HUB_SILENCE_LIMIT.as_secs_f64()`, so the runner's own `--help` is the one
/// surface on which it publishes the limit it ships with. This asserts that
/// what it publishes is the constant — the two cannot drift apart without
/// this failing — and that the pair of figures the tests above hand the
/// binary is one `herd_proto::silence_threshold_refusal` really accepts, so
/// that a shortened threshold in the gate is a legal configuration and not a
/// fixture only a test could run.
#[test]
fn the_runner_publishes_the_deployed_silence_limit() {
    let help = std::process::Command::new(binary("herd-runner"))
        .arg("--help")
        .output()
        .expect("herd-runner runs");
    assert!(help.status.success(), "`herd-runner --help` must succeed");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();
    let at = help
        .find("--hub-silence-limit")
        .unwrap_or_else(|| panic!("the runner must publish its silence limit:\n{help}"));
    let published = &help[at..];
    let deployed = format!(
        "[default: {}]",
        herd_runner::HUB_SILENCE_LIMIT.as_secs_f64()
    );
    assert!(
        published.contains(&deployed),
        "the published default must be the constant the runner concludes from, {deployed}:\n{}",
        &published[..published.len().min(400)]
    );

    // And the shortened pair the two tests above run on is legal: the binary
    // refuses a limit that cannot outlast three of its own reports, and a
    // test whose figures it would refuse proves nothing at all.
    assert!(
        herd_proto::silence_threshold_refusal(
            "the limit these tests give the real runner",
            silence_limit(),
            "the cadence these tests give it",
            Duration::try_from_secs_f64(HEALTH_EVERY_SECONDS).expect("a positive, finite figure"),
        )
        .is_none(),
        "the shortened threshold in this file has to be one the real binary accepts"
    );

    // And the shortened pair is the deployed pair's own shape rather than two
    // convenient numbers: both ratios are read out of the constants, so the
    // runner under test lets exactly as many of its own unanswered reports
    // pass before concluding as it does in the field.
    let deployed =
        herd_runner::HUB_SILENCE_LIMIT.as_secs_f64() / herd_proto::HEALTH_EVERY.as_secs_f64();
    let shortened = SILENCE_LIMIT_SECONDS / HEALTH_EVERY_SECONDS;
    assert!(
        (deployed - shortened).abs() < 1e-9,
        "the shortened pair lets {shortened} reports pass and the deployed pair {deployed}: a \
         faster clock is a shortening, a different ratio is a different fixture"
    );
}
