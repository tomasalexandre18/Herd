//! E-37 against the real binaries: what this hub says out loud, and what it
//! never says.
//!
//! Four things are proved here, and each of them is a way E-37 can be
//! shipped and still be wrong:
//!
//! 1. **Zero events per step** — the acceptance criterion in the consumer's
//!    own words, *"a day's ntfy count does not grow with the number of step
//!    jobs"*. A fleet that runs jobs all day and loses nothing produces no
//!    event at all, and the one runner joining and leaving produces exactly
//!    two. A notification system nobody mutes is one that only speaks when
//!    something happened.
//! 2. **A sink that hangs delays no verdict and costs the hub none of its
//!    stop budget** — with a sink that really hangs, not a mock: a command
//!    that does not return. A job submitted while the sink is stuck gets its
//!    verdict, and the hub asked to stop in that state stops inside its own
//!    bound and says so by leaving with 0.
//! 3. **The owner's own switch is the owner's, and the orchestrator's is
//!    not** — E-37 asks for *"an owner switch changed from the owner's
//!    side"*, which is a narrower thing than "the switch moved". A position
//!    this hub asked for is not news to whoever asked for it.
//! 4. **A detached job's end is said when its submitter asked, and never
//!    otherwise** — E-37's fifth thing, and the one way this list could grow
//!    without anybody deciding to grow it. The gate is the asking written on
//!    the record, so a fleet of step jobs nobody asked about stays silent
//!    however many of them there are, and what the topic's count follows is
//!    the number of askings a person typed.
//!
//! The sink in every test is a command writing to a file under this test's
//! own temporary directory, or a `sleep`. **Nothing here touches anything
//! outside this machine**: no webhook, no public URL, no loopback port but
//! the hub's own.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with the other files that drive the real binaries.

mod real_herd;

use std::path::Path;
use std::time::Instant;

use real_herd::{Herd, PATIENCE};

/// What every runner in this file is told about its host's floor, and why.
///
/// **It is not about memory**, and no assertion here reads a memory figure:
/// it is about the runner *starting at all* on a shared machine. A runner
/// lowers what it lends to the room under the tightest cgroup ceiling
/// enclosing it (D119), and it refuses to start when its own floor is above
/// that room — rightly, because nothing could ever be admitted there. On this
/// workstation the enclosing slice has been seen to leave 1 003 MiB against
/// the default floor of 1 024 MiB, which turns every test in this file into a
/// report about what else is running on the host (ORCH-069, ORCH-107). The
/// floor is the machine operator's own setting; these tests set it low and
/// assert nothing that depends on it.
const A_FLOOR_A_BUSY_HOST_CAN_KEEP: [&str; 2] = ["--host-memory-floor", "64MiB"];

/// Every event the sink has been given, in order, as JSON.
fn events(file: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("the sink was given something that is not one JSON object: {line} ({error})")
            })
        })
        .collect()
}

/// What happened, in order, as a reader of the topic would see it.
fn kinds(file: &Path) -> Vec<String> {
    events(file)
        .into_iter()
        .map(|event| {
            event["kind"]
                .as_str()
                .unwrap_or("an event with no kind")
                .to_owned()
        })
        .collect()
}

