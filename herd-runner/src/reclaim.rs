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
//!
//! ## And a probe's, by the same mechanism
//!
//! A busy probe is a command the runner starts too (D51), and it has no
//! cgroup at all — so `killpg` is the *only* handle M5 gave it, and `killpg`
//! is exactly the handle a new session escapes. `/bin/sh -c "setsid sleep
//! 7779 & wait"` as a probe, and a `SIGTERM` to the runner, left `sleep 7779`
//! running at ppid 1: the group kill reached the shell and its ordinary
//! children, and nothing reached the one that had left the group. D62 says a
//! probe is killed as one group "on a timeout and on a clean stop alike", so
//! a probe carries a marker of its own and is swept the same way, rather than
//! a second mechanism being invented for it.
//!
//! One marker variable, one sweep, one startup reaper: a probe stray left by
//! a runner that was `SIGKILL`ed is collected by [`reclaim_a_past_life`]
//! beside a job's, because [`pid_in`] reads both shapes of tag.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use herd_proto::JobId;

use crate::cgroup::Ceiling;
use crate::confine::kill_process_group;
use crate::root::{reclaim_directory, DataRoot, Workspace};

/// The environment variable carrying the marker of a job — or of a probe
/// reading, which is swept by the same code and collected by the same reaper.
pub const MARKER_VARIABLE: &str = "HERD_JOB_MARKER";

/// What every job's name starts with, in `/sys/fs/cgroup` and in the marker.
const JOB_PREFIX: &str = "herd-job-";

/// What every probe reading's marker starts with. A probe has no cgroup and
/// no workspace, so this names nothing but the marker.
const PROBE_PREFIX: &str = "herd-probe-";

/// Counts jobs, so that no two tags minted by this process are equal.
static JOBS: AtomicU64 = AtomicU64::new(0);

/// The same, for probe readings. Its own counter, so that a probe tag can
/// never collide with a job tag of the same runner even by accident.
static PROBES: AtomicU64 = AtomicU64::new(0);

/// How long to keep sweeping for a process that has been signalled.
const SWEEP_TRIES: u32 = 40;
const SWEEP_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// The same sweep, on a much shorter budget, for a probe reading.
///
/// A job's sweep happens once, when that job ends, and a build's stray
/// process is worth waiting a second for. A probe's happens on **every**
/// reading, and once per probe on the stop path — where everything the runner
/// has to do shares the owner's five seconds (invariant 2). Ten hanging
/// probes on the job's budget would have put ten seconds of `/proc` sweeping
/// inside those five. Two rounds are what the work needs — one to signal, one
/// to see it gone — and the rest is margin for a slow host.
const PROBE_SWEEP_TRIES: u32 = 4;

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

/// One probe reading's unique name: `herd-probe-<runner pid>-<counter>`.
///
/// Minted per *reading*, not per capability: two readings of the same probe
/// never overlap (`Capacity::sweep` awaits each), but a tag that outlived one
/// reading would make the next reading's sweep able to kill the previous
/// one's leftovers and count them as its own. The runner's pid is in it for
/// the same reason it is in a job tag — so [`reclaim_a_past_life`] can tell a
/// stray whose runner is gone from one somebody is still watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTag(String);

