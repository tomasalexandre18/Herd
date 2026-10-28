# merge-gate-and-report — handoff 2026-10-28

- **Item:** the first local landing. ORCH-134, ORCH-135, ORCH-111,
  ORCH-131 and ORCH-125 closed; ORCH-119 given a dated sighting;
  ORCH-140 to ORCH-145 opened; transition step 3 of `workflow.md`
  *Not an option* carried
- **Agent:** herd-merger
- **Branch / commit:** integration branch `merge-landing-1`, off
  `origin/main` `7eb9585`; this handoff is its tip. Its worktree, created
  by this agent under its own session scratchpad (ORCH-031):

  ```text
  /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-landing-1
  ```
- **Landing:** this one. **The project's first local landing: no pull
  request, two branches, one `--no-ff` merge commit whose message is the
  record below, one push, one post-push `ci.yml` run.** The record is
  copied whole under *The landing record*, and it is the merge commit's
  message.

## Done

### 1. The batch, listed and merged one at a time

Fetched, then compared with `origin/main` `7eb9585` before anything was
planned. **The four gate branches are a linear chain**, verified rather
than assumed, with `git merge-base --is-ancestor`:

```
origin/bootstrap-the-gate            f258e51  is ancestor of
origin/harden-the-gate               446e632  is ancestor of
origin/no-sigpipe-assertions         6649bd1  is ancestor of
origin/orch-111-stdin-independence   20f1500  is ancestor of
origin/pre-push-no-pipe              0e1c061  <- contains all four
```

So **two** things were merged, in this order, one at a time, each its own
`--no-ff` merge commit onto one integration branch:

1. **`origin/pre-push-no-pipe` `0e1c061`** — merge commit `b8caf06`.
   12 files, +2412 -76: the five handoffs, `scripts/gate.sh`,
   `scripts/githooks/pre-push`, `scripts/check-orchestration.sh`,
   `scripts/expected-skips/local.txt` and `local-under-load.txt`,
   `docs/orchestration/workflow.md` and `templates/handoff.md`.
2. **`origin/orch-125-state-root-report` `dad7366`** — merge commit
   `ccade1f`. 6 files, +1789 -7: its handoff, `herd-hub/src/report.rs`
   (new), `herd-hub/tests/state_root_report.rs` (new),
   `herd-hub/src/main.rs`, `src/lib.rs` and `src/store.rs`. It was cut
   from `origin/main` at `bc83717`, not from `7eb9585`, which is why
   `store.rs` needed a real merge.

**Decisions added, per branch, and conflicts expected:** the two sets of
files are disjoint except `herd-hub/src/store.rs`, and **neither branch
touches `docs/decisions.md`, `docs/orchestration/board.md` or
`docs/status.md`** — so no decision entry arrived with either branch, and
every `D-NEXT` and every board row below is written by `herd-merger`
alone, from the handoffs' proposed text. One conflict was expected, in
`store.rs`; it did not materialise as a conflict.

### 2. The conflict log

**Zero conflicts.** Both merges were clean. One file merged
automatically and is logged because an automatic merge of a file both
sides changed is exactly where a silent revert hides:

- **`herd-hub/src/store.rs` — auto-merged by the `ort` strategy, kept
  whole from both sides, nothing resolved by hand.** `main`'s side (via
  `bc83717..7eb9585`) adds `readmit_to_the_budget` and its call site,
  ORCH-124's work; `orch-125`'s side widens six names to `pub(crate)`
  (`JOBS_DIR`, `RECORD_FILE`, `KEPT_FILES_DIR`, `OUTPUT_PARTS_DIR`,
  `OUTPUT_GONE_DIR`, `directory_bytes`) so the report reads the layout's
  own names rather than a second copy of them. Checked in **both**
  directions: `git diff origin/main HEAD -- herd-hub/src/store.rs` holds
  only the six visibility lines, and
  `git diff origin/orch-125-state-root-report HEAD -- herd-hub/src/store.rs`
  holds only `main`'s own ORCH-124 hunks. Nothing was kept from one side
  at the other's expense and no behaviour was touched.

