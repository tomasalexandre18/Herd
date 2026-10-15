//! The declaration file in the requester's own checkout: the commands that
//! checkout declares, and one of them resolved into what a submission carries
//! (E-11, D106).
//!
//! One TOML file, `tools/herd/commands.toml`, read **here** — in `herdctl`,
//! where the repository already is. The hub and a runner never see TOML and
//! hold no repository: what travels is [`herd_proto::Declaration`], the
//! declaration resolved, with the placeholders only the requester could fill
//! already filled (D107).
//!
//! Two halves, and the split is the whole module:
//!
//! * [`Commands`] is the file, parsed and checked — every label is a label,
//!   every priority is a level, and no command says two things about one
//!   question. A file is refused as a whole, naming the command and the key:
//!   it is the contract of a repository, and a typo in one command is a defect
//!   whoever edits the file wants to hear about, not a landmine for whoever
//!   submits a different one next week.
//! * [`Resolved`] is one of its commands, for one commit and one requester's
//!   arguments: the declaration a submission carries, the declared line, the
//!   figures, the labels, and everything M11 and M12 will read off the record.
//!
//! **Herd's code and docs name no consumer command (E-11).** Nothing below —
//! and no test in it — knows a single name out of the consumer's file. The
//! *shape* is theirs; the names in the fixtures are invented here, which is
//! also what makes the acceptance test meaningful: a fourth command is a
//! fourth table, not a change to this file.
//!
//! This is plain synchronous work — one file read and some string
//! substitution — and it stays synchronous while the rest of `herdctl` is
//! async, for `checkout`'s reason: there is nothing here to wait on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use herd_proto::{
    args_as_line, Declaration, Executor, LabelSet, Priority, ARGS_TOKEN, COMMIT_TOKEN,
};
use serde::Deserialize;

/// Where a checkout declares the commands Herd may run for it, relative to
/// the work tree root.
///
/// The consumer's own path, and not configurable: a submission that had to be
/// told where the declaration lives would be a submission that could be
/// pointed at another repository's.
pub const DECLARATION_PATH: &str = "tools/herd/commands.toml";

/// The word a command uses to say "whatever level the submitter asks for".
///
/// Not one of [`Priority`]'s four on purpose: it is the absence of a fixed
/// level, and a command that said `step` would be a command a merger could not
/// put in front of the queue.
pub const REQUESTER_PRIORITY: &str = "requester";

/// What a command declares about the level its jobs are weighed at (E-18).
///
/// Two things a caller must be able to tell apart: a level the declaration
/// fixes, and a command that leaves the level to whoever submits it. Absent
/// from the file means [`DeclaredPriority::Requester`], which is what nine of
/// the first consumer's ten commands want.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredPriority {
    /// The level the submitter asks for. The declaration says nothing.
    Requester,
    /// The one level this command is always weighed at, whoever submits it.
    Fixed(Priority),
}

impl DeclaredPriority {
    /// The level a job of this command is weighed at, given the level the
    /// requester asked for.
    ///
    /// A fixed level wins: it is the declaration's statement about its own
    /// command — the cache producer may always give way, however eager the
    /// thing that submitted it.
    pub fn or(self, asked: Priority) -> Priority {
        match self {
            DeclaredPriority::Requester => asked,
            DeclaredPriority::Fixed(level) => level,
        }
    }

    /// The level the declaration fixed, or `None` when it fixed none.
    pub fn fixed(self) -> Option<Priority> {
        match self {
            DeclaredPriority::Requester => None,
            DeclaredPriority::Fixed(level) => Some(level),
        }
    }

    /// What the file said, as the file spells it.
    pub fn name(self) -> &'static str {
        match self {
            DeclaredPriority::Requester => REQUESTER_PRIORITY,
            DeclaredPriority::Fixed(level) => level.name(),
        }
    }

    /// The word, read strictly: `requester`, or one of the four levels.
    ///
    /// The refusal lists all five names, because a person who typed one of
    /// them wrong needs the list and not a second guess.
    fn read(named: &str) -> Result<DeclaredPriority, String> {
        if named == REQUESTER_PRIORITY {
            return Ok(DeclaredPriority::Requester);
        }
        named
            .parse::<Priority>()
            .map(DeclaredPriority::Fixed)
            .map_err(|_| {
                format!(
                    "{named:?} is not a priority; it is {REQUESTER_PRIORITY:?}, or one of {}",
                    Priority::names()
                )
            })
    }
}

/// Where a command's labels come from (D74).
///
/// Two cases, and a caller must not confuse them. Without `labels_from`, the
/// declared labels **are** the job's. With it, the labels the job really needs
/// are only known once a reading has run on a machine, and the declared labels
/// are the **baseline** the reading job itself is placed against — the first
/// consumer's label-reading runs their own tool, so it needs their build
/// capability to run at all, and it is an ordinary job like any other.
///
/// Both absent is legal and is [`Labels::Declared`] of an empty set: a job
/// that needs no capability in particular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Labels {
    /// The job's own labels, as the declaration states them.
    Declared(LabelSet),
    /// The labels come from a reading, which has not happened yet.
    Read {
        /// The shell line that prints them, with `{commit}` and `{args}`
        /// filled like any other line.
        line: String,
        /// The labels the reading job itself is placed against (D74).
        baseline: LabelSet,
    },
}

impl Labels {
    /// The job's own labels, when the declaration states them, and `None`
    /// when they come from a reading that has not run.
    ///
    /// `None` is the whole point: a caller that wants the job's real
    /// requirements and gets `None` has to go and read them (D74), rather
    /// than place a job against a baseline that was never its own.
    pub fn declared(&self) -> Option<&LabelSet> {
        match self {
            Labels::Declared(labels) => Some(labels),
            Labels::Read { .. } => None,
        }
    }

