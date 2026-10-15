//! M9 against the real binaries: the owner's switch, a dry run, a deadline, a
//! parent, and the metal lease (E-33, E-07, E-09, E-10, E-27).
//!
//! All five are about what happens *between* processes, so all five are driven
//! as processes: a `herd-hub`, one or two `herd-runner`s and `herdctl`. The
//! arithmetic each of them rests on is a unit test of `herd_hub::matcher`;
//! only this proves the arithmetic is wired to anything — and E-33's ten
//! seconds is a figure that can only be *measured* here.

mod real_herd;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use real_herd::{Herd, PATIENCE};

/// E-33's figure, as this test holds it to.
const TEN_SECONDS: Duration = Duration::from_secs(10);

/// A runner with one build slot, as the owner's PC has (issue #16).
const ONE_BUILD_SLOT: &str = r#"{"capabilities":[
    {"name":"build","kind":{"type":"counted","slots":1}}
]}"#;

/// The owner's PC as E-27 describes it: one build slot, and a `metal-pc`
/// lease that is **declared drain-first in her own file** (D86) and read
/// through a command probe with the consumer's own exit codes — 0 free, 10
/// busy, anything else unreadable (issue #16).
const A_METAL_LEASE_THAT_DRAINS_FIRST: &str = r#"{"capabilities":[
    {"name":"build","kind":{"type":"counted","slots":1}},
    {"name":"metal-pc","kind":{"type":"counted","slots":1},"drain_first":true,
     "busy_probe":{"type":"command","command":["/bin/sh","-c","PROBE"],
                   "busy_exit_code":10,"free_exit_code":0}}
]}"#;

fn capability_file(herd: &Herd, which: &str, contents: &str) -> String {
    let path = herd.logs.path().join(format!("{which}-capabilities.json"));
    std::fs::write(&path, contents).expect("the capability file is written");
    path.display().to_string()
}

/// A lease probe of the shape issue #16 pins: exit 10 with `holder=` and
/// `expires=` on stdout while the lease file is there, exit 0 when it is not.
fn a_lease_probe(herd: &Herd, lease: &std::path::Path) -> String {
    let script = format!(
        "if [ -e {lease} ]; then echo holder=alexa expires=1792000000; exit 10; \
         else exit 0; fi",
        lease = lease.display()
    );
    capability_file(
        herd,
        "pc",
        &A_METAL_LEASE_THAT_DRAINS_FIRST.replace("PROBE", &script),
    )
}

fn state_of(herd: &Herd, job: &str) -> String {
    herd.record(job)["state"]["type"]
        .as_str()
        .unwrap_or("unreadable")
        .to_owned()
}

fn outcome_of(herd: &Herd, job: &str) -> serde_json::Value {
    herd.record(job)["state"]["outcome"].clone()
}

/// How long until a submission is refused because no machine will take it —
/// which is what "the hub stopped claiming" means from the outside (E-33).
async fn until_nothing_is_claimed(herd: &Herd, began: Instant) -> (Duration, String) {
    until_nothing_of_this_kind_is_claimed(herd, began, &[]).await
}

