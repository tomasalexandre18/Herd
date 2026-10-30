use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use herd_runner::capability::{
    CapabilityConfig, ProbeSettings, DEFAULT_PROBE_INTERVAL_MS, DEFAULT_PROBE_TIMEOUT_MS,
};
use herd_runner::cgroup::{
    CpuQuota, IoCeiling, DEFAULT_CPU_WEIGHT, DEFAULT_IO_WEIGHT, DEFAULT_PIDS_MAX,
};
use herd_runner::confine::{
    DEFAULT_DISK_BUDGET, DEFAULT_LOG_CAP, DEFAULT_MEMORY_MAX, DEFAULT_NICE, DEFAULT_OUTPUT_CAP,
    DEFAULT_TIMEOUT_SECONDS,
};
use herd_runner::mesh;
use herd_runner::packstore::PACK_BUDGET_FLAG;
use herd_runner::root::DataRoot;
use herd_runner::size::parse_bytes;
use herd_runner::switch::DEFAULT_POLL_MS;
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
/// A repeated option is not an error here: the last one wins (D163).
///
/// This runner's command line has two authors. A deployment writes the
/// figures it chose — `deploy/nixos-herd-runner.nix` and
/// `deploy/herd-runner.service` both put every cap on the `ExecStart` line —
/// and the operator of the machine appends to it, through
/// `services.herd-runner.extraArgs` or by hand. Without this, the second
/// author cannot correct the first: clap's default is to refuse the whole
/// command line with *"cannot be used multiple times"*, so a figure a
/// deployment stated could only be changed by editing the deployment.
///
/// Options that collect a list — `--privileged-helper`,
/// `--declare-capability`, `--deny-capability` — are unaffected: their action
/// is to append, and every occurrence is still kept.
#[derive(Debug, Parser)]
#[command(name = "herd-runner", version, about, args_override_self = true)]
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
    /// How much declared output one job may have carried into its record:
    /// everything it leaves under `{out}`, with its relative paths.
    ///
    /// 1 GiB by default, and the flag **raises** it as well as lowers it.
    /// Output too large for one control frame is streamed rather than inlined
    /// (`herd_proto::INLINE_OUTPUT_MAX`, 32 MiB), so there is no longer a
    /// frame-shaped ceiling for this figure to be.
    ///
    /// What bounds it instead is `--job-disk-budget`: a job writes `{out}`
    /// inside its workspace, and the workspace stops at that budget first, so
    /// a cap above it could never bind. Such a cap is **clamped** to the
    /// budget at startup, with both figures printed, rather than refused — an
    /// unreachable cap says more than it can do, which is no reason for a
    /// working runner to stop starting.
    ///
    /// Over the cap **no file comes back** and the job's record says so with
    /// both figures: a cap on an output is not a truncation of it, and a
    /// cut-down results file that still parses reads like the truth. `0`
    /// returns a verdict and nothing else.
    #[arg(long, env = "HERD_JOB_OUTPUT_CAP", value_name = "SIZE")]
    job_output_cap: Option<String>,
    /// How much one job's workspace may hold before the runner stops it.
    #[arg(long, env = "HERD_JOB_DISK_BUDGET", value_name = "SIZE")]
    job_disk_budget: Option<String>,
    /// How much of this machine's disk the pack store may hold: `40GiB`,
    /// `512MiB`, or plain bytes.
    ///
    /// **This is not `--job-disk-budget`, which is 20 GiB per job by
    /// default.** That one bounds the scratch space of one job and is
    /// reclaimed with it; this one bounds what this machine keeps *between*
    /// jobs — the content one job leaves for a later job of the same commit,
    /// under `packs/` in the data root.
    ///
    /// **Absent by default, and absent means nothing bounds it**: the store
    /// grows until the disk does not, and this runner says so at startup.
    /// There is no default figure and there will not be one — how much of a
    /// disk is lent to Herd is a statement about the machine, like
    /// `--host-memory`, and a number chosen here would be right on the
    /// machine it was chosen on and absurd on every other.
    ///
    /// What is printed against it at startup is **measured**: the bytes
    /// `packs/objects/` really holds, each content once, and how many whole
    /// packs that is.
    #[arg(long, env = "HERD_PACK_BUDGET", value_name = "SIZE")]
    pack_budget: Option<String>,
    /// CPU niceness added to a job. IO niceness goes to the idle class with it.
    #[arg(long, env = "HERD_JOB_NICE", value_name = "N")]
    job_nice: Option<i32>,
    /// How much CPU a job gets when something else on this machine wants it:
    /// `cpu.weight` in the job's cgroup, 1 to 10000 against the 100 an
    /// ordinary process has. 10 by default.
    ///
    /// A share and not a cap: a job at 10 yields almost all of a contended
    /// core and still uses every core of a machine nobody else is using,
    /// which is what niceness alone was failing to do (the first consumer's
    /// twelve-thread build took a whole host down beside `nice 10`).
    #[arg(long, env = "HERD_JOB_CPU_WEIGHT", value_name = "N")]
    job_cpu_weight: Option<u32>,
    /// A hard CPU ceiling per job, in cores: `4`, `1.5`. **None by default.**
    ///
    /// The reasoning is `--host-memory`'s: a hard ceiling is a lend an
    /// operator declares, not something Herd assumes, and a quota wastes a
    /// machine nobody else is using. Where it is given and this host cannot
    /// enforce it, the startup is refused rather than quietly reduced to the
    /// weight above.
    #[arg(long, env = "HERD_JOB_CPU_MAX", value_name = "CORES")]
    job_cpu_max: Option<f64>,
    /// The same share for block IO: `io.weight` in the job's cgroup, 1 to
    /// 10000 against 100. 10 by default.
    ///
    /// Honoured only by the bfq scheduler or by blk-iocost. On a host where
    /// neither is in play the runner says at startup that this number is
    /// written and ignored, because a bound a runner does not have is a bound
    /// it must not claim.
    #[arg(long, env = "HERD_JOB_IO_WEIGHT", value_name = "N")]
    job_io_weight: Option<u32>,
    /// A hard IO ceiling per job, per second, for the device the data root is
    /// on: `rbps=200MB,wbps=100MB,riops=4000,wiops=2000`. Any subset of those
    /// four keys, at least one of them. **None by default.**
    ///
    /// `io.weight` above is a *share* of contended bandwidth: it decides who
    /// gets the disk when two cgroups both want it and caps nothing, so one
    /// job alone still saturates an idle disk. This is a *ceiling*, enforced
    /// by blk-throttle whatever the IO scheduler is — the lever for a host
    /// where one build can freeze everything else on it.
    ///
    /// There is no default and there will not be one: a throughput figure is
    /// right on the machine it was measured on and absurd everywhere else, so
    /// absent means no `io.max` line written at all rather than a guess.
    ///
    /// **How to pick one, rather than invent one.** Where blk-iocost is
    /// enabled, the kernel has already measured the device and left the
    /// figures in `/sys/fs/cgroup/io.cost.model`, keyed by the same
    /// `major:minor` this ceiling names: `rbps`, `wbps` and the IOPS beside
    /// them. A defensible ceiling is a fraction of those — a quarter is a
    /// reasonable first try — and not a round number somebody liked. The
    /// figure is a statement about **how much of a shared device this machine
    /// lends Herd**, which is `--host-memory`'s shape exactly; on a host where
    /// Herd shares one queue with production services, that fraction is the
    /// whole question. Include the IOPS keys as well as the bytes: a freeze is
    /// a latency event, and small random IO is what causes one.
    ///
    /// `io.max` is **per-device**, so the ceiling is written for the device
    /// the data root is on and nothing else. Where this host cannot carry a
    /// ceiling for that path — a ZFS data root, no cgroup delegation, a path
    /// whose device cannot be resolved — the startup is **refused** naming
    /// the path and the reason, never reduced to the weight. A job that
    /// writes an absolute path *outside* its data root is bounded by neither
    /// this nor the weight.
    #[arg(long, env = "HERD_JOB_IO_MAX", value_name = "rbps=…,wbps=…")]
    job_io_max: Option<String>,
    /// How many processes one job may hold at once: `pids.max` in its cgroup.
    /// 1024 by default.
    ///
    /// A plain number and not optional: a fork bomb inside a job must not be
    /// able to take the host down, and no build needs an unbounded process
    /// count.
    #[arg(long, env = "HERD_JOB_PIDS_MAX", value_name = "N")]
    job_pids_max: Option<u64>,
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
    /// How much memory Herd's jobs may use on this host: `17GiB`, `2048MiB`,
    /// or plain bytes.
    ///
    /// The input to the `-j` a build gets here (E-22). Read from
    /// `/proc/meminfo` when not given, which is the whole machine — so an
    /// owner lending part of her PC says how much here. A quarter of
    /// whatever it is is kept back from build threads in any case, because a
    /// runner must never be the reason its host falls over (invariant 2).
    #[arg(long, env = "HERD_HOST_MEMORY", value_name = "SIZE")]
    host_memory: Option<String>,
    /// How many hardware threads Herd's jobs may use on this host (E-22).
    /// Read from the host when not given.
    #[arg(long, env = "HERD_HOST_THREADS", value_name = "N")]
    host_threads: Option<u32>,
    /// How much of this host's memory stays this host's, whatever is queued:
    /// `6GiB`, `3072MiB`, or plain bytes (E-29).
    ///
    /// The per-runner floor admission subtracts before it admits anything, so
    /// that a machine keeps room to live (invariant 2). A fifth of the
    /// memory, between 1 GiB and 8 GiB, when not given — which is about the
    /// 6 GiB the first consumer asks of their server and the 3 GiB they ask of
    /// the owner's PC. A floor at or past the memory lent is refused at
    /// startup: it is a machine that can admit nothing, and denying its
    /// capabilities is the honest way to say that.
    #[arg(long, env = "HERD_HOST_MEMORY_FLOOR", value_name = "SIZE")]
    host_memory_floor: Option<String>,
    /// How much room on this host's tmpfs a job may reserve: `8GiB`,
    /// `2048MiB`, or plain bytes (E-31).
    ///
    /// Measured from `--host-tmpfs-path` when not given, and clamped to what
    /// is really there when it is: a figure larger than the mount is a
    /// reservation nothing backs. A figure that can be neither given nor
    /// measured stays **absent** — never zero — and a job that needs a
    /// reservation is then refused on this machine by name rather than
    /// admitted on an assumption.
    #[arg(long, env = "HERD_HOST_TMPFS", value_name = "SIZE")]
    host_tmpfs: Option<String>,
    /// Which tmpfs `--host-tmpfs` is measured from. `/dev/shm` by default.
    #[arg(long, env = "HERD_HOST_TMPFS_PATH", value_name = "PATH")]
    host_tmpfs_path: Option<String>,
    /// How preferred this runner is when more than one could run a job
    /// (E-19). Higher goes first; zero by default.
    ///
    /// A job waits for the most preferred machine that could run it rather
    /// than falling back to a lesser one that is free — up to the hub's
    /// `--stickiness-limit`. One integer, declared by whoever runs the
    /// machine: a requester never names a machine (invariant 6).
    #[arg(
        long,
        env = "HERD_PREFERENCE",
        value_name = "N",
        allow_negative_numbers = true
    )]
    preference: Option<i32>,
    /// Where the owner's switch is kept (E-33). Under the data root by
    /// default, as `switch`.
    ///
    /// A file rather than a socket, because a runner takes no instruction
    /// over the network (invariant 3): with no mesh address configured it
    /// listens on nothing at all, and the one socket `--mesh-listen` may ever
    /// open serves a job's artefacts to peers, never commands — which
    /// addresses that may be is `mesh.rs`'s rule (D10, D11, D222). So
    /// `herd-runner switch off --reason ...` writes the file
    /// and a running runner reads it on a short clock. That it is a file is
    /// also what makes it work with the hub down — which is when an owner is
    /// most likely to want her machine back — and what makes a position
    /// outlive a reboot.
    #[arg(long, env = "HERD_SWITCH_FILE", value_name = "PATH")]
    switch_file: Option<PathBuf>,
    /// How often the switch file is read, in milliseconds. A second by
    /// default, against E-33's ten seconds.
    #[arg(long, env = "HERD_SWITCH_POLL", value_name = "MS")]
    switch_poll: Option<u64>,
    /// The one address this runner may serve a job's artefacts from, on the
    /// private mesh its owner already runs: `10.9.0.3`, `10.9.0.3:9440`,
    /// `[fd7a:1::3]:9440`. **Absent by default, and absent means this machine
    /// serves nothing and listens on nothing** — which is a complete runner,
    /// not a reduced one (D10's outbound-only path).
    ///
    /// Judged at startup and **refused by name** when it would break
    /// invariant 3: a wildcard (`0.0.0.0`, `::`), an address of this host's
    /// default-route interface, or a globally routable address. Accepted for
    /// loopback and for a private, carrier-grade-NAT (a tailnet's
    /// `100.64/10`), link-local or unique-local address that is *not* on the
    /// default-route interface — the three conditions of D11, refused the way
    /// a bad data root is (E-45).
    ///
    /// A machine with one network card on a flat LAN therefore has no
    /// permitted address here and is outbound-only. That is the cost of
    /// *"never behind a port forwarded on someone's box"*, not a defect:
    /// join it to a mesh and name the address the mesh gave it.
    ///
    /// Without a port, the kernel picks one and the advertised endpoint
    /// carries what it got. The owner's switch still decides whether
    /// anything is served at all: `off` serves nothing (E-33, D11).
    #[arg(long, env = "HERD_MESH_LISTEN", value_name = "ADDR")]
    mesh_listen: Option<String>,
    /// What peers are told to dial, when it differs from `--mesh-listen` —
    /// which it does on a mesh that translates addresses. Defaults to the
    /// listen address.
    ///
    /// Carries the same class rule as `--mesh-listen`, so a wildcard or a
    /// globally routable address is refused: this is the address a peer
    /// dials, and an address the public internet can reach is not a private
    /// mesh. No routing check, though — this host's own route table says
    /// nothing about what a peer can reach.
    #[arg(long, env = "HERD_MESH_ADVERTISE", value_name = "ADDR")]
    mesh_advertise: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

