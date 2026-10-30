# pack-fetch — handoff 2026-10-30

- **Item:** M11's pack half, steps **S5** (the fetch endpoint and the
  capability it checks) and **S9** (the peer fetch client and D10's three
  paths) of the night's plan, plus the module move S4 deliberately left for
  whoever built the endpoint. **S10's push is not in this branch** — see
  *Not done*.
- **Agent:** herd-step
- **Branch / commit:** `pack-fetch` / `111e94d`, based on
  `origin/pack-variants` `8344e07` with `origin/main` `351c826` (batch 11)
  and `origin/pack-store` `0f56d38` merged in. **Both merges were clean**,
  no conflict in either.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`
- **This branch needs `pack-variants` and `pack-store` in the same batch.**
  It is based on the first and merges the second; neither is on `main`.

## Done

### 0. The two merges, and the two things they broke

`git merge origin/main` (18 files, 3735 insertions) and
`git merge origin/pack-store` (6 files, 2804 insertions) both reported
`Merge made by the 'ort' strategy` with no conflict. **Git saw no conflict
and there were two anyway**, both found by running the tests before writing
anything:

1. **`herd-runner/src/packstore.rs`'s digest test is red on the merged tree
   and green on either parent alone.** It is routed to the agent holding that
   file and is **not fixed here** — the file is on this brief's do-not-touch
   list. Full diagnosis under *Risks and findings*, finding 1. It is the one
   red in this branch's gate.
2. **`herd-hub/tests/owed_scan_fails.rs`** — landed by batch 11 *after*
   `pack-variants` was cut — builds `ToHub::Hello` and `ToHub::Health` by
   hand and did not set the fields `pack-variants` added. Fixed here, because
   it is this merge's doing and the change is the one the compiler forces:
   `mesh_endpoint: None` and the three pack-store figures absent, which is
   what a stand-in that serves nothing and holds no pack store honestly
   reports.

### 1. The mesh rule is a library module (`5c5c598`)

`pub mod mesh;` in `herd-runner/src/lib.rs`; `mod mesh;` gone from
`main.rs`, which now reaches it through `herd_runner::mesh` like every other
module it uses. The five attributes that existed only because the module was
in a binary are gone with it: `#[allow(dead_code)]` on `MeshListener` and on
`may_serve`, and `#[cfg(test)]` on `HostRouting::declared`, `::unreadable`
and `::default_route_addresses` — the guard `declared`'s own doc said it
would shed *"in the library, where a `pub` item needs no such guard"*.

Two things only a real compile found, neither visible to
`cargo check --lib`: `mesh.rs`'s own tests called
`herd_runner::testing::skipped`, which does not resolve from inside the
crate that defines it (`crate::` now), and the `owed_scan_fails.rs` fields
above. **The fifteen mesh tests moved with the module**, from the binary's
test target into the library's.

### 2. The endpoint (`cd5c592`) — `herd-runner/src/endpoint.rs`

**It binds through `mesh::MeshListener` and nowhere else.** The source guard
permits the name `TcpListener` in one named module and `mesh.rs` is that
module, so this one goes through the alias. The source guard
`no_module_of_the_runner_can_bind_a_socket` is green with the endpoint
built, and its own detector still fires.

**No socket unless it may serve.** `mesh::may_serve(mesh, serves_content)`
is the whole gate; neither half is re-read here. It is consulted on **every
turn of the loop** and not once at startup, because `off` arrives while the
socket is already open — so the listener is *dropped*, and the socket leaves
`/proc/net/tcp`. Proved both ways by `serves_nothing_means_no_socket_at_all`:
off from the start opens nothing (`opened: 0`), and on-then-off opens one,
accepts a real connection, then stops accepting them and says
*"no socket of any kind left"* naming E-33.

**A capability exists because the hub pushed it (D209).**
`Granted::pushed_by_hub` is the only way into the table and is named for
where a capability comes from; `FromHub::ExpectTransfer` is its one caller.
The table lives on `Session`, because it has to survive a redial: a
capability forgotten on a reconnect would be refused by the one party the
hub told to expect it.

**Each refusal with the case that fired it**, all in
`every_capability_refusal_has_a_case_that_fires_it` against one real store
and one real pack:

