//! Byte sizes in configuration.
//!
//! Every cap Herd enforces is a number of bytes, and a number of bytes is
//! unreadable past a few megabytes. So the configuration accepts a plain
//! integer or an integer with a unit suffix, and nothing else: a string and a
//! number, never an expression (invariant 1).
//!
//! **It lives here because three binaries now state byte figures**: a
//! runner's caps, the hub's output budget (D152), and the refusals that
//! print them back. A unit table is not a protocol message, and D151 was
//! right that a logging helper has no business in this crate — but a figure
//! an operator sets on one side of the link and reads back from the other
//! is exactly a thing the two sides must agree about, which is what this
//! crate is for. `herd_runner::size` re-exports it, so the runner's own
//! command line is unchanged.
//!
//! **Two rules live here, not one, and they are side by side on purpose.**
//! [`show_bytes`] says a figure a human *typed*, in the family they typed it
//! in; [`measured_bytes`] says a figure Herd *measured*, exactly, with a
//! binary approximation beside it. They are not interchangeable, the
//! difference is a defect the first consumer actually read (ORCH-186), and
//! the doc comment on each one says which figure wants which.

/// Parse a byte size: `4096`, `64KiB`, `2GiB`, `500MB`.
///
/// Both the binary (`KiB`, 1024) and the decimal (`kB`, 1000) families are
/// accepted, case-insensitively, with or without the trailing `B`. A value
/// that does not parse names itself in the error, because a cap read wrongly
/// is worse than a cap refused.
pub fn parse_bytes(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("a byte size cannot be empty".to_owned());
    }
    let digits_end = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (digits, suffix) = trimmed.split_at(digits_end);
    if digits.is_empty() {
        return Err(format!("{trimmed:?} does not start with a number of bytes"));
    }
    let number: u64 = digits
        .parse()
        .map_err(|_| format!("{digits:?} is too large to be a number of bytes"))?;
    let multiplier = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1u64,
        "k" | "kb" => 1_000,
        "ki" | "kib" => 1 << 10,
        "m" | "mb" => 1_000_000,
        "mi" | "mib" => 1 << 20,
        "g" | "gb" => 1_000_000_000,
        "gi" | "gib" => 1 << 30,
        "t" | "tb" => 1_000_000_000_000,
        "ti" | "tib" => 1 << 40,
        other => {
            return Err(format!(
                "{other:?} is not a byte unit; use B, KiB, MiB, GiB, TiB or their \
                 decimal kB, MB, GB, TB"
            ))
        }
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| format!("{trimmed:?} is larger than this machine can address"))
}

/// A byte count as a human reads it, for the one line a refusal prints. Exact
/// multiples only: a cap is a figure, and a rounded figure in a refusal is
/// how someone concludes the cap was not the one they set.
///
/// **Both families, binary first.** Every cap in Herd is a power of two and
/// reads as one; the hub's output budget is not — 200 GB is a figure Alexa
/// chose in decimal (D148), and printing it as `200000000000 bytes` at every
/// start would make the one number an operator has to recognise the one
/// number they cannot. A size that is an exact multiple of neither family
/// stays in bytes, which is the honest answer and not a rounding.
pub fn show_bytes(bytes: u64) -> String {
    for (unit, scale) in [
        ("GiB", 1u64 << 30),
        ("GB", 1_000_000_000),
        ("MiB", 1 << 20),
        ("MB", 1_000_000),
        ("KiB", 1 << 10),
        ("kB", 1_000),
    ] {
        if bytes >= scale && bytes.is_multiple_of(scale) {
            return format!("{} {unit}", bytes / scale);
        }
    }
    format!("{bytes} bytes")
}

