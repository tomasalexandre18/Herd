//! What this runner offers, and what its external probes say about it.
//!
//! Three origins meet here (E-24). A capability can be **probed** — detected
//! on this host — **declared** in the runner's configuration, or **denied** by
//! the runner's owner, and the offer the hub receives carries all three so
//! that a reader sees which: the one that matters is `kvm: present, denied`, a
//! capability this host has and its owner refuses. **Denial wins**, over
//! detection and over declaration both, because the owner of the machine is
//! the one person whose "no" Herd may never second-guess.
//!
//! Detection is a fixed table of device paths and nothing else ([`DETECTED`]).
//! Extending it is a change to this file, not a line of configuration, and
//! that is deliberate: a detection rule a user could write would be a
//! predicate language, and a predicate language with a plugin loader behind it
//! is what invariant 1 refuses. Everything an owner needs beyond the table is
//! a *declaration*, which is data.
//!
//! The probes (E-26) are the other half. A probe is a lock-file path or a
//! command line with two exit codes — strings and numbers, [`herd_proto`]'s
//! [`BusyProbe`] — and this module is the only thing in Herd that runs one. It
//! runs them on the runner, off the hub entirely, under a timeout, and turns
//! each one into an [`Availability`]. A capability a probe says is busy is
//! suspended; a job needing it waits and is never failed (D38).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use herd_proto::{
    Availability, BusyProbe, Capability, CapabilityError, CapabilityKind, CapabilityOffer,
    CapabilityOrigins, CapabilityState, LabelSet, OfferedCapability,
};
use serde::Deserialize;

/// What a runner looks for on its own host, and where.
///
/// Three devices, because those are the three the first consumer's machines
/// are judged on (E-47), and a table rather than a rule because a rule would
/// be a language. A name here is offered as [`herd_proto::CapabilityKind`]'s
/// `shared`: it is a fact about the machine, not a resource with slots. An
/// owner who wants one of these counted, excluded or probed declares it, and
/// the declaration keeps the `present` origin beside its own.
pub const DETECTED: &[(&str, &str)] = &[
    ("kvm", "/dev/kvm"),
    ("dri", "/dev/dri"),
    ("vfio", "/dev/vfio/vfio"),
];

/// How often a runner re-reads its probes, in milliseconds.
///
/// A probe is a `stat` or a short command, so this is cheap; what sets the
/// floor is that a lock file an operator has just removed should free the
/// capability while they are still watching. Injectable, because a test that
/// really waited this out would be a test CI drops.
pub const DEFAULT_PROBE_INTERVAL_MS: u64 = 2_000;

/// How long one probe may take before the runner gives up on it, in
/// milliseconds. Past it the probe is killed and read as
/// [`Availability::Unknown`], which suspends the capability.
pub const DEFAULT_PROBE_TIMEOUT_MS: u64 = 2_000;

/// How often probes are read, and how long each may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeSettings {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for ProbeSettings {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(DEFAULT_PROBE_INTERVAL_MS),
            timeout: Duration::from_millis(DEFAULT_PROBE_TIMEOUT_MS),
        }
    }
}

/// A runner's capability configuration, as a file holds it.
///
/// The file is a path given at startup (invariant 7) and what it declares is
/// [`herd_proto`]'s own [`Capability`]: the protocol is the contract
/// (invariant 4), so this carries no shape of its own past the door. What the
/// file does *not* carry is origins — reading it is what makes a capability
/// `declared`, and a file that could claim `probed` could claim a device the
/// host has not got.
///
/// **The reading is strict.** It goes through the private types below, which
/// refuse an unknown key at every level and name both the key and the
/// capability that carried it. The protocol's types are not made strict to
/// achieve that, and must not be: they are a wire contract whose compatibility
/// stance is its own. A file is the other thing entirely — an operator wrote
/// it by hand, once, and will not see it again — and `probe` where
/// `busy_probe` was meant would otherwise offer the capability **unprobed**,
/// handing out a resource something outside Herd is holding because of one
/// missing word (E-26).
#[derive(Debug, Clone, Default)]
pub struct CapabilityFile {
    /// Capabilities this runner offers because its owner says so.
    pub capabilities: Vec<Capability>,
    /// Capabilities this runner refuses, whatever the host has.
    pub deny: Vec<String>,
}

