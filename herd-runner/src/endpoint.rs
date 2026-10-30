//! The one socket a runner may ever hold open: the endpoint that serves a
//! pack's objects to a peer (D10, D11, D200, D209).
//!
//! ## It binds through [`crate::mesh`] and nowhere else
//!
//! `herd-runner/tests/confinement.rs` permits the name `TcpListener` in
//! exactly one module of this crate, and that module is `mesh.rs`. So this
//! one binds [`crate::mesh::MeshListener`] — an alias, which is not that
//! name — and the permit stays a door with a name on it rather than an
//! exception that grew a second entrance.
//!
//! ## It opens no socket at all unless it may serve
//!
//! [`mesh::may_serve`] is the whole of the gate, and this module does not
//! re-read either half of it: no mesh address means outbound-only, and
//! [`crate::Runner::serves_content`] is where `off` and draining are already
//! read (E-33, D11's third condition). Two readings of one rule is how `off`
//! comes to mean two things.
//!
//! And it is consulted on **every turn of the loop**, not once at startup:
//! `off` arrives while the endpoint is already bound, and the answer has to
//! be able to change under it. When it goes false the listener is **dropped**
//! — the socket leaves `/proc/net/tcp`, so "no socket at all" is a fact about
//! the process and not a promise about a code path — and when it comes back
//! the endpoint binds again.
//!
//! ## A capability is only valid because the hub pushed it here
//!
//! This is the hole the whole design exists to avoid, so it is worth saying
//! plainly: **a token a peer presents is never evidence of anything.** The
//! hub mints one per `(pack, fetching peer)` and pushes it to *this* runner
//! over *this* runner's own already-authenticated control link
//! ([`herd_proto::pack::ExpectTransfer`], D209). [`Granted::pushed_by_hub`]
//! is the only way a capability enters this table, and its name says where it
//! came from. Nothing in this module mints one, derives one, or widens one.
//!
//! What that buys and what it does not: the transfer is **authorised, not
//! confidential** (D200). A stranger who finds the port cannot read a pack
//! off it. The bytes in flight on the fleet's own mesh are not secret, there
//! is no TLS and no PKI on this leg, and that is a decision and not an
//! omission.
//!
//! ### The peer wire carries no peer name, and that is a real limit
//!
//! A capability names the peer it is for ([`FetchCapability::peer`]), and
//! [`herd_proto::pack::PeerRequest`] carries **no** peer identity: the fields
//! are a protocol version, a token, a pack, an object and an offset. So this
//! endpoint cannot refuse "the wrong peer" by name, because it cannot tell
//! one dialler from another. What binds a transfer to a peer is possession of
//! the token, which is exactly what "authorised, not confidential" means — a
//! bearer capability's holder *is* its subject.
//!
//! What the endpoint does refuse by name is every other scope the capability
//! carries, and the first of those is the one that case was reaching for: a
//! token minted for one peer's transfer of pack P does not read pack Q
//! ([`Refusal::WrongPack`]), and one minted to be pushed to does not
//! authorise being read from ([`Refusal::WrongDirection`]).
//!
//! ## A refusal is a sentence, never an absence
//!
//! D150's lesson, from the other side of the wire: a fetcher told nothing
//! waits for bytes that will never come, and the party that knows why is the
//! one that has to speak. Every path out of [`authorise`] that serves no
//! bytes writes a [`herd_proto::pack::PeerRefusal`] before the connection
//! closes, and [`Refusal`] is a named variant and not a string so that the
//! fetcher's code can read what the fetcher's operator reads.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use herd_proto::pack::{
    decode_peer_header, encode_peer_header, take_peer_body, ExpectTransfer, FetchCapability,
    PackId, PeerFramingRefusal, PeerHeaderRead, PeerRefusal, PeerReply, PeerRequest,
    TransferDirection, PACK_CHUNK_BYTES, PACK_ENDPOINT_READ_BYTES,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::mesh::{self, Mesh};
use crate::packstore::PackStore;

/// How often the endpoint asks whether it may still serve.
///
/// E-33 promises the owner's switch bites inside ten seconds, and this is
/// well inside it. Injectable for the reason [`crate::Config::switch_poll`]
/// is: a test about the socket being gone must not spend a second of its
/// patience on this.
pub const SERVES_POLL: Duration = Duration::from_millis(200);

// ------------------------------------------------- what the hub pushed ----

/// The capabilities the hub has pushed to this runner, by token (D209).
///
/// **The only way in is [`Granted::pushed_by_hub`]**, whose name is the whole
/// of the rule: a capability exists here because the hub said so on this
/// runner's own authenticated link, never because a peer presented one.
#[derive(Clone, Default)]
pub struct Granted {
    held: Arc<Mutex<std::collections::BTreeMap<String, FetchCapability>>>,
}

impl Granted {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a capability the **hub** pushed over this runner's control link.
    ///
    /// Returns the sentence an operator reads. Called from the frame handler
    /// for [`herd_proto::FromHub::ExpectTransfer`] and from nowhere else: a
    /// second caller would be a second way for a capability to come into
    /// existence, which is the one thing this type is for.
    pub fn pushed_by_hub(&self, expect: &ExpectTransfer) -> String {
        let capability = FetchCapability {
            token: expect.token.clone(),
            pack: expect.pack.clone(),
            peer: expect.peer.clone(),
            direction: expect.direction,
            objects: expect.objects.clone(),
            expires_unix_s: expect.expires_unix_s,
        };
        let scope = if capability.objects.is_empty() {
            "the whole pack".to_owned()
        } else {
            format!("{} object(s) of it", capability.objects.len())
        };
        let said = format!(
            "the hub says to expect {} of pack {} by {}, for {scope}, until {} — this runner \
             takes a transfer's authority from the hub it is joined to and never from the peer \
             presenting it (D209)",
            match capability.direction {
                TransferDirection::Fetch => "a fetch",
                TransferDirection::Push => "a push",
            },
            capability.pack,
            capability.peer,
            capability.expires_unix_s
        );
        if let Ok(mut held) = self.held.lock() {
            held.insert(capability.token.clone(), capability);
        }
        said
    }

