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
pub mod declaration;
pub mod outputs;
pub mod placement;
pub mod size;

pub use capability::{
    check_label, metal_label, Availability, BusyProbe, Capability, CapabilityError, CapabilityKind,
    CapabilityOffer, CapabilityOrigins, CapabilityState, Contention, Holdings, LabelSet,
    OfferedCapability,
};
pub use declaration::{
    args_as_line, declared_spans, filled_outside_the_arguments, shell_quote, Declaration, Executor,
    LabelsFrom, Requirements, Step, StepRun, ARGS_TOKEN, COMMIT_TOKEN, IN_TOKEN, JOB_TOKEN,
    LABELS_KEY, LINE_TOKEN, MEMORY_KEY, OUT_TOKEN, READING_KEYS, SCENARIOS_KEY, TMPFS_KEY,
};
pub use outputs::{
    check_relative, JobOutputs, OutputFile, FRAME_CEILING, INLINE_OUTPUT_MAX, OUTPUT_CHUNK_BYTES,
    PER_FILE_OVERHEAD,
};
pub use placement::{
    default_memory_floor_mib, parallelism_tokens, size_mib, with_parallelism, HostSize,
    Parallelism, PARALLELISM_TOKEN,
};
pub use size::{parse_bytes, show_bytes};

/// The version announced in [`ToHub::Hello`] and echoed in
/// [`FromHub::Welcome`], and the first thing the hub checks: a peer that does
/// not announce exactly this number is refused by name
/// ([`version_mismatch_reason`]), whatever else it got right.
///
/// **1 was the first deployed protocol and 2 is the first bump since the
/// freeze** (D73, D148). The development numbers 2 to 8 each existed for a
/// real format break, but every one of those breaks was made on a protocol
/// nothing spoke outside this repository; carrying eight compatibility eras
/// into a deployment would have described a history no peer ever lived. M16
/// set the number back to 1, and from M16 on it is not a development counter —
/// so **this 2 is a compatibility statement, approved by Alexa on
/// 2026-10-16**: M11 needs frames for a job's streamed outputs and for the
/// content registry, both of them new variants, and D124 and D128 between
/// them make a new variant on either enum breaking whatever serde would
/// tolerate.
///
/// **And this is the bump that makes the next one cheaper, by exactly one
/// enum arm.** Both frame enums now carry a `#[serde(other)]` catch-all, so a
/// frame whose `type` a build has never heard of **decodes** instead of
/// failing to. What each peer then does with it is deliberately different,
/// and the difference is the whole value:
///
/// * a **runner** logs it and ignores it, because a frame it does not know is
///   work it is not needed for. Before this, the decode error killed the link
///   and the runner redialled **into the same undecodable frame** — it looped
///   rather than shrugged, which D128 recorded as a defect and did not fix.
///   So a runner-only variant added after this is a log line, not an outage;
/// * a **requester** still ends the run, because one that cannot read what
///   the hub is telling it must not report a verdict (D124). The `bail!` is
///   right and it stays; what the catch-all buys here is a named frame type
///   in the sentence instead of a serde error about an unknown variant;
/// * the **hub** refuses the peer by name rather than ignoring it, because a
///   request it cannot read is work that will never be done, and silence
///   would leave the peer waiting for it.
///
/// None of that makes a `FromHub` variant that can reach a requester free.
/// The rule from D124 and D128 still stands for anything a requester sees;
/// what has changed is that it no longer stands for the runner.
///
/// **What moving it now costs.** The handshake tests equality, not a floor:
/// the day this becomes 2, every runner and every `herdctl` still built
/// against 1 is refused at the handshake and stays refused until its host is
/// rebuilt — a fleet going quiet machine by machine, and requesters that
/// cannot submit, out of one constant. So a bump is a compatibility statement:
/// it needs Alexa's approval, and the order in which the hub and its peers are
/// moved is decided before it lands.
///
/// **What still has to move it.** "Additive" is not the same as "an older peer
/// survives it", and serde only half closes the gap. A frame whose `type` an
/// older build has never heard of now decodes — into the catch-all described
/// below — so it is an *ignorable* message rather than a read error. That is
/// enough for a runner and **not** enough for a requester: `herdctl` refuses
/// the frame on purpose, ending the run it was following with no verdict for
/// a job that may well have passed, which is the shape of D117 and is the
/// right behaviour (D124). So **adding a [`FromHub`] variant that can reach a
/// requester is still a breaking change and still moves this number**,
/// whatever serde would tolerate on the wire. A variant only a runner ever
/// sees no longer is. A new *field* on an existing variant may leave the
/// number alone, and only when it is `#[serde(default)]` *and* means, when
/// absent, exactly what its default says.
///
/// The gap between two *builds* speaking the same protocol is a separate
/// question with a separate answer ([`version_gap`], E-51): a minor gap is
/// served with a note, a major one refused. This number only says whether two
/// peers can read each other's frames at all.
pub const PROTOCOL_VERSION: u32 = 2;

/// How often a runner says it is alive and what it is doing (E-40).
///
/// Here rather than in the runner because both sides depend on the figure: a
/// runner reports at this cadence, and the hub's silence threshold is only
/// meaningful as a multiple of it ([`SILENCE_MARGIN`]). Two crates each
/// holding their own 15 s would be two truths about one contract
/// (invariant 4).
pub const HEALTH_EVERY: Duration = Duration::from_secs(15);

/// How often the hub speaks to a follower that would otherwise hear nothing
/// ([`FromHub::StillThere`]).
///
/// **Deliberately the same number as [`HEALTH_EVERY`]**, and the sameness is
/// the point: "how often a quiet Herd link speaks" is one figure to remember
/// and not two. The two cadences are the two directions of one problem — a
/// runner says it is alive to the hub, and the hub says it is alive to
/// whoever is following a job — so a reader who knows 15 s knows both, and an
/// operator who shortens one has an obvious name for the other.
///
/// Here rather than in the hub because it is part of what a follower is
/// promised: a connection following a job that is not over carries a frame at
/// least this often, whatever the job is doing (invariant 4). No client acts
/// on the figure — nothing dials on it, nothing times out on it — which is
/// why the frame it paces needed no protocol break to add.
pub const STILL_THERE_EVERY: Duration = Duration::from_secs(15);

/// How much of a job's recorded log one [`FromHub::JobLog`] frame carries:
/// **8 MiB**.
///
/// A job's log is the one piece of a record that was already large, and until
/// this figure existed the backlog travelled **whole**: a `herdctl wait <id>`
/// on a job that was over got one frame holding everything the job had ever
/// printed. The default log cap is 64 MiB, base64 makes that about 85 MiB,
/// and [`FRAME_CEILING`] is 64 MiB — so a job that printed more than about
/// 48 MiB produced a frame neither side could put through a socket. What a
/// requester saw was the link closing with no verdict and exit 125, which is
/// D117's exact shape, for a job that may well have passed. D131 and D137
/// derived the *output* cap from the frame and never came back to the
/// neighbour the first 64 MiB had been taken from.
///
/// So the backlog travels in pieces, and this is the piece. Several frames
/// concatenate to exactly the bytes one frame carried — `JobLog` has always
/// meant "here are bytes of this job's log", and a requester writes them out
/// in the order they arrive — so this needs no variant, no field and no
/// [`PROTOCOL_VERSION`] bump, which is the one change D128 leaves the number
/// alone for.
///
/// **Why 8 MiB and not the 32 MiB an output may be.** Two bounds, and the
/// smaller wins. Base64 puts 8 MiB on the wire as about 10.7 MiB, leaving
/// five-sixths of the ceiling unused — room that is deliberate, because this
/// figure has to stay right if an operator raises the log cap. And D138's
/// real objection to a big frame was never the ceiling but the **event
/// loop**: a frame costs its whole encode and decode before anything else is
/// served, and this one is sent to a follower whose link lives by a 15 s
/// cadence ([`STILL_THERE_EVERY`]). Tens of milliseconds a frame is inside
/// that cadence by three orders of magnitude; the few hundred milliseconds
/// 32 MiB measured is inside it by one.
pub const JOB_LOG_FRAME_BYTES: usize = 8 << 20;

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

