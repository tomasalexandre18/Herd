//! Running one job, under every cap at once, on a link that may go away.
//!
//! The order matters and is the whole of E-34 and E-30:
//!
//! 1. A workspace inside the data root, empty, and the job's working
//!    directory. Reclaimed when the job ends, whatever the job did to it —
//!    including the modes it left behind.
//! 2. A cgroup with the memory ceiling set **before** any process is in it.
//! 3. The job's own session, so every process it starts shares one group, and
//!    one marker in its environment, so the ones that leave that group can
//!    still be found.
//! 4. Nice on CPU and the idle class on IO.
//! 5. A time cap, a log cap and a disk budget, each ending the job with a
//!    named reason rather than letting it hang, flood or fill a disk.
//! 6. A kill of the whole cgroup, the whole process group and every process
//!    still carrying the job's marker, when the job's own process is gone —
//!    so a job that leaves a background child behind does not outlive itself.
//! 7. What was measured, reported with the outcome (E-15).
//!
//! ## One job, one or four processes
//!
//! A job is either an **ad-hoc** command line — one process, which is every
//! job Herd ran before M10 and how Herd is tested against itself with no
//! consumer tree in the picture (E-11) — or a **declared command**, which is
//! the consumer's four executor steps run in order through one shell each
//! ([`crate::step`], D108). Both go through this one path, so the log, the
//! caps, the cancellation, the spool and the verdict are the same machinery
//! for both; what differs is the list of stages and what each stage's ending
//! means.
//!
//! The caps that are about the whole attempt — the memory ceiling, the log
//! cap, the disk budget — are set once and hold across every step. The time
//! cap is per step, because a cold cache pull in `up` must not eat a build's
//! declared minutes (D108).
//!
//! Nothing is killed *between* steps. The four steps are one job sharing one
//! workspace, so a process `up` started is part of the job and may well be
//! what `run` needs; the cgroup, the process group and the marker sweep take
//! everything once, when the whole attempt is over. A step a cap or a cancel
//! stopped is killed at once, which is the enforcement itself.
//!
//! ## A cap outranks the job's own exit status
//!
//! A breached cap is the end of the job, whatever the process did afterwards.
//! A job whose log was cut at the cap may still exit 0 a millisecond later,
//! and a job whose output pipe was closed under it exits on `SIGPIPE`: both
//! used to be reported as the job's own verdict, which made a truncated build
//! look green. So the cap is checked once more after the process has gone,
//! and whatever it says wins (D24, D25, D31).
//!
//! **The memory ceiling is the one of the four the runner does not watch**,
//! and it is read instead of watched: the kernel kills, and the job cgroup's
//! own counters are what say so afterwards ([`OomWatermark`]). The three the
//! watchdog trips outrank an exit status as above; the kernel's kill is
//! charged to the step whose processes it took, and a step that exited 0 is
//! not one of them — with `memory.oom.group = 1` a shell the kernel killed
//! has no code to report at all. That is the one place where this section's
//! rule is read the other way round, and the reason is in `OomWatermark`.
//!
//! ## Ended from outside, and by whom
//!
//! A job is also ended by a requester's cancel (E-06), by a drain (invariant
//! 2), or by the hub writing its attempt off (E-21). All three arrive through
//! one [`crate::CancelToken`], and all three end in the same kill as a cap
//! does — the cgroup, the process group, the marker sweep. What differs is
//! what the hub is told afterwards: a cancel is an outcome
//! ([`JobOutcome::Cancelled`]), while an abandoned attempt has no outcome at
//! all, because the runner is not saying how the job went, it is saying this
//! go did not happen.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use herd_proto::{
    Cap, Declaration, JobId, JobMetrics, JobOutcome, LogStream, Step, StepRun, ToHub,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use crate::cgroup::{JobCgroup, OomKills};
use crate::confine::{
    job_environment, kill_process_group, ChildSetup, Confined, JobsAtOnce, POLL_INTERVAL,
};
use crate::metrics::{host_load_milli, process_rss_bytes};
use crate::reclaim::{kill_by_marker, JobTag};
use crate::root::{DataRoot, Unmeasurable, Workspace};
use crate::step::{self, Stage};
use crate::{CancelToken, Ending, NotTaken, Session};

/// Output is read in chunks this size; a chunk is sent as soon as the process
/// has written anything, which is what makes a log arrive while it is running.
const READ_CHUNK: usize = 8 * 1024;

/// How long the runner waits for a killed job's pipes to close before giving
/// up on them. A kill by cgroup or by process group is immediate; this bound
/// exists so that one wedged descriptor cannot hold a job's report forever.
///
/// It is also what bounds the gap between two steps: a step that left a
/// process holding its stdout is waited for this long and no longer, so the
/// next step's output cannot be held up for ever by the last step's stray
/// child.
///
/// A drain has its own, shorter budget ([`crate::DRAIN_GRACE`]) and does not
/// wait on this: a runner being stopped gives the machine back inside five
/// seconds whatever a descriptor is doing (invariant 2), and the hub then
/// records that attempt as lost rather than the runner reporting it late.
const ENDED_JOB_OUTPUT_GRACE: Duration = Duration::from_secs(5);

/// Which cap tripped, as one atom the log pumps and the watchdog share.
const NO_CAP: u8 = 0;
const LOG_CAP: u8 = 1;

/// The packs an assignment resolved, as `FromHub::AssignJob` handed them over
/// (M11, E-12).
///
/// One struct and not two arguments because the two halves are one subject
/// and are judged together: what this job consumes under `{in}`, and what it
/// is to leave under `{out}`.
pub struct Packs {
    /// Each `needs` key, resolved by the hub: the manifest and the holders.
    /// Empty is the ordinary case and means the job needs no pack.
    pub packs_in: Vec<herd_proto::pack::AssignedPack>,
    /// Each `leaves` key and the path under `{out}` the job writes it at.
    /// Empty means the job leaves nothing to register.
    pub packs_out: std::collections::BTreeMap<String, String>,
}

impl Packs {
    /// Why this job cannot be run **at all**, when its packs say it cannot.
    ///
    /// One case and not a list: a job that asks for or leaves a pack and is
    /// not a declared command. `{in}` and `{out}` are made only for a
    /// declared command (D107, [`crate::root`]), so there is nowhere to lay a
    /// pack out and nowhere to take one from — and a pack is registered for a
    /// `(commit, key)` pair, which only a declared command names (E-11,
    /// D106). The hub refuses to resolve such a job's keys for that same
    /// reason, so this is the runner saying the same thing from its own end
    /// rather than a second rule.
    ///
    /// **The attempt is handed back and the job is not run**, because the one
    /// outcome that must not happen is a job that runs without its inputs and
    /// reports success: `{in}/<key>` would be absent, `{out}` would be swept
    /// with nothing registered, and the pack a later job waits for would
    /// never exist. That is D150's silence exactly.
    ///
    /// `None` is every job Herd runs today, and every declared one.
    fn why_this_runner_cannot(&self, declared: bool) -> Option<String> {
        if declared || (self.packs_in.is_empty() && self.packs_out.is_empty()) {
            return None;
        }
        let keys = |keys: Vec<&String>| {
            keys.into_iter()
                .map(String::as_str)
                .collect::<Vec<&str>>()
                .join(", ")
        };
        let mut asks = Vec::new();
        if !self.packs_in.is_empty() {
            asks.push(format!(
                "needs the pack(s) {}",
                keys(self.packs_in.iter().map(|pack| &pack.key).collect())
            ));
        }
        if !self.packs_out.is_empty() {
            asks.push(format!(
                "leaves the pack(s) {}",
                keys(self.packs_out.keys().collect())
            ));
        }
        Some(format!(
            "this job {} and is not a declared command, so it has no {} to lay a pack out in \
             and no {} to take one from, and it names no commit to register one for. Running it \
             would report success for a job that never saw its inputs, so the attempt is handed \
             back instead",
            asks.join(" and "),
            herd_proto::IN_TOKEN,
            herd_proto::OUT_TOKEN,
        ))
    }
}

