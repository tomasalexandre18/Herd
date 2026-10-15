//! A job's requirements, read on a runner and never guessed (D74), and the
//! key the answer is kept under (D82).
//!
//! A declared command may say `labels_from` instead of static labels: the
//! labels a test needs depend on the scenarios it selects, and only the
//! consumer's own tool knows them. So the hub cannot place such a job until
//! something has run that tool's reading — an ordinary job on a runner,
//! confined and recorded like any other — and read what it printed.
//!
//! What lives here is **pure**, and it is only the two pieces the
//! orchestration is built on, plus the two limits D74 puts on it.
//!
//! * [`Answer::read`] — the strict reading of what the reading printed.
//! * [`ReadingKey`] — the key the answer is cached under.
//! * [`verdict`] — given how the reading job *ended*, whether there is an
//!   answer to read at all.
//! * [`Needs`] and [`ReadingJob`] — the no-recursion limit, as types rather
//!   than as a comment.
//!
//! What is deliberately not here: submitting the reading, looking in the
//! cache, writing the figures onto the real job's record, placing it. Those
//! are the hub's, and they read as a few lines on top of this.
//!
//! # The reading is read strictly, like the capability file (D60)
//!
//! The reading prints lines of `key=value`, and the keys are the consumer's
//! contract: [`LABELS`], [`MEMORY_MIB`], [`TMPFS_MIB`], [`SCENARIOS`]. All
//! four are required, each exactly once, in any order — the consumer's own
//! two answers are not in alphabetical order, so the order cannot be part of
//! the contract.
//!
//! Everything else refuses the real job, naming what was wrong:
//!
//! * an **unknown** key, named, with the four that are not;
//! * a **missing** key, named;
//! * a **duplicated** key, named, with both values it was given;
//! * a figure that is **not a whole number of decimal digits**, named;
//! * a label that is not a label — through [`LabelSet`], so the one rule
//!   about what a label is stays in the one place that has it.
//!
//! **Three tolerances this reading does not have, and why.** This answer
//! decides which machine a job runs on. A reading that half-parses is how a
//! job ends up on the wrong machine, holding an exclusive GPU it never asked
//! for or wanting a tmpfs that is not there — and nothing would say so,
//! because a half-read answer looks exactly like a complete one.
//!
//! * **A blank line refuses**, and so does a line with no `=`. A reading
//!   that started printing something else has changed its contract, which is
//!   precisely the thing D74 wants to hear about by name rather than to skip
//!   over. The one exception is not a tolerance: an answer may end with one
//!   `\n`, which is the last line's terminator and not a line.
//! * **Nothing is trimmed.** No rule here has an exception, so each kind of
//!   stray whitespace is refused by the rule it actually breaks: `" labels"`
//!   is an unknown key, `" 3584"` is not decimal digits, `"kvm "` is not a
//!   label. A `\r\n` answer refuses for the same reason — the `\r` is in the
//!   value — which is why the lines are cut with `split('\n')` and not with
//!   [`str::lines`], whose silent `\r` strip would be a tolerance smuggled
//!   in from the standard library.
//! * **`labels=` with nothing after it refuses.** An empty label set is a
//!   legal thing for a *requester* to ask for, but it is not a legal
//!   *answer*: it cannot be told apart from a tool that computed nothing,
//!   and D74's rule is that a job is never placed on a guess.
//!
//! **One obligation the API cannot enforce, so it is written down.**
//! [`Answer::read`] is given the reading's **own output**, not the job's
//! whole log. The consumer's `up` and `fetch` steps print to the same stream
//! — how long a warm cache took, among other things (E-14) — and those lines
//! would refuse. The alternative is to scan a log for the lines that happen
//! to parse, which is exactly the half-parsing the strictness is for: it
//! would read `labels=build` out of a line of someone's prose. So the caller
//! hands over the `run` step's output, and a reading that printed anything
//! else is refused rather than mined.
//!
//! # What `scenarios` is for
//!
//! Nothing Herd decides. The three figures Herd uses are the labels, the
//! memory and the tmpfs — they go onto the job's record and admission weighs
//! them (E-29, E-31). `scenarios` is a count the consumer reports for
//! information, and it is kept here for one purpose: [`Answer::line`], the
//! sentence an explanation quotes (E-04, E-36), so a human reading why a job
//! went where it went sees how many scenarios asked for it.
//!
//! It is still **required**, and a non-numeric `scenarios` still refuses.
//! The strict reading is about the *shape* of the answer, not about which of
//! its figures Herd happens to consume: a reading that stopped printing
//! `scenarios`, or started printing a word there, is a reading whose
//! contract moved, and D74 exists so that this is a sentence rather than a
//! silence.

use std::fmt;

use herd_proto::{Declaration, JobId, JobOutcome, LabelSet};

/// The comma-separated list of capabilities the job needs.
pub const LABELS: &str = "labels";
/// How much memory the whole job needs, in MiB (E-29).
pub const MEMORY_MIB: &str = "memory_mib";
/// How much tmpfs it reserves while it runs, in MiB (E-31).
pub const TMPFS_MIB: &str = "tmpfs_mib";
/// How many scenarios the arguments selected. Reported, never placed on.
pub const SCENARIOS: &str = "scenarios";

