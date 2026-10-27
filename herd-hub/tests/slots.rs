//! Matching, slots and stickiness against the real binaries (M7).
//!
//! Everything here drives `herd-hub`, `herd-runner` and `herdctl` as
//! processes, because the four musts of this milestone are about what happens
//! between two machines: a slot that is given back when a runner is killed, a
//! job that moves when the machine it was waiting for disappears, a line on a
//! record that an operator greps for. A unit test of the claim step proves the
//! arithmetic (see `herd_hub::matcher`); only this proves that the arithmetic
//! is wired to anything.

mod real_herd;

use std::time::{Duration, Instant};

use real_herd::Herd;

/// A runner that has one build slot and an iGPU nobody may share: the owner's
/// PC, as issue #16 describes it (`build` counted 1, `igpu-vfio` exclusive
/// host-wide).
const ONE_SLOT_AND_AN_IGPU: &str = r#"{"capabilities":[
    {"name":"build","kind":{"type":"counted","slots":1}},
    {"name":"igpu-vfio","kind":{"type":"exclusive"}}
]}"#;

/// Their server: two build slots.
const TWO_BUILD_SLOTS: &str = r#"{"capabilities":[
    {"name":"build","kind":{"type":"counted","slots":2}}
]}"#;

/// A `kvm` that something outside Herd is holding, by a lock file this test
/// creates and removes (E-26). The label exists, so a job that needs it
/// **waits**; that is the half of E-36 that must never read as a failure.
/// `build` also refuses to be held beside `kvm` here, so that the two halves
/// of E-36 can be asserted on one machine: a job asking for both can never
/// run anywhere, and a job asking for `kvm` alone simply waits for the lock
/// to go.
const KVM_BEHIND_A_LOCK: &str = r#"{"capabilities":[
    {"name":"build","kind":{"type":"counted","slots":1},"excludes":["kvm"]},
    {"name":"kvm","kind":{"type":"shared"},
     "busy_probe":{"type":"lock_file","path":"PATH"}}
]}"#;

/// A command that says when it started and when it stopped, in the job's own
/// log, so that two jobs' intervals can be compared afterwards from the
/// records alone.
/// `which` makes the command different from every other job's, because two
/// identical submissions are **one** job (E-05) and a test that forgot that
/// would be comparing one job's interval with itself.
fn marks_its_interval(which: &str, seconds: &str) -> String {
    format!("printf '{which}\\n' >/dev/null; date +%s.%N; sleep {seconds}; date +%s.%N")
}

/// The same, held open until the test creates `gate` instead of for a number
/// of seconds.
///
/// For a test whose point is that something is true *while this job holds its
/// slot*: a job that sleeps for a fixed time is a race between the test and
/// the clock, and the test losing it does not fail — it passes, having
/// proved nothing, because the slot it meant to find occupied was free
/// (D101).
fn marks_its_interval_until(which: &str, gate: &std::path::Path) -> String {
    format!(
        "printf '{which}\\n' >/dev/null; date +%s.%N; \
         while [ ! -e {} ]; do sleep 0.05; done; date +%s.%N",
        gate.display()
    )
}

/// The two moments one of those jobs printed.
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

/// Where the capability file of one runner goes.
fn capability_file(herd: &Herd, which: &str, contents: &str) -> String {
    let path = herd.logs.path().join(format!("{which}-capabilities.json"));
    std::fs::write(&path, contents).expect("the capability file is written");
    path.display().to_string()
}

/// The runner an attempt of this job was handed to, as its record says.
fn ran_on(herd: &Herd, job: &str) -> Option<String> {
    herd.record(job)["attempts"]
        .as_array()?
        .last()?
        .get("runner")?
        .as_str()
        .map(str::to_owned)
}

fn state_of(herd: &Herd, job: &str) -> String {
    herd.record(job)["state"]["type"]
        .as_str()
        .unwrap_or("unreadable")
        .to_owned()
}

