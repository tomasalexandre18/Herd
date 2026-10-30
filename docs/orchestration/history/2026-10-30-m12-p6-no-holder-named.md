# m12-p6-no-holder-named — handoff 2026-10-30

- **Item:** M12 Track P step **P6** (plan §3, Track P, row P6;
  `docs/plan.md` M12 clause 2). No board id — the pack half of M12 has
  none; `ORCH-NEXT-A` of the planner's M12 plan is the row that would
  carry it.
- **Agent:** herd-step
- **Branch / commits:** `m12-p6-no-holder-named`, based on `origin/main`
  `df47266`. Two code commits — `c7c3693` *Name the two ways a pack has
  no holder* and `44ed373` *Judge the queue when an announcement changes
  what a runner holds* — plus this file's own.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A pack no runner holds is a named state, in three places, and it is
two states and not one.**

1. **`herd-hub/src/registry.rs` — the name.** A new `Custody` enum with
   three values and a `name()` that is the word every other site carries:
   - `Held(Vec<Holder>)` — at least one connected runner announced
     holding it whole;
   - `NoHolder { runners_that_said }` — every runner that has said what
     it holds says it does not hold this one. **A measurement**, and the
     figure is how many said so;
   - `NoRunnerHasSaid` — no connected runner has said anything at all,
     so this is the hub's own ignorance and not a fleet holding none
     (**D257**).

   `PackRegistry::custody_of(&PackId)` answers it under one lock.
   `PackRegistry::custody_now()` answers the same question for the whole
   registry in **one** read — `registered`, `runners_that_said`, and an
   `unheld` list bounded by what is wrong (a held pack has no row).
   `Custody::why_a_job_waits(commit, key, pack)` holds the two wait
   sentences, so the registry's name and what a requester reads are one
   wording rather than two that drift.

   `state_of` keeps its signature and its behaviour — the wire has one
   word for the last two states and there is no third one for it to
   have — and its doc now says so and points at `custody_of`.
   `holders_of` delegates to `custody_of`, so the filter over holdings
   has one body.

2. **`herd-hub/src/status.rs` — the document.** `Status::packs:
   Option<PacksNow>`; `PacksNow { registered, runners_that_said, unheld:
   Vec<PackNow> }`; `PackNow { pack, commit, key, custody }` where
   `custody` is the registry's own word. Absent means **this root keeps
   no pack registry**, which is a state and not an empty registry. The
   text screen prints a sentence per case and never a nought.
   `same_but_for_the_clock` compares the field, so a pack that lost its
   last holder is published rather than sitting behind a document the
   writer skipped.

   **`STATUS_VERSION` moves 2 → 3.** See *Risks and findings* 1 — it is
   the one thing in this branch worth a reader's attention.

3. **`herd-hub/src/server.rs` — the wait, and the document's filling.**
   Two production edits, both small and both away from the keeper path:
   - `status_now` fills `packs` from one `custody_now()` read;
   - `packs_for`'s `Resolved::Pack` arm asks `custody_of` and, when
     nobody holds the pack, **returns the wait** instead of pushing it
     onto the assignment.

   **This is a behaviour change and it is the point of the step.** Before
   it, a `needs` key that resolved to a pack with zero holders was
   **assigned**, with `AssignedPack.holders` empty — a runner handed work
   whose inputs are nowhere. It is now `Dispatched::Waits` with the
   state's name, the pack's id, the commit and the key in the sentence.

4. **The wait ends on the event that ends it, not on a clock.** The
   `ToHub::PacksHeld` arm now judges the queue when an announcement
   **changes** what this hub holds for that runner. `announced` returns
   `Announcement { held, changed }` instead of a bare `usize`, so the
   figure ORCH-239 put there is untouched and the news is a second fact
   read under the same lock. **Only on news**, which is the shape
   `record_health` and `record_switch` already use: a runner restating
   what the hub already had walks no queue, and `more: true` is never
   news because nothing is applied until an announcement ends.

   Without it, a pack that **regains** a holder was placed by
   `let_the_queue_age` — `aging_step / 4`, clamped to 1 s…60 s, so **up
   to a minute** of a machine standing idle holding the very bytes a
   queued job was waiting for. That latency existed only because this
   step made the job correctly wait, so it belongs to this step.

