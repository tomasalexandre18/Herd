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
//! | `{job}`, `{in}`, `{out}`, `{ran}`, `{line}` | the runner | the id is the hub's, the directories and `{ran}`'s path are the runner's own, and `{line}` is the command line going into the `run` hook |
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
/// The path this job's `fetch` hook may write the revision it really reached
/// into (D176).
///
/// **A path, not a value.** Herd holds no repository and runs no VCS (E-11,
/// invariant 1), so it cannot resolve a revision and does not try. What it
/// does is hand the consumer's own hook a file name inside the job's
/// workspace and read back the one line the hook leaves there — the same
/// shape as [`LabelsFrom`], where the consumer's command answers and Herd
/// reads the answer strictly.
///
/// **Writing it is optional and stays optional.** A `fetch` hook that names
/// this token nowhere leaves no file, the runner reports no revision, and
/// the record says so in as many words. Silence is a normal state: never an
/// error, never a refusal, and never read as agreement (D23's rule).
pub const RAN_TOKEN: &str = "{ran}";

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
/// the hub fills it (E-22); it is the only one of the eight that may also be
/// absent from a declaration that asks for no `-j`.
pub const TOKENS: [&str; 8] = [
    JOB_TOKEN,
    COMMIT_TOKEN,
    ARGS_TOKEN,
    IN_TOKEN,
    OUT_TOKEN,
    RAN_TOKEN,
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
    /// The revision the tree was really at when this step ended, as the
    /// consumer's own hook reported it through [`RAN_TOKEN`] (D176).
    ///
    /// Set on the `fetch` step and on no other: `fetch` is the step that puts
    /// the tree at the commit, so it is the step whose end the question is
    /// about. A revision read after `run` would be whatever the job's own
    /// command left behind.
    ///
    /// **`None` means "this runner did not report a revision", never "it
    /// matched".** A peer that predates the field, a consumer whose hook
    /// writes nothing, and a file that could not be read all land here, and
    /// none of them is evidence that the tree was at the declared commit. The
    /// rule is D23's and the shape is the one the health report's `suspended`
    /// needed: a default that *agreed* would read an old peer's silence as a
    /// clean bill of health.
    ///
    /// Herd does not know what produced this string and never parses it. It
    /// is compared with [`Declaration::commit`], byte for byte, by the hub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran_commit: Option<String>,
}

/// What an attempt said about the revision it really ran, beside the commit
/// the job declared (D176).
///
/// The hub builds one of these when a declared command's `fetch` step is
/// recorded, holds it on the attempt, and hands it to the requester with the
/// verdict. It is the first thing on a Herd record that can **contradict** a
/// verdict's commit: before it, "this verdict is about commit X" rested
/// entirely on the assumption that a `fetch` exiting 0 left the tree at X,
/// and nothing on the record could say otherwise.
///
/// **It records; it refuses nothing.** A divergence does not fail the job,
/// does not change its exit code, and does not change the verdict: the
/// verdict is the job's and a side channel does not become one (D20).
/// Whether a divergence should also *stop* a job is a second decision, to be
/// taken on the divergences this field makes visible rather than before
/// there are any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRun {
    /// The commit the job declared, forty lowercase hex, as `herdctl`
    /// resolved it in the requester's own checkout (E-01, D106).
    pub declared: String,
    /// What the runner reported, or `None` for "this runner did not report a
    /// revision".
    ///
    /// Serialised **even when it is `None`**, as an explicit `null`: an
    /// absent key and a key that is there and empty read the same to a
    /// careless reader, and the whole point of the field is that silence is
    /// not agreement.
    #[serde(default)]
    pub ran: Option<String>,
}

impl CommitRun {
    /// Whether the revision reported contradicts the commit declared.
    ///
    /// `false` for silence, which is not agreement either — [`CommitRun::ran`]
    /// is what tells the two apart, and the one question this answers is "is
    /// there a divergence on the record".
    pub fn diverged(&self) -> bool {
        self.ran
            .as_deref()
            .is_some_and(|ran| ran != self.declared.as_str())
    }

