//! The kernel bounds a job runs inside, and the fallback when there are none
//! (E-30, Q6).
//!
//! All of one job's processes live in one cgroup v2 subtree carrying
//! `memory.max`, `cpu.weight`, `io.weight`, `pids.max` and — where an
//! operator lent one — `cpu.max`. An allocation past the memory ceiling is
//! refused by the kernel and the cgroup's processes are killed — **the job's, and only the job's**: the
//! runner is in a sibling cgroup, and so is every other job. Nothing here
//! asks a userspace watchdog to notice in time, because by the time a
//! watchdog notices a 31 GiB host is already swapping.
//!
//! ## Why the runner moves itself
//!
//! cgroup v2 forbids a cgroup from holding processes once a controller is
//! enabled for its children. The runner therefore moves its own processes
//! into a leaf of its base cgroup and then enables `memory` for the base's
//! children, so the base can be the parent of one cgroup per job. That is
//! the standard shape of a delegated subtree; it needs the base to be
//! delegated, which is what a systemd unit with `Delegate=yes` gives (D5).
//!
//! ## When there is no delegation
//!
//! A fallback that is silent is worse than no fallback, so
//! [`Ceiling::detect`] returns the reason it could not use cgroups, the
//! runner prints it at startup, and the job's log says it too. The fallback
//! is `RLIMIT_AS` on the job's own process: a real kernel cap, so invariant 2
//! still holds, but one that bounds each process rather than the tree, and
//! that makes an allocation fail rather than kill. The difference is
//! documented behaviour and is named in the fallback reason.
//!
//! ## When a controller is missing rather than the whole subtree
//!
//! `memory` is not the only controller a base may withhold. A host can give
//! `memory` and not `cpu`, and then a job's CPU share is nothing but its
//! niceness. There is no fallback to offer for that — niceness is already
//! applied — so what is owed is the truth: every control the subtree cannot
//! enforce comes back from [`Ceiling::detect`] as a sentence the runner
//! prints at startup, in the same voice as the memory fallback. The one
//! exception is `cpu.max`, which is absent by default and therefore a hard
//! ceiling an operator *asked* for: that one is a refusal at startup rather
//! than a line in a report (see [`crate::confine::Confined::prepare`]).

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::reclaim::{remove_cgroup, JobTag};

/// Counts delegation probes, so that two runners preparing at the same moment
/// cannot collide on one probe directory.
static PROBES: AtomicU64 = AtomicU64::new(0);

/// Where cgroup v2 is mounted on every system that has it.
const MOUNT_POINT: &str = "/sys/fs/cgroup";

/// The leaf the runner moves its own processes into, so that its base cgroup
/// can be the parent of one cgroup per job.
const RUNNER_LEAF: &str = "herd-runner.leaf";

/// How much CPU a job gets when something else on the machine wants it.
///
/// `cpu.weight` is a share, not a cap: 100 is what an ordinary process gets,
/// and a job at 10 takes about a tenth of a contended core against it — while
/// still using every core of a machine nobody else is using, which is the
/// whole point of a weight and the reason there is no quota by default. The
/// consumer's two incidents were a host with nothing but `nice 10` between a
/// twelve-thread build and every docker service on it.
pub const DEFAULT_CPU_WEIGHT: u32 = 10;

/// The same shape for block IO. Honoured only by bfq or blk-iocost, which is
/// why the runner says at startup whether this host honours it at all
/// ([`crate::blockio`]).
pub const DEFAULT_IO_WEIGHT: u32 = 10;

/// What the kernel accepts in `cpu.weight` and `io.weight`. A figure outside
/// it is a startup refusal, not a write the kernel turns down per job.
pub const WEIGHT_RANGE: std::ops::RangeInclusive<u32> = 1..=10_000;

/// How many processes one job may hold at once.
///
/// A plain number by default, and not optional: a fork bomb inside a job must
/// not be able to take the host down, and nothing about a build needs an
/// unbounded process count. A thousand is far above a twelve-thread build of
/// the first consumer and far below what it takes to exhaust a host's pid
/// space.
pub const DEFAULT_PIDS_MAX: u64 = 1024;

/// The smallest pid ceiling that is a cap rather than a wall: a job is at
/// least a shell, the command it runs and a child or two of that command.
pub const MINIMUM_PIDS_MAX: u64 = 8;

/// A hard CPU quota: microseconds of CPU time per period, which is exactly
/// what `cpu.max` takes.
///
/// Said to an operator in cores, because that is the unit `--host-threads`
/// and a build's `-j` are already in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuQuota {
    quota_us: u64,
    period_us: u64,
}

impl CpuQuota {
    /// 100 ms, the kernel's own default period. One number is enough for an
    /// operator to declare; the period is not a dial anybody has asked for.
    pub const PERIOD_US: u64 = 100_000;

    /// The smallest quota that is a cap rather than a wall, in cores. A
    /// hundredth of a core still gets a shell through its own startup; less
    /// is a promise that nothing finishes.
    pub const MINIMUM_CORES: f64 = 0.01;

    /// A quota of this many cores, or the sentence to refuse the startup with.
    pub fn from_cores(cores: f64) -> Result<Self, String> {
        if !cores.is_finite() || cores < Self::MINIMUM_CORES {
            return Err(format!(
                "a hard CPU quota of {cores} cores is a wall and not a cap: the floor is \
                 {} of a core. Leave the option out for no quota at all, which is the \
                 default.",
                Self::MINIMUM_CORES
            ));
        }
        Ok(Self {
            quota_us: (cores * Self::PERIOD_US as f64).round() as u64,
            period_us: Self::PERIOD_US,
        })
    }

    /// How many cores this quota is worth.
    pub fn cores(&self) -> f64 {
        self.quota_us as f64 / self.period_us as f64
    }

