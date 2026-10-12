# Plan

Reworked 2026-10-12 from the chatOnS requirement sheet (issue #1, expectations
E-01..E-55 — 44 must, 9 should, 2 could; mapping in `docs/coverage.md`). M0 is
done; everything after it is replaced. Each milestone is one session on one
branch, ending with `cargo test --workspace` green and `docs/status.md`
replaced. Done means a passing automated test; where acceptance needs real
hardware or a chatOnS deliverable, the automated stand-in is named and the
rest is a manual check or a gated item closed in M18. Vocabulary per CLAUDE.md
(the sheet's "client" is our runner); chatOnS reaches Herd only as declared
commands, labels and figures — strings and numbers, never meanings.

## Milestones

### M1 — End-to-end skeleton (`m1-end-to-end`)
Nix flake dev shell; workspace, four crates; herd-proto v1 (Hello/Welcome with
protocol version and token field; submit, assign, log chunk, finished); hub
dispatches to its single runner; `herdctl run` streams the log, exits with the
job's code.
**Done when** an in-process hub+runner+requester test runs a trivial command,
asserting streamed output within 2 s and exit-code propagation (0 and
non-zero); proto round-trips pass; a version mismatch is refused naming both
versions. Substrate only; satisfies no E alone.

### M2 — Durable job store (`m2-job-store`)
Hub state on disk from birth (Q1): one directory per job — `job.json`
by atomic rename, `events.jsonl` appended, log, kept files. Record fields;
state machine with timestamped transitions; queue rebuilt at startup; a
runner holds a finished result until acknowledged, redelivering after a hub
restart. Detached submission, `herdctl wait <id>`, log readable as a file
afterwards. The minimum reconnect (D17): a runner that loses its control
connection redials with a simple bounded retry and keeps its running job
alive across the gap, so the held-result machinery above is reachable;
an attempt interrupted by the drop is recorded as interrupted by name,
never left silently `running`.
**Done when** a test kills the hub mid-job against the real binaries and
the job finishes on its runner, redials, redelivers its held result, result
recorded, queue intact (E-44); a detached wait returns the exit code and
the log file equals the stream (E-03); the record is plain files readable
while the job runs (E-15 core, E-53). Metric fields exist, filled in M4.

### M3 — The link survives reality (`m3-link-robustness`)
M2's minimum reconnect (D17) gets a backoff policy; health report every ~15 s;
~60 s of silence = genuinely lost: running jobs requeued once at priority,
the record naming both attempts, a returning runner finishing nothing of
the old attempt. Cancel propagates hub → runner. Drain ≤ 5 s (invariant 2).
**Done when** killing the runner mid-job past the lost threshold requeues
within 90 s with a two-attempt record (E-21); cancel ends the job within
10 s while a sibling job continues (E-06; kill hardened in M4); a draining
busy runner exits ≤ 5 s; heartbeat ages land in hub state (E-40 partial,
surfaced in M13).

### M4 — Runner confinement (`m4-runner-confinement`)
One configured data root: a bad root refused, nothing written outside it
(E-45). Unprivileged, never sudo; root only via a configured fixed-list helper
(E-46). Per-job workspace, process-group/cgroup kill, nice CPU+IO, no inbound
socket, disk budget (E-34). Kernel memory ceiling: cgroup v2 MemoryMax, logged
fallback, WSL per Q6 (E-30). Timeout, log cap. Collects load, memory peak,
bytes written (completes E-15's fields).
**Done when** a job writing outside the root fails naming the path; an
allocation past the ceiling kills the job and only the job (automated in a
small cgroup; the WSL OOM replay is a manual M16 check); metrics appear in the
record; the runner binds no listening socket.

### M5 — Capability model (`m5-capability-model`)
One algebra in herd-proto (Q3): capability = name + kind (exclusive host-wide
/ counted(n) / shared) + exclusion list + optional external busy probe (a
lock-file path or a command whose exit code means busy — strings, never
plugins). A runner offers probed ∪ declared ∪ denied, each with origin (E-24).
`metal-<m>` is counted(1) with a lease probe (E-27's model); `reference` rides
on the kinds (E-28); external busy suspends availability, fails nothing
(E-26).
**Done when** unit tests cover kind semantics, exclusion closure, origin
reporting (`kvm: present, denied`); a held lock file keeps a queued job queued
with the reason recorded, releasing it afterwards.

### M6 — Queue and priorities (`m6-queue-priorities`)
One durable queue for every job (E-20). Four levels set at submission, FIFO
within, aging so the lowest never starves, operator raise (E-18; merger-first
is E-43). Dedup on (commit, command, args, labels): the second submission
joins the first, both callers one result id, higher priority wins (E-05).
**Done when** simulated-time tests show a lowest-priority job behind a steady
higher stream running within the aging bound, a merger submitted last claiming
the next free slot, and two identical submissions producing one execution with
two completed waits.

### M7 — Matching, slots, no fallback (`m7-matching-slots`)
Claiming matches labels against the algebra, enforcing kinds under contention
(E-25). Counted build slots; per-slot parallelism derived from memory and
threads, recorded (E-22). Stickiness: a job waits for an eligible runner while
one is available (connected, switch on), moves only when none is, under a
visible safety limit (E-19). One explanatory line per claim; impossible label
sets fail fast (E-36).
**Done when** two counted jobs serialize and an exclusive one never overlaps
them; a modelled PC never runs two builds, its derived `-j` recorded; with the
eligible runner busy the job stays queued, disconnected it moves within 10 s;
every placement in a simulated day greps to one line; an unsatisfiable job
fails with the reason.

### M8 — Admission and explained waits (`m8-admission`)
Memory admission: declared need vs free minus a per-runner floor minus what
running jobs hold (E-29). tmpfs reservation before start, released at end;
short → wait, never fail (E-31). Host-wide sums; load recorded, never a gate
(E-32). A queued stream reports position, what is ahead and why, at least
every 60 s; refusals carry figures (E-04).
**Done when** admission unit tests pass with figures in refusals (`1.2 GiB
free in tmpfs, 2 GiB needed`); a reservation-starved job waits then starts
when room frees; a 10-minute simulated wait leaves periodic reasons.

### M9 — Owner control (`m9-owner-control`)
Owner switch per runner — off-with-reason, drain, on — honoured in 10 s, set
from the runner's host or via herdctl, state visible (E-33). Dry run (E-07).
Deadline (E-09; needed by E-41). Dependency: at most one named parent, start
after its success, else cancelled with the reason — one edge, no conditions
(E-10; needed by E-12's flow). Metal drain-first: a `metal-<m>` job drains its
host's build slot before starting, bounded 15 min (completes E-27).
**Done when** `off` stops claims within 10 s and stops serving content too
(D11), reason visible; dry run lists
eligible runners and the queue ahead, runs nothing; a past-deadline job shows
`cancelled (deadline)`; a dependent starts only after its parent; a simulated
metal trial never overlaps a build on its host.

### M10 — Command declaration contract (`m10-command-contract`)
The consumer declares commands in one file in its own tree: name, command
line, labels, resources, declared outputs, and a prepare step declaring the
cache keys it needs and leaves (Q5). A
submission names a declared command and a commit of a configured bare
repository — a branch resolves to a recorded hash, never a machine (E-01);
dirty or unpushed trees refused naming the files (E-08). The runner
materializes the commit, runs the command inside `nix develop` with the
consumer's environment, confined per M4. Output framing: first line
`ran on: <runner> (<why>)`, then the tool's verdict and exit code unchanged,
0/1/75 passed through, format versioned (Q7) (E-02). Herd's code and docs name
no consumer command (E-11).
**Done when** a fixture consumer repo declares two commands and adding a third
changes nothing on Herd's side; e2e asserts the `ran on:` line, the verbatim
verdict, exit codes 0/1/75; a dirty submission is refused naming the files.
The real chatOnS declaration closes in M18.

### M11 — Content registry, direct transfer (`m11-content-registry`)
The hub is the registry and the authority, never the byte path (D10). It holds
manifests — a pack is a list of content hashes keyed by (commit, key) — and
which runners hold which blobs; it issues a short-lived, scoped fetch
capability for a (blob, peer) pair. The bytes move **runner to runner**: each
runner keeps a per-file content-addressed store under its data root and
advertises a fetch endpoint (Q2); a runner needing a pack asks the hub who
holds it and fetches directly, only the hashes it lacks. Three paths, in
order: **same host** — hardlink or reflink inside the local store, zero bytes
on any network; **peer reachable** — direct fetch from a holder; **outbound
only** (a runner with no mesh) — it pushes to and pulls from a keeper, never
serves. A fetch endpoint binds to the one configured mesh interface and is
refused at startup if it would bind a default or public one (D11). A pack is
complete or absent: the manifest is registered only once every blob is held
and fsynced (E-17, E-13, E-12).
**Done when** a two-runner e2e produces a fixture pack on runner A and
consumes it on runner B with the hub seeing no payload byte (asserted by
counting bytes on the hub's sockets); a same-host fetch moves zero network
bytes and no second copy on disk; a second pack sharing files with the first
transfers only the new blobs; killing a transfer mid-flight leaves the pack
unregistered and no partial blob readable; an outbound-only runner completes
both directions without listening; a configuration that would bind a default
or public interface is refused at startup; a fetch without a valid capability
is refused. chatOnS's `0 crates compiled` acceptance closes in M18.
Most likely milestone to overflow one session; if it does, split at the
registry boundary — local CAS plus the same-host and keeper paths first,
peer-to-peer fetch and capabilities second.

### M12 — Custody, budgets, GC, locality (`m12-custody-gc`)
Custody (D10): a key family names one or more **keepers** — runners that pin
what the retention policy keeps, so a pack outlives the machine that built it
and survives its owner switching that machine off. The server's runner is the
default keeper; a pack with no reachable holder is a named, visible state, not
a silent failure. Per-runner budgets shown as use against budget, gc on each
runner never removing what a queued or running job references nor what its
keeper role pins (N = 5 most recent per key family, configurable) (E-16).
Result retention by priority class — 14/90 days, configurable (closes E-15).
Warm caches per D9 — Herd carries no cache engine, it carries the consumer's:
the prepare step runs before the job with its declared cache keys, its
duration recorded against the job (E-14); a runner reports the keys it holds
in its health report; and because content is addressed per file, the consumer
may register cache diffs as ordinary packs, so a 57-700 MB diff between mains
moves only the blobs the target lacks — directly, peer to peer.
Placement preference among available eligible runners: keys already held /
shortest prepare, then most free memory, reference last (E-23); E-19 still
governs waiting.
**Done when** a pack whose only non-keeper holder goes offline is still
fetchable from its keeper; a pack with no holder at all reports that state by
name; gc on a runner with a budget 10 GiB below use comes in under it keeping
every pinned and referenced item; a step result past retention is gone while a
60-day merger result reads fine; prepare duration is recorded per job; a
fixture cache diff registered as a pack moves only the missing blobs; and
placement prefers the runner already holding the key, picking reference last.

### M13 — Status and events (`m13-status-events`)
One status document, JSON with a format version (Q7), carrying the queue,
runners, running jobs and leases with the fields E-35 lists, refreshed on
change, plus one text screen (E-35); M3's health ages surfaced (E-40). Events
pushed to a configured URL or command — exactly E-37's list, never per step
job by default (E-37).
**Done when** an e2e snapshot validates against the schema and drives a
fixture renderer with no other source, and a simulated day of step jobs emits
exactly the expected events and zero per-step ones.

### M14 — Scheduled jobs and the hook path (`m14-scheduled-jobs`)
Hub-config schedule entries: at a local time, if a named ref of the bare
repository moved since the last run, submit a declared command with labels,
priority, deadline (E-41) — that one predicate, nothing richer (Q13, invariant
1). Exclusivity from label kinds, deadline from M9. The push-hook path is a
documented detached `herdctl` submission; the hook itself is the consumer's
(E-42).
**Done when** a simulated clock fires at 03:00 only when the ref moved; the
job carries its deadline and its exclusive label keeps everything else off
that runner until it ends, a 03:30 job waiting or going elsewhere; a scripted
bare-repo hook lands a lowest-priority job in e2e.

### M15 — Auth and versions (`m15-auth-versions`)
Per-runner token minted by one operator action (`herdctl runner approve`),
stored hashed; an unapproved runner claims nothing; removal revokes (E-48,
Q8). Runner build version in Hello beside the protocol version: warn on minor,
refuse on major, naming both (E-51).
**Done when** e2e shows an unapproved runner getting no job and one approval
command fixing it; mismatch tests assert both versions in warning and refusal.

### M16 — The hub deployed, the server's runner native (`m16-deploy-hub`)
Flake builds the hub image; docker compose beside the consumer's dashboard;
`wss://` through the existing reverse proxy; state and artefact volumes with
their budgets (Q9). The server's runner runs natively, unprivileged, with
`/dev/kvm`, `/dev/dri`, `/dev/vfio` and the consumer's namespaces reachable
(E-47), restarting with the host (E-52, server side).
**Done when** a fixture job submitted from a requester outside the compose
network runs on the server's native runner over `wss://` and returns its log
and exit code (automated against the deployed pair); a hub restart under
docker compose re-asserts M2's no-loss test on the real volume; a reboot of
the server brings the runner back with no hand.

### M17 — The owner's PC joins (`m17-pc-runner`)
The second machine, and the one with the hard conditions. Runner on WSL2: a
login shell for Nix, the documented WSL checklist, a Windows logon task, no
inbound socket, the disk budget (E-50, E-52, E-34 on real hardware). The join
recipe written and walked by a person, under 30 minutes (E-49). The owner's
switch reachable from her side by a desktop shortcut (E-33 on real hardware).
The mesh path verified rather than assumed (D11): either the mesh daemon
inside WSL (needs `/dev/net/tun`; their WSL has kernel 6.18 and systemd) or
WSL mirrored networking sharing the Windows interface — whichever works goes
in the recipe.
**Done when** the PC joins by the recipe alone and runs a fixture job from the
server's hub; the timing of the walk is recorded. Three checks are manual on
real hardware and recorded in `docs/status.md`: the 30-minute join, the
2026-10-12 WSL OOM replayed against M4's ceiling (E-30), and the shortcut
flipping the switch within 10 s (E-33).

### M18 — First consumer acceptance (`m18-onboarding`)
Close every chatOnS-gated acceptance with their deliverables (declaration
file, pack and `test --pack`, results.json, push hook, nightly command):
E-02's real verdicts, E-11's one-entry change, E-12's `0 crates compiled`,
E-14's prepare timing, E-41 live at 03:00, E-42 from their hook.
**Done when** the gated list in `docs/coverage.md` is walked with chatOnS on
the real machines; each item checked or a numbered follow-up. Paced by their
side; blocks nothing of ours.

## Critical path

The longest chain is the **scheduler spine**:
**M1 → M2 → M5 → M6 → M7 → M8 → M9**, then deployment **M16 → M17 → M18**.

Off the spine but **not optional** — each carries musts and each must land
before M16, because the first consumer cannot submit a single real job
without them: M3 (the link), M4 (confinement), M10 (the command contract),
M11 and M12 (artefacts), M13 (status and events), M14 (the scheduled job),
M15 (auth). They are parallelisable, not deferrable. The only genuinely
deferrable items are E-38, E-39 and E-55 (see `docs/coverage.md`).

- M1 → M2: the store persists the lifecycle M1 creates; anything built before
  it would be rewritten onto it — the walk-back E-44 exists to forbid.
- M2 → M5: durable queue entries embed label requirements; the algebra's types
  must exist before entries persist in final shape.
- M5 → M6: dedup keys and queue entries carry label sets; the queue cannot be
  final on a provisional algebra.
- M6 → M7: matching claims from the priority queue; kind and slot enforcement
  is the claim step itself.
- M7 → M8: admission is a further claim-time filter inside the matcher; E-04's
  explanations annotate its refusals.
- M8 → M9: switch, drain and dry run manipulate claim and admission state that
  must exist first.
- M9, M10, M11, M12, M13, M14, M15 → M16: nothing is exposed over `wss://`
  until a job can be declared, confined, placed, stored, observed and
  authenticated. Deploying earlier would freeze the public formats (Q7) on a
  surface still moving.
- M16 → M17: the PC joins a hub that already works; its switch (E-33) and
  ceiling (E-30, via M4) are the owner's hard conditions and are checked on
  her machine, not simulated.
- M17 → M18: acceptance needs both machines on the real deployment.

## Parallel tracks

One branch per milestone; parallel milestones meeting in herd-proto or the
hub's module tree conflict, so the spine owns herd-proto and a parallel track
gets its message additions agreed before its branch opens. CLAUDE.md allows
one sub-agent at a time, with Alexa's agreement — at most one track beside the
spine.

- **Track R — runner** (M4, then M10): herd-runner plus a fixture consumer
  repo. Depends on M1 (M10 also on M4, Q5, Q7); must not touch the hub's
  scheduler modules. Best delegation candidate: long, isolated, few shared
  files.
- **Track S — store** (M11, then M12's engine): a new hub storage module, blob
  endpoint, runner fetch. Depends on M2 and Q2/Q4; integrates with M10's
  declared outputs at the end; M12's placement preference needs M7. Must not
  touch the scheduler. Second delegation candidate.
- **Track O — ops** (M13, M14, M15): M13 reads hub state across many modules,
  stays with the orchestrator; M14/M15 are small, after M9 and M1, before M16.
- M3 is off the formal path but touches the hub's connection and queue code;
  the orchestrator runs it between M2 and M5, not in parallel.

## Decide before coding

| Q | Decision | Recommendation | Blocks |
|---|---|---|---|
| Q1 | Hub persistence model | Directory per job: `job.json` by atomic rename, `events.jsonl` appended; no database. Atomicity is E-17's primitive reused; records stay plain files the consumer can read (E-53). | M2 |
| Q2 | The data path | **Herd is never in the byte path (D10).** The hub registers manifests and who holds what, and issues scoped fetch capabilities; runners move blobs directly. A runner advertises a fetch endpoint and binds it to one configured mesh interface only, never a default or public one (D11). Same host: hardlink/reflink, zero network. A runner with no mesh is outbound-only: it pushes to and pulls from a keeper and never serves. | M11, M12 |
| Q3 | Resource algebra shape | One Capability type {name, kind: exclusive / counted(n) / shared, excludes, busy-probe}; memory and tmpfs are numeric admissions, not labels; a lease is counted(1) with an external probe. | M5 |
| Q4 | Content addressing | Per file, not per archive. It buys three things at once: inter-pack dedup (E-13), GC pinning (E-16), and "fetch the diff" for free — a peer fetches only the hashes it lacks, so no delta protocol and no rsync is ever needed. | M11 |
| Q5 | Warm caches | Herd grows no cache engine of its own, but must actively *carry* one: (a) the artefact store is generic, so the consumer may publish cache diffs through it as ordinary packs — per-file addressing (Q4) means a 57-700 MB diff costs only its missing blobs; (b) the declaration contract has a first-class **prepare step** with declared cache keys and a recorded duration; (c) a runner reports which keys it holds, and placement prefers a holder (E-23). Herd never knows what a cache *is*. | M10, M11, M12 |
| Q6 | WSL memory ceiling | All job processes in one cgroup v2 subtree with MemoryMax (WSL2 ships systemd); documented fallback; verified on the real PC in M17. | M4 |
| Q7 | Public format versioning | `ran on:` line, status JSON and event payloads each carry a format version; v1 freezes at M16. | M10, M13 |
| Q8 | Authentication | Per-runner token minted at approval, hashed at the hub; no mTLS (the proxy terminates TLS); one shared requester token to start. | M15 |
| Q9 | Where state and content live, budgets | Hub volume: job records and manifests only — kilobytes, no payloads. Content lives in each runner's store under its data root, with a per-runner budget (the server's keeper share of their 80 GiB offer; the PC's 900 GiB). | M2, M11, M12 |
| Q10 | Name and licence | Keep "Herd"; the licence is outward-facing — ask Alexa before M16. | M16 |
| Q11 | Runner packaging | A flake output (binary via nix profile or copied); semver carried in Hello (E-51). | M16, M17 |
| Q12 | Local foreground mode | Do not build one: local hub plus runner via `cargo run` is the debug path; chatOnS keeps its own local path. | — |
| Q13 | Schedule predicate | Exactly {local time, ref moved since last run, submission template}; anything richer is refused as workflow. | M14 |

## Conflicts and risks

- **Herd must not become a data mover (Alexa, 2026-10-12).** D7 fixed the
  wrong half of the problem: it moved packs off the JSON frames but kept them
  going through the hub. Orchestrating and relaying bytes are different jobs,
  and routing 1.5 GB through a container and a reverse proxy to reach a disk
  on the same machine is the clearest case against it. D10 supersedes D7: the
  hub registers content and authorises fetches, runners move the bytes.
- **The mesh is the owner's, not ours** (D11, approved 2026-10-12). A tailnet
  is validated as the network layer, and that is a deployment choice: Herd
  must never depend on a VPN product, because a coordination service we do not
  run would breach invariant 1 (no cloud) and a network layer inside Herd
  would breach it again. Herd receives one address to bind and one to
  advertise. With no mesh a runner is outbound-only and nothing breaks.
  Invariant 3 was rewritten for this: runners dial out for control and may
  listen on a private mesh interface, but no port is ever opened on a router.
- **Direct transfer costs something.** Peer liveness, partial holders, and
  "who actually has this blob" are harder to debug than one central store;
  custody (D10, M12) and a visible no-holder state exist to pay that cost
  honestly. Their own E-55 already asks for peer exchange, so this is the
  spec simplified, not stretched.
- **Invariant 1 judgements.** E-10 stays inside the line capped at one parent,
  start-after-success, no conditions — a primitive, not a graph. E-41 is a
  timer plus one predicate (Q13), not a workflow; if the predicate creeps, the
  fallback is a consumer-side cron submitting via herdctl. E-42 is entirely
  the consumer's hook. E-26's busy probes stay configuration strings; the
  moment they grow an API they are plugins — refuse.
- **E-14, the cache (Alexa, 2026-10-12).** The cache is load-bearing for the
  first consumer — few build machines, and a cold build is over 30 minutes —
  so "Herd does not manage it" must not become "Herd ignores it". Herd grows
  no cache engine (that would shape it around one consumer), but the generic
  artefact store, the declared prepare step and key-aware placement must be
  good enough to carry theirs. See D9.
- **Timing vocabulary.** Invariant 2's ≤ 5 s stop, E-06's 10 s job kill,
  E-33's 10 s switch and E-27's 15 min metal drain are four different things;
  M9 specifies all four.
- **E-19 vs E-23.** Stickiness decides when a job may move; locality
  preference only orders among available eligible runners (M7 vs M12).
- **Shared host.** Hub and reference runner share the server: nightly
  exclusivity silences the runner's jobs, not the hub container; on a swapless
  31 GiB host E-29's 6 GiB floor is the only guard — enforced, never advisory.
- **E-32's p90 < 12** is an adoption outcome, not a test: tests assert the
  mechanisms; the figure is read from statistics after M18.
- **Gated on chatOnS** (fixture tests stand in until M18): E-02, E-11, E-12,
  E-14, E-38, E-41, E-42, E-53, E-54 — each needs a chatOnS deliverable named
  in `docs/coverage.md`.
- **Size honesty.** The sheet carries 44 musts (recounted; not 38) across 15
  must-bearing milestones — not a handful of sessions, and pretending
  otherwise is how work gets walked back.

## Order rationale

Durability before the scheduler, or every scheduler feature is later rewritten
onto the store. The algebra before the queue, or persisted entries change
shape. Matching before admission, because admission is a filter inside the
claim step. Confinement before the command contract, so consumer commands
never once run unconfined. The contract before the store's integration,
because packs are declared outputs. Formats freeze only at deployment, so M10
and M13 can still correct them cheaply. Auth before exposure. Reordering any
of these pairs means building on a surface the next milestone must break.
