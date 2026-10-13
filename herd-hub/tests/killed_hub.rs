//! E-44 against the real binaries: kill the hub, mid-job, with a signal.
//!
//! The in-process tests cannot reach this. Dropping a hub's runtime ends the
//! tasks that were serving a runner, but the runner in those tests is a task
//! in the *same* process, and the socket it was reading dies with a kind of
//! politeness a real kill does not have. What a killed hub does to a runner on
//! another machine is close the connection under it with a reset — and the
//! defect this file exists to forbid is the runner taking that as the end of
//! its life, exiting, killing the job it was running, and leaving the job's
//! record reading `running` for ever (D17).
//!
//! So everything here is a real process: `herd-hub`, `herd-runner` and
//! `herdctl`, as a person would run them, with the hub killed by a signal and
//! started again on the same state root and the same port.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with the other file that drives the real binaries.

mod real_herd;

use std::fs::File;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use herd_proto::JobOutcome;
use real_herd::{binary, Herd, PATIENCE};
use tokio::process::Command;

/// The test that matters: a hub killed while a job is running loses neither
/// the runner, nor the job, nor its result (E-44, D17).
async fn a_killed_hub_loses_nothing(signal: rustix::process::Signal) {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // Long enough to still be running when the hub dies, and loud enough on
    // both sides of the gap to say what survived.
    let job = herd
        .submit("printf 'before the kill\\n'; sleep 4; printf 'after it\\n'; exit 7")
        .await;
    herd.wait_until("the job is running and its first line is recorded", || {
        herd.record(&job)["state"]["type"] == "running"
            && herd.log(&job).contains("before the kill")
    })
    .await;

    // ---- The kill.
    hub.signal(signal).await;
    assert!(!hub.alive(), "the hub is gone");
    // The runner's control connection has just been reset under it. Half a
    // second is far longer than it takes a process to die of that.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        runner.alive(),
        "the runner died with the hub ({signal:?}), so its job died too: {}",
        herd.said("runner")
    );

    // ---- The hub comes back, same port, same state root.
    let mut second = herd.start_hub("second").await;
    assert!(
        herd.said("second").contains("1 job(s) not over"),
        "the hub that comes back inherits the unfinished job: {}",
        herd.said("second")
    );
    assert!(
        herd.said("second")
            .contains("the hub stopped while this job was"),
        "an interrupted attempt must be named, not left reading `running`: {}",
        herd.said("second")
    );

    // A requester that comes back by id alone gets the job's own code and its
    // output, from a hub that never saw the job start (E-03, D16, D22).
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(7),
        "the job's own exit code must survive the kill: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert!(
        String::from_utf8_lossy(&waited.stdout).contains("before the kill"),
        "a wait must print the job's log: {:?}",
        String::from_utf8_lossy(&waited.stdout)
    );

    // And the record is whole: the end as the runner reported it, the
    // interruption named before it, the log still there.
    let record = herd.record(&job);
    assert_eq!(record["state"]["type"], "finished");
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 7})
    );
    let states = herd.states_of(&job);
    assert_eq!(
        states,
        ["queued", "assigned", "running", "interrupted", "finished"],
        "every transition is on the record, in order"
    );
    assert!(
        herd.log(&job).contains("before the kill"),
        "the log the hub wrote before it died: {:?}",
        herd.log(&job)
    );

    // The runner is still a runner, on the hub that came back.
    let after = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'still here\\n'"])
        .await;
    assert_eq!(after.status.code(), Some(0), "{}", herd.said("runner"));
    assert_eq!(String::from_utf8_lossy(&after.stdout), "still here\n");

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_killed_outright_mid_job_loses_nothing() {
    a_killed_hub_loses_nothing(rustix::process::Signal::KILL).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hub_asked_to_stop_mid_job_loses_nothing() {
    a_killed_hub_loses_nothing(rustix::process::Signal::TERM).await;
}

/// D19, with the real binaries: a hub that cannot write a result does not
/// destroy it, and does not drop the runner that is holding it.
///
/// `chmod 500` on a running job's directory is the cheapest honest stand-in
/// for a full disk: every write into that directory fails, and the result the
/// runner is about to report has nowhere to go.
#[tokio::test(flavor = "multi_thread")]
async fn a_result_the_hub_cannot_write_is_kept_until_it_can() {
    use std::os::unix::fs::PermissionsExt;

    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 1; exit 3").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // Nothing can be written in the job's directory from here.
    let dir = herd.job_dir(&job);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("read-only");
    if herd_hub::durable::probe_writable(&dir).is_ok() {
        // Running as a user permissions do not apply to; there is nothing to
        // keep from being written.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable");
        return;
    }

    herd.wait_until("the hub has failed to record the outcome", || {
        herd.said("first").contains("cannot write it")
    })
    .await;
    // The record could not move on, the hub said so in full, and — this is
    // the point — the runner was not dropped for the hub's own failure.
    assert_eq!(herd.record(&job)["state"]["type"], "running");
    assert!(
        herd.said("first").contains("NOT acknowledged"),
        "the hub must say the result is still the runner's: {}",
        herd.said("first")
    );
    assert!(
        runner.alive(),
        "the runner was dropped: {}",
        herd.said("runner")
    );
    let sibling = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'still serving\\n'"])
        .await;
    assert_eq!(
        sibling.status.code(),
        Some(0),
        "the runner's connection did not survive: {}",
        String::from_utf8_lossy(&sibling.stderr)
    );

    // The disk comes back, and so does the hub. The runner still holds the
    // result and redelivers it: a stalled job is recoverable, a destroyed
    // outcome would not have been.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable again");
    hub.signal(rustix::process::Signal::TERM).await;
    let mut second = herd.start_hub("second").await;
    herd.wait_until("the held result is recorded at last", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    assert_eq!(
        herd.record(&job)["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 3}),
        "the outcome the runner held is the outcome recorded"
    );

    second.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// D29, and the one thing a record may never do: a log that is short of
/// output says so, at the place where the output is missing — and M3's, which
/// is that there is nothing to say, because the output comes back.
///
/// Twelve `kill -9`s of the hub during one job is how the defect was found.
/// The hub came back each time, the runner redialled and went on streaming,
/// and everything the job had printed while there was no hub was simply gone —
/// a quarter of the lines, with the record reading `finished{exited 5}` and
/// nothing anywhere saying a byte was missing. M2 made the loss visible; M3
/// makes it not happen: the runner holds every chunk until the hub says it has
/// it, and sends again what was never acknowledged (D36). So the promise this
/// test now holds the code to is the strong one — **not one line of the job's
/// output is missing after twelve killed hubs** — with the marker kept as the
/// fallback for output that really is gone, asserted where it can be
/// provoked (`link.rs`).
#[tokio::test(flavor = "multi_thread")]
async fn a_log_survives_twelve_killed_hubs_whole() {
    const KILLS: u32 = 12;

    let herd = Herd::new();
    let mut hub = herd.start_hub("hub-0").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // A line every 50 ms until the test says stop, so output is always being
    // produced when a hub is killed and always resumes when one comes back.
    // `END` makes a whole line recognisable: a lost chunk can cut a line in
    // half, and a half line must not be mistaken for a line of its own.
    let stop = herd.logs.path().join("stop");
    let job = herd
        .submit(&format!(
            "i=0; while [ ! -f '{}' ]; do i=$((i+1)); printf 'line %s END\\n' \"$i\"; \
             sleep 0.05; done; exit 5",
            stop.display()
        ))
        .await;
    herd.wait_until("the job is running and its output is arriving", || {
        herd.record(&job)["state"]["type"] == "running" && herd.log(&job).contains("line 1 END")
    })
    .await;

    // A whole line of the job's own, as the log keeps it. A line a lost chunk
    // cut in half is not one.
    let whole_lines = || herd.log(&job).matches(" END\n").count();

    for round in 1..=KILLS {
        // Enough output between two kills that the log has substance on both
        // sides of every gap, as the job that found this had.
        let so_far = whole_lines();
        herd.wait_until("more of the job's output reaches the hub", || {
            whole_lines() >= so_far + 5
        })
        .await;
        hub.signal(rustix::process::Signal::KILL).await;
        hub = herd.start_hub(&format!("hub-{round}")).await;
    }
    // The output resuming is what makes the last gap visible: the first chunk
    // after a gap is the one whose number does not follow.
    let before_the_end = whole_lines();
    herd.wait_until("the output resumes on the hub that came back", || {
        whole_lines() > before_the_end
    })
    .await;

    // The job ends, on the twelfth hub, and keeps its own exit code.
    File::create(&stop).expect("the stop file");
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(5),
        "the job's own code must survive twelve kills: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert_eq!(herd.record(&job)["state"]["type"], "finished");

    // ---- What the log says about itself. The job numbered its own lines, so
    // the log is checkable against the job's intent rather than against the
    // hub's bookkeeping: the numbers must run 1, 2, 3, … with nothing missing,
    // nothing repeated, and nothing out of order. A gap is allowed only if the
    // hub named it where it happened (D29) — and after M3 there should be
    // none, because the runner held the chunks and sent them again (D36).
    let log = herd.log(&job);
    let mut last: Option<u64> = None;
    let mut announced = false;
    let mut gaps = 0u32;
    let mut lines = 0u32;
    for line in log.lines() {
        if line.contains("never reached this log") {
            announced = true;
            continue;
        }
        let Some(number) = line
            .strip_prefix("line ")
            .and_then(|rest| rest.strip_suffix(" END"))
            .and_then(|digits| digits.parse::<u64>().ok())
        else {
            // A line a lost chunk cut in half, or the blank line a marker
            // stands on. Neither is one of the job's lines.
            continue;
        };
        lines += 1;
        if let Some(last) = last {
            assert!(
                number > last,
                "line {number} is in the log twice, or out of order, after \
                 line {last}: a log with a line written twice is as false as \
                 one with a line missing. {log}"
            );
            if number != last + 1 {
                assert!(
                    announced,
                    "lines {} to {} are missing from the log and nothing says so: \
                     a record that is short of output and silent about it is the \
                     defect this test exists for",
                    last + 1,
                    number - 1
                );
                gaps += 1;
            }
        }
        announced = false;
        last = Some(number);
    }
    assert!(
        lines > 100,
        "the log holds too few of the job's lines: {log}"
    );
    assert_eq!(
        last,
        Some(lines as u64),
        "the job printed {lines} lines numbered 1 upwards and the log must \
         hold every one of them: twelve killed hubs may cost a job nothing \
         (D36)"
    );
    assert_eq!(
        gaps, 0,
        "twelve killed hubs cost this job {gaps} stretch(es) of output; the \
         runner holds what it sends until the hub acknowledges it, so none \
         should be missing. {log}"
    );
    // The record agrees with the log about that: no gap in one, no gap event
    // in the other. A reader of the record alone must not have to parse the
    // log to learn whether it is complete.
    let markers = log.matches("never reached this log").count();
    let path = herd.job_dir(&job).join("events.jsonl");
    let events = std::fs::read_to_string(&path).expect("the events read");
    let recorded = events
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["event"] == "log_gap")
        .count();
    assert_eq!(
        recorded, markers,
        "every marker in the log is an event on the record: {events}"
    );
    assert_eq!(markers, 0, "nothing was lost, so nothing is named as lost");
    // One attempt, throughout. Twelve hubs died; the job was never run twice.
    let record = herd.record(&job);
    assert_eq!(
        record["attempts"].as_array().expect("attempts").len(),
        1,
        "a hub dying is not a runner being lost: {record}"
    );
    assert_eq!(
        herd.states_of(&job).first().map(String::as_str),
        Some("queued"),
        "and the transitions are still the transitions: {events}"
    );

    // Twelve kills, and nothing half written left behind in the job's
    // directory either.
    let litter: Vec<String> = std::fs::read_dir(herd.job_dir(&job))
        .expect("the job directory lists")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(
        litter.is_empty(),
        "a kill left a temporary behind: {litter:?}"
    );

    hub.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// D19's other half, with the real binaries: a wait on a job the hub cannot
/// write down says so, instead of saying nothing at all.
///
/// The record of such a job reads `running` and stays there — the runner holds
/// the result, this hub has stopped trying to store it — and a `herdctl wait`
/// on it used to print absolutely nothing, with no way for a requester to tell
/// a stalled job from a slow one. How long a hub tries before giving up is
/// M3's policy; being mute about it was never a policy.
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_on_a_job_the_hub_cannot_write_down_says_so() {
    use std::os::unix::fs::PermissionsExt;

    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 1; exit 3").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // Nothing can be written in the job's directory from here, so the outcome
    // the runner is about to report has nowhere to go.
    let dir = herd.job_dir(&job);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("read-only");
    if herd_hub::durable::probe_writable(&dir).is_ok() {
        // Running as a user permissions do not apply to; there is nothing to
        // keep from being written.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable");
        return;
    }

    herd.wait_until("the hub has given up writing the outcome", || {
        herd.said("first").contains("cannot write it")
    })
    .await;
    assert_eq!(
        herd.record(&job)["state"]["type"],
        "running",
        "the record cannot have moved on"
    );

    // A requester comes back to the job by its id. The job is not over, and
    // nothing is going to happen to it: the wait has to say that much.
    let mut waiting = herd.start_wait(&job);
    let said = async {
        loop {
            if herd.said("wait").contains("cannot write it down") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), said)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the wait said nothing about a job the hub cannot write down: {:?}",
                herd.said("wait")
            )
        });
    assert!(
        herd.said("wait").contains(&job),
        "the note must carry the reason, which names the file: {:?}",
        herd.said("wait")
    );
    assert!(
        herd.said("wait").contains("redelivers it"),
        "and what happens next: {:?}",
        herd.said("wait")
    );
    // Still waiting: no verdict has been invented for a job that has one, in
    // the runner's memory, that the hub could not store.
    assert!(waiting.alive(), "the wait must not have answered anything");
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(herd.logs.path().join("wait-out")).unwrap()),
        "working\n",
        "stdout stays the job's own output; the note is Herd speaking, on stderr"
    );

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable again");
    waiting.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
    runner.signal(rustix::process::Signal::TERM).await;
}

