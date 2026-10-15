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

---

## 2026-10-13 — D73 Before M16 the version numbers are free-running

**Context.** M6 made the priority a required field in `SubmitJob` and in
`job.json`, so it moved `PROTOCOL_VERSION` and `RECORD_VERSION` from 1
to 2, and a pre-M6 state root is now refused by name with no migration.
Alexa was asked, because a protocol break is his to approve: his answer
was that before M16 nothing uses either number, so bumping is harmless
and staying at 1 would have been fine too. Between here and M15 the
formats will break several more times, so the question worth settling
is not this bump but what the numbers mean while nothing is deployed.

**Choice.** Before M16 both numbers are free-running development
counters. Every format break advances the one it touches, neither
promises compatibility with anything, and **no migration is ever
written for a pre-M16 state root** — it is refused by name and the
operator deletes it. At M16, the deployment milestone, both are set
back to **1**: the first deployed protocol is version 1 and the freeze
begins there. After that a bump is a compatibility statement and
carries whatever migration it needs.

**Why not simply leave them at 1 until M16.** A required field absent
from an old record is a deserialisation failure, not a refusal: the
reader gets a parse error naming a missing field instead of a sentence
saying the root predates a format change. Advancing the number is what
turns that into the refusal the store already knows how to make, which
is why M6's bump is kept rather than reverted.

**Alternative rejected.** Carrying the development counters into the
deployment and reaching M16 at version nine or so. Rejected because a
reader would then read eight compatibility eras that never existed,
and the first version chatOnS pins at adoption should be 1.

---

## 2026-10-13 — D74 A job's requirements are read on a runner, never guessed

**Context.** Until now Herd assumed a job's requirements were known at
submission. Issue #16 breaks that: the labels a test needs, its memory
and its tmpfs depend on the scenarios the test selects, and only the
consumer's own tool knows them. Their declaration names a `labels_from`
command — the same arguments plus `--labels`, which builds and boots
nothing and prints `labels=…`, `tmpfs_mib=…`, `memory_mib=…`,
`scenarios=…`. So the hub must learn a job's requirements before it can
place it, and it cannot learn them by itself.

**Choice.** The reading runs **on a runner**, not in the hub. It is an
ordinary job: it goes through the one queue, is confined by M4's caps,
and is recorded, so a refusal can quote it (E-36) and the reading itself
is kept (E-15). The hub places it against the command's *declared*
baseline labels — chatOnS's `--labels` needs the tree and nothing else,
so `build` — then reads the answer, writes the requirements onto the
record, and places the real job with M5's algebra and M8's admission.
Then state these limits, each of which matters:

- the answer is read **strictly**, like the capability file (D60): an
  unknown key, a missing key, a duplicated key or a non-numeric figure
  refuses the job **by name**;
- a reading that exits non-zero, or that does not finish inside its
  cap, **refuses the job by name** — a job is never placed on a guess;
- **no recursion**: a command reached through `labels_from` never
  itself resolves a `labels_from`;
- the answer is **cached by (commit, command, args)**, in the job
  store, because it is a property of those three alone — this is the
  same key family as M6's dedup key, and without the cache every
  submission pays a reading.

**Consequence for the plan, and it costs nothing.** M7 and M8 are built
against **the requirements held on the record**, and stay exactly where
they are in the order. Where those requirements came from — a requester
who stated them, or a reading that produced them — is the command
contract's problem, so **the reading lands in M10** with the
declaration file. M7's and M8's plan entries now say they consume the
requirements on the record; M10's entry gains the reading, with its
cache and its two refusals.

**Alternative rejected.** The hub running the reading itself. Rejected
because the hub would then execute the consumer's code, unconfined, on
the machine that holds every job's record — and the runner already has
M5's probe machinery for exactly this shape of work.

**Confirmed by the consumer: the cache key is correct.** `labels_from`'s
answer depends only on (commit, command, args): `--labels` reads only the
arguments and the scenario table compiled into that commit, and nothing on
the machine — no load, no free memory, no free tmpfs. They committed to
opening an issue before ever changing that. Their figures are computed for
the `-j N` they were given, though, so N is part of `args` and the cache
key is only correct if N is chosen first — which is what D82 settles.

**Confirmed by the consumer: one parent is enough.** Their only `needs`
is test-pack → the build that made the pack of the same commit; they do
not foresee two parents and asked to keep the cap E-10 states (invariant 1).

---

## 2026-10-13 — D75 Coalescing drops a queued job, never one in flight

**Context.** M6 shipped deduplication (E-05): a second submission of
work an open job is already doing **joins** it. Issue #16 asks for
something different under E-42: a `coalesce` key, where "a queued
produce of an older main is **dropped** when a newer one arrives".
Dedup joins; coalescing discards. They are two behaviours, not one.

**Choice.** A command may declare a `coalesce` key beside its
`dedup_key`. At most one job with a given coalesce key sits in the
queue: when a newer one arrives, the older one is **superseded** — a
new outcome variant `superseded { by }` naming the job that replaced
it, exiting **125** because Herd has no verdict of its own (consistent
with D40, where `cancelled` and `lost` are outcomes and both exit 125).
`herdctl wait` on a superseded job says which job superseded it.
**Only a queued job is ever superseded** — work already running is
never discarded, which is the consumer's own wording and also keeps
invariant 2 honest about not throwing away a host's effort. A dedup
join and a coalesce drop can both apply to the same submission; dedup
is considered first.

**Where.** M14, with E-42's push hook, which is the only thing that
needs it. M14's plan entry gains the coalesce key and the supersede
rule.

**Alternative rejected.** Folding coalescing into M6's dedup as a flag.
Rejected because the two answers are opposite — one caller gets the
running job's result, the other gets told its job no longer exists —
and a single switch over that would be read wrong by whoever touches
it next.

---

## 2026-10-13 — D76 A lease is a busy probe; a requeue code is declared

**Context.** The consumer's exit codes include **75**: "metal machine
absent or leased to someone else: requeue, not a failure". Herd passes
a job's own code through untouched today (D20), so 75 would reach the
requester as a verdict. But their own label table already says
`metal-<machine>` has capacity one and is **busy outside Herd while a
lease file has not expired** — which is exactly M5's busy probe (D48,
D51).

**Choice.** Two halves, in this order. First, **the lease is a busy
probe**: a `metal-*` capability reads the consumer's lease file, so a
job whose machine is leased **waits** (`StandBy`, D38) instead of
running and failing. Second, a command may declare **one** exit code
meaning "the capability I was given went away under me" — 75 for them.
That code is the race between the probe and the run, so it causes a
**bounded requeue with the capability re-probed**, not a failure; when
the budget is spent the job ends with an outcome naming the code and
the runners that answered it, exiting 125. This requeue budget is
**separate from E-21's two attempts for a lost runner** — conflating
them would let a leased machine burn the budget that exists for a
disappearing runner.

**Where.** M9, with E-27's metal lease and drain-first. M9's plan
entry gains the lease-as-probe and the declared requeue code.

**Alternative rejected.** Hard-coding 75 in Herd. Rejected by
invariant 1: 75 is the consumer's convention, and a project-agnostic
service takes it as a declared field.

---

## 2026-10-13 — D77 The record timestamps the executor's steps

**Context.** The consumer's declaration wraps every job in four of
their own steps — `up` (a workspace over their nearest warm cache),
`fetch`, `run`, `down` — and asks that the steps be timestamped so the
preparation time is readable in the job's record. Their `up` may pull a
whole cache the first time, which is minutes, and today that time
would be invisible inside the job's wall time.

**Choice.** The job record gains one line per step: its name, when it
started, how long it took, and its exit code. Four **named** steps in a
fixed order, with no conditions, no branching and no step a job can
add — this is **not** a workflow language, and the boundary is
invariant 1's: the day a step gains a condition, Herd has grown the
thing it refuses to be. This is a record-format change, free before
M16 and a migration after it (D73), so it lands with the declaration
in **M10**. E-04's and E-36's explanations may then quote preparation
time.

**Alternative rejected.** Leaving the steps inside the job's wall
time. Rejected because a thirty-minute cold cache pull would read as a
slow test, and the consumer would be debugging the wrong thing.

---

## 2026-10-13 — D78 The runner never needs Docker, and proves it

**Context.** We told the consumer that on the deployment host the
runner's user is in the `docker` group, which is root-equivalent
through the daemon's socket. Issue #16 calls that a blocker: their
project never touches Docker, the host's Docker serves other
workloads that are not theirs, and their jobs are agents' code run
unattended. Their rules are one data root and root only through a
short fixed helper list (E-45, E-46).

**Choice.** Agreed without reservation, because the runner has never
needed Docker for anything it does — the group membership is the
host's current configuration, not a design requirement. Herd commits
to three checkable things: no crate in the workspace depends on a
Docker client and the string `docker.sock` appears nowhere in it; a
test asserts that while a job runs, **no open file descriptor of the
runner resolves to the Docker socket**; and the runner's whole run —
claim, execute, return the result, reconnect after the service
restarts — is exercised with that socket **unreachable to its user**.
The hub may still run in docker compose: that is the operator's
deployment and needs nothing from the runner's user. The acceptance
belongs with the deployment, so this lands in **M16**, and removing the
user from the group is a NixOS change that is Alexa's to make.

**Alternative rejected.** Keeping the membership and arguing the
runner does not use it. Rejected because the consumer's rule is about
what is *reachable*, not about what is used, and a bug or a bad job is
precisely the case the rule exists for.

## 2026-10-13 — D79 Holdings are read from the records, never counted

**Context.** M7 has to know how many slots of a counted capability are
taken on each runner (E-22, E-25). The obvious shape is a counter per
runner, incremented at the claim and decremented when the job ends. That
counter has one job — never, ever to leak — and seven ways to fail it: a
job exits, is signalled, is stopped by a cap, is cancelled queued, is
cancelled running, is lost, is superseded by a requeue, or its runner's
link drops mid-job. A hub restart has to rebuild it. One forgotten path
halves a machine for the rest of the hub's life, silently.

**Choice.** There is no counter. A runner's holdings are rebuilt at the
start of **every dispatch pass** from the job records: a job holds its
labels on the runner of its open attempt exactly while its record is
`assigned`, `running` or `interrupted` with an attempt that has no
`ended_at`. That is the same fact the watchdog, the requeue and the
restart already work from, so the release *is* the write that ends the
job and there is no second path to forget. A hub that restarts reads the
same thing the hub before it would have read, so "rebuilt after a
restart" is not a code path either — there is nothing to rebuild.

The pass carries its holdings forward as it goes: a job placed on the
first job of a pass contends with the second one in the same pass, which
is what stops an exclusive claim and a counted claim being admitted
together. Taking the labels is [`Holdings::admit`], which checks again
under the claim lock before it writes, so a judgement that has gone
stale places nothing.

A job the records say is out there is written into the holdings
**unchecked** (`Holdings::already_holding`). Checking would refuse to
write down a job whose label its runner has since stopped offering, the
host would read as idle, and something host-wide would be handed out on
top of work that is still running.

**Cost.** One read of the unfinished job directories per pass, which
every pass already did for the queue's order (E-20), plus a few
hundred bytes of holdings per pass. At the first consumer's scale — tens
of unfinished jobs, two runners — that is nothing, and it buys a class
of defect that cannot be represented.

**Alternative rejected.** A counter with a careful release on every
path. Rejected on the arithmetic above: eight release paths and a
restart, against a derived value that cannot drift from the record it is
derived from.

---

## 2026-10-13 — D80 A draining runner is not available, so nothing waits for it

**Context.** E-19 says a job waits for a runner of its labels "as long as
such a runner is available (connected, switch on)". A runner that is
**draining** — asked to stop, finishing what it has, taking nothing new,
gone inside five seconds (invariant 2) — is connected. So the question
had to be answered either way, and tested either way.

**Choice.** Draining is **not available**. A job does not stick to a
machine that is leaving; it is placed on the next eligible runner at
once, and the sentence of a job that has nowhere else to go names the
drain. The claim step still *sees* a draining runner and still says so,
rather than leaving it out of the fleet: a job that cannot run has to be
able to say that the machine it would have gone to is on its way out.

**Why this way round.** The harm is one-sided. Waiting for a draining
runner costs the whole drain and then a re-judgement anyway, on a
machine that by then offers nothing — up to the stickiness limit of
three hours if the drain is a shutdown. Not waiting costs a job that
runs on a less preferred machine, which is slower and correct. E-19's
own words are "connected, switch on", and a draining runner has in
effect already answered that switch.

**Alternative rejected.** Reading "connected" literally, so a drain
holds its queue for three hours. Rejected because an owner shutting her
PC down is the case E-19 exists to serve, not the case it exists to
fight.

---

## 2026-10-13 — D81 Preference is one integer, declared by the machine

