# pack-assignment — handoff 2026-10-30

- **Item:** ORCH-234, ORCH-239, and half a sentence the brief called ORCH-243
  (no such row exists on the board; the highest filed is ORCH-240)
- **Agent:** herd-step
- **Branch / commit:** `pack-assignment` / 006ba46 plus this handoff's own
  commit, based on origin/main 26e83cf
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**ORCH-234, the frame half, and the premise was short by one step.**
`Asked.needs` and `Asked.leaves` did ride the wire — and were **dropped at
`new_job`**. They reached no `NewJob`, no `JobRecord` and no `Declaration`,
so there was nothing at the assignment site to resolve: the row's "what is
missing is the call at the assignment site" was one step short, and the
missing step came first.

1. `NewJob` and `JobRecord` gain `needs: Vec<String>` and
   `leaves: BTreeMap<String, String>`, `serde(default)` and additive at
   record version 2 on exactly the argument `JobRecord::packs_in`'s doc
   makes. `new_job` carries them off `Asked` through a new
   `NewJob::consuming`. They are on the **record** and not consumed at the
   submission because the resolve is per *attempt*: a requeue onto another
   machine (E-21) resolves again, as it re-derives the `-j`.
2. `packs_for` in `herd-hub/src/server.rs` resolves each `needs` key
   against `PackRegistry::resolve` for the declaration's commit and fills
   `FromHub::AssignJob`'s `packs_in`; `packs_out` is `record.leaves`,
   carried over unresolved, because a `leaves` entry is the requester's own
   key and path and the hub has nothing to look up.
3. **It runs before `begin_attempt`.** A job that waits for a pack must not
   have spent one of its goes on a machine that never saw it, and no record
   may name a runner that was handed nothing.
4. **An unresolvable key is a wait, named, and never a `Refused`** (D13).
   Four cases, each with its own sentence: nothing registered for that
   `(commit, key)`; the registry's own refusal (two ids for one key); a
   store that keeps no registry at all; and the registry failing to read its
   own disk. A job with non-empty `needs` and no declaration also waits,
   because only a declared command names a commit.
5. That needed `dispatch` to stop answering a `bool`. Its caller had one
   sentence for two different facts and used the link's — "runner X's
   connection ended while this pass was placing it there" would have been
   printed for a pack that does not resolve. `Dispatched` has three
   outcomes: `Sent`, `LinkWent`, `Waits(because)`.
6. **The ordinary case consults nothing.** `needs` empty returns before the
   store is asked for a registry, so a hub with no registry dispatches
   exactly as it did.

**The runner stops discarding the fields** (`herd-runner/src/lib.rs`,
`herd-runner/src/job.rs`). They are carried into `job::execute` as a
`job::Packs`, and a job this build cannot serve is **handed back with every
key named** rather than run: `{in}/<key>` would be an empty directory and
`{out}` would be swept with nothing registered, so the job would exit 0 and
look green while the pack a later job waits for never came to exist. That is
D150's silence and the case `herd_proto::pack::PackUnavailable`'s own doc was
written for. The same from the other end for `packs_out`.

**ORCH-239, as ruled.** `PackRegistry::announced` answers how many packs
that runner now stands at — the applied set once `more` is false, what has
arrived so far while it is true — **read under the lock that just wrote
it**, and `relay_from_runner` prints that instead of
`announcement.packs.len()`. Both journal branches use it, because "so far"
was the same defect waiting for a different frame order: the measured 516
read as `holds 4 pack(s)` only because 4 was the last frame's length, and
`512 so far` was right only because the first frame was full.

**ORCH-243.** The registry module doc said the runner "neither answers that
frame nor constructs `ToHub::PacksHeld` of its own accord". Half of that is
now false (batch 16) and the half about `FromHub::ReadPacksAgain` stands, so
half a sentence moved. **Three more sentences in that module doc were made
false by this branch and are corrected with it**: the paragraph that said the
assignment side is not wired, the heading that said the asking is owed on
both sides, and the clause that said the holdings mechanism was measured
"against no real runner" — `packs_announced.rs` measures it against both
binaries as processes.

**Tests.** Five, all new, all green:

- `herd-hub/tests/pack_frames.rs`, through a socket:
  `a_job_whose_needs_resolve_is_assigned_the_manifest_the_holder_and_
  its_leaves`
  (the manifest **verbatim**, `pack.meta`, the holder *and* its endpoint,
  and `packs_out` equal to the `leaves`) and
  `a_job_needing_a_pack_nothing_registered_waits_by_name_and_is_not_assigned`
  (the hub's sentence read where a requester reads it, no `AssignJob`, no
  `Refused`, and the keys on the record). Both drive a hand-written
  `Requester` peer, because `herdctl` has no flag for `needs` and adding one
  is another row's.
