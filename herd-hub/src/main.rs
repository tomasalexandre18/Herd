use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use herd_hub::{Hub, JobDirStore, JobStore, Policy, DEFAULT_LISTEN_ADDR, LOST_AFTER};

/// Where the hub keeps its state unless told otherwise. Relative on purpose:
/// a development hub writes beside the repository, and the deployment names
/// its volume (Q9, M16).
const DEFAULT_STATE_DIR: &str = "herd-state";

/// The Herd hub: accepts a runner and requesters, dispatches jobs, relays logs.
#[derive(Debug, Parser)]
#[command(name = "herd-hub", version, about)]
struct Args {
    /// Address to listen on. Plain ws: TLS is the reverse proxy's job (D4).
    #[arg(long, env = "HERD_LISTEN", default_value = DEFAULT_LISTEN_ADDR)]
    listen: SocketAddr,
    /// Directory holding the hub's state: one directory per job (D8). It is
    /// created if it is missing, and the hub refuses to start if it cannot be
    /// written.
    #[arg(long, env = "HERD_STATE_DIR", default_value = DEFAULT_STATE_DIR)]
    state_dir: PathBuf,
    /// Seconds without a health report after which a runner is lost and the
    /// jobs it was running are attempted once more (E-21).
    #[arg(long, env = "HERD_LOST_AFTER", default_value_t = LOST_AFTER.as_secs_f64())]
    lost_after: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let store = JobDirStore::open(&args.state_dir).with_context(|| {
        format!(
            "the hub's state directory {} is not usable",
            args.state_dir.display()
        )
    })?;
    let unfinished = store.unfinished_jobs()?.len();
    let policy = Policy {
        lost_after: std::time::Duration::try_from_secs_f64(args.lost_after.max(0.0))
            .unwrap_or(LOST_AFTER),
        ..Policy::default()
    };
    let hub = Hub::bind_with_policy(args.listen, Arc::new(store), policy).await?;
    println!(
        "herd-hub {} listening on ws://{}, state in {}, {unfinished} job(s) not over, \
         a runner is lost after {:?}",
        herd_hub::hub_version(),
        hub.local_addr(),
        args.state_dir.display(),
        policy.lost_after,
    );
    hub.serve().await
}
