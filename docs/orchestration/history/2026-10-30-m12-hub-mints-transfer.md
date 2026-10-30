# m12-hub-mints-transfer — handoff 2026-10-30

- **Item:** no row existed; one is drafted below for the merger to file
- **Agent:** herd-step
- **Branch / commit:** `m12-hub-mints-transfer` / `b0eca2f`, based on
  **`ae1188f`** (`origin/m11-pack-layout`), whose own parent is
  `origin/main` `df47266`. **Not based on `origin/main` directly** — the
  reason is the first finding below
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The hub's half of D209 exists. `ToHub::AskTransferCapability` is
answered rather than refused by name, and `FromHub::ExpectTransfer` is
constructed by a production path for the first time in the workspace.

One arm of `herd-hub/src/server.rs`'s `relay_from_runner`, plus two free
functions beside the other pack helpers. Nothing else of the file moved,
and `herd-hub/src/matcher.rs` and `herd-hub/src/report.rs` were not
touched.

What the arm does, in order:

1. reads the **asking** runner's entitlement first, through the existing
   `a_pack_may_be_fetched` — the same window a fetch is read in. A
   capability is a credential, so a runner asking against somebody
   else's job is told no before the hub has looked up who holds what;
2. refuses `TransferDirection::Push` and `peer: None` by name as the
   push path, which is ORCH-220 and the step after this one;
3. refuses an ask against the asker itself;
4. reads the holder off **the hub's own registry** (`holders_of`), never
   off what the asker said, and refuses a named machine that holds
   nothing;
5. refuses an **outbound-only** holder — one that advertised no mesh
   address — by name, as the case the push path exists for, rather than
   minting a capability with no address in it;
6. refuses a holder the hub has no connection to, because
   `ExpectTransfer` travels the holder's own link and a token the holder
   was never told about is refused at its endpoint as `NotPushedByHub`;
7. mints a token, sends `ExpectTransfer` to the **holder first** and
   `TransferCapability` to the asker second, and journals the scope.

Three invariants, as facts a reader can check in the diff:

- **D10, invariant 3.** No byte of a pack passes through the hub, and
  the arm chooses no address: `TransferCapability::endpoint` is the
  string the holder advertised in its handshake, which invariant 3 and
  D11 already constrain to a private mesh interface. Nothing widens a
  listen address and no port is opened.
- **Invariant 7, D200.** The token is the roster's scheme and not a
  second one: 32 bytes from `rustix::rand::getrandom`, lowercase hex, a
  prefix (`herd-pack-`) that names what it is, and no fallback to the
  clock — a host that gives no random bytes mints nothing and says so.
  It is scoped to one pack, one peer, one direction and the asked-for
  object list, expires at `PACK_CAPABILITY_LIFETIME` (300 s) on the
  hub's clock, and **no part of it reaches the journal**. The mint is a
  call of its own rather than a call into `roster`, because the roster's
  sentences are about a machine an operator approved and would be a lie
  here; `roster.rs` was not modified.
- **Invariant 4.** `herd-proto` is untouched. Both frames were already
  on the wire at `PROTOCOL_VERSION` 3, and every field the arm fills
  already existed.

D13 and D261 hold. Every path that will not authorise sends
`PackRefused` — non-terminal, about one pack — and never
`FromHub::Refused`; both links are proven still carrying traffic after
each refusal. The dry run is untouched: `herdctl would-run` answers at
the dispatch and never reaches this frame, and M12 L3's three tests are
unchanged and green.

**Tests.** Four, all new, all against a hub over a socket:

- `herd-hub/tests/pack_frames.rs` (in-process hub, real connection
  loop, two hand-driven runners that the labels make deterministic):
  `the_hub_mints_a_capability_tells_the_holder_and_answers_the_asker`,
  `a_transfer_this_hub_will_not_authorise_is_refused_by_name_and_`
  `the_link_stays_up` (one name, wrapped),
  `an_outbound_only_holder_and_a_holder_that_has_gone_are_each_said`;
- `herd-hub/tests/pack_chain.rs` (the hub as a **process**, so its
  journal is in a file a test can read):
  `the_hub_authorises_a_transfer_and_writes_no_token_down`.

