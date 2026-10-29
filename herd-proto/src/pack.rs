//! Packs: what a job leaves for a later job, and how one machine gets it
//! from another (M11, D10, D11, D200, D209).
//!
//! A pack is the first consumer's build output, measured by them on
//! 2026-10-13 (issue #16): **401.6 MiB in 557 files**, built in 38 s and
//! verified in 1.5–2.1 s. Their `build` command leaves one and their
//! `test-pack` command needs the one the build of the same commit left. That
//! is the whole of the relationship — one parent, no graph (invariant 1).
//!
//! ## Why the peer wire lives here
//!
//! Invariant 4 says every message lives in `herd-proto` and nowhere else, and
//! it does not make an exception for the leg that does not touch the hub. The
//! bytes of a pack move **runner to runner** (D10: the hub is never a disk
//! sink for content it has no use for), so [`PeerRequest`], [`PeerReply`] and
//! [`PeerRefusal`] are a second wire — and a second wire written twice, once
//! in the runner that serves and once in the runner that fetches, would be
//! two truths about one contract. It is one wire, written once, here.
//!
//! It gets **no version of its own**. Both peers are joined to the same hub
//! at the same [`crate::PROTOCOL_VERSION`], so a peer that could talk to the
//! hub at all already agrees about this; [`PeerRequest::protocol_version`]
//! carries the number so a mismatch is refused **by name** rather than read
//! as a malformed request, and that is all it is for. One version, not two.
//!
//! ## The three parts of a pack, and the one that does not travel far
//!
//! ```text
//! <id>/manifest.tsv     "# chatons pack 1", "# profile debug", then per entry:
//!                       sha256 TAB size TAB octal mode TAB kind f|d|l TAB role TAB path [TAB link target]
//! <id>/pack.meta        commit, profile, scenarios, sizes per class — not in the id
//! <id>/objects/<sha256> each file's bytes once, under its own sha256
//! <id>                  = sha256 of manifest.tsv
//! ```
//!
//! `manifest.tsv` and `pack.meta` are **kilobytes** — about 42 KiB raw for
//! the consumer's 557 entries, some 56 KiB once base64'd — so they ride the
//! control plane on an ordinary frame, which is literally D10's *"metadata is
//! kilobytes"*. Only `objects/<sha256>` moves peer to peer. That split has a
//! property worth naming: because the hub receives the manifest's bytes, it
//! can recompute the id by hashing them, so it **never takes a pack's
//! identity on a runner's word**.
//!
//! ## The identity rule, which is the single most important thing in here
//!
//! **`<id>` is the sha256 of `manifest.tsv`'s bytes.** Not of a canonical
//! form of them, not of the entries, not of anything Herd could compute from
//! a parse — of the bytes, exactly as the consumer's tool wrote them. So a
//! manifest that is parsed and **reprinted** is a different pack: one trailing
//! newline, one column reordered, one `0644` written `644`, and Herd has
//! silently minted an id for a pack nobody has.
//!
//! [`PackManifest`] is built so that cannot happen by accident. It owns the
//! verbatim bytes in a private field, it hands them out by reference, it has
//! **no `Serialize`, no `Display`, no `to_string` and no method anywhere that
//! produces manifest text**, and its [`fmt::Debug`] prints a summary rather
//! than the content so that even a debug print cannot be mistaken for a copy.
//! The only way to get a manifest's bytes out of Herd is the bytes that went
//! in. On the wire the manifest is therefore a `Vec<u8>` carried base64
//! ([`crate::bytes_as_base64`]) and never a struct: a struct would have to be
//! re-serialised to be sent, and that is exactly the mistake.
//!
//! ## What Herd reads out of a manifest, and what it refuses to
//!
//! Herd enumerates the `sha256` column of the **`kind f`** rows, so it knows
//! which objects a complete pack is made of. It interprets nothing else: not
//! the mode, not the role, not the path, not the link target, and it never
//! follows a link or lays out a tree. The consumer's `--pack DIR` *"lays the
//! files out in a tree of its own"*, so laying out is theirs and
//! `{in}/<key>` is a directory of the three-part shape above.
//!
//! **The `kind` column is load-bearing.** A `kind d` and a `kind l` row carry
//! a sha256 too, of something that is not a file's bytes, so a completeness
//! check that looked every sha256 up under `objects/` would report a
//! perfectly good pack as incomplete. The filter on `kind == f`
//! ([`PackManifest::files`], [`PackManifest::objects_needed`]) is the
//! substance, and the fixture every test in this crate uses carries a `d` row
//! **and** an `l` row so that the filter is proven and not assumed.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The first line of a manifest Herd will read: `# chatons pack 1`.
///
/// Read and **refused by name**, never interpreted (invariant 1). The
/// consumer owns this format and may move it to 2; when they do, Herd says so
/// in a sentence naming both numbers rather than guessing which columns
/// changed. That is the only extensibility point in the pack half, and it is
/// the consumer's, not a plugin of Herd's.
pub const CHATONS_PACK_FORMAT: &str = "# chatons pack 1";

