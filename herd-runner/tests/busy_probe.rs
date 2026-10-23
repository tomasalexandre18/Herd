//! The command probes of E-26, against real processes.
//!
//! These live in their own test binary rather than beside the module they
//! test, and the reason is worth writing down: a probe is a `fork` plus an
//! `exec`, and between the two the child holds every descriptor its parent
//! held. One of those is the `flock` on a runner's data root (D32, E-45). The
//! unit tests of `root` take that lock, release it and take it again within
//! microseconds, so a probe forking in another thread of the same process made
//! that assertion fail about one run in six — not a defect in either, an
//! artefact of both living in one process. A real runner takes its data root
//! once, for the life of the process, and never races itself.
//!
//! The lock-file probe, which forks nothing, is unit-tested in the module.

use std::time::{Duration, Instant};

use herd_proto::{Availability, BusyProbe};
use herd_runner::capability::evaluate;

/// How long a hanging probe of these tests sleeps for: a number **this test
/// process** alone is sleeping, so that finding one is finding ours.
///
/// One each, because these tests run in one binary at the same time — and
/// the pid, because a constant is not unique enough. `confinement.rs`
/// declared two of the same figures, and a second checkout of this
/// repository running its own suite declares all of them: either one turns
/// a sweep of `/proc` into an assertion about somebody else`s leavings
/// (D101).
fn a_sleep_nobody_else_is_sleeping(which: u32) -> String {
    (1_000_000 + std::process::id() * 10 + which).to_string()
}

/// Two numbers, both configuration, and nothing else is read: not the
/// probe's output, not its name, not what the capability means.
#[tokio::test]
async fn a_command_probe_reads_its_two_configured_codes() {
    let timeout = Duration::from_secs(5);
    let busy = BusyProbe::command(["/bin/sh", "-c", "exit 0"]);
    let free = BusyProbe::command(["/bin/sh", "-c", "exit 1"]);
    assert!(evaluate(&busy, timeout).await.suspends());
    assert_eq!(evaluate(&free, timeout).await, Availability::Free);
    // Swapped, the meanings swap: the codes are the configuration's, not
    // Herd's.
    let swapped = BusyProbe::Command {
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 3".to_owned()],
        busy_exit_code: 3,
        free_exit_code: 0,
    };
    assert!(evaluate(&swapped, timeout).await.suspends());
    // A probe that writes a great deal and exits free is still free: its
    // output is never read, which is what keeps it a probe and not a protocol.
    let noisy = BusyProbe::command(["/bin/sh", "-c", "yes nonsense | head -c 200000; exit 1"]);
    assert_eq!(evaluate(&noisy, timeout).await, Availability::Free);
}

