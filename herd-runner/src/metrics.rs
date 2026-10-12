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