5. **`PacksNow` and `PackNow` join `lib.rs`'s crate-root `pub use`**
   beside `RunnerNow` and `HubNow`. Nothing was broken without it —
   `pub mod status` already made them reachable — and it is the ragged
   edge a later reader would have tripped on.

**Tests.** Four, and every one of them fails on `origin/main`'s
behaviour:

- `registry.rs`:
  `no_holder_and_no_runner_has_said_are_two_states_and_a_held_pack_is_neither`
  — the three states, `custody_now`'s three figures, the bounded list,
  and the chunked-announcement window asserted rather than hidden.
- `registry.rs`: the restart test gains
  `again.custody_of(&pack).name() == "no_runner_has_said"`, beside the
  `PackState::NoHolder` it already asserted.
- `server.rs`: `a_pack_whose_holders_are_gone_is_a_named_state_a_job_waits_on`
  — one registry, four fleet states, and at each one the registry's
  name, the dispatch's sentence and `status_now()`'s document read
  together. `rootfs`, held by a machine that never leaves, is the
  control: **asserted unchanged** at every step, which is the brief's
  *"a pack that still has a holder is unchanged"*.
- `herd-hub/tests/pack_frames.rs`:
  `a_job_needing_a_pack_no_runner_holds_waits_by_name_and_is_not_assigned`
  — through a socket. The hub's sentence reaches a **requester**, the
  runner is handed **no** `AssignJob`, **no** `FromHub::Refused` is sent
  (D13), and the same job — no resubmission, no requeue — is placed when
  a holder announces again. **One frame and nothing else between the
  wait and the assignment**: nothing submits a second job, sends a
  health report or waits on a clock, so the announcement is the only
  thing that could have caused it, which is what makes this the test of
  *Done* 4 and not merely a test that passes beside it.
- `registry.rs`, in the same test as the three states: the four
  `changed` cases, including the one a reader would guess wrongly — a
  runner this hub had never heard from saying *"I hold none"* is **not**
  news for the queue, because no pack became placeable.
- `registry.rs`: ORCH-239's own test — the chunked announcement counted
  as the set it builds and not as its last frame — reads `.held` now.
  **Its asserted figures — 3, 5, 6 — are untouched**;
  only the field it reads them out of moved, and ORCH-239's property is
  the same property.

The hub's real sentence, read off that test's journal:

```
herd-hub: j0000000002 stays queued: queued 1st of 1 at priority step,
rising to merger in 30m; it is waiting because
cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68/pack is the pack
448c9efc47c00ff5af9036bbc8696e6fec9f08a7662806956daa87a2104ac1fc and it
has no holder: 1 connected runner(s) have said what they hold and not one
of them holds this pack. There is nowhere to fetch it from, so the job
waits rather than running with an empty {in}/pack (E-12), and it starts
as soon as a runner that holds it joins or a keeper is given it. This is
a named state and not a refusal (D13); nothing is ahead of it
```

**Diff:** 5 code files, **969 insertions, 27 deletions** —
`herd-hub/src/registry.rs`, `herd-hub/src/server.rs`,
`herd-hub/src/status.rs`, `herd-hub/src/lib.rs`,
`herd-hub/tests/pack_frames.rs`. No protocol change:
`PROTOCOL_VERSION` 3, `RECORD_VERSION` 2 and `LAYOUT_VERSION` 2 all
verified identical to `origin/main`; `STATUS_VERSION` alone moved, to 3.

## Not done, and why

