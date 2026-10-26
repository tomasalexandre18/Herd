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
//!
//! ## The declared line is a template too
//!
//! A command's own line may name `{job}`, `{in}`, `{out}` and `{ran}` like
//! any hook, and they are filled — before the line is pasted into `{line}`,
//! and never over the text a requester's arguments put there.
//! [`line_filled`] is where that order is kept and why it is the order it is.
//!
//! ## The revision a `fetch` reached
//!
//! `{ran}` names a file the consumer's `fetch` hook may write the revision it
//! really reached into, and [`ran_commit`] is how the runner reads it back
//! after that step. Herd runs no `git` and knows no VCS (E-11): the hook
//! answers, and the runner carries the answer home without parsing it. A
//! hook that writes nothing is the ordinary case and says nothing, which is
//! not the same as saying the tree was where it was asked to be.

use std::path::Path;
use std::time::Duration;

use herd_proto::{
    filled_outside_the_arguments, Declaration, JobId, Step, IN_TOKEN, JOB_TOKEN, LINE_TOKEN,
    OUT_TOKEN, RAN_TOKEN,
};

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

/// The paths of this job's own workspace a declaration may name — the
/// runner's three, as one argument.
///
/// One struct rather than three parameters because they travel together and
/// always will: they are all the same workspace's, they are all filled in
/// the same pass, and a fourth would otherwise be a fourth argument.
pub(crate) struct Places<'a> {
    /// `{in}`: what this job consumes.
    pub in_dir: &'a Path,
    /// `{out}`: what it leaves behind.
    pub out_dir: &'a Path,
    /// `{ran}`: where its `fetch` hook may say which revision it reached.
    pub ran: &'a Path,
}

/// The four stages of a declared command, or the sentence the attempt is
/// given back with.
///
/// `line` is the declared command line, already checked by
/// [`why_that_line_will_not_do`].
pub(crate) fn planned(
    declaration: &Declaration,
    job_id: &JobId,
    places: Places<'_>,
    line: &str,
    declared_timeout: Option<Duration>,
    own_timeout: Duration,
) -> Result<Vec<Stage>, String> {
    // The runner's four, in one pass each (D107). `{line}` is filled *after*
    // they are, and on purpose: it is a requester's own string, and filling it
    // first would make a brace inside it a template — which is the one way a
    // declaration's trust could reach past the declaration.
    let in_dir = places.in_dir.display().to_string();
    let out_dir = places.out_dir.display().to_string();
    let ran_path = places.ran.display().to_string();
    let filled = declaration
        .executor
        .filled(JOB_TOKEN, job_id.as_str())
        .filled(IN_TOKEN, &in_dir)
        .filled(OUT_TOKEN, &out_dir)
        .filled(RAN_TOKEN, &ran_path);
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
             {IN_TOKEN}, {OUT_TOKEN} and {RAN_TOKEN}: a hook or an environment value holding \
             a token nobody fills would be run with a brace in it",
            declaration.command,
            declaration.commit,
            left.join(" and ")
        ));
    }
    // The declared line is a template of its own, and its tokens are filled
    // before it is pasted in — see [`line_filled`].
    let line = line_filled(
        line,
        &declaration.args,
        job_id.as_str(),
        &in_dir,
        &out_dir,
        &ran_path,
    );
    let filled = filled.filled(LINE_TOKEN, &line);

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

/// The declared command's own line, with the runner's tokens filled in the
/// text the **declaration** wrote and in nothing else.
///
/// A declared line is a template exactly as the four hooks are: the first
/// consumer's `tool pack --out {out}/packs` means the directory, not three
/// characters. So `{job}`, `{in}` and `{out}` are filled here, before the
/// line is pasted into the `run` hook's `{line}`.
///
/// **The order is the whole of it, and the order is D107's.** The
/// requester's arguments were pasted into this line before the job was
/// minted, quoted, so that the line on the record is the line that will run
/// and M6's key sees two submissions with different arguments as two jobs.
/// D107's property is that a value a substitution put there is never itself
/// scanned for tokens — `--only '{out}'` is an argument and not a template,
/// and a fixed point is the one way a declaration's trust could reach past
/// the declaration. The only order that keeps both is to fill the declared
/// text *before* `{args}` is in it, so that is what happens: the spans the
/// requester's arguments contributed are held out of the pass, and the
/// declared text around them is filled.
///
/// Holding the arguments out of the pass is [`filled_outside_the_arguments`],
/// in `herd-proto`, because the hub's `{j}` has the same problem and one of
/// the two solving it alone is how the two would drift.
fn line_filled(
    line: &str,
    args: &[String],
    job: &str,
    in_dir: &str,
    out_dir: &str,
    ran_path: &str,
) -> String {
    filled_outside_the_arguments(line, args, |text| {
        text.replace(JOB_TOKEN, job)
            .replace(IN_TOKEN, in_dir)
            .replace(OUT_TOKEN, out_dir)
            .replace(RAN_TOKEN, ran_path)
    })
}