- `herd-hub/src/registry.rs`:
  `a_chunked_announcement_is_counted_as_the_set_it_builds_and_not_as_
  its_last_frame`,
  **three** frames of 3, 2 and 1, so that no single frame's length is the
  right answer.
- `herd-runner/src/job.rs`: three unit tests on `Packs::why_this_runner_cannot`
  — no pack is not a refusal, a pack to lay out is named by its key and by
  `{in}`, a pack to collect likewise by `{out}`.
- `herd-hub/tests/packs_announced.rs` asserts `holds 516 pack(s)` where it
  asserted 4, **and the comment that described the defect is gone**, replaced
  by what is now asserted and why the fixture is past the frame window.

## Not done, and why

1. **The resolved ids are written to no `JobRecord::packs_in`.** That field's
   doc now carries the measurement instead of claiming otherwise: putting
   them there means a write at the moment `begin_attempt` numbers the
   attempt — the one write that moment allows — which is a `JobStore`
   signature and five implementors. So a reader of `job.json` cannot yet tell
   which pack an attempt was handed, and neither can `herd-hub report`.
   Deliberately out of this branch, on the brief's scope discipline.
2. **The `{in}`/`{out}` layout.** Writing a pack's objects under
   `{in}/<key>` and collecting `{out}` back into a manifest to register is
   not built, and `ToHub::RegisterPack` is still constructed nowhere under
   `herd-runner/src`. This is the larger piece the brief said to bound: the
   boundary is `Packs::why_this_runner_cannot`, one function, and the next
   step replaces it.
3. **No requester surface for `needs`/`leaves`.** `herdctl::Declared` has
   no field for either, so only a peer that encodes its own frame can submit
   one. Not widened here; it is the requester half and its own row.
