//! Getting a pack a job needs, by the cheapest path available (D10).
//!
//! ## The three paths, in D10's order, and the path taken is reported
//!
//! 1. **Same host.** Every object is already in this runner's store, so
//!    nothing moves on any wire and `{in}/<key>` is hard links into the
//!    store. The claim is about **inodes and not bytes**: a byte count of
//!    nought is also what a failed transfer reports, and only the inode says
//!    the bytes were shared rather than absent.
//! 2. **Peer.** Only the objects this runner **lacks** move, and the figure
//!    is counted off the socket. A second pack sharing objects with the first
//!    transfers only the new ones, which is where "fetch the diff" actually
//!    happens — in the fetcher's own content store, which is why the hub
//!    keeps holdings per manifest and not per blob.
//! 3. **Outbound-only.** A runner with no mesh address fetches exactly like
//!    one with an endpoint: dialling out needs nothing inbound. **It holds no
//!    listener throughout**, which is the assertion that matters, and
//!    `herd-runner/tests/confinement.rs` makes it against the real process.
//!
//! ## What moves, and what does not
//!
//! **Only `objects/<sha256>` ever moves peer to peer.** `manifest.tsv` and
//! `pack.meta` ride the control plane, on the assignment, which is both D10's
//! "metadata is kilobytes" and the reason the hub never takes a pack's
//! identity on a runner's word: it hashes the bytes it received.
//!
//! This module therefore never reprints a manifest. The bytes the hub sent
//! are written to the staging directory verbatim and
//! [`PackManifest::disagreement_with`] is asked whether they are the pack the
//! hub named — because the id **is** the sha256 of those bytes, so a manifest
//! parsed and printed again would be a different pack.
//!
//! ## Killed mid-flight leaves no readable object and no pack
//!
//! Not re-implemented here: [`crate::packstore`] already guarantees it, and
//! this module's whole job is to not defeat it. Fetched bytes land in
//! `packs/incoming/<nonce>/objects/`, under a directory name nothing ever
//! looks up, and [`PackStore::ingest`] is what moves them — hashing the
//! staged copy, fsyncing it, renaming it under its own name, and writing the
//! manifest **last**. So at no instant does `objects/<sha256>` hold part of
//! something, and the pack is absent rather than half-registered.
//!
//! ## A refusal reaches the fetcher as a sentence
//!
//! D150's defect was a frame dropped in silence and a requester told its
//! collection was not there, with no error anywhere. The guard here is that
//! **every path out of [`fetch`] that gets no pack returns an
//! [`Unavailable`] with a sentence in it**, and that every wait is bounded by
//! [`Patience`]: a peer that stops answering is a refusal on a clock, not a
//! fetch that never returns. A peer's own [`PeerRefusal`] is carried through
//! word for word, because the party that knows why is the one that has to
//! speak.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use herd_proto::pack::{
    decode_peer_header, encode_peer_header, take_peer_body, AskTransferCapability, AssignedPack,
    FetchPath, Holder, PackId, PackManifest, PackState, PackTransferred, PeerFramingRefusal,
    PeerHeaderRead, PeerRefusal, PeerReply, PeerRequest, TransferCapability, TransferDirection,
    PACK_ENDPOINT_READ_BYTES,
};
use herd_proto::JobId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::packstore::PackStore;

/// Where a fetch stages the objects it is pulling, under the pack store.
///
/// A fourth directory beside `objects/`, `manifests/` and `refused/`, and
/// **created by this module rather than by [`PackStore`]** — see this file's
/// own note in the handoff. It is under `packs/` deliberately: the staging
/// copy has to be on the same filesystem as `objects/` or
/// [`PackStore::ingest`]'s hard link becomes a second copy of every byte
/// fetched.
const INCOMING_DIR: &str = "incoming";

/// How long a fetch waits for each thing, so that nothing here can wait for
/// ever.
///
/// Injectable for the reason every other bound in this crate is: a test about
/// an unreachable holder must be able to prove the refusal without spending
/// the shipped figure on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    /// Dialling one holder.
    pub dial: Duration,
    /// One read off a peer's stream.
    pub read: Duration,
    /// Asking the hub for a capability.
    pub capability: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            // Short, because a holder that cannot be dialled is a fact the
            // fetcher wants quickly: there may be another holder, and a job
            // is waiting on the answer either way.
            dial: Duration::from_secs(10),
            // Generous, because one read is one chunk of a transfer that may
            // be carrying hundreds of megabytes on somebody's home uplink.
            read: Duration::from_secs(60),
            capability: Duration::from_secs(30),
        }
    }
}

/// What one fetch did, and by which of D10's paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub pack: PackId,
    /// **Reported, not inferred.** D10's claim is not "it arrived" but that
    /// the cheapest available path was the one taken, and a figure that does
    /// not say which path it measured cannot support that.
    pub path: FetchPath,
    /// The peer the objects came from, when any did.
    pub peer: Option<String>,
    /// Objects this runner already held. On the same-host path this is all of
    /// them.
    pub objects_held: u32,
    /// Objects that moved.
    pub objects_fetched: u32,
    /// Body bytes read off the peer's socket. **Nought on the same-host
    /// path**, where it is the inode and not this figure that carries the
    /// claim.
    pub bytes: u64,
    pub ms: u64,
}

impl Fetched {
    /// The frame that tells the hub what this cost (not sent by this module).
    pub fn transferred(&self, job_id: JobId) -> PackTransferred {
        PackTransferred {
            job_id,
            pack: self.pack.clone(),
            peer: self.peer.clone(),
            path: self.path,
            bytes: self.bytes,
            objects_fetched: self.objects_fetched,
            objects_held: self.objects_held,
            ms: self.ms,
        }
    }

    /// The line an operator reads.
    pub fn sentence(&self, key: &str) -> String {
        let by = match self.path {
            FetchPath::SameHost => "already on this host".to_owned(),
            FetchPath::Peer => format!("fetched from {}", self.peer.as_deref().unwrap_or("a peer")),
            FetchPath::ViaPeer => format!(
                "carried by {}",
                self.peer.as_deref().unwrap_or("a peer the hub named")
            ),
        };
        format!(
            "pack {} is laid out for {key}: {by}; {} object(s) already held, {} fetched, {} \
             byte(s) off the network, in {}ms",
            self.pack, self.objects_held, self.objects_fetched, self.bytes, self.ms
        )
    }
}