/// The four keys a reading says, in the order a sentence names them.
///
/// Sorted, so that the sentence refusing an unknown or a missing key reads
/// the same way every time and a person can scan it for the one they typed.
pub const KEYS: [&str; 4] = [LABELS, MEMORY_MIB, SCENARIOS, TMPFS_MIB];

/// The four keys as they read inside a sentence.
fn keys_said() -> String {
    KEYS.iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Why the real job is refused, in one sentence a person can act on (E-36).
///
/// It carries a clause rather than a whole sentence, because the same clause
/// has to read correctly in two places: on its own, and attributed to the
/// reading job that produced it. [`Refusal::naming`] is the second one, and
/// it is the one the hub writes onto the real job's record — a reader of that
/// record has to be able to go and look at the reading itself (E-15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(String);

impl Refusal {
    /// A clause that follows "the reading …".
    fn new(clause: impl Into<String>) -> Self {
        Self(clause.into())
    }

    /// The sentence on its own.
    pub fn reason(&self) -> String {
        format!("the reading {}", self.0)
    }

    /// The sentence that refuses the real job, naming the reading job whose
    /// answer refused it.
    pub fn naming(&self, reading: &JobId) -> String {
        format!(
            "this job's requirements were read by job {reading}, which {}",
            self.0
        )
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for Refusal {}

/// What a reading said: the labels the job needs, and its figures.
///
/// There is no variant of this for "and then read again": the labels are a
/// [`LabelSet`], which is a set of names and cannot spell a further reading.
/// That is D74's no-recursion limit, and it is why [`Answer::needs`] can only
/// ever answer [`Needs::Known`]. See [`Needs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The capabilities the job needs (E-25), canonical through [`LabelSet`].
    pub labels: LabelSet,
    /// What the whole job needs, in MiB — [`crate::JobRecord::memory_mib`].
    pub memory_mib: u64,
    /// What it reserves on its runner's tmpfs, in MiB —
    /// [`crate::JobRecord::tmpfs_mib`].
    pub tmpfs_mib: u64,
    /// How many scenarios the arguments selected. Reported, never placed on;
    /// see the module's own note on it.
    pub scenarios: u64,
}

impl Answer {
    /// Read what a reading printed, strictly (D74).
    ///
    /// Given the reading's own output; see the module's note on that. Every
    /// refusal names the key it is about.
    pub fn read(output: &str) -> Result<Self, Refusal> {
        let body = output.strip_suffix('\n').unwrap_or(output);
        if body.is_empty() {
            return Err(Refusal::new(format!(
                "printed nothing, and a reading says all four of {}",
                keys_said()
            )));
        }

        // Four slots, filled once each. The first value is kept and the
        // second refuses: a reading that said a figure twice does not know
        // which one it means, and taking either of them is the guess D74
        // forbids.
        let mut labels: Option<&str> = None;
        let mut memory_mib: Option<&str> = None;
        let mut tmpfs_mib: Option<&str> = None;
        let mut scenarios: Option<&str> = None;

        for line in body.split('\n') {
            let Some((key, value)) = line.split_once('=') else {
                return Err(if line.is_empty() {
                    Refusal::new("printed a blank line, and every line of a reading is key=value")
                } else {
                    Refusal::new(format!("printed the line {line:?}, which is not key=value"))
                });
            };
            let slot = match key {
                LABELS => &mut labels,
                MEMORY_MIB => &mut memory_mib,
                TMPFS_MIB => &mut tmpfs_mib,
                SCENARIOS => &mut scenarios,
                unknown => {
                    return Err(Refusal::new(format!(
                        "printed the key {unknown:?}, which is not one of {}",
                        keys_said()
                    )))
                }
            };
            if let Some(first) = *slot {
                return Err(Refusal::new(format!(
                    "printed {key:?} twice, as {first:?} and then {value:?}, \
                     and a reading says each key once"
                )));
            }
            *slot = Some(value);
        }

        Ok(Self {
            labels: label_set(require(LABELS, labels)?)?,
            memory_mib: figure(
                MEMORY_MIB,
                require(MEMORY_MIB, memory_mib)?,
                "a number of MiB",
            )?,
            tmpfs_mib: figure(TMPFS_MIB, require(TMPFS_MIB, tmpfs_mib)?, "a number of MiB")?,
            scenarios: figure(SCENARIOS, require(SCENARIOS, scenarios)?, "a count")?,
        })
    }

    /// The one door the hub uses when a reading job ends: the verdict on how
    /// it ended, and then the strict reading of what it printed (D74).
    ///
    /// One call rather than two, so that a caller cannot read the output of a
    /// reading that failed.
    pub fn of_reading(outcome: &JobOutcome, output: &str) -> Result<Self, Refusal> {
        verdict(outcome)?;
        Self::read(output)
    }

    /// What the real job needs now that the reading has answered.
    ///
    /// Always [`Needs::Known`]. Not a policy and not a `match` somebody could
    /// add an arm to: an [`Answer`] holds a [`LabelSet`], and there is no way
    /// to spell a second reading in one (D74).
    pub fn needs(&self) -> Needs {
        Needs::Known(self.labels.clone())
    }

    /// The one sentence an explanation quotes (E-04, E-36), with all four
    /// figures in it — which is what `scenarios` is kept for.
    pub fn line(&self) -> String {
        format!(
            "the reading says {} scenarios: labels {}, {} MiB, {} MiB of tmpfs",
            self.scenarios, self.labels, self.memory_mib, self.tmpfs_mib
        )
    }
}

/// A key the reading did not print.
fn require<'a>(key: &str, value: Option<&'a str>) -> Result<&'a str, Refusal> {
    value.ok_or_else(|| {
        Refusal::new(format!(
            "printed no {key:?}, and a reading says all four of {}",
            keys_said()
        ))
    })
}