impl CapabilityFile {
    /// Read one, naming the path when it will not read.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
        Self::parse(&text)
            .map_err(|why| format!("{} is not a capability file: {why}", path.display()))
    }

    /// The same reading, over the text itself, so that what a key refuses can
    /// be asserted without a file on a disk.
    ///
    /// Each capability is taken as JSON first and read strictly second, for
    /// one reason: serde's own refusal names the key it did not know and
    /// nothing else, and "unknown field `probe`" in a file of nine
    /// capabilities is not an answer. The name is lifted out of the raw value
    /// — which is still there however badly the rest of the entry is spelled —
    /// so the refusal names the capability too.
    fn parse(text: &str) -> Result<Self, String> {
        let shape: FileShape = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let mut capabilities = Vec::with_capacity(shape.capabilities.len());
        for raw in shape.capabilities {
            let named = match raw.get("name").and_then(serde_json::Value::as_str) {
                Some(name) => format!("the capability {name:?}"),
                None => "a capability with no name".to_owned(),
            };
            let read: FileCapability =
                serde_json::from_value(raw).map_err(|error| format!("{named}: {error}"))?;
            capabilities.push(
                read.into_capability()
                    .map_err(|error| format!("{named}: {error}"))?,
            );
        }
        Ok(Self {
            capabilities,
            deny: shape.deny,
        })
    }
}

/// The file's top level, strictly.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileShape {
    #[serde(default)]
    capabilities: Vec<serde_json::Value>,
    #[serde(default)]
    deny: Vec<String>,
}

/// One capability as a file spells it: every key known, nothing ignored.
///
/// A mirror of [`Capability`], and the test
/// `the_file_reads_every_shape_the_protocol_can_write` is what keeps it one:
/// it writes each shape with the protocol's own serialiser and reads it back
/// through here, so a field added to `Capability` without a field added here
/// turns red rather than becoming a key this file quietly refuses.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCapability {
    name: String,
    kind: FileKind,
    #[serde(default)]
    excludes: Vec<String>,
    #[serde(default)]
    busy_probe: Option<FileProbe>,
}

impl FileCapability {
    fn into_capability(self) -> Result<Capability, CapabilityError> {
        let capability = Capability {
            name: self.name,
            kind: self.kind.into(),
            excludes: LabelSet::new(self.excludes)?,
            busy_probe: self.busy_probe.map(BusyProbe::from),
        };
        // Refused here as well as in `offer`, so that a bad capability in a
        // file is a bad capability whoever reads it.
        capability.check()?;
        Ok(capability)
    }
}

/// [`CapabilityKind`] as a file spells it.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum FileKind {
    Exclusive,
    Counted { slots: u32 },
    Shared,
}

impl From<FileKind> for CapabilityKind {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Exclusive => CapabilityKind::Exclusive,
            FileKind::Counted { slots } => CapabilityKind::Counted { slots },
            FileKind::Shared => CapabilityKind::Shared,
        }
    }
}

/// [`BusyProbe`] as a file spells it, exit-code defaults included.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum FileProbe {
    LockFile {
        path: String,
    },
    Command {
        command: Vec<String>,
        #[serde(default = "busy_exit_code")]
        busy_exit_code: i32,
        #[serde(default = "free_exit_code")]
        free_exit_code: i32,
    },
}

fn busy_exit_code() -> i32 {
    herd_proto::capability::DEFAULT_BUSY_EXIT_CODE
}

fn free_exit_code() -> i32 {
    herd_proto::capability::DEFAULT_FREE_EXIT_CODE
}

impl From<FileProbe> for BusyProbe {
    fn from(probe: FileProbe) -> Self {
        match probe {
            FileProbe::LockFile { path } => BusyProbe::LockFile { path },
            FileProbe::Command {
                command,
                busy_exit_code,
                free_exit_code,
            } => BusyProbe::Command {
                command,
                busy_exit_code,
                free_exit_code,
            },
        }
    }
}

/// Whether the host really has what a path stands for.
///
/// Existence is not the question for a device: a `/dev/kvm` this runner cannot
/// open is a capability it would accept jobs for and then fail them. So a
/// device node is opened for reading and writing — which is what using it
/// needs and what creates nothing — and a directory is present when it has an
/// entry in it. Anything beyond that is not detectable without running the
/// consumer's own command, and an owner who knows better declares or denies.
fn present(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Err(_) => false,
        Ok(found) if found.is_dir() => std::fs::read_dir(path)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false),
        Ok(_) => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .is_ok(),
    }
}

/// The capabilities this host has, by name, from [`DETECTED`].
pub fn detect() -> Vec<String> {
    detect_among(DETECTED)
}

