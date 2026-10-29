//! Byte sizes in configuration — the one table, kept in `herd-proto`.
//!
//! The parser and the formatter moved to [`herd_proto::size`] when the hub
//! grew a byte figure of its own (`--output-budget`, D152). Two copies of a
//! unit table is how `500MB` comes to mean two different numbers on two
//! sides of one fleet, and a budget an operator sets on the hub and reads
//! back in a runner's refusal has to be the same number both times.
//!
//! This module stays so that `herd_runner::size::parse_bytes` goes on
//! meaning what it meant: the runner's own caps are still spelled the same
//! way on its command line.
//!
//! **And it holds one formatter of its own, [`measured_bytes`].** The table
//! in `herd-proto` is about figures an operator *typed*, which is why it is
//! shared: a `500MB` set on the hub and read back in a runner's refusal has
//! to be the same number both times. A figure this runner *measured* was
//! typed by nobody and is agreed with nobody, so it is said here and said
//! exactly. The two are not interchangeable and the doc comment on each
//! says which figure wants which.

pub use herd_proto::size::{parse_bytes, show_bytes};

/// A byte count **this runner measured**, as a reader of a log line sees it:
/// the exact figure always, and a binary approximation beside it once the
/// exact figure stops being readable.
///
/// Not [`show_bytes`], and the difference is the whole reason this exists.
/// `show_bytes` prints a figure in the unit family it divides exactly, so a
/// cap an operator *typed* reads back the way they typed it — `200GB` as
/// `200 GB` (D148, D152). A spool's weight was typed by nobody: its decimal
/// family means nothing, and "the first unit it happens to divide by" makes
/// the unit a property of the measurement. The first consumer read
/// `535691 kB` on one offer line out of seventy-five and bytes on the other
/// seventy-four, because that one total happened to be a round multiple of
/// 1000 and of nothing larger. The figure was right; the unit moved.
///
/// So: exactness first and never instead. This is the hub's rule and the
/// hub's shape — the hub's own `report::bytes_said` and
/// `retention::said_bytes` print the same sentence — and the test of which
/// of the two formatters a site wants is one question: *did a human type
/// this figure, or did we measure it?*
pub fn measured_bytes(bytes: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if bytes < KIB {
        return format!("{bytes} bytes");
    }
    let (scale, unit) = if bytes >= GIB {
        (GIB, "GiB")
    } else if bytes >= MIB {
        (MIB, "MiB")
    } else {
        (KIB, "KiB")
    };
    format!("{bytes} bytes ({:.2} {unit})", bytes as f64 / scale as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The consumer's own figure, pinned as the literal line they will read.
    ///
    /// **This expectation is not a round trip and must not become one.** A
    /// test that asserted `measured_bytes(n)` against a second call of
    /// `measured_bytes` would stay green through exactly the change that
    /// breaks the reader: the writer and the reader moving together. The
    /// string is here, spelled out, because somebody downstream reads it.
    #[test]
    fn the_consumers_own_figure_prints_as_bytes_and_never_as_a_round_kb() {
        assert_eq!(
            measured_bytes(535_691_000),
            "535691000 bytes (510.87 MiB)",
            "535691000 is the figure the first consumer read as `535691 kB` on one offer \
             line in seventy-five. If this line changed, do NOT paste the new output over \
             the expectation: that is the one repair that is always wrong here, because the \
             expectation *is* the bug report. Change the formatter back."
        );
        assert!(
            !measured_bytes(535_691_000).contains("kB"),
            "no decimal unit may reach a measured figure: nobody typed it"
        );
    }

    /// The whole class, not the one case: a figure that divides exactly by
    /// 1000 is what `show_bytes` reaches for `kB` on, so each of these would
    /// have changed unit before.
    #[test]
    fn a_figure_that_divides_by_a_thousand_does_not_change_unit() {
        for (bytes, expected) in [
            (1_000u64, "1000 bytes"),
            (2_000, "2000 bytes (1.95 KiB)"),
            (1_000_000, "1000000 bytes (976.56 KiB)"),
            (535_691_000, "535691000 bytes (510.87 MiB)"),
            (1_000_000_000, "1000000000 bytes (953.67 MiB)"),
            (200_000_000_000, "200000000000 bytes (186.26 GiB)"),
        ] {
            assert_eq!(
                measured_bytes(bytes),
                expected,
                "a measured figure keeps its bytes whatever it divides by. If this line \
                 changed, the formatter changed: do not paste the new output over the \
                 expectation, because this list is the defect's own class."
            );
        }
    }

    /// Below a kibibyte there is nothing an approximation would make more
    /// readable, so there is none — the hub's shape exactly (`1136 bytes
    /// (1.11 KiB)` above it, `999 bytes` below).
    #[test]
    fn a_small_figure_carries_no_approximation_and_a_large_one_does() {
        assert_eq!(measured_bytes(0), "0 bytes");
        assert_eq!(measured_bytes(999), "999 bytes");
        assert_eq!(measured_bytes(1023), "1023 bytes");
        assert_eq!(measured_bytes(1024), "1024 bytes (1.00 KiB)");
        assert_eq!(measured_bytes(1136), "1136 bytes (1.11 KiB)");
        assert_eq!(measured_bytes(1 << 20), "1048576 bytes (1.00 MiB)");
        assert_eq!(measured_bytes(2 << 30), "2147483648 bytes (2.00 GiB)");
    }

    /// And the other formatter is untouched: a figure a human typed still
    /// reads back in the family they typed it in. This test is here so that
    /// a later tidy cannot "unify" the two without a red gate.
    #[test]
    fn a_typed_cap_still_reads_back_in_its_own_family() {
        assert_eq!(
            show_bytes(parse_bytes("200GB").expect("a budget")),
            "200 GB"
        );
        assert_eq!(show_bytes(parse_bytes("64MiB").expect("a cap")), "64 MiB");
        assert_eq!(show_bytes(2 << 30), "2 GiB");
    }
}
