# merge-landing-2 — handoff 2026-10-29

- **Item:** the second local landing. ORCH-104, ORCH-119, ORCH-146 and
  ORCH-147 closed; ORCH-098, ORCH-105 and ORCH-020 answered; ORCH-087
  re-labelled post-1.0; ORCH-106 taken off *Waiting*; ORCH-123, ORCH-143
  and ORCH-144 amended; ORCH-148 to ORCH-158 opened
- **Agent:** herd-merger
- **Branch / commit:** integration branch `merge-landing-2`, off
  `origin/main` `09229db`; this handoff is its tip. Its worktree, created
  by this agent under its own session scratchpad (ORCH-031):

  ```text
  /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-landing-2
  ```
- **Landing:** this one. **Four branches, one `--no-ff` merge commit whose
  message is the record below, one push, one post-push `ci.yml` run.** The
  record is copied whole under *The landing record* and is that commit's
  message.

## Done

### 1. The batch, listed before anything was planned

Fetched, then compared with `origin/main` `09229db`. **All four branches
are cut from `09229db` and are mutually independent** — verified with
`git merge-base --is-ancestor` rather than taken from the brief, which
said it believed 2-4 were and asked for the check:

```text
orch-119-announced-ceiling  7bb6578  merge-base 09229db  origin/main IS ancestor
gate-load-from-the-bench    24f2a3e  merge-base 09229db  origin/main IS ancestor
orch-146-retention-preview  bdb1d9f  merge-base 09229db  origin/main IS ancestor
orch-104-walk-cost          9a9f7ae  merge-base 09229db  origin/main IS ancestor
```

**Files touched, per branch, against `origin/main`:**

| branch | files | what |
|---|---|---|
| `orch-119-announced-ceiling` | 2 | its handoff; `herd-hub/tests/announced_job_ceiling.rs` |
| `gate-load-from-the-bench` | 5 | its handoff; `scripts/gate.sh`; `scripts/check-orchestration.sh`; `scripts/expected-skips/README.md`; `docs/status.md` |
| `orch-146-retention-preview` | 5 | its handoff; `herd-hub/src/retention.rs` (new); `herd-hub/tests/retention_preview.rs` (new); `herd-hub/src/main.rs`; `herd-hub/src/lib.rs` |
| `orch-104-walk-cost` | 3 | its handoff; `herd-hub/tests/walk_cost.rs` (new); `docs/disk-policy.md` |

**The order was forced and was kept.** `orch-119-announced-ceiling`
first, because it fixes the assertion `main` is red on **and** touches
`herd-hub/tests/announced_job_ceiling.rs`, one of three sites pinning
`RECORD_VERSION == 1` and `PROTOCOL_VERSION == 2` that a later batch must
rewrite; landing it first keeps those two jobs from colliding (and D199
has now made that later batch a real one, behind ORCH-156).

**Decisions added by the branches: none.** **Not one of the four touches
`docs/decisions.md` or `docs/orchestration/board.md`**, so every entry
and every row below is `herd-merger`'s, written from the four handoffs'
proposed text and from the orchestrator's brief. `docs/status.md` is
touched by exactly one branch, `gate-load-from-the-bench`.

**Conflicts expected: none**, and that was the prediction before merging,
because the four file sets are disjoint. Four `--no-ff` merges, one at a
time onto one integration branch, so a later red run is attributable to a
known merge commit:

1. `orch-119-announced-ceiling` `7bb6578` → merge commit `dde9d80`
2. `gate-load-from-the-bench` `24f2a3e` → merge commit `9c9ef18`
3. `orch-146-retention-preview` `bdb1d9f` → merge commit `b049568`
4. `orch-104-walk-cost` `9a9f7ae` → merge commit `e5c9476`

### 2. The conflict log

**Empty, and measured rather than assumed.** Each merge reported
`conflicts: 0` from `git diff --name-only --diff-filter=U`, and no file
was resolved by hand. Nothing in this landing changed a specification or
an invariant by resolution, so nothing was escalated under step 2.