**No conflict changed a specification or an invariant**, so nothing was
escalated.

### 3. The board check, both ways

Run although the batch had no conflict — `workflow.md` *One landing per
batch* says a batch with no conflict needs it most, because a silent
revert then has nothing to resolve and nobody has to read the file.

- **Direction 1, each branch's own lines verbatim in the merged file.**
  For every one of the 18 files either branch changed against the merge
  base, the merged blob was compared with the branch's blob by object
  id: **17 of 18 are byte-identical to their branch**, and the one that
  is not is `herd-hub/src/store.rs`, logged above and checked both ways
  on its own. **Neither branch wrote a line into `board.md`**, so the
  verbatim-board-line check is satisfied with nothing to check — stated
  rather than skipped, because "no board hunk" is the condition and not
  an excuse.
- **Direction 2, the merged file differs from each branch only by the
  other's hunks.** `git diff --name-only origin/pre-push-no-pipe HEAD`
  before this commit was exactly orch-125's own six files, and
  `git diff --name-only origin/orch-125-state-root-report HEAD` was
  exactly the set `pre-push-no-pipe` changed plus what `main` gained
  between `bc83717` and `7eb9585` — the two lists compared equal as
  sets. No third file moved.

**Every branch of the batch left its handoff** in
`docs/orchestration/history/`: five for the chain
(`2026-10-28-bootstrap-the-gate.md`, `-harden-the-gate.md`,
`-no-sigpipe-assertions.md`, `-orch-111-stdin-independence.md`,
`-pre-push-no-pipe.md`) and one for the other branch
(`2026-10-28-orch-125-state-root-report.md`). All six are in the tree
this gate runs on and land in the same push as the record.

### 4. Transition step 3: `ci.yml`'s `pull_request:` trigger is gone

One line deleted from `.github/workflows/ci.yml`. `push: branches:
[main]` is kept, `concurrency`, `permissions` and both jobs are
untouched, and `release.yml` and `release-dev.yml` were not opened
(D162, D168, D172). **`ci.yml` now starts a run only on a push to
`main`.** Nothing in `check-docs.sh` or `check-orchestration.sh` asserts
anything about the triggers, checked before the edit.

This is the last of the three steps of `workflow.md` *Not an option*
that is ours. **Step 2 — narrowing the branch protection — is Alexa's
and is still pending**, and this landing did not wait for it and did not
touch branch protection, `enforce_admins` or the repository's
visibility.

### 5. The five decision numbers

Allocated once for the whole batch, in merge order, appended at the end
of `docs/decisions.md`; nothing renumbered, nothing edited. **The next
free number is D190**: the highest entry on `origin/main` is **D189**,
and the highest on **every branch not yet landed** is also D189
(measured over all 100-odd unlanded remote branches, the runner-up being
D184). **D143, D154 and D170 stay gaps** (D148).

| allocated | title | from |
|---|---|---|
| **D190** | The gate as built: steps 0 to 8, one verdict, one receipt | `bootstrap-the-gate` |
| **D191** | A blessing is not a gate run, and the load is read twice | `harden-the-gate` |
| **D192** | A reader that can exit early never sits downstream | `no-sigpipe-assertions` and `pre-push-no-pipe`, which are one rule |
| **D193** | A gate script owns its own fd 0 | `orch-111-stdin-independence` |
| **D194** | `herd-hub report` reads a state root without a lock | `orch-125-state-root-report` |

Two allocation judgements, stated because they are mine and not the
handoffs':

- **D192 carries two branches' findings in one entry.**
  `no-sigpipe-assertions` measured the mechanism and
  `pre-push-no-pipe` fixed the one site that could refuse a landing;
  they proposed two `D-NEXT` texts for the same rule, and two entries
  for one rule is how a reader ends up believing there are two.
- **No decision was allocated for the `pull_request:` deletion.** D189's
  *The transition* paragraph already prescribes it as step 3 by name. A
  second entry would be a duplicate; the deletion is recorded in the
  landing record and in `docs/status.md`.

### 6. The board and the status