/// Every placement line in the whole state root: E-36's `grep`, as an
/// operator would run it.
fn every_claim_line(herd: &Herd) -> Vec<String> {
    let mut lines = Vec::new();
    let jobs = herd.state.path().join("jobs");
    let mut directories: Vec<_> = std::fs::read_dir(&jobs)
        .expect("the state root has a jobs directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    directories.sort();
    for directory in directories {
        let events = std::fs::read_to_string(directory.join("events.jsonl")).unwrap_or_default();
        for line in events.lines() {
            if line.contains("\"claimed by") {
                lines.push(line.to_owned());
            }
        }
    }
    lines
}

/// Two counted jobs serialise on a one-slot runner, and an exclusive job
/// never overlaps either of them (E-22, E-25, D46).
///
/// Read off the jobs' own logs: each prints the moment it started and the
/// moment it stopped, so "they did not overlap" is an assertion about three
/// intervals and not about the hub's opinion of itself.
#[tokio::test(flavor = "multi_thread")]
async fn two_counted_jobs_serialise_and_an_exclusive_one_overlaps_neither() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &["--capabilities", &file, "--no-detect"],
    );
    herd.wait_for_runner().await;

    let first = herd
        .submit_needing("build", "step", &marks_its_interval("first", "0.6"))
        .await;
    let second = herd
        .submit_needing("build", "step", &marks_its_interval("second", "0.6"))
        .await;
    let exclusive = herd
        .submit_needing("igpu-vfio", "step", &marks_its_interval("alone", "0.4"))
        .await;

    for job in [&first, &second, &exclusive] {
        let job = job.clone();
        herd.wait_until(&format!("{job} is over"), || {
            state_of(&herd, &job) == "finished"
        })
        .await;
    }

    let one = interval(&herd, &first);
    let two = interval(&herd, &second);
    let alone = interval(&herd, &exclusive);
    for (name, other) in [("the second build", two), ("the exclusive job", alone)] {
        assert!(
            one.1 <= other.0 || other.1 <= one.0,
            "the first build ran {one:?} and {name} ran {other:?}: they overlap, \
             and one build slot means one build at a time (E-22)"
        );
    }
    assert!(
        two.1 <= alone.0 || alone.1 <= two.0,
        "the second build ran {two:?} and the exclusive job ran {alone:?}: \
         `exclusive` is host-wide and overlaps nothing (D46)"
    );

    // And every one of the three placements reads as one line, with its
    // figures (E-36).
    let lines = every_claim_line(&herd);
    assert!(
        lines.iter().any(|line| line.contains("build slot 1 of 1")),
        "the claim lines were {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("igpu-vfio is host-wide")),
        "the claim lines were {lines:#?}"
    );
}

/// A runner killed with `kill -9` in the middle of a job gives its slot back,
/// and the next job of the same label runs on it (E-21, E-22).
///
/// The leak this is about is silent: a hub that counted slots in memory and
/// forgot to decrement one here would have a machine half the size for the
/// rest of its life, with nothing in any log to say so.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_killed_mid_job_gives_its_slot_back() {
    let herd = Herd::new();
    // Two seconds of silence is lost, so the test does not spend a minute
    // proving a threshold it is not about.
    let _hub = herd.start_hub_losing_runners_after("hub", 2.0).await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let options = ["--capabilities", file.as_str(), "--no-detect"];
    let mut runner = herd.start_runner_with("wsl-pc", "runner", "runner", &options);
    herd.wait_for_runner().await;

    let doomed = herd.submit_needing("build", "step", "sleep 600").await;
    herd.wait_until("the first build is running", || {
        state_of(&herd, &doomed) == "running"
    })
    .await;

    // The whole machine, torn out from under the job.
    runner.signal(rustix::process::Signal::KILL).await;
    let killed_at = Instant::now();

    // A second runner of the same name takes its place, and a second build is
    // submitted. It can only run if the slot the dead one was holding came
    // back.
    let _replacement = herd.start_runner_with("wsl-pc", "replacement", "replacement", &options);
    herd.wait_until("the replacement runner has joined", || {
        herd.said("hub").matches("has joined").count() >= 2
    })
    .await;

    // The slot the dead runner was holding came back if, and only if, the
    // job's second go can be placed at all (E-21): the one slot of this
    // machine is the only one there is.
    herd.wait_until("the torn-out job has a second attempt", || {
        herd.record(&doomed)["attempts"]
            .as_array()
            .is_some_and(|attempts| attempts.len() == 2)
    })
    .await;
    let record = herd.record(&doomed);
    assert_eq!(
        record["attempts"][1]["runner"].as_str(),
        Some("wsl-pc"),
        "the second attempt went to the machine that replaced the dead one, \
         {:?} after the kill: {record}",
        killed_at.elapsed()
    );
    let second_line = record["attempts"][1]["why"]
        .as_str()
        .expect("every placement has its line")
        .to_owned();
    assert!(
        second_line.contains("build slot 1 of 1"),
        "the slot came back as one of one, not as a second one on top of a \
         slot nobody released: {second_line}"
    );

    // And it is taken again, which is the other half: a slot released twice
    // would be a machine that runs two builds. A third build waits, with the
    // figures.
    //
    // Submitted only once the hub has a runner **now**, which is not what
    // "has joined" above proves. This hub loses a runner after two seconds of
    // silence, and on a host where a healthy runner's whole process can be
    // descheduled for that long — a two-core CI runner, or anything running
    // at a low CPU weight — the hub drops it, correctly, and it rejoins. A
    // submission landing in that gap is refused outright, `no runner is
    // connected`, and this test then dies of a fleet that was empty for a
    // moment rather than of anything it is about. Counting the hub's own
    // joins against its own departures is the exact condition; a residual
    // window of one `herdctl` spawn remains, and only the question in the
    // report — whether that refusal should be a wait (D88) — can close it.
    herd.wait_until("a runner is connected to the hub now", || {
        let said = herd.said("hub");
        said.matches("has joined").count() > said.matches(" is gone; an attempt").count()
    })
    .await;
    let waiting = herd
        .submit_needing("build", "step", "printf 'must not start\\n'")
        .await;
    herd.wait_until("the third build has said why it waits", || {
        herd.record(&waiting)["waiting_for"].is_string()
    })
    .await;
    let why = herd.record(&waiting)["waiting_for"]
        .as_str()
        .expect("a sentence")
        .to_owned();
    assert!(
        why.contains("build has 1 slot(s) and 1 are taken"),
        "the slot is held by the job that was given another go: {why}"
    );
}

