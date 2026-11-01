//! `events.jsonl`'s nine kinds of line, frozen as literal text, and the
//! match that makes a tenth kind a compile error.
//!
//! # Why the file is a published surface, and who decided it
//!
//! D8, under *Reversibility*: *"The on-disk layout becomes a compatibility
//! surface the moment the consumer reads it; the record format therefore
//! carries a version from the start."* The condition is met — the first
//! consumer parses this file with their own code (E-53, E-54; D292) — so
//! [`crate::job`]'s first sentence calls these types a compatibility
//! surface and `docs/handover.md` lists `events.jsonl` among what crosses
//! the line. None of that was in question; what was missing was a detector.
//!
//! # Why literal text
//!
//! What is promised to a parser that is not written in Rust is the
//! **bytes**: the spelling of each `event` tag, every field name, the
//! `record_version` key in front of the line. A test that checked the shape
//! — a round trip, a `serde_json::Value` probe — stays green while a key is
//! renamed on both sides at once, and that is the one break the consumer
//! would feel. `herd-proto/tests/frozen_formats.rs` made this argument for
//! E-02's `ran on:` line and it holds unchanged here: *"A test that checked
//! the shape — a regex, a round trip — stays green while a space moves."*
//!
//! # Why a frozen fixture **alone** would not have been a detector
//!
//! The tree had already run that experiment and lost it.
//! `herd-hub/tests/announced_job_ceiling.rs` carried a hand-written list of
//! **eight** event kinds and was green on the morning after a **ninth**
//! landed, because nothing about the eight had changed. A fixture of
//! today's lines says nothing about kind ten. So this file is three things
//! at once, and it is the third that binds:
//!
//! 1. one byte-frozen line per kind, produced by the hub's own writer
//!    ([`JobDirStore::event_line`]) and not by a second serialiser written
//!    for the test;
//! 2. a **decode leg**: that same text, hand-written into a job's
//!    `events.jsonl` and read back through [`JobStore::job_events`], so each
//!    line is proven readable and not merely proven stable;
//! 3. [`frozen_line`] and [`frozen_state`], **exhaustive matches** that name
//!    every variant and every one of its fields with no `..` anywhere. A
//!    tenth variant, a field added to a variant and a field taken out of one
//!    are all three a **compile error in this file**. The compiler is the
//!    part that cannot be walked past, printed "ok" and forgotten.
//!
//! # Why it lives inside the crate
//!
//! [`EventLine`] — the type that puts [`RECORD_VERSION`] in front of every
//! line — is private to [`crate::store`], and so is the writer that uses it.
//! A test outside the crate can reach these bytes only through a hub that
//! wrote them, which means a runner, a clock and a job; this one reaches the
//! writer itself. Its own file for [`super::ordering_oracle`]'s reason: it
//! is a gate, not one more test of the store.

use super::*;

use crate::job::{JobEventKind, Mint};
use herd_proto::JobOutcome;

/// What to do when one of this file's assertions fires. Carried in every
/// failure message, because the wrong repair is the easy one and it is
/// silent: pasting the new text into the expectation converts a break the
/// consumer's parser would have shown us here into one they find in
/// production, and it does it while printing "ok".
///
/// There are exactly two honest repairs:
///
/// 1. **The change was not intended.** Revert it. These bytes are parsed
///    outside Herd, by code that is not built on our types (D292), and they
///    are not ours to reword in passing.
/// 2. **The change was intended, and it is additive.** A *new event kind* is
///    additive and leaves [`RECORD_VERSION`] alone — D292's ruling, on the
///    ground that a line's absence has always meant the thing did not
///    happen. Then the repair is three edits in this file and a sentence in
///    the release notice: a new frozen const, a new arm in [`frozen_line`],
///    a new entry in [`every_kind`], and [`HOW_MANY_KINDS`] moved by one.
///    Nothing in here is rewritten.
/// 3. **The change was intended and it is *not* additive** — a field
///    removed, a tag respelled, a field's meaning changed. That is a record
///    format change: [`RECORD_VERSION`] moves in the same commit, with
///    [`crate::LAYOUT_VERSION`] beside it (D218), and it needs Alexa's
///    approval and an operator instruction, because what stops being
///    readable is the consumer's whole state root.
///
/// Editing an expectation here and leaving the version at its old number is
/// none of the three, and is the one outcome this file exists to make
/// impossible to do quietly.
const WHEN_THIS_FIRES: &str = "\
this is a FROZEN published format: every line of events.jsonl is parsed by \
the first consumer with their own code (D8, E-53, E-54, D292). Either the \
change was unintended — revert it — or a NEW KIND was added, which is \
additive (D292): add a frozen const, an arm in frozen_line, an entry in \
every_kind, move HOW_MANY_KINDS by one, and name the kind in the release \
notice. Or a field was removed or a tag respelled, which is NOT additive: \
RECORD_VERSION moves in the same commit, with LAYOUT_VERSION beside it \
(D218), and that needs Alexa's approval and an operator instruction. \
Editing the expectation below to match new output, with the version left at \
its old number, is the one repair that is always wrong: it tells the \
consumer's parser it is reading a format it is not";

