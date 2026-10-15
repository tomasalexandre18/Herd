//! M6 against the real binaries: the order jobs actually start in, and what
//! a `kill -9` does to the queue (E-18, E-20, E-43, E-05).
//!
//! Nothing here can be done in process. The order a merger starts in is the
//! order of two `AssignJob` frames the hub really sent, read off the hub's
//! own output; what a restart does to a job's accumulated aging needs a hub
//! that was killed outright and a state root that outlived it; and "two
//! callers, one result" is two `herdctl` processes exiting, which is the only
//! place an exit code exists at all (D20).
//!
//! The slot a queued job is waiting for is made and freed with a **lock
//! file**: the runner offers one capability with a `lock_file` busy probe,
//! so a job needing that capability waits while the file is there and starts
//! when it goes (E-26, D38). That is the smallest honest stand-in for a busy
//! machine — counted slots are E-22 and M7's — and it is how the first
//! consumer's own GPU lock works.

mod real_herd;

use std::time::Duration;

use real_herd::Herd;

/// How long a job waits before it counts as one level higher, in the tests
/// here that are about aging. Two seconds, so that ninety minutes of
/// behaviour takes six.
const AGING_STEP_IN_THESE_TESTS: &str = "2";

/// The capability every job here needs, and whose lock file is the slot.
const HELD: &str = "kvm";

/// A capability file offering [`HELD`], busy while `lock` exists.
fn capability_file(herd: &Herd, lock: &std::path::Path) -> std::path::PathBuf {
    let path = herd.state.path().join("capabilities.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"capabilities":[
                {{"name":"{HELD}","kind":{{"type":"shared"}},
                 "busy_probe":{{"type":"lock_file","path":"{}"}}}}
            ]}}"#,
            lock.display()
        ),
    )
    .expect("the capability file is written");
    path
}

/// E-43 and the by-hand check that goes with it: a merger's job submitted
/// **last** takes the next free slot, ahead of the lower-priority jobs that
/// were queued before it.
///
/// Two step jobs and a producer go in first and wait on the lock file; the
/// merger goes in last; the lock is released; the hub's own output says
/// which attempt it handed out first.
#[tokio::test(flavor = "multi_thread")]
async fn a_merger_submitted_last_takes_the_next_free_slot() {
    let herd = Herd::new();
    let lock = herd.state.path().join("held-by-something-outside-herd");
    std::fs::write(&lock, "a human is using this machine").expect("the lock file is written");
    let capabilities = capability_file(&herd, &lock);

    let _hub = herd.start_hub("first").await;
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
    // The runner is up once the hub has heard its offer: a submission before
    // that is refused for want of a runner, which is not what is under test.
    herd.wait_until("the runner has joined", || {
        herd.said("first")
            .contains("runner the-real-runner has joined")
    })
    .await;

    // Four jobs that cannot start, in this order. Each has to be a different
    // piece of work, or the second would *join* the first (E-05).
    let mut queued = Vec::new();
    for (priority, what) in [
        ("step", "first-step"),
        ("step", "second-step"),
        ("producer", "the-producer"),
        ("merger", "the-merger"),
    ] {
        queued.push((
            what,
            herd.submit_needing(HELD, priority, &format!("printf '{what} ran\\n'"))
                .await,
        ));
    }
    // Nothing has started: they are all waiting on something outside Herd.
    herd.wait_until("every job says why it is waiting", || {
        queued
            .iter()
            .all(|(_, id)| herd.record(id)["waiting_for"].is_string())
    })
    .await;
    for (what, id) in &queued {
        let record = herd.record(id);
        let said = record["waiting_for"].as_str().expect("a sentence");
        assert!(
            said.contains(HELD),
            "{what} ({id}) should say what it waits for: {said}"
        );
        assert_eq!(record["state"]["type"], "queued");
    }

    // The slot frees.
    std::fs::remove_file(&lock).expect("the lock file goes");

    // Every job runs, and the hub's own output says which one it handed out
    // first. The merger was submitted last and must be the first attempt.
    herd.wait_until("every job has ended", || {
        queued
            .iter()
            .all(|(_, id)| herd.record(id)["state"]["type"] == "finished")
    })
    .await;
    let said = herd.said("first");
    let order: Vec<&str> = said
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("herd-hub: ")?;
            let id = rest.split(' ').next()?;
            queued
                .iter()
                .find(|(_, queued_id)| queued_id == id)
                .filter(|_| rest.contains("is attempt 1 on runner"))
                .map(|(what, _)| *what)
        })
        .collect();
    assert_eq!(
        order.first(),
        Some(&"the-merger"),
        "the merger was submitted last and must have taken the free slot first; \
         the hub handed them out in the order {order:?}"
    );
    // And the rest went in priority order, FIFO inside the level: the two
    // step jobs in the order they were submitted, then the producer.
    assert_eq!(
        order,
        vec!["the-merger", "first-step", "second-step", "the-producer"],
        "the one queue must come out highest level first, FIFO within a level"
    );
}

