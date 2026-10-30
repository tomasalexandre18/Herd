# Batch 26 re-gated — merge handoff and landing record

- **Item:** batch 26's landing, re-gated: a job's packs are laid out and
  registered (**D267**), E-42's detached submit asserted, a suspected
  release blocker refuted, and the one red assertion settled **at its
  fixture** (**D268**)
- **Agent:** herd-merger
- **Branch / commit:** `batch-26-regate`, this handoff's own commit on top
  of `baf5edb6` — the branch is `origin/batch-26-integration`
  (`22f9846`) fast-forwarded onto `origin/settle-owed-scan-red`
  (`771ef9a`) plus one docs commit of mine
- **Landing:** batch 26, **four branches**, one `--no-ff` merge commit,
  one push to `main`, one post-push `ci.yml` run, under standing
  authorisation ORCH-001 (Alexa, 2026-10-24, re-granted 2026-10-28)
- **No tag.** No `cut-v*` branch is in this batch, so D212's act does not
  arise. `cut-v1.0.0` is named under *Left out* with its sha and with
  both reasons its scope table is now wrong.

**This is the second merger on batch 26 and the first gate was RED.** The
previous merger integrated the three branches, allocated D267 and
ORCH-286 to ORCH-290, ran both directions of the board check, gated the
tip **red**, and then did the right thing: it left `main` at `1df941b`,
wrote no receipt, did not resubmit the gate and did not touch a test.
That handling is the standard and this handoff does not improve on it.

## The date, and the earlier handoff's landing line

`docs/orchestration/history/2026-10-30-merge-batch-26.md`, committed on
this branch at `22f9846`, carries *"**Batch:** 26, landed 2026-10-30 by
herd-merger, three branches"* although **no landing happened when it was
written**. Its author correctly did not amend it — a handoff is committed
at step 6 and never touched again — and flagged it for whoever re-gated.

**Checked here, as asked.** This handoff is written on **2026-10-30** at
20:4x local time and the push follows within the hour, so that line
becomes **true on its date** and **false only in its branch count**: the
landing is **four** branches, not three. That one word is the whole
error, it is named here rather than left for a reader to find, and the
merge message says four in every place it counts them. If the push slips
past midnight the **date** in both files is one day early; the merger's
report carries the true push timestamp, and that is the only place it can
be, because this file is committed before the gate runs.

## What this file cannot carry, and where each of those lives

This handoff is the **last change to the tree the gate runs on**
(ORCH-121), so everything below happens after it is committed:

| deferred | where it is |
|---|---|
| the gated sha as 40 hex | the merge message; it is the sha of this file's own commit, which does not exist while this file is being written |
| the gate's verbatim verdict line | the receipt, and the merger's report |
| the receipt's path under `target/herd-gate/` | named below by its shape; the store is per worktree and nothing was copied in from any branch |
| the skip **list** as run, and the at-rest `skip-list-sha256` | the receipt and the merger's report; the list decides, never a count (D27, D179, D187) |
| whether the two load-gated flood benches declined this time | the merger's report |
| the Herd job id the gate ran under, its wall time and its `bytes_written` | the merger's report |
| the merge commit sha and this push's `ci.yml` run id | the merger's report, and the **next** landing's board edit |
| what the fast-forward did to the orchestrator's checkout | the merger's report; it happens after `main` has moved |

- **Batch:** 26, **four branches**, landed 2026-10-30 by herd-merger
- **Gate:** `./scripts/gate.sh` on `baf5edb6` plus this handoff's own
  commit, which is the merge's second parent and the sha whose receipt
  will be written as that sha plus `.receipt` under `target/herd-gate/`
  in this worktree and nowhere else.
  `git diff --quiet` between that sha and the merge commit is required to
  be silent, and the merge message carries both resolved. Run **inside a
  Herd job** submitted with `/mnt/ssd_980/herd/queue/bin/herdctl` and
  never a bare `herdctl`, `--label build --memory-mib 4700`, with
  `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/batch-26-regate` exported
  inside the job — which also keeps this submission from being coalesced
  into an identical one — the job's command entering this worktree so the
  receipt lands where `pre-push` looks, and `./scripts/gate.sh` run
  **bare and whole** inside it, never nested in `nix develop` and never
  one job per step: `gate.sh` writes its receipt at its own step 8, after
  step 7 has judged step 6's skip lists, so a per-step job cannot produce
  one. **A red step blocks the landing, is reported red, and `main` does
  not move**, so this line states the sha and never a verdict it cannot
  yet have