/// A pack this runner could not get, **named**.
///
/// A named, visible state and never a wait that does not end: that is the
/// whole of what this type is for. [`PackState`] says which of the three the
/// hub would call it; `why` is the sentence, and it always says what was
/// tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    pub pack: Option<PackId>,
    pub state: PackState,
    pub why: String,
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

/// Where a capability comes from: the hub, over this runner's own control
/// link.
///
/// A trait and not a call into the connection loop, so that this module is
/// testable against a hand-written hub and does not wait on the step that
/// wires the frames. The ask and the answer are the **real frames**
/// ([`AskTransferCapability`], [`TransferCapability`]), so that step is an
/// adapter and not a translation.
pub trait Authoriser: Send + Sync {
    fn ask<'a>(
        &'a self,
        ask: AskTransferCapability,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TransferCapability, String>> + Send + 'a>,
    >;
}

/// Get `assigned` into `at`, by the cheapest path that works.
///
/// `at` is `{in}/<key>`, and what lands there is the pack's own three-part
/// shape — `manifest.tsv`, `pack.meta`, `objects/<sha256>` — and not a
/// materialised tree: laying a tree out is the consumer's tool's job
/// (`--pack DIR`), and it is what keeps symlink reproduction, mode
/// restoration and empty-directory preservation out of Herd entirely.
pub async fn fetch(
    store: &PackStore,
    job_id: &JobId,
    assigned: &AssignedPack,
    at: &Path,
    hub: &dyn Authoriser,
    patience: Patience,
) -> Result<Fetched, Unavailable> {
    let began = Instant::now();
    // The bytes the hub sent, read and never reprinted. The id is the sha256
    // of exactly these bytes, so this is also the check that the hub and this
    // runner mean the same pack.
    let manifest = PackManifest::parse(assigned.manifest.clone()).map_err(|why| Unavailable {
        pack: Some(assigned.pack.clone()),
        state: PackState::Incomplete,
        why: format!(
            "pack {} for key {} is not a pack this runner can read: {why}. The manifest came \
             from the hub on the assignment and its bytes are the pack's identity, so there is \
             nothing here to retry",
            assigned.pack, assigned.key
        ),
    })?;
    if let Some(disagreement) = manifest.disagreement_with(&assigned.pack) {
        return Err(Unavailable {
            pack: Some(assigned.pack.clone()),
            state: PackState::Incomplete,
            why: format!(
                "the manifest the hub sent for key {} is not the pack the hub named it. \
                 {disagreement}. A pack's id is the sha256 of its manifest's bytes, so these \
                 two cannot both be right and this runner guesses at neither",
                assigned.key
            ),
        });
    }
    let missing = store.missing(&manifest).map_err(|why| Unavailable {
        pack: Some(manifest.id().clone()),
        state: PackState::Incomplete,
        why: format!(
            "this runner cannot tell which of pack {}'s objects it holds: {why}",
            manifest.id()
        ),
    })?;
    let needed = manifest.objects_needed().len();
    let objects_fetched = u32::try_from(missing.len()).unwrap_or(u32::MAX);
    let objects_held = u32::try_from(needed.saturating_sub(missing.len())).unwrap_or(u32::MAX);

    // The staging area, removed on every path out of here including the
    // failing ones: see `Incoming`.
    let incoming = Incoming::under(store, manifest.id()).map_err(|why| Unavailable {
        pack: Some(manifest.id().clone()),
        state: PackState::Incomplete,
        why: format!(
            "this runner has nowhere to stage pack {}: {why}",
            manifest.id()
        ),
    })?;
    // Verbatim, both of them. Nothing in Herd prints a manifest.
    incoming
        .write("manifest.tsv", manifest.bytes())
        .and_then(|()| match &assigned.meta {
            Some(meta) => incoming.write("pack.meta", meta),
            None => Ok(()),
        })
        .map_err(|why| Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::Incomplete,
            why: format!("pack {} cannot be staged: {why}", manifest.id()),
        })?;

    let (path, peer, bytes) = if missing.is_empty() {
        // Path 1. Nothing is asked of the hub and nothing is dialled: a
        // runner that already holds every object lays the pack out without
        // saying a word to anybody, which is the case the manifest rides the
        // assignment for.
        (FetchPath::SameHost, None, 0u64)
    } else {
        let wanted: Vec<String> = missing.keys().cloned().collect();
        let holder = reachable_holder(&assigned.holders, &manifest, &wanted)?;
        let capability = tokio::time::timeout(
            patience.capability,
            hub.ask(AskTransferCapability {
                job_id: job_id.clone(),
                pack: manifest.id().clone(),
                peer: Some(holder.runner.clone()),
                direction: TransferDirection::Fetch,
                objects: wanted.clone(),
            }),
        )
        .await
        .map_err(|_| Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::Incomplete,
            why: format!(
                "the hub did not answer in {:?} when asked for a capability to fetch {} \
                 object(s) of pack {} from {}. A transfer is authorised by the hub and by \
                 nothing else (D209), so this runner does not dial the holder on its own \
                 authority",
                patience.capability,
                wanted.len(),
                manifest.id(),
                holder.runner
            ),
        })?
        .map_err(|why| Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::Incomplete,
            why: format!(
                "the hub will not authorise a fetch of pack {} from {}: {why}",
                manifest.id(),
                holder.runner
            ),
        })?;
        let bytes = pull(&incoming, &capability, &wanted, &missing, patience).await?;
        (FetchPath::Peer, Some(holder.runner.clone()), bytes)
    };

    // One ingest for both paths, and it is `packstore`'s and not this
    // module's: the hash of the staged copy, the fsync, the rename under the
    // object's own name, and the manifest written last. An object already in
    // the store is skipped there, so the same-host path costs one read of the
    // manifest and nothing else.
    let held = store.ingest(incoming.path()).map_err(|why| Unavailable {
        pack: Some(manifest.id().clone()),
        state: PackState::Incomplete,
        why: format!(
            "pack {} did not go into this runner's store and is therefore not registered as \
             held: {why}",
            manifest.id()
        ),
    })?;
    drop(incoming);
    store
        .lay_out(&held.manifest, at)
        .map_err(|why| Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::Incomplete,
            why: format!(
                "pack {} is in this runner's store and could not be laid out at {} for key {}: \
             {why}",
                manifest.id(),
                at.display(),
                assigned.key
            ),
        })?;
    Ok(Fetched {
        pack: manifest.id().clone(),
        path,
        peer,
        objects_held,
        objects_fetched,
        bytes,
        ms: u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// A holder this runner can dial, or the named state that says why not.
///
/// The three cases are distinct and each is a sentence, because "the pack did
/// not arrive" is the one answer that helps nobody.
fn reachable_holder<'a>(
    holders: &'a [Holder],
    manifest: &PackManifest,
    wanted: &[String],
) -> Result<&'a Holder, Unavailable> {
    if holders.is_empty() {
        return Err(Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::NoHolder,
            why: format!(
                "pack {} is registered and no runner the hub knows of holds a complete copy, so \
                 {} object(s) this job needs exist nowhere this fleet can reach. This is a \
                 state and not a wait: nothing here will become true by being asked again",
                manifest.id(),
                wanted.len()
            ),
        });
    }
    holders
        .iter()
        .find(|holder| holder.endpoint.is_some())
        .ok_or_else(|| Unavailable {
            pack: Some(manifest.id().clone()),
            state: PackState::NoHolder,
            why: format!(
                "every one of the {} runner(s) holding pack {} is outbound-only — it listens on \
                 nothing, because its host has no address invariant 3 permits it to serve from \
                 — so none of them can be dialled, and the bytes have to be **pushed** to this \
                 runner by one of them on a connection it opens. This build does not push: it \
                 is a named state and not a silence, and the push is the step after this one",
                holders.len(),
                manifest.id()
            ),
        })
}

