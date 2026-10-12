//! Reclaiming what a job, or a whole previous runner, left behind.
//!
//! Three things of a job outlive the moment it ends if nobody takes them
//! away: its processes, its cgroup and its workspace. The runner takes all
//! three away when a job ends, when it is asked to stop, and — because
//! `SIGKILL` cannot be handled — at its next startup, for the jobs of the
//! life it did not get to finish.
//!
//! ## How a job's processes are found again
//!
//! Under a cgroup there is nothing to find: one write to `cgroup.kill` takes
//! the whole tree, whatever it did to detach itself. The fallback has no such
//! handle. `killpg` reaches the job's process group, which is everything the
//! job started — unless something called `setsid`, which leaves the group by
//! definition and used to survive its job.
//!
//! So the runner stamps one unique marker into every job's environment, which
//! it can do because it builds that environment rather than inheriting it
//! (D28), and sweeps `/proc` for processes still carrying it. A process keeps
//! its environment across `fork` and `exec`, so a double fork, a `nohup` and a
//! `setsid` all stay findable. What stays beyond reach is a process that
//! scrubs its own environment, and the fallback line says so.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use herd_proto::JobId;

use crate::cgroup::Ceiling;
use crate::confine::kill_process_group;
use crate::root::{reclaim_directory, DataRoot, Workspace};

/// The environment variable carrying a job's marker.
pub const MARKER_VARIABLE: &str = "HERD_JOB_MARKER";

/// What every job's name starts with, in `/sys/fs/cgroup` and in the marker.
const JOB_PREFIX: &str = "herd-job-";

/// Counts jobs, so that no two tags minted by this process are equal.
static JOBS: AtomicU64 = AtomicU64::new(0);

/// How long to keep sweeping for a process that has been signalled.
const SWEEP_TRIES: u32 = 40;
const SWEEP_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// One job's unique name: `herd-job-<runner pid>-<counter>-<job id>`.
///
/// The same string names the job's cgroup, its workspace and its marker. The
/// runner's pid and the counter are what make it unique — two runners may
/// share a cgroup root and job ids are only unique per hub, and a name that
/// collided would mean one job's kill reaching another job's processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobTag(String);

