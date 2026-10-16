//! A job's declared outputs, and the two carriages they come home by.
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
//! produced once, part of what the job *is*, and read next by whoever asked
//! for the job rather than by another machine. So they ride the same path,
//! into the `files/` directory D8 already reserved in every job's record.
//!
//! What separates a record from a pack is **who reads it next**, not how big
//! it is (D148). A pack is an input to a future job, keyed by (commit, key),
//! and it never touches the hub. A collection is evidence about one job,
//! keyed by that job's id, and it always does.
//!
//! ## Two carriages, and a requester cannot tell which
//!
//! [`INLINE_OUTPUT_MAX`] is no longer a refusal boundary. At or under it a
//! collection rides the verdict frame exactly as it always did; above it the
//! verdict carries the **manifest** — every path, its length, its sha256 —
//! and the bytes follow file by file in pieces of [`OUTPUT_CHUNK_BYTES`].
//!
//! The runner measures and chooses, because nobody knows in advance whether a
//! run leaves 1 MiB or 400 MiB, and a requester that had to declare which it
//! wanted would have to know. So **a requester declares nothing, and what it
//! ends up with is the same collection either way** — one `JobOutputs` whose
//! every entry carries its bytes. That is a property with a test behind it,
//! not an intention.
//!
//! Over the job's whole output cap there is still a **refusal with figures**
//! and no file at all — never a truncation, because a truncated
//! `results.json` that still parses reads like the truth.
//!
//! ## What this costs on the wire
//!
//! A frame each way for the bytes, and one for a requester to ask — which
//! moves [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) to 2 (D124, D128,
//! approved 2026-10-16). The inline carriage keeps D137's single round trip
//! untouched: its bytes are already on the verdict, so nothing is missing and
//! nothing is asked for.

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
/// The line between the two carriages a collection can travel by: **32 MiB**.
///
/// At or under this a collection rides the verdict frame, inline, exactly as
/// it did before streaming existed. Above it the verdict carries the manifest
/// alone and the bytes follow it file by file (D148). **A requester never
/// declares which it wants and cannot tell which it got**, because nobody
/// knows in advance whether a run leaves 1 MiB or 400 MiB; the runner
/// measures the collection when the job ends and chooses.
///
/// **The figure is unchanged and derived, and only its meaning moved.** It
/// was D131's refusal boundary — over it, no file came back at all — and it
/// is now the boundary between two ways of carrying one. The arithmetic is
/// the same arithmetic, because it is still what one frame can hold: a
/// collection rides inside one JSON frame, base64-encoded, so a payload of
/// `n` bytes becomes about `4n/3` on the wire plus the paths and the verdict
/// around it. Against [`FRAME_CEILING`] that puts the bound at 48 MiB; 32 MiB
/// is that bound with room to spare, measured rather than assumed — 32 MiB of
/// payload in 200 files encodes to a 42 MiB frame, leaving 22 MiB of the
/// ceiling for the paths, the steps and the metrics beside it.
///
/// **And the frame was never the only reason.** At 32 MiB a frame costs about
/// 140 MiB of peak memory and a few hundred milliseconds to encode and decode
/// on each of the three parties. Those curves are linear, so a few hundred
/// megabytes on one frame would cost over a gigabyte per party and tens of
/// seconds of blocked event loop — past the 15 s cadence a quiet link and a
/// health report both live by ([`crate::HEALTH_EVERY`]), which would make a
/// large output indistinguishable from a lost runner. That objection is
/// exactly what the streamed carriage answers, in pieces of
/// [`OUTPUT_CHUNK_BYTES`], and it is why the threshold stays where the frame
/// put it rather than being raised now that there is somewhere else to go.
pub const INLINE_OUTPUT_MAX: u64 = 32 << 20;

/// How much of one output file one [`crate::ToHub::JobFileChunk`] or
/// [`crate::FromHub::JobFile`] carries: **1 MiB**.
///
/// **Derived from the event loop, not from the ceiling.** 1 MiB base64's to
/// about 1.4 MiB, which is a fortieth of [`FRAME_CEILING`] — so the ceiling
/// is not what chose it. What chose it is D138's real objection to a big
/// frame: every frame costs its whole encode and decode before the party
/// doing it serves anything else, and a 400 MiB collection streams while its
/// runner must go on reporting health inside 15 s
/// ([`crate::HEALTH_EVERY`]) and its follower's link must go on breathing
/// inside 15 s ([`crate::STILL_THERE_EVERY`]). At 1 MiB a frame is single-
/// digit milliseconds of work and a 400 MiB collection is four hundred such
/// pauses, none of which any cadence can notice; at 32 MiB it would be a few
/// hundred milliseconds a frame, which is inside the cadence by one order of
/// magnitude instead of three.
///
/// The cost of the choice is the round-trip count — four hundred frames for
/// 400 MiB — and it is paid on a link that is already carrying the bytes
/// those frames hold.
pub const OUTPUT_CHUNK_BYTES: u64 = 1 << 20;