/// The attack, and the by-hand check that goes with it: a hub killed with
/// `SIGKILL` while a low-priority job is aging behind higher work must come
/// back with the job's accumulated wait **intact** — neither reset, which
/// would starve it for ever, nor jumped to the top, which would make every
/// restart a promotion (D68).
#[tokio::test(flavor = "multi_thread")]
async fn a_hub_killed_while_a_job_ages_does_not_lose_its_wait() {
    let herd = Herd::new();
    let lock = herd.state.path().join("held-by-something-outside-herd");
    std::fs::write(&lock, "held").expect("the lock file is written");
    let capabilities = capability_file(&herd, &lock);

    let mut hub = herd
        .start_hub_with("first", &["--aging-step", AGING_STEP_IN_THESE_TESTS])
        .await;
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
        herd.said("first")
            .contains("runner the-real-runner has joined")
    })
    .await;

    // A producer job, which is three levels from the top: at a two-second
    // step it is weighed as `nightly` after two seconds, `step` after four
    // and `merger` after six.
    let producer = herd
        .submit_needing(HELD, "producer", "printf 'the producer ran\\n'")
        .await;
    herd.wait_until("the producer is waiting", || {
        herd.record(&producer)["waiting_for"].is_string()
    })
    .await;
    let submitted_at = herd.record(&producer)["submitted_at"]
        .as_str()
        .expect("a moment")
        .to_owned();

    // Let it age one step, and no more, so that "it came back maximally
    // aged" is a thing this test can see.
    herd.wait_until("the producer has aged one level", || {
        herd.record(&producer)["waiting_for"]
            .as_str()
            .is_some_and(|said| said.contains("weighed as nightly"))
    })
    .await;

    // Killed outright, with no chance to write anything down.
    hub.signal(rustix::process::Signal::KILL).await;
    let same_hub_again = herd
        .start_hub_with("second", &["--aging-step", AGING_STEP_IN_THESE_TESTS])
        .await;
    let _ = &same_hub_again;

    // The hub that came back says where the job stands, and the wait it
    // accumulated is still there: the moment it was submitted is the moment
    // on its own record, which the kill could not touch.
    herd.wait_until("the hub that came back says where the job stands", || {
        herd.record(&producer)["waiting_for"]
            .as_str()
            .is_some_and(|said| said.contains("weighed as"))
    })
    .await;
    assert_eq!(
        herd.record(&producer)["submitted_at"]
            .as_str()
            .expect("a moment"),
        submitted_at,
        "a restart must not move the moment a job was submitted"
    );
    let said = herd.record(&producer)["waiting_for"]
        .as_str()
        .expect("a sentence")
        .to_owned();
    assert!(
        said.contains("at priority producer"),
        "the record still says what the requester asked for: {said}"
    );
    assert!(
        said.contains("weighed as nightly") || said.contains("weighed as step"),
        "the job must come back with the wait it had, neither reset to \
         producer nor jumped to merger: {said}"
    );
    assert!(
        !said.contains("weighed as merger"),
        "a restart must not come back maximally aged: {said}"
    );
    assert_eq!(
        herd.record(&producer)["priority"],
        "producer",
        "aging is computed, never written: the record keeps the level the \
         requester asked for"
    );

    // And it does run, in the end: the lowest level never starves (E-18).
    std::fs::remove_file(&lock).expect("the lock file goes");
    herd.wait_until("the producer ran", || {
        herd.record(&producer)["state"]["type"] == "finished"
    })
    .await;
    assert!(herd.log(&producer).contains("the producer ran"));
}

