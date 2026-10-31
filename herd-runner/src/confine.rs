//! How a job is confined: what the caps are, and what is done to a process
//! between fork and exec to make them hold (E-34, E-45, E-46, E-30).
//!
//! Everything here is configuration made of strings and numbers (invariant
//! 1). There is no notion of a step, a stage or a policy expression: a runner
//! has one data root, five caps and a list of helper paths, and that is the
//! whole surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cgroup::{
    Ceiling, CpuQuota, IoCeiling, IoMax, JobLimits, DEFAULT_CPU_WEIGHT, DEFAULT_IO_WEIGHT,
    DEFAULT_PIDS_MAX, MINIMUM_PIDS_MAX, WEIGHT_RANGE,
};
use crate::privilege::PrivilegedHelpers;
use crate::reclaim::{reclaim_a_past_life, JobTag, LiveJobs, MARKER_VARIABLE};
use crate::root::{DataRoot, Workspace};
use crate::size::show_bytes;

/// The ceiling a job gets unless the machine's owner says otherwise. Two
/// gibibytes is small enough to be safe on the 31 GiB server beside the hub
/// and on a laptop, and a real build that needs more says so in M8's
/// admission rather than by taking the host down.
pub const DEFAULT_MEMORY_MAX: u64 = 2 << 30;

/// The smallest memory ceiling that is a cap rather than a wall.
///
/// `--job-memory-max 0` was refused and `--job-memory-max 1kB` was accepted,
/// and the two do the same thing: refuse every job. Under the fallback
/// (`RLIMIT_AS`, D23) the figure bounds the address space of each process, and
/// on this host `/bin/sh -c 'echo ok'` needs 16 MiB of it: 8 MiB fails in the
/// dynamic loader, 4 MiB fails to allocate at all, 1 MiB segfaults. So 16 MiB
/// is where a ceiling stops being a cap on a job and becomes a guarantee that
/// nothing runs — and a runner that cannot run `/bin/sh` is not a runner.
///
/// Measured rather than guessed, and deliberately generous in the refusal's
/// direction: refusing a ceiling that would work is a startup an operator can
/// argue with, while accepting one that cannot is a runner whose every job
/// fails for a reason nobody looks for.
pub const MINIMUM_MEMORY_MAX: u64 = 16 << 20;

/// An hour. Long enough for a cold build of the first consumer (over thirty
/// minutes), short enough that a wedged job is noticed the same morning.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 3600;

/// Sixty-four mebibytes of log. A build that prints more than that is a build
/// nobody reads the log of, and an unbounded log is a disk filling up on the
/// hub as much as on the runner.
pub const DEFAULT_LOG_CAP: u64 = 64 << 20;

/// Twenty gibibytes per job of workspace. The content store (M11, M12) has
/// its own budget; this is one job's scratch space.
pub const DEFAULT_DISK_BUDGET: u64 = 20 << 30;
/// How much declared output a job may bring home in all: **1 GiB**.
///
/// **This is the whole collection, not the frame.** Up to
/// [`herd_proto::INLINE_OUTPUT_MAX`] it rides the verdict; above that the
/// verdict carries the manifest and the bytes follow (D148). So the flag that
/// sets this is no longer "how much fits on a frame" — that figure is derived
/// and nobody tunes it — but "how much may this machine hand back for one
/// job", which is a real question for whoever owns the machine.
///
/// **1 GiB is chosen, not derived, and the distinction matters** (D138). The
/// consumer states a few hundred megabytes for their screenshot-heavy runs;
/// this is comfortably above that and no arithmetic produced it. The one
/// measurement that would change it is their real distribution of output size
/// per job. Every other figure around it *is* derived —
/// [`herd_proto::INLINE_OUTPUT_MAX`] from base64 against the frame ceiling,
/// [`herd_proto::OUTPUT_CHUNK_BYTES`] from the 15 s cadence — and this one
/// sits here rather than beside them so it is not read as one of them.
///
/// It was 32 MiB, and it was the ceiling: the default was the ceiling and the
/// flag could only lower it, because there was nowhere above a frame to go.
/// There is now, so the flag may raise it, and what bounds it instead is the
/// hub's own per-job acceptance and its output budget (M12).
///
/// **Over it is still a refusal with figures and no file at all** — never a
/// truncation, because a cut-down `results.json` that still parses reads like
/// the truth. What the refusal names has changed: it used to point at M11 as
/// somewhere else to go, and M11 is this, so it names the cap and the flag.
pub const DEFAULT_OUTPUT_CAP: u64 = 1 << 30;

/// Two things about that figure that must stay true, checked by the compiler
/// rather than by a test, because both are relationships between constants
/// and a test would only find out at run time what `cargo build` can refuse.
///
/// **Above the inline threshold**, because the relationship inverted: this
/// used to *be* the frame figure, so the flag could only lower it. It is now
/// above it on purpose — that is what having a second carriage means — and a
/// change that quietly put it back under would take the streamed path out of
/// reach of every default deployment without failing anything else.
///
/// **At or under the default disk budget**, because a job writes `{out}`
/// inside its workspace: a default cap above the default budget would be
/// clamped on every runner nobody had configured, which is a line printed at
/// every startup for a figure Herd chose itself.
const _: () = assert!(DEFAULT_OUTPUT_CAP > herd_proto::INLINE_OUTPUT_MAX);
const _: () = assert!(DEFAULT_OUTPUT_CAP <= DEFAULT_DISK_BUDGET);

/// A job is background work on somebody's machine, so it is nice by default.
/// 10 leaves interactive work ahead of it without starving it.
pub const DEFAULT_NICE: i32 = 10;

/// How often the runner looks at a running job's disk use and memory.
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Every cap and every path a runner confines a job with.
#[derive(Debug, Clone)]
pub struct Confinement {
    /// The one directory this runner may write in (E-45).
    pub data_root: PathBuf,
    /// Kernel memory ceiling per job, in bytes (E-30).
    pub memory_max: u64,
    /// Time cap per job. A job past it ends with a named reason.
    pub timeout: Duration,
    /// Bytes of log a job may produce before it is stopped.
    pub log_cap: u64,
    /// Bytes of declared output a job may leave under `{out}` and have
    /// carried into its record. Over it, nothing comes back and the refusal
    /// names both figures: a cap on an output is **not** a truncation of it
    /// ([`crate::outputs`]).
    pub output_cap: u64,
    /// Bytes a job's workspace may hold before it is stopped.
    pub disk_budget: u64,
    /// CPU niceness added to a job's process. IO niceness goes with it.
    pub nice: i32,
    /// `cpu.weight` in a job's cgroup: the share it gets under contention.
    /// Low by default, so a job yields to everything else on the machine and
    /// still uses an idle one fully.
    pub cpu_weight: u32,
    /// `cpu.max` in a job's cgroup: a hard quota, and none by default.
    pub cpu_max: Option<CpuQuota>,
    /// `io.weight` in a job's cgroup, low by default and inert on a host
    /// whose block layer does not weight IO — which the runner says at
    /// startup rather than leaving to be discovered.
    pub io_weight: u32,
    /// `io.max` in a job's cgroup: a **hard** per-device IO ceiling, and
    /// **none by default**. `io.weight` above shares contended bandwidth;
    /// this caps it. Where one is asked for and this host cannot carry one,
    /// the startup is refused naming the path and the reason — never reduced
    /// to the weight and never written as a zero.
    pub io_max: Option<IoCeiling>,
    /// `pids.max` in a job's cgroup: a fork bomb inside a job must not be
    /// able to take the host down.
    pub pids_max: u64,
    /// A cgroup to hang job cgroups under, instead of the runner's own.
    pub cgroup_root: Option<PathBuf>,
    /// The fixed list of helper commands allowed to need privilege (E-46).
    pub privileged_helpers: Vec<String>,
}

impl Confinement {
    /// The defaults with one data root filled in.
    pub fn with_data_root(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
            memory_max: DEFAULT_MEMORY_MAX,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            log_cap: DEFAULT_LOG_CAP,
            output_cap: DEFAULT_OUTPUT_CAP,
            disk_budget: DEFAULT_DISK_BUDGET,
            nice: DEFAULT_NICE,
            cpu_weight: DEFAULT_CPU_WEIGHT,
            cpu_max: None,
            io_weight: DEFAULT_IO_WEIGHT,
            io_max: None,
            pids_max: DEFAULT_PIDS_MAX,
            cgroup_root: None,
            privileged_helpers: Vec::new(),
        }
    }

    /// The controls a job's cgroup carries, as the cgroup code takes them.
    ///
    /// `io.max` is not among them, and that is the per-device difference: it
    /// is resolved against the device the data root is on and proved against
    /// the subtree at startup, and only then carried
    /// ([`Ceiling::carry_io_max`]).
    pub fn job_limits(&self) -> JobLimits {
        JobLimits {
            memory_max: self.memory_max,
            cpu_weight: self.cpu_weight,
            cpu_max: self.cpu_max,
            io_weight: self.io_weight,
            io_max: None,
            pids_max: self.pids_max,
        }
    }
}

