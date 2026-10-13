//! Herd's wire contract.
//!
//! Every message a hub, a runner or a requester sends lives here and nowhere
//! else (CLAUDE.md invariant 4). The crate does no I/O: it defines the message
//! types, the protocol version, and the JSON framing used on the WebSocket.
//!
//! Framing (D4): one message per WebSocket **text** frame, a serde-tagged
//! JSON object carrying its variant in `"type"`.
//!
//! One payload in here is not text: a job's output. A compiler may write
//! anything at all — a NUL byte, a stray latin-1 byte in a filename — and the
//! record Herd keeps has to be the bytes the process wrote, not a rewriting of
//! them (D18, E-15). Log payloads are therefore `Vec<u8>`, carried as base64
//! inside the JSON frame by [`bytes_as_base64`].

use std::fmt;
use std::time::Duration;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub mod capability;
pub mod placement;

pub use capability::{
    check_label, metal_label, Availability, BusyProbe, Capability, CapabilityError, CapabilityKind,
    CapabilityOffer, CapabilityOrigins, CapabilityState, Contention, Holdings, LabelSet,
    OfferedCapability,
};
pub use placement::{
    default_memory_floor_mib, parallelism_tokens, size_mib, with_parallelism, HostSize,
    Parallelism, PARALLELISM_TOKEN,
};

/// The version announced in [`ToHub::Hello`] and echoed in
/// [`FromHub::Welcome`]. It is bumped whenever a message shape changes in a
/// way an older peer could not read.
///
/// 2 since M6: [`ToHub::SubmitJob`] carries a [`Priority`], and carries it
/// *required*, so that a submission can never acquire a level nobody chose
/// (E-18). That is not an additive change — a version 1 submission has no
/// priority field at all — so the number moves, which is exactly what the
/// handshake's version is for.
///
/// 3 since M7: a runner announces how big its host is and how preferred it
/// is ([`HostSize`], `preference`), and a submission may carry the memory one
/// of its build threads needs, from which the hub derives the job's `-j`
/// (E-22). Both are new fields on existing messages, so an older peer would
/// be *served* rather than refused — and served wrongly: a version 2 runner
/// announces no size, and a hub reading that as a machine of no size would
/// derive nothing for it and place nothing on it. The number moves instead.
/// Both numbers are free-running development counters until M16, where they
/// are set back to 1 (D73).
///
/// 4 since M8: a runner announces the memory it keeps for itself and the
/// tmpfs room jobs may reserve on it ([`HostSize::memory_floor_mib`],
/// [`HostSize::tmpfs_mib`]) and the load it last measured; a submission may
/// carry the whole-job memory and the tmpfs its requester declares. The
/// reason the number moves is the same as M7's, and sharper: a version 3
/// runner announces no floor and no tmpfs, and a hub that read either as zero
/// would admit a job into the memory its host needs to stay alive
/// (invariant 2) or take a reservation on room nobody said was there.
pub const PROTOCOL_VERSION: u32 = 4;

/// How often a runner says it is alive and what it is doing (E-40).
///
/// Here rather than in the runner because both sides depend on the figure: a
/// runner reports at this cadence, and the hub's silence threshold is only
/// meaningful as a multiple of it ([`SILENCE_MARGIN`]). Two crates each
/// holding their own 15 s would be two truths about one contract
/// (invariant 4).
pub const HEALTH_EVERY: Duration = Duration::from_secs(15);

/// How many health reports a silence threshold must be able to miss before
/// it fires.
///
/// Three. One missed report is a scheduler hiccup, a garbage collection, a
/// socket buffer; two is a link having a bad minute. A threshold shorter than
/// three reports does not detect a lost runner, it detects jitter — and what
/// it does with that detection is burn an attempt of a job that is running
/// perfectly well. The deployment's own figures are the shape of this: a
/// report every 15 s, lost after 60 s, which is four.
pub const SILENCE_MARGIN: u32 = 3;

/// Whether a silence threshold is long enough to be about silence, and the
/// sentence to refuse it with when it is not.
///
/// `--lost-after 8` against a 15 s health cadence burnt both attempts of a
/// healthy job in nineteen seconds and ended it `lost`: the hub was not
/// measuring a lost runner, it was measuring the gap between two reports it
/// had asked for. A configuration that cannot work is refused at startup,
/// naming both figures and the margin — never discovered from a job's record
/// afterwards.
///
/// Both sides call this, in three places. The hub checks its `--lost-after`
/// against the cadence it expects its runners to report at; a runner checks
/// how long it believes a silent hub against its own `--health-every`,
/// because the hub answers every report and the mistake is exactly as easy to
/// make in that direction. And the hub checks its threshold against the
/// cadence a runner **really** reports at, announced in
/// [`ToHub::Hello::health_every_ms`] — because two configurations that each
/// pass their own check can still be fatal together: `--lost-after 3` with
/// `--health-every 1` on the hub and `--health-every 10` on the runner spent
/// both attempts of a quiet `sleep 30` in about six seconds and ended it
/// `lost`. Neither side is wrong on its own; only the pair is, and only the
/// handshake can see the pair.
///
/// A cadence of zero used to be read as "no cadence to judge", and let the
/// check through: `--health-every 0` paired with `--health-every 1
/// --lost-after 3` passed every one of these three call sites, the runner
/// never reported, and the same `lost` a mismeasured pair produces followed.
/// Zero is not an unset cadence, it is a runner that never reports at
/// all — so it is refused here, naming the figure, exactly where the three
/// call sites already turn to this function (D44's precedent: a ceiling of
/// zero is refused by name rather than let through as "nothing to measure").
pub fn silence_threshold_refusal(
    threshold_name: &str,
    threshold: Duration,
    interval_name: &str,
    interval: Duration,
) -> Option<String> {
    if interval.is_zero() {
        return Some(format!(
            "{interval_name} is 0s: a health cadence of zero is not a cadence, there is \
             nothing for {threshold_name} to outlast and nothing a runner ever really reports \
             at"
        ));
    }
    let needed = interval.saturating_mul(SILENCE_MARGIN);
    if threshold >= needed {
        return None;
    }
    Some(format!(
        "{threshold_name} is {:?} and {interval_name} is {:?}: a silence threshold has to \
         outlast {SILENCE_MARGIN} health reports ({:?} here), or a runner reporting \
         perfectly well is declared lost between two of its own reports and the job it is \
         running loses an attempt it never spent",
        threshold, interval, needed
    ))
}