/// How much of one object one [`PeerReply`] carries: **1 MiB**.
///
/// **Derived from the endpoint's read buffer and from the consumer's measured
/// objects — deliberately not from [`crate::FRAME_CEILING`]**, which D148
/// forbids moving and which is a property of a WebSocket frame the peer path
/// does not use at all. The peer leg is a plain stream between two runners;
/// nothing on it is a frame of the control protocol.
///
/// The two figures that chose it:
///
/// * eight reads of [`PACK_ENDPOINT_READ_BYTES`], so a chunk is a handful of
///   syscalls rather than one per kilobyte or one per pack;
/// * the consumer's pack is 401.6 MiB in 557 files, a mean object of about
///   738 KiB, so **the median object is one chunk** and the common case costs
///   one request and one reply. A smaller chunk would turn 557 objects into
///   thousands of round trips; a larger one would buy nothing, because most
///   objects are smaller than this already.
pub const PACK_CHUNK_BYTES: u64 = 8 * PACK_ENDPOINT_READ_BYTES;

/// The fetch endpoint's read buffer: **128 KiB**.
///
/// The primitive [`PACK_CHUNK_BYTES`] is computed from, here rather than in
/// the runner for the reason [`crate::HEALTH_EVERY`] is here: both ends
/// depend on it. A serving runner sizes its per-connection buffer by it, and
/// a fetching runner's chunk is a multiple of it, so one figure bounds the
/// endpoint's memory per peer and the chunk count of a transfer at once.
pub const PACK_ENDPOINT_READ_BYTES: u64 = 128 << 10;

/// The largest `manifest.tsv` Herd carries on a frame: **8 MiB**.
///
/// A manifest that will not fit is **refused by name, never truncated**: a
/// truncated manifest still parses, still hashes to something, and would
/// register a pack that is missing files nobody can see — the shape of
/// failure D166 and D148 both exist to prevent.
///
/// Two readings of the figure, so it is a bound and not a guess. Against the
/// wire: 8 MiB base64's to about 10.7 MiB, which leaves most of
/// [`crate::FRAME_CEILING`]'s 64 MiB for `pack.meta` and the frame around it.
/// Against the consumer: their 557 entries are about 42 KiB, some 77 bytes an
/// entry, so this is room for roughly 109 000 entries — about 195 times the
/// pack they measured.
pub const MANIFEST_MAX_BYTES: u64 = 8 << 20;

/// How long a transfer capability the hub minted stays usable: **5 minutes**.
///
/// It has to outlive a whole pack, because a capability that expires
/// mid-transfer turns a slow link into a refusal nobody can explain. Against
/// the consumer's 401.6 MiB that is a floor of about 1.4 MB/s sustained —
/// slower than any link two machines of one fleet share, and twenty times the
/// 15 s health cadence ([`crate::HEALTH_EVERY`]) so a transfer never has to
/// race a report.
///
/// It is still **ephemeral and per (pack, fetching peer)** (D200, D209): the
/// lifetime is what bounds a leaked token, not what bounds a transfer.
pub const PACK_CAPABILITY_LIFETIME: Duration = Duration::from_secs(300);

/// A pack's identity: the **sha256 of `manifest.tsv`'s bytes**, lowercase hex.
///
/// A newtype rather than a `String` because the one way to get this wrong is
/// to compute it over something else — a parse, a re-print, a directory name
/// somebody typed — and a distinct type is what makes
/// [`PackId::of_manifest_bytes`] the obvious and only way in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackId(String);

impl PackId {
    /// The id of a pack whose `manifest.tsv` is exactly these bytes.
    ///
    /// **The only constructor that computes anything**, and it computes over
    /// the bytes it was handed, verbatim. Whoever holds bytes that came off a
    /// disk or off a frame can mint the id; nobody can mint one from a parsed
    /// manifest, because there is no function that takes one.
    pub fn of_manifest_bytes(bytes: &[u8]) -> Self {
        Self(crate::sha256_hex(bytes))
    }

    /// An id a peer sent, taken as a name and not as a claim.
    ///
    /// Separate from [`PackId::of_manifest_bytes`] and named so, because the
    /// two are not the same act: this one is "the pack called this", and it is
    /// the hub's job to recompute the id from the manifest's bytes and refuse
    /// a mismatch by name rather than believe the string.
    pub fn as_announced(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a manifest row says a row is: a file, a directory, or a symlink.
///
/// The only column Herd interprets, and the reason it has to:
/// [`ManifestKind::File`] rows are the ones whose sha256 names an object
/// under `objects/`. The other two carry a sha256 of something that is not a
/// file's bytes, and a completeness check that did not filter would call a
/// good pack incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    /// `f`: bytes that live under `objects/<sha256>`.
    File,
    /// `d`: a directory. Its sha256 is not an object.
    Dir,
    /// `l`: a symlink, whose target is the row's last column. Herd never
    /// follows it and never reproduces it; the consumer's own tool lays the
    /// tree out.
    Link,
}

impl ManifestKind {
    /// The one-letter column, as the consumer's tool writes it.
    pub fn letter(self) -> &'static str {
        match self {
            Self::File => "f",
            Self::Dir => "d",
            Self::Link => "l",
        }
    }