    /// How many capabilities are held right now.
    pub fn len(&self) -> usize {
        self.held.lock().map(|held| held.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop every capability that has expired, and say how many went.
    ///
    /// An expired capability is refused whether or not this has run — the
    /// check is [`authorise`]'s and the clock is the one that decides. This
    /// is only so that a long-lived runner's table does not grow with every
    /// transfer it was ever told to expect.
    pub fn forget_expired(&self, now_unix_s: i64) -> usize {
        let Ok(mut held) = self.held.lock() else {
            return 0;
        };
        let before = held.len();
        held.retain(|_, capability| capability.expires_unix_s >= now_unix_s);
        before - held.len()
    }

    fn held(&self, token: &str) -> Option<FetchCapability> {
        self.held.lock().ok()?.get(token).cloned()
    }
}

impl std::fmt::Debug for Granted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The tokens are bearer secrets (D200), so the count and not the
        // contents: a log line is the one place a capability would leak to
        // somebody the hub never gave it to.
        write!(f, "Granted({} capability/ies held)", self.len())
    }
}

// --------------------------------------------------------- refusals ----

/// Why this endpoint served no bytes, named.
///
/// Named and not stringly for D150's reason twice over: the sentence is what
/// an operator reads, and the variant is what the fetcher's code reads — and
/// a fetcher that got only a sentence could not tell "ask the hub for a
/// fresh capability" from "stop asking".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A request carrying no token at all.
    NoCapability,
    /// A token this runner was never pushed. **Not** a hint that another one
    /// would work: this is the D209 refusal, and a peer that minted its own
    /// capability lands here.
    NotPushedByHub,
    /// A capability whose moment has passed.
    Expired {
        expires_unix_s: i64,
        now_unix_s: i64,
    },
    /// A capability for one pack, presented asking for another.
    WrongPack {
        minted_for: PackId,
        asked_for: PackId,
    },
    /// A capability to be pushed to, presented asking to read.
    WrongDirection { minted: TransferDirection },
    /// An object outside the capability's own list.
    OutOfScope { object: String, covers: usize },
    /// An object the pack's manifest does not name as a file.
    NotInThePack { object: String, pack: PackId },
    /// The store could not answer about that pack at all.
    Unreadable { why: String },
    /// The framing itself was refused ([`PeerFramingRefusal`] owns the
    /// sentence, for invariant 4's reason).
    Framing(PeerFramingRefusal),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCapability => f.write_str(
                "pack transfer refused: that request carries no capability at all, and this \
                 endpoint serves nothing unauthorised. A transfer's authority is minted by the \
                 hub and pushed to this runner over its own link (D209); ask the hub for one",
            ),
            Self::NotPushedByHub => f.write_str(
                "pack transfer refused: that token is not one the hub pushed to this runner, so \
                 nothing here can say what it authorises. A capability is valid because the hub \
                 told *this* runner to expect it over *this* runner's authenticated link \
                 (D209) — never because a peer presented it. It may also simply have expired \
                 and been forgotten: ask the hub for a fresh one",
            ),
            Self::Expired {
                expires_unix_s,
                now_unix_s,
            } => write!(
                f,
                "pack transfer refused: that capability stopped being usable at {expires_unix_s} \
                 and it is now {now_unix_s}, {}s later. A capability is deliberately short-lived \
                 ({}s), because a bearer token on a wire with no TLS is worth exactly as long as \
                 it lives (D200): ask the hub for a fresh one",
                now_unix_s.saturating_sub(*expires_unix_s),
                herd_proto::pack::PACK_CAPABILITY_LIFETIME.as_secs()
            ),
            Self::WrongPack {
                minted_for,
                asked_for,
            } => write!(
                f,
                "pack transfer refused: that capability is for pack {minted_for} and the request \
                 asks for pack {asked_for}. A capability is scoped to one pack, so a token \
                 minted for one transfer cannot read another pack's bytes off this runner"
            ),
            Self::WrongDirection { minted } => write!(
                f,
                "pack transfer refused: that capability authorises {}, and the request asks to \
                 read. A runner that may be pushed to has not thereby agreed to be fetched \
                 from, and the direction is on the capability so that the two are never the \
                 same permission",
                match minted {
                    TransferDirection::Push => "a push to this runner",
                    TransferDirection::Fetch => "a fetch from this runner",
                }
            ),
            Self::OutOfScope { object, covers } => write!(
                f,
                "pack transfer refused: object {object} is not one of the {covers} this \
                 capability covers. A fetcher asks for the objects it lacks and is authorised \
                 for those: an object it did not ask for is out of scope even when it is in the \
                 same pack"
            ),
            Self::NotInThePack { object, pack } => write!(
                f,
                "pack transfer refused: pack {pack}'s manifest names no file object {object}. \
                 Only the `kind f` rows name bytes under `objects/`, so a digest from a `kind d` \
                 or `kind l` row is not an object this or any runner holds — and a name the \
                 manifest does not carry is never joined onto the store's path"
            ),
            Self::Unreadable { why } => write!(
                f,
                "pack transfer refused: this runner cannot read that pack out of its own store: \
                 {why}"
            ),
            Self::Framing(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl From<PeerFramingRefusal> for Refusal {
    fn from(refusal: PeerFramingRefusal) -> Self {
        Self::Framing(refusal)
    }
}

/// One authorised read: which object, how big it is, and where to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorised {
    pub object: String,
    /// The object's whole size, from the manifest — so a reply's
    /// `object_length` is the manifest's figure and not the file's, and a
    /// file that disagreed with it is a refusal rather than a short object.
    pub object_length: u64,
    pub offset: u64,
}

