# merge-batch-3 — handoff 2026-10-29

- **Item:** batch 3, the third local landing. ORCH-119 amended and still
  `done`; **ORCH-159** and **ORCH-160** opened; **D208** and **D209**
  allocated. ORCH-155 and ORCH-152 carried forward, untouched.
- **Agent:** herd-merger
- **Branch / commit:** integration branch `batch-3-integration`, off
  `origin/main` `b73d38c`; this handoff is its tip. Its worktree, created by
  this agent under its own session scratchpad (ORCH-031):

  ```text
  /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-3
  ```
- **Landing:** this one. **Two branches, one `--no-ff` merge commit whose
  message is the record below, one push, one post-push `ci.yml` run on
  `main`.** The record is copied whole under *The landing record* and is that
  commit's message.

## Done

### 1. The batch, listed before anything was planned

Fetched, then compared with `origin/main` `b73d38c`. Measured with
`git merge-base --is-ancestor` rather than taken from the brief:

```text
orch-119-clockless-oracle  fd9500c  merge-base b73d38c  origin/main IS ancestor
joining-mesh-residue       820823e  merge-base 09229db  origin/main is NOT ancestor
```

`joining-mesh-residue` is cut from `09229db`, which is batch 2's base, so it
is **one batch behind**. That is not a conflict — its two files are disjoint
from everything batch 2 touched — but it is what makes the board check below
say something, and it is why the check was run.

Neither branch is an ancestor of the other.

**Files touched, per branch, against `origin/main`:**

| branch | files | what | decisions | conflicts expected |
|---|---|---|---|---|
| `orch-119-clockless-oracle` | 2 | `herd-hub/tests/announced_job_ceiling.rs` (+138/−26); its handoff | none in the tree; one proposed in prose | none |
| `joining-mesh-residue` | 2 | `docs/joining.md` (+76); its handoff | one `D-NEXT` proposed in its handoff | none |

**Neither branch touches `docs/decisions.md`, `docs/orchestration/board.md`
or `docs/status.md`**, so every entry and every row below is
`herd-merger`'s, written from the two handoffs' proposed text and from the
orchestrator's brief. **No `D-NEXT` header exists in either branch's
`docs/decisions.md`** — checked with `git grep D-NEXT` over both branches'
whole `docs/` tree; every hit is prose about the mechanism. The proposals
live in the handoffs.

**Merge order, and why it was that one.** `orch-119-clockless-oracle` first:
it is the branch that should take `main` out of its red streak, and it is the
only one carrying Rust. The residue second, as prose. Two `--no-ff` merges,
one at a time onto one integration branch, so a later red run is
attributable to a known merge commit and not to a blur:

1. `orch-119-clockless-oracle` `fd9500c` → merge commit `1a4fb9b`
2. `joining-mesh-residue` `820823e` → merge commit `1fab8f6`

Then `6ce54c5`, the docs allocation, and this file as the tip.

**Not carried, deliberately:** `orch-119-announced-ceiling` `7bb6578` is
already in `b73d38c` as batch 2. The branch is left alone; deleting it is not
this task.

### 2. The conflict log

**Empty, and measured rather than assumed.** Each merge reported zero
unmerged paths from `git diff --name-only --diff-filter=U`, and no file was
resolved by hand. The two branches' file sets are disjoint from each other
and from the eighteen files batch 2 changed between `09229db` and `b73d38c`;
`docs/joining.md` in particular was not touched by batch 2, which is the one
overlap a branch cut from `09229db` could have had.

Nothing in this landing changed a specification or an invariant by
resolution, so nothing was escalated under step 2.

### 3. The board check, both ways

`workflow.md` *One landing per batch* says a batch with no conflict needs
this most. Run on the merged tree **before** `herd-merger` added anything of
its own.

**Every branch's own `board.md` line, verbatim, in the merged file.**

- `orch-119-clockless-oracle`: 352 lines, **0** not present verbatim.
- `joining-mesh-residue`: 318 lines, **20** not present verbatim.