    /// The one sentence a reader is given for this, in Herd's own words.
    pub fn line(&self) -> String {
        match &self.ran {
            Some(ran) if ran == &self.declared => {
                format!("declared {}, and the runner ran {ran}", self.declared)
            }
            Some(ran) => format!(
                "declared {}, ran {ran}: this attempt's tree was at a different revision \
                 from the one this job is recorded against",
                self.declared
            ),
            None => format!(
                "declared {}; this runner did not report a revision, so nothing here \
                 confirms the tree was at it",
                self.declared
            ),
        }
    }
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

/// The spans of a line the **declaration itself** wrote, with the text a
/// requester's arguments put there held out (D107, D139).
///
/// Every pass over an already-resolved line has the same problem — the
/// runner's `{job}`, `{in}` and `{out}`, the hub's `{j}` — so it is answered
/// once, here. `{args}` is filled **before the job is minted**, for M6's key
/// (D107), and D107's property is that a value a substitution pasted in is
/// never itself scanned for tokens: `--only '{out}'` is an argument, not a
/// template. A single pass over the whole line cannot keep both, so the pass
/// is made over the declared text only.
///
/// The arguments are held out by the only thing that identifies them
/// exactly — the requester's own words, which already travel beside the line
/// ([`Declaration::args`]) and were quoted into it by one function
/// ([`args_as_line`]). Nothing is encoded, escaped or marked, and no frame
/// changes. An empty argument list, or a line no `{args}` reached, means
/// there is no requester text in the line at all and the whole of it is
/// declared.
///
/// Given back as the declared spans and the text between them, so that
/// filling a token ([`filled_outside_the_arguments`]) and *counting* one are
/// the same idea of what "declared" means rather than two. The failure
/// direction is conservative either way: a declared token left literal, never
/// a requester's own text read as Herd's.
pub fn declared_spans<'line>(line: &'line str, args: &[String]) -> (Vec<&'line str>, String) {
    let pasted = args_as_line(args);
    if pasted.is_empty() || !line.contains(pasted.as_str()) {
        return (vec![line], pasted);
    }
    (line.split(pasted.as_str()).collect(), pasted)
}

/// `line` with `fill` applied to the text the declaration wrote and to
/// nothing a requester's arguments put there (D107, D139).
pub fn filled_outside_the_arguments(
    line: &str,
    args: &[String],
    fill: impl Fn(&str) -> String,
) -> String {
    let (spans, pasted) = declared_spans(line, args);
    spans
        .into_iter()
        .map(fill)
        .collect::<Vec<_>>()
        .join(&pasted)
}

/// A command whose requirements are not knowable from its declaration: the
/// line that reads them, and the labels that reading is itself placed against
/// (D74).
///
/// Which capabilities one commit's tests need is a property of that commit,
/// so the declaration names a command of the consumer's own that prints them.
/// What travels on a submission is this: the line, with `{commit}` and
/// `{args}` already filled like any other (D107), and the command's
/// **declared** labels, which are the baseline the *reading* runs against —
/// the first consumer's reading runs their own tool over their own tree, so it
/// needs their build capability to run at all, and it is an ordinary job like
/// any other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelsFrom {
    /// The shell line that prints the requirements.
    pub line: String,
    /// The labels the reading job is placed against — never the labels of the
    /// job that follows it.
    #[serde(default, skip_serializing_if = "crate::LabelSet::is_empty")]
    pub baseline: crate::LabelSet,
}

/// The one key an answer may carry that is a list: the capabilities the job
/// needs, comma-separated.
pub const LABELS_KEY: &str = "labels";
/// How much memory the whole job needs, in MiB (E-29).
pub const MEMORY_KEY: &str = "memory_mib";
/// How much room on the runner's tmpfs it reserves while it runs (E-31).
pub const TMPFS_KEY: &str = "tmpfs_mib";
/// How many of the consumer's own scenarios the run covers. Recorded, and
/// acted on by nothing: it is their figure and Herd has no use for it, but a
/// record a reader re-judges months later should say what the reading said
/// (E-15, E-54).
pub const SCENARIOS_KEY: &str = "scenarios";