/// The attack: the durable order. A hub killed mid-queue must come back with
/// the same queue, every priority as it was, and the deduplication links
/// intact — an index that lived only in memory would run one piece of work
/// twice after a restart (D69).
#[tokio::test(flavor = "multi_thread")]
async fn the_queue_its_priorities_and_its_joins_all_come_back() {
    let herd = Herd::new();
    let lock = herd.state.path().join("held-by-something-outside-herd");
    std::fs::write(&lock, "held").expect("the lock file is written");
    let capabilities = capability_file(&herd, &lock);
    let runner_options = [
        "--capabilities",
        capabilities.to_str().expect("a utf-8 path"),
        "--no-detect",
        "--probe-interval",
        "50",
    ];

    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner_with("the-real-runner", "runner", "runner", &runner_options);
    herd.wait_until("the runner has joined", || {
        herd.said("first")
            .contains("runner the-real-runner has joined")
    })
    .await;

    let levels = [
        ("producer", "work-a"),
        ("nightly", "work-b"),
        ("merger", "work-c"),
        ("step", "work-d"),
    ];
    let mut queued = Vec::new();
    for (priority, what) in levels {
        queued.push((
            priority,
            what,
            herd.submit_needing(HELD, priority, &format!("printf '{what} ran\\n'"))
                .await,
        ));
    }
    // And one join, before the kill: a second submission of `work-a`.
    let joined = herd
        .submit_needing(HELD, "merger", "printf 'work-a ran\\n'")
        .await;
    assert_eq!(
        joined, queued[0].2,
        "an identical submission must join the open job"
    );

    herd.wait_until("every job is waiting", || {
        queued
            .iter()
            .all(|(_, _, id)| herd.record(id)["waiting_for"].is_string())
    })
    .await;

    // Killed outright, runner and all.
    hub.signal(rustix::process::Signal::KILL).await;
    runner.signal(rustix::process::Signal::KILL).await;
    let _hub = herd.start_hub("second").await;
    let _runner = herd.start_runner_with("the-real-runner", "runner2", "runner", &runner_options);
    herd.wait_until("the runner has joined the hub that came back", || {
        herd.said("second")
            .contains("runner the-real-runner has joined")
    })
    .await;

    // Every record is as it was: its level and the submissions it answers.
    for (priority, what, id) in &queued {
        let record = herd.record(id);
        assert_eq!(
            record["state"]["type"], "queued",
            "{what} ({id}) should still be queued"
        );
        if *what == "work-a" {
            // It was joined by a merger-level submission, so the *record*
            // carries merger: a raise is written, unlike aging.
            assert_eq!(record["priority"], "merger");
            assert_eq!(record["submissions"], 2);
        } else {
            assert_eq!(record["priority"], *priority, "{what} ({id})");
            assert_eq!(record["submissions"], 1, "{what} ({id})");
        }
    }

    // The deduplication link came back with the queue: a third identical
    // submission still joins, rather than running the same work twice.
    let joined_again = herd
        .submit_needing(HELD, "step", "printf 'work-a ran\\n'")
        .await;
    assert_eq!(
        joined_again, queued[0].2,
        "a hub that came back must still know which open job is doing this work"
    );
    assert_eq!(herd.record(&queued[0].2)["submissions"], 3);

    // And when the slot frees, the queue comes out in order.
    std::fs::remove_file(&lock).expect("the lock file goes");
    herd.wait_until("every job has ended", || {
        queued
            .iter()
            .all(|(_, _, id)| herd.record(id)["state"]["type"] == "finished")
    })
    .await;
    let said = herd.said("second");
    let order: Vec<&str> = said
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("herd-hub: ")?;
            let id = rest.split(' ').next()?;
            queued
                .iter()
                .find(|(_, _, queued_id)| queued_id == id)
                .filter(|_| rest.contains("is attempt 1 on runner"))
                .map(|(_, what, _)| *what)
        })
        .collect();
    assert_eq!(
        order,
        vec!["work-a", "work-c", "work-d", "work-b"],
        "work-a was raised to merger by the submission that joined it and was \
         minted first, so it goes before work-c; then the step, then the nightly"
    );
    // One execution for the three submissions of `work-a`, and its log holds
    // its line once.
    let log = herd.log(&queued[0].2);
    assert_eq!(
        log.matches("work-a ran").count(),
        1,
        "three submissions of one piece of work ran it once: {log:?}"
    );
}

