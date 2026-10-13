//! The capability algebra: what a runner can offer and what a job may hold.
//!
//! One algebra, here and nowhere else (Q3, invariant 4). A **label** is the
//! name a requester asks for; a **capability** is what one runner offers under
//! that name, with the rules that govern holding it. The two words are kept
//! apart on purpose: a job's requirements persist in the hub's queue entries
//! (M6) and must not drag a runner's kinds, exclusions and probes into them.
//!
//! A capability is a name, a [`CapabilityKind`], an exclusion list and an
//! optional [`BusyProbe`]. Every one of those is a string or a number. A probe
//! that grew an API or a callback would be a plugin, which invariant 1
//! forbids, so a probe is a path or a command line and two exit codes — Herd
//! runs it and reads the number it exits with, and never learns what it means.
//!
//! What lives here is **pure**: the types, their validation, and
//! [`Holdings`], which answers "may this job hold these labels on this runner
//! right now". Probing the host is the runner's (`herd-runner`'s
//! `capability` module); deciding what to do with a `no` is the hub's.
//!
//! Three things deliberately do **not** appear.
//!
//! * Memory and tmpfs. They are numeric admissions, decided against figures
//!   that change every second (M8), and a capability is a name you hold.
//! * `metal-<machine>`. It is [`Capability::lease`] — `counted(1)` with a
//!   probe — and nothing in Herd knows the prefix means anything (E-27).
//! * `reference`. It is a capability like any other, carried because a runner
//!   declared it (E-28). Nothing here names it, and a test proves that by
//!   showing it behaves exactly as any other shared capability does.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::JobId;

/// Something about a capability or a label set does not hold, said with the
/// name that does not hold it.
///
/// One error type for the whole algebra: these are configuration mistakes and
/// protocol violations, and both have to arrive naming the offender rather
/// than as a shrug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityError(String);

impl CapabilityError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CapabilityError {}

/// The longest a label may be. Long enough for `metal-<machine>` and a cache
/// key family, short enough that a report stays a line.
pub const MAX_LABEL_LENGTH: usize = 64;

/// Whether `name` is a label at all.
///
/// Lower case, digits, `.`, `_` and `-`. A fixed, small character set, for
/// three reasons that all bite later: a label is rendered into reports whose
/// separators are `:` and `,` (see [`CapabilityOrigins`]), it is compared for
/// equality in every placement decision — so two labels differing by case
/// would be a bug nobody could see — and it lands in durable queue entries
/// (M6) that no later milestone may have to migrate.
pub fn check_label(name: &str) -> Result<(), CapabilityError> {
    if name.is_empty() {
        return Err(CapabilityError::new("a label cannot be the empty string"));
    }
    if name.len() > MAX_LABEL_LENGTH {
        return Err(CapabilityError::new(format!(
            "the label {name:?} is {} characters, and a label may be at most \
             {MAX_LABEL_LENGTH}",
            name.len()
        )));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')))
    {
        return Err(CapabilityError::new(format!(
            "the label {name:?} carries {bad:?}, and a label is lower-case \
             letters, digits, '.', '_' and '-'"
        )));
    }
    Ok(())
}

/// A set of labels: sorted, without repetition, every name checked.
///
/// Canonical on purpose. M6 keys its deduplication on a job's labels among
/// other things (E-05), and two submissions that asked for the same two
/// capabilities in a different order are the same submission. Sorting here is
/// what makes that true once, for everyone, instead of in each caller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct LabelSet(Vec<String>);

impl LabelSet {
    /// Build a set, refusing a name that is not a label.
    ///
    /// A bad name is an error rather than a name that silently matches
    /// nothing: a typo in a submission must come back as a sentence, not as a
    /// job that waits for ever for a capability no machine will ever offer.
    pub fn new<S: Into<String>>(
        labels: impl IntoIterator<Item = S>,
    ) -> Result<Self, CapabilityError> {
        let mut names: Vec<String> = Vec::new();
        for label in labels {
            let name = label.into();
            check_label(&name)?;
            names.push(name);
        }
        names.sort();
        names.dedup();
        Ok(Self(names))
    }

    /// One label, for the common case.
    pub fn of(label: impl Into<String>) -> Result<Self, CapabilityError> {
        Self::new([label.into()])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, label: &str) -> bool {
        self.0
            .binary_search_by(|held| held.as_str().cmp(label))
            .is_ok()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// Add a label, keeping the set canonical. Private: a set a peer sent is
    /// what it is, and the only thing that grows one is the symmetric closure
    /// of an exclusion list ([`CapabilityOffer::new`]).
    fn insert(&mut self, label: String) {
        if let Err(at) = self.0.binary_search(&label) {
            self.0.insert(at, label);
        }
    }
}

impl TryFrom<Vec<String>> for LabelSet {
    type Error = CapabilityError;

    fn try_from(labels: Vec<String>) -> Result<Self, Self::Error> {
        Self::new(labels)
    }
}

impl From<LabelSet> for Vec<String> {
    fn from(set: LabelSet) -> Self {
        set.0
    }
}

impl fmt::Display for LabelSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("none");
        }
        f.write_str(&self.0.join(", "))
    }
}