| Refusal | The case |
|---|---|
| `NoCapability` | an empty `token` |
| `NotPushedByHub` | `"a-token-i-minted"`, well formed, never granted — and the table is asserted still empty afterwards, so a refused presentation teaches the runner nothing |
| `Expired` | `expires_unix_s = now - 1`; **and the boundary**, `= now`, which is accepted — valid in its last second |
| `WrongPack` | a capability minted for a second store's pack, presented against the first |
| `OutOfScope` | an object of this pack outside the capability's one-object list |
| `WrongDirection` | a `Push` capability asked to be read from |
| `NotInThePack` | the fixture's own `kind d` and `kind l` digests, which **are** in the manifest and are not objects |
| `Framing(ProtocolVersion)` | `PROTOCOL_VERSION - 1`, refused in `herd-proto`'s own sentence |

The eighth assertion of that test is that the whole-pack capability **does**
serve the object, so none of the above is a check that refuses everything.

**A refusal is a sentence on the wire** (D150 from the serving side):
`a_refused_peer_is_told_why_over_the_wire` reads a `PeerRefusal` off a real
socket, asserts its text, and asserts the endpoint served nought bytes.

**Invariant 2 with a transfer in flight: 77.46 µs, measured.** The peer reads
one chunk and stops, filling the socket buffer, so the endpoint is blocked
inside `write_all` — the state a polite stop would wait out. The stop aborts
it, and the peer's own read then ends, which its framing reads as a truncated
body and not as a silence.

**The one confinement line**, as its author asked:
`every_listening_socket_of_the_runner_is_its_mesh_address`'s
`assert!(listening.is_empty(), …)` becomes `assert_eq!(listening.len(), 1, …)`.
**The two assertions above it are untouched** and were already the rule. The
doc comment's property 2, which said *"`herd-runner` today still binds
nothing"*, is corrected to what the walk now finds, because leaving a false
sentence next to a true assertion is worse than the one-line brief.

### 3. The fetch (`111e94d`) — `herd-runner/src/fetch.rs`

D10's three paths in D10's order, with the path taken on `Fetched::path` and
asserted in every test.

| Path | Assertion | Figure |
|---|---|---|
| **1 same host** | `st_ino` of `{in}/pack/objects/<sha>` **equals** the store's, and `st_nlink >= 2` | `bytes: 0`, `objects_fetched: 0`, `objects_held: 1`, `peer: None` |
| **2 peer** | only the missing objects move, counted on both sides | first transfer `79` bytes / 2 objects; second pack, sharing one object, `56` bytes / 1 object / `objects_held: 1`; third fetch of the first pack is `SameHost` at `0` |
| **3 outbound-only** | the fetcher's own endpoint loop reports `opened: 0` and `connections: 0` **while** the fetch completes | `FetchPath::Peer`, `13` bytes |

The byte figures are asserted against **two independent counts** — the
fetcher's own and the holder's sentences — which is what makes either worth
quoting. Two peer transfers of three missing objects took **two
connections**, asserted: one connection carries every object of one
transfer, because 557 dials for one pack is what the alternative costs.

**Only `objects/<sha256>` moves.** The manifest and the meta ride the
assignment. Nothing here reprints a manifest — the bytes are written
verbatim and `PackManifest::disagreement_with` refuses a hub that named an
id those bytes do not hash to. The same-host test asserts the manifest laid
out at `{in}` is byte-identical to the bytes the hub sent.

**Killed mid-flight leaves no readable object and the pack unregistered**, and
the store's guarantee is not re-implemented, only not defeated: fetched bytes
land in `packs/incoming/<id>-<nanos>-<pid>/objects/`, under a name nothing
looks up, and `PackStore::ingest` is the only thing that ever places one.
The test **cuts at a point and not after a span** — it waits until object
bytes are really staged and the object is not whole, then aborts the task.
Measured: cut with `1048576` of `6291456` bytes staged. A timed cut would be
a guess about this host's speed, and the host that lost the guess would
assert against a finished transfer and pass, which is D101 exactly.

**Every path that gets no pack returns a sentence, bounded by `Patience`.**
`a_pack_that_cannot_be_had_is_named_and_bounded` fires all three shapes: no
holder at all (`PackState::NoHolder`, *"a state and not a wait"*), every
holder outbound-only (named, and it says the bytes must be **pushed** and
that this build does not push), and a hub that will not authorise. A peer's
own `PeerRefusal` is carried through word for word, proved by pointing a
fetch at an endpoint the hub never pushed to.

