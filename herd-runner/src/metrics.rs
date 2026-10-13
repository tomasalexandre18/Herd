//! What a runner measures while a job runs, so that the job's record carries
//! figures and not just a verdict (E-15's resource fields).
//!
//! Three figures, from three different places: the host's load from
//! `/proc/loadavg`, the job's peak memory from its cgroup where there is one
//! and from its own process where there is not, and what it left on disk from
//! the workspace the disk budget is enforced against. A figure that cannot be
//! read stays absent — never zero, because zero is a measurement.

use std::fs;

/// The host's one-minute load average, times 1000.
///
/// Load is recorded, never a gate: E-32 is explicit that a busy host is a
/// fact to report, not a reason to refuse a job. Admission is M8's, and it is
/// about memory.
pub fn host_load_milli() -> Option<u64> {
    let text = fs::read_to_string("/proc/loadavg").ok()?;
    let first = text.split_whitespace().next()?;
    let (whole, fraction) = match first.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (first, "0"),
    };
    let whole: u64 = whole.parse().ok()?;
    // /proc/loadavg has two decimals; pad to three so the unit is milli
    // whatever the kernel prints.
    let mut digits = fraction.chars().filter(|c| c.is_ascii_digit());
    let mut milli = 0u64;
    for scale in [100u64, 10, 1] {
        let digit = digits.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
        milli += u64::from(digit) * scale;
    }
    Some(whole.saturating_mul(1000).saturating_add(milli))
}

/// How big this host is, as far as it can be read from the host itself
/// (E-22).
///
/// `MemTotal` from `/proc/meminfo`, and the thread count the standard library
/// reports. Both become the input to the hub's `-j` derivation, so **a figure
/// that cannot be read is absent and never zero**: a host reporting 0 MiB
/// would be a host no job fits on, and a hub that read it as a figure would
/// derive from nothing.
///
/// `MemTotal` is the whole machine, which is not always what the owner is
/// lending. That is why the runner's `--host-memory` exists and why the
/// derivation keeps a quarter of whatever it is told in reserve: the owner of
/// a machine is the authority on how much of it Herd may have, and the
/// default must at least not be a figure larger than the machine.
pub fn host_size() -> herd_proto::HostSize {
    herd_proto::HostSize::new(
        total_memory_mib(),
        std::thread::available_parallelism()
            .ok()
            .and_then(|threads| u32::try_from(threads.get()).ok()),
    )
}

/// Where a Linux host's tmpfs is, unless its owner says otherwise.
///
/// `/dev/shm` is a tmpfs on every Linux that has one, it is where a tool that
/// wants shared memory looks, and it is readable without privilege. An owner
/// whose project uses a different mount says so with `--host-tmpfs-path`, and
/// one who wants to lend a figure of her own choosing says so with
/// `--host-tmpfs` and this is never consulted.
pub const DEFAULT_TMPFS_PATH: &str = "/dev/shm";

/// How much room the tmpfs at `path` has in all, in MiB, or the reason there
/// is no figure (E-31).
///
/// The **capacity**, deliberately, and not what is free this second. What is
/// free is the hub's arithmetic: this figure less what the jobs its records
/// say are running here declared they would reserve (D79). A runner that
/// announced free room instead would be announcing a number that moves under
/// the hub between the judgement and the claim, and a reservation weighed
/// against a moving figure is the overcommit E-32 forbids.
///
/// `Err` and never zero when the path is not there, is not a tmpfs, or cannot
/// be read: a hub handed `Some(0)` would read a machine that lends no tmpfs,
/// which is a different statement from a machine nobody could measure, and
/// M4's disk figure reported as zero is the defect this rule is written for.
pub fn tmpfs_room_mib(path: &str) -> std::result::Result<u64, String> {
    let measured = statfs(path)?;
    if measured.kind != TMPFS_MAGIC {
        return Err(format!(
            "{path} is not a tmpfs (its filesystem type is {:#x}), so the room on it is not \
             tmpfs room; give --host-tmpfs-path a tmpfs, or --host-tmpfs the figure this \
             machine lends",
            measured.kind
        ));
    }
    // A block size or a block count of zero is a mount this cannot measure,
    // not a mount with no room: `Ok(0)` would be announced as a host that
    // lends no tmpfs, which is a statement, and the whole rule here is that an
    // unmeasurable figure is absent rather than said.
    if measured.block_size == 0 || measured.blocks == 0 {
        return Err(format!(
            "{path} measures as {} blocks of {} bytes, which is not a size this can read",
            measured.blocks, measured.block_size
        ));
    }
    Ok(measured.blocks.saturating_mul(measured.block_size) / (1 << 20))
}

/// The magic number of a tmpfs, as the kernel has reported it since 2.4.
const TMPFS_MAGIC: i64 = 0x0102_1994;

