//! The hub's pack registry: the authority on which pack a `(commit, key)` is,
//! and **never a byte of one** (D10, D11, E-12, E-17).
//!
//! **Nothing calls this from the network yet.** The pack frames are numbered
//! in `herd_proto::pack` and are not wired into the hub's connection loop;
//! that is a separate step which owns `herd-hub/src/server.rs`. What is here
//! is the registry and its storage, unit-proven on its own. Its existence is
//! not a working feature: no runner can register a pack with this build and
//! no job is assigned one.
//!
//! ## What it holds, and where
//!
//! ```text
//! <state root>/
//!   packs/
//!     manifests/
//!       <id>/                 <id> is the sha256 of manifest.tsv, lowercase hex
//!         manifest.tsv        the bytes, **verbatim**, as the runner sent them
//!         pack.meta           the bytes, verbatim; absent when the pack has none
//!         registered.json     who registered it, for which (commit, key), when
//!     manifests.part/         registrations that were interrupted; cleared at open
//! ```
//!
//! The directory this adds to the state root is why [`crate::LAYOUT_VERSION`]
//! is 2: a directory is the one kind of change that has no additive escape
//! hatch, since there is no migration tool and none planned
//! (`docs/operating.md`).
//!
//! ## The three properties the shape is for
//!
//! **1. A registration is one `rename(2)`, so it is whole or absent (E-17).**
//! Every file of a pack is written and fsynced inside
//! `packs/manifests.part/<unique>/`, that directory is fsynced, and only then
//! does it take the name `packs/manifests/<id>`. A hub killed anywhere before
//! that rename leaves a directory under `manifests.part/` which **nothing
//! reads**: the pack does not resolve, its id finds no manifest, and the next
//! open removes the residue by name. A hub killed after it leaves a pack that
//! is complete. There is no third outcome, and in particular there is no
//! second rename to be interrupted between.
//!
//! **2. The `(commit, key)` index is derived, never stored.** It is rebuilt at
//! [`PackRegistry::open`] by reading each `registered.json`, exactly as
//! `index/state.json` is rebuilt from the job directories. A durable index
//! would be a second thing to keep true and a second write to interrupt, and
//! it would buy nothing: the directories are the source of truth and they are
//! small.
//!
//! **3. The id is recomputed from the bytes, here as well as on the frame.** A
//! pack directory whose `manifest.tsv` does not hash to the directory's own
//! name is set aside by name and never resolved. The identity rule — `<id>` is
//! the sha256 of `manifest.tsv`'s bytes (the consumer's issue #16, measured
//! 2026-10-13: 401.6 MiB in 557 files) — is not something the hub takes on
//! anybody's word, including the word of a directory name somebody typed.
//!
//! ## Holdings are volatile, and that is the design
//!
//! Which runners hold a complete copy is kept **in memory only**, per manifest
//! and never per blob, and is re-announced after every
//! [`herd_proto::FromHub::Welcome`]. So there is no holdings file to go stale,
//! and a hub that has just restarted offers no holder that is no longer there.
//! The registry survives a restart; the holdings deliberately do not.
//!
//! [`herd_proto::pack::PackState::Incomplete`] is therefore **not the hub's to
//! compute**, and [`PackRegistry::state_of`] never returns it: holdings are per
//! manifest, so "some object of this pack is held nowhere" is a statement about
//! blobs that this hub has no information about. A fetcher consults its own
//! content store for what it lacks, which is where "fetch only the diff"
//! actually happens.
//!
//! ## What is a refusal, and what is an error
//!
//! A refusal is a **value** ([`Registered::Refused`]) and not an `Err`, because
//! it reaches a runner as `FromHub::PackRefused`, which is non-terminal —
//! `FromHub::Refused` closes the link (D13) and a pack nobody can agree about
//! must not cost a runner its connection. An `Err` here is this hub failing to
//! use its own disk.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use herd_proto::pack::{AssignedPack, Holder, PackId, PackManifest, PackState};
use serde::{Deserialize, Serialize};

use crate::durable;
use crate::job::{Result, StoreError, Timestamp, RECORD_VERSION};

/// The directory the registry owns, under the state root.
pub(crate) const PACKS_DIR: &str = "packs";
/// Where the published pack directories live, one per [`PackId`].
const MANIFESTS_DIR: &str = "manifests";
/// Where a registration is assembled before it is published, and the one
/// place an interrupted registration can leave anything.
///
/// A sibling of `manifests/` and not a suffix inside it, for the reason
/// `files.part` is a sibling of `files/`: a reader listing the published
/// directory must never see a pack that is still being written.
const MANIFESTS_PART_DIR: &str = "manifests.part";
const MANIFEST_FILE: &str = "manifest.tsv";
const META_FILE: &str = "pack.meta";
const REGISTERED_FILE: &str = "registered.json";

/// Unique suffixes for staging directories within one process.
static STAGINGS: AtomicU64 = AtomicU64::new(0);

/// `registered.json`: what the hub knows about a pack that is not in the
/// manifest's own bytes.
///
/// It carries [`RECORD_VERSION`] for the reason `job.json` does: the first
/// consumer reads the hub's files directly (E-53) and a file this build cannot
/// read must be named rather than guessed at. A version mismatch here sets
/// that **one pack** aside, exactly as it sets one job aside (D218), and never
/// refuses the root.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegisteredFile {
    record_version: u32,
    /// The id, written beside the directory that is named for it, so that a
    /// directory somebody renamed is detectable rather than merely wrong.
    pack: PackId,
    commit: String,
    key: String,
    /// The runner whose post-fsync frame registered it, by the name the hub
    /// knows it as.
    runner: String,
    /// The job and attempt that produced it, so a pack's provenance is on the
    /// disk and not only in a log line.
    job: String,
    attempt: u32,
    registered_at: Timestamp,
    /// The manifest's own length, so a reader of this file alone can tell a
    /// truncated `manifest.tsv` from a short one.
    manifest_bytes: u64,
    /// How many `kind f` rows the manifest has, and how many bytes they
    /// declare. Printed, never used as a completeness check: the objects live
    /// on the runners and the hub holds none of them.
    objects: u64,
    object_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    meta_bytes: Option<u64>,
}

/// Enough of `registered.json` to read its version before trusting the rest.
///
/// `Option`, and that is the point: a file with **no** `record_version` key is
/// a file that does not say, and a refusal describing an absent field as a
/// field that reads zero is a sentence an operator cannot act on.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    #[serde(default)]
    record_version: Option<u32>,
}

