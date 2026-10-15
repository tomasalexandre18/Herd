#![allow(dead_code)]
//! The fixture consumer repository M10's acceptance is judged against: a real
//! git repository, with a real bare remote, declaring real commands.
//!
//! M10 is the command contract (E-11): a checkout declares the commands Herd
//! may run for it, and adding one more changes nothing on Herd's side. That
//! sentence cannot be tested against a string constant, because the two halves
//! it is about are both about a repository — [`herdctl::checkout`] reads a
//! commit out of one, and [`herdctl::declaration`] reads a file out of the same
//! one. So this builds the repository, with `git init`, `git commit` and
//! `git push`, and every state a test asserts on is a state this fixture put
//! there (D100, D101).
//!
//! **The remote is bare and the push is real.** `herdctl::checkout` refuses a
//! commit no remote-tracking ref holds, because a runner fetches the commit
//! from the consumer's own origin and a commit only this checkout has is a job
//! that could never start (E-08). A fixture that faked that — a second
//! directory called `origin`, a ref written by hand — would make the refusal
//! untestable: "pushed" has to mean a `git push` that happened, and "unpushed"
//! a commit that was deliberately left behind.
//!
//! **No consumer command is named here (E-11).** The *shape* of the
//! declaration is the first consumer's; every name in it — the tool, the
//! commands, the labels, the environment — is invented for this fixture. That
//! is also what makes the acceptance meaningful: a third command is a third
//! TOML table and not a change to any Rust.
//!
//! **The hooks and the lines are runnable shell**, and they are meant to be
//! run: the end-to-end test M10 still owes will put them through a real runner.
//! See [`DECLARATION`] for what each one does. Nothing here is a stub that
//! returns a plausible string.
//!
//! One fixture per test, built fresh. Nothing in this module is shared between
//! tests, and the three mutators ([`Fixture::dirty`],
//! [`Fixture::commit_unpushed`], [`Fixture::add_third_command`]) each assert
//! that the state they were asked for is the state the repository is now in —
//! a mutator that quietly did nothing would hand its caller a test that passes
//! on a premise nobody established.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use herdctl::checkout::Checkout;
use herdctl::declaration::{Commands, DECLARATION_PATH};
use tempfile::TempDir;

/// The two commands the fixture declares.
pub const ASSEMBLE: &str = "assemble";
pub const PROBE: &str = "probe";
/// The command [`Fixture::add_third_command`] adds, and nothing declares
/// before it is called.
pub const SKETCH: &str = "sketch";

/// The branch the fixture's one commit is on, and the tag pointing at it: two
/// kinds of ref, so a test can prove that resolution peels a tag as well as
/// reading a branch.
pub const BRANCH: &str = "main";
/// The tag on that same commit.
pub const TAG: &str = "grove-1.0";

/// The remote's one name, as the fixture's `git remote add` gives it.
pub const REMOTE: &str = "origin";

/// A tracked file [`Fixture::dirty`] changes by default.
///
/// In a subdirectory on purpose: a dirty refusal names the path git reports,
/// and a path with a directory in it is the one a reader would actually type.
pub const TRACKED: &str = "src/grove.txt";

/// Where the fixture's own tool lives, relative to the work tree.
pub const STEP_SCRIPT: &str = "tools/herd/step.sh";
/// Where the `labels_from` reading lives, relative to the work tree.
pub const LABELS_SCRIPT: &str = "tools/herd/labels.sh";

/// The exit code [`STEP_SCRIPT`] gives for the step name `fail`.
///
/// No declared command uses that name. It is here so that the end-to-end test
/// can ask the fixture for a job that reports a verdict rather than a success:
/// a fixture that could only ever exit 0 could not stand in for a consumer's
/// failing test run.
pub const FAIL_CODE: i32 = 3;

