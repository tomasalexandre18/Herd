//! E-35 and E-40 against the real binaries: the status document a serving
//! hub publishes, read off the state root while the hub is serving.
//!
//! **Why this cannot be an in-process test.** What E-35 promises is that
//! somebody who is not us can learn what the fleet is doing without reading
//! a log — and the reader is a program on the operator's host, looking at a
//! file on a volume while a hub holds the `flock` on `hub.lock` beside it.
//! An in-process hub with a stub store proves none of that: it cannot show
//! that the document appears on a real disk, that `herd-hub status` reads it
//! through the lock, that a real runner's own figures reach it, or that a
//! real job in flight is named in it. So the hub here is a process, the
//! runner is a process, and the jobs are real jobs.
//!
//! Three things are proved, and they are three different claims.
//!
//! 1. **It says what the fleet is really doing** — a real runner's build,
//!    what it lends, its capability and its health age; a real job running
//!    on it with the line that explains the placement; a real queued job
//!    with what it is waiting for.
//! 2. **It is published on change and on nothing else.** A hub where
//!    nothing happens does not rewrite the document *at all* — not its
//!    bytes, not its timestamp, not the `as_of` inside it — over a span
//!    many times the publisher's own floor; and a change made after that
//!    span lands well inside it.
//! 3. **Reading it disturbs nothing.** Hundreds of reads leave the file
//!    untouched, and `herd-hub status` against the live root answers while
//!    a job is running — which it could not do if it took the hub's lock.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with the other files that drive the real binaries.
//! Every wait here is on a condition, never on a sleep, with the one
//! deliberate exception of the quiet window in claim 2, where the *absence*
//! of an event is the thing being measured and there is nothing to wait on.

mod real_herd;

use std::time::{Duration, Instant};

use herd_hub::{Status, STATUS_FILE, STATUS_VERSION};
use real_herd::Herd;

/// How long any wait here may take before it is a failure. The harness's own
/// figure, repeated rather than exported: CI is slow and this is never a
/// substitute for waiting on a real condition.
const PATIENCE: Duration = Duration::from_secs(30);

/// How long nothing is allowed to happen for, in the claim that nothing
/// happening produces no writes.
///
/// Four seconds, which is eighty times the publisher's floor
/// (`status::AT_MOST_EVERY`, 50 ms) and twenty times the health cadence
/// these tests run their runners at. Any clock-driven writer with a period
/// at or under this would have rewritten the file inside it.
const QUIET: Duration = Duration::from_secs(4);

/// How long a change may take to reach the document, in the other half of
/// that claim.
///
/// Deliberately **shorter than [`QUIET`]**, and that is what makes the pair
/// decisive rather than suggestive: a writer on a clock of period P would
/// have written during the quiet window if P ≤ QUIET, and could not have
/// answered inside this bound if P > QUIET. Two seconds rather than two
/// hundred milliseconds because a two-core CI runner spawning a process is
/// the slow part, not the publisher.
const SOON: Duration = Duration::from_secs(2);

/// The capability the runner in these tests declares.
///
/// Declared rather than probed, and with `--no-detect` beside it, so that
/// the document's capability list is exactly one entry whatever host this
/// runs on: a `shared` capability with no probe, which cannot be suspended
/// and has no slot count. A probed fleet would make this test's assertions
/// depend on whether the host has a `/dev/kvm`.
const WHAT_IT_LENDS: &str = "build";

/// What the runner in these tests is told to keep for its host, and the
/// ceiling it puts on one job.
///
/// **Fixtures, not weakened assertions**, for the same reason
/// `real_herd::ROOM_FOR_A_SECOND_JOB` is one. What a runner announces is the
/// room in the cgroup it stands in, read once at startup (D119) — so on a
/// shared host that figure moves between two runs of this file. With the
/// defaults, a runner started while the enclosing slice held 17 of its
/// 18 GiB refused to start at all: *"--host-memory-floor is 1024 MiB and
/// this host lends 1014 MiB, so nothing could ever be admitted here"*.
/// Measured here on 2026-10-29 with three other tasks building on the same
/// workstation; the same file had passed minutes earlier with 3 177 MiB of
/// room.
///
/// These two figures leave a runner room to start and to hold a job on any
/// host that lends a few hundred megabytes. Nothing this file asserts is
/// about either of them: what is asserted is that the document carries the
/// machine's own figures, whatever they are.
const A_LEND_ANY_HOST_CAN_MAKE: [&str; 2] =
    ["--host-memory-floor=64MiB", "--job-memory-max=256MiB"];