/// How many kinds of line `events.jsonl` has. Nine since `collection_short`
/// landed (D167). It is asserted against [`every_kind`] rather than trusted,
/// so the list and the number cannot drift apart silently.
const HOW_MANY_KINDS: usize = 9;

/// The one moment every frozen line below is stamped with, so that these
/// tests read the same on CI, on a laptop and twice on the same host. A
/// whole second, because [`Timestamp::at_unix_seconds`] takes seconds and a
/// nanosecond field would be the only variable part of the text.
const THE_MOMENT: i64 = 1_793_437_963;

/// That moment as the record spells it: RFC 3339, UTC, `Z` and not
/// `+00:00`. Part of the format and frozen with the rest of it.
const THE_MOMENT_AS_WRITTEN: &str = "2026-10-31T09:12:43Z";

fn at() -> Timestamp {
    Timestamp::at_unix_seconds(THE_MOMENT)
}

// ---------------------------------------------------------------------------
// The nine lines, whole
// ---------------------------------------------------------------------------
//
// Each const is one line of `events.jsonl` exactly as the hub writes it, with
// no trailing newline (`append_line` adds that). The `\` at the end of a
// fragment is a string continuation: Rust drops the newline and the next
// line's indentation, so what these literals hold is one unbroken line.

const TRANSITION_QUEUED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"transition\",\
\"state\":{\"type\":\"queued\"}}";

const TRANSITION_ASSIGNED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"transition\",\
\"state\":{\"type\":\"assigned\"}}";

const TRANSITION_RUNNING: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"transition\",\
\"state\":{\"type\":\"running\"}}";

const TRANSITION_INTERRUPTED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"transition\",\
\"state\":{\"type\":\"interrupted\",\"reason\":\"the hub stopped seeing this \
attempt\"}}";

const TRANSITION_FINISHED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"transition\",\
\"state\":{\"type\":\"finished\",\"outcome\":{\"type\":\"exited\",\"code\":0}}}";

const ATTEMPT_STARTED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\
\"event\":\"attempt_started\",\"attempt\":1,\"runner\":\"server\",\
\"why\":\"the only runner offering kvm\"}";

const ATTEMPT_ENDED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\
\"event\":\"attempt_ended\",\"attempt\":1,\"reason\":\"the runner went silent\"}";

const LOG_GAP: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"log_gap\",\
\"first_missing\":41,\"last_missing\":43}";

const JOINED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"joined\",\
\"submissions\":2,\"asked_for\":\"step\"}";

const PRIORITY_RAISED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\
\"event\":\"priority_raised\",\"from\":\"nightly\",\"to\":\"step\",\
\"why\":\"a submission that joined asked for more\"}";

const WAITING: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\"event\":\"waiting\",\
\"reason\":\"no runner offers kvm\"}";

const OUTPUTS_REMOVED: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\
\"event\":\"outputs_removed\",\"files\":17,\"bytes\":44040192,\
\"why\":\"removed 2026-10-31 under the 200 GB output budget\"}";

const COLLECTION_SHORT: &str = "\
{\"record_version\":2,\"at\":\"2026-10-31T09:12:43Z\",\
\"event\":\"collection_short\",\"files_here\":640,\"files_owed\":657,\
\"bytes_here\":42467328,\"bytes_owed\":86507520,\"runner\":\"server\",\
\"why\":\"the runner is gone and 17 files never arrived\"}";

// ---------------------------------------------------------------------------
// The matches that bind
// ---------------------------------------------------------------------------