/// The same, over a table a test supplies. The real table is a `const`, so
/// this is how the detection half of E-24 is asserted on a host that may or
/// may not have `/dev/kvm`.
pub fn detect_among(table: &[(&str, &str)]) -> Vec<String> {
    table
        .iter()
        .filter(|(_, path)| present(Path::new(path)))
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// Build this runner's offer out of the three origins (E-24).
///
/// `detected` is what the host has, `declared` is what the configuration
/// says, `denied` is what the owner refuses. A name in more than one of them
/// is one entry carrying every origin that applies, so the report can show
/// all of them; a declaration's kind, exclusions and probe win over the plain
/// `shared` a detection implies, because the owner wrote them down; and
/// denial makes the entry unofferable without making it invisible.
///
/// **One name is declared once.** Two declarations of `build` are two
/// different capabilities claiming one name — a kind, an exclusion list and a
/// probe each — and nothing can choose between them: taking the last would
/// silently drop the exclusion the first declared, which is a guard an
/// operator wrote down and believes is in force. So it is refused here,
/// naming the name, before the runner dials.
pub fn offer(
    detected: &[String],
    declared: Vec<Capability>,
    denied: &[String],
) -> Result<CapabilityOffer, CapabilityError> {
    let mut entries: BTreeMap<String, OfferedCapability> = BTreeMap::new();
    for name in detected {
        herd_proto::check_label(name)?;
        entries.insert(
            name.clone(),
            OfferedCapability::new(
                Capability::shared(name.clone()),
                CapabilityOrigins::probed(),
            ),
        );
    }
    // Declaring a name the host also has is not a duplicate: that is the
    // merge this function exists for. Declaring it twice is.
    let mut names_declared: BTreeSet<String> = BTreeSet::new();
    for capability in declared {
        capability.check()?;
        let name = capability.name.clone();
        if !names_declared.insert(name.clone()) {
            return Err(CapabilityError::new(format!(
                "{name} is declared twice; one runner offers a capability \
                 under one name, with one kind, one exclusion list and one \
                 probe"
            )));
        }
        let probed = entries.get(&name).is_some_and(|found| found.origins.probed);
        entries.insert(
            name,
            OfferedCapability::new(
                capability,
                CapabilityOrigins {
                    probed,
                    declared: true,
                    denied: false,
                },
            ),
        );
    }
    for name in denied {
        herd_proto::check_label(name)?;
        let entry = entries.entry(name.clone()).or_insert_with(|| {
            // Denying something neither detected nor declared is not an error:
            // an owner may refuse a capability before a machine ever grows it,
            // and the refusal has to survive the day it does.
            OfferedCapability::new(
                Capability::shared(name.clone()),
                CapabilityOrigins::default(),
            )
        });
        entry.origins.denied = true;
    }
    CapabilityOffer::new(entries.into_values().collect())
}

/// Read one probe, once, and say what it means.
///
/// Every way this can go wrong lands in [`Availability::Unknown`], which
/// suspends the capability rather than handing it out: a probe exists because
/// something outside Herd owns the thing, and a probe nobody can read is not
/// permission to assume it is free.
pub async fn evaluate(probe: &BusyProbe, timeout: Duration) -> Availability {
    match probe {
        BusyProbe::LockFile { path } => lock_file(Path::new(path)),
        BusyProbe::Command {
            command,
            busy_exit_code,
            free_exit_code,
        } => run_probe(command, *busy_exit_code, *free_exit_code, timeout).await,
    }
}

/// A lock-file probe: busy while the path is there.
///
/// `symlink_metadata`, not `metadata`: a dangling symlink an operator left as
/// a lock is a lock, and following it would read it as a free capability.
fn lock_file(path: &Path) -> Availability {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Availability::busy(format!("the lock file {} is there", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Availability::Free,
        // EACCES on a parent directory, a broken mount: the runner cannot see
        // whether the lock is held, which is not the same as it being free.
        Err(error) => Availability::unknown(format!(
            "the lock file {} cannot be looked for: {error}",
            path.display()
        )),
    }
}

/// A command probe: run it, read the number it exits with, and nothing else.
///
/// Its output is not read at all — that is what keeps a probe a probe rather
/// than a protocol — and its stdin is closed so that a probe waiting for input
/// hits the timeout instead of waiting for ever.
async fn run_probe(
    command: &[String],
    busy_exit_code: i32,
    free_exit_code: i32,
    timeout: Duration,
) -> Availability {
    let shown = command.join(" ");
    let (program, arguments) = match command.split_first() {
        Some(split) => split,
        None => return Availability::unknown("this probe has no command to run"),
    };
    let mut child = match tokio::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        // A command that is not there, or is not executable. The commonest
        // broken probe there is, and the one that must never read as free.
        Err(error) => {
            return Availability::unknown(format!("the probe {shown} cannot be started: {error}"))
        }
    };
    let waited = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(waited) => waited,
        Err(_) => {
            // A probe that hangs is killed here. It cannot hold the runner up
            // past this timeout, and it never reaches the hub at all: the hub
            // learns a verdict, never a probe.
            let _ = child.kill().await;
            return Availability::unknown(format!(
                "the probe {shown} did not answer within {} ms and was killed",
                timeout.as_millis()
            ));
        }
    };
    let status = match waited {
        Ok(status) => status,
        Err(error) => {
            return Availability::unknown(format!("waiting for the probe {shown} failed: {error}"))
        }
    };
    match status.code() {
        Some(code) if code == busy_exit_code => {
            Availability::busy(format!("the probe {shown} exited {code}, which means busy"))
        }
        Some(code) if code == free_exit_code => Availability::Free,
        // The case that would otherwise be read as free: 127 from a shell that
        // found no command, 2 from a tool given a bad option, anything the
        // configuration gave no meaning to.
        Some(code) => Availability::unknown(format!(
            "the probe {shown} exited {code}, and it means busy with \
             {busy_exit_code} and free with {free_exit_code}"
        )),
        None => Availability::unknown(format!("the probe {shown} was killed by a signal")),
    }
}

