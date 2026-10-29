//! The requester's own checkout: where the commit a submission carries is
//! read (D106).
//!
//! A submission names a **commit**, never a machine (E-01), and the commit is
//! read here — in `herdctl`, where the repository already is. The hub and a
//! runner hold no repository and never run git: a runner fetches the commit
//! from the consumer's own origin, which is the whole reason this module
//! refuses a tree that does not match its commit, or a commit no remote holds
//! (E-08). A job that could never be fetched is better refused in the second
//! it is asked for than an hour later on a machine.
//!
//! Three rules, and none of them has a flag to get past it:
//!
//! * a ref — or a hash, short or whole — resolves to exactly one commit hash;
//! * a tracked modification or a staged change refuses, naming the files;
//! * a commit no remote-tracking ref holds refuses, naming the hash.
//!
//! An untracked file does **not** refuse: it was never part of any commit, so
//! it says nothing about the commit being submitted, while a tracked change
//! means the tree and the commit disagree about what the job would build.
//!
//! This is plain synchronous work — a handful of short `git` reads — and it
//! stays synchronous while the rest of `herdctl` is async: there is nothing
//! here to wait on concurrently.

use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How many paths a dirty refusal names before it counts the rest.
///
/// A tree with two hundred changed files must still give a sentence a person
/// can read, so the list is bounded and the remainder is a figure.
const PATHS_NAMED: usize = 5;

/// A commit a submission may carry: forty hex, and what the requester called
/// it.
///
/// Both halves travel: the hash is what Herd records and what a runner
/// fetches, and `named` is what the person typed — a refusal that says only
/// the hash makes the reader go and look up which branch that was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub named: String,
}

impl Commit {
    /// How a refusal says which commit: the hash, and the name it came from
    /// when that adds anything.
    fn said(&self) -> String {
        if self.named == self.hash {
            self.hash.clone()
        } else {
            format!("{} ({})", self.hash, self.named)
        }
    }
}

/// The requester's own checkout: the work tree root every `git` read here is
/// given explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    root: PathBuf,
}

impl Checkout {
    /// The checkout the requester is standing in, found upward from `from`.
    ///
    /// Git's own upward discovery does the finding, so a linked worktree and
    /// a `.git` file work exactly as they do for the person's own commands.
    pub fn found_from(from: &Path) -> Result<Checkout> {
        let said = run_git(from, &["rev-parse", "--show-toplevel"])?;
        let root = said.out.trim();
        if !said.worked() || root.is_empty() {
            bail!(
                "no git checkout at {} or any directory above it ({}): herdctl reads the \
                 commit where the repository already is, so run it inside the checkout you \
                 are submitting",
                from.display(),
                said.complaint()
            );
        }
        Ok(Checkout {
            root: PathBuf::from(root),
        })
    }

    /// The work tree root, as git itself spells it.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A ref — or a hash — resolved to the one commit a submission will
    /// carry.
    ///
    /// This is the single door, and it is the whole of E-01 and E-08: the name
    /// resolves, then a dirty tree refuses, then a commit no remote holds
    /// refuses. Resolution comes first so that an unknown name is reported as
    /// an unknown name even in a tree that is also dirty.
    ///
    /// `HEAD` is what a caller passes when the requester named nothing.
    pub fn resolve(&self, named: &str) -> Result<Commit> {
        let commit = self.commit_named(named)?;
        self.refuse_if_dirty()?;
        self.refuse_if_unpushed(&commit)?;
        Ok(commit)
    }

    /// The commit a name stands for, with nothing said about the tree.
    fn commit_named(&self, named: &str) -> Result<Commit> {
        // `--end-of-options` so that a name beginning with a dash is a name
        // and never an option, and `^{commit}` so that a tag or a tree is
        // peeled — or refused for what it is.
        let peeled = format!("{named}^{{commit}}");
        let said = run_git(
            &self.root,
            &["rev-parse", "--verify", "--end-of-options", &peeled],
        )?;
        if !said.worked() {
            bail!("{}", self.cannot_resolve(named, &said));
        }
        let hash = said.out.trim().to_owned();
        if !is_whole_hash(&hash) {
            bail!(
                "git resolved '{named}' in {} to '{hash}', which is not the forty lowercase hex \
                 characters of a commit hash: a submission records that hash and nothing else, \
                 so name a commit this checkout can give one for",
                self.root.display()
            );
        }
        Ok(Commit {
            hash,
            named: named.to_owned(),
        })
    }