/// The environment key carrying `{commit}`.
pub const ENV_COMMIT: &str = "GROVE_COMMIT";
/// The environment key carrying `{args}`.
pub const ENV_ARGS: &str = "GROVE_ARGS";
/// The environment key carrying `{in}`, which stays `{in}` until a runner
/// fills it.
pub const ENV_IN: &str = "GROVE_IN";
/// The environment key carrying `{out}`, which stays `{out}` until a runner
/// fills it.
pub const ENV_OUT: &str = "GROVE_OUT";
/// The environment key carrying the bare remote `fetch` fetches from.
pub const ENV_REMOTE: &str = "GROVE_REMOTE";

/// Where the fixture writes the path of its own bare remote into the
/// declaration.
///
/// The remote is a temporary directory, so its path is not knowable until the
/// fixture is built; everything else in [`DECLARATION`] is written as the
/// consumer would write it.
const REMOTE_PLACEHOLDER: &str = "@REMOTE@";

/// The declaration the fixture's checkout holds: `tools/herd/commands.toml`,
/// in the shape [`herdctl::declaration`] parses.
///
/// # The four hooks
///
/// Each is one shell line, run as one, in this order. Each appends its own
/// name to `<job>/steps.txt` and prints a line, so a test can read back which
/// steps ran and in which order:
///
/// * `up` makes the workspace this job gets — the directory `{job}` names —
///   opens its step log, and echoes the arguments it was given. `{args}` is
///   pasted bare and never inside quotes of the hook's own: `args_as_line`
///   has already quoted each word, and a second layer of quoting is what a
///   requester's argument carrying a quote character would break out of.
/// * `fetch` puts the commit in that workspace: `git init`, a fetch from this
///   repository's **own bare remote** (`$GROVE_REMOTE`), and a checkout of
///   `{commit}`. This is the hook that makes the fixture's remote load-bearing
///   rather than decorative.
/// * `run` logs the step and then runs `{line}` — the declared command line,
///   pasted whole, which is the one thing Herd never interprets.
/// * `down` logs the step and prints the whole log **on stderr**, so a reader
///   of the job's output sees the four steps that ran without anything of the
///   workspace's coming after the tool's verdict: E-02's output *ends* with
///   that verdict, and a teardown that printed on stdout would come after it.
///   It runs however `run` ended.
///
/// # The environment
///
/// `GROVE_COMMIT` and `GROVE_ARGS` are the two values `herdctl` fills, carried
/// as an environment a command line can read instead of re-reading git.
/// `GROVE_IN` and `GROVE_OUT` are the two directories M11 gives a job, left as
/// `{in}` and `{out}` for the runner. `GROVE_REMOTE` is where `fetch` fetches
/// from.
///
/// # The two commands
///
/// Shaped like a build and a test run, and between them they exercise every
/// key of the file: a static `labels` list and a `labels_from` reading with its
/// baseline, `build_thread_mib` against `memory_mib`, a `tmpfs_mib` of 0, a
/// `timeout_s` each, a fixed priority against `requester`, `needs` against
/// `leaves`, a `coalesce` key and a `dedup_key`.
const DECLARATION: &str = r#"# The commands Herd may run for this repository.
#
# Every name in this file is invented for Herd's own tests. What is borrowed
# from a real consumer is the shape of the file, never a name out of it
# (E-11): Herd's code and tests name no consumer command, and this fixture is
# the proof that they do not have to — a command is a table here and nothing
# anywhere else.
#
# The four hooks each append their own step name to the job's step log and
# print a line, so a test can read back what ran. `fetch` fetches from this
# repository's own bare remote, which is the only way a runner ever gets a
# commit: Herd hands over a hash and holds no repository.

[executor]
up = "mkdir -p '{job}' && printf 'up %s\\n' '{job}' >> '{job}/steps.txt' && echo 'grove: workspace' '{job}' 'args:' {args}"
fetch = "cd '{job}' && git init -q . && git fetch -q --no-tags \"$GROVE_REMOTE\" '+refs/heads/*:refs/remotes/origin/*' && git checkout -q -f {commit} && printf 'fetch %s\\n' '{commit}' >> steps.txt && printf 'grove: at %s\\n' '{commit}'"
run = "cd '{job}' && printf 'run\\n' >> steps.txt && {line}"
down = "printf 'down %s\\n' '{job}' >> '{job}/steps.txt' && cat '{job}/steps.txt' >&2"

