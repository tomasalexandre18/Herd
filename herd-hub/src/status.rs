//! What the fleet is doing, as one document in the state root (E-35, E-40).
//!
//! **Why a document and not a frame.** The plan called for a document
//! because a new `FromHub` variant reaching a requester is a breaking
//! change and there would be no second chance to make one. That reason is
//! void — the consumer resets entirely at v1.0.0, so the protocol may move
//! whenever it is useful — and the choice was made again on the merits.
//! Three things decided it, and none of them is compatibility.
//!
//! 1. **A reader takes no lock and writes nothing.** `Status::read` opens
//!    one file read-only. There is no `flock` on `hub.lock`, no
//!    `Mutex<Live>`, no dispatch pass held up, no frame for the hub to
//!    serialise and no connection for it to authenticate. A frame cannot
//!    make that promise: answering one means taking the peers lock on the
//!    serving path, and the question "what is the fleet doing" is asked by
//!    dashboards, in loops, precisely when the hub is busiest.
//! 2. **A hub that has stopped still answers.** The last document stays on
//!    the volume, saying when it was published and whether the hub was on
//!    its way out when it wrote it ([`HubNow::stopping`]). *What was the
//!    fleet doing when it died* is the operator's first question after an
//!    outage, and it is the one question a live query can never answer.
//! 3. **It is where the operator already looks.** Herd is shipped, not
//!    operated (D161, invariant 9): the consumer runs the hub and reads
//!    records off that volume today (E-53). `herd-hub report` and
//!    `herd-hub retention-preview` are the precedent — answer a question
//!    about the state root without touching the hub — and
//!    `herd-hub status` joins them, needing no port, no token, no image and
//!    no `herdctl` build.
//!
//! What a frame would have bought is a live answer rather than the last
//! published one. The gap is bounded by [`AT_MOST_EVERY`] and is not a
//! refresh interval: nothing here runs on a clock, so a quiet hub writes
//! nothing at all, and a change is published as soon as the writer can
//! build it. The honest cost is named rather than hidden — the document
//! carries [`Status::as_of`], so a reader is never guessing how old it is.
//!
//! **Its own version.** [`STATUS_VERSION`] is this document's shape and
//! nothing else's. It is deliberately not `PROTOCOL_VERSION` (what peers
//! say to each other), not `RECORD_VERSION` (what a job's record is) and
//! not `LAYOUT_VERSION` (what the state root holds): a renderer reading
//! this file must be able to tell that *this* shape moved without being
//! told that every record on the volume is unreadable. Q7 asks for the
//! status JSON to carry a format version; this is it.
//!
//! **An unknown is never a zero.** Every figure the hub may not know is an
//! `Option` that is left off the JSON entirely when it is absent, and the
//! text screen prints the word `unknown` for it. This is the standing rule
//! (D27, and D23's for a host figure): a runner whose load cannot be read
//! has no load on this document, not `0`, because a dashboard drawing a
//! flat green line for a figure nobody measured is worse than one drawing
//! a gap. The same goes for a queued job the hub has not yet recorded a
//! reason for ([`QueuedNow::waiting_for`]) and for a runner that announced
//! no build ([`RunnerNow::version`]).
//!
//! **What the hub cannot know, and does not pretend to.** A lease held by
//! something outside Herd (E-27) is not visible here: what is visible is
//! what the machine's own busy probe last said, in the probe's words, under
//! [`CapabilityNow::suspended`]. The hub has no other source and invents
//! none. Nor does this document carry how much of the output budget is in
//! use: the budget is a figure the hub was given and is here, but the use
//! against it is a walk of `jobs/*/files` that `herd-hub report` already
//! does on demand — putting it here would make every publication a walk of
//! the volume.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use herd_proto::{CapabilityOrigins, JobId, Priority, Switch};
use serde::{Deserialize, Serialize};

use crate::job::Timestamp;

/// The shape of `status.json`. See the module's note on why this number is
/// separate from the protocol's, the record's and the layout's.
pub const STATUS_VERSION: u32 = 1;

/// The document's name in the state root.
///
/// Beside `store.json` and `hub.lock` rather than inside `jobs/`, because it
/// is about the hub and not about one job — and because an operator asking
/// "what is going on" should find it by listing the volume's top level.
pub const STATUS_FILE: &str = "status.json";

