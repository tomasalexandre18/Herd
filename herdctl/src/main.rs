use std::io::Write;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use herd_proto::{JobId, JobOutcome, LabelSet, LogStream, Priority};
use herdctl::{
    could_not_start_message, exit_code_for, impossible_exit_code_reason, no_verdict_message,
    no_verdict_reason, stopped_by_cap_message, Config, DEFAULT_HUB_URL, NO_VERDICT_CODE,
};

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

/// The requester CLI: submit a job, follow its log, exit with its code.
#[derive(Debug, Parser)]
#[command(name = "herdctl", version, about)]
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
        /// The command, after `--`.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
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
        /// The command, after `--`.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
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
        /// The command, after `--`.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
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
        /// Why. Required for `off` and `drain`, refused for `on`.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Wait for a job that already exists, and exit with its code.
    Wait {
        /// The id a submission printed.
        job_id: String,
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

/// What a subcommand leaves the process to exit with.
enum Done {
    /// A job ended, and its end decides the exit code (D16).
    Job(JobOutcome),
    /// The command did its own job; nothing ran.
    Said,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match run(args).await {
        Ok(Done::Job(outcome)) => {
            // The reason goes to stderr, never to stdout: stdout carried the
            // job's own output and must stay the job's alone.
            match &outcome {
                JobOutcome::CouldNotStart { reason } => {
                    eprintln!("herdctl: {}", could_not_start_message(reason));
                }
                JobOutcome::StoppedByCap { cap, limit, detail } => {
                    eprintln!(
                        "herdctl: {}",
                        stopped_by_cap_message(*cap, *limit, detail.as_deref())
                    );
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
            if let Some(reason) = impossible_exit_code_reason(&outcome) {
                eprintln!("herdctl: {}", no_verdict_message(&reason));
            }
            // Cancelled, or lost with its runner: a real end, and not one that
            // carries a verdict. Said in words, because 125 cannot say which
            // of them it was (D20).
            if let Some(reason) = no_verdict_reason(&outcome) {
                eprintln!("herdctl: {}", no_verdict_message(&reason));
            }
            ExitCode::from(exit_code_for(&outcome))
        }
        Ok(Done::Said) => ExitCode::SUCCESS,
        // Nothing ran, or nothing came back: Herd has no verdict to give, and
        // saying so with 1 would be indistinguishable from a job whose tests
        // failed (D20). One code, one meaning, and the reason on stderr.
        Err(error) => {
            eprintln!("herdctl: {}", no_verdict_message(&format!("{error:#}")));
            ExitCode::from(NO_VERDICT_CODE)
        }
    }
}

async fn run(args: Args) -> Result<Done> {
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
            command,
        } => {
            // Said out loud, on stderr, before anything runs: a level is
            // what decides whether this job goes before or after somebody
            // else's, and a default nobody can see is the defect E-18 is
            // most easily broken by. stdout stays the job's own.
            eprintln!("herdctl: {}", submitting_at(priority));
            // Each stream is written to its own, so a caller's pipes and
            // redirections behave as if the command had run here.
            let finished = herdctl::run_job(
                &config,
                command,
                labels_of(labels)?,
                priority,
                declared(build_thread_mib, memory_mib, tmpfs_mib, asking)?,
                |stream, data| match stream {
                    LogStream::Stdout => write_now(&mut std::io::stdout(), data),
                    LogStream::Stderr => write_now(&mut std::io::stderr(), data),
                },
                // Herd's own words, never the job's: stdout stays the job's
                // alone. A queued job's reason arrives here (E-26).
                |note| eprintln!("herdctl: {note}"),
            )
            .await?;
            Ok(Done::Job(finished.outcome))
        }
        Action::Submit {
            labels,
            priority,
            build_thread_mib,
            memory_mib,
            tmpfs_mib,
            asking,
            command,
        } => {
            eprintln!("herdctl: {}", submitting_at(priority));
            let job_id = herdctl::submit_job(
                &config,
                command,
                labels_of(labels)?,
                priority,
                declared(build_thread_mib, memory_mib, tmpfs_mib, asking)?,
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
            command,
        } => {
            let asked = declared(build_thread_mib, memory_mib, tmpfs_mib, asking)?
                .asked(command, labels_of(labels)?, priority);
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
                        println!("{}", herdctl::switch_line(runner));
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
                    println!("{}", herdctl::set_switch(&config, runner, switch).await?);
                }
            }
            Ok(Done::Said)
        }
        Action::Wait { job_id } => {
            // A wait prints the job's output exactly as `run` does: the log
            // the hub had recorded before this wait attached, then whatever
            // arrives afterwards. Without it a detached requester could only
            // read a job's output by reading the hub's state directory, which
            // a requester on another machine cannot do (D22).
            let finished = herdctl::wait_for_job(
                &config,
                JobId::new(job_id),
                |stream, data| match stream {
                    LogStream::Stdout => write_now(&mut std::io::stdout(), data),
                    LogStream::Stderr => write_now(&mut std::io::stderr(), data),
                },
                |note| eprintln!("herdctl: {note}"),
            )
            .await?;
            Ok(Done::Job(finished.outcome))
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

/// Everything a requester declares about a job, from the options it typed.
///
/// One function, for `run`, `submit` **and** `would-run`, which is what makes
/// a dry run answer about the job a submission would make (E-07). A deadline
/// is resolved here, against this machine's clock, so what travels is an
/// instant (E-09).
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
    })
}

fn labels_of(labels: Vec<String>) -> Result<LabelSet> {
    LabelSet::new(labels)
        .map_err(anyhow::Error::msg)
        .context("--label")
}

fn write_now(out: &mut impl Write, data: &[u8]) {
    let _ = out.write_all(data);
    let _ = out.flush();
}