/// A byte count **somebody measured**, as a reader of a report or a log line
/// sees it: the exact figure always, and a binary approximation beside it
/// once the exact figure stops being readable.
///
/// Not [`show_bytes`], and the asymmetry between the two is why they are in
/// one file. The test of which one a site wants is a single question: *did a
/// human type this figure, or did we measure it?*
///
/// - A figure **a human typed** reads back through [`show_bytes`], in the
///   unit family it was typed in: `200GB` as `200 GB` (D148, D152). That is
///   the only way an operator can tell the hub took the number they meant.
/// - A figure **we measured** reads back through this one. Nobody typed it,
///   so it belongs to no family: the exact count stays, and a binary
///   approximation rides beside it.
///
/// The asymmetry is deliberate and it has a bug behind it. `show_bytes`
/// walks `[GiB, GB, MiB, MB, KiB, kB]` and returns the first unit the figure
/// divides *exactly* — a divisibility branch and not a threshold. On a
/// measurement that makes the unit a property of the number: the first
/// consumer read `535691 kB` on one spool offer line out of seventy-five and
/// bytes on the other seventy-four, because that one total happened to be a
/// round multiple of 1000 and of nothing larger (ORCH-186). The figure was
/// right; the unit moved.
///
/// So: exactness first and never instead. Every number this prints is
/// evidence somebody will do arithmetic on, and a rounded one is how a
/// figure gets quoted back as the figure it was not. Below a kibibyte there
/// is nothing an approximation would make more readable, so there is none.
///
/// **One body, three callers.** The hub's state-root report
/// (`herd_hub::report`), the hub's retention preview (`herd_hub::retention`)
/// and the runner's spool and output lines (`herd_runner::size`) all print
/// this sentence. It was written out three times until ORCH-186's fold —
/// a rule written in three places is a rule fixed in two — and the two
/// copies in the hub were identical to the byte.
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

    #[test]
    fn plain_numbers_and_every_unit_parse() {
        assert_eq!(parse_bytes("0"), Ok(0));
        assert_eq!(parse_bytes("4096"), Ok(4096));
        assert_eq!(parse_bytes("4096B"), Ok(4096));
        assert_eq!(parse_bytes("1KiB"), Ok(1024));
        assert_eq!(parse_bytes("1kB"), Ok(1000));
        assert_eq!(parse_bytes("2GiB"), Ok(2 << 30));
        assert_eq!(parse_bytes(" 2 gib "), Ok(2 << 30));
        assert_eq!(parse_bytes("500MB"), Ok(500_000_000));
        assert_eq!(parse_bytes("1TiB"), Ok(1 << 40));
    }

    #[test]
    fn a_size_that_does_not_parse_names_itself() {
        let error = parse_bytes("plenty").expect_err("not a size");
        assert!(error.contains("plenty"), "{error}");
        let error = parse_bytes("2 bananas").expect_err("not a unit");
        assert!(error.contains("bananas"), "{error}");
        assert!(
            error.contains("GiB"),
            "the error must list the units: {error}"
        );
        let error = parse_bytes("").expect_err("empty");
        assert!(error.contains("empty"), "{error}");
    }

    /// A cap that silently wrapped would be a cap of a few bytes.
    #[test]
    fn a_size_too_large_to_hold_is_refused_not_wrapped() {
        let error = parse_bytes("99999999999999999999TiB").expect_err("far too large");
        assert!(error.contains("99999999999999999999"), "{error}");
    }

    #[test]
    fn a_shown_size_is_exact_or_stays_in_bytes() {
        assert_eq!(show_bytes(2 << 30), "2 GiB");
        assert_eq!(show_bytes(64 << 20), "64 MiB");
        assert_eq!(show_bytes(1024), "1 KiB");
        assert_eq!(show_bytes(1025), "1025 bytes");
        assert_eq!(show_bytes(0), "0 bytes");
    }

    /// The decimal family, which is what the hub's output budget is spelled
    /// in (D148, D152). A figure an operator typed as `200GB` has to read
    /// back as `200 GB`, or they cannot tell the hub took the number they
    /// meant.
    #[test]
    fn a_decimal_size_reads_back_in_the_family_it_was_written_in() {
        assert_eq!(
            show_bytes(parse_bytes("200GB").expect("a budget")),
            "200 GB"
        );
        assert_eq!(show_bytes(500_000_000), "500 MB");
        assert_eq!(show_bytes(1_000), "1 kB");
        // Binary first, so a figure that is both stays binary: 1 GiB is not
        // a multiple of a decimal gigabyte, and 2 kB is not a multiple of a
        // kibibyte, so neither can be read two ways.
        assert_eq!(show_bytes(1 << 30), "1 GiB");
        assert_eq!(show_bytes(2_000), "2 kB");
        // And a figure in neither family is still bytes rather than a lie
        // with one decimal place.
        assert_eq!(show_bytes(1_234_567), "1234567 bytes");
    }

    // --- the measured rule, and the line between it and the typed one ---

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
    /// readable, so there is none — the shape the hub's report has printed
    /// all along (`1136 bytes (1.11 KiB)` above it, `999 bytes` below).
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

    /// **The two rules on one figure, which is the whole reason both exist.**
    ///
    /// Folding the measured rule into this file put the two formatters an
    /// arm's length apart, and the next tidy will be tempted to make them
    /// one. These are the figures that make that red: each one reads
    /// differently under the two rules, and both readings are right for
    /// their own question.
    #[test]
    fn the_typed_rule_and_the_measured_rule_disagree_on_purpose() {
        for (bytes, typed, measured) in [
            // The consumer's figure: a round multiple of 1000 and of
            // nothing larger, which is how the unit moved.
            (535_691_000u64, "535691 kB", "535691000 bytes (510.87 MiB)"),
            // The hub's output budget, as Alexa typed it (D148, D152).
            (200_000_000_000, "200 GB", "200000000000 bytes (186.26 GiB)"),
            // A cap in the binary family: same family both ways, and still
            // not the same sentence.
            (64 << 20, "64 MiB", "67108864 bytes (64.00 MiB)"),
        ] {
            assert_eq!(show_bytes(bytes), typed, "the typed rule");
            assert_eq!(measured_bytes(bytes), measured, "the measured rule");
            assert_ne!(
                show_bytes(bytes),
                measured_bytes(bytes),
                "if these two ever agree on {bytes}, one of the rules was erased. They \
                 answer different questions: did a human type this figure, or did we \
                 measure it?"
            );
        }
    }

    /// And the typed rule, after the fold, still reads a typed cap back in
    /// its own family. Here with `parse_bytes` in front of it, which is the
    /// round trip an operator actually performs.
    #[test]
    fn a_typed_cap_still_reads_back_in_its_own_family() {
        assert_eq!(
            show_bytes(parse_bytes("200GB").expect("a budget")),
            "200 GB"
        );
        assert_eq!(show_bytes(parse_bytes("64MiB").expect("a cap")), "64 MiB");
        assert_eq!(show_bytes(parse_bytes("2GiB").expect("a cap")), "2 GiB");
    }
}