    /// The labels the job a submission makes carries **now**: the declared
    /// ones, or the baseline the reading job is placed against.
    pub fn at_submission(&self) -> &LabelSet {
        match self {
            Labels::Declared(labels) => labels,
            Labels::Read { baseline, .. } => baseline,
        }
    }

    /// The line that reads the labels, when there is one.
    pub fn reading(&self) -> Option<&str> {
        match self {
            Labels::Declared(_) => None,
            Labels::Read { line, .. } => Some(line),
        }
    }
}

/// When a command wants to be submitted without anybody asking (E-41).
///
/// Parsed, checked for shape and carried; **acted on by nothing yet** — the
/// scheduler is a later milestone, and a declaration whose schedule this
/// module quietly dropped would be a nightly that never ran and said nothing.
/// The two times are the consumer's own strings, kept as written: turning
/// `03:00` into an instant needs a day and a time zone, neither of which the
/// file names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    /// The local time of day the pass is wanted at, as written. Required: a
    /// schedule that does not say when is not a schedule.
    pub at: String,
    /// The local time of day after which the pass is no longer worth
    /// starting, as written (E-09's deadline, in the file's own words).
    #[serde(default)]
    pub deadline: Option<String>,
    /// The ref whose having moved is the condition of the pass.
    #[serde(default)]
    pub if_moved: Option<String>,
}

/// One declared command, resolved for one commit and one requester's
/// arguments: everything a submission needs, with the file no longer in the
/// picture.
///
/// This is what the CLI builds a [`herd_proto::Asked`] out of — through
/// [`Resolved::command`], [`Resolved::declared`] and [`Resolved::priority`],
/// so the submission path that already exists takes it unchanged and nothing
/// re-reads the file.
///
/// Every string in it has had `{args}` and `{commit}` filled and **nothing
/// else**: `{job}`, `{in}`, `{out}` and `{line}` are the runner's to fill and
/// `{j}` is the hub's (D107), so they are still here, as text, deliberately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What travels on the submission and is kept on the record: the
    /// command's name, the commit, the arguments as typed, and the four
    /// executor hooks with `{args}` and `{commit}` filled.
    pub declaration: Declaration,
    /// The declared command line, filled, as it will be pasted into the `run`
    /// hook's `{line}`.
    ///
    /// Carried beside the declaration rather than in it because that is where
    /// the protocol puts it: it is [`herd_proto::Asked::command`], the line
    /// the job *is*, and the hub hands it to the runner as such.
    pub line: String,
    /// The capabilities this job needs, or the reading that will say (D74).
    pub labels: Labels,
    /// How much memory the whole job needs, in MiB (E-29).
    pub memory_mib: Option<u64>,
    /// How much memory one of its build threads needs, in MiB (E-22).
    pub build_thread_mib: Option<u32>,
    /// How much room on the runner's tmpfs it reserves while it runs, in MiB
    /// (E-31). `Some(0)` is a real declaration and not an absence.
    pub tmpfs_mib: Option<u64>,
    /// How long its `run` step may take, in seconds — the declared figure,
    /// which the runner's own cap bounds and never raises (D108).
    pub timeout_s: Option<u64>,
    /// The level it is weighed at, or the requester's own (E-18).
    pub priority: DeclaredPriority,
    /// The keys of the artefacts this job consumes, which a runner lays out
    /// under `{in}` (M11).
    pub needs: Vec<String>,
    /// The artefacts to keep when it is over: a key, and the path under
    /// `{out}` it is left at (M11).
    pub leaves: BTreeMap<String, String>,
    /// The key a queued job of the same key is dropped for when a newer one
    /// arrives (E-42), filled.
    pub coalesce: Option<String>,
    /// The key two submissions are one submission by (E-05, E-42), filled.
    pub dedup_key: Option<String>,
    /// When it wants to be submitted on its own (E-41).
    pub schedule: Option<Schedule>,
}

impl Resolved {
    /// What a submission carries as its command: the declared line, as one
    /// shell line.
    ///
    /// One element, never split into an argv: a declared line is handed to one
    /// shell whole, which is the consumer's own rule — "you never interpret a
    /// command line" — and the line here already holds their quoting and
    /// their `&&`.
    pub fn command(&self) -> Vec<String> {
        vec![self.line.clone()]
    }

    /// What this command declares about one job's resources, as the
    /// [`crate::Declared`] the submission path already takes.
    ///
    /// Three fields are deliberately left at their default, because they are
    /// the **requester's** and not the declaration's: the deadline, the parent
    /// and the requeue exit code. A caller fills those in from what the person
    /// typed.
    pub fn declared(&self) -> crate::Declared {
        crate::Declared {
            build_thread_mib: self.build_thread_mib,
            memory_mib: self.memory_mib,
            tmpfs_mib: self.tmpfs_mib,
            declaration: Some(self.declaration.clone()),
            timeout_s: self.timeout_s,
            ..crate::Declared::default()
        }
    }

    /// The level to weigh this job at, given the level the requester asked
    /// for (E-18).
    pub fn priority(&self, asked: Priority) -> Priority {
        self.priority.or(asked)
    }
}

/// The declaration file, parsed and checked: the executor every command runs
/// through, and the commands themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commands {
    path: PathBuf,
    executor: Executor,
    commands: BTreeMap<String, Command>,
}

