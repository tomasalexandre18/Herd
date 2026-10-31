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
//! ## What else is standing in the base (2026-10-30)
//!
//! Moving *itself* out is not enough, because the rule is about the cgroup
//! and not about this process: **anything at all directly in the base
//! refuses the write**, with `EBUSY`. A runner that read that refusal as "I
//! may not write `cgroup.subtree_control`" fell back to `RLIMIT_AS` — a
//! per-process address-space limit where a job-tree ceiling was promised,
//! which is invariant 2 degrading without a word.
//!
//! Measured in a delegated scope that day, the test binary alone in its base
//! at `exec`: at twelve test threads two assertions skipped on `EBUSY (os
//! error 16)` in two runs of three and three in the third; at one thread none
//! skipped. Two controls, one variable, and the variable is concurrency —
//! but **not** the concurrency of two `prepare` calls, which was the first
//! reading and was wrong. Every `prepare` moves out before it writes, so the
//! process itself is never directly in the base at a write. What was in the
//! base was a **real `herd-runner` binary a test had spawned**, forked out of
//! the base before any `prepare` left it and so standing directly in it:
//! `cgroup.procs` listed `3175023 (herd-runner)`. That is the production
//! shape the defect was reported for — two runners sharing one delegated base
//! — and at one thread it did not happen because the first `prepare` came
//! before the first fork.
//!
//! So an `EBUSY` is **read** rather than guessed: `cgroup.procs` says who is
//! standing in the base, `/proc` says whose each of them is, and the answer
//! decides what happens. This process and the processes it started are its
//! own to move, and they go into the leaf; a process that is nobody's of this
//! runner's is a refusal naming its pid, because nothing the runner does next
//! would move it and a retry would be a spin against a condition that will
//! not change. A `cgroup.procs` that will not read is reported as a refusal
//! this runner cannot explain, which is this project's rule about an absence.
//! Then one retry, never a loop.
//!
//! A job's process is never one of the ones moved: a job is written into its
//! own cgroup between fork and exec ([`JobCgroup::procs_file`]), so it is
//! never directly in the base.
//!
//! **No lock was added, and that is a decision.** Two `prepare` calls in one
//! process were the first explanation and the measurement refuted it, so a
//! mutex here would guard a window nobody has seen. There is one window a
//! lock would have shut: the fallback below moves this process **back into
//! the base** to leave the host as it found it, and another `prepare` writing
//! `cgroup.subtree_control` in that instant meets an `EBUSY` that is nothing
//! to do with its own host. The reading above already answers it — the
//! occupant is this process, which is this process's to move — so the lock
//! would be redundant as well as unmeasured.
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
//!
//! ## A controller is read, never inferred from a file being there
//!
//! The reading above has a sibling, and it is the same mistake in a smaller
//! place: **a file named after a controller proves nothing about that
//! controller.** `io.pressure` is in a cgroup whether or not `io` is among
//! its controllers — PSI is not controller-gated — and measured on this
//! project's host on 2026-10-30, `app.slice` holds `io.pressure` while `io`
//! is absent from its `cgroup.controllers`. A reading that answered "the
//! controller is handed down" from an `io.*` file would answer *yes* for a
//! cgroup where `io` had never been delegated at all.
//!
//! So "which controls can a job's cgroup really carry" is answered by a
//! controller list and by nothing else: `cgroup.subtree_control` for what a
//! cgroup hands to its children ([`is_enabled_for_children`]), and
//! `cgroup.controllers` for what a cgroup has been handed — which is what
//! [`Ceiling::prove_io_max`] reads in the probe cgroup that stands in for a
//! job's. Until 2026-10-30 that proof instead read whether `<probe>/io.max`
//! *existed*, to tell "this kernel has no blk-throttle" from "the `io`
//! controller is not handed down": one sentence for two different hosts,
//! with a diagnosis resting on a file. **Neither refusal changed class when
//! that was fixed.** An `io.max` is a ceiling an operator *asked* for, so
//! every way it cannot be enforced is a startup refusal, before and after;
//! `io.weight` is unasked-for and stays absent and declared.

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

/// Where the process table is, so that a pid out of `cgroup.procs` can be
/// asked whose it is. Passed as an argument below rather than read from here,
/// for the reason [`crate::room`] gives about its own walk: a reading has to
/// be assertable against a tree a test builds.
const PROC: &str = "/proc";

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
    /// for — in a probe cgroup under the same base every job cgroup will hang
    /// under, by reading which controllers that cgroup was handed and then
    /// writing the real value into it.
    ///
    /// Done rather than inferred, exactly as delegation is
    /// ([`prepare_base`]'s probe). Three things could each make a ceiling
    /// vanish, and they are three different hosts:
    ///
    /// - the `io` controller is not handed down to the base's children. That
    ///   one **is** readable, and only from the one file that says it:
    ///   `<probe>/cgroup.controllers`, the controllers that cgroup was given.
    ///   Not from `<probe>/io.max` being there — see the module's *A
    ///   controller is read, never inferred from a file being there*;
    /// - the kernel was built without blk-throttle, so `io.max` is not an
    ///   interface file at all and the write finds nothing to write to;
    /// - `io.max` is there and the kernel refuses *this device*.
    ///
    /// The controller list answers the first before anything is written, and
    /// the write answers the other two, which is why both are here. The
    /// answer has to be a refusal, because a ceiling quietly weaker than
    /// asked for is the wrong failure on a machine where one job can
    /// saturate the disk (D116, D129).
    pub fn prove_io_max(&self, io_max: &IoMax) -> Result<(), String> {
        self.prove_io_max_through(io_max, Kernel::real())
    }

    /// [`Ceiling::prove_io_max`] with the two kernel rules it depends on
    /// named, so that a test can meet them in a tree it builds: making a
    /// cgroup populates it with the interface files its parent's
    /// `cgroup.subtree_control` dictates, and cgroupfs creates no file a
    /// write names — `ENOENT` is the whole of "this kernel has no
    /// blk-throttle" as a runner can see it. The reason is
    /// [`enable_for_children_through`]'s: our account gets no cgroup v2
    /// delegation on this host, so a test of the real kernel path skips and a
    /// skip is not a proof (D27).
    fn prove_io_max_through(&self, io_max: &IoMax, kernel: Kernel<'_>) -> Result<(), String> {
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
        (kernel.mkdir)(&probe).map_err(|error| {
            format!(
                "no cgroup could be made under {} to try the ceiling in ({error})",
                base.display()
            )
        })?;
        let proved = prove_io_max_in(&probe, base, io_max, kernel);
        // Before the answer is returned, and whatever the answer is: a probe
        // left behind is a cgroup nothing will collect.
        let _ = fs::remove_dir(&probe);
        proved
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

/// The two counters `memory.events` keeps about out-of-memory kills, read
/// together because only one of them can answer *which step* a kill belongs
/// to.
///
/// `oom_kill` counts **processes**. With `memory.oom.group = 1` written on
/// every job cgroup ([`JobCgroup::create`]) one kill event takes the whole
/// job tree, and the kernel bumps this counter once per task as it walks
/// them. The walk is not instantaneous — the first task's death is what wakes
/// the runner up, while the rest are still being counted — so a reading taken
/// the moment a step's child is reaped can be **mid-burst**, and the
/// remainder lands in whatever is read next. That is how one kill came to be
/// charged to two steps.
///
/// `oom_group_kill` counts **events**: the kernel bumps it once, before the
/// first task of the group is signalled, so by the time any death can be
/// observed it is already settled. It is the counter that matches the flag
/// this runner itself sets, and it is what a step's verdict is read from
/// wherever the kernel publishes it (Linux 5.17 and later). `None` says this
/// kernel does not, and then the per-process counter is all there is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OomKills {
    /// `oom_kill`: processes the kernel has killed here, cumulative.
    pub processes: u64,
    /// `oom_group_kill`: group kills, one per event — or `None` on a kernel
    /// that does not publish the field.
    pub group_kills: Option<u64>,
}

