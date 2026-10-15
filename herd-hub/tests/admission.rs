//! Admission and explained waits against the real binaries (M8).
//!
//! The unit tests of the claim step prove the arithmetic (see
//! `herd_hub::matcher`); this proves the arithmetic is wired to a real runner
//! announcing real figures, to a real `herdctl` declaring a real need, and to
//! a hub that can be killed in the middle of it.
//!
//! Every tmpfs figure here is **declared** with `--host-tmpfs` rather than
//! measured off the host, for the reason M7's slot tests declare their
//! capabilities: a test whose expectations depend on how much room
//! `/dev/shm` happens to have on the machine running it is a test that passes
//! or fails for a reason that is nothing to do with Herd.

mod real_herd;

use std::time::{Duration, Instant};

use real_herd::Herd;

/// A runner that lends four gibibytes of tmpfs and eight of memory, keeping
/// one back as its floor. Small figures on purpose: the arithmetic is the
/// same at four gibibytes as at thirty-one, and a test can reason about it.
fn a_small_lender() -> Vec<&'static str> {
    vec![
        "--host-tmpfs",
        "4GiB",
        "--host-memory",
        "8GiB",
        "--host-memory-floor",
        "1GiB",
        "--host-threads",
        "4",
    ]
}

fn state_of(herd: &Herd, job: &str) -> String {
    herd.record(job)["state"]["type"]
        .as_str()
        .unwrap_or("unreadable")
        .to_owned()
}

/// The two moments a job printed, so that "they did not overlap" is an
/// assertion about intervals and not about the hub's opinion of itself.
fn interval(herd: &Herd, job: &str) -> (f64, f64) {
    let log = herd.log(job);
    let moments: Vec<f64> = log
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .collect();
    assert_eq!(
        moments.len(),
        2,
        "{job} should have printed two moments, its log is {log:?}"
    );
    (moments[0], moments[1])
}

fn marks_its_interval(which: &str, seconds: &str) -> String {
    format!("printf '{which}\\n' >/dev/null; date +%s.%N; sleep {seconds}; date +%s.%N")
}

/// A job that holds whatever it reserved until the test lets it go.
///
/// For the tests whose subject is a reservation that is **still held** while
/// something else is judged against it. A `sleep` cannot do that job: it makes
/// the window a duration, and every one of those tests then has a race between
/// how long the sleep is and how long its own steps take. `sleep 4` across a
/// `kill -9`, a hub restart, a rejoin and two submissions is the shape that
/// actually failed in the suite — and it failed in the way that is hardest to
/// read, because once the holder has ended there is room, the job under test
/// is claimed instead of refused, and the wait is left looking for a sentence
/// that will never be written.
///
/// Returns the file that releases it and the script to submit.
fn a_job_held_until_the_test_says_so(herd: &Herd, which: &str) -> (std::path::PathBuf, String) {
    let release = herd.logs.path().join(format!("release-{which}"));
    (
        release.clone(),
        format!(
            "printf '{which}\\n'; until [ -e {} ]; do sleep 0.05; done",
            release.display()
        ),
    )
}

/// Let one of those jobs finish.
fn let_it_finish(release: &std::path::Path) {
    std::fs::write(release, b"go").expect("the test releases the job it was holding");
}
/// The same, for a test that also measures *when* the job held its room.
///
/// It prints the two moments [`interval`] reads, around a wait on the release
/// file instead of around a `sleep`: the room is held until the test has
/// proved what it needed to prove, and the non-overlap assertion is then made
/// over a window the test controls rather than one it hoped would be long
/// enough.
fn a_job_that_marks_its_interval_until_the_test_says_so(
    herd: &Herd,
    which: &str,
) -> (std::path::PathBuf, String) {
    let release = herd.logs.path().join(format!("release-{which}"));
    (
        release.clone(),
        format!(
            "printf '{which}\\n' >/dev/null; date +%s.%N; until [ -e {} ]; do sleep 0.05; \
             done; date +%s.%N",
            release.display()
        ),
    )
}

