//! The requester side: submit a job, follow its log, learn how it ended.
//!
//! A requester never names a machine (invariant 6): it says what to run and
//! the hub decides where. The surface is `run` to submit and follow, `submit`
//! to fire and leave with the id, `wait` to come back to an id later, and
//! `cancel` to ask for a job to be ended wherever it is running (E-06).

pub mod checkout;
pub mod declaration;
pub mod moved;
pub mod verdict;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    cap_stop_reason, from_frame, to_frame, ApprovedRunner, Asked, Cap, CapabilityOffer,
    Declaration, FromHub, JobId, JobMetrics, JobOutcome, LabelSet, LogStream, Priority, Role,
    RunnerSwitch, Switch, ToHub, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Where herdctl looks for the hub unless told otherwise.
pub const DEFAULT_HUB_URL: &str = "ws://127.0.0.1:8787";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What a requester needs to reach a hub.
#[derive(Debug, Clone)]
pub struct Config {
    pub hub_url: String,
    /// Bearer token. Carried from M1, checked by the hub from M15.
    pub token: Option<String>,
    /// Name reported to the hub, for its logs and reports.
    pub name: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hub_url: DEFAULT_HUB_URL.to_owned(),
            token: None,
            name: None,
        }
    }
}

/// How a followed job ended, which job it was, and what the runner measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub job_id: JobId,
    pub outcome: JobOutcome,
    pub metrics: JobMetrics,
    /// What the job left under its `{out}` directory, as its record holds it
    /// (D131).
    ///
    /// Carried here and written nowhere by default: E-02's stream is the
    /// `ran on:` line, the job's own bytes, then the verdict, and an output
    /// file on stdout would be read by the consumer's tooling as the job's
    /// output. `herdctl run --outputs-to <dir>` is what puts them on a disk,
    /// and it says on **stderr** which files it wrote.
    pub outputs: herd_proto::JobOutputs,
    /// Where it ran and why there, as the hub said (E-02, D109, D166).
    ///
    /// Kept beside the outcome because the machine-readable verdict carries
    /// it, and because a `ran on:` line that has been printed on stdout is a
    /// line nothing can read back: it is the job's own output from the moment
    /// it is written.
    ///
    /// **Recorded even for an ending the printed line is withheld for.** The
    /// stdout line frames a job's *output*, so a job that never started gets
    /// none (D16); this field is a record, and a machine that failed to start
    /// a job is exactly the machine somebody needs named. `None` means the
    /// hub never placed the job at all — it was refused, or it was impossible.
    pub ran_on: Option<RanOn>,
    /// What the attempt said about the revision it really ran, beside the
    /// commit this job declared, as the hub holds it (D176).
    ///
    /// `None` is "nobody said", from any of its three causes — an ad-hoc job,
    /// a job no runner reached, a hub that predates the field — and from none
    /// of them does it follow that the tree was at the declared commit.
    pub commit: Option<herd_proto::CommitRun>,
}

/// The two parts of E-02's framing line, kept rather than only printed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RanOn {
    /// The runner's own name — grievances 10a and 10c, which asked for this
    /// and nothing more.
    pub runner: String,
    /// Why there, in the hub's own words: the sentence the placement line
    /// carries inside its parentheses.
    pub why: String,
}

/// `herdctl`'s exit code when there is no verdict to report: Herd could not
/// run the job, lost it before it finished, refused it, or could not be
/// reached at all.
///
/// 125 is the convention of a tool that wraps another one (`env`, `timeout`):
/// the wrapper failed, not the command. It is one code for one meaning —
/// *Herd cannot tell you whether this job passed* — because a script that
/// cannot tell "the tests failed" from "the hub was down" will read a Herd
/// failure as a verdict, which is the one mistake E-02 and E-43 cannot
/// survive (D20). The job's own code is worn only when there is a real
/// verdict. A job could exit 125 by itself, which is why the reason is also
/// printed on stderr, and why the wire carries a typed outcome rather than a
/// number.
pub const NO_VERDICT_CODE: u8 = 125;

/// The stable prefix of the one stderr line that names a job's id.
///
/// **The whole of the fallback contract is this line's presence.** 125 says
/// "Herd has no verdict", and it says it both for a job that exists and ran
/// somewhere and for a submission that never left — which are opposite
/// instructions to a requester that falls back. One would have it decline to
/// re-run; the other would have it re-run safely. A caller that cannot tell
/// them apart runs the work twice, and work run twice is a wrong answer for
/// a build and test system, not a slow one.
///
/// So: **whenever a job exists, `herdctl` names its id on this line, on
/// stderr, as soon as the hub has said it** — before anything can go wrong,
/// because a line printed only on the failures that were thought of cannot
/// cover the one that was not. The rule a wrapper writes is "did I get an
/// id?", and the answer is worth more than a code would be: an id is a
/// handle, so the fallback is `herdctl wait <id>` rather than merely
/// declining to duplicate.
///
/// It is on **stderr**, which the job's own bytes never share (E-02's
/// framing — `ran on: …`, the job's output, the verdict — is all stdout), so
/// it cannot collide with a job's output and does not disturb that stream's
/// order.
pub const JOB_ID_PREFIX: &str = "herdctl: job-id: ";

/// The other half of the contract: the one stderr line printed when nothing
/// was submitted, so no job of this requester's exists.
///
/// A refusal before the job was minted, a bad token, an unreachable hub, a
/// usage error. These are the cases where running the work again is safe,
/// and a falling-back requester needs that certainty more than anything
/// else here.
pub const NO_JOB_LINE: &str =
    "herdctl: no-job: nothing was submitted, so no job of yours exists and running this again \
     starts no second one";

/// The third state, and it is real: the submission left and was never
/// acknowledged, so a job may exist whose id nobody learned.
///
/// Said rather than hidden, because the honest answer to "is there a job?"
/// here is "possibly", and a line claiming no job exists would be the
/// duplicate-work defect wearing the other mask. What makes it safe to act
/// on is E-05: submitting the same work again **joins** the open job rather
/// than starting a second one, so a requester that retries after this line
/// duplicates nothing.
pub const JOB_UNKNOWN_LINE: &str =
    "herdctl: job-unknown: the submission left but was never acknowledged, so a job may exist \
     whose id this requester never learned; submitting the same job again joins that one \
     rather than starting a second (E-05)";

/// The one stderr line that names a job's id, built in one place so that a
/// test and a wrapper read the same thing. See [`JOB_ID_PREFIX`].
pub fn job_id_line(job_id: &JobId) -> String {
    format!("{JOB_ID_PREFIX}{job_id}")
}

/// `herdctl`'s exit code when a runner stopped the job at one of its caps
/// (M4): the time cap, the memory ceiling, the log cap or the disk budget.
/// 124 is `timeout(1)`'s code for exactly this, generalised to the other
/// three because they are the same event — the machine said no, so the job
/// has no verdict of its own. Which cap it was travels in the outcome and is
/// printed on stderr; 124 alone is never meant to say.
pub const STOPPED_BY_CAP_CODE: u8 = 124;

/// What `herdctl` exits with for a job that ended this way.
///
/// An exit code is worn untouched — 0, 1 and 75 included, as E-02 needs. A
/// signal becomes `128 + signal`, exactly as a shell reports one, so no
/// sentinel value is spent on it.
///
/// A job that was cancelled, and one whose runner was lost, are both "Herd
/// cannot give you a verdict" and so both 125 (D20): nobody ran the tests to
/// the end, and the one mistake a caller must not be able to make is reading
/// either of those as a failing build.
pub fn exit_code_for(outcome: &JobOutcome) -> u8 {
    match outcome {
        // A real process code is 0..=255. Anything else came from a peer
        // inventing one, and cannot be worn — so this is Herd with no verdict
        // to give, exactly like the cases below, and never 1: 1 is the code
        // that means "the job failed" (D20), and a lying peer must not be able
        // to make a caller read a failure that nothing established.
        JobOutcome::Exited { code } => u8::try_from(*code).unwrap_or(NO_VERDICT_CODE),
        JobOutcome::Signalled { signal } => {
            128u8.saturating_add(u8::try_from(*signal).unwrap_or(127))
        }
        JobOutcome::CouldNotStart { .. }
        | JobOutcome::Cancelled { .. }
        | JobOutcome::Impossible { .. }
        | JobOutcome::Lost { .. }
        // 125 like the rest of them, and for the plainest reason in the
        // list: nobody ran this job at all. Not 124, which would say a
        // budget stopped it; not 1, which would say it failed. Which of the
        // five it was is `cause` in the verdict document and the sentence on
        // stderr, never the number (D182, D20).
        | JobOutcome::RefusedAsFull { .. }
        // 125 and **not** 124, which is the whole of D166's third part: the
        // machine the job ran on could not do its own part, so nobody
        // established anything about this job. 124 would say a budget killed
        // it, which is a verdict about the submission and was never true.
        | JobOutcome::SetupFailed { .. } => NO_VERDICT_CODE,
        JobOutcome::StoppedByCap { .. } => STOPPED_BY_CAP_CODE,
    }
}