/// Run one job: announce it, stream every step's output, cap each step,
/// report how it ended and what it cost.
///
/// Nine arguments, and they are the job: what to run, how it was declared,
/// which packs it consumes and leaves, what it is allowed to cost, who to
/// tell and how full this machine already is. Collapsing them into a struct
/// would name the struct after this function and move the same list one line
/// up.
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    job_id: JobId,
    attempt: u32,
    command: Vec<String>,
    declaration: Option<Declaration>,
    declared_timeout: Option<Duration>,
    packs: Packs,
    session: Session,
    confined: Arc<Confined>,
    ceiling: Arc<JobsAtOnce>,
) {
    // Taken in hand first, before a byte is written anywhere: that is what
    // numbers this job's output, holds its result against an acknowledgement,
    // and gives back the token every way of ending it goes through.
    //
    // It is also the one place this machine's own limits are held. Assigned
    // while this runner takes no job — because it is stopping (invariant 2),
    // because its owner has withdrawn the machine (E-33), or because it is
    // already as full as its own caps let it be (invariant 2 again) — the
    // attempt is handed straight back, so the job waits for a runner that
    // will actually run it rather than failing. The reason names which of the
    // three it was, because "the PC said no", "she is gaming on the PC" and
    // "that machine is full" are different facts about different machines.
    let cancel = match session.take_job(&job_id, attempt, ceiling.most()) {
        Ok(cancel) => cancel,
        // A refusal for this machine's own ceiling says so in a field as well
        // as in the sentence (D182): the hub charges a job a go for an
        // attempt that ended *because of the runner*, and a machine that is
        // here, healthy and holding exactly what it said it would hold is not
        // that. The other two refusals are about this runner going away —
        // draining, or withdrawn by its owner — and are counted as they
        // always were.
        Err(not_taken @ (NotTaken::AtItsCeiling { .. } | NotTaken::StillRunningThisJob { .. })) => {
            let running = match not_taken {
                NotTaken::AtItsCeiling { running, .. }
                | NotTaken::StillRunningThisJob { running, .. } => running,
                _ => unreachable!("the pattern above admits nothing else"),
            };
            session.abandon_at_its_ceiling(
                &job_id,
                attempt,
                &not_taken.sentence(&ceiling),
                running,
            );
            return;
        }
        Err(not_taken) => {
            session.abandon(&job_id, attempt, &not_taken.sentence(&ceiling));
            return;
        }
    };

    // Before anything is made on this host: a job whose packs this runner
    // cannot serve at all. Handed back rather than run, and before the
    // workspace exists, because the one outcome that must not happen is a job
    // that runs without its inputs and reports success (D150).
    if let Some(refusal) = packs.why_this_runner_cannot(declaration.is_some()) {
        eprintln!("herd-runner: handing {job_id} back: {refusal}");
        session.abandon(&job_id, attempt, &refusal);
        return;
    }

    // Before anything is made on this host: a command line nobody could run.
    if let Some(declaration) = &declaration {
        if let Some(refusal) = step::why_that_line_will_not_do(declaration, &command) {
            session.abandon(&job_id, attempt, &refusal);
            return;
        }
    } else if command.is_empty() {
        report_could_not_start(
            &job_id,
            attempt,
            &session,
            "a job needs a command to run".to_owned(),
        );
        return;
    }

    // E-45: the job gets one place to work, inside the one data root. A root
    // that will not give one is a refusal naming the path, not a job that
    // runs in the runner's own directory.
    let workspace = match confined.root().open_workspace(&job_id) {
        Ok(workspace) => workspace,
        Err(reason) => {
            report_could_not_start(&job_id, attempt, &session, reason);
            return;
        }
    };

    let caps = confined.caps();
    let stages = match &declaration {
        // Exactly the job Herd has run since M4: one command line, one
        // process, the runner's own cap, and no step on the record at all.
        None => step::ad_hoc(&command, caps.timeout).into_iter().collect(),
        Some(declaration) => {
            // `{in}` and `{out}` exist before `up` runs, under this job's own
            // directory and therefore inside the one data root (D107, E-45).
            if let Err(reason) = workspace.open_exchange() {
                reclaim_or_say(confined.root(), &workspace, &job_id, attempt, &session);
                report_could_not_start(&job_id, attempt, &session, reason);
                return;
            }
            match step::planned(
                declaration,
                &job_id,
                step::Places {
                    in_dir: workspace.in_dir(),
                    out_dir: workspace.out_dir(),
                    ran: &workspace.ran_path(),
                },
                &command[0],
                declared_timeout,
                caps.timeout,
            ) {
                Ok(stages) => stages,
                Err(refusal) => {
                    reclaim_or_say(confined.root(), &workspace, &job_id, attempt, &session);
                    session.abandon(&job_id, attempt, &refusal);
                    return;
                }
            }
        }
    };
    if stages.is_empty() {
        reclaim_or_say(confined.root(), &workspace, &job_id, attempt, &session);
        report_could_not_start(
            &job_id,
            attempt,
            &session,
            "a job needs a command to run".to_owned(),
        );
        return;
    }

    // **What this job consumes, laid out under `{in}` before a step runs**
    // (M11, E-12, D10). Here and not earlier because `{in}` is made with the
    // workspace; here and not later because `up` is the first thing that may
    // read it, and the consumer's own `run` line reads `{in}/<key>` directly.
    //
    // A pack that cannot be got hands the attempt back with the sentence the
    // fetch wrote, and the hub has already been told `PackUnavailable` by
    // name: a job whose `{in}/<key>` is empty must not be a green-looking job
    // with no error anywhere (D150). The workspace is reclaimed on the way
    // out, exactly as a refused command line reclaims it above.
    if let Some(declaration) = &declaration {
        match session
            .lay_out_packs(
                &job_id,
                &declaration.commit,
                confined.root().path(),
                workspace.in_dir(),
                &packs.packs_in,
            )
            .await
        {
            Ok(said) => {
                for line in said {
                    eprintln!("herd-runner: job {job_id} {line}");
                }
            }
            Err(refusal) => {
                eprintln!("herd-runner: handing {job_id} back: {refusal}");
                reclaim_or_say(confined.root(), &workspace, &job_id, attempt, &session);
                session.abandon(&job_id, attempt, &refusal);
                return;
            }
        }
    }

    // One name for this job's cgroup, its marker and the runner's own log
    // lines about it. Unique per runner process, so one job's kill can never
    // reach another job's processes.
    let tag = JobTag::mint(&job_id);
    let cgroup = confined.ceiling().open_for_job(&tag);

    // Counted across every step and both streams: the log the hub keeps and
    // the requester reads is one log, whatever produced it.
    let log_bytes = Arc::new(AtomicU64::new(0));
    let tripped = Arc::new(AtomicU8::new(NO_CAP));

    let mut ran = Attempt {
        // Only a declared command has a `fetch` step to answer for, and only
        // it is given `{ran}` to answer with (D176).
        ran_path: declaration.as_ref().map(|_| workspace.ran_path()),
        ..Attempt::default()
    };
    // The attempt's own clock, started by the first process there is.
    let mut started: Option<Instant> = None;
    // On the live list from the first process on, so a runner that is stopped
    // — or that dies — leaves no process, no cgroup and no workspace to
    // nobody. The pid on it is whichever step is running.
    let mut live: Option<u64> = None;
    let mut pids: Vec<u32> = Vec::new();
    // One cgroup per job and not per step, so its kill counters are
    // job-scoped by construction and every step-level reading of them carries
    // its own baseline. The watermark is that baseline.
    let mut oom_mark = OomWatermark::default();

    for stage in &stages {
        // `down` is the one stage that runs however the others ended, and it
        // is not a condition but the opposite: a step that only ran on
        // success would leak a workspace on every failure (D108). Everything
        // else is skipped once the job is over — there is no workspace left
        // to run it in.
        if ran.over() && stage.step != Some(Step::Down) {
            continue;
        }
        // The two figures that meet on `run`, in the job's own log before the
        // step runs: the record has to name which of them bound the job
        // however the job ends (D108).
        if let Some(said) = &stage.announce {
            say_in_the_log(&session, &job_id, attempt, said);
        }

        let step_started_unix_ms = unix_ms_now();
        let step_started = Instant::now();

        // The descriptor is opened here, in the runner, and written by the
        // step's own process before it execs: that is what leaves no instant
        // in which a job process exists outside its ceiling. Reopened per
        // step, so the runner holds no copy of it while anything runs.
        let procs = match cgroup.as_ref().map(JobCgroup::procs_file).transpose() {
            Ok(procs) => procs,
            Err(error) => {
                ran.step_never_started(
                    stage,
                    StepAt {
                        started_unix_ms: step_started_unix_ms,
                        took: step_started.elapsed(),
                    },
                    format!("the job's memory cgroup could not be opened for writing: {error}"),
                    &session,
                    &job_id,
                    attempt,
                );
                continue;
            }
        };
        let setup = ChildSetup {
            cgroup_procs_fd: procs
                .as_ref()
                .map(|file| {
                    use std::os::fd::AsRawFd;
                    file.as_raw_fd()
                })
                .unwrap_or(-1),
            nice: caps.nice,
            // Only where there is no cgroup: the two must never be stacked, or
            // a job under the real ceiling would also hit a per-process limit
            // and report the wrong thing.
            address_space_max: match cgroup {
                Some(_) => None,
                None => Some(caps.memory_max),
            },
        };

        let mut builder = Command::new(&stage.program);
        builder
            .args(&stage.arguments)
            .current_dir(workspace.work_dir())
            // Nothing of the runner's environment is a job's business.
            .env_clear()
            .envs(job_environment(&workspace, &tag))
            // The declaration's own environment last, so a consumer that
            // names a variable Herd also sets gets the value it declared.
            // That is no weaker than the hook itself, which is a shell line
            // and could set anything.
            .envs(stage.env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            // Safe: `apply` does syscalls only, and holds nothing that a fork
            // could have left locked.
            unsafe {
                builder.pre_exec(move || setup.apply());
            }
        }

        let mut child = match builder.spawn() {
            Ok(child) => child,
            Err(error) => {
                // Names whichever of the two missing things is actually
                // missing, so the reason reads as Herd failing to run this
                // command rather than the command failing — and so it does
                // not send the reader after a shell that is there (D151).
                ran.step_never_started(
                    stage,
                    StepAt {
                        started_unix_ms: step_started_unix_ms,
                        took: step_started.elapsed(),
                    },
                    why_a_step_could_not_start(&stage.program, workspace.work_dir(), &error),
                    &session,
                    &job_id,
                    attempt,
                );
                continue;
            }
        };
        // The runner holds no copy of the descriptor while the step runs.
        drop(procs);
        let pid = child.id().unwrap_or(0);
        pids.push(pid);
        match live {
            None => {
                started = Some(Instant::now());
                live = Some(confined.live().add(
                    job_id.clone(),
                    attempt,
                    tag.clone(),
                    pid,
                    cgroup.as_ref().map(|cgroup| cgroup.dir().to_path_buf()),
                    workspace.clone(),
                ));
                session.send(ToHub::JobStarted {
                    job_id: job_id.clone(),
                    attempt,
                });
            }
            // The stop path's backstop kills the step that is running now,
            // not the one that ran first.
            Some(token) => confined.live().now_running(token, pid),
        }

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let pumps = tokio::spawn({
            let pump_out = stream_output(
                stdout,
                LogStream::Stdout,
                job_id.clone(),
                attempt,
                Arc::clone(&log_bytes),
                Arc::clone(&tripped),
                caps.log_cap,
                session.clone(),
            );
            let pump_err = stream_output(
                stderr,
                LogStream::Stderr,
                job_id.clone(),
                attempt,
                Arc::clone(&log_bytes),
                Arc::clone(&tripped),
                caps.log_cap,
                session.clone(),
            );
            async move {
                tokio::join!(pump_out, pump_err);
            }
        });

        let watched = watch(
            &mut child,
            Watch {
                pid,
                deadline: step_started + stage.timeout,
                disk_budget: caps.disk_budget,
                workspace: workspace.clone(),
                cgroup: cgroup.as_ref(),
                tripped: Arc::clone(&tripped),
                tag: tag.clone(),
                job_id: job_id.clone(),
                // Nothing more can be asked of a job that was already asked
                // once — [`CancelToken::end`] refuses a second ask — so a
                // `down` running after a cancel is watching for something
                // that cannot arrive, and watching it would end `down` the
                // instant it started. A drain during a `down` that nobody had
                // cancelled is still heard, and a `down` that outlives a drain
                // is killed by the stop path's own backstop (invariant 2).
                cancel: ran.ending.as_ref().map_or(Some(&cancel), |_| None),
            },
        )
        .await;

        // Read the kernel's own verdict before killing anything ourselves, so
        // that an out-of-memory kill is told apart from the runner's kill —
        // and charged to the step it happened in and to no other
        // ([`OomWatermark`]).
        let now = cgroup
            .as_ref()
            .map_or_else(OomKills::default, JobCgroup::oom_kills);
        let oomed = oom_mark.killed_in_this_step(now, &watched.outcome);

        // Both pipes are drained before the step's line is recorded, so the
        // steps' output reaches the log in the order the steps ran —
        // bounded, because a descriptor held by something we failed to kill
        // must not hold the job.
        if tokio::time::timeout(ENDED_JOB_OUTPUT_GRACE, pumps)
            .await
            .is_err()
        {
            eprintln!("herd-runner: job {job_id} left a pipe open after its step ended");
        }

        ran.step_ended(
            stage,
            StepAt {
                started_unix_ms: step_started_unix_ms,
                took: step_started.elapsed(),
            },
            watched,
            oomed,
            caps,
            &session,
            &job_id,
            attempt,
        );
    }

    let peak_memory_bytes = cgroup
        .as_ref()
        .and_then(JobCgroup::peak_bytes)
        .or(ran.peak_rss);

    // E-32's figure, taken while the job's own cgroup is still there:
    // recorded with the job, reported, and a gate on nothing (D119).
    let pressure = crate::metrics::pressure(cgroup.as_ref().map(JobCgroup::dir));

    // Whatever the job left behind dies with it, every time: the job's own
    // processes being gone is not the same thing as the job being over
    // (E-34, E-06's kill hardened). Every step's process group, because each
    // step got a session of its own, and then the marker sweep for anything
    // that left them.
    kill_everything_of(cgroup.as_ref(), &pids, &tag, &job_id);

    // The last measurement, taken once nothing of the job is running: this is
    // the figure a job that wrote and exited at once is caught by.
    let final_disk = workspace.measure();
    if let Err(reason) = &final_disk {
        ran.unmeasurable = Some(reason.clone());
    }
    // Only for a job nothing has already ended. A job somebody ended, one a
    // cap stopped, and one whose preparation never came off have each got
    // their reason, and "stopped at its disk cap" would replace a true reason
    // with a cap that is not why the job is over.
    if ran.nothing_ended_it() {
        if let Some(unmeasurable) = ran.unmeasurable.clone() {
            // A workspace the runner cannot measure is a disk budget it
            // cannot enforce, and an unenforced budget must never read as a
            // job that passed. The figure is absent and the job ends either
            // way; what the ending is *called* turns on why the measurement
            // failed (D166).
            ran.ended_unmeasurable(&unmeasurable, caps.disk_budget);
        } else if final_disk
            .as_ref()
            .is_ok_and(|used| *used > caps.disk_budget)
        {
            ran.cap = Some(Capped {
                cap: Cap::Disk,
                limit: caps.disk_budget,
                detail: None,
            });
        }
    }
    if let Some(unmeasurable) = &ran.unmeasurable {
        eprintln!(
            "herd-runner: job {job_id} could not be held to its disk budget, because \
             its workspace could not be measured: {}{}",
            unmeasurable.why,
            if unmeasurable.refused {
                // Said here too, on the machine's own stderr, because the
                // person who can fix this is the one reading this journal
                // (D166).
                ". This runner was refused that path rather than reaching a budget, so the \
                 job is reported as a setup failure and not as a disk cap"
            } else {
                ""
            }
        );
    }

    let outcome = ran.outcome();

    // A figure the runner could not take is absent, never zero (D23, D30): a
    // measured zero and "the job hid what it wrote" must never read alike.
    let bytes_written = match (ran.unmeasurable.is_some(), final_disk) {
        (true, _) => None,
        (false, Ok(used)) => Some(ran.peak_disk.unwrap_or(0).max(used)),
        (false, Err(_)) => None,
    };
    let metrics = JobMetrics {
        host_load_milli: host_load_milli(),
        peak_memory_bytes,
        bytes_written,
        log_bytes: log_bytes.load(Ordering::SeqCst),
        wall_time_ms: started
            .map(|started| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0),
        pressure,
    };

    // What the job left under `{out}`, taken **before** the workspace goes
    // and on every ending that produced one: a success, a non-zero exit, a
    // cap, a cancel. D108 fixed a workspace leaking on failure for this class
    // of reason, and the reasoning runs the same way here — the job that
    // fails is very often the one whose output you most want, so this has no
    // success condition (see [`crate::outputs`]).
    //
    // Nothing is collected for an **abandoned** attempt, and that is not an
    // omission: an abandoned attempt says nothing about the job at all, there
    // is no frame for its outputs to ride on, and the attempt that follows
    // makes its own `{out}` from nothing. A runner that is *killed* leaves no
    // frame either, and its workspace is reclaimed by the next start's reaper
    // — that is the lost-attempt case the hub requeues (E-21), not an output
    // Herd dropped on the floor.
    //
    // A collection too big for the verdict frame is **renamed** out of the
    // workspace into the spool below instead, where it waits for the hub to
    // take it (D148). The spool is under the data root for one reason that is
    // not tidiness: it is on the same filesystem as `{out}`, so moving
    // 400 MiB out of a workspace that is about to be reclaimed is a directory
    // entry and not a copy.
    // And the packs this job declared it **leaves** go into this runner's
    // pack store and out of `{out}` before that collection reads the
    // directory — inside `collect`, which is the one place that ordering can
    // be stated once (ORCH-153, [`crate::packstore::leave_packs`]). A ~400
    // MiB pack left where it is would be collected as the job's output
    // evidence and streamed runner → hub → requester as if it were a result.
    let collected = match &ran.ending {
        Some(Ending::Abandoned(_)) => crate::outputs::Collected::nothing(),
        _ => crate::outputs::collect(
            workspace.out_dir(),
            confined.root().path(),
            caps.output_cap,
            &crate::outputs::spool_path(confined.root().path(), &job_id, attempt),
            &declared_leaves(&packs.packs_out),
        ),
    };
    let crate::outputs::Collected {
        outputs,
        spool,
        packs,
    } = collected;
    // **Registered with the hub, pack by pack** (E-17, ORCH-260). After the
    // ingest and therefore after the fsync, which is the whole of E-17: a
    // pack this frame names is on this runner's disk whole, so a later job
    // handed that key is handed something complete or nothing at all.
    //
    // The commit comes out of the job's own declaration, because a pack is
    // registered for a `(commit, key)` pair and only a declared command names
    // a commit (E-11, D106) — and `leave_packs` answers an empty list for
    // every job with no `leaves`, so a job with packs here has a declaration.
    //
    // A link that is gone is said out loud rather than passed over: the pack
    // is not lost, since the next connection announces the holding, but the
    // `(commit, key)` it was left under is **not** registered, so a later job
    // needing that key would wait for a pack this hub was never told about.
    for pack in &packs {
        let id = pack.held.manifest.id();
        let registered = match declaration.as_ref() {
            Some(declaration) => session.register_pack(&job_id, attempt, &declaration.commit, pack),
            None => false,
        };
        if registered {
            eprintln!(
                "herd-runner: job {job_id} left {:?} at {:?} as pack {id} ({} object(s), {} \
                 new); it is in this runner's pack store and registered with the hub",
                pack.key, pack.at, pack.held.objects, pack.held.objects_added
            );
        } else {
            eprintln!(
                "herd-runner: job {job_id} left {:?} at {:?} as pack {id} ({} object(s), {} \
                 new); it is in this runner's pack store and is NOT registered with the hub, \
                 because there was no link to say so on. This runner holds it and announces \
                 that on its next connection, but nothing has told this hub which (commit, key) \
                 it answers, so a later job needing {:?} will wait for it",
                pack.key, pack.at, pack.held.objects, pack.held.objects_added, pack.key
            );
        }
    }
    // And the holdings this job just changed, said again to the hub (D231
    // decision 5): an announcement replaces the hub's whole view of this
    // runner, so a pack that has just entered the store is only known once
    // the whole list is said again. Only when a pack really landed —
    // `leave_packs` answers an empty list for every job that declared no
    // `leaves` and for every one whose declared artefact was refused, and a
    // job that changed nothing has nothing to re-announce.
    //
    // **The other production path that changes what a runner holds is the
    // layout above**, which ingests whatever it fetched and announces for
    // itself (`Session::lay_out_packs`). Two announcement sites and not one,
    // because the two happen at opposite ends of a job's life and an
    // announcement replaces the hub's whole view: a single site would leave
    // the hub stale in one of the two directions for the length of a job.
    if !packs.is_empty() {
        eprintln!(
            "herd-runner: job {job_id} changed what this runner holds. {}",
            session.announce_packs_held(confined.root().path())
        );
    }
    if let Some(refusal) = &outputs.refused {
        eprintln!("herd-runner: job {job_id} {refusal}");
    }
    for said in &outputs.skipped {
        eprintln!("herd-runner: job {job_id} output: {said}");
    }

    // The cgroup goes away with its guard; the workspace is reclaimed here.
    // A workspace that will not go away is a disk filling up, which is
    // exactly what a runner must not do to its host — so it is said in the
    // job's own record and not only in the runner's log.
    drop(cgroup);
    reclaim_or_say(confined.root(), &workspace, &job_id, attempt, &session);

    // What the hub is told, and the one place the three ways of ending part.
    //
    // Held until the hub acknowledges it, and sent again on every connection
    // until it does: a result is the one thing a runner cannot reproduce
    // (E-44, D17, D19).
    //
    // Before the job leaves the live list, on purpose. A runner that is
    // stopping waits for that list to empty and for nothing to be held
    // (see [`crate::run`]'s drain), so leaving the list first would open an
    // instant in which the job is neither running nor held — and a stop that
    // looked in that instant would find nothing to write down and take the
    // verdict with it. CI found exactly that instant.
    match ran.ending.clone() {
        // An abandoned attempt has no outcome, by definition: the runner is
        // not telling the hub how the job went, it is telling it that this go
        // did not happen. The hub decides whether there is another (D39).
        Some(Ending::Abandoned(reason)) => session.abandon(&job_id, attempt, &reason),
        Some(Ending::Cancelled(reason)) => {
            // Unless a cap had already ended it, in which case the cap is
            // what the job ended of and 124 is the honest code (D31).
            let outcome = match outcome {
                JobOutcome::StoppedByCap { .. } => outcome,
                _ => JobOutcome::Cancelled { reason },
            };
            session.finish(
                &job_id, attempt, outcome, metrics, ran.steps, outputs, spool,
            );
        }
        None => session.finish(
            &job_id, attempt, outcome, metrics, ran.steps, outputs, spool,
        ),
    }
    if let Some(token) = live {
        confined.live().remove(token);
    }
}

