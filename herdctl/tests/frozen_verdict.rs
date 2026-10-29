//! The machine-readable verdict `herdctl --verdict-to` writes, frozen as
//! **literal text**: the whole document, byte for byte, including the order
//! of its keys, the two-space indentation, the `null`s and the final newline.
//!
//! Why literal text and not a shape. This document exists because parsing
//! prose was the thing we were asking of the consumer's agent and it was one
//! rewording away from reading its fleet wrong (D166). Replacing a reworded
//! sentence with a reworded *document* would be no improvement, so after
//! v1.0.0 these bytes are a commitment. A test that checked the shape — the
//! key set, a round trip, `json["passed"] == true` — stays green while
//! `exit_code_from` is renamed, while the pretty printer is swapped for a
//! compact one, while the first field stops being first, and while the
//! trailing newline disappears. Every one of those breaks a reader that
//! `grep`s, `jq -r`s, diffs two verdicts, or concatenates them into a log.
//! So this asserts the file's whole contents.
//!
//! **Where the version sits is part of the freeze.** The document's first
//! field is `verdict_format_version`, so a reader can refuse a version it has
//! never heard of before it reads anything else — which only works if it is
//! written first, and `serde_json::Value` sorts keys and would hide a change
//! to that. Golden text does not.
//!
//! **Nothing here depends on a machine.** Every variable part of a verdict is
//! an input to the functions under test and is supplied by name below: the
//! job id, the runner's name and the hub's reason, the declared and ran
//! commits, the cap's figures, the sentences `herdctl` said, and all of
//! `metrics` — which is the one part that is genuinely a measurement, and so
//! is given fixed figures here rather than measured. The only path this test
//! touches is a `tempfile` directory whose name never reaches an assertion.

use herd_proto::{Cap, CommitRun, JobId, JobMetrics, JobOutcome, JobOutputs, Pressure};
use herdctl::verdict::{JobExists, Verdict};
use herdctl::{Finished, RanOn};

/// What to do when one of this file's assertions fires. Carried in every
/// failure message, because the wrong repair is the easy one and it is
/// silent: a frozen format whose test is made green again by pasting the new
/// document into the expectation is worse than no test at all — it converts a
/// break a consumer would have seen here into one they find in their
/// wrapper, and it does it while printing "ok".
///
/// There are exactly two honest repairs:
///
/// 1. **The change was not intended.** Revert it. A program reads this
///    document; a field that moved or a `null` that became an absence is a
///    reader's `KeyError`, whatever it looks like on a screen.
/// 2. **The change was intended.** Then read what moves the version
///    ([`herd_proto::VERDICT_FORMAT_VERSION`]): *removing* a field or
///    changing what one means moves it, and **adding** a field does not,
///    because every reader of JSON already ignores what it was not looking
///    for. So an added field is an expectation updated here and the version
///    left alone — deliberately, with the new field documented — and
///    anything else is a version bump in the same commit, written down in
///    `docs/decisions.md` and in the version table in `docs/architecture.md`.
///
/// What is never right is reaching for this file first. If the diff below is
/// a key you did not mean to rename, a `null` that vanished, or the whole
/// document on one line, the code is what is wrong.
const WHEN_THIS_FIRES: &str = "\
this is a FROZEN public format (D166). Either the change was unintended — \
revert it, a program reads these bytes — or it was intended: adding a field \
leaves VERDICT_FORMAT_VERSION alone and is updated here on purpose, while \
removing a field or changing what one means moves the version in the same \
commit and is written down in docs/decisions.md. Pasting new output over \
this expectation to get back to green, with the version left at its old \
number, is the one repair that is always wrong";

/// Forty hex, spelled out so that the expected document below is literal
/// text and not a value this test computed.
const DECLARED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RAN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// What a verdict is written through, so that the pretty printing and the
/// final newline are frozen with the document and not just the serializer's
/// output.
fn written(verdict: &Verdict) -> String {
    let dir = tempfile::tempdir().expect("a directory to write the verdict into");
    let path = dir.path().join("verdict.json");
    verdict
        .write_to(&path)
        .expect("the verdict is written where it was asked for");
    std::fs::read_to_string(&path).expect("and read back, bytes and all")
}

/// A job that ended on a machine, with **every** field of the document
/// carrying something: the cap's figures, both commits and their
/// disagreement, a sentence, and the full set of metrics down to the
/// per-resource pressure. The one absence is `job_memory_milli`, which is
/// there on purpose: the nested protocol types keep the protocol's own habit
/// of leaving a figure nobody measured *out* rather than writing a zero that
/// reads as a measurement (D23), and that difference between the document's
/// own `null`s and the protocol's absences is itself part of the format.
fn a_job_that_was_capped() -> Finished {
    Finished {
        job_id: JobId::new("j1197"),
        outcome: JobOutcome::StoppedByCap {
            cap: Cap::Disk,
            limit: 21_474_836_480,
            detail: Some("the workspace could not be measured".to_owned()),
        },
        metrics: JobMetrics {
            host_load_milli: Some(43_560),
            peak_memory_bytes: Some(1_073_741_824),
            bytes_written: Some(4_096),
            log_bytes: 12,
            wall_time_ms: 1_500,
            pressure: Some(Pressure {
                host_cpu_milli: Some(1_200),
                host_memory_milli: Some(0),
                host_io_milli: Some(3_400),
                job_cpu_milli: Some(5_600),
                job_memory_milli: None,
                job_io_milli: Some(7_800),
            }),
        },
        outputs: JobOutputs::default(),
        ran_on: Some(RanOn {
            runner: "server".to_owned(),
            why: "the only runner offering kvm".to_owned(),
        }),
        commit: Some(CommitRun {
            declared: DECLARED.to_owned(),
            ran: Some(RAN.to_owned()),
        }),
    }
}

