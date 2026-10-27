//! A job's end, written for a reader that is a program (D166).
//!
//! Everything `herdctl` says about how a job ended is prose on stderr. That
//! was a deliberate choice and it is still the right one for a person at a
//! terminal: an exit code cannot tell a cancelled job from a lost one, and
//! D109 made the two sentences a requester most needs — the `ran on:` line and
//! the three fallback lines — stable enough to parse. But *parsing prose* is
//! what we were asking of the agent that submits these jobs, and it is the
//! complaint behind the consumer's grievances 9, 10a and 10c and point 3.2 of
//! the scope review. An agent that must `sed` a sentence to learn which
//! machine ran its build is an agent one rewording away from reading its
//! fleet wrong.
//!
//! So `herdctl run --verdict-to <FILE>` and `herdctl wait --verdict-to <FILE>`
//! write this document instead, and the prose stays exactly as it was.
//!
//! **Three questions it answers that no exit code can.** Which of the eight
//! ways a job can end this was ([`Verdict::cause`]); whether the number
//! `herdctl` exited with is the job's own or Herd's
//! ([`Verdict::exit_code_from`]) — because a job that itself exits 124 or 125
//! wears that code untouched and is otherwise indistinguishable from a cap or
//! from Herd having no verdict; and whether a job of this requester's exists
//! at all ([`Verdict::job_exists`]), which is D109's stderr contract made
//! machine-readable.
//!
//! **Every one of this document's own fields is always there.** A field with
//! nothing in it is `null`, never absent, so a reader can index the document
//! rather than probe it. (What is nested inside `outcome` and `metrics` is
//! the protocol's own shape and keeps the protocol's own habits, where a
//! figure nobody measured is absent rather than a zero that reads as one.)
//! Adding a field therefore does not move
//! [`herd_proto::VERDICT_FORMAT_VERSION`]; removing one, or changing what one
//! means, does.

use std::path::Path;

use anyhow::{Context, Result};
use herd_proto::{JobMetrics, JobOutcome, VERDICT_FORMAT_VERSION};
use serde::Serialize;

use crate::{Finished, RanOn};

/// What ends a run that never got an outcome at all: the hub could not be
/// reached, it refused the submission, the link went while the job was still
/// running.
///
/// Not one of [`JobOutcome`]'s own words, and it cannot collide with one:
/// those are the variants' serde tags and none of them is this.
pub const NO_VERDICT_CAUSE: &str = "no_verdict";

