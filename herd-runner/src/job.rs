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

use crate::cgroup::JobCgroup;
use crate::confine::{
    job_environment, kill_process_group, ChildSetup, Confined, JobsAtOnce, POLL_INTERVAL,
};
use crate::metrics::{host_load_milli, process_rss_bytes};
use crate::reclaim::{kill_by_marker, JobTag};
use crate::root::{Unmeasurable, Workspace};
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

/// Run one job: announce it, stream every step's output, cap each step,
/// report how it ended and what it cost.
///
/// Eight arguments, and they are the job: what to run, how it was declared,
/// what it is allowed to cost, who to tell and how full this machine already
/// is. Collapsing them into a struct would name the struct after this
/// function and move the same list one line up.
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    job_id: JobId,
    attempt: u32,
    command: Vec<String>,
    declaration: Option<Declaration>,
    declared_timeout: Option<Duration>,
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
        // as in the sentence (D-NEXT): the hub charges a job a go for an
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

    // Before anything is made on this host: a command line nobody could run.
    if let Some(declaration) = &declaration {
        if let Some(refusal) = step::why_that_line_will_not_do(declaration, &command) {
            session.abandon(&job_id, attempt, &refusal);
            return;
        }
    } else if command.is_empty() {
        report_could_not_start(&job_id, &session, "a job needs a command to run".to_owned());
        return;
    }

    // E-45: the job gets one place to work, inside the one data root. A root
    // that will not give one is a refusal naming the path, not a job that
    // runs in the runner's own directory.
    let workspace = match confined.root().open_workspace(&job_id) {
        Ok(workspace) => workspace,
        Err(reason) => {
            report_could_not_start(&job_id, &session, reason);
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
                reclaim_or_say(&workspace, &job_id, &session);
                report_could_not_start(&job_id, &session, reason);
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
                    reclaim_or_say(&workspace, &job_id, &session);
                    session.abandon(&job_id, attempt, &refusal);
                    return;
                }
            }
        }
    };
    if stages.is_empty() {
        reclaim_or_say(&workspace, &job_id, &session);
        report_could_not_start(&job_id, &session, "a job needs a command to run".to_owned());
        return;
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
    let mut oom_kills = 0u64;

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
            say_in_the_log(&session, &job_id, said);
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
                Arc::clone(&log_bytes),
                Arc::clone(&tripped),
                caps.log_cap,
                session.clone(),
            );
            let pump_err = stream_output(
                stderr,
                LogStream::Stderr,
                job_id.clone(),
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
        // that an out-of-memory kill is told apart from the runner's kill.
        let since = oom_kills;
        oom_kills = cgroup.as_ref().map_or(0, JobCgroup::oom_kills);
        let oomed = oom_kills > since;

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
    let collected = match &ran.ending {
        Some(Ending::Abandoned(_)) => crate::outputs::Collected::nothing(),
        _ => crate::outputs::collect(
            workspace.out_dir(),
            confined.root().path(),
            caps.output_cap,
            &crate::outputs::spool_path(confined.root().path(), &job_id, attempt),
        ),
    };
    let crate::outputs::Collected { outputs, spool } = collected;
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
    reclaim_or_say(&workspace, &job_id, &session);

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
            session.finish(&job_id, outcome, metrics, ran.steps, outputs, spool);
        }
        None => session.finish(&job_id, outcome, metrics, ran.steps, outputs, spool),
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
            Some(Step::Down) => self.say_the_takedown_did_not_come_off(session, job_id, &how),
            Some(step) => self.say_the_job_never_started(session, job_id, step, &how),
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
        let how = Self::how_it_went(exit_code, &ended);
        self.record(stage, at, exit_code, ended, session, job_id, attempt);

        // Ahead of everything else read off this step: a workspace the runner
        // was refused is the machine's problem at `up` exactly as it is at
        // `run`, and letting a preparation step turn it into
        // `could_not_start` would hide both the machine's part in it and the
        // path somebody has to go and fix (D166).
        if let Some(reason) = refused_workspace {
            if self.refused_workspace(stage.step, reason) {
                self.say_the_takedown_did_not_come_off(session, job_id, &how);
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
                    self.say_the_takedown_did_not_come_off(session, job_id, &how);
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
                        self.say_the_job_never_started(session, job_id, step, &how);
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
        step: Step,
        how: &str,
    ) {
        self.could_not_start = Some(format!("the job never started: its {step} step {how}"));
        say_in_the_log(
            session,
            job_id,
            &format!(
                "the {step} step {how}, so this job never started and the declared command \
                 was not run"
            ),
        );
    }

    /// A takedown that did not come off: said in the job's log, because bytes
    /// left on a host nobody will collect are what invariant 2 forbids above
    /// all — and the verdict is left exactly as it was.
    fn say_the_takedown_did_not_come_off(&self, session: &Session, job_id: &JobId, how: &str) {
        say_in_the_log(
            session,
            job_id,
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
                Step::Fetch => self.the_revision_it_reached(session, job_id),
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
    fn the_revision_it_reached(&self, session: &Session, job_id: &JobId) -> Option<String> {
        let path = self.ran_path.as_ref()?;
        match step::ran_commit(path) {
            step::RanCommit::Said(revision) => Some(revision),
            step::RanCommit::Nothing => None,
            step::RanCommit::Unusable(why) => {
                say_in_the_log(
                    session,
                    job_id,
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
fn say_in_the_log(session: &Session, job_id: &JobId, said: &str) {
    session.send_chunk(
        job_id,
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
fn reclaim_or_say(workspace: &Workspace, job_id: &JobId, session: &Session) {
    let Err(error) = workspace.reclaim() else {
        return;
    };
    let said = format!(
        "herd-runner: the workspace {} of job {job_id} could not be reclaimed and is \
         left on this host: {error}\n",
        workspace.base_dir().display()
    );
    eprintln!("{}", said.trim_end());
    // Into the job's log, which is part of its record: the runner's own
    // stderr is not somewhere a requester can read. Numbered from the job's
    // own counter like any other chunk — a number of its own invention would
    // read, at the hub, as every chunk in between having been lost (D29).
    session.send_chunk(job_id, LogStream::Stderr, said.into_bytes());
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
fn report_could_not_start(job_id: &JobId, session: &Session, reason: String) {
    session.finish(
        job_id,
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
async fn stream_output<R>(
    mut pipe: R,
    stream: LogStream,
    job_id: JobId,
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
                session.send_chunk(&job_id, stream, buffer[..keep].to_vec());
                if keep < read {
                    tripped.store(LOG_CAP, Ordering::SeqCst);
                    break;
                }
            }
            Err(error) => {
                session.send_chunk(
                    &job_id,
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
}