/// [`JobCgroup::oom_kills`] over the text, so that the reading is provable
/// without a cgroup to read it from.
///
/// A field that is absent and a field that will not parse are one answer for
/// `oom_group_kill`, and that answer is `None`: both mean this kernel offers
/// nothing better than the per-process counter.
pub fn oom_kills_in(text: &str) -> OomKills {
    let field = |name: &str| {
        text.lines()
            .filter_map(|line| line.strip_prefix(name))
            .filter_map(|count| count.trim().parse::<u64>().ok())
            .next()
    };
    OomKills {
        processes: field("oom_kill ").unwrap_or(0),
        group_kills: field("oom_group_kill "),
    }
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

    /// What the kernel has killed in this cgroup for going past the ceiling.
    /// Non-zero is what turns a SIGKILL into a named outcome instead of
    /// "signalled by something".
    ///
    /// A `memory.events` nobody could read reads as zero, which is why the
    /// baseline a step's verdict is judged against never moves backwards
    /// ([`crate::job`]'s own watermark): a single unreadable step would
    /// otherwise charge the next one with every kill the job ever had.
    pub fn oom_kills(&self) -> OomKills {
        match fs::read_to_string(self.dir.join("memory.events")) {
            Ok(text) => oom_kills_in(&text),
            Err(_) => OomKills::default(),
        }
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

    // The same enable as the three controllers below, and the same reading of
    // an `EBUSY`. Memory is the one that decides between a kernel ceiling and
    // the `RLIMIT_AS` fallback, which is why the race was measured here first
    // and why a guessed refusal here is invariant 2 going quiet.
    if let Err(why) = enable_for_children(&base, "memory", leaf.as_deref()) {
        // Undo the move: a runner that falls back must leave the host as
        // it found it, including the leaf it was about to stand in.
        if let Some(leaf) = &leaf {
            let _ = fs::write(base.join("cgroup.procs"), "0");
            let _ = fs::remove_dir(leaf);
        }
        return Err(why);
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
        if let Err(why) = enable_for_children(&base, controller, leaf.as_deref()) {
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
/// Four outcomes, and they are not the same: the controller is already handed
/// down, the base never had it to give, the base has it and this runner may
/// not write `cgroup.subtree_control`, or the base has it and **something is
/// standing directly in the base**. Only the first is silence.
///
/// That last one is `EBUSY`, and it is the one this function has to be
/// careful about, because the thing standing in the base is usually the
/// runner itself. See the module's *Two prepares at once*: the refusal is
/// read rather than guessed, out of `cgroup.procs`, and a retry happens only
/// where the reading says the base is this runner's own to clear.
fn enable_for_children(base: &Path, controller: &str, leaf: Option<&Path>) -> Result<(), String> {
    enable_for_children_through(base, controller, leaf, Kernel::real())
}

/// [`enable_for_children`] with the two kernel rules it depends on named, so
/// that a test can meet them in a tree it builds.
///
/// The host this is written on gives our account no delegated cgroup outside
/// a `systemd-run --user --scope`, so a test of the real kernel path skips
/// here and a skip is not a proof (D27). What a test *can* do is what
/// [`crate::room`] does with its walk: build the tree, and let the code under
/// test read and write it. The difference is that this path depends on the
/// kernel *refusing* a write, which no file in a temporary directory will do
/// — so the write goes through [`Kernel`], and the fixture's writer enforces
/// cgroup v2's two rules itself: a controller cannot be handed to the
/// children of a cgroup that holds processes, and writing `0` to a
/// `cgroup.procs` moves this process there from wherever it was.
fn enable_for_children_through(
    base: &Path,
    controller: &str,
    leaf: Option<&Path>,
    kernel: Kernel<'_>,
) -> Result<(), String> {
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
    let control = base.join("cgroup.subtree_control");
    let plus = format!("+{controller}");
    let refused = match (kernel.write)(&control, &plus) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    // Anything but EBUSY is not the no-internal-process rule and must not be
    // described as if it were: a permission failure is EACCES, a controller
    // that is not there is ENOENT, and neither has anything to do with who is
    // standing in the cgroup.
    if refused.raw_os_error() != Some(libc::EBUSY) {
        return Err(format!(
            "the {controller} controller cannot be enabled for the children of {}: {refused}",
            base.display()
        ));
    }
    let occupancy = Occupancy::read(base, kernel);
    // An absence is reported as an absence: a runner that cannot read
    // `cgroup.procs` does not know whose the processes in that cgroup are,
    // and must not guess either way.
    if let Some(why) = &occupancy.unreadable {
        return Err(format!(
            "the {controller} controller cannot be enabled for the children of {}: {refused}. \
             cgroup v2 refuses that write when processes sit directly in the cgroup, and {} \
             cannot be read ({why}) — so this runner cannot say whether those processes are \
             its own or somebody else's, only that the kernel says there are some",
            base.display(),
            base.join("cgroup.procs").display()
        ));
    }
    // A process that is nobody's of this runner's is a refusal and never a
    // retry: nothing this runner does next will move it, so a retry would be
    // a spin against a condition that is not going to change. Checked
    // **before** anything is moved, so that a base this runner is not going
    // to get is a base it leaves exactly as it found it.
    let strangers = occupancy.strangers();
    if !strangers.is_empty() {
        return Err(format!(
            "the {controller} controller cannot be enabled for the children of {}: {refused}. \
             cgroup v2 refuses that write while processes sit directly in the cgroup, and {} \
             lists {}, which is no process of this runner's — so this base is shared with \
             something else. Give the runner a base cgroup of its own — a systemd unit with \
             Delegate=yes, or systemd-run --user --scope -p Delegate=yes — or move that \
             process out of it",
            base.display(),
            base.join("cgroup.procs").display(),
            Occupancy::name_them(&strangers)
        ));
    }
    // Everything left standing in the base is this runner's own: itself, not
    // yet migrated, or a process it started before it migrated. Both are its
    // to move and the leaf is where they go — a job's process never is one of
    // them, because a job is written into its own cgroup between fork and
    // exec ([`JobCgroup::procs_file`]) and is never directly in the base.
    let mine = occupancy.mine();
    if !mine.is_empty() {
        let Some(leaf) = leaf else {
            return Err(format!(
                "the {controller} controller cannot be enabled for the children of {}: \
                 {refused}. {} of this runner's own is directly in that cgroup, and this \
                 runner has no leaf of its own to move it into — it had read its own cgroup \
                 as something other than {}",
                base.display(),
                Occupancy::name_them(&mine),
                base.display()
            ));
        };
        for one in &mine {
            // `0` for this process itself, which is the only form that needs
            // no write access over anybody else's processes; a pid for a
            // process this runner started, which it may move because it
            // started it and because the base is delegated to it.
            let who = if one.whose == Whose::ThisRunner {
                "0".to_owned()
            } else {
                one.pid.to_string()
            };
            (kernel.write)(&leaf.join("cgroup.procs"), &who).map_err(|error| {
                format!(
                    "the {controller} controller cannot be enabled for the children of {}: \
                     {refused}, because {} of this runner's own is still directly in it — \
                     and it could not be moved into {}: {error}",
                    base.display(),
                    Occupancy::name_them(std::slice::from_ref(one)),
                    leaf.display()
                )
            })?;
        }
    }
    // Another prepare in another process may have enabled it while this one
    // was reading, in which case there is nothing left to write.
    if is_enabled_for_children(base, controller) {
        return Ok(());
    }
    // Once, never in a loop: the condition that made the retry honest has
    // been acted on, so a second refusal is a refusal and not a moment to
    // wait out.
    (kernel.write)(&control, &plus).map_err(|again| {
        format!(
            "the {controller} controller cannot be enabled for the children of {}: {again}. \
             Everything directly in that cgroup was this runner's own, it moved all of it \
             into its leaf and tried once more, and the kernel refused again — so something \
             arrived in that cgroup between the two writes, or the refusal is not about who \
             is standing there",
            base.display()
        )
    })
}

/// What this module needs from the kernel, so that every one of them can be
/// met by a tree a test builds: where `/proc` is, which pid this process is,
/// what writing a cgroup control file does, and what making a cgroup does.
#[derive(Clone, Copy)]
struct Kernel<'a> {
    /// Where the process table is mounted. `/proc` in a runner.
    proc_root: &'a Path,
    /// This process's own pid, as `cgroup.procs` writes it.
    me: i32,
    /// Writing a cgroup control file. `fs::write` in a runner; in a fixture,
    /// cgroup v2's own rules over a directory tree.
    write: fn(&Path, &str) -> io::Result<()>,
    /// Making a cgroup. `fs::create_dir` in a runner, where the kernel then
    /// populates the new cgroup with the interface files its parent's
    /// `cgroup.subtree_control` dictates; in a fixture, that population is
    /// what the fake has to do itself, because an empty directory would hand
    /// [`prove_io_max_in`] a cgroup no kernel ever makes.
    mkdir: fn(&Path) -> io::Result<()>,
}

impl Kernel<'static> {
    /// This host's own kernel.
    fn real() -> Self {
        Self {
            proc_root: Path::new(PROC),
            me: std::process::id() as i32,
            write: |path, value| fs::write(path, value),
            mkdir: |path| fs::create_dir(path),
        }
    }
}

/// Whose a process standing directly in the base is.
///
/// The distinction is the whole point: an `EBUSY` caused by a process this
/// runner started, or by its own un-migrated self, is a thing the runner can
/// fix in the next instruction, and an `EBUSY` caused by anything else is a
/// refusal. A message that named only the second — "something other than this
/// runner still has processes directly in that cgroup", which is what this
/// said until 2026-10-30 — sent a reader looking for a third party that, in
/// the one case ever measured, did not exist: the something-other was the
/// runner's own child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whose {
    /// This very process: the runner has not moved itself out yet.
    ThisRunner,
    /// A descendant of this process, started before the runner moved out.
    ItsChild,
    /// Neither this process nor a descendant of it.
    Foreign,
    /// Listed in `cgroup.procs` and gone from the process table by the time
    /// it was looked up. It is not in the way of anything.
    Gone,
}

/// One process `cgroup.procs` listed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Occupant {
    pid: i32,
    /// What `/proc/<pid>/stat` calls it, so a refusal names something a
    /// reader can go and look at. Empty for a process that has gone.
    name: String,
    whose: Whose,
}