/// One figure: decimal digits and nothing else.
///
/// Not [`str::parse`] on its own, which accepts a leading `+` and would read
/// `+3584` as a figure the consumer never printed. Digits first, then the
/// parse, so that too large a figure refuses as the number it is rather than
/// as a shrug.
fn figure(key: &str, value: &str, what: &str) -> Result<u64, Refusal> {
    let printed = format!("{key}={value}");
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Refusal::new(format!(
            "printed {printed:?}, and {key} is {what} written in decimal digits"
        )));
    }
    value.parse::<u64>().map_err(|_| {
        Refusal::new(format!(
            "printed {printed:?}, and {key} is {what} that has to fit in 64 bits"
        ))
    })
}

/// The label list, through the one rule about what a label is.
///
/// [`LabelSet`] sorts, deduplicates and checks each name, so `kvm,kvm` is one
/// label and `GPU` is a refusal — and the sentence it refuses with is the
/// same sentence a bad label in a submission or in a capability file gets.
fn label_set(value: &str) -> Result<LabelSet, Refusal> {
    if value.is_empty() {
        return Err(Refusal::new(format!(
            "printed {LABELS:?} with nothing after it, and a reading that needs \
             no capability is not the same thing as a reading that computed none"
        )));
    }
    LabelSet::new(value.split(',')).map_err(|bad| {
        Refusal::new(format!(
            "printed \"{LABELS}={value}\", which is not a label set: {bad}"
        ))
    })
}

/// Whether a reading that has ended left an answer to read (D74).
///
/// A job is never placed on a guess, so every ending but a clean exit refuses
/// the real job, naming how the reading ended — the exit code, the signal,
/// the cap and its figure. [`JobOutcome`] is the input because it already
/// carries every way a job can end, and a verdict written against a narrower
/// type would have to decide what to do about a kind of ending it had never
/// been shown.
pub fn verdict(outcome: &JobOutcome) -> Result<(), Refusal> {
    match outcome {
        JobOutcome::Exited { code: 0 } => Ok(()),
        JobOutcome::Exited { code } => Err(Refusal::new(format!(
            "exited {code} rather than 0, so whatever it printed is not an answer"
        ))),
        JobOutcome::Signalled { signal } => Err(Refusal::new(format!(
            "was killed by signal {signal} before it could say"
        ))),
        JobOutcome::CouldNotStart { reason } => {
            Err(Refusal::new(format!("could not be started: {reason}")))
        }
        JobOutcome::StoppedByCap { cap, limit, detail } => {
            let mut said = format!(
                "was stopped at its {} cap of {limit} {}",
                cap.name(),
                cap.unit()
            );
            if let Some(detail) = detail {
                said.push_str(&format!(" ({detail})"));
            }
            Err(Refusal::new(said))
        }
        JobOutcome::Cancelled { reason } => Err(Refusal::new(format!("was cancelled: {reason}"))),
        JobOutcome::Lost { reason } => Err(Refusal::new(format!("was lost: {reason}"))),
        JobOutcome::Impossible { reason } => {
            Err(Refusal::new(format!("could never have run: {reason}")))
        }
    }
}

/// What a job needs before it can be placed: labels that are known, or the
/// reading that will say (D74).
///
/// **This is where the no-recursion limit lives, and it lives as a type.**
/// The graph is `Needs` → [`ReadingJob`] → ([`ReadingKey`], `String`,
/// [`LabelSet`]), and it has no cycle: a `ReadingJob` has nowhere to put a
/// `Needs`, so a chain of two readings has no representation at all. The two
/// things that could have started such a chain both answer `Known` without a
/// `match`: [`ReadingJob::needs`], whose labels are the baseline it was
/// given, and [`Answer::needs`], whose labels are the ones it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Needs {
    /// Place the job against these.
    Known(LabelSet),
    /// Read them first, with this.
    Read(ReadingJob),
}

impl Needs {
    /// The labels, when they are known.
    pub fn known(&self) -> Option<&LabelSet> {
        match self {
            Needs::Known(labels) => Some(labels),
            Needs::Read(_) => None,
        }
    }

    /// The reading to run first, when there is one.
    pub fn reading(&self) -> Option<&ReadingJob> {
        match self {
            Needs::Known(_) => None,
            Needs::Read(reading) => Some(reading),
        }
    }
}

/// The reading of one command's requirements, as the ordinary job it is.
///
/// Everything the hub needs to submit it and to cache its answer, and nothing
/// else. Its own labels are a [`LabelSet`] — the command's declared baseline
/// (D74) — which is what makes it unable to ask for a reading of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingJob {
    key: ReadingKey,
    line: String,
    baseline: LabelSet,
}

