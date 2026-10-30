//! The runner's content-addressed pack store: the bytes one job leaves for a
//! later job of the same commit, kept once per content under the data root.
//!
//! **Nothing in this module binds a socket, dials, fetches or serves.** It is
//! a directory on this machine's disk and the four operations a disk needs:
//! put a pack in, say what is missing from one, lay one out for a job, and
//! take a job's declared pack out of `{out}`. The peer fetch endpoint, the
//! transfer client and the registration frame are later steps of M11 and are
//! owned elsewhere — so the existence of this module is **not** a working
//! pack feature, and a reader who finds it and concludes that a runner can
//! obtain a pack from a peer today is reading more into it than is here.
//!
//! ## The shape, which is the consumer's and not ours
//!
//! The first consumer measured their own pack on 2026-10-13 (their issue
//! #16): 401.6 MiB in 557 files, built in 38 s, verified in 1.5–2.1 s, in
//! three parts:
//!
//! ```text
//! <pack>/manifest.tsv      "# chatons pack 1", "# profile debug", then per entry:
//!                          sha256 TAB size TAB octal mode TAB kind f|d|l TAB role TAB path [TAB link target]
//! <pack>/pack.meta         commit, profile, scenarios, sizes per class — not part of the id
//! <pack>/objects/<sha256>  each file's bytes once, under its own sha256
//! ```
//!
//! That is the shape of a **pack directory**: what a job leaves at its
//! declared `leaves` path under `{out}`, and what a later job is given at
//! `{in}/<key>`. The two are deliberately the same shape, read one way and
//! written the other, so there is one thing to understand and not two.
//!
//! **The store's own layout is not that shape**, and the difference is the
//! point of the store:
//!
//! ```text
//! <data root>/packs/objects/<sha256>           every pack's bytes, once per content
//! <data root>/packs/manifests/<id>/manifest.tsv  the bytes, verbatim
//! <data root>/packs/manifests/<id>/pack.meta     when the pack had one
//! <data root>/packs/refused/<job>-<attempt>/<declared path>
//!                                             a declared pack Herd could not
//!                                             read, kept exactly as the job
//!                                             left it
//! ```
//!
//! Objects are shared across packs — two packs of two commits that differ in
//! one file hold one copy of everything else — which is why they live beside
//! the manifests rather than under them. A pack directory laid out for a job
//! is **hard links** into `objects/`, so handing a 400 MiB pack to a job
//! costs the directory entries and not the bytes; [`LaidOut::bytes_copied`]
//! is the figure that says so, and it is reported rather than assumed.
//!
//! ## What it occupies, as a figure it keeps rather than a walk it repeats
//!
//! The store carries its own byte total and pack count
//! ([`PackStore::usage`]) and the budget its operator gave it
//! ([`PACK_BUDGET_FLAG`]). **One walk seeds them and nothing walks again**:
//! [`PackStore::measure`] reads `objects/` once at startup, each
//! [`PackStore::ingest`] adds the bytes it really placed, and every other
//! reader does arithmetic. That is not an optimisation of a figure nobody
//! needed — a runner announces what it holds at every welcome and at every
//! change, and a total derived by walking would read the whole store each
//! time, against the consumer's own 401.6 MiB in 557 files. The counter
//! [`PackStore::objects_listed`] exists so that a test can assert the walk
//! does not happen, by counting the calls rather than by timing them.
//!
//! **Nothing here evicts anything.** The budget is measured against and
//! reported; what spends it is a later step, and a reader who finds a budget
//! and concludes this runner enforces one is reading more into it than is
//! here.
//!
//! ## Three rules this module exists to get right
//!
//! 1. **A pack's id is the sha256 of `manifest.tsv`'s bytes**, so a manifest
//!    that is parsed and reprinted is a *different pack*. Nothing here ever
//!    writes manifest text: what goes to disk is
//!    [`herd_proto::PackManifest::bytes`], the bytes that were read, and a
//!    test compares the stored file against the source file byte for byte.
//!    The type itself makes the mistake unavailable (see its own doc), and
//!    this module keeps it that way.
//! 2. **Only `kind f` rows name an object.** A `kind d` or `kind l` row
//!    carries a sha256 too, of something that is not a file's bytes, so a
//!    completeness check over *every* sha256 calls a perfectly good pack
//!    incomplete. Every question this module asks about completeness goes
//!    through [`herd_proto::PackManifest::objects_needed`], which filters,
//!    and the fixtures carry a `d` row and an `l` row so the filter is proven
//!    and not assumed.
//! 3. **Herd does not destroy an operator's bytes.** A declared pack this
//!    runner cannot ingest leaves `{out}` — it is not the record's business
//!    either way, and ~400 MiB of unreadable pack left where it is would put
//!    the whole collection over the job's output cap so that `results.json`
//!    does not come back for it — but it is **set aside and kept**, under
//!    `refused/` above, with the reason and its figures said on the record
//!    and on stderr. Nothing in this runner ever removes it: not a timer, not
//!    [`crate::sweep`], not [`crate::reclaim`]'s reaper, not a start. The
//!    same shape the sweep already takes with a spool no hub will ask for
//!    (`.unclaimed`) and the hub with a record it cannot read, for the same
//!    reason: removing hundreds of megabytes is a decision with an owner, and
//!    the owner is the person whose disk it is. And the same **terminal**
//!    rule, so the state does not flip every boot — bytes under `refused/`
//!    are never ingested, never laid out and never looked at again.
//! 4. **A pack is complete or absent** (E-17). Objects are placed under a
//!    staging name, fsynced and renamed, so a partial object is never visible
//!    under its final name — a later completeness check would count it, and a
//!    pack missing those bytes would read as whole. The manifest is written
//!    **last**, after every object is on the disk: an ingest killed anywhere
//!    leaves objects (each one whole, or not there at all) and no pack.
//!
//! ## What is checked, and what is taken on trust
//!
//! Every object's bytes are hashed as they are placed and refused if they
//! disagree with the name they were going to be stored under. A
//! content-addressed store whose `objects/<sha>` does not hash to `<sha>` is
//! a lie that propagates: the next machine fetches those bytes by that name
//! and verifies nothing, because it asked for a name it trusts. The cost is
//! one read of the pack — the consumer's own figure for verifying 401.6 MiB
//! is 1.5–2.1 s — and it is paid on ingest, once per content.
//!
//! The sha256 column is also checked to be **64 lowercase hex characters
//! before it is joined onto a path** — a manifest is the consumer's file, and
//! a row whose first column read `../../something` would otherwise name a
//! path outside the store. **The rule itself lives in `herd-proto`**, as
//! `herd_proto::pack::check_object_digest`, applied at the one place a
//! `ManifestEntry` is built and `pub` precisely so that the readers which
//! join that column onto a path delegate rather than keep a copy. This
//! module delegates, in `object_name`. It carried its own hand-rolled
//! copy until 2026-10-30, and that copy had already drifted far enough to
//! cost a red gate.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use herd_proto::pack::{check_object_digest, PacksHeld};
use herd_proto::{show_bytes, PackId, PackManifest, MANIFEST_MAX_BYTES};

use crate::size::measured_bytes;

/// The subtree of the data root this store owns, beside `jobs/`,
/// `held-results/` and `outputs/`.
pub const PACKS_DIR: &str = "packs";

/// The flag that tells a runner how much of its disk this store may hold.
///
/// Named once, here, where the budget lives, so that the startup line, the
/// health report and whatever eviction comes later all quote the same string.
/// `mesh::LISTEN_FLAG`'s precedent: a flag spelled out in three sentences is
/// a flag renamed in two of them.
pub const PACK_BUDGET_FLAG: &str = "--pack-budget";

/// Where every pack's bytes live, once per content.
const OBJECTS_DIR: &str = "objects";

/// Where one directory per pack id holds that pack's manifest and meta.
const MANIFESTS_DIR: &str = "manifests";

/// Where a declared pack this runner could not ingest is kept.
///
/// Beside `objects/` and `manifests/`, and named for **what it is** rather
/// than for when it happened or for what is to become of it. Those two hold
/// the store proper — content a runner can hand to a job — and this is the
/// third thing a declared pack can turn out to be: bytes Herd holds and
/// cannot read. `refused` is the word this module already uses in every
/// sentence that says so (`is refused by name`, `a short object is
/// refused`), so an operator who reads one of those lines and then looks at
/// the disk finds the same word on it; `rejected`, `quarantine`, `bad` and
/// `orphans` would each have been a second vocabulary for one fact, and
/// `pending` or `retry` would promise something no code here does.
///
/// It is created **on demand** and never by [`PackStore::open`], for the
/// reason `packs/` itself is: an empty directory on every runner's disk is a
/// feature announcing itself before it exists.
///
/// **Kept as the job left it includes the modes it left.** Nothing under here
/// restores a directory's permissions — the record walk takes the runner's
/// access back under `{out}` and [`crate::reclaim`]'s reaper does it under
/// `jobs/`, and neither of them comes here — so a pack whose job chmodded its
/// directories to `0000` is kept shut, and the operator whose disk it is may
/// need `sudo` to read their own bytes. That is the promise working rather
/// than a defect: what is here is byte for byte and mode for mode what was
/// declared, and a runner that quietly re-opened it would be editing it. The
/// rename itself is unaffected, because it needs write access on `{out}` and
/// not on the directory being moved.
pub const REFUSED_DIR: &str = "refused";

/// The file whose bytes *are* the pack's identity.
pub const MANIFEST_FILE: &str = "manifest.tsv";

/// The pack's provenance, which is **not** part of its id: a pack with no
/// `pack.meta` is an ordinary pack.
pub const META_FILE: &str = "pack.meta";

/// How many pack ids ride one [`herd_proto::ToHub::PacksHeld`] frame.
///
/// **Chosen against a frame ceiling that is measured and not guessed**: the
/// hub refuses a frame over `MAX_FRAME_BYTES`, which is `1 << 20`
/// (`herd-hub/src/server.rs:167`, read 2026-10-30). A [`PackId`] is 64 hex
/// characters, so one id costs 67 bytes of JSON array — two quotes and a
/// comma — and 512 of them is about 34 KiB, some thirty times under that
/// ceiling with the rest of the frame's envelope nowhere near the difference.
///
/// It is **deliberately far below** what would fit, and the two questions are
/// not the same one: what a hub can read is a ceiling, while this is what
/// decides whether the `more: true` path is walked by a store of a few
/// hundred packs or only by one of fifteen thousand — and a path nothing in
/// the field ever walks is a path that is not really there. The test fixture
/// holds `PACKS_HELD_PER_FRAME + 4` for the same reason `FILES_IN_FLIGHT + 4`
/// is the figure elsewhere: a fixture at or below the window measures the
/// wrong state.
pub const PACKS_HELD_PER_FRAME: usize = 512;

/// What an object or a manifest directory is called while it is being made.
///
/// Two properties, and both are load-bearing. It begins with a dot and holds
/// a `-`, so it can never be mistaken for a 64-hex object name by anything
/// that looks one up — which is the whole reason a partial object cannot be
/// counted as a present one. And it is unique per process and call, so two
/// ingests of the same pack on one runner stage into two names and neither
/// sees the other's half-written bytes.
const STAGING_PREFIX: &str = ".ingest-";

static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

/// One validated pack store under one data root.
///
/// **The figures are shared per data root and not per value.** A
/// [`PackStore`] is opened wherever one is needed — the collection opens one
/// at the end of a job, the announcement opens one every time it says what
/// this runner holds — so a total kept in the value would be measured again
/// at each of them, which is the walk this store must not do. What it holds
/// instead is a handle onto one [`Standing`] per data root per process: the
/// running total, the pack count and the budget, seeded once by
/// [`PackStore::measure`] and moved by each [`PackStore::ingest`].
#[derive(Debug, Clone)]
pub struct PackStore {
    path: PathBuf,
    standing: Arc<Mutex<Standing>>,
}

/// What this store occupies, as a figure it keeps rather than a walk it
/// repeats.
///
/// Both figures count exactly what the store's own two rules count:
/// `bytes` is every `objects/` entry whose name is a sha256
/// ([`check_object_digest`]'s rule, so a staging name is never counted), and
/// `packs` is every `manifests/` entry [`PackStore::held_packs`] would
/// announce. One definition per figure, so what the budget is spent against
/// and what the hub is told are the same two numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackUse {
    /// Every object's bytes, each content counted once — which is what the
    /// store costs the disk, because an object is one file however many
    /// packs name it.
    pub bytes: u64,
    /// How many whole packs this store holds.
    pub packs: u64,
    /// Whether anything has read the store yet.
    ///
    /// **False is not zero.** A figure nobody has measured is a figure this
    /// runner has not got, and the two must not be confused: a hub told
    /// `0 bytes` by a runner that never looked would believe an empty store.
    /// [`PackStore::measure`] is what makes it true.
    pub measured: bool,
}

/// One data root's pack-store figures, held once per process.
///
/// A `Mutex` and not five atomics, because nothing here is on a hot path —
/// an ingest, an announcement, a startup — and because the alternative is
/// five independent loads of figures that are read together and have to agree.
#[derive(Debug, Default)]
struct Standing {
    bytes: u64,
    packs: u64,
    measured: bool,
    /// What the operator said this store may hold, in bytes. **`None` is
    /// "nobody said"**, never nought: a budget of nought is a store that
    /// holds nothing, and absent is a store nothing bounds.
    budget: Option<u64>,
    /// How many times `objects/` has been listed, in this process, for this
    /// data root.
    ///
    /// Not a diagnostic: it is the assertion. The one thing this task had to
    /// get right is that saying what a runner holds costs no walk of the
    /// objects directory, and a test that timed two announcements on a host
    /// shared with the first consumer's production would be measuring the
    /// host. So every listing of that directory goes through
    /// [`PackStore::objects_bytes`], which counts itself here, and the test
    /// asserts the counter does not move across an announcement. The claim
    /// the counter rests on — that `objects_bytes` is the only listing of
    /// `objects/` in this crate's product code — is checked by `grep` and
    /// stated in that test rather than left to be inferred.
    objects_listed: u64,
}

