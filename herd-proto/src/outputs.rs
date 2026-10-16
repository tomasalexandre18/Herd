//! A job's declared outputs: the small files that travel with its verdict.
//!
//! Herd returned an exit code and a log, and nothing else. A job's `{out}`
//! directory was handed to its steps and reclaimed with the workspace the
//! moment the job ended, so the first consumer's whole result contract — a
//! `results.json` and the files beside it — never left the machine that
//! produced it.
//!
//! ## Why this is not the byte path D10 removed
//!
//! D10 takes Herd out of the byte path **between runners**: a pack must not
//! be relayed hub-wards so that another machine can have it, because the hub
//! would become a disk sink and a bandwidth bottleneck for content it has no
//! use for. It says nothing about a job's own **record**, and Herd already
//! carries the log runner → hub → requester as part of that record (D8, D18,
//! E-15). A job's declared outputs are the same kind of thing as its log:
//! small, produced once, part of what the job *is*, and useless to anyone but
//! whoever asked for the job. So they ride the same path, under a cap, into
//! the `files/` directory D8 already reserved in every job's record.
//!
//! The line between the two is the cap and nothing else. Over it there is a
//! **refusal with figures** and no file at all — never a truncation, because
//! a truncated `results.json` that still parses reads like the truth — and
//! the refusal says that large artefacts are M11's direct transfer, not this.
//!
//! ## Additive on the wire, deliberately
//!
//! [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) is frozen at 1 and a hub is
//! deployed, so this adds no frame: one `#[serde(default)]` field on the two
//! `job_finished` frames, whose absence means exactly what its default says —
//! this peer collected nothing. That is the one change D128 leaves the number
//! alone for, and it is why there is no `fetch_outputs` frame and no third
//! round trip anywhere.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The largest frame this protocol carries, in bytes.
///
/// The hub sets this on every socket it accepts (`herd_hub`'s
/// `FRAME_CEILING_BYTES`), and a runner and a `herdctl` set it on the socket
/// they dial, so the figure is Herd's own rather than whichever default the
/// WebSocket library happens to ship. It lives here because it is a property
/// of the wire, and because the output cap below is **derived** from it: a
/// number that may be set no higher than what can arrive has to be computed
/// from what can arrive, not guessed beside it.
///
/// A test in `herd-hub` asserts the two are the same number, so they cannot
/// drift.
pub const FRAME_CEILING: u64 = 64 << 20;

/// The largest an output cap may be set to, and the ceiling the default below
/// sits at.
///
/// **Derived, with the arithmetic in the open.** A collection travels inside
/// one JSON frame, base64-encoded, so a payload of `n` bytes becomes about
/// `4n/3` on the wire plus the paths and the verdict around it. Against
/// [`FRAME_CEILING`] that puts the arithmetic bound at 48 MiB; 32 MiB is the
/// bound with room to spare, measured rather than assumed — 32 MiB of payload
/// in 200 files encodes to a 42 MiB frame, which leaves 22 MiB of the ceiling
/// unused for the paths, the steps and the metrics beside it.
///
/// **And the frame is not the only reason it is here.** At 32 MiB a frame
/// costs about 140 MiB of peak memory and a few hundred milliseconds to
/// encode and decode on each of the three parties. Those curves are linear,
/// so a few hundred megabytes would cost over a gigabyte per party and tens
/// of seconds of blocked event loop — past the 15 s cadence a quiet link and
/// a health report both depend on ([`crate::HEALTH_EVERY`]), which would make
/// a large output look exactly like a lost runner. **That is the boundary**:
/// at this size a job's output travels like its log, and beyond it the same
/// mechanism would be artefact distribution wearing the record's clothes,
/// which is M11's direct runner-to-runner transfer and not this (D10).
///
/// A configuration above this is refused at startup, naming both figures,
/// rather than producing a frame nobody can read.
pub const MAX_OUTPUT_CAP: u64 = 32 << 20;

/// What one file costs the cap on top of its own bytes and its own path.
///
/// The cap has to bound the **frame**, not the payload: a job writing a
/// million empty files would weigh nothing by its bytes and still make a JSON
/// object no byte cap had anything to say about, well past the hub's own
/// frame ceiling — a frame that does not arrive at all, which is the one
/// failure a cap exists to turn into a sentence. So each file is charged its
/// data, its path and this, and one flag then bounds the whole collection as
/// it travels.
///
/// 128 bytes is above the JSON an entry really costs — two keys, two quoted
/// strings, the braces and the commas — so the charge is never an
/// underestimate of the frame being bounded.
pub const PER_FILE_OVERHEAD: u64 = 128;