/// When one step started and how long it took, as the record carries it
/// (D77).
struct StepAt {
    started_unix_ms: i64,
    took: Duration,
}

/// The baseline one step's out-of-memory verdict is read against, and the one
/// place it moves.
///
/// There is **one cgroup per job, not per step** — created once in
/// [`execute`], with `memory.max`, `memory.swap.max` and `memory.oom.group`
/// written once, and only `cgroup.procs` reopened per step — so the kernel's
/// kill counters are job-scoped by construction and every step-level reading
/// of them has to carry its own baseline. This is that baseline, and the two
/// things it has to get right are both facts about *when* a counter settles,
/// not about the counter's value:
///
/// * **A group kill is counted once, before the first death.** Which is why
///   `oom_group_kill` is what a step is judged on wherever the kernel
///   publishes it. `oom_kill` counts a task at a time while the group is
///   being walked, and the runner is woken by the *first* of those deaths, so
///   the reading it takes can be mid-burst and the rest of the **same** kill
///   lands in the next step's delta. One job's record showed `run` stopped at
///   the memory cap and `down` stopped at the memory cap, 5 s later, with one
///   OOM event in the kernel log and nothing at all during `down`
///   ([`crate::cgroup::OomKills`] has the mechanism).
/// * **A step that exited 0 was not stopped by anything.** With
///   `memory.oom.group = 1` the whole job tree dies together, so a shell that
///   exited cleanly cannot be a shell the kernel killed, whatever a counter
///   read after it says. A non-zero code keeps its cap: a job whose own
///   process was killed while its shell reported 137 *was* stopped by the
///   ceiling, and that is the case this must not swallow.
///
/// The watermark never moves backwards, because an unreadable `memory.events`
/// reads as zero ([`JobCgroup::oom_kills`]) and a baseline one unreadable
/// step had lowered would charge the next step with every kill the job ever
/// had.
#[derive(Debug, Default, Clone, Copy)]
struct OomWatermark(OomKills);

impl OomWatermark {
    /// Did an out-of-memory kill happen **in this step**? Advances the
    /// baseline either way, because the counters are cumulative.
    fn killed_in_this_step(&mut self, now: OomKills, outcome: &JobOutcome) -> bool {
        let before = self.0;
        self.0 = OomKills {
            processes: before.processes.max(now.processes),
            group_kills: match (before.group_kills, now.group_kills) {
                (Some(was), Some(is)) => Some(was.max(is)),
                (was, is) => was.or(is),
            },
        };
        if matches!(outcome, JobOutcome::Exited { code: 0 }) {
            return false;
        }
        match (before.group_kills, now.group_kills) {
            (Some(was), Some(is)) => is > was,
            // No `oom_group_kill` on this kernel: the per-process counter is
            // all there is, and the clause above is what keeps a step that
            // came off from wearing somebody else's kill.
            _ => now.processes > before.processes,
        }
    }
}

/// A cap that ended the attempt, with everything the record says about it.
#[derive(Debug, Clone)]
struct Capped {
    cap: Cap,
    limit: u64,
    detail: Option<String>,
}

/// What an attempt has come to, filled in as its steps end.
#[derive(Debug, Default)]
struct Attempt {
    /// Every step that ran, in the order it ran. Empty for an ad-hoc job,
    /// which runs no executor step at all (D77).
    steps: Vec<StepRun>,
    /// What the `run` step came to, or the one process of an ad-hoc job.
    verdict: Option<JobOutcome>,
    /// Why the job never started, when a preparation step says so (D108).
    could_not_start: Option<String>,
    /// A cap that ended the attempt.
    cap: Option<Capped>,
    /// Why the machine this attempt ran on could not do its own part, when
    /// that is what ended it (D166).
    ///
    /// One case today, and it is the one the first consumer hit three times:
    /// the walk that holds a job to its disk budget was **refused** a path
    /// inside the job's own workspace. The budget cannot be enforced, so the
    /// job still ends — that half is invariant 2 and was always right — but
    /// it is not a cap and must not wear one's exit code.
    setup_failed: Option<String>,
    /// How somebody outside the job asked for it to end, if somebody did.
    ending: Option<Ending>,
    /// The high-water mark of the workspace across every step, or `None` if
    /// no measurement could be taken at all.
    peak_disk: Option<u64>,
    /// Why a measurement could not be taken, if one could not — and whether
    /// it was an access the runner was refused, which is what decides between
    /// D30's cap and D166's setup failure.
    unmeasurable: Option<Unmeasurable>,
    peak_rss: Option<u64>,
    /// Where this job's `fetch` hook may have written the revision it really
    /// reached, for a declared command (D176). `None` for an ad-hoc job,
    /// which has no executor step and no `{ran}`.
    ran_path: Option<PathBuf>,
}

impl Attempt {
    /// Whether the steps before `down` can still run.
    fn over(&self) -> bool {
        self.could_not_start.is_some()
            || self.cap.is_some()
            || self.setup_failed.is_some()
            || self.ending.is_some()
    }

    /// Whether anything has ended this attempt other than its own steps
    /// running to their end.
    fn nothing_ended_it(&self) -> bool {
        !self.over()
    }

