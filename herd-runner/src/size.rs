//! Byte sizes in configuration.
//!
//! Every cap a runner enforces is a number of bytes, and a number of bytes is
//! unreadable past a few megabytes. So the configuration accepts a plain
//! integer or an integer with a unit suffix, and nothing else: a string and a
//! number, never an expression (invariant 1).

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
pub fn show_bytes(bytes: u64) -> String {
    for (unit, scale) in [("GiB", 1u64 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)] {
        if bytes >= scale && bytes.is_multiple_of(scale) {
            return format!("{} {unit}", bytes / scale);
        }
    }
    format!("{bytes} bytes")
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
}