/// Why there is no verdict, when the job's end is one of the ends that is not
/// a verdict. `None` for an end that is one.
///
/// The sentence goes on stderr beside the 125, so that a caller reading a
/// terminal can tell a cancelled job from a lost one from a build that never
/// started — the exit code cannot, and is not asked to.
pub fn no_verdict_reason(outcome: &JobOutcome) -> Option<String> {
    match outcome {
        JobOutcome::Cancelled { reason } => Some(format!("this job was cancelled: {reason}")),
        JobOutcome::Lost { reason } => {
            Some(format!("the runner executing this job was lost: {reason}"))
        }
        // Not "no machine is free": no machine could ever be. The sentence
        // says which requirement made it so, because the only thing to do
        // about one of these is change the submission (E-36).
        JobOutcome::Impossible { reason } => Some(format!(
            "no runner could ever run this job, so it was not queued for one: {reason}"
        )),
        // And the one that is the opposite of that: every machine it was
        // offered to could have run it and none of them had room. The
        // sentence names the machines and the bound, because adding capacity
        // — or finding out why the hub's count and a runner's disagree — is
        // the whole of what there is to do about it (D182).
        JobOutcome::RefusedAsFull {
            reason,
            refusals,
            refusals_allowed,
            runners,
        } => Some(format!(
            "no machine had room for this job: it was handed straight back {refusals} time(s) \
             of the {refusals_allowed} this hub allows, by {}. Nothing ran and nothing failed. \
             {reason}",
            if runners.is_empty() {
                "machines that were already full".to_owned()
            } else {
                runners
                    .iter()
                    .map(|was| match was.holding {
                        Some(holding) => format!("{} (holding {holding})", was.name),
                        None => was.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )),
        _ => None,
    }
}

/// The one sentence `herdctl` prints on stderr for a job a runner stopped at
/// a cap. The runner is the subject: the job did not fail, it was stopped.
///
/// `detail` is the cap's own reason when its figure alone would mislead — a
/// job of four kilobytes "stopped at its disk cap of 21474836480 bytes" being
/// the case that made the field exist (D30).
pub fn stopped_by_cap_message(cap: Cap, limit: u64, detail: Option<&str>) -> String {
    format!(
        "this job has no verdict of its own: {}",
        cap_stop_reason(cap, limit, detail)
    )
}

/// The one sentence `herdctl` prints on stderr when Herd could not run the
/// job. Herd is the subject on purpose: the job did not fail, it never ran.
/// A function, so the wording lives in one place and a test can rely on it.
pub fn could_not_start_message(reason: &str) -> String {
    format!("herd could not run this job, so it never started: {reason}")
}

/// The one sentence `herdctl` prints on stderr for a job that ran on a machine
/// which could not do its own part (D166).
///
/// The **machine** is the subject, as Herd is in [`could_not_start_message`]
/// and the runner is in [`stopped_by_cap_message`], because that is the one
/// thing this sentence has to get across: nothing about the submission is at
/// fault and re-submitting it unchanged will fail again in the same way until
/// somebody fixes the host. `reason` names the path.
pub fn setup_failed_message(reason: &str) -> String {
    format!(
        "this job has no verdict of its own: the machine it ran on could not do its part, \
         which is nothing the job or its requester did: {reason}"
    )
}

/// The one sentence `herdctl` prints on stderr when it has no verdict to give:
/// the hub could not be reached, it refused the job, or the job was lost
/// before it finished. Herd is the subject, as above, and the exit code is
/// [`NO_VERDICT_CODE`] — never the job's, because there is no job's code to
/// wear (D20).
pub fn no_verdict_message(reason: &str) -> String {
    format!("herd cannot give you a verdict for this job: {reason}")
}

/// Why an `exited` outcome cannot be worn as an exit code, when it cannot.
///
/// A unix process exits with 0 to 255, so this is unreachable from a real
/// one; what it guards against is a peer reporting something else. The caller
/// gets [`NO_VERDICT_CODE`] and this sentence on stderr, because an exit code
/// Herd had to invent must never be mistaken for the job's own.
pub fn impossible_exit_code_reason(outcome: &JobOutcome) -> Option<String> {
    match outcome {
        JobOutcome::Exited { code } if u8::try_from(*code).is_err() => Some(format!(
            "the exit code reported for this job is {code}, which no process can exit with \
             (an exit code is 0 to 255)"
        )),
        _ => None,
    }
}

/// Submit one command and follow it to its end.
///
/// `on_log` is called with each chunk as it arrives, in the order the runner
/// produced it, carrying the bytes the process wrote (D18); the binary prints
/// them, a test times them.
/// `labels` names the capabilities the job needs (E-25) — names only, because
/// a requester never names a machine (invariant 6). A job needing a capability
/// something outside Herd is holding stays queued and says so through
/// `on_note`; it is never refused for it (E-26, D38), so a caller that ignores
/// notes simply waits.
/// What a requester declares about one job's resources (E-22, E-29, E-31).
///
/// One struct rather than three arguments carried side by side: they are
/// three answers to one question — how much of a machine this job needs — and
/// the hub refuses two of them given together.
#[derive(Debug, Clone, Default)]
pub struct Declared {
    /// How much memory one build thread needs, in MiB (E-22).
    pub build_thread_mib: Option<u32>,
    /// How much memory the whole job needs, in MiB (E-29).
    pub memory_mib: Option<u64>,
    /// How much tmpfs it reserves while it runs, in MiB (E-31).
    pub tmpfs_mib: Option<u64>,
    /// When this job stops being worth starting, in seconds since the Unix
    /// epoch (E-09). Absolute, and judged on the hub's clock.
    pub deadline_unix_s: Option<i64>,
    /// The one job this one starts after (E-10). At most one, no conditions.
    pub after: Option<String>,
    /// The exit code this job's command means "what you gave me has gone" by
    /// (D76). 75 for the first consumer.
    pub requeue_exit_code: Option<i32>,
    /// The declared command this job is, read in the requester's own checkout
    /// (E-11, D106). `None` for an ad-hoc command line.
    pub declaration: Option<Declaration>,
    /// What that command declared its `run` step may take, in seconds (D108).
    pub timeout_s: Option<u64>,
    /// The reading that will say what this job needs, when its command
    /// declares `labels_from` in place of static labels (D74).
    ///
    /// Carried rather than acted on here: the labels the submission goes out
    /// with are the command's declared **baseline**, and the hub runs the
    /// reading as an ordinary job on a runner. `herdctl` fills the line's
    /// `{commit}` and `{args}` like every other line it resolves (D107), and
    /// that is the whole of its part in it.
    pub labels_from: Option<herd_proto::LabelsFrom>,
}

impl Declared {
    /// What a requester asks for, as one [`herd_proto::Asked`] (E-07).
    ///
    /// One conversion, used by `run`, `submit` **and** the dry run, so that
    /// the job E-07 judges and the job a submission makes are one job.
    pub fn asked(self, command: Vec<String>, labels: LabelSet, priority: Priority) -> Asked {
        Asked {
            command,
            labels,
            priority,
            build_thread_mib: self.build_thread_mib,
            memory_mib: self.memory_mib,
            tmpfs_mib: self.tmpfs_mib,
            deadline_unix_s: self.deadline_unix_s,
            after: self.after.map(JobId::new),
            requeue_exit_code: self.requeue_exit_code,
            declaration: self.declaration,
            labels_from: self.labels_from,
            timeout_s: self.timeout_s,
            needs: Vec::new(),
            leaves: std::collections::BTreeMap::new(),
        }
    }
}

/// Something Herd itself says about a job, and where it belongs.
///
/// Two kinds, because they go to two different places and a caller cannot
/// tell them apart from the text. A **note** is Herd's own words about the
/// job — it goes on stderr, because stdout is the job's. The **framing**
/// line is E-02's first line, and it goes on stdout ahead of the job's first
/// byte: the first consumer's tooling reads it there.
///
/// It is **not** carried by `on_log`, and that is the whole reason this type
/// exists: `on_log` is the bytes the job's process wrote (D18), and the
/// stream a requester follows has to equal the `log` file on the hub's disk
/// byte for byte. A framing line sent down that path would make the two
/// differ, which two tests in `herd-hub/tests/durability.rs` pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said<'a> {
    /// Herd's own words about the job: on stderr.
    Note(&'a str),
    /// E-02's first line, already built by [`herd_proto::ran_on_line`]: on
    /// stdout, before the job's output.
    Framing(&'a str),
    /// The submission frame has left this process. **Nothing is printed for
    /// it**, and that is deliberate: a submission about to be acknowledged
    /// has nothing to say, and a submission that is never acknowledged is
    /// reported once, at the end, by [`JOB_UNKNOWN_LINE`].
    ///
    /// It is said at all because it is the moment the answer to "does a job
    /// exist?" stops being *no* and becomes *possibly*, and only the caller
    /// that wrote the frame knows when that was.
    Submitting,
    /// The hub **refused the submission in a frame**, before any job was
    /// minted: proof that nothing was submitted, and the one thing that may
    /// walk [`Said::Submitting`] back.
    ///
    /// Nothing is printed for it either — the refusal itself is already on
    /// stderr as the error this run ends with. What it carries is the
    /// correction: a refusal arriving in answer to a submission means the
    /// hub minted nothing, so the caller may say so (`no-job`) instead of
    /// `job-unknown`, which asks a falling-back requester to go and look for
    /// a job that does not exist.
    ///
    /// **It is only ever sound because every `Refused` on the submission
    /// path precedes the minting.** The hub used to carry one that did not —
    /// a second refusal *after* the submission's own dispatch pass, answering
    /// 125 about a job that already had its wait on its record (D140, D142).
    /// A caller that learns this from a frame that could follow a minting
    /// would be claiming no job exists about a job that does, which is the
    /// same defect wearing the other mask, so the emitting side guards it.
    Refused,
    /// A job certainly exists and this is its id: the hub minted it and said
    /// so, or — on a wait — answered about it at all. Printed on stderr as
    /// [`job_id_line`], immediately: see [`JOB_ID_PREFIX`] for why it cannot
    /// wait until the end.
    Submitted(&'a JobId),
}

/// E-02's framing line, held until there is output for it to be the first
/// line of.
///
/// The hub says where a job went the moment the attempt is handed out, which
/// is **before** the job has started and so before anybody knows whether it
/// can start at all. D16 is exact about that case: a job Herd could not run
/// exits 125 with the reason on stderr, and stdout is left empty — nothing a
/// caller could read as the job having said anything. Printing the framing
/// line as the frame arrived would have put a line on the stdout of a job
/// that never ran.
///
/// So the line is held here and released at the first moment it really is a
/// first line: ahead of the job's first byte of output, or with the verdict
/// for a job that produced none. The one outcome that releases nothing is
/// [`JobOutcome::CouldNotStart`] — a job that never started has no output to
/// frame. Every other ending happened *on a machine*, and where that was is
/// the line's whole point.
///
/// A requeued attempt (E-21) replaces a line still held and sends a second
/// one once it has output of its own, which is the truth about a job that ran
/// in two places.
#[derive(Default)]
struct Framing {
    held: Option<String>,
    /// The last placement, kept whether or not the line is ever printed: it
    /// is what [`Finished::ran_on`] carries (D166). A requeued attempt
    /// replaces it, which is the truth about where the job really ran.
    placed: Option<RanOn>,
}

impl Framing {
    /// The hub handed the job to a runner and said why there.
    fn placed(&mut self, runner: &str, why: &str) {
        self.held = Some(herd_proto::ran_on_line(runner, why));
        self.placed = Some(RanOn {
            runner: runner.to_owned(),
            why: why.to_owned(),
        });
    }

    /// Let it out, if it is still held. Called ahead of every byte of the
    /// job's output, so the line cannot come second.
    fn release(&mut self, said: &mut impl FnMut(Said)) {
        if let Some(line) = self.held.take() {
            said(Said::Framing(&line));
        }
    }

    /// Whether this ending is one the framing line belongs to: anything but
    /// a job that never started (D16).
    fn belongs_to(outcome: &JobOutcome) -> bool {
        !matches!(outcome, JobOutcome::CouldNotStart { .. })
    }
}

/// What a requester following a job is going to do about its output files
/// (D167).
///
/// The hub holds a job's verdict until its streamed collection has all
/// reached it, because a requester that fetches on the verdict otherwise
/// gets a short directory. That costs a wait, and a requester that is never
/// going to ask for the files should not pay it — so it says which it is,
/// and this is that answer in one word at every call site that has to give
/// it.
///
/// [`Wanting::Files`] is the default, in Rust as on the wire: silence has to
/// fail towards waiting, because the requester that says nothing is the one
/// built before the question existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wanting {
    /// It will fetch them, so the verdict waits until they are all on the
    /// hub.
    #[default]
    Files,
    /// It will not, so the verdict comes as soon as it is recorded — which
    /// is what `herdctl run` with no `--outputs-to` has always done.
    OnlyTheVerdict,
}

impl Wanting {
    fn wants_files(self) -> bool {
        self == Wanting::Files
    }
}

/// Submit a job and follow it to its end, wanting its files.
///
/// [`run_job_wanting`] is the same call for a requester that will not fetch
/// them. This one is the safe default and the one every test uses: it waits
/// for a collection it is not going to ask for, which costs latency and
/// cannot give a wrong answer.
pub async fn run_job(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
    on_log: impl FnMut(LogStream, &[u8]),
    said: impl FnMut(Said),
) -> Result<Finished> {
    run_job_wanting(
        Wanting::default(),
        config,
        command,
        labels,
        priority,
        declared,
        on_log,
        said,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_job_wanting(
    wanting: Wanting,
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
    mut on_log: impl FnMut(LogStream, &[u8]),
    mut said: impl FnMut(Said),
) -> Result<Finished> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: declared.asked(command, labels, priority),
            wants_files: wanting.wants_files(),
            // A `run` stays and reads the verdict on its own stream, so
            // there is nobody for the hub to announce the end to: E-37's
            // event is for the submission that *left*. The flag is offered
            // on `submit` alone, which is why this is a constant and not an
            // argument.
            announce_end: false,
        },
    )
    .await
    .context("submitting the job")?;
    // From here a job may exist whatever happens next, including nothing.
    said(Said::Submitting);

    let mut job_id: Option<JobId> = None;
    let mut framing = Framing::default();
    while let Some(message) = next_message(&mut socket).await? {
        match message {
            // The id, out on stderr the moment the hub says it: every way
            // this run can end without a verdict is downstream of here, and
            // so is every way it can be killed from outside (JOB_ID_PREFIX).
            FromHub::JobAccepted { job_id: accepted } => {
                said(Said::Submitted(&accepted));
                job_id = Some(accepted);
            }
            FromHub::LogChunk { stream, data, .. } => {
                framing.release(&mut said);
                on_log(stream, &data);
            }
            // A job this requester has just submitted has produced nothing
            // before it attached, so neither of these is expected here; both
            // are still shown rather than swallowed, the recorded log as the
            // job's own output and a note as Herd's own words (stderr).
            FromHub::JobLog { data, .. } => {
                framing.release(&mut said);
                on_log(LogStream::Stdout, &data);
            }
            FromHub::JobNote { note, .. } => said(Said::Note(&note)),
            // The hub keeping this connection warm under a job that is
            // running and printing nothing. **Nothing happens here, and that
            // is the whole of it**: not a byte on stdout, not a word on
            // stderr, no release of the held `ran on:` line — E-02's framing
            // is the line, the job's own bytes, then the verdict, and a
            // breath from the hub is in none of those places.
            //
            // Handled by name rather than left to the fallback below, which
            // is a `bail!`: a frame a requester "cannot use" ends the run
            // with an error, so swallowing this one by accident was never
            // available. And by name rather than by a wildcard so that the
            // next frame added to the protocol still meets that `bail!`
            // instead of being silently ignored.
            FromHub::StillThere { .. } => {}
            // E-02's first line, on **stdout**, ahead of the job's first
            // byte: the output a requester sees is framed with where the job
            // ran and why there, and the job's own output follows it
            // unchanged (D109). It is held rather than printed here — see
            // [`Framing`] — and it goes through the output callback rather
            // than through a note because a note is Herd's own words on
            // stderr, and this line is part of the output the first
            // consumer's tooling reads.
            FromHub::RanOn { runner, why, .. } => framing.placed(&runner, &why),
            FromHub::JobFinished {
                job_id,
                outcome,
                metrics,
                outputs,
                commit,
            } => {
                if Framing::belongs_to(&outcome) {
                    framing.release(&mut said);
                }
                let _ = socket.close(None).await;
                return Ok(Finished {
                    job_id,
                    outcome,
                    metrics,
                    outputs,
                    ran_on: framing.placed.clone(),
                    commit,
                });
            }
            FromHub::Refused { reason } => {
                // **Only when no id has been learned**, and the guard is the
                // whole of what makes this sound. A refusal in answer to the
                // submission is proof the hub minted nothing; a refusal
                // arriving *after* a `JobAccepted` is about something else,
                // for whatever reason a future hub might have, and must never
                // make this process claim no job exists about a job that
                // does (D142).
                if job_id.is_none() {
                    said(Said::Refused);
                }
                bail!("the hub refused the job: {reason}")
            }
            other => bail!(
                "the hub sent {}, which a requester cannot use",
                message_kind(&other)
            ),
        }
    }
    match job_id {
        Some(job_id) => bail!("the hub closed the connection while {job_id} was still running"),
        // Not "no job exists": the submission left, and whether the hub
        // minted one before the link went is exactly what is unknown here.
        // The id line was never printed, so the caller reads this as the
        // third state and E-05 makes retrying safe (JOB_UNKNOWN_LINE).
        None => bail!("the hub closed the connection without saying whether it accepted the job"),
    }
}

/// Submit one command and leave, with the id the hub minted.
///
/// The job runs whether anyone is listening or not — the hub records it — so
/// this is how a push hook or a script fires work and gets on with its day
/// (E-03, E-42). The id is the only handle needed afterwards:
/// [`wait_for_job`] takes it, and so does the job's directory on the hub's
/// disk.
pub async fn submit_job(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
) -> Result<JobId> {
    submit_job_saying(config, command, labels, priority, declared, |_| {}).await
}

/// [`submit_job`], telling the caller what it knows about whether a job
/// exists as it learns it (see [`JOB_ID_PREFIX`]).
///
/// A separate entry point rather than one more argument on `submit_job`:
/// a test that only wants an id should not have to say so, and the binary
/// is the one caller that has to answer "is there a job?" after a failure.
pub async fn submit_job_saying(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
    said: impl FnMut(Said),
) -> Result<JobId> {
    submit_job_announcing(config, command, labels, priority, declared, false, said).await
}

/// [`submit_job_saying`], for a submitter that **asks for this job's end to
/// be announced** (E-37).
///
/// The whole of what a requester does about an event: say so once, at
/// submission time. Everything after that is the hub's and the operator's —
/// whether a sink is configured at all, where it points, what the topic is
/// called. A submitter gets no answer about it and should not: the asking is
/// recorded on `job.json` whether anybody is listening or not, which is what
/// makes `herdctl submit --announce-end` safe to put in a crontab on a day
/// the webhook is down.
///
/// A third entry point rather than a fourth argument on [`submit_job`], for
/// the reason [`submit_job_saying`] is a second: the dozens of callers that
/// only want an id must not have to answer a question they have no opinion
/// about, and the default — not asking — is E-37's *"never per step job by
/// default"*.
pub async fn submit_job_announcing(
    config: &Config,
    command: Vec<String>,
    labels: LabelSet,
    priority: Priority,
    declared: Declared,
    announce_end: bool,
    mut said: impl FnMut(Said),
) -> Result<JobId> {
    if command.is_empty() {
        bail!("a job needs a command to run");
    }
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::SubmitJob {
            asked: declared.asked(command, labels, priority),
            // A `submit` leaves with the id and closes, so it follows
            // nothing and nothing is ever held for it: the hub drops a
            // follower when its connection goes. It sends the default rather
            // than an answer it has no business giving (D167).
            wants_files: Wanting::default().wants_files(),
            announce_end,
        },
    )
    .await
    .context("submitting the job")?;
    // From here a job may exist whatever happens next, including nothing.
    said(Said::Submitting);
    match next_message(&mut socket).await? {
        Some(FromHub::JobAccepted { job_id }) => {
            said(Said::Submitted(&job_id));
            let _ = socket.close(None).await;
            Ok(job_id)
        }
        Some(FromHub::Refused { reason }) => {
            // The first frame after the submission, so no id can have been
            // learned here by construction — `JobAccepted` returns above.
            // The guard `run_job` needs is therefore already kept by the
            // shape of this match (D142).
            said(Said::Refused);
            bail!("the hub refused the job: {reason}")
        }
        Some(other) => bail!(
            "the hub answered a submission with {}",
            message_kind(&other)
        ),
        // As in `run_job`: the submission left, so whether a job exists is
        // unknown and must not be said to be no.
        None => bail!("the hub closed the connection without saying whether it accepted the job"),
    }
}

/// Wait for a job that already exists, by id, and learn how it ended.
///
/// Works on a job that is still queued, one that is running, and one that
/// ended before this call was made: the answer comes from the hub's record
/// either way.
///
/// The job's output comes with it (D22). `on_log` is called with whatever the
/// job had already written when this wait attached — one call, both streams
/// interleaved as the hub's `log` file keeps them, so it is reported as
/// stdout — and then with every chunk that arrives afterwards on its own
/// stream. A remote requester has no other way to read a detached job's
/// output.
///
/// `on_note` is called with anything the hub says about the job itself: in M2
/// exactly one sentence, for a job whose attempt was interrupted and whose
/// result may never come back (D17). It is Herd speaking, not the job, so the
/// binary puts it on stderr.
pub async fn wait_for_job(
    config: &Config,
    job_id: JobId,
    on_log: impl FnMut(LogStream, &[u8]),
    said: impl FnMut(Said),
) -> Result<Finished> {
    wait_for_job_wanting(Wanting::default(), config, job_id, on_log, said).await
}

/// The same wait, saying whether this requester will fetch the job's output
/// files (D167).
pub async fn wait_for_job_wanting(
    wanting: Wanting,
    config: &Config,
    job_id: JobId,
    mut on_log: impl FnMut(LogStream, &[u8]),
    mut said: impl FnMut(Said),
) -> Result<Finished> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::WaitForJob {
            job_id: job_id.clone(),
            wants_files: wanting.wants_files(),
        },
    )
    .await
    .context("asking to be told how the job ends")?;

    let mut framing = Framing::default();
    // A wait submits nothing, so there is nothing new to duplicate — but a
    // wait that ends without a verdict is still a job left running
    // somewhere, and the contract is one rule everywhere: if the job exists,
    // its id is on stderr (JOB_ID_PREFIX). The hub answering about it at all
    // is what proves it exists; a refusal says it does not.
    let mut named = false;
    while let Some(message) = next_message(&mut socket).await? {
        if !named && !matches!(message, FromHub::Refused { .. }) {
            said(Said::Submitted(&job_id));
            named = true;
        }
        match message {
            FromHub::JobFinished {
                job_id,
                outcome,
                metrics,
                outputs,
                commit,
            } => {
                if Framing::belongs_to(&outcome) {
                    framing.release(&mut said);
                }
                let _ = socket.close(None).await;
                return Ok(Finished {
                    job_id,
                    outcome,
                    metrics,
                    outputs,
                    ran_on: framing.placed.clone(),
                    commit,
                });
            }
            FromHub::JobLog { data, .. } => {
                framing.release(&mut said);
                on_log(LogStream::Stdout, &data);
            }
            FromHub::LogChunk { stream, data, .. } => {
                framing.release(&mut said);
                on_log(stream, &data);
            }
            FromHub::JobNote { note, .. } => said(Said::Note(&note)),
            // The hub keeping a quiet wait's connection warm. Nothing is
            // printed for it, here as in [`run_job`]: a `herdctl wait` on a
            // job that runs for an hour in silence is exactly the case the
            // frame exists for, and exactly the case where one stray
            // character would break the output a reader is anchored on.
            FromHub::StillThere { .. } => {}
            // E-02's first line, on **stdout**, ahead of the job's first
            // byte: the output a requester sees is framed with where the job
            // ran and why there, and the job's own output follows it
            // unchanged (D109). It is held rather than printed here — see
            // [`Framing`] — and a wait that attached to a job already running
            // gets it off the record's own attempt, so the sentence is the
            // one that was recorded (E-36).
            FromHub::RanOn { runner, why, .. } => framing.placed(&runner, &why),
            FromHub::Refused { reason } => bail!("the hub refused the wait: {reason}"),
            other => bail!(
                "the hub sent {}, which a requester cannot use",
                message_kind(&other)
            ),
        }
    }
    // The hub closed without ever answering. The job this wait names is not
    // this invocation's to duplicate, and coming back to it is idempotent,
    // so it is named here too rather than left to the sentence below: one
    // rule, every path (JOB_ID_PREFIX).
    if !named {
        said(Said::Submitted(&job_id));
    }
    bail!("the hub closed the connection while {job_id} was still running")
}