/// The document with everything in it, whole.
#[test]
fn a_full_verdict_is_these_bytes_and_no_others() {
    let verdict = Verdict::of(
        &a_job_that_was_capped(),
        vec!["the runner stopped this job".to_owned()],
        124,
    );
    assert_eq!(
        written(&verdict),
        r#"{
  "verdict_format_version": 1,
  "job_id": "j1197",
  "cause": "stopped_by_cap",
  "passed": false,
  "exit_code": 124,
  "exit_code_from": "herd",
  "job_exists": "a_job",
  "ran_on": {
    "runner": "server",
    "why": "the only runner offering kvm"
  },
  "cap": {
    "cap": "disk",
    "limit": 21474836480,
    "unit": "bytes",
    "detail": "the workspace could not be measured"
  },
  "said": [
    "the runner stopped this job"
  ],
  "declared_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "ran_commit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "commit_diverged": true,
  "outcome": {
    "type": "stopped_by_cap",
    "cap": "disk",
    "limit": 21474836480,
    "detail": "the workspace could not be measured"
  },
  "metrics": {
    "host_load_milli": 43560,
    "peak_memory_bytes": 1073741824,
    "bytes_written": 4096,
    "log_bytes": 12,
    "wall_time_ms": 1500,
    "pressure": {
      "host_cpu_milli": 1200,
      "host_memory_milli": 0,
      "host_io_milli": 3400,
      "job_cpu_milli": 5600,
      "job_io_milli": 7800
    }
  }
}
"#,
        "{WHEN_THIS_FIRES}"
    );
}

/// The document a passing job leaves, which is the one a consumer's wrapper
/// reads most often and the one whose `null`s matter most: four of them, each
/// a question this run could not answer, every key present so that a reader
/// indexes the document rather than probing it.
#[test]
fn the_verdict_of_a_job_that_passed_is_these_bytes_and_no_others() {
    let passed = Finished {
        job_id: JobId::new("j1197"),
        outcome: JobOutcome::Exited { code: 0 },
        metrics: JobMetrics {
            log_bytes: 12,
            wall_time_ms: 1_500,
            ..JobMetrics::default()
        },
        outputs: JobOutputs::default(),
        ran_on: Some(RanOn {
            runner: "server".to_owned(),
            why: "the only runner offering kvm".to_owned(),
        }),
        commit: None,
    };
    assert_eq!(
        written(&Verdict::of(&passed, Vec::new(), 0)),
        r#"{
  "verdict_format_version": 1,
  "job_id": "j1197",
  "cause": "exited",
  "passed": true,
  "exit_code": 0,
  "exit_code_from": "job",
  "job_exists": "a_job",
  "ran_on": {
    "runner": "server",
    "why": "the only runner offering kvm"
  },
  "cap": null,
  "said": [],
  "declared_commit": null,
  "ran_commit": null,
  "commit_diverged": null,
  "outcome": {
    "type": "exited",
    "code": 0
  },
  "metrics": {
    "log_bytes": 12,
    "wall_time_ms": 1500
  }
}
"#,
        "{WHEN_THIS_FIRES}"
    );
}

/// The document a run that never got an outcome leaves — the hub unreachable,
/// the submission refused, the link gone mid-job. Frozen too, because it is
/// written rather than skipped precisely so that a wrapper cannot act on
/// yesterday's answer, and a wrapper that reads it has to be able to tell
/// `"cause": "no_verdict"` from any of the protocol's own eight endings.
#[test]
fn a_run_with_no_verdict_at_all_is_these_bytes_and_no_others() {
    let none = Verdict::none(
        None,
        JobExists::Nothing,
        "the hub could not be reached".to_owned(),
    );
    assert_eq!(
        written(&none),
        r#"{
  "verdict_format_version": 1,
  "job_id": null,
  "cause": "no_verdict",
  "passed": false,
  "exit_code": 125,
  "exit_code_from": "herd",
  "job_exists": "nothing",
  "ran_on": null,
  "cap": null,
  "said": [
    "the hub could not be reached"
  ],
  "declared_commit": null,
  "ran_commit": null,
  "commit_diverged": null,
  "outcome": null,
  "metrics": null
}
"#,
        "{WHEN_THIS_FIRES}"
    );
}

/// The version number itself, which this file freezes and does not move.
///
/// Asserted apart from the documents above so that a bump cannot slip
/// through as "one of the numbers in the golden text": the constant is the
/// thing a consumer's parser keys on, and 1 is what it is.
#[test]
fn the_verdict_format_version_is_1() {
    assert_eq!(herd_proto::VERDICT_FORMAT_VERSION, 1, "{WHEN_THIS_FIRES}");
}
