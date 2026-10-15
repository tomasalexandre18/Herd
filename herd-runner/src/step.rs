//! A declared command, planned as the four shell lines a runner runs it
//! through (E-11, D77, D107, D108).
//!
//! What is here is the *plan*: the four lines, the environment each one is
//! given, the cap each one is bounded by, and the two refusals a declaration
//! can earn before anything is started. Running them — under the memory
//! ceiling, the log cap, the disk budget, and whoever may cancel — is
//! [`crate::job`], which puts an ad-hoc command and a declared one through
//! one path so that the log, the caps, the spool and the verdict are the same
//! machinery for both.
//!
//! ## Four steps, fixed order, no conditions
//!
//! `up`, `fetch`, `run`, `down`, every time, with nothing a declaration can
//! add, skip or make conditional: the day a step gains a condition, Herd has
//! grown the workflow language invariant 1 refuses to be. Two things about
//! the order are *not* conditions, because they are the shape itself and no
//! declaration can change them (D108):
//!
//! * a preparation step that did not come off means the job **never
//!   started**, so what would have followed it is not run — there is no
//!   workspace to run it in;
//! * `down` runs however the three before it ended, including after a failed
//!   `up`, a cap and a cancellation. A workspace nobody takes down is bytes
//!   left on somebody's machine, which is the one thing invariant 2 forbids
//!   above all.
//!
//! ## One shell line, one shell
//!
//! Each hook is handed to `sh -c` whole and never split into an argv Herd
//! invented. "You never interpret a command line" is the consumer's own rule,
//! and their `fetch` — `git fetch -q origin && git switch -q --detach <hash>`
//! — is only a command at all to a shell (D107).

use std::path::Path;
use std::time::Duration;

use herd_proto::{Declaration, JobId, Step, IN_TOKEN, JOB_TOKEN, LINE_TOKEN, OUT_TOKEN};

/// The shell every hook is handed to.
const SHELL: &str = "/bin/sh";

/// One process to run, with everything it runs under.
///
/// An ad-hoc job is one stage whose `step` is `None`: it is not an executor
/// step, it has no step line on the record, and it is bounded by the runner's
/// own figure like every job before M10 was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stage {
    pub step: Option<Step>,
    pub program: String,
    pub arguments: Vec<String>,
    /// What the declaration asks every step to be given, on top of the
    /// environment a job already gets.
    ///
    /// Appended after [`crate::confine::job_environment`], so a declaration
    /// that names a variable Herd also sets wins. That is no weaker than what
    /// the hook itself could do — a shell line can set any variable it likes
    /// — and it is the consumer's own environment block, which exists to be
    /// given to their tool.
    pub env: Vec<(String, String)>,
    /// The time cap this stage is bounded by.
    pub timeout: Duration,
    /// Whose figure [`Stage::timeout`] is, as a capped step is recorded
    /// (D108): the record never says "stopped at the time cap" without
    /// saying whose cap it was.
    pub whose_timeout: String,
    /// What the job's log is told before this stage runs, if anything.
    ///
    /// Set for `run` only, where two figures meet and the record has to say
    /// which of them bound the job (D108).
    pub announce: Option<String>,
}

/// The sentence a step stopped at its time cap is recorded with.
pub(crate) fn stopped_at_the_time_cap(stage: &Stage) -> String {
    format!(
        "stopped at the time cap of {} s, {}",
        stage.timeout.as_secs(),
        stage.whose_timeout
    )
}

/// The runner's own figure, as every step but `run` is bounded by it.
fn the_owner_s_figure() -> String {
    "this runner's own figure".to_owned()
}

/// The one stage of an ad-hoc job: the command line as it arrived, argv and
/// all, under the runner's own cap.
pub(crate) fn ad_hoc(command: &[String], own_timeout: Duration) -> Option<Stage> {
    let (program, arguments) = command.split_first()?;
    Some(Stage {
        step: None,
        program: program.clone(),
        arguments: arguments.to_vec(),
        env: Vec::new(),
        timeout: own_timeout,
        whose_timeout: the_owner_s_figure(),
        announce: None,
    })
}

