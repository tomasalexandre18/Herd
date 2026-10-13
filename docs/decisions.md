# Decisions

Append-only. One dated entry per decision: context, choice, alternative
rejected. Newest at the bottom. Keep entries short; this is not an ADR store.

---

## 2026-10-12 — D1 Language and runtime: Rust

**Context.** Herd is one service plus long-lived native clients that run on
other people's machines. A runner must have a small, predictable footprint and
must never be the reason a workstation falls over. The protocol will be
versioned and will grow; drifting message shapes between hub and runner is the
most likely source of silent bugs.

**Choice.** Rust, stable toolchain, single cargo workspace. `serde` for the
protocol, `tokio` for async, `tokio-tungstenite` for WebSocket, `clap` for
CLIs. Toolchain pinned by a Nix flake (nothing installed globally on the
server).

**Alternative rejected.** Go — faster to write, equally good single binary,
`GOMEMLIMIT` makes the footprint argument weak. Rejected because the typed
protocol is the core asset here: exhaustive matching on message enums turns
protocol drift into compile errors, and a GC-free runner is easier to argue
about when the promise is "never takes your machine down". Cost accepted:
roughly 1.3× the development effort of Go on network plumbing.

**Reversibility.** High for libraries, low for the language. If Rust proves
too slow to iterate in during M1–M2, say so and we switch before M3.

---

## 2026-10-12 — D2 Vocabulary: hub / runner / requester / job

**Context.** "Client" is ambiguous in this project: the machines that execute
work are clients of the WebSocket service, but so is the thing that asks for
work. Two different roles behind one word will leak into type names.

**Choice.** **hub** = the service. **runner** = a client offering capacity and
executing jobs. **requester** = whoever submits a job. **job** = one requested
unit of work. The word "client" is not used in code or docs.

**Alternative rejected.** Keeping "server/client" as in the original brief —
rejected because the server machine also hosts a runner, so "the server is
also a client" needs a sentence of explanation every time.

---

## 2026-10-12 — D3 Decomposition: four crates

**Context.** Hub, runner and requester must be separately deployable; the
protocol must not be duplicated between them.

**Choice.**

- `herd-proto` — message types, serde, protocol version constant. No I/O.
- `herd-hub` — listens, authenticates runners, tracks their state, dispatches
  jobs, relays logs. Holds all scheduling logic.
- `herd-runner` — dials the hub, declares capabilities, executes jobs under
  caps, streams logs. Holds all host-safety logic.
- `herdctl` — requester CLI: submit a job, follow its logs, cancel it, exit
  with the job's exit code.

**Alternative rejected.** A single binary with subcommands — rejected because
a runner on someone else's machine should not ship the hub's scheduler, and
the trust boundary between hub and runner should be visible in the build.

---

## 2026-10-12 — D4 Transport: WebSocket, JSON, runner dials out

**Context.** The hub is a normal web app behind the existing reverse proxy on
the NixOS server. Clients sit behind NAT, on WSL, and must open no port.

**Choice.** One WebSocket connection per runner and per requester, initiated
by the client. TLS terminated by the reverse proxy (`wss://` outside,
plain `ws://` inside docker compose and in tests). Messages are JSON text
frames, one message per frame, serde-tagged enums (`{"type": "...", ...}`).
First message is a `Hello` carrying the protocol version and a bearer token;
mismatched versions are refused with a clear reason. Log output travels as
UTF-8 strings (lossy conversion at the runner).

**Alternative rejected.** gRPC — bidirectional streaming needs HTTP/2 end to
end through the proxy, adds codegen to the build, and buys little here.
Raw TCP with custom framing — loses TLS and proxy handling for free. Binary
codec (MessagePack/CBOR) — premature; serde makes it a one-line swap if log
volume ever justifies it, and JSON frames are debuggable with `websocat`.

---

## 2026-10-12 — D5 Deployment shape

**Context.** The server is NixOS, apps run in docker compose, TLS already
terminated there. Runners live on machines whose owners must be able to cut
them in seconds.

**Choice.** The hub ships as a container image built from the Nix flake and
runs under docker compose like any other app on the server, with no state
outside its own volume. A runner is a plain long-lived binary started by the
machine's owner (systemd user service on Linux, a shell command on WSL);
stopping it is `Ctrl-C` or `systemctl --user stop`, and it drains in ≤ 5 s.

**Alternative rejected.** Running runners as containers too — rejected for the
WSL and GPU/KVM cases, where the point is to use the host's real hardware.

---

## 2026-10-12 — D6 Deliberately left open

These wait for the chatOnS requirement sheet (incoming GitHub issue). Nothing
in M1–M3 may assume an answer, and no heavy dependency may be added for them.

- **Authentication** beyond a shared bearer token per runner (per-runner keys,
  mTLS, revocation).
- **Scheduling policy**: how capabilities are declared and matched, how
  priority interacts with fairness, exclusive hardware leases.
- **Host isolation mechanism**: cgroup v2 scope vs other mechanisms, and the
  fallback when cgroup v2 delegation is unavailable (WSL needs checking).
- **Logs and results retention**: in-memory only, on disk, for how long, and
  whether a result is retrievable after the requester disconnects.
- **Job input model**: how a commit reaches the runner (git fetch by the
  runner, archive pushed through the hub, shared cache).

---

## 2026-10-12 — D7 Blob transfer channel (amends D4)

> **Superseded the same day by D10.** D7 was right that payloads do not belong
> in JSON control frames and wrong about the topology: it still routed them
> through the hub. Kept for the record.

**Context.** Issue #1 arrived with the first consumer's requirement sheet.
E-12 asks Herd to store *packs*: build artefact sets of hundreds of megabytes,
up to about 1.5 GB, fetched by a runner on another machine. D4 says every
message is a JSON text frame on the WebSocket, which would mean base64 through
the control channel — tens of percent of overhead, a head-of-line block on the
connection that also carries heartbeats, and no resumable transfer. The sheet
explicitly leaves "the transport's details beyond `wss://`" to the Herd
project, so this is ours to settle.

**Choice.** Keep the WebSocket as the control channel exactly as D4 describes
it, and add a plain HTTP channel on the hub for blob `GET` and `PUT`, behind
the same reverse proxy, authenticated by the same per-runner token. The
WebSocket carries references (manifest hashes, pack keys), never payloads.

**Alternative rejected.** Binary WebSocket frames with a chunking protocol —
rejected because it reimplements range requests, resumption and caching that
HTTP already has, on the one connection whose liveness we depend on.

**Reversibility.** Easy to extend (a direct runner-to-runner path, E-55, is a
later addition on top), hard to remove once runners depend on the endpoint.

---

## 2026-10-12 — D8 Hub persistence: a directory per job, no database