/// The shortest gap between two publications.
///
/// **Not a refresh interval, and the difference is the whole design.**
/// Nothing wakes up on this clock: the writer sleeps until something
/// changes, and a hub where nothing happens publishes nothing, for ever.
/// All this does is bound how often a *burst* of changes can be written
/// down.
///
/// **What it is bounding, with the arithmetic.** Building one document
/// costs a walk of the hub's own queue — the same `unfinished_jobs()` read
/// a dispatch pass makes — and the bell is rung by every access to the
/// hub's live state, which on a busy fleet is many times a second. Without
/// a floor the publisher would build as fast as it could, so the ceiling on
/// what it costs is `1 / this` queue walks a second: **four**, at this
/// figure, on top of the passes the hub already makes. With 200 jobs queued
/// that is 800 small record reads a second in the worst case and one
/// directory listing a second in the ordinary one.
///
/// **A quarter of a second is a chosen figure, not a measured one.** No
/// bench produced it, and the arithmetic above is a bound rather than a
/// reading: it is picked as the longest delay no human reading a screen
/// would notice, against the cheapest cost that buys. It is sixty times the
/// 15 s health cadence E-40 asks for in the other direction, so no change
/// an operator is waiting on is held perceptibly — and the document carries
/// [`Status::as_of`], so even a reader who cares can tell.
///
/// It is also what makes the "no clock" property testable from outside: a
/// document that has not been rewritten over several seconds of quiet has
/// not been rewritten once, and a change published inside this is a change
/// no clock of any period could have carried
/// (`herd-hub/tests/status.rs`).
pub const AT_MOST_EVERY: Duration = Duration::from_millis(250);

/// Where the document lives under a state root.
pub fn status_path(root: &Path) -> PathBuf {
    root.join(STATUS_FILE)
}

/// What the fleet is doing, as the hub last published it.
///
/// Ordered as an operator reads it: the hub itself, then the machines, then
/// the work that is moving, then the work that is not. Every list is sorted
/// by a stable key — runners by name, jobs by id — so that two publications
/// of the same facts are the same bytes, which is what lets the writer skip
/// a write that would change nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// [`STATUS_VERSION`] as the hub that wrote this had it.
    pub status_version: u32,
    /// When this document was published.
    ///
    /// The one field a reader must look at first: everything below it was
    /// true then, and a document is only as live as the hub that wrote it.
    /// It is also the one field excluded from
    /// [`Status::same_but_for_the_clock`], because a clock that moved is not
    /// news about the fleet.
    pub as_of: Timestamp,
    pub hub: HubNow,
    /// Every runner connected to this hub, by name.
    pub runners: Vec<RunnerNow>,
    /// Every runner approved on this state root that is **not** here.
    ///
    /// The complement of `runners`, and not a decoration: "what runners are
    /// connected" is only actionable beside "and which ones should be". A
    /// fleet of three with one away reads identically to a fleet of two
    /// until this list exists.
    pub away: Vec<AwayNow>,
    /// Every job that has been handed to a runner and has not ended.
    pub running: Vec<RunningNow>,
    /// Every job still in the queue, with what each one is waiting for.
    pub queued: Vec<QueuedNow>,
}