/// Why a declared job's command line will not do, if it will not.
///
/// The declared line is exactly one word by construction: the requester built
/// it from its own checkout and the hub substituted `{j}` word by word, so a
/// line that arrived as anything else is a line nobody can be sure of.
/// Joining it on a space might make the right command and might make two, and
/// Herd does not interpret a command line (E-11).
///
/// Given back rather than run, because it is a submission to fix and not a
/// job that failed: a line Herd had to guess at would run *something*, and
/// whatever came out would be recorded as a verdict about the consumer's
/// tests (D20's rule, applied one step earlier).
pub(crate) fn why_that_line_will_not_do(
    declaration: &Declaration,
    command: &[String],
) -> Option<String> {
    if command.len() == 1 {
        return None;
    }
    Some(format!(
        "command {} of commit {} arrived as {} word(s) and a declared command's line is \
         exactly one word: this runner neither joins nor splits a command line",
        declaration.command,
        declaration.commit,
        command.len()
    ))
}

/// The four stages of a declared command, or the sentence the attempt is
/// given back with.
///
/// `line` is the declared command line, already checked by
/// [`why_that_line_will_not_do`].
pub(crate) fn planned(
    declaration: &Declaration,
    job_id: &JobId,
    in_dir: &Path,
    out_dir: &Path,
    line: &str,
    declared_timeout: Option<Duration>,
    own_timeout: Duration,
) -> Result<Vec<Stage>, String> {
    // The runner's three, in one pass each (D107). `{line}` is filled *after*
    // they are, and on purpose: it is a requester's own string, and filling it
    // first would make a brace inside it a template — which is the one way a
    // declaration's trust could reach past the declaration.
    let filled = declaration
        .executor
        .filled(JOB_TOKEN, job_id.as_str())
        .filled(IN_TOKEN, &in_dir.display().to_string())
        .filled(OUT_TOKEN, &out_dir.display().to_string());
    // Every token but `{line}`, which is expected to still be there. Checked
    // over the whole executor rather than hook by hook, because a `{down}`
    // nobody could fill would be found after `up` had already made a
    // workspace — and a workspace that cannot be taken down is the leak
    // invariant 2 is about.
    let left: Vec<&str> = filled
        .tokens_left()
        .into_iter()
        .filter(|token| *token != LINE_TOKEN)
        .collect();
    if !left.is_empty() {
        return Err(format!(
            "command {} of commit {} still names {} after this runner filled {JOB_TOKEN}, \
             {IN_TOKEN} and {OUT_TOKEN}: a hook or an environment value holding a token \
             nobody fills would be run with a brace in it",
            declaration.command,
            declaration.commit,
            left.join(" and ")
        ));
    }
    let filled = filled.filled(LINE_TOKEN, line);

    let env: Vec<(String, String)> = filled
        .env
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    let run_cap = run_cap(declared_timeout, own_timeout, &declaration.command);
    Ok(Step::ALL
        .iter()
        .map(|step| Stage {
            step: Some(*step),
            program: SHELL.to_owned(),
            arguments: vec!["-c".to_owned(), filled.hook(*step).to_owned()],
            env: env.clone(),
            timeout: match step {
                Step::Run => run_cap.limit,
                _ => own_timeout,
            },
            whose_timeout: match step {
                Step::Run => run_cap.whose.clone(),
                _ => the_owner_s_figure(),
            },
            announce: match step {
                Step::Run => Some(run_cap.said.clone()),
                _ => None,
            },
        })
        .collect())
}

/// Which figure bounds the `run` step.
struct RunCap {
    limit: Duration,
    /// Whose figure it is, for the line a capped step is recorded with.
    whose: String,
    /// The sentence the job's log gets before `run` starts, naming both
    /// figures, so the record says which of the two bound the job however the
    /// job ends (D108).
    said: String,
}

