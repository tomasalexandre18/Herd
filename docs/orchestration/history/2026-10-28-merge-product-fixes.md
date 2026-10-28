# merge-product-fixes — handoff 2026-10-28

- **Item:** ORCH-124 (integrated, `done`); the second branch had no board
  item of its own and leaves D186, ORCH-128
- **Agent:** herd-merger
- **Branch / commit:** `merge-product-fixes` / `74c28a1` plus this handoff
  commit, based on `origin/main` `bc83717`
- **PR:** see *Gates and PR* below

## Done

A two-branch product batch, merged in the order briefed, one PR for the
batch (method in force today: one PR per batch, CI green, Evidence
filled, ORCH-001).

1. `orch-124-output-removed-readmit` (`6476202`) — the hub re-admits an
   evicted output collection to the budget when its bytes land again.
2. `real-herd-patience-timeouts` (`ee8dec0`) — three `real_herd` fixtures
   declare the job slots they need.

Both were already based on `origin/main` `bc83717`, which is still the
tip, so no rebase was needed and neither branch was rewritten.

### Comparison with `origin/main`, before merging

| branch | files | nature |
|---|---|---|
| `orch-124-…` | `herd-hub/src/store.rs` (+149), `herd-hub/src/store/ordering_oracle.rs` (+37/−5), its handoff | product + test + oracle |
| `real-herd-…` | `herd-hub/tests/{killed_hub,owner_control,real_herd/mod}.rs` (+54/−1), its handoff | tests only, no product code |

**Conflicts expected: none, and none occurred.** The two branches touch
`herd-hub` in disjoint places — one in `src/`, the other in `tests/` —
and their handoffs are differently named files in
`docs/orchestration/history/`. Both merges were `--no-ff` and reported
`Merge made by the 'ort' strategy` with no conflict path.

**Conflict log: empty.** Nothing was resolved, so nothing is recorded
here; no specification or invariant question arose from a conflict.
`orch-125-state-root-report`, named in the brief as another agent's work
on `herd-hub`, has no commit of its own yet (its worktree is at
`bc83717`) and was not touched.

### Decisions allocated

