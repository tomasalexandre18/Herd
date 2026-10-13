use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use herd_runner::capability::{
    CapabilityConfig, ProbeSettings, DEFAULT_PROBE_INTERVAL_MS, DEFAULT_PROBE_TIMEOUT_MS,
};
use herd_runner::confine::{
    DEFAULT_DISK_BUDGET, DEFAULT_LOG_CAP, DEFAULT_MEMORY_MAX, DEFAULT_NICE, DEFAULT_TIMEOUT_SECONDS,
};
use herd_runner::root::DataRoot;
use herd_runner::size::parse_bytes;
use herd_runner::{
    Backoff, Config, Confinement, DEFAULT_HUB_URL, DRAIN_GRACE, HEALTH_EVERY, HUB_SILENCE_LIMIT,
};

/// A Herd runner: dials the hub, executes jobs under caps, streams their logs.
///
/// Every cap is a number, and every path is a path: there is no policy
/// language and no plugin here, by design (invariant 1).
///
/// Asked to stop — `SIGTERM`, or a Ctrl-C — it drains: it takes no new job,
/// ends what it is running, says so to the hub, and exits inside five seconds
/// (invariant 2). A verdict the hub has not acknowledged by then is written
/// under the data root and delivered on the next start (D34).
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
    /// Seconds between health reports (E-40).
    #[arg(long, env = "HERD_HEALTH_EVERY", default_value_t = HEALTH_EVERY.as_secs_f64())]
    health_every: f64,
    /// Seconds of silence from the hub after which this link is dead.
    #[arg(long, env = "HERD_HUB_SILENCE_LIMIT", default_value_t = HUB_SILENCE_LIMIT.as_secs_f64())]
    hub_silence_limit: f64,
    /// Seconds before the first redial; it doubles from there.
    #[arg(long, env = "HERD_REDIAL_FIRST", default_value_t = Backoff::default().first.as_secs_f64())]
    redial_first: f64,
    /// The longest a redial delay ever gets, in seconds.
    #[arg(long, env = "HERD_REDIAL_CAP", default_value_t = Backoff::default().cap.as_secs_f64())]
    redial_cap: f64,
    /// Seconds of an unreachable hub after which this runner gives up and
    /// says so. Long enough to outlast a maintenance window on purpose.
    #[arg(long, env = "HERD_GIVE_UP_AFTER", default_value_t = Backoff::default().give_up_after.as_secs_f64())]
    give_up_after: f64,
    /// Seconds a drain may take before the process exits regardless. Must
    /// stay under the five seconds invariant 2 promises the owner.
    #[arg(long, env = "HERD_DRAIN_GRACE", default_value_t = DRAIN_GRACE.as_secs_f64())]
    drain_grace: f64,
    /// The one directory this runner may write in (E-45). Defaults to
    /// `$XDG_STATE_HOME/herd-runner`, or `~/.local/state/herd-runner`.
    /// A root this runner cannot use is refused at startup, naming it.
    #[arg(long, env = "HERD_DATA_ROOT", value_name = "PATH")]
    data_root: Option<PathBuf>,
    /// Kernel memory ceiling per job: `2GiB`, `512MiB`, or plain bytes.
    #[arg(long, env = "HERD_JOB_MEMORY_MAX", value_name = "SIZE")]
    job_memory_max: Option<String>,
    /// Time cap per job, in seconds. A job past it ends by name, not by hang.
    #[arg(long, env = "HERD_JOB_TIMEOUT", value_name = "SECONDS")]
    job_timeout: Option<u64>,
    /// How much log one job may produce before the runner stops it.
    #[arg(long, env = "HERD_JOB_LOG_CAP", value_name = "SIZE")]
    job_log_cap: Option<String>,
    /// How much one job's workspace may hold before the runner stops it.
    #[arg(long, env = "HERD_JOB_DISK_BUDGET", value_name = "SIZE")]
    job_disk_budget: Option<String>,
    /// CPU niceness added to a job. IO niceness goes to the idle class with it.
    #[arg(long, env = "HERD_JOB_NICE", value_name = "N")]
    job_nice: Option<i32>,
    /// A delegated cgroup v2 directory to hang job cgroups under, instead of
    /// this runner's own. Useful where the runner is not a systemd unit.
    #[arg(long, env = "HERD_CGROUP_ROOT", value_name = "PATH")]
    cgroup_root: Option<PathBuf>,
    /// A command, by absolute path, that this runner may call even though it
    /// needs privilege (E-46). Repeat the option, or separate paths with `:`
    /// in the environment variable. A list, never a pattern: anything not on
    /// it is refused by name.
    #[arg(
        long = "privileged-helper",
        env = "HERD_PRIVILEGED_HELPERS",
        value_delimiter = ':',
        value_name = "PATH"
    )]
    privileged_helpers: Vec<String>,
    /// A JSON file declaring what this runner offers: each capability's name,
    /// kind (`exclusive`, `counted` with slots, or `shared`), what it cannot
    /// be held alongside, and an optional external busy probe — a lock-file
    /// path or a command whose exit code means busy (E-24, E-26).
    ///
    /// Read strictly: an unknown key, or a key written twice, refuses this
    /// runner's startup naming the key and the capability. A `probe` where
    /// `busy_probe` was meant, or a second `busy_probe` overwriting the
    /// first, would otherwise offer a capability something outside Herd is
    /// holding (E-26).
    ///
    /// Data, not code: there is no plugin here and never will be (invariant
    /// 1). Secrets stay out of the repo by this being a path given at
    /// startup (invariant 7).
    #[arg(long, env = "HERD_CAPABILITIES", value_name = "PATH")]
    capabilities: Option<PathBuf>,
    /// A capability this runner offers, by name, with no kind and no probe —
    /// `shared`. Repeat the option, or separate names with `:` in the
    /// environment variable. Anything that needs a kind goes in the file.
    #[arg(
        long = "declare-capability",
        env = "HERD_DECLARE_CAPABILITIES",
        value_delimiter = ':',
        value_name = "NAME"
    )]
    declare_capability: Vec<String>,
    /// A capability this runner refuses, whatever the host has (E-24).
    /// Repeat the option, or separate names with `:` in the environment
    /// variable. A denial wins over detection and over declaration both.
    #[arg(
        long = "deny-capability",
        env = "HERD_DENY_CAPABILITIES",
        value_delimiter = ':',
        value_name = "NAME"
    )]
    deny_capability: Vec<String>,
    /// Do not look at the host for capabilities at all; offer only what is
    /// declared. For a test that must read the same on every machine.
    #[arg(long, env = "HERD_NO_DETECT")]
    no_detect: bool,
    /// How often the external busy probes are read, in milliseconds.
    #[arg(long, env = "HERD_PROBE_INTERVAL", value_name = "MS")]
    probe_interval: Option<u64>,
    /// How long one probe may take before it is killed and read as
    /// unreadable, which suspends its capability. In milliseconds.
    #[arg(long, env = "HERD_PROBE_TIMEOUT", value_name = "MS")]
    probe_timeout: Option<u64>,
}

fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(Duration::ZERO)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let data_root = match args.data_root {
        Some(path) => path,
        None => DataRoot::default_path().map_err(anyhow::Error::msg)?,
    };
    let mut confinement = Confinement::with_data_root(data_root);
    if let Some(size) = args.job_memory_max {
        confinement.memory_max = parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--job-memory-max")?;
    } else {
        confinement.memory_max = DEFAULT_MEMORY_MAX;
    }
    confinement.timeout = Duration::from_secs(args.job_timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS));
    confinement.log_cap = match args.job_log_cap {
        Some(size) => parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--job-log-cap")?,
        None => DEFAULT_LOG_CAP,
    };
    confinement.disk_budget = match args.job_disk_budget {
        Some(size) => parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--job-disk-budget")?,
        None => DEFAULT_DISK_BUDGET,
    };
    confinement.nice = args.job_nice.unwrap_or(DEFAULT_NICE);
    confinement.cgroup_root = args.cgroup_root;
    confinement.privileged_helpers = args.privileged_helpers;

    let probes = ProbeSettings {
        interval: Duration::from_millis(args.probe_interval.unwrap_or(DEFAULT_PROBE_INTERVAL_MS)),
        timeout: Duration::from_millis(args.probe_timeout.unwrap_or(DEFAULT_PROBE_TIMEOUT_MS)),
    };
    // Resolved here, before the dial: a capability file that does not read is
    // a machine that would offer the wrong thing, so it is refused at startup
    // naming the path, exactly as a bad data root is (E-45's rule).
    let capabilities = CapabilityConfig {
        file: args.capabilities,
        declared: args.declare_capability,
        denied: args.deny_capability,
        detect: !args.no_detect,
        probes,
    }
    .resolve()
    .map_err(anyhow::Error::msg)
    .context("this runner's capabilities")?;

    println!("herd-runner: joining {}", args.hub);
    herd_runner::run(Config {
        hub_url: args.hub,
        name: args.name,
        token: args.token,
        health_every: seconds(args.health_every),
        hub_silence_limit: seconds(args.hub_silence_limit),
        redial: Backoff {
            first: seconds(args.redial_first),
            cap: seconds(args.redial_cap),
            give_up_after: seconds(args.give_up_after),
        },
        drain_grace: seconds(args.drain_grace),
        confinement,
        capabilities,
        probes,
    })
    .await
}
