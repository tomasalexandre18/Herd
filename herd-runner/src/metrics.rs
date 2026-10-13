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

/// `MemTotal` in MiB, or nothing when `/proc/meminfo` cannot be read or does
/// not carry it.
fn total_memory_mib() -> Option<u64> {
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