/// How many jobs may hold one capability at once, and what else may run while
/// they do.
///
/// Three kinds, and no fourth: a kind is a rule about contention, and anything
/// richer would be a policy language (invariant 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityKind {
    /// Host-wide. One holder, and **nothing else runs on that runner while it
    /// is held** — not another job holding another capability, not a job
    /// holding none at all.
    ///
    /// This is what makes it a different kind from `counted(1)` rather than a
    /// synonym for it: `counted(1)` serialises the holders of one name, while
    /// `exclusive` empties the machine. It is the nightly-trial shape the plan
    /// describes — an exclusive label keeps everything else off that runner
    /// until it ends (M14) — and the only honest reading of "host-wide".
    Exclusive,
    /// At most `slots` jobs hold it at once. Others wait; nothing is refused
    /// for it (D38).
    Counted { slots: u32 },
    /// Any number of holders. A fact about the machine rather than a resource:
    /// `reference`, an architecture, a cache key family.
    Shared,
}

impl CapabilityKind {
    /// How many holders are allowed, or `None` for unlimited.
    pub fn holders_allowed(self) -> Option<u32> {
        match self {
            CapabilityKind::Exclusive => Some(1),
            CapabilityKind::Counted { slots } => Some(slots),
            CapabilityKind::Shared => None,
        }
    }

    /// Whether holding it empties the runner of everything else.
    pub fn is_host_wide(self) -> bool {
        matches!(self, CapabilityKind::Exclusive)
    }

    /// The kind as it reads in a sentence and on the wire.
    pub fn name(self) -> &'static str {
        match self {
            CapabilityKind::Exclusive => "exclusive",
            CapabilityKind::Counted { .. } => "counted",
            CapabilityKind::Shared => "shared",
        }
    }

    fn check(self) -> Result<(), CapabilityError> {
        match self {
            CapabilityKind::Counted { slots: 0 } => Err(CapabilityError::new(
                "counted(0) is a capability nothing can ever hold; deny the \
                 capability instead, or give it at least one slot",
            )),
            _ => Ok(()),
        }
    }
}

/// The exit code a command probe means "busy" with, unless it says otherwise.
/// Zero, because that is what `test -e`, `pgrep` and `fuser` exit with when
/// the thing they looked for is there.
pub const DEFAULT_BUSY_EXIT_CODE: i32 = 0;

/// The exit code a command probe means "free" with, unless it says otherwise.
pub const DEFAULT_FREE_EXIT_CODE: i32 = 1;

fn default_busy_exit_code() -> i32 {
    DEFAULT_BUSY_EXIT_CODE
}

