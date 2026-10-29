//! The two public formats that live in this crate, frozen as **literal
//! text**: E-02's `ran on:` line, and the [`herd_proto::OUTPUT_FORMAT_VERSION`]
//! it carries.
//!
//! Why literal text and not a shape. Both are promised stable to anyone
//! scripting against Herd, and until now the promise was a sentence in a doc
//! comment with no machine behind it. A test that checked the *shape* — a
//! regex, a round trip through [`herd_proto::ran_on_parts`] — stays green
//! while a space moves, a word changes or the separator before the version
//! turns from `;` into `,`, and every one of those breaks a consumer's `sed`
//! just as thoroughly as dropping a field would. So these assert the whole
//! string, byte for byte.
//!
//! **Nothing here depends on a machine.** The runner's name and the hub's
//! reason are the line's only variable parts and both are arguments, so these
//! tests read the same on CI, on a laptop and twice on the same host.

use herd_proto::{ran_on_line, ran_on_parts, OUTPUT_FORMAT_VERSION, RAN_ON_PREFIX};

/// What to do when one of this file's assertions fires. Carried in every
/// failure message, because the wrong repair is the easy one and it is
/// silent: a frozen format whose test is made green again by pasting the new
/// text into the expectation is worse than no test at all — it converts a
/// break a consumer would have seen here into one they find in production,
/// and it does it while printing "ok".
///
/// There are exactly two honest repairs:
///
/// 1. **The change was not intended.** Revert it. This line is read by the
///    first consumer's tooling (E-02) and by anyone who scripted against
///    `herdctl`; it is not ours to reword in passing.
/// 2. **The change was intended.** Then it is a format change, and
///    [`OUTPUT_FORMAT_VERSION`] moves with it in the same commit, which is
///    what tells a reader's parser which framing it is holding (Q7, D73).
///    Say so in `docs/decisions.md` and in the version table in
///    `docs/architecture.md`.
///
/// Updating the expected text and leaving the version alone is neither, and
/// is the one outcome this file exists to make impossible to do quietly.
const WHEN_THIS_FIRES: &str = "\
this is a FROZEN public format (E-02, Q7). Either the change was unintended \
— revert it, a requester's parser reads these bytes — or it was intended, in \
which case OUTPUT_FORMAT_VERSION moves in the same commit and the change is \
written down in docs/decisions.md. Editing this expectation to match new \
output, with the version left at its old number, is the one repair that is \
always wrong: it tells every consumer's parser it is reading a format it is \
not";

/// E-02's first line, whole.
///
/// The two variable parts are named and supplied here: `<runner>` is the
/// runner's own name as the hub knows it, and `<why>` is the hub's free-text
/// reason for that machine. Everything else in this string — the prefix and
/// its single space, the space before the parenthesis, the `; herd output
/// format N` that rides *inside* the parentheses (D109), the closing
/// parenthesis with nothing after it — is the format.
#[test]
fn the_ran_on_line_is_this_text_and_no_other() {
    assert_eq!(
        ran_on_line("server", "the only runner offering kvm"),
        "ran on: server (the only runner offering kvm; herd output format 1)",
        "{WHEN_THIS_FIRES}"
    );

    // A second line with other variable parts, so that what is pinned above
    // is the format and not one lucky pair of arguments. A `why` that is
    // itself several clauses — which every real one is — changes nothing
    // about where the version sits or how it is introduced.
    assert_eq!(
        ran_on_line(
            "big-box",
            "claimed by big-box: labels none; it needs no capability; preference 0"
        ),
        "ran on: big-box (claimed by big-box: labels none; it needs no capability; \
         preference 0; herd output format 1)",
        "{WHEN_THIS_FIRES}"
    );
}

/// The prefix a reader anchors on, frozen too: it is published as
/// [`RAN_ON_PREFIX`] precisely so that a consumer can key on it, which makes
/// its eight bytes part of the promise.
#[test]
fn the_prefix_a_reader_anchors_on_is_this_text() {
    assert_eq!(RAN_ON_PREFIX, "ran on: ", "{WHEN_THIS_FIRES}");
    assert!(
        ran_on_line("server", "why").starts_with(RAN_ON_PREFIX),
        "{WHEN_THIS_FIRES}"
    );
}

/// The reader side, pinned against the **frozen literal** rather than
/// against whatever the formatter happens to produce.
///
/// Asserting `ran_on_parts(ran_on_line(..))` would be a round trip, and a
/// round trip stays green when both halves move together — which is exactly
/// the change that breaks every consumer who wrote their own reader to
/// `^ran on: (\S+) \((.*)\)$` (D109). So the input here is the literal text
/// a consumer is holding, and the split of it is frozen with it: the runner
/// is one word, and the reason keeps the version inside it.
#[test]
fn the_frozen_line_splits_into_these_two_parts() {
    let frozen = "ran on: server (the only runner offering kvm; herd output format 1)";
    assert_eq!(
        ran_on_parts(frozen),
        Some((
            "server",
            "the only runner offering kvm; herd output format 1"
        )),
        "{WHEN_THIS_FIRES}"
    );
}

/// The version number itself, and the bytes a requester actually reads it as.
///
/// The constant is **1** and this task moves nothing; the assertion is here
/// so that a bump cannot happen by itself, unaccompanied by the decision
/// entry that tells a consumer what changed. Its only printed surface in the
/// whole of Herd is the line above, so it is frozen in both places: as the
/// number a reader of the crate sees, and as the eight-and-a-bit bytes a
/// reader of stdout sees.
#[test]
fn the_output_format_version_is_1_and_prints_as_this_suffix() {
    assert_eq!(OUTPUT_FORMAT_VERSION, 1, "{WHEN_THIS_FIRES}");
    assert!(
        ran_on_line("server", "why").ends_with("; herd output format 1)"),
        "{WHEN_THIS_FIRES}"
    );
    // And the number in the line really is the constant, so that the two
    // cannot drift into disagreeing about what they both claim to be.
    assert!(
        ran_on_line("server", "why")
            .ends_with(&format!("; herd output format {OUTPUT_FORMAT_VERSION})")),
        "{WHEN_THIS_FIRES}"
    );
}