- **Skips:** the at-rest expectation is **12** runtime `HERD-SKIPPED`
  assertions, the standing cgroup-delegation twelve, with the `#[ignore]`
  census **COMPLETE at 38** — all twelve named under *Evidence* below and
  printed as skips, never counted into a pass (D27, invariant 5). The two
  load-gated flood benches, `flood-fifty` and `flood-two-fifty`, decline
  above a one-minute load of 8, which makes the list **14**; the gate
  compares the **list** and never the count (D187, D179), and when that
  branch fires the at-rest `skip-list-sha256` is unreachable, which is
  expected and is **not** a failure. **`settle-owed-scan-red` adds no
  skip and none of the twelve is in `owed_scan_fails`**, so a green there
  is not bought with a skip — which is the first thing to check about a
  landing that follows a red gate
- **CI run:** none yet — this push starts it; its id goes in the merger's
  report and is recorded by the **next** landing's board edit, never by a
  second push for this batch. Batch 25's run **41100568813** was re-read
  by id at this integration and is `completed` / `success`, so no board
  row is owed on it and this landing's board edit records it

## What changed and why

**M11's layout step, which is the batch's one product change; two
measurements that cost no production code; and the settlement of the one
red assertion, which cost no production code either.** This moves M11 and
it touches E-12 and E-17; E-42's own recount is deliberately not here.

`herdctl` read `needs` and `leaves` off `commands.toml`, validated them
and threw them away, and nothing under `herd-runner/src` ever constructed
`ToHub::RegisterPack` — two breaks in one chain, ORCH-274 and ORCH-260.
Both halves land together, in one branch, because filling the fields
alone would make every declared command that names `needs` wait at a
dispatch where nothing registers a pack (**D267**).

**What `main` can do now that it could not before:** a declared command
leaves a pack, the hub registers it on `(commit, key)`, a later declared
command at that commit is laid out with it under `{in}`, and its own tool
reads the bytes — proved against the real hub, runner and `herdctl` as
processes in `herd-hub/tests/pack_chain.rs`.

**What it still cannot do, and the record says so rather than implying
otherwise:** the transfer in that proof is `SameHost` and **nought bytes
off the network**. The hub mints no transfer capability —
`ToHub::AskTransferCapability` is refused by name,
`FromHub::ExpectTransfer` is sent by nothing, and
`endpoint::Granted::pushed_by_hub` has no production feeder — so **a pack
cannot move between two machines at all**, which is E-12's whole point.
That is **ORCH-286**, filed by the previous merger because no row named
it, and it is the critical path.

**And the red gate, which was neither this batch's fault nor the
product's.** `owed_scan_fails.rs`'s
`one_failed_listing_silences_every_runner_and_each_is_still_answered`
breaks a listing by leaving the hub's `jobs` directory at `0o111`, and
**Herd's own runner un-chmodded the fixture**: `workspace.measure()` runs
every 200 ms (`herd-runner/src/job.rs:1555`,
`herd-runner/src/confine.rs:106`), walks the job's whole base including
`tmp/` — which is the suite's `$TMPDIR` — and any directory it cannot
list goes through `restore_access`, `set_permissions(mode | 0o700)`. The
fixture's `0o111` became **`0o711`** within 200 ms, the hub's scan then
succeeded, and it correctly asked both runners to resume. **`0o711` where
the fixture set `0o111` is the fingerprint.** The trigger is job
**duration**, not concurrency: the red gate job ran 390 s, about 1950
walks; an isolated control ran 0.3 s and was green five of five. The
product is doing exactly what invariant 2 and D166 require of it, **no
product file was changed and none is wanted**, and `herd-hub/src` is
untouched by this whole batch. **D268** states the rule the fixtures must
keep, and `settle-owed-scan-red` guards the premise of the two hub tests
of it without loosening a single assertion.