/// This runner's offer and what its probes currently say.
///
/// Kept across connections, like the results a runner holds: a probe's verdict
/// is a fact about the host and does not belong to whichever link happens to
/// be up. A runner that redials announces this again rather than
/// reconstructing it.
#[derive(Debug, Clone, Default)]
pub struct Capacity {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    offer: CapabilityOffer,
    settings: ProbeSettings,
    availability: Mutex<BTreeMap<String, Availability>>,
}

impl Capacity {
    /// Hold an offer, with every probed capability starting out unreadable.
    ///
    /// Unknown, not free, until a probe has actually been read once: the first
    /// sweep happens before the runner joins a hub, and if it somehow did not,
    /// the safe answer is the one that makes a job wait.
    pub fn new(offer: CapabilityOffer, settings: ProbeSettings) -> Self {
        let availability = offer
            .entries()
            .iter()
            .filter(|entry| entry.capability.busy_probe.is_some())
            .map(|entry| {
                (
                    entry.name().to_owned(),
                    Availability::unknown(format!(
                        "{} has not been read yet",
                        entry
                            .capability
                            .busy_probe
                            .as_ref()
                            .expect("filtered to the probed entries")
                            .describe()
                    )),
                )
            })
            .collect();
        Self {
            inner: Arc::new(Inner {
                offer,
                settings,
                availability: Mutex::new(availability),
            }),
        }
    }

    pub fn offer(&self) -> &CapabilityOffer {
        &self.inner.offer
    }

    pub fn settings(&self) -> ProbeSettings {
        self.inner.settings
    }

    /// How many capabilities have a probe to read.
    pub fn probes(&self) -> usize {
        self.inner
            .offer
            .entries()
            .iter()
            .filter(|entry| entry.capability.busy_probe.is_some())
            .count()
    }

    /// What to send the hub: one entry per probed capability.
    ///
    /// A capability with no probe is absent from the snapshot rather than
    /// reported free, because there is nothing to report: the hub reads a
    /// missing entry as free, which it is, and nobody has to keep a list of
    /// unprobed names in two places.
    pub fn snapshot(&self) -> Vec<CapabilityState> {
        self.with(|availability| {
            availability
                .iter()
                .map(|(name, state)| CapabilityState::new(name.clone(), state.clone()))
                .collect()
        })
    }

    /// What one capability's probe last said.
    pub fn availability_of(&self, name: &str) -> Availability {
        self.with(|availability| {
            availability
                .get(name)
                .cloned()
                .unwrap_or(Availability::Free)
        })
    }

    /// Read every probe once, and say whether anything changed.
    ///
    /// Sequential on purpose: each probe is bounded by the timeout, so one
    /// sweep is bounded too, and no two evaluations of the same probe can ever
    /// overlap however slow the host is.
    pub async fn sweep(&self) -> bool {
        let mut changed = false;
        for entry in self.inner.offer.entries() {
            let Some(probe) = &entry.capability.busy_probe else {
                continue;
            };
            let read = evaluate(probe, self.inner.settings.timeout).await;
            let was = self
                .with(|availability| availability.insert(entry.name().to_owned(), read.clone()));
            if was.as_ref() != Some(&read) {
                changed = true;
                match read.reason() {
                    Some(reason) => {
                        eprintln!("herd-runner: {} is suspended: {reason}", entry.name())
                    }
                    None => eprintln!("herd-runner: {} is free again", entry.name()),
                }
            }
        }
        changed
    }

    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<String, Availability>) -> T) -> T {
        let mut availability = self
            .inner
            .availability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut availability)
    }
}