/// After a hub restart the slots are read from the records, not from memory
/// (E-22, E-44).
///
/// A hub that kept a counter would come back believing every machine idle and
/// would double-book the one slot the runner is still using. The runner here
/// survives the hub, as D17 has it, so the job it is holding is genuinely
/// still running while the new hub decides what to do with the queue.
#[tokio::test(flavor = "multi_thread")]
async fn the_slots_are_rebuilt_from_the_records_after_a_hub_restart() {
    let herd = Herd::new();
    let hub = herd.start_hub("first").await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &["--capabilities", &file, "--no-detect"],
    );
    herd.wait_for_runner().await;

    // Held open by a file rather than by a count of seconds: what this test
    // is about is only true while the inherited job still holds the one
    // slot, and a job that ends on a clock takes that with it.
    let gate = herd.logs.path().join("release-the-inherited-build");
    let holding = herd
        .submit_needing(
            "build",
            "step",
            &marks_its_interval_until("inherited", &gate),
        )
        .await;
    herd.wait_until("the build is running", || {
        state_of(&herd, &holding) == "running"
    })
    .await;

    // The hub dies with the job still out there.
    let mut hub = hub;
    hub.signal(rustix::process::Signal::KILL).await;
    let _second_hub = herd.start_hub("second").await;
    herd.wait_until("the runner has found the hub that came back", || {
        herd.said("second").contains("has joined")
    })
    .await;

    // A second build, submitted to the hub that came back. The one slot is
    // still held by a job the new hub only knows from its records.
    let second = herd
        .submit_needing(
            "build",
            "step",
            &marks_its_interval("after the restart", "0.3"),
        )
        .await;
    // The positive half, and the one the interval comparison below cannot
    // make on its own: the hub that came back **says** the one slot is taken,
    // by a job it only knows from a record. Without this the comparison is
    // satisfied by a second build that ran after the first had ended for
    // reasons of its own, which proves nothing about counting.
    herd.wait_until("the second build has said why it waits", || {
        herd.record(&second)["waiting_for"].is_string()
    })
    .await;
    let why = herd.record(&second)["waiting_for"]
        .as_str()
        .expect("a sentence")
        .to_owned();
    assert!(
        why.contains("build has 1 slot(s) and 1 are taken"),
        "the hub that came back must count the inherited job's slot: {why}"
    );
    assert_eq!(state_of(&herd, &second), "queued", "{why}");

    // Only now may the inherited job end.
    std::fs::write(&gate, b"the test has seen the slot held").expect("the gate is opened");
    for job in [&holding, &second] {
        let job = job.clone();
        herd.wait_until(&format!("{job} is over"), || {
            state_of(&herd, &job) == "finished"
        })
        .await;
    }
    let first = interval(&herd, &holding);
    let next = interval(&herd, &second);
    assert!(
        first.1 <= next.0 || next.1 <= first.0,
        "the job the hub inherited ran {first:?} and the one submitted after \
         the restart ran {next:?}: the restarted hub double-booked the one \
         build slot, which is what reading the slots from the records exists \
         to prevent"
    );
}