/// The same, for a probe that declares a label.
///
/// Every probe the hub *accepts* becomes a job, and a job the machine under
/// test claims is counted on the line `herdctl switch` prints. A test that
/// then asserts a figure has to be able to say which jobs it put there — so it
/// probes with a label the machine has one slot of, already held by the job
/// the test started. Each accepted probe then queues instead of running, and
/// the only thing ever claimed on that machine is the job the test created
/// (D100).
async fn until_nothing_of_this_kind_is_claimed(
    herd: &Herd,
    began: Instant,
    options: &[&str],
) -> (Duration, String) {
    loop {
        let output = herd.submit_with(options, "printf nothing").await;
        if !output.status.success() {
            return (
                began.elapsed(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            );
        }
        assert!(
            began.elapsed() < TEN_SECONDS,
            "E-33 gives ten seconds and the hub was still claiming after {:?}; it says: {}",
            began.elapsed(),
            herd.switches().await
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A job that runs until this test lets it finish, and then prints `says`.
///
/// Returned with the file that releases it. Everything here that needs work
/// *in flight* while something else is asserted used to hold it there with
/// `sleep 2` or `sleep 3`, which is a guess about how long the assertions
/// take. On a loaded two-core host they take longer, the job ends early, and
/// what is then asserted is a machine with nothing on it: either a figure
/// that is wrong, or a proof that passes while proving nothing (D100). A job
/// held on a file the test creates ends when the test says so and not before,
/// and writes nothing at all until then.
fn a_job_held_until_the_test_says_so(herd: &Herd, which: &str, says: &str) -> (PathBuf, String) {
    let release = herd.logs.path().join(format!("release-{which}"));
    let script = format!(
        "until [ -e {} ]; do sleep 0.05; done; printf {says}",
        release.display()
    );
    (release, script)
}

/// Let one of those jobs finish.
fn let_it_finish(release: &Path) {
    std::fs::write(release, b"go").expect("the test releases the job it was holding");
}

/// The line `herdctl switch` prints about one machine, if it prints one.
fn line_about(said: &str, runner: &str) -> Option<String> {
    said.lines()
        .find(|line| line.starts_with(&format!("{runner}:")))
        .map(str::to_owned)
}

/// The same, insisting there is one.
fn the_line_about(said: &str, runner: &str) -> String {
    line_about(said, runner)
        .unwrap_or_else(|| panic!("`herdctl switch` says nothing about {runner}: {said}"))
}

/// Wait until `herdctl switch` says exactly this about one machine.
///
/// The figures on that line are the runner's own, carried on its health
/// report, so they lag what they describe by up to one cadence. A test that
/// read them without waiting would be asserting a state it had not
/// established (D100).
async fn until_the_fleet_says(herd: &Herd, runner: &str, line: &str) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let said = herd.switches().await;
        if line_about(&said, runner).as_deref() == Some(line) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "`herdctl switch` never said {line:?}; it says: {said}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait until nothing is being written in the hub's state root, and come back
/// with every byte of it.
///
/// For the tests that compare the whole state root before and after a dry run
/// (E-07) and have no single file to watch. A record is more than one file —
/// `job.json` lands before the matching line of `events.jsonl` — so "the job
/// I was waiting for has finished" is not the same as "nothing is in flight",
/// and a `before` taken between the two measures the hub finishing a job
/// rather than the dry run. Two identical readings a cadence apart is a
/// heuristic and is admitted as one; where a test knows what it is waiting
/// for, it waits for that instead (D100).
async fn until_the_state_root_is_quiet(herd: &Herd) -> Vec<(String, Vec<u8>)> {
    let deadline = Instant::now() + PATIENCE;
    let mut last = herd.state_bytes();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = herd.state_bytes();
        if now == last {
            return now;
        }
        assert!(
            Instant::now() < deadline,
            "the hub never stopped writing in its state root"
        );
        last = now;
    }
}

/// Submit until the hub refuses **for this reason**, and come back with what
/// it said.
///
/// What a runner prints as it dials is the runner's own news; a submission
/// means something only once the *hub* has that runner, and nothing in the
/// runner's log says when that is. So the condition waited on here is the
/// refusal itself: refused for her reason is the answer, refused because
/// nothing is connected yet is a wait, and a submission the hub **accepts**
/// fails at once rather than being retried — a machine that claims work after
/// its owner switched it off is exactly the defect these tests watch for
/// (D100).
async fn until_a_submission_is_refused_saying(herd: &Herd, reason: &str) -> String {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let output = herd.submit_with(&[], "printf nothing").await;
        assert!(
            !output.status.success(),
            "the machine claimed work it had to refuse; it says: {}",
            herd.switches().await
        );
        let refusal = String::from_utf8_lossy(&output.stderr).into_owned();
        if refusal.contains(reason) {
            return refusal;
        }
        assert!(
            Instant::now() < deadline,
            "no refusal ever carried {reason:?}; the last one said: {refusal}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ------------------------------------------------------------------ E-33

/// `off`, set on the machine itself, stops the hub claiming inside ten
/// seconds — **while a job is running** — and does not touch that job
/// (E-33, attack 1).
///
/// The count on the line this asserts is a figure the test creates, and it is
/// made so by the fixture rather than by a looser assertion (D100). The
/// machine has **one** build slot and nothing detected; the job in flight is
/// a build, so it holds that slot; and the probes that establish the hub has
/// stopped claiming are builds too, so every one the hub accepts queues
/// instead of being claimed. Before this, the probes were unlabelled, a
/// second one was claimed on a slow host while the switch was still moving,
/// and the hub rightly said `2 jobs running there finish` — a correct
/// sentence that a literal `1 job` could not survive.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_off_on_the_machine_stops_claims_in_ten_seconds_and_spares_the_job() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--switch-poll",
            "100",
        ],
    );
    herd.wait_for_runner().await;

    // Something real is running on the machine when its owner wants it back,
    // and it runs until this test says otherwise rather than for a guessed
    // number of seconds.
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "spared", "done");
    let running = herd.submit_ok(&["--label", "build"], &script).await;
    herd.wait_until("the job has started", || {
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running")
    })
    .await;
    // And the hub has caught up with that before anything is switched: the
    // count it prints is the runner's own figure, carried on a health report,
    // so it is waited for rather than assumed.
    until_the_fleet_says(&herd, "wsl-pc", "wsl-pc: on, 1 job running there finishes").await;

    let began = Instant::now();
    herd.switch_on_the_machine("runner", &["off", "--reason", "she is gaming"])
        .await;
    // **The owner's own path is written first**, and this is where that order
    // is pinned: `herd-runner switch` is the write, and the running runner
    // reads it afterwards. The order through the hub is now the same (D99), and
    // a change that inverted either one would break here rather than on a
    // slower host three pull requests later.
    let on_her_disk = std::fs::read_to_string(herd.switch_file("runner"))
        .expect("`herd-runner switch` writes the file before anything else happens");
    assert!(on_her_disk.contains("she is gaming"), "{on_her_disk}");
    let (took, refusal) =
        until_nothing_of_this_kind_is_claimed(&herd, began, &["--label", "build"]).await;
    assert!(
        took < TEN_SECONDS,
        "E-33 asks for ten seconds; it took {took:?}"
    );
    assert!(
        refusal.contains("wsl-pc is switched off (she is gaming)"),
        "the refusal must carry her own reason (E-33): {refusal}"
    );
    // And the state is visible, with the reason, through herdctl — the whole
    // line and not a fragment of it: the machine's name, the owner's own
    // words, and the **one** job this test put there, which *finishes*. That
    // last word is the whole difference between her switch and a shutdown.
    let said = herd.switches().await;
    assert_eq!(
        the_line_about(&said, "wsl-pc"),
        "wsl-pc: off (she is gaming), 1 job running there finishes",
        "E-33 asks for the position, its reason and what becomes of the work in flight: {said}"
    );

    // The job that was running is **not** killed. That is the whole
    // difference between the owner's switch and a shutdown — and it is still
    // running now, after the hub has stopped claiming, because this test is
    // the only thing that can end it.
    assert!(
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running"),
        "a switch off must leave the job in flight alone (E-33); it is {}",
        state_of(&herd, &running)
    );
    let_it_finish(&release);
    let waited = herd.herdctl(&["wait", &running]).await;
    assert!(
        waited.status.success(),
        "a switch off must not touch the job in flight (E-33): {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert_eq!(
        real_herd::job_output(&waited.stdout).trim(),
        "done",
        "the job ran to its end"
    );
    assert_eq!(state_of(&herd, &running), "finished");
    assert_eq!(outcome_of(&herd, &running)["code"], 0);

    // Switched back on, the machine claims again.
    herd.switch_on_the_machine("runner", &["on"]).await;
    let back = Instant::now();
    loop {
        if herd
            .submit_with(&[], "printf claimed again")
            .await
            .status
            .success()
        {
            break;
        }
        assert!(
            back.elapsed() < TEN_SECONDS,
            "`on` is honoured in ten seconds too; {:?} passed",
            back.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The same switch, moved by an orchestrator through `herdctl` (E-33, D99).
///
/// Both halves of E-33 — her machine and the server — and they leave **one**
/// state: the position a `herdctl` set is written to the runner's own switch
/// file, so a restart keeps it and the owner sees what an agent did.
///
/// **The file is read in the instant `herdctl switch` returns, with no wait of
/// any kind**, because that is the guarantee D99 added: a success from
/// `herdctl switch` means the position is already on the machine's disk. This
/// test used to read the file after waiting for the hub to stop claiming, which
/// passed on a twelve-thread host and failed on a two-core one — and what it
/// was failing on was not the test but the product, which had told its owner
/// success a second and a half before her machine knew anything about it.
/// Polling until the file appears would make this green again and hide exactly
/// that, so nothing here polls.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_off_through_herdctl_stops_claims_in_ten_seconds_and_is_written_down() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner_with("wsl-pc", "runner", "runner", &["--switch-poll", "100"]);
    herd.wait_for_runner().await;

    let began = Instant::now();
    let said = herd
        .herdctl(&["switch", "wsl-pc", "off", "--reason", "a reboot is due"])
        .await;
    assert!(
        said.status.success(),
        "herdctl switch failed: {} {}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    // Not a poll, not a sleep: the next statement after the command returned.
    let written = std::fs::read_to_string(herd.switch_file("runner")).expect(
        "D99: a successful `herdctl switch` means the runner has already written the position \
         down, so this read cannot be allowed to wait for it",
    );
    assert!(written.contains("\"off\""), "{written}");
    assert!(written.contains("a reboot is due"), "{written}");
    // And the command says so, naming where: an owner reading success has to
    // be able to tell which half of E-33 she was promised.
    let answer = String::from_utf8_lossy(&said.stdout);
    assert!(
        answer.contains("written the position down"),
        "the success has to say the position is durable (D99): {answer}"
    );
    assert!(
        answer.contains(&herd.switch_file("runner").display().to_string()),
        "and where it is: {answer}"
    );
    // E-33's ten seconds is still a promise about the hub, and it is still
    // kept — now from a command that no longer returns before the runner has
    // acted.
    let (took, refusal) = until_nothing_is_claimed(&herd, began).await;
    assert!(took < TEN_SECONDS, "it took {took:?}");
    assert!(refusal.contains("a reboot is due"), "{refusal}");

    // A runner nobody is connected as is named rather than silently ignored —
    // and is not success either, because nothing was switched (D99).
    let missing = herd
        .herdctl(&["switch", "nowhere", "off", "--reason", "x"])
        .await;
    assert!(
        !missing.status.success(),
        "a switch nothing applied must not report success"
    );
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("no runner called nowhere"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    // And `off` with no reason is refused before it reaches anything: a
    // machine withdrawn for no stated reason is one nobody can switch back.
    let unreasoned = herd.herdctl(&["switch", "wsl-pc", "off"]).await;
    assert!(!unreasoned.status.success());
    assert!(
        String::from_utf8_lossy(&unreasoned.stderr).contains("needs a reason"),
        "{}",
        String::from_utf8_lossy(&unreasoned.stderr)
    );
}

/// An acknowledgement that does not arrive is a **partial result naming the
/// runner**, never success (E-33, D99).
///
/// The companion to the test above: `herdctl switch` now reports success only
/// when the runner has the position on its disk, so there has to be an honest
/// answer for the case where the runner says nothing. The runner here is paused
/// with `SIGSTOP` — plainly there, answering nothing, which is what a wedged
/// machine or a hung write looks like from the hub — and the ten seconds E-33
/// gives are allowed to run out.
#[tokio::test(flavor = "multi_thread")]
async fn an_acknowledgement_that_times_out_is_a_partial_result_naming_the_runner() {
    let herd = Herd::new();
    // Comfortably longer than the ten seconds this test spends paused, so that
    // what is measured is the acknowledgement and not a lost runner.
    let _hub = herd.start_hub_losing_runners_after("hub", 60.0).await;
    let mut runner =
        herd.start_runner_with("wsl-pc", "runner", "runner", &["--switch-poll", "100"]);
    herd.wait_for_runner().await;

    runner.signal_without_waiting(rustix::process::Signal::STOP);
    let began = Instant::now();
    let said = herd
        .herdctl(&["switch", "wsl-pc", "off", "--reason", "a reboot is due"])
        .await;
    let took = began.elapsed();
    runner.signal_without_waiting(rustix::process::Signal::CONT);

    assert!(
        !said.status.success(),
        "a position nobody confirmed must never be reported as success (D99): {}",
        String::from_utf8_lossy(&said.stdout)
    );
    // Bounded by E-33's own figure: it waits, and it stops waiting.
    assert!(
        took >= TEN_SECONDS && took < TEN_SECONDS * 3,
        "the wait is E-33's ten seconds, and it took {took:?}"
    );
    let partial = String::from_utf8_lossy(&said.stderr);
    assert!(
        partial.contains("wsl-pc"),
        "the partial result has to name the runner (D99): {partial}"
    );
    assert!(
        partial.contains("has not confirmed writing the position down"),
        "and say exactly what is not true: {partial}"
    );
    assert!(
        partial.contains("off (a reboot is due)"),
        "and which position was asked for, with her reason: {partial}"
    );
    // Nothing was printed on stdout, which is where this command's answers go:
    // a caller that reads stdout must not find half a success there.
    assert!(
        String::from_utf8_lossy(&said.stdout).trim().is_empty(),
        "{}",
        String::from_utf8_lossy(&said.stdout)
    );
}

/// A position a runner cannot write down is reported as in force and **not
/// durable**, naming the runner (E-33, D99).
///
/// The other half of the same honesty. A runner that cannot write its switch
/// file still applies the position — the owner asked for her machine back, and
/// a read-only data root is not a reason to go on claiming work — so the hub
/// honours it, nothing new is claimed, and a restart of that runner would
/// nonetheless forget it. That is two true sentences and one of them is bad
/// news, so neither is reported as plain success.
///
/// The switch file here is given a path whose parent is a regular file, which
/// no `mkdir` can make: a deterministic unwritable switch with no permissions
/// games and no root.
#[tokio::test(flavor = "multi_thread")]
async fn a_position_the_runner_cannot_write_is_reported_as_not_durable() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let blocked = herd.logs.path().join("not-a-directory");
    std::fs::write(&blocked, b"a file where a directory would have to be")
        .expect("the fixture writes the obstacle");
    let unwritable = blocked.join("switch");
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--switch-poll",
            "100",
            "--switch-file",
            &unwritable.display().to_string(),
        ],
    );
    herd.wait_for_runner().await;

    let began = Instant::now();
    let said = herd
        .herdctl(&["switch", "wsl-pc", "off", "--reason", "a reboot is due"])
        .await;
    assert!(
        !said.status.success(),
        "a position that would not survive a restart is not success (D99): {}",
        String::from_utf8_lossy(&said.stdout)
    );
    let partial = String::from_utf8_lossy(&said.stderr);
    assert!(
        partial.contains("wsl-pc") && partial.contains("could not write the position down"),
        "the partial result names the runner and what failed (D99): {partial}"
    );
    assert!(
        partial.contains(&blocked.display().to_string()),
        "and the path it could not write: {partial}"
    );
    // And it is not a refusal to obey: the machine really is withdrawn.
    let (took, refusal) = until_nothing_is_claimed(&herd, began).await;
    assert!(took < TEN_SECONDS, "it took {took:?}");
    assert!(refusal.contains("a reboot is due"), "{refusal}");
}

/// A switch set off outlives the runner process (E-33).
///
/// An owner who switched her PC off before rebooting it has not changed her
/// mind. The position is announced in the handshake, so there is no window in
/// which the hub believes a withdrawn machine is `on`.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_restarted_while_switched_off_comes_back_switched_off() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    {
        let _runner =
            herd.start_runner_with("wsl-pc", "first", "runner", &["--switch-poll", "100"]);
        herd.wait_for_runner().await;
    }
    herd.switch_on_the_machine("runner", &["off", "--reason", "she is gaming"])
        .await;
    let _again = herd.start_runner_with("wsl-pc", "second", "runner", &["--switch-poll", "100"]);
    herd.wait_until("the runner has rejoined switched off", || {
        herd.said("second").contains("switch says off")
    })
    .await;
    // That line is the runner's own: it says the machine read its file, not
    // that the hub has the connection yet. So the refusal is what is waited
    // on, and a submission the hub accepts fails at once (D100).
    let refusal = until_a_submission_is_refused_saying(&herd, "she is gaming").await;
    assert!(
        refusal.contains("wsl-pc"),
        "and the refusal names the machine: {refusal}"
    );
}

