//! ORCH-081, against the real binaries: `SIGTERM` stops the hub, it does not
//! kill it.
//!
//! Until this existed, `SIGTERM` reached the hub with its default
//! disposition and killed it exactly as `SIGKILL` does — so every ordinary
//! restart of the hub was, to the store, a crash. Every instance of this hub
//! is in docker compose at the first consumer's site, which means
//! `docker compose stop`, a host reboot and a redeploy all arrive that way.
//! `killed_hub.rs` proves the crash costs nothing; this file proves the
//! ordinary stop is no longer one.
//!
//! **What a clean stop is here**, and the three parts are asserted
//! separately below:
//!
//! 1. the hub stops accepting connections and submissions, and starts no
//!    further dispatch pass, so no attempt is numbered onto a runner it is
//!    about to disconnect from;
//! 2. it lets the dispatch pass already in flight finish, for at most
//!    [`herd_hub::STOP_WITHIN`], and **says which of the two happened** — the
//!    exit code is the whole of that answer, 0 for a stop that finished
//!    inside the bound and non-zero for one that gave up;
//! 3. it leaves running jobs alone. They are their runners', the runners
//!    hold their verdicts until a hub records them (E-44, D34), and the hub
//!    that comes back takes them.
//!
//! **What this file deliberately does not try to assert.** That a dispatch
//! pass was really in flight at the instant the signal landed. At any queue
//! depth a test can build in a few seconds a pass is sub-millisecond, so
//! arranging that collision from outside the process would be a guess
//! dressed as an assertion. The bounded wait itself is proved where it can
//! be proved exactly — `server.rs`'s
//! `the_stop_waits_for_the_pass_in_flight_and_gives_up_at_its_bound`, which
//! holds the claim lock from a thread and checks both answers. What is
//! proved here is the outcome against real processes: the exit code, the
//! surviving runner, and a state root that is whole.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with `killed_hub.rs`.

mod real_herd;

use std::time::{Duration, Instant};

use real_herd::{Herd, Process, PATIENCE};

/// A label no runner in these tests offers, so a job needing it stands in the
/// queue until something takes it off the disk. The honest way to have a
/// queue that does not move while the test looks at it.
const NOBODY_OFFERS_THIS: &str = "a-label-no-runner-here-has";

/// How many such jobs. Enough that "the queue survived" is a statement about
/// a queue and not about one record, and few enough that submitting them is
/// seconds rather than minutes on a shared host.
const QUEUED: usize = 12;

/// The loose outer bound on the wall clock of a stop.
///
/// **Not the bound the product promises**, which is `STOP_WITHIN` and is
/// asserted by the exit code: the hub measures its own wait with an
/// `Instant` and reports an overrun by exiting non-zero, so a test that
/// re-measured it from outside would only be adding process teardown, the
/// signal's delivery and this host's scheduler to a figure the hub already
/// knows exactly. This is here to catch a hub that does not come back at
/// all, and it is three times the bound plus room so that a loaded shared
/// host cannot make it red.
const NO_LONGER_THAN: Duration = Duration::from_secs(30);

/// Ask a process to stop, and do not mind if it has already gone.
///
/// `Process::ask_it_to_stop` panics on a failed `kill`, which is right
/// everywhere else and wrong for the test below: sending a second `SIGTERM`
/// is a race with the first one being honoured, and a hub that had already
/// finished stopping is the *good* outcome of that race, not a harness
/// failure.
fn ask_again(process: &mut Process) {
    let Some(pid) = process.child.id() else {
        return;
    };
    let Ok(pid) = rustix::process::Pid::from_raw(pid as i32).ok_or(()) else {
        return;
    };
    let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
}

/// Everything a reader must still be able to do with the state root.
///
/// Not "the files look plausible": each of these is something the hub itself
/// or the first consumer does on every start (E-53, E-54). A temporary left
/// behind would be a write cut in half, a record that will not parse would
/// be a record replaced non-atomically, and either is the thing a stop path
/// exists to make impossible.
fn the_state_root_is_whole(herd: &Herd) {
    for (path, bytes) in herd.state_bytes() {
        assert!(
            !path.contains(".tmp-"),
            "a temporary was left in the state root at {path}: a write was cut in half"
        );
        if path.ends_with("job.json") {
            let record: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| panic!("{path} is not whole json: {e}"));
            assert!(
                record["state"]["type"].is_string(),
                "{path} parses but says no state: {record}"
            );
        }
        if path.ends_with("events.jsonl") {
            for (which, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
                serde_json::from_str::<serde_json::Value>(line)
                    .unwrap_or_else(|e| panic!("{path} line {} is not whole json: {e}", which + 1));
            }
        }
    }
}