/// D17's other half: when nothing comes back, the job stays unfinished but is
/// *named* as interrupted, and a wait on it says so instead of hanging mutely
/// for ever.
///
/// The hub and the runner are both killed, so the result exists nowhere: the
/// runner held it in memory and the runner is gone. M2 does not decide that
/// such an attempt is lost — the ~60 s silence threshold and the requeue are
/// M3's — so the wait does go on waiting. What it may not do is wait in
/// silence, which is what it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_wait_on_an_interrupted_job_says_so_rather_than_hanging_mutely() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd.submit("printf 'working\\n'; sleep 30").await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;

    // Both ends of the attempt, gone. Nothing can report this job's end.
    hub.signal(rustix::process::Signal::KILL).await;
    runner.signal(rustix::process::Signal::KILL).await;

    let mut second = herd.start_hub("second").await;
    let mut waiting = herd.start_wait(&job);
    let said = async {
        loop {
            if herd.said("wait").contains("interrupted") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), said)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the wait said nothing about a job whose attempt was interrupted: {:?}",
                herd.said("wait")
            )
        });
    assert!(
        herd.said("wait")
            .contains("the hub stopped while this job was"),
        "the note must carry the reason the record holds: {:?}",
        herd.said("wait")
    );
    // Still waiting, as M2 means it to: no verdict has been invented.
    assert!(waiting.alive(), "the wait must not have answered anything");
    assert_eq!(
        herd.record(&job)["state"]["type"],
        "interrupted",
        "the record names the interruption rather than reading `running`"
    );

    waiting.signal(rustix::process::Signal::TERM).await;
    second.signal(rustix::process::Signal::TERM).await;
}

