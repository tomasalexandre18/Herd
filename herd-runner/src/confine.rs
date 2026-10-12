//! How a job is confined: what the caps are, and what is done to a process
//! between fork and exec to make them hold (E-34, E-45, E-46, E-30).
//!
//! Everything here is configuration made of strings and numbers (invariant
//! 1). There is no notion of a step, a stage or a policy expression: a runner
//! has one data root, five caps and a list of helper paths, and that is the
//! whole surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cgroup::Ceiling;
use crate::privilege::PrivilegedHelpers;
use crate::root::{DataRoot, Workspace};
use crate::size::show_bytes;

/// The ceiling a job gets unless the machine's owner says otherwise. Two
/// gibibytes is small enough to be safe on the 31 GiB server beside the hub
/// and on a laptop, and a real build that needs more says so in M8's
/// admission rather than by taking the host down.
pub const DEFAULT_MEMORY_MAX: u64 = 2 << 30;

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
            cgroup_root: None,
            privileged_helpers: Vec::new(),
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
        if confinement.timeout.is_zero() {
            return Err("a time cap of 0 s would stop every job before it ran".to_owned());
        }
        let ceiling = Ceiling::detect(confinement.memory_max, confinement.cgroup_root.as_deref());
        Ok(Self {
            root,
            ceiling,
            helpers,
            confinement,
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
        ];
        match self.ceiling.fallback_reason() {
            None => lines.push(
                "memory ceiling: one cgroup v2 subtree per job, enforced by the kernel".to_owned(),
            ),
            Some(why) => lines.push(format!(
                "memory ceiling: FALLING BACK to a per-process address-space limit, \
                 which bounds each process instead of the job's tree and makes an \
                 allocation fail instead of killing the job. Reason: {why}"
            )),
        }
        if self.helpers.allowed().is_empty() {
            lines.push("privileged helpers: none configured".to_owned());
        } else {
            lines.push(format!(
                "privileged helpers: {}",
                self.helpers.allowed().join(", ")
            ));
        }
        lines
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    pub fn ceiling(&self) -> &Ceiling {
        &self.ceiling
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
pub fn job_environment(workspace: &Workspace) -> Vec<(String, String)> {
    let mut environment = vec![
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
            // nice() returning -1 is ambiguous with a legitimate -1 priority,
            // so errno is cleared first.
            *libc::__errno_location() = 0;
            if libc::nice(self.nice) == -1 && *libc::__errno_location() != 0 {
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
        // Either the real ceiling or the fallback, never silence about which.
        assert!(
            report.contains("memory ceiling:"),
            "the report must say how the ceiling is enforced: {report}"
        );
    }

    /// A fallback has to be impossible to miss in the report, because the
    /// report is the only place an operator sees it before a job misbehaves.
    #[test]
    fn a_fallback_is_shouted_not_mentioned() {
        let temp = TempDir::new("confine-fallback");
        let mut confinement = Confinement::with_data_root(temp.path());
        confinement.cgroup_root = Some(PathBuf::from("/sys/fs/cgroup/herd-no-such-cgroup-m4"));
        let confined = Confined::prepare(confinement).expect("a missing cgroup is not fatal");
        let report = confined.report().join("\n");
        assert!(report.contains("FALLING BACK"), "{report}");
        assert!(report.contains("herd-no-such-cgroup-m4"), "{report}");
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
        let workspace = root
            .open_workspace(&herd_proto::JobId::new("j1"))
            .expect("made");
        let environment = job_environment(&workspace);
        let names: Vec<&str> = environment.iter().map(|(name, _)| name.as_str()).collect();
        assert!(!names.contains(&"HERD_TOKEN"), "{names:?}");
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
