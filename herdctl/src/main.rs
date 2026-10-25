use std::cell::Cell;
use std::io::Write;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use herd_proto::{JobId, JobOutcome, LabelSet, LogStream, Priority};
use herdctl::verdict::{JobExists, Verdict};
use herdctl::{
    could_not_start_message, impossible_exit_code_reason, job_id_line, no_verdict_message,
    no_verdict_reason, setup_failed_message, stopped_by_cap_message, Config, DEFAULT_HUB_URL,
    JOB_UNKNOWN_LINE, NO_JOB_LINE, NO_VERDICT_CODE,
};

/// What this invocation knows about whether a job of its own exists.
///
/// The one question a requester that falls back has to answer, and the
/// reason [`herdctl::JOB_ID_PREFIX`] exists: 125 alone says "no verdict",
/// which is true both of a job that is running somewhere and of a submission
/// that never left, and those two call for opposite actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum WhatExists {
    /// This command submits nothing, so the question does not arise and no
    /// line is printed for it. `cancel`, `raise`, `wait`, `would-run` and
    /// the operator commands: none of them can leave a job behind that was
    /// not already there, and a sentence about submissions on one of them
    /// would be a sentence about the wrong thing.
    #[default]
    NotASubmission,
    /// Nothing was submitted. Running the work again starts no second job.
    Nothing,
    /// The submission left and was never acknowledged: a job may exist, and
    /// E-05 means re-submitting the same work joins it.
    Unknown,
    /// A job exists and its id has been printed.
    AJob,
}

/// It only ever moves forward: once a submission has left, no later failure
/// can make it not have left. The one step back there is does not come
/// through here — see [`refused`], which is narrow on purpose.
fn learn(known: &Cell<WhatExists>, now: WhatExists) {
    known.set(known.get().max(now));
}

/// The hub refused the submission in a frame, which is **proof** that it
/// minted nothing: `Unknown` becomes `Nothing`, and nothing else moves.
///
/// Deliberately not [`learn`]. The ratchet exists because a submission that
/// has left cannot be made not to have left by a later failure, and that is
/// still true of every failure — a dropped socket, a killed process, a frame
/// nobody can read. A refusal is the single case where the hub has *said*
/// what happened, and it is the case that matters most: it is the only one
/// where re-running the work is safe, and `job-unknown` asks a falling-back
/// requester to go looking for a job that does not exist (D139, D142).
///
/// So the step back is written out rather than expressed as an ordering, and
/// it is written to be read: `AJob` stays `AJob` — an id has been printed and
/// no later frame can unprint it — and `NotASubmission` stays itself, because
/// a command that submits nothing has nothing to be refused about. Only the
/// middle state moves, and only in the one direction.
fn refused(known: &Cell<WhatExists>) {
    if known.get() == WhatExists::Unknown {
        known.set(WhatExists::Nothing);
    }
}

/// The level a submission that does not name one is weighed at: the ordinary
/// branch step (E-18).
///
/// There is a default because an agent submitting a build of its branch is
/// submitting a step, and making every caller type it would make the option
/// noise rather than a decision. What there must not be is an *invisible*
/// one, so this one is shown three times over: in `--help` as the default, on
/// stderr at every submission ([`submitting_at`]), and on the job's own
/// record, which carries the level whatever its origin.
const DEFAULT_PRIORITY: &str = "step";

/// The three M9 options every job-making subcommand shares (E-07, E-09,
/// E-10, D76).
///
/// A flattened group rather than three options written out four times: `run`,
/// `submit` and `would-run` have to take exactly the same set, or the dry run
/// would be answering about a job nobody can submit.
#[derive(Debug, Clone, clap::Args)]
struct Asking {
    /// When this job stops being worth starting (E-09).
    ///
    /// An absolute moment in RFC 3339 — `2026-10-14T05:00:00+02:00` — or a
    /// span from now: `+15m`, `+2h`, `+90s`. A job that has not **started**
    /// by then ends `cancelled (deadline)`, with the reason naming what it
    /// was waiting for when the moment came. A job that has started is never
    /// touched by it; the time cap is what bounds a running job.
    ///
    /// It is also the only bound on a job no connected machine could ever fit
    /// (D88), which waits rather than failing, deliberately and possibly for
    /// ever.
    #[arg(long, value_name = "WHEN")]
    deadline: Option<String>,
    /// The one job this one starts after (E-10).
    ///
    /// It starts after that job **succeeds** — exit 0 — and ends `cancelled`
    /// naming the parent if it ends any other way. At most one, with no
    /// conditions: a second edge would be a workflow language and Herd
    /// refuses to be one. The parent must already exist.
    #[arg(long, value_name = "JOB_ID")]
    after: Option<String>,
    /// The exit code this job's command means "the capability I was given
    /// went away under me" by (D76).
    ///
    /// Not a verdict: such a code requeues the job with the capability read
    /// again, a bounded number of times, and then ends it naming the code and
    /// the machines that answered it. Separate from the two goes a job gets
    /// at a runner that disappears.
    #[arg(long, value_name = "CODE")]
    requeue_exit_code: Option<i32>,
}

/// How a submission names a command the **consumer** declared, rather than a
/// command line of its own (E-11, D106).
///
/// A flattened group, like [`Asking`], because `run`, `submit` and
/// `would-run` have to take exactly the same set or a dry run would answer
/// about a job no submission makes (E-07).
///
/// Everything here is read **in the requester's own checkout**: the
/// declaration file, and the commit a ref stands for. The hub and a runner
/// never see TOML and never hold a repository (D106), which is also what
/// makes the two refusals E-08 asks for possible at all — naming the files
/// can only be done where the files are.
#[derive(Debug, Clone, clap::Args)]
struct Declaring {
    /// A command this checkout declares, by name, instead of a command line
    /// after `--` (E-11).
    ///
    /// Its line, its labels, its level and its three figures all come from
    /// `tools/herd/commands.toml` in the checkout, so none of `--label`,
    /// `--memory-mib`, `--build-thread-mib` or `--tmpfs-mib` may be given
    /// with it: the declaration is the authority on what its own command
    /// needs. `--priority` still may — a command that fixes its level
    /// overrules it, and one that does not takes it.
    #[arg(
        long = "command",
        value_name = "NAME",
        conflicts_with_all = ["labels", "build_thread_mib", "memory_mib", "tmpfs_mib"]
    )]
    declared_command: Option<String>,
    /// The commit to run it on: a branch, a tag or a hash this checkout
    /// holds, resolved here to the hash the job records (E-01).
    ///
    /// A dirty tree is refused naming the files that changed, and a commit no
    /// remote-tracking ref holds is refused naming the hash — a runner
    /// fetches the commit from your own origin, so one only this checkout has
    /// is a job that could never start (E-08).
    #[arg(
        long,
        value_name = "REF",
        default_value = "HEAD",
        requires = "declared_command"
    )]
    commit: String,
    /// Which checkout to read the declaration and the commit in. The working
    /// directory by default, which is where a requester already stands.
    #[arg(long, value_name = "PATH", requires = "declared_command")]
    checkout: Option<std::path::PathBuf>,
}