/// E-04 and E-31: a job that cannot be given the room it asks for is told so
/// **with both figures**, and the two ways of not fitting say different
/// things.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_carries_the_free_figure_and_the_needed_figure() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner_with("lender", "runner", "runner", &a_small_lender());
    herd.wait_for_runner().await;

    // More tmpfs than that machine has in all: it says *ever*, not *not now*,
    // and names both figures. The job is kept alive all the same — E-31 says
    // short is a wait and never a failure, and a bigger machine may dial in.
    let impossible_here = herd
        .submit_reserving(None, Some(6 * 1024), "printf 'never runs here\\n'")
        .await;
    herd.wait_until("the too-big job has been judged", || {
        herd.waiting_for(&impossible_here).contains("tmpfs")
    })
    .await;
    let said = herd.waiting_for(&impossible_here);
    assert!(
        said.contains("lender has 4 GiB of tmpfs in all and this job needs 6 GiB"),
        "{said}"
    );
    assert!(said.contains("can ever cover it"), "{said}");
    assert_eq!(state_of(&herd, &impossible_here), "queued", "{said}");

    // And a job short of room *now*: 4 GiB there, 3 GiB held by a job that
    // is running, 2 GiB wanted. E-04's own sentence, with its own figures.
    // Held until this test is done with it, not for ten seconds: the sentence
    // below is about room this job is holding, so it has to still be holding
    // it when the sentence is read (the same shape as attack 10).
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "holder");
    let holding = herd.submit_reserving(None, Some(3 * 1024), &script).await;
    herd.wait_until("the holder is running", || {
        state_of(&herd, &holding) == "running"
    })
    .await;
    let short = herd
        .submit_reserving(None, Some(2 * 1024), "printf 'waits for room\\n'")
        .await;
    herd.wait_until("the short job has been judged", || {
        herd.waiting_for(&short).contains("free in tmpfs")
    })
    .await;
    let said = herd.waiting_for(&short);
    assert!(
        said.contains("1 GiB free in tmpfs, 2 GiB needed (4 GiB of tmpfs there in all)"),
        "E-04 asks for a sentence with the figures in it: {said}"
    );
    assert!(
        said.contains("1 job running there holds"),
        "and for what is in the way: {said}"
    );
    // Short *now* sticks to the machine that will free the room; too big for
    // ever does not. That difference is the whole of attack 4.
    assert!(said.contains("it is waiting for lender"), "{said}");
    assert!(
        !herd
            .waiting_for(&impossible_here)
            .contains("waiting for lender"),
        "{}",
        herd.waiting_for(&impossible_here)
    );

    // E-04's third element: what is ahead of it, by name.
    assert!(
        said.contains("ahead of it: ") || said.contains("nothing is ahead of it"),
        "{said}"
    );

    // The holder has done its work; it ends by being let go rather than by
    // running out of sleep, and the cancels below then have nothing to kill.
    let_it_finish(&release);
    let _ = herd.herdctl(&["cancel", &holding]).await;
    let _ = herd.herdctl(&["cancel", &short]).await;
    let _ = herd.herdctl(&["cancel", &impossible_here]).await;
}

/// E-31's own acceptance criterion: a job that cannot reserve **waits**, and
/// starts when a job ends. And then nothing is left behind — the proof being
/// that a job needing the whole of the machine's tmpfs runs straight away
/// afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn a_starved_reservation_waits_then_starts_and_leaks_nothing() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner_with("lender", "runner", "runner", &a_small_lender());
    herd.wait_for_runner().await;

    // Two 3 GiB reservations on a 4 GiB tmpfs. They cannot overlap.
    //
    // The first holds its room until this test has proved the second is
    // waiting for it, rather than for a second and a half: the submission and
    // the judgement below are a real process and a dispatch pass, which on a
    // loaded host take longer than that — and then the second job is claimed
    // because the room is free, which is correct of the hub and proves nothing
    // about E-31. It still prints the two moments [`interval`] reads.
    let (release, script) = a_job_that_marks_its_interval_until_the_test_says_so(&herd, "first");
    let first = herd.submit_reserving(None, Some(3 * 1024), &script).await;
    herd.wait_until("the first reservation is taken", || {
        state_of(&herd, &first) == "running"
    })
    .await;
    let second = herd
        .submit_reserving(None, Some(3 * 1024), &marks_its_interval("second", "0.2"))
        .await;
    herd.wait_until("the second job has been told why it waits", || {
        herd.waiting_for(&second).contains("free in tmpfs")
    })
    .await;
    assert_eq!(
        state_of(&herd, &second),
        "queued",
        "a reservation that cannot be taken waits: {}",
        herd.waiting_for(&second)
    );

    // Proved. The first may have its room back now, and the second starts
    // only after it does — which is what the intervals below check.
    let_it_finish(&release);
    for job in [&first, &second] {
        let job = job.clone();
        herd.wait_until(&format!("{job} is over"), || {
            state_of(&herd, &job) == "finished"
        })
        .await;
    }
    let (first_from, first_to) = interval(&herd, &first);
    let (second_from, second_to) = interval(&herd, &second);
    assert!(
        first_to <= second_from || second_to <= first_from,
        "the two reservations overlapped: {first_from}..{first_to} and \
         {second_from}..{second_to}. 3 GiB twice does not fit in 4 GiB, and the \
         host that swaps is invariant 2 broken"
    );
    assert!(
        second_from >= first_to,
        "the second job started before the first released its room"
    );

    // Nothing leaked: a job wanting every gibibyte of that tmpfs runs at
    // once. Under a leak it would wait for ever, which is exactly the shape
    // of harm a leaked reservation has — a machine quietly smaller than it
    // is, for the rest of the hub's life.
    let whole_lot = herd
        .submit_reserving(None, Some(4 * 1024), "printf 'the whole tmpfs\\n'")
        .await;
    herd.wait_until("the job that needs all of it is over", || {
        state_of(&herd, &whole_lot) == "finished"
    })
    .await;
    assert!(
        herd.log(&whole_lot).contains("the whole tmpfs"),
        "a job needing the whole tmpfs did not run, so something was never released: {}",
        herd.waiting_for(&whole_lot)
    );
}