/// Read the probes for ever, telling `on_change` whenever a verdict changes.
///
/// Never ends. The caller spawns it beside the redial loop rather than inside
/// one connection, because a probe is about the host and goes on being true
/// while there is no hub to tell.
pub async fn watch(capacity: Capacity, mut on_change: impl FnMut(Vec<CapabilityState>)) {
    let interval = capacity.settings().interval;
    loop {
        tokio::time::sleep(interval).await;
        if capacity.sweep().await {
            on_change(capacity.snapshot());
        }
    }
}

/// What a runner says about its capabilities at startup, on its own stderr.
///
/// The lines E-24 asks to be able to read, plus the probes it will be reading
/// and any exclusion that names a capability this runner does not offer. One
/// function, so that the binary and a test read the same strings.
pub fn report(capacity: &Capacity) -> Vec<String> {
    let offer = capacity.offer();
    let mut lines = Vec::new();
    if offer.is_empty() {
        lines.push("this runner offers no capability".to_owned());
        return lines;
    }
    lines.extend(offer.report());
    lines.extend(offer.inert_exclusions());
    let probes = capacity.probes();
    if probes > 0 {
        lines.push(format!(
            "{probes} capability(ies) are read from an external probe every \
             {} ms; a probe that cannot be read suspends its capability",
            capacity.settings().interval.as_millis()
        ));
    }
    lines
}

/// Everything a runner needs to decide what it offers, before it has read a
/// single probe.
#[derive(Debug, Clone, Default)]
pub struct CapabilityConfig {
    /// A file of declarations, if the owner gave one.
    pub file: Option<PathBuf>,
    /// Capabilities declared on the command line, as plain shared names.
    pub declared: Vec<String>,
    /// Capabilities the owner refuses.
    pub denied: Vec<String>,
    /// Whether to look at the host at all. Off is for a test that must be the
    /// same on every machine; nothing in production turns it off.
    pub detect: bool,
    pub probes: ProbeSettings,
}

