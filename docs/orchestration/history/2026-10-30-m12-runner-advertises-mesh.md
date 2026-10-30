# m12-runner-advertises-mesh — handoff 2026-10-30

- **Item:** ORCH-NEXT — the row `m12-hub-mints-transfer` drafted
  (`history/2026-10-30-m12-hub-mints-transfer.md`, section
  *`docs/orchestration/board.md` — one new row*). It has no number yet:
  that branch is unlanded, so the merger allocates it.
- **Agent:** herd-step
- **Branch / commit:** `m12-runner-advertises-mesh` / <sha>, based on
  `origin/m12-hub-mints-transfer` `20150d4` — **not `main`**. Three
  branches chain here and each needs the one below it:
  `m11-pack-layout` `ae1188f` (the runner lays packs out) →
  `m12-hub-mints-transfer` `20150d4` (the hub mints the capability) →
  this one. `herd-hub/tests/pack_chain.rs` exists only on this stack,
  and so does the runner's production capability-ask path, so `main` is
  not a base from which this could be built or tested.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The defect, and what replaced it.** `herd-runner` sent
`mesh_endpoint: None` in the one `ToHub::Hello` it builds —
unconditionally, whatever `--mesh-listen` it was given
(`herd-runner/src/lib.rs:4406` at the base). It now sends the endpoint
it really bound.

The sequencing is the design and not an implementation detail, because
`--mesh-listen 127.0.0.1` means port **0**: the kernel picks it, at bind
time, in a task started *beside* the redial loop — and
`PROTOCOL_VERSION` is frozen at 3 with no frame that revises a mesh
endpoint later. So:

1. **`herd-runner/src/endpoint.rs`** gains `Advertisement` — three
   values, `Unsettled`, `Nothing`, `At(SocketAddr)` — and `Advertised`,
   a `tokio::sync::watch` wrapper with `say`, `now` and `settled`.
   `serve_while` takes one and writes it at **every** point where the
   socket's existence is decided: `At(advertise)` beside the
   `Said::Listening` sentence, from the same line, so the figure an
   operator reads and the figure the hub is told cannot drift;
   `Nothing` before the loop when there is no mesh address, on a bind
   that failed, when the owner's switch closes the socket, and on a turn
   where no socket exists and none is allowed. The healthy arm
   `(Some(_), true)` says nothing and **rebinds nothing**, which is the
   arm a reconnect's hello reads its endpoint through. The `_ => {}`
   wildcard is now the two explicit arms, so a fourth state could not be
   added without deciding what it advertises.
2. **`herd-runner/src/lib.rs`** gains `Session::advertised` beside
   `Session::granted`, for the same reason the capacity is there: it is
   true of the *runner*, not of a connection, and a value held by a
   connection would be lost on a redial. `run_until` hands it to the
   endpoint task and then **awaits it once, before `serve_forever`
   dials anything**, printing what the handshake will say. The one path
   with no endpoint task at all — a pack store that will not open —
   settles `Nothing` in `run_until` itself, or that runner would never
   say hello.
3. **The hello reads, it does not wait.** `serve_once` uses
   `now().on_the_wire()`. Deliberate and load-bearing: `serve_once` is
   also the whole runner for a large part of this workspace's suite,
   started against a hub with no endpoint task beside it and a bare
   `Session::new()`, and an await there would be a hello that never
   left. Eighteen call sites in `herd-hub/tests` do exactly that.

**Nothing was weakened and nothing moved.** `herd-proto` is untouched
(`Hello.mesh_endpoint` already existed). `herd-runner/src/mesh.rs` is
untouched: the address advertised is the one `Mesh::resolve` already
accepted at startup with the bound port substituted, and the
invariant-3 guards of D222 are byte for byte what they were. Loopback is
advertisable because `permitted` already returns `Ok` for it before the
routing table is consulted — a deliberate clause, and the only mesh a
single workstation running a two-runner end-to-end has. No
`TcpListener` was named anywhere under `herd-runner`.