/// E-05 through the real binaries, which is where an exit code exists: two
/// submissions, two waits, one execution — the same code and the same log on
/// both.
#[tokio::test(flavor = "multi_thread")]
async fn two_submissions_and_two_waits_share_one_execution() {
    let herd = Herd::new();
    let _hub = herd.start_hub("first").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    let script = "for i in 1 2 3; do printf 'line %s\\n' \"$i\"; sleep 0.2; done; exit 3";
    let first = herd.submit(script).await;
    let second = herd.submit(script).await;
    assert_eq!(
        first, second,
        "two identical submissions must come back with one id"
    );

    // Two waits, started together, each its own process.
    let first_wait = ["wait", first.as_str()];
    let second_wait = ["wait", second.as_str()];
    let (one, two) = tokio::join!(herd.herdctl(&first_wait), herd.herdctl(&second_wait));

    for (which, output) in [("the first wait", &one), ("the second wait", &two)] {
        assert_eq!(
            output.status.code(),
            Some(3),
            "{which} must wear the job's own exit code; it said {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let said = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .replace("\r\n", "\n")
            .to_string()
    };
    assert_eq!(
        said(&one),
        "line 1\nline 2\nline 3\n",
        "the first wait printed the whole log"
    );
    assert_eq!(
        said(&two),
        said(&one),
        "both waits must print the same log, because there was one execution"
    );

    let record = herd.record(&first);
    assert_eq!(record["submissions"], 2);
    assert_eq!(
        record["attempts"]
            .as_array()
            .expect("a list of attempts")
            .len(),
        1,
        "one execution, not two"
    );
    assert_eq!(
        herd.log(&first).matches("line 1").count(),
        1,
        "the log holds the job's output once"
    );
}

/// A submission whose work is **over** is a new job, not a stale verdict
/// (D70). The nastiest shape of it: the first job was cancelled, so joining
/// it would hand the second caller a 125 for a cancellation it never asked
/// for (D40, D20).
#[tokio::test(flavor = "multi_thread")]
async fn a_submission_after_a_cancelled_job_runs_again_rather_than_wearing_its_125() {
    let herd = Herd::new();
    let _hub = herd.start_hub("first").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    let script = "printf 'started\\n'; sleep 30";
    let first = herd.submit(script).await;
    herd.wait_until("the job is running", || {
        herd.record(&first)["state"]["type"] == "running"
    })
    .await;
    let cancelled = herd.herdctl(&["cancel", &first]).await;
    assert!(
        cancelled.status.success(),
        "the cancel is answered: {}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
    herd.wait_until("the job is over", || {
        herd.record(&first)["state"]["type"] == "finished"
    })
    .await;
    // 125 for the caller that asked for the cancellation: no verdict (D40).
    let waited = herd.herdctl(&["wait", &first]).await;
    assert_eq!(waited.status.code(), Some(125));

    // The same work again: a job of its own, which runs and has a verdict.
    let again = herd.submit("printf 'started\\n'; exit 0").await;
    assert_ne!(again, first);
    let second_run = herd.herdctl(&["wait", &again]).await;
    assert_eq!(
        second_run.status.code(),
        Some(0),
        "a submission after a cancelled one must get its own verdict, never \
         the 125 of a cancellation it did not ask for: {}",
        String::from_utf8_lossy(&second_run.stderr)
    );
    assert_eq!(herd.record(&again)["submissions"], 1);
}

/// The aging bound, end to end, through the hub's real clock: a producer job
/// behind a stream of higher-priority submissions runs — and runs *before*
/// the stream — once it has aged to the top.
///
/// The same code the simulated-time tests drive, with `Timestamp::now()` in
/// front of it instead of a moment a test chose. Six seconds of real time
/// stand in for E-18's ninety minutes.
#[tokio::test(flavor = "multi_thread")]
async fn the_lowest_priority_job_runs_within_the_aging_bound_on_the_real_clock() {
    let herd = Herd::new();
    let lock = herd.state.path().join("held-by-something-outside-herd");
    std::fs::write(&lock, "held").expect("the lock file is written");
    let capabilities = capability_file(&herd, &lock);

    let _hub = herd
        .start_hub_with("first", &["--aging-step", AGING_STEP_IN_THESE_TESTS])
        .await;
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
        herd.said("first")
            .contains("runner the-real-runner has joined")
    })
    .await;

    let producer = herd
        .submit_needing(HELD, "producer", "printf 'the producer ran\\n'")
        .await;
    // A steady stream of step jobs, submitted after it, for as long as it
    // takes the producer to climb three levels — and then until the hub says
    // it has.
    //
    // Waited on as a condition rather than measured off the clock. The
    // sentence on a record is rewritten only when a dispatch pass finds
    // something new (M6), and a submission is what causes a pass, so the
    // stream is also what makes the hub restate the queue: reading the record
    // the moment the clock passes the bound reads whichever pass happened
    // last, which on a busy host is a pass from a second earlier. The
    // assertion below is unchanged — it is the loop's exit condition now, and
    // a run where it never becomes true fails naming the sentence it got.
    let bound = Duration::from_secs(2 * 3);
    let started = std::time::Instant::now();
    let deadline = started + real_herd::PATIENCE;
    let mut stream = Vec::new();
    let said = loop {
        stream.push(
            herd.submit_needing(
                HELD,
                "step",
                &format!("printf 'a step job ran\\n'; # {}", stream.len()),
            )
            .await,
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        let said = herd.record(&producer)["waiting_for"]
            .as_str()
            .expect("a sentence")
            .to_owned();
        // By now the producer is at the top and in front of every one of them.
        if started.elapsed() >= bound
            && said.contains("weighed as merger")
            && said.contains("queued 1st of")
        {
            break said;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a producer past the bound must be at the front of the queue: {said}"
        );
    };
    assert!(
        stream.len() > 5,
        "the stream should be a stream: {} jobs",
        stream.len()
    );
    assert!(
        said.contains("weighed as merger") && said.contains("queued 1st of"),
        "a producer past the bound must be at the front of the queue: {said}"
    );

    // The slot frees, and it is the producer that takes it.
    std::fs::remove_file(&lock).expect("the lock file goes");
    herd.wait_until("the producer ran", || {
        herd.record(&producer)["state"]["type"] == "finished"
    })
    .await;
    let order: Vec<String> = herd
        .said("first")
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("herd-hub: ")?;
            let id = rest.split(' ').next()?;
            rest.contains("is attempt 1 on runner")
                .then(|| id.to_owned())
        })
        .collect();
    assert_eq!(
        order.first().map(String::as_str),
        Some(producer.as_str()),
        "the aged producer must be handed out before the stream that was \
         submitted after it; the hub handed out {order:?}"
    );
}
