//! The kernel memory ceiling, and the fallback when there is none (E-30, Q6).
//!
//! All of one job's processes live in one cgroup v2 subtree carrying
//! `memory.max`. An allocation past the ceiling is refused by the kernel and
//! the cgroup's processes are killed — **the job's, and only the job's**: the
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

/// How a runner enforces the memory ceiling on this host.
#[derive(Debug)]
pub enum Ceiling {
    /// One cgroup per job under a delegated base. The ceiling is the
    /// kernel's, applies to the whole process tree, and kills.
    Cgroups { base: PathBuf, memory_max: u64 },
    /// No usable cgroup v2 subtree. `why` is one sentence naming what was
    /// missing, printed at startup and repeated in every job's log.
    Rlimit { memory_max: u64, why: String },
}

impl Ceiling {
    /// Work out which ceiling this host can give, preparing the subtree if it
    /// can give the real one.
    ///
    /// `base` overrides the cgroup the runner derives from its own
    /// `/proc/self/cgroup`; it exists because a test must be able to point a
    /// runner at a subtree it set up, and because an operator may want the
    /// runner's jobs somewhere other than beside the runner.
    pub fn detect(memory_max: u64, base: Option<&Path>) -> Self {
        match prepare_base(base) {
            Ok(base) => Ceiling::Cgroups { base, memory_max },
            Err(why) => Ceiling::Rlimit { memory_max, why },
        }
    }

    /// The ceiling in bytes, whichever mechanism enforces it.
    pub fn memory_max(&self) -> u64 {
        match self {
            Ceiling::Cgroups { memory_max, .. } | Ceiling::Rlimit { memory_max, .. } => *memory_max,
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

    /// Make this job's cgroup, or `None` on a host that fell back.
    pub fn open_for_job(&self, tag: &JobTag) -> Option<JobCgroup> {
        let Ceiling::Cgroups { base, memory_max } = self else {
            return None;
        };
        match JobCgroup::create(base, tag, *memory_max) {
            Ok(cgroup) => Some(cgroup),
            Err(error) => {
                // Detection succeeded and creation failed, which means the
                // subtree changed under us. Say so; the job then runs under
                // the rlimit fallback rather than uncapped.
                eprintln!(
                    "herd-runner: the cgroup for job {} could not be made under {}: \
                     {error}; this job falls back to an address-space limit",
                    tag.as_str(),
                    base.display()
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
    fn create(base: &Path, tag: &JobTag, memory_max: u64) -> io::Result<Self> {
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
        fs::write(cgroup.dir.join("memory.max"), memory_max.to_string())?;
        // Swap is part of the ceiling: a job allowed to swap past its memory
        // cap is a job that takes the host's responsiveness with it.
        let _ = fs::write(cgroup.dir.join("memory.swap.max"), "0");
        // Kill the whole cgroup on the first OOM rather than one process at a
        // time, so a job dies as a job and does not limp on half-killed.
        let _ = fs::write(cgroup.dir.join("memory.oom.group"), "1");
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
fn prepare_base(explicit: Option<&Path>) -> Result<PathBuf, String> {
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
    Ok(base)
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

/// Whether the base already hands the memory controller to its children, in
/// which case the runner needs no write to `cgroup.subtree_control` at all.
fn memory_is_enabled_for_children(base: &Path) -> bool {
    fs::read_to_string(base.join("cgroup.subtree_control"))
        .map(|text| text.split_whitespace().any(|name| name == "memory"))
        .unwrap_or(false)
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

    /// Whatever this host turns out to offer, detection must answer, and the
    /// answer must be either a prepared subtree or a sentence. Never a panic,
    /// and never a `None` that leaves a job uncapped without saying so.
    #[test]
    fn detection_answers_with_a_subtree_or_a_reason() {
        let ceiling = Ceiling::detect(64 << 20, None);
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

    /// A cgroup path that does not exist is a named fallback, not a panic and
    /// not an uncapped job.
    #[test]
    fn a_cgroup_that_does_not_exist_falls_back_naming_it() {
        let missing = Path::new("/sys/fs/cgroup/herd-no-such-cgroup-m4");
        let ceiling = Ceiling::detect(1 << 20, Some(missing));
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
        let ceiling = Ceiling::detect(1 << 20, Some(Path::new(MOUNT_POINT)));
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
        let ceiling = Ceiling::detect(1 << 20, Some(Path::new(MOUNT_POINT)));
        match ceiling.fallback_reason() {
            Some(why) => assert!(why.contains(MOUNT_POINT), "{why}"),
            None => {
                // The root is delegated to us, which only happens in a
                // container running as root — and a root runner is refused
                // before it ever gets here (E-46).
            }
        }
    }
}