### 3. The board check, both ways

`workflow.md` *One landing per batch* says a batch with no conflict needs
this most, so it was run on the merged tree **before** `herd-merger` added
anything of its own.

**Direction 1 — each branch's own hunks survive verbatim.** For every file
a branch touched, `git diff <branch> HEAD -- <file>` is silent:

```text
ok  orch-119-announced-ceiling : every file it touched is byte-identical to its tip
ok  gate-load-from-the-bench   : every file it touched is byte-identical to its tip
ok  orch-146-retention-preview : every file it touched is byte-identical to its tip
ok  orch-104-walk-cost         : every file it touched is byte-identical to its tip
```

**Direction 2 — the merged tree differs from each branch only by the other
branches' files.** The set `git diff --name-only <branch> HEAD` is exactly
the union of the other three branches' file sets, with nothing extra:

```text
ok  orch-119-announced-ceiling : 13 files changed, all of them other branches'
ok  gate-load-from-the-bench   : 10 files changed, all of them other branches'
ok  orch-146-retention-preview : 10 files changed, all of them other branches'
ok  orch-104-walk-cost         : 12 files changed, all of them other branches'
```

**No branch's `board.md` line exists to preserve**, because no branch
wrote one; the verbatim half of the check is therefore trivially met and
is recorded as such rather than claimed as a pass of something.

**All four handoffs are in the tree**, one per branch, which was checked
by name under `docs/orchestration/history/`:
`2026-10-28-orch-119-announced-ceiling.md`,
`2026-10-29-gate-load-from-the-bench.md`,
`2026-10-28-orch-146-retention-preview.md`,
`2026-10-28-orch-104-walk-cost.md`.

### 4. The two traps the brief named, and what was found

**`docs/status.md` carried a literal `D-NEXT`**, put there by
`gate-load-from-the-bench` because a branch may not allocate a number.
It is now **D196**. `check-docs.sh`'s `D-NEXT` rule reads only
`docs/decisions.md` headers, so nothing would have caught it; it was
replaced by hand and the file was then grepped to prove none is left.

**`docs/status.md` was at exactly 90 lines against a limit of 90**, and
this landing restates it by definition. It is **90 lines again**, and
getting there cost five passes: the first restatement came out at 112
lines, and every line of the new v1.0.0 section had to be paid for by
cutting one elsewhere. Four facts were deliberately dropped to afford it
and each is on the board instead — the `matcher.rs:421` wording, the
`check-orchestration.sh` CI skip count, the `release.yml` sentence and
`ORCH-131`'s worktree count. `CLAUDE.md` is untouched and still 150.

**And the 13-case failure shape was seen a third time, by me.** Not in
`check-docs.sh` but in the board: `perl` without `use utf8` double-encoded
five `§` characters into the rows it inserted, which `check-docs.sh`
accepted as valid UTF-8 and only a `grep` for `Ã|Â` caught. The repair is
in the tree. It is the same class as ORCH-144 — a docs-shaped defect that
the docs check does not name — and it is why the board rows were grepped
for mojibake and the file re-validated with `iconv` before committing.

### 5. The thirteen decision numbers

**D195 to D207, allocated once for the whole batch, in merge order first
and then Alexa's.** The next free number above the highest on
`origin/main` (**D194**) and above the highest on **every** unlanded
branch: all 300-odd local and remote refs were scanned for a
`docs/decisions.md` header above D194 and **none holds one**, including
the live `joining-mesh-residue`, which touches only `docs/joining.md`.
Gaps stay (D148): **D143**, **D154** and **D170** are untouched, and
`check-docs.sh` still prints its one standing note about D170.

In merge order, from the branches' own proposed text:

- **D195** — a runner's own count is not always its ceiling (ORCH-119).
- **D196** — the gate reads the load off the bench that refused.
- **D197** — a retention pass that would empty the state root refuses.
- **D198** — the record walk's measured cost replaces the derived.

Then the nine Alexa took on 2026-10-29, which no branch carried:

- **D199** — all three versions move before v1.0.0, and the ordinary
  release ships before them (ORCH-156).
- **D200** — pack bytes travel in clear on the operator's private mesh.
- **D201** — retention ships active, merger 90 days and the rest 14.
- **D202** — the output floor stays, the hub stops promising it.
- **D203** — GitHub is storage; branch protection is not pursued.
- **D204** — "stable" means v1.0.0.
- **D205** — M11's pack half is inside v1.0.0: 44 of 44.
- **D206** — M17 is outside v1.0.0, E-49 and E-50 unmet by choice.
- **D207** — E-41's scheduler keeps D106; the predicate moves out.

`./scripts/check-docs.sh` says `docs ok` with no duplicate and no
malformed header.

### 6. The board and the status

`docs/orchestration/board.md`: nine rows amended in place, one moved
between sections, thirteen added, and *Next* item 9 closed with item 10
written. The six sections keep their order and every row has its eight
cells; `check-docs.sh` checks both. Details are in *Board rows* of the
record below.

`docs/status.md` is restated to this landing at 90 lines, with a new
section — *What v1.0.0 means, now that it is defined* — that the file has
never had, because until D204 the project had no definition to state.

### 7. Housekeeping

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/landing-2`, outside this
worktree and off the root filesystem, and is deleted once `main` has
moved. `./scripts/bench-preflight.sh` ran before the heavy phase. The
batch's five worktrees are removed after the push, and the orchestrator's
checkout is fast-forwarded last of all, with its two preconditions
checked and the outcome printed either way.

## Not done, and why

- **No pull request, no tag, no release.** This landing is a push to
  `main` under ORCH-001 and nothing else. The ordinary release is the
  orchestrator's next question to Alexa, and **ORCH-156** says it ships
  at layout 1 and record 1.
- **Nothing of the version triple was started.** D199 is recorded, and
  ORCH-156 exists precisely so that no brief takes it before the ordinary
  release. Not one line of `PROTOCOL_VERSION`, `RECORD_VERSION` or
  `LAYOUT_VERSION` moved here.
- **`joining-mesh-residue` is not in this batch**, by the brief. D200 is
  recorded here because a decision of Alexa's that no branch carries is
  the merger's to write down; the prose that states it where an operator
  will read it lands on that branch, in `docs/joining.md`.
- **ORCH-143 was not folded in.** Its case text is written and tested and
  would move `check-orchestration.sh` to 270, but no branch in this batch
  touches `pre-push`, and a merger adding an untested case to the gate it
  is about to run is the one thing the method forbids. Its row says so.
- **ORCH-104's ORCH-N1, N2 and N3 were placed although the brief did not
  list them.** N1 is an amendment to **ORCH-123**'s row, not a new id; N2
  and N3 are **ORCH-157** and **ORCH-158**. They come from a branch in
  this batch and would otherwise be lost with it, which `CLAUDE.md`
  forbids — but they are the orchestrator's to remove if that judgement
  was mine to make and I made it wrongly. Flagged in the report.
- **`docs/plan.md` and `docs/coverage.md` are untouched.** D205, D206 and
  D207 each make a part of them false, and rewriting `plan.md` needs a
  reviewer: that is **ORCH-155** and **ORCH-152**, the next batch's work.
- **No product behaviour changed by the merger.** Every Rust line in this
  landing came from a branch.

## Gates

- **Gate:** green — `./scripts/gate.sh` on the tip whose parent is `5f3b92d`,
  and that tip is this merge's **second parent**. **The gated sha itself
  cannot be in this file**, because it is the sha of the commit that adds
  this file; no text here can name it. The rule is met the only way it can
  be: the gated tree differs from `5f3b92d` by this one file, the merge
  commit's message carries the gated sha in full, and `git rev-parse HEAD^2`
  on that merge recovers it exactly. Receipt under `target/herd-gate/` of
  the integration worktree; the verdict line verbatim is in `herd-merger`'s
  report and not here (ORCH-121).

**One `./scripts/gate.sh` run, on a clean tree, as the last thing before
the merge.** It gates the **integration tip — this handoff's own commit,
which is the merge commit's second parent** — and not the merge commit,
whose message is this record and which therefore cannot exist yet.

- **The receipt it will write:** `target/herd-gate/SHA.receipt` in the
  integration worktree named above, where `SHA` is that integration tip.
  Its parent is `5f3b92d`, and `git rev-parse HEAD^2` run on the merge
  commit on `main` recovers the gated sha exactly; the merge commit's
  message carries it in full.
- **The verdict line, verbatim, and the gate's whole output are in
  `herd-merger`'s report to the orchestrator and are deliberately not in
  this file.** This is ORCH-121 applied to the merger: the gate must run
  on the tree that is pushed, so this handoff has to be in that tree
  already and nothing may be added to it afterwards.
- **What has to hold at the merge**, and what is measured rather than
  claimed: a green receipt for `git rev-parse <merge>^2`, and
  `git diff --quiet` between that sha and the merge commit — an identical
  tree. A gate run on another tree proves nothing about this one.

**The skips, as a list and never a count (D27, D179).** The twelve names
of `scripts/expected-skips/local.txt` are what this host skips at rest,
every one of them the same cgroup v2 delegation refusal, every one printed
as a skip and **none counted as a pass**:

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

Two more are possible and are **not** a different count but a different
list: `flood-fifty` and `flood-two-fifty`, the load-gated flood benches of
`scripts/expected-skips/local-under-load.txt`. Under **D196**, landing in
this very batch, each is excused only if its own `HERD-SKIPPED` line names
this bench's ceiling with a figure above both that ceiling and
`HERD_GATE_LOAD_NOTE` — by name **and** by reason. Which of the two lists
the gate applied, and on whose figure, is in the verdict line in the
report. Anything outside these fourteen names is RED and `main` does not
move.

**Prose-only gates already run, on the committed tree before this file:**
`./scripts/check-docs.sh` → `docs ok`, with its one standing note that
D170 is a gap. The four Rust gates are not claimed here: they run inside
`gate.sh`, as steps 3 to 6, and this batch carries real Rust from three
branches.

## The landing record

The merge commit's message, in full. One thing in it cannot be identical
to the commit's own copy, and it is said rather than hidden: the
`GATEDSHA` token below is **this handoff's own commit sha**, which the
merge message carries and which this file cannot contain. Everything else
is word for word.

```text
Land batch 2: the ceiling fix, the gate's load rule, retention-preview

