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
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use herd_proto::{
    Availability, BusyProbe, Capability, CapabilityError, CapabilityKind, CapabilityOffer,
    CapabilityOrigins, CapabilityState, LabelSet, OfferedCapability,
};
use serde::Deserialize;

use crate::reclaim::ProbeTag;

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
///
/// **And nothing is written twice.** A key repeated anywhere in the file
/// refuses the runner's startup, naming the key and the capability. The same
/// harm arrives that way — `busy_probe` written twice offered a leased machine
/// as free while its lock was held — and the reading that stops it is
/// [`WithEveryKey`], because a duplicate is gone before any strict type can
/// see it.
#[derive(Debug, Clone, Default)]
pub struct CapabilityFile {
    /// Capabilities this runner offers because its owner says so.
    pub capabilities: Vec<Capability>,
    /// Capabilities this runner refuses, whatever the host has.
    pub deny: Vec<String>,
}

impl CapabilityFile {
    /// Read one, naming the path when it will not read.
    ///
    /// Including a probe whose program this host could never start (D165):
    /// that is a configuration error and it refuses the runner's startup
    /// here, where the refusal can name the file its owner wrote.
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = Self::read_as_written(path)?;
        let unstartable = file.unstartable_probes();
        if !unstartable.is_empty() {
            return Err(format!(
                "{} declares a probe this host cannot start: {}. A probe that cannot be \
                 started is not a busy capability, it is a capability suspended for as long \
                 as this file is live, and every job that asks for it waits behind a reading \
                 that will never arrive — so it is refused here rather than discovered in a \
                 queue (D165)",
                path.display(),
                unstartable.join("; ")
            ));
        }
        Ok(file)
    }

    /// The same reading **without** the startability check, which is what
    /// `herd-runner check` needs: it reports every probe in the file, and a
    /// refusal on the first one would hide the rest.
    pub fn read_as_written(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
        Self::parse(&text)
            .map_err(|why| format!("{} is not a capability file: {why}", path.display()))
    }

    /// Every probe in this file whose program could not be spawned at all, one
    /// sentence each, naming the capability and the program (D165).
    ///
    /// **This is not D49.** D49 is the runtime rule and it does not move: a
    /// probe that *runs* and whose answer cannot be read — a timeout, a signal,
    /// an exit code the configuration gave no meaning to — suspends its
    /// capability, because something outside Herd owns the thing and an
    /// unreadable probe is not permission to assume it is free. That rule is
    /// right and it is why the real outage was survivable at all. What it is
    /// not is a way to notice a *typo*: a file carrying the unsubstituted
    /// placeholder `@probe@` suspended two capabilities for hours while the
    /// runner looked healthy, because every sweep failed with ENOENT and every
    /// ENOENT was faithfully reported as "suspended" in a waiting sentence
    /// nobody reads until something is late.
    ///
    /// So the two are told apart by *when* and by *what*: a program that
    /// cannot be spawned can never answer, so nothing is learned by asking it
    /// every two seconds for ever. It is settled once, before the runner
    /// starts, like a cadence the handshake refuses (D53).
    pub fn unstartable_probes(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .filter_map(|capability| {
                let BusyProbe::Command { command, .. } = capability.busy_probe.as_ref()? else {
                    // A lock-file probe runs no program. A path it cannot
                    // look at is D49's business and stays there: a lock
                    // nobody can see is a lock that may be held.
                    return None;
                };
                let program = command.first()?;
                let why = unstartable(program)?;
                Some(format!(
                    "the capability {:?} is probed by {} and {why}",
                    capability.name,
                    command.join(" ")
                ))
            })
            .collect()
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
    ///
    /// A key written **twice** is caught here rather than by the strict types
    /// below, and that is not a preference: a JSON object is a map, and the
    /// map a [`serde_json::Value`] is read into keeps the last of two equal
    /// keys and drops the first without a word. The strict types never see
    /// the duplicate at all — by the time one is handed a value, the file has
    /// already become "whatever the last line said", and no
    /// `deny_unknown_fields` can recover what is gone. So the duplicate is
    /// noticed while the text goes by, by [`WithEveryKey`].
    fn parse(text: &str) -> Result<Self, String> {
        let shape: FileShape = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let mut capabilities = Vec::with_capacity(shape.capabilities.len());
        for raw in shape.capabilities {
            let named = match raw.value.get("name").and_then(serde_json::Value::as_str) {
                Some(name) => format!("the capability {name:?}"),
                None => "a capability with no name".to_owned(),
            };
            if let Some(key) = &raw.repeated {
                return Err(format!("{named}: {}", repeated_key_refusal(key)));
            }
            let read: FileCapability =
                serde_json::from_value(raw.value).map_err(|error| format!("{named}: {error}"))?;
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

/// What a key written twice is refused with: one sentence in one place, so
/// every level of the file refuses it the same way.
fn repeated_key_refusal(key: &str) -> String {
    format!(
        "the key {key:?} is written more than once, and this file is read \
         exactly as it is written: taking the last value would silently drop \
         what the first one said — a probe, an exclusion or a kind its owner \
         wrote down and believes is in force"
    )
}

/// One JSON value, read with every key it was written with.
///
/// This type exists because [`serde_json::Value`] cannot carry the mistake
/// that has to be reported. An object is a map: reading `{"busy_probe": A,
/// "busy_probe": B}` into one inserts both and keeps `B`, and nothing
/// downstream can tell that apart from a file that only ever said `B`. The
/// detection has to happen where the keys go past, which is the visitor that
/// builds the map.
///
/// The duplicate is **remembered rather than raised**. Raising it where it is
/// seen would refuse the file naming the key and nothing else, because `name`
/// may well be written after the duplicate; keeping it lets
/// [`CapabilityFile::parse`] name the capability too, which is the whole
/// reason this file is read in two passes.
#[derive(Debug)]
struct WithEveryKey {
    value: serde_json::Value,
    /// The first key written twice at or under this value, as a path: `kind`
    /// for a repeated kind, `busy_probe.path` for one inside a probe.
    repeated: Option<String>,
}

impl WithEveryKey {
    fn of(value: serde_json::Value) -> Self {
        Self {
            value,
            repeated: None,
        }
    }
}

impl<'de> Deserialize<'de> for WithEveryKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EveryKey)
    }
}