/// Pull every wanted object off one peer, on one connection, into the staging
/// directory.
///
/// One connection for all of them: the consumer's measured pack is 557 files,
/// and a dial per object would be 557 dials for one transfer.
async fn pull(
    incoming: &Incoming,
    capability: &TransferCapability,
    wanted: &[String],
    sizes: &std::collections::BTreeMap<String, u64>,
    patience: Patience,
) -> Result<u64, Unavailable> {
    let refuse = |why: String| Unavailable {
        pack: Some(capability.pack.clone()),
        state: PackState::Incomplete,
        why,
    };
    let mut stream = tokio::time::timeout(
        patience.dial,
        tokio::net::TcpStream::connect(&capability.endpoint),
    )
    .await
    .map_err(|_| {
        refuse(format!(
            "{} did not answer in {:?}, so pack {}'s {} missing object(s) could not be fetched \
             from {}. A holder that cannot be dialled is a fact about this minute and is said \
             rather than waited on",
            capability.endpoint,
            patience.dial,
            capability.pack,
            wanted.len(),
            capability.peer
        ))
    })?
    .map_err(|error| {
        refuse(format!(
            "{} cannot be dialled: {error}. Pack {}'s {} missing object(s) are held by {}, \
             which this runner cannot reach",
            capability.endpoint,
            capability.pack,
            wanted.len(),
            capability.peer
        ))
    })?;

    let mut buffer: Vec<u8> = Vec::new();
    let mut read = vec![0u8; PACK_ENDPOINT_READ_BYTES as usize];
    let mut bytes = 0u64;
    for object in wanted {
        let size = *sizes.get(object).unwrap_or(&0);
        let (line, _) = encode_peer_header(&PeerRequest {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            token: capability.token.clone(),
            pack: capability.pack.clone(),
            object: object.clone(),
            offset: 0,
        })
        .map_err(|why| {
            refuse(format!(
                "a request for object {object} will not encode: {why}"
            ))
        })?;
        stream.write_all(&line).await.map_err(|error| {
            refuse(format!(
                "the request for object {object} could not be sent to {}: {error}",
                capability.peer
            ))
        })?;
        let mut at = 0u64;
        while at < size {
            // One whole header line, then exactly as many raw bytes as it
            // promised. A short read of a chunk is a refusal and not a short
            // object: an object is its sha256.
            let header = loop {
                match decode_peer_header::<PeerReply>(&buffer) {
                    Ok(PeerHeaderRead::Read(header)) => break header,
                    Ok(PeerHeaderRead::Incomplete) => {}
                    Err(PeerFramingRefusal::HeaderUnreadable { error }) => {
                        // It may be the other header this leg carries, which
                        // is the whole of how a refusal reaches a fetcher as
                        // a sentence rather than as an absence (D150).
                        if let Ok(PeerHeaderRead::Read(said)) =
                            decode_peer_header::<PeerRefusal>(&buffer)
                        {
                            return Err(refuse(format!(
                                "{} refused to serve object {object} of pack {}: {}",
                                capability.peer, capability.pack, said.header.why
                            )));
                        }
                        return Err(refuse(format!(
                            "{} answered with something that is neither a reply nor a refusal: \
                             {error}",
                            capability.peer
                        )));
                    }
                    Err(why) => {
                        return Err(refuse(format!(
                            "{}'s reply about object {object} is refused: {why}",
                            capability.peer
                        )))
                    }
                }
                let got = tokio::time::timeout(patience.read, stream.read(&mut read))
                    .await
                    .map_err(|_| {
                        refuse(format!(
                            "{} sent nothing for {:?} while object {object} of pack {} was \
                             {at} byte(s) in of {size}. A transfer that stopped is said rather \
                             than waited on, and it resumes at the last whole chunk",
                            capability.peer, patience.read, capability.pack
                        ))
                    })?
                    .map_err(|error| {
                        refuse(format!(
                            "reading object {object} from {} failed at {at} of {size} byte(s): \
                             {error}",
                            capability.peer
                        ))
                    })?;
                if got == 0 {
                    return Err(refuse(format!(
                        "{} closed the connection with object {object} of pack {} {at} byte(s) \
                         in of {size}. What arrived is discarded rather than written: a short \
                         body is never a short object",
                        capability.peer, capability.pack
                    )));
                }
                buffer.extend_from_slice(&read[..got]);
            };
            if header.header.object != *object || header.header.offset != at {
                return Err(refuse(format!(
                    "{} answered about object {} at {} when this runner asked for object \
                     {object} at {at}: the two ends have lost count of the stream, so nothing \
                     later on it is written",
                    capability.peer, header.header.object, header.header.offset
                )));
            }
            if header.header.object_length != size {
                return Err(refuse(format!(
                    "{} says object {object} is {} byte(s) and pack {}'s manifest says {size}. \
                     Two different byte strings do not share a sha256, so one of those figures \
                     is wrong and neither is written",
                    capability.peer, header.header.object_length, capability.pack
                )));
            }
            // Enough body for this header, or read again.
            let body = loop {
                match take_peer_body(&buffer[header.body_at..], header.body_bytes) {
                    Ok((body, _)) => break body.to_vec(),
                    Err(PeerFramingRefusal::BodyTruncated { .. }) => {}
                    Err(why) => {
                        return Err(refuse(format!(
                            "{}'s chunk of object {object} is refused: {why}",
                            capability.peer
                        )))
                    }
                }
                let got = tokio::time::timeout(patience.read, stream.read(&mut read))
                    .await
                    .map_err(|_| {
                        refuse(format!(
                            "{} sent nothing for {:?} in the middle of a chunk of object \
                             {object}",
                            capability.peer, patience.read
                        ))
                    })?
                    .map_err(|error| {
                        refuse(format!(
                            "reading object {object} from {} failed: {error}",
                            capability.peer
                        ))
                    })?;
                if got == 0 {
                    return Err(refuse(format!(
                        "{} closed the connection in the middle of a chunk of object {object}: \
                         what arrived is discarded rather than written",
                        capability.peer
                    )));
                }
                buffer.extend_from_slice(&read[..got]);
            };
            incoming
                .append(object, &body)
                .map_err(|why| refuse(format!("object {object} cannot be staged: {why}")))?;
            let consumed = header.body_at + body.len();
            buffer.drain(..consumed);
            at += body.len() as u64;
            bytes += body.len() as u64;
        }
    }
    Ok(bytes)
}