impl ProbeTag {
    pub fn mint() -> Self {
        Self(format!(
            "{PROBE_PREFIX}{}-{}",
            std::process::id(),
            PROBES.fetch_add(1, Ordering::SeqCst)
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
    sweep_for(tag.as_str(), SWEEP_TRIES)
}

/// The same, for one probe reading, on [`PROBE_SWEEP_TRIES`].
///
/// Called after `killpg` on the probe path, which has no cgroup to fall back
/// on at all: a probe that gave itself a new session is reachable this way
/// and no other (D62).
pub fn kill_probe_by_marker(tag: &ProbeTag) -> usize {
    sweep_for(tag.as_str(), PROBE_SWEEP_TRIES)
}

/// Signal everything carrying `marker` until nothing does, or the budget is
/// out. One sweep, used by both callers above.
fn sweep_for(marker: &str, tries: u32) -> usize {
    let mut killed = 0usize;
    for _ in 0..tries {
        let carriers = processes_carrying(marker);
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
fn processes_carrying(marker: &str) -> Vec<u32> {
    let needle = format!("{MARKER_VARIABLE}={marker}");
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
    job_id: JobId,
    /// Which attempt of it: a chunk of a job's log belongs to an attempt,
    /// and this registry is where the stop path finds the one it has to say
    /// a left-behind workspace in.
    attempt: u32,
    tag: JobTag,
    pid: u32,
    cgroup: Option<PathBuf>,
    workspace: Workspace,
}

/// What became of one job the stop path had to end itself.
///
/// `said` is the line for the runner's stderr. `workspace_left` is the one
/// thing that must not stop there: bytes left on a host nobody will collect
/// are what invariant 2 forbids above all, so the caller — which has the
/// session, and therefore the job's log — puts it in the job's own record,
/// exactly as the job-end path does.
#[derive(Debug, Clone)]
pub struct EndedJob {
    pub job_id: JobId,
    pub attempt: u32,
    pub said: String,
    pub workspace: PathBuf,
    pub workspace_left: Option<String>,
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
    pub fn add(
        &self,
        job_id: JobId,
        attempt: u32,
        tag: JobTag,
        pid: u32,
        cgroup: Option<PathBuf>,
        workspace: Workspace,
    ) -> u64 {
        let token = self.next.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push((
                token,
                Live {
                    job_id,
                    attempt,
                    tag,
                    pid,
                    cgroup,
                    workspace,
                },
            ));
        }
        token
    }

    /// Say which process of this job is running now.
    ///
    /// A declared command runs four processes, one per executor step, and
    /// each gets a session of its own: the backstop below kills the process
    /// group of the pid it holds, so the pid it holds has to be the step that
    /// is running rather than the one that ran first. The marker sweep would
    /// reach the right tree anyway; this is what makes the group kill reach
    /// it *first*, which is the difference between killing a job and killing
    /// its last step's shell (invariant 2).
    pub fn now_running(&self, token: u64, pid: u32) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some((_, job)) = jobs.iter_mut().find(|(held, _)| *held == token) {
                job.pid = pid;
            }
        }
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
    /// This is the backstop of a runner being stopped: the orderly drain asks
    /// every job's own task to end its process tree and report, and this ends
    /// whatever did not manage that inside the owner's five seconds
    /// (invariant 2). A job dies with the runner that was watching it rather
    /// than becoming a process nobody owns.
    ///
    /// What it returns is per job, not one blob of text, because one of those
    /// lines has to reach somewhere a requester can read: a workspace left
    /// behind goes into the job's own record, not only onto stderr.
    pub fn end_all(&self) -> Vec<EndedJob> {
        let taken: Vec<(u64, Live)> = match self.jobs.lock() {
            Ok(mut jobs) => std::mem::take(&mut *jobs),
            Err(_) => Vec::new(),
        };
        let mut ended = Vec::new();
        for (_, job) in taken {
            if let Some(dir) = &job.cgroup {
                let _ = fs::write(dir.join("cgroup.kill"), "1");
            }
            kill_process_group(job.pid);
            let swept = kill_by_marker(&job.tag);
            let workspace_left = job.workspace.reclaim().err();
            let workspace = match &workspace_left {
                None => "its workspace reclaimed".to_owned(),
                Some(error) => format!("ITS WORKSPACE LEFT BEHIND: {error}"),
            };
            if let Some(dir) = &job.cgroup {
                remove_cgroup(dir);
            }
            ended.push(EndedJob {
                said: format!(
                    "ended job {} (pid {}, {swept} stray process(es) swept), {workspace}",
                    job.tag.as_str(),
                    job.pid
                ),
                job_id: job.job_id,
                attempt: job.attempt,
                workspace: job.workspace.base_dir().to_path_buf(),
                workspace_left,
            });
        }
        ended
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
                "killed the orphaned process {pid} of {marker}, left by a runner \
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
            match reclaim_directory(&path, root.path()) {
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

    // The spools under `outputs/`, for the same reason and with one more.
    //
    // A collection too big to ride its verdict is moved out of the workspace
    // into `<data root>/outputs/<job>-<attempt>/` and kept there until the
    // hub says it holds every byte (D19, D148). That hold lives in **this
    // process's memory**: the only place a spool is ever registered is at the
    // end of the job that produced it (`Session::end_job`), and nothing
    // anywhere reads `outputs/` back. So a runner that was `SIGKILL`ed leaves
    // bytes that no later runner can ever offer and no acknowledgement can
    // ever free — the hub asking for them is answered "this runner holds no
    // collection for it" — and until now nothing removed them either. On the
    // first consumer's own machine that is a directory that only grows, which
    // is the plainest breach of invariant 2 there is.
    //
    // Named with the figure, like every other thing reclaimed here, because
    // these are somebody's build outputs and a line saying how much went is
    // the only trace left of them.
    let spools = root.path().join(crate::outputs::SPOOL_DIR);
    if let Ok(entries) = fs::read_dir(&spools) {
        for entry in entries.flatten() {
            let path = entry.path();
            let weight = crate::root::measure_tree(&path, root.path())
                .map(crate::size::show_bytes)
                .unwrap_or_else(|_| "an unmeasurable number of".to_owned());
            match reclaim_directory(&path, root.path()) {
                Ok(()) => said.push(format!(
                    "reclaimed {weight} of output left in {} by a previous life: no runner \
                     can offer a spool it did not make, so these bytes could never have \
                     reached the hub",
                    path.display()
                )),
                Err(error) => said.push(format!(
                    "COULD NOT reclaim the leftover output spool {}: {error}",
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
///
/// Read from `/proc/<pid>/environ`, and this sweep runs **once**, from
/// [`reclaim_a_past_life`] at startup. So anything it cannot see here it does
/// not see later either, and the three limits below are worth naming
/// separately: one of them cannot cost a job process, two of them can.
///
/// **A process between `fork` and `exec` carries its parent's environment,
/// not its own.** So a child spawned in that instant shows whatever its
/// parent showed, and the marker it was given is not there yet. This one
/// **cannot** hide a process this sweep ought to kill, and the reason is
/// structural rather than lucky: the processes it hunts belong to a runner
/// that is *gone*, which means they `exec`ed long ago, and a child one of
/// them forks in the middle of this walk inherits a *marked* parent's
/// environment and is therefore visible anyway. The only invisible shape is a
/// marked child of an **unmarked** parent, and the only unmarked parent that
/// spawns marked children is a live runner starting a job — whose jobs this
/// sweep is required to leave alone (the `minter_is_gone` test below). A test
/// planting an orphan from the test binary is exactly that shape, which is
/// why the fixture waits for the marker rather than guessing at the `exec`.
///
/// **A zombie minter reads as alive**, because `/proc/<pid>` is still there
/// for a process that has exited and not been reaped — so a runner killed
/// whose supervisor has not waited on it yet is read as holding its jobs, and
/// they are spared. Narrow: it needs a new runner to start inside that window,
/// and systemd reaps its own children at once. Permanent when it happens,
/// because this sweep does not run again.
///
/// **A reused pid reads as alive** for the same reason: the kernel hands pids
/// out again, and a marker naming a pid that now belongs to something else is
/// read as a live runner's. Inherent to naming a runner by its pid; closing it
/// needs an identity a pid alone cannot give — the process start time in
/// `/proc/<pid>/stat` beside it, so that "pid 4211 as it was then" is
/// distinguishable from "pid 4211 now".
///
/// The last two are **not** covered by the cgroup half of
/// [`reclaim_a_past_life`] either: that reads the runner's pid out of the
/// cgroup's name and asks the same `minter_is_gone`. A job whose own command
/// scrubs its environment (`env -i`) is a third thing and *is* covered there,
/// because a cgroup is killed wholesale by name and needs no marker.
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

/// The runner pid inside a job tag, a probe tag, a cgroup name or a marker.
///
/// A probe tag is read here as well as a job's, which is what makes the
/// startup reaper collect a probe stray left by a runner that was
/// `SIGKILL`ed: `orphaned_job_processes` finds the marker either way and
/// needs only to know whose it was.
fn pid_in(name: &str) -> Option<u32> {
    name.strip_prefix(JOB_PREFIX)
        .or_else(|| name.strip_prefix(PROBE_PREFIX))
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

    /// **The leak invariant 2 cared about most, closed.** A collection too
    /// big to ride its verdict is moved to `<data root>/outputs/<job>-<n>/`
    /// and removed only on the hub's acknowledgement
    /// (`herd-runner/src/lib.rs`, `files_acknowledged`). A runner that was
    /// `SIGKILL`ed never gets that acknowledgement, and the next runner
    /// cannot offer the bytes either — the only place a spool is registered
    /// is the end of the job that produced it — so before this they stayed on
    /// the consumer's disk for ever.
    #[test]
    fn the_reaper_reclaims_an_output_spool_a_previous_life_left_behind() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = crate::testing::TempDir::new("reclaim-spool");
        let root = DataRoot::open(temp.path()).expect("a throwaway data root");
        let spool = temp.path().join("outputs").join("j0000000001-1");
        fs::create_dir_all(&spool).expect("the spool directory");
        fs::write(spool.join("results.json"), vec![b'x'; 4096]).expect("the spooled file");
        let ceiling = Ceiling::detect(
            crate::confine::Confinement::with_data_root(temp.path()).job_limits(),
            None,
        );

        let said = reclaim_a_past_life(&root, &ceiling);

        assert!(
            !spool.exists(),
            "a spool no runner can ever offer must not outlive the runner that made it"
        );
        assert!(
            said.iter().any(|line| line.contains("j0000000001-1")),
            "and what went has to be named, with the figure: {said:?}"
        );
    }

    /// A tag reaches the filesystem as a cgroup name and a path, so an id
    /// shaped like a path must not survive in it.
    #[test]
    fn a_hostile_job_id_is_sanitised_into_the_tag() {
        let tag = JobTag::mint(&JobId::new("../../etc/passwd"));
        assert!(!tag.as_str().contains('/'), "{}", tag.as_str());
        assert!(!tag.as_str().contains(".."), "{}", tag.as_str());
    }

    /// The startup reaper, asserted on a real process: a job process whose
    /// runner is gone is killed, and one whose runner is alive is not.
    ///
    /// The dead runner is played by a pid that really has exited, which is
    /// exactly what the reaper has to recognise; the live one is this test
    /// process. Deterministic, because neither claim depends on what any
    /// other runner on this host happens to be doing.
    #[test]
    #[cfg(unix)]
    fn the_reaper_kills_a_job_whose_runner_is_gone_and_spares_one_whose_runner_is_not() {
        // Nothing else in this binary may reap while this test has an orphan
        // planted: another reaper killing it first is the one flake this
        // assertion ever had.
        let _reaping = crate::testing::one_reaper_at_a_time();
        let gone = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("the fixture starts");
        let dead_pid = gone.id();
        let mut gone = gone;
        let _ = gone.wait();
        assert!(minter_is_gone(dead_pid), "the fixture's pid is still there");
        assert!(
            !minter_is_gone(std::process::id()),
            "this process is alive, so its jobs are nobody else's to take"
        );

        // Two processes carrying a marker each: one minted by a runner that is
        // gone, one by this one.
        let mut orphan = spawn_marked(&format!("{JOB_PREFIX}{dead_pid}-0-orphan"));
        let mut watched = spawn_marked(JobTag::mint(&JobId::new("watched")).as_str());

        let found = orphaned_job_processes();
        assert!(
            found.iter().any(|(pid, _)| *pid == orphan.id()),
            "the orphan {} was not found among {found:?}",
            orphan.id()
        );
        assert!(
            !found.iter().any(|(pid, _)| *pid == watched.id()),
            "a live runner's job was taken for an orphan: {found:?}"
        );

        for (pid, _) in found {
            kill_pid(pid);
        }
        // These are this test's own children, so their exit has to be waited
        // for; a real orphan belongs to init, which reaps it.
        let ended = orphan.wait().expect("the orphan's exit is readable");
        assert!(!ended.success(), "the orphan was not killed: {ended:?}");
        assert!(
            fs::metadata(format!("/proc/{}", watched.id())).is_ok(),
            "the watched job was killed with the orphan"
        );
        kill_pid(watched.id());
        let _ = watched.wait();
    }

    #[cfg(unix)]
    fn spawn_marked(marker: &str) -> std::process::Child {
        let child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 20")
            .env_clear()
            .env(MARKER_VARIABLE, marker)
            // PATH, so that the shell can find `sleep`: a built environment
            // with no PATH is a shell that exits at once.
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("the fixture starts");
        // **The environment is not readable as soon as the process exists.**
        // Between `fork` and `exec` the child's `/proc/<pid>/environ` is still
        // its parent's, and the parent does not carry the marker — so a sweep
        // run in that window finds nothing at all, which is exactly how this
        // test failed: "the orphan was not found among []", an empty list
        // rather than a race lost to another reaper. A hundred milliseconds
        // stood here, which was a guess at how long an `exec` takes. This
        // waits for the thing the sweep reads, and says which way it went
        // wrong if it never comes.
        wait_until_marked(child.id(), marker);
        child
    }

    /// Wait until `pid` carries `marker` where [`orphaned_job_processes`]
    /// looks for it.
    #[cfg(unix)]
    fn wait_until_marked(pid: u32, marker: &str) {
        let wanted = format!("{MARKER_VARIABLE}={marker}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let carried = fs::read(format!("/proc/{pid}/environ"))
                .map(|environ| {
                    environ
                        .split(|byte| *byte == 0)
                        .filter_map(|entry| std::str::from_utf8(entry).ok())
                        .any(|entry| entry == wanted)
                })
                .unwrap_or(false);
            if carried {
                return;
            }
            // Which of the two ways it failed, because they need different
            // fixes: a fixture that is gone never ran `sleep` at all, and a
            // fixture that is there but unmarked is a sweep reading the wrong
            // place.
            assert!(
                std::time::Instant::now() < deadline,
                "the fixture {pid} never carried {wanted} where the sweep reads it, and {}",
                if minter_is_gone(pid) {
                    "it is gone: it exited before it could be swept, so this host's PATH may \
                     not reach a `sleep`"
                } else {
                    "it is still running, so its environ never showed the marker"
                }
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// The sweep is the whole of the fallback's new strength, so it is
    /// asserted against a real process that has left its process group.
    #[test]
    #[cfg(unix)]
    fn the_sweep_kills_a_process_that_left_its_process_group() {
        let _reaping = crate::testing::one_reaper_at_a_time();
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
        let carriers = processes_carrying(tag.as_str());
        assert!(
            carriers.len() >= 2,
            "the fixture should show the shell and its detached child: {carriers:?}"
        );

        let killed = kill_by_marker(&tag);
        assert!(killed >= 2, "only {killed} process(es) were swept");
        assert!(
            processes_carrying(tag.as_str()).is_empty(),
            "the sweep left something behind: {:?}",
            processes_carrying(tag.as_str())
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