/// What a requester wants written to a disk about a job it followed: the
/// job's own output **files** (D131), and the job's end as JSON (D166).
///
/// A flattened group shared by `run` and `wait`, because the two are the same
/// question asked at two moments: `run` follows a job to its end, `wait`
/// attaches to one by its id — often long after it ran — and both are handed
/// the job's kept files in the frame that carries the verdict. That is exactly
/// why there is no `herdctl outputs <id>` subcommand: `herdctl wait <id>
/// --outputs-to <dir>` already *is* the after-the-fact fetch.
#[derive(Debug, Clone, clap::Args)]
struct Keeping {
    /// Where to write the files the job left under `{out}`, keeping their
    /// relative paths: `files/results.json` lands at
    /// `<DIR>/files/results.json`.
    ///
    /// Left out, nothing is written and the job's files stay on the hub, in
    /// its record. They are **never** printed: stdout is the `ran on:` line,
    /// the job's own bytes and its verdict, and nothing else (E-02). What
    /// this option writes is reported on stderr, one line per file.
    ///
    /// A job whose output was over the runner's cap writes no file at all and
    /// says so on stderr with both figures — never a truncated file, which
    /// would read like a whole one.
    #[arg(long = "outputs-to", value_name = "DIR")]
    outputs_to: Option<std::path::PathBuf>,
    /// Where to write this job's verdict as JSON, for a requester that is a
    /// program (D166).
    ///
    /// The document names the cause in one word, says whether the exit code
    /// is the job's own or Herd's, names the runner the job ran on, carries a
    /// cap's figures when a cap ended the job, and says whether a job of
    /// yours exists at all — which is the question 125 cannot answer. It
    /// carries its own format version as its first field.
    ///
    /// **A file and never a stream.** Stdout is the job's own output, framed
    /// and nothing else (E-02), and stderr carries Herd's prose, which the
    /// same wrapper is already reading a line at a time; a JSON document
    /// spliced into either is one nobody can parse. A caller that wants it on
    /// a pipe names one: `--verdict-to /dev/fd/3`.
    ///
    /// It is written however the run ends, including when there is no verdict
    /// at all — a wrapper that found yesterday's file would act on
    /// yesterday's answer.
    #[arg(long = "verdict-to", value_name = "FILE")]
    verdict_to: Option<std::path::PathBuf>,
}
/// What `herdctl --help` says about exit codes, and about the one thing a
/// code cannot say.
///
/// Written out in `--help` rather than left to a document, because the
/// person who needs it is writing a wrapper around this binary and `--help`
/// is where they are already looking. `deploy/release-notice.md` carries the
/// same table for whoever is reading that instead.
const EXIT_CODES: &str = "\
Exit codes:
  the job's own  worn untouched — 0, 1, 75, whatever the process exited with
  124            a runner stopped the job at a cap: time, memory, log or disk
  125            Herd has no verdict: it could not run the job, lost it,
                 refused it, could not be reached, or the machine it ran on
                 could not do its own part. Never a failing build
  128 + n        the job was killed by signal n, as a shell reports one
  2              a command did part of what it says and named the rest —
                 including `--outputs-to` on a job that passed whose output
                 files did not all arrive. The verdict still stands and is
                 printed; every missing path is named on stderr

