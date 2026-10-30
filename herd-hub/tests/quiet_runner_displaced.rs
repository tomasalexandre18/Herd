//! **H1b against the real binaries: the runner wearing this name has gone
//! quiet, so the newcomer takes it** — with the incumbent's socket still
//! open.
//!
//! `Peers::register_runner` has three ways out when a name is already held:
//! the same instance dialling again, a twin told to stand by, and this one —
//! *"runner {name} has said nothing for {quiet_for:?}"*, which kicks the
//! connection the hub is holding and admits the newcomer. It fires when the
//! incumbent `reports_seen > 0 && quiet_for > health_is_stale_after()`, which
//! is `lost_after / 2`: a runner that reported at least once and then stopped,
//! **while its link is still up**. The startup audit of 2026-10-30 found it
//! was the one branch in that function with no test of any kind —
//! `rg "has said nothing for"` over every test directory returned zero hits,
//! and `capabilities.rs:769-778` deliberately waits for the hub to let go of
//! the dead connection first, which takes the easy path and says so.
//!
//! What the consumer would see without it, in the audit's own words: *a
//! suspended laptop whose runner is displaced by its own twin, both processes
//! briefly believing they hold one attempt; or a name that cannot be replaced
//! for thirty seconds.* Thirty because `lost_after` is 60 s deployed. Which
//! of the two it is depends on a `>` and a `/ 2`, and nothing was reading
//! them.
//!
//! **Why nothing produced this state before.** A runner that reported once
//! and then stopped, still connected, is not something a test can ask a
//! healthy process for: killing it closes the socket, and then the hub's
//! watchdog frees the name and the newcomer walks in through the easy path.
//! The production way to make a process that is plainly there and answers
//! nothing is to suspend it, which is what a laptop lid, a frozen host or a
//! `cgroup` freezer does — and `owner_control.rs` already has the device, for
//! the same reason (D99). So the incumbent here is `SIGSTOP`ped, by pid, and
//! `SIGCONT`ed before this test ends.
//!
//! **The ordering is this test's and not the scheduler's, which is the whole
//! difficulty.** The displacement lives in a window: the dial must arrive
//! after `quiet_for` passes `lost_after / 2` and before the watchdog drops
//! the link at `lost_after`. Sleeping through the first half and hoping for
//! the second is the shape that let D221 through — a fixture that was green
//! three runs running while measuring the wrong state. So neither bound is
//! guessed:
//!
//! - *that the incumbent reported* is read off `health_reports_seen` in the
//!   hub's own status document, not assumed from a cadence;
//! - *that it has been quiet long enough* is read off `last_health_at` in
//!   that same document — the very `Timestamp` the hub writes on the line
//!   below the `Instant` its `quiet_for` is measured from
//!   (`State::record_health`), so the test is reading the hub's own copy of
//!   the clause it is about to take;
//! - *that the watchdog had not already acted* is asserted after the fact,
//!   from the hub's own output: `register_runner` can only print *"has said
//!   nothing for"* while it still holds the incumbent, and the watchdog's own
//!   *"nothing has come back on it for"* must be absent. A machine so slow
//!   that the watchdog won the race makes this test **red**, never green —
//!   the line it waits for is printed on one path only.
//!
//! Run five times in a row on 2026-10-30 for exactly that reason, and the
//! five runs are in the task's handoff.
//!
//! **And the ordering device was shown to be load-bearing rather than
//! trusted for its silence.** With [`PAST_THE_THRESHOLD`] raised from 500 ms
//! to 4500 ms — which puts the dial past `lost_after` instead of past
//! `lost_after / 2` — this test went **red**, and the hub's own output in the
//! failure said why: *"dropping the connection to runner the-real-runner:
//! nothing has come back on it for 8.318273887s"*. The watchdog had freed the
//! name, the runner was gone from the status document, and the wait below ran
//! out. So the window this test aims at is real, the line it refuses to see
//! is a line the hub really does print in the other case, and a run that
//! loses the race fails rather than passing for the wrong reason. If this
//! test ever becomes flaky, that is the experiment to repeat: the margin is
//! the only figure in it worth moving.