/// What a runner's post-fsync registration frame asks for.
///
/// Minted from `herd_proto::ToHub::RegisterPack` and from nothing else. That
/// frame is the hub's half of E-17: the runner sends it **after** every object
/// of the pack is on its own disk and fsynced, so a pack this hub has
/// registered is a pack that exists somewhere whole. The hub cannot verify
/// that — it holds no byte — which is exactly why the frame is the only thing
/// that registers anything, and why nothing else in this module publishes a
/// pack.
#[derive(Debug, Clone)]
pub struct Registration {
    /// The commit the pack was built at, as the declaration resolved it.
    pub commit: String,
    /// The `leaves` key it answers.
    pub key: String,
    /// The id the runner announced. **Taken as a name, not as a claim**: the
    /// hub recomputes it from `manifest` and refuses a mismatch by name.
    pub announced: PackId,
    /// `manifest.tsv`, verbatim, as it came off the frame.
    pub manifest: Vec<u8>,
    /// `pack.meta`, verbatim, when the pack has one. Not part of the id.
    pub meta: Option<Vec<u8>>,
    pub runner: String,
    pub job: String,
    pub attempt: u32,
    /// The hub's clock, passed in rather than read here, so that what this
    /// module writes is a function of what it was given.
    pub at: Timestamp,
}

/// What became of a registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registered {
    /// Published by one rename. The pack is now readable and resolvable.
    Fresh(PackId),
    /// This exact pack was already registered for this `(commit, key)`;
    /// nothing was written.
    ///
    /// Not a refusal: a runner re-sends its registration after a hub restart
    /// or a lost acknowledgement, and the second send must be the same answer
    /// as the first. The id was still recomputed from the bytes before this
    /// was decided.
    Already(PackId),
    /// Refused, with the sentence a runner is told. Nothing was written.
    Refused(String),
}

impl Registered {
    /// The refusal's sentence, when it is one.
    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Refused(why) => Some(why),
            _ => None,
        }
    }
}

/// One published pack, as the registry knows it without reading its bytes
/// again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub pack: PackId,
    pub commit: String,
    pub key: String,
    pub runner: String,
    pub job: String,
    pub attempt: u32,
    pub registered_at: Timestamp,
    pub manifest_bytes: u64,
    pub objects: u64,
    pub object_bytes: u64,
}

/// What one runner has announced it holds.
#[derive(Debug, Default)]
struct Holding {
    /// Where it serves content, or `None` for an outbound-only runner.
    endpoint: Option<String>,
    /// The packs it holds, as the last **complete** announcement said.
    held: BTreeSet<PackId>,
    /// The frames of an announcement that has not finished arriving
    /// (`more: true`). Applied, and cleared, by the frame that says `more:
    /// false`.
    arriving: BTreeSet<PackId>,
}

#[derive(Debug, Default)]
struct Live {
    /// The derived index: which pack a `(commit, key)` resolves to.
    by_key: BTreeMap<(String, String), PackId>,
    /// `(commit, key)` pairs that two different ids are registered for. A
    /// refusal, by name, at every resolve and at every register — never a
    /// newest-wins.
    conflicts: BTreeMap<(String, String), BTreeSet<PackId>>,
    /// Everything this registry knows about each published pack.
    entries: BTreeMap<PackId, Entry>,
    /// Pack directories this build will not use, and why. Named at every open
    /// and never repaired, exactly as a quarantined job is (V10, D218).
    quarantined: BTreeMap<String, String>,
    /// Who holds what. **In memory only.**
    holdings: BTreeMap<String, Holding>,
}

/// The hub's registry of packs.
#[derive(Debug)]
pub struct PackRegistry {
    root: PathBuf,
    live: Mutex<Live>,
}

impl PackRegistry {
    /// Open, or create, the registry under a state root, and rebuild its
    /// index from the directories.
    ///
    /// Residue from an interrupted registration is removed here, by name. The
    /// manifest of every published pack is read and **re-hashed**: a pack
    /// whose bytes do not hash to its own directory name is set aside rather
    /// than served. That costs one read of each manifest at startup — tens of
    /// kilobytes apiece on the consumer's own measurement — and it is the only
    /// place the identity rule can be checked for a pack this hub did not
    /// receive itself.
    pub fn open(root: &Path) -> Result<Self> {
        let packs = root.join(PACKS_DIR);
        let manifests = packs.join(MANIFESTS_DIR);
        let staging = packs.join(MANIFESTS_PART_DIR);
        for directory in [&packs, &manifests, &staging] {
            durable::create_dir(directory)
                .map_err(|error| failed("creating", directory, &error))?;
        }
        durable::probe_writable(&manifests)
            .map_err(|error| failed("writing in", &manifests, &error))?;

        let registry = Self {
            root: root.to_path_buf(),
            live: Mutex::new(Live::default()),
        };
        registry.clear_staging()?;
        registry.rebuild()?;
        Ok(registry)
    }

    /// Where the registry's directory is, for whoever has to say so.
    pub fn directory(&self) -> PathBuf {
        self.root.join(PACKS_DIR)
    }

    fn manifests_dir(&self) -> PathBuf {
        self.directory().join(MANIFESTS_DIR)
    }

    fn staging_dir(&self) -> PathBuf {
        self.directory().join(MANIFESTS_PART_DIR)
    }

    fn pack_dir(&self, pack: &PackId) -> PathBuf {
        self.manifests_dir().join(pack.as_str())
    }

    /// Remove what an interrupted registration left, and say what was
    /// removed.
    ///
    /// Said out loud because it is the visible trace of a hub that was killed
    /// mid-registration, and a runner is about to be asked for that pack
    /// again. Nothing here was ever readable as a pack, so nothing is lost.
    fn clear_staging(&self) -> Result<()> {
        let staging = self.staging_dir();
        let names =
            durable::names_in(&staging).map_err(|error| failed("listing", &staging, &error))?;
        if names.is_empty() {
            return Ok(());
        }
        for name in &names {
            let at = staging.join(name);
            eprintln!(
                "herd-hub: {} is a pack registration that did not finish; nothing of it was \
                 ever readable as a pack, and it is removed",
                at.display()
            );
            let removed = if at.is_dir() {
                fs::remove_dir_all(&at)
            } else {
                fs::remove_file(&at)
            };
            removed.map_err(|error| failed("removing", &at, &error))?;
        }
        durable::sync_dir(&staging).map_err(|error| failed("syncing", &staging, &error))?;
        Ok(())
    }

