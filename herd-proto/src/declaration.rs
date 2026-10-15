//! What a declared command carries, and the four steps a runner runs it
//! through (E-11, D77).
//!
//! The consumer declares its commands in one TOML file in its own tree. That
//! file is parsed in **`herdctl`**, in the requester's own checkout, and never
//! here: the hub and a runner hold no repository and never see TOML (D106).
//! What travels is this module's types — the declaration **resolved**: the
//! command's name, the commit as forty hex, the arguments the requester gave,
//! and the consumer's own four steps with the placeholders only the requester
//! could fill already filled in.
//!
//! Both sides are here for invariant 4's reason: a requester that substituted
//! one token and a runner that substituted another would be two answers to one
//! question, and the line on the record would be the one nobody ran. So the
//! token names, the substitution and the quoting live in one place, and each
//! side is told — below, by name — which tokens are its own to fill.
//!
//! **Who fills what.** Three places, each the one that knows the value:
//!
//! | Token | Filled by | Because |
//! |---|---|---|
//! | `{args}`, `{commit}` | `herdctl` | the requester typed the arguments and resolved the commit in its own checkout (D106) |
//! | `{j}` | the hub | it is a property of the machine the hub chose, derived as it chooses one (E-22, D82) |
//! | `{job}`, `{in}`, `{out}`, `{line}` | the runner | the id is the hub's but the directories are the runner's own, and `{line}` is the command line going into the `run` hook |
//!
//! `{args}` and `{commit}` are filled **before the job is minted**, which is
//! not a detail: the line on the record is then the line that will run, so
//! M6's deduplication key sees two submissions of one command with different
//! arguments as the two jobs they are.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The job's id, as the consumer's executor wants it: the name of the
/// workspace it makes.
pub const JOB_TOKEN: &str = "{job}";
/// The commit, forty hex, resolved in the requester's own checkout (D106).
pub const COMMIT_TOKEN: &str = "{commit}";
/// The arguments the requester gave, quoted for a shell by
/// [`args_as_line`].
pub const ARGS_TOKEN: &str = "{args}";
/// The directory holding what this job consumes — the artefacts of an earlier
/// job of the same commit (M11).
pub const IN_TOKEN: &str = "{in}";
/// The directory this job leaves its artefacts in (M11).
pub const OUT_TOKEN: &str = "{out}";
/// The declared command line, as it goes into the `run` hook.
///
/// The one token that is not a value but the command itself: the consumer's
/// `run` hook is `<their tool> ws run {job} -- {line}`, and `{line}` is where
/// the line they declared is pasted. Herd never interprets that line — their
/// contract's words — so it is pasted as text and their tool receives it
/// through the shell that runs the hook.
pub const LINE_TOKEN: &str = "{line}";

/// Every token Herd fills, for the one check worth making: a declaration
/// carrying a token nobody fills is refused by name rather than run with a
/// brace in it.
///
/// [`crate::PARALLELISM_TOKEN`] is in the list because it is Herd's own and
/// the hub fills it (E-22); it is the only one of the seven that may also be
/// absent from a declaration that asks for no `-j`.
pub const TOKENS: [&str; 7] = [
    JOB_TOKEN,
    COMMIT_TOKEN,
    ARGS_TOKEN,
    IN_TOKEN,
    OUT_TOKEN,
    LINE_TOKEN,
    crate::PARALLELISM_TOKEN,
];

/// One of the consumer's four steps, in the one order they run in.
///
/// **Four named steps, no conditions, no branching, and no step a job can
/// add.** This is not a workflow language and the boundary is invariant 1's:
/// the day a step gains a condition, Herd has grown the thing it refuses to
/// be. The enum is closed for exactly that reason — a list of steps read from
/// a declaration would be the first half of one (D77).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// A workspace for this job, over the consumer's nearest warm cache
    /// (E-14). The step that may take minutes the first time a machine runs
    /// one of their jobs, which is the whole reason the record timestamps
    /// steps at all (D77).
    Up,
    /// The commit, fetched and checked out by the workspace itself. Herd hands
    /// over a hash and holds no repository (D106).
    Fetch,
    /// The declared command line. **This step's exit code is the job's
    /// verdict**, worn untouched — 0, 1 and 75 included (E-02, D20).
    Run,
    /// The workspace, taken down. It runs however `run` ended, and however
    /// `up` and `fetch` ended: a step that only ran on success would leak a
    /// workspace on every failure, and a condition is the thing the four
    /// steps must not have.
    Down,
}