**The merged file against each branch.** Byte identical to
`orch-119-clockless-oracle`'s, and 54 insertions / 20 deletions against
`joining-mesh-residue`'s. Both answers are the right one, and the second
needed the second direction to show it: the residue branch **did not touch
`board.md`** (its copy is byte identical to `09229db`'s), so its 20 absent
lines are exactly the 20 lines batch 2 replaced. Counted both ways and
equal — 20 lines of `09229db`'s board are absent from the merged board, and
20 of the residue branch's lines are absent from it. The merged `board.md`
is byte identical to `origin/main`'s, which is what a branch that did not
edit the board must produce. A one-way check would have read those 20 as a
loss.

**Handoffs:** both branches left theirs under
`docs/orchestration/history/`, dated 2026-10-29 —
`2026-10-29-orch-119-clockless-oracle.md` and
`2026-10-29-joining-mesh-residue.md`.

### 4. The decisions allocated

Measured, not taken from the brief: the highest number on `origin/main` is
**D207**, and the highest on **any** of the 31 branches not yet in
`origin/main` is also D207 (`orch-119-clockless-oracle`'s own copy). So the
next free number is **D208**, which agrees with the brief.

- **D208** — *A bound test's premise is built, and the runner's word.* From
  `orch-119-clockless-oracle`, which proposed no `D-NEXT` itself. Written to
  govern the next such test and not only this one: four numbered rules, the
  third of which keeps `Assigned` where the hub's view genuinely is the
  quantity under test, and the fourth of which forbids widening the timeout
  instead.
- **D209** — *Pack transfer: we authorise, the operator encrypts.* From
  `joining-mesh-residue`, whose handoff proposed the text as `D-NEXT`.

**Judgement asked for in the brief: the residue's section earns its own
number and does not ride the row.** It is an owner's decision — Alexa's, of
2026-10-29 — about what the product protects, taken before a single pack
frame was numbered, and the residue it knowingly accepts (pack bytes in
clear on a mesh that does not encrypt, a capability replayable while it
lives) is exactly the kind of thing a later reader has to be able to reach
by number rather than by finding a board row. The board row that remains,
ORCH-159, is the *work* M11 owes; D209 is the *property*.

Gaps stay (D148, invariant 8): **D143, D154 and D170 are untouched.** Both
entries were appended at the end of the file; nothing was renumbered and no
existing entry was edited. The two headers were each shortened once, before
any commit, because `check-docs.sh` found them at 85 and 86 columns against
the 79-column rule.

### 5. The board rows

- **ORCH-119** — state unchanged, `done`. Amended in its *next action* and
  *evidence* cells with the third sighting and the second fix. It records
  that run **`40857453150`** on `b73d38c` was the **third consecutive red**
  run on `main` (`40720983497` on `7eb9585`, `40762764493` on `09229db`,
  then this one); that this red was a **different assertion** from the one
  D195 fixed — **1156, not 1179**; and that the fix **removes the regime**
  rather than widening a timeout, with `CEILING`, `THE_OUTAGE` and
  `SLEEP_ACROSS_THE_OUTAGE` all untouched.
- **ORCH-159** — new, `open`, in *Open*. What M11 must satisfy now that
  `joining.md` states it, and the correction of the "nothing on your machine
  listens" bullet in the same change. **Reallocated**, as the brief
  instructed and as the residue's own handoff asked for: that handoff
  proposed ORCH-146, and ORCH-146 is the retention preview's.
- **ORCH-160** — new, `open`, in *Open*. The `JobState::Assigned` survey,
  carrying the orchestrator's first pass verbatim: the ten string-form
  waiters in `herd-hub/tests/owner_control.rs` at lines 315, 365, 651, 674,
  979, 1026, 1163, 1211, 1377 and 1552; the two `kill -9` tests at :1525 and
  :1619; and **:1552 named as the one that reads worst**, immediately before
  the hub is killed at :1569. The row says in terms that it is **a survey and
  not a sweep**, that it is deliberately not adjudicated, and that
  `announced_job_ceiling.rs:240`'s remaining `Assigned`-or-`Running` is
  **correct and stays**, because it measures a peak of attempts in the air
  and nothing cuts a link after it.
- **Carried forward, not closed:** ORCH-155 stays `owed`, ORCH-152 stays
  `open`. Neither is this landing's to touch, and no row was dropped because
  a session ended.
- **Batch 2's judgement stands**, confirmed by the orchestrator and changed
  in no way here: ORCH-104's N2 as ORCH-157, N3 as ORCH-158, N1 folded into
  ORCH-123 as an amendment rather than a new id.

### 6. One repair to `board.md` nobody asked for, and why it is reported

Batch 2's `perl` without `use utf8` double-encoded **41 em dashes** in
`docs/orchestration/board.md` — not five characters, and all of them one
character: `c3 a2 c2 80 c2 94` where `e2 80 94` belongs. `check-docs.sh`
accepts them, because they are valid UTF-8 of the correct width.

All 41 are repaired, byte for byte: **123 bytes removed** (41 × 3), 22 lines
touched, inode and mode unchanged, `git diff --numstat` 22/22, and nothing
else in the file altered. Written in place (`perl` to a temporary file, then
`cat` back over the original) so that the mode is not dropped the way
`awk`+`mv` once dropped `check-orchestration.sh`'s `100755` and sent
`gate.sh` step 2 to exit 126.

**The grep being recommended for this does not catch it, and that is the
finding worth keeping.** A double-encoded *three-byte* character begins
`c3 a2`, which renders `â` — not `Ã` and not `Â`. So `grep -rnE 'Ã|Â'` finds
a mangled `§` (two bytes, `c2 a7` → `Â§`) and is blind to a mangled `—`,
`≤` or `→`. The detector that works looks for a C3- or C2-led byte
immediately followed by another:

```text
perl -e 'open(my $H,"<:raw",$ARGV[0]); local $/; my $c=<$H>;
  my $n=()= $c =~ /\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]/g; print "$n\n";' FILE
```

It reports **zero** across every tracked `.md` file of this tree. The two
surviving `Ã|Â` hits are the previous landing's own handoff quoting that
grep as text, and the `§§` in `docs/remediation/09-runner-self-limits.md` is
a real pair of section signs, not damage. **History under `history/` was not
rewritten**; the board is the live list, the handoffs are append-only.

### 7. `docs/status.md`, at 90 of 90 with no headroom

Two edits, and the first paid for the second.

- **A duplicated bullet removed.** Lines 65-68 carried the same sentence
  twice — *"(**D202**). `r_file` is still unmeasured (ORCH-145), which is
  why …"* — once ending "ORCH-156 ordering matters to them" and once
  "the ordering constraint of ORCH-156 matters to them". Collapsed to one.
  **Two lines freed.**
- **The paragraph about `main` being red was stale and is restated.** It
  said *"`main` has been red on its post-push runs since `7eb9585`, always
  `announced_job_ceiling.rs:1179`; the fix lands in this batch (D195)"* —
  written for batch 2, and now wrong twice over: D195 landed, and the third
  red was at **1156**. It now names all three runs with their shas, which
  assertion each was, and D208. **Two lines spent.**

Net zero: `docs/status.md` is **90 lines of 90**. `CLAUDE.md` is untouched at
**150 of 150**.

## Not done, and why

- **No product change, by either branch or by me.** Nothing in `herd-proto`,
  `herd-hub/src`, `herd-runner` or `herdctl` is touched by this batch.
  `PROTOCOL_VERSION` stays 2, `RECORD_VERSION` 1, `LAYOUT_VERSION` 1.
- **The `JobState::Assigned` survey is not done.** The first pass is real and
  is in ORCH-160 with its coordinates; the adjudication is the task, and two
  of the ten sites say in their own comments that they wait for nothing
  before the kill on purpose, so some may be sound.
- **ORCH-126's two open decisions are untouched** — the "as full as the
  ceiling they announced" wording, and whether `ceiling_refusals()` should
  count both arms.
- **No tag, no release, no deployment, no pull request**, nothing on the
  consumer's instance (D161), and no second push for this batch.
- **`orch-119-announced-ceiling` is not deleted.** Left alone, as instructed.

## Gates

- **Gate:** green — `./scripts/gate.sh` on the integration tip off `6ce54c5`,
  and that tip is this merge's **second parent**. **The gated
  sha itself cannot be in this file**, because it is the sha of the commit
  that adds this file; no text here can name it. The rule is met the only
  way it can be: the gated tree differs from `6ce54c5` by this one file, the
  merge commit's message carries the gated sha in full, and
  `git rev-parse HEAD^2` on that merge recovers it exactly. Receipt under
  `target/herd-gate/` of the integration worktree named above; the verdict
  line verbatim is in `herd-merger`'s report and not here (ORCH-121).

**One `./scripts/gate.sh` run, on a clean tree, as the last thing before the
merge.** It gates the **integration tip — this handoff's own commit, which
is the merge commit's second parent** — and not the merge commit, whose
message is the landing record and which therefore cannot exist yet.

- **The receipt it will write:** `target/herd-gate/SHA.receipt`, under the
  integration worktree named at the top of this file, where `SHA` is that
  integration tip. In full, with `SHA` the only unknown:

  ```text
  /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-3/target/herd-gate/SHA.receipt
  ```

  Its parent is `6ce54c5`, and `git rev-parse HEAD^2` run on the merge
  commit on `main` recovers `SHA` exactly.
- **The verbatim output, the verdict line and the skip list are in
  `herd-merger`'s report to the orchestrator and are deliberately not in
  this file.** This is ORCH-121 applied to the merger: the gate must run on
  the tree that is pushed, so this handoff has to be in that tree already
  and **nothing may be added to it afterwards**. **The post-push run id is
  in that report too, and not here**, for the same reason — the run cannot
  exist before the commit that triggers it, and a second push to record it
  is the one thing the method forbids.
- **What has to hold at the merge**, measured rather than claimed: a green
  receipt for `git rev-parse <merge>^2`, and `git diff --quiet` between that
  sha and the merge commit — an identical tree. A gate run on another tree
  proves nothing about this one.
- **A green gate does not travel between worktrees.** The receipt store is
  per worktree, so neither branch's own green gate is this landing's;
  `herd-merger` gates its own integration tip. No receipt was copied in.

**The skips, as a list and never a count (D27, D179).** The twelve at-rest
names of `scripts/expected-skips/local.txt` are what this host skips, every
one of them the same cgroup v2 delegation it does not grant, every one
printed as a skip and **none counted as a pass**:

```text
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces
```

Two more are possible and are **not a different count but a different
list**: `flood-fifty` and `flood-two-fifty`, the load-gated flood benches of
`scripts/expected-skips/local-under-load.txt`. Under **D196** each is
excused only if its own `HERD-SKIPPED` line names that bench's ceiling with
a figure above both that ceiling and `HERD_GATE_LOAD_NOTE` — by name **and**
by reason. Which list the gate applied, and on whose figure, is in the
verdict line in the report. Anything outside these fourteen names is RED and
`main` does not move.

**Docs gates run by `herd-merger` on this tree, before this file:**

```text
./scripts/check-docs.sh            →  docs ok  (exit 0)
./scripts/check-orchestration.sh   →  269 passed, 0 failed
```

`check-orchestration.sh` was run **twice and measured here**, not carried
over from the brief: **269 passed, 0 failed** on the merged tree before any
edit of mine, and **269 passed, 0 failed** after all of them. The figure has
not moved from `b73d38c`. `check-docs.sh` was run after each edit rather
than once at the end, which is ORCH-144's trap — a single length or width
overflow anywhere in the tree turns 13 `check-orchestration.sh` cases red at
once — and it caught two over-long decision headers while they were still
cheap to fix. The four Rust gates are not claimed here: they run inside
`gate.sh`, as steps 3 to 6.

## The landing record

The merge commit's message, in full. One thing in it cannot be identical to
the commit's own copy, and it is said rather than hidden: the `GATEDSHA`
token below is **this handoff's own commit sha**, which the merge message
carries and which this file cannot contain. Everything else is word for
word.

```text
Land batch 3: the clockless ceiling oracle and the mesh residue

- **Batch:** 3, two branches, landed 2026-10-29 by herd-merger. One --no-ff
  merge commit whose message is this record, one push, one post-push ci.yml
  run on main that is then read.
- **Gate:** green — ./scripts/gate.sh on GATEDSHA, this commit's second
  parent, with `git diff --quiet GATEDSHA HEAD` silent, so the gated tree and
  the pushed tree are identical. Receipt under target/herd-gate/ of the
  integration worktree. The verbatim output and the skip list are in
  herd-merger's report to the orchestrator: they cannot be in this tree,
  because the gate ran on this tree (ORCH-121).
- **Skips:** the twelve at-rest names of scripts/expected-skips/local.txt,
  every one of them the same cgroup v2 delegation this host does not grant,
  plus the two load-gated flood benches of local-under-load.txt if and only
  if their own skip lines excuse them by name and by reason (D196). Each is
  printed as a skip and never counted as a pass (D27, invariant 5). The list
  decides and the count does not (D179); the names are in herd-merger's
  handoff and the applied list is in the verdict line.
- **CI run:** none yet — this push starts it. The run cannot exist before the
  commit that triggers it, so its id goes to herd-merger's report now and the
  next landing's board edit records it; a second push for one batch is the one
  thing the method forbids.

## What changed and why

Two things land. Neither changes the product: one is a test, one is prose.

**The clock comes out of the ceiling bound test's premise, and main should
stop being red.** announced_job_ceiling.rs established "this runner is
holding CEILING jobs" by waiting for `JobState::Assigned | JobState::Running`
and then cutting the link. `Assigned` is written when the hub *sends*
`FromHub::AssignJob`; `Running` only on the runner's own `ToHub::JobStarted`,
from inside the lock that counts the ceiling (herd-hub/src/server.rs:4144).
Measured on an instrumented copy: the cut landed one millisecond after the
hub's second dispatch, so an assignment the runner had not yet read went down
with the link, and the machine was left holding one original or none. The
premise is now built rather than timed — the jobs are held open on a release
file, as admission.rs does, and the link is cut on the runner's word. No
timeout was widened: CEILING, THE_OUTAGE and SLEEP_ACROSS_THE_OUTAGE are
untouched, and D195's sharpened assertion at 1179 is untouched with them.

**What main can do now that it could not before:** state, as a rule and not
as one repair, that a bound test's premise is built and that occupancy is the
runner's fact to report (D208) — and say what a pack transfer protects before
a single pack frame is numbered (D209).

No milestone moves. No `E-nn` expectation is delivered or withdrawn by this
batch; D209 is a property M11 must now satisfy, carried as ORCH-159.

## Branches in this batch

In the order they were merged, one at a time onto one integration branch:

1. orch-119-clockless-oracle fd9500c — ORCH-119 — merge commit 1a4fb9b.
   Handoff: docs/orchestration/history/2026-10-29-orch-119-clockless-oracle.md
2. joining-mesh-residue 820823e — ORCH-159 (reallocated; its handoff proposed
   ORCH-146, which the retention preview already holds) — merge commit
   1fab8f6. Handoff:
   docs/orchestration/history/2026-10-29-joining-mesh-residue.md

orch-119-announced-ceiling 7bb6578 is deliberately not in this batch: it is
already in b73d38c, landed as batch 2.

## Evidence

One verdict per branch, named for it, and then the batch's own run on the
merged tree. Claiming a gate passed is not evidence.

**orch-119-clockless-oracle, the branch's own gates (its agent's, in its own
worktree, on fd9500c).** gate.sh GREEN, receipt read rather than the prose
around it: verdict GREEN, steps 0..8 all ok, skips-observed 12,
load-excused-skips none, load1 1.53 at the start and 2.07 after the suite,
suite PASSED. Before that, on the fix alone: cargo fmt clean, cargo clippy
clean, `cargo test -p herd-hub --test announced_job_ceiling` 8 passed 0
failed 1 ignored in 11.39s, check-orchestration 269 passed 0 failed,
check-docs ok. Its own red-before/green-after measurement, at one condition:
10 of 10 red at 1156 on origin/main's test, 10 of 10 green on the branch;
20 of 20 green under the load that gave the old test the formerly-red arm 11
times in 20. Regimes 2 and 3 driven on purpose against the fixed oracle:
green 5 of 5, and red 5 of 5 at 1156 with CI's own message.

**joining-mesh-residue, the branch's own gates (its agent's, on 820823e).**
check-docs.sh exit 0, "docs ok", with the standing D170-gap note. The four
Rust gates were NOT RUN on that branch and are reported as not run, never as
passed: the branch touches only docs/, which is what CLAUDE.md *Commands*
asks of a prose change, and the Rust gates for it are this landing's own
gate.sh run.

**The batch's own run, on the merged tree.** One ./scripts/gate.sh on a clean
tree, the last thing before the merge, on GATEDSHA — steps 0 to 8 in ci.yml's
own order. Its verbatim output, its verdict line and the skip list it applied
are in herd-merger's report; the receipt is named in herd-merger's handoff.
Measured by herd-merger on the merged tree before writing anything of its
own, and again after: check-orchestration.sh 269 passed, 0 failed — unmoved
from b73d38c, which is the figure the ORCH-119 agent reported and which is
here confirmed independently rather than carried over.

**Conflicts: none, and it was measured rather than assumed.** Both merges
reported zero unmerged paths from `git diff --name-only --diff-filter=U`; the
two file sets are disjoint and disjoint from everything batch 2 changed. No
file was resolved by hand, so nothing in this landing changed a
specification or an invariant by resolution.

**The board check, both ways, run on the merged tree before herd-merger
added anything.** Neither branch touches board.md. Every one of the 352 lines
of orch-119-clockless-oracle's board.md is present verbatim in the merged
file. joining-mesh-residue is one batch behind, so 20 of its 318 lines are
absent — and those 20 are exactly the 20 lines of 09229db's board that batch
2 replaced, counted both ways and equal. The merged board.md is byte
identical to origin/main's, which is the right answer: a branch that did not
edit the board must not reinstate a stale copy of it.

## Left out

- **orch-119-announced-ceiling** 7bb6578, already on main in b73d38c. The
  branch is left alone; deleting it is not this landing's task.
- **The product reporting defect**, unchanged and confirmed again on this
  branch: a `StillRunningThisJob` refusal is recorded as `refused_as_full`
  and the record then says the machine was "as full as the ceiling they
  announced" and advises "Add capacity" about a machine holding 1 of 2. It is
  wording and attribution, not loss, and it belongs to ORCH-126.
- **The JobState::Assigned survey is not done**, and is not pretended to be.
  The first pass is real and is written into ORCH-160 with its coordinates;
  the adjudication is a task, and two of the ten sites may well be sound.
- **No tag, no release, no deployment, no pull request**, and nothing on the
  consumer's instance (D161). Nothing in herd-proto, herd-hub/src,
  herd-runner or herdctl is touched by this batch at all.
- **The two open decisions from the previous ORCH-119 handoff** — the "as
  full as the ceiling they announced" wording, and whether
  `ceiling_refusals()` should count both arms — are still open, in ORCH-126.

## Decisions allocated

Allocated once for the whole batch, in merge order, above the highest number
on origin/main (D207) and on every unlanded branch (measured: the highest any
of them holds is D207). Gaps stay (D148, invariant 8): D143, D154 and D170
are untouched. Nothing was renumbered and no existing entry was edited.

- **D208** — A bound test's premise is built, and the runner's word. From
  orch-119-clockless-oracle. Worded to govern the next such test: build the
  premise on a release file and never on a sleep; wait on the word of
  whoever owns the fact, which for occupancy is `JobState::Running`; keep
  `Assigned` where the quantity under test really is the hub's own view; and
  remove the regime rather than widen the timeout. It records what is left as
  a **bounded race with its measurement, not an impossibility** — E-21 stops
  the written-off originals at the redial, so the second attempt must still
  reach `take_job` before their processes are gone, never lost in 35 measured
  runs. The claim that is true unqualified is narrower: the regime is no
  longer produced by scheduling on the dispatch path.
- **D209** — Pack transfer: we authorise, the operator encrypts. From
  joining-mesh-residue, Alexa's decision of 2026-10-29. It earns its own
  number rather than riding ORCH-159's row because it is an owner's decision
  about what the product protects, taken before any pack frame was numbered,
  and because the residue it accepts knowingly — pack bytes in clear on a
  mesh that does not encrypt, and a capability replayable while it lives — is
  the kind of thing a reader must be able to find by number.

## Board rows

- **ORCH-119** — stays `done`, amended with the third sighting and the second
  fix: run 40857453150 on b73d38c was the third consecutive red run on main
  (after 40720983497 on 7eb9585 and 40762764493 on 09229db), this red was a
  *different* assertion from the one D195 fixed (1156, not 1179), and the fix
  **removes the regime** rather than widening a timeout. The residual race is
  recorded as bounded and measured, not as impossible.
- **ORCH-159** — new, `open`. M11 must satisfy what joining.md now states
  (D209), and the "nothing on your machine listens" bullet is corrected in
  the same change. This is the row joining-mesh-residue's handoff proposed as
  ORCH-146; that id is the retention preview's, so it was reallocated here,
  which is what the handoff asked for.
- **ORCH-160** — new, `open`. The JobState::Assigned survey, with the first
  pass's coordinates in the row: ten string-form waiters in
  herd-hub/tests/owner_control.rs, the two kill -9 tests, and :1552 named as
  the one that reads worst. It records that
  announced_job_ceiling.rs:240's remaining `Assigned`-or-`Running` is
  **correct and stays**, because otherwise a later agent will "fix" it.
- **Nothing is dropped because a session ended.** ORCH-155 (the roadmap
  living only in a scratchpad file) stays `owed` and ORCH-152 stays `open`;
  neither is this landing's to close. Batch 2's placement of ORCH-104's
  findings stands unchanged on the previous merger's judgement: N2 as
  ORCH-157, N3 as ORCH-158, N1 folded into ORCH-123 as an amendment.
- **One repair, recorded because it is a change to the board nobody asked
  for.** Batch 2's perl without `use utf8` double-encoded **41 em dashes** in
  board.md, not five characters; check-docs.sh accepts them because they are
  valid UTF-8 of the correct width. All 41 are repaired here, byte for byte
  (123 bytes removed, 41 times 3, inode and mode unchanged, 22 lines touched,
  nothing else altered). **The grep that was being recommended for this does
  not catch it:** a double-encoded three-byte character such as `—` begins
  `c3 a2`, which is `â`, not `Ã` or `Â`, so `grep -rnE 'Ã|Â'` sees nothing.
  The detector that works is a search for a C3- or C2-led byte immediately
  followed by another, and it now reports zero across the tree. The two
  surviving `Ã|Â` hits are the previous landing's own handoff quoting that
  grep as text, and `§§` in docs/remediation/09-runner-self-limits.md is a
  real pair of section signs. History under history/ was not rewritten.

## CI run

none yet at the time this record was written — this push starts it. Its id
comes from `gh run list --workflow ci.yml --branch main --limit 1` and is in
herd-merger's report to the orchestrator. **This landing was judged worth
waiting on rather than only recording:** three consecutive post-push runs
have been red and whether main is green is the first thing Alexa needs, so
the conclusion is read and reported by id. A red is reported red, plainly,
and becomes an ORCH-nnn row before new work is briefed; main does not move
again for it, because a second push for one batch is what the method forbids.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc
```

## Risks and findings

1. **`main` has been red three times running, and this landing is the one
   that should end it — but one sample does not prove a clock.** D208 takes
   the clock out of the premise, and what is left is a **bounded race, not
   an impossibility**: E-21 stops the written-off originals at the redial,
   so the second attempt must still reach `take_job` before their processes
   are gone. Never lost in 35 measured runs of the fixed test, 20 of them on
   two pinned CPUs under eight spinners — a rate, not a proof. The ORCH-119
   handoff's own §3 reads as "the fix removes the regime"; its §7 is the one
   that is exactly right, and the record follows §7. **Anyone reading this
   the day it goes red again should start at the redial ordering, not at a
   timeout.**
2. **The survey in ORCH-160 is a first pass and not a verdict.** Ten sites
   named, none adjudicated, two of them documented as deliberately waiting
   for nothing. The row says so. The one claim made firmly is the negative
   one: `announced_job_ceiling.rs:240` is correct and must not be "fixed".
3. **D209 describes a protocol surface that is not numbered.** "The hub
   mints a token and pushes it to the holder over the holder's own control
   link" needs a `FromHub` variant that does not exist on either enum. The
   decision and `joining.md` are both worded as design, so neither is a
   false claim about shipped behaviour — but it is a claim M11 now has to
   satisfy, and ORCH-159 exists so that nobody has to rediscover it.
4. **`docs/joining.md`'s "nothing on your machine listens" bullet is
   half-true from the day the fetch endpoint lands.** Left alone on purpose
   by the branch, and the correction is written into ORCH-159 as part of
   M11's docs change rather than done now, which would make the document
   describe an endpoint that does not exist.
5. **`docs/status.md` has no headroom at all and neither does `CLAUDE.md`.**
   90 of 90 and 150 of 150. This landing only fitted because a duplicated
   bullet was there to remove. The next landing that needs a line in
   `status.md` has to take one out, and there is no obvious duplicate left.
6. **The mojibake detector in circulation is wrong** (section 6 above). Any
   brief or handoff that says `grep -rnE 'Ã|Â'` is the check will miss every
   mangled em dash, and this project writes em dashes everywhere.
7. **Nothing in CI was re-run by me**, no force-push, no `--no-verify`, no
   `--admin`, no tag, no release, no pull request, and one push for this
   batch.

## Resume point

`main` carries batch 3 once the push in step 8 succeeds. The next session
starts by reading **the post-push run id in `herd-merger`'s report**, and
records it in the **next** landing's board edit — not by a second push here.
If that run is red, it becomes an `ORCH-nnn` row before new work is briefed
(`workflow.md`, *When the post-push run on `main` is red*).

Open and nearest: **ORCH-160** (the survey, brief it one site at a time),
**ORCH-159** (M11's pack half against the stated property), **ORCH-155**
(the roadmap still only in a scratchpad file) and **ORCH-152**
(`docs/plan.md` against the code). None was closed by this landing.

## Processes

No build, no suite and no background job started by hand; everything heavy
ran inside `./scripts/gate.sh`, in the foreground, once. No load generator,
no spinner, no `pkill`, no `killall`, and nothing signalled by name. The only
process this agent ended is none: nothing was left running.

`CARGO_TARGET_DIR` was set outside the worktree for the gate's build; it and
this worktree's own `target/` — which holds only the gate's log, its suite
output and its receipt — are removed once the branch is on the origin, as
`workflow.md` *Concurrency and host rules* requires, and the removal is
reported with its path in `herd-merger`'s report. `/mnt/ssd_980` is the
consumer's production disk too.

Nothing was written outside this worktree and this agent's own scratchpad
subdirectory, with the single exception of step 10's `--ff-only` advance of
`/mnt/ssd_980/herd-project`, whose two preconditions were checked first and
whose outcome — the shas it moved between, or which precondition stopped
it — is in the report.