- **Batch:** 2, four branches, landed 2026-10-29 by herd-merger. One --no-ff
  merge, one push, one post-push ci.yml run that is then read.
- **Gate:** green — ./scripts/gate.sh on this commit's second parent,
  GATEDSHA,
  with `git diff --quiet` between that sha and this commit silent, and its
  receipt under target/herd-gate/ of the integration worktree. The verbatim
  output is in herd-merger's report to the orchestrator: it cannot be in
  this tree, because the gate ran on this tree (ORCH-121).
- **Skips:** the twelve names of scripts/expected-skips/local.txt, all one
  cgroup v2 delegation refusal, plus the two load-gated flood benches if and
  only if their own skip lines excuse them by name and by reason (D196). Each
  is printed as a skip and never counted as a pass (D27, invariant 5); the
  list is named in herd-merger's handoff and the applied list is in the
  verdict line.
- **CI run:** none yet — this push starts it. The run cannot exist before the
  commit that triggers it, so its id goes to herd-merger's report now and the
  next landing's board edit records it; a second push for one batch is the one
  thing the method forbids.

## What changed and why

Three things land, and nine decisions of Alexa's are written down.

**`main` stops being red, and for a stated reason rather than a re-run.**
announced_job_ceiling.rs:1179 pinned one of the two refusals that set the
`holding` field, and the other one — a machine still running another attempt
of that same job, D182 — reports a figure below the ceiling by construction.
The assertion now reads the figure against the record's own sentence, which
holds in both arms, and the ceiling arm is still held to == CEILING exactly.
Reproduced at 13 failures in 20 under load and 0 in 35 unconstrained; 0 in 20
after the fix, with 13 of those 20 taking the formerly-red arm. That is D195.