impl CapabilityConfig {
    /// Resolve the configuration into the offer this runner will make.
    pub fn resolve(&self) -> Result<CapabilityOffer, String> {
        let mut declared: Vec<Capability> = Vec::new();
        let mut denied = self.denied.clone();
        if let Some(path) = &self.file {
            let file = CapabilityFile::read(path)?;
            declared.extend(file.capabilities);
            denied.extend(file.deny);
        }
        // A name given on the command line is a shared capability and nothing
        // more. Anything with a kind, an exclusion or a probe goes in the file:
        // a flag syntax for the whole algebra would be a grammar, and a
        // grammar on a command line is the first half of a policy language.
        for name in &self.declared {
            declared.push(Capability::shared(name.clone()));
        }
        let detected = if self.detect { detect() } else { Vec::new() };
        offer(&detected, declared, &denied).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(offer: &CapabilityOffer) -> Vec<String> {
        offer.offered_labels().iter().map(str::to_owned).collect()
    }

    /// E-24's own example, in the shape the plan writes it: a capability the
    /// host has and its owner refuses.
    #[test]
    fn a_denied_capability_is_reported_present_and_denied() {
        let offer =
            offer(&["kvm".to_owned()], Vec::new(), &["kvm".to_owned()]).expect("a valid offer");
        assert_eq!(offer.report(), vec!["kvm: present, denied".to_owned()]);
        assert!(
            offer.available("kvm").is_none(),
            "a denied capability is never offered"
        );
        assert!(names(&offer).is_empty());
    }

    /// Denial beats detection and declaration both, and beats them together.
    #[test]
    fn denial_wins_over_detection_and_declaration() {
        let offer = offer(
            &["kvm".to_owned()],
            vec![Capability::counted("kvm", 2)],
            &["kvm".to_owned()],
        )
        .expect("a valid offer");
        assert_eq!(
            offer.report(),
            vec!["kvm: present, declared, denied".to_owned()],
            "every origin that applies must be visible"
        );
        assert!(offer.available("kvm").is_none());
        // Declared only, then denied.
        let offer = offer_of(&[], &["build"], &["build"]);
        assert_eq!(offer.report(), vec!["build: declared, denied".to_owned()]);
        assert!(offer.available("build").is_none());
    }

    fn offer_of(detected: &[&str], declared: &[&str], denied: &[&str]) -> CapabilityOffer {
        offer(
            &detected.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>(),
            declared
                .iter()
                .map(|n| Capability::shared(*n))
                .collect::<Vec<_>>(),
            &denied.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>(),
        )
        .expect("a valid offer")
    }

    #[test]
    fn each_origin_reads_as_itself() {
        assert_eq!(offer_of(&["dri"], &[], &[]).report(), vec!["dri: present"]);
        assert_eq!(
            offer_of(&[], &["build"], &[]).report(),
            vec!["build: declared"]
        );
        assert_eq!(
            offer_of(&["kvm"], &["kvm"], &[]).report(),
            vec!["kvm: present, declared"]
        );
        // Denying a capability nothing offers is kept, so the refusal survives
        // the day the machine grows it.
        assert_eq!(offer_of(&[], &[], &["vfio"]).report(), vec!["vfio: denied"]);
    }

    /// A declaration carries a kind; a detection cannot, so the declaration's
    /// wins while the `present` origin is kept beside it.
    #[test]
    fn a_declaration_s_kind_wins_over_a_detection_s_default() {
        let offer = offer(
            &["kvm".to_owned()],
            vec![Capability::counted("kvm", 3)],
            &[],
        )
        .expect("a valid offer");
        let entry = offer.get("kvm").expect("kvm is offered");
        assert_eq!(entry.capability.kind.holders_allowed(), Some(3));
        assert!(entry.origins.probed && entry.origins.declared);
        assert_eq!(entry.report_line(), "kvm: present, declared");
    }

    #[test]
    fn a_capability_with_a_bad_name_is_refused_by_name() {
        let error = offer(&[], vec![Capability::shared("KVM")], &[])
            .expect_err("an upper-case name is not a label");
        assert!(error.to_string().contains("KVM"), "{error}");
    }

    /// E-28: `reference` is a capability like any other. Nothing in Herd
    /// names it, so it must behave exactly as an arbitrary shared capability
    /// of another name does.
    #[test]
    fn the_reference_label_rides_on_the_kinds_like_any_other() {
        let reference = offer_of(&[], &["reference"], &[]);
        let anything = offer_of(&[], &["pineapple"], &[]);
        assert_eq!(
            reference.get("reference").expect("offered").capability.kind,
            anything.get("pineapple").expect("offered").capability.kind
        );
        assert_eq!(reference.report(), vec!["reference: declared"]);
        assert_eq!(anything.report(), vec!["pineapple: declared"]);
    }

    /// E-27's model: a lease is counted(1) plus a probe, and nothing reads the
    /// `metal-` prefix.
    #[test]
    fn a_metal_lease_is_counted_one_with_a_probe() {
        let name = herd_proto::metal_label("pc");
        assert_eq!(name, "metal-pc");
        let lease = Capability::lease(&name, BusyProbe::lock_file("/run/herd/metal-pc.lease"));
        let offer = offer(&[], vec![lease], &[]).expect("a valid offer");
        let entry = offer.get("metal-pc").expect("the lease is offered");
        assert_eq!(entry.capability.kind.holders_allowed(), Some(1));
        assert!(entry.capability.busy_probe.is_some());
        assert_eq!(Capacity::new(offer, ProbeSettings::default()).probes(), 1);
    }

    #[tokio::test]
    async fn a_lock_file_probe_is_busy_only_while_the_file_is_there() {
        let dir = crate::testing::TempDir::new("lock-file-probe");
        let lock = dir.path().join("held");
        let probe = BusyProbe::lock_file(lock.to_string_lossy().into_owned());
        let timeout = Duration::from_secs(1);
        assert_eq!(evaluate(&probe, timeout).await, Availability::Free);
        std::fs::write(&lock, b"held by hand").expect("the lock file is written");
        let busy = evaluate(&probe, timeout).await;
        assert!(busy.suspends(), "{busy:?}");
        assert!(
            busy.reason().expect("a reason").contains("held"),
            "the reason must name the lock file: {busy:?}"
        );
        std::fs::remove_file(&lock).expect("the lock file is removed");
        assert_eq!(evaluate(&probe, timeout).await, Availability::Free);
    }

    // The command probes live in `tests/busy_probe.rs`, not here: a probe
    // forks, and between the fork and the exec the child holds the flock on
    // this crate's data root, which the unit tests of `root` take and
    // release microseconds apart. See that file's own note.

    /// A capability starts suspended and becomes free only once a probe has
    /// actually been read.
    #[tokio::test]
    async fn a_capability_is_suspended_until_its_probe_has_been_read() {
        let dir = crate::testing::TempDir::new("capacity-sweep");
        let lock = dir.path().join("held");
        std::fs::write(&lock, b"held").expect("written");
        let offer = offer(
            &[],
            vec![Capability::lease(
                "metal-pc",
                BusyProbe::lock_file(lock.to_string_lossy().into_owned()),
            )],
            &[],
        )
        .expect("a valid offer");
        let capacity = Capacity::new(offer, ProbeSettings::default());
        let first = capacity.availability_of("metal-pc");
        assert!(
            matches!(first, Availability::Unknown { .. }),
            "unread is unknown, never free: {first:?}"
        );
        assert!(capacity.sweep().await, "the first read is a change");
        assert!(matches!(
            capacity.availability_of("metal-pc"),
            Availability::Busy { .. }
        ));
        assert!(
            !capacity.sweep().await,
            "nothing changed on the second read"
        );
        std::fs::remove_file(&lock).expect("removed");
        assert!(capacity.sweep().await, "the lock going away is a change");
        assert_eq!(capacity.availability_of("metal-pc"), Availability::Free);
        // A capability with no probe is not in the snapshot at all.
        assert_eq!(capacity.snapshot().len(), 1);
    }

    #[test]
    fn a_capability_with_no_probe_is_free_and_unreported() {
        let capacity = Capacity::new(offer_of(&[], &["build"], &[]), ProbeSettings::default());
        assert_eq!(capacity.probes(), 0);
        assert!(capacity.snapshot().is_empty());
        assert_eq!(capacity.availability_of("build"), Availability::Free);
    }

    #[test]
    fn a_configuration_file_declares_the_whole_algebra() {
        let dir = crate::testing::TempDir::new("capability-file");
        let path = dir.path().join("capabilities.json");
        std::fs::write(
            &path,
            br#"{
                "capabilities": [
                    {"name": "build", "kind": {"type": "counted", "slots": 2},
                     "excludes": ["metal-pc"]},
                    {"name": "metal-pc", "kind": {"type": "counted", "slots": 1},
                     "busy_probe": {"type": "lock_file", "path": "/run/metal-pc"}}
                ],
                "deny": ["kvm"]
            }"#,
        )
        .expect("written");
        let config = CapabilityConfig {
            file: Some(path),
            detect: false,
            ..CapabilityConfig::default()
        };
        let offer = config.resolve().expect("the file resolves");
        assert_eq!(
            offer.report(),
            vec![
                "build: declared".to_owned(),
                "kvm: denied".to_owned(),
                "metal-pc: declared".to_owned(),
            ]
        );
        // The closure ran: metal-pc excludes build too, though only build
        // said so.
        assert!(offer
            .get("metal-pc")
            .expect("offered")
            .capability
            .excludes
            .contains("build"));
        assert!(offer.inert_exclusions().is_empty());
    }

