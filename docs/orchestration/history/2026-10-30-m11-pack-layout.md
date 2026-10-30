# m11-pack-layout — handoff 2026-10-30

- **Item:** ORCH-274 and ORCH-260, with the layout obligation ORCH-234
  left owed (*Still owed* item 2)
- **Agent:** herd-step
- **Branch / commit:** `m11-pack-layout` / see the push, based on
  origin/main `df47266`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

M11's `{in}`/`{out}` chain is closed and proved end to end against the
real binaries as processes. Six changes, in the order the chain runs.

**1. `herdctl` carries `needs` and `leaves` (ORCH-274).**
`herdctl/src/lib.rs` `Declared` gains both fields and `Declared::asked`
reads them instead of writing `Vec::new()` / `BTreeMap::new()`;
`herdctl/src/declaration.rs` `Resolved::declared()` fills them from the
command's own table. There is **no `--needs` flag** and the doc says
why: `Declared::asked` is one conversion on purpose, so `run`, `submit`
and the dry run all answer about the same job (E-07). The ad-hoc path
in `herdctl/src/main.rs` writes both out as empty rather than
defaulting them, so that list stays the whole of what an ad-hoc job
declares.

**2. The runner lays `{in}/<key>` out before the first step runs.**
`Session::lay_out_packs` (`herd-runner/src/lib.rs`) walks the
assignment's `packs_in` and calls `fetch::fetch` into
`workspace.in_dir().join(key)`, reports each pack's cost to the hub as
`ToHub::PackTransferred`, and announces the holdings once at the end.
Called from `job::execute` after the workspace and `{in}` exist and
before the cgroup is opened, so `up` is the first thing that may read
it. A pack it cannot get sends `ToHub::PackUnavailable` naming the
`(commit, key)`, reclaims the workspace and hands the attempt back —
never a job that runs without its inputs and reports success (D150).

**3. The runner registers what the job leaves (ORCH-260).**
`declared_leaves` now returns the assignment's `packs_out` instead of
an empty map, so `packstore::leave_packs` really takes the declared
pack out of `{out}` before the record walk; `Session::register_pack`
then sends `ToHub::RegisterPack` with the manifest and `pack.meta`
**verbatim**, after the ingest and therefore after the fsync (E-17).
`ToHub::RegisterPack` was constructed in **0** places under
`herd-runner/src` and is now constructed in one.

**4. The capability adapter `fetch::Authoriser` was written for.**
`Inner::capability_asks` is a pending-answer map keyed by
`(job, pack)`; `Session::ask_for_a_capability` registers the waiter
*before* the frame goes out, `Session::transfer_capability` hands the
hub's answer to it, `Session::pack_refused` hands it the hub's own
refusal word for word, and `Session::detach` clears the map so a link
that goes is an answer and not a silence. An `AskEnded` guard takes the
entry out however the ask ends, including the exit that is not in the
function at all: `fetch` bounds it with `Patience::capability`, and a
timeout **drops** the future rather than returning through it.
`serve_hub` gained three arms — `TransferCapability`, `PackRefused`,
`PackRegistered` — which fell to the "not a runner's business" wildcard
before.

**5. The refusal narrowed rather than disappeared.**
`Packs::why_this_runner_cannot` was "any pack at all"; it is now one
case — a pack on a job that is **not a declared command**, which has no
`{in}`, no `{out}` and no commit to register a `(commit, key)` for.
`None` for every declared command, which is the one place this could
have silently put the old stop back, and a unit test asserts exactly
that.

**6. `herdctl`'s own fixture declared a pack it could not honour.**
`assemble`'s `leaves` named `tree.txt` and `logs/assemble.log` — two
**files**, where a `leaves` path is a pack and a pack is a directory
holding `manifest.tsv`, `pack.meta` and `objects/`. And `probe`
declared `needs = ["grove-tree"]`. Both were invisible while the fields
were discarded; both are behaviour the moment they travel. So the two
base commands now declare **neither** field and the chain is its own
appended pair — `forge` leaves `grove-grain` at the directory `grain`,
`graft` needs it, both added by `Fixture::add_the_pack_chain` — and
`tests/declared.rs` asserts the pair lines up. The reason that split is
not cosmetic is in *Gates* below: the first full run went red because
`assemble` declaring a pack it did not leave put a `skipped` sentence
on its record, which made a test about a job with no outputs also a
test about a pack.