    /// Why a name did not give a commit: it is not known here at all, or it is
    /// known and is not a commit.
    fn cannot_resolve(&self, named: &str, failed: &Said) -> String {
        let root = self.root.display();
        let plain = run_git(
            &self.root,
            &["rev-parse", "--verify", "--end-of-options", named],
        );
        if let Ok(plain) = plain {
            if plain.worked() {
                let object = plain.out.trim().to_owned();
                let kind = run_git(&self.root, &["cat-file", "-t", &object])
                    .ok()
                    .filter(Said::worked)
                    .map(|said| said.out.trim().to_owned())
                    .unwrap_or_else(|| "not a commit".to_owned());
                return format!(
                    "'{named}' names a {kind} in {root}, not a commit: a job carries one \
                     commit, so name a branch, a tag or a commit hash"
                );
            }
        }
        format!(
            "'{named}' is not a ref or a commit {root} holds ({}): fetch it, or name a branch, \
             a tag or a hash this checkout has",
            failed.complaint()
        )
    }

    /// Refuse a tree that does not say what its commit says, naming the files
    /// (E-08).
    ///
    /// Untracked files are off, on purpose: see this module's header. There is
    /// no flag past this refusal, by design — a job that built something other
    /// than the commit it reports is worse than a job that did not run.
    pub fn refuse_if_dirty(&self) -> Result<()> {
        // `-z` rather than quoted paths: a path with a space, a quote or a
        // newline in it is then still exactly one field, and a refusal can
        // print the name the person would type.
        let said = run_git(
            &self.root,
            &["status", "--porcelain=v1", "--untracked-files=no", "-z"],
        )?;
        if !said.worked() {
            bail!(
                "git could not read the state of the checkout at {} ({}): a repository that \
                 cannot be read is never read as a clean one",
                self.root.display(),
                said.complaint()
            );
        }
        let changed = changed_paths(&said.out);
        if changed.is_empty() {
            return Ok(());
        }
        bail!("{}", dirty_refusal(&self.root, &changed));
    }

    /// Refuse a commit no remote-tracking ref holds, naming the hash (E-08).
    ///
    /// A runner fetches the commit from the consumer's own origin, so a commit
    /// that is only in this checkout is a job that could never start.
    pub fn refuse_if_unpushed(&self, commit: &Commit) -> Result<()> {
        let remotes = run_git(&self.root, &["remote"])?;
        if !remotes.worked() {
            bail!(
                "git could not list the remotes of {} ({}): whether a commit can be fetched is \
                 never assumed",
                self.root.display(),
                remotes.complaint()
            );
        }
        let named: Vec<&str> = remotes.out.split_whitespace().collect();
        if named.is_empty() {
            bail!(
                "{} is on no remote: {} has no remote at all, so a runner would have nowhere \
                 to fetch it from. Add a remote and push the branch that holds it, then submit \
                 again",
                commit.said(),
                self.root.display()
            );
        }
        let holding = run_git(
            &self.root,
            &["branch", "--remotes", "--contains", &commit.hash],
        )?;
        if !holding.worked() {
            bail!(
                "git could not tell whether {} is on a remote of {} ({}): a commit is never \
                 taken for pushed",
                commit.said(),
                self.root.display(),
                holding.complaint()
            );
        }
        if holding.out.trim().is_empty() {
            bail!(
                "{} is on no remote-tracking ref of {}: a runner fetches the commit from your \
                 own origin, so push the branch that holds it and submit again",
                commit.said(),
                named.join(", ")
            );
        }
        Ok(())
    }