**Every new assertion was watched red**, by mutating the committed tree
and restoring it. Which assertion went red for which clause:

| Mutation | Red at |
|---|---|
| A — `server.rs` reverted to `ae1188f` | all four new tests, each naming the old *"does not mint … yet"* refusal: `pack_chain.rs:785`, `pack_frames.rs:1218`, `:1435`, `:1565` |
| B — `ExpectTransfer` never sent | `pack_frames.rs:1233`, the holder's wait times out; the `pack_chain` test still passed, which is why both exist |
| C — the journal prints the token | `pack_chain.rs:820`, and the panic quotes the leaked line |
| D — every capability widened to the whole pack | `pack_frames.rs:1306`, `left: []` against the one object asked for |
| E — the holder told its own name | `pack_frames.rs:1364`, `left: "the-runner-that-holds-it"` |

A's first run stopped at the first failing binary, so it was re-run with
`--no-fail-fast`; the figures above are that run's.

## Not done, and why

- **No pack moved between two machines.** The brief's *Done when* —
  B gets the bytes from A over A's own content endpoint — is **not**
  asserted, and cannot be from this branch. `herd-runner` does not
  advertise a mesh endpoint, so no real runner can be a holder a peer is
  sent to. Full measurement in *Risks and findings*, first item.
- **`pack_chain.rs`'s `SameHost` assertion was not turned into `Peer`.**
  Same cause. The file gained an authorisation test beside it instead.
- **The push path** (`TransferDirection::Push`, `peer: None`) mints
  nothing and is refused by name. That is ORCH-220, plan S10, and was
  out of scope.
- **`docs/decisions.md` and `docs/orchestration/board.md`** are
  untouched; both drafts are below.

## Gates

Run through Herd, `--label build --memory-mib 4700`, with
`./scripts/bench-preflight.sh` run before each heavy phase and clear
each time (load 5.00, 4.89, 4.99, 2.45, 1.37, 2.38 against a ceiling of
9.00). Output before this file was committed, verbatim in the agent's
report:

- `cargo build --workspace` — job `j0000000126`, green,
  `Finished dev profile … in 27.51s`;
- `cargo fmt --all` then
  `cargo clippy --workspace --all-targets -- -D warnings` — job
  `j0000000128`, green, no warning emitted, `Finished dev profile … in
  23.91s`. `--all-targets`, so the new tests compiled under it;
- `cargo test -p herd-hub --test pack_frames --test pack_chain` — job
  `j0000000135`, `10 passed; 0 failed` and `2 passed; 0 failed`;
- the red-watch above — jobs `j0000000136` and `j0000000137`.