`docs/orchestration/board.md`: **seven rows rewritten, six added**, six
sections still in order, every `ORCH-` row still eight cells with a
known state, no duplicate id — `check-docs.sh` checks all four and is
green.

- **ORCH-111 — `done`, and its text replaced rather than appended to.**
  It is **not** a SIGPIPE defect: a socket on the caller's fd 0 made
  `bash -c` children source `/etc/bashrc`, which rebuilt `PATH` from the
  test's own `HOME=/wrong/home` and discarded the `PATH` the test
  supplied. `253 passed, 0 failed` with `/dev/null` on fd 0 against
  `232 passed, 21 failed` with a socket, same tree `6649bd1`, same
  minute. It cost **21 of 253**, not one. The row says so because the
  orchestrator signed off a wrong diagnosis of it earlier the same day.
- **ORCH-131 — `done`, measured**, not re-run: `git config --get
  core.hooksPath` returns `/mnt/ssd_980/herd-project/scripts/githooks`,
  absolute, with the date and the three places it was proved from. The
  two rows that asserted the relative value in passing, **ORCH-136's and
  ORCH-137's `done` cells, are corrected**.
- **ORCH-134 — `done` cell updated** (the gate goes green here now),
  **ORCH-135 — `done`** (both skip lists measured, step 3's sixth
  precondition added), **ORCH-125 — `done`, as a new row**: the
  planner's plan reserved the number and no board ever carried it.
- **ORCH-119 — stays `open`, with the sighting the orchestrator was
  holding:** `main` went **RED** on post-push run `40720983497` at
  `7eb9585`, `announced_job_ceiling.rs:1179`, on content that was
  **green** on PR run `40719469672` ten minutes earlier; the method
  batch touches no `.rs` and no `Cargo.*` file, so it cannot be the
  cause, and it was deliberately not re-run until green.
- **Six new rows.** `ORCH-140` — `check-docs.sh` exited 1 while printing
  only the pre-existing D170 note and **not** the `sections are …`
  message its planted defect provokes, 1 sample in 12 loaded runs on
  `446e632`; **not** the pipe race, because that race's signature is the
  expected text being present and here it is absent; lead is `fresh()`'s
  32 whole-repo copies. `ORCH-141` — the two `.claude/hooks` guards are
  safe **only** because they set no `pipefail`, and the direction is
  **fail-open**: adding one would make 14 races that *allow* the write
  the guard exists to deny. `ORCH-142` — `check-docs.sh:247,249`.
  `ORCH-143` — nothing mechanical freezes `pre-push`'s here-string
  shape; the tested case text is in the `pre-push-no-pipe` handoff and
  moves the count to 257; owner `herd-step`, **the first task of the
  next batch**, and the orchestrator decided not to hold this landing
  for it. `ORCH-144` — a docs defect anywhere in a batch's tree turns
  **13** `expect_green` cases red at once and says nothing about the
  branch under test. `ORCH-145` — `waiting`: ask the operator for one
  `herd-hub report` reading, and decide the dedup threshold after it.
- The *Next* section's transition paragraph now says **steps 1 and 3 are
  done and step 2 is Alexa's**, where it said only step 1 was done.
- **ORCH-139 left exactly as it is**, as briefed.

`docs/status.md` restated to this landing, **90 lines** — the limit, with
no headroom: the method as it now stands, the gate and what it refuses,
`pull_request:` gone and `ci.yml` running only on a push to `main`,
fourteen changes in no release (D194 is the fourteenth), `main` 309
commits past v0.3.7, the consumer's instance paused, and ORCH-119's red
run named. To stay inside 90 lines I dropped the v0.3.6 lineage sentence
and D179's clause; both are decisions and board rows, not state.

### 7. Housekeeping

The three worktrees their agents kept because handoffs cite their gate
logs and receipts — `wt-sigpipe`, `wt-orch-111`, `wt-pre-push` under
this session's scratchpad — are removed **after** the push, not before,
and what became of them is in the report. The orchestrator's checkout is
fast-forwarded last of all, after `main` has moved, with both
preconditions checked as commands; the before and after shas are in the
report.