/// The whole capability check, as a function of a request, a clock and a
/// store.
///
/// Pure in everything that matters: it does no I/O on the socket and takes
/// `now_unix_s` rather than reading a clock, so every refusal below has a
/// unit test that fires it without waiting for anything.
pub fn authorise(
    granted: &Granted,
    store: &PackStore,
    request: &PeerRequest,
    now_unix_s: i64,
) -> Result<Authorised, Refusal> {
    // Before anything else, because a peer on another protocol may not even
    // mean the same thing by these fields.
    herd_proto::pack::check_peer_protocol_version(request.protocol_version)?;
    if request.token.is_empty() {
        return Err(Refusal::NoCapability);
    }
    let capability = granted
        .held(&request.token)
        .ok_or(Refusal::NotPushedByHub)?;
    if now_unix_s > capability.expires_unix_s {
        return Err(Refusal::Expired {
            expires_unix_s: capability.expires_unix_s,
            now_unix_s,
        });
    }
    if capability.pack != request.pack {
        return Err(Refusal::WrongPack {
            minted_for: capability.pack,
            asked_for: request.pack.clone(),
        });
    }
    if capability.direction != TransferDirection::Fetch {
        return Err(Refusal::WrongDirection {
            minted: capability.direction,
        });
    }
    if !capability.objects.is_empty() && !capability.objects.contains(&request.object) {
        return Err(Refusal::OutOfScope {
            object: request.object.clone(),
            covers: capability.objects.len(),
        });
    }
    // The manifest, and the `kind f` filter inside `objects_needed`, are what
    // say whether this is an object at all: a `kind d` or `kind l` row's
    // digest is of something that is not a file's bytes, and no `objects/`
    // entry will ever answer to it. The digest itself needs no check of mine
    // — `herd_proto::pack::check_object_digest` refused anything but 64
    // lowercase hex at the one place this entry was built, and a third copy
    // of that rule is how two of them come to disagree.
    let manifest = store
        .manifest_of(&capability.pack)
        .map_err(|why| Refusal::Unreadable { why })?;
    let object_length = *manifest
        .objects_needed()
        .get(&request.object)
        .ok_or_else(|| Refusal::NotInThePack {
            object: request.object.clone(),
            pack: capability.pack.clone(),
        })?;
    if request.offset > object_length {
        return Err(Refusal::Framing(PeerFramingRefusal::BodyPastTheObject {
            offset: request.offset,
            length: 0,
            object_length,
        }));
    }
    Ok(Authorised {
        object: request.object.clone(),
        object_length,
        offset: request.offset,
    })
}

// ------------------------------------------------------- the socket ----

/// What the endpoint says, so that the runner can print it and a test can
/// read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// A socket is now open on the mesh address.
    Listening {
        at: SocketAddr,
        advertise: SocketAddr,
    },
    /// There is no socket, and why there is none.
    Closed { why: String },
    /// An object was served whole.
    Served { object: String, bytes: u64 },
    /// A peer was refused, by name.
    Refused { why: String },
}

impl Said {
    /// The line an operator reads on the runner's stderr.
    pub fn sentence(&self) -> String {
        match self {
            Self::Listening { at, advertise } => format!(
                "serving pack objects to peers on {at}, advertised as {advertise}; this is the \
                 one socket this runner holds (invariant 3, D11)"
            ),
            Self::Closed { why } => {
                format!("this runner is listening on nothing: {why}")
            }
            Self::Served { object, bytes } => {
                format!("served object {object} to a peer, {bytes} byte(s)")
            }
            Self::Refused { why } => why.clone(),
        }
    }
}

/// Where the endpoint's sentences go. An `Arc` because every connection
/// handler holds one.
pub type Voice = Arc<dyn Fn(Said) + Send + Sync>;

/// A voice that says nothing, for a test that is about something else.
pub fn silent() -> Voice {
    Arc::new(|_| {})
}

/// What one run of [`serve_while`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// How many times a socket was opened. **Nought is the assertion for an
    /// outbound-only runner and for one that is `off` throughout**, and it is
    /// a count rather than a flag so that a close-and-reopen is visible.
    pub opened: u32,
    /// How many times it was closed again because this runner stopped being
    /// allowed to serve.
    pub closed: u32,
    /// Connections accepted.
    pub connections: u64,
    /// Objects served whole.
    pub objects: u64,
    /// Body bytes written to peers. The endpoint's own count of what left
    /// this machine.
    pub bytes: u64,
    /// Peers refused, by any of [`Refusal`]'s names.
    pub refused: u64,
}