/// The three broken probes, each handled by name rather than by a guess, and
/// never as "free" — a probe nobody can read is not permission to hand out a
/// capability something outside Herd may be holding.
#[tokio::test]
async fn a_broken_probe_is_unknown_and_never_free() {
    let timeout = Duration::from_millis(300);

    // Not there at all: the commonest broken probe there is.
    let missing = BusyProbe::command(["/nonexistent/herd-probe"]);
    let read = evaluate(&missing, timeout).await;
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "a missing probe is unknown, not busy and not free: {read:?}"
    );
    assert!(read.suspends(), "{read:?}");
    assert!(read.reason().expect("a reason").contains("herd-probe"));

    // Non-zero for another reason: 7 means neither busy nor free here, and
    // "anything that is not busy is free" would have read it as free.
    let confused = BusyProbe::command(["/bin/sh", "-c", "exit 7"]);
    let read = evaluate(&confused, timeout).await;
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "an exit code with no meaning must not read as free: {read:?}"
    );
    let reason = read.reason().expect("a reason");
    assert!(reason.contains('7'), "{reason}");
    assert!(reason.contains("busy with 0"), "{reason}");

    // 127, which is what a shell gives a command it could not find: the exact
    // number a one-code rule would have read as free.
    let not_found = BusyProbe::command(["/bin/sh", "-c", "exec no-such-herd-probe"]);
    assert!(matches!(
        evaluate(&not_found, timeout).await,
        Availability::Unknown { .. }
    ));

    // Killed by a signal: no exit code at all.
    let signalled = BusyProbe::command(["/bin/sh", "-c", "kill -TERM $$"]);
    let read = evaluate(&signalled, timeout).await;
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "a signalled probe has no code to read: {read:?}"
    );
    assert!(read.reason().expect("a reason").contains("signal"));

    // Hanging: bounded by the timeout, killed, and said as such. What this
    // forbids is a probe holding a runner — and through it a hub — for ever.
    // It sleeps for a number nobody else would, because the assertion that it
    // left nothing behind is made below.
    let hangs_for = a_sleep_nobody_else_is_sleeping(2);
    let started = std::time::Instant::now();
    let hangs = BusyProbe::command(["/bin/sh", "-c", &format!("sleep {hangs_for}")]);
    let read = evaluate(&hangs, timeout).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a hanging probe must not hold the runner: {:?}",
        started.elapsed()
    );
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "a hanging probe is unknown: {read:?}"
    );
    assert!(read.reason().expect("a reason").contains("300 ms"));

    // A probe that waits on its standard input does not wait: its stdin is
    // closed, so the read sees end-of-file at once and the probe answers
    // instead of hanging. Whatever it then exits with is its own business —
    // what is asserted here is that it came back, and quickly.
    let started = std::time::Instant::now();
    let reads_stdin = BusyProbe::command(["/bin/sh", "-c", "read line || exit 9"]);
    let read = evaluate(&reads_stdin, timeout).await;
    assert!(
        started.elapsed() < timeout,
        "a probe reading its stdin must see end-of-file at once, not wait: {:?}",
        started.elapsed()
    );
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "exit 9 means neither busy nor free here: {read:?}"
    );
}

/// A probe that is killed leaves nothing of itself running.
///
/// The shape this is about is the ordinary one: a probe is a command an
/// owner wrote (D51), and an owner writes a shell line. A kill that reached
/// only the shell would leave whatever it started behind — and a probe is
/// re-read every interval, so one hanging shell line means an orphan every
/// two seconds for as long as the runner is up. That is a runner
/// destabilising its host through its own configuration, which invariant 2
/// forbids, and it is what made a test suite leave thirty-two `sleep`s on
/// this machine. The probe is its own process group and is killed as one.
#[tokio::test]
async fn a_killed_probe_leaves_nothing_of_itself_behind() {
    // `sleep & wait` rather than a bare `sleep`, which a shell execs into
    // itself: what is asserted here is a probe with a *child*, which is the
    // shape that leaked. The hanging probe asserted above is the execed one.
    let hangs_and_is_swept_for = a_sleep_nobody_else_is_sleeping(3);
    let hangs = BusyProbe::command([
        "/bin/sh",
        "-c",
        &format!("sleep {hangs_and_is_swept_for} & wait"),
    ]);
    let read = evaluate(&hangs, Duration::from_millis(200)).await;
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "a hanging probe is unknown: {read:?}"
    );
    // The kill is a signal, so the children go away a moment after the probe
    // has been read: this waits for them rather than racing them.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let survivors = processes_with_argument(&hangs_and_is_swept_for);
        if survivors.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{survivors:?} outlived the probe that started them; a probe read \
             every interval would strew the host with these"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A runner asked to stop takes its probes' children with it.