    /// Read every published pack directory and build the `(commit, key)`
    /// index from it.
    fn rebuild(&self) -> Result<()> {
        let manifests = self.manifests_dir();
        let names =
            durable::names_in(&manifests).map_err(|error| failed("listing", &manifests, &error))?;
        let mut live = self.locked();
        live.by_key.clear();
        live.conflicts.clear();
        live.entries.clear();
        live.quarantined.clear();
        for name in names {
            let at = manifests.join(&name);
            if !looks_like_an_id(&name) {
                // Not ours, so not a pack. Said, never deleted: this
                // directory belongs to whoever put it here.
                eprintln!(
                    "herd-hub: ignoring {}, whose name is not a pack id",
                    at.display()
                );
                continue;
            }
            let pack = PackId::as_announced(name.clone());
            match self.read_published(&pack)? {
                Ok(entry) => {
                    let key = (entry.commit.clone(), entry.key.clone());
                    let already = live.by_key.get(&key).cloned();
                    match already {
                        Some(first) if first != entry.pack => {
                            let both = live.conflicts.entry(key.clone()).or_default();
                            both.insert(first);
                            both.insert(entry.pack.clone());
                            let why = two_ids(&key.0, &key.1, both);
                            eprintln!("herd-hub: {why}");
                        }
                        _ => {
                            live.by_key.insert(key, entry.pack.clone());
                        }
                    }
                    live.entries.insert(entry.pack.clone(), entry);
                }
                Err(why) => {
                    eprintln!("herd-hub: {why}");
                    live.quarantined.insert(name, why);
                }
            }
        }
        Ok(())
    }

    /// One published pack, read and re-hashed, or the sentence saying why this
    /// build will not use it.
    fn read_published(&self, pack: &PackId) -> Result<std::result::Result<Entry, String>> {
        let dir = self.pack_dir(pack);
        let registered = dir.join(REGISTERED_FILE);
        let text = match read_optional(&registered)? {
            Some(text) => text,
            None => {
                return Ok(Err(format!(
                    "{} holds no {REGISTERED_FILE}, so this build does not know what pack it is \
                     or which (commit, key) it answers; it is set aside and never resolved",
                    dir.display()
                )))
            }
        };
        let probe: VersionProbe = match serde_json::from_str(&text) {
            Ok(probe) => probe,
            Err(error) => {
                return Ok(Err(format!(
                    "{} is not a registration this build can read: {error}",
                    registered.display()
                )))
            }
        };
        if probe.record_version != Some(RECORD_VERSION) {
            return Ok(Err(crate::job::record_version_mismatch_reason(
                &registered,
                probe.record_version,
            )));
        }
        let file: RegisteredFile = match serde_json::from_str(&text) {
            Ok(file) => file,
            Err(error) => {
                return Ok(Err(format!(
                    "{} is not a registration this build can read: {error}",
                    registered.display()
                )))
            }
        };
        let manifest_path = dir.join(MANIFEST_FILE);
        let Some(bytes) = read_optional_bytes(&manifest_path)? else {
            return Ok(Err(format!(
                "{} holds no {MANIFEST_FILE}; a pack is its manifest, so there is nothing here to \
                 resolve and it is set aside",
                dir.display()
            )));
        };
        // The identity rule, checked against the disk and not only against a
        // frame: the directory's name is a claim like any other.
        let computed = PackId::of_manifest_bytes(&bytes);
        if &computed != pack {
            return Ok(Err(format!(
                "{}'s {} bytes hash to {computed}, and the directory is named {pack}. A pack's id \
                 is the sha256 of its manifest's bytes, so this directory holds a different pack \
                 from the one it is named for; it is set aside and never resolved",
                manifest_path.display(),
                bytes.len()
            )));
        }
        if file.pack != computed {
            return Ok(Err(format!(
                "{} says this pack is {}, and its own {} bytes hash to {computed}; it is set \
                 aside and never resolved",
                registered.display(),
                file.pack,
                bytes.len()
            )));
        }
        Ok(Ok(Entry {
            pack: computed,
            commit: file.commit,
            key: file.key,
            runner: file.runner,
            job: file.job,
            attempt: file.attempt,
            registered_at: file.registered_at,
            manifest_bytes: file.manifest_bytes,
            objects: file.objects,
            object_bytes: file.object_bytes,
        }))
    }

    /// Register a pack, on the runner's post-fsync frame and on nothing else
    /// (E-17).
    ///
    /// Three refusals, each by name and each a value rather than an error:
    ///
    /// - a `manifest.tsv` this build cannot parse — refused whole, never
    ///   truncated, because a truncated manifest still parses and would
    ///   register a pack missing files nobody can see;
    /// - the id the runner announced not being the sha256 of the bytes it
    ///   sent, with both digests;
    /// - a `(commit, key)` that already resolves to a **different** id.
    ///
    /// The third is not a newest-wins and will not become one. The consumer's
    /// own promise is that identical outputs give the identical pack, so two
    /// ids for one `(commit, key)` means the outputs really differed; picking
    /// either hands some later test the wrong artefact, silently, and that is
    /// strictly worse than a job that waits with a reason on it.
    pub fn register(&self, registration: Registration) -> Result<Registered> {
        let manifest = match PackManifest::parse(registration.manifest) {
            Ok(manifest) => manifest,
            Err(why) => {
                return Ok(Registered::Refused(format!(
                    "the manifest of the pack {} announced for {}/{} is not one this hub can \
                     read, so no pack is registered for that key and the job that needs it \
                     waits: {why}",
                    registration.announced, registration.commit, registration.key
                )))
            }
        };
        if let Some(why) = manifest.disagreement_with(&registration.announced) {
            return Ok(Registered::Refused(format!(
                "the pack {} announced for {}/{} is refused: {why}. The hub recomputes a pack's \
                 id from the manifest's bytes it was sent and never takes it on a runner's word, \
                 because an id that was not computed over these bytes names a pack whose \
                 completeness nobody has checked",
                registration.announced, registration.commit, registration.key
            )));
        }
        let pack = manifest.id().clone();
        let key = (registration.commit.clone(), registration.key.clone());
        {
            let live = self.locked();
            if let Some(both) = live.conflicts.get(&key) {
                return Ok(Registered::Refused(two_ids(&key.0, &key.1, both)));
            }
            match live.by_key.get(&key) {
                Some(already) if already == &pack => {
                    if self.pack_dir(&pack).join(MANIFEST_FILE).exists() {
                        return Ok(Registered::Already(pack));
                    }
                    // The index says it is here and the disk says it is not.
                    // Fall through and publish: a re-send is how a pack
                    // whose directory was removed under the hub comes back.
                }
                Some(already) => {
                    let mut both = BTreeSet::new();
                    both.insert(already.clone());
                    both.insert(pack.clone());
                    return Ok(Registered::Refused(two_ids(&key.0, &key.1, &both)));
                }
                None => {}
            }
        }

        let objects = manifest.objects_needed();
        let entry = Entry {
            pack: pack.clone(),
            commit: registration.commit.clone(),
            key: registration.key.clone(),
            runner: registration.runner.clone(),
            job: registration.job.clone(),
            attempt: registration.attempt,
            registered_at: registration.at,
            manifest_bytes: manifest.bytes().len() as u64,
            objects: objects.len() as u64,
            object_bytes: objects.values().sum(),
        };
        let file = RegisteredFile {
            record_version: RECORD_VERSION,
            pack: pack.clone(),
            commit: entry.commit.clone(),
            key: entry.key.clone(),
            runner: entry.runner.clone(),
            job: entry.job.clone(),
            attempt: entry.attempt,
            registered_at: entry.registered_at,
            manifest_bytes: entry.manifest_bytes,
            objects: entry.objects,
            object_bytes: entry.object_bytes,
            meta_bytes: registration.meta.as_ref().map(|meta| meta.len() as u64),
        };
        let staged = self.stage(&manifest, registration.meta.as_deref(), &file)?;
        self.publish(staged, &pack)?;

        let mut live = self.locked();
        live.by_key.insert(key, pack.clone());
        live.entries.insert(pack.clone(), entry);
        Ok(Registered::Fresh(pack))
    }