/// A drain is a drain: nothing new, what is held finishes, then it leaves
/// (E-33, invariant 2, attack 2).
///
/// The job in flight is **waited for**, not killed — which is what makes this
/// different from the shutdown drain a `SIGTERM` starts.
#[tokio::test(flavor = "multi_thread")]
async fn a_drain_finishes_its_job_and_then_the_runner_leaves() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let mut runner =
        herd.start_runner_with("wsl-pc", "runner", "runner", &["--switch-poll", "100"]);
    herd.wait_for_runner().await;

    let (release, script) = a_job_held_until_the_test_says_so(&herd, "drained", "finished");
    let running = herd.submit_ok(&[], &script).await;
    herd.wait_until("the job has started", || {
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running")
    })
    .await;
    herd.switch_on_the_machine("runner", &["drain", "--reason", "a reboot is due"])
        .await;

    // Nothing new, at once.
    let began = Instant::now();
    let (took, refusal) = until_nothing_is_claimed(&herd, began).await;
    assert!(took < TEN_SECONDS, "{took:?}");
    assert!(refusal.contains("a reboot is due"), "{refusal}");

    // The runner is **still here**, because it still holds the job — and the
    // job is still running, because nothing but this test can end it. Held
    // there by a `sleep 2`, both of those were a guess: a job that had quietly
    // ended while the assertions above ran would leave this test asserting a
    // drain with nothing to drain (D100).
    assert!(
        runner.alive(),
        "a drain waits for the job it holds (invariant 2): {}",
        herd.said("runner")
    );
    assert!(
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running"),
        "a drain must not kill the job it holds; it is {}",
        state_of(&herd, &running)
    );

    // What it held ran to its end, and only then did the runner go.
    let_it_finish(&release);
    let waited = herd.herdctl(&["wait", &running]).await;
    assert!(
        waited.status.success(),
        "a drain must finish what it holds, never kill it: {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert_eq!(real_herd::job_output(&waited.stdout).trim(), "finished");
    let gone = tokio::time::timeout(Duration::from_secs(20), runner.gone())
        .await
        .expect("a draining runner leaves once it holds nothing");
    assert!(gone.success(), "it leaves quietly: {gone:?}");
    assert!(
        herd.said("runner").contains("holds nothing, so it leaves"),
        "the runner says why it left: {}",
        herd.said("runner")
    );
}