/// Hold the endpoint open for as long as this runner may serve, and come back
/// when `stop` resolves.
///
/// The three states and the one gate:
///
/// - `mesh` is `None` — outbound-only. **No socket is ever opened**, the loop
///   still runs (so that a stop is still a stop), and `opened` is nought.
/// - [`mesh::may_serve`] is false — `off`, or draining. Any socket is
///   dropped, so the process holds none.
/// - true — one socket, on the configured mesh address, and nothing else.
///
/// `stop` resolving aborts whatever is in flight rather than waiting it out:
/// invariant 2 gives the owner five seconds for the **whole** runner, and a
/// peer that loses a transfer reads `BodyTruncated` — a sentence — and
/// resumes at the last whole chunk. A transfer is never worth more than the
/// owner's machine.
pub async fn serve_while(
    mesh: Option<Mesh>,
    store: PackStore,
    granted: Granted,
    serves_content: impl Fn() -> bool,
    poll: Duration,
    voice: Voice,
    stop: impl Future<Output = ()>,
) -> Report {
    let mut report = Report::default();
    let mut listener: Option<mesh::MeshListener> = None;
    let mut in_flight: tokio::task::JoinSet<Connection> = tokio::task::JoinSet::new();
    let mut stop = std::pin::pin!(stop);
    if mesh.is_none() {
        voice(Said::Closed {
            why: format!(
                "no mesh address is configured, so this runner is outbound-only: it fetches and \
                 pushes and serves nothing ({} sets one)",
                mesh::LISTEN_FLAG
            ),
        });
    }
    loop {
        let may = mesh::may_serve(mesh.as_ref(), serves_content());
        match (&listener, may) {
            (None, true) => {
                // The one bind in the crate, through the one alias.
                let wanted = mesh.as_ref().expect("may_serve is false with no mesh");
                match mesh::MeshListener::bind(wanted.listen).await {
                    Ok(bound) => {
                        let at = bound.local_addr().unwrap_or(wanted.listen);
                        // A listen port of 0 means the kernel picked one, so
                        // what peers are told has to carry the port the bind
                        // really got rather than the nought that was asked
                        // for.
                        let advertise = if wanted.advertise.port() == 0 {
                            SocketAddr::new(wanted.advertise.ip(), at.port())
                        } else {
                            wanted.advertise
                        };
                        listener = Some(bound);
                        report.opened += 1;
                        voice(Said::Listening { at, advertise });
                    }
                    Err(error) => {
                        // Said and retried, not fatal: the address was judged
                        // acceptable at startup (`mesh.rs`), so a bind that
                        // fails here is a port in use or an interface that
                        // has gone away — a fact about the host this minute,
                        // which is not a reason to take the runner's jobs
                        // away from it.
                        voice(Said::Closed {
                            why: format!(
                                "{} cannot be bound: {error}. This runner goes on running jobs \
                                 and serves no peer until it can",
                                wanted.listen
                            ),
                        });
                    }
                }
            }
            (Some(_), false) => {
                listener = None;
                report.closed += 1;
                // Aborted with the socket: a transfer in flight is content
                // being served, and `off` means this machine serves nothing.
                in_flight.abort_all();
                voice(Said::Closed {
                    why: "the owner's switch or a drain says this runner serves nothing, so the \
                          mesh socket is closed and there is no socket of any kind left (E-33, \
                          D11)"
                        .to_owned(),
                });
            }
            _ => {}
        }
        // Whatever finished since the last turn, totalled without waiting for
        // anything: `try_join_next` and not `join_next` inside the `select!`
        // below, because a branch that borrowed the set would collide with
        // the accept branch that spawns into it.
        while let Some(done) = in_flight.try_join_next() {
            if let Ok(one) = done {
                report.objects += one.objects;
                report.bytes += one.bytes;
                report.refused += one.refused;
            }
        }
        let tick = tokio::time::sleep(poll);
        let accepted = match &listener {
            Some(bound) => {
                tokio::select! {
                    () = &mut stop => break,
                    () = tick => None,
                    accepted = bound.accept() => Some(accepted),
                }
            }
            None => {
                tokio::select! {
                    () = &mut stop => break,
                    () = tick => None,
                }
            }
        };
        match accepted {
            Some(Ok((stream, _from))) => {
                report.connections += 1;
                let store = store.clone();
                let granted = granted.clone();
                let voice = Arc::clone(&voice);
                in_flight.spawn(async move { serve_one(stream, store, granted, voice).await });
            }
            Some(Err(error)) => voice(Said::Closed {
                why: format!("a peer's connection could not be accepted: {error}"),
            }),
            None => {}
        }
    }
    // The stop path. What has already finished is totalled — a transfer that
    // completed before the owner asked is not unreported because of when she
    // asked — and what is still in flight is aborted without being awaited,
    // for the reason in this function's doc. **A transfer aborted mid-object
    // contributes no figures**, which is why the fetcher's own count and not
    // this one is what the byte assertions are made against.
    while let Some(done) = in_flight.try_join_next() {
        if let Ok(one) = done {
            report.objects += one.objects;
            report.bytes += one.bytes;
            report.refused += one.refused;
        }
    }
    in_flight.abort_all();
    drop(listener);
    report
}

/// What one connection did, so the loop can total it.
#[derive(Debug, Default)]
struct Connection {
    objects: u64,
    bytes: u64,
    refused: u64,
}