    /// The one outcome the hub is told.
    ///
    /// A cap first, because a breached cap is the end of the job whatever the
    /// process did afterwards (D24, D25, D31). Then a preparation step that
    /// did not come off, which is the job never having started rather than a
    /// verdict (D108, D20). Then what `run` said, worn untouched — 0, 1 and
    /// 75 alike (E-02). A `down` that failed is on its own step line and
    /// changes none of this: the verdict already exists, and a workspace that
    /// would not be taken down is an operator's problem, not a test result.
    fn outcome(&self) -> JobOutcome {
        if let Some(capped) = &self.cap {
            return JobOutcome::StoppedByCap {
                cap: capped.cap,
                limit: capped.limit,
                detail: capped.detail.clone(),
            };
        }
        // After the cap rather than before it, and the two can never both be
        // set anyway: a setup failure is recorded exactly where a cap would
        // have been and in its place (D166). The order here is for whoever
        // reads this function — a cap is still the first thing that ends a
        // job — and not a precedence anything depends on.
        if let Some(reason) = &self.setup_failed {
            return JobOutcome::SetupFailed {
                reason: reason.clone(),
            };
        }
        if let Some(reason) = &self.could_not_start {
            return JobOutcome::CouldNotStart {
                reason: reason.clone(),
            };
        }
        self.verdict.clone().unwrap_or_else(|| {
            // No step produced a verdict and nothing ended the attempt: the
            // `run` step was never reached and nobody said why, which is a
            // bug in this file rather than a job that failed. Said as what it
            // is instead of printed as an exit code.
            JobOutcome::CouldNotStart {
                reason: "this runner ran the job's steps and none of them produced a verdict"
                    .to_owned(),
            }
        })
    }

    /// Record a workspace the runner was **refused** during one step, and
    /// answer whether that step was the takedown (D166).
    ///
    /// The distinction is D108's and it is not a detail. A refusal seen while
    /// the job's own steps are running ends the attempt: the disk budget
    /// could not be enforced, so there is no verdict to stand behind. One
    /// seen while the workspace is being *taken down* is a different fact
    /// entirely — the verdict already exists, and a tree that will not go
    /// away is an operator's problem and never a test result — so it is said
    /// in the job's log, where `down` failures are said, and the verdict is
    /// left exactly as it was.
    ///
    /// The first refusal is kept rather than the last: the earliest one is
    /// the one that stopped the job, and the steps after it only see the same
    /// path still unreadable.
    fn refused_workspace(&mut self, step: Option<Step>, reason: String) -> bool {
        if step == Some(Step::Down) {
            return true;
        }
        let _ = self.setup_failed.get_or_insert(reason);
        false
    }

    /// End an attempt whose workspace could not be measured, as the one thing
    /// it is (D30, D166).
    ///
    /// Two endings out of one mechanism, because the mechanism has two
    /// causes. A walk that was **refused** a path is a fact about how the
    /// machine is set up: no submission could have caused it, nothing a
    /// requester changes will fix it, and 124 would tell them a budget killed
    /// their build. Anything else — a path that is not a directory, a
    /// measuring task that died — is the runner failing at its own work, and
    /// D30's reading still holds: the job is stopped by the cap it could not
    /// be held to, with the real reason beside the figure so that four
    /// kilobytes never read as a filled disk.
    fn ended_unmeasurable(&mut self, unmeasurable: &Unmeasurable, budget: u64) {
        if unmeasurable.refused {
            self.setup_failed = Some(herd_proto::workspace_refused_reason(
                budget,
                &unmeasurable.why,
            ));
        } else {
            self.cap = Some(Capped {
                cap: Cap::Disk,
                limit: budget,
                detail: Some(format!(
                    "this runner could not measure the job's workspace, so it could not hold \
                     the job to its disk budget at all: {}",
                    unmeasurable.why
                )),
            });
        }
    }

    /// How a step went, in the words its own line uses.
    fn how_it_went(exit_code: Option<i32>, ended: &Option<String>) -> String {
        match (exit_code, ended) {
            (Some(code), _) => format!("exited {code}"),
            (None, Some(said)) => said.clone(),
            // Neither: said as what it is rather than read as a success.
            (None, None) => "ended with nothing said about how".to_owned(),
        }
    }

    /// Record a step whose shell never ran at all.
    #[allow(clippy::too_many_arguments)]
    fn step_never_started(
        &mut self,
        stage: &Stage,
        at: StepAt,
        reason: String,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
    ) {
        let how = format!("could not be started: {reason}");
        self.record(
            stage,
            at,
            None,
            Some(format!("this step {how}")),
            session,
            job_id,
            attempt,
        );
        match stage.step {
            // An ad-hoc job's one process, or the declared line itself: Herd
            // failed to run what it was given, which is not the job failing
            // (D20).
            None | Some(Step::Run) => self.verdict = Some(JobOutcome::CouldNotStart { reason }),
            Some(Step::Down) => {
                self.say_the_takedown_did_not_come_off(session, job_id, attempt, &how)
            }
            Some(step) => self.say_the_job_never_started(session, job_id, attempt, step, &how),
        }
    }

    /// Record a step that ran, and what its ending means for the attempt.
    #[allow(clippy::too_many_arguments)]
    fn step_ended(
        &mut self,
        stage: &Stage,
        at: StepAt,
        watched: Watched,
        oomed: bool,
        caps: &crate::confine::Confinement,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
    ) {
        if let Some(used) = watched.peak_disk {
            self.peak_disk = Some(self.peak_disk.unwrap_or(0).max(used));
        }
        if let Some(rss) = watched.peak_rss {
            self.peak_rss = Some(self.peak_rss.unwrap_or(0).max(rss));
        }
        if let Some(reason) = watched.unmeasurable {
            self.unmeasurable = Some(reason);
        }

        // Which cap stopped this step, if one did. The kernel's own kill is
        // read where the watchdog saw nothing: an out-of-memory kill reaches
        // us as a signal, and a bare "signalled by 9" would tell nobody why
        // (E-30).
        // A disk cap the runner was *refused* its way into is not a cap at
        // all (D166). Judged before `capped` is built, so that neither this
        // step's line nor the attempt's ending can go on calling it one.
        let refused_workspace = match (watched.cap, &self.unmeasurable) {
            (Some(Cap::Disk), Some(unmeasurable)) if unmeasurable.refused => Some(
                herd_proto::workspace_refused_reason(caps.disk_budget, &unmeasurable.why),
            ),
            _ => None,
        };

        let capped = match (watched.cap, oomed) {
            // Said as the setup failure it is, below. Nothing is recorded as
            // a cap here, which is the whole of D166's change.
            _ if refused_workspace.is_some() => None,
            (Some(cap), _) => Some(Capped {
                cap,
                limit: limit_of(cap, stage, caps),
                // The *reason* travels with the cap, not only the cap's
                // figure. A job that wrote four kilobytes and could not be
                // measured was being told "stopped at its disk cap of
                // 21474836480 bytes", which is a true number in a false
                // sentence (D30).
                detail: match (cap, &self.unmeasurable) {
                    (Cap::Disk, Some(unmeasurable)) => Some(format!(
                        "this runner could not measure the job's workspace, so it could not \
                         hold the job to its disk budget at all: {}",
                        unmeasurable.why
                    )),
                    _ => None,
                },
            }),
            (None, true) => Some(Capped {
                cap: Cap::Memory,
                limit: caps.memory_max,
                detail: None,
            }),
            (None, false) => None,
        };

        // What the step's own line says: a code when a shell exited, and why
        // there is none when nothing did. Never a zero for a code that was
        // not read (D23's rule, as `StepRun` states it).
        let (exit_code, ended) = if let Some(said) = &refused_workspace {
            (None, Some(said.clone()))
        } else {
            match (&capped, &watched.ending, &watched.outcome) {
                // The time cap is the one of the four whose figure is the step's
                // own rather than the runner's, so its sentence says whose it is
                // (D108).
                (Some(capped), _, _) if capped.cap == Cap::Time => {
                    (None, Some(step::stopped_at_the_time_cap(stage)))
                }
                (Some(capped), _, _) => (
                    None,
                    Some(herd_proto::cap_stop_reason(
                        capped.cap,
                        capped.limit,
                        capped.detail.as_deref(),
                    )),
                ),
                (None, Some(asked), _) => (
                    None,
                    Some(format!("was ended before it finished: {}", asked.reason())),
                ),
                (None, None, JobOutcome::Exited { code }) => (Some(*code), None),
                (None, None, JobOutcome::Signalled { signal }) => {
                    (None, Some(format!("was killed by signal {signal}")))
                }
                (None, None, JobOutcome::CouldNotStart { reason }) => (None, Some(reason.clone())),
                (None, None, other) => (None, Some(format!("ended as {other:?}"))),
            }
        };
        // What a later line says this step did. A cap says it as the step's
        // own, never in the job's sentence: "the down step the runner stopped
        // this job at its memory cap of …" is two claims in one clause.
        let how = match &capped {
            Some(capped) => how_a_cap_stopped_this_step(stage, capped),
            None => Self::how_it_went(exit_code, &ended),
        };
        self.record(stage, at, exit_code, ended, session, job_id, attempt);

        // The step that decided the verdict says so in the job's **own** log,
        // naming itself and the figure, because that log is what a reader
        // reads first and until now the cap never reached it from here.
        if let Some(capped) = &capped {
            say_in_the_log(
                session,
                job_id,
                attempt,
                &the_cap_that_stopped_this_step(stage, capped),
            );
        }

        // Ahead of everything else read off this step: a workspace the runner
        // was refused is the machine's problem at `up` exactly as it is at
        // `run`, and letting a preparation step turn it into
        // `could_not_start` would hide both the machine's part in it and the
        // path somebody has to go and fix (D166).
        if let Some(reason) = refused_workspace {
            if self.refused_workspace(stage.step, reason) {
                self.say_the_takedown_did_not_come_off(session, job_id, attempt, &how);
            }
            return;
        }

        let came_off = exit_code == Some(0);
        match stage.step {
            // An ad-hoc job's one process, and the declared line itself: its
            // code **is** the verdict, worn untouched — 0, 1 and 75 alike
            // (E-02) — unless a cap or somebody outside the job ended it,
            // which are the two things that are not a verdict.
            None | Some(Step::Run) => match (capped, watched.ending) {
                (Some(capped), _) => self.cap = Some(capped),
                (None, Some(asked)) => self.ending = Some(asked),
                (None, None) => self.verdict = Some(watched.outcome),
            },
            // The takedown happens after the verdict exists and cannot
            // change it, however it ends (D108).
            Some(Step::Down) => {
                if !came_off {
                    self.say_the_takedown_did_not_come_off(session, job_id, attempt, &how);
                }
            }
            // `up` and `fetch`: preparation. A step that did not come off
            // means the job never started, and never an exit code a caller
            // could read as a failing test (D108, D20).
            Some(step) => match watched.ending {
                // Somebody ended the job while it was still preparing. That
                // is the job's ending and not a job that could not start:
                // nobody is asking what the preparation came to.
                Some(asked) => self.ending = Some(asked),
                None => {
                    if !came_off {
                        self.say_the_job_never_started(session, job_id, attempt, step, &how);
                    }
                }
            },
        }
    }

    /// The job never started, as its record and its log both say it.
    fn say_the_job_never_started(
        &mut self,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
        step: Step,
        how: &str,
    ) {
        self.could_not_start = Some(format!("the job never started: its {step} step {how}"));
        say_in_the_log(
            session,
            job_id,
            attempt,
            &format!(
                "the {step} step {how}, so this job never started and the declared command \
                 was not run"
            ),
        );
    }

    /// A takedown that did not come off: said in the job's log, because bytes
    /// left on a host nobody will collect are what invariant 2 forbids above
    /// all — and the verdict is left exactly as it was.
    fn say_the_takedown_did_not_come_off(
        &self,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
        how: &str,
    ) {
        say_in_the_log(
            session,
            job_id,
            attempt,
            &format!(
                "the down step {how}, so this job's workspace may be left on this host. The \
                 job's verdict is unchanged."
            ),
        );
    }