/// One command as the file declares it: checked, and not yet resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Command {
    line: String,
    labels: Labels,
    memory_mib: Option<u64>,
    build_thread_mib: Option<u32>,
    tmpfs_mib: Option<u64>,
    timeout_s: Option<u64>,
    priority: DeclaredPriority,
    needs: Vec<String>,
    leaves: BTreeMap<String, String>,
    coalesce: Option<String>,
    dedup_key: Option<String>,
    schedule: Option<Schedule>,
}

impl Commands {
    /// The declaration of the checkout rooted at `root`.
    pub fn read_in(root: &Path) -> Result<Commands> {
        Commands::read_from(&root.join(DECLARATION_PATH))
    }

    /// The declaration at one path.
    ///
    /// A missing file is not an empty one: it is refused with the path it
    /// looked for and what the file is for, because the alternative is
    /// "no such command" about a file nobody has written.
    pub fn read_from(path: &Path) -> Result<Commands> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => bail!(
                "no declaration at {}: {DECLARATION_PATH} is where a checkout declares the \
                 commands Herd may run for it, so add it there — or submit a command line of \
                 your own instead",
                path.display()
            ),
            Err(why) => {
                return Err(why).with_context(|| {
                    format!(
                        "could not read the declaration at {}: herdctl reads it in your own \
                         checkout, where the repository already is",
                        path.display()
                    )
                })
            }
        };
        Commands::parse(&text, path)
    }

    /// The declaration, from the text of the file at `path`.
    ///
    /// `path` is carried for the refusals alone — every one of them says which
    /// file it is about, since a requester may be standing in a checkout whose
    /// declaration it has never read.
    pub fn parse(text: &str, path: &Path) -> Result<Commands> {
        // Two reads of one small file, so the two refusals keep their own
        // voices: TOML that does not parse at all is a different thing from
        // TOML that parses into something this is not. Both carry toml's own
        // message, which names the line and the column.
        if let Err(why) = text.parse::<toml::Table>() {
            bail!("{} is not valid TOML: {why}", path.display());
        }
        let file: File = toml::from_str(text)
            .map_err(|why| anyhow::anyhow!("{} is not a declaration: {why}", path.display()))?;
        let executor = Executor {
            up: file.executor.up,
            fetch: file.executor.fetch,
            run: file.executor.run,
            down: file.executor.down,
            env: file.executor.env,
        };
        let mut commands = BTreeMap::new();
        for (name, held) in file.commands {
            // Deserialized one command at a time so that a refusal can say
            // **which** command declared the key it is refusing. A single pass
            // over the whole file would name the key and leave the reader to
            // count tables.
            let declared = CommandFile::deserialize(held).map_err(|why| {
                anyhow::anyhow!("{} cannot declare command {name:?}: {why}", path.display())
            })?;
            let command = declared.checked(&name, path)?;
            commands.insert(name, command);
        }
        Ok(Commands {
            path: path.to_owned(),
            executor,
            commands,
        })
    }

    /// Which file this declaration was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every command the file declares, sorted.
    pub fn names(&self) -> Vec<&str> {
        self.commands.keys().map(String::as_str).collect()
    }

    /// The four hooks every command runs through, as declared — nothing
    /// filled.
    pub fn executor(&self) -> &Executor {
        &self.executor
    }

    /// One declared command, resolved for one commit and one requester's
    /// arguments.
    ///
    /// `commit` is the hash [`crate::checkout`] resolved in the requester's
    /// own checkout: forty lowercase hex, and nothing is resolved here. What
    /// this does is **fill `{args}` and `{commit}`, and only those two**, in
    /// the four hooks, the environment, the command's own line and the two
    /// keys of E-42 — because the requester is the one place that knows them
    /// (D107), and because a line still holding `{args}` when the job is
    /// minted is a line two submissions with different arguments would be
    /// deduplicated into one by.
    ///
    /// `{job}`, `{in}`, `{out}` and `{line}` are left alone: they are the
    /// runner's, and `{j}` is the hub's.
    pub fn resolve(&self, named: &str, commit: &str, args: &[String]) -> Result<Resolved> {
        let Some(command) = self.commands.get(named) else {
            bail!("{}", self.no_such_command(named));
        };
        if !is_whole_hash(commit) {
            bail!(
                "command {named:?} was asked for commit {commit:?}, which is not forty \
                 lowercase hex: a submission carries the hash its own checkout resolved, never \
                 a ref for a machine to resolve"
            );
        }
        // One pass per token, in this order, and the order is the rule: the
        // commit goes in first, so an argument that itself says `{commit}` is
        // an argument and never a template. Nothing here loops until it stops
        // changing — that is how a requester's own string would become one
        // (D107).
        let line = args_as_line(args);
        let fill = |text: &str| {
            text.replace(COMMIT_TOKEN, commit)
                .replace(ARGS_TOKEN, &line)
        };
        Ok(Resolved {
            declaration: Declaration {
                command: named.to_owned(),
                commit: commit.to_owned(),
                args: args.to_vec(),
                executor: self
                    .executor
                    .filled(COMMIT_TOKEN, commit)
                    .filled(ARGS_TOKEN, &line),
            },
            line: fill(&command.line),
            labels: match &command.labels {
                Labels::Declared(labels) => Labels::Declared(labels.clone()),
                Labels::Read {
                    line: reading,
                    baseline,
                } => Labels::Read {
                    line: fill(reading),
                    baseline: baseline.clone(),
                },
            },
            memory_mib: command.memory_mib,
            build_thread_mib: command.build_thread_mib,
            tmpfs_mib: command.tmpfs_mib,
            timeout_s: command.timeout_s,
            priority: command.priority,
            needs: command.needs.clone(),
            leaves: command.leaves.clone(),
            coalesce: command.coalesce.as_deref().map(fill),
            dedup_key: command.dedup_key.as_deref().map(fill),
            schedule: command.schedule.clone(),
        })
    }

    /// One sentence for a name the file does not declare, listing the names it
    /// does — sorted, because that is the list a person who mistyped one
    /// reads.
    fn no_such_command(&self, named: &str) -> String {
        let path = self.path.display();
        if self.commands.is_empty() {
            return format!(
                "{named:?} is not a command {path} declares: it declares no command at all"
            );
        }
        format!(
            "{named:?} is not a command {path} declares; it declares {}",
            self.names().join(", ")
        )
    }
}