impl Standing {
    fn reading(&self) -> PackUse {
        PackUse {
            bytes: self.bytes,
            packs: self.packs,
            measured: self.measured,
        }
    }
}

/// The one [`Standing`] for this data root's store, made on first use.
///
/// One entry per data root, and a process has one data root: the map exists
/// because a **test** process has many, each in its own temporary directory,
/// and because keying the figures by path is what lets every
/// [`PackStore::open`] of one root read the figure the first of them seeded.
///
/// The key is [`keyed`]'s answer and not the path as given, for the reason
/// that function carries.
fn standing_for(path: &Path) -> Arc<Mutex<Standing>> {
    static STANDINGS: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Mutex<Standing>>>>> = OnceLock::new();
    let standings = STANDINGS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut standings = standings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(
        standings
            .entry(keyed(path))
            .or_insert_with(|| Arc::new(Mutex::new(Standing::default()))),
    )
}

/// This store's directory as **every** caller will spell it.
///
/// [`crate::root::DataRoot::open`] canonicalises the root it validates, so
/// everything inside a running runner opens this store under a canonical
/// path — while the path the operator typed, which is what the startup line
/// has, may be relative, may end in a separator, or may run through a
/// symlinked home. Two spellings would be two [`Standing`]s: the budget
/// would be registered against the one nobody reads and the figure would
/// start from nothing at the first announcement. That is a silent failure
/// and it would be found, if at all, in a health report of a machine nobody
/// in this project may log into (D161).
///
/// So the key is canonical, and it is derived **creating nothing**: the
/// nearest ancestor that exists is canonicalised and the part that does not
/// exist yet is joined back on. That is the same answer `canonicalize` gives
/// once the root is made, because what makes it is `create_dir_all` and a
/// directory `create_dir_all` made is not a symlink. A path whose every
/// ancestor is unreadable falls back to itself, which is one spelling and
/// therefore still one figure.
fn keyed(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            break;
        }
        if let Ok(real) = fs::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) => real.join(rest),
                Err(_) => real,
            };
        }
    }
    path.to_path_buf()
}

/// Take the budget for this data root's store before anything has opened it,
/// and answer the line to print about it.
///
/// **Reads the store and creates nothing.** This runs at startup, where the
/// data root has not yet been validated: creating `packs/` here would take
/// E-45's refusal out of the hands of the code that owns it and leave a
/// directory behind on a root that was about to be refused by name. A store
/// that is not there yet holds nothing, which is a measurement and not an
/// error.
///
/// The figure the operator typed reads back through [`show_bytes`] and the
/// figure this runner measured through [`measured_bytes`] — the asymmetry
/// `herd_proto::size` exists for, and the reason this sentence carries both.
pub fn take_budget(data_root: &Path, budget: Option<u64>) -> String {
    let store = PackStore::at(data_root);
    store.set_budget(budget);
    let held = match store.measure() {
        Ok(held) => held,
        Err(why) => {
            return match budget {
                Some(budget) => format!(
                    "this runner's pack store may hold {} ({PACK_BUDGET_FLAG}) and what it \
                     holds now was not read: {why}",
                    show_bytes(budget)
                ),
                None => format!(
                    "this runner's pack store has no budget ({PACK_BUDGET_FLAG} sets one) and \
                     what it holds now was not read: {why}"
                ),
            }
        }
    };
    match budget {
        Some(budget) => format!(
            "this runner's pack store holds {} in {} pack(s) of the {} it may hold \
             ({PACK_BUDGET_FLAG})",
            measured_bytes(held.bytes),
            held.packs,
            show_bytes(budget)
        ),
        None => format!(
            "this runner's pack store holds {} in {} pack(s) and has no budget: \
             {PACK_BUDGET_FLAG} is what bounds it, and without one this machine's packs are \
             bounded by its disk",
            measured_bytes(held.bytes),
            held.packs
        ),
    }
}

/// A pack that is now in the store, and what putting it there cost.
///
/// This is what the step that registers a pack with the hub needs and all it
/// needs: the manifest's verbatim bytes (and therefore its id) and the meta
/// beside it. The figures are here because they are measurements — what a
/// hard link really costs is nought, and a figure that says so is worth more
/// than a comment claiming it.
#[derive(Debug)]
pub struct Held {
    /// The manifest, with its bytes verbatim and its id computed over them.
    pub manifest: PackManifest,
    /// `pack.meta`'s bytes, when the pack had one.
    pub meta: Option<Vec<u8>>,
    /// How many distinct objects a complete copy of this pack holds: the
    /// `kind f` rows, by sha256.
    pub objects: u32,
    /// How many of them this ingest placed. The rest were already in the
    /// store, under the same name, at the same size — the dedup, as a figure.
    pub objects_added: u32,
    /// Bytes placed by hard link, which cost the filesystem nothing but a
    /// directory entry.
    pub bytes_linked: u64,
    /// Bytes placed by copy, which cost a second copy on the disk. Nought
    /// whenever the pack and the store are on one filesystem, which is what
    /// both being under the data root means.
    pub bytes_copied: u64,
}

/// A pack laid out for a job, and what laying it out cost.
#[derive(Debug)]
pub struct LaidOut {
    /// How many objects were linked or copied into the directory.
    pub objects: u32,
    /// Bytes shared with the store rather than duplicated.
    pub bytes_linked: u64,
    /// Bytes that had to be copied, which is a second copy on the disk. The
    /// zero-extra-bytes promise is exactly this figure being nought.
    pub bytes_copied: u64,
}

/// One pack taken out of a job's `{out}` and put in the store.
///
/// Handed back from the collection so the step that owns the hub's
/// registration frame can register it: the key the requester declared, the
/// path it was at, and the pack itself.
#[derive(Debug)]
pub struct LeftPack {
    /// The requester's own key for this artefact — `leaves = { pack =
    /// "packs" }` gives `pack`.
    pub key: String,
    /// Where it was under `{out}`, as declared. Kept for the report: it is
    /// not a path anything joins onto anything afterwards.
    pub at: String,
    /// The pack, as the store now holds it.
    pub held: Held,
}

impl PackStore {
    /// Open — and create — the pack store under a validated data root.
    ///
    /// Takes the data root's path rather than the [`crate::root::DataRoot`]
    /// itself so that the collection can open the store from what it already
    /// has. Creating the two directories is the whole of it: there is no
    /// index, no database and nothing to recover, because the content
    /// addressing *is* the index and a name either exists or does not.
    ///
    /// **Opened again, it reads the same figures**: what the store occupies
    /// belongs to the data root and not to the value (see [`Standing`]), so
    /// the announcement — which opens a store every time it says what this
    /// runner holds — pays no walk for it.
    pub fn open(data_root: &Path) -> Result<Self, String> {
        let store = Self::at(data_root);
        for directory in [store.objects_dir(), store.manifests_dir()] {
            fs::create_dir_all(&directory).map_err(|error| {
                format!(
                    "the pack store {} cannot be created: {error}",
                    directory.display()
                )
            })?;
        }
        Ok(store)
    }

    /// Name this data root's store, **creating nothing**.
    ///
    /// [`PackStore::open`] is what a caller that is about to *use* the store
    /// wants: it creates the two directories and refuses by name if it
    /// cannot. This is for the caller that only wants to read the store or
    /// to remember something about it — [`take_budget`] at startup, before
    /// the data root is validated at all — and it is deliberately private
    /// for that reason.
    fn at(data_root: &Path) -> Self {
        let path = data_root.join(PACKS_DIR);
        let standing = standing_for(&path);
        Self { path, standing }
    }

    fn standing(&self) -> MutexGuard<'_, Standing> {
        self.standing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The store's own directory: `<data root>/packs`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where every pack's bytes live, once per content.
    pub fn objects_dir(&self) -> PathBuf {
        self.path.join(OBJECTS_DIR)
    }

    /// Where one directory per pack id holds its manifest.
    pub fn manifests_dir(&self) -> PathBuf {
        self.path.join(MANIFESTS_DIR)
    }

    /// Where a declared pack this runner could not read is kept, whether or
    /// not anything is in there (see [`REFUSED_DIR`]).
    ///
    /// **Nothing in this store ever reads below it.** It is not a second
    /// place to look for an object and not a queue: `objects_dir` and
    /// `manifests_dir` are the store, and this is the one thing in `packs/`
    /// whose only reader is a human.
    pub fn refused_dir(&self) -> PathBuf {
        self.path.join(REFUSED_DIR)
    }

    /// Where the pack one attempt declared at `relative` is kept when it
    /// could not be ingested.
    ///
    /// `attempt` is the `<job>-<attempt>` name the spool and the sweep
    /// already spell, and `relative` is the declared path under `{out}` —
    /// already through [`herd_proto::check_relative`] by the time this is
    /// called, so it is normal components only and cannot climb anywhere.
    /// Keeping the declared path inside means the kept tree reads like the
    /// `{out}` it came from, and that two artefacts one job declared at two
    /// paths cannot land on each other.
    pub fn refused_at(&self, attempt: &str, relative: &str) -> PathBuf {
        self.refused_dir().join(attempt).join(relative)
    }

    /// This pack's manifest directory, whether or not it exists.
    pub fn pack_dir(&self, id: &PackId) -> PathBuf {
        self.manifests_dir().join(id.as_str())
    }

    /// Whether this store has a registered manifest for that id.
    ///
    /// True means the manifest is there, which — because the manifest is
    /// written last — means every object was on the disk when it was written.
    pub fn holds(&self, id: &PackId) -> bool {
        self.pack_dir(id).join(MANIFEST_FILE).is_file()
    }

    /// Read a pack's manifest back out of the store.
    ///
    /// The id is **recomputed** over the bytes that were read and refused if
    /// it is not the id asked for. The store is the runner's own directory,
    /// so this is not a trust boundary; it is the one check that turns a
    /// hand-edited or half-written manifest into a refusal naming both
    /// digests instead of a pack whose name no longer describes it.
    pub fn manifest_of(&self, id: &PackId) -> Result<PackManifest, String> {
        let path = self.pack_dir(id).join(MANIFEST_FILE);
        let bytes = read_bounded(&path)?.ok_or_else(|| {
            format!(
                "this store holds no pack {id}: {} is not there",
                path.display()
            )
        })?;
        let manifest = PackManifest::parse(bytes)
            .map_err(|why| format!("{} will not read: {why}", path.display()))?;
        if let Some(disagreement) = manifest.disagreement_with(id) {
            return Err(format!("{} is not the pack it is filed under. {disagreement}. Remove that directory and the pack is absent rather than wrong", path.display()));
        }
        Ok(manifest)
    }

    /// The meta beside a stored manifest, if the pack had one.
    pub fn meta_of(&self, id: &PackId) -> Result<Option<Vec<u8>>, String> {
        read_bounded(&self.pack_dir(id).join(META_FILE))
    }

    /// Which of this pack's objects this store does **not** hold, with the
    /// size each one must be.
    ///
    /// Empty means a complete copy. The `kind f` filter lives in
    /// [`PackManifest::objects_needed`] and every completeness question in
    /// Herd goes through it: asked over every row's sha256 instead, this
    /// would report a pack with a directory or a symlink in it as incomplete
    /// for ever, because those digests are not of any file's bytes and no
    /// `objects/` entry will ever answer to them.
    pub fn missing(&self, manifest: &PackManifest) -> Result<BTreeMap<String, u64>, String> {
        let mut missing = BTreeMap::new();
        for (sha256, size) in manifest.objects_needed() {
            let name = object_name(&sha256)?;
            if !self.holds_object(name, size)? {
                missing.insert(sha256, size);
            }
        }
        Ok(missing)
    }

    /// Whether this store holds every object of that pack.
    pub fn holds_whole(&self, manifest: &PackManifest) -> Result<bool, String> {
        Ok(self.missing(manifest)?.is_empty())
    }