/// The hub itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubNow {
    /// This hub's build, as `herd-hub --version` prints it.
    pub version: String,
    /// Which state this is ([`crate::JobStore::state_root_id`]).
    ///
    /// Here because a job id only means anything inside one state root: a
    /// renderer pointed at a volume somebody rebuilt must be able to tell.
    pub state_root_id: String,
    /// The protocol this hub speaks, and the record shape it writes.
    ///
    /// Two facts a reader of this document cannot get anywhere else without
    /// a shell on the host, and the pair that decides whether a runner or a
    /// state root from elsewhere can join this hub at all (E-51, D73).
    pub protocol_version: u32,
    pub record_version: u32,
    /// The cadence this hub expects a health report at, in milliseconds
    /// (E-40).
    ///
    /// The denominator for every age in `runners`: an age of 40 s means
    /// nothing until you know whether a report was due every 15 s or every
    /// minute. Milliseconds, and an integer, because the protocol says this
    /// cadence in `health_every_ms` already (`ToHub::Hello`) and because a
    /// float in a document is a field two readers round differently.
    pub health_every_ms: u64,
    /// How long a runner may be unheard from before this hub calls it lost
    /// and reattempts its jobs (E-21), in milliseconds.
    pub lost_after_ms: u64,
    /// The hub's own budget for the large files a record carries, in bytes
    /// (D148, D152).
    ///
    /// The figure the hub was given, which is all it knows without walking
    /// the volume. **How much of it is in use is deliberately absent**: that
    /// is a walk of `jobs/*/files`, and `herd-hub report` is where an
    /// operator asks for it.
    pub output_budget_bytes: u64,
    /// True when this hub has been asked to stop and is on its way out.
    ///
    /// The field that tells a stale document from a live one for the right
    /// reason. A document whose `as_of` is twenty minutes old says either
    /// "nothing has changed" or "the hub is gone"; with this set, it says
    /// the second, in the hub's own last word.
    pub stopping: bool,
}

/// One connected runner: what it is, what it lends, what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerNow {
    pub name: String,
    /// The build it announced in its handshake (E-51, D166). Absent from a
    /// peer that announced none, never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Which *process* of that name this is. Absent from a peer that
    /// announced none; two live runners sharing a name is a configuration
    /// mistake the hub refuses to hide, so what it judged them on is here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Where its owner's switch stands, with her reason (E-33).
    pub switch: Switch,
    /// True while it is finishing what it holds and taking nothing new
    /// because it is shutting down (invariant 2). Distinct from a `drain`
    /// switch, which is the owner asking for the same thing.
    pub draining: bool,
    /// How preferred it is when more than one machine could run a job
    /// (E-19). Zero from a runner that declared nothing.
    pub preference: i32,
    /// When this hub last heard from it (E-40).
    ///
    /// **An instant, not an age.** An age written into a file is stale the
    /// moment the file is closed; an instant stays true, and the reader
    /// subtracts it from its own clock. It is also what keeps this document
    /// off a timer: this field changes when a report arrives and at no other
    /// moment, so a quiet fleet produces no writes.
    pub last_health_at: Timestamp,
    /// How many health reports this connection has carried. Zero is a
    /// runner that has only just joined, which is not a runner that has gone
    /// quiet — and the pair of figures is the only way to tell them apart.
    pub health_reports_seen: u64,
    /// The one-minute load average it last reported, times 1000 (E-32).
    ///
    /// Recorded and never a gate. Absent for a machine that cannot read it
    /// and for one that has not reported yet — never `0`, which is a real
    /// reading of an idle host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_milli: Option<u64>,
    pub lends: Lends,
    /// The jobs this machine said it was holding in its last health report,
    /// by id.
    ///
    /// The runner's own word, beside `running` below, which is the records'.
    /// They agree in the ordinary case; when they do not, the disagreement
    /// is the fact worth having — it is what a redial after a link outage
    /// looks like (D182).
    pub holding: Vec<JobId>,
}

/// What one machine lends Herd, as the machine itself declared it.
///
/// Every figure is the runner's, never the hub's: the owner of a machine is
/// the authority on how much of it Herd may have (D23). Absent means the
/// host did not expose it or its owner did not say, and a hub that read an
/// absent figure as zero would be admitting jobs into memory the host needs
/// to stay alive (invariant 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lends {
    /// Memory for Herd's jobs, in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
    /// Of that, what the host keeps for itself: the floor admission
    /// subtracts before it admits anything (E-29).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_floor_mib: Option<u64>,
    /// Hardware threads for Herd's jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    /// The **capacity** of the tmpfs a job may reserve room on, in MiB
    /// (E-31). Not what is free this second: free is this less what the
    /// records say the jobs running here declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmpfs_mib: Option<u64>,
    /// The most jobs it says it will hold at once, as **it** worked the
    /// figure out (D181). Absent is "it did not say", never unlimited and
    /// never zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs_at_once: Option<u32>,
    /// Every capability it offers, by label.
    pub capabilities: Vec<CapabilityNow>,
}