/// Every key an answer carries, and the whole vocabulary: a key-shaped line
/// naming anything else is refused by name (D60's reading, applied here).
pub const READING_KEYS: [&str; 4] = [LABELS_KEY, MEMORY_KEY, TMPFS_KEY, SCENARIOS_KEY];

/// What a [`LabelsFrom`] reading answered: the requirements of every job of
/// that command, on that commit, with those arguments (D74).
///
/// The hub writes these onto the job's record before it places it, and M7's
/// matching and M8's admission then read them off the record like any other
/// job's — which is the whole of D74's consequence for the plan: where a
/// job's requirements came from is this module's problem and nobody else's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirements {
    /// The capabilities the job needs (E-25).
    #[serde(default, skip_serializing_if = "crate::LabelSet::is_empty")]
    pub labels: crate::LabelSet,
    /// How much memory the whole job needs, in MiB (E-29).
    pub memory_mib: u64,
    /// How much tmpfs it reserves while it runs, in MiB (E-31). Nought is a
    /// real answer here, unlike the memory figure: `build` and `check` really
    /// need no tmpfs.
    pub tmpfs_mib: u64,
    /// The consumer's own count of the scenarios the run covers.
    pub scenarios: u64,
}

impl Requirements {
    /// The answer, read **strictly**, out of what the reading printed.
    ///
    /// Read the way the capability file is read (D60), and for D60's own
    /// reason: one character is the difference between a job admitted against
    /// the figure it needs and a job admitted against a figure nobody stated.
    /// So an unknown key, a missing key, a duplicated key and a figure that is
    /// not a figure are each a refusal, and the caller refuses the job that
    /// was waiting for this answer **by name** rather than placing it on a
    /// guess.
    ///
    /// **What counts as an answer line.** `<key>=<value>`, with the key one
    /// lowercase word of `[a-z0-9_]`. Everything else in the output is the
    /// consumer's executor talking — their `up` announces a workspace, their
    /// `fetch` says which commit it went to, their `down` prints a step log —
    /// and a reading's output is a job's output like any other (D18), so there
    /// is no second stream to read the answer out of. A line that is not
    /// key-shaped is therefore not an answer and not a refusal either; a line
    /// that *is* key-shaped is held to the whole of the rule above.
    ///
    /// The bytes are the job's log, so they are not promised to be text: what
    /// is not valid UTF-8 cannot be a key line and is skipped by the same
    /// rule, never guessed at.
    pub fn read(output: &[u8]) -> Result<Requirements, String> {
        let mut said: BTreeMap<&str, &str> = BTreeMap::new();
        for line in output.split(|byte| *byte == b'\n') {
            let Ok(line) = std::str::from_utf8(line) else {
                continue;
            };
            let Some((key, value)) = answer_line(line) else {
                continue;
            };
            if !READING_KEYS.contains(&key) {
                return Err(format!(
                    "it printed {key:?}, which is not one of the keys an answer carries \
                     ({}): a reading's answer is read strictly, because a key nobody checked \
                     is a requirement nobody stated",
                    READING_KEYS.join(", ")
                ));
            }
            if let Some(already) = said.insert(key, value) {
                return Err(format!(
                    "it printed {key:?} twice, as {already:?} and then as {value:?}: one key \
                     says one thing, and taking the last of two would be a figure nobody chose"
                ));
            }
        }
        for key in READING_KEYS {
            if !said.contains_key(key) {
                return Err(format!(
                    "it printed no {key:?} line, and an answer carries all four of {}: a \
                     missing key is not a default",
                    READING_KEYS.join(", ")
                ));
            }
        }
        let labels = read_labels(said[LABELS_KEY])?;
        let memory_mib = read_figure(MEMORY_KEY, said[MEMORY_KEY])?;
        if memory_mib == 0 {
            return Err(format!(
                "it said the whole job needs 0 MiB of memory, which is not a figure: a job \
                 that needs no memory in particular declares nothing rather than nought, and \
                 nothing is not something a reading can say ({MEMORY_KEY})"
            ));
        }
        Ok(Requirements {
            labels,
            memory_mib,
            tmpfs_mib: read_figure(TMPFS_KEY, said[TMPFS_KEY])?,
            scenarios: read_figure(SCENARIOS_KEY, said[SCENARIOS_KEY])?,
        })
    }