**The end-to-end: `herd-hub/tests/pack_chain.rs`.** A real hub, a real
runner and a real `herdctl`, all as processes, with a fixture checkout
of its own (bare remote, `commands.toml`, a tool that writes and reads
a pack). One runner, deliberately: D10's first path is the same host,
and the figure the hub is told is `SameHost` with **nought** network
bytes. It asserts, in order: the dry run says *"it would be claimed and
then wait for its packs"* and names the key and commit (D261) before
anything is registered; the producer runs and its pack is registered;
the pack is **absent from the producer's record** (ORCH-153, D223); the
consumer is laid out and **its own tool reads the bytes** out of
`{in}/<key>/objects/<sha256>` and its `pack.meta`; the transfer is
`SameHost` with zero bytes; and a key nothing registers is a wait that
names the key.

**Both detectors were proven red before the green was trusted.**
Measured on this branch, each by one temporary edit reverted
afterwards:

- layout removed (`lay_out_packs` returns `Ok` without laying out) →
  `DETECTOR-RUN EXIT: 101`, red at *"the consumer must be laid out and
  run"*, because the consumer's own tool exits 69 on an empty `{in}`;
- registration removed (`register_pack` not called) →
  `DETECTOR2 EXIT: 101`, red at *"and not as a pack it could not
  register"*.

The second run also found a weak assertion and it is fixed: `"NOT
registered with the hub"` **contains** `"registered with the hub"`, so
the `wait_until` on the shorter phrase passed while the registration
was gone. It now waits on `"pack store and registered with the hub"`,
which only the positive sentence holds, and the comment records the
measurement.

The fixture's manifest carries a `kind d` row **and** a `kind l` row
with sha256 columns that name no object, so the `kind f` filter is
proven and not assumed: without it the completeness check looks those
two up and reports a good pack as incomplete.

## Not done, and why

1. **The peer path cannot complete, because the hub mints no
   capability.** `herd-hub/src/server.rs`'s `ToHub::AskTransferCapability`
   arm refuses by name — *"the hub's half of D209 — minting the token and
   pushing ExpectTransfer to the holder over the holder's own link — is
   not written"* — and the hub sends no `FromHub::ExpectTransfer` at all,
   so `endpoint::Granted::pushed_by_hub` is fed by nothing in production.
   The runner side of that round trip is now complete and real: it asks
   the real frame and carries the hub's refusal into a named
   `PackUnavailable`. **No board row named this**, which is why one is
   drafted below. It is not ORCH-220: that row is the push direction
   (plan S10), and this is the authorisation both directions need.
2. **`Claims::referenced` is still empty**, so the gc knows nothing of
   what a running job's `{in}` holds. That is ORCH-275 and was out of
   scope; the stale sentence on `Session::pack_claims` which said the
   layout step "does not put it in this session yet" is amended to say
   the layout step is landed and the claim set is still owed.
3. **`JobRecord::packs_in` is still written by nothing** (ORCH-248), so
   `job.json` cannot say which pack an attempt consumed. The e2e reads
   the figures off the hub's journal instead. Untouched on purpose: it
   is a `JobStore` signature and five implementors.
4. **No two-runner end-to-end**, which is plan S11 and needs item 1
   first: with no capability there is no transfer to count.
5. **`herd-proto` untouched**, as the brief required. Nothing needed a
   frame or a field that was not already numbered at
   `PROTOCOL_VERSION` 3.
6. **`herd-hub/src/matcher.rs` and `server.rs`'s keeper path
   untouched**, as the brief required. The hub needed no change at all:
   its `needs` path was complete and the two breaks were both on the
   client and the runner.
7. **`docs/plan.md`, `docs/coverage.md` and `docs/architecture.md` not
   updated** for what this lands. ORCH-221 already owns the runner's
   under-described modules and the brief named no docs path; a row is
   drafted to carry this landing into it rather than a silent edit here.

## Gates

Run through Herd, one job per phase, `--label build --memory-mib 4700`,
`./scripts/bench-preflight.sh` cleared before each — and **obeyed once**:
it refused at load 10.08 against its ceiling of 9.00 and the phase
waited until 8.59.

**The first full run went red and is reported red.** Job
`j0000000105`:

```
BUILD EXIT: 0
FMT EXIT: 1
CLIPPY EXIT: 0
TEST EXIT: 101
```

Two failures, both caused by this branch's own fixture change and
neither fixed by editing an expectation:

- `herdctl::declared`'s
  `the_two_commands_carry_the_figures_and_the_levels_they_declare`
  — the new `add_the_pack_chain` guarded on the bare command **name**,
  and `DECLARATION`'s own comments name both commands, so it refused to
  add a declaration that declared nothing. The guard now tests the table
  header `[commands.graft]`, and the comment records the measurement.
- `herdctl::outputs::a_job_that_leaves_nothing_says_nothing_about_outputs`
  — `assemble` declaring a pack it did not leave put
  `outputs.skipped: ["… left nothing there, so there is no pack of it
  to keep"]` on its record, where that test asserts a job that kept
  nothing has **no outputs field at all**. The test is right and the
  fixture was wrong: both of M11's fields left the base declaration and
  the chain became its own appended pair. This is the concrete form of
  the finding in *Risks* 1 — a declared field that travels is
  behaviour, and a declaration that cannot be honoured is a defect the
  day the field arrives.

`cargo fmt --all --check` was red for formatting alone and was fixed
with `cargo fmt --all`, never by hand-editing around it.

The green run, job `j0000000108`, on the tree as it stands bar this
file — verbatim output in the agent's report, with the skip list and
the `#[ignore]` census compared as **lists** and not as counts (D187):
the standing **twelve** `HERD-SKIPPED` of cgroup v2 delegation, and the
census at **38**, whose file-and-name list is byte-identical to
`main`'s at `df47266` (`diff` of the two `grep -rn '^#\[ignore'`
listings is empty). Neither moved on this branch.

The last run, named and not quoted, because it is made on the tree that
holds this file: `./scripts/check-docs.sh`, output to
`/mnt/ssd_980/herd/build/m11-pack-layout/check-docs-final.log`. Its
verbatim output is in the agent's report.

## Risks and findings

1. **The fixture declared a pack Herd cannot keep, and nothing noticed
   because the field was discarded.** `leaves = { "grove-log" =
   "logs/assemble.log" }` is a file; a `leaves` path is a pack
   directory. This is the general shape of ORCH-274's cost: a validator
   that accepts a field nobody reads lets a declaration drift from what
   the product can honour, and the drift surfaces as behaviour the day
   the field travels. The consumer's own `commands.toml` is on their
   side of the fence (D161) and **may have the same defect**; it is a
   report and not a task, and it is the first thing to look at if a
   declared command starts waiting after this lands. Herd's own fixture
   had held the defect since M10, and it took a red gate to find half
   of it.
2. **Six tests would have hung, not failed.** `probe` declaring `needs`
   with nothing registering it is a job that waits for ever, which is
   correct behaviour and an invisible test failure. Found by reading
   the call sites rather than by running them, and worth saying because
   the same shape is what the consumer will meet. The *other* half of
   the same fixture defect — `leaves` declared and not left — was
   **not** caught by reading, and the suite caught it: see *Gates*.
   One of the two was found by inspection and one by a red gate, which
   is the honest ratio.
3. **A job whose pack cannot be got spends a go.** The attempt is
   handed back through `Session::abandon`, which is the same frame a
   full machine sends, and the hub charges a go for an attempt that
   ended because of the runner (D182). A pack that is absent is not the
   runner's fault, and `abandon_at_its_ceiling` exists precisely for
   that distinction. Not changed here because the hub-side rule is
   `herd-hub`'s and the brief fenced off its dispatch; the honest
   statement is that a fleet none of whose runners can get a pack will
   burn that job's goes. The common case does not reach it: the hub
   holds such a job at the dispatch instead, and that path is tested.
4. **One runner proves the same-host path and nothing about a wire.**
   The zero-bytes figure is the assertion D10's same-host clause rests
   on and it is real, but it is also the figure a transfer that never
   happened would report — which is why the test asserts the path
   (`SameHost`) and the object counts beside it, and why the detector
   was proven red.
5. **`herdctl` is untouched on the `would-run` path and its three D261
   tests are untouched**; they live in `herd-hub/src/server.rs`'s own
   test module over a synthetic record, so nothing here could have
   reached them. The e2e asserts the same sentence through the real CLI
   as well, which it could not do before: with `needs` dropped, that
   answer was unreachable from `herdctl` at all.