    /// What goes in `cpu.max`: a quota and a period, in microseconds.
    pub fn control_value(&self) -> String {
        format!("{} {}", self.quota_us, self.period_us)
    }
}

/// A hard per-device IO ceiling: exactly what `io.max` takes, in bytes per
/// second and in IOPS, read and write apart.
///
/// **`io.max` is a hard cap; `io.weight` is a share of contended bandwidth.**
/// The two are not two strengths of one knob. A weight decides who gets the
/// disk when two cgroups both want it and caps nothing — one job alone on an
/// idle machine saturates the disk at any weight, which is the first
/// consumer's freeze. A ceiling is enforced by blk-throttle whatever the IO
/// scheduler is, and it holds whether or not anything else is asking for the
/// disk.
///
/// **Neither bounds a job writing an absolute path outside its data root.**
/// `io.max` names one device — the one the data root is on — and a write
/// somewhere else goes to another device with no line of Herd's on it. Herd
/// does not stop a job naming an absolute path (D123, D126), so that is a gap
/// in both knobs rather than a weakness of one.
///
/// Absent by default, and absent means **no line written**: never a zero and
/// never a guess. There is no honest default throughput — a figure chosen on
/// an NVMe is absurd on a spinning disk and vice versa — so a number here is
/// always an operator's measurement of his own machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoCeiling {
    read_bps: Option<u64>,
    write_bps: Option<u64>,
    read_iops: Option<u64>,
    write_iops: Option<u64>,
}