**Left as it is, deliberately:** the `no-sigpipe-assertions` handoff's
*Processes* line says "two extra worktrees … removed" where one was
removed and one kept. A past handoff is an append-only record and is not
edited.

## Not done, and why

- **ORCH-143 is not done and this landing was not held for it.** The
  orchestrator decided that on 2026-10-28; the row carries the tested
  case text as its next action and is the next batch's first task. Until
  it lands, only a comment in `pre-push` stops the pipes coming back.
- **ORCH-119 was not re-run.** Step 1 of `workflow.md`'s red-`main`
  procedure is `gh run rerun --failed`, and it is still owed on that
  row; the orchestrator deliberately did not run it, so the sighting is
  recorded and not retried.
- **The branch protection, `enforce_admins` and the repository's
  visibility are untouched.** Transition step 2 is Alexa's.
- **No tag and no release.** Alexa's word, one act at a time.
- **ORCH-141 and ORCH-142 are rows and not fixes**, and
  `check-docs.sh:247,249` and the 14 guard sites still carry D192's
  shape. None can fire today, and the guard sites are inert by
  construction.
- **`core.hooksPath` was not re-set.** It is already absolute and
  correct, and running that command is a repository-wide act over 85
  live worktrees.
- **No pull request was opened.** The one exception, the method landing,
  was #106 and is already on `main`; there is no second one.

## Gates

**Before this commit**, on the merged tree, both prose gates and both
green:

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

```
$ ./scripts/check-orchestration.sh
check-orchestration: 256 passed, 0 failed
```

`./scripts/bench-preflight.sh` before the heavy phase:

```
filesystem:  /mnt/ssd_980, 338 GiB free
load (1m):   0.55 on 12 cores, ceiling 9.00
memory:      22 GiB available, floor 2
bench-preflight: ok, safe to start.
```

**The last run, named and not quoted — this is ORCH-121 applied to the
merger.** `./scripts/gate.sh`, one run, on a clean tree, on the tip that
carries this file, which is the integration tip and **the second parent
of the merge commit that lands this batch**. The gate must run on the
tree that is pushed, so this file had to be in that tree already and
**nothing is added to it afterwards**. Its receipt will be written as
`<sha>.receipt` in `target/herd-gate/` of the integration worktree named
under *Branch / commit* above, where `<sha>` is this handoff's own commit
— which is exactly why that sha cannot be written here.

**What is in the report to the orchestrator and deliberately not in this
file:** the gate's **verbatim output** and its **skip list**, the gated
sha, the **merge commit sha**, the **post-push `ci.yml` run id**, and
what step 10 did to the orchestrator's checkout. Every one of them
happens after this file was committed, and a second commit to carry them
would need a second gate run and a second push — which is the one thing
*One landing per batch* forbids.

**Skips.** The expectation this gate compares against is
`scripts/expected-skips/local.txt`, **twelve** names, every one of them
the same cgroup v2 delegation this host does not grant, plus a two-name
**delta** list that applies only above a one-minute load of 8
(`flood-fifty`, `flood-two-fifty`). The twelve are named one by one in
the record below, each as a skip and never as a pass (D27, invariant 5).
The **list** actually observed, and the count, are in the report — a
count is not a signal (D27, D179).

## The landing record

The merge commit's message, in full. One thing in it cannot be identical
to the commit's own copy and it is said rather than hidden: the
`GATEDSHA` token below is this handoff's own commit sha, which the merge
message carries and which this file cannot contain. Everything else is
word for word.