/// E-21 against the real binaries: `kill -9` the runner mid-job, and the job
/// is attempted again on the runner that takes its place, with the record
/// naming both goes.
///
/// The in-process version of this (`link.rs`) drives a hand-written runner
/// that goes quiet. This one kills a process: the socket really closes, the
/// job's child really dies with it, the result it was holding really is gone,
/// and the hub has to decide on a clock that the attempt is not coming back.
/// That is the failure mode that got through M2's first pass, so it is the one
/// a real-binary test has to cover.
#[tokio::test(flavor = "multi_thread")]
async fn a_killed_runner_s_job_is_attempted_again_on_another_runner() {
    let herd = Herd::new();
    // Three seconds instead of sixty: the threshold is the hub's policy, and
    // what is being proved is what happens when it passes.
    let mut hub = herd.start_hub_losing_runners_after("first", 3.0).await;
    let mut doomed = herd.start_runner_called("the-doomed-runner", "runner");
    herd.wait_for_runner().await;

    // A command that behaves differently the second time it is run, so that
    // the first attempt is still going when its runner is killed and the
    // second one can finish. It is also the shape of the thing being tested:
    // the job really is run twice, on two machines, and the record has to say
    // so.
    let marker = herd.logs.path().join("ran-once");
    let job = herd
        .submit(&format!(
            "if [ -f '{0}' ]; then printf 'the second attempt\\n'; exit 0; fi; \
             : > '{0}'; printf 'the first attempt\\n'; sleep 120; printf 'never\\n'",
            marker.display()
        ))
        .await;
    herd.wait_until("the job is running and has printed something", || {
        herd.record(&job)["state"]["type"] == "running"
            && herd.log(&job).contains("the first attempt")
    })
    .await;

    // ---- The kill. Nothing of this attempt survives on the runner's side:
    // the process is gone and so is the result it would have reported.
    let killed_at = Instant::now();
    doomed.signal(rustix::process::Signal::KILL).await;
    assert!(!doomed.alive(), "the runner is gone");

    // A runner takes its place, and the hub hands it the job again.
    let mut replacement = herd.start_runner_called("the-replacement-runner", "second-runner");
    // Waiting on the *transitions*, not only on the record: the hub writes
    // `job.json` and then appends to `events.jsonl`, in that order and on
    // purpose (a transition is readable in the journal only once the record
    // agrees with it), so a test that reads the record and then the journal
    // in one breath can see a finished job with its last line not yet
    // there. That is the race, not a missing transition.
    herd.wait_until(
        "the job ends on the runner that replaced the dead one",
        || {
            herd.record(&job)["state"]["type"] == "finished"
                && herd
                    .states_of(&job)
                    .last()
                    .is_some_and(|last| last == "finished")
        },
    )
    .await;
    let took = killed_at.elapsed();
    assert!(
        took < Duration::from_secs(90),
        "a killed runner's job must be attempted again within 90 s; this took {took:?}"
    );

    // ---- The record. Two attempts, each naming its runner, the first one
    // saying why it ended, and the second one's verdict kept.
    let record = herd.record(&job);
    let attempts = record["attempts"].as_array().expect("attempts").clone();
    assert_eq!(
        attempts.len(),
        2,
        "a reader must see that this job ran twice and why: {record}"
    );
    assert_eq!(attempts[0]["number"], 1);
    assert_eq!(attempts[0]["runner"], "the-doomed-runner");
    let why = attempts[0]["ended"]
        .as_str()
        .expect("the attempt that was lost says why it ended");
    assert!(
        why.contains("runner")
            && (why.contains("not coming back") || why.contains("stopped answering")),
        "the reason must name the lost runner: {why}"
    );
    assert_eq!(attempts[1]["number"], 2);
    assert_eq!(attempts[1]["runner"], "the-replacement-runner");
    assert!(
        attempts[1]["ended"].is_null(),
        "the attempt that produced the verdict needs no excuse: {record}"
    );
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0}),
        "the second attempt's own verdict, not an invented one: {record}"
    );

    // The transitions say the same thing in order, and the log says where the
    // second attempt begins so that nobody reads the first one's output as
    // part of it.
    assert_eq!(
        herd.states_of(&job),
        ["queued", "assigned", "running", "queued", "assigned", "running", "finished"],
        "the job went round exactly once"
    );
    let log = herd.log(&job);
    assert!(log.contains("the first attempt"), "{log}");
    assert!(log.contains("the second attempt"), "{log}");
    assert!(
        log.contains("attempt 2 of this job starts here"),
        "the log must say where the second attempt begins: {log}"
    );
    assert!(
        log.find("the first attempt") < log.find("the second attempt"),
        "in the order they happened: {log}"
    );
    assert!(
        !log.contains("never\n"),
        "the first attempt never got there: {log}"
    );

    // And a requester that comes back by id gets the second attempt's code.
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(0),
        "{}",
        herd.said("second-runner")
    );

    replacement.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// Invariant 2 against the real binary: a busy runner asked to stop is gone