/// The visitor behind [`WithEveryKey`]: a value is built exactly as
/// `serde_json` builds one, except that a map notices a key it already holds.
struct EveryKey;

impl<'de> serde::de::Visitor<'de> for EveryKey {
    type Value = WithEveryKey;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::Null))
    }

    fn visit_none<E>(self) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::Null))
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<WithEveryKey, D::Error> {
        deserializer.deserialize_any(EveryKey)
    }

    fn visit_bool<E>(self, value: bool) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<WithEveryKey, E> {
        Ok(WithEveryKey::of(serde_json::Value::from(value)))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut access: A,
    ) -> Result<WithEveryKey, A::Error> {
        let mut values = Vec::new();
        let mut repeated: Option<String> = None;
        while let Some(element) = access.next_element::<WithEveryKey>()? {
            // A duplicate inside the third element of a list reads as
            // `[2].key`, so the path still leads to it.
            if repeated.is_none() {
                if let Some(inner) = element.repeated {
                    repeated = Some(format!("[{}].{inner}", values.len()));
                }
            }
            values.push(element.value);
        }
        Ok(WithEveryKey {
            value: serde_json::Value::Array(values),
            repeated,
        })
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut access: A,
    ) -> Result<WithEveryKey, A::Error> {
        let mut object = serde_json::Map::new();
        let mut repeated: Option<String> = None;
        while let Some(key) = access.next_key::<String>()? {
            let element = access.next_value::<WithEveryKey>()?;
            let written_twice = object.contains_key(&key);
            // The key itself, written twice at this level: the case that
            // silently dropped a probe, a kind or an exclusion list.
            if written_twice && repeated.is_none() {
                repeated = Some(key.clone());
            }
            // Or a key written twice somewhere under this one.
            if repeated.is_none() {
                if let Some(inner) = element.repeated {
                    repeated = Some(format!("{key}.{inner}"));
                }
            }
            // **The first value is the one kept**, which is the opposite of
            // what a map does. Nothing is ever *acted* on here — a file with
            // a duplicate anywhere in it is refused — so the only thing this
            // value is read for is to say which capability is being refused.
            // Last-wins made that sentence name a capability the operator had
            // not written: `{"name":"build","name":"other"}` was refused
            // naming `"other"`, the value the swallowing map happened to
            // keep, and an operator looking for `"other"` in a file that says
            // `build` has been sent to the wrong line.
            if !written_twice {
                object.insert(key, element.value);
            }
        }
        Ok(WithEveryKey {
            value: serde_json::Value::Object(object),
            repeated,
        })
    }
}

