use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use herd_hub::{Hub, JobDirStore, JobStore, Policy, AGING_STEP, DEFAULT_LISTEN_ADDR, LOST_AFTER};
use herd_proto::HEALTH_EVERY;

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
    /// jobs it was running are attempted once more (E-21). It must outlast
    /// three health reports, or a healthy runner is declared lost between two
    /// of its own: the hub refuses to start otherwise, naming both figures.
    #[arg(long, env = "HERD_LOST_AFTER", default_value_t = LOST_AFTER.as_secs_f64())]
    lost_after: f64,
    /// Seconds between the health reports this hub expects of its runners
    /// (E-40). It sends none itself: this is what `--lost-after` is judged
    /// against at startup. A runner that announces a cadence of its own is
    /// judged against *that* when it dials, and refused by name if
    /// `--lost-after` cannot outlast three of its reports — so this figure is
    /// the expectation, never the whole of the check.
    #[arg(long, env = "HERD_HEALTH_EVERY", default_value_t = HEALTH_EVERY.as_secs_f64())]
    health_every: f64,
    /// Seconds a queued job waits before its priority counts as one level
    /// higher, so that the lowest level never starves (E-18). The bound it
    /// gives is this figure times the number of levels above the job: at the
    /// default, ninety minutes for the lowest of the four. Zero turns aging
    /// off, which is said in the sentence every waiting job carries.
    #[arg(long, env = "HERD_AGING_STEP", default_value_t = AGING_STEP.as_secs_f64())]
    aging_step: f64,
}

/// Seconds as a duration, with a negative figure read as zero rather than
/// silently becoming a very long wait.
fn seconds(value: f64) -> std::time::Duration {
    std::time::Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(std::time::Duration::ZERO)
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
    // A job the hub read and will not act on is named here as well as on the
    // store's own log, because the count below would otherwise be the only
    // number anyone reads and it does not include them (D52).
    let quarantined = store.quarantined()?;
    for (id, why) in &quarantined {
        println!("herd-hub: {id} is quarantined and will not be dispatched: {why}");
    }
    let policy = Policy {
        lost_after: seconds(args.lost_after),
        health_every: seconds(args.health_every),
        aging_step: seconds(args.aging_step),
        ..Policy::default()
    };
    let state_root_id = store.state_root_id().to_owned();
    // A timing that cannot mean what it says is refused here, before the port
    // is bound and before any runner can join (E-21, E-40).
    let hub = Hub::bind_with_policy(args.listen, Arc::new(store), policy).await?;
    println!(
        "herd-hub {} listening on ws://{}, state in {} ({state_root_id}), \
         {unfinished} job(s) not over, {} quarantined, a runner reporting every {:?} is \
         lost after {:?}, a queued job rising one priority level every {:?}",
        herd_hub::hub_version(),
        hub.local_addr(),
        args.state_dir.display(),
        quarantined.len(),
        policy.health_every,
        policy.lost_after,
        policy.aging_step,
    );
    hub.serve().await
}