/// inside five seconds, has refused a job submitted in the meantime, and the
/// job it was running is named on the record rather than silently lost.
///
/// `SIGTERM` is how an owner asks today — `systemctl stop`, a Ctrl-C, a
/// container told to go. The switch she flips from her desktop is E-33 and
/// M9's; this is the stop path it pulls.
#[tokio::test(flavor = "multi_thread")]
async fn a_busy_runner_asked_to_stop_is_gone_in_five_seconds() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd
        .submit("printf 'working\\n'; sleep 600; printf 'never\\n'")
        .await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running" && herd.log(&job).contains("working")
    })
    .await;

    // ---- The owner asks. Nothing is waited for before the clock starts.
    let asked_at = Instant::now();
    runner.ask_it_to_stop();

    // A job submitted while it is draining is refused by name, rather than
    // handed to a runner that is leaving. Polled, because the refusal becomes
    // true the moment the hub hears the runner is draining — which is the
    // first thing a draining runner says.
    let mut refusal = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let attempt = herd.herdctl(&["run", "--", "/bin/sh", "-c", "true"]).await;
        if !attempt.status.success() {
            refusal = String::from_utf8_lossy(&attempt.stderr).into_owned();
            assert_eq!(
                attempt.status.code(),
                Some(125),
                "a refused submission has no verdict to report (D20): {refusal}"
            );
            break;
        }
    }
    assert!(
        refusal.contains("draining") || refusal.contains("no runner is connected"),
        "a job submitted while the runner is stopping must be refused by name: {refusal:?}"
    );

    // ---- And it is gone, inside the five seconds invariant 2 promises.
    let status = tokio::time::timeout(Duration::from_secs(5), runner.gone())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "invariant 2: a runner asked to stop is gone in five seconds. \
                 It said: {}",
                herd.said("runner")
            )
        });
    let took = asked_at.elapsed();
    assert!(
        took <= Duration::from_secs(5),
        "invariant 2 promises five seconds; this took {took:?}"
    );
    assert_eq!(
        status.code(),
        Some(0),
        "a runner asked to stop stops cleanly: {}",
        herd.said("runner")
    );
    assert!(
        herd.said("runner").contains("draining"),
        "and says what it is doing: {}",
        herd.said("runner")
    );

    // ---- What became of the job it was running. Named, with the reason, and
    // waiting for a runner that can take it — never silently lost.
    // Both facts, because they are two writes. The hub ends the attempt on
    // the record, says in the log what that attempt's output lost, and only
    // then requeues the job — so there is a moment in which the attempt reads
    // `ended` while the state still reads `running`, and a wait on the first
    // of them alone caught it about once in three runs. Waiting on the pair
    // asserts no less: a hub that left the job `running` for good still times
    // out here, with the record in the message.
    herd.wait_until(
        "the drained job's attempt is ended by name and the job is no longer running",
        || {
            let record = herd.record(&job);
            record["attempts"][0]["ended"].is_string() && record["state"]["type"] != "running"
        },
    )
    .await;
    let record = herd.record(&job);
    let why = record["attempts"][0]["ended"]
        .as_str()
        .expect("the attempt says why it ended");
    assert!(
        why.contains("asked it to stop"),
        "the record must say the owner asked: {why}"
    );
    assert_eq!(
        record["state"]["type"], "queued",
        "the job waits for a runner rather than being failed: {record}"
    );
    assert!(
        herd.log(&job).contains("working"),
        "what it printed before the drain is still its log: {}",
        herd.log(&job)
    );
    assert!(
        !herd.log(&job).contains("never"),
        "and it did not go on running: {}",
        herd.log(&job)
    );

    hub.signal(rustix::process::Signal::TERM).await;
}