/// One capability a runner offers, and whether it can be given out now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityNow {
    pub label: String,
    /// `exclusive`, `counted` or `shared`, as the wire and the record spell
    /// it.
    pub kind: String,
    /// How many jobs may hold it at once. Absent for `shared`, which is
    /// unlimited — and that is not the same statement as `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Where it came from: probed, declared, denied (E-24).
    pub origins: CapabilityOrigins,
    /// Why nothing may be given it right now, in the probe's own words
    /// (E-26).
    ///
    /// Absent means nothing is holding it up. It covers all three ways a
    /// capability is suspended — a probe that says busy, a probe that could
    /// not be read, and a probe whose first reading has not arrived — and
    /// says which, because "suspended" with no reason is a fleet an operator
    /// cannot act on. What it is **not** is a claim about who holds the
    /// lease: the hub has only the probe's word and offers nothing more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended: Option<String>,
}

/// One runner approved here and not connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwayNow {
    pub name: String,
    /// When it was approved, so an approval nobody ever wired up is
    /// distinguishable from a machine that is merely off (E-48).
    pub approved_at: Timestamp,
}

/// One job that is out on a machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningNow {
    pub id: JobId,
    /// `assigned` (sent, not yet started), `running`, or `interrupted` (an
    /// attempt this hub stopped seeing). All three are jobs a runner is
    /// holding and none of them is in the queue.
    pub state: String,
    /// The machine, by the name it announced.
    pub runner: String,
    /// Which go this is: 1 for the first, 2 after a loss (E-21).
    pub attempt: u32,
    pub priority: Priority,
    pub labels: Vec<String>,
    pub started_at: Timestamp,
    /// The one line that explains this placement, as the attempt recorded it
    /// (E-36): the runner, the labels it answered for, the slot figures, any
    /// derived `-j` and the preference.
    pub why_here: String,
}

/// One job still in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedNow {
    pub id: JobId,
    pub priority: Priority,
    pub labels: Vec<String>,
    pub submitted_at: Timestamp,
    /// Why it has not started, as the hub last recorded it (E-04, E-36).
    ///
    /// **Absent is "no reason recorded yet", not "nothing is wrong".** A job
    /// minted a millisecond ago has not been through a dispatch pass, so
    /// nothing has computed a reason for it; a reader that printed an absent
    /// reason as a blank cell would be saying the hub is fine with this job
    /// waiting. The text screen says `no reason recorded yet` in so many
    /// words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<String>,
    /// How many goes this job has already had, which is how a job waiting
    /// for the first time is told from one that has been handed back twice.
    pub attempts_so_far: u32,
}

impl Status {
    /// Read the document a hub published in this state root.
    ///
    /// `Ok(None)` when there is none: a root no hub has served since this
    /// existed, or one whose hub has not published yet. That is a real
    /// answer and not an error — and it is emphatically not an empty fleet,
    /// which is why it is `None` and not [`Status::default`] (there is no
    /// such thing here on purpose).
    ///
    /// A document of another [`STATUS_VERSION`] is **refused by name, with
    /// both figures**, never read as if it were this one: the rule the
    /// records and the roster already live by (D73).
    ///
    /// **It takes no lock and writes nothing.** One read-only open of one
    /// file, published by `rename`, so what comes back is one whole
    /// publication or the previous one — never half of either. That is the
    /// same guarantee a record's lockless read already gives (ORCH-082),
    /// and the reason this is safe to run in a loop against a serving hub.
    pub fn read(root: &Path) -> io::Result<Option<Self>> {
        let path = status_path(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let status: Self = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a status document: {error}", path.display()),
            )
        })?;
        // A shape this build does not know is **refused by name, with both
        // figures**, and never read as if it were this one. The same rule
        // the records and the roster live by (D73): there is no migration
        // here either, and a renderer handed a document from another Herd
        // has to be told so rather than shown a fleet assembled out of the
        // fields that happened to match.
        if status.status_version != STATUS_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} is a status document of version {}, and this build \
                     reads version {STATUS_VERSION}. There is no migration: \
                     the hub that publishes this root and whatever is \
                     reading it are different versions of Herd.",
                    path.display(),
                    status.status_version,
                ),
            ));
        }
        Ok(Some(status))
    }

    /// Whether these two publications say the same thing about the fleet.
    ///
    /// Everything but [`Status::as_of`], which is why this exists rather
    /// than `==`: the clock moves between every two builds, and a writer
    /// comparing whole documents would rewrite the file on every wake and
    /// turn a change-driven publisher into an fsync loop. What this compares
    /// is the facts, and a hub whose facts have not changed writes nothing.
    pub fn same_but_for_the_clock(&self, other: &Self) -> bool {
        self.status_version == other.status_version
            && self.hub == other.hub
            && self.runners == other.runners
            && self.away == other.away
            && self.running == other.running
            && self.queued == other.queued
    }

    /// How many jobs this document accounts for, running and queued.
    pub fn jobs_not_over(&self) -> usize {
        self.running.len() + self.queued.len()
    }
}