/// What this binary does instead of being a runner.
///
/// Two subcommands, and both are the machine owner's: everything else about a
/// runner is an option on the runner itself.
#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Validate a capability file and read each of its probes once (D165).
    ///
    /// `herd-runner check --capabilities /etc/herd/capabilities.json` reads
    /// the file exactly as a runner reads it, says of every command probe
    /// whether its program can be started at all, runs each probe once and
    /// prints what it answered. It exits non-zero on anything that would
    /// refuse a runner's startup, so it is the thing to run *before*
    /// deploying a file rather than after a job is late.
    ///
    /// A probe that runs and answers something unreadable is printed and does
    /// not fail this command: that suspends its capability at runtime (D49)
    /// and is often the honest answer of the moment — a lock that is held.
    Check {
        /// The file to check. Defaults to `HERD_CAPABILITIES`, so a machine
        /// whose unit sets it needs no argument here.
        #[arg(long, env = "HERD_CAPABILITIES", value_name = "PATH")]
        capabilities: Option<PathBuf>,
        /// How long one probe may take before it is killed and read as
        /// unreadable, in milliseconds.
        #[arg(long, env = "HERD_PROBE_TIMEOUT", value_name = "MS")]
        probe_timeout: Option<u64>,
    },
    /// Set this machine's switch and leave (E-33).
    ///
    /// `herd-runner switch off --reason "I am gaming"` withdraws the machine:
    /// nothing new is claimed, nothing is served to a peer, and the jobs it
    /// already holds finish. `drain` is the same and then leaves once it
    /// holds nothing. `on` puts it back. Honoured within ten seconds, and it
    /// outlives both this command and the runner process.
    Switch {
        /// One of `on`, `off`, `drain`.
        position: String,
        /// Why. Required for `off` and `drain`: a machine withdrawn for no
        /// stated reason is one nobody can decide to switch back on. For
        /// `on`, accepted and not kept — the position needs none, and
        /// refusing the command for carrying one was a bad trade (D166).
        #[arg(long)]
        reason: Option<String>,
    },
}

fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(Duration::ZERO)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // `herd-runner check --capabilities ...`: reads the file, reads its
    // probes once, prints both and leaves. First of all, before the data root
    // is resolved and before anything dials, because this runs on an
    // operator's machine while she is still writing the file (D165).
    if let Some(Command::Check {
        capabilities,
        probe_timeout,
    }) = &args.command
    {
        let path = capabilities.clone().or_else(|| args.capabilities.clone());
        let Some(path) = path else {
            anyhow::bail!(
                "herd-runner check needs a capability file: --capabilities PATH, or \
                 HERD_CAPABILITIES in the environment"
            );
        };
        let timeout = Duration::from_millis(probe_timeout.unwrap_or(DEFAULT_PROBE_TIMEOUT_MS));
        let (lines, ok) = herd_runner::capability::check(
            &path,
            ProbeSettings {
                interval: Duration::from_millis(DEFAULT_PROBE_INTERVAL_MS),
                timeout,
            },
        )
        .await;
        for line in lines {
            println!("herd-runner: {line}");
        }
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }
    let data_root = match args.data_root {
        Some(path) => path,
        None => DataRoot::default_path().map_err(anyhow::Error::msg)?,
    };
    // Where the owner's switch lives: under the data root, which is the one
    // directory this runner may write in (E-45), unless she says otherwise.
    let switch_file = args
        .switch_file
        .clone()
        .unwrap_or_else(|| herd_runner::switch::default_path(&data_root));
    // `herd-runner switch off --reason ...`: writes the file and leaves. This
    // is E-33's "settable by the owner from her machine" — one command, which
    // is what a desktop shortcut can be (M17), and nothing on this machine
    // listens for it to talk to: with no mesh address configured a runner
    // listens on nothing at all, and the one socket `--mesh-listen` permits
    // serves a job's artefacts to peers rather than taking an instruction.
    // `mesh.rs` holds the rule for which addresses may ever be served from,
    // and this version holds that rule and no endpoint (invariant 3, D10,
    // D11, D222).
    if let Some(Command::Switch { position, reason }) = &args.command {
        let switch = herd_proto::Switch::parse(position, reason.as_deref())
            .map_err(anyhow::Error::msg)
            .context("herd-runner switch")?;
        herd_runner::switch::write(&switch_file, &switch)
            .map_err(anyhow::Error::msg)
            .context("writing the switch")?;
        // `on` carries no reason and no longer refuses one (D166). Said where
        // the owner typed it, so a reason that goes nowhere is at least one
        // somebody read.
        if let (herd_proto::Switch::On, Some(given)) = (
            &switch,
            reason
                .as_deref()
                .map(str::trim)
                .filter(|given| !given.is_empty()),
        ) {
            println!(
                "herd-runner: the reason given is not kept, because the position that carries \
                 one is the withdrawal: {given}"
            );
        }
        println!(
            "herd-runner: the switch in {} is now {}. A runner of this data root honours it \
             within ten seconds, whether or not a hub is up (E-33)",
            switch_file.display(),
            switch.sentence()
        );
        return Ok(());
    }
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
    confinement.output_cap = match args.job_output_cap {
        Some(size) => parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--job-output-cap")?,
        None => DEFAULT_OUTPUT_CAP,
    };
    // Nothing is checked here, and there is no frame-shaped ceiling left for
    // this figure to be past: output too large for one control frame is
    // streamed rather than inlined, so `herd_proto::INLINE_OUTPUT_MAX`
    // (32 MiB) is the threshold between the two carriages and not a cap
    // (D148). What bounds the figure is `--job-disk-budget`, and a cap above
    // that budget is **clamped** to it by `Confined::prepare`, with both
    // figures printed, rather than refused — an unreachable cap says more
    // than it can do, which is no reason a working runner should stop
    // starting (D89's precedent). The help text on `--job-output-cap` above
    // says the same.
    confinement.disk_budget = match args.job_disk_budget {
        Some(size) => parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--job-disk-budget")?,
        None => DEFAULT_DISK_BUDGET,
    };
    confinement.nice = args.job_nice.unwrap_or(DEFAULT_NICE);
    confinement.cpu_weight = args.job_cpu_weight.unwrap_or(DEFAULT_CPU_WEIGHT);
    confinement.cpu_max = match args.job_cpu_max {
        Some(cores) => Some(
            CpuQuota::from_cores(cores)
                .map_err(anyhow::Error::msg)
                .context("--job-cpu-max")?,
        ),
        None => None,
    };
    confinement.io_weight = args.job_io_weight.unwrap_or(DEFAULT_IO_WEIGHT);
    // Absent means absent: no ceiling resolved, no device named, no io.max
    // written. A default throughput would be wrong on every machine but the
    // one it was chosen on.
    confinement.io_max = match &args.job_io_max {
        Some(spec) => Some(
            IoCeiling::parse(spec)
                .map_err(anyhow::Error::msg)
                .context("--job-io-max")?,
        ),
        None => None,
    };
    confinement.pids_max = args.job_pids_max.unwrap_or(DEFAULT_PIDS_MAX);
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

    // Where this machine may serve a job's artefacts from, if anywhere
    // (invariant 3, D10, D11). Judged **here**, before the dial and before
    // anything is announced, because D11 asks for a configuration that would
    // bind a public or default interface to be refused at startup the way a
    // bad data root is — not corrected, not narrowed, and never discovered by
    // a peer finding a port where there should be none.
    //
    // The accepted address is carried into `Config`, where
    // `endpoint::serve_while` binds it — through `mesh::MeshListener` and
    // nowhere else, which is the one module of this crate the confinement
    // guard permits to name a listener.
    let mesh = match (&args.mesh_listen, &args.mesh_advertise) {
        (Some(listen), advertise) => {
            let mesh =
                mesh::Mesh::resolve(listen, advertise.as_deref(), &mesh::HostRouting::read())
                    .map_err(|refused| anyhow::anyhow!("{refused}"))?;
            println!("herd-runner: {}", mesh.said());
            Some(mesh)
        }
        (None, Some(advertise)) => anyhow::bail!(
            "{} {advertise} was given with no {}: an address peers are told to dial, with \
             nothing listening on it, is a promise this runner cannot keep. Name the address \
             to serve from, or leave both out and this machine is outbound-only (D10)",
            mesh::ADVERTISE_FLAG,
            mesh::LISTEN_FLAG
        ),
        (None, None) => {
            // Said out loud, because "this machine serves nothing" is a fact
            // an operator reading the startup report should not have to infer
            // from a line that is not there.
            println!(
                "herd-runner: no mesh address, so this runner is outbound-only: it serves \
                 nothing to a peer and listens on nothing ({} sets one)",
                mesh::LISTEN_FLAG
            );
            None
        }
    };

    // How big this host is. Read from the host, then overridden by whatever
    // its owner said — one figure at a time, so an owner who declares the
    // memory she is lending keeps the thread count the host reports.
    let mut host = herd_runner::metrics::host_size();
    if let Some(size) = args.host_memory {
        let bytes = parse_bytes(&size)
            .map_err(anyhow::Error::msg)
            .context("--host-memory")?;
        let mib = bytes / (1 << 20);
        // Zero is refused rather than announced. A figure of zero is not "no
        // figure": it is a machine of no size, and a hub handed one would
        // derive a `-j` from nothing (the `Some(0)` that caused a real defect
        // in M4). An owner who wants this machine to take no build denies the
        // capability — `--deny-capability build` — which is the honest way to
        // say it and the one a reader can see.
        if mib == 0 {
            anyhow::bail!(
                "--host-memory {size} is {bytes} bytes, which is under a MiB: a host of no \
                 size is not a figure a -j can be derived from. Give the memory Herd's jobs \
                 may use here, or deny the capabilities this machine should not take."
            );
        }
        // An owner may lend less of her machine than it has. She may not lend
        // more: the floor E-29 subtracts would then guard memory that does
        // not exist, and the first job past the real total is the host
        // swapping. Clamped, and the clamp is printed with both figures.
        let (lendable, said) = herd_runner::metrics::lendable_memory_mib(
            mib,
            herd_runner::metrics::total_memory_mib(),
        );
        if let Some(said) = said {
            println!("{said}");
        }
        host.memory_mib = Some(lendable);
    }
    if let Some(threads) = args.host_threads {
        if threads == 0 {
            anyhow::bail!(
                "--host-threads 0 is a machine that can run nothing: deny the capabilities \
                 this machine should not take instead."
            );
        }
        host.threads = Some(threads);
    }
    // And whatever is lent, it is clamped to the room in the cgroup this
    // runner is actually in (D119). A runner inside a slice carrying
    // `MemoryMax` has less room than the machine advertises, and admitting
    // against the machine is how a slice gets OOM-killed while Herd believes
    // it was careful. Applied after the owner's figure and before the floor,
    // so the floor is derived from — and checked against — the room that can
    // really bite. On a host with no enclosing limit this reads nothing and
    // changes nothing: see `room::measure`.
    let (announced, said) =
        herd_runner::room::announced_memory_mib(host.memory_mib, herd_runner::room::measure());
    if let Some(said) = said {
        println!("herd-runner: {said}");
    }
    host.memory_mib = announced;
    // What this host keeps for itself, whatever is queued (E-29). Derived
    // from what is lent when nobody says, so a runner of this version always
    // announces a floor — an absent floor is read by the hub as a figure it
    // was never given, and nothing is admitted against one.
    host.memory_floor_mib = match args.host_memory_floor {
        Some(size) => {
            let bytes = parse_bytes(&size)
                .map_err(anyhow::Error::msg)
                .context("--host-memory-floor")?;
            Some(bytes / (1 << 20))
        }
        None => host.memory_mib.map(herd_proto::default_memory_floor_mib),
    };
    // A floor that leaves nothing is refused where it is written, rather than
    // discovered in the sentence of every job this machine turns away.
    if let (Some(memory), Some(floor)) = (host.memory_mib, host.memory_floor_mib) {
        if floor >= memory {
            anyhow::bail!(
                "--host-memory-floor is {floor} MiB and this host lends {memory} MiB, so \
                 nothing could ever be admitted here. A floor is the room the host keeps to \
                 stay alive, not the whole machine: lower it, or deny the capabilities this \
                 machine should not take."
            );
        }
    }
    // And how much tmpfs a job may reserve here (E-31). Given, measured, or
    // absent — and absent is unknown, never zero.
    let tmpfs_path = args
        .host_tmpfs_path
        .unwrap_or_else(|| herd_runner::metrics::DEFAULT_TMPFS_PATH.to_owned());
    let measured = herd_runner::metrics::tmpfs_room_mib(&tmpfs_path);
    host.tmpfs_mib = match args.host_tmpfs {
        Some(size) => {
            let bytes = parse_bytes(&size)
                .map_err(anyhow::Error::msg)
                .context("--host-tmpfs")?;
            let asked = bytes / (1 << 20);
            match &measured {
                // The same rule as the memory: lend less than the mount has,
                // never more. A reservation nothing backs is a reservation.
                Ok(room) if asked > *room => {
                    println!(
                        "herd-runner: --host-tmpfs asked for {asked} MiB and {tmpfs_path} holds \
                         {room} MiB in all, so {room} MiB is what jobs may reserve here"
                    );
                    Some(*room)
                }
                Ok(_) => Some(asked),
                Err(why) => {
                    println!(
                        "herd-runner: lending {asked} MiB of tmpfs as asked; it was not checked \
                         against the mount, because {why}"
                    );
                    Some(asked)
                }
            }
        }
        None => match &measured {
            Ok(room) => Some(*room),
            Err(why) => {
                // Said out loud, because the consequence is visible in the
                // fleet: this machine takes no job that needs a reservation,
                // and the hub's refusal will name it.
                println!(
                    "herd-runner: this host announces no tmpfs room, so it takes no job that \
                     reserves any: {why}"
                );
                None
            }
        },
    };

    // Printed, because these three figures decide what this machine will and
    // will not take and an operator reading a refusal has to be able to find
    // where they came from (E-04, E-29, E-31).
    println!(
        "herd-runner: admission here is {} of memory less a {} floor, and {} of tmpfs",
        host.memory_mib
            .map_or("an unannounced amount".to_owned(), herd_proto::size_mib),
        host.memory_floor_mib
            .map_or("unannounced".to_owned(), herd_proto::size_mib),
        host.tmpfs_mib.map_or(
            "no announced amount, so no job that reserves any".to_owned(),
            herd_proto::size_mib
        )
    );
    // And the one figure this machine holds against the hub rather than with
    // it: how many jobs it starts at once, derived from the two above and
    // from its own per-job memory cap. Printed here beside them because it is
    // the same arithmetic, and because an operator who sees an attempt handed
    // back for this reason has to be able to find the figure that did it
    // (invariant 2).
    println!(
        "herd-runner: {}",
        herd_runner::jobs_at_once(&host, confinement.memory_max).why()
    );
    // And what this machine keeps *between* jobs, which until now it kept
    // against no figure at all: the pack store's budget and what it holds
    // now. Printed beside the admission lines above because an operator
    // reading one is reading the same question — how much of this machine is
    // lent to Herd — and because the budget decides what a later eviction
    // will take.
    //
    // Taken here, before the dial, and **creating nothing**: the data root is
    // validated inside `herd_runner::run`, and a `packs/` made on this line
    // would leave a directory behind on a root that was about to be refused
    // by name (E-45).
    let pack_budget = match args.pack_budget {
        Some(size) => Some(
            parse_bytes(&size)
                .map_err(anyhow::Error::msg)
                .context(PACK_BUDGET_FLAG)?,
        ),
        None => None,
    };
    println!(
        "herd-runner: {}",
        herd_runner::packstore::take_budget(&confinement.data_root, pack_budget)
    );
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
        host,
        preference: args.preference.unwrap_or(0),
        switch_file: Some(switch_file),
        switch_poll: Duration::from_millis(args.switch_poll.unwrap_or(DEFAULT_POLL_MS)),
        mesh,
        // Not options: the window and the bound on waiting for it are Herd's
        // own figures, and an owner has no way to know what to set them to
        // (D158). They exist on `Config` for the tests that have to exceed
        // them.
        ..Config::default()
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The behaviour a deployment depends on: the figure stated last is the
    /// figure this runner uses (D163).
    ///
    /// The case this exists for is real and was a refusal on the live fleet.
    /// `deploy/nixos-herd-runner.nix` writes `--host-tmpfs 4GiB` on the
    /// ExecStart line, and the operator of that machine — who may not own
    /// this repository — appends `--host-tmpfs 10GiB` through
    /// `services.herd-runner.extraArgs`. Clap's default would refuse that
    /// whole command line with *"cannot be used multiple times"*, which
    /// leaves the figure ours and nobody else's; `args_override_self` makes
    /// the appended one win.
    ///
    /// Read as a unit test on `Args` rather than on a spawned binary,
    /// because what is asserted is the parse and nothing downstream of it:
    /// a repeated flag has to resolve the same way on a host with no tmpfs,
    /// no cgroup delegation and no hub to dial.
    #[test]
    fn a_repeated_option_takes_the_value_stated_last() {
        let args = Args::try_parse_from([
            "herd-runner",
            "--host-tmpfs",
            "4GiB",
            "--host-tmpfs-path",
            "/dev/shm",
            "--host-tmpfs",
            "10GiB",
        ])
        .expect("a repeated option parses rather than refusing the command line");
        assert_eq!(
            args.host_tmpfs.as_deref(),
            Some("10GiB"),
            "the operator's figure is the one appended last"
        );
        assert_eq!(
            args.host_tmpfs_path.as_deref(),
            Some("/dev/shm"),
            "a flag given once keeps its value"
        );

        // Every figure the NixOS module puts on the line, each one stated
        // twice: all seven are options of that module today, and an
        // operator must be able to correct any of them from outside it.
        let args = Args::try_parse_from([
            "herd-runner",
            "--host-tmpfs",
            "4GiB",
            "--host-tmpfs-path",
            "/dev/shm",
            "--job-log-cap",
            "64MiB",
            "--job-disk-budget",
            "20GiB",
            "--job-nice",
            "10",
            "--health-every",
            "15",
            "--drain-grace",
            "3",
            "--host-tmpfs",
            "16GiB",
            "--host-tmpfs-path",
            "/run/shm",
            "--job-log-cap",
            "128MiB",
            "--job-disk-budget",
            "40GiB",
            "--job-nice",
            "5",
            "--health-every",
            "30",
            "--drain-grace",
            "4",
        ])
        .expect("seven repeated options parse");
        assert_eq!(args.host_tmpfs.as_deref(), Some("16GiB"));
        assert_eq!(args.host_tmpfs_path.as_deref(), Some("/run/shm"));
        assert_eq!(args.job_log_cap.as_deref(), Some("128MiB"));
        assert_eq!(args.job_disk_budget.as_deref(), Some("40GiB"));
        assert_eq!(args.job_nice, Some(5));
        assert_eq!(args.health_every, 30.0);
        assert_eq!(args.drain_grace, 4.0);
    }

    /// The other half of the same change, and the one that would be a silent
    /// loss: an option that collects a list still collects every occurrence.
    ///
    /// `args_override_self` replaces a prior occurrence of an argument, and
    /// it is exactly the arguments whose action is to *append* that it does
    /// not touch. A runner is told which privileged helpers it may call and
    /// which capabilities it offers or refuses by repeating one flag, so a
    /// last-wins reading of those would narrow a machine's declaration to
    /// its final line and refuse the rest by name later, on a job.
    #[test]
    fn an_option_that_collects_a_list_keeps_every_occurrence() {
        let args = Args::try_parse_from([
            "herd-runner",
            "--privileged-helper",
            "/sbin/modprobe",
            "--privileged-helper",
            "/run/wrappers/bin/mount",
            "--declare-capability",
            "kvm",
            "--declare-capability",
            "rig-cpu",
            "--deny-capability",
            "docker",
            "--deny-capability",
            "nvidia",
        ])
        .expect("a list option parses repeated");
        assert_eq!(
            args.privileged_helpers,
            ["/sbin/modprobe", "/run/wrappers/bin/mount"],
            "a helper list is a list, not the last path given"
        );
        assert_eq!(args.declare_capability, ["kvm", "rig-cpu"]);
        assert_eq!(args.deny_capability, ["docker", "nvidia"]);
    }

    /// The command itself is still well-formed, including the `switch`
    /// subcommand `args_override_self` propagates to. Clap's own assertions
    /// catch a conflict this change could introduce and they run nowhere
    /// else: nothing in the suite builds this binary's parser.
    #[test]
    fn the_runner_s_command_line_is_well_formed() {
        Args::command().debug_assert();
    }

    /// Everything `herd-runner --help` tells an operator a cap's figure is,
    /// read back **through the constant that decides it**.
    ///
    /// **Why this test exists at all.** `--help` is the first operator
    /// surface anybody reads and it was, until this, the only one in the
    /// repository with no gate of any kind: `herd-proto` has frozen-format
    /// tests, `size.rs` has golden strings, `confine.rs` has compile-time
    /// `const _: () = assert!(…)`, `release.yml` reads the version out of the
    /// source — and the figures in these doc comments were correct by
    /// somebody's attention. Attention is exactly what failed: `v0.3.7`,
    /// `v0.4.0` and `v0.4.1` all shipped a `--job-output-cap` that said *"32
    /// MiB by default, which is also the most this can be"* against a
    /// [`DEFAULT_OUTPUT_CAP`] of 1 GiB — wrong by 32×, inverted about what
    /// the flag does, wrong about the consequence, and caught by a human
    /// reading rather than by a run.
    ///
    /// **Why every figure is interpolated and none is retyped.** A test that
    /// spelled `"1 GiB"` out again would be a fourth copy of the same rule
    /// and would go green on the day the constant moved — which is the defect
    /// it is supposed to catch. So each figure below arrives through the
    /// constant's own formatter, and the words around it are the sentence
    /// being checked.
    ///
    /// **Whitespace is flattened first** because clap wraps help text to a
    /// width this test has no business knowing: a phrase that straddles a
    /// line break is still the same sentence to the person reading it.
    ///
    /// Blocks deliberately **not** asserted here, each for a reason:
    /// `--job-memory-max`, `--job-log-cap`, `--job-disk-budget` and
    /// `--host-tmpfs` carry example *syntax* (`2GiB`, `512MiB`) and state no
    /// default, so there is nothing to disagree with a constant —
    /// `--pack-budget` carries the same example syntax and *is* asserted,
    /// because the figure it states is not its own: it is
    /// [`DEFAULT_DISK_BUDGET`], quoted to tell two budgets apart that an
    /// operator would otherwise confuse, and a quoted figure is exactly the
    /// kind that goes stale where the constant moves;
    /// `--health-every` and the other cadences state no figure in prose at
    /// all — clap renders their `[default: …]` straight out of the constant,
    /// which is already the thing this test is for; `--drain-grace`'s "five
    /// seconds" is invariant 2's promise and **not** [`DRAIN_GRACE`], which
    /// is three, so pairing them would assert a falsehood; and
    /// `--job-io-max`'s "a quarter is a reasonable first try" is advice about
    /// a figure an operator measures, not a figure Herd holds.
    #[test]
    fn the_long_help_states_every_figure_its_constants_decide() {
        let help = flattened_long_help();

        // The fraction a reader is told is kept back is the complement of the
        // share the code hands out, so it is derived and not copied: change
        // `BUILD_SHARE_*` and the word this produces changes with it.
        let kept_back = a_fraction_in_words(
            herd_proto::placement::BUILD_SHARE_DENOMINATOR
                - herd_proto::placement::BUILD_SHARE_NUMERATOR,
            herd_proto::placement::BUILD_SHARE_DENOMINATOR,
        );

        // And the floor's three figures, all read out of
        // `default_memory_floor_mib` by asking it rather than by repeating
        // what it says: the clamp's two ends at the extremes, and the
        // denominator from a host where neither end binds.
        let lowest = herd_proto::default_memory_floor_mib(0);
        let highest = herd_proto::default_memory_floor_mib(u64::MAX / 5);
        // The denominator is **found** rather than assumed: the smallest host
        // size whose floor falls strictly inside the clamp and divides it
        // exactly. Searching for it is what keeps this test from having to
        // know the figure it is about to check.
        let (unclamped_host, unclamped) = (1..=100_000u64)
            .map(|mib| (mib, herd_proto::default_memory_floor_mib(mib)))
            .find(|(mib, floor)| *floor > lowest && *floor < highest && mib % floor == 0)
            .expect(
                "some host size has a default floor strictly inside the clamp that divides \
                 it exactly; if none does, the default stopped being a plain fraction and \
                 the sentence in the help needs reading rather than this test",
            );
        let a_fraction_of_the_memory = a_fraction_in_words(1, unclamped_host / unclamped);

        for (figure, flag) in [
            (
                format!(
                    "{} by default, and the flag",
                    herd_proto::show_bytes(DEFAULT_OUTPUT_CAP)
                ),
                "--job-output-cap's default",
            ),
            (
                format!(
                    "INLINE_OUTPUT_MAX`, {})",
                    herd_proto::show_bytes(herd_proto::INLINE_OUTPUT_MAX)
                ),
                "the frame ceiling --job-output-cap is no longer bounded by",
            ),
            (
                format!(
                    "{} to {} against the 100 an ordinary process has. {} by default",
                    herd_runner::cgroup::WEIGHT_RANGE.start(),
                    herd_runner::cgroup::WEIGHT_RANGE.end(),
                    DEFAULT_CPU_WEIGHT
                ),
                "--job-cpu-weight's range and default",
            ),
            (
                format!(
                    "{} to {} against 100. {} by default",
                    herd_runner::cgroup::WEIGHT_RANGE.start(),
                    herd_runner::cgroup::WEIGHT_RANGE.end(),
                    DEFAULT_IO_WEIGHT
                ),
                "--job-io-weight's range and default",
            ),
            (
                format!("{DEFAULT_PIDS_MAX} by default"),
                "--job-pids-max's default",
            ),
            (
                format!(
                    "not `--job-disk-budget`, which is {} per job by default",
                    herd_proto::show_bytes(DEFAULT_DISK_BUDGET)
                ),
                "--pack-budget's distinction from --job-disk-budget",
            ),
            (
                format!("{kept_back} of whatever it is is kept back"),
                "--host-memory's share kept back from build threads",
            ),
            (
                format!(
                    "{a_fraction_of_the_memory} of the memory, between {} and {}, when not \
                     given",
                    herd_proto::show_bytes(lowest << 20),
                    herd_proto::show_bytes(highest << 20)
                ),
                "--host-memory-floor's default and its two bounds",
            ),
        ] {
            assert!(
                help.to_lowercase().contains(&figure.to_lowercase()),
                "`herd-runner --help` must state {flag} as its constant decides it, and the \
                 rendered help does not contain {figure:?}.\n\
                 \n\
                 **The constant is the authority and the help text is what is wrong.** Do \
                 not change the constant to match the sentence, and do not change this \
                 test: fix the doc comment on the flag. A figure in `--help` is read by an \
                 operator who then sets a cap by it, and a wrong one has shipped in three \
                 releases before now.\n\
                 \n\
                 --- the rendered long help, with its whitespace flattened:\n{help}"
            );
        }
    }

    /// The long help, rendered from the real `Args` and flattened to one
    /// line, because clap's wrap width is not this test's business.
    fn flattened_long_help() -> String {
        Args::command()
            .render_long_help()
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A fraction as `--help` spells it, derived from the two numbers rather
    /// than typed beside them.
    ///
    /// The panic is the useful half: a fraction nobody has a word for here is
    /// one somebody has just changed, and the sentence that mentions it needs
    /// reading before this test can be made to pass.
    fn a_fraction_in_words(numerator: u64, denominator: u64) -> String {
        match (numerator, denominator) {
            (1, 2) => "half".to_owned(),
            (1, 3) => "a third".to_owned(),
            (1, 4) => "a quarter".to_owned(),
            (1, 5) => "a fifth".to_owned(),
            (1, 6) => "a sixth".to_owned(),
            (1, 8) => "an eighth".to_owned(),
            _ => panic!(
                "`--help` spells this fraction in words and this test has no word for \
                 {numerator}/{denominator}. Somebody has changed the share or the floor: \
                 read the sentence that quotes it, then either add the word here or write \
                 the figure in the help"
            ),
        }
    }
}
