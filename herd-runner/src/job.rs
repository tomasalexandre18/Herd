//! Running one job, under every cap at once.
//!
//! The order matters and is the whole of E-34 and E-30:
//!
//! 1. A workspace inside the data root, empty, and the job's working
//!    directory. Removed when the job ends, whatever the job did in it.
//! 2. A cgroup with the memory ceiling set **before** any process is in it.
//! 3. The job's own session, so every process it starts shares one group.
//! 4. Nice on CPU and the idle class on IO.
//! 5. A time cap, a log cap and a disk budget, each ending the job with a
//!    named reason rather than letting it hang, flood or fill a disk.
//! 6. A kill of the whole group and the whole cgroup when the job's own
//!    process is gone — so a job that leaves a background child behind does
//!    not outlive itself.
//! 7. What was measured, reported with the outcome (E-15).

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_proto::{Cap, JobId, JobMetrics, JobOutcome, LogStream, ToHub};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use crate::cgroup::JobCgroup;
use crate::confine::{job_environment, kill_process_group, ChildSetup, Confined, POLL_INTERVAL};
use crate::metrics::{host_load_milli, process_rss_bytes};
use crate::root::Workspace;
use crate::{Link, Session};

/// Output is read in chunks this size; a chunk is sent as soon as the process
/// has written anything, which is what makes a log arrive while it is running.
const READ_CHUNK: usize = 8 * 1024;

/// How long the runner waits for a killed job's pipes to close before giving
/// up on them. A kill by cgroup or by process group is immediate; this bound
/// exists so that one wedged descriptor cannot hold a job's report forever.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// Which cap tripped, as one atom the log pumps and the watchdog share.
const NO_CAP: u8 = 0;
const LOG_CAP: u8 = 1;

