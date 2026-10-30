# m12-p1-pack-budget — handoff 2026-10-30

- **Item:** M12 task **P1** of `herd-planner`'s M12 plan (Track P, first
  row). **No board row exists for M12's pack half** — the plan's own
  measurement, restated as `ORCH-NEXT-A` in its §10 and drafted again
  below, so this task closes nothing and claims no id.
- **Agent:** herd-step
- **Branch / commit:** `m12-p1-pack-budget` / `1a29a24` (this handoff's
  commit follows it), based on origin/main `3f3a07c`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`herd-runner/src/packstore.rs`** (+716 / −9)

1. **`PackStore` carries what it occupies, as a running figure.**
   `PackUse { bytes, packs, measured }`, read by `PackStore::usage()`
   with **no filesystem access of any kind**. `bytes` counts every
   `objects/` entry whose name `check_object_digest` accepts, so a
   staging file a kill left behind weighs nought exactly as it is
   announced as nothing; `packs` counts what `held_packs` would
   announce. One definition per figure.
2. **One walk seeds them and nothing walks again.**
   `PackStore::measure()` reads `objects/` once and `manifests/` once;
   each `ingest` adds the bytes it really placed. `measured` is a third
   field because **false is not nought**: a store nothing has read is
   not an empty store, and a hub told `0 bytes` by a runner that never
   looked would believe one (this is what P2a sends `None` for).
3. **The figures belong to the data root, not to the value.** A
   `PackStore` is opened wherever one is needed — the collection opens
   one at the end of a job, `Runner::announce_packs_held` opens one every
   time it says what this runner holds — so a total kept in the value
   would be measured again at each. One `Standing` per data root per
   process, behind a `Mutex`, holding the two figures, the budget and the
   `objects/`-listing counter.
4. **The key is canonical.** `DataRoot::open` canonicalises what it
   validates (`root.rs:140`), so everything inside a running runner opens
   this store under a canonical path, while the startup line has the path
   the operator typed. `keyed()` resolves the nearest existing ancestor
   and joins the rest back on — the same answer `canonicalize` gives once
   the root is made, **creating nothing**, because the startup call runs
   before the data root is validated.
5. **`write_manifest` now answers whether *this* call filed the pack**,
   which is what the pack count moves by. Both of its no-write exits —
   already there, and another ingest won the rename — answer false, so
   one pack cannot be counted twice however many ingests of it run at
   once.
6. **`PACK_BUDGET_FLAG`** (`--pack-budget`) and
   **`packstore::take_budget(data_root, Option<u64>)`**: registers the
   operator's figure against that data root, measures the store
   read-only, and answers the one sentence `main.rs` prints. The typed
   figure reads back through `herd_proto::show_bytes` and the measured
   one through `measured_bytes` — `herd_proto::size`'s own asymmetry.
7. **Nothing evicts anything.** Said in the module doc as well as here:
   the budget is measured against and reported, and the eviction that
   spends it is P3.

**`herd-runner/src/main.rs`** (+58 / −1)

8. `--pack-budget SIZE` / `HERD_PACK_BUDGET`, **no default** — absent
   means nobody said, and the startup line says so. The help explains
   the distinction an operator would otherwise get wrong, quoting
   `--job-disk-budget`'s own default figure.
9. The startup line, printed beside the admission figures, before the
   dial:

```
herd-runner: this runner's pack store holds 0 bytes in 0 pack(s) of the 40 GiB it may hold (--pack-budget)
herd-runner: this runner's pack store holds 0 bytes in 0 pack(s) and has no budget: --pack-budget is what bounds it, and without one this machine's packs are bounded by its disk
```

   Both read off the real binary at `--data-root` under this agent's
   scratchpad, with and without the flag.

10. **D245**: one more pairing in
    `the_long_help_states_every_figure_its_constants_decide`, reading
    `herd_proto::show_bytes(DEFAULT_DISK_BUDGET)` back out of
    `--pack-budget`'s rendered help. The brief's warning was followed to
    the letter: the help states **no claim an assertion cannot hold** —
    no default figure of its own, because it has none.

**Six new tests, all in `packstore.rs`'s own module** (the figures and the
counter are not `pub` to a `tests/` crate's taste, and the store's
fixtures are there):

