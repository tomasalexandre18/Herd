use std::io::Write;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use herd_proto::{JobId, JobOutcome, LogStream};
use herdctl::{
    could_not_start_message, exit_code_for, impossible_exit_code_reason, no_verdict_message,
    no_verdict_reason, stopped_by_cap_message, Config, DEFAULT_HUB_URL, NO_VERDICT_CODE,
};

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
        /// The command, after `--`.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// Submit a command and print its job id, without waiting for it.
    ///
    /// The job runs and is recorded whether anyone is following it or not, so
    /// `herdctl wait <id>` works any time afterwards.
    Submit {
        /// The command, after `--`.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
        command: Vec<String>,
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
        Action::Run { command } => {
            // Each stream is written to its own, so a caller's pipes and
            // redirections behave as if the command had run here.
            let finished = herdctl::run_job(&config, command, |stream, data| match stream {
                LogStream::Stdout => write_now(&mut std::io::stdout(), data),
                LogStream::Stderr => write_now(&mut std::io::stderr(), data),
            })
            .await?;
            Ok(Done::Job(finished.outcome))
        }
        Action::Submit { command } => {
            let job_id = herdctl::submit_job(&config, command).await?;
            // The id alone on stdout, so that `id=$(herdctl submit -- ...)`
            // is all a hook needs (E-42).
            println!("{job_id}");
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
    }
}

fn write_now(out: &mut impl Write, data: &[u8]) {
    let _ = out.write_all(data);
    let _ = out.flush();
}