/// A job assigned in the instant the switch moves is handed back, not run
/// (E-33, attack 2).
///
/// The hub sends an assignment; the switch moves before the runner takes the
/// job in hand. The runner abandons the attempt, naming the switch, and the
/// hub places the job on the other machine — nothing is lost and the machine
/// is given back.
#[tokio::test(flavor = "multi_thread")]
async fn a_job_assigned_as_the_switch_moves_is_handed_back_naming_the_switch() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
    let _pc = herd.start_runner_with(
        "wsl-pc",
        "pc",
        "pc",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--preference",
            "10",
            "--switch-poll",
            "100",
        ],
    );
    let server_file = capability_file(&herd, "server", ONE_BUILD_SLOT);
    let _server = herd.start_runner_with(
        "build-host",
        "server",
        "server",
        &["--capabilities", &server_file, "--no-detect"],
    );
    // Both of them as the **hub** has them, not as their own logs say they are
    // dialling: a build submitted while the server was still joining was
    // refused, and this test then read a job id that had never been minted
    // (D100).
    until_the_fleet_says(&herd, "wsl-pc", "wsl-pc: on, nothing running").await;
    until_the_fleet_says(&herd, "build-host", "build-host: on, nothing running").await;
    // The PC is preferred, so a build belongs there (E-19). Switched off, the
    // same build goes to the server instead — at once, because a withdrawn
    // machine is not available and nothing waits for it (D80).
    herd.switch_on_the_machine("pc", &["off", "--reason", "she is gaming"])
        .await;
    // Waited for on the hub's own view rather than slept off. The 400 ms that
    // stood here was a guess about how long a loaded host takes to carry the
    // position from her disk to the hub, and a build submitted a moment too
    // early would be placed on the machine she had just withdrawn — which
    // this test would then report as the product placing it wrongly (D100).
    until_the_fleet_says(
        &herd,
        "wsl-pc",
        "wsl-pc: off (she is gaming), nothing running",
    )
    .await;
    // `submit_ok`, so a refusal is a failure here and says so: read through
    // `submit_with`, a refused submission gave an empty id and the next line
    // panicked trying to read the record of a job that was never minted.
    let build = herd.submit_ok(&["--label", "build"], "printf built").await;
    herd.wait_until("the build is over", || {
        state_of(&herd, &build) == "finished"
    })
    .await;
    let ran_on = herd.record(&build)["attempts"][0]["runner"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        ran_on, "build-host",
        "a job must not wait for a machine its owner has withdrawn (D80, E-33)"
    );
}

// ------------------------------------------------------------------ E-07

/// A dry run says where a job would run and changes **nothing** (E-07,
/// attack 3).
///
/// Proved by taking every byte of the state root before and after: no record,
/// no event, no id, no reservation. And then proved not to be lying, by
/// submitting the same job for real and comparing the two answers (attack 4).
#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_changes_nothing_and_says_what_the_real_pass_would_say() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &["--capabilities", &file, "--no-detect"],
    );
    herd.wait_for_runner().await;
    // One build is already running, so the slot is taken and the dry run has
    // something real to be right about.
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "holding", "held");
    let holding = herd.submit_ok(&["--label", "build"], &script).await;
    // Waited all the way to `running`, and the job writes nothing until it
    // ends: the comparison below has to be against a state root that is
    // quiescent, or it would be measuring the job rather than the dry run.
    // **Which is why it ends when this test says so.** Held there by a
    // `sleep 3`, the job finished mid-comparison on a loaded two-core host and
    // the bytes it wrote were read as a dry run writing to the state root —
    // reproduced, and the reason this is a file and not a duration (D100).
    //
    // And waited for in **both** files of the record, not only in `job.json`:
    // the `running` line of `events.jsonl` lands after the record says
    // `running`, and `state_bytes` reads both. That window is what actually
    // reddened CI here, and it is microseconds wide on an idle host and wide
    // enough to step in on a loaded two-core one.
    herd.wait_until(
        "the first build is running, in its record and in its events",
        || {
            state_of(&herd, &holding) == "running"
                && herd.states_of(&holding).last().map(String::as_str) == Some("running")
        },
    )
    .await;

    let before = until_the_state_root_is_quiet(&herd).await;
    let said = herd
        .herdctl(&[
            "would-run",
            "--label",
            "build",
            "--",
            "/bin/sh",
            "-c",
            "printf hi",
        ])
        .await;
    assert!(
        said.status.success(),
        "the dry run failed: {}",
        String::from_utf8_lossy(&said.stderr)
    );
    let after = herd.state_bytes();
    assert_eq!(
        before.len(),
        after.len(),
        "a dry run must write nothing: {} paths before, {} after",
        before.len(),
        after.len()
    );
    if before != after {
        for ((path, was), (also, now)) in before.iter().zip(after.iter()) {
            assert_eq!(path, also, "a dry run must not move a file (E-07)");
            assert_eq!(
                String::from_utf8_lossy(was),
                String::from_utf8_lossy(now),
                "a dry run must not write to {path} (E-07)"
            );
        }
        panic!("a dry run must leave the state root byte for byte as it found it (E-07)");
    }

    let lines = String::from_utf8_lossy(&said.stdout).into_owned();
    assert!(
        lines.contains("would run on: wsl-pc (build)"),
        "E-07 asks for the eligible runners and their labels: {lines}"
    );
    assert!(
        lines.contains("queued 1 of 1"),
        "E-07 asks for what is queued ahead: {lines}"
    );
    assert!(
        lines.contains("it would wait: "),
        "the slot is taken, so it would wait: {lines}"
    );
    assert!(
        lines.contains("no job id was minted"),
        "the answer says so in so many words: {lines}"
    );
    // **Not lying.** The real submission gets the same verdict, in the same
    // words, out of the same function: it waits, for the same reason.
    let real = herd.submit_ok(&["--label", "build"], "printf hi").await;
    herd.wait_until("the real job has been judged", || {
        !herd.waiting_for(&real).is_empty()
    })
    .await;
    let waiting = herd.waiting_for(&real);
    assert!(
        waiting.contains("build slot") || waiting.contains("waiting for wsl-pc"),
        "the real pass waits for the same reason the dry run gave: {waiting} against {lines}"
    );
    let_it_finish(&release);
}