/// Which side of the conversation a connecting peer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Offers capacity and executes jobs.
    Runner,
    /// Submits jobs and follows them.
    Requester,
}

/// How a log payload travels inside the JSON frame: standard base64, padded.
///
/// JSON cannot carry an arbitrary byte, and a JSON string cannot carry one
/// either: serialising a job's output as text would turn every invalid UTF-8
/// sequence into U+FFFD and lose it for good. So the bytes are encoded, and
/// what comes back out is byte for byte what went in (D18).
pub mod bytes_as_base64 {
    use data_encoding::BASE64;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        BASE64
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

/// Which of a job's two output streams a log chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// What a requester asks for its job to be weighed at (E-18).
///
/// Four levels, in the first consumer's own words and ordered
/// `merger > step > nightly > producer`: a merger's verification of what is
/// about to land on `main` beats a branch step, which beats the nightly,
/// which beats the cache producer. Four and no more — a number, or a level an
/// operator could invent, is the beginning of a scheduling language, and
/// invariant 1 says no.
///
/// There is deliberately **no `Default`**. A level nobody chose is the defect
/// this type exists to make impossible: it would be invisible on the wire,
/// invisible in the record, and wrong exactly when it mattered. The CLI shows
/// the level it sends, the frame carries it, and the record keeps it.
///
/// The order is spelled out in [`Priority::rank`] rather than derived. A
/// derived `Ord` over this declaration order would make `Merger` the
/// *smallest* value, and "the greater priority wins" would then quietly mean
/// the opposite of what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// A merger's jobs: the check, and the scenarios a merge overlaps (E-43).
    Merger,
    /// The ordinary branch step an agent submits.
    Step,
    /// The scheduled nightly run (E-41).
    Nightly,
    /// The cache producer, which may always give way.
    Producer,
}

impl Priority {
    /// Every level, highest first: the order aging climbs and a help text
    /// lists.
    pub const ALL: [Priority; 4] = [
        Priority::Merger,
        Priority::Step,
        Priority::Nightly,
        Priority::Producer,
    ];