/// One file a job left under its `{out}` directory, on its way to the record.
///
/// `path` is **relative to `{out}`**, with its directories preserved:
/// `files/results.json` comes back as `files/results.json` and never
/// flattened to `results.json`, because two files of that name under two
/// directories are two files and a consumer reads them by their paths.
///
/// ## Keyed by job id, not by the declaration's `leaves` names
///
/// The first consumer asked whether this is "a directory fetched by job id,
/// or keyed by the `leaves` names". It is the id, with the paths intact, for
/// three reasons that are all about who knows what:
///
/// * A requester that attaches with `herdctl wait <id>` has **the id and
///   nothing else**. It never saw the declaration — it may be another machine
///   or a script an hour later — so the id is the only key that always works.
/// * `leaves` is a *declaration* concern. Keying the transport on declared
///   names would make the shape of the record depend on a file the hub does
///   not own and never reads; D74 already settled that the hub works from
///   what is on the record, whoever put it there.
/// * Content-addressed, `leaves`-keyed retrieval is precisely M11. Keeping
///   this path keyed on the id is what stops the two blurring into one
///   half-built artefact store.
///
/// So `leaves` goes on meaning what it means in the declaration, and this
/// mechanism brings the `{out}` directory home intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    pub path: String,
    /// The bytes of the file, unaltered, carried as base64 inside the JSON
    /// frame for the reason a log chunk is (D18): a job's output is not text,
    /// and a `results.json` rewritten through U+FFFD is a `results.json` that
    /// is no longer the one the job wrote.
    #[serde(with = "crate::bytes_as_base64")]
    pub data: Vec<u8>,
}

impl OutputFile {
    /// What this file costs the cap: its bytes, its path, and
    /// [`PER_FILE_OVERHEAD`].
    pub fn weight(&self) -> u64 {
        self.data.len() as u64 + self.path.len() as u64 + PER_FILE_OVERHEAD
    }

    /// Where this file goes under a directory, or why it goes nowhere.
    ///
    /// The one check, used by the runner that produces the path and by the
    /// hub that writes the file, so a path cannot be judged acceptable at one
    /// end and dangerous at the other (invariant 4). A collection walk cannot
    /// produce `..` — but a frame is not a walk, and the hub must never join
    /// a string a peer sent onto its own state root without asking this.
    pub fn relative_path(&self) -> Result<PathBuf, String> {
        check_relative(&self.path).map(|()| PathBuf::from(&self.path))
    }
}

/// Why this is not a path that may be joined onto a directory, if it is not.
///
/// Normal components only: no root, no prefix, no `.`, and above all no `..`.
/// A single rejected shape would be enough to write a file outside the job's
/// own directory in the record.
pub fn check_relative(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("an output file with no name at all".to_owned());
    }
    let mut components = 0usize;
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) => components += 1,
            Component::ParentDir => {
                return Err(format!(
                    "{path:?} climbs out of the job's output directory with `..`"
                ))
            }
            Component::CurDir => {
                return Err(format!("{path:?} holds a `.` component"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("{path:?} is an absolute path, not a relative one"));
            }
        }
    }
    if components == 0 {
        return Err(format!("{path:?} names no file"));
    }
    Ok(())
}

/// What a job left behind, as its verdict carries it.
///
/// Three fields rather than an enum, because the three say different things
/// and a reader needs all of them at once: what came back, why nothing came
/// back, and what was there and was deliberately not taken.
///
/// [`Default`] — no file, no refusal, nothing skipped — is what an ad-hoc job
/// produces, what a job the hub ended without ever placing produces, and what
/// an older peer's frame means when the field is absent. All three are the
/// same statement: this job's record holds no output file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobOutputs {
    /// The files, in the order the collection walked them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<OutputFile>,
    /// Why no file came back although the job left some: the cap, with the
    /// figures (E-04's style — name the limit and the measurement, never just
    /// the limit).
    ///
    /// **Never set together with a short list of `files`.** Over the cap is
    /// all or nothing: a caller that found both would have a partial result
    /// set that reads like a whole one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// One sentence per thing under `{out}` that is not a regular file, or
    /// that could not be read. A symlink that was not followed is in here,
    /// and so is a file whose name is not UTF-8.
    ///
    /// Kept because "`results.json` did not come back" and "`results.json` is
    /// a symlink and a symlink is never followed" are different facts, and a
    /// consumer debugging the first one has no way to find the second.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

impl JobOutputs {
    /// Whether there is nothing here to say, and therefore nothing to put on
    /// the wire: what keeps every frame of every job that leaves no output
    /// byte-identical to the frame it was before this existed.
    pub fn is_silent(&self) -> bool {
        self.files.is_empty() && self.refused.is_none() && self.skipped.is_empty()
    }

