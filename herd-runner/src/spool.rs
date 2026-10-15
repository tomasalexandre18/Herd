//! A verdict that has to outlive the process holding it (D34).
//!
//! A runner holds a finished job's result until the hub acknowledges it
//! (E-44), because the process is gone and the result exists nowhere else.
//! That hold is memory, and a runner asked to stop while the hub is
//! unreachable would take the verdict with it — the one thing Herd never does
//! (D19). So on its way out the runner writes the `job_finished` frame
//! itself, verbatim and fsynced, to `<data root>/held-results/<job>.json`,
//! outside `jobs/` so that the startup reaper cannot clear it.
//!
//! **And reads it back.** The next start picks up every file here before it
//! dials, offers them on every link it makes, and removes one only once the
//! hub has said the verdict is on its disk. No human is in that loop: a
//! verdict written down by a runner being stopped reaches the hub by itself,
//! which is the whole point of writing it down.
//!
//! Four things can happen to a spooled verdict, and each is said out loud:
//!
//! - the hub acknowledges it, and the file goes;
//! - the hub it was produced for no longer exists — its state root was
//!   recreated, and the job ids this file names mean other jobs now — so it is
//!   **never offered**, and its bytes are kept under a name that says why. A
//!   verdict offered to the wrong state is worse than a verdict nobody
//!   records: it lands on another job's record and reads like the truth;
//! - the hub says the attempt is over — an unknown job, a job already
//!   finished, an attempt superseded by a requeue (D39) — and the file goes
//!   **by name**, because a verdict the hub will never record must not be
//!   offered on every connection for ever;
//! - the file cannot be decoded at all, and it is set aside under a name that
//!   says so rather than deleted: an unreadable file may still be the only
//!   trace of a verdict left anywhere on this host.

use std::fs;
use std::path::{Path, PathBuf};

use herd_proto::{to_frame, JobId, JobOutcome, ToHub};

use crate::root::DataRoot;

/// What a file that could not be decoded is renamed to.
const UNREADABLE: &str = "unreadable";

/// What a file belonging to another hub's state root is renamed to.
///
/// Kept, not deleted, for the same reason as an unreadable one: it is
/// somebody's verdict, and the hub that could have recorded it is the one
/// whose state no longer exists. Nobody can record it now, but a human
/// looking for what happened to a job on a hub that was wiped has to be able
/// to find it.
const FOREIGN: &str = "foreign-state-root";

/// One verdict read back from the data root, waiting for the hub.
#[derive(Debug, Clone)]
pub struct Spooled {
    path: PathBuf,
    job_id: JobId,
    attempt: u32,
    outcome: JobOutcome,
    frame: ToHub,
}