## Drafted for `herd-merger`

The decision entry, in full, headed `D-NEXT` because the merger
allocates the number and gaps stay (D148, invariant 8):

```markdown
## 2026-10-30 — D-NEXT Both halves of M11's layout land together

`herdctl` read `needs` and `leaves` off `commands.toml`, validated
them, and threw them away: `Resolved::declared()` carried neither onto
`Declared`, which had no field for either, and `Declared::asked`
hardcoded an empty vector and an empty map. So `Asked.needs` was empty
on every submission and every dry run this CLI made, and the hub's
whole `needs` path — complete, tested and landed in batch 18 — was
reachable only from a client that is not `herdctl` (ORCH-274). At the
other end, `ToHub::RegisterPack` was constructed in **0** places under
`herd-runner/src`, so no pack was ever registered and the matcher's
locality term read an empty map for ever (ORCH-260).

**Decided: the two halves land in one branch, and never one in front
of the other.** Filling the two fields makes every declared command
that already names `needs` go from *running* to *waiting at the
dispatch* on a hub where nothing registers a pack — correct behaviour
(D150: a job whose `{in}/<key>` would be empty must not be a
green-looking job) and a change on the consumer's side of the fence
(D161). Shipping the CLI half alone would have been a product that
stops working for a reason nobody could act on.

**Three things the landing settles, each of which could have been got
wrong quietly.**

1. **A `leaves` path is a pack directory, not a file.** Herd's own test
   fixture declared `leaves = { "grove-log" = "logs/assemble.log" }`
   and had done since M10. Nothing noticed, because the field was
   discarded before it reached anything that could refuse it. That is
   the general cost of a validator that accepts a field nobody reads:
   the declaration drifts from what the product can honour, and the
   drift arrives as behaviour on the day the field travels.
2. **There is no `--needs` flag and there will not be one.**
   `Declared::asked` is one conversion on purpose — it is what makes a
   dry run answer about the job a submission would really make (E-07)
   — so the keys reach the wire from the declaration and from nowhere
   else. A flag would let a requester name a key the command's own line
   cannot read.
3. **A runner refuses a pack only on a job that is not declared.** The
   old refusal was "any pack at all", which was honest while the layout
   did not exist. What remains is the one case that cannot be served:
   an ad-hoc command line has no `{in}`, no `{out}` and no commit, so
   there is nowhere to lay a pack out and no `(commit, key)` to
   register one for. Every declared command is laid out.

**What is still not reachable, and it is now named rather than
implied:** the hub mints no transfer capability and sends no
`FromHub::ExpectTransfer`, so D10's peer path is refused by name at the
hub and only the same-host path completes. The runner's side of that
round trip is real — it asks the numbered frame and carries the hub's
refusal into a `PackUnavailable` that names the pack — which means the
peer path starts working the day the hub mints, with no runner change.

Proved by `herd-hub/tests/pack_chain.rs`, against the real hub, runner
and `herdctl` as processes: a declared command leaves a pack, the hub
registers it, a later declared command is laid out with it and **its
own tool reads the bytes**, and the transfer is reported `SameHost`
with nought bytes off the network. Both halves were watched red first
— the layout removed, and the registration removed — because a green
chain is the one place this could have passed while proving nothing.
```

The board rows, in full. ORCH-274 and ORCH-260 close; one row is new.