[executor.env]
# The commit and the arguments, filled by herdctl before the job is minted, so
# a command line can read them without running git.
GROVE_COMMIT = "{commit}"
GROVE_ARGS = "{args}"
# The two directories M11 gives a job: what it consumes, and where what it
# leaves is kept. Passed by name rather than pasted into a hook, and left for
# the runner to fill.
GROVE_IN = "{in}"
GROVE_OUT = "{out}"
# Where a runner fetches the commit from: this repository's own remote.
GROVE_REMOTE = "@REMOTE@"

# A build: it declares what one of its build threads needs, so the hub derives
# the `-j` from the machine it picks and pastes the figure where `{j}` is. It
# needs nothing on the tmpfs and says so with a nought, which is a real
# declaration and not an absence. Its level is fixed: a cache producer gives
# way to everything, however eager the thing that submitted it.
[commands.assemble]
line = "sh tools/herd/step.sh assemble -j {j} -- {args}"
labels = ["grove-toolchain", "linux"]
build_thread_mib = 1792
tmpfs_mib = 0
timeout_s = 2700
priority = "producer"
leaves = { "grove-tree" = "tree.txt", "grove-log" = "logs/assemble.log" }
dedup_key = "assemble-{commit}-{args}"

# A test run: what it needs is not knowable from this file, because which
# capabilities one commit's tests want is a property of that commit — so it
# declares a reading, and its `labels` are then the baseline the reading job
# itself is placed against rather than the labels of the job that follows it.
# It takes the build's artefact, declares what the whole job needs, and leaves
# its level to whoever submits it.
[commands.probe]
line = "sh tools/herd/step.sh probe --from \"$GROVE_IN\" -- {args}"
labels = ["grove-toolchain"]
labels_from = "sh tools/herd/labels.sh {commit} {args}"
memory_mib = 4096
timeout_s = 7200
priority = "requester"
needs = ["grove-tree"]
coalesce = "probe-{commit}"
"#;

/// The third command, appended by [`Fixture::add_third_command`].
///
/// A whole table and nothing else: that is E-11's acceptance sentence, and the
/// reason this is a `const` of TOML rather than a line of Rust that builds one.
const THIRD_COMMAND: &str = r#"
# A third command, added after the fact. Nothing on Herd's side changes for
# it: no Rust, no protocol, no deployment, and nothing about the two commands
# above (E-11).
[commands.sketch]
line = "sh tools/herd/step.sh sketch -- {args}"
labels = ["grove-toolchain"]
timeout_s = 600
"#;

/// The fixture's one tool, standing in for whatever a consumer runs.
///
/// Every declared command line calls it. It appends the step name it was given
/// to `steps.txt` in the directory it runs in — the workspace the `up` hook
/// made — and prints one line naming itself and its arguments, so a test can
/// read back what ran with which arguments. It exits 0, except for the name
/// `fail`, which exits [`FAIL_CODE`].
const STEP_SOURCE: &str = r#"#!/bin/sh
# The fixture consumer's one tool. Appends the step it was asked for to
# steps.txt in the directory it runs in, prints one line, prints its verdict,
# and exits with it.
#
# `--verdict <code>` anywhere in its arguments says which of the consumer's
# three codes this run reports: 0 a pass, 1 a fail, 75 absent-or-leased. A
# real tool decides that from what it found; a fixture is told, which is how
# a test can ask for each of the three without a second command. The name
# `fail` still exits 3, for a code that is none of the three.
set -u
step=${1:?a step name}
shift
verdict=0
previous=
for word in "$@"; do
    if [ "$previous" = --verdict ]; then
        verdict=$word
    fi
    previous=$word