    /// The one line a record and an explanation say an answer in (E-15, E-36).
    pub fn line(&self) -> String {
        let labels = if self.labels.is_empty() {
            "no capability in particular".to_owned()
        } else {
            format!("labels {}", self.labels)
        };
        format!(
            "{labels}, {} MiB of memory, {} MiB of tmpfs, {} scenario(s)",
            self.memory_mib, self.tmpfs_mib, self.scenarios
        )
    }
}

/// How much more than a job declared its kernel ceiling allows: **one tenth
/// of the declared sum**, and this is the only margin in the product.
///
/// **Why there has to be one at all.** The consumer's kernel memcg
/// out-of-memory report for their job `j0000000069`, on a Herd job cgroup,
/// read `memory: usage 8388608kB, limit 8388608kB … shmem 4219834368` while
/// the killed process's own `shmem-rss` was **0** — 4.03 GiB of anonymous
/// memory plus 3.93 GiB of shmem against an 8.00 GiB ceiling. Tmpfs pages a
/// job writes are charged to the cgroup that wrote them, and a job cgroup
/// sets `memory.swap.max` to 0, so those pages cannot be reclaimed either: a
/// job's tmpfs reservation sits **inside** its memory ceiling, not beside it.
/// Herd admits [`Requirements::memory_mib`] (E-29) and
/// [`Requirements::tmpfs_mib`] (E-31) as two independent quantities and they
/// are not independent. A ceiling of `memory_mib + tmpfs_mib` exactly is the
/// figure that killed that job, because the last page of a reservation the
/// job is entitled to use is the page that trips the limit — so a ceiling
/// with no margin would kill every job that uses the tmpfs it reserved, on
/// the first day it was enforced.
///
/// **Why one tenth, and why a proportion.** The report's own figures do not
/// close on its limit: 8.00 GiB less 4.03 GiB of anon and 3.93 GiB of shmem
/// leaves about 40 MiB — a residue of that order at that rounding, and the
/// only measurement of it there is. That residue is what the kernel charges
/// the cgroup on the job's behalf and no declaration can see: page tables,
/// slab, kernel stacks, socket buffers. It scales with the job's own
/// footprint and its process count, not with the host, so the margin is a
/// proportion and not a figure of bytes. One tenth is an order of magnitude
/// above that residue, which is the side to be wrong on for a cap whose
/// purpose is to kill a runaway job and not a working one; and it is small
/// enough that packing by the granted sum, which will charge this ceiling
/// against the memory a host lends, pays a tenth of a job for it rather than
/// a whole one.
///
///
/// **What the margin applies to, and what it never lifts.** It applies to the
/// *declared sum* and to nothing else. The ceiling a runner writes is bounded
/// above by its owner's `--job-memory-max`, which is an absolute cap on the
/// machine: the margin can make a declared figure's ceiling larger than the
/// declaration, never larger than what the machine's owner lends one job
/// (`herd_runner::job_ceiling`). A margin that could lift an owner's
/// configured bound would be a figure a requester sent weakening invariant 2
/// on somebody else's host.
/// **One figure, in one place.** Both sides have to charge the same ceiling —
/// `herd-hub/src/matcher.rs` is explicit that a hub counting a different set
/// from the runner is the defect — so the margin lives here, on the contract,
/// and neither crate carries a second copy of it.
pub const CEILING_MARGIN_PERCENT: u64 = 10;