/// Where M4's stop meets M3's drain and M2's held result, with the real
/// binaries: a runner told to stop while it is holding a verdict no hub has
/// acknowledged does **not** take that verdict with it (D34).
///
/// The job here has already *finished* when the stop arrives. That is the
/// case the spool exists for, and the reconciliation of the two milestones:
/// a job still **running** when the owner asks is abandoned by name, because
/// inventing "killed by signal 9" for work nobody finished would be a false
/// verdict and the hub would have no reason to attempt the job again
/// (invariant 2, D39 — that is
/// `a_busy_runner_asked_to_stop_is_gone_in_five_seconds`). A job that *ran to
/// its end* produced a real verdict, the hub never took it, and nothing is
/// allowed to destroy it (D19, E-44).
///
/// What is written is the `job_finished` frame itself, so nothing has to be
/// reinterpreted to replay it (invariant 4). A verdict on a disk is
/// recoverable; a verdict in the memory of a process that has exited is not.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_stopped_while_holding_a_result_leaves_it_on_disk() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd
        .submit("printf 'working\\n'; sleep 1; printf 'done\\n'; exit 7")
        .await;
    herd.wait_until("the job is running and has printed something", || {
        herd.record(&job)["state"]["type"] == "running" && herd.log(&job).contains("working")
    })
    .await;

    // The hub goes first, so there is nothing left to acknowledge anything.
    // The job keeps running under the runner, which is D17's whole point.
    hub.signal(rustix::process::Signal::KILL).await;
    assert!(!hub.alive(), "the hub is gone");
    herd.wait_until("the runner has noticed the hub is gone", || {
        herd.said("runner").contains("the link to the hub ended")
    })
    .await;
    // And now it finishes, with nobody to tell. The result exists in exactly
    // one place: the memory of this runner.
    herd.wait_until("the job ended with no hub to tell", || {
        herd.said("runner")
            .contains("ended while there was no hub to tell")
    })
    .await;
    assert!(runner.alive(), "D17: a runner outlives its hub");

    // ---- The owner asks it to stop, and it must not leave with the verdict.
    runner.ask_it_to_stop();
    let status = tokio::time::timeout(Duration::from_secs(5), runner.gone())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "invariant 2: a runner asked to stop is gone in five seconds. It said: {}",
                herd.said("runner")
            )
        });
    assert_eq!(status.code(), Some(0), "{}", herd.said("runner"));

    let said = herd.said("runner");
    let held = herd.runner_root("runner").join("held-results");
    let spooled: Vec<PathBuf> = std::fs::read_dir(&held)
        .unwrap_or_else(|error| {
            panic!(
                "a runner that stopped holding a result must leave it in {}: \
                 {error}. It said: {said}",
                held.display()
            )
        })
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(
        spooled.len(),
        1,
        "exactly the one unacknowledged result belongs there: {spooled:?}"
    );
    let frame = std::fs::read_to_string(&spooled[0]).expect("the held result reads");
    let message: herd_proto::ToHub = herd_proto::from_frame(&frame).unwrap_or_else(|error| {
        panic!("a held result is a frame; {frame} did not decode: {error}")
    });
    match message {
        herd_proto::ToHub::JobFinished {
            job_id, outcome, ..
        } => {
            assert_eq!(job_id.as_str(), job, "the frame names the job it is about");
            assert_eq!(
                outcome,
                JobOutcome::Exited { code: 7 },
                "the job ran to its end, so the real verdict is what is kept"
            );
        }
        other => panic!("a held result is a job_finished frame, not {other:?}"),
    }
    // Impossible to miss: a verdict that reached no hub is said out loud, with
    // the path, never left for someone to discover.
    assert!(
        said.contains("never acknowledged") && said.contains(&held.display().to_string()),
        "the runner must say where the verdict went: {said}"
    );
    // And nothing invented a verdict the hub was never told: the record is
    // not finished. It reads `running` because this hub was killed outright
    // and never got to name the interruption.
    assert_ne!(
        herd.record(&job)["state"]["type"],
        "finished",
        "nothing may claim a verdict the hub was never told"
    );
}

/// The obligation M4 left open, closed: a spooled verdict reaches the hub by
/// itself, with no human in the loop (D34).
///
/// The runner of the test above wrote a verdict to its data root and exited.
/// Here a hub comes back on the same state root and a runner comes back on
/// the same data root — which is a restart, nothing more — and the job's
/// record ends up carrying the exit code that job really had. Then the file
/// is gone, because the hub acknowledged it, and only because of that.
#[tokio::test(flavor = "multi_thread")]
async fn a_spooled_verdict_reaches_the_hub_when_the_runner_comes_back() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd
        .submit("printf 'working\\n'; sleep 1; printf 'done\\n'; exit 7")
        .await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running" && herd.log(&job).contains("working")
    })
    .await;
    hub.signal(rustix::process::Signal::KILL).await;
    herd.wait_until("the job ended with no hub to tell", || {
        herd.said("runner")
            .contains("ended while there was no hub to tell")
    })
    .await;
    runner.ask_it_to_stop();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner.gone()).await;
    let held = herd.runner_root("runner").join("held-results");
    assert_eq!(
        std::fs::read_dir(&held).into_iter().flatten().count(),
        1,
        "the verdict is on the disk before anything is restarted"
    );

    // ---- The restart. Nobody tells either process anything about the file.
    let mut hub = herd.start_hub("second").await;
    let mut second = herd.start_runner_rooted("the-real-runner", "second-runner", "runner");
    herd.wait_until("the spooled verdict is picked up", || {
        herd.said("second-runner").contains("picked up the verdict")
    })
    .await;
    herd.wait_until("the job's record carries the verdict it really had", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    let record = herd.record(&job);
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 7}),
        "the verdict that reached the hub a restart late is the job's own: {record}"
    );

    // Acknowledged, and only then removed: the file is the one copy until the
    // hub has it on a disk (D19).
    herd.wait_until("the spool file is gone once the hub has it", || {
        std::fs::read_dir(&held).into_iter().flatten().count() == 0
    })
    .await;
    assert!(
        herd.said("second-runner").contains("been acknowledged"),
        "delivering a verdict a restart late is said out loud: {}",
        herd.said("second-runner")
    );

    // And a requester coming back by id is given that code, not Herd's own.
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(
        waited.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&waited.stderr)
    );

    second.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// The tail gap, with the exact reproduction M4's verification used: `kill -9`