done
printf '%s %s\n' "$step" "$*" >> steps.txt
printf 'grove %s: %s\n' "$step" "$*"
if [ "$step" = fail ]; then
    exit 3
fi
# Last, because a caller reads the verdict off the end of the output (E-02).
printf 'grove %s: verdict %s\n' "$step" "$verdict"
exit "$verdict"
"#;

/// What a `labels_from` reading runs: what this commit's own tests need, read
/// on a machine because only the commit knows (D74).
///
/// A real consumer runs its own tool over its own tree. This prints the four
/// keys Herd reads, in the shape D74 states — one `key=value` per line on
/// standard output — and `--answer <mode>` picks **which** of the answers it
/// gives, so that one fixture stands for a reading that answers, one that
/// answers something that is not an answer, one that fails and one that never
/// ends. A real tool decides what to say from what it found; a fixture is
/// told, which is how a test asks for each case without a second command.
///
/// The mode travels in the command's own arguments, which is not an
/// accident: the arguments are part of the cache key (D74), so each case is
/// its own entry and no case is answered out of another's cache.
const LABELS_SOURCE: &str = r#"#!/bin/sh
# What `labels_from` runs: what this commit's tests need, four keys on stdout.
#
# `--answer <mode>` says which answer this run gives. The modes are the ones
# Herd has to tell apart: an answer, four ways of printing something that is
# not one, a reading that fails, and a reading that never ends.
set -u
answer=good
previous=
for word in "$@"; do
    if [ "$previous" = --answer ]; then
        answer=$word
    fi
    previous=$word
done
# One line of the fixture's own, before the answer: a reading runs through the
# four executor steps like any other job, so its output is never only its
# answer.
printf 'grove labels: reading for %s\n' "$answer"
case $answer in
good)
    printf 'labels=grove-toolchain,linux\n'
    printf 'memory_mib=2048\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    ;;
unknown-key)
    printf 'labels=grove-toolchain,linux\n'
    printf 'memory_mib=2048\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    printf 'scenario=4\n'
    ;;
missing-key)
    printf 'labels=grove-toolchain,linux\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    ;;
twice)
    printf 'labels=grove-toolchain,linux\n'
    printf 'labels=linux\n'
    printf 'memory_mib=2048\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    ;;
not-a-figure)
    printf 'labels=grove-toolchain,linux\n'
    printf 'memory_mib=plenty\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    ;;
refuses)
    printf 'grove labels: this tree cannot say what it needs\n' >&2
    exit 7
    ;;
never-ends)
    # Longer than any cap a test gives the reading, and it prints the answer
    # afterwards: a reading stopped at its cap must refuse the job even though
    # it would have answered eventually.
    sleep 60
    printf 'labels=grove-toolchain,linux\n'
    printf 'memory_mib=2048\n'
    printf 'tmpfs_mib=0\n'
    printf 'scenarios=4\n'
    ;;
*)
    printf 'grove labels: no such answer: %s\n' "$answer" >&2
    exit 64
    ;;
esac
exit 0
"#;

/// The answer [`LABELS_SOURCE`] gives in its `good` mode, which is the one
/// every test that is not about a refusal asks for.
///
/// Written here so that the tests compare against the fixture's own figures
/// rather than against numbers typed into a test file — and so that the
/// memory figure is visibly **not** the one `probe` declares: the reading is
/// the thing that knows, and its figure is the one the job is admitted
/// against.
pub const READ_LABELS: [&str; 2] = ["grove-toolchain", "linux"];
pub const READ_MEMORY_MIB: u64 = 2048;
pub const READ_TMPFS_MIB: u64 = 0;
pub const READ_SCENARIOS: u64 = 4;
/// What `probe` declares for the whole job, which the reading's figure
/// replaces: the two are deliberately different.
pub const PROBE_DECLARES_MEMORY_MIB: u64 = 4096;