/// E-04's cadence, through the real binaries: a job that waits says so again
/// on a clock, and does not say it once per dispatch pass.
///
/// Ten minutes of waiting are simulated by shortening the cadence rather than
/// by waiting ten minutes — the hub's own setting, the hub's own loop. Twenty
/// cadences pass here, so the bound this asserts is the same arithmetic a
/// ten-minute wait at sixty seconds gets.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_job_says_why_again_on_a_clock_and_does_not_flood() {
    let herd = Herd::new();
    // A cadence of 200 ms, and a wait of four seconds: twenty cadences, which
    // is twice the ten minutes E-04's acceptance criterion names.
    let _hub = herd
        .start_hub_with("hub", &["--wait-restatement", "0.2"])
        .await;
    let _runner = herd.start_runner_with("lender", "runner", "runner", &a_small_lender());
    herd.wait_for_runner().await;

    // Held until this test is done with it. Thirty seconds stood here, which
    // was five times the work below — a submission, a judgement, a `herdctl
    // wait` and four seconds of watching — and five times is a margin, not a
    // condition.
    let (_release, script) = a_job_held_until_the_test_says_so(&herd, "holder");
    let holding = herd.submit_reserving(None, Some(3 * 1024), &script).await;
    herd.wait_until("the holder is running", || {
        state_of(&herd, &holding) == "running"
    })
    .await;
    let waiting = herd
        .submit_reserving(None, Some(3 * 1024), "printf 'never gets room here\\n'")
        .await;
    herd.wait_until("the second job has been told why it waits", || {
        herd.waiting_for(&waiting).contains("free in tmpfs")
    })
    .await;

    // A `herdctl wait` attached to it, so the restatements land in a
    // transcript a person would actually read.
    let mut following = herd.start_wait(&waiting);
    let watching_from = Instant::now();
    let said = |herd: &Herd| {
        std::fs::read_to_string(herd.logs.path().join("wait-err"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("is queued and has not started"))
            .count()
    };
    herd.wait_until("the wait has heard the reason at least once", || {
        said(&herd) >= 1
    })
    .await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let watched = watching_from.elapsed();
    let lines = said(&herd);
    following.ask_it_to_stop();

    // It does not stop. Twenty cadences, so at least a few lines — and the
    // cheap way to fail this is to say the reason only when it *changes*,
    // which gives exactly one.
    assert!(
        lines >= 5,
        "{lines} lines over {watched:?} at a 200 ms cadence: a ten-minute wait would be \
         silence, and E-04 asks for at least every 60 s"
    );
    // And it does not flood. The bound is one line per cadence plus one, and
    // the dispatch pass ran far oftener than that — a line per pass would be
    // hundreds.
    // The clock's shortest interval is `cadence - tick`, three quarters of
    // the cadence, because the cadence is a deadline rather than an interval
    // (D91). Plus a couple for M6's news path, which adds a line whenever
    // the sentence genuinely changes.
    let bound = (watched.as_millis() / 150) as usize + 6;
    assert!(
        lines <= bound,
        "{lines} lines over {watched:?} is past the bound of {bound}: the reason is being \
         said once per dispatch pass rather than once per cadence"
    );
    println!("herd-hub: {lines} restatements over {watched:?}, bound {bound}");

    let _ = herd.herdctl(&["cancel", &holding]).await;
    let _ = herd.herdctl(&["cancel", &waiting]).await;
}