/// the hub while a job is running, let the job's last line be the one that is
/// lost, start the hub again, and ask the requester what it got.
///
/// Before D36 this was a silently short log reported as a success: the earlier
/// output, exit 42, and no sign anywhere that a line was missing. A gap is
/// only detectable when the chunk *after* it arrives, so chunks lost with
/// nothing following them left the record short and said nothing — the defect
/// class this project keeps refusing.
///
/// What closes it is the chunk count on `job_finished`: the hub learns how
/// many chunks the attempt wrote, asks for what it is missing, and names the
/// shortfall in the log if it never comes. So the requester ends with either
/// the missing line or a marker that says it is gone. Never silence.
#[tokio::test(flavor = "multi_thread")]
async fn a_log_that_loses_its_tail_says_so_rather_than_reading_short() {
    let herd = Herd::new();
    let mut hub = herd.start_hub("first").await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    // `before` lands while the hub is up; `after` is written in the instant
    // the hub does not exist, so it is the tail — and the end follows it with
    // nothing in between to give the loss away.
    let job = herd
        .submit("printf 'before\\n'; sleep 2; printf 'after\\n'; exit 42")
        .await;
    herd.wait_until("the first line is recorded", || {
        herd.log(&job).contains("before")
    })
    .await;

    hub.signal(rustix::process::Signal::KILL).await;
    assert!(!hub.alive(), "the hub is gone");
    // The job goes on and writes its tail into a link that is not there.
    herd.wait_until("the runner has noticed the hub is gone", || {
        herd.said("runner").contains("the link to the hub ended")
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(runner.alive(), "D17: a runner outlives its hub");

    let mut hub = herd.start_hub("second").await;
    herd.wait_until("the job ends on the hub that came back", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;

    // The verdict is the job's own, and the log is not allowed to be quietly
    // short: either the line is there, or something names it as lost.
    let record = herd.record(&job);
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 42}),
        "{record}"
    );
    let log = herd.log(&job);
    let names_the_gap = log.contains("missing") || log.contains("output") && log.contains("lost");
    assert!(
        log.contains("after") || names_the_gap,
        "a log that lost its tail must either get it back or say it is gone, \
         never read short in silence: {log:?}"
    );

    // And the requester reading it by id sees exactly the same thing: the
    // exit code the job really had, over a log that is honest about itself.
    let waited = herd.herdctl(&["wait", &job]).await;
    assert_eq!(waited.status.code(), Some(42), "{log:?}");
    let seen = String::from_utf8_lossy(&waited.stdout).into_owned();
    assert!(seen.contains("before"), "{seen:?}");
    assert!(
        seen.contains("after") || seen.contains("missing") || seen.contains("lost"),
        "the requester must not be handed a short log with nothing said: {seen:?}"
    );

    runner.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// **Fix 1 against the real binaries: whose verdict is it?**
///
/// A job runs as attempt 1 on `runner-x`. A `job_finished` frame for that
/// exact attempt is planted in `runner-y`'s spool — the one place a verdict
/// legitimately waits — `runner-x` is killed outright, and `runner-y` starts
/// and offers what it found. Nothing about the frame is malformed: it names a
/// job the hub really has, an attempt the hub really is waiting for, and a
/// state root `runner-y` really does belong to. It is a lie only in its
/// author.
///
/// The hub used to believe it: exit 99 and metrics nobody measured, on a
/// record still naming attempt 1 on `runner-x`, with the real job's output in
/// the log underneath. Now it is refused by name, the planted file is dropped
/// by name on the runner's side (D42), and the job goes on to be attempted
/// again and to end with its own verdict.
#[tokio::test]
async fn a_verdict_planted_in_another_runner_s_spool_is_refused_by_name() {
    let herd = Herd::new();
    let mut hub = herd.start_hub_losing_runners_after("first", 3.0).await;
    let mut x = herd.start_runner_called("runner-x", "runner-x");
    herd.wait_for_runner().await;

    let job = herd
        .submit("printf 'the job that really ran\\n'; sleep 2; exit 5")
        .await;
    herd.wait_until("the job is running on runner-x", || {
        herd.record(&job)["state"]["type"] == "running"
            && herd.log(&job).contains("the job that really ran")
    })
    .await;
    let record = herd.record(&job);
    assert_eq!(
        record["attempts"][0]["runner"],
        serde_json::json!("runner-x")
    );
    let attempt = record["attempts"][0]["number"]
        .as_u64()
        .expect("an attempt is numbered") as u32;

    // The fixture: a verdict for that attempt, in the spool of a runner that
    // never ran it and that genuinely serves this hub's state root.
    herd.pretend_the_root_was_welcomed("runner-y");
    let planted = herd.runner_root("runner-y").join("held-results");
    std::fs::create_dir_all(&planted).expect("the fixture makes the spool directory");
    let planted = planted.join(format!("{job}.json"));
    std::fs::write(
        &planted,
        herd_proto::to_frame(&herd_proto::ToHub::JobFinished {
            job_id: herd_proto::JobId::new(&job),
            attempt,
            outcome: JobOutcome::Exited { code: 99 },
            chunks_sent: 0,
            metrics: herd_proto::JobMetrics {
                log_bytes: 4096,
                wall_time_ms: 77,
                ..herd_proto::JobMetrics::default()
            },
        })
        .expect("the fixture frame encodes"),
    )
    .expect("the fixture writes the planted verdict");

    // `runner-y` joins beside `runner-x` — since M7 this hub holds a fleet —
    // and offers what it found in its spool at once, while the real attempt
    // is still running on the machine it was handed to. That makes the lie
    // sharper than it was: the frame arrives from a connected, healthy,
    // welcome runner, about an attempt that is alive, and it is refused on
    // the one ground that matters — it is not that runner's attempt.
    let mut y = herd.start_runner_called("runner-y", "runner-y");
    herd.wait_until("the hub refuses the planted verdict by name", || {
        herd.said("first").contains(&format!(
            "attempt {attempt} of {job} was handed to runner runner-x"
        ))
    })
    .await;
    assert!(
        herd.said("first").contains("came from runner runner-y"),
        "the refusal names who spoke, or an operator has nothing to go on: {}",
        herd.said("first")
    );
    // Only now is the machine that really is running the job torn out, so
    // that the rest of this test is about the job's own second go.
    x.signal(rustix::process::Signal::KILL).await;

    // Nothing of it was recorded. The record still names attempt 1 on
    // runner-x, carries no verdict and no figures, and the log is the job's
    // own.
    let record = herd.record(&job);
    assert_ne!(
        record["state"]["type"], "finished",
        "a foreign verdict ended the job: {record}"
    );
    assert_eq!(
        record["metrics"],
        serde_json::json!({"log_bytes": 0, "wall_time_ms": 0})
    );
    assert_eq!(
        record["attempts"][0]["runner"],
        serde_json::json!("runner-x")
    );

    // And runner-y drops the file by name rather than offering it for ever.
    herd.wait_until("the planted file is dropped by name", || {
        herd.said("runner-y")
            .contains("the hub will not record the spooled verdict")
            && !planted.exists()
    })
    .await;

    // The job itself is not a casualty of any of this: its attempt was lost
    // with runner-x, so it is attempted again — on runner-y, which is a
    // perfectly good runner — and it ends with its own exit code.
    herd.wait_until("the job ends with its own verdict", || {
        herd.record(&job)["state"]["type"] == "finished"
    })
    .await;
    let record = herd.record(&job);
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 5}),
        "the job's verdict is its own, and 99 was never it: {record}"
    );
    assert_eq!(
        record["attempts"].as_array().map(Vec::len),
        Some(2),
        "two attempts, the first named as lost: {record}"
    );

    y.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// **Fix 2 against the real binaries: the state root that was recreated.**