/// The modes [`LABELS_SOURCE`] answers in, as a test names one.
///
/// `--answer <mode>` goes in the command's own arguments, so it reaches the
/// reading through `{args}` exactly as a requester's own argument would.
pub const ANSWER_GOOD: &str = "good";
pub const ANSWER_UNKNOWN_KEY: &str = "unknown-key";
pub const ANSWER_MISSING_KEY: &str = "missing-key";
pub const ANSWER_TWICE: &str = "twice";
pub const ANSWER_NOT_A_FIGURE: &str = "not-a-figure";
pub const ANSWER_REFUSES: &str = "refuses";
pub const ANSWER_NEVER_ENDS: &str = "never-ends";
/// The code [`LABELS_SOURCE`] exits with in its `refuses` mode.
pub const REFUSES_CODE: i32 = 7;

/// The fixture consumer repository: a bare remote, and a work tree that has
/// pushed to it.
pub struct Fixture {
    dir: TempDir,
}

impl Fixture {
    /// Build the repository: a bare remote, a work tree with the declaration,
    /// the two scripts and some tracked content, committed and pushed, with a
    /// tag on the commit.
    ///
    /// `None` means `git` is not on this host's PATH and the test has skipped
    /// loudly (D27) under the name it gave. A caller returns on `None`; it must
    /// never read that as a repository with nothing in it.
    pub fn new(test: &str) -> Option<Fixture> {
        if !git_is_here() {
            herd_runner::testing::skipped(
                test,
                "git is not on this host's PATH, so no fixture consumer repository could be \
                 built and nothing about a requester's checkout could be read",
            );
            return None;
        }
        let dir = TempDir::new().expect("a temporary directory for the fixture");
        for made in ["home", "remote", "work"] {
            std::fs::create_dir_all(dir.path().join(made)).expect("the fixture's directories");
        }
        let fixture = Fixture { dir };

        // A bare remote, which is what a remote is: there is no work tree to
        // push into and no way for the push below to be anything but real.
        fixture.git_in(
            "remote",
            &["init", "-q", "--bare", "--initial-branch", BRANCH],
        );

        fixture.git(&["init", "-q", "--initial-branch", BRANCH, "."]);
        fixture.write("README.md", "The fixture consumer repository.\n");
        fixture.write(TRACKED, "one\n");
        fixture.write("src/other.txt", "two\n");
        fixture.write(DECLARATION_PATH, &fixture.declaration_text());
        fixture.write(STEP_SCRIPT, STEP_SOURCE);
        fixture.write(LABELS_SCRIPT, LABELS_SOURCE);
        fixture.make_runnable(STEP_SCRIPT);
        fixture.make_runnable(LABELS_SCRIPT);
        fixture.git(&["add", "-A"]);
        fixture.git(&["commit", "-qm", "declare the commands Herd may run"]);
        fixture.git(&["tag", TAG]);

        let remote = fixture.remote();
        fixture.git(&["remote", "add", REMOTE, &remote.to_string_lossy()]);
        fixture.git(&["push", "-q", "--set-upstream", REMOTE, BRANCH]);
        fixture.git(&["push", "-q", REMOTE, TAG]);

        // Established, not assumed: the commit the tests resolve is on a
        // remote-tracking ref, which is the premise every one of them that is
        // *not* about the unpushed refusal stands on.
        let head = fixture.rev_parse("HEAD");
        let holding = fixture.git(&["branch", "--remotes", "--contains", &head]);
        assert!(
            holding.contains(&format!("{REMOTE}/{BRANCH}")),
            "the fixture pushed {head} and no remote-tracking ref holds it: {holding:?}"
        );
        Some(fixture)
    }