/// Exactly forty lowercase hex characters, and nothing else.
fn is_whole_hash(hash: &str) -> bool {
    hash.len() == 40
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// The file's own shape. Unknown keys are refused by name, here and in every
/// table below: a declaration Herd read past is a declaration whose author
/// believes something Herd never saw.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    executor: ExecutorFile,
    /// Held as raw TOML so that each one is deserialized on its own, with its
    /// name to hand for the refusal.
    #[serde(default)]
    commands: BTreeMap<String, toml::Value>,
}

/// The `[executor]` table. A mirror of [`Executor`] rather than that type
/// itself, so that an unknown hook is refused by name — the protocol's type is
/// the resolved shape and is not the file's parser.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorFile {
    up: String,
    fetch: String,
    run: String,
    down: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// One `[commands.<name>]` table, as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandFile {
    line: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    labels_from: Option<String>,
    #[serde(default)]
    memory_mib: Option<u64>,
    #[serde(default)]
    build_thread_mib: Option<u32>,
    #[serde(default)]
    tmpfs_mib: Option<u64>,
    #[serde(default)]
    timeout_s: Option<u64>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    needs: Vec<String>,
    #[serde(default)]
    leaves: BTreeMap<String, String>,
    #[serde(default)]
    coalesce: Option<String>,
    #[serde(default)]
    dedup_key: Option<String>,
    #[serde(default)]
    schedule: Option<Schedule>,
}