| test | what it holds |
|---|---|
| `a_measured_store_reports_its_objects_summed_lengths_and_its_pack_count` | the fixture's own `len()`s summed, three contents across two packs, each counted once; `measured` false before anything read it; a staging file weighs nought |
| `a_second_ingest_moves_the_total_by_the_new_blobs_only` | the second pack names two objects, one already held: the total moves by the one placed. A third ingest moves neither figure. The running total is then compared against a fresh walk |
| `saying_what_this_runner_holds_lists_no_objects_directory` | five announcements, performed by `announce_packs_held`'s own three calls, list `objects/` zero times — **by the counter, never by timing** |
| `the_budget_is_taken_once_and_read_from_every_store_of_that_root` | the sentence's two formatters; the budget read back from a second `open`; absent is `None` and not `0` |
| `a_store_that_is_not_there_yet_holds_nothing_and_creates_nothing` | a root that does not exist reads as `0 bytes in 0 pack(s)` and is **not created** (E-45 stays the validator's refusal) |
| `two_spellings_of_one_data_root_are_one_store` | four spellings of one root — real, symlinked, with `.`, with a trailing separator — read one budget and one figure |

**Each new assertion was demonstrated red before it was trusted**, and
the file restored to a byte-identical sha256 each time
(`d65f0dca…` then `73b49c16…`, both verified with `sha256sum -c`):

- a `self.objects_bytes()` added to `held_packs` → the counter test red
  at its *detector* assertion (`left: 2, right: 1`);
- the same call added to `PackStore::open` → red at the *announcement*
  assertion itself (`left: 3, right: 2`), which is the clause the plan
  asks for;
- `20 GiB` → `30 GiB` in `--pack-budget`'s help → the D245 test red,
  naming the flag and printing the rendered help;
- `.entry(keyed(path))` → `.entry(path.to_path_buf())` → the
  path-identity test red (`left: None, right: Some(8589934592)`).

## Not done, and why

1. **Nothing reports the figures to the hub.** `Health`'s
   `pack_store_bytes`, `pack_store_budget_bytes` and `packs_held` are
   still `None`: that is **P2a**, which owns `herd-runner/src/lib.rs`,
   and P1's *Owns* is `packstore.rs` and `main.rs`. Everything P2a needs
   is now a call away — `store.usage()` and `store.budget()` — and
   `PackUse::measured` is exactly the "did not say" / "zero is a
   measurement" distinction its *Done when* asks for.
2. **No eviction.** P3.
3. **No board row written and no `docs/decisions.md` entry appended.**
   M12's pack half has no row to close (the plan measured this), the
   brief asked for no `D-NEXT`, and `decisions.md` is `herd-merger`'s
   alone. Both drafts are below, in full.
4. **`deploy/` was not touched.** The deployed runner carries
   `--job-disk-budget 20GiB` and no pack budget; adding one to
   `deploy/nixos-herd-runner.nix` would be choosing a figure for the
   consumer's machines, which is theirs (D161) and not in *Owns*. It is
   a drafted row below.

## Gates

**Run before this commit, on the code commit `1a29a24`'s tree**, all
four Rust gates under `nix develop --command`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-p1`:

```
=== cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.31s
BUILD EXIT: 0
=== cargo fmt --all --check
FMT EXIT: 0
=== cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.27s
CLIPPY EXIT: 0
```

`clippy --all-targets` is also the answer to the brief's third trap:
**every test crate compiles**, not only the lib.

```
=== nix develop --command ./scripts/test.sh
SUITE EXIT: 0
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
passed 1143 failed 0 ignored 38
skips: 12
```

The 12 skips are **the same list**, name for name, as the run made before
any of this task's code existed (`diff` of the two sorted lists is
empty): all 12 are this host's missing cgroup v2 delegation and its
missing `io.max` device, none is a pack skip, and none is new. D27:
printed as skipped, never counted as passed.

**One red suite run, named rather than buried.** The *second* of three
runs came back `SUITE EXIT: 101` on
`herd-hub/tests/owner_control.rs:872`
`a_dry_run_changes_nothing_and_says_what_the_real_pass_would_say`, with
two extra load-gated skips (`flood-fifty`, `flood-two-fifty`: *"the
host's one-minute load is Some(10.2), over this bench's ceiling of 8"*).
That is **ORCH-244**, which the board already carries with two sightings
from two batch-18 branches; this is the **third sighting, in a third
branch**. Nothing was edited for it. The target alone is green
(`20 passed`) and the whole suite is green on the run quoted above, at
load 3.77. The host, not the diff: it pays to read a suite twice.

**The last run, on the tree this handoff is committed into:**
`./scripts/check-docs.sh`, run bare, output to
this agent's scratchpad as
`p1/check-docs-last.log` (the full path is in the agent's report)
and verbatim in this agent's report. `./scripts/gate.sh` was **not** run:
the batch's gate is `herd-merger`'s.

## Risks and findings

1. **The path-identity trap was live and is now closed, and P2a must not
   reopen it.** `DataRoot::open` canonicalises (`root.rs:140`), and
   `Runner::announce_packs_held` is called with
   `confined.root().path()` (`lib.rs:3812`) — the canonical path — while
   `main.rs` has the path the operator typed. A figure keyed by the
   literal string would have been registered against the spelling nobody
   reads, and P2a would have shipped a health report saying "no budget,
   0 bytes" for a runner holding gigabytes, on a machine nobody here may
   log into. `keyed()` and
   `two_spellings_of_one_data_root_are_one_store` are the fix and its
   proof. **Found by reading, not by a gate**: every gate was green
   before it.
2. **The running figure is arithmetic, and two races can make it drift
   by a rounding.** Two concurrent ingests placing the *same* new object
   can both report its bytes (`holds_object` guards, a window remains);
   the pack count cannot drift, because `write_manifest` answers which
   call filed it. `measure()` is the absolute answer and may be called
   again at any time. Nothing in the runner ingests two packs at once
   today — one collection per attempt — so this is a property of the
   design and not an observed defect.
3. **`PackStore::open` is called on every announcement and
   `create_dir_all`s two directories each time.** Pre-existing
   (`lib.rs:1331`), untouched, and now slightly more visible because the
   figures survive it. Worth a line in P2a or P3, not a change here.
4. **`--pack-budget` has no default, deliberately, and that is a choice
   a reader could call a gap.** It is P2a's *Done when* that decides it:
   *"a runner with no `--pack-budget` sends `None` for the budget"*. A
   default would make that clause unmeetable, and a byte figure chosen
   here would be right on one machine and absurd on every other —
   `--job-io-max`'s and `--host-memory`'s own reasoning. The consequence
   is real and is named in finding 4 of *Not done*: **until an operator
   sets the flag, the deployed runner is still unbounded on packs.**
5. **D245's gap is wider than `--pack-budget`.** The brief's warning —
   that the help gate *pairs figures, not claims* — holds: nothing in
   the repository can catch `--job-output-cap`'s old *"which is also the
   most this can be"*, because no constant disagrees with a false claim
   about what a flag does. This task avoided writing such a claim; it did
   not close the hole. Not drafted as a row: it is a method finding for
   whoever reads D245 next.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: the brief asked for none, and nothing here
is a decision rather than the plan's own instruction carried out.

Two `board.md` rows, **both for the *Owed* table**, whose header is
`ID | Item | Origin | Owner | State | Next action | Depends on | Evidence`.
Ids unallocated (`ORCH-NEXT-P2A`, `ORCH-NEXT-PACKDEPLOY`) because the
merger allocates and gaps stay (D148, invariant 8). The first supersedes
nothing: `ORCH-NEXT-A` of the plan's §10, the row for M12's whole pack
half, is still unwritten and still wanted.

```
| ORCH-NEXT-P2A | **The runner now measures its pack store and no one is told.** M12 P1 landed `PackStore::usage()` — bytes, pack count and a `measured` flag — and `PackStore::budget()`, both with no filesystem access, on branch `m12-p1-pack-budget`. `Health`'s `pack_store_bytes`, `pack_store_budget_bytes` and `packs_held` are still `None` at the build site, and the hub discards all three at `server.rs:4692-4694`, so the whole pack population remains unreported: zero occurrences of pack in `herd-hub/src/report.rs` and `herd-hub/src/status.rs`, which is §0.7's 65 GB precedent one directory over | M12 P1's handoff *Not done* 1, 2026-10-30; herd-planner's M12 plan Track P rows P2a and P2b | herd-step | owed | P2a then P2b of the plan, in that order and in two worktrees: fill the triple on the health report from `usage()` and `budget()`, where **absent still means did not say and zero is a measurement** — `PackUse::measured` is exactly that distinction and is already there; then stop the hub discarding it, into `status.json` and `herd-hub status`. **P2a must open the store under the same path `DataRoot::open` canonicalised**, or the figure it reads is a second one: see P1's handoff finding 1 | M12 P1 | `herd-runner/src/packstore.rs` `PackUse`, `usage`, `budget`, `take_budget`; `herd-runner/src/lib.rs:3002-3004`; `herd-proto/src/lib.rs:1692-1711`; `herd-hub/src/server.rs:4692-4694`; `history/2026-10-30-m12-p1-pack-budget.md` |
```

```
| ORCH-NEXT-PACKDEPLOY | **The deployed runner has a pack budget available to it and still carries none.** `--pack-budget` exists as of M12 P1 with no default, by P2a's own *Done when* — absent means nobody said, never nought — so a machine whose unit does not name it keeps packs until its disk is full. `deploy/nixos-herd-runner.nix` writes seven figures on its ExecStart line, `--job-disk-budget 20GiB` among them, and no pack budget of any kind | M12 P1's handoff *Not done* 4 and *Risks* 4, 2026-10-30 | the orchestrator | open | **A figure for the consumer's machines is theirs and not ours (D161, invariant 9), so this is a question and not a task:** offer the module an option with no default, so an operator sets it in one place, and report that without one the store is bounded by the disk. Do **not** choose a byte figure here — `--host-memory`'s and `--job-io-max`'s rule — and do not make it required, which would refuse every existing unit on upgrade | M12 P1; ORCH-215 for the removal half only | `deploy/nixos-herd-runner.nix`; `deploy/herd-runner.service`; `herd-runner/src/main.rs` `--pack-budget`; `herd-runner/src/packstore.rs` `PACK_BUDGET_FLAG`; `history/2026-10-30-m12-p1-pack-budget.md` |
```

## Decisions needed

**None blocking.** One worth the orchestrator's eye, and it is finding 4
above, not a question for Alexa: `--pack-budget` ships with no default,
so **nothing is bounded until somebody sets it**. The recommendation is
`ORCH-NEXT-PACKDEPLOY`'s own — offer the NixOS module an option with no
default and report the consequence; the fallback, if that is judged too
slow, is for P3 to refuse to start a runner whose store is already past a
budget it was given, which bounds nothing extra but makes the figure's
absence visible on the first start rather than on the first full disk.

## Resume point

```sh
# P2a, in its own worktree off the batch that lands this one:
git worktree add -b m12-p2a-pack-health <path> origin/main
# it owns herd-runner/src/lib.rs's health build site at :3002-3004 and
# nothing of packstore.rs. Its three figures are:
#   store.usage().bytes / .packs  (PackUse::measured says "did not say")
#   store.budget()                (None means nobody said, never 0)
# opened on the SAME path DataRoot::open canonicalised — handoff finding 1
```

L2 and R0, the plan's other two openers, are untouched by this branch
and need nothing from it.

## Processes

- **Started and ended by this agent:** `cargo`/`rustc` under
  `nix develop` for build, fmt, clippy and three suite runs, each in the
  foreground and each exited before the next; two `herd-runner` binaries
  under `timeout 6` against `ws://127.0.0.1:1` with a data root in this
  agent's own scratchpad, to read the startup line off the real binary.
  No PID signalled, nothing killed, no `pkill`.
- **Not touched:** the consumer's hub `3006543` and runner `3021002`,
  and this project's own instance (hub `565152`, runner `607643` on
  `127.0.0.1:8791`).
- `./scripts/bench-preflight.sh` run before each heavy phase: four
  clearances, `ok, safe to start` each time, at loads 0.46, 1.87, 3.77
  and 3.77. No clause lifted, `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` never
  set.
- **Build tree:** `/mnt/ssd_980/herd/build/m12-p1`, deleted by literal
  path once the branch was pushed; never on the root filesystem. The
  worktree's own `target/` was never created.