fn default_free_exit_code() -> i32 {
    DEFAULT_FREE_EXIT_CODE
}

/// How a runner asks something outside Herd whether a capability is in use
/// (E-26).
///
/// Configuration, not code: a path, a command line, two numbers. Herd starts
/// the thing, reads the number it exits with, and maps it to
/// [`Availability`] — it never learns what the capability *is*. That line is
/// invariant 1's: the moment a probe took a callback, a shared object or a
/// script Herd evaluated, Herd would have a plugin system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BusyProbe {
    /// Busy while this path exists. The mechanism an operator reaches for
    /// first, because `touch` and `rm` are the whole interface.
    LockFile { path: String },
    /// Busy when this command exits with `busy_exit_code`, free when it exits
    /// with `free_exit_code`, and **unknown for any other code** — which
    /// suspends the capability and says so.
    ///
    /// Two numbers rather than "zero means busy and everything else means
    /// free", because everything else includes 127, the code a shell gives a
    /// command that is not there. A broken probe that reads as "free" would
    /// hand out a capability something outside Herd is holding, which is
    /// exactly the harm invariant 2 forbids.
    Command {
        /// The program and its arguments. Run directly, with no shell: a
        /// shell would make this a one-line script language.
        command: Vec<String>,
        #[serde(default = "default_busy_exit_code")]
        busy_exit_code: i32,
        #[serde(default = "default_free_exit_code")]
        free_exit_code: i32,
    },
}

impl BusyProbe {
    /// Busy while `path` exists.
    pub fn lock_file(path: impl Into<String>) -> Self {
        BusyProbe::LockFile { path: path.into() }
    }

    /// Busy when this command exits 0, free when it exits 1.
    pub fn command<S: Into<String>>(command: impl IntoIterator<Item = S>) -> Self {
        BusyProbe::Command {
            command: command.into_iter().map(Into::into).collect(),
            busy_exit_code: DEFAULT_BUSY_EXIT_CODE,
            free_exit_code: DEFAULT_FREE_EXIT_CODE,
        }
    }

    /// What the probe is, in a sentence, so a wait reason can name it.
    pub fn describe(&self) -> String {
        match self {
            BusyProbe::LockFile { path } => format!("the lock file {path}"),
            BusyProbe::Command { command, .. } => format!("the probe {}", command.join(" ")),
        }
    }

    fn check(&self) -> Result<(), CapabilityError> {
        match self {
            BusyProbe::LockFile { path } if path.is_empty() => Err(CapabilityError::new(
                "a lock-file probe needs a path to look for",
            )),
            BusyProbe::Command { command, .. } if command.is_empty() => Err(CapabilityError::new(
                "a command probe needs a command to run",
            )),
            BusyProbe::Command {
                command,
                busy_exit_code,
                free_exit_code,
            } if busy_exit_code == free_exit_code => Err(CapabilityError::new(format!(
                "the probe {} would mean both busy and free with the same exit \
                 code {busy_exit_code}",
                command.join(" ")
            ))),
            _ => Ok(()),
        }
    }
}

/// One capability: a name, a kind, what it cannot be held alongside, and
/// optionally how to ask the host whether something outside Herd has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub kind: CapabilityKind,
    /// Labels this capability cannot be held alongside, **as declared**. The
    /// relation that is enforced is the symmetric closure of every such list
    /// in one offer; see [`CapabilityOffer::new`].
    #[serde(default, skip_serializing_if = "LabelSet::is_empty")]
    pub excludes: LabelSet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_probe: Option<BusyProbe>,
}

impl Capability {
    /// A capability any number of jobs may hold.
    pub fn shared(name: impl Into<String>) -> Self {
        Self::of(name, CapabilityKind::Shared)
    }

    /// A capability whose holder has the runner to itself.
    pub fn exclusive(name: impl Into<String>) -> Self {
        Self::of(name, CapabilityKind::Exclusive)
    }