    /// The level's name, as the wire, the record and the CLI all spell it.
    pub fn name(self) -> &'static str {
        match self {
            Priority::Merger => "merger",
            Priority::Step => "step",
            Priority::Nightly => "nightly",
            Priority::Producer => "producer",
        }
    }

    /// How high the level is, 0 being the highest. Small on purpose: the
    /// queue is sorted on this number, and "0 comes first" is the order a
    /// sort already has.
    pub fn rank(self) -> u8 {
        match self {
            Priority::Merger => 0,
            Priority::Step => 1,
            Priority::Nightly => 2,
            Priority::Producer => 3,
        }
    }

    /// Whether this level is served before `other`.
    pub fn outranks(self, other: Priority) -> bool {
        self.rank() < other.rank()
    }

    /// The level one step up, or `None` at the top. What aging climbs.
    pub fn higher(self) -> Option<Priority> {
        match self {
            Priority::Merger => None,
            Priority::Step => Some(Priority::Merger),
            Priority::Nightly => Some(Priority::Step),
            Priority::Producer => Some(Priority::Nightly),
        }
    }

    /// This level raised by `levels` steps, stopping at the top.
    pub fn raised_by(self, levels: u32) -> Priority {
        let mut level = self;
        for _ in 0..levels {
            match level.higher() {
                Some(higher) => level = higher,
                None => break,
            }
        }
        level
    }

    /// How many steps there are between this level and the top: how far
    /// aging has to climb before this job waits behind nothing.
    pub fn steps_to_the_top(self) -> u32 {
        u32::from(self.rank())
    }

    /// Every level's name, for a refusal or a help text.
    pub fn names() -> String {
        Priority::ALL
            .iter()
            .map(|level| level.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    /// Exactly one of the four, refused by name otherwise.
    ///
    /// Never clamped and never guessed: a typo must not become a level. The
    /// four names are in the refusal, so the caller does not have to go
    /// looking for them (E-18, invariant 1).
    fn from_str(text: &str) -> std::result::Result<Self, Self::Err> {
        Priority::ALL
            .into_iter()
            .find(|level| level.name() == text)
            .ok_or_else(|| {
                format!(
                    "{text:?} is not a priority; it is one of {}",
                    Priority::names()
                )
            })
    }
}

/// How a job ended.
///
/// A job's end is not an integer. A process can exit with a code, be killed by
/// a signal, or never start at all — and later milestones add ends that are
/// none of those: M3's lost runner, M8's refusals, M9's deadline. So the end
/// travels as a tagged object, exactly as a message does, and a new way for a
/// job to end is a new variant: purely additive, since no existing frame
/// changes shape. Every variant carries named fields for the same reason, so
/// that one of them can gain a field without breaking the others.
///
/// What this type must never do is fold Herd's own failures into the
/// consumer's exit code. The code in [`JobOutcome::Exited`] is the command's
/// own, untouched, so that 0 means pass, 1 means fail and 75 means "absent or
/// leased" (E-02) — and 127 means whatever the command meant by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobOutcome {
    /// The process ran and exited with this code, reported verbatim.
    Exited { code: i32 },
    /// The process was killed by this signal, and so has no exit code.
    Signalled { signal: i32 },
    /// Herd never got an outcome out of the process: it could not be spawned,
    /// or waiting for it failed. `reason` is one sentence for a human naming
    /// what failed — the executable that was not found, say. This is Herd
    /// failing to run the job, never the job failing.
    CouldNotStart { reason: String },
    /// The runner ended the job because it broke one of the caps its owner
    /// configured (M4). Which cap, and what its figure was, so that a human
    /// reads "stopped at its time cap of 600 s" and never just "killed".
    ///
    /// This is neither the job failing nor Herd failing: the job asked for
    /// more than the machine was willing to give it. A runner must say this
    /// rather than let a job hang or grow without end (invariant 2).
    ///
    /// `detail` is for the case where the cap's figure alone would be a true
    /// number inside a false sentence. A job whose workspace could not be
    /// measured is stopped by the disk cap (D30) and may have written four
    /// kilobytes: "stopped at its disk cap of 21474836480 bytes" is then a
    /// lie, and the real reason belongs in the record rather than only on the
    /// runner's stderr. Absent whenever the cap's own figure says it all.
    StoppedByCap {
        cap: Cap,
        limit: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A requester asked for this job to be cancelled and the runner ended it
    /// (E-06). The process was most likely killed by a signal, and reporting
    /// `signalled` would be true and useless: what a reader needs to know is
    /// that somebody asked for this, so there is no verdict to read and
    /// nothing failed.
    Cancelled { reason: String },
    /// The runner executing this job went silent and the job will not be
    /// attempted again (E-21). D15 named this variant in advance as the one
    /// M3 would have to add, and this is it: a job can end because the
    /// machine it was on stopped answering, which is neither an exit code,
    /// nor a signal, nor a failure to start.
    Lost { reason: String },
    /// No machine could ever run this job, so the hub ended it instead of
    /// queueing it for ever (E-36, M7).
    ///
    /// The one terminal answer the claim step gives, and it is deliberately
    /// narrow: a label nobody offers *today* is a wait, because a machine
    /// that offers it may dial in tomorrow. This is for what no fleet can
    /// fix — a job asking for two labels that exclude each other, or asking
    /// for a derived `-j` with no figure to derive it from. `reason` names
    /// the labels or the figure, because the only thing to do about one of
    /// these is change the submission.
    ///
    /// Distinct from [`JobOutcome::CouldNotStart`], which is a job Herd tried
    /// to run: this one never reached a machine at all.
    Impossible { reason: String },
}

/// Which cap a runner enforced against a job.
///
/// A new cap is a new variant here rather than a new [`JobOutcome`] variant,
/// because every cap ends a job the same way — the runner killed it — and
/// only the figure and the sentence differ. An unknown variant is a decode
/// error naming it, never a job silently read as having exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    /// The job ran longer than the time cap. Its limit is in seconds.
    Time,
    /// A job process allocated past the memory ceiling. Its limit is in bytes.
    Memory,
    /// The job wrote more log than the cap allows. Its limit is in bytes.
    Log,
    /// The job's workspace grew past its disk budget. Its limit is in bytes.
    Disk,
}

impl Cap {
    /// The cap's name, as it reads in a sentence and on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Cap::Time => "time",
            Cap::Memory => "memory",
            Cap::Log => "log",
            Cap::Disk => "disk",
        }
    }

    /// The unit the cap's limit is expressed in.
    pub fn unit(self) -> &'static str {
        match self {
            Cap::Time => "s",
            Cap::Memory | Cap::Log | Cap::Disk => "bytes",
        }
    }
}

