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

/// How long a hanging probe of these tests sleeps for. Numbers nobody else on
/// this host would be sleeping, so that finding one is finding ours — and one
/// each, because these tests run in one binary at the same time.
const HANGS_FOR: &str = "444442";
const HANGS_AND_IS_SWEPT_FOR: &str = "444443";

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
    let started = std::time::Instant::now();
    let hangs = BusyProbe::command(["/bin/sh", "-c", &format!("sleep {HANGS_FOR}")]);
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
    let hangs = BusyProbe::command([
        "/bin/sh",
        "-c",
        &format!("sleep {HANGS_AND_IS_SWEPT_FOR} & wait"),
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
        let survivors = processes_with_argument(HANGS_AND_IS_SWEPT_FOR);
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