**The gate stops guessing why a bench skipped.** It sampled /proc/loadavg
twice and allowed a two-name delta list when either figure was over 8; a load
burst that begins and ends inside the suite is invisible to both, so two
correct skips were called unexpected and a clean tree went red. The same rule
was wrong the other way and nobody had noticed: a busy host excused every
delta name whatever the reason, so a flood bench that skipped because it could
not bind a port read GREEN — an assertion nobody checked, reported as
expected, which is invariant 5 broken inside the gate. The excuse now comes
out of the refusing bench's own skip line, scoped by name and by reason.
check-orchestration.sh goes 256 to 269. That is D196.

**M12 gets the task that has to exist before its engine.**
`herd-hub retention-preview` answers what record retention would remove and
what it would cost, and removes nothing: no deletion path and no flag that
could be given one. Four spans, required, with compulsory units and no
default — D194's refusal to print a threshold, applied. Two threshold-free
refusals, one of which refutes the rule the planner proposed. That is D197,
and with it ORCH-146.

**And the figures M12 is sized against are measured now, not derived.** The
plan was wrong in both directions: the full record walk is 3.29 s cold at
33 340 directories, which had never been measured at all, while the narrow
sweep pass is a third of its derived figure. Reading in ext4 hash order costs
double cold against id order, which is a bigger saving than the parse fix
anyone was counting on. That is D198.

M12 is moved, not completed: E-15 has its preview and not its engine. No
milestone is finished by this landing and no E-nn becomes met.

## Branches in this batch

In the order they were merged, all four cut from origin/main 09229db and
verified mutually independent with git merge-base --is-ancestor:

1. `orch-119-announced-ceiling` — 7bb6578, ORCH-119. Merge commit dde9d80.
   Handoff: docs/orchestration/history/2026-10-28-orch-119-announced-ceiling.md
   First, and not by preference: it fixes the assertion main is red on and it
   touches one of the three sites pinning RECORD_VERSION == 1 and
   PROTOCOL_VERSION == 2 that a later batch must rewrite.
2. `gate-load-from-the-bench` — 24f2a3e, ORCH-147 (proposed ORCH-N4). Merge
   commit 9c9ef18. Handoff:
   docs/orchestration/history/2026-10-29-gate-load-from-the-bench.md
3. `orch-146-retention-preview` — bdb1d9f, ORCH-146. Merge commit b049568.
   Handoff: docs/orchestration/history/2026-10-28-orch-146-retention-preview.md
4. `orch-104-walk-cost` — 9a9f7ae, ORCH-104. Merge commit e5c9476. Handoff:
   docs/orchestration/history/2026-10-28-orch-104-walk-cost.md

Merged one at a time onto one integration branch off 09229db, so that a later
red run is attributable to a known merge commit rather than to a blur. Zero
conflicts, measured at each merge and not assumed.

## Evidence

**The batch's own gate, on the merged tree.** ./scripts/gate.sh on GATEDSHA,
this commit's second parent, trees identical by git diff --quiet. Verdict line
verbatim and whole output in herd-merger's report; receipt
GATEDSHA.receipt under target/herd-gate/ of the integration worktree. It is
the gate that counts for this landing.

**Each branch's own output, separately, never one blurred summary.**

1. orch-119-announced-ceiling. Its own gate on 326b714 was RED at step 7
   only, on flood-fifty and flood-two-fifty, which is the host-measurement
   defect branch 2 of this batch fixes. Steps 2 to 6 ok, suite 365 s.
   check-orchestration.sh 256 passed before and after. The fix's evidence is a
   rate at each condition: taskset -c 0,1 plus 8 spinners, 13 failures in 20
   before and 0 in 20 after with 13 of the 20 taking the formerly-red arm;
   plus 4 spinners, 4 in 20 before and 0 in 20 after; unconstrained, 0 in 35
   both. And 10 of 10 red at a different assertion (1156) with the outage at
   4 s on an idle 12-core host, which brackets the same window from the other
   side and shows the host is incidental.