/// The owner's switch on one runner (E-33).
///
/// The one thing in Herd that belongs to the person whose machine it is, and
/// the condition on which she lends it at all: three positions, honoured in
/// **ten seconds**, settable from her own machine and from an orchestrator,
/// with the reason she gave visible wherever the position is.
///
/// It is **not** the shutdown drain. A runner asked to stop by `SIGTERM` gives
/// the machine back inside five seconds and abandons what it is running
/// (invariant 2); this says something slower and more useful — *stop taking
/// work* — and leaves the job in flight alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "position", rename_all = "snake_case")]
pub enum Switch {
    /// The ordinary position: this runner claims work.
    On,
    /// Withdrawn by its owner. Nothing new is claimed, **nothing is served to
    /// a peer** (D11), the jobs it already holds run to their end, and
    /// everything queued for it is claimable elsewhere at once (D80). The
    /// runner stays connected, so the reason stays visible and `on` can be set
    /// from anywhere.
    Off { reason: String },
    /// Finish and leave. Nothing new is claimed, what is held runs to its end,
    /// and the runner disconnects once it holds nothing — the owner's way of
    /// saying "I want this process gone, but not at the cost of the build it
    /// is running".
    Drain { reason: String },
}

impl Switch {
    /// Whether this runner may be given new work.
    pub fn claims(&self) -> bool {
        matches!(self, Switch::On)
    }

    /// Whether this runner may serve content to a peer (D10, D11).
    ///
    /// **The one gate**, and it is a method on the protocol's own type rather
    /// than a line inside the registry M11 brings, because `off` is the owner
    /// saying "this machine is mine again": a runner that stopped claiming
    /// jobs while still pushing a 400 MiB pack up her link would have
    /// honoured half of E-33. A drain is the other way round — it is
    /// finishing work it already holds, and a peer halfway through fetching
    /// what that work produced is part of finishing it.
    pub fn serves_content(&self) -> bool {
        !matches!(self, Switch::Off { .. })
    }

    /// Whether the runner leaves once it holds nothing.
    pub fn leaves_when_empty(&self) -> bool {
        matches!(self, Switch::Drain { .. })
    }

    /// The position's own word: `on`, `off`, `drain`.
    pub fn name(&self) -> &'static str {
        match self {
            Switch::On => "on",
            Switch::Off { .. } => "off",
            Switch::Drain { .. } => "drain",
        }
    }

    /// The reason its owner gave, when the position carries one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Switch::On => None,
            Switch::Off { reason } | Switch::Drain { reason } => Some(reason),
        }
    }

    /// The position and its reason, as every line that mentions it says them.
    ///
    /// One formatter, because this clause lands in four places — the runner's
    /// own stderr, the hub's log, the sentence explaining why a job is not
    /// going to that machine, and `herdctl switch` — and an operator grepping
    /// for the reason she typed has to find it spelled one way.
    pub fn sentence(&self) -> String {
        match self {
            Switch::On => "on".to_owned(),
            Switch::Off { reason } => format!("off ({reason})"),
            Switch::Drain { reason } => format!("draining ({reason})"),
        }
    }

    /// Read a position from the word an owner typed, with the reason she gave.
    ///
    /// A reason is **required** for `off` and for `drain`, and refused for
    /// `on`: E-33 asks for `off <reason>` in so many words, and a machine
    /// withdrawn for no stated reason is a machine nobody can decide to switch
    /// back on.
    pub fn parse(position: &str, reason: Option<&str>) -> Result<Self, String> {
        let said = reason.map(str::trim).filter(|reason| !reason.is_empty());
        match (position, said) {
            ("on", None) => Ok(Switch::On),
            ("on", Some(_)) => Err(
                "switching a runner on takes no reason: a reason is what explains a machine \
                 being withdrawn"
                    .to_owned(),
            ),
            ("off", Some(reason)) => Ok(Switch::Off {
                reason: reason.to_owned(),
            }),
            ("drain", Some(reason)) => Ok(Switch::Drain {
                reason: reason.to_owned(),
            }),
            ("off" | "drain", None) => Err(format!(
                "switching a runner {position} needs a reason (--reason): the position is \
                 visible to everyone whose job is not running, and a withdrawal nobody can \
                 explain is one nobody can undo"
            )),
            (other, _) => Err(format!(
                "{other:?} is not a switch position; it is one of on, off, drain"
            )),
        }
    }
}

impl Default for Switch {
    /// `on`, because a runner nobody has withdrawn is a runner that claims.
    /// Used for what an absent switch file means, and nowhere on the wire:
    /// the field is required there.
    fn default() -> Self {
        Switch::On
    }
}

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence())
    }
}

/// How long E-33 gives a position to be honoured.
///
/// Ten seconds, and it lives here because both sides spend it: the runner
/// reads its file on a short clock inside this budget, and the hub bounds the
/// wait for [`ToHub::SwitchApplied`] by it before it answers `herdctl switch`
/// with what it does and does not know (D99). Two crates each holding their
/// own ten would be two truths about one promise (invariant 4).
pub const SWITCH_HONOURED_WITHIN: Duration = Duration::from_secs(10);

/// Whether a position a runner applied will outlive the runner (E-33, D99).
///
/// The difference this draws is the whole of D99. A position is *in force* the
/// moment the runner applies it — it stops claiming, the hub is told, the queue
/// is re-judged. A position is *durable* only once it is on the machine's own
/// disk, and that is the half an owner is actually relying on: a runner killed
/// before the file lands comes back in its previous position while she believes
/// her machine is off.
///
/// So the two are reported apart, and the hub never calls the first one
/// success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SwitchPersistence {
    /// In force **and** on the runner's disk, fsynced: a machine killed now
    /// comes back on this position, with the reason its owner gave.
    Written { path: String },
    /// In force on the running process and nowhere else, so a restart would
    /// forget it. A read-only data root, a full disk, or a runner started with
    /// no switch file at all. Never reported to an owner as plain success: what
    /// she asked for is a machine she can take back, and a position that dies
    /// with the process is only half of that.
    NotWritten { why: String },
}

impl SwitchPersistence {
    /// Whether the position will outlive the runner process.
    pub fn is_durable(&self) -> bool {
        matches!(self, SwitchPersistence::Written { .. })
    }
}

/// What one runner's switch is, as the hub reports the fleet's (E-33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerSwitch {
    pub name: String,
    pub switch: Switch,
    /// How many jobs it is running. A machine switched off with a job still on
    /// it is the ordinary case and not a contradiction: its work finishes.
    pub jobs: u32,
    /// Whether it is also stopping for good (`SIGTERM`, invariant 2).
    pub stopping: bool,
}