/// The kernel memory ceiling **one job** is held to, with where the figure
/// came from and what this host can actually do about it.
///
/// Until now there was no such thing: `--job-memory-max` was resolved once at
/// startup and the same `JobLimits` was handed to every job, so a job the hub
/// admitted against 8 GiB of declared need ran under the 2 GiB its runner's
/// owner had lent — the runner being the only thing in the product that
/// writes `memory.max` at all. D180 named that and declined to close it by
/// guessing; the frame now carries the figure
/// ([`herd_proto::JobNeed`]), and this is where it becomes a ceiling.
#[derive(Debug, Clone)]
pub struct JobCeiling {
    memory_max: u64,
    need: Option<herd_proto::JobNeed>,
    floored: bool,
}

/// What one job's ceiling is, given this runner's own figure and what the hub
/// costed the job at.
///
/// **Two cases, and the second one is why the flag survives.** A job the hub
/// costed carries both of its figures and is held to their sum plus
/// [`herd_proto::CEILING_MARGIN_PERCENT`] — the ceiling admission weighed it
/// against, and the tmpfs is in it because tmpfs pages are charged to the
/// cgroup that writes them. A job costed at nothing gets `--job-memory-max`
/// exactly as it always has: `herd_hub::JobRecord::memory_need_mib` answers
/// `Ok(None)` for a real class of job — `herdctl submit -- true` — which is
/// admitted on its capabilities alone, so the global figure is the only one
/// anybody has stated about it and it is not going away.
///
/// **Floored at [`MINIMUM_MEMORY_MAX`], which is a measurement.** 16 MiB is
/// where a ceiling stops being a cap on a job and becomes a guarantee that
/// nothing runs: `/bin/sh -c 'echo ok'` needs that much address space on the
/// host that figure was measured on. [`Confined::prepare`] already refuses a
/// *global* ceiling below it, because invariant 2 asks for a cap and not for
/// a wall; a per-job figure below it is the same wall arriving from the hub
/// instead of from a command line, and it gets the same answer. The job is
/// **not** refused for it — a declaration too small to start a shell is the
/// requester's mistake, not work this machine should hand back — and the line
/// below says the floor was what got written.
pub fn job_ceiling(global: u64, need: Option<herd_proto::JobNeed>) -> JobCeiling {
    let asked = match need {
        Some(need) => need.ceiling_bytes(),
        None => global,
    };
    JobCeiling {
        memory_max: asked.max(MINIMUM_MEMORY_MAX),
        need,
        floored: asked < MINIMUM_MEMORY_MAX,
    }
}

impl JobCeiling {
    /// The figure to write, in bytes.
    pub fn memory_max(&self) -> u64 {
        self.memory_max
    }

    /// What this runner says about this job's ceiling, or nothing when there
    /// is nothing new to say.
    ///
    /// **Silence for a job the hub costed at nothing**, and that silence is
    /// deliberate: its ceiling is the one the startup report already named,
    /// and a line per job repeating a figure that has not moved is noise an
    /// operator learns to skip past — which is how the line that matters gets
    /// missed.
    ///
    /// **A host with no cgroup of its own to put the job in says so, per
    /// job.** The fallback is `RLIMIT_AS` on the job's first process: it
    /// bounds one process's *address space*, not the resident memory of the
    /// job's process tree, so a job of several processes holds more than the
    /// figure and an allocation past it fails instead of killing. That is a
    /// weaker thing than the ceiling the hub admitted this job against, and
    /// the honest word for it is **not enforced** — a runner that printed the
    /// figure and left it there would be reporting a ceiling it does not
    /// hold.
    pub fn line(&self, in_a_cgroup_of_its_own: bool, fallback: Option<&str>) -> Option<String> {
        let need = self.need?;
        let floor = if self.floored {
            format!(
                ", floored at {} because a ceiling under that refuses every job",
                show_bytes(MINIMUM_MEMORY_MAX)
            )
        } else {
            String::new()
        };
        Some(if in_a_cgroup_of_its_own {
            format!(
                "declares {}{floor}, and that is the memory.max of its own cgroup on this host",
                need.line()
            )
        } else {
            format!(
                "declares {}{floor}, and this host CANNOT enforce a per-job ceiling: {} What \
                 it gets instead is an address-space limit of {} on its first process, which \
                 bounds neither the job's process tree nor its resident memory — so the \
                 ceiling this job was admitted against is NOT ENFORCED here",
                need.line(),
                fallback.unwrap_or(
                    "this runner has no delegated cgroup v2 subtree to make a job cgroup in."
                ),
                show_bytes(self.memory_max),
            )
        })
    }
}

/// How many jobs this runner holds at once, and the sentence that says why.
///
/// **Derived from what this machine itself declared, and from nothing the hub
/// says.** Invariant 2's first half — *a runner never destabilizes its host* —
/// was until now defended only by the hub's arithmetic: `take_job` refused for
/// a drain and for the owner's switch and for no count at all, so a hub that
/// was wrong, of another build, or not a hub at all sent 250 assignments and
/// got 250 jobs (`herd-hub/tests/fake_hub_flood.rs`). A machine's own bound
/// has to be held on the machine.
///
/// The arithmetic is the one invariant 2 is actually about: every job here
/// runs under the same kernel memory ceiling ([`Confinement::memory_max`], one
/// figure for every job and not a per-job negotiation), so N jobs permit N
/// times that much, and the bound is how many of them fit in the memory this
/// host says it lends after the floor it says it keeps. At 2 GiB a job, a
/// 31 GiB server keeping 6 GiB for itself holds 12; the owner's 17 GiB PC
/// keeping 3 GiB holds 6.
///
/// It is a **worst case**, deliberately: the cap is what a job is permitted,
/// not what it takes, and a bound set on what jobs usually take is a bound
/// that is not there on the day they do not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobsAtOnce {
    most: usize,
    why: String,
}

impl JobsAtOnce {
    /// The most jobs this runner starts at once.
    pub fn most(&self) -> usize {
        self.most
    }

    /// How that figure was arrived at, in the words both the startup report
    /// and the refusal the hub is sent quote.
    pub fn why(&self) -> &str {
        &self.why
    }
}

/// What a machine that will not say how big it is holds at once.
///
/// One. Not a guess at a larger number: the figure exists to bound a host
/// nobody can do arithmetic about, and the only count that is certainly safe
/// on a machine of unknown size is the one invariant 2 would allow on the
/// smallest machine there is. Its owner lifts it by saying how much the
/// machine lends (`--host-memory`), which is the same sentence the hub
/// already needs before it will place anything that costs memory there.
pub const JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY: usize = 1;

/// The clause that marks a sentence as **this runner's own ceiling**, and the
/// one thing that tells a runner-origin refusal apart from the hub's.
///
/// Attribution, not decoration. A refusal for this ceiling travels verbatim:
/// [`JobsAtOnce::why`] goes into `ToHub::AttemptAbandoned`'s `reason`, and
/// `end_attempt` writes it unchanged into the record's `attempt_ended` line.
/// Whoever reads that record later — an operator, or
/// `herd-hub/tests/announced_job_ceiling.rs`'s oracle — has nothing but the
/// words to decide whether the attempt was spent on the runner's ceiling or
/// on a timeout, a cancel or a runner that went away.
///
/// The hub has a ceiling sentence of its own,
/// `herd_hub::matcher::A_CEILING_THE_RUNNER_ANNOUNCED` — *"it said it holds
/// at most N at once"* — which it writes on a `waiting` line when it holds a
/// job back. The two used to share the words *"holds at most"*, so a test
/// matching on that substring could not say which side had spoken. **Neither
/// of these two clauses is a substring of the other, and neither sentence
/// contains the other's clause**, which is what makes the attribution
/// decidable; `herd-hub/tests/announced_job_ceiling.rs` asserts it against
/// both real sentences rather than trusting this paragraph.
///
/// Every branch of [`jobs_at_once`] carries it, the silent host included: a
/// runner that refuses because it will not say how big it is is refusing for
/// its ceiling exactly as much as one that did the arithmetic.
pub const A_RUNNERS_OWN_CEILING: &str = "this runner holds at most";