    /// Every pack this store holds, by id, in the order the names sort.
    ///
    /// **This is what a runner announces** ([`herd_proto::pack::PacksHeld`]),
    /// and it is deliberately the cheapest reading of the store that is still
    /// true: one `read_dir` of `manifests/` and one `stat` per entry. A
    /// directory counts when its name is a sha256 of the shape
    /// `check_object_digest` accepts and it holds a [`MANIFEST_FILE`] —
    /// which is [`PackStore::holds`]'s own rule, and the manifest is written
    /// last, so its presence is the store's statement that a complete copy
    /// landed.
    ///
    /// **The name is taken as a name and never re-hashed**, exactly as
    /// [`PackId::as_announced`] says an announced id is. Re-hashing would
    /// read every manifest on every announcement — a pack of 557 files
    /// carries on the order of 100 KiB of manifest (the consumer's issue #16,
    /// measured 2026-10-13: 401.6 MiB in 557 files), so a store of a few
    /// hundred packs would be tens of megabytes read each time a job ends.
    /// The consequence is named rather than hidden: a manifest directory
    /// whose name does not hash to its own bytes is announced as held, and
    /// [`PackStore::manifest_of`] is what refuses it by name — which every
    /// path that would serve or lay out that pack already goes through.
    ///
    /// The hex check is also what keeps a half-made ingest out of an
    /// announcement: a staging name begins with a dot and holds a `-`, so it
    /// can never be read as a 64-hex id.
    ///
    /// An unreadable `manifests/` is an **error and never an empty list**: an
    /// empty announcement is the retraction "I hold none", and it would be a
    /// false one here.
    pub fn held_packs(&self) -> Result<Vec<PackId>, String> {
        let directory = self.manifests_dir();
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "what this runner holds cannot be read: {} will not open: {error}",
                directory.display()
            )
        })?;
        let mut held = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "what this runner holds cannot be read: {} will not list: {error}",
                    directory.display()
                )
            })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if check_object_digest(name).is_err() {
                continue;
            }
            let id = PackId::as_announced(name.to_owned());
            if self.holds(&id) {
                held.push(id);
            }
        }
        held.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(held)
    }

    /// Read the whole store once and seed the running figure.
    ///
    /// **One walk, at startup, and the only one.** Everything after it is
    /// arithmetic: [`PackStore::ingest`] adds the bytes it really placed, and
    /// [`PackStore::usage`] reads what is there without opening a directory
    /// at all. That split is the point of the task this came from — a runner
    /// that re-walked `objects/` whenever it said what it holds would read
    /// the whole store on every job's end, and the consumer's own pack is
    /// 401.6 MiB in 557 files.
    ///
    /// A store that is **not there** holds nothing, and that is a
    /// measurement: this is called before the data root is validated, and a
    /// runner starting on a fresh root would otherwise print an error about
    /// a directory nothing has yet had a reason to make.
    ///
    /// Called again, it re-seeds: the absolute figure wins over the running
    /// one, which is what makes it the answer to any drift. It is **not** to
    /// be called on an announcement; nothing in this crate does.
    pub fn measure(&self) -> Result<PackUse, String> {
        let bytes = self.objects_bytes()?;
        let packs = match self.held_packs() {
            Ok(held) => u64::try_from(held.len()).unwrap_or(u64::MAX),
            Err(_) if !self.manifests_dir().exists() => 0,
            Err(why) => return Err(why),
        };
        let mut standing = self.standing();
        standing.bytes = bytes;
        standing.packs = packs;
        standing.measured = true;
        Ok(standing.reading())
    }

    /// What this store holds, as the running figure says — **with no
    /// filesystem access of any kind**.
    ///
    /// This is what an announcement, a health report and an eviction check
    /// may call as often as they like. [`PackUse::measured`] is false until
    /// something has called [`PackStore::measure`], and a caller that sends
    /// the figure onward says "did not measure" rather than nought for it.
    pub fn usage(&self) -> PackUse {
        self.standing().reading()
    }

    /// What the operator said this store may hold, in bytes, or `None` if
    /// nobody said ([`PACK_BUDGET_FLAG`]).
    pub fn budget(&self) -> Option<u64> {
        self.standing().budget
    }

    /// Remember what the operator said this store may hold.
    ///
    /// Taken once, at startup, by [`take_budget`], and read from every
    /// [`PackStore`] of that data root afterwards — which is why it is kept
    /// beside the figures rather than threaded through every call that might
    /// one day want it.
    pub fn set_budget(&self, budget: Option<u64>) {
        self.standing().budget = budget;
    }

    /// How many times `objects/` has been listed for this data root in this
    /// process.
    ///
    /// Exposed for one assertion: that saying what this runner holds costs no
    /// listing of that directory. See [`Standing::objects_listed`] for why it
    /// is a counter and not a stopwatch.
    pub fn objects_listed(&self) -> u64 {
        self.standing().objects_listed
    }

    /// Every object's bytes: one `read_dir` of `objects/` and one `stat` per
    /// entry.
    ///
    /// **The only listing of `objects/` in this crate's product code**, which
    /// is what makes [`PackStore::objects_listed`] a statement about the
    /// whole crate and not about this function. It counts a name
    /// [`check_object_digest`] accepts and nothing else, so a staging file a
    /// kill left behind weighs nought here exactly as it is announced as
    /// nothing: one rule for both halves of the same question.
    fn objects_bytes(&self) -> Result<u64, String> {
        let directory = self.objects_dir();
        self.standing().objects_listed += 1;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => {
                return Err(format!(
                    "what this runner's pack store holds cannot be measured: {} will not \
                     open: {error}",
                    directory.display()
                ))
            }
        };
        let mut bytes = 0u64;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "what this runner's pack store holds cannot be measured: {} will not \
                     list: {error}",
                    directory.display()
                )
            })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if check_object_digest(name).is_err() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        Ok(bytes)
    }

    /// Put a pack directory into the store.
    ///
    /// `dir` is a pack directory in the three-part shape above: it holds
    /// `manifest.tsv`, optionally `pack.meta`, and an `objects/` directory
    /// with one file per `kind f` row. Anything else is refused by name
    /// rather than guessed at — in particular a directory holding a single
    /// subdirectory that is itself a pack, which is a shape nobody declared
    /// and which Herd would have to invent a rule to recognise.
    ///
    /// **Checked, and not merely stated.** The directory's own names are read
    /// before anything is placed, so a job that leaves a build log beside its
    /// pack is refused by name and [`leave_packs`] then keeps the whole
    /// directory — which is the point of refusing. Taking the three parts out
    /// and removing what was left would be Herd deciding, unasked, that a
    /// declared directory is part pack and part output; which of those bytes
    /// are which is the declaration's to say, and the declaration says the
    /// directory is a pack.
    ///
    /// **Idempotent.** A pack the store already holds whole costs one read of
    /// the manifest and nothing else; [`Held::objects_added`] is then nought,
    /// which is the dedup as a figure rather than as a claim.
    ///
    /// **The manifest is written last**, so an ingest that dies partway
    /// leaves objects and no pack (E-17): what is on the disk is either a
    /// whole object under its own name or nothing at all, and the pack is
    /// absent until all of it is there.
    pub fn ingest(&self, dir: &Path) -> Result<Held, String> {
        let manifest_path = dir.join(MANIFEST_FILE);
        let bytes = read_bounded(&manifest_path)?.ok_or_else(|| {
            format!(
                "{} is not a pack directory: it holds no {MANIFEST_FILE}, and a pack's identity \
                 is that file's bytes",
                dir.display()
            )
        })?;
        // The directory's shape before its content, and before any object is
        // placed: what is refused here is kept whole by the caller, and what
        // gets past here has its remainder removed with it.
        nothing_beyond_a_pack(dir)?;
        let manifest = PackManifest::parse(bytes)
            .map_err(|why| format!("{} will not read: {why}", manifest_path.display()))?;
        let meta = read_bounded(&dir.join(META_FILE))?;

        let needed = manifest.objects_needed();
        let objects = u32::try_from(needed.len()).unwrap_or(u32::MAX);
        let mut held = Held {
            manifest,
            meta,
            objects,
            objects_added: 0,
            bytes_linked: 0,
            bytes_copied: 0,
        };
        let objects_dir = dir.join(OBJECTS_DIR);
        for (sha256, size) in &needed {
            let name = object_name(sha256)?;
            if self.holds_object(name, *size)? {
                continue;
            }
            let (carriage, bytes) = self.place_object(name, *size, &objects_dir.join(name))?;
            held.objects_added += 1;
            match carriage {
                Carriage::Linked => held.bytes_linked += bytes,
                Carriage::Copied => held.bytes_copied += bytes,
            }
        }
        let filed = self.write_manifest(&held.manifest, held.meta.as_deref())?;
        // The running figure moves by what this ingest really placed and by
        // nothing else: an object the store already held was not placed and
        // weighs nothing here, and a pack already filed under its id is not a
        // second pack. Which of those happened is `write_manifest`'s answer
        // rather than a second question to the disk — two concurrent ingests
        // of one new pack would both have seen "not there", and only the one
        // whose rename landed says it filed it.
        let mut standing = self.standing();
        standing.bytes = standing
            .bytes
            .saturating_add(held.bytes_linked)
            .saturating_add(held.bytes_copied);
        if filed {
            standing.packs = standing.packs.saturating_add(1);
        }
        drop(standing);
        Ok(held)
    }

    /// Lay a pack out for a job, at `at`, in the same three-part shape it was
    /// ingested in.
    ///
    /// **Hard links, so this costs no bytes at all**: the job's `{in}` and
    /// the store are both under the data root and therefore on one
    /// filesystem, and [`LaidOut::bytes_copied`] is nought whenever that
    /// holds. A copy is the fallback and never silent — the figure says which
    /// happened, and nothing has to read this function to find out.
    ///
    /// Nothing here is fsynced, deliberately: `{in}` is made for one attempt
    /// of one job and is reclaimed with its workspace, so durability across a
    /// power cut would be paying for a promise nobody asked for.
    ///
    /// Refused, by name and with figures, if the store does not hold the
    /// whole pack: half a pack at `{in}` would let the consumer's own tool
    /// lay out a tree with files missing, which is the failure a pack exists
    /// to make impossible.
    pub fn lay_out(&self, manifest: &PackManifest, at: &Path) -> Result<LaidOut, String> {
        let needed = manifest.objects_needed();
        let missing = self.missing(manifest)?;
        if !missing.is_empty() {
            let short: u64 = missing.values().sum();
            return Err(format!(
                "this store holds {} of pack {}'s {} object(s) and is short {} ({}); a pack is \
                 laid out whole or not at all, because a tree with files missing is what a pack \
                 exists to make impossible",
                // Two lengths from two walks of one manifest. They agree in
                // every path through this module, and a refusal is the last
                // place that should die on a subtract if they ever do not.
                needed.len().saturating_sub(missing.len()),
                manifest.id(),
                needed.len(),
                missing.len(),
                measured_bytes(short)
            ));
        }
        let objects_at = at.join(OBJECTS_DIR);
        fs::create_dir_all(&objects_at)
            .map_err(|error| format!("{} cannot be made: {error}", objects_at.display()))?;
        // Verbatim, from the manifest in hand. Nothing in Herd prints a
        // manifest, so what lands here is the bytes the id was computed over
        // and a job reading it reads the pack it was promised.
        let manifest_at = at.join(MANIFEST_FILE);
        fs::write(&manifest_at, manifest.bytes())
            .map_err(|error| format!("{} cannot be written: {error}", manifest_at.display()))?;
        if let Some(meta) = self.meta_of(manifest.id())? {
            let meta_at = at.join(META_FILE);
            fs::write(&meta_at, &meta)
                .map_err(|error| format!("{} cannot be written: {error}", meta_at.display()))?;
        }
        let mut laid = LaidOut {
            objects: 0,
            bytes_linked: 0,
            bytes_copied: 0,
        };
        for (sha256, size) in needed {
            let name = object_name(&sha256)?;
            let from = self.objects_dir().join(name);
            let to = objects_at.join(name);
            // A name already there is a previous lay-out of the same pack
            // into the same attempt's `{in}`, which the runner owns and made.
            // Taken away first, because a link onto an existing name fails
            // and the copy below would then silently make a second copy of
            // bytes the store already holds.
            let _ = fs::remove_file(&to);
            match link_or_copy(&from, &to) {
                Ok(Carriage::Linked) => laid.bytes_linked += size,
                Ok(Carriage::Copied) => laid.bytes_copied += size,
                Err(error) => {
                    return Err(format!(
                        "{} cannot be laid out at {}: {error}",
                        from.display(),
                        to.display()
                    ))
                }
            }
            laid.objects += 1;
        }
        Ok(laid)
    }

    /// Whether `objects/<name>` is there at the size the manifest declares.
    ///
    /// A file there at a **different** size is refused rather than replaced:
    /// two different byte strings do not share a sha256, so one of the two
    /// figures is wrong, and overwriting would let a bad manifest destroy a
    /// good object. The refusal names the path, because removing that one
    /// file is what lets the next attempt place it again.
    fn holds_object(&self, name: &str, size: u64) -> Result<bool, String> {
        let path = self.objects_dir().join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && metadata.len() == size => Ok(true),
            Ok(metadata) if metadata.is_file() => Err(format!(
                "{} is {} and the manifest says it is {}; two different byte strings do not \
                 share a sha256, so one of those figures is wrong and this object is not \
                 replaced on the strength of a manifest. Remove that file and the next ingest \
                 places it again",
                path.display(),
                measured_bytes(metadata.len()),
                measured_bytes(size)
            )),
            Ok(_) => Err(format!(
                "{} is in the store's objects directory and is not a regular file, so nothing \
                 can be read through it",
                path.display()
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("{} cannot be looked at: {error}", path.display())),
        }
    }

    /// Place one object, under a staging name, and only then under its own.
    ///
    /// The order is the whole of rule 3: link or copy into
    /// `objects/.ingest-…`, hash **the staged file** — the only name those
    /// bytes have inside this store, so nothing that reads or writes the
    /// store can change them between the hash and the rename — refuse if the
    /// bytes are not the bytes the name promises, fsync, and rename.
    ///
    /// **It is not the only name they have anywhere.** On the link path the
    /// staged file is the same inode as the job's own file under `{out}`, so
    /// the job has a name for it too; what makes the hash trustworthy is that
    /// the job's processes are gone before [`leave_packs`] runs, and not the
    /// staging name. A later caller that ingested a directory something could
    /// still be writing to would have to copy, or hash after the rename, and
    /// this sentence is what tells it so.
    ///
    /// A kill at any point before the rename leaves a staging file
    /// that no lookup will ever ask for and that the [`Staged`] guard removes
    /// on the way out of this function; a kill after it leaves a whole
    /// object. There is no instant at which `objects/<sha>` holds part of
    /// something.
    fn place_object(
        &self,
        name: &str,
        size: u64,
        source: &Path,
    ) -> Result<(Carriage, u64), String> {
        match fs::symlink_metadata(source) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(metadata) if metadata.is_symlink() => {
                return Err(format!(
                    "{} is a symlink, and an object is never read through one: a link is how a \
                     pack would name bytes the job never wrote",
                    source.display()
                ))
            }
            Ok(_) => {
                return Err(format!(
                    "{} is not a regular file and holds no object's bytes; it is not opened, \
                     because a fifo nobody is writing would block this read for ever",
                    source.display()
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(format!(
                    "this pack's manifest names object {name} and {} is not there, so the pack \
                     in hand is already incomplete and is not registered as whole",
                    source.display()
                ))
            }
            Err(error) => return Err(format!("{} cannot be looked at: {error}", source.display())),
        }
        let staged = Staged::at(staging_path(&self.objects_dir()));
        let carriage = link_or_copy(source, staged.path()).map_err(|error| {
            format!(
                "{} cannot be staged at {}: {error}",
                source.display(),
                staged.path().display()
            )
        })?;
        let (read, digest) = crate::outputs::hash_file(staged.path())
            .map_err(|error| format!("{} cannot be read back: {error}", staged.path().display()))?;
        if read != size || digest != name {
            return Err(format!(
                "{} was going to be stored as object {name} at {}, and it is {} whose sha256 is \
                 {digest}. A content-addressed store whose object does not hash to its own name \
                 is a lie the next machine cannot detect, because it asks for the name it \
                 trusts — so nothing was stored",
                source.display(),
                measured_bytes(size),
                measured_bytes(read)
            ));
        }
        fs::File::open(staged.path())
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("{} cannot be fsynced: {error}", staged.path().display()))?;
        let final_path = self.objects_dir().join(name);
        fs::rename(staged.path(), &final_path).map_err(|error| {
            format!(
                "{} cannot be renamed to {}: {error}",
                staged.path().display(),
                final_path.display()
            )
        })?;
        staged.kept();
        fsync_dir(&self.objects_dir())?;
        Ok((carriage, read))
    }

    /// Write the manifest, and the meta beside it, as the **last** act of an
    /// ingest.
    ///
    /// A directory renamed into place, so a reader sees a pack with its
    /// manifest or no pack at all. A pack already filed under that id is left
    /// exactly as it is: the id is the sha256 of these bytes, so what is
    /// there is byte for byte what would be written.
    ///
    /// **True means this call is the one that filed it**, which is what the
    /// running pack count is moved by. Both of the ways this returns without
    /// writing — the pack was already there, or another ingest of the same
    /// pack won the rename — answer false, so one pack cannot be counted
    /// twice however many ingests of it run at once.
    fn write_manifest(&self, manifest: &PackManifest, meta: Option<&[u8]>) -> Result<bool, String> {
        let final_dir = self.pack_dir(manifest.id());
        if final_dir.join(MANIFEST_FILE).is_file() {
            return Ok(false);
        }
        let staged = Staged::dir(staging_path(&self.manifests_dir()));
        fs::create_dir_all(staged.path())
            .map_err(|error| format!("{} cannot be made: {error}", staged.path().display()))?;
        // The bytes that were read, never bytes assembled from the parse: a
        // reprinted manifest is a different pack, and this is the one place
        // in the runner that could have got that wrong.
        write_fsynced(&staged.path().join(MANIFEST_FILE), manifest.bytes())?;
        if let Some(meta) = meta {
            write_fsynced(&staged.path().join(META_FILE), meta)?;
        }
        fsync_dir(staged.path())?;
        match fs::rename(staged.path(), &final_dir) {
            Ok(()) => {}
            // Another ingest of the same pack finished between the check
            // above and this rename, and a rename onto a non-empty directory
            // fails. Its bytes are these bytes — the id says so — so the pack
            // is there and this is not a failure. Decided by what is on the
            // disk rather than by an errno, because which errno a kernel
            // gives for that is not the fact being asked about.
            Err(error) => {
                if final_dir.join(MANIFEST_FILE).is_file() {
                    return Ok(false);
                }
                return Err(format!(
                    "{} cannot be renamed to {}: {error}",
                    staged.path().display(),
                    final_dir.display()
                ));
            }
        }
        staged.kept();
        fsync_dir(&self.manifests_dir())?;
        Ok(true)
    }
}