4. **ORCH-233 untouched** (the hub's ask), and nothing of M12.

## Gates

All four Rust gates plus `check-docs.sh`, run on this tree before this file
was committed. `./scripts/gate.sh` was **not** run: it is the batch's at
integration, `herd-merger`'s and not this agent's.

```
nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.54s
BUILD_STATUS=0

nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.72s
CLIPPY_STATUS=0

nix develop --command cargo fmt --all --check
FMT_STATUS=0

nix develop --command ./scripts/test.sh
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
TEST_SH_STATUS=0
  (1133 passed, 0 failed, 38 ignored, summed over every target)

./scripts/check-docs.sh
docs ok
CHECKDOCS_STATUS=0
```

**The suite's first run was RED, and it is reported red.** One failure,
`owner_control::a_dry_run_changes_nothing_and_says_what_the_real_pass_
would_say`
at `herd-hub/tests/owner_control.rs:872`: two status snapshots taken either
side of a dry run, where the same job was `assigned` in the first and
`running` in the second. The job's own `JobStarted` arrived between the two
reads. **Nothing was edited for it.** It then passed 3 times out of 3 alone
and the whole suite passed on its second run, under a load that had dropped
from the first. It is a pre-existing race in that test's fixture and not this
branch's: nothing here changes dispatch ordering for a job with empty
`needs`, whose resolve returns before any lock or any I/O. A finding below.

**Skips.** 12 runtime `HERD-SKIPPED` and 38 compile-time `HERD-IGNORED`, the
figures the brief named. The two lists are **identical** between the two
runs, compared by name and not by count (D187) — `diff` of the sorted names
is empty. All 12 runtime skips are the cgroup-v2-delegation and io.max family
this host cannot carry, plus
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`. No
skip moved and no `#[ignore]` moved.

**The last run** is `./scripts/check-docs.sh` on the committed tree, after
this file; its output is in this agent's report and not in the tree.

## Risks and findings

1. **`owner_control::a_dry_run_changes_nothing…` is flaky under load.** It
   compares two `Status` snapshots and asserts `running` is unchanged, while
   the fixture's job is a live job walking `assigned` → `running`. Red once in
   a full suite under load, green 3/3 alone and green in a second full suite.
   Not touched, not reddened by this branch, and not recorded anywhere in
   `docs/` yet — a row's worth, and a reason to read a suite twice before
   blaming a diff.
2. **One seat is spent per pass on a job whose pack never resolves.**
   `matcher::place` claims the seat before `dispatch` is called, so a job
   that then waits for a pack leaves that runner idle for the rest of the
   pass. It is one seat and one pass — the next pass re-judges — but a
   permanently unresolvable job at the head of the queue wastes one seat on
   every pass. Giving the seat back needs a `matcher` API this branch did not
   open. The same was already true of the `LinkWent` path.
3. **A job with `needs` and no declaration waits rather than being refused.**
   A pack is registered for a `(commit, key)` pair and only a declared
   command names a commit, so such a submission can never be placed. It
   would be better refused at the submission, where a requester is standing
   to be told (E-36). Left as a wait here because refusing at the submission
   is a new refusal on the requester's surface.
4. **Handing a job back charges it a go.** `Session::abandon`, not
   `abandon_at_its_ceiling`: a runner that cannot lay a pack out is not a
   machine at its ceiling, so the ordinary count applies and a job needing a
   pack will fail by name after its goes. That is visible and bounded, and it
   is a behaviour choice, reversible in one edit: the alternative is a job
   that waits for ever on a fleet where no runner can serve it.
5. **`ORCH-243` is not a row.** The brief named it; the board's highest id is
   ORCH-240 and `grep -c ORCH-243 docs/orchestration/board.md` is 0. The work
   is done in the tree; the row is drafted below rather than invented on the
   board, because an agent allocates no id.
6. **`ugrep` is not on this PATH.** `MEMORY.md` says "`grep` here is ugrep";
   in this shell `ugrep` is `command not found` and `grep` is GNU grep. Every
   search here used `grep`.
7. **The planted fixtures hold no objects.** `packs_announced.rs`'s packs are
   manifests with no blobs, as its own doc already says, and
   `pack_frames.rs`'s manifest names objects nobody holds. Nothing here
   measures completeness, and nothing here claims to.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: this branch decides nothing the board had not
already ruled. ORCH-234's shape was on its row and ORCH-239 was ruled by the
orchestrator on 2026-10-30.

ORCH-234 and ORCH-239 are set to `done` **on the branch**, with their outcome
in the *Next step* cell; nothing else on the board was touched. One row is
proposed, for work this branch has **already done**, so that the half-sentence
is not closed invisibly:

```
| ORCH-NEXT-A | **The pack registry's module doc said the runner constructs `ToHub::PacksHeld` in no place, and batch 16 made that false.** Its other half held — the hub constructs `FromHub::ReadPacksAgain` nowhere outside tests — so it was half a sentence and not a paragraph. The brief that found it called it ORCH-243, which is not a filed id: the board's highest is ORCH-240 | brief for `pack-assignment`, 2026-10-30; measured against `herd-runner/src/lib.rs` and the board | herd-step | done | **Already done on `pack-assignment`, 2026-10-30**, with three further sentences of the same module doc that this branch itself made false: the paragraph saying the assignment side is not wired, the heading saying the asking is owed on both sides, and the clause saying the holdings mechanism was measured against no real runner. Filed as a row so the correction is visible rather than arriving as a side effect of another row | ORCH-232, landed in batch 16 | `herd-hub/src/registry.rs` module doc; `history/2026-10-30-pack-assignment.md` |
```

Two rows are **not** drafted, because what to do about them is a judgement
above this agent's, and both are stated as findings above: the record half of
ORCH-234 (finding in *Not done* 1) and the flaky `owner_control` test
(*Risks* 1).

## Decisions needed

1. **Does a job needing a pack no runner can lay out keep spending its goes?**
   Recommendation: yes, as built — it fails by name and visibly, which is
   better than waiting for ever. Fallback: `abandon_at_its_ceiling`-shaped
   handback that spends no go, which turns it into an unbounded wait.
   Reversible in one edit either way.
2. **Who takes the record half of ORCH-234** — the `JobStore` signature plus
   five implementors so that `job.json` says which pack an attempt consumed?
   Recommendation: a row of its own before the layout step, because the
   layout will want the id on the record anyway. Fallback: fold it into the
   layout row.

## Resume point

The layout step, which is the whole of what is left between here and a pack
that actually reaches a job:

```
# in herd-runner: replace Packs::why_this_runner_cannot with the real thing
#   - lay each AssignedPack out under {in}/<key> from the pack store,
#     fetching what it lacks from a holder (D10's three paths)
#   - collect each packs_out path out of {out}, write a manifest, fsync,
#     and send ToHub::RegisterPack — constructed nowhere today
nix develop --command cargo test -p herd-runner
nix develop --command cargo test -p herd-hub --test pack_frames
```

## Processes

- Started and ended by this task: `cargo`/`rustc` under
  `nix develop --command` only, each in the foreground and each already
  exited. No background process, no `pkill`, nothing killed by name.
- The first consumer's hub (3006543) and runner (3021002) were **not
  touched**: they are load this task waited out, and
  `./scripts/bench-preflight.sh`
  was run and green before each heavy phase.
- Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-assignment`,
  never on the root filesystem. Deleted by literal path once the branch was
  pushed; the agent's report states the `rm -rf` and the disk free after it.