/// How many jobs a runner of this size, capping every job at `memory_max`,
/// holds at once.
///
/// The floor is the host's own when it names one and
/// [`herd_proto::default_memory_floor_mib`] when it does not — the same
/// default the runner binary fills in before it dials, so the figure a runner
/// enforces is the figure it announced. A host that will not say how much
/// memory it lends gets [`JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY`], because an
/// unreadable figure is not read as plenty (D23's rule) any more than it is
/// read as zero.
///
/// Never zero: [`Confined::prepare`] already refuses a memory ceiling that
/// would refuse every job, on the ground that invariant 2 asks for a cap and
/// not for a wall, and a count of zero is that same wall one level up. A
/// machine too small to hold one job at its own ceiling holds one anyway, and
/// the sentence says what the arithmetic was.
pub fn jobs_at_once(host: &herd_proto::HostSize, memory_max: u64) -> JobsAtOnce {
    let per_job_mib = (memory_max / (1 << 20)).max(1);
    let Some(lent) = host.memory_mib else {
        return JobsAtOnce {
            most: JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY,
            why: format!(
                "this machine does not say how much memory it lends, so how many jobs of {} \
                 fit on it cannot be derived: {A_RUNNERS_OWN_CEILING} \
                 {JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY} job(s) at once until --host-memory \
                 says otherwise",
                show_bytes(memory_max)
            ),
        };
    };
    let floor = host
        .memory_floor_mib
        .unwrap_or_else(|| herd_proto::default_memory_floor_mib(lent));
    let admissible = lent.saturating_sub(floor);
    let most = usize::try_from((admissible / per_job_mib).max(1)).unwrap_or(usize::MAX);
    JobsAtOnce {
        most,
        why: format!(
            "this machine lends {} and keeps {} as its own floor, and caps every job at {}, \
             so {A_RUNNERS_OWN_CEILING} {most} job(s) at once",
            herd_proto::size_mib(lent),
            herd_proto::size_mib(floor),
            show_bytes(memory_max)
        ),
    }
}

/// A confinement that has been checked against this host: the data root is
/// usable, this process is unprivileged, the helper list is well formed, and
/// the memory ceiling is either a cgroup subtree or a named fallback.
///
/// Building one is the runner's whole startup refusal path, so a runner that
/// has joined a hub is a runner that can already confine a job.
#[derive(Debug)]
pub struct Confined {
    root: DataRoot,
    ceiling: Ceiling,
    helpers: PrivilegedHelpers,
    confinement: Confinement,
    live: LiveJobs,
    /// What the startup reaper found of a previous life, for the report.
    reclaimed: Vec<String>,
    /// What the notebook of things this runner could NOT reclaim still names,
    /// and what came off it because somebody cleared it ([`crate::janitor`]).
    ///
    /// Beside `reclaimed` and never inside it: one is the reaper saying what
    /// it took away, the other is a list of what is still on this host, and a
    /// report that ran the two sentences together would read as the second
    /// having been dealt with too.
    left_behind: Vec<String>,
    /// Whether the `io.weight` a job carries reaches the device that job
    /// works on, and why either way. A bound that is written and ignored
    /// must be said out loud, not assumed — and so must one that does bite,
    /// because the answer is about a device and not about this host (D120).
    io_weight_reach: crate::blockio::IoWeightReach,
}

impl Confined {
    /// Check everything, or refuse naming what is wrong.
    pub fn prepare(mut confinement: Confinement) -> Result<Self, String> {
        crate::privilege::refuse_if_privileged()?;
        let root = DataRoot::open(&confinement.data_root)?;
        let helpers = PrivilegedHelpers::new(confinement.privileged_helpers.clone())?;
        if confinement.memory_max == 0 {
            return Err(
                "a memory ceiling of 0 bytes would refuse every job; invariant 2 \
                        asks for a cap, not for a wall"
                    .to_owned(),
            );
        }
        // And neither is a ceiling no process could live under. `0` was
        // refused while `1kB` was accepted, and both refuse every job.
        if confinement.memory_max < MINIMUM_MEMORY_MAX {
            return Err(format!(
                "a memory ceiling of {} would refuse every job, exactly as 0 does: \
                 a shell alone needs {} of address space. The floor is {}; invariant 2 \
                 asks for a cap, not for a wall.",
                show_bytes(confinement.memory_max),
                show_bytes(MINIMUM_MEMORY_MAX),
                show_bytes(MINIMUM_MEMORY_MAX)
            ));
        }
        if confinement.timeout.is_zero() {
            return Err("a time cap of 0 s would stop every job before it ran".to_owned());
        }
        // An output cap a job could never reach is **clamped, loudly**.
        //
        // This check used to be "a cap bigger than one frame", and it was a
        // refusal: a collection travelled inside the verdict, so a larger cap
        // would have produced a frame nothing could decode — D128's redial
        // loop, which is worth refusing a startup over. The streamed carriage
        // removed that ceiling (D148), so the frame no longer bounds the
        // flag. What does is the disk: a job writes `{out}` inside its
        // workspace, and the workspace stops at `--job-disk-budget`, so an
        // output cap above that budget can never bind.
        //
        // **Clamped rather than refused**, which is D89's precedent for
        // exactly this shape — a memory floor larger than the machine's real
        // memory guards nothing and is clamped where it is written, not made
        // a reason the runner will not start. The two cases differ in what is
        // at stake: an undecodable frame destroys a verdict, while an
        // unreachable cap merely says more than it can do. Lowering a disk
        // budget must not stop a runner that was working.
        if confinement.output_cap > confinement.disk_budget {
            println!(
                "herd-runner: --job-output-cap {} is more than --job-disk-budget {}, so it \
                 could never bind — a job writes its output inside its workspace and the \
                 workspace stops at the disk budget first. Using {} as the output cap. Raise \
                 the disk budget if a job is meant to hand back more than that.",
                show_bytes(confinement.output_cap),
                show_bytes(confinement.disk_budget),
                show_bytes(confinement.disk_budget)
            );
            confinement.output_cap = confinement.disk_budget;
        }
        // The weights are the kernel's own range, checked here so that a
        // figure outside it is one refusal at startup rather than a write the
        // kernel turns down once per job, silently, forever.
        for (option, weight) in [
            ("--job-cpu-weight", confinement.cpu_weight),
            ("--job-io-weight", confinement.io_weight),
        ] {
            if !WEIGHT_RANGE.contains(&weight) {
                return Err(format!(
                    "{option} {weight} is outside what the kernel accepts for a cgroup \
                     weight ({}..={}). A weight is a share against the 100 an ordinary \
                     process has, not a percentage and not a cap.",
                    WEIGHT_RANGE.start(),
                    WEIGHT_RANGE.end()
                ));
            }
        }
        // And a pid ceiling, which is a cap and not a wall for the same
        // reason the memory ceiling has a floor.
        if confinement.pids_max < MINIMUM_PIDS_MAX {
            return Err(format!(
                "--job-pids-max {} would refuse every job: a job is at least a shell, the \
                 command it runs and a child or two of that command. The floor is {}.",
                confinement.pids_max, MINIMUM_PIDS_MAX
            ));
        }
        let mut ceiling =
            Ceiling::detect(confinement.job_limits(), confinement.cgroup_root.as_deref());
        // D23's loud fallback is for a host that cannot give the real ceiling.
        // An operator who named `--cgroup-root` asked for a specific subtree:
        // falling back there would answer a precise request with something
        // else, so it is a refusal at startup naming what the subtree lacks.
        if let (Some(asked), Some(why)) = (&confinement.cgroup_root, ceiling.fallback_reason()) {
            return Err(format!(
                "--cgroup-root {} cannot be used: {why}. Remove the option to let this \
                 runner use its own cgroup, or make that subtree usable.",
                asked.display()
            ));
        }
        // `cpu.max` is the one control that is absent unless an operator
        // declares it, which makes asking for it a precise request for a hard
        // ceiling — the same kind of request `--cgroup-root` is. A runner that
        // took the option and enforced no quota would be a machine lending a
        // guarantee it does not have, so it is a refusal naming what is
        // missing. The three controls that have defaults take the other road:
        // a loud line in the startup report, because refusing there would
        // mean a runner that will not start on an ordinary host.
        if confinement.cpu_max.is_some() {
            if let Some(why) = ceiling.cannot_enforce("cpu") {
                return Err(format!(
                    "--job-cpu-max asks for a hard CPU quota and this host cannot enforce \
                     one: {why}. Run this runner as a systemd unit with Delegate=yes, or \
                     inside systemd-run --user --scope -p Delegate=yes, or leave the \
                     option out — without it a job's CPU is bounded by a weight and by \
                     niceness, which need nothing granted."
                ));
            }
        }
        // `io.max` takes the same road as `cpu.max`, one step further: a hard
        // ceiling an operator declared, so a host that cannot carry one is a
        // refusal and never a warning. It goes further because the knob is
        // **per-device**: a value with no device is not a weak ceiling, it is
        // not a ceiling, so the device under the data root is resolved first
        // and a path whose device cannot be resolved — ZFS above all, where
        // no cgroup IO knob reaches at any value — is itself the refusal.
        //
        // Deliberately unlike `io.weight`, which is written and reported
        // inert on such a host: a weight nobody honours costs nothing, and a
        // ceiling nobody honours is a false promise on the one machine the
        // first consumer's builds freeze (D116, D120, D129).
        if let Some(asked) = confinement.io_max {
            let device = crate::blockio::ceiling_device(root.path()).map_err(|why| {
                format!(
                    "--job-io-max asks for a hard IO ceiling and no ceiling can be written \
                     for {}: {why}. Leave the option out for no ceiling at all, which is \
                     the default — a job's IO is then shaped by io.weight, which shares \
                     contended bandwidth and caps nothing.",
                    root.path().display()
                )
            })?;
            let io_max = IoMax {
                ceiling: asked,
                device: device.device,
            };
            ceiling.prove_io_max(&io_max).map_err(|why| {
                format!(
                    "--job-io-max {} asks for a hard IO ceiling on device {} ({} from {}, \
                     which is where {} is), and this host would not take it: {why}. Run this \
                     runner as a systemd unit with Delegate=yes, or inside systemd-run \
                     --user --scope -p Delegate=yes, or leave the option out — a ceiling \
                     this host cannot enforce must not be announced as one.",
                    asked.describe(),
                    io_max.device,
                    device.mount_point,
                    device.source,
                    root.path().display()
                )
            })?;
            ceiling.carry_io_max(io_max);
        }
        // Asked once, at startup, because the answer is about this host's
        // block layer and cannot change under a running runner without
        // somebody changing a scheduler by hand.
        let io_weight_reach = crate::blockio::io_weight_reach(root.path());
        // Before the first job and before the hub: a runner that was killed
        // left processes, cgroups and workspaces nothing else will collect.
        // Not its output spools — those are the only copy of a result until
        // the hub acknowledges them (D19) and `Session::pick_up_collections`
        // offers them again a few lines after this returns.
        let reclaimed = reclaim_a_past_life(&root, &ceiling);
        // After the reaper, because the reaper is what puts things on this
        // list, and a reconciliation that ran first would have reported as
        // still-here a workspace this start had just managed to remove. One
        // `lstat` per entry and no walk: this is the startup path.
        let left_behind = crate::janitor::reconcile(&root);
        Ok(Self {
            root,
            ceiling,
            helpers,
            confinement,
            live: LiveJobs::default(),
            reclaimed,
            left_behind,
            io_weight_reach,
        })
    }