/// Take every pack a job declared out of `{out}`, **before** anything reads
/// that directory for the record.
///
/// This is ORCH-153, and the shape is deliberate: the pack is *gone*, not
/// excluded. An exclusion list in the collection walk would be a fifth rule
/// beside four existing ones, and the failure it guards against is silent —
/// a ~400 MiB pack collected as a job's output evidence and streamed runner →
/// hub → requester as if it were a result, which is the record half doing
/// exactly its job on bytes that are not a record's business. An exclusion is
/// a thing a later edit quietly breaks; an absence is not.
///
/// `leaves` is the requester's declaration: a key, and the path under `{out}`
/// the pack is left at (`leaves = { pack = "packs" }`). Keys are taken in
/// order, so two runs of one job report in one order.
///
/// **A declared pack path leaves `{out}` whether or not it could be stored,
/// and in neither case is it destroyed.** Why it leaves is the record: a pack
/// Herd could not ingest, left in place, is 400 MiB of ordinary output over
/// the job's output cap, and the whole collection — `results.json` included —
/// would be refused for it, which is one failure becoming two with the second
/// one silent. A declared pack is outside the record by declaration, so it is
/// outside the record in both outcomes.
///
/// Where it goes is [`REFUSED_DIR`]: **set aside and kept**, with the reason
/// and the figures on the job's own report and on the machine's stderr, and
/// nothing in this runner ever removes it. Herd does not destroy an
/// operator's bytes — the sweep keeps a spool no hub will claim, the hub
/// keeps a record it cannot read, and a fourth path that quietly deleted
/// hundreds of megabytes would be the only place in Herd that does. How long
/// it is kept is not a question this has an answer to: indefinitely, and
/// removing it is the operator's.
///
/// `attempt` is the `<job>-<attempt>` name that keeps one job's refused packs
/// apart from another's.
pub(crate) fn leave_packs(
    out_dir: &Path,
    data_root: &Path,
    attempt: &str,
    leaves: &BTreeMap<String, String>,
) -> (Vec<LeftPack>, Vec<String>) {
    let mut left = Vec::new();
    let mut said = Vec::new();
    if leaves.is_empty() {
        return (left, said);
    }
    let store = match PackStore::open(data_root) {
        Ok(store) => store,
        Err(why) => {
            // Nothing is removed: with no store there is nowhere for the
            // bytes to go, and taking them away would destroy them for a
            // reason that is the runner's own and not the job's.
            said.push(format!(
                "this job's declared artefact(s) {} are still under its output directory and are \
                 not in a pack store, because this runner has none: {why}",
                keys_said(leaves)
            ));
            return (left, said);
        }
    };
    for (key, relative) in leaves {
        if let Err(why) = herd_proto::check_relative(relative) {
            said.push(format!(
                "this job declared that it leaves {key:?} at {relative:?}, which is not a path \
                 under an output directory: {why}"
            ));
            continue;
        }
        let at = out_dir.join(relative);
        match fs::symlink_metadata(&at) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(metadata) if metadata.is_symlink() => {
                said.push(format!(
                    "this job declared that it leaves {key:?} at {relative:?}, and that is a \
                     symlink; a link is never followed, because a link out of the workspace is \
                     how a pack would be made of bytes this job never wrote"
                ));
                continue;
            }
            Ok(_) => {
                said.push(format!(
                    "this job declared that it leaves {key:?} at {relative:?}, and that is not a \
                     directory; a pack is a directory holding {MANIFEST_FILE}, {META_FILE} and \
                     {OBJECTS_DIR}/"
                ));
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // The job declared it and did not leave it. Said, never
                // silent (D23): a missing artefact is a fact about the job.
                said.push(format!(
                    "this job declared that it leaves {key:?} at {relative:?} and left nothing \
                     there, so there is no pack of it to keep"
                ));
                continue;
            }
            Err(error) => {
                said.push(format!(
                    "this job's declared artefact {key:?} at {relative:?} could not be looked \
                     at: {error}"
                ));
                continue;
            }
        }
        match store.ingest(&at) {
            Ok(held) => {
                let id = held.manifest.id().clone();
                left.push(LeftPack {
                    key: key.clone(),
                    at: relative.clone(),
                    held,
                });
                if let Err(error) = fs::remove_dir_all(&at) {
                    // In the store and still under `{out}`: the bytes are
                    // safe and the record is about to carry them anyway,
                    // which is the one outcome ORCH-153 exists to prevent.
                    // Named with the pack's id, because the record that
                    // follows will have a pack in it and somebody will ask
                    // why.
                    said.push(format!(
                        "this job's artefact {key:?} is in this runner's pack store as {id}, and \
                         {relative:?} could NOT be taken out of its output directory: {error}. \
                         The record below therefore carries the pack's own files, which is not \
                         what a record is for"
                    ));
                }
            }
            Err(why) => {
                said.push(format!(
                    "this job declared that it leaves {key:?} at {relative:?} and that is not a \
                     pack this runner can keep: {why}. {}",
                    // Measured in the one arm whose sentence carries the
                    // figure, and still before anything moves, because
                    // arguments are evaluated before the call: walking a
                    // 557-file pack on the success path is a walk for
                    // nothing.
                    set_aside(&store, attempt, relative, &at, apparent_bytes(&at))
                ));
            }
        }
    }
    said.sort();
    (left, said)
}

/// Move a declared pack this runner could not read out of `{out}` and
/// **keep** it, and say so in the register [`crate::sweep::set_aside`]
/// already uses for a spool no hub will ever claim.
///
/// One `rename` inside the data root, so ~400 MiB costs a directory entry and
/// not a copy — the same reason the spool lives under the data root. Nothing
/// is removed on any path through here, including the path where the rename
/// fails: a pack that cannot be moved aside stays exactly where the job left
/// it, and the sentence says that too, because the record below will then
/// carry the pack's own files and somebody will ask why.
///
/// `weight` is measured before the move, which is the only time it can be.
fn set_aside(store: &PackStore, attempt: &str, relative: &str, at: &Path, weight: u64) -> String {
    let kept = store.refused_at(attempt, relative);
    let lost = |error: String| {
        format!(
            "Its {} could NOT be moved aside ({error}), so they are still in {} and the record \
             below therefore carries the pack's own files, which is not what a record is for",
            measured_bytes(weight),
            at.display()
        )
    };
    if let Some(parent) = kept.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return lost(format!("{} cannot be created: {error}", parent.display()));
        }
    }
    // Never onto something already there: a rename onto a non-empty directory
    // fails, which is the outcome wanted — the bytes of whatever is under
    // that name are not this attempt's to replace.
    match fs::rename(at, &kept) {
        Ok(()) => format!(
            "Its {} are kept as {}: a declared artefact is never collected as the job's output, \
             so it is out of the output directory — left where it is, one pack this runner \
             cannot read would put the whole record over its output cap — and it is kept there \
             for as long as it is on this disk. Nothing in this runner reads it, offers it or \
             gives it to a job again, nothing here will ever remove it, and removing it is \
             yours to do",
            measured_bytes(weight),
            kept.display()
        ),
        Err(error) => lost(error.to_string()),
    }
}