/// A dry run of a job nothing could take says so, and still changes nothing
/// (E-07).
#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_of_an_unplaceable_job_says_so_and_still_writes_nothing() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &["--capabilities", &file, "--no-detect"],
    );
    herd.wait_for_runner().await;
    // The job `wait_for_runner` ran is over, which is not the same as its
    // record being finished being written: the same two-file window as above,
    // and here there is no id to watch, so the root is waited on until it
    // stops changing (D100).
    let before = until_the_state_root_is_quiet(&herd).await;
    let said = herd
        .herdctl(&[
            "would-run",
            "--label",
            "metal-dell",
            "--",
            "/bin/sh",
            "-c",
            "printf hi",
        ])
        .await;
    let lines = String::from_utf8_lossy(&said.stdout).into_owned();
    assert!(
        lines.contains("would run on: nothing connected to this hub could run it"),
        "{lines}"
    );
    assert_eq!(before, herd.state_bytes(), "still nothing written (E-07)");

    // And a dry run naming a parent that does not exist is answered rather
    // than refused silently: the same rule a submission is held to.
    let said = herd
        .herdctl(&["would-run", "--after", "j0000000099", "--", "/bin/true"])
        .await;
    let lines = String::from_utf8_lossy(&said.stdout).into_owned();
    assert!(
        lines.contains("it would be refused at submission"),
        "{lines}"
    );
    assert_eq!(before, herd.state_bytes());
}

// ------------------------------------------------------------------ E-09

/// A queued job past its deadline ends `cancelled (deadline)`; a job that has
/// **started** is never touched by one (E-09, attack 5).
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_cancels_a_queued_job_and_never_a_running_one() {
    let herd = Herd::new();
    // An aging step of four seconds, which is a re-judging pass a second: a
    // deadline is judged on a dispatch pass, and the passes that are not on a
    // clock are provoked by something happening. The job holding the slot here
    // is held until this test releases it, so nothing happens — and read
    // against the default step, a quarter of which is clamped to a minute,
    // the deadline fired long after the thirty seconds a test waits. Before,
    // the pass came from the `sleep 3` job ending, which made the cancellation
    // a side effect of a duration nobody chose (D100).
    let _hub = herd
        .start_hub_with("hub", &["--lost-after", "60", "--aging-step", "4"])
        .await;
    let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &["--capabilities", &file, "--no-detect"],
    );
    herd.wait_for_runner().await;

    // A job that starts at once, with a deadline that passes **while it
    // runs**. Its deadline must do nothing to it: a running job is the time
    // cap's business (M4, exit 124), never E-09's.
    //
    // Five seconds rather than one, and held on a file rather than a `sleep`.
    // A deadline that has to pass *after* the job started gives the hub until
    // then to start it, and on a loaded two-core host a dispatch pass can be
    // more than a second behind the submission: this job was then cancelled
    // before it ever ran, and the test sat waiting thirty seconds for a slot
    // nothing would ever take (D100).
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "survives", "survived");
    let running = herd
        .submit_ok(&["--label", "build", "--deadline", "+5s"], &script)
        .await;
    herd.wait_until("the long job holds the slot", || {
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running")
    })
    .await;
    // And a second job of the same label, which therefore waits — and whose
    // deadline passes while it waits. It is **judged** first: the sentence
    // asserted below is the one a job waiting for a named thing is cancelled
    // with, and a deadline that fires on the very first pass to touch a job
    // says instead, just as truly, that nothing had been recorded about why it
    // was waiting. Asserting one of those while racing the other is how this
    // reddened CI (D100).
    let queued = herd
        .submit_ok(&["--label", "build", "--deadline", "+5s"], "printf never")
        .await;
    herd.wait_until(
        "the queued job has been judged, before its deadline",
        || !herd.waiting_for(&queued).is_empty(),
    )
    .await;
    herd.wait_until("the queued job is cancelled by its deadline", || {
        state_of(&herd, &queued) == "finished"
    })
    .await;
    let outcome = outcome_of(&herd, &queued);
    assert_eq!(outcome["type"], "cancelled", "{outcome}");
    let reason = outcome["reason"].as_str().unwrap_or_default();
    assert!(
        reason.starts_with("cancelled (deadline)"),
        "E-09's own words: {reason}"
    );
    assert!(
        reason.contains("When the moment came it was"),
        "a job cancelled by a deadline must say what it was waiting for (attack 11): {reason}"
    );
    // D40: cancelled is an outcome and exits 125, in words rather than a code
    // nobody can read.
    let waited = herd.herdctl(&["wait", &queued]).await;
    assert_eq!(waited.status.code(), Some(125));
    assert!(
        String::from_utf8_lossy(&waited.stderr).contains("cancelled (deadline)"),
        "{}",
        String::from_utf8_lossy(&waited.stderr)
    );

    // The one that had started is **still running**, and its own deadline has
    // certainly passed: the queued job was given the same span a moment later,
    // so a moment later is when its deadline fell, and it has already fired.
    assert!(
        matches!(state_of(&herd, &running).as_str(), "assigned" | "running"),
        "a deadline must never touch a job that has started (E-09); it is {}",
        state_of(&herd, &running)
    );
    // And it then ran to its end, untouched.
    let_it_finish(&release);
    let waited = herd.herdctl(&["wait", &running]).await;
    assert!(
        waited.status.success(),
        "a deadline must never kill a job that has started (E-09): {}",
        String::from_utf8_lossy(&waited.stderr)
    );
    assert_eq!(real_herd::job_output(&waited.stdout).trim(), "survived");
}