    /// The commit a **ref** stands at: the one read E-41's predicate is
    /// built on (D207, [`crate::moved`]).
    ///
    /// Separate from [`Checkout::resolve`], and deliberately narrower than
    /// it in one way and wider in another:
    ///
    /// * it insists on a **ref**. A hash resolves perfectly well and is
    ///   refused anyway, because a hash names one commit for ever: a
    ///   predicate asking "has it moved?" about one would answer "it moved"
    ///   on its first run and "it did not" on every run after, for ever,
    ///   and nobody would be told why. A ref that does not exist is refused
    ///   by name for the same reason — a ref that is not there has not
    ///   moved, and must never be read as having moved;
    /// * it says nothing about the tree or about any remote. It is a read
    ///   of where a ref points, not a submission. E-08's two refusals still
    ///   happen, on the way out: the hash this gives is handed back through
    ///   [`Checkout::resolve`] by the submission itself, so a dirty tree or
    ///   an unpushed commit is refused there, once, by the code that
    ///   already does it.
    pub fn tip_of_ref(&self, named: &str) -> Result<Commit> {
        let full = self.full_name_of_ref(named)?;
        let commit = self.commit_named(&full)?;
        Ok(Commit {
            hash: commit.hash,
            // The full name, not what was typed: `main` and
            // `refs/heads/main` are the same ref, and a state file that
            // recorded one of the two spellings would read as a different
            // ref the day somebody typed the other.
            named: full,
        })
    }