impl JobTag {
    /// Mint the tag for a job about to start.
    pub fn mint(job_id: &JobId) -> Self {
        let safe: String = job_id
            .as_str()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Self(format!(
            "{JOB_PREFIX}{}-{}-{safe}",
            std::process::id(),
            JOBS.fetch_add(1, Ordering::SeqCst)
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Kill everything still carrying this job's marker, and say how many.
///
/// Called after `killpg` on the fallback path, where a process that gave
/// itself a new session is otherwise out of reach.
pub fn kill_by_marker(tag: &JobTag) -> usize {
    let mut killed = 0usize;
    for _ in 0..SWEEP_TRIES {
        let carriers = processes_carrying(tag);
        if carriers.is_empty() {
            break;
        }
        for pid in carriers {
            if kill_pid(pid) {
                killed += 1;
            }
        }
        std::thread::sleep(SWEEP_PAUSE);
    }
    killed
}

/// Every live process whose environment carries this marker, this runner's
/// own processes excepted.
fn processes_carrying(tag: &JobTag) -> Vec<u32> {
    let needle = format!("{MARKER_VARIABLE}={}", tag.as_str());
    let mut found = Vec::new();
    let me = std::process::id();
    for pid in live_pids() {
        if pid == me {
            continue;
        }
        // Readable only for this user's own processes, which is exactly the
        // set a job's processes belong to.
        let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) else {
            continue;
        };
        if environ
            .split(|byte| *byte == 0)
            .any(|entry| entry == needle.as_bytes())
        {
            found.push(pid);
        }
    }
    found
}

fn live_pids() -> Vec<u32> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .collect()
}

#[cfg(unix)]
fn kill_pid(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // Safe: one signal to one pid; an already-dead pid is an ESRCH we have
    // nothing to do about.
    unsafe { libc::kill(pid, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
fn kill_pid(_pid: u32) -> bool {
    false
}

/// Whether the process that minted a name is still there.
///
/// This is the whole test for "left behind", and it is deliberately the
/// weakest possible claim: a *live* pid means somebody may still be watching
/// that job, so its processes and its cgroup are not this runner's to take.
/// Nothing is inferred from the command name — a runner may be started
/// through a wrapper, under a different argv0, or be a test binary, and a
/// reaper that killed what it merely failed to recognise would be far worse
/// than one that waits for the next restart.
///
/// The cost is pid reuse: a dead runner's pid taken over by something else
/// makes its leftovers survive one more life. A leftover that lingers is a
/// nuisance; a live job killed by a stranger is a lost build.
fn minter_is_gone(pid: u32) -> bool {
    if pid == std::process::id() {
        return false;
    }
    fs::metadata(format!("/proc/{pid}")).is_err()
}

/// Everything one running job can leave behind, kept so that a runner being
/// stopped can end its jobs instead of orphaning them.
#[derive(Debug)]
struct Live {
    tag: JobTag,
    pid: u32,
    cgroup: Option<PathBuf>,
    workspace: Workspace,
}

/// The jobs this runner is running right now.
#[derive(Debug, Default)]
pub struct LiveJobs {
    jobs: Mutex<Vec<(u64, Live)>>,
    next: AtomicU64,
}

impl LiveJobs {
    /// Record a job that has just been spawned. The token returned is what
    /// takes it off the list again.
    pub fn add(&self, tag: JobTag, pid: u32, cgroup: Option<PathBuf>, workspace: Workspace) -> u64 {
        let token = self.next.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push((
                token,
                Live {
                    tag,
                    pid,
                    cgroup,
                    workspace,
                },
            ));
        }
        token
    }

    /// Take a job off the list, because it has ended and cleaned up itself.
    pub fn remove(&self, token: u64) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.retain(|(held, _)| *held != token);
        }
    }

    pub fn count(&self) -> usize {
        self.jobs.lock().map(|jobs| jobs.len()).unwrap_or(0)
    }

    /// End every running job and reclaim what it held, naming each one.
    ///
    /// This is what a runner does when its owner asks it to stop: a job dies
    /// with the runner that was watching it rather than becoming a process
    /// nobody owns. M3 owns the orderly drain (invariant 2's ≤ 5 s); this is
    /// the part that must never be missing, which is that nothing survives.
    pub fn end_all(&self) -> Vec<String> {
        let taken: Vec<(u64, Live)> = match self.jobs.lock() {
            Ok(mut jobs) => std::mem::take(&mut *jobs),
            Err(_) => Vec::new(),
        };
        let mut said = Vec::new();
        for (_, job) in taken {
            if let Some(dir) = &job.cgroup {
                let _ = fs::write(dir.join("cgroup.kill"), "1");
            }
            kill_process_group(job.pid);
            let swept = kill_by_marker(&job.tag);
            let workspace = match job.workspace.reclaim() {
                Ok(()) => "its workspace reclaimed".to_owned(),
                Err(error) => format!("ITS WORKSPACE LEFT BEHIND: {error}"),
            };
            if let Some(dir) = &job.cgroup {
                remove_cgroup(dir);
            }
            said.push(format!(
                "ended job {} (pid {}, {swept} stray process(es) swept), {workspace}",
                job.tag.as_str(),
                job.pid
            ));
        }
        said
    }
}

/// Reclaim what a previous life of this runner left behind, and name it.
///
/// A runner that was `SIGKILL`ed could not clean up: its job's processes are
/// reparented to init, its job cgroups stay, and its workspaces stay. Nothing
/// else will ever collect them, so the next runner does, at startup, before
/// it joins a hub. Returns one line per thing reclaimed, for the startup
/// report — silence here is as bad as a silent fallback.
pub fn reclaim_a_past_life(root: &DataRoot, ceiling: &Ceiling) -> Vec<String> {
    let mut said = Vec::new();

    // Processes first: a workspace is not worth reclaiming while something is
    // still writing into it.
    for (pid, marker) in orphaned_job_processes() {
        if kill_pid(pid) {
            said.push(format!(
                "killed the orphaned job process {pid} of {marker}, left by a runner \
                 that is gone"
            ));
        }
    }

    // The workspaces. This runner owns the root exclusively (it holds its
    // lock), so anything under jobs/ at startup is a leftover by definition.
    let jobs = root.jobs_dir();
    if let Ok(entries) = fs::read_dir(&jobs) {
        for entry in entries.flatten() {
            let path = entry.path();
            match reclaim_directory(&path) {
                Ok(()) => said.push(format!(
                    "reclaimed the workspace {} left by a previous life",
                    path.display()
                )),
                Err(error) => said.push(format!(
                    "COULD NOT reclaim the leftover workspace {}: {error}",
                    path.display()
                )),
            }
        }
    }

    // The cgroups, whose names carry the pid of the runner that made them.
    if let Some(base) = ceiling.base() {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_ours = name.starts_with(JOB_PREFIX) || name.starts_with("herd-delegation-");
                if !is_ours || !entry.path().is_dir() {
                    continue;
                }
                let stale = pid_in(&name).is_some_and(minter_is_gone);
                if !stale {
                    continue;
                }
                remove_cgroup(&entry.path());
                said.push(format!(
                    "reclaimed the cgroup {} left by a previous life",
                    entry.path().display()
                ));
            }
        }
    }
    said
}

