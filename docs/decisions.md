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

---

## 2026-10-15 — D134 The reaper records what it reaped; it runs no one's code

**Context.** The first consumer reports a real leak. On a runner's death —
`SIGKILL`, OOM, reboot — the startup reaper removes the job workspace, but
their `up` step made state **outside** it: a workspace layer and files on a
RAM tmpfs. Removing the workspace does not undo that, so each death leaks.
They asked for either of two things: run the declared `down` for every reaped
job, or at least publish the reaped job ids where a janitor can read them.

**Choice: the second, now; the first never from the reaper.**

Running the consumer's `down` out of the reaper would put arbitrary consumer
code on a runner's **startup path**, and that path has promises: a runner
refuses to start on a root it cannot use, and stops cleanly in ≤ 5 s
(invariant 2). A `down` that hangs would hold a runner's startup open with no
one watching, and a `down` that needs the workspace would need it **after**
the reaper's whole reason for existing is to remove it. It also needs the
declaration, which lives on the **hub's** record, not on the runner's disk —
so the reaper would have to dial, authenticate and read before it has even
joined. Each of those is a reason; together they are a different component.

So the reaper **writes down what it reaped**, durably, outside `jobs/` where
its own sweep cannot clear it — the job id, what was removed, what was killed,
and when — and says so on stderr as it already does. A janitor reads that
file and runs whatever the consumer needs. Herd names the mess; the consumer
owns the cleanup. That is the same division D10 draws for bytes.

**The orchestrator-shaped answer, for later and named here so it is not
reinvented.** The right home for a `down` after a death is the **hub**
dispatching it as a **job** — it has the declaration, it knows the attempt
died, a job carries its own caps and timeout, and a cleanup that fails is then
a visible verdict rather than a silent hang on somebody's boot. That is a
feature with a queue, a priority and a test, not a line in the reaper. It
waits for a milestone of its own.

**What the consumer must know meanwhile**, and it is the honest part: between
a runner's death and a janitor's next pass, their out-of-workspace state is
**still there**. Herd is not promising to have cleaned it, only to have told
them precisely what to clean. Anything that must not outlive a job belongs
inside the workspace, which Herd does reclaim, whatever the job did to it.
## 2026-10-15 — D135 A hard IO ceiling, and pressure that gates nothing

**Context.** The first consumer's build server froze three times on disk
saturation. Herd bounded a job's memory by the kernel and its IO by a
**weight** — and a weight shares contended bandwidth, it never caps it, so on
a host where one job can saturate a disk Herd had no answer. D116 named
`io.max` as "the honest next step if IO must actually be bounded"; it must.
Separately, the consumer asked for PSI to brake admission, which E-32 forbids
(D119).

**`io.max`, optional and absent by default.** `--job-io-max
rbps=…,wbps=…,riops=…,wiops=…`, any subset of the four keys, written to the
job's cgroup `io.max` for **the device the data root is on**. Absent means
**no line written** — never a zero, never a guess: there is no honest default
throughput, because a figure measured on an NVMe is absurd on a spinning disk.
Zero in any key is refused where it is written, like a memory ceiling of zero:
it is a wall and not a cap.

**It refuses rather than misleads, and that is the deliberate difference from
`io.weight`.** A weight nobody honours costs nothing, so it is written and
reported inert (D120). A *ceiling* nobody honours is a false promise, so a
ceiling asked for on a host that cannot carry one is a **startup refusal
naming the path and the reason** — a ZFS data root (no cgroup IO knob reaches
a ZFS dataset at any value), no cgroup delegation, no blk-throttle in the
kernel, a path whose device cannot be resolved. `cpu.max` already took this
road; `io.max` goes one step further because the knob is **per-device**: a
value with no device is not a weaker ceiling, it is not a ceiling, so the
device is resolved first and an unresolvable path is itself the refusal.

**Proved by writing it, not inferred.** Three different things could make a
ceiling vanish — the `io` controller not handed down, a kernel without
`CONFIG_BLK_DEV_THROTTLING` so `io.max` does not exist, or a device the kernel
refuses — and none is visible from a file somebody could read. So startup
writes the real value into a probe cgroup under the same base every job hangs
under and reads it back, exactly as delegation is probed. Measured on this
host, inside a delegated scope, by a job reading its own cgroup: `io.max` on
`259:0` carries the four limits it was given.

**Not fixed by this.** A job writing an absolute path **outside** its data
root writes to another device, which neither the ceiling nor the weight
reaches (D123, D126). That is said in the startup report rather than left to
be discovered.

**PSI is recorded and gates nothing.** `/proc/pressure/{cpu,memory,io}` and
the job's own cgroup `*.pressure`, as `some avg10` in thousandths of a
percent, on one new `#[serde(default)]` field of `JobMetrics` — which is the
one change allowed to leave the frozen `PROTOCOL_VERSION` and `RECORD_VERSION`
alone (D73), and the reason this is a field and not a frame. An unreadable or
absent file leaves the figure **absent, never zero**: a zero reads as "no
pressure", which is a lie told to whoever is looking into a freeze. Nothing
compares it to anything, and there is a test that says so over the sources of
every module that decides about a job — because E-32 is a promise about Herd
and not about one Tuesday's run.

**Whether a ceiling is the right answer to a freeze.** It is the right
*bound* and it is not a complete answer. A ceiling caps throughput and
nothing caps the size of what a job writes to a device other than its data
root, so a disk can still fill; a 20 GiB per-job workspace budget is what
bounds that, and only inside the root. The cheap figure a throttled host
still owes its operator is how long a job spent stalled, which is now
recorded.

**Why the weight is not enough, measured on this host and not argued.**
`/sys/fs/cgroup/io.cost.qos` reads `259:0 enable=1 ctrl=auto rpct=0.00
rlat=5000 wpct=0.00 wlat=5000`. `rpct` and `wpct` are the latency percentiles
blk-iocost watches to decide a device is saturated, and at **zero** it watches
none: the controller apportions the device by weight and never lowers the
total load to protect latency. So every weight on this machine can be honoured
exactly while `/proc/pressure/io` reads `full avg60=43.7` — which is the
freeze, with the knob working as designed. A weight decides *who* gets a
saturated device; nothing in the weight path decides *whether* it gets
saturated. The runner now says that in its startup report where it finds
`rpct=0 wpct=0`, rather than printing "io.weight is enforced" and leaving the
reader to assume more than that means.

**What is still not Herd's to fix.** The systemic answer on a shared host is
host hygiene: every workload in a weighted cgroup, and `io.cost.qos` given a
real latency percentile so iocost throttles instead of only apportioning.
That would protect everything on the machine, Herd included, and it is
`/etc/nixos` — the owner's, like D129's libvirtd socket. Herd's own part is
the bound it can place on itself without knowing who else exists: a weight is
a promise about a ratio, and a ratio with an unknown denominator is not a
bound, so invariant 2 cannot be kept with a weight alone.

**What sharing one queue changes about the figure.** On this server a job's
data root, the consumer's docker data root, their work and this checkout are
all on `/dev/nvme0n1p1` — one request queue. A per-job ceiling there bounds a
job against **their services** and not merely against other Herd jobs, which
is the useful direction and also the reason there is no default: a ceiling
that is right where Herd is the only tenant is wrong where it shares a queue
with production. So the help text says how to derive one instead of offering
one — a fraction of the kernel's own measurement of the device in
`/sys/fs/cgroup/io.cost.model` (`rbps=3102524156`, `wbps=1742780862` here),
with the IOPS keys included, because a freeze is a latency event and small
random IO is what causes one.

---

## 2026-10-15 — D136 Why this host freezes, and the one line that would fix it

**This is the sharpest measurement of the night and it is not about Herd.**
`/sys/fs/cgroup/io.cost.qos` on this server reads:

```
259:0 enable=1 ctrl=auto rpct=0.00 rlat=5000 wpct=0.00 wlat=5000 …
```

`rpct` and `wpct` are the **latency percentiles blk-iocost watches** to decide
a device has fallen behind. At **zero** it watches none: the controller runs
purely off its cost model, **apportions** the disk by weight, and never
**reduces** the total load offered to it.

**So every weight on this machine can be honoured exactly while
`/proc/pressure/io` reads `full avg60=43`.** `full` means every non-idle task
stalled — including the consumer's own weight-20 slice, which is why their
`IOWeight` mitigation did not stop the freezes. Putting everything in a
weighted cgroup would make the stalls **fair, not absent**, and a frozen
service does not need fairness.

**The single largest win available is one line of `/etc/nixos`, and it is not
ours.** Give `io.cost.qos` a real `rpct`/`wpct` and iocost becomes a
**latency-targeting throttle for every cgroup at once** — it adapts to the
device instead of guessing a number, and it protects the consumer's services
from everything on the host, Herd included. That is Alexa's to apply, like
D129's libvirtd socket, and it is worth more to them than anything Herd can
ship.

**Herd still needs its own ceiling, for a reason independent of the host.** A
weight is a promise about a **ratio**, and a ratio with an unknown denominator
is not a bound. Invariant 2 — a runner never destabilises its host — cannot be
kept with a knob whose effect depends on who else happens to be in a cgroup.
The ceiling is the only IO bound Herd can state about *itself* while knowing
nothing about the rest of the machine, which is also exactly why it is
optional and off by default.

Proof the two knobs are not redundant, measured on one host in one run: on
`/mnt/hdd` (`sdb`) blk-iocost knows no cost model, so `io.weight` there is
genuinely inert — and the `io.max` ceiling was written and held anyway.

**Its honest limits.** It bounds **rate**, not volume, and only on the device
the data root is on. Disk saturation it addresses; a disk filling up it does
not — that is the workspace budget, and only inside the data root. The
absolute-path gap of D123 is untouched. All of that is now in the runner's
startup report rather than left to be discovered.
## 2026-10-15 — D137 What the output cap had to be, and why

**Context.** Herd returns an exit code and logs. It does not return files.
A job's `{out}` directory is handed to its steps and the whole workspace is
reclaimed when the job ends — success, failure or death. Measured on the
deployed server: `…/jobs/j0000000012/work/out/files/results.json` existed
during the run and the whole of `jobs/` was empty after it. The first
consumer's entire result contract is a `results.json` plus `{out}/files/`,
so without this they can use Herd for nothing but pass/fail.

**Choice. A job's declared outputs, up to a cap, are part of its record,
exactly like its log.** They are collected on the runner before the
workspace is reclaimed, written into the job's `files/` directory on the hub
under D8's discipline (temp, fsync, rename, fsync the directory), and handed
to a requester on the frame that already carries the verdict.

**This is not the byte path D10 removed.** D10 takes Herd out of the byte
path *between runners*: a pack must not be relayed hub-wards so another
machine can have it. It says nothing about a job's own **record**, and Herd
already carries the log runner → hub → requester as part of that record
(D8, D18, E-15). What makes the two different is size and purpose, which is
why the cap below is the whole of the boundary and not a detail of it.

**The mechanism, and why no new frame.** `PROTOCOL_VERSION` is frozen at 1
and a hub is deployed. D128's rule is that a new variant on either enum
moves the number, and only a `#[serde(default)]` field whose absence means
what its default says leaves it alone. So this adds **one defaulted field**
to `ToHub::JobFinished` and one to `FromHub::JobFinished`, and no variant:
absence means "this peer collected nothing", which is exactly true of every
build that predates it. **`PROTOCOL_VERSION` stays 1 and no deployed binary
has to be rebuilt to keep working.**

`RECORD_VERSION` stays 1 for the matching reason. A job directory gains no
*name*: `files/` has been created, empty, in every job directory since M2 —
D8 called it "kept files beside it" and E-15 asks for exactly that. `job.json`
gains one `#[serde(default, skip_serializing_if)]` summary, so a job that
keeps nothing writes a record byte-identical to the one it wrote before, and
every record already on the live state root reads unchanged.

**Keyed by the job id, with relative paths preserved.** The consumer asked
whether this is a directory fetched by job id or keyed by the declaration's
`leaves` names. It is the id: a requester attaching with `herdctl wait <id>`
has the id and nothing else — it never saw the declaration — so the id is
the only key that always works; `leaves` is a declaration concern and keying
the transport on it would make the record's shape depend on a file the hub
does not own (D74); and `leaves`-keyed, content-addressed retrieval is
precisely M11, which this must not blur into. `files/results.json` comes
back as `files/results.json`, never flattened.

**How a requester gets them: the verdict frame, not a subcommand.** The
deciding case is a requester that was never connected while the job ran, and
`herdctl wait <id>` is already that case — so `herdctl wait <id>
--outputs-to <dir>` *is* the after-the-fact fetch, with no new command, no
new frame and no second round trip. The hub reads them off the record for
both the live follower and the late attach, so there is one answer to "what
did this job leave" and it is the disk.

**E-02 is untouched.** Nothing is written to stdout. `--outputs-to <dir>`
writes the files there and reports them on **stderr**, one line each; with
no flag nothing is written. An output file in E-02's stream would be read by
the consumer's own tooling as their tool's output.

**Collected on every ending that produced a workspace**, with no success
condition: a success, a non-zero exit, a cap, a cancel. D108 fixed a
workspace leaking on failure for this class of reason and it runs the same
way here — the job that fails is the one whose results you most want, and
the consumer's current workaround exists for exactly that case. An abandoned
attempt collects nothing: it says nothing about the job, there is no frame
for its files to ride, and the next attempt makes its own `{out}`.

**What is collected.** Every regular file under `{out}`, relative paths
kept. A **symlink is never followed, wherever it points** — not "unless it
leaves the workspace": telling a safe link from one aimed at `/etc/shadow`
means resolving it, resolving it races a job still being killed, and the
only safe rule has no exception. An empty directory is not represented. A
fifo, a socket or a device is skipped **by its type before anything is
opened**, because opening a fifo nobody writes blocks for ever and would
hang the job's own report. A name that is not UTF-8 is skipped. Each of
these earns a sentence on the record rather than silence, because
"`results.json` did not come back" and "it is a symlink" are different facts.
A job killed mid-write leaves a half-written file and it is collected as it
stands: Herd cannot know what the writer meant, the partial file is the
evidence in the case someone is debugging, and the verdict already says the
job was capped or cancelled. A consumer wanting all-or-nothing renames into
place.

**Over the cap is a refusal with figures, never a truncation.** No file at
all comes back and the sentence names the cap, the measured size and the
file count, and says that large artefacts are M11's direct transfer. A
truncated `results.json` that still parses is the worst possible outcome: it
reads like the truth. The verdict is **not** touched by it — the tests
really did pass or fail, and turning a cap on a side channel into a verdict
is the mistake D20 exists to prevent — so it is a loud sentence and never an
exit code.

**The cap is 32 MiB, derived and measured, and it is also the ceiling.**
The first reasoning here was wrong twice over: 64 MiB was taken from the log
cap, which is the wrong neighbour — a log is what a person reads, a declared
output is what a program reads — and then 8 MiB was picked from "a results
file is kilobytes", which is short of what the consumer actually produces.
Their real figures are a results file of a few kB plus per-scenario logs and
screenshots reaching a few hundred MB. So the figure is derived from what
the mechanism can carry rather than from an analogy:

- a collection rides one JSON frame, base64-encoded, against a 64 MiB frame
  ceiling, which puts the arithmetic bound at 48 MiB; measured, 32 MiB of
  payload in 200 files encodes to a 42 MiB frame, leaving 22 MiB for
  everything else on the frame;
- measured, 32 MiB costs about 140 MiB of peak memory and a few hundred ms
  to encode and to decode, on each of the three parties;
- both curves are linear, so a few hundred MB would cost over a gigabyte per
  party and **tens of seconds of blocked event loop** — past the 15 s
  cadence a health report and a quiet link both live by, which would make a
  large output indistinguishable from a lost runner.

**So a few hundred megabytes is over the cap on purpose, and that is the
boundary this entry exists to hold.** At 32 MiB a job's output travels like
its log. Past it, the same mechanism is artefact distribution wearing the
record's clothes, and it is M11's direct runner-to-runner transfer — with
the hub's volume to size and a retention policy to write, since Q9 says the
hub holds kilobytes. The consumer's screenshot-heavy runs land on the far
side of that line and are refused by name with figures.

Because 32 MiB is a ceiling and not a taste, **the default is the ceiling
and the flag only lowers it**, and a `--job-output-cap` above it is refused
at startup with both figures — a cap past the frame would otherwise produce
a verdict frame the hub cannot decode, which is D128's redial loop and not a
degraded result. `0` returns a verdict and nothing else. A test builds a
collection at the cap and puts it through the real encoder, so the figure is
checked rather than reasoned about.