/// A deadline already past when the job is submitted fires on the first
/// pass (E-09, attack 6).
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_already_past_at_submission_fires_at_once() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;
    let job = herd
        .submit_ok(&["--deadline", "1970-01-01T00:00:00Z"], "printf never")
        .await;
    herd.wait_until("it is cancelled", || state_of(&herd, &job) == "finished")
        .await;
    let outcome = outcome_of(&herd, &job);
    assert_eq!(outcome["type"], "cancelled", "{outcome}");
    assert!(
        outcome["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("cancelled (deadline)"),
        "{outcome}"
    );
    assert_eq!(herd.log(&job), "", "it never ran");
}

/// A hub restarted after a deadline passed fires it on its first pass
/// (E-09, attack 6, attack 12).
///
/// The deadline is a field on the record, judged against the clock, so there
/// is nothing for a restart to rebuild and nothing for it to forget.
#[tokio::test(flavor = "multi_thread")]
async fn a_hub_restarted_after_a_deadline_passed_cancels_the_job() {
    let herd = Herd::new();
    let job;
    let submitted;
    {
        let _hub = herd.start_hub("first").await;
        let file = capability_file(&herd, "pc", ONE_BUILD_SLOT);
        let _runner = herd.start_runner_with(
            "wsl-pc",
            "runner",
            "runner",
            &["--capabilities", &file, "--no-detect"],
        );
        herd.wait_for_runner().await;
        // Nothing offers `metal-dell`, so this waits — and its deadline
        // passes while the hub is down.
        //
        // Five seconds rather than one. The deadline must not fall before the
        // first hub has judged the job, and the pass that judges a submission
        // is not synchronous with the answer: on a loaded two-core host it ran
        // more than a second late, the hub about to be killed cancelled the
        // job itself, and this test then waited thirty seconds for a reason
        // that would never be written — while what it is here to prove, that
        // the *restarted* hub fires the deadline, had not been tried at all
        // (D100).
        submitted = Instant::now();
        job = herd
            .submit_ok(
                &["--label", "metal-dell", "--deadline", "+5s"],
                "printf never",
            )
            .await;
        herd.wait_until("the job is queued and waiting", || {
            !herd.waiting_for(&job).is_empty()
        })
        .await;
    }
    // Down until the deadline has certainly gone by, counted from the
    // submission the deadline was measured against rather than guessed at.
    while submitted.elapsed() < Duration::from_secs(6) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _hub = herd.start_hub("second").await;
    let _runner = herd.start_runner_called("wsl-pc", "again");
    // Waited for, so that the pass the restarted hub makes when a runner joins
    // has certainly happened: the point of this test is the deadline, not how
    // quickly a runner dials.
    herd.wait_for_runner().await;
    herd.wait_until("the restarted hub cancels it", || {
        state_of(&herd, &job) == "finished"
    })
    .await;
    assert_eq!(outcome_of(&herd, &job)["type"], "cancelled");
}

// ------------------------------------------------------------------ E-10

/// A dependent starts after its parent succeeds, and is cancelled by name
/// when its parent does not (E-10, attack 7).
#[tokio::test(flavor = "multi_thread")]
async fn a_dependent_waits_for_its_parent_and_is_cancelled_when_it_fails() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    // A parent that takes its time, and a child that must not start first.
    // The parent is held on a file rather than a `sleep 2`: a parent that had
    // ended before the child was judged would leave the wait below looking for
    // a reason the hub had no cause to record, and thirty seconds later the
    // test would fail having proved nothing about E-10 (D100).
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "parent", "parent");
    let parent = herd.submit_ok(&[], &script).await;
    let child = herd.submit_ok(&["--after", &parent], "printf child").await;
    herd.wait_until("the child has been judged", || {
        herd.waiting_for(&child).contains(&parent)
    })
    .await;
    let waiting = herd.waiting_for(&child);
    assert!(
        waiting.contains(&format!("it starts after {parent}")),
        "{waiting}"
    );
    assert_eq!(
        herd.log(&child),
        "",
        "a child must not run before its parent (E-10)"
    );
    // The parent is still in flight as that is read, because nothing but this
    // test can end it.
    assert!(
        matches!(state_of(&herd, &parent).as_str(), "assigned" | "running"),
        "the parent has to still be in flight for the line above to prove anything; it is {}",
        state_of(&herd, &parent)
    );
    let_it_finish(&release);
    herd.wait_until("the child has run", || {
        state_of(&herd, &child) == "finished"
    })
    .await;
    assert_eq!(outcome_of(&herd, &child)["code"], 0);
    // The parent really did finish first, which is the only thing E-10 asks.
    assert_eq!(state_of(&herd, &parent), "finished");

    // A parent that fails cancels its child **by name**, saying how.
    let failing = herd.submit_ok(&[], "exit 3").await;
    herd.wait_until("the parent failed", || {
        state_of(&herd, &failing) == "finished"
    })
    .await;
    let orphan = herd.submit_ok(&["--after", &failing], "printf never").await;
    herd.wait_until("the child is cancelled", || {
        state_of(&herd, &orphan) == "finished"
    })
    .await;
    let outcome = outcome_of(&herd, &orphan);
    assert_eq!(outcome["type"], "cancelled", "{outcome}");
    let reason = outcome["reason"].as_str().unwrap_or_default();
    assert!(reason.starts_with("cancelled (parent)"), "{reason}");
    assert!(reason.contains(&failing), "it names the parent: {reason}");
    assert!(
        reason.contains("exited 3 rather than 0"),
        "and says how the parent went: {reason}"
    );
    assert_eq!(herd.log(&orphan), "", "it never ran");
}