/// What a runner measured while a job ran, reported with the outcome and kept
/// in the job's record (E-15).
///
/// A figure a host does not expose is absent, never zero: nobody may read
/// "0 bytes written" as a measurement that was taken. The two figures the
/// runner always knows itself are not optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobMetrics {
    /// The host's one-minute load average when the job ended, times 1000.
    /// An integer on purpose: it lands in a record a consumer compares, and
    /// a float would print two equal measurements differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_load_milli: Option<u64>,
    /// The most memory the job's processes held at once, in bytes. Read from
    /// the job's cgroup where there is one, and otherwise sampled from the
    /// job's own process, which undercounts a process tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_memory_bytes: Option<u64>,
    /// The high-water mark of the job's workspace, in bytes: the most the job
    /// had written and not deleted. This is the figure the disk budget is
    /// enforced against, not a block-layer counter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<u64>,
    /// Bytes of log the job produced, as the runner counted them.
    pub log_bytes: u64,
    /// Wall-clock milliseconds from the spawn to the outcome.
    pub wall_time_ms: u64,
}

/// A job's identity, minted by the hub.
///
/// Opaque on purpose: M1 numbers jobs in memory, M2 makes each one a directory
/// on disk, and no peer may depend on the shape of the string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One job a runner is executing, as its health report names it.
///
/// `chunks_sent` is how many chunks of that job's output the runner has handed
/// to its link since the attempt began — and therefore the number the hub's
/// own count has to agree with. The hub writes the chunks it receives and
/// counts them; when the runner says it has sent more than the hub has
/// written, output is missing, and the hub can ask for it back instead of
/// learning of the loss only when a later chunk arrives with a number that
/// does not follow. That late discovery is the one case M2 could not close:
/// chunks lost with nothing after them look exactly like a job gone quiet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningJob {
    pub job_id: JobId,
    /// Which attempt of that job this runner is executing.
    pub attempt: u32,
    pub chunks_sent: u64,
}