impl ReadingJob {
    /// The reading of one declared command, for one commit, one argument
    /// list and the `-j` the job was given.
    ///
    /// It derives the cache key itself rather than taking one, so that the
    /// key and the reading it belongs to cannot disagree.
    pub fn for_command(
        declaration: &Declaration,
        parallelism: Option<u32>,
        line: impl Into<String>,
        baseline: LabelSet,
    ) -> Self {
        Self {
            key: ReadingKey::of(declaration, parallelism),
            line: line.into(),
            baseline,
        }
    }

    /// The key its answer is cached under.
    pub fn key(&self) -> &ReadingKey {
        &self.key
    }

    /// The line the reading job runs: the command's `labels_from`, filled.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The labels the reading job itself is placed against (D74): the
    /// command's declared baseline, which for the first consumer is the
    /// capability their tool needs in order to run at all.
    pub fn baseline(&self) -> &LabelSet {
        &self.baseline
    }

    /// What the reading job needs before *it* can be placed: its baseline.
    ///
    /// Always [`Needs::Known`], by construction rather than by choice — see
    /// [`Needs`]. This is D74's "a command reached through `labels_from`
    /// never itself resolves a `labels_from`", said in a way that cannot be
    /// got wrong: the hub asks every job it is about to place what it needs,
    /// a reading job included, and a reading job's answer is its baseline.
    pub fn needs(&self) -> Needs {
        Needs::Known(self.baseline.clone())
    }
}

/// The key a reading's answer is cached under: (commit, command, args), where
/// **the derived `-j` is part of the args** (D74, D82).
///
/// # Why those three, and why they are enough
///
/// The consumer confirmed it: their reading reads only its arguments and the
/// scenario table compiled into that commit, and nothing on the machine — no
/// load, no free memory, no free tmpfs. So the answer is a property of the
/// three, the cache is sound, and without it every submission would pay a
/// reading.
///
/// # Why the `-j` is in the key, and why the ordering rule is in the signature
///
/// Their figures are computed **for the `-j` they were given**. N comes from
/// the machine the hub chose (E-22, D83), and the job's memory requirement
/// comes from a reading that needs N — which is circular unless the order is
/// fixed. D82 fixes it: **N first, requirements second, placement third**,
/// with N written onto [`crate::JobRecord::parallelism`] by the write that
/// numbers the attempt.
///
/// This is where that order stops being a rule somebody has to remember.
/// Neither constructor can produce a key without being handed the figure, so
/// a caller that has not chosen N has nothing to look the cache up with. A
/// requeue onto a differently sized machine derives its own N and therefore
/// looks up a different key, which is exactly right: the old answer was for
/// the old machine's N.
///
/// N is carried as a piece of its own rather than spliced into the argument
/// list, because whether it reaches the command through the requester's
/// typing (`-j 4` among the args) or through the hub's filling of `{j}` in
/// the line (D107) is an accident of how the consumer wrote the declaration,
/// and the key must not depend on which. `None` — a job that asked for no
/// derivation — is a key of its own, distinct from every `Some`.
///
/// # Why it is length-prefixed
///
/// The same reason [`crate::job::dedup_key`] is, and this is the same key
/// family: `["a b", "c"]` and `["a", "b c"]` are different arguments, and any
/// key built by joining strings with a separator makes them equal, whatever
/// separator is chosen, since an argument may contain it. Each piece is its
/// length in bytes, a colon, then the bytes — so the encoding can be parsed
/// back and two different argument lists can never collide. Two readings
/// sharing a cache entry they should not share is a job placed on another
/// job's requirements, which is the one failure this key exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadingKey(String);

impl ReadingKey {
    /// The key for a commit, a command name, an argument list and the `-j`
    /// the job was given.
    pub fn new(commit: &str, command: &str, args: &[String], parallelism: Option<u32>) -> Self {
        let mut key = String::new();
        key.push_str("commit ");
        push_counted(&mut key, commit);
        key.push_str(" command ");
        push_counted(&mut key, command);
        key.push_str(" args ");
        for arg in args {
            push_counted(&mut key, arg);
        }
        key.push_str(" j ");
        match parallelism {
            Some(threads) => push_counted(&mut key, &threads.to_string()),
            // A counted piece always begins with a digit, so this word can
            // never be one: "no derivation" and "-j 4" cannot collide.
            None => key.push_str("none"),
        }
        Self(key)
    }