**The one thing in this batch that is bigger than this batch.** The
settlement found that this is a **class, not one test**, and that one
member is a **hollow green**:
`herd-runner/tests/confinement.rs`'s
`a_job_cannot_hide_what_it_wrote_from_its_disk_budget` runs
`chmod 300 h; sleep 6` — a six-second premise against a 200 ms walk — so
**run through Herd it no longer exercises the defect it is named for and
would stay green with `restore_access` removed from the product**. It
asserts `StoppedByCap { Disk }`, which holds either way. **A green suite
through Herd therefore does not today exercise that one assertion**, and
no report, including this one, may be read as if it did. Four more
members have sub-millisecond windows, several siblings are latently red,
and the whole inventory is **ORCH-291** with the hollow green as
**ORCH-292**. Its fix is on another branch and is **not in this batch**.

## Branches in this batch

In the order they were merged, which is also the order the evidence below
is read in. Each branch's handoff is its own file and this record never
blurs the four into one summary.

1. `m11-pack-layout`, tip `ae1188f`, base `df47266` — **ORCH-274** and
   **ORCH-260**, M11's `{in}`/`{out}` layout step.
   `docs/orchestration/history/2026-10-30-m11-pack-layout.md`
2. `e42-producer-submit`, tip `c47a72a`, base `df47266` — **ORCH-148**'s
   measurement; **tests and prose only, no `src` file in any crate**.
   `docs/orchestration/history/2026-10-30-e42-producer-submit.md`
3. `leaves-naming-a-file`, tip `673a4a7`, base `df47266` — a measurement
   that refuted a suspected release blocker; **one handoff and nothing
   else**.
   `docs/orchestration/history/2026-10-30-leaves-naming-a-file.md`
4. `settle-owed-scan-red`, tip `771ef9a`, base `22f9846` — the
   settlement of batch 26's red gate (ORCH-197's test, D225's fixture);
   **tests and prose only, no `src` file in any crate**, and it carries
   D268's drafted entry and ORCH-291's and ORCH-292's drafted rows.
   `docs/orchestration/history/2026-10-30-settle-owed-scan-red.md`

The first three are based on `df47266`, one landing behind `main` at
`1df941b`. The fourth is based on the integration tip of the first
three, which is why it merged as a fast-forward and why its changed-file
set could not collide with theirs.

**ORCH-274 and ORCH-260 close; ORCH-220 does not** — it is the push
*direction*, plan S10, and stays `owed`. The layout obligation itself was
named inside **ORCH-234**, a row already `done`, and never had a row of
its own, so nothing else closes here.

## Evidence

**Each branch's own output, separately.** A batch is not permission for
one blurred summary, so the four verdicts are read one at a time before
the batch's own run on the merged tree.

### 1. `m11-pack-layout`, on its own base

Five gates green on `ae1188f`, run through Herd, one job per phase,
`--label build --memory-mib 4700`, `./scripts/bench-preflight.sh`
cleared before each. Verbatim output and figures are in that branch's own
handoff, which this file does not copy:
`docs/orchestration/history/2026-10-30-m11-pack-layout.md`. The chain was
watched **red first in both halves** — the layout removed, and the
registration removed — because a green end-to-end chain is the one place
this could have passed while proving nothing.

### 2. `e42-producer-submit`, on its own base

Five gates green on `c47a72a`; `herdctl/tests/detached_dedup.rs` is the
new assertion and no crate's `src` is touched. Verbatim output in
`docs/orchestration/history/2026-10-30-e42-producer-submit.md`.