**Context.** E-44 requires that a service restart lose no job and no result,
with the queue intact and runners delivering results of jobs that were running
across the restart. E-15 requires the full job record on disk — fields, state
transitions with timestamps, log, kept files — and E-53 requires the consumer
to be able to read a job's record while it runs. This is the single most
load-bearing structural choice: if the hub starts life with its state in
memory, every scheduling milestone built on top gets rewritten.

**Choice.** The hub's state is files from the first commit of M2. One directory
per job: `job.json` written by write-to-temp-then-rename, `events.jsonl`
appended for transitions, the log as a file, kept files beside it. The queue is
rebuilt by scanning at startup. A runner holds a finished result until the hub
acknowledges it, and redelivers after a restart.

**Alternative rejected.** SQLite — rejected because the records must be plain
readable files for the consumer (E-53) and for after-the-fact inspection
(E-54), atomic rename is already the primitive E-17 demands for artefacts, and
the expected volume (hundreds of jobs a day) does not need a query engine.
If search (E-39) later justifies one, it can be an index *derived* from the
files, never the source of truth.

**Reversibility.** The on-disk layout becomes a compatibility surface the
moment the consumer reads it; the record format therefore carries a version
from the start.

---

## 2026-10-12 — D9 Warm caches: Herd carries one, Herd is not one

**Context.** E-14 asks that the consumer's warm caches (one immutable built
tree per main commit, ~11 GB, 57-700 MB of unique content between consecutive
mains, ~27 GiB for the five kept) be available on a build runner before its
job starts, and offers two options: Herd carries them as artefacts, or Herd
lets the consumer's own replication run as a preparation step and reports its
time. Alexa's ruling (2026-10-12): the cache is load-bearing for the first
consumer — there are few build machines and a cold build is over 30 minutes —
so "Herd does not manage it" must not degrade into "Herd ignores it"; Herd
must be able to *carry* a cache system even if it does not own one.

**Choice.** Herd grows no cache engine, and no concept of "a cache", but three
mechanisms must be good enough to carry someone else's:

1. **The artefact store is generic.** Content-addressed per file (D7's channel,
   per-file addressing), so a consumer may publish cache diffs through it as
   ordinary packs and a 57-700 MB diff transfers only the blobs the target
   runner lacks. We do not model caches; we make the store good enough that a
   cache rides it.
2. **The prepare step is first class.** A declared command may carry a prepare
   step naming the cache keys it needs and leaves; it runs before the job on
   the chosen runner, under the same confinement, and its duration is recorded
   against the job.
3. **Placement is key-aware.** A runner reports the keys it holds in its health
   report; among runners that are eligible and available, the hub prefers one
   that already holds the key or has the shortest prepare (E-23). This never
   overrides E-19: a job still waits for its machine rather than moving to a
   colder one.

**Alternative rejected.** Herd replicating the caches itself — rejected because
a bespoke 27 GiB replication engine, with its own diffing and eviction, is the
consumer's cache engine living inside a scheduler. If mechanism 1 proves too
slow in practice, the fix is to measure it and improve the store, not to grow
a second subsystem.

**Reversibility.** High: all three mechanisms are useful regardless, and none
assumes a cache exists.

---

## 2026-10-12 — D10 The data path: Herd is never in the byte path
### (supersedes D7's topology)

**Context.** Alexa, 2026-10-12: *data must not transit between runners through
Herd — that is not its role, it orchestrates; the exchange should be direct,
over something like a tailnet; HTTP through the hub crosses too many layers
and is counterproductive locally.* He is right, and D7 only fixed half the
problem: it moved packs off the JSON control frames but left them going
through the hub. Three cases make it plain. The hub would become a bandwidth
bottleneck and a disk sink for content it has no use for. Every transfer would
cost two hops instead of one. And on the server — which hosts both the hub
container and a runner — fetching a pack would mean pushing 1.5 GB through a
container boundary and a reverse proxy to reach a file already on that disk.
Their own E-55 asks for peer exchange as a "could"; this makes it the default.

**Choice.** Three things that D7 conflated are now separate:

1. **Control plane — Herd's.** The hub holds manifests (a pack is a list of
   content hashes keyed by (commit, key)), knows which runner holds which
   blobs, decides placement, and issues a short-lived scoped capability for a
   (blob, peer) pair. All of it metadata: kilobytes.
2. **Data plane — not Herd's.** Each runner keeps a per-file content-addressed
   store under its data root and advertises a fetch endpoint. A runner needing
   a pack asks the hub who holds it and fetches **directly from that peer**,
   only the hashes it lacks. Three paths in order: *same host* — hardlink or
   reflink in the local store, zero bytes on any network; *peer reachable* —
   direct fetch; *outbound only* — a runner that accepts no inbound connection
   (E-34) pushes to and pulls from a keeper and never serves.
3. **Custody — Herd's policy, a runner's disk.** A key family names one or
   more **keepers** that pin what retention keeps, so a pack outlives the
   machine that built it and survives that machine being switched off. The
   server's runner is the default keeper. A pack with no reachable holder is a
   named, visible state, never a silent failure.

**The network is not ours.** Herd must not depend on a VPN product. A
coordination service we do not run would breach invariant 1 (no cloud), and a
network layer inside Herd would breach it again. Herd consumes an advertised
endpoint — a tailnet address, a LAN address, a hostname are all the same
string to it. A tailnet is an excellent deployment choice for these machines;
it is not a Herd dependency, and Herd works on a bare LAN with no mesh at all.

**What this costs.** Peer liveness, partial holders and "who actually holds
this blob" are harder to debug than one central store. Custody and the visible
no-holder state are what pays for it. Content addressing per file (Q4) is what
makes "fetch the diff" free: a peer fetches the hashes it lacks, so no delta
protocol and no rsync is ever needed.

**Alternative rejected.** The hub as a relay of last resort for peers that
cannot reach each other — deliberately not built. It is the design we just
removed, it would be used quietly whenever the direct path broke, and it would
hide exactly the failures we need to see. An outbound-only runner reaches a
keeper instead; if two runners genuinely cannot reach any common keeper, that
is a network fact to fix, reported by name.

**Resolved the same day by D11.** The question was: the owner's rule is that
her PC "accepts no inbound connection", yet on a tailnet serving a blob means
listening on the mesh interface — so is the PC a serving peer or
outbound-only? Answered: a serving peer, bound to the mesh interface only.

---

## 2026-10-12 — D11 What "no inbound connection" means (resolves D10)

**Context.** D10 left one question for Alexa: E-34 says the owner's PC
"accepts no inbound connection", but serving a blob to a peer means listening
on a socket. His ruling (2026-10-12): *the tailnet is fine — the rule is no
port opened on a home router; that would be too much. If the network layer can
go through a tailnet, it is approved.*