mod real_herd;

use std::time::Duration;

use herd_hub::{RunnerNow, Status, Timestamp};
use real_herd::Herd;

/// The name both processes wear. One name is what the branch is about: a
/// second machine under a second name is a fleet, and nothing is displaced.
const NAME: &str = "the-real-runner";

/// How long this hub waits before it calls a runner lost, in seconds.
///
/// Eight, which is a window of four: the branch opens at `lost_after / 2`
/// and the watchdog closes it at `lost_after`, so the dial has about three
/// and a half seconds of room after the wait below. The deployed figure is
/// 60 and this is the same mechanism on a shorter clock — the hub's policy
/// rather than a constant for exactly this reason.
const LOST_AFTER_SECONDS: f64 = 8.0;

/// How far past the hub's staleness threshold the test waits before dialling.
///
/// Half a second, measured against the hub's own record of its last report
/// rather than against a sleep, so it is a margin on a reading and not a
/// guess at a duration.
const PAST_THE_THRESHOLD: Duration = Duration::from_millis(500);

#[tokio::test(flavor = "multi_thread")]
async fn a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin() {
    let herd = Herd::new();
    let mut hub = herd
        .start_hub_losing_runners_after("hub", LOST_AFTER_SECONDS)
        .await;
    let mut incumbent = herd.start_runner_called(NAME, "incumbent");
    herd.wait_for_runner().await;

    // The hub's own figures, off the document it publishes: the threshold
    // this test is about is `lost_after / 2`, and `lost_after` is read back
    // from the hub rather than assumed to be what the command line asked
    // for. If either ever stops meaning this, the assertion is here and not
    // in a comment.
    let lost_after = Duration::from_millis(the_document(&herd).hub.lost_after_ms);
    assert_eq!(
        lost_after,
        Duration::from_secs_f64(LOST_AFTER_SECONDS),
        "the hub serves the threshold this test asked it for"
    );
    let stale_after = lost_after / 2;

    // ---- The state nothing else produces: reported once, then silent.
    //
    // The report is waited for on the hub's count of them, because
    // `reports_seen > 0` is half of the branch's own condition: a runner
    // that has only just joined is not a runner that has gone quiet, and the
    // pair of figures is the only way to tell them apart.
    herd.wait_until(
        "the hub has counted a health report from the incumbent",
        || the_runner(&herd).is_some_and(|runner| runner.health_reports_seen > 0),
    )
    .await;
    incumbent.signal_without_waiting(rustix::process::Signal::STOP);

    // Quiet for longer than the hub's own staleness threshold, measured on
    // the hub's own record of when it last heard from this runner. Re-read
    // on every turn, so a report that was already in flight when the process
    // stopped moves the reading forward instead of invalidating it — which is
    // also why the count is not compared across the stop: a frame already in
    // the socket when the process froze is one the hub may legitimately read
    // afterwards, and `last_health_at` says so rather than hiding it.
    herd.wait_until(
        "the hub's record of the incumbent's last report is older than its threshold",
        || {
            the_runner(&herd).is_some_and(|runner| {
                Timestamp::now().since(runner.last_health_at) > stale_after + PAST_THE_THRESHOLD
            })
        },
    )
    .await;
    // Both clauses of the branch, read off the hub's own document in the
    // breath before the twin dials: it has reported, and it has been quiet
    // for longer than the threshold. This is the state the audit says
    // nothing in the suite produces.
    let quiet = the_runner(&herd).expect("the incumbent still holds the name");
    assert!(
        quiet.health_reports_seen > 0,
        "the branch needs a runner that reported at least once; this one reported \
         {} times",
        quiet.health_reports_seen
    );
    assert!(
        Timestamp::now().since(quiet.last_health_at) > stale_after,
        "and one quiet for longer than {stale_after:?}, which it is not: last heard \
         from {:?} ago",
        Timestamp::now().since(quiet.last_health_at)
    );

    // ---- The twin dials, under the same name, on a data root of its own.
    //
    // A root of its own because a runner locks its data root for the life of
    // the process (D32) and the incumbent is suspended, not gone — which is
    // also what the deployed case looks like: two processes, one name, one of
    // them wedged.
    let mut newcomer = herd.start_runner_rooted(NAME, "newcomer", "newcomer");
    let displaced = format!(
        "dropping the connection this hub was holding, because runner {NAME} has said \
         nothing for"
    );
    herd.wait_until("the hub says the name's holder has said nothing", || {
        herd.said("hub").contains(&displaced)
    })
    .await;
    let said = herd.said("hub");

    // The branch, in the hub's own words. It is printed on one path only —
    // `register_runner`, with the incumbent still in `peers.runners` — so
    // this line *is* the evidence that the displacement happened rather than
    // the hub having let go first.
    assert!(
        said.contains(&displaced),
        "the hub must say whose link it dropped and why: {said}"
    );
    // The other half of the same claim, and the one that keeps this test
    // honest: had the watchdog dropped the link at `lost_after` before the
    // twin dialled, the name would have been free, nothing would have been
    // displaced, and this test would have proved what `capabilities.rs`
    // already proves.
    assert!(
        !said.contains("nothing has come back on it for"),
        "the watchdog freed the name first, so the quiet-incumbent branch was never \
         taken and this run measured the wrong state: {said}"
    );
    // Nor was the newcomer told to wait, which is the third way out of
    // `register_runner` and the one a twin gets while the incumbent is still
    // being heard from.
    assert!(
        !said.contains("asked to stand by"),
        "the newcomer was told to stand by, so the hub did not read the incumbent as \
         quiet: {said}"
    );
    assert_eq!(
        said.matches(&format!("runner {NAME} has joined")).count(),
        2,
        "both processes joined under the one name: the incumbent, then the twin that \
         replaced it: {said}"
    );

    // ---- And the newcomer is really serving, not merely admitted.
    let ran = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'the twin ran it'"])
        .await;
    assert_eq!(
        ran.status.code(),
        Some(0),
        "a job submitted after the displacement runs on the twin: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        real_herd::job_output(&ran.stdout).trim(),
        "the twin ran it",
        "and its output is the twin's"
    );
    assert!(
        the_runner(&herd).is_some(),
        "the name is held by a runner the hub is hearing from: {said}"
    );

    // ---- Nothing is left suspended on a shared host.
    //
    // Before the signals that end the two processes, and by pid through the
    // handle this test started them with — never by name. A `SIGTERM` to a
    // stopped process is queued and not delivered, so a test that ended here
    // without the `SIGCONT` would leave a stopped `herd-runner` behind for
    // whoever looked at the host next.
    incumbent.signal_without_waiting(rustix::process::Signal::CONT);
    incumbent.signal(rustix::process::Signal::TERM).await;
    assert!(
        !incumbent.alive(),
        "the suspended runner was continued and then ended"
    );
    newcomer.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// The document this hub publishes on its state root, which must be there:
/// every figure this test reads is one the hub writes down.
fn the_document(herd: &Herd) -> Status {
    Status::read(herd.state.path())
        .expect("the status document reads")
        .expect("a serving hub has published one")
}

/// What the document says about the runner wearing the one name, if the hub
/// is holding a connection under it at all.
fn the_runner(herd: &Herd) -> Option<RunnerNow> {
    Status::read(herd.state.path())
        .ok()
        .flatten()
        .and_then(|status| {
            status
                .runners
                .into_iter()
                .find(|runner| runner.name == NAME)
        })
}