///
/// The distinct case, and the one the timeout path did not cover: the probe is
/// not hanging past anything — its timeout is a minute away — it is simply
/// *in flight* when the owner asks the runner to stop. `kill_on_drop` reaps
/// the probe's own process and nothing it started, so the runner exited in a
/// fifth of a second and left `sleep` running, reparented to init. Invariant 2
/// says a runner never destabilises its host; a probe is read every two
/// seconds, so one stop at the wrong moment is one orphan, and a machine
/// restarted all day is a machine filling up with them.
///
/// The real binary, a real `SIGTERM` and a real sweep of `/proc`: the
/// cancellation this is about happens between a task being dropped and a
/// process exiting, which nothing short of the process can show.
#[tokio::test]
async fn a_runner_told_to_stop_takes_its_probes_children_with_it() {
    let hangs_while_the_runner_stops_for = a_sleep_nobody_else_is_sleeping(4);
    let data_root = herd_runner::testing::TempDir::new("probe-stop");
    let capabilities = data_root.path().join("capabilities.json");
    // A probe of the ordinary shape (D51): a shell line with a child. The
    // timeout is a minute, so this reading is still in flight when the signal
    // arrives — what is under test is the stop, not the timeout.
    std::fs::write(
        &capabilities,
        format!(
            r#"{{"capabilities":[
                {{"name":"metal-pc","kind":{{"type":"counted","slots":1}},
                 "busy_probe":{{"type":"command",
                   "command":["/bin/sh","-c","sleep {hangs_while_the_runner_stops_for} & wait"]}}}}
            ]}}"#
        ),
    )
    .expect("the capability file is written");
    // No hub: the probes are read beside the redial loop, so a runner with
    // nothing to dial still reads them — and this test is about the runner's
    // own host, not about a link.
    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg("ws://127.0.0.1:1")
        .arg("--data-root")
        .arg(data_root.path())
        .arg("--capabilities")
        .arg(&capabilities)
        .arg("--no-detect")
        .arg("--probe-interval")
        .arg("50")
        .arg("--probe-timeout")
        .arg("60000")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the real runner binary runs");
    let pid = runner.id().expect("a spawned process has a pid");

    let deadline = Instant::now() + Duration::from_secs(20);
    while processes_with_argument(&hangs_while_the_runner_stops_for).is_empty() {
        assert!(Instant::now() < deadline, "the probe never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Safe: one signal, to one process this test spawned itself.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let asked = Instant::now();
    let ended = tokio::time::timeout(Duration::from_secs(10), runner.wait())
        .await
        .expect("a runner asked to stop does not hang on its probes")
        .expect("the runner's own exit is readable");
    assert!(ended.success(), "a runner told to stop left by {ended:?}");
    // Invariant 2's five seconds, measured: killing a probe's group is not
    // allowed to be paid for with the owner's stop time.
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "the stop took {:?}",
        asked.elapsed()
    );

    // The kill is a signal, so the children go a moment after the runner has:
    // this waits for them rather than racing them.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let survivors = processes_with_argument(&hangs_while_the_runner_stops_for);
        if survivors.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{survivors:?} outlived the runner whose probe started them, \
             reparented to init; a clean stop may not strew a host with these"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A probe's child that gave itself a new **session** dies with the reading
/// too — on the timeout path.
///
/// The group kill is what D62 promised, and a new session is precisely what a
/// process group kill cannot reach: `setsid` leaves the group by definition.
/// `/bin/sh -c "setsid sleep N & wait"` survived every kill the probe path
/// had, at ppid 1, which made D62's "killed as one" not quite true. It is
/// M4's own case, and the fix is M4's own mechanism — a marker in the
/// environment and a sweep of `/proc` — rather than a second one.
#[tokio::test]
async fn a_probe_child_that_left_the_group_dies_on_the_timeout_too() {
    let leaves_its_session_on_a_timeout_for = a_sleep_nobody_else_is_sleeping(5);
    let Some(setsid) = setsid_on_this_host() else {
        herd_runner::testing::skipped(
            "a_probe_child_that_left_the_group_dies_on_the_timeout_too",
            "setsid is not on this host, so a probe child cannot be put in a new session \
             to be swept out of one",
        );
        return;
    };
    let hangs = BusyProbe::command([
        "/bin/sh",
        "-c",
        &format!("{setsid} sleep {leaves_its_session_on_a_timeout_for} & wait"),
    ]);
    let read = evaluate(&hangs, Duration::from_millis(200)).await;
    assert!(
        matches!(read, Availability::Unknown { .. }),
        "a hanging probe is unknown: {read:?}"
    );
    wait_until_gone(
        &leaves_its_session_on_a_timeout_for,
        "outlived the probe that started them by leaving its process group; the group \
         kill cannot reach a new session and the marker sweep is what must",
    )
    .await;
}

/// And on a clean stop, which is the case verification actually caught.
///
/// Observed before the fix: this exact probe, a real `SIGTERM` to the real
/// runner binary, and `sleep` still running at ppid 1 afterwards. One
/// survivor per stop rather than one per reading, so not a host hazard — but
/// D62 says a probe is killed as one group "on a timeout and on a clean stop
/// alike", and that has to be true.
///
/// The real binary and a real signal, like its sibling above it: the kill
/// this is about happens between a task being dropped and a process exiting,
/// which nothing short of the process can show.
#[tokio::test]
async fn a_probe_child_that_left_the_group_dies_when_the_runner_is_stopped() {
    let leaves_its_session_while_the_runner_stops_for = a_sleep_nobody_else_is_sleeping(6);
    let Some(setsid) = setsid_on_this_host() else {
        herd_runner::testing::skipped(
            "a_probe_child_that_left_the_group_dies_when_the_runner_is_stopped",
            "setsid is not on this host, so a probe child cannot be put in a new session \
             to be swept out of one",
        );
        return;
    };
    let data_root = herd_runner::testing::TempDir::new("probe-setsid-stop");
    let capabilities = data_root.path().join("capabilities.json");
    // A minute of timeout, so the reading is still in flight when the signal
    // arrives: what is under test is the stop, not the timeout.
    std::fs::write(
        &capabilities,
        format!(
            r#"{{"capabilities":[
                {{"name":"metal-pc","kind":{{"type":"counted","slots":1}},
                 "busy_probe":{{"type":"command",
                   "command":["/bin/sh","-c",
                     "{setsid} sleep {leaves_its_session_while_the_runner_stops_for} & wait"]}}}}
            ]}}"#
        ),
    )
    .expect("the capability file is written");
    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_herd-runner"))
        .arg("--hub")
        .arg("ws://127.0.0.1:1")
        .arg("--data-root")
        .arg(data_root.path())
        .arg("--capabilities")
        .arg(&capabilities)
        .arg("--no-detect")
        .arg("--probe-interval")
        .arg("50")
        .arg("--probe-timeout")
        .arg("60000")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the real runner binary runs");
    let pid = runner.id().expect("a spawned process has a pid");

    let deadline = Instant::now() + Duration::from_secs(20);
    let escaped = loop {
        if let Some(found) =
            processes_with_argument(&leaves_its_session_while_the_runner_stops_for).first()
        {
            break *found;
        }
        assert!(Instant::now() < deadline, "the probe never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // The whole premise, asserted rather than assumed: this process really is
    // the leader of a session of its own, so no kill of the probe's process
    // group can reach it and only the marker sweep can.
    assert_eq!(
        session_of(escaped),
        Some(escaped),
        "the fixture must really have left its group, or this test proves nothing"
    );

    // Safe: one signal, to one process this test spawned itself.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let asked = Instant::now();
    let ended = tokio::time::timeout(Duration::from_secs(10), runner.wait())
        .await
        .expect("a runner asked to stop does not hang on sweeping for its probes")
        .expect("the runner's own exit is readable");
    assert!(ended.success(), "a runner told to stop left by {ended:?}");
    // Invariant 2's five seconds, measured: the `/proc` sweep this fix adds
    // is not allowed to be paid for with the owner's stop time.
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "the stop took {:?}",
        asked.elapsed()
    );

    wait_until_gone(
        &leaves_its_session_while_the_runner_stops_for,
        "outlived the runner whose probe started them, in a session of their own at \
         ppid 1; a clean stop may not leave one of these behind",
    )
    .await;
}