/// The acceptance criterion of E-37, measured: the count does not grow with
/// the step jobs.
#[tokio::test(flavor = "multi_thread")]
async fn a_day_of_step_jobs_is_no_events_and_a_runner_coming_and_going_is_two() {
    let herd = Herd::new();
    let sink_file = herd.logs.path().join("events.jsonl");
    // A real sink, and the plainest one there is: `cat` appending to a file.
    // One line per event is the hub's doing, not the shell's.
    let spec = format!("cat >> {}", sink_file.display());
    let _hub = herd.start_hub_with("hub", &["--notify-sink", &spec]).await;
    let said = herd.said("hub");
    assert!(
        said.contains("events go to") && said.contains("Never one per step job"),
        "the hub says at startup where its events go and what it will not send: {said}"
    );

    let mut runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &A_FLOOR_A_BUSY_HOST_CAN_KEEP,
    );
    herd.wait_for_runner().await;
    herd.wait_until("the runner's joining has reached the sink", || {
        kinds(&sink_file) == ["runner_joined"]
    })
    .await;
    let joined = events(&sink_file);
    assert_eq!(
        joined[0]["herd_event_version"], 1,
        "every payload says which format it is: {joined:?}"
    );
    assert_eq!(joined[0]["runner"], "the-real-runner");
    assert!(
        joined[0]["version"].is_string() && joined[0]["at"].is_string(),
        "a join says which build joined and when: {joined:?}"
    );

    // **The day's work.** Jobs that run, jobs that fail, jobs that print:
    // every one of them a step job, which is what a day at the first
    // consumer's site is made of. `wait_for_runner` has already run one more.
    for which in 0..4 {
        let output = herd
            .herdctl(&[
                "run",
                "--",
                "/bin/sh",
                "-c",
                &format!("printf 'step {which}\\n'"),
            ])
            .await;
        assert!(
            output.status.success(),
            "the step job ran: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let failed = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'red\\n'; exit 3"])
        .await;
    assert_eq!(
        failed.status.code(),
        Some(3),
        "a step job's own exit code reaches its requester, and is not an event"
    );
    // And one of the day's jobs **detached**, since an end can now be an
    // event (E-37): a `submit` that asked for nothing is as silent as a
    // `run`, which is what keeps this test's claim true now that there is a
    // way to ask. The asking case is
    // `a_detached_jobs_end_is_said_when_its_submitter_asked_and_never_otherwise`.
    let detached = herd.submit_ok(&[], "printf 'detached\\n'").await;
    herd.wait_until("the detached step job ends", || {
        herd.states_of(&detached).last().map(String::as_str) == Some("finished")
    })
    .await;
    assert_eq!(
        kinds(&sink_file),
        ["runner_joined"],
        "six step jobs, one of them red and one of them detached, and not one event: \
         E-37's count must not grow with the number of step jobs. The sink holds: {:?}",
        events(&sink_file)
    );

    // And the other half of "a client lost or joined".
    runner.ask_it_to_stop();
    let _ = runner.gone().await;
    herd.wait_until("the runner's going has reached the sink", || {
        kinds(&sink_file) == ["runner_joined", "runner_lost"]
    })
    .await;
    let all = events(&sink_file);
    assert_eq!(all.len(), 2, "two events for a machine that came and went");
    assert_eq!(all[1]["runner"], "the-real-runner");
    assert!(
        all[1]["why"]
            .as_str()
            .expect("a reason")
            .contains("link to this hub closed"),
        "the event says why the machine is gone: {all:?}"
    );
    assert!(
        all[1]["body"].as_str().expect("a body").is_ascii(),
        "the consumer's topic takes ASCII bodies: {all:?}"
    );
}

/// Property 1 and property 2, against a sink that really does not answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_sink_that_hangs_delays_no_verdict_and_costs_the_hub_no_stop_budget() {
    let herd = Herd::new();
    // A sink that hangs, the way a broken webhook hangs: it accepts the work
    // and never comes back. Forty-five seconds is far longer than everything
    // measured below, and the hub's own `DELIVER_WITHIN` gives up on it well
    // before that, so nothing of this test outlives it.
    let mut hub = herd
        .start_hub_with("hub", &["--notify-sink", "sleep 45"])
        .await;
    let mut runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &A_FLOOR_A_BUSY_HOST_CAN_KEEP,
    );
    // The runner joining is the event that is now stuck in the sink: from
    // here on, everything this hub does, it does with a notification in
    // flight that will never complete.
    herd.wait_for_runner().await;

    // ---- Property 1: the verdict.
    let began = Instant::now();
    let output = herd
        .herdctl(&[
            "run",
            "--",
            "/bin/sh",
            "-c",
            "printf 'the verdict\\n'; exit 0",
        ])
        .await;
    let took = began.elapsed();
    assert!(
        output.status.success(),
        "a job's verdict must not wait on a notification: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        real_herd::job_output(&output.stdout).contains("the verdict"),
        "the job's own output reaches its requester: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    // Loose on purpose, and it does not have to be tight: the sink holds for
    // 45 s and the harness gives up on a `herdctl` that has not come back in
    // `PATIENCE` (30 s), so a verdict that waited on the sink could not reach
    // this line at all — it would have panicked above, naming the wait. A
    // tighter figure here would be a measurement of this shared host's load
    // and not of the product.
    assert!(
        took < PATIENCE,
        "the job took {took:?} with a sink that hangs for 45 s; a verdict must not wait on \
         a notification at all"
    );

    // ---- Property 2: the stop budget.
    let began = Instant::now();
    hub.ask_it_to_stop();
    let how_it_went = tokio::time::timeout(PATIENCE, hub.gone())
        .await
        .expect("a hub asked to stop comes back, whatever its sink is doing");
    let stopping_took = began.elapsed();
    // **The hub's own statement that it stopped inside its bound.** It exits
    // non-zero when the wait ran past `STOP_WITHIN`, so a zero here is the
    // product saying the blocked sink cost it nothing — a stronger assertion
    // than re-timing the stop from out here, where this host's load is in
    // the figure.
    assert_eq!(
        how_it_went.code(),
        Some(0),
        "a hub with a notification stuck in a sink that will not answer for 45 s must still \
         stop cleanly inside {:?} (it took {stopping_took:?}): {}",
        herd_hub::STOP_WITHIN,
        herd.said("hub")
    );
    assert!(
        stopping_took < PATIENCE,
        "the hub took {stopping_took:?} to stop with a hanging sink; the sink is not in the \
         stop path and must not be able to extend it"
    );
    runner.ask_it_to_stop();
    let _ = runner.gone().await;
}