    /// The kind a column letter names, or nothing if Herd has never heard of
    /// it.
    ///
    /// `None` is a **refusal** at every call site and never a shrug. A kind
    /// letter this build does not know, read as "not a file", would drop the
    /// row out of the completeness check — so a pack missing that row's bytes
    /// would register as complete, which is the quiet wrong answer the filter
    /// exists to prevent.
    pub fn from_letter(letter: &str) -> Option<Self> {
        match letter {
            "f" => Some(Self::File),
            "d" => Some(Self::Dir),
            "l" => Some(Self::Link),
            _ => None,
        }
    }
}

/// One row of a `manifest.tsv`, parsed.
///
/// A **view**, never a source. Nothing in Herd writes one of these back out
/// as text: the manifest's bytes are the manifest, and these are what Herd
/// read out of them in order to know which objects the pack is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Column 1, lowercase hex. For a `kind f` row this is the name of the
    /// object under `objects/`; for the other kinds it is a digest of
    /// something that is not a file's bytes, and Herd does not look it up.
    pub sha256: String,
    /// Column 2: the entry's size in bytes, as the consumer's tool measured
    /// it.
    pub size: u64,
    /// Column 3, **as written** — the octal text, not a parsed number.
    ///
    /// Herd never applies a mode, so parsing one would be inventing a
    /// use for it; and the text is what a consumer reading a pack's
    /// provenance would compare against their own manifest.
    pub mode: String,
    /// Column 4, the only column Herd interprets.
    pub kind: ManifestKind,
    /// Column 5: the consumer's own classification of the entry. Carried,
    /// printed, never matched against a list of roles Herd knows (E-11).
    pub role: String,
    /// Column 6, relative to the pack's own root. Not a path Herd ever joins
    /// onto anything: the consumer's tool lays the tree out.
    pub path: String,
    /// Column 7, present on the rows that have one. Never followed.
    pub link_target: Option<String>,
}

/// A `manifest.tsv` and what Herd read out of it, with the bytes kept
/// **verbatim**.
///
/// ## Why this type cannot be re-serialised, and how
///
/// The pack's id is the sha256 of these bytes, so a reprinted manifest is a
/// different pack (see the module doc). Four things together make that
/// unavailable rather than merely discouraged:
///
/// 1. `bytes` is **private**, and the accessor returns `&[u8]` — the bytes
///    that went in, not bytes assembled from the parse;
/// 2. there is **no `Serialize`** and no `Deserialize`. A struct carried on a
///    frame has to be serialised to be sent, and that is the mistake; so what
///    rides a frame is `Vec<u8>` base64 ([`crate::bytes_as_base64`]) and this
///    type never leaves the process that parsed it;
/// 3. there is **no `Display`, no `ToString`** and no method anywhere that
///    returns manifest text. There is nothing to call;
/// 4. even [`fmt::Debug`] prints a summary — the id, the byte count, the row
///    counts — rather than the content, so a debug print of a manifest cannot
///    be pasted anywhere as a manifest.
///
/// What a caller can do is ask for the bytes it already has, the id computed
/// over them, and the objects the pack is made of. That is every use Herd
/// has.
#[derive(Clone, PartialEq, Eq)]
pub struct PackManifest {
    bytes: Vec<u8>,
    id: PackId,
    headers: Vec<String>,
    entries: Vec<ManifestEntry>,
}

impl PackManifest {
    /// Parse a manifest's bytes, keeping them.
    ///
    /// Refuses by name, with figures, rather than reading past anything it
    /// does not understand: an unreadable manifest is a pack Herd will not
    /// register, and a pack it will not register is a named waiting state
    /// ([`PackState::Incomplete`]) rather than a silence.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() as u64 > MANIFEST_MAX_BYTES {
            return Err(format!(
                "this manifest is {} bytes and Herd carries at most {MANIFEST_MAX_BYTES} on a \
                 frame; it is refused whole rather than truncated, because a truncated manifest \
                 still parses and would register a pack that is missing files nobody can see",
                bytes.len()
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| format!("this manifest is not UTF-8: {error}"))?;
        let mut lines = text.lines();
        match lines.next() {
            Some(first) if first == CHATONS_PACK_FORMAT => {}
            Some(first) => {
                return Err(format!(
                    "this manifest's first line is {first:?} and Herd reads \
                     {CHATONS_PACK_FORMAT:?}. The format is the consumer's, and Herd refuses one \
                     it has not been taught rather than guessing which columns moved"
                ))
            }
            None => return Err("this manifest is empty".to_owned()),
        }
        let mut headers = Vec::new();
        let mut entries = Vec::new();
        for (offset, line) in lines.enumerate() {
            // 1 for the format line already taken, 1 because a reader counts
            // from one: the number in a refusal must match what an operator
            // sees in an editor.
            let number = offset + 2;
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('#') {
                headers.push(header.trim().to_owned());
                continue;
            }
            entries.push(entry_of_row(line, number)?);
        }
        let id = PackId::of_manifest_bytes(&bytes);
        Ok(Self {
            bytes,
            id,
            headers,
            entries,
        })
    }

    /// The manifest's bytes, exactly as they arrived.
    ///
    /// What goes on a frame, what is written to disk, and what the id is
    /// computed over — one source for all three, which is the whole of the
    /// identity rule.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The pack's id: the sha256 of [`PackManifest::bytes`], computed at
    /// parse time.
    pub fn id(&self) -> &PackId {
        &self.id
    }