impl Step {
    /// The four, in the fixed order they run in.
    pub const ALL: [Step; 4] = [Step::Up, Step::Fetch, Step::Run, Step::Down];

    /// The name the record and every explanation use. One word, theirs.
    pub fn name(self) -> &'static str {
        match self {
            Step::Up => "up",
            Step::Fetch => "fetch",
            Step::Run => "run",
            Step::Down => "down",
        }
    }

    /// Whether this step is preparation — a step whose failure means the job
    /// never started, rather than a verdict.
    ///
    /// `up` and `fetch` prepare; a non-zero code from either is
    /// [`crate::JobOutcome::CouldNotStart`] naming the step, never an exit code
    /// a caller could read as a failing test (D20). `run` carries the verdict.
    /// `down` happens after the verdict exists and cannot change it.
    pub fn is_preparation(self) -> bool {
        matches!(self, Step::Up | Step::Fetch)
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The consumer's four hooks and the environment they want, as declared.
///
/// Each hook is **one shell line**, run as one: their own `fetch` is
/// `git fetch -q origin && git switch -q --detach {commit}` and their `sh`
/// command is a shell line by definition. "You never interpret a command
/// line" is their contract's rule, and the only way to keep it is to hand the
/// line to a shell whole rather than split it into an argv Herd invented.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executor {
    pub up: String,
    pub fetch: String,
    pub run: String,
    pub down: String,
    /// The environment every step is given, values substituted like the hooks.
    ///
    /// Sorted, because a record two runs of one command leave must compare
    /// equal, and a hash map's order would make two identical jobs read
    /// differently.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Executor {
    /// The hook of one step.
    pub fn hook(&self, step: Step) -> &str {
        match step {
            Step::Up => &self.up,
            Step::Fetch => &self.fetch,
            Step::Run => &self.run,
            Step::Down => &self.down,
        }
    }

    /// The same executor with `token` replaced by `value` in every hook and
    /// every environment value.
    ///
    /// Substitution is textual and done once per token, so a value that
    /// itself contains a brace is never rescanned: a commit is forty hex, a
    /// directory is the runner's own, and an argument the requester typed is
    /// quoted before it gets here ([`args_as_line`]). What this must not do is
    /// loop until nothing changes, which is how a requester's argument becomes
    /// a template.
    pub fn filled(&self, token: &str, value: &str) -> Executor {
        Executor {
            up: self.up.replace(token, value),
            fetch: self.fetch.replace(token, value),
            run: self.run.replace(token, value),
            down: self.down.replace(token, value),
            env: self
                .env
                .iter()
                .map(|(key, held)| (key.clone(), held.replace(token, value)))
                .collect(),
        }
    }

    /// Which of [`TOKENS`] this executor still carries, by name.
    ///
    /// What a refusal quotes: a hook left holding `{out}` after the runner
    /// filled everything it fills is a declaration naming a token Herd does
    /// not provide, and the job is refused by name rather than run with a
    /// brace in its command line.
    pub fn tokens_left(&self) -> Vec<&'static str> {
        TOKENS
            .iter()
            .copied()
            .filter(|token| {
                Step::ALL
                    .iter()
                    .any(|step| self.hook(*step).contains(token))
                    || self.env.values().any(|held| held.contains(token))
            })
            .collect()
    }
}

/// A declared command, resolved: what `herdctl` read in the requester's own
/// checkout and sent with the submission (D106).
///
/// It travels on the submission, is held on the job's record, and is handed
/// back to the runner with the attempt — so a job requeued onto another
/// machine runs the same four steps on the same commit, and a record read
/// months later says what ran without a repository to look it up in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    /// The declared command's name, as the consumer's own file names it.
    ///
    /// Herd's code and docs name no consumer command (E-11): this is a string
    /// that came out of their file and is carried, recorded and printed, never
    /// matched against a list of names Herd knows.
    pub command: String,
    /// The commit this job runs, forty lowercase hex.
    ///
    /// Resolved in the requester's checkout, never on a machine (E-01): what
    /// arrives here is a hash, so there is no ref for two runners to resolve
    /// differently and no moment at which `main` means two things.
    pub commit: String,
    /// The arguments the requester gave, as typed.
    ///
    /// Carried as a list rather than as the line they became, because the
    /// `labels_from` reading is cached by (commit, command, args) (D74) and a
    /// cache key over a quoted line would key on the quoting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The consumer's four steps, with `{args}` and `{commit}` already filled.
    pub executor: Executor,
}