/// Write a job's kept files under `directory`, keeping their relative paths,
/// and come back with one line per file written (D131).
///
/// Three things this deliberately does:
///
/// * **It never touches stdout.** The caller puts these lines on stderr. An
///   output file printed into E-02's stream would be read by the consumer's
///   own tooling as part of the job's output, which is the one thing E-02
///   cannot survive.
/// * **It checks every path again**, with [`herd_proto::check_relative`] —
///   the same function the runner and the hub use. These paths arrived over a
///   socket and are about to be joined onto a directory on the requester's
///   own disk, which is the third place in Herd where that would be a hole
///   and the third place the one check closes it.
/// * **It writes nothing when the collection was refused.** Over the cap
///   there are no files, and a directory the caller can find empty is the
///   honest shape of that: the sentence is what says why.
/// * **It says nothing about which carriage the collection came by.** By the
///   time it is called every entry either holds its bytes or is known to be
///   absent, whichever way they travelled — see [`fetch_missing_outputs`] —
///   so one function writes both and there is nothing here to branch on
///   (D148).
///
/// An entry with no bytes is **named and not written**. That is the manifest
/// outliving the bytes reaching a requester's terminal: twelve files on the
/// record and eight on the disk have to read as twelve, or the caller reads
/// eight as all of them.
pub fn write_outputs(
    directory: &std::path::Path,
    outputs: &herd_proto::JobOutputs,
) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for file in &outputs.files {
        herd_proto::check_relative(&file.path).map_err(|why| {
            anyhow!("the hub named an output file this requester will not write: {why}")
        })?;
        let Some(data) = &file.data else {
            written.push(format!(
                "did NOT get {} ({} bytes on this job's record): the hub does not have it",
                directory.join(&file.path).display(),
                file.len
            ));
            continue;
        };
        let path = directory.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("making {}", parent.display()))?;
        }
        std::fs::write(&path, data).with_context(|| format!("writing {}", path.display()))?;
        written.push(format!("wrote {} ({} bytes)", path.display(), data.len()));
    }
    Ok(written)
}

