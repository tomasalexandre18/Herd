# runner-announces — handoff 2026-10-30

- **Item:** ORCH-232
- **Agent:** herd-step
- **Branch / commit:** `runner-announces` / three commits, the last
  being this handoff; the code tip is
  `562ddbf`, based on origin/main `6d10427`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The whole of ORCH-232: a runner that announces what it holds, and the
measurement against both real binaries that nothing in this mechanism
had.

### The product (commit 1, `903ff72`)

Before this, **`herd-runner` constructed `ToHub::PacksHeld` in no
place**, so every running hub's holdings map was empty whatever was on
a runner's disk. The hub cannot ask either — it does not send
`FromHub::ReadPacksAgain`, withdrawn on a measurement (ORCH-233) — so
the announcement is unprompted or there is none.

Three pieces, in `herd-runner/src/`:

**`PackStore::held_packs()`** (`packstore.rs`) answers
`Result<Vec<PackId>, String>`: one `read_dir` of `manifests/` and one
`stat` per entry. A directory counts when its name is a sha256 of the
shape `check_object_digest` accepts **and** it holds a `manifest.tsv`,
which is `PackStore::holds`'s own rule — and the manifest is written
last, so its presence is the store's statement that a complete copy
landed.

Two properties are deliberate and both are in the method's doc. The
name is **taken as a name and never re-hashed**, exactly as
`PackId::as_announced` says an announced id is: re-hashing would read
every manifest on every announcement, and a pack of 557 files carries
on the order of 100 KiB of manifest (the consumer's issue #16,
measured 2026-10-13: 401.6 MiB in 557 files), so a store of a few
hundred packs would be tens of megabytes read each time a job ends.
And an unreadable `manifests/` is an **error, never an empty list**: an
empty announcement is the frame set's only retraction, so a `read_dir`
that failed would otherwise tell the hub to forget every holding this
runner really has — a false statement built out of a filesystem error.

**`announcement_frames(Vec<PackId>) -> Vec<PacksHeld>`** cuts at
`PACKS_HELD_PER_FRAME` and **always answers at least one frame**. Every
frame but the last carries `more: true`; the last carries `more:
false`, which is what applies the announcement. An empty holding
answers one frame with an empty `packs` and `more: false`, because that
frame is the frame set's only retraction and no announcement at all
says nothing.

`PACKS_HELD_PER_FRAME` is **512**, and the figure's provenance is in
its doc: the hub refuses a frame over `MAX_FRAME_BYTES`, which is
`1 << 20` (`herd-hub/src/server.rs:167`, read 2026-10-30). A `PackId`
is 64 hex characters, so one id costs 67 bytes of JSON array and 512 of
them is about 34 KiB — that last figure is a **computation and not a
reading**; what is measured is the test's assertion that the widest
frame, encoded through the production encoder `herd_proto::to_frame`,
is under `1 << 20`. The number is deliberately far below what would
fit, because what a hub can read is a ceiling while this is what
decides whether the `more: true` path is walked by a store of a few
hundred packs or only by one of fifteen thousand.

**`Session::announce_packs_held(&Path) -> String`** (`lib.rs`) opens the
store, walks it, sends the frames and answers the line to say about it.
Two call sites:

1. the handshake, after `session.announce_capabilities()` and before
   `report_health` is spawned — on **every** connection including a
   reconnection, because the hub forgets a gone runner's holdings and a
   restarted hub never knew them;
2. `job.rs`, after the lines that name each pack a job left, **only
   when a pack really landed** (`leave_packs` answers an empty list for
   every job that declared no `leaves` and for every one whose declared
   artefact was refused).

A link that goes mid-announcement loses the whole of it by design:
nothing is applied until a frame arrives with `more: false`, the hub
forgets a gone runner's holdings anyway, and the next welcome announces
again from the disk.

### The measurement (commit 2, `562ddbf`)

`herd-hub/tests/packs_announced.rs`, two tests, **both binaries as
processes**. This is the thing the mechanism has never had:
`pack_frames.rs` drives five tests through a socket with a peer that
encodes its own frames, and what such a peer proves is that a hub told
about a holding records it — the sequence it asserts is the sequence
the test author typed. Until this file nothing measured whether the
`herd-runner` binary says anything at all.