/// The file's top level, strictly.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileShape {
    #[serde(default)]
    capabilities: Vec<WithEveryKey>,
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
    /// E-27's drain-first, declared **here and only here** (D86).
    ///
    /// A job holding this capability starts only on a quiet host, and the host
    /// claims nothing new while one is waiting. It is the machine operator's
    /// own statement about her own machine, in her own file, exactly as a
    /// probe is: nothing about a label's spelling makes Herd behave this way,
    /// and no declaration living in a *repository* can ever turn into it.
    #[serde(default)]
    drain_first: bool,
}

impl FileCapability {
    fn into_capability(self) -> Result<Capability, CapabilityError> {
        let capability = Capability {
            name: self.name,
            kind: self.kind.into(),
            excludes: LabelSet::new(self.excludes)?,
            busy_probe: self.busy_probe.map(BusyProbe::from),
            drain_first: self.drain_first,
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

/// Why a probe's program could never be spawned on this host, if it could not
/// (D165).
///
/// Asked before the runner starts, and answered the way the kernel will answer
/// it: a program with a `/` in it is that path, and a bare word is looked for
/// on `PATH`, which is what `execvp` does. Nothing is run — this is the
/// question "is there anything here to run at all", not "what does it say",
/// and running the operator's command is `herd-runner check`'s job.
///
/// **Conservative in one direction on purpose.** It answers `Some` only when
/// no spawn could succeed, so a case it cannot see — a program executable only
/// by a user this process is not — passes here and is caught at runtime by
/// D49, suspending its capability as before. A false refusal would be a runner
/// that will not start on a file that works, which is worse than the trap this
/// closes.
fn unstartable(program: &str) -> Option<String> {
    if program.is_empty() {
        return Some("its program is the empty string, which names nothing".to_owned());
    }
    if program.contains('/') {
        return match executable(Path::new(program)) {
            Ok(()) => None,
            Err(why) => Some(format!("its program {program:?} {why}")),
        };
    }
    let mut searched = 0usize;
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        searched += 1;
        if executable(&directory.join(program)).is_ok() {
            return None;
        }
    }
    Some(format!(
        "its program {program:?} is not on this process's PATH, in any of the {searched} \
         directory(ies) on it, so there is nothing to start"
    ))
}

/// Whether a path is something this process could execute, and what is wrong
/// with it when it is not.
fn executable(path: &Path) -> Result<(), String> {
    let found = std::fs::metadata(path).map_err(|error| format!("cannot be looked at: {error}"))?;
    if found.is_dir() {
        return Err("is a directory, and a directory cannot be run".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if found.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "is not executable by anybody (mode {:o})",
                found.permissions().mode() & 0o7777
            ));
        }
    }
    Ok(())
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
///
/// The probe is its own **process group** *and* carries its own marker, and
/// both die with the reading, every time: when the probe answers, when it
/// hangs past the timeout, and when the reading itself is dropped because the
/// runner is stopping. A probe is a command an owner wrote (D51), and the
/// ordinary shape of one is a shell line: killing only the shell leaves
/// whatever it started running, so a probe read every two seconds would strew
/// the host with orphans — a runner destabilising its host through its own
/// configuration, which invariant 2 forbids.
///
/// The group alone was not enough, and this is the whole reason the marker is
/// here. `killpg` reaches everything the probe started *in its group*, TERM
/// -ignoring children included, and a new session is by definition not in it:
/// `/bin/sh -c "setsid sleep 7779 & wait"` as a probe left `sleep 7779`
/// running at ppid 1 after the runner was `SIGTERM`ed. So the reading is
/// swept out of `/proc` by marker afterwards, which is exactly M4's mechanism
/// for a job's processes, reused rather than reinvented
/// ([`crate::reclaim::kill_probe_by_marker`]). [`ProbeGroup`] is what makes
/// both hold on the dropped path too.
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
    let tag = ProbeTag::mint();
    let mut probe = tokio::process::Command::new(program);
    probe
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // The marker the `/proc` sweep looks for. A probe inherits the
        // runner's environment rather than being given a built one (it is the
        // owner's own command, not a job — D51), so this is added to it
        // rather than being the whole of it, and it is kept across every
        // `fork`, `exec`, `nohup` and `setsid` the probe can do.
        .env(crate::reclaim::MARKER_VARIABLE, tag.as_str())
        .kill_on_drop(true);
    // 0 means "a new group led by the child", so the child's own pid is the
    // group's, which is what makes the kill below reach everything it started.
    #[cfg(unix)]
    probe.process_group(0);
    let mut child = match probe.spawn() {
        Ok(child) => child,
        // A command that is not there, or is not executable. The commonest
        // broken probe there is, and the one that must never read as free.
        Err(error) => {
            return Availability::unknown(format!("the probe {shown} cannot be started: {error}"))
        }
    };
    // Armed from here to the end of this reading, whichever way it ends — and
    // that includes not ending at all. A runner asked to stop drops the task
    // that is reading its probes, which drops this future mid-`wait`, and the
    // group has to die then too: `kill_on_drop` reaps the shell and nothing
    // the shell started.
    let mut group = ProbeGroup {
        pid: child.id(),
        tag,
    };
    let waited = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(waited) => waited,
        Err(_) => {
            // A probe that hangs is killed here, with everything it started.
            // It cannot hold the runner up past this timeout, and it never
            // reaches the hub at all: the hub learns a verdict, never a probe.
            group.kill();
            let _ = child.kill().await;
            return Availability::unknown(format!(
                "the probe {shown} did not answer within {} ms and was killed",
                timeout.as_millis()
            ));
        }
    };
    // The probe itself is over; whatever it left running is not. Killed every
    // time, as M4 kills what a job leaves behind: a probe that backgrounds
    // something and exits free would otherwise leak one process per interval.
    group.kill();
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