`forget_incoming` runs at startup beside the verdicts and collections a
previous life left, because a **killed** process is the only thing that can
leave a staging directory behind: a fetch removes its own on every path out,
the refused ones included.

### 4. The wiring in `lib.rs`

`Config::mesh: Option<mesh::Mesh>` (default `None` — outbound-only unless an
owner names an address, the only default invariant 3 permits), carried from
`main.rs`'s existing resolution. `run_until` spawns `endpoint::serve_while`
beside the redial loop, not inside a connection — a peer fetching a pack has
nothing to do with whether this machine's hub is up. The stop fires the
endpoint's oneshot **at the start of the owner's five seconds**, with the
probes, and the task is aborted *and awaited* at the end for the probes'
reason: the abort asks for the drop and it is the drop that closes the
listener.

## Not done, and why

1. **The push half of path 3 is not built**, and it is the one clause of the
   brief's part two that is missing. The plan puts it in S10
   (`herd-runner/src/push.rs`) and it needs a decision the design has not
   taken — see *Decisions needed* 1. The state it leaves is **named and not
   silent**: a pack whose every holder is outbound-only is refused with a
   sentence saying the bytes must be pushed and that this build does not push.
2. **No pack frame is sent or received by the runner except
   `FromHub::ExpectTransfer`.** `AskTransferCapability` and
   `TransferCapability` are the real frames and go through the `Authoriser`
   trait, so the step that wires them to the control link is an adapter and
   not a translation; `PackTransferred` is built by `Fetched::transferred` and
   not sent; `RegisterPack`, `AskPack`, `PacksHeld`, `PackUnavailable` are
   untouched. The fetch is not yet called from the job's `{in}` preparation
   either: that is the assignment path, and it needs `AssignJob`'s
   `packs_in`, which is S8's.
