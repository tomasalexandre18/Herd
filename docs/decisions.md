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