    /// A capability `slots` jobs may hold at once.
    pub fn counted(name: impl Into<String>, slots: u32) -> Self {
        Self::of(name, CapabilityKind::Counted { slots })
    }

    /// A lease: one holder, and something outside Herd may be holding it
    /// (E-27).
    ///
    /// There is no `Lease` kind, and that is the point. A lease is
    /// `counted(1)` plus a probe, so the thing E-27 asks for already falls out
    /// of the algebra; `metal-<machine>` is one of these and nothing in Herd
    /// reads the prefix. The drain-first behaviour that goes with a metal
    /// trial is M9's and lives nowhere near this type.
    pub fn lease(name: impl Into<String>, probe: BusyProbe) -> Self {
        Self {
            busy_probe: Some(probe),
            ..Self::of(name, CapabilityKind::Counted { slots: 1 })
        }
    }

    fn of(name: impl Into<String>, kind: CapabilityKind) -> Self {
        Self {
            name: name.into(),
            kind,
            excludes: LabelSet::default(),
            busy_probe: None,
        }
    }

    /// Declare that this capability cannot be held alongside `labels`.
    pub fn excluding<S: Into<String>>(
        mut self,
        labels: impl IntoIterator<Item = S>,
    ) -> Result<Self, CapabilityError> {
        self.excludes = LabelSet::new(labels)?;
        Ok(self)
    }

    /// Attach an external busy probe.
    pub fn probed_by(mut self, probe: BusyProbe) -> Self {
        self.busy_probe = Some(probe);
        self
    }

    /// Whether this capability is valid on its own terms. An offer checks the
    /// things that only make sense between capabilities.
    pub fn check(&self) -> Result<(), CapabilityError> {
        check_label(&self.name)?;
        self.kind.check()?;
        if self.excludes.contains(&self.name) {
            return Err(CapabilityError::new(format!(
                "{} excludes itself; a kind says how many jobs may hold one \
                 capability, an exclusion says which other capabilities it \
                 cannot be held alongside",
                self.name
            )));
        }
        if let Some(probe) = &self.busy_probe {
            probe.check()?;
        }
        Ok(())
    }
}

/// The name a metal lease on `machine` is held under (E-27).
///
/// A convenience for whoever writes a configuration, and nothing more: the
/// hub and the runner see the string this returns, never the machine.
pub fn metal_label(machine: &str) -> String {
    format!("metal-{machine}")
}

/// Where a capability in a runner's offer came from (E-24).
///
/// The three origins are not exclusive: a capability can be detected on the
/// host, declared in the configuration and denied by the runner's owner all at
/// once, and a reader has to be able to see each of them. That is why this is
/// three flags and not one enum — collapsing them would be exactly the loss of
/// information E-24 forbids, and `kvm: present, denied` is the case that
/// proves it.
///
/// **Denial wins.** A denied capability is reported and never offered, whether
/// the host has it or not and whether the configuration asked for it or not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityOrigins {
    /// The runner detected it on the host.
    #[serde(default)]
    pub probed: bool,
    /// The runner's configuration declared it.
    #[serde(default)]
    pub declared: bool,
    /// The runner's owner refused it. Wins over the other two.
    #[serde(default)]
    pub denied: bool,
}

impl CapabilityOrigins {
    pub fn probed() -> Self {
        Self {
            probed: true,
            ..Self::default()
        }
    }

    pub fn declared() -> Self {
        Self {
            declared: true,
            ..Self::default()
        }
    }

    pub fn denied() -> Self {
        Self {
            denied: true,
            ..Self::default()
        }
    }

    /// Whether a job may hold it: something put it here, and nobody refused it.
    pub fn is_offered(self) -> bool {
        (self.probed || self.declared) && !self.denied
    }