/// A job's end, as a program reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// The format this document is in, first so that a reader can refuse a
    /// version it has never heard of before it reads anything else.
    pub verdict_format_version: u32,
    /// The job's id, or `null` when no job of this requester's exists.
    pub job_id: Option<String>,
    /// What ended the job, in one word: the [`JobOutcome`] variant's own serde
    /// tag ([`JobOutcome::cause`]), or [`NO_VERDICT_CAUSE`].
    ///
    /// **This is grievance 9 in one field**: every verdict names its cause,
    /// and the word is the protocol's own rather than a second vocabulary
    /// that could drift from it.
    pub cause: String,
    /// Whether the job **passed**: it ran and exited nought
    /// ([`JobOutcome::is_success`]). Nothing else is a pass, and the four
    /// different failures behind a `false` are what `cause` is for.
    pub passed: bool,
    /// The code `herdctl` exited this invocation with.
    pub exit_code: u8,
    /// Whose code that is.
    pub exit_code_from: ExitCodeFrom,
    /// Whether a job of this requester's exists, which is the one question
    /// 125 cannot answer and the whole reason D109's stderr lines exist.
    pub job_exists: JobExists,
    /// The runner the job ran on and why there — grievances 10a and 10c.
    ///
    /// `null` only when the hub never placed the job: it was refused, or no
    /// machine could ever have run it. Present even for a job that reached a
    /// machine and failed to start there, where the stdout framing line is
    /// deliberately withheld (D16) and the machine's name is the one thing
    /// worth having.
    pub ran_on: Option<RanOn>,
    /// The cap's figures, when a cap ended the job.
    pub cap: Option<CapReport>,
    /// The sentences `herdctl` printed on stderr about this ending, in the
    /// order it printed them. Empty for an ending that needs none.
    ///
    /// Here so that a wrapper which logs one thing logs the same thing a
    /// person would have read. It is prose: **nothing is meant to match on
    /// it**, which is what the fields above are for.
    pub said: Vec<String>,
    /// The commit this job was submitted for, forty hex, or `null` for a job
    /// that declared none (D176).
    pub declared_commit: Option<String>,
    /// The revision the machine says it really ran, or `null` for **"nobody
    /// said"**.
    ///
    /// `null` is not agreement and must not be read as any. It is what a
    /// consumer whose `fetch` hook does not write `{ran}` gets, what a job
    /// that never reached a machine gets, and what a hub predating the field
    /// gets. [`Verdict::commit_diverged`] is the field to branch on.
    pub ran_commit: Option<String>,
    /// Whether the two above contradict each other: `true` for a verdict
    /// attributed to a commit the tree was **not** at, `false` for two
    /// strings that agree, `null` for a question nobody can answer because
    /// one of the two is missing.
    ///
    /// **`true` does not make the job a failure.** The exit code is the
    /// job's and a side channel does not become one (D20): what the three
    /// fields give a wrapper is the first thing on a Herd verdict that can
    /// contradict its own commit, and what to do about that is the
    /// wrapper's to decide until Herd is told otherwise.
    pub commit_diverged: Option<bool>,
    /// The outcome exactly as the protocol carries it, tag and all, for a
    /// reader that wants the whole thing rather than the summary above.
    /// `null` when Herd never got one.
    pub outcome: Option<JobOutcome>,
    /// What the runner measured while the job ran (E-15). `null` when no job
    /// ran to report anything.
    pub metrics: Option<JobMetrics>,
}

/// Whose number [`Verdict::exit_code`] is.
///
/// **The field the ambiguity in `herdctl --help`'s exit-code table needed.**
/// A job's own exit of 124 or 125 is worn untouched, so by the code alone it
/// is indistinguishable from a cap and from Herd having no verdict. With this
/// beside it the three are three: `{"exit_code": 124, "exit_code_from":
/// "job"}` is a build that chose to exit 124, and `{"exit_code": 124,
/// "exit_code_from": "herd"}` is a cap, with `cause` naming which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitCodeFrom {
    /// The process's own exit code, worn untouched — 0, 1, 75, 124, 125 and
    /// anything else it really exited with (E-02).
    Job,
    /// `128 + n` for the signal `n` that killed the process, exactly as a
    /// shell reports one. The signal itself is in `outcome`.
    Signal,
    /// Herd's own: 124 for a cap, 125 for no verdict. The job's code, if it
    /// ever had one, is not in this number.
    Herd,
}

/// Whether a job of this requester's exists — D109's three stderr lines, as a
/// field.
///
/// A requester that falls back has to know this and the exit code cannot say:
/// re-running a job that exists does the work twice, and declining to re-run
/// a submission that never left does it never.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobExists {
    /// A job exists and `job_id` is its id. Do not submit again; `herdctl
    /// wait <id>` picks it back up.
    AJob,
    /// Nothing was submitted, so running the work again is safe and starts no
    /// second job.
    Nothing,
    /// The submission left and was never acknowledged, so a job may exist
    /// whose id nobody learned. Submitting the same work again **joins** it
    /// rather than starting a second one (E-05).
    Unknown,
}