/// Serve one peer until it goes away, one object per request.
///
/// Several objects on one connection, deliberately: the consumer's measured
/// pack is 557 files, and a connection per object would be 557 dials for one
/// transfer. The request is read, the object is streamed in
/// [`PACK_CHUNK_BYTES`] chunks without being asked again, and then the next
/// request is read off the same stream.
async fn serve_one(
    mut stream: tokio::net::TcpStream,
    store: PackStore,
    granted: Granted,
    voice: Voice,
) -> Connection {
    let mut did = Connection::default();
    let mut buffer: Vec<u8> = Vec::new();
    let mut read = vec![0u8; PACK_ENDPOINT_READ_BYTES as usize];
    loop {
        // One whole request line, or a peer that has gone away.
        let request: PeerRequest = loop {
            match decode_peer_header::<PeerRequest>(&buffer) {
                Ok(PeerHeaderRead::Read(header)) => {
                    // A request carries no body, which is the trait's answer
                    // and not this function's guess.
                    let (_, rest) =
                        match take_peer_body(&buffer[header.body_at..], header.body_bytes) {
                            Ok(split) => split,
                            Err(refusal) => {
                                did.refused += 1;
                                say_refusal(&mut stream, &voice, &Refusal::Framing(refusal)).await;
                                return did;
                            }
                        };
                    let rest = rest.to_vec();
                    buffer = rest;
                    break header.header;
                }
                Ok(PeerHeaderRead::Incomplete) => {}
                Err(refusal) => {
                    did.refused += 1;
                    say_refusal(&mut stream, &voice, &Refusal::Framing(refusal)).await;
                    return did;
                }
            }
            match stream.read(&mut read).await {
                // A peer that closed between requests is a peer that is
                // done, not a peer that was refused: an orderly end to a
                // transfer looks exactly like this.
                Ok(0) => return did,
                Ok(got) => buffer.extend_from_slice(&read[..got]),
                Err(_) => return did,
            }
        };
        let allowed = match authorise(&granted, &store, &request, now_unix_s()) {
            Ok(allowed) => allowed,
            Err(refusal) => {
                did.refused += 1;
                say_refusal(&mut stream, &voice, &refusal).await;
                return did;
            }
        };
        let path = store.objects_dir().join(&allowed.object);
        let mut offset = allowed.offset;
        loop {
            let want = PACK_CHUNK_BYTES.min(allowed.object_length - offset);
            if want == 0 {
                break;
            }
            let bytes = match read_chunk(path.clone(), offset, want).await {
                Ok(bytes) if bytes.len() as u64 == want => bytes,
                Ok(bytes) => {
                    // The manifest said one size and the file is another.
                    // Refused and not served short: an object is its sha256.
                    did.refused += 1;
                    say_refusal(
                        &mut stream,
                        &voice,
                        &Refusal::Framing(PeerFramingRefusal::BodyTruncated {
                            promised: want,
                            arrived: bytes.len(),
                        }),
                    )
                    .await;
                    return did;
                }
                Err(error) => {
                    did.refused += 1;
                    say_refusal(
                        &mut stream,
                        &voice,
                        &Refusal::Unreadable {
                            why: format!("{} cannot be read: {error}", path.display()),
                        },
                    )
                    .await;
                    return did;
                }
            };
            let reply = PeerReply {
                object: allowed.object.clone(),
                offset,
                length: want,
                object_length: allowed.object_length,
            };
            let (line, promised) = match encode_peer_header(&reply) {
                Ok(both) => both,
                Err(refusal) => {
                    did.refused += 1;
                    say_refusal(&mut stream, &voice, &Refusal::Framing(refusal)).await;
                    return did;
                }
            };
            debug_assert_eq!(promised, bytes.len() as u64);
            if stream.write_all(&line).await.is_err() || stream.write_all(&bytes).await.is_err() {
                // A peer that went away mid-object. Nothing of this runner's
                // is wrong, and nothing is left behind here: the bytes are
                // the store's and are untouched.
                return did;
            }
            offset += want;
            did.bytes += want;
        }
        did.objects += 1;
        voice(Said::Served {
            object: allowed.object.clone(),
            bytes: allowed.object_length - allowed.offset,
        });
    }
}

/// Write the refusal and close. The sentence goes to the peer **and** to this
/// runner's own operator: one of the two has the context to act on it, and
/// which one it is depends on the refusal.
async fn say_refusal(stream: &mut tokio::net::TcpStream, voice: &Voice, refusal: &Refusal) {
    let why = refusal.to_string();
    voice(Said::Refused { why: why.clone() });
    if let Ok((line, _)) = encode_peer_header(&PeerRefusal { why }) {
        let _ = stream.write_all(&line).await;
    }
    let _ = stream.flush().await;
}

/// `want` bytes of a file at `offset`, off the blocking pool.
///
/// `spawn_blocking` and not `tokio::fs`, which this crate does not compile:
/// the same shape `job.rs` already uses to measure a workspace.
async fn read_chunk(path: PathBuf, offset: u64, want: u64) -> std::io::Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0u8; usize::try_from(want).unwrap_or(usize::MAX)];
        let mut filled = 0usize;
        while filled < bytes.len() {
            match file.read(&mut bytes[filled..])? {
                0 => break,
                got => filled += got,
            }
        }
        bytes.truncate(filled);
        Ok(bytes)
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?
}