impl CommandFile {
    /// The command, checked: every label a label, the priority one of five
    /// words, and one answer per question.
    ///
    /// Checked at parse rather than at resolution, and the file is refused as
    /// a whole: the declaration is a repository's contract, so a label that is
    /// not a label is a defect in the file whoever edits it wants told at
    /// once, not one that waits until somebody submits that particular
    /// command.
    fn checked(self, name: &str, path: &Path) -> Result<Command> {
        let at = format!("command {name:?} in {}", path.display());
        if self.memory_mib.is_some() && self.build_thread_mib.is_some() {
            bail!(
                "{at} declares both memory_mib and build_thread_mib: a figure for the whole job \
                 and a figure for one of its build threads are two answers to one question, so \
                 declare the one this command means"
            );
        }
        let declared = LabelSet::new(self.labels)
            .map_err(|why| anyhow::anyhow!("{at} declares a label it cannot have: {why}"))?;
        let labels = match self.labels_from {
            // With a reading, the declared labels are the baseline the
            // reading job itself is placed against (D74) — not the labels of
            // the job that follows it.
            Some(line) => Labels::Read {
                line,
                baseline: declared,
            },
            None => Labels::Declared(declared),
        };
        let priority = match self.priority.as_deref() {
            Some(named) => DeclaredPriority::read(named)
                .map_err(|why| anyhow::anyhow!("{at} declares a priority it cannot have: {why}"))?,
            None => DeclaredPriority::Requester,
        };
        Ok(Command {
            line: self.line,
            labels,
            memory_mib: self.memory_mib,
            build_thread_mib: self.build_thread_mib,
            tmpfs_mib: self.tmpfs_mib,
            timeout_s: self.timeout_s,
            priority,
            needs: self.needs,
            leaves: self.leaves,
            coalesce: self.coalesce,
            dedup_key: self.dedup_key,
            schedule: self.schedule,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herd_proto::{Step, IN_TOKEN, JOB_TOKEN, LINE_TOKEN, OUT_TOKEN, PARALLELISM_TOKEN};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A commit hash: forty lowercase hex, as `checkout` resolves them.
    const COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";

    /// The path every refusal in these tests names. A path and not a file:
    /// the tests that are about the text build the text themselves, and only
    /// the two tests about the *file* touch a disk.
    fn a_path() -> PathBuf {
        PathBuf::from("/a/checkout").join(DECLARATION_PATH)
    }

    /// The `[executor]` table these fixtures share: the shape the consumer
    /// asked for — four shell lines and an environment — carrying every token
    /// Herd fills, and no consumer command in it (E-11).
    const EXECUTOR: &str = r#"
[executor]
up    = "wrap up {job} {commit}"
fetch = "wrap in {job} -- 'fetch origin && switch --detach {commit}'"
run   = "wrap in {job} -- {line}"
down  = "wrap down {job} --purge"
env   = { FINDINGS = "{out}/findings.json", ARGUMENTS = "{args}" }
"#;

    /// Two declared commands, every field of the contract used at least once
    /// between them. The names are invented here: Herd's own code and tests
    /// name no consumer command (E-11).
    const KNEAD: &str = r#"
[commands.knead]
line             = "shape --into {out}/dough {args}"
labels           = ["anvil", "sprocket"]
build_thread_mib = 1792
tmpfs_mib        = 0
timeout_s        = 2700
leaves           = { dough = "dough" }
"#;

    const PROOF: &str = r#"
[commands.proof]
line        = "watch --rise {in}/dough -j {j} {args}"
labels      = ["anvil"]
labels_from = "watch --rise {in}/dough {args} --labels"
memory_mib  = 3584
tmpfs_mib   = 4096
timeout_s   = 7200
priority    = "nightly"
needs       = ["dough"]
coalesce    = "proof:main"
dedup_key   = "proof:{commit}"

[commands.proof.schedule]
at       = "03:00"
deadline = "05:00"
if_moved = "refs/heads/main"
"#;

    /// The third command E-11's acceptance adds: nothing but a third table.
    const GLAZE: &str = r#"
[commands.glaze]
line     = "coat {out}/glaze {args}"
priority = "producer"
"#;

    /// The two-command file, parsed. The input is the text above and nothing
    /// else, so every assertion below is about state this test established
    /// (D100).
    fn two_commands() -> Commands {
        let text = format!("{EXECUTOR}{KNEAD}{PROOF}");
        Commands::parse(&text, &a_path()).expect("the fixture is a declaration")
    }

    /// What a refusal said, for a test that wants the sentence rather than the
    /// type.
    fn refusal(result: Result<impl std::fmt::Debug>) -> String {
        match result {
            Ok(got) => panic!("this was accepted and should not have been: {got:?}"),
            Err(why) => format!("{why:#}"),
        }
    }

    #[test]
    fn a_file_declaring_two_commands_declares_both_and_their_executor() {
        let commands = two_commands();
        assert_eq!(commands.names(), ["knead", "proof"]);
        assert_eq!(commands.path(), a_path());
        let executor = commands.executor();
        assert_eq!(executor.hook(Step::Up), "wrap up {job} {commit}");
        assert_eq!(executor.hook(Step::Run), "wrap in {job} -- {line}");
        assert_eq!(executor.hook(Step::Down), "wrap down {job} --purge");
        assert_eq!(
            executor.env.get("FINDINGS").map(String::as_str),
            Some("{out}/findings.json"),
            "the declared executor is the file's own text, nothing filled"
        );
    }

    /// Every field of one command reaching the resolved form, by value.
    #[test]
    fn every_declared_field_reaches_the_resolved_command() {
        let resolved = two_commands()
            .resolve("proof", COMMIT, &[])
            .expect("proof is declared by the fixture");
        assert_eq!(resolved.declaration.command, "proof");
        assert_eq!(resolved.declaration.commit, COMMIT);
        assert_eq!(resolved.declaration.args, Vec::<String>::new());
        assert_eq!(resolved.memory_mib, Some(3584));
        assert_eq!(resolved.build_thread_mib, None);
        assert_eq!(resolved.tmpfs_mib, Some(4096));
        assert_eq!(resolved.timeout_s, Some(7200));
        assert_eq!(
            resolved.priority,
            DeclaredPriority::Fixed(Priority::Nightly)
        );
        assert_eq!(resolved.needs, ["dough"]);
        assert_eq!(resolved.leaves, BTreeMap::new());
        assert_eq!(resolved.coalesce.as_deref(), Some("proof:main"));
        assert_eq!(
            resolved.schedule,
            Some(Schedule {
                at: "03:00".to_owned(),
                deadline: Some("05:00".to_owned()),
                if_moved: Some("refs/heads/main".to_owned()),
            })
        );

        let other = two_commands()
            .resolve("knead", COMMIT, &[])
            .expect("knead is declared by the fixture");
        assert_eq!(other.build_thread_mib, Some(1792));
        assert_eq!(other.memory_mib, None);
        assert_eq!(other.timeout_s, Some(2700));
        assert_eq!(other.priority, DeclaredPriority::Requester);
        assert_eq!(other.needs, Vec::<String>::new());
        assert_eq!(
            other.leaves,
            BTreeMap::from([("dough".to_owned(), "dough".to_owned())])
        );
        assert_eq!(other.coalesce, None);
        assert_eq!(other.dedup_key, None);
        assert_eq!(other.schedule, None);
        assert_eq!(
            other.labels.declared().map(LabelSet::to_string),
            Some("anvil, sprocket".to_owned())
        );
    }

    /// The two tokens the requester fills, filled in all four places they can
    /// be: the hooks, the environment, the command's own line, and E-42's two
    /// keys (D107).
    #[test]
    fn the_commit_and_the_arguments_are_filled_in_the_hooks_the_environment_and_the_line() {
        let args = vec!["--only".to_owned(), "one,two".to_owned()];
        let resolved = two_commands()
            .resolve("proof", COMMIT, &args)
            .expect("proof is declared by the fixture");
        let executor = &resolved.declaration.executor;
        assert_eq!(
            executor.hook(Step::Up),
            format!("wrap up {JOB_TOKEN} {COMMIT}")
        );
        assert_eq!(
            executor.hook(Step::Fetch),
            format!("wrap in {JOB_TOKEN} -- 'fetch origin && switch --detach {COMMIT}'")
        );
        assert_eq!(
            executor.env.get("ARGUMENTS").map(String::as_str),
            Some("'--only' 'one,two'"),
            "an environment value is a line like any other"
        );
        assert_eq!(
            resolved.line,
            format!("watch --rise {IN_TOKEN}/dough -j {PARALLELISM_TOKEN} '--only' 'one,two'")
        );
        assert_eq!(
            resolved.labels.reading(),
            Some(format!("watch --rise {IN_TOKEN}/dough '--only' 'one,two' --labels").as_str()),
            "the reading runs the same arguments, so it is filled too"
        );
        assert_eq!(
            resolved.dedup_key.as_deref(),
            Some(format!("proof:{COMMIT}").as_str()),
            "a key that does not carry the commit would make two commits one job (E-05)"
        );
        assert_eq!(
            resolved.coalesce.as_deref(),
            Some("proof:main"),
            "a key with no token in it is carried as written"
        );
    }

    /// The four tokens this module must **not** fill, and Herd's own `{j}`:
    /// they belong to the runner and the hub (D107), and a declaration that
    /// reached the wire without them would be a declaration nobody could run.
    #[test]
    fn the_tokens_the_runner_and_the_hub_fill_are_left_alone() {
        let resolved = two_commands()
            .resolve("knead", COMMIT, &["--fast".to_owned()])
            .expect("knead is declared by the fixture");
        let executor = &resolved.declaration.executor;
        let hooks: Vec<&str> = Step::ALL.iter().map(|step| executor.hook(*step)).collect();
        assert!(
            hooks.iter().all(|hook| hook.contains(JOB_TOKEN)),
            "{hooks:?} — every hook of the fixture names {JOB_TOKEN}"
        );
        assert!(
            executor.hook(Step::Run).contains(LINE_TOKEN),
            "the run hook still has to be given the line: {:?}",
            executor.hook(Step::Run)
        );
        assert_eq!(
            executor.env.get("FINDINGS").map(String::as_str),
            Some("{out}/findings.json"),
            "the runner's own directory is not the requester's to know"
        );
        assert!(
            resolved.line.contains(OUT_TOKEN),
            "{:?} — the line's {OUT_TOKEN} survives",
            resolved.line
        );
        // And the one Herd fills itself, on the other command's line.
        let other = two_commands()
            .resolve("proof", COMMIT, &[])
            .expect("proof is declared by the fixture");
        assert!(
            other.line.contains(PARALLELISM_TOKEN) && other.line.contains(IN_TOKEN),
            "{:?} — {PARALLELISM_TOKEN} is the hub's and {IN_TOKEN} is the runner's",
            other.line
        );
        // Said once more as the protocol itself says it: nothing is left
        // except the five this module does not fill.
        let mut left = executor.tokens_left();
        left.sort_unstable();
        assert_eq!(left, [JOB_TOKEN, LINE_TOKEN, OUT_TOKEN]);
    }

    /// An argument with a space in it is one word when the shell gets it, and
    /// an argument that itself names a token is text and never a template
    /// (D107's one-pass rule).
    #[test]
    fn an_argument_is_one_quoted_word_and_a_token_inside_one_stays_text() {
        let args = vec![
            "--note".to_owned(),
            "two words".to_owned(),
            format!("--where={OUT_TOKEN}"),
            format!("--at={COMMIT_TOKEN}"),
        ];
        let resolved = two_commands()
            .resolve("knead", COMMIT, &args)
            .expect("knead is declared by the fixture");
        assert_eq!(
            resolved.line,
            format!(
                "shape --into {OUT_TOKEN}/dough '--note' 'two words' '--where={OUT_TOKEN}' \
                 '--at={COMMIT_TOKEN}'"
            ),
            "one quoted word per argument, and neither brace was filled"
        );
        assert!(
            !resolved.line.contains(COMMIT),
            "{:?} — an argument saying {COMMIT_TOKEN} must not become the commit",
            resolved.line
        );
        assert_eq!(
            resolved.declaration.args, args,
            "the arguments travel as typed as well, for the reading's cache key (D74)"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_naming_the_key_and_the_command() {
        let text = format!("{EXECUTOR}{KNEAD}\nmemory_mb = 3584\n");
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("memory_mb"), "{said}");
        assert!(said.contains("\"knead\""), "{said}");
        assert!(said.contains(DECLARATION_PATH), "{said}");
    }

    #[test]
    fn an_unknown_key_in_the_executor_is_refused_naming_it() {
        let text = format!("{EXECUTOR}sideways = \"wrap sideways\"\n{KNEAD}");
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("sideways"), "{said}");
        assert!(said.contains("is not a declaration"), "{said}");
    }

    #[test]
    fn an_unknown_command_is_refused_listing_the_names_the_file_declares() {
        let commands = two_commands();
        let said = refusal(commands.resolve("kneed", COMMIT, &[]));
        assert!(said.contains("\"kneed\""), "{said}");
        assert!(
            said.contains("it declares knead, proof"),
            "{said} — sorted, which is the list a person who mistyped reads"
        );
    }

    #[test]
    fn a_file_declaring_no_command_says_so_rather_than_listing_nothing() {
        let commands = Commands::parse(EXECUTOR, &a_path()).expect("an executor is a declaration");
        assert_eq!(commands.names(), Vec::<&str>::new());
        let said = refusal(commands.resolve("knead", COMMIT, &[]));
        assert!(said.contains("it declares no command at all"), "{said}");
    }

    #[test]
    fn a_command_with_no_line_is_refused_naming_the_command() {
        let text = format!("{EXECUTOR}\n[commands.knead]\nlabels = [\"anvil\"]\n");
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("missing field `line`"), "{said}");
        assert!(said.contains("\"knead\""), "{said}");
    }

    #[test]
    fn a_command_declaring_both_memory_figures_is_refused_naming_the_command() {
        let text = format!(
            "{EXECUTOR}\n[commands.knead]\nline = \"shape\"\nmemory_mib = 3584\n\
             build_thread_mib = 1792\n"
        );
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("\"knead\""), "{said}");
        assert!(
            said.contains("declares both memory_mib and build_thread_mib"),
            "{said}"
        );
        // Either on its own is fine, which is what makes the refusal about
        // the pair and not about the fields.
        for one in ["memory_mib = 3584", "build_thread_mib = 1792"] {
            let text = format!("{EXECUTOR}\n[commands.knead]\nline = \"shape\"\n{one}\n");
            Commands::parse(&text, &a_path()).expect("one figure answers one question");
        }
    }

    #[test]
    fn a_label_that_is_not_a_label_is_refused_naming_the_command() {
        let text =
            format!("{EXECUTOR}\n[commands.knead]\nline = \"shape\"\nlabels = [\"Anvil\"]\n");
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("\"knead\""), "{said}");
        assert!(said.contains("label it cannot have"), "{said}");
        assert!(
            said.contains("\"Anvil\"") && said.contains("lower-case"),
            "{said} — the protocol's own complaint, carried"
        );
    }