    /// The key for a resolved declaration, which is where the three come from
    /// on a real submission, and the `-j` the job was given.
    pub fn of(declaration: &Declaration, parallelism: Option<u32>) -> Self {
        Self::new(
            &declaration.commit,
            &declaration.command,
            &declaration.args,
            parallelism,
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReadingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One piece of a key, as its byte length and its bytes.
fn push_counted(key: &mut String, piece: &str) {
    key.push_str(&piece.len().to_string());
    key.push(':');
    key.push_str(piece);
}

#[cfg(test)]
mod tests {
    use super::*;

    use herd_proto::{Cap, Executor};

    /// The first consumer's own first answer, from issue #16, verbatim. Their
    /// command was
    /// `cargo xtask test --only unit,posix,desktop,games-gl -j 4 --labels`.
    const FOUR_SCENARIOS: &str = "\
labels=build,gpu-venus,kvm,reference
tmpfs_mib=4096
memory_mib=3584
scenarios=4
";

    /// Their second one, verbatim, from
    /// `cargo xtask test --pack <dir> --only posix,desktop -j 2 --labels`.
    const TWO_SCENARIOS: &str = "\
labels=kvm
tmpfs_mib=4096
memory_mib=3584
scenarios=2
";

    fn declaration(commit: &str, command: &str, args: &[&str]) -> Declaration {
        Declaration {
            command: command.to_owned(),
            commit: commit.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            executor: Executor::default(),
        }
    }

    fn a_commit() -> String {
        "cb5a2e8b".repeat(5)
    }

    /// Read an answer that is expected to be read, or say what refused it.
    fn read(output: &str) -> Answer {
        match Answer::read(output) {
            Ok(answer) => answer,
            Err(refused) => panic!("this answer is the consumer's own: {refused}"),
        }
    }

    /// Refuse an answer that is expected to be refused, and hand back the
    /// sentence so the test can assert what it names.
    fn refused(output: &str) -> String {
        match Answer::read(output) {
            Ok(answer) => panic!("this was read as an answer rather than refused: {answer:?}"),
            Err(refusal) => refusal.reason(),
        }
    }

    // ---- the consumer's two real answers --------------------------------

    /// Their four-scenario answer parses to the figures it shows and the four
    /// labels in it. Note the key order — `labels`, `tmpfs_mib`,
    /// `memory_mib`, `scenarios` — which is not alphabetical, and which is
    /// why the order is not part of the contract.
    #[test]
    fn the_consumer_s_four_scenario_answer_reads_as_the_figures_it_shows() {
        let answer = read(FOUR_SCENARIOS);
        assert_eq!(
            answer.labels,
            LabelSet::new(["build", "gpu-venus", "kvm", "reference"])
                .expect("these four are labels")
        );
        assert_eq!(answer.memory_mib, 3584);
        assert_eq!(answer.tmpfs_mib, 4096);
        assert_eq!(answer.scenarios, 4);
    }

    /// Their pack answer: one label, the same two figures, two scenarios.
    #[test]
    fn the_consumer_s_two_scenario_answer_reads_as_the_figures_it_shows() {
        let answer = read(TWO_SCENARIOS);
        assert_eq!(answer.labels, LabelSet::of("kvm").expect("kvm is a label"));
        assert_eq!(answer.memory_mib, 3584);
        assert_eq!(answer.tmpfs_mib, 4096);
        assert_eq!(answer.scenarios, 2);
    }

    /// The sentence an explanation quotes carries all four figures — which is
    /// the whole reason `scenarios` is kept at all.
    #[test]
    fn the_answer_s_line_quotes_all_four_figures_including_the_scenarios() {
        let said = read(FOUR_SCENARIOS).line();
        assert!(said.contains("4 scenarios"), "{said}");
        assert!(said.contains("build, gpu-venus, kvm, reference"), "{said}");
        assert!(said.contains("3584 MiB"), "{said}");
        assert!(said.contains("4096 MiB of tmpfs"), "{said}");
    }

    /// An answer with no trailing newline is the same answer: the one `\n` a
    /// tool ends on is the last line's terminator, not a line.
    #[test]
    fn a_trailing_newline_is_a_terminator_and_not_a_line() {
        let with = read(FOUR_SCENARIOS);
        let without = read(FOUR_SCENARIOS.trim_end_matches('\n'));
        assert_eq!(with, without);
    }

    // ---- the five refusals ----------------------------------------------

    /// An unknown key refuses, naming the key it does not know and the four
    /// it does — so a person who typed `tmpfs` reads what they should have
    /// typed.
    #[test]
    fn an_unknown_key_refuses_naming_it_and_the_four_it_is_not() {
        let said =
            refused("labels=kvm\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\ntmpfs=4096\n");
        assert!(said.contains("\"tmpfs\""), "{said}");
        for key in KEYS {
            assert!(said.contains(&format!("{key:?}")), "{said}");
        }
    }

    /// Each of the four keys, left out on its own, refuses naming that key.
    #[test]
    fn a_missing_key_refuses_naming_the_key_that_is_missing() {
        let whole = [
            "labels=kvm",
            "memory_mib=3584",
            "tmpfs_mib=4096",
            "scenarios=2",
        ];
        for left_out in 0..whole.len() {
            let (key, _) = whole[left_out]
                .split_once('=')
                .expect("every line of the fixture is key=value");
            let partial = whole
                .iter()
                .enumerate()
                .filter(|(at, _)| *at != left_out)
                .map(|(_, line)| *line)
                .collect::<Vec<_>>()
                .join("\n");
            let said = refused(&partial);
            assert!(
                said.contains(&format!("printed no {key:?}")),
                "leaving out {key} refused with: {said}"
            );
        }
    }

    /// A duplicated key refuses naming the key and **both** values, so a
    /// person can see the two figures that disagreed. Taking either of them
    /// would be the guess D74 forbids.
    #[test]
    fn a_duplicated_key_refuses_naming_the_key_and_both_its_values() {
        let said =
            refused("labels=kvm\nmemory_mib=3584\nmemory_mib=7168\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(said.contains("\"memory_mib\" twice"), "{said}");
        assert!(said.contains("\"3584\""), "{said}");
        assert!(said.contains("\"7168\""), "{said}");
    }

    /// A figure that is not a figure refuses naming the key, for each of the
    /// three figures. `labels` is not here: it has its own rule, below.
    #[test]
    fn a_figure_that_is_not_numeric_refuses_naming_the_key() {
        for (bad, key) in [
            (
                "labels=kvm\nmemory_mib=lots\ntmpfs_mib=4096\nscenarios=2\n",
                MEMORY_MIB,
            ),
            (
                "labels=kvm\nmemory_mib=3584\ntmpfs_mib=4g\nscenarios=2\n",
                TMPFS_MIB,
            ),
            (
                "labels=kvm\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=two\n",
                SCENARIOS,
            ),
        ] {
            let said = refused(bad);
            assert!(
                said.contains(key) && said.contains("decimal digits"),
                "{key} refused with: {said}"
            );
        }
    }

    /// A label that is not a label refuses through [`LabelSet`], so the
    /// sentence is the one rule's own sentence and not a second copy of it.
    #[test]
    fn a_label_that_is_not_a_label_refuses_through_the_one_rule() {
        let said = refused("labels=build,GPU\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(said.contains("labels=build,GPU"), "{said}");
        // The sentence `check_label` itself writes, borrowed rather than
        // reinvented here.
        let own = herd_proto::check_label("GPU").expect_err("GPU is not a label");
        assert!(said.contains(&own.to_string()), "{said}");
    }

    /// A figure no `u64` can hold refuses as the number it is rather than as
    /// a shrug.
    #[test]
    fn a_figure_too_large_for_sixty_four_bits_refuses_naming_the_key() {
        let said = refused(
            "labels=kvm\nmemory_mib=99999999999999999999999\ntmpfs_mib=4096\nscenarios=2\n",
        );
        assert!(said.contains("memory_mib"), "{said}");
        assert!(said.contains("64 bits"), "{said}");
    }

    /// A leading `+` is not decimal digits. `str::parse` would have taken it,
    /// which is why the digits are checked first.
    #[test]
    fn a_signed_figure_refuses_even_though_rust_would_parse_it() {
        assert_eq!("+3584".parse::<u64>(), Ok(3584), "this is what is refused");
        let said = refused("labels=kvm\nmemory_mib=+3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(said.contains("memory_mib=+3584"), "{said}");
    }

    // ---- the tolerances this reading does not have ----------------------

    /// `labels=` with nothing after it refuses naming the key: it cannot be
    /// told apart from a tool that computed nothing.
    #[test]
    fn an_empty_label_list_refuses_naming_the_key() {
        let said = refused("labels=\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(said.contains("\"labels\""), "{said}");
        assert!(said.contains("computed none"), "{said}");
    }

    /// A blank line in the middle refuses, saying what every line of a
    /// reading is.
    #[test]
    fn a_blank_line_refuses() {
        let said = refused("labels=kvm\n\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(said.contains("blank line"), "{said}");
        assert!(said.contains("key=value"), "{said}");
    }

    /// A line with no `=` refuses, quoting the line — which is what a reading
    /// printing its own progress onto the same stream looks like.
    #[test]
    fn a_line_with_no_equals_refuses_quoting_it() {
        let said = refused(
            "warm cache pulled in 38 s\nlabels=kvm\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n",
        );
        assert!(said.contains("warm cache pulled in 38 s"), "{said}");
        assert!(said.contains("not key=value"), "{said}");
    }

    /// Nothing at all refuses saying so, rather than as four missing keys.
    #[test]
    fn a_reading_that_printed_nothing_refuses_saying_so() {
        for empty in ["", "\n"] {
            let said = refused(empty);
            assert!(said.contains("printed nothing"), "{said}");
        }
    }

    /// Whitespace is tolerated nowhere, and each kind is refused by the rule
    /// it actually breaks: a padded key is an unknown key, a padded figure is
    /// not decimal digits, a padded label is not a label.
    #[test]
    fn surrounding_whitespace_is_tolerated_nowhere() {
        let padded_key = refused(" labels=kvm\nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(padded_key.contains("\" labels\""), "{padded_key}");

        let padded_figure =
            refused("labels=kvm\nmemory_mib= 3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(
            padded_figure.contains("memory_mib= 3584"),
            "{padded_figure}"
        );

        let padded_label = refused("labels=kvm \nmemory_mib=3584\ntmpfs_mib=4096\nscenarios=2\n");
        assert!(padded_label.contains("labels=kvm "), "{padded_label}");
    }

    /// A `\r\n` answer refuses: the `\r` is in the value, and `split('\n')`
    /// is what lets it be seen at all.
    #[test]
    fn a_carriage_return_refuses_rather_than_being_stripped() {
        let said = refused("labels=kvm\r\nmemory_mib=3584\r\ntmpfs_mib=4096\r\nscenarios=2\r\n");
        assert!(said.contains("labels"), "{said}");
    }

    // ---- the verdict on how the reading ended ---------------------------

    /// A clean exit has an answer to read. Without this, every assertion
    /// below would hold just as well of a verdict that refused everything
    /// (D100).
    #[test]
    fn a_reading_that_exited_zero_has_an_answer_to_read() {
        assert_eq!(verdict(&JobOutcome::Exited { code: 0 }), Ok(()));
        let answer = Answer::of_reading(&JobOutcome::Exited { code: 0 }, FOUR_SCENARIOS)
            .expect("a clean exit and the consumer's own answer");
        assert_eq!(answer.scenarios, 4);
    }

    /// The sentence a refused ending gives, or a panic saying it was not
    /// refused.
    fn ended(outcome: &JobOutcome) -> String {
        match verdict(outcome) {
            Ok(()) => panic!("{outcome:?} was read as having an answer"),
            Err(refusal) => refusal.reason(),
        }
    }

    /// A non-zero exit refuses naming the code, and nothing it printed is
    /// read — even when what it printed is a perfectly good answer.
    #[test]
    fn a_non_zero_exit_refuses_naming_the_code_and_reads_nothing() {
        let said = ended(&JobOutcome::Exited { code: 1 });
        assert!(said.contains("exited 1"), "{said}");

        let refusal = Answer::of_reading(&JobOutcome::Exited { code: 1 }, FOUR_SCENARIOS)
            .expect_err("a reading that failed has no answer, whatever it printed");
        assert!(refusal.reason().contains("exited 1"), "{refusal}");
    }

    /// A timeout is the time cap, and the refusal says which cap and what its
    /// figure was — their `test` command declares two hours.
    #[test]
    fn a_timeout_refuses_naming_the_cap_and_its_figure() {
        let said = ended(&JobOutcome::StoppedByCap {
            cap: Cap::Time,
            limit: 7200,
            detail: None,
        });
        assert!(said.contains("time cap"), "{said}");
        assert!(said.contains("7200 s"), "{said}");
    }

    /// Another cap is named as itself, with its own unit, and a `detail` is
    /// carried rather than dropped — a figure alone can be a true number
    /// inside a false sentence.
    #[test]
    fn another_cap_is_named_as_itself_and_its_detail_is_carried() {
        let said = ended(&JobOutcome::StoppedByCap {
            cap: Cap::Memory,
            limit: 2_147_483_648,
            detail: Some("the cgroup could not be read".to_owned()),
        });
        assert!(said.contains("memory cap"), "{said}");
        assert!(said.contains("2147483648 bytes"), "{said}");
        assert!(said.contains("the cgroup could not be read"), "{said}");
    }

    /// A signal refuses naming the signal.
    #[test]
    fn a_signal_refuses_naming_it() {
        let said = ended(&JobOutcome::Signalled { signal: 9 });
        assert!(said.contains("signal 9"), "{said}");
    }

    /// A cancelled reading refuses, in the words of whoever cancelled it.
    #[test]
    fn a_cancelled_reading_refuses_in_the_words_it_was_cancelled_with() {
        let said = ended(&JobOutcome::Cancelled {
            reason: "its requester went away".to_owned(),
        });
        assert!(said.contains("was cancelled"), "{said}");
        assert!(said.contains("its requester went away"), "{said}");
    }

    /// The three remaining endings each refuse, each in their own words.
    #[test]
    fn the_other_endings_each_refuse_in_their_own_words() {
        for (outcome, expected) in [
            (
                JobOutcome::CouldNotStart {
                    reason: "no such executable".to_owned(),
                },
                "could not be started: no such executable",
            ),
            (
                JobOutcome::Lost {
                    reason: "its runner went silent".to_owned(),
                },
                "was lost: its runner went silent",
            ),
            (
                JobOutcome::Impossible {
                    reason: "no machine offers build".to_owned(),
                },
                "could never have run: no machine offers build",
            ),
        ] {
            let said = ended(&outcome);
            assert!(said.contains(expected), "{outcome:?} refused with: {said}");
        }
    }

    /// The sentence the hub writes onto the real job's record names the
    /// reading job, so a reader can go and look at it (E-15).
    #[test]
    fn the_refusal_the_real_job_gets_names_the_reading_job() {
        let refusal =
            verdict(&JobOutcome::Exited { code: 1 }).expect_err("a non-zero exit is refused");
        let said = refusal.naming(&JobId::new("7f3a9c21"));
        assert!(said.contains("job 7f3a9c21"), "{said}");
        assert!(said.contains("exited 1"), "{said}");
    }

    // ---- the cache key --------------------------------------------------

    fn key(commit: &str, command: &str, args: &[&str], parallelism: Option<u32>) -> ReadingKey {
        let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        ReadingKey::new(commit, command, &owned, parallelism)
    }

    /// The same three, with the same `-j`, are one key — otherwise the cache
    /// would never hit and every submission would pay a reading.
    #[test]
    fn the_same_commit_command_and_args_are_one_key() {
        let commit = a_commit();
        assert_eq!(
            key(&commit, "test", &["--only", "posix", "-j", "4"], Some(4)),
            key(&commit, "test", &["--only", "posix", "-j", "4"], Some(4))
        );
    }

    /// Two readings differing only in their `-j` are two keys. This is D82:
    /// their figures are computed *for the N they were given*, so one cache
    /// entry for both would hand a job the other N's memory figure.
    #[test]
    fn two_readings_differing_only_in_their_j_are_two_keys() {
        let commit = a_commit();
        let args = ["--only", "posix,desktop"];
        let four = key(&commit, "test", &args, Some(4));
        let two = key(&commit, "test", &args, Some(2));
        assert_ne!(four, two, "{four} and {two}");
    }

    /// A job that asked for no derivation is a key of its own, distinct from
    /// every `Some` — including `Some(1)`, which is a real derived figure
    /// (D83 never derives zero).
    #[test]
    fn no_derivation_is_a_key_of_its_own() {
        let commit = a_commit();
        let args = ["--only", "posix"];
        let none = key(&commit, "test", &args, None);
        for threads in [1, 2, 4] {
            assert_ne!(
                none,
                key(&commit, "test", &args, Some(threads)),
                "-j {threads}"
            );
        }
    }

    /// Two commits are two keys: the scenario table is compiled into the
    /// commit, so the same arguments can select different scenarios.
    #[test]
    fn two_commits_are_two_keys() {
        let args = ["--only", "posix"];
        assert_ne!(
            key(&"a".repeat(40), "test", &args, Some(4)),
            key(&"b".repeat(40), "test", &args, Some(4))
        );
    }

    /// Two commands are two keys.
    #[test]
    fn two_commands_are_two_keys() {
        let commit = a_commit();
        let args = ["--only", "posix"];
        assert_ne!(
            key(&commit, "test", &args, Some(4)),
            key(&commit, "test-pack", &args, Some(4))
        );
    }

    /// Two argument lists are two keys.
    #[test]
    fn two_argument_lists_are_two_keys() {
        let commit = a_commit();
        assert_ne!(
            key(&commit, "test", &["--only", "posix"], Some(4)),
            key(&commit, "test", &["--only", "posix,desktop"], Some(4))
        );
    }

    /// One set of words split in two places is two keys. A key built by
    /// joining with a separator would make these equal, whatever separator
    /// was chosen — and the cost of that collision is a job placed on another
    /// job's requirements.
    #[test]
    fn one_set_of_words_split_differently_is_two_keys() {
        let commit = a_commit();
        assert_ne!(
            key(&commit, "test", &["a b", "c"], Some(4)),
            key(&commit, "test", &["a", "b c"], Some(4))
        );
    }

    /// The key a declaration gives is the key its three fields give: the hub
    /// reads them off one record and never restates them.
    #[test]
    fn a_declaration_s_key_is_its_three_fields_key() {
        let commit = a_commit();
        let declared = declaration(&commit, "test", &["--only", "posix"]);
        assert_eq!(
            ReadingKey::of(&declared, Some(4)),
            key(&commit, "test", &["--only", "posix"], Some(4))
        );
    }

    // ---- the no-recursion limit -----------------------------------------

    /// A reading job's own labels are its baseline, and are `Known`: the hub
    /// asks every job it places what it needs, a reading job included, and a
    /// reading job never asks for a reading (D74).
    ///
    /// The stronger statement — that a chain of two readings cannot be
    /// *written down* — belongs to the type graph, which has no cycle: a
    /// `ReadingJob` has nowhere to put a `Needs`. What is asserted here is
    /// what a test can establish at run time.
    #[test]
    fn a_reading_job_needs_its_baseline_and_never_another_reading() {
        let baseline = LabelSet::of("build").expect("build is a label");
        let reading = ReadingJob::for_command(
            &declaration(&a_commit(), "test", &["--only", "posix"]),
            Some(4),
            "cargo xtask test --only posix -j 4 --labels",
            baseline.clone(),
        );
        assert_eq!(reading.baseline(), &baseline);
        assert_eq!(reading.needs(), Needs::Known(baseline));
        assert!(
            reading.needs().reading().is_none(),
            "a reading job asked for a reading of its own"
        );
    }

    /// What a reading answers is `Known` too, so the job placed on it is
    /// placed and not read again.
    #[test]
    fn an_answer_needs_the_labels_it_read_and_never_another_reading() {
        let answer = read(FOUR_SCENARIOS);
        assert_eq!(answer.needs(), Needs::Known(answer.labels.clone()));
        assert!(
            answer.needs().reading().is_none(),
            "an answer asked for a second reading"
        );
    }

    /// `Needs` answers one of the two questions and never both, so a caller
    /// cannot place a job it should have read first.
    #[test]
    fn needs_answers_either_the_labels_or_the_reading() {
        let baseline = LabelSet::of("build").expect("build is a label");
        let reading = ReadingJob::for_command(
            &declaration(&a_commit(), "test", &["--only", "posix"]),
            Some(4),
            "cargo xtask test --only posix -j 4 --labels",
            baseline.clone(),
        );
        let to_read = Needs::Read(reading.clone());
        assert_eq!(to_read.reading(), Some(&reading));
        assert_eq!(to_read.known(), None);

        let known = Needs::Known(baseline.clone());
        assert_eq!(known.known(), Some(&baseline));
        assert_eq!(known.reading(), None);
    }

    /// The reading job carries the line it runs and the key its answer is
    /// cached under, and derives the key itself so the two cannot disagree.
    #[test]
    fn a_reading_job_carries_its_line_and_derives_its_own_key() {
        let commit = a_commit();
        let declared = declaration(&commit, "test", &["--only", "posix"]);
        let line = "cargo xtask test --only posix -j 4 --labels";
        let reading = ReadingJob::for_command(
            &declared,
            Some(4),
            line,
            LabelSet::of("build").expect("build is a label"),
        );
        assert_eq!(reading.line(), line);
        assert_eq!(reading.key(), &ReadingKey::of(&declared, Some(4)));
    }
}