    /// The work tree root: the checkout a requester would be standing in.
    pub fn root(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    /// The bare remote the work tree pushed to: where a runner would fetch the
    /// commit from.
    pub fn remote(&self) -> PathBuf {
        self.dir.path().join("remote")
    }

    /// A `HOME` of the fixture's own, so that running the declared hooks never
    /// reads the person's git config.
    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// The checkout, as `herdctl` finds it.
    pub fn checkout(&self) -> Checkout {
        Checkout::found_from(&self.root()).expect("the fixture's own checkout is found")
    }

    /// The declaration, as `herdctl` reads it out of the work tree.
    pub fn commands(&self) -> Commands {
        Commands::read_in(&self.root()).expect("the fixture's declaration parses")
    }

    /// What the fixture's **own** git says a name resolves to: the independent
    /// reading a test compares `herdctl` against, so no hash is ever written
    /// into a test.
    pub fn rev_parse(&self, named: &str) -> String {
        let hash = self.git(&["rev-parse", &format!("{named}^{{commit}}")]);
        assert_eq!(hash.len(), 40, "git gave {hash:?} for {named:?}");
        hash
    }

    /// Change a tracked file, so the tree and its commit disagree (E-08).
    ///
    /// The path is checked to be **tracked** first: an untracked file is not a
    /// dirty tree, and a mutator that wrote one would hand back a test that
    /// asserts a refusal it had not caused. Returns the path as git reports it,
    /// which is what the refusal names.
    pub fn dirty(&self, rel: &str) -> String {
        self.git(&["ls-files", "--error-unmatch", "--", rel]);
        let held = std::fs::read_to_string(self.root().join(rel)).expect("the tracked file reads");
        self.write(rel, &format!("{held}and a change that is in no commit\n"));
        let reported = self.git(&["status", "--porcelain=v1", "--untracked-files=no"]);
        assert!(
            reported.contains(rel),
            "the fixture changed {rel:?} and git reports {reported:?}"
        );
        rel.to_owned()
    }

    /// Commit a change and deliberately **not** push it, returning its hash.
    ///
    /// The tree is left clean, so what a caller then asserts is about the
    /// commit being on no remote and about nothing else. That the commit is on
    /// no remote-tracking ref is checked here rather than hoped for.
    pub fn commit_unpushed(&self, rel: &str) -> String {
        self.write(rel, "a line that only this checkout has\n");
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "a commit no remote has"]);
        let hash = self.rev_parse("HEAD");
        let holding = self.git(&["branch", "--remotes", "--contains", &hash]);
        assert!(
            holding.trim().is_empty(),
            "the fixture meant to leave {hash} unpushed and a remote-tracking ref holds it: \
             {holding:?}"
        );
        let dirty = self.git(&["status", "--porcelain=v1", "--untracked-files=no"]);
        assert!(
            dirty.trim().is_empty(),
            "the unpushed commit left the tree dirty, which is a second refusal: {dirty:?}"
        );
        hash
    }

    /// Add a third command to the declaration, commit it and push it.
    ///
    /// Committed and pushed because that is the state the acceptance is about:
    /// a consumer adds a command, pushes it, and nothing on Herd's side moves.
    /// It also leaves the fixture clean and pushed, so the only thing that
    /// changed for a caller is the file's contents. Returns the new commit's
    /// hash; the command's name is [`SKETCH`].
    pub fn add_third_command(&self) -> String {
        let path = self.root().join(DECLARATION_PATH);
        let held = std::fs::read_to_string(&path).expect("the declaration reads");
        assert!(
            !held.contains(SKETCH),
            "the declaration already declares {SKETCH:?}, so adding it proves nothing"
        );
        std::fs::write(&path, format!("{held}{THIRD_COMMAND}"))
            .expect("the declaration is added to");
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "declare a third command"]);
        self.git(&["push", "-q", REMOTE, BRANCH]);
        let hash = self.rev_parse("HEAD");
        let holding = self.git(&["branch", "--remotes", "--contains", &hash]);
        assert!(
            holding.contains(&format!("{REMOTE}/{BRANCH}")),
            "the third command was pushed and no remote-tracking ref holds {hash}: {holding:?}"
        );
        hash
    }

    /// Run one shell line the way a runner would run a hook: one `sh -c`, in
    /// `at`, with the executor's environment and nothing of this process's own
    /// but `PATH`.
    ///
    /// `HOME` is the fixture's own, so the person's git config is out of the
    /// way and the hooks say the same thing on every machine.
    pub fn shell(&self, at: &Path, env: &BTreeMap<String, String>, line: &str) -> Output {
        Command::new("sh")
            .arg("-c")
            .arg(line)
            .current_dir(at)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.home())
            .env("LC_ALL", "C")
            .envs(env)
            .stdin(Stdio::null())
            .output()
            .expect("sh runs the fixture's hook")
    }

    /// One git command in the work tree, with a config of its own.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in("work", args)
    }

    /// One git command in one of the fixture's directories, with a config of
    /// its own: the host's `~/.gitconfig` and `/etc/gitconfig` are out of the
    /// way, so the fixture is the same repository on every machine. A failure
    /// of the fixture's own git panics at once rather than becoming a confusing
    /// assertion later.
    pub fn git_in(&self, dir: &str, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.dir.path().join(dir))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.home())
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Herd Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@herd.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_NAME", "Herd Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@herd.invalid")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .stdin(Stdio::null())
            .output()
            .expect("git runs in the fixture");
        assert!(
            output.status.success(),
            "the fixture's own git failed in {dir}: git {}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The declaration as this fixture holds it: [`DECLARATION`] with its own
    /// bare remote's path written in.
    fn declaration_text(&self) -> String {
        DECLARATION.replace(REMOTE_PLACEHOLDER, &self.remote().to_string_lossy())
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's directory");
        }
        std::fs::write(path, text).expect("the fixture writes a file");
    }

    /// Make a script executable, where the platform has such a thing.
    ///
    /// The declared lines all run it as `sh <script>`, so this is not what
    /// makes them work — it is here so the fixture is the repository a person
    /// would have committed.
    fn make_runnable(&self, rel: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = self.root().join(rel);
            let mut perms = std::fs::metadata(&path)
                .expect("the script's metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("the script is made executable");
        }
    }
}