/// Who is standing directly in a cgroup, read out of `cgroup.procs` and the
/// process table rather than assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Occupancy {
    occupants: Vec<Occupant>,
    /// Why `cgroup.procs` could not be read, when it could not. An unreadable
    /// occupancy is **not** an empty one, and nothing here lets the two be
    /// confused.
    unreadable: Option<String>,
}

impl Occupancy {
    /// Read `base/cgroup.procs`, and for each pid in it ask the process table
    /// whose it is.
    fn read(base: &Path, kernel: Kernel<'_>) -> Self {
        let procs = base.join("cgroup.procs");
        let text = match fs::read_to_string(&procs) {
            Ok(text) => text,
            Err(error) => {
                return Self {
                    occupants: Vec::new(),
                    unreadable: Some(error.to_string()),
                }
            }
        };
        let occupants = text
            .split_whitespace()
            .filter_map(|word| word.parse::<i32>().ok())
            .map(|pid| {
                let Some((name, _)) = process_entry(kernel.proc_root, pid) else {
                    return Occupant {
                        pid,
                        name: String::new(),
                        whose: Whose::Gone,
                    };
                };
                let whose = if pid == kernel.me {
                    Whose::ThisRunner
                } else if descends_from(kernel.proc_root, pid, kernel.me) {
                    Whose::ItsChild
                } else {
                    Whose::Foreign
                };
                Occupant { pid, name, whose }
            })
            .collect();
        Self {
            occupants,
            unreadable: None,
        }
    }

    /// The processes this runner may move: itself, and the ones it started.
    /// A process that has gone is in nobody's way and is not among them.
    fn mine(&self) -> Vec<Occupant> {
        self.occupants
            .iter()
            .filter(|one| matches!(one.whose, Whose::ThisRunner | Whose::ItsChild))
            .cloned()
            .collect()
    }

    /// The processes this runner may not touch, which are the ones that turn
    /// an `EBUSY` into a refusal rather than a retry.
    fn strangers(&self) -> Vec<Occupant> {
        self.occupants
            .iter()
            .filter(|one| one.whose == Whose::Foreign)
            .cloned()
            .collect()
    }