```markdown
| ORCH-260 | **E-23's locality term is landed and inert**: the matcher prefers, at equal `preference`, a runner holding the packs a job's `needs` resolve to, and in production no pack is ever registered — `ToHub::RegisterPack` is constructed in **0** places under `herd-runner/src` (18 occurrences elsewhere in the tree, so the zero is measured), so `pack_for` answers `None` for every key and the map the term reads is always empty. `ToHub::PacksHeld` **is** constructed (10 places), so holdings arrive and only the registration is missing | `m12-l2-locality` handoff, 2026-10-30, finding 1; filed by `herd-merger` at batch 20's landing | herd-planner | done | Fold into M11's `{in}`/`{out}` layout step, which the `pack-assignment` handoff already names as its resume point: that step is where `RegisterPack` is constructed for the first time. No separate task unless that step slips . **Done 2026-10-30 on `m11-pack-layout`, with ORCH-274, in one branch.** `Session::register_pack` sends `ToHub::RegisterPack` after `packstore::leave_packs` has ingested the declared pack and therefore after the fsync (E-17), with `manifest.tsv` and `pack.meta` verbatim; the construction count under `herd-runner/src` is 1 and no longer 0. A link that is gone is said out loud rather than passed over, naming the key a later job will wait for. Proved end to end in `herd-hub/tests/pack_chain.rs` against the real binaries as processes, and **watched red first** with the call removed | the layout step landed | `herd-runner/src/lib.rs` `register_pack`, `herd-runner/src/job.rs` the ending sequence; `herd-hub/tests/pack_chain.rs`; **D-NEXT**; ORCH-274, ORCH-234 |
| ORCH-274 | **`herdctl` parses `needs` and `leaves` and then throws them away, so E-23's locality term cannot be reached from the CLI at all.** ORCH-260 names one break in the chain — nothing constructs `ToHub::RegisterPack` — and there are **two**. Measured at `73c2dc8`: `herdctl/src/declaration.rs` reads both fields off `commands.toml` (the file's own field at `:589`, `Command` at `:332`, `Resolved` at `:259`, filled at `:490`), and then `Resolved::declared()` at `:291` does not carry either onto `crate::Declared`, which has no field for them; `Declared::asked` at `herdctl/src/lib.rs:390` hardcodes `needs: Vec::new()` and `leaves: BTreeMap::new()`. So `Asked.needs` is empty on every submission **and** every dry run this CLI makes, `JobRecord::needs` is empty on every record, and the hub's whole `needs` path — `packs_for`, `holders_of_what_it_needs`, the locality term, M12 L3's explanation — is reachable only from a client that is not `herdctl`. The hub side is complete and tested; the wire carries the fields at `PROTOCOL_VERSION` 3 and the record at `RECORD_VERSION` 2 | `m12-l3-would-run-locality`, 2026-10-30, finding 1; measured at `73c2dc8` | herd-planner | done | Fold it into M11's `{in}`/`{out}` layout step beside ORCH-260, which is where `RegisterPack` is constructed for the first time, and land the two halves together or neither . **Done 2026-10-30 on `m11-pack-layout`, both halves in one branch, as this row asked.** `Declared` carries both fields, `Resolved::declared()` fills them and `Declared::asked` reads them; **no `--needs` flag**, because that one conversion is what makes a dry run answer about the job a submission really makes (E-07). **Two findings the filling produced, and the second is the general shape:** Herd's own fixture declared `leaves` at a **log file** where a `leaves` path is a pack directory, and nothing had noticed because the field never reached anything that could refuse it; and `probe` declared `needs` with nothing registering it, which once the field travels is a job that **waits for ever** — six tests in `labels_from.rs` would have hung rather than failed. The consumer's own `commands.toml` is theirs (D161) and may hold the same defect, which is a report and not a task | the layout step landed | `herdctl/src/lib.rs` `Declared`, `asked`; `herdctl/src/declaration.rs` `declared()`; `herdctl/src/main.rs` the ad-hoc path; `herdctl/tests/fixture/mod.rs`, `herdctl/tests/declared.rs`; `herd-hub/tests/pack_chain.rs`; **D-NEXT**; ORCH-260, **D255** |
| ORCH-NEW | **The hub mints no transfer capability, so D10's peer path is refused at the hub and only the same-host path completes — and no row said so.** Measured at `df47266` plus `m11-pack-layout`: `herd-hub/src/server.rs`'s `ToHub::AskTransferCapability` arm answers `FromHub::PackRefused` with its own sentence, *"the hub's half of D209 — minting the token and pushing ExpectTransfer to the holder over the holder's own link — is not written"*, and `FromHub::ExpectTransfer` is sent by **nothing** in the hub, so `herd-runner/src/endpoint.rs`'s `Granted::pushed_by_hub` is fed by no production path and the endpoint can accept no token. **The runner's side of that round trip is now complete**: the layout step asks the numbered frame, waits on a per-session pending map keyed by `(job, pack)`, and carries the hub's refusal word for word into a `PackUnavailable` that names the pack — so the peer path starts working the day the hub mints, with no runner change. **This is not ORCH-220**: that row is the push *direction* (plan S10), and this is the authorisation both directions need before either can move a byte | `m11-pack-layout`, 2026-10-30, *Not done* 1, drafted in full; the absence of a row verified on the board with the detector proven first — `capability` matches twice and neither match is a live row about minting | herd-step | owed | Mint the token where the ask arrives, push `FromHub::ExpectTransfer` to the **holder** over the holder's own already-authenticated link and answer `FromHub::TransferCapability` to the asker (D209, D200): random, short-lived at `PACK_CAPABILITY_LIFETIME`, scoped to one pack, one peer, one direction and one object list. **It owns `server.rs`'s pack arms and nothing else** — `a_pack_may_be_fetched` is already the arrival-window predicate it needs, which is D150's finding applied once already. Then the two-runner end-to-end (plan S11) and ORCH-220's push become reachable; neither is before this | M11's layout step landed | `herd-hub/src/server.rs` the `ToHub::AskTransferCapability` arm and `a_pack_may_be_fetched`; `herd-runner/src/endpoint.rs` `Granted::pushed_by_hub`; `herd-runner/src/lib.rs` `ask_for_a_capability`, `transfer_capability`, `pack_refused`; `herd-proto/src/pack.rs` `TransferCapability`, `ExpectTransfer`, `PACK_CAPABILITY_LIFETIME`; **D209**, **D200**, **D10**; ORCH-159, ORCH-220, ORCH-253 |
```

