//! A follower of a quiet job is never left in silence.
//!
//! The defect this file closes is a *wrong* answer, not a slow one. A job that
//! is **running** and printing nothing sent its follower nothing at all — no
//! output, no note, no verdict until the end — so the connection carried no
//! bytes for the whole of a quiet build. Through a reverse proxy with an idle
//! timeout (the hub sits behind Cloudflare) that connection is closed under
//! the follower, `herdctl` sees the stream end with no verdict, and it exits
//! 125 on a job that succeeded. A merger reading 125 concludes the work never
//! ran.
//!
//! A **queued** job looks safe — E-04 re-explains the wait, so frames keep
//! flowing — and is not: that cadence is an operator's setting, sixty seconds
//! by default where this one is fifteen, and a job the hub has not yet found a
//! reason for is restated never. So the hub breathes on every job that is not
//! over, and these four tests are the four halves of that:
//!
//! 1. a follower of a quiet **running** job keeps receiving frames, on a
//!    clock, and then gets the right verdict and exit code;
//! 2. a follower of a **queued** job whose reason is ten minutes from being
//!    restated gets them too;
//! 3. a follower of a job that has **ended** gets none — its stream is over;
//! 4. `herdctl` prints **nothing** for the frame: E-02's framing — the
//!    `ran on:` line, the job's own bytes, the verdict — survives a job that
//!    was quiet for eight of them.
//!
//! The quiet job waits on a **gate file** rather than on a number of seconds.
//! A job that sleeps for a fixed time is a race between the test and the
//! clock, and the test losing that race does not fail: it passes, having
//! proved nothing. Here the job stays quiet until this test says otherwise,
//! so "how long was the follower quiet for" is the test's decision and not
//! the scheduler's.

mod real_herd;

use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, FromHub, JobOutcome, LabelSet, Priority, Role, ToHub, PROTOCOL_VERSION,
};
use real_herd::{job_output, Herd, PATIENCE};
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The liveness cadence these tests run the hub at, in seconds.
///
/// The 15 s of [`herd_proto::STILL_THERE_EVERY`] is the default and the figure
/// a deployment runs; this is the same mechanism on a shorter clock, because a
/// test that waited out the default would be a test CI deletes. It is the
/// hub's own policy rather than a constant for exactly that reason — the
/// behaviour is the behaviour whatever the number is.
const BREATH_EVERY: &str = "0.2";

/// The same figure as a duration, for the arithmetic below.
const BREATH: Duration = Duration::from_millis(200);

/// How many breaths this test waits for before it lets the job finish.
///
/// Eight, so that the quiet stretch it measures is over a second long —
/// several times the cadence, which is what makes "it is periodic" an
/// assertion rather than a coincidence. Waiting for eight *frames* is waiting
/// on a condition, never on a sleep: a loaded host makes this test slower and
/// never makes it pass for the wrong reason.
const BREATHS_WANTED: usize = 8;

/// A job that produces not one byte until this test opens the gate, and then
/// prints one word and succeeds.
///
/// The silence is the whole point: no output means no `LogChunk`, and a
/// running job means no explained wait, so without the liveness frame this
/// connection carries nothing whatever between the placement and the verdict.
fn quiet_until(gate: &Path) -> String {
    format!(
        "while [ ! -e {} ]; do sleep 0.05; done; printf done",
        gate.display()
    )
}