    /// Name them the way a refusal has to: pid and the name the process table
    /// gives, so a reader has something to go and look at.
    fn name_them(occupants: &[Occupant]) -> String {
        occupants
            .iter()
            .map(|one| format!("{} ({})", one.pid, one.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The name and parent of one process, or `None` for a pid the process table
/// does not have.
///
/// `/proc/<pid>/stat` is read whole rather than by field index from the left:
/// the second field is the command in parentheses and a command may contain
/// both spaces and a closing parenthesis, so the only safe split is at the
/// **last** `)` in the line.
fn process_entry(proc_root: &Path, pid: i32) -> Option<(String, i32)> {
    let text = fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_owned();
    // After the `)` come the state and then the parent's pid.
    let mut rest = text.get(close + 1..)?.split_whitespace();
    let _state = rest.next()?;
    let parent = rest.next()?.parse().ok()?;
    Some((name, parent))
}

/// Whether `pid` has `ancestor` somewhere above it in the process table.
///
/// Bounded, because a walk up a table that is being changed underneath the
/// reader must end whatever it reads.
fn descends_from(proc_root: &Path, pid: i32, ancestor: i32) -> bool {
    let mut at = pid;
    for _ in 0..64 {
        let Some((_, parent)) = process_entry(proc_root, at) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        if parent <= 1 {
            return false;
        }
        at = parent;
    }
    false
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

/// What a probe cgroup says about an `io.max` ceiling, once there is one.
///
/// Split out of [`Ceiling::prove_io_max_through`] so that every road out of
/// here — the two refusals and the proof alike — passes the probe's removal.
fn prove_io_max_in(
    probe: &Path,
    base: &Path,
    io_max: &IoMax,
    kernel: Kernel<'_>,
) -> Result<(), String> {
    // Which controllers this cgroup was handed, out of the one file that says
    // so. An absence is reported as an absence, as everywhere else here: a
    // runner that cannot read the list does not know what the cgroup can
    // carry, and must not read it off whatever else is lying in there.
    let listing = probe.join("cgroup.controllers");
    let handed_down = fs::read_to_string(&listing).map_err(|error| {
        format!(
            "{} cannot be read ({error}), so nothing here can say which controllers a \
             job's cgroup under {} is handed — and an io.max is a ceiling only where the \
             io controller is one of them",
            listing.display(),
            base.display()
        )
    })?;
    if !handed_down.split_whitespace().any(|name| name == "io") {
        return Err(format!(
            "the io controller is not handed down to the children of {}: a cgroup made \
             there is handed {:?}, and io is not among them, so an io.max written there \
             would bound nothing. An io.* file in that cgroup proves nothing about the \
             controller — io.pressure is there either way. Run this runner as a systemd \
             unit with Delegate=yes, or inside systemd-run --user --scope -p \
             Delegate=yes, so that io is delegated to it",
            base.display(),
            handed_down.trim()
        ));
    }
    let control = probe.join("io.max");
    // The controller is handed down, so the two readings left are the
    // kernel's own: cgroupfs creates no file a write names, so `ENOENT` here
    // is `io.max` not being an interface file on this kernel at all, and
    // anything else is the kernel refusing this value for this device.
    (kernel.write)(&control, &io_max.control_value()).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "the io controller is handed down to the children of {} and {} is still \
                 not there, so this kernel has no per-cgroup IO throttling to hold a \
                 ceiling with (CONFIG_BLK_DEV_THROTTLING): {error}",
                base.display(),
                control.display()
            )
        } else {
            format!(
                "the kernel refused {:?} in {}: {error}",
                io_max.control_value(),
                control.display()
            )
        }
    })?;
    // Read back, because a write the kernel took and did not keep would
    // otherwise read as a proof.
    let Ok(readable) = fs::read_to_string(&control) else {
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
            "the kernel took the ceiling for device {} and then listed none: {} reads {:?}",
            io_max.device,
            control.display(),
            readable.trim()
        ));
    }
    Ok(())
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
    use crate::testing::TempDir;

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

    // ------------------------------------------- the first-enable race ----

    /// A base cgroup with its leaf, as a runner's own delegated subtree
    /// looks: the controllers it offers, nothing handed down yet, and the
    /// pids standing directly in it.
    ///
    /// A tree rather than a scope, for [`crate::room`]'s reason: this host
    /// gives our account no delegated cgroup outside `systemd-run --user
    /// --scope`, so a test of the real kernel path would skip here and a skip
    /// is not a proof (D27). What this host *can* do is let the code read and
    /// write a tree, and [`fake_kernel`] supplies the one thing a temporary
    /// directory will not: the refusal.
    fn fake_base(root: &Path, in_the_base: &[i32]) -> PathBuf {
        let base = root.join("base");
        fs::create_dir_all(base.join(RUNNER_LEAF)).expect("a base and its leaf");
        fs::write(base.join("cgroup.controllers"), "cpu io memory pids")
            .expect("a writable cgroup.controllers");
        fs::write(base.join("cgroup.subtree_control"), "")
            .expect("a writable cgroup.subtree_control");
        fs::write(
            base.join("cgroup.procs"),
            in_the_base
                .iter()
                .map(|pid| pid.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("a writable cgroup.procs");
        fs::write(base.join(RUNNER_LEAF).join("cgroup.procs"), "")
            .expect("a writable leaf cgroup.procs");
        base
    }

    /// A process table: one `/proc/<pid>/stat` per process, written the way
    /// the kernel writes it — the command in parentheses, the state, then the
    /// parent's pid.
    fn fake_proc(root: &Path, processes: &[(i32, &str, i32)]) -> PathBuf {
        let proc_root = root.join("proc");
        for (pid, name, parent) in processes {
            let dir = proc_root.join(pid.to_string());
            fs::create_dir_all(&dir).expect("a process directory");
            fs::write(
                dir.join("stat"),
                format!("{pid} ({name}) S {parent} {parent} 0 0 -1 4194560"),
            )
            .expect("a writable stat");
        }
        proc_root
    }

    /// Where the fixture counts the writes to `cgroup.subtree_control`, so
    /// that "retried once" and "not retried at all" are different readings
    /// and not the same green. Beside the tree rather than in a static: this
    /// crate's tests run in one process on twelve threads, and a static
    /// counter would be shared by every one of them.
    fn subtree_writes(base: &Path) -> usize {
        fs::read_to_string(
            base.parent()
                .expect("a base has a root")
                .join("subtree-writes"),
        )
        .map(|text| text.lines().count())
        .unwrap_or(0)
    }

    fn note_a_subtree_write(base: &Path, value: &str) {
        use std::io::Write;
        let path = base
            .parent()
            .expect("a base has a root")
            .join("subtree-writes");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("a writable counter");
        file.write_all(format!("{value}\n").as_bytes())
            .expect("a counted write");
    }

    /// cgroup v2's two rules over a directory tree, which is what makes the
    /// race deterministic here instead of hoped for:
    ///
    /// - a controller cannot be handed to the children of a cgroup that holds
    ///   processes — the write is refused with `EBUSY`, which is the whole
    ///   defect;
    /// - writing a pid to a `cgroup.procs` moves that process there, out of
    ///   wherever it was, and `0` means the process doing the writing.
    ///
    /// Nothing is captured, because this is a plain `fn` pointer: `0` is this
    /// process's own pid, exactly as the code under test writes it.
    fn fake_kernel(path: &Path, value: &str) -> io::Result<()> {
        let dir = path.parent().expect("a control file sits in a cgroup");
        match path.file_name().and_then(|name| name.to_str()) {
            Some("cgroup.subtree_control") => {
                note_a_subtree_write(dir, value);
                let holders = fs::read_to_string(dir.join("cgroup.procs")).unwrap_or_default();
                if holders.split_whitespace().next().is_some() {
                    return Err(io::Error::from_raw_os_error(libc::EBUSY));
                }
                let mut enabled = fs::read_to_string(path).unwrap_or_default();
                enabled.push(' ');
                enabled.push_str(value.trim_start_matches('+'));
                fs::write(path, enabled.trim())
            }
            Some("cgroup.procs") => {
                let moved = if value == "0" {
                    std::process::id().to_string()
                } else {
                    value.to_owned()
                };
                // Nothing is taken out of anywhere until the destination has
                // been written, because the kernel's own migration is one
                // step and a fixture that moved a process into nowhere would
                // be a weaker rule than the real one.
                let mut here = fs::read_to_string(path).unwrap_or_default();
                here.push('\n');
                here.push_str(&moved);
                fs::write(path, here.trim())?;
                let without_it = |procs: &Path| {
                    if procs == path {
                        return;
                    }
                    let kept = fs::read_to_string(procs)
                        .unwrap_or_default()
                        .split_whitespace()
                        .filter(|pid| *pid != moved)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = fs::write(procs, kept);
                };
                if let Some(parent) = dir.parent() {
                    without_it(&parent.join("cgroup.procs"));
                }
                Ok(())
            }
            other => panic!("the enable wrote {other:?}, which is not a cgroup control file"),
        }
    }

    /// The same two rules, with the **first** write to `cgroup.subtree_control`
    /// refused whatever is standing in the cgroup: the stale refusal, where
    /// whatever was in the base has already left by the time it is read.
    fn fake_kernel_refusing_once(path: &Path, value: &str) -> io::Result<()> {
        let dir = path.parent().expect("a control file sits in a cgroup");
        if path.file_name().and_then(|name| name.to_str()) == Some("cgroup.subtree_control") {
            let marker = dir.join("refused-once");
            if !marker.exists() {
                fs::write(&marker, "").expect("a writable marker");
                note_a_subtree_write(dir, value);
                return Err(io::Error::from_raw_os_error(libc::EBUSY));
            }
        }
        fake_kernel(path, value)
    }

    /// The same two rules, with every write to `cgroup.subtree_control`
    /// refused: for the one case where the refusal cannot be explained at
    /// all, because `cgroup.procs` will not read.
    fn fake_kernel_always_refusing(path: &Path, value: &str) -> io::Result<()> {
        let dir = path.parent().expect("a control file sits in a cgroup");
        if path.file_name().and_then(|name| name.to_str()) == Some("cgroup.subtree_control") {
            note_a_subtree_write(dir, value);
            return Err(io::Error::from_raw_os_error(libc::EBUSY));
        }
        fake_kernel(path, value)
    }

    fn fake_kernel_at(proc_root: &Path, write: fn(&Path, &str) -> io::Result<()>) -> Kernel<'_> {
        Kernel {
            proc_root,
            me: std::process::id() as i32,
            write,
            mkdir: |path| fs::create_dir(path),
        }
    }

    /// This runner's own self, still in the base when the write goes out.
    /// cgroup v2's no-internal-process rule refuses it with `EBUSY`, and
    /// until 2026-10-30 the runner read that as "I may not write
    /// `cgroup.subtree_control`" and fell back to `RLIMIT_AS` — a per-process
    /// address-space limit where a job-tree ceiling was promised.
    ///
    /// So the `EBUSY` is read: the only thing in the base is this runner
    /// itself, which it can move, so it moves and tries once more. Exactly
    /// two writes, and the second one is taken.
    #[test]
    fn an_ebusy_caused_by_this_runners_own_un_migrated_self_is_moved_and_retried() {
        let temp = TempDir::new("enable-race");
        let me = std::process::id() as i32;
        let base = fake_base(temp.path(), &[me]);
        let proc_root = fake_proc(temp.path(), &[(me, "herd-runner", 1)]);
        let leaf = base.join(RUNNER_LEAF);

        enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel),
        )
        .expect("a base holding nothing but this runner is a base this runner can clear");

        assert!(
            is_enabled_for_children(&base, "memory"),
            "the controller must really be handed down: {:?}",
            fs::read_to_string(base.join("cgroup.subtree_control"))
        );
        assert_eq!(
            fs::read_to_string(base.join("cgroup.procs"))
                .expect("a readable cgroup.procs")
                .split_whitespace()
                .count(),
            0,
            "the runner must have left the base"
        );
        assert_eq!(
            fs::read_to_string(leaf.join("cgroup.procs"))
                .expect("a readable leaf cgroup.procs")
                .trim(),
            me.to_string(),
            "and it must be standing in its leaf"
        );
        assert_eq!(
            subtree_writes(&base),
            2,
            "one write refused and one taken: never a loop"
        );
    }

    /// **The known-bad direction, and the point of the pair.** A process in
    /// the base that is *not* this runner's is a real refusal, and a retry
    /// there would be a spin against a condition that is not going to change.
    /// One write, no move, no enable — and a refusal that names the pid and
    /// what the process table calls it, so a reader has something to go and
    /// look at.
    ///
    /// A child of this runner's stands in the base beside the stranger, and
    /// is **not** moved: a base this runner is not going to get is a base it
    /// leaves exactly as it found it, so the reading comes before the moving.
    #[test]
    fn an_ebusy_caused_by_a_foreign_process_is_refused_naming_it_and_never_retried() {
        let temp = TempDir::new("enable-foreign");
        let me = std::process::id() as i32;
        let stranger = 4_242_001;
        let own_child = 4_242_008;
        let base = fake_base(temp.path(), &[own_child, stranger]);
        // A command with a space and a closing parenthesis in it, because
        // `/proc/<pid>/stat` puts the command in parentheses and the only
        // safe split is at the last one.
        let proc_root = fake_proc(
            temp.path(),
            &[
                (me, "herd-runner", 1),
                (own_child, "herd-runner", me),
                (stranger, "sh (x) y", 1),
            ],
        );
        let leaf = base.join(RUNNER_LEAF);

        let why = enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel),
        )
        .expect_err("a base shared with somebody else is not this runner's to clear");

        assert!(why.contains(&stranger.to_string()), "{why}");
        assert!(
            why.contains("sh (x) y"),
            "the name must be read whole: {why}"
        );
        assert!(
            why.contains("no process of this runner's"),
            "the refusal must say whose the process is: {why}"
        );
        assert!(
            why.contains("Delegate=yes"),
            "and what to do about it: {why}"
        );
        assert!(
            !why.contains(&own_child.to_string()),
            "the refusal is about the stranger, not about this runner's own: {why}"
        );
        assert_eq!(
            subtree_writes(&base),
            1,
            "a refusal that cannot change must be refused, not retried"
        );
        assert!(!is_enabled_for_children(&base, "memory"));
        assert_eq!(
            fs::read_to_string(leaf.join("cgroup.procs"))
                .expect("a readable leaf cgroup.procs")
                .trim(),
            "",
            "nothing may be moved into a base this runner is not going to get"
        );
        assert!(
            fs::read_to_string(base.join("cgroup.procs"))
                .expect("a readable cgroup.procs")
                .contains(&own_child.to_string()),
            "and the base is left exactly as it was found"
        );
    }

    /// **The defect as the kernel showed it** (2026-10-30, in a delegated
    /// scope). The thing standing in the base was not this runner's own
    /// un-migrated self — every `prepare` moves out before it writes — but a
    /// real `herd-runner` binary a test had spawned, forked out of the base
    /// before any `prepare` left it: `cgroup.procs` listed `3175023
    /// (herd-runner)`. The message of the day blamed "something other than
    /// this runner", and the something-other was the runner's own child; a
    /// reader chasing a third party found none.
    ///
    /// A process this runner started is a process this runner may move, so it
    /// is moved into the leaf and the write is tried once more. A grandchild
    /// rather than a child, so that the walk up the process table is what
    /// answers and not one `stat` read.
    #[test]
    fn an_ebusy_caused_by_a_process_this_runner_started_moves_it_rather_than_blaming_a_stranger() {
        let temp = TempDir::new("enable-own-child");
        let me = std::process::id() as i32;
        let child = 4_242_002;
        let grandchild = 4_242_003;
        let base = fake_base(temp.path(), &[grandchild]);
        let proc_root = fake_proc(
            temp.path(),
            &[
                (me, "herd-runner", 1),
                (child, "sh", me),
                (grandchild, "herd-runner", child),
            ],
        );
        let leaf = base.join(RUNNER_LEAF);

        enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel),
        )
        .expect("a process this runner started is a process this runner may move");

        assert!(is_enabled_for_children(&base, "memory"));
        assert_eq!(
            fs::read_to_string(leaf.join("cgroup.procs"))
                .expect("a readable leaf cgroup.procs")
                .trim(),
            grandchild.to_string(),
            "the runner's own grandchild must be standing in the leaf"
        );
        assert_eq!(
            fs::read_to_string(base.join("cgroup.procs"))
                .expect("a readable cgroup.procs")
                .split_whitespace()
                .count(),
            0
        );
        assert_eq!(subtree_writes(&base), 2, "one refused and one taken");
    }

    /// And the one place a refusal about this runner's own is still a
    /// refusal: it could not be moved. The sentence has to say *that* —
    /// which process, and where it would not go — rather than reach for the
    /// no-internal-process rule a second time.
    #[test]
    fn a_process_of_this_runners_that_will_not_move_is_a_refusal_saying_so() {
        let temp = TempDir::new("enable-immovable");
        let me = std::process::id() as i32;
        let child = 4_242_009;
        let base = fake_base(temp.path(), &[child]);
        let proc_root = fake_proc(temp.path(), &[(me, "herd-runner", 1), (child, "sh", me)]);
        let leaf = base.join(RUNNER_LEAF);
        // A directory where the leaf's `cgroup.procs` should be: the move has
        // somewhere to go and cannot go there.
        fs::remove_file(leaf.join("cgroup.procs")).expect("the file goes");
        fs::create_dir(leaf.join("cgroup.procs")).expect("and a directory takes its place");

        let why = enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel),
        )
        .expect_err("a process that will not move is a refusal");

        assert!(why.contains(&child.to_string()), "{why}");
        assert!(why.contains("could not be moved into"), "{why}");
        assert!(
            why.contains(&leaf.display().to_string()),
            "the refusal must name where it would not go: {why}"
        );
        assert_eq!(subtree_writes(&base), 1, "and the write is not tried again");
    }

    /// An `EBUSY` with nothing in the base at all: whatever the kernel was
    /// refusing for has gone between the write and the reading. That is the
    /// one case a retry is honest with no move at all — and the runner must
    /// **not** move itself, because it was never in the base.
    #[test]
    fn an_ebusy_with_nothing_left_in_the_base_is_retried_without_a_move() {
        let temp = TempDir::new("enable-stale");
        let me = std::process::id() as i32;
        let base = fake_base(temp.path(), &[]);
        let proc_root = fake_proc(temp.path(), &[(me, "herd-runner", 1)]);
        let leaf = base.join(RUNNER_LEAF);

        enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel_refusing_once),
        )
        .expect("a refusal with nothing behind it is worth exactly one more try");

        assert!(is_enabled_for_children(&base, "memory"));
        assert_eq!(subtree_writes(&base), 2);
        assert_eq!(
            fs::read_to_string(leaf.join("cgroup.procs"))
                .expect("a readable leaf cgroup.procs")
                .trim(),
            "",
            "nothing of this runner's was in the base, so nothing had to move"
        );
    }

    /// And the absence, said as an absence, which is this project's own rule.
    /// A refusal this runner cannot explain — `cgroup.procs` will not read —
    /// is reported as one it cannot explain: not as its own self, not as a
    /// stranger, and not retried, because the condition behind it is unknown.
    #[test]
    fn an_ebusy_this_runner_cannot_explain_says_that_rather_than_guessing() {
        let temp = TempDir::new("enable-unreadable");
        let me = std::process::id() as i32;
        let base = fake_base(temp.path(), &[]);
        // A directory where the file should be: present, and unreadable as
        // text, which is as close as a test gets to a `cgroup.procs` the
        // kernel will not hand over.
        fs::remove_file(base.join("cgroup.procs")).expect("the file goes");
        fs::create_dir(base.join("cgroup.procs")).expect("and a directory takes its place");
        let proc_root = fake_proc(temp.path(), &[(me, "herd-runner", 1)]);
        let leaf = base.join(RUNNER_LEAF);

        let why = enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel_always_refusing),
        )
        .expect_err("a refusal with no reading behind it is still a refusal");

        assert!(
            why.contains("cannot be read"),
            "the absence must be named: {why}"
        );
        assert!(
            why.contains("cannot say whether those processes are its own or somebody else's"),
            "and said as an absence rather than guessed either way: {why}"
        );
        assert_eq!(
            subtree_writes(&base),
            1,
            "a condition nobody measured is not a condition to retry on"
        );
    }

    /// The two outcomes that were already right, held still: a controller
    /// already handed down is silence and no write at all, and a controller
    /// the base never had is a sentence naming what it does have.
    #[test]
    fn a_controller_already_handed_down_or_never_offered_is_not_a_write() {
        let temp = TempDir::new("enable-nothing-to-do");
        let me = std::process::id() as i32;
        let base = fake_base(temp.path(), &[me]);
        let proc_root = fake_proc(temp.path(), &[(me, "herd-runner", 1)]);
        let leaf = base.join(RUNNER_LEAF);
        fs::write(base.join("cgroup.subtree_control"), "cpu memory pids")
            .expect("a writable cgroup.subtree_control");
        enable_for_children_through(
            &base,
            "memory",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel_always_refusing),
        )
        .expect("a controller already handed down needs nothing written");
        assert_eq!(subtree_writes(&base), 0);

        fs::write(base.join("cgroup.controllers"), "cpu memory pids")
            .expect("a writable cgroup.controllers");
        let why = enable_for_children_through(
            &base,
            "io",
            Some(&leaf),
            fake_kernel_at(&proc_root, fake_kernel_always_refusing),
        )
        .expect_err("a base that does not offer io cannot hand it down");
        assert!(why.contains("does not offer the io controller"), "{why}");
        assert!(why.contains("cpu memory pids"), "{why}");
        assert_eq!(subtree_writes(&base), 0);
    }

    /// Whose a pid is, read out of the process table rather than assumed:
    /// this process, a descendant of it however deep, a stranger, and a pid
    /// that has gone between `cgroup.procs` and `/proc`. The last one is in
    /// nobody's way and must not be reported as a stranger.
    #[test]
    fn who_is_in_a_cgroup_is_read_out_of_the_process_table() {
        let temp = TempDir::new("occupancy");
        let me = std::process::id() as i32;
        let child = 4_242_004;
        let grandchild = 4_242_005;
        let stranger = 4_242_006;
        let departed = 4_242_007;
        let base = fake_base(temp.path(), &[me, grandchild, stranger, departed]);
        let proc_root = fake_proc(
            temp.path(),
            &[
                (me, "herd-runner", 1),
                (child, "sh", me),
                (grandchild, "cargo", child),
                (stranger, "postgres", 1),
            ],
        );

        let who = Occupancy::read(&base, fake_kernel_at(&proc_root, fake_kernel));
        assert_eq!(who.unreadable, None);
        assert_eq!(
            who.occupants
                .iter()
                .map(|one| (one.pid, one.whose))
                .collect::<Vec<_>>(),
            vec![
                (me, Whose::ThisRunner),
                (grandchild, Whose::ItsChild),
                (stranger, Whose::Foreign),
                (departed, Whose::Gone),
            ]
        );
        assert_eq!(
            who.mine().iter().map(|one| one.pid).collect::<Vec<_>>(),
            vec![me, grandchild],
            "itself and what it started are its to move; a process that has \
             gone is in nobody's way"
        );
        assert_eq!(
            who.strangers()
                .iter()
                .map(|one| one.pid)
                .collect::<Vec<_>>(),
            vec![stranger]
        );
        assert!(
            Occupancy::name_them(&who.strangers()).contains("postgres"),
            "a refusal names what the process table calls them"
        );

        // And an unreadable `cgroup.procs` is not an empty one.
        let missing = temp.path().join("gone");
        fs::create_dir(&missing).expect("a cgroup with no cgroup.procs");
        let nothing = Occupancy::read(&missing, fake_kernel_at(&proc_root, fake_kernel));
        assert!(nothing.unreadable.is_some());
        assert!(nothing.occupants.is_empty());
        assert!(nothing.mine().is_empty() && nothing.strangers().is_empty());
    }

    // ------------------------------- a controller is read, not inferred ----

    /// A ceiling to prove, on the device the fixtures below accept.
    fn an_io_ceiling() -> IoMax {
        IoMax {
            ceiling: IoCeiling::parse("rbps=200MB,wbps=100MB").expect("a ceiling"),
            device: "259:0".to_owned(),
        }
    }

    /// Marker files the fake kernel reads out of the base, so that one `fn`
    /// pointer can be every host shape the proof has to tell apart. Beside
    /// the controller list rather than in a static, for [`subtree_writes`]'s
    /// reason: this crate's tests run on twelve threads in one process.
    ///
    /// - `BLK_THROTTLE`: this kernel was built with per-cgroup IO throttling,
    ///   so a cgroup handed `io` gets an `io.max` interface file. Without it
    ///   the file is absent however `io` is delegated — a kernel built
    ///   without `CONFIG_BLK_DEV_THROTTLING`.
    /// - `STRAY_IO_MAX`: a cgroup is left holding an `io.max` file that its
    ///   controller list does not back. **The known-bad fixture**, and the
    ///   shape measured on this project's host on 2026-10-30: `app.slice`
    ///   holds `io.pressure` while `io` is absent from its
    ///   `cgroup.controllers`, because PSI files are not controller-gated.
    ///   Put on `io.max` itself here, because that is the file the proof
    ///   touches and therefore the file an existence reading would believe.
    const BLK_THROTTLE: &str = "fixture-blk-throttle";
    const STRAY_IO_MAX: &str = "fixture-stray-io-max";

    /// A delegated base as [`Ceiling::prove_io_max`] meets it: the
    /// controllers it hands to its children, and the host shape its kernel
    /// has.
    fn fake_io_base(root: &Path, handed_to_children: &str, shape: &[&str]) -> Ceiling {
        let base = root.join("base");
        fs::create_dir_all(&base).expect("a base");
        fs::write(base.join("cgroup.controllers"), "cpu io memory pids")
            .expect("a writable cgroup.controllers");
        fs::write(base.join("cgroup.subtree_control"), handed_to_children)
            .expect("a writable cgroup.subtree_control");
        for marker in shape {
            fs::write(base.join(marker), "").expect("a writable marker");
        }
        Ceiling::Cgroups {
            base,
            limits: JobLimits::with_memory_max(1 << 30),
            unenforced: Vec::new(),
        }
    }

    /// Making a cgroup, as cgroup v2 makes one: the new cgroup is handed
    /// exactly what its parent's `cgroup.subtree_control` lists, and the
    /// kernel populates it with the interface files that follow from that —
    /// plus `io.pressure`, which is there either way.
    fn fake_cgroup_mkdir(path: &Path) -> io::Result<()> {
        fs::create_dir(path)?;
        let parent = path.parent().expect("a cgroup sits in a cgroup");
        let handed = fs::read_to_string(parent.join("cgroup.subtree_control")).unwrap_or_default();
        fs::write(path.join("cgroup.controllers"), handed.trim())?;
        fs::write(path.join("cgroup.procs"), "")?;
        // Not controller-gated, and the whole reason this defect was
        // reachable: an `io.*` file says nothing about `io`.
        fs::write(path.join("io.pressure"), "some avg10=0.00\n")?;
        let io = handed.split_whitespace().any(|name| name == "io");
        let throttling = parent.join(BLK_THROTTLE).exists();
        if (io && throttling) || parent.join(STRAY_IO_MAX).exists() {
            fs::write(path.join("io.max"), "")?;
        }
        Ok(())
    }

    /// Writing a cgroup control file, with the one rule this proof depends
    /// on: **cgroupfs creates no file**. A write naming an interface file
    /// that is not there is `ENOENT`, which is the only way a runner sees a
    /// kernel with no blk-throttle. A device the fixture has no queue for is
    /// refused the way the kernel refuses one, with `EINVAL`.
    fn fake_io_write(path: &Path, value: &str) -> io::Result<()> {
        if !path.exists() {
            return Err(io::Error::from_raw_os_error(libc::ENOENT));
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("io.max")
            && value.split_whitespace().next() != Some("259:0")
        {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        fs::write(path, value)
    }

    fn fake_io_kernel(mkdir: fn(&Path) -> io::Result<()>) -> Kernel<'static> {
        Kernel {
            proc_root: Path::new(PROC),
            me: std::process::id() as i32,
            write: fake_io_write,
            mkdir,
        }
    }

    /// **The known-bad direction, and the point of this test.** A cgroup
    /// holding an `io.max` file whose `cgroup.controllers` does not list `io`
    /// is a cgroup where an `io.max` bounds nothing — and until 2026-10-30
    /// the proof wrote into that file, read the device back out of it and
    /// called the ceiling proved, because it never read the controller list
    /// at all. An operator would have been told his hard IO ceiling was
    /// enforced by a kernel that was never handed the controller to enforce
    /// it with.
    #[test]
    fn an_io_file_in_a_cgroup_handed_no_io_controller_proves_no_ceiling() {
        let temp = TempDir::new("io-max-stray-file");
        // Everything but `io` handed down, and an `io.max` lying there
        // anyway: the one shape an existence reading cannot survive.
        let ceiling = fake_io_base(
            temp.path(),
            "cpu memory pids",
            &[BLK_THROTTLE, STRAY_IO_MAX],
        );
        let why = ceiling
            .prove_io_max_through(&an_io_ceiling(), fake_io_kernel(fake_cgroup_mkdir))
            .expect_err("a file is not a controller");
        assert!(
            why.contains("the io controller is not handed down"),
            "the refusal must name the controller as the thing that is missing: {why}"
        );
        assert!(
            why.contains("\"cpu memory pids\""),
            "and quote the list it read, so a reader can check it: {why}"
        );
        assert!(
            !why.contains("CONFIG_BLK_DEV_THROTTLING") && !why.contains("refused"),
            "and must not offer this host's kernel build or this device as the \
             explanation, which are the other two hosts: {why}"
        );
    }

    /// The same, where the controller list cannot be read at all. An absence
    /// is reported as an absence, as everywhere else in this module: not as a
    /// controller that is there, and not as one that is not.
    #[test]
    fn a_controller_list_that_will_not_read_is_a_refusal_naming_the_absence() {
        let temp = TempDir::new("io-max-unreadable-list");
        let ceiling = fake_io_base(temp.path(), "cpu io memory pids", &[BLK_THROTTLE]);
        // A kernel whose mkdir makes the cgroup and populates nothing, which
        // is the one thing no real kernel does — so the list is missing
        // rather than empty.
        let why = ceiling
            .prove_io_max_through(
                &an_io_ceiling(),
                fake_io_kernel(|path| fs::create_dir(path)),
            )
            .expect_err("an unreadable controller list proves nothing");
        assert!(
            why.contains("cgroup.controllers") && why.contains("cannot be read"),
            "the refusal must name the file it could not read: {why}"
        );
    }

    /// **The two hosts the existence reading made one.** A base that
    /// withholds `io` and a kernel with no blk-throttle both end with no
    /// `io.max` to write into, and the old reading said the same sentence for
    /// both — naming each as a possibility and neither as the answer. They
    /// are not the same host and the operator's next move is not the same, so
    /// each gets its own refusal.
    #[test]
    fn a_withheld_controller_and_a_kernel_with_no_throttling_read_apart() {
        let temp = TempDir::new("io-max-two-hosts");
        let withheld = fake_io_base(temp.path(), "cpu memory pids", &[BLK_THROTTLE]);
        let why = withheld
            .prove_io_max_through(&an_io_ceiling(), fake_io_kernel(fake_cgroup_mkdir))
            .expect_err("a base that withholds io carries no ceiling");
        assert!(
            why.contains("the io controller is not handed down")
                && why.contains("Delegate=yes")
                && !why.contains("CONFIG_BLK_DEV_THROTTLING"),
            "a withheld controller is a delegation to grant, and nothing about \
             this kernel's build: {why}"
        );

        let other = TempDir::new("io-max-no-throttling");
        // `io` really handed down, and no `io.max` interface file on this
        // kernel: the write finds nothing to write to.
        let unthrottled = fake_io_base(other.path(), "cpu io memory pids", &[]);
        let why = unthrottled
            .prove_io_max_through(&an_io_ceiling(), fake_io_kernel(fake_cgroup_mkdir))
            .expect_err("a kernel with no blk-throttle holds no ceiling");
        assert!(
            why.contains("CONFIG_BLK_DEV_THROTTLING")
                && why.contains("io controller is handed down")
                && !why.contains("is not handed down"),
            "a kernel with no throttling is a kernel build, and the controller \
             is explicitly not the thing that is wrong: {why}"
        );
    }

    /// The third host, which the controller list must not swallow: `io`
    /// handed down, `io.max` there, and the kernel refusing *this device*.
    /// That refusal is the kernel's own and is reported as such.
    #[test]
    fn a_device_the_kernel_refuses_is_the_kernels_refusal_and_not_the_controllers() {
        let temp = TempDir::new("io-max-wrong-device");
        let ceiling = fake_io_base(temp.path(), "cpu io memory pids", &[BLK_THROTTLE]);
        let elsewhere = IoMax {
            device: "8:16".to_owned(),
            ..an_io_ceiling()
        };
        let why = ceiling
            .prove_io_max_through(&elsewhere, fake_io_kernel(fake_cgroup_mkdir))
            .expect_err("a device with no queue is no ceiling");
        assert!(
            why.contains("the kernel refused") && why.contains("8:16"),
            "the refusal must be the kernel's, naming the value it turned down: {why}"
        );
        assert!(
            !why.contains("not handed down"),
            "and must not blame the controller, which this base hands down: {why}"
        );
    }

    /// And the other direction of the pair, so that a proof which refuses
    /// everything is not mistaken for a fix: a base that genuinely lists
    /// `io`, on a kernel that throttles, still proves — and the write really
    /// reaches `io.max`, which the refused-device test above is what shows.
    #[test]
    fn a_base_that_really_hands_io_down_proves_the_ceiling() {
        let temp = TempDir::new("io-max-proved");
        let ceiling = fake_io_base(temp.path(), "cpu io memory pids", &[BLK_THROTTLE]);
        ceiling
            .prove_io_max_through(&an_io_ceiling(), fake_io_kernel(fake_cgroup_mkdir))
            .expect("a base that hands io down takes the ceiling");
    }

    /// `memory.events` is read as **two** counters, because only one of them
    /// says how many kill *events* there were.
    ///
    /// The text is the kernel's own shape, with `oom_group_kill` where Linux
    /// 5.17 and later put it. `low`, `high` and `max` are there so the
    /// parser is proved against a file with other fields in it rather than
    /// against the one line it wants.
    #[test]
    fn memory_events_is_read_as_a_process_count_and_an_event_count() {
        let found = oom_kills_in(
            "low 0\nhigh 0\nmax 41\noom 1\noom_kill 8\noom_group_kill 1\n",
        );
        assert_eq!(
            found,
            OomKills {
                processes: 8,
                group_kills: Some(1),
            },
            "eight tasks killed, in one group-kill event"
        );
    }

    /// A kernel that does not publish `oom_group_kill` says so as absence,
    /// never as zero: zero would read as "no event has ever happened here",
    /// which is a claim this file is not making.
    #[test]
    fn a_kernel_without_the_group_counter_is_read_as_absence() {
        let found = oom_kills_in("low 0\nhigh 0\nmax 41\noom 1\noom_kill 8\n");
        assert_eq!(found.processes, 8);
        assert_eq!(
            found.group_kills, None,
            "absent is not zero: one is a kernel that cannot tell us, the \
             other is a kernel telling us nothing was killed"
        );
    }

    /// Nothing at all — the file missing, or a cgroup that was never made —
    /// is zero kills and no event count, and the baseline in
    /// [`crate::job`] is a high-water mark for exactly this reason.
    #[test]
    fn an_empty_or_unreadable_memory_events_is_no_kills() {
        assert_eq!(oom_kills_in(""), OomKills::default());
        assert_eq!(oom_kills_in("oom_kill not-a-number\n"), OomKills::default());
    }
}