/// One approved runner, as `herdctl runner list` prints it (E-48).
///
/// No token and no hash: the hub holds a hash and hands out neither. What an
/// operator needs is the name she approved, when she approved it, and whether
/// the machine is here now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedRunner {
    pub name: String,
    /// When the approval was made, in seconds since the epoch.
    pub approved_unix_s: u64,
    /// Whether a runner of that name is connected to this hub right now.
    pub connected: bool,
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

/// The same encoding for bytes that may not be on the frame at all
/// ([`OutputFile::data`]).
///
/// Separate from [`bytes_as_base64`] rather than generic over the option,
/// because the distinction it keeps is load-bearing: absent means the bytes
/// are elsewhere, and an empty string means a file of zero bytes. A single
/// codec that read one as the other would turn "the bytes are streaming" into
/// "the file is empty", which is the kind of quiet wrong answer a results
/// file should never be able to give.
pub mod opt_bytes_as_base64 {
    use data_encoding::BASE64;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => serializer.serialize_str(&BASE64.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        let Some(text) = Option::<String>::deserialize(deserializer)? else {
            return Ok(None);
        };
        BASE64
            .decode(text.as_bytes())
            .map(Some)
            .map_err(serde::de::Error::custom)
    }
}

/// The sha256 of some bytes, lowercase hex.
///
/// One function, here, for the reason the protocol holds every other shared
/// figure: a runner computes the digest a manifest announces and the hub
/// verifies a streamed file against it, and two implementations of "the
/// sha256 of this file" would be two truths about one contract
/// (invariant 4). The hex is lowercase and that is part of the contract, not
/// a formatting preference — a comparison is a string comparison.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut rolling = Sha256Rolling::new();
    rolling.update(bytes);
    rolling.finish()
}

/// The same digest over bytes that arrive a piece at a time.
///
/// Both sides of a streamed file need this and neither can hold the file: a
/// runner hashes 400 MiB off its disk to announce it, and the hub hashes the
/// chunks as they land to verify it. A `sha256_hex` of the whole thing would
/// mean reading it into memory, which is the cost the streamed carriage
/// exists to avoid.
///
/// It is here beside [`sha256_hex`] so the two cannot disagree about the
/// hex — one of them formatting uppercase would make every verification fail
/// for a reason nobody would look for (invariant 4).
pub struct Sha256Rolling(sha2::Sha256);

impl Sha256Rolling {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        use sha2::Digest;

        Self(sha2::Sha256::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest;

        self.0.update(bytes);
    }

    pub fn finish(self) -> String {
        use sha2::Digest;
        use std::fmt::Write;

        let digest = self.0.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
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
    /// D74 adds one more, and it is of the same kind: a job whose
    /// requirements are read on a machine, whose reading did not answer. No
    /// machine can run such a job because nothing knows what it needs, and
    /// the only thing to do about it is to fix the reading — so `reason`
    /// names the reading, the command and what was wrong with its answer.
    ///
    /// Distinct from [`JobOutcome::CouldNotStart`], which is a job Herd tried
    /// to run: this one never reached a machine at all.
    Impossible { reason: String },
}

impl JobOutcome {
    /// Whether this is a job that **passed**: it ran and exited nought.
    ///
    /// The one definition, because three places needed it and two of them
    /// had written it out (D79). Everything else is a job that did not pass,
    /// and the distinction is deliberately that coarse: a test that exited 1,
    /// a step that could not start, a runner that went away and a job somebody
    /// cancelled are four different failures, and what they have in common is
    /// the only thing a reader of a *green* dashboard cares about. The
    /// sweeper reads it to decide which evidence is worth keeping longest
    /// (D152), and a passing job's evidence is the evidence nobody ever opens.
    pub fn is_success(&self) -> bool {
        matches!(self, JobOutcome::Exited { code: 0 })
    }
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
    /// How stalled the host and the job's own cgroup were when the job ended
    /// (E-32, D119). Absent on a kernel without PSI, or where no figure at
    /// all could be read — never a zero, which reads as "no pressure".
    ///
    /// `#[serde(default)]` and absent means exactly that: a peer that has
    /// never heard of pressure sends none and a peer reading an older frame
    /// or an older record finds none, which is the truth in both cases. That
    /// is the one change allowed to leave [`PROTOCOL_VERSION`] and the hub's
    /// record version alone (D73), and it is why this is a field on an
    /// existing variant rather than a frame of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<Pressure>,
}

/// How much time was lost to waiting for a resource, as the kernel's pressure
/// stall information reports it (E-32, D119).
///
/// **Recorded, never a gate.** The first consumer asked for pressure to brake
/// admission and that was refused: E-32 is their own expectation that the load
/// figure is recorded and never a criterion, and a hub that silently stopped
/// dispatching would be indistinguishable from a hub that had died. So these
/// figures travel with a job's outcome, land in its record, and are compared
/// against nothing anywhere in Herd.
///
/// Each figure is `some avg10` from one `pressure` file — the share of the
/// last ten seconds in which **at least one** task was stalled on that
/// resource — as a percentage times 1000, the same `_milli` scale
/// [`JobMetrics::host_load_milli`] uses. `12.34 %` is `12340`, and the full
/// range is `0..=100_000`. An integer because it lands in a record a consumer
/// compares, and two equal measurements must not print differently.
///
/// The `host_*` figures come from `/proc/pressure/*` and are about the whole
/// machine, including everything on it that is not Herd. The `job_*` figures
/// come from the job's own cgroup and are about that job alone; they are
/// absent where the kernel does not expose per-cgroup pressure or where the
/// job had no cgroup of its own.
///
/// **Every figure is absent rather than zero when it could not be read**
/// (D23): a zero says "nothing was stalled", which is a measurement, and a
/// consumer reading one off an unreadable file would be reading a lie.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pressure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_cpu_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_memory_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_io_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_cpu_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_memory_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_io_milli: Option<u64>,
}