/// The options every runner in this file is started with.
fn runner_options() -> Vec<String> {
    let mut options = vec![
        // Nothing probed, so the capability list in the document is exactly
        // what this test declared whatever the host has.
        "--no-detect".to_owned(),
        "--declare-capability".to_owned(),
        WHAT_IT_LENDS.to_owned(),
    ];
    options.extend(A_LEND_ANY_HOST_CAN_MAKE.iter().map(|o| (*o).to_owned()));
    options
}
/// The document, or a failure saying there is none.
fn document(herd: &Herd) -> Status {
    Status::read(herd.state.path())
        .expect("the status document reads")
        .unwrap_or_else(|| {
            panic!(
                "no {STATUS_FILE} in {}: the hub published no status document",
                herd.state.path().display()
            )
        })
}

/// Wait until the document says something, and answer with it.
async fn until(
    herd: &Herd,
    what: &str,
    inside: Duration,
    mut ready: impl FnMut(&Status) -> bool,
) -> Status {
    let waiting_since = Instant::now();
    let deadline = waiting_since + inside;
    loop {
        if let Ok(Some(status)) = Status::read(herd.state.path()) {
            if ready(&status) {
                return status;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the status document never showed {what} inside {inside:?}. It last \
             said:\n{}\n{}",
            Status::read(herd.state.path())
                .ok()
                .flatten()
                .map(|status| status.to_string())
                .unwrap_or_else(|| "(no document at all)".to_owned()),
            herd.everything_said()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// E-35 and E-40 on a real fleet: a runner, what it lends, a job running on
/// it, and a job that is waiting with the reason beside it.
#[tokio::test(flavor = "multi_thread")]
async fn the_document_names_a_real_fleet_a_real_job_and_what_a_queued_job_waits_for() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;

    // A hub with no runner publishes, and what it publishes is an empty
    // fleet rather than silence: "nobody is connected" is an answer, and it
    // is the answer an operator most needs after an outage.
    let alone = until(&herd, "a document at all", PATIENCE, |_| true).await;
    assert_eq!(
        alone.status_version, STATUS_VERSION,
        "the document carries its own format version (Q7)"
    );
    assert!(
        alone.runners.is_empty() && alone.away.is_empty(),
        "no runner is connected and none is approved yet: {alone}"
    );
    assert!(
        !alone.hub.version.is_empty() && alone.hub.protocol_version > 0,
        "the hub names its own build and the protocol it speaks: {alone}"
    );
    assert!(
        !alone.hub.stopping,
        "a hub that is serving does not say it is stopping"
    );

    // A real runner joins. Everything asserted below is the machine's own
    // word, carried through the handshake — not a figure the hub invented.
    let _runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &runner_options()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let fleet = until(&herd, "the runner", PATIENCE, |status| {
        !status.runners.is_empty()
    })
    .await;
    assert_eq!(fleet.runners.len(), 1, "one runner joined: {fleet}");
    let runner = &fleet.runners[0];
    assert_eq!(runner.name, "the-real-runner");
    assert!(
        runner.version.is_some(),
        "the runner announced its build and the document carries it (E-51, \
         D166): {fleet}"
    );
    assert_eq!(
        runner.switch,
        herd_proto::Switch::On,
        "its owner's switch is on (E-33): {fleet}"
    );
    assert!(
        runner.lends.memory_mib.is_some() && runner.lends.threads.is_some(),
        "the document says what this machine lends, from the machine's own \
         handshake (E-22): {fleet}"
    );
    let capabilities = &runner.lends.capabilities;
    assert_eq!(
        capabilities.len(),
        1,
        "the runner declared exactly one capability: {fleet}"
    );
    assert_eq!(capabilities[0].label, WHAT_IT_LENDS);
    assert_eq!(
        capabilities[0].kind, "shared",
        "a capability declared with no kind is shared: {fleet}"
    );
    assert_eq!(
        capabilities[0].slots, None,
        "a shared capability has no holder limit, and that is not the same \
         statement as a limit of zero: {fleet}"
    );
    assert!(
        capabilities[0].suspended.is_none(),
        "a capability with no probe is not suspended: {fleet}"
    );
    assert!(
        capabilities[0].origins.declared,
        "the document says where the capability came from (E-24): {fleet}"
    );

    // E-40: the age is in the document, as an instant, and it is the
    // *reports seen* figure that tells a runner which has only just joined
    // from one that has gone quiet.
    let reporting = until(&herd, "a health report", PATIENCE, |status| {
        status
            .runners
            .first()
            .is_some_and(|runner| runner.health_reports_seen > 0)
    })
    .await;
    let runner = &reporting.runners[0];
    assert!(
        runner.last_health_at >= fleet.runners[0].last_health_at,
        "a health report moves the moment the hub last heard from the \
         machine, and never backwards: {reporting}"
    );

    // A job nothing in this fleet can run stays queued, and the document
    // carries the reason the record carries (E-04, E-36, D84).
    let waiting = herd
        .submit_needing("no-machine-here-offers-this", "step", "printf never")
        .await;
    let held = until(&herd, "a queued job with its reason", PATIENCE, |status| {
        status
            .queued
            .iter()
            .any(|job| job.id.as_str() == waiting && job.waiting_for.is_some())
    })
    .await;
    let queued = held
        .queued
        .iter()
        .find(|job| job.id.as_str() == waiting)
        .expect("the job just found above");
    assert_eq!(
        queued.labels,
        vec!["no-machine-here-offers-this".to_owned()],
        "the document says what the job asked for: {held}"
    );
    assert_eq!(
        queued.waiting_for.as_deref(),
        Some(herd.waiting_for(&waiting).as_str()),
        "the document's reason is the record's reason, word for word: {held}"
    );
    assert!(
        held.to_string().contains(&format!(
            "waiting for: {}",
            queued.waiting_for.as_deref().unwrap_or_default()
        )),
        "the text screen prints the reason the record holds, not a blank: \
         {held}"
    );

    // And a job this fleet *can* run is named where it is running, with the
    // line that explains why it is there (E-36).
    let running = herd
        .submit_needing(WHAT_IT_LENDS, "step", "sleep 3; printf done")
        .await;
    let out = until(&herd, "the job on its runner", PATIENCE, |status| {
        status.running.iter().any(|job| job.id.as_str() == running)
    })
    .await;
    let job = out
        .running
        .iter()
        .find(|job| job.id.as_str() == running)
        .expect("the job just found above");
    assert_eq!(job.runner, "the-real-runner");
    assert_eq!(job.attempt, 1);
    assert!(
        job.state == "assigned" || job.state == "running",
        "a job out on a machine is assigned or running, and the document says \
         which: {out}"
    );
    assert!(
        !job.why_here.is_empty() && job.why_here.contains("the-real-runner"),
        "every placement carries its one explaining line (E-36): {out}"
    );
    assert!(
        out.runners[0]
            .holding
            .iter()
            .any(|id| id.as_str() == running)
            || out.running.len() == 1,
        "the machine's own word about what it holds is in the document \
         beside the records': {out}"
    );

    // The job ends, and the document stops claiming it is running — the
    // queued one is still there, because nothing in this fleet can take it.
    let after = until(&herd, "the job to end", PATIENCE, |status| {
        !status.running.iter().any(|job| job.id.as_str() == running)
    })
    .await;
    assert!(
        after.queued.iter().any(|job| job.id.as_str() == waiting),
        "the job nobody can run is still waiting: {after}"
    );
}

/// The document is published **on change**, and a hub where nothing happens
/// publishes nothing at all — reads included.
///
/// Two measurements in one hub, because each is only decisive beside the
/// other. See [`QUIET`] and [`SOON`] for why the two spans are the way round
/// they are.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_happening_writes_nothing_and_a_change_is_published_at_once() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let path = herd.state.path().join(STATUS_FILE);
    let before = until(&herd, "a document at all", PATIENCE, |_| true).await;
    let bytes = std::fs::read(&path).expect("the document reads");
    let written_at = std::fs::metadata(&path)
        .expect("the document has metadata")
        .modified()
        .expect("this filesystem keeps modification times");

    // The quiet window, with the document being hammered throughout: this is
    // the "reading it writes nothing" half as well as the "no clock" half,
    // and one window proves both because a read that wrote would show up as
    // the same changed byte or timestamp a clock would.
    let quiet_until = Instant::now() + QUIET;
    let mut reads = 0_u32;
    while Instant::now() < quiet_until {
        let read = Status::read(herd.state.path())
            .expect("the document reads")
            .expect("the document is there");
        assert!(
            read.same_but_for_the_clock(&before),
            "nothing happened, so nothing the document says may have changed"
        );
        reads += 1;
    }
    assert!(
        reads > 100,
        "the document was read {reads} times, which is too \
         few to say anything about reads"
    );
    assert_eq!(
        std::fs::read(&path).expect("the document reads"),
        bytes,
        "nothing happened for {QUIET:?} and the document was read {reads} \
         times: not one byte of it may have changed. A document that moved \
         here is a document on a clock, or a read that writes."
    );
    assert_eq!(
        std::fs::metadata(&path)
            .expect("the document has metadata")
            .modified()
            .expect("this filesystem keeps modification times"),
        written_at,
        "the file was not rewritten at all over {QUIET:?} — the same \
         assertion as the bytes above, and a stronger one: a rewrite with \
         identical content would still move this"
    );
    assert_eq!(
        document(&herd).as_of,
        before.as_of,
        "even the moment inside the document has not moved: this hub \
         published once and then, with nothing happening, never again"
    );

    // Now a runner joins, which is a change, and the document has to carry
    // it well inside the span over which nothing was written above.
    let began = Instant::now();
    let _runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &runner_options()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let fleet = until(&herd, "the runner", PATIENCE, |status| {
        !status.runners.is_empty()
    })
    .await;
    let took = began.elapsed();
    assert!(
        took < SOON,
        "a runner joining took {took:?} to reach the document, which is not \
         inside {SOON:?}. Measured from before the runner process was even \
         spawned, so this bounds the publisher and the runner's own startup \
         together: {fleet}"
    );
    assert_ne!(
        fleet.as_of, before.as_of,
        "the publication that carried the change is a new one"
    );

    // Three more changes in a row, each measured on its own, so that the one
    // above cannot have been a clock that happened to be due. Each job is
    // one nothing here can run, so it stays in the queue and the document
    // keeps growing by exactly one entry.
    for round in 1..=3 {
        let label = format!("nobody-offers-this-{round}");
        let began = Instant::now();
        let id = herd.submit_needing(&label, "step", "printf never").await;
        let queued = until(&herd, &format!("job {id}"), PATIENCE, |status| {
            status.queued.iter().any(|job| job.id.as_str() == id)
        })
        .await;
        let took = began.elapsed();
        assert!(
            took < SOON,
            "round {round}: the submission took {took:?} to reach the \
             document, past {SOON:?}: {queued}"
        );
        assert_eq!(
            queued.queued.len(),
            round,
            "round {round}: the document grew by exactly this submission: \
             {queued}"
        );
    }
}

/// Reading the document against a hub that is serving a real job takes no
/// lock: `herd-hub status` answers while `hub.lock` is held.
///
/// The library read is proved not to write by the previous test's quiet
/// window; what is proved here is the other half, and it is the half an
/// operator meets — the command they run inside the hub's own container,
/// against the root the hub is holding, while work is in flight.
#[tokio::test(flavor = "multi_thread")]
async fn herd_hub_status_answers_while_the_hub_holds_its_lock_and_a_job_runs() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &runner_options()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let running = herd
        .submit_needing(WHAT_IT_LENDS, "step", "sleep 3; printf done")
        .await;
    until(&herd, "the job on its runner", PATIENCE, |status| {
        status.running.iter().any(|job| job.id.as_str() == running)
    })
    .await;

    // The real binary, against the live root, with the hub holding the
    // `flock` on `hub.lock` and a job in flight. A command that took that
    // lock would block here until the hub stopped; one that walked the
    // volume would be slow. This neither blocks nor walks.
    let began = Instant::now();
    let output = tokio::process::Command::new(real_herd::binary("herd-hub"))
        .arg("status")
        .arg(herd.state.path())
        .output()
        .await
        .expect("herd-hub status runs");
    let took = began.elapsed();
    let screen = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "herd-hub status failed against a serving hub: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        took < SOON,
        "herd-hub status took {took:?} against a serving hub, past {SOON:?}: \
         it is meant to open one file and print it"
    );
    assert!(
        screen.contains("runners connected: 1"),
        "the screen names the fleet it read: {screen}"
    );
    assert!(
        screen.contains("the-real-runner"),
        "the screen names the machine: {screen}"
    );
    assert!(
        screen.contains(&running),
        "the screen names the job that is running: {screen}"
    );
    assert!(
        screen.contains("this document was published"),
        "the screen says how old what it printed is, so nobody reads a \
         stopped hub's last word as live: {screen}"
    );

    // And the job the hub was serving throughout came out unharmed.
    let verdict = herd.herdctl(&["wait", &running]).await;
    assert!(
        verdict.status.success(),
        "the job the hub was serving while the document was read ended badly: \
         {}",
        String::from_utf8_lossy(&verdict.stderr)
    );

    // A root no hub has published in is a question that was not answered,
    // and it is not reported as an empty fleet.
    let nowhere = tempfile::tempdir().expect("a temporary directory");
    let refused = tokio::process::Command::new(real_herd::binary("herd-hub"))
        .arg("status")
        .arg(nowhere.path())
        .output()
        .await
        .expect("herd-hub status runs");
    assert!(
        !refused.status.success(),
        "a root with no document must not exit 0 with nothing to say"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(STATUS_FILE),
        "the refusal names the file it did not find: {}",
        String::from_utf8_lossy(&refused.stderr)
    );
}