    /// The full name of a ref — `refs/remotes/origin/main` — or a refusal
    /// saying what the name is instead.
    fn full_name_of_ref(&self, named: &str) -> Result<String> {
        let said = run_git(
            &self.root,
            &[
                "rev-parse",
                "--symbolic-full-name",
                "--end-of-options",
                named,
            ],
        )?;
        // The **last** non-empty line, and this is not fussiness: in
        // `--symbolic-full-name` mode `git rev-parse` echoes every argument
        // it has no ref for, `--end-of-options` included, so the output of a
        // name that resolves perfectly well is two lines with the marker
        // first (measured on git 2.54.0: `--end-of-options\nrefs/heads/x`).
        // Trimming the whole output and testing its start reads the marker
        // and refuses every ref there is. The marker itself stays, because
        // without it a ref whose name begins with a dash would be read as an
        // option.
        let full = said
            .out
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or_default()
            .to_owned();
        if said.worked() && full.starts_with("refs/") {
            return Ok(full);
        }
        let root = self.root.display();
        // A name git resolves and has no ref for is a hash, or something
        // peeled from one. It is refused for what it is rather than for
        // being unknown, because the two call for opposite corrections.
        let object = run_git(
            &self.root,
            &["rev-parse", "--verify", "--end-of-options", named],
        )?;
        if object.worked() {
            bail!(
                "'{named}' names an object in {root} and not a ref that can move: a predicate \
                 watching a ref needs a ref — name a branch or a remote-tracking ref, like \
                 refs/remotes/origin/main"
            );
        }
        bail!(
            "'{named}' is not a ref {root} holds ({}): a ref that does not exist has not \
             moved, so nothing was submitted. Fetch it, or name the ref you watch in full, \
             like refs/remotes/origin/main",
            said.complaint()
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

/// The paths `git status --porcelain=v1 -z` reported as changed, each once.
///
/// A rename or a copy carries its source as a second field; the change is the
/// new path, and the old one is not a second change.
fn changed_paths(status: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut records = status.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        // `XY <path>`: two status codes, a space, then the path.
        if record.len() < 4 {
            continue;
        }
        let codes = &record[..2];
        let path = &record[3..];
        if codes.contains('R') || codes.contains('C') {
            let _ = records.next();
        }
        paths.push(path.to_owned());
    }
    paths
}

/// One sentence for a dirty tree, however many files are in it.
fn dirty_refusal(root: &Path, changed: &[String]) -> String {
    let named = changed
        .iter()
        .take(PATHS_NAMED)
        .cloned()
        .collect::<Vec<String>>()
        .join(", ");
    let rest = changed.len().saturating_sub(PATHS_NAMED);
    let list = if rest == 0 {
        named
    } else {
        format!("{named}, and {rest} more")
    };
    let root = root.display();
    match changed.len() {
        1 => format!(
            "{root} has 1 change that is in no commit: {list}. A runner builds the commit and \
             never your working tree, so commit or stash it — nothing gets past this refusal"
        ),
        many => format!(
            "{root} has {many} changes that are in no commit: {list}. A runner builds the \
             commit and never your working tree, so commit or stash them — nothing gets past \
             this refusal"
        ),
    }
}

/// What one `git` invocation answered.
struct Said {
    status: std::process::ExitStatus,
    out: String,
    err: String,
}

impl Said {
    fn worked(&self) -> bool {
        self.status.success()
    }

    /// Git's own complaint, on one line, for a refusal to carry.
    ///
    /// This is what keeps a broken repository from reading as a dirty one: the
    /// refusal says what git said, not what Herd guessed.
    fn complaint(&self) -> String {
        let said = self
            .err
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<&str>>()
            .join("; ");
        if said.is_empty() {
            match self.status.code() {
                Some(code) => format!("git exited {code} and said nothing"),
                None => "git was killed by a signal and said nothing".to_owned(),
            }
        } else {
            format!("git said: {said}")
        }
    }
}

/// Run one `git` read in `at`, with an environment built rather than
/// inherited.
///
/// Built, because E-42 wants this usable from a `post-receive` hook: a hook's
/// environment carries `GIT_DIR`, `GIT_INDEX_FILE`, `GIT_QUARANTINE_PATH` and
/// friends, every one of which would silently point these reads at a
/// different repository than the one `-C` names. Clearing them is the whole
/// point; what is kept is the little that must be:
///
/// * `PATH`, or git cannot be found at all — on NixOS it is nowhere a default
///   search path would look;
/// * `HOME`, because the person's own config is part of their checkout's
///   reality (`safe.directory`, `includeIf`), and a read that ignored it would
///   refuse repositories their own git accepts;
/// * `LC_ALL=C`, so git's messages and `--porcelain` are the one parse this
///   module was written against;
/// * `GIT_TERMINAL_PROMPT=0`, because there is no TTY and a prompt would hang
///   a submission;
/// * `GIT_OPTIONAL_LOCKS=0`, because every call here is a read and must never
///   fight the person's own git for the index lock.
fn run_git(at: &Path, args: &[&str]) -> Result<Said> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(at)
        .args(args)
        .env_clear()
        .env("PATH", path_of_the_host())
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    let output = command.output().with_context(|| {
        format!(
            "could not run git in {}: herdctl reads the commit a submission carries with git, \
             so git must be on the PATH",
            at.display()
        )
    })?;
    Ok(Said {
        status: output.status,
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Where to look for the `git` binary: the caller's `PATH`, and a last resort
/// so that a cleared environment never turns into "git is not installed".
fn path_of_the_host() -> OsString {
    std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Output;
    use tempfile::TempDir;

    /// A real git repository, built here.
    ///
    /// Every state these tests assert on is one the fixture put there with
    /// real `git init`, `commit` and `clone` — so "pushed" means a
    /// remote-tracking ref that actually exists, and a test can never pass on
    /// a premise it did not establish (D100, D101).
    struct Fixture {
        dir: TempDir,
    }

    /// The tracked files every fixture starts with: enough of them that the
    /// bounded refusal has something to bound.
    const TRACKED: usize = 12;

    impl Fixture {
        /// A source repository with one commit on `main`, and a clone of it.
        /// In the clone, `origin/main` really holds `HEAD`.
        ///
        /// `None` means git is missing and the test has skipped loudly.
        fn cloned(test: &str) -> Option<Fixture> {
            let fixture = Fixture::new(test)?;
            fixture.run("source", &["init", "-q", "--initial-branch=main", "."]);
            fixture.write("source", "README.md", "what the tests resolve\n");
            for n in 0..TRACKED {
                fixture.write("source", &format!("f{n:02}.txt"), "one\n");
            }
            fixture.git("source", &["add", "-A"]);
            fixture.git("source", &["commit", "-qm", "the commit under test"]);
            let source = fixture.at("source");
            let work = fixture.at("work");
            fixture.run(
                ".",
                &[
                    "clone",
                    "-q",
                    "--no-hardlinks",
                    &source.to_string_lossy(),
                    &work.to_string_lossy(),
                ],
            );
            Some(fixture)
        }

        /// A repository with one commit and no remote at all.
        fn lone(test: &str) -> Option<Fixture> {
            let fixture = Fixture::new(test)?;
            fixture.run("work", &["init", "-q", "--initial-branch=main", "."]);
            fixture.write("work", "README.md", "alone\n");
            fixture.git("work", &["add", "-A"]);
            fixture.git("work", &["commit", "-qm", "the only commit"]);
            Some(fixture)
        }

        fn new(test: &str) -> Option<Fixture> {
            if !git_is_here() {
                // D27: an assertion that could not run must be impossible to
                // mistake for one that passed.
                herd_runner::testing::skipped(
                    test,
                    "git is not on this host's PATH, so nothing about a requester's checkout \
                     could be read",
                );
                return None;
            }
            let dir = TempDir::new().expect("a temporary directory for the fixture");
            for made in ["home", "source", "work"] {
                std::fs::create_dir_all(dir.path().join(made)).expect("the fixture's directories");
            }
            Some(Fixture { dir })
        }

        /// The checkout under test: the clone where there is one, the lone
        /// repository otherwise.
        fn checkout(&self) -> Checkout {
            Checkout::found_from(&self.at("work")).expect("the fixture's own checkout")
        }

        fn at(&self, repo: &str) -> PathBuf {
            self.dir.path().join(repo)
        }

        /// One git command in the fixture, with a config of its own: the host's
        /// `~/.gitconfig` and `/etc/gitconfig` are out of the way, so these
        /// tests say the same thing on every machine.
        fn run(&self, repo: &str, args: &[&str]) -> Output {
            let output = Command::new("git")
                .arg("-C")
                .arg(self.at(repo))
                .args(args)
                .env_clear()
                .env("PATH", path_of_the_host())
                .env("HOME", self.at("home"))
                .env("LC_ALL", "C")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
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
                "the fixture's own git failed: git {}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            output
        }

        /// What the fixture's own git says, trimmed: the independent reading a
        /// test compares the module against.
        fn git(&self, repo: &str, args: &[&str]) -> String {
            String::from_utf8_lossy(&self.run(repo, args).stdout)
                .trim()
                .to_owned()
        }

        fn write(&self, repo: &str, rel: &str, text: &str) {
            let path = self.at(repo).join(rel);
            std::fs::write(path, text).expect("the fixture writes a file");
        }
    }

    fn git_is_here() -> bool {
        match Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// The refusal, as a reader would see it.
    fn refusal<T>(result: Result<T>) -> String {
        match result {
            Ok(_) => panic!("this was meant to be refused and was accepted"),
            Err(why) => why.to_string(),
        }
    }

    #[test]
    fn a_branch_resolves_to_the_hash_its_own_git_holds() {
        let Some(fixture) = Fixture::cloned("a_branch_resolves_to_the_hash_its_own_git_holds")
        else {
            return;
        };
        let held = fixture.git("work", &["rev-parse", "main"]);
        let commit = fixture.checkout().resolve("main").expect("main resolves");
        assert_eq!(commit.hash, held);
        assert_eq!(commit.named, "main");
        assert!(
            is_whole_hash(&commit.hash),
            "{} is not forty hex",
            commit.hash
        );
    }

    #[test]
    fn head_resolves_to_the_commit_the_branch_is_on() {
        let Some(fixture) = Fixture::cloned("head_resolves_to_the_commit_the_branch_is_on") else {
            return;
        };
        let held = fixture.git("work", &["rev-parse", "HEAD"]);
        let commit = fixture.checkout().resolve("HEAD").expect("HEAD resolves");
        assert_eq!(commit.hash, held);
        // The default a caller passes is kept as the person typed it.
        assert_eq!(commit.named, "HEAD");
    }

    #[test]
    fn a_short_hash_resolves_to_the_whole_forty() {
        let Some(fixture) = Fixture::cloned("a_short_hash_resolves_to_the_whole_forty") else {
            return;
        };
        let whole = fixture.git("work", &["rev-parse", "HEAD"]);
        let short = &whole[..8];
        let commit = fixture
            .checkout()
            .resolve(short)
            .expect("a short hash resolves");
        assert_eq!(commit.hash, whole);
        assert_eq!(commit.named, short);
    }

    #[test]
    fn a_name_this_checkout_does_not_know_is_refused_by_name() {
        let Some(fixture) =
            Fixture::cloned("a_name_this_checkout_does_not_know_is_refused_by_name")
        else {
            return;
        };
        // Established: the fixture's own git does not know this name either.
        let known = fixture.git("work", &["branch", "--list", "no-such-branch"]);
        assert_eq!(known, "", "the fixture was meant to have no such branch");
        let why = refusal(fixture.checkout().resolve("no-such-branch"));
        assert!(why.contains("'no-such-branch'"), "{why}");
        assert!(why.contains("is not a ref or a commit"), "{why}");
    }

    #[test]
    fn a_name_that_is_not_a_commit_is_refused_saying_what_it_is() {
        let Some(fixture) =
            Fixture::cloned("a_name_that_is_not_a_commit_is_refused_saying_what_it_is")
        else {
            return;
        };
        // Established: this name resolves, and what it resolves to is a tree.
        let tree = fixture.git("work", &["rev-parse", "HEAD^{tree}"]);
        assert_eq!(fixture.git("work", &["cat-file", "-t", &tree]), "tree");
        let why = refusal(fixture.checkout().resolve("HEAD^{tree}"));
        assert!(why.contains("names a tree"), "{why}");
        assert!(why.contains("not a commit"), "{why}");
    }

    #[test]
    fn a_tracked_modification_is_refused_with_its_path() {
        let Some(fixture) = Fixture::cloned("a_tracked_modification_is_refused_with_its_path")
        else {
            return;
        };
        fixture.write("work", "README.md", "changed under the commit\n");
        // Established: git itself sees one tracked modification.
        assert_eq!(
            fixture.git(
                "work",
                &["status", "--porcelain=v1", "--untracked-files=no"]
            ),
            "M README.md"
        );
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains("1 change that is in no commit"), "{why}");
        assert!(why.contains("README.md"), "{why}");
        assert!(why.contains("commit or stash it"), "{why}");
    }

    #[test]
    fn a_staged_change_is_refused_with_its_path() {
        let Some(fixture) = Fixture::cloned("a_staged_change_is_refused_with_its_path") else {
            return;
        };
        fixture.write("work", "staged.txt", "added to the index only\n");
        fixture.git("work", &["add", "staged.txt"]);
        // Established: the change is in the index, not in any commit.
        assert_eq!(
            fixture.git(
                "work",
                &["status", "--porcelain=v1", "--untracked-files=no"]
            ),
            "A  staged.txt"
        );
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains("staged.txt"), "{why}");
        assert!(why.contains("1 change that is in no commit"), "{why}");
    }

    #[test]
    fn a_rename_is_one_change_named_once() {
        let Some(fixture) = Fixture::cloned("a_rename_is_one_change_named_once") else {
            return;
        };
        fixture.git("work", &["mv", "f00.txt", "renamed.txt"]);
        // Established: git reports this as a rename, with both paths.
        let status = fixture.git(
            "work",
            &["status", "--porcelain=v1", "--untracked-files=no"],
        );
        assert_eq!(status, "R  f00.txt -> renamed.txt");
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains("1 change that is in no commit"), "{why}");
        assert!(why.contains("renamed.txt"), "{why}");
        // The source of a rename is not a second change.
        assert!(!why.contains("f00.txt"), "{why}");
    }

    #[test]
    fn an_untracked_file_is_not_refused() {
        let Some(fixture) = Fixture::cloned("an_untracked_file_is_not_refused") else {
            return;
        };
        fixture.write("work", "scratch.txt", "never in any commit\n");
        // Established: the file is there and git calls it untracked.
        assert_eq!(
            fixture.git(
                "work",
                &["status", "--porcelain=v1", "--untracked-files=normal"]
            ),
            "?? scratch.txt"
        );
        let held = fixture.git("work", &["rev-parse", "HEAD"]);
        let commit = fixture
            .checkout()
            .resolve("HEAD")
            .expect("an untracked file is not a tracked change");
        assert_eq!(commit.hash, held);
    }

    #[test]
    fn many_changed_files_give_a_sentence_a_person_can_read() {
        let Some(fixture) = Fixture::cloned("many_changed_files_give_a_sentence_a_person_can_read")
        else {
            return;
        };
        for n in 0..TRACKED {
            fixture.write("work", &format!("f{n:02}.txt"), "two\n");
        }
        // Established: git sees exactly twelve tracked modifications.
        let status = fixture.git(
            "work",
            &["status", "--porcelain=v1", "--untracked-files=no"],
        );
        assert_eq!(status.lines().count(), TRACKED);
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains("12 changes that are in no commit"), "{why}");
        assert!(why.contains("f00.txt"), "{why}");
        assert!(why.contains("and 7 more"), "{why}");
        // Five paths and a figure, not twelve paths: the sentence stays one a
        // person reads rather than scrolls.
        assert!(!why.contains("f11.txt"), "{why}");
        assert!(why.len() < 400, "{} characters: {why}", why.len());
    }

    #[test]
    fn a_clone_whose_branch_is_pushed_is_accepted() {
        let Some(fixture) = Fixture::cloned("a_clone_whose_branch_is_pushed_is_accepted") else {
            return;
        };
        // Established: a remote-tracking ref really holds this commit.
        let held = fixture.git("work", &["rev-parse", "HEAD"]);
        assert_eq!(
            fixture.git("work", &["rev-parse", "refs/remotes/origin/main"]),
            held
        );
        let commit = fixture
            .checkout()
            .resolve("main")
            .expect("a pushed branch is submittable");
        assert_eq!(commit.hash, held);
    }

    #[test]
    fn a_commit_no_remote_holds_is_refused_by_hash() {
        let Some(fixture) = Fixture::cloned("a_commit_no_remote_holds_is_refused_by_hash") else {
            return;
        };
        fixture.write("work", "README.md", "a commit that was never pushed\n");
        fixture.git("work", &["add", "-A"]);
        fixture.git("work", &["commit", "-qm", "local only"]);
        let local = fixture.git("work", &["rev-parse", "HEAD"]);
        // Established: the tree is clean, there is a remote, and no
        // remote-tracking ref holds this commit.
        assert_eq!(
            fixture.git(
                "work",
                &["status", "--porcelain=v1", "--untracked-files=no"]
            ),
            ""
        );
        assert_eq!(fixture.git("work", &["remote"]), "origin");
        assert_eq!(
            fixture.git("work", &["branch", "--remotes", "--contains", &local]),
            ""
        );
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains(&local), "{why}");
        assert!(why.contains("on no remote-tracking ref of origin"), "{why}");
        assert!(why.contains("push the branch that holds it"), "{why}");
    }