**Choice.** E-34 is read as **no port opened on a router, nothing reachable
from the public internet, no port forwarding** — not as "no socket at all". A
runner may therefore serve content to peers, under three conditions that are
tested, not documented:

1. The fetch endpoint binds to **one named interface or address** given in the
   runner's configuration — the mesh address. Never `0.0.0.0`, never `::`,
   never a default-route interface. A configuration that would bind a public
   or default interface is refused at startup, the way a bad data root is
   (E-45).
2. Herd still builds and requires no network. The mesh is the machine owner's,
   set up outside Herd; Herd only receives the address to bind and the address
   to advertise. With no mesh, a runner is outbound-only and nothing breaks.
3. The owner's switch (E-33) stops the fetch endpoint too: `off` means the
   machine serves nothing, not just claims nothing.

**Consequence.** The owner's PC becomes a serving peer rather than
outbound-only, so a laptop wanting a pack built on the PC fetches it straight
from the PC instead of detouring through the keeper. The outbound-only path
stays in the design for any machine without a mesh.

**To verify on the real machine (M17), not assumed.** Whether the mesh is
reachable from inside WSL2 by running the mesh daemon in WSL (needs
`/dev/net/tun`, and their WSL has kernel 6.18 and systemd) or by WSL's
mirrored networking mode sharing the Windows interface. Both are plausible;
the join recipe records whichever actually works, and the automated test
asserts the binding rule above regardless of which it is.