    /// What the collection costs the cap, all of it.
    pub fn weight(&self) -> u64 {
        self.files.iter().map(OutputFile::weight).sum()
    }

    /// A collection that is nothing but a refusal, with whatever was skipped
    /// along the way kept: the figures are the point, and what was skipped is
    /// part of why the figures are what they are.
    pub fn refusing(reason: String, skipped: Vec<String>) -> Self {
        Self {
            files: Vec::new(),
            refused: Some(reason),
            skipped,
        }
    }

    /// How many files, for the one line a requester is told.
    pub fn count(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_keeps_its_directories_and_an_escaping_one_is_refused() {
        assert_eq!(check_relative("files/results.json"), Ok(()));
        assert_eq!(check_relative("results.json"), Ok(()));
        assert_eq!(check_relative("a/b/c/d.txt"), Ok(()));
        for bad in [
            "../results.json",
            "files/../../results.json",
            "/etc/passwd",
            "./results.json",
            "",
        ] {
            let why = check_relative(bad)
                .expect_err("a path that is not plainly relative must be refused");
            assert!(!why.is_empty(), "the refusal has to say something: {bad:?}");
        }
        // And the refusal names the path, because the hub prints it.
        let why = check_relative("../out").expect_err("refused");
        assert!(why.contains("../out"), "{why}");
        assert!(why.contains(".."), "{why}");
    }

    #[test]
    fn a_file_is_charged_its_bytes_its_path_and_the_per_file_overhead() {
        let file = OutputFile {
            path: "files/results.json".to_owned(),
            data: vec![b'x'; 10],
        };
        assert_eq!(file.weight(), 10 + 18 + PER_FILE_OVERHEAD);
        // An empty file is not free: that is what stops a million of them
        // making a frame no cap bounded.
        let empty = OutputFile {
            path: "e".to_owned(),
            data: Vec::new(),
        };
        assert_eq!(empty.weight(), 1 + PER_FILE_OVERHEAD);
    }

    /// The cap really does fit a frame, encoded rather than reasoned about.
    ///
    /// This is the assertion [`MAX_OUTPUT_CAP`] exists for and the one a
    /// comment cannot make: a collection at exactly the cap is built, put
    /// through the real encoder, and the frame it produces is measured
    /// against [`FRAME_CEILING`]. Base64's four-thirds is arithmetic anyone
    /// can do wrong; this cannot be wrong, and it is what fails the day
    /// somebody raises the cap or adds a field to the frame.
    ///
    /// It costs a few seconds, which is the whole point: the alternative is
    /// discovering it as an undecodable verdict on a deployed hub (D128).
    #[test]
    fn a_collection_at_the_cap_still_fits_one_frame() {
        // Many files rather than one, because the paths and the per-file
        // JSON are part of what has to fit, and because that is the shape
        // the first consumer's per-scenario output has.
        let count = 200usize;
        let path_of = |n: usize| format!("files/scenario-{n:04}/screenshot.png");
        let charged = PER_FILE_OVERHEAD as usize + path_of(0).len();
        let each = (MAX_OUTPUT_CAP as usize - count * charged) / count;
        let outputs = JobOutputs {
            files: (0..count)
                .map(|n| OutputFile {
                    path: path_of(n),
                    data: vec![0xa5; each],
                })
                .collect(),
            refused: None,
            skipped: Vec::new(),
        };
        assert!(
            outputs.weight() <= MAX_OUTPUT_CAP,
            "the fixture must be a collection the cap would accept: {} over {MAX_OUTPUT_CAP}",
            outputs.weight()
        );
        let frame = crate::to_frame(&crate::ToHub::JobFinished {
            job_id: crate::JobId::new("j0000000001"),
            attempt: 1,
            outcome: crate::JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: crate::JobMetrics::default(),
            steps: Vec::new(),
            outputs,
        })
        .expect("a verdict carrying the largest allowed collection encodes");
        assert!(
            (frame.len() as u64) < FRAME_CEILING,
            "a collection at the cap makes a {} byte frame, past the {FRAME_CEILING} byte \
             ceiling: either the cap comes down or the ceiling goes up, but a frame nobody \
             can read is not an option",
            frame.len()
        );
    }

    #[test]
    fn nothing_collected_is_silent_and_a_refusal_is_not() {
        assert!(JobOutputs::default().is_silent());
        assert!(!JobOutputs::refusing("too big".to_owned(), Vec::new()).is_silent());
        assert!(!JobOutputs {
            skipped: vec!["a symlink".to_owned()],
            ..JobOutputs::default()
        }
        .is_silent());
    }
}