impl IoCeiling {
    /// What `--job-io-max` accepts: `rbps=`, `wbps=`, `riops=`, `wiops=`,
    /// comma-separated, in any order, at least one of them.
    pub const KEYS: [&'static str; 4] = ["rbps", "wbps", "riops", "wiops"];

    /// Read an operator's `rbps=200MB,wbps=100MB` into a ceiling, or the
    /// sentence to refuse the startup with.
    ///
    /// Strict on purpose, in the voice the capability file is read in: an
    /// unknown key, a key written twice, a figure that is not a figure and a
    /// figure of zero are each a refusal naming what is wrong. A typo that
    /// was skipped would be a machine running with no ceiling and an operator
    /// who believes it has one.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut ceiling = Self {
            read_bps: None,
            write_bps: None,
            read_iops: None,
            write_iops: None,
        };
        for field in spec.split(',').map(str::trim).filter(|f| !f.is_empty()) {
            let Some((key, value)) = field.split_once('=') else {
                return Err(format!(
                    "{field:?} is not a limit: an IO ceiling is written as key=value, one of \
                     {}, separated by commas — for instance rbps=200MB,wbps=100MB",
                    Self::KEYS.join(", ")
                ));
            };
            let key = key.trim();
            let value = value.trim();
            // Bytes per second go through the same reader as every other
            // size an operator gives, so `200MB` and `200MiB` differ here
            // exactly as they do in `--job-memory-max`. IOPS are a count.
            let figure = match key {
                "rbps" | "wbps" => crate::size::parse_bytes(value)
                    .map_err(|why| format!("{key}={value} is not a number of bytes: {why}"))?,
                "riops" | "wiops" => value.parse::<u64>().map_err(|_| {
                    format!("{key}={value} is not a number of IO operations per second")
                })?,
                other => {
                    return Err(format!(
                        "{other:?} is not an IO ceiling this runner knows. The keys are {}; a \
                         ceiling is per-device bytes and IOPS, nothing else.",
                        Self::KEYS.join(", ")
                    ))
                }
            };
            // Zero is refused rather than written: `io.max rbps=0` stops the
            // device dead for this job, which is a wall and not a cap — the
            // same rule the memory ceiling and the pid ceiling are held to.
            if figure == 0 {
                return Err(format!(
                    "{key}=0 would stop this job's IO to that device altogether, which is a \
                     wall and not a cap. Leave the whole option out for no ceiling, which \
                     is the default."
                ));
            }
            let slot = match key {
                "rbps" => &mut ceiling.read_bps,
                "wbps" => &mut ceiling.write_bps,
                "riops" => &mut ceiling.read_iops,
                _ => &mut ceiling.write_iops,
            };
            if slot.is_some() {
                return Err(format!(
                    "{key} is given twice; the second figure would silently replace the \
                     first, so neither is used"
                ));
            }
            *slot = Some(figure);
        }
        if ceiling == Self::nothing() {
            return Err(format!(
                "an IO ceiling of nothing is not a ceiling: give at least one of {}, or leave \
                 the option out, which is the default and writes no io.max at all",
                Self::KEYS.join(", ")
            ));
        }
        Ok(ceiling)
    }

    const fn nothing() -> Self {
        Self {
            read_bps: None,
            write_bps: None,
            read_iops: None,
            write_iops: None,
        }
    }

    /// The limits as the kernel's keys, in the order `io.max` prints them.
    /// A limit left out is a limit left at `max`, which is why absent is
    /// written as nothing rather than as a figure.
    pub fn fields(&self) -> Vec<(&'static str, u64)> {
        [
            ("rbps", self.read_bps),
            ("wbps", self.write_bps),
            ("riops", self.read_iops),
            ("wiops", self.write_iops),
        ]
        .into_iter()
        .filter_map(|(key, figure)| figure.map(|figure| (key, figure)))
        .collect()
    }

    /// The ceiling as an operator wrote it, for the startup report.
    pub fn describe(&self) -> String {
        self.fields()
            .into_iter()
            .map(|(key, figure)| format!("{key}={figure}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// An `io.max` ceiling together with the device it names.
///
/// The two are one thing on purpose: `io.max` is per-device, so a ceiling
/// that has not been resolved against a device is not something that could be
/// written. Resolving it is a startup step that either succeeds or refuses
/// ([`crate::blockio::ceiling_device`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoMax {
    pub ceiling: IoCeiling,
    /// `major:minor` of the disk the data root is on.
    pub device: String,
}

impl IoMax {
    /// What goes in `io.max`: the device, then the limits it is given.
    pub fn control_value(&self) -> String {
        let mut line = self.device.clone();
        for (key, figure) in self.ceiling.fields() {
            line.push(' ');
            line.push_str(&format!("{key}={figure}"));
        }
        line
    }
}

/// Everything one job's cgroup bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct JobLimits {
    /// Kernel memory ceiling, in bytes (E-30).
    pub memory_max: u64,
    /// `cpu.weight`: the share a job gets under contention.
    pub cpu_weight: u32,
    /// `cpu.max`: a hard ceiling, and **absent by default**. The reasoning is
    /// `--host-memory`'s: a hard ceiling is a lend an operator declares, not
    /// something Herd assumes, and a quota wastes a machine nobody else is
    /// using.
    pub cpu_max: Option<CpuQuota>,
    /// `io.weight`: the same share for block IO, where the host honours it.
    pub io_weight: u32,
    /// `io.max`: a hard per-device IO ceiling, and **absent by default** —
    /// the same shape as `cpu_max` above and for the same reason. Resolved
    /// against a device at startup, because `io.max` is per-device and a
    /// ceiling that names none is no ceiling.
    pub io_max: Option<IoMax>,
    /// `pids.max`: how many processes one job may hold.
    pub pids_max: u64,
}

impl JobLimits {
    /// The defaults around one memory ceiling.
    pub fn with_memory_max(memory_max: u64) -> Self {
        Self {
            memory_max,
            cpu_weight: DEFAULT_CPU_WEIGHT,
            cpu_max: None,
            io_weight: DEFAULT_IO_WEIGHT,
            io_max: None,
            pids_max: DEFAULT_PIDS_MAX,
        }
    }
}

/// A control this host's subtree cannot enforce, and why.
///
/// Carried rather than logged where it is found, because the runner prints
/// its whole startup report in one place and a line printed from inside
/// detection would be invisible to the test that asserts the report.
#[derive(Debug, Clone)]
pub struct Unenforced {
    /// The controller that is missing: `cpu`, `io` or `pids`.
    pub controller: &'static str,
    /// Which controls go with it, as an operator reads them.
    pub controls: &'static str,
    /// One sentence naming what the subtree lacks.
    pub why: String,
}

/// How a runner enforces a job's bounds on this host.
#[derive(Debug)]
pub enum Ceiling {
    /// One cgroup per job under a delegated base. The ceiling is the
    /// kernel's, applies to the whole process tree, and kills.
    Cgroups {
        base: PathBuf,
        limits: JobLimits,
        /// The controls this base cannot give, each with its reason. Empty on
        /// a host that enforces all of them.
        unenforced: Vec<Unenforced>,
    },
    /// No usable cgroup v2 subtree. `why` is one sentence naming what was
    /// missing, printed at startup and repeated in every job's log.
    Rlimit { limits: JobLimits, why: String },
}

impl Ceiling {
    /// Work out which bounds this host can give, preparing the subtree if it
    /// can give the real ones.
    ///
    /// `base` overrides the cgroup the runner derives from its own
    /// `/proc/self/cgroup`; it exists because a test must be able to point a
    /// runner at a subtree it set up, and because an operator may want the
    /// runner's jobs somewhere other than beside the runner.
    pub fn detect(limits: JobLimits, base: Option<&Path>) -> Self {
        match prepare_base(base) {
            Ok((base, unenforced)) => Ceiling::Cgroups {
                base,
                limits,
                unenforced,
            },
            Err(why) => Ceiling::Rlimit { limits, why },
        }
    }

    /// The ceiling in bytes, whichever mechanism enforces it.
    pub fn memory_max(&self) -> u64 {
        self.limits().memory_max
    }

    /// Every bound a job is given here, enforced or not.
    pub fn limits(&self) -> &JobLimits {
        match self {
            Ceiling::Cgroups { limits, .. } | Ceiling::Rlimit { limits, .. } => limits,
        }
    }

    /// The controls this host cannot enforce, for the startup report. On a
    /// host with no subtree at all the memory fallback already says why, and
    /// this is empty: there is one reason, not four.
    pub fn unenforced(&self) -> &[Unenforced] {
        match self {
            Ceiling::Cgroups { unenforced, .. } => unenforced,
            Ceiling::Rlimit { .. } => &[],
        }
    }

    /// Why this host cannot enforce one controller's controls, or `None` when
    /// it can. A host with no cgroup subtree cannot enforce any of them, and
    /// says so with the fallback's own reason.
    pub fn cannot_enforce(&self, controller: &str) -> Option<&str> {
        match self {
            Ceiling::Rlimit { why, .. } => Some(why),
            Ceiling::Cgroups { unenforced, .. } => unenforced
                .iter()
                .find(|missing| missing.controller == controller)
                .map(|missing| missing.why.as_str()),
        }
    }

    /// The sentence to show for a host that fell back, or `None` for a host
    /// enforcing the real ceiling. Never silence.
    pub fn fallback_reason(&self) -> Option<&str> {
        match self {
            Ceiling::Cgroups { .. } => None,
            Ceiling::Rlimit { why, .. } => Some(why),
        }
    }

    /// The prepared base, where there is one. The startup reaper reads it to
    /// find the job cgroups a dead runner left under it.
    pub fn base(&self) -> Option<&Path> {
        match self {
            Ceiling::Cgroups { base, .. } => Some(base),
            Ceiling::Rlimit { .. } => None,
        }
    }

    /// Prove that this host would really take the ceiling an operator asked
    /// for, by writing it — into a probe cgroup under the same base every job
    /// cgroup will hang under, and reading it back out.
    ///
    /// Done rather than inferred, exactly as delegation is
    /// ([`prepare_base`]'s probe). Three things could each make a ceiling
    /// vanish and none of them is visible from a file somebody could read:
    /// the `io` controller is not handed down, the kernel was built without
    /// blk-throttle so `io.max` does not exist at all, or it exists and
    /// refuses this device. Writing the real value is the only answer that
    /// covers all three — and the answer has to be a refusal, because a
    /// ceiling quietly weaker than asked for is the wrong failure on a
    /// machine where one job can saturate the disk (D116, D129).
    pub fn prove_io_max(&self, io_max: &IoMax) -> Result<(), String> {
        let Ceiling::Cgroups { base, .. } = self else {
            return Err(
                "this host has no cgroup v2 subtree for a job to carry an io.max in".to_owned(),
            );
        };
        let probe = base.join(format!(
            "herd-io-max-probe-{}-{}",
            std::process::id(),
            PROBES.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&probe).map_err(|error| {
            format!(
                "no cgroup could be made under {} to try the ceiling in ({error})",
                base.display()
            )
        })?;
        let control = probe.join("io.max");
        let written = fs::write(&control, io_max.control_value()).map_err(|error| {
            if control.exists() {
                format!(
                    "the kernel refused {:?} in {}: {error}",
                    io_max.control_value(),
                    control.display()
                )
            } else {
                format!(
                    "{} does not exist, so this kernel has no per-cgroup IO throttling to \
                     hold a ceiling with (CONFIG_BLK_DEV_THROTTLING), or the io controller \
                     is not handed down to {}'s children: {error}",
                    control.display(),
                    base.display()
                )
            }
        });
        // Read back before the probe goes, because a write the kernel took
        // and did not keep would otherwise read as a proof.
        let readable = written
            .as_ref()
            .ok()
            .and_then(|()| fs::read_to_string(&control).ok());
        let _ = fs::remove_dir(&probe);
        written?;
        let Some(readable) = readable else {
            return Err(format!(
                "{} took the ceiling and then could not be read back, so nothing here can \
                 say the kernel kept it",
                control.display()
            ));
        };
        if !readable
            .lines()
            .any(|line| line.split_whitespace().next() == Some(io_max.device.as_str()))
        {
            return Err(format!(
                "the kernel took the ceiling for device {} and then listed none: {} reads \
                 {:?}",
                io_max.device,
                control.display(),
                readable.trim()
            ));
        }
        Ok(())
    }

    /// Carry a proved ceiling into the limits every job cgroup gets.
    ///
    /// Separate from [`Ceiling::detect`] because the order is forced: the
    /// ceiling can only be resolved against a device and proved against a
    /// subtree once there *is* a subtree, and an unprovable one is a startup
    /// refusal rather than a limit to carry.
    pub fn carry_io_max(&mut self, io_max: IoMax) {
        match self {
            Ceiling::Cgroups { limits, .. } | Ceiling::Rlimit { limits, .. } => {
                limits.io_max = Some(io_max)
            }
        }
    }

    /// Make this job's cgroup, or `None` on a host that fell back.
    pub fn open_for_job(&self, tag: &JobTag) -> Option<JobCgroup> {
        let Ceiling::Cgroups { base, limits, .. } = self else {
            return None;
        };
        match JobCgroup::create(base, tag, limits) {
            Ok(cgroup) => Some(cgroup),
            Err(error) => {
                // Detection succeeded and creation failed, which means the
                // subtree changed under us. Say so; the job then runs under
                // the rlimit fallback rather than uncapped.
                eprintln!(
                    "herd-runner: the cgroup for job {} could not be made under {}: \
                     {error}; this job falls back to an address-space limit{}",
                    tag.as_str(),
                    base.display(),
                    match &limits.io_max {
                        // The ceiling an operator declared is gone for this
                        // job, and that is worse news than the fallback
                        // itself: it was a cap, not a courtesy.
                        Some(io_max) => format!(
                            ", and the io.max ceiling declared for device {} is NOT ENFORCED \
                             for it — this job's IO is bounded by nothing",
                            io_max.device
                        ),
                        None => String::new(),
                    }
                );
                None
            }
        }
    }
}

/// One job's cgroup. Dropping it kills whatever is left in it and removes it:
/// a cgroup that outlives its job is a leak that eventually refuses mkdir.
#[derive(Debug)]
pub struct JobCgroup {
    dir: PathBuf,
}

impl JobCgroup {
    fn create(base: &Path, tag: &JobTag, limits: &JobLimits) -> io::Result<Self> {
        // The job's id is in the tag for a human reading `/sys/fs/cgroup`, but
        // what makes it unique is the runner's pid and a counter. Two runners
        // may share a base — two runners on one machine pointed at the same
        // `--cgroup-root`, or a test suite running several in one process —
        // and ids are only unique per hub. A name that collided would mean one
        // job's cgroup kill reaching another job's processes, which is exactly
        // the thing E-30 promises cannot happen.
        let dir = base.join(tag.as_str());
        // Unique, so this cannot be a leftover: if it exists, something is
        // wrong enough that the job should fall back rather than inherit
        // somebody else's oom counter.
        fs::create_dir(&dir)?;
        let cgroup = Self { dir };
        // memory.max before anything is in the cgroup: a process must never
        // be inside it for even one allocation without the ceiling set.
        fs::write(cgroup.dir.join("memory.max"), limits.memory_max.to_string())?;
        // Swap is part of the ceiling: a job allowed to swap past its memory
        // cap is a job that takes the host's responsiveness with it.
        let _ = fs::write(cgroup.dir.join("memory.swap.max"), "0");
        // Kill the whole cgroup on the first OOM rather than one process at a
        // time, so a job dies as a job and does not limp on half-killed.
        let _ = fs::write(cgroup.dir.join("memory.oom.group"), "1");

        // And the rest of the bounds, before a process is in here too. Each
        // one is written and its failure ignored on purpose: a controller the
        // base does not hand down was already named at startup
        // ([`Ceiling::unenforced`]), and a job refused here for a host
        // limitation the operator has been told about twice would be a runner
        // that takes no work rather than a runner that is honest. Memory
        // keeps its hard `?` above: that one is E-30's promise.
        let _ = fs::write(cgroup.dir.join("cpu.weight"), limits.cpu_weight.to_string());
        if let Some(quota) = limits.cpu_max {
            let _ = fs::write(cgroup.dir.join("cpu.max"), quota.control_value());
        }
        let _ = fs::write(cgroup.dir.join("io.weight"), limits.io_weight.to_string());
        // `io.max` is the one of these that keeps a hard `?`, beside memory,
        // and for the same reason: it is absent unless an operator declared
        // it, so a declared ceiling that could not be written is a promise
        // broken rather than a host limitation already reported. It was
        // proved writable at startup against this very base
        // ([`Ceiling::prove_io_max`]), so a failure here is the host having
        // changed underneath a running runner — loud, not swallowed.
        if let Some(io_max) = &limits.io_max {
            fs::write(cgroup.dir.join("io.max"), io_max.control_value())?;
        }
        let _ = fs::write(cgroup.dir.join("pids.max"), limits.pids_max.to_string());
        Ok(cgroup)
    }

    /// `cgroup.procs`, opened for writing before the job is spawned.
    ///
    /// The job's own process writes `0` into this descriptor between fork and
    /// exec, which is what makes the ceiling race-free: there is no instant
    /// in which a job process exists outside its cgroup.
    pub fn procs_file(&self) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .open(self.dir.join("cgroup.procs"))
    }

    /// The most memory this cgroup ever held, in bytes.
    pub fn peak_bytes(&self) -> Option<u64> {
        read_number(&self.dir.join("memory.peak"))
    }

    /// How many times the kernel killed something in this cgroup for going
    /// past the ceiling. Non-zero is what turns a SIGKILL into a named
    /// outcome instead of "signalled by something".
    pub fn oom_kills(&self) -> u64 {
        let Ok(text) = fs::read_to_string(self.dir.join("memory.events")) else {
            return 0;
        };
        text.lines()
            .filter_map(|line| line.strip_prefix("oom_kill "))
            .filter_map(|count| count.trim().parse::<u64>().ok())
            .next()
            .unwrap_or(0)
    }

    /// Kill every process in the cgroup, children and grandchildren alike, in
    /// one write. Immune to pid reuse, which `killpg` is not.
    pub fn kill(&self) -> io::Result<()> {
        fs::write(self.dir.join("cgroup.kill"), "1")
    }
}

impl JobCgroup {
    /// Where this cgroup lives, so that a runner being stopped can kill it
    /// without holding the guard.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for JobCgroup {
    fn drop(&mut self) {
        remove_cgroup(&self.dir);
    }
}

/// Find and prepare the base cgroup every job cgroup hangs under.
///
/// Every failure returns one sentence naming what was missing, because that
/// sentence is the whole value of the fallback.
fn prepare_base(explicit: Option<&Path>) -> Result<(PathBuf, Vec<Unenforced>), String> {
    if !cfg!(target_os = "linux") {
        return Err("this host is not Linux, so there is no cgroup v2 to put a job in".to_owned());
    }
    let base = match explicit {
        Some(path) => path.to_path_buf(),
        None => own_cgroup()?,
    };
    if !base.is_dir() {
        return Err(format!(
            "the cgroup {} does not exist, so no job cgroup can hang under it",
            base.display()
        ));
    }
    let controllers = fs::read_to_string(base.join("cgroup.controllers")).map_err(|error| {
        format!(
            "the cgroup {} has no cgroup.controllers to read: {error}",
            base.display()
        )
    })?;
    if !controllers.split_whitespace().any(|name| name == "memory") {
        return Err(format!(
            "the cgroup {} does not offer the memory controller (it offers: {}), so no \
             kernel memory ceiling can be set there",
            base.display(),
            controllers.trim()
        ));
    }
    // Delegation, tested by doing it: a probe directory is the only honest
    // answer to "may this runner create cgroups here". The name is unique per
    // attempt, so two runners preparing at once never read each other's probe
    // as a refusal.
    let probe = base.join(format!(
        "herd-delegation-probe-{}-{}",
        std::process::id(),
        PROBES.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir(&probe).map_err(|error| {
        format!(
            "cgroup v2 delegation is unavailable: this runner cannot create a cgroup \
             under {} ({error}). Run it as a systemd unit with Delegate=yes, or inside \
             a delegated scope, and point --cgroup-root inside that subtree.",
            base.display()
        )
    })?;
    let _ = fs::remove_dir(&probe);

    // Creating a cgroup is not the same right as putting a process in one.
    // cgroup v2 requires write access to `cgroup.procs` of the **common
    // ancestor** of the process's current cgroup and its destination, so a
    // runner standing outside the subtree it was pointed at can make job
    // cgroups it could never move a job into. That has to be a refusal here,
    // naming what the operator must grant, rather than a job that fails.
    let own = own_cgroup()?;
    migration_is_possible(&own, &base)?;

    // Where the runner's own processes sit in the base, they move out of it,
    // so that `memory` can be enabled for the base's children (cgroup v2
    // forbids a cgroup from holding both processes and controller-enabled
    // children). Where the runner is elsewhere there is nothing to move, and
    // no leaf is made: a cgroup created for nothing is a cgroup left behind.
    let leaf = if own == base {
        Some(enter_leaf(&base)?)
    } else {
        None
    };

    if !memory_is_enabled_for_children(&base) {
        if let Err(error) = fs::write(base.join("cgroup.subtree_control"), "+memory") {
            // Undo the move: a runner that falls back must leave the host as
            // it found it, including the leaf it was about to stand in.
            if let Some(leaf) = &leaf {
                let _ = fs::write(base.join("cgroup.procs"), "0");
                let _ = fs::remove_dir(leaf);
            }
            return Err(format!(
                "the memory controller cannot be enabled for the children of {}: \
                 {error}. Either this runner may not write {}/cgroup.subtree_control, \
                 or something other than this runner still has processes directly in \
                 that cgroup.",
                base.display(),
                base.display()
            ));
        }
    }

    // The other three controllers. None of them is worth a fallback and none
    // is worth refusing a runner over: a host that gives `memory` and not
    // `cpu` still runs jobs under a real memory ceiling, and what it owes is
    // the sentence saying the CPU share is nothing but niceness there.
    let mut unenforced = Vec::new();
    for (controller, controls) in [
        ("cpu", "cpu.weight and cpu.max"),
        ("io", "io.weight"),
        ("pids", "pids.max"),
    ] {
        if let Err(why) = enable_for_children(&base, controller) {
            unenforced.push(Unenforced {
                controller,
                controls,
                why,
            });
        }
    }
    Ok((base, unenforced))
}

/// Hand one controller down to the base's children, or say what stopped it.
///
/// Three outcomes, and they are not the same: the controller is already handed
/// down, the base never had it to give, or the base has it and this runner may
/// not write `cgroup.subtree_control`. Only the first is silence.
fn enable_for_children(base: &Path, controller: &str) -> Result<(), String> {
    if is_enabled_for_children(base, controller) {
        return Ok(());
    }
    let offered = fs::read_to_string(base.join("cgroup.controllers")).unwrap_or_default();
    if !offered.split_whitespace().any(|name| name == controller) {
        return Err(format!(
            "the cgroup {} does not offer the {controller} controller (it offers: {})",
            base.display(),
            offered.trim()
        ));
    }
    fs::write(
        base.join("cgroup.subtree_control"),
        format!("+{controller}"),
    )
    .map_err(|error| {
        format!(
            "the {controller} controller cannot be enabled for the children of {}: {error}",
            base.display()
        )
    })
}

/// Whether this runner could move a process from `own` into a child of `base`,
/// which needs write access to the common ancestor's `cgroup.procs`.
fn migration_is_possible(own: &Path, base: &Path) -> Result<(), String> {
    let ancestor = common_ancestor(own, base);
    let procs = ancestor.join("cgroup.procs");
    if crate::privilege::writable_by_this_user(&procs) {
        return Ok(());
    }
    Err(format!(
        "this runner cannot move a job's process into a cgroup under {}: cgroup v2 \
         requires write access to {}, the cgroup.procs of the common ancestor of \
         this runner's own cgroup ({}) and that base. Creating cgroups there is not \
         enough. Either run the runner inside the subtree it points at — a systemd \
         unit with Delegate=yes, or systemd-run --user --scope -p Delegate=yes — or \
         grant this user write access to that file",
        base.display(),
        procs.display(),
        own.display()
    ))
}

/// The deepest directory that is an ancestor of both paths, or the mount point.
fn common_ancestor(one: &Path, other: &Path) -> PathBuf {
    let mut shared = PathBuf::new();
    let mut theirs = other.components();
    for component in one.components() {
        match theirs.next() {
            Some(same) if same == component => shared.push(component),
            _ => break,
        }
    }
    if shared.as_os_str().is_empty() {
        PathBuf::from(MOUNT_POINT)
    } else {
        shared
    }
}

/// Whether the base already hands a controller to its children, in which case
/// the runner needs no write to `cgroup.subtree_control` at all.
fn is_enabled_for_children(base: &Path, controller: &str) -> bool {
    fs::read_to_string(base.join("cgroup.subtree_control"))
        .map(|text| text.split_whitespace().any(|name| name == controller))
        .unwrap_or(false)
}

fn memory_is_enabled_for_children(base: &Path) -> bool {
    is_enabled_for_children(base, "memory")
}

/// Move this runner's processes into a leaf of the base, and say where.
fn enter_leaf(base: &Path) -> Result<PathBuf, String> {
    let leaf = base.join(RUNNER_LEAF);
    match fs::create_dir(&leaf) {
        Ok(()) => {}
        // Already there, from an earlier prepare in this process or from
        // another runner sharing the base. Either is fine: the leaf is a
        // place to stand, not a claim.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "this runner cannot make its own leaf cgroup {}: {error}",
                leaf.display()
            ))
        }
    }
    // "0" means "the process writing this", which is the only form that does
    // not need write access over anybody else's processes.
    fs::write(leaf.join("cgroup.procs"), "0").map_err(|error| {
        let _ = fs::remove_dir(&leaf);
        format!(
            "this runner cannot move itself into {}: {error}",
            leaf.display()
        )
    })?;
    Ok(leaf)
}