/// How a figure the hub does not know is printed: the word, never a zero.
const UNKNOWN: &str = "unknown";

/// One optional figure, as the screen prints it.
fn or_unknown<T: fmt::Display>(value: Option<T>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => UNKNOWN.to_owned(),
    }
}

impl fmt::Display for Status {
    /// The one text screen E-35 asks for, rendered from this document and
    /// from nothing else.
    ///
    /// Deliberately a render of the JSON rather than a second gatherer: a
    /// screen with its own source would be a second answer to the same
    /// question, and the first time the two disagreed nobody would know
    /// which to believe. It prints `unknown` wherever the document is
    /// silent, so an operator reading the screen and a program reading the
    /// file are told the same thing about what nobody measured.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "herd-hub {} — status document version {}, published {}",
            self.hub.version, self.status_version, self.as_of,
        )?;
        writeln!(
            f,
            "state root {}, protocol {}, records {}",
            self.hub.state_root_id, self.hub.protocol_version, self.hub.record_version,
        )?;
        writeln!(
            f,
            "a health report is expected every {:?} and a runner is lost after \
             {:?}; the output budget is {} bytes (what is used against it: \
             `herd-hub report`)",
            Duration::from_millis(self.hub.health_every_ms),
            Duration::from_millis(self.hub.lost_after_ms),
            self.hub.output_budget_bytes,
        )?;
        if self.hub.stopping {
            writeln!(
                f,
                "THIS HUB WAS STOPPING when it published this. Nothing below \
                 is live.",
            )?;
        }
        writeln!(f)?;

        writeln!(f, "runners connected: {}", self.runners.len())?;
        for runner in &self.runners {
            writeln!(
                f,
                "  {} ({}) switch {}, last heard from {}, {} report(s) seen, \
                 load {}, preference {}{}",
                runner.name,
                or_unknown(runner.version.as_deref()),
                runner.switch.name(),
                runner.last_health_at,
                runner.health_reports_seen,
                or_unknown(runner.load_milli.map(|milli| format!("{milli}/1000"))),
                runner.preference,
                if runner.draining { ", draining" } else { "" },
            )?;
            if let Some(reason) = runner.switch.reason() {
                writeln!(f, "    its owner's reason: {reason}")?;
            }
            writeln!(
                f,
                "    lends {} MiB (keeping {} back), {} thread(s), {} MiB of \
                 tmpfs, at most {} job(s) at once",
                or_unknown(runner.lends.memory_mib),
                or_unknown(runner.lends.memory_floor_mib),
                or_unknown(runner.lends.threads),
                or_unknown(runner.lends.tmpfs_mib),
                or_unknown(runner.lends.jobs_at_once),
            )?;
            if runner.lends.capabilities.is_empty() {
                writeln!(f, "    offers no capability")?;
            }
            for capability in &runner.lends.capabilities {
                writeln!(
                    f,
                    "    {}: {} ({}{})",
                    capability.label,
                    capability.origins,
                    capability.kind,
                    match capability.slots {
                        Some(slots) => format!(", {slots} slot(s)"),
                        None => String::new(),
                    },
                )?;
                if let Some(why) = &capability.suspended {
                    writeln!(f, "      suspended: {why}")?;
                }
            }
            writeln!(
                f,
                "    it says it is holding: {}",
                if runner.holding.is_empty() {
                    "nothing".to_owned()
                } else {
                    runner
                        .holding
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            )?;
        }

        writeln!(f)?;
        writeln!(f, "runners approved and away: {}", self.away.len())?;
        for away in &self.away {
            writeln!(f, "  {} (approved {})", away.name, away.approved_at)?;
        }

        writeln!(f)?;
        writeln!(f, "running: {}", self.running.len())?;
        for job in &self.running {
            writeln!(
                f,
                "  {} {} on {} since {}, attempt {}, level {}, labels {}",
                job.id,
                job.state,
                job.runner,
                job.started_at,
                job.attempt,
                job.priority.name(),
                labels_or_none(&job.labels),
            )?;
            writeln!(f, "    why there: {}", job.why_here)?;
        }

        writeln!(f)?;
        writeln!(f, "queued: {}", self.queued.len())?;
        for job in &self.queued {
            writeln!(
                f,
                "  {} since {}, level {}, labels {}, {} attempt(s) so far",
                job.id,
                job.submitted_at,
                job.priority.name(),
                labels_or_none(&job.labels),
                job.attempts_so_far,
            )?;
            writeln!(
                f,
                "    waiting for: {}",
                job.waiting_for
                    .as_deref()
                    .unwrap_or("no reason recorded yet"),
            )?;
        }
        Ok(())
    }
}