### 3. `leaves-naming-a-file`, on its own base

A measurement, `673a4a7`, that refuted a suspected release blocker: a
`leaves` naming a plain file does **not** stop that file coming back as
an ordinary `{out}` output. One handoff and nothing else in the tree, so
the Rust gates had nothing of its own to judge; its own run and reasoning
are in `docs/orchestration/history/2026-10-30-leaves-naming-a-file.md`,
and the row it produced is **ORCH-289**, filed `done`, with **ORCH-290**
in *Waiting* for Alexa.

### 4. `settle-owed-scan-red`, on the integration tip

All five gates green on the **committed** tree
`7e5abd5345da8c4fd2bb4d1cca908317994a7f52`, `git status` clean, in Herd
job `j0000000155`, before its handoff was added:

```
tree sha: 7e5abd5345da8c4fd2bb4d1cca908317994a7f52
GATE1 exit=0   cargo fmt --all --check
GATE2 exit=0   cargo build --workspace
GATE3 exit=0   cargo clippy --workspace --all-targets -- -D warnings
GATE4 exit=0   ./scripts/test.sh
   12 assertion(s) COULD NOT RUN on this host and were skipped.
   ignored census: COMPLETE (38 ignored, 0 without a reason)
   test suite: PASSED
GATE5 exit=0   ./scripts/check-docs.sh  ->  docs ok
RATE: 0 red of 200 whole-binary runs inside this Herd job
```