**The last run, named and not quoted**: the four Rust gates and
`./scripts/check-docs.sh` on the tree this file is committed into, as
one Herd job, with its output going to `final-gates.log` in this agent's
own scratchpad directory (the full path is in the agent's report) and
verbatim in the agent's report.

Skips: the standing **twelve** `HERD-SKIPPED` lines of cgroup
delegation, and the `#[ignore]` census **COMPLETE at 38**, compared as a
list and not as a count (D187). Both are reported from the final run in
the agent's report; a `HERD-SKIPPED` line is a skip and never a pass
(D27).

## Risks and findings

**1. The brief's premise *"The runner needs nothing — its side is
complete"* is refuted, and it is what blocks the brief's own
*Done when*.** `herd-runner` sends `mesh_endpoint: None` in its
handshake **unconditionally**:

- `herd-runner/src/lib.rs:4406` (on `ae1188f`), in the one `ToHub::Hello`
  the binary constructs, with the comment *"`None` until this runner has
  a mesh address to advertise, which is S4's work… Absent is the
  outbound-only runner, and that is exactly what this build is"*;
- so `herd_proto::pack::Holder::endpoint` is `None` for every real
  runner, whatever `--mesh-listen` it was given;
- so `herd_runner::fetch::reachable_holder` refuses every holder as
  outbound-only, and so does the arm this branch wrote;
- and `herd-runner/src/endpoint.rs:513` resolves the **real** advertised
  address at bind time and hands it only to a `voice` callback that
  prints it. Nothing feeds it back to the session.

Measured, not inferred: `git grep -n mesh_endpoint` over `herd-runner`
on `ae1188f` returns that one line and nothing else. The hub side is
already complete for it — `State::mesh_endpoint_of` reads the handshake
field and the registry records it per holding (`server.rs`, the
`PacksHeld` arm) — and `herd-hub/tests/pack_frames.rs`'s `ENDPOINT`
constant exists precisely because the hub *used* to discard it.

**It is not a one-line fill, and that is why it was left as a finding
rather than taken.** With `--mesh-listen 127.0.0.1` the port is `0` and
the kernel picks it at bind time, which happens in a task started
*beside* the redial loop — so the advertised address is not known when
the hello is sent. The frozen protocol has no frame that updates a mesh
endpoint later, so either the bind must be awaited before the first dial
(a restructure of the runner's startup), or the advertisement is only
honest for an explicit `--mesh-listen ADDR:PORT`. That is a decision,
and it is the question below.

Two further reasons it was not taken here: `herd-runner/src/lib.rs` is
the file `m11-pack-layout` is landing (+394 lines in it), and the brief
named the runner as owning nothing of this task.

**2. `herd-hub/tests/pack_chain.rs` does not exist on `origin/main`.**
It is `m11-pack-layout`'s, and so is the runner's whole production
capability-ask path (`HubMints`, the pending-answer map,
`FromHub::TransferCapability`'s arm). On `df47266`,
`herd_runner::fetch::fetch` has **no caller outside its own `#[cfg(test)]`
module** and `impl Authoriser` exists only there. So this branch was cut
from `ae1188f`, not from `df47266`. `ae1188f`'s parent **is** `df47266`,
so the branch carries exactly `main` plus that one commit and merges
with no shared line beyond it.

**3. `m12-p5-hub-keepers` and this branch do not touch a shared line.**
P5's only hunk inside `relay_from_runner` is at old `server.rs:5716`, in
the **`RegisterPack`** arm (the `tell_the_keepers` trigger). This
branch's change begins ~150 lines further down, in the
`AskTransferCapability` arm, and adds two functions immediately *before*
the doc comment of `a_pack_registry`, which P5 does not touch. No line
of P5's needed taking.

**4. The "spends a go" path is now less reachable, not more.** Before
this, *every* ask was refused, so every cross-machine fetch failed and
went back through `Session::abandon`, which the hub charges (D182).
Authorising the ask is what removes that for the holder-reachable case.
What remains chargeable is unchanged in kind: a transfer that fails after
the capability was minted. No charging rule was touched.

**5. A race the hub cannot close, named rather than hidden.** The two
frames go to two connections. The holder's is queued first, which is the
only ordering the hub controls, but delivery is concurrent — so an asker
that dialled within the delivery window could be refused as
`NotPushedByHub`. The frozen protocol has no acknowledgement for
`ExpectTransfer`, so no hub-side wait is available. The comment in the
arm says so. On a mesh link the window is sub-millisecond and the asker
must still complete a TCP dial; whether the fetcher should retry once on
that refusal is the runner's question, not this arm's.

**6. The fixture pack in `pack_chain.rs`'s new test has no objects on
disk anywhere.** The test asserts the **authorisation**, so there is
nothing for it to read; the registration is of a manifest, exactly as
`pack_frames.rs`'s is. It proves no transfer and does not claim to.

## Drafted for `herd-merger`

### `docs/decisions.md` — append verbatim

```markdown
## 2026-10-30 — D-NEXT The hub authorises a transfer; it is never on the
path

`ToHub::AskTransferCapability` was answered by a refusal naming itself
— *"this hub does not mint a transfer capability for pack … yet"* — and
`FromHub::ExpectTransfer` was constructed in **no place** in the
workspace. `herd_runner::endpoint::Granted::pushed_by_hub` is the one
door a capability may enter a holder by, and it was fed by no
production path, so no pack could move between two machines at all.
That is E-12 — *packs by (commit, key), no shared build trees* — with
its one mechanism missing. This entry settles how the hub closes it.

**1. The hub mints, and the hub is not the byte path.** It reads the
asking runner's entitlement, reads the holder off its own registry,
mints a token, tells the holder over the holder's own already
authenticated link and answers the asker. No byte of a pack passes
through the hub (D10): what crosses it is an authorisation.

**2. It chooses no address; it repeats one.**
`TransferCapability::endpoint` is the string the holder advertised in
its handshake. Invariant 3 and D11 already constrain that to a private
mesh interface, enforced at the runner since D222, so authorising a
transfer widens nothing and opens no port. A capability is never minted
against an address the hub invented, and a holder that advertised none
is **outbound-only** — named as such, which is the case the push path
exists for, and never answered with a capability carrying no address.

**3. The token is the roster's scheme and not a second one.** 32 bytes
from `getrandom`, lowercase hex, behind a prefix that names what it is,
with **no fallback to the clock**: a host that gives no random bytes
mints nothing and says so. It is scoped to one pack, one peer, one
direction and the object list asked for — an ask for one row is never
answered with the whole pack — and expires at
`PACK_CAPABILITY_LIFETIME`, 300 s, on the hub's clock, as an instant
and not an offset, because the machine that compares it is not the
machine that minted it. **No part of it is journalled**, and that is
asserted against the hub's own stderr: a journal is read, shipped and
pasted, and a bearer capability's holder *is* its subject (D200,
invariant 7). The mint is a separate function rather than a call into
`roster`, because the roster's sentences are about an approved machine
and would be a lie here; the scheme is shared, the explanation is the
caller's.

**4. The holder is told before the asker is answered.** The two frames
go to two connections, so the queue order is the only ordering the hub
controls, and this is the one that does not race the asker's dial
against the holder's knowledge of the token. The protocol has no
acknowledgement for `ExpectTransfer`, so the hub can do no more than
order the two sends; the remaining window is named in the arm and is
the fetcher's to retry over, not the hub's to close.

**5. Every refusal is about one pack, and names which of six it is.**
`PackRefused` and never `FromHub::Refused` (D13): a transfer this hub
will not authorise costs the asking runner neither its link nor its
other jobs. The six are a job that is not this runner's, a push, a
`peer: None`, an ask against the asker itself, a machine that holds
nothing, and an outbound-only or disconnected holder. **None of them
turns a wait into a refusal**: a job whose pack has no holder is held at
the dispatch and never reaches this frame, so D261's dry-run answer and
D13 are both unaffected.

**6. No protocol move.** Both frames were already on the wire at
`PROTOCOL_VERSION` 3 and every field filled already existed, so the
authorisation needed nothing the frozen surface lacked.

**What this does not make true.** **No pack has moved between two
machines**, and nothing here may be reported as a working transfer.
`herd-runner` sends `mesh_endpoint: None` in its handshake
unconditionally, so every real runner is an outbound-only holder
whatever `--mesh-listen` it was given, and the hub correctly refuses to
mint against an address nobody advertised. The runner's advertisement
and the push direction are each their own row. What is proved is the
hub's half, against a hub on a socket and against the hub as a process.
```

### `docs/orchestration/board.md` — one new row, section *Owed*

A single row, no literal pipe in any cell:

```markdown
| ORCH-NEXT | **The runner advertises no mesh endpoint, so no pack can move between two machines — and this is now the head of M11's critical path.** `herd-runner` sends `mesh_endpoint: None` in the one `ToHub::Hello` it builds (`herd-runner/src/lib.rs:4406` on `ae1188f`), with a comment calling the advertisement *"S4's work"*. Measured: `git grep -n mesh_endpoint` over `herd-runner` returns that one line. So `Holder::endpoint` is `None` for every real runner whatever `--mesh-listen` it was given, `fetch::reachable_holder` refuses every holder as outbound-only, and the hub — whose half is now built, `m12-hub-mints-transfer` — correctly refuses to mint against an address nobody advertised. The hub side needs nothing: `State::mesh_endpoint_of` reads the field and the registry records it per holding. **This is not ORCH-220**, which is the push *direction* (plan S10) and is still open: this row is the authorisation **both** directions need, and the brief that produced it found no row naming it | `m12-hub-mints-transfer`, 2026-10-30, after `m11-pack-layout` reported the runner's half complete; the hub half is built and landing, the advertisement is not | herd-step | owed | **One field, and one decision inside it.** `--mesh-listen 127.0.0.1` means port `0`, and the kernel picks it at bind time in a task started beside the redial loop — so the address is not known when the hello is sent, and `endpoint.rs:513` hands the resolved one only to a callback that prints it. The frozen protocol has no frame that updates a mesh endpoint later, so the taker chooses: await the bind before the first dial (a restructure of the runner's startup, honest for every form of the flag), or advertise only an explicit `ADDR:PORT` and say so. **Recommend awaiting the bind**, because a flag that works only in one of its two spellings is a trap for the consumer. It touches a crate, so it owes all four Rust gates, and the measurement that closes it is `herd-hub/tests/pack_chain.rs`'s transfer assertion turned from `SameHost` into `Peer` with two real runners, one started with `--mesh-listen` on loopback | the hub's half, `m12-hub-mints-transfer`, landing; `m11-pack-layout` for the runner's fetch path | `herd-runner/src/lib.rs:4406`; `herd-runner/src/endpoint.rs:513` and its `Said::Listening`; `herd-runner/src/fetch.rs` `reachable_holder`; `herd-runner/src/mesh.rs` `LISTEN_FLAG`; `herd-hub/src/server.rs` `mesh_endpoint_of` and the `AskTransferCapability` arm; `herd-hub/tests/pack_frames.rs` `ENDPOINT`; `history/2026-10-30-m12-hub-mints-transfer.md`; D209, D200, D10, D11, D222; invariant 3; E-12; ORCH-220 |
```

## Decisions needed

**Question.** When a runner is given `--mesh-listen 127.0.0.1` — port
`0`, the kernel picks — what does it advertise to the hub, given that
the frozen protocol has no frame that updates a mesh endpoint after the
handshake?

**Recommendation.** Await the bind before the first dial. The runner
already starts the endpoint beside the redial loop; having the redial
wait on the bound address makes every spelling of the flag work, and the
outbound-only case stays exactly what it is — no address, so nothing to
wait for.

**Fallback.** Advertise only an explicit `--mesh-listen ADDR:PORT` and
refuse to claim a mesh endpoint for the port-`0` form, saying so at
startup beside the other caps the runner prints. Cheaper, and it leaves
a flag that silently does less in one of its two spellings.

This is *how*, not *what*, so it is a technical choice for whoever takes
the drafted row — not a question for Alexa.

## Resume point

Take the drafted board row: fill `herd-runner`'s `Hello.mesh_endpoint`
from the bound endpoint, then turn `herd-hub/tests/pack_chain.rs`'s
transfer assertion from `SameHost` into `Peer` with a second real runner
started `--mesh-listen 127.0.0.1`. That is the brief's *Done when*, and
the hub's half it stands on is this branch.

Exact next command for whoever continues this branch:

```sh
git fetch origin m12-hub-mints-transfer
git worktree add -b <task> <path> origin/m12-hub-mints-transfer
```

## Processes

PIDs started and ended: none held. Every heavy phase ran as a Herd job
(`j0000000126`, `j0000000128`, `j0000000130`, `j0000000133`,
`j0000000135`, `j0000000136`, `j0000000137`, and the final gate run), so
no compiler was started in this session's own process tree. One stray
`cat` of this agent's own (PID 2137651) blocked on stdin and was killed
by PID; nothing else was signalled. PID 662711, 565152, 3006543 and
3021002 were not touched.

Build tree: `debug with tests, 3.0 GiB` at
`/mnt/ssd_980/herd/build/m12-hub-mints-transfer` — profile named before
the figure, and well under the 17 G a full debug tree with tests weighs
because only `herd-hub`'s two test binaries were built, not the whole
suite. `/mnt/ssd_980` had **344 G** free when it was measured. Deleted
with `rm -rf` once the branch was pushed; the agent's report says so.