    /// The origins as the report spells them, in a fixed order.
    ///
    /// `probed` reads **present**, because that is what a probe of the host
    /// established and it is the word E-24's own example uses.
    pub fn words(self) -> Vec<&'static str> {
        let mut words = Vec::new();
        if self.probed {
            words.push("present");
        }
        if self.declared {
            words.push("declared");
        }
        if self.denied {
            words.push("denied");
        }
        if words.is_empty() {
            words.push("nowhere");
        }
        words
    }
}

impl fmt::Display for CapabilityOrigins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words().join(", "))
    }
}

/// A capability as one runner offers it, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferedCapability {
    #[serde(flatten)]
    pub capability: Capability,
    pub origins: CapabilityOrigins,
}

impl OfferedCapability {
    pub fn new(capability: Capability, origins: CapabilityOrigins) -> Self {
        Self {
            capability,
            origins,
        }
    }

    pub fn name(&self) -> &str {
        &self.capability.name
    }

    pub fn is_offered(&self) -> bool {
        self.origins.is_offered()
    }

    /// The line E-24 asks for: `kvm: present, denied`.
    ///
    /// One function, so the runner's startup report, the hub's log and a test
    /// all read the same string.
    pub fn report_line(&self) -> String {
        format!("{}: {}", self.capability.name, self.origins)
    }
}

/// Everything one runner offers, with the exclusion relation already closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<OfferedCapability>", into = "Vec<OfferedCapability>")]
pub struct CapabilityOffer {
    entries: Vec<OfferedCapability>,
}