/// The runner the harness below lends, named so that E-02's framing line can
/// be asserted against a name a test chose.
pub const RUNNER: &str = "declared-runner";

/// What the fixture's commands ask for, offered by the one runner here —
/// including the two labels the `labels_from` reading answers with, since a
/// job placed on a reading's answer is placed on machines of *those* labels.
///
/// Shared, because nothing in this fixture contends for anything: what is
/// being asserted is the contract, not the placement, and the placement has
/// its own suite.
pub const WHAT_THE_FIXTURE_ASKS_FOR: &str = r#"[
    {"name":"grove-toolchain","kind":{"type":"shared"},"origins":{"declared":true}},
    {"name":"linux","kind":{"type":"shared"},"origins":{"declared":true}}
]"#;

/// A hub with one runner joined, and the means to run `herdctl` in a checkout.
///
/// In process rather than as binaries, except for `herdctl` itself: what the
/// tests here assert is what a **caller** sees — the status its shell read and
/// the bytes on its stdout — so the requester is the real binary, and the hub
/// and the runner are the real code with their own state and data roots.
pub struct Harness {
    hub_url: String,
    pub state: TempDir,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    _data_root: herd_runner::testing::TempDir,
}

impl Harness {
    /// A hub and a runner, with the hub's own default timing.
    pub async fn new() -> Harness {
        Harness::with_policy(herd_hub::Policy::default()).await
    }