/// Run one job: announce it, stream both streams, cap it, report how it ended
/// and what it cost.
pub async fn execute(
    job_id: JobId,
    command: Vec<String>,
    session: Session,
    confined: Arc<Confined>,
) {
    let Some((program, arguments)) = command.split_first() else {
        report_could_not_start(&job_id, &session, "a job needs a command to run".to_owned());
        return;
    };

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

    let cgroup_name = workspace
        .base_dir()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| job_id.to_string());
    let cgroup = confined.ceiling().open_for_job(&cgroup_name);
    // The descriptor is opened here, in the runner, and written by the job's
    // own process before it execs: that is what leaves no instant in which a
    // job process exists outside its ceiling.
    let procs = match cgroup.as_ref().map(JobCgroup::procs_file).transpose() {
        Ok(procs) => procs,
        Err(error) => {
            let _ = workspace.remove();
            report_could_not_start(
                &job_id,
                &session,
                format!("the job's memory cgroup could not be opened for writing: {error}"),
            );
            return;
        }
    };

    let caps = confined.caps();
    let setup = ChildSetup {
        cgroup_procs_fd: procs
            .as_ref()
            .map(|file| {
                use std::os::fd::AsRawFd;
                file.as_raw_fd()
            })
            .unwrap_or(-1),
        nice: caps.nice,
        // Only where there is no cgroup: the two must never be stacked, or a
        // job under the real ceiling would also hit a per-process limit and
        // report the wrong thing.
        address_space_max: match cgroup {
            Some(_) => None,
            None => Some(caps.memory_max),
        },
    };

    let mut builder = Command::new(program);
    builder
        .args(arguments)
        .current_dir(workspace.work_dir())
        // Nothing of the runner's environment is a job's business.
        .env_clear()
        .envs(job_environment(&workspace))
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
            let _ = workspace.remove();
            // Names the executable, so the reason reads as Herd failing to
            // run this command rather than the command failing.
            report_could_not_start(&job_id, &session, format!("{program}: {error}"));
            return;
        }
    };
    // The runner holds no copy of the descriptor while the job runs.
    drop(procs);
    let started = Instant::now();
    let pid = child.id().unwrap_or(0);
    session.link.send(ToHub::JobStarted {
        job_id: job_id.clone(),
    });

    // One counter for both streams, so the hub can restore the order this
    // runner saw the output in, and one byte counter for the log cap.
    let seq = Arc::new(AtomicU64::new(0));
    let log_bytes = Arc::new(AtomicU64::new(0));
    let tripped = Arc::new(AtomicU8::new(NO_CAP));
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let pumps = tokio::spawn({
        let pump_out = stream_output(
            stdout,
            LogStream::Stdout,
            job_id.clone(),
            Arc::clone(&seq),
            Arc::clone(&log_bytes),
            Arc::clone(&tripped),
            caps.log_cap,
            session.link.clone(),
        );
        let pump_err = stream_output(
            stderr,
            LogStream::Stderr,
            job_id.clone(),
            Arc::clone(&seq),
            Arc::clone(&log_bytes),
            Arc::clone(&tripped),
            caps.log_cap,
            session.link.clone(),
        );
        async move {
            tokio::join!(pump_out, pump_err);
        }
    });

    let watched = watch(
        &mut child,
        Watch {
            pid,
            started,
            timeout: caps.timeout,
            disk_budget: caps.disk_budget,
            workspace: workspace.clone(),
            cgroup: cgroup.as_ref(),
            tripped: Arc::clone(&tripped),
        },
    )
    .await;

    // Read the kernel's own verdict before killing anything ourselves, so
    // that an out-of-memory kill is told apart from the runner's kill.
    let oom_kills = cgroup.as_ref().map_or(0, JobCgroup::oom_kills);
    let peak_memory_bytes = cgroup
        .as_ref()
        .and_then(JobCgroup::peak_bytes)
        .or(watched.peak_rss);

    // Whatever the job left behind dies with it, every time: the job's own
    // process being gone is not the same thing as the job being over
    // (E-34, E-06's kill hardened).
    if let Some(cgroup) = &cgroup {
        let _ = cgroup.kill();
    }
    kill_process_group(pid);

    // Both pipes are drained before the outcome is reported, so no output can
    // arrive after JobFinished — bounded, because a descriptor held by
    // something we failed to kill must not hold the report.
    if tokio::time::timeout(DRAIN_GRACE, pumps).await.is_err() {
        eprintln!("herd-runner: job {job_id} left a pipe open after being killed");
    }

    let outcome = if let Some(cap) = watched.cap {
        JobOutcome::StoppedByCap {
            cap,
            limit: limit_of(cap, confined.caps()),
        }
    } else if oom_kills > 0 {
        // The kernel killed the job's cgroup for going past the ceiling. It
        // reaches us as a signal, and a bare "signalled by 9" would tell
        // nobody why (E-30).
        JobOutcome::StoppedByCap {
            cap: Cap::Memory,
            limit: caps.memory_max,
        }
    } else {
        watched.outcome
    };

    let final_disk = workspace.size_bytes();
    let metrics = JobMetrics {
        host_load_milli: host_load_milli(),
        peak_memory_bytes,
        bytes_written: Some(watched.peak_disk.max(final_disk)),
        log_bytes: log_bytes.load(Ordering::SeqCst),
        wall_time_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };

    // The cgroup goes away with its guard; the workspace is removed here, and
    // a workspace that will not go away is said out loud: it is a disk
    // filling up, which is exactly what a runner must not do to its host.
    drop(cgroup);
    if let Err(error) = workspace.remove() {
        eprintln!(
            "herd-runner: the workspace {} of job {job_id} could not be removed: {error}",
            workspace.base_dir().display()
        );
    }

    // Held until the hub acknowledges it, and redelivered on every
    // connection until it does: a result is the one thing a runner cannot
    // reproduce (E-44, D17, D19).
    session.report(&job_id, outcome, metrics);
}

/// What the watchdog needs to know about the job it is watching.
struct Watch<'a> {
    pid: u32,
    started: Instant,
    timeout: Duration,
    disk_budget: u64,
    workspace: Workspace,
    cgroup: Option<&'a JobCgroup>,
    tripped: Arc<AtomicU8>,
}

/// What the watchdog saw.
struct Watched {
    outcome: JobOutcome,
    cap: Option<Cap>,
    peak_disk: u64,
    peak_rss: Option<u64>,
}