    /// Put one step's line on the attempt and send it at once (D77).
    ///
    /// Both, deliberately: the frame fills the record while the attempt is
    /// still going, so a 5 h nightly stopped at its cap still shows the
    /// preparation it did, and the complete list goes again with the verdict
    /// and takes over. A step of an ad-hoc job does not exist, so nothing is
    /// recorded for it.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        stage: &Stage,
        at: StepAt,
        exit_code: Option<i32>,
        ended: Option<String>,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
    ) {
        let Some(step) = stage.step else {
            return;
        };
        let run = StepRun {
            step,
            started_unix_ms: at.started_unix_ms,
            duration_ms: u64::try_from(at.took.as_millis()).unwrap_or(u64::MAX),
            exit_code,
            ended,
            // Only `fetch`, and only once it has ended: it is the step that
            // puts the tree at the commit, so it is the step whose answer
            // this is. A revision read after `run` would be whatever the
            // job's own command left behind (D176).
            ran_commit: match step {
                Step::Fetch => self.the_revision_it_reached(session, job_id, attempt),
                _ => None,
            },
        };
        self.steps.push(run.clone());
        session.send(ToHub::StepFinished {
            job_id: job_id.clone(),
            attempt,
            step: run,
        });
    }

    /// What this job's `fetch` hook said the tree really reached, if it said
    /// anything (D176).
    ///
    /// Three answers and one of them is absence, which is the one every
    /// consumer gives until it adds `{ran}` to its own `fetch` line. A file
    /// that is there and cannot be used is **also** absence on the record —
    /// the runner has no revision to put there — and the job's log says why,
    /// so a hook writing the wrong thing does not look like a hook writing
    /// nothing.
    fn the_revision_it_reached(
        &self,
        session: &Session,
        job_id: &JobId,
        attempt: u32,
    ) -> Option<String> {
        let path = self.ran_path.as_ref()?;
        match step::ran_commit(path) {
            step::RanCommit::Said(revision) => Some(revision),
            step::RanCommit::Nothing => None,
            step::RanCommit::Unusable(why) => {
                say_in_the_log(
                    session,
                    job_id,
                    attempt,
                    &format!(
                        "the fetch step left a file at {} and {why}, so this attempt reports \
                         no revision and nothing here confirms which commit was built",
                        path.display()
                    ),
                );
                None
            }
        }
    }
}

/// Why a step's spawn failed, naming whichever missing thing is the real one
/// (D151).
///
/// `ENOENT` out of a spawn is ambiguous in a way that matters here. The child
/// `chdir`s into its working directory and *then* `exec`s the program, and a
/// failure at either point arrives as the same `io::Error`: `NotFound`, `os
/// error 2`, and the same `Display` text down to the byte. The error carries
/// neither path, so nothing in it says which of the two was missing — the
/// sentence has to work that out by looking.
///
/// Blaming the program unconditionally is wrong exactly where it costs most.
/// A declared command's `down` step whose own earlier `--purge` removed the
/// workspace fails because the ground under it is gone, and a reader told
/// `/bin/sh: No such file or directory` goes hunting for a missing shell on a
/// host where `/bin/sh` is perfectly fine. So the directory is checked, and
/// when it is the thing that is gone it is what gets named — the consumer can
/// then see that their own takedown removed it.
fn why_a_step_could_not_start(
    program: &str,
    work_dir: &std::path::Path,
    error: &std::io::Error,
) -> String {
    // Only `NotFound` is ambiguous: every other errno came from the exec or
    // from `pre_exec`, and the program is the thing to name.
    if error.kind() == std::io::ErrorKind::NotFound && !work_dir.exists() {
        return format!(
            "the directory it would have run in is gone: {} — a step that removes the \
             workspace leaves the next one nothing to run in",
            work_dir.display()
        );
    }
    format!("{program}: {error}")
}

/// What this job declared it **leaves**: a key, and the path under `{out}`
/// the artefact is left at (`leaves = { pack = "packs" }`, M11, E-17).
///
/// **It comes off the assignment and not out of the declaration**, which is
/// the one thing about this seam worth stating: `FromHub::AssignJob` carries
/// `packs_out` as the hub resolved it (ORCH-234), and the hub resolved it out
/// of the job's own record. A runner that re-read the declaration would be a
/// second reader of the requester's file and could disagree with the hub
/// about what a job leaves.
///
/// Empty for an ad-hoc job, which has no `{out}` at all, and empty for every
/// declared command that leaves no pack — which is every job the first
/// consumer submits today.
fn declared_leaves(packs_out: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    packs_out.clone()
}

/// Now, in milliseconds since the Unix epoch, on this runner's clock (D77).
fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_millis()).ok())
        // A clock before the epoch is a host whose clock is wrong, which is
        // not something to crash a job over: the record says the attempt's
        // runner, and a reader can see the figure is nonsense.
        .unwrap_or(0)
}

/// One of Herd's own sentences in the job's log, which is part of its record.
///
/// The runner's own stderr is not somewhere a requester can read, and these
/// are the lines that explain a verdict a step produced. Numbered from the
/// job's own counter like any other chunk — a number of its own invention
/// would read, at the hub, as every chunk in between having been lost (D29).
fn say_in_the_log(session: &Session, job_id: &JobId, attempt: u32, said: &str) {
    session.send_chunk(
        job_id,
        attempt,
        LogStream::Stderr,
        format!("herd-runner: {said}\n").into_bytes(),
    );
}

/// Kill the job's cgroup, every step's process group, and anything still
/// carrying its marker. Idempotent on purpose: it runs when a cap trips, when
/// a requester cancels, and once more when the job's processes are gone.
fn kill_everything_of(cgroup: Option<&JobCgroup>, pids: &[u32], tag: &JobTag, job_id: &JobId) {
    match cgroup {
        // One write takes the whole tree, whatever it did to detach itself.
        Some(cgroup) => {
            let _ = cgroup.kill();
            for pid in pids {
                kill_process_group(*pid);
            }
        }
        // No cgroup, so no handle on the tree: every step's process group —
        // each step gets a session of its own — and then every process still
        // carrying this job's marker, which is how a `setsid` child, a double
        // fork and a `nohup` are reached without one.
        None => {
            for pid in pids {
                kill_process_group(*pid);
            }
            let swept = kill_by_marker(tag);
            if swept > 0 {
                eprintln!(
                    "herd-runner: job {job_id} left {swept} process(es) outside its \
                     process group; they were killed by marker"
                );
            }
        }
    }
}

/// Reclaim a job's workspace, saying so in the job's record if it will not go.
///
/// The runner owns every inode under there, so this failing means something
/// stranger than a mode — and whatever it is, the requester reading the job's
/// log must be able to see that this host now carries bytes nobody collected.
///
/// **Three readers, and the third is the one who can act.** The runner's
/// stderr is for whoever is watching this process; the job's log is for the
/// requester, who can see it but cannot reach the disk; and the notebook
/// ([`crate::janitor`]) is for the person on this host, who is the only one
/// of the three who can remove what is still there. A job whose *setup*
/// failed on a workspace it was refused (D166) comes through here like any
/// other, which is why that case leaves an entry without a path of its own.
fn reclaim_or_say(
    root: &DataRoot,
    workspace: &Workspace,
    job_id: &JobId,
    attempt: u32,
    session: &Session,
) {
    let Err(error) = workspace.reclaim() else {
        return;
    };
    let said = format!(
        "herd-runner: the workspace {} of job {job_id} could not be reclaimed and is \
         left on this host: {error}\n",
        workspace.base_dir().display()
    );
    eprintln!("{}", said.trim_end());
    // Durably, where a restart cannot take it away and where it is named with
    // its figure. Nothing is returned and nothing can fail: a notebook that
    // cannot be written costs this job nothing (D134).
    crate::janitor::write_down(
        root,
        &crate::janitor::Leftover::workspace_of(workspace.base_dir(), job_id, attempt, &error),
    );
    // Into the job's log, which is part of its record: the runner's own
    // stderr is not somewhere a requester can read. Numbered from the job's
    // own counter like any other chunk — a number of its own invention would
    // read, at the hub, as every chunk in between having been lost (D29).
    session.send_chunk(job_id, attempt, LogStream::Stderr, said.into_bytes());
}

/// What the watchdog needs to know about the step it is watching.
struct Watch<'a> {
    pid: u32,
    /// When this step is past its time cap. Per step, not per job: a cold
    /// cache pull in `up` must not eat a build's declared minutes (D108).
    deadline: Instant,
    disk_budget: u64,
    workspace: Workspace,
    cgroup: Option<&'a JobCgroup>,
    tripped: Arc<AtomicU8>,
    tag: JobTag,
    job_id: JobId,
    /// How the job is ended from outside it: a cancel, a drain, a stale
    /// attempt. One token, so there is one kill. `None` for a step that is
    /// already running *because* of such an ending — the takedown after a
    /// cancel — where nothing more can be asked.
    cancel: Option<&'a CancelToken>,
}

/// What the watchdog saw of one step.
struct Watched {
    outcome: JobOutcome,
    cap: Option<Cap>,
    /// How somebody outside the job asked for it to end, if somebody did.
    ending: Option<Ending>,
    /// The high-water mark of the workspace, or `None` if no measurement
    /// could be taken at all.
    peak_disk: Option<u64>,
    /// Why a measurement could not be taken, if one could not, and whether it
    /// was an access the runner was refused (D166).
    unmeasurable: Option<Unmeasurable>,
    peak_rss: Option<u64>,
}

