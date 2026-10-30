# m12-r1-free-space-guard — handoff 2026-10-30

- **Item:** ORCH-095 (M12 plan, §3 Track R, task R1)
- **Agent:** herd-step
- **Branch / commit:** `m12-r1-free-space-guard` / `f1b2896`, based on
  origin/main `a8cf397` (batch 19). First committed on `3f3a07c` and
  rebased onto `a8cf397` when batch 19 landed mid-task; all five gates
  were run again after the rebase, and the quoted output below is the
  post-rebase run.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`--volume-free-floor` exists, has no default, and the free space on the
volume the state root sits on now decides when a collection is removed
(D213).** Seven things, each checkable:

1. **The flag.** `herd-hub --volume-free-floor SIZE`
   (`HERD_VOLUME_FREE_FLOOR`), parsed by `herd_proto::parse_bytes` before
   the state root is opened — because opening it sweeps, and this is the
   setting that decides what a sweep takes. No `default_value_t`, so
   `Budget::free_floor` is `Option<u64>` and `None` on every hub that did
   not ask.
2. **The ceiling, derived at every trigger.**
   `Budget::ceiling_against(held, free, floor)` in `herd-hub/src/job.rs`
   is the arithmetic; `JobDirStore::ceiling(held)` in
   `herd-hub/src/store.rs` is the one body that calls it, and both
   triggers go through it: `held_more`'s comparison is now
   `held > self.ceiling(held).bytes` where it was
   `held > self.budget.bytes`, and `sweep_now` takes one reading at the
   start of its walk and bounds its whole loop with it.
3. **One `statvfs`, or none at all.** `JobDirStore::ceiling` returns
   before the syscall when `free_floor` is `None`. *Absent means today's
   behaviour* is therefore true of what the hub **does** and not only of
   the number it compares against.
4. **The measurement.** `free_bytes_on(path)` is `rustix::fs::statvfs`,
   `f_bavail * f_frsize`. `rustix` with the `fs` feature was already a
   `herd-hub` dependency (`herd-hub/Cargo.toml`), so `Cargo.toml` is
   untouched and `Cargo.lock` does not move. `f_bavail` and not `f_bfree`
   on purpose: `f_bfree` includes the reserve `ext4` keeps for `root`,
   and the hub runs unprivileged in the consumer's deployment.
5. **The startup line distinguishes three cases**, read through
   `JobDirStore::volume_ceiling()` so the figure printed is the body the
   sweeper uses (D79). Observed against the real binary, at
   `/mnt/ssd_980/herd/build/m12-r1/debug/herd-hub`:
   - no flag — *"this hub may hold 200 GB of job output, which is the
     200 GB output budget alone, with no --volume-free-floor asked for,
     so this hub does not measure its volume. NOT derived from the
     volume: no free space on &lt;root&gt; was measured to produce it."*
   - `--volume-free-floor 20GB` — *"with --volume-free-floor set, this
     hub may hold 200 GB of job output right now — derived now from
     352257650688 bytes (328.07 GiB) free against a 20 GB
     --volume-free-floor, under the 200 GB output budget. That figure is
     DERIVED from the volume and is re-derived at every eviction
     trigger…"*
   - `--volume-free-floor 900TB` — the same shape with *"may hold 0
     bytes"*, which is this host a long way under that floor.
6. **A hub that cannot measure its volume keeps today's behaviour and
   says so once.** `JobDirStore::say_it_cannot_measure` is a named
   `stderr` line with the reason and the path; the `Live::said_cannot_measure`
   flag is **never cleared**, unlike `said_over_budget`, because a
   `statvfs` that fails on a mount fails again at every trigger for the
   life of the process and a line per arriving collection is D151's
   defect in a new place.