    /// Every row, in file order.
    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    /// The `#` lines after the format line, trimmed, in order — the
    /// consumer's own `profile debug` and whatever else they put there.
    ///
    /// Carried so a report can print them. Never matched against a list.
    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    /// The rows whose bytes live under `objects/`: **`kind f` and nothing
    /// else**.
    ///
    /// The filter this module exists to get right. A `kind d` or `kind l` row
    /// also carries a sha256, of something that is not a file's bytes, so
    /// every completeness question goes through here.
    pub fn files(&self) -> impl Iterator<Item = &ManifestEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == ManifestKind::File)
    }

    /// The objects a complete copy of this pack holds: each `kind f` row's
    /// sha256, with the size it must be.
    ///
    /// Distinct by sha256, because a pack with the same bytes at two paths
    /// stores them once — that is what content addressing buys, and it is why
    /// the figure a transfer reports is objects and not rows.
    pub fn objects_needed(&self) -> BTreeMap<String, u64> {
        self.files()
            .map(|entry| (entry.sha256.clone(), entry.size))
            .collect()
    }

    /// Whether this pack's sha256 is the one a peer announced.
    ///
    /// The hub's own check, by name: it recomputes the id from the bytes it
    /// received and refuses a mismatch, so a pack's identity is never taken on
    /// a runner's word.
    pub fn disagreement_with(&self, announced: &PackId) -> Option<String> {
        (announced != &self.id).then(|| {
            format!(
                "this manifest's {} bytes hash to {}, and the pack was announced as {announced}. \
                 A pack's id is the sha256 of its manifest's bytes, so these are two different \
                 packs and not one pack named twice",
                self.bytes.len(),
                self.id
            )
        })
    }
}

/// A summary, never the content: see [`PackManifest`]'s own doc, point 4.
impl fmt::Debug for PackManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let files = self.files().count();
        f.debug_struct("PackManifest")
            .field("id", &self.id.as_str())
            .field("bytes", &self.bytes.len())
            .field("rows", &self.entries.len())
            .field("files", &files)
            .finish()
    }
}

/// One row, or why it is not one.
fn entry_of_row(line: &str, number: usize) -> Result<ManifestEntry, String> {
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < 6 || columns.len() > 7 {
        return Err(format!(
            "line {number} of this manifest has {} tab-separated column(s), and a row has six or \
             seven: sha256, size, mode, kind, role, path, and a link target on the rows that have \
             one",
            columns.len()
        ));
    }
    let kind = ManifestKind::from_letter(columns[3]).ok_or_else(|| {
        format!(
            "line {number} of this manifest has kind {:?}, and Herd reads f, d or l. A kind it \
             has not been taught is refused rather than read as \"not a file\", because a row \
             dropped out of the completeness check is a pack that registers while its bytes are \
             missing",
            columns[3]
        )
    })?;
    let size = columns[1].parse::<u64>().map_err(|error| {
        format!(
            "line {number} of this manifest has size {:?}, which is not a number of bytes: {error}",
            columns[1]
        )
    })?;
    Ok(ManifestEntry {
        sha256: columns[0].to_owned(),
        size,
        mode: columns[2].to_owned(),
        kind,
        role: columns[4].to_owned(),
        path: columns[5].to_owned(),
        link_target: columns.get(6).map(|target| (*target).to_owned()),
    })
}

/// Which way the bytes go on a transfer the hub authorised.
///
/// Two values and not a boolean, because the hub mints a capability for each
/// and the holder is told which one it is being asked for: a runner that may
/// be *fetched from* has not thereby agreed to be *pushed to*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    /// The asking peer pulls the objects from the holder.
    Fetch,
    /// The asking peer pushes the objects to the holder — D10's outbound-only
    /// path, for a runner that listens on nothing at all (invariant 3).
    Push,
}

/// Which of D10's three paths a pack actually came by.
///
/// Reported on [`PackTransferred`] and asserted in the end-to-end, because
/// "it arrived" is not the claim D10 makes: the claim is that the cheapest
/// available path was the one taken, and a figure that does not say which
/// path it measured cannot support it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchPath {
    /// Already on this host: hardlinked or reflinked inside the local store.
    /// **Zero network bytes and no second copy.**
    SameHost,
    /// Fetched straight from a holder the fetcher can reach.
    Peer,
    /// Neither peer could be dialled by the other, so a peer the hub named
    /// carried it. Still never the hub (D10).
    ViaPeer,
}

/// The named states a pack can be in from where a job stands.
///
/// Three, and the point of each is that it is **said** rather than being an
/// absence somebody has to infer. A job whose `needs` cannot be met waits
/// with a reason on it; it is not a silence and it is not
/// [`crate::FromHub::Refused`], which closes the link (D13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackState {
    /// A manifest is registered for this `(commit, key)` and every object of
    /// it is held somewhere.
    Registered,
    /// A manifest is registered and some object of it is held nowhere this
    /// hub knows of.
    Incomplete,
    /// A manifest is registered and every runner that held it is gone.
    NoHolder,
}