/// Wait for one step, enforcing the caps that need watching and obeying
/// whoever asks for the job to end.
///
/// Every ending kills the same way: the job's whole cgroup and process group
/// and marker, and then this goes on waiting for the process so that it is
/// reaped rather than left as a zombie.
///
/// The process ending does **not** end this function's duty: the cap is
/// examined one last time, because a step can breach a cap and exit in the
/// same breath, and the breach is what the step ended of.
async fn watch(child: &mut Child, watch: Watch<'_>) -> Watched {
    let mut cap: Option<Cap> = None;
    let mut ending: Option<Ending> = None;
    let mut peak_disk: Option<u64> = None;
    let mut unmeasurable: Option<Unmeasurable> = None;
    let mut peak_rss: Option<u64> = None;

    let outcome = loop {
        // `Child::wait` is cancel-safe, so losing this race costs nothing,
        // and so is a fresh `ended()`: it reads a watch channel.
        tokio::select! {
            status = child.wait() => {
                let ended = match status {
                    Ok(status) => outcome_of(status),
                    // The process started but Herd never learned how it
                    // ended, so it has no code and no signal to report.
                    Err(error) => JobOutcome::CouldNotStart {
                        reason: format!("waiting for the job's process failed: {error}"),
                    },
                };
                // The last word on the caps, before the child's status is
                // allowed to be the answer. The log cap above all: the pump
                // trips it and the process may well exit 0 right after.
                if cap.is_none() && watch.tripped.load(Ordering::SeqCst) == LOG_CAP {
                    cap = Some(Cap::Log);
                }
                break ended;
            }
            asked = async {
                match watch.cancel {
                    Some(cancel) => cancel.ended().await,
                    None => std::future::pending().await,
                }
            }, if ending.is_none() => {
                // A requester's cancel, a drain, or the hub writing this
                // attempt off. The same kill as a cap, so that nothing the
                // job started survives being ended either.
                println!(
                    "herd-runner: ending job {} because {}",
                    watch.job_id,
                    asked.reason()
                );
                ending = Some(asked);
                kill_everything_of(watch.cgroup, &[watch.pid], &watch.tag, &watch.job_id);
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }

        // A job somebody has ended is over: it is not held to its caps any
        // more, and the only thing left to do is reap it.
        if cap.is_none() && ending.is_none() {
            // The disk walk is filesystem work, so it happens off the
            // runtime: a job with a large workspace must not stall the
            // runner's connection to the hub.
            let workspace = watch.workspace.clone();
            let measured = tokio::task::spawn_blocking(move || workspace.measure())
                .await
                .unwrap_or_else(|error| {
                    // The runner failing at its own work, not a path it was
                    // refused: D30's cap, not D166's setup failure.
                    Err(crate::root::Unmeasurable {
                        why: format!("the measuring task failed: {error}"),
                        refused: false,
                    })
                });
            match &measured {
                Ok(used) => peak_disk = Some(peak_disk.unwrap_or(0).max(*used)),
                Err(unreadable) => unmeasurable = Some(unreadable.clone()),
            }
            if watch.cgroup.is_none() {
                if let Some(rss) = process_rss_bytes(watch.pid) {
                    peak_rss = Some(peak_rss.unwrap_or(0).max(rss));
                }
            }

            cap = if watch.tripped.load(Ordering::SeqCst) == LOG_CAP {
                Some(Cap::Log)
            } else if measured.is_err() || measured.is_ok_and(|used| used > watch.disk_budget) {
                // Either the job is past its budget, or the runner cannot
                // tell any more — and a budget that cannot be measured
                // cannot be enforced, which ends the job by name rather than
                // letting it run on unbounded.
                Some(Cap::Disk)
            } else if Instant::now() >= watch.deadline {
                Some(Cap::Time)
            } else {
                None
            };
            if cap.is_some() {
                kill_everything_of(watch.cgroup, &[watch.pid], &watch.tag, &watch.job_id);
            }
        }
    };

    Watched {
        outcome,
        cap,
        ending,
        peak_disk,
        unmeasurable,
        peak_rss,
    }
}

/// The figure that goes with a cap, in the cap's own unit.
///
/// The time cap is the one that is per step: it is the smaller of what the
/// command declared and what this runner's owner lends, which is the step's
/// own figure and not the runner's (D108).
fn limit_of(cap: Cap, stage: &Stage, caps: &crate::confine::Confinement) -> u64 {
    match cap {
        Cap::Time => stage.timeout.as_secs(),
        Cap::Memory => caps.memory_max,
        Cap::Log => caps.log_cap,
        Cap::Disk => caps.disk_budget,
    }
}

/// What a cap did to **this step**, in the step's own grammar.
///
/// [`herd_proto::cap_exceeded_reason`] is the job's sentence — *"the runner
/// stopped this job at its memory cap of N bytes"* — and it is the right one
/// for the attempt's outcome, which is about the job. Pasted behind a step's
/// name it reads *"the down step the runner stopped this job at…"*, which
/// says two different things at once and is what left a reader unable to tell
/// which step a kill happened in. A step's own line says what happened to the
/// step and nothing about the job.
fn how_a_cap_stopped_this_step(stage: &Stage, capped: &Capped) -> String {
    match capped.cap {
        // The one of the four whose figure is the step's own rather than the
        // runner's, so its sentence says whose it is (D108).
        Cap::Time => format!(
            "was stopped at the time cap of {} s, {}",
            capped.limit, stage.whose_timeout
        ),
        cap => format!(
            "was stopped at this runner's own {} cap of {} {}",
            cap.name(),
            capped.limit,
            cap.unit()
        ),
    }
}

/// The line a capped step writes into the **job's own log**, naming itself.
///
/// Until this existed the step that decided the verdict put its cap sentence
/// in the record's `StepRun.ended` and in the attempt's outcome and nowhere
/// else, so a reader of the log met the memory cap exactly once — attached to
/// `down`, by way of the takedown line — and could not tell which step the
/// kill happened in without fetching the record.
///
/// The figure is named for what it is. The three attempt-wide caps are this
/// **runner's** figures, built once at startup: `memory.max` is written from
/// one runner-wide `--job-memory-max` and never from the job's declaration,
/// so a job that declared 1792 MiB and was stopped at 8589934592 bytes was
/// not stopped at anything it asked for. Said plainly here, because it has
/// already misread twice.
fn the_cap_that_stopped_this_step(stage: &Stage, capped: &Capped) -> String {
    let what = match stage.step {
        Some(step) => format!("the {step} step"),
        // An ad-hoc job has one process and no executor step at all (D77).
        None => "this job's command".to_owned(),
    };
    let how = how_a_cap_stopped_this_step(stage, capped);
    match capped.cap {
        Cap::Time => format!("{what} {how}"),
        _ => format!(
            "{what} {how} — this runner's own ceiling, the figure it holds every job it takes \
             to, and not a figure this job declared"
        ),
    }
}

/// How a finished process ended, in the protocol's terms. A unix status is an
/// exit code or a signal, never both.
#[cfg(unix)]
fn outcome_of(status: std::process::ExitStatus) -> JobOutcome {
    use std::os::unix::process::ExitStatusExt;

    match status.code() {
        Some(code) => JobOutcome::Exited { code },
        // Killed, and `signal` is then set. Signal 0 is one no platform ever
        // delivers, so it reads as "killed, by something unnamed".
        None => JobOutcome::Signalled {
            signal: status.signal().unwrap_or(0),
        },
    }
}

/// Elsewhere a process only ever has an exit code.
#[cfg(not(unix))]
fn outcome_of(status: std::process::ExitStatus) -> JobOutcome {
    JobOutcome::Exited {
        code: status.code().unwrap_or(0),
    }
}

/// Report a job that never ran. No `JobStarted`, because nothing started, and
/// no log chunk: the job wrote nothing, and Herd's own failure belongs in the
/// outcome rather than in the job's output.
///
/// And no output file either: every caller here is a job whose `{out}` was
/// never made, or was made and could not be run in. There is nothing under it
/// to collect, and saying so with the default is the same statement.
fn report_could_not_start(job_id: &JobId, attempt: u32, session: &Session, reason: String) {
    session.finish(
        job_id,
        attempt,
        JobOutcome::CouldNotStart { reason },
        JobMetrics::default(),
        Vec::new(),
        herd_proto::JobOutputs::default(),
        None,
    );
}

/// Forward one pipe to the hub as it fills, never waiting for the process,
/// and stop at the log cap.
///
/// The cap is counted across both streams and across every step, because the
/// cap is about the log the hub stores and the requester reads, which is one
/// log.
// Eight: the pipe, which attempt's log it is, the cap shared across both
// pumps and the session to send on. Same reason as the three above.
#[allow(clippy::too_many_arguments)]
async fn stream_output<R>(
    mut pipe: R,
    stream: LogStream,
    job_id: JobId,
    attempt: u32,
    log_bytes: Arc<AtomicU64>,
    tripped: Arc<AtomicU8>,
    log_cap: u64,
    session: Session,
) where
    R: AsyncRead + AsyncReadExt + Unpin,
{
    let mut buffer = vec![0u8; READ_CHUNK];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let already = log_bytes.fetch_add(read as u64, Ordering::SeqCst);
                let room = log_cap.saturating_sub(already);
                if room == 0 {
                    tripped.store(LOG_CAP, Ordering::SeqCst);
                    break;
                }
                // The chunk that crosses the cap is cut at it, so the log the
                // hub keeps is exactly as long as the cap says.
                let keep = usize::try_from(room).unwrap_or(read).min(read);
                // The bytes as they were read: a job's output is not text, and
                // a chunk boundary may fall in the middle of a character
                // (D18). Nothing here rewrites what the process wrote.
                session.send_chunk(&job_id, attempt, stream, buffer[..keep].to_vec());
                if keep < read {
                    tripped.store(LOG_CAP, Ordering::SeqCst);
                    break;
                }
            }
            Err(error) => {
                session.send_chunk(
                    &job_id,
                    attempt,
                    LogStream::Stderr,
                    format!("herd-runner: reading the job's output failed: {error}\n").into_bytes(),
                );
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confine::Confinement;

    fn a_stage(timeout: Duration) -> Stage {
        Stage {
            step: Some(Step::Run),
            program: "/bin/sh".to_owned(),
            arguments: vec!["-c".to_owned(), "true".to_owned()],
            env: Vec::new(),
            timeout,
            whose_timeout: "this runner's own figure".to_owned(),
            announce: None,
        }
    }

    /// The mechanism behind D151's first defect, demonstrated rather than
    /// asserted from memory: a spawn whose working directory is gone and a
    /// spawn whose program is gone fail **identically**.
    ///
    /// Same `ErrorKind`, same errno, same `Display` text, and no path in
    /// either — so nothing in the error says which of the two was missing,
    /// and a sentence built by pasting the program's name onto it names the
    /// wrong thing exactly half the time.
    #[test]
    fn a_missing_working_directory_and_a_missing_program_fail_alike() {
        let gone = crate::testing::TempDir::new("spawn-into-nothing");
        let work_dir = gone.path().join("work");
        std::fs::create_dir_all(&work_dir).expect("the fixture makes it");
        std::fs::remove_dir_all(&work_dir).expect("and then takes it away");

        let shell = "/bin/sh";
        assert!(
            std::path::Path::new(shell).exists(),
            "this test is about a shell that is there"
        );
        let no_cwd = std::process::Command::new(shell)
            .arg("-c")
            .arg("true")
            .current_dir(&work_dir)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect_err("a child cannot chdir into a directory that is gone");
        let no_program = std::process::Command::new(gone.path().join("not-a-program"))
            .current_dir(gone.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect_err("a child cannot exec a program that is not there");

        assert_eq!(no_cwd.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(no_cwd.raw_os_error(), no_program.raw_os_error());
        assert_eq!(
            no_cwd.to_string(),
            no_program.to_string(),
            "the two errors are indistinguishable, which is the whole defect"
        );

        // So the sentence has to look. The directory is gone: that is what it
        // must name, and it must not send the reader after a shell that is
        // sitting right there.
        let said = why_a_step_could_not_start(shell, &work_dir, &no_cwd);
        assert!(
            said.contains(&work_dir.display().to_string()),
            "the directory that is missing must be named: {said}"
        );
        assert!(
            !said.contains(shell),
            "a shell that exists must not be blamed for it: {said}"
        );

        // And the other way round, the program is still the thing to name:
        // the directory is there, so nothing else can be at fault.
        let said = why_a_step_could_not_start(shell, gone.path(), &no_program);
        assert!(
            said.contains(shell),
            "with its directory in place, the step's program is what failed: {said}"
        );
    }

    #[test]
    fn every_cap_s_limit_is_reported_in_the_cap_s_own_unit() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.timeout = Duration::from_secs(42);
        caps.memory_max = 1 << 20;
        caps.log_cap = 2 << 20;
        caps.disk_budget = 3 << 20;
        let stage = a_stage(caps.timeout);
        assert_eq!(limit_of(Cap::Time, &stage, &caps), 42);
        assert_eq!(limit_of(Cap::Memory, &stage, &caps), 1 << 20);
        assert_eq!(limit_of(Cap::Log, &stage, &caps), 2 << 20);
        assert_eq!(limit_of(Cap::Disk, &stage, &caps), 3 << 20);
    }

    /// The figure a capped step reports is the step's own, not the runner's:
    /// a `run` step bounded by a smaller declared figure must not be recorded
    /// against the runner's larger one (D108).
    #[test]
    fn the_time_cap_reported_is_the_step_s_own_figure() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.timeout = Duration::from_secs(3600);
        let stage = a_stage(Duration::from_secs(45));
        assert_eq!(limit_of(Cap::Time, &stage, &caps), 45);
    }

    /// A cap outranks every other reading of an attempt, and a preparation
    /// step that did not come off outranks a verdict nothing produced
    /// (D24, D108).
    #[test]
    fn a_cap_outranks_a_verdict_and_could_not_start_outranks_nothing() {
        let mut ran = Attempt {
            verdict: Some(JobOutcome::Exited { code: 0 }),
            ..Attempt::default()
        };
        assert_eq!(ran.outcome(), JobOutcome::Exited { code: 0 });
        ran.could_not_start = Some("the job never started: its up step exited 3".to_owned());
        assert!(matches!(ran.outcome(), JobOutcome::CouldNotStart { .. }));
        ran.cap = Some(Capped {
            cap: Cap::Time,
            limit: 45,
            detail: None,
        });
        assert_eq!(
            ran.outcome(),
            JobOutcome::StoppedByCap {
                cap: Cap::Time,
                limit: 45,
                detail: None
            }
        );
    }

    /// The split D166 is about: a workspace the runner was **refused** is a
    /// setup failure naming the path, and anything else that stops a
    /// measurement is still D30's cap.
    ///
    /// One mechanism, two endings, and the difference is the whole of why the
    /// first consumer's j984, j989 and j1197 were wrong. All three ended
    /// `stopped_by_cap` with `cap: disk` and exit 124, with a detail saying
    /// plainly that the runner could not *measure* the workspace at all — a
    /// true sentence under a false outcome, and 124 told their requester a
    /// budget had killed the build. Nothing of the sort happened: the trees
    /// were deposited under another ownership and `chmod` on somebody else's
    /// inode is refused however often it is tried.
    #[test]
    fn a_workspace_the_runner_was_refused_is_a_setup_failure_and_not_a_cap() {
        let budget = 21_474_836_480;
        let path = "/srv/herd/jobs/j1197/work/out/packs/pack-f0.pack";

        let mut ran = Attempt {
            verdict: Some(JobOutcome::Exited { code: 0 }),
            ..Attempt::default()
        };
        ran.ended_unmeasurable(
            &Unmeasurable {
                why: format!(
                    "{path} cannot be read even after its mode was restored: \
                              Permission denied (os error 13)"
                ),
                refused: true,
            },
            budget,
        );
        assert!(
            ran.cap.is_none(),
            "no cap ended this job, and 124 would say one had"
        );
        assert!(!ran.nothing_ended_it(), "the job is over all the same");
        let JobOutcome::SetupFailed { reason } = ran.outcome() else {
            panic!(
                "a refused workspace is a setup failure: {:?}",
                ran.outcome()
            )
        };
        assert!(
            reason.contains(path),
            "the path somebody has to go and fix has to be in it: {reason}"
        );
        assert!(
            reason.contains(&budget.to_string()),
            "and the budget it could not be held to: {reason}"
        );

        // The other half of the mechanism is untouched. A measurement that
        // failed for any reason but access is the runner failing at its own
        // work, and an unenforceable budget must still never read as a job
        // that passed (D30).
        let mut ran = Attempt {
            verdict: Some(JobOutcome::Exited { code: 0 }),
            ..Attempt::default()
        };
        ran.ended_unmeasurable(
            &Unmeasurable {
                why: "/srv/herd/jobs/j7/work cannot be measured: Not a directory".to_owned(),
                refused: false,
            },
            budget,
        );
        assert!(ran.setup_failed.is_none());
        let JobOutcome::StoppedByCap { cap, limit, detail } = ran.outcome() else {
            panic!("{:?}", ran.outcome())
        };
        assert_eq!((cap, limit), (Cap::Disk, budget));
        assert!(
            detail.is_some_and(|detail| detail.contains("could not measure")),
            "the figure alone would be a true number in a false sentence (D30)"
        );
    }

    /// A refusal seen while the workspace is being **taken down** leaves the
    /// verdict alone (D108, D166).
    ///
    /// The `down` step is the one that runs however the others ended, so it
    /// is the one place where an ending read off a step would arrive after a
    /// verdict had already been established — and throwing a verdict away
    /// because a workspace would not go quietly is exactly what D108 forbids.
    #[test]
    fn a_refusal_during_the_takedown_leaves_an_established_verdict_alone() {
        let mut ran = Attempt {
            verdict: Some(JobOutcome::Exited { code: 0 }),
            ..Attempt::default()
        };
        assert!(
            ran.refused_workspace(Some(Step::Down), "/out/packs cannot be read".to_owned()),
            "a takedown's refusal is said in the log, not made the job's ending"
        );
        assert!(ran.setup_failed.is_none());
        assert_eq!(ran.outcome(), JobOutcome::Exited { code: 0 });

        // On the job's own steps it *is* the ending, and the first one is the
        // one kept: the steps after it see the same path still unreadable.
        assert!(!ran.refused_workspace(Some(Step::Run), "the first path".to_owned()));
        ran.refused_workspace(None, "a later one".to_owned());
        let JobOutcome::SetupFailed { reason } = ran.outcome() else {
            panic!("{:?}", ran.outcome())
        };
        assert_eq!(reason, "the first path");
    }

    /// An attempt whose steps all ran to their end is one nothing stopped, so
    /// the last disk measurement is still allowed to stop it.
    #[test]
    fn only_an_attempt_nothing_ended_is_judged_by_the_last_measurement() {
        let mut ran = Attempt {
            verdict: Some(JobOutcome::Exited { code: 1 }),
            ..Attempt::default()
        };
        assert!(ran.nothing_ended_it());
        ran.ending = Some(Ending::Cancelled("a requester asked".to_owned()));
        assert!(!ran.nothing_ended_it());
    }

    /// A job whose **setup** failed is the case the first consumer is blocked
    /// on: nothing of theirs ran, and what Herd made for it is still on the
    /// host. Every ending a job can have — a setup failure, a refusal, a cap,
    /// a clean exit — leaves through [`reclaim_or_say`], so this proves the
    /// wiring once at the function all four arms call.
    ///
    /// The workspace is wrecked the way `root.rs` and the drain's own test
    /// wreck one: the base directory is replaced by a **file**, so
    /// `remove_dir_all` fails with `ENOTDIR` and no mode can fix it. That is
    /// the one unreclaimable workspace an unprivileged test can make, and the
    /// 4 096 bytes it holds are the figure the entry has to carry.
    #[test]
    fn a_workspace_a_job_left_behind_reaches_the_notebook_with_its_figure() {
        let temp = crate::testing::TempDir::new("janitor-from-a-job");
        let root = DataRoot::open(temp.path()).expect("a throwaway data root opens");
        let job_id = JobId::new("j0000000030");
        let workspace = root.open_workspace(&job_id).expect("a workspace is made");
        std::fs::remove_dir_all(workspace.base_dir()).expect("the fixture clears it");
        std::fs::write(workspace.base_dir(), vec![b'w'; 4_096]).expect("the fixture wrecks it");

        let session = Session::new();
        let _cancel = session
            .take_job(&job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");
        reclaim_or_say(&root, &workspace, &job_id, 1, &session);

        let notebook =
            std::fs::read_to_string(root.left_behind_path()).expect("the notebook is written");
        assert!(
            notebook.contains(&format!("left-behind: {}", workspace.base_dir().display())),
            "the path the owner has to go to: {notebook}"
        );
        assert!(
            notebook.contains(&format!(
                "holds: {} in 1 file(s)",
                crate::size::show_bytes(4_096)
            )),
            "and what it weighs, or the owner cannot tell it from an empty one: {notebook}"
        );
        assert!(
            notebook.contains("left-by: job j0000000030, attempt 1"),
            "and whose job it was: {notebook}"
        );
        // The requester's half of the same fact, unchanged by any of this.
        assert!(
            session
                .held_output_of(&job_id, 1)
                .contains("could not be reclaimed"),
            "the job's own record still carries it too"
        );
        let _ = std::fs::remove_file(workspace.base_dir());
    }

    /// A janitor's notebook is not on the critical path. With the notebook's
    /// path occupied by a directory no `write` can replace, the job's record
    /// still reaches the session — which is the frame the hub is sent — and
    /// nothing in the reclaim path returns an error for it.
    #[test]
    fn a_notebook_that_cannot_be_written_never_costs_a_job_its_record() {
        let temp = crate::testing::TempDir::new("janitor-unwritable-from-a-job");
        let root = DataRoot::open(temp.path()).expect("a throwaway data root opens");
        std::fs::create_dir_all(root.left_behind_path()).expect("the fixture blocks the path");
        let job_id = JobId::new("j0000000031");
        let workspace = root.open_workspace(&job_id).expect("a workspace is made");
        std::fs::remove_dir_all(workspace.base_dir()).expect("the fixture clears it");
        std::fs::write(workspace.base_dir(), b"not a directory").expect("the fixture wrecks it");

        let session = Session::new();
        let _cancel = session
            .take_job(&job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");
        reclaim_or_say(&root, &workspace, &job_id, 1, &session);

        let record = session.held_output_of(&job_id, 1);
        assert!(
            record.contains("could not be reclaimed") && record.contains("left on this host"),
            "a notebook nobody can write must not cost a job its record: {record:?}"
        );
        assert!(
            root.left_behind_path().is_dir(),
            "and nothing removed or renamed what was in the way"
        );
        let _ = std::fs::remove_file(workspace.base_dir());
    }

    // ------------------------------------- one kill, one step (D-NEXT) ----

    /// A reading of the job cgroup's two kill counters, as a step takes it.
    fn read(processes: u64, group_kills: Option<u64>) -> OomKills {
        OomKills {
            processes,
            group_kills,
        }
    }

    /// The rule this replaces, written out so the defect is **computed** here
    /// and not asserted from memory: `oomed = oom_kills > since` over the
    /// per-process counter, which is what `job.rs` did until D-NEXT.
    fn the_old_rule(before: OomKills, now: OomKills) -> bool {
        now.processes > before.processes
    }

    fn signalled() -> JobOutcome {
        JobOutcome::Signalled { signal: 9 }
    }

    fn exited(code: i32) -> JobOutcome {
        JobOutcome::Exited { code }
    }

    /// Job j0000000069's own readings, replayed: **one** OOM event, charged
    /// to **one** step.
    ///
    /// The first consumer's record had `up` exit 0, `fetch` exit 0, `run`
    /// stopped at the memory cap after 218 217 ms and `down` stopped at the
    /// memory cap after 5 097 ms — the same sentence twice. Their kernel log
    /// has one `oom-killer` invocation, at the end of `run`, a burst of
    /// per-process kills over about 4 ms, `down` starting 2 ms after that
    /// burst and **no** kernel OOM line at all during `down`'s 5.1 s.
    ///
    /// So the counter was still climbing when `run` read it: eight tasks were
    /// in the kill set and `run`'s reading was taken 3.7 ms into the walk.
    /// The remainder of the **same** kill was what `down` saw.
    #[test]
    fn one_oom_kill_is_charged_to_one_step() {
        let mut mark = OomWatermark::default();

        // `up` and `fetch`: nothing has been killed.
        assert!(!mark.killed_in_this_step(read(0, Some(0)), &exited(0)));
        assert!(!mark.killed_in_this_step(read(0, Some(0)), &exited(0)));

        // `run`: the group kill is counted once and already settled; the
        // per-process counter is three of eight and still going.
        let at_run = read(3, Some(1));
        assert!(
            mark.killed_in_this_step(at_run, &signalled()),
            "the kill happened in `run` and `run` is what must report it"
        );

        // `down`: 5.1 s later, the same kill, fully counted. Nothing new.
        let at_down = read(8, Some(1));
        assert!(
            the_old_rule(at_run, at_down),
            "the detector is proved on the known-bad case first: the old rule \
             is true here, which is the whole defect"
        );
        assert!(
            !mark.killed_in_this_step(at_down, &exited(0)),
            "one kill, one step: `down` must not inherit `run`'s verdict"
        );
    }

    /// The same replay with a `down` that ended **without** an exit code, so
    /// that the group counter is what has to carry it.
    ///
    /// Both guards fix j0000000069 and the two are tested apart on purpose: a
    /// test where `down` exits 0 passes with the group counter taken out, and
    /// one where it is signalled passes with the exit-code clause taken out.
    /// Together they leave neither clause able to be removed unnoticed.
    #[test]
    fn one_oom_kill_is_charged_to_one_step_even_when_down_is_signalled_too() {
        let mut mark = OomWatermark::default();
        let at_run = read(3, Some(1));
        assert!(mark.killed_in_this_step(at_run, &signalled()));

        let at_down = read(8, Some(1));
        assert!(
            the_old_rule(at_run, at_down),
            "the old rule is true here too, which is what has to be refused"
        );
        assert!(
            !mark.killed_in_this_step(at_down, &signalled()),
            "no second event in `oom_group_kill`, so no second kill — \
             whatever ended `down`"
        );
    }

    /// And the fix does not work by making `down` unable to report a cap.
    /// Two real kills — two `oom_group_kill` events — are both reported.
    #[test]
    fn two_real_kills_are_both_reported() {
        let mut mark = OomWatermark::default();
        assert!(mark.killed_in_this_step(read(8, Some(1)), &signalled()));
        assert!(
            mark.killed_in_this_step(read(11, Some(2)), &signalled()),
            "a second event in `down` is a second kill and says so"
        );
    }

    /// The same, on a kernel with no `oom_group_kill` to read (before Linux
    /// 5.17): the per-process counter is all there is, and two kills still
    /// read as two.
    #[test]
    fn two_real_kills_are_both_reported_without_the_group_counter() {
        let mut mark = OomWatermark::default();
        assert!(mark.killed_in_this_step(read(8, None), &signalled()));
        assert!(mark.killed_in_this_step(read(16, None), &signalled()));
    }

    /// A step that exited 0 was not stopped by anything, whatever a counter
    /// read after it says. This is the clause that holds on a kernel with no
    /// `oom_group_kill`, where the mid-burst reading cannot be told apart
    /// from a new kill by the counter alone.
    #[test]
    fn a_step_that_exited_cleanly_is_never_said_to_have_been_capped() {
        let mut mark = OomWatermark::default();
        assert!(mark.killed_in_this_step(read(3, None), &signalled()));
        assert!(
            !mark.killed_in_this_step(read(8, None), &exited(0)),
            "a shell that exited 0 is not a shell the kernel killed: \
             `memory.oom.group = 1` takes the whole tree"
        );
    }

    /// And a non-zero code keeps its cap. A job whose own process was killed
    /// while its shell reported 137 **was** stopped by the ceiling, and that
    /// is the case the clause above must not swallow.
    #[test]
    fn a_step_that_exited_non_zero_keeps_its_memory_verdict() {
        let mut mark = OomWatermark::default();
        assert!(mark.killed_in_this_step(read(1, Some(1)), &exited(137)));
    }

    /// An unreadable `memory.events` reads as zero
    /// ([`JobCgroup::oom_kills`]), so the baseline never moves backwards: one
    /// unreadable step must not charge the next one with every kill the job
    /// ever had.
    #[test]
    fn an_unreadable_memory_events_never_charges_the_next_step() {
        let mut mark = OomWatermark::default();
        assert!(mark.killed_in_this_step(read(5, Some(1)), &signalled()));
        // The read failed: zero, which is lower than the baseline.
        assert!(!mark.killed_in_this_step(read(0, None), &signalled()));
        // And the one after it sees the same counters it always saw.
        assert!(
            !mark.killed_in_this_step(read(5, Some(1)), &signalled()),
            "the watermark is a high-water mark, so a failed read costs \
             nothing but that step's own answer"
        );
    }

    // --------------------------------- the step names itself (D-NEXT) ----

    fn a_step(step: Step) -> Stage {
        Stage {
            step: Some(step),
            ..a_stage(Duration::from_secs(60))
        }
    }

    /// What the watchdog saw of a step that nothing but the kernel ended.
    fn watched(outcome: JobOutcome) -> Watched {
        Watched {
            outcome,
            cap: None,
            ending: None,
            peak_disk: None,
            unmeasurable: None,
            peak_rss: None,
        }
    }

    fn at() -> StepAt {
        StepAt {
            started_unix_ms: 0,
            took: Duration::from_secs(1),
        }
    }

    /// The consumer's own figure: a runner-wide `--job-memory-max` of 8 GiB,
    /// against a job that declared 1792 MiB.
    const EIGHT_GIB: u64 = 8_589_934_592;

    /// The line one capped step writes into the job's own log, as a reader
    /// meets it.
    fn capped_line(step: &str) -> String {
        format!("the {step} step was stopped at this runner's own memory cap of {EIGHT_GIB} bytes")
    }

    fn a_session(job_id: &JobId) -> (Session, CancelToken) {
        let session = Session::new();
        let cancel = session
            .take_job(job_id, 1, 4)
            .expect("a fresh session is not draining and is not full");
        (session, cancel)
    }

    /// The step that decided the verdict says so in the job's **own** log,
    /// naming itself and naming whose figure the ceiling is.
    ///
    /// Before this, the cap sentence reached the log only through the
    /// takedown line, so a reader met the memory cap exactly once and it was
    /// attached to `down`.
    #[test]
    fn the_capped_step_names_itself_in_the_job_s_log() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.memory_max = EIGHT_GIB;
        let job_id = JobId::new("j0000000069");
        let (session, _cancel) = a_session(&job_id);
        let mut ran = Attempt::default();

        ran.step_ended(
            &a_step(Step::Run),
            at(),
            watched(signalled()),
            true,
            &caps,
            &session,
            &job_id,
            1,
        );

        let log = session.held_output_of(&job_id, 1);
        assert!(
            log.contains(&capped_line("run")),
            "the step that was stopped names itself and its figure: {log}"
        );
        assert!(
            log.contains("not a figure this job declared"),
            "and the figure is named for what it is — their job declared 1792 \
             MiB and read 8589934592 bytes as its own: {log}"
        );
        assert!(
            !log.contains("stopped this job at its memory cap"),
            "the job's sentence is the attempt's outcome, not a step's line: {log}"
        );
        assert_eq!(
            ran.cap.as_ref().map(|capped| capped.cap),
            Some(Cap::Memory),
            "and the verdict is unchanged: the job was stopped by the memory cap"
        );
    }

    /// A `down` that came off is recorded as having come off: its own exit
    /// code, no cap sentence, and no takedown line claiming a workspace may
    /// have been left behind.
    ///
    /// This is the reader's half of j0000000069. With `oomed` true for
    /// `down`, `capped` overrode `down`'s exit code — the record showed no
    /// code at all and `came_off` was false — so the log gained *"the down
    /// step the runner stopped this job at its memory cap of 8589934592
    /// bytes, so this job's workspace may be left on this host"* for a
    /// takedown that had in fact succeeded.
    #[test]
    fn a_down_step_that_came_off_is_never_called_capped() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.memory_max = EIGHT_GIB;
        let job_id = JobId::new("j0000000069");
        let (session, _cancel) = a_session(&job_id);
        let mut ran = Attempt::default();
        // `run` was stopped by the ceiling, which is the job's verdict.
        ran.step_ended(
            &a_step(Step::Run),
            at(),
            watched(signalled()),
            true,
            &caps,
            &session,
            &job_id,
            1,
        );
        // And `down` ran for 5 s and exited 0, with no new kill.
        ran.step_ended(
            &a_step(Step::Down),
            at(),
            watched(exited(0)),
            false,
            &caps,
            &session,
            &job_id,
            1,
        );

        let down = ran
            .steps
            .iter()
            .find(|step| step.step == Step::Down)
            .expect("the takedown is on the record");
        assert_eq!(down.exit_code, Some(0), "its own code, not a cap's silence");
        assert_eq!(down.ended, None, "and nothing said about how it ended");

        let log = session.held_output_of(&job_id, 1);
        assert!(
            !log.contains("the down step was stopped"),
            "no cap is charged to a takedown that came off: {log}"
        );
        assert!(
            !log.contains("may be left on this host"),
            "and no workspace is said to be left behind by a takedown that \
             did its work: {log}"
        );
    }

    /// Two real kills read as two lines a reader can tell apart without
    /// fetching the record.
    #[test]
    fn two_capped_steps_read_as_two_lines_in_the_log() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.memory_max = EIGHT_GIB;
        let job_id = JobId::new("j0000000460");
        let (session, _cancel) = a_session(&job_id);
        let mut ran = Attempt::default();
        for step in [Step::Run, Step::Down] {
            ran.step_ended(
                &a_step(step),
                at(),
                watched(signalled()),
                true,
                &caps,
                &session,
                &job_id,
                1,
            );
        }

        let log = session.held_output_of(&job_id, 1);
        // The step-naming line, which only a capped step writes: the takedown
        // line that follows it carries the same clause about the ceiling, and
        // counting both would count one step twice.
        let named: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("not a figure this job declared"))
            .collect();
        assert_eq!(
            named.len(),
            2,
            "two kills, two lines, each naming its own step: {log}"
        );
        assert!(
            named[0].contains(&capped_line("run")),
            "{log}"
        );
        assert!(
            named[1].contains(&capped_line("down")),
            "{log}"
        );
        // And the takedown still says what a failed takedown means, as the
        // step's own ending and never in the job's sentence (D108).
        assert!(
            log.contains(
                "the down step was stopped at this runner's own memory cap of 8589934592 bytes, \
                 so this job's workspace may be left on this host"
            ),
            "{log}"
        );
        assert!(
            !log.contains("the down step the runner stopped this job"),
            "the two claims must not be pasted into one clause: {log}"
        );
    }

    /// The time cap keeps its own grammar: its figure is the **step's**, not
    /// the runner's, so its line must not call it the runner's ceiling
    /// (D108).
    #[test]
    fn the_time_cap_s_line_names_whose_figure_it_is() {
        let caps = Confinement::with_data_root("/nowhere");
        let stage = a_step(Step::Run);
        let capped = Capped {
            cap: Cap::Time,
            limit: limit_of(Cap::Time, &stage, &caps),
            detail: None,
        };
        let said = the_cap_that_stopped_this_step(&stage, &capped);
        assert_eq!(
            said,
            "the run step was stopped at the time cap of 60 s, this runner's own figure"
        );
        assert!(
            !said.contains("not a figure this job declared"),
            "a declared figure can be exactly what bounded it: {said}"
        );
    }
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    fn a_pack(key: &str) -> herd_proto::pack::AssignedPack {
        let manifest = b"# chatons pack 1\n".to_vec();
        herd_proto::pack::AssignedPack {
            key: key.to_owned(),
            pack: herd_proto::pack::PackId::of_manifest_bytes(&manifest),
            manifest,
            meta: None,
            holders: Vec::new(),
        }
    }

    /// **A job with no pack is every job this build runs, and it runs.**
    ///
    /// Asserted first because it is the condition every other test in the
    /// suite depends on: the stop that remains for a pack must not stop
    /// anything else.
    #[test]
    fn a_job_that_consumes_and_leaves_no_pack_is_not_handed_back() {
        let packs = Packs {
            packs_in: Vec::new(),
            packs_out: BTreeMap::new(),
        };
        assert_eq!(packs.why_this_runner_cannot(false), None);
        assert_eq!(packs.why_this_runner_cannot(true), None);
    }

    /// **A declared command's packs are laid out and not refused**, which is
    /// the whole of what this step changed: the keys the hub resolved reach
    /// `Session::lay_out_packs` instead of a sentence handing the attempt
    /// back.
    ///
    /// Asserted here, on the predicate, because this is the one place that
    /// could silently put the old stop back: a refusal returned for a
    /// declared command would hand back every job the first consumer's build
    /// and test pair makes, and it would look like caution.
    #[test]
    fn a_declared_command_s_packs_are_laid_out_rather_than_handed_back() {
        let packs = Packs {
            packs_in: vec![a_pack("built"), a_pack("fixtures")],
            packs_out: BTreeMap::from([("built".to_owned(), "artefacts".to_owned())]),
        };
        assert_eq!(
            packs.why_this_runner_cannot(true),
            None,
            "a declared command with packs at both ends is run, not refused"
        );
    }

    /// **A pack on an ad-hoc job is handed back with both halves named**
    /// (D150, D107).
    ///
    /// An ad-hoc command line gets no `{in}` and no `{out}` and names no
    /// commit, so there is nowhere to lay a pack out, nowhere to take one
    /// from, and no `(commit, key)` to register one for. The sentence is the
    /// whole of the behaviour, so it is the thing asserted: it names every
    /// key at each end, and both directories, so nobody has to guess what
    /// could not be done.
    #[test]
    fn a_pack_on_a_job_that_is_not_declared_is_named_at_both_ends() {
        let packs = Packs {
            packs_in: vec![a_pack("built"), a_pack("fixtures")],
            packs_out: BTreeMap::from([("kept".to_owned(), "artefacts".to_owned())]),
        };
        let said = packs
            .why_this_runner_cannot(false)
            .expect("a pack on an ad-hoc job is a refusal");
        assert!(
            said.contains("built, fixtures"),
            "every key it would have consumed, in the order the hub sent them: {said}"
        );
        assert!(
            said.contains("kept"),
            "and the key it would have left: {said}"
        );
        assert!(
            said.contains(herd_proto::IN_TOKEN) && said.contains(herd_proto::OUT_TOKEN),
            "and both directories it has none of: {said}"
        );
        assert!(
            said.contains("names no commit"),
            "and the reason a pack could not be registered either: {said}"
        );
    }

    /// Each end refuses on its own: a job that only consumes, and a job that
    /// only leaves, are both refused when they are not declared.
    #[test]
    fn either_end_alone_refuses_an_undeclared_job() {
        let consuming = Packs {
            packs_in: vec![a_pack("built")],
            packs_out: BTreeMap::new(),
        };
        let said = consuming
            .why_this_runner_cannot(false)
            .expect("a pack to consume is a refusal on its own");
        assert!(
            said.contains("needs the pack(s) built") && !said.contains("leaves the pack"),
            "only the half that applies is said: {said}"
        );
        let leaving = Packs {
            packs_in: Vec::new(),
            packs_out: BTreeMap::from([("built".to_owned(), "artefacts".to_owned())]),
        };
        let said = leaving
            .why_this_runner_cannot(false)
            .expect("a pack to leave is a refusal on its own");
        assert!(
            said.contains("leaves the pack(s) built") && !said.contains("needs the pack"),
            "and so is the other half: {said}"
        );
    }

    /// **What a job leaves comes off the assignment**, so the collection
    /// takes exactly the paths the hub resolved and no others.
    #[test]
    fn what_a_job_leaves_is_what_the_assignment_said() {
        let packs_out = BTreeMap::from([
            ("built".to_owned(), "artefacts".to_owned()),
            ("logs".to_owned(), "logs/pack".to_owned()),
        ]);
        assert_eq!(declared_leaves(&packs_out), packs_out);
        assert!(
            declared_leaves(&BTreeMap::new()).is_empty(),
            "and a job that leaves nothing hands the collection nothing"
        );
    }
}