7. **Both figures are said when the volume is what bound the eviction.**
   `removal_sentence` names the free space that was measured, the floor
   that was asked for and the ceiling those two produced, and does
   **not** blame the output budget; `say_what_the_sweep_did` adds a line
   saying the budget is not what bound it and that raising it would
   change nothing. When the **budget** is what bound it, both sentences
   are the ones that shipped, word for word.

**Seventeen new tests, all green, no `#[ignore]` added.** Nine in
`herd-hub/src/job.rs` (`mod the_free_space_ceiling`, pure arithmetic, no
filesystem) and eight in `herd-hub/src/store.rs` (`mod tests`, against
real `statvfs` on the temporary volume):

```
job::the_free_space_ceiling::room_to_spare_on_the_volume_leaves_the_budget_in_charge
job::the_free_space_ceiling::a_volume_running_low_binds_tighter_than_the_budget
job::the_free_space_ceiling::free_space_on_the_floor_is_the_ceiling_the_hub_already_sits_at
job::the_free_space_ceiling::a_volume_below_its_floor_puts_the_ceiling_below_what_is_held
job::the_free_space_ceiling::a_shortfall_larger_than_what_is_held_saturates_at_nothing
job::the_free_space_ceiling::headroom_that_would_overflow_saturates_into_the_budget
job::the_free_space_ceiling::the_ceiling_carries_where_its_figure_came_from
job::the_free_space_ceiling::a_typed_ceiling_and_a_derived_one_read_through_different_renderers
job::the_free_space_ceiling::the_shipped_budget_asks_for_no_free_space_floor
store::tests::a_volume_under_its_free_floor_evicts_inside_the_budget
store::tests::with_no_free_floor_the_ceiling_is_the_budget_and_nothing_is_measured
store::tests::a_measurable_volume_carries_its_reading_into_the_ceiling
store::tests::the_volume_measurement_fails_loudly_on_a_path_that_is_not_there
store::tests::an_unmeasurable_volume_falls_back_to_the_budget
store::tests::a_hub_that_cannot_measure_its_volume_says_so_exactly_once
store::tests::bytes_resent_after_a_free_space_eviction_do_not_book_into_could_not_free
store::tests::the_ceiling_is_derived_at_the_trigger_and_not_kept
```

**The row's three `Done when` clauses, each against its test.**

- *A fixture volume under the floor evicts while inside its budget, and
  says both figures* —
  `a_volume_under_its_free_floor_evicts_inside_the_budget`. The budget is
  1 MiB and the two collections are 1 KiB each, so `held` never comes
  within a thousandth of the budget. **The fixture volume is the real
  one and the floor is what moves**: there is no way to mount a small
  filesystem without being root, so the test reads the volume's own free
  space through `free_bytes_on` and sets the floor a **terabyte** above
  it — larger than any filesystem this test could run on, so no activity
  on the host can make the premise false mid-run, and the premise is
  asserted (`a_floor_this_volume_is_under` panics if it does not hold).
  The identical fixture with `free_floor: None` is run in the same test
  as a control and removes nothing at all, which is what proves the
  budget is not the explanation.
- *A hub whose `statvfs` fails keeps today's behaviour and says so once*
  — three tests, deliberately apart.
  `the_volume_measurement_fails_loudly_on_a_path_that_is_not_there`
  **proves the detector** on a path known not to exist and then shows
  the same call answering on its parent, so the silence below is a
  result and not a dead branch;
  `an_unmeasurable_volume_falls_back_to_the_budget` asserts the ceiling
  is the budget exactly;
  `a_hub_that_cannot_measure_its_volume_says_so_exactly_once`
  asserts on the body that **decides** — `say_it_cannot_measure` returns
  whether this call is the one that spoke — so a version that printed
  every time and set the flag anyway would fail it, where a test reading
  the flag alone would pass.