/// A parent that is cancelled, or lost, is not a parent that succeeded
/// (E-10, attack 7).
#[tokio::test(flavor = "multi_thread")]
async fn only_a_parent_that_exited_zero_counts_as_success() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    // Cancelled by its requester.
    let parent = herd.submit_ok(&[], "sleep 30").await;
    herd.wait_until("it is running", || {
        matches!(state_of(&herd, &parent).as_str(), "assigned" | "running")
    })
    .await;
    let child = herd.submit_ok(&["--after", &parent], "printf never").await;
    herd.herdctl(&["cancel", &parent]).await;
    herd.wait_until("the child is cancelled with it", || {
        state_of(&herd, &child) == "finished"
    })
    .await;
    let reason = outcome_of(&herd, &child)["reason"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(reason.contains("was cancelled"), "{reason}");

    // And a parent that could never run.
    let impossible = herd
        .submit_ok(&["--build-thread-mib", "1792"], "cargo xtask pack")
        .await;
    herd.wait_until("the parent is refused", || {
        state_of(&herd, &impossible) == "finished"
    })
    .await;
    let child = herd
        .submit_ok(&["--after", &impossible], "printf never")
        .await;
    herd.wait_until("its child is cancelled", || {
        state_of(&herd, &child) == "finished"
    })
    .await;
    let reason = outcome_of(&herd, &child)["reason"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(reason.contains("could never have run"), "{reason}");
}

/// A job cannot name a parent that does not exist — which is also why no job
/// can name **itself** and no two jobs can name each other (E-10, attack 7).
#[tokio::test(flavor = "multi_thread")]
async fn a_parent_must_already_exist_so_no_cycle_can_be_written() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let _runner = herd.start_runner();
    herd.wait_for_runner().await;

    // A parent that is not there.
    let refused = herd
        .submit_with(&["--after", "j0000000099"], "printf never")
        .await;
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(said.contains("j0000000099"), "{said}");
    assert!(
        said.contains("only ever depend on one older than itself"),
        "the refusal says why a cycle cannot be written: {said}"
    );

    // A job naming the id it is about to be given: the same refusal, which is
    // what makes self-reference unrepresentable rather than merely checked.
    let next = herd.submit_ok(&[], "printf first").await;
    let guessed = format!(
        "j{:010}",
        next.trim_start_matches('j').parse::<u64>().unwrap_or(0) + 1
    );
    let refused = herd
        .submit_with(&["--after", &guessed], "printf itself")
        .await;
    assert!(
        !refused.status.success(),
        "a job must not be able to name the id it is about to be given"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(&guessed),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

// ------------------------------------------------------------------ E-27

/// A `metal-*` job whose lease is held **waits** and names the holder; it
/// runs when the lease clears (E-27, D76, attack 8).
#[tokio::test(flavor = "multi_thread")]
async fn a_leased_metal_job_waits_and_runs_when_the_lease_clears() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let lease = herd.logs.path().join("metal-pc.lease");
    std::fs::write(&lease, b"holder=alexa\n").expect("the lease is held");
    let file = a_lease_probe(&herd, &lease);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--probe-interval",
            "150",
        ],
    );
    herd.wait_until("the runner has read its lease", || {
        herd.said("runner").contains("metal-pc is suspended")
    })
    .await;

    let trial = herd
        .submit_ok(&["--label", "metal-pc"], "printf trial")
        .await;
    herd.wait_until("the trial is waiting on the lease the probe read", || {
        herd.waiting_for(&trial).contains("exited 10")
    })
    .await;
    let waiting = herd.waiting_for(&trial);
    assert!(
        waiting.contains("exited 10, which means busy"),
        "a job whose machine is leased waits rather than failing (E-27, D76): {waiting}"
    );
    assert_eq!(herd.log(&trial), "", "it has not run");

    // The lease clears, and the job runs — with nothing having failed.
    std::fs::remove_file(&lease).expect("the lease is released");
    herd.wait_until("the trial has run", || {
        state_of(&herd, &trial) == "finished"
    })
    .await;
    assert_eq!(outcome_of(&herd, &trial)["code"], 0);
    assert_eq!(herd.log(&trial), "trial");
}

/// A `metal-*` job empties its host's build slot before it starts, and the
/// host claims nothing new while it waits (E-27, attack 10).
#[tokio::test(flavor = "multi_thread")]
async fn a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let lease = herd.logs.path().join("metal-pc.lease");
    let file = a_lease_probe(&herd, &lease);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--probe-interval",
            "150",
        ],
    );
    herd.wait_for_runner().await;

    // A build is running on the machine, and the trial arrives behind it.
    // Held on a file rather than a `sleep 2`: the drain-first reason this
    // asserts only exists while the build is still there, and a build that had
    // ended first would leave both waits below hunting a sentence the hub had
    // no cause to write (D100).
    let (release, script) = a_job_held_until_the_test_says_so(&herd, "first-build", "first-build");
    let build = herd.submit_ok(&["--label", "build"], &script).await;
    herd.wait_until("the build holds the slot", || {
        matches!(state_of(&herd, &build).as_str(), "assigned" | "running")
    })
    .await;
    let trial = herd
        .submit_ok(&["--label", "metal-pc"], "printf trial")
        .await;
    herd.wait_until("the trial is draining the host", || {
        herd.waiting_for(&trial).contains("being emptied")
    })
    .await;
    let waiting = herd.waiting_for(&trial);
    assert!(
        waiting.contains("starts only on a quiet host"),
        "E-27's drain-first, with its figures: {waiting}"
    );
    assert!(waiting.contains("15m drain-first limit"), "{waiting}");

    // **A build submitted during the drain is not claimed.** This is the
    // whole of the drain: without it, builds that kept arriving would starve
    // the trial for ever.
    let second_build = herd
        .submit_ok(&["--label", "build"], "printf second-build")
        .await;
    herd.wait_until("the second build has been judged", || {
        !herd.waiting_for(&second_build).is_empty()
    })
    .await;
    let held_off = herd.waiting_for(&second_build);
    assert!(
        held_off.contains(&format!("being emptied for {trial}")),
        "a host being emptied for a metal job must claim nothing new (E-27): {held_off}"
    );

    // The trial runs once the host is quiet — which is now, because this test
    // lets the build it was emptying for finish — and the second build follows
    // it.
    let_it_finish(&release);
    herd.wait_until("the trial has run", || {
        state_of(&herd, &trial) == "finished"
    })
    .await;
    assert_eq!(herd.log(&trial), "trial");
    herd.wait_until("the second build has run", || {
        state_of(&herd, &second_build) == "finished"
    })
    .await;
    // Neither build overlapped the trial: the first ended before the trial's
    // attempt began, and the second began after it ended.
    assert_eq!(state_of(&herd, &build), "finished");
    assert_eq!(herd.log(&build), "first-build");
    assert_eq!(herd.log(&second_build), "second-build");
}

/// A declared requeue code is retried on its own budget, and then ends naming
/// the code and the machines that answered it (D76, attack 9).
///
/// Three goes, which is **more than E-21's two**: the proof that a leased
/// machine does not spend the budget that exists for a runner that
/// disappears.
#[tokio::test(flavor = "multi_thread")]
async fn a_declared_requeue_code_is_bounded_and_spends_no_lost_runner_budget() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let lease = herd.logs.path().join("metal-pc.lease");
    let file = a_lease_probe(&herd, &lease);
    let _runner = herd.start_runner_with(
        "wsl-pc",
        "runner",
        "runner",
        &[
            "--capabilities",
            &file,
            "--no-detect",
            "--probe-interval",
            "150",
        ],
    );
    herd.wait_for_runner().await;

    // The probe says the lease is free; the command finds it taken and exits
    // the code it declares for exactly that. This is D76's race.
    let trial = herd
        .submit_ok(
            &["--label", "metal-pc", "--requeue-exit-code", "75"],
            "printf 'lease gone\\n'; exit 75",
        )
        .await;
    herd.wait_until("it has spent its budget", || {
        state_of(&herd, &trial) == "finished"
    })
    .await;
    let outcome = outcome_of(&herd, &trial);
    assert_eq!(
        outcome["type"], "lost",
        "a spent requeue budget is not a verdict of 75 (D76): {outcome}"
    );
    let reason = outcome["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("came back 75"),
        "it names the code: {reason}"
    );
    assert!(
        reason.contains("3 of 3 goes spent"),
        "and the budget, with its figures: {reason}"
    );
    assert!(
        reason.contains("wsl-pc"),
        "and the machines that answered it: {reason}"
    );

    let record = herd.record(&trial);
    let attempts = record["attempts"].as_array().expect("attempts").clone();
    assert_eq!(
        attempts.len(),
        3,
        "three goes at the capability, which is more than E-21's two attempts — the proof \
         that the two budgets are separate (D76): {attempts:#?}"
    );
    for attempt in &attempts {
        assert_eq!(
            attempt["requeue_code"], 75,
            "every one of these ended on the declared code, and the record says so, which is \
             what keeps the budgets apart: {attempt}"
        );
    }
    // The capability was read again each time, rather than the budget being
    // spent against one stale reading.
    let said = herd.said("runner");
    assert!(
        said.contains("reading every probe again now"),
        "D76 asks for the capability to be re-probed: {said}"
    );
    // And the requester is told in words, with 125 rather than 75: a 75 that
    // reached a requester as a verdict is the defect D76 exists to prevent.
    let waited = herd.herdctl(&["wait", &trial]).await;
    assert_eq!(waited.status.code(), Some(125));
}