/// Which cap ended the job, and its figures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapReport {
    /// `time`, `memory`, `log` or `disk`, in the protocol's own words.
    pub cap: String,
    /// The figure, in `unit`.
    pub limit: u64,
    /// `s` for the time cap, `bytes` for the other three.
    pub unit: String,
    /// The cap's own reason, when its figure alone would mislead (D30).
    ///
    /// **A non-null `detail` means the job did not reach `limit`.** The case
    /// it exists for is a job of four kilobytes stopped by a disk budget of
    /// twenty gigabytes, because the budget could not be *enforced* at all —
    /// and a reader that showed the figure as a measurement would be showing
    /// a true number inside a false sentence.
    pub detail: Option<String>,
}

impl Verdict {
    /// The document for a job that ended, however it ended.
    pub fn of(finished: &Finished, said: Vec<String>, exit_code: u8) -> Self {
        Self {
            verdict_format_version: VERDICT_FORMAT_VERSION,
            job_id: Some(finished.job_id.to_string()),
            cause: finished.outcome.cause().to_owned(),
            passed: finished.outcome.is_success(),
            exit_code,
            exit_code_from: ExitCodeFrom::of(&finished.outcome),
            // A job that ended is a job that existed, whatever this
            // invocation did to make it: `wait` was handed its id and `run`
            // was told it by the hub.
            job_exists: JobExists::AJob,
            ran_on: finished.ran_on.clone(),
            cap: CapReport::of(&finished.outcome),
            said,
            declared_commit: finished
                .commit
                .as_ref()
                .map(|commit| commit.declared.clone()),
            ran_commit: finished
                .commit
                .as_ref()
                .and_then(|commit| commit.ran.clone()),
            // Three states in one field, and the third is the honest one:
            // a comparison nobody can make is `null`, never `false` (D23's
            // rule, applied to a boolean).
            commit_diverged: finished
                .commit
                .as_ref()
                .filter(|commit| commit.ran.is_some())
                .map(herd_proto::CommitRun::diverged),
            outcome: Some(finished.outcome.clone()),
            metrics: Some(finished.metrics.clone()),
        }
    }

    /// The same document, saying that [`Verdict::exit_code`] is **Herd's own
    /// number and not the job's** (D167).
    ///
    /// For the one case where a job that ended perfectly well does not leave
    /// this invocation exiting 0: `--outputs-to` was given and the collection
    /// came back short, so `herdctl` exits 2 — *a command did part of what it
    /// says*. [`ExitCodeFrom::of`] reads the code's origin off the outcome,
    /// and the outcome says `exited 0`, so without this the document would
    /// label a 2 as the job's own and a reader would go looking for a build
    /// that exited 2. There was none.
    ///
    /// [`Verdict::passed`] is deliberately **not** touched. The job passed:
    /// that is the verdict, it is the record's, and a side channel does not
    /// become one (D20). What says the run was incomplete is the code, this
    /// field beside it, and the sentence in [`Verdict::said`] naming every
    /// file that is missing.
    pub fn with_herd_s_own_code(mut self) -> Self {
        self.exit_code_from = ExitCodeFrom::Herd;
        self
    }

    /// The document for a run that got no outcome at all: the hub could not be
    /// reached, it refused the submission, the link went mid-job.
    ///
    /// **Written, rather than leaving no file.** A wrapper that reads this
    /// document and finds yesterday's would act on yesterday's answer, which
    /// is a worse failure than any this is reporting.
    pub fn none(job_id: Option<String>, job_exists: JobExists, said: String) -> Self {
        Self {
            verdict_format_version: VERDICT_FORMAT_VERSION,
            job_id,
            cause: NO_VERDICT_CAUSE.to_owned(),
            passed: false,
            exit_code: crate::NO_VERDICT_CODE,
            exit_code_from: ExitCodeFrom::Herd,
            job_exists,
            ran_on: None,
            cap: None,
            said: vec![said],
            // A run that got no outcome learned nothing about any tree.
            declared_commit: None,
            ran_commit: None,
            commit_diverged: None,
            outcome: None,
            metrics: None,
        }
    }