    /// A baseline label goes through the same door: a reading job is an
    /// ordinary job and is placed against labels like any other (D74).
    #[test]
    fn a_baseline_label_that_is_not_a_label_is_refused_too() {
        let text = format!(
            "{EXECUTOR}\n[commands.knead]\nline = \"shape\"\nlabels = [\"anvil!\"]\n\
             labels_from = \"shape --labels\"\n"
        );
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(said.contains("\"knead\"") && said.contains("'!'"), "{said}");
    }

    /// With `labels_from`, the declared labels are the baseline and the job's
    /// own labels are not yet known; without it, they are the job's (D74).
    #[test]
    fn a_reading_makes_the_declared_labels_a_baseline_and_not_the_job_s() {
        let commands = two_commands();
        let reading = commands
            .resolve("proof", COMMIT, &[])
            .expect("proof declares labels_from");
        assert_eq!(
            reading.labels.declared(),
            None,
            "the labels of the job that follows the reading are not known yet"
        );
        assert_eq!(
            reading.labels.at_submission().to_string(),
            "anvil",
            "what the reading job itself is placed against"
        );
        assert_eq!(
            reading.labels.reading(),
            Some("watch --rise {in}/dough  --labels")
        );

        let stated = commands
            .resolve("knead", COMMIT, &[])
            .expect("knead declares labels of its own");
        assert_eq!(stated.labels.reading(), None);
        assert_eq!(
            stated.labels.declared().map(LabelSet::to_string),
            Some("anvil, sprocket".to_owned())
        );
        assert_eq!(
            stated.labels.at_submission(),
            stated.labels.declared().expect("stated labels"),
            "with no reading, what a submission carries is the job's own set"
        );
    }