Highest on `origin/main` was **D184**. **`gh pr list` returned an empty
list: no PR was open at this allocation**, so no number was held by one.
Gaps **D143, D154, D170** stay (D148); D170 was held by PR #67, closed
unmerged as obsolete (recorded in D183's own number note), so it does
not return to the pool.

- **D185 — Bytes landing again re-admit an evicted collection.**
- **D186 — A test declares the job slots its runner needs.**

**D185 is not the text the branch proposed, and the difference is the
point.** The proposal said the rejected alternative was "refusing the
write, which loses output without telling the requester". That reads as
false against the code: `files_may_land`
(`herd-hub/src/server.rs:4835`) already refuses a chunk for a marked
collection, citing D152 — I read it before writing. The branch's own
author says so in their findings 1 and 2. So D185 records instead:

- the reachable path is the **unlocked-read race**, not a runner
  re-sending (`files_may_land` reads the record with no lock,
  `write_output_chunk` reads it again and locks only inside
  `held_more`);
- **both layers are kept deliberately** (Alexa, 2026-10-28) — the server
  refuses, the store would re-admit — as defence in depth over a budget
  whose failure mode is silent and permanent, not a half-applied
  decision;
- it is **not** the explanation for the consumer's disk: they confirmed
  2026-10-28 they have never seen the "nothing more may be removed"
  line, which is consistent with the race being narrow.

**D186 keeps both dead hypotheses**, named as refuted with the evidence:
memory throttling and an IO stall (there was no pressure), plus the
third premise that the three tests pass when run alone (they do not —
each reproduces at `--test-threads=1 --exact`). They are not edited out.

### Board

- **ORCH-124 → `done`**, and corrected: the row proposed a `removed`
  check that **refuses** the chunk, which is not what landed, and the
  row's opening premise ("a resend … refills `files/`") is wrong for the
  reason above. Both are stated in the row rather than quietly rewritten.
- **ORCH-111** gains the three new samples (7/14, 5/11, 0/5 — **12 red
  in 30**), the exact split the brief reported, **and the counter-sample
  I measured** (see *Risks and findings* 2). Its stale line reference
  `:438` is corrected to `:464`, and the evidence cell to `:455-466`.
- **ORCH-126 (new, owner unassigned, `open`)** — what a runner lends is
  read once at startup and never revised, so a runner that starts under
  a full page cache lends a fraction of its machine for its whole life
  and nobody is told the figure is a snapshot. Measurement in the row:
  slice at **13.31 GiB → room 4.69 GiB → 1 slot**; an hour later
  **11.75 GiB → 6.25 GiB → 2 slots**; tipping point the slice at
  **13 GiB**. The *once* is deliberate (D119, D79, E-32) and is not the
  finding; the silence is. Two shapes are set out, and the re-read one is
  flagged as a specification change for Alexa.
- **ORCH-127 (new, `open`)** — the two comments D185 lands still narrate
  the refused resend path as the motivation. Prose only. **Not fixed at
  integration on purpose:** rewriting an author's committed reasoning is
  not the merger's, so it is recorded instead.
- **ORCH-128 (new, `open`)** — the class D186 leaves open: any
  `real_herd` test that comes to need two concurrent jobs inherits the
  same silent dependency.
- **ORCH-125 is deliberately skipped**, held by the branch
  `orch-125-state-root-report`.
- **ORCH-120 and everything about the method were not touched**, as
  briefed; none of the five `method-*` branches was read or merged.

### Status

`docs/status.md` restated to 2026-10-28, **90 lines** (the limit): 247
commits past v0.3.7, the suite figures below, D185 added as a ninth
defect in no release (thirteen changes), and ORCH-124 replaced by
ORCH-126 in *what the software still does not do*. To stay inside 90
lines I dropped two sentences that were state no longer: ORCH-069 and
ORCH-107 being `done` (they are board rows). Both handoffs are present
in `docs/orchestration/history/`.

## Not done, and why

- **`files_may_land` is unchanged.** Alexa's decision of 2026-10-28
  keeps both layers, so there is nothing owed here; it is recorded in
  D185 and ORCH-124 rather than left as an open question.
- **ORCH-127 not applied.** The merger does not rewrite an author's
  committed comments outside a conflict.
- **No tag, no release, no deployment, nothing on the consumer's
  instance** (D161). No force-push, no `--no-verify`, no `--admin`.

## Gates

All five, on the integration tip **after** the board and decision
commit, **before** the PR (ORCH-121). Tip `74c28a1`, host one-minute
load **1.19**. Every gate's output was written to a file and the file
read whole; no `tail` was used to judge a gate.

```text
TIP: 74c28a11ba29da81fd7acec5cbbb9d3d10a5396a  LOAD: 1.19 2.27 3.08

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.61s
build exit=0

$ nix develop --command cargo fmt --all --check
fmt exit=0            (no output)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.98s
clippy exit=0

$ nix develop --command ./scripts/test.sh
839 passed, 0 failed  (55 `test result:` lines, 0 of them FAILED)
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
test.sh exit=0 wall=183s

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check-docs exit=0
```

The six tests this batch bears on, all `ok`:

```text
test store::tests::bytes_written_after_an_eviction_are_swept_again ... ok
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test a_result_the_hub_cannot_write_is_kept_until_it_can ... ok
test a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it ... ok
test a_drain_first_reservation_survives_a_kill_nine_on_either_side ... ok
```

The last three are the three that spent 30 s each before `ee8dec0`.

**Skips: 12, each a cgroup-v2 delegation refusal on this host.** Printed
as skips and never counted as passes (D27). Named:

```text
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

**`check-orchestration.sh` (CI's `docs` job, not one of the five) was
run twice here and was green both times**, 146 passed 0 failed — see
*Risks and findings* 2.

This handoff is a docs file, so `check-docs.sh` is re-run after it is
committed and before the push; that run's output is in the PR. The four
Rust gates above ran on `74c28a1`, which differs from the pushed head
only by this Markdown file, and CI re-runs all of them on the PR head.

## Risks and findings

1. **The brief's skip count was right about rest and wrong about my
   first run — both figures are reported.** It said to expect 12 skips,
   every one a cgroup refusal. The **evidence run gives exactly that**.
   An earlier measurement run of mine, taken while the host was at
   one-minute load **13.62** (other agents compiling), gave **14**: the
   same twelve plus `flood-fifty` and `flood-two-fifty`, which are
   load-gated at a ceiling of 8 and are **not** cgroup refusals. That is
   the documented 12-at-rest/14-under-load split (D27, ORCH-070,
   ORCH-107) observed twice in one day, and `status.md` now says so.
   Nothing was hidden: I ran the suite three times in all and the last,
   on the pushed tip, is the evidence.

2. **The brief's "exact split" for ORCH-111 did not reproduce, and this
   is a correction.** The brief reported that across 30 runs every red
   one was a shell invocation that also wrote files and every green one
   ran the script alone. I ran the discriminating experiment on the
   integration tip: once with the script alone, once deliberately inside
   a file-writing compound. **Both green, 146 passed 0 failed.** So
   writing files is **not sufficient** to make the case red. The
   mechanism is still the best lead and the rate still ~1 in 3 over
   those 30 runs, but "exact" is too strong, and ORCH-111 now carries
   the counter-sample and asks for the experiment at N runs a side
   rather than 1. I did not weaken the brief's samples; I added mine
   beside them.

3. **ORCH-124's board row and the branch's own code comments disagree
   with the code, in the same way.** Documented in D185, in the ORCH-124
   row, and in the new ORCH-127. The code is right; the prose around it
   describes a path `files_may_land` refuses.

4. **D185's test models the race at store level, not through the link.**
   Two `write_output_chunk` calls is a fair model — the race is precisely
   a second write arriving against a marked record — but it is not an
   end-to-end proof, and the branch explains why one is impossible while
   `files_may_land` stands. Worth knowing before anyone reads the test as
   covering a runner-driven path.

5. **`outputs.first_fetched_at` survives a re-admission**, so a
   collection fetched before the first eviction keeps "fetched" and is
   ranked for eviction before an unfetched one. The author flagged it as
   a judgement and left it; I did not change it, and it is not in a
   board row. Raising it here so it is not lost.

6. **Three suite runs on a shared host.** Two were forced by ORCH-121:
   each time I corrected a docs figure the tree changed, so the evidence
   had to be re-taken. Worth noting as a cost of the rule, not a
   complaint about it; the alternative is quoting a gate for a tree that
   was not pushed, which is the defect ORCH-121 exists for.

## Decisions needed

**None for this batch.** Alexa's decision of 2026-10-28 (both layers)
and ORCH-001 cover everything here. Two things are parked as board rows
and neither blocks:

- **ORCH-126**, shape (a) one sentence in the runner's startup report
  versus (b) re-reading the room. **(b) is a specification change** —
  it reopens E-32 and D79 — and is Alexa's, not an agent's.
  *Recommendation:* (a), as its own task.
- **ORCH-128**, whether every `real_herd` runner should declare the cap.
  *Recommendation:* no, not now, as D186 records.

## Resume point

Nothing is pending on this branch. The merge state, the PR number, the
CI run and the merged sha are in the PR and in the report to the
orchestrator. Next, independent of this batch:

```sh
# ORCH-111's discriminating experiment, N runs a side
for i in $(seq 1 10); do ./scripts/check-orchestration.sh > alone-$i.log 2>&1; done
for i in $(seq 1 10); do echo x > w-$i.txt && ./scripts/check-orchestration.sh > comp-$i.log 2>&1; done
```

## Processes

Every command was a foreground `nix develop --command …` or a shell
builtin in this agent's own worktree,
`…/scratchpad/merge-product-fixes/wt`. No background process was
started. **No PID was killed; no `pkill`, no `killall`, no kill by
name.** The hubs, runners and `herdctl` processes the suite spawns were
each reaped by the harness's `kill_on_drop`. One compile at a time
throughout. Nothing was written in `/mnt/ssd_980/herd-project`, in
another agent's worktree, or anywhere outside this worktree and this
session's scratchpad.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/merge-product-fixes`, outside
the worktree, is deleted immediately after the push; the gate logs under
`…/scratchpad/gates-tip/` are kept until this handoff is accepted.