impl Pressure {
    /// Whether nothing at all could be read. A reading with no figure in it
    /// is **not** a reading of no pressure, so the caller reports `None`
    /// rather than an empty one.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The figures that are there, named as an operator reads them. Empty for
    /// a host that exposes no pressure at all, which is how the absence stays
    /// visible instead of printing as zeroes.
    pub fn named(&self) -> Vec<(&'static str, u64)> {
        [
            ("host cpu", self.host_cpu_milli),
            ("host memory", self.host_memory_milli),
            ("host io", self.host_io_milli),
            ("job cpu", self.job_cpu_milli),
            ("job memory", self.job_memory_milli),
            ("job io", self.job_io_milli),
        ]
        .into_iter()
        .filter_map(|(name, figure)| figure.map(|figure| (name, figure)))
        .collect()
    }
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
        /// Which **build** is speaking, beside which protocol it speaks
        /// (E-51).
        ///
        /// The protocol version says whether two peers can read each other's
        /// frames; it says nothing about whether they agree on what the
        /// frames *mean*. A runner built three releases ago reads every field
        /// of today's `AssignJob` and may still confine a job differently,
        /// report a step differently, or spool a verdict in a shape the hub
        /// has stopped reading — all of it inside one protocol version. So
        /// the build travels too, and the hub answers a minor gap with a note
        /// and a major one with a refusal, both naming the two versions so
        /// that one line says which side to move.
        ///
        /// **Required**, for the reason the switch is: a hub that read a
        /// missing build version as "the same as mine" would warn about
        /// nothing and refuse nothing, which is E-51 not implemented. That is
        /// what moved [`PROTOCOL_VERSION`] to 8.
        peer_version: String,
        role: Role,
        /// Bearer token: what says this peer is allowed here (E-48, Q8).
        ///
        /// Carried from M1 and **validated from M15**. A runner's token is
        /// the one minted when an operator approved it by name (`herdctl
        /// runner approve`), kept at the hub only as a hash, and a runner
        /// that carries none — or the wrong one, or one that was revoked when
        /// the runner was removed — is refused terminally (D13) with a
        /// sentence saying what to do about it. A requester's is the one
        /// shared token the hub was started with, when it was started with
        /// one.
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
        /// Where its owner's switch stands (E-33).
        ///
        /// In the handshake because a switch outlives the runner process: an
        /// owner who switches her PC off and reboots it has not changed her
        /// mind, and a runner that rejoined `on` would be a machine that gave
        /// itself back. **Required, with no `serde(default)`**, for the reason
        /// the priority is: a hub that read a missing switch as `on` would
        /// hand work to a withdrawn machine, which is the one thing E-33
        /// exists to prevent. That is what moved [`PROTOCOL_VERSION`] to 5.
        switch: Switch,
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
    ///
    /// Everything it asks for is one [`Asked`], shared with [`ToHub::DryRun`]
    /// — which is E-07's whole safety property. A dry run that described its
    /// job in a second shape would be a dry run whose answer is about a
    /// slightly different job than the one a submission makes, and that is
    /// exactly the lie E-07 must not tell.
    SubmitJob { asked: Asked },
    /// Requester asks where a job **would** run, and for nothing to happen
    /// (E-07).
    ///
    /// Answered with [`FromHub::WouldRun`]. Nothing is minted, nothing is
    /// written, no slot and no reservation is taken: the hub judges the
    /// hypothetical job through the same claim step a submission goes
    /// through, over a copy of the fleet it throws away.
    DryRun { asked: Asked },
    /// Requester or owner sets one runner's switch (E-33).
    ///
    /// The orchestrator's half of E-33. The owner's own half needs no frame at
    /// all: she writes the switch file on her machine, which is what keeps
    /// invariant 3 — nothing listens on a runner.
    SetSwitch { runner: String, switch: Switch },
    /// Runner reports what it did with the position the hub asked it for
    /// (E-33, D99).
    ///
    /// **The acknowledgement `herdctl switch` waits for.** Before D99 the only
    /// thing that came back was the switch on the next health report, and a
    /// health report says the position is *in force*, not that it is on the
    /// machine's disk — the runner applies a position it could not write and
    /// says so on stderr, which is the right thing to do and indistinguishable
    /// from here. So the two are separated: this frame carries the position and
    /// whether it is durable, and the hub answers the requester with what this
    /// frame said or with the fact that it never arrived.
    ///
    /// It travels on the control connection the runner already holds, because
    /// invariant 3 says nothing listens on a runner. Sent only for a position
    /// the hub asked for: an owner setting her own switch file asked no one
    /// anything, and the hub's view of her machine converges on the next health
    /// report as it always did.
    SwitchApplied {
        switch: Switch,
        persistence: SwitchPersistence,
    },
    /// Requester asks where every runner's switch stands (E-33).
    AskFleetSwitches {},
    /// An operator approves one runner by name, and the hub mints its token
    /// (E-48, Q8).
    ///
    /// **One action.** Approving a runner that is already approved mints it a
    /// new token and forgets the old one, which is how a token that has
    /// leaked is replaced: there is no second command for that, because a
    /// second command is a second thing to get wrong.
    ApproveRunner { runner: String },
    /// An operator removes one runner, which revokes its token (E-48).
    ///
    /// The machine is told, on the connection it is holding, and claims
    /// nothing from that moment: a revocation that only took effect at the
    /// next dial would leave a runner Herd no longer trusts holding a link
    /// and taking work.
    RemoveRunner { runner: String },
    /// An operator asks which runners are approved on this hub (E-48).
    AskApprovedRunners {},
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
        /// Every executor step this attempt ran, in the order it ran them
        /// (D77).
        ///
        /// The authoritative copy: [`ToHub::StepFinished`] fills the record as
        /// the attempt goes, so a job stopped at a cap or lost with its runner
        /// still shows the preparation it did — and this list, which the hub
        /// takes over the ones it already has, is what an attempt that reached
        /// its end says it ran. Empty for a job that is not a declared command.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        steps: Vec<StepRun>,
        /// What the job left under its `{out}` directory, collected before
        /// the workspace was reclaimed, up to the runner's output cap
        /// ([`JobOutputs`]).
        ///
        /// Default — nothing — from an ad-hoc job, which has no `{out}`, and
        /// from any peer that does not say: absence means this runner
        /// collected no output file, which is exactly what a build predating
        /// the field means.
        #[serde(default, skip_serializing_if = "JobOutputs::is_silent")]
        outputs: JobOutputs,
    },
    /// One executor step of an attempt has ended, with its figures (D77).
    ///
    /// Sent as the step ends rather than only with the verdict, for two
    /// reasons. A 5 h nightly stopped at its cap, or a runner lost under a
    /// 30-minute cache pull, would otherwise leave **no** step on the record —
    /// the preparation time invisible in exactly the case someone is looking
    /// for it. And E-04's and E-36's explanations may quote preparation time
    /// (D77), which they can only do if the hub holds the step before the job
    /// ends.
    ///
    /// Not acknowledged and not spooled, unlike a log chunk or a verdict: a
    /// step line is neither output nor a verdict, and the complete list
    /// arrives again with [`ToHub::JobFinished`]. A link that dies under it
    /// costs the record nothing it does not get back.
    StepFinished {
        job_id: JobId,
        attempt: u32,
        step: StepRun,
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
        /// Where its owner's switch stands (E-33).
        ///
        /// Repeated on every report, and sent **at once** when it moves rather
        /// than waiting for the next one: E-33's ten seconds is a promise
        /// about the hub stopping, not about the runner noticing. The report
        /// is what makes the position converge anyway — a `set_switch` frame
        /// the runner never received leaves the hub's view wrong for one
        /// cadence and no longer.
        switch: Switch,
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
    /// One piece of one file of a job's streamed collection (D148).
    ///
    /// **The file is not announced here, because it was announced on the
    /// verdict.** `JobFinished` carried the whole manifest — every path, its
    /// length and its sha256 — before a byte of any file moved, so by the
    /// time a chunk arrives the hub already knows how long this file is and
    /// what it must hash to. That is why there is no `job_file_begin` frame:
    /// the manifest *is* the announcement, it is made once for the collection
    /// rather than once per file, and it is made on a frame that was going to
    /// be sent anyway.
    ///
    /// `offset` is where these bytes start, and it is checked rather than
    /// trusted. A resumed transfer is the whole reason it is on the wire: the
    /// hub holds a part-written file of a known length, tells the runner so
    /// ([`FromHub::JobFilesHeld`]), and a chunk that does not begin exactly
    /// where the hub's own bytes end is refused by name instead of splicing
    /// two different reads of the same path into one file that then fails its
    /// hash for a reason nobody can see.
    JobFileChunk {
        job_id: JobId,
        /// Which attempt produced it, for the reason [`ToHub::LogChunk`]
        /// carries one: a second attempt's files are not the first's.
        attempt: u32,
        /// Relative to `{out}`, directories preserved, and named on the
        /// manifest. A path that is not on it is refused — the hub never
        /// writes a file a verdict did not promise.
        path: String,
        offset: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Requester asks for the files of a job it holds the manifest of (D148).
    ///
    /// **Only the streamed carriage ever sends this**, and that is the point
    /// of its existing. Under the inline threshold a collection's bytes are
    /// already on the verdict frame, so nothing is missing and nothing is
    /// asked for — D137's single round trip is untouched. Above it the bytes
    /// are on the hub's disk and this is how they are called for, which costs
    /// one frame against a payload of hundreds of megabytes and means the hub
    /// never pushes bytes at a requester that did not want them.
    ///
    /// A requester that asks for files it already has asks for nothing: it
    /// names only what it is missing.
    AskJobFiles {
        job_id: JobId,
        /// The paths wanted. Empty means every path on the manifest.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        paths: Vec<String>,
    },
    /// A frame of a type this build has never heard of.
    ///
    /// **The hub answers this with a refusal naming the type, never with
    /// silence.** A request it cannot read is work that will never be done,
    /// and a peer told nothing would wait for it — which is the failure a
    /// catch-all is supposed to prevent and not a cheaper version of it. The
    /// other direction is the opposite ([`FromHub::Unknown`]): a *runner*
    /// ignores what it does not know, because a frame it has never heard of
    /// is work it is not needed for.
    #[serde(other)]
    Unknown,
}

/// Everything a requester asks for about one job (E-07).
///
/// One type, carried by [`ToHub::SubmitJob`] and by [`ToHub::DryRun`] alike,
/// because E-07 asks where a job *would* go and the only useful answer is the
/// one the real claim would give. Two shapes would be two jobs, and the dry
/// run would be answering about the second one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked {
    pub command: Vec<String>,
    /// The capabilities this job needs, by name (E-25). Names only: a
    /// requester says what it needs, never which machine has it (invariant 6),
    /// and never how the capability is governed.
    #[serde(default, skip_serializing_if = "LabelSet::is_empty")]
    pub labels: LabelSet,
    /// Which of the four levels this job is weighed at (E-18).
    ///
    /// Required, with no `serde(default)`: a submission that does not say is
    /// refused by name rather than given a level nobody chose. That is what
    /// moved [`PROTOCOL_VERSION`] to 2, and it is the whole point — the one
    /// way this field could do harm is by being invisible.
    pub priority: Priority,
    /// How much memory one of this job's build threads needs, in MiB.
    ///
    /// Present means "derive my `-j` from the machine you place me on" (E-22):
    /// the hub divides the chosen runner's share of memory by this figure,
    /// caps it by the host's threads, records the result and puts it where the
    /// command's [`PARALLELISM_TOKEN`] is. Absent means the job takes no `-j`
    /// and nothing is derived for it.
    ///
    /// A per-thread figure and a whole-job figure are different things, and
    /// this is the per-thread one — 1 792 MiB for the first consumer's builds.
    /// Whole-job memory admission is M8's (E-29).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_thread_mib: Option<u32>,
    /// How much memory this whole job needs, in MiB (E-29).
    ///
    /// What admission costs the job at: a runner takes it only if its memory,
    /// less the floor it keeps for itself, less what the jobs already out
    /// there declared, covers this figure. Absent means the job declares no
    /// whole-job memory need — which is not zero: it is a job admission has no
    /// figure to weigh, and it is admitted on capabilities alone.
    ///
    /// Mutually exclusive with `build_thread_mib`: a build declares what one
    /// of its threads needs and the hub multiplies by the `-j` it derived, a
    /// test declares what the whole job needs. A submission carrying both says
    /// two things about one question and is refused by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
    /// How much room on the runner's tmpfs this job reserves while it runs, in
    /// MiB (E-31).
    ///
    /// Reserved before the job starts and released by the record ending, so
    /// there is nothing to leak (D79). `Some(0)` is a real declaration — the
    /// first consumer's `build` and `check` need no tmpfs at all — and absent
    /// means the same thing said by not saying it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmpfs_mib: Option<u64>,
    /// When this job stops being worth starting, in **seconds since the Unix
    /// epoch** (E-09).
    ///
    /// An absolute instant, not an offset, and judged on the **hub's** clock:
    /// the consumer's nightly is "skipped if it has not started by 05:00", and
    /// an offset from a submission that was itself delayed would move the
    /// 05:00. Unix seconds rather than a formatted time because a number has
    /// no time zone to be wrong about — the requester's CLI is where a human's
    /// `05:00` or `+15m` is turned into one.
    ///
    /// It bounds **waiting** and nothing else: a job that has not started by
    /// then ends `cancelled`, and a job that *has* started is never touched by
    /// it (that is M4's time cap, exit 124).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_unix_s: Option<i64>,
    /// The one job this one starts after, if it names one (E-10).
    ///
    /// **One parent, no conditions**: it starts after that job *succeeds*, and
    /// if the parent ends any other way this job ends `cancelled` naming it. A
    /// list, a condition or a second edge would be a workflow language, which
    /// invariant 1 refuses — anything more than this is refused by name rather
    /// than grown into a graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<JobId>,
    /// The one exit code this job's command means "the capability I was given
    /// went away under me" by (D76).
    ///
    /// 75 for the first consumer, whose `metal` commands exit it when the
    /// machine's lease turns out to be held after all. Such a code is **not a
    /// verdict**: it causes a bounded requeue with the capability re-probed,
    /// on a budget of its own, separate from E-21's two attempts for a runner
    /// that disappears. Absent means every code this job exits with is its own
    /// verdict, reported verbatim (D20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requeue_exit_code: Option<i32>,
    /// The declared command this job is, when it is one (E-11, D106).
    ///
    /// Present means the job is a declared command on a commit: the runner
    /// runs it through the consumer's four executor steps, and `command` is
    /// the declared line going into the `run` hook's [`LINE_TOKEN`]. Absent
    /// means `command` is the whole of what runs, which is every job Herd ran
    /// before M10 and every `herdctl run -- …` after it.
    ///
    /// Both forms stay, deliberately. The ad-hoc one is how Herd is tested
    /// against itself without a consumer tree in the picture (E-11 wants no
    /// consumer command in Herd's own code), and the declared one is how a
    /// real job arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<Declaration>,
    /// The reading that will say what this job needs, when its command's
    /// requirements are not knowable from the declaration (D74).
    ///
    /// Present means [`Asked::labels`] is the **baseline** the reading itself
    /// is placed against and not this job's own requirements: the hub runs the
    /// reading as an ordinary job on a runner, writes what it answered onto
    /// the record, and places this job on that. Absent means `labels` and the
    /// figures beside them are what the requester stated, which is every job
    /// before D74 and every one whose command declares static labels.
    ///
    /// A requester fills the line's `{commit}` and `{args}` like any other
    /// (D107); everything else about the reading is the hub's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels_from: Option<LabelsFrom>,
    /// How long this job's `run` step may take, in seconds, as its command
    /// declares (E-11).
    ///
    /// The consumer's own figures: 45 min for a build, 2 h for a test, 5 h for
    /// the nightly. It bounds the `run` step and nothing else — the
    /// preparation steps are bounded by the runner's own cap, which is its
    /// owner's figure and not a requester's to raise (invariant 2, D108).
    ///
    /// Absent means the job is capped at the runner's own figure, which is
    /// what every job before M10 was. A figure **above** the runner's own does
    /// not raise it: the smaller of the two wins, because the machine belongs
    /// to its owner and the record says which of the two bound the job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
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
        /// What the hub has to say about the gap between its build and this
        /// peer's, when there is a gap it is willing to serve (E-51).
        ///
        /// A major gap is a refusal and never arrives here. A minor one is
        /// served — the frames mean the same thing — and still worth one
        /// line, because the two sides drifting is how a fleet ends up with
        /// one machine behaving unlike the others. It travels rather than
        /// being left on the hub's own stdout so that it reaches the person
        /// who can act on it: the owner of the machine, reading her runner's
        /// stderr. The hub says it too.
        ///
        /// `None` is the ordinary case: the two builds agree.
        version_note: Option<String>,
    },
    /// The hub declines to go on, and says why. Always the last frame.
    Refused { reason: String },
    /// Where a hypothetical job would run, and nothing else (E-07).
    ///
    /// Lines rather than a structure, and said in the hub's own words: the
    /// eligible runners with the labels each of them answers for, what is
    /// queued ahead, and the verdict the claim step gave — built by the same
    /// function that builds the line on a real placement (E-36), so the two
    /// cannot drift. M13's machine-readable status is the structured view
    /// (E-35); this is the sentence a person reads before submitting.
    WouldRun { lines: Vec<String> },
    /// The hub has set a runner's switch, or says why it has not (E-33).
    SwitchSet {
        said: String,
        /// Whether the runner **confirmed writing the position down**
        /// ([`ToHub::SwitchApplied`], D99).
        ///
        /// `false` is never success. It covers three cases and `said` names
        /// which: no runner of that name is connected; the runner answered
        /// that it applied the position but could not write it; and the
        /// acknowledgement did not arrive inside
        /// [`SWITCH_HONOURED_WITHIN`] — in which case the hub *is* honouring
        /// the position and the machine's own disk may or may not know it yet.
        ///
        /// Required, with no `serde(default)`: either default would be a lie
        /// about a position an owner is relying on. That is what moved
        /// [`PROTOCOL_VERSION`] to 6.
        confirmed: bool,
    },
    /// Where every runner's switch stands (E-33).
    FleetSwitches { runners: Vec<RunnerSwitch> },
    /// One runner is approved, and here is the token it is to carry (E-48).
    ///
    /// **The only time the token is ever readable.** The hub keeps a hash of
    /// it and nothing else (invariant 7, Q8), so an operator who loses this
    /// line approves the runner again rather than looking the token up — and
    /// the thing nobody can do is read it off the hub's disk.
    RunnerApproved {
        runner: String,
        token: String,
        said: String,
    },
    /// One runner is removed and its token revoked (E-48), or it was never
    /// approved and nothing changed. `said` is which.
    RunnerRemoved { said: String },
    /// Which runners are approved on this hub (E-48).
    ApprovedRunners { runners: Vec<ApprovedRunner> },
    /// The owner, through the hub, moves this runner's switch (E-33).
    ///
    /// A runner that receives it **writes the position to its own switch file
    /// first** and only then applies it, so that the two ways of setting it —
    /// her own machine and an orchestrator — leave one state behind, in the
    /// same order, and a restart keeps it. It then answers with
    /// [`ToHub::SwitchApplied`] (D99).
    SetSwitch { switch: Switch, why: String },
    /// Read your probes again, now (D76).
    ///
    /// Sent to the runner a job has just answered its declared requeue code
    /// on: the capability it held turned out to be taken, so whatever the
    /// probe last said about it is known to be stale. Without this the requeue
    /// would be judged on the reading that caused it, and the budget would be
    /// spent against one stale answer.
    ReadProbesAgain { why: String },
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
        /// The declared command, when the job is one (E-11, D106).
        ///
        /// Handed back with every attempt rather than held by the runner that
        /// first saw it: a job requeued onto another machine must run the same
        /// four steps on the same commit, and the record is the only thing
        /// that knows them both.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        declaration: Option<Declaration>,
        /// What the declaration says this job's `run` step may take, in
        /// seconds. The runner caps it at the smaller of this and its own
        /// figure (D108).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_s: Option<u64>,
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
    /// stderr is not in it. Sent before anything else about that job (D22).
    ///
    /// **Bytes of the log, not the whole of it.** The backlog is sent in
    /// consecutive frames of at most [`JOB_LOG_FRAME_BYTES`] and a requester
    /// writes each out as it arrives, so several frames are byte-identical to
    /// the one frame this used to be. It used to be one, and a job that
    /// printed more than about 48 MiB then built a frame past
    /// [`FRAME_CEILING`] — no verdict and exit 125 for a job that had
    /// passed. Nothing about the frame changed to fix that; only how much of
    /// the log one of them is allowed to hold.
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
    /// The hub is still there and this job is not over. **Nothing is printed
    /// for it.**
    ///
    /// A follower hears from the hub when something happens: a line of
    /// output, the reason a job is still queued, where it was placed, the
    /// verdict. A job that is *running* and printing nothing produces none of
    /// those, and a quiet build is the ordinary case rather than the
    /// exception — so the connection carried **no frames at all** for as long
    /// as the job stayed quiet. Behind a reverse proxy with an idle timeout
    /// that is a connection closed under the follower, and `herdctl` then saw
    /// the stream end with no verdict and exited 125 on a job that had in
    /// fact succeeded. A *wrong* answer rather than a degraded one: a merger
    /// reading 125 concludes the work never ran.
    ///
    /// So the hub keeps the link warm itself, every [`STILL_THERE_EVERY`], to
    /// every follower of a job that is not yet over. No client needs a
    /// heartbeat of its own, none redials on this, and none learns anything
    /// new about timing from it. A job that has **ended** gets none: its
    /// stream is over, and a frame after the verdict would be the hub talking
    /// about something that is finished.
    ///
    /// **Not a [`FromHub::JobNote`], and the distinction is not cosmetic.** A
    /// note is text a person reads, on stderr; this frame is read by nobody.
    /// A client prints nothing for it — not on stdout, not on stderr, not
    /// into a log — and it is no part of the job's output: E-02's framing is
    /// the `ran on:` line, then the job's own bytes, then the verdict, and
    /// this frame belongs to none of those three. `job_id` is here so that a
    /// follower of more than one job can tell which link is being kept warm,
    /// and there is deliberately nothing else to carry: a field a reader
    /// could act on would make this a message rather than a breath.
    StillThere { job_id: JobId },
    /// Where this job is running, and why there (E-02, E-36).
    ///
    /// The first consumer's first expectation: "the output starts with one
    /// line `ran on: <client> (<why>)` and ends with our tool's own verdict
    /// and exit code". This frame is what that line is made of, and it is a
    /// frame rather than a line because Herd must **not** write it into the
    /// job's log: a record's log is the bytes the job's process wrote, and
    /// nothing else (D18, E-15). The requester's CLI builds the sentence and
    /// prints it on its own stdout, ahead of the job's first byte.
    ///
    /// Sent when the attempt is handed out, and sent again to a requester
    /// that attaches later — a `herdctl wait` on a job that is already
    /// running has to be able to print the same first line. A job that is
    /// requeued onto another machine (E-21) produces a **second** one, which
    /// is the truth about a job that ran in two places and is worth a line.
    ///
    /// `why` is the one line that explains the placement (E-36) — the same
    /// sentence the record's attempt carries, so the two cannot drift.
    RanOn {
        job_id: JobId,
        runner: String,
        why: String,
    },
    /// How the job ended, relayed to the requester that submitted it, with
    /// what the runner measured while it ran.
    JobFinished {
        job_id: JobId,
        outcome: JobOutcome,
        #[serde(default)]
        metrics: JobMetrics,
        /// The files the job left under `{out}`, read back off the record
        /// ([`JobOutputs`]).
        ///
        /// Off the **record**, not relayed through memory, so a `herdctl
        /// wait` that attaches an hour after the job ran gets the same bytes
        /// the requester who followed it got — which is why this is a field
        /// here and not a subcommand of its own: the frame a late attach
        /// already receives is the after-the-fact fetch.
        ///
        /// Not part of E-02's stream: `herdctl` writes them where it was
        /// told to and nowhere else, so stdout stays `ran on:`, the job's own
        /// bytes, then the verdict.
        #[serde(default, skip_serializing_if = "JobOutputs::is_silent")]
        outputs: JobOutputs,
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
    /// What the hub holds of a job's streamed collection, so a runner resumes
    /// rather than starts again (D148).
    ///
    /// **Sent unprompted**, for every job of a reconnecting runner whose
    /// collection the hub has not finished receiving — the same shape as
    /// [`FromHub::ResendLog`], and for the same reason: the hub is the side
    /// that knows what it is missing, so asking it would be a round trip
    /// spent learning what it could simply say. A runner that has just
    /// redialled with 300 MiB spooled needs one frame to know where to pick
    /// up, not a conversation.
    ///
    /// Each entry says how many bytes of that path the hub holds. Equal to
    /// the manifest's length means complete and verified; less means a
    /// part-written file to continue; a path absent from the list means not a
    /// byte of it has arrived.
    JobFilesHeld {
        job_id: JobId,
        attempt: u32,
        held: Vec<FileHeld>,
    },
    /// The hub holds every file the manifest named, verified, and the runner
    /// may drop its spool.
    ///
    /// [`FromHub::JobResultAck`] for the files, and separate from it on
    /// purpose: the verdict is acknowledged when the verdict is written, and
    /// a runner must be free to forget a result it has delivered without
    /// waiting on hundreds of megabytes it has not. The two acknowledgements
    /// are the two halves of a record and they arrive when each half is
    /// durable.
    JobFilesAck { job_id: JobId, attempt: u32 },
    /// One piece of one file of a job's collection, on its way to a requester
    /// that asked for it ([`ToHub::AskJobFiles`]).
    ///
    /// The requester's side of [`ToHub::JobFileChunk`], and bounded the same
    /// way. Read off the hub's disk, so this frame flows for a job that
    /// finished an hour ago and for one finishing now without the two being
    /// different code: there is one answer to "what did this job leave" and
    /// it is the disk (D137).
    JobFile {
        job_id: JobId,
        path: String,
        offset: u64,
        #[serde(with = "bytes_as_base64")]
        data: Vec<u8>,
    },
    /// Every file the requester asked for has been sent, and here is what
    /// could not be.
    ///
    /// **The frame that makes an absent output answerable instead of empty**
    /// (D148). The manifest outlives the bytes: a job's record keeps every
    /// path, length and sha256 for as long as the record exists, and only the
    /// bytes are ever removed. So a requester asking for something that is
    /// not there is told which paths, how big they were, and why they are
    /// gone — never handed a short directory it would read as a whole one.
    ///
    /// Two reasons reach it today — a file whose runner never came back, and
    /// a path the hub refuses to read back off its own disk — and a third
    /// when M12's sweeper lands, which is eviction. All three say the same
    /// kind of sentence, which is why there is one frame for them.
    JobFilesSent {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        absent: Vec<AbsentFile>,
    },
    /// A frame of a type this build has never heard of.
    ///
    /// **A runner logs it and ignores it; a requester ends its run.** The
    /// asymmetry is deliberate and it is the reason a protocol break was
    /// worth spending on an enum arm (D148). Before this variant existed the
    /// decode failed outright, and the two clients failed in two unacceptable
    /// ways: `herdctl` reported no verdict for a job that may have passed,
    /// and `herd-runner` dropped the link and **redialled into the same
    /// undecodable frame** for ever (D128). Ignoring is right for a runner,
    /// because a frame it does not know is work it is not needed for; it is
    /// wrong for a requester, because a requester that cannot read what the
    /// hub is telling it must not pretend to have a verdict (D124).
    #[serde(other)]
    Unknown,
}