    /// Neither key is a job that needs nothing refused: it is a job that needs
    /// no capability in particular.
    #[test]
    fn a_command_naming_no_label_at_all_is_a_job_that_needs_none() {
        let text = format!("{EXECUTOR}{GLAZE}");
        let resolved = Commands::parse(&text, &a_path())
            .expect("a command may need no capability")
            .resolve("glaze", COMMIT, &[])
            .expect("glaze is declared by the fixture");
        assert_eq!(resolved.labels, Labels::Declared(LabelSet::default()));
        assert!(resolved.labels.at_submission().is_empty());
    }

    /// `0` is a declaration — "this job needs no tmpfs" — and absence is a
    /// job admission has no tmpfs figure for. The two must not collapse.
    #[test]
    fn a_tmpfs_figure_of_zero_is_a_declaration_and_not_an_absence() {
        let zero = two_commands()
            .resolve("knead", COMMIT, &[])
            .expect("knead declares tmpfs_mib = 0");
        assert_eq!(zero.tmpfs_mib, Some(0));
        let text = format!("{EXECUTOR}{GLAZE}");
        let absent = Commands::parse(&text, &a_path())
            .expect("glaze declares no tmpfs")
            .resolve("glaze", COMMIT, &[])
            .expect("glaze is declared by the fixture");
        assert_eq!(absent.tmpfs_mib, None);
        assert_ne!(zero.tmpfs_mib, absent.tmpfs_mib);
    }

    /// A fixed level and the requester's own are two different answers, and a
    /// caller can tell which it got.
    #[test]
    fn a_fixed_priority_and_the_requester_s_own_tell_themselves_apart() {
        let commands = Commands::parse(&format!("{EXECUTOR}{KNEAD}{PROOF}{GLAZE}"), &a_path())
            .expect("the fixture is a declaration");
        let asked = Priority::Merger;

        let own = commands.resolve("knead", COMMIT, &[]).expect("declared");
        assert_eq!(own.priority, DeclaredPriority::Requester);
        assert_eq!(own.priority.fixed(), None);
        assert_eq!(own.priority(asked), Priority::Merger, "the submitter's own");
        assert_eq!(own.priority.name(), "requester");

        let fixed = commands.resolve("glaze", COMMIT, &[]).expect("declared");
        assert_eq!(fixed.priority, DeclaredPriority::Fixed(Priority::Producer));
        assert_eq!(fixed.priority.fixed(), Some(Priority::Producer));
        assert_eq!(
            fixed.priority(asked),
            Priority::Producer,
            "a fixed level is the declaration's statement and a merger cannot raise it"
        );
        assert_eq!(fixed.priority.name(), "producer");
    }

    #[test]
    fn a_priority_that_is_not_a_level_is_refused_listing_the_five_words() {
        let text =
            format!("{EXECUTOR}\n[commands.knead]\nline = \"shape\"\npriority = \"urgent\"\n");
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(
            said.contains("\"knead\"") && said.contains("\"urgent\""),
            "{said}"
        );
        for word in ["requester", "merger", "step", "nightly", "producer"] {
            assert!(said.contains(word), "{said} — it must list {word}");
        }
    }