    #[test]
    fn a_checkout_with_no_remote_at_all_is_refused() {
        let Some(fixture) = Fixture::lone("a_checkout_with_no_remote_at_all_is_refused") else {
            return;
        };
        // Established: one commit, a clean tree, and not one remote.
        let held = fixture.git("work", &["rev-parse", "HEAD"]);
        assert_eq!(fixture.git("work", &["remote"]), "");
        assert_eq!(
            fixture.git(
                "work",
                &["status", "--porcelain=v1", "--untracked-files=no"]
            ),
            ""
        );
        let why = refusal(fixture.checkout().resolve("HEAD"));
        assert!(why.contains(&held), "{why}");
        assert!(why.contains("has no remote at all"), "{why}");
        assert!(why.contains("Add a remote and push"), "{why}");
    }

    #[test]
    fn the_checkout_is_found_from_a_directory_below_its_root() {
        let Some(fixture) =
            Fixture::cloned("the_checkout_is_found_from_a_directory_below_its_root")
        else {
            return;
        };
        let deep = fixture.at("work").join("deep").join("deeper");
        std::fs::create_dir_all(&deep).expect("a directory below the root");
        let found = Checkout::found_from(&deep).expect("the checkout is found upward");
        // Compared against what the fixture's own git calls the root.
        assert_eq!(
            found.root(),
            Path::new(&fixture.git("work", &["rev-parse", "--show-toplevel"]))
        );
    }