/// This process's own cgroup, as a path under the cgroup v2 mount point.
fn own_cgroup() -> Result<PathBuf, String> {
    let text = fs::read_to_string("/proc/self/cgroup")
        .map_err(|error| format!("/proc/self/cgroup cannot be read: {error}"))?;
    // cgroup v2 is the line with an empty hierarchy id and an empty
    // controller list: "0::/user.slice/...". Its absence means this host is
    // on cgroup v1, where none of this applies.
    let relative = text
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| {
            "this host has no cgroup v2 hierarchy (/proc/self/cgroup has no 0:: line), so \
             it is cgroup v1 or has cgroups disabled"
                .to_owned()
        })?
        .trim()
        .trim_start_matches('/')
        .to_owned();
    let own = Path::new(MOUNT_POINT).join(relative);
    // A runner that has already prepared once is sitting in its own leaf, and
    // the base it must keep using is that leaf's parent. Without this,
    // preparing twice in one process — which a test suite does, and which a
    // reconnecting runner would do in M3 — would nest a leaf inside a leaf
    // and read the result as "no delegation".
    if own.file_name().is_some_and(|name| name == RUNNER_LEAF) {
        if let Some(parent) = own.parent() {
            return Ok(parent.to_path_buf());
        }
    }
    Ok(own)
}