///
/// No hand-edited file anywhere. A hub's state root is wiped — a new volume,
/// a restored backup, `rm -rf` on a directory — so its job ids start again at
/// `j0000000001`, while the runner's spool from the previous root is still on
/// its disk naming exactly that id. The two together make a verdict that is
/// plausible and false, and that is what M2's author deferred to M3 as "the
/// store's identity in `Welcome`".
#[tokio::test]
async fn a_verdict_spooled_for_a_state_root_that_was_recreated_is_never_believed() {
    let herd = Herd::new();
    let mut hub = herd.start_hub_losing_runners_after("first", 3.0).await;
    let mut runner = herd.start_runner();
    herd.wait_for_runner().await;

    let job = herd
        .submit("printf 'the first life\\n'; sleep 1; exit 3")
        .await;
    herd.wait_until("the job is running", || {
        herd.record(&job)["state"]["type"] == "running"
    })
    .await;
    let first_root_id = herd.state_root_id();

    // The hub dies, the job ends with nobody to tell, and the runner is
    // stopped: the verdict goes to its spool (D34).
    hub.signal(rustix::process::Signal::KILL).await;
    herd.wait_until("the job ended with no hub to tell", || {
        herd.said("runner")
            .contains("ended while there was no hub to tell")
    })
    .await;
    runner.ask_it_to_stop();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner.gone()).await;
    let spool = herd.runner_root("runner").join("held-results");
    assert_eq!(
        std::fs::read_dir(&spool).into_iter().flatten().count(),
        1,
        "the verdict of the first life is on the disk"
    );

    // The state root is recreated. Nobody touches the runner's disk.
    std::fs::remove_dir_all(herd.state.path()).expect("the fixture wipes the state root");
    std::fs::create_dir_all(herd.state.path()).expect("and leaves an empty one");
    let mut hub = herd.start_hub_losing_runners_after("second", 3.0).await;
    let second_root_id = herd.state_root_id();
    assert_ne!(
        first_root_id, second_root_id,
        "a recreated state root is a different state root, or none of this can be told apart"
    );

    let mut again = herd.start_runner_rooted("the-real-runner", "second-runner", "runner");
    herd.wait_until("the runner is told which state root it is holding", || {
        let said = herd.said("second-runner");
        said.contains(&first_root_id) && said.contains(&second_root_id)
    })
    .await;
    assert!(
        herd.said("second-runner").contains("NOT offered"),
        "a verdict for a vanished hub is never offered, and that is said: {}",
        herd.said("second-runner")
    );
    assert!(
        std::fs::read_dir(&spool)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .ends_with(".foreign-state-root")),
        "its bytes are kept under a name that says why, never destroyed (D19)"
    );

    // And now the collision itself. A fresh state root hands out ids from
    // one, so it is walked to the very id the spooled verdict names — which
    // is the whole hazard: the same string, two different jobs, two different
    // hubs' worth of state.
    herd.wait_until("the runner is serving the new state root", || {
        herd.said("second").contains("has joined")
    })
    .await;
    let number: u64 = job
        .trim_start_matches('j')
        .trim_start_matches('0')
        .parse()
        .expect("a job id is a number behind its padding");
    for _ in 1..number {
        herd.submit("true").await;
    }
    let second = herd.submit("printf 'the second life\\n'; exit 0").await;
    assert_eq!(
        second, job,
        "the fresh root hands out the id the spooled verdict names"
    );
    herd.wait_until("the new root's first job is over", || {
        herd.record(&second)["state"]["type"] == "finished"
    })
    .await;
    let record = herd.record(&second);
    assert_eq!(
        record["state"]["outcome"],
        serde_json::json!({"type": "exited", "code": 0}),
        "a verdict from a state root that is gone landed on a live job: {record}"
    );
    assert!(
        herd.log(&second).contains("the second life")
            && !herd.log(&second).contains("the first life"),
        "and the log is this job's own: {}",
        herd.log(&second)
    );

    again.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// **Fix 4: a silence threshold that cannot mean what it says.**
///
/// `--lost-after 8` with the 15 s health cadence E-40 asks for burnt *both*
/// attempts of a live, perfectly healthy job in nineteen seconds and ended it
/// `lost`: the hub was timing the gap between two reports it had itself asked
/// for. It is refused at startup, naming both figures and the margin, by the
/// real binary — because that is where an operator makes this mistake.
///
/// The same mistake is possible on the runner's side, in the one direction it
/// can be made there, and it is refused the same way.
#[tokio::test]
async fn a_lost_after_shorter_than_the_health_cadence_is_refused_at_startup() {
    let herd = Herd::new();
    let hub = std::process::Command::new(binary("herd-hub"))
        .arg("--listen")
        .arg(herd.addr().to_string())
        .arg("--state-dir")
        .arg(herd.state.path())
        .arg("--lost-after")
        .arg("8")
        .output()
        .expect("herd-hub runs");
    assert!(
        !hub.status.success(),
        "a hub that cannot keep its promise about a live runner must not start"
    );
    let said = String::from_utf8_lossy(&hub.stderr);
    for figure in ["8s", "15s", "3 health reports", "45s"] {
        assert!(
            said.contains(figure),
            "the refusal must name {figure}: {said}"
        );
    }
    assert!(
        TcpStream::connect(herd.addr()).is_err(),
        "a refused hub must not be listening"
    );

    let runner = std::process::Command::new(binary("herd-runner"))
        .arg("--hub")
        .arg(herd.url())
        .arg("--data-root")
        .arg(herd.runner_root("refused"))
        .arg("--hub-silence-limit")
        .arg("8")
        .output()
        .expect("herd-runner runs");
    assert!(
        !runner.status.success(),
        "a runner that would redial a live link for ever must not start"
    );
    let said = String::from_utf8_lossy(&runner.stderr);
    for figure in ["8s", "15s", "3 health reports", "45s"] {
        assert!(
            said.contains(figure),
            "the runner's refusal must name {figure} too: {said}"
        );
    }
}