/// The line each kind of event serialises to.
///
/// **This match is the detector.** Every arm names its variant's fields one
/// by one, and there is no `..` and no `_` arm anywhere in it, so three
/// different changes stop the build here rather than passing silently:
///
/// - a **tenth variant** of [`JobEventKind`] — the match is not exhaustive;
/// - a **field added** to a variant — the pattern does not mention it;
/// - a **field removed** from one — the pattern names a field that is gone.
///
/// Every one of those three changes what the consumer's parser reads, and
/// not one of them changes any existing line, which is why a fixture of
/// today's lines would have let all three through. Read [`WHEN_THIS_FIRES`]
/// before repairing anything here.
fn frozen_line(kind: &JobEventKind) -> &'static str {
    match kind {
        JobEventKind::Transition { state } => frozen_state(state),
        JobEventKind::AttemptStarted {
            attempt: _,
            runner: _,
            why: _,
        } => ATTEMPT_STARTED,
        JobEventKind::AttemptEnded {
            attempt: _,
            reason: _,
        } => ATTEMPT_ENDED,
        JobEventKind::LogGap {
            first_missing: _,
            last_missing: _,
        } => LOG_GAP,
        JobEventKind::Joined {
            submissions: _,
            asked_for: _,
        } => JOINED,
        JobEventKind::PriorityRaised {
            from: _,
            to: _,
            why: _,
        } => PRIORITY_RAISED,
        JobEventKind::Waiting { reason: _ } => WAITING,
        JobEventKind::OutputsRemoved {
            files: _,
            bytes: _,
            why: _,
        } => OUTPUTS_REMOVED,
        JobEventKind::CollectionShort {
            files_here: _,
            files_owed: _,
            bytes_here: _,
            bytes_owed: _,
            runner: _,
            why: _,
        } => COLLECTION_SHORT,
    }
}

/// The `transition` line each [`JobState`] serialises to, under the same rule
/// as [`frozen_line`] and for the same reason.
///
/// A state is as published as a kind is: `{"event":"transition"}` carries a
/// nested `state` object, so a sixth state — M9's cancellation was named in
/// advance — changes what the consumer reads just as a tenth kind would.
/// Exhaustive, every field named, no `..`.
///
/// It stops one level down. `JobState::Finished` carries a
/// [`JobOutcome`], which is `herd-proto`'s type and travels on the wire as
/// well as onto the disk; one outcome is frozen here, as the nesting's shape,
/// and the outcome's own variants are not enumerated. That is a gap and it is
/// named rather than papered over.
fn frozen_state(state: &JobState) -> &'static str {
    match state {
        JobState::Queued => TRANSITION_QUEUED,
        JobState::Assigned => TRANSITION_ASSIGNED,
        JobState::Running => TRANSITION_RUNNING,
        JobState::Interrupted { reason: _ } => TRANSITION_INTERRUPTED,
        JobState::Finished { outcome: _ } => TRANSITION_FINISHED,
    }
}

/// One event of every kind, in the order [`JobEventKind`] declares them.
///
/// The samples are what gets serialised; [`frozen_line`] is what says which
/// text each must come out as. Both are needed: the list without the match
/// is the whitelist that did not bind, and the match without the list is a
/// mapping nothing exercises.
fn every_kind() -> Vec<JobEvent> {
    vec![
        JobEvent::transition(at(), JobState::Queued),
        JobEvent::attempt_started(at(), 1, "server", "the only runner offering kvm"),
        JobEvent::attempt_ended(at(), 1, "the runner went silent"),
        JobEvent::log_gap(at(), 41, 43),
        JobEvent::joined(at(), 2, Priority::Step),
        JobEvent {
            at: at(),
            what: JobEventKind::PriorityRaised {
                from: Priority::Nightly,
                to: Priority::Step,
                why: "a submission that joined asked for more".to_owned(),
            },
        },
        JobEvent::waiting(at(), "no runner offers kvm".to_owned()),
        JobEvent::outputs_removed(
            at(),
            17,
            44_040_192,
            "removed 2026-10-31 under the 200 GB output budget",
        ),
        JobEvent::collection_short(
            at(),
            &crate::job::CollectionShortfall {
                files_here: 640,
                files_owed: 657,
                bytes_here: 42_467_328,
                bytes_owed: 86_507_520,
                runner: "server".to_owned(),
            },
            "the runner is gone and 17 files never arrived",
        ),
    ]
}

/// One `transition` event per state, in the order [`JobState`] declares them.
fn every_state() -> Vec<JobEvent> {
    vec![
        JobEvent::transition(at(), JobState::Queued),
        JobEvent::transition(at(), JobState::Assigned),
        JobEvent::transition(at(), JobState::Running),
        JobEvent::transition(
            at(),
            JobState::Interrupted {
                reason: "the hub stopped seeing this attempt".to_owned(),
            },
        ),
        JobEvent::transition(
            at(),
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        ),
    ]
}

/// A store in a directory that goes away with the test, for the one thing
/// only the crate can reach: the writer that puts [`RECORD_VERSION`] in
/// front of a line.
fn a_store() -> (tempfile::TempDir, JobDirStore) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    (dir, store)
}