**Context.** E-19 asks for a job to wait for the runner of its labels
rather than fall back to another eligible one, and its acceptance
criterion is "with the PC on and busy, a second build shows `queued 2nd
on pc` and never moves to the server". That only means something if the
hub knows the PC is the *right* machine for a build — the consumer
measures it at 3.1x their loaded server — and a requester may never name
a machine (invariant 6).

**Choice.** A runner declares one integer in its handshake:
`--preference N`, higher first, zero by default. The claim step places a
job on the most preferred runner that can take it now; when the most
preferred *available* runner cannot take it now, the job waits for that
one. The figure is in the line explaining every placement and in the
sentence of every job waiting on it.

One integer, per machine, and nothing more: not per capability, not per
job, not an expression. M12 adds cache locality (E-23) as a further term
on top of it rather than a second mechanism, and the moment this wants
to be anything a person would call a rule, invariant 1 says no.

**What it costs, said plainly.** E-19 is a deliberate loss of
throughput: three builds submitted at once on this fleet queue behind
the one slot of the preferred machine while a two-slot machine sits
idle, until the stickiness limit lets them move. That is what the first
consumer asked for and what their acceptance criterion asserts. The
limit is what keeps it from being starvation, and it is why E-19 asks
for the limit to be visible.

**A consequence worth knowing.** A job waiting for its preferred runner
does **not** hold the queue behind it: the next job is offered to the
fleet as usual. So a lower-priority job can take a free lesser machine
while a higher-priority one waits for the machine it prefers. E-19 over
E-18, bounded by the limit, and said out loud here because it is
surprising.

**Alternative rejected.** Per-capability preference, which the first
consumer's fleet does not need — the only label two of their machines
both offer is `build` — and which would have been the first line of a
policy language.

---

## 2026-10-13 — D82 The `-j` is chosen before the requirements are read

**Context.** E-22 derives a build's `-j` from the machine it lands on.
Issue #16's answers add two facts: their tool treats the figure as a
**ceiling** — it may run fewer scenarios at once, never more, and lowers
the count itself when its tmpfs is short — and their `--labels` reading
computes its memory and tmpfs figures **for the `-j` it was given**. The
second one is a trap: N comes from the machine's memory, and the job's
memory requirement comes from a reading that needs N.

**Choice.** **N first, requirements second, placement third.** The
derived figure is written onto the **job record** (`parallelism`) by the
same write that numbers the attempt, before the job is handed to
anything, so that whatever later reads the job's requirements reads the
N the job was given. No interface in Herd may ask for a job's
requirements and then choose N, and none may choose N from a reading's
output.

Because the figure is a property of the machine, a requeue onto a
differently sized machine derives its own and rewrites the record's
field; M10's reading must therefore be re-read, or its cache keyed, on
the figure. The arithmetic of each attempt stays on that attempt's
explanatory line.

**What it means for admission (M8).** Their ceiling promise is what
makes this sound: a figure obtained for a given N stays valid on any
machine, so admitting against N is always safe and Herd never has to
model their lowering.

**Alternative rejected.** Deriving N at the moment the job is handed
over, with the record told afterwards. Rejected because it reads
identically today and is circular the moment M10 lands.

---

## 2026-10-13 — D83 A derived `-j` rounds down and keeps a quarter back

**Context.** E-22 wants `-j` derived from a runner's memory and threads
at the consumer's measured 1 792 MiB per build thread, and names the
answer it expects: about `-j 7` on the owner's 17 GiB PC. Three quarters
of 17 GiB is 13 056 MiB, which is 7 threads of 1 792. All of 17 GiB
would be 9, two more than they found fits.

**Choice.** `-j = min(threads / slots, (memory × 3 / 4 / slots) /
1 792)`, rounded **down**, never below 1, with every figure in the line
that explains the placement.

* **Three quarters.** The last quarter is the host's: the page cache the
  build reads through, the linker's peak over the mean the per-thread
  figure is, and the desktop session of whoever owns the machine
  (invariant 2). It also reproduces their measured figure exactly.
* **Divided by the slots.** A host must survive **every** slot being
  full, not the first job to arrive. Two build slots on their 31 GiB
  server are `-j 6` each, not `-j 12` each.
* **Rounded down.** A thread the memory does not cover is a host that
  swaps. Rounding up is the direction that breaks invariant 2.
* **Never zero.** `-j 0` is "no limit" to `make` and an error to cargo;
  a slot whose share does not cover one thread gets one thread, and the
  line says the share does not cover it.
* **A figure that is missing is missing.** A runner that does not say
  how much memory or how many threads it has takes no job whose `-j` has
  to be derived, and the sentence names which figure is absent. `0` is
  refused where it is written — at the runner's startup — rather than
  announced as a size: an owner who wants a machine to take no build
  denies the capability.

**Alternative rejected.** Handing out the whole of the memory and
letting M8's admission catch the overcommit. Rejected because it would
contradict the one figure E-22's acceptance criterion names, and because
the host that swaps is the owner's own PC.

---

## 2026-10-13 — D84 "Nobody ever" ends a job; "nobody now" queues it

**Context.** E-36 asks for impossible label sets to fail fast, and D38
says waiting is never a refusal. The two meet on one question: a job
whose labels no connected runner offers — is that an end or a wait?

**Choice.** A job is ended (`JobOutcome::Impossible`, terminal) only
when nothing about the fleet could ever fix it:

* two of its labels exclude each other on a runner that offers both, so
  no machine could hold them together;
* it asks for a derived `-j` and carries no figure to derive it from, or
  carries a figure of zero, or has nowhere in its command for the figure
  to go.

Everything else is a **wait**, with its reason on the record: a label
nothing here offers, a label its owner denied, a label a probe outside
Herd is holding, a slot that is taken, a host emptied for something
host-wide, a machine that has not said how big it is. A runner that
offers the label may dial in a minute from now, and the hub cannot tell
"nobody has it" from "the machine that has it is switched off" — so it
must never read the second as the first.

**The order the claim step asks these questions in is load-bearing.**
Self-contradiction is judged **before** anything about the moment: a job
asking for two labels that exclude each other, one of which a lock file
happens to be holding, would otherwise be told it is waiting for that
lock, for ever, for work that could never have run.

**Alternative rejected.** Keeping a roster of every label the hub has
ever been offered and failing anything outside it. Rejected because the
roster would have to be durable to survive a restart, and an empty
roster on a fresh hub would fail every job in the queue by name.

---

## 2026-10-13 — D85 Herd keeps a job's files under `{out}`, names what it drops

**Context.** The consumer's `results.json` lists, per scenario, the files a
test left — serial logs, screendumps, recordings — and asks that Herd
keep them with the result (E-15). We asked whether those paths are always
under `{out}`, the directory Herd gives the job under the runner's data
root. Their first answer was **no, not today**: the files were written in
their workspace's scratch area, inside the data root but outside `{out}`. They
then changed it the same day and reported it tested: when `CHATONS_RESULTS`
is set, every kept file is placed at `{out}/files/<scenario>/<name>` before
`results.json` is written — hard-linked, or copied across filesystems,
a name collision becoming `<n>-<name>`, and a file that can be neither
linked nor copied dropped from the list **and reported in their log**.

**Choice.** Herd keeps what a job left **under `{out}`** and nothing else. A
path in a result that points outside `{out}` is **not kept**, and Herd says
so by name in the job's record rather than dropping it in silence — the
same rule as everywhere else here: a figure or a file that is missing says
it is missing (contrast M4's `Some(0)` defect, and D27's rule that a skip
is never read as a pass). This is why `{out}` exists as a named directory
rather than Herd scanning the data root: the data root also holds the
runner's own state, other jobs' directories, and the consumer's workspace,
and a job's result must not be able to name any of them.

**Alternative rejected.** Keeping any path a result names, as long as it
falls inside the runner's data root. Rejected because a result would then
be able to attach another job's files, or the runner's own state, to itself
— and because E-45's confinement is cooperative (D28), so a job's claim
about where its files are is not evidence.

---

## 2026-10-13 — D86 A busy probe is declared by its machine's owner, not a repo

**Context.** The consumer proposed declaring their metal-lease probe in
their own `tools/herd/commands.toml`, as a `busy_probe` field beside each
`metal-<machine>` label: `cargo xtask metal <machine> --lease-status`,
exit **0** free, **10** busy, **1** cannot tell, **64** unknown machine,
with everything other than 0 and 10 to be treated as busy. The probe itself
is exactly the shape D48 requires — two known codes, so a missing command
exiting 127 can never read as free — and they asked for the stricter
reading themselves.

**Choice.** The probe's **contract** is taken as offered; the **declaration**
is not. D51 already says a busy probe is the machine owner's command: it
runs with the runner's privileges, outside any job, without the caps and
the confinement a job's command line runs under (M4). Reading it out of
a repository would let anyone who can push to that repository choose what
executes on every metal runner, unconfined and with no job attached — a
different trust story from "a consumer declares what its jobs need", and the
one Herd refuses. So the consumer's `busy_probe` block stays in their file
as **the text an operator copies** into that runner's capability file, which
is where Herd reads it from, and a `metal-<machine>` capability therefore
exists only on a machine whose owner put it there deliberately. The consumer
agreed without reservation, noting it is the same rule they apply to root
("root only through a short fixed helper list").

**What the consumer will observe, and it differs slightly from what they
asked.** They asked that any code other than 0 and 10 be read as busy. Herd
reads it as **unreadable**, which under D48 **suspends** the capability:
the job waits exactly as it would for a real lease, and the reason names the
probe and its exit code. The outcome is the same — the job waits — but a
permanently broken probe surfaces as a capability nobody can offer rather than
a machine that is mysteriously always busy. Their 64, an unknown machine,
is therefore loud the day a label and a machine name drift apart. Also
record: a probe still waiting on their lock when the owner asks the runner
to stop is **killed** by the probe group, not waited for — invariant 2's
five seconds outranks a reading, and that reading simply does not count.

**Alternative rejected.** Reading the probe from the consumer's
declaration. Rejected as above; it would need authentication and a trust
model that does not exist before M15 (D57).
## 2026-10-13 — D87 A running job holds what it declared, read off its record

**Context.** E-29 weighs a job's need against "free memory minus a
per-client floor minus what running jobs hold", and M8 had to decide what
*hold* means. The tempting reading is the true one: what the job's processes
are actually using, which the runner already measures (M4's `peak_memory`).
It is also wrong in a way that cannot be seen until a host swaps. A job
admitted a second ago has allocated **nothing** yet, so a hub costing real
usage admits a second job beside it, and then both grow into their
declarations at once.

**Choice.** Admission counts the **declared** need of every job the records
say has an open attempt on that runner, and never a measurement. Two figures
live on the record for it — `memory_mib` and `tmpfs_mib` — and a build's
whole-job need is its per-thread figure times the `-j` it was given, which
is why D82's order (N first, requirements second) is load-bearing rather
than tidy. The declaration is honest because M4's cap already stops a job
that exceeds it: a job over its figure is killed, not tolerated, so the
figure is a ceiling the kernel enforces and not a guess.

The sums are derived **from the records at the start of every pass**, through
the same filter the slot counts use (D79): one `out_there` iterator, two
derivations. There is no reservation table, so none of the eight ways a job
ends has a release path to forget, and a hub that restarts reads what the hub
before it would have read — "rebuilt after a restart" is not a code path.
The pass carries the figures forward as it goes, so two jobs offered in one
pass are weighed against each other and not only against what was running
when the pass began.

**A holding that cannot be costed is named, never zero.** A build out on a
runner with no `-j` on its record is a bug somewhere else; what it must not
be is a machine that reads idle on top of a running build. Such a record
stops that runner admitting anything more that costs memory, by name, with
the job id in the sentence.

**What "free" means here, and it is narrower than E-29's word.** Not what
`free` prints: the memory the machine's owner **lends**, less the floor, less
what is declared. A live figure would move under the hub between the
judgement and the claim, and a reservation weighed against a moving figure is
exactly the overcommit E-32 forbids. The live figures are still recorded —
load on every placement line, peak memory on every record — they are simply
not what admission is arithmetic over.

**Alternative rejected.** Reading `MemAvailable` per pass and admitting
against it. Rejected on the two-jobs-in-one-pass case above, and because it
makes a placement unreproducible: the same fleet in the same state would
make different choices, and E-36 asks for a decision an operator can
explain.

---

## 2026-10-13 — D88 Short of room now waits; too big ever does not stick

**Context.** E-31 says a job that cannot reserve its tmpfs **waits, not
fails**. Read flatly, that makes every shortfall a wait — including a job
asking for 8 GiB on a machine with 2 GiB, which would then be told, every
minute, that it is "waiting for build-host" for something that can never
happen. That is M7's own defect in a new place: it judged a suspended
capability before a self-contradiction and left a job waiting for a lock
that was irrelevant to why it could never run.

**Choice.** Three answers, not two, and the difference is whether the job
**sticks** to the machine (E-19).

* **The room is there and something has it** → the job waits *for that
  machine*, because a job ending there is what frees the room. The sentence
  carries both figures: `1.2 GiB free in tmpfs, 2 GiB needed`.
* **More than the machine has in all**, or a machine that does not say what
  it has → *not here, whatever happens here*. The job does not stick to it,
  the sentence says `has 4 GiB of tmpfs in all and this job needs 6 GiB … no
  reservation there can ever cover it`, and the job is free to be placed on a
  machine that could fit it.
* **The job's own figures are not figures** → terminal (D84). A whole-job
  memory of `0`, or a job declaring both a per-thread figure and a whole-job
  figure, ends with the reason: a human has to fix it, and a job waiting for
  that would wait for ever.

**What we did not do, and the risk accepted.** A job that no *connected*
runner could ever fit is **not** failed: it waits, and its sentence says
"ever" rather than "not now". A fleet is not a closed world — the owner's PC
is asleep at night and its 17 GiB come back in the morning — and a hub that
killed the nightly because the big machine was briefly away would be worse
than one that waited. The cost is real and is stated here: such a job can
wait for ever, with a sentence an operator greps in one go, holding nothing
else up (E-19's wait is non-blocking). The bound that ends it is **E-09's
deadline, in M9**, which is where a bound on waiting belongs.

**Alternative rejected.** Ending a job whose size no connected machine can
take. Rejected on the asleep-PC case, and because E-31's own words are
"wait, never fail".

---

## 2026-10-13 — D89 An override lends less of a machine, never more

**Context.** M7 left a runner's announced memory as `MemTotal` unless its
owner overrides it with `--host-memory`, which is right — `MemTotal` is the
whole machine and an owner lends part of hers. M8 subtracts a floor from that
figure so the host keeps room to live. The two together have a hole: an
override **larger** than the real `MemTotal` makes the floor guard memory
that does not exist. `--host-memory 64GiB --host-memory-floor 6GiB` on a
17 GiB machine announces 58 GiB of admission, and the first job past the real
total is the owner's PC swapping — the 2026-10-12 OOM E-30 exists to stop,
arrived by a different road.

**Choice.** The figure is **clamped to the machine, loudly**, where it is
written: the runner prints both figures and the reason at startup, and
announces the clamped one. Clamped rather than refused because the owner's
intent is still readable and the clamp is still safe — she is lending her
machine, and the clamp lends all of it — and because a runner that would not
start takes a machine out of the fleet for a typo. When `MemTotal` cannot be
read there is nothing to clamp against, so her figure stands and the line
says it was not checked. The one option ruled out from the start was a silent
accept. `--host-tmpfs` is clamped the same way against the mount it names.

**The floor itself.** `--host-memory-floor`, announced by the machine like
every other figure about it, because its owner is the authority on how much
of it Herd may have. Absent, it is **a fifth of what is lent, between 1 GiB
and 8 GiB** — a fraction chosen to reproduce the two figures E-29 names
rather than invented: a fifth of their 31 GiB server is 6.2 GiB against the
6 GiB they ask, and a fifth of the owner's 17 GiB PC is 3.4 GiB against her
3. A floor at or past the memory lent is **refused at startup**: it is a
machine that can admit nothing, and denying its capabilities is the honest
way to say that. A floor a peer never announced at all is **unknown**, and
nothing is admitted against an unknown — a hub reading it as zero would admit
a job into exactly the memory the floor exists to keep.

**This is not double-counting E-22's quarter.** D83 keeps a quarter of a
host's memory back from *build threads*, per slot, to choose `-j`. The floor
is subtracted from *whole-job admission*. They are two gates on two
questions, and the first consumer's own fleet passes both: `-j 6` in each of
two slots on the server is 2 × 10.5 GiB against 25 GiB admissible, and `-j 7`
on the PC is 12.2 GiB against 14 — it fits, and barely, which is the figure
E-22 and E-29 between them were always going to produce.

**What the clamp cost, found by CI and not by reasoning.** M7's real-binary
test of E-22 starts a runner with `--host-memory 17GiB` and asserts the
process was given `-j 7`. That passed on the development server and went red
on GitHub Actions the moment the clamp landed, because that box has 16 GiB:
the lending figure was clamped and the derivation gave 6. The clamp is right
and the test's assumption was wrong — it was simulating a machine larger than
the one it ran on, and nothing had been making that visible. So the exact
`-j 7` is now asserted **only on a host that really has 17 GiB**, and skipped
with its reason elsewhere (D27); the derivation itself is asserted with no
host at all in `herd_proto::placement`, and what that test now asserts on
*every* host is the thing only the real binaries can show — that the figure
on the record, the figure in the explanatory line and the figure the process
was given are one value. The net effect is one host-dependent assertion that
is now loud about being host-dependent, which is the trade D27 exists to
make.

**Alternative rejected.** Refusing the oversized override by name. Rejected
because the clamp carries the same safety and keeps the machine in the fleet;
both beat the silent accept, which is what mattered.

---

## 2026-10-13 — D90 A tmpfs figure is a capacity, and unknown is not zero

**Context.** E-31 reserves room on "the client's tmpfs". The figure could be
what is free this second, read per pass, or the mount's capacity with
reservations subtracted.

**Choice.** The runner announces the **capacity** — measured from
`--host-tmpfs-path` (`/dev/shm` by default, checked to really be a tmpfs) or
given by its owner — and what is *free* is that figure less what the records
say is declared (D87). A figure that moves under the hub between the
judgement and the claim cannot be weighed against. The consumer's tool
lowering its own scenario count when its tmpfs is short is theirs to do and
Herd never models it (D82): Herd admits against the `-j` it chose, and their
ceiling promise makes that safe.

**A figure that cannot be read is absent, and a job needing a reservation is
refused on that machine by name.** Never `Some(0)`, which is a real and
different statement — a machine that lends no tmpfs — and never an
assumption that the room is there. This is M4's unmeasurable-disk-as-zero
defect, forbidden in the one place M8 could have repeated it. A job
reserving **nothing** is of course still placed on such a machine: `build`
and `check` need no tmpfs, and `0` is how they say so.

**Alternative rejected.** Reading free tmpfs per dispatch pass. Rejected on
the moving-figure argument and on cost: it is a syscall per runner per pass
for a number that is wrong by the time it is used.

---

## 2026-10-13 — D91 E-04's cadence is a figure, with a bound stated

**Context.** E-04 wants a queued stream to report its position, what is
ahead and why "at least every 60 s". That has two failure modes and M6 had
already chosen one of them: `record_waiting` writes and tells only when the
sentence is **news**, which is right for a record — history is not a
countdown — and leaves a requester following a job stuck on one lock file in
silence for an hour. The opposite is as bad: a line per dispatch pass, and a
pass runs when a runner joins, when a probe changes its mind, when a job
ends, when one is submitted, and once a minute besides. M7 added more.

**Choice.** A clock of its own, `--wait-restatement`, 60 s by default,
printed at startup, refused at zero. One task reads the sentence the
dispatch pass already wrote on the record — it judges nothing itself — and
tells each follower again no oftener and no rarer than the cadence. News
resets the cadence, so a requester told the reason this second is not told it
again a moment later because a clock came round.

**The cadence is a deadline, not an interval, and measuring is what found
that.** A loop that wakes every `tick` and fires when a job was last told
`cadence` ago produces gaps of up to `cadence + tick`. The first by-hand
ten-minute wait measured gaps of 45 s, 60 s and **61 s** against a figure
E-04 words as *at least* every 60 s, and one second over is still over. So
the decision fires a tick early: the interval lands in
`[cadence − tick, cadence]`, the worst gap is the cadence itself, and the
tick is a quarter of it.

**The bound, stated exactly, because two paths add up.** The clock
contributes at most `W / (cadence − tick) + 1` lines — **fourteen** over ten
minutes at the default. M6's news path contributes on top of it whenever the
sentence genuinely changes, and the sentence carries a countdown (`rising to
merger in 28m`), so it changes about once a minute: `roughly` rounds to the
minute precisely so that it is once a minute and not once a second. The
figures measured by hand over two real ten-minute waits are **11 lines**
before the deadline fix and **20 after it** — the two paths interleave rather
than coincide, the clock firing at 45 s and the countdown changing at 60 s —
against the 300-odd a line per dispatch pass would have given. Twenty lines
over ten minutes is two a minute for one followed job, which is a transcript
a person reads; it is recorded here rather than tuned away, because the
alternative is suppressing a restatement when the sentence is *about* to
change, and that is cleverness nobody could debug. Writing to the *record*
stays news-only, so the history was 11 lines in both runs and not every
restatement.

E-04's "what is ahead" is now in the sentence too: the jobs in front, by id
and by level, three named and the rest counted — three because the string
lands in `waiting_for` and a sentence naming a four-hundred-job queue would
be a kilobyte of history per job per position.

**Alternative rejected.** Restating inside the dispatch pass. Rejected
because the pass's cadence is an event rate, not a clock, and because it
would put a per-follower send inside the lock D92 measures.

---

## 2026-10-13 — D92 What the whole-pass lock costs, measured

**Context.** The `dispatching` lock is held for one pass over the **whole**
queue, including every claim's and every waiting line's fsyncs. M7 booked
this as a known cost and did not measure it; M8 adds admission arithmetic
and two more figures per claim inside the same lock, so it was measured.

**The figures**, on the development server, debug build, 400 queued jobs and
no runner connected — the shape an operator meets after a restart:

* **3.31 s** for the pass that writes a waiting line on every record: 400
  records, 400 fsynced `events.jsonl` lines, ~8 ms each. This is disk, not
  arithmetic.
* **46.8 ms** for the next pass, where no sentence is news and nothing is
  written. That is what a steady-state pass costs, 400 jobs included.

**Choice.** Recorded, not fixed. The cost is dominated by the fsyncs M2's
durability asks for, and the admission arithmetic is not visible in it; the
steady-state figure — a tenth of a millisecond per queued job — is far inside
anything the first consumer's scale (tens of unfinished jobs) can notice. A
test prints both figures on every run and fails if a pass over 400 jobs ever
takes two minutes, which catches something going quadratic without pinning a
figure a slow CI box cannot meet.

**What is honestly bad about it, and left alone.** During that 3.31 s no
runner's probe snapshot is applied and no submission is dispatched. At four
hundred *queued* jobs after a restart that is a real three-second stall, and
it grows linearly with the queue. Narrowing the lock is a redesign of the
claim step's atomicity — the one thing that makes "two jobs offered in one
pass contend" true — so it is not done here, under the rule that M8 widens
into no redesign. It is written down so that the first person to see a
ten-second pass knows the shape of it and does not have to measure it again.

**Alternative rejected.** Taking the lock per job rather than per pass.
Rejected for now because a pass would then see the fleet change underneath
it, and the two-jobs-in-one-pass contention this milestone exists to get
right would stop being expressible.

---
## 2026-10-13 — D93 A hub's housekeeping dies with its own future

**Context.** `Hub::serve` spawns four background tasks — the lost-runner
watchdog, the result-write retry, the queue's aging and M8's wait
restatement — and each holds an `Arc<State>`, which holds the store, which
holds the **`flock` on the state root**. The four `abort()` calls sat at the
end of `serve`, after the accept loop. A dropped future never reaches its
end, and dropping that future is exactly what every end-to-end harness in
this workspace does: it runs `serve` in a task and aborts the task when the
test is over. So a hub that was gone went on holding its state root against
whatever came next, with nothing to say so.

It had been latent since M1 with three tasks. M8's fourth made it bite at
about **one run in six** of `herdctl/tests/exit_code.rs`, as
`the state root … is already in use by herd-hub, process N` — naming a hub
that nothing was running. Found by CI, reproduced locally, and bisected to
the task by removing it.

**Choice.** The handles live in a guard whose `Drop` aborts them, so the
tasks end with the future that owns them however that future ends. A test
asserts the property directly: bind a hub, spawn `serve`, abort the task,
and the state root opens again. Under the old shape that test fails with the
flake's own sentence.

**Why it matters past the flake.** The deployment restarts the hub in docker
compose (M16), and a hub whose previous process had not quite let go of the
root would refuse to start with a message blaming a hub that does not exist.
D21 made the lock name its holder precisely so an operator has something to
act on; a holder that is a ghost is worse than no name at all.

**Alternative rejected.** Dropping the `Arc<State>` from the tasks instead,
by passing them a `Weak`. Rejected because every one of them needs the state
for the whole of its life, so the upgrade would be in the loop body and a
failed upgrade would be a silent exit — the same leak, harder to see.

---
## 2026-10-13 — D94 A deadline is absolute and bounds a wait, not a run

**Context.** E-09 asks for "a deadline after which a job is cancelled if not
started", and the consumer's case is their nightly: start by 05:00 or skip
with the reason. Three things had to be fixed before a line of it could be
written — what the moment is relative to, whose clock judges it, and what it
does to a job that has already started.

**Choice.**

* **Absolute, not an offset.** The wire carries seconds since the Unix epoch
  (`ToHub::Asked::deadline_unix_s`); `herdctl --deadline` accepts either an
  RFC 3339 moment or a span from now (`+15m`) and resolves the span against
  the requester's clock *before* sending. An offset from the submission would
  have moved the consumer's 05:00 every time the submission itself was late,
  which is the one case E-41 exists for.
* **Judged on the hub's clock**, which is the only clock in Herd that decides
  anything, and recorded on the record as it arrived (`JobRecord::deadline`).
  A deadline read against a requester's clock would be a job cancelled
  because a laptop's time zone was wrong.
* **It bounds waiting and nothing else.** A job that has *started* is never
  touched by its deadline: the only thing that ends a running job on a clock
  is M4's time cap, exit 124. That falls out of where the judgement lives —
  the claim step only ever sees queued jobs — and it is asserted directly,
  with a job whose deadline passes while it runs and which finishes
  normally.
* **The end is `cancelled`, not `impossible`.** `Placement::Cancel` is a new
  variant beside `Impossible` because the two are different facts: nobody
  *could* ever run this against nobody *should* now. D40 gives both exit 125,
  and the words are what tell them apart.
* **Judged after the job's own contradictions, before anything about the
  fleet.** A submission whose figures contradict each other says *that*, even
  when its deadline has also passed — otherwise a malformed job would be
  reported as a late one and the bug would never be found (M7's own defect, in
  a new place). But the deadline is judged before an empty fleet and before
  the parent, because it is **D88's only bound**: a job no connected runner
  could ever fit waits deliberately and for ever, and a bound that fired only
  when a machine happened to be connected would not be one.

**What the operator reads.** `cancelled (deadline): it had not started by
<moment>, which is 4m ago, … When the moment came it was: <the whole sentence
the job was waiting on>`. The last clause is the one that matters and is
tested on its own: a job cancelled by a deadline it only missed because the
one machine of its labels was switched off is a correct cancellation and a
baffling one, unless the reason says `she is gaming`.

**Alternative rejected.** A relative deadline, "cancel if not started within
N minutes of submission". Rejected on E-41: their nightly's bound is a time of
day, and expressing it as an offset makes it drift by exactly the delay it is
meant to catch.

---
## 2026-10-13 — D95 A parent's non-success cancels its child by name

**Context.** E-10 is a `should` that E-12's pack flow needs: a test starts
after the build that makes its pack. It is also the expectation most likely to
grow into a workflow language, which invariant 1 refuses.

**Choice.** One edge, no conditions, and **success is exit 0 and nothing
else**. `Asked::after` carries at most one job id. A parent still going makes
the child wait, with the parent named and its state in the sentence. A parent
that ended any other way — a non-zero exit, a signal, `could_not_start`,
`stopped_by_cap`, `cancelled`, `lost`, `impossible` — ends the child
`cancelled (parent)`, naming the parent *and how it ended*, in the words the
parent's own requester would have read. A parent this hub has no record of is
`Gone` and is also a cancellation: a parent that cannot be read is never read
as one that succeeded.

**The cycle is unrepresentable, not merely refused.** A submission naming a
parent that does not exist is refused at the submission, by name, with the
reason spelled out. Because ids are minted by the hub in order, under one
lock, a job can only ever name one *older than itself* — so a job cannot name
itself (its id does not exist yet), and two jobs cannot name each other (the
first would have to name an id not yet minted). There is no cycle check
anywhere in Herd and none is needed, which is the point: the refusal of a
missing parent is doing that work, and a test asserts it by guessing the next
id and being refused.

**Where it is decided.** `matcher::Parent::judge`, a pure function of the
parent's standing, resolved outside the claim step by whoever has the store —
because the claim step is a pure function of a record and a fleet, and a
parent that had to be looked up inside it would make it a function of a disk.
A finished parent is not in the queue at all, so the lookup is a read of the
store; the *reading* of what an ending means is in one tested place.

**Alternative rejected.** Letting a child whose parent failed run anyway, or
waiting for ever. The first is a test run against a build that is not there;
the second is a job nobody is told about. Cancellation with the parent named
is the only ending an operator can act on.

---
## 2026-10-13 — D96 The requeue budget is three, counted apart from E-21's two

**Context.** D76 settled that a command may declare one exit code meaning "the
capability I was given went away under me" — 75 for the first consumer — and
that such a code causes a **bounded** requeue with the capability re-probed,
on a budget separate from E-21's two attempts. It left the size of the budget
and the mechanism of the separation open.

**Choice.** **Three**, as `--requeue-budget`, printed at startup. Three
because the race this bounds is small and genuine — the lease was free when
the probe was read and taken by the time the job started — so one retry is
usually enough and a second machine is worth trying; and bounded because a
lease somebody holds all afternoon would otherwise requeue a job for ever,
which is a job nobody is told about.

**How they are kept apart.** Not by two counters. An attempt carries
`requeue_code: Option<i32>` — the declared code it ended *because of*, or
nothing — and both budgets are read off the attempts every time they are
asked: `JobRecord::requeues_spent` counts the attempts that carry one,
`JobRecord::goes_a_runner_took` counts the ended attempts that do not. That
follows D79 exactly: there is nothing to leak, nothing to rebuild after a
restart, and no third place where the two could disagree.

**The defect this closes before it was written.** E-21's budget was
`if attempt < attempts_allowed` — the raw attempt *number*. A job answering 75
twice would have reached attempt 3 and been declared `lost` on the next
genuine loss, having spent on a leased machine the goes that exist for a
machine that disappears. Both call sites now count from the record, and the
unit test asserts the two counts move independently in both directions.

**When the budget is spent** the job ends `Lost`, exit 125 (D40), with a
reason naming the code, the figures (`3 of 3 goes spent`) and every machine
that answered it. Not `Exited { code: 75 }`: a 75 reaching a requester as a
verdict is the whole of what D76 exists to prevent.

---
## 2026-10-13 — D97 Three positions, a file, and what each one stops

**Context.** E-33 is the owner's condition for lending her machine at all:
`off <reason>`, `drain`, `on`, honoured in ten seconds, settable from her own
machine and from an orchestrator, with the position and its reason visible.
Invariant 3 says nothing on a runner's machine ever listens, so "settable from
her machine" could not be a port.

**Choice. It is a file**, `<data-root>/switch`, written by
`herd-runner switch <position> --reason …` and read by the running runner every
second (`--switch-poll`). Three things fall out of that and all three are
wanted: it works with the hub down, which is when an owner most wants her
machine back; it outlives the process, so a runner restarted on a machine its
owner switched off does not give itself back; and a position set *through the
hub* is written here too, so there is one state and not two. The position
rides in the handshake and on every health report, required rather than
defaulted (`PROTOCOL_VERSION` 5), because a hub that read a missing switch as
`on` would claim on a withdrawn machine.

**What each position stops.**

| | claims | serves content (D11) | job in flight | the process |
|---|---|---|---|---|
| `on` | yes | yes | runs | runs |
| `off <reason>` | **no** | **no** | **finishes** | stays connected |
| `drain <reason>` | no | yes | **finishes** | leaves when empty |

* **`off` stops serving content as well as claiming.** The owner is saying the
  machine is hers again, and a runner that stopped taking jobs while still
  pushing a 400 MiB pack up her link would have honoured half of E-33. There
  is no fetch endpoint in Herd until M11, so what exists today is the gate —
  `Switch::serves_content`, with `Session::serves_content` beside it — and the
  rule that M11's endpoint is built behind it and nowhere else. The gate is
  asserted; the endpoint cannot be, and this decision is where that is
  written down.
* **Neither position touches the job in flight.** That is the whole difference
  between the owner's switch and a shutdown: a `SIGTERM` gives the machine
  back inside five seconds and abandons what is running (invariant 2), while
  `off` is the polite version and would lose her exactly the work she was
  being polite about. A `drain` keeps serving content while it still holds a
  job, because a peer halfway through fetching what that job produced is part
  of finishing it.
* **A job assigned in the instant the switch moves is handed back**, not run:
  the check is in `take_job`, under the lock that takes the job in hand, and
  the runner abandons the attempt naming the switch. The hub then places the
  job elsewhere — nothing lost, machine given back.
* **A withdrawn machine is not *available*** (D80, extended): a job does not
  stick to it, everything queued for it is claimable elsewhere in the same
  pass, and the sentence of a job with nowhere else to go carries the owner's
  own words.

**Where the ten seconds goes.** A second of polling at worst, a frame sent the
moment the position moves rather than at the next report, and a dispatch pass
the hub makes on receiving it. Measured with the real binaries from both
sides, while a job was running; it is comfortably inside.

**A switch file that will not read leaves the position where it is**, loudly.
The dangerous reading is the other one: a mangled file read as `on` is a
withdrawn machine quietly taking work again.

---
## 2026-10-13 — D98 Drain-first is declared, and it reserves the host

**Context.** E-27 asks that a metal trial on the PC "drain the PC's build slot
first (nothing new claimed, up to 15 min)". Two constraints shaped the answer.
`herd-proto` says in so many words that nothing in Herd knows the `metal-`
prefix means anything, and D86 says a probe is declared by the machine's
operator in that runner's own capability file and never read out of a
consumer's repository.

**Choice.** A capability carries `drain_first: bool`, declared in the runner's
own capability file beside its probe. It means two things, and the second is
the one exclusivity does not give:

* **a quiet host**: a job holding this capability starts only when no other
  job of this runner's is running;
* **a reservation**: from the moment such a job is queued and eligible here,
  this machine claims nothing new.

Without the reservation a trial waiting for a quiet host would starve for as
long as builds kept arriving — which is exactly what E-27's own wording exists
to answer, and what `exclusive` alone does not do. The reservations are
computed from the queued records and the fleet **before a pass places
anything** (`matcher::reservations`), because a reservation discovered while
walking the queue in priority order would be found only after everything ahead
of the trial had been claimed. Queue order still decides *which* job owns a
contested host: the first one in it.

**Bounded, and measured from the job's submission.** `--drain-first-limit`,
fifteen minutes, the figure E-27 names. From submission because that is a
field on the record, so the bound survives a restart and cannot leak — a hub
that remembered when each drain began would have a third thing to rebuild, and
the one it forgot would hold a machine empty for ever (D79). Past the limit the
reservation is gone, the host takes other work again, and the job's sentence
says **the drain did not win**, with what to do about it, rather than going on
promising that it is about to.

**What this costs, stated.** A drain-first job reserves its host from
submission even when something *else* is also in its way — a lease held
outside Herd, say. The bound is what makes that safe, and the sentence names
the figures, but a machine can be held empty for up to fifteen minutes for a
job that could not have started anyway. The alternative — reserving only when
the quiet host is the single remaining obstacle — needs the pass to be
simulated before it is run, and the complexity is not worth it at one
declared capability on one machine.

**Nothing reads the prefix**, and a test proves it: the same label
`metal-pc`, declared without the flag, behaves as any counted capability does
and the trial runs beside a build.

---
## 2026-10-13 — D99 `herdctl switch` succeeds only once it is written down

**Context.** M9 shipped E-33's switch and its own tests passed. The defect was
found by a **docs-only pull request's CI run**: the same test,
`a_switch_off_through_herdctl_stops_claims_in_ten_seconds_and_is_written_down`,
failed on the two-core CI host with `the runner wrote its switch down: Os {
code: 2, kind: NotFound }`. Nothing in that pull request touched any code. The
twelve-thread development machine simply never lost the race, and a two-core
box tests an ordering that a fast one cannot: every assertion about *which of
two things happened first* is, on a wide host, an assertion about nothing.

The race was real and it was not only in the test. `herdctl switch <runner>
<position>` returned the moment the hub had **sent** the ask; the hub stopped
claiming when the runner's health report arrived, and the runner's own switch
file — the file that makes the position outlive a reboot (E-33) — was written
afterwards, because `apply_a_switch` applied the position first and wrote it
second. So there was a window, measured at about 1.5 s on two cores, in which
the product lied to its owner: she had been told success, the hub honoured the
position, and a runner killed inside that window came back in its **previous**
position. For a command whose whole purpose is that an owner can take her
machine back, that is the wrong answer.

**Choice.** Two changes, and the second is the one that closes the window.

* **The runner writes the file before it applies the position.** The owner's
  own path was always this way round — `herd-runner switch` *is* the write, and
  the running runner reads it afterwards — so now both paths leave the same
  state in the same order. A write that fails still applies the position: she
  asked for her machine back, and a read-only data root is not a reason to go
  on claiming work. `switch::write` also fsyncs the temporary and the
  directory, because `Ok` now means "durable" to a caller that reports success
  on the strength of it.
* **The acknowledgement travels back and `herdctl switch` waits for it.** New
  frame `ToHub::SwitchApplied { switch, persistence }`, where `persistence` is
  `Written { path }` or `NotWritten { why }`. It rides the control connection
  the runner already holds (invariant 3 — nothing listens on a runner). The hub
  bounds the wait by `SWITCH_HONOURED_WITHIN`, E-33's own ten seconds, now a
  constant in `herd-proto` because both sides spend it.

**`herd-proto` gained a frame and `PROTOCOL_VERSION` moved 5 → 6.** Also
`FromHub::SwitchSet` gained a required `confirmed: bool`. Both numbers are
free-running before M16 (D73), so this is cheap, but it is not free and it is
said out loud: a version 5 peer is refused by name. The reason to move rather
than default: a version 5 runner never acknowledges, so every `herdctl switch`
against it would report a position nobody confirmed — a true sentence about a
peer that is merely old, which an owner reads as a fault on her machine. And
`confirmed` missing would default to something, and either default is a lie in
one direction.

**An unconfirmed switch is never plain success.** `herdctl switch <runner>
<position>` exits **2** (`NOT_SETTLED_CODE`) with the sentence on stderr in
three cases: no runner of that name is connected, the runner applied the
position but could not write it, and no acknowledgement arrived in ten seconds.
Not 1, which means "the job failed" (D20), and not 125, which means a job with
no verdict — this command runs no job, and a script switching a fleet off
before a maintenance window has to be able to tell "the hub was unreachable"
from "the machine is off but may forget it". The timeout sentence names the
runner, the position asked for, **what the hub actually sees** — read, not
assumed, because the runner is the authority on the position (D98) — and what
is not true: that the runner confirmed writing it down.

**Alternative rejected: read the health report as the acknowledgement.** The
runner already sends its switch on every report and sends one the instant the
position moves, so no new frame would have been needed. But a health report
says the position is *in force*, not that it is on the machine's disk — the
runner applies a position it could not write, and rightly — and the two are
indistinguishable from the hub. A `herdctl switch` that reported success on a
health report would have been reporting something nobody had established, which
is the defect again with a smaller window.

**Alternative rejected: poll for the file in the test.** It would have made CI
green in one line and hidden the product defect for good. A test that waits for
a file to appear is a test that has agreed the product may take its time; what
E-33 is worth depends on it not taking any. The test now reads the file in the
statement after the one that read success, and the `kill -9` test kills the
runner in the statement after that — no wait of any kind in either.

**What is proved, and how.** The race was reproduced before it was fixed, by
putting a 1.5 s sleep between applying and writing: the original test failed
with the CI host's exact panic. With the fix, the same 1.5 s sleep moved to
*before* the write — the slow half, now on the honest side of the order —
leaves all twenty tests green, including pinned to two cores with `taskset`.
Pushed past the budget (a 12 s write) the command reports the partial result
instead of a false success, which is the bound working. Three tests carry it:
the file read with no wait after success, the acknowledgement timing out
(runner paused with `SIGSTOP`) reported as a partial result naming the runner,
and the position that cannot be written reported as in force and not durable.

**What is not proved.** The fsync is a code path, not a measurement: nothing
here pulls power from a machine to show that the position survived. And the
ten-second bound is spent by the *requester's* connection, so a `herdctl
switch` against a wedged runner holds one hub connection for ten seconds; that
is one command's own connection and nothing else waits on it, but it has not
been measured under a fleet being switched off at once.

---

## 2026-10-13 — D100 A test asserts only what it established

**Context.** Two of M9's tests have now gone red on the two-core CI host and
green on this twelve-thread one, and neither failure was caused by the change
under test — the second one failed on a pull request containing nothing but
documentation. D99 was the first and it was a real product defect. The second
was not: `herdctl switch` said `wsl-pc: off (she is gaming), 2 jobs running
there finish`, which is true, correct and correctly pluralised, and the test
demanded the literal `1 job running there finishes`. The test had created one
job; the other was its own: the loop that establishes "the hub has stopped
claiming" submits a job every 50 ms, and on a slow host one of those was
claimed before the switch moved.

That is one instance of one mistake, and the file was full of it. Sweeping it
under `taskset -c 0,1` with five spinning processes on the same two cores
turned up **six** failures across seven tests in `owner_control.rs`, plus one
in `herd-hub`'s own unit tests, every one of them a test asserting something
it had not established.

**Choice.** A test may assert a figure, a sentence or an ordering only when it
put that state there itself. Four rules, and the fixtures to keep them.

* **Hold work with a file, never a `sleep`.**
  `a_job_held_until_the_test_says_so` returns a script that waits for a
  path the test creates. Six jobs that were `sleep 2` or `sleep 3` are now
  held until the assertions about them are done. A `sleep` is a guess about
  how long an assertion takes, and the guess is wrong on a loaded host in the
  one direction that matters: the job ends first, and what is then asserted
  is a machine with nothing on it.
* **Make the count a fact of the fixture.** The machine in the switch test
  has one build slot and nothing detected, the job in flight is a build, and
  so are the probes — so every probe the hub accepts queues instead of being
  claimed, and the only thing ever running there is the job the test started.
  The assertion is now the **whole line**, `wsl-pc: off (she is gaming), 1 job
  running there finishes`: the machine, her own words, the figure, and
  *finishes* rather than dying, which is the whole difference between her
  switch and a shutdown. A regex that passed on any number would have made
  CI green and the test worthless.
* **Wait on the hub's view, not on what a runner printed.** `joining` and
  `switch says off` are a runner's own news, printed before the hub has the
  connection. Three tests read them as permission to submit; one of them then
  read "no runner is connected" as the refusal it was looking for, and another
  read a job id that had never been minted. `until_the_fleet_says` waits for
  `herdctl switch` to say exactly one line, and
  `until_a_submission_is_refused_saying` waits for the refusal itself —
  treating a submission the hub *accepts* as the defect it would be, and
  failing at once rather than retrying.
* **Raise a flag inside the lock it is about.** `herd-hub`'s
  `a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim` asks whether a
  probe's snapshot was applied while a claim was open. It read a flag the pass
  thread set *after* `dispatch_queued_jobs` returned — after the `dispatching`
  lock was released — so a snapshot let into that gap read it as `false`. The
  instrumented store now raises it while the pass still holds the lock, which
  a snapshot must take. The product was never wrong here; the test said it
  was, on two cores, in seven of nine `cargo test --workspace` runs — and in
  none of five runs of that target on its own, so it needs the contention to
  show at all.

**Two figures widened, and said out loud.** E-09's two tests set a deadline
`+1s` and needed the hub to judge the job before it fell. A dispatch pass is
not synchronous with the answer a submission gets, and on two loaded cores it
ran more than a second late: one test had its *running* job cancelled before
it started and then waited thirty seconds for a slot nobody would take, and
the other was cancelled with the hub's other true sentence — "nothing had been
recorded about why it was waiting" — instead of the one it asserted. Both are
now `+5s`, with the precondition waited for explicitly, and the hub in the
first is given `--aging-step 4` so that the deadline fires on its own clock
rather than as a side effect of another job ending. Five seconds is a measured
margin and not a proof: it is a figure, it is in the test next to why, and a
host five times slower again would need another.

**What is proved, and how.** Every fix was made to fail first. The hard-coded
count was reproduced with the CI host's exact panic — `2 jobs running there
finish` — in two of four runs with five spinning processes pinned to cores 0
and 1; the kill-nine switch test in two of three, the dry run's
byte-for-byte comparison in two of three, and the four found later once the
first two were fixed and stopped masking them. With the fixes, twenty tests
pass ten times out of ten under that same load, and the suite is green
unpinned. The unit test above failed in six of six `./scripts/test.sh` runs
pinned to two cores on unmodified `main` — and in none of five runs of its
own target alone, which is why nobody had seen it — and passes there now.

**The one left alone, and why.** `capabilities.rs`'s
`a_label_no_runner_offers_leaves_the_job_queued` failed once in three runs
last round and is not touched here, because it did not reproduce: twenty runs
of that file on two pinned cores under the same load, and three whole suite
runs pinned, all green. What was found instead is a mechanism that fits the
symptom exactly, and it is arithmetic rather than a guess. That test kills its
runner and starts a replacement under the **same name** with a different
instance id. While the hub still holds the dead connection — it has to notice
the close first, and that is a race a loaded host loses — the replacement is a
twin, and a twin is told to **stand by for `lost_after / 2`, which is 30 s**;
`reports_seen > 0` gates the escape hatch, and at that harness's default
cadence of 15 s the dead runner had sent none. The file's own patience is
**30 s**. The two figures are equal, so the test fails the moment the race is
lost. Proved out of band, by freezing a joined runner with `SIGSTOP` and
dialling a second under its name: `the hub asked this runner to stand by for
30s`. The answer is therefore not a bigger patience — it is for the test to
wait until the hub has let go of the old connection, or for that harness to
pass a short `--lost-after`. Named here, and left to whoever owns that file.

**What is not proved.** `until_the_state_root_is_quiet` — for the dry run that
has no single file to watch — is two identical readings 200 ms apart, which is
a heuristic and is written down as one. The `events.jsonl` line of a
transition lands *after* the `job.json` that announces it, so a reader of a
record can see a state whose transition is not yet recorded; the tests here
now wait for both files, and whether the product should write them the other
way round is not this change's question. And the load these figures were
measured under is one machine's: five spinners on two cores is a shape of
contention, not the CI host.

---

## 2026-10-13 — D101 The sweep's rule: wait for the hub, hold with a file

**Context.** D100 swept one file, `owner_control.rs`, and found eight tests
asserting what they had never established. This is the same sweep over the
other ten test files, their shared harnesses and the unit tests that touch
timing, locks or ordering. The method was D100's: `taskset -c 0,1` with
thirteen spinning processes on the same two cores, and a figure measured
before it is argued about.

**What it found, as a class.** Not one new instance of D100's first
mistake — an assertion about a count the test did not create. Three other
shapes instead, and all three are the same error wearing different clothes:
**a test that does not establish the state it then asserts against.**

* **A fixture that ends on a clock.** `slots.rs`'s restart test held its one
  build slot with `sleep 4` and then asserted that a second build did not
  overlap it. A host slow enough to spend four seconds on the restart gets
  that for nothing, from a slot that was free — the test passes having
  proved the opposite of its name. The slot is now held by a file the test
  removes, and the hub that came back must **say** the slot is taken, with
  its figures, before anything is released. `killed_hub.rs`'s tail-loss test
  had the same shape twice over, a `sleep 2` in the job and a `sleep 3` in
  the test, and is now two files: one that lets the tail be written and one
  the job writes when it is over. `confinement.rs`'s invariant-3 test slept
  400 ms and then checked the runner "while a job is running"; the job now
  says when it has started and waits to be let go.
* **A fixture that never happens at all.** `end_to_end.rs`'s
  `the_same_runner_dialling_again_replaces_its_own_dead_connection` dialled
  a second `herd_runner::Session` beside the harness's own runner. A
  `Session` mints the instance id, so a new one is a **twin** — which the
  test directly above it proves is told to stand by. The case under test
  therefore never occurred, and the assertion that followed, that a job ran,
  was answered by the harness's runner, which nothing had replaced. It now
  dials one `Session` twice with no other runner in the herd, so the job
  runs on the connection that dialled again or on nothing. The hub
  superseding a connection it is *still holding* stays where it is pinned
  deterministically, in `link.rs`.
* **A cancel that lands before the thing it is about.** `link.rs`'s
  `a_cancel_ends_one_job_within_ten_seconds_and_a_sibling_runs_on` waited
  for the job to be `Running` and cancelled it, then asserted that what the
  job had printed was still in its log. `Running` is the runner reporting a
  shell it has *spawned*, and the shell has still to reach its first
  `printf`. Pinned to two cores under five spinners the kill won that race
  in **one run of six** — and in the first full pinned run of the suite —
  and the test failed saying output had been lost when the job had never
  produced any: a true sentence about a false premise. It waits for the
  line now, and passed eight of eight under the same load afterwards.
* **A runner's own log read as the hub's view.** `capabilities.rs` waited
  for the runner to print `qemu is suspended` and then asserted, on sight,
  that the hub had been told. It is waited for now.

**And the rule it leaves behind, which is one sentence.** *Hold a fixture
open with a file the test removes, never with a number of seconds; and wait
for the hub's own view, never for a runner's news about it.*

**Figures, because the ones that were argued about turned out to be fine.**
The margins this sweep suspected were measured rather than rewritten, under
thirteen spinners on two pinned cores: the killed-hub admission test used
0.47–0.59 s of its 4 s hold, the starved reservation 0.04–0.23 s of 1.5 s,
the slots restart 0.42–0.57 s of 4 s, the two-submissions dedup 0.09–0.12 s
of 0.6 s, and the aging restart 2.95 s of its 6 s — of which 2 s is the wait
it is *meant* to take. `ten_hanging_probes_do_not_delay_the_runner_joining`
joined in 70–182 ms against its 2 000 ms bound. None of them was touched. A
measurement that says "leave it alone" is worth as much as one that does not.

**What is still a sleep, and why it was left.** Two windows remain —
`capabilities.rs`'s eight probe intervals and `recorded_offer.rs`'s, now
replaced. The first is 800 ms against a 100 ms interval that is itself wall
clock, so the arithmetic is sound and the assertion is a negative one that
can only under-prove; it is named here rather than quietly fixed.

**A diagnostic that said nothing.** `real_herd`'s `wait_until` printed
`self.said("first")` on a timeout — the hub of the tests that call their
first hub `first`, and the empty string for every other test in the file.
The twelve-kills failure below printed its sentence and then nothing at
all, which costs a rerun to learn what the suite already knew. It names
every process of the herd now.

**A product defect, reported and not fixed: see D104.** The runner's redial
backoff never resets on a connection that *served*, only on one the hub
closed cleanly, so twelve killed hubs walk it to its 30 s cap — against a
test patience of 30 s. That test now caps its runner's redial at a second;
the policy is Alexa's to set.

**A hazard found and closed.** `busy_probe.rs` and `confinement.rs` both
sweep `/proc` for a process carrying a known `sleep` duration, and both
declared those durations as constants — two of which **collide between the
two files**, and all of which collide with a second checkout of this
repository running its own suite. The duration is now
`1_000_000 + pid * 10 + n`, so the sweep can only ever find this test
process's own leavings.

## 2026-10-13 — D102 A twin's thirty seconds, and the wait that is not one

**Context.** D100 named this and deliberately left it: `capabilities.rs`'s
`a_label_no_runner_offers_leaves_the_job_queued` kills its runner and starts
a replacement under the **same name**. While the hub still holds the dead
connection the replacement is a twin, and a twin is told to stand by for
`lost_after / 2` — 30 s at this hub's default. That file's own `PATIENCE` is
also 30 s. Two equal figures, so the test fails the instant the race is lost.

**Measured, not argued.** Instrumented to record whether the hub had already
let go at the moment the replacement was started, and how long it took to
let go: under thirteen spinners on two pinned cores, the hub had **not** let
go in **eight runs of eight**, and took **220 to 640 ms** to notice the
close. The test won every one of those races only because spawning a process
and completing a handshake took longer still. It is a coin weighted in the
test's favour on this host and nothing more; a host where the hub's reactor
is starved and process spawn is cheap turns it over.

**Decision.** Not a bigger patience, and not a short `--lost-after` either:
the test waits for **the hub's own view of the loss**. A new harness method
kills the runner, reaps it, and then waits until the hub has printed
`runner … is gone` before anything dials under that name again. The
replacement is then not a twin, by construction rather than by luck.

A bigger patience was rejected because it makes the test slower in exactly
the case it is wrong about, and says nothing about why. A short
`--lost-after` was rejected because it is a second figure to keep paired
with a health cadence — the hub refuses a threshold that is not comfortably
longer than the cadence it expects — and because it changes what every other
test in that file is running against, to fix one of them.

**And it is now asserted, not merely avoided.** The test asserts that the
hub never said `asked to stand by`. Under the old shape that assertion would
have failed on exactly the runs the race was lost, instead of the test
hanging for thirty seconds and then failing for want of a runner — which is
the difference between a failure that names its cause and one that does not.

## 2026-10-13 — D103 `job.json` is never behind `events.jsonl`

**Context.** D100 left this open as a product question. A reader of a job's
directory can see `job.json` saying `running` with nothing in
`events.jsonl` yet: the record is written first and the transition line
appended after it, so for the width of one file append the summary is ahead
of the history. E-53 promises the records are readable during a run, so what
a reader may see in that window is a promise and not an accident.

**Decision. The order stays as it is, and it is the rule rather than the
window that is written down:**

> `job.json` is the authority on where a job stands **now**.
> `events.jsonl` is its history, and may be one transition behind the
> record. It is **never ahead of it**: a line in `events.jsonl` means the
> record already agrees with that line, or has moved past it.

So a reader who wants both — a state and the transition that produced it —
waits on `events.jsonl`, which is the later of the two writers. A test that
reads `job.json` as proof that the matching line has landed is reading the
wrong side of the window; none in this suite currently does, and the ones
that read both wait on the log or on a state written after the one they are
about.

**Why not the other order.** Writing `events.jsonl` first would make the
window point the other way: a reader could see `finished` in the history
while the record still said `running`, which is the authoritative file being
stale about the one thing it exists to answer. Worse, a hub killed inside
that window would leave a history claiming a transition the record never
took — and recovery reads `job.json`, so the hub that came back would
disagree with its own history for ever, with nothing to reconcile it from.
The present order fails the other way: a crash in the window leaves a
correct record and a history one line short, which is a loss of detail and
not a contradiction.

**Cost of the decision as taken.** A reader polling `job.json` alone can act
on a state a moment before its line is readable. That is the cost, it is
bounded by one append, and E-53's row in `docs/coverage.md` now says so.

## 2026-10-13 — D104 A served connection still counts as a redial failure

**A product defect, found by this sweep and deliberately not fixed here.**

`herd-runner`'s `serve_forever` resets its consecutive-failure count in one
place only: when `serve_once` returns `Ok(())`, which is the hub closing the
connection **cleanly**. Every other ending — a hub killed, a reset link, a
NAT timeout — increments it. The count is never reset by a connection that
*worked*: a runner that joined, served for an hour and then lost the link is
treated as the next consecutive failure of the previous one.

So the delays walk up and stay up: 0.5 s, 1 s, 2 s, 4 s, 8 s, 16 s, then the
30 s cap, for ever, however long each connection lasted in between. The
backoff's own documented intent is the opposite — *"500 ms is for the common
case, a hub restarted by its deployment: the runner is back before anybody
notices"* — and that is true of the first restart only. By the seventh, a
hub that is **up** is left waiting half a minute for its runners.

**How it surfaced.** `killed_hub.rs`'s `a_log_survives_twelve_killed_hubs_
whole` kills the hub twelve times in one job. Its per-round patience is 30 s
and the backoff cap is 30 s: equal figures, the same trap D102 closes in
`capabilities.rs`. It failed, unpinned, in one run of four, and the runs
that passed took 148 to 179 s — nearly all of it spent asleep. That test now
runs its runner with `--redial-cap 1`, because it is a test of a log
surviving kills and not of the redial policy, and it takes **9.9 s**.

**Worse than the backoff, and measured after the sweep reported it: the
runner *exits*.** `failing_since`, the instant the give-up bound is measured
from, is reset in that same one place — so the bound is not a span of
unreachability at all. It is the time since the runner's **first ever** failed
ending, whatever happened in between. Proved against the real binaries with
`--give-up-after 3 --redial-first 0.05`: the runner failed to dial a hub that
was not up yet, the hub started, the runner joined and served for **five
seconds**, and the first `kill -9` of that hub ended the process for good.

```
Error: giving up on ws://127.0.0.1:18791: 27 attempt(s) over 3s failed,
which is longer than a runner waits for a hub to come back
```

Every part of that sentence that matters is false: the hub had come back, and
the runner had been serving on it. At the default bound it reads: a runner
that misses one dial, then runs healthily for a day, is killed for good by the
next hub restart. That is the one failure an unattended fleet cannot have, and
the one Alexa named when he asked for M15d — nobody going back to every
machine by hand.

**And the clean close is not the common case; it is no case at all.** The
`Ok(())` arm that does the resetting wants the hub to close the WebSocket with
a handshake. Measured: a hub stopped with **SIGTERM** — the polite stop, what
a deployment does — ends the link as `Connection reset without closing
handshake`, and the runner's very next line is `dialling … again (attempt 2)`.
The reset path therefore never runs in practice, and both counters are
monotonic for the life of the process.

**Decision: report it, do not change it here.** A sweep of the tests is not
where the redial policy is rewritten, and the fix has a real trade to settle:
resetting on any welcomed connection turns a hub that accepts and dies in a
loop into a 2 Hz dial loop, so the reset wants a floor — a connection that
lasted at least the cap, say. Both counters are in scope, the failure count
*and* `failing_since`.

**What it costs until then.** Nothing today: the hub here is only ever stopped
by hand, nothing is deployed, and a runner holds its jobs and its
unacknowledged output across the gap (D36, D17). But it cannot ship to a real
fleet, so it is fixed before M16 puts a runner on the server for good.

## 2026-10-15 — D105 A connection that served clears the backoff

**Context.** D104 recorded the defect and left the policy to Alexa: the
runner's redial counters were cleared only by a hub that closed the
WebSocket cleanly, which is measurably a case that never happens. He asked
for it fixed before M10, because everything after this assumes a runner that
survives its hub.

**Choice. A connection that was welcomed and then lasted at least the cap
clears both counters, whatever ended it.** Three parts, and each is load-
bearing:

* **Welcomed, not merely attempted.** A dial that hung for a minute and a
  link that served for a minute are the same span on the clock and opposite
  facts about the hub, so the loop cannot time it — the session records the
  moment the hub welcomed it, and `Session::served_since` answers only for a
  welcome that arrived during the attempt being judged. Without that bound a
  runner that served for an hour and then lost its hub for good would read
  its own stale welcome as proof that every failed dial since had been a
  connection.
* **Both counters, not just the delay.** `failing_since` is what the give-up
  bound measures from, and it was the serious half: it made a day of
  *unreachability* into a day since this process's first bad dial. Measured
  before the fix, a runner that had served for five seconds was killed for
  good by the first `kill -9` of its hub.
* **The floor is the cap, and that is the whole argument.** A hub that
  accepts and dies in a loop would otherwise be answered every 500 ms for
  ever, and the give-up bound would never fire on it, because each short
  connection would wipe the record of the last. With the floor at the cap, a
  reset can only follow a connection that lasted at least as long as the
  longest delay this backoff would ever have waited — so **no reset can make
  a runner dial faster than once per cap**, whatever a broken hub does. The
  floor costs nothing in the case it is there for and bounds everything else.

**Alternative rejected.** Resetting on any welcomed connection, however
short, which is the obvious fix and the one D104 sketched. It turns a
crash-looping hub into a 2 Hz dial loop per runner and makes `--give-up-after`
unreachable, so a hub that is broken rather than absent is never reported. A
separate `--settled-after` figure was rejected for D102's reason: a second
number that has to be kept paired with the first, to say something the first
already says.

**Also changed, because it said the opposite.** `Backoff::default`'s comment
claimed 500 ms was "for the common case, a hub restarted by its deployment".
That was true of the first restart only; it now says what makes it true.

**Verified.** `Backoff::settled` is pure and unit-tested at and either side
of the floor, with `None` for an attempt that never got in; a second unit
test pins the stale-welcome bound. End to end against the real binaries, a
runner with four failed dials behind it joins, serves a job, has its hub
killed under it — a reset, never a clean close — and comes back saying
`attempt 2`, not `attempt 5`. That assertion is on the counter the runner
itself prints, so a connection that had *not* counted fails it loudly rather
than passing on an unestablished premise (D101). Mutated back to the old
behaviour, the test fails.

**What is left.** `a_log_survives_twelve_killed_hubs_whole` keeps
`--redial-cap 1`. Its hubs live for well under the default 30 s cap, so they
do not clear the counters — correctly, a hub dying that fast is one to back
off from — and the cap keeps the test's figures apart from its patience. It
is a fixture, and it is now the only one.

## 2026-10-15 — D106 The declaration is read where the repository already is

**Context.** M10's plan entry said a submission names "a commit of a
configured bare repository", which would have given the hub a repository to
read `commands.toml` from and a ref to resolve. Rereading the consumer's own
contract (issue #16) before writing any of it: their `[executor]` `fetch`
hook is `git fetch -q origin && git switch -q --detach {commit}`. **The
workspace fetches the commit itself.** Herd was about to be given a
repository it has no use for.

**Choice. `herdctl` reads `tools/herd/commands.toml` in the requester's own
checkout, resolves a ref to a hash there, and the resolved declaration
travels with the submission.** The hub and a runner never see TOML, never
hold a repository and never run git.

* **E-08 needs that working tree anyway.** Refusing a dirty or unpushed
  submission *naming the files* can only be done where the files are. One
  place reads git, and it is the CLI.
* **It keeps invariant 3 free.** No repository on the hub, no fetch, no
  credential, nothing to keep in sync with a push.
* **It is their rule, not an inference**: "You never interpret a command
  line." Herd carries the line and substitutes the placeholders.

**The cost, stated.** The hub trusts the requester for the declaration. That
is not new — `Asked::command` is already an arbitrary command line from a
requester — so it opens nothing that was closed, and who may submit at all
is M15's. What it does mean is that the declaration is **not** a thing Herd
can enforce against a commit: a requester that sends a line the commit does
not declare is a requester lying about its own repository, and Herd's answer
is the record, which keeps what it was sent.

**Alternative rejected.** A bare repository configured on the hub. It buys
one thing — the hub checking the declaration against the commit — and costs
git in the hub's container, a repository someone has to keep current, and a
second copy of the consumer's tree on the machine that holds every record.
Rejected for the same reason D74 kept the `labels_from` reading off the hub.

**Consequence for `docs/plan.md`.** M10's entry loses the configured bare
repository and gains the resolution and the two refusals in `herdctl`. Only
`herdctl` depends on a TOML parser.

## 2026-10-15 — D107 Three places fill a placeholder, each the one that knows

**Context.** The consumer's declaration is a template: their four executor
hooks and their command lines carry `{job} {commit} {args} {in} {out}`, their
`run` hook carries `{line}`, and Herd's own `{j}` (E-22) goes in the line.
Something has to put a value in each, and the obvious reading — "the runner
substitutes everything, it is the one running the command" — is wrong in a way
that is invisible until M6 is looked at.

**Choice. Each token is filled by the one place that knows its value, and
`{args}` and `{commit}` are filled *before the job is minted*.**

| Token | Filled by | Because |
|---|---|---|
| `{args}`, `{commit}` | `herdctl` | the requester typed the arguments and resolved the commit in its own checkout (D106) |
| `{j}` | the hub | a property of the machine it chose, derived as it chooses one (E-22, D82) |
| `{job}`, `{in}`, `{out}`, `{line}` | the runner | the id is the hub's, the directories are the runner's own |

The sharp one is `{args}`. A line still holding `{args}` when the job is minted
is a line two submissions of one command with *different arguments* share — and
M6's deduplication key is computed over the command, so the second submission
would join the first and two requesters would be handed one run of the wrong
arguments (E-05 says a second submission of **open work** joins; it does not
say two different jobs are one). Filling them in `herdctl` makes the line on
the record the line that will run, which is also what makes a record readable
years later (E-54) without the declaration file to hand.

**Three limits, each of which matters.**

* **Substitution is one pass per token, never a loop.** A value that itself
  contains a brace is text, not a template: `--only '{out}'` is an argument.
  A fixed point would turn a requester's own string into a template, which is
  the one way a declaration's trust (D106) could reach beyond the declaration.
* **A hook is one shell line, handed to one shell.** Their `fetch` is
  `git fetch -q origin && git switch -q --detach {commit}` and their `sh`
  command is a shell line by definition. "You never interpret a command line"
  is their rule, and splitting a line into an argv Herd invented is exactly
  interpreting it.
* **`{args}` is shell-quoted, one word per argument.** Pasting a requester's
  argument raw *would* be interpreting it: `--only 'a b'` would reach their
  tool as two arguments, and a `$(…)` in an argument would run. Single quotes,
  with the one escape a single quote needs, because inside them a shell
  expands nothing at all. Quoting is how an argument is preserved, not how it
  is read.

**Alternative rejected.** Carrying the arguments to the runner and filling them
there, which looks tidier — one place substitutes everything. Rejected for the
deduplication defect above, and because the record would then hold a template
rather than a command.

## 2026-10-15 — D108 The command declares its cap, the owner bounds it

**Context.** Each declared command names a `timeout_s` — 45 min for a build,
2 h for a test, 5 h for the nightly. A runner already has a time cap of its
own, given by its owner at startup, and invariant 2 requires every job to run
under an explicit one. Two figures, one job, and the first consumer's `up` step
may pull a whole warm cache — ~30 min the first time a machine runs one of
their jobs — *before* their command starts.

**Choice. `timeout_s` bounds the `run` step. The preparation steps are bounded
by the runner's own figure. Neither raises the other: the cap on `run` is the
smaller of the declared figure and the runner's, and the record says which one
bound the job.**

* A cap over the whole attempt would let a cold cache pull eat a build's 45
  minutes, and the job would be reported as a slow build — which is precisely
  the misreading D77's timestamps exist to prevent.
* A declared figure that could *raise* the runner's would let a requester
  overrule the owner of a machine she is lending (invariant 2, E-33's spirit).
  The consumer says what its command needs; the owner says what her machine
  lends; the smaller wins and is named.
* Nothing new is invented for the preparation steps. They have no declared
  figure, and the owner's cap is the explicit bound invariant 2 asks for.

**What a step's ending means, which falls out of the same reading.**

* `up` or `fetch` non-zero → the job **never started**:
  `could_not_start`, naming the step and its code, and never an exit code a
  caller could read as a failing test (D20).
* `run` → **the verdict**, worn untouched: 0, 1 and 75 alike (E-02).
* `down` non-zero → recorded on its step line and **cannot change the
  verdict**: the verdict already exists, and a workspace that failed to be
  taken down is an operator's problem, not a test result.
* `down` runs however the three before it ended. Not a condition — the
  opposite: a step that ran only on success would leak a workspace on every
  failure, and the four steps must have no conditions at all (D77,
  invariant 1).

**And the steps are reported as they end**, not only with the verdict
(`ToHub::StepFinished`). A 5 h nightly stopped at its cap, or a runner lost
under a cache pull, would otherwise leave **no** step on the record — the
preparation time invisible in exactly the case someone is looking for it. The
complete list arrives again with the verdict and takes over, so the frame needs
no acknowledgement and no spool: it is neither output nor a verdict, and a link
that dies under it costs the record nothing it does not get back.

---

## 2026-10-15 — D109 Deployment outranks breadth: M16 pulled forward

**Context.** The owner's build server was OOM-killed on 2026-10-15 by a fleet
of agents running unbounded builds. That is not an accident of the day: it is
the pain the first consumer's sheet already measured (load median 29, p90 46
on 12 threads; the PC OOM-killed; 71 % of detached runs red) and it keeps
happening **because Herd is not yet there to bound it**. Nine milestones are
merged and none of them is in production, so none of them has prevented
anything. Alexa's ruling: get to M16 very fast, cleanly but fast.

**Choice.** The path to a deployed hub is **M10 → M15 → M16**, then
**M15d → M17**. M11, M12, M13 and M14 move to after M16.

Two corrections to the plan's own reasoning that this exposed:

1. **M15d was wrongly placed before M16.** The plan itself says the server's
   native runner is updated by Nix (E-47), so self-update buys nothing for the
   first deployment; it serves the non-Nix hosts. It is owed before the PC
   joins, not before the hub ships. One whole milestone left the critical path.
2. **"Nothing is exposed until a job can be stored and observed" was too
   strong.** A first deployment needs a job that can be declared, confined,
   admitted, placed, switched off and authenticated. Artefacts, a status
   document and a scheduler's clock make Herd *broader*, not *deployable*.

**What this costs, stated rather than discovered later.**

- No M11/M12: a build and its test must run on the same runner. No
  build-here-test-there, and no artefact custody.
- No M13: the consumer's dashboard sees nothing of Herd and keeps its own
  status files. Their own E-53 permits exactly this for a transition period.
- No M14: their cron and their push hook keep submitting on their side.
- No M15d: updating a runner needs a hand on its host. Nix covers the server.
- **M16 bounds the server; it does not unload it.** The PC has no runner until
  M17, and M17 before M11 gives a PC good only for `check` work, since without
  a pack a build there produces nothing the server can test from and the
  owner's rule denies the PC `kvm`.

**Alternative rejected.** Finishing M11-M14 first, as the plan had it, and
deploying a complete Herd once. Rejected because every week of breadth is
another week of the exact failure Herd exists to prevent, on the machine the
project is built on. Deploying narrow and growing in production is the cheaper
error here.

**Consequence for Q7.** Deploying earlier freezes the job-record format and
the `ran on:` line earlier, and M16 still sets `PROTOCOL_VERSION` and
`RECORD_VERSION` to 1 and freezes them (D73). The status and event formats do
not exist yet, so they stay free — a benefit of this order, not a cost.

---

## 2026-10-15 — D110 E-02's line is a frame, not a log line

**Context.** E-02 wants a job's output to begin with one line
`ran on: <runner> (<why>)` and to end with the consumer tool's own verdict and
exit code, unchanged. The first implementation printed the line the moment the
frame arrived, which put a `ran on:` line on the stdout of a job that **never
started** — breaking D16's rule that stdout stays empty when Herd could not run
the job at all.

**Choice.** Three separations, each load-bearing:

1. **The hub reports the placement, `herdctl` writes the sentence.** The hub
   sends a `RanOn` frame; the human-readable line is composed at the requester.
2. **The line is not part of the job's log.** The record's log stays exactly
   the bytes the process wrote (D18). The frame is presentation, not output.
3. **The line is held until there is output to frame.** It is released just
   ahead of the job's first output byte, or with the verdict if the job
   produced none, and **never** for `could_not_start` — which is what keeps
   D16's empty stdout true. A job that produced no output at all still gets
   the line: it did run somewhere, and that is what the line says.

`OUTPUT_FORMAT_VERSION` rides **inside** the parentheses (Q7), so a reader
anchored on the closing `)` still matches `^ran on: (\S+) \((.*)\)$`. The
inverse of the formatter lives in `herd-proto` beside it; no test and no crate
keeps a hand-rolled copy of the format.

**A lock widened with it.** The claim lock now covers follower registration: a
job is dispatchable the instant `submit` returns, so a pass fired by anything
else would have sent the line to a follower list the requester had not yet
joined.

**`herdctl`'s declared-command surface.** `--command` is exclusive with
`--label` and the three `--*-mib` values: the declaration is the authority, and
a flag silently losing to a file is worse than a refusal. `--priority` is not
exclusive; which level won is said on stderr.

---

## 2026-10-15 — D111 Two limits of E-02 that Herd cannot enforce

**Context.** Found while making E-02 real against a fixture consumer.

**"Ends with the tool's verdict" is a constraint on the consumer's
declaration, not on Herd.** The fixture's `down` hook printed its step log on
stdout, putting teardown chatter after the verdict; moving it to stderr gave
the fixture E-02's shape. But a consumer whose own teardown writes to stdout
breaks "ends with the verdict" by itself, and no amount of care on our side
prevents it. Herd guarantees the *first* line and that it never alters the
job's bytes; the *last* line is the consumer's to keep. **This belongs in the
M18 conversation with chatOnS**, not in a silent assumption.

**A runner that names no memory floor admits nothing.** A declared command with
a build-thread figure makes the hub derive a memory need; a runner started
without a host size and floor then admits nothing, and the job waits for ever
on a machine that could have run it. The behaviour is correct — admission
against an unknown host must refuse — but silence is not. **M16 owes a loud
failure at startup**: a runner that cannot admit any declared work says so
when it starts, rather than queueing jobs for ever.

---

## 2026-10-15 — D112 The test-side reader of E-02's format, and one stray file

**Context.** Two findings from integrating M10's framing with M15's
authentication. Both small; both the kind of thing that rots if unwritten.

**The inverse of E-02's format exists three times on the test side**
(`herd-hub/tests/real_herd::job_output_bytes`, `herdctl/tests/exit_code.rs`,
`herdctl/tests/declared_end_to_end.rs::framed`), because a test support module
cannot cross a crate boundary. Invariant 4 is **not** broken: the format itself
has exactly one definition, in `herd-proto`, and all three call
`herd_proto::unframe`. What is duplicated is only the *insistence* — the panic
message saying the line must be there. **Decision: leave it.** Three thin
wrappers around one definition is cheaper than a test-only public surface on
the contract crate, which would then need its own compatibility care. If a
fourth appears, move the insistence into `herd-proto` behind a test feature.

**`hub.out` and `hub.err` were tracked at the repository root**, committed in
`213fcf1`: 120 bytes of a `herd-hub` usage error from somebody driving the
binary by hand, and an empty file. Removed, and named in `.gitignore` —
deliberately not as `*.out`/`*.err`, since a test fixture may legitimately be
called `expected.out`. Nothing sensitive was in them, and that is the point
worth recording: a stray redirection at the root of a public repository is a
leak waiting for the one time it is not empty.

---

## 2026-10-15 — D113 A connection's liveness clock is the health report alone

**Context.** Found while chasing a test that failed once in five full-suite
runs and never again in eighty-eight targeted ones. The flake was not the
finding; this is.

The hub writes a connection off when `last_health.elapsed()` passes
`lost_after`, and **only** a `Health` frame resets `last_health`. A frame about
a running attempt resets `attempt_seen`, never the connection's clock. So a
runner that is streaming a job's log steadily, but whose health task is
starved or wedged, is written off as lost and its **live** attempt is written
off with it — requeued once at its priority (E-21).

**Why it is defensible.** The health report is the declared liveness signal
(E-40: every ~15 s; silence for ~60 s is lost, E-21), and it is also the only
frame carrying the figures admission needs — free memory, free tmpfs, load. A
runner that cannot report health is a runner the hub cannot admit against.

**Why it is a limit worth writing down.** It is asymmetric with
`attempt_seen`, and it is the mechanism that makes any test with a short
`lost_after` sensitive to machine load. More to the point for the first
deployment: the first consumer's builds run tens of minutes on a host whose
measured load p90 is 46 on 12 threads. A starved health task there would
requeue a thirty-minute build that was visibly alive.

**Decided: leave the behaviour as it is for now, and record the limit.** The
cheap correction, if it proves needed, is to separate the two questions the
clock conflates: *is this runner alive* — any frame answers it, so a live
attempt is never written off — and *are its figures fresh* — only a health
report answers that, and stale figures should stop **new admissions** rather
than kill running work. That is the shape to reach for; it is not in M10 and
it is not in M16's definition, so it is Alexa's call whether it precedes the
first deployment.

---

## 2026-10-15 — D114 The shared declared-command harness approves its runner

**Context.** M15's E-48 (an unapproved runner is given no job) and D74's
reading (a job's requirements are read by an ordinary job on a runner) were
written in parallel and met in the merge.

**The interaction worth naming.** Because a reading *is* an ordinary job, an
unapproved runner leaves the reading unclaimed, which leaves the real job
waiting on requirements nobody will ever read — and nothing fails loudly. The
failure is silence, which is the mode this project keeps legislating against.

**Choice.** Every harness that lends a real runner names it and approves it,
through the one `approve_runner` path, exactly as `confinement.rs` does. No
test holds its own notion of an approved runner.

---

## 2026-10-15 — D115 `wss://` with rustls and the bundled webpki roots