2. gate-load-from-the-bench. check-orchestration.sh 256 to 269: nineteen new
   cases, six that encoded the old rule removed. 8 of the 19 go red against
   the base gate; four mutations of the new gate are each caught by at least
   one case. Five real gate runs on this host with a real burst: run A on the
   base went RED 7:unexpected-skip with the gate reading 1.75 and 5.95 while
   the benches read 13.34; runs B, D and E took that same path on the branch
   and step 7 was ok each time with the excuse printed by name, figure and
   ceiling; run D GREEN with
   skips=under-load(flood-fifty=11.53>8,flood-two-fifty=11.53>8); run C is the
   honest at-rest run, 12 skips, the two benches reported as having RAN. Run B
   was RED at step 6 on a suite flake in announced_job_ceiling.rs:1179 — the
   test branch 1 of this batch fixes, and it is carried as ORCH-119's second
   sample.
3. orch-146-retention-preview. 14 new assertions over a fixture whose every
   job.json is padded to 512 bytes, so each printed figure is checkable by
   arithmetic; asserted figure by figure and then as whole rendered text. A
   run while a serving hub held the flock on hub.lock left the state root
   byte-identical afterwards, and it was also run by hand against a hub that
   was really serving. store.rs untouched; RECORD_VERSION 1 and
   PROTOCOL_VERSION 2 unmoved.
4. orch-104-walk-cost. Release build, median of five warm runs and one cold
   run per cell, at 1 000 / 3 334 / 10 000 / 33 340 / 100 000 directories. The
   100 000 row is the only one taken under load (8.08-10.90) and is marked;
   its cold cell is the one figure asked to be re-taken at rest before anyone
   quotes it. The warm walk is load-insensitive between load 3 and 10 on this
   host, measured twice by two phases that share no code (0.7 % and 3.5 %
   apart). Order: cold id order 0.50x at 3 334 and 0.41x at 33 340, run in
   both sequences (1.83x and 2.01x reversed) with a warm control showing
   0.99x and 1.02x. drop_caches was not used — it needs root and the
   consumer's production shares this host — so every cold figure is a floor.
   No product file touched.

**Skips.** The twelve names of scripts/expected-skips/local.txt, listed in
full in herd-merger's handoff under *Gates*, all one cgroup v2 delegation
refusal; plus flood-fifty and flood-two-fifty if and only if their own skip
lines excuse them by name and by reason (D196). Fourteen names at most, each
printed as a skip, none counted as a pass. The count is not the signal and is
not the gate's test: the list is (D27, D179, D187).

## Left out

- No pull request, no tag, no release. The ordinary release is Alexa's next
  act and ORCH-156 says it ships at layout 1 and record 1.
- Nothing of the version triple D199 authorises: not one line of
  PROTOCOL_VERSION, RECORD_VERSION or LAYOUT_VERSION moved. ORCH-156 exists
  so no brief takes it before that release.
- joining-mesh-residue is not in this batch. D200 is recorded here because a
  decision no branch carries is the merger's to write down; its operator-facing
  prose lands on that branch in docs/joining.md.
- ORCH-143's case, which would move check-orchestration.sh to 270: no branch
  here touches pre-push, and a merger adding an untested case to the gate it
  is about to run on is what the method forbids.
- docs/plan.md and docs/coverage.md, which D205, D206 and D207 each make
  partly false. That rewrite needs a reviewer: ORCH-155 and ORCH-152.
- M12's retention engine, ORCH-098's largest task. The class map is answered
  (D201) and the preview is on main; the engine is unstarted and lands after
  ORCH-094.

## Decisions allocated