/// A probe's process group **and** its marker, both killed when the reading
/// ends however it ends.
///
/// A guard rather than a line at each exit, because one of this future's exits
/// is not written anywhere: the task that reads the probes is dropped when the
/// runner is asked to stop, and the reading in flight is then cancelled at its
/// `await`. `kill_on_drop` reaps the probe's own process and nothing else, so a
/// probe of the ordinary shape — `sleep 7777 & wait`, a shell with a child —
/// left that child running, reparented to init, for every stop. That is the
/// orphan-strewing invariant 2 forbids, and it is this type's whole reason to
/// exist: a `Drop` runs on the cancelled path, and an explicit kill cannot.
///
/// Two handles and not one, because they catch different escapes. The group
/// reaches everything the probe started that stayed in it — including a child
/// that ignores `SIGTERM`, since the signal is `SIGKILL`. The marker reaches
/// what left the group: `setsid` is not in the group by definition, and was
/// the one survivor verification found of a probe that is otherwise killed as
/// one (D62).
struct ProbeGroup {
    pid: Option<u32>,
    tag: ProbeTag,
}

impl ProbeGroup {
    /// Kill the group now, and not twice: a pid that has been waited for may
    /// belong to something else by the time anyone asks again.
    ///
    /// The group first, then the sweep, in that order: `killpg` is one
    /// syscall and takes the ordinary case, and the sweep then has only
    /// whatever left the group to find — usually nothing, which is a single
    /// pass over `/proc` and no sleeping at all.
    fn kill(&mut self) {
        if let Some(pid) = self.pid.take() {
            crate::confine::kill_process_group(pid);
            crate::reclaim::kill_probe_by_marker(&self.tag);
        }
    }
}