/// Sent to the hub by a runner or a requester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToHub {
    /// First frame on every connection, from either role.
    Hello {
        protocol_version: u32,
        role: Role,
        /// Bearer token. Carried from M1, validated from M15 (D6, Q8).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        /// Human-readable name of the peer, for logs and reports.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// *This process*, as distinct from the name it answers to.
        ///
        /// A name says which machine and which role; it does not say which
        /// run. Without something that does, the hub cannot tell a runner
        /// **redialling** — the same process, whose previous connection the
        /// hub has not noticed the end of — from a **twin**: a second process
        /// configured with the same name. Both look identical in a name, and
        /// the two cases need opposite answers: a redial supersedes the
        /// connection it is replacing, while a twin must not be allowed to
        /// kick a runner that is alive and running a job. Told apart by a
        /// name alone, two live twins supersede each other for ever.
        ///
        /// Minted once per process, carried on every dial that process makes,
        /// and never read for anything but this comparison: it is an
        /// identity, not an address and not a credential (M15's token is the
        /// credential).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
        /// Which hub state root the durable things this peer is holding
        /// belong to, when it is holding any.
        ///
        /// A runner's spool outlives the hub it was produced for (D34, D42),
        /// and a hub's job ids restart at `j0000000001` on a state root that
        /// was recreated. Those two facts meet in a verdict that is
        /// *plausible and false*: the frame names a job id the new hub has
        /// genuinely minted, for an attempt it has genuinely handed out, and
        /// nothing in it says it was produced for a hub whose state no longer
        /// exists. So the state root has an identity, it travels here, and a
        /// peer carrying state from another one is told so by name rather
        /// than believed.
        ///
        /// `None` from a peer holding nothing durable — every requester, and
        /// a runner that has never met a hub.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_root: Option<String>,
        /// How often this runner will report that it is alive, in
        /// milliseconds — its real cadence, not a figure anyone expects of
        /// it (E-40).
        ///
        /// It travels because the mistake it prevents needs **both** sides at
        /// once. Each side alone refuses a silence threshold shorter than
        /// [`SILENCE_MARGIN`] of its own reports, and two individually legal
        /// configurations still destroy a healthy job: a hub losing a runner
        /// after 3 s while expecting a report every 1 s, and a runner really
        /// reporting every 10 s, are each self-consistent, and together they
        /// burn both of a quiet job's attempts in six seconds. Only the
        /// pairing is wrong, so the pairing is what the handshake has to be
        /// able to see.
        ///
        /// `None` from a requester, which reports nothing, and from any peer
        /// that does not say: a hub cannot refuse a figure it was never
        /// given, and it does not pretend to know one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        health_every_ms: Option<u64>,
        /// What this runner offers, with each capability's origin (E-24).
        ///
        /// It rides in the handshake because offering capacity is what
        /// joining *is*: a hub that had a runner but not yet its offer would
        /// have a window in which it could only guess, and guessing is how a
        /// job lands on a machine that cannot run it. Empty from a requester,
        /// and from a runner that offers nothing — which is still a runner
        /// that can run a job needing nothing.
        #[serde(default, skip_serializing_if = "CapabilityOffer::is_empty")]
        capabilities: CapabilityOffer,
        /// How big this host is: the memory and the threads Herd's jobs may
        /// use on it (E-22).
        ///
        /// In the handshake because it is the input to a derivation the hub
        /// makes at claim time, and because it is a property of the machine
        /// rather than of the moment — a figure that moved while a job ran
        /// would change a `-j` already recorded for it. Either figure may be
        /// absent, and an absent figure is absent: never zero.
        #[serde(default, skip_serializing_if = "HostSize::is_silent")]
        host: HostSize,
        /// How preferred this runner is when more than one could run a job
        /// (E-19).
        ///
        /// One integer, higher first, zero by default, declared by whoever
        /// runs the machine. It is the whole of M7's placement preference,
        /// and it is deliberately not a language (invariant 1): the first
        /// consumer's case is one fact — their owner's PC builds 3.1x faster
        /// than their loaded server, so a build belongs there and waits for
        /// it rather than falling back to the server (E-19). A requester
        /// still never names a machine (invariant 6); it is the machines that
        /// say how they rank.
        ///
        /// M12 adds cache locality (E-23) as a further term on top of this,
        /// not a second mechanism.
        #[serde(default, skip_serializing_if = "is_no_preference")]
        preference: i32,
        /// The host's one-minute load average as this runner dialled, times
        /// 1000 (E-32). Recorded, never a gate; see
        /// [`ToHub::Health::load_milli`], which is where it is refreshed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        load_milli: Option<u64>,
    },
    /// Runner reports what its external busy probes say (E-26).
    ///
    /// The whole set every time, not a delta: a snapshot is idempotent, so a
    /// frame nobody saw costs nothing, and a runner that has just redialled
    /// tells the new hub everything by sending one. Sent once on every
    /// connection, and again whenever any probe's verdict changes.
    ///
    /// A capability a probe says is busy is **suspended, not refused**: a job
    /// that needs it waits, with the reason on its record, and runs when the
    /// probe clears (D38, E-26). Nothing here ever fails a job.
    CapabilitiesChanged { capabilities: Vec<CapabilityState> },
    /// Requester asks for work to be done.
    SubmitJob {
        command: Vec<String>,
        /// The capabilities this job needs, by name (E-25). Names only: a
        /// requester says what it needs, never which machine has it
        /// (invariant 6), and never how the capability is governed.
        #[serde(default, skip_serializing_if = "LabelSet::is_empty")]
        labels: LabelSet,
        /// Which of the four levels this job is weighed at (E-18).
        ///
        /// Required, with no `serde(default)`: a submission that does not say
        /// is refused by name rather than given a level nobody chose. That is
        /// what moved [`PROTOCOL_VERSION`] to 2, and it is the whole point —
        /// the one way this field could do harm is by being invisible.
        priority: Priority,
        /// How much memory one of this job's build threads needs, in MiB.
        ///
        /// Present means "derive my `-j` from the machine you place me on"
        /// (E-22): the hub divides the chosen runner's share of memory by
        /// this figure, caps it by the host's threads, records the result and
        /// puts it where the command's [`PARALLELISM_TOKEN`] is. Absent means
        /// the job takes no `-j` and nothing is derived for it.
        ///
        /// A per-thread figure and a whole-job figure are different things,
        /// and this is the per-thread one — 1 792 MiB for the first
        /// consumer's builds. Whole-job memory admission is M8's (E-29).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build_thread_mib: Option<u32>,
        /// How much memory this whole job needs, in MiB (E-29).
        ///
        /// What admission costs the job at: a runner takes it only if its
        /// memory, less the floor it keeps for itself, less what the jobs
        /// already out there declared, covers this figure. Absent means the
        /// job declares no whole-job memory need — which is not zero: it is a
        /// job admission has no figure to weigh, and it is admitted on
        /// capabilities alone.
        ///
        /// Mutually exclusive with `build_thread_mib`: a build declares what
        /// one of its threads needs and the hub multiplies by the `-j` it
        /// derived, a test declares what the whole job needs. A submission
        /// carrying both says two things about one question and is refused by
        /// name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        memory_mib: Option<u64>,
        /// How much room on the runner's tmpfs this job reserves while it
        /// runs, in MiB (E-31).
        ///
        /// Reserved before the job starts and released by the record ending,
        /// so there is nothing to leak (D79). `Some(0)` is a real declaration
        /// — the first consumer's `build` and `check` need no tmpfs at all —
        /// and absent means the same thing said by not saying it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tmpfs_mib: Option<u64>,
    },
    /// An operator raises one job above what its requester asked for (E-18).
    ///
    /// Only ever upwards. Lowering a job is not offered: it is the way to
    /// starve something by hand, and the queue's own aging is what moves a
    /// level in the other direction. The hub answers in one sentence, whether
    /// it raised the job, found it there already, or refused.
    RaiseJob { job_id: JobId, priority: Priority },
    /// Requester asks to be told how a job that already exists ends.
    ///
    /// This is what makes a submission detachable: `herdctl submit` leaves
    /// with the id and nothing else, and a later `herdctl wait <id>` — or
    /// another machine, or the same script an hour afterwards — attaches to the
    /// job by its id alone. A job that is already over is answered at once
    /// from the hub's record, which is why the hub's state has to be durable
    /// for this message to mean anything.
    WaitForJob { job_id: JobId },
    /// Runner reports that it has spawned the job's process.
    JobStarted { job_id: JobId, attempt: u32 },
    /// Runner reports output. `seq` counts chunks per job, across both
    /// streams, from 0, and the runner numbers a chunk as it enqueues it, so
    /// the numbers arrive in the order they were given out.
    ///
    /// What `seq` is for is **gap detection**, not reordering: the hub knows
    /// the number of the last chunk it wrote, and a number that is not its
    /// successor means the chunks in between never arrived — a control
    /// connection that dropped, most often. The hub says so in the job's log
    /// rather than leaving the log quietly short (D29). The single `log` file
    /// is arrival-ordered and promises nothing about which stream a byte came
    /// from, so nothing anywhere reorders by `seq`.
    ///
    /// `data` is the bytes the process wrote, unaltered (D18).
    LogChunk {
        job_id: JobId,
        /// Which attempt produced it. A job can be attempted twice (E-21) and
        /// the second attempt's output is not the first's, so a chunk whose
        /// attempt is over is refused by name rather than written into the
        /// record of the attempt that is current.
        attempt: u32,
        stream: LogStream,
        seq: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Runner reports how the job ended, how much output that attempt
    /// produced in all, and what it measured while it ran.
    ///
    /// `chunks_sent` is what closes the lost-tail case: when the job ends the
    /// hub learns the exact number of chunks the attempt wrote, so a log that
    /// is short says so at once instead of waiting for a chunk that is never
    /// coming. The hub asks for the missing ones back before it records the
    /// end, and names them in the log only once they are truly gone.
    JobFinished {
        job_id: JobId,
        attempt: u32,
        outcome: JobOutcome,
        chunks_sent: u64,
        /// Default for a peer that measured nothing; never silently zero for
        /// a figure that was simply not readable (see [`JobMetrics`]).
        #[serde(default)]
        metrics: JobMetrics,
    },
    /// Runner reports that it is alive and what it is doing (E-40).
    ///
    /// Sent about every 15 s, and the mechanism everything about a lost runner
    /// rests on: the hub keeps the age of the last report, a runner silent for
    /// about 60 s is lost and its running jobs are requeued once (E-21), and
    /// the age is what M13 surfaces. `draining` says the runner is stopping
    /// and takes no new job (invariant 2), so the hub stops handing work to a
    /// runner on its way out the moment it says so.
    Health {
        jobs: Vec<RunningJob>,
        draining: bool,
        /// The host's one-minute load average when this report was made,
        /// times 1000 (E-32).
        ///
        /// **Recorded, never a gate.** E-32 is explicit that a busy host is a
        /// fact to report and not a reason to refuse a job, so this figure
        /// reaches the hub, lands on the line explaining every placement, and
        /// is compared against nothing anywhere. It rides on the health report
        /// rather than the handshake because it is a fact about the moment;
        /// the handshake carries the one the runner measured as it dialled, so
        /// that a job placed before the first report still has a figure.
        ///
        /// Absent when `/proc/loadavg` cannot be read — never zero, because
        /// zero is a measurement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        load_milli: Option<u64>,
    },
    /// Runner gives an attempt back without a verdict, and says why.
    ///
    /// Not an outcome: the job did not end, this *attempt* did. A runner asked
    /// to stop while a job is running abandons it rather than inventing a
    /// verdict for it (invariant 2), and a runner handed a job while it is
    /// draining abandons it before it starts. What becomes of the job — one
    /// more attempt, or an end — is the hub's policy and never the runner's.
    AttemptAbandoned {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// Requester asks for a job to be cancelled (E-06).
    ///
    /// The hub propagates it to whichever runner holds the attempt; a
    /// requester never names a machine (invariant 6).
    CancelJob { job_id: JobId },
}

/// Sent by the hub to a runner or a requester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromHub {
    /// The handshake succeeded.
    Welcome {
        protocol_version: u32,
        hub_version: String,
        /// The identity of the state root this hub is serving from.
        ///
        /// The answer to [`ToHub::Hello`]'s `state_root`, and it has to be
        /// here rather than only compared at the hub: a runner that announced
        /// a state root this hub does not own learns *which* root it is
        /// talking to now, and can deal with what it is holding — by name —
        /// instead of offering a verdict for a vanished hub on every link it
        /// ever makes.
        state_root: String,
    },
    /// The hub declines to go on, and says why. Always the last frame.
    Refused { reason: String },
    /// The submitted job exists and will be followed on this connection.
    JobAccepted { job_id: JobId },
    /// The runner is asked to execute this job, as attempt `attempt` of it.
    ///
    /// The number is the hub's, minted when the attempt is recorded, and the
    /// runner carries it on everything it says about that job afterwards. It
    /// is what lets the hub tell a frame of the attempt it is waiting for from
    /// a frame of an attempt it has already given up on (E-21).
    AssignJob {
        job_id: JobId,
        attempt: u32,
        command: Vec<String>,
    },
    /// The runner is asked to end a job it is executing, and told why.
    ///
    /// A requester asked for it (E-06), or the hub has decided this attempt is
    /// over. Scoped to one attempt of one job: a sibling job on the same
    /// runner is not touched by it.
    CancelJob {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// The runner said something about an attempt that is not its to speak
    /// about, and nothing of it was recorded.
    ///
    /// Two cases, one answer. The attempt is **over**: a runner that was gone
    /// long enough to be declared lost comes back still holding the frames of
    /// its attempt — a result, output, the lot — and the job it was running
    /// may already be on its second attempt. Or the attempt was **never
    /// handed to this runner**: an attempt belongs to the runner the hub gave
    /// it to, and a frame about it from anyone else is a verdict, or a
    /// paragraph of log, about work this peer did not do. A spool file from a
    /// hub whose state root was recreated is exactly that, with no malice and
    /// no hand-editing anywhere.
    ///
    /// Neither may touch the record (E-21), and the runner may not be left
    /// guessing why nothing happens: it is told by name — the reason says
    /// which runner holds the attempt — and it ends whatever it is still
    /// doing for it, spool file included (D42). Not a refusal: `Refused` is
    /// terminal (D13) and this connection goes on, because the runner itself
    /// is fine and may be given work.
    StaleAttempt {
        job_id: JobId,
        attempt: u32,
        reason: String,
    },
    /// Every chunk of this job's output numbered below `through_seq` is on the
    /// hub's disk, and the runner may forget it.
    ///
    /// The same promise as [`FromHub::JobResultAck`], for output: a runner
    /// holds the chunks it has sent until they are recorded, so a link that
    /// dies under a running job costs the log nothing. Sent after the chunk is
    /// appended, never before.
    LogAck {
        job_id: JobId,
        attempt: u32,
        through_seq: u64,
    },
    /// The hub is missing this job's output from `from_seq` onwards, and asks
    /// the runner to send it again.
    ///
    /// What the runner still holds it resends under the numbers it first gave
    /// them; what it no longer holds is genuinely gone, and the hub then names
    /// it in the log rather than leaving the log quietly short (D29).
    ResendLog {
        job_id: JobId,
        attempt: u32,
        from_seq: u64,
    },
    /// Not now, and not never: dial again in `retry_in_ms`.
    ///
    /// A runner redialling into a hub that has not yet noticed its previous
    /// connection is gone used to be told "a runner is already connected" and,
    /// `Refused` being terminal (D13), died of it. So that case is not a
    /// refusal at all: the hub says stand by, names what it is waiting for,
    /// and the runner comes back. D13's own warning, honoured — a peer that
    /// waits is not a peer that was refused.
    StandBy { reason: String, retry_in_ms: u64 },
    /// The hub has the runner's health report, and is itself still there.
    ///
    /// The answer matters as much as the report: a TCP connection can stay
    /// open to a hub that is gone, and a runner that only ever writes would
    /// never find out. Hearing nothing at all from the hub for as long as the
    /// hub would take to call the *runner* lost is a dead link, and the runner
    /// redials.
    HealthRecorded {},
    /// Output relayed to the requester that submitted the job, as bytes
    /// (D18). `seq` is the runner's own numbering, carried through for the
    /// same reason as in [`ToHub::LogChunk`]: telling a gap from a reordering.
    LogChunk {
        job_id: JobId,
        stream: LogStream,
        seq: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// The output a job had already produced when a requester attached to it.
    ///
    /// `herdctl wait` attaches to a job by its id alone, often long after the
    /// job ran, so the stream it would have followed is over. This frame is
    /// what the hub has on its disk: both streams in the order they arrived,
    /// which is how the `log` file keeps them, so the split between stdout and
    /// stderr is not in it. Sent once, before anything else about that job
    /// (D22).
    JobLog {
        job_id: JobId,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Something a requester following a job should know, on a connection that
    /// stays open.
    ///
    /// Not a refusal: `Refused` is terminal (D13), and a note is the opposite
    /// — the job is still there and this connection goes on. M2 sends a note
    /// for each of the two ways a job that is waited for can be going
    /// nowhere: an attempt that was interrupted and whose runner has not come
    /// back (D17), and an attempt whose end the hub cannot write to its disk
    /// (D19). Waiting for such a job is never silent. M8's explained waits are
    /// this frame's real home.
    JobNote { job_id: JobId, note: String },
    /// How the job ended, relayed to the requester that submitted it, with
    /// what the runner measured while it ran.
    JobFinished {
        job_id: JobId,
        outcome: JobOutcome,
        #[serde(default)]
        metrics: JobMetrics,
    },
    /// The hub has the job's outcome on its disk, and the runner may forget it.
    ///
    /// A result is the one thing a runner cannot simply resend from its
    /// memory of the world: the process is gone, so if the result is lost the
    /// work is lost. A runner therefore holds a finished job's outcome until
    /// this frame arrives and redelivers it otherwise (E-44). The hub sends it
    /// only after the record is written and fsynced, never before: an
    /// acknowledgement of something still in memory would be a lie the next
    /// power cut collects on.
    JobResultAck { job_id: JobId },
}

/// The one sentence a cap refusal reads as, so that the runner, the record
/// and `herdctl` all say the same thing and a test can rely on it. It names
/// the cap and its figure, as every Herd refusal must.
pub fn cap_exceeded_reason(cap: Cap, limit: u64) -> String {
    format!(
        "the runner stopped this job at its {} cap of {limit} {}",
        cap.name(),
        cap.unit()
    )
}

/// The same sentence, with the reason a cap carries when its figure alone
/// would mislead ([`JobOutcome::StoppedByCap`]'s `detail`).
///
/// Without a detail this is exactly [`cap_exceeded_reason`]. With one it says
/// the cap could not be *held to* rather than that the job reached it, so
/// nobody reads a job that wrote four kilobytes as a job that filled a disk.
pub fn cap_stop_reason(cap: Cap, limit: u64, detail: Option<&str>) -> String {
    match detail {
        None => cap_exceeded_reason(cap, limit),
        Some(detail) => format!(
            "the runner stopped this job because it could not hold it to its {} cap of \
             {limit} {}: {detail}",
            cap.name(),
            cap.unit()
        ),
    }
}

/// The wording of a version refusal, so that every peer reads one sentence and
/// a test can rely on it. Names both versions, as the handshake must.
pub fn version_mismatch_reason(announced: u32) -> String {
    format!(
        "protocol version mismatch: the hub speaks version {PROTOCOL_VERSION}, \
         the peer announced version {announced}"
    )
}

/// Why a frame about an attempt was not acted on, when the attempt was handed
/// to another runner.
///
/// One sentence, here, because three places need the same one: the hub says
/// it on its own stderr, sends it in [`FromHub::StaleAttempt`], and a test
/// asserts it. It names both runners on purpose — "refused" without "whose
/// attempt this is" leaves an operator with nothing to go on.
pub fn not_this_runner_s_attempt_reason(
    job_id: &JobId,
    attempt: u32,
    holder: &str,
    speaker: &str,
) -> String {
    format!(
        "attempt {attempt} of {job_id} was handed to runner {holder}, and this frame came \
         from runner {speaker}: an attempt belongs to the runner it was given to, so \
         nothing of this is recorded"
    )
}

/// Why nothing of a frame about a job that is over was recorded, naming the
/// job, its state and the attempt the frame claimed.
///
/// A terminal record is the last thing a job ever has: its outcome, its
/// metrics and its log are closed together, and a frame that arrives
/// afterwards is refused *by name* rather than written into a record a reader
/// has already been given. The sentence is here because the hub says it on
/// its own stderr, sends it in [`FromHub::StaleAttempt`], and a test asserts
/// it — one sentence, three readers (invariant 4).
pub fn a_frame_about_a_job_that_is_over_reason(
    job_id: &JobId,
    state: &str,
    attempt: u32,
) -> String {
    format!(
        "{job_id} is over ({state}), and this frame is about attempt {attempt} of it: a \
         record that has reached its end is never written to again — not its outcome, not \
         its metrics, not its log — so nothing of this is recorded"
    )
}

/// Why a peer carrying another state root's state is not served, naming both
/// roots.
///
/// Said to the peer and printed by the hub. The peer is not broken and the
/// hub is not broken: one of them is holding state for a hub that no longer
/// exists, and the only wrong answer is for either side to guess.
pub fn foreign_state_root_reason(hub_root: &str, peer_root: &str) -> String {
    format!(
        "this hub serves the state root {hub_root}, and this peer announced {peer_root}: \
         whatever it is holding — a spooled verdict, above all — belongs to another hub's \
         state, where the same job ids mean different jobs. Nothing of it is believed here. \
         Deal with it and dial again"
    )
}

/// Why a second runner claiming a name already held by a live one is told to
/// wait, naming the name and both instances.
///
/// A configuration mistake said plainly. Two runners on one host is a
/// supported shape — with **two names**; one name claimed twice cannot be
/// served, because every job, every attempt and every record entry would say
/// which runner it was on and mean two machines.
pub fn runner_twin_reason(name: &str, incumbent: &str, newcomer: &str, heard: Duration) -> String {
    format!(
        "runner {name} is already connected and alive — heard from {heard:?} ago — as \
         instance {incumbent}, and this is instance {newcomer}. One name, two live \
         processes: that is a configuration mistake, not a reconnection, and this hub will \
         not take turns killing one of them. Two runners on one host is a supported shape \
         with two names; give this one a name of its own"
    )
}

/// Why a handshake this hub could not read is refused, carrying what was
/// wrong with it.
///
/// The one terminal refusal that comes from a frame rather than from a
/// judgement about peers, and it exists because the alternative was silence.
/// A capability offer is validated when it is decoded — a name that is not a
/// name, one capability offered twice, an exclusion of something that is not
/// a label — so a mistake in a runner's configuration makes its whole `Hello`
/// undecodable. The hub used to drop such a connection, which put the reason
/// on the hub's stdout and gave the runner's owner a reset socket and nothing
/// to read. Dialling again cannot help: the configuration is on the runner's
/// disk and will say the same thing next time, which is exactly D13's shape
/// of "no". So the reason travels, and it is the last frame on that
/// connection.
pub fn unreadable_handshake_reason(what_went_wrong: &str) -> String {
    format!(
        "this hub could not read the handshake: {what_went_wrong}. Nothing was dispatched \
         and dialling again will not change it — the offer is in this peer's own \
         configuration. Correct it and start the peer again"
    )
}

/// Whether a runner declared no preference at all, which is the default.
fn is_no_preference(value: &i32) -> bool {
    *value == 0
}

/// Encode one message as the text of one WebSocket frame.
pub fn to_frame<M: Serialize>(message: &M) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

/// Decode the text of one WebSocket frame into one message.
pub fn from_frame<M: DeserializeOwned>(frame: &str) -> Result<M, serde_json::Error> {
    serde_json::from_str(frame)
}