/// The smaller of the declared figure and the runner's own, named (D108).
///
/// Neither raises the other, and a tie goes to the runner's: the consumer
/// says what its command needs, the owner of the machine says what she lends,
/// and a requester does not overrule her (invariant 2). The preparation steps
/// have no declared figure at all — their `up` may pull a whole warm cache —
/// so the owner's cap is the explicit bound invariant 2 asks of them.
fn run_cap(declared: Option<Duration>, own: Duration, command: &str) -> RunCap {
    match declared {
        Some(declared) if declared < own => RunCap {
            limit: declared,
            whose: format!("the figure command {command} declared"),
            said: format!(
                "the run step is capped at {} s, the figure command {command} declared, which \
                 is smaller than this runner's own {} s",
                declared.as_secs(),
                own.as_secs()
            ),
        },
        Some(declared) => RunCap {
            limit: own,
            whose: the_owner_s_figure(),
            said: format!(
                "the run step is capped at {} s, this runner's own figure: command {command} \
                 declared {} s, and a declared figure does not raise the figure the owner of \
                 this machine lends",
                own.as_secs(),
                declared.as_secs()
            ),
        },
        None => RunCap {
            limit: own,
            whose: the_owner_s_figure(),
            said: format!(
                "the run step is capped at {} s, this runner's own figure, because command \
                 {command} declared no figure of its own",
                own.as_secs()
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herd_proto::{Executor, COMMIT_TOKEN};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A declaration shaped like the consumer's, with `{commit}` and `{args}`
    /// already filled as they arrive (D107), and no consumer command named
    /// anywhere in it (E-11).
    fn declared(run: &str) -> Declaration {
        Declaration {
            command: "fixture".to_owned(),
            commit: "a".repeat(40),
            args: Vec::new(),
            executor: Executor {
                up: format!("mktree {JOB_TOKEN}"),
                fetch: "fetchit cb5a".to_owned(),
                run: run.to_owned(),
                down: format!("rmtree {JOB_TOKEN}"),
                env: BTreeMap::from([("RESULTS".to_owned(), format!("{OUT_TOKEN}/results.json"))]),
            },
        }
    }

    fn plan(declaration: &Declaration, line: &str) -> Result<Vec<Stage>, String> {
        planned(
            declaration,
            &JobId::new("j0000000007"),
            &PathBuf::from("/data/jobs/j0000000007/in"),
            &PathBuf::from("/data/jobs/j0000000007/out"),
            line,
            None,
            Duration::from_secs(60),
        )
    }

    #[test]
    fn the_four_stages_are_the_four_hooks_in_order_each_handed_to_one_shell() {
        let stages = plan(&declared(&format!("inside {LINE_TOKEN}")), "true")
            .expect("a declaration with every token filled plans");
        assert_eq!(
            stages.iter().map(|stage| stage.step).collect::<Vec<_>>(),
            [
                Some(Step::Up),
                Some(Step::Fetch),
                Some(Step::Run),
                Some(Step::Down)
            ]
        );
        for stage in &stages {
            assert_eq!(stage.program, SHELL);
            assert_eq!(
                stage.arguments.len(),
                2,
                "one shell line, never an argv: {:?}",
                stage.arguments
            );
            assert_eq!(stage.arguments[0], "-c");
        }
        assert_eq!(stages[0].arguments[1], "mktree j0000000007");
        assert_eq!(stages[2].arguments[1], "inside true");
        assert_eq!(stages[3].arguments[1], "rmtree j0000000007");
    }

    #[test]
    fn the_runner_s_tokens_are_filled_in_the_environment_as_well_as_the_hooks() {
        let stages = plan(&declared(&format!("inside {LINE_TOKEN}")), "true")
            .expect("a declaration with every token filled plans");
        for stage in &stages {
            assert_eq!(
                stage.env,
                [(
                    "RESULTS".to_owned(),
                    "/data/jobs/j0000000007/out/results.json".to_owned()
                )],
                "every step is given the same environment, substituted"
            );
        }
    }

    /// The declared line is filled last, so a brace a requester typed inside
    /// it stays text rather than becoming a template (D107).
    #[test]
    fn a_brace_inside_the_declared_line_is_not_filled_again() {
        let stages = plan(
            &declared(&format!("inside {LINE_TOKEN}")),
            &format!("tool --only '{OUT_TOKEN}'"),
        )
        .expect("a line holding a brace is a line, not a template");
        assert_eq!(
            stages[2].arguments[1],
            format!("inside tool --only '{OUT_TOKEN}'")
        );
    }

    #[test]
    fn a_hook_left_holding_a_token_is_given_back_naming_it() {
        let mut declaration = declared(&format!("inside {LINE_TOKEN}"));
        declaration.executor.fetch = format!("fetchit {COMMIT_TOKEN}");
        let refusal = plan(&declaration, "true").expect_err("a token nobody fills is refused");
        assert!(
            refusal.contains(COMMIT_TOKEN),
            "the refusal has to name the token: {refusal}"
        );
        assert!(refusal.contains("fixture"), "{refusal}");
    }

    /// The environment counts: a value with a brace in it would reach their
    /// tool as a path with a brace in it.
    #[test]
    fn an_environment_value_left_holding_a_token_is_given_back_naming_it() {
        let mut declaration = declared(&format!("inside {LINE_TOKEN}"));
        declaration
            .executor
            .env
            .insert("WHERE".to_owned(), format!("{COMMIT_TOKEN}/here"));
        let refusal = plan(&declaration, "true").expect_err("a token nobody fills is refused");
        assert!(refusal.contains(COMMIT_TOKEN), "{refusal}");
    }

    #[test]
    fn a_declared_line_that_is_not_one_word_is_given_back_naming_that() {
        let declaration = declared(&format!("inside {LINE_TOKEN}"));
        assert_eq!(
            why_that_line_will_not_do(&declaration, &["true".to_owned()]),
            None,
            "one word is the shape a declared line arrives in"
        );
        for command in [vec![], vec!["two".to_owned(), "words".to_owned()]] {
            let refusal = why_that_line_will_not_do(&declaration, &command)
                .expect("a line that is not one word is refused");
            assert!(
                refusal.contains(&format!("{} word(s)", command.len())),
                "{refusal}"
            );
            assert!(refusal.contains("exactly one word"), "{refusal}");
            assert!(refusal.contains("fixture"), "{refusal}");
        }
    }

    #[test]
    fn the_smaller_of_the_two_figures_caps_the_run_step_and_the_record_says_which() {
        let declaration = declared(&format!("inside {LINE_TOKEN}"));
        let own = Duration::from_secs(60);
        let here = |declared: Option<u64>| {
            planned(
                &declaration,
                &JobId::new("j1"),
                Path::new("/in"),
                Path::new("/out"),
                "true",
                declared.map(Duration::from_secs),
                own,
            )
            .expect("it plans")
        };

        // The declared figure is the smaller one: it binds, and it is named.
        let stages = here(Some(10));
        assert_eq!(stages[2].timeout, Duration::from_secs(10));
        let said = stages[2].announce.clone().expect("run announces its cap");
        assert!(said.contains("capped at 10 s"), "{said}");
        assert!(said.contains("command fixture declared"), "{said}");
        assert!(said.contains("60 s"), "{said}");
        assert!(
            stopped_at_the_time_cap(&stages[2]).contains("the figure command fixture declared"),
            "{}",
            stopped_at_the_time_cap(&stages[2])
        );

        // The runner's own is the smaller one: a declared figure does not
        // raise what the machine's owner lends.
        let stages = here(Some(600));
        assert_eq!(stages[2].timeout, own);
        let said = stages[2].announce.clone().expect("run announces its cap");
        assert!(said.contains("capped at 60 s"), "{said}");
        assert!(said.contains("does not raise"), "{said}");
        assert!(
            stopped_at_the_time_cap(&stages[2]).contains("this runner's own figure"),
            "{said}"
        );

        // A tie goes to the owner's figure, which is the one that is true of
        // the machine.
        let stages = here(Some(60));
        assert_eq!(stages[2].timeout, own);
        assert!(
            stages[2]
                .announce
                .as_ref()
                .is_some_and(|said| said.contains("does not raise")),
            "{:?}",
            stages[2].announce
        );

        // No declared figure at all: every job before M10.
        let stages = here(None);
        assert_eq!(stages[2].timeout, own);
        assert!(
            stages[2]
                .announce
                .as_ref()
                .is_some_and(|said| said.contains("declared no figure of its own")),
            "{:?}",
            stages[2].announce
        );
    }

    /// The preparation steps and the takedown are bounded by the runner's own
    /// figure whatever the command declared: they have no declared figure,
    /// and the owner's cap is the explicit bound invariant 2 asks for.
    #[test]
    fn every_step_but_run_is_capped_at_the_runner_s_own_figure() {
        let stages = planned(
            &declared(&format!("inside {LINE_TOKEN}")),
            &JobId::new("j1"),
            Path::new("/in"),
            Path::new("/out"),
            "true",
            Some(Duration::from_secs(5)),
            Duration::from_secs(60),
        )
        .expect("it plans");
        for stage in [&stages[0], &stages[1], &stages[3]] {
            assert_eq!(stage.timeout, Duration::from_secs(60));
            assert_eq!(stage.whose_timeout, "this runner's own figure");
            assert!(
                stage.announce.is_none(),
                "only run has two figures to choose between"
            );
        }
        assert_eq!(stages[2].timeout, Duration::from_secs(5));
    }

    #[test]
    fn an_ad_hoc_job_is_one_stage_that_is_no_step_at_all() {
        let stage = ad_hoc(
            &["/bin/sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            Duration::from_secs(60),
        )
        .expect("a command with a program is one stage");
        assert_eq!(stage.step, None);
        assert_eq!(stage.program, "/bin/sh");
        assert_eq!(stage.arguments, ["-c", "true"]);
        assert!(stage.env.is_empty());
        assert_eq!(ad_hoc(&[], Duration::from_secs(60)), None);
    }
}
