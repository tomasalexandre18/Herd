# m12-p4-runner-custody — handoff 2026-10-30

- **Item:** M12 Track P step **P4**, the runner honours custody; and
  **ORCH-264**, the pack gc nothing called
- **Agent:** herd-step
- **Branch / commit:** `m12-p4-runner-custody`; the code is `7e19eed`
  plus `cdf751a` (the two gate fixes), this handoff's own commit being
  the tip; based on origin/main `73c2dc8`
  (batch 22 landed `8af300e` while this ran; the brief said to stay on
  `73c2dc8` and let the merger rebase)
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**1. `Welcome.keeper_for` is read, and kept across a redial.** The set
lives on the session as `Inner::pins`
(`herd-runner/src/lib.rs`), not on a connection, and it is taken in the
`FromHub::Welcome` arm of `serve_once` — **before the link is attached
and before anything sweeps this store**, which is the window
`herd-proto` names: *"a runner that learned it from a later frame would
have a window in which it could evict the very pack it is keeping"*.
`Session::keeps_for` is what takes it, and it answers the one line a
keeper says on its own machine's stderr.

**The welcome is authoritative, so an empty `keeper_for` clears what a
previous connection pinned.** That is a decision and it is drafted below
as `D-NEXT` (1). The count the welcome's keys are kept at is this
runner's own `packstore::KEEP_PER_KEY`, because the welcome carries keys
and no figure.

**2. `FromHub::PinPacks` is applied, and says so once.**
`Session::pin_packs` **replaces** the set rather than adding to it, for
the reason `ToHub::PacksHeld` replaces rather than appends: a frame that
only ever added could never retract. One line per frame, naming what is
kept now, how many of each, and what was kept before; a frame that
changes nothing says that it changed nothing; a frame of no keys is
custody withdrawn and never a custody of nothing. The arm was falling
to `serve_hub`'s catch-all, which printed *"ignoring pin_packs, which is
not a runner's business"*.

**3. No protocol change.** `herd-proto` is untouched — `git diff
--stat` names three files and none of them is in that crate. Both frames
were already at `PROTOCOL_VERSION` 3.

**4. ORCH-264's headline is closed: the pack budget is now spent.**
`PackStore::evict_to_budget` has a caller,
`Session::sweep_packs_to_budget`, run **once per connection, before the
announcement of what this runner holds**. Both halves of that sentence
are load-bearing and both are argued at the call site:

- *Why the session and not an ingest.* The pass may not run without the
  custody set or it evicts the very packs a keeper exists to hold, and
  `lib.rs` is the only place in the crate that has that set.
  `packstore::leave_packs` and `fetch.rs` both ingest with no session in
  hand, so a pass called from either would run with `pins: None`, which
  is worse than no pass at all. Threading the pins to those two edges
  needs `herd-runner/src/outputs.rs` and `herd-runner/src/job.rs`, which
  P4 does not own. What this costs is in *Not done* 1.
- *Why before the announcement.* An announcement that went first would
  leave the hub holding this runner as a holder of packs the pass is
  about to take, and the frame set has no retraction but a later whole
  announcement. Swept first, the announcement is the truth about the
  disk as it is now — ORCH-240 read from the announcer's side.
- *Off the reactor.* `tokio::task::spawn_blocking`, like `job.rs`'s
  `workspace.measure()`: the pass walks every manifest, unlinks objects
  and takes `Shared::work`, which a fetch belonging to a job of the
  **previous** connection may be holding, because a job does not end
  when a link does.
- *It runs even with no budget*, because finishing an eviction a kill
  stopped is not spending a budget — P3's own order — and it answers
  **no lines at all** in that case, so no runner in the fleet gains
  stderr noise its startup line had not already said.

**5. The done-when, asserted against the real binary as processes.**
`herd-hub/tests/pack_custody.rs`, three tests, the shipped
`herd-runner` started as a child process each time against a
hand-written hub (the division `pack_store_health.rs` already makes, and
for the same reason: the real hub sends `keeper_for: Vec::new()` and no
`PinPacks` at all, so it cannot make a runner a keeper until P5). Two
identical disks, one `--pack-budget 1500`, **one field of one frame**
between the runs:

- `an_unpinned_runner_loses_packs_to_come_under_its_budget` — announces
  **1** pack of 3 and says *"2 pack(s) and 2 object(s) taken away"*;
- `a_keeper_keeps_every_pack_of_a_pinned_key_under_the_same_budget` —
  announces **3**, says it is a keeper for `"pack"`, and says it is
  *still over its budget* because what is left is *"pinned here as a
  keeper's"*;
- `a_pin_packs_frame_after_the_welcome_changes_the_set_and_says_so_once`
  — exactly **one** line matching `the hub pins`, naming the new keys,
  the new count and the old keys.

What each run announces is also compared against what is on its disk,
so the test cannot pass on a frame that lies.

**Why the packs are all one size (1 000 bytes, 3 000 against 1 500).**
The eviction order is by age and a file's mtime on Linux comes from a
coarse clock whose granularity is a scheduler tick: three packs planted
in one loop can be filed in the same instant and the id then breaks the
tie. Three sizes would have made the surviving **count** depend on which
pack went first — a flake, not a measurement. Three equal packs make it
the budget's answer: any two cover the overrun and no one of them does.
*Which* pack goes is `packstore`'s own unit measurement, where a fixture
can plant the mtimes it needs.

**6. Four unit tests at the seam this task built**, in `lib.rs`'s own
test module: the keeper/unpinned pair at library level (which asserts
*which* of the session's fields the pass reads), custody taken from
every welcome with an empty one clearing it, the `PinPacks` sentences,
and a store nothing bounds being swept and saying nothing.

**7. Three doc statements in `packstore.rs` that are no longer true are
corrected** — the module doc's *"Nothing in this crate calls it yet"*,
`Claims`'s *"has no caller in this crate yet"*, and `Claims::pins`'s
*"reading the frame … is P4's"*. Each now names its caller. One
statement was **added** rather than corrected, on
`Claims::referenced`: what the empty set costs today is bounded by the
filesystem, because a pack already laid out for a job is hard links into
`objects/`, so unlinking the store's own name leaves the job's copy
whole.

## Not done, and why

1. **The gc fires on one edge only: a connection.** A runner that stays
   connected for a week and ingests all week does not sweep until it
   redials. ORCH-264 asked for the collection and attempt seams; those
   need the pins threaded through `outputs.rs` and `job.rs`, which this
   task does not own, and a periodic pass is a timer nothing in this
   file has. Drafted as a successor row below.
2. **`Claims::referenced` is still empty in the field**, so P3's
   reference clause is still proven against a claim set a test wrote and
   not against a real job. That is ORCH-264's second half and it waits
   on M11's `{in}`/`{out}` layout step (ORCH-220), unchanged by this
   task.
3. **Nothing on the hub side.** `server.rs:3230` still sends
   `keeper_for: Vec::new()` and no `PinPacks`: choosing a keeper is P5
   and the brief's *Owns* excludes it. So **in production nothing is
   pinned yet** and this step's effect in the field today is the budget
   being spent at all.
4. **`PinPacks` does not trigger a pass.** Pinning only ever protects
   more, and an unpinning leaves the store holding what it was already
   holding a moment ago until the next pass. Neither is worth an
   eviction inside the frame loop, where `Shared::work` may be held by a
   fetch's ingest.
5. **No protocol change, no `herd-proto` edit, no hub edit, no board or
   `decisions.md` edit.** As briefed.

## Gates

Four separate Herd jobs, not one, on the coordinator's instruction after
batch 22's single-job `gate.sh` hit the runner's 20 GiB disk cap
mid-suite with no verdict. Every submission went through
`/mnt/ssd_980/herd/queue/bin/herdctl` with `--label build --memory-mib
6000 --verdict-to <file>`, after one `./scripts/bench-preflight.sh`
(**ok**: 342 GiB free, load 6.23 of a 9.00 ceiling, 20 GiB available,
14 480 MiB of slice room). `CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/m12-p4-runner-custody` in all four, so the
cold build's bytes were paid once and the next three ran warm.

The jobs build the **working tree**, which is `7e19eed` plus this
untracked handoff while the last of them ran; nothing under `docs/`
enters a Rust compilation, so the Rust evidence is `7e19eed`'s.

| job | command | id | exit | wall | bytes_written |
|---|---|---|---|---|---|
| 1 | `cargo build --workspace --all-targets` | `j0000000044` | **0** | 121 s | 89 502 164 |
| 2 | `cargo fmt --all --check` | `j0000000047` | **1 — RED** | 2 s | 3 402 568 |
| 2b | `cargo fmt --all` (the fix, a write and not a check) | `j0000000049` | 0 | 4 s | 3 402 568 |
| 2c | `cargo fmt --all --check` again | `j0000000052` | **0** | 2 s | 3 344 888 |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | `j0000000050` | **101 — RED** | 37 s | 89 764 212 |
| 3b | the same, after the fix | `j0000000051` | **0** | 25 s | 89 415 137 |
| 4 | `./scripts/test.sh` | `j0000000053` | **0** | 318 s | 346 331 592 |

Each line is `nix develop --command <command>` inside the job, one level
and never nested in a second `nix develop`.

**Two gates were red and both are reported red.** Neither was a failing
assertion and neither was worked around:

- **job 2, `cargo fmt --all --check`, exit 1** — rustfmt wanted a
  different shape for four `assert!`/`assert_eq!` calls in the new tests.
  Fixed by `cargo fmt --all` (job 2b) and re-checked green (job 2c).
- **job 3, `cargo clippy`, exit 101** — one error,
  `clippy::unnecessary_lazy_evaluations` at `lib.rs:1472`:
  `(!keeper_for.is_empty()).then(|| …)` with no work in the closure
  becomes `then_some`. Fixed as clippy's own `help` says and re-run green
  (job 3b).

Both fixes are commit `cdf751a`, which carries **nothing but those two**;
the clippy fix came before the final fmt check, so job 2c and job 4 both
ran on the tree that commit left.

The suite, verbatim where it matters:

```
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
```

1 201 passed, 0 failed, 38 ignored, summed over every test target. The
six assertions this task adds all ran:
`an_unpinned_runner_loses_packs_to_come_under_its_budget`,
`a_keeper_keeps_every_pack_of_a_pinned_key_under_the_same_budget`,
`a_pin_packs_frame_after_the_welcome_changes_the_set_and_says_so_once`
and the four in `lib.rs`'s test module — `ok` every one.

The **last run** is `./scripts/check-docs.sh`, on the tree with this
file committed, from the worktree root; its output goes to the agent's
report. An earlier `check-docs.sh` on the same tree without this file
printed `docs ok`.

**Skips, as a list and never a count (D187).** The run's twelve
`HERD-SKIPPED` names were extracted and `diff`ed against
`scripts/expected-skips/local.txt`'s twelve: **identical, no line either
way**. They are the standing cgroup-delegation twelve, every one of them
saying *"this runner cannot create a cgroup under
/sys/fs/cgroup/chatons.slice/…​ (Permission denied)"*, and under a Herd
job the reason is the queue runner's scope rather than this shell's. The
`#[ignore]` census printed `COMPLETE (38 ignored, 0 without a reason)`,
and an anchored `grep -c '^[[:space:]]*#\[ignore'` over the tree returns
**38** as well — the same figure counted twice, from the run and from the
source, which is the comparison D187 asks for.

**No skip belongs to this task**: nothing here is `#[ignore]`d and
nothing here prints `HERD-SKIPPED`. A skip is copied as a skip and never
counted as a pass (D27).

Two further checks ran locally because they need no toolchain and are
not a heavy phase: `./scripts/check-orchestration.sh` — **324 passed, 0
failed** — and the `check-docs.sh` named below.

## Risks and findings

1. **A premise of the brief is refuted by the code, and the honest
   version is narrower.** The brief says *"`Session::pack_store` is the
   only caller of `PackStore::open` in the crate and must stay the
   only one"*. At `73c2dc8` there are **four** callers:
   `Session::pack_store` (`lib.rs`), `packstore::leave_packs`
   (`packstore.rs`), `fetch::forget_incoming`'s site in `serve` and the
   mesh endpoint's site, the last two also in `lib.rs`. The doc comment
   on `Session::pack_store` makes the same over-claim in the tree
   (*"The only place in this crate that calls
   `packstore::PackStore::open`"*). What the property really protects is
   narrower and I kept it: **the health report reads the store
   `Session::pack_store` remembered**, so there is no second spelling of
   the data root in the report's path, and my sweep goes through that
   same method rather than opening a store of its own. I did **not**
   correct the doc comment: the Rust gates had already been submitted on
   `7e19eed`, and an edit to `lib.rs` afterwards would have left them
   covering a tree that no longer existed. Drafted as a row below.
2. **The sweep can take a pack a job is about to be handed.**
   `Claims::referenced` is empty, so a pass that runs while a job is
   being set up can unlink an object between a completeness check and
   `lay_out`. The outcome is a **re-fetch, said by name**, never a job
   handed half a pack: a pack already laid out is hard links, and the
   store's own lock serialises an eviction against an ingest. It is
   stated on `Claims::referenced` in the tree and it is ORCH-264's
   second half.
3. **A `--pack-budget` smaller than one pack destroys packs to no
   purpose, one connection at a time.** Nothing in P1..P4 refuses a
   budget below what a single pack of this fleet weighs: the pass will
   take every unpinned pack, announce an empty holding, and the next
   fetch will bring one back. That is the budget working as specified
   and it is also a foot-gun with no refusal in front of it. Drafted as
   a row below.
4. **New file in `herd-hub/tests/`.** `pack_custody.rs` is additive and
   collides with nothing on `main` at `73c2dc8` or `8af300e`, but a
   batch 22+ branch adding a file of that name would conflict. It lives
   beside the hub's tests because
   `no_module_of_the_runner_can_bind_a_socket`
   (`herd-runner/tests/confinement.rs`) forbids `TcpListener` anywhere
   under the runner crate — invariant 3.
5. **`herd-proto`'s `PinPacks` doc still says *"unimplemented until
   M12"***. True as written and now stale in spirit; not edited,
   because `herd-proto` is outside *Owns* and the sentence is not wrong.

## Drafted for `herd-merger`

The full `docs/decisions.md` entry, headed `D-NEXT` for the merger to
number, and the full text of every `board.md` row proposed. Nothing here
is a pointer and nothing is a summary.

### The decision entry

```markdown
## D-NEXT — 2026-10-30 — The runner spends its pack budget once per
connection, and the welcome is the whole truth about custody

M12 P4 wired `PackStore::evict_to_budget`, which landed in batch 21
with no caller (ORCH-264), and had to answer two questions the
milestone does not: **where** the gc fires and **what an empty
`keeper_for` means**. Both are decided here, both are reversible, and
both are measured from the code rather than argued.

**1. The gc fires once per connection, in `serve_once`, before the
announcement of what this runner holds** —
`Session::sweep_packs_to_budget`, on `spawn_blocking`.

Not beside an ingest, which is where a gc would ordinarily live. The
pass may not run without the hub's custody set, or it evicts the very
packs a keeper exists to hold; the custody set lives on the session;
and `lib.rs` is the only place in the crate that has it.
`packstore::leave_packs` and `herd-runner/src/fetch.rs` both ingest
with no session in hand, so a pass called from either would run with
`pins: None` — worse than no pass at all. Threading the pins to those
two edges needs `herd-runner/src/outputs.rs` and
`herd-runner/src/job.rs`, which P4 does not own.

**Before** the announcement and not after it, because the frame set
has no retraction but a later whole announcement: a runner that
announced first would leave the hub offering a holder of packs the
pass is about to take, which is the outcome the eviction's own order
exists to avoid (ORCH-240), read from the announcer's side.

It runs even for a store nobody bounded, because finishing an eviction
a kill stopped is not spending a budget, and it then answers **no
lines at all**: a pass that printed on every connection of every
runner with no `--pack-budget` would be new noise on every machine in
the fleet, saying what the startup line already said.

What this costs is said plainly: a runner that stays connected for a
week and ingests all week does not sweep until it redials. That
residue is a board row of its own and not a defect in this choice.

**2. `Welcome.keeper_for` is authoritative at every dial, so an empty
list clears what a previous connection pinned.**

A hub fills `keeper_for` from its own policy at every dial and sends
`PinPacks` when that policy changes while the link is up, so the
welcome is the whole truth about custody at the moment a connection
begins. A runner that kept yesterday's custody after a hub that no
longer names it would pin packs nobody asked it to keep, for ever,
with nothing able to retract them — a hub that forgot would never send
the frame that could. `herd-proto` says the same thing from the other
side: *"a runner reading an empty list is a runner with no custody"*.

`PinPacks` **replaces** the set rather than adding to it, for the
reason `ToHub::PacksHeld` replaces rather than appends, and is said
once per frame: a frame that changes nothing says that it changed
nothing, and a frame of no keys is custody withdrawn and never a
custody of nothing an eviction has to read.

The welcome carries keys and no figure, so its keys are kept at
`packstore::KEEP_PER_KEY`; `PinPacks` is what refines the count,
because it is the frame that has a count to refine it with.

Reversible either way: both are one function each in
`herd-runner/src/lib.rs`, and the asserted behaviour is in
`herd-hub/tests/pack_custody.rs` and `lib.rs`'s own test module.
```

### The board rows

ORCH-264 closes on its headline, with its residue moved to a successor
row rather than left inside a `done` cell:

```markdown
| ORCH-264 | **The runner's pack gc is written and nothing calls it, so no pack budget is ever spent.** Closed on its headline by M12 P4: `PackStore::evict_to_budget` has a caller, `Session::sweep_packs_to_budget`, run once per connection and before the announcement of what this runner holds, with the hub's `keeper_for`/`PinPacks` set in force. Asserted against the real binary as two processes on two identical disks under one budget — the unpinned runner announces 1 pack of 3 and the keeper announces 3 — in `herd-hub/tests/pack_custody.rs`. **What it does not close is its own second half**, now ORCH-NEXT below: the collection and attempt seams, and `Claims::referenced` against a real job | M12 P3's handoff; closed by M12 P4, 2026-10-30 | herd-step | done | nothing further on this row | nothing | branch `m12-p4-runner-custody`, commit `7e19eed`; `history/2026-10-30-m12-p4-runner-custody.md`; D-NEXT decision 1 |
```

```markdown
| ORCH-NEXT | **The pack gc fires on one edge only, so a runner that never redials never sweeps.** M12 P4 put the pass at the connection edge because that is the only place in the crate with the hub's custody in hand; a runner connected for a week, ingesting all week, stays over its budget until it redials. The two edges ORCH-264 asked for — the end of a collection and the start of an attempt — need the pins threaded through `herd-runner/src/outputs.rs` and `herd-runner/src/job.rs`, which P4 did not own, and `Claims::referenced` still comes from nothing, so P3's reference clause is still proven against a claim set a test wrote | M12 P4's handoff *Not done* 1 and 2, 2026-10-30 | herd-step | owed | Pass the pins and the real claim set to the collection and attempt seams, or give the session a bounded periodic pass, and re-prove the reference clause against a real job rather than a fixture | M11's `{in}`/`{out}` layout step (ORCH-220) | `herd-runner/src/lib.rs` `sweep_packs_to_budget`; `herd-runner/src/packstore.rs` `Claims::referenced`; ORCH-240, ORCH-264 |
```

```markdown
| ORCH-NEXT | **A `--pack-budget` below the weight of one pack is accepted in silence and destroys every unpinned pack, one connection at a time.** Nothing in M12 P1..P4 weighs the figure an operator typed against what a pack of this fleet costs: the pass takes every unpinned pack, the runner announces an empty holding, and the next job fetches one back. The budget is working as specified and there is no refusal, no floor and no sentence in front of it — the same shape `--volume-free-floor` has on the hub side (ORCH-263) | M12 P4's handoff *Risks* 3, 2026-10-30 | herd-step | owed | Decide whether the runner refuses, warns once or says nothing, and name the figure it is weighed against; a floor derived from nothing measured is worse than none | nothing | `herd-runner/src/packstore.rs` `take_budget`, `evict_to_budget`; `herd-runner/src/main.rs` `--pack-budget`; ORCH-263 |
```

```markdown
| ORCH-NEXT | **`Session::pack_store`'s doc claims to be the only caller of `PackStore::open` in the crate, and there are four.** `packstore::leave_packs`, `fetch::forget_incoming`'s site and the mesh endpoint's site all open a store too, measured at `73c2dc8`. The property the doc is protecting is real and narrower — the **health report** reads the store that method remembered, so the report has no second spelling of the data root — and M12 P4's sweep goes through that same method rather than opening its own. The sentence as written would be read by the next agent as a rule the tree already breaks | M12 P4's handoff *Risks* 1, 2026-10-30; not corrected on that branch because the Rust gates had already been submitted on its commit | herd-step | owed | Narrow the sentence to what it protects, in one line, in the next task that touches `herd-runner/src/lib.rs` | nothing | `herd-runner/src/lib.rs` `Inner::pack_store`, `Session::pack_store` |
```

## Decisions needed

**None for Alexa.** Both decisions this task took are inside the
mandate and reversible, and both are drafted above as `D-NEXT` for the
merger to number rather than questions for him.

One **question for the orchestrator**, not for Alexa, and it is a
sequencing question and not a technical one: P5 is what makes any of
this visible in the field, because today's hub names no keeper. Until
P5 lands, this step's observable effect is the budget being spent —
which is ORCH-264, and worth saying in a report as such rather than as
"custody works".

## Resume point

```sh
# P5: the hub chooses keepers and says so. The runner half is done, so
# P5 is asserted end to end against this branch's test file as its
# model, and the two frames need no protocol move.
git worktree add -b m12-p5-keeper-policy <path> origin/main
# Owns: herd-hub/src/server.rs (the `keeper_for: Vec::new()` at :3230,
# and the PinPacks send), herd-hub/src/main.rs (the operator setting).
# Done when: two runners, one named keeper — the keeper's Welcome
# carries the keys and the other's omits the field entirely (the
# skip_serializing_if is already asserted in round_trip.rs:1973), and
# changing the setting sends PinPacks and nothing else.
```

## Processes

- **Started and ended by this agent:** one `./scripts/bench-preflight.sh`
  and **four** `herdctl run` submissions, each through
  `/mnt/ssd_980/herd/queue/bin/herdctl` and never bare, because bare
  `herdctl`'s compiled-in default hub is the **consumer's production**
  (D258). Every `cargo`, `nix` and `rustc` process ran **inside those
  jobs**, under the runner, not under this shell. The job ids and each
  job's `bytes_written` are in the agent's report.
- **No PID was signalled, nothing was killed, no `pkill`, no `killall`,
  nothing by name.** No process of another agent or of the consumer was
  read, signalled or touched.
- **`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set and never
  exported**, by the brief's instruction.
- **Build tree:** `/mnt/ssd_980/herd/build/m12-p4-runner-custody`,
  `du -sh` with the profile named in the agent's report, and deleted
  once the branch was pushed.