**Two things found while building it.** The clients dialled with the
WebSocket library's defaults, whose `max_frame_size` is **16 MiB** — smaller
than what the hub is configured to send, so a legitimate verdict frame over
16 MiB would have reached `herdctl` as a closed connection and a 125, a job
with no verdict for a job that passed (D117's shape). Both clients now state
Herd's own ceiling, which is one constant in `herd-proto` that the hub takes
too. And D19's promise needed widening: the ends the hub holds in memory
when it cannot write them now carry the job's **files**, because the
workspace is already reclaimed by then and a retry that wrote the verdict
and forgot the files would destroy half a record D19 says is never
destroyed.

**Alternative rejected.** A `herdctl outputs <id>` subcommand fetching the
files afterwards. Rejected because `herdctl wait` already attaches to a job
long after it ran, so the fetch exists either way and a second command would
be a second thing to keep working — and because a frame that arrives with
the verdict cannot be forgotten by a caller that got its exit code and left.

---

---

## 2026-10-15 — D138 My cap instruction was wrong, and the 16 MiB bug

**I told the agent the wrong number and it refused me with measurements.**
D131 reasoned the cap from the log cap; then the consumer said they produce
"at most a few hundred MB", so I told the agent to size for that figure and
said 64 MiB would refuse the normal case. Both of us were reasoning about
*their* files. Neither of us was reasoning about **the frame**.

Measured: 32 MiB of payload in 200 files becomes a **42 MiB frame** and about
140 MiB of peak memory per party, base64 on one JSON frame against a 64 MiB
ceiling. Both curves are linear, so a few hundred megabytes is a **>400 MiB
frame** — rejected outright — **>1 GiB per party**, and **tens of seconds of
blocked event loop**. That last one is the real objection: it is past the
15 s cadence that a health report and a quiet follower's link both live by,
so a large output would be **indistinguishable from a lost runner**. The
feature would have manufactured D117's defect.

So the cap is **32 MiB**, derived from what can actually arrive rather than
from what anybody wants, and a `--job-output-cap` above it is refused at
startup with both figures rather than becoming an undecodable frame and
D128's redial loop.

**And that number is the boundary D131 was trying to name.** At 32 MiB a
job's output travels like its log. Past it, the same mechanism is artefact
distribution wearing the record's clothes — and the cost is not only CPU: Q9
says the hub volume holds job records and manifests, kilobytes and no
payloads, and there is **no retention or GC on job records** before M12. At a
few hundred megabytes a job, hundreds of jobs a day fills the hub's disk in
days with nothing to collect it.

**So the consumer's screenshots do not fit, and I have to tell them so after
saying they would.** `results.json` at a few kB comes home today. Their
per-scenario logs and screenshots are refused by name with figures. Carrying
those needs three things together — chunked output frames, which moves
`PROTOCOL_VERSION` to 2; hub-side streaming to disk instead of a frame held
whole; and a retention policy, which is M12. That is a milestone, and the
agent was right not to build it blind.

**A real bug found on the way, and it was already there.** Both clients
dialled with the WebSocket library's default `max_frame_size` of **16 MiB** —
*smaller than the 64 MiB the hub is configured to send*. Any legitimate frame
over 16 MiB would have reached `herdctl` as a closed connection and exit
**125**: no verdict for a job that passed, D117's exact shape, on a path
nobody had exercised because nothing yet sent a frame that large. One
constant in `herd-proto` now states Herd's own ceiling and all three take it.

**One thing the agent decided that I am keeping.** An over-cap refusal does
**not** change the verdict. The tests really did pass or fail, and D20 forbids
turning a cap on a side channel into a verdict about the job — so it is a
loud sentence on stderr and on the record, never an exit code. A consumer who
wants a missing `results.json` to be fatal makes that check themselves.
## 2026-10-16 — D139 Two defects the first consumer found, and one order

Both reported by the first consumer, who has finished their side. Neither
needed the wire: `PROTOCOL_VERSION` and `RECORD_VERSION` stay at 1 and no
`FromHub` or `ToHub` variant moved (D124, D128).

**A declared command's own line is a template, and its tokens are filled
before `{args}` is in it.** `tool pack --out {out}/packs` reached their hook
with the three characters `{out}`: the runner filled `{job}`, `{in}` and
`{out}` in the four hooks, then pasted the line into `{line}` last, and the
line itself was never looked at. The fix is not to fill the line wholesale.
D107's property is that **a value a substitution pasted in is never itself
scanned for tokens** — `--only '{out}'` is an argument, not a template — and
the requester's arguments were quoted into that line before the job was
minted, which is itself D107's and which M6 keys on. Filling the line
wholesale would turn a requester's own string into a template, which is the
one way a declaration's trust could reach past the declaration.

So the order is: fill the declared text, then the arguments. The three
figures that make the order the only possible one are worth writing down,
because they look independent and are not:

* `{args}` is filled **before the job is minted**, for M6 (D107);
* `{in}` and `{out}` are knowable only **after** it is placed, on the runner;
* D107 forbids a pass over a value a previous pass pasted in.

Taken together, a single pass over the whole line cannot satisfy all three.
What makes it possible with no new frame and no escaping is that the
requester's words already travel beside the line — `Declaration::args`, which
D74's cache key needed — and are quoted into it by one function. So the
runner holds the argument spans out of the pass and fills the declared text
around them. Nothing is encoded, escaped or marked; the failure direction is
conservative, a declared token left literal, never a requester's text turned
into a path.

**Known gap, not closed here:** `{j}` is filled by the hub over the whole
command, *after* the arguments are in it (`with_parallelism`), so an argument
that says `{j}` already becomes a thread count. Same shape, different place,
and the hub holds `declaration.args` on the record, so it is fixable the same
way. Left alone because it is a hub change on a deployed hub and nobody has
hit it.

**"Nothing was submitted" and "submitted, no verdict" are told apart by an
id, not by a code.** Both exit 125, and they ask a falling-back requester for
opposite actions: re-run the second and the work is done twice, decline the
first and it is never done. For a build and test system work run twice is a
wrong answer, not a slow one.

**No new exit code, and the reason is that the space is genuinely full.**
0‑255 belongs to the job, 124 is the caps, 128+n is a signal, and 126/127 are
what a **shell** returns for "not executable" and "not found" — so a wrapper
invoking `herdctl` through `sh` could see those from the shell itself and
misread them. Anything below 124 collides with a real verdict, and the
collision matters precisely because the wrapper cannot tell the two apart.

The discriminator is a line on **stderr**, which a job's own bytes never
share, so it cannot collide with a job's output and does not disturb E-02's
stdout order:

* `herdctl: job-id: <id>` — a job exists. Printed **the moment the hub says
  the id**, not at the end: a line printed only on the failures that were
  thought of cannot cover the one that was not, and it has to survive a
  wrapper that gives up and kills `herdctl`.
* `herdctl: no-job: …` — nothing was submitted; running the work again is
  safe.
* `herdctl: job-unknown: …` — the submission left and was never
  acknowledged. **A third state, and it is real**: a job may exist whose id
  nobody learned. Said rather than hidden, because a line claiming no job
  exists would be the same defect wearing the other mask. E-05 is what makes
  it actionable — the same submission joins the open job rather than starting
  a second one.

It is strictly better than a code would have been: an id lets them recover
with `herdctl wait <id>` instead of merely declining to duplicate. The rule a
wrapper writes is "did I get an id?", never the parsing of prose. A command
that submits nothing — `wait`, `cancel`, `raise`, a dry run, the operator
commands — prints none of the three: a sentence about submissions there would
answer a question nobody asked.

---

## 2026-10-16 — D140 A refusal and a wait for the same fleet state

**Found while chasing a flaky test, and it is the more important half of
that work.** The hub holds two different answers to one question. For a job
already queued, `matcher.rs` says `Wait { why: "no runner is connected to
this hub" }`. For a **new** submission in exactly that state, `server.rs`
refuses outright. Which one a job gets depends only on whether it arrived a
moment before or after the last link dropped — so that wait sentence is
**unreachable for a newly submitted job**.

**It contradicts D88.** D88's own "what we did not do" says a job no
connected runner could ever fit **waits**, because "a fleet is not a closed
world — the owner's PC is asleep at night and its 17 GiB come back in the
morning". A *transient* gap is the same situation with **weaker** evidence,
and it is treated more harshly. The second refusal branch — every runner
draining or switched off — is worse still, because E-33's switch is
explicitly a temporary owner action.

**It is concrete now that Herd is deployed.** Every hook that submits during
a hub restart, a runner restart or a brief disconnection gets **exit 125**.
With the deployment live and the consumer about to submit from their own
tooling, that is not a hypothetical.

**The principled shape, for which the hub already holds the facts:** refuse
when **no runner has ever been approved** on this state root — a
configuration mistake, which must not wait for ever — and **queue**, with the
sentence that already exists, when the fleet is merely away. The distinction
is between "this can never work" and "nobody is here right now", which is
exactly the distinction D88 draws.

**Not changed.** It alters what a deployed hub does with a submission, which
is a behaviour the first consumer is about to build against, and it is the
kind of change Alexa decides. Written here so the next person meets the
question rather than the symptom.

**A second instance of the same class, in the same area.** `{j}` is filled by
`with_parallelism` over the **whole** command after the requester's arguments
are already in it, so an argument containing the literal `{j}` becomes a
thread count today. That is D107's property broken in the hub, the same way
it was broken in the runner for `{in}` and `{out}` — and the hub holds
`declaration.args` on the record, so the same fix applies. Left alone for the
same reason: it changes a deployed hub, and nobody has hit it.

---

## 2026-10-16 — D141 Q10 settled: Herd is Apache-2.0

**Context.** The licence was the last open question from the first session and
the only one deliberately left to Alexa, because relicensing is expensive to
undo: it needs every contributor's consent. He chose **Apache-2.0** from the
three I put to him.

**Why it is the right one here.** It grants patent rights explicitly, which
MIT does not, and it is the de facto norm for Rust infrastructure tooling — so
nobody integrating Herd has to stop and think about it. MPL-2.0 was the
alternative if every reuse had to stay open; the GPL was argued against and
not chosen, because Herd is a service somebody deploys rather than a library
somebody links, so strong copyleft would not buy what it costs in friction.

**How it is recorded, and the care that mattered.** `LICENSE` at the root is
the **verbatim** canonical text from `apache.org`, 11 358 bytes and 202 lines,
checked byte-identical. Three copies already on this machine — two from
crates, one shipped with cargo — all differed from each other, which is
exactly why none of them was used and why the text was not written from
memory. `license = "Apache-2.0"` is on `[workspace.package]`, which is what
tooling reads.

**One thing deliberately not invented.** Apache-2.0's `LICENSE` carries no
copyright line — the appendix in it is a template, not a statement — so no
holder name was needed and none was guessed. If Herd should assert attribution
it goes in a `NOTICE` file or in file headers, with a name only Alexa can
give. That is a one-line addition whenever he wants it, and its absence
changes nothing about the grant.

---

## 2026-10-16 — D142 D140 carried out: the roster refuses, the fleet waits

**What changed.** A submission is refused in **one** fleet state now: no
runner is approved on this state root. Everything else about a fleet that is
away — nobody connected this second, every runner draining, every runner
switched off by its owner — **queues** the job. Alexa approved D140; this is
the ruling built, and no frame moved: `PROTOCOL_VERSION` and `RECORD_VERSION`
stay at 1 (D124, D128).

**The sentence is the one that already existed, and that is the point.** A
queued job's words come from `matcher::place` and are wrapped in M8's
explained wait by the one path every queued job goes through — the position,
what is ahead, and why (E-04, E-36). So a job queued a moment after the last
link dropped reads exactly like one queued a minute before it, which is what
the two answers to one fleet state had made impossible.

**The second check after the pass is gone.** The submission path used to
re-ask "is there a runner?" *after* its dispatch pass and refuse if not. That
was the half of the defect that is hardest to see: the pass had already
written the job's wait on its record, and the refusal then threw the
follower away and answered 125 about a job that exists.

**What bounds such a job, asserted rather than assumed.** E-09's deadline is
D88's only bound on a job nothing can run, and it is judged in
`matcher::place` **before** anything about the fleet — so a pass fired by the
queue's own ageing loop cancels a job waiting for an empty fleet with no
runner connected at all. A cancel reaches it where it stands, because
cancelling a queued job is a store write and asks no machine. Both are in
`herd-hub/tests/away_fleet.rs`, with the real binaries, beside E-04's
restatement reaching a `herdctl wait` on such a job.

**Not touched: the unplaceable-forever case M7 and D88 settled.** A job
needing a capability no connected runner declares still **waits**, with `no
connected runner can run it: …`, and a job whose own figures contradict each
other still ends terminally (`Placement::Impossible`). Neither was in D140's
scope and neither moved.

**What an existing requester notices.** A submission that used to exit 125
during a hub restart, a runner restart or a brief disconnection now returns an
id and the job runs when the fleet comes back. A wrapper that read 125 as
"decline, do not duplicate" gets a verdict instead — strictly better, and
E-05 already made retrying safe. The only refusal left is the one a person has
to fix, and it quotes the hub's own startup line: `0 runner(s) are approved on
this state root`. A requester that wants a bound on waiting passes
`--deadline`, which is where D88 says a bound on waiting belongs.

**And `{j}`, the second instance of D107's defect.** `with_parallelism` filled
`{j}` over the whole command *after* `herdctl` had quoted the requester's
arguments into it, so an argument saying `{j}` became a thread count — and
`parallelism_tokens` counted it, which was worse: two of them end the job by
name. Fixed the way D139 fixed `{in}` and `{out}` in the runner, and with the
*same function*: `herd_proto::declared_spans` holds the argument spans out of
the pass and the declared text around them is filled. `herd-runner`'s own
`line_filled` now calls it too, so the two paths cannot drift. The property
preserved is D107's: **a value a substitution pasted in is never itself
scanned for tokens** — the failure direction is a declared token left
literal, never a requester's text turned into a figure.

**Found on the way, and fixed here because here is where it became
provable.** `herdctl` printed `job-unknown` for a submission the hub
**refused in a frame** — "a job may exist whose id you never learned" —
where `no-job` is the true answer: a refusal *is* the hub saying it minted
nothing. D139's two lines ask a falling-back requester for opposite actions,
so this was not a wording slip; it sent a wrapper looking for a job that does
not exist, in the one case where re-running the work is safe.

It predates this change, and the reason it belongs to it anyway is the first
commit above. A `Refused` frame is proof the hub minted nothing **only if no
refusal can follow a minting** — and until the post-pass refusal was deleted,
one could: that branch answered 125 about a job that already had its wait on
its record. Every `FromHub::Refused` on the `SubmitJob` path now precedes the
minting, so the proof holds, and this correction is the last step of the same
change rather than a tidy-up somewhere else.

**It is not a two-line fix, because the ratchet it steps on is deliberate.**
`learn()` only ever moves forward, and rightly: once a submission has left,
no later failure can make it not have left — not a dropped socket, not a
killed process, not a frame nobody can read. So the step back is **authorised
by proof rather than expressed as an ordering**: a new `Said::Refused`,
emitted from both submission paths, and a `refused()` in the binary that does
*not* go through `learn()` and moves `Unknown` → `Nothing` and nothing else.
`AJob` stays `AJob` — an id has been printed and no frame can unprint it —
and `NotASubmission` stays itself.

And the emitting side guards it too: `run_job` says it **only while no id has
been learned**, so a `Refused` arriving after a `JobAccepted`, for any reason
a future hub might have, can never make `herdctl` claim no job exists about a
job that does. That would be the same defect wearing the other mask, and it
is the more dangerous mask: work declined is work never done. In
`submit_job_saying` the guard is kept by the shape of the match — the refusal
is the first frame after the submission and `JobAccepted` returns above it.

**What is asserted, and what is not.** A submission against a genuinely empty
roster exits 125 with the `no-job` line and **without** the `job-unknown`
line, both directions, for `run` and `submit` alike
(`herdctl/tests/exit_code.rs`, and again through the real binaries in
`herd-hub/tests/away_fleet.rs`). The narrowness of the step back is a unit
test of its own. What is **not** asserted is the emitting guard: it would need
a hub that answers `JobAccepted` and then refuses, which no hub does, and a
fixture built to misbehave would be testing the fixture. The half of that
guard which makes such a hub harmless anyway — `AJob` does not move — is
asserted.

---

## 2026-10-16 — D144 The server runner's real caps, and the module behind them

**What the server runner carries now, with Alexa's approval.** The first
consumer asked for settings on the `server` runner before submitting their
first declared job, he approved them, and the unit was restarted with them:
`--job-timeout 10800`, `--job-pids-max 4096`,
`--job-io-max rbps=775MB,wbps=435MB,riops=180000,wiops=105000`,
`--capabilities /mnt/ssd_980/herd-etc/chatons-runner.json`, and
`PATH=/run/wrappers/bin:/run/current-system/sw/bin` where it had been the
system profile alone — the setuid wrappers live only in the first of those,
and the profile carries the same names (`sudo`, `su`, `mount`, `fusermount`)
as plain `r-xr-xr-x` binaries that fail for want of the bit, so the order is
load-bearing and not a preference. Everything else is unchanged: 8 GiB per
job, 20 GiB of host memory over a 6 GiB floor, 8 threads, 64 MiB of log,
20 GiB of disk, 4 GiB of tmpfs measured on /dev/shm.

**The `io.max` figures are theirs, and their correction to mine is the part
worth keeping.** They are a quarter of the kernel's own cost model for device
`259:0`, which is the derivation the help text asks for rather than a round
number somebody liked (D135). `/sys/fs/cgroup/io.cost.model` reads
`rbps=3102524156 rseqiops=724816 rrandiops=778122 wbps=1742780862
wseqiops=425702 wrandiops=443193`. The bytes are arithmetic and we agreed:
775 MB and 435 MB. The IOPS are not, because the model gives **two** columns
and I had read the wrong one — `wiops=110000` is a quarter of 443193, the
random column. They take each IOPS key as the **lower of the two columns**,
which here is the sequential one both times: 724816/4 → `riops=180000` and
425702/4 → `wiops=105000`. Theirs is right. A ceiling derived from the higher
column promises throughput the device only reaches on its best-case access
pattern, so it would be a ceiling that does not bind where it matters; the
lower column is a figure the device holds whatever a job's IO looks like.

**The ceiling is proved by writing it, and the startup line names the whole
device.** `herd-runner: io.max for device 259:0
(/mnt/ssd_980/herd-runner): rbps=775000000 wbps=435000000 riops=180000
wiops=105000 — … proved at startup by writing it.` `259:0` is `nvme0n1`, the
whole NVMe, and not the partition the data root is actually on: a cgroup's IO
knobs live on the device with the request queue, and `259:1` has no
`queue/` directory of its own. So reading `259:0` in that line is correct and
not a resolution bug.

**What the per-device ceiling does and does not cover here, which is the fact
to keep.** On this host `/mnt/ssd_980/chatons` — the consumer's whole tree —
is the *same mount* as the data root, `/dev/nvme0n1p1` on `/mnt/ssd_980`. So
the ceiling, which is per cgroup *and* per device, covers everything a job
touches under that mount and not merely the data root: wider than D135's
caution reads. But `/nix/store` is `rpool/nix`, ZFS, which **no** cgroup IO
knob reaches at any value. The consequence is sharp and worth saying in one
line: a job whose heavy phase is `nix build` is outside the ceiling, and a job
whose heavy phase is cargo under `/mnt/ssd_980` is inside it. For this
consumer, whose answer to toolchains is `nix develop`, that is not a corner
case.

**The module could not say three of those figures, so a rebuild would have
weakened the deployment.** `deploy/nixos-herd-runner.nix` hardcoded
`--job-timeout 5400` and had no way at all to ask for `--job-pids-max` or
`--job-io-max` except as strings in `extraArgs`. Alexa's next
`nixos-rebuild` — the one whose whole purpose is to make the deployment
survive a reboot (E-52, D130) — would therefore have replaced an agreed 3 h
cap with 1.5 h, 4096 pids with 1024, and **a hard IO ceiling with none at
all**, which is the single thing this consumer most needs from us. So the
module moved in the same breath: `jobTimeout` (default still 5400, so no
other machine changes), `jobPidsMax` and `jobIoMax` both absent by default so
that absent means the flag is off the line and the runner's own default
stands, `jobPath` defaulting to the wrappers directory first, and the server's
five values written out as the worked example in the header. The general
lesson is not about these three options: **a transient unit and a module that
disagree is a deployment that regresses the moment it is made permanent.** The
module's header already said the copied unit and it must be changed together;
nothing said the *running* unit and the module must be.

**`--host-tmpfs-path` was asked for and refused.** The request was to point
it at a directory of the consumer's. That option only says **where
`--host-tmpfs` is measured** — one admission figure, read once from the
filesystem at that path and announced to the hub — and it never places a
single byte a job writes. The path asked for is mode `0700` and owned by
another user, so the runner, which runs as `herd`, cannot `statfs` it at all.
The exact consequence, read off the code rather than assumed: because
`--host-tmpfs 4GiB` **is** given on this unit, an unmeasurable path does not
blank the figure — the runner lends the 4 GiB as asked and prints that it was
not checked against the mount, so the only effect is to drop the clamp that
keeps the announcement no larger than the mount really is. The figure goes
**absent**, and every job declaring a tmpfs reservation is then refused on
this machine by name, only if `--host-tmpfs` is dropped in the same move.
Either way the change buys nothing a job can use. What they actually want is
their declared command's own `TMPDIR` — and the runner builds a job's
environment from scratch, so a job's `TMPDIR` is the one it creates under that
job's workspace, never one inherited from the unit and never this option's.
That is a different request and it stays open.

**One mistake in their capability file, caught before it emptied the
machine.** `reference` — a fact about this host, a checkout that is simply
*there* — was declared `exclusive`. `exclusive` means a job holding it runs
alone: declaring it on a permanent fact would have made every `reference` job
drain the runner to itself. `shared` is the kind for a fact about a machine,
and the file now says so. `rig-cpu` stays `exclusive` on purpose: it is the
machine, and a job that wants the machine does want it empty.
## 2026-10-16 — D145 Windows means WSL2, and what a native runner would cost

**The question.** Alexa asked whether a Windows version is conceivable. The
answer is in two halves, because "Herd on Windows" means two different
programs.

**The runner: WSL2, which is already the plan.** The second machine of M17 is
a Windows PC, and it joins as a runner *inside* WSL2 — a real Linux kernel
(theirs is 6.18) with cgroup v2 and systemd, which is exactly the host the
runner is written for. Q6 already rules the memory ceiling that way and M17
already carries the WSL checklist and the Windows logon task. So a Windows
machine contributes capacity today with no port: nothing to build, and the
only Windows-specific surface is a logon task that starts a WSL command.

**A native Windows runner is refused, and the price is the reason.** The
runner's whole safety story is cgroup v2: `memory.max`, `cpu.weight`,
`io.max`, `pids.max`, one subtree per job, and `/proc/pressure` recorded
beside it. Where there is no cgroup the code falls back to `RLIMIT_AS`, which
is itself Unix. On Windows there is neither, so the fallback is *nothing* —
invariant 2 would break on the first job, silently, which is the one failure
mode this project refuses. Making it honest means a third confinement
backend on Job Objects, and that backend is not a uniform translation:

- **memory** exists (`JobMemoryLimit`) but with different semantics — the
  allocation *fails* instead of the offender being killed, so a job over its
  cap surfaces as a compiler error, not as our exit 124;
- **kill the whole tree** exists, cleanly (`KILL_ON_JOB_CLOSE`);
- **CPU share** exists (Job Object CPU rate control);
- **block IO has no per-job equivalent at all.** There is no `io.max`.
  Storage QoS is for Hyper-V and SMB, not for a process tree. The knob that
  stops a build from freezing its host — the one thing the first consumer most
  needs from us — cannot be written on Windows;
- **pressure has no equivalent**, so admission (D119) loses the figure it
  reads;
- **a clean stop loses its graceful step**: there is no `SIGTERM` to a job's
  children, so "stop in ≤ 5 s" becomes "terminate in ≤ 5 s";
- **there is no tmpfs**, so the workspace measurement has no meaning there.

That is a milestone's worth of work for a machine that can already join
through WSL2, and it permanently doubles every confinement test — the tests
that are the only reason we believe invariant 2. Invariant 1 says to refuse
that kind of surface, and this is refused.

**`herdctl.exe` is the cheap half, and is not refused — just not built yet.**
Its sources contain no Unix-specific code and its dependencies are all
portable; it is a WebSocket client, an exit code and a file write. If someone
ever wants to submit from PowerShell without WSL, that is a cross-compile
target in the release workflow, not a port. It stays unbuilt until a requester
actually needs it, because WSL already gives a Windows user a working
`herdctl`.

**What would reopen the runner half.** A Windows machine that cannot run WSL2
and whose capacity we need anyway. Until one exists, the answer to "a Windows
version?" is "yes, through WSL2" for a runner and "on request" for the CLI.

**On the number.** There is no D143. Three branches appended an entry on the
same day and each took the next free number against a `main` that then moved
under it; this one was renumbered twice to keep the file monotonic, which
matters here — the file is read in order — where a contiguous sequence does
not. Nothing references 143.

---

## 2026-10-16 — D146 A harness must not ask the network what it knows

**`main` went red on the test harness, not on the product.** CI's `check`
job failed in the `herd-hub` integration binary `killed_hub`, on
`a_connection_that_served_starts_the_redial_counters_over`, 130 ms after
that test started:

```
thread '…' panicked at herd-hub/tests/real_herd/mod.rs:265:21:
approving the-real-runner failed: herdctl: herd cannot give you a verdict
for this job: dialling ws://127.0.0.1:34889: IO error: Connection refused
(os error 111): Connection refused (os error 111)
```

It reproduced on a docs-only pull request, which is what proved it was
`main` and not the change. Nothing in the hub, the runner or `herdctl` was
wrong.

**The defect: `Herd::approve` decided how to approve by probing its own
port.** A runner is approved before it starts (E-48), and there are two
ways: `herdctl runner approve` over the wire, which needs a hub up, or
minting the token on the state root directly, which is what a deployment
does before its first hub start and which needs the root's lock free (D21).
The two want opposite things of that lock, so the only question is "is a hub
of mine running" — and `approve` answered it with `TcpStream::connect` on
the herd's own address. In the failing test there is no hub at that point on
purpose: the runner is started against a port nothing is listening on, and
the hub appears twenty lines later. The probe said the hub was up, the wire
path was taken, and `herdctl` was refused.

**Proved: a stray listener on the port is sufficient, and is what the CI
output looks like.** A listener put on the herd's port on purpose, which
accepts the probe's one connection and then closes, reproduces the CI panic
at the same line and word for word — same message, same
`Connection refused (os error 111)`. That is now a test,
`a_stray_listener_does_not_decide_how_a_runner_is_approved`, so what struck
once in CI is one run here.

**Hypothesis, not proved: the stray listener in CI was `free_port`'s own.**
`free_port` hands out a port by binding `127.0.0.1:0`, reading the number
and letting the listener go; a port it has handed out before is passed over.
Two measurements on the development host, Linux 6.18, make it the candidate:
`bind(0)` returns a port it released within some 120 allocations of a
28 232-port range, so a repeat is the ordinary case that loop exists for —
and on the way to passing a repeat over, `free_port` held a *listening
socket on another herd's port* across a contended `Mutex`. Two cores make
that window a scheduler timeslice wide, which fits a failure seen only
there. What was not done is catching it happening: the natural failure did
not reproduce here in 60 runs of the test alone and 25 of the whole binary
under a low-weight scope, nor in the runs before the fix. The mechanism is
sufficient and the window is real; that this particular window is the one
CI fell into remains inference.

**Refuted: the kernel's own source-port choice.** The first hypothesis was
a loopback self-connect — `free_port`'s number goes back into the ephemeral
range, the kernel hands it to an outgoing connection as its *source* port,
and a TCP connection whose source port equals its destination port
completes by simultaneous open. The second half is true: forcing the source
port with `bind` and then connecting to that same port succeeds on this
kernel. The first half is not. 200 000 `TcpStream::connect` calls to a dead
loopback port in the ephemeral range produced zero self-connections, so
Linux's automatic source-port choice does not pick the destination port, and
that mechanism cannot fire through `connect`. The CI runner's kernel is a
different one and was not measured.

**Choice: the approval path is read off the herd's own state.** A `Herd`
starts every hub it has and is handed each one back when it dies, so it can
simply count them. `HubCensus` is incremented when a hub is spawned and
decremented exactly once however that process ends — signalled, waited for,
or merely dropped — and `approve` reads the count. It keeps its blocking
signature, which forty-odd call sites depend on. A hub killed and restarted,
and `forget_approvals` on a recreated state root, both follow from the count
with no special case.

**And the failure mode is named rather than made unlikely.** If the wire
path is ever taken while nothing answers, the assertion now opens with "THE
HARNESS IS WRONG, NOT THE PRODUCT", gives the count the herd believed, and
says not to go looking in `herd-hub`. The other side says the matching
thing: a state root that will not open because something holds its lock
accuses the census of having lost count. The old sentence — "approving
failed: connection refused" — reads as the product refusing an approval, and
that is what it cost: the hunt began in the hub and the runner and reached
the harness last. A harness that lies about which side is broken is worse
than one that merely fails.

**Alternative rejected: `free_port` holding its listener until the hub
binds.** It cannot. The hub binds the port itself through its own
`--listen`, which is the code these tests exist to drive, so a harness still
holding it would hand the hub `Address already in use`; passing the socket
down to the child would mean the hub no longer binds its own port. What is
done instead is to close the listener before the handed-out set is touched,
which shrinks the window to almost nothing — and, with nothing probing any
more, to nothing that decides anything.

**Carried through the suite.** `capabilities.rs` waited for its hub with the
same connect probe; it now waits for the hub to *say* it is listening, as
`real_herd::start_hub_with` already did for the same reason. Two assertions
in `killed_hub.rs` still use `TcpStream::connect(..).is_err()` to check that
a refused hub did not bind: those are assertions about the product rather
than decisions taken by the harness, and with `free_port`'s window closed
their only remaining exposure is the self-connect refuted above. They are
left alone.

**On the number.** Written as D147, renumbered to 146 on merge: the entry
that had claimed 146 was on a branch this one overtook, and it is now 147.
The file is read in order, so order is what is kept — the same collision
D145 describes, resolved the same way.

---

## 2026-10-16 — D147 The first files home, and the first figures under qos

**A job's files come home on the real deployment, and this is the entry
that stops that being a claim about a test suite.** The first consumer's
`j0000000030` (declared `sh`) and `j0000000031` (declared `test posix -j 1`,
labels `build`+`kvm`) both ended at exit 0, and their `results.json` — 50 and
616 bytes — are in `jobs/<id>/files/` on the hub's disk. I read them there
rather than taking the report: the 616-byte one carries the consumer's own
verdict object, `"verdict":"ok"`, `"seconds":189.1`, and a `load_after` of
**118.34**. So D131's ruling and D137's cap are not merely built; the path
from a job's `{out}` to a requester's directory has run end to end on this
host, which is the one claim M16's deployment could not make this morning.

**What that job's own figures say about the host.** It ran `posix` in 57.5 s
of a 189 s job while the machine's load reached 118, and it neither failed nor
was throttled out of existence. The job's cgroup carried the caps and the
scheduler did the rest.

**The first measurement of `io.cost.qos` under load, from the consumer.**
D136 asked for a latency target and somebody set it; D135 built the hard
ceiling. Their first data point, during an ACL walk plus a QEMU: IO pressure
`full avg10` reached **95 % for one to two minutes**, spread **evenly** across
`docker.service`, `user.slice`, `herd-runner.service` and their own slice
(60-80 % `full avg60` each), with NVMe `inflight` at 0 at the sample — and it
fell back to 4.8 % within about 20 s. **No freeze of the kind this host used
to have.**

Three things worth keeping from that, in order of how much they cost us to
learn:

- **Even spread is the signature of apportionment working**, not of a
  bottleneck. Before the latency target, `rpct=0.00 wpct=0.00` meant
  blk-iocost apportioned the device and never reduced total load (D120,
  D136), so one writer could hold the disk while the pressure figure said
  everyone was stalled. Four slices each at 60-80 % with the device *idle*
  at the sample is the controller holding each of them to a share.
- **A pressure figure near 100 % is not a verdict about the machine.** E-32
  says pressure is recorded and gates nothing, and this is the first live
  evidence for why: 95 % `full avg10` for two minutes, and the host stayed
  usable. A gate on that number would have refused every job for two minutes
  and been wrong each time.
- **`inflight` 0 under 95 % pressure** means the stall was in waiting, not in
  the device. Pressure measures time processes could not make progress; it
  does not say the disk was busy. Anyone reading these numbers later should
  not infer saturation from them.

This is one data point, taken during an unusual operation, and it is not a
heavy window. The consumer will send figures from a real one. Recorded now
because it is the first evidence either way, and because the shape of it —
even, brief, recovered — is what we would want to see again.

**What is still unproven, so that this entry does not overstate.** The
reaper's janitor list is unbuilt (D134) and the consumer's failing `up` steps
walked into it: when a `down` step fails too, nothing here sweeps after it.
No `wss://` dial has happened. And the runner's unit is still transient
(D130), so every cap in D144 survives exactly until the next reboot.

---

## 2026-10-16 — D149 M17 prepared without the machine, and what is unproven

**The consumer's top priority is the second machine, and the owner's time
has to be booked for it.** So everything M17 needs that can be prepared
without a WSL2 host is prepared now, and the valuable half of this entry
is the other list: what is still guesswork, and the exact command that
ends each guess. The machine is Alexa's own PC — Windows, WSL2 with
kernel 6.18 and systemd, outbound only, behind NAT, on the tailnet — and
it joins as a runner *inside* WSL2 by D145.

**What that machine must end up with, and where each of the four lives
now.** Her switch (E-33) is a file under the data root, and
`docs/joining.md` gives it a section written for her rather than for an
operator, with the two desktop shortcuts and the one trap that was
verified here: `herd-runner switch` creates the data root if it is
missing and reports success either way, so a shortcut with a typo'd path
is cheerful and inert. A kernel-enforced ceiling of about 12 GiB (Q6,
invariant 2) is `MemoryMax=12G` on a user unit carrying `Delegate=yes`,
with `--host-memory 11GiB` under it so that the runner announces the
figure that was written instead of clamping and saying so (D119). One
build slot and `kvm` denied are `deploy/capabilities-wsl-pc.json`, four
lines, validated by running the real binary against it — it resolves to
`build: declared` and `kvm: present, denied`.

**The memory ceiling is the whole milestone, and the runner says which
one it got.** Delegation present and the startup report reads `memory
ceiling: one cgroup v2 subtree per job, enforced by the kernel`. Absent
and it reads `memory ceiling: FALLING BACK to a per-process
address-space limit`, followed by the process-group kill and by
`cpu.weight, cpu.max, io.weight and pids.max: NOT ENFORCED AT ALL on
this host`. The document says plainly that the fallback is **not
acceptable on this machine**: `RLIMIT_AS` caps each process, so a build
forking eight compilers holds the ceiling eight times over, it makes an
allocation fail rather than killing the job, and with no cgroup nothing
bounds a fork bomb. On a server that is arguable; on the PC somebody
plays on it is not invariant 2.

**Two ceilings, and conflating them is the likely mistake.**
`.wslconfig`'s `memory=` is what Windows gives the WSL2 VM; everything
Herd sets is inside that. The chain is written down outside in — PC RAM,
VM RAM, `MemoryMax`, `--host-memory`, `--host-memory-floor`,
`--job-memory-max` — with a coherent set for a 32 GiB PC, and with the
statement that **on a 16 GiB PC the 12 GiB of Q6 has to be revisited
rather than squeezed**: a 12 GiB ceiling on a 16 GiB machine is not
lending capacity, it is taking the machine. The number Alexa has to look
up is the PC's RAM.

**One script, run once on the PC, and we both learn the truth.**
`deploy/wsl2-check.sh` answers in one block: systemd as PID 1, cgroup v2
mounted, whether a `--user --scope -p Delegate=yes` really gets a subtree
with the `memory` controller in it, what `.wslconfig` gave the VM against
what `/proc/meminfo` says it got, whether the hub's address answers a TCP
connection from inside the distribution, and whether `herd-runner
--version` runs. It is bash, read-only but for one transient scope and
one probe directory it removes, and it marks what it could not find out
as `UNKNOWN` rather than passing it. Run on this NixOS host it prints
`0 FAILED, 4 UNKNOWN`, correctly says this is not WSL2, and correctly
fails to make the delegation assertion — which is the behaviour wanted of
it. There is no `shellcheck` in the dev shell or on this host, so it was
checked with `bash -n` and by running it, and that is all.

**What nobody has verified, with the command that settles each.** This is
the half of the entry to read before booking his time.

| Unverified | What ends it |
| --- | --- |
| whether this distribution already boots systemd | `cat /etc/wsl.conf`, then `ps -p 1 -o comm=` — must print `systemd` |
| whether this PC's WSL is new enough to honour `[boot] systemd=true` | `wsl --version`, from Windows |
| whether a user scope on WSL2 gets a delegated subtree at all | the `delegation` line of `deploy/wsl2-check.sh` |
| whether the `memory` controller reaches a user unit there, which is what makes `MemoryMax=12G` and the per-job ceiling real | the controller list on that same line; no `memory`, no ceiling, and the answer is a system unit instead |
| what WSL2 gives the VM with no `.wslconfig` — the rule has changed between versions | `grep MemTotal /proc/meminfo` inside, and treat it as the answer |
| the PC's actual RAM, on which every figure depends | `powershell -c "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"` |
| whether the WSL2 VM keeps running after the logon task's `wsl.exe` exits | log in, close everything, wait two minutes, then `wsl.exe -d <distro> -- systemctl --user --no-pager status herd-runner` |
| whether `loginctl enable-linger` behaves there | `loginctl show-user "$USER" -p Linger` must print `Linger=yes` |
| whether the VM is killed under her on idle, sleep or hibernate | the same status command after each; `[wsl2] vmIdleTimeout` is the first thing to read if it is |
| whether a Windows shortcut carries the quoted `--reason` through | double-click it once, then `cat ~/.local/state/herd-runner/switch` |
| whether the tailnet reaches the hub from inside the distribution, and by which of the two shapes | the `hub-tcp`, `tailscale` and `/dev/net/tun` lines of the script |
| whether an `io.weight` reaches a WSL2 data root at all | the runner's own `io.weight` line at startup (D120) |
| the 30-minute join, the OOM replay (E-30) and the switch inside 10 s | M17's three manual checks, on the machine, recorded in `docs/status.md` |

**One thing found while reading, reported and not fixed.** Nothing in the
runner needs changing for WSL2: the code paths this machine exercises are
the same ones the server exercises, and the only Windows-specific surface
is a logon task that starts a distribution. The one real sharp edge is
the `switch` command creating its own data root, which is a product
question and not a WSL2 one — it is written down in `docs/joining.md` as
a trap to check for rather than changed here.

---

## 2026-10-16 — D148 A record streams through the hub; a pack never does

**The first consumer has made Herd their only path and asked for three
things: chunked return, hub-side streaming, and a retention policy, sized
for a few hundred MB per job.** Their sentence is *"once a job can run on
another machine, logs, screenshots and scenario files no longer sit on the
server's disk; results.json alone is not enough to diagnose a red test."*
Today a job's `{out}` comes home inside its record, on the frame that
carries the verdict, capped at 32 MiB (D131, D137) — and D147 proved that
path on the real deployment with a 616-byte `results.json`. Their
screenshots are refused by name with figures, which is D138 admitting I had
told them otherwise.

**The finding that decides it: there are two flows, and only one of them is
the byte path D10 removed.** Both are "files a job produced", which is why
the consumer's sentence runs them together, and they are not the same thing.

* **Runner → runner.** A build makes a pack; a test job on another machine
  consumes it. ~400 MiB in ~560 files, measured. What reads it next is a
  **job**. This is E-12, E-55 and M11, and D10 is exactly right about it:
  the hub registers, peers transfer, the same host hardlinks, a keeper
  covers the machine with no mesh. Nothing here changes it.
* **Runner → requester.** A job's log, its `results.json`, its per-scenario
  screenshots. What reads it next is a **person or an agent diagnosing a red
  test**, holding a job id and nothing else. This is the **record** — E-15,
  D8's `files/`, D131's ruling, and D137's cap.

**Size is not what separates them. Who reads them next is.** A 400 MiB pack
and a 400 MiB screenshot directory weigh the same and have nothing else in
common: one is an input to future work, keyed by (commit, key),
content-addressed, wanted on a particular machine; the other is evidence
about one job, keyed by that job's id, wanted by whoever asks.

**So the answer is (c), and it is not a compromise between (a) and (b).** It
is (a) for the record and (b) for packs, with the boundary drawn at the
consumer instead of at a byte count. Alexa leans to (b) for everything
because it is the reading that repeals no invariant. Three findings argue
against it for the record.

**One: the hub has carried bytes since M1, and it is called the log.**
`ToHub::LogChunk` → a `log` file on the hub's disk → `FromHub::LogChunk` to
the follower. 64 MiB a job by default, in 8 KiB chunks, sequence-numbered,
acknowledged by `LogAck { through_seq }`, re-asked for by
`ResendLog { from_seq }`, surviving a hub restart and a lost tail (D29,
D36, E-44). Nobody reads that as a breach of D10, because it is not one. By
D131's own argument a screenshot is in that category and not the pack's. So
**the mechanism for streaming large record content through the hub already
exists, is deployed and is tested** — what does not exist is using it for
files. That is the whole of the work, and it is why this costs less than it
looks.

**Two: (b) does not actually save the disk, because the default keeper is
the server's runner.** D10 makes it the default keeper and D126 put its data
root on `/mnt/ssd_980` — the same NVMe as the hub's volume. Under (b),
yesterday's screenshots survive the PC being switched off only by being
pinned on the server, so the bytes land on the consumer's own NVMe either
way. (b) moves which directory owns them and adds a replication step; the
disk budget and the retention policy that (a) is accused of needing are
needed by (b) too, which is what E-16 already says.

**Three: (b) puts a new requirement on the one party we have no leverage
over.** A requester today needs one address and one token, and D145 is
contemplating a `herdctl.exe` that needs no more. Under (b) it would need
mesh membership, a second protocol, and a runner that is awake — so "how do
I read yesterday's failing screenshots?" would be answered *"from the PC, if
she has not switched it off"*. That is the brief's own custody worry, and it
is a standing cost rather than a case to handle. **A verdict and its
evidence must be readable from the hub alone, because the hub is the one
thing that is always up.**

**What this costs, said plainly.** The hub's volume stops being the
kilobytes Q9 promised and becomes a budgeted resource with a sweeper — which
is new, is work, and is the price of this choice rather than a detail of it.
And the server's own runner pays a copy of its own output through the
container boundary, which is D10's own complaint; no same-host shortcut is
taken, because writing straight into the hub's volume would couple a runner
to the hub's storage layout across that boundary.

**A large output is declared as nothing at all.** `{out}` keeps meaning
exactly what it means. A requester declares no size, no class and no flag,
because the brief's requirement is right: nobody knows in advance whether a
run leaves 1 MiB or 400 MiB. The **runner** measures the collection when the
job ends and picks the carriage:

* at or under 32 MiB it rides the verdict frame, exactly as today — one
  round trip, no new frame on the common path, nothing D137 built touched;
* over it, up to the job's output budget, it streams.

Both land in `jobs/<id>/files/` with relative paths kept, and
`--outputs-to <dir>` is the same flag writing the same tree. **A requester
cannot tell which carriage was used except by a line on the record.**

That is why 32 MiB is **kept as a figure and changed in meaning**: it stops
being a refusal boundary and becomes the boundary between two carriages. The
frame arithmetic that derived it (`FRAME_CEILING`, base64, 140 MiB of peak
memory a party) is untouched and still governs the inline path, so the
number is still derived from what can arrive. What does change is the flag:
`--job-output-cap` becomes **how much a job may return in all**, may now be
set above 32 MiB because a carriage for that exists, and is bounded by what
the hub will accept per job rather than by what one frame holds. The inline
threshold becomes a constant nobody tunes, which is what pure frame
arithmetic should always have been.

**`leaves` is still not this.** `needs`/`leaves` are M11's packs, keyed by
(commit, key), content-addressed, for a job downstream. The record is keyed
by job id for a requester who never saw the declaration. D137 settled that
and this does not reopen it.

**Chunking and resumption: the log's own discipline, applied to files.**

* Each file is announced with its relative path, its length and its
  **sha256** before a byte of it moves. The hash is not decoration: it is
  what makes a resumed transfer safe, and it is what a requester verifies.
* Bytes travel in bounded chunks — 1 MiB, derived the way
  `JOB_LOG_FRAME_BYTES` below is derived, from the event loop and not from
  the ceiling. The hub writes into the temp file D8's discipline already
  uses and renames only **after** the declared hash verifies.
* **The verdict goes first and the files follow it.** Three reasons, and
  they all point the same way: D20 says a side channel never delays or
  alters a verdict; invariant 2 says stopping never waits for bytes; and a
  runner that dies mid-transfer has then already delivered the verdict, so
  the job is readable and only its evidence is pending. The record carries
  the manifest from the moment the verdict lands, each file `pending` or
  `held`, so "12 files, 3 arrived" is a readable state and not a gap.
  `herdctl run` with no flag exits on the verdict and waits for nothing,
  which is today's behaviour; `--outputs-to` waits for what it asked for and
  says so.
* **When the link dies halfway**, the runner still holds the collection: it
  is copied out of `{out}` into the runner's own data root before the
  workspace is reclaimed, and held until the hub acknowledges the whole of
  it. That is M2's existing promise about a finished result, extended to its
  files — D19 had to be widened for exactly this reason once already (D137).
  On redial the hub says which files it holds complete; the runner resumes
  the rest, and resumes a partial one from the length of the hub's temp
  file. A hash that does not match at the end discards that file and retries
  it once; a second mismatch is a named line on the record, never a quiet
  half-file.
* **Hub → requester needs no resumption state at all.** The bytes are on the
  hub's disk, so a `--outputs-to` that died is simply run again, skipping
  every file it already has at the right length and hash. The manifest's
  hashes are the whole of the resumption design on that leg.
* **What the requester sees**: one line per file on stderr as it lands,
  never stdout (E-02). On an incomplete fetch the **job's exit code stands**
  — D20 again, and D138 kept that ruling once already — and the failure is a
  loud sentence naming what arrived and what did not. `--outputs-required`
  is the flag for a requester that wants it fatal, because the alternative
  is making it diff a directory against a manifest the hub already holds.
* **Invariant 2 is not negotiated with.** A transfer in flight is abandoned
  mid-chunk when the owner says `off`; the collection stays on the runner's
  disk and resumes next time. A **drain** is the other case and has to be
  said out loud: a draining runner finishes the job it has, and "finished"
  must include delivering that job's record, or a drain would destroy the
  result of the job it had just allowed to complete.

**Retention, in the consumer's own question.** They will ask *how long do I
have to fetch yesterday's failing screenshots?* The answer is a number, and
there are two numbers because there are two kinds of thing.

* The record proper — `job.json`, `events.jsonl`, the log, the inline files
  — keeps M12's retention by priority class: 14 days and 90 days (E-15).
* A job's **large files** get their own budget on the hub's volume, in
  bytes, with a floor in days: **7 days or the budget, whichever runs out
  first**, oldest job first. Both printed at startup like every other
  setting the hub has, and shown as use against budget in M13's status
  document.
* **The floor is a promise the hub cannot always keep, so it says when it
  breaks it.** Evicting something still inside the floor is a named line and
  an event (M13), never a silent trim. *"Seven days, unless the fleet
  produces more than the budget in less than seven days, in which case we
  tell you on the day"* is a sentence a consumer can plan against. *"About a
  week"* is not.
* **The hub deletes, on its own volume** — one disk, one sweeper, no
  custody, which is most of the point of choosing (a) for this flow. A
  runner deletes only its own undelivered collections, and only once the hub
  has acknowledged them or the record they belong to is gone.

**And what a requester gets for something already gone is the best part of
this: the manifest outlives the bytes.** `job.json` keeps every file's path,
length and sha256 — kilobytes, so there is never a reason to delete it — and
retention removes only the bytes. So `herdctl wait <id> --outputs-to <dir>`
on an evicted job answers *12 files, 338 MiB, removed 2026-10-30 under the
200 GB output budget*, with the names. It never returns an empty directory
and never returns a short one, and the same shape answers the runner that
never came back: *4 of 12 files never arrived; `pc` has not been seen since
…*. **A record that had files says so for as long as the record exists.**
That is also E-54's offline re-judging getting an honest answer instead of a
missing one.

**Custody (M12) shrinks again, and the switched-off PC stops being a
problem for the record.** A job's record has left the runner by the time its
files are acknowledged, and its verdict before that, so **a verdict is never
unreadable because a machine is asleep** — the brief's worry, and (b)'s
standing cost. What keepers are still exactly right for is **packs**: an
input to future work, genuinely runner-to-runner, with D10's keeper, its
named no-reachable-holder state, and per-runner budgets. So M12 carries
custody of packs, GC of runner disks, and the hub's own output budget, and
carries no custody of records at all.
**The frame cost: yes, this needs version 2, and version 2 is approved.**
**The frame cost: yes, this needs version 2.** Plainly, with no dodge
available. It needs new variants on both enums — `ToHub` for a runner
announcing a file and sending its bytes, `FromHub` for the hub telling a
reattaching runner what it already holds and for streaming files to a
requester — and D124 and D128 together say a new variant on either enum is
breaking whatever serde would tolerate. D137 used the only defaulted-field
dodge there was.

A break is paid by the deployment, not by the code, so it is spent **once**.
What should ride along:

1. **All of M11's registry frames**: a runner advertising which blobs it
   holds, registering a manifest, asking who holds a pack, and the hub's
   short-lived scoped fetch capability for a (blob, peer) pair. They are the
   next milestone and they are already specified. One coordinated restart of
   a hub and two runners instead of two.
2. **The runner's unknown-frame arm.** D128 recorded that an unknown
   `FromHub` variant makes a runner **loop** — the decode fails, the link
   dies, it redials into the same frame — and called it a defect without
   fixing it. A version boundary is the one moment to: a catch-all variant
   so a runner logs and ignores what it cannot read. It does **not** make
   future additions free, because `herdctl`'s `bail!` is correct and stays
   (D124: a requester that cannot read what the hub is telling it must not
   report a verdict). It means a runner-only addition after version 2 is a
   note rather than an outage.
3. **Nothing from M15.** Approval and the version pin are not designed, and
   a frame invented for them now would be invented from reading, which is
   the mistake D128 is an entry about. If M15 needs one it pays for version
   3.

**What must not ride along**, because a moving number makes everything look
free: raising `FRAME_CEILING`, raising the 32 MiB inline threshold, or
moving the log cap upward. Each is derived from what one party can hold in
memory at once, and a new carriage beside them changes none of those
derivations.

**Two rulings from Alexa, both taken on 2026-10-16, both decided rather
than deferred.**

1. **`PROTOCOL_VERSION` moves to 2, approved for M11**, bundled exactly as
   recommended: this entry's file frames, M11's registry frames, and the
   catch-all arm. He accepts that the hub and both runners restart together
   and that `herdctl` is rebuilt. The version refusal already names both
   numbers (E-51), so a stale peer is told rather than broken, and that
   property is asserted rather than assumed.
2. **The hub's output budget is 200 GB, with the seven-day floor above.**
   He chose the figure rather than wait for the consumer's, and it is to be
   adjusted when they send jobs per day times typical output size.

**The budget is a chosen number and must never read as a derived one.**
200 GB is not computed from anything: no measurement produced it, and the
arithmetic that would — a few hundred MB times the number of jobs a day
that really leave that much — needs a figure only the consumer has. **The
one measurement that would change it** is their distribution of output size
per job per day; at five such jobs a day 200 GB is months of headroom, at
five hundred it is under two days and the floor breaks every day. Every
other figure in this design is derived from what a mechanism can carry —
`FRAME_CEILING` from what one party can hold in memory, the 32 MiB inline
threshold from base64 against that ceiling — and this one is not, which is
why it sits in its own paragraph instead of beside them. D138's lesson is
not "never choose": it is that a chosen number must not wear a derived
number's clothes, because whoever reasons from it next will reason from the
clothes.

**What is unproven, so this entry does not overstate.** No byte of a few
hundred MB has gone through the hub; the log path has now carried 56 MiB end
to end, which is evidence and not the figure. 400 MiB in ~560 files means
~560 announce-and-verify round trips on top of the bytes, and the per-file
cost is not measured. The event-loop claim — that 1 MiB chunks keep a
400 MiB transfer inside the 15 s cadence a quiet link lives by — is an
argument, not a measurement, and it is the first thing M11 must measure.
And the resumption hash retry exists because it is cheap, not because a
mismatch was ever observed.

**One defect found while designing this, and it is what was built today.**
`FromHub::JobLog` carried a job's **whole** recorded log in one frame. The
default log cap is 64 MiB, base64 makes that about 85 MiB, and
`FRAME_CEILING` is 64 MiB — so a job that printed more than about 48 MiB
built a frame neither side could put through a socket, and a `herdctl wait`
on it got a closed link and **exit 125 with no verdict for a job that had
passed**. D117's exact shape, on the one path nothing had ever exercised,
and the same class of mistake D137 found at 16 MiB and then fixed only for
the output frame — having taken its own first cap from this very neighbour
without ever checking it. Measured, before the fix, on a real hub with a
real runner and the real requester code:

```
a late attach to a loud job returns a verdict rather than a closed link
Caused by: Space limit exceeded: Message too long: 78293727 > 67108864
```

The fix is that the backlog travels in frames of `JOB_LOG_FRAME_BYTES`,
8 MiB, and a requester writes each out as it arrives — several frames are
byte-identical to the one frame this used to be, because `JobLog` has always
meant "here are bytes of this job's log". **No variant, no field, no
`PROTOCOL_VERSION` move, and the deployed `herdctl` reads it unchanged**,
which is the one change D128 leaves the number alone for. 8 MiB and not
32 MiB because base64 leaves five-sixths of the ceiling unused at that size
— room kept on purpose, so raising `--job-log-cap` cannot silently reopen
this — and because D138's real objection to a big frame was never the
ceiling but the event loop.

It is also the one slice of this design that was separable **without the
protocol move**, which is why it went to `main` on its own (PR #37) instead
of waiting here: the first consumer has removed their fallback, so a fix
they get without redeploying anything is worth more than a tidy one they do
not. Everything else above needs version 2 and lands together.

**On the number, and on what a number is for.** This entry is 148 and the
entry above it is 149: two branches took the next free number the same day,
and this one landed second. It was not renamed, and that is the rule worth
stating — **a decision number cited in code is an identifier, not a
position**. Forty-odd comments across `herd-proto`, `herd-hub` and the plan
name D148; renumbering it to tidy one inversion in this file would rewrite
all of them and break every reference anybody has already read. D145 kept
order where order was free. Here it is not, so the identifier wins.

---

## 2026-10-16 — D150 Building D148's carriage, and the four things it found

**D148 was approved and then built, and three of its sentences turned out to
be incompatible with code that was already there.** D128 is an entry about
exactly this — *"a prescription I write into a decision is a hypothesis until
it is built"* — so this is the follow-up it asks for. `PROTOCOL_VERSION` is
now **2**, the two carriages work end to end on real binaries, and none of
what follows was visible from reading.

**One: "the verdict goes first and the files follow" is incompatible with
`is_current`, and every chunk was silently dropped.** The hub guards every
frame about a job with one check — is this the attempt I am waiting for? — and
that check answers `JobIsOver` for a finished job, logs it, replies
`StaleAttempt` and skips the frame. Right for a log chunk, which can only
arrive while a job runs. Exactly wrong for a file chunk, which by design
**always** arrives after the verdict. So the runner streamed 32 chunks, the
hub read all 32, wrote none, and a requester was told its collection was not
there — with no error anywhere, because dropping a stale frame is a normal
thing for the hub to do.

The fix is a separate check (`files_may_land`) asserting what actually has to
be true: the record says this collection streamed, the attempt is the one on
the record, and the runner is the machine that ran it. **The general shape,
worth keeping:** a new frame that breaks an existing invariant about *when*
frames arrive will be refused by whatever enforces that invariant, silently,
because refusing is that code's job. "The verdict goes first" was one
sentence in a design and a load-bearing change to the hub's frame discipline.

**Two: a part-written file at full length is not a finished file, and two
definitions of "complete" disagreed for a hundred milliseconds.** The hub
writes chunks into `files.part/`, verifies the digest, and renames into
`files/` — so between the last byte landing and the rename there is a window
as long as it takes to hash the file, which for 32 MiB is about a tenth of a
second. `collections_owed` judged completeness off the **holdings**, which
report a part file's length, so in that window it said the collection was
complete; `send_the_files` then told the requester the bytes were no longer
coming, and the requester went looking in `files/` for something not yet
there. What it reported was *"the hub does not have it"* for a collection that
was a tenth of a second from arriving.

Now there is one definition, `collection_is_whole`, and every caller uses it:
in `files/`, at the manifest's length, verified. D79's rule about slots —
derive it once, so two answers cannot disagree — applies to bytes, and the
cost of two answers here was a wrong one at the worst possible moment.

**Three: `--outputs-to` has to wait, and the hub has to say whether waiting
is worth anything.** D148 said it waits, in one clause, and the first
implementation asked once. "Not there yet" and "gone for good" are opposite
instructions and look identical in a list of names, so `AbsentFile` now
carries `still_coming` and `held`: the requester waits while the figure moves
and stops when it stops. **The bound is on silence, not on duration** —
thirty seconds without progress — because a few hundred megabytes on a slow
link is slow and not broken, and a deadline on the transfer itself would make
the consumer's biggest runs the ones that fail.

**Four: the output cap's startup check became a clamp, not a refusal.** The
old check refused a cap bigger than one frame, and refusing was right: a cap
past the frame produced a verdict nothing could decode, which is D128's
redial loop. With the streamed carriage there is no frame ceiling, and what
bounds the flag instead is the disk — a job writes `{out}` inside its
workspace, which stops at `--job-disk-budget`. That is a figure that *cannot
bind* rather than one that breaks something, which is D89's case exactly, so
it is clamped loudly where it is written. Found by the confinement suite: its
harness lends a 64 MiB disk budget, the refusal fired, and **fifteen tests
lost their runner at startup** — a legitimate configuration refused. A test
suite rejecting a new refusal en masse is evidence about the refusal.

**And D128's loop, measured at last.** The catch-all arm was the cheap half
of this break, and it is now asserted against the real runner's redial loop
with a hand-written hub that says something from the future. With the arm:
**one dial** in 1.2 s, and health reports still flowing. With the arm removed:
**178 dials** in the same 1.2 s. D128 described that loop from reading the
code; this is the figure.

```
assertion `left == right` failed: a frame it cannot read must not make a
runner redial: 178 dials in 1.2 s is D128's loop, not a shrug
```

The three answers are all tested where the behaviour lives: a runner ignores
and stays, a requester ends its run naming the frame type (D124's `bail!`,
kept), and the hub refuses by name rather than leaving a peer waiting for
work it cannot read.

**What is built, and what is deliberately not.** Built: the protocol move,
the catch-all, the manifest on the verdict, the inline and streamed carriages
with the choice made by the runner off a walk that has only measured, the
spool as a **rename** out of the workspace rather than a copy, per-file
sha256 verified before a rename on the hub and again by the requester,
resumption from the hub's own holdings, the window that bounds bytes in
flight, and the manifest outliving the bytes. Not built, as the orchestrator
scoped it: the 200 GB budget, the sweeper, the eviction order and the line
when eviction breaks the seven-day floor. The manifest-outlives-the-bytes
path is nevertheless already exercised, because a file that never arrived
reaches exactly the state an evicted one will.

**What is still unproven.** The suite streams 32 MiB, not a few hundred, so
the consumer's real size has still never crossed a link. The event-loop claim
— that 1 MiB chunks keep a large transfer inside the 15 s cadence — is now
plausible rather than measured: nothing went quiet in these tests, and no
test asserts a health report's timing during a transfer. The window that
bounds bytes in flight is eight chunks and that figure is chosen, not
measured against a slow link. And a requester still assembles a collection in
memory before writing it: streaming removed the base64 blow-up and the
140 MiB-per-party frame cost, and it did not remove holding the payload once.
Writing each file to disk as it completes is the next slice, beside the
sweeper.

---

## 2026-10-16 — D151 The two ways a failure lies to the reader

**One real failure on the live deployment, two defects reported, four
instances found, and all four are one defect wearing different clothes.**
Job `j0000000057` — the first consumer's
`test web-firefox,unit -j 1` — ran while the host was at **load 306** with IO
pressure `full avg10` near **96 %**, none of it Herd's: another agent's GPU
QEMU and a Minecraft server. The hub lost the runner twice, E-21 spent its
one requeue, the job ended `lost`. Neither process died. The liveness
behaviour itself is a separate question for Alexa and nothing here touches
it: the 60 s threshold, E-21's single requeue and the stale-attempt rule are
exactly as they were.

What this entry is about is what the record *said* afterwards. **A failure
can lie to the person reading it in exactly two ways: by naming the wrong
cause, and by burying the right one under a thousand repetitions.** Both
happened in this one job, and both are now fixed.

**The first lie: a step that cannot start blamed the shell.** Attempt 2's
`down` step reported `this step could not be started: /bin/sh: No such file
or directory (os error 2)`, on a host where `/bin/sh` is a perfectly good
symlink into the Nix store. The real cause is the working directory:
`herd-runner/src/job.rs` spawns every step with `current_dir` set to the
workspace, and attempt 1's `down` template ends in `rm -rf
…/agents/herd-{job}`, so attempt 2's `down` was spawned into a directory its
own predecessor had removed.

The mechanism is proved rather than assumed, and the test that proves it is
`a_missing_working_directory_and_a_missing_program_fail_alike`. A child
`chdir`s into its working directory and *then* `exec`s, and a failure at
either point comes back as **the same error object**: `ErrorKind::NotFound`,
`raw_os_error` 2, and a `Display` string equal byte for byte. The error
carries neither path. So nothing in it could have told the runner which of
the two was missing — and the lie was not Rust's: it was Herd's own
`format!("{}: {error}", stage.program)`, pasting the program's name onto an
error that never mentioned it.

`why_a_step_could_not_start` now looks before it speaks. On `NotFound` it
asks whether the working directory is still there; if it is not, that is what
gets named, with its path, and the program is not mentioned at all. A
consumer reading their own `down` step can now see that their own `--purge`
removed the ground under it.

**The second lie: the runner floods the journal exactly when the host is
stalled.** After the link came back, the runner wrote *attempt 1 of
j0000000057 is over as far as the hub is concerned …* **1497 times**, mostly
inside the same millisecond. One line per buffered item, not one per attempt.

The hub is not at fault: a frame it will not record must be refused by name,
so it answers every chunk of a dead attempt with its own `StaleAttempt`. The
runner's fault was echoing every answer. 1497 journal writes onto a host
already IO-stalled at 96 % is **a runner making its host's stall worse at the
worst possible moment**, which is the exact opposite of what invariant 2
promises. It is said once per attempt now, gated on a bounded ring of
attempts already announced (`REFUSALS_REMEMBERED`, 32) that deliberately does
not live on the job: the first refusal releases the job, and every repeat
arrives after it, so the memory has to outlive what it is about. No trailing
count: the frames simply stop, there is no end to report one at, and a second
line to say the first line was right would be the same mistake smaller.

**The same shape, found twice more while looking.** `JobInHand::remember`
dropped one journal line per chunk the held-bytes cap evicted — and the cap
only bites while the link is down, which is the same moment and the same
host. A chatty job on a broken link therefore wrote a line per chunk for as
long as it ran, indefinitely, with no 1497 to bound it. It now says what the
cap is once and reports the total when the job is let go, which is a count
that is genuinely worth having: it says how much output was lost. And
`testing::skipped` wrote the skip log with `writeln!`, which puts the text
and the newline on the wire as two calls; two tests skipping at once
interleaved between them and landed on one line, so `scripts/test.sh`, which
counts lines, announced **12** skipped assertions where there were **13**. A
count of what was *not* checked that is itself wrong is precisely what D27
exists to prevent. One `write_all` per line fixes it, and the gate run for
this branch reports 13.

**And the hub had it too, independently.** `is_current` in `herd-hub` wrote
`herd-hub: {reason}` once per refused frame, so during the very same burst
the hub made its own ~1497 writes — into Docker's logging stack, which is if
anything the heavier path, on the same stalled host. The first scoping of
this work was "fix the runner", and that was wrong: the defect is *a
per-frame path emitting a per-frame line where the path is reached in bulk*,
and half a fix still drowns the next person to read a stalled host.

**The hub's memory is not shaped like the runner's, and should not be.** The
runner has one hub, so a ring in its `Inner` keyed on `(job, attempt)` is
enough. The hub serves every runner at once, so the same key would have two
runners silencing each other's refusals. Rather than putting the runner's
name *in* the key, `AlreadySaid` lives inside `relay_from_runner`, which
serves exactly one connection: the name is in the key implicitly, and the
bound gets a structural half the runner's ring cannot have — when the
connection ends the memory goes with it, with no eviction policy and no lock
on a hot path. It is still a ring (`REFUSALS_REMEMBERED`, 32) for the long
tail of a link that stays up for days. A reconnect buys one line per attempt
again, and that is right rather than a leak: a runner that dialled back and
is still flushing a dead attempt is a *new* fact about a flapping link.

The key carries the *kind* of refusal as well — superseded, the job is over,
another runner's attempt, the standing could not be read. Collapsing those
would be the opposite error to the one being fixed: an attempt that was
superseded and whose job then ended is refused for a genuinely different
reason the second time, and repeating the first reason for ever would make
the journal wrong instead of merely quiet. At most four lines per attempt,
one in practice. The refusal **on the wire** is untouched and still goes out
once per frame — that is what stops the runner resending, so it is E-21 and
not a logging question. The test asserts all three counts: one journal line,
1497 refusals, 1497 `StaleAttempt` frames.

**The lesson that generalises past this entry: a defect found on one side of
a protocol is worth looking for on the other.** That is twice in one day. The
`{j}` fill was the first — the runner's `{in}`/`{out}` handling and the hub's
`{j}` turned out to be the same defect sitting in two places — and this is
the second, with both ends of the link having grown the same per-frame line
without either one copying the other. Two implementations of one
conversation make the same mistakes about it, because the mistake is in how
the conversation is thought about rather than in either codebase. Looking
across is cheap; it has now paid twice.

**On testing a count rather than a sentence.** The defect was never what the
line said; it was that one true line was written 1497 times. A test asserting
the wording would go green again the day somebody reintroduces the loop
saying something else, so
`a_stale_attempt_is_announced_once_however_many_frames_it_buffered` counts
the **writes**: 1497 refusals of one attempt must leave one line. Measuring
that needed one concession from the production code. `eprintln!` consults
libtest's per-thread output capture — which is inherited by every thread a
test spawns, so there is no thread from which the macro can be made to reach
file descriptor 2 inside a test binary — and a line that cannot reach fd 2
cannot be counted. That path therefore writes through `journal()`, straight
to the descriptor, where `testing::stderr_lines_about` can divert and count
it. Same bytes, same place, and the invariant is now measurable rather than
merely asserted. With the gate removed the test reports `1497`, which is the
field failure reproduced exactly.

The hub's test is built the same way and proved the same way:
`a_burst_of_refused_frames_leaves_one_line_in_the_hub_s_journal` drives the
real `is_current` with a job the store has never heard of — which is exactly
what a written-off attempt looks like from the hub — and with the dedupe
removed it reports `1497` as well. `herd-hub` therefore has a `journal()` of
its own, a deliberate twin of the runner's rather than something shared: a
logging helper is not a protocol message, so it has no business in
`herd-proto`, and four lines in each binary is cheaper than a crate that
exists to hold them. The hub's unit tests reach
`herd_runner::testing::stderr_lines_about` through the dev-dependency on
`herd-runner` that its end-to-end tests already need.

---

## 2026-10-16 — D152 The output budget, and what order evidence goes in

**D150 shipped the carriage and left the half that makes it safe to
release: large outputs landed on the hub's disk and nothing removed
them.** The first consumer runs long jobs many times a day and wants a few
hundred MB per job, so an unbounded store fills the volume in days — on a
host whose IO is already its sore point (D136), whose hub shares a device
with the runner's data root (D126) and with the consumer's own tree.
Streaming without a sweeper is a disk-full outage with a delay fuse. This
entry is the sweeper, and Alexa's two figures carried out: **200 GB, with a
seven-day floor** (D148).

**What the budget counts, and the three things it deliberately does not.**
`--output-budget` bounds the **whole, verified collections** in every job's
`files/` directory, across the state root. Not the state root itself: a
record is kilobytes, a log has a cap of its own (`--job-log-cap`, D137),
and one number answering for three things that fail in three different ways
is a number nobody can reason about. Not part-written bytes either, and
that one is a choice with a cost — a transfer in flight is real on the disk
and outside the budget, so the volume may sit above it by one
`--job-output-cap` per connected runner while collections cross. The
alternative was evicting the thing the hub is in the middle of being given,
which is not an alternative. At two runners and the shipped cap that is
2 GiB against 200 GB; it is named rather than hidden, and the hub says so
when it is over budget with nothing it may take.

**The sweeper is triggered by an event and never by a clock.** The only
thing that can put a hub over its output budget is a collection arriving,
so a timer would be a second writer waking up on a quiet hub to find
nothing to do — and a second writer against D8's one-writer discipline is
exactly the shape the brief warned about. So the sweep runs where bytes
become whole: the last chunk of a streamed file, the inline write of a
small one, and once at startup. **Nothing is removed for merely being
old.** Seven days is a floor, not a lifetime: a collection from last year
is still there if there is room for it, because the only thing that makes
room worth taking is a shortage of it. That is a test
(`a_hub_inside_its_budget_removes_nothing_however_old_the_bytes_are`).

**And it does not walk the store on every verdict, because a cheap wrong
number decides when to look and an expensive right one decides what to
do.** A running total lives in the store's `Live` state and is added to
wherever a file becomes whole. It is a **trigger**: past the budget, a
sweep runs, and the sweep walks `jobs/*/files` and overwrites the total
with what it measured. So the figure may drift — a part file thrown away
for a bad digest, a directory an operator removed by hand — and the drift
costs a sweep that runs a little early or a little late, never a collection
deleted on a number nobody checked. Self-correcting by construction, which
is the property that let me put a walk in at all.

The cost of that walk is measured rather than argued, on this host, with
100 000 job directories of which 1 000 hold bytes:

```
sweep 1: 1.96 s   (cold)
sweep 2: 1.15 s
sweep 3: 1.18 s
collections_owed 1: 1.31 s   (cold)
collections_owed 2: 0.91 s
collections_owed 3: 0.87 s
```

**The second half of that measurement is a finding about code already on
`main`, and it is worse than the thing I was measuring.**
`collections_owed` reads **every record** in the store, and
`send_the_files` calls it on every fetch — including every 200 ms poll of a
`herdctl --outputs-to` waiting for a transfer. A 400 MiB collection
crossing at 100 000 jobs of history would spend roughly a second of hub
time per poll doing nothing but re-reading history. The sweep's walk is
cheaper than that and runs orders of magnitude less often, so this work
does not make the hub's scaling worse — but the poll does, it is D148's
own code, and it is written down here rather than quietly left. It is not
fixed in this branch: it is not what the branch is about, the fix is a
derived index with its own staleness question, and inventing one while
holding a sweeper is how two half-built mechanisms happen.

**The lock discipline, in the order the steps happen.** The walk is
deliberately **unlocked** — it only reads the disk, and holding the store's
one mutex across a second of `read_dir` would stall every log chunk of
every running job. Each eviction then takes the lock and does four things:
re-reads the record (because the walk was unlocked and the collection may
have been fetched, completed or already evicted since), **renames**
`files/` to `files.gone/`, writes the record and the job's history line,
and gives the lock back. The unlinking happens after that, with nothing
held: several hundred `unlink` calls on a loaded host is the one part of
this that takes real time and nothing is waiting on it.

The rename is D8's discipline run backwards. A write becomes visible by one
atomic rename; a removal becomes invisible by one. Without it a reader
arriving in the middle of `remove_dir_all` over a 560-file collection would
find four of twelve files and nothing saying so — a short collection that
reads like a whole one, which is the single failure this whole design
refuses. A `files.gone` directory left on the disk is a hub that was killed
between the rename and the unlink; the next sweep removes it, and the first
sweep runs at startup, so a crash costs disk until the next start and never
an inconsistent answer.

**The eviction order is the real decision, and oldest-first is wrong in
exactly the way that matters.** On its own it deletes last week's one red
test to keep this morning's thousand green ones — and the red one is the
reason the consumer asked for any of this: *"results.json alone is not
enough to diagnose a red test."* **LRU is worse, and backwards.** A cache
evicts what nothing has touched because untouched means unwanted; here a
collection nobody has fetched is evidence **nobody has seen**, while one a
requester read yesterday is already on that requester's disk. The classic
answer has the sign flipped.

So the order is four keys, and the first outranks the other three:

1. **outside the floor before inside it.** Seven days is a promise, so
   nothing inside it goes while anything outside it remains — even a
   failing collection from last month before a passing one from this
   morning. That is the one place the outcome does not win, and it is what
   makes the floor a floor rather than a preference.
2. **a job that passed before a job that did not.** Green artefacts are
   the ones nobody ever opens.
3. **fetched before never fetched**, for the reason above.
4. **oldest first**, which is only the tie-break — the only key two
   identical jobs can differ on.

Four keys is more machinery than two, and it is still **one order with no
knob**: an operator sets a size and a floor, and nothing anywhere lets one
job's evidence be declared more important than another's. A per-job
retention class would be the first line of a policy language and invariant
1 refuses it. What is kept is asserted at least as hard as what goes —
`the_sweeper_takes_a_read_green_collection_before_an_unread_red_one` names
four survivors and one victim, because a sweeper that freed more than it
had to would be a worse defect than one that freed nothing at all.

Two signals had to become durable for this. `first_fetched_at` on the
record is written **once ever**, on the first fetch that really handed
bytes over: the sweeper only needs the boolean, and a last-read stamp would
be a record write per poll on a path that already polls. The outcome was
already there; `JobOutcome::is_success` is now one function instead of the
two places that had written `Exited { code: 0 }` out (D79).

**When the floor and the budget disagree, the budget wins and the hub says
so on the day.** It is the only honest answer: the floor is a promise about
time and the budget is a fact about a disk, and a hub that honoured the
promise would hand the operator a full volume instead of a sentence. So
eviction continues into the floor, the per-record reason says how far
inside it the collection was, and one line on stderr names both figures and
what just happened. D148 asked for *"seven days, unless the fleet produces
more than the budget in less than seven days, in which case we tell you on
the day"*, and that is the sentence. A quiet trim here would be the trap:
the next thing after evidence disappears with no reason is somebody
concluding Herd lost it.

**One line per sweep, not one per collection** — D151 was yesterday and its
lesson is a day old: a path reached in bulk must not emit a line per item.
A sweep that frees fifty collections is one journal line. What is
per-collection is the **record**: each evicted job's `job.json` carries the
sentence and its `events.jsonl` carries an `outputs_removed` line, which is
where "what happened to this job's screenshots" is answered a week later,
beside the manifest that still names them. The over-budget-and-nothing-to-
free complaint is said once per episode and cleared when a sweep comes back
inside the budget, because that path is reached by every file that lands.

**The manifest outliving the bytes was already true, and the eviction path
would have broken it in three places.** D150 built it and a file that never
arrived already reached the state an evicted one does, so the carriage
needed nothing new — but an *evicted* collection is not an unfinished one,
and three pieces of code could not tell them apart:

* `collections_owed` judges "still owed" by the collection not being whole,
  which is exactly true of one the sweeper took. The hub would have asked
  its runner to resend the bytes, swept them again at the next verdict, and
  looped — two correct mechanisms fighting.
* `files_may_land` would have accepted those resent chunks, refilling the
  disk the sweep had just freed.
* `AbsentFile::still_coming` would have said `true`, so a requester would
  have waited out D150's thirty-second stall bound and then been told
  something worse than the truth.

All three now read `outputs.removed`, and the requester gets the sentence
the record carries: *12 files, 338 MiB, removed 2026-10-30 under the
200 GB output budget*, with the names under it. The count-and-size line is
`herdctl`'s and fires only when every absence shares one reason — two
reasons are two facts, and a tidy sentence over them would hide one.

**Three things in the brief or in D148 that turned out to be wrong, said
plainly.**

1. **D148 spells the figure both ways.** Alexa's ruling says *"200 GB"*;
   the example sentence four paragraphs earlier says *"the 200 GiB
   budget"*. They differ by 7.4 %. The ruling is the ruling, so the default
   is 200 GB decimal — and `show_bytes` grew the decimal family so that a
   figure typed as `200GB` reads back as `200 GB` and not as
   `200000000000 bytes`, which is how somebody concludes the hub took a
   number they did not mean.
2. **"Shown as use against budget in M13's status document" cannot be
   satisfied yet** and is not: M13 does not exist. What exists is the
   startup line and the per-sweep line. The figure is on the store and
   ready for M13 to read.
3. **The byte-size parser had to move.** `parse_bytes`/`show_bytes` lived
   in `herd-runner`, and the hub cannot depend on it. D151 was right that a
   logging helper has no business in `herd-proto` — but a **unit table** is
   not a logging helper: a figure an operator sets on one side of the link
   and reads back from the other is precisely a thing the two sides must
   agree about, and two copies is how `500MB` comes to mean two numbers. It
   is `herd_proto::size` now, re-exported from `herd_runner::size` so no
   runner command line changed.

**What is unproven.** The 1.1 s walk is one host, one filesystem, warm
cache, and 100 000 directories is a guess at what a year looks like rather
than a measurement of one. No sweep has ever run against a few hundred MB
of real collection — the end-to-end case evicts 4 KiB — so the unlink cost
of ~560 files is still arithmetic. A hub whose budget is smaller than one
collection walks on every completing file and frees nothing; it is bounded
to one journal line and is pathological at 200 GB, but it is a real shape
and nothing measures it. Part-written bytes of a collection whose runner
never comes back are counted and can never be evicted, so a fleet that lost
runners mid-transfer would drift above the budget by that much with no way
down but an operator; the figure is reported and the gap is not closed.
And the floor has only ever been crossed in a test that set it to seven
days and wrote three kilobytes — no real seven days has passed anywhere.

---

## 2026-10-18 — D153 The hub's image goes to Docker Hub on the tag

**The hand step this removes has blocked a deployment twice, and it was
the only part of a release that a tag could not reproduce.** Everything
else about v0.3.0 comes out of `.github/workflows/release.yml`: three
static musl binaries, proved by running a real hub, a real runner and two
real jobs inside an `archlinux` and an `alpine` container, a `SHA256SUMS`,
and `deploy/release-notice.md` quoted into the body. The hub's **image**
was built by the same flake and went nowhere — somebody built it on the
server and `docker load`ed it. Now the tag pushes it, and
`deploy/docker-compose.yml` names what was pushed.

**From the flake's archive and nowhere else.** There is no `Dockerfile`
and there will not be one. `packages.<system>.hub-image` is the single
derivation that has ever built this image; a second way to build the hub
is how the thing deployed stops being the thing tested. The workflow does
`nix build .#hub-image` and pushes the archive it gets.

**`skopeo copy`, not `docker load` then `docker tag` then `docker push`.**
Three reasons, in the order they weigh:

1. **The source is a file, not a name.** `docker load` puts the image in
   the host's shared image store under the tag the archive carries, and
   the push that follows names a *tag*. On a machine where that tag
   already means something — which is exactly the server — the step can
   push something that is not what was just built. `skopeo copy
   docker-archive:hub-image docker://…` names the archive, and there is
   nothing else it could send.
2. **The manifest survives.** `dockerTools.buildLayeredImage` writes its
   layers already compressed, so the blobs in the archive hash to the
   digests in its manifest and skopeo pushes them untouched. Measured
   here: `skopeo inspect docker-archive:` on the archive reports
   `sha256:9cfbdbd8a22f3d97f50510499b5cb4c334d9a7e450af113df75289cec04fb7d9`,
   and a copy out of it writes that same digest to `--digestfile`. The
   digest a release prints is therefore a property of the flake output,
   not of whatever a daemon did to it on the way.
3. **The token never lands on an argv or in `~/.docker/config.json`.**
   `skopeo login --password-stdin --authfile` into a temporary file of
   the job's own, removed on the way out. `--dest-creds user:token` would
   have been one line shorter and readable out of `/proc` (invariant 7).

`--insecure-policy` is on both calls, and the word is misleading: it means
"consult no containers signature policy", which is what `docker push`
also does, and it says nothing about transport — TLS to Docker Hub is
untouched. Without it skopeo refuses to start on a host with no
`/etc/containers/policy.json`, which is how it was found: this project's
own server has none.

skopeo comes from the flake, as `devShells.<system>.release`, and not from
`apt` in the workflow. Same argument D12 makes about every other tool: the
thing that pushes a release should be pinned like the thing that builds
one. It is not in the default shell; nobody developing Herd needs it.

**No `latest`, and the reason is this release in particular.** Herd has
exactly one hub and a fleet that speaks to it, and `PROTOCOL_VERSION`
moved from 1 to 2 in 0.3.0 — a version-2 hub refuses a version-1 runner
*by name*, which is a hard stop for anything with no fallback. A floating
tag means the next `docker compose up -d` can carry the hub across that
line at a moment nobody chose: a reboot, a host upgrade, an unrelated
`pull`. Naming the version is what keeps a protocol move a decision
instead of an accident. There is no argument on the other side worth the
trade here — `latest` buys a shorter command for a deployment that
happens a few times a year and is read out of a release notice anyway.
A digest is accepted everywhere the tag is, and is stronger: a tag is a
name somebody can move, a digest is the bytes.

**What Alexa has to create, exactly.** Three things on Docker Hub, one
repository **variable** and one repository **secret**, and nothing else.
The last two are on two different tabs of one settings page, and which
tab is part of the instruction rather than something to work out:

| Where | What |
| --- | --- |
| Docker Hub | an account, if there is not one. Its name becomes part of every image reference a deployer types |
| Docker Hub | a **public** repository named exactly `herd-hub` under that account. Public, so that `docker compose pull` on the server needs no credentials at all |
| Docker Hub | an **access token** (Account settings → Personal access tokens), scoped to **that one repository**, permission **Read & Write**. Not the account password, and not an account-wide token |
| GitHub → Settings → Secrets and variables → Actions → **Variables** | repository variable `DOCKERHUB_USERNAME`, the account name |
| GitHub → Settings → Secrets and variables → Actions → **Secrets** | repository secret `DOCKERHUB_TOKEN`, the access token |

**The namespace is a variable, and that is not a detail.** It is in every
image reference a deployer types — the compose file, the release body,
`env.example`, the `docker pull` somebody runs at two in the morning — so
it is public the instant anything is pushed under it, and holding it in a
secret would have GitHub mask it in every log line this workflow writes.
That is the part that matters: a push that fails prints skopeo's own
error text, and `docker.io/***/herd-hub: unauthorized` names neither the
account nor the repository it was refused on. A masked log is a release
nobody can diagnose, which costs more than the nothing the secrecy buys.
The token stays a secret; it is the only thing here that is one.

**A missing credential stops the run before anything exists.** The first
step after the version is read checks both and, if either is empty, says
which one is missing, **which tab it goes on** — Variables for the name,
Secrets for the token — what the token's scope must be, why the name is
not a secret, and the sentence "nothing of this release has been
published", then exits 1. It runs before `nix build`, before the
binaries, and a very long way before `gh release create`. Names only, no
values, and no `set -x` anywhere in the file: GitHub masks a secret it
recognises, and a workflow should not be relying on that.

**A release stays one thing, and the order is what makes it so.** One
job, not three: the binaries, the image and the release are one release,
and steps in one job run in an order a reader can see. The order is
binaries → prove them on Arch and Alpine → upload them to the run →
build the image → prove it → push it → pull it back and prove that →
*then* create the GitHub release. So a tag can never leave binaries
published and an image missing. The one partial state it can leave is the
reverse — the image on Docker Hub and no release — and that one is both
understandable and re-runnable: re-running the tag pushes identical
content to the same tag, which is a no-op the registry accepts, and then
creates the release. The pre-existing `gh release view` branch already
makes a second run idempotent on the release side.

**Verified the way a downloader would, by pulling it back.** After the
push the workflow deletes the local tag, pulls the image **by digest**
from Docker Hub, and runs `scripts/image-smoke.sh` against what the
registry served: a hub in the container shaped the way
`deploy/docker-compose.yml` shapes it — read-only root, a volume on
`/var/lib/herd` and nothing under it, `HERD_STATE_DIR` naming that same
path, the image's own non-root user — with the static runner and
`herdctl` on the host dialling the published loopback port, and two real
jobs through it, the second checking the verdict is worn untouched. That
is the image sibling of `scripts/release-smoke.sh`, and it runs twice:
once on the locally loaded archive, which needs no credentials and so
happens on every hand-run too, and once on the pulled digest.

**The deployment uses it, through one variable, and that is the one place
this deviates from the brief.** `deploy/docker-compose.yml` cannot name a
Docker Hub account the repository does not know — the account lives in the
`DOCKERHUB_USERNAME` variable, which a checked-out tree cannot read, and
inventing a namespace in a compose file is how a deployment comes to
point at somebody else's image. So the compose file takes
`${HERD_HUB_IMAGE:?…}`, **required** and not defaulted, because a hub
starting quietly on the wrong image is worse than a compose file that
stops; `deploy/env.example` spells out the value, where the namespace
comes from, that a digest is accepted, and why there is no `latest` to
point at. The same variable is how the local build still works —
`HERD_HUB_IMAGE=herd-hub:0.3.0` after `docker load` — which is why the
registry path and the offline path are now one line apart instead of two
compose files. `pull_policy: never` is gone, so `docker compose pull &&
docker compose up -d` is the upgrade.

**What is still masked, now that the namespace is not.** GitHub masks
the value of secrets and of nothing else, so with the account name moved
to a variable the only masked strings left in this workflow are
`DOCKERHUB_TOKEN` and the implicit `GITHUB_TOKEN`. Neither appears in
anything a reader needs: the pushed digest is printed in full, skopeo's
own error text carries the registry, the repository and the tag intact,
and `gh release create` prints the release URL. So a failed push is
readable end to end.

It is not, however, always *informative*, and that is a different
problem with no fix here: Docker Hub answers a bad token with `401
unauthorized: incorrect username or password` even when the name is
right, and answers a token whose scope is too narrow with `403 …
requested access to the resource is denied`. Both lines arrive unmasked
and both now name `docker.io/<account>/herd-hub`, which is the whole
gain — but the first sentence blames the username, and it is the token
that will be wrong. Read the code, not the sentence. The login runs as
its own step before the copy, so at least which of the two failed is
never in doubt.

**What is unproven, and it is the half that matters.** None of this has
run. There is no Docker Hub account yet, no variable, no secret and no
tag, so the push, the login, the digest file, the pull-back and the
second smoke run exist only as YAML — and D132 is the entry that says a
release step nobody has run is a release step that does not work. The
preflight has never run either, which is worth saying twice because it
is the step whose whole job is to fire on a day nothing else works.
What *was* proved, locally, on this host:

* the flake's archive carries exactly `herd-hub:0.3.0` in its
  `manifest.json` `RepoTags`, which is what the new assertion step
  compares against the workspace version;
* `skopeo` reads that gzipped `docker-archive:` without being told it is
  compressed, and a copy out of it preserves the manifest digest, as
  above;
* `scripts/image-smoke.sh` passes against the real flake-built image —
  containerised hub, host runner, two jobs, the second exiting 7 and
  `herdctl` exiting 7;
* editing the comment at the top of `nix/hub-image.nix` leaves the
  derivation at the same store path, so nothing about what the image
  contains changed in this work.

Two things could not be checked at all without the credentials: that
Docker Hub accepts a `buildLayeredImage` manifest as skopeo writes it
(no reason it should not; it is an ordinary Docker v2 schema 2 manifest),
and that a token scoped to one repository is enough for `skopeo login`
against `docker.io` — a repository-scoped token authenticates fine and
is only refused at the point of writing somewhere it does not cover, but
that sentence is read and not measured. One more is only half checked:
`skopeo copy` into `docker-daemon:` fails on this host with `writing
blob: io: read/write on closed pipe`, which is why the local proof above
went through a re-tagged `docker load` instead. The workflow does not use
that transport — it loads with `docker load` and pushes with
`docker://` — so it is a note about this server, not about the release.

---

## 2026-10-18 — D155 The served floor, and a test that hoped to clear it

**`main` went red on the test again, not on the product.** CI's `check`
job failed on a documentation-only pull request, in the `herd-hub`
integration binary `killed_hub`, on
`a_connection_that_served_starts_the_redial_counters_over`:

```
panicked at herd-hub/tests/killed_hub.rs:1747:5:
a runner that had been serving must redial as if this were its first failure,
not carry on from the four dials that preceded it:
herd-runner: dialling ws://127.0.0.1:44583 again in 46.669602ms (attempt 5)
```

Same test as D146, different line and a different cause, so it is written
down separately rather than filed under the fix that is already in. The
product is right; the test asserted a property it had not established.

**What the test was missing.** `Backoff::settled` resets the redial
counters only for a connection that lasted at least the cap, and the cap
is the whole argument: a hub that accepts and dies in a loop must not be
answered every `first` for ever, and the give-up bound must still fire on
a hub that is broken rather than absent (D104). The test passes
`--redial-cap 0.05`, so **the floor it has to clear is 50 ms** — and
nothing in the test held the connection open that long. It waited for
`wait_for_runner`, which is a `printf` over in a millisecond, read the
log, and killed the hub. The span from the hub's welcome to the runner
noticing the reset was whatever the host happened to make it.

**Measured, by printing the span the runner itself judges.** With a
temporary line in `serve_forever` reporting `served_since` and the cap at
every redial decision, under
`systemd-run --scope -p IOWeight=20 -p CPUWeight=20 -p AllowedCPUs=0-1`,
six runs of the whole binary on two cores gave 65.5, 77.2, 78.1, 82.1,
173.9 and 298.8 ms against the 50 ms floor. Eight runs of the binary at
`--test-threads 18` on twelve cores gave 71.9 to 436.1 ms. **The smallest
margin seen was 15 ms**, on exactly the two-core shape CI runs. That is
not a test with a property, it is a test with a coin.

**The failure reproduces on demand, word for word, by moving the floor
rather than the host.** Raising the test's cap to `0.5` so that the floor
sits above the span the host was going to produce gives:

```
herd-runner: DEBUG served=Some(305.71204ms) cap=500ms settled=false failures=3
…
a runner that had been serving must redial as if this were its first failure,
not carry on from the four dials that preceded it:
herd-runner: dialling ws://127.0.0.1:37711 again in 319.95527ms (attempt 5)
```

which is CI's panic in the same shape, including the `(attempt 5)` — the
counter reaching 4 because the three dials before the hub came up were
never spent. The arithmetic checks out too: `(attempt 5)` is the *only*
number a missed reset can print here, since the test waits for
`(attempt 4)` and the hub comes up straight after it.

**Not reproduced: the natural crossing.** 25 runs of the single test and
16 of the whole binary under the low-weight scope, most of them on two
cores, never put the span under 50 ms. So the mechanism is proved and the
figures are measured, but that CI's run crossed the floor the natural way
is read off CI's own line rather than seen here. Nothing else explains
`(attempt 5)`: the slice the test takes cannot produce it, because
`climbed` is read before the kill and the file only grows — the boundary
in the brief's second hypothesis is sound, and it is the floor that is
not.

**What a user would have seen, had it been the product.** Nothing, this
time: the reset fired at 65 ms where the floor is 50 ms, as specified.
The thing worth keeping an eye on is the floor itself at its shipped
value. The cap in production is 30 s, so a link that cannot hold a
connection for 30 s never resets anything, and a runner on it walks the
24 h give-up bound and **exits** while it is still carrying jobs between
drops. D104 chose that on purpose — it is the same sentence that stops an
accept-and-die hub from being hammered — and it is not changed here. But
the WSL2 PC, behind a NAT on a laptop-grade link, is the first machine
where "every connection shorter than thirty seconds" is imaginable, and
if it ever gives up with jobs behind it, this paragraph is where to
start. Changing the rule is Alexa's call, not a test fix.

**The repair: the premise is made true, and the assertion is about the
first redial.** Two changes, both in the test.

1. **A job that sleeps six floors.** After `wait_for_runner`, the test
   runs `sleep 0.3` through `herdctl run` and requires it to succeed. The
   job cannot start before the welcome, cannot report before it has
   slept, and is answered before the kill, so the connection's life is
   bounded below by a sleeping child's own clock — the one figure a
   starved host can only make longer. Six times the floor instead of the
   1.3 that was there.
2. **The first redial after the kill, not any line in the slice.** The
   hub is dead, so the storm that follows counts up through 3, 4, 5 and a
   search of the whole slice would eventually find whatever number it was
   looking for; only the first announcement carries the counter. It is
   matched on whole lines beginning `herd-runner: dialling `, so a read
   that caught the file mid-write cannot leave half of an earlier line
   looking like the one under test.

**The repaired test still fails on the defect it exists for.** With
`Backoff::settled` forced back to its pre-D104 answer — never settled —
the test fails with the same sentence and the same `(attempt 5)`. A test
that cannot fail would have been the worse outcome of the two, so this
was checked rather than argued.

**One thing seen and not diagnosed.** In 25 runs of the single test
pinned to two CPUs on a host at load average 40, one run failed
differently: the runner printed `joining ws://…` and then nothing at all
for the full 30 s patience, so the first wait timed out before any dial
was announced. On loopback a dial at a port nobody holds is refused in
microseconds, so a 30 s silence is a stall somewhere, not a slow refusal
— but it was seen once, under starvation harsher than CI's (two shared
CPUs against CI's two dedicated ones), and it is not this entry's
failure. Written down so the next reader of a `killed_hub` timeout has
it.

---

## 2026-10-18 — D156 The dedup key carries the commit a declaration names

**Context.** E-05 joins a second submission of the same work, and D69
fixed what "the same work" means: the command and the labels, counted.
That was right for the only kind of submission that existed then, the
ad-hoc one, whose command line contains whatever revision it names — so
keying on the line keyed on the commit as a side effect. `dedup_key`'s
own comment said as much: "the commit will join it when a submission
names one (E-01, M10): it is part of the command's meaning, and today
the command is where it is."

M10 made that sentence false and nobody came back to it. A declared
submission keeps its commit in `Declaration::commit`, a field of its
own, and its `command` is the resolved line — which need not mention
the commit at all, and usually does not, because `{commit}` goes into
the executor's `up` and `fetch` steps. A declared line that
interpolates nothing commit-shaped is therefore byte-identical at every
commit in the repository, and every submission of it joined one job.

**The defect, which is the worst class: a wrong answer presented as a
right one.** A requester asked Herd to check commit A and was handed
commit B's verdict, silently, with nothing in the output, the record or
the logs saying anything had been substituted. Confirmed on the live
hub's own disk:

```
j0000000098  "submissions":4
  "command":["cargo fmt --check && CARGO_BUILD_JOBS={j} cargo xtask clippy"]
  "commit":"7bc3795600b41c148636cc6a5ac8d540c180ce54"

j0000000146  "submissions":2
  "command":["cargo fmt --check && CARGO_BUILD_JOBS={j} cargo xtask clippy"]
  "commit":"bc78ebdc277b9ac7380d6ae0bfc9a71ca5d2ee4d"
```

Four requesters joined j98. They had asked about four different
commits and all four were given 7bc3795's verdict, from one execution,
with identical output in every log.

**The exposure is wider than the first consumer believes.** They
reported it against their argument-less command and think only that one
is affected. It is not. Their command with arguments escaped only
because the *arguments* differed between the two runs they happened to
compare: `{args}` is filled into the line before the job is minted
(D107), so two different scenarios are two different lines and two
different keys — but two submissions of **one** scenario at two commits
produce the same line, the same arguments and the same key, and collide
exactly as the argument-less one does. Every declared command whose
resolved line does not happen to carry the revision is exposed, which
is the normal case and not the exception.

**Choice.** The key is the command, the labels, **and the commit a
declaration names**. The commit is written as one more counted field,
`length:bytes`, after the labels — D69's encoding, for D69's reason: a
commit pasted onto its neighbour could make a shorter line with a
longer commit equal to a longer line with a shorter one. An **ad-hoc**
submission has no declaration, so no commit field is written at all —
not an empty one — and its key means exactly what it has always meant,
because its line really does carry whatever revision it names. A
declared key is an ad-hoc key with one field appended, so the two can
never be equal.

What does not change: the priority stays out of the key, and the higher
level still wins on a join (E-05); the labels stay in it, canonical
from `LabelSet`; two submissions of one declared command at one commit
are still one execution, which is E-05's whole value — a push hook
firing twice must not run the work twice — and a fix that made every
key unique would have been no fix at all.

**No migration, and that is a claim, not an omission.** The key is
*derived*, never stored: D69 put it that way on purpose, and `job.json`
has no key field, nor has the index file, which carries only the
version, the job count and the pending ids. The open-key map is rebuilt
from the records at every start, so this change alters what is computed
from a record and not what is written in one. Every existing record
already carries its whole `declaration`, so it keys correctly under the
new rule with no field added, no default and no bump of
`RECORD_VERSION`, which stays 1 (D73). `PROTOCOL_VERSION` does not move
either: the key is hub-internal and no message carries it.

The records that merged wrongly are still wrong, and nothing here
rewrites them: a joined record says it answered N submissions and
cannot say which commits those requesters asked about, because that was
never written down. They are over; the next submission of any of those
commits mints and runs (D70).

**The general lesson, which is why this entry is long.** *A field that
moves out of a string takes its meaning out of every key built from
that string.* Nothing failed to compile when M10 gave the commit a slot
of its own; no type changed, no test went red, and the key simply
stopped distinguishing something it had been distinguishing by
accident. The comment that recorded the accident — "today the command
is where it is" — was the only warning, and it was written as a
reassurance rather than as a tripwire. Before giving a submission a new
field, ask whether it used to be part of a string some key already
reads; and when a comment says a property holds "today", give it a test
rather than a sentence.

**Tests.** Unit: the key over a declared submission whose line says
nothing about the commit, one commit against two, arguments against
commits, an ad-hoc line against a declared one, and a byte moved from
the line into the commit. Store: two declared submissions at one commit
mint and join, at two commits mint twice, and a hub that restarts
rebuilds the link keyed on the commit. End to end, through the real
`herdctl` binary, a real hub, a real runner and the four executor steps
— `herdctl/tests/declared_dedup.rs` — two submissions of one declared
command at two commits, each caller told about its own commit by the
fixture's own `fetch` hook; and the same two at one commit, one
execution answering both. Both end-to-end tests hold the first job
inside `run` on a gate the test opens, so the overlap the question is
asked in is arranged and not raced: without the gate they would pass
against the very defect they exist for.

**Alternative rejected.** Keying on the whole `Declaration`, executor
hooks and environment included. It would catch the commit, since the
resolved hooks carry it, but it would also make a job that differs only
in a hook's wording a second execution — and the declaration is read
from the requester's working tree, so an uncommitted edit to the TOML
would silently stop a push hook's two firings from merging.

---

## 2026-10-18 — D157 The latency target is not what stalls this host

**The first consumer proposed loosening `io.cost.qos` from a p95 of 5 ms to
10-20 ms, and the measurement says it would change nothing.** Their
reasoning was sound: iocost cuts everyone's rate when the device's p95
crosses the target, a DRAM-less 980 crosses 5 ms in bursts, and they see IO
pressure at 95-99 % for a minute at a time. Every step of that is plausible.
It is also, on the evidence, not what is happening.

**What was measured.** `cost.vrate` from `/sys/fs/cgroup/io.stat` beside
`full avg10` from `/proc/pressure/io`, every two seconds, 784 samples across
26 minutes of ordinary work on the server — builds, QEMU, the usual
neighbours. `vrate` is the percentage of the cost model the controller is
currently granting: at 100 it is granting exactly the model, below it is
throttling, above it has decided the device is faster than modelled.

```
quiet   (full avg10 < 5 %):  mean vrate 95.4   over 524 samples
stalled (full avg10 > 50 %): mean vrate 96.5   over  20 samples
```

**The two are indistinguishable, and the sign is backwards from the
hypothesis.** During the two worst episodes — pressure climbing to 78.67 %
and to 86.22 % — vrate sat at 104.80 and 91.60, which is the controller wide
open. The *lowest* vrate in the whole run, 69.02, happened at 4.21 %
pressure: it trimmed while nothing was stalled. Across the run vrate ranged
69 to 148 with no relationship to pressure at all.

**So the stalls are processes waiting on IO the device is already doing, not
iocost refusing to let them have it.** Loosening the target would tune a
knob that is not engaged, and the minute-long stalls would come back
unchanged — while latency grew for everyone the rest of the time, which is
what a looser target actually buys. D136 asked for the target and D135 built
the hard ceiling; neither is the thing to move next.

**What this does not establish.** No sample landed inside one of the
95-99 % minute-long spikes the consumer reports — the worst seen here is
86 % for about twenty seconds. If vrate collapses toward the `min=1.00`
floor *there* and only there, the earlier suggestion stands: raise the
floor, which bounds how deep a cut can go without letting latency grow all
the time. The sampler is cheap and the next heavy window settles it. Until
then this entry says what 784 samples say and not more.

**The method is the part to keep.** Both sides of this had a mechanism that
sounded right, and two messages of argument had already been spent on it. One
`awk` over a log settled it in favour of neither. When the next host
question comes with a plausible story attached, measure the thing the story
is about before agreeing with it.

---

## 2026-10-19 — D158 A send window that ran down, and a spool that stays

**The first consumer's chained jobs stopped bringing their outputs home on
the night of 2026-10-19.** The build ended `pack result: ok`, the record
carried the full manifest — path, length and sha256 for every one of
hundreds of files — and `jobs/<id>/files/` on the hub was empty. `herdctl
wait --outputs-to` reported *"did NOT get … the hub does not have it"* for
every path, and the consumer's wrapper failed with `no pack came home`.
Jobs `j214`, `j217`, `j218`, `j219`, `j220`, `j222`, and in fact `j209`
before any of them.

**Two defects, and the obvious link between them is not real.** The
chronology said the outage began right after the one job that logged a
failed spool removal, and that reading was wrong: the removal failure had
been happening all evening, nine times, including under every collection
that worked. It is a real defect and it is not this one.

### The outage: one permit per finished collection, never given back

A streamed collection (D148) is sent chunk by chunk through a window of
`FILES_IN_FLIGHT` = 8 permits held on the link. A permit is taken before a
chunk is read and given back by `write_frames` once the chunk is on the
wire. The pump's loop took a permit, then asked the session what to send
next, and **when the answer was "nothing, it is all sent" it returned
without the permit** — `acquire().map(|permit| permit.forget())` forgot it
at the moment of acquisition, so there was nothing left to drop.

So every collection that completed cost the window one permit for ever. The
window is per connection, which is what made this look like a time bomb
rather than a leak: the runner joined the hub at 22:21 UTC, eight
collections were acknowledged — `j190`, `j193`, `j188`, `j198`, `j196`,
`j204`, `j207`, `j211` — and the ninth, `j209`, arrived at a window of
zero and waited on `acquire()` for ever. Everything after it queued behind
the same closed window. Not one chunk of `j214` ever reached the hub: its
`files/` was empty and it had no `parts/` directory at all.

**Fixed by holding the permit and forgetting it only once
`Session::send` has queued the frame `write_frames` will hand it back
for.** Every other way out of the loop now drops it, and dropping returns
it.

### Why the suite did not catch it

**Every streamed-collection test ran exactly one collection and then threw
its hub and its runner away.** `carriages.rs` built a fresh `Harness` per
journey; a window that is whole at the start of every connection is whole
for the only collection any test ever ran on it. A leak that costs one
permit per *success* is invisible to a suite whose longest link carries one
success.

The regression test is two collections in a row on one hub, one runner and
one link, with the window set to 1 through the new
`Config::files_in_flight` — the same injection `Confinement::output_cap`
already uses, for the same reason: at the shipped figure of eight the test
would cost nine real transfers of over 32 MiB. It fails on the code that
shipped. Beside it, a unit test asserts the property directly at the real
figure: **a collection that has gone through gives back everything it
borrowed.**

### The silence, which is its own defect

Nobody said anything. The runner waited on a semaphore; the hub had no
error to report because nothing arrived and no redial happened to make it
notice; the requester was told only what it did not get. An hour of an
operator's time went on establishing a fact the runner knew the whole time.

The runner now says it, because the runner is the only party that can: the
hub sees an absence and the requester sees a short collection, and only the
sender knows it is waiting for room. A pump that cannot get a permit within
`FILES_STALL_AFTER` — fifteen seconds, under `herdctl`'s own thirty-second
stall bound, so the line is already in the journal when the requester gives
up — writes one line per collection naming the job and saying that the
collection is fine and the window is not opening.

### The second defect: a spool the runner may not remove

`herd-runner: the hub has every file of j0000000211 and
/mnt/ssd_980/herd-runner/outputs/j0000000211-1 will not go away: Permission
denied (os error 13)` — on a tree whose every directory is `drwxr-xr-x` and
whose top the runner owns. `find … -type d ! -perm -u=w` over the whole
spool returns zero, which is what sent the first reading of this down the
wrong path: `-perm -u=w` tests the *owner's* write bit, and the owner is
not the runner.

Under `j0000000211-1/packs/` the directories are `drwxrwsr-x server users`,
written by uid 1002; the runner is uid 986 and is in no group but its own.
**Unlinking a name is a write to the directory that holds it**, so a
directory the runner did not create is a directory the runner cannot empty,
whatever its mode says — and it cannot `chmod` or `chown` its way out,
because both are the owner's to do. The consumer's job moves a
content-addressed pack tree, prepared beforehand by their own tooling, into
`{out}`; `{out}` is renamed into the spool by D148's carriage, ownership
and all. 7.4 GB of spool had accumulated across sixteen jobs.

Herd cannot remove those bytes and will not pretend to. What it can do is
stop costing an hour to diagnose: the message now walks the spool, names
the shallowest directory the runner does not own and its uid beside the
runner's own, and says that the bytes are safe on the hub and that what is
left is this runner's copy. No behaviour changes — the spool is still not
removed, and the collection it belonged to was delivered.

### What to keep

**A resource that is returned on the happy path must be tested over more
than one happy path.** Both halves of this entry are things that worked
perfectly the first time: the window carried eight collections before it
closed, and the spool removal failed on a tree that had already been
delivered. A test that runs one of anything cannot see either. Where Herd
holds a per-connection or per-process resource, the test to write is the
*second* use of it, and the suite now has one for the file window.

No protocol change: `PROTOCOL_VERSION` stays at 2 and the fix is entirely
inside the runner.

---

## 2026-10-19 — D159 A requirement reading is matched, never counted

**The incident.** The server runner's `build` was set to one slot. A
`labels_from` reading is minted as a job carrying the declared command's
labels (D74), so it had to take that single slot — and it queued **46
minutes** behind a long build, while **thirteen** jobs whose requirements
that reading answers queued behind the reading. One convoy, thirteen jobs,
out of one job that builds and boots nothing. The mitigation was to put
`build` back to two, which is a revert and not a fix; the consumer wants one
build at a time, and this is what lets them have it.

**Alexa's ruling.** A reading is matched on the declared command's labels and
**does not consume the counted slot**. One rule about one kind of job, not a
scheduling option (invariant 1): there is no new knob, no new setting and no
policy anywhere in this.

**The distinction, which is the whole of the entry: eligibility is not
availability.**

A reading must still land only on a machine that *could* run the command. It
reads that machine's capabilities and answers with figures the real job is
then placed on, so a reading on the wrong machine answers with figures that
mean nothing — the worst failure available here, because nothing about it
looks wrong. So a reading is matched on what a runner **offers and denies**,
exactly as any job is. It is matched on **nothing that is about contention**,
because a `--labels` line takes none of it.

The two halves were already told apart in the code, which is why this is a
narrow change rather than a new concept: `HeldUp::clears_with_time` and
`Contention::clears_with_time()` have said since M5 and M7 which obstacles are
about the *machine* and which are about the *moment*. A reading is held to the
first and not the second.

### The three answers, each with its reason

**1. A counted slot: not consumed and not waited for.** The whole point of the
change. A slot is contention and only contention; a reading holds none and
`Holdings::holders_of` does not count one. Both directions are tested: a
reading is claimed on a machine whose only slot is taken, **and** a reading
already running does not stop a real job taking the slot it would have held.
That second half is where the forty-six minutes were.

**2. An exclusive capability: a reading never takes the machine and never
waits for it to be empty — but a machine a host-wide job has already emptied
stays empty, a reading included.**

The two arguments are both real and they are about different things, so both
win where they apply:

* `ExclusiveNeedsIdle` — "this label needs an idle runner" — is the reading
  *consuming* exclusivity. A reading able to hold a whole machine empty for
  itself is the incident in a worse costume: not one slot but every slot. It
  does not apply to a reading.
* `HostIsExclusive` — "a host-wide job is holding this machine" — is the
  reading *respecting* exclusivity. D46 already decided that host-wide empties
  the host of *everything*, "including work that asked for nothing", and a
  reading is such work. `exclusive` is the machine owner's promise that
  nothing runs beside her job, and the figures an exclusive job exists to
  measure are exactly what a process beside it would spoil. A promise broken
  for a cheap process is a promise broken. It does apply.

Same reasoning for the two neighbours of exclusivity. A reading **never asks
for a quiet host** (E-27's `drain_first`): nothing may hold a machine empty
for fifteen minutes on account of a `--labels` line, so
`needs_a_quiet_host` answers `None` for one and `reservations` reserves
nothing for one. A reading **does respect** a host being emptied for somebody
else: readings allowed to slip onto a draining machine would starve the drain
for ever, which is the attack E-27 exists to answer, and the wait is bounded
by the same fifteen minutes everything else is. And a reading **counts among
the jobs running** on a machine, so an exclusive job waits for it — at most
the five minutes of `READING_CAP` — rather than starting beside it.

A pairwise `excludes` is contention between *holders*, and a reading is not
one: it holds nothing for the exclusion to bite on. Host-wide is about the
machine; an exclusion is between two capabilities. That is why one reaches a
reading and the other does not.

**3. A busy probe's verdict: a reading does not wait for it, whichever kind of
suspension it is.**

A probe says something *outside* Herd holds the capability. What `suspends()`
protects is the **handing out of that capability** (E-26, D48) — and a reading
is handed none: it runs a `--labels` line, which does not touch the GPU the
lock file is about. The judgement itself is not split and not duplicated:
`Availability::suspends()` stays the one place that decides what suspends,
made once in `herd-proto` for the runner and the hub alike. What D159 changes
is only *who is held to it*, and it changes it at the one `if` that already
separates the two halves.

The failure direction settles it, and the asymmetry is total. A reading that
runs while a probe says busy costs one `--labels` line on a machine something
else is using. A reading that waits for a probe nobody can read — a missing
binary, a broken script, a runner whose first snapshot never arrives — waits
for **ever**, and every job its answer unblocks waits behind it. That is
tonight's incident with no bound on it at all. Where one mistake costs a shell
line and the other costs a convoy, make the cheap one.

### What a reading is still admitted against, and still bounded by

**Slot-free, never admission-free.** A reading runs a real process on a real
machine, so the room there still has to cover it (D119): admission is in its
path exactly as it is in any job's, and a reading declaring more than is free
waits with the figures in its sentence. In practice it declares nothing —
`reading_job` gives it no memory and no tmpfs figure, because it builds and
boots nothing — so the arithmetic costs it zero today. The thing that must not
happen is the *step* being skipped, and it is not: the day a consumer's
`--labels` needs room, the room is weighed.

`READING_CAP` is untouched: five minutes, the hub's own figure, because a
reading boots nothing. The runner's own cap still bounds it and is never
raised by it (D108) — a reading carries the hub's figure where a declared
command carries its own, and the smaller of that and the owner's still wins.

**No recursion, still structural.** The hub is the only thing that mints a
reading and mints it with none of its own (D74's third limit). D159 adds a
second field to that same mint — `reading_for`, the job the reading was minted
for — and nothing on the wire can ask for either. `PROTOCOL_VERSION` stays at
2: nothing about this crosses the link. `RECORD_VERSION` does not move either,
for D73's reason — one more optional field with a `serde` default, read on an
old record as a job that is not a reading.

### And the sentence, which is half of why this cost forty-five minutes

Every sentence Herd produced about the convoy was **true**. The waiters
correctly said `build has 1 slot(s) and 1 are taken`, and not one of them said
the thing holding that slot was a reading rather than a build. A figure a
human cannot break down is a figure that gets broken down by hand, at night.

So every sentence that counts the jobs on a machine now says how many of them
are requirement readings: the slot waiter (where a reading is now *beside* the
count, holding no slot, and the sentence says so), the host-wide waiter, the
drain-first waiter, and admission's own "N jobs running there hold X of
memory". One wording, in one function, `of_which`. And the line explaining a
reading's own placement says in as many words that it holds none of the
machine — so a record of a reading can never be read as a record of something
that held a slot.

---

## 2026-10-23 — D160 The architecture is drawn, in one document

**`docs/architecture.md` holds the whole architecture as Mermaid, and it is
reference rather than a journal** — it changes when the behaviour changes,
like `docs/runner-confinement.md`, which it points at instead of copying.
Alexa asked for a complete picture because he is going to review one, and
"complete" was the acceptance test, not brevity.

**The cut: one top-level diagram that really is the whole system, then one
detail diagram per box of it, each complete in itself.** A single graph
holding everything would be a hairball nobody can review, which fails the
requirement it was meant to meet; a set of fragments with no whole is the
same failure wearing the other costume. So there is exactly one picture of
the system and twenty-four expansions of its boxes, and the document says so
in its first two sentences, because a reader must be able to tell a
deliberate structure from a dodge.

**Everything is read out of the code, never out of the plan.** Where the code
and `docs/plan.md` or `docs/coverage.md` disagree, the code is drawn and the
disagreement is reported as a finding — a diagram that quietly sided with a
document would make the document true by drawing it.

**One styling convention, in every diagram, with a legend as the first
figure.** Four classes — `built`, `unbuilt`, `store`, `outside` — and two
arrows, solid for a path that exists and dashed for one that does not.
**Anything unbuilt also carries the word in its own label**, so the
distinction survives a renderer that drops CSS, and §12 collects every
unbuilt thing in one table with where each gap is recorded. A reviewer who
cannot tell a drawn line that exists from one that is planned is reading a
sales brochure.

Decision numbers and expectation numbers sit beside the structures they
justify, so a review can jump from a box to the reasoning.

**Every block was rendered before this was committed**, with
`mermaid-cli` 11.12.0: twenty-five of twenty-five. Two defects it caught are
worth writing down, because they are silent and both were in the first
draft — a `;` inside a `sequenceDiagram` message is a **statement
separator** and swallows the next line, and a transition label in a
`stateDiagram-v2` must not be quoted. The renderer is not in the flake and
should not be: this is a once-per-document check, not a gate.

---

## 2026-10-23 — D162 The image ships on the release, binaries ship first

**Three tags in a row produced nothing, and the durations say why.**
`gh run list --workflow=release.yml`: v0.3.1 failed in 22 s, v0.3.2 in 24 s,
v0.3.3 in 26 s. Twenty-two seconds is not a build — it is a checkout, a Nix
install, one `nix eval`, and then the credential preflight D153 put in front
of everything, refusing the run because `vars.DOCKERHUB_USERNAME` and
`secrets.DOCKERHUB_TOKEN` do not exist. `gh release list` shows the
consequence: the newest release is **v0.3.0, of 2026-10-16**. Three versions
of Herd have been tagged and none of them has a binary anybody can download.
The step whose whole purpose was to prevent half a release has instead
prevented three whole ones, and it did it on the day the first consumer
became the sole operator of the running hub and was promised deployment
artefacts.

Alexa's decision, which this entry records: **take the registry off the
release path.** He is not creating the Docker Hub account or the token, and
that is settled, not pending.

**What survives of D153, and what does not.** What survives is its intent
and its main structural rule. The intent — an operator gets a runnable hub
image out of a tag without building it — is kept in full, and is in fact
better served than before, because it no longer depends on an account
nobody has. The rule — `nix/hub-image.nix` is the single derivation that
has ever built this image, there is no Dockerfile and no second build — is
untouched and is the reason the archive can be trusted at all. So is the
"no `latest`" argument, which the new delivery strengthens rather than
weakens: a tag that arrived by `docker load` cannot be moved by anything
but another `docker load`, so a fleet cannot cross a `PROTOCOL_VERSION`
line because of a reboot.

What does not survive: the registry as the only way to get the image, the
`skopeo` push and the pull-back-by-digest inside the workflow, the
preflight, and — this is the one that matters most — D153's ordering
argument. D153 reasoned that a tag must never leave binaries published and
an image missing, and put every image step *before* the GitHub release to
guarantee it. That is a defensible trade when the two halves are equally
reliable. It is the wrong trade when one half depends on a third party and
the other does not, and the cost of being wrong is what the first paragraph
measures. So the order is now the reverse of D153's and the reversal is the
point: binaries built, proved on Arch and on Alpine, summed, **published**
— and only then the image built, asserted, smoke-tested and attached. From
the release step onward a failure costs the archive and never the binaries.

**The body is written in two halves for the same reason.** A release that
describes an asset it does not have is worse than a release with three
files and no fourth, so the half of the body that says "`docker load` this
archive" is added by `gh release edit` after the archive is really
uploaded. `SHA256SUMS` goes up twice for the same reason: three lines with
the release, a fourth appended and re-uploaded with the archive, with the
count asserted at both points. A reader who took only the binaries uses
`sha256sum --ignore-missing -c`, which is now what every document here
prints.

**Keeping a registry path, and why it is not in the workflow.** The brief
asked whether an optional, skipped-when-unconfigured push belongs in this
file. It does not, and "remove it" is the recommendation. Three reasons.
First, it was never working machinery: D153's own closing paragraph says
"none of this has run" — not the login, not the copy, not the digest file,
not the pull-back. Deleting unexercised YAML costs nothing, because there
is nothing proven to lose. Second, an `if: vars.… != ''` step forks
everything downstream of it — the body would need a digest line in one
branch and not the other, the pull-back smoke would run or not — and
conditional branches nobody exercises are precisely where the next broken
release comes from. Third, a skipped step is a quiet step: a release that
silently did not publish where somebody expected it to is a worse failure
mode than one that never claimed to.

The path to a registry is therefore kept where it cannot touch a release:
`devShells.release` still carries `skopeo`, and `flake.nix`,
`deploy/README.md` and `deploy/release-notice.md` each give the one command
that copies the downloaded archive into any registry. It is a thing a
person runs, by hand, after a release exists. If somebody later wants it
automated, the two commits D153 shipped (`0a4e2fd`, `7a7e4bd`) are in
history and can be read back — but it would want to be a separate workflow
on a separate trigger, never a step the tag has to get past.

**`scripts/image-smoke.sh` earns its place more than it did, not less.**
D153 ran it twice: once on the locally loaded archive, once on an image
pulled back from the registry by digest, and called the second the one that
mattered. The second run is gone with the registry. What is left is the
run that was previously the warm-up, and it is now the only proof the image
has — and it happens to be the *right* proof, because it is exactly what an
operator now does: `docker load`, then run the container shaped the way
`deploy/docker-compose.yml` shapes it. The script and the deployment walk
one path. It also needs no credentials, which is the other half of why it
survived: nothing in it can stop a release for want of a secret. Keep it,
unchanged except for a header that no longer describes a registry.

**The archive, measured rather than assumed.** 18 958 841 bytes — 18.1 MiB
— built from this tree at 0.3.3, well inside GitHub's per-asset limit, and
`herd-hub-image-<version>.tar.gz` on the release page. One more figure is
worth recording because it was not expected: the image id in that archive,
`sha256:6e92753d0c66…`, is **the id of the container running on this server
right now**. The operator's hub was loaded from this same derivation by
hand, so the archive the release will attach is byte-for-byte what is
already in production. `docker load` of it was a no-op.

**How this was proved without burning v0.3.4.** The next real tag
finalises a handover and must not be the experiment, so none was created.
What was *executed*, locally, on this host: `nix build .#hub-image`; the
archive's `manifest.json` carrying exactly `herd-hub:0.3.3`; `docker load`;
`scripts/image-smoke.sh herd-hub:0.3.3` green against the loaded tag — a
containerised hub, a host runner, two real jobs, the second exiting 7; and
then the operator's own path end to end, `deploy/docker-compose.yml` with
`HERD_HUB_IMAGE=herd-hub:0.3.3` in a `.env`, `up -d`, the hub's three
startup lines, a runner approved and a real job through it — run under a
project name and a port of its own so the live `herd` project at `/srv/herd`
was never touched, and torn down with `down -v`.

What is **unexercised** and must be said plainly: no step of the new
workflow has run on GitHub. The reordering, the two-half body, the second
`SHA256SUMS` upload and the `gh release edit` exist only as YAML, and D132
is the entry that says a release step nobody has run is a release step that
does not work. Two things close that, in order, and neither needs a tag:

1. `workflow_dispatch` on this branch with `tag` left empty. It runs
   everything above the release — both distribution smokes, the static
   assertions, the sums, the image build, the archive assertion,
   `image-smoke.sh` — and creates nothing. This is possible only because
   `release.yml` already carries `workflow_dispatch` **on `main`**: GitHub
   offers the trigger from the default branch and then runs the file from
   the ref you pick. The note in `d2d37b7` saying a hand-run cannot reach a
   non-default branch was true when this workflow was not yet on `main`,
   and is no longer; so the temporary `branches:` trigger that commit
   deleted does not need to come back. This is also why the `tag` input
   keeps its name and shape here and only its description changed: the
   form GitHub offers is read from the default branch's copy of the
   file, so an input renamed on a branch is an input a hand-run on that
   branch cannot be given.
2. `workflow_dispatch` with `tag: v0.3.3` — a tag that already exists and
   whose run failed. It publishes the release v0.3.3 never got, which is
   the handover artefact, and in doing so exercises the two `gh release`
   steps and the body halves for real. The workspace is at 0.3.3, so the
   version check passes. v0.3.1 and v0.3.2 cannot be recovered this way
   and will not be: the workspace no longer says those numbers, and the
   check that refuses them is right to.

Both are Alexa's to run, like anything outward-facing.

**On the number.** This entry is 162 and the last on `main` is 159: D160
and D161 are held by two unmerged branches, and the gap is left rather than
filled, for the reason D148's closing paragraph gives — a decision number
cited in code is an identifier, not a position. D143 and D154 do not exist
either.

---

## 2026-10-23 — D163 A runner's figures belong to the machine's owner

**The refusal.** The first consumer's jobs ask for 10 GiB of tmpfs. The
server's runner lends 4 GiB, so the hub's record of `j0000001566` says, in
its own words, that *"server has 4 GiB of tmpfs in all and this job needs
10 GiB, so no reservation there can ever cover it"*. That is not a wait: it
is a permanent refusal, and three jobs are held by it. `/dev/shm` on that
host is 16 GiB — the capacity is there and has been all along. What stood in
the way was `"--host-tmpfs 4GiB"`, a string literal in
`deploy/nixos-herd-runner.nix`.

**And the operator could not work around it.** The honest attempt is to
append `--host-tmpfs 8GiB` through `services.herd-runner.extraArgs`, which
is what that option is for. It failed:
`the argument '--host-tmpfs <SIZE>' cannot be used multiple times`. So the
figure was ours and nobody else's, on a machine we do not own, with no way
for its owner to say otherwise short of editing this repository. That is the
thing being fixed, and it is worth more than the one number: a deployment
that states a cap its operator cannot restate is a deployment that decides
for someone else's hardware.

### The two halves

**Seven figures become options of `services.herd-runner`.** `hostTmpfs`,
`hostTmpfsPath`, `jobLogCap`, `jobDiskBudget`, `jobNice`, `healthEvery` and
`drainGrace`, each defaulted to the value that was literal in the module a
minute ago: `4GiB`, `/dev/shm`, `64MiB`, `20GiB`, `10`, `15`, `3`. The
module already had `jobTimeout`, `jobMemoryMax`, `hostMemory`,
`hostMemoryFloor` and `hostThreads`; there was no reason beyond history for
the line to be half declared, and the half left out included the one figure
that refuses work outright.

**No default moves.** The point is that an operator can choose, not that we
choose differently for her. This is checkable rather than asserted: the
module's `ExecStart` was evaluated before and after the change with
`nixosSystem` and the two strings are byte-identical, so a `nixos-rebuild`
that only picks up these options is a no-op and not a restart anybody has to
plan. The server will raise `hostTmpfs` next, in `/etc/nixos`, which is the
owner's file and the owner's move.

**A repeated flag on the runner takes the last value.** `args_override_self`
on `herd-runner`'s clap command, which is clap's own word for "replace prior
occurrences of arguments rather than error". It is what makes `extraArgs` an
escape hatch again: this command line has two authors — a deployment that
writes the figures it chose, and the operator of the machine, who appends to
it — and before this the second author could not correct the first. Options
whose action is to append are untouched by it, which matters here:
`--privileged-helper`, `--declare-capability` and `--deny-capability` each
declare a *list* by repetition, and a last-wins reading of those would
quietly narrow a machine's declaration to its final line and refuse the rest
later, on a job. That is asserted, not assumed.

### Why plain types and not `nullOr`, with the one exception recorded

`jobPidsMax` and `jobIoMax` are `nullOr` because absence is a *statement*
there: the flag leaves the line entirely and the runner's own default stands
(`pids.max` 1024, and no `io.max` written at all). None of these seven is in
that position. All seven are on the ExecStart line today, every one of them
with a figure somebody chose, and keeping them on it is what makes the
defaults a no-op.

One of them does have a real third state, and it is recorded here rather
than built: with `--host-tmpfs` absent the runner *measures* the mount and
lends what it finds. On the project's server that would be 16 GiB rather
than 4, which is a different decision about what the host lends and not a
spelling of the same one — so `hostTmpfs = null` meaning "lend whatever
`/dev/shm` has" is a coherent option and a separate ruling. It is Alexa's,
not this entry's. A figure is also the safer default of the two: a machine
that lends what it happens to measure lends more the day somebody remounts
`/dev/shm`, and invariant 2 is about the host never falling over.

### Only the runner, and why that is not an inconsistency

`herd-hub` and `herdctl` keep clap's refusal, deliberately.

The hub has no command line to override. `deploy/docker-compose.yml`
configures it entirely through the environment — `HERD_LISTEN`,
`HERD_STATE_DIR`, `HERD_REQUESTER_TOKEN_FILE` — and an environment variable
holds one value by construction, so there is no second author to make room
for. The day the hub's deployment grows an argument list this argument comes
back, and nothing about the change would be harder then.

`herdctl` is the opposite case: its command line has one author, a requester
typing or generating it in one place, and a flag given twice there is a
mistake rather than an override. A requester who writes `--priority` twice
wants to be told, not quietly given the second one — the hub's queue is the
wrong place to discover a generated command line was wrong. So consistency
across three binaries would buy nothing and would cost `herdctl`'s loudest
check on its own callers.

### Done means a passing test

`a_repeated_option_takes_the_value_stated_last` reads the parse directly, on
`Args`, in `herd-runner/src/main.rs` beside the struct it is about — the
runner had no argument tests at all, and `herdctl`'s own `mod tests` in its
`main.rs` is the precedent for where one goes. It asserts the operator's
case (`--host-tmpfs 4GiB … --host-tmpfs 10GiB` yields 10 GiB) and then all
seven of the module's figures stated twice over. Verified to fail without
`args_override_self`, which is the only way to know it is testing anything.
Two more beside it: the list options keep every occurrence, and
`Args::command().debug_assert()`, because nothing in the suite built this
binary's parser before and clap's own conflict assertions ran nowhere.

### Two things that were no longer true, and the number

`deploy/herd-runner.service` said the copied-unit path *"is what the
project's server runs today"* and the module said the server *"stays on the
imperative path"* until the owner imports it. Neither is true:
`systemctl cat herd-runner` on that host prints a `/nix/store` unit with
`Documentation=file:///etc/herd/joining.md`, which only this module
produces, and `docs/status.md` has said the runner is declarative since
D144. The module's worked example also described the live unit as
*"transient"* and the rebuild as *"the rebuild that makes it permanent"* —
that rebuild has happened. All three corrected, because the one thing both
files promise is that they describe the host as it is.

**On the number.** This is D163, and the three numbers under it are spoken
for by work that is still in flight: D160 by an unmerged pull request the
owner is reviewing, D161 by `docs-handover`, which appended its own entry
while this branch was being written, and D162 by a branch on the release
path. This entry was D161 for an afternoon and was renumbered rather than
contested, which is the cheap direction: the citations that move with it are
three — two comments in `herd-runner/src/main.rs` and one option description
in `deploy/nixos-herd-runner.nix` — and all three are on this branch, where
nobody has read them yet. That is exactly the calculation D148 refused to
make for itself, and the rule is the same one read from the other end: a
decision number is an identifier and not a position, so it is free to move
before anything cites it and fixed afterwards. The gaps stay, here and at
D143 and D154.

---

## 2026-10-23 — D164 A persisted drain is read as off

**The outage.** 15:06 today, on the machine's own operator's account, not
inferred from the code: `herdctl switch server drain`, then a
`systemctl restart`. Every start read the persisted position, held nothing,
and left at once — systemd started the runner **eight times in two minutes**.
`herdctl switch server on` then failed with *"no runner called server is
connected to this hub"*, because the runner was never connected long enough
for the instruction to land. The only way out was a shell on the machine and
`herd-runner switch on`.

Three mechanisms, each defensible alone and lethal together: the position is
persisted in the data root so that an owner's withdrawal outlives a reboot
(D99); `drain` means "finish and leave" by its own specification; and the
NixOS module sets `Restart = "always"` with `RestartSec = "5s"`, which is what
a runner that never crashes should have. Nothing here was a bug. The composite
was.

**Choice.** A `drain` found in the switch file **at startup** is read as
`off`, with the reason its owner gave, and the file is rewritten to say so.

The reasoning is about what the word *is*. A drain is an instruction to one
process — finish what you hold, then go — and a process that holds nothing
because it has only just started has already carried both halves out. Obeying
it literally is not obedience, it is a loop. `off` is the position a restart
can carry meaningfully: nothing new is claimed, nothing is served to a peer,
the reason stays visible in every sentence that mentions the machine, and the
runner **stays connected**. That last clause is the whole value of the choice
and not a side effect of it — it is what puts the recovery back within reach
of the hub, so an operator who wants the machine back on types
`herdctl switch server on` from wherever she is instead of finding a shell on
the host.

**Alternatives rejected.** An `--exit-when-drained` flag, which the first
consumer also proposed: it makes the loop opt-in rather than impossible, and
the operator who sets it is the same operator who would not have predicted
this. Keeping the runner connected and idle *in* `drain`: it would mean a
position that says "leave when empty" and does not, which is worse than
converting it, because nothing in the sentence an operator reads would have
changed. Doing nothing at the runner and having the hub hold a pending switch
for a disconnected runner — the consumer's third suggestion — is sound hub
state and a separate decision, and this change makes it unnecessary *for this
failure*: the runner is now reachable, so there is nothing to queue for it.
It would still earn its keep for a runner that is genuinely down while an
operator switches it, and it stays open.

**The file is rewritten, which is the part worth arguing about.** The
alternative is to convert in memory each time and leave the owner's file
saying `drain` for ever. That would be two truths about one switch: the hub
told `off`, the disk saying `drain`, and the watcher — which re-reads that
file every second — finding `drain` again and leaving. So the conversion is
written back, fsynced, exactly as any other position is (D99), and the next
restart has nothing to convert. When the write fails, which is a read-only or
full data root, the position is still taken and the watcher is handed that one
stale reading to ignore until the file changes — because a position that is in
force but not durable is already D99's case, and the loop must not come back
through it. A `drain` written **later**, by anybody, differs from that reading
and is obeyed in full: it is the owner speaking to this process, which is what
a drain is for.

**Done means a passing test.** On the pure reading,
`a_persisted_drain_starts_a_runner_off_and_not_leaving`; and
`herd-runner/tests/persisted_switch.rs`, which starts
a real runner on a persisted `drain` with nowhere to dial and asserts it is
still there two seconds later and that its file now says `off`. Verified to
fail — in 1.1 s, the way the server failed — with the conversion stubbed out,
which is the only way to know it tests anything. Its sibling asserts the
control: a persisted `off` still starts `off` and nobody rewrites her file.

---

## 2026-10-23 — D165 A probe that cannot be started refuses the file

**The outage, and it cost hours.** The first consumer's capability file
carried the literal, unsubstituted placeholders `@probe@` and
`@agents_user@`. Every probe sweep therefore failed with ENOENT, `kvm` and
`gpu-venus` were suspended for as long as that file was live, and jobs queued
behind capabilities that could never clear. The runner was up, healthy, and
reporting faithfully: the hub's waiting sentence said *"the probe
`@probe@ qemu @agents_user@` cannot be started: No such file or
directory"*, in as many words, on every job. Nobody reads a waiting sentence
until something is already late.

**Choice.** A probe whose program cannot be spawned **at all** refuses the
capability file when it is loaded, naming the file, the capability and the
program; and `herd-runner check --capabilities FILE` validates a file and runs
each of its probes once, exiting non-zero on anything that would refuse a
startup.

**This is not D49, and the next reader will want to conflate them.** D49 is
the runtime rule and it does not move: a probe that *runs* and whose answer
cannot be read — a timeout, a signal, an exit code the configuration gave no
meaning to, a lock nobody can look at — suspends its capability, because
something outside Herd owns the thing and an unreadable probe is never
permission to assume it is free. That rule is why today was survivable rather
than destructive. What it is not is a way to notice a *typo*, and it cannot be
made into one without becoming the other judgement: "suspend" and "refuse" are
answers to different questions.

So the two are told apart by what they are asked. A program that cannot be
spawned can never answer, so there is nothing to learn by asking it every two
seconds for the rest of the deployment; it is settled once, before the process
is a runner, the way a health cadence is settled at the handshake (D53). A
program that answers something unreadable is answering, and that is a state of
the world, not a state of the file.

**The seam is the file, not the startup.** `CapabilityFile::read` is where it
lands, rather than a check somewhere in `run_until`, because the thing an
operator got wrong is a file she wrote and the refusal has to name it. The
judgement itself is `unstartable`: a program with a `/` in it is that path, a
bare word is looked for on `PATH` — which is what `execvp` does — and nothing
is run. It is **conservative in one direction on purpose**: it answers "no
spawn could succeed" only when it is sure, so a program executable by a user
this process is not passes the check and is caught at runtime by D49 as
before. A false refusal would be a runner that will not start on a file that
works, which is a worse trap than the one being closed.

`herd-runner check` is the other half and the one that matches the lesson: the
file was wrong before it was deployed. It reads the file strictly, says of
every command probe whether its program can be started, runs each probe once
and prints the verdict, and exits non-zero on a refusal only — an unreadable
*answer* is printed as D49's suspension and does not fail the command, because
a held lock is often the honest answer of the moment. It reads the file *as
written* rather than through `read`, so it reports every probe instead of
stopping at the first. One caveat it prints rather than hides: the `PATH` it
searches is its own, and a runner under a unit file may have another.

**Done means a passing test.**
`a_probe_whose_program_cannot_be_started_refuses_the_file` writes the
consumer's own file, placeholders and all, and asserts the refusal names the
path, the capability and `@probe@`;
`a_probe_that_names_a_real_program_or_a_lock_file_starts_a_runner` and
`a_program_is_unstartable_only_when_no_spawn_could_work` are the other side of
the line, the second on all six shapes the judgement has to tell apart;
`check_reads_a_capability_file_and_its_probes_once` runs the subcommand's own
function on both a broken file and a good one, in the test binary that is
allowed to fork.

### Two tests that had to change, and one that was not testing what it said

`a_broken_probe_suspends_its_own_capability_and_hangs_nothing` — D49's own
end-to-end test — declared `/nonexistent/herd-probe` as one of its three
broken probes, which this change refuses at startup, so the runner would never
have reached the behaviour under test. Its fixture is now a real file,
executable, whose shebang names an interpreter that is not there: the spawn
still fails with ENOENT, at runtime, and the test's own assertion ("cannot be
started") is untouched. That is a better fixture than the old one as well as a
necessary one, because it is the case that genuinely survives this change — a
probe whose interpreter is uninstalled under a running runner — and it is
exactly where the startup check stops: it asks whether there is anything to
run, never whether running it will work.

`ten_hanging_probes_do_not_delay_the_runner_joining` declared ten probes of
`/bin/sleep 8`. **There is no `/bin/sleep` on this project's own host** —
`/bin` on NixOS holds `sh` and nothing else — so the ten probes this test says
it measures were ten instant ENOENTs, and the claim it prints a number for was
not being made. The probes are `/bin/sh -c "sleep 8"` now and really hang; the
join is still inside one timeout. The test was green before and green after,
which is why nobody noticed, and only the new refusal made it say anything at
all.

### One stale comment, corrected

`Capacity::new` said *"the first sweep happens before the runner joins a
hub"*. It does not, and has not since the sweep moved: `run_until` says in as
many words that the probes are read *beside* the join and not in front of it,
because a first sweep awaited there made joining cost as long as the slowest
probe — twenty seconds, measured. The design is still safe for the reason the
comment then half-stated: a probed capability with no reading yet is suspended
at both ends under D49. So this is a comment that contradicted the code, not a
hole; it now says what the code does and why that is safe.

**On the numbers.** Two entries rather than one, because these are two
unrelated mechanisms observed on one day and invariant 8 asks for one dated
entry per decision. D160 to D163 are held by branches in flight, so these take
the next two free numbers; the gaps stay, here and at D143 and D154, under
D148's rule.

---

## 2026-10-23 — D166 The product says what only a shell could say

**What changed under us, and it changes what this work is for.** As of today
the first consumer is the **sole operator** of the running system: they own
the hub container, the runner units, the capability files, the disk, the
restarts. We ship software; they run it. Until today a person with a shell
could answer *why did this job fail* and *is the fleet healthy* by reading
our source and their disk. Now the answer has to arrive in the product,
because whoever is asking has neither.

Today's proof of what that costs: their capability file carried
unsubstituted placeholders, every probe failed with ENOENT, `kvm` and
`gpu-venus` were suspended for hours with every job that needed them
waiting, and the only reason anybody found out is that we read their disk —
an access we have both now given up. `herdctl runner list`, which is where
an operator looks first, said nothing about any of it.

**Choice.** Three things, in one entry because they are one complaint with
three faces: a machine-readable verdict, a fleet listing that names a
runner's build and what it is not handing out, and a permission failure
reported as a setup failure instead of a disk cap.

They close chatOnS's grievances 9 (every verdict names its cause), 10a and
10c (the runner's name in a machine-readable result), and point 3.2 of the
scope review (the exit codes are ambiguous and a requester parses prose).
Grievance 10a's second half is folded in at the end.

### 1. `--verdict-to <FILE>`, and the two fields that earn it

`herdctl run` and `herdctl wait` take `--verdict-to <FILE>` and write the
job's end as JSON. `JobOutcome` was already `Serialize`; what was missing
was any way to ask for it.

The document carries the cause in one word, the runner's name and why
there, a cap's figures when a cap ended the job, the sentences `herdctl`
printed, the outcome as the wire carries it, and what the runner measured.
Two of its fields are the reason it exists rather than a convenience:

* `exit_code_from` — `job`, `signal` or `herd`. `herdctl --help`'s own
  exit-code table admits that a job's **own** exit of 124 or 125 is worn
  untouched and is therefore indistinguishable from a cap and from Herd
  having no verdict. One field apart, the three are three:
  `{"exit_code": 124, "exit_code_from": "job"}` is a build that chose 124,
  and `{"exit_code": 124, "exit_code_from": "herd"}` is a cap, with `cause`
  saying which.
* `job_exists` — `a_job`, `nothing` or `unknown`. This is D109's three
  stderr lines, which a falling-back requester has had to `sed` for, as a
  field. The prose stays exactly as it was; nothing about it is withdrawn.

`cause` is the outcome variant's **own serde tag**, through a new
`JobOutcome::cause()`, so the word in the document and the word on the wire
cannot drift apart. A second table of those names would have been a second
answer to "why did this job end", which is what invariant 4 exists to keep
single.

**A file and never a stream.** stdout is the job's own output, framed and
nothing else (E-02), and stderr carries Herd's prose that the same wrapper
is already reading a line at a time; a JSON document spliced into either is
one nobody can parse reliably. A caller that wants it on a pipe names one:
`--verdict-to /dev/fd/3`.

**Written however the run ends**, no verdict included — `cause` is then
`no_verdict`. The alternative was to leave no file, and that is the worse
failure: a wrapper reading the document would find the *previous* run's and
act on the previous run's answer. Failing to write it is said loudly on
stderr and never changes the exit code, for the reason `--outputs-to`
already has: the verdict is the answer the caller is waiting for and a path
the requester named and cannot write is a problem with that path.

**The version follows the precedent rather than inventing one.**
`OUTPUT_FORMAT_VERSION` is a `u32` in `herd-proto`, carried inside the
thing it versions, documented with what moves it, and listed in the version
table in `docs/architecture.md`. So `VERDICT_FORMAT_VERSION` is a `u32` in
`herd-proto` beside it, carried as the document's **first** field so a
reader can refuse a version before it reads anything else, and in the same
table. A second number rather than a reuse of the first, because the two
framings are parsed by different readers and will not move together.
Adding a field does not move it — every key is always present, `null`
included, so a document that gains one still answers everything it
answered — while removing a field, or changing what one means, does.

### 2. The build and the suspensions, in the listing

`ApprovedRunner` and `RunnerSwitch` gain one defaulted field each, a
`RunnerFacts` carrying the runner's announced build and every capability it
is not handing out with the reason. The hub already knew both: it computes
`version_gap` at the handshake and it receives `CapabilitiesChanged`. What
it did with the first was print a line on its **own stderr**, which is a
place nobody operating the fleet through `herdctl` ever looks.

`herdctl runner list` now prints the build on the runner's line and one
indented line per suspension underneath. Indented and one each, rather than
appended: a probe's reason is a sentence with a path in it — the
consumer's were sixty characters apiece — and two of them on one line is a
line nobody reads to the end. `herdctl switch`, which is the same fleet
seen from the owner's side, says the same two things.

**`suspended` is an `Option<Vec<_>>` and the `Option` is load-bearing.** An
empty list means "this hub is connected to the machine and nothing is
suspended"; absent means "this hub did not say". Had it been a plain `Vec`
with a `serde` default, a frame from a hub that has never heard of the
field would have decoded as a clean bill of health — which is the exact
defect this entry is about, rebuilt one layer down. A runner that is not
connected carries neither field, because a machine that is not here offers
nothing and a figure for it would be a report about a runner that left.

Neither field moves `PROTOCOL_VERSION`: a new field on an existing variant
may leave it alone when it is `#[serde(default)]` and means, absent,
exactly what its default says, and both do.

### 3. A permission failure is a setup failure

Three of the consumer's jobs — j984, j989, j1197 — ended `stopped_by_cap`
with `cap: disk` and exit **124**, carrying a detail that said plainly that
the runner could not *measure* the job's workspace, so it could not hold
the job to its disk budget at all, naming a path under `{out}` that cannot
be read. The text was honest; the outcome type was a lie, and 124 told
their requester that a budget killed their build.

The mechanism is known and is not ours to fix: the runner walks `{out}` as
uid `herd`, the consumer's jobs deposit trees owned by `server:users`, and
`chmod` on an inode somebody else owns is refused however often it is
retried. That half is theirs and they are already moving to runner-owned
trees. **Nothing about the walk's behaviour changes here**, including that
the job is still ended — a budget that cannot be enforced must never read
as a job that passed, which is D30 and invariant 2 and is right. What
changes is what the ending is *called*.

`root::measure` now answers with an `Unmeasurable { why, refused }` instead
of a bare sentence, where `refused` is `ErrorKind::PermissionDenied`. The
kind rides with the sentence rather than being re-derived by matching on its
words, which is the one way this could have been done wrong. A refused
measurement ends the job `SetupFailed`, which exits **125** and names the
path. Every other failure to measure — a path that is not a directory, a
measuring task that died — is the runner failing at its own work, and D30's
cap stays right for it.

**Why a new variant and not `CouldNotStart`.** Two reasons, and the second
is the one that settles it. `CouldNotStart` means Herd never got a process
out of the job, which is false here — the job ran, and produced output. And
it is the one ending the `ran on:` line is deliberately **withheld** for
(D16), because a job that never started has no output to frame. This job
ran on a named machine, and that machine's name is the only actionable
thing in the whole report: the setup that is broken is its own. Reporting it
as `CouldNotStart` would have hidden exactly the fact grievances 10a and
10c asked for.

**What the variant costs, said out loud.** `JobOutcome`'s own doc blesses a
new variant as the additive way to add an ending, and it does not move
`PROTOCOL_VERSION` under the rule that covers frames. But "additive" is not
"an older peer survives it": `JobOutcome` has no `#[serde(other)]`
catch-all, so an older `herdctl` decoding `setup_failed` fails to read the
frame. What it then does is exit **125** with a sentence naming what it
could not read — which is the same exit code this variant produces, with a
worse sentence. That is materially unlike D117's shape, where the hazard was
a job that *passed* being read as no verdict: here there is no verdict
either way, so an older requester is degraded and never misled. The same
reasoning covers a record: a hub rolled back below this change and reading a
`setup_failed` record refuses to start naming the file, loudly, rather than
guessing — which is what `RECORD_VERSION`'s contract already promises for
anything it cannot read. A catch-all variant on `JobOutcome` would make the
*next* ending cheaper and was deliberately left out of this change: it
would have to be a unit variant, so a record round-tripping through it
would lose its reason, and that is a decision of its own.

### Grievance 10a's second half: `switch on --reason` no longer fails

`herdctl switch <runner> on --reason …` was refused with *"switching a
runner on takes no reason"*. The position still carries none — a machine
nobody has withdrawn needs no explaining — but refusing a command because it
carries one extra piece of information is a bad trade: an operator switching
a fleet back on after a maintenance window types the same `--reason` on
every line, and the one that was `on` was the one that failed. The reason is
now accepted, dropped, and said where it was typed, on both halves of E-33
— `herdctl switch` and `herd-runner switch`.

A switch *file* that says `on` with a reason is likewise read rather than
refused, and that case moved out of the list in
`nothing_unreadable_reads_as_a_runner_that_claims`. The hazard that test
exists for is untouched: a truncated file, an unknown position and a missing
one are all still errors, because none of them says what it wants. This one
says `on` in so many words and carries one surplus field — and nothing in
Herd writes a reason on `on`, so such a file was typed by a person, and
refusing it would withdraw a machine nobody meant to withdraw.

### Done means a passing test

`the_verdict_document_tells_a_job_s_own_code_from_herd_s`, against the real
binary, a real hub and a real runner: a job that chooses to exit 124 and a
job a cap stopped at the same code, told apart by `exit_code_from`, with the
runner's name read out of `ran_on.runner`; and the run that got no verdict
at all, which overwrites a stale document planted first, because that was
the failure worth building the test around.
`a_workspace_the_runner_was_refused_is_a_setup_failure_and_not_a_cap` is the
split of part 3 at the `Attempt` level, both branches, asserting that the
reason names the path and the budget and that no cap is recorded;
`a_refusal_during_the_takedown_leaves_an_established_verdict_alone` holds the
one line D108 draws through it — the `down` step runs however the others
ended, so a refusal seen there is said in the job's log and the verdict is
left exactly as it was, which is a defect this change had on its first
writing;
`a_path_the_runner_cannot_take_its_access_back_on_is_refused_and_not_capped`
reaches a **real** refusal with no second user by putting a directory the
walk must search above the data root, which is the one place the
restoration deliberately does not reach — and skips itself loudly, under
D27, on a host where mode 000 does not bind the user running it.
`the_fleet_listing_names_a_runner_s_build_and_what_it_is_not_handing_out`
drives the real binaries with a probe that really starts and really says
busy, and asserts the build on the connected runner's line, the suspension
under it, and that the machine which is *not* connected claims neither.
The protocol's round trip carries both new fields, set and unset.

**On the number.** This entry is D166 and the last on `main` is D165. D161
is still held by an unmerged branch, so the gaps are D143's, D154's and
D161's, and all three stand under D148's rule. One entry rather than three,
because the three are one decision about who the reader is — and because
doing them apart would have meant three branches touching the same four
files in the same week.

---

## 2026-10-23 — D167 A job is finished when its evidence is here

**The first consumer, now the only operator of this fleet, reported the worst
defect Herd has had and reported it properly: with job ids, versions, and the
transfer itself already ruled out.** Runner 0.3.2, hub 0.3.3, herdctl 0.3.3.
Job `j1586` was recorded finished at **14:11:43**; its files went on arriving
until **14:12:40**; a `herdctl wait --outputs-to` that fetched at 14:12:20 was
told **195 of 657** files were not there — *"did NOT get … the hub does not
have it"* — and a second `herdctl wait` minutes later got all of them.
`j1578` and `j1580` were 268 and 273 files short the same way. The gap
between "finished" and the last output was **55 to 130 seconds**, and they
were working around it per build.

**Nothing was lost and nothing was broken. The word *finished* was wrong.**
Their own first move was to check that every object of every pack was on the
record, and it was. What the hub did was relay `FromHub::JobFinished` the
moment it had written the end, carrying whatever `read_the_outputs` found on
the disk *at that instant* — and a streamed collection arrives **after** the
verdict by design (D148). D150 then fixed `is_current` precisely so that a
file chunk arriving after the verdict is not dropped. So this is not a slip
in one function: the system was built to expect late arrivals and built to
report the job over before them. **"This job finished successfully" and "this
job's evidence is on the hub" were two different facts reported as one.**

**The choice: the verdict waits, and the record does not.** The consumer
offered two shapes — report finished only when the outputs are here, or make
`wait` block while they are in flight — and the answer is the first, with one
line of the second's reasoning kept. `ToHub::JobFinished` still records the
end exactly when it did; what is held back is only the sentence a requester
reads, in memory, per job. Three reasons, and the first is the one that
decides it:

1. **Recording the end is what frees the slot.** Confirmed in
   `matcher::out_there` rather than assumed: a dispatch pass counts a job as
   occupying its runner for exactly as long as the record says an attempt is
   open there — `Assigned | Running | Interrupted` with no `ended_at` — and
   from nothing else (D79). `holdings`, `reserved` and the admission algebra
   all read that one filter. So delaying the write would have held a build
   slot for up to two minutes after the work was done, on machines with one
   or two of them, once per job. Holding the *frame* touches nothing the
   matcher reads.
2. **A requester is not only `herdctl`.** Their tooling keys off the verdict;
   so would anything else. Fixing the requester would leave the hub still
   saying something untrue, and the next consumer would find it again.
3. **It is what D148 already promised** — *"`--outputs-to` waits for what it
   asked for and says so"* — delivered at the end that can keep it.

**It cannot wait for ever, and the bound has two legs because the failures
are two.** `COLLECTION_SILENCE`, 30 s, on *silence* and never on duration: a
few hundred megabytes on a slow link is slow and not broken, and a deadline
on the transfer itself would make the fleet's biggest jobs the ones that
fail. The figure is twice the runner's own stall sentence, so the machine
sending the bytes speaks before the hub speaks for it, and it is the same
figure as the requester's own fetch bound (D150) so that the two cannot
disagree about when a transfer stopped. A constant and not a flag: nothing an
operator can tune makes it better. The other leg needs no clock at all — **a
runner that is not connected is sending nothing**, so a held verdict whose
owing machine has gone is released at once, naming it. That is also what
answers a `herdctl wait` an hour later on a collection that never completed:
immediately, rather than after half a minute of nothing.

**And a wait that cannot be rewarded must not be silent.** A held verdict
says why it is held, on the clock that already keeps a followed job's stream
alive (E-04's precedent, and `still_there_every`'s cadence) — *"this job has
ended and its files are still arriving: 462 of 661 output file(s) and 338 MiB
of 400 MiB have reached this hub from wsl-pc"*. Sixty seconds of waiting is
four such lines instead of a minute of silence after a job the requester
watched finish. It is one mechanism and not two on purpose: the note **is**
the traffic that keeps the link warm, and `say_again_why_jobs_wait` gives a
job whose record is terminal neither a restatement nor a breath — right when
a follower was always dropped at the verdict, and a hole the moment one is
not.

**Who waits: the requester says, and that is the second half of this
entry.** The first shape held every verdict, whoever was following, because
the hub cannot guess which requester is going to fetch — so a `herdctl run`
with no `--outputs-to`, which receives no byte of the collection and never
asks about it, paid the minute or two the bytes take. D148 had promised that
such a run *"exits on the verdict and waits for nothing"*, and that promise
was going to be broken as an accepted cost, on the argument that a field on
the wait frame was protocol surface while a branch was in `herd-proto`.
That argument expired when the branch merged, and Alexa pulled the line the
same hour. It was the right call: an accepted cost that nobody has to accept
is just a defect with a paragraph in front of it.

So `wants_files` rides `SubmitJob` and `WaitForJob`, the hub keeps it **per
follower** rather than per job, and one verdict goes to some followers at
once while the rest of them wait. Two requesters on one job answering
differently are both right, which is the whole shape: `herdctl run --command
build` is told the moment the end is recorded, and the `herdctl wait
<id> --outputs-to` beside it is told when its collection is whole.

**The field's default is the load-bearing part, and it defaults the way
silence has to fail.** Absent means *wants them*. The frames that leave it
out are the ones from every `herdctl` already deployed, so defaulted the
other way the upgrade that fixed this defect would have handed every
existing requester the early verdict and put it straight back in — the fix
arriving as a regression, for exactly the people it was for. `bool`'s own
`Default` is `false`, so the safe answer is the one a reader would least
expect, which is why it is a named function with the reasoning on it and why
`a_wait_that_predates_wants_files_is_read_as_wanting_them` asserts it off
literal bytes rather than off today's types. Same shape as D166's `facts`,
whose absence means "this hub did not say": an absent field reports what the
peer did not tell you, and the safe reading of that silence is the one that
costs a little waiting rather than the one that costs a wrong answer. A
defaulted field moves no version number (D124, D128), so this still ships
without a coordinated restart.

**Beside `asked`, not inside it.** `Asked` is shared with `DryRun` by E-07's
own safety property — a dry run must describe the same job a submission
makes — and a dry run has no files to want. It is also not a fact about the
job at all: it is a fact about one follower, and two followers of one job
disagree about it by design.

**The exit code, which is where this entry contradicts an earlier one.**
D148 ruled that on an incomplete fetch *"the job's exit code stands"*, with
`--outputs-required` as the flag for anyone who wanted it fatal — and that
flag was never built, so for a year the honest answer to a short collection
was a loud sentence and a **zero**. That is what the consumer was working
around: a wrapper cannot act on a sentence. So `herdctl` now exits **2** —
*"a command did part of what it says and named the rest"*, D99's code, with
its existing meaning rather than a second one — and exits it in exactly one
case: `--outputs-to` was given, the collection came back short, and the job's
own code was **0**. A failing build whose screenshots are short still exits
1, a job stopped by a cap still exits 124, a job with no verdict still exits
125. **The only code replaced is the one that says "everything you asked for
happened".**

That is not a repeal of D20. D20 forbids a side channel becoming *a verdict
about the job*: the verdict is printed, is on the record, is unaltered, and
2 is `herdctl` speaking about itself. 125 keeps its single meaning, which is
the property the project has defended twice. `a_collection_the_sweeper_took_…`
asserted the 0 and now asserts the 2, with its D20 paragraph rewritten rather
than deleted — a collection retention took is also a `--outputs-to` that
wrote a short directory, and one rule with no special cases is worth more
than a kind answer for the evicted case.

**And D166's document is where that distinction gets made in a field rather
than in a sentence**, which is the field doing the work it was added for.
A 2 is not the job's code, so the document says `"exit_code_from": "herd"`
beside it; without that a reader would go looking for a build that exited 2
and there was none. `passed` is deliberately left **true** for a green build
whose files are short, because `passed` is the *job's* verdict and turning it
false would be the D20 violation this entry just argued against. What tells a
wrapper the run was incomplete is the code, that field beside it, and the
sentence in `said` naming how many files of how many are missing — which is
why the sentence is returned out of `keep_the_outputs` and pushed onto `said`
rather than printed where it is discovered. A wrapper reading the document
and keying on `passed` alone would still act on an incomplete set; the code
is the signal, and it is the one the consumer's `run.sh` already reads.

**One finding that contradicts the brief, and it is why the suite was
green.** `herdctl::fetch_missing_outputs` *does* wait, and has since D150. It
gave up anyway, and the reason is arithmetic: each round asks only for what
is still missing, and reported the bytes of **that round**. The first round
of a 657-file collection is by far the biggest, so every round after it
reported less than the best figure seen, however many files it had just
collected — `since` never reset, and the stall bound fired 30 s later on a
transfer that was arriving normally. 14:11:43 plus the first round plus
thirty seconds is 14:12:20, which is the minute the consumer measured. **With
a one-file collection the figure is monotonic by accident**, and one file is
what the suite streamed, which is exactly how this stayed invisible. The
progress figure is now seeded with what earlier rounds already hold. The hub
side above would have masked this defect without fixing it; both are fixed,
and only one of them was in the brief.

**What is proven, and how.** `herd-hub/tests/outputs_whole.rs` runs a real
hub, a real runner and a real declared job leaving **33 files of a megabyte**
— over the inline threshold in total, and *many*, which is the shape that
hid this — with a hand-written requester that fetches the **instant** it is
told the job finished, once, with no retry. With the hold stubbed out it
reports `did NOT get 33 of 33 file(s)`, which is the field failure
reproduced at smaller figures; with the fix, nothing is absent. The same file
asserts the honest end: a runner that announces a collection and goes away
without sending a byte of it leaves a wait that **ends**, naming the machine
that owed the bytes, with the file named off the manifest that outlives them.
And a third test is the one the field made possible: two followers on one
job, one wanting files and one not, with the second holding its verdict while
the first still has none — judged off `collection_owed` rather than off a
sleep, so a run that reaches it with the collection already complete **fails
saying nothing was measured** instead of passing quietly. `herdctl` asserts
the exit code either way, including that a failing build keeps its 1 and that
D166's `SetupFailed` passes through untouched.

**What is deliberately not done.** `read_the_outputs` and its neighbours are
untouched beyond the one call site. No **frame** was added: the sentence
rides `FromHub::JobNote`, which every deployed `herdctl` already reads, and
the one new field is defaulted, so this ships without a version move and
without a coordinated restart — which for a defect being worked around per
build is most of its value. `JobStore::collection_owed` is new and
`collections_owed` is now written in terms of it: one definition of complete,
asked two ways, because walking every record to answer about one job is the
work D152 refused to do per verdict.

**On the number.** D167, after D166, which merged while this was being
written and whose `exit_code_from` this entry leans on. D161 is still held by
an unmerged branch, so the gaps are D143's, D154's and D161's, all three
under D148's rule.

---

## 2026-10-23 — D161 Herd is shipped, not operated

**Context.** The owner decided on 2026-10-23 that Herd no longer operates
the hub or the runner: *"tu ne dois plus gérer le hub et le runner, cette
responsabilité tombe au client, soit chatOnS OS."* Until now the project
ran its own fleet — one hub in docker compose and one native runner on
the NixOS server — and every milestone from M16 on was proved against
that deployment. The two analyses written the same day found the fleet
half-dead in ways only its author could see: two capabilities suspended
behind an unsubstituted `@probe@` in the consumer's capability file,
seven runner figures the operator cannot set, a `drain` that restarts
into a loop, 90 GB of outputs with no eviction ever, a spool the runner
cannot free, and a release workflow that has published nothing since
v0.3.0. Every one of those was found by reading the server's disk.

**Choice.** Herd becomes software we build and ship, not a service we
run. We own the protocol, the four crates, the deployment files as
reference texts, the releases and the documentation. The consumer owns
every running instance: the hub container and its volume, the runner
units and their data roots, the capability files and the probes, the
tokens, the disk, the restarts and the upgrades. What crosses the line
is always a named artefact — a file format, a flag, a document, an exit
code, a schema — and never a person logging in. A fact about their
machines reaches us as a bug report carrying a job id and two version
numbers, or it does not reach us. `docs/handover.md` holds the plan: the
line subsystem by subsystem, what must be built before the handover is
honest, what we stop doing, what they must accept, the milestones
revised, the order, and the risks.

**The one rule that keeps this from being a withdrawal.** We may not
hand over an instance that only its author can tell is healthy. So the
handover is gated on one release, v0.3.4, that makes the instance
judgeable from outside: a runner that refuses a probe it cannot start
and a `herd-runner check` to run before starting it; a verdict a machine
can read; the seven module options; a persisted `drain` read as `off`
at startup; the version on the fleet listing; a reading costed at a
figure and bounded per runner; and a release workflow that publishes
without credentials we do not hold. Until that release exists the
decision is taken and the handover is not done.

**What is cut with it.** M15d's self-update (a third deployment path
into machines we no longer deploy; D66 was the owner's requirement for
a fleet he operated), M14's scheduler half (a timer and a `herdctl
submit` on their side), M11's pack half and M12's peer-to-peer (a
distributed cache for a fleet we do not run, with no measurement
defending it). Each cut costs nothing today; `docs/handover.md` §7 names
what each would cost to keep.

**Alternatives rejected.**

1. *Keep operating the server's pair as the reference deployment while
   the consumer operates the rest.* Rejected: it is the arrangement that
   produced today's fleet — one party with the keys and no obligation,
   the other with the obligation and no keys. Two operators of one
   instance is nobody operating it.
2. *Shrink Herd to a library and let the consumer build the service.*
   Rejected: the four crates, the unit and the compose file are the
   product; invariant 1 is kept by refusing features, not by refusing
   to ship.
3. *Hand over now and fix the gate afterwards.* Rejected: a trap we
   operated around becomes a trap we shipped the moment the handover is
   announced, and the first grievance against a restart loop or a
   half-dead runner would be answered by us logging in again.

**Consequences.** `docs/status.md` describes the software, never a
deployment; `docs/plan.md`'s M17 becomes a supported-platform milestone
and M18 is walked by the consumer against a release; a new M19 is the
handover itself; measurement access is replaced by the report format,
the fleet listing and M13's status document, in that order. The
remaining loss — we are told about problems instead of discovering them
— is the decision, and is written down as such (`docs/handover.md` §8).

**What the gate cost, and what the handover did not wait for.** Written
on the day of the decision, this entry budgeted the gate at five to six
days and made it a precondition. It was built in one. D162 puts a
release on the page with no credentials to hold, and v0.3.4 is the first
one since v0.3.0; D163 makes the seven figures module options; D164
reads a persisted `drain` as `off`; D165 refuses a capability file whose
probe cannot be started and adds `herd-runner check`; D166 writes a
verdict a program can read and puts a runner's build and its suspended
capabilities on the fleet listing. D167 is one this plan did not
foresee: the consumer found a job reported finished while its outputs
were still arriving. **D162 to D165 are in v0.3.4, cut at 17:43; D166
and D167 merged after it and are in no release.** **One gate item is
open** — a requirement reading is still admitted at a cost of zero and
nothing bounds how many run on one runner. And the handover itself was
announced before any of it landed, with the consumer accepting the
inventory of the defects they were taking on, so alternative 3 above
records a plan that was rejected and not the way it went.
`docs/handover.md` §3.0 says where the gate stands, item by item.

**On the number.** This entry is 161 and the six above it are 162 to
167. It took the first free number on 2026-10-23 against a `main` that
ended at D159 — D160 was being appended by PR #54 the same morning — and
then six entries landed while it waited for the owner's word on the
merge. It is appended rather than slid into its numeric place:
`docs/decisions.md` is append-only (invariant 8), and an insertion into
the middle of an append-only file is not a thing a reader of the diff
can check. D148's rule is the one that applies — **a decision number
cited in code is an identifier, not a position** — and D167 ruled the
same way from the other side while this branch was open, counting 161
with 143 and 154 as a gap, "all three under D148's rule". The number is
cited by `docs/handover.md`, by this branch's commit and by the owner's
approval of it; renumbering to 168 would buy one contiguous sequence and
cost three citations that have already been read.

## 2026-10-23 — D168 A tag states the two numbers an operator asks us for

**The first consumer, now the sole operator of the running hub, had to ask us
whether rolling an image back was safe.** Their update script answers that
from `RECORD_VERSION`: a hub reads only a state root whose number is exactly
its own and refuses to start on any other, so a rollback across a move in
that number cannot read the state it is rolling back onto. The number is in
`herd-hub/src/job.rs` and nowhere a deployer can reach it — `herd-hub
--version` does not print it, and no release body carried it — so the script
did the correct thing with an unknown and marked the rollback unsafe, and a
person asked. The answer was read out of the source for them: it is **1**, and
`git log -S'pub const RECORD_VERSION'` shows it has not moved since the
v0.3.0 freeze (D73). That answer should not have come from us. The handover's
whole premise is that a question only this repository can answer is
answerable from what a tag produces, so the number goes in the artefact.

**`PROTOCOL_VERSION` goes beside it, unasked.** It is the same kind of
question with the same kind of consequence — whether a runner and a hub of
two different builds talk at all — and an operator holding two releases wants
both answers in one place. Two lines cost nothing; being asked a second time
costs a message each way.

**They go in the generated half of the body, not in the notice.** Both were
plausible: `deploy/release-notice.md` is quoted into every release and is
where the *meaning* of these numbers belongs, and does now carry it. But the
figures belong in the half the workflow writes, for two reasons. The first is
drift: a number read out of the checked-out tree at build time is the number
the binaries in that same release were compiled from, and a number typed into
a prose file is a number somebody has to remember to change — this project has
already shipped a `SHA256SUMS` covering two binaries out of three because a
glob was written once and not re-read. The second is the ordering D162 bought:
the head half is written and published **with the binaries**, before the image
is so much as built, so a release that loses the archive half still states its
two numbers. Putting them in the image half would make the rollback answer
depend on the step most likely to fail.

**Read with `sed`, not with `nix eval`, and the choice is narrower than it
looks.** The version and the triple come out of `nix eval .#static…` because
the flake genuinely holds them: `flake.nix` reads
`workspace.package.version` once and every package takes it from there. No
flake attribute holds either of these two numbers, and giving it one means a
`builtins.match` over a 2 000-line Rust file inside the file that is the
build's contract. It is a regex over the source either way; the question is
only where it lives. It lives beside the release note it serves, in the step
that already derives the rest, and not in the thing that builds the binaries.
Both constants are declared exactly once as `pub const …: u32 = N;`, which is
what makes a one-line read honest rather than lucky.

**A number that came out empty would be worse than no line at all**, which is
why the step asserts rather than interpolates. The release body promises a
shape — `^RECORD_VERSION: [0-9]+$`, said in the body itself so a script may
rely on it — and a body carrying `RECORD_VERSION:` with nothing after it would
let that script match nothing and conclude nothing while looking like it had
checked. So the step refuses an empty value, a non-numeric one, and two
declarations at once, naming the constant and the file it looked in, before
anything is built. It deliberately tolerates anything after the semicolon, a
trailing comment included: that still declares the same number, and a tag is
the wrong moment to discover somebody added one. Proven the way D162's own
work was proven — the shell blocks extracted from the workflow and run
verbatim, offline, against this tree, with `gh` stubbed: the body comes out
carrying `RECORD_VERSION: 1` and `PROTOCOL_VERSION: 2`, `grep -E` matches both
lines, a declaration deleted, renamed or duplicated fails the step with its
own sentence, and a hand-bumped `7` is read as 7, which is the proof the line
is read and not typed.

**What is deliberately not done.** `herd-hub --version` is untouched: this is
a CI and documentation change, and in any case a binary cannot answer a
question an operator has *before* downloading it, which is when rollback is
decided. Nothing compares this release's numbers with the previous release's —
the workflow does not have the previous release in hand, and the comparison is
the operator's to make, now that both releases state their own. And v0.3.5's
published body is left alone: it is already out and already read, and editing
a release underneath the person deploying it is its own small betrayal. The
next tag carries the lines.

**On the number.** This entry is D168 and the last on `main` is D167. D161 is
still held by an unmerged branch, so the gaps are D143's, D154's and D161's,
all three under D148's rule.

## 2026-10-24 — D169 A reading is bounded, and a lost one is read again

**The incident, twice in one day.** At 03:10:56 the first consumer's nightly
submitted **214 jobs in one second** — 48 `test`, 166 `test-pack`, priority
nightly, commit 923fe476c0de — against a fleet of one runner, hub and runner
both 0.3.5. Every one of those jobs needed its requirements read on a machine
first (D74). A reading holds no slot, empties no host and waits for no probe
(D159), so **nothing anywhere counted them**: the matcher put some two
hundred readings on that one machine at once, on a 20 GiB, eight-thread lend.
The host saturated. The runner's own kvm probe then timed out in a loop from
03:11 to 03:50 — "did not answer within 2000 ms and was killed" — which
suspends the capability (D49), so jobs needing kvm found no runner that could
ever run them. **201 of the 214 ended 125 with no test run**; 6 passed, 5
failed, 1 was memory-capped, and one job waited 6 h 55 for a reading that
never came back.

At 10:09 the batch resubmitted the 203 groups that had ended 125, once each.
The same thing happened at sharper figures: **255 `herd-job` cgroups at once
and a host load of 399 at 10:25**. They stopped the batch at 10:26 and the
machine was normal by 10:29.

**The runner stayed up, and so did the hub.** Nothing crashed either time;
the host was simply buried. That is the honest description and it is a
stronger argument for a bound than a crash would have been — what is being
protected is not a process but a machine that stays usable enough for its
probes to answer inside two seconds and for jobs to go on being placed. 255
is the observed ceiling of "no bound at all".

### 1. A reading takes a place among a runner's readings, and there are two

The cap is **in the claim step's seat accounting**, not a gate in front of
it. `judge` already decides everything one machine can do with one job, from
the labels a runner offers down to the room a job declares, and a second
place that decided what a machine may take would be a second answer to one
question — the defect `out_there`'s own doc comment warns about, where the
first job counted by one and not the other is a machine that reads idle on
top of work. It is read off `Holdings::readings_running()`, which is derived
from the records every pass and from nothing else (D79): there is no counter
to leak, nothing to rebuild after a restart, and a reading gives its place
back by ending, whichever of the eight ways it ends.

Over the cap a reading gets `Verdict::Later`, which is the same "wait, never
fail" every taken slot already gets (E-31): it **queues**, it is never
refused, and it is never moved to a machine that suits it less. That is also
what keeps a queued reading off the dead-runner timer — a reading that is
waiting has no attempt in the air, so there is nothing for the watchdog to
count.

**Two, and it is a setting** (`--readings-at-once`, `HERD_READINGS_AT_ONCE`).
Two rather than one because a reading is cheap and what is wanted is a bound,
not a queue of one: one running while the next is handed out keeps a fleet of
readings moving at the speed of the machine. Two rather than eight because
the figure has to be survivable *on top of* the real work the machine is
already running, and nothing else is counting readings. Zero is refused at
startup, naming the flag: it would be a reading that is never placed and a
queue that never moves. A hub-side figure and not a runner flag, like
`--reading-cap` and `--drain-first-limit` beside it — a reading is the hub's
own job, nobody declared it, and a figure a runner announced would be a
figure the hub had to defend against a runner announcing two hundred.

D159 said "no new knob, no new setting and no policy anywhere in this", and
this is one. The sentence stands for what it was about: whether a reading
*consumes the capability it was placed against*, which it still does not.
What D159 did not settle is how many readings one machine is handed, because
until a consumer submitted a whole pass at once the answer was never more
than a handful. It is a bound on the kind's own concurrency, not a scheduling
policy, and it is one number with a default that nobody has to set.

**The alternative, and why not.** The consumer proposed that a reading take a
slot of the label it reads. It is refused, for three reasons and the third is
decisive. It re-creates D159's convoy exactly: a reading for an exclusive or
host-wide label would wait for real work and then hold the machine empty,
which is the stickiness grievance in another costume. It makes the bound a
property of whichever capability file the fleet happens to ship rather than a
figure an operator can see and set. And **the label it reads does not exist
yet**: `reading_job` places a reading against `labels_from.baseline`, the
declared baseline, precisely because what the commit needs is the thing the
reading is about to answer. There is no slot of "the label it reads" to take
at the moment the decision is made.

### 2. A lost reading is read again, and a busy machine is not a gone one

Two defects turned a buried host into 201 refusals, and the cap alone would
have hidden rather than fixed them.

**`settle_readings` had one branch for every way a reading can fail.** A
reading that printed something that is not an answer, exited non-zero or was
stopped by its cap has told the hub something about the commit, and D74's
rule applies: refuse the job by name rather than place it on a guess. A
reading that was **lost** told it nothing at all. Lumping the two together is
how "a job is never placed on requirements nobody read" came to be said about
jobs whose commits were perfectly fine and whose fleet was merely busy. A
lost reading now puts the job back to where it started — `by` is `None`
again, so the next pass mints a fresh one through the cache exactly as it
minted the first — and the job is refused only when the goes are spent, in a
sentence that says the fleet could not keep a reading alive rather than that
the command is wrong.

**The watchdog believed a clock over the machine.** It ends any attempt
unheard of for `--lost-after` with "runner X stopped answering … so it is not
coming back", and it said that of a runner that was connected and reporting.
A health report names every job its runner is holding and nothing else, so a
connected runner has **already answered the question**. The hub now asks it
before it believes the clock: a report that names the attempt means the
machine is busy, not gone — no go is spent, nothing is requeued, and the
silence is said out loud once with what does bound it (the reading cap, or
the job's own time cap on that machine, invariant 2). A report that does not
name it means the attempt is gone from a machine that is here, which is both
truer and *faster* than the timer. A connection nothing has come back on yet
decides nothing, and is bounded by the link check on that same connection. A
runner that is **not** connected is untouched: the clock decides, and a
genuinely dead machine is still noticed in 60 s. The link-silence check above
it is not changed at all.

**What could not be confirmed, said plainly.** A health report that names an
attempt already refreshes that attempt's clock — `is_current` does it for
every frame it accepts, and the runner's `running_jobs` names every job it
holds — so a runner reporting on time, inside the threshold, with the
attempt still in hand, cannot have had it declared lost by that path alone.
Two routes remain, and both are what the change above answers. One is a
report whose **processing** straddles the threshold: the attempt's clock is
set while the report is being walked and the link's is set when it is done,
so a hub spending a minute on two hundred entries has the first clock
expire while the second is young. The other is a link dropped and redialled:
the new connection's clock starts fresh while every attempt's is still old,
so the attempts are written off one by one against a machine that is
plainly back — which is exactly the "attempt 1 of jXXXX is not the attempt
this hub is waiting for" the runner logged all night. Neither was reproduced
from the journal; both are closed by asking the machine instead of the
clock.

**And one line of latency that only showed up once the rest worked.** A job
the hub gives up on — goes spent, ended `lost` — did not judge the queue
afterwards, where the requeue beside it always has. Nothing noticed while
the only thing waiting was a slot, because the next event along would find
it; with D169 the thing waiting is a *reading to be minted*, and the next
event on a quiet fleet is the restatement cadence. The end-to-end test
measured it: a hundred seconds of a nightly nobody could account for. A job
that is over is holding nothing, so the queue is judged, at both ends of
that function.

### The budgets, which must not quietly spend each other

Three now, and they count three different things off the record (D79), which
is what keeps them apart:

* `Policy::attempts_allowed` (E-21, D39) counts goes at **a runner that went
  away**, from the attempts on the record that ended without a verdict.
* `--requeue-budget` (D76) counts goes at **a capability that turned out to
  be taken**, from `Attempt::requeue_code`. A lost reading sets no requeue
  code, so it can never spend this.
* `Reading::lost` counts **readings of one job lost to a machine that went
  away**, on that job's own record. The figure it is weighed against is
  `attempts_allowed`, deliberately: a job gets as many goes at a reading lost
  to a machine that disappeared as E-21 gives it at the machine itself. Two
  figures for one idea would be a knob nobody could reason about.

So the worst case is bounded and visible: `attempts` attempts at each of
`attempts` readings, four in all at the defaults, and then one refusal naming
both. No new requeue path was added — the watchdog's new answer *stops* a
requeue, it never starts one — so nothing here can loop.

**Nothing crosses the wire.** `PROTOCOL_VERSION` stays at 2 and
`RECORD_VERSION` stays at 1 (D73). The one new field, `Reading::lost`, is
`#[serde(default)]` and is not written when it is nought, so a record this
hub writes reads on an older one as a job none of whose readings was lost —
which is exactly what that hub could have written.

### What is proven

`herdctl/tests/bounded_readings.rs`, against a real hub, a real runner and
the fixture consumer's own four executor steps on a real commit. Eight jobs
submitted at once, each needing a reading of its own, and the overlap of the
readings' attempts computed **from the records** — the same fact the claim
step counts: at most two ever ran together, and at least two did, so a run
where nothing overlapped fails saying it proved nothing. Every one of the
eight ends with its own verdict and not one `lost`. The second test takes the
machine away under a reading as many times as E-21 gives it goes, brings it
back each time, and asserts the job runs: one reading lost on the record, a
second reading that answered, and the job's own exit 0 where 0.3.5 gave 125.
Both were run against `main` and both fail there, in the terms this entry
is about: the first measures **eight readings at once** on the one runner,
and the second reads back the job's own "is refused and was never placed".
Beside them the arithmetic is unit-tested where it
belongs — the cap in the claim step, including four readings through one pass
being two claims and two waits; the budget on `Reading`; the watchdog's
judgement as a pure function of what a runner last said.

## 2026-10-24 — D171 A wait is written down when its cause changes

**The incident, an hour after the release that caused it.** 0.3.6 shipped
D169 at about 11:10; the first consumer installed it and ran a full pass at
12:10:20 — 214 job groups, one runner, `--readings-at-once` at its default
of 2. **The bound worked**: the runner held at most four `herd-job` cgroups,
load 16–21, and not one job ended 125. That is the whole of what D169 was
for, and it held.

Then the hub stopped answering. From about 12:14 `herdctl runner list`
timed out — 10 s in their collector, then three tries at 30 s and one at
180 s, every one `rc 124`. The runner's journal says the other half: *"the
hub has said nothing for 60s, so this link is dead even though its socket is
open"* at 12:14:15, 12:15:53 and 12:17:29, redialling each time, and at
12:18:29 *"j0000002485 ended while there was no hub to tell"*. By then the
runner held **nought** cgroups against a queue of **184 waiting**: the pass
had stopped moving. The hub container was not busy — 1.8–3.9 % of a CPU,
209 MiB, 13 PIDs. It was blocked, not saturated.

**What was actually blocking it**, from the thread states and the state
volume they captured before restarting: of the thirteen threads, exactly
one was always in `D`, uninterruptible disk wait, and it moved between two
different tokio workers across samples; the rest were waiting. In two
minutes the hub modified **775 files** — `index/state.json`, and for every
queued job both `jobs/<id>/job.json` and `jobs/<id>/events.jsonl`. One
queued job, `j0000002656`, had accumulated **94 lines** in its
`events.jsonl`, each one a full *"stays queued: queued 65th of 165 …"*
explanation. Their hub log carried **20 683** `stays queued` lines since
12:10, 1 931 of them inside one three-minute window.

### The defect: a figure that moves inside a sentence that is compared

A dispatch pass walks the queue in order and, for every job it leaves
queued, calls `say_it_waits` with E-04's sentence. That sentence is two
things wearing one coat. The **cause** is a fact about the job — the
reading it waits for has not answered, the capability it needs is
suspended, the runner it sticks to is busy — and it changes rarely. Around
the cause the sentence wraps a snapshot of the whole queue: `queued 74th of
184`, and the three jobs ahead of it by name.

`record_waiting` wrote only "when it is news", and judged news by comparing
the whole sentence. So one job arriving, starting or ending changed the
total — and therefore the sentence — of every job standing still behind it.
On a queue of 184 that is 184 rewritten records and 184 appended history
lines per pass, each one `sync_all`'d with its directory, for a job about
which nothing whatever had happened.

Before D169 this could not bite. A reading was placed the moment it was
minted, so a one-runner fleet never stood a queue deeper than a job or two
and the per-job cost was paid twice. D169's bound is correct and is the one
thing that worked that night; what it changed is that the queue is now
*deep*, and it put a cost that had always been linear in the queue in front
of a queue two hundred long.

The rest follows without any further defect. A dispatch pass is a
synchronous function called straight from async callers, so a pass that
spends seconds in fsync is a tokio worker that spends seconds unavailable —
and this hub has a dozen callers that each start one, serializing on
`State::dispatching`, each occupying the worker that called it. With no
worker left, nothing is polled: not the answer to a health report, which is
what tells a runner its link is alive, and not the accept of the requester
asking for a fleet listing. The runner's 60 s silence limit then expires, it
redials, and the redial starts another pass. Measured on the host this was
written on: one pass over 400 queued jobs took **3.21 s** writing a line for
each and **62 ms** writing none.

### The decision

**News is what changed about this job, not what changed around it.** The
sentence is three things, and only two of them are facts about the job it
is written on: its **cause**, its own **standing** — the level it was
submitted at and the level the queue weighs it at now (E-18) — and its
**place among the others**. `say_it_waits` now takes the cause and the
standing beside the whole sentence, compares cause-and-weighed-level
against the pair it last wrote for that job, and touches the disk only
when that has changed. Everything the record does keeps doing it: the
first judgement of a job is written, a job that ages says so on the pass
it happens, a job nobody is following still says on its disk why nothing
is happening to it (E-53), and the line in `events.jsonl` still carries
the whole sentence with the figures that held when it was written.

The **countdown** to the next level is deliberately outside the key, while
the level it counts down to is inside it. A countdown is a clock and not an
event: including it would make every queued job a write a minute, which is
the same defect at a fiftieth of the rate. The cost is named rather than
hidden — between two level changes the record's "rising to merger in 12m"
decays, and a reader of `job.json` alone sees a countdown up to one aging
step old. What a requester is *told* does not decay, because it comes from
memory.

**The sentence is not lost; it moves to memory.** `Peers::waiting` holds,
per queued job, the whole sentence the last pass built, and that is what
every reader is given: the restatement clock E-04 asks for, and a `herdctl
wait` that attaches an hour later. So a requester still learns its position
and what is ahead, and learns this minute's, not the one that held when its
cause was last written. The map sits beside the others in `Peers` for the
reason they are there — it is a fact about a stream, not about a job — and
it is pruned by the pass that fills it, against the queue it has just
walked, so it is never larger than the queue and nothing survives the job
it is about. A hub that restarts has an empty one, and its first pass
writes every queued job's reason once, which is right: a new hub has told
nobody anything.

**E-04 and E-36 are kept whole, and were checked rather than assumed.**
E-04 asks that a wait be explained with figures and said again at least
every 60 s; its coverage row already says the record keeps the news-only
rule and the *stream* carries the cadence. That is the mechanism this
leans on — "ask and be told" was already the design, and only the record's
definition of news changes. E-36 asks that every decision be explained,
and no explanation is withdrawn: a placement still writes its line, a job
that can never run still ends naming why, and a queued job still carries
its reason on disk and answers for it when asked.

### What was rejected

* **Weakening D169's bound.** It is the one thing that worked, and the
  symptom it would hide is not the one it caused.
* **Writing the position on a slower clock.** It keeps the figure fresh on
  disk at the cost of 184 fsyncs a minute of a non-event, and the history
  stays unreadable — 7 000 lines over a forty-minute pass instead of
  20 000.
* **Dropping the position from the sentence.** Then nothing stale is
  written, and E-04 loses the figure it asks for by name.
* **Keeping the figures on the record and comparing a stripped copy.** One
  truth, two spellings, and the next figure anyone adds to the sentence is
  a defect that returns silently.

**What is still paid, and it is bounded.** A whole queue submitted in one
second ages in one second too, so every aging step every queued job is
news at once: 184 records written in one burst, which on the figures above
is about 1.5 s of fsync every thirty minutes at the default step. That is
a thousandth of what this entry removes, it is a real event every time,
and it is the thing the next entry — the one that moves persistence off
the reactor — would make free.

### The deeper defect, deliberately not fixed here

A tokio worker in `D` state says the hub's persistence is not insulated
from its reactor at all: every dispatch pass, every store write, runs on
the runtime. This entry does not fix that, and the fix above does not
depend on it being fixed — it takes the pass from 3.21 s to 62 ms at 400
queued jobs, which is three orders of magnitude of headroom on the figure
that mattered. Moving the pass off the reactor means either `spawn_blocking`
at a dozen call sites, several of them synchronous functions reached from
synchronous functions, or a dedicated pass thread fed by a channel — and
both change how a pass's errors are returned and when `dispatching` is
held, which is the guarantee D100's test is built on. That is its own
decision, with its own tests, and not a thing to do in an hour against a
stopped integration.

**0.3.6 was ours and this follows it by an hour.** The two tests shipped
with D169 covered the incident D169 was about — they proved the bound and
the second reading — and neither stood a deep queue for longer than it took
to prove the point, so neither could see a cost that is only paid per
queued job per pass. The gap is recorded here as the shape of test that was
missing, not as a one-off: a bound that makes a queue deep is a change to
what every pass costs.

### What is proven

Two tests in `herd-hub/src/server.rs`, both run against `d57f23a` — the
v0.3.6 tag as the consumer installed it — and both fail there.

* `a_queue_that_moves_writes_nothing_about_the_jobs_standing_still_in_it`
  stands 120 jobs against no runner, lets one more arrive before each of
  twelve passes, and counts the `waiting` lines in one watched job's
  `events.jsonl`. Neither its cause nor its level changes, so the count is
  **1**: the line its first judgement wrote. On v0.3.6 it is **13**. The
  same test then
  asserts the other half — that a requester is told the current total while
  the record carries the one its cause was written with — so the trade is
  pinned where it can be seen rather than described here and nowhere else.
* `a_deep_queue_leaves_the_runtime_a_worker_to_answer_with` stands 300
  jobs, runs two concurrent dispatch passes on a two-worker runtime, and
  asks the runtime — from outside it, so the measurement cannot be starved
  by what it measures — to poll a task that does nothing. The bound is two
  seconds, a requester's patience and not a figure about disks. Measured:
  **17.3 s** on v0.3.6, **0.23 s** with this entry.

`one_pass_over_a_large_queue_is_measured_and_the_figure_is_printed` keeps
printing the two pass costs beside each other, and its second figure now
means what it always claimed to: what a pass costs when the queue has
moved, rather than what it costs when nothing at all has happened.

**Nothing crosses the wire and nothing changes on disk.**
`PROTOCOL_VERSION` stays at 2 and `RECORD_VERSION` stays at 1: no field is
added to a record, and the field this is about is written less often and
otherwise unchanged. A 0.3.6 record reads here, and a record this writes
reads on 0.3.6.

**On the number.** This entry is D171. D170 is held by an unmerged branch.

## 2026-10-24 — D173 The orchestrator briefs and verifies; agents write

**Context.** `CLAUDE.md` (2026-10-13) said "Claude works alone. A
sub-agent only for one long, isolated task, never several in parallel".
The owner said otherwise on 2026-10-15 (*"tu es orchestrateur, tu lances
des sous-agents"*, *"autant d'agent que besoin"*) and again on 2026-10-23
(*"un agent toujours tu est orchestrateur"*), and those corrections lived
only in Claude's auto-memory, which no execution agent loads. Between the
two dates the orchestrator's own shell ran 60 `sed -i`, 29 `git commit`,
62 `git merge`, 47 `gh pr merge`, 12 `git tag` and 14 `gh release`. The
independent audit of 2026-10-24 found the contradiction, the unversioned
rules, a scratchpad collision, a `pkill -f` that killed a neighbour's
suite, two branches allocating one decision number, and no list of owed
work or waiting decisions that survived a session.

**Choice.** The organisation is multi-agent and the contract is in the
repository, not in memory. The orchestrator (main session) plans, briefs,
verifies evidence, escalates and reports; it never edits a repository
file, commits, merges, resolves a conflict, tags or releases. Five roles
under `.claude/agents/` do that work, one bounded task each, in their own
worktree, ending with a dated handoff under `docs/orchestration/history/`.
`docs/orchestration/board.md` is the one list of open work and waiting
decisions; `docs/status.md` points at it. `herd-merger` is the only writer
of this file: a branch writes `D-NEXT`, the merger allocates the next free
number at integration, one branch at a time; gaps stay (D148).
`scripts/check-docs.sh` rejects a duplicate number, a malformed header, a
leftover `D-NEXT`, a board out of shape and a contract that says the
orchestrator works alone; `scripts/check-orchestration.sh` proves each of
those checks goes red. A `PreToolUse` hook refuses the main session's
writes and git/gh write verbs — best effort, documented as such in
`docs/orchestration/permissions.md`, with the review of traces as the
real protection. Herd is shipped, not operated (D161) is restated as
invariant 9 so that no injected persona can override it.

**Alternatives rejected.** Keeping the rules in auto-memory: not loaded by
agents, not versioned, lost on a `CLAUDE_CONFIG_DIR` move (it happened
between 2026-10-15 and 2026-10-23). A `permissions.deny` on `Edit`: it
applies to agents too and would paralyse `herd-step`. One file per
decision now: it does not remove the allocation race by itself and costs a
168-file migration; analysed in `docs/orchestration/decisions-structure.md`
and left to the owner (ORCH-003).

**Decided the same day.** Alexa, asked ORCH-001 through the orchestrator,
answered *"non il peux merge par defaut"*: `herd-merger` merges any PR whose
CI is green and whose Evidence block is filled, without a per-PR word and
with no excluded path. Pushing a tag and publishing a release are a separate
act and stay on Alexa's word, each one singly.

**Still the owner's.** The invariant wording (ORCH-002); the journal's
layout (ORCH-003); the shared `CLAUDE_CONFIG_DIR` (ORCH-030).

**On the number.** This entry is D173; `origin/main` ends at D171, PR #67
holds D170 and PR #70 holds D172, so 173 is the next free number above
both. Written by the remediation agent under its bootstrap exception; from
here on only `herd-merger` allocates.

## 2026-10-24 — D172 A hotfix is installable in five minutes, and says so

**Measured, on the day this was written.** v0.3.6 went out at 09:49 and put
a regression on the first consumer's only runner; v0.3.7 undid it. The clock
from the fix being written to the fix being installable: PR #68 opened
11:05:23, merged 11:47:25; PR #69, which only bumped a version, opened
11:55:12 and merged 12:03:27; the release run finished 12:08:47. **Sixty-three
minutes**, of which the build that produced the bytes was **five minutes and
nine seconds** — eight per cent. The other fifty-eight were two pull
requests, two `ci.yml` runs at 362–490 s each, two merges, a conflict in this
very file, and the gaps between a person noticing one step had finished and
starting the next. None of that found anything. The regression it was
guarding against had already shipped past it that morning.

**So the lane is: a `v*.*.*-dev*` tag on any branch publishes a pre-release.**
`.github/workflows/release-dev.yml` triggers on that tag and does nothing
itself but call `release.yml` with `dev: true`. The observed cost of a
`release.yml` run over the last four tags is 228–347 s, so a dev build is
**four to six minutes from `git push --tags` to a file somebody can
download** — and on the morning above it would have put the fix on that
runner at about 11:10 instead of 12:09.

**It calls the release workflow; it does not copy it.** This is the whole of
the design and it is worth saying why, because a nine-line file that reads
like a stub is easy to "improve" into a self-contained one. A dev build is
the first thing anybody reaches for in an emergency, and a lane that builds
*almost* like the release path is a lane that can clear a fix which then
fails on the real tag an hour later — the exact failure the lane exists to
prevent, reintroduced one layer down. So `workflow_call` was added to
`release.yml` and a dev build is that build: the same
`nix build .#static` and `nix build .#hub-image`, the same four-way static
assertion, the same `release-smoke.sh` on `archlinux` and on `alpine`, the
same `image-smoke.sh` against the loaded archive, the same `SHA256SUMS` and
its count. Two inputs exist and nothing else changed: `dev`, and `tag` for
the hand-run that already existed. Both are empty on a tag push and on the
hand-run, so every `$DEV` test is inert on the path that cuts real releases —
proven the way D168's work was proven, by extracting the body step's shell
and running it offline with `gh` stubbed: on `DEV=false` the body comes out
byte-identical to today's, and the only delta on `DEV=true` is the preamble
prepended to it.

**Three properties the lane may not lose.**

*It is a pre-release.* `--prerelease` is forced by `$DEV` and no longer rests
only on the tag carrying a hyphen. The first consumer's updater asks this
repository for a release; a dev build that could answer that question would
put an unreviewed hub on a fleet because somebody spelled a tag without a
suffix. The flag is what makes taking a dev build a decision.

*It carries the two numbers.* The step D168 added reads `RECORD_VERSION` and
`PROTOCOL_VERSION` out of the source with `sed`, refuses an empty or
non-numeric value before anything is built, and writes them as
`^RECORD_VERSION: [0-9]+$`. It is reused unchanged and unconditionally. This
is not decoration: their updater **halts when a body names no number**, so a
dev build without the lines would stop their automation rather than be
ignored by it — the one way a lane meant to speed things up could stop them
altogether.

*The body says what the build is, first.* Above the install lines, before
anything a reader could copy: this is not a release, it came off branch X at
commit Y, there was no pull request and no review, and the merge gate did not
run on it here. The branch is asked of GitHub's
`commits/{sha}/branches-where-head` rather than of git, because a tag push
checks out one commit and fetching a history to learn one name would cost the
lane the seconds it exists to save; an empty or failed answer is printed as
one and never guessed at, and never fails the release.

**What the lane trades away, stated plainly.** `nix flake check`, `cargo
fmt`, `cargo clippy` and `./scripts/test.sh` do not run in GitHub on a dev
tag. They are run on the branch, on the machine the change was written on,
and **nothing checks that they were**. That is a real safety net removed, and
the argument for removing it is not that the net never catches anything — it
is that this net is being asked to guard the wrong thing. It guards `main`,
where a mistake reaches everybody and is expensive to withdraw. A dev build
reaches one machine, by name, chosen by an operator who has just been told in
the first paragraph of its body that nobody reviewed it, and withdrawing it
is `docker load` of the previous archive. The cost of a bad dev build is one
watched host and a rollback; the cost of the net is an hour on every hotfix,
paid whether or not anything is wrong. On the day measured above it was paid
twice and caught neither regression. **A dev build is also never promoted** —
a change that proves itself on one still goes through a pull request, the
gate and a tag of its own — so nothing the gate would have caught can reach
`main` by this route. The net is not removed; it is moved off the critical
path of finding out.

**And one thing the lane deliberately does not skip.** It would be easy to
drop the two distribution smokes and save ninety seconds. They are the single
most valuable minute and a half in the file: they are the only place anything
runs these binaries on a host with no Nix, which is every host that matters,
and the owner's stated reason for this lane is to find *new* problems in real
conditions. A lane that skips the real-conditions proof to go faster is
faster at nothing worth having. The same goes for the static assertions and
the image smoke, which cost seconds.

**Why one tag starts exactly one workflow.** `release.yml`'s trigger gained
`"!v*.*.*-dev*"` beside its `"v*"`. Without it a dev tag would match both
files and start two runs racing to create one release, and the loser's error
would be the first thing anybody read in an emergency. The exclusion is the
same pattern the dev lane includes, so the two cannot drift apart silently.

**Why no branch input on the hand-run.** `release-dev.yml`'s
`workflow_dispatch` takes a tag that already exists and nothing else. A
release has to hang on a tag — that tag is the only handle by which an
operator holding a strange binary six months from now gets back to the exact
commit — and a dispatch given a branch would have to create one, which makes
a workflow that mints tags out of a lane that is supposed to be nine lines.
The case a branch input would serve is already served: `release.yml`'s own
dispatch with an empty tag rehearses the whole build from any branch and
creates nothing. So the dispatch here is the recovery door only — a dev tag
whose run died halfway is republished without pushing a second tag for the
same commit.

**The tag names the workspace's version, and that is not negotiable either.**
`release.yml` refuses a tag that is not `v$version` or a suffix of it, before
building, because the file names and the image tag come from
`Cargo.toml` and would otherwise lie about what is inside them. So a hotfix
branch bumps `workspace.package.version` to the version it is fixing towards
and tags `v<that>-dev1`. That is one line, and the branch needs it to become
a release later regardless; cutting the assertion to save it would buy a dev
build whose `herd-hub-image-0.3.7.tar.gz` contains 0.3.8, which an operator
comparing two archives could not untangle.

**What is deliberately not done.** No cache was added to speed the Nix build.
A binary cache in the release path is an optional thing that can fail, and
D162 is this project's own receipt for what an optional thing in the release
path costs — three tags that produced nothing. No artefact was dropped and
the image is still always built: both would make a dev run differ from the
real one, and the point of calling `release.yml` was that it does not. Those
three are worth about two of the five minutes between them and are written
down here as rejected rather than forgotten, so that the next person who
wants the lane faster starts from the reasons and not from the stopwatch.

**On the number.** This entry is D172. D170 is held by an unmerged branch and
D171 is the last on `main`.

## 2026-10-25 — D175 A refused file is named, never erased

**The defect, measured and not supposed.** A verdict carrying two inline
output files, one of whose bytes do not hash to the sha256 beside them,
produced: `herdctl` exit code **0**, one manifest entry of two on
`job.json`, and one file of two under `--outputs-to`. The bench that
measured it is `herdctl/tests/inline_digest_mismatch.rs` and the figures are
in `docs/remediation/04b-integrity-and-containment.md` §1. **0 is the one
code that means "everything you asked for happened"**, and a requester that
asked for a job's evidence read it on a collection it did not get.

**The chain, in four links.** `record_outputs` pushed the manifest entry
*after* the digest check, so a refusal `continue`d past the push and the
entry was never written. `herdctl` counts its shortfall **off the manifest**
— `missing = files.iter().filter(|f| !f.is_held()).count()` — so an entry
that does not exist is not missing. `missing` was 0, `files_short` was
false, and `code_for` had nothing to raise. **The one mechanism D167 built
to stop a short collection reading as a whole one was bypassed by the one
path that removes the evidence of the shortfall.**

**The rule it broke was written two lines below it.** The same function
states: *"the manifest first, the bytes if they are here … what a reader of
`job.json` must never find is a path that is not there **and** no way to
tell why"*. That rule is repeated in `job_outputs`, in
`herdctl::write_outputs`, and in the streamed path D148 built it for. Every
other absence — a file still crossing, a collection the sweeper took, a file
the store cannot read back — keeps its entry and names its absence. The
digest case was the single exception in the whole design of outputs, and it
was not an exception anybody decided: no sentence anywhere states it.

**The correction is at the cause: the entry is written down before the
bytes are judged.** Two statements swapped in
`JobDirStore::record_outputs`. The refusal still refuses the bytes, still
names both digests on `skipped`, and still never writes the file. What it no
longer does is erase the only trace a requester counts. The file is then
*announced and not held*, which is the shape the rest of the system already
knows how to read: `herdctl` counts it short, exits 2, and says which path
and why.

**Not a filter in `herdctl`, and not a new field.** Two alternatives were
rejected. Making `herdctl` exit non-zero whenever `skipped` is non-empty
would turn D131's ordinary sentences — a symlink, a fifo, a name that is not
UTF-8 — into failures, and those are decided facts about one file that
D131 ruled are *"a loud sentence and never an exit code"*. Carrying a new
"refused" flag on the record or the wire would cost a field for a fact the
manifest already expresses by an entry that is not held. **Neither
`PROTOCOL_VERSION` nor `RECORD_VERSION` moves**, no frame gains a field, and
no record written before this reads differently after it.

**The neighbouring case is left alone, deliberately, and it is not the same
case.** A file whose *path* the hub will not join onto its own directory —
`../escape.json` — keeps no manifest entry, and its requester still reads 0.
That drop is decided and right: the manifest is the list of paths this store
joins onto its own directory, `write_output_chunk` resolves a chunk against
it and `job_outputs` reads a file back by it, so a path that must never be
joined must never be on it. The record names the file on `skipped` and that
is where it exists. **What was never decided is the exit code**, and
searching `docs/decisions.md` finds nothing that rules on it: D131 settled
the record, D167 settled the short collection, neither mentions a path the
hub refuses. It is inherited from the drop, exactly as the digest case's 0
was. It is now pinned by a test that says so in its own comment, so that
changing it is a decision and not an accident. **Alexa's to take**, with one
recommendation: a hub-refused path should cost the zero too, by the same
reasoning as this entry, and it needs a shape that does not put an
unjoinable path on the manifest.

**And the stopgap, which is not the fix.** Every sentence `herdctl` had
about a job's files — the refusal, the `skipped` lines, the per-path "not on
the hub" lines — reached stderr and nothing else. A wrapper that wanted to
protect itself had to grep prose, which is the parsing D166 added a verdict
document to make unnecessary. They are now in `Verdict::said` as well as on
stderr, in the order they are printed. They do **not** move the exit code:
a fact about one file is never a verdict about the job (D131, D20).

**Impact statement for the first consumer.**

*What changes.* Jobs whose verdict carried an inline output file whose bytes
contradicted their own sha256 used to exit **0** with that file silently
absent. They now exit **2** — `herdctl`'s own code for "this command did
part of what it says" (D167) — with the path named on stderr and in the
verdict document. Nothing else changes: a failing build still exits 1, a
capped job still exits 124, a job with no verdict still exits 125, and a job
whose files all arrived still exits 0. The job's own verdict is untouched in
every case; 2 appears only where 0 would have, and the verdict document
labels it `"exit_code_from": "herd"`, never `"job"`.

*When it can happen.* Only when a runner's frame disagrees with itself: a
runner of a different build, a corrupted spool read, memory corruption. A
runner of the same build computes the length and the digest from the buffer
it sends and cannot produce it. We have no evidence it has happened on your
instance; nothing on a record distinguishes it after the fact except the
`skipped` sentence, which is on `job.json` and always was.

*What a requester adapts.* If your tooling treats any non-zero `herdctl`
exit as "the build failed", a 2 will now read as a failure where it used to
read as a pass. That is the intent — the output set really was incomplete —
but the right reading is "the build's verdict is in the document; this run
did not bring back everything it was asked for". Read `exit_code` together
with `exit_code_from` from `--verdict-to`: `"job"` is the build's answer,
`"herd"` is ours about this invocation. You can also drop any workaround
that compares the file count under `--outputs-to` with what you expected:
that is what the 2 now tells you.

*Going back.* Reverting this commit restores the old behaviour. **A record
written by the corrected version stays readable**: the only difference is an
extra manifest entry for a file that is not on disk, which is the same shape
as a streamed collection still crossing, and `RECORD_VERSION` is unchanged
at 1. An old `herdctl` reading such a record counts that entry as missing
and exits 2 — the corrected behaviour, from the record alone — so a rollback
of the hub is clean and a rollback of `herdctl` alone does not restore the
false success for jobs already recorded. Nothing has to be migrated, deleted
or rewritten in either direction.

**One consequence worth naming, and it is pre-existing.** A job holding a
manifest entry whose bytes never landed never satisfies
`collection_is_whole`, and the output sweeper only takes whole collections
(D152), so such a job's kept files are never evicted under the output
budget. That is already true of every streamed collection that never
completed; this entry adds the inline digest case to it. Not fixed here: it
is the sweeper's rule, not this defect's.

**On the number.** This entry is D175. D174 is held by the unmerged branch
of PR #72 and D173 is the last on `main`.

## 2026-10-25 — D176 A verdict says which revision it really ran

**Context.** Nothing a runner sent carried a revision. `StepRun` held the
step, the start, the duration and the exit code; the commit existed on the
way *out* only — `Declaration::commit`, the text of the `fetch` and `up`
lines `herdctl` filled, and the deduplication key. So "this verdict is about
commit X" rested entirely on the assumption that a `fetch` exiting 0 left the
tree at X. A `fetch` with a fallback, a stale mirror, a `switch` whose
failure an `&&` swallowed, or a shared cache all produce a verdict attributed
to a commit the tree was never at, and **nothing on the record could
contradict it afterwards**. There was no oracle for the first consumer's I01
at all: not a weak check, but nothing to check against.

**Choice.** A new placeholder, `{ran}`, filled by the runner like `{job}`,
`{in}` and `{out}`, naming a path inside the job's own workspace that the
consumer's `fetch` hook may write the revision it really reached into. The
runner reads it **after the `fetch` step**, trims it, bounds it, and puts it
on that step as `ran_commit: Option<String>`. The hub compares it with
`Declaration::commit` when it records the step, and writes `CommitRun {
declared, ran }` onto the attempt. The requester is told the same thing in
the verdict frame, and `herdctl`'s verdict document carries
`declared_commit`, `ran_commit` and `commit_diverged`.

**A divergence is recorded and never refused.** The job's verdict, its exit
code and its outcome are exactly what they were: the verdict is the job's and
a side channel does not become one (D20). A refusal would turn a reporting
gap into a new failure mode on the day the field ships, on a fleet where some
runners report and some do not — and a hub that refuses on a *mismatch* is
the recording hub plus one `if`, which can be added later on the divergences
this field makes visible. What was missing was not enforcement; it was a fact
on the record.

**Absence means "did not say", never "it matched".** `None` on the step,
`"ran": null` on the record, `null` on the verdict document, and
`commit_diverged: null` rather than `false` when the comparison cannot be
made. A default that agreed would read an older runner's silence, and every
consumer's first day, as a clean bill of health — the same mistake a plain
`Vec` default would have made of the health report's `suspended` (D169's
neighbourhood, and D23's rule).

**No protocol break and no migration.** `PROTOCOL_VERSION` stays **2** and
`RECORD_VERSION` stays **1**, asserted by a test. Every addition is
`#[serde(default)]` and means, absent, exactly what its default says, which
is the additive rule this project already wrote down twice. The record has no
step type of its own — it holds `Vec<herd_proto::StepRun>` — so the wire
change and the record change are one change and cannot be staged apart.

**What it costs on a rollback, said plainly.** A hub of an earlier version
**reads** a record carrying the field and **erases it the first time it
writes that record back**. Serde drops what it never decoded and has nothing
to re-emit. The reachable paths are ordinary: `record_state` on any job still
in flight, and `note_outputs_fetched` on a finished job, at the first fetch
of its outputs. A rollback therefore reads every record and preserves this
field on none of the ones it touches. For a diagnostic field that is an
acceptable cost; it is written here and asserted in
`herd-hub/tests/ran_commit_across_versions.rs` so that it is known rather
than discovered.

**Why a token and not an environment variable.** Both would hand the hook a
path. They differ on the day a consumer updates its declaration before every
machine in the fleet has been restarted. An older runner leaves `{ran}`
unfilled, the shell treats the brace as the ordinary filename it is, the hook
succeeds, and the runner reports nothing. An unset `$HERD_RAN_COMMIT_FILE`
makes the same line `> ""`, which fails, and a failed `fetch` is a job that
never started — every job of that consumer, on every machine not yet
restarted. The first failure mode is a stray file in a workspace that is
about to be reclaimed; the second is a fleet-wide outage. Both are asserted
with a real `/bin/sh` in that test file.

**Rejected.** *The runner runs `git rev-parse` itself* — Herd would learn one
VCS and stop being project-agnostic (E-11: Herd's code names no consumer
command); the token gets the same fact without Herd knowing what produced it.
*The hub re-resolves the commit* — it would need the repository, the network
and VCS knowledge (invariants 1 and 3). *A fifth `verify` step* — four steps,
fixed order, no conditions; a step with a condition is the workflow language
invariant 1 refuses to be (D77). *The hook prints the revision and the runner
greps the log* — a job's log is the bytes the job's process wrote (D18), and
parsing prose is what D166 exists to make unnecessary. *Refusing a divergence
in this increment* — above. *A new `events.jsonl` variant for a divergence* —
`JobEventKind` has no `#[serde(other)]`, so a new variant is the one addition
to the record that an older hub could **not** read; the divergence is a field
on the attempt instead.

**Herd runs no `git` and parses nothing.** It hands the consumer's hook a
path, reads back one line, checks that it is one printable word of at most
128 characters, and compares it with the declared commit byte for byte. That
is the same shape as `labels_from`, where the consumer's own command answers
and Herd reads the answer strictly. A file that is there and unusable is
reported as **silence** on the record and said out loud in the job's log, so
a hook writing the wrong thing does not look like a hook writing nothing.

**What the consumer can do today, with nothing shipped.** Append
`&& [ "$(git rev-parse HEAD)" = "<hash>" ]` to the `fetch` hook. A divergence
is then a preparation failure — the job never runs, exit 125 — instead of a
wrong verdict. It is containment and not a fix: the guarantee sits in their
file, Herd's record still says nothing, and a consumer who changes that line
loses the protection with nobody noticing. It is worth asking for anyway,
because it protects the next build and this entry protects the one after the
deployment.

**On the number.** This entry is D176. D175 is the refused-file decision
integrated in the same pass, D174 is held by the unmerged branch of PR #72
and D170 by that of PR #67.

## 2026-10-25 — D-NEXT A runner's live jobs are keyed by job and attempt

**Decision.** `herd_runner::Session`'s map of the jobs a runner has in hand
is keyed by **(job, attempt)** and no longer by job id alone. Every
operation on an entry names both; an acknowledgement, which names no
attempt, resolves to the attempt that is *holding an end*; and
`Session::end_job` takes an attempt rather than an `Option<u32>` it had to
compare after the lookup.

**The defect.** A runner can hold two attempts of one job at once, and the
path is ordinary rather than exotic: the link drops, the hub's watchdog ends
the attempt it has stopped hearing about and queues the job again, the job
goes on running on the machine, and the next attempt of it is offered to
that same machine the moment it redials. Keyed by job id, `take_job`'s
`insert` *replaced* the first entry. Three consequences, every one of them
seen in a real run:

- **a running attempt stopped being counted.** `jobs_running()` reads the
  map, so a machine running two processes reported one. The place it did not
  have was then available to the next assignment, which is invariant 2
  broken by the bookkeeping rather than by a job.
- **output was numbered under the wrong attempt.** `send_chunk` looked the
  job up and sent the chunk under whatever `attempt` the entry carried, so
  the first attempt's output arrived at the hub labelled as the second's —
  where it is refused as superseded, and the log of the attempt that is
  really running is short with nothing saying so.
- **an acknowledgement took the live attempt out.**
  `FromHub::JobResultAck` carries a job id and nothing else, so `release`
  removed whatever entry stood under that id. Once a second attempt had been
  assigned, that was the attempt that was *running*: it then ran with
  nothing counting it, nothing naming it in a health report, and a hub that
  wrote it off for silence while the process was working perfectly. The same
  `insert` had already destroyed the first attempt's unacknowledged verdict,
  which D19 says is never destroyed.

**Why the key and not a guard.** A guard in `take_job` — refuse a second
attempt of a job this runner is already running — is also wanted, and is a
separate decision about what a runner may accept. It does not make this one
unnecessary: a map that cannot represent what is on the machine is a map
that lies about it whatever the admission rule is, and the three defects
above are all failures of representation. Keyed by both, the four
attempt-aware lookups that used to be "find by id, then compare the number"
become one lookup that cannot find the wrong thing, and `JobInHand` loses
its own `attempt` field — two places to read an attempt number from are two
numbers that can disagree.

**What it does not fix, and must be said.** A workspace belongs to a **job**
and not to an attempt (`DataRoot::open_workspace` takes a `JobId`, and
clears the directory if it is already there), so two attempts of one job on
one machine still clear each other's working directory. This change makes
the machine's own books honest; it does not make running one job twice on
one host safe, and nothing here should be read as saying it does.

**No version moves.** Nothing on the wire and nothing on the hub's disk
changes: this is one crate's internal map. `PROTOCOL_VERSION` stays 2 and
`RECORD_VERSION` stays 1.

**Evidence.** Three tests in `herd-runner/src/lib.rs`, red on `199e4df`
before a line of the fix and green after, one per consequence:
`two_attempts_of_one_job_are_two_entries_in_the_machines_own_count`,
`handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running`,
`an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other`.