/// Where `setsid` is on this host, if it is anywhere.
///
/// Looked up rather than assumed: the fixture needs a real new session and
/// there is no portable shell builtin that makes one. A host without it
/// records a skip (D27) instead of a green run that checked nothing.
fn setsid_on_this_host() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("setsid"))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

/// The session a process belongs to: field 6 of `/proc/<pid>/stat`, counting
/// from 1. Equal to the pid itself exactly when the process leads its own.
///
/// `comm` is field 2 and may contain spaces and brackets, so the split starts
/// after the last `)` rather than at the first space.
fn session_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    // After `comm` the next field is `state`, so `session` is the fourth of
    // what is left: state, ppid, pgrp, session.
    rest.split_whitespace().nth(3)?.parse().ok()
}

/// Wait for every process carrying `needle` to be gone, and say what is left
/// if any of them stays.
///
/// A kill is a signal, so the children go a moment after whatever was holding
/// them: this waits for them rather than racing them.
async fn wait_until_gone(needle: &str, complaint: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let survivors = processes_with_argument(needle);
        if survivors.is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "{survivors:?} {complaint}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every live process whose command line carries this argument.
///
/// The same sweep `confinement.rs` uses to find a job's detached children,
/// kept here rather than shared: a test binary that reaches into another
/// test binary for a helper is a worse dependency than ten lines of `/proc`.
fn processes_with_argument(needle: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if cmdline
            .split(|byte| *byte == 0)
            .any(|argument| argument == needle.as_bytes())
        {
            found.push(pid);
        }
    }
    found
}