/// E-19 in both of its halves, against two real machines: with the eligible
/// runner busy the job stays queued, and with that runner gone it moves —
/// well inside the ten seconds E-19's acceptance criterion names.
#[tokio::test(flavor = "multi_thread")]
async fn a_job_waits_for_the_busy_runner_it_prefers_and_moves_when_it_goes() {
    let herd = Herd::new();
    // Two seconds of silence is lost, and a waiting job is told why again
    // every 200 ms: the second is how this test watches the hub judge the
    // same job several times over without a sleep standing in for it.
    let _hub = herd
        .start_hub_with("hub", &["--lost-after", "2", "--wait-restatement", "0.2"])
        .await;
    let pc_file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let server_file = capability_file(&herd, "server", TWO_BUILD_SLOTS);
    // The PC is the preferred machine for a build: 3.1x the loaded server,
    // which is the fact E-19 exists to honour.
    let mut pc = herd.start_runner_with(
        "wsl-pc",
        "pc",
        "pc",
        &[
            "--capabilities",
            &pc_file,
            "--no-detect",
            "--preference",
            "10",
        ],
    );
    let _server = herd.start_runner_with(
        "build-host",
        "server",
        "server",
        &["--capabilities", &server_file, "--no-detect"],
    );
    herd.wait_until("both runners have joined", || {
        herd.said("hub").matches("has joined").count() >= 2
    })
    .await;

    let first = herd.submit_needing("build", "step", "sleep 600").await;
    herd.wait_until("the first build is running", || {
        state_of(&herd, &first) == "running"
    })
    .await;
    assert_eq!(
        ran_on(&herd, &first).as_deref(),
        Some("wsl-pc"),
        "the preferred machine takes the first build"
    );

    let second = herd
        .submit_needing("build", "step", "printf 'moved\\n'")
        .await;
    // It waits, and it goes on waiting: the server has two free slots and is
    // the wrong machine for it (E-19).
    herd.wait_until("the second build has said why it is waiting", || {
        herd.record(&second)["waiting_for"].is_string()
    })
    .await;
    let waiting = herd.record(&second)["waiting_for"]
        .as_str()
        .expect("a sentence")
        .to_owned();
    assert!(
        waiting.contains("waiting for wsl-pc") && waiting.contains("build-host"),
        "the sentence must name both machines: {waiting}"
    );
    assert!(
        waiting.contains("--stickiness-limit"),
        "and the limit on the wait has to be visible (E-19): {waiting}"
    );
    // Not a sleep: a sleep that ends early proves nothing happened, and a
    // sleep that ends late is a figure nobody can defend. A follower hears
    // the reason restated on the hub's own clock, so three of them is three
    // separate judgements of this job — each one an opportunity to move it
    // to the machine with two free slots, and each one declined (D101).
    let mut following = herd.start_wait(&second);
    let restatements = || {
        std::fs::read_to_string(herd.logs.path().join("wait-err"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("is queued and has not started"))
            .count()
    };
    herd.wait_until(
        "the hub has judged the second build three times over",
        || restatements() >= 3,
    )
    .await;
    following.ask_it_to_stop();
    assert_eq!(
        state_of(&herd, &second),
        "queued",
        "it moved to the server while the machine it prefers was merely busy"
    );

    // Now the PC is gone, which is the one thing that lets it move.
    pc.signal(rustix::process::Signal::KILL).await;
    let killed_at = Instant::now();
    herd.wait_until("the second build has run somewhere else", || {
        state_of(&herd, &second) == "finished"
    })
    .await;
    let moved_in = killed_at.elapsed();
    assert_eq!(ran_on(&herd, &second).as_deref(), Some("build-host"));
    assert!(
        moved_in < Duration::from_secs(10),
        "E-19 asks for the move within ten seconds; it took {moved_in:?}"
    );
}

/// The two halves of E-36 that are easiest to confuse, side by side: an
/// unsatisfiable label set ends the job with the labels named, and a label
/// something outside Herd is holding makes an otherwise identical job wait.
#[tokio::test(flavor = "multi_thread")]
async fn an_impossible_label_set_fails_while_a_held_one_waits() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let lock = herd.logs.path().join("kvm.lock");
    std::fs::write(&lock, b"something outside herd holds this\n").expect("the lock file");
    let file = capability_file(
        &herd,
        "pc",
        &KVM_BEHIND_A_LOCK.replace("PATH", &lock.display().to_string()),
    );
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--probe-interval",
            "100",
        ],
    );
    herd.wait_for_runner().await;

    // Impossible: the runner offers `build` and `kvm`, and its own
    // configuration says a job may not hold `build` and `kvm` together.
    let impossible = herd
        .herdctl(&[
            "run",
            "--label",
            "build,kvm",
            "--priority",
            "step",
            "--",
            "/bin/sh",
            "-c",
            "printf 'this must never run\\n'",
        ])
        .await;
    let said = String::from_utf8_lossy(&impossible.stderr).into_owned();
    assert!(
        said.contains("no runner could ever run this job"),
        "an unsatisfiable job must end, with the reason: {said}"
    );
    assert!(
        said.contains("build") && said.contains("kvm"),
        "and the reason must name the labels: {said}"
    );
    assert_eq!(
        impossible.status.code(),
        Some(125),
        "a job that never ran has no verdict of its own"
    );

    // And the other direction: `kvm` alone is held by something outside Herd,
    // so the job waits and is not failed (E-26, D38).
    let waiting = herd
        .submit_needing("kvm", "step", "printf 'ran at last\\n'")
        .await;
    herd.wait_until("the kvm job has said why it waits", || {
        herd.record(&waiting)["waiting_for"].is_string()
    })
    .await;
    let why = herd.record(&waiting)["waiting_for"]
        .as_str()
        .expect("a sentence")
        .to_owned();
    assert!(
        why.contains("kvm is suspended") && why.contains("not refused"),
        "a held label is a wait, never a failure: {why}"
    );
    assert_eq!(state_of(&herd, &waiting), "queued");

    // The lock goes, the job runs. Nothing was ever failed.
    std::fs::remove_file(&lock).expect("the lock file goes");
    herd.wait_until("the kvm job has run", || {
        state_of(&herd, &waiting) == "finished"
    })
    .await;
    assert!(herd.log(&waiting).contains("ran at last"));
}