**E-12's cross-machine half is discharged.**
`herd-hub/tests/pack_chain.rs` gains
`a_pack_moves_from_one_real_runner_to_another`: two real runners, the
producer labelled onto one and the consumer onto the other,
and the pack moving peer to peer. It asserts, in order: the holder
advertises a real port before it says hello and the asker advertises
nothing; the producer's pack is registered by the holder and **not** by
the asker; the consumer's own tool reads the pack's bytes out of
`{in}/<key>` and its `pack.meta`; the hub reports `FetchPath::Peer` —
not `SameHost` — with `1 object(s) fetched, 0 already held` and a
**positive** byte figure; and the one string that exists only inside the
object's bytes is **nowhere** in the hub's journal or under its state
root.

**The figure, with its unit and the direction of its error.** **114 body
bytes** crossed the mesh on the run of 2026-10-30 quoted in *Gates*,
counted off the socket by the fetcher. It is a **lower bound** on what
left the holder: the peer leg's header lines are real bytes on the same
socket and are not in this count, and a transfer aborted mid-object
contributes none. It is a figure about this fixture's one object — the
two lines the fixture tool writes — and it is **not** an acceptance
criterion for anything: the assertion is `bytes > 0`, because nought is
what both `SameHost` and a failed transfer report and only a positive
figure says a socket carried something.

**The same-host test is kept, deliberately and against the brief's
literal wording.** The brief said to *turn* `pack_chain.rs`'s transfer
assertion from `SameHost` into `Peer`. A second test is added instead
and the first is left intact, because D10 has two paths and the
same-host path's **nought** network bytes is the assertion that clause
rests on — the file's own header calls it that — and it is the common
case on nearly every job. Turning the one assertion round would have
discharged E-12 by deleting the measurement of D10's first path. Said
here rather than quietly.

**Three more promises, each asserted.** A new file,
`herd-hub/tests/runner_advertises_its_mesh.rs`, reads
`Hello.mesh_endpoint` off the wire against the real binary, with a
hand-written hub in the test process (`silent_hub.rs`'s shape, and its
reason: reading one field out of one frame is the thing a hand-built
peer does better, and the real hub exposes no surface to ask it back).
It asserts: a runner with no mesh flag advertises `None` and says it is
outbound-only; a runner given `--mesh-listen 127.0.0.1` advertises a
port that is **not** the nought it asked for, with a socket really
there, reached from outside its process; and after the hub drops the
first link the **second** hello carries the same endpoint with the same
socket still open — nothing rebound.
`pack_chain.rs`'s
`an_outbound_only_holder_is_refused_by_name_and_nothing_moves`
is the same two-machine fixture with the flag removed: the refusal names
the state and the key, and **no** transfer is reported at all.

**Three unit tests in `endpoint.rs`** carry the parts a process cannot
show cheaply: the advertised port is the bound one and the same as the
operator's own sentence, with `report.opened == 1`; an outbound-only
runner settles immediately rather than leaving a dial waiting; and a
runner switched `off` advertises nothing, starts advertising when it is
switched on, and stops again when it is switched off — which is the
clause that stops the hub minting against a port the process no longer
holds.

## Not done, and why

- **The push direction.** A holder with no advertisable address still
  cannot be reached and nothing pushes to the asker instead. That is
  ORCH-220 and plan S10, named by the hub's refusal and by this
  branch's own `an_outbound_only_holder_is_refused_by_name_and_nothing_moves`.
- **A retry on `NotPushedByHub`.** Not built, and the reason is a
  finding rather than a preference: *Risks and findings* 1, and a
  drafted board row.
- **`docs/decisions.md` and `docs/orchestration/board.md` are
  untouched.** The entry and both rows are drafted below, in full, as
  the brief asked; the merger allocates the number.