    /// The lines a runner prints once at startup, so that what it will and
    /// will not enforce is visible before the first job rather than inferred
    /// from a job that behaved oddly.
    pub fn report(&self) -> Vec<String> {
        let c = &self.confinement;
        let mut lines = vec![
            format!("data root: {}", self.root.path().display()),
            format!(
                "caps per job: {} memory, {} s, {} of log, {} of declared output, \
                 {} of disk, nice {}",
                show_bytes(c.memory_max),
                c.timeout.as_secs(),
                show_bytes(c.log_cap),
                show_bytes(c.output_cap),
                show_bytes(c.disk_budget),
                c.nice
            ),
            format!(
                "cgroup controls per job: cpu.weight {}, {}, io.weight {}, pids.max {}",
                c.cpu_weight,
                match c.cpu_max {
                    Some(quota) => format!(
                        "cpu.max {:.2} cores ({})",
                        quota.cores(),
                        quota.control_value()
                    ),
                    // Said rather than left out: an absent hard ceiling is a
                    // choice (a quota wastes an idle machine), and an
                    // operator reading this has to see that it was a choice.
                    None => "no cpu.max, so no hard CPU quota — declare one with \
                             --job-cpu-max if this machine must never lend more"
                        .to_owned(),
                },
                c.io_weight,
                c.pids_max
            ),
        ];
        // A control the subtree withholds, in the fallback's own voice: there
        // is nothing to fall back to for a CPU share, so the only honest
        // thing is to say it is not there.
        for missing in self.ceiling.unenforced() {
            lines.push(format!(
                "{}: NOT ENFORCED on this host, because {}. A job's only CPU and IO \
                 courtesy there is nice {} and the idle IO class.",
                missing.controls, missing.why, c.nice
            ));
        }
        if self.ceiling.fallback_reason().is_some() {
            lines.push(format!(
                "cpu.weight, cpu.max, io.weight and pids.max: NOT ENFORCED AT ALL on this \
                 host: there is no cgroup for a job to carry them. A job here is bounded \
                 by nice {}, the idle IO class and an address-space limit, and nothing \
                 bounds how many processes it forks.",
                c.nice
            ));
        } else if let Some(why) = self.io_weight_reach.inert_reason() {
            // Written and ignored is the failure mode D23 exists to forbid,
            // and the only one of the four that the cgroup core cannot tell
            // us about: the write succeeds whatever the block layer does.
            lines.push(format!(
                "io.weight {}: WRITTEN BUT INERT for jobs here, because {}. An absolute \
                 io.max is the only IO lever that works whatever the scheduler is, and \
                 this runner {}",
                c.io_weight,
                why,
                // Which it is depends on what the operator declared, and
                // saying "this runner sets none" while a ceiling is written
                // would be the same kind of stale claim this whole line
                // exists to prevent. Measured: on this project's SATA disk
                // blk-iocost knows no cost model, so the weight is inert
                // there and the ceiling still holds — which is exactly the
                // difference between the two knobs.
                match self.ceiling.limits().io_max.as_ref() {
                    Some(io_max) => format!(
                        "sets one, on device {}: that ceiling is this host's only effective \
                         IO bound for a job, and the weight beside it does nothing here.",
                        io_max.device
                    ),
                    None => "sets none: a job's IO here is bounded by the idle IO class \
                             alone, which a scheduler of none and a ZFS dataset both ignore."
                        .to_owned(),
                }
            ));
        } else {
            // And the good news is said too, with the device in it. D116
            // called this host's NVMe inert on the evidence of the *host's*
            // schedulers (D120): an operator has to be able to read which
            // device the answer is about, because moving the data root moves
            // the answer.
            lines.push(format!(
                "io.weight {}: enforced for jobs here, because {}.",
                c.io_weight, self.io_weight_reach.why
            ));
        }
        // And the hard ceiling beside the share, said in both states. The
        // distinction is the whole point of the line: a weight shares
        // contended bandwidth and a ceiling caps it, so a reader who has only
        // seen the weight has not been told whether this machine can be
        // saturated by one job.
        match self.ceiling.limits().io_max.as_ref() {
            Some(io_max) => lines.push(format!(
                "io.max for device {} ({}): {} — a hard per-second ceiling the kernel \
                 enforces whatever the IO scheduler is, proved at startup by writing it. \
                 It bounds what a job writes under the data root; a job writing an \
                 absolute path outside the data root writes to another device, which \
                 neither this ceiling nor io.weight reaches.",
                io_max.device,
                self.root.path().display(),
                io_max.ceiling.describe()
            )),
            None => lines.push(
                "no io.max, so no hard IO ceiling: io.weight is a share of contended \
                 bandwidth and caps nothing, so one job alone can still saturate this \
                 disk. Declare a ceiling with --job-io-max rbps=…,wbps=…,riops=…,wiops=… \
                 if that matters here; there is no default, because a throughput figure \
                 is right on one machine only."
                    .to_owned(),
            ),
        }
        match self.ceiling.fallback_reason() {
            None => lines.push(
                "memory ceiling: one cgroup v2 subtree per job, enforced by the kernel".to_owned(),
            ),
            Some(why) => {
                lines.push(format!(
                    "memory ceiling: FALLING BACK to a per-process address-space limit, \
                     which bounds each process instead of the job's tree and makes an \
                     allocation fail instead of killing the job. Reason: {why}"
                ));
                // The third weakness, and the one that matters most: without a
                // cgroup there is no handle on the job's whole tree, so the
                // kill is a process-group kill plus a sweep of /proc for this
                // job's marker. It reaches a double fork, a nohup and a
                // setsid; it does not reach a process that scrubs its own
                // environment.
                lines.push(
                    "kill without a cgroup: FALLING BACK to killing the job's process \
                     group and then sweeping /proc for processes still carrying this \
                     job's marker. That reaches a job that gave itself a new session; \
                     it cannot reach one that erases its own environment, which a \
                     cgroup kill would still catch."
                        .to_owned(),
                );
            }
        }
        // And the figure that bounds nothing, said beside the ones that do so
        // that nobody has to guess which it is. E-32 is the first consumer's
        // own expectation that a load figure is recorded and never a
        // criterion, and they asked for pressure to brake admission; this
        // line is the refusal, in the open (D119).
        match crate::metrics::pressure(None) {
            Some(_) => lines.push(
                "pressure: read from /proc/pressure, and from each job's own cgroup where \
                 the kernel exposes it, recorded with every job's outcome — and a gate on \
                 nothing. No job is ever refused or delayed here because this host is \
                 stalled (E-32)."
                    .to_owned(),
            ),
            // Absent, never zero: a row of zeroes would read as a machine
            // under no pressure at all, which is the opposite of what an
            // unreadable file says.
            None => lines.push(
                "pressure: NOT READABLE on this host, so every job's record here leaves the \
                 figure absent rather than reporting a zero — this kernel has no \
                 /proc/pressure (CONFIG_PSI). Nothing gated on it in any case (E-32)."
                    .to_owned(),
            ),
        }
        if self.helpers.allowed().is_empty() {
            lines.push("privileged helpers: none configured".to_owned());
        } else {
            lines.push(format!(
                "privileged helpers: {}",
                self.helpers.allowed().join(", ")
            ));
        }
        // E-46 is about more than this process's uid: a user who can reach a
        // container runtime, or who is in a group that can become root, is
        // root-equivalent whatever its uid says. Said as a warning, never a
        // refusal: whether a machine's own owner accepts that is his call.
        for warning in crate::privilege::root_equivalent_warnings() {
            lines.push(format!("WARNING: {warning}"));
        }
        for reclaimed in &self.reclaimed {
            lines.push(format!("reaped at startup: {reclaimed}"));
        }
        // Last, and in its own voice. Everything above is what this runner
        // will and will not do; this is what is on the machine's disk right
        // now that no runner will ever take away, and the owner is the only
        // reader who can do anything about it.
        for left in &self.left_behind {
            lines.push(format!("left behind: {left}"));
        }
        lines
    }