**Context.** `wss://` did not work at all: `tokio-tungstenite` was pulled with
default features, so **no TLS crate existed in `Cargo.lock`** — no rustls, no
native-tls, no openssl. The hub never needed TLS (Cloudflare terminates it in
front), but `herd-runner` and `herdctl` are the side that *dials*, and a client
with no TLS cannot begin a connection it is Cloudflare's job to end. M16 is
unaffected — the server's runner and every requester reach the hub on loopback
`ws://` — but M17, the owner's PC dialling in from another machine, was
impossible.

**Choice.** rustls, with the **bundled webpki roots**, and no trust-this-CA
option. The reason is specifically that Cloudflare fronts the hub: the only
`wss://` dial is to a publicly trusted certificate, and a frozen Mozilla root
set verifies it on a host whose own trust store we know nothing about. Not a
general preference — if the hub were ever fronted by a private CA instead, this
is the decision to revisit, and it would need a flag.

`rustls` is named in the workspace manifest **for its crypto provider alone**
(`ring`): tokio-tungstenite pulls rustls with no provider, and
`ClientConfig::builder()` *panics* without one — a latent process abort on the
first `wss://` dial, not a connection error.

**Alternative rejected.** The host's trust store, and OpenSSL or native-tls —
the latter for the system library a runner would then need on every machine it
is dropped onto.