/// How much of one streamed output file the hub holds
/// ([`FromHub::JobFilesHeld`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHeld {
    pub path: String,
    /// Bytes on the hub's disk for this path, complete or part-written.
    pub bytes: u64,
}

/// One file a job's record names and the hub cannot hand over **yet**
/// ([`FromHub::JobFilesSent`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsentFile {
    pub path: String,
    /// How big it was, off the manifest, because "338 MiB of screenshots are
    /// gone" and "something is gone" are different sentences.
    pub len: u64,
    /// Why, in one sentence a requester can act on.
    pub why: String,
    /// How many bytes of it the hub holds: 0, or a part-written file's
    /// length.
    ///
    /// **This is what tells a waiting requester that waiting is working.** A
    /// collection still crossing and a collection that will never arrive look
    /// identical in a list of names, and the difference between them is
    /// whether this figure moves.
    #[serde(default)]
    pub held: u64,
    /// Whether the hub is still expecting these bytes.
    ///
    /// **The field that makes "not yet" different from "gone", and they must
    /// never be confused.** The verdict goes first and the bytes follow
    /// (D148), so a requester asking for a collection the instant its job
    /// ended is asking for something genuinely in flight — and told "gone"
    /// it would report a lost result for a job whose evidence was seconds
    /// away. Told "still coming" it waits, which is what `--outputs-to`
    /// promises.
    ///
    /// `false` is the answer that is final: no runner is going to send these
    /// bytes, because none holds them — a machine that never came back
    /// today, M12's retention later. Both deserve the same sentence and both
    /// get it.
    #[serde(default)]
    pub still_coming: bool,
}

