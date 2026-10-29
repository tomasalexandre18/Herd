//! The mesh-address rule: the only addresses a runner may ever serve content
//! from, and the refusal for every other one (invariant 3, D10, D11).
//!
//! ## What this module is, and what it deliberately is not
//!
//! Invariant 3 reads: *a runner opens the control connection to the hub and
//! needs nothing inbound to work. It may listen on a **private mesh
//! interface** to serve content to peers (D10, D11) — bound to that interface
//! only, never on a default or public one, and never behind a port forwarded
//! on someone's box.* D11 turned that into three conditions, and the first of
//! them is a **startup** condition: *"a configuration that would bind a
//! public or default interface is refused at startup, the way a bad data root
//! is"*.
//!
//! This module is that refusal and nothing else. **It binds no socket.** It
//! reads the host's routing, classifies an address, and either hands back a
//! [`Mesh`] or says by name why the address may not be served from. The
//! endpoint that walks through the door — the one listener a runner may ever
//! hold — is the next step, and [`MeshListener`] is the door's only name, so
//! that `herd-runner/tests/confinement.rs` can forbid the type everywhere
//! else in the crate rather than forbid it nowhere in particular.
//!
//! ## The rule
//!
//! `--mesh-listen ADDR` is refused at startup, naming the address and the
//! reason, when it is
//!
//! - a **wildcard** — `0.0.0.0`, `::`: it binds every interface this host
//!   has, including the default-route one, which is the whole thing D11
//!   forbids;
//! - an address of the **default-route interface**: that is the interface
//!   this host reaches the internet through, and so the interface a port
//!   forwarded on a router would land on;
//! - **globally routable**: an address the public internet can address is
//!   not a private mesh, whatever the firewall in front of it happens to say
//!   today.
//!
//! It is accepted for **loopback**, and for a **private** (RFC 1918),
//! **CGNAT** (`100.64/10`, which is what a tailnet hands out), **link-local**
//! or **unique-local** address that is *not* on the default-route interface.
//!
//! `--mesh-advertise ADDR` is what peers are told and defaults to the listen
//! address. It carries the same class rule — a wildcard or a globally
//! routable address is refused — but no routing check: it is the address a
//! *peer* dials, and this host's routing table says nothing about that.
//!
//! ## Two consequences, stated here rather than discovered later
//!
//! **A single-NIC flat-LAN machine has no permitted serving address, and is
//! therefore outbound-only.** Its one address is on the default-route
//! interface, so every form of `--mesh-listen` it could write is refused.
//! That is not a defect and it is not a gap to be closed by relaxing the
//! rule: it is exactly what *"never behind a port forwarded on someone's
//! box"* costs, and it is why D10 keeps a path that needs no listener at all.
//! An operator who wants that machine to serve joins it to a mesh — a
//! tailnet, a WireGuard network — and names the address the mesh gave it.
//!
//! **Loopback is permitted deliberately, not by accident.** It is neither a
//! default-route address nor a globally routable one, so the rule above
//! admits it on its own terms; this module returns on it before the routing
//! is even consulted, so that no host shape can take it away. Two reasons it
//! must stay admitted: a loopback endpoint is reachable by nothing off the
//! machine, which is the strictest reading of invariant 3 there is; and the
//! two-runner end-to-end that makes the pack half *done* (invariant 5) runs
//! two runners on one workstation, where loopback is the only mesh there is.
//!
//! ## Where the rule cannot be decided, it refuses
//!
//! The default-route reading needs the host's routing table and the host's
//! own addresses. Where neither can be read — a host with no `/proc`, a
//! sandbox that hides `getifaddrs` — the answer is **not** "permit and hope":
//! anything but loopback is refused, naming the reason, which is the strictly
//! conservative half of the choice. An operator on such a host gets a runner
//! that is outbound-only, which is the state D10 already has a path for.
//!
//! ## The switch still wins
//!
//! D11's third condition is that the owner's switch (E-33) stops the fetch
//! endpoint too. That gate exists already, as
//! [`crate::Runner::serves_content`], and this module does **not** restate
//! it: [`may_serve`] takes that method's answer as an argument rather than
//! reading the switch a second time. Two readings of one rule is how `off`
//! comes to mean two different things.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// The one socket type a runner may ever hold open for a peer, and the only
/// place in `herd-runner` where the name appears.
///
/// `herd-runner/tests/confinement.rs` forbids `TcpListener` in every module
/// of this crate but this one, and asserts that it is named here — so the
/// permit is a door with a name on it rather than an exception nobody wrote
/// down. Nothing binds it yet: the endpoint is the next step, and it binds
/// through this alias so that the guard keeps holding.
#[allow(dead_code)] // The door; the step that walks through it is next (D10, D11).
pub type MeshListener = tokio::net::TcpListener;

/// The flag whose value this rule judges.
pub const LISTEN_FLAG: &str = "--mesh-listen";