And one row for the prose this landing owes, which belongs with
ORCH-221 rather than beside it:

```markdown
| ORCH-NEW | **Four documents do not carry M11's `{in}`/`{out}` chain, which is now live.** ORCH-221 already names the three the runner's new modules left behind; this adds what the layout step itself changed and is filed separately because it is about behaviour and not about an inventory. `docs/plan.md` M11 still describes the layout as unbuilt; `docs/coverage.md`'s E-12 and E-17 rows do not say a declared command's pack now travels; and nothing in `docs/joining.md` or `docs/operating.md` tells an operator the sentence her runner will print — a declared command that names `needs` waits at the dispatch until a pack is registered for its commit, which is the one behaviour change a consumer sees from this landing | `m11-pack-layout`, 2026-10-30, *Not done* 7, drafted in full | herd-step | owed | Prose only, so `check-docs.sh` and the batch's own gate. Fold into ORCH-221 if that row is taken first and the same hand is in `plan.md` anyway; otherwise it is one pass naming the constant behind every figure and giving each citation a declared base commit (D239). **The one sentence that must reach an operator** is the waiting one, because it is the only way this landing can look like a hang | ORCH-221 | `docs/plan.md` M11; `docs/coverage.md` E-12, E-17; `docs/joining.md`; `docs/operating.md`; `history/2026-10-30-m11-pack-layout.md`; ORCH-221, ORCH-274 |
```

## Decisions needed

None. Everything above is inside the mandate and reversible, and
nothing here is a question about *what* Herd is or promises.

One thing to **report** rather than ask, and it is already in *Risks*
above: the consumer's own `commands.toml` may declare `leaves` at a
path that is not a pack directory, exactly as Herd's own fixture did
since M10. From the release that carries this, such a command keeps
running and its artefact is simply not kept, with a sentence on the
runner's stderr saying so; and a command declaring `needs` for a key
nothing leaves **waits** rather than running. Neither is a defect and
neither can be fixed from our side of the fence (D161).

## Resume point

The hub's half of D209 — the drafted `ORCH-NEW` above. One worktree on
`herd-hub/src/server.rs`'s pack arms: mint the token where the ask
arrives, push `ExpectTransfer` to the holder over the holder's own
link, answer `TransferCapability` to the asker. The runner side needs
nothing: `herd-hub/tests/pack_chain.rs` is the fixture to extend, and a
second runner started with `--mesh-listen` on loopback is what turns
its `SameHost` assertion into a `Peer` one.

## Processes

Jobs submitted through the queue's `herdctl`, each a separate job on
`queue-local`: `j0000000093` and `j0000000098` (builds),
`j0000000100` (the e2e), `j0000000102` and `j0000000103` (the two
detector runs), `j0000000105` (the four Rust gates, red) and
`j0000000108` (them again, green). No process was started outside Herd
and nothing was signalled: `PID 662711`, `565152`, `3006543` and
`3021002` were not touched.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m11-pack-layout`, measured
with its profile named before its number, and removed once the branch
was pushed — the figure and the removal are in the agent's report.