/// A job's labels in a sentence, or the words for a job that asked for none.
///
/// `none` rather than an empty string, because a job holding no label is an
/// ordinary job and an empty cell reads as a formatting fault.
fn labels_or_none(labels: &[String]) -> String {
    if labels.is_empty() {
        "none".to_owned()
    } else {
        labels.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_status() -> Status {
        Status {
            status_version: STATUS_VERSION,
            as_of: Timestamp::at_unix_seconds(1_700_000_000),
            hub: HubNow {
                version: "0.3.7".to_owned(),
                state_root_id: "root-abc".to_owned(),
                protocol_version: 2,
                record_version: 1,
                health_every_ms: 15_000,
                lost_after_ms: 60_000,
                output_budget_bytes: 200_000_000_000,
                stopping: false,
            },
            runners: vec![RunnerNow {
                name: "the-pc".to_owned(),
                version: Some("0.3.7".to_owned()),
                instance: Some("pid-7".to_owned()),
                switch: Switch::On,
                draining: false,
                preference: 3,
                last_health_at: Timestamp::at_unix_seconds(1_699_999_990),
                health_reports_seen: 42,
                load_milli: None,
                lends: Lends {
                    memory_mib: Some(17_000),
                    memory_floor_mib: Some(3_400),
                    threads: Some(12),
                    tmpfs_mib: None,
                    jobs_at_once: Some(2),
                    capabilities: vec![CapabilityNow {
                        label: "kvm".to_owned(),
                        kind: "counted".to_owned(),
                        slots: Some(1),
                        origins: CapabilityOrigins::probed(),
                        suspended: Some("its probe answered 10 (busy)".to_owned()),
                    }],
                },
                holding: vec![JobId::new("j0000000001")],
            }],
            away: vec![AwayNow {
                name: "the-server".to_owned(),
                approved_at: Timestamp::at_unix_seconds(1_600_000_000),
            }],
            running: vec![RunningNow {
                id: JobId::new("j0000000001"),
                state: "running".to_owned(),
                runner: "the-pc".to_owned(),
                attempt: 1,
                priority: Priority::Step,
                labels: vec!["build".to_owned()],
                started_at: Timestamp::at_unix_seconds(1_699_999_900),
                why_here: "claimed by the-pc (build, 1 of 2 slots)".to_owned(),
            }],
            queued: vec![QueuedNow {
                id: JobId::new("j0000000002"),
                priority: Priority::Producer,
                labels: vec![],
                submitted_at: Timestamp::at_unix_seconds(1_699_999_950),
                waiting_for: None,
                attempts_so_far: 0,
            }],
        }
    }

    #[test]
    fn a_figure_the_hub_does_not_know_is_absent_from_the_json() {
        let json = serde_json::to_value(a_status()).expect("a status serialises");
        let runner = &json["runners"][0];
        assert!(
            runner.get("load_milli").is_none(),
            "a load nobody could read must be absent, never 0: {runner}"
        );
        assert!(
            runner["lends"].get("tmpfs_mib").is_none(),
            "a tmpfs capacity nobody could read must be absent, never 0"
        );
        assert!(
            json["queued"][0].get("waiting_for").is_none(),
            "a reason the hub has not computed yet must be absent"
        );
    }

    #[test]
    fn the_screen_prints_unknown_where_the_document_is_silent() {
        let screen = a_status().to_string();
        assert!(
            screen.contains("load unknown"),
            "a load nobody measured reads as unknown: {screen}"
        );
        assert!(
            screen.contains("unknown MiB of tmpfs"),
            "an unread tmpfs capacity reads as unknown: {screen}"
        );
        assert!(
            screen.contains("waiting for: no reason recorded yet"),
            "a queued job with no recorded reason says so: {screen}"
        );
        assert!(
            !screen.contains("load 0"),
            "nothing on this screen may read a missing figure as zero: {screen}"
        );
    }

    #[test]
    fn the_screen_names_the_version_and_the_moment_it_was_published() {
        let screen = a_status().to_string();
        assert!(
            screen.contains(&format!("status document version {STATUS_VERSION}")),
            "the screen names the document's own version: {screen}"
        );
        assert!(
            screen.contains("2023-11-14T22:13:20Z"),
            "the screen names when the document was published: {screen}"
        );
    }

    #[test]
    fn a_hub_that_was_stopping_says_so_on_its_last_word() {
        let mut status = a_status();
        status.hub.stopping = true;
        assert!(
            status.to_string().contains("THIS HUB WAS STOPPING"),
            "a document published on the way out must not read as live"
        );
    }

    #[test]
    fn the_clock_alone_is_not_news_about_the_fleet() {
        let first = a_status();
        let mut later = first.clone();
        later.as_of = Timestamp::at_unix_seconds(1_700_000_099);
        assert!(
            first.same_but_for_the_clock(&later),
            "a document that differs only in when it was built says nothing new"
        );
        later.runners[0].health_reports_seen += 1;
        assert!(
            !first.same_but_for_the_clock(&later),
            "a health report is news and must be published"
        );
    }

    #[test]
    fn a_root_with_no_document_answers_none_and_not_an_empty_fleet() {
        let root = tempfile::tempdir().expect("a temporary root");
        assert_eq!(
            Status::read(root.path()).expect("reading a root with no document"),
            None,
            "a root no hub has published in is not a hub with no runners"
        );
    }

    #[test]
    fn a_document_survives_a_round_trip_through_the_disk() {
        let root = tempfile::tempdir().expect("a temporary root");
        let status = a_status();
        crate::durable::write_file(
            &status_path(root.path()),
            &serde_json::to_vec_pretty(&status).expect("a status serialises"),
        )
        .expect("writing the document");
        let read = Status::read(root.path())
            .expect("reading the document")
            .expect("a document that was just written is there");
        assert_eq!(read, status);
    }

    #[test]
    fn a_file_that_is_not_a_status_document_is_an_error_naming_the_path() {
        let root = tempfile::tempdir().expect("a temporary root");
        crate::durable::write_file(&status_path(root.path()), b"{\"status_version\":")
            .expect("writing a truncated document");
        let error = Status::read(root.path()).expect_err("a truncated document is not read");
        assert!(
            error.to_string().contains(STATUS_FILE),
            "the error names the file that would not parse: {error}"
        );
    }

    #[test]
    fn a_document_of_another_version_is_refused_naming_both_figures() {
        let root = tempfile::tempdir().expect("a temporary root");
        let mut status = a_status();
        status.status_version = STATUS_VERSION + 7;
        crate::durable::write_file(
            &status_path(root.path()),
            &serde_json::to_vec_pretty(&status).expect("a status serialises"),
        )
        .expect("writing the document");
        let error = Status::read(root.path()).expect_err("another version is not read");
        let said = error.to_string();
        assert!(
            said.contains(&(STATUS_VERSION + 7).to_string())
                && said.contains(&STATUS_VERSION.to_string()),
            "the refusal names the version it found and the one it reads: {said}"
        );
    }
}