- *ORCH-124's known leak is not widened* —
  `bytes_resent_after_a_free_space_eviction_do_not_book_into_could_not_free`.
  It is the shape of the landed
  `bytes_written_after_an_eviction_are_swept_again`
  with the eviction driven by the free floor instead of a tiny budget,
  and that is the point: a hub under its free floor is over its ceiling
  **permanently** — evicting its own collections cannot raise free space
  above a floor set above the whole volume — so this is the one place the
  leak would be permanent rather than transient. `could_not_free` is 0
  on the second sweep and the re-sent bytes are taken again. The
  ordering that makes it work is the landed one and is unchanged:
  `write_output_chunk` calls `readmit_to_the_budget` **before**
  `held_more`.

**Docs.** `docs/operating.md`'s settings table gains the flag's row, and
its heading and count sentence are corrected — to **four budgets and six
figures**, which also folds in batch 19's own `--pack-budget` on the
runner, absent from that table when it landed (see *Risks and findings*).
`docs/orchestration/board.md` closes ORCH-095 with its dated resolution,
keeping its question, origin, analysis and fallback.
`docs/plan.md`, `docs/coverage.md` and `docs/disk-policy.md` are
untouched, as the brief required.

## Not done, and why

- **Record removal under space pressure is not here.** This task moves
  the right-hand side of one comparison; the act that removes a *record*
  rather than a collection's bytes is **R4** (ORCH-173, ORCH-102,
  ORCH-099's store half) and needs R2 and R3 first. D213's sentence
  speaks of a record; what ships here is the free-space trigger that R4
  will reuse, applied to the one population the sweeper can already take.
- **`B` is not re-derived.** Whether the budget becomes
  `min(200 GB, 25 % of capacity)` is the second of ORCH-095's two
  figures, and it is deliberately not taken: it is Alexa's, nothing
  needs it, and the row records that it was left.
- **No second `store.rs` change.** The one I wanted and left: when the
  ceiling is below the budget, the sweeper's candidate order is still
  D152's four keys, which were chosen for a budget and not for a volume.
  It is not wrong — the keys are about *which* evidence is worth keeping,
  which does not change with the reason for the shortage — but nobody has
  asked whether a volume-driven sweep should prefer the largest
  collection, which would reach the floor in fewer removals. That is a
  question for **R3**, which owns eviction-by-blob and byte-accurate
  progress, and it is named here rather than answered.
- **`docs/architecture.md:1208`** still says *"TWO figures and no third:
  --output-budget, 200 GB, and --output-floor, 7 days, both printed at
  startup. No tier, no class, no per-job override — invariant 1"*. There
  are now three. Not edited: it is a structural claim in a file this task
  does not own, and a board row for it is drafted below.
- **`herd-hub status` and `status.json` say nothing about the ceiling.**
  The derived figure exists only on the startup line and in the eviction
  sentences. §0.7's reporting requirement is P2b's and R7's; a row is
  drafted below.

## Gates

**Run before this file was committed, on `f1b2896` (rebased onto
`a8cf397`), all five, each run bare and none piped.**

```
$ ./scripts/bench-preflight.sh
load (1m):   0.83 on 12 cores, ceiling 9.00
bench-preflight: ok, safe to start.

$ nix develop --command cargo build --workspace
BUILD exit: 0

$ nix develop --command cargo fmt --all --check
FMT exit: 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
CLIPPY exit: 0

$ ./scripts/check-docs.sh
docs ok
DOCS exit: 0

$ nix develop --command ./scripts/test.sh
SUITE exit: 0
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
passed: 1165   failed: 0
```

`clippy --workspace --all-targets` is also the compile of every test
crate, which is how the *"a new test file has nothing to conflict with"*
trap is answered: every target in the workspace compiles on this base.