**Invariant amended.** CLAUDE.md invariant 3 was "no inbound port, no relay,
no tunnel on client machines; the hub is the only thing that listens". That is
now false and has been rewritten (Alexa's approval, 2026-10-12).

---

## 2026-10-12 — D12 Toolchain pinning: nixpkgs' stable rust, no overlay

**Context.** M1 had to make `nix develop` the only source of tools (Alexa,
2026-10-12). Rust can come from nixpkgs directly or from an overlay (fenix,
oxalica) that offers exact versions, nightlies and extra components.

**Choice.** nixpkgs' stable rust, pinned by `flake.lock`. The current pin
resolves to rust **1.98.1**. No overlay, no `rust-toolchain.toml`, no rustup.

**Alternative rejected.** An overlay — rejected for the first commit: it adds
an input to trust and a second binary cache to depend on, in exchange for
version control we do not need. We build on stable and have no component that
nixpkgs lacks. If we ever need a pinned exact version or a nightly, the
overlay goes in then, as one input and one line.

**Reversibility.** High. `flake.lock` is the pin; moving it is a commit.

---

## 2026-10-12 — D13 One refusal frame, always last

**Context.** M1 needed a way to say "no" for a version mismatch, and then the
same shape turned out to fit "no runner is connected", "a runner is already
connected" and protocol misuse.

**Choice.** `Refused { reason }` is the hub's single, general "no", and it is
always the last frame on that connection: after a refusal the hub closes. The
reason is a sentence meant for a human, carrying its figures.

**Consequence to watch.** This is a *terminal* no. E-04's explained waits are
the opposite — a job that waits is not refused, and it must keep receiving
reasons on an open connection. M8 therefore needs a distinct, non-terminal
message and must not reach for `Refused`. Noted here so that it is not
discovered by abuse of this frame.

---

## 2026-10-12 — D14 The hub's endpoint and the CLI surface

**Context.** The hub needs a default listen address, and nothing in the
requirement sheet specifies one.

**Choice.** `127.0.0.1:8787` by default, overridden by one `--listen` option
or `HERD_LISTEN` — one address-and-port value, never a separate host and port.
Loopback by default on purpose: the hub is exposed by the reverse proxy in
M16, never by binding a public interface itself. Clients take `--hub <url>`
with a `ws://` or `wss://` scheme, so the transport is explicit at the call
site rather than inferred.

**Alternative rejected.** Defaulting to `0.0.0.0` for convenience in
development — rejected because a default that listens publicly is the kind of
thing that reaches a server once and stays.

---

## 2026-10-12 — D15 A job's end is an outcome, not an exit code

**Context.** M1 first reported a job's end as `exit_code: i32`, with 127 for
"could not be started" and -1 for "signalled". That field has to carry the
consumer's own code untouched — E-02 relies on 0, 1 and 75 — and 127 is a
shell convention a consumer's command can produce itself. Worse, a job can end
in ways that are not an exit code at all: M3's lost runner, M8's refusals,
M9's deadline.

**Choice.** `JobFinished` carries a tagged `outcome`, today `exited { code }`,
`signalled { signal }` and `could_not_start { reason }`. **A new way for a job
to end is a new variant, never a magic number.** Every variant has named
fields so adding one stays additive on the wire; an unknown variant is a
decode error naming it, never a job silently read as having exited.
`JobStore` keeps the outcome whole, so M2 persists it as it is.

**Alternative rejected.** Keeping the integer and documenting the sentinels —
rejected because the collision is undetectable by construction, and because
M2 would have persisted it before M3 forced the change anyway.

**`PROTOCOL_VERSION` stays at 1.** The shape changed incompatibly, but the
version exists to catch a mismatch between deployed peers and nothing has ever
been deployed. Bumping would record a break against no peer and make the first
shipped version 2 for nothing.

---

## 2026-10-12 — D16 herdctl's exit code

**Context.** E-02 requires the consumer's verdict and exit code to reach the
caller unchanged, while Herd still needs a way to say "I could not run this".

**Choice.** `exited` — that exact code, including 0, 1, 75 and 127. `signalled`
— `128 + signal`, the shell convention, so no sentinel is needed. `could_not
_start` — **125**, with the reason on **stderr** worded as Herd failing rather
than the job failing; stdout carries only the job's own output, and is left
empty when the job never ran.

**The one imperfection, accepted.** 125 is not collision-free: a job may exit
125 itself. What separates them is the outcome on the wire, and in the terminal
the stderr sentence — a test asserts that a job exiting 125 does *not* get it.
A reserved code that no program can produce does not exist, so the honest fix
is the typed outcome above, not a cleverer number.


---

## 2026-10-12 — D17 The minimum reconnect belongs to M2, not M3

**Context.** M2's own test suite was green while the real binaries lost
the job entirely: killing the hub mid-job made the runner process exit,
its job child die, and the record sit in `running` forever with `herdctl
wait` hanging. A runner that dies on disconnect can never redeliver, so
M2's held-result-until-acknowledged machinery was unreachable and E-44
was untestable.

**Choice.** M2 gains exactly the minimum: the runner survives a dropped control
connection, redials with a simple bounded retry, keeps its running job alive
across the gap, and redelivers a held result until acknowledged. M3 keeps
the backoff policy, the ~15 s health report, the ~60 s silence-means-lost
threshold, requeue-once at priority, cancel propagation and drain ≤ 5
s. A job whose attempt was interrupted is recorded as interrupted by name,
never left silently `running`.

**Alternative rejected.** Merging M2 and letting M3 close E-44 — that
would have put a milestone documented as done, and a permanently wedged
record, on `main`.

---

## 2026-10-12 — D18 Log chunks carry bytes, not a string

**Context.** A job printing `0xFF` had it rewritten to U+FFFD on the wire
and on disk, while NUL survived; a compiler can emit non-UTF-8 and E-15
promises the full record.

**Choice.** Log payloads travel as bytes (base64 inside the JSON frame), so the
on-disk log is byte-identical to what the process wrote. `PROTOCOL_VERSION`
stays 1 because nothing is deployed — this is the cheapest moment this
change will ever cost.

**Alternative rejected.** Declaring logs UTF-8 with replacement, which
silently falsifies a job's output.

---

## 2026-10-12 — D19 A result the hub cannot persist is never destroyed

**Context.** A job directory the hub could not write made it log the failure,
drop the runner's connection and lose the outcome permanently; a full disk
would have killed every runner at once.

**Choice.** The hub acknowledges only what it has fsynced, never acknowledges
what it failed to write, retries, and never drops a runner because of its own
storage failure — the runner keeps holding. A stalled job is recoverable;
a destroyed verdict is not.

**Alternative rejected.** Failing the job, which converts a hub problem
into a false verdict.

---

## 2026-10-12 — D20 Exit 1 means the job failed

**Context.** "No runner is connected", "the runner was lost while the job
was running", "the hub closed the connection without accepting the job"
and a dial failure all exited 1, indistinguishable from a job whose tests
failed — breaking the promise E-02 and E-43 rest on.

**Choice.** D16's single sentinel widens from "could not start" to "Herd
cannot give you a verdict": every Herd-level failure exits **125** with
the reason on stderr and stdout empty; the job's own code is worn only when
there is a real verdict.

**Alternative rejected.** A distinct code per failure mode, which multiplies
sentinels colliding with real exit codes.

---

## 2026-10-12 — D21 One hub per state root

**Context.** Two hubs shared a state root happily and the derived index
went stale; and a state root whose `jobs/` was unwritable started fine,
then failed every submission with the reason only in the hub's log.

**Choice.** Lock the state root at startup, refusing a second hub by naming
the holder, and extend the startup write probe to `jobs/` so the refusal
happens at startup with the path named.

**Alternative rejected.** Trusting operators not to point two hubs at
one directory.

---

## 2026-10-12 — D22 The log file exists from the start, and `wait` prints it

**Context.** A job producing no output left no log file at all, so
a consumer reading the record got ENOENT; and `herdctl wait` printed no
output whatsoever, so a detached requester could only retrieve a log by
reading the hub's state directory — impossible for a remote requester.

**Choice.** Create the log when the job starts, even empty, and make `wait`
print the job's log the way `run` does.

**Alternative rejected.** Documenting the state directory as the retrieval
path, which assumes every requester shares the hub's filesystem.

---

## 2026-10-12 — D23 The memory ceiling falls back to RLIMIT_AS, loudly

**Context.** cgroup v2 `MemoryMax` needs a delegated subtree, which a plain
login session does not have.

**Choice.** Where delegation is unavailable the runner sets `RLIMIT_AS`
instead and announces the fallback and its reason at startup — a real
kernel cap, so invariant 2 still holds. It is weaker on purpose and the
weakness is named: per process rather than per tree, and it makes an
allocation fail rather than killing the job.

**Alternative rejected.** A silent fallback, or refusing to run — a
runner that will not start on an undelegated host is useless on exactly
the machines Herd targets.

---

## 2026-10-12 — D24 A cap breach is one outcome variant

**Context.** D15 made a job's end a tagged outcome so a new way to end is
a new variant.

**Choice.** Every cap ends a job as `StoppedByCap { cap, limit }` with a
nested `Cap` value naming which one — memory, time, log, disk. A new cap
is a new `Cap`, never a new way to end.

**Alternative rejected.** One outcome variant per cap, which would reopen
the magic-number problem D15 closed.

---

## 2026-10-12 — D25 A cap breach exits 124

**Context.** D16 and D20 fix the exit-code table, and "Herd stopped your
job at a limit" is neither the job's verdict nor an absence of verdict.

**Choice.** `herdctl` exits **124** for every cap breach — `timeout(1)`'s
code, generalised — with the cap in the typed outcome and named on
stderr. The table is now: the job's own code when there is a verdict,
128+signal when signalled, 124 stopped by a cap, 125 when Herd cannot give
a verdict.

**Alternative rejected.** A code per cap.

---

## 2026-10-12 — D26 `herd-runner` depends on `libc`

**Context.** Confinement needs `setsid`, `setpriority`, `ioprio_set`,
`setrlimit` and a write to `cgroup.procs` between fork and exec.

**Choice.** Take `libc` directly rather than a confinement crate, so what
the runner does to a host is visible in our own code.

**Alternative rejected.** A sandboxing crate, which would hide the host
contract behind someone else's abstraction.

---

## 2026-10-12 — D27 A skipped assertion is never a quiet pass

**Context.** The cgroup ceiling cannot be asserted without delegation, so
some assertions are real on a delegated host and impossible on GitHub Actions.

**Choice.** Such an assertion prints a `HERD-SKIPPED` line that a script
lifts into a CI warning and the run summary. Invariant 5 says done means
a passing test, so a test that did not really run must be impossible to
mistake for one that did.

**Alternative rejected.** Failing on an undelegated host, which would make
CI red for a host property rather than a defect.

---

## 2026-10-12 — D28 E-45 is the cooperative version, and the limit is recorded

**Context.** "Nothing a job writes lands outside the data root" cannot be
enforced without a mount namespace, and a mount namespace needs privilege
that E-46 forbids the runner to hold. The two expectations genuinely conflict.

**Choice.** Herd confines what it directs — the job's working directory,
`HOME`, `TMPDIR` and `XDG_*` all inside the data root, the job's environment
built rather than inherited, any escaping path refused by name, a disk
budget on the workspace — and states plainly that a job determined to write
elsewhere with the runner's own permissions can. This is a recorded limit,
not an oversight, and it is the first thing to raise with the first consumer.

**Alternative rejected.** Holding privilege to build a namespace, which
breaks E-46 and invariant 2's spirit.

---

## 2026-10-13 — D29 The log file is arrival-ordered

**Context.** The single `log` file interleaves stdout and stderr by
arrival, so `echo OUT1; echo ERR1 >&2; echo OUT2` may legitimately
record `OUT1 OUT2 ERR1`, and the store was discarding the per-job
sequence numbers that decision D4 introduced on day one.

**Choice.** The log is the job's output as a terminal would have shown
it, and promises no per-stream exactness; the sequence numbers serve
**gap detection**, not reordering.

**Alternative rejected.** Reconstructing true interleaving, which would
cost a second index on every chunk of a three-million-line job for a
transcript nobody reads that way.

---

## 2026-10-13 — D30 An unmeasurable workspace is a cap failure, never a success

**Context.** A job could blind the runner's disk accounting with
`chmod 300` on a subdirectory, be reported `Exited{0}`, and leave
100 MB on the host permanently — invariant 2.

**Choice.** The runner restores the access it owns in order to
measure, and if a figure cannot be measured the job ends as
`stopped_by_cap{disk}` with the figure **absent**; a measured zero and
an unmeasured figure are different values and must never be confused.

**Alternative rejected.** Treating an unreadable directory as zero
bytes, which is how the bypass worked.

---

## 2026-10-13 — D31 A breached cap outranks the child's exit status

**Context.** A job that breached its log cap and then exited promptly
was reported `Exited{0}` with the log silently truncated, and one
that kept writing surfaced the cap's own `SIGPIPE` as the job's
verdict. A build service that reports a truncated build as green has
no worse failure mode.

**Choice.** The cap is re-read after the child is gone and always
wins, ending the job as `stopped_by_cap` and exiting 124.

**Alternative rejected.** Reporting whichever came first, which makes
the verdict a race.

---

## 2026-10-13 — D32 One runner per data root

**Context.** Two runners sharing a data root made workspaces, cgroup
names and the reaper ambiguous, and "left behind by a dead runner"
meaningless.

**Choice.** The runner locks its data root at startup and refuses a
second one by naming the holder — the same choice D21 made for the
hub's state root.

**Alternative rejected.** Namespacing per runner inside one root,
which keeps the ambiguity and buys nothing.

---

## 2026-10-13 — D33 An explicit cgroup root that cannot be used refuses

**Context.** D23's `RLIMIT_AS` fallback exists for a *host* that
cannot delegate a cgroup; an operator who names a specific subtree is
making a precise request.

**Choice.** An unusable explicit `--cgroup-root` refuses the startup,
naming what the operator must grant — cgroup v2 needs write access on
the **common ancestor's** `cgroup.procs` — while an absent one still
falls back loudly.

**Alternative rejected.** Falling back silently from an explicit
request, which would hand someone a weaker ceiling than they asked
for without telling them.

---

## 2026-10-13 — D34 An unacknowledged verdict is spooled on the way out

**Context.** D17 has a runner hold a finished result until the hub
acknowledges it, and D19 forbids ever destroying an outcome, but a
runner told to stop while the hub is unreachable had nowhere to put
one.

**Choice.** On its way out the runner writes the `job_finished` frame
verbatim and fsynced to `<data root>/held-results/<job>.json`,
outside `jobs/` so the startup reaper cannot clear it, naming each
path on stderr; it first ends its jobs, then keeps serving a live hub
briefly so an acknowledgement still lands rather than spooling a
verdict the hub already has.

**Alternative rejected.** Dropping the verdict on stop, which is the
one thing Herd never does.

---

## 2026-10-13 — D35 The job record's metric field names follow the protocol

**Context.** M2 reserved placeholder metric fields in the hub's
`job.json` and M4 filled them from `herd-proto`'s own type; two names
for one thing would be two truths, against invariant 4.

**Choice.** The record carries the protocol's names —
`host_load_milli`, `peak_memory_bytes`, `bytes_written`, `log_bytes`,
`wall_time_ms` — and `RECORD_VERSION` stays 1 because nothing is
deployed.

**Alternative rejected.** Keeping both sets behind a translation
layer.

---

## 2026-10-13 — D36 A runner holds its output until it is acknowledged

**Context.** A log could lose its tail invisibly, because a gap is
only detectable when the chunk after it arrives, so chunks lost with
nothing following left the record silently short.

**Choice.** The runner holds log chunks until the hub acknowledges
them, replays them on attach, and `job_finished` carries the
attempt's chunk count so a missing tail is asked for once and named
only if it never comes; the holding is bounded at 4 MiB per job, and
`log.seq` counts **bytes**, because a chunk appended but never
counted was being written twice after a `kill -9`.

**Alternative rejected.** Unbounded holding, which turns a long
outage into a runner that eats its own host.

---

## 2026-10-13 — D37 What is timed for loss is an attempt, not a connection

**Context.** "Sixty seconds of silence means lost" does not say
whose silence, and timing the connection is wrong twice over — a
replacement runner masks the loss of the old one's job, and a runner
killed and restarted is connected and perfectly healthy while its
job exists nowhere.

**Choice.** The hub times the **attempt**.

**Alternative rejected.** Timing the connection, which both of this
milestone's real-binary failures came from.

---

## 2026-10-13 — D38 Waiting is not being refused

**Context.** D13 makes `Refused` the hub's single terminal "no" and
always the last frame, so a runner redialing into a hub that had not
yet noticed its previous connection was told "a runner is already
connected" and died of its own reconnection.

**Choice.** A new `StandBy { reason, retry_in_ms }` frame says
"wait", with a same-name supersede rule, leaving `Refused` terminal
exactly as D13 intended.

**Alternative rejected.** Reusing `Refused` for a transient state,
which D13 explicitly forbids and M8's explained waits would have
broken next.

---

## 2026-10-13 — D39 Two attempts, counted by the record

**Context.** E-21 requeues a lost job **once**, and the count has to
live somewhere that survives a hub restart.

**Choice.** The attempts are the entries on the job's own record, so
the limit is read from durable state rather than held in memory; a
second loss is a terminal outcome, not a third attempt. A drain race
can spend the second attempt before anything ran — chosen
deliberately, not an oversight.

**Alternative rejected.** A counter in the queue entry, which a
restart would have to reconstruct anyway.

---

## 2026-10-13 — D40 `cancelled` and `lost` are outcome variants

**Context.** D15 made a job's end a tagged outcome so that a new way
to end is a new variant and never a magic number.

**Choice.** A cancelled job and a job whose runner was lost twice
both end as named `JobOutcome` variants, and both exit **125**
because Herd has no verdict to give — consistent with D20.

**Alternative rejected.** Inventing a distinct exit code for each,
which multiplies sentinels colliding with real exit codes.

---

## 2026-10-13 — D41 A cap carries the reason its figure would misstate

**Context.** A job whose workspace could not be measured was told "stopped this
job at its disk cap of 21474836480 bytes" when it had written about four
kilobytes; the figure was true of the cap and false of the job, and the real
reason reached only the runner's stderr.

**Choice.** `StoppedByCap` gained a `detail`, so the record carries "could not
measure the workspace" beside the limit. Additive, so `PROTOCOL_VERSION` stays
1.

**Alternative rejected.** Leaving the figure to speak for itself, which makes a
record mislead whoever reads it later.

---

## 2026-10-13 — D42 A spooled verdict is delivered, its file cleared on ack

**Context.** D34 had a stopping runner write an unacknowledged verdict to
`<data root>/held-results/<job>.json`, and nothing read it back.

**Choice.** The runner picks up its spool before its first dial and offers
every spooled frame on each attach, removing a file only once the hub
acknowledges it; a verdict the hub rejects — unknown job, already finished,
superseded attempt — is dropped by name, and an undecodable file is kept as
`.unreadable` rather than deleted.

**Alternative rejected.** Deleting on send, which trades a stalled verdict for
a lost one.

---

## 2026-10-13 — D43 A job still running when stopped is an abandoned attempt

**Context.** The stop path used to report the job it killed as `signalled 9`,
so a requester saw a failure it had not earned and the hub saw a verdict that
stopped the requeue.

**Choice.** A drain hands the attempt back and the hub attempts the job again
under D39's limit of two.

**Alternative rejected.** Reporting the kill as the job's outcome, which blames
a job for its host's owner going to bed.

---

## 2026-10-13 — D44 A memory ceiling has a floor

**Context.** `--job-memory-max 0` was refused while `1kB` was accepted, a
ceiling under which no job can start, so the runner silently refused
everything.

**Choice.** A ceiling below a measured 16 MiB floor is refused at startup,
naming the floor.

**Alternative rejected.** Accepting any figure and letting every job fail,
which looks like a Herd fault rather than a configuration one.

---

## 2026-10-13 — D45 A privileged helper is judged on the file the kernel runs

**Context.** The escalation denylist matched the last component of the path it
was handed, so a symlink named `harmless` pointing at `unshare` was accepted.

**Choice.** The path is resolved and the target judged, it must be a regular
file, and the setuid/setgid check applies to it. A directory is refused by
name.

**Alternative rejected.** Trusting the name given, which makes the allowlist a
formality.

---

## 2026-10-13 — D46 `exclusive` is host-wide; `counted(1)` is not

**Context.** Read narrowly, "nothing else may hold it or what it excludes"
makes `exclusive` a synonym for `counted(1)`, and then M14's nightly
exclusivity has nothing to rest on.

**Choice.** While an exclusive capability is held, nothing else runs on that
runner at all, and taking it requires an idle runner; `counted(1)` merely
limits one capability to a single holder.

**Alternative rejected.** Collapsing the two kinds, which would have forced a
second exclusivity mechanism later.

---

## 2026-10-13 — D47 Exclusion closes symmetrically, never transitively

**Context.** Exclusion is declared on one side of a pair, and a closure rule
has to be fixed before queue entries embed label sets.

**Choice.** Symmetric, because exclusion is a property of the pair — otherwise
whether two capabilities overlap would depend on which job started first; never
transitive, because `a–b` and `b–c` say nothing about `a–c` and closing
transitively would silently forbid pairs nobody declared. A whole mutually
exclusive group is spelled as one `exclusive` capability or declared in full.

**Alternative rejected.** Transitive closure, which turns a typo into a policy.

---

## 2026-10-13 — D48 A probe nobody can read suspends its capability

**Context.** An external busy probe can fail in ways that mean nothing about
whether the resource is busy — a missing command, a signal, a timeout, an exit
code nobody declared.

**Choice.** Those all read as `unknown`, which suspends the capability, and a
command probe declares both a busy and a free exit code so that `127` cannot be
mistaken for "free"; a hanging probe is killed at `--probe-timeout`.

**Alternative rejected.** Defaulting to free, which hands a job to a runner an
external holder is already using — the exact harm E-26 exists to prevent.

---

## 2026-10-13 — D49 A capability with a probe is suspended until read

**Context.** Between a runner's handshake and its first availability snapshot
the hub knows a probed capability exists but nothing about its state.

**Choice.** The hub suspends it until a reading lands, closing the join race on
both sides.

**Alternative rejected.** Assuming free until told otherwise, which makes every
reconnection a window.

---

## 2026-10-13 — D50 Waiting is a reason on the record, not a state

**Context.** A queued job needs to say why it is waiting, and E-04 will later
need figures in that sentence.

**Choice.** `waiting_for` on `job.json` plus a `waiting` line in
`events.jsonl`, written only when it changes; the job's state stays `queued`.

**Alternative rejected.** A `JobState` variant for waiting, which would
multiply states for every reason a job can wait and force a record migration
each time a new reason appears.

---

## 2026-10-13 — D51 A busy probe is the owner's command, not Herd's

**Context.** E-26's probe is by design "a command whose exit code means busy",
so a probe can run a shell and do anything the runner's user can; "no shell" is
a convention, not a guard.

**Choice.** This is accepted and stated plainly — a probe is configuration the
runner's owner writes, it runs with the runner's privileges, and Herd neither
sandboxes nor interprets it. It stays strings and numbers, so it is not a
plugin surface (invariant 1).

**Alternative rejected.** Validating or restricting probe commands, which would
grow into exactly the plugin API invariant 1 forbids.

---

## 2026-10-13 — D52 Durable labels are read permissively

**Context.** A label set validates on deserialisation, so a single persisted
job carrying a label today's rules reject would stop the hub from starting at
all — one bad record bricking the service.

**Choice.** Labels already on disk are read permissively and a job whose labels
the current rules reject is quarantined by name, visible and skipped;
validation stays strict at submission, where a requester can be told.

**Alternative rejected.** Strict validation on the durable path, which makes a
tightening of the rules a denial of service against ourselves.

---

## 2026-10-13 — D53 A silence threshold is judged against the real cadence

**Context.** A hub at `--health-every 1 --lost-after 3` and a runner at
`--health-every 10` each passed their own startup check and then destroyed
a healthy job, burning both its attempts in six seconds, because the
runner's cadence never crossed the handshake.

**Choice.** The runner announces its real cadence in `Hello` and the hub
refuses a pairing that cannot satisfy its silence threshold with the
three-report margin, naming all the figures. This is judged once,
terminally, at the handshake — a cadence is settled when the process
starts, so standing by would wait for ever on something only a person can
change.

**Alternative rejected.** Checking each side alone, which is how two
legal configurations killed a healthy job.

---

## 2026-10-13 — D54 A terminal record is immutable at one write point

**Context.** A late frame bearing the correct runner name for a finished
job appended to its closed log with a false "chunks never reached this
log" marker and let a metrics write replace real figures with forged
ones, because the guard existed on one writer and not another.

**Choice.** The rule sits at the store's single write entry point so it
covers every writer, and a terminal transition stamps the attempt's end.

**Alternative rejected.** A guard per function, which is exactly how this
one was missed.

---

## 2026-10-13 — D55 A blank identity is no identity

**Context.** An empty instance string matched the "has an instance" arm,
so a peer ended up with a blank effective name, and a blank name matches
every other blank-named peer's attempt.

**Choice.** A blank or whitespace-only name, instance or state root is
read as absent and refused like a nameless peer; identities are trimmed.

**Alternative rejected.** Treating the empty string as a value, which
made one attempt writable by any blank peer.

---

## 2026-10-13 — D56 A runner that can't record a hub's identity still works

**Context.** A runner whose data root cannot be written cannot remember
which state root it spoke to.

**Choice.** It announces no state root, is admitted, and says so in one
loud line — safe because "no root" never equals a hub's root, so a stale
verdict is set aside rather than believed.

**Alternative rejected.** Making it stand by, which would idle a useful
runner over a detail that costs nothing.

---

## 2026-10-13 — D57 The ownership gate is not a credential

**Context.** An attempt belongs to the runner it was handed to, but a
runner's name is **self-asserted** until M15 mints per-runner tokens
(E-48), so a hand-crafted peer reaching the hub's port can announce any
name and, while the legitimate runner is down, write a false verdict,
fabricated metrics and forged log lines onto its job.

**Choice.** State this plainly in the code at the gate and in the docs,
keep the hub on loopback by default (D14), and do not expose Herd beyond
loopback before M15. The twin rule blocks the impersonation only while
the real runner is alive.

**Alternative rejected.** Building authentication here, which is M15's
work and would have been done twice.

---

## 2026-10-13 — D58 An unreadable handshake is refused with the reason

**Context.** An undecodable or invalid first frame got a TCP reset, so
the reason reached only the hub's own output and the runner's owner
never saw it.

**Choice.** `Refused { reason }` as the last frame before the close,
extending D13 from judgements about a peer to a frame the hub cannot
read at all.

**Alternative rejected.** A silent reset, which makes a configuration
mistake look like a network fault.

---

## 2026-10-13 — D59 One lock orders a claim against a probe's reading

**Context.** A capability snapshot going busy between the moment a claim
was judged and the moment it was recorded handed a job to a runner an
external holder had just taken.

**Choice.** The claim lock is taken before capability state is touched,
in one documented order and never the other way, so a claim and the
availability it was judged against cannot disagree.

**Alternative rejected.** Judging availability outside the claim, which
made the window inevitable.

---

## 2026-10-13 — D60 The capability file is read strictly, through its shape

**Context.** An unknown key — `probe` where `busy_probe` was meant —
offered a capability unprobed, and a job ran while an external holder
held the lock; a duplicated key did the same by silently taking the last
value.

**Choice.** The file deserialises through private types that refuse
unknown and duplicated keys at every level, naming the key and the
capability, while the protocol's own types keep whatever compatibility
stance they have. The file is an operator-facing surface and deserves
the strict reading.

**Alternative rejected.** Tolerating unknown keys for forward
compatibility, which turns one character into E-26's exact harm.

---

## 2026-10-13 — D61 One runner declares a capability name once

**Context.** A name declared twice silently kept the last declaration and
dropped the first one's exclusion list.

**Choice.** A duplicate name refuses the runner's startup, naming it.

**Alternative rejected.** Last-wins, which loses an exclusion without a
word.

---

## 2026-10-13 — D62 A probe is killed as a process group

**Context.** A busy probe is a command the owner writes (D51) and may
spawn children; reaping only the leader left them on the host, against
invariant 2.

**Choice.** Each probe runs in its own process group and is killed as
one, on a timeout and on a clean stop alike.

**Alternative rejected.** Trusting a probe to clean up after itself.

---

## 2026-10-13 — D63 One terminal test, two doors over it

**Context.** PR #11 made `JobDirStore::open_for_writing` the single
write entry point, checking `is_terminal()` under the store mutex: a
writer could not reach a record without passing the refusal, so the
rule was unbypassable by construction. M5 split it because
`record_waiting` must answer `false` rather than error on a job that
ended mid-pass — an erroring dispatch pass propagates up and drops the
runner's connection, a worse failure than a skipped waiting line.

**Choice.** `open_unless_over` holds the one terminal test;
`open_for_writing` wraps it and refuses by name. One terminal test, two
doors over it: a writer that believes in what it writes is refused by
name, a dispatch pass with nothing to write is simply told `false`.
Recorded honestly: the rule still holds today — nothing writes through
`open_unless_over` but `record_waiting`, and a test asserts the record
it leaves untouched — but a future writer picking the wrong door is now
stopped by review rather than by the compiler.

**Alternative rejected.** Keep one door and let `record_waiting` error,
rejected because it turns a harmless race into a dropped runner
connection.

---

## 2026-10-13 — D64 A probe reading is swept out of /proc like a job

**Context.** A capability probe is a command the owner wrote (D51) and
it can call `setsid()`, leaving its own session and surviving the group
kill (D62) — the same escape M4 found for job children.

**Choice.** Reuse M4's mechanism rather than invent a second one: every
reading carries a unique `HERD_JOB_MARKER` of shape
`herd-probe-<runner pid>-<counter>`, and `ProbeGroup` sweeps `/proc` for
it after the group kill, on the timeout path and the dropped-task path
alike; the startup reaper reads probe tags beside a job's. The probe
sweep's budget is four rounds (~100 ms) against a job's forty (~1 s),
because a probe reading happens periodically where a job ends once, and
invariant 2's five seconds has to hold across however many readings are
in flight. A stray resisting `SIGKILL` longer than that is left to the
next reading's sweep or the next runner's startup reaper.

**Alternative rejected.** A second sweep mechanism built just for
probes, rejected because M4 already solved the identical problem.

**Booked weaknesses.** Two, neither closed by this PR: (a) the sweep's
resistant case — a probe that outlives four rounds — was reasoned
about above, not tested; (b) every probe reading now scans `/proc` and
reads `environ` of every pid, estimated at a few milliseconds and not
measured, so a host with thousands of processes and a short
`--probe-interval` carries new periodic work nobody has benchmarked.

---

## 2026-10-13 — D65 A key written twice keeps its first value

**Context.** The capability file is refused outright if any key is
duplicated (D60), and the only thing that value is ever read for is
naming the refusal to the operator.

**Choice.** `WithEveryKey` keeps the first of two equal keys:
`{"name":"build","name":"other"}` names `"build"`, the name the
operator actually wrote first, and `"other"` appears nowhere in the
message.

**Alternative rejected.** Keep the last, which is what
`serde_json::Map::insert` does by default and would quote back a name
the operator may never have intended to write.

---

## 2026-10-13 — D66 A runner updates itself, and comes back alone

**Context.** Alexa manages both this project and its first consumer,
and will run the service on machines he does not want to touch again
once they are joined. "I do not want to come back by hand on every
runner to update it" is an operating requirement of the owner, not one
of chatOnS's 55 expectations; it gets a milestone and this decision
rather than an invented `E-56`, because that sheet belongs to the
consumer. One consequence is immediate: a runner can only obey an
instruction it already understands, so the update frames must be in
the protocol **before the first deployment**. A fleet shipped without
them is manual for ever.

**Choice.** A new milestone **M15d**, between M15 and M16. The hub
names a version and the artefact's digest; the runner fetches it
outbound (invariant 3), verifies the digest, and swaps only while it
holds no job, draining first (invariant 2). The host service keeps the
previous binary, and the rollback is **local**: the new runner must
complete a handshake and one health report inside a deadline, or the
service restores the previous version and restarts it with no hub
involved. The restored runner then reports what it refused and why, so
a failed rollout is read from the hub and not from the machine. During
a rollout the hub accepts the previous protocol version, and a runner
too old to understand the instruction is **told to update rather than
refused** — a refusal is a dead machine someone has to go and touch,
which is the failure this milestone exists to remove.

**Why it cannot land before M15.** Telling a host "fetch this and run
it" is a remote code execution channel into every machine of the
fleet. Runner names are self-asserted until M15 (D57), so before
authentication exists anyone reaching the hub's port could own the
whole fleet through this mechanism. M15 is a hard predecessor, and the
digest check is not optional.

**Alternative rejected.** Widening M15 itself to "auth, versions and
self-update", which is the same handshake conversation. Rejected by
Alexa in favour of a separate milestone: the periphery of M15 stays
readable, and `M15d` inserts without renumbering M16, M17 and M18.

**Not covered by this decision.** The hub updates with its deployment
— it is one machine in docker compose on the NixOS server. The
server's own runner runs natively and is updated by Nix (E-47), so
M15d serves the non-Nix hosts: the WSL PC and whatever joins later.

---

## 2026-10-13 — D67 Aging is computed from the record, never written

**Context.** E-18 needs the lowest level never to starve. The obvious
shape — a counter per queued job, ticked up and written down — costs a
write per job per tick and loses whatever the tick had not yet written
when the hub dies.

**Choice.** A job's effective level is a pure function of its record and
the current moment: its declared level raised one step per whole aging
step it has spent queued, capped at the top. The moment is an argument,
so the simulated-time tests drive the same function the hub calls with
`Timestamp::now()` — there is no second implementation. The default step
is 30 min, which puts the lowest level at the top 90 min after
submission, the figure E-18's own criterion names; `--aging-step` tunes
it. The queue is re-judged on every event that could change it and, so
that a waiting job's figures are never stale, on a timer at a quarter of
the step.

**Alternative rejected.** A persisted aging counter, which has to be
written to be believed and starves a job for every tick it loses.

---

## 2026-10-13 — D68 A restart keeps a job's accumulated wait, downtime included

**Context.** A hub killed mid-queue must not reset what a job has
waited — that starves it afresh at every restart — and must not hand it
back maximally aged either.

**Choice.** A job's age is wall-clock time since the `submitted_at` on
its own record, which was fsynced when the job was minted. A restart
therefore changes nothing: the age is what it is, and the time the hub
was down counts as waiting, because the requester was waiting. Relative
order among jobs queued before the crash is untouched by downtime, since
all of them gain the same. A clock that steps backwards saturates at
zero, so a job can read younger than it is but never older.

**Alternative rejected.** Accumulating queued time in the record and
pausing it while the hub is down: it needs a periodic write, and a hub
that restarts often would keep losing the unwritten part — starvation
through bookkeeping.

---

## 2026-10-13 — D69 The dedup key is the command and the labels, counted

**Context.** E-05 joins a second submission of the same work. A key made
by joining the pieces with a separator makes `["a b","c"]` and
`["a","b c"]` the same work, whatever separator is chosen, because a
command may contain it.

**Choice.** The key is each piece of the command and each label written
as its byte length, a colon and its bytes — parseable back, so two
different lists can never produce one key. Labels come canonical from
`LabelSet`, so naming two in the other order is the same work; the
priority is *not* in the key, since two agents asking for one build at
two levels are asking for one build. The key is derived from the record's
own fields and never stored beside them, and the index of open keys is
rebuilt from the records at every start — so a join survives a restart
instead of running the work twice.

**Alternative rejected.** A separate durable index of keys, which is a
second copy that can disagree with the records it describes.

---

## 2026-10-13 — D70 A submission joins only a job that is not over

**Context.** A second identical submission arriving after the first
ended could be handed the first's verdict. For a job that ended
`cancelled` or `lost` that verdict is 125 (D40) — a second caller wearing
"no verdict" for a cancellation it never asked for — and for any ending
it makes a rerun impossible to ask for.

**Choice.** Only an open job is joined; work that is over is minted and
run again, as its own job with its own verdict. The store takes the key
out of the open-work index at the moment the job reaches its end, under
the same lock, so there is no window in which a finished job is still
joinable.

**Alternative rejected.** Returning the existing result, which inherits
a stale or 125 verdict and leaves no way to ask for the work again.

---

## 2026-10-13 — D71 A submission must name its priority

**Context.** A level nobody chose is invisible on the wire, invisible in
the record, and wrong exactly when it matters — the job goes before or
after somebody else's work on the strength of it.

**Choice.** `SubmitJob` carries a required `Priority`, `Priority` has no
`Default`, and `job.json` carries the level as a required field. A frame
without one is refused by name, as is a name that is not one of the four
— never clamped to the nearest. `herdctl` has a visible default (`step`,
shown in `--help` and echoed on stderr at every submission), because an
agent submitting its branch is submitting a step; what it must not have
is an *invisible* one. Both versions moved for this: `PROTOCOL_VERSION`
to 2 and `RECORD_VERSION` to 2, so a pre-M6 record is refused with both
figures rather than read as if its requester had asked for the middle
level. Nothing is deployed; they freeze at M16.

**Alternative rejected.** A defaulted field, which keeps both version
numbers and buys that with a level a reader cannot tell from a chosen
one.

---

## 2026-10-13 — D72 A priority is only ever raised

**Context.** E-18 lets the consumer's orchestrator raise one job.
Offering the other direction offers a way to starve something by hand,
and the queue's own aging is what moves a level down.

**Choice.** `RaiseJob` raises and nothing else: a request to lower is
answered with a sentence naming both levels and writes nothing, a job
already at or above the level asked for is a no-op, and a job that is
over is refused by name through the store's one write door (D54) — which
is also what makes a joining submission's raise safe, since it goes
through the same door.

**Alternative rejected.** A `set_priority` that takes any level, whose
first use would be the one it should not have.