/// The version of the output Herd frames a job's own output with (Q7).
///
/// 1, and it freezes at M16 (Q7, D73). It is carried **inside the
/// parentheses** of [`ran_on_line`], which is the one place it can go: E-02
/// asks that the output *start* with one line `ran on: <runner> (<why>)`, so
/// a version on a line of its own would push that line down, and a version
/// after the closing parenthesis would break a reader anchored on it. Inside,
/// it is part of the free text the `why` already is, and
/// `^ran on: (\S+) \((.*)\)$` is satisfied exactly (D109).
pub const OUTPUT_FORMAT_VERSION: u32 = 1;

/// E-02's first line: where this job ran, and why there.
///
/// Built here rather than in the requester's CLI because the hub's tests
/// assert on it and the CLI prints it, and two places writing one sentence is
/// what invariant 4 exists to prevent. Printed on the requester's **stdout**,
/// ahead of the job's first byte, and never written into the record's log —
/// that log is the bytes the job's process wrote and nothing else (D18).
pub fn ran_on_line(runner: &str, why: &str) -> String {
    format!("ran on: {runner} ({why}; herd output format {OUTPUT_FORMAT_VERSION})")
}

/// What is in front of [`ran_on_line`], so that a reader can be anchored on
/// it and not on a substring that could appear in a job's own output.
pub const RAN_ON_PREFIX: &str = "ran on: ";