/// The staging directory one fetch pulls into, removed whenever this value
/// goes out of scope.
///
/// Removed on **every** path out of [`fetch`], the failing ones included: a
/// fetch that was refused halfway has no business leaving a few hundred
/// megabytes of a pack nobody will ever ask for on its owner's disk. A
/// process *killed* mid-fetch does leave one, which is what
/// [`forget_incoming`] is for.
struct Incoming {
    path: PathBuf,
}

impl Incoming {
    fn under(store: &PackStore, id: &PackId) -> Result<Self, String> {
        // The id plus the clock: two attempts of two jobs may be fetching the
        // same pack at the same moment, and they must not stage into one
        // directory.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        let path = store.path().join(INCOMING_DIR).join(format!(
            "{}-{nonce}-{}",
            id.as_str(),
            std::process::id()
        ));
        std::fs::create_dir_all(path.join("objects"))
            .map_err(|error| format!("{} cannot be made: {error}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let at = self.path.join(name);
        std::fs::write(&at, bytes)
            .map_err(|error| format!("{} cannot be written: {error}", at.display()))
    }

    /// Append one chunk to a staged object.
    ///
    /// `object` is a digest out of a parsed [`herd_proto::pack::ManifestEntry`],
    /// which `herd_proto::pack::check_object_digest` has already refused
    /// unless it is 64 lowercase hex — so it is a name and not a path, and
    /// there is deliberately no second check of it here.
    fn append(&self, object: &str, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let at = self.path.join("objects").join(object);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&at)
            .map_err(|error| format!("{} cannot be opened: {error}", at.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("{} cannot be written: {error}", at.display()))
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Remove whatever a previous life of this runner left staging, and say what
/// went.
///
/// A fetch removes its own staging directory on every path out of itself, so
/// what this finds is only ever the work of a process that was **killed**
/// mid-transfer. Called at startup beside the other two things a previous
/// life leaves behind, and for the same reason: bytes nobody will ever ask
/// for, on the owner's disk, that only the next start can see.
pub fn forget_incoming(store: &PackStore) -> Option<String> {
    let at = store.path().join(INCOMING_DIR);
    let mut staged = 0usize;
    let mut bytes = 0u64;
    let entries = std::fs::read_dir(&at).ok()?;
    for entry in entries.flatten() {
        staged += 1;
        bytes += weigh(&entry.path());
    }
    if staged == 0 {
        return None;
    }
    match std::fs::remove_dir_all(&at) {
        Ok(()) => Some(format!(
            "{staged} half-fetched pack(s) were left staging at {} by a previous life of this \
             runner, holding {bytes} byte(s); they are removed, because a staged object is not \
             an object — it is under no name anything looks up, and the pack it belongs to was \
             never registered",
            at.display()
        )),
        Err(error) => Some(format!(
            "{staged} half-fetched pack(s) holding {bytes} byte(s) are at {} and could not be \
             removed: {error}. Nothing reads them and nothing will; removing them is yours to do",
            at.display()
        )),
    }
}

fn weigh(at: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(at) else {
        return std::fs::symlink_metadata(at)
            .map(|it| it.len())
            .unwrap_or(0);
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => weigh(&entry.path()),
            _ => entry.metadata().map(|it| it.len()).unwrap_or(0),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::{self, Granted};
    use crate::mesh::{HostRouting, Mesh};
    use crate::testing::TempDir;
    use herd_proto::pack::ExpectTransfer;
    use std::os::unix::fs::MetadataExt;
    use std::sync::Arc;

    /// A hand-written hub: it mints a capability and pushes it to the holder
    /// exactly as D209 says the real one does, and returns it to the fetcher.
    ///
    /// The *shape* is the contract, not the mechanism: the ask and the answer
    /// are the real frames, so the step that wires this to a control link is
    /// an adapter.
    struct HandWrittenHub {
        endpoint: String,
        holder: String,
        /// The holder's own table — written here, which is what makes the
        /// pushed-by-the-hub half real in this test rather than assumed.
        granted: Granted,
        lifetime: i64,
    }

    impl Authoriser for HandWrittenHub {
        fn ask<'a>(
            &'a self,
            ask: AskTransferCapability,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<TransferCapability, String>> + Send + 'a>,
        > {
            Box::pin(async move {
                let token = format!("minted-{}-{}", ask.pack.as_str(), ask.objects.len());
                let expires_unix_s = endpoint::now_unix_s() + self.lifetime;
                // Pushed to the holder first, over the holder's own link.
                self.granted.pushed_by_hub(&ExpectTransfer {
                    pack: ask.pack.clone(),
                    peer: "the-fetcher".to_owned(),
                    token: token.clone(),
                    expires_unix_s,
                    direction: ask.direction,
                    objects: ask.objects.clone(),
                });
                Ok(TransferCapability {
                    job_id: ask.job_id,
                    pack: ask.pack,
                    peer: self.holder.clone(),
                    endpoint: self.endpoint.clone(),
                    token,
                    expires_unix_s,
                    direction: ask.direction,
                    objects: ask.objects,
                })
            })
        }
    }

    /// A hub that authorises nothing, so that the fetcher's own refusal is
    /// the thing under test.
    struct NoHub(String);

    impl Authoriser for NoHub {
        fn ask<'a>(
            &'a self,
            _ask: AskTransferCapability,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<TransferCapability, String>> + Send + 'a>,
        > {
            Box::pin(async move { Err(self.0.clone()) })
        }
    }

    /// Build a pack directory, ingest it into `store`, and give back what the
    /// hub would put on the assignment.
    ///
    /// The fixture always carries a `kind d` row and a `kind l` row, so that
    /// nothing in a fetch can pass by looking every digest in the manifest up
    /// under `objects/`.
    fn a_pack(
        store: &PackStore,
        at: &TempDir,
        label: &str,
        files: &[(&str, &[u8])],
    ) -> AssignedPack {
        let dir = at.path().join(format!("staging-{label}"));
        let objects = dir.join("objects");
        std::fs::create_dir_all(&objects).expect("a pack directory");
        let mut rows = String::from(herd_proto::CHATONS_PACK_FORMAT);
        rows.push('\n');
        rows.push_str(&format!("# profile {label}\n"));
        for (name, bytes) in files {
            let sha = herd_proto::sha256_hex(bytes);
            std::fs::write(objects.join(&sha), bytes).expect("an object");
            rows.push_str(&format!(
                "{sha}\t{}\t0644\tf\tartefact\t{name}\n",
                bytes.len()
            ));
        }
        rows.push_str(&format!(
            "{}\t0\t0755\td\tartefact\tbin\n",
            herd_proto::sha256_hex(b"a directory is not a file's bytes")
        ));
        rows.push_str(&format!(
            "{}\t0\t0777\tl\tartefact\tbin/link\t/nix/store/elsewhere\n",
            herd_proto::sha256_hex(b"a symlink is not a file's bytes")
        ));
        let meta = format!("commit = {label}\n");
        std::fs::write(dir.join("manifest.tsv"), rows.as_bytes()).expect("a manifest");
        std::fs::write(dir.join("pack.meta"), meta.as_bytes()).expect("a meta");
        let held = store.ingest(&dir).expect("the pack ingests");
        std::fs::remove_dir_all(&dir).expect("the staging pack goes");
        AssignedPack {
            key: "pack".to_owned(),
            pack: held.manifest.id().clone(),
            manifest: held.manifest.bytes().to_vec(),
            meta: Some(meta.into_bytes()),
            holders: Vec::new(),
        }
    }

    /// The same, without ingesting: what a runner that does **not** hold the
    /// pack is handed on its assignment.
    fn as_assigned_elsewhere(assigned: &AssignedPack) -> AssignedPack {
        AssignedPack {
            holders: Vec::new(),
            ..assigned.clone()
        }
    }

    fn quick() -> Patience {
        Patience {
            dial: Duration::from_millis(300),
            read: Duration::from_secs(5),
            capability: Duration::from_secs(5),
        }
    }

    /// Everything the endpoint said, so a test can assert the holder's own
    /// account of what it served.
    type Heard = Arc<std::sync::Mutex<Vec<endpoint::Said>>>;

    /// What one holder's endpoint served, out of its own sentences.
    ///
    /// The [`endpoint::Report`] cannot be used for this: a connection the
    /// fetcher has finished with is still open until the holder's side reads
    /// the end of it, so a test that stopped the endpoint the moment its
    /// fetch returned would be racing the holder's `read` — and the figures
    /// of a connection still in flight are deliberately not in the report.
    /// The sentences are emitted as each object finishes, which is the event
    /// the assertion is actually about.
    fn served(heard: &Heard) -> (u64, u64) {
        heard
            .lock()
            .expect("a lock")
            .iter()
            .filter_map(|said| match said {
                endpoint::Said::Served { bytes, .. } => Some(*bytes),
                _ => None,
            })
            .fold((0, 0), |(objects, total), bytes| {
                (objects + 1, total + bytes)
            })
    }

    /// How many bytes **of objects** are staged under a store's `incoming/`.
    ///
    /// Only the `objects/` subtrees, because the manifest is written there
    /// before a single object byte arrives: a measure that counted it would
    /// read as "the transfer has begun" the instant the staging directory
    /// existed, and the cut below would then not be mid-object at all.
    fn staged_bytes(staging: &Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(staging) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| weigh(&entry.path().join("objects")))
            .sum()
    }

    /// Wait until the holder says it has served `objects` objects.
    async fn until_served(heard: &Heard, objects: u64) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while served(heard).0 < objects {
            assert!(
                Instant::now() < deadline,
                "the holder served {:?} and not {objects} object(s)",
                served(heard)
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Start a real endpoint and give back its address, its sentences, and
    /// the handles to stop it with.
    async fn an_endpoint(
        store: PackStore,
        granted: Granted,
    ) -> (
        String,
        Heard,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<endpoint::Report>,
    ) {
        let mesh = Mesh::resolve("127.0.0.1:0", None, &HostRouting::read())
            .expect("loopback is permitted");
        let heard = Arc::new(std::sync::Mutex::new(Vec::<endpoint::Said>::new()));
        let kept = Arc::clone(&heard);
        let (stops, stop) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(endpoint::serve_while(
            Some(mesh),
            store,
            granted,
            endpoint::Advertised::new(),
            || true,
            Duration::from_millis(5),
            Arc::new(move |said| kept.lock().expect("a lock").push(said)),
            async move {
                let _ = stop.await;
            },
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        let at = loop {
            assert!(Instant::now() < deadline, "the endpoint never bound");
            let found = heard
                .lock()
                .expect("a lock")
                .iter()
                .find_map(|said| match said {
                    endpoint::Said::Listening { at, .. } => Some(*at),
                    _ => None,
                });
            if let Some(at) = found {
                break at;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        (at.to_string(), heard, stops, serving)
    }

    /// Path 1: already on this host. **Zero network bytes and one inode**, and
    /// the inode is the assertion — a byte count of nought is also what a
    /// failed transfer reports.
    #[tokio::test]
    async fn the_same_host_path_shares_one_inode_and_moves_nothing() {
        let temp = TempDir::new("fetch-same-host");
        let store = PackStore::open(temp.path()).expect("a store");
        let assigned = a_pack(&store, &temp, "build", &[("bin/tool", b"the tool's bytes")]);
        let into = temp.path().join("in").join("pack");
        let got = fetch(
            &store,
            &JobId::new("j1"),
            &assigned,
            &into,
            &NoHub("the hub is never asked on this path".to_owned()),
            quick(),
        )
        .await
        .expect("a pack already held is laid out");
        assert_eq!(got.path, FetchPath::SameHost, "{got:?}");
        assert_eq!(got.bytes, 0, "a byte moved on the same-host path: {got:?}");
        assert_eq!(got.objects_fetched, 0, "{got:?}");
        assert_eq!(got.objects_held, 1, "{got:?}");
        assert!(got.peer.is_none(), "{got:?}");

        // The claim: one inode, shared between the store and `{in}`.
        let sha = herd_proto::sha256_hex(b"the tool's bytes");
        let in_store = std::fs::metadata(store.objects_dir().join(&sha)).expect("the store's copy");
        let in_job = std::fs::metadata(into.join("objects").join(&sha)).expect("the job's copy");
        assert_eq!(
            in_store.ino(),
            in_job.ino(),
            "the job's copy is a second copy of the bytes, not a link to the store's"
        );
        assert!(
            in_store.nlink() >= 2,
            "the store's object has {} link(s), so nothing was shared",
            in_store.nlink()
        );
        // And the manifest at `{in}` is the bytes the id was computed over,
        // not a reprint.
        assert_eq!(
            std::fs::read(into.join("manifest.tsv")).expect("the manifest is laid out"),
            assigned.manifest,
            "the manifest laid out for the job is not the bytes the pack's id was computed over"
        );
        assert_eq!(
            std::fs::read(into.join("pack.meta")).expect("the meta is laid out"),
            assigned.meta.clone().expect("the fixture has a meta")
        );
    }

    /// Path 2: only the objects this runner lacks move, counted — and a
    /// second pack sharing objects with the first transfers only the new
    /// ones.
    #[tokio::test]
    async fn only_the_objects_this_runner_lacks_move() {
        let holder_at = TempDir::new("fetch-holder");
        let fetcher_at = TempDir::new("fetch-fetcher");
        let holder = PackStore::open(holder_at.path()).expect("the holder's store");
        let fetcher = PackStore::open(fetcher_at.path()).expect("the fetcher's store");

        let shared: &[u8] = b"a library both packs carry, and it is not small at all";
        let first_only: &[u8] = b"the first pack's own object";
        let second_only: &[u8] = b"the second pack's own object, which is a different length";
        let one = a_pack(
            &holder,
            &holder_at,
            "one",
            &[("lib/shared.a", shared), ("bin/one", first_only)],
        );
        let two = a_pack(
            &holder,
            &holder_at,
            "two",
            &[("lib/shared.a", shared), ("bin/two", second_only)],
        );

        let granted = Granted::new();
        let (at, heard, stops, serving) = an_endpoint(holder.clone(), granted.clone()).await;
        let hub = HandWrittenHub {
            endpoint: at,
            holder: "the-holder".to_owned(),
            granted,
            lifetime: 300,
        };
        let holders = vec![Holder {
            runner: "the-holder".to_owned(),
            endpoint: Some(hub.endpoint.clone()),
        }];

        // The first pack: both objects move, and the figure is their bytes.
        let first = AssignedPack {
            holders: holders.clone(),
            ..as_assigned_elsewhere(&one)
        };
        let got = fetch(
            &fetcher,
            &JobId::new("j1"),
            &first,
            &fetcher_at.path().join("in-one"),
            &hub,
            quick(),
        )
        .await
        .expect("the first pack is fetched");
        assert_eq!(got.path, FetchPath::Peer, "{got:?}");
        assert_eq!(got.peer.as_deref(), Some("the-holder"), "{got:?}");
        assert_eq!(got.objects_fetched, 2, "{got:?}");
        assert_eq!(got.objects_held, 0, "{got:?}");
        assert_eq!(
            got.bytes,
            (shared.len() + first_only.len()) as u64,
            "the first transfer moved {} bytes and the two objects are {} + {}",
            got.bytes,
            shared.len(),
            first_only.len()
        );

        // The second pack shares one object with the first. Only the new one
        // moves, and the figure says so exactly.
        let second = AssignedPack {
            holders: holders.clone(),
            ..as_assigned_elsewhere(&two)
        };
        let got = fetch(
            &fetcher,
            &JobId::new("j2"),
            &second,
            &fetcher_at.path().join("in-two"),
            &hub,
            quick(),
        )
        .await
        .expect("the second pack is fetched");
        assert_eq!(got.path, FetchPath::Peer, "{got:?}");
        assert_eq!(
            got.objects_held, 1,
            "the shared object was fetched again: {got:?}"
        );
        assert_eq!(got.objects_fetched, 1, "{got:?}");
        assert_eq!(
            got.bytes,
            second_only.len() as u64,
            "the second transfer moved {} bytes and only {} were new",
            got.bytes,
            second_only.len()
        );

        // And the third fetch of the first pack is the same-host path, which
        // is the whole of why the diff is computed in the fetcher's store.
        let got = fetch(
            &fetcher,
            &JobId::new("j3"),
            &first,
            &fetcher_at.path().join("in-again"),
            &hub,
            quick(),
        )
        .await
        .expect("a pack already held needs no peer");
        assert_eq!(got.path, FetchPath::SameHost, "{got:?}");
        assert_eq!(got.bytes, 0, "{got:?}");

        // The holder's own account, out of its own sentences: three objects
        // served for two transfers of three missing objects, and the byte
        // total is the fetcher's figures added up. Two independent counts of
        // one transfer agreeing is what makes either of them worth quoting.
        until_served(&heard, 3).await;
        assert_eq!(
            served(&heard),
            (
                3,
                (shared.len() + first_only.len() + second_only.len()) as u64
            ),
            "the holder's account of what it served: {:?}",
            heard.lock().expect("a lock")
        );
        let _ = stops.send(());
        let report = serving.await.expect("the endpoint comes back");
        assert_eq!(report.refused, 0, "{report:?}");
        assert_eq!(
            report.connections, 2,
            "two peer transfers took {} connection(s); one connection carries every object of \
             one transfer, because 557 dials for one pack is what that costs: {report:?}",
            report.connections
        );
    }

    /// Path 3's half that this build has: a fetcher with **no mesh address**
    /// completes a peer fetch and holds no listener while doing it.
    ///
    /// The listener half is asserted here as the endpoint loop's own count of
    /// sockets opened, which is nought; against the real process it is
    /// `herd-runner/tests/confinement.rs`'s first property, where a runner
    /// with no `--mesh-listen` is walked in `/proc/net/tcp` and holds
    /// nothing.
    #[tokio::test]
    async fn an_outbound_only_runner_fetches_and_holds_no_listener() {
        let holder_at = TempDir::new("fetch-outbound-holder");
        let fetcher_at = TempDir::new("fetch-outbound");
        let holder = PackStore::open(holder_at.path()).expect("the holder's store");
        let fetcher = PackStore::open(fetcher_at.path()).expect("the fetcher's store");
        let pack = a_pack(
            &holder,
            &holder_at,
            "out",
            &[("bin/tool", b"bytes to pull")],
        );

        let granted = Granted::new();
        let (at, _heard, stops, serving) = an_endpoint(holder.clone(), granted.clone()).await;
        let hub = HandWrittenHub {
            endpoint: at.clone(),
            holder: "the-holder".to_owned(),
            granted,
            lifetime: 300,
        };

        // The fetcher's own endpoint: configured with no mesh address at all,
        // which is what outbound-only is.
        let (fetcher_stops, fetcher_stop) = tokio::sync::oneshot::channel::<()>();
        let mine = tokio::spawn(endpoint::serve_while(
            None,
            fetcher.clone(),
            Granted::new(),
            endpoint::Advertised::new(),
            || true,
            Duration::from_millis(5),
            endpoint::silent(),
            async move {
                let _ = fetcher_stop.await;
            },
        ));

        let assigned = AssignedPack {
            holders: vec![Holder {
                runner: "the-holder".to_owned(),
                endpoint: Some(at),
            }],
            ..as_assigned_elsewhere(&pack)
        };
        let got = fetch(
            &fetcher,
            &JobId::new("j1"),
            &assigned,
            &fetcher_at.path().join("in"),
            &hub,
            quick(),
        )
        .await
        .expect("a runner that listens on nothing can still pull");
        assert_eq!(got.path, FetchPath::Peer, "{got:?}");
        assert_eq!(got.bytes, b"bytes to pull".len() as u64, "{got:?}");

        let _ = fetcher_stops.send(());
        let ours = mine.await.expect("the fetcher's endpoint comes back");
        assert_eq!(
            ours.opened, 0,
            "an outbound-only runner opened a socket: {ours:?}"
        );
        assert_eq!(ours.connections, 0, "{ours:?}");
        let _ = stops.send(());
        let _ = serving.await;
    }

    /// A pack whose only holder cannot be reached is a **named, visible
    /// state** and not a wait that never ends — in all three of its shapes.
    #[tokio::test]
    async fn a_pack_that_cannot_be_had_is_named_and_bounded() {
        let temp = TempDir::new("fetch-unreachable");
        let holder_at = TempDir::new("fetch-unreachable-holder");
        let holder = PackStore::open(holder_at.path()).expect("the holder's store");
        let fetcher = PackStore::open(temp.path()).expect("the fetcher's store");
        let pack = a_pack(&holder, &holder_at, "gone", &[("bin/tool", b"bytes")]);
        let hub = NoHub("no runner this hub knows of holds it".to_owned());
        let into = temp.path().join("in");

        // 1. No holder at all.
        let began = Instant::now();
        let why = fetch(
            &fetcher,
            &JobId::new("j1"),
            &as_assigned_elsewhere(&pack),
            &into,
            &hub,
            quick(),
        )
        .await
        .expect_err("a pack nobody holds is not fetched");
        assert_eq!(why.state, PackState::NoHolder, "{why}");
        assert!(
            why.why.contains("no runner the hub knows of holds")
                && why.why.contains("a state and not a wait"),
            "{why}"
        );

        // 2. Every holder outbound-only: the push path, which this build does
        // not have, said rather than waited on.
        let why = fetch(
            &fetcher,
            &JobId::new("j2"),
            &AssignedPack {
                holders: vec![Holder {
                    runner: "a-runner-behind-a-router".to_owned(),
                    endpoint: None,
                }],
                ..as_assigned_elsewhere(&pack)
            },
            &into,
            &hub,
            quick(),
        )
        .await
        .expect_err("an outbound-only holder cannot be dialled");
        assert_eq!(why.state, PackState::NoHolder, "{why}");
        assert!(
            why.why.contains("outbound-only") && why.why.contains("pushed"),
            "{why}"
        );

        // 3. A holder with an address that answers nothing.
        let why = fetch(
            &fetcher,
            &JobId::new("j3"),
            &AssignedPack {
                holders: vec![Holder {
                    runner: "a-runner-that-died".to_owned(),
                    endpoint: Some("127.0.0.1:1".to_owned()),
                }],
                ..as_assigned_elsewhere(&pack)
            },
            &into,
            &hub,
            quick(),
        )
        .await
        .expect_err("a hub that authorises nothing fetches nothing");
        assert!(
            why.why.contains("will not authorise")
                && why.why.contains("no runner this hub knows of holds it"),
            "{why}"
        );
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "three refusals took {:?}; every one of them is meant to be bounded",
            began.elapsed()
        );
        // Nothing of any of the three is left on the disk.
        assert!(
            !fetcher.holds(&pack.pack),
            "a pack that was never fetched is registered"
        );
        assert_eq!(
            std::fs::read_dir(fetcher.objects_dir())
                .expect("the objects directory")
                .count(),
            0,
            "a refused fetch left objects behind"
        );
    }

    /// A peer's refusal reaches the fetcher **as a sentence**, which is the
    /// half of D150 this module owes.
    #[tokio::test]
    async fn a_peer_refusal_reaches_the_fetcher_word_for_word() {
        let holder_at = TempDir::new("fetch-refused-holder");
        let fetcher_at = TempDir::new("fetch-refused");
        let holder = PackStore::open(holder_at.path()).expect("the holder's store");
        let fetcher = PackStore::open(fetcher_at.path()).expect("the fetcher's store");
        let pack = a_pack(&holder, &holder_at, "refused", &[("bin/tool", b"bytes")]);

        // The endpoint's table stays empty: the hub below mints a capability
        // for the fetcher and **does not push it to the holder**, which is
        // exactly the D209 hole, seen from the other side.
        let (at, _heard, stops, serving) = an_endpoint(holder.clone(), Granted::new()).await;
        let hub = HandWrittenHub {
            endpoint: at.clone(),
            holder: "the-holder".to_owned(),
            // A table nothing here serves from, so the push never reaches the
            // endpoint.
            granted: Granted::new(),
            lifetime: 300,
        };
        let why = fetch(
            &fetcher,
            &JobId::new("j1"),
            &AssignedPack {
                holders: vec![Holder {
                    runner: "the-holder".to_owned(),
                    endpoint: Some(at),
                }],
                ..as_assigned_elsewhere(&pack)
            },
            &fetcher_at.path().join("in"),
            &hub,
            quick(),
        )
        .await
        .expect_err("a capability the holder was never pushed serves nothing");
        assert!(
            why.why.contains("refused to serve object")
                && why.why.contains("not one the hub pushed")
                && why.why.contains("D209"),
            "the peer's own sentence did not reach the fetcher: {why}"
        );
        assert!(
            !fetcher.holds(&pack.pack),
            "a refused transfer registered a pack"
        );
        let _ = stops.send(());
        let report = serving.await.expect("the endpoint comes back");
        assert_eq!(report.refused, 1, "{report:?}");
        assert_eq!(report.bytes, 0, "{report:?}");
    }

    /// Killed mid-flight: no readable object, no registered pack, and nothing
    /// left staging.
    ///
    /// The store already guarantees the first half — a partial object is
    /// never visible under its final name — so what is proved here is that
    /// this module does not defeat it: the bytes are staged under a name
    /// nothing looks up, and `ingest` is the only thing that ever places one.
    #[tokio::test]
    async fn a_fetch_killed_mid_flight_leaves_no_object_and_no_pack() {
        let holder_at = TempDir::new("fetch-killed-holder");
        let fetcher_at = TempDir::new("fetch-killed");
        let holder = PackStore::open(holder_at.path()).expect("the holder's store");
        let fetcher = PackStore::open(fetcher_at.path()).expect("the fetcher's store");
        // Several chunks, so that there is a middle to be cut in.
        let big = vec![3u8; (herd_proto::pack::PACK_CHUNK_BYTES * 6) as usize];
        let pack = a_pack(&holder, &holder_at, "killed", &[("bin/big", &big)]);
        let sha = herd_proto::sha256_hex(&big);

        let granted = Granted::new();
        let (at, _heard, stops, serving) = an_endpoint(holder.clone(), granted.clone()).await;
        let hub = HandWrittenHub {
            endpoint: at.clone(),
            holder: "the-holder".to_owned(),
            granted,
            lifetime: 300,
        };
        let assigned = AssignedPack {
            holders: vec![Holder {
                runner: "the-holder".to_owned(),
                endpoint: Some(at),
            }],
            ..as_assigned_elsewhere(&pack)
        };
        let into = fetcher_at.path().join("in");
        // Cut at a **point** and not after a span: the fetch is killed once
        // bytes of the object are really staged and the object is not whole
        // yet. A timed cut would be a guess about this host's speed, and the
        // host that lost the guess would run the whole assertion against a
        // transfer that had finished — which is D101's defect exactly, and it
        // would have passed.
        let staging = fetcher.path().join(INCOMING_DIR);
        let hub = Arc::new(hub);
        let task = tokio::spawn({
            let (fetcher, assigned, into, hub) = (
                fetcher.clone(),
                assigned.clone(),
                into.clone(),
                Arc::clone(&hub),
            );
            async move {
                fetch(
                    &fetcher,
                    &JobId::new("j1"),
                    &assigned,
                    &into,
                    &*hub,
                    quick(),
                )
                .await
            }
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let staged = loop {
            assert!(
                Instant::now() < deadline,
                "nothing was ever staged, so nothing was cut mid-flight"
            );
            let staged = staged_bytes(&staging);
            assert!(
                staged < big.len() as u64,
                "the whole object was staged before it could be cut: {staged} of {}",
                big.len()
            );
            if staged > 0 {
                break staged;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        };
        println!(
            "the fetch was cut with {staged} of {} byte(s) staged",
            big.len()
        );
        task.abort();
        let cut = task.await;
        assert!(cut.is_err(), "the fetch was not cut: {cut:?}");
        assert!(
            !fetcher.holds(&pack.pack),
            "a pack whose transfer was cut is registered as held"
        );
        assert!(
            !fetcher.objects_dir().join(&sha).exists(),
            "a half-fetched object is readable under its own name"
        );
        assert!(
            !into.exists(),
            "a cut transfer laid a pack out for the job anyway"
        );
        // And the staging directory went with the future: the `Incoming`
        // guard removes it on every path out, the cancelled one included.
        let left: Vec<_> = std::fs::read_dir(&staging)
            .map(|entries| entries.flatten().map(|at| at.path()).collect())
            .unwrap_or_default();
        assert!(
            left.is_empty(),
            "a cancelled fetch left {left:?} staging under {}",
            staging.display()
        );
        let _ = stops.send(());
        let _ = serving.await;
    }

    /// What a previous life left staging is removed at startup, and it is
    /// said.
    #[test]
    fn a_half_fetched_pack_a_kill_left_behind_is_removed_and_said() {
        let temp = TempDir::new("fetch-forget");
        let store = PackStore::open(temp.path()).expect("a store");
        // Nothing there is silence and not a line about an empty directory.
        assert!(forget_incoming(&store).is_none());
        let left = store
            .path()
            .join(INCOMING_DIR)
            .join("deadbeef-1-2")
            .join("objects");
        std::fs::create_dir_all(&left).expect("a previous life's staging");
        std::fs::write(left.join("half-an-object"), b"1234567890").expect("some bytes");
        let said = forget_incoming(&store).expect("a staged pack is reported");
        assert!(
            said.contains("1 half-fetched pack(s)")
                && said.contains("10 byte(s)")
                && said.contains("never registered"),
            "{said}"
        );
        assert!(
            !store.path().join(INCOMING_DIR).exists(),
            "the staging area is still there"
        );
        assert!(forget_incoming(&store).is_none(), "and it is said once");
    }
}