/// Nothing M9 adds can leak, because nothing M9 adds is counted (D79,
/// attack 12).
///
/// A host held empty for a metal trial, and then a `kill -9` on each side in
/// turn. The reservation is **derived from the records every pass**, so there
/// is nothing for a restart to rebuild and nothing for a crash to leave
/// behind: after the hub is killed and comes back the host is still being
/// emptied for the same job, and after the runner is killed the slot the dead
/// build held is gone and the trial runs.
#[tokio::test(flavor = "multi_thread")]
async fn a_drain_first_reservation_survives_a_kill_nine_on_either_side() {
    let herd = Herd::new();
    let lease = herd.logs.path().join("metal-pc.lease");
    let file = a_lease_probe(&herd, &lease);
    let options = [
        "--capabilities",
        file.as_str(),
        "--no-detect",
        "--probe-interval",
        "150",
    ];
    let trial;
    let held_off;
    let mut runner;
    {
        let _hub = herd.start_hub_losing_runners_after("first", 2.0).await;
        runner = herd.start_runner_with("wsl-pc", "runner", "runner", &options);
        herd.wait_for_runner().await;
        let build = herd
            .submit_ok(&["--label", "build"], "sleep 60; printf never")
            .await;
        herd.wait_until("the build holds the slot", || {
            matches!(state_of(&herd, &build).as_str(), "assigned" | "running")
        })
        .await;
        trial = herd
            .submit_ok(&["--label", "metal-pc"], "printf trial")
            .await;
        herd.wait_until("the host is being emptied", || {
            herd.waiting_for(&trial).contains("being emptied")
        })
        .await;
        held_off = herd
            .submit_ok(&["--label", "build"], "printf second-build")
            .await;
        herd.wait_until("the second build is held off", || {
            herd.waiting_for(&held_off).contains("being emptied")
        })
        .await;
        // The hub, with `kill -9`: no drain, no chance to write anything
        // down.
        let mut hub = _hub;
        hub.signal(rustix::process::Signal::KILL).await;
    }
    // A hub that rebuilt a reservation from a counter would have none here.
    let _hub = herd.start_hub_losing_runners_after("second", 2.0).await;
    herd.wait_until("the restarted hub is still emptying the host", || {
        herd.waiting_for(&held_off).contains("being emptied")
            || herd.waiting_for(&trial).contains("being emptied")
    })
    .await;

    // And now the runner, with `kill -9`, taking the build with it. The slot
    // that build held is gone the moment its record says the attempt is over,
    // so the host is quiet and the trial runs on the runner that replaces it.
    runner.signal(rustix::process::Signal::KILL).await;
    let _again = herd.start_runner_with("wsl-pc", "again", "runner", &options);
    herd.wait_until("the trial runs on a quiet host", || {
        state_of(&herd, &trial) == "finished"
    })
    .await;
    assert_eq!(herd.log(&trial), "trial");
    // What becomes of the build that was held off is deliberately **not**
    // asserted: the build the killed runner was running gets its second go
    // (E-21) and holds the one slot again for the whole of its sleep, so the
    // second build waits behind it for a minute. That is E-21 working rather
    // than a leak, and the leak this test is about is the reservation — which
    // is proved by the trial having run at all: a reservation that had leaked
    // would have held the host empty against the trial itself, and one that
    // had been lost in the restart would have let a build in first.
    let _ = &held_off;
}

/// A switch survives a `kill -9` **in the instant `herdctl switch` returned**
/// (E-33, attack 12, D99).
///
/// The failure this defect would have caused in the field, and the one that
/// matters. Before D99 there was a window — about a second and a half on a
/// two-core host — between `herdctl switch off` reporting success and the
/// position reaching the machine's disk. A runner killed inside it came back in
/// its **previous** position while its owner believed her machine was off: she
/// had been told success, the hub honoured it, and a reboot gave the machine
/// away again.
///
/// So nothing is waited for here. The kill is the statement after the one that
/// read success, which is the strongest form of this test there is: the earlier
/// version of it waited for the hub to stop claiming first, which stepped
/// straight over the window.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_survives_a_kill_nine_on_the_runner() {
    let herd = Herd::new();
    let _hub = herd.start_hub_losing_runners_after("hub", 2.0).await;
    let mut runner =
        herd.start_runner_with("wsl-pc", "runner", "runner", &["--switch-poll", "100"]);
    herd.wait_for_runner().await;
    let said = herd
        .herdctl(&["switch", "wsl-pc", "off", "--reason", "she is gaming"])
        .await;
    assert!(
        said.status.success(),
        "herdctl switch failed: {} {}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    // No wait of any kind between the success and the kill.
    runner.signal(rustix::process::Signal::KILL).await;
    let _again = herd.start_runner_with("wsl-pc", "again", "runner", &["--switch-poll", "100"]);
    herd.wait_until("the runner is back", || {
        herd.said("again").contains("joining")
    })
    .await;
    // `joining` is the replacement runner's own news, and it is printed before
    // the hub has the connection: a submission made on the strength of it was
    // refused because *nothing was connected yet*, and the test then read that
    // refusal for her reason and failed. Reproduced on two pinned cores, twice
    // in three runs. So the refusal is the condition — and a submission the
    // hub **accepts** is the defect this test exists for and fails at once
    // (D100).
    let refusal = until_a_submission_is_refused_saying(&herd, "she is gaming").await;
    assert!(
        refusal.contains("wsl-pc"),
        "a runner killed in the instant `herdctl switch` returned comes back switched off, \
         and the refusal names it (E-33, D99): {refusal}"
    );
    // The reason is also where it has to be for the next life of this runner:
    // on the disk, not only in the hub's memory.
    let written = std::fs::read_to_string(herd.switch_file("runner"))
        .expect("the position the owner was promised is on the machine");
    assert!(written.contains("she is gaming"), "{written}");
}