/// **Two timings that are each legal and together kill a healthy job**, with
/// the real binaries and the figures that did it.
///
/// `herd-hub --health-every 1 --lost-after 3` passes the hub's own startup
/// check: three seconds outlasts three of the reports it expects. `herd-runner
/// --health-every 10 --hub-silence-limit 30` passes the runner's: thirty
/// seconds outlasts three of the reports it sends. Both started, and a quiet
/// `sleep 30` burnt **both** of its attempts and ended `lost` in about six
/// seconds — the hub was timing a silence the runner had never promised to
/// break. Neither configuration is wrong alone, so neither check could see
/// it; the runner's real cadence now crosses the handshake and the hub judges
/// the pairing by its figures.
///
/// The defaults (15 and 60 on both sides) are safe, which is exactly why this
/// would be met by the first person to tune one side.
#[tokio::test]
async fn a_runner_reporting_too_rarely_for_the_hub_s_threshold_is_refused_by_figures() {
    let herd = Herd::new();
    // A hub that is entirely self-consistent: it loses a runner after three
    // seconds and expects a report every second.
    let mut hub = herd
        .start_hub_with("first", &["--health-every", "1", "--lost-after", "3"])
        .await;

    // And a runner that is entirely self-consistent too, reporting every ten
    // seconds and giving a silent hub thirty.
    let mut quiet = herd.start_runner_with(
        "the-real-runner",
        "runner",
        "runner",
        &["--health-every", "10", "--hub-silence-limit", "30"],
    );
    let left = tokio::time::timeout(PATIENCE, quiet.gone())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "a runner the hub cannot keep its promises about must be turned away, not \
                 served: {}",
                herd.said("runner")
            )
        });
    assert!(
        !left.success(),
        "and a refusal is terminal (D13): the process ends rather than dialling again"
    );

    // The refusal names the figures on both sides and the margin they need:
    // this is what an operator has to act on.
    let said = herd.said("runner");
    for figure in ["3s", "10s", "3 health reports", "30s"] {
        assert!(
            said.contains(figure),
            "the runner must be told {figure}: {said}"
        );
    }
    assert!(
        herd.said("first").contains("is refused"),
        "and the hub says on its own stderr which runner it turned away: {}",
        herd.said("first")
    );

    // Nothing was placed on it, so no job was ever put at risk: the
    // submission is refused by name instead of being handed to a runner whose
    // job the hub would declare lost between two of its own reports.
    let output = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "sleep 30"])
        .await;
    assert_eq!(
        output.status.code(),
        Some(125),
        "Herd's own failure to place a job is 125: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no runner is connected"),
        "and it says why: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The same machine, reporting often enough, is welcome: what was refused
    // is a pairing of figures, never a runner.
    let mut often = herd.start_runner_with(
        "the-real-runner",
        "second-runner",
        "runner",
        &["--health-every", "0.5"],
    );
    herd.wait_for_runner().await;
    often.signal(rustix::process::Signal::TERM).await;
    hub.signal(rustix::process::Signal::TERM).await;
}

/// The exact reproduction that found this hole: `--health-every 0` used to be
/// read as "no cadence to judge" and let through every one of the three
/// places that call [`herd_proto::silence_threshold_refusal`].
///
/// `herd-hub --health-every 1 --lost-after 3` is a legitimate pairing on its
/// own (three seconds outlasts three one-second reports) and still starts.
/// `herd-runner --health-every 0 --hub-silence-limit 30` is not: a runner
/// that never reports is not one thirty seconds of silence can be judged
/// against. It used to start anyway, dial this very hub, cross its
/// handshake, and then simply never report — a quiet `sleep 30` burnt both
/// its attempts and ended `lost` in about seven seconds, the identical
/// symptom D53's refusal exists to close. Now it refuses before it ever
/// dials, naming the figure.
///
/// The other half of D44's precedent — a figure is refused by name, not
/// reinterpreted — is that a hub asked to *declare* a cadence of zero for
/// itself is refused exactly the same way a runner *reporting* zero is.
#[tokio::test(flavor = "multi_thread")]
async fn a_health_cadence_of_zero_is_refused_on_both_sides() {
    let herd = Herd::new();

    // Timed and `kill_on_drop`, not a bare blocking `.output()`: the defect
    // this guards is a runner that joins and sits connected rather than
    // refusing, and a test that cannot tell that apart from "still starting"
    // would hang on the very regression it exists to catch, instead of
    // failing it.
    let runner_output = Command::new(binary("herd-runner"))
        .arg("--hub")
        .arg(herd.url())
        .arg("--data-root")
        .arg(herd.runner_root("zero"))
        .arg("--health-every")
        .arg("0")
        .arg("--hub-silence-limit")
        .arg("30")
        .kill_on_drop(true)
        .output();
    let runner = tokio::time::timeout(PATIENCE, runner_output)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "a runner reporting at a cadence of zero must refuse at startup, not join \
                 the hub and sit connected"
            )
        })
        .expect("herd-runner runs");
    assert!(
        !runner.status.success(),
        "a runner that never reports must not start, let alone dial a hub"
    );
    let said = String::from_utf8_lossy(&runner.stderr);
    assert!(
        said.contains("0s"),
        "the refusal must name the figure that is wrong: {said}"
    );

    // The reproduction's own hub: a legitimate pairing on its own terms, so
    // it starts exactly as it did before this fix — the defect was never on
    // this side of this particular pairing, only the runner's.
    let mut hub = herd
        .start_hub_with("first", &["--health-every", "1", "--lost-after", "3"])
        .await;

    // A hub declaring a cadence of zero for itself, on a fresh port since the
    // one above is already bound.
    let declares_zero = Herd::new();
    let bad_hub_output = Command::new(binary("herd-hub"))
        .arg("--listen")
        .arg(declares_zero.addr().to_string())
        .arg("--state-dir")
        .arg(declares_zero.state.path())
        .arg("--health-every")
        .arg("0")
        .kill_on_drop(true)
        .output();
    let bad_hub = tokio::time::timeout(PATIENCE, bad_hub_output)
        .await
        .unwrap_or_else(|_| {
            panic!("a hub declaring a cadence of zero must refuse at startup, not bind its port")
        })
        .expect("herd-hub runs");
    assert!(
        !bad_hub.status.success(),
        "a hub that declares a cadence of zero must not start either"
    );
    let said = String::from_utf8_lossy(&bad_hub.stderr);
    assert!(
        said.contains("0s"),
        "the hub's own refusal must name the figure too: {said}"
    );
    assert!(
        TcpStream::connect(declares_zero.addr()).is_err(),
        "a refused hub must not be listening"
    );

    hub.signal(rustix::process::Signal::TERM).await;
}