The first, named at column nought because its own name is 76 characters
and 79 is the limit:

`the_real_runner_announces_what_it_holds_and_the_hub_records_every_frame_of_it`

plants `PACKS_HELD_PER_FRAME + 4` = **516** pack manifests in the
runner's store, starts a real hub and a real runner, and asserts the
hub's own journal: the accumulating frame
(`512 so far and more to come`) and the frame that applies. Nothing
asked for either: the test also asserts the runner never read an ask.
And the second,
`a_real_runner_that_holds_no_pack_says_so_and_the_hub_forgets_what_it_had`,
is the retraction: an empty store, and the hub answering
`every holding it had is forgotten`.

**Why 516 and not a smaller fixture.** A fixture at or below the window
is green while measuring nothing, because it never cuts — the same trap
`spool_offer_answered.rs` was caught by with one collection against a
window of eight, and the same `+ 4`. Four over the window forces
exactly two frames, so the `more: true` path is walked and the
remainder is not a whole window either. The boundary itself is covered
in the unit test: exactly `PACKS_HELD_PER_FRAME` is one frame, and it
applies — the off-by-one the other way would send an empty second
frame, which a hub reads as a retraction of what the first just said.

**What is planted, and what is not.** Each pack is a manifest directory
in the store's own shape: a 64-hex name that **is** the sha256 of the
`manifest.tsv` beside it. The objects are not planted; see finding 4.

### The board

ORCH-232's **state cell only**, `owed` to `done`, as the brief allowed
and nothing else in that file. A concurrent prose step owns the rest of
it.

## Not done, and why

- **ORCH-233 (the hub's ask) and ORCH-234 (`AssignJob`'s `packs_in` /
  `packs_out`) are untouched**, as the brief said. ORCH-233 is now
  **strictly cheaper and strictly less necessary**: the mechanism works
  without it, so restoring `ReadPacksAgain` is the optimisation the
  orchestrator ruled it is, and its blocker is unchanged — the seven
  brittle helpers in `link.rs` and `versions.rs`. ORCH-234 is no
  cheaper for this work.
- **`ToHub::RegisterPack` is still sent by nothing.** A job's
  `JobOutputs::packs` is printed and announced as a holding; it is not
  registered, so the hub learns *that this runner holds a pack* and
  never *which `(commit, key)` it is*. That was already true and is not
  this row's.
- **`fetch::fetch` has no production caller**, so the job path is the
  only place in this build where holdings change. Said in the comment
  at that site: when something calls the fetch, it announces there too
  or the hub's view goes stale in the other direction.