    /// The jobs running right now, so that a runner being stopped can end
    /// them rather than orphan them.
    pub fn live(&self) -> &LiveJobs {
        &self.live
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    pub fn ceiling(&self) -> &Ceiling {
        &self.ceiling
    }

    /// Why `io.weight` cannot bite for a job here, where it cannot. A test
    /// asks this to know which half of the report it may assert.
    pub fn io_weight_inert(&self) -> Option<&str> {
        self.io_weight_reach.inert_reason()
    }

    /// The whole answer, including the one that says the weight does bite.
    pub fn io_weight_reach(&self) -> &crate::blockio::IoWeightReach {
        &self.io_weight_reach
    }

    pub fn helpers(&self) -> &PrivilegedHelpers {
        &self.helpers
    }

    pub fn caps(&self) -> &Confinement {
        &self.confinement
    }
}

/// The environment a job is given.
///
/// Built from nothing rather than inherited: the runner's own environment may
/// hold a token, a `TMPDIR` pointing at the host's disk, or a `HOME` full of
/// credentials, and none of that is a job's business. `PATH` is carried
/// through because a command needs to find its tools, and the consumer's real
/// environment arrives in M10 inside `nix develop`.
///
/// One variable is Herd's own: the job's marker. Because the environment is
/// built rather than inherited, the marker is carried by exactly the job's
/// processes and by nothing else on the host, which is what lets the runner
/// find a process that left the job's process group (see [`crate::reclaim`]).
pub fn job_environment(workspace: &Workspace, tag: &JobTag) -> Vec<(String, String)> {
    let mut environment = vec![
        (MARKER_VARIABLE.to_owned(), tag.as_str().to_owned()),
        ("HOME".to_owned(), path_string(workspace.home_dir())),
        ("TMPDIR".to_owned(), path_string(workspace.tmp_dir())),
        ("TMP".to_owned(), path_string(workspace.tmp_dir())),
        ("TEMP".to_owned(), path_string(workspace.tmp_dir())),
        ("PWD".to_owned(), path_string(workspace.work_dir())),
        // XDG directories are where tools put caches and state when they are
        // well behaved. Pointed inside the workspace, so that being well
        // behaved lands inside the data root.
        (
            "XDG_CACHE_HOME".to_owned(),
            path_string(&workspace.home_dir().join(".cache")),
        ),
        (
            "XDG_CONFIG_HOME".to_owned(),
            path_string(&workspace.home_dir().join(".config")),
        ),
        (
            "XDG_DATA_HOME".to_owned(),
            path_string(&workspace.home_dir().join(".local/share")),
        ),
        (
            "XDG_STATE_HOME".to_owned(),
            path_string(&workspace.home_dir().join(".local/state")),
        ),
        (
            "XDG_RUNTIME_DIR".to_owned(),
            path_string(workspace.tmp_dir()),
        ),
    ];
    for carried in ["PATH", "LANG", "LC_ALL", "TZ", "TERM"] {
        if let Ok(value) = std::env::var(carried) {
            environment.push((carried.to_owned(), value));
        }
    }
    environment
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// What the job's own process does to itself between fork and exec.
///
/// This runs in the child, after `fork` and before `exec`, where almost
/// nothing is allowed: no allocation, no lock, no call into the runtime. So
/// it holds only numbers and one already-open descriptor, and does only
/// syscalls.
#[derive(Debug, Clone, Copy)]
pub struct ChildSetup {
    /// `cgroup.procs` of the job's cgroup, already open, or `-1`.
    pub cgroup_procs_fd: i32,
    /// CPU niceness to add.
    pub nice: i32,
    /// Address-space limit in bytes, used only where there is no cgroup.
    pub address_space_max: Option<u64>,
}

#[cfg(unix)]
impl ChildSetup {
    /// Apply everything, in the order that matters.
    ///
    /// # Safety
    ///
    /// Called between fork and exec in a single-threaded child. Every call
    /// below is a syscall with no allocation and no locking.
    pub unsafe fn apply(&self) -> std::io::Result<()> {
        // 1. Its own session, and therefore its own process group. Every
        //    child the job spawns inherits it, which is what makes "kill the
        //    job" mean "kill the job and everything it started" even where
        //    there is no cgroup. It also detaches the job from the runner's
        //    controlling terminal, so Ctrl-C on the runner does not reach it
        //    by accident and a drain stays orderly.
        if libc::setsid() == -1 {
            // Already a process group leader: a group of its own is what
            // matters, and that is then already true.
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }

        // 2. Into the job's cgroup, before a single byte is allocated by the
        //    program that is about to be exec'd. Writing "0" means "me",
        //    which is the only form that needs no rights over anyone else's
        //    cgroup. A failure here is fatal: a job outside its cgroup is a
        //    job with no ceiling, and that must be a refusal to start.
        if self.cgroup_procs_fd >= 0 {
            let me = b"0";
            if libc::write(self.cgroup_procs_fd, me.as_ptr().cast(), 1) != 1 {
                return Err(std::io::Error::last_os_error());
            }
        }

        // 3. Nice on CPU, and the idle class on IO. Both are a job being a
        //    guest on someone's machine. Lowering one's own IO priority to
        //    the idle class needs no privilege.
        if self.nice != 0 {
            // The configured value is the niceness the job ends up with, not
            // an increment: a runner that is itself niced must not multiply
            // it. And it is a floor, never a ceiling — an unprivileged
            // process cannot lower its niceness, and a runner started niced
            // beyond this is a choice of its owner to respect.
            *libc::__errno_location() = 0;
            let current = libc::getpriority(libc::PRIO_PROCESS, 0);
            let current = if *libc::__errno_location() == 0 {
                current
            } else {
                0
            };
            if self.nice > current && libc::setpriority(libc::PRIO_PROCESS, 0, self.nice) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // IOPRIO_WHO_PROCESS = 1, and a priority is (class << 13) | data,
            // with class 3 = idle.
            const IOPRIO_WHO_PROCESS: libc::c_int = 1;
            const IOPRIO_CLASS_IDLE: libc::c_int = 3;
            let priority = IOPRIO_CLASS_IDLE << 13;
            // A kernel without ioprio_set is not a reason to refuse the job;
            // the CPU niceness above is the part that matters.
            let _ = libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS as libc::c_long,
                0 as libc::c_long,
                priority as libc::c_long,
            );
        }

        // 4. No core dumps. A job that segfaults with a 2 GiB ceiling would
        //    otherwise write a 2 GiB core into its workspace and spend the
        //    disk budget on it.
        let no_core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let _ = libc::setrlimit(libc::RLIMIT_CORE, &no_core);

        // 5. The fallback ceiling, where there is no cgroup. RLIMIT_AS bounds
        //    each process rather than the job's tree, and makes an allocation
        //    fail rather than killing — which is why it is the fallback and
        //    why the runner says so out loud.
        if let Some(bytes) = self.address_space_max {
            let limit = libc::rlimit {
                rlim_cur: bytes as libc::rlim_t,
                rlim_max: bytes as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

/// Kill a whole process group, children and grandchildren with it.
///
/// Used where there is no cgroup, and as a second pass where there is: a
/// process that set up its own session escapes the group but not the cgroup,
/// and a cgroup that could not be made leaves only the group.
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // Safe: kill on a negative pid signals the process group, and a group
    // that is already empty is an ESRCH we have nothing to do about.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub fn kill_process_group(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use herd_proto::HostSize;

    // ------------------- how many jobs this machine holds at once ---------

    /// The two machines the first consumer actually has, at the shipped
    /// per-job ceiling. 31 GiB less a 6 GiB floor is 25 GiB, which is twelve
    /// jobs of 2 GiB; 17 GiB less 3 GiB is 14 GiB, which is seven.
    #[test]
    fn the_ceiling_is_how_many_per_job_caps_fit_in_what_the_host_lends() {
        let server = HostSize::new(Some(31 * 1024), Some(12)).with_floor(Some(6 * 1024));
        assert_eq!(jobs_at_once(&server, DEFAULT_MEMORY_MAX).most(), 12);
        let pc = HostSize::new(Some(17 * 1024), Some(8)).with_floor(Some(3 * 1024));
        assert_eq!(jobs_at_once(&pc, DEFAULT_MEMORY_MAX).most(), 7);
    }

    /// A host that names no floor is not a host with no floor: the figure
    /// the runner binary would have filled in is the figure the arithmetic
    /// uses, so what a runner enforces is what it announces.
    #[test]
    fn a_host_that_names_no_floor_keeps_the_default_one_back() {
        let silent = HostSize::new(Some(31 * 1024), Some(12));
        let named = HostSize::new(Some(31 * 1024), Some(12))
            .with_floor(Some(herd_proto::default_memory_floor_mib(31 * 1024)));
        assert_eq!(
            jobs_at_once(&silent, DEFAULT_MEMORY_MAX),
            jobs_at_once(&named, DEFAULT_MEMORY_MAX)
        );
    }

    /// D23's rule, applied to a count: a figure that cannot be read is not
    /// read as plenty. A machine that will not say how big it is runs one job
    /// at a time and the sentence says what would lift it.
    #[test]
    fn a_host_that_will_not_say_how_big_it_is_holds_one_job() {
        let silent = HostSize::default();
        let ceiling = jobs_at_once(&silent, DEFAULT_MEMORY_MAX);
        assert_eq!(ceiling.most(), JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY);
        assert!(
            ceiling.why().contains("--host-memory"),
            "the sentence has to name the flag that lifts it: {}",
            ceiling.why()
        );
    }

    /// Invariant 2 asks for a cap, not for a wall — the sentence
    /// [`Confined::prepare`] refuses a zero memory ceiling with. A machine
    /// too small to hold one job at its own ceiling holds one anyway.
    #[test]
    fn a_machine_too_small_for_one_job_still_holds_one() {
        let tiny = HostSize::new(Some(2048), Some(2)).with_floor(Some(2048));
        assert_eq!(jobs_at_once(&tiny, DEFAULT_MEMORY_MAX).most(), 1);
        let inverted = HostSize::new(Some(1024), Some(1)).with_floor(Some(8192));
        assert_eq!(
            jobs_at_once(&inverted, DEFAULT_MEMORY_MAX).most(),
            1,
            "a floor past the memory lent must not wrap round"
        );
    }

    /// The owner's three knobs are the whole of the control surface: a
    /// smaller per-job cap on the same machine holds proportionally more.
    #[test]
    fn a_smaller_per_job_cap_holds_more_of_them() {
        let host = HostSize::new(Some(31 * 1024), Some(12)).with_floor(Some(6 * 1024));
        assert_eq!(jobs_at_once(&host, 512 << 20).most(), 50);
        assert!(
            jobs_at_once(&host, 512 << 20).why().contains("512 MiB"),
            "the sentence quotes the cap it divided by: {}",
            jobs_at_once(&host, 512 << 20).why()
        );
    }

    #[test]
    fn a_prepared_confinement_reports_its_caps_and_its_root() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-report");
        let confined = Confined::prepare(Confinement::with_data_root(temp.path()))
            .expect("an unprivileged process with a writable root can confine");
        let report = confined.report().join("\n");
        assert!(
            report.contains(&temp.path().display().to_string()),
            "{report}"
        );
        assert!(report.contains("2 GiB memory"), "{report}");
        assert!(report.contains("3600 s"), "{report}");
        assert!(report.contains("64 MiB of log"), "{report}");
        assert!(report.contains("nice 10"), "{report}");
        assert!(report.contains("none configured"), "{report}");
        // The four cgroup controls, with the one that is absent by default
        // said rather than left out: an absent hard quota is a decision.
        assert!(
            report.contains("cpu.weight 10") && report.contains("io.weight 10"),
            "{report}"
        );
        assert!(report.contains("pids.max 1024"), "{report}");
        assert!(report.contains("no cpu.max"), "{report}");
        // Either the real ceiling or the fallback, never silence about which.
        assert!(
            report.contains("memory ceiling:"),
            "the report must say how the ceiling is enforced: {report}"
        );
    }

    /// A fallback has to be impossible to miss in the report, because the
    /// report is the only place an operator sees it before a job misbehaves.
    ///
    /// Only a host that really falls back can be asked this, and a host that
    /// enforces the real ceiling says so loudly instead — which is the
    /// assertion in `the_runner_says_which_memory_ceiling_it_enforces`.
    #[test]
    fn a_fallback_is_shouted_not_mentioned() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-fallback");
        let confined = Confined::prepare(Confinement::with_data_root(temp.path()))
            .expect("an unprivileged process with a writable root can confine");
        let report = confined.report().join("\n");
        if confined.ceiling().fallback_reason().is_none() {
            crate::testing::skipped(
                "a_fallback_is_shouted_not_mentioned",
                "this host enforces the real cgroup ceiling, so there is no fallback \
                 line to assert here",
            );
            return;
        }
        assert!(report.contains("FALLING BACK"), "{report}");
        // D23 named two weaknesses of the fallback and left out the one that
        // matters most: without a cgroup there is no handle on the job's whole
        // tree. The startup report names all three or it is lying.
        assert!(
            report.contains("kill without a cgroup: FALLING BACK"),
            "the fallback's weakest point is the kill, and it must be named: {report}"
        );
        assert!(
            report.contains("new session") && report.contains("marker"),
            "{report}"
        );
    }

    /// An operator who names `--cgroup-root` asked for one subtree in
    /// particular. A runner that could not use it says so and stops, naming
    /// what the subtree lacks — it does not quietly enforce something else.
    #[test]
    fn an_unusable_cgroup_root_refuses_the_startup_naming_what_it_lacks() {
        let temp = TempDir::new("confine-bad-cgroup-root");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.cgroup_root = Some(PathBuf::from("/sys/fs/cgroup/herd-no-such-cgroup-m4"));
        let error = Confined::prepare(confinement)
            .expect_err("a cgroup root that is not there cannot be used");
        assert!(error.contains("--cgroup-root"), "{error}");
        assert!(error.contains("herd-no-such-cgroup-m4"), "{error}");
        assert!(
            error.contains("does not exist"),
            "the refusal must name what is wrong with the subtree: {error}"
        );
    }

    #[test]
    fn a_bad_data_root_refuses_the_whole_startup_naming_the_path() {
        let temp = TempDir::new("confine-bad-root");
        let file = temp.path().join("a-file");
        std::fs::write(&file, b"not a directory").expect("the fixture writes");
        let error = Confined::prepare(Confinement::with_data_root(&file))
            .expect_err("a file is not a data root");
        assert!(error.contains(&file.display().to_string()), "{error}");
    }

    #[test]
    fn a_cap_of_zero_is_refused_rather_than_enforced() {
        let temp = TempDir::new("confine-zero");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.memory_max = 0;
        let error = Confined::prepare(confinement.clone()).expect_err("0 bytes is not a cap");
        assert!(error.contains("memory ceiling of 0"), "{error}");

        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.timeout = Duration::ZERO;
        let error = Confined::prepare(confinement).expect_err("0 s is not a cap");
        assert!(error.contains("time cap of 0"), "{error}");
    }

    /// An output cap a job could never reach is clamped to the disk budget,
    /// and the clamp is said out loud.
    ///
    /// This assertion used to be about the frame, and it used to be a
    /// *refusal*: a cap past what one verdict frame could carry would have
    /// produced a frame the hub could not decode, which is D128's redial
    /// loop and worth refusing a startup over. The streamed carriage removed
    /// that ceiling (D148), and the bound that is left is the disk — a job
    /// writes `{out}` inside its workspace and the workspace stops at the
    /// disk budget.
    ///
    /// So it clamps, which is D89's precedent for a figure that cannot bind:
    /// lowering a disk budget must not stop a runner that was working, and a
    /// cap that says more than it can do is a line to print rather than a
    /// machine to take out of the fleet.
    #[test]
    fn an_output_cap_larger_than_the_disk_budget_is_clamped_to_it() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-output-cap");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.disk_budget = 64 << 20;
        confinement.output_cap = confinement.disk_budget * 4;
        let confined = Confined::prepare(confinement).expect("an unreachable cap is not a refusal");
        assert_eq!(
            confined.caps().output_cap,
            64 << 20,
            "the cap that is used is the one that can bind"
        );
        // Equal to the budget is left alone: this is a clamp on what cannot
        // bind, not a margin somebody has to guess at. Its own data root,
        // because the one above is locked for the life of its `Confined`.
        drop(confined);
        let other = TempDir::new("confine-output-cap-exact");
        let mut confinement = Confinement::with_data_root(other.path());
        confinement.disk_budget = 64 << 20;
        confinement.output_cap = confinement.disk_budget;
        let confined = Confined::prepare(confinement).expect("prepares");
        assert_eq!(confined.caps().output_cap, 64 << 20);
    }

    /// Finding 5 of M4's verification: `--job-memory-max 1kB` was accepted,
    /// and it refuses every job exactly as `0` does — only `0` was refused.
    /// A ceiling too small to run anything is a wall, and the refusal has to
    /// name the floor so the operator knows what to write instead.
    #[test]
    fn a_memory_ceiling_too_small_to_run_anything_is_refused_naming_the_floor() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-tiny");
        for tiny in [1, 1_000, 1 << 20, MINIMUM_MEMORY_MAX - 1] {
            let mut confinement = Confinement::with_data_root(temp.path());
            confinement.memory_max = tiny;
            let error = Confined::prepare(confinement)
                .err()
                .unwrap_or_else(|| panic!("a ceiling of {tiny} bytes was accepted"));
            assert!(
                error.contains("refuse every job") && error.contains("16 MiB"),
                "the refusal must name the floor: {error}"
            );
        }
        // And the floor itself is accepted: the refusal is a floor, not a
        // second default.
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.memory_max = MINIMUM_MEMORY_MAX;
        Confined::prepare(confinement).expect("the floor is a usable ceiling");
    }