- **`docs/status.md`, `docs/plan.md` and `docs/coverage.md` are
  untouched.** The brief named none of them, and three branches are
  unlanded ahead of this one, so a restatement written here would be
  written against a tree that does not exist yet. What a later pass
  owes, named so it is not lost: `docs/coverage.md`'s **E-12** row
  (line 69) is `part`, and the sentence it carries — *"the runner binds
  and drops both"*, *"`ToHub::RegisterPack` is constructed nowhere
  under `herd-runner/src`"*, *"no job is ever given a pack"* — is
  refuted by this stack, which registers, lays out, and now moves a
  pack between two machines. **E-17** (line 74) carries the same
  refuted clause. Neither may be recounted from this branch: the
  measurement is of `main`, and this is not `main`.

## Gates

Every compile, test and gate on this branch went through our Herd
instance with `/mnt/ssd_980/herd/queue/bin/herdctl`, `--label build
--memory-mib 4700`, `--verdict-to`, and
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-runner-advertises-mesh` on
the command line. `./scripts/bench-preflight.sh` was run before each and
said `ok, safe to start` every time. **No local run of anything.**

Runs made **before** this file was committed:

1. `cargo clippy --workspace --all-targets -- -D warnings` — **red
   once**, green after. The red was mine and is reported as red: `error:
   you seem to be trying to use match for destructuring a single
   pattern` in the new `runner_advertises_its_mesh.rs`
   (`-D clippy::single-match`), fixed by a let-else. Second run:
   `Finished dev profile … in 20.12s`, `"passed": true`,
   `"exit_code": 0`.
2. `cargo test -p herd-runner --lib endpoint::` — 8 passed, 0 failed,
   0 ignored, 299 filtered out, including the three new ones.
3. `cargo test -p herd-hub --test runner_advertises_its_mesh` — 2
   passed, 0 failed.
4. `cargo test -p herd-hub --test pack_chain` — 4 passed, 0 failed,
   including `a_pack_moves_from_one_real_runner_to_another` and
   `an_outbound_only_holder_is_refused_by_name_and_nothing_moves`.
5. **Four mutations, each applied alone and reverted from a byte copy.**
   Their verbatim output is in this agent's report, with the failing
   assertion named. Summary, and the clause each one reddens:
   - **M1** the holder is given no `--mesh-listen`:
     `a_pack_moves_from_one_real_runner_to_another` **RED**, at
     `real_herd/mod.rs:992` — *"timed out after 30s waiting until the
     holder says what it advertises"*, with the asker's own
     *"no mesh address … listens on nothing"* under it. It goes red at
     the premise and not at the `Peer` line, which is honest and is
     said that way: with no advertisement there is nothing to mint
     against, so the chain stops before a transfer is reported at all.
     The positive form of the same fact is a green test —
     `an_outbound_only_holder_is_refused_by_name_and_nothing_moves`
     asserts there is **no** `moved pack` line of any path.
   - **M2** the fixture tool echoes the whole object instead of its
     first line, so the needle reaches a job log the hub keeps: **RED**
     at the hub scan, and it names where — *"a byte of the pack passed
     through the hub …  left: Some("the hub's state root, at
     jobs/j0000000004/log")"*. The clause: the detector fires on a
     known-bad fixture, so its silence means something. **Run twice** —
     the first run was invalid, because the byte copy it restored from
     had been retaken while M1 was still applied (my error, named in
     *Risks and findings* 7); the valid run is the second, and the same
     job then put the file back and ran it green, printing
     `herd-test: 114 body byte(s) crossed the mesh`.
   - **M3** `At(advertise)` becomes `At(wanted.advertise)`, so the port
     asked for is advertised instead of the port bound: unit test
     **RED** — *"the advertisement carries the nought that was asked
     for and not the port the kernel gave: 127.0.0.1:0"* — and process
     test **RED** — *"the runner advertised the nought it asked for …
     which is a port no peer can dial: 127.0.0.1:0"*. The clause: the
     advertisement carries the kernel's port.
   - **M4** the healthy arm drops the listener every turn, so the
     endpoint rebinds under the link: **not a detector.** Run twice:
     first run the process test RED and the unit test GREEN, second run
     **both GREEN**. It is therefore quoted as neither a pass nor a
     proof, and the clause it was meant to redden — a redial advertises
     the same endpoint — has **no watched-red**. *Risks and findings* 2
     says why, and says what the test does prove instead.
6. `cargo fmt --all --check` — **red once**, green after, and the red is
   reported as red: three diffs, all mine and all in the new code
   (`pack_chain.rs:1062`, `runner_advertises_its_mesh.rs:274`,
   `endpoint.rs:1634`), each one rustfmt joining or splitting a line I
   had wrapped by hand. Second run: `"passed": true`,
   `"exit_code": 0`. That job waited several minutes in the queue
   behind another agent's — the queue working, not resubmitted.
7. `./scripts/check-docs.sh` on the tree before this file: `docs ok`.
   `./scripts/check-orchestration.sh`: `324 passed, 0 failed`. Both run
   locally and deliberately: they need no toolchain, which is the one
   exception `CLAUDE.md` names for a prose check, and both are run
   again by `gate.sh` as its steps 1 and 2.

The **last** run, on the committed tree, is the one this file cannot
quote: `./scripts/gate.sh`, through Herd, with its log at
`<scratchpad>/m12-runner-advertises/gate.log` and its receipt under
`<worktree>/target/herd-gate/`, plus `./scripts/check-docs.sh` with its
output at `<scratchpad>/m12-runner-advertises/check-docs-final.txt`. The
verbatim output of both is in this agent's report.

**Skips.** 12 `HERD-SKIPPED`, all of them the cgroup-delegation family
of `scripts/expected-skips/local.txt`, and the `#[ignore]` census
**COMPLETE at 38**, compared as a list and not as a count (D187). This
branch adds **no** skip and **no** `#[ignore]`: the anchored census
`grep -rc '^[[:space:]]*#\[ignore' herd-*/ herdctl/ --include='*.rs'`
sums to 38 here and
`grep -c '^HERD-IGNORED' scripts/expected-skips/ignored.txt` is 38 —
the unanchored form answers 51 and is the wrong figure
(`gotchas/an-attribute-census-needs-an-anchor.md`).