/// Wait for the job, enforcing the caps that need watching.
///
/// Every cap ends the same way: kill the job's whole group and cgroup, then
/// go on waiting for the process so that it is reaped rather than left as a
/// zombie, and report the cap by name.
async fn watch(child: &mut Child, watch: Watch<'_>) -> Watched {
    let deadline = watch.started + watch.timeout;
    let mut cap: Option<Cap> = None;
    let mut peak_disk = 0u64;
    let mut peak_rss: Option<u64> = None;

    let outcome = loop {
        // `Child::wait` is cancel-safe, so losing this race costs nothing.
        tokio::select! {
            status = child.wait() => break match status {
                Ok(status) => outcome_of(status),
                // The process started but Herd never learned how it ended,
                // so it has no code and no signal to report. M3's "lost" is
                // where this belongs; until then it is Herd failing to run
                // the job, said in full.
                Err(error) => JobOutcome::CouldNotStart {
                    reason: format!("waiting for the job's process failed: {error}"),
                },
            },
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }

        if cap.is_none() {
            // The disk walk is filesystem work, so it happens off the
            // runtime: a job with a large workspace must not stall the
            // runner's connection to the hub.
            let workspace = watch.workspace.clone();
            let used = tokio::task::spawn_blocking(move || workspace.size_bytes())
                .await
                .unwrap_or(0);
            peak_disk = peak_disk.max(used);
            if watch.cgroup.is_none() {
                if let Some(rss) = process_rss_bytes(watch.pid) {
                    peak_rss = Some(peak_rss.unwrap_or(0).max(rss));
                }
            }

            cap = if watch.tripped.load(Ordering::SeqCst) == LOG_CAP {
                Some(Cap::Log)
            } else if used > watch.disk_budget {
                Some(Cap::Disk)
            } else if Instant::now() >= deadline {
                Some(Cap::Time)
            } else {
                None
            };
            if cap.is_some() {
                if let Some(cgroup) = watch.cgroup {
                    let _ = cgroup.kill();
                }
                kill_process_group(watch.pid);
            }
        }
    };

    Watched {
        outcome,
        cap,
        peak_disk,
        peak_rss,
    }
}

/// The figure that goes with a cap, in the cap's own unit.
fn limit_of(cap: Cap, caps: &crate::confine::Confinement) -> u64 {
    match cap {
        Cap::Time => caps.timeout.as_secs(),
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
fn report_could_not_start(job_id: &JobId, session: &Session, reason: String) {
    session.report(
        job_id,
        JobOutcome::CouldNotStart { reason },
        JobMetrics::default(),
    );
}

/// Forward one pipe to the hub as it fills, never waiting for the process,
/// and stop at the log cap.
///
/// The cap is counted across both streams, because the cap is about the log
/// the hub stores and the requester reads, which is one log.
#[allow(clippy::too_many_arguments)]
async fn stream_output<R>(
    mut pipe: R,
    stream: LogStream,
    job_id: JobId,
    seq: Arc<AtomicU64>,
    log_bytes: Arc<AtomicU64>,
    tripped: Arc<AtomicU8>,
    log_cap: u64,
    link: Link,
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
                link.send_chunk(&job_id, stream, &seq, buffer[..keep].to_vec());
                if keep < read {
                    tripped.store(LOG_CAP, Ordering::SeqCst);
                    break;
                }
            }
            Err(error) => {
                link.send_chunk(
                    &job_id,
                    LogStream::Stderr,
                    &seq,
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

    #[test]
    fn every_cap_s_limit_is_reported_in_the_cap_s_own_unit() {
        let mut caps = Confinement::with_data_root("/nowhere");
        caps.timeout = Duration::from_secs(42);
        caps.memory_max = 1 << 20;
        caps.log_cap = 2 << 20;
        caps.disk_budget = 3 << 20;
        assert_eq!(limit_of(Cap::Time, &caps), 42);
        assert_eq!(limit_of(Cap::Memory, &caps), 1 << 20);
        assert_eq!(limit_of(Cap::Log, &caps), 2 << 20);
        assert_eq!(limit_of(Cap::Disk, &caps), 3 << 20);
    }
}