/// The lease probe of E-27, against **the consumer's own four exit codes**
/// (issue #16, D76, attack 8).
///
/// `cargo xtask metal <machine> --lease-status` exits 0 free, 10 busy with
/// `holder=` and `expires=` on stdout, 1 when it cannot tell, and 64 for an
/// unknown machine or bad usage. Herd is configured with two of those — busy
/// 10, free 0 — and the whole of this test is that the other two, and
/// everything like them, **suspend the capability naming the code** (D48)
/// rather than reading as free.
///
/// The consumer asked for "treat as busy"; this is the stricter reading they
/// agreed to, and the reason is in the two sentences it produces: a
/// permanently broken probe surfaces as a capability nobody can offer, with
/// its exit code in the sentence, instead of a machine that is mysteriously
/// always busy.
#[tokio::test]
async fn the_lease_probe_is_read_on_exactly_its_two_declared_codes() {
    let timeout = Duration::from_secs(5);
    // As a machine's operator declares it in her own capability file (D86).
    let lease = |exit: &str| BusyProbe::Command {
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!("echo holder=alexa expires=1792000000; {exit}"),
        ],
        busy_exit_code: 10,
        free_exit_code: 0,
    };

    // 0 — free: no lease, or an expired one.
    assert_eq!(
        evaluate(&lease("exit 0"), timeout).await,
        Availability::Free
    );

    // 10 — busy: somebody holds it. The job waits (D38), it is never failed,
    // and the sentence carries the code.
    let read = evaluate(&lease("exit 10"), timeout).await;
    assert!(
        matches!(read, Availability::Busy { .. }),
        "10 is busy: {read:?}"
    );
    let reason = read.reason().expect("a busy probe says why");
    assert!(reason.contains("exited 10"), "{reason}");
    assert!(reason.contains("which means busy"), "{reason}");

    // 1 — cannot tell: an unreadable or unparsable lease, or its lock held
    // past five seconds. **Suspended, naming the code**, not busy and never
    // free.
    for (code, what) in [
        ("exit 1", "a lease it cannot read"),
        ("exit 64", "an unknown machine or bad usage"),
        ("exec no-such-xtask", "no such command at all (127)"),
        ("kill -TERM $$", "killed by a signal"),
    ] {
        let read = evaluate(&lease(code), timeout).await;
        assert!(
            matches!(read, Availability::Unknown { .. }),
            "{what} must suspend the capability, never read as free: {read:?}"
        );
        assert!(read.suspends(), "{what}: {read:?}");
        assert_ne!(
            read,
            Availability::Free,
            "{what} must never be free: a lease nobody can read is not a lease nobody holds"
        );
        let reason = read.reason().expect("an unreadable probe says why");
        // Either the code it gave, or the fact that it gave none.
        assert!(
            reason.contains("exited") || reason.contains("signal"),
            "{what}: the sentence has to name what happened: {reason}"
        );
        assert!(
            reason.contains("metal") || reason.contains("xtask") || reason.contains("holder"),
            "and which probe it was: {reason}"
        );
    }

    // And the output is never read, whatever it says: `holder=alexa` on
    // stdout beside an exit of 0 is **free**, because the exit code is the
    // whole of the contract (D51 — a probe is a command and two numbers, not
    // a protocol).
    let lying = BusyProbe::Command {
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "echo holder=somebody expires=99999999999; exit 0".to_owned(),
        ],
        busy_exit_code: 10,
        free_exit_code: 0,
    };
    assert_eq!(evaluate(&lying, timeout).await, Availability::Free);
}