/// Refuse a declared pack directory that holds anything a pack does not,
/// naming what was found.
///
/// [`PackStore::ingest`]'s own rule, enforced rather than stated. What hangs
/// on it is not tidiness: a declared directory that ingests has
/// [`leave_packs`] remove **all** of it, so a job that left
/// `{out}/packs/build.log` beside its pack had that log destroyed on the
/// success path — by the module that keeps the pack on the failure path.
/// Refused, the whole directory goes to [`REFUSED_DIR`] intact instead, which
/// is what every other place Herd meets bytes it cannot account for already
/// does.
///
/// Only the names directly under `dir` are read. What is inside the three
/// parts is their own business, and a pack whose `objects/` carries a file no
/// `kind f` row names is still the pack its manifest says it is.
fn nothing_beyond_a_pack(dir: &Path) -> Result<(), String> {
    let listed =
        fs::read_dir(dir).map_err(|error| format!("{} cannot be read: {error}", dir.display()))?;
    let mut strays = Vec::new();
    for entry in listed {
        let entry = entry.map_err(|error| format!("{} cannot be read: {error}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == MANIFEST_FILE || name == META_FILE || name == OBJECTS_DIR {
            continue;
        }
        strays.push(name);
    }
    if strays.is_empty() {
        return Ok(());
    }
    // Sorted, so one directory reads the same twice, and cut short, so a
    // directory of a thousand names is a sentence and not a wall.
    strays.sort();
    let mut named = strays
        .iter()
        .take(STRAYS_NAMED)
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let rest = strays.len().saturating_sub(STRAYS_NAMED);
    if rest > 0 {
        named.push_str(&format!(" and {rest} more name(s)"));
    }
    Err(format!(
        "{} holds {named}, which a pack directory does not have, and a pack directory is \
         {MANIFEST_FILE} plus optionally {META_FILE} plus {OBJECTS_DIR}/ and nothing else. The \
         whole of it is kept as it is rather than partly taken apart: which of those bytes are \
         a pack's and which are the job's own output is the declaration's to say, and taking \
         the three parts out of here would have removed the rest of it with them",
        dir.display()
    ))
}

/// How many stray names one refusal spells before it counts the rest.
const STRAYS_NAMED: usize = 3;

/// The declared keys, for a sentence about all of them at once.
fn keys_said(leaves: &BTreeMap<String, String>) -> String {
    leaves
        .iter()
        .map(|(key, relative)| format!("{key:?} at {relative:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The apparent size of a directory tree, for a sentence that has to say what
/// was lost. Never followed through a symlink, and silent about what it
/// cannot read: it is a figure in a report and a gate on nothing.
fn apparent_bytes(at: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![at.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    total
}

/// Which way an object's bytes got where they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carriage {
    /// A second name for the same inode: no bytes moved and none were
    /// allocated.
    Linked,
    /// A second copy on the disk, which is what a pack store exists to avoid
    /// and what it falls back to across a filesystem boundary.
    Copied,
}

/// A hard link, or a copy when the filesystem will not have one.
///
/// Both ends are under the data root in every path Herd takes, so the link
/// succeeds and the copy is the honest fallback for a data root somebody
/// assembled out of two mounts. The caller reports which happened as a
/// figure, so nobody has to read this function to know what a pack cost.
fn link_or_copy(from: &Path, to: &Path) -> io::Result<Carriage> {
    match fs::hard_link(from, to) {
        Ok(()) => Ok(Carriage::Linked),
        Err(_) => {
            fs::copy(from, to)?;
            Ok(Carriage::Copied)
        }
    }
}

/// `objects/<name>` as a path component, or why that column is not one.
///
/// **One rule, one wording, one place it can change.** The rule is
/// [`check_object_digest`] — exactly 64 lowercase hex characters — which
/// `herd-proto` applies at the one place a `ManifestEntry` is built and
/// exposes `pub` for exactly this: a reader that joins the digest column onto
/// a path delegates instead of keeping a second copy of the rule. This
/// function is that delegation and nothing more.
///
/// It stays rather than being deleted because this module is the one that
/// performs the join, so a digest reaching it by some later path with no
/// parse behind it is still refused here rather than joined. What it must
/// never again be is a *second statement* of the rule.
///
/// **Why that matters is measured and not argued.** Until 2026-10-30 this was
/// a hand-rolled copy, and the two copies had already drifted: `herd-proto`
/// spelled the rule "64 lowercase hex digits" and this module spelled it
/// "sixty-four lowercase hex". Both branches were green alone, and their
/// merge was red on a test asserting one of the two spellings. The
/// duplication, not either wording, was the defect.
fn object_name(sha256: &str) -> Result<&str, String> {
    check_object_digest(sha256)?;
    Ok(sha256)
}

/// Cut what a runner holds into the frames that announce it.
///
/// **One frame is always returned, and an empty list is not nothing.** A
/// runner that holds no pack announces one frame with an empty `packs` and
/// `more: false`, which is the frame set's only retraction: the hub replaces
/// its whole view of this runner with that emptiness and forgets every
/// holding it had. No announcement at all says nothing, and the two must not
/// be confused — a runner that has dropped everything it held and said
/// nothing leaves a hub offering holders that are not there.
///
/// Every frame but the last carries `more: true`, so the hub accumulates and
/// applies nothing until the last one arrives. The chunking is cut at
/// [`PACKS_HELD_PER_FRAME`], whose doc carries the measurement it is cut
/// against.
pub fn announcement_frames(held: Vec<PackId>) -> Vec<PacksHeld> {
    if held.is_empty() {
        return vec![PacksHeld {
            packs: Vec::new(),
            more: false,
        }];
    }
    let mut frames: Vec<PacksHeld> = held
        .chunks(PACKS_HELD_PER_FRAME)
        .map(|chunk| PacksHeld {
            packs: chunk.to_vec(),
            more: true,
        })
        .collect();
    // The last frame is what applies the announcement, and there is always
    // one: `chunks` over a non-empty slice yields at least one chunk.
    if let Some(last) = frames.last_mut() {
        last.more = false;
    }
    frames
}

/// A unique name under `directory` for something being made.
fn staging_path(directory: &Path) -> PathBuf {
    let unique = NEXT_STAGING.fetch_add(1, Ordering::SeqCst);
    directory.join(format!("{STAGING_PREFIX}{}-{unique}", std::process::id()))
}

/// Something half-made, removed unless it was renamed into place.
///
/// The reason the store's promise holds through a failure in the middle of
/// an ingest rather than only through a clean one: every early return from
/// [`PackStore::place_object`] and [`PackStore::write_manifest`] passes
/// through this `Drop`, so the half-made thing goes whether the function
/// succeeded, refused or panicked.
struct Staged {
    path: PathBuf,
    directory: bool,
    keep: bool,
}

impl Staged {
    fn at(path: PathBuf) -> Self {
        Self {
            path,
            directory: false,
            keep: false,
        }
    }

    fn dir(path: PathBuf) -> Self {
        Self {
            path,
            directory: true,
            keep: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// It has been renamed into place: there is nothing at this name any
    /// more, and nothing to remove.
    fn kept(mut self) {
        self.keep = true;
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let _ = if self.directory {
            fs::remove_dir_all(&self.path)
        } else {
            fs::remove_file(&self.path)
        };
    }
}

/// Read a file whose whole content has to fit in memory, or `None` if it is
/// not there.
///
/// Bounded by [`MANIFEST_MAX_BYTES`], which is the bound a manifest has to
/// obey to ride a frame at all, and the same bound is applied to `pack.meta`
/// for the same reason: both travel on the control plane and neither is ever
/// truncated, because a truncated manifest still parses and would register a
/// pack that is missing files nobody can see.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} cannot be looked at: {error}", path.display())),
    };
    if !metadata.is_file() {
        return Err(format!(
            "{} is not a regular file, and a pack's manifest and meta are files",
            path.display()
        ));
    }
    if metadata.len() > MANIFEST_MAX_BYTES {
        return Err(format!(
            "{} is {} and Herd carries at most {} on a frame; it is refused whole rather than \
             truncated",
            path.display(),
            measured_bytes(metadata.len()),
            measured_bytes(MANIFEST_MAX_BYTES)
        ));
    }
    fs::read(path)
        .map(Some)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))
}

/// Write bytes and get them onto the disk before anything is renamed over
/// them.
fn write_fsynced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes)
        .and_then(|()| fs::File::open(path).and_then(|file| file.sync_all()))
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))
}