/// **The defect, closed.** A job that runs for over a second without printing
/// anything keeps its follower hearing from the hub, and then gives it the
/// verdict and the exit code.
///
/// The assertion that matters is the third one: the **longest silence** on
/// that connection. Counting frames alone would pass on a burst of eight
/// breaths in one millisecond followed by twenty minutes of nothing, which is
/// the bug. What a proxy's idle timeout measures is the gap, so that is what
/// is measured here.
#[tokio::test(flavor = "multi_thread")]
async fn a_quiet_running_job_keeps_its_follower_hearing_from_the_hub() {
    let herd = Herd::new();
    let _hub = herd
        .start_hub_with("hub", &["--still-there-every", BREATH_EVERY])
        .await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    let gate = herd.logs.path().join("open-the-gate");
    let mut socket = requester_socket(&herd).await;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: herdctl::Declared::default().asked(
                vec!["/bin/sh".to_owned(), "-c".to_owned(), quiet_until(&gate)],
                LabelSet::default(),
                Priority::Step,
            ),
        },
    )
    .await;

    // Every frame, with the moment it arrived: the gaps are the evidence.
    let mut heard: Vec<(Instant, FromHub)> = Vec::new();
    let mut breaths = 0usize;
    while breaths < BREATHS_WANTED {
        // Bounded per frame rather than over the whole loop, so that **this**
        // is the sentence a failure prints: the defect being guarded against
        // is a connection that carries nothing, and "nothing arrived for
        // fifty cadences" is exactly what it looks like from here.
        let frame = tokio::time::timeout(BREATH * 50, next(&mut socket))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "the hub sent {breaths} liveness frame(s) and then went quiet under a \
                     running job that was printing nothing; a follower of a job that is not \
                     over must hear something every {BREATH:?}.\n{}",
                    herd.everything_said()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "the hub closed the connection under a running job.\n{}",
                    herd.everything_said()
                )
            });
        if matches!(frame, FromHub::StillThere { .. }) {
            breaths += 1;
        }
        // A quiet job must not have produced output, and a running one is not
        // waiting: either would mean this test measured the wrong silence.
        assert!(
            !matches!(frame, FromHub::LogChunk { .. } | FromHub::JobLog { .. }),
            "this job prints nothing until the gate opens, so the silence being measured \
             would not be silence: {frame:?}"
        );
        heard.push((Instant::now(), frame));
    }

    // Let it finish, and take the verdict off the same connection.
    std::fs::write(&gate, "go").expect("the gate opens");
    let outcome = loop {
        let frame = next(&mut socket).await.unwrap_or_else(|| {
            panic!(
                "the hub closed the connection before the verdict.\n{}",
                herd.everything_said()
            )
        });
        heard.push((Instant::now(), frame.clone()));
        if let FromHub::JobFinished { outcome, .. } = frame {
            break outcome;
        }
    };

    // One: it was placed, and the line E-02 is built from came before any of
    // the job's own bytes and before the verdict — a liveness frame has not
    // pushed itself in front of it.
    let placed = heard
        .iter()
        .position(|(_, frame)| matches!(frame, FromHub::RanOn { .. }))
        .unwrap_or_else(|| panic!("the follower was never told where the job ran: {heard:?}"));
    let first_byte = heard
        .iter()
        .position(|(_, frame)| {
            matches!(
                frame,
                FromHub::LogChunk { .. } | FromHub::JobLog { .. } | FromHub::JobFinished { .. }
            )
        })
        .unwrap_or_else(|| panic!("the job never said anything at all: {heard:?}"));
    assert!(
        placed < first_byte,
        "E-02's line comes before the job's first byte and before the verdict (D109): \
         {heard:?}"
    );

    // Two: the breaths are spread over the quiet stretch rather than bunched.
    let breathed_at: Vec<Instant> = heard
        .iter()
        .filter(|(_, frame)| matches!(frame, FromHub::StillThere { .. }))
        .map(|(at, _)| *at)
        .collect();
    assert_eq!(breathed_at.len(), BREATHS_WANTED, "{heard:?}");
    let spread = breathed_at[BREATHS_WANTED - 1] - breathed_at[0];
    // The hub fires a tick early on purpose, so a gap lands in
    // `[cadence - tick, cadence]` with the tick a quarter of the cadence —
    // three quarters of a cadence at the very least, and two thirds asserted
    // so that the figure is a bound and not a knife edge. A burst would be
    // two orders of magnitude under it.
    let least = (BREATH * 2 / 3) * (BREATHS_WANTED as u32 - 1);
    assert!(
        spread >= least,
        "{BREATHS_WANTED} liveness frames arrived inside {spread:?}, which is a burst and \
         not a cadence of {BREATH:?}: at least {least:?} was expected.\n{heard:?}"
    );

    // Three, and this is the one the bug would fail: **no long silence**.
    // Without the liveness frame this connection carries nothing at all from
    // the placement to the verdict, which is a gap as long as the build.
    let longest = heard
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .max()
        .unwrap_or(Duration::MAX);
    // Ten cadences: loose enough that a loaded CI host does not fail it,
    // and strictly false of a connection that carried nothing for a second
    // and a half.
    let allowed = BREATH * 10;
    assert!(
        longest <= allowed,
        "the follower heard nothing for {longest:?} while its job was running; a quiet link \
         must carry a frame every {BREATH:?} and never go {allowed:?} without one.\n{heard:?}"
    );
    assert!(
        longest < spread / 2,
        "the longest silence ({longest:?}) is most of the run ({spread:?}), so the follower \
         was effectively silent after all.\n{heard:?}"
    );

    // And the answer itself is the job's own, which is what the 125 was
    // standing in for.
    assert_eq!(outcome, JobOutcome::Exited { code: 0 }, "{heard:?}");
}

