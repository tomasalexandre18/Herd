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

    let holding = herd
        .submit_needing("build", "step", &marks_its_interval("inherited", "4"))
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
    let _hub = herd.start_hub_losing_runners_after("hub", 2.0).await;
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
    tokio::time::sleep(Duration::from_millis(600)).await;
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

/// The derived `-j` reaches the process, and the figure on the record is that
/// same figure (E-22).
///
/// The job prints what it was given, so "recorded" and "passed" are compared
/// against each other rather than both read from the hub.
#[tokio::test(flavor = "multi_thread")]
async fn the_derived_j_is_recorded_and_is_what_the_job_is_given() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_SLOT_AND_AN_IGPU);
    // The owner's PC as her figures have it: 17 GiB usable, 20 threads to
    // WSL. At 1 792 MiB per build thread that is E-22's own `-j 7`.
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--host-memory",
            "17GiB",
            "--host-threads",
            "20",
        ],
    );
    herd.wait_for_runner().await;

    let ran = herd
        .herdctl(&[
            "run",
            "--label",
            "build",
            "--build-thread-mib",
            "1792",
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
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout).trim(),
        "built with -j7",
        "the figure E-22 asks for, in the process's own words"
    );

    // And the same figure on the record, where M10's requirements reading
    // will have to find it (D82).
    let job = std::fs::read_dir(herd.state.path().join("jobs"))
        .expect("jobs")
        .filter_map(Result::ok)
        .map(|entry| {
            entry
                .path()
                .file_name()
                .expect("a job id")
                .to_string_lossy()
                .into_owned()
        })
        .max()
        .expect("the build is the newest job");
    let record = herd.record(&job);
    assert_eq!(record["parallelism"], 7, "{record}");
    assert_eq!(record["build_thread_mib"], 1792, "{record}");
    let why = record["attempts"][0]["why"]
        .as_str()
        .expect("every attempt carries its explanation")
        .to_owned();
    assert!(why.contains("-j 7"), "{why}");
    assert!(why.contains("17408 MiB and 20 threads"), "{why}");
    assert!(why.contains("1792 MiB per build thread"), "{why}");
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