For a requester that is a program: --verdict-to
  `herdctl run --verdict-to <FILE>` and `herdctl wait --verdict-to <FILE>`
  write the job's end as JSON, so nothing below has to be parsed out of a
  sentence. The fields that answer what a code cannot:

    cause            one word: exited, signalled, stopped_by_cap,
                     could_not_start, setup_failed, cancelled, lost,
                     impossible — or no_verdict when Herd never got one
    exit_code_from   job, signal or herd. **This is what tells a job's own
                     exit of 124 or 125 from Herd's**, which the code alone
                     cannot: a build that exits 124 reads
                     {\"exit_code\": 124, \"exit_code_from\": \"job\"}, and a
                     cap reads {\"exit_code\": 124, \"exit_code_from\": \"herd\"}
    job_exists       a_job, nothing or unknown — the three stderr lines below,
                     as a field
    ran_on           the runner the job ran on, and why there
    cap              which cap and its figures, when a cap ended the job

  Every key is always present; a field with nothing in it is null. The
  document carries its own `verdict_format_version` as its first field. The
  file is written however the run ends, no verdict included, so a stale one
  is never read as this run's answer. It is a file and not a stream because
  stdout belongs to the job (E-02) and stderr carries the prose below.

Falling back after 125:
  125 is one code for two opposite situations — a job that exists and may
  still be running, and a submission that never left. Re-running the first
  does the work twice; declining to re-run the second does it never. The
  code cannot tell them apart, so stderr does, in one documented line:

    herdctl: job-id: <id>      a job exists. Do not submit again:
                               `herdctl wait <id>` picks it back up
    herdctl: no-job: ...       nothing was submitted. Running again is safe
    herdctl: job-unknown: ...  the submission left unacknowledged. A job may
                               exist; submitting the same work again joins
                               it rather than starting a second one (E-05)

  The rule is the presence of an id, never the wording of a sentence. The
  line is on stderr, which a job's own output never shares, and appears as
  soon as the hub says the id — so it survives a wrapper that gives up and
  kills herdctl. Seeing none of the three means herdctl died before it could
  say; treat that as job-unknown.

  A wrapper reads it like this:

    out=$(herdctl run --command build --commit \"$sha\" 2>&1 >/dev/null) || {
      id=$(printf '%s\\n' \"$out\" | sed -n 's/^herdctl: job-id: //p' | tail -1)
      if [ -n \"$id\" ]; then herdctl wait \"$id\"; else run_it_locally; fi
    }
";

/// The requester CLI: submit a job, follow its log, exit with its code.
#[derive(Debug, Parser)]
#[command(name = "herdctl", version, about, after_long_help = EXIT_CODES)]
struct Args {
    /// The hub to submit to.
    #[arg(long, env = "HERD_HUB_URL", default_value = DEFAULT_HUB_URL, global = true)]
    hub: String,
    /// Bearer token. No secret belongs in the repo (invariant 7).
    #[arg(long, env = "HERD_TOKEN", hide_env_values = true, global = true)]
    token: Option<String>,
    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Run a command on a runner the hub chooses, and follow its output.
    ///
    /// A requester never names a machine (invariant 6).
    Run {
        /// A capability this job needs, by name (E-25). Repeat the option, or
        /// separate names with `,`. Never a machine: the hub chooses
        /// (invariant 6).
        #[arg(long = "label", value_delimiter = ',', value_name = "NAME")]
        labels: Vec<String>,
        /// Which of the four levels this job is weighed at (E-18):
        /// merger, step, nightly or producer, highest first.
        #[arg(long, default_value = DEFAULT_PRIORITY, value_name = "LEVEL")]
        priority: Priority,
        /// How much memory one build thread of this job needs, in MiB.
        ///
        /// Given, the hub derives this job's `-j` from the machine it places
        /// it on and puts the figure where the command's `{j}` is (E-22).
        /// The command must then carry exactly one `{j}`, or the job is
        /// refused by name rather than run with a figure nothing was told.
        #[arg(long, value_name = "MIB")]
        build_thread_mib: Option<u32>,
        /// How much memory this whole job needs, in MiB (E-29).
        ///
        /// What admission weighs it at on whatever machine it goes to: a
        /// runner takes it only if its memory, less the floor it keeps for
        /// itself, less what the jobs already out there declared, covers
        /// this. Not given together with `--build-thread-mib`, which is the
        /// per-thread figure a build declares instead.
        #[arg(long, value_name = "MIB")]
        memory_mib: Option<u64>,
        /// How much room on the runner's tmpfs this job reserves while it
        /// runs, in MiB (E-31).
        ///
        /// Reserved before it starts and released when it ends, however it
        /// ends. A job that cannot be given the room **waits**; one asking
        /// for more than a machine has in all is never sent there.
        #[arg(long, value_name = "MIB")]
        tmpfs_mib: Option<u64>,
        #[command(flatten)]
        asking: Asking,
        #[command(flatten)]
        declaring: Declaring,
        #[command(flatten)]
        keeping: Keeping,
        /// The command, after `--` — or, with `--command`, the arguments
        /// that declared line's `{args}` stands for.
        #[arg(trailing_var_arg = true, num_args = 0..)]
        command: Vec<String>,
    },
    /// Submit a command and print its job id, without waiting for it.
    ///
    /// The job runs and is recorded whether anyone is following it or not, so
    /// `herdctl wait <id>` works any time afterwards.
    Submit {
        /// A capability this job needs, by name (E-25). Repeat the option, or
        /// separate names with `,`.
        #[arg(long = "label", value_delimiter = ',', value_name = "NAME")]
        labels: Vec<String>,
        /// Which of the four levels this job is weighed at (E-18):
        /// merger, step, nightly or producer, highest first.
        #[arg(long, default_value = DEFAULT_PRIORITY, value_name = "LEVEL")]
        priority: Priority,
        /// How much memory one build thread of this job needs, in MiB (E-22).
        /// See `herdctl run --help`.
        #[arg(long, value_name = "MIB")]
        build_thread_mib: Option<u32>,
        /// How much memory this whole job needs, in MiB (E-29).
        ///
        /// What admission weighs it at on whatever machine it goes to: a
        /// runner takes it only if its memory, less the floor it keeps for
        /// itself, less what the jobs already out there declared, covers
        /// this. Not given together with `--build-thread-mib`, which is the
        /// per-thread figure a build declares instead.
        #[arg(long, value_name = "MIB")]
        memory_mib: Option<u64>,
        /// How much room on the runner's tmpfs this job reserves while it
        /// runs, in MiB (E-31).
        ///
        /// Reserved before it starts and released when it ends, however it
        /// ends. A job that cannot be given the room **waits**; one asking
        /// for more than a machine has in all is never sent there.
        #[arg(long, value_name = "MIB")]
        tmpfs_mib: Option<u64>,
        #[command(flatten)]
        asking: Asking,
        #[command(flatten)]
        declaring: Declaring,
        /// The command, after `--` — or, with `--command`, the arguments
        /// that declared line's `{args}` stands for.
        #[arg(trailing_var_arg = true, num_args = 0..)]
        command: Vec<String>,
    },
    /// Say where a job **would** run, and run nothing (E-07).
    ///
    /// The eligible runners with the labels each of them answers for, what is
    /// queued ahead of it, and the verdict the claim step would give — judged
    /// through exactly the same code a submission goes through, against the
    /// fleet and the queue as they are this second. Nothing is minted, nothing
    /// is written, no slot and no reservation is taken.
    #[command(name = "would-run", alias = "dry-run")]
    WouldRun {
        /// A capability this job would need, by name (E-25).
        #[arg(long = "label", value_delimiter = ',', value_name = "NAME")]
        labels: Vec<String>,
        /// Which of the four levels it would be weighed at (E-18).
        #[arg(long, default_value = DEFAULT_PRIORITY, value_name = "LEVEL")]
        priority: Priority,
        /// How much memory one build thread would need, in MiB (E-22).
        #[arg(long, value_name = "MIB")]
        build_thread_mib: Option<u32>,
        /// How much memory the whole job would need, in MiB (E-29).
        #[arg(long, value_name = "MIB")]
        memory_mib: Option<u64>,
        /// How much tmpfs it would reserve, in MiB (E-31).
        #[arg(long, value_name = "MIB")]
        tmpfs_mib: Option<u64>,
        #[command(flatten)]
        asking: Asking,
        #[command(flatten)]
        declaring: Declaring,
        /// The command, after `--` — or, with `--command`, the arguments
        /// that declared line's `{args}` stands for.
        #[arg(trailing_var_arg = true, num_args = 0..)]
        command: Vec<String>,
    },
    /// Read or move a runner's owner switch (E-33).
    ///
    /// With no arguments: where every runner's switch stands, with the reason
    /// its owner gave. With a runner and a position: move it, honoured within
    /// ten seconds.
    ///
    /// This is the orchestrator's half of E-33. The owner's own half is
    /// `herd-runner switch <position> --reason ...` on the machine itself,
    /// which needs no hub and works when the hub is down.
    Switch {
        /// The runner to move. Left out, every runner's switch is printed.
        runner: Option<String>,
        /// One of `on`, `off`, `drain`.
        position: Option<String>,
        /// Why. Required for `off` and `drain`; for `on`, accepted and not
        /// kept, because that position needs none (D166).
        #[arg(long)]
        reason: Option<String>,
    },
    /// Approve, remove or list the runners this hub will serve (E-48).
    ///
    /// An unapproved runner claims nothing: it is refused by name, with a
    /// sentence telling whoever reads its stderr to run `herdctl runner
    /// approve <name>`. Approving mints that runner's token and prints it
    /// once — the hub keeps only a hash of it (invariant 7).
    Runner {
        #[command(subcommand)]
        action: RunnerAction,
    },
    /// Wait for a job that already exists, and exit with its code.
    Wait {
        /// The id a submission printed.
        job_id: String,
        #[command(flatten)]
        keeping: Keeping,
    },
    /// Ask for a job to be cancelled, wherever it is running (E-06).
    ///
    /// The hub reaches the runner executing it; a requester never names a
    /// machine (invariant 6). Prints what became of the job and exits 0 — the
    /// job's own exit code belongs to a job that ran to its end, and a
    /// cancelled one has none.
    Cancel {
        /// The id a submission printed.
        job_id: String,
    },
    /// Raise a queued job's priority above what its requester asked for
    /// (E-18).
    ///
    /// For an orchestrator, not for the requester that submitted the job.
    /// Upwards only: lowering a job is how something gets starved on purpose,
    /// and the queue's own aging is what moves a level the other way. Prints
    /// what the hub did and exits 0.
    Raise {
        /// The id a submission printed.
        job_id: String,
        /// The level to raise it to: merger, step, nightly or producer,
        /// highest first.
        #[arg(long, value_name = "LEVEL")]
        priority: Priority,
    },
}

/// What an operator does about which runners this hub serves (E-48, Q8).
///
/// Three verbs and no more: approve a machine, remove it, see who is
/// approved. There is no "rotate" — approving again mints a new token and
/// revokes the old — and no "show me the token", because the hub keeps a hash
/// and has nothing to show.
#[derive(Debug, Subcommand)]
enum RunnerAction {
    /// Approve one runner and print the token it is to carry, once.
    ///
    /// The token goes to **stdout**, alone, so that `token=$(herdctl runner
    /// approve wsl-pc)` is all a deployment script needs; the sentence
    /// explaining it goes to stderr. Give it to that machine as
    /// `herd-runner --token <token>` or `HERD_TOKEN`.
    ///
    /// Approving a runner that is already approved mints a new token and
    /// revokes the old one, which is how a token that has leaked is replaced.
    Approve {
        /// The name that runner announces — `herd-runner --name`.
        name: String,
    },
    /// Remove one runner, revoking its token (E-48).
    ///
    /// If it is connected it is told, on the link it is holding, and claims
    /// nothing from that moment. A job it is already executing is left to
    /// finish or to be lost the ordinary way: a removal is not a way to kill
    /// a build.
    Remove {
        /// The name that runner announces.
        name: String,
    },
    /// Say which runners are approved, and which of them are connected.
    List,
}

/// What a subcommand leaves the process to exit with.
enum Done {
    /// A job ended, and its end decides the exit code (D16). The whole of
    /// what came back, not only the ending, because the machine-readable
    /// verdict names the runner and carries what it measured (D166).
    ///
    /// Boxed because it is much the largest of the three and every other
    /// arm would otherwise carry its weight.
    Job {
        finished: Box<herdctl::Finished>,
        /// The sentence naming the output files `--outputs-to` was promised
        /// and did not get, or `None` when it got all of them or was never
        /// given (D167).
        ///
        /// Carried out of [`keep_the_outputs`] rather than printed there,
        /// because it belongs in the verdict document beside the others: a
        /// wrapper reading that file is the reader this fact is for, and it
        /// is also what makes this invocation exit non-zero.
        short_of_its_files: Option<String>,
        /// What the hub said about this job's collection that is not the
        /// shortfall: the refusal, and every `skipped` sentence (D-NEXT).
        ///
        /// Already printed by [`keep_the_outputs`], where a reader needs
        /// them in order. Carried out as well so they reach the verdict
        /// document: a wrapper that is told "a file was dropped and here is
        /// why" had, until this, only prose on stderr to find it in, which
        /// is the parsing D166 exists to make unnecessary. They do **not**
        /// move the exit code — D131's sentences are facts about one file
        /// and never a verdict about the job.
        about_the_outputs: Vec<String>,
    },
    /// The command did its own job; nothing ran.
    Said,
    /// The command did **part** of what it says, and said which part on
    /// stderr (D99).
    ///
    /// One case today: `herdctl switch <runner> <position>` where the runner
    /// has not confirmed writing the position down. The position may be in
    /// force and not durable, and an owner who read a zero exit code there
    /// would believe her machine stays hers past a reboot.
    ///
    /// [`NOT_SETTLED_CODE`] rather than [`herdctl::NO_VERDICT_CODE`]: 125 says
    /// "Herd has no verdict for this *job*", and this command runs no job. It
    /// is a distinct code because a script that switches a fleet off before a
    /// maintenance window has to be able to tell "the hub could not be
    /// reached" from "the machine is off but may forget it".
    SaidOnlyHalf,
}

/// `herdctl`'s exit code for a command that did part of what it says and
/// named the rest on stderr (D99).
///
/// Not 1: 1 means "the job failed" (D20), and nothing here ran a job. Not 125:
/// that is a job with no verdict. 2 is free, and the sentence on stderr is
/// what a person reads either way.
const NOT_SETTLED_CODE: u8 = 2;

/// What a run exits with, given how the job ended and whether the files its
/// requester asked for all arrived (D167).
///
/// **The job's verdict always wins when it has one to give.** A failing build
/// whose screenshots are short exits 1, because the build failing is the more
/// important fact and the missing files are named on stderr beside it; a job
/// stopped by a cap still exits 124, a setup failure still exits what D166
/// gave it, and one with no verdict still exits 125. The one code this
/// replaces is **0** — and that is the whole of the change, because 0 is the
/// only code that says "everything you asked for happened". A requester that
/// passed `--outputs-to`, got a short directory and read a zero would act on
/// an incomplete set, which is what the first consumer was working around at
/// a cost per build.
///
/// It does not reopen D20. D20 is about a side channel never becoming a
/// *verdict about the job*: the verdict is printed, is in the document, and
/// is not altered here. 2 is `herdctl` speaking about itself — "a command did
/// part of what it says and named the rest" — which is exactly what a fetch
/// that wrote eight files of twelve did. The document says so in as many
/// words, because `exit_code_from` then reads `herd` rather than `job`
/// (D166's own field, doing the work it was added for).
fn code_for(outcome: &JobOutcome, files_short: bool) -> u8 {
    match herdctl::exit_code_for(outcome) {
        0 if files_short => NOT_SETTLED_CODE,
        code => code,
    }
}

/// Where a verdict document goes, and what this invocation already knew
/// about the job it is about (D166).
///
/// Taken off the command **before** it is consumed, because the document has
/// to be written on the paths that have no job to carry it: a hub that could
/// not be reached, a submission the hub refused, a link that went while the
/// job was still running. A wrapper that read this file and found the
/// previous run's would act on the previous run's answer, which is worse than
/// anything it could be reporting.
struct Reporting {
    to: Option<std::path::PathBuf>,
    /// The id a `wait` was handed. `run` learns its own from the hub.
    job_id: Option<String>,
}

impl Reporting {
    fn of(action: &Action) -> Self {
        match action {
            Action::Run { keeping, .. } => Self {
                to: keeping.verdict_to.clone(),
                job_id: None,
            },
            Action::Wait { job_id, keeping } => Self {
                to: keeping.verdict_to.clone(),
                job_id: Some(job_id.clone()),
            },
            // Nothing else takes the option: a command that runs no job has
            // no verdict to report, and `submit` is not waiting for one.
            _ => Self {
                to: None,
                job_id: None,
            },
        }
    }

    /// Whether a job of this requester's exists, as the document says it.
    fn job_exists(&self, known: WhatExists) -> JobExists {
        match (&self.job_id, known) {
            // A `wait` was handed an id, so a job exists whatever this
            // invocation then managed to do about it.
            (Some(_), _) => JobExists::AJob,
            (None, WhatExists::AJob) => JobExists::AJob,
            (None, WhatExists::Unknown) => JobExists::Unknown,
            // `NotASubmission` cannot arrive here — only `run` and `wait`
            // take the option, and `run` learns `Nothing` before it submits
            // anything — and it would mean the same thing if it did: this
            // invocation left no job behind.
            (None, WhatExists::Nothing | WhatExists::NotASubmission) => JobExists::Nothing,
        }
    }

    /// Write it, and when it cannot be written say so and nothing more.
    ///
    /// **It never touches the exit code.** The verdict is the answer the
    /// caller is waiting for and it has already been established; a file the
    /// requester named and cannot write is a problem with that path, exactly
    /// as it is for `--outputs-to`. So the sentence is loud, and the file's
    /// absence is the other half of it.
    fn write(&self, verdict: &Verdict) {
        let Some(path) = &self.to else {
            return;
        };
        if let Err(error) = verdict.write_to(path) {
            eprintln!(
                "herdctl: this job's verdict could not be written to {}: {error:#}",
                path.display()
            );
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let reporting = Reporting::of(&args.command);
    let known = Cell::new(WhatExists::default());
    match run(args, &known).await {
        Ok(Done::Job {
            finished,
            short_of_its_files,
            about_the_outputs,
        }) => {
            // Each sentence goes to stderr, never to stdout: stdout carried
            // the job's own output and must stay the job's alone. They are
            // **kept** as well as printed, so that a wrapper reading the
            // verdict document logs the same words a person would have read
            // (D166).
            let mut said = Vec::new();
            match &finished.outcome {
                JobOutcome::CouldNotStart { reason } => {
                    said.push(could_not_start_message(reason));
                }
                JobOutcome::SetupFailed { reason } => said.push(setup_failed_message(reason)),
                JobOutcome::StoppedByCap { cap, limit, detail } => {
                    said.push(stopped_by_cap_message(*cap, *limit, detail.as_deref()));
                }
                JobOutcome::Exited { .. }
                | JobOutcome::Signalled { .. }
                // Said below, by no_verdict_reason, with the other ends that
                // carry no verdict.
                | JobOutcome::Cancelled { .. }
                | JobOutcome::Impossible { .. }
                | JobOutcome::Lost { .. } => {}
            }
            // An exit code outside 0..=255 is one no process can have worn,
            // so there is no verdict to report and the reason is said as
            // Herd's own failure, never as the job's (D20).
            if let Some(reason) = impossible_exit_code_reason(&finished.outcome) {
                said.push(no_verdict_message(&reason));
            }
            // Cancelled, or lost with its runner: a real end, and not one that
            // carries a verdict. Said in words, because 125 cannot say which
            // of them it was (D20).
            if let Some(reason) = no_verdict_reason(&finished.outcome) {
                said.push(no_verdict_message(&reason));
            }
            // The shortfall last, after the per-file lines `keep_the_outputs`
            // has already written, and in `said` rather than straight to
            // stderr so that the document carries it too (D167).
            let files_short = short_of_its_files.is_some();
            if let Some(sentence) = short_of_its_files {
                said.push(sentence);
            }
            for sentence in &said {
                eprintln!("herdctl: {sentence}");
            }
            // **After the printing loop, because they have already been
            // said** — `keep_the_outputs` prints them where a reader needs
            // them, ahead of the per-file lines. What is added here is the
            // document's copy and nothing else (D-NEXT).
            said.extend(about_the_outputs);
            let code = code_for(&finished.outcome, files_short);
            let verdict = Verdict::of(&finished, said, code);
            // And whose number it is. A 2 is this command's own, never the
            // job's, so the document must not label it `job` — which is the
            // distinction D166 added the field to be able to draw.
            reporting.write(&if files_short {
                verdict.with_herd_s_own_code()
            } else {
                verdict
            });
            ExitCode::from(code)
        }
        Ok(Done::Said) => ExitCode::SUCCESS,
        // The sentence was printed where the command prints: what this arm
        // decides is only that it is not success (D99).
        Ok(Done::SaidOnlyHalf) => ExitCode::from(NOT_SETTLED_CODE),
        // Nothing ran, or nothing came back: Herd has no verdict to give, and
        // saying so with 1 would be indistinguishable from a job whose tests
        // failed (D20). One code, one meaning, and the reason on stderr.
        Err(error) => {
            let said = no_verdict_message(&format!("{error:#}"));
            eprintln!("herdctl: {said}");
            // And then the one thing a falling-back requester has to know,
            // which the code cannot say. A job that exists has already named
            // itself on stderr, the moment the hub said its id; the other
            // two states are said here, where they become final.
            match known.get() {
                WhatExists::NotASubmission | WhatExists::AJob => {}
                WhatExists::Unknown => eprintln!("{JOB_UNKNOWN_LINE}"),
                WhatExists::Nothing => eprintln!("{NO_JOB_LINE}"),
            }
            // The same three states, in a field rather than in a sentence:
            // this is the half of D109 a program was being asked to `sed`.
            reporting.write(&Verdict::none(
                reporting.job_id.clone(),
                reporting.job_exists(known.get()),
                said,
            ));
            ExitCode::from(NO_VERDICT_CODE)
        }
    }
}

async fn run(args: Args, known: &Cell<WhatExists>) -> Result<Done> {
    let config = Config {
        hub_url: args.hub,
        token: args.token,
        name: None,
    };
    match args.command {
        Action::Run {
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            asking,
            declaring,
            keeping,
            command,
        } => {
            // This command can leave a job behind, so the question "does one
            // exist?" is live from here on, and its answer is said on stderr
            // whichever way this ends (`JOB_ID_PREFIX`).
            learn(known, WhatExists::Nothing);
            let job = submitting(
                declaring,
                labels,
                priority,
                build_thread_mib,
                memory_mib,
                tmpfs_mib,
                asking,
                command,
            )?;
            // Said out loud, on stderr, before anything runs: a level is
            // what decides whether this job goes before or after somebody
            // else's, and a default nobody can see is the defect E-18 is
            // most easily broken by. stdout stays the job's own. A declared
            // command may have fixed the level, so this says the level the
            // job really carries and not the one that was asked for.
            eprintln!("herdctl: {}", submitting_at(job.priority));
            // Each stream is written to its own, so a caller's pipes and
            // redirections behave as if the command had run here.
            let finished = herdctl::run_job_wanting(
                // Whether the hub holds this job's verdict until its output
                // files have all reached it (D167). Said here, where the flag
                // that decides it lives, so that a `herdctl run` which asked
                // for no files still exits on the verdict and waits for
                // nothing — D148's promise, kept.
                wanting(&keeping),
                &config,
                job.command,
                job.labels,
                job.priority,
                job.declared,
                |stream, data| match stream {
                    LogStream::Stdout => write_now(&mut std::io::stdout(), data),
                    LogStream::Stderr => write_now(&mut std::io::stderr(), data),
                },
                // Herd's own words go to stderr: stdout stays the job's
                // alone. A queued job's reason arrives here (E-26). The one
                // exception is E-02's first line, which *is* part of the
                // output and goes on stdout ahead of it (D109).
                |said| say_it(known, said),
            )
            .await?;
            let kept = keep_the_outputs(&config, &keeping, &finished).await;
            Ok(Done::Job {
                finished: Box::new(finished),
                short_of_its_files: kept.short_of_its_files,
                about_the_outputs: kept.about_the_outputs,
            })
        }
        Action::Submit {
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            asking,
            declaring,
            command,
        } => {
            // This command can leave a job behind, so the question "does one
            // exist?" is live from here on, and its answer is said on stderr
            // whichever way this ends (`JOB_ID_PREFIX`).
            learn(known, WhatExists::Nothing);
            let job = submitting(
                declaring,
                labels,
                priority,
                build_thread_mib,
                memory_mib,
                tmpfs_mib,
                asking,
                command,
            )?;
            eprintln!("herdctl: {}", submitting_at(job.priority));
            let job_id = herdctl::submit_job_saying(
                &config,
                job.command,
                job.labels,
                job.priority,
                job.declared,
                |said| say_it(known, said),
            )
            .await?;
            // The id alone on stdout, so that `id=$(herdctl submit -- ...)`
            // is all a hook needs (E-42).
            println!("{job_id}");
            Ok(Done::Said)
        }
        Action::WouldRun {
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            asking,
            declaring,
            command,
        } => {
            // A dry run submits nothing by construction (E-07), so it stays
            // `NotASubmission` and says nothing about jobs that exist.
            let job = submitting(
                declaring,
                labels,
                priority,
                build_thread_mib,
                memory_mib,
                tmpfs_mib,
                asking,
                command,
            )?;
            let asked = job.declared.asked(job.command, job.labels, job.priority);
            // On stdout, because this *is* the answer the caller asked for —
            // unlike `run`, where stdout belongs to the job.
            for line in herdctl::would_run(&config, asked).await? {
                println!("{line}");
            }
            Ok(Done::Said)
        }
        Action::Switch {
            runner,
            position,
            reason,
        } => {
            match (runner, position) {
                (None, _) => {
                    let runners = herdctl::fleet_switches(&config).await?;
                    if runners.is_empty() {
                        println!("no runner is connected to this hub");
                    }
                    for runner in &runners {
                        for line in herdctl::switch_lines(runner) {
                            println!("{line}");
                        }
                    }
                }
                (Some(runner), None) => {
                    anyhow::bail!(
                        "`herdctl switch {runner}` does not say what to set it to: give a \
                         position (on, off, drain), or leave the runner out to read every \
                         switch"
                    );
                }
                (Some(runner), Some(position)) => {
                    let switch = herd_proto::Switch::parse(&position, reason.as_deref())
                        .map_err(anyhow::Error::msg)?;
                    // `on` takes no reason and no longer refuses one (D166).
                    // Said here, where the person typed it, rather than
                    // dropped in silence: a reason that goes nowhere is at
                    // least a reason somebody read.
                    if let (herd_proto::Switch::On, Some(given)) = (
                        &switch,
                        reason
                            .as_deref()
                            .map(str::trim)
                            .filter(|given| !given.is_empty()),
                    ) {
                        eprintln!(
                            "herdctl: switching {runner} on; the reason given is not kept, \
                             because the position that carries one is the withdrawal: {given}"
                        );
                    }
                    // This waits for the runner to have the position on its
                    // own disk, bounded by E-33's ten seconds (D99). Anything
                    // short of that is said on stderr and is not success: the
                    // owner asked for a machine she can take back, and a
                    // position that dies with the runner process is half of
                    // that.
                    let (said, confirmed) = herdctl::set_switch(&config, runner, switch).await?;
                    if !confirmed {
                        eprintln!("herdctl: {said}");
                        return Ok(Done::SaidOnlyHalf);
                    }
                    println!("{said}");
                }
            }
            Ok(Done::Said)
        }
        Action::Runner { action } => {
            match action {
                RunnerAction::Approve { name } => {
                    let (token, said) = herdctl::approve_runner(&config, name).await?;
                    // The sentence on stderr and the token alone on stdout,
                    // so that a deployment script can take the token and a
                    // person reading a terminal still gets told what it is
                    // and that it is shown once.
                    eprintln!("herdctl: {said}");
                    println!("{token}");
                }
                RunnerAction::Remove { name } => {
                    println!("{}", herdctl::remove_runner(&config, name).await?);
                }
                RunnerAction::List => {
                    let runners = herdctl::approved_runners(&config).await?;
                    if runners.is_empty() {
                        println!(
                            "no runner is approved on this hub, so no runner can claim a job: \
                             `herdctl runner approve <name>` mints one a token"
                        );
                    }
                    for runner in &runners {
                        for line in herdctl::approved_runner_lines(runner) {
                            println!("{line}");
                        }
                    }
                }
            }
            Ok(Done::Said)
        }
        Action::Wait { job_id, keeping } => {
            // A wait prints the job's output exactly as `run` does: the log
            // the hub had recorded before this wait attached, then whatever
            // arrives afterwards. Without it a detached requester could only
            // read a job's output by reading the hub's state directory, which
            // a requester on another machine cannot do (D22).
            let finished = herdctl::wait_for_job_wanting(
                wanting(&keeping),
                &config,
                JobId::new(job_id),
                |stream, data| match stream {
                    LogStream::Stdout => write_now(&mut std::io::stdout(), data),
                    LogStream::Stderr => write_now(&mut std::io::stderr(), data),
                },
                |said| say_it(known, said),
            )
            .await?;
            let kept = keep_the_outputs(&config, &keeping, &finished).await;
            Ok(Done::Job {
                finished: Box::new(finished),
                short_of_its_files: kept.short_of_its_files,
                about_the_outputs: kept.about_the_outputs,
            })
        }
        Action::Cancel { job_id } => {
            let note = herdctl::cancel_job(&config, JobId::new(job_id)).await?;
            println!("{note}");
            Ok(Done::Said)
        }
        Action::Raise { job_id, priority } => {
            let note = herdctl::raise_job(&config, JobId::new(job_id), priority).await?;
            println!("{note}");
            Ok(Done::Said)
        }
    }
}

/// Turn `--label` into a canonical label set, refusing a name that is not a
/// label.
///
/// Refused here, before anything is submitted: a typo that reached the hub
/// would become a job waiting for a capability no machine will ever offer,
/// and the caller would read a wait where they should read a sentence.
/// What every submission says about its own level, on stderr.
///
/// One sentence, with the word `default` in it when the level was not asked
/// for, because "this job is a step" and "nobody said, so it is a step" are
/// different facts and only the second one is worth looking at twice.
fn submitting_at(priority: Priority) -> String {
    if priority.name() == DEFAULT_PRIORITY {
        format!(
            "submitting at priority {priority} (the default; --priority takes one of {})",
            Priority::names()
        )
    } else {
        format!("submitting at priority {priority}")
    }
}

/// One submission, however it was named: a command line typed after `--`, or
/// a command the consumer's own checkout declares.
struct Submitting {
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: herdctl::Declared,
}

/// Turn what a person typed into the one submission it means.
///
/// One function for `run`, `submit` **and** `would-run`, which is what makes
/// a dry run answer about the job a submission would really make (E-07) — and
/// what makes a dry run of a *declared* command refuse the same dirty tree a
/// submission would.
///
/// The two shapes it takes, and nothing in between:
///
/// * a **command line** after `--`, which declares nothing beyond the
///   figures the person typed and is capped by the runner's own figure;
/// * `--command <name>`, which is read in the requester's own checkout
///   (D106): the ref resolves to the hash the job will record, a dirty tree
///   and an unpushed commit are refused naming what is wrong (E-08), and the
///   declaration supplies the line, the labels, the level and the figures.
///   The trailing arguments are that line's `{args}`.
#[allow(clippy::too_many_arguments)]
fn submitting(
    declaring: Declaring,
    labels: Vec<String>,
    priority: Priority,
    build_thread_mib: Option<u32>,
    memory_mib: Option<u64>,
    tmpfs_mib: Option<u64>,
    asking: Asking,
    command: Vec<String>,
) -> Result<Submitting> {
    let requester = declared(build_thread_mib, memory_mib, tmpfs_mib, asking)?;
    let Some(named) = declaring.declared_command.clone() else {
        // A command line of the requester's own. Refused here rather than by
        // clap, because `--command` makes the trailing arguments optional and
        // a person who gave neither needs to be told which of the two to give.
        if command.is_empty() {
            bail!(
                "nothing to run: give a command line after `--`, or name one this checkout \
                 declares with `--command <name>`"
            );
        }
        return Ok(Submitting {
            command,
            labels: labels_of(labels)?,
            priority,
            declared: requester,
        });
    };

    // The checkout the requester is standing in, unless it named another.
    let from = match declaring.checkout {
        Some(path) => path,
        None => {
            std::env::current_dir().context("finding the checkout to read the declaration in")?
        }
    };
    let checkout = herdctl::checkout::Checkout::found_from(&from)?;
    // E-01 and both of E-08's refusals, in that order: a person who mistyped
    // a branch is told about the branch and not about their tree.
    let commit = checkout
        .resolve(&declaring.commit)
        .with_context(|| format!("--commit {}", declaring.commit))?;
    let commands = herdctl::declaration::Commands::read_in(checkout.root())?;
    let resolved = commands
        .resolve(&named, &commit.hash, &command)
        .with_context(|| format!("--command {named}"))?;
    // Said out loud, because a declared submission is a job whose command
    // line the person did not type: what resolved, and to which commit.
    eprintln!(
        "herdctl: {named} of {} ({}), as {} declares it",
        commit.hash,
        commit.named,
        commands.path().display()
    );
    Ok(Submitting {
        command: resolved.command(),
        labels: resolved.labels.at_submission().clone(),
        priority: resolved.priority(priority),
        // The declaration's figures, with the three fields that are the
        // requester's own — the deadline, the parent, the requeue code —
        // kept from what the person typed.
        declared: herdctl::Declared {
            after: requester.after,
            deadline_unix_s: requester.deadline_unix_s,
            requeue_exit_code: requester.requeue_exit_code,
            ..resolved.declared()
        },
    })
}

/// Everything a requester declares about a job, from the options it typed.
///
/// A deadline is resolved here, against this machine's clock, so what travels
/// is an instant (E-09).
fn declared(
    build_thread_mib: Option<u32>,
    memory_mib: Option<u64>,
    tmpfs_mib: Option<u64>,
    asking: Asking,
) -> Result<herdctl::Declared> {
    let deadline_unix_s = match &asking.deadline {
        Some(said) => Some(herdctl::deadline_at(said).context("--deadline")?),
        None => None,
    };
    Ok(herdctl::Declared {
        build_thread_mib,
        memory_mib,
        tmpfs_mib,
        deadline_unix_s,
        after: asking.after,
        requeue_exit_code: asking.requeue_exit_code,
        // An ad-hoc command line declares nothing and is capped by the
        // runner's own figure: a declared command comes in by its name and
        // its commit, which is M10's own surface (E-11, D106).
        declaration: None,
        // And a command line a person typed states its own labels: there is
        // no declaration to read them out of, so there is no reading (D74).
        labels_from: None,
        timeout_s: None,
    })
}

fn labels_of(labels: Vec<String>) -> Result<LabelSet> {
    LabelSet::new(labels)
        .map_err(anyhow::Error::msg)
        .context("--label")
}

/// Where each thing Herd says about a job goes.
///
/// One function for `run` and for `wait`, because the two print the same
/// job's output and must frame it the same way. A note is Herd speaking, so
/// it is named and goes to stderr; the framing line is E-02's first line of
/// the **output**, so it goes to stdout with no prefix of ours — their
/// tooling reads `^ran on: (\S+) \((.*)\)$` there (D109).
/// What a job left behind, on **stderr** and on a disk if one was named
/// (D131).
///
/// Everything here goes to stderr and nothing to stdout. E-02's stream is the
/// `ran on:` line, then the job's own bytes, then the verdict — a file written
/// into it, or even a sentence about one, would be read by the consumer's own
/// tooling as output their tool produced.
///
/// Three things are said, and each of them only when it is true:
///
/// * the refusal, **always**, whether or not `--outputs-to` was given: a
///   requester whose `results.json` did not come back has to hear it, and the
///   verdict is not the place to say it — the tests really did pass or fail,
///   and D20 forbids turning a cap on a side channel into a verdict about the
///   job. So this is a loud sentence and never an exit code.
/// * what was skipped, **always**, for the same reason: "it is a symlink and
///   a symlink is not followed" is the fact somebody needs and the record
///   alone would not reach them.
/// * one line per file written, only with `--outputs-to`.
///
/// **And the fetch happens here, only when `--outputs-to` was given.** A
/// streamed collection's verdict carries the manifest and not the bytes
/// (D148), so a requester that wants the files asks for them — and one that
/// does not never pays for hundreds of megabytes it was going to throw away.
/// That is why `herdctl run` with no flag still exits on the verdict and
/// waits for nothing, which is exactly what it did before streaming existed.
///
/// **It comes back with the sentence naming what is short, if anything is**
/// (D167). A requester that asked for files and did not get all of them must
/// not read a zero, so the answer reaches the exit code through [`code_for`]
/// — and the sentence goes with it, because the verdict document is where a
/// wrapper reads this and `said` is the field it reads it from. A short set is
/// a fact about this fetch and not about the job, so it never touches the
/// verdict itself.
async fn keep_the_outputs(
    config: &herdctl::Config,
    keeping: &Keeping,
    finished: &herdctl::Finished,
) -> Kept {
    let mut collection = finished.outputs.clone();
    // **Said where a reader needs them, and kept so the document has them
    // too** (D-NEXT). Every sentence about this job's files used to reach
    // stderr and nothing else, so a wrapper had to grep prose for facts D166
    // added a file to carry: which path is absent and why, what the hub
    // refused, what it would not believe. They are collected in the order
    // they are printed and handed back. What they are not is an exit code —
    // a fact about one file is never a verdict about the job (D131, D20).
    let mut about_the_outputs = Vec::new();
    // A fetch that failed outright leaves nothing in hand, and a requester
    // that asked for files has then certainly not got them: short, whatever
    // the manifest says.
    let mut could_not_fetch = None;
    if keeping.outputs_to.is_some() {
        match herdctl::fetch_missing_outputs(config, &finished.job_id, &mut collection).await {
            Ok(said) => {
                for line in said {
                    eprintln!("herdctl: {line}");
                    about_the_outputs.push(line);
                }
            }
            // Never the verdict, for D20's reason: the verdict is the job's
            // and a side channel never becomes one. It is the exit code of
            // *this command* that changes, because this command was asked for
            // files and did not come back with them (D167).
            Err(error) => {
                could_not_fetch = Some(format!(
                    "the files {} left could not be fetched: {error:#}",
                    finished.job_id
                ));
            }
        }
    }
    let outputs = &collection;
    if let Some(refusal) = &outputs.refused {
        eprintln!("herdctl: {refusal}");
        about_the_outputs.push(refusal.clone());
    }
    for said in &outputs.skipped {
        let sentence = format!("this job's output: {said}");
        eprintln!("herdctl: {sentence}");
        about_the_outputs.push(sentence);
    }
    let Some(directory) = &keeping.outputs_to else {
        // Nothing asked for, so nothing written — and still one line when
        // there was something to write, because a requester that forgot the
        // option would otherwise never learn the files exist at all.
        if !outputs.files.is_empty() {
            eprintln!(
                "herdctl: {} kept {} output file(s); pass --outputs-to <DIR> to write them",
                finished.job_id,
                outputs.count()
            );
        }
        return Kept {
            short_of_its_files: None,
            about_the_outputs,
        };
    };
    // **What is short, counted off the manifest and not off the directory.**
    // An entry with no bytes is one this requester was promised and does not
    // have, whichever way it travelled and whatever became of it — still
    // crossing when the wait gave up, taken by the hub's sweeper, or never
    // sent at all. The manifest outlives the bytes (D148), so the count is
    // honest for all three.
    let missing = outputs.files.iter().filter(|file| !file.is_held()).count();
    let mut could_not_write = None;
    match herdctl::write_outputs(directory, outputs) {
        Ok(written) => {
            for line in written {
                eprintln!("herdctl: {line}");
            }
        }
        // The sentence is said and the verdict stands; what this costs is
        // this command's own exit code (D167). A requester that cannot write
        // to the directory it named has a problem with that directory, not
        // with the job — and has not got the files either.
        Err(error) => {
            could_not_write = Some(format!(
                "the files {} left could not be written under {}: {error:#}",
                finished.job_id,
                directory.display()
            ));
        }
    }
    // **One sentence naming the shortfall, after the per-file lines.** A
    // reader who has just seen 195 lines needs the count and the
    // consequence, and a wrapper needs to find the reason without parsing
    // 195 lines — which is what the verdict document's `said` is for, so
    // this is returned rather than printed here (D167).
    let short_of_its_files = if let Some(sentence) = could_not_fetch.or(could_not_write) {
        Some(sentence)
    } else if missing > 0 {
        Some(format!(
            "{}: {missing} of {} output file(s) are not here. Every missing path is named \
             above and every one is still on this job's record. This run did part of what it \
             says, so it does not exit 0.",
            finished.job_id,
            outputs.files.len(),
        ))
    } else {
        None
    };
    Kept {
        short_of_its_files,
        about_the_outputs,
    }
}

/// What [`keep_the_outputs`] comes back with: the one sentence that costs
/// this invocation its zero, and the sentences that only have to be on the
/// record (D-NEXT).
///
/// Two fields and not one list, because the difference between them is the
/// exit code and a reader of this type should not have to infer it.
struct Kept {
    /// The shortfall, or `None` when nothing was promised and missing
    /// (D167).
    short_of_its_files: Option<String>,
    /// The refusal and the `skipped` sentences, already printed.
    about_the_outputs: Vec<String>,
}

/// Whether this invocation is going to fetch the job's output files (D167).
///
/// One question, one answer, off the one flag that decides it: `--outputs-to`
/// is the whole of "this requester will fetch them", and a `herdctl run`
/// without it still exits on the verdict and waits for nothing.
fn wanting(keeping: &Keeping) -> herdctl::Wanting {
    if keeping.outputs_to.is_some() {
        herdctl::Wanting::Files
    } else {
        herdctl::Wanting::OnlyTheVerdict
    }
}

fn say_it(known: &Cell<WhatExists>, said: herdctl::Said) {
    match said {
        herdctl::Said::Note(note) => eprintln!("herdctl: {note}"),
        herdctl::Said::Framing(line) => {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
        // Nothing is printed: the state is remembered, and said at the end
        // only if nothing better was learned (`JOB_UNKNOWN_LINE`).
        herdctl::Said::Submitting => learn(known, WhatExists::Unknown),
        // Nothing is printed: the refusal is already the error this run ends
        // with. **Not through `learn`** — this is the one step back, and it
        // is narrow by construction (see `refused`).
        herdctl::Said::Refused => refused(known),
        // Printed here and now, on stderr, because every failure that could
        // swallow it — including one that kills this process — is after
        // this point (`JOB_ID_PREFIX`).
        herdctl::Said::Submitted(job_id) => {
            learn(known, WhatExists::AJob);
            eprintln!("{}", job_id_line(job_id));
        }
    }
}

fn write_now(out: &mut impl Write, data: &[u8]) {
    let _ = out.write_all(data);
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A short collection never exits 0, and never overwrites a verdict**
    /// (D167).
    ///
    /// The two halves are one rule and have to be asserted together, because
    /// each on its own is a plausible mistake. Reporting the job's code
    /// whatever happened is the behaviour the first consumer was working
    /// around: a green build with 195 of 657 files missing exited 0, and a
    /// wrapper acting on that acted on an incomplete set. Reporting 2
    /// whenever anything was missing would be the opposite mistake — a
    /// failing build whose screenshots are short would stop reading as a
    /// failing build, which is what D20 forbids.
    #[test]
    fn a_short_collection_costs_the_zero_and_nothing_else() {
        let passed = JobOutcome::Exited { code: 0 };
        assert_eq!(code_for(&passed, false), 0, "nothing missing, nothing said");
        assert_eq!(
            code_for(&passed, true),
            NOT_SETTLED_CODE,
            "a job that passed and files that did not arrive is a run that did part of \
             what it says"
        );

        // Every code that *is* a verdict, or a named end, outranks the
        // shortfall: it is the more important fact and the missing files are
        // named on stderr beside it.
        for (outcome, code) in [
            (JobOutcome::Exited { code: 1 }, 1u8),
            (JobOutcome::Exited { code: 75 }, 75),
            (JobOutcome::Signalled { signal: 9 }, 128u8.saturating_add(9)),
            (
                JobOutcome::Lost {
                    reason: "its runner went away".to_owned(),
                },
                herdctl::NO_VERDICT_CODE,
            ),
            // D166's ending, passed through untouched: a setup failure is
            // not a short collection and must not start reading as one.
            (
                JobOutcome::SetupFailed {
                    reason: "its runner may not read the capability file".to_owned(),
                },
                herdctl::NO_VERDICT_CODE,
            ),
        ] {
            assert_eq!(
                code_for(&outcome, true),
                code,
                "{outcome:?} keeps its own code whether or not the files arrived"
            );
            assert_eq!(code_for(&outcome, false), code);
        }
    }

    /// The step back a refusal authorises is **one** transition, and the
    /// ratchet keeps its meaning everywhere else (D139, D142).
    ///
    /// The emitting half of the guard — that `run_job` says this only while
    /// no id has been learned — needs a hub that answers `JobAccepted` and
    /// then refuses, which no hub does and which is not worth a fixture that
    /// misbehaves on purpose. What *is* asserted here is the half that would
    /// make such a hub harmless anyway: `AJob` does not move.
    #[test]
    fn a_refusal_walks_back_only_the_one_state_it_proves() {
        // The case this exists for: the submission left, the hub refused it
        // in a frame, so nothing was minted and re-running is safe.
        let known = Cell::new(WhatExists::NotASubmission);
        learn(&known, WhatExists::Nothing);
        learn(&known, WhatExists::Unknown);
        refused(&known);
        assert_eq!(known.get(), WhatExists::Nothing);

        // An id has been printed. No frame can unprint it, and a requester
        // told no job exists would decline work that is already running.
        let known = Cell::new(WhatExists::AJob);
        refused(&known);
        assert_eq!(known.get(), WhatExists::AJob);

        // A command that submits nothing has nothing to be refused about,
        // and must not start claiming otherwise.
        let known = Cell::new(WhatExists::NotASubmission);
        refused(&known);
        assert_eq!(known.get(), WhatExists::NotASubmission);

        // Already proven: idempotent, because nothing is printed for it and
        // a second refusal says the same thing.
        let known = Cell::new(WhatExists::Nothing);
        refused(&known);
        assert_eq!(known.get(), WhatExists::Nothing);

        // And the ratchet itself is untouched: a later failure still cannot
        // make a submission not have left.
        let known = Cell::new(WhatExists::Unknown);
        learn(&known, WhatExists::Nothing);
        assert_eq!(
            known.get(),
            WhatExists::Unknown,
            "only a refusal walks this back, and only because the hub said so"
        );
    }
}