/// E-37 asks for the switch the **owner** moved, which is not every switch.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_own_switch_is_an_event_and_the_hubs_own_asking_is_not() {
    let herd = Herd::new();
    let sink_file = herd.logs.path().join("events.jsonl");
    let spec = format!("cat >> {}", sink_file.display());
    let _hub = herd.start_hub_with("hub", &["--notify-sink", &spec]).await;
    let mut runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        // Her own switch file read on a short clock, so that E-33's ten
        // seconds is not what this test waits out.
        &[
            "--switch-poll",
            "100",
            A_FLOOR_A_BUSY_HOST_CAN_KEEP[0],
            A_FLOOR_A_BUSY_HOST_CAN_KEEP[1],
        ],
    );
    herd.wait_for_runner().await;
    herd.wait_until("the runner has joined", || {
        kinds(&sink_file) == ["runner_joined"]
    })
    .await;

    // ---- The orchestrator's side first: `herdctl switch` is somebody
    // asking, and the asker already knows the answer. It must not be an
    // event.
    // `off` and not `drain`: a drained runner leaves once it holds nothing,
    // which is a machine *lost* and an event of its own. `off` keeps the link
    // up, which is the case this assertion is about.
    let asked = herd
        .herdctl(&[
            "switch",
            "wsl-pc",
            "off",
            "--reason",
            "the orchestrator wants it quiet",
        ])
        .await;
    assert!(
        asked.status.success(),
        "herdctl switch works: {}",
        String::from_utf8_lossy(&asked.stderr)
    );
    // Back to `on`, through the hub again, so that the owner's own write
    // below is the thing that moves the position.
    let back = herd
        .herdctl(&["switch", "wsl-pc", "on", "--reason", "it may claim again"])
        .await;
    assert!(
        back.status.success(),
        "herdctl switch works: {}",
        String::from_utf8_lossy(&back.stderr)
    );
    assert_eq!(
        kinds(&sink_file),
        ["runner_joined"],
        "a position this hub asked for is not news to whoever asked for it: {:?}",
        events(&sink_file)
    );

    // ---- Her side: the file on her own machine, which is the whole of
    // E-33's owner half (invariant 3 — nothing listens on a runner).
    herd.switch_on_the_machine("runner", &["off", "--reason", "she is gaming"])
        .await;
    herd.wait_until("her switch has reached the sink", || {
        kinds(&sink_file) == ["runner_joined", "owner_switch"]
    })
    .await;
    let all = events(&sink_file);
    assert_eq!(all[1]["runner"], "wsl-pc");
    assert_eq!(
        all[1]["switch"], "off",
        "the position is a field of its own, not a sentence to parse: {all:?}"
    );
    assert_eq!(
        all[1]["why"], "she is gaming",
        "her own reason is what makes the event worth reading: {all:?}"
    );

    runner.ask_it_to_stop();
    let _ = runner.gone().await;
}