```text
Land the gate batch: the local gate, its hook and herd-hub report

- **Batch:** the gate batch, two branches, landed 2026-10-28 by herd-merger.
  The project's first local landing: no pull request, one --no-ff merge, one
  push, one post-push ci.yml run.
- **Gate:** green — ./scripts/gate.sh on GATEDSHA, this commit's second
  parent, with `git diff --quiet GATEDSHA HEAD` silent. Receipt
  GATEDSHA.receipt in the integration worktree's target/herd-gate/. The
  verbatim output is in herd-merger's report to the orchestrator: it cannot
  be in this tree, because the gate ran on this tree (ORCH-121).
- **Skips:** 12, all one capability, named below under *Evidence* and each
  printed as a skip and never counted as a pass (D27, invariant 5).
- **CI run:** none yet — this push starts it. The run cannot exist before the
  commit that triggers it, so its id goes to herd-merger's report now and the
  next landing's board edit records it; a second push for one batch is the one
  thing the method forbids.

## What changed and why

Two things, and the method change they complete.

**The local gate is real and green on this host.** Until today
`scripts/gate.sh` could not finish a plain run here at all: the expected skip
list it reads did not exist, so it refused with exit 3 and no verdict, and
`CLAUDE.md`'s push condition named a mechanism nobody could satisfy. The two
host lists are now measured and committed, four defects in the gate and its
hook are fixed, and the hook's own reads of the landing record no longer sit
downstream of a killed writer. `main` can now be landed on without GitHub.

**This landing is transition step 3**, the last of the three steps of
`workflow.md` *Not an option* that is ours: it deletes `ci.yml`'s
`pull_request:` trigger, which only a lane needing no pull request could do,
since a pull request removing that trigger never produces the two contexts it
requires. `ci.yml` now runs on a push to `main` and nothing else.
`release.yml` and `release-dev.yml` are untouched (D162, D168, D172). Step 2,
narrowing the branch protection, is Alexa's and still pending; nothing waited
on it, because `enforce_admins` is false against an admin credential.

**And `herd-hub report [STATE_DIR]` exists**, which moves no milestone and
answers no numbered expectation but produces the one figure that decides
whether file-level deduplication is ever built.

## Branches in this batch

In the order they were merged:

1. `pre-push-no-pipe` — `0e1c061`, ORCH-134, ORCH-135, ORCH-111, ORCH-131 and
   ORCH-143's subject. A linear chain of five tasks, verified with
   `git merge-base --is-ancestor` before planning: `bootstrap-the-gate`
   (`f258e51`) in `harden-the-gate` (`446e632`) in `no-sigpipe-assertions`
   (`6649bd1`) in `orch-111-stdin-independence` (`20f1500`) in this tip. Five
   handoffs, one per task:
   docs/orchestration/history/2026-10-28-bootstrap-the-gate.md,
   2026-10-28-harden-the-gate.md, 2026-10-28-no-sigpipe-assertions.md,
   2026-10-28-orch-111-stdin-independence.md, 2026-10-28-pre-push-no-pipe.md
2. `orch-125-state-root-report` — `dad7366`, ORCH-125. Cut from `main` at
   `bc83717` and independent of the chain. Handoff:
   docs/orchestration/history/2026-10-28-orch-125-state-root-report.md

Merged one at a time onto one integration branch off `origin/main` `7eb9585`,
so that a later red run is attributable to a known merge commit.

## Evidence

**The batch's own run, on the merged tree.** `./scripts/gate.sh` on GATEDSHA,
the integration tip and this commit's second parent, steps 0 to 8 ok, verdict
`gate: GREEN sha=GATEDSHA steps=0-8:ok`. `git diff --quiet GATEDSHA HEAD` is
silent: identical trees, different shas, which is what makes the receipt bind
(workflow.md *What a landing is*; the merge commit's message is this record,
so its sha cannot exist before the gate). Verbatim output in the merger's
report and in the receipt, not in this tree.

**Twelve skips, named, and not one of them a flood bench.** All twelve are the
same capability: this host grants no cgroup v2 delegation, so a job may not
create a cgroup under its scope. They matched
`scripts/expected-skips/local.txt` exactly.
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

**Each branch's own evidence, separately, from its own handoff.**

`pre-push-no-pipe`, and the four tasks under it. Each branch's own figures,
not one summary:

- `bootstrap-the-gate`: four gate runs, because `--bless` dirties the tree and
  step 0 refuses a dirty tree, so no run can both bless and certify a sha.
  Run A `--bless` at rest wrote 12 lines after a 240 s suite ending
  `test suite: PASSED`; run B `--bless` under load1 9.56 into a scratch file
  gave 14, so the delta is exactly two; run D plain at load1 9.04 read both
  committed lists; run E plain at rest on `2add9db` certified the tip.
  `check-orchestration: 243 passed, 0 failed` in every one.
  `core.hooksPath` made absolute and proved from three places, including a
  pre-landing worktree at `9d3d101` where the relative form resolved to a
  directory that does not exist.
- `harden-the-gate`: GREEN before (`f258e51`, load1 0.54) and GREEN after
  (`2fbb8e1`, load1 0.60), same twelve skips. A third run, deliberately under
  twelve spinners, is reported **RED** —
  `gate: RED sha=2fbb8e1… failed=2:check-orchestration skips=under-load(load1-end=14.99>8)`
  — with step 7 ok: it certifies the two-reading fix and it went red at step 2
  on the pipe race that branch then found. Reported red, nothing edited to
  turn it green. 243 to 253 cases, none dropped.
- `no-sigpipe-assertions`: 25 conversions, `grep -cE 'printf.*\|[[:space:]]*grep -q'`
  26 before and 0 after, no pipe into `grep` of any kind left. The mechanism
  measured at eight payload sizes, 200 runs each at load1 13-15: 0/200 up to
  4 161 bytes, 1/200 at 4 965 and 7 980, 14/200 at 64 kB, 200/200 at 1 MB;
  a here-string 0/200 at every size. Twelve interleaved before/after runs
  under load, 253 passed 0 failed on both sides, and the handoff says plainly
  that the counts **had** to come out the same because no site there reads
  more than 3 692 bytes.
- `orch-111-stdin-independence`: `253 passed, 0 failed` with `/dev/null` on
  fd 0 against `232 passed, 21 failed` with an AF_UNIX socketpair, same tree
  `6649bd1`, same minute. Five interleaved runs each way after the fix, all
  `256 passed, 0 failed`. GREEN gate on `f056c25`. Three textual cases, each
  seen red on removal.
- `pre-push-no-pipe`: the false refusal reproduced on the base with a
  **correct** record, driven the way git drives the hook: 0/200 at 3 091 and
  6 051 bytes, 4/200 at 62 291 under load1 12-16, 50/50 at 1 045 091 at rest.
  The fixed hook refused none of 900 correct landings, and 25/25 real pushes
  through `core.hooksPath` were accepted. All thirteen refusals it owes still
  red at a 62 kB record. `check-orchestration: 256 passed, 0 failed` before
  and after.

`orch-125-state-root-report`: all five gates run on the branch. `cargo fmt`
exit 0, `cargo build --workspace` and `cargo clippy --all-targets -D warnings`
clean, `check-docs.sh` exit 0. `./scripts/test.sh` on the branch alone was
**RED**, exit 101, on the three `real_herd` fixtures of D186 — reported red by
its author, proved pre-existing, and green on a throwaway merge with
`real-herd-patience-timeouts`: `test suite: PASSED`, 14 skips at load1 8.39,
the twelve above plus `flood-fifty` and `flood-two-fifty`. That fix is on
`main` since `ee8dec0`, so the figure that counts is this landing's own gate.
Its three new tests passed in every run.

**The merge itself.** Zero conflicts. One file, `herd-hub/src/store.rs`, was
auto-merged and was checked in both directions: against `origin/main` it
differs only by orch-125's six `pub(crate)` widenings, and against
`origin/orch-125-state-root-report` only by `main`'s own ORCH-124 work. Every
other file of either branch is byte-identical to its branch. The board check
both ways is trivially satisfied and was still run: **neither branch touched
board.md, decisions.md or status.md**, so there was no line to lose, and
herd-merger wrote all three alone.

## Left out

- **The branch protection is untouched**, and so are `enforce_admins` and the
  repository's visibility. Transition step 2 is Alexa's.
- **No tag, no release.** Those stay Alexa's word, one act at a time.
- **ORCH-143 is not done and this landing was deliberately not held for it.**
  Nothing mechanical freezes `pre-push`'s new here-string shape; the case text
  is written and tested in the `pre-push-no-pipe` handoff and moves
  `check-orchestration.sh` to 257. It is the first task of the next batch.
- **ORCH-119 is not re-run.** `main` went red on post-push run 40720983497 at
  `7eb9585`, on content green on PR run 40719469672 ten minutes before. It
  stays a dated sighting on an open row, not a retry.
- **`check-docs.sh:247,249` keep the pipe shape** (ORCH-142) and the two
  `.claude/hooks` guards keep it inertly (ORCH-141). Neither can fire today.
- **No decision was allocated for the `pull_request:` deletion itself.** D189
  *The transition* already prescribes it as step 3; a second entry would be a
  duplicate, and the deletion is recorded here and in `docs/status.md`.

## Decisions allocated

Five, in merge order, the next free numbers above D189 — the highest on
`origin/main` and on every branch not yet landed. D143, D154 and D170 stay
gaps (D148).

- **D190** The gate as built: steps 0 to 8, one verdict, one receipt
- **D191** A blessing is not a gate run, and the load is read twice
- **D192** A reader that can exit early never sits downstream
- **D193** A gate script owns its own fd 0
- **D194** `herd-hub report` reads a state root without a lock

## Board rows

- **ORCH-111 — done**, and its text replaced: it is **not** a SIGPIPE defect.
  A socket on the caller's fd 0 made `bash -c` children source `/etc/bashrc`,
  which rebuilt `PATH` from the test's own `HOME=/wrong/home`. 21 of 253, not
  one. A wrong diagnosis of this row was signed off earlier the same day.
- **ORCH-125 — done** (new row; the planner's plan reserved the number and no
  board ever carried it). **ORCH-131 — done**, measured, with ORCH-136's and
  ORCH-137's passing assertions of the relative value corrected with it.
  **ORCH-134 — done** cell updated: the gate goes green here now.
  **ORCH-135 — done**: both skip lists measured, step 3's sixth precondition
  added.
- **ORCH-119 — open**, with the dated red-on-`main` sighting above.
- **New and open:** ORCH-140 (`check-docs.sh` red while printing only the
  D170 note, one sample in twelve, not the pipe race), ORCH-141 (the two
  `.claude/hooks` guards are safe only because they set no `pipefail`, and the
  direction is fail-open), ORCH-142 (`check-docs.sh:247,249`), ORCH-143
  (nothing freezes `pre-push`'s shape; next batch's first task), ORCH-144 (a
  docs defect anywhere turns 13 `expect_green` cases red at once).
- **New and waiting:** ORCH-145 — ask the operator for one `herd-hub report`
  reading, and decide the dedup threshold with the number in hand.
- **ORCH-139** left as it is. The *Next* section records that transition steps
  1 and 3 are done and step 2 is Alexa's.

## CI run

Recorded, not waited on, and this is the first landing where that duty
exists. The run id cannot be in this message: the run is started by the push
of this commit. It goes to herd-merger's report immediately after the push,
from `gh run list --workflow ci.yml --branch main --limit 1`, and the next
landing's board edit is what carries it into the repository, because a second
push for one batch is forbidden. Its verdict is read at the next session
start, and a failure becomes an ORCH row before new work is briefed. The
landing was **not** judged risky: the gate was green on the merged tree at
rest, and the one open red on `main` (ORCH-119) is a known contention
sighting that this batch's `.rs` changes do not touch.
```