/// How one step of one attempt went: its name, when it started, how long it
/// took, and what it exited with (D77).
///
/// One of these per step, on the attempt rather than on the job, because a job
/// attempted twice prepared twice. Their `up` may pull a whole warm cache the
/// first time a machine runs one of their jobs — minutes, and invisible inside
/// a job's wall time, which is what D77 was written for. An explanation may
/// quote it (E-04, E-36).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRun {
    pub step: Step,
    /// When the step started, in **milliseconds since the Unix epoch**, on the
    /// runner's clock.
    ///
    /// A number rather than a formatted time, for the reason
    /// [`crate::Asked::deadline_unix_s`] is one: a number has no time zone to
    /// be wrong about. The runner's clock rather than the hub's because the
    /// runner is what watched the step; the record says whose it is by saying
    /// which runner held the attempt.
    pub started_unix_ms: i64,
    pub duration_ms: u64,
    /// The code the step's shell exited with.
    ///
    /// Absent when nothing exited: a step a cap stopped, or one that could not
    /// be started at all. Never zero for a step whose code was not read — zero
    /// is a measurement, and this one would be read as success (D23's rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Why there is no exit code, when there is none. `None` whenever
    /// `exit_code` is there, and the two are never both absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
}

impl StepRun {
    /// The one line the record carries for this step (D77).
    ///
    /// Written by the hub from the figures the runner measured, so that the
    /// four lines of an attempt read as one thing however many runners the job
    /// visited.
    pub fn line(&self) -> String {
        let took = format!(
            "{}.{:03} s",
            self.duration_ms / 1000,
            self.duration_ms % 1000
        );
        match (self.exit_code, &self.ended) {
            (Some(code), _) => format!("{}: {took}, exit {code}", self.step),
            (None, Some(why)) => format!("{}: {took}, no exit code: {why}", self.step),
            // Neither: a peer that measured a step and said nothing about how
            // it ended. Said as what it is rather than printed as a success.
            (None, None) => format!("{}: {took}, nothing said about how it ended", self.step),
        }
    }
}

