use anyhow::Result;
use clap::Parser;
use herd_runner::{Config, DEFAULT_HUB_URL};

/// A Herd runner: dials the hub, executes jobs, streams their logs.
#[derive(Debug, Parser)]
#[command(name = "herd-runner", version, about)]
struct Args {
    /// The hub to dial. The runner opens this connection; nothing listens here.
    #[arg(long, env = "HERD_HUB_URL", default_value = DEFAULT_HUB_URL)]
    hub: String,
    /// Name this runner reports to the hub.
    #[arg(long, env = "HERD_RUNNER_NAME")]
    name: Option<String>,
    /// Bearer token, carried in the handshake. No secret belongs in the repo
    /// (invariant 7), so this comes from the environment or the command line.
    #[arg(long, env = "HERD_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!("herd-runner: joining {}", args.hub);
    herd_runner::run(Config {
        hub_url: args.hub,
        name: args.name,
        token: args.token,
    })
    .await
}