/// E-37's fifth thing: **a detached job's end, when the submitter asked**.
///
/// Three claims, and the third is the one the acceptance criterion rests on:
///
/// 1. A detached submission that asked gets exactly one event when it ends,
///    carrying the job, the cause in one machine-readable word and the
///    ending in a sentence.
/// 2. **Every ending, not only a green one.** A job that exited 3 is as much
///    an end as one that exited 0, and a submitter who is not there to read
///    the verdict is exactly the person who needs to hear about the red one.
/// 3. **A submission that did not ask emits nothing**, and that is asserted
///    against a run of jobs on either side of the ones that did: the count
///    the topic sees grows with the number of *askings*, which is a number a
///    person typed, and not with the number of step jobs, which is a number
///    the fleet produces. That is E-37's *"a day's ntfy count does not grow
///    with the number of step jobs"*, measured.
///
/// The asking is read off `job.json` as well as off the topic, because the
/// record is where it lives between the submission and the end, and a hub
/// that announced the right thing while writing the wrong record would be
/// passing this test by accident.
#[tokio::test(flavor = "multi_thread")]
async fn a_detached_jobs_end_is_said_when_its_submitter_asked_and_never_otherwise() {
    let herd = Herd::new();
    let sink_file = herd.logs.path().join("events.jsonl");
    let spec = format!("cat >> {}", sink_file.display());
    let _hub = herd.start_hub_with("hub", &["--notify-sink", &spec]).await;
    let said = herd.said("hub");
    assert!(
        said.contains("Five things are said") && said.contains("--announce-end"),
        "the hub says at startup that an end can be one of its events, and how to ask for \
         one: {said}"
    );
    let mut runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &A_FLOOR_A_BUSY_HOST_CAN_KEEP,
    );
    herd.wait_for_runner().await;
    herd.wait_until("the runner's joining has reached the sink", || {
        kinds(&sink_file) == ["runner_joined"]
    })
    .await;

    // ---- Detached, and asking for nothing: the ordinary step job, submitted
    // the ordinary way. `submit` is the detached door — it leaves with the id
    // and closes — so these are jobs with nobody following them at all, which
    // is precisely the case an over-eager implementation would announce.
    let mut quiet = Vec::new();
    for which in 0..3 {
        quiet.push(
            herd.submit_ok(&[], &format!("printf 'quiet {which}\\n'"))
                .await,
        );
    }
    for job in &quiet {
        herd.wait_until("a detached step job nobody asked about ends", || {
            herd.states_of(job).last().map(String::as_str) == Some("finished")
        })
        .await;
        assert_eq!(
            herd.record(job)["announce_end"],
            serde_json::Value::Null,
            "nobody asked, so no key is written: {:?}",
            herd.record(job)
        );
    }
    assert_eq!(
        kinds(&sink_file),
        ["runner_joined"],
        "three detached step jobs, every one of them over, and not one event: {:?}",
        events(&sink_file)
    );

    // ---- And one that asked.
    let asked = herd
        .submit_ok(
            &["--announce-end"],
            "printf 'the one that asked\\n'; exit 0",
        )
        .await;
    herd.wait_until("the end of the job that asked reaches the sink", || {
        kinds(&sink_file) == ["runner_joined", "job_ended"]
    })
    .await;
    assert_eq!(
        herd.record(&asked)["announce_end"],
        serde_json::Value::Bool(true),
        "the asking is on the record, which is where it lives between the submission and \
         the end: {:?}",
        herd.record(&asked)
    );
    let all = events(&sink_file);
    assert_eq!(all[1]["herd_event_version"], 1);
    assert_eq!(all[1]["job"], asked.as_str());
    assert_eq!(
        all[1]["cause"], "exited",
        "the cause is one machine-readable word, which a consumer's router switches on: \
         {all:?}"
    );
    assert_eq!(
        all[1]["how"], "exited 0",
        "and the sentence beside it is the ending a person reads, with no second lookup: \
         {all:?}"
    );
    assert!(
        all[1]["body"]
            .as_str()
            .expect("a body")
            .contains(asked.as_str()),
        "the body names the job: {all:?}"
    );
    assert!(
        all[1]["body"].as_str().expect("a body").is_ascii()
            && all[1]["title"].as_str().expect("a title").is_ascii(),
        "the consumer's topic takes ASCII: {all:?}"
    );

    // ---- A red one that asked. **The end is the end**: a detached submitter
    // who is not there to read exit 3 is exactly who needs telling.
    let red = herd
        .submit_ok(&["--announce-end"], "printf 'red\\n'; exit 3")
        .await;
    herd.wait_until("the red end reaches the sink too", || {
        kinds(&sink_file) == ["runner_joined", "job_ended", "job_ended"]
    })
    .await;
    let all = events(&sink_file);
    assert_eq!(all[2]["job"], red.as_str());
    assert_eq!(all[2]["cause"], "exited");
    assert_eq!(
        all[2]["how"], "exited 3",
        "a job that failed says which code, and says nothing about a 0 it was never \
         compared against: {all:?}"
    );

    // ---- And the criterion, from the other side: more step jobs after the
    // askings, and the count stands where the askings left it.
    let mut after = Vec::new();
    for which in 0..3 {
        after.push(
            herd.submit_ok(&[], &format!("printf 'after {which}\\n'"))
                .await,
        );
    }
    for job in &after {
        herd.wait_until("a later detached step job ends", || {
            herd.states_of(job).last().map(String::as_str) == Some("finished")
        })
        .await;
    }
    assert_eq!(
        kinds(&sink_file),
        ["runner_joined", "job_ended", "job_ended"],
        "eight detached submissions ran and two of them asked, so the topic holds two \
         ends: a day's count grows with the askings and not with the step jobs (E-37). \
         The sink holds: {:?}",
        events(&sink_file)
    );

    runner.ask_it_to_stop();
    let _ = runner.gone().await;
}