/// The flag that says what peers are told, which defaults to the listen
/// address.
pub const ADVERTISE_FLAG: &str = "--mesh-advertise";

// ------------------------------------------------- address classes ----

/// How far an address reaches, which is the whole question the class half of
/// the rule asks.
///
/// Rust's own `is_global` is still unstable, and it would be the wrong thing
/// to lean on anyway: the list below is a statement about which addresses
/// Herd is willing to serve from and it belongs where a reader can check it
/// against D11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// `0.0.0.0`, `::` — every interface at once.
    Wildcard,
    /// `127.0.0.0/8`, `::1`.
    Loopback,
    /// RFC 1918: `10/8`, `172.16/12`, `192.168/16`.
    Private,
    /// RFC 6598 `100.64/10`, which is the range a tailnet hands out.
    Cgnat,
    /// `169.254/16`, `fe80::/10`.
    LinkLocal,
    /// RFC 4193 `fc00::/7`.
    UniqueLocal,
    /// Multicast: `224/4`, `ff00::/8`. Not an address to be served from.
    Multicast,
    /// Documentation, benchmarking, discard, broadcast, `0/8`, `240/4`:
    /// addresses that are neither private nor sensibly public.
    Reserved,
    /// Addressable from the public internet.
    Global,
}

impl Reach {
    /// Whether an address of this class may be served from at all, before any
    /// question about this host's interfaces.
    pub fn servable(self) -> bool {
        matches!(
            self,
            Reach::Loopback | Reach::Private | Reach::Cgnat | Reach::LinkLocal | Reach::UniqueLocal
        )
    }

    /// What a refusal says about this class, in the operator's terms.
    fn said(self) -> &'static str {
        match self {
            Reach::Wildcard => "a wildcard, which binds every interface this host has",
            Reach::Loopback => "loopback",
            Reach::Private => "a private address",
            Reach::Cgnat => "a carrier-grade NAT address, which is what a tailnet hands out",
            Reach::LinkLocal => "a link-local address",
            Reach::UniqueLocal => "a unique-local address",
            Reach::Multicast => "a multicast address, which is not an address to serve from",
            Reach::Reserved => "a reserved address, which is neither private nor usefully public",
            Reach::Global => "globally routable, so the public internet can address it",
        }
    }
}

/// Which class an address falls in.
///
/// An IPv4-mapped IPv6 address (`::ffff:10.0.0.1`) is classified by the IPv4
/// address inside it: it is the same host on the same wire, and reading it as
/// an exotic IPv6 address would have made `::ffff:` a way round the rule.
pub fn reach_of(address: IpAddr) -> Reach {
    match address {
        IpAddr::V4(address) => reach_of_v4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(inner) => reach_of_v4(inner),
            None => reach_of_v6(address),
        },
    }
}

fn reach_of_v4(address: Ipv4Addr) -> Reach {
    let [a, b, _, _] = address.octets();
    if address.is_unspecified() {
        return Reach::Wildcard;
    }
    if address.is_loopback() {
        return Reach::Loopback;
    }
    if address.is_private() {
        return Reach::Private;
    }
    // RFC 6598, `100.64.0.0/10`: the second octet runs 64..=127.
    if a == 100 && (64..=127).contains(&b) {
        return Reach::Cgnat;
    }
    if address.is_link_local() {
        return Reach::LinkLocal;
    }
    if address.is_multicast() {
        return Reach::Multicast;
    }
    let reserved = address.is_broadcast()
        // `0.0.0.0/8` — "this network".
        || a == 0
        // Documentation, RFC 5737. Written out rather than taken from
        // `Ipv4Addr::is_documentation`, which is still unstable.
        || address.octets()[..3] == [192, 0, 2]
        || address.octets()[..3] == [198, 51, 100]
        || address.octets()[..3] == [203, 0, 113]
        // Benchmarking, RFC 2544.
        || (a == 198 && (b == 18 || b == 19))
        // The deprecated 6to4 relay anycast prefix, RFC 7526.
        || address.octets()[..3] == [192, 88, 99]
        // `240.0.0.0/4`, reserved for future use.
        || a >= 240;
    if reserved {
        return Reach::Reserved;
    }
    Reach::Global
}

fn reach_of_v6(address: Ipv6Addr) -> Reach {
    if address.is_unspecified() {
        return Reach::Wildcard;
    }
    if address.is_loopback() {
        return Reach::Loopback;
    }
    if address.is_multicast() {
        return Reach::Multicast;
    }
    let first = address.segments()[0];
    // `fe80::/10`.
    if first & 0xffc0 == 0xfe80 {
        return Reach::LinkLocal;
    }
    // `fc00::/7`, RFC 4193.
    if first & 0xfe00 == 0xfc00 {
        return Reach::UniqueLocal;
    }
    // `2001:db8::/32`, documentation; `100::/64`, discard-only.
    if address.segments()[..2] == [0x2001, 0x0db8] || address.segments()[..4] == [0x0100, 0, 0, 0] {
        return Reach::Reserved;
    }
    Reach::Global
}