/// Ask for a job to be cancelled, and come back with what the hub says
/// happened (E-06).
///
/// A requester never names a machine (invariant 6), here as anywhere: it names
/// the job, and the hub reaches whichever runner is executing it. The answer is
/// one sentence, because there are three true answers — it was queued and is
/// over, it is being ended on its runner, it was already finished — and a
/// caller that cannot tell them apart would have to guess.
/// Ask where a job would run, and for nothing to happen (E-07).
///
/// The same [`Asked`] a submission carries, so the answer is about the job a
/// submission would make. Returns the hub's lines, in order.
pub async fn would_run(config: &Config, asked: Asked) -> Result<Vec<String>> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::DryRun { asked })
        .await
        .context("asking where a job would run")?;
    match next_message(&mut socket).await? {
        Some(FromHub::WouldRun { lines }) => {
            let _ = socket.close(None).await;
            Ok(lines)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the question: {reason}"),
        Some(other) => bail!("the hub answered a dry run with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Move one runner's switch through the hub (E-33).
///
/// The orchestrator's half. The owner's half is `herd-runner switch` on the
/// machine itself, which needs no hub at all.
///
/// Answers with the hub's sentence and **whether the runner confirmed writing
/// the position down** (D99). The second half is not decoration: this call
/// blocks until the runner has the position on its own disk, or until E-33's
/// ten seconds are spent, and a caller that ignored the flag would report
/// success for a position a `kill -9` could still undo.
pub async fn set_switch(config: &Config, runner: String, switch: Switch) -> Result<(String, bool)> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::SetSwitch { runner, switch })
        .await
        .context("setting a switch")?;
    match next_message(&mut socket).await? {
        Some(FromHub::SwitchSet { said, confirmed }) => {
            let _ = socket.close(None).await;
            Ok((said, confirmed))
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused: {reason}"),
        Some(other) => bail!("the hub answered a switch with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Where every runner's switch stands (E-33).
pub async fn fleet_switches(config: &Config) -> Result<Vec<RunnerSwitch>> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::AskFleetSwitches {})
        .await
        .context("asking where the fleet's switches stand")?;
    match next_message(&mut socket).await? {
        Some(FromHub::FleetSwitches { runners }) => {
            let _ = socket.close(None).await;
            Ok(runners)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused: {reason}"),
        Some(other) => bail!("the hub answered with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// One runner's switch, as `herdctl switch` prints it (E-33).
///
/// The position, the owner's reason, and whether work is still running there —
/// because "off with a job on it" is the ordinary case and a line that did not
/// say so would read like a contradiction. Then, since D166, the build the
/// machine runs, and an indented line for each capability it is not handing
/// out: this is the same fleet `herdctl runner list` shows from the other
/// side, and an operator must not have to learn which of the two tells her
/// the truth about a machine.
pub fn switch_lines(runner: &RunnerSwitch) -> Vec<String> {
    let mut line = format!("{}: {}", runner.name, runner.switch.sentence());
    line.push_str(&match runner.jobs {
        0 => ", nothing running".to_owned(),
        1 => ", 1 job running there finishes".to_owned(),
        many => format!(", {many} jobs running there finish"),
    });
    if runner.stopping {
        line.push_str("; it is also stopping (SIGTERM)");
    }
    if let Some(version) = &runner.facts.version {
        line.push_str(&format!("; herd {version}"));
    }
    let mut lines = vec![line];
    lines.extend(suspension_lines(&runner.facts));
    lines
}

/// What a fleet listing says, under a runner's own line, about the
/// capabilities it is offering and not handing out (D49, D166).
///
/// **Indented and one per capability**, rather than appended to the line
/// above. A probe's reason is a sentence with a path in it — the first
/// consumer's were sixty characters each — and two of them on one line is a
/// line nobody reads to the end. The listing is where an operator looks first
/// and these are the lines she has to see.
///
/// Nothing is printed when nothing is suspended, and nothing is printed when
/// the hub did not say: a line claiming a clean bill of health is exactly
/// what a hub that cannot answer must not produce
/// ([`herd_proto::RunnerFacts`]).
fn suspension_lines(facts: &herd_proto::RunnerFacts) -> Vec<String> {
    facts
        .suspensions()
        .iter()
        .map(|suspended| format!("  {}", suspended.sentence()))
        .collect()
}

/// A deadline as a requester types it, in seconds since the Unix epoch
/// (E-09).
///
/// Two spellings, and the relative one is resolved **here**, against this
/// machine's clock, so that what goes on the wire is an instant: `+15m` means
/// a quarter of an hour from now whatever the hub is doing, and `05:00
/// tomorrow` means that moment. The hub then judges the instant against its
/// own clock, which is the only clock in Herd that decides anything.
///
/// A deadline already in the past is **not** refused. "Start this only if you
/// can start it at once" is a real thing to ask for, E-41's nightly is
/// exactly that shape, and the job ends `cancelled (deadline)` on the first
/// pass with the reason naming both moments.
pub fn deadline_at(said: &str) -> Result<i64> {
    let said = said.trim();
    let now = time::OffsetDateTime::now_utc();
    if let Some(offset) = said.strip_prefix('+') {
        let seconds = parse_offset_seconds(offset)
            .ok_or_else(|| anyhow::anyhow!("{said:?} is not a span: try +15m, +2h or +90s"))?;
        return Ok(now.unix_timestamp() + seconds);
    }
    let parsed = time::OffsetDateTime::parse(said, &time::format_description::well_known::Rfc3339)
        .with_context(|| {
            format!(
                "{said:?} is not a moment: give it as RFC 3339 \
                 (2026-10-14T05:00:00+02:00) or as a span from now (+15m)"
            )
        })?;
    Ok(parsed.unix_timestamp())
}

/// `90s`, `15m`, `2h`, `1d` as seconds.
fn parse_offset_seconds(said: &str) -> Option<i64> {
    let (figure, unit) = said.split_at(said.len().checked_sub(1)?);
    let figure: i64 = figure.parse().ok()?;
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return None,
    };
    figure.checked_mul(scale)
}

pub async fn cancel_job(config: &Config, job_id: JobId) -> Result<String> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::CancelJob {
            job_id: job_id.clone(),
        },
    )
    .await
    .context("asking for the job to be cancelled")?;
    match next_message(&mut socket).await? {
        Some(FromHub::JobNote { note, .. }) => {
            let _ = socket.close(None).await;
            Ok(note)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the cancel: {reason}"),
        Some(other) => bail!("the hub answered a cancel with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering the cancel"),
    }
}

/// Ask for a job's priority to be raised, and come back with what the hub
/// says happened (E-18).
///
/// For the consumer's orchestrator, which is the only thing that raises a
/// job: an agent says what its job is worth at submission and does not come
/// back to argue. Upwards only — the hub refuses a request to lower one —
/// and never on a job that is over.
pub async fn raise_job(config: &Config, job_id: JobId, priority: Priority) -> Result<String> {
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::RaiseJob {
            job_id: job_id.clone(),
            priority,
        },
    )
    .await
    .context("asking for the job's priority to be raised")?;
    match next_message(&mut socket).await? {
        Some(FromHub::JobNote { note, .. }) => {
            let _ = socket.close(None).await;
            Ok(note)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the raise: {reason}"),
        Some(other) => bail!("the hub answered a raise with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering the raise"),
    }
}

/// Fill in the bytes of any manifest entry that arrived without them.
///
/// **This is the function that makes the two carriages indistinguishable**
/// (D148), and it does it by not knowing about them. A collection's manifest
/// is complete whichever way it travelled; what differs is only whether the
/// bytes came with it. So: anything already holding its bytes is left alone,
/// anything missing them is asked for, and what comes back fills the same
/// `files` list. The caller is then holding one collection and cannot tell
/// which way it came.
///
/// Two consequences worth naming, because they are the design and not
/// side-effects:
///
/// * **The inline carriage sends nothing.** Nothing is missing, so there is
///   no frame and no second round trip — D137's single round trip, kept
///   exactly.
/// * **Re-running a fetch that died is free.** The caller passes whatever it
///   already has; only what is still missing is asked for. That is the whole
///   of resumption on this leg: the bytes are on the hub's disk, so there is
///   no transfer state to recover, just a shorter list to ask for.
///
/// A file the hub cannot hand over is **left without bytes and named** in the
/// returned sentences rather than invented or dropped. That is the manifest
/// outliving the bytes: a requester asking for yesterday's screenshots after
/// retention took them is told which files, how big they were and why they
/// are gone, and never handed a short collection it would read as whole.
/// How long a fetch goes on waiting for a collection that is **not getting
/// any further**.
///
/// There is deliberately no bound on a transfer that is making progress: a
/// few hundred megabytes on a slow link is slow and not broken, and a
/// deadline on it would turn the consumer's biggest runs into the ones that
/// fail. What is bounded is a transfer that has stopped — a runner that went
/// away mid-collection — and the bound is on *silence*, the same shape as
/// every other liveness figure in Herd (D148).
pub const OUTPUTS_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// How often a waiting fetch asks again.
const OUTPUTS_POLL: std::time::Duration = std::time::Duration::from_millis(200);

pub async fn fetch_missing_outputs(
    config: &Config,
    job_id: &JobId,
    outputs: &mut herd_proto::JobOutputs,
) -> Result<Vec<String>> {
    let mut said = Vec::new();
    // **Waiting is part of the promise.** The verdict goes first and the
    // bytes follow (D148), so a requester asking the instant its job ended is
    // asking for a collection that is genuinely still crossing. The hub says
    // which absences are still coming and how far they have got, so this
    // waits while they move and stops when they stop.
    let mut best = 0u64;
    let mut since = std::time::Instant::now();
    loop {
        let (progress, coming) = fetch_once(config, job_id, outputs, &mut said).await?;
        if !coming {
            return Ok(said);
        }
        if progress > best {
            best = progress;
            since = std::time::Instant::now();
        } else if since.elapsed() >= OUTPUTS_STALL_LIMIT {
            said.push(format!(
                "{job_id}: its runner stopped sending this collection {:?} ago, with \
                 {progress} byte(s) of it here. Whatever is missing above is named; nothing \
                 was truncated.",
                OUTPUTS_STALL_LIMIT
            ));
            return Ok(said);
        }
        tokio::time::sleep(OUTPUTS_POLL).await;
    }
}

/// The one sentence for a whole collection that is gone for one reason, or
/// `None` when the absences do not share one.
///
/// Two or more files missing for the same reason is a *collection* that is
/// gone, and that is what the reader needs told once: how many files, how
/// much, and why. Different reasons mean different facts, and summarising
/// them would be the kind of tidy sentence that hides one of them — so then
/// there is no summary and the per-file lines stand on their own.
fn one_reason_for(job_id: &JobId, absent: &[herd_proto::AbsentFile]) -> Option<String> {
    let first = absent.first()?;
    if absent.len() < 2 || absent.iter().any(|gone| gone.why != first.why) {
        return None;
    }
    let bytes: u64 = absent.iter().map(|gone| gone.len).sum();
    Some(format!(
        "{job_id}: {} file(s), {}, {} — every path, size and digest is still on this \
         job's record, and they are named below",
        absent.len(),
        herd_proto::show_bytes(bytes),
        first.why,
    ))
}

/// One round of asking. Returns how many bytes of the collection the hub
/// holds and whether any of it is still on its way.
async fn fetch_once(
    config: &Config,
    job_id: &JobId,
    outputs: &mut herd_proto::JobOutputs,
    said: &mut Vec<String>,
) -> Result<(u64, bool)> {
    let wanted: Vec<String> = outputs
        .files
        .iter()
        .filter(|file| !file.is_held())
        .map(|file| file.path.clone())
        .collect();
    if wanted.is_empty() {
        return Ok((0, false));
    }
    // **What this requester already holds counts towards the progress it
    // reports**, and leaving it out was a defect this round could not see
    // (D167). A round asks only for what is still missing, so the figure it
    // came back with was the bytes of *that* round — and the first round of a
    // 657-file collection is by far the biggest. Every round after it
    // reported less than the first, however many files it had just collected,
    // so the caller's stall bound saw a transfer that never beat its best and
    // gave up thirty seconds later on a collection that was arriving
    // normally. With a one-file collection — which is what the suite streamed
    // — the figure is monotonic by accident and nothing showed.
    let mut progress: u64 = outputs
        .files
        .iter()
        .filter(|file| file.is_held())
        .map(|file| file.len)
        .sum();
    let mut socket = join(config).await?;
    send(
        &mut socket,
        &ToHub::AskJobFiles {
            job_id: job_id.clone(),
            paths: wanted,
        },
    )
    .await
    .context("asking the hub for this job's files")?;
    let mut arriving: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    // Whether any of it is still on its way. The progress figure is above,
    // seeded with what earlier rounds already collected.
    let mut coming = false;
    loop {
        match next_message(&mut socket).await? {
            Some(FromHub::JobFile {
                path, offset, data, ..
            }) => {
                let holding = arriving.entry(path.clone()).or_default();
                // The offset is checked here too, for the reason the hub
                // checks it: bytes appended at the wrong place make a file
                // that is a plausible mixture of two reads, and the digest
                // check below would then fail for a reason nobody could see.
                if offset != holding.len() as u64 {
                    bail!(
                        "the hub sent {path} from byte {offset} and this requester holds {} \
                         of it: a collection cannot be assembled out of order",
                        holding.len()
                    );
                }
                holding.extend_from_slice(&data);
            }
            Some(FromHub::JobFilesSent { absent, .. }) => {
                let _ = socket.close(None).await;
                let mut final_absences = Vec::new();
                for gone in absent {
                    progress += gone.held;
                    if gone.still_coming {
                        // **Not said yet**, because it is not a conclusion:
                        // the bytes are crossing and the caller is about to
                        // ask again. A line here would print a sentence per
                        // poll about a transfer that is working fine.
                        coming = true;
                        continue;
                    }
                    final_absences.push(gone);
                }
                // **The one line first, then the names.** A collection the
                // hub's sweeper took is gone all at once and for one
                // reason, and the sentence a requester needs is the
                // collection's — *12 files, 338 MiB, removed 2026-10-30
                // under the 200 GB output budget* (D148, D152). Printing
                // only the per-file lines would make somebody count twelve
                // of them to learn one fact. The per-file lines stay,
                // because which paths are gone is the other half: a short
                // directory must never read as a whole one.
                if let Some(summary) = one_reason_for(job_id, &final_absences) {
                    said.push(summary);
                }
                for gone in final_absences {
                    said.push(format!(
                        "{} ({} bytes on this job's record) is not on the hub: {}",
                        gone.path, gone.len, gone.why
                    ));
                }
                break;
            }
            Some(FromHub::Refused { reason }) => {
                bail!("the hub refused to hand over this job's files: {reason}")
            }
            Some(other) => bail!(
                "the hub answered a request for a job's files with {}",
                message_kind(&other)
            ),
            None => bail!("the hub closed the connection while sending this job's files"),
        }
    }
    for file in &mut outputs.files {
        let Some(data) = arriving.remove(&file.path) else {
            continue;
        };
        // **Verified against the manifest before it is believed.** The
        // digest is the whole reason a streamed transfer is safe to resume,
        // and a requester that took the bytes on trust would be the one party
        // that never checked.
        let candidate = herd_proto::OutputFile {
            data: Some(data),
            ..file.clone()
        };
        match candidate.disagreement() {
            None => {
                progress += file.len;
                *file = candidate;
            }
            Some(why) => said.push(format!(
                "{} did not arrive intact and was not kept: {why}",
                file.path
            )),
        }
    }
    Ok((progress, coming))
}

/// What this requester will read off the wire: Herd's own ceiling, not the
/// library's default.
///
/// The WebSocket library's own `max_frame_size` is 16 MiB, which nobody here
/// chose and which is **smaller than a frame the hub may legitimately send**:
/// a verdict carrying a job's kept files is one frame, and the hub reads and
/// writes up to [`herd_proto::FRAME_CEILING`]. Left at the default, a job
/// whose output was well inside the runner's cap would have arrived as a
/// closed connection and a 125 — a job with no verdict, for a job that
/// passed, which is the shape of defect D117 named. So the figure is stated
/// here, the same one on every socket in Herd.
fn frame_limits() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    let ceiling = usize::try_from(herd_proto::FRAME_CEILING).unwrap_or(usize::MAX);
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(ceiling))
        .max_frame_size(Some(ceiling))
}

/// Dial the hub and get through the handshake.
async fn join(config: &Config) -> Result<Socket> {
    let (mut socket, _response) =
        tokio_tungstenite::connect_async_with_config(&config.hub_url, Some(frame_limits()), false)
            .await
            .with_context(|| format!("dialling {}", config.hub_url))?;
    send(
        &mut socket,
        &ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            // Which build is asking, beside which protocol it speaks (E-51).
            peer_version: requester_version().to_owned(),
            role: Role::Requester,
            token: config.token.clone(),
            name: config.name.clone(),
            // A requester offers no capacity and runs nothing, so it has no
            // size and no preference: both are what the hub asks of a runner
            // (E-22, E-19).
            host: herd_proto::HostSize::default(),
            // Nor a ceiling on jobs held at once: a requester holds none, so
            // there is no figure here to be silent about (D181).
            jobs_at_once: None,
            preference: 0,
            // Nor does a requester measure anything about a host (E-32).
            load_milli: None,
            // And a requester has no owner's switch: nothing is ever claimed
            // on it (E-33). `on` is what "nobody has withdrawn this" means.
            switch: Switch::On,
            // Neither belongs to a requester. It holds nothing between
            // connections — no attempt, no spooled verdict — so there is
            // nothing for the hub to tell apart and nothing to attribute to a
            // state root. A requester is one command's worth of conversation.
            instance: None,
            state_root: None,
            // Nor a cadence: a requester reports nothing, so there is no
            // silence of its own for a hub to time (E-40 is the runner's).
            health_every_ms: None,
            // A requester offers nothing: capacity is a runner's word.
            capabilities: CapabilityOffer::empty(),
            // And no mesh endpoint: pack content moves runner to runner
            // (D10), so a requester has nothing to serve and never would.
            mesh_endpoint: None,
        },
    )
    .await
    .context("sending hello")?;
    match next_message(&mut socket).await? {
        Some(FromHub::Welcome { version_note, .. }) => {
            // What the hub says about the gap between its build and this
            // one, when it says anything (E-51). On stderr, where every one
            // of Herd's own words goes: stdout belongs to the answer the
            // caller asked for.
            if let Some(note) = version_note {
                eprintln!("herdctl: {note}");
            }
            Ok(socket)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused this requester: {reason}"),
        Some(other) => bail!("the hub answered hello with {}", message_kind(&other)),
        None => bail!("the hub closed the connection during the handshake"),
    }
}

/// Which build of `herdctl` this is, as the handshake announces it (E-51).
pub fn requester_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Approve one runner by name, and come back with the token the hub minted
/// for it (E-48, Q8).
///
/// One operator action. The token is readable here and nowhere else: the hub
/// keeps a hash of it, so an operator who loses it approves the runner again
/// rather than looking it up.
pub async fn approve_runner(config: &Config, runner: String) -> Result<(String, String)> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::ApproveRunner { runner })
        .await
        .context("approving a runner")?;
    match next_message(&mut socket).await? {
        Some(FromHub::RunnerApproved { token, said, .. }) => {
            let _ = socket.close(None).await;
            Ok((token, said))
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the approval: {reason}"),
        Some(other) => bail!("the hub answered an approval with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Remove one runner, which revokes its token (E-48).
pub async fn remove_runner(config: &Config, runner: String) -> Result<String> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::RemoveRunner { runner })
        .await
        .context("removing a runner")?;
    match next_message(&mut socket).await? {
        Some(FromHub::RunnerRemoved { said }) => {
            let _ = socket.close(None).await;
            Ok(said)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused the removal: {reason}"),
        Some(other) => bail!("the hub answered a removal with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// Which runners are approved on this hub (E-48).
pub async fn approved_runners(config: &Config) -> Result<Vec<ApprovedRunner>> {
    let mut socket = join(config).await?;
    send(&mut socket, &ToHub::AskApprovedRunners {})
        .await
        .context("asking which runners are approved")?;
    match next_message(&mut socket).await? {
        Some(FromHub::ApprovedRunners { runners }) => {
            let _ = socket.close(None).await;
            Ok(runners)
        }
        Some(FromHub::Refused { reason }) => bail!("the hub refused: {reason}"),
        Some(other) => bail!("the hub answered with {}", message_kind(&other)),
        None => bail!("the hub closed the connection without answering"),
    }
}

/// One approved runner, as `herdctl runner list` prints it (E-48, D166).
///
/// No token and no hash: there is nothing to print, which is the property
/// E-48 is built on.
///
/// **This is the one command an operator runs to see whether the fleet is
/// working**, and until D166 it could not say so: a machine whose `kvm` and
/// `gpu-venus` probes had been failing for hours, with every job that needed
/// them waiting, printed the same line as a healthy one. So the runner's
/// build is on its line, and each capability it is not handing out gets an
/// indented line under it, with the reason.
pub fn approved_runner_lines(runner: &ApprovedRunner) -> Vec<String> {
    let mut line = format!(
        "{}: approved {}, {}",
        runner.name,
        approved_at(runner.approved_unix_s),
        if runner.connected {
            "connected now"
        } else {
            "not connected"
        }
    );
    if let Some(version) = &runner.facts.version {
        line.push_str(&format!(", herd {version}"));
    }
    let mut lines = vec![line];
    lines.extend(suspension_lines(&runner.facts));
    lines
}

/// When an approval was made, as a human reads it, falling back to the raw
/// figure rather than inventing a date.
fn approved_at(unix_s: u64) -> String {
    let seconds = i64::try_from(unix_s).unwrap_or(i64::MAX);
    match time::OffsetDateTime::from_unix_timestamp(seconds)
        .ok()
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }) {
        Some(at) => at,
        None => format!("at unix second {unix_s}"),
    }
}

async fn send(socket: &mut Socket, message: &ToHub) -> Result<()> {
    let frame = to_frame(message).context("encoding a message")?;
    socket.send(Message::text(frame)).await.context("sending")
}

async fn next_message(socket: &mut Socket) -> Result<Option<FromHub>> {
    while let Some(frame) = socket.next().await {
        match frame.context("reading a frame")? {
            Message::Text(text) => {
                let message: FromHub = from_frame(text.as_str())
                    .map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
                // **A requester still refuses what it cannot read, and that
                // is right** (D124). The catch-all changed what a runner does
                // with an unknown frame, not what a requester does: one that
                // cannot understand what the hub is telling it must not go on
                // to report a verdict, so the run ends with no verdict and
                // exit 125 — which is a true statement about what this build
                // knows, where a guess would be a false one about the job.
                //
                // What the catch-all buys here is the **name**. This used to
                // be a serde error about an unknown variant; it is now a
                // sentence saying which frame and what to do about it, which
                // is the difference between an operator who can act and one
                // who files a bug.
                if message == FromHub::Unknown {
                    bail!(
                        "the hub sent a frame of type {:?}, which this requester does not \
                         know. It speaks protocol {PROTOCOL_VERSION}; the hub is newer, so \
                         rebuild herdctl rather than trusting a verdict this one cannot \
                         read.",
                        herd_proto::frame_type(text.as_str())
                            .unwrap_or_else(|| "(none)".to_owned())
                    );
                }
                return Ok(Some(message));
            }
            Message::Close(_) => return Ok(None),
            Message::Binary(_) => bail!("the protocol carries text frames only (D4)"),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
    Ok(None)
}

fn message_kind(message: &FromHub) -> &'static str {
    match message {
        FromHub::Welcome { .. } => "welcome",
        FromHub::Refused { .. } => "refused",
        FromHub::StandBy { .. } => "stand_by",
        FromHub::HealthRecorded {} => "health_recorded",
        FromHub::JobAccepted { .. } => "job_accepted",
        FromHub::AssignJob { .. } => "assign_job",
        FromHub::CancelJob { .. } => "cancel_job",
        FromHub::StaleAttempt { .. } => "stale_attempt",
        FromHub::LogChunk { .. } => "log_chunk",
        FromHub::LogAck { .. } => "log_ack",
        FromHub::ResendLog { .. } => "resend_log",
        FromHub::JobLog { .. } => "job_log",
        FromHub::JobFile { .. } => "job_file",
        FromHub::JobFilesSent { .. } => "job_files_sent",
        FromHub::JobFilesHeld { .. } => "job_files_held",
        FromHub::JobFilesAck { .. } => "job_files_ack",
        FromHub::Unknown => "a frame of a type this build does not know",
        FromHub::JobNote { .. } => "job_note",
        FromHub::StillThere { .. } => "still_there",
        FromHub::RanOn { .. } => "ran_on",
        FromHub::JobFinished { .. } => "job_finished",
        FromHub::JobResultAck { .. } => "job_result_ack",
        FromHub::WouldRun { .. } => "would_run",
        FromHub::SwitchSet { .. } => "switch_set",
        FromHub::FleetSwitches { .. } => "fleet_switches",
        FromHub::RunnerApproved { .. } => "runner_approved",
        FromHub::RunnerRemoved { .. } => "runner_removed",
        FromHub::ApprovedRunners { .. } => "approved_runners",
        FromHub::SetSwitch { .. } => "set_switch",
        FromHub::ReadProbesAgain { .. } => "read_probes_again",
        // The pack half's seven, numbered at protocol 3 and handled by
        // nobody yet. Named here because this function's whole job is saying
        // what arrived, and a frame falling to `Unknown`'s sentence would
        // claim this build has never heard of a frame it defines.
        FromHub::PackHolders(_) => "pack_holders",
        FromHub::TransferCapability(_) => "transfer_capability",
        FromHub::ExpectTransfer(_) => "expect_transfer",
        FromHub::PackRegistered(_) => "pack_registered",
        FromHub::PackRefused(_) => "pack_refused",
        FromHub::ReadPacksAgain(_) => "read_packs_again",
        FromHub::PinPacks(_) => "pin_packs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole collection gone for one reason is one sentence, with the
    /// count, the size and the reason in it (D148's own example, D152).
    #[test]
    fn a_collection_that_went_all_at_once_is_said_in_one_line() {
        let gone = |path: &str, len: u64, why: &str| herd_proto::AbsentFile {
            path: path.to_owned(),
            len,
            why: why.to_owned(),
            held: 0,
            still_coming: false,
        };
        let swept = "removed 2026-10-30 under the 200 GB output budget";
        let said = one_reason_for(
            &JobId::new("j0000000012"),
            &[
                gone("files/a.png", 300 << 20, swept),
                gone("files/b.png", 38 << 20, swept),
            ],
        )
        .expect("two files gone for one reason is one fact");
        assert!(said.contains("j0000000012"), "{said}");
        assert!(said.contains("2 file(s)"), "{said}");
        assert!(said.contains("338 MiB"), "the size, added up: {said}");
        assert!(said.contains(swept), "and the reason, verbatim: {said}");

        // Two reasons are two facts, and a tidy sentence over them would
        // hide one. There is then no summary and the per-file lines stand.
        assert!(one_reason_for(
            &JobId::new("j0000000012"),
            &[
                gone("files/a.png", 1, swept),
                gone("files/b.png", 1, "never reached this hub"),
            ],
        )
        .is_none());
        // And one file needs no summary: its own line already says it all.
        assert!(one_reason_for(&JobId::new("j1"), &[gone("a", 1, swept)]).is_none());
        assert!(one_reason_for(&JobId::new("j1"), &[]).is_none());
    }

    #[test]
    fn an_exit_code_is_worn_untouched() {
        // E-02 reads these three: pass, fail, absent or leased. 127 is a code
        // a command may choose, and it must not mean anything to herdctl.
        for code in [0, 1, 42, 75, 125, 127, 255] {
            assert_eq!(
                exit_code_for(&JobOutcome::Exited { code }),
                u8::try_from(code).expect("a code in range"),
                "exit code {code} was not worn as it was"
            );
        }
    }

    /// The three lines a falling-back requester reads are told apart by
    /// their prefix alone, never by their prose, and none of them is a
    /// prefix of another — a wrapper grepping for one must not match a
    /// second.
    #[test]
    fn the_three_lines_of_the_fallback_contract_cannot_be_confused() {
        let named = job_id_line(&JobId::new("j0000000007"));
        assert_eq!(named, "herdctl: job-id: j0000000007");
        assert!(named.starts_with(JOB_ID_PREFIX));
        let lines = [JOB_ID_PREFIX, NO_JOB_LINE, JOB_UNKNOWN_LINE];
        for (i, one) in lines.iter().enumerate() {
            for (j, other) in lines.iter().enumerate() {
                assert!(
                    i == j || !other.starts_with(one),
                    "{other:?} begins with {one:?}, so one grep matches both"
                );
            }
        }
        // Every one of them is `herdctl`'s own voice and says so, so a
        // reader of a mixed stream knows who is speaking.
        for line in lines {
            assert!(line.starts_with("herdctl: "), "{line:?}");
        }
        // The two that mean "no id" never look like one.
        for line in [NO_JOB_LINE, JOB_UNKNOWN_LINE] {
            assert!(!line.contains(JOB_ID_PREFIX), "{line:?}");
        }
    }

    #[test]
    fn a_signal_becomes_the_shell_s_hundred_and_twenty_eight_plus_it() {
        assert_eq!(exit_code_for(&JobOutcome::Signalled { signal: 15 }), 143);
        assert_eq!(exit_code_for(&JobOutcome::Signalled { signal: 9 }), 137);
    }

    /// D20: 1 is the code that means "the job failed". An exit code no
    /// process can have worn is Herd with no verdict to give, said as such,
    /// and never a failure a caller would act on. Unreachable from a real unix
    /// process; reachable from a peer that lies.
    #[test]
    fn an_exit_code_no_process_could_wear_gives_no_verdict_rather_than_a_failure() {
        for code in [-1, 256, 1000, i32::MIN, i32::MAX] {
            let outcome = JobOutcome::Exited { code };
            assert_eq!(
                exit_code_for(&outcome),
                NO_VERDICT_CODE,
                "exit code {code} must not be read as a verdict"
            );
            let reason = impossible_exit_code_reason(&outcome).expect("the reason is said");
            assert!(
                reason.contains(&code.to_string()),
                "the reason must name the code: {reason}"
            );
        }
        // A code a process really can wear is still its own, 125 included.
        for code in [0, 1, 125, 255] {
            assert!(
                impossible_exit_code_reason(&JobOutcome::Exited { code }).is_none(),
                "{code} is a code a process can exit with"
            );
        }
    }

    #[test]
    fn a_job_that_never_started_is_the_wrapper_s_own_code() {
        assert_eq!(
            exit_code_for(&JobOutcome::CouldNotStart {
                reason: "no-such-tool: not found".to_owned()
            }),
            125
        );
    }

    /// A job the machine stopped is not a job that failed, so it does not get
    /// a verdict code. 124 is `timeout`'s, and every cap shares it.
    #[test]
    fn a_job_stopped_at_a_cap_is_the_wrapper_s_cap_code() {
        for cap in [Cap::Time, Cap::Memory, Cap::Log, Cap::Disk] {
            assert_eq!(
                exit_code_for(&JobOutcome::StoppedByCap {
                    cap,
                    limit: 600,
                    detail: None
                }),
                124,
                "cap {} did not get the cap code",
                cap.name()
            );
        }
    }

    #[test]
    fn the_cap_sentence_names_the_cap_and_its_figure() {
        let message = stopped_by_cap_message(Cap::Memory, 2_147_483_648, None);
        assert!(message.contains("memory"), "{message}");
        assert!(message.contains("2147483648"), "{message}");
        assert!(
            message.contains("no verdict of its own"),
            "a stopped job must not read as a failing one: {message}"
        );
    }

    /// Finding 2 of M4's verification: a job that wrote four kilobytes and
    /// whose workspace could not be measured was told "the runner stopped
    /// this job at its disk cap of 21474836480 bytes". The figure was true
    /// and the sentence was a lie, and the real reason was only ever on the
    /// runner's stderr. The cap now carries it (D30).
    #[test]
    fn a_cap_whose_figure_would_mislead_says_what_really_happened() {
        let message = stopped_by_cap_message(
            Cap::Disk,
            21_474_836_480,
            Some("this runner could not measure the job's workspace"),
        );
        assert!(
            message.contains("could not measure"),
            "the requester must be told the real reason: {message}"
        );
        assert!(
            message.contains("could not hold it"),
            "and that the cap is what could not be enforced, not what was \
             reached: {message}"
        );
        assert!(
            !message.contains("stopped this job at its disk cap"),
            "the misleading sentence must not be the one printed: {message}"
        );
    }

    #[test]
    fn the_could_not_start_sentence_blames_herd_and_carries_the_reason() {
        let message = could_not_start_message("no-such-tool: not found");
        assert!(
            message.starts_with("herd could not run this job"),
            "{message}"
        );
        assert!(message.contains("no-such-tool: not found"), "{message}");
    }
}