/// `herd-runner check --capabilities FILE`, which is the whole lesson of the
/// outage of 2026-10-23: the file was wrong before it was deployed, and
/// nothing said so until jobs were late (D165).
///
/// It lives in this binary and not beside the module because it *runs* the
/// probes, and running one is a `fork` — the hazard this file's own header
/// explains.
#[tokio::test]
async fn check_reads_a_capability_file_and_its_probes_once() {
    let settings = herd_runner::capability::ProbeSettings {
        interval: Duration::from_secs(2),
        timeout: Duration::from_secs(5),
    };
    let dir = herd_runner::testing::TempDir::new("check-capabilities");

    // The file the consumer deployed, with its placeholders unsubstituted.
    // One probe, unstartable: `check` exits non-zero and says which.
    let broken = dir.path().join("broken.json");
    std::fs::write(
        &broken,
        br#"{"capabilities":[
            {"name":"kvm","kind":{"type":"shared"},
             "busy_probe":{"type":"command",
                           "command":["@probe@","qemu","@agents_user@"]}}
        ]}"#,
    )
    .expect("written");
    let (lines, ok) = herd_runner::capability::check(&broken, settings).await;
    assert!(
        !ok,
        "a file a runner refuses must fail this command: {lines:#?}"
    );
    let said = lines.join("\n");
    assert!(said.contains("@probe@"), "{said}");
    assert!(said.contains("kvm"), "{said}");
    assert!(said.contains("REFUSED"), "{said}");

    // And a file whose probes can be started: each is run once and its
    // verdict printed, and an unreadable *answer* is reported without
    // failing the command — that suspends the capability at runtime (D49)
    // and is often the honest answer of the moment.
    let fine = dir.path().join("fine.json");
    let held = dir.path().join("held.lock");
    std::fs::write(&held, b"").expect("the lock is held");
    std::fs::write(
        &fine,
        format!(
            r#"{{"capabilities":[
                {{"name":"free-one","kind":{{"type":"shared"}},
                 "busy_probe":{{"type":"command","command":["/bin/sh","-c","exit 1"]}}}},
                {{"name":"busy-one","kind":{{"type":"shared"}},
                 "busy_probe":{{"type":"command","command":["/bin/sh","-c","exit 0"]}}}},
                {{"name":"unreadable-one","kind":{{"type":"shared"}},
                 "busy_probe":{{"type":"command","command":["/bin/sh","-c","exit 42"]}}}},
                {{"name":"metal-pc","kind":{{"type":"counted","slots":1}},
                 "busy_probe":{{"type":"lock_file","path":{}}}}}
            ],"deny":["nvidia"]}}"#,
            serde_json::to_string(&held.display().to_string()).expect("a path encodes")
        )
        .as_bytes(),
    )
    .expect("written");
    let (lines, ok) = herd_runner::capability::check(&fine, settings).await;
    assert!(
        ok,
        "every probe here names a program that starts, so the file is deployable: {lines:#?}"
    );
    let said = lines.join("\n");
    assert!(said.contains("free-one: shared"), "{said}");
    assert!(said.contains("free"), "{said}");
    assert!(said.contains("busy"), "{said}");
    assert!(
        said.contains("unreadable") && said.contains("D49"),
        "an unreadable answer is reported as D49's suspension, not as a refusal: {said}"
    );
    assert!(
        said.contains("nvidia: denied"),
        "the denials are part of what the file says: {said}"
    );
}