    #[test]
    fn a_directory_that_is_no_checkout_is_refused() {
        let Some(fixture) = Fixture::cloned("a_directory_that_is_no_checkout_is_refused") else {
            return;
        };
        let plain = fixture.at("home");
        // Established: git itself finds no repository there.
        let found = Checkout::found_from(&plain);
        let why = refusal(found);
        assert!(why.contains("no git checkout at"), "{why}");
        assert!(why.contains(&plain.display().to_string()), "{why}");
        assert!(why.contains("run it inside the checkout"), "{why}");
    }

    #[test]
    fn a_broken_repository_does_not_read_as_a_dirty_one() {
        let Some(fixture) = Fixture::cloned("a_broken_repository_does_not_read_as_a_dirty_one")
        else {
            return;
        };
        let checkout = fixture.checkout();
        // Established: the checkout was readable, and then its git directory
        // was taken away under it.
        checkout
            .refuse_if_dirty()
            .expect("clean before it is broken");
        std::fs::remove_dir_all(fixture.at("work").join(".git")).expect("the git directory goes");
        let why = refusal(checkout.refuse_if_dirty());
        assert!(why.contains("could not read the state"), "{why}");
        assert!(why.contains("git said:"), "{why}");
        assert!(
            !why.contains("in no commit"),
            "a broken repository read as a dirty one: {why}"
        );
    }
}