    /// A weight outside the kernel's range is a figure that would be refused
    /// once per job forever, invisibly. It is refused once, at startup,
    /// naming the range and saying what a weight is.
    #[test]
    fn a_weight_outside_the_kernel_s_range_refuses_the_startup() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-weights");
        for impossible in [0, 10_001, u32::MAX] {
            let mut confinement = Confinement::with_data_root(temp.path());
            confinement.cpu_weight = impossible;
            let error = Confined::prepare(confinement)
                .err()
                .unwrap_or_else(|| panic!("a cpu.weight of {impossible} was accepted"));
            assert!(
                error.contains("--job-cpu-weight") && error.contains("1..=10000"),
                "{error}"
            );

            let mut confinement = Confinement::with_data_root(temp.path());
            confinement.io_weight = impossible;
            let error = Confined::prepare(confinement)
                .err()
                .unwrap_or_else(|| panic!("an io.weight of {impossible} was accepted"));
            assert!(error.contains("--job-io-weight"), "{error}");
        }
    }

    /// And a pid ceiling too small to hold a shell and its command is a wall,
    /// refused with its floor exactly as a 1 kB memory ceiling is.
    #[test]
    fn a_pid_ceiling_too_small_to_run_anything_is_refused_naming_the_floor() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-pids");
        for wall in [0, 1, MINIMUM_PIDS_MAX - 1] {
            let mut confinement = Confinement::with_data_root(temp.path());
            confinement.pids_max = wall;
            let error = Confined::prepare(confinement)
                .err()
                .unwrap_or_else(|| panic!("a pids.max of {wall} was accepted"));
            assert!(
                error.contains("--job-pids-max") && error.contains("refuse every job"),
                "{error}"
            );
        }
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.pids_max = MINIMUM_PIDS_MAX;
        Confined::prepare(confinement).expect("the floor is a usable ceiling");
    }

    /// `cpu.max` is absent by default, so asking for it is asking for a hard
    /// guarantee. A host that cannot give one refuses the startup naming what
    /// to grant, rather than running jobs under the weight and calling it a
    /// quota.
    #[test]
    fn a_cpu_quota_this_host_cannot_enforce_refuses_the_startup() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-quota");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.cpu_max = Some(CpuQuota::from_cores(1.5).expect("1.5 cores is a quota"));
        match Confined::prepare(confinement) {
            Err(error) => {
                assert!(error.contains("--job-cpu-max"), "{error}");
                assert!(
                    error.contains("Delegate=yes"),
                    "the refusal must name what to grant: {error}"
                );
            }
            Ok(confined) => {
                // This host can enforce a quota, so the other half of the
                // promise is the one to assert: it is in the report, in cores
                // and in the kernel's own two numbers.
                let report = confined.report().join("\n");
                assert!(
                    report.contains("cpu.max 1.50 cores") && report.contains("150000 100000"),
                    "{report}"
                );
            }
        }
    }

    /// **The refusal, which is the whole point of this option.** A hard IO
    /// ceiling asked for on a path that cannot carry one is a **startup
    /// refusal naming the path and the reason** — never a warning, never a
    /// silent fallback to the weight, never a zero.
    ///
    /// A tmpfs is the deterministic case on every Linux: a real, writable
    /// data root on a filesystem that is no block device, so `io.max` has no
    /// device to hold a line against. ZFS is the same answer by a different
    /// road and is asserted in [`crate::blockio`], where the mount can be
    /// stated rather than hoped for.
    ///
    /// Contrast this deliberately with `io.weight`, which is written and
    /// reported inert on such a host: a weight nobody honours costs nothing,
    /// and a ceiling nobody honours is a false promise on the one machine the
    /// first consumer's builds freeze (D116, D120, D129).
    #[test]
    fn an_io_ceiling_no_device_could_carry_refuses_the_startup_naming_the_path() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let tmpfs = Path::new("/dev/shm");
        if !tmpfs.is_dir() {
            crate::testing::skipped(
                "an_io_ceiling_no_device_could_carry_refuses_the_startup_naming_the_path",
                "this host has no /dev/shm, so there is no filesystem here that is certainly \
                 not a block device — a host with a tmpfs would prove that --job-io-max on \
                 a path no device backs refuses the startup naming the path",
            );
            return;
        }
        let _reaping = crate::testing::one_reaper_at_a_time();
        let root = tmpfs.join(format!("herd-io-max-refusal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("a tmpfs is writable");
        let mut confinement = Confinement::with_data_root(&root);
        confinement.io_max = Some(IoCeiling::parse("rbps=200MB,wbps=100MB").expect("a ceiling"));
        let error = Confined::prepare(confinement)
            .expect_err("no io.max line can be written for a path on a tmpfs");
        let _ = std::fs::remove_dir_all(&root);
        assert!(error.contains("--job-io-max"), "{error}");
        assert!(
            error.contains(&root.display().to_string()),
            "the refusal must name the path it is about: {error}"
        );
        assert!(
            error.contains("not one block device"),
            "the refusal must say why, or it reads as a bug: {error}"
        );
        assert!(
            error.contains("shares contended bandwidth and caps nothing"),
            "the refusal must say what leaving the option out costs, because that is the \
             choice it is handing back: {error}"
        );
    }

    /// And where this host *can* carry one, the other half of the promise:
    /// the ceiling is proved by writing it, it names the device the data root
    /// is on, and the report says which device and that it is a hard cap.
    #[test]
    fn a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-io-max");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.io_max = Some(IoCeiling::parse("wbps=64MiB,wiops=500").expect("a ceiling"));
        match Confined::prepare(confinement) {
            Err(error) => {
                // This host cannot carry one, which is a refusal and not a
                // warning. It still has to name the path and the reason, and
                // a reader of a green suite has to know what was not proved.
                assert!(error.contains("--job-io-max"), "{error}");
                assert!(
                    error.contains(&temp.path().display().to_string()),
                    "{error}"
                );
                crate::testing::skipped(
                    "a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device",
                    &format!(
                        "this host refused the ceiling, correctly, because: {error} — a host \
                         with a delegated cgroup v2 subtree (systemd-run --user --scope -p \
                         Delegate=yes) and a data root on a block device would prove that \
                         the ceiling is written, names that device, and is reported as a \
                         hard cap"
                    ),
                );
            }
            Ok(confined) => {
                let io_max = confined
                    .ceiling()
                    .limits()
                    .io_max
                    .clone()
                    .expect("a proved ceiling is carried into every job's limits");
                // Per-device, and the device is the one the data root is on.
                let expected = crate::blockio::ceiling_device(confined.root().path())
                    .expect("the ceiling was proved, so the device resolved");
                assert_eq!(io_max.device, expected.device);
                assert_eq!(
                    io_max.control_value(),
                    format!("{} wbps=67108864 wiops=500", expected.device)
                );
                let report = confined.report().join("\n");
                assert!(
                    report.contains(&format!("io.max for device {}", expected.device)),
                    "the report must name the device, because moving the data root moves \
                     the answer: {report}"
                );
                assert!(
                    report.contains("wbps=67108864 wiops=500"),
                    "the report must say what the ceiling is: {report}"
                );
                assert!(
                    report.contains("hard per-second ceiling"),
                    "a reader has to be able to tell this from io.weight: {report}"
                );
                assert!(
                    report.contains("outside the data root"),
                    "the report must say what the ceiling does not reach: {report}"
                );
            }
        }
    }

    /// Absent by default, and absent is said out loud rather than left out:
    /// an operator reading the report has to see that no hard ceiling is a
    /// choice, and that the weight beside it caps nothing.
    #[test]
    fn no_declared_ceiling_is_reported_as_no_ceiling_and_not_as_a_zero() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-no-io-max");
        let confinement = Confinement::with_data_root(temp.path());
        assert_eq!(confinement.io_max, None, "absent by default");
        let confined = Confined::prepare(confinement).expect("the default needs nothing granted");
        assert_eq!(
            confined.ceiling().limits().io_max,
            None,
            "absent must mean no line written, never a zero and never a guess"
        );
        let report = confined.report().join("\n");
        assert!(
            report.contains("no io.max, so no hard IO ceiling"),
            "{report}"
        );
        assert!(
            report.contains("caps nothing"),
            "the line has to say what a weight is, or it reads as a bound: {report}"
        );
        assert!(
            report.contains("there is no default"),
            "and why there is no figure: {report}"
        );
    }

    /// A declared ceiling changes the inert-weight line, and must: "an
    /// absolute io.max is the only lever that works here, and this runner
    /// sets none" is false the moment one is set, and a stale claim in a
    /// startup report is the exact defect that whole line exists to prevent.
    ///
    /// Measured on this project's server: on its SATA disk blk-iocost knows
    /// no cost model, so the weight is inert there **and the ceiling still
    /// holds** — which is the difference between the two knobs, live.
    #[test]
    fn a_declared_ceiling_changes_what_the_inert_weight_line_says() {
        // Over the decision rather than over a host: whether this host's
        // weight is inert is not this test's question, and a sentence that
        // contradicts itself is.
        let inert = "io.weight 10: WRITTEN BUT INERT for jobs here, because the disk says so. \
                     An absolute io.max is the only IO lever that works whatever the \
                     scheduler is, and this runner sets none: a job's IO here is bounded by \
                     the idle IO class alone.";
        assert!(
            inert.contains("sets none"),
            "this is the sentence for a runner with no ceiling"
        );
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-inert-and-ceiling");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.io_max = Some(IoCeiling::parse("wbps=1MB").expect("a ceiling"));
        let Ok(confined) = Confined::prepare(confinement) else {
            crate::testing::skipped(
                "a_declared_ceiling_changes_what_the_inert_weight_line_says",
                "this host cannot carry a ceiling, so there is no report with one in it \
                 here — a delegated scope with its data root on a block device would prove \
                 that the inert-weight line stops saying \"this runner sets none\" once a \
                 ceiling is declared",
            );
            return;
        };
        let report = confined.report().join("\n");
        assert!(
            !report.contains("sets none"),
            "the report claims no ceiling is set while one is: {report}"
        );
        if let Some(line) = report
            .lines()
            .find(|line| line.contains("WRITTEN BUT INERT"))
        {
            assert!(
                line.contains("sets one, on device"),
                "an inert weight beside a real ceiling must say which of the two bounds a \
                 job here: {line}"
            );
        }
    }

    /// Pressure is read, recorded and said out loud — and the line says in
    /// as many words that it gates nothing (E-32, D119).
    #[test]
    fn the_report_says_pressure_is_recorded_and_gates_nothing() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("confine-pressure");
        let confined = Confined::prepare(Confinement::with_data_root(temp.path()))
            .expect("an unprivileged process with a writable root can confine");
        let report = confined.report().join("\n");
        let line = report
            .lines()
            .find(|line| line.starts_with("pressure:"))
            .expect("the report says what it does with pressure, either way");
        assert!(
            line.contains("E-32"),
            "the line must name the expectation it is keeping: {line}"
        );
        if line.contains("NOT READABLE") {
            assert!(
                line.contains("absent rather than reporting a zero"),
                "an unreadable figure is absent, never zero: {line}"
            );
        } else {
            assert!(
                line.contains("a gate on nothing")
                    && line.contains("No job is ever refused or delayed"),
                "the guarantee is the point of the line: {line}"
            );
        }
    }

    #[test]
    fn a_bad_helper_list_refuses_the_whole_startup() {
        let temp = TempDir::new("confine-helpers");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.privileged_helpers = vec!["mount".to_owned()];
        let error = Confined::prepare(confinement).expect_err("a relative helper is refused");
        assert!(error.contains("mount"), "{error}");
    }

    /// Nothing of the runner's own environment reaches a job, and every
    /// directory a well-behaved tool writes to is inside the data root.
    #[test]
    fn a_job_s_environment_is_built_not_inherited_and_points_inside_the_root() {
        let temp = TempDir::new("confine-env");
        let root = DataRoot::open(temp.path()).expect("usable");
        let job = herd_proto::JobId::new("j1");
        let workspace = root.open_workspace(&job).expect("made");
        let tag = JobTag::mint(&job);
        let environment = job_environment(&workspace, &tag);
        let names: Vec<&str> = environment.iter().map(|(name, _)| name.as_str()).collect();
        assert!(!names.contains(&"HERD_TOKEN"), "{names:?}");
        // The marker is the one variable Herd puts there on purpose.
        assert_eq!(
            environment
                .iter()
                .find(|(name, _)| name == MARKER_VARIABLE)
                .map(|(_, value)| value.as_str()),
            Some(tag.as_str()),
            "a job must carry its own marker: {names:?}"
        );
        for writable in [
            "HOME",
            "TMPDIR",
            "TMP",
            "TEMP",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
        ] {
            let value = environment
                .iter()
                .find(|(name, _)| name == writable)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("{writable} must be set for a job"));
            assert!(
                Path::new(&value).starts_with(root.path()),
                "{writable} points outside the data root: {value}"
            );
        }
    }

    // ------------------- one job's own kernel memory ceiling --------------

    /// A job the hub costed is held to **its** figure, and a job costed at
    /// nothing is held to the machine's.
    ///
    /// Both halves in one test because the second is the one a careless fix
    /// would lose: `herd_hub::JobRecord::memory_need_mib` answers `Ok(None)`
    /// for a real class of job — `herdctl submit -- true` — so
    /// `--job-memory-max` is not a legacy figure to be replaced, it is the
    /// answer for every job nobody costed.
    #[test]
    fn a_costed_job_gets_its_own_ceiling_and_an_uncosted_one_gets_the_machine_s() {
        let global = 2 << 30;
        let costed = job_ceiling(
            global,
            Some(herd_proto::JobNeed {
                memory_mib: 4096,
                tmpfs_mib: 4096,
            }),
        );
        assert_eq!(
            costed.memory_max(),
            9011 * (1 << 20),
            "8192 MiB declared plus the margin, and not the machine's 2 GiB"
        );
        assert_ne!(
            costed.memory_max(),
            global,
            "the whole defect was a declared job running under the machine's one figure"
        );

        let uncosted = job_ceiling(global, None);
        assert_eq!(
            uncosted.memory_max(),
            global,
            "a job the hub costed at nothing keeps --job-memory-max exactly"
        );
        assert_eq!(
            uncosted.line(true, None),
            None,
            "and there is nothing new to say about it: the startup report named that figure"
        );
    }

    /// A declaration too small to start a shell is **floored**, not obeyed.
    ///
    /// [`MINIMUM_MEMORY_MAX`] is a measurement — 16 MiB is what
    /// `/bin/sh -c 'echo ok'` needs of address space — and
    /// [`Confined::prepare`] already refuses a *global* figure under it
    /// because invariant 2 asks for a cap and not for a wall. A per-job
    /// figure under it is that same wall arriving from the hub, and it gets
    /// the same answer; the job still runs, and the line says the floor is
    /// what was written.
    #[test]
    fn a_declaration_under_the_floor_is_floored_and_says_so() {
        let tiny = job_ceiling(
            2 << 30,
            Some(herd_proto::JobNeed {
                memory_mib: 1,
                tmpfs_mib: 0,
            }),
        );
        assert_eq!(tiny.memory_max(), MINIMUM_MEMORY_MAX);
        let said = tiny
            .line(true, None)
            .expect("a costed job has a ceiling worth a line");
        assert!(
            said.contains("floored at"),
            "a figure the runner raised must say it raised it: {said}"
        );

        // And the floor is a floor and not a replacement: a figure above it
        // is left exactly alone.
        let above = job_ceiling(
            2 << 30,
            Some(herd_proto::JobNeed {
                memory_mib: 1024,
                tmpfs_mib: 0,
            }),
        );
        assert_eq!(above.memory_max(), 1126 * (1 << 20));
        assert!(!above
            .line(true, None)
            .expect("a line")
            .contains("floored at"));
    }

    /// A host that cannot make a cgroup says the ceiling is **NOT ENFORCED**,
    /// per job, rather than printing the figure and leaving it there.
    ///
    /// The fallback is `RLIMIT_AS` on the job's first process: one process's
    /// address space, not the resident memory of a process tree. That is a
    /// weaker thing than what the hub admitted the job against, and a runner
    /// that named the figure without naming the weakness would be reporting a
    /// ceiling it does not hold.
    #[test]
    fn a_host_that_cannot_enforce_a_per_job_ceiling_says_so_per_job() {
        let ceiling = job_ceiling(
            2 << 30,
            Some(herd_proto::JobNeed {
                memory_mib: 4096,
                tmpfs_mib: 0,
            }),
        );
        let fallen_back = ceiling
            .line(false, Some("there is no cgroup v2 here."))
            .expect("a costed job on a fallback host has something to say");
        assert!(
            fallen_back.contains("NOT ENFORCED"),
            "the honest word has to be in it: {fallen_back}"
        );
        assert!(
            fallen_back.contains("there is no cgroup v2 here."),
            "and the host's own reason, so an operator knows what to grant: {fallen_back}"
        );
        assert!(
            fallen_back.contains("4096 MiB of memory"),
            "with the figure that is not being enforced: {fallen_back}"
        );

        // The same ceiling on a host that *can* enforce it says the opposite,
        // and that pair is what makes the sentence above a measurement of the
        // host rather than a constant.
        let enforced = ceiling
            .line(true, None)
            .expect("a costed job has a ceiling worth a line");
        assert!(!enforced.contains("NOT ENFORCED"), "{enforced}");
        assert!(enforced.contains("memory.max"), "{enforced}");
    }
}