Thirteen, D195 to D207, allocated once for the whole batch — in merge order
first, then Alexa's. Next free above D194 on origin/main and above every
unlanded branch, all of which were scanned; none holds a number above D194.
Gaps stay: D143, D154 and D170 are untouched (D148, invariant 8).

From the branches, in merge order:

- D195 — A runner's own count is not always its ceiling (orch-119).
- D196 — The gate reads the load off the bench that refused (gate-load).
- D197 — A retention pass that would empty the state root refuses (orch-146).
- D198 — The record walk's measured cost replaces the derived (orch-104).

From Alexa, 2026-10-29, carried by no branch:

- D199 — All three versions move before v1.0.0.
- D200 — Pack bytes travel in clear on the operator's private mesh.
- D201 — Retention ships active, merger 90 days and the rest 14.
- D202 — The output floor stays, the hub stops promising it.
- D203 — GitHub is storage; branch protection is not pursued.
- D204 — "Stable" means v1.0.0.
- D205 — M11's pack half is inside v1.0.0, which makes it 44 of 44.
- D206 — M17 is outside v1.0.0, E-49 and E-50 unmet by choice.
- D207 — E-41's scheduler keeps D106, the predicate moves out.

## Board rows

**Closed, done:** ORCH-119 (and it carries two samples, not one: CI run
40762764493 at 09229db, and a gate run under a deliberate burst at load1
11-15 where the runner's control link reset and re-dialled eight or more
times); ORCH-104; ORCH-146 (new, done); ORCH-147 (new, done — the gate's load
rule, proposed as ORCH-N4 in ORCH-104's handoff).

**Answered:** ORCH-098 → done, the class map as written plus active by
default (D201), the engine still unstarted. ORCH-105 → done, (i) and (iii)
together (D202). ORCH-020 → done, packs are inside v1.0.0 (D205), and its
premise is corrected in the row: D161 blocks verification on the consumer's
machines, while the done-when here is a two-runner end-to-end that runs on
this workstation through herd-hub/tests/real_herd/mod.rs.

**Re-labelled:** ORCH-087 → post-1.0, a tenth exclusive store.rs step that
would invalidate the others' diffs and that no E-nn asks for. ORCH-106 →
taken off *Waiting* and into *Owed*, owner herd-step: it is a how, not a
decision for Alexa, it is live on their machine, nobody there can repair it,
and it had been sitting behind four unrelated questions. Its RECORD_VERSION
caveat stays and goes into the brief, which D199 makes sharper.

**Amended:** ORCH-123, re-sized off the measured quarter rather than the
apparent 80 %, with the note that the attribution inverts cold. ORCH-144,
confirmed from a second independent direction — the thirteen cases. ORCH-143,
still open after this batch and said to be, with the reason.

**New:** ORCH-146, ORCH-147 (both done, above); ORCH-148, five declaration
fields parsed, validated and silently discarded while coverage.md marks M10
merged; ORCH-149, --bless can still adopt a flood name into local.txt, whose
next step is the orchestrator's decision that the gate must refuse to bless a
delta name; ORCH-150, the gate now depends on a HERD-SKIPPED line's wording —
a coupling to record, not a defect; ORCH-151, HERD_GATE_LOAD_NOTE and
LOAD_CEILING are two names for 8 with nothing tying them together; ORCH-152,
plan.md disagrees with the code in about a dozen places, four of which change
what gets built; ORCH-153, the pack-collection defect — a pack would stream
runner to hub to requester as record evidence, the exact byte path D10
removed, and this must be written down before any pack code is briefed;
ORCH-154, the consumer's disk as a rate and not a stock, ~11 GB/day, first
eviction within weeks of their resuming, four-key eviction order never run in
production; ORCH-155, the roadmap to v1.0.0 exists only in a scratchpad and
is the next batch's work, with a reviewer because it rewrites plan.md;
ORCH-156, the ordinary release ships at layout 1 and record 1 and no brief
may take the version triple early; ORCH-157 and ORCH-158, ORCH-104's N2 and
N3 — the startup pass needs a bound of its own, and sorting names after
names_in halves the cold cost.

**And *Next* item 9 is closed without being done:** transition step 2,
narrowing the branch protection, is deliberately not pursued (D203), which is
the accepted final state and not a gap. Item 10 lists the nine decisions and
the order to read them in before briefing anything.

## CI run

none yet at the time this record was written — this push starts it. Its id
comes from gh run list --workflow ci.yml --branch main --limit 1 and is in
herd-merger's report to the orchestrator, recorded and **not waited on**,
which is the wait the local gate exists to stop paying. It is the first
post-push run since 7eb9585 that could be green, so this one was judged worth
reading rather than only recording: whether main is red is the first thing
Alexa needs. The verdict is in the report, and a failure becomes an ORCH-nnn
row before new work is briefed.
```

## Risks and findings

1. **`main` has been red since `7eb9585` and this landing is the one that
   should end it, but one sample does not prove a clock.** D195 fixes the
   assertion at 1179; **assertion 1156 in the same file sits on the same
   window**, one regime further out, and was red 10 of 10 at a 4 s outage
   on an idle host. Contention does not push towards it (0 of 40 under
   load), so it is not what CI hit — but the margin is ~1.5 s of wall
   clock and it is a margin, not a mechanism. If `main` goes red there,
   it is ORCH-119's neighbourhood and not a new finding.
2. **A RED gate verdict can now carry `skips=under-load(...)`.** Run B of
   `gate-load-from-the-bench` produced exactly that. It is correct — one
   skip was excused and another step failed — but anyone grepping
   `under-load` to mean "green under load" would be wrong. The field says
   which list applied, not what the verdict is.
3. **Thirteen new rows is a lot of board at once**, and eleven of them
   came from the orchestrator's brief rather than from a branch's
   evidence. Each carries its origin cell saying so. Two, ORCH-157 and
   ORCH-158, are mine to have placed at all; see *Not done*.
4. **ORCH-153 is the most dangerous row in this batch** and nothing in
   this landing fixes it. D205 puts the pack half inside v1.0.0, so the
   first pack brief is what would hit it, and today
   `herd-runner/src/outputs.rs:12-40` would collect a 400 MiB pack as
   record evidence against the hub's 200 GB budget.
5. **`perl` without `use utf8` silently double-encodes.** It put five
   mojibake `§` into board rows that `check-docs.sh` accepted. Repaired,
   and worth knowing for anyone editing these files by script: validate
   with `iconv` and grep for `Ã|Â`, because the docs check will not.

## Decisions needed

**None blocking.** Two things for the orchestrator:

1. **Were ORCH-157 and ORCH-158 mine to place?** ORCH-104's handoff
   proposed four rows; the brief listed one of them (N4, now ORCH-147).
   I placed N2 and N3 as ORCH-157 and ORCH-158 and folded N1 into
   ORCH-123, because they come from a branch in this batch and would
   otherwise be lost. *Recommendation:* keep them; an open item is never
   dropped because a batch ended. *Fallback:* strike the two rows — the
   evidence survives in the handoff on `main` either way.
2. **ORCH-149's next step is written as a decision already taken** — the
   gate refuses to bless a delta name, owner `herd-step`, per the brief.
   It is a change to the gate, so `CLAUDE.md` requires a reviewer pass;
   that is named in the row and not assumed away.

## Resume point

`main` carries D195 to D207, the four branches of this batch and a board
whose next three things are, in order: read this landing's post-push
`ci.yml` verdict (in the report; a failure becomes a row before anything
is briefed), put the ordinary release to Alexa with **ORCH-156** attached,
and brief **ORCH-155** with a reviewer, since it rewrites `plan.md` and
**ORCH-152** is the list of what must come out in that rewrite. The
version triple of D199 waits behind the release, deliberately.

## Processes

No build or test process was started by hand: the only heavy phase is the
single `./scripts/gate.sh` run, whose children are its own. No load
generator was started. Nothing was signalled, no `pkill`, no `killall`,
and no PID not started by this agent was touched.