/// **Attack 10.** A hub killed with `kill -9` while a job holds memory and
/// tmpfs comes back with the figures right, because there was never anything
/// to rebuild: a reservation is a field on a record whose attempt is open
/// (D79).
#[tokio::test(flavor = "multi_thread")]
async fn a_killed_hub_reads_the_reservations_back_off_the_records() {
    let herd = Herd::new();
    let hub = herd.start_hub("first").await;
    let _runner = herd.start_runner_with("lender", "runner", "runner", &a_small_lender());
    herd.wait_for_runner().await;

    // A job holding 3 of the 4 gibibytes of tmpfs and 4 of the 7 admissible
    // gibibytes of memory, and **still running** when the hub dies and for as
    // long as the assertions below need it: held on a file rather than on a
    // `sleep`, because everything from here to the last judgement has to
    // happen while it is in flight, and a `kill -9`, a hub restart, a rejoin
    // and two submissions do not fit inside a figure chosen in advance. With
    // `sleep 4` here the suite timed out at load 19, looking for a sentence
    // the hub had no reason to write any more (D100's lesson, in M8's test).
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "inherited");
    let holding = herd
        .submit_reserving(Some(4 * 1024), Some(3 * 1024), &script)
        .await;

    herd.wait_until("the job is running", || {
        state_of(&herd, &holding) == "running"
    })
    .await;

    let mut hub = hub;
    hub.signal(rustix::process::Signal::KILL).await;
    let _second_hub = herd.start_hub("second").await;
    herd.wait_until("the runner has found the hub that came back", || {
        herd.said("second").contains("has joined")
    })
    .await;

    // The new hub knows that job only from its records — and the figures are
    // right on the other side: a second job that would not fit beside it is
    // told so, with the arithmetic.
    let would_not_fit = herd
        .submit_reserving(None, Some(2 * 1024), "printf 'should wait\\n'")
        .await;
    herd.wait_until("the new hub has judged it", || {
        herd.waiting_for(&would_not_fit).contains("tmpfs")
    })
    .await;
    let said = herd.waiting_for(&would_not_fit);
    assert!(
        said.contains("1 GiB free in tmpfs, 2 GiB needed"),
        "the hub that came back must count the inherited job's reservation: {said}"
    );
    assert!(said.contains("1 job running there holds"), "{said}");

    // And the memory half of the same fact.
    let too_much_memory = herd
        .submit_reserving(Some(5 * 1024), None, "printf 'should wait too\\n'")
        .await;
    herd.wait_until("the new hub has judged the memory one", || {
        herd.waiting_for(&too_much_memory).contains("memory")
    })
    .await;
    let said = herd.waiting_for(&too_much_memory);
    assert!(
        said.contains("3 GiB free of its 7 GiB memory admission, 5 GiB needed"),
        "the inherited job's 4 GiB of declared memory must still be held: {said}"
    );
    // Both sentences above are only worth anything while the attempt whose
    // reservation they count is still out there, so that is asserted rather
    // than assumed: if it had ended there would be room, and the hub would
    // have been right to claim these two instead of refusing them.
    //
    // Either state proves it. `running` is a hub that has been told so on a
    // live link; `interrupted` is this hub's own reading of an attempt it
    // inherited from a record and has not heard about yet — which is the very
    // thing this test is about, and it holds the reservation exactly as
    // `running` does (D79).
    let holder = state_of(&herd, &holding);
    assert!(
        matches!(holder.as_str(), "running" | "interrupted"),
        "the inherited attempt has to still be out there for the two sentences above to \
         prove anything about a reservation read back off its record; it is {holder}"
    );

    // When the inherited job ends, both get their room back — nothing was
    // pinned by the restart.
    let_it_finish(&release);
    for job in [&holding, &would_not_fit, &too_much_memory] {
        let job = job.clone();
        herd.wait_until(&format!("{job} is over"), || {
            state_of(&herd, &job) == "finished"
        })
        .await;
    }
    assert!(herd.log(&would_not_fit).contains("should wait"));
    assert!(herd.log(&too_much_memory).contains("should wait too"));
}

/// E-32: the load figure reaches the hub, lands on the line explaining every
/// placement, and is a gate on nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_load_is_on_the_placement_line_and_stops_nothing() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner_with("lender", "runner", "runner", &a_small_lender());
    herd.wait_for_runner().await;

    let job = herd
        .submit_reserving(Some(1024), Some(512), "printf 'ran\\n'")
        .await;
    herd.wait_until("the job is over", || state_of(&herd, &job) == "finished")
        .await;
    let why = herd.record(&job)["attempts"][0]["why"]
        .as_str()
        .expect("an attempt carries the line explaining it (E-36)")
        .to_owned();
    assert!(
        why.contains("recorded and not a gate (E-32)"),
        "E-32 asks for the figure to be recorded: {why}"
    );
    assert!(why.contains("admitted 1 GiB of memory"), "{why}");
    assert!(why.contains("reserved 512 MiB in tmpfs"), "{why}");
}