## Risks and findings

1. **The hook that will judge this push is `main`'s copy, not this
   tree's, and it still carries the defect this batch fixes.**
   `core.hooksPath` is the absolute
   `/mnt/ssd_980/herd-project/scripts/githooks`, so the **hook body**
   comes from the orchestrator's checkout at `7eb9585` and has the
   `pre-push:291` pipe. Its `root` is `git rev-parse --show-toplevel`,
   so the `gate.sh` it invokes and the receipts it reads are **this
   worktree's**, the new ones — which is the only reason the gate can be
   verified at all, since `scripts/expected-skips/local.txt` exists only
   on this batch and `main`'s `gate.sh` therefore cannot run at rest.
   The record above is about 12.7 kB. Measured on this host on
   2026-10-28: the defect was 0/200 at 6 051 bytes at rest **and** under
   load1 12-16, 0/200 at 62 291 bytes **at rest**, and 4/200 at 62 291
   bytes only under load; the host is at load1 0.55. So the measured
   risk at this size and this load is nil. **If it fires anyway its
   signature is unmistakable** — a refusal naming a `## Section` that
   `git log -1 --format=%B` shows is present — and the answer is to stop
   and report it, never `--no-verify`, never a force, never a blind
   retry. What happened is in the report.
2. **`docs/status.md` is at 90 lines, the limit, with zero headroom.**
   The next change to it is not a free edit; something comes out. Same
   shape as ORCH-129 for `CLAUDE.md`.