    /// A schedule that does not say when is not a schedule, and the keys
    /// inside it are refused by name like every other.
    #[test]
    fn a_schedule_says_when_and_refuses_a_key_it_does_not_know() {
        let text = format!(
            "{EXECUTOR}\n[commands.knead]\nline = \"shape\"\n[commands.knead.schedule]\n\
             deadline = \"05:00\"\n"
        );
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(
            said.contains("missing field `at`") && said.contains("\"knead\""),
            "{said}"
        );

        let text = format!(
            "{EXECUTOR}\n[commands.knead]\nline = \"shape\"\n[commands.knead.schedule]\n\
             at = \"03:00\"\nwhen = \"tuesday\"\n"
        );
        let said = refusal(Commands::parse(&text, &a_path()));
        assert!(
            said.contains("when") && said.contains("\"knead\""),
            "{said}"
        );
    }

    #[test]
    fn a_commit_that_is_not_forty_lowercase_hex_is_refused() {
        let commands = two_commands();
        for not_a_hash in ["main", "cb5a2e8", &COMMIT.to_uppercase(), ""] {
            let said = refusal(commands.resolve("knead", not_a_hash, &[]));
            assert!(
                said.contains("forty") && said.contains("\"knead\""),
                "{said} — {not_a_hash:?} is not a hash"
            );
        }
        commands
            .resolve("knead", COMMIT, &[])
            .expect("forty lowercase hex is a hash");
    }

    /// What the CLI will hand the submission path: the line as one shell
    /// word-list, the figures, and the declaration itself — with no second
    /// reading of the file.
    #[test]
    fn the_resolved_command_is_what_a_submission_carries() {
        let resolved = two_commands()
            .resolve("proof", COMMIT, &["--only".to_owned(), "one".to_owned()])
            .expect("proof is declared by the fixture");
        assert_eq!(
            resolved.command(),
            vec![format!(
                "watch --rise {IN_TOKEN}/dough -j {PARALLELISM_TOKEN} '--only' 'one'"
            )],
            "one shell line, never an argv Herd invented"
        );
        let declared = resolved.declared();
        assert_eq!(declared.memory_mib, Some(3584));
        assert_eq!(declared.build_thread_mib, None);
        assert_eq!(declared.tmpfs_mib, Some(4096));
        assert_eq!(declared.timeout_s, Some(7200));
        assert_eq!(declared.declaration.as_ref(), Some(&resolved.declaration));
        assert_eq!(
            (
                declared.deadline_unix_s,
                declared.after,
                declared.requeue_exit_code
            ),
            (None, None, None),
            "the deadline, the parent and the requeue code are the requester's, not the file's"
        );
    }

    #[test]
    fn a_missing_file_is_refused_with_the_path_and_what_the_file_is_for() {
        let dir = TempDir::new().expect("a temporary directory");
        let said = refusal(Commands::read_in(dir.path()));
        let looked_for = dir.path().join(DECLARATION_PATH);
        assert!(
            said.contains(&looked_for.display().to_string()),
            "{said} — it must name {}",
            looked_for.display()
        );
        assert!(
            said.contains("declares the commands Herd may run for it"),
            "{said}"
        );
    }

    /// The file read where the repository already is (D106): the text on disk
    /// is this test's own, and what comes back is parsed from it.
    #[test]
    fn the_declaration_is_read_under_the_checkout_root() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = dir.path().join(DECLARATION_PATH);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(&path, format!("{EXECUTOR}{KNEAD}{PROOF}")).expect("the file");
        let commands = Commands::read_in(dir.path()).expect("the file this test wrote");
        assert_eq!(commands.names(), ["knead", "proof"]);
        assert_eq!(commands.path(), path);
        assert_eq!(
            commands
                .resolve("knead", COMMIT, &[])
                .expect("knead is declared")
                .timeout_s,
            Some(2700)
        );
    }

    #[test]
    fn a_file_that_is_not_toml_carries_toml_s_own_line_and_column() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = dir.path().join(DECLARATION_PATH);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(&path, "[executor\nup = \"wrap up\"\n").expect("the file");
        let said = refusal(Commands::read_from(&path));
        assert!(said.contains("is not valid TOML"), "{said}");
        assert!(
            said.contains("line 1"),
            "{said} — toml's own message says where"
        );
        assert!(said.contains(&path.display().to_string()), "{said}");
    }

    /// E-11's own acceptance: a third command is a third table, and **nothing
    /// in the parser changes**. The three-command file goes through exactly
    /// the call the two-command one does, and the new command resolves.
    #[test]
    fn a_third_command_parses_and_resolves_through_the_same_code_path() {
        let two = two_commands();
        let three = Commands::parse(&format!("{EXECUTOR}{KNEAD}{PROOF}{GLAZE}"), &a_path())
            .expect("a third table is a third command");
        assert_eq!(three.names(), ["glaze", "knead", "proof"]);
        assert_eq!(
            three.executor(),
            two.executor(),
            "the executor is untouched by a command being added"
        );
        for name in two.names() {
            assert_eq!(
                three.resolve(name, COMMIT, &[]).expect("still declared"),
                two.resolve(name, COMMIT, &[]).expect("declared"),
                "{name} resolves to the same thing as before the third was added"
            );
        }
        let new = three
            .resolve("glaze", COMMIT, &["--thin".to_owned()])
            .expect("the third command resolves like the first two");
        assert_eq!(
            new.line,
            format!("coat {OUT_TOKEN}/glaze '--thin'"),
            "filled by the same two passes, in the same order"
        );
        assert_eq!(new.declaration.command, "glaze");
        assert_eq!(new.declaration.commit, COMMIT);
        assert_eq!(new.priority, DeclaredPriority::Fixed(Priority::Producer));
        assert_eq!(
            new.declaration.executor,
            two.resolve("knead", COMMIT, &["--thin".to_owned()])
                .expect("declared")
                .declaration
                .executor,
            "one executor, shared: a third command runs through the same four steps"
        );
    }
}