    /// Assemble a registration under `manifests.part/`, every byte of it
    /// fsynced, and hand back the directory that is ready to be renamed.
    ///
    /// Split from [`Self::publish`] so that "the hub was killed before the
    /// pack became visible" is a state a test can produce exactly, at the one
    /// instant it matters, rather than approximate by hand.
    fn stage(
        &self,
        manifest: &PackManifest,
        meta: Option<&[u8]>,
        file: &RegisteredFile,
    ) -> Result<PathBuf> {
        let unique = STAGINGS.fetch_add(1, Ordering::Relaxed);
        let at = self
            .staging_dir()
            .join(format!("{}-{}-{unique}", std::process::id(), file.pack));
        if at.exists() {
            fs::remove_dir_all(&at).map_err(|error| failed("removing", &at, &error))?;
        }
        durable::create_dir(&at).map_err(|error| failed("creating", &at, &error))?;
        // The manifest's **bytes**, never a re-serialisation of the parse:
        // the id is the sha256 of these, so printing the rows back out would
        // mint a different pack.
        let manifest_path = at.join(MANIFEST_FILE);
        durable::write_file(&manifest_path, manifest.bytes())
            .map_err(|error| failed("writing", &manifest_path, &error))?;
        if let Some(meta) = meta {
            let meta_path = at.join(META_FILE);
            durable::write_file(&meta_path, meta)
                .map_err(|error| failed("writing", &meta_path, &error))?;
        }
        let registered = at.join(REGISTERED_FILE);
        let text =
            serde_json::to_vec(file).map_err(|error| failed("writing", &registered, &error))?;
        durable::write_file(&registered, &text)
            .map_err(|error| failed("writing", &registered, &error))?;
        // Each write above fsynced its own file and the directory entry that
        // names it. This is the directory itself, so that the set of names in
        // it survives the rename that publishes them.
        durable::sync_dir(&at).map_err(|error| failed("syncing", &at, &error))?;
        Ok(at)
    }

    /// Make a staged registration visible, with one `rename(2)`.
    ///
    /// The whole of E-17's hub half is this line's being one call: before it
    /// the pack is absent, after it the pack is complete, and there is no
    /// state in between for a crash to leave.
    fn publish(&self, staged: PathBuf, pack: &PackId) -> Result<()> {
        let published = self.pack_dir(pack);
        if published.exists() {
            // Only reachable when the index and the disk disagreed above. A
            // rename onto a non-empty directory fails on Linux, so the stale
            // one comes off first; it was already unresolvable.
            fs::remove_dir_all(&published)
                .map_err(|error| failed("removing", &published, &error))?;
        }
        fs::rename(&staged, &published).map_err(|error| failed("renaming", &staged, &error))?;
        let manifests = self.manifests_dir();
        durable::sync_dir(&manifests).map_err(|error| failed("syncing", &manifests, &error))?;
        Ok(())
    }

    /// Which pack a `(commit, key)` is, and nothing else.
    ///
    /// `Ok(None)` is "nothing is registered for that key", which is a job that
    /// waits with a reason on it and not a refusal (D13). `Err` is reserved
    /// for a disk this hub cannot read; a `(commit, key)` with two ids comes
    /// back through [`Self::resolve`]'s refusal, not through here.
    pub fn pack_for(&self, commit: &str, key: &str) -> std::result::Result<Option<PackId>, String> {
        let at = (commit.to_owned(), key.to_owned());
        let live = self.locked();
        if let Some(both) = live.conflicts.get(&at) {
            return Err(two_ids(commit, key, both));
        }
        Ok(live.by_key.get(&at).cloned())
    }

    /// The pack a job's `needs` key resolves to, with its manifest, its meta
    /// and its holders, ready to ride an assignment.
    ///
    /// The manifest's bytes come off the disk **verbatim**; nothing here
    /// reprints a manifest, because the id is the sha256 of exactly those
    /// bytes.
    pub fn resolve(&self, commit: &str, key: &str) -> Result<Resolved> {
        let pack = match self.pack_for(commit, key) {
            Ok(Some(pack)) => pack,
            Ok(None) => return Ok(Resolved::Nothing),
            Err(why) => return Ok(Resolved::Refused(why)),
        };
        let dir = self.pack_dir(&pack);
        let manifest_path = dir.join(MANIFEST_FILE);
        let Some(manifest) = read_optional_bytes(&manifest_path)? else {
            return Ok(Resolved::Refused(format!(
                "{commit}/{key} resolves to the pack {pack}, and {} is not there. The registry \
                 names a pack this root no longer holds the manifest of, so nothing can be \
                 assigned for that key until a runner registers it again",
                manifest_path.display()
            )));
        };
        let meta = read_optional_bytes(&dir.join(META_FILE))?;
        let holders = self.holders_of(&pack);
        Ok(Resolved::Pack(AssignedPack {
            key: key.to_owned(),
            pack,
            manifest,
            meta,
            holders,
        }))
    }