/// A **queued** job's follower is kept warm too, and a queued job is not safe
/// by itself.
///
/// The tempting reading of the fix is "queued jobs are fine, E-04 already
/// talks to them". It is not true twice over: E-04's restatement runs at a
/// figure an operator chooses — sixty seconds by default, four cadences — and
/// a job the hub has not yet found a *reason* for is restated never. Either is
/// a link carrying nothing.
///
/// So this hub restates at ten minutes and breathes at the test cadence, and
/// the job waits on a capability something outside Herd is holding (E-26) —
/// which is the only honest way to make a job stand still: the label *exists*,
/// so the job is placeable and queued rather than refused as impossible, and
/// it waits until this test removes the lock.
#[tokio::test(flavor = "multi_thread")]
async fn a_queued_job_whose_reason_is_not_restated_is_kept_warm_anyway() {
    let herd = Herd::new();
    // Ten minutes between restatements, so E-04's clock cannot be what keeps
    // this connection alive. The one note it sends at the start is the hub
    // answering the submission, not the clock coming round.
    let _hub = herd
        .start_hub_with(
            "hub",
            &[
                "--wait-restatement",
                "600",
                "--still-there-every",
                BREATH_EVERY,
            ],
        )
        .await;
    // A runner has to be *there*, and the capability has to exist: a hub with
    // no runner that could ever take the job refuses the submission outright
    // (D13) instead of queueing it, and a refusal is not a wait.
    let lock = herd.state.path().join("held-by-something-outside-herd");
    std::fs::write(&lock, "held").expect("the lock file is written");
    let capabilities = herd.logs.path().join("capabilities.json");
    std::fs::write(
        &capabilities,
        format!(
            r#"{{"capabilities":[{{"name":"gated","kind":{{"type":"shared"}},
               "busy_probe":{{"type":"lock_file","path":"{}"}}}}]}}"#,
            lock.display()
        ),
    )
    .expect("the capability file is written");
    let _runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &[
            "--capabilities",
            capabilities.to_str().expect("a utf-8 path"),
            "--no-detect",
            "--probe-interval",
            "50",
        ],
    );
    herd.wait_until("the runner has joined", || {
        herd.said("hub").contains("has joined")
    })
    .await;

    let mut socket = requester_socket(&herd).await;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: herdctl::Declared::default().asked(
                vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "printf never".to_owned(),
                ],
                LabelSet::of("gated").expect("a label"),
                Priority::Step,
            ),
        },
    )
    .await;

    let mut breaths = 0usize;
    let mut heard: Vec<FromHub> = Vec::new();
    while breaths < BREATHS_WANTED {
        let frame = tokio::time::timeout(BREATH * 50, next(&mut socket))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "the hub sent {breaths} liveness frame(s) to a job that is queued with ten \
                     minutes to its next restatement, and then went quiet.\n{}",
                    herd.everything_said()
                )
            })
            .unwrap_or_else(|| panic!("the hub closed the connection: {heard:?}"));
        if matches!(frame, FromHub::StillThere { .. }) {
            breaths += 1;
        }
        assert!(
            !matches!(frame, FromHub::JobFinished { .. }),
            "the capability is held, so nothing can have run this job: {heard:?}"
        );
        heard.push(frame);
    }
}

/// A follower of a job that has **ended** gets no liveness frame: its stream
/// is over.
///
/// The frame exists to keep a link warm under work that is still happening,
/// and a verdict is the last word about a job. A breath after it would be the
/// hub talking about something finished — and, worse, a frame arriving after
/// the one a reader stops at.
#[tokio::test(flavor = "multi_thread")]
async fn a_follower_of_a_job_that_has_ended_gets_no_liveness_frame() {
    let herd = Herd::new();
    let _hub = herd
        .start_hub_with("hub", &["--still-there-every", BREATH_EVERY])
        .await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf over").await;
    herd.wait_until("the job is over", || {
        herd.record(&job)["state"]["type"].as_str() == Some("finished")
    })
    .await;

    // A `herdctl wait` on a job that is already over, by hand: the record,
    // the log, the verdict — and then nothing.
    let mut socket = requester_socket(&herd).await;
    send(
        &mut socket,
        &ToHub::WaitForJob {
            job_id: herd_proto::JobId::new(&job),
        },
    )
    .await;

    let mut heard: Vec<FromHub> = Vec::new();
    loop {
        let frame = next(&mut socket)
            .await
            .unwrap_or_else(|| panic!("the wait was answered with nothing: {heard:?}"));
        let done = matches!(frame, FromHub::JobFinished { .. });
        assert!(
            !matches!(frame, FromHub::StillThere { .. }),
            "a job that has ended gets no liveness frame: {heard:?}"
        );
        heard.push(frame);
        if done {
            break;
        }
    }

    // Many cadences of nothing. The connection is still open — the hub holds
    // it for whatever this requester asks next — so what is asserted is that
    // the hub says nothing on it, not that it closes it.
    let quiet_for = BREATH * 15;
    let after = tokio::time::timeout(quiet_for, socket.next()).await;
    assert!(
        after.is_err(),
        "the hub sent {after:?} about a job that was already over; after the verdict there \
         is nothing left to keep warm.\n{}",
        herd.everything_said()
    );
}