struct Filesystem {
    kind: i64,
    blocks: u64,
    block_size: u64,
}

#[cfg(target_os = "linux")]
fn statfs(path: &str) -> std::result::Result<Filesystem, String> {
    use std::ffi::CString;
    let c_path = CString::new(path)
        .map_err(|_| format!("{path} has a zero byte in it, so it is no path"))?;
    // Safe: `statfs` fills a struct this call owns, from a path this call
    // owns, and its answer is checked before the struct is read.
    let mut answer = unsafe { std::mem::zeroed::<libc::statfs>() };
    let asked = unsafe { libc::statfs(c_path.as_ptr(), &mut answer) };
    if asked != 0 {
        return Err(format!(
            "{path} cannot be measured: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Filesystem {
        // `f_type` is already an i64 on the targets Herd builds for; the
        // field is kept typed so a port does not read a truncated magic.
        kind: answer.f_type as i64,
        blocks: answer.f_blocks,
        block_size: u64::try_from(answer.f_bsize).unwrap_or(0),
    })
}

#[cfg(not(target_os = "linux"))]
fn statfs(path: &str) -> std::result::Result<Filesystem, String> {
    Err(format!(
        "{path} cannot be measured: this is not Linux, and Herd only knows how to measure a \
         tmpfs there"
    ))
}

/// The memory this runner may announce, given what its owner asked for and
/// what the machine actually has (E-29, invariant 2).
///
/// **An override may lend less than the machine has, never more.** The floor
/// E-29 subtracts exists so a host keeps room to live, and a figure larger
/// than `MemTotal` makes it guard memory that does not exist: a 6 GiB floor
/// on a declared 64 GiB is 58 GiB of admission on a machine of 17, and the
/// first job past the real total is the owner's PC swapping — which is the
/// 2026-10-12 OOM E-30 exists to stop, arrived by a different road.
///
/// So the figure is **clamped, loudly**, and the sentence names both figures.
/// Clamped rather than refused because the owner's intent is still readable
/// and still safe — she is lending her machine, and the clamp lends all of it
/// — and because a runner that would not start would take a machine out of the
/// fleet for a typo. Never silently accepted: that was the one option ruled
/// out from the start.
///
/// When `MemTotal` could not be read there is nothing to clamp against, so
/// the owner's figure stands and the sentence says it was not checked. An
/// unreadable figure is unknown, not zero, here as everywhere.
pub fn lendable_memory_mib(asked_mib: u64, mem_total_mib: Option<u64>) -> (u64, Option<String>) {
    match mem_total_mib {
        None => (
            asked_mib,
            Some(format!(
                "herd-runner: lending {asked_mib} MiB as asked; this host's MemTotal could not \
                 be read, so the figure was not checked against the machine"
            )),
        ),
        Some(total) if asked_mib > total => (
            total,
            Some(format!(
                "herd-runner: --host-memory asked for {asked_mib} MiB and this machine has \
                 {total} MiB in all, so {total} MiB is what is lent. A figure larger than the \
                 machine would make the per-runner admission floor guard memory that does not \
                 exist (E-29), and the first job past the real total is a host that swaps."
            )),
        ),
        Some(_) => (asked_mib, None),
    }
}

/// `MemTotal` in MiB, or nothing when `/proc/meminfo` cannot be read or does
/// not carry it.
pub fn total_memory_mib() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("MemTotal:") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let figure: u64 = words.next()?.parse().ok()?;
        // The kernel writes kB and has always written kB; a line in any other
        // unit is one this cannot read, and an unreadable figure is absent.
        return match words.next() {
            Some("kB") => Some(figure / 1024),
            _ => None,
        };
    }
    None
}

/// The resident memory of one process, in bytes, read from `/proc`.
///
/// Used only where there is no cgroup to ask: it is the job's own process and
/// not its children, which undercounts a process tree. [`crate::cgroup`] is
/// where the honest figure comes from.
pub fn process_rss_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let resident_pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(resident_pages.saturating_mul(page_size()))
}

#[cfg(unix)]
fn page_size() -> u64 {
    // Safe: sysconf reads one constant out of the C library.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 {
        size as u64
    } else {
        4096
    }
}