/// E-36's own acceptance criterion: a day's placements grep to one line
/// each — no placement unexplained, and none explained twice.
///
/// A "day" here is twelve jobs at four priorities across two machines, with
/// slots contended so that most of them wait first. The waiting sentences are
/// rewritten as the queue moves (M6), which is exactly the interaction that
/// could turn one placement into two lines or none.
#[tokio::test(flavor = "multi_thread")]
async fn every_placement_in_a_busy_day_greps_to_exactly_one_line() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let pc_file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let server_file = capability_file(&herd, "server", TWO_BUILD_SLOTS);
    let _pc = herd.start_runner_with(
        "wsl-pc",
        "pc",
        "pc",
        &[
            "--capabilities",
            &pc_file,
            "--no-detect",
            "--preference",
            "10",
        ],
    );
    let _server = herd.start_runner_with(
        "build-host",
        "server",
        "server",
        &["--capabilities", &server_file, "--no-detect"],
    );
    herd.wait_until("both runners have joined", || {
        herd.said("hub").matches("has joined").count() >= 2
    })
    .await;

    let mut jobs = Vec::new();
    for (at, priority) in ["producer", "step", "merger", "nightly"]
        .into_iter()
        .cycle()
        .take(12)
        .enumerate()
    {
        jobs.push(
            herd.submit_needing("build", priority, &format!("printf 'job {at}\\n'"))
                .await,
        );
    }
    for job in &jobs {
        let job = job.clone();
        herd.wait_until(&format!("{job} is over"), || {
            state_of(&herd, &job) == "finished"
        })
        .await;
    }

    let lines = every_claim_line(&herd);
    let placements: usize = jobs
        .iter()
        .map(|job| {
            herd.record(job)["attempts"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0)
        })
        .sum();
    assert_eq!(placements, jobs.len(), "every job ran once");
    // The `wait_for_runner` jobs of the harness are not in `jobs`, so the
    // count is over the whole state root: every placement of every job,
    // including the ones that only proved a runner had joined.
    let every_attempt: usize = std::fs::read_dir(herd.state.path().join("jobs"))
        .expect("jobs")
        .filter_map(Result::ok)
        .map(|entry| {
            let record = std::fs::read_to_string(entry.path().join("job.json")).unwrap_or_default();
            serde_json::from_str::<serde_json::Value>(&record)
                .ok()
                .and_then(|record| record["attempts"].as_array().map(Vec::len))
                .unwrap_or(0)
        })
        .sum();
    assert_eq!(
        lines.len(),
        every_attempt,
        "one explanatory line per placement, and exactly one (E-36). \
         The lines were:\n{}",
        lines.join("\n")
    );
    for line in &lines {
        assert_eq!(line.matches("claimed by").count(), 1, "{line}");
        assert!(line.contains("preference"), "{line}");
    }
}