/// The test ORCH-081 exists for: an ordinary stop is no longer a crash.
#[tokio::test(flavor = "multi_thread")]
async fn a_hub_asked_to_stop_leaves_cleanly_and_the_store_is_whole() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // ---- In flight: a job really running on a real runner, with a real
    // requester following it, and a queue standing behind both.
    let running = herd
        .submit("printf 'before the stop\\n'; sleep 6; printf 'after it\\n'; exit 7")
        .await;
    let mut follower = herd.start_wait(&running);
    herd.wait_until("the job is running and its first line is recorded", || {
        herd.record(&running)["state"]["type"] == "running"
            && herd.log(&running).contains("before the stop")
    })
    .await;
    let mut queued = Vec::new();
    for which in 0..QUEUED {
        queued.push(
            herd.submit_needing(NOBODY_OFFERS_THIS, "step", &format!("true {which}"))
                .await,
        );
    }
    herd.wait_until("the whole queue is judged and on the disk", || {
        queued
            .iter()
            .all(|job| herd.record(job)["state"]["type"] == "queued")
    })
    .await;

    // ---- The stop.
    let began = Instant::now();
    hub.ask_it_to_stop();
    let how_it_went = tokio::time::timeout(PATIENCE, hub.gone())
        .await
        .expect("a hub asked to stop comes back");
    let took = began.elapsed();

    // **The bound, as the hub itself measured it.** A stop whose wait for
    // the pass in flight ran past `STOP_WITHIN` exits non-zero and says so,
    // so a zero here is the product's own statement that it stopped inside
    // its bound — a stronger and far less flaky assertion than re-timing it
    // from out here.
    assert_eq!(
        how_it_went.code(),
        Some(0),
        "a hub asked to stop must stop, not die of it, and must say it stopped inside its \
         bound by leaving with 0 (it took {took:?}): {}",
        herd.said("first")
    );
    assert!(
        took < NO_LONGER_THAN,
        "the hub took {took:?} to stop, which is past any bound this test can call patience"
    );
    let said = herd.said("first");
    assert!(
        said.contains("asked to stop (SIGTERM)"),
        "the hub must name the signal it stopped on: {said}"
    );
    assert!(
        said.contains("No new connection and no new submission is accepted"),
        "the hub must say it closed the door before it waited: {said}"
    );
    assert!(
        said.contains("it stopped cleanly"),
        "a clean stop must say so, with how long the pass in flight took: {said}"
    );

    // ---- Nothing the hub did on its way out touched the runner or its job.
    assert!(
        runner.alive(),
        "the runner must outlive a hub that was merely stopped: {}",
        herd.said("runner")
    );
    assert_eq!(
        herd.record(&running)["state"]["type"],
        "running",
        "a job running on a runner is not cancelled by its hub stopping: it belongs to the \
         runner, which holds its verdict until a hub records it (E-44)"
    );
    for job in &queued {
        assert_eq!(
            herd.record(job)["state"]["type"],
            "queued",
            "a job queued when the hub was asked to stop is still queued for the next hub, \
             never half-assigned to a runner this hub was about to leave"
        );
    }
    the_state_root_is_whole(&herd);

    // ---- What the requester that was following saw, written down because
    // it is the one place a clean stop is still indistinguishable from a
    // crash.
    //
    // The hub exits, so the socket goes with the process and the follower
    // reads a reset — the same thing `killed_hub.rs` describes for a kill.
    // `herdctl` therefore ends on its *no verdict* path: 125, the reason on
    // stderr, and the job's id printed, which is the whole of the fallback
    // contract (`herdctl::NO_VERDICT_CODE`). That is honest and it is
    // recoverable — the wait by id below gets the real code — and it is not
    // a sentence naming the stop. Saying that would mean either a frame for
    // it, which is a protocol change and out of bounds (`PROTOCOL_VERSION`
    // is frozen at 2, D73), or a graceful WebSocket close on the way out,
    // which is possible and is a separate piece of work: it changes which
    // branch a *runner* takes on the ending link, and D104 records that
    // branch as one measurement says never happens today.
    //
    // Asserted rather than left unexamined, so that a future change to it
    // is a decision and not a surprise.
    let followed = tokio::time::timeout(PATIENCE, follower.gone())
        .await
        .expect("the follower comes back once its hub is gone");
    let follower_said = herd.said("wait");
    assert_eq!(
        followed.code(),
        Some(herdctl::NO_VERDICT_CODE as i32),
        "a follower whose hub went must answer 'no verdict' — never success, and never the \
         job's own code invented: {follower_said}"
    );
    assert!(
        follower_said.contains("cannot give you a verdict"),
        "and must say so in the sentence the fallback contract names: {follower_said}"
    );
    assert!(
        follower_said.contains(&format!("job-id: {running}")),
        "and must have printed the id, which is what makes coming back for the verdict \
         possible at all (E-03, E-05): {follower_said}"
    );

    // ---- The hub comes back on the same state root and the same port, and
    // everything that was in flight is still there.
    let mut second = herd.start_hub("second").await;
    let started = herd.said("second");
    assert!(
        started.contains(&format!("{} job(s) not over", QUEUED + 1)),
        "the hub that comes back inherits the running job and the whole queue: {started}"
    );
    assert!(
        started.contains("0 quarantined"),
        "nothing the stop wrote may be a record the next hub cannot read: {started}"
    );

    // The runner redials and hands over the verdict it was holding.
    herd.wait_until("the verdict the runner held is recorded", || {
        herd.record(&running)["state"]["type"] == "finished"
    })
    .await;
    assert_eq!(
        herd.record(&running)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 7}),
        "the job's own exit code must survive its hub being stopped under it"
    );
    assert!(
        herd.log(&running).contains("after it"),
        "and so must the output it produced after the hub went: {}",
        herd.log(&running)
    );
    let waited = herd.herdctl(&["wait", &running]).await;
    assert_eq!(
        waited.status.code(),
        Some(7),
        "a requester coming back by id alone gets the job's own code (E-03): {}",
        String::from_utf8_lossy(&waited.stderr)
    );

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// ORCH-081: a second `SIGTERM` during a stop must not make anything worse.
///
/// An operator who does not see the hub go in the first second sends another
/// one, and a deployment script may send several. The rule is that the stop
/// already under way is **never hurried**: hurrying it is precisely what
/// would turn it back into the crash it exists to stop being. So the handler
/// notes the extra signal in a line and does nothing else — and, because the
/// handler is installed, none of them kills the process the way the default
/// disposition would.
///
/// A burst rather than exactly two. Whether any one of them lands while the
/// drain is still running is a race with the drain being microseconds long,
/// and a test that asserted on winning that race would be flaky by
/// construction. A burst makes the race irrelevant: whatever the interleaving
/// was, the hub still has to exit cleanly and still has to have lost nothing,
/// and that is what is asserted.
#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_stop_signals_is_still_one_clean_stop() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let running = herd
        .submit("printf 'before the burst\\n'; sleep 6; exit 5")
        .await;
    herd.wait_until("the job is running", || {
        herd.record(&running)["state"]["type"] == "running"
    })
    .await;
    let queued = herd
        .submit_needing(NOBODY_OFFERS_THIS, "step", "true")
        .await;
    herd.wait_until("the queued job is judged", || {
        herd.record(&queued)["state"]["type"] == "queued"
    })
    .await;

    hub.ask_it_to_stop();
    for _ in 0..200 {
        ask_again(&mut hub);
    }
    let how_it_went = tokio::time::timeout(PATIENCE, hub.gone())
        .await
        .expect("a hub asked to stop many times still comes back");

    assert_eq!(
        how_it_went.code(),
        Some(0),
        "a burst of SIGTERMs is still one clean stop, and none of them may kill the hub the \
         way the default disposition would: {}",
        herd.said("first")
    );
    assert_eq!(
        herd.record(&queued)["state"]["type"],
        "queued",
        "the queued job is untouched by however many signals arrived"
    );
    assert!(
        runner.alive(),
        "the runner outlives the burst too: {}",
        herd.said("runner")
    );
    the_state_root_is_whole(&herd);

    // And the hub that comes back is a hub, not a wreck.
    let mut second = herd.start_hub("second").await;
    herd.wait_until("the verdict the runner held is recorded", || {
        herd.record(&running)["state"]["type"] == "finished"
    })
    .await;
    assert_eq!(
        herd.record(&running)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 5}),
        "nothing was lost to the burst"
    );

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}