impl CapabilityOffer {
    /// Validate an offer and close its exclusion relation.
    ///
    /// # The closure rule
    ///
    /// **Exclusion is symmetric, and it is not transitive.** The relation
    /// enforced is the symmetric closure of the declared lists: if `a`
    /// excludes `b`, then `b` excludes `a`, computed once here so that
    /// everything downstream reads an already-symmetric relation.
    ///
    /// Symmetry is not a nicety. Exclusion is a statement that two things
    /// cannot run on one host at the same time, and that is a property of the
    /// pair; if it were read as declared, whether `a` and `b` could overlap
    /// would depend on which of them a runner happened to start first. A
    /// configuration that lists only one direction gets the other one, rather
    /// than an order-dependent machine.
    ///
    /// Transitivity is refused, deliberately. `a` excludes `b` and `b`
    /// excludes `c` leaves `a` and `c` free to overlap, because the two
    /// statements are about two different contentions and nothing was said
    /// about the third: `kvm` may exclude `metal-pc` and `metal-pc` may
    /// exclude `build` while `kvm` and `build` share a machine perfectly well.
    /// Closing transitively would forbid pairs nobody declared, silently, and
    /// grow as the configuration grew. Someone who does want a whole group to
    /// be mutually exclusive already has the tool for it and needs no closure
    /// rule: one [`CapabilityKind::Exclusive`] capability, or a declared list
    /// naming each member.
    ///
    /// A cycle is therefore not a special case at all. `a` excludes `b` and
    /// `b` excludes `a` is what the symmetric closure of either half already
    /// is: the computation adds the missing direction, finds nothing more to
    /// add, and terminates.
    pub fn new(entries: Vec<OfferedCapability>) -> Result<Self, CapabilityError> {
        let mut by_name: BTreeMap<String, OfferedCapability> = BTreeMap::new();
        for entry in entries {
            entry.capability.check()?;
            if by_name.contains_key(entry.name()) {
                return Err(CapabilityError::new(format!(
                    "{} is offered twice; one runner offers a capability under \
                     one name, with one kind",
                    entry.name()
                )));
            }
            by_name.insert(entry.name().to_owned(), entry);
        }
        // The symmetric closure. Collected first, applied second: the lists
        // being read must not change under the reading.
        let reversed: Vec<(String, String)> = by_name
            .values()
            .flat_map(|entry| {
                let name = entry.name().to_owned();
                entry
                    .capability
                    .excludes
                    .iter()
                    .map(|excluded| (excluded.to_owned(), name.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (excluded, name) in reversed {
            // An exclusion naming a capability this runner does not offer is
            // inert rather than wrong — nothing here can hold it — and the
            // runner says so in its report rather than guessing at a typo.
            if let Some(entry) = by_name.get_mut(&excluded) {
                entry.capability.excludes.insert(name);
            }
        }
        Ok(Self {
            entries: by_name.into_values().collect(),
        })
    }

    /// An offer of nothing. A runner with no capabilities is a runner that can
    /// still run a job that needs none.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, sorted by name, denied ones included: a report has to show
    /// what was refused (E-24).
    pub fn entries(&self) -> &[OfferedCapability] {
        &self.entries
    }

    pub fn get(&self, label: &str) -> Option<&OfferedCapability> {
        self.entries
            .binary_search_by(|entry| entry.name().cmp(label))
            .ok()
            .map(|at| &self.entries[at])
    }

    /// The capability a job may hold under this label, or `None` when this
    /// runner does not offer it — because nothing put it there, or because its
    /// owner denied it.
    pub fn available(&self, label: &str) -> Option<&Capability> {
        self.get(label)
            .filter(|entry| entry.is_offered())
            .map(|entry| &entry.capability)
    }

    /// The labels a job may ask for here.
    pub fn offered_labels(&self) -> LabelSet {
        LabelSet(
            self.entries
                .iter()
                .filter(|entry| entry.is_offered())
                .map(|entry| entry.name().to_owned())
                .collect(),
        )
    }

    /// One line per capability, as E-24 asks to see them.
    pub fn report(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(OfferedCapability::report_line)
            .collect()
    }

    /// Exclusions naming a capability this runner does not offer.
    ///
    /// Inert — there is nothing here to contend with — but worth saying out
    /// loud, because the overwhelmingly likely cause is a misspelled name in
    /// a configuration that then protects nothing.
    pub fn inert_exclusions(&self) -> Vec<String> {
        let mut inert = Vec::new();
        for entry in &self.entries {
            for excluded in entry.capability.excludes.iter() {
                if self.get(excluded).is_none() {
                    inert.push(format!(
                        "{} excludes {excluded}, which this runner does not \
                         offer, so the exclusion does nothing",
                        entry.name()
                    ));
                }
            }
        }
        inert
    }
}

impl TryFrom<Vec<OfferedCapability>> for CapabilityOffer {
    type Error = CapabilityError;

    fn try_from(entries: Vec<OfferedCapability>) -> Result<Self, Self::Error> {
        Self::new(entries)
    }
}

impl From<CapabilityOffer> for Vec<OfferedCapability> {
    fn from(offer: CapabilityOffer) -> Self {
        offer.entries
    }
}

/// What a runner's probe says about one capability right now (E-26).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Availability {
    /// Free as far as the runner can tell. A capability with no probe is
    /// always this.
    Free,
    /// Something outside Herd is holding it. `reason` names what said so.
    Busy { reason: String },
    /// The probe could not be read: the command is not there, it exited with
    /// a code the configuration gives no meaning to, it was signalled, or it
    /// ran past its timeout. `reason` says which.
    Unknown { reason: String },
}

impl Availability {
    /// Whether the capability is suspended: nothing may be given it now.
    ///
    /// **A probe that cannot be read suspends.** This is the one judgement
    /// that had to be made and it is made here, in one place, so that no
    /// caller can make the other one. Reading an unreadable probe as "free"
    /// would hand out a capability something outside Herd may be holding —
    /// starting a build on a machine someone is using bare, which is the harm
    /// invariant 2 exists to prevent. Reading it as suspended costs a job that
    /// waits with a sentence naming the broken probe and the runner, visible
    /// in the job's record and never terminal (D38). Fail safe, loudly, rather
    /// than fail open, silently.
    pub fn suspends(&self) -> bool {
        !matches!(self, Availability::Free)
    }

    /// Why it is suspended, if it is.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Availability::Free => None,
            Availability::Busy { reason } | Availability::Unknown { reason } => Some(reason),
        }
    }

    pub fn busy(reason: impl Into<String>) -> Self {
        Availability::Busy {
            reason: reason.into(),
        }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Availability::Unknown {
            reason: reason.into(),
        }
    }
}

/// One capability's name and what its probe says, as a runner reports it.
///
/// The runner sends the whole set each time anything changes rather than a
/// delta: a snapshot is idempotent, survives a frame nobody saw, and needs no
/// reconciliation after a reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityState {
    pub name: String,
    pub availability: Availability,
}