    /// A key this file does not know refuses the runner's startup, naming the
    /// key and the capability that carried it.
    ///
    /// This is the defect that made the strict reading necessary, verbatim:
    /// `probe` where `busy_probe` was meant. Read permissively, it offers
    /// `metal-pc` **unprobed** — the external holder is never asked about, a
    /// job lands on a machine someone is using bare, and the whole of E-26 is
    /// defeated by one missing word in a file nobody looks at twice.
    #[test]
    fn an_unknown_key_refuses_the_file_naming_the_key_and_the_capability() {
        let typo = r#"{"capabilities":[
            {"name":"metal-pc","kind":{"type":"counted","slots":1},
             "probe":{"type":"lock_file","path":"/run/metal-pc"}}
        ]}"#;
        let why = CapabilityFile::parse(typo).expect_err("`probe` is not `busy_probe`");
        assert!(why.contains("probe"), "the key must be named: {why}");
        assert!(
            why.contains("metal-pc"),
            "the capability must be named: {why}"
        );

        // And at every level: inside the kind, inside the probe, and in an
        // entry with no name at all.
        for (what, text, expected) in [
            (
                "a key inside a kind",
                r#"{"capabilities":[{"name":"build","kind":{"type":"counted","slots":1,"slot":2}}]}"#,
                "build",
            ),
            (
                "a key inside a probe",
                r#"{"capabilities":[{"name":"metal-pc","kind":{"type":"shared"},
                   "busy_probe":{"type":"lock_file","path":"/run/l","paths":[]}}]}"#,
                "metal-pc",
            ),
            (
                "an entry with no name",
                r#"{"capabilities":[{"kind":{"type":"shared"}}]}"#,
                "a capability with no name",
            ),
            (
                "a key at the top level",
                r#"{"capabilities":[],"capabilites":[]}"#,
                "capabilites",
            ),
        ] {
            let why = CapabilityFile::parse(text)
                .err()
                .unwrap_or_else(|| panic!("{what} must be refused, not ignored"));
            assert!(why.contains(expected), "{what} must name {expected}: {why}");
        }
    }

    /// The file's shape is the protocol's shape, and this is what keeps it so.
    ///
    /// Every capability the protocol can write is written with the protocol's
    /// own serialiser and read back through the file's strict types. A field
    /// added to [`Capability`] and not to [`FileCapability`] fails here —
    /// loudly, once — instead of becoming a declaration this file silently
    /// refuses.
    #[test]
    fn the_file_reads_every_shape_the_protocol_can_write() {
        let shapes = vec![
            Capability::shared("plain"),
            Capability::exclusive("nightly"),
            Capability::counted("build", 4),
            Capability::lease("metal-pc", BusyProbe::lock_file("/run/metal-pc.lease")),
            Capability::shared("probed").probed_by(BusyProbe::Command {
                command: vec!["/bin/true".to_owned(), "--check".to_owned()],
                busy_exit_code: 3,
                free_exit_code: 9,
            }),
            Capability::counted("excluding", 2)
                .excluding(["nightly", "build"])
                .expect("a valid exclusion"),
        ];
        for capability in shapes {
            let text = serde_json::to_string(&serde_json::json!({
                "capabilities": [capability.clone()],
            }))
            .expect("the protocol's own type serializes");
            let read = CapabilityFile::parse(&text)
                .unwrap_or_else(|why| panic!("the file must read what the protocol writes: {why}"));
            assert_eq!(read.capabilities, vec![capability]);
        }
    }

    /// Two declarations of one name are two capabilities claiming it, and
    /// nothing can choose between them.
    ///
    /// Read as "the last one wins", `build` declared `exclusive` excluding
    /// `kvm` and then `counted(9)` starts a runner whose exclusion is gone —
    /// a guard its owner wrote down and believes is in force. Refused by
    /// name, at startup.
    #[test]
    fn a_capability_declared_twice_is_refused_by_name() {
        let error = offer(
            &[],
            vec![
                Capability::exclusive("build")
                    .excluding(["kvm"])
                    .expect("a valid exclusion"),
                Capability::shared("kvm"),
                Capability::counted("build", 9),
            ],
            &[],
        )
        .expect_err("build is declared twice");
        assert!(error.to_string().contains("build"), "{error}");
        assert!(error.to_string().contains("twice"), "{error}");

        // The same name detected *and* declared is not a duplicate: that is
        // the merge of origins E-24 asks for.
        let offer = offer(
            &["kvm".to_owned()],
            vec![Capability::counted("kvm", 2)],
            &[],
        )
        .expect("a detection and a declaration of one name are one entry");
        assert_eq!(offer.report(), vec!["kvm: present, declared".to_owned()]);
    }

    /// And the same through the file, which is where it will be written.
    #[test]
    fn a_file_declaring_one_name_twice_stops_the_runner() {
        let dir = crate::testing::TempDir::new("declared-twice");
        let path = dir.path().join("capabilities.json");
        std::fs::write(
            &path,
            br#"{"capabilities":[
                {"name":"build","kind":{"type":"exclusive"},"excludes":["kvm"]},
                {"name":"kvm","kind":{"type":"shared"}},
                {"name":"build","kind":{"type":"counted","slots":9}}
            ]}"#,
        )
        .expect("written");
        let config = CapabilityConfig {
            file: Some(path),
            detect: false,
            ..CapabilityConfig::default()
        };
        let error = config.resolve().expect_err("build is declared twice");
        assert!(error.contains("build"), "{error}");
        assert!(error.contains("twice"), "{error}");
    }

    #[test]
    fn an_unreadable_configuration_file_is_refused_by_path() {
        let config = CapabilityConfig {
            file: Some(PathBuf::from("/nonexistent/herd-capabilities.json")),
            ..CapabilityConfig::default()
        };
        let error = config.resolve().expect_err("a missing file is refused");
        assert!(
            error.contains("/nonexistent/herd-capabilities.json"),
            "{error}"
        );
    }

    #[test]
    fn an_exclusion_naming_nothing_this_runner_offers_is_said_out_loud() {
        let offer = offer(
            &[],
            vec![Capability::shared("build")
                .excluding(["metal-typo"])
                .expect("a valid exclusion")],
            &[],
        )
        .expect("a valid offer");
        let said = offer.inert_exclusions();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("metal-typo"), "{said:?}");
        let capacity = Capacity::new(offer, ProbeSettings::default());
        assert!(
            report(&capacity)
                .iter()
                .any(|line| line.contains("metal-typo")),
            "the startup report must carry it: {:?}",
            report(&capacity)
        );
    }

    /// The detection table is read against the real host, so the assertion
    /// has to be about the table rather than about this machine.
    #[test]
    fn detection_reads_a_fixed_table_of_paths() {
        let found = detect();
        for name in &found {
            assert!(
                DETECTED.iter().any(|(known, _)| known == name),
                "{name} is not in the table"
            );
        }
        // A table of paths that cannot exist detects nothing; one of a path
        // that always does detects it. Both go through the same code the real
        // table goes through.
        assert!(detect_among(&[("kvm", "/nonexistent/kvm")]).is_empty());
        assert_eq!(detect_among(&[("dev", "/dev")]), vec!["dev".to_owned()]);
    }
}