/// The derived `-j` reaches the process, the record carries that same figure,
/// and the figure is a **known constant on every host** (E-22).
///
/// The job prints what it was given, so "recorded" and "passed" are compared
/// against each other rather than both read from the hub.
///
/// Two jobs on one runner, because the derivation is the minimum of two
/// branches and a test that drove one of them would still pass with the other
/// deleted:
///
/// | per-thread figure | memory branch | thread branch | derived |
/// |---|---|---|---|
/// | 256 MiB | 1536 / 256 = **6** | 20 | `-j 6` |
/// | 64 MiB  | 1536 / 64 = 24     | **20** | `-j 20` |
///
/// **Why 2 GiB and not the owner's own 17 GiB.** A lend is a figure a runner
/// is *asked* for; what it announces is that figure lowered to what is really
/// there — `MemTotal` (D89) and the room under the tightest cgroup ceiling
/// enclosing it (D119). This test used to declare 17 GiB and guard on
/// `MemTotal` alone, which left the cgroup clamp through unseen — red on a
/// workstation inside a bounded slice — and skipped the assertion outright on
/// every host smaller than the owner's PC, CI included. Red in one place and
/// never executed anywhere else, which by D27 means the figure was proved
/// nowhere (ORCH-069).
///
/// Three quarters of 2 048 MiB is 1 536 MiB on every machine that can build
/// this workspace, so both rows above are constants and both assertions are
/// exact; [`real_herd::a_lend_this_host_can_really_make`] says so by name
/// rather than skipping if a host ever is that tight. E-22's own figure —
/// 17 GiB and 1 792 MiB a thread is `-j 7` — is asserted on the arithmetic
/// itself, with no host in it at all, in `herd_proto::placement`.
#[tokio::test(flavor = "multi_thread")]
async fn the_derived_j_is_recorded_and_is_what_the_job_is_given() {
    real_herd::a_lend_this_host_can_really_make(THE_LEND_MIB, 0);
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--host-memory",
            "2GiB",
            "--host-threads",
            "20",
            // Written out, so the share the `-j` comes out of belongs to this
            // test rather than to a default that could move. The floor is
            // admission's business (E-29) and not the derivation's, but one
            // at or above the lend is a runner that refuses to start.
            "--host-memory-floor",
            "512MiB",
        ],
    );
    herd.wait_for_runner().await;

    // 256 MiB a thread: the share binds, at six of the twenty threads this
    // host says it has.
    the_derived_figure_is(&herd, 256, 6).await;
    // 64 MiB a thread: the share would cover twenty-four, so the thread count
    // is what the figure comes out at. Without the minimum this would be 24,
    // and twenty-four build threads on twenty hardware threads is the
    // oversubscription the derivation exists to avoid.
    the_derived_figure_is(&herd, 64, 20).await;
}

/// How much memory the runner above is told to lend. 2 GiB: small enough that
/// no host running this suite can clamp it, large enough that three quarters
/// of it is a whole number of gibibytes.
const THE_LEND_MIB: u64 = 2 * 1024;