/// What one file costs the inline threshold on top of its own bytes and its
/// own path.
///
/// The threshold has to bound the **frame**, not the payload: a job writing a
/// million empty files would weigh nothing by its bytes and still make a JSON
/// object no byte count had anything to say about, well past the hub's own
/// frame ceiling — a frame that does not arrive at all, which is the one
/// failure a bound exists to turn into a sentence. So each file is charged
/// its data, its path and this, and the charge is what decides the carriage.
///
/// 128 bytes is above the JSON an entry really costs — the keys, the quoted
/// strings, the braces and the commas — so the charge is never an
/// underestimate of the frame being bounded. A manifest entry now also
/// carries a length and a 64-character digest, which is inside this.
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
    /// How long the file is, off the manifest rather than off the bytes.
    ///
    /// **It is on the wire even when the bytes are**, and the redundancy is
    /// the point: it is what the streamed carriage resumes against and what
    /// the hub checks a finished file's size with, so one field answers "how
    /// far through is this" and "is this all of it" for both carriages. A
    /// length that disagrees with the bytes beside it is a frame refused by
    /// name, never a file written to whichever of the two the reader
    /// happened to use.
    #[serde(default)]
    pub len: u64,
    /// The sha256 of the file, lowercase hex, computed by
    /// [`sha256_hex`](crate::sha256_hex).
    ///
    /// **Not decoration.** It is what makes a resumed transfer safe — without
    /// it, two reads of a path that changed between them splice into a file
    /// that is a plausible mixture of both — and it is what lets a requester
    /// skip what it already holds instead of fetching a collection twice. It
    /// also outlives the bytes: a record keeps it after retention has removed
    /// the file, so a judge re-reading an old result knows what it was
    /// looking for (E-54).
    #[serde(default)]
    pub sha256: String,
    /// The bytes of the file, unaltered, carried as base64 inside the JSON
    /// frame for the reason a log chunk is (D18): a job's output is not text,
    /// and a `results.json` rewritten through U+FFFD is a `results.json` that
    /// is no longer the one the job wrote.
    ///
    /// `None` means **the bytes are not on this frame**, which is three
    /// different facts and never "the file is empty": a collection travelling
    /// by the streamed carriage, where they follow the verdict; a manifest
    /// read back off a record whose bytes the caller did not ask for; or a
    /// file whose bytes are gone. An empty file is `Some(vec![])` with a
    /// `len` of 0, and the two must not be confused — which is why this is an
    /// `Option` and not an empty vector.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::opt_bytes_as_base64"
    )]
    pub data: Option<Vec<u8>>,
}

impl OutputFile {
    /// One file whose bytes are in hand: the manifest is computed from them,
    /// so a length or a digest can never disagree with the data beside it.
    pub fn collected(path: String, data: Vec<u8>) -> Self {
        Self {
            len: data.len() as u64,
            sha256: crate::sha256_hex(&data),
            path,
            data: Some(data),
        }
    }

    /// One file announced by its manifest entry, its bytes elsewhere.
    pub fn announced(path: String, len: u64, sha256: String) -> Self {
        Self {
            path,
            len,
            sha256,
            data: None,
        }
    }

    /// The bytes, or an empty slice when this entry is a manifest entry only.
    pub fn bytes(&self) -> &[u8] {
        self.data.as_deref().unwrap_or(&[])
    }

    /// Whether the bytes travelled with the manifest.
    pub fn is_held(&self) -> bool {
        self.data.is_some()
    }

    /// What this file costs the inline threshold: its bytes, its path, and
    /// [`PER_FILE_OVERHEAD`].
    ///
    /// Charged off `len` rather than off `data`, so the figure is the same
    /// before the bytes are read as after — which is what lets the runner
    /// choose a carriage from a walk that has only measured.
    pub fn weight(&self) -> u64 {
        self.len + self.path.len() as u64 + PER_FILE_OVERHEAD
    }

    /// Why the bytes on this frame are not the file the manifest describes,
    /// if they are not.
    ///
    /// The one check, made wherever a frame's bytes are about to be believed.
    /// A length and a digest that travel beside the data they describe are
    /// worth nothing unless somebody compares them, and the comparison has to
    /// live next to the fields so it cannot be the step a caller forgot.
    pub fn disagreement(&self) -> Option<String> {
        let Some(data) = &self.data else {
            return None;
        };
        if data.len() as u64 != self.len {
            return Some(format!(
                "{}: the frame says {} byte(s) and carries {}",
                self.path,
                self.len,
                data.len()
            ));
        }
        let real = crate::sha256_hex(data);
        if real != self.sha256 {
            return Some(format!(
                "{}: the frame says sha256 {} and carries {real}",
                self.path, self.sha256
            ));
        }
        None
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
    /// Which carriage the bytes travel by: `false` on the verdict frame,
    /// `true` file by file after it (D148).
    ///
    /// **This is the one line that says which, and a requester is not
    /// supposed to read it.** It is here so an operator looking at a record
    /// can tell, and so the hub knows whether to expect chunks for a job it
    /// has just written a verdict for. What a requester gets is the same
    /// collection either way — that property is asserted, not hoped for —
    /// so a caller branching on this is a caller doing something the design
    /// says it should not have to.
    ///
    /// Absent means `false`, which is what every collection that predates
    /// streaming was.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streamed: bool,
}

impl JobOutputs {
    /// Whether there is nothing here to say, and therefore nothing to put on
    /// the wire: what keeps every frame of every job that leaves no output
    /// byte-identical to the frame it was before this existed.
    pub fn is_silent(&self) -> bool {
        self.files.is_empty() && self.refused.is_none() && self.skipped.is_empty()
    }