// ------------------------------------------------- the host's routing ----

/// What this host says about its own routing, read once at startup.
///
/// `None` inside means the reading failed, which is a refusal and not a
/// permission: see the module header.
#[derive(Debug, Clone, Default)]
pub struct HostRouting {
    tables: Option<Tables>,
}

/// The two facts the default-route half of the rule needs.
#[derive(Debug, Clone, Default)]
struct Tables {
    /// Every interface carrying a default route, by name.
    default_route_interfaces: Vec<String>,
    /// Every address this host holds, with the interface holding it.
    addresses: Vec<(String, IpAddr)>,
}

impl HostRouting {
    /// Read the real host. Linux only, through `/proc/net` and `getifaddrs`.
    pub fn read() -> Self {
        let interfaces = read_default_route_interfaces();
        let addresses = host_addresses();
        let Some(default_route_interfaces) = interfaces else {
            return Self { tables: None };
        };
        // Both halves or neither. A route table with no interface addresses
        // beside it cannot answer "is this one of mine", and an answer of
        // "no, so permit it" would be the one failure this rule must not
        // have. Every Linux host holds at least `lo`, so an empty address
        // list means the reading failed rather than that there is nothing
        // there.
        if addresses.is_empty() {
            return Self { tables: None };
        }
        Self {
            tables: Some(Tables {
                default_route_interfaces,
                addresses,
            }),
        }
    }

    /// A host assembled by hand, for the tests that must read the same on
    /// every machine: the interfaces holding a default route, and every
    /// address this host holds.
    ///
    /// `cfg(test)` for as long as this module lives in the binary rather than
    /// in the library, where a `pub` item needs no such guard.
    #[cfg(test)]
    pub fn declared(
        default_route_interfaces: Vec<String>,
        addresses: Vec<(String, IpAddr)>,
    ) -> Self {
        Self {
            tables: Some(Tables {
                default_route_interfaces,
                addresses,
            }),
        }
    }

    /// A host whose routing could not be read at all.
    #[cfg(test)]
    pub fn unreadable() -> Self {
        Self { tables: None }
    }

    /// Whether this host's routing could be read.
    pub fn readable(&self) -> bool {
        self.tables.is_some()
    }

    /// The default-route interface holding `address`, if any.
    fn default_route_interface_of(&self, address: IpAddr) -> Option<&str> {
        let tables = self.tables.as_ref()?;
        tables
            .addresses
            .iter()
            .filter(|(_, held)| *held == address)
            .find_map(|(interface, _)| {
                tables
                    .default_route_interfaces
                    .iter()
                    .find(|named| *named == interface)
                    .map(String::as_str)
            })
    }

    /// Every address this host holds on a default-route interface. Used by
    /// the test that proves the refusal fires on the machine it runs on.
    #[cfg(test)]
    pub fn default_route_addresses(&self) -> Vec<(String, IpAddr)> {
        let Some(tables) = self.tables.as_ref() else {
            return Vec::new();
        };
        tables
            .addresses
            .iter()
            .filter(|(interface, _)| tables.default_route_interfaces.contains(interface))
            .cloned()
            .collect()
    }
}

/// The interfaces carrying a default route, from `/proc/net`.
///
/// `None` when neither table could be read, which is what tells
/// [`HostRouting::read`] that this host cannot answer the question.
fn read_default_route_interfaces() -> Option<Vec<String>> {
    let v4 = std::fs::read_to_string("/proc/net/route").ok();
    let v6 = std::fs::read_to_string("/proc/net/ipv6_route").ok();
    if v4.is_none() && v6.is_none() {
        return None;
    }
    let mut found: Vec<String> = Vec::new();
    for name in v4
        .as_deref()
        .map(default_route_interfaces_v4)
        .unwrap_or_default()
        .into_iter()
        .chain(
            v6.as_deref()
                .map(default_route_interfaces_v6)
                .unwrap_or_default(),
        )
    {
        if !found.contains(&name) {
            found.push(name);
        }
    }
    Some(found)
}

/// Parse `/proc/net/route`: `Iface`, hex `Destination`, … , hex `Mask`. A
/// default route is destination zero under mask zero.
fn default_route_interfaces_v4(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        if all_zero(fields[1]) && all_zero(fields[7]) && route_is_usable(fields[3]) {
            found.push(fields[0].to_owned());
        }
    }
    found
}

/// Parse `/proc/net/ipv6_route`: destination, hex prefix length, …, flags,
/// and the interface last. A default route is `::/0`.
fn default_route_interfaces_v6(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if all_zero(fields[0]) && all_zero(fields[1]) && route_is_usable(fields[8]) {
            found.push(fields[fields.len() - 1].to_owned());
        }
    }
    found
}