/// Every live process carrying a Herd job marker whose runner is gone.
fn orphaned_job_processes() -> Vec<(u32, String)> {
    let needle = format!("{MARKER_VARIABLE}=");
    let mut orphans = Vec::new();
    let me = std::process::id();
    for pid in live_pids() {
        if pid == me {
            continue;
        }
        let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) else {
            continue;
        };
        let marker = environ
            .split(|byte| *byte == 0)
            .filter_map(|entry| std::str::from_utf8(entry).ok())
            .find_map(|entry| entry.strip_prefix(needle.as_str()));
        let Some(marker) = marker else { continue };
        // A marker whose runner is still alive belongs to a job that runner is
        // still watching, and is none of this runner's business.
        if !pid_in(marker).is_some_and(minter_is_gone) {
            continue;
        }
        orphans.push((pid, marker.to_owned()));
    }
    orphans
}

/// The runner pid inside a job tag, cgroup name or marker.
fn pid_in(name: &str) -> Option<u32> {
    name.strip_prefix(JOB_PREFIX)
        .or_else(|| name.strip_prefix("herd-delegation-probe-"))?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// Kill whatever is in a cgroup and remove it. `rmdir` refuses a cgroup that
/// still has members and the kill is asynchronous, so a few tries beat one.
pub(crate) fn remove_cgroup(dir: &Path) {
    let _ = fs::write(dir.join("cgroup.kill"), "1");
    for _ in 0..50 {
        if fs::remove_dir(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_unique_per_job_and_carries_this_runner_s_pid() {
        let job = JobId::new("j1");
        let first = JobTag::mint(&job);
        let second = JobTag::mint(&job);
        assert_ne!(first, second, "two tags for one id must still differ");
        assert_eq!(pid_in(first.as_str()), Some(std::process::id()));
    }

    /// A tag reaches the filesystem as a cgroup name and a path, so an id
    /// shaped like a path must not survive in it.
    #[test]
    fn a_hostile_job_id_is_sanitised_into_the_tag() {
        let tag = JobTag::mint(&JobId::new("../../etc/passwd"));
        assert!(!tag.as_str().contains('/'), "{}", tag.as_str());
        assert!(!tag.as_str().contains(".."), "{}", tag.as_str());
    }

    /// The sweep is the whole of the fallback's new strength, so it is
    /// asserted against a real process that has left its process group.
    #[test]
    #[cfg(unix)]
    fn the_sweep_kills_a_process_that_left_its_process_group() {
        let tag = JobTag::mint(&JobId::new("sweep"));
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            // setsid is the escape killpg cannot answer: the child leaves the
            // job's process group entirely.
            .arg("setsid sleep 30 >/dev/null 2>&1 & echo $!; sleep 30")
            .env_clear()
            .env(MARKER_VARIABLE, tag.as_str())
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the fixture starts");
        // Give the grandchild time to exist before sweeping for it.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let carriers = processes_carrying(&tag);
        assert!(
            carriers.len() >= 2,
            "the fixture should show the shell and its detached child: {carriers:?}"
        );

        let killed = kill_by_marker(&tag);
        assert!(killed >= 2, "only {killed} process(es) were swept");
        assert!(
            processes_carrying(&tag).is_empty(),
            "the sweep left something behind: {:?}",
            processes_carrying(&tag)
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