**The fix is proved both ways, which is what makes it a fix and not a
quietening.** On the **unmodified gated binary**
(`owed_scan_fails-045c1bc641b099f5`, byte-identical to the gate's), under
a faithful stand-in for `restore_access`: **10 red of 10**, with the
gate's own message byte for byte. On the fixed one: **0 red of 10** alone
and **0 red of 10** as a whole binary with the restore still firing every
run, plus **0 red of 200** whole-binary runs inside a Herd job. And on a
host that restores faster than the fixture can set, the fixed test goes
**red with a named sentence** — not skipped, which would have moved the
`HERD-SKIPPED` list.

The mechanism itself is measured separately and deterministically, 1 of 1
in job `j0000000147`: `mode=2111 read_dir=EACCES` at t=0.10 s,
`mode=2711 read_dir=ok` at t=0.20 s, on a directory under that job's own
`$TMPDIR`.

**What is still not proved, and the sceptic's question is left standing:**
the gate's own red job workspace is gone, so its mode cannot be read
back, and what is shown is that this mechanism produces that exact
failure — not that no second unknown mechanism with the same fingerprint
exists.

### 5. The batch, on the merged tree

One `./scripts/gate.sh` run, whole and bare, inside one Herd job, on the
sha named on the **Gate:** line above — `baf5edb6` plus this handoff's
own commit. Its verbatim verdict, its skip list as run, the receipt path
and the job's figures are in the receipt, in the merge message and in the
merger's report, and **cannot be here** for ORCH-121's reason: the gate
must run on the tree that is pushed, so nothing may be added to that tree
after this file is committed.

**The twelve `HERD-SKIPPED` assertions expected at rest, by name**, each
printed as a skip and never counted into a pass (D27, invariant 5):

```
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

and the two that join them above a one-minute load of 8, making the list
14 and never a count of 14:

```
HERD-SKIPPED: flood-fifty
HERD-SKIPPED: flood-two-fifty
```

**All twelve are the assertions nearest `herd-runner`'s cgroup path, and
a skip is not a proof** (D27). This batch's one product change is
runner- and CLI-side, so the question whether any of the twelve covers it
is answered under *Risks* and never assumed here. **None of the twelve is
in `owed_scan_fails`**, so the assertion that was red is one this host
really runs, and its green is a green.

### 6. The board check, both ways — and this shape needs it most

**No branch of this batch touched `docs/orchestration/board.md` at all.**
Every board change in this landing is a merger's own allocation, which is
exactly the case `workflow.md`'s *One landing per batch* says needs the
check most: nothing collided, so nothing would have complained.

Direction one, **every non-blank line of each branch's `board.md` present
verbatim in the merged file**, with the four lines that are not, named:

```
m11-pack-layout:       4 of 891 non-blank lines not verbatim
e42-producer-submit:   4 of 891
leaves-naming-a-file:  4 of 891
settle-owed-scan-red:  4 of 1016
```

For the first three the four are `ORCH-253`, `ORCH-148`, `ORCH-260` and
`ORCH-274`. Each is accounted for and none is an accident: `ORCH-253`'s
branch line is **not** on `origin/main`, so batch 25 restated it and
these branches are one landing behind; `ORCH-148`, `ORCH-260` and
`ORCH-274`'s branch lines **are** still on `origin/main`, and they are
the previous merger's three deliberate restatements, committed in
`48e7c38` (ORCH-274 and ORCH-260 to `done`, ORCH-148's next action
extended and still `open`). For `settle-owed-scan-red` the four are the
four lines **this** merger restated, all inside item 22 of *Next*.

Direction two, **the merged file differs from each branch only by the
other branches' hunks and by the merger's own allocations**: 170 changed
lines against each of the first three, 45 against `settle-owed-scan-red`
— and those 45 are the whole of this landing's own board edit, namely the
two new rows, item 22's branch count, item 22's batch-25 CI clause and
item 22's re-gate addendum.

**The detector was proven before its silence was trusted.** A row no
branch ever wrote, `ORCH-777`, was planted in the input and direction
one named it alongside the four real lines. A check only ever seen quiet
is not known to work.

### 7. Every branch left its handoff

All four are in `docs/orchestration/history/` on the merged tree:
`2026-10-30-m11-pack-layout.md`, `2026-10-30-e42-producer-submit.md`,
`2026-10-30-leaves-naming-a-file.md`,
`2026-10-30-settle-owed-scan-red.md`, plus the previous merger's own
`2026-10-30-merge-batch-26.md`, which stays in the tree exactly as it was
committed and whose one wrong word is named at the top of this file.

## Left out

Each with its sha, because a reader must be able to tell a deliberate
omission from a forgotten branch.

- **`cut-v1.0.0`, `f3e9df0`** — left out again, for **two** reasons now:
  it carries an **unsigned release body** (**D210**: a release's notes are
  Alexa's own act), and its scope table is wrong **twice over** — it
  counts **E-42 as nothing built** when E-42's dedup rule is on `main`
  (ORCH-288), and **P5 landed in batch 25**, which moves E-16's reading.
- **`e42-recount-part`, `7cd9902`** — green, for the next batch. E-42's
  recount is one ruling and landing it twice is the one way to get it
  wrong.
- **`m12-p6-no-holder-named`, `abac501`** — green, for the next batch.
- **`orch-282-weigh-records`, `135d1ab`** — green, for the next batch.
- **`m12-hub-mints-transfer`, `20150d4`** — green, for the next batch,
  and it is the branch that answers ORCH-286, this landing's own critical
  path.
- **ORCH-292's fix** — the hollow green is on its own branch and is
  deliberately not here; it is a product-facing question about how a test
  proves the runner restored when something else would have, not a
  fixture tidy-up.
- **The sentence `settle-owed-scan-red` drafted for
  `docs/orchestration/workflow.md`** — a prose addition by the *Execution
  goes through Herd* bullet, drafted verbatim in that branch's handoff and
  marked by its author as not its own to place. It is **not placed here
  either**: another agent holds `orch-282-weigh-records`, which is in that
  same file, and a merger who writes prose into a file an unlanded branch
  is editing manufactures the conflict the batch did not have. The draft
  survives verbatim in the committed handoff that lands with this batch,
  so nothing is lost; placing it is a prose task on `workflow.md` once
  ORCH-282's branch has landed.
- **`docs/status.md` grew by nothing** — see *Risks and findings* 3.

## Decisions allocated

One, in merge order, for the whole batch.

- **D268 — A suite fixture that sets a mode must guard its premise**
  (2026-10-30), from `settle-owed-scan-red`'s drafted `D-NEXT` entry.

**How the number was measured, not assumed.** D268 is the next free
number above **D267**, which is the highest on `origin/main` **and** on
every branch not yet landed: all **40** non-ancestor remote refs and all
**26** non-ancestor local branches were read, and none holds a `D` above
267 or an `ORCH` above 290 (`ORCH-998` and `ORCH-999` are
`check-orchestration.sh` fixtures, not rows). **No gap was closed: D143,
D154 and D170 stay** (D148, invariant 8). `./scripts/check-docs.sh` is
green, and it is what rejects a duplicate number or a malformed header.

**D267 is already on this branch** and is **not** re-allocated here: the
previous merger allocated it at `48e7c38`, and it is named above only so
that a reader of one file can see the batch's whole decision set.

**One paragraph of D268 is mine and not the drafting agent's**, and it is
marked as such here because a decision entry is otherwise that agent's
wording: the closing *"What this decision does not fix"*, which says that
only `a_failed_listing`'s two assertions are guarded and points at
ORCH-291 and ORCH-292 by their allocated numbers — numbers the author
could not know. Nothing else in the entry was reworded; the header was
reshaped from the draft's `## D-NEXT — title (2026-10-30)` to this file's
own `## YYYY-MM-DD — Dn title`, which `check-docs.sh` requires.

## Board rows

- **ORCH-291**, new, `owed`, owner `herd-step` — the inventory of D268's
  class, **every member named**: fixed in `owed_scan_fails.rs`; latently
  red and not fixed in
  `owed_scan_fails_against_the_real_runner.rs` assertions 1 and 2,
  `herd-hub/src/server.rs:11455`, `herd-runner/src/sweep.rs:708` and
  `:786`, and `herd-runner/src/root.rs:1100`; and five sites **ruled out
  and named as not members** (`durability.rs:1107`, `killed_hub.rs:192`
  and `:490`, `store.rs:4918`, `spool.rs:463`).
- **ORCH-292**, new, `owed`, owner `herd-step`, depends on ORCH-291 — the
  **hollow green**, with its four sub-millisecond siblings named. Its
  *Next action* records that branch `settle-confinement-hollow` was cut
  off this batch's tip `771ef9a` for it and **carries no commit of its
  own at this landing**, which is why the row is `owed` and not `active`.
- **ORCH-274** → `done` and **ORCH-260** → `done`, both by the previous
  merger at `48e7c38`; **ORCH-148** stays `open` with its next action
  extended by the measurement; **ORCH-220 stays `owed`** — it is the push
  direction, plan S10, and this landing does not touch it; **ORCH-286**
  `owed` (the hub mints no transfer capability — and it is **not**
  ORCH-220), **ORCH-287** `owed`, **ORCH-288** `open`, **ORCH-289**
  `done`, **ORCH-290** `waiting`, owner **Alexa**.
- **Item 22 of *Next*** restated by this landing: four branches instead
  of three, the red gate and why the product is innocent, D268 with its
  measurement, ORCH-291 and ORCH-292, and batch 25's run re-read by id.

## CI run

**None yet — this push starts it, and it is recorded, not waited on.**
Its id comes from
`gh run list --workflow ci.yml --branch main --limit 1` right after the
push, goes into the merger's report, and is recorded by the **next**
landing's board edit — never by a second push for this batch, which is
the rule that makes a red run attributable. This landing is **not judged
risky**: no product file changed since the gate's own tree, the batch
touches no `herd-hub/src`, and the one behavioural change in it was
already carried by a green CI run on batch 25's `main`.

**Batch 25's run is settled here and owes nothing.** `ci.yml` run
**41100568813** at `1df941b`: `status: completed`,
**`conclusion: success`**, 17:59:46Z, read by id at this integration. The
previous handoff could only read it as `queued`; this landing's board
edit carries the verdict, so **no `ORCH-nnn` row is owed on it**.

## Conflicts and resolutions

**None, in either half of the batch, and both halves are measured rather
than asserted.**

The previous merger merged the first three branches one at a time onto
`batch-26-integration` and recorded **zero conflicts** with pairwise
disjoint changed-file sets. That work is on this branch at `22f9846` and
was not redone.

The fourth branch is a **fast-forward**: `origin/settle-owed-scan-red`
(`771ef9a`) has `origin/batch-26-integration` (`22f9846`) as an ancestor,
verified with `git merge-base --is-ancestor`, so `git merge --ff-only`
moved the branch and resolved nothing. Its own changed-file set against
that tip is four files —
`docs/orchestration/history/2026-10-30-settle-owed-scan-red.md`,
`herd-hub/tests/a_failed_listing/mod.rs`,
`herd-hub/tests/owed_scan_fails.rs` and
`herd-hub/tests/owed_scan_fails_against_the_real_runner.rs` — and none
of the other three branches touches any of them.

**Nothing in this batch is a specification or an invariant change**, so
nothing here was escalated. The one thing that could have been — the red
assertion — was settled **in the fixture** and not in the product, and
the product was left exactly as invariant 2 and D166 require it to be.

## Gates

**One `./scripts/gate.sh` run on a clean tree, and it is the last thing
that runs before the merge.** The receipt it will write is
`target/herd-gate/` plus the sha of this handoff's own commit, with a
`.receipt` suffix, in **this** worktree and nowhere else: the receipt
store is per worktree (`$HERD_GATE_RECEIPTS`, default
`target/herd-gate/`), a green gate does not travel between worktrees, and
**no receipt was copied in from any branch** — `settle-owed-scan-red` ran
its five gates as named steps inside a job and wrote no `gate.sh` receipt
at all, and a copied receipt would name a sha and prove nothing about the
tree being pushed. That sha is the merge's **second parent**, and
`./scripts/gate.sh --verify-receipt` is what `scripts/githooks/pre-push`
asks about it.

**The gate runs through Herd and not locally** (**D258**): submitted with
`/mnt/ssd_980/herd/queue/bin/herdctl`, **never bare** — a bare `herdctl`
has `ws://127.0.0.1:8787` compiled in, which on this host is the
**consumer's production hub**, so a forgotten `--hub` submits to their
production — with `--label build`, `--memory-mib 4700` and
`--verdict-to`, under
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/batch-26-regate` exported
**inside** the job. The memory figure is a measurement: a gate's whole
process tree peaks at **4.46 GiB**. The distinct target directory is also
what keeps this submission from being coalesced into an identical one.

`./scripts/bench-preflight.sh` runs **before** the submission and **its
refusal is obeyed**. `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` is **never**
set or exported.

**One build slot and two step agents were running when this re-gate
started**, so **a wait was expected and no submission was repeated**. A
capped job has no verdict and no receipt and would be reported as a
block, not as a red gate. **If this gate is red again it is reported red
with the failing assertion's name, `main` does not move, the gate is not
resubmitted and no test is touched** — which is what the previous merger
did, and it is the standard.

**The verbatim output, the skip list as run and the job's figures are not
in this file and cannot be**, for ORCH-121's reason. They are in the
receipt, in the merge message and in the merger's report.

**The first gate of this batch was red and its own log is gone.**
`gate.log` was deleted with a build tree; its substance survives in
`gate-job.log` and the full 160 KB of test output is preserved outside
any build tree, under the settling agent's `gate-evidence-batch-26/`
directory in this session's scratchpad. This merger's own evidence is
moved out of its build tree **before** the tree is deleted, for the same
reason.

## Risks and findings

1. **The hollow green is the finding that outlives this batch, and it is
   stated in the landing record rather than left in a handoff.** A green
   suite through Herd does **not** today exercise
   `a_job_cannot_hide_what_it_wrote_from_its_disk_budget`'s assertion:
   its premise is exposed for six seconds against a 200 ms walk, so the
   outer runner restores the mode long before the inner runner measures,
   and the test would stay green with `restore_access` removed from the
   product. That is a test proving nothing it claims, it is **ORCH-292**,
   and a record that hid it would be the first dishonest one today. Four
   more members have sub-millisecond windows — a small hole in the wrong
   direction rather than a hollow test — and it is a **code reading**: no
   run was made with `restore_access` removed.
2. **A gate run through Herd is not equivalent to a gate run locally for
   this class of assertion, and that is the measured cost of D258.** The
   same tree is sound locally and unsound through Herd. This is reported,
   not litigated: D258 is today's own directive, the product is right,
   and the fixtures are what move.
3. **`docs/status.md` grew by nothing and that was a decision.** It sits
   at its hard ceiling of **90** lines and was already restated for this
   batch by the previous merger, so D268's fact was fitted into the
   existing *narrower than their names* sentence rather than added to it.
   Three things were traded out to pay for it, all of them method that
   `CLAUDE.md` and `workflow.md` hold in full by that paragraph's own
   statement: *"protection that does not bind us"*, *"the skip list,
   never the count"* (the **D187** pointer stays, and the skip figures
   stay two paragraphs below), and D249's sentence about how the consumer
   exchange is opened, compressed to *"the exchange is ours to open,
   unanswered"*. Nothing measured was removed. **The next restatement
   should be a restatement and not a patch**: this file is now the third
   landing in a row to pay for a line at that ceiling.
4. **The twelve skips sit nearest `herd-runner`'s cgroup path and this
   batch's one product change is runner-side**, so the question has to be
   asked rather than waved away. None of the twelve touches pack layout,
   pack registration or `herdctl`'s declaration path: they are cgroup
   delegation, io.max, cpu.max, pids.max and pressure, all of which
   require a delegated scope this host cannot give. The layout step's own
   assertions are in `pack_chain.rs`, `declared.rs` and `fixture/mod.rs`,
   and every one of them ran.
5. **ORCH-286 is the critical path and `m12-hub-mints-transfer` is
   sitting green and unlanded.** The record above is careful not to be
   read as *"a pack travels"*; the branch that would make it travel is
   named under *Left out* with its sha.

## Resume point

`main` has moved to this landing's merge commit, the post-push `ci.yml`
run id is in the merger's report, and the next batch is already stacked:
`e42-recount-part` (`7cd9902`), `m12-p6-no-holder-named` (`abac501`),
`orch-282-weigh-records` (`135d1ab`) and `m12-hub-mints-transfer`
(`20150d4`), all green and all for the next merger. Three things that
batch should carry and this one could not: the `workflow.md` sentence
drafted by `settle-owed-scan-red`, a restatement of `docs/status.md`
rather than another patch at its ceiling, and the record of **this**
push's `ci.yml` run in its board edit.

## Processes

- One Herd job, the gate, submitted through
  `/mnt/ssd_980/herd/queue/bin/herdctl` and never bare, `--label build
  --memory-mib 4700`, with its own `CARGO_TARGET_DIR` so that it could
  not be coalesced into another identical submission. Its id, wall time
  and `bytes_written` are in the merger's report.
- `./scripts/bench-preflight.sh` before the submission, its refusal
  obeyed, and `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` never set or exported.
- No `cargo` outside that job, no second compile, nothing killed by name,
  no `pkill`. The only process ids read were the queue's runner, our hub
  and the consumer's two, and none of them was signalled.
- The orchestrator's checkout at `/mnt/ssd_980/herd-project` was used for
  one `git worktree add` and one `git fetch`, which are not changes to
  what it says, and for step 10's `--ff-only` **after** `main` had moved,
  with both preconditions checked first and the result printed. What that
  did is in the merger's report.