#[cfg(not(unix))]
fn page_size() -> u64 {
    4096
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The figure must be a plausible load, and it must be present on a Linux
    /// host: /proc/loadavg is not optional there.
    #[test]
    fn the_host_s_load_is_read_as_an_integer_number_of_thousandths() {
        let load = host_load_milli();
        if cfg!(target_os = "linux") {
            let load = load.expect("/proc/loadavg is readable on Linux");
            assert!(
                load < 10_000_000,
                "a load of {load} thousandths is not a load"
            );
        }
    }

    /// **Attack 5.** An override larger than the machine would make the
    /// per-runner admission floor guard memory that does not exist. Clamped,
    /// and the clamp is said out loud with both figures — a silent accept was
    /// never on the table.
    #[test]
    fn an_override_larger_than_the_machine_is_clamped_and_said_out_loud() {
        let (lent, said) = lendable_memory_mib(64 * 1024, Some(17 * 1024));
        assert_eq!(lent, 17 * 1024, "a machine of 17 GiB cannot lend 64");
        let said = said.expect("a clamp is never silent");
        assert!(said.contains("asked for 65536 MiB"), "{said}");
        assert!(said.contains("has 17408 MiB in all"), "{said}");
        assert!(said.contains("17408 MiB is what is lent"), "{said}");
        assert!(
            said.contains(
                "would make the per-runner admission floor guard memory that does \
                           not exist"
            ),
            "the sentence has to say why, or the clamp reads as a bug: {said}"
        );
    }

    /// Lending **less** than the machine has is the whole point of the
    /// option, and it passes through untouched and unremarked.
    #[test]
    fn an_owner_may_lend_less_of_her_machine_than_it_has() {
        let (lent, said) = lendable_memory_mib(17 * 1024, Some(24 * 1024));
        assert_eq!(lent, 17 * 1024);
        assert_eq!(said, None, "lending part of a machine is not news");
        // Exactly the whole machine is not a lie either.
        let (lent, said) = lendable_memory_mib(17 * 1024, Some(17 * 1024));
        assert_eq!(lent, 17 * 1024);
        assert_eq!(said, None);
    }

    /// And when `MemTotal` could not be read there is nothing to clamp
    /// against. The owner's figure stands, and the sentence says it was not
    /// checked — rather than the figure being silently read as zero, or the
    /// runner refusing to start on a host whose `/proc` it cannot see.
    #[test]
    fn an_unreadable_memtotal_leaves_the_figure_alone_and_says_so() {
        let (lent, said) = lendable_memory_mib(64 * 1024, None);
        assert_eq!(lent, 64 * 1024);
        let said = said.expect("an unchecked figure is said to be unchecked");
        assert!(said.contains("MemTotal could not be read"), "{said}");
        assert!(said.contains("was not checked"), "{said}");
    }

    /// E-31's input. On a Linux host `/dev/shm` is a tmpfs with room on it,
    /// and a path that is not one is an **error** rather than a zero: a hub
    /// handed `Some(0)` would read a machine that lends no tmpfs, which is a
    /// different statement from a machine nobody could measure.
    #[test]
    fn the_tmpfs_room_is_measured_or_named_but_never_zero() {
        if !cfg!(target_os = "linux") {
            return;
        }
        match tmpfs_room_mib(DEFAULT_TMPFS_PATH) {
            Ok(room) => assert!(room > 0, "{DEFAULT_TMPFS_PATH} has no room at all?"),
            // A container without /dev/shm is a real host; what matters is
            // that it says so rather than announcing a figure.
            Err(why) => assert!(why.contains(DEFAULT_TMPFS_PATH), "{why}"),
        }
        // A path that exists and is not a tmpfs: named, with its type.
        let why = tmpfs_room_mib("/proc").expect_err("/proc is not a tmpfs");
        assert!(why.contains("is not a tmpfs"), "{why}");
        // A path that does not exist at all: named, and not read as empty.
        let why = tmpfs_room_mib("/herd-no-such-mount-point").expect_err("there is no such path");
        assert!(why.contains("cannot be measured"), "{why}");
    }

    /// E-22's input. On Linux both figures are there, and the memory is a
    /// plausible size for a machine — above all it is not zero, because zero
    /// is what a derivation must never be handed.
    #[test]
    fn this_host_says_how_big_it_is_and_never_says_zero() {
        let size = host_size();
        if cfg!(target_os = "linux") {
            let memory = size.memory_mib.expect("/proc/meminfo carries MemTotal");
            assert!(memory > 64, "{memory} MiB is not a machine");
            assert!(memory < 64 * 1024 * 1024, "{memory} MiB is not a machine");
            let threads = size.threads.expect("the thread count is readable");
            assert!(threads >= 1, "a host with no thread cannot run this test");
            assert!(size.what_is_missing().is_none());
        }
    }

    #[test]
    fn this_process_has_a_resident_size_and_a_dead_one_has_none() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let mine = process_rss_bytes(std::process::id()).expect("/proc has this process");
        assert!(mine > 0, "this process holds no memory at all?");
        assert!(
            mine < 100 * (1 << 30),
            "{mine} bytes resident is not a measurement"
        );
        // Pid 0 never exists, so the figure is absent rather than zero.
        assert_eq!(process_rss_bytes(0), None);
    }
}