impl Drop for ProbeGroup {
    fn drop(&mut self) {
        self.kill();
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
    /// Unknown, not free, until a probe has actually been read once. The first
    /// sweep happens *beside* the join and not in front of it — a first sweep
    /// awaited there made joining cost as long as the slowest probe
    /// (`crate::run_until`, `watch`) — so a runner really does announce its offer
    /// with no reading in it, and this is what makes that safe: a probed
    /// capability with no reading is suspended at both ends (D49), so the
    /// answer a job gets is the one that makes it wait.
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
    /// **All of them at once.** A sweep used to read them one after another,
    /// which bounded it by the timeout *times the number of probes*: ten
    /// probes that hang cost twenty seconds, and the runner spent all of it
    /// not being a runner. Nothing ever needed them in order — each one
    /// answers a question about a different thing — so one sweep now costs
    /// one timeout however many probes there are.
    ///
    /// What is still true is that no two evaluations of the *same* probe
    /// overlap: a sweep reads each probe once, and [`watch`] awaits the sweep
    /// before starting the next, so a probe slower than the interval delays
    /// its own next reading rather than running twice over.
    pub async fn sweep(&self) -> bool {
        let readings = futures_util::future::join_all(
            self.inner
                .offer
                .entries()
                .iter()
                .filter_map(|entry| {
                    entry
                        .capability
                        .busy_probe
                        .as_ref()
                        .map(|probe| (entry.name(), probe))
                })
                .map(|(name, probe)| async move {
                    (name, evaluate(probe, self.inner.settings.timeout).await)
                }),
        )
        .await;
        let mut changed = false;
        for (name, read) in readings {
            let was = self.with(|availability| availability.insert(name.to_owned(), read.clone()));
            if was.as_ref() != Some(&read) {
                changed = true;
                match read.reason() {
                    Some(reason) => eprintln!("herd-runner: {name} is suspended: {reason}"),
                    None => eprintln!("herd-runner: {name} is free again"),
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
///
/// The **first** sweep is immediate rather than one interval in, and it is
/// this task's rather than the runner's startup path: the runner joins and
/// the readings follow it. That costs nothing, because a capability whose
/// probe has not been read is suspended at both ends (D49) — the runner says
/// so in its first snapshot, and a hub that has had the offer and no reading
/// suspends it anyway. What it buys is a runner that is a runner while its
/// probes are being read: ten probes that hang used to hold the first dial
/// for as long as they hung.
pub async fn watch(capacity: Capacity, mut on_change: impl FnMut(Vec<CapabilityState>)) {
    let interval = capacity.settings().interval;
    loop {
        if capacity.sweep().await {
            on_change(capacity.snapshot());
        }
        tokio::time::sleep(interval).await;
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

/// What `herd-runner check --capabilities FILE` prints, and whether the file
/// would let a runner start (D165).
///
/// The whole lesson of the outage this comes from is that the file was wrong
/// *before* it was deployed and nothing said so until jobs were late. So this
/// does what the runner does, in the order the runner does it, on a machine
/// where being wrong costs nothing: it reads the file strictly, asks of every
/// command probe whether its program could be started at all, and then runs
/// each probe **once** and prints the verdict.
///
/// Two kinds of bad news, kept apart on purpose. A probe whose program cannot
/// be started refuses — `false` comes back and the caller exits non-zero,
/// because that is exactly what the runner will do with this file. A probe
/// that runs and answers something unreadable is printed and does **not**
/// refuse: that is D49's business, it suspends the capability at runtime, and
/// it is frequently the honest answer of the moment rather than a mistake in
/// the file — a lock that is held, a device in use.
///
/// One caveat, printed rather than hidden: this process's `PATH` is the one
/// searched, and a runner under a unit file may have another. A probe found
/// here and not there is a `PATH` the unit has to be told about.
pub async fn check(path: &Path, probes: ProbeSettings) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let file = match CapabilityFile::read_as_written(path) {
        Ok(file) => file,
        Err(why) => return (vec![why], false),
    };
    lines.push(format!(
        "{} reads: {} capability(ies) declared, {} denied",
        path.display(),
        file.capabilities.len(),
        file.deny.len()
    ));
    let mut refused = Vec::new();
    for capability in &file.capabilities {
        let kind = match capability.kind {
            CapabilityKind::Exclusive => "exclusive".to_owned(),
            CapabilityKind::Counted { slots } => format!("counted, {slots} slot(s)"),
            CapabilityKind::Shared => "shared".to_owned(),
        };
        let mut said = format!("{}: {kind}", capability.name);
        if !capability.excludes.is_empty() {
            said.push_str(&format!(
                ", not alongside {}",
                capability.excludes.as_slice().join(" ")
            ));
        }
        if capability.drain_first {
            said.push_str(", drain-first");
        }
        lines.push(said);
        let Some(probe) = &capability.busy_probe else {
            continue;
        };
        if let BusyProbe::Command { command, .. } = probe {
            if let Some(why) = command.first().and_then(|program| unstartable(program)) {
                let said = format!(
                    "  {}: REFUSED — {why}. A runner will not start on this file",
                    probe.describe()
                );
                refused.push(said.clone());
                lines.push(said);
                continue;
            }
        }
        let read = evaluate(probe, probes.timeout).await;
        lines.push(match &read {
            Availability::Free => format!("  {}: free", probe.describe()),
            Availability::Busy { reason } => format!("  {}: busy — {reason}", probe.describe()),
            // Not a refusal: D49 suspends the capability while this lasts,
            // and the probe answered, so the file named something real.
            Availability::Unknown { reason } => format!(
                "  {}: unreadable — {reason}. This suspends {} for as long as it lasts (D49) \
                 and does not stop a runner starting",
                probe.describe(),
                capability.name
            ),
        });
    }
    for name in &file.deny {
        lines.push(format!("{name}: denied, whatever this host has"));
    }
    if refused.is_empty() {
        lines.push(
            "every probe in this file names a program that can be started. Note that the PATH \
             searched is this process's; a runner started from a unit file may have another"
                .to_owned(),
        );
    } else {
        lines.push(format!(
            "{} probe(s) in this file cannot be started, so a runner given it refuses to \
             start (D165)",
            refused.len()
        ));
    }
    (lines, refused.is_empty())
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

    /// A key written twice refuses the file, naming the key and the
    /// capability — at every level, and whatever the second value is.
    ///
    /// This is the same harm as the unknown key above, by the other route,
    /// and it was live: `busy_probe` written twice, the second one pointing
    /// at a path that is not there, offered `metal-pc` **free while the lock
    /// was held** — the runner said "metal-pc is free again" and a job ran on
    /// a machine an external holder had. The whole of E-26 lost to a
    /// copy-and-paste nobody could see, because a map keeps the last of two
    /// equal keys.
    ///
    /// It is not only the probe. A second `kind` turns an `exclusive`
    /// capability into a shared one, a second `excludes` drops an exclusion
    /// its owner wrote down, and a second `name` is two capabilities in one
    /// entry — the very duplicate-name defect
    /// `a_capability_declared_twice_is_refused_by_name` refuses, slipping
    /// through underneath it.
    #[test]
    fn a_key_written_twice_refuses_the_file_naming_the_key_and_the_capability() {
        // First, the mechanism, stated rather than assumed: plain JSON really
        // does drop the first of two equal keys. That is what this guard is
        // for, and a reader of this test should not have to take it on trust.
        let swallowed: serde_json::Value = serde_json::from_str(
            r#"{"busy_probe":{"type":"lock_file","path":"HELD"},
                "busy_probe":{"type":"lock_file","path":"FREE"}}"#,
        )
        .expect("duplicate keys are legal JSON");
        assert_eq!(
            swallowed["busy_probe"]["path"], "FREE",
            "a map keeps the last of two equal keys, which is the defect"
        );

        for (what, text, keys) in [
            (
                "a probe written twice",
                r#"{"capabilities":[
                    {"name":"metal-pc","kind":{"type":"counted","slots":1},
                     "busy_probe":{"type":"lock_file","path":"HELD"},
                     "busy_probe":{"type":"lock_file","path":"FREE"}}]}"#,
                vec!["busy_probe", "metal-pc"],
            ),
            (
                "a probe written twice, the second one null",
                r#"{"capabilities":[
                    {"name":"metal-pc","kind":{"type":"counted","slots":1},
                     "busy_probe":{"type":"lock_file","path":"HELD"},
                     "busy_probe":null}]}"#,
                vec!["busy_probe", "metal-pc"],
            ),
            (
                "a kind written twice",
                r#"{"capabilities":[
                    {"name":"nightly","kind":{"type":"exclusive"},
                     "kind":{"type":"shared"}}]}"#,
                vec!["kind", "nightly"],
            ),
            (
                "an exclusion list written twice",
                r#"{"capabilities":[
                    {"name":"build","kind":{"type":"shared"},
                     "excludes":["nightly"],"excludes":[]}]}"#,
                vec!["excludes", "build"],
            ),
            (
                "a name written twice",
                r#"{"capabilities":[
                    {"name":"build","name":"metal-pc","kind":{"type":"shared"}}]}"#,
                vec!["name"],
            ),
            (
                "a key written twice inside a kind",
                r#"{"capabilities":[
                    {"name":"build","kind":{"type":"counted","slots":1,"slots":9}}]}"#,
                vec!["kind.slots", "build"],
            ),
            (
                "a key written twice inside a probe",
                r#"{"capabilities":[
                    {"name":"metal-pc","kind":{"type":"shared"},
                     "busy_probe":{"type":"lock_file","path":"HELD","path":"FREE"}}]}"#,
                vec!["busy_probe.path", "metal-pc"],
            ),
            (
                "a key written twice in an entry with no name",
                r#"{"capabilities":[
                    {"kind":{"type":"shared"},"excludes":[],"excludes":[]}]}"#,
                vec!["excludes", "a capability with no name"],
            ),
        ] {
            let why = CapabilityFile::parse(text)
                .err()
                .unwrap_or_else(|| panic!("{what} must be refused, not taken last-wins"));
            for key in keys {
                assert!(why.contains(key), "{what} must name {key}: {why}");
            }
            assert!(
                why.contains("more than once"),
                "{what} must say what is wrong: {why}"
            );
        }

        // And when `name` itself is the key written twice, the capability
        // named is the **first** value — the one the operator wrote and
        // meant. Naming the last sent the operator looking for a capability
        // that is not in the file: the second value is only there because a
        // map keeps it, and `{"name":"build","name":"other"}` was refused
        // naming `"other"`.
        let why = CapabilityFile::parse(
            r#"{"capabilities":[
                {"name":"build","name":"other","kind":{"type":"shared"}}]}"#,
        )
        .expect_err("a name written twice is refused");
        assert!(
            why.contains("\"build\""),
            "the refusal names the capability the operator wrote: {why}"
        );
        assert!(
            !why.contains("other"),
            "and not the one only a swallowing map kept: {why}"
        );

        // And the file's own top-level keys, which serde's own derive
        // refuses — asserted here so that a change of reading cannot drop it
        // silently.
        for (what, text, key) in [
            (
                "the capability list written twice",
                r#"{"capabilities":[{"name":"build","kind":{"type":"shared"}}],
                    "capabilities":[]}"#,
                "capabilities",
            ),
            (
                "the denial list written twice",
                r#"{"capabilities":[],"deny":["kvm"],"deny":[]}"#,
                "deny",
            ),
        ] {
            let why = CapabilityFile::parse(text)
                .err()
                .unwrap_or_else(|| panic!("{what} must be refused"));
            assert!(why.contains(key), "{what} must name {key}: {why}");
        }

        // A runner reads this through `resolve`, and that is where the
        // refusal has to land: the process does not start, so there is no
        // runner to announce anything free.
        let dir = crate::testing::TempDir::new("written-twice");
        let path = dir.path().join("capabilities.json");
        std::fs::write(
            &path,
            br#"{"capabilities":[
                {"name":"metal-pc","kind":{"type":"counted","slots":1},
                 "busy_probe":{"type":"lock_file","path":"HELD"},
                 "busy_probe":{"type":"lock_file","path":"FREE"}}
            ]}"#,
        )
        .expect("written");
        let error = CapabilityConfig {
            file: Some(path.clone()),
            detect: false,
            ..CapabilityConfig::default()
        }
        .resolve()
        .expect_err("a runner does not start on a file that says a probe twice");
        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains("busy_probe"), "{error}");
        assert!(error.contains("metal-pc"), "{error}");
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

    /// **The outage of 2026-10-23, closed at the file.** A probe whose
    /// program cannot be started at all refuses the runner's startup, naming
    /// the file, the capability and the program (D165).
    ///
    /// The real file carried the literal, unsubstituted placeholders
    /// `@probe@` and `@agents_user@`. Every sweep failed with ENOENT, `kvm`
    /// and `gpu-venus` were suspended for as long as that file was live, and
    /// jobs queued behind capabilities that could never clear — while the
    /// runner looked perfectly healthy. The hub said so in every waiting
    /// sentence, which nobody reads until something is already late.
    ///
    /// **This is not D49, and the two must not be conflated.** D49 is the
    /// runtime rule and it is untouched: a probe that *runs* and answers
    /// something unreadable suspends its capability, because an unreadable
    /// probe is never permission to assume a thing nothing outside Herd is
    /// holding. What is added here is a configuration judgement made once,
    /// before the runner starts, about a program that can never answer at
    /// all.
    #[test]
    fn a_probe_whose_program_cannot_be_started_refuses_the_file() {
        let dir = crate::testing::TempDir::new("unstartable-probe");
        let path = dir.path().join("capabilities.json");
        std::fs::write(
            &path,
            br#"{"capabilities":[
                {"name":"kvm","kind":{"type":"shared"},
                 "busy_probe":{"type":"command",
                               "command":["@probe@","qemu","@agents_user@"]}}
            ]}"#,
        )
        .expect("written");
        let error = CapabilityConfig {
            file: Some(path.clone()),
            detect: false,
            ..CapabilityConfig::default()
        }
        .resolve()
        .expect_err("a runner does not start on a probe it could never run");
        assert!(
            error.contains(&path.display().to_string()),
            "the refusal names the file its owner wrote: {error}"
        );
        assert!(
            error.contains("kvm"),
            "and the capability it would suspend: {error}"
        );
        assert!(
            error.contains("@probe@"),
            "and the program that is not there: {error}"
        );

        // The file is still readable *as written*, which is what
        // `herd-runner check` reports from: it names every probe rather than
        // stopping at the first.
        let read = CapabilityFile::read_as_written(&path).expect("the file itself parses");
        assert_eq!(read.unstartable_probes().len(), 1);
    }

    /// And the probes that *can* be started do not refuse anything — neither
    /// a program on `PATH`, nor one by absolute path, nor a lock file, whose
    /// probe runs no program at all and whose unreadability is D49's business
    /// and stays there.
    #[test]
    fn a_probe_that_names_a_real_program_or_a_lock_file_starts_a_runner() {
        let dir = crate::testing::TempDir::new("startable-probe");
        let path = dir.path().join("capabilities.json");
        std::fs::write(
            &path,
            br#"{"capabilities":[
                {"name":"kvm","kind":{"type":"shared"},
                 "busy_probe":{"type":"command","command":["/bin/sh","-c","exit 1"]}},
                {"name":"dri","kind":{"type":"shared"},
                 "busy_probe":{"type":"command","command":["sh","-c","exit 1"]}},
                {"name":"metal-pc","kind":{"type":"counted","slots":1},
                 "busy_probe":{"type":"lock_file","path":"/run/no-such-lock-of-ours"}}
            ]}"#,
        )
        .expect("written");
        let read = CapabilityFile::read(&path)
            .unwrap_or_else(|why| panic!("this file has to start a runner: {why}"));
        assert!(read.unstartable_probes().is_empty());
    }

    /// The judgement itself, on the four shapes it has to tell apart.
    #[test]
    fn a_program_is_unstartable_only_when_no_spawn_could_work() {
        assert!(unstartable("/bin/sh").is_none(), "an executable file");
        assert!(unstartable("sh").is_none(), "a bare word on PATH");
        assert!(unstartable("").is_some(), "the empty string names nothing");
        assert!(
            unstartable("@probe@").is_some(),
            "the placeholder that caused this"
        );
        assert!(
            unstartable("/nonexistent/herd-probe").is_some(),
            "an absolute path with nothing at it"
        );
        assert!(
            unstartable("/tmp").is_some(),
            "a directory is not a program"
        );
        let dir = crate::testing::TempDir::new("unexecutable-probe");
        let script = dir.path().join("probe.sh");
        std::fs::write(&script, b"#!/bin/sh\nexit 1\n").expect("written");
        let why = unstartable(script.to_str().expect("a utf-8 path"))
            .expect("a file nobody may execute cannot be started");
        assert!(why.contains("not executable"), "{why}");
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