/// A rename is on the disk only once its directory is.
fn fsync_dir(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("{} cannot be fsynced: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use std::os::unix::fs::MetadataExt;

    /// The sha of a row that is **not** a file's bytes: a `kind d` row's
    /// digest, valid hex and absent from `objects/` for ever.
    fn dir_sha() -> String {
        "2".repeat(64)
    }

    /// The same for a `kind l` row.
    fn link_sha() -> String {
        "3".repeat(64)
    }

    /// Write a pack directory in the consumer's own three-part shape.
    ///
    /// **Four rows, and two of them are not files.** A fixture of `kind f`
    /// rows alone would let every mistake in this module pass: the `d` row
    /// and the `l` row carry a sha256 of something that is not a file's
    /// bytes, so they are what proves the completeness filter rather than
    /// assuming it (and the module's doc, rule 2, says why).
    ///
    /// Returns the manifest's bytes, so a test can compare what the store
    /// wrote against what it read and see that nothing was reprinted.
    fn write_pack(at: &Path, files: &[(&str, &[u8])], meta: Option<&[u8]>) -> Vec<u8> {
        fs::create_dir_all(at.join(OBJECTS_DIR)).expect("a pack directory");
        let mut text = format!("{}\n# profile debug\n", herd_proto::CHATONS_PACK_FORMAT);
        for (path, content) in files {
            let sha = herd_proto::sha256_hex(content);
            text.push_str(&format!(
                "{sha}\t{}\t0644\tf\tartefact\t{path}\n",
                content.len()
            ));
            fs::write(at.join(OBJECTS_DIR).join(&sha), content).expect("an object");
        }
        // The two rows that are not files, with the paths the first
        // consumer's own manifest has: a directory and a symlink beside it.
        text.push_str(&format!("{}\t0\t0755\td\tartefact\tbin\n", dir_sha()));
        text.push_str(&format!(
            "{}\t0\t0777\tl\tartefact\tbin/tool-latest\tbin/tool\n",
            link_sha()
        ));
        let bytes = text.into_bytes();
        fs::write(at.join(MANIFEST_FILE), &bytes).expect("a manifest");
        if let Some(meta) = meta {
            fs::write(at.join(META_FILE), meta).expect("a meta");
        }
        bytes
    }

    /// `stat`, for the assertions that are measurements and not readings of
    /// the code.
    fn stat(path: &Path) -> fs::Metadata {
        fs::symlink_metadata(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// Every name directly under a directory, sorted.
    fn names_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("the directory is readable")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Ingested twice, stored once — **measured on the inode**, not on a byte
    /// count.
    ///
    /// Two pack directories hold the same bytes at two paths, as two builds
    /// of two commits that differ in one file do. The second ingest places
    /// nothing: `objects_added` is nought, the store's object is still the
    /// inode the *first* pack's file was linked to, its link count has not
    /// moved, and the second pack's own file is a different inode entirely —
    /// so nothing was copied and nothing was replaced.
    ///
    /// A byte count would have passed with two copies on the disk, which is
    /// the whole thing a content-addressed store exists not to do.
    #[test]
    fn the_same_content_ingested_twice_is_one_object() {
        let temp = TempDir::new("pack-dedup");
        let store = PackStore::open(temp.path()).expect("a store");
        let shared: &[u8] = b"the bytes both packs hold";
        let sha = herd_proto::sha256_hex(shared);

        let first = temp.path().join("first");
        write_pack(&first, &[("bin/tool", shared)], None);
        let second = temp.path().join("second");
        write_pack(
            &second,
            &[("lib/tool.so", shared), ("only/here", b"second only")],
            None,
        );

        let a = store.ingest(&first).expect("the first pack is ingested");
        assert_eq!(a.objects, 1, "one `kind f` row");
        assert_eq!(a.objects_added, 1);
        assert_eq!(a.bytes_copied, 0, "both ends are on one filesystem");

        let stored = store.objects_dir().join(&sha);
        assert_eq!(
            stat(&stored).ino(),
            stat(&first.join(OBJECTS_DIR).join(&sha)).ino(),
            "the store's object is the pack's own inode, linked rather than copied"
        );
        assert_eq!(
            stat(&stored).nlink(),
            2,
            "two names for one inode: the pack's and the store's"
        );

        let b = store.ingest(&second).expect("the second pack is ingested");
        assert_eq!(b.objects, 2);
        assert_eq!(
            b.objects_added, 1,
            "only the object the store did not already hold was placed"
        );
        assert_eq!(
            stat(&stored).nlink(),
            2,
            "the second ingest placed nothing, so no third name appeared"
        );
        assert_ne!(
            stat(&stored).ino(),
            stat(&second.join(OBJECTS_DIR).join(&sha)).ino(),
            "the second pack's file is a different inode: it was not linked, because the \
             store already held those bytes"
        );
        assert_eq!(
            names_in(&store.objects_dir()).len(),
            2,
            "three `kind f` rows across two packs, two distinct contents, two objects: {:?}",
            names_in(&store.objects_dir())
        );
    }

    /// An ingest that does not finish leaves **no readable object** and no
    /// pack.
    ///
    /// Three arms, because "interrupted" has three shapes that reach the
    /// disk, and the one that matters is the first: a partial object visible
    /// under its final name would be counted by a later completeness check,
    /// and a pack missing those bytes would read as whole.
    ///
    /// **What is not done here, said plainly:** no process is killed in the
    /// middle of a write. A kill at a chosen instant cannot be staged
    /// deterministically from a unit test, and a timing-based one is a flake
    /// — which erodes invariant 5 rather than proving anything. What is
    /// proved instead is the property that makes a kill safe: bytes that are
    /// not the bytes the name promises never appear under that name, a failed
    /// ingest files no manifest, and a staging file left behind by a kill is
    /// not counted by anything.
    #[test]
    fn an_interrupted_ingest_leaves_no_readable_object_and_no_pack() {
        let temp = TempDir::new("pack-interrupted");
        let store = PackStore::open(temp.path()).expect("a store");

        // Arm one: the bytes under a name are not the bytes that name
        // promises — which is exactly what a transfer stopped halfway would
        // leave in a source directory.
        let wrong = temp.path().join("wrong");
        write_pack(&wrong, &[("bin/tool", b"the whole of it")], None);
        let sha = herd_proto::sha256_hex(b"the whole of it");
        fs::write(wrong.join(OBJECTS_DIR).join(&sha), b"the whole").expect("a short object");
        let why = store.ingest(&wrong).expect_err("a short object is refused");
        assert!(
            why.contains(&sha) && why.contains("does not hash to its own name"),
            "the refusal names the object and why it is one: {why}"
        );
        assert_eq!(
            names_in(&store.objects_dir()),
            Vec::<String>::new(),
            "nothing is in the store: not under the final name, and not under a staging \
             name either"
        );
        assert!(
            !store.holds(&PackId::of_manifest_bytes(
                &fs::read(wrong.join(MANIFEST_FILE)).expect("the manifest")
            )),
            "and no pack was filed"
        );

        // Arm two: an ingest that gets halfway. The first object is placed
        // and the second is not there at all, so the ingest refuses with
        // objects already on the disk — and the pack is **absent**, which is
        // E-17's complete-or-absent.
        let half = temp.path().join("half");
        let manifest = write_pack(
            &half,
            &[("a", b"the first object"), ("b", b"the second object")],
            None,
        );
        // Objects are placed in sha256 order and not in row order, because
        // that is the order `objects_needed()` hands them over in. So the
        // object that is taken away is chosen by its digest: the *last* one,
        // which makes "one object was placed and then the ingest refused"
        // the outcome on every machine rather than on half of them.
        let mut shas = [
            herd_proto::sha256_hex(b"the first object"),
            herd_proto::sha256_hex(b"the second object"),
        ];
        shas.sort();
        let [placed, absent] = shas;
        let (whole, gone) = if absent == herd_proto::sha256_hex(b"the second object") {
            (b"the first object".to_vec(), b"the second object".to_vec())
        } else {
            (b"the second object".to_vec(), b"the first object".to_vec())
        };
        fs::remove_file(half.join(OBJECTS_DIR).join(&absent)).expect("one object goes");
        let why = store
            .ingest(&half)
            .expect_err("a pack missing an object is refused");
        assert!(
            why.contains(&absent) && why.contains("already incomplete"),
            "the refusal names the missing object: {why}"
        );
        let id = PackId::of_manifest_bytes(&manifest);
        assert!(
            !store.holds(&id),
            "the manifest is written last, so a half-done ingest files no pack"
        );
        assert_eq!(
            names_in(&store.objects_dir()),
            vec![placed.clone()],
            "what did get placed is one whole object under its own name, which is harmless: \
             it is the pack that is absent, not the bytes that are half there"
        );
        assert_eq!(
            fs::read(store.objects_dir().join(&placed)).expect("the placed object"),
            whole,
            "and it is whole"
        );

        // Arm three: what a kill really leaves on the disk — a staging file
        // with part of an object in it. Nothing counts it, because nothing
        // ever looks a staging name up.
        let stray = store.objects_dir().join(format!("{STAGING_PREFIX}12345-7"));
        fs::write(&stray, &gone[..gone.len() - 4]).expect("a staging file is planted");
        let parsed = PackManifest::parse(manifest.clone()).expect("the manifest parses");
        assert_eq!(
            store
                .missing(&parsed)
                .expect("the question is answerable")
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![absent.clone()],
            "the planted staging file is not the missing object, whatever is in it: nothing \
             ever looks a staging name up, which is why a kill mid-ingest cannot leave bytes \
             that a completeness check counts"
        );
        // And the pack still ingests afterwards, so a stray staging file is
        // not a jam either.
        fs::write(half.join(OBJECTS_DIR).join(&absent), &gone).expect("the object comes back");
        let held = store.ingest(&half).expect("the whole pack is ingested");
        assert!(store.holds(held.manifest.id()));
        assert!(store
            .holds_whole(&parsed)
            .expect("the question is answerable"));
    }

    /// Completeness is computed over the `kind f` rows and nothing else —
    /// and the test carries the version **without** the filter, so the
    /// detector is proved to fire rather than described as firing.
    ///
    /// The fixture's `d` and `l` rows carry perfectly well-formed digests
    /// that no `objects/` entry will ever answer to. Asked the filtered way,
    /// the stored pack is complete. Asked the way a missing filter would ask
    /// it — every row's sha256 — the same stored pack is two objects short,
    /// for ever, on every machine.
    #[test]
    fn completeness_is_computed_over_kind_f_rows_only() {
        let temp = TempDir::new("pack-kind-filter");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("pack");
        let manifest = write_pack(&dir, &[("a", b"first"), ("b", b"second")], None);
        let parsed = PackManifest::parse(manifest).expect("the manifest parses");
        assert_eq!(parsed.entries().len(), 4, "two files, a directory, a link");

        store.ingest(&dir).expect("the pack is ingested");
        assert_eq!(
            store.missing(&parsed).expect("the question is answerable"),
            BTreeMap::new(),
            "every `kind f` object is there, so the pack is complete"
        );

        // The known-bad implementation, in the test, asked of the same store:
        // every row's digest instead of the file rows'.
        let unfiltered: Vec<String> = parsed
            .entries()
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect();
        let absent: Vec<String> = unfiltered
            .into_iter()
            .filter(|sha| !store.objects_dir().join(sha).is_file())
            .collect();
        assert_eq!(
            absent,
            vec![dir_sha(), link_sha()],
            "a completeness check over every sha256 calls this complete pack two objects \
             short, and would do so on every machine for ever: that is what the `kind f` \
             filter in PackManifest::objects_needed is for"
        );
    }

    /// Laying a pack out for a job costs **zero extra bytes**, measured on
    /// the inode and on the allocated blocks rather than read off the code.
    ///
    /// Two jobs are given the same pack. Every object in both directories is
    /// the store's own inode; the link count says how many names it has; and
    /// the blocks the filesystem has allocated for the whole lot, counted
    /// once per distinct inode, are one copy of the pack and not three.
    #[test]
    fn laying_a_pack_out_for_a_job_costs_no_extra_bytes() {
        let temp = TempDir::new("pack-lay-out");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("pack");
        // Big enough that a copy would be visible in `st_blocks`: a few
        // bytes would be inside one block either way, and then the
        // measurement would prove nothing.
        let first = vec![b'a'; 512 << 10];
        let second = vec![b'b'; 256 << 10];
        let manifest = write_pack(
            &dir,
            &[("bin/tool", &first), ("data/seed.bin", &second)],
            Some(b"commit = deadbeef\n"),
        );
        let parsed = PackManifest::parse(manifest.clone()).expect("the manifest parses");
        store.ingest(&dir).expect("the pack is ingested");
        fs::remove_dir_all(&dir).expect("the job's own copy goes, as it does after a job");

        let mut inodes = std::collections::BTreeSet::new();
        let mut allocated = 0u64;
        for sha in parsed.objects_needed().keys() {
            let stored = stat(&store.objects_dir().join(sha));
            inodes.insert(stored.ino());
            allocated += stored.blocks() * 512;
        }

        for (number, at) in ["jobs/j1/in/pack", "jobs/j2/in/pack"].iter().enumerate() {
            let at = temp.path().join(at);
            let laid = store.lay_out(&parsed, &at).expect("the pack is laid out");
            assert_eq!(laid.objects, 2);
            assert_eq!(
                laid.bytes_copied, 0,
                "a pack laid out on the store's own filesystem is linked, never copied"
            );
            assert_eq!(laid.bytes_linked, (512 << 10) + (256 << 10));
            assert_eq!(
                fs::read(at.join(MANIFEST_FILE)).expect("the manifest is there"),
                manifest,
                "and the manifest at {{in}} is the bytes the id was computed over"
            );
            assert!(at.join(META_FILE).is_file(), "the meta rides beside it");
            for sha in parsed.objects_needed().keys() {
                let here = stat(&at.join(OBJECTS_DIR).join(sha));
                assert_eq!(
                    here.ino(),
                    stat(&store.objects_dir().join(sha)).ino(),
                    "{sha} at {} is the store's own inode",
                    at.display()
                );
                assert_eq!(
                    here.nlink() as usize,
                    number + 2,
                    "one name in the store and one per job that has been given the pack"
                );
            }
        }

        // The measurement that a reading of the code could not make: every
        // distinct inode under the store and both `{in}` directories, and
        // the blocks the filesystem has allocated for them.
        let mut seen = std::collections::BTreeSet::new();
        let mut total = 0u64;
        for directory in [
            store.objects_dir(),
            temp.path().join("jobs/j1/in/pack").join(OBJECTS_DIR),
            temp.path().join("jobs/j2/in/pack").join(OBJECTS_DIR),
        ] {
            for name in names_in(&directory) {
                let metadata = stat(&directory.join(name));
                if seen.insert(metadata.ino()) {
                    total += metadata.blocks() * 512;
                }
            }
        }
        assert_eq!(
            seen.len(),
            inodes.len(),
            "six names across three directories, and two inodes behind them"
        );
        assert_eq!(
            total,
            allocated,
            "the pack is allocated once however many jobs hold it: {} against one copy's {}",
            measured_bytes(total),
            measured_bytes(allocated)
        );
    }

    /// The manifest the store writes is the manifest it was given, byte for
    /// byte, and the id is the sha256 of exactly those bytes.
    ///
    /// The one mistake this module could make that nothing downstream could
    /// detect: a manifest parsed and reprinted is a *different pack*, and
    /// every machine that is told its id would then fetch bytes under a name
    /// that is not theirs.
    #[test]
    fn a_manifest_is_stored_verbatim_and_never_reprinted() {
        let temp = TempDir::new("pack-verbatim");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("pack");
        let manifest = write_pack(&dir, &[("a", b"first")], Some(b"commit = c0ffee\n"));
        let held = store.ingest(&dir).expect("the pack is ingested");

        assert_eq!(
            held.manifest.id(),
            &PackId::of_manifest_bytes(&manifest),
            "the id is the sha256 of the manifest's own bytes"
        );
        assert_eq!(
            fs::read(store.pack_dir(held.manifest.id()).join(MANIFEST_FILE))
                .expect("the stored manifest"),
            manifest,
            "stored byte for byte: a reprinted manifest is a different pack"
        );
        assert_eq!(
            fs::read(dir.join(MANIFEST_FILE)).expect("the source manifest"),
            manifest,
            "and the job's own file was not rewritten on the way through"
        );
        assert_eq!(held.meta.as_deref(), Some(&b"commit = c0ffee\n"[..]));
        let read_back = store
            .manifest_of(held.manifest.id())
            .expect("the stored pack reads back");
        assert_eq!(read_back.bytes(), manifest.as_slice());
        assert_eq!(read_back.id(), held.manifest.id());
    }

    /// A manifest whose first column is not a digest is refused by name, and
    /// nothing is joined onto the store's objects directory.
    ///
    /// **The whole sentence, verbatim, because there is now only one.** The
    /// rule is `herd_proto::pack::check_object_digest`'s and `object_name` is
    /// a two-line delegation to it, so the refusal an operator reads has one
    /// author and one wording and a test can hold it to that.
    ///
    /// It could not before, and the history is the reason this assertion
    /// looks the way it does. Two statements of the rule existed — this
    /// module's "sixty-four lowercase hex" and `herd-proto`'s "64 lowercase
    /// hex digits" — and whichever of the two spoke first depended on which
    /// branch you were standing on: both were green alone and their merge was
    /// red here. That was unblocked by weakening this assertion to
    /// `contains("lowercase hex")`, the substring the two wordings happened
    /// to share, which left the duplication standing and would have caught
    /// the next drift, if at all, by a substring. The duplication is gone, so
    /// the workaround goes with it.
    ///
    /// Note which code speaks: the hostile row is refused by the **parse**,
    /// inside `PackManifest::parse`, before `object_name` is reached at all.
    /// That is the design — the rule is applied where the entry is built —
    /// and it is why the expected text carries the parse's own `line 2 of
    /// this manifest:` prefix.
    #[test]
    fn an_object_name_that_is_not_a_digest_is_refused_rather_than_joined_onto_a_path() {
        let temp = TempDir::new("pack-bad-name");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("pack");
        write_pack(&dir, &[("a", b"first")], None);
        let text = format!(
            "{}\n../../../escaped\t5\t0644\tf\tartefact\tbin/tool\n",
            herd_proto::CHATONS_PACK_FORMAT
        );
        fs::write(dir.join(MANIFEST_FILE), text.as_bytes()).expect("a hostile manifest");
        let why = store.ingest(&dir).expect_err("that is not an object name");
        assert!(
            why.contains(
                "line 2 of this manifest: \"../../../escaped\" is 16 character(s) and a sha256 is \
                 64 lowercase hex digits. This is the column Herd builds a path out of — \
                 `objects/<sha256>` — so a value that is not a digest is a path outside the \
                 pack's own store, and it is refused here rather than joined"
            ),
            "the refusal is the one sentence this project speaks for this rule, whole: {why}"
        );
        assert_eq!(
            names_in(&store.objects_dir()),
            Vec::<String>::new(),
            "and nothing was placed anywhere"
        );
        // Upper case is not the spelling either: one pack's object must have
        // one name, or the store holds the same bytes twice. Asserted by its
        // whole sentence as well, for the reason above — `is_err()` would
        // have passed on either of the two wordings and on a third.
        let upper = herd_proto::sha256_hex(b"first").to_uppercase();
        let text = format!(
            "{}\n{}\t5\t0644\tf\tartefact\tbin/tool\n",
            herd_proto::CHATONS_PACK_FORMAT,
            upper
        );
        fs::write(dir.join(MANIFEST_FILE), text.as_bytes()).expect("an upper-case manifest");
        let first_upper = upper
            .chars()
            .find(|c| c.is_ascii_hexdigit() && c.is_uppercase())
            .expect("this digest has a letter in it");
        let why = store.ingest(&dir).expect_err("one spelling, not two");
        assert!(
            why.contains(&format!(
                "line 2 of this manifest: {upper:?} contains the uppercase hex digit \
                 {first_upper:?}, and Herd reads lowercase only. It is refused rather than \
                 lowercased: a content-addressed store that accepted both spellings could hold \
                 one object's bytes twice under two names, and nothing would say which of the \
                 two the manifest meant"
            )),
            "and the uppercase refusal is that one sentence too: {why}"
        );
    }

    /// A pack the store does not hold whole is not laid out at all, and the
    /// refusal carries the figures.
    #[test]
    fn a_pack_the_store_does_not_hold_whole_is_not_laid_out() {
        let temp = TempDir::new("pack-partial-lay-out");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("pack");
        let manifest = write_pack(&dir, &[("a", b"first"), ("b", b"second")], None);
        let parsed = PackManifest::parse(manifest).expect("the manifest parses");
        store.ingest(&dir).expect("the pack is ingested");
        fs::remove_file(store.objects_dir().join(herd_proto::sha256_hex(b"second")))
            .expect("one object is taken away under the store's feet");

        let at = temp.path().join("jobs/j1/in/pack");
        let why = store
            .lay_out(&parsed, &at)
            .expect_err("half a pack is not laid out");
        assert!(
            why.contains("holds 1 of pack") && why.contains("short 1"),
            "the refusal carries the figures: {why}"
        );
    }

    /// **A declared pack this runner cannot read is set aside and kept**, and
    /// the sentence names the path, the figures and whose the removal is.
    ///
    /// The decision this asserts: `{out}` is left, because a pack Herd cannot
    /// read would otherwise be collected as the job's output and put the
    /// whole record over its cap — but the bytes are *kept*, because Herd
    /// does not destroy an operator's bytes. Red if a later edit goes back to
    /// removing them, which is the edit this test exists for.
    #[test]
    fn a_pack_that_cannot_be_read_is_kept_rather_than_removed() {
        let temp = TempDir::new("pack-set-aside-kept");
        let out = temp.path().join("jobs/j17-2/out");
        // A declared path with no `manifest.tsv` in it: not a pack, and the
        // bytes are a build's and not Herd's to throw away.
        let bytes = vec![b'q'; 7777];
        let file = out.join("packs/some/tree/stray.bin");
        fs::create_dir_all(file.parent().expect("a parent")).expect("the directories");
        fs::write(&file, &bytes).expect("the stray file");
        let leaves = BTreeMap::from([("pack".to_owned(), "packs".to_owned())]);

        let (left, said) = leave_packs(&out, temp.path(), "j17-2", &leaves);

        assert!(left.is_empty(), "nothing was stored: {left:?}");
        assert!(
            !out.join("packs").exists(),
            "it is out of {{out}}, so the record below cannot be refused for it"
        );
        let kept = temp
            .path()
            .join(PACKS_DIR)
            .join(REFUSED_DIR)
            .join("j17-2")
            .join("packs");
        assert_eq!(
            fs::read(kept.join("some/tree/stray.bin")).expect("the bytes are still on this disk"),
            bytes,
            "kept exactly as the job left it, down to the declared path it was at"
        );
        let line = said.join(" | ");
        for part in [
            "7777 bytes",
            "are kept as",
            &kept.display().to_string(),
            "Nothing in this runner reads it",
            "removing it is yours to do",
        ] {
            assert!(
                line.contains(part),
                "the sentence must say {part:?} — the figures, where they are, and that the \
                 removal is the operator's: {line}"
            );
        }
        assert!(
            !line.contains("are gone"),
            "and it must not say the bytes are gone, because they are not: {line}"
        );
    }

    /// **The terminal rule: a set-aside pack is never picked up again**, so
    /// the state does not flip every boot.
    ///
    /// Composed the way [`crate::sweep`]'s own regression test is, over the
    /// runner's two real startup entry points rather than a stand-in for
    /// them, because that is the defect shape this guards against: the
    /// reaper and the sweep each had a green test of their own and neither
    /// ran the startup sequence, and v0.4.0 shipped a sweep that deleted what
    /// it existed to re-adopt (ORCH-164). The question here is the same one
    /// asked of a `.unclaimed` spool — does a start take these bytes away,
    /// and does it offer them to anything? — and the answer must be no twice.
    #[test]
    fn a_start_neither_removes_a_set_aside_pack_nor_picks_it_up() {
        let _reaping = crate::testing::one_reaper_at_a_time();
        let temp = TempDir::new("pack-set-aside-start");
        // A refused pack on the disk before anything starts, as the job above
        // leaves one.
        let kept = temp
            .path()
            .join(PACKS_DIR)
            .join(REFUSED_DIR)
            .join("j0000000031-1")
            .join("packs");
        let manifest = write_pack(&kept, &[("bin/tool", b"the bytes a build made")], None);
        // ...with its manifest cut short, which is what made it unreadable:
        // the pack is not a pack, and nothing may come to read it as one.
        fs::write(kept.join(MANIFEST_FILE), &manifest[..20]).expect("a truncated manifest");

        // Startup, step one: the confinement, which reaps a past life.
        let confined = crate::confine::Confined::prepare(
            crate::confine::Confinement::with_data_root(temp.path()),
        )
        .expect("a throwaway data root confines");
        let reaped = confined.report().join("\n");

        // Startup, step two: the sweep, on the same data root.
        let session = crate::Session::new();
        let swept = session.pick_up_collections(confined.root()).join("\n");

        assert_eq!(
            fs::read(
                kept.join(OBJECTS_DIR)
                    .join(herd_proto::sha256_hex(b"the bytes a build made"))
            )
            .expect("the kept bytes survive a start"),
            b"the bytes a build made",
            "a start removes nothing under {}: it is the operator's to remove and nobody \
             else's. The reaper said: {reaped}",
            temp.path().join(PACKS_DIR).join(REFUSED_DIR).display()
        );
        // And neither start path has anything to say about it: it is
        // terminal, the way a `.unclaimed` spool is, so it is not offered, not
        // resumed and not counted. Asserted on the path rather than on the
        // word, because this machine's own cgroups and stray processes are a
        // shared host's business and no part of this claim.
        let area = temp
            .path()
            .join(PACKS_DIR)
            .join(REFUSED_DIR)
            .display()
            .to_string();
        assert!(
            !reaped.contains(&area) && !swept.contains(&area),
            "nothing on the startup path names {area}. reaper: {reaped} / sweep: {swept}"
        );
        // And the store proper does not see it either: this is not a second
        // place an object is looked for.
        let store = PackStore::open(temp.path()).expect("a store on the same root");
        assert_eq!(
            names_in(&store.objects_dir()),
            Vec::<String>::new(),
            "a refused pack's objects are not the store's objects"
        );
    }

    /// **Nothing in this runner removes what it set aside** — asserted over
    /// the source, which is the only way to assert about code that does not
    /// exist yet.
    ///
    /// Two clauses. No line anywhere in this crate both removes something and
    /// names the refused area; and the refused area is named in exactly one
    /// module, so there is one place to read and no second opinion about who
    /// owns those bytes. The detector is run against a known-bad line first,
    /// because a check that passes by never firing proves nothing.
    #[test]
    fn no_source_in_this_runner_removes_a_set_aside_pack() {
        let removes_a_refused_path = |line: &str| {
            let removal = ["remove_dir_all(", "remove_file(", "remove_dir("]
                .iter()
                .any(|call| line.contains(call));
            removal && (line.contains("refused") || line.contains("REFUSED"))
        };
        assert!(
            removes_a_refused_path("            fs::remove_dir_all(store.refused_dir())?;"),
            "the detector must fire on a line that really does take one away, or its silence \
             below means nothing"
        );

        let mut names_it: Vec<String> = Vec::new();
        for (path, text) in crate::testing::source_files() {
            // The code only: the fixtures below make and drop their own
            // temporary directories, and a test is not shipped behaviour.
            let code = match text.split_once("#[cfg(test)]") {
                Some((code, _)) => code.to_owned(),
                None => text,
            };
            for line in code.lines() {
                assert!(
                    !removes_a_refused_path(line),
                    "{}: a pack Herd set aside is kept until its owner removes it, and this \
                     line takes one away: {line}",
                    path.display()
                );
            }
            if code.contains("REFUSED_DIR") || code.contains("refused_at(") {
                names_it.push(
                    path.file_name()
                        .expect("a file name")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        names_it.sort();
        assert_eq!(
            names_it,
            ["packstore.rs"],
            "the refused area has one owner in the runner's code"
        );
    }

    /// A directory that is not a pack is refused by name, and the name says
    /// what a pack is — nobody has to read this module to find out.
    #[test]
    fn a_directory_that_is_not_a_pack_is_refused_by_name() {
        let temp = TempDir::new("pack-not-a-pack");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("not-a-pack");
        fs::create_dir_all(dir.join("some/tree")).expect("an ordinary output directory");
        fs::write(dir.join("some/tree/file.txt"), b"a file").expect("a file");
        let why = store.ingest(&dir).expect_err("that is not a pack");
        assert!(
            why.contains(MANIFEST_FILE) && why.contains("a pack's identity"),
            "the refusal says what is missing and why it matters: {why}"
        );
    }

    /// **A directory that holds more than a pack is refused, and the same
    /// directory without that one name ingests.**
    ///
    /// Both halves on one fixture, because each one alone would pass a wrong
    /// check: a check that refused everything would satisfy the first
    /// assertion and fail the second, and the code as it stood before this
    /// test — whose doc already claimed this rule — satisfied the second and
    /// failed the first. What hangs on it is in
    /// `outputs::tests::a_job_s_own_file_beside_a_declared_pack_is_kept`: an
    /// ingested directory has **all** of it removed from `{out}`, so this
    /// refusal is the only thing between a job's build log and
    /// `remove_dir_all`.
    ///
    /// Nothing of a refused directory reaches the store either, which is the
    /// other half of "kept as it is": it is refused before an object is
    /// placed, not half-ingested and then regretted.
    #[test]
    fn a_directory_holding_more_than_a_pack_is_refused_and_the_same_one_without_it_ingests() {
        let temp = TempDir::new("pack-more-than-a-pack");
        let store = PackStore::open(temp.path()).expect("a store");
        let dir = temp.path().join("declared");
        let object: &[u8] = b"the bytes the pack is of";
        let manifest = write_pack(&dir, &[("bin/tool", object)], Some(b"commit = deadbeef\n"));
        // One name beside the three parts, and the shape of the defect: a job
        // that leaves its own build log next to the pack it declares.
        fs::write(dir.join("build.log"), b"a job's own output").expect("the stray file");

        let why = store.ingest(&dir).expect_err("that is more than a pack");
        for part in [
            "\"build.log\"",
            "a pack directory does not have",
            MANIFEST_FILE,
            META_FILE,
            OBJECTS_DIR,
            "nothing else",
            "kept as it is rather than partly taken apart",
        ] {
            assert!(
                why.contains(part),
                "the refusal must say {part:?} — what was found, where, that a pack is the \
                 three parts and nothing else, and that the whole directory is kept rather \
                 than partly removed: {why}"
            );
        }
        assert_eq!(
            names_in(&store.objects_dir()),
            Vec::<String>::new(),
            "and not one object of it was placed: refused before anything is taken, so there \
             is nothing to regret"
        );
        assert_eq!(names_in(&store.manifests_dir()), Vec::<String>::new());
        assert!(
            dir.join("build.log").is_file() && dir.join(MANIFEST_FILE).is_file(),
            "the directory itself is untouched by the refusal; what becomes of it is the \
             caller's, and the caller keeps it"
        );

        // The same directory, that one name taken away: a pack again.
        fs::remove_file(dir.join("build.log")).expect("the stray goes");
        let held = store
            .ingest(&dir)
            .expect("a clean three-part pack still ingests, or this check refuses everything");
        assert_eq!(
            held.manifest.id(),
            &herd_proto::PackId::of_manifest_bytes(&manifest),
            "and it is the pack its own manifest's bytes say it is"
        );
        assert_eq!(held.objects, 1);
        assert_eq!(held.objects_added, 1);
        assert_eq!(
            fs::read(store.objects_dir().join(herd_proto::sha256_hex(object)))
                .expect("the object is in the store"),
            object,
            "with its bytes where a later job can be given them"
        );
        assert!(
            store
                .holds_whole(&held.manifest)
                .expect("the question is answerable"),
            "whole, so `pack.meta` beside a manifest is an ordinary pack and not a stray"
        );
    }

    /// What a runner announces is every pack the store **holds**, and the
    /// three things in `manifests/` that are not one are left out by name.
    ///
    /// Two packs are really ingested, so the ids are the store's own and not
    /// a test's invention. Beside them, planted deliberately: a 64-hex
    /// directory with **no manifest**, which is an ingest that placed its
    /// objects and died before the manifest was written (the module's rule 1)
    /// and is therefore a pack that is absent rather than half-present; a
    /// staging name, which is what a *running* ingest looks like; and a name
    /// that is not a digest at all. An announcement that carried any of the
    /// three would tell the hub this runner holds a pack it cannot serve.
    #[test]
    fn what_a_runner_announces_is_every_pack_it_holds_and_nothing_half_made() {
        let temp = TempDir::new("pack-announce");
        let store = PackStore::open(temp.path()).expect("a store");

        let first_at = temp.path().join("first");
        let first = write_pack(&first_at, &[("bin/one", b"the first pack")], None);
        let second_at = temp.path().join("second");
        let second = write_pack(&second_at, &[("bin/two", b"the second pack")], None);
        store.ingest(&first_at).expect("the first pack ingests");
        store.ingest(&second_at).expect("the second pack ingests");
        let first = PackId::of_manifest_bytes(&first);
        let second = PackId::of_manifest_bytes(&second);

        // An ingest that never wrote its manifest: hex-named, and not a pack.
        let objects_only = "a".repeat(64);
        fs::create_dir_all(store.manifests_dir().join(&objects_only)).expect("the planted name");
        // An ingest that is happening right now.
        fs::create_dir_all(staging_path(&store.manifests_dir())).expect("a staging name");
        // And something nobody can read as a digest.
        fs::create_dir_all(store.manifests_dir().join("not-a-digest")).expect("the odd name");

        let announced = store.held_packs().expect("the store is readable");
        let mut expected = vec![first, second];
        expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(
            announced, expected,
            "the two packs that are whole, in the order the names sort, and not one of the three \
             that are not packs"
        );
    }

    /// A store that will not be read is an **error**, never an empty
    /// announcement.
    ///
    /// The distinction is the whole of why the walk answers a `Result`: an
    /// empty announcement is the frame set's only retraction, so a
    /// `read_dir` that failed would tell the hub to forget every holding this
    /// runner really has — a false statement built out of a filesystem error.
    #[test]
    fn a_store_that_will_not_be_read_is_an_error_and_not_an_empty_announcement() {
        let temp = TempDir::new("pack-announce-unreadable");
        let store = PackStore::open(temp.path()).expect("a store");
        let pack_at = temp.path().join("pack");
        write_pack(
            &pack_at,
            &[("bin/one", b"a pack that is really held")],
            None,
        );
        store.ingest(&pack_at).expect("the pack ingests");
        assert_eq!(
            store
                .held_packs()
                .expect("readable while it is there")
                .len(),
            1,
            "one pack is held, which is what makes the next line a measurement"
        );

        fs::remove_dir_all(store.manifests_dir()).expect("the directory goes");
        let why = store
            .held_packs()
            .expect_err("an unreadable store is not an empty one");
        assert!(
            why.contains("what this runner holds cannot be read"),
            "the error says what could not be read rather than answering nothing: {why}"
        );
    }

    /// A holding too large for one frame is cut, and **only the last frame
    /// applies**.
    ///
    /// The fixture holds `PACKS_HELD_PER_FRAME + 4` ids, four over the
    /// window, for the reason `FILES_IN_FLIGHT + 4` is the figure elsewhere:
    /// a fixture at or below the window is green while measuring nothing,
    /// because it never cuts. Four over forces exactly two frames, so the
    /// `more: true` path is walked and the remainder is not a whole window
    /// either.
    ///
    /// The byte figure is measured through the **production encoder**
    /// (`herd_proto::to_frame`, which is what the runner's link writes) and
    /// compared against the hub's own ceiling, `MAX_FRAME_BYTES = 1 << 20`
    /// (`herd-hub/src/server.rs:167`). A chunk size that fit the array and
    /// not the frame would be a runner whose every announcement the hub
    /// refuses.
    #[test]
    fn a_holding_too_large_for_one_frame_is_cut_and_only_the_last_frame_applies() {
        let held: Vec<PackId> = (0..PACKS_HELD_PER_FRAME + 4)
            .map(|which| PackId::of_manifest_bytes(format!("pack {which}").as_bytes()))
            .collect();

        let frames = announcement_frames(held.clone());
        assert_eq!(frames.len(), 2, "cut into two, so the cutting is exercised");
        assert_eq!(frames[0].packs.len(), PACKS_HELD_PER_FRAME);
        assert!(
            frames[0].more,
            "the first frame accumulates and applies nothing"
        );
        assert_eq!(frames[1].packs.len(), 4);
        assert!(
            !frames[1].more,
            "the last frame is what replaces the hub's view"
        );
        let announced: Vec<PackId> = frames
            .iter()
            .flat_map(|frame| frame.packs.clone())
            .collect();
        assert_eq!(
            announced, held,
            "every id rides exactly one frame, in the order it was held"
        );

        // The hub's ceiling, measured against the encoder the link uses.
        const MAX_FRAME_BYTES: usize = 1 << 20;
        let widest = frames
            .iter()
            .map(|frame| {
                herd_proto::to_frame(&herd_proto::ToHub::PacksHeld(frame.clone()))
                    .expect("the frame encodes")
                    .len()
            })
            .max()
            .expect("there are frames");
        assert!(
            widest < MAX_FRAME_BYTES,
            "the widest frame is {widest} bytes, which the hub would refuse over \
             {MAX_FRAME_BYTES}"
        );

        // And exactly at the window: one frame, and it is the one that
        // applies. The off-by-one the other way round would send an empty
        // second frame, which a hub reads as a retraction of what the first
        // one just said.
        let exactly = announcement_frames(held[..PACKS_HELD_PER_FRAME].to_vec());
        assert_eq!(exactly.len(), 1, "a full window is still one frame");
        assert!(!exactly[0].more, "and it applies");
    }

    /// A runner that holds nothing announces **one empty frame that applies**,
    /// and that is not the same as announcing nothing.
    ///
    /// It is the only retraction the frame set has. No announcement says
    /// nothing, and a hub told nothing goes on offering a holder that has
    /// dropped everything it had.
    #[test]
    fn a_runner_that_holds_nothing_announces_one_empty_frame_that_applies() {
        let frames = announcement_frames(Vec::new());
        assert_eq!(frames.len(), 1, "nothing to say is still said");
        assert!(frames[0].packs.is_empty());
        assert!(
            !frames[0].more,
            "and it applies, or the hub would accumulate an emptiness for ever"
        );
    }

    /// A store of known blobs reports **exactly** their summed lengths and
    /// their count.
    ///
    /// The figures are built out of the fixture's own bytes — `shared.len()`
    /// and the rest — and never typed beside it, so a change to the fixture
    /// moves the expectation with it rather than reddening a number nobody
    /// can place.
    ///
    /// **Before anything measures, the figure says so.** `measured` is false
    /// on a store nothing has read, and false is not nought: a hub told
    /// `0 bytes` by a runner that never looked would believe an empty store.
    #[test]
    fn a_measured_store_reports_its_objects_summed_lengths_and_its_pack_count() {
        let temp = TempDir::new("pack-measure");
        let store = PackStore::open(temp.path()).expect("a store");

        let fresh = store.usage();
        assert!(
            !fresh.measured,
            "nothing has read this store yet, and that is not the same as it being empty"
        );
        assert_eq!((fresh.bytes, fresh.packs), (0, 0));

        let one: &[u8] = b"the first pack's own bytes";
        let two: &[u8] = b"a second content, of a different length entirely";
        let shared: &[u8] = b"bytes both packs name";

        let first = temp.path().join("first");
        write_pack(&first, &[("bin/tool", one), ("lib/shared", shared)], None);
        let second = temp.path().join("second");
        write_pack(&second, &[("bin/other", two), ("lib/shared", shared)], None);
        store.ingest(&first).expect("the first pack is ingested");
        store.ingest(&second).expect("the second pack is ingested");

        let measured = store.measure().expect("the store is measurable");
        assert!(measured.measured, "it has now been read");
        assert_eq!(
            measured.bytes,
            (one.len() + two.len() + shared.len()) as u64,
            "three distinct contents across two packs, each counted once: {:?}",
            names_in(&store.objects_dir())
        );
        assert_eq!(measured.packs, 2, "two manifests, two packs");
        assert_eq!(
            store.usage(),
            measured,
            "the running figure is what the walk just seeded"
        );

        // A staging file a kill left behind weighs nought, exactly as it is
        // announced as nothing: one rule — a 64-hex name — for both halves of
        // the same question.
        fs::write(
            store.objects_dir().join(format!("{STAGING_PREFIX}99-1")),
            b"half an object",
        )
        .expect("a staging file");
        let again = store.measure().expect("the store is measurable");
        assert_eq!(
            again.bytes, measured.bytes,
            "a half-made object is not in the store, so it is not in the store's figure"
        );
        assert_eq!(again.packs, measured.packs);
    }

    /// A second `ingest` moves the total **by the new blobs only**.
    ///
    /// Which is the whole claim of a content-addressed store, as a figure:
    /// the second pack names two objects and the store already held one of
    /// them, so what the total moves by is the one it did not hold. Ingested
    /// a third time, nothing moves at all.
    ///
    /// And the running figure is checked against the walk at the end, which
    /// is the one assertion that catches arithmetic drifting from the disk:
    /// `measure` reads `objects/` and the running figure has not read
    /// anything since the first one.
    #[test]
    fn a_second_ingest_moves_the_total_by_the_new_blobs_only() {
        let temp = TempDir::new("pack-running-total");
        let store = PackStore::open(temp.path()).expect("a store");
        let shared: &[u8] = b"the bytes both packs hold";
        let only_in_the_second: &[u8] = b"second only, and longer than the first";

        let first = temp.path().join("first");
        write_pack(&first, &[("bin/tool", shared)], None);
        store.ingest(&first).expect("the first pack is ingested");
        store.measure().expect("seeded from the disk");
        assert_eq!(
            (store.usage().bytes, store.usage().packs),
            (shared.len() as u64, 1)
        );

        let second = temp.path().join("second");
        write_pack(
            &second,
            &[("lib/tool.so", shared), ("only/here", only_in_the_second)],
            None,
        );
        let held = store.ingest(&second).expect("the second pack is ingested");
        assert_eq!(held.objects_added, 1, "one of its two objects was placed");
        assert_eq!(
            store.usage().bytes,
            (shared.len() + only_in_the_second.len()) as u64,
            "the total moved by the object that was placed and not by the one the store \
             already held"
        );
        assert_eq!(store.usage().packs, 2);

        // Again, with the same directory: a pack the store holds whole costs
        // nothing and counts nothing.
        let standing = store.usage();
        let third = store.ingest(&second).expect("an idempotent ingest");
        assert_eq!(third.objects_added, 0);
        assert_eq!(
            store.usage(),
            standing,
            "a pack already held moves neither figure"
        );

        // And the arithmetic agrees with the disk, which is what makes the
        // running figure worth keeping rather than merely cheap.
        let walked = store.measure().expect("the store is measurable");
        assert_eq!(
            walked, standing,
            "the walk and the running total are the same two numbers"
        );
    }

    /// **Saying what this runner holds lists no `objects/` directory**, and
    /// that is asserted by counting the listings rather than by timing them.
    ///
    /// **Why a counter and not a stopwatch.** This host is shared with the
    /// first consumer's production (D161) and with other agents' builds; a
    /// test that announced twice and compared the durations would be
    /// measuring the load on the machine, and would be green on a quiet one
    /// whatever the code did. So every listing of `objects/` in this crate's
    /// product code goes through `PackStore::objects_bytes` — checked by
    /// `grep` over `herd-runner/src/`, where the only other `read_dir` of
    /// that directory is in a test — and the counter it keeps is the
    /// assertion.
    ///
    /// **The announcement is performed as the runner performs it**, by the
    /// same three calls `Runner::announce_packs_held` makes in that order:
    /// `PackStore::open` of the data root, `held_packs`, `announcement_frames`.
    /// A fresh `open` is the case that matters — the figure survives it
    /// because it belongs to the data root and not to the value, and an
    /// implementation that re-measured on open would walk the store at every
    /// welcome and at the end of every job.
    ///
    /// **The detector is proven to fire** before its silence is trusted: the
    /// counter is shown moving on a `measure`, which is the one call that is
    /// supposed to move it.
    #[test]
    fn saying_what_this_runner_holds_lists_no_objects_directory() {
        let temp = TempDir::new("pack-no-walk");
        let store = PackStore::open(temp.path()).expect("a store");
        let pack = temp.path().join("pack");
        write_pack(&pack, &[("bin/tool", b"some bytes to hold")], None);
        store.ingest(&pack).expect("the pack is ingested");

        // The detector, proven on the call that must move it.
        let before = store.objects_listed();
        store.measure().expect("the store is measurable");
        assert_eq!(
            store.objects_listed(),
            before + 1,
            "a measurement lists objects/ exactly once, and this counter is what says so"
        );

        let seeded = store.usage();
        let listed = store.objects_listed();
        for _ in 0..5 {
            // Exactly `Runner::announce_packs_held`'s own three calls.
            let announcing = PackStore::open(temp.path()).expect("the store, opened again");
            let held = announcing.held_packs().expect("what this runner holds");
            let frames = announcement_frames(held);
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].packs.len(), 1, "the one pack it holds");
            assert_eq!(
                announcing.objects_listed(),
                listed,
                "announcing what this runner holds listed objects/ again; the figure is a \
                 running one and an announcement must do arithmetic, not a walk"
            );
            assert_eq!(
                announcing.usage(),
                seeded,
                "a store opened again reads the figure its data root already had, rather \
                 than starting from nothing"
            );
        }
        assert_eq!(store.objects_listed(), listed, "and nor did any of them");
    }

    /// The budget is the operator's figure, kept per data root, and **absent
    /// is not nought**.
    ///
    /// `None` is "nobody said" and nought is "hold nothing": a hub handed
    /// `0` for a budget nobody set would report a runner that may keep no
    /// pack at all. The sentence printed at startup says which it is, and
    /// the figure in it reads back through `show_bytes` — the formatter for a
    /// figure a human typed — while what was measured reads back through
    /// `measured_bytes`.
    #[test]
    fn the_budget_is_taken_once_and_read_from_every_store_of_that_root() {
        let temp = TempDir::new("pack-budget");
        let store = PackStore::open(temp.path()).expect("a store");
        assert_eq!(store.budget(), None, "nobody has said anything yet");

        let pack = temp.path().join("pack");
        let bytes: &[u8] = b"a pack's one object";
        write_pack(&pack, &[("bin/tool", bytes)], None);
        store.ingest(&pack).expect("the pack is ingested");

        let budget = 40 << 30;
        let said = take_budget(temp.path(), Some(budget));
        assert!(
            said.contains(&show_bytes(budget)) && said.contains(PACK_BUDGET_FLAG),
            "the startup line states the budget as its own formatter prints it, and names \
             the flag that set it: {said}"
        );
        assert!(
            said.contains(&measured_bytes(bytes.len() as u64)),
            "and what it holds now as a measurement, which is the other formatter: {said}"
        );
        assert_eq!(
            PackStore::open(temp.path())
                .expect("the store, opened again")
                .budget(),
            Some(budget),
            "the budget belongs to the data root, so every store of it reads the figure the \
             operator gave once"
        );

        // Absent, said and kept apart from nought.
        let empty = TempDir::new("pack-no-budget");
        let said = take_budget(empty.path(), None);
        assert!(
            said.contains("no budget") && said.contains(PACK_BUDGET_FLAG),
            "a runner with no pack budget says so and names the flag: {said}"
        );
        let store = PackStore::open(empty.path()).expect("a store");
        assert_eq!(store.budget(), None, "absent, and not nought");
        assert_eq!(
            (
                store.usage().bytes,
                store.usage().packs,
                store.usage().measured
            ),
            (0, 0, true),
            "a store with nothing in it was read, and holds nothing: measured, and nought"
        );
    }

    /// A store that is **not there yet** holds nothing, and that is a
    /// measurement rather than an error.
    ///
    /// `take_budget` runs at startup, before `herd_runner::run` has validated
    /// the data root, so it reads the store and creates nothing: a `packs/`
    /// made on that line would leave a directory behind on a root that was
    /// about to be refused by name (E-45), and an error printed about a
    /// directory nothing has had a reason to make yet would be noise at every
    /// first start.
    #[test]
    fn a_store_that_is_not_there_yet_holds_nothing_and_creates_nothing() {
        let temp = TempDir::new("pack-absent");
        let root = temp.path().join("not-made-yet");
        let said = take_budget(&root, None);
        assert!(
            said.contains("0 bytes") && said.contains("0 pack(s)"),
            "a store that is not there holds nothing, and says nothing is wrong: {said}"
        );
        assert!(
            !root.exists(),
            "and it made neither the data root nor {PACKS_DIR}/ on the way"
        );
    }

    /// Two spellings of one data root are **one** store's figures.
    ///
    /// This is not tidiness, it is the one way this task could have shipped
    /// a silent nought. The startup line takes the budget against the path
    /// the operator typed; everything inside a running runner opens this
    /// store under `DataRoot`'s **canonicalised** path, because
    /// `crate::root::DataRoot::open` canonicalises what it validates. A
    /// figure keyed by the literal string would have been registered against
    /// the spelling nobody reads, and the health report of a machine nobody
    /// in this project may log into (D161) would have carried a budget of
    /// "nobody said" and a total of nought.
    ///
    /// Three spellings here, all of one directory: through a symlink, with a
    /// trailing separator, and with a `.` component in the middle.
    #[test]
    fn two_spellings_of_one_data_root_are_one_store() {
        let temp = TempDir::new("pack-one-key");
        let real = temp.path().join("real-root");
        fs::create_dir_all(&real).expect("the data root");
        let linked = temp.path().join("linked-root");
        std::os::unix::fs::symlink(&real, &linked).expect("a symlinked spelling of it");

        let budget = 8 << 30;
        take_budget(&linked, Some(budget));

        let pack = temp.path().join("pack");
        let bytes: &[u8] = b"one object, counted once";
        write_pack(&pack, &[("bin/tool", bytes)], None);
        PackStore::open(&real)
            .expect("the store, by its real path")
            .ingest(&pack)
            .expect("the pack is ingested");

        for spelling in [
            real.clone(),
            linked.clone(),
            real.join("."),
            PathBuf::from(format!("{}/", linked.display())),
        ] {
            let store = PackStore::open(&spelling).expect("the store, spelled another way");
            assert_eq!(
                store.budget(),
                Some(budget),
                "{} read a different budget from the one the operator gave once",
                spelling.display()
            );
            assert_eq!(
                (store.usage().bytes, store.usage().packs),
                (bytes.len() as u64, 1),
                "{} read a different figure from the one the ingest moved",
                spelling.display()
            );
        }
    }
}
