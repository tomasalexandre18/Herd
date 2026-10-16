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

pub use herd_proto::size::{parse_bytes, show_bytes};