/// A runner that holds a complete copy of a pack, and how to reach it.
///
/// Holdings are **per manifest, not per blob** and **volatile**: they are
/// re-announced after every [`crate::FromHub::Welcome`], so a hub that
/// restarted has no stale holder to offer and no durable holdings file to
/// keep true. Blob-level holdings would buy nothing — a fetcher consults its
/// own content store for what it lacks, which is where "fetch only the diff"
/// actually happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    /// The runner's name, as the hub knows it. Not a machine a requester may
    /// ask for (invariant 6): this reaches a *runner* that has been given a
    /// job, never a requester.
    pub runner: String,
    /// Where it serves content, when it serves content at all.
    ///
    /// `None` is an **outbound-only** holder: a runner that listens on
    /// nothing, because its host has no address invariant 3 permits it to
    /// bind. It is not a defect and not a degraded holder; it is why D10 has
    /// a push path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// One pack a job needs, resolved, as [`crate::FromHub::AssignJob`] hands it
/// over.
///
/// The manifest and the holders travel **with the assignment**, so the common
/// case costs **no round trip**: a runner that already holds every object
/// lays `{in}/<key>` out without saying a word to anybody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignedPack {
    /// The `needs` key this pack answers, as the consumer's declaration
    /// spells it. It is what names the directory under `{in}`.
    pub key: String,
    pub pack: PackId,
    /// `manifest.tsv`, **verbatim**, base64 on the frame.
    ///
    /// Bytes and not a struct, for the reason [`PackManifest`] has no
    /// `Serialize`: the id is the sha256 of these bytes, so a re-serialised
    /// manifest would be a different pack.
    #[serde(with = "crate::bytes_as_base64")]
    pub manifest: Vec<u8>,
    /// `pack.meta`, verbatim, when the pack has one. Not part of the id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::opt_bytes_as_base64"
    )]
    pub meta: Option<Vec<u8>>,
    /// Who holds a complete copy, in no particular order. Empty is
    /// [`PackState::NoHolder`] and is said, not inferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<Holder>,
}

/// An ephemeral bearer authorisation for one transfer (D200, D209).
///
/// **Authorised, not confidential**: no TLS and no PKI on the peer path. The
/// token is random, short-lived ([`PACK_CAPABILITY_LIFETIME`]), scoped to one
/// pack, one peer, one direction and — when the fetcher asked for less than
/// the whole pack — one list of objects. What it buys is that a stranger who
/// finds the endpoint cannot read a pack off it; what it does not claim to
/// buy is secrecy of the bytes in flight on the fleet's own mesh.
///
/// The hub **pushes it to the holder** over the holder's own already
/// authenticated control link ([`ExpectTransfer`]) and returns it to the
/// fetcher ([`TransferCapability`]). A holder therefore never mints one
/// locally and never takes one on a peer's word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchCapability {
    /// Random, minted by the hub, never derived from anything a peer sent.
    pub token: String,
    pub pack: PackId,
    /// The one peer this token is for, by the name the hub knows it as. A
    /// token presented by anybody else is refused by name.
    pub peer: String,
    pub direction: TransferDirection,
    /// The objects it covers. **Empty means the whole pack**, which is the
    /// case for a peer that holds none of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
    /// When it stops being usable, in seconds since the Unix epoch, on the
    /// hub's clock — an instant and not an offset, for the reason
    /// [`crate::Asked::deadline_unix_s`] is one.
    pub expires_unix_s: i64,
}

/// One request on the peer wire: give me this much of this object.
///
/// Not a frame of the control protocol — this is the second wire, between two
/// runners, and the hub never sees one (D10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRequest {
    /// The protocol both peers are joined to the hub at.
    ///
    /// Carried so a mismatch is **refused by name** rather than read as a
    /// malformed request. The peer wire gets no version of its own: see the
    /// module doc.
    pub protocol_version: u32,
    /// The capability the hub minted and pushed to the holder.
    pub token: String,
    pub pack: PackId,
    /// The object wanted, by its sha256 — which is also its name under
    /// `objects/`. An object the pack's manifest does not name is refused by
    /// name, so a token for one pack cannot read another's bytes.
    pub object: String,
    /// Where in the object these bytes start. A resumed transfer is why it is
    /// on the wire, and it is checked rather than trusted, exactly as
    /// [`crate::ToHub::JobFileChunk`]'s is.
    pub offset: u64,
}

/// One reply on the peer wire: these bytes of that object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerReply {
    pub object: String,
    pub offset: u64,
    /// The object's whole length, so a fetcher knows when it is done without
    /// consulting the manifest a second time.
    pub length: u64,
    /// At most [`PACK_CHUNK_BYTES`].
    #[serde(with = "crate::bytes_as_base64")]
    pub data: Vec<u8>,
}

/// Why a peer served nothing, in a sentence.
///
/// A refusal and not a silence, for D150's reason: a fetcher told nothing
/// waits for bytes that will never come, and the party that knows why is the
/// one that has to speak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRefusal {
    pub why: String,
}