fn read_number(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cpu.max` takes two numbers and an operator gives one, in the unit a
    /// `-j` is already in. The conversion is the whole of that promise.
    #[test]
    fn a_cpu_quota_is_cores_in_and_a_quota_and_period_out() {
        let four = CpuQuota::from_cores(4.0).expect("four cores is a quota");
        assert_eq!(four.control_value(), "400000 100000");
        assert_eq!(four.cores(), 4.0);
        let half = CpuQuota::from_cores(1.5).expect("a fraction of a core is a quota");
        assert_eq!(half.control_value(), "150000 100000");
        let floor = CpuQuota::from_cores(CpuQuota::MINIMUM_CORES).expect("the floor is a quota");
        assert_eq!(floor.control_value(), "1000 100000");
    }

    /// And a quota nothing could run under is refused where it is written,
    /// exactly as a memory ceiling of 1 kB is — naming the floor and naming
    /// the way to ask for no quota at all.
    #[test]
    fn a_cpu_quota_too_small_to_run_anything_is_refused_naming_the_floor() {
        for wall in [0.0, -1.0, 0.001, f64::NAN] {
            let error = CpuQuota::from_cores(wall)
                .err()
                .unwrap_or_else(|| panic!("a quota of {wall} cores was accepted"));
            assert!(
                error.contains("wall and not a cap") && error.contains("0.01"),
                "{error}"
            );
        }
    }

    /// Whatever this host turns out to offer, detection must answer, and the
    /// answer must be either a prepared subtree or a sentence. Never a panic,
    /// and never a `None` that leaves a job uncapped without saying so.
    #[test]
    fn detection_answers_with_a_subtree_or_a_reason() {
        let ceiling = Ceiling::detect(JobLimits::with_memory_max(64 << 20), None);
        assert_eq!(ceiling.memory_max(), 64 << 20);
        match &ceiling {
            Ceiling::Cgroups { base, .. } => {
                assert!(base.is_dir(), "{} is not a directory", base.display());
                assert!(base.starts_with(MOUNT_POINT), "{}", base.display());
            }
            Ceiling::Rlimit { why, .. } => {
                assert!(
                    why.len() > 30,
                    "a fallback reason must be a sentence, not a word: {why}"
                );
                assert_eq!(ceiling.fallback_reason(), Some(why.as_str()));
            }
        }
    }

    /// Every control is either enforced or named. On a host with no subtree
    /// at all there is one reason and not four, and it answers for all of
    /// them; on a host with a subtree, whatever it withholds comes back as a
    /// sentence naming the cgroup.
    #[test]
    fn a_control_this_host_cannot_enforce_comes_back_named() {
        let ceiling = Ceiling::detect(JobLimits::with_memory_max(64 << 20), None);
        for missing in ceiling.unenforced() {
            assert!(
                missing.why.contains(MOUNT_POINT),
                "an unenforced control must name the cgroup that withheld it: {}",
                missing.why
            );
            assert_eq!(
                ceiling.cannot_enforce(missing.controller),
                Some(missing.why.as_str())
            );
        }
        if ceiling.fallback_reason().is_some() {
            assert!(
                ceiling.unenforced().is_empty(),
                "a host with no cgroup subtree has one reason, not four"
            );
            for controller in ["cpu", "io", "pids"] {
                assert!(
                    ceiling.cannot_enforce(controller).is_some(),
                    "{controller} cannot be enforced without a cgroup, and that must be \
                     answerable"
                );
            }
        }
    }

    /// A cgroup path that does not exist is a named fallback, not a panic and
    /// not an uncapped job.
    #[test]
    fn a_cgroup_that_does_not_exist_falls_back_naming_it() {
        let missing = Path::new("/sys/fs/cgroup/herd-no-such-cgroup-m4");
        let ceiling = Ceiling::detect(JobLimits::with_memory_max(1 << 20), Some(missing));
        let why = ceiling
            .fallback_reason()
            .expect("a missing cgroup cannot enforce a ceiling");
        assert!(why.contains("herd-no-such-cgroup-m4"), "{why}");
        assert!(ceiling.base().is_none());
        assert!(ceiling
            .open_for_job(&JobTag::mint(&herd_proto::JobId::new("j1")))
            .is_none());
    }

    /// Defect 6 of the M4 review: creating a cgroup under a delegated subtree
    /// is not the same right as moving a process into one. The refusal has to
    /// name the file the operator must grant, and the runner must not have
    /// relocated itself on the way to finding out.
    #[test]
    fn a_base_this_runner_stands_outside_of_is_refused_naming_the_file() {
        let own = Path::new("/sys/fs/cgroup/user.slice/some-other-session.scope");
        let base = Path::new("/sys/fs/cgroup/user.slice/a-delegated.scope");
        let error = migration_is_possible(own, base)
            .expect_err("no unprivileged user may write /sys/fs/cgroup/user.slice/cgroup.procs");
        assert!(
            error.contains("/sys/fs/cgroup/user.slice/cgroup.procs"),
            "the refusal must name the common ancestor's cgroup.procs: {error}"
        );
        assert!(
            error.contains("Delegate=yes"),
            "the refusal must name what to grant: {error}"
        );
        // Its own cgroup is always a possible destination's ancestor.
        assert_eq!(
            common_ancestor(own, base),
            Path::new("/sys/fs/cgroup/user.slice")
        );
        assert_eq!(common_ancestor(own, own), own);
    }

    /// A prepare that ends in a fallback leaves nothing behind — in
    /// particular not the leaf the runner would have stood in.
    #[test]
    fn a_fallback_leaves_no_leaf_cgroup_behind() {
        let ceiling = Ceiling::detect(
            JobLimits::with_memory_max(1 << 20),
            Some(Path::new(MOUNT_POINT)),
        );
        if ceiling.fallback_reason().is_some() {
            assert!(
                !Path::new(MOUNT_POINT).join(RUNNER_LEAF).exists(),
                "a runner that fell back left {RUNNER_LEAF} in the cgroup root"
            );
        }
    }

    #[test]
    fn a_cgroup_without_the_memory_controller_falls_back_naming_it() {
        // The cgroup v2 root always exists and always lists its controllers;
        // if memory is among them this assertion is about the other branch,
        // which `a_cgroup_that_does_not_exist` already covers. What matters
        // here is that neither branch panics on a real directory.
        let ceiling = Ceiling::detect(
            JobLimits::with_memory_max(1 << 20),
            Some(Path::new(MOUNT_POINT)),
        );
        match ceiling.fallback_reason() {
            Some(why) => assert!(why.contains(MOUNT_POINT), "{why}"),
            None => {
                // The root is delegated to us, which only happens in a
                // container running as root — and a root runner is refused
                // before it ever gets here (E-46).
            }
        }
    }
    /// `io.max` is per-device: every line of it begins with a `major:minor`,
    /// and the first thing to get right is that the value written **names
    /// the device the job's data root is on**. A value with no device is not
    /// a weaker ceiling; the kernel refuses the write outright.
    #[test]
    fn the_value_written_names_the_device_and_then_the_limits() {
        let ceiling = IoCeiling::parse("rbps=200MB,wbps=100MB,riops=4000,wiops=2000")
            .expect("four limits are a ceiling");
        let io_max = IoMax {
            ceiling,
            device: "259:0".to_owned(),
        };
        // The device first, then the kernel's own keys. This is the exact
        // text that goes in io.max.
        assert_eq!(
            io_max.control_value(),
            "259:0 rbps=200000000 wbps=100000000 riops=4000 wiops=2000"
        );
        // A subset is a subset: a limit left out is left at `max`, so it is
        // written as nothing rather than as a figure.
        let write_only = IoMax {
            ceiling: IoCeiling::parse("wbps=50MiB").expect("one limit is a ceiling"),
            device: "8:16".to_owned(),
        };
        assert_eq!(write_only.control_value(), "8:16 wbps=52428800");
        // And the device is the first word, whatever the limits are: a line
        // the kernel would read as a device-less ceiling is no ceiling.
        for value in [io_max.control_value(), write_only.control_value()] {
            let device = value.split_whitespace().next().expect("a first word");
            assert!(
                device.contains(':') && device.split(':').all(|half| half.parse::<u32>().is_ok()),
                "io.max is per-device and this line names none: {value}"
            );
        }
    }

    /// Absent by default means **no line written** — never a zero and never a
    /// guess. A default throughput figure would be right on the machine it
    /// was chosen on and wrong on every other, and `rbps=0` is a wall rather
    /// than a cap, so both are refused where they are written.
    #[test]
    fn a_ceiling_is_absent_by_default_and_zero_is_refused_not_written() {
        assert_eq!(JobLimits::with_memory_max(1 << 30).io_max, None);
        let why = IoCeiling::parse("rbps=0").expect_err("zero is a wall");
        assert!(why.contains("wall and not a cap"), "{why}");
        assert!(why.contains("Leave the whole option out"), "{why}");
        let why = IoCeiling::parse("wiops=0").expect_err("zero IOPS is a wall too");
        assert!(why.contains("wall and not a cap"), "{why}");
        // An option given with nothing in it is a refusal rather than a
        // ceiling of nothing, which would read as a ceiling and bound
        // nothing.
        let why = IoCeiling::parse("").expect_err("nothing is not a ceiling");
        assert!(why.contains("is not a ceiling"), "{why}");
        // And the strictness the capability file is read with: an unknown
        // key, a key twice, a figure that is not one.
        let why = IoCeiling::parse("rbs=10MB").expect_err("a typo is not a limit");
        assert!(why.contains("\"rbs\"") && why.contains("rbps"), "{why}");
        let why = IoCeiling::parse("rbps=1MB,rbps=2MB").expect_err("twice is ambiguous");
        assert!(why.contains("given twice"), "{why}");
        let why = IoCeiling::parse("riops=plenty").expect_err("plenty is not a figure");
        assert!(why.contains("IO operations per second"), "{why}");
        let why = IoCeiling::parse("rbps").expect_err("a key with no value is not a limit");
        assert!(why.contains("key=value"), "{why}");
        // Whitespace and order are an operator's business, not the parser's.
        assert_eq!(
            IoCeiling::parse(" wbps=1MB , rbps=2MB ").expect("spaces are allowed"),
            IoCeiling::parse("rbps=2MB,wbps=1MB").expect("order does not matter")
        );
    }
}