    /// Write it where the requester asked for it, as JSON and one line's end.
    ///
    /// Pretty-printed, because the first reader of a new format is always a
    /// person checking that it says what they think, and a 400-byte document
    /// costs nothing to indent.
    pub fn write_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("making {}", parent.display()))?;
        }
        let mut json = serde_json::to_string_pretty(self).context("encoding the verdict")?;
        json.push('\n');
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    }
}

impl ExitCodeFrom {
    fn of(outcome: &JobOutcome) -> Self {
        match outcome {
            // The one case where `exited` is not the job's code: a peer
            // reporting a number no process can have exited with, which
            // `herdctl` answers with its own 125 (`impossible_exit_code_reason`).
            JobOutcome::Exited { code } if u8::try_from(*code).is_err() => ExitCodeFrom::Herd,
            JobOutcome::Exited { .. } => ExitCodeFrom::Job,
            JobOutcome::Signalled { .. } => ExitCodeFrom::Signal,
            _ => ExitCodeFrom::Herd,
        }
    }
}

impl CapReport {
    fn of(outcome: &JobOutcome) -> Option<Self> {
        match outcome {
            JobOutcome::StoppedByCap { cap, limit, detail } => Some(Self {
                cap: cap.name().to_owned(),
                limit: *limit,
                unit: cap.unit().to_owned(),
                detail: detail.clone(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herd_proto::{Cap, JobId};

    fn finished(outcome: JobOutcome) -> Finished {
        Finished {
            job_id: JobId::new("j1197"),
            outcome,
            metrics: JobMetrics::default(),
            outputs: herd_proto::JobOutputs::default(),
            ran_on: Some(RanOn {
                runner: "server".to_owned(),
                why: "the only runner offering kvm".to_owned(),
            }),
            commit: None,
        }
    }

    /// A job whose declared commit and reported revision are both known.
    fn finished_at(declared: &str, ran: Option<&str>) -> Finished {
        Finished {
            commit: Some(herd_proto::CommitRun {
                declared: declared.to_owned(),
                ran: ran.map(str::to_owned),
            }),
            ..finished(JobOutcome::Exited { code: 0 })
        }
    }

    /// The three states of the commit question, as a program reads them
    /// (D176).
    ///
    /// The one that matters is the middle: a job that **passed** and whose
    /// tree was at another revision is `passed: true` and
    /// `commit_diverged: true` at once, because the verdict is the job's and
    /// a side channel does not become one (D20). A document that quietly
    /// turned the pass into a failure would be deciding for the consumer.
    #[test]
    fn a_verdict_says_which_revision_ran_and_whether_it_is_the_declared_one() {
        let agreed = Verdict::of(
            &finished_at(&"a".repeat(40), Some(&"a".repeat(40))),
            vec![],
            0,
        );
        assert_eq!(agreed.commit_diverged, Some(false));
        assert_eq!(agreed.ran_commit.as_deref(), Some("a".repeat(40).as_str()));

        let diverged = Verdict::of(
            &finished_at(&"a".repeat(40), Some(&"b".repeat(40))),
            vec![],
            0,
        );
        assert_eq!(diverged.commit_diverged, Some(true));
        assert!(
            diverged.passed,
            "a divergence is recorded beside the verdict and never instead of it (D20)"
        );
        assert_eq!(diverged.exit_code, 0, "and it does not touch the code");

        // Silence. `null`, not `false`: nobody made the comparison, and a
        // `false` here would be a comparison that came out clean.
        let silent = Verdict::of(&finished_at(&"a".repeat(40), None), vec![], 0);
        assert_eq!(silent.commit_diverged, None);
        assert_eq!(silent.ran_commit, None);
        assert_eq!(
            silent.declared_commit.as_deref(),
            Some("a".repeat(40).as_str())
        );

        // And a job that declared nothing at all says nothing at all.
        let ad_hoc = Verdict::of(&finished(JobOutcome::Exited { code: 0 }), vec![], 0);
        assert_eq!(ad_hoc.declared_commit, None);
        assert_eq!(ad_hoc.commit_diverged, None);
    }

    /// Every field of this document is **there**, `null` or not, so a reader
    /// indexes it rather than probing it — the module's own rule, which a
    /// new field is the easiest thing to break.
    #[test]
    fn the_three_commit_fields_are_always_keys_of_the_document() {
        let json = serde_json::to_value(Verdict::of(
            &finished(JobOutcome::Exited { code: 0 }),
            vec![],
            0,
        ))
        .expect("the document serialises");
        for key in ["declared_commit", "ran_commit", "commit_diverged"] {
            assert!(
                json.get(key).is_some(),
                "{key} has to be a key of every verdict, null or not: {json}"
            );
        }
    }

    /// The distinction the exit-code table could not draw (D166): a job that
    /// exits 124 of its own accord, and a cap that stopped one.
    #[test]
    fn a_job_s_own_124_is_told_apart_from_a_cap_s() {
        let own = Verdict::of(&finished(JobOutcome::Exited { code: 124 }), Vec::new(), 124);
        assert_eq!(own.cause, "exited");
        assert_eq!(own.exit_code, 124);
        assert_eq!(own.exit_code_from, ExitCodeFrom::Job);
        assert!(own.cap.is_none(), "nothing capped this job");

        let capped = Verdict::of(
            &finished(JobOutcome::StoppedByCap {
                cap: Cap::Time,
                limit: 600,
                detail: None,
            }),
            Vec::new(),
            124,
        );
        assert_eq!(capped.cause, "stopped_by_cap");
        assert_eq!(capped.exit_code, 124);
        assert_eq!(capped.exit_code_from, ExitCodeFrom::Herd);
        let cap = capped.cap.expect("a cap ended it, so its figures are here");
        assert_eq!(
            (cap.cap.as_str(), cap.limit, cap.unit.as_str()),
            ("time", 600, "s")
        );

        // And the same for 125, which is the other code a job can wear by
        // itself and the one D20 is most afraid of.
        let own = Verdict::of(&finished(JobOutcome::Exited { code: 125 }), Vec::new(), 125);
        assert_eq!(
            (own.cause.as_str(), own.exit_code_from),
            ("exited", ExitCodeFrom::Job)
        );
        let lost = Verdict::of(
            &finished(JobOutcome::Lost {
                reason: "its runner went silent".to_owned(),
            }),
            Vec::new(),
            125,
        );
        assert_eq!(
            (lost.cause.as_str(), lost.exit_code_from),
            ("lost", ExitCodeFrom::Herd)
        );
    }

    /// Grievances 10a and 10c: the runner's name, in a field, for every
    /// ending that reached a machine — including the one whose stdout framing
    /// line is withheld.
    #[test]
    fn the_runner_s_name_is_a_field_on_every_ending_that_reached_a_machine() {
        for outcome in [
            JobOutcome::Exited { code: 0 },
            JobOutcome::Signalled { signal: 9 },
            JobOutcome::CouldNotStart {
                reason: "the executable is not there".to_owned(),
            },
            JobOutcome::SetupFailed {
                reason: "/out/packs cannot be read".to_owned(),
            },
        ] {
            let verdict = Verdict::of(&finished(outcome.clone()), Vec::new(), 0);
            assert_eq!(
                verdict.ran_on.as_ref().map(|ran_on| ran_on.runner.as_str()),
                Some("server"),
                "{outcome:?} ran on a machine and must name it"
            );
        }
    }

    /// A setup failure is 125 and never a cap's 124, and it says which of the
    /// eight endings it is (D166).
    #[test]
    fn a_setup_failure_is_no_verdict_and_names_itself() {
        let verdict = Verdict::of(
            &finished(JobOutcome::SetupFailed {
                reason: "/srv/herd/out/packs cannot be read".to_owned(),
            }),
            vec!["the machine it ran on could not do its part".to_owned()],
            crate::NO_VERDICT_CODE,
        );
        assert_eq!(verdict.cause, "setup_failed");
        assert_eq!(verdict.exit_code, 125);
        assert_eq!(verdict.exit_code_from, ExitCodeFrom::Herd);
        assert!(!verdict.passed);
        assert!(
            verdict.cap.is_none(),
            "no cap ended this job, which is the whole point"
        );
    }

    /// Every key is there whatever the ending, so a reader indexes the
    /// document instead of probing it — and the version is the first of them.
    #[test]
    fn the_document_has_the_same_keys_whatever_happened() {
        let keys = |verdict: &Verdict| {
            let value: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(verdict).expect("encodes"))
                    .expect("decodes");
            let object = value.as_object().expect("an object").clone();
            object.keys().cloned().collect::<Vec<String>>()
        };
        let ended = Verdict::of(&finished(JobOutcome::Exited { code: 0 }), Vec::new(), 0);
        let never = Verdict::none(
            None,
            JobExists::Nothing,
            "the hub could not be reached".to_owned(),
        );
        assert_eq!(keys(&ended), keys(&never));
        // And the version really is written first. Asserted on the bytes,
        // because `serde_json::Value` sorts an object's keys and would hide
        // the order the document is actually written in.
        assert!(
            serde_json::to_string(&ended)
                .expect("encodes")
                .starts_with("{\"verdict_format_version\":"),
            "a reader must be able to check the version before anything else"
        );
        assert_eq!(ended.verdict_format_version, VERDICT_FORMAT_VERSION);
        assert_eq!(never.cause, NO_VERDICT_CAUSE);
        assert_eq!(never.job_exists, JobExists::Nothing);
    }

    /// **A job no machine had room for is a verdict a program can read, and
    /// it is not `lost`** (D182).
    ///
    /// Alexa's requirement of 2026-10-25, in one assertion: the cause is the
    /// ending's own name, the 125 is Herd's and says so in
    /// `exit_code_from`, and the sentence on stderr names the machines and
    /// the bound — because adding capacity, or finding out why the hub's
    /// count and a runner's disagree, is the whole of what there is to do
    /// about it.
    #[test]
    fn a_job_no_machine_had_room_for_names_its_own_cause_and_wears_herd_s_125() {
        let refused = JobOutcome::RefusedAsFull {
            reason: "the hub's count and theirs never agreed".to_owned(),
            refusals: 8,
            refusals_allowed: 8,
            runners: vec![herd_proto::RunnerThatWasFull {
                name: "capped-runner".to_owned(),
                holding: Some(2),
            }],
        };
        let said = crate::no_verdict_reason(&refused).expect("there is no verdict to give");
        let verdict = Verdict::of(
            &finished(refused.clone()),
            vec![said.clone()],
            crate::NO_VERDICT_CODE,
        );
        assert_eq!(verdict.cause, "refused_as_full");
        assert_eq!(verdict.exit_code, crate::NO_VERDICT_CODE);
        assert_eq!(verdict.exit_code_from, ExitCodeFrom::Herd);
        assert!(!verdict.passed, "nothing ran, so nothing passed");
        // And **not** the ending a reader would go looking for a dead host
        // over, which is what this was reported as before it had a name.
        assert_ne!(verdict.cause, "lost");
        // The sentence beside the 125, which is where a person reads which
        // of the five "no verdict" ends it was (D20), and which the
        // document keeps verbatim.
        assert_eq!(verdict.said, vec![said.clone()]);
        assert!(
            said.contains("8 time(s) of the 8"),
            "both figures, in the sentence as well as in the fields: {said}"
        );
        assert!(
            said.contains("capped-runner (holding 2)"),
            "and the machine with the count it gave: {said}"
        );
    }
}