/// What a runner holds, announced after [`crate::FromHub::Welcome`] and
/// whenever it changes.
///
/// **An announcement *replaces* the hub's view of this runner's holdings; it
/// never adds to it.** That is the whole of how a holding is retracted, and
/// it has to be settled here rather than discovered later: a runner that
/// deleted a pack — disk pressure, M12 eviction, a [`PinPacks`] that
/// superseded it — says so by announcing what it holds *now*, with the
/// deleted pack simply not in the list. If an announcement appended, nothing
/// in this surface could ever say "I no longer hold that", and the frame
/// that said it would be a new `ToHub` variant, which is the one shape that
/// cannot be added without a fourth protocol version.
///
/// `more` is **required and not optional**, and it is the one *field* in the
/// pack surface that could not be added later. The replace rule is what makes
/// it load-bearing: a holding too large for one frame has to be chunkable
/// from the day the frame is numbered, because without the flag a hub reading
/// the first frame would replace its whole view with a fragment of the
/// truth — and a hub that read the fragment as "and that is all of them"
/// would drop every pack the runner really holds. The announcement is
/// complete, and is applied, when a frame arrives with `more: false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacksHeld {
    pub packs: Vec<PackId>,
    /// `true` when another [`PacksHeld`] follows for the same announcement,
    /// so the hub accumulates and does not apply yet. `false` ends it and the
    /// accumulated list replaces what the hub held for this runner.
    #[serde(default)]
    pub more: bool,
}

/// A runner registers a pack it has just finished writing (E-17).
///
/// Sent **after the fsync**, never before: a pack is complete or absent, and
/// an interrupted registration leaves nothing a later job can be handed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterPack {
    pub job_id: crate::JobId,
    /// Which attempt produced it. A second attempt's pack is not the first's.
    pub attempt: u32,
    /// The commit the pack is of, forty lowercase hex.
    pub commit: String,
    /// The `leaves` key it was left under.
    pub key: String,
    /// `manifest.tsv`, **verbatim**, base64. The hub recomputes the id from
    /// these bytes and refuses a mismatch by name.
    #[serde(with = "crate::bytes_as_base64")]
    pub manifest: Vec<u8>,
    /// `pack.meta`, verbatim, when there is one. Not part of the id, so a
    /// pack with no meta is a pack and not an error.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::opt_bytes_as_base64"
    )]
    pub meta: Option<Vec<u8>>,
}

/// A runner asks for a pack its job needs and was not handed.
///
/// The uncommon path: the common one costs no round trip, because
/// [`AssignedPack`] rode the assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskPack {
    pub job_id: crate::JobId,
    pub commit: String,
    pub key: String,
}

/// A runner asks the hub to authorise a transfer (D209).
///
/// The hub mints the token, pushes it to the holder over the holder's own
/// link ([`ExpectTransfer`]) and answers here
/// ([`crate::FromHub::TransferCapability`]). A runner never mints one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskTransferCapability {
    pub job_id: crate::JobId,
    pub pack: PackId,
    /// The holder it would like to deal with.
    ///
    /// `None` means **the hub names one** — the outbound-only case, where the
    /// asking runner cannot dial anybody and needs a peer that can dial it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    pub direction: TransferDirection,
    /// The objects it lacks. **Empty means the whole pack.**
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
}

/// A transfer finished, with the figures that say which of D10's paths it
/// took and what it cost.
///
/// The measurement M11 owes: D148's own "what is unproven" is that 400 MiB in
/// ~560 objects is ~560 round trips whose per-object cost **is not measured**.
/// A frame that reports objects, bytes and milliseconds is what turns that
/// argument into a figure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackTransferred {
    pub job_id: crate::JobId,
    pub pack: PackId,
    /// The peer the bytes came from or went to. `None` on
    /// [`FetchPath::SameHost`], where there was no peer and no network byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    pub path: FetchPath,
    /// Network bytes. **Zero on [`FetchPath::SameHost`]**, and that zero is
    /// the assertion D10's same-host clause rests on.
    pub bytes: u64,
    /// How many objects moved.
    pub objects_fetched: u32,
    /// How many it already had. The two together are the "fetch only the
    /// diff" claim, stated in figures rather than in prose.
    pub objects_held: u32,
    pub ms: u64,
}

/// A runner cannot get a pack its job needs, and says why.
///
/// The frame that keeps D150's failure from happening again on this half: a
/// job whose `{in}/<key>` is empty must not be a green-looking job with no
/// error anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackUnavailable {
    pub job_id: crate::JobId,
    /// `None` when nothing is registered for the `(commit, key)` at all —
    /// there is no pack to name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackId>,
    pub commit: String,
    pub key: String,
    pub why: String,
}

/// What the hub knows about a `(commit, key)`: the manifest and who holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackHolders {
    pub job_id: crate::JobId,
    pub commit: String,
    pub key: String,
    /// `None` — with `manifest` also absent — means **nothing is registered**
    /// for this `(commit, key)`. Said, rather than answered with an empty
    /// holder list that a runner would have to read as either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::opt_bytes_as_base64"
    )]
    pub manifest: Option<Vec<u8>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::opt_bytes_as_base64"
    )]
    pub meta: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<Holder>,
}

/// The hub authorises the asking peer's transfer and tells it where to go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferCapability {
    pub job_id: crate::JobId,
    pub pack: PackId,
    /// The holder this capability is against.
    pub peer: String,
    /// Where to dial it — the holder's advertised mesh address, which is
    /// never a default or public interface (invariant 3, D11).
    pub endpoint: String,
    pub token: String,
    pub expires_unix_s: i64,
    pub direction: TransferDirection,
    /// Empty means the whole pack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
}