    /// What a `(commit, key)` is, as one of the named states.
    ///
    /// **Never [`PackState::Incomplete`]**: holdings are per manifest, so this
    /// hub has no information about which blobs are held and inventing the
    /// state would be inventing the information. See the module doc.
    pub fn state_of(
        &self,
        commit: &str,
        key: &str,
    ) -> std::result::Result<Option<(PackId, PackState)>, String> {
        let Some(pack) = self.pack_for(commit, key)? else {
            return Ok(None);
        };
        let state = if self.holders_of(&pack).is_empty() {
            PackState::NoHolder
        } else {
            PackState::Registered
        };
        Ok(Some((pack, state)))
    }

    /// The manifest's bytes, verbatim, or nothing at all.
    pub fn manifest_bytes(&self, pack: &PackId) -> Result<Option<Vec<u8>>> {
        read_optional_bytes(&self.pack_dir(pack).join(MANIFEST_FILE))
    }

    /// `pack.meta`'s bytes, verbatim, or nothing at all.
    pub fn meta_bytes(&self, pack: &PackId) -> Result<Option<Vec<u8>>> {
        read_optional_bytes(&self.pack_dir(pack).join(META_FILE))
    }

    /// Everything this registry holds, by id.
    pub fn entries(&self) -> Vec<Entry> {
        self.locked().entries.values().cloned().collect()
    }

    /// The pack directories this build will not use, and why, by their
    /// directory name.
    ///
    /// Empty on every ordinary root. Named here for the same reason a
    /// quarantined job is: a pack left out of every answer must be visible,
    /// not silently absent.
    pub fn quarantined(&self) -> Vec<(String, String)> {
        self.locked()
            .quarantined
            .iter()
            .map(|(name, why)| (name.clone(), why.clone()))
            .collect()
    }

    /// Every `(commit, key)` two different ids are registered for, with both
    /// ids and the sentence that refuses it.
    pub fn conflicts(&self) -> Vec<((String, String), String)> {
        self.locked()
            .conflicts
            .iter()
            .map(|(at, both)| (at.clone(), two_ids(&at.0, &at.1, both)))
            .collect()
    }

    /// What a runner holds, as it announced it (`ToHub::PacksHeld`).
    ///
    /// **Replace, not add.** An announcement is the whole of what that runner
    /// holds; `more: true` accumulates the frames of one announcement and
    /// `more: false` applies it. An empty announcement with `more: false` is
    /// the real statement "I hold none", which is why it must not be read as
    /// "nothing to say".
    pub fn announced(
        &self,
        runner: &str,
        endpoint: Option<String>,
        packs: Vec<PackId>,
        more: bool,
    ) {
        let mut live = self.locked();
        let holding = live.holdings.entry(runner.to_owned()).or_default();
        holding.endpoint = endpoint;
        holding.arriving.extend(packs);
        if !more {
            holding.held = std::mem::take(&mut holding.arriving);
        }
    }

    /// Forget everything a runner announced, because its link is gone.
    ///
    /// The volatile half of the design: a holder that is not connected is not
    /// a holder, and the registry itself is untouched.
    pub fn runner_gone(&self, runner: &str) {
        self.locked().holdings.remove(runner);
    }

    /// Forget every holding, because this hub is starting again or has asked
    /// the fleet to re-announce (`FromHub::ReadPacksAgain`).
    pub fn forget_all_holdings(&self) {
        self.locked().holdings.clear();
    }

    /// Who holds a complete copy of this pack, in runner-name order.
    pub fn holders_of(&self, pack: &PackId) -> Vec<Holder> {
        self.locked()
            .holdings
            .iter()
            .filter(|(_, holding)| holding.held.contains(pack))
            .map(|(runner, holding)| Holder {
                runner: runner.clone(),
                endpoint: holding.endpoint.clone(),
            })
            .collect()
    }

    /// How many packs each connected runner says it holds, in runner-name
    /// order. For a report; never a placement input here.
    pub fn holdings(&self) -> Vec<(String, usize)> {
        self.locked()
            .holdings
            .iter()
            .map(|(runner, holding)| (runner.clone(), holding.held.len()))
            .collect()
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Live> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// What a `needs` key resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The pack, ready to ride an assignment.
    Pack(AssignedPack),
    /// Nothing is registered for that `(commit, key)`. A waiting state with a
    /// reason, not a refusal that closes anything (D13).
    Nothing,
    /// This hub will not answer for that key, and here is the sentence why.
    Refused(String),
}

/// The one sentence for a `(commit, key)` two different ids are registered
/// for.
///
/// Written once because it is said in three places — at open, at a
/// registration and at a resolve — and a refusal worded three ways is three
/// refusals an operator has to recognise.
fn two_ids(commit: &str, key: &str, both: &BTreeSet<PackId>) -> String {
    let ids: Vec<&str> = both.iter().map(PackId::as_str).collect();
    format!(
        "{commit}/{key} has two different packs registered for it — {} — and this hub refuses to \
         pick one. A pack's id is the sha256 of its manifest, and the promise the whole mechanism \
         rests on is that identical outputs give the identical pack, so two ids for one \
         (commit, key) means the outputs really differed. Choosing either would hand some later \
         job the wrong artefact and say nothing, which is worse than a job that waits. Nothing is \
         assigned for that key until one of the two is removed from packs/manifests/ by whoever \
         owns this root",
        ids.join(" and ")
    )
}

/// Whether a directory name could be a pack id at all: 64 lowercase hex
/// digits, which is what a sha256 is.
fn looks_like_an_id(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("reading", path, &error)),
    }
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("reading", path, &error)),
    }
}