    /// The same, with a timing a test chose — a `labels_from` reading capped
    /// at two seconds, say, so that a reading which never ends can be seen
    /// refusing the job it was read for without waiting five minutes (D74).
    pub async fn with_policy(policy: herd_hub::Policy) -> Harness {
        use herd_hub::{Hub, JobDirStore, JobStore};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
        // An unapproved runner is given no job (E-48), so this harness
        // approves the one runner it lends and hands it the token that
        // approval minted. Without it the declaration resolves, the job is
        // minted, and nothing ever claims it — including a `labels_from`
        // reading, which is an ordinary job on a runner like any other (D74).
        let token = store
            .approve_runner(RUNNER)
            .expect("the state root mints a token");
        let hub = Hub::bind_with_policy(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            store as Arc<dyn JobStore>,
            policy,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let mut tasks = vec![tokio::spawn(async move {
            let _ = hub.serve().await;
        })];

        // One data root per runner, thrown away with the harness (E-45).
        let root = herd_runner::testing::TempDir::new("declared-e2e");
        let offer: herd_proto::CapabilityOffer = serde_json::from_str(WHAT_THE_FIXTURE_ASKS_FOR)
            .expect("the capabilities this fixture's commands ask for");
        let config = herd_runner::Config {
            hub_url: hub_url.clone(),
            name: Some(RUNNER.to_owned()),
            token: Some(token),
            capabilities: offer,
            // A size the owner states, rather than this host's own. The
            // fixture's build declares a per-thread figure, so the hub
            // derives a memory need and admission weighs it (E-29): a runner
            // that says nothing about the floor it keeps for itself admits
            // nothing at all, and the job would wait for ever on a machine
            // that could have run it. Stated figures also make the derived
            // `-j` the same on every host, which is what keeps these tests
            // about the contract and not about the machine they ran on.
            host: herd_proto::HostSize {
                memory_mib: Some(8192),
                threads: Some(4),
                memory_floor_mib: Some(1024),
                tmpfs_mib: None,
            },
            confinement: herd_runner::Confinement::with_data_root(root.path()),
            ..herd_runner::Config::default()
        };
        tasks.push(tokio::spawn(async move {
            let _ = herd_runner::run(config).await;
        }));
        // Wait on the hub's own state, never on a sleep.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Harness {
            hub_url,
            state,
            tasks,
            _data_root: root,
        }
    }

    /// Run `herdctl` **in** a checkout, as a requester standing in one does.
    ///
    /// The working directory is the whole point: the declaration and the
    /// commit are read where the repository already is (D106), and nothing
    /// tells the binary where that is but the directory it was started in.
    pub async fn herdctl_in(&self, fixture: &Fixture, arguments: &[&str]) -> Output {
        tokio::process::Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&self.hub_url)
            .args(arguments)
            .current_dir(fixture.root())
            // A git config of the fixture's own, so the person's identity and
            // their `~/.gitconfig` are out of the way and the refusals read
            // the same on every machine.
            .env("HOME", fixture.home())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .expect("herdctl runs")
    }

    /// Every job record on the hub's disk, oldest id first, read as a reader
    /// outside Herd would (E-15).
    pub fn records(&self) -> Vec<serde_json::Value> {
        let jobs = self.state.path().join("jobs");
        let mut dirs: Vec<PathBuf> = match std::fs::read_dir(&jobs) {
            Ok(entries) => entries
                .map(|entry| entry.expect("a job directory").path())
                .collect(),
            // No job was ever minted, which is a state a test may be about.
            Err(_) => Vec::new(),
        };
        dirs.sort();
        dirs.iter()
            .map(|dir| {
                let text = std::fs::read_to_string(dir.join("job.json")).expect("the record reads");
                serde_json::from_str(&text).expect("the record is json")
            })
            .collect()
    }

    /// The record of the one job this harness ran.
    pub fn only_record(&self) -> serde_json::Value {
        let mut records = self.records();
        assert_eq!(
            records.len(),
            1,
            "exactly one job was submitted: {records:?}"
        );
        records.remove(0)
    }

    /// One job's log, byte for byte as the hub wrote it (D18).
    pub fn log(&self, job_id: &str) -> String {
        let path = self.state.path().join("jobs").join(job_id).join("log");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|why| panic!("the log of {job_id} at {} reads: {why}", path.display()))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Whether `git` can be run at all.
pub fn git_is_here() -> bool {
    match Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// The refusal a `Result` carries, as a reader would see it.
pub fn refusal<T: std::fmt::Debug>(result: anyhow::Result<T>) -> String {
    match result {
        Ok(held) => panic!("this was meant to be refused and was accepted: {held:?}"),
        Err(why) => why.to_string(),
    }
}