/// Now, on this host's clock, in seconds since the epoch.
///
/// The hub's clock is what minted the expiry and this is the holder's, which
/// is why a capability's life is five minutes and not five seconds: D200
/// bought a short-lived bearer token, not a synchronised fleet.
pub fn now_unix_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packstore::PackStore;
    use crate::testing::TempDir;
    use herd_proto::pack::PackManifest;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    /// A pack directory in the three-part shape, ingested, and the manifest
    /// that names it. One `kind f` row per file, plus a `d` row and an `l`
    /// row, so that nothing here can pass by looking every digest up.
    fn a_pack(store: &PackStore, at: &TempDir, files: &[(&str, &[u8])]) -> PackManifest {
        let dir = at.path().join("pack");
        let objects = dir.join("objects");
        std::fs::create_dir_all(&objects).expect("a pack directory");
        let mut rows = String::from(herd_proto::CHATONS_PACK_FORMAT);
        rows.push('\n');
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
        std::fs::write(dir.join("manifest.tsv"), rows.as_bytes()).expect("a manifest");
        let held = store.ingest(&dir).expect("the pack ingests");
        std::fs::remove_dir_all(&dir).expect("the staging pack goes");
        held.manifest
    }

    fn expect(
        manifest: &PackManifest,
        peer: &str,
        direction: TransferDirection,
        objects: Vec<String>,
        expires_unix_s: i64,
    ) -> ExpectTransfer {
        ExpectTransfer {
            pack: manifest.id().clone(),
            peer: peer.to_owned(),
            token: format!("token-for-{peer}"),
            expires_unix_s,
            direction,
            objects,
        }
    }

    fn ask(manifest: &PackManifest, token: &str, object: &str) -> PeerRequest {
        PeerRequest {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            token: token.to_owned(),
            pack: manifest.id().clone(),
            object: object.to_owned(),
            offset: 0,
        }
    }

    /// Every capability refusal, each with the case that fires it, against
    /// one real store and one real pack.
    ///
    /// Together rather than one test each because the fixture is the same and
    /// the point is the **set**: a refusal missing from this list is a path
    /// that serves bytes without saying what authorised them.
    #[test]
    fn every_capability_refusal_has_a_case_that_fires_it() {
        let temp = TempDir::new("endpoint-caps");
        let store = PackStore::open(temp.path()).expect("a store");
        let manifest = a_pack(&store, &temp, &[("bin/tool", b"the tool's bytes")]);
        let object = herd_proto::sha256_hex(b"the tool's bytes");
        let granted = Granted::new();
        let now = 1_000_000i64;

        // 1. None presented.
        let mut none = ask(&manifest, "", &object);
        none.token = String::new();
        let why = authorise(&granted, &store, &none, now).expect_err("no token, no bytes");
        assert_eq!(why, Refusal::NoCapability, "{why}");
        assert!(
            why.to_string().contains("no capability at all") && why.to_string().contains("D209"),
            "{why}"
        );

        // 2. One the hub did not push: well formed, never granted.
        let why = authorise(
            &granted,
            &store,
            &ask(&manifest, "a-token-i-minted", &object),
            now,
        )
        .expect_err("a peer's own token authorises nothing");
        assert_eq!(why, Refusal::NotPushedByHub, "{why}");
        assert!(
            why.to_string().contains("not one the hub pushed") && why.to_string().contains("D209"),
            "{why}"
        );

        // And the table is still empty, so a refused presentation does not
        // teach this runner anything.
        assert!(granted.is_empty(), "a refusal never adds a capability");

        // 3. Expired. The hub pushed it; its moment has passed.
        let stale = expect(
            &manifest,
            "peer-stale",
            TransferDirection::Fetch,
            Vec::new(),
            now - 1,
        );
        granted.pushed_by_hub(&stale);
        let why = authorise(
            &granted,
            &store,
            &ask(&manifest, &stale.token, &object),
            now,
        )
        .expect_err("an expired capability serves nothing");
        assert_eq!(
            why,
            Refusal::Expired {
                expires_unix_s: now - 1,
                now_unix_s: now
            },
            "{why}"
        );
        assert!(why.to_string().contains("stopped being usable"), "{why}");
        // The boundary: valid *at* its expiry second, not one past it.
        let edge = expect(
            &manifest,
            "peer-edge",
            TransferDirection::Fetch,
            Vec::new(),
            now,
        );
        granted.pushed_by_hub(&edge);
        authorise(&granted, &store, &ask(&manifest, &edge.token, &object), now)
            .expect("a capability is usable in its last second");

        // 4. The wrong pack — the scope the peer wire can really check. A
        // capability minted for another pack's transfer reads nothing here.
        let other = TempDir::new("endpoint-other");
        let other_store = PackStore::open(other.path()).expect("a second store");
        let elsewhere = a_pack(&other_store, &other, &[("bin/other", b"another pack")]);
        let for_elsewhere = expect(
            &elsewhere,
            "peer-wrong-pack",
            TransferDirection::Fetch,
            Vec::new(),
            now + 60,
        );
        granted.pushed_by_hub(&for_elsewhere);
        let why = authorise(
            &granted,
            &store,
            &ask(&manifest, &for_elsewhere.token, &object),
            now,
        )
        .expect_err("one pack's token does not read another's bytes");
        assert!(
            matches!(why, Refusal::WrongPack { .. })
                && why.to_string().contains("scoped to one pack"),
            "{why}"
        );

        // 5. Out of scope: in the pack, outside the capability's own list.
        let narrow = expect(
            &manifest,
            "peer-narrow",
            TransferDirection::Fetch,
            vec![herd_proto::sha256_hex(b"something else entirely")],
            now + 60,
        );
        granted.pushed_by_hub(&narrow);
        let why = authorise(
            &granted,
            &store,
            &ask(&manifest, &narrow.token, &object),
            now,
        )
        .expect_err("an object it did not ask for is out of scope");
        assert!(
            matches!(why, Refusal::OutOfScope { covers: 1, .. })
                && why.to_string().contains("out of scope"),
            "{why}"
        );

        // 6. A push capability does not authorise a read.
        let pushed = expect(
            &manifest,
            "peer-push",
            TransferDirection::Push,
            Vec::new(),
            now + 60,
        );
        granted.pushed_by_hub(&pushed);
        let why = authorise(
            &granted,
            &store,
            &ask(&manifest, &pushed.token, &object),
            now,
        )
        .expect_err("a push is not a fetch");
        assert!(
            matches!(
                why,
                Refusal::WrongDirection {
                    minted: TransferDirection::Push
                }
            ) && why.to_string().contains("has not thereby agreed"),
            "{why}"
        );

        // 7. A digest the manifest does not name as a file — and the `kind d`
        // and `kind l` rows of the fixture are the case, because their
        // digests ARE in the manifest and are not objects.
        let whole = expect(
            &manifest,
            "peer-whole",
            TransferDirection::Fetch,
            Vec::new(),
            now + 60,
        );
        granted.pushed_by_hub(&whole);
        for (kind, bytes) in [
            ("d", &b"a directory is not a file's bytes"[..]),
            ("l", &b"a symlink is not a file's bytes"[..]),
        ] {
            let digest = herd_proto::sha256_hex(bytes);
            let why = authorise(
                &granted,
                &store,
                &ask(&manifest, &whole.token, &digest),
                now,
            )
            .expect_err("a row that is not a file names no object");
            assert!(
                matches!(why, Refusal::NotInThePack { .. })
                    && why.to_string().contains("`kind f` rows"),
                "kind {kind}: {why}"
            );
        }

        // 8. The protocol version, refused by herd-proto's own sentence.
        let mut old = ask(&manifest, &whole.token, &object);
        old.protocol_version = herd_proto::PROTOCOL_VERSION - 1;
        let why = authorise(&granted, &store, &old, now).expect_err("another protocol");
        assert!(
            matches!(
                why,
                Refusal::Framing(PeerFramingRefusal::ProtocolVersion { .. })
            ),
            "{why}"
        );

        // And the whole-pack capability does serve the object, so none of the
        // above is this check refusing everything.
        let allowed = authorise(
            &granted,
            &store,
            &ask(&manifest, &whole.token, &object),
            now,
        )
        .expect("the pack's own object, in scope, unexpired");
        assert_eq!(allowed.object, object);
        assert_eq!(allowed.object_length, b"the tool's bytes".len() as u64);
    }

    /// A capability enters the table through the hub's frame and through
    /// nothing else, and the sentence says so.
    #[test]
    fn a_capability_exists_only_because_the_hub_pushed_it() {
        let temp = TempDir::new("endpoint-d209");
        let store = PackStore::open(temp.path()).expect("a store");
        let manifest = a_pack(&store, &temp, &[("bin/tool", b"bytes")]);
        let granted = Granted::new();
        assert!(granted.is_empty());
        let said = granted.pushed_by_hub(&expect(
            &manifest,
            "peer-b",
            TransferDirection::Fetch,
            Vec::new(),
            now_unix_s() + 60,
        ));
        assert!(
            said.contains("the hub says to expect")
                && said.contains("never from the peer presenting it")
                && said.contains("D209"),
            "{said}"
        );
        assert_eq!(granted.len(), 1);
        // The source guard: the only mention of the table's own insert in
        // this crate is the one method named for where a capability comes
        // from. A second way in would be the hole D209 closes.
        let mut inserts: Vec<String> = Vec::new();
        for (path, text) in crate::testing::source_files() {
            for (number, line) in text.lines().enumerate() {
                if line.contains("held.insert(") || line.contains("self.held.lock") {
                    inserts.push(format!("{}:{}", path.display(), number + 1));
                }
            }
        }
        assert!(
            !inserts.is_empty(),
            "the walk found no write of the capability table at all, so its silence about every \
             other module would have proved nothing"
        );
        assert!(
            inserts
                .iter()
                .all(|at| at.contains("endpoint.rs") || at.contains("confinement.rs")),
            "a capability table is written outside endpoint.rs: {inserts:?}"
        );
        // Forgetting is by the clock and nothing else.
        assert_eq!(granted.forget_expired(now_unix_s() + 120), 1);
        assert!(granted.is_empty());
    }

    /// `serves_content() == false` means no socket at all — before, after and
    /// in both of E-33's positions — and an outbound-only runner never opens
    /// one however the switch moves.
    #[tokio::test]
    async fn serves_nothing_means_no_socket_at_all() {
        let temp = TempDir::new("endpoint-no-socket");
        let store = PackStore::open(temp.path()).expect("a store");
        // A mesh address the rule permits: loopback, which is a deliberate
        // clause of `mesh.rs` and not an accident of it.
        let mesh = Mesh::resolve("127.0.0.1:0", None, &crate::mesh::HostRouting::read())
            .expect("loopback is permitted");

        // 1. Off from the start: the loop runs and binds nothing.
        let (gone, stop) = tokio::sync::oneshot::channel::<()>();
        let served = tokio::spawn(serve_while(
            Some(mesh.clone()),
            store.clone(),
            Granted::new(),
            || false,
            Duration::from_millis(5),
            silent(),
            async move {
                let _ = stop.await;
            },
        ));
        tokio::time::sleep(Duration::from_millis(60)).await;
        let _ = gone.send(());
        let report = served.await.expect("the endpoint comes back");
        assert_eq!(
            report.opened, 0,
            "a runner that serves nothing opened a socket: {report:?}"
        );

        // 2. On, then off: it opens one and closes it again, and the close is
        // the switch's and not the stop's.
        let serving = Arc::new(AtomicBool::new(true));
        let watching = Arc::clone(&serving);
        let heard = Arc::new(Mutex::new(Vec::<Said>::new()));
        let kept = Arc::clone(&heard);
        let (gone, stop) = tokio::sync::oneshot::channel::<()>();
        let served = tokio::spawn(serve_while(
            Some(mesh),
            store,
            Granted::new(),
            move || watching.load(Ordering::SeqCst),
            Duration::from_millis(5),
            Arc::new(move |said| kept.lock().expect("a lock").push(said)),
            async move {
                let _ = stop.await;
            },
        ));
        // Bound.
        let deadline = Instant::now() + Duration::from_secs(5);
        let at = loop {
            assert!(Instant::now() < deadline, "the endpoint never bound");
            let found = heard
                .lock()
                .expect("a lock")
                .iter()
                .find_map(|said| match said {
                    Said::Listening { at, .. } => Some(*at),
                    _ => None,
                });
            if let Some(at) = found {
                break at;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        // Reachable, so the socket is real and not a sentence about one.
        tokio::net::TcpStream::connect(at)
            .await
            .expect("the endpoint accepts a connection while it may serve");
        // The owner switches off.
        serving.store(false, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "the socket was still there after the switch moved: {:?}",
                heard.lock().expect("a lock")
            );
            if tokio::net::TcpStream::connect(at).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let _ = gone.send(());
        let report = served.await.expect("the endpoint comes back");
        assert_eq!(report.opened, 1, "{report:?}");
        assert_eq!(
            report.closed, 1,
            "the switch's close is counted, not inferred: {report:?}"
        );
        let said = heard
            .lock()
            .expect("a lock")
            .iter()
            .map(Said::sentence)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            said.contains("no socket of any kind left") && said.contains("E-33"),
            "{said}"
        );
    }

    /// Invariant 2 with a transfer in flight: the owner's ask comes back in
    /// well under five seconds while an object is still being written.
    ///
    /// The peer stops reading after one chunk, which fills the socket buffer
    /// and leaves the endpoint blocked inside `write_all` — the state a
    /// naive stop would wait out. The figure is printed, because "under five
    /// seconds" is a measurement and not a claim.
    #[tokio::test]
    async fn a_stop_with_a_transfer_in_flight_is_inside_five_seconds() {
        let temp = TempDir::new("endpoint-stop");
        let store = PackStore::open(temp.path()).expect("a store");
        // Big enough that one chunk is nowhere near all of it, so the write
        // really blocks: eight chunks.
        let big = vec![7u8; (PACK_CHUNK_BYTES * 8) as usize];
        let manifest = a_pack(&store, &temp, &[("bin/big", &big)]);
        let object = herd_proto::sha256_hex(&big);
        let granted = Granted::new();
        let capability = expect(
            &manifest,
            "peer-slow",
            TransferDirection::Fetch,
            Vec::new(),
            now_unix_s() + 300,
        );
        granted.pushed_by_hub(&capability);
        let mesh = Mesh::resolve("127.0.0.1:0", None, &crate::mesh::HostRouting::read())
            .expect("loopback is permitted");
        let heard = Arc::new(Mutex::new(Vec::<Said>::new()));
        let kept = Arc::clone(&heard);
        let (gone, stop) = tokio::sync::oneshot::channel::<()>();
        let served = tokio::spawn(serve_while(
            Some(mesh),
            store,
            granted,
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
                    Said::Listening { at, .. } => Some(*at),
                    _ => None,
                });
            if let Some(at) = found {
                break at;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let mut peer = tokio::net::TcpStream::connect(at)
            .await
            .expect("a peer dials the endpoint");
        let (line, _) = encode_peer_header(&PeerRequest {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            token: capability.token.clone(),
            pack: manifest.id().clone(),
            object,
            offset: 0,
        })
        .expect("a request encodes");
        peer.write_all(&line).await.expect("the request goes");
        // One chunk's worth read, and then nothing: the endpoint is now
        // blocked writing into a buffer this peer is not draining.
        let mut seen = vec![0u8; 64 << 10];
        let got = peer.read(&mut seen).await.expect("the first bytes arrive");
        assert!(
            got > 0,
            "the transfer never started, so nothing is in flight"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;

        let asked = Instant::now();
        let _ = gone.send(());
        let report = served.await.expect("the endpoint comes back");
        let took = asked.elapsed();
        println!(
            "endpoint stop with a transfer in flight: {took:?} (invariant 2's bound is 5s); \
             {report:?}"
        );
        assert!(
            took < Duration::from_secs(5),
            "a stop with a transfer in flight took {took:?}, and invariant 2 gives the owner's \
             whole runner five seconds"
        );
        assert_eq!(report.connections, 1, "{report:?}");
        // And the peer's own read now ends — it is told by the stream \
        // closing, which its framing reads as a truncated body rather than
        // as a silence.
        let ended = tokio::time::timeout(Duration::from_secs(5), async {
            let mut sink = vec![0u8; 1 << 20];
            loop {
                match peer.read(&mut sink).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
            }
        })
        .await;
        assert!(ended.is_ok(), "the peer was left hanging after the stop");
    }

    /// A peer refused over a real socket gets the sentence, not a silence.
    #[tokio::test]
    async fn a_refused_peer_is_told_why_over_the_wire() {
        let temp = TempDir::new("endpoint-refused");
        let store = PackStore::open(temp.path()).expect("a store");
        let manifest = a_pack(&store, &temp, &[("bin/tool", b"the tool's bytes")]);
        let object = herd_proto::sha256_hex(b"the tool's bytes");
        let mesh = Mesh::resolve("127.0.0.1:0", None, &crate::mesh::HostRouting::read())
            .expect("loopback is permitted");
        let heard = Arc::new(Mutex::new(Vec::<Said>::new()));
        let kept = Arc::clone(&heard);
        let (gone, stop) = tokio::sync::oneshot::channel::<()>();
        let served = tokio::spawn(serve_while(
            Some(mesh),
            store,
            // Empty: the hub pushed nothing, so every token is one it did not
            // push.
            Granted::new(),
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
                    Said::Listening { at, .. } => Some(*at),
                    _ => None,
                });
            if let Some(at) = found {
                break at;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let mut peer = tokio::net::TcpStream::connect(at)
            .await
            .expect("a peer dials");
        let (line, _) = encode_peer_header(&PeerRequest {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            token: "a-token-no-hub-ever-minted".to_owned(),
            pack: manifest.id().clone(),
            object,
            offset: 0,
        })
        .expect("a request encodes");
        peer.write_all(&line).await.expect("the request goes");
        let mut answered = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(10), peer.read_to_end(&mut answered))
            .await
            .expect("the endpoint answers rather than hanging");
        read.expect("the refusal arrives");
        let decoded = decode_peer_header::<PeerRefusal>(&answered).expect("one header line");
        let PeerHeaderRead::Read(header) = decoded else {
            panic!("the refusal did not arrive as a whole line: {answered:?}");
        };
        assert!(
            header.header.why.contains("not one the hub pushed")
                && header.header.why.contains("D209"),
            "{}",
            header.header.why
        );
        assert_eq!(header.body_bytes, 0, "a refusal carries no body");
        let _ = gone.send(());
        let report = served.await.expect("the endpoint comes back");
        assert_eq!(report.refused, 1, "{report:?}");
        assert_eq!(report.bytes, 0, "a refused peer got bytes: {report:?}");
    }
}