impl CapabilityState {
    pub fn new(name: impl Into<String>, availability: Availability) -> Self {
        Self {
            name: name.into(),
            availability,
        }
    }
}

/// Why a job may not hold the labels it asked for, right now, on this runner.
///
/// Every variant names the label and its figures, because this sentence ends
/// up on a job's record as the reason it is waiting, and a reason with no
/// figures is not a reason (E-04). None of these is a refusal: a contention
/// passes, and D38 exists so that "wait" never has to be said with `Refused`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contention {
    /// This runner does not offer the label at all, or its owner denied it.
    /// What to *do* about it — wait for a runner that does, or fail the job
    /// fast because no runner ever will (E-36) — is the hub's, in M7.
    NotOffered { label: String },
    /// Two of the labels asked for cannot be held together, so no machine
    /// anywhere could ever run this job.
    ExcludedWithin { label: String, with: String },
    /// Something already running here holds a capability the asked-for label
    /// excludes.
    Excluded {
        label: String,
        held: String,
        by: JobId,
    },
    /// Every slot of a counted capability is taken.
    NoSlot {
        label: String,
        slots: u32,
        holders: u32,
    },
    /// The label is host-wide and this runner is not idle.
    ExclusiveNeedsIdle { label: String, running: usize },
    /// A host-wide capability is held here, so nothing else runs.
    HostIsExclusive { held: String, by: JobId },
}

impl Contention {
    /// The one sentence this contention reads as.
    pub fn reason(&self) -> String {
        match self {
            Contention::NotOffered { label } => {
                format!("this runner does not offer {label}")
            }
            Contention::ExcludedWithin { label, with } => format!(
                "{label} and {with} cannot be held together, so this job asks \
                 for something no runner can give it"
            ),
            Contention::Excluded { label, held, by } => {
                format!("{label} cannot be held alongside {held}, which {by} is holding")
            }
            Contention::NoSlot {
                label,
                slots,
                holders,
            } => format!("{label} has {slots} slot(s) and {holders} are taken"),
            Contention::ExclusiveNeedsIdle { label, running } => format!(
                "{label} is host-wide and this runner is running {running} \
                 other job(s)"
            ),
            Contention::HostIsExclusive { held, by } => format!(
                "{by} holds {held}, which is host-wide, so nothing else runs \
                 here until it ends"
            ),
        }
    }

    /// Whether no runner could ever satisfy this, however long the job waits.
    ///
    /// Only one contention is of that kind: a job asking for two labels that
    /// exclude each other. Everything else passes. M7 is what acts on this
    /// (E-36); the algebra only has to be able to tell the two apart.
    pub fn is_impossible(&self) -> bool {
        matches!(self, Contention::ExcludedWithin { .. })
    }
}

impl fmt::Display for Contention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason())
    }
}

/// What one runner is holding right now, and the rules for taking more.
///
/// The algebra's working end. It holds *jobs*, not labels, for two reasons
/// that both matter: a host-wide capability has to know whether anything at
/// all is running, which a count of labels cannot say; and releasing is
/// something a job does, once, whatever it was holding.
///
/// Pure bookkeeping — no clock, no I/O, no probes. Where this gets consulted
/// is the claim step, and that is M7's: this type exists now because M6's
/// queue entries persist the labels it reasons about, and an algebra that
/// arrived after them would have reshaped them.
#[derive(Debug, Clone, Default)]
pub struct Holdings {
    offer: CapabilityOffer,
    held: BTreeMap<JobId, LabelSet>,
}