/// The line the hub would write for this event, through the hub's own
/// serialiser and no other.
fn written_by_the_hub(store: &JobDirStore, event: &JobEvent) -> String {
    store
        .event_line(Path::new("events.jsonl"), event)
        .expect("an event serialises")
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// Every kind of line, byte for byte, as the hub writes it.
#[test]
fn every_kind_of_line_is_this_text_and_no_other() {
    let (_dir, store) = a_store();
    for event in every_kind() {
        let written = written_by_the_hub(&store, &event);
        assert_eq!(
            written,
            frozen_line(&event.what),
            "a line of events.jsonl changed shape. {WHEN_THIS_FIRES}",
        );
        println!("{written}");
    }
}

/// Every state a `transition` line can carry, byte for byte.
#[test]
fn every_transition_state_is_this_text_and_no_other() {
    let (_dir, store) = a_store();
    for event in every_state() {
        let JobEventKind::Transition { state } = &event.what else {
            panic!("every_state builds transitions and nothing else");
        };
        let written = written_by_the_hub(&store, &event);
        assert_eq!(
            written,
            frozen_state(state),
            "a transition line's state object changed shape. {WHEN_THIS_FIRES}",
        );
        println!("{written}");
    }
}

/// The nine kinds are nine, they are all different, and the list and the
/// match agree about which they are.
///
/// What this catches that the two tests above do not: a kind added to
/// [`every_kind`] and pointed at an existing const, or a kind quietly
/// dropped from the list. The arm count is the compiler's business; the
/// list's is here.
#[test]
fn there_are_nine_kinds_and_the_list_holds_all_of_them() {
    let tags: BTreeSet<String> = every_kind()
        .iter()
        .map(|event| {
            let line: serde_json::Value =
                serde_json::from_str(frozen_line(&event.what)).expect("a frozen line is json");
            line["event"]
                .as_str()
                .expect("every line names its kind")
                .to_owned()
        })
        .collect();

    assert_eq!(
        tags.len(),
        HOW_MANY_KINDS,
        "every_kind must hold one event of each of the {HOW_MANY_KINDS} kinds, with no kind \
         twice and none missing; it holds {tags:?}. {WHEN_THIS_FIRES}"
    );
    assert_eq!(
        every_kind().len(),
        HOW_MANY_KINDS,
        "the list is longer than the set of kinds it covers, so two of its entries are the \
         same kind. {WHEN_THIS_FIRES}"
    );
    println!("the nine kinds: {tags:?}");
}

/// The version in front of every line, named here on purpose.
///
/// The frozen text above carries `"record_version":2` nine times over, so a
/// move of [`RECORD_VERSION`] turns every assertion in this file red. That
/// is correct and it is intended — a record format change is exactly what
/// those assertions are for — and this test is the one that says so in one
/// sentence rather than leaving a reader to infer it from nine diffs.
#[test]
fn every_line_carries_the_record_version_in_front_of_it() {
    assert_eq!(
        RECORD_VERSION, 2,
        "RECORD_VERSION moved, so every frozen line in this file is now wrong — which is the \
         point of them. Re-derive all of them against the new number in the same commit, and \
         check that crate::LAYOUT_VERSION moved with it (D218). {WHEN_THIS_FIRES}"
    );

    for event in every_kind().into_iter().chain(every_state()) {
        let line = frozen_line(&event.what);
        assert!(
            line.starts_with("{\"record_version\":2,\"at\":"),
            "a line that does not start with its version is a line `tail -f` cannot read on \
             its own: {line}. {WHEN_THIS_FIRES}"
        );
        assert!(
            line.contains(THE_MOMENT_AS_WRITTEN),
            "the moment is part of the format: RFC 3339 in UTC, `Z` and not `+00:00`. \
             {line}. {WHEN_THIS_FIRES}"
        );
    }
}

/// The decode leg: the frozen text, hand-written to a job's `events.jsonl`
/// and read back through the reader the hub itself uses.
///
/// This is the half a frozen expectation cannot do on its own. The text
/// above proves the hub still *writes* these bytes; this proves the bytes
/// are still *readable*, and that what comes back out of them is the event
/// that went in — so a field renamed on both sides at once, which a round
/// trip would smile at, fires on the frozen text while a line the reader has
/// stopped accepting fires here.
#[test]
fn the_frozen_lines_read_back_as_the_events_they_were_written_from() {
    let (_dir, store) = a_store();
    let id = store
        .create_job(NewJob::new(vec!["a-command".to_owned()]))
        .expect("a job is minted")
        .id;

    let written: Vec<JobEvent> = every_kind().into_iter().chain(every_state()).collect();
    let text: String = written
        .iter()
        .map(|event| format!("{}\n", frozen_line(&event.what)))
        .collect();
    let path = store.job_dir(&id).join(EVENTS_FILE);
    fs::write(&path, &text).expect("the history is written by hand");

    let read = store
        .job_events(&id)
        .expect("a hand-written history is readable")
        .expect("the job exists");

    assert_eq!(
        read, written,
        "the frozen lines no longer read back as the events they were written from, so this \
         hub cannot read a history it would itself have written. {WHEN_THIS_FIRES}"
    );
    println!(
        "{} frozen lines written to {} and read back whole",
        read.len(),
        path.display()
    );
}