**The last run**, on the tree with this file in it:
`./scripts/check-docs.sh`, bare, output to
the scratchpad log named in that report (a path too long for this
file's 79 columns),
and verbatim in the agent's report. The four Rust gates are **not** re-run
for it: this file is prose and changes no compiled byte.

**Skips: 12, every one of them the standing cgroup-delegation set, each
named.**

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
an_io_weight_that_cannot_bite_is_said_out_loud
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces
```

All twelve are the same cause: *"this runner cannot create a cgroup under
/sys/fs/cgroup/chatons.slice/… (Permission denied)"*.

**The two load-gated flood benches declined on an earlier run and ran on
this one, which is the delta the brief warned about and nothing
structural.** On the pre-rebase run the one-minute load was **9.77**
against their ceiling of 8 and `flood-fifty` and `flood-two-fifty`
printed `HERD-SKIPPED` — 14 skips. On the gated run the load was **2.08**
and both ran and passed, which is why the count above is 12. No test was
touched to make that happen.

## Risks and findings

1. **The row's own formula cannot evict, and this is the one place the
   task departs from its brief.** ORCH-095 and the plan both write the
   ceiling `min(B, held + max(0, free − R))`. Read with the clamp on the
   **headroom**, that expression is `≥ held` for every input, because the
   ceiling is recomputed at each trigger from the *current* `held`. So
   `held > ceiling` is only ever true because of `B`, and the free-space
   guard would be dead code — on a volume with no space left, which is
   the one situation it exists for. The clamp belongs on the **ceiling**:
   `min(B, held + free − R)`, saturating at both ends, which on a volume
   under its floor puts the ceiling below what the hub holds by exactly
   the shortfall. That is what ships, the reasoning is in
   `Budget::ceiling_against`'s doc comment, and
   `a_volume_below_its_floor_puts_the_ceiling_below_what_is_held`
   asserts both halves — the strict inequality, without which nothing
   ever evicts, and the exact figure, without which it would take more
   than the volume asked for.
2. **The canonicalisation hazard the coordinator raised is refuted, and
   the refutation is worth keeping.** `JobDirStore::open_with` does
   **not** canonicalise: `let root = root.as_ref().to_path_buf()`
   (`herd-hub/src/store.rs`), and every write in that file is a `join`
   onto that same `PathBuf`. `free_bytes_on` is given `&self.root` — the
   same value — so the measurement resolves through the same symlinks,
   the same `.` components and the same mounts as the writes it measures,
   and cannot answer about a different device. This is unlike
   `DataRoot::open` on the runner, which does canonicalise. **What would
   reintroduce it** is measuring a canonicalised copy, or re-deriving the
   path from `args.state_dir` instead of asking the store; the doc
   comment on `free_bytes_on` says so in those words. One consequence
   worth naming: the hub's startup line prints `args.state_dir` in the
   no-floor branch, which is cosmetic there because nothing was measured.
3. **`show_bytes` was the wrong renderer for a measured figure, and the
   tree already knew.** The first version printed every byte count
   through `herd_proto::show_bytes`, and the real binary's startup line
   came out as *"derived now from 344450708 KiB free"* — right,
   unreadable, and the unit a property of the number rather than of the
   quantity, which is **ORCH-186** exactly. `herd_proto::size::measured_bytes`
   exists for this and its doc comment states the rule: *"did a human
   type this figure, or did we measure it?"*. Applied: the budget and the
   floor go through `show_bytes` (typed), the free space through
   `measured_bytes` (measured), and the ceiling through
   `Ceiling::said_bytes()`, which chooses by whether the ceiling **is**
   the typed budget. Caught by reading the binary's output, not by any
   gate; `a_typed_ceiling_and_a_derived_one_read_through_different_renderers`
   is the regression.
4. **`collection_is_whole`'s rule did not move, so `retention.rs`'s
   replica is not stale.** The brief asked to say so loudly if it did.
   The function is untouched on this branch — `git diff a8cf397 HEAD`
   shows no hunk inside it — and nothing this task added reads or
   re-derives wholeness. R3 still owns that rule.
5. **`Budget` gained a field, which is the one API break this task
   makes.** Eight struct literals needed `free_floor: None`:
   `herd-hub/src/main.rs`, four in `herd-hub/src/store.rs`'s tests,
   `herd-hub/tests/retention_preview.rs`, `herdctl/tests/fixture/mod.rs`,
   and `herd-hub/tests/crash_points.rs` needed none because it already
   used `..Budget::default()`. **A branch written against an older base
   that constructs `Budget` will not compile after this lands**, and the
   failure is a compile error and not a silent one. Inventoried against
   `origin/main` `a8cf397` with `git grep -n 'Budget {' origin/main
   -- '*.rs'`: fifteen hits, seven of them `fn output_budget` signatures
   and the `struct`/`impl` lines, eight real literals, all eight fixed.
   Batch 19 added none.
6. **The over-the-ceiling line changed shape, and only when the ceiling
   is not the budget.** `say_what_the_sweep_did`'s once-per-episode
   `stderr` line now compares against `ceiling.bytes` rather than
   `budget.bytes`, which is what makes it fire for a hub under its free
   floor with nothing it may remove — the situation an operator most
   needs it for. Its **text** is unchanged byte for byte when the ceiling
   is the budget, which is every hub with no `--volume-free-floor`; the
   clause naming the ceiling is added only when the ceiling is lower.
7. **A sweep takes one reading, not one per collection, and the argument
   for that is in the code.** `held + free` does not move as the sweeper
   works: every byte it frees leaves `held` and arrives in `free`. So one
   absolute figure bounds a whole sweep. What **does** move it is
   anything else writing to the volume during the sweep, which is the
   next trigger's business — stated in `Budget::ceiling_against`'s doc
   comment rather than left for a reader to rediscover.
8. **The eviction's candidate order is still D152's four keys** (see
   *Not done*). A volume-driven sweep could reach its floor in fewer
   removals by preferring the largest collection. Unasked and unanswered;
   R3's.
9. **`herd-hub report` and `status.json` still say nothing about the
   ceiling**, so an operator who cannot read the startup log cannot see
   the figure that is removing their evidence. Row drafted below.
10. **The free-space floor is not validated against the budget.** A hub
    given `--volume-free-floor` larger than its volume starts, derives a
    ceiling of 0, and evicts every collection it may take — which is
    honest, said in three sentences, and is also a configuration nobody
    would choose on purpose. **R8** owns startup refusals of
    configurations that cannot mean what they say
    (`--output-floor 30d` against a 14-day span); this belongs beside it,
    and the row is drafted below rather than built here, because a
    refusal is a behaviour change on the one path an operator cannot work
    around.

## Drafted for `herd-merger`

### `docs/decisions.md` — the full entry to paste

```markdown
## 2026-10-30 — D-NEXT The free-space floor is a fact about a disk, not a second policy

D213 said free space on the filesystem the state root sits on decides when
something is removed, never the output budget. ORCH-095 asked whether a
second disk number is the first line of a policy language, which invariant
1 forbids. It is not, and the distinction is worth writing down because the
next disk figure will be asked the same question.

**A policy knob ranks one job's evidence above another's. A floor says
where the disk stops.** `--volume-free-floor` names no job, no class, no
label and no priority; it cannot be set per job and there is nothing to
express in it. What goes, and in what order, is still D152's four keys and
still Herd's. The only thing the floor changes is the number those keys
are read down to — from `--output-budget` to
`min(--output-budget, held + free − floor)`, derived from `statvfs` at
every trigger rather than from a reading taken at startup, because the
volume is shared with whatever else the operator put on it and a reading
from an hour ago is a guard against the past.

**It ships with no default, and that is the decision rather than a
deferral.** Required when a free-space trigger is wanted; absent, nothing
measures the volume, not one `statvfs` is called, and the output budget
alone decides — this hub's behaviour before the flag existed, byte for
byte. A figure chosen here would be a figure applied to a disk nobody in
this project may look at (invariant 9, D161), and the thing it does when
it is wrong is delete evidence its owner never asked it to delete. That is
D194's rule and `herd-hub report`'s, and it is why the second of ORCH-095's
two figures — whether the budget becomes `min(200 GB, 25 % of capacity)` —
is left exactly where it was.

**A hub that cannot measure its volume keeps the behaviour of a hub with
no floor and says so once**, on `stderr`, naming the path and the reason.
Not an error and not a refusal to start: a guard that is not in force is a
worse outage as a dead hub than as a loud one. Said once and never again,
because a `statvfs` that fails on a mount fails again at every trigger for
the life of the process, and a line per arriving collection is D151's
defect in a new place.

**And the arithmetic ORCH-095's row carried was wrong, which is recorded
because the row was read by three documents.** The row wrote the ceiling
`min(B, held + max(0, free − R))`. With the clamp on the headroom that
expression is never below `held`, because it is recomputed at each trigger
from the current `held` — so the guard would be dead code on a volume with
no room left. The clamp belongs on the ceiling:
`min(B, held + free − R)`, saturating, which on a volume under its floor
puts the ceiling below what the hub holds by exactly the shortfall and
frees that and no more.

Evidence: `herd-hub/src/job.rs` (`Budget::free_floor`,
`Budget::ceiling_against`, `Ceiling`, `FreeSpace`);
`herd-hub/src/store.rs` (`held_more`, `JobDirStore::ceiling`,
`free_bytes_on`, `say_it_cannot_measure`); `herd-hub/src/main.rs`;
`docs/operating.md`; `history/2026-10-30-m12-r1-free-space-guard.md`;
17 tests, 1165 passing, 12 skips all cgroup-delegation.
```

### `docs/orchestration/board.md` — three rows to paste

To the **Open work** table, in id order wherever the merger allocates
them. ORCH-095 itself is already closed on this branch and needs nothing
from the merger.

```markdown
| ORCH-NEXT-A | **`docs/architecture.md` says there are two disk figures and there are now three.** Line 1208 reads *"TWO figures and no third: --output-budget, 200 GB, and --output-floor, 7 days, both printed at startup. No tier, no class, no per-job override — invariant 1"*. `--volume-free-floor` landed on `m12-r1-free-space-guard`, directed by D213, so the claim is false and the sentence it is making — that this is a bound and not a policy — is still true and is the one worth keeping | `m12-r1-free-space-guard`, 2026-10-30, named as a finding rather than edited: a structural claim in a file the task does not own | herd-step | owed | Restate the node as **three** figures and keep its argument: a floor on free space names no job and expresses nothing, so invariant 1 is not touched. The wording to reuse is in `Budget`'s own doc comment in `herd-hub/src/job.rs`, which was rewritten for the same reason. Prose only, so `check-docs.sh` alone | the ORCH-095 landing | `docs/architecture.md:1208`; `herd-hub/src/job.rs` `Budget`; `history/2026-10-30-m12-r1-free-space-guard.md` |
| ORCH-NEXT-B | **The derived ceiling is on the startup line and nowhere a report can read it.** `herd-hub report` and `status.json` say nothing about `--volume-free-floor`, the free space measured against it, or the ceiling those two produce — so an operator whose evidence is being removed by the volume rather than by the budget can see why only in the hub's log, and only if they still have it. §0.7's requirement is that a population be reported with its size; this is the figure that explains the removals | `m12-r1-free-space-guard`, 2026-10-30 | herd-step | owed | Fold it into **P2b** or **R7**, both of which already own `status.rs` and `report.rs`, rather than taking a third task over those files. What to carry: the floor as typed, the free space as measured, the ceiling, and which of the three provenances produced it — `herd_hub::Ceiling` already answers all four and `Ceiling::said()` already renders the sentence | ORCH-095 (landed); P2b, R7 | `herd-hub/src/job.rs` `Ceiling`; `herd-hub/src/main.rs` startup line; `herd-hub/src/status.rs` and `report.rs`, no occurrence of the ceiling |
| ORCH-NEXT-C | **A `--volume-free-floor` larger than its volume is accepted in silence and evicts everything the sweeper may take.** The hub starts, derives a ceiling of 0, says so in three sentences and removes every collection it is allowed to remove. Honest, and a configuration nobody chooses on purpose — the same class as `--output-floor 30d` against a 14-day span, which **R8** refuses at startup before the port is bound | `m12-r1-free-space-guard`, 2026-10-30, named rather than built: a startup refusal is a behaviour change on the one path an operator cannot work around | herd-step | owed | Take it **with R8** and not before it, in the same `main.rs` pass and with the same shape: name both figures, refuse before the state root is opened, and refuse only what cannot mean what it says. The open question is which comparison that is — a floor above the volume's **capacity** is certainly absurd, a floor above its current **free space** is the ordinary case this feature exists for, and the two are easy to confuse | R8 (ORCH-100, ORCH-101) | `herd-hub/src/main.rs`; `herd-hub/src/store.rs` `JobDirStore::ceiling`; `history/2026-10-30-m12-r1-free-space-guard.md` |
```

## Decisions needed

**None for Alexa.** The mechanism was D213's and the two figures were
ruled by the orchestrator on 2026-10-30 — ship with no default, leave `B`
alone — which is what this branch implements and what ORCH-095's closed
row records.

For the orchestrator, one thing to know rather than decide: **Track R's
serial chain is now unblocked at its first link and R2 is next**, and R2
still waits on **ORCH-199 E** (deduplication on by default), which is
Alexa's and has not been asked. R1 was put first precisely so the chain
would not wait on him; that purchase is spent the moment R2 starts.

## Resume point

**`herd-merger`:** nothing else is owed on this branch. It is one commit
on `a8cf397` with all five gates green; the drafted `D-NEXT` and the
three board rows above are the whole of what the merger must paste.

**The next task in Track R is R2 (ORCH-171, deduplication by hard link),
and it is blocked on ORCH-199 E, not on this.** When it starts:

```sh
git worktree add -b m12-r2-dedup-hard-link <path> origin/main
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-r2
```

R2 owns `herd-hub/src/store.rs` exclusively and must not share a worktree
or a window with R1, R3, R4, R6 or R7. Two things from this task that
bear on it directly: **`Budget` has a third field**, so a branch cut
before this one will not compile until its `Budget` literals are fixed
(eight sites, listed under *Risks and findings* 5); and **`held_more`'s
right-hand side is now `self.ceiling(held).bytes`** — R2 and R3 change how
the left-hand side is *produced* (`directory_bytes`' one body, `evict`'s
decrement) and should not need to touch the comparison at all, which is
the boundary §12's H-E predicted and this task confirms.

## Processes

**PIDs started: four, all mine, all ended on their own.** Three
`herd-hub` processes run under `timeout 4` to read the startup lines
against the real binary — one per provenance case — and one
`herd-hub --help`. Each exited by its own `timeout` or at the end of its
output; none was signalled, and no PID that this task did not start was
sent anything. The consumer's hub `3006543` and runner `3021002`, and
this project's pinned hub `565152` and runner `607643`, were not touched,
read or signalled.

**Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-r1`, never
the worktree and never the root filesystem.**
`du -sh /mnt/ssd_980/herd/build/m12-r1` → **16G, debug profile**, which
is above the 12–14 GB the brief gave for eight measurements on
2026-10-30; this tree carried two full gate runs, one before the rebase
and one after. Deleted by literal path once the branch was pushed:
`rm -rf /mnt/ssd_980/herd/build/m12-r1`. The worktree's own `target/` was
never written to. `/mnt/ssd_980` had 352 GiB free at the first preflight
and the disk was never the binding constraint.