/// One argument, quoted so that a shell hands it back as the one word it is.
///
/// Herd does not interpret a command line, and pasting a requester's argument
/// into a shell line raw would be interpreting it — `--only 'a b'` would reach
/// their tool as two arguments. Single quotes, with the one escape a single
/// quote needs, because inside them a shell expands nothing at all: no
/// variable, no glob, no backtick. An empty argument quotes to `''`, which is
/// how a shell says "a word of no characters" rather than nothing.
pub fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The arguments as one shell word-list, quoted, for [`ARGS_TOKEN`].
///
/// No arguments gives the empty string, so `cmd {args}` is `cmd` and not
/// `cmd ''`.
pub fn args_as_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn an_executor() -> Executor {
        Executor {
            up: format!("tool ws up {JOB_TOKEN} {COMMIT_TOKEN}"),
            fetch: format!("tool ws run {JOB_TOKEN} -- 'git switch --detach {COMMIT_TOKEN}'"),
            run: format!("tool ws run {JOB_TOKEN} -- {LINE_TOKEN}"),
            down: format!("tool ws down {JOB_TOKEN} --purge"),
            env: BTreeMap::from([("RESULTS".to_owned(), format!("{OUT_TOKEN}/results.json"))]),
        }
    }

    /// Substitution reaches the hooks **and** the environment: their
    /// `CHATONS_RESULTS` is `{out}/results.json`, and an environment the
    /// substitution skipped would hand their tool a path with a brace in it.
    #[test]
    fn filling_a_token_reaches_the_hooks_and_the_environment() {
        let filled = an_executor().filled(OUT_TOKEN, "/data/jobs/j1/out");
        assert_eq!(
            filled.env.get("RESULTS").map(String::as_str),
            Some("/data/jobs/j1/out/results.json")
        );
        let filled = filled.filled(JOB_TOKEN, "j1");
        assert_eq!(filled.up, format!("tool ws up j1 {COMMIT_TOKEN}"));
        assert_eq!(filled.down, "tool ws down j1 --purge");
    }

    /// A value that itself holds a token is never rescanned: an argument that
    /// says `{out}` is an argument, not a template (D106's trust is in the
    /// declaration, and this is the one place a requester's text could become
    /// one).
    #[test]
    fn a_filled_value_holding_a_token_is_not_filled_again() {
        let executor = Executor {
            run: format!("tool run {LINE_TOKEN}"),
            ..Executor::default()
        };
        let filled = executor
            .filled(LINE_TOKEN, &format!("echo {OUT_TOKEN}"))
            .filled(OUT_TOKEN, "/data/out");
        assert_eq!(
            filled.run, "tool run echo /data/out",
            "the second fill substitutes the token the first one left, once"
        );
        let other_way = executor
            .filled(OUT_TOKEN, "/data/out")
            .filled(LINE_TOKEN, &format!("echo {OUT_TOKEN}"));
        assert_eq!(
            other_way.run,
            format!("tool run echo {OUT_TOKEN}"),
            "filled before the line arrived, the line's own brace stays text"
        );
    }

    /// What a refusal quotes: the tokens a declaration still carries once
    /// everything Herd fills has been filled.
    #[test]
    fn the_tokens_left_are_named_one_by_one() {
        let executor = an_executor();
        let left = executor.tokens_left();
        assert!(left.contains(&JOB_TOKEN), "{left:?}");
        assert!(left.contains(&COMMIT_TOKEN), "{left:?}");
        assert!(left.contains(&LINE_TOKEN), "{left:?}");
        assert!(
            left.contains(&OUT_TOKEN),
            "{left:?} — the environment counts"
        );
        assert!(
            !left.contains(&IN_TOKEN),
            "{left:?} — a token nothing names is not left"
        );
        let nothing_left = Step::ALL
            .iter()
            .fold(executor, |executor, _| executor)
            .filled(JOB_TOKEN, "j1")
            .filled(COMMIT_TOKEN, "cb5a")
            .filled(LINE_TOKEN, "true")
            .filled(OUT_TOKEN, "/out");
        assert!(nothing_left.tokens_left().is_empty());
    }

    /// An argument with a space in it reaches the command as one word, and an
    /// argument with a quote in it reaches it with the quote.
    #[test]
    fn an_argument_is_quoted_so_a_shell_gives_it_back_whole() {
        assert_eq!(shell_quote("--only"), "'--only'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        // Nothing inside single quotes expands, which is the point: a
        // requester's argument is text, not a shell programme.
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_quote("*"), "'*'");
    }

    #[test]
    fn no_argument_is_an_empty_line_rather_than_an_empty_word() {
        assert_eq!(args_as_line(&[]), "");
        assert_eq!(
            args_as_line(&["--only".to_owned(), "posix,desktop".to_owned()]),
            "'--only' 'posix,desktop'"
        );
    }

    /// The four steps, named as the consumer names them, in the one order
    /// they run in.
    #[test]
    fn the_four_steps_are_named_and_ordered() {
        let names: Vec<_> = Step::ALL.iter().map(|step| step.name()).collect();
        assert_eq!(names, ["up", "fetch", "run", "down"]);
        assert!(Step::Up.is_preparation() && Step::Fetch.is_preparation());
        assert!(
            !Step::Run.is_preparation() && !Step::Down.is_preparation(),
            "run carries the verdict and down happens after it"
        );
    }

    #[test]
    fn a_step_s_line_says_its_duration_and_its_code() {
        let ran = StepRun {
            step: Step::Up,
            started_unix_ms: 1_791_878_153_000,
            duration_ms: 95_400,
            exit_code: Some(0),
            ended: None,
        };
        assert_eq!(ran.line(), "up: 95.400 s, exit 0");
        let capped = StepRun {
            step: Step::Run,
            started_unix_ms: 1_791_878_248_400,
            duration_ms: 2_700_000,
            exit_code: None,
            ended: Some("stopped at the time cap, 2700 s".to_owned()),
        };
        assert_eq!(
            capped.line(),
            "run: 2700.000 s, no exit code: stopped at the time cap, 2700 s"
        );
    }
}