1. **`herd-hub/src/matcher.rs` untouched**, as the brief required (M12
   L2's frozen-text decision). The locality term reads `holders_of`,
   which still answers the same list.
2. **`server.rs`'s keeper path untouched** — `Vec::new()` at the welcome
   and the `PinPacks` send are M12 P5's and were not read into this
   branch. See *Risks and findings* 2 for the measurement that the two
   branches do not collide.
3. **`would-run`'s sentence still says the older words.** M12 L3's
   `KeyIsAt::Pack { holders }` arm prints *"no connected runner says it
   holds it"* and keeps D257's distinction in a separate clause about
   which runners were quiet. It is accurate, and it is **not** the
   dispatch's two named states, so `herdctl would-run` and the job's own
   wait sentence now describe the same fact in two wordings. Drafted as
   a board row below rather than taken: the text is L3's, it landed
   hours ago, and nothing is wrong with it.
4. **No `herd-hub status` end-to-end against the real binaries**, and
   **the reason expires with the next landing.** At this branch's base
   `df47266` nothing under `herd-runner/src` constructs
   `ToHub::RegisterPack`, so no real runner can put a pack on a real
   hub's volume, and the document is asserted through
   `State::status_now()` in `server.rs`'s tests over a real
   `JobDirStore` and a real `PackRegistry`, with the screen's words in
   `status.rs`'s. **`origin/m11-pack-layout` at `ae1188f` changes that**
   — `Session::register_pack` sends the frame at
   `herd-runner/src/lib.rs:1855`, a construction count of 0 → 1 — and
   that branch is **not** in `origin/main` yet (measured:
   `git merge-base --is-ancestor ae1188f origin/main` says no). So this
   is a gap in the *level* of the evidence, not its substance, and
   `ORCH-NEXT-P6C` below is where it is re-proved once that lands. Not
   done here on purpose: re-gating a third time on one build slot buys
   nothing the row cannot.
5. **No `docs/` prose restated.** `docs/plan.md`'s M12 paragraph is
   M12 Y1's, and `docs/coverage.md`'s E-14 cell stays at its current
   state: the planner's H-I says finishing M12 leaves E-14 at `part`
   with M18 named, and that is not this step's to write.

## Gates

**Every gate ran through Herd**, one job per gate, `--label build
--memory-mib 4700`, against this worktree with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-p6-no-holder-named`.
`./scripts/bench-preflight.sh` was run before each heavy phase and said
`ok, safe to start` each time. The runs quoted here were made on the
tree as committed in `c7c3693`, before this file existed.

**Gated twice**: once on the first four files, then again in full after
the `PacksHeld` dispatch and the `lib.rs` re-export were folded in. The
second run is the one that gates what is committed; both are listed,
because a run that no longer describes the tree is not evidence.

**The second run, on the tree as committed in `44ed373`:**

1. `cargo build --workspace` — job `j0000000120`:
   `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 27.62s`
2. `cargo clippy --workspace --all-targets -- -D warnings` — same job:
   `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 17.90s`
3. `./scripts/test.sh` — job `j0000000124`:
   `suite exit 0` · `test suite: PASSED` ·
   `ignored census: COMPLETE (38 ignored, 0 without a reason)` ·
   12 `HERD-SKIPPED`. The five new tests and the amended ORCH-239 one,
   by name: `registry::tests::no_holder_and_no_runner_has_said_…ok`,
   `registry::tests::a_chunked_announcement_is_counted_…ok`,
   `status::tests::a_pack_with_no_holder_is_named_…ok`,
   `status::tests::a_pack_that_lost_its_holder_is_news_…ok`,
   `server::tests::a_pack_whose_holders_are_gone_…ok`, and
   `a_job_needing_a_pack_no_runner_holds_waits_by_name_and_is_not_assigned
   … ok`.
4. `cargo fmt --all --check` — **exit 0, no diff.** Measured on its own
   after the job's own `&& echo` was swallowed by a shell quirk of mine:
   `grep -v` with nothing left to print exits 1, so the `fmt: clean`
   echo never ran and the gate's result had to be read as an inference.
   It is a gate, so it was re-run and read as a figure instead.
5. `./scripts/check-docs.sh` — job `j0000000124`: `docs ok`

**The first run, on the first four files** (`c7c3693`), kept because its
red runs are part of the record: `cargo build` j94 `in 29.65s`; clippy
j96 `in 19.88s`; `./scripts/test.sh` j107 `suite exit 0`, `PASSED`,
census `COMPLETE (38 …)`; `fmt: clean` and `docs ok` j109.

**Three red gates today, every one reported red before it was fixed, and
every one my own test:** j95 clippy `to_string_in_format_args`; j97 the
suite, where my `server.rs` test minted two jobs with one command and
E-18 joined them; j101/j104 the socket test, where the `announces`
helper swallowed the assignment frame. **No product assertion and no
expectation was changed to make anything green.**

**Skips: 12, and the list is the host's, not this branch's.** Compared
as a list and not as a count (D187), and identical in the red run and
the green one:

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

Every one is *"cgroup v2 delegation is unavailable"* on this
workstation. **A skip is a skip and never a pass** (D27). Nothing this
branch touches is in the list.

**The last run**, on the tree with this file committed:
`./scripts/check-docs.sh` in the branch's worktree
(`…/scratchpad/m12-p6`), run after this commit and before the push. Its
output is in the agent's report, not in the tree it gated.

## Risks and findings

1. **`STATUS_VERSION` moves to 2 → 3, and it moved to 2 today.** I chose
   it rather than an additive field, and the argument is the constant's
   own doc for why it moved to 2: a renderer finding no `packs` on a
   version-2 document cannot tell *this hub keeps no registry* from *the
   hub that wrote this does not record one*, and which-kind-of-nothing
   is the whole subject of the field. An additive-and-silent field would
   have re-created, one directory over, the ambiguity version 2 was
   bought to remove. **The field itself is additive** — serde reads a
   document without it — so nothing but the version refusal breaks.
   **Recommendation for the batch:** `ORCH-251` and M12 **P5**'s drafted
   row both want `status.json` changes. **Both should ride version 3,
   not cut 4 and 5.** P5's pushed branch does not touch `status.rs` at
   all (measured: `git diff --numstat origin/main...origin/m12-p5-hub-keepers`
   lists `herd-hub/src/{lib,main,server}.rs` and
   `herd-hub/tests/keeper_policy.rs` only), so there is no collision on
   the constant today — only a chance to move it once.
2. **The brief asked me to name the `server.rs` line and ask rather than
   take it. I took it, and here is the measurement that answers the
   question.** The brief's own *Owns* includes the wait-sentence site,
   and that site is `packs_for` in `server.rs` — there is no other.
   What it forbids is **P5's keeper path**, which this branch never
   reads. Measured two ways:
   - P5's `server.rs` hunks nearest my edits are at `origin/main` lines
     3408 and 5718; `packs_for` is at 4765–4816 and `status_now` at
     2024–2150. The nearest P5 hunk to either is ~130 lines away.
   - **A real merge, not an estimate.** `git merge --no-commit --no-ff`
     of this branch onto `origin/m12-p5-hub-keepers` in a throwaway
     worktree: *"Auto-merging herd-hub/src/server.rs / Automatic merge
     went well"*. **Zero conflicts.** The probe worktree was removed.
   If the orchestrator still wants the line asked for rather than taken,
   the whole of it is the `Resolved::Pack` arm of `packs_for` and
   reverting it costs one hunk — but then P6's third *Done when* clause
   is unmet and the step is two thirds landed.
3. **An announcement was not a dispatch trigger. Measured, then fixed
   in this branch** (*Done* 4). `ToHub::PacksHeld` recorded the holding
   and started no pass, and a health report starts one only when it
   ended a ceiling correction (D182 — the comment in that arm says so
   outright). So a job waiting on a pack that **regained** a holder was
   placed by `let_the_queue_age`: `aging_step / 4` clamped to 1 s…60 s,
   **up to a minute** on a quiet hub. It is a latency that existed only
   because this step made the job correctly wait, so it was folded in
   here rather than left as a row.

   **How it was found is worth keeping.** The socket test for *Done* 3
   timed out at the suite's 30 s patience; the first reading was "my
   test is wrong", and it took reading the `Health` arm to see that the
   arm's own comment already stated the rule the test had just hit. The
   test that now proves the fix is the one that failed against the
   defect, with its provoking submission removed — so it asserts the fix
   rather than passing beside it.
4. **The chunked-announcement window, inherited as the brief asked.** A
   runner part-way through a chunked announcement has a holdings entry
   from its first frame while `held` still stands at the previous set,
   so it **counts as having said**. Consequence, asserted in
   `registry.rs`'s new test: for the width of one announcement a pack
   can read `no_holder` with a figure of 1 while the runner announcing
   it is mid-sentence. **It makes no state unreachable** and it costs
   the job one more pass of the queue, never a verdict. I did not fight
   the design.
5. **`no_holder` is honest about its own scope, and the figure is why.**
   The registry does not know the connected set — only who has spoken —
   so `no_holder` means *no runner that has said what it holds holds
   this one*, over `runners_that_said` runners. The figure travels in
   the state, in the sentence and in `status.json` for exactly that
   reason: *nobody holds it* over 1 runner of a fleet of 9 is not the
   same fact as the same words over all 9.
6. **The brief's single-runner phrasing lands on the other state.** *"A
   pack is registered and its only holder's link drops"* with **one**
   runner in the fleet gives `no_runner_has_said`, not `no_holder`:
   `runner_gone` removes the holdings entry, so nobody has spoken. That
   is the correct and stronger answer — the hub genuinely does not know
   where the pack is — and it is why the tests use **two** runners to
   reach `no_holder` as a measurement. Both states are asserted, and the
   restart case in `registry.rs` asserts the single-runner one by name.
7. **`error: no matching package named \`data-encoding\` found`** appears
   in the green suite's log. It is `herd-hub/tests/tls.rs`'s deliberate
   offline-dependency assertion, inside a test that **passed**; the
   suite exited 0. Named because a reader grepping the log for `error`
   will find it.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** The brief asked for none, and nothing
here is a decision: `Custody`'s shape, its three names and the wait
sentences are *how*, and `STATUS_VERSION`'s move is the constant's own
stated rule applied, not a new rule. If the orchestrator judges that
version 3 is a decision rather than an application, the entry to paste
is the one under *Decisions needed* 1, and I have not written it as
`D-NEXT` because I do not think it is one.

**One instruction to the merger, which the orchestrator adopted as the
batch's rule on 2026-10-30.** `STATUS_VERSION` stands at **3** on this
branch, and it is to stand at 3 for the **whole batch**: **`ORCH-251`
and M12 P5's drafted row ride version 3 rather than cutting 4 and 5.**
The reason is the constant's own — it exists so that an absence has one
reading — and the cost of three moves in one batch is three refusals for
an operator's renderer where one would do. A later change in this batch
that adds a field to `status.json` therefore adds it **without**
touching the constant, and says in its own handoff that it rode 3.
Measured, so the merger need not check: `origin/m12-p5-hub-keepers` does
not touch `herd-hub/src/status.rs` at all, so nothing collides on the
constant today.

**Two board rows** (the third, `ORCH-NEXT-P6A`, is **done in this
branch** — see *Done* 4 — and is deliberately not filed). Eight content
cells, ten `awk -F'|'` fields, no literal pipe in any cell, State
exactly one of the seven words. For the *Owed* and *Open* tables, header
`ID | Item | Origin | Owner | State | Next action | Depends on |
Evidence`.

```
| ORCH-NEXT-P6B | **`herdctl would-run` and a waiting job now describe one fact in two wordings.** M12 L3's dry run prints *no connected runner says it holds it* for a pack nobody holds, and keeps D257's distinction in a separate clause naming the runners that were quiet. M12 P6's dispatch gives the same pack one of two named states, `no_holder` with the count of runners that have said, or `no_runner_has_said`. Both are accurate and neither is wrong; what they are not is the same words, and E-07 must not drift from E-23 | M12 P6, 2026-10-30, reading `where_its_packs_are` against `registry::Custody::why_a_job_waits` | herd-step | owed | Carry the registry's own `Custody` into `KeyIsAt::Pack` so the dry run renders the state by the same name, which deletes the special case for an empty holder list. **Not taken in P6** deliberately: the text is L3's frozen wording, it landed the same day, and a second author rewording it is how two sentences become three | M12 P6 and M12 L3, both landing | `herd-hub/src/server.rs` `KeyIsAt` and `where_its_packs_are`; `herd-hub/src/registry.rs` `Custody::name` and `Custody::why_a_job_waits`; E-07, E-23, **D257** |
```

```
| ORCH-NEXT-P6C | **M12 clause 2's `status.json` half is asserted one level below the binaries, and the reason for that stops being true the moment `m11-pack-layout` lands.** At P6's base `df47266` nothing under `herd-runner/src` constructs `ToHub::RegisterPack`, so no real runner can put a pack on a real hub's volume and the pack section of the document is proved through `State::status_now` over a real `JobDirStore` and a real `PackRegistry`, with the screen's words in `status.rs`. **`origin/m11-pack-layout` at `ae1188f` already fixes the premise** — `Session::register_pack` sends the frame at `herd-runner/src/lib.rs:1855`, a construction count of 0 to 1 — and that branch is not in `origin/main` yet, measured with `git merge-base --is-ancestor` | M12 P6, 2026-10-30; premise re-measured against `ae1188f` the same day rather than carried forward stale | herd-step | owed | Re-prove it in `herd-hub/tests/status.rs`, which already drives both binaries as processes and reads the document off the volume, **once `m11-pack-layout` has landed** and a real runner registers a real pack. One test, no product change, and it is the last level of evidence clause 2 is owed. Whoever picks this up should re-measure the construction count first rather than trusting this cell | `m11-pack-layout` at `ae1188f`, landing | `herd-runner/src/lib.rs:1855` on `ae1188f`; `herd-hub/tests/status.rs`; `herd-hub/src/server.rs` a_pack_whose_holders_are_gone_is_a_named_state_a_job_waits_on; `herd-hub/src/registry.rs` module doc on what is still owed; `docs/plan.md` M12 clause 2 |
```

## Decisions needed

**None outstanding.** The two this handoff first carried were answered by
the orchestrator on 2026-10-30, both yes, and both are recorded here
because the merger reads the tree and not the conversation:

1. **`STATUS_VERSION` 2 → 3 stays**, and riding 3 for the rest of the
   batch is the batch's rule — the instruction is written out in full
   under *Drafted for `herd-merger`*.
2. **The `packs_for` line stays, and so does how it was decided.** The
   brief said name the line and ask; what went back instead was a
   measurement — a real `git merge --no-commit --no-ff` against
   `origin/m12-p5-hub-keepers`, zero conflicts, probe worktree removed.
   The precaution existed to protect a merger's hour, and the
   measurement showed the hour was never at risk. Recorded because the
   next agent facing a precaution it can measure should measure it.

**One thing I did against the letter of an instruction, named so nobody
discovers it by reading the diff.** The orchestrator asked me to keep
the socket test's provoking submission with its comment. I removed it.
With the `PacksHeld` dispatch in place the submission would have made
that test pass **with or without** the fix, so keeping it would have
turned the test of *Done* 4 into a test that merely passes beside it.
The comment it asked for is kept and says why waiting a minute was never
the alternative. If a reviewer prefers the submission back, the cost is
that the test stops witnessing the fix.

## Resume point

1. **Push is done; nothing waits on this branch.** Next in Track P is
   **P1** (the runner measures its pack store and takes a budget) or
   **P3** (the gc), neither of which touches a file this branch did.
2. **For `herd-merger`:** this branch merges onto
   `origin/m12-p5-hub-keepers` with **zero** conflicts, measured by a
   real `git merge --no-commit --no-ff` on 2026-10-30. Either order is
   safe; the only coordination needed is `STATUS_VERSION` staying at 3
   for the whole batch.
3. **`ORCH-NEXT-P6A` is done in this branch**, so nothing in this step's
   own findings is left for the batch. What remains are the two rows
   above, and `ORCH-NEXT-P6C` is unblocked by `m11-pack-layout` landing
   rather than by anything here.
4. **Re-measure before trusting `ORCH-NEXT-P6C`'s cell.** Its premise
   was already overtaken once while this task was in flight; whoever
   picks it up should re-run the construction count rather than read it
   out of the row.

## Processes

- Started: the gate jobs `j0000000094`–`j0000000109` (first round) and
  `j0000000120`, `j0000000124` (the re-gate), each submitted to the
  queue's hub with `herdctl run` and each ended with its own verdict.
  Nothing was killed by name and no signal was sent to any PID of the
  queue's runner, this project's hub or the consumer's. One `cat` of
  mine blocked on stdin in a malformed shell command and was killed by
  its **own** PID, `1284379`, read back from `ps -eo pid,args` before
  the signal and gone after it; it had written nothing.
- **The queue was waited on, never worked around.** The re-gate's suite
  job sat behind other agents on the single build slot for several
  minutes; it was not resubmitted, and each submission carried its own
  `CARGO_TARGET_DIR` so that no two were coalesced.
- Build trees, both deleted:
  `/mnt/ssd_980/herd/build/m12-p6-no-holder-named`, **debug with tests,
  17 GiB**, and `…-no-holder-named-regate`, **debug with tests, 13 GiB**
  (`du -sh`, profile named before the number).
- **Two measurements for the `build-weight-off-root` gotcha, and they
  bracket its own figure.** It says *"a debug tree with tests is
  ~12–14 GB"* (seven agents, 2026-10-30). 13 GiB is inside that; 17 GiB
  is above it, and the difference between the two trees is what each had
  seen — the 17 GiB one carried a workspace build, clippy over all
  targets **and three separate suite runs**, the 13 GiB one one of each.
  So the gotcha's range is right for a tree that has been gated once,
  and a tree that has been iterated in runs ~30 % heavier. Not a
  contradiction; a second reading that says what the spread is.
- Worktrees: `…/scratchpad/m12-p6` (this branch) and
  `…/scratchpad/conflict-probe`, the merge probe, removed the moment it
  had answered.