**Proof, not argument.** Before: `URL error: TLS support not compiled in`.
After, against a real endpoint: `HTTP error: 200 OK` — the handshake completed
and the certificate verified; the server merely answered HTTP instead of a
WebSocket upgrade. Tests assert a real ClientHello reaches the wire and that
the failure is the peer's alert, never the missing-feature string.

---

## 2026-10-15 — D116 A job's cgroup bounds cpu, io and pids, and says so

**Context.** The runner wrote exactly one cgroup control, `memory.max`. CPU and
IO were left to `nice 10` and the idle IO class, so a job ignoring the `-j` it
is handed took every core. The first consumer's two incidents of 2026-10-15
were a global OOM and a two-minute swap and IO freeze that took every docker
service on the host down; Alexa approved these bounds and the consumer asked
for them in writing.

**Choice.** Beside `memory.max`, in each job's cgroup: `cpu.weight` **10** by
default, `io.weight` **10**, `pids.max` **1024**, and `cpu.max` **absent** by
default. The split matters: the three with defaults report loudly when the
controller is withheld, while `cpu.max` — which claims nothing unless asked —
**refuses the startup** when it is asked for on a host that cannot enforce it
(D23's refusal over silent degradation). Weights outside `1..=10000` refuse the
startup naming the range. Nothing gates on load: the consumer's E-32 says the
load figure is recorded and never a criterion.

**`io.weight` is written and ignored on this host, and the runner says so.**
Measured: no device offers `bfq` anywhere (`[none] mq-deadline kyber`), and
`/sys/fs/cgroup/io.cost.qos` is empty — blk-iocost enabled for no device. So
the block layer has no per-cgroup weighting to apply. Worse, and this is the
part to carry forward: **the idle IO priority the runner already sets is itself
inert on a `none` scheduler.** As it stands Herd therefore has *no* effective
IO bound on this machine, and the runner prints that in its startup report
rather than implying a bound it does not have.

**Deliberately not built.** `io.max`, an absolute throughput or IOPS ceiling,
which is the one IO lever that works whatever the scheduler is. It is the
honest next step if IO must actually be bounded, and it was not asked for.

---

## 2026-10-15 — D117 Two traps between a green suite and a working deployment

**Context.** Both found while making M16 real; both would have cost hours on
the server and neither is visible from a test run.

**`Delegate=yes` only delegates to the scope's *only* process.** The
measurement that cgroup delegation closes the two memory skips holds for a
*bare* command: `systemd-run --user --scope -p Delegate=yes -- herd-runner`.
Put a wrapper in the scope — `… -- timeout 5 herd-runner` — and enabling
`+memory` returns `EBUSY`, the runner falls back, and the run silently proves
nothing. Whoever verifies the ceiling on the real machine must check the
runner's own startup line (`memory ceiling: one cgroup v2 subtree per job,
enforced by the kernel`) rather than trusting the scope.

**`herdctl` has no heartbeat and no redial.** The runner is fine: a `Close`
frame returns success, spending no failure counter, and its health report every
15 s means its link is never idle — Cloudflare's idle timeout, around 100
seconds, cannot reach it. A requester is different. A *queued* job is safe,
because the hub restates why it waits every 60 s. But a **running** job that
prints nothing for 100 s sends its follower no frames at all, so a requester
dialling `wss://` through Cloudflare would take the close, report that it has
no verdict and **exit 125 while the job succeeds**. A wrong answer, not a
degraded one — and a merger reading 125 would conclude the work never ran.

Unreachable at M16, where every requester is on loopback `ws://`. **It must be
closed before the first requester is remote**, and the cheap shape is the hub
keeping a running job's followers warm on the same cadence it already uses to
restate a wait, so that no client needs its own heartbeat.

---

## 2026-10-15 — D118 The hub keeps a running job's followers warm

**Context.** D117 named the defect and left it for "before the first remote
requester". Closing it is half a session and leaving it open means the first
real deployment can answer 125 on a job that passed. It is being closed now
rather than being carried as a known wrong answer.

**Choice.** A new additive `FromHub` frame, silent, carrying only the job id,
sent by the **hub** to every follower of a **non-terminal** job when nothing
has gone out on that connection for the cadence. The cadence is a `pub const`
next to `HEALTH_EVERY` and deliberately the same 15 s, so there is one number
to remember for how often a quiet Herd link speaks. `herdctl` handles it by
name and prints nothing: not stdout, not stderr, not counted as job output,
and it cannot disturb E-02's framing. A terminal job gets none — its stream
is over.

**Why at the hub and not in the client.** Every future requester inherits the
fix without learning anything about timing, and there is one place where the
cadence lives. A client-side heartbeat would have to be added again to every
tool that follows a job, and a tool that forgot it would fail exactly the way
D117 describes.

**Rejected.** Reusing `JobNote`: a note is text a person reads, and a liveness
frame is read by nobody — a client that printed it would put noise between
E-02's first line and the job's first byte. Also rejected: making `herdctl`
redial and resume, which is strictly more code for the same symptom and still
leaves a silent link silent.

The cadence is injectable so the end-to-end test does not sleep 15 s.

---

## 2026-10-15 — D119 Admission reads the room it is in; PSI is a figure

**Context.** The first consumer asked for two things in one breath: that
admission read `MemAvailable` and PSI, and that PSI act as a brake. The second
half collides with E-32, which says the load figure is **recorded, never a
gate**.

**Choice, split in two.** The memory half is taken, and in a stronger form
than asked: admission reads the room in **the cgroup the runner is actually
in**, not the host's `MemAvailable`. A runner inside a systemd slice carrying
`MemoryMax` has less room than the host advertises, and admitting against the
host's figure is how a slice gets OOM-killed while Herd believes it was
careful. Where there is no enclosing limit the host figure is the answer, so
the stricter read costs nothing on a bare machine.

The PSI half is refused as a gate and taken as a figure: pressure is recorded
with the job and reported, and it never refuses an admission. E-32 is the
consumer's own expectation, and a brake that silently stops dispatching is
indistinguishable from a hub that has died — the failure mode it would
introduce is worse than the queueing it would prevent.

**Rejected.** Making PSI a gate behind a switch: a switch whose default is off
is not a feature, and whose default is on breaks E-32 for everyone.

---

## 2026-10-15 — D120 `io.weight` does bite, on one of this host's two disks

**Correction of D116.** D116 recorded `io.weight` as inert here on the
evidence that no device offers `bfq` and that `/sys/fs/cgroup/io.cost.qos` was
empty. The first half is still true. The second was a reading taken **before
anything had written an `io.weight`**, and that is the whole mistake:
blk-iocost runs with `ctrl=auto`, so the first non-default `io.weight` written
to a cgroup **enables the controller itself** on any device whose cost model is
known. Herd's own write is what turns it on. After the M16 suite ran, that
file reads:

```
259:0 enable=1 ctrl=auto rpct=0.00 rlat=5000 wpct=0.00 wlat=5000 …
```

**The real shape, measured.** `259:0` is `nvme0n1`, the ext4 disk that carries
`/mnt/ssd_980`, and it has a populated linear cost model. IO to that disk
**is** weighted per cgroup. `rpool` — ZFS, a mirror of two SATA SSDs, carrying
`/` and `/tmp` — has no iocost entry and gets none: ZFS does not attribute its
pool IO to the writing cgroup, so no cgroup IO knob reaches it, `io.weight`
and `io.max` alike.

So whether Herd has an effective IO bound **depends on where the job works**,
not on the host. On the NVMe it has one. On ZFS it has none and never will.
That is also why the suite's `io.weight` assertion skipped rather than proved
anything: its scratch path was under `/tmp`, which is ZFS — the skip said so
in as many words, which is exactly what D27's loud skips are for.

**Consequence for the first consumer.** Their slice-level `IOWeight`, the
mitigation for the 19:15 freeze, is **not** inert as a mechanism — but it is
inert for everything their build does on ZFS. If that freeze was ZFS IO, the
knob cannot reach it at any value. The lever that works is to put the work
that must be bounded on the NVMe, and `io.max` is a second lever **there and
only there**.

**What this changes in Herd.** The runner's startup report must decide
"written but inert" from the **device its job directory is actually on**, not
from the host's IO schedulers — the check it has today would call the NVMe
inert. That is the next piece of work, and it is the honest version of the
line the runner already prints.

---

## 2026-10-15 — D121 D78's Docker promise, restated so it is true

**Context.** D78 commits Herd to three checkable things, and the first is
"the string `docker.sock` appears nowhere in it". Writing the test for the
second one proved the first **false as literally written**: `git grep
docker.sock` finds it in `herd-runner/src/privilege.rs`, as two of the string
literals in `RUNTIME_SOCKETS`.

**Choice.** Restate the promise rather than keep a claim a `grep` refutes, and
rather than delete the warning the claim was protecting. The sense of D78 was
never "the characters are absent"; it was "Herd never talks to that socket".
So the promise is now:

> No crate in the workspace depends on a Docker client, and the only place
> the workspace names `docker.sock` is the list `privilege.rs` warns about
> and the test that watches it — nothing opens, dials or executes it.

Both halves are checkable, which is the point: the whole of `Cargo.lock`
contains nothing matching `docker|bollard|shiplift|containerd|podman|oci`,
and there is no HTTP client in the graph beyond `tokio-tungstenite`; and
`RUNTIME_SOCKETS` has exactly two readers, the startup warning and the test.

**Rejected.** Deleting the warning to make the original sentence true. The
warning is what tells an owner that a uid is not the whole story — a user in
`docker` is root through that socket whatever the process's uid says — and
D78 keeps it on purpose. A promise is worth less than the thing it describes.

`docs/plan.md`'s M16 paragraph carried the same sentence and now carries this
one.

---

## 2026-10-15 — D122 What the startup reaper cannot tell apart

**Context.** The `reclaim` test failed in isolation as readily as under load,
and I suspected the reaper. It is sound, but chasing it named three shapes the
sweep genuinely cannot distinguish. They are written where the sweep reads
`environ`; they are here because a limit nobody records is rediscovered.

The sweep finds a past life's processes by reading `/proc/<pid>/environ` for a
marker naming the runner that minted them, and kills those whose minter is
gone.

**The fork/exec window is not a defect, and the reason is structural.**
Between `fork` and `exec` a child's `environ` is still its *parent's*. A
sweep in that window sees a job's process as unmarked — which is exactly why
the old fixture, which slept 100 ms on the written belief that "the
environment is readable as soon as the process exists", saw **zero** marked
processes. But the sweep hunts processes of a runner that is *gone*: those
`exec`ed long ago, and a child one of them forks during the walk inherits a
**marked** parent's environ, so it is visible anyway. The only invisible shape
is a marked child of an unmarked parent, and the only unmarked parent minting
marked children is a **live** runner starting a job — whose jobs the sweep is
required to skip. So no job process can be missed.

**Two shapes are permanent when they happen**, because the sweep runs once at
startup and there is no second pass. A **zombie** minter reads as alive —
narrow, since it needs a new runner to start before the supervisor reaps the
old one, and systemd reaps at once. A **reused pid** reads as alive, which is
inherent to naming a runner by its pid; closing it needs an identity a pid
alone cannot give, the start time from `/proc/<pid>/stat` kept beside it.
Neither is covered by the cgroup half either: it asks the same question of the
same pid.

**Not fixed, deliberately.** Both are narrow, and the cost is a leftover
process rather than a wrong verdict. Recorded so that a leftover process on a
machine running two runners is recognised instead of investigated — and
because the reused-pid case is the one that will bite first when M17 puts a
second runner on one box.

---

## 2026-10-15 — D123 Four things a working first start taught us

**Context.** All four came out of making `docker compose up -d` actually work
on a fresh volume, and none of them is visible from a green test suite. The
compose file's own comment had anticipated the first failure mode and still
did not avoid it, which is the lesson in itself: a comment is not a test.

**One path, not three.** The image bakes `/var/lib/herd`, owned by the hub's
uid, precisely so docker seeds a fresh named volume from it and the volume
inherits that ownership. The compose file mounted `/var/lib/herd/state` — a
subdirectory the image does not contain — so docker created the mount point
itself, as root, and the non-root hub refused to start, naming the directory.
Correct refusal, broken deployment. The mount, `HERD_STATE_DIR` and the
image's baked directory are now the same string. The hub's own relative
default still resolves inside the volume, so an operator who forgets the
variable gets a working hub whose state is still on the disk he would back up.

**Docker ignores `uid`, `gid` and `mode` on a file secret outside swarm.** No
warning, no effect: the host file is bind-mounted as it is. A root-owned 0400
token — what `env.example` told the operator to make — is unreadable by the
hub, which fails with `reading the requester token … Permission denied`. The
token is now 0444 inside a 0700 directory: the **directory** is what keeps it
off the host, the mode is only what the container sees, and the image holds no
users but `herd` and root. The runner's `EnvironmentFile=` stays 0400, because
systemd reads it as root — the two are different for a reason, and both say so
where they are written.

**The startup reaper is host-wide, not data-root-wide.** A runner on a brand
new, empty data root reaped a stray process left by an unrelated test suite:
the sweep walks all of `/proc` for the marker and kills anything whose minting
runner is dead, whichever root it belonged to. That is deliberate — nothing
else will ever collect them — but it means a second runner on one box collects
the first's leftovers. **This is the thing to remember before M17**, and it
compounds the reused-pid limit in D122: two runners on one machine make the
"is the minter gone?" question answerable wrongly for the first time.

**The reverse-proxy fragment does not fit the machine it is for.** It is
nginx; the server runs **Traefik v3 and cloudflared**. The fragment's trailing
note says Caddy, Traefik and HAProxy "each need exactly this" and then gives
no Traefik configuration, so the README's proxy step is prose, not an
instruction, for the only host it will ever be applied to. What is needed is a
router on the existing TLS entrypoint plus a `serversTransport` carrying
`forwardingTimeouts.readIdleTimeout` — and nothing about the `wss://` half of
M16 has been exercised, only the loopback path the server's own runner uses.
Also cosmetic but real: `herd-runner.service` declares
`Documentation=file:///etc/herd/joining.md`, which nothing in `deploy/`
installs, so `systemctl help herd-runner` fails on a fresh install.

---

## 2026-10-15 — D124 An additive frame to a requester is a breaking change

**Context.** Found while closing D118, and it corrects a belief I was about to
write into the frozen protocol version: that a frame a peer has never heard of
is harmless because serde has defaults.

**It is not, for a requester.** `herdctl`'s unknown-frame arm is a `bail!` —
"the hub sent X, which a requester cannot use" — which ends the run with **no
verdict**, and so exits 125. An old `herdctl` meeting a new frame therefore
fails a job that may well have succeeded, which is the exact shape of defect
D117 named and D118 closed. Serde's tolerance never enters it: the frame
parses, and then the client refuses it on purpose.

**So the rule, from the freeze at version 1 onward:** adding a `FromHub`
variant that can reach a **requester** is a breaking change and must move
`PROTOCOL_VERSION`, whatever the wire format would tolerate. A variant that
only ever reaches a runner is a separate question, to be answered by reading
the runner's own fallback rather than assuming the two clients behave alike.

**Why the `bail!` stays.** It is the right behaviour: a requester that cannot
understand what the hub is telling it must not pretend to have a verdict. The
cost is paid by the *deployment*, in a version bump, which is where D73 wants
it paid — not by a client guessing.

This is also why D118's frame cost nothing: it is sent only to followers, and
a peer that has never heard of it is a peer that never receives it. That is a
property of **where** it is sent, not of it being additive, and the two must
not be confused again.

---

## 2026-10-15 — D125 A queued job is not safe either

**Correction of my own reasoning in D117 and D118.** Both say a *queued* job's
follower is protected because the hub restates why it waits. That is wrong in
one case, and the case is not rare: the restatement is `--wait-restatement`,
60 s by default — four of D118's cadences — and a job for which the hub has
not yet found a `waiting_for` reason is restated **never**.

So the keepalive goes to every **non-terminal** job, queued as well as
running, and a note the hub has just sent postpones the next breath by a
cadence so the two never double up. The implementation does this; this entry
is here because the reasoning in the two entries above it is what a reader
would otherwise carry away.

The general shape of the mistake, worth keeping: "another mechanism already
covers that case" is only true if that mechanism fires **unconditionally**.
This one fires once a reason exists.

---

## 2026-10-15 — D126 The server runner's data root goes on the NVMe

**Context.** D120 established that `io.weight` bites on `nvme0n1` and never on
ZFS. `deploy/herd-runner.service` sets `--data-root %S/herd-runner`, which is
`/var/lib/herd-runner` — and `/var/lib` on this server is `rpool/root`, ZFS.
So the unit's own `IOWeight=50`, and every per-job `io.weight` under it, would
have been decorative: a runner whose jobs work on ZFS has no IO bound at all,
and the startup report would have said so in as many words.

**Choice.** The server runner's data root moves to the ext4 NVMe —
`/mnt/ssd_980/herd-runner`, 347 GiB free against a 20 GiB per-job disk budget
— created ahead of time, owned by the runner's user, mode 0700. That is also
simply the right disk for build scratch: it is the fast one, and a build's
writes stop competing with the pool that carries `/`.

**What is given up.** `StateDirectory=herd-runner` created the directory,
owned it and set its mode before the runner started, so the flag and the
directory could never drift. Off the state directory, that becomes the
operator's to get right — so the directory is made with the user, in the same
step, and the runner refuses to start on a root it cannot use, naming the
path. The failure mode is loud either way; what is lost is that it could not
happen at all.

**Why not leave it and accept no IO bound.** Because the first consumer's
freezes are IO, this is the one machine they build on, and an `IOWeight` line
that does nothing is worse than no line: it reads, to whoever looks next, as
a bound that exists.

**Not fixed by this.** ZFS still gets no cgroup IO knob, so anything a job
writes *outside* the data root — and D123's note that Herd does not stop a
job naming an absolute path — is still unbounded. And `io.weight` is a
weight, not a ceiling: it shares contended bandwidth, it does not cap it.
`io.max` remains unbuilt and remains the only hard IO lever.
## 2026-10-15 — D127 The `wss://` half of M16, for the proxy that is there

**Context.** D123's last paragraph: the reverse-proxy fragment is nginx and
the server runs Traefik v3 behind cloudflared, so the README's proxy step was
prose rather than an instruction for the only host it will ever be applied
to. Nothing of the `wss://` path had been exercised.

**Choice.** A Traefik v3 **file-provider** fragment,
`deploy/reverse-proxy/traefik-herd-hub.yml`, beside the nginx one rather than
replacing it: one file per proxy, the README sends the reader to the one that
names his, and the fragment's old claim that Caddy, Traefik and HAProxy "each
need exactly this" is replaced by what actually differs between them. The
file provider and not docker labels because a `serversTransport` — the
timeouts — can only be *defined* in a dynamic-configuration file, and because
it keeps Herd's compose file free of one proxy's vocabulary.

**The hub has to be reachable from the proxy's container, and was not.** A
loopback publish is a DNAT rule for 127.0.0.1 alone, so the containerised
Traefik on this host cannot reach `127.0.0.1:8787` at all — the fragment
would have 502'd on a first try. `deploy/docker-compose.traefik.yml` is an
override that puts the hub on the proxy's existing external network under the
alias `herd-hub`, keeping the loopback publish for the host's own runner and
`herdctl`. It opens no port on any interface. The alternative, publishing on
the docker bridge gateway and dialling `host.docker.internal`, is what
another stack on this host does and is rejected here: it offers the hub to
every container on the machine rather than to the proxy's network.

**Traefik v3 needs nothing for the upgrade, and `readIdleTimeout` is the
wrong knob.** v3 proxies HTTP/1.1 and hijacks an upgraded connection
inherently; nginx's `Upgrade`/`Connection` ritual exists because nginx
proxies as HTTP/1.0, and copying it over would be cargo. D123 suggested
`forwardingTimeouts.readIdleTimeout`; that field drives **HTTP/2** ping
frames and has no effect on an HTTP/1.1 WebSocket backend, so it is
deliberately absent and the fragment says why. What is set is
`idleConnTimeout` 300 s, `dialTimeout` 10 s, `responseHeaderTimeout` 30 s.
The client-side figures (`respondingTimeouts`) are *static*, per entrypoint,
and shared with every other service on the host: not Herd's to change, and
not needing it — all three defaults are far above the cadence.

**The cadence is what makes the timeouts a non-problem.** Every Herd link
carries a frame at least every 15 s — a runner's health report, and since
D118 the hub's keep-warm to a running job's followers. So each figure on the
path has only to beat 15 s, and the outer constraint, **Cloudflare's ~100 s
idle window, is a figure that can change under us** and is named as one. If
it ever fell below 15 s, `HEALTH_EVERY` is what moves, not this file.

**Nothing buffers.** Traefik buffers neither direction unless the `buffering`
middleware is added — which would kill the handshake outright — and
`maxRequestBodyBytes` lives inside that same middleware, so there is no size
cap to raise. No middleware at all on this router: forward auth answers a
WebSocket upgrade with a 302 to a login page, which no runner can answer, and
an IP-reputation bouncer can ban a runner that redials with backoff.

**The certificate, written where the operator chooses one.** Cloudflare holds
it and terminates TLS; Traefik holds none on the tunnel path; Herd holds none
anywhere. Because the clients verify against the bundled webpki roots with no
trust-this-CA option (D115), a Cloudflare **Origin CA** certificate, a
self-signed one, Traefik's built-in fallback certificate, or a hostname two
labels deep under Universal SSL each make every runner and requester fail to
dial — as a TLS error, before the hub sees a byte, so Herd cannot explain it.
That is the trap most likely to cost an hour and it is stated three times:
the fragment, `deploy/README.md`, and `docs/joining.md`.

**What is proven and what is written down.** Proven offline: the fragment
loads in Traefik v3.7.14 — router, service and serversTransport created, no
unknown field (checked with a deliberate typo, which Traefik rejects by
name) — and the compose override merges. **Not proven: one real dial through
Cloudflare**, because proving it means changing a live shared proxy on a host
full of other people's services. Instead `deploy/README.md` gains "Step 4,
exactly": the walk, as commands with their expected output, including a
240-second quiet link — past Cloudflare's ~100 s, Traefik's 180 s and 60 s at
once — and a table mapping *the second a link dies at* to the component that
closed it. A guess written as a measurement is worse than a gap, so the
section says at the top that it has never been run.

**Also.** `herd-runner.service` pointed `Documentation=` at
`file:///etc/herd/joining.md`, which nothing installs, so `systemctl help`
failed on a fresh install. It is now the document's URL: nothing in `deploy/`
copies files onto a host and that should not start, and a URL is true on
every machine, including one that never saw the repo. The local alternative
is one `install` command, written on the line above it.

---

## 2026-10-15 — D128 Two of my own rulings, corrected by building them

**D123 named the wrong knob.** Its last paragraph prescribed a Traefik
`serversTransport` carrying `forwardingTimeouts.readIdleTimeout`. That knob
drives **HTTP/2 ping frames** and has no effect whatever on an HTTP/1.1
WebSocket backend, which is what the hub is. It is deliberately absent from
`traefik-herd-hub.yml`, and the fragment says why where someone would
otherwise add it back. The knobs that matter are `idleConnTimeout`,
`dialTimeout` and `responseHeaderTimeout`; the two that would actually cut a
quiet link, `respondingTimeouts.readTimeout` and `idleTimeout`, are **static
and shared with every service on the host**, so they are the owner's and are
left alone — all three of their defaults are already far above the 15 s
Herd's own cadence has to beat.

**D124 was too narrow, in the direction that matters.** It said an unknown
`FromHub` variant is a breaking change *for a requester*, and left the runner
as a separate question. The runner is the same answer, for a stronger reason
than `herdctl`'s `bail!`: `FromHub` is `#[serde(tag = "type")]` with no
`#[serde(other)]`, so an unknown frame **does not decode at all** —

```
Err(Error("unknown variant `still_there`, expected one of `welcome`, …"))
```

— and the runner's decode error kills the link, which it then redials **into
the same undecodable frame**. It loops; it does not shrug. The `other =>` arm
that prints "not a runner's business" covers only variants that build already
knows, and is not forward compatibility.

So the rule at the frozen constant is the wider one: adding a variant to
`FromHub` **or** `ToHub` moves `PROTOCOL_VERSION`, whatever serde would
tolerate. Only a `#[serde(default)]` field whose absence means exactly what
its default says leaves the number alone.

**What both have in common,** and the reason they are in one entry: each was
a plausible ruling made from reading, and each was falsified by someone
making it real. A prescription I write into a decision is a hypothesis until
it is built — so an agent told to follow one must be free to come back and
say it is wrong, which is what happened twice here.

---

## 2026-10-15 — D129 The hub is deployed, and deploying it found two things

**The deployment.** Hub in docker compose on the server, `herd-hub:0.1.0`
built from the frozen tree, on a fresh volume, loopback only, with a
requester token. The runner native on the host as a systemd service under a
dedicated `herd` user — uid 986, group `herd` and nothing else, **not** in
`docker` — with `Delegate=yes`, its data root on the NVMe (D126).

**What is now proven on the real machine**, read back by a job itself out of
the kernel rather than asserted:

```
my cgroup: /system.slice/herd-runner.service/herd-job-507751-7-j0000000008
memory.max   8589934592      cpu.weight   10
io.weight    default 10      pids.max     1024
cpu.max      max 100000
```

`ulimit -v` inside a job reports `unlimited`, which is how we know the
ceiling is the cgroup and not the `RLIMIT_AS` fallback. Two assertions that
**could never be made** on an undelegated dev user are closed. A job at exit
0, one at exit 7, and a job silent for 50 s — past three keepalive cadences —
whose follower never went quiet and which returned its verdict normally
(D118, live). Every `ran on:` line carried real figures: 20 GiB lent, 6 GiB
floor, load recorded and not a gate.

**Finding 1: a job's `PATH` had no shell and no coreutils.** Herd builds a
job's environment from scratch, deliberately — that is what stops a stray
variable in the runner's shell redirecting a job's writes. What the job then
gets is systemd's service `PATH`, which on this host is zfs-user, dosfstools,
mtools, e2fsprogs, util-linux-minimal, openssh and systemd. So
`herdctl run -- sh -c '…'` failed with `sh: No such file or directory` and
exited **125**, and inside a job `cat` and `cut` were not found either. The
deployment must therefore *state* a job's `PATH`; the right value is
host-specific, so it is the unit's to give and not Herd's to guess. Fixed
with one `Environment=PATH=/run/current-system/sw/bin`, and a bare `sh -c`
works.

**Finding 2, and it is not ours to close.**
`/var/run/libvirt/libvirt-sock` on this host is **`srw-rw-rw-`** — mode 0666,
writable by every user on the machine, and libvirtd runs as root. The runner
said so itself at startup:

> WARNING: this runner's user can write /var/run/libvirt/libvirt-sock, which
> is a container or VM runtime and therefore root on this host: a job could
> start a privileged container and escape every cap. E-46's "unprivileged" is
> not true of this deployment.

That warning is exactly what `privilege.rs` exists for, and D78's reasoning —
that a uid is not the whole story — is vindicated by it catching a hole
nobody was looking for. The `herd` user has no privileged group; the socket
is simply open to all. **Herd did not introduce this and cannot close it:**
the fix is the host's libvirtd socket mode, which is `/etc/nixos`, and it
affects every service on the machine rather than Herd alone. Hiding the path
from the runner would need a mount namespace, and E-47 is the statement that
this runner must see the host as it is — so that cure would undo the
milestone.

Until it is fixed, the honest statement is the runner's own: **every per-job
cap on this host is sound against a job that stays inside Herd, and bypassable
by a job that chooses to talk to libvirtd.** It is recorded here rather than
left in a log line because it is the one thing on this deployment that a
reader must not discover by accident.

---

## 2026-10-15 — D130 On this host there is no imperative path for the unit

**Context.** `deploy/` is written on the principle that `/etc/nixos` is the
owner's, so everything needing it is an **OWNER** instruction and the module
added by D126 is "an offering, not an installation". The unit's own header
says to install it as `/etc/systemd/system/herd-runner.service`. That turns
out to be impossible here.

**Measured.** `/etc/systemd/system` on this server is **read-only** — NixOS
manages `/etc` by activation, so a unit file cannot be dropped into it:

```
tee: /etc/systemd/system/herd-runner.service: Read-only file system
```

So the two paths `deploy/README.md` offers are not alternatives on this
machine. The imperative one does not exist. `/run/systemd/system` is
writable but is cleared on every boot, which fails the one thing the unit is
for (E-52: back after a reboot, with no hand).

**What is actually running tonight.** A **transient** unit, built with
`systemd-run` carrying every directive the file carries — `Delegate=yes`,
`OOMPolicy=continue`, `MemoryMax=22G`, `MemorySwapMax=0`, `KillMode=mixed`,
`CPUWeight`/`IOWeight=50`, `LimitNOFILE`, `RequiresMountsFor`,
`Environment=PATH=…`, the token through `EnvironmentFile=` — and the same
`ExecStart` argument list, against a Nix store path held alive by a gcroot.
It is a faithful runner and it proves everything D129 records.

**It does not survive a reboot.** That is the honest state: **E-52 is
unmet on the server until the module is imported and `nixos-rebuild`
run.** No wording makes a transient unit persistent, and pretending
otherwise would be the worst kind of green.

**So the module is not an offering here, it is the only route**, and
`deploy/README.md` must stop presenting a choice it does not have. The step
needs `/etc/nixos` plus a rebuild on a server running other people's
services, which is Alexa's by the working agreement and is the one thing
still outstanding on M16.

**Worth noting for the next host.** A machine with a writable `/etc` has both
routes and the README is right as written for it; the wording has to name
which kind of host the reader is on, rather than assuming the imperative one
always works. That is the general lesson: "install this file there" is a
claim about the target, not about the file.

---

## 2026-10-15 — D131 A job's small declared outputs travel like its log

**Context.** Measured on the live deployment: Herd returns an exit code and
logs and **does not return files**. A job wrote `files/results.json` under
`{out}`, the file existed while it ran, and the workspace was reclaimed with
everything in it. The first consumer's whole result contract is a
`results.json` plus `{out}/files/`, so without this they can use Herd for
pass and fail and nothing else.

**The tension, and which way it resolves.** D10 takes Herd out of the byte
path **between runners**: a pack must not be relayed hub-wards to reach
another machine, and that is what M11's registry and direct transfer exist
for. It says nothing about a job's own **record** — and Herd already carries
the log runner → hub → requester as part of that record (D8, D18). So a
declared output that is small is in the same category as a log line, not in
the same category as a pack.

**Choice.** A job's `{out}` contents, up to a cap, are collected before the
workspace is reclaimed and written into the job's directory on the hub under
the same atomic discipline as everything else there (D8). Relative paths are
preserved — `files/results.json` comes back as `files/results.json` — and a
requester that attaches long after the job ran gets them, because `herdctl
wait` is exactly that requester.

Collected on **every** ending that produced a workspace: success, a non-zero
exit, a cap, a cancel, a death. A job that fails is often the one whose
output is most wanted, and D108 already fixed a workspace leaking on failure
for the same class of reason.

**Over the cap is a refusal carrying figures, never a truncation.** A
truncated `results.json` that still parses is the worst outcome available: it
reads like the truth. The refusal names the cap and the real size and points
at M11, in the shape E-04's refusals already use.

**Rejected.** Waiting for M11 and telling the consumer to write outside the
data root in the meantime. It works — Herd does not stop a job naming an
absolute path — but it makes every consumer invent its own drop directory,
and the thing they would be working around is a gap of one afternoon.

**What this is not.** It is not artefact distribution. Large outputs, packs
and anything another runner needs stay with M11, direct and off the hub's
path. The cap is what keeps the two apart, and it is a flag so that the line
is the operator's to draw.
## 2026-10-15 — D132 A release is three static musl binaries and a notice

**Why at all.** The first consumer's team deploys on their own machines —
an Arch box, a Windows PC's WSL2 — so a release has to be something a
person downloads and runs. What the flake builds by default is not:

```
ld-linux-x86-64.so.2 => /nix/store/…-glibc-2.42-67/lib64/…
libc.so.6            => /nix/store/…-glibc-2.44-25/lib/…
```

Those binaries exec only on a host with a Nix store, which makes a
release that contains them useless to whoever is joining.

**The route: `pkgsStatic` through the existing flake**, as
`packages.<system>.static`, and not a second toolchain in the workflow.
The reason is that it cost nothing to keep D12: nixpkgs' musl cross set
has `rustc`, `cargo`, `gcc` and `binutils` in the public cache — 141 MiB
of download, measured — so **one** derivation is built and the workspace
compiles in 50 s. A `rustup` step in the workflow would have been a
second pinning of the toolchain for no gain in speed. `nix eval --raw
.#static.version` is then where the artefact names get their version,
which is `Cargo.toml`'s `workspace.package.version` by the same route as
the image's tag (`nix/hub-image.nix`), so a release cannot be named for a
version its binaries do not answer to.

**What the workflow asserts, because a green release of unusable
binaries is worse than no release.** `file` must say static, `ldd` must
find no loader, `readelf -l` must show no `PT_INTERP`, and the bytes must
name no `/nix/store` path. Then `scripts/release-smoke.sh` runs a hub, a
runner and two real jobs out of the release binaries inside an
`archlinux` container and an `alpine` one — glibc and musl, neither with
any Nix — so the claim "runs on any distribution" is a test and not a
hope. `--help` three times would only have proved the loader; a job that
execs a shell proves musl's own resolver, its sockets and its `/etc`
reading as well.

**x86_64 only, deliberately.** Every machine in the herd today is
x86_64. The cost of adding aarch64 is known and is not a cross-compile:
`pkgsCross.aarch64-multiplatform-musl` has to **build `rustc` from
source** — the cache has no `aarch64-unknown-linux-musl-rustc`, so it is
LLVM and a full bootstrap, 1.7 GiB and far past a 30-minute job — while
a native `ubuntu-24.04-arm` runner needs only a matrix entry and an
arm64 container image, because `flake.nix` already lists
`aarch64-linux`. So when the first non-x86_64 machine appears, the answer
is a second runner in the matrix, not a cross build.

**And a notice, treated as half of the release:**
`deploy/release-notice.md`, quoted into the release body. Its subject is
the thing a binary cannot say about itself — that run bare the runner
works, and that without a unit carrying `Delegate=yes` there is no
per-job `memory.max`, no `cpu.weight`, `io.weight` or `pids.max`, no
cgroup kill, no restart after a reboot and a token on a command line that
every user of the host can read out of `/proc`. One row of its table
turned out the other way round from the way it was briefed, and is
written as measured: a job's `PATH` is **fine** bare, because the runner
carries its own `PATH` into a job, and is the unit's one real trap —
which is D129's finding, now in the document the deployer reads.

---

## 2026-10-15 — D133 D129's `PATH` finding was half wrong

**Correction.** D129 records that a job's `PATH` had no shell and no
coreutils, which is true of the **deployment** and was a real exit 125 on a
real job. What it implies and should not is that Herd gives a job nothing.

`herd-runner/src/confine.rs` carries `PATH`, `LANG`, `LC_ALL`, `TZ` and
`TERM` from the **runner's own environment** into the job. So a runner
started from a person's shell hands its jobs a usable `PATH`, and
`herdctl run -- sh -c '…'` works with no configuration at all. What broke
was the unit: systemd's default service `PATH` on this host holds no shell,
and that is what the job inherited.

**So the unit is, on this one point, a risk rather than a gain** — the only
line in the bare-binary-versus-service comparison that runs that way, and
`deploy/release-notice.md` says so rather than selling the unit as strictly
better. Everything else in that comparison still favours the unit, and the
reason is unchanged: without `Delegate=yes` there is no per-job `memory.max`,
no `cpu.weight`, no `io.weight`, no `pids.max`, no restart after a reboot,
and the token sits on a command line that `/proc` shows to every user.

**Why this matters beyond the one variable.** The thing that makes a
deployment's environment wrong is not that it is empty but that it is
**different from the one the author tested in**. A `PATH` inherited from a
developer's shell is why nobody noticed; `Environment=PATH=…` is the fix
because it states the value instead of inheriting whatever is there.

Found by an agent that was told the opposite in its brief, measured it, and
wrote down what it measured. That is the behaviour to keep.