- **The registry's accumulated content is not read back in the
  end-to-end test.** `ToHub::PacksHeld` has no acknowledgement in the
  frame set (batch 15's finding 5), so there is no frame to read and no
  `herdctl` surface that answers it; the hub's journal is the only
  observation point two processes have. The accumulate-and-replace rule
  itself is proved by `pack_frames.rs` against a peer, and that
  division is deliberate rather than an omission.

## Gates

Run **before** this file was committed, on the branch with both code
commits in place. `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/runner-announces`.

`cargo fmt --all --check`: **clean**, exit 0 — on the second run. The
first was **red** (two rustfmt diffs in `packstore.rs`'s new tests) and
is reported in the agent's report with the trap that nearly hid it.

`cargo clippy --workspace --all-targets -- -D warnings`: **exit 0**,
`Finished dev profile ... in 18.03s`, no warning emitted.

`cargo build --workspace`: **exit 0**,
`Finished dev profile ... in 8.50s`.

`cargo test -p herd-runner --lib packstore::`: **16 passed, 0 failed,
0 ignored, 249 filtered out**, 0.09 s — four of the sixteen are new.

`cargo test -p herd-hub --test packs_announced`: **2 passed, 0 failed**,
13.89 s. The first run of that file was **1 passed, 1 failed** in
42.96 s, and finding 1 below is what it found.

`./scripts/test.sh` and `./scripts/check-docs.sh`: both named below,
with their output in the agent's report.

Preflight before the suite, joined with `&&` and never `;`
(`gotchas/a-gate-joined-by-a-semicolon-is-advice.md`): clearing, with
**load 4.29 on a ceiling of 9.00** and **16700 MiB of slice room
against a floor of 8192** — a margin, quoted as a margin and not as a
bar.

**The last runs, named and not quoted:** `nix develop --command
./scripts/test.sh` and `./scripts/check-docs.sh`, on the committed tree
including this file. Their verbatim output goes to the agent's report;
nothing is added to the tree after them.

**Skips:** expected **12 runtime `HERD-SKIPPED`** and **38 compile-time
`#[ignore]`s**, every one inherited and none introduced here — this
branch adds four `#[test]`s and two `#[tokio::test]`s and no
`#[ignore]` and no skip. The suite's own skip list is in the agent's
report; a `HERD-SKIPPED` line is copied as a skip and never counted as
a pass (D27).

## Risks and findings

1. **The hub's applied-announcement journal line names the last frame's
   count, not the holdings it recorded.** Measured 2026-10-30 through
   two real processes: the runner announced **516** packs in **2**
   frames and the hub printed `is announcing its packs, 512 so far and
   more to come` and then `holds 4 pack(s), which replaces what this
   hub had for it`. The registry accumulates all 516 — only the
   sentence is wrong. The cause is one line:
   `relay_from_runner` reads `announcement.packs.len()` off the frame in
   hand rather than what the registry now holds. **Not fixed here**:
   that is a `herd-hub/src/server.rs` change and this task's write path
   was `herd-runner/src/`. The test is written against what the hub
   really says, with the finding in the test's own comment so no reader
   takes the expectation for the intent.
2. **A manifest directory whose name does not hash to its own bytes is
   announced as held.** The walk takes the name as a name, for the cost
   reason in its doc. `PackStore::manifest_of` refuses such a directory
   by name, and every path that would serve or lay out that pack goes
   through it, so the wrongness surfaces at the fetch and not at the
   announcement. Named in the method's doc rather than left silent.
3. **Holdings are announced per manifest and completeness is not
   re-checked.** `held_packs` asks `holds`, not `holds_whole`: the
   manifest is written last, so its presence means every object was on
   the disk when it was written — and **not** that every object is
   still there. Nothing in this build removes an object, so the two
   agree today. **M12's eviction is where they part**: an eviction that
   removes objects and leaves the manifest makes this runner announce a
   pack it can no longer serve. The eviction must remove the manifest,
   or announce after itself, or this walk must grow a completeness
   check it deliberately does not have.
4. **The end-to-end fixture plants manifests and no objects**, which is
   finding 3 as a test property: it measures the announcement and not
   completeness. A reader who takes those 516 packs for 516 servable
   packs would be wrong.
5. **An announcement is sent on a link that may be gone**, and the
   frames are then dropped by `send_on`. The hub applies nothing
   without a `more: false` frame and forgets a gone runner's holdings,
   so nothing is left half-applied; the line the runner prints says so
   rather than claiming an announcement that did not go.
6. **Three of the orchestrator's mid-task messages were misdelivered to
   this agent.** They ask for ORCH-237 and ORCH-238 to be written, for
   prose-only gates, and — in the third — tell their reader *not* to
   touch ORCH-232's state cell because `runner-announces` owns it,
   which is this branch. They belong to the concurrent `board-two-rows`
   step. Nothing of them was acted on: writing those rows would have
   been this agent editing the one file another agent holds, which is
   the collision the brief named. Reported rather than worked around.

## Drafted for `herd-merger`

The brief asked for no `docs/decisions.md` entry and no new board row.
One entry is **offered** below because two of this branch's choices are
decision-shaped and the merger reads the tree and nothing else; if the
merger judges the batch does not need it, nothing is lost by dropping
it. **The board change is already applied** — ORCH-232's state cell,
`owed` to `done` — and no row text is proposed.

### The offered `docs/decisions.md` entry

```markdown
## D-NEXT — 2026-10-30 — A runner announces what it holds, unprompted,
and an empty announcement is sent

**Context.** D231 decision 5 says a runner's holdings are re-announced
after every `FromHub::Welcome`. Until this branch neither side did it:
the hub had no way to ask (`FromHub::ReadPacksAgain` was written and
withdrawn on a measurement, ORCH-233) and `herd-runner` constructed
`ToHub::PacksHeld` in no place, so every running hub's holdings map was
empty whatever was on a runner's disk.

**Decision.** The runner announces **unprompted**, after its own
`Welcome` and again whenever a job leaves a pack in its store. The hub's
ask is then an optimisation and not the mechanism: the side that owns
the fact volunteers it, and nothing in the hub had to change for the
substance of decision 5 to become true.

**An empty announcement is sent, and is not the same as no
announcement.** A runner holding nothing sends one frame with an empty
`packs` and `more: false`, because that frame is the only retraction the
frame set has; a hub told nothing goes on offering a holder that has
dropped everything it had. The one case that sends nothing is a store
that will not be read: `PackStore::held_packs` answers an **error**
there and never an emptiness, or a `read_dir` failure would retract
every holding a runner really has.

**The cut is at 512 ids per frame, and the figure is not the ceiling.**
The hub refuses a frame over `MAX_FRAME_BYTES`, `1 << 20`
(`herd-hub/src/server.rs:167`); 512 ids of 64 hex characters is about
34 KiB, some thirty times under it. The number is deliberately far
below what would fit, because what a hub can read is a ceiling while
this is what decides whether the `more: true` path is walked by a store
of a few hundred packs or only by one of fifteen thousand — and a path
nothing in the field walks is a path that is not really there. The
fixture is `PACKS_HELD_PER_FRAME + 4`, the same reasoning
`FILES_IN_FLIGHT + 4` already carries.

**What a runner announces is a manifest it holds, not a pack it has
verified.** The walk takes a 64-hex directory name as a name and never
re-hashes it (`PackId::as_announced`'s own rule), and asks `holds` and
not `holds_whole`. Re-hashing would read every manifest on every
announcement — tens of megabytes for a store of a few hundred packs,
each time a job ends. The consequences are named where they bite:
`manifest_of` refuses a directory whose name does not describe it, and
M12's eviction must remove a manifest with its objects or a runner will
announce a pack it can no longer serve.

**Measured against both real binaries**, which the holdings mechanism
had never been: `herd-hub/tests/packs_announced.rs` starts a real hub
and a real runner, plants 516 packs, and reads the hub's own journal for
the accumulating frame and the frame that applies. It also found that
the hub's applied line names the last frame's count and not the
holdings recorded — `holds 4 pack(s)` after recording 516 — which is a
`herd-hub` line and is left as a finding.
```

## Decisions needed

1. **The hub's `holds n pack(s)` line is wrong for any chunked
   announcement** (finding 1). *Recommendation:* fix it where it is
   read, by journalling what the registry holds for that runner after
   the applying frame rather than the frame's own length. *Fallback:*
   leave it and keep the finding, since the registry is right and only
   the sentence misleads — but it is the one sentence an operator has
   about holdings, and `pack_frames.rs` asserts the hub's words too, so
   a later reader will meet it again.
2. **Whether `held_packs` must grow a completeness check before M12's
   eviction lands** (finding 3). *Recommendation:* no — make the
   eviction remove the manifest with the objects and re-announce, which
   keeps the walk cheap and puts the obligation on the code that
   creates the divergence. *Fallback:* `holds_whole` in the walk, which
   costs a read of every manifest and a `stat` of every object on every
   announcement.

## Resume point

ORCH-233, with the ordering the board already carries: brief one step
to make the seven helpers in `herd-hub/tests/link.rs` and
`versions.rs` **skip frames they are not about**, then restore the
`ReadPacksAgain` send and the test removed with it as a recovery path
after the hub's own restart. Nothing of ORCH-232 blocks it, and nothing
of it blocks ORCH-234.

```sh
git fetch origin && git log --oneline origin/runner-announces -3
```

## Processes

PIDs started: none held. Every process this task started was a `nix
develop --command cargo …` child or a test's own hub/runner process,
each ended with its command; the first consumer's hub and runner, in
the process table since 20:23, were not touched and no `pkill` or
`killall` was run.

Own `target/`: the build went to
`/mnt/ssd_980/herd/build/runner-announces` and never onto the root
filesystem (`gotchas/build-weight-off-root.md`); it is removed once the
branch is pushed, and the worktree holds no `target/` of its own.