/// **E-02 survives it.** `herdctl` prints nothing at all for a liveness frame.
///
/// Not "it does not crash": the output a requester sees is one line saying
/// where the job ran, then the job's own bytes unchanged, then the verdict,
/// and the first consumer's tooling is anchored on that. A job quiet for eight
/// cadences gets eight of these frames, and the stdout below must be
/// byte-for-byte what it would have been without them.
///
/// It is also the regression test for the shape of the fix. `herdctl`'s
/// fallback for a frame it does not know is a `bail!` — "the hub sent X, which
/// a requester cannot use" — so a frame that was not handled **by name** would
/// not be quietly ignored here, it would end the run with an error and no
/// verdict. Which is the 125 this whole change exists to remove.
#[tokio::test(flavor = "multi_thread")]
async fn herdctl_prints_nothing_for_a_liveness_frame() {
    let herd = Herd::new();
    let _hub = herd
        .start_hub_with("hub", &["--still-there-every", BREATH_EVERY])
        .await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    // Quiet for a second and a half, which is eight cadences at the figure
    // above, then one word. A fixed sleep is safe here because the assertion
    // is about `herdctl`'s output and not about a moment: a slower host sends
    // *more* breaths, never fewer.
    let output = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "sleep 1.5; printf done"])
        .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "the job succeeded, so `herdctl run` exits with its code and not with a wrapper's: \
         {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        herd.everything_said()
    );
    // `job_output` insists on E-02's first line having E-02's shape, and
    // hands back everything after it: the job's own bytes, and nothing a
    // liveness frame put there.
    assert_eq!(
        job_output(&output.stdout),
        "done",
        "a liveness frame must not reach stdout, and must not disturb the framing around \
         the job's own output (E-02)"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("still_there") && !stderr.contains("cannot use"),
        "a liveness frame must not reach stderr either, as a note or as a complaint: \
         {stderr}"
    );
}

/// A requester connection, through the handshake, with nothing submitted.
///
/// By hand rather than through `herdctl`, because what these tests count is
/// *frames*, and `herdctl` is the thing that is meant to print nothing for
/// one of them. The hub and the runner are the real binaries either way.
async fn requester_socket(herd: &Herd) -> Socket {
    let (mut socket, _response) = tokio_tungstenite::connect_async(herd.url())
        .await
        .expect("the hub accepts a connection");
    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: herdctl::requester_version().to_owned(),
        role: Role::Requester,
        token: None,
        name: Some("a-follower-counting-frames".to_owned()),
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: Default::default(),
        host: herd_proto::HostSize::default(),
        preference: 0,
        load_milli: None,
        switch: herd_proto::Switch::On,
    };
    send(&mut socket, &hello).await;
    match next(&mut socket).await {
        Some(FromHub::Welcome { .. }) => socket,
        other => panic!("the hub answered the handshake with {other:?}"),
    }
}

async fn send(socket: &mut Socket, message: &ToHub) {
    socket
        .send(Message::text(to_frame(message).expect("encodes")))
        .await
        .expect("the frame goes out");
}

/// The next frame the hub sends, decoded. `None` is the connection closing.
async fn next(socket: &mut Socket) -> Option<FromHub> {
    loop {
        let message = tokio::time::timeout(PATIENCE, socket.next())
            .await
            .expect("the hub answers rather than going quiet")?
            .expect("the frame reads");
        match message {
            Message::Text(text) => {
                return Some(
                    from_frame(text.as_str()).unwrap_or_else(|e| panic!("{text} decodes: {e}")),
                )
            }
            Message::Close(_) => return None,
            // The protocol carries text (D4); a ping is the library's own.
            _ => continue,
        }
    }
}