fn all_zero(field: &str) -> bool {
    !field.is_empty() && field.chars().all(|c| c == '0')
}

/// Whether a route's `Flags` column describes a route traffic really takes.
///
/// **Measured on this host, 2026-10-29, and the reason this function exists:**
/// `/proc/net/ipv6_route` carried three `::/0` entries on `lo`, with metric
/// `ffffffff` and flags `00200200` — `RTF_REJECT | RTF_NONEXTHOP`, the
/// unreachable default the kernel keeps when there is none. Counting those
/// made `lo` a "default-route interface", and the first version of this rule
/// refused `--mesh-listen 127.0.0.1` on a workstation. Loopback returns
/// before the routing is consulted, so that particular refusal could not
/// reach an operator — but the same reading would have called any address on
/// a rejecting interface a default-route address, which is a refusal nobody
/// could act on. So the two bits are read: `RTF_UP` set, `RTF_REJECT` clear.
fn route_is_usable(flags: &str) -> bool {
    const RTF_UP: u32 = 0x0001;
    const RTF_REJECT: u32 = 0x0200;
    let Ok(flags) = u32::from_str_radix(flags, 16) else {
        // A column this code cannot read is treated as a route that counts:
        // the conservative direction here is to see a default route, not to
        // miss one.
        return true;
    };
    flags & RTF_UP != 0 && flags & RTF_REJECT == 0
}