    /// What the collection costs the inline threshold, all of it.
    pub fn weight(&self) -> u64 {
        self.files.iter().map(OutputFile::weight).sum()
    }

    /// Whether this collection is small enough to ride the verdict frame.
    ///
    /// The whole of the carriage decision, in one place, so the runner that
    /// chooses and any test that checks the choice are reading one function
    /// (invariant 4).
    pub fn fits_inline(&self) -> bool {
        self.weight() <= INLINE_OUTPUT_MAX
    }

    /// The first manifest entry whose bytes contradict it, if any.
    pub fn disagreement(&self) -> Option<String> {
        self.files.iter().find_map(OutputFile::disagreement)
    }

    /// A collection that is nothing but a refusal, with whatever was skipped
    /// along the way kept: the figures are the point, and what was skipped is
    /// part of why the figures are what they are.
    pub fn refusing(reason: String, skipped: Vec<String>) -> Self {
        Self {
            files: Vec::new(),
            refused: Some(reason),
            skipped,
            streamed: false,
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
        let file = OutputFile::collected("files/results.json".to_owned(), vec![b'x'; 10]);
        assert_eq!(file.weight(), 10 + 18 + PER_FILE_OVERHEAD);
        // An empty file is not free: that is what stops a million of them
        // making a frame no cap bounded.
        let empty = OutputFile::collected("e".to_owned(), Vec::new());
        assert_eq!(empty.weight(), 1 + PER_FILE_OVERHEAD);
        // And an entry announced but not carried is charged the same, which
        // is what lets the runner choose a carriage off a walk that has only
        // measured.
        let announced = OutputFile::announced("e".to_owned(), 0, empty.sha256.clone());
        assert_eq!(announced.weight(), empty.weight());
    }

    /// An empty file and a file whose bytes are elsewhere are not the same
    /// thing, and the wire has to keep them apart.
    ///
    /// The mistake this guards is a quiet one: a codec that wrote `None` as
    /// an empty string would turn "the bytes are streaming" into "the file is
    /// empty", and an empty `results.json` parses — which is exactly the
    /// class of wrong answer D131 refused a truncation for.
    #[test]
    fn a_file_with_no_bytes_here_is_not_a_file_with_no_bytes() {
        let empty = OutputFile::collected("e".to_owned(), Vec::new());
        let elsewhere = OutputFile::announced("e".to_owned(), 0, empty.sha256.clone());
        assert!(empty.is_held() && !elsewhere.is_held());
        let there = crate::to_frame(&empty).expect("an empty file encodes");
        let not_here = crate::to_frame(&elsewhere).expect("an announced file encodes");
        assert_ne!(there, not_here);
        assert_eq!(
            crate::from_frame::<OutputFile>(&there).expect("it decodes"),
            empty
        );
        assert_eq!(
            crate::from_frame::<OutputFile>(&not_here).expect("it decodes"),
            elsewhere
        );
    }

    /// Bytes that contradict the manifest beside them are named, not written.
    #[test]
    fn bytes_that_disagree_with_their_manifest_entry_are_named() {
        let mut wrong_length = OutputFile::collected("r".to_owned(), vec![1, 2, 3]);
        wrong_length.len = 4;
        let why = wrong_length
            .disagreement()
            .expect("a length that disagrees is a disagreement");
        assert!(why.contains('4') && why.contains('3'), "{why}");

        let mut wrong_digest = OutputFile::collected("r".to_owned(), vec![1, 2, 3]);
        wrong_digest.sha256 = "0".repeat(64);
        let why = wrong_digest
            .disagreement()
            .expect("a digest that disagrees is a disagreement");
        assert!(why.contains("sha256"), "{why}");

        // And an entry carrying no bytes cannot disagree with them: there is
        // nothing to compare, and calling that a disagreement would refuse
        // every manifest the streamed carriage sends.
        assert!(OutputFile::announced("r".to_owned(), 3, "x".to_owned())
            .disagreement()
            .is_none());
    }

    /// The cap really does fit a frame, encoded rather than reasoned about.
    ///
    /// This is the assertion [`INLINE_OUTPUT_MAX`] exists for and the one a
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
        let each = (INLINE_OUTPUT_MAX as usize - count * charged) / count;
        let outputs = JobOutputs {
            files: (0..count)
                .map(|n| OutputFile::collected(path_of(n), vec![0xa5; each]))
                .collect(),
            ..JobOutputs::default()
        };
        assert!(
            outputs.weight() <= INLINE_OUTPUT_MAX,
            "the fixture must be a collection the cap would accept: {} over {INLINE_OUTPUT_MAX}",
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