fn failed(what: &str, path: &Path, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::new(format!("{what} {} failed: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture manifest: two `kind f` rows, one `kind d` and one `kind l`.
    ///
    /// The `d` and `l` rows are what make the fixture prove anything. Both
    /// carry a sha256 of something that is **not** a file's bytes, so any
    /// walk of a manifest that does not filter on `kind == f` would count them
    /// as objects — and a manifest with only `f` rows in it would let that
    /// defect through.
    fn a_manifest() -> Vec<u8> {
        let mut text = String::from("# chatons pack 1\n# profile debug\n");
        text.push_str(&format!("{}\t12\t0644\tf\tbin\tbin/tool\n", "1".repeat(64)));
        text.push_str(&format!(
            "{}\t34\t0644\tf\tlib\tlib/libthing.rlib\n",
            "4".repeat(64)
        ));
        text.push_str(&format!("{}\t0\t0755\td\tbin\tbin\n", "2".repeat(64)));
        text.push_str(&format!(
            "{}\t7\t0777\tl\tbin\tbin/tool-latest\tbin/tool\n",
            "3".repeat(64)
        ));
        text.into_bytes()
    }

    fn a_registration(root_at: Timestamp) -> Registration {
        let manifest = a_manifest();
        Registration {
            commit: "c0ffee".to_owned(),
            key: "pack".to_owned(),
            announced: PackId::of_manifest_bytes(&manifest),
            manifest,
            meta: Some(b"commit c0ffee\nprofile debug\n".to_vec()),
            runner: "tower".to_owned(),
            job: "j0000000001".to_owned(),
            attempt: 1,
            at: root_at,
        }
    }

    fn at() -> Timestamp {
        Timestamp::at_unix_seconds(1_793_145_600)
    }

    fn registry() -> (tempfile::TempDir, PackRegistry) {
        let dir = tempfile::tempdir().expect("a temporary root");
        let registry = PackRegistry::open(dir.path()).expect("the registry opens");
        (dir, registry)
    }

    #[test]
    fn a_registration_is_published_and_resolves_with_its_verbatim_bytes() {
        let (_dir, registry) = registry();
        let registration = a_registration(at());
        let expected = registration.announced.clone();
        let outcome = registry.register(registration).expect("registered");
        assert_eq!(outcome, Registered::Fresh(expected.clone()));

        let resolved = registry.resolve("c0ffee", "pack").expect("resolves");
        let Resolved::Pack(pack) = resolved else {
            panic!("{resolved:?}");
        };
        assert_eq!(pack.pack, expected);
        assert_eq!(pack.key, "pack");
        assert_eq!(
            pack.manifest,
            a_manifest(),
            "the bytes come back verbatim, because the id is their sha256"
        );
        assert_eq!(
            PackId::of_manifest_bytes(&pack.manifest),
            expected,
            "and they still hash to the id they were registered under"
        );
        assert_eq!(
            pack.meta.as_deref(),
            Some(&b"commit c0ffee\nprofile debug\n"[..])
        );
        assert!(
            pack.holders.is_empty(),
            "nothing has announced it holds this yet"
        );
    }

    #[test]
    fn the_same_pack_registered_twice_is_already_and_writes_nothing() {
        let (_dir, registry) = registry();
        let first = registry.register(a_registration(at())).expect("registered");
        let pack = match first {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        let again = registry.register(a_registration(at())).expect("registered");
        assert_eq!(
            again,
            Registered::Already(pack),
            "a re-sent registration is the same answer as the first, not a refusal: a runner \
             re-sends after a lost acknowledgement"
        );
    }

    #[test]
    fn an_announced_id_that_is_not_the_sha256_of_the_bytes_is_refused_by_name() {
        let (_dir, registry) = registry();
        let mut registration = a_registration(at());
        let real = registration.announced.clone();
        let lie = PackId::as_announced("0".repeat(64));
        registration.announced = lie.clone();
        let outcome = registry.register(registration).expect("answered");
        let why = outcome.refusal().expect("a refusal");
        assert!(why.contains(lie.as_str()), "{why}");
        assert!(why.contains(real.as_str()), "{why}");
        assert!(
            why.contains("never takes it on a runner's word"),
            "the refusal says what the check is for: {why}"
        );
        assert!(
            matches!(registry.pack_for("c0ffee", "pack"), Ok(None)),
            "and nothing was registered"
        );
        assert!(
            registry.manifest_bytes(&real).expect("read").is_none(),
            "not even under the id the bytes really hash to"
        );
    }

    #[test]
    fn two_ids_for_one_commit_and_key_is_refused_by_name_and_not_a_newest_wins() {
        let (_dir, registry) = registry();
        let first = a_registration(at());
        let first_id = first.announced.clone();
        registry.register(first).expect("registered");

        // The same (commit, key), a different manifest — which is a different
        // pack by definition, since the id is the manifest's sha256.
        let mut other = a_registration(at());
        let mut bytes = a_manifest();
        bytes.extend_from_slice(
            format!("{}\t9\t0644\tf\tbin\tbin/extra\n", "5".repeat(64)).as_bytes(),
        );
        other.manifest = bytes.clone();
        other.announced = PackId::of_manifest_bytes(&bytes);
        let second_id = other.announced.clone();
        assert_ne!(first_id, second_id);

        let outcome = registry.register(other).expect("answered");
        let why = outcome.refusal().expect("a refusal");
        assert!(why.contains(first_id.as_str()), "{why}");
        assert!(why.contains(second_id.as_str()), "{why}");
        assert!(why.contains("refuses to pick one"), "{why}");
        assert!(
            why.contains("identical outputs give the identical pack"),
            "the refusal says why two ids cannot be one pack: {why}"
        );
        assert_eq!(
            registry
                .pack_for("c0ffee", "pack")
                .expect("no conflict yet"),
            Some(first_id),
            "the first registration is untouched: a refusal changes nothing"
        );
        assert!(
            registry.manifest_bytes(&second_id).expect("read").is_none(),
            "and the second pack's bytes were never written"
        );
    }

    #[test]
    fn a_manifest_this_hub_cannot_read_is_refused_rather_than_registered() {
        let (_dir, registry) = registry();
        let mut registration = a_registration(at());
        registration.manifest = b"# some other tool 4\nwhatever\n".to_vec();
        registration.announced = PackId::of_manifest_bytes(&registration.manifest);
        let outcome = registry.register(registration).expect("answered");
        let why = outcome.refusal().expect("a refusal");
        assert!(why.contains("not one this hub can read"), "{why}");
        assert!(
            why.contains("the job that needs it waits"),
            "a refused pack is a named waiting state, never a silence: {why}"
        );
    }

    #[test]
    fn an_interrupted_registration_leaves_nothing_readable_and_is_cleared_at_open() {
        let (dir, registry) = registry();
        let registration = a_registration(at());
        let pack = registration.announced.clone();
        let manifest = PackManifest::parse(registration.manifest.clone()).expect("parses");
        let file = RegisteredFile {
            record_version: RECORD_VERSION,
            pack: pack.clone(),
            commit: registration.commit.clone(),
            key: registration.key.clone(),
            runner: registration.runner.clone(),
            job: registration.job.clone(),
            attempt: registration.attempt,
            registered_at: registration.at,
            manifest_bytes: manifest.bytes().len() as u64,
            objects: 2,
            object_bytes: 46,
            meta_bytes: None,
        };
        // Everything a registration writes, fsynced, and then the hub dies:
        // the rename that publishes it never happens.
        let staged = registry
            .stage(&manifest, registration.meta.as_deref(), &file)
            .expect("staged");
        assert!(
            staged.join(MANIFEST_FILE).exists(),
            "the bytes are on the disk"
        );

        assert!(
            matches!(registry.pack_for("c0ffee", "pack"), Ok(None)),
            "and nothing resolves"
        );
        assert!(
            registry.manifest_bytes(&pack).expect("read").is_none(),
            "and the id finds no manifest"
        );
        assert_eq!(
            registry.resolve("c0ffee", "pack").expect("resolves"),
            Resolved::Nothing
        );

        // A hub starting on that root reads nothing of it either, and removes
        // the residue by name.
        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        assert!(matches!(again.pack_for("c0ffee", "pack"), Ok(None)));
        assert!(again.entries().is_empty());
        assert!(again.quarantined().is_empty());
        assert!(
            !staged.exists(),
            "the interrupted registration is gone: {}",
            staged.display()
        );

        // The control, which is what makes the four assertions above about
        // the interruption and not about a registry that never works: the
        // same registration, published, is readable.
        let outcome = again.register(a_registration(at())).expect("registered");
        assert_eq!(outcome, Registered::Fresh(pack.clone()));
        assert!(again.manifest_bytes(&pack).expect("read").is_some());
    }

    #[test]
    fn the_registry_survives_a_restart_and_the_holdings_do_not() {
        let (dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        registry.announced(
            "tower",
            Some("100.64.0.9:7777".to_owned()),
            vec![pack.clone()],
            false,
        );
        assert_eq!(
            registry.holders_of(&pack),
            vec![Holder {
                runner: "tower".to_owned(),
                endpoint: Some("100.64.0.9:7777".to_owned())
            }]
        );
        assert_eq!(
            registry.state_of("c0ffee", "pack").expect("a state"),
            Some((pack.clone(), PackState::Registered))
        );

        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        assert_eq!(
            again.pack_for("c0ffee", "pack").expect("no conflict"),
            Some(pack.clone()),
            "the registry is on the disk and comes back"
        );
        assert_eq!(
            again.manifest_bytes(&pack).expect("read").as_deref(),
            Some(&a_manifest()[..]),
            "with the manifest's bytes verbatim"
        );
        assert!(
            again.holders_of(&pack).is_empty(),
            "and no holder, because holdings are volatile and re-announced after Welcome"
        );
        assert_eq!(
            again.state_of("c0ffee", "pack").expect("a state"),
            Some((pack, PackState::NoHolder)),
            "which is a named state and not a silence"
        );
    }

    #[test]
    fn an_announcement_replaces_what_a_runner_held_and_an_empty_one_means_none() {
        let (_dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        registry.announced("tower", None, vec![pack.clone()], false);
        assert_eq!(registry.holdings(), vec![("tower".to_owned(), 1)]);
        // An empty announcement is the statement "I hold none", not "nothing
        // to say".
        registry.announced("tower", None, Vec::new(), false);
        assert_eq!(registry.holdings(), vec![("tower".to_owned(), 0)]);
        assert!(registry.holders_of(&pack).is_empty());

        // `more: true` accumulates and does not apply; the frame that says
        // `more: false` is what applies the whole announcement.
        registry.announced("tower", None, vec![pack.clone()], true);
        assert!(
            registry.holders_of(&pack).is_empty(),
            "a holding that has not finished arriving is not a holding"
        );
        registry.announced("tower", None, Vec::new(), false);
        assert_eq!(registry.holdings(), vec![("tower".to_owned(), 1)]);

        registry.runner_gone("tower");
        assert!(registry.holdings().is_empty());
        assert!(registry.holders_of(&pack).is_empty());
    }

    #[test]
    fn a_pack_directory_whose_manifest_does_not_hash_to_its_name_is_set_aside() {
        let (dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        // One byte changed under the hub: the directory's name is now a claim
        // the bytes do not support.
        let mut bytes = a_manifest();
        bytes.push(b'\n');
        let at = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join(pack.as_str())
            .join(MANIFEST_FILE);
        durable::write_file(&at, &bytes).expect("written");

        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        assert!(
            matches!(again.pack_for("c0ffee", "pack"), Ok(None)),
            "it is not resolved"
        );
        let aside = again.quarantined();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(aside[0].0, pack.as_str());
        assert!(aside[0].1.contains(pack.as_str()), "{}", aside[0].1);
        assert!(
            aside[0]
                .1
                .contains(PackId::of_manifest_bytes(&bytes).as_str()),
            "both digests, so a reader knows which way round it is: {}",
            aside[0].1
        );
    }

    #[test]
    fn two_ids_on_the_disk_for_one_key_are_refused_at_every_open() {
        let (dir, registry) = registry();
        let first = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        // A second pack for the same (commit, key), put there by hand: the
        // registry's own `register` refuses this, so the only way a root
        // holds it is somebody else having written it.
        let mut bytes = a_manifest();
        bytes.extend_from_slice(
            format!("{}\t9\t0644\tf\tbin\tbin/extra\n", "5".repeat(64)).as_bytes(),
        );
        let second = PackId::of_manifest_bytes(&bytes);
        let at_dir = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join(second.as_str());
        durable::create_dir(&at_dir).expect("made");
        durable::write_file(&at_dir.join(MANIFEST_FILE), &bytes).expect("written");
        durable::write_file(
            &at_dir.join(REGISTERED_FILE),
            &serde_json::to_vec(&RegisteredFile {
                record_version: RECORD_VERSION,
                pack: second.clone(),
                commit: "c0ffee".to_owned(),
                key: "pack".to_owned(),
                runner: "elsewhere".to_owned(),
                job: "j0000000002".to_owned(),
                attempt: 1,
                registered_at: at(),
                manifest_bytes: bytes.len() as u64,
                objects: 3,
                object_bytes: 55,
                meta_bytes: None,
            })
            .expect("serialises"),
        )
        .expect("written");

        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        let why = again
            .pack_for("c0ffee", "pack")
            .expect_err("two ids is a refusal");
        assert!(why.contains(first.as_str()), "{why}");
        assert!(why.contains(second.as_str()), "{why}");
        let resolved = again.resolve("c0ffee", "pack").expect("answered");
        assert!(
            matches!(resolved, Resolved::Refused(ref said) if said == &why),
            "the resolve says the same sentence: {resolved:?}"
        );
        // And a third registration for that key is refused too, rather than
        // picking the newest.
        let outcome = again.register(a_registration(at())).expect("answered");
        assert!(outcome.refusal().is_some(), "{outcome:?}");
        assert_eq!(again.conflicts().len(), 1);
    }

    #[test]
    fn a_registration_of_another_record_version_sets_that_one_pack_aside() {
        let (dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        let at_file = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join(pack.as_str())
            .join(REGISTERED_FILE);
        // One past whatever this build reads, computed rather than written
        // down, so this test does not have to be edited when the number
        // moves.
        let newer = RECORD_VERSION + 1;
        durable::write_file(
            &at_file,
            format!(r#"{{"record_version":{newer},"pack":"{pack}"}}"#).as_bytes(),
        )
        .expect("written");

        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        let aside = again.quarantined();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(
            aside[0].1.contains(&format!("record version {newer}")),
            "{}",
            aside[0].1
        );
        assert!(
            aside[0].1.contains(&format!("version {RECORD_VERSION}")),
            "{}",
            aside[0].1
        );
        assert!(matches!(again.pack_for("c0ffee", "pack"), Ok(None)));
    }

    #[test]
    fn a_registration_with_no_record_version_at_all_is_said_to_be_absent() {
        let (dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        let at_file = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join(pack.as_str())
            .join(REGISTERED_FILE);
        durable::write_file(&at_file, br#"{"pack":"x"}"#).expect("written");

        drop(registry);
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        let aside = again.quarantined();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(
            aside[0].1.contains("carries no record version at all"),
            "an absent field is described as absent and never as a field that reads zero: {}",
            aside[0].1
        );
        assert!(!aside[0].1.contains("record version 0"), "{}", aside[0].1);
    }

    #[test]
    fn the_objects_counted_on_a_registration_are_the_kind_f_rows_and_nothing_else() {
        let (_dir, registry) = registry();
        let pack = match registry.register(a_registration(at())).expect("registered") {
            Registered::Fresh(pack) => pack,
            other => panic!("{other:?}"),
        };
        let entries = registry.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].objects, 2,
            "the fixture has four rows: two `kind f`, one `d` and one `l`. The `d` and `l` rows \
             carry a sha256 of something that is not a file's bytes, so counting them would call \
             a good pack's object list longer than it is — and a completeness check built on that \
             count would look them up under objects/ and report a good pack as incomplete"
        );
        assert_eq!(entries[0].object_bytes, 12 + 34);
        assert_eq!(entries[0].pack, pack);
        assert_eq!(entries[0].runner, "tower");
        assert_eq!(entries[0].job, "j0000000001");
    }

    #[test]
    fn a_directory_that_is_not_a_pack_id_is_left_alone_and_never_read() {
        let (dir, registry) = registry();
        drop(registry);
        let at_dir = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join("README");
        durable::create_dir(&at_dir).expect("made");
        let again = PackRegistry::open(dir.path()).expect("the registry opens");
        assert!(again.entries().is_empty());
        assert!(
            again.quarantined().is_empty(),
            "not ours, so not a pack and not a fault"
        );
        assert!(at_dir.exists(), "and never removed: it is not Herd's");
    }

    /// Every byte of a pack is on the disk **before** the pack is visible,
    /// asserted on the trace of the durable operations themselves rather than
    /// on a reading of the code.
    ///
    /// The two halves are taken apart deliberately: the trace says every one
    /// of the three files was `fsync`ed, and the filesystem says the published
    /// name did not exist while that was happening. Together they are E-17's
    /// hub half — a pack is complete or absent — and neither on its own is.
    #[test]
    fn nothing_of_a_pack_is_visible_before_every_byte_of_it_is_synced() {
        let dir = tempfile::tempdir().expect("a temporary root");
        let registry = PackRegistry::open(dir.path()).expect("the registry opens");
        let registration = a_registration(at());
        let pack = registration.announced.clone();
        let manifest = PackManifest::parse(registration.manifest.clone()).expect("parses");
        let file = RegisteredFile {
            record_version: RECORD_VERSION,
            pack: pack.clone(),
            commit: registration.commit.clone(),
            key: registration.key.clone(),
            runner: registration.runner.clone(),
            job: registration.job.clone(),
            attempt: registration.attempt,
            registered_at: registration.at,
            manifest_bytes: manifest.bytes().len() as u64,
            objects: 2,
            object_bytes: 46,
            meta_bytes: None,
        };
        let published = dir
            .path()
            .join(PACKS_DIR)
            .join(MANIFESTS_DIR)
            .join(pack.as_str());

        let watch = durable::watch::armed(dir.path());
        let _ = watch.take();
        let staged = registry
            .stage(&manifest, registration.meta.as_deref(), &file)
            .expect("staged");
        let trace = watch.take();
        drop(watch);

        for name in [MANIFEST_FILE, META_FILE, REGISTERED_FILE] {
            let path = staged.join(name);
            assert!(
                trace
                    .iter()
                    .any(|step| step.op == durable::watch::Op::TemporarySynced
                        && step.path == path),
                "{name}'s bytes were fsynced before anything was published: {trace:?}"
            );
            assert!(
                trace.iter().any(|step| step.op
                    == durable::watch::Op::DirectorySynced
                    && step.path == path),
                "and so was the directory entry naming it: {trace:?}"
            );
        }
        assert!(
            !published.exists(),
            "and the published name does not exist yet: {}",
            published.display()
        );
        assert!(
            trace.iter().all(|step| !step.path.starts_with(&published)),
            "nothing of the staging touched the published name: {trace:?}"
        );

        // The one call that makes it visible, and the control for all of the
        // above: before it nothing resolves, after it everything does.
        assert!(registry.manifest_bytes(&pack).expect("read").is_none());
        registry.publish(staged, &pack).expect("published");
        assert!(published.exists());
        assert!(registry.manifest_bytes(&pack).expect("read").is_some());
    }
}