/// What the hub costed one job at, as the runner receives it (E-29, E-31).
///
/// The hub has always known this and the runner never did: `AssignJob`
/// carried a command, a declaration and a time cap, and the runner — the only
/// thing in the product that writes a kernel memory ceiling — capped every
/// job at the one figure its owner gave it at startup. D180 named that gap
/// and refused to close it by guessing (*"`AssignJob` carries no declared
/// memory need, only a command"*); carrying the figure was left as a protocol
/// decision, and it has been made.
///
/// **Both figures, because one of them is useless alone.** See
/// [`CEILING_MARGIN_PERCENT`]: the tmpfs a job reserves is charged to the
/// job's own cgroup, so a ceiling derived from the memory figure alone kills
/// a job for using exactly what it declared.
///
/// **Sent whole or not at all.** A job that declares no whole-job memory need
/// is a real state — `herdctl submit -- true` — and the hub says so by
/// sending no `JobNeed` rather than by sending a zero: a runner handed
/// `memory_mib: 0` would have to decide what nought meant, and nought is not
/// a figure (the rule [`Requirements::read`] already enforces on a reading).
/// Nothing sent means *this job was costed at nothing*, and the runner holds
/// it to the ceiling its own owner gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobNeed {
    /// What the whole job needs, in MiB (E-29). Never zero: a job with no
    /// figure carries no `JobNeed` at all.
    pub memory_mib: u64,
    /// What it reserves on the runner's tmpfs while it runs, in MiB (E-31).
    /// Nought is a real answer here, unlike the memory figure.
    pub tmpfs_mib: u64,
}

impl JobNeed {
    /// The kernel ceiling this grant earns, in MiB: both figures, plus
    /// [`CEILING_MARGIN_PERCENT`] of their sum.
    ///
    /// Saturating rather than wrapping, and the saturation is not reachable
    /// from a real declaration: a figure near `u64::MAX` MiB is a submission
    /// no host could satisfy, so the job is refused by admission long before
    /// a runner writes a ceiling for it.
    pub fn ceiling_mib(&self) -> u64 {
        self.memory_mib
            .saturating_add(self.tmpfs_mib)
            .saturating_mul(100 + CEILING_MARGIN_PERCENT)
            / 100
    }

    /// The same figure in bytes, which is the unit `memory.max` is written
    /// in.
    pub fn ceiling_bytes(&self) -> u64 {
        self.ceiling_mib().saturating_mul(1 << 20)
    }

    /// The one line a runner says this grant in: its own log, and the
    /// sentence a host that cannot enforce the ceiling has to print.
    pub fn line(&self) -> String {
        format!(
            "{} MiB of memory and {} MiB of tmpfs, which is {} MiB of ceiling once the \
             {CEILING_MARGIN_PERCENT}% margin is on",
            self.memory_mib,
            self.tmpfs_mib,
            self.ceiling_mib()
        )
    }
}

/// One `<key>=<value>` out of a line, when the line is one.
///
/// The key is one lowercase word: `GROVE_OUT=/data/out` in a shell's own
/// tracing is not an answer, and neither is a sentence with an equals sign in
/// it. See [`Requirements::read`] for why this has to be a shape rather than
/// "every line".
fn answer_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.trim().split_once('=')?;
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    Some((key, value.trim()))
}

/// The labels of an answer: comma-separated names, each one a label.
///
/// An empty value is an empty set — a commit whose tests need no capability in
/// particular is a real answer — and anything that is not a label is refused
/// with the name in it, as a submission's labels are.
fn read_labels(value: &str) -> Result<crate::LabelSet, String> {
    if value.trim().is_empty() {
        return Ok(crate::LabelSet::default());
    }
    let names: Vec<String> = value
        .split(',')
        .map(|name| name.trim().to_owned())
        .collect();
    crate::LabelSet::new(names).map_err(|why| {
        format!("its {LABELS_KEY} line says {value:?}, and that is not a list of labels: {why}")
    })
}