impl Holdings {
    pub fn new(offer: CapabilityOffer) -> Self {
        Self {
            offer,
            held: BTreeMap::new(),
        }
    }

    pub fn offer(&self) -> &CapabilityOffer {
        &self.offer
    }

    /// How many jobs are running here.
    pub fn running(&self) -> usize {
        self.held.len()
    }

    /// How many jobs hold this label.
    pub fn holders_of(&self, label: &str) -> u32 {
        u32::try_from(
            self.held
                .values()
                .filter(|labels| labels.contains(label))
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// Whether a job holding `labels` could start here now, and why not if it
    /// could not.
    ///
    /// The rules, in the order they are checked — first match wins, because
    /// the reason a human reads should be the plainest one:
    ///
    /// 1. every label must be offered here and not denied;
    /// 2. no two of the asked-for labels may exclude each other;
    /// 3. nothing host-wide may already be held;
    /// 4. a host-wide label needs an idle runner;
    /// 5. a counted label needs a free slot — `shared` has none to need;
    /// 6. no label may be held alongside a label something else already holds
    ///    and excludes.
    pub fn check(&self, labels: &LabelSet) -> Result<(), Contention> {
        for label in labels.iter() {
            if self.offer.available(label).is_none() {
                return Err(Contention::NotOffered {
                    label: label.to_owned(),
                });
            }
        }
        for label in labels.iter() {
            let capability = self
                .offer
                .available(label)
                .expect("every label was just found in the offer");
            for other in labels.iter() {
                if other != label && capability.excludes.contains(other) {
                    return Err(Contention::ExcludedWithin {
                        label: label.to_owned(),
                        with: other.to_owned(),
                    });
                }
            }
        }
        // Something host-wide is held: nothing else runs, whatever it asks for.
        for (job, held) in &self.held {
            for label in held.iter() {
                if self
                    .offer
                    .get(label)
                    .is_some_and(|entry| entry.capability.kind.is_host_wide())
                {
                    return Err(Contention::HostIsExclusive {
                        held: label.to_owned(),
                        by: job.clone(),
                    });
                }
            }
        }
        for label in labels.iter() {
            let capability = self
                .offer
                .available(label)
                .expect("every label was just found in the offer");
            if capability.kind.is_host_wide() && !self.held.is_empty() {
                return Err(Contention::ExclusiveNeedsIdle {
                    label: label.to_owned(),
                    running: self.held.len(),
                });
            }
            if let Some(slots) = capability.kind.holders_allowed() {
                let holders = self.holders_of(label);
                if holders >= slots {
                    return Err(Contention::NoSlot {
                        label: label.to_owned(),
                        slots,
                        holders,
                    });
                }
            }
            for (job, held) in &self.held {
                for other in held.iter() {
                    if capability.excludes.contains(other) {
                        return Err(Contention::Excluded {
                            label: label.to_owned(),
                            held: other.to_owned(),
                            by: job.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Take the labels for `job`, or say why it cannot.
    ///
    /// Checking and taking are one call on purpose: a caller that asked first
    /// and took afterwards would have a window in which the answer changed.
    pub fn admit(&mut self, job: &JobId, labels: LabelSet) -> Result<(), Contention> {
        self.check(&labels)?;
        self.held.insert(job.clone(), labels);
        Ok(())
    }

    /// Give back whatever a job was holding. Idempotent: a job that ended
    /// twice — a result redelivered, say — releases once.
    pub fn release(&mut self, job: &JobId) {
        self.held.remove(job);
    }

    /// What a job is holding, if it is holding anything.
    pub fn held_by(&self, job: &JobId) -> Option<&LabelSet> {
        self.held.get(job)
    }
}