impl Spooled {
    pub fn job_id(&self) -> &JobId {
        &self.job_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The frame to send, exactly as the runner that produced it wrote it.
    pub fn frame(&self) -> &ToHub {
        &self.frame
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The hub has it on its disk: the file is no longer the only copy, so it
    /// goes. Says so, because a verdict arriving a restart late is worth a
    /// line in the log of the runner that delivered it.
    pub fn delivered(&self) -> Vec<String> {
        let mut said = vec![format!(
            "the verdict of {} ({:?}), written down when a previous runner was stopped, \
             has reached the hub and been acknowledged",
            self.job_id, self.outcome
        )];
        said.extend(self.remove());
        said
    }

    /// The hub will never record it: the job is unknown to it, already
    /// finished, or this attempt was superseded. Named rather than retried.
    pub fn refused(&self, reason: &str) -> Vec<String> {
        let mut said = vec![format!(
            "the hub will not record the spooled verdict of {} (attempt {}, {:?}): \
             {reason}. It is dropped rather than offered again, and this line is the \
             last record of it",
            self.job_id, self.attempt, self.outcome
        )];
        said.extend(self.remove());
        said
    }

    /// This verdict was produced for another hub's state root, so no hub will
    /// ever record it: it is not offered, and its file is kept under a name
    /// that says why.
    ///
    /// The case this exists for needs no hand-editing of anything. A hub's
    /// state root is recreated — a new volume, a wiped directory — and its job
    /// ids start again at `j0000000001`, while this file, written for the
    /// `j0000000001` of the root that is gone, is still on this disk and still
    /// looks exactly like a verdict the hub is waiting for. Offering it would
    /// put an exit code from one job onto the record of another.
    pub fn for_a_foreign_state_root(&self, reason: &str) -> Vec<String> {
        vec![format!(
            "the spooled verdict of {} (attempt {}, {:?}) was produced for another hub's \
             state root ({reason}); no hub can record it now, so it is NOT offered. {}",
            self.job_id,
            self.attempt,
            self.outcome,
            set_aside(&self.path, FOREIGN)
        )]
    }

    fn remove(&self) -> Vec<String> {
        match fs::remove_file(&self.path) {
            Ok(()) => Vec::new(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => vec![format!(
                "COULD NOT remove the spool file {}: {error}. It will be offered again at \
                 the next start, which the hub answers as a verdict it already has",
                self.path.display()
            )],
        }
    }
}

/// Every verdict a previous life of this runner left behind, and what to say
/// about the ones that could not be read.
pub fn pick_up(root: &DataRoot) -> (Vec<Spooled>, Vec<String>) {
    let directory = root.held_results_dir();
    let mut spooled = Vec::new();
    let mut said = Vec::new();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        // No directory at all is the ordinary case: nothing was ever spooled.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (spooled, said),
        Err(error) => {
            return (
                spooled,
                vec![format!(
                    "COULD NOT read {} for verdicts a previous runner left: {error}. If one \
                     is in there, it is not being delivered",
                    directory.display()
                )],
            )
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        match read_one(&path) {
            Ok(one) => {
                said.push(format!(
                    "picked up the verdict of {} (attempt {}, {:?}) from {}: it was never \
                     acknowledged, and it is offered to the hub until it is",
                    one.job_id,
                    one.attempt,
                    one.outcome,
                    path.display()
                ));
                spooled.push(one);
            }
            Err(reason) => said.push(format!(
                "the spool file {} holds something this runner cannot deliver ({reason}); it \
                 is not offered to the hub. {}",
                path.display(),
                set_aside(&path, UNREADABLE)
            )),
        }
    }
    (spooled, said)
}

/// Read one spool file, or say what is wrong with it.
fn read_one(path: &Path) -> Result<Spooled, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("it cannot be read: {error}"))?;
    let frame: ToHub = herd_proto::from_frame(&text)
        .map_err(|error| format!("it is not a frame this runner understands: {error}"))?;
    let ToHub::JobFinished {
        job_id,
        attempt,
        outcome,
        ..
    } = frame.clone()
    else {
        return Err(
            "it is not a job_finished frame, which is the only thing that \
                    belongs in here"
                .to_owned(),
        );
    };
    Ok(Spooled {
        path: path.to_path_buf(),
        job_id,
        attempt,
        outcome,
        frame,
    })
}

/// Move a spool file out of the way under a name that says why, keeping its
/// bytes, and say where they went.
///
/// Never deleted: whatever is in there was written as somebody's verdict, and
/// a human reading this line has to be able to go and look at it. Two reasons
/// reach here — a file nobody can decode ([`UNREADABLE`]) and a verdict for a
/// state root that is gone ([`FOREIGN`]) — and they differ only in the name
/// the bytes are kept under.
fn set_aside(path: &Path, extension: &str) -> String {
    let mut aside = path.as_os_str().to_os_string();
    aside.push(".");
    aside.push(extension);
    let aside = PathBuf::from(aside);
    match fs::rename(path, &aside) {
        Ok(()) => format!("Its bytes are kept as {}", aside.display()),
        Err(error) => format!(
            "Its bytes could NOT even be moved aside ({error}), so they are still in {}",
            path.display()
        ),
    }
}

/// Write every unacknowledged verdict to the data root, fsynced, and say what
/// happened to each one.
///
/// The summary line comes first in what is returned and is written **last**:
/// saying "they are being written" before the write is a sentence that a
/// read-only directory makes false, and the next line then contradicts it.
pub fn write_all(root: &DataRoot, held: Vec<(JobId, JobOutcome, ToHub)>) -> Vec<String> {
    if held.is_empty() {
        return Vec::new();
    }
    let asked = held.len();
    let mut written = 0usize;
    let mut lines = Vec::new();
    for (job_id, outcome, frame) in held {
        // Whatever goes wrong, the outcome itself goes on stderr: a verdict
        // in a log a human can read beats a verdict nowhere.
        match (root.held_result_path(&job_id), to_frame(&frame)) {
            (Ok(path), Ok(text)) => match write_fsynced(&path, text.as_bytes()) {
                Ok(()) => {
                    written += 1;
                    lines.push(format!(
                        "the result of {job_id} is held in {} and has not reached a hub: \
                         {outcome:?}. The next runner on this data root delivers it",
                        path.display()
                    ));
                }
                Err(error) => lines.push(format!(
                    "COULD NOT write the unacknowledged result of {job_id} to {}: {error}. \
                     It ended with {outcome:?} and that is the only record of it",
                    path.display()
                )),
            },
            (path, frame) => lines.push(format!(
                "COULD NOT write the unacknowledged result of {job_id} ({}{}). It ended \
                 with {outcome:?} and that is the only record of it",
                path.err().unwrap_or_default(),
                frame.err().map(|e| e.to_string()).unwrap_or_default()
            )),
        }
    }
    let mut said = vec![if written == asked {
        format!(
            "{asked} result(s) were never acknowledged by a hub; they are NOT lost, they \
             are written under {}",
            root.held_results_dir().display()
        )
    } else {
        format!(
            "{asked} result(s) were never acknowledged by a hub, and only {written} of \
             them could be written under {}. Read the lines below: a verdict that is \
             not in a file is in none",
            root.held_results_dir().display()
        )
    }];
    said.extend(lines);
    said
}

/// Write down, under a name that says why, verdicts this runner is holding
/// for a hub state root that no longer exists.
///
/// Not offered to anybody: the hub that could have recorded them is gone, and
/// the hub that replaced it mints the same job ids for other jobs, so sending
/// one would put this verdict on another job's record. Not destroyed either —
/// a verdict is never destroyed (D19) — so the bytes land beside the spool
/// under [`FOREIGN`], where a human looking for what became of a job on a hub
/// that was wiped can find them.
pub fn write_all_for_a_foreign_state_root(
    root: &DataRoot,
    held: Vec<(JobId, JobOutcome, ToHub)>,
    why: &str,
) -> Vec<String> {
    let mut said = Vec::new();
    for (job_id, outcome, frame) in held {
        let written = match (root.held_result_path(&job_id), to_frame(&frame)) {
            (Ok(path), Ok(text)) => {
                let aside = PathBuf::from({
                    let mut name = path.into_os_string();
                    name.push(".");
                    name.push(FOREIGN);
                    name
                });
                match write_fsynced(&aside, text.as_bytes()) {
                    Ok(()) => format!("its bytes are kept as {}", aside.display()),
                    Err(error) => format!("and its bytes could NOT be kept: {error}"),
                }
            }
            (path, frame) => format!(
                "and its bytes could NOT be kept ({}{})",
                path.err().unwrap_or_default(),
                frame.err().map(|e| e.to_string()).unwrap_or_default()
            ),
        };
        said.push(format!(
            "the verdict of {job_id} ({outcome:?}) was produced for another hub's state root \
             ({why}); no hub can record it now, so it is NOT offered and this line is the \
             record of it — {written}"
        ));
    }
    said
}

fn write_fsynced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)?;
    fs::File::open(path).and_then(|file| file.sync_all())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use herd_proto::JobMetrics;

    fn verdict(job: &str, attempt: u32, code: i32) -> (JobId, JobOutcome, ToHub) {
        let job_id = JobId::new(job);
        let outcome = JobOutcome::Exited { code };
        (
            job_id.clone(),
            outcome.clone(),
            ToHub::JobFinished {
                job_id,
                attempt,
                outcome,
                chunks_sent: 3,
                metrics: JobMetrics::default(),
                steps: Vec::new(),
            },
        )
    }

    /// The round trip that D34 only did half of: written on the way out, read
    /// back on the way in, byte for byte the frame the runner would have sent.
    #[test]
    fn a_spooled_verdict_is_read_back_as_the_frame_it_was() {
        let temp = TempDir::new("spool-round-trip");
        let root = DataRoot::open(temp.path()).expect("a throwaway root");
        let (_, _, frame) = verdict("j0000000007", 2, 42);
        let said = write_all(&root, vec![verdict("j0000000007", 2, 42)]);
        assert!(
            said.iter().any(|line| line.contains("NOT lost")),
            "the summary has to be there: {said:?}"
        );

        let (spooled, picked) = pick_up(&root);
        assert_eq!(spooled.len(), 1, "{picked:?}");
        assert_eq!(
            spooled[0].frame(),
            &frame,
            "the frame must survive verbatim"
        );
        assert_eq!(spooled[0].attempt(), 2);
        assert!(
            picked.iter().any(|line| line.contains("picked up")),
            "picking a verdict up is not something to do quietly: {picked:?}"
        );
    }

    /// Acknowledged means the file goes, and only then.
    #[test]
    fn an_acknowledged_verdict_leaves_no_file_and_a_refused_one_leaves_none_either() {
        let temp = TempDir::new("spool-acknowledged");
        let root = DataRoot::open(temp.path()).expect("a throwaway root");
        write_all(&root, vec![verdict("j1", 1, 0)]);
        let (spooled, _) = pick_up(&root);
        assert!(
            spooled[0].path().exists(),
            "the file is there to start with"
        );
        let said = spooled[0].delivered();
        assert!(!spooled[0].path().exists(), "{said:?}");
        assert!(pick_up(&root).0.is_empty());

        write_all(&root, vec![verdict("j2", 1, 0)]);
        let (spooled, _) = pick_up(&root);
        let said = spooled[0]
            .refused("the job was already finished")
            .join("\n");
        assert!(said.contains("j2"), "{said}");
        assert!(said.contains("already finished"), "{said}");
        assert!(
            said.contains("last record of it"),
            "dropping a verdict is said in full or it is a verdict lost quietly: {said}"
        );
        assert!(!spooled[0].path().exists(), "{said}");
    }

    /// A file nobody can decode is kept, not deleted: it may be the only
    /// trace of a verdict on this host, and a human has to be able to look.
    #[test]
    fn a_file_that_cannot_be_decoded_is_set_aside_rather_than_offered_or_deleted() {
        let temp = TempDir::new("spool-unreadable");
        let root = DataRoot::open(temp.path()).expect("a throwaway root");
        let path = root.held_result_path(&JobId::new("j3")).expect("a path");
        fs::write(
            &path,
            b"{\"type\":\"health\",\"jobs\":[],\"draining\":false}",
        )
        .expect("the fixture writes");

        let (spooled, said) = pick_up(&root);
        assert!(spooled.is_empty(), "nothing undeliverable is offered");
        let said = said.join("\n");
        assert!(said.contains("cannot deliver"), "{said}");
        assert!(!path.exists(), "{said}");
        assert!(
            path.with_extension("json.unreadable").exists(),
            "the bytes must be kept: {said}"
        );
        // And a second start does not pick the set-aside file up again.
        assert!(pick_up(&root).0.is_empty());
    }

    /// Finding 3 of M4's verification: the sentence promising the write came
    /// out before the write was attempted, so a directory nobody can write
    /// made it false and the line after it said so.
    #[test]
    fn the_summary_never_promises_a_write_that_did_not_happen() {
        let temp = TempDir::new("spool-unwritable");
        let root = DataRoot::open(temp.path()).expect("a throwaway root");
        let directory = root.held_results_dir();
        fs::create_dir_all(&directory).expect("the fixture makes it");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o500))
                .expect("the fixture chmods");
        }
        let said = write_all(&root, vec![verdict("j4", 1, 7)]).join("\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .expect("the fixture puts it back");
        }
        assert!(
            said.contains("COULD NOT write"),
            "a write that failed has to say so: {said}"
        );
        assert!(
            !said.contains("they are NOT lost"),
            "the summary promised a write that did not happen: {said}"
        );
        assert!(
            said.contains("only 0 of them could be written"),
            "the summary must say how many really landed: {said}"
        );
    }
}