/// One figure of an answer: a whole number of MiB, or of scenarios.
///
/// Nothing is coerced. A figure with a unit in it, a negative one, a decimal
/// one and an empty one are all the same refusal, because each of them is a
/// number Herd would otherwise have had to invent.
fn read_figure(key: &str, value: &str) -> Result<u64, String> {
    value.parse::<u64>().map_err(|_| {
        format!(
            "its {key} line says {value:?}, which is not a whole number: a figure a job is \
             admitted against is never guessed at, so the reading is refused rather than \
             rounded"
        )
    })
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

    /// What a reading prints, in the shape the first consumer's `--labels`
    /// prints it: four keys, one per line (D74). The chatter around it is the
    /// executor's own — a reading's output is a job's output, and their `up`,
    /// `fetch` and `down` hooks all talk.
    const AN_ANSWER: &str = "\
grove: workspace j7 args: --only a b
grove: at cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68
labels=anvil,sprocket
memory_mib=4096
tmpfs_mib=8192
scenarios=37
up j7
fetch cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68
";

    /// The answer, with the whole of it read and the executor's own lines
    /// passed over: a reading runs through the four steps like any other job,
    /// so its output is never only the answer.
    #[test]
    fn an_answer_is_read_out_of_a_job_s_own_output() {
        let read = Requirements::read(AN_ANSWER.as_bytes()).expect("the fixture is an answer");
        assert_eq!(
            read.labels.iter().collect::<Vec<_>>(),
            ["anvil", "sprocket"]
        );
        assert_eq!(read.memory_mib, 4096);
        assert_eq!(read.tmpfs_mib, 8192);
        assert_eq!(read.scenarios, 37);
        assert!(
            read.line().contains("4096 MiB of memory") && read.line().contains("37 scenario"),
            "{}",
            read.line()
        );
    }

    /// D74's first refusal, all four of its ways. Each one names what is
    /// wrong, and none of them answers with a requirement nobody stated.
    #[test]
    fn an_answer_that_is_not_one_is_refused_rather_than_guessed_at() {
        let refusal = |text: &str| match Requirements::read(text.as_bytes()) {
            Ok(read) => panic!("this was read as an answer and is not one: {read:?}"),
            Err(why) => why,
        };

        // An unknown key: `scenario` where `scenarios` was meant. D60's own
        // case, one character wide.
        let unknown = refusal(&AN_ANSWER.replace("scenarios=", "scenario="));
        assert!(
            unknown.contains("\"scenario\"") && unknown.contains(SCENARIOS_KEY),
            "the refusal must name the key and list the four: {unknown}"
        );

        // A missing key.
        let missing = refusal(&AN_ANSWER.replace("memory_mib=4096\n", ""));
        assert!(
            missing.contains(MEMORY_KEY) && missing.contains("a missing key is not a default"),
            "{missing}"
        );

        // A duplicated key, which taking the last value of would make a
        // figure nobody chose.
        let twice = refusal(&AN_ANSWER.replace("tmpfs_mib=8192", "tmpfs_mib=8192\ntmpfs_mib=16"));
        assert!(
            twice.contains("twice") && twice.contains("\"8192\"") && twice.contains("\"16\""),
            "the refusal must name both values: {twice}"
        );

        // A figure that is not one, in each of the ways a tool gets it wrong.
        for said in ["4096 MiB", "4.5", "-1", "", "plenty"] {
            let figure =
                refusal(&AN_ANSWER.replace("memory_mib=4096", &format!("memory_mib={said}")));
            assert!(
                figure.contains(MEMORY_KEY) && figure.contains("not a whole number"),
                "{said:?} was read as a figure: {figure}"
            );
        }

        // And a label that is not a label, which is the same refusal a
        // submission's own labels earn.
        let label = refusal(&AN_ANSWER.replace("anvil,sprocket", "anvil,NOT A LABEL"));
        assert!(label.contains("not a list of labels"), "{label}");

        // Nought where a memory figure was meant: not a job that needs no
        // memory, which is what declaring nothing says.
        let nought = refusal(&AN_ANSWER.replace("memory_mib=4096", "memory_mib=0"));
        assert!(nought.contains("not a figure"), "{nought}");
    }

    /// A commit whose tests need no capability in particular is a real
    /// answer, and nought of tmpfs is a real figure — unlike nought of
    /// memory.
    #[test]
    fn an_answer_may_say_no_capability_and_no_tmpfs() {
        let read =
            Requirements::read(b"labels=\nmemory_mib=512\ntmpfs_mib=0\nscenarios=0\n" as &[u8])
                .expect("an answer that asks for nothing in particular");
        assert!(read.labels.is_empty());
        assert_eq!((read.tmpfs_mib, read.scenarios), (0, 0));
        assert!(
            read.line().contains("no capability in particular"),
            "{}",
            read.line()
        );
    }

    /// What is not key-shaped is the executor talking, and what is not text
    /// at all is not an answer either: neither is guessed at, and neither
    /// refuses a job.
    #[test]
    fn nothing_but_a_key_shaped_line_is_read_as_an_answer() {
        let mut output = b"\xff\xfe not text at all\n".to_vec();
        output.extend_from_slice(b"GROVE_OUT=/data/jobs/j7/out\n");
        output.extend_from_slice(b"= nothing before the equals\n");
        output.extend_from_slice(b"a sentence with = in it\n");
        output.extend_from_slice(AN_ANSWER.as_bytes());
        let read = Requirements::read(&output).expect("the answer is still in there");
        assert_eq!(read.memory_mib, 4096);
    }

    /// The three states of the commit question, and the one that is easiest
    /// to get wrong (D176).
    ///
    /// Silence is **not** a divergence and it is **not** agreement. A type
    /// that collapsed it into either would be the whole defect back again
    /// in a smaller box.
    #[test]
    fn a_reported_revision_is_told_from_silence_and_from_a_divergence() {
        let declared = "cb5a".repeat(10);
        let agreed = CommitRun {
            declared: declared.clone(),
            ran: Some(declared.clone()),
        };
        assert!(!agreed.diverged());
        assert!(agreed.line().contains(&declared));

        let diverged = CommitRun {
            declared: declared.clone(),
            ran: Some("9f41".repeat(10)),
        };
        assert!(diverged.diverged());
        assert!(
            diverged.line().contains("a different revision"),
            "{}",
            diverged.line()
        );

        let silent = CommitRun {
            declared,
            ran: None,
        };
        assert!(!silent.diverged(), "nobody answered, so nobody disagreed");
        assert!(
            silent.line().contains("did not report a revision"),
            "{}",
            silent.line()
        );
    }

    /// Silence is an explicit `null` and never an absent key, so that a
    /// reader cannot miss it by looking only at the keys that are there.
    #[test]
    fn silence_is_written_down_rather_than_left_out() {
        let json = serde_json::to_string(&CommitRun {
            declared: "cb5a".repeat(10),
            ran: None,
        })
        .expect("it serialises");
        assert!(json.contains("\"ran\":null"), "{json}");
    }

    /// A step from a peer that never heard of the field decodes as having
    /// said nothing, which is the only default that is honest.
    #[test]
    fn a_step_with_no_revision_on_it_decodes_as_having_said_nothing() {
        let older = r#"{"step":"fetch","started_unix_ms":1,"duration_ms":2,"exit_code":0}"#;
        let step: StepRun = serde_json::from_str(older).expect("it decodes");
        assert_eq!(step.ran_commit, None);
    }

    #[test]
    fn a_step_s_line_says_its_duration_and_its_code() {
        let ran = StepRun {
            step: Step::Up,
            started_unix_ms: 1_791_878_153_000,
            duration_ms: 95_400,
            exit_code: Some(0),
            ended: None,
            ran_commit: None,
        };
        assert_eq!(ran.line(), "up: 95.400 s, exit 0");
        let capped = StepRun {
            step: Step::Run,
            started_unix_ms: 1_791_878_248_400,
            duration_ms: 2_700_000,
            exit_code: None,
            ended: Some("stopped at the time cap, 2700 s".to_owned()),
            ran_commit: None,
        };
        assert_eq!(
            capped.line(),
            "run: 2700.000 s, no exit code: stopped at the time cap, 2700 s"
        );
    }

    /// The margin is the whole reason a ceiling is a function of the two
    /// figures and not their sum, and the figures here are the consumer's own.
    ///
    /// Their job `j0000000069` died on a Herd job cgroup at
    /// `usage 8388608kB, limit 8388608kB`, of which `shmem 4219834368` —
    /// 3.93 GiB — was tmpfs the job had written, while the killed process's
    /// own `shmem-rss` was 0. A job declaring 4096 MiB of memory and
    /// reserving 4096 MiB of tmpfs declares exactly that 8.00 GiB, so a
    /// ceiling of the sum is the ceiling it died at.
    #[test]
    fn a_ceiling_clears_the_sum_that_killed_j0000000069() {
        let need = JobNeed {
            memory_mib: 4096,
            tmpfs_mib: 4096,
        };
        assert_eq!(
            need.memory_mib + need.tmpfs_mib,
            8192,
            "8192 MiB is the 8.00 GiB limit the kernel reported"
        );
        assert!(
            need.ceiling_mib() > 8192,
            "a ceiling equal to the declared sum is the figure that killed that job, so a \
             margin of nought would ship the defect: {} MiB",
            need.ceiling_mib()
        );
        assert_eq!(need.ceiling_mib(), 9011);
        assert_eq!(need.ceiling_bytes(), 9011 * (1 << 20));
    }

    /// The tmpfs reservation is **inside** the ceiling, not beside it: shmem
    /// is charged to the cgroup that writes it and `memory.swap.max` is 0, so
    /// those pages are part of what the limit counts.
    #[test]
    fn the_tmpfs_reservation_is_inside_the_ceiling() {
        let reserving = JobNeed {
            memory_mib: 4096,
            tmpfs_mib: 4096,
        };
        let reserving_none = JobNeed {
            memory_mib: 4096,
            tmpfs_mib: 0,
        };
        assert_eq!(reserving_none.ceiling_mib(), 4505);
        assert!(
            reserving.ceiling_mib() >= reserving_none.ceiling_mib() + 4096,
            "a job that reserves 4096 MiB of tmpfs must be allowed all of it on top of its \
             memory figure: {} MiB against {} MiB",
            reserving.ceiling_mib(),
            reserving_none.ceiling_mib()
        );
    }

    /// A figure no machine could serve **saturates**, and the direction is
    /// the point: a wrapped ceiling is a *small* number, which is the one
    /// failure of this arithmetic that would be silent.
    #[test]
    fn an_impossible_figure_saturates_rather_than_wrapping() {
        let absurd = JobNeed {
            memory_mib: u64::MAX,
            tmpfs_mib: u64::MAX,
        };
        assert_eq!(absurd.ceiling_mib(), u64::MAX / 100);
        assert_eq!(absurd.ceiling_bytes(), u64::MAX);
    }

    /// The line a runner prints names both declared figures and the ceiling
    /// they earn, because an operator reading one figure cannot tell which of
    /// the three it is.
    #[test]
    fn the_line_names_both_figures_and_the_ceiling() {
        let said = JobNeed {
            memory_mib: 4096,
            tmpfs_mib: 4096,
        }
        .line();
        assert_eq!(
            said,
            "4096 MiB of memory and 4096 MiB of tmpfs, which is 9011 MiB of ceiling once the \
             10% margin is on"
        );
    }
}