3. **`herd-proto`, `herd-hub`, `herdctl`, `packstore.rs` and `outputs.rs` are
   not touched** (the last two by instruction; the hub and the proto are
   other steps'). The one exception is `herd-hub/tests/owed_scan_fails.rs`,
   which this branch's own merge broke — §0 item 2.
4. **`docs/` carries nothing about any of this.** `docs/architecture.md`'s
   frame and module inventories, `docs/plan.md`'s M11 section and
   `docs/runner-confinement.md`'s "binds nothing" sentence all now understate
   what the runner does. They are not this step's files and the sentences are
   listed in finding 4 so nobody has to find them again.
5. **No end-to-end.** S11 is the step that makes the pack half *done*
   (invariant 5), and nothing here claims otherwise: every proof above is a
   unit test against a hand-written hub plus the real endpoint, which is what
   the plan asked of S5 and S9.
6. **The per-object cost is not measured.** S12's figure. The endpoint
   reports objects, bytes and milliseconds, which is what that step needs.

## Gates

**Runs made on the committed code, before this file was written**, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-fetch`, each heavy phase
preceded by `./scripts/bench-preflight.sh` clear:

```
$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.06s
exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s
exit 0

$ nix develop --command cargo test -p herd-runner --no-fail-fast
  unittests src/lib.rs    258 passed; 1 failed; 0 ignored
  unittests src/main.rs     3 passed; 0 failed; 0 ignored
  tests/busy_probe.rs       8 passed; 0 failed; 0 ignored
  tests/confinement.rs     42 passed; 0 failed; 0 ignored
  tests/declared_command.rs 21 passed; 0 failed; 0 ignored
  tests/persisted_switch.rs 2 passed; 0 failed; 0 ignored
error: 1 target failed:
```

**The one failure is `packstore.rs`'s
`an_object_name_that_is_not_a_digest_is_refused_…`** (the full name is one
identifier, too long for this file's columns). **It is the merge collision
of finding 1, it is green on either parent alone, and it is in a file this
brief forbids this branch to edit.** It is routed to the agent holding
`packstore.rs`.

**The figure from before anything was written**, as the brief asked:
`cargo test -p herd-runner --lib` on the merged tree, before the first line
of this branch's code, was **231 passed; 1 failed** — the failure already
being finding 1. It is 258 passed now, and 246 of those 258 were reachable
after the module move alone, because the fifteen mesh tests came with it.

**The last runs, named and not quoted** (a handoff is a docs file, so the run
that gates the tree holding this file cannot quote itself):
`./scripts/check-docs.sh` and bare `./scripts/gate.sh` — never inside
`nix develop` — on the tree with this file committed, after two consecutive
clears of the unmodified `bench-preflight.sh` and with no override anywhere
in the environment. Output goes to
`<scratchpad>/pack-fetch/docs-final.log` and
`<scratchpad>/pack-fetch/gate.log`, and verbatim into the agent's report.
**The gate will be red while finding 1 is red, and on that red this branch
is not pushed.**

**Skips:** none from any run above — every binary reported `0 ignored`. The
gate's own skip list is in the agent's report with the run that produced it.

**Preflight readings behind the runs**, every one a clear of the unmodified
script, `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` never set:

```
load 1.26 / 9.00, slice room 17871 MiB, floor 8192 MiB   ok
load 0.94 / 9.00, slice room 17881 MiB, floor 8192 MiB   ok   (+22s)
load 0.62 / 9.00, slice room 16261 MiB, floor 8192 MiB   ok
load 0.51 / 9.00, slice room 16168 MiB, floor 8192 MiB   ok   (+22s)
```

## Risks and findings

### 1. The merge collision, which is the night's recurring defect in miniature

The test named under *Gates* asserts
`why.contains("sixty-four lowercase hex")`. On the merged tree the
hostile row is refused **earlier**, by `pack-variants`'
`herd_proto::pack::check_object_digest` inside `entry_of_row`, whose sentence
says *"64 lowercase hex digits"* — digits, numerals. The behaviour is right:
the row is refused by name, `objects_dir()` is empty afterwards, and the two
other assertions of that test pass. Only the expectation's spelling belongs
to the speaker that no longer speaks first.

It is the merge and neither parent: `herd-runner/src/packstore.rs` does not
exist on `origin/pack-variants`, and `check_object_digest` has **0** hits in
`origin/pack-store`'s `herd-proto/src/pack.rs`. So one rule with two
sentences has not merely risked drifting — it **has** drifted, and the drift
surfaced as a red test in a third place. The real fix (the runner's check
delegating to `herd-proto`'s, so one rule has one wording) needs a tree
holding both branches and is therefore after the batch lands, not tonight's.

### 2. The peer wire carries no peer identity, so "the wrong peer" is
###    not a refusal the endpoint can make

The brief asked for five named capability refusals including *the wrong
peer*. `FetchCapability` names the peer it is for; `PeerRequest` carries a
protocol version, a token, a pack, an object and an offset, and **no peer
identity at all**. So the endpoint cannot tell one dialler from another and
cannot refuse that case by name.

This is not a gap to close by adding a field: what binds a transfer to a peer
is possession of the token, which is exactly what *authorised, not
confidential* means (D200) — a bearer capability's holder **is** its subject.
What the endpoint does refuse, and what that case was reaching for, is
`WrongPack`: a token minted for one peer's transfer of pack P reads no other
pack's bytes. `WrongDirection` is beside it. Both are tested. The module
header says all of this in the code, so the next reader does not rediscover
it.

### 3. `packs/incoming/` is a fourth directory under `packs/` that
###    `PackStore` does not own

`fetch.rs` creates and removes `<data root>/packs/incoming/…` itself, because
`packstore.rs` is on this brief's do-not-touch list. It is under `packs/`
deliberately — the staging copy must be on the same filesystem as `objects/`
or `ingest`'s hard link becomes a second copy of every byte fetched — but the
*layout* of that directory belongs in the module whose doc enumerates
`objects/`, `manifests/` and `refused/`. **It should become a
`PackStore::incoming_at()` beside `refused_at()` when that file is free**, and
until then two modules know the shape of one directory.

Compatible with the change in flight on `pack-store`: a staging directory
holds exactly `manifest.tsv`, `pack.meta` and `objects/`, which is the set
`ingest` is being taught to require.

### 4. Four documents now understate what the runner does

Not this step's files, listed so nobody has to find them again:
`docs/runner-confinement.md` (the runner binds a socket now, on one mesh
address, and the guard it documents has changed shape);
`docs/architecture.md` (its module inventory has no `endpoint` or `fetch`,
and its frame inventory no pack frames); `docs/plan.md`'s M11 section; and
`docs/coverage.md`'s E-12/E-17 rows, which S0 was to correct and which this
branch does not know the state of.

### 5. What would be silent if the hub stopped answering, as asked

Three places, and all three are a **timeout with a sentence** rather than a
wait:

- **Asking for a capability** — `Patience::capability`, 30 s. A hub that
  never answers `AskTransferCapability` yields *"the hub did not answer in
  30s … this runner does not dial the holder on its own authority"*. It does
  **not** fall back to dialling unauthorised, which is the one silent
  behaviour that would be tempting here.
- **The holder's own capability** — if the hub returns a capability to the
  fetcher and the `ExpectTransfer` push to the holder is lost, the fetcher
  gets the holder's `NotPushedByHub` refusal **word for word**, which is the
  tested case `a_peer_refusal_reaches_the_fetcher_word_for_word`. That is the
  D150 shape closed: the party that knows why speaks, and the sentence
  crosses the wire.
- **A transfer that stops mid-object** — `Patience::read`, 60 s, and a
  `read` returning 0 is a refusal naming how far in it got. What arrived is
  discarded rather than written.

**The one place this branch cannot speak for** is the hub's own arrival
window, which is S7's: a `RegisterPack` or an `AskPack` that reaches the hub
outside what `is_current` assumes would be dropped there, and nothing in this
branch would hear about it, because nothing in this branch sends either
frame. The endpoint and the fetch are both pull-shaped and both bounded, so
there is no path *here* that waits for a hub frame that never comes — the
exposure is entirely on the frames this branch does not yet send.

### 6. Two smaller risks

- **The endpoint re-binds after `off` → `on`.** With `--mesh-listen ADDR:0`
  the kernel picks a port, so a close and re-open changes the advertised
  endpoint, and the hub's holder record would be stale until the runner
  announces again. Nothing announces yet, so nothing is wrong today; the step
  that sends `Hello`/`PacksHeld` with `mesh_endpoint` must announce the
  **bound** port and announce it again after a re-bind.
- **A transfer aborted by a stop contributes no figures to
  `endpoint::Report`.** Deliberate and documented, and it is why the byte
  assertions are made against the fetcher's count and the holder's sentences
  rather than against that struct.

## Drafted for `herd-merger`

```markdown
## D-NEXT — 2026-10-30 — The peer wire's authority is the hub's push, and its subject is the token

A runner serves a pack's objects on one socket, on its single configured
mesh address, through `mesh::MeshListener` and from no other module
(invariant 3, D10, D11). What authorises a transfer is a capability the
**hub** pushed to that runner over that runner's own already
authenticated control link (`FromHub::ExpectTransfer`, D209);
`endpoint::Granted::pushed_by_hub` is the only way one enters the table,
and it lives on `Session` so that a redial does not lose a capability the
hub has already promised a waiting job.

**The peer wire carries no peer identity, and this is the decision rather
than an omission.** `PeerRequest` is a protocol version, a token, a pack,
an object and an offset. So the endpoint cannot refuse "the wrong peer" by
name, and it does not pretend to: what binds a transfer to a peer is
possession of the token, which is what *authorised, not confidential*
means (D200) — a bearer capability's holder is its subject. The scopes
that **are** refused by name are the ones the wire can check: no
capability presented, one the hub did not push, an expired one, a token
for another pack, a `Push` capability asked to be read from, an object
outside the capability's list, and a digest the manifest does not name as
a file. Each has a case that fires it, and a whole-pack capability is
asserted to serve the object, so the set is not a check that refuses
everything.

A fetch takes D10's paths in D10's order and **reports which one it
took**, because D10's claim is not "it arrived" but that the cheapest
available path was the one taken. Same host is asserted on the **inode**
and not on the byte count, since nought bytes is also what a failed
transfer reports. Only the objects the fetcher lacks move, and the diff is
computed in the fetcher's own content store — which is why the hub keeps
holdings per manifest and not per blob.

Every path that gets no pack is a **named, visible state with a sentence
in it**, and every wait is bounded: no holder at all, every holder
outbound-only (which needs a push), a hub that will not authorise, a
holder that cannot be dialled, a transfer that stopped. A peer's own
refusal crosses the wire word for word. That is D150's finding applied to
this half: the party that knows why is the one that has to speak, and an
absence is never an answer.
```

```markdown
### Board rows proposed

- **ORCH-nnn — One digest rule, one wording.** `herd-runner`'s
  `packstore::object_name` and `herd_proto::pack::check_object_digest`
  state the same rule in two sentences, and on a tree holding both
  `pack-variants` and `pack-store` that drift is a red test
  (`an_object_name_that_is_not_a_digest_is_refused_rather_than_joined_onto_a_path`,
  diagnosed in `history/2026-10-30-pack-fetch.md` finding 1). Make the
  runner's check delegate, so one rule has one wording. Needs a tree with
  both branches on it, so: after the batch lands.
- **ORCH-nnn — `packs/incoming/` belongs to `PackStore`.** `fetch.rs`
  creates and removes a fourth directory under `packs/` that
  `packstore.rs`'s own doc does not enumerate, because that file was held
  when the fetch was written. Move it behind a
  `PackStore::incoming_at()` beside `refused_at()`.
- **ORCH-nnn — The push path (plan S10).** M11's done-when asks an
  outbound-only runner to complete **both** directions. The pull half is
  built and proved; the push needs the peer wire's one open question —
  see *Decisions needed* 1 of
  `history/2026-10-30-pack-fetch.md`.
- **ORCH-nnn — Four documents understate the runner.**
  `docs/runner-confinement.md`, `docs/architecture.md`'s module and frame
  inventories, `docs/plan.md`'s M11 section. Prose only.
```

## Decisions needed

1. **How a push is expressed on the peer wire** — the only question S10
   cannot start without. The numbered message set is pull-shaped:
   `PeerRequest` means *give me bytes*, `PeerReply` carries no token, and in
   the push case the receiver can be dialled while the holder cannot, so the
   bytes must flow on a connection the **holder** opened.
   **Recommendation:** the leading `PeerRequest` authorises the
   *connection*, and the capability's `direction` says which way the bytes
   then flow — `Fetch`, and the endpoint replies with `PeerReply` + body;
   `Push`, and the dialler follows its own `PeerRequest` with `PeerReply` +
   body frames that the endpoint stages and ingests. This costs **no
   protocol change**: `FetchCapability::direction` exists for exactly this,
   and `TransferDirection`'s own doc already says the holder *"is told which
   one it is being asked for"*.
   **Fallback:** a push-only frame on the peer wire, which costs a fourth
   payload type on a leg `pack-variants` has already frozen — strictly
   worse, and it is the reason to decide before S10 starts rather than
   during it.
2. **Whether `PackState` needs a fourth name.** The three are `Registered`,
   `Incomplete`, `NoHolder`, and *"held only by peers this fetcher cannot
   dial"* is none of them: the holders are not gone. This branch reports it
   as `NoHolder` with a sentence that says exactly which case it is.
   **Recommendation:** leave the enum at three and keep the sentence, since a
   fourth variant is a protocol change and the state disappears the day S10
   lands. **Fallback:** number it with the rest if S10 is deferred past
   v1.0.0.

## Resume point

```sh
cd /mnt/ssd_980/herd-project/.claude/worktrees/pack-fetch
git merge origin/pack-store          # once the digest-assertion fix is on it
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-fetch
nix develop --command cargo test -p herd-runner --lib   # expect 259 passed; 0 failed
./scripts/bench-preflight.sh         # twice, 20s apart, unmodified
./scripts/check-docs.sh && ./scripts/gate.sh            # bare, never inside nix develop
git push -u origin pack-fetch        # only on a green gate
rm -rf /mnt/ssd_980/herd/build/pack-fetch
```

Then S10 (the push), once *Decisions needed* 1 is answered, on this branch's
tip; and S8 for `AssignJob`'s `packs_in`, which is what makes `fetch` get
called from a job's `{in}` preparation at all.

## Processes

Started and ended: `cargo`, `clippy`, `rustfmt` and `cargo test` processes
under `nix develop`, each awaited to completion; the test binaries' own
children (`herd-runner`, `herd-hub` spawned by `confinement.rs`) are
`kill_on_drop` and ended with their tests. Nothing was killed by name, and
no PID of the first consumer's live hub (3006543) or runner (3021002) was
touched.

Own `target/`: `/mnt/ssd_980/herd/build/pack-fetch`, to be removed once this
branch is pushed, by that literal path. It is **not** removed while the
branch is unpushed, which is the condition `workflow.md` sets.