## Risks and findings

**1. A peer's refusal carries a sentence and no name, so the fetcher
does not retry the one refusal that is nobody's fault.** This is the
brief's open question answered, and the answer is *no retry*, for a
reason the brief did not have.

The race is real and was named below this branch: the hub queues
`ExpectTransfer` to the holder and answers the asker second, delivery
over two connections is concurrent, and an asker that dials inside that
window is refused `NotPushedByHub` — for which the frozen protocol
gives the hub no acknowledgement to wait out. A fetcher that retried
once would close it.

It cannot. `herd_proto::pack::PeerRefusal` is `{ why: String }`, one
field. `herd_runner::endpoint::Refusal` **is** a named enum, and its own
doc says why — *"a fetcher that got only a sentence could not tell 'ask
the hub for a fresh capability' from 'stop asking'"* — but the name is
flattened into prose at the wire and the fetcher receives only the
sentence. So a retry would have to match a substring of an operator's
sentence, which breaks the day somebody rewords it, and would retry
`Expired` and `OutOfScope` too if the match were loose. Giving the frame
a code is a `herd-proto` change, which invariant 4 forbids me: **I
stopped and it is a drafted board row**, not an edit.

What makes that acceptable rather than a defect left open: when the race
does fire, the job-level retry absorbs it. The fetch returns
`Unavailable`, `Session::lay_out_packs` sends `ToHub::PackUnavailable`
with the sentence, the attempt is handed back (`job.rs`, *"handing
{job_id} back"*), and the hub re-dispatches — a second attempt asks for
a fresh capability and races again, independently. So the cost is
latency and a confusing journal line, not a lost pack. **And it did not
fire in any run made here**: the two-machine test was green on this
branch's own code **twice** before the gate (the targeted `pack_chain`
run and M2's own restore-and-rerun), plus whatever the gate's suite
makes it — three runs is a small number and is said as one, not as a
frequency. The window is the holder reading a frame already on its
socket against the asker reading a frame, waking a task, dialling TCP
and writing a request, which is why the holder wins; it is not a
guarantee. **No sleep was added to the test** and no ordering was
papered over.

**2. One clause has no watched-red: "a redial advertises the same
endpoint".** The mutation written for it — drop the listener on every
healthy turn of `serve_while`'s loop, so the endpoint rebinds under the
link — is **not a detector**. Run twice on identical code it gave two
different answers: first the process test red and the unit test green,
then both green. So it is quoted as neither, and the clause is
**unproven by mutation**.

Why a mutation is hard to find here is itself part of the answer:
nothing in `serve_while` rebinds while the socket is open, and
`Advertised` holds one value for the process, so there is no single line
whose removal makes a second hello differ from the first — a rebinding
endpoint is a different design, not a changed line. What the test does
prove, positively and repeatably, is that after the hub drops the first
link the second hello **did** carry the first's endpoint and the socket
at that address was still accepting from outside the process. That is a
measurement of the behaviour and not of a guard, and the difference is
named here rather than left for a reader to assume.

The unit test's `report.opened == 1` is worth one sentence of warning
for the same reason: under the M4 mutation it passed, because that test
finishes inside one turn of a 5 ms poll and never sees a second bind. A
reader could take it for a guard against rebinding. It is not one.

**3. The "no byte through the hub" assertion is a scan, and its needle
had to be made.** The hub keeps a pack's manifest, its `pack.meta` and
the job's log by design — metadata rides the control plane, which is
D10's own clause — and the consumer's tool prints the object's first
line into that log. So the first version of this assertion would have
fired on evidence of its own making. The fixture's object now carries a
**second** line that nothing ever echoes, and that is the needle. The
detector was proven to fire: mutation M2 made the tool print the whole
object instead of its first line, the needle reached the job log on the
hub, and the assertion named the hub's state root.

**4. `run_until` prints one new line on every runner start.** *"this
runner's handshake will say it serves pack content …"*, before the first
dial. It is on stderr beside the other caps a runner prints and it is
what a two-machine test reads the advertised endpoint out of. It is new
output on a surface other tests assert against; the suite is the
measurement and it is green.

**5. The brief's literal instruction was not followed, deliberately.**
It said to *turn* `pack_chain.rs`'s transfer assertion from `SameHost`
into `Peer`. The same-host test is kept and a second test added, because
the nought network bytes of D10's first path is the assertion that
clause rests on and the common case on nearly every job. The reading is
mine and it is reported rather than left to be noticed.

**6. What is still not true.** No **push**: a holder that listens on
nothing cannot be dialled, is refused by name, and nothing in this build
pushes to the asker instead (ORCH-220, plan S10). Nothing here may be
reported as cross-machine packs working in both directions.

**7. One mutation run of mine was invalid, and the cause is worth the
next agent's attention.** The mutation script restored each file from a
byte copy taken before the first mutation. I then edited
`pack_chain.rs` **while that job was running** — adding the byte-figure
print — and refreshed the byte copy from the worktree, which at that
instant still carried M1's mutation. So the copy was poisoned, every
later `restore` reinstated M1, and M2's result was a measurement of M1.
Caught by reading the failure message rather than the exit status: M2's
panic was *"waiting until the holder says what it advertises"*, which is
M1's sentence and not M2's. Fixed by reverting the one line, re-taking
the copy, and running M2 alone; both figures in *Gates* are from that
run. The rule this cost me: **a mutation job owns the files it mutates
for its whole run**, and a tree is not a scratchpad while a job holds
it.

## Drafted for `herd-merger`

Two things, both in full and both fenced so that their own line lengths
are not measured against the 79-column rule this file is measured
against: one `docs/decisions.md` entry headed `D-NEXT`, and two
`docs/orchestration/board.md` rows. **Nothing in `docs/decisions.md` or
`docs/orchestration/board.md` was edited on this branch.** The merger
allocates the number and gaps stay (D148, invariant 8).

### `docs/decisions.md` — one new entry, appended

Paste as it stands, replacing `D-NEXT` with the allocated number. Every
line of it is already inside 79 columns unfenced, measured.

```markdown
## 2026-10-30 — D-NEXT A runner awaits its bind before its first dial

`herd-runner` sent `mesh_endpoint: None` in the one `ToHub::Hello` it
builds, unconditionally and whatever `--mesh-listen` it was given
(`herd-runner/src/lib.rs:4406` at `ae1188f`; `git grep -n mesh_endpoint`
over `herd-runner` returned that one line and nothing else). So
`Holder::endpoint` was `None` for every real runner,
`fetch::reachable_holder` refused every holder as outbound-only, and
the hub — correctly — minted no capability against an address nobody
had advertised. **No pack could move between two machines at all**, and
the half that was missing was one field.

Filling it is not a fill, because of one fact about the flag.
`--mesh-listen 127.0.0.1` means port **0**: the kernel picks it, at bind time,
in a task started *beside* the redial loop — and `PROTOCOL_VERSION` is
frozen at 3 with **no frame that revises a mesh endpoint later**
(invariant 4). So the address is not knowable when a cold start's hello
is built, and a hello that guessed would be uncorrectable for the life
of the link: the hub would mint capabilities against a port nothing
holds, and every peer sent there would be refused for a reason no
operator could see.

**1. The dial awaits the bind.** `herd_runner::run_until` starts the
endpoint task, then waits once on `endpoint::Advertised::settled()`
before `serve_forever` dials anything. The rejected alternative was to
advertise only an explicit `ADDR:PORT` and refuse to claim an endpoint
for the bare form. It is cheaper and it was rejected because it leaves
a flag that silently does less in one of its two spellings:
`--mesh-listen 127.0.0.1` would bind, serve, and never be offered a
transfer. **A flag that half-works is worse than one that refuses.**

**2. The wait cannot be where a runner hangs, and that is a property of
the type and not a timeout.** `Advertisement` has three values —
`Unsettled`, `Nothing`, `At(SocketAddr)` — and every path through
`endpoint::serve_while`'s first turn leaves one of the last two: no mesh
address settles `Nothing` before the loop, a bind that **fails** settles
`Nothing` and the runner dials anyway and runs every job, and a runner
started switched `off` settles `Nothing` because it holds no socket. The
one path with no endpoint task at all — a pack store that will not open
— settles `Nothing` in `run_until` itself. There is no timeout on the
wait because there is nothing to time out: a runner that cannot learn
its own endpoint is an outbound-only runner, which is a named state the
hub refuses as a holder by name, never a runner that refuses to start.

**3. The advertisement follows the socket, and never changes under a
link.** It is a `tokio::sync::watch` and not a `OnceLock`, because the
owner's switch closes the socket and that runner stops being a holder:
the **next** connection has to say so, or the hub goes on minting
against a port the process no longer holds. What it must never do is
change mid-link, and it cannot: the endpoint is bound once and outlives
every connection, so a reconnect reads the value already settled and
**rebinds nothing**. `serve_once` therefore *reads* (`now()`) where
`run_until` *waits* (`settled()`) — deliberately, because `serve_once`
is also the whole runner for much of this workspace's suite, started
with no endpoint task beside it, and a hello that waited there would be
a hello that never left.

**4. No guard was weakened, and no protocol moved.** The address
advertised is the one `mesh::Mesh::resolve` already accepted at startup,
with the bound port substituted — the invariant-3 guards of D222 are
untouched, byte for byte. Loopback is advertisable because `mesh.rs`'s
`permitted` already returns `Ok` for it before the routing table is
consulted, which is a deliberate clause and the only mesh a single
workstation running the two-runner end-to-end has. `herd-proto` is
unchanged: `Hello.mesh_endpoint` already existed.

**What this makes true, measured.** A pack moves from one real runner to
another, peer to peer. `herd-hub/tests/pack_chain.rs`'s two-machine test
reports `FetchPath::Peer` with a positive byte figure and `1 object(s)
fetched, 0 already held`, the consumer's own tool reads the pack's bytes
out of `{in}/<key>`, and the one string that exists only inside the
object's bytes is **nowhere** in the hub's journal or under its state
root (D10: the hub is registry and authority, never the byte path).

**What it does not make true.** The **push** direction is still absent:
a holder that listens on nothing cannot be dialled, is refused by name,
and nothing in this build pushes to the asker instead (ORCH-220, plan
S10). And the `ExpectTransfer` race named by the branch below this one
is **not** closed: the hub tells the holder first and answers the asker
second, delivery over two connections is concurrent, and an asker
dialling inside that window is refused `NotPushedByHub`. No retry was
added, for the reason in this branch's handoff:
`herd_proto::pack::PeerRefusal` carries only a `why` sentence, so a
fetcher cannot name that one refusal without widening the frozen
protocol, and a retry keyed on a substring of an operator's sentence is
worse than the race.
```

### `docs/orchestration/board.md` — two rows

The **first** closes the row `m12-hub-mints-transfer` drafted and goes
to *Owed* as `done`, or replaces that row if the merger has already
filed it with a number. The **second** is new and goes to *Open*: it is
the finding of *Risks and findings* 1, and it needs a decision that is
not mine because it would move `herd-proto`. No literal `|` in any
cell; eight cells each, checked.

```markdown
| ORCH-NEXT | **The runner advertises the mesh endpoint it really bound, and a pack moves between two machines.** `herd-runner` sent `mesh_endpoint: None` in the one `ToHub::Hello` it builds, unconditionally and whatever `--mesh-listen` it was given, so `Holder::endpoint` was `None` for every real runner and the hub correctly refused to mint against an address nobody advertised. It now awaits the bind before its first dial: `endpoint::Advertised`, three-valued, written by the one step that binds and read by every hello — `--mesh-listen 127.0.0.1` means port `0` and the kernel picks it, and the frozen protocol has no frame that revises a mesh endpoint later. Every spelling of the flag works; a runner with no flag, one whose bind failed and one started switched `off` each settle on "nothing to advertise" immediately, dial anyway, run every job, and are refused as holders **by name**. `herd-proto` untouched, invariant-3 guards untouched. **E-12's cross-machine half is discharged**: `herd-hub/tests/pack_chain.rs`'s two-machine test moves a pack from one real binary to another, reports `FetchPath::Peer` with a positive byte figure and `1 object(s) fetched, 0 already held`, and asserts the one string that exists only inside the object's bytes is nowhere in the hub's journal or under its state root | the row drafted by `m12-hub-mints-transfer`, 2026-10-30, taken the same day by `m12-runner-advertises-mesh` | herd-step | done | land with the batch; the decision entry is drafted in the handoff, headed `D-NEXT` | the hub's half `m12-hub-mints-transfer` and the layout `m11-pack-layout`, both unlanded ahead of this branch | branch `m12-runner-advertises-mesh`; `docs/orchestration/history/2026-10-30-m12-runner-advertises-mesh.md`; `herd-runner/src/endpoint.rs` `Advertised` and `Advertisement`; `herd-runner/src/lib.rs` `Session::advertised` and the await in `run_until`; `herd-hub/tests/runner_advertises_its_mesh.rs`; `herd-hub/tests/pack_chain.rs`; three mutations watched red and a fourth reported as no detector; D209, D200, D10, D11, D222; invariant 3; invariant 4; E-12 |
| ORCH-NEXT | **A peer's refusal carries a sentence and no name, so a fetcher cannot retry the one refusal that is nobody's fault.** `herd_proto::pack::PeerRefusal` is `{ why: String }`. `herd_runner::endpoint::Refusal` **is** a named enum — and its own doc says why, *"a fetcher that got only a sentence could not tell 'ask the hub for a fresh capability' from 'stop asking'"* — but the name is flattened into prose at the wire. The consequence has one live instance: the hub queues `ExpectTransfer` to the holder and answers the asker second, delivery over two connections is concurrent, and an asker dialling inside that window is refused `NotPushedByHub` — which the frozen protocol gives the hub no acknowledgement to wait out. A fetcher that retried once would close the race; it cannot name the refusal to retry it, and a retry keyed on a substring of an operator's sentence would break the day the sentence is reworded. **Never observed in the two-machine end-to-end** over the runs made on this branch, which is a small number and not a frequency, and the job-level retry absorbs it when it does fire — the attempt is handed back with the sentence, the hub re-dispatches, and the fetch asks again — so this is a latency and a confusing journal line, not a lost pack | `m12-runner-advertises-mesh`, 2026-10-30, while deciding whether its fetcher should retry; the race itself was named by `m12-hub-mints-transfer` | herd-planner | open | **Decide whether the peer leg carries a refusal code beside its sentence**, which is a `herd-proto` change and therefore a protocol decision and not a fix. Two shapes to weigh: a `code` field on `PeerRefusal` mirroring `endpoint::Refusal`'s variants, which is additive and skipped-when-absent like every other field this protocol has grown; or leaving the wire alone and closing the race at the hub, which the protocol affords no acknowledgement for. The second question the brief should settle is what a fetcher is allowed to do with a code once it has one: *one* retry on `NotPushedByHub` alone, with the reason on the code, and nothing on any other refusal — a fetcher that retried `Expired` or `OutOfScope` would be a fetcher that hammered a holder for a capability it will never be given | invariant 4 is frozen at `PROTOCOL_VERSION` 3, so this needs Alexa's word on whether the protocol may move or whether no-compatibility-to-preserve covers it | `herd-proto/src/pack.rs:863`; `herd-runner/src/endpoint.rs` `Refusal` and its doc; `herd-runner/src/fetch.rs` `pull`'s `PeerRefusal` arm; `herd-hub/src/server.rs` the `AskTransferCapability` arm and its *"the holder is told first, and the asker second"* comment; `history/2026-10-30-m12-hub-mints-transfer.md`; `history/2026-10-30-m12-runner-advertises-mesh.md`; D209, D150 |
```

## Decisions needed

**Question.** May the peer leg's `PeerRefusal` carry a refusal **code**
beside its sentence? It is a `herd-proto` change on a protocol
invariant 4 freezes at `PROTOCOL_VERSION` 3, so it is not mine to make.

**Recommendation.** Yes, additive and skipped-when-absent like every
other field this protocol has grown, mirroring
`herd_runner::endpoint::Refusal`'s variants — and then *one* retry in
the fetcher on `NotPushedByHub` alone, with the reason written on the
code and nothing on any other refusal. `CLAUDE.md` says there is no
compatibility to preserve before v1.0.0, which is why this is a
question about whether the protocol *should* move and not about whether
it *may*.

**Fallback.** Leave the wire alone. The race stays open, the job-level
retry absorbs it, and the cost is latency and one confusing journal
line rather than a lost pack — which is what this branch ships.

## Resume point

**The push direction (ORCH-220, plan S10)**, which is now the only half
of E-12 that is absent: a holder that listens on nothing pushes a pack's
objects to the asker on a connection it opens, because it needs nothing
inbound to do so. Its fixture already exists on this branch —
`an_outbound_only_holder_is_refused_by_name_and_nothing_moves` is
exactly the state the push has to turn from a refusal into a transfer.

```sh
git fetch origin m12-runner-advertises-mesh
git worktree add -b <task> <path> origin/m12-runner-advertises-mesh
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/<task>
```

## Processes

PIDs started: none held. Every compile, test and gate ran as a Herd job
on `queue-local` (job ids in the verdict files under
`<scratchpad>/m12-runner-advertises/*.json`); this agent started no
long-lived process of its own and signalled nothing. One shell of its
own making hung on a stray `cat` reading stdin and was killed by its own
pid, 2925287, before it had written anything; nothing else was signalled
and no `pkill` or kill-by-name was used anywhere.

Build tree: `CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/m12-runner-advertises-mesh`,
never under the worktree and never on the root filesystem. Its measured
size and its deletion are in this agent's report, because both happen
after this file is committed.
