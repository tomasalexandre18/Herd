//! Byte sizes — both rules, kept in `herd-proto`, named here.
//!
//! The parser and the two formatters all live in [`herd_proto::size`]. Two
//! copies of a unit table is how `500MB` comes to mean two different numbers
//! on two sides of one fleet, and a budget an operator sets on the hub and
//! reads back in a runner's refusal has to be the same number both times.
//!
//! This module stays so that `herd_runner::size::parse_bytes` and its
//! neighbours go on meaning what they meant: the runner's own caps are still
//! spelled the same way on its command line, and its measured lines still
//! call [`measured_bytes`] by that name.
//!
//! **Two rules, and picking the wrong one is a defect the first consumer
//! read** (ORCH-186). [`show_bytes`] is for a figure a human *typed* and
//! returns it in the family they typed it in; [`measured_bytes`] is for a
//! figure this runner *measured* and says it exactly, with a binary
//! approximation beside it. [`measured_bytes`] was written here first,
//! because the rule was needed here first; it moved into `herd-proto`
//! beside `show_bytes` once the hub's two identical copies of it could be
//! folded in with it. The rationale, the asymmetry and the golden figures
//! are all in [`herd_proto::size`], which is the one place a reader looking
//! for either rule would look.

pub use herd_proto::size::{measured_bytes, parse_bytes, show_bytes};

#[cfg(test)]
mod tests {
    use super::*;

    /// This module's job, after the fold, is to carry the names: the
    /// runner's `sweep.rs` and `outputs.rs` call `measured_bytes` through
    /// here, and a re-export that went to the wrong function would print a
    /// measurement under the typed rule again.
    ///
    /// One figure each, and the consumer's own. The class of figures and the
    /// rules' asymmetry are pinned where the bodies are, in
    /// `herd-proto/src/size.rs` — not a second time here, because a rule
    /// written in two places is a rule fixed in one.
    #[test]
    fn the_re_exports_name_the_two_rules_and_not_one_rule_twice() {
        assert_eq!(measured_bytes(535_691_000), "535691000 bytes (510.87 MiB)");
        assert_eq!(show_bytes(535_691_000), "535691 kB");
        assert_eq!(parse_bytes("64MiB"), Ok(64 << 20));
    }
}
