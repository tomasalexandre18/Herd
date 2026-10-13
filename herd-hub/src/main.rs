use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use herd_hub::{
    Hub, JobDirStore, JobStore, Policy, Stickiness, AGING_STEP, DEFAULT_LISTEN_ADDR,
    DRAIN_FIRST_LIMIT, LOST_AFTER, REQUEUE_BUDGET, STICKINESS_LIMIT, WAIT_RESTATEMENT,
};
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
    /// Seconds a job waits for the runner it is sticking to before this hub
    /// places it on a less preferred one (E-19).
    ///
    /// A job goes to the most preferred machine that can run it, and waits
    /// for that machine while it is connected rather than falling back to a
    /// lesser one that is free. This is the safety limit on that wait, and
    /// E-19 asks for it to be visible: it is printed at startup and named in
    /// the sentence of every job waiting on it. Zero makes a job move the
    /// moment anything else can take it, which is to say it turns the
    /// stickiness off.
    #[arg(long, env = "HERD_STICKINESS_LIMIT", default_value_t = STICKINESS_LIMIT.as_secs_f64())]
    stickiness_limit: f64,
    /// Seconds between two tellings of why a queued job is still waiting,
    /// to whoever is following it (E-04).
    ///
    /// E-04 asks for "at least every 60 s", which is the default. It is a
    /// setting because both ways of failing it are real: a rarer cadence
    /// leaves a ten-minute wait in silence, and a line per dispatch pass
    /// fills a transcript with hundreds of identical sentences. Over a wait
    /// of W a follower gets at most W / this + 1 lines. Zero is refused.
    #[arg(long, env = "HERD_WAIT_RESTATEMENT", default_value_t = WAIT_RESTATEMENT.as_secs_f64())]
    wait_restatement: f64,
    /// Seconds a job may hold a runner empty while it waits for that host to
    /// go quiet (E-27).
    ///
    /// A capability its machine's operator declared `drain_first` starts only
    /// on a quiet host, and while such a job is waiting the host claims
    /// nothing new. This bounds that: past it the host takes other work
    /// again and the job says, in the sentence on its record, that the drain
    /// did not win. Fifteen minutes, the figure E-27 names.
    #[arg(long, env = "HERD_DRAIN_FIRST_LIMIT", default_value_t = DRAIN_FIRST_LIMIT.as_secs_f64())]
    drain_first_limit: f64,
    /// How many goes a job gets at a capability that turned out to be taken
    /// (D76).
    ///
    /// A job may declare one exit code meaning "what you gave me has gone"
    /// (`herdctl --requeue-exit-code`, 75 for the first consumer). Such a
    /// code is not a verdict: it requeues the job with the capability read
    /// again, this many times, and then ends it naming the code and the
    /// machines that answered it. **Separate from `--attempts`**, which is
    /// E-21's budget for a runner that disappears.
    #[arg(long, env = "HERD_REQUEUE_BUDGET", default_value_t = REQUEUE_BUDGET)]
    requeue_budget: u32,
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
        stickiness: Stickiness {
            limit: seconds(args.stickiness_limit),
        },
        wait_restatement: seconds(args.wait_restatement),
        drain_first_limit: seconds(args.drain_first_limit),
        requeue_budget: args.requeue_budget,
        ..Policy::default()
    };
    let state_root_id = store.state_root_id().to_owned();
    // A timing that cannot mean what it says is refused here, before the port
    // is bound and before any runner can join (E-21, E-40).
    let hub = Hub::bind_with_policy(args.listen, Arc::new(store), policy).await?;
    println!(
        "herd-hub {} listening on ws://{}, state in {} ({state_root_id}), \
         {unfinished} job(s) not over, {} quarantined, a runner reporting every {:?} is \
         lost after {:?}, a queued job rising one priority level every {:?}, and waiting \
         up to {:?} for the runner it is sticking to before it is placed elsewhere, and \
         saying again every {:?} why a job it is following is still queued; a job that \
         needs a quiet host holds one empty for at most {:?}, and a job whose declared \
         requeue code comes back gets {} goes at it, separately from the {} goes it gets \
         at a runner that goes away",
        herd_hub::hub_version(),
        hub.local_addr(),
        args.state_dir.display(),
        quarantined.len(),
        policy.health_every,
        policy.lost_after,
        policy.aging_step,
        policy.stickiness.limit,
        policy.wait_restatement,
        policy.drain_first_limit,
        policy.requeue_budget,
        policy.attempts_allowed,
    );
    hub.serve().await
}