/// Every address this host holds, with the interface holding it.
///
/// `getifaddrs` because the question is exactly "which interface is this
/// address on", and that is the one question `/proc/net` cannot answer for
/// IPv4: there is no file listing an interface's IPv4 addresses. libc is
/// already how this crate reaches the syscalls the standard library does not
/// (`metrics.rs`, `privilege.rs`, `root.rs`).
#[cfg(unix)]
fn host_addresses() -> Vec<(String, IpAddr)> {
    let mut found = Vec::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` fills `head` with a list it owns, and every exit
    // from this function below frees it exactly once through `freeifaddrs`.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return found;
    }
    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: the cursor is non-null and points into the list the call
        // above filled; the list is not mutated while it is walked.
        let entry = unsafe { &*cursor };
        cursor = entry.ifa_next;
        if entry.ifa_name.is_null() || entry.ifa_addr.is_null() {
            continue;
        }
        // SAFETY: a non-null `ifa_name` is a NUL-terminated C string.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: a non-null `ifa_addr` points at a `sockaddr` whose family
        // says which larger structure it really is; only the two families
        // read below are dereferenced, each as its own type.
        let family = i32::from(unsafe { (*entry.ifa_addr).sa_family });
        let address = if family == libc::AF_INET {
            let sin = entry.ifa_addr.cast::<libc::sockaddr_in>();
            // SAFETY: the family says this is a `sockaddr_in`.
            let octets = unsafe { (*sin).sin_addr.s_addr }.to_ne_bytes();
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        } else if family == libc::AF_INET6 {
            let sin6 = entry.ifa_addr.cast::<libc::sockaddr_in6>();
            // SAFETY: the family says this is a `sockaddr_in6`.
            let octets = unsafe { (*sin6).sin6_addr.s6_addr };
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        } else {
            None
        };
        if let Some(address) = address {
            found.push((name, address));
        }
    }
    // SAFETY: `head` is the list `getifaddrs` filled and has not been freed.
    unsafe { libc::freeifaddrs(head) };
    found
}

#[cfg(not(unix))]
fn host_addresses() -> Vec<(String, IpAddr)> {
    Vec::new()
}

// ------------------------------------------------------- the refusal ----

/// Why an address may not be served from. Every variant names the address and
/// the reason, because a refusal an operator cannot act on is a crash with
/// better manners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The value is not an address at all.
    Unparsed(String),
    /// A `%zone` suffix, which this rule does not carry yet.
    ScopeId,
    /// `0.0.0.0` or `::`.
    Wildcard,
    /// An address of an interface this host holds a default route on.
    DefaultRoute { interface: String },
    /// A class that may not be served from: globally routable above all.
    NotServable(Reach),
    /// This host's routing could not be read, so the default-route question
    /// has no answer and the address is refused rather than guessed at.
    RoutingUnreadable,
}

/// A refused mesh address: which flag carried it, what was written, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub flag: &'static str,
    pub given: String,
    pub reason: Reason,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { flag, given, .. } = self;
        match &self.reason {
            Reason::Unparsed(why) => write!(
                out,
                "{flag} {given} is not an address: {why}. Write the address the mesh gave this \
                 machine, with an optional port — 10.0.0.4, 10.0.0.4:9440, [fd7a::1]:9440"
            ),
            Reason::ScopeId => write!(
                out,
                "{flag} {given} carries a %zone scope id, which this rule does not carry yet. \
                 Name a unique-local or private mesh address instead of a scoped link-local one"
            ),
            Reason::Wildcard => write!(
                out,
                "{flag} {given} is a wildcard: it would bind the default-route interface along \
                 with every other one, which is what invariant 3 and D11 refuse. Name the one \
                 address of the mesh this machine is on"
            ),
            Reason::DefaultRoute { interface } => write!(
                out,
                "{flag} {given} is an address of {interface}, which is this host's \
                 default-route interface: that is the interface a port forwarded on a router \
                 would land on, and invariant 3 refuses it (D11). Name an address of a private \
                 mesh interface — a tailnet or WireGuard address — or run this machine \
                 outbound-only, which needs no mesh address at all"
            ),
            Reason::NotServable(class) => write!(
                out,
                "{flag} {given} is {}, and a runner serves content only on a private mesh \
                 interface (invariant 3, D11). Name an address of the mesh this machine is on, \
                 or run it outbound-only",
                class.said()
            ),
            Reason::RoutingUnreadable => write!(
                out,
                "{flag} {given} cannot be judged on this host: neither /proc/net/route nor this \
                 host's own interface addresses could be read, so whether this address is on \
                 the default-route interface has no answer here. It is refused rather than \
                 guessed at. Loopback is always permitted, and a runner with no mesh address \
                 is outbound-only"
            ),
        }
    }
}

// ----------------------------------------------- the accepted address ----

/// A mesh address that passed the rule: what the endpoint will bind, and what
/// peers are told.
///
/// A listen port of `0` means the kernel picks one; the advertised endpoint
/// then carries the port the bind really got, which the endpoint step
/// resolves because it is the step that binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mesh {
    pub listen: SocketAddr,
    pub advertise: SocketAddr,
}

impl Mesh {
    /// Judge what the operator wrote.
    ///
    /// `listen` is `--mesh-listen`'s value, `advertise` is
    /// `--mesh-advertise`'s and defaults to the listen address. `routing` is
    /// what this host said about itself, read once.
    pub fn resolve(
        listen: &str,
        advertise: Option<&str>,
        routing: &HostRouting,
    ) -> Result<Self, Refused> {
        let listen = parse(LISTEN_FLAG, listen)?;
        permitted(listen.ip(), routing).map_err(|reason| Refused {
            flag: LISTEN_FLAG,
            given: listen.ip().to_string(),
            reason,
        })?;
        let advertise = match advertise {
            None => listen,
            Some(written) => {
                let advertise = parse(ADVERTISE_FLAG, written)?;
                // The class rule, and not the routing one: this is the
                // address a *peer* dials, and this host's route table says
                // nothing about what a peer can reach.
                let class = reach_of(advertise.ip());
                if !class.servable() {
                    return Err(Refused {
                        flag: ADVERTISE_FLAG,
                        given: advertise.ip().to_string(),
                        reason: match class {
                            Reach::Wildcard => Reason::Wildcard,
                            other => Reason::NotServable(other),
                        },
                    });
                }
                advertise
            }
        };
        Ok(Self { listen, advertise })
    }

    /// What a runner prints about its mesh on startup, beside every other cap
    /// it says it will enforce.
    pub fn said(&self) -> String {
        let port = if self.listen.port() == 0 {
            " on a port the kernel picks".to_owned()
        } else {
            String::new()
        };
        format!(
            "content may be served to peers on {}{}, advertised as {} ({}); nothing else of \
             this runner ever listens (invariant 3, D11)",
            self.listen.ip(),
            port,
            self.advertise,
            reach_of(self.listen.ip()).said()
        )
    }
}

/// `IP`, `IP:PORT`, or `[IPv6]:PORT`. A bare address means port `0`: the
/// kernel picks, and the bind reports what it got.
fn parse(flag: &'static str, written: &str) -> Result<SocketAddr, Refused> {
    let written = written.trim();
    let refuse = |reason| Refused {
        flag,
        given: written.to_owned(),
        reason,
    };
    if written.contains('%') {
        return Err(refuse(Reason::ScopeId));
    }
    if let Ok(address) = written.parse::<SocketAddr>() {
        return Ok(SocketAddr::new(plain(address.ip()), address.port()));
    }
    match written.parse::<IpAddr>() {
        Ok(address) => Ok(SocketAddr::new(plain(address), 0)),
        Err(error) => Err(refuse(Reason::Unparsed(error.to_string()))),
    }
}

/// `::ffff:10.0.0.1` as `10.0.0.1`, everything else unchanged.
///
/// Done where the value is parsed rather than where it is classified, so that
/// the one address this host really holds is the one every later step compares
/// against: the first version classified the mapped form correctly and then
/// looked it up in the interface list under the mapped spelling, found no
/// match, and **accepted** the default-route address of the machine it was
/// running on. `::ffff:` was a way round the rule for exactly as long as that
/// lookup was the one thing not normalised.
fn plain(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(inner) => match inner.to_ipv4_mapped() {
            Some(mapped) => IpAddr::V4(mapped),
            None => address,
        },
        IpAddr::V4(_) => address,
    }
}

/// The rule itself, over one address and one reading of the host.
///
/// Order matters and is the order of the refusals in D11: a wildcard is said
/// to be a wildcard, a public address is said to be public even when it is
/// also on the default route, and only a servable class is then asked which
/// interface it sits on.
fn permitted(address: IpAddr, routing: &HostRouting) -> Result<(), Reason> {
    let class = reach_of(address);
    if class == Reach::Wildcard {
        return Err(Reason::Wildcard);
    }
    if !class.servable() {
        return Err(Reason::NotServable(class));
    }
    // Loopback, before the routing is consulted at all: see the module
    // header. It is reachable from nothing off this machine, and it is the
    // only mesh a single workstation running the two-runner end-to-end has.
    if class == Reach::Loopback {
        return Ok(());
    }
    if !routing.readable() {
        return Err(Reason::RoutingUnreadable);
    }
    match routing.default_route_interface_of(address) {
        Some(interface) => Err(Reason::DefaultRoute {
            interface: interface.to_owned(),
        }),
        None => Ok(()),
    }
}

/// Whether a runner with this configuration may serve content right now.
///
/// `serves_content` is [`crate::Runner::serves_content`]'s answer, passed in
/// rather than recomputed: that method is where `off` and draining are
/// already read (D11's third condition, E-33), and a second reading of the
/// switch is how `off` comes to mean two things. No mesh address means
/// outbound-only, which is never serving.
#[allow(dead_code)] // The endpoint that consults it is the next step.
pub fn may_serve(mesh: Option<&Mesh>, serves_content: bool) -> bool {
    mesh.is_some() && serves_content
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host with one mesh interface and one ordinary one, the ordinary one
    /// carrying the default route. The shape every fixture below judges
    /// against, so that no assertion here depends on the machine it runs on.
    fn two_interfaces() -> HostRouting {
        HostRouting::declared(
            vec!["eth0".to_owned()],
            vec![
                ("lo".to_owned(), "127.0.0.1".parse().unwrap()),
                ("eth0".to_owned(), "192.168.1.42".parse().unwrap()),
                ("tailscale0".to_owned(), "100.84.1.7".parse().unwrap()),
                ("wg0".to_owned(), "10.9.0.3".parse().unwrap()),
                ("wg0".to_owned(), "fd7a:1::3".parse().unwrap()),
            ],
        )
    }

    fn resolve(written: &str) -> Result<Mesh, Refused> {
        Mesh::resolve(written, None, &two_interfaces())
    }

    #[test]
    fn a_wildcard_is_refused_by_name() {
        for written in ["0.0.0.0", "0.0.0.0:9440", "::", "[::]:9440"] {
            let refused = resolve(written).expect_err(written);
            assert_eq!(refused.reason, Reason::Wildcard, "{written}");
            let said = refused.to_string();
            assert!(said.contains(LISTEN_FLAG), "{said}");
            assert!(said.contains("wildcard"), "{said}");
        }
    }

    #[test]
    fn an_address_of_the_default_route_interface_is_refused_by_name() {
        let refused = resolve("192.168.1.42:9440").expect_err("eth0 holds the default route");
        assert_eq!(
            refused.reason,
            Reason::DefaultRoute {
                interface: "eth0".to_owned()
            }
        );
        let said = refused.to_string();
        // The interface, the address, and what to do instead: a refusal that
        // names none of the three is one an operator cannot act on.
        assert!(said.contains("eth0"), "{said}");
        assert!(said.contains("192.168.1.42"), "{said}");
        assert!(said.contains("outbound-only"), "{said}");
    }

    #[test]
    fn a_globally_routable_address_is_refused_by_name() {
        for written in ["203.0.113.9", "8.8.8.8:9440", "[2606:4700::1111]:9440"] {
            let refused = resolve(written).expect_err(written);
            assert!(
                matches!(refused.reason, Reason::NotServable(_)),
                "{written}: {:?}",
                refused.reason
            );
            let said = refused.to_string();
            assert!(said.contains(LISTEN_FLAG), "{said}");
        }
        // And the one that is public rather than merely reserved says so.
        let said = resolve("8.8.8.8").expect_err("public").to_string();
        assert!(said.contains("globally routable"), "{said}");
    }

    #[test]
    fn a_multicast_or_reserved_address_is_refused_by_name() {
        for (written, word) in [
            ("224.0.0.1", "multicast"),
            ("255.255.255.255", "reserved"),
            ("240.1.2.3", "reserved"),
            ("[2001:db8::1]:9440", "reserved"),
        ] {
            let said = resolve(written).expect_err(written).to_string();
            assert!(said.contains(word), "{written}: {said}");
        }
    }

    #[test]
    fn loopback_is_accepted_and_that_is_deliberate() {
        let mesh = resolve("127.0.0.1:9440").expect("loopback is permitted on purpose");
        assert_eq!(mesh.listen, "127.0.0.1:9440".parse().unwrap());
        assert_eq!(mesh.advertise, mesh.listen);
        // Including where this host's routing cannot be read at all: the
        // two-runner end-to-end must not depend on a route table.
        let blind = Mesh::resolve("127.0.0.1:9440", None, &HostRouting::unreadable())
            .expect("loopback needs no routing reading");
        assert_eq!(blind.listen, mesh.listen);
        assert!(Mesh::resolve("[::1]:9440", None, &HostRouting::unreadable()).is_ok());
    }

    #[test]
    fn a_private_address_off_the_default_route_is_accepted() {
        // Each of the four classes a mesh really hands out, on an interface
        // that is not the default route's: RFC 1918, CGNAT (a tailnet),
        // unique-local, and a bare address with no port.
        for written in ["10.9.0.3:9440", "100.84.1.7:9440", "[fd7a:1::3]:9440"] {
            let mesh = resolve(written).unwrap_or_else(|refused| panic!("{written}: {refused}"));
            assert_eq!(mesh.listen.to_string(), written);
        }
        let bare = resolve("10.9.0.3").expect("a bare address is a port the kernel picks");
        assert_eq!(bare.listen.port(), 0);
        assert!(bare.said().contains("kernel picks"), "{}", bare.said());
    }

    #[test]
    fn the_same_address_is_refused_on_the_default_route_and_accepted_off_it() {
        // The one pair that proves the routing half of the rule is doing the
        // work: one address, two hosts, differing only in which interface
        // carries the default route.
        let address: IpAddr = "10.9.0.3".parse().unwrap();
        let on = HostRouting::declared(vec!["wg0".to_owned()], vec![("wg0".to_owned(), address)]);
        let off = HostRouting::declared(
            vec!["eth0".to_owned()],
            vec![
                ("eth0".to_owned(), "192.168.1.42".parse().unwrap()),
                ("wg0".to_owned(), address),
            ],
        );
        assert_eq!(
            Mesh::resolve("10.9.0.3:9440", None, &on)
                .expect_err("the mesh interface carries the default route here")
                .reason,
            Reason::DefaultRoute {
                interface: "wg0".to_owned()
            }
        );
        assert!(Mesh::resolve("10.9.0.3:9440", None, &off).is_ok());
    }

    #[test]
    fn a_single_nic_flat_lan_machine_has_no_permitted_address() {
        // Stated as a test rather than only in prose: this is the machine
        // shape the rule turns into an outbound-only runner, and it is a
        // consequence of D11 rather than a defect (module header).
        let flat = HostRouting::declared(
            vec!["eth0".to_owned()],
            vec![
                ("lo".to_owned(), "127.0.0.1".parse().unwrap()),
                ("eth0".to_owned(), "192.168.1.42".parse().unwrap()),
            ],
        );
        for written in ["192.168.1.42", "0.0.0.0", "203.0.113.9"] {
            assert!(
                Mesh::resolve(written, None, &flat).is_err(),
                "{written} would have been served from a flat LAN's only interface"
            );
        }
        // Its one way to serve anything on this machine is loopback, which is
        // reachable from nothing off it — so for a peer, it is outbound-only.
        assert!(Mesh::resolve("127.0.0.1:9440", None, &flat).is_ok());
    }

    #[test]
    fn a_host_whose_routing_cannot_be_read_refuses_everything_but_loopback() {
        let blind = HostRouting::unreadable();
        let refused = Mesh::resolve("10.9.0.3:9440", None, &blind)
            .expect_err("an unreadable host decides nothing, so it refuses");
        assert_eq!(refused.reason, Reason::RoutingUnreadable);
        let said = refused.to_string();
        assert!(said.contains("refused rather than guessed at"), "{said}");
        assert!(said.contains("outbound-only"), "{said}");
    }

    #[test]
    fn an_ipv4_mapped_address_is_judged_as_the_ipv4_address_inside_it() {
        // `::ffff:192.168.1.42` is the default-route address written another
        // way, and reading it as an exotic IPv6 address would have been a way
        // round the rule.
        let refused = resolve("[::ffff:192.168.1.42]:9440").expect_err("the same address");
        assert_eq!(
            refused.reason,
            Reason::DefaultRoute {
                interface: "eth0".to_owned()
            }
        );
        assert_eq!(reach_of("::ffff:8.8.8.8".parse().unwrap()), Reach::Global);
        assert_eq!(reach_of("::ffff:0.0.0.0".parse().unwrap()), Reach::Wildcard);
    }

    #[test]
    fn a_value_that_is_not_an_address_and_a_scope_id_are_refused_by_name() {
        let said = resolve("eth0")
            .expect_err("an interface name is not an address")
            .to_string();
        assert!(said.contains("is not an address"), "{said}");
        let said = resolve("fe80::1%eth0").expect_err("a scope id").to_string();
        assert!(said.contains("%zone"), "{said}");
    }

    #[test]
    fn the_advertised_address_carries_the_class_rule_and_defaults_to_the_listen_address() {
        let routing = two_interfaces();
        let mesh = Mesh::resolve("10.9.0.3:9440", Some("100.84.1.7:9440"), &routing)
            .expect("a mesh address peers can dial");
        assert_eq!(mesh.listen, "10.9.0.3:9440".parse().unwrap());
        assert_eq!(mesh.advertise, "100.84.1.7:9440".parse().unwrap());
        let defaulted = Mesh::resolve("10.9.0.3:9440", None, &routing).expect("accepted");
        assert_eq!(defaulted.advertise, defaulted.listen);
        for written in ["0.0.0.0:9440", "8.8.8.8:9440"] {
            let refused = Mesh::resolve("10.9.0.3:9440", Some(written), &routing)
                .expect_err("an address no peer should be told to dial");
            assert_eq!(refused.flag, ADVERTISE_FLAG, "{written}");
        }
        // Not the routing rule, though: the default-route interface of *this*
        // host says nothing about what a peer can reach, so an advertised
        // address is judged on its class alone.
        assert!(Mesh::resolve("10.9.0.3:9440", Some("192.168.1.42:9440"), &routing).is_ok());
    }

    #[test]
    fn the_switch_still_decides_whether_anything_is_served() {
        // D11's third condition, read through `Runner::serves_content` and
        // not restated here.
        let mesh = resolve("10.9.0.3:9440").expect("accepted");
        assert!(may_serve(Some(&mesh), true));
        assert!(!may_serve(Some(&mesh), false));
        assert!(!may_serve(None, true), "no mesh address is outbound-only");
    }

    #[test]
    fn the_route_tables_are_parsed_as_the_kernel_writes_them() {
        let v4 =
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                  eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                  eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n\
                  wg0\t0000090A\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";
        assert_eq!(default_route_interfaces_v4(v4), vec!["eth0".to_owned()]);
        // The third line is the one this host really has: an unreachable
        // `::/0` on `lo`, metric `ffffffff`, flags `00200200` —
        // `RTF_REJECT | RTF_NONEXTHOP`. Counting it made `lo` a
        // default-route interface, which is the defect `route_is_usable`
        // exists for, so the fixture carries it rather than describing it.
        let v6 = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
                  fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0\n\
                  fd7a000100000000000000000000000000 40 00000000000000000000000000000000 00 \
                  00000000000000000000000000000000 00000100 00000000 00000000 00000001 wg0\n\
                  00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
                  00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
        assert_eq!(default_route_interfaces_v6(v6), vec!["eth0".to_owned()]);
        // And the same for IPv4: a rejecting default route is not a route
        // anything takes.
        assert!(default_route_interfaces_v4(
            "Iface\tDestination\n\
             lo\t00000000\t00000000\t0201\t0\t0\t0\t00000000\t0\t0\t0\n"
        )
        .is_empty());
        assert!(
            route_is_usable("0003") && !route_is_usable("00200200") && !route_is_usable("0000")
        );
        // A table with no default route in it yields none, rather than the
        // first interface it saw.
        assert!(default_route_interfaces_v4(
            "Iface\tDestination\n\
             eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n"
        )
        .is_empty());
    }

    /// The refusal, against the machine this test is running on rather than
    /// against a fixture: whatever address this host holds on its own
    /// default-route interface must be refused, by name, with that
    /// interface's name in the sentence.
    ///
    /// This is what keeps the two halves honest together — the rule above and
    /// the reading of the host below it. A host with no default route at all
    /// proves nothing here and says so out loud (D27) rather than passing.
    #[test]
    fn the_rule_refuses_this_hosts_own_default_route_address() {
        const TEST: &str = "the_rule_refuses_this_hosts_own_default_route_address";
        let routing = HostRouting::read();
        if !routing.readable() {
            herd_runner::testing::skipped(
                TEST,
                "this host's routing table and interface addresses could not be read, so it \
                 holds no default-route address to refuse",
            );
            return;
        }
        let held = routing.default_route_addresses();
        let Some((interface, address)) = held
            .iter()
            .find(|(_, address)| reach_of(*address).servable())
            .cloned()
        else {
            herd_runner::testing::skipped(
                TEST,
                "this host holds no servable address on a default-route interface, so the \
                 default-route refusal has nothing to fire on here",
            );
            return;
        };
        let written = format!("{address}");
        let refused = Mesh::resolve(&written, None, &routing)
            .expect_err("an address of this host's default-route interface");
        assert_eq!(
            refused.reason,
            Reason::DefaultRoute {
                interface: interface.clone()
            },
            "{written} is on {interface}, which carries this host's default route"
        );
        let said = refused.to_string();
        assert!(said.contains(&interface), "{said}");
        assert!(said.contains(&written), "{said}");
    }
}