/// The two parts of [`ran_on_line`]: the runner, and the reason with its
/// format version. `None` for a line that is not E-02's.
///
/// The inverse of `ran_on_line` lives beside it rather than in whoever reads
/// the output, because that is the only way the format has one definition
/// (invariant 4). The shape is exactly `^ran on: (\S+) \((.*)\)$`, which is
/// what a caller writing its own reader would use — anything looser would
/// accept a line their reader drops on the floor.
pub fn ran_on_parts(line: &str) -> Option<(&str, &str)> {
    let inside = line.strip_prefix(RAN_ON_PREFIX)?.strip_suffix(')')?;
    let (runner, why) = inside.split_once(" (")?;
    // `\S+`: one word, so a runner's name can never swallow the parenthesis.
    if runner.is_empty() || runner.contains(char::is_whitespace) {
        return None;
    }
    Some((runner, why))
}

/// Split what a requester's CLI wrote on stdout into E-02's framing line and
/// the job's **own** output, which follows it byte for byte.
///
/// `None` when there is no framing line, which is a job that produced no
/// output and never started (D16): the line frames output, so with none to
/// frame there is nothing on stdout at all. The remainder is bytes, never a
/// string — a job's output is bytes and is not re-encoded on the way through
/// (D18).
pub fn unframe(stdout: &[u8]) -> Option<(String, &[u8])> {
    let end = stdout.iter().position(|byte| *byte == b'\n')?;
    let line = std::str::from_utf8(&stdout[..end]).ok()?;
    ran_on_parts(line)?;
    Some((line.to_owned(), &stdout[end + 1..]))
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

/// What the hub does about the gap between its build and a peer's (E-51).
///
/// Two answers and one of them is not an answer: a major gap is refused, a
/// minor gap is served with a note, and builds that agree say nothing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionGap {
    /// Whether this gap ends the connection. `true` is a [`FromHub::Refused`]
    /// and therefore terminal (D13): nothing but a new build changes it.
    pub refuse: bool,
    /// The one line a human reads, naming **both** versions.
    pub reason: String,
}