/// Run one build that prints the `-j` it was handed, and insist that the
/// figure the process got, the figure on its record and the figure in the
/// line explaining its placement are one value — and that the value is
/// `expected` (E-22, E-36).
///
/// `expected` is written out by the caller and never computed here: a test
/// that derived its expectation the way the hub derives the figure would pass
/// whatever the hub produced, the wrong answer included.
async fn the_derived_figure_is(herd: &Herd, build_thread_mib: u64, expected: u64) {
    let ran = herd
        .herdctl(&[
            "run",
            "--label",
            "build",
            "--build-thread-mib",
            &build_thread_mib.to_string(),
            "--",
            "/bin/sh",
            "-c",
            "printf 'built with -j{j}\\n'",
        ])
        .await;
    assert!(
        ran.status.success(),
        "the build failed: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    // After E-02's framing line, which is what the requester's stdout begins
    // with: the `-j` the hub derived is in the *job's* own words (D109).
    let said = real_herd::job_output(&ran.stdout).trim().to_owned();
    assert_eq!(
        said,
        format!("built with -j{expected}"),
        "{build_thread_mib} MiB a thread, out of three quarters of the {THE_LEND_MIB} MiB \
         this runner lends, capped by its 20 threads. The hub explained the placement it \
         made, and the figures in that line are the ones to read: {}",
        herd.said("hub")
            .lines()
            .filter(|line| line.contains("is attempt 1 on runner"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // And the same figure on the record, where M10's requirements reading
    // will have to find it (D82). Found by its own per-thread figure rather
    // than by taking the newest directory, so that two builds in one test
    // cannot be read for each other.
    let record = the_build_declaring(herd, build_thread_mib);
    let why = record["attempts"][0]["why"]
        .as_str()
        .expect("every attempt carries its explanation")
        .to_owned();
    assert!(
        why.contains(&format!("{build_thread_mib} MiB per build thread")),
        "{why}"
    );
    // The two figures the derivation was made from, as the runner announced
    // them. A lend the host clamped would show up here, as a memory figure
    // that is not this test's, instead of as an unexplained `-j`.
    assert!(
        why.contains(&format!("{THE_LEND_MIB} MiB and 20 threads")),
        "{why}"
    );
    // **The three figures are one value**: the one written on the record, the
    // one in the line explaining the placement, and the one the process was
    // actually given. Under M7's own defect — a `-j` derived at the moment of
    // handing over, with the record told afterwards — these three could
    // differ, and that is what this is here to catch.
    let derived = record["parallelism"]
        .as_u64()
        .unwrap_or_else(|| panic!("a derived -j is on the record: {record}"));
    assert_eq!(derived, expected, "{record}");
    assert_eq!(said, format!("built with -j{derived}"), "{record}");
    assert!(why.contains(&format!("-j {derived}")), "{why}");
}

/// The record of the one build that declared `build_thread_mib` MiB a thread.
///
/// Exactly one, insisted on: a helper that silently took the first of two
/// would make the assertions above pass by reading the other job's record.
fn the_build_declaring(herd: &Herd, build_thread_mib: u64) -> serde_json::Value {
    let mut found: Vec<serde_json::Value> = std::fs::read_dir(herd.state.path().join("jobs"))
        .expect("jobs")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let record = std::fs::read_to_string(entry.path().join("job.json")).ok()?;
            serde_json::from_str::<serde_json::Value>(&record).ok()
        })
        .filter(|record| record["build_thread_mib"].as_u64() == Some(build_thread_mib))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one build declared {build_thread_mib} MiB a thread, and the records hold {} of them",
        found.len()
    );
    found.remove(0)
}

/// A figure of zero is refused where it is written, not announced as a size
/// (E-22, and the `Some(0)` that caused a real defect in M4).
///
/// The honest absent case — a host whose `/proc/meminfo` cannot be read — is
/// asserted in `herd_hub::matcher`'s own tests, where a fleet can be put in
/// that state; it cannot be reached through the real binaries on a Linux host
/// without lying to one of them. What *can* be reached through them is an
/// operator typing the figure, and that is refused by name at startup rather
/// than becoming a machine no job fits on.
#[tokio::test(flavor = "multi_thread")]
async fn a_host_size_of_zero_is_refused_where_it_is_written() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    let mut refused = herd.start_runner_with(
        "wsl-pc",
        "zero",
        "zero",
        &["--capabilities", &file, "--no-detect", "--host-memory", "0"],
    );
    let ended = refused.gone().await;
    assert!(
        !ended.success(),
        "a runner told its host has no memory must refuse to start"
    );
    let said = herd.said("zero");
    assert!(
        said.contains("not a figure a -j can be derived from"),
        "and say so by name: {said}"
    );
}
