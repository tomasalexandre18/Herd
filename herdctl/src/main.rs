use std::io::Write;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use herd_proto::{JobOutcome, LogStream};
use herdctl::{could_not_start_message, exit_code_for, Config, DEFAULT_HUB_URL};

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
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match run(args).await {
        Ok(outcome) => {
            // The reason goes to stderr, never to stdout: stdout carried the
            // job's own output and must stay the job's alone.
            if let JobOutcome::CouldNotStart { reason } = &outcome {
                eprintln!("herdctl: {}", could_not_start_message(reason));
            }
            ExitCode::from(exit_code_for(&outcome))
        }
        Err(error) => {
            eprintln!("herdctl: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<JobOutcome> {
    let config = Config {
        hub_url: args.hub,
        token: args.token,
        name: None,
    };
    let Action::Run { command } = args.command;
    // Each stream is written to its own, so a caller's pipes and redirections
    // behave as if the command had run here.
    let finished = herdctl::run_job(&config, command, |stream, data| match stream {
        LogStream::Stdout => {
            let mut out = std::io::stdout();
            let _ = out.write_all(data.as_bytes());
            let _ = out.flush();
        }
        LogStream::Stderr => {
            let mut err = std::io::stderr();
            let _ = err.write_all(data.as_bytes());
            let _ = err.flush();
        }
    })
    .await?;
    Ok(finished.outcome)
}