/// The hub tells a **holder** to expect a transfer, over the holder's own
/// already authenticated control link (D209).
///
/// This is the frame that makes the peer wire need no PKI: the holder learns
/// the token from the hub it is already authenticated to, so a token offered
/// by a peer is checked against something the hub said and never against
/// something the peer did.
///
/// It carries no `job_id`, and that absence is deliberate: the holder is not
/// a party to the job that wants the pack, and a job id it cannot do anything
/// with would be the hub telling one runner about another's work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectTransfer {
    pub pack: PackId,
    /// The peer that will present the token.
    pub peer: String,
    pub token: String,
    pub expires_unix_s: i64,
    pub direction: TransferDirection,
    /// Empty means the whole pack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
}

/// The pack is registered; the runner may stop owing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRegistered {
    pub job_id: crate::JobId,
    pub pack: PackId,
    pub commit: String,
    pub key: String,
}

/// The hub will not register or resolve this pack, and says why.
///
/// **Non-terminal, and deliberately not [`crate::FromHub::Refused`]**, which
/// closes the link (D13). The cases are a refusal about one pack and not
/// about this runner: an id that does not match the manifest's bytes, a
/// manifest Herd cannot read, a `(commit, key)` with two different registered
/// ids. The link stays up and the runner's other jobs are untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRefused {
    pub job_id: crate::JobId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackId>,
    pub why: String,
}

/// The hub asks for holdings again, after its own restart.
///
/// Holdings are volatile by design, so a hub that restarted knows no holder
/// and has to ask. The symmetry with [`crate::FromHub::ReadProbesAgain`] is
/// the point: one shape for "I lost what you told me, tell me again", so an
/// operator who knows one knows both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPacksAgain {
    pub why: String,
}

/// The hub asks a keeper to hold on to packs of these keys (M12, E-16).
///
/// Numbered now and unimplemented until M12, because a `FromHub` variant a
/// **runner** sees is free to add later while this one's companions are not,
/// and numbering the set together is what keeps the keeper half from needing
/// its own version. What chooses *which* runner is a keeper is M12's policy
/// and is not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinPacks {
    /// The `leaves` keys to keep.
    pub keys: Vec<String>,
    /// How many packs per key to keep — the newest `keep_per_key` of each.
    pub keep_per_key: u32,
}