/// What to do about a peer announcing `peer_version` to a hub built as
/// `hub_version`, and nothing when the two agree (E-51).
///
/// A *major* difference refuses. A major version is where Herd's own
/// behaviour is allowed to change under an unchanged protocol — how a job is
/// confined, what a spooled verdict looks like, which of its steps a runner
/// reports — so two peers a major apart can read each other's frames and
/// still disagree about what they commit to. That is the failure a refusal
/// prevents, and it is the one a protocol version cannot see.
///
/// A *minor* difference is served, because within a major version Herd
/// promises the behaviour is the same; it is still said out loud, because a
/// fleet whose machines drift apart is a fleet where one machine's results
/// are not the others'.
///
/// A version string this hub cannot read — not `major.minor…` — is **served
/// with a note**, never refused: turning a working machine away over the
/// shape of a string would be a hub refusing what it merely does not
/// understand, and the credential, not the version, is what says a peer is
/// allowed here. The note says the gap could not be judged, which is the
/// honest thing to put in front of a human.
///
/// `peer_kind` is the word the sentence uses for the far side — "runner" or
/// "requester" — because an operator reading one line needs to know which
/// machine to go to.
pub fn version_gap(peer_kind: &str, hub_version: &str, peer_version: &str) -> Option<VersionGap> {
    if hub_version == peer_version {
        return None;
    }
    match (major_minor(hub_version), major_minor(peer_version)) {
        (Some((hub_major, _)), Some((peer_major, _))) if hub_major != peer_major => {
            Some(VersionGap {
                refuse: true,
                reason: format!(
                    "build version mismatch: this hub is version {hub_version} and this \
                     {peer_kind} is version {peer_version}, a major version apart. Within one \
                     major version Herd promises its behaviour is the same, and across one it \
                     promises nothing — so this {peer_kind} is refused rather than served \
                     differently from the rest of the fleet. Move the {peer_kind} to \
                     {hub_major}.x, or move the hub to {peer_major}.x"
                ),
            })
        }
        (Some(_), Some(_)) => Some(VersionGap {
            refuse: false,
            reason: format!(
                "build version mismatch: this hub is version {hub_version} and this \
                 {peer_kind} is version {peer_version}. The major versions agree, so the two \
                 behave alike and this connection is served; move one side to the other's \
                 version so the fleet does not drift"
            ),
        }),
        _ => Some(VersionGap {
            refuse: false,
            reason: format!(
                "build version unreadable: this hub is version {hub_version} and this \
                 {peer_kind} announced {peer_version:?}, which is not a major.minor version. \
                 It is served — a version string this hub cannot read is not a reason to turn \
                 a working machine away — but nothing about the gap between the two builds \
                 could be judged"
            ),
        }),
    }
}

/// The major and minor of a version string, or `None` if it has neither.
///
/// Deliberately small: Herd compares two of its own build numbers, and
/// pre-release tags, build metadata and the rest of semver's grammar are
/// things it has never produced. Anything it cannot read is handled as
/// unreadable by [`version_gap`] rather than guessed at.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Why a runner nobody approved is given nothing, and what to do about it
/// (E-48, Q8).
///
/// Terminal (D13): the machine is not on this hub's roster, and dialling
/// again says the same thing. The sentence carries the command that fixes it,
/// because the person reading it on a runner's stderr is usually the person
/// who can type it.
pub fn unapproved_runner_reason(name: &str) -> String {
    format!(
        "runner {name} is not approved on this hub, so it is given no job: an operator \
         approves it with `herdctl runner approve {name}`, which mints the token this runner \
         then carries as --token or HERD_TOKEN"
    )
}

/// Why an approved runner that carried no token is still given nothing.
pub fn missing_runner_token_reason(name: &str) -> String {
    format!(
        "runner {name} is approved on this hub but carried no token, so it is given no job: \
         give it the token `herdctl runner approve {name}` printed, as --token or HERD_TOKEN"
    )
}

/// Why an approved runner carrying the wrong token is given nothing.
///
/// It says the name is known and the token is not the one that was minted,
/// and nothing about the token itself: a sentence that quoted what it
/// received would put a credential in every log that caught it.
pub fn wrong_runner_token_reason(name: &str) -> String {
    format!(
        "runner {name} is approved on this hub, but the token it carried is not the one that \
         approval minted, so it is given no job: run `herdctl runner approve {name}` and give \
         the machine the token it prints"
    )
}

/// Why a runner an operator has just removed stops claiming, said on the
/// connection it is still holding (E-48).
pub fn revoked_runner_reason(name: &str) -> String {
    format!(
        "runner {name} has been removed from this hub and its token is revoked, so it claims \
         nothing from here on: `herdctl runner approve {name}` mints a new token if this \
         machine is to come back"
    )
}

/// Why a requester that carried no token is refused, when the hub was started
/// with one (Q8).
pub fn missing_requester_token_reason() -> String {
    "this hub requires a requester token and this requester carried none: give it as \
     --token or HERD_TOKEN, from the file the hub was started with \
     (--requester-token-file)"
        .to_owned()
}

/// Why a requester carrying the wrong token is refused (Q8).
pub fn wrong_requester_token_reason() -> String {
    "this requester's token is not the one this hub was started with: take it from the file \
     named by --requester-token-file (HERD_REQUESTER_TOKEN_FILE) on the hub"
        .to_owned()
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

/// The `type` a frame announces itself as, whatever this build knows about it.
///
/// The catch-all variants are unit variants, so serde discards the tag on its
/// way to them: a peer that only matched on the enum would know it had met
/// something unknown and not **what**. That is the difference between a log
/// line somebody can act on — `ignoring a frame of type "pack_capability"` —
/// and one nobody can, so the tag is read back off the text here.
///
/// Peeks one field and allocates one string; it is called only on the
/// unknown path, which is by construction rare.
pub fn frame_type(frame: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Tagged {
        #[serde(rename = "type")]
        kind: String,
    }
    serde_json::from_str::<Tagged>(frame)
        .ok()
        .map(|tagged| tagged.kind)
}
