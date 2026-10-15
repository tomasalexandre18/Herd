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
    Ceiling, CpuQuota, JobLimits, DEFAULT_CPU_WEIGHT, DEFAULT_IO_WEIGHT, DEFAULT_PIDS_MAX,
    MINIMUM_PIDS_MAX, WEIGHT_RANGE,
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
            disk_budget: DEFAULT_DISK_BUDGET,
            nice: DEFAULT_NICE,
            cpu_weight: DEFAULT_CPU_WEIGHT,
            cpu_max: None,
            io_weight: DEFAULT_IO_WEIGHT,
            pids_max: DEFAULT_PIDS_MAX,
            cgroup_root: None,
            privileged_helpers: Vec::new(),
        }
    }

    /// The four controls a job's cgroup carries, as the cgroup code takes
    /// them.
    pub fn job_limits(&self) -> JobLimits {
        JobLimits {
            memory_max: self.memory_max,
            cpu_weight: self.cpu_weight,
            cpu_max: self.cpu_max,
            io_weight: self.io_weight,
            pids_max: self.pids_max,
        }
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
    /// Whether the `io.weight` a job carries reaches the device that job
    /// works on, and why either way. A bound that is written and ignored
    /// must be said out loud, not assumed — and so must one that does bite,
    /// because the answer is about a device and not about this host (D120).
    io_weight_reach: crate::blockio::IoWeightReach,
}

impl Confined {
    /// Check everything, or refuse naming what is wrong.
    pub fn prepare(confinement: Confinement) -> Result<Self, String> {
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
        let ceiling = Ceiling::detect(confinement.job_limits(), confinement.cgroup_root.as_deref());
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
        // Asked once, at startup, because the answer is about this host's
        // block layer and cannot change under a running runner without
        // somebody changing a scheduler by hand.
        let io_weight_reach = crate::blockio::io_weight_reach(root.path());
        // Before the first job and before the hub: a runner that was killed
        // left processes, cgroups and workspaces nothing else will collect.
        let reclaimed = reclaim_a_past_life(&root, &ceiling);
        Ok(Self {
            root,
            ceiling,
            helpers,
            confinement,
            live: LiveJobs::default(),
            reclaimed,
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
                "caps per job: {} memory, {} s, {} of log, {} of disk, nice {}",
                show_bytes(c.memory_max),
                c.timeout.as_secs(),
                show_bytes(c.log_cap),
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
                 this runner sets none: a job's IO here is bounded by the idle IO class \
                 alone, which a scheduler of none and a ZFS dataset both ignore.",
                c.io_weight, why
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
}