/// How many bytes of the `{ran}` file this runner looks at.
///
/// A revision is one short line. Anything beyond this is not a revision, and
/// reading a job-sized file into memory to find that out is a job writing
/// into the runner's address space.
const RAN_READ_MAX: u64 = 4096;

/// The longest revision this runner will carry home.
///
/// Wide enough for every hash a VCS has shipped — a git sha256 is 64 — and
/// narrow enough that the figure on a record is a revision and not a
/// paragraph. Herd does not know what produced the string, so the only
/// checks it makes are the ones that hold for *any* answer: one line, no
/// control characters, bounded.
const RAN_MAX: usize = 128;

/// What the `{ran}` file said, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RanCommit {
    /// The hook wrote a revision, and this is it, untouched but for the
    /// whitespace around it.
    Said(String),
    /// No file, or an empty one: this job's hook reports no revision. The
    /// ordinary case, and not an error.
    Nothing,
    /// There was a file and it cannot be used as an answer, with the reason.
    ///
    /// Reported as *silence* on the record — the runner has no revision to
    /// put there — and said out loud in the job's log, because a consumer
    /// whose hook writes the wrong thing would otherwise see exactly what a
    /// consumer whose hook writes nothing sees.
    Unusable(String),
}

/// Read back what this job's `fetch` hook left at `{ran}` (D176).
///
/// Strict in the three ways an answer from outside Herd has to be strict, and
/// incurious in every other way: the string is never parsed, never matched
/// against a shape Herd invented, and never lowercased. Herd compares it with
/// the declared commit byte for byte and that is all it ever does with it.
pub(crate) fn ran_commit(path: &Path) -> RanCommit {
    use std::io::Read;

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return RanCommit::Nothing,
        Err(error) => {
            return RanCommit::Unusable(format!("it could not be opened: {error}"));
        }
    };
    let mut held = String::new();
    // Bounded before anything else: the file is the job's to write and the
    // runner's to read, which is exactly the direction a cap belongs in.
    if let Err(error) = file.take(RAN_READ_MAX).read_to_string(&mut held) {
        return RanCommit::Unusable(format!("it could not be read as text: {error}"));
    }
    let said = held.lines().next().unwrap_or_default().trim();
    if said.is_empty() {
        return RanCommit::Nothing;
    }
    if said.len() > RAN_MAX {
        return RanCommit::Unusable(format!(
            "its first line is {} characters and a revision this runner will carry is at \
             most {RAN_MAX}",
            said.len()
        ));
    }
    if !said.chars().all(|c| c.is_ascii_graphic()) {
        return RanCommit::Unusable(
            "its first line holds something that is not a printable ASCII word, and a \
             revision on a record has to be one"
                .to_owned(),
        );
    }
    RanCommit::Said(said.to_owned())
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
    use herd_proto::{args_as_line, Executor, COMMIT_TOKEN};
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
            Places {
                in_dir: &PathBuf::from("/data/jobs/j0000000007/in"),
                out_dir: &PathBuf::from("/data/jobs/j0000000007/out"),
                ran: &PathBuf::from("/data/jobs/j0000000007/ran"),
            },
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

    /// A declared command's **own line** names the directories like any
    /// hook: `tool pack --out {out}/packs` means the directory, and reaching
    /// their hook with three characters is the defect this closes.
    #[test]
    fn the_declared_line_s_own_tokens_are_filled_before_it_is_pasted_in() {
        let stages = plan(
            &declared(&format!("inside {LINE_TOKEN}")),
            &format!("tool pack --out {OUT_TOKEN}/packs --from {IN_TOKEN}/x --as {JOB_TOKEN}"),
        )
        .expect("a declaration with every token filled plans");
        assert_eq!(
            stages[2].arguments[1],
            "inside tool pack --out /data/jobs/j0000000007/out/packs \
             --from /data/jobs/j0000000007/in/x --as j0000000007"
        );
    }

    /// The adversarial half, and the one that proves D107 still holds: the
    /// declared text around a requester's arguments is filled, the arguments
    /// themselves are not. A requester who typed `{out}` typed three
    /// characters, and a value a substitution pasted in is never itself
    /// scanned for tokens.
    #[test]
    fn a_token_a_requester_typed_in_an_argument_is_text_and_stays_text() {
        let mut declaration = declared(&format!("inside {LINE_TOKEN}"));
        declaration.args = vec![
            "--only".to_owned(),
            OUT_TOKEN.to_owned(),
            format!("{IN_TOKEN}/{JOB_TOKEN}"),
        ];
        // The line as `herdctl` resolved it: the declared template with the
        // arguments quoted into its `{args}` (D107).
        let line = format!(
            "tool pack --out {OUT_TOKEN}/packs {}",
            args_as_line(&declaration.args)
        );
        let stages = plan(&declaration, &line).expect("it plans");
        assert_eq!(
            stages[2].arguments[1],
            format!(
                "inside tool pack --out /data/jobs/j0000000007/out/packs \
                 '--only' '{OUT_TOKEN}' '{IN_TOKEN}/{JOB_TOKEN}'"
            )
        );
    }

    /// An argument that is the whole of the line's requester text and holds
    /// no brace at all leaves the declared tokens filled either side of it.
    #[test]
    fn the_declared_text_on_both_sides_of_the_arguments_is_filled() {
        let mut declaration = declared(&format!("inside {LINE_TOKEN}"));
        declaration.args = vec!["a b".to_owned()];
        let line = format!("tool {} --out {OUT_TOKEN}", args_as_line(&declaration.args));
        let stages = plan(&declaration, &line).expect("it plans");
        assert_eq!(
            stages[2].arguments[1],
            "inside tool 'a b' --out /data/jobs/j0000000007/out"
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
                Places {
                    in_dir: Path::new("/in"),
                    out_dir: Path::new("/out"),
                    ran: Path::new("/ran"),
                },
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
            Places {
                in_dir: Path::new("/in"),
                out_dir: Path::new("/out"),
                ran: Path::new("/ran"),
            },
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

    /// `{ran}` is filled like the other three the runner owns, in every
    /// hook, in the environment, and in the declared line.
    #[test]
    fn the_path_the_fetch_hook_answers_at_is_filled_everywhere_the_others_are() {
        let mut declaration = declared(&format!("pack --into {RAN_TOKEN} {LINE_TOKEN}"));
        declaration.executor.fetch = format!("reach it and say so > {RAN_TOKEN}");
        declaration
            .executor
            .env
            .insert("REVISION_AT".to_owned(), RAN_TOKEN.to_owned());
        let stages = plan(&declaration, &format!("build --note {RAN_TOKEN}"))
            .expect("a declaration naming {ran} plans");
        let fetch = &stages[1].arguments[1];
        assert_eq!(fetch, "reach it and say so > /data/jobs/j0000000007/ran");
        assert_eq!(
            stages[1].env,
            [
                (
                    "RESULTS".to_owned(),
                    "/data/jobs/j0000000007/out/results.json".to_owned()
                ),
                (
                    "REVISION_AT".to_owned(),
                    "/data/jobs/j0000000007/ran".to_owned()
                )
            ]
        );
        assert!(
            stages[2].arguments[1].contains("build --note /data/jobs/j0000000007/ran"),
            "the declared line is a template too (D107): {:?}",
            stages[2].arguments[1]
        );
    }

    /// The three answers the `{ran}` file can give, and the one it must not:
    /// a file that cannot be used is **silence**, not a revision and not a
    /// failure of the job.
    #[test]
    fn the_revision_file_is_read_strictly_and_its_absence_is_not_an_error() {
        let dir = crate::testing::TempDir::new("ran-commit");
        let path = dir.path().join("ran");

        assert_eq!(
            ran_commit(&path),
            RanCommit::Nothing,
            "no file is the state every consumer is in before it writes one"
        );

        // The ordinary answer, with the newline a shell's `>` leaves.
        std::fs::write(&path, "0a2cf73a1b2c3d4e5f60718293a4b5c6d7e8f901\n").expect("written");
        assert_eq!(
            ran_commit(&path),
            RanCommit::Said("0a2cf73a1b2c3d4e5f60718293a4b5c6d7e8f901".to_owned())
        );

        // A hook that wrote and said nothing.
        std::fs::write(&path, "   \n").expect("written");
        assert_eq!(ran_commit(&path), RanCommit::Nothing);

        // Herd does not know what produced the string: a revision that is
        // not a git hash is carried home untouched, because E-11 means Herd
        // names no consumer's tool and knows no VCS.
        std::fs::write(&path, "svn:r48217").expect("written");
        assert_eq!(ran_commit(&path), RanCommit::Said("svn:r48217".to_owned()));

        // Only the first line: a hook that printed its own chatter after the
        // answer still answered.
        std::fs::write(&path, "abc123\nand some chatter\n").expect("written");
        assert_eq!(ran_commit(&path), RanCommit::Said("abc123".to_owned()));

        // Bounded, and not truncated: half a revision reported as a
        // revision would be worse than none.
        std::fs::write(&path, "x".repeat(RAN_MAX + 1)).expect("written");
        assert!(matches!(ran_commit(&path), RanCommit::Unusable(_)));

        // A line with a control character in it is not a word, and a record
        // is not somewhere to paste one.
        std::fs::write(&path, "abc\u{7}123").expect("written");
        assert!(matches!(ran_commit(&path), RanCommit::Unusable(_)));

        // Nor is a file that is not text at all.
        std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("written");
        assert!(matches!(ran_commit(&path), RanCommit::Unusable(_)));

        // A whole job's worth of bytes is read up to the cap and no
        // further: the file is the job's to write and the runner's to read.
        std::fs::write(&path, "y".repeat(16 << 20)).expect("written");
        assert!(matches!(ran_commit(&path), RanCommit::Unusable(_)));
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