3. **This batch's `.rs` changes are `herd-hub` only and do not touch
   `announced_job_ceiling`**, so ORCH-119's red on `main` is not
   addressed by the landing and could reappear on this landing's own
   post-push run. That would be a second sighting of a known row, not a
   regression of this batch, and it is reported as whatever it is.
4. **A green gate here is evidence from one host.** It is one cgroup
   slice, not a host (D187); the twelve skips mean twelve confinement
   assertions behind invariant 2 were **not** checked; and the release
   path, a pristine checkout and GitHub's own drift are outside it
   (D190's own *what it cannot cover*). The post-push run is now the
   only thing a second machine ever says about a landing, and there is
   still no nightly pass — a known gap, and a larger one since
   2026-10-28.
5. **`orch-125`'s own suite run was red on its branch**, on D186's three
   `real_herd` fixtures, and its author reported it red and proved it
   pre-existing. That fix is on `main` since `ee8dec0`, which I verified
   is an ancestor of `7eb9585`, so this landing's own gate is the figure
   that counts for it.

## Decisions needed

None blocks this landing. Two are handed on:

1. **The dedup threshold** — ORCH-145, with Alexa. Recommendation: ask
   for one `herd-hub report` reading from the operator and decide with
   the number in hand; the shape proposed is "build it only if `r_file`
   is 4 or more". Fallback: leave it unrecorded; the tool is on `main`
   either way. D194 deliberately prints no threshold.
2. **ORCH-144's shape** — whether `expect_green`'s diagnostic is fixed
   or `workflow.md` gains a line. Recommendation: the diagnostic, one
   branch in `check-orchestration.sh:96-99`. Fallback: the prose. Not an
   owner's decision either way; the row carries both.

## Resume point

`main` has moved, so the next session resumes from it and not from a
branch:

```sh
git -C /mnt/ssd_980/herd-project fetch origin
git log --merges -1 --format=%B origin/main   # this landing's record
gh run list --workflow ci.yml --branch main --limit 5
```

**Read that run list before any new work is briefed**, which is now a
session-start duty: a `failure` becomes a board row first (`workflow.md`
*When the post-push run on `main` is red*). **Two reds are live on that
list already** — `40720983497` at `7eb9585` (ORCH-119, deliberately not
re-run) and whatever this landing's own run says, whose id is in the
merger's report.

**The next batch's first task is ORCH-143**, whose case text is written
and tested and sits in `history/2026-10-28-pre-push-no-pipe.md`
*Decisions needed* 1. ORCH-142 goes with it or just after it.

## Processes

**None started by this task.** No spinner, no load generator, no hub, no
runner, no `herdctl`, nothing of the consumer's. No `cargo` or
`nix develop` outside the single `./scripts/gate.sh` run named under
*Gates*, which is the foreground of its own shell and exits on its own;
its PID is in the report. Nothing was killed — no `pkill`, no `killall`,
nothing by name.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/landing-1`, outside the
worktree and off the root filesystem, and is deleted once the push is
done (`workflow.md` *Concurrency and host rules*). The integration
worktree's own `target/herd-gate/` holds the gate's log and its receipt,
which is why the worktree outlives the push; what became of it, and of
the three worktrees `wt-sigpipe`, `wt-orch-111` and `wt-pre-push` that
this landing is clearing, is in the report.