/// The one sentence a peer reads when the protocol it announced is not this
/// peer wire's ([`PeerRequest::protocol_version`]).
///
/// Here beside the frames rather than in the runner, for invariant 4's
/// reason: a refusal written once in the serving runner and once in the
/// fetching one would be two sentences about one contract.
pub fn peer_version_mismatch_reason(announced: u32) -> String {
    format!(
        "pack transfer refused: this peer serves protocol version {}, and the asking peer \
         announced version {announced}. The peer wire has no version of its own — both ends are \
         joined to the same hub at the same protocol — so this means one of the two machines has \
         not been rebuilt, and it will be refused at the hub's handshake too",
        crate::PROTOCOL_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest in the consumer's own shape, with **a `d` row and an `l`
    /// row** beside the files.
    ///
    /// The `d` and the `l` are the whole point of the fixture. Both carry a
    /// sha256 of something that is not a file's bytes, so a completeness
    /// check that forgot [`ManifestKind::File`] would look them up under
    /// `objects/`, find nothing, and call a perfectly good pack incomplete. A
    /// fixture of `f` rows alone would let that code pass.
    pub(crate) fn a_manifest() -> String {
        [
            CHATONS_PACK_FORMAT,
            "# profile debug",
            "1111111111111111111111111111111111111111111111111111111111111111\t12\t0755\tf\tbin\tbin/tool",
            "2222222222222222222222222222222222222222222222222222222222222222\t4096\t0755\td\tbin\tbin",
            "3333333333333333333333333333333333333333333333333333333333333333\t7\t0777\tl\tbin\tbin/tool-latest\tbin/tool",
            "4444444444444444444444444444444444444444444444444444444444444444\t900\t0644\tf\tdata\tshare/table.bin",
        ]
        .join("\n")
    }

    #[test]
    fn the_id_is_the_sha256_of_the_bytes_and_of_nothing_else() {
        let bytes = a_manifest().into_bytes();
        let manifest = PackManifest::parse(bytes.clone()).expect("the fixture parses");
        assert_eq!(
            manifest.bytes(),
            bytes.as_slice(),
            "verbatim, or it is a different pack"
        );
        assert_eq!(
            manifest.id(),
            &PackId::of_manifest_bytes(&bytes),
            "the id is computed over the bytes, not over the parse"
        );
        assert_eq!(manifest.id().as_str(), &crate::sha256_hex(&bytes));
    }

    /// One byte changed is a different pack, which is what "verbatim" costs
    /// and why nothing may reprint a manifest.
    #[test]
    fn a_trailing_newline_is_a_different_pack() {
        let one = PackManifest::parse(a_manifest().into_bytes()).expect("parses");
        let other =
            PackManifest::parse(format!("{}\n", a_manifest()).into_bytes()).expect("parses");
        assert_eq!(
            one.entries().len(),
            other.entries().len(),
            "the same rows, so a parse cannot tell them apart"
        );
        assert_ne!(one.id(), other.id(), "and yet two packs, by one byte");
    }

    #[test]
    fn only_the_kind_f_rows_name_objects() {
        let manifest = PackManifest::parse(a_manifest().into_bytes()).expect("parses");
        assert_eq!(
            manifest.entries().len(),
            4,
            "two files, a directory, a link"
        );
        assert_eq!(manifest.files().count(), 2);
        let needed = manifest.objects_needed();
        assert_eq!(
            needed.len(),
            2,
            "a d row and an l row name no object: {needed:?}"
        );
        assert_eq!(needed.get(&"1".repeat(64)), Some(&12));
        assert_eq!(needed.get(&"4".repeat(64)), Some(&900));
        assert!(
            !needed.contains_key(&"2".repeat(64)),
            "the directory's sha256 is not a file's bytes"
        );
        assert!(
            !needed.contains_key(&"3".repeat(64)),
            "the link's sha256 is not a file's bytes"
        );
    }

    #[test]
    fn a_link_row_carries_its_target_and_a_file_row_does_not() {
        let manifest = PackManifest::parse(a_manifest().into_bytes()).expect("parses");
        let link = manifest
            .entries()
            .iter()
            .find(|entry| entry.kind == ManifestKind::Link)
            .expect("the fixture has a link");
        assert_eq!(link.link_target.as_deref(), Some("bin/tool"));
        assert_eq!(
            link.mode, "0777",
            "the mode is carried as written, never parsed"
        );
        let file = manifest.files().next().expect("the fixture has a file");
        assert_eq!(file.link_target, None);
        assert_eq!(file.role, "bin", "carried, never matched against a list");
    }

    #[test]
    fn a_header_this_build_does_not_know_is_refused_naming_both() {
        let said = PackManifest::parse(b"# chatons pack 2\n".to_vec())
            .expect_err("a format Herd was not taught is refused");
        assert!(said.contains("chatons pack 2"), "{said}");
        assert!(said.contains(CHATONS_PACK_FORMAT), "{said}");
    }

    /// A kind letter this build has never heard of is a **refusal**, not a
    /// row read as "not a file".
    #[test]
    fn an_unknown_kind_letter_is_refused_rather_than_dropped() {
        let text = format!(
            "{CHATONS_PACK_FORMAT}\n{}\t1\t0644\tx\trole\tp",
            "5".repeat(64)
        );
        let said = PackManifest::parse(text.into_bytes()).expect_err("x is not a kind");
        assert!(said.contains("\"x\""), "{said}");
        assert!(
            said.contains("completeness"),
            "the refusal has to say what reading it as \"not a file\" would cost: {said}"
        );
    }

    #[test]
    fn a_manifest_past_the_cap_is_refused_whole_and_never_truncated() {
        let mut bytes = a_manifest().into_bytes();
        bytes.resize(MANIFEST_MAX_BYTES as usize + 1, b'\n');
        let said = PackManifest::parse(bytes).expect_err("past the cap");
        assert!(said.contains(&MANIFEST_MAX_BYTES.to_string()), "{said}");
        assert!(said.contains("truncated"), "{said}");
    }

    #[test]
    fn an_announced_id_that_is_not_the_bytes_is_a_disagreement_naming_both() {
        let manifest = PackManifest::parse(a_manifest().into_bytes()).expect("parses");
        assert_eq!(manifest.disagreement_with(manifest.id()), None);
        let said = manifest
            .disagreement_with(&PackId::as_announced("0".repeat(64)))
            .expect("a mismatch is named");
        assert!(said.contains(manifest.id().as_str()), "{said}");
        assert!(said.contains(&"0".repeat(64)), "{said}");
    }

    /// The type's own guarantee, asserted the one way a test can: a debug
    /// print of a manifest is a summary and carries none of its rows, so
    /// nothing read off a `Debug` can be pasted anywhere as a manifest.
    #[test]
    fn debug_prints_a_summary_and_not_the_manifest() {
        let manifest = PackManifest::parse(a_manifest().into_bytes()).expect("parses");
        let shown = format!("{manifest:?}");
        assert!(shown.contains("files: 2"), "{shown}");
        assert!(
            !shown.contains("bin/tool"),
            "no row of the manifest is in it: {shown}"
        );
        assert!(!shown.contains(CHATONS_PACK_FORMAT), "{shown}");
    }

    #[test]
    fn the_chunk_is_derived_from_the_read_buffer_and_not_from_the_frame_ceiling() {
        assert_eq!(PACK_CHUNK_BYTES, 8 * PACK_ENDPOINT_READ_BYTES);
        assert_eq!(PACK_CHUNK_BYTES, 1 << 20);
        assert_ne!(
            PACK_CHUNK_BYTES,
            crate::FRAME_CEILING,
            "the peer leg is not a frame of the control protocol (D10, D148)"
        );
    }

    #[test]
    fn the_peer_refusal_names_both_versions_and_says_the_hub_will_refuse_too() {
        let said = peer_version_mismatch_reason(2);
        assert!(
            said.contains(&crate::PROTOCOL_VERSION.to_string()),
            "{said}"
        );
        assert!(said.contains("announced version 2"), "{said}");
        assert!(said.contains("handshake"), "{said}");
    }
}
