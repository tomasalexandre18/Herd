# board-two-rows — handoff 2026-10-30

- **Item:** ORCH-235, ORCH-236, ORCH-237 and ORCH-238, all four
  allocated by this task, in two instructions and two board commits
- **Agent:** herd-step
- **Branch / commit:** `board-two-rows`, based on `origin/main` `6d10427`
  (*Land batch 15: the hub answers its pack frames, and a withdrawal*).
  `de01647` files ORCH-235 and ORCH-236; `f79f70e` is the first version
  of this file; `12f5fc3` files ORCH-237 and ORCH-238 and amends
  ORCH-235's note; the extension of this file is the commit on top of
  that. **One branch, one push, nothing landed.**
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`
- **Prose only.** Two files changed, four board rows added and one
  amended, nothing outside `docs/`. No code, no `docs/decisions.md`, no
  `docs/status.md`, no tag, no merge, no pull request.

## Done

### 1. The numbering, verified before anything was written

`ORCH-234` is the highest id in the file, which was measured and not
assumed:

```sh
$ grep -oE 'ORCH-[0-9]+' docs/orchestration/board.md \
    | sort -u -t- -k2 -n | tail -3
ORCH-232
ORCH-233
ORCH-234
```

So **ORCH-235** and **ORCH-236** are the next two free numbers. No `D`
number was allocated and `docs/decisions.md` is untouched: it is
`herd-merger`'s file alone.

The second instruction's pair was numbered the same way and separately:
after `de01647` the highest id in the file was **ORCH-236**, so
**ORCH-237** and **ORCH-238** are the next two free. Checked against the
file and not against the first measurement, because the file had moved
in between — which is the whole subject of ORCH-237.

### 2. ORCH-235 — a new test file breaks silently on a widened struct

Filed **`open`**, owner `herd-planner`, in *Open — seen, not agreed or
sized*, directly after ORCH-231. `open` rather than `owed` because the
orchestrator's call of 2026-10-30 was the call to **file** the row; the
fix itself is batch 14's merger's proposal and is neither agreed nor
sized — where test-side constructors live is a real design question,
since `#[cfg(test)]` does not cross a crate boundary.

Its three gates were each re-checked against the tree rather than copied
from the brief:

| the gate | what the tree says |
|---|---|
| batch 13 | `history/2026-10-30-merge-batch-13.md:328-331`: `error[E0063]`, `mesh_endpoint` on `ToHub::Hello` **and** `pack_store_budget_bytes`, `pack_store_bytes`, `packs_held` on `ToHub::Health`, in the new file `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs`. The brief named three of those four fields; the row names all four and the two variants they sit on |
| batch 14 | `history/2026-10-30-merge-batch-14.md:246-252` and `:270-276`: `keeper_for` on `FromHub::Welcome` at `herd-hub/tests/silent_hub.rs:196`. The merger's both-ways measurement reproduced here: `grep -c keeper_for herd-proto/src/lib.rs` is **1** on `main` and **0** at `ebbfdd8^`, and `git show ebbfdd8 -- herd-proto/src/lib.rs` adds `keeper_for: Vec<String>` at its line 440 |
| `pack-fetch` | `history/2026-10-30-pack-fetch.md:42-47`: **not a new file, and the other direction** — the *branch* widened `ToHub::Hello` and `ToHub::Health` and the file that stopped compiling was `herd-hub/tests/owed_scan_fails.rs`, already on `main`, landed by batch 11 after `pack-variants` was cut |

That third difference is in the row, because it changes what a fix has
to do: constructors have to serve both directions, not only the
new-file one. Git is silent in both — in the first two because a new
file has nothing to conflict with, in the third because the branch
never touched the file that broke.

The family is cross-referenced as the board already names it — *a rule
written in N places is fixed in N-1*, ORCH-131, ORCH-132 and ORCH-133,
all three about prose — and the row says this is that family applied to
**test data**, with three red gates as its measurement. ORCH-133's own
spelling is `N-1` with an ASCII hyphen and the new row matches it.

### 3. The orchestrator's own corrected figures, attached to ORCH-235

There is **no place on this board that records the orchestrator's own
errors**: searched for one (`orchestrator's figure`, `orchestrator's
error`, `the brief gave`, `figure in a brief`, `premise`) and the only
neighbouring thing is a per-landing *Risks and findings* entry in a
handoff. So the brief's fallback applies and it is attached to ORCH-235
as a one-line note inside the *Item* cell, in the brief's own terms and
not softened: the brief to batch 14 gave `herd-hub/tests/silent_hub.rs`
as **487 lines** when the branch's file was **539** (547 on `main`
after the eight-line resolution, which is what `wc -l` gives today), the
reason to keep is *"a figure in a brief becomes an acceptance
criterion"*, **the error is the orchestrator's and not an agent's**, and
it is the fourth of its figures corrected this session.

**A fifth was added to the same note in `12f5fc3`**, on the
orchestrator's own instruction and in its own terms: its brief for
ORCH-235 named **three** fields missing at batch 13's gate where the
error named **four** across two variants. The note now carries both,
numbered, with the sentence *"both errors are the orchestrator's, not
an agent's"*. Nothing was softened and nothing was moved out of the
*Item* cell: there is still no other place on this board for them.

### 4. ORCH-236 — a comparison that never ran

Filed **`owed`**, owner `herd-step`, in *Owed — agreed, not started*,
after ORCH-234. `owed` because the rule itself is not in question: it is
a host fact plus the project's own detector rule, and what is not
started is writing it where it will be read.

Re-measured on this host at this task, read-only:

```sh
$ command -v bc; echo "bc exit: $?"
bc exit: 1
$ command -v python3; echo "python3 exit: $?"
python3 exit: 1
$ command -v python; echo "python exit: $?"
python exit: 1
$ command -v awk
/run/current-system/sw/bin/awk
$ grep -rn '\bbc\b' scripts/ .claude/
(no output)
```

So the trap is for a brief and for a one-line shell call, not for
anything in the tree, and the row says so. `awk "BEGIN{exit !(...)}"` is
named as the replacement because `scripts/gate.sh` already uses exactly
that shape for exactly this comparison, at `:278`, `:291` and `:371`.

### 5. The gotcha was not edited, and that is a deliberate judgment

The brief left to me whether the `bc` sentence is worth a line **inside**
`docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`.
It is — and it is **not added on this branch**, because that file is
already being rewritten on an unlanded branch:

```sh
$ git log --oneline main..gotcha-pipe-status
d10d402 Add the pipe-status gotcha handoff
93d0901 Amend the semicolon gotcha with the pipe that hides a status
$ git show gotcha-pipe-status:docs/orchestration/gotchas/\
a-gate-joined-by-a-semicolon-is-advice.md | head -1
# A status you did not look at is a status you did not get
$ git diff --stat main gotcha-pipe-status -- \
    docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md
 .../a-gate-joined-by-a-semicolon-is-advice.md | 99 ++-
```

Two things follow, and both are in ORCH-236's *Next action*:

- **The retitle is already written and is not mine.** The brief gave the
  new title as a fact — *"retitled A status you did not look at is a
  status you did not get"* — and it is `gotcha-pipe-status`'s first
  commit, not something this task was being asked to do. On `main` the
  heading is still *"A gate joined by a semicolon is advice"*, written
  there with the character itself. Nothing here
  renames the file or the heading, and the row cites the file **by
  path**, as its six existing citations do.
- **A one-sentence edit from here would collide** with 99 changed lines
  on that branch, for one sentence. It belongs in that landing. The row
  says what the sentence must say and that `bc`'s absence is the
  mechanism, so whoever lands `gotcha-pipe-status` can write it without
  reading this handoff.

### 6. Nothing was duplicated, and nothing was amended

Checked before filing, for both rows: no existing row carries either
subject under another number. `E0063`, `new test file`, `constructor`,
`builder` and `widen` return nothing in `board.md`; `bc`, `detector` and
`plateau` return nothing either. ORCH-229 is the nearest neighbour to
ORCH-236 and is a **different** defect — a preflight refusal joined with
`;` — so it is cross-referenced as the same family and left untouched.
**No row was amended in place.** The brief's ORCH-228 precedent did not
apply because there was nothing to amend.

### 7. ORCH-237 — two branches can claim one `ORCH-nnn`

Filed **`open`**, owner `herd-planner`, in *Open*, after ORCH-235. The
three candidate shapes are stated and **not** chosen, the orchestrator's
lean to (c) is written as a lean and not as a ruling, and the harder
half — two mergers preparing batches concurrently — is recorded as the
orchestrator's own doing.

**The row changes one of the three candidates, because candidate (a)
already exists.** `scripts/check-docs.sh` already greps the board's ids,
sorts them, passes them through `uniq -d` and reports *"uses an id
twice"*. It was proved to fire rather than trusted for its silence, on a
copy of the board with ORCH-234's row appended a second time:

```text
$ grep -oE <the id pattern> dup-fixture.md | sort | uniq -d
| ORCH-234
$ grep -oE <the id pattern> board.md | sort | uniq -d
[]
```

(The pattern itself is not written here: it contains a literal pipe
character, and this file's *Drafted* section has to stay pastable.) So
the detector works, is silent on the real file, and reads **one** file —
which is why it can only speak after both rows are in one tree, that is
after both have landed. That is exactly the weakness the brief predicted
for it, and it means the candidate is not a thing to build.

**And the family is already named and already solved once.** This is the
`D`-number collision — `gotchas/decision-number-collisions.md`, whose
own observation is *"a branch allocates its number against the `main` it
saw; two branches in flight see the same `main`"*, with D145 renumbered
twice — in its board-row costume. Its fix is a provisional id plus a
**single allocator** at integration (ORCH-003, which says in as many
words that *"separate files do not stop two branches both creating
`D170`; the single allocator does"*). What defeated it here is that
there were **two allocators at once**, which is the orchestrator's own
doing and is in the row as such.

The facts of the collision were taken from the tree, not from the brief:
batch 15's merger drafted `ORCH-NEXT-A`, `-B` and `-C`; batch 14's
integration branch held ORCH-231 in a commit titled *Amend ORCH-228 and
open ORCH-231 for batch 14*; its handoff was already committed, which
under **ORCH-121** is the last change before a gate; and if batch 14 had
gone red, *"ORCH-231 is a gap and stays one"*.

### 8. ORCH-238 — a green pushed branch missed by two landings

Filed **`open`**, owner `herd-planner`, in *Open*, after ORCH-237.
**Every figure of the orchestrator's measurement reproduced exactly**,
against a fresh `git fetch` at this task:

```text
$ git log --oneline origin/main..origin/gotcha-pipe-status
d10d402 Add the pipe-status gotcha handoff
93d0901 Amend the semicolon gotcha with the pipe that hides a status
$ git diff --shortstat origin/main...origin/gotcha-pipe-status
 3 files changed, 340 insertions(+), 2 deletions(-)
$ git merge-base --is-ancestor origin/gotcha-pipe-status origin/main \
    && echo YES || echo "NO - not an ancestor"
NO - not an ancestor
$ # per-branch --is-ancestor over every origin ref but main and HEAD
TOTAL not ancestors of main: 31
```

**2 commits, 340 insertions, 3 files, not an ancestor** — and **31**
rather than 30 because `origin/board-two-rows`, the branch carrying this
file, had joined the list between the orchestrator's 06:57 reading and
mine. 31 minus that one is the orchestrator's **30**, confirmed; the row
says so, because a count that moves when the counter pushes is worth
stating once rather than reconciling later.

**One figure the row adds: the clock.** All times are committer times on
2026-10-30, `+01:00`, measured with `git log -1 --format=%cI`:

| when | what |
|---|---|
| 06:07:50 | `93d0901`, the gotcha amendment |
| 06:09:26 | `d10d402`, its handoff — the branch complete |
| 06:21:15 | `49fb201`, batch 15's board restate |
| 06:30:40 | `d428bf4`, **batch 14 lands** |
| 06:32:46 | `b14401e`, batch 15 rebuilt on batch 14's landing |
| 06:34:08 | `90ed7d6`, batch 15's handoff, its last change before its gate |
| 06:43:30 | `6d10427`, **batch 15 lands** |
| 06:55, 06:57 | the orchestrator's two readings of the origin |

So the branch's two commits existed **21 minutes** before batch 14's
landing and **25** before batch 15's handoff. **What cannot be measured
from this host is the push time** — an origin ref has no reflog here —
so the row states the commit times as a **floor** and claims no more
than a floor. That distinction is the whole difference between a listing
error and a timing accident, and it is left visible rather than
resolved.

**Also measured, because the brief asked for it to be said plainly:**
`gotcha-pipe-status` is the only unreachable origin branch with a
2026-10-30 tip apart from `board-two-rows` itself — the next most recent
is `merge-queue-target-deleted` at 2026-10-27 — so *the only unlanded
branch of this session's own work* is a measured statement and not a
turn of phrase. And **batch 15's own method is quoted from its *Left
out* rather than characterised**: *"the only branches on the origin newer
than `main` were `pack-wire` and `runner-silence`"*, which is recency.
Batch 14's handoff names no left-out branch of any kind, which is why
the row says batch 15's naming of it — with a wrong reason — is the
behaviour worth making the rule.

## Not done, and why

- **The sentence inside the semicolon gotcha** — judged worth writing and
  deliberately left to `gotcha-pipe-status`'s landing, for the collision
  reason in *Done* 5. ORCH-236 carries the text it has to say.
- **The retitle of that gotcha** — already committed on
  `gotcha-pipe-status`; not this task's to make, and `main`'s heading is
  unchanged here.
- **`docs/decisions.md`** — untouched, no `D-NEXT` drafted. The brief
  asked for board rows only and that file is `herd-merger`'s alone.
- **`docs/status.md`** — untouched, and it is at its 90-line ceiling. Two
  new board rows make no sentence in it false: it describes the state of
  the software, and nothing about the software changed.
- **ORCH-232's row** — not touched, not read into any edit. The
  concurrent `runner-announces` step owns its state cell.
- **The four Rust gates** — **NOT RUN** locally, see *Gates*.
- **A mechanical guard for either row** — sized in both rows and taken in
  neither: for ORCH-235 none is possible from a branch, because the
  breakage exists only in the merged tree; for ORCH-236 a
  `check-orchestration.sh` case on `bc` under `scripts/` would guard a
  relapse and fix nothing, the count being zero today.
- **Nothing was built for ORCH-237 or ORCH-238 either.** Both are rows
  that state candidate shapes and pick none: ORCH-237's pick is
  `herd-planner`'s and the orchestrator's lean is recorded as a lean,
  ORCH-238's listing change touches `docs/orchestration/workflow.md` and
  `.claude/agents/herd-merger.md`, and **neither file was edited** — a
  method change is not a board row's to make on the way past.
- **`gotcha-pipe-status` was not landed, merged or rebased by this
  task.** It is named in two rows and left exactly as it was: landing is
  `herd-merger`'s and nothing else is this agent's to do with it.
- **No `ORCH-NEXT` placeholder was introduced**, although ORCH-237 is
  partly about the absence of one: inventing a convention mid-task is
  the kind of method change the row exists to have decided.

## Gates

**Prose only, so the four Rust gates were NOT RUN locally** — not
`cargo build`, not `cargo test` through `./scripts/test.sh`, not
`cargo clippy`, not `cargo fmt --check`. They are **not run**, which is
not the same as passed, and what covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s and not
this agent's. No compile and no suite ran on this branch, so
`./scripts/bench-preflight.sh` was not required and no heavy phase
existed to guard.

`./scripts/check-docs.sh`, run on each board edit **before** the handoff
commit that followed it — on `de01647` (ORCH-235 and ORCH-236) and again
on `12f5fc3` (ORCH-237, ORCH-238 and ORCH-235's amended note). Both runs
printed the same two lines:

```text
$ ./scripts/check-docs.sh
docs ok
EXIT=0
```

Run in addition on `de01647` as well, **not** as this task's gate and
not claimed as one, for the one number the brief asked to be verified
rather than assumed — `check-orchestration.sh` needs no toolchain (it
drives a fake cargo) and is read-only. It is run once more beside the
last `check-docs.sh` below, and that second output is in the report:

```text
$ ./scripts/check-orchestration.sh
...
ok    gate.sh redirects its stdin before its first step
ok    pre-push keeps the stdin git hands it: the ref list is its input

check-orchestration: 324 passed, 0 failed
EXIT=0
```

**324 passed, 0 failed, and no skip line at all**, which is exactly the
figure the brief carried: **no divergence to flag**. The count is printed
only by a run — the script has no static total — so it is the run above
and not a derivation.

**The last run, named and not quoted:** `./scripts/check-docs.sh` on the
tree that includes this file, from the worktree root, its output going
to a log beside it and verbatim into this agent's report —

```text
worktree: <scratchpad>/board-two-rows/wt
log:      <scratchpad>/board-two-rows/check-docs-last.log
<scratchpad> = /mnt/ssd_980/herd/claude/tmp/claude-1002/\
-mnt-ssd-980-herd-project/\
3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad
```

— and it cannot be in this file: a
handoff is itself a docs file, and a run made before it was committed
tested a different tree. **This file was committed and gated twice**,
once per instruction, and it is the second of those two runs that the
sentence above names; the first one's output is in the agent's first
report, where it said `docs ok` on the tree that carried two rows
instead of four.

**Skips: no skip.** `check-docs.sh` and `check-orchestration.sh` each
printed none, and no `HERD-SKIPPED` or `HERD-IGNORED` line was produced
by anything this task ran. No suite ran, so there is no skip list to
report and none is claimed.

**Nothing in the second instruction changed what is gated or how.** It
is two more rows in the same file plus a note amended in a third, so
`check-docs.sh` remains the whole of this task's gate and the four Rust
gates remain **not run**.

## Risks and findings

1. **Two of the brief's figures are not in the tree, and one of them
   diverges.** The `bc` finding is recorded at
   `history/2026-10-30-merge-batch-15.md` *Risks and findings* 4 — by
   **batch 15's** merger, read-only, about **batch 14's** wait loop —
   and it gives the load as *"about 2.0 to 2.9"*, not the **1.58 to
   2.5** the brief gave. The row uses the tree's figure. The brief's
   other two details are in no file on this branch: the merger's
   sentence *"a broken detector was indistinguishable from a busy host
   — exactly a zero is not a result until the detector is proven"*, and
   the proposal of test-side constructors with the reason it was not
   filed. Both reached the orchestrator in a report and not in the tree,
   so both are written into the rows in the orchestrator's words rather
   than quoted from a file a reader can open; the quoted sentence is
   therefore **paraphrased** in ORCH-236 rather than quoted, which is
   the one place the brief's wording was not used verbatim.
2. **The brief attributes the `bc` self-report to batch 14's merger; the
   tree attributes the measurement to batch 15's.** What the tree
   supports: the broken loop was **batch 14's merger's own** pre-gate
   wait, and the agent who measured, diagnosed and wrote it down was
   **batch 15's merger**, which says *"Nothing was touched and this is
   not a defect of that landing"*. ORCH-236's *Origin* says exactly
   that. If batch 14's merger also disclosed it in its own report, that
   provenance is not in any file and is not claimed here.
3. **ORCH-236 sits three lines below ORCH-232**, whose state cell the
   concurrent `runner-announces` step is editing. Two unchanged rows
   (ORCH-233, ORCH-234) separate the two changes, which is enough for
   git to merge them without a conflict; if it does conflict, the
   resolution is *take both* — their row text does not overlap at all.
   ORCH-235 is 90-odd lines away in *Open* and cannot interact with it.
4. **The pipe trap fired, and the detector caught it before the gate
   did.** ORCH-236 as first drafted carried the `bc` test as a literal
   shell snippet, so `awk -F'|'` counted **NF=11** against the required
   10. Measured before insertion, on the drafted row in a scratchpad
   file, not after a red gate. It was **reworded, not escaped** — *"a
   test that fed `echo "$l < 5.0"` through a pipe into `bc -l` and
   compared the captured output with `1`"* — and both rows then
   asserted NF=10. This is batch 14's merger's finding 1 reproduced
   exactly: the brief predicted it and the prediction was right.
5. **ORCH-235's state is a judgment that could go the other way.** It is
   `open` because the fix is unsized, while the orchestrator's message
   says *"I made it, on 2026-10-30, and this row is that call"* — read
   as the call to file the row, not as agreement to a fix whose shape is
   explicitly another agent's proposal. If the orchestrator meant the
   fix is agreed, the cell moves to `owed` and the row moves to *Owed*;
   nothing else in it changes.
6. **Neither row claims a mechanical check exists.** Both say what was
   sized and not taken, and ORCH-235 says plainly that no check run from
   a branch can catch its defect, because the broken tree is the merged
   one. That is a property of the shape, not a gap in the tooling.
7. **One candidate in the brief for ORCH-237 was already built, and the
   row says so.** The brief offered *"a duplicate-id check, which catches
   it only after both land"* as a shape to consider; `check-docs.sh`
   already has it. Nothing is wrong with the brief's judgement of its
   strength — it is exactly that weak — but a row that asked for it to
   be written would have asked for a second copy of a check that is
   already there, which is the ORCH-131/132/133 family all over again.
   Proved to fire before being described, on a doctored board (*Done* 7).
8. **ORCH-238's push time is unmeasurable from here, and the row says
   so rather than rounding it off.** The commit times put the branch 21
   minutes ahead of batch 14's landing, but an origin ref carries no
   reflog on this host, so *"it was on the origin when the batch was
   listed"* is **not** something this task can assert. The row claims a
   floor. If the push happened after batch 15's handoff was committed,
   batch 15's *"no such branch existed to take"* was true when written
   and only the **method** was wrong; the method is what the row is
   about either way, so the conclusion does not move — but the blame
   would, and that is why the gap is left visible.
9. **The count in ORCH-238 moved while this task was running.** The
   orchestrator measured **30** unreachable origin branches at 06:57 and
   this task measured **31**, the difference being `board-two-rows`
   itself. Both figures are in the row with their reason. A reader who
   re-measures tomorrow will get another number, which is a property of
   the measure and not a defect in it.
10. **The brief's quote for ORCH-237 is a close paraphrase, and the row
    uses the tree's wording.** The brief gave *"A branch listing never
    showed this — I found it by reading the host's process list"*; the
    handoff in the tree says *"an unlanded integration branch in another
    worktree is not something a branch listing shows: it was found by
    reading the host, not by reading the origin"*. Same claim, and the
    *process list* detail is in no file, so the row quotes what a reader
    can open. Third time this task took a file over a report.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: the brief asked for none, no `D` number
was allocated, and that file is `herd-merger`'s alone.

**All four board rows are already applied** on this branch, at
`docs/orchestration/board.md`: ORCH-236 after ORCH-234 in *Owed* and
ORCH-235 after ORCH-231 in *Open*, both in `de01647`; ORCH-237 and
ORCH-238 after ORCH-235 in *Open*, in `12f5fc3`. They are reproduced
here in full and verbatim, as the template requires, so that a conflict
can be resolved from this file without reading a report. Each is one
line and asserts `NF=10` under `awk -F'|'`; **none contains a literal
pipe character**, which is why two shell patterns in them are described
in words rather than written out.

**One row was amended in place, in `12f5fc3`:** ORCH-235's *Item* cell,
whose note now carries **both** of the orchestrator's corrected figures
instead of one. The verbatim text below is the amended row — the
post-amendment version is the only one that should be pasted anywhere.
Nothing else in that row changed, and no other existing row was touched;
**ORCH-232's state cell was not read into any edit.**

**ORCH-236, in *Owed — agreed, not started*, immediately after
ORCH-234:**

| ORCH-236 | **A comparison that never ran is as silent as a status nobody looked at: there is no `bc` on this host, so a load-gated wait loop waited its whole cap.** Batch 14's merger guarded its own pre-gate wait with a test that fed `echo "$l < 5.0"` through a pipe into `bc -l` and compared the captured output with `1`; `bc: command not found` printed on every iteration, the test was therefore false at every reading, its consecutive-clear counter never advanced, and the loop idled the **full 25 minutes** it had set itself at a one-minute load of about **2.0 to 2.9** — readings its own condition would have cleared within a minute. It **failed safe**: all it ever did was wait, and the window it bought itself happened to be at rest, so no landing has a defect from it. **It is a merger reporting its own tooling against this project's own rule**, which is what makes it worth a row: a broken detector was indistinguishable from a busy host, and a zero is not a result until the detector is proven. Confirmed again on this host at this task, read-only: `command -v bc`, `command -v python3` and `command -v python` all return nothing, while `awk` is at `/run/current-system/sw/bin/awk`; and `bc` appears in no file under `scripts/` or `.claude/`, so this is a trap for a brief and for a one-line shell call rather than a defect in the tree. Same family as the semicolon and the pipe (ORCH-229): **a shell construct that fails open and prints its failure where nobody is reading** | measured read-only by batch 15's merger while deciding whether to race batch 14, `history/2026-10-30-merge-batch-15.md` *Risks and findings* 4, 2026-10-30, on batch 14's own wait loop; filed on the orchestrator's instruction, 2026-10-30 | herd-step | owed | **Two sentences, in force from now whether or not this row has moved.** (1) Anything load-gated on this host — a brief, a wait loop, a script — compares with **shell integer arithmetic or `awk`**, never with `bc`: `awk "BEGIN{exit !(...)}"` is what `scripts/gate.sh` itself uses for exactly this comparison, at `:278`, `:291` and `:371`. There is no `bc`, no `python3` and no `python` on this PATH. (2) A wait loop that never fires is indistinguishable from a host that never clears, so such a loop **proves itself on a reading it must accept** before it is trusted to reject one: one printed iteration against a value the condition has to clear. **Where the prose goes, and why not here:** it is one sentence inside `docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`, naming `bc`'s absence as the mechanism — and the unlanded branch `gotcha-pipe-status` already rewrites that whole file, retitling it *A status you did not look at is a status you did not get* and adding the pipe costume, so the sentence belongs **in that landing** rather than in a second branch editing the same 99 lines. Cite the file by path and not by heading, as its six existing citations do, because the heading is what that branch changes. Mechanical option, sized and not taken: a `check-orchestration.sh` case that reds on `bc` under `scripts/` — today that count is **zero**, so it would guard against a relapse and fix nothing | — | `history/2026-10-30-merge-batch-15.md` *Risks and findings* 4 and *Left out*; `history/2026-10-30-merge-batch-14.md`; `docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md` and the unlanded `gotcha-pipe-status`; `scripts/gate.sh:278`, `:291`, `:371`; `scripts/bench-preflight.sh`; ORCH-229 |

**ORCH-235, in *Open — seen, not agreed or sized*, immediately after
ORCH-231:**

| ORCH-235 | **A new test file has nothing to conflict with, so git never asks — and a protocol struct widened on `main` after the branch's base breaks it silently.** Only a compile says so: `Merge made by the 'ort' strategy` is not a statement about whether the tree builds, and a reviewer reading the diff sees nothing wrong, because the hand-written struct literal and the field that arrived are in different files. **It has now reddened three gates.** *Batch 13*: `error[E0063]`, `mesh_endpoint` missing on `ToHub::Hello` and `pack_store_budget_bytes`, `pack_store_bytes` and `packs_held` missing on `ToHub::Health`, in `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs` — a **new** file from `audit-tests`, whose base `63a9aff` was earlier than the sibling branches that widened those two variants. *Batch 14*: `error[E0063]`, `keeper_for` missing on `FromHub::Welcome` at `herd-hub/tests/silent_hub.rs:196`, also a **new** file, and measured both ways by that merger rather than asserted — `grep -c keeper_for` on `herd-proto/src/lib.rs` is **0** at the branch's base `b16f732` and **1** on `main`, the field having arrived in `ebbfdd8`. *`pack-fetch`'s merge*: the same silence in the other direction, the branch widening the variants and an existing file being the one that stopped compiling (`herd-hub/tests/owed_scan_fails.rs`, landed by batch 11 after `pack-variants` was cut) — which is why a fix has to serve both directions. **All three resolutions were mechanical and none settled a specification**, which is exactly why this row is about the shape and not about any one of them; and in all three cases the branch's own gate had been genuinely green on its own base. *Two figures of the orchestrator's own, recorded here because the board has no other place for them.* **(1)** Its brief to batch 14 gave `herd-hub/tests/silent_hub.rs` as **487 lines** when the file was **539** (547 after the eight-line resolution): the **fourth** of its figures corrected this session. **(2)** Its brief for this row named **three** fields missing at batch 13's gate when the error named **four**, across two variants — `mesh_endpoint` on `ToHub::Hello`, and `pack_store_budget_bytes`, `pack_store_bytes` and `packs_held` on `ToHub::Health`: the **fifth**, and this row's own account of batch 13 above names the measured four. That merger's reason for reporting the first is the one to keep for both — *"a figure in a brief becomes an acceptance criterion"* — and **both errors are the orchestrator's, not an agent's** | three landings measured by `herd-merger`, 2026-10-30: batches 13 and 14 and `pack-fetch`'s merge. **Proposed by batch 14's merger, which deliberately did not file it** — filing is a board edit, and a board edit at that point would have discarded a green gate and re-gated on a loaded host — and left the call to the orchestrator; **the orchestrator made it on 2026-10-30 and this row is that call** | herd-planner | open | **Decide whether `ToHub` and `FromHub` get test-side constructors or builders, so that a new field has one place to be added instead of N test files.** That is batch 14's merger's proposal, and the question it leaves open is where they live, since `#[cfg(test)]` does not cross a crate boundary: a cargo feature on `herd-proto`, a `testing` module beside `herd_runner::testing`, or a dev-only crate. Two things the brief should settle rather than the code: the default a constructor hands out must be the empty, absent-on-the-wire value — which is what all three resolutions independently chose and what `herd-proto/tests/round_trip.rs` already asserts does not reach the wire — and the sweep that adopts the constructor is test-only, so it owes all four Rust gates and nothing else. **The family is already named on this board**: *a rule written in N places is fixed in N-1* (ORCH-131, ORCH-132, ORCH-133, all three prose), and this is that family applied to **test data**, with three red gates as its measurement. Cheaper interim measure, sized and not taken: no mechanical check is possible from a branch, because the breakage exists only in the merged tree — the thing that catches it is the merger gating its own integration tip, which it already does | — | `history/2026-10-30-merge-batch-13.md` (the first gate, red, and the nine byte-identical lines); `history/2026-10-30-merge-batch-14.md` *Risks and findings* 0 and the `keeper_for` resolution, and *Risks and findings* 3 for the 487-against-539; `history/2026-10-30-pack-fetch.md` §0.2; `herd-proto/src/lib.rs`; `herd-hub/tests/silent_hub.rs:196`; `herd-hub/tests/owed_scan_fails.rs`; `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs`; ORCH-131, ORCH-132, ORCH-133 |

**ORCH-237, in *Open — seen, not agreed or sized*, immediately after
ORCH-235:**

| ORCH-237 | **Two branches can claim one `ORCH-nnn` and nothing notices until both have landed.** Batch 15's merger drafted its rows as `ORCH-NEXT-A`, `-B` and `-C`, took **231** as the next free number, and then found **batch 14's unlanded integration branch already holding ORCH-231** — the WebSocket keepalive, committed there as *Amend ORCH-228 and open ORCH-231 for batch 14* — whose handoff was already committed, which under **ORCH-121** is the last change before a gate and therefore not rewritable. So the batch that could still move cheaply moved, and its three rows became **232, 233 and 234**. That merger's own words for the mechanism: *"an unlanded integration branch in another worktree is not something a branch listing shows: it was found by reading the host, not by reading the origin"*. **Had batch 14 gone red and never landed, ORCH-231 would be a gap and would stay one** — an id is an identifier and gaps stay (**D148**), and that landing did not reclaim it. **This is the `D`-number collision in its board-row costume, and the `D` half is already solved**: a provisional id plus a single allocator at integration (ORCH-003, `gotchas/decision-number-collisions.md`, **D145**, **D148**). What defeated the same mechanism here is that there were **two allocators at once**: two mergers preparing batches in parallel against one `origin/main`, which is the orchestrator's own doing and the orchestrator's to own | `history/2026-10-30-merge-batch-15.md` *Risks and findings* 2 and *Decisions allocated* 3, 2026-10-30; filed on the orchestrator's instruction, 2026-10-30 | herd-planner | open | **Three candidate shapes, stated and not chosen — and one of them is already built and already known to be too late.** (a) *A duplicate-id check*: **it exists.** `scripts/check-docs.sh` already sorts the board's ids, reports any the `uniq -d` beside it returns, and says *"uses an id twice"*; proven at this task to fire on a doctored copy that repeats ORCH-234 and to stay silent on the real file. It reads **one file**, so it can only speak once both rows are in one tree — which is after both have landed. Exactly as weak as predicted, and not a thing to build. (b) *A reserved block per agent*: costs nothing and leaves gaps, which are lawful anyway (D148). (c) **The orchestrator allocating the number in the brief**, where the serialisation already exists: briefs are written one at a time, so the orchestrator is the only actor that sees both branches before either one exists. **The orchestrator leans to (c) and says plainly that this is a lean and not a ruling**; the pick is `herd-planner`'s. **The harder half, named by that merger and solved by none of the three:** two mergers should not be preparing batches concurrently at all — that was the orchestrator's doing, it is the orchestrator's to own, and it is the condition under which a single allocator stops being single | `ORCH-003` | `history/2026-10-30-merge-batch-15.md` *Risks and findings* 2, *Decisions allocated* 3, and its *Board rows* section; `history/2026-10-30-merge-batch-14.md`; `scripts/check-docs.sh`, the board's duplicate-id check; `docs/orchestration/gotchas/decision-number-collisions.md`; ORCH-003, ORCH-121; **D145**, **D148** |

**ORCH-238, in *Open — seen, not agreed or sized*, immediately after
ORCH-237:**

| ORCH-238 | **A green pushed branch was missed by two landings in a row, because the batch list was built from recency instead of from what `main` cannot reach.** `origin/gotcha-pipe-status` at `d10d402` is **2 commits, 340 insertions across 3 files**, and **not an ancestor of `origin/main`** — measured by the orchestrator at **06:55** and re-verified at **06:57** against a fresh fetch, and reproduced figure for figure at this task. Batch 15's *Left out* says *"no such branch existed to take"* and names its method in the same sentence — *"the only branches on the origin newer than `main` were `pack-wire` and `runner-silence`"* — which is **recency, not reachability**. The clock says the branch was there to be seen: its tip was committed at **06:09:26**, batch 14 landed at **06:30:40**, batch 15's own handoff (its last change before its gate, ORCH-121) at **06:34:08**, and batch 15 landed at **06:43:30**. *Whether those two commits had reached the origin by then cannot be measured from this host*, an origin ref carrying no reflog here, so the commit times are a floor and this row claims no more than a floor. **The noise is the real mechanism:** **30** origin branches are not ancestors of `main`, and most are stale leftovers whose content reached `main` another way, which is exactly how one live branch gets lost among them — the count is now 31 because the branch carrying this row joined the list, which is itself the measure moving under the measurer. `gotcha-pipe-status` is the **only unlanded branch of this session's own work**: the only unreachable origin branch with a 2026-10-30 tip apart from this one | measured by the orchestrator, 2026-10-30 at 06:55 and 06:57, re-measured at this task; `history/2026-10-30-merge-batch-15.md` *Left out*; filed on the orchestrator's instruction, 2026-10-30 | herd-planner | open | **Derive the batch list from unreachable content, and make a merger say what it left out.** The listing a merger builds its batch from should ask, of every origin branch, whether `main` can reach its content — not whether its tip looks recent. The shape is one `git merge-base --is-ancestor` per branch, which is what produced the 30 above and costs about a second. It needs a companion, because most of those 30 **should** stay unlanded and a list of 30 that a merger must justify is a list a merger will skim: a merger states in its handoff every unreachable branch it did **not** take, and why. **Batch 15's merger already did exactly that** — it named this branch under *Left out*, with a wrong reason rather than with silence, which is the only reason the miss is legible at all and is the behaviour worth making the rule; batch 14's handoff names no left-out branch of any kind. **First claim on the result:** `gotcha-pipe-status` is owed a landing, and it carries ORCH-236's `bc` sentence with it | — | `history/2026-10-30-merge-batch-15.md` *Left out*; `history/2026-10-30-merge-batch-14.md` *Left out*; `origin/gotcha-pipe-status` at `d10d402`; `docs/orchestration/workflow.md` *What a landing is*; ORCH-121, ORCH-236 |

## Decisions needed

**None from Alexa.** Neither row asks him anything: both are *how* —
tooling and test data — and no *Waiting* row was added. Nothing in
either touches the protocol's promises, the consumer's side, a release
or an irreversible act.

**The one question this file carried has been answered and is closed.**
It asked whether ORCH-235 should be `open` or `owed`. The orchestrator
answered on 2026-10-30: **`open`, in *Open*, owner unchanged, nothing to
change** — the call it had made was the call to *file* the row, and the
`#[cfg(test)]` crate-boundary constraint is precisely why the shape is
`herd-planner`'s to settle. The row is as filed and was not edited for
this.

**Nothing new for Alexa.** All four rows are *how* — test data, shell
arithmetic, id allocation, branch listing — so no *Waiting* row was
added by either instruction. ORCH-237 and ORCH-238 each name something
that is the **orchestrator's** to own rather than Alexa's: two mergers
preparing batches concurrently, and a batch list built from recency.
Both are recorded in the rows as the orchestrator's own, at its
instruction.

## Resume point

Nothing on this branch. It is four board rows and this file; it is
pushed, and the next act on it is `herd-merger`'s at the next landing.
**It is itself now an instance of ORCH-238** — a green pushed branch
that `main` cannot reach — and ORCH-237 is why its four ids were
measured against the file twice rather than once.

The four rows name their own next steps, and none is started:

```sh
# ORCH-235, for herd-planner: decide where test-side constructors for
# ToHub and FromHub live, given that #[cfg(test)] does not cross a
# crate boundary. The three resolutions to read first:
sed -n '240,320p' docs/orchestration/history/2026-10-30-merge-batch-14.md
sed -n '310,360p' docs/orchestration/history/2026-10-30-merge-batch-13.md
sed -n '40,50p'   docs/orchestration/history/2026-10-30-pack-fetch.md

# ORCH-236, for whoever lands gotcha-pipe-status: add the one bc
# sentence to the gotcha that branch already retitles.
git log --oneline main..gotcha-pipe-status

# ORCH-237, for herd-planner: pick (a), (b) or (c). (a) is already
# built, so read it before choosing against it.
sed -n '167,183p' scripts/check-docs.sh
sed -n '328,343p' docs/orchestration/history/2026-10-30-merge-batch-15.md

# ORCH-238, for herd-planner, then every merger: the listing that
# should replace "newest branches on the origin".
for b in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin); do
  git merge-base --is-ancestor "$b" origin/main || echo "$b"
done
```

## Processes

**No PID started and none ended**, across both instructions. No
background job, no binary, no compile, no suite, no bench: two shell
scripts in the foreground (`check-docs.sh` four times,
`check-orchestration.sh` twice) plus read-only `git`, `grep`, `sed`,
`awk`, `wc` and one `git fetch`. Nothing was killed, and no `pkill` or
kill-by-name was used at any point. **Nothing of another agent's was
read into an edit**: `runner-announces` holds ORCH-232 and that row's
state cell is byte-identical to `main`'s.

**No `target/` to delete, and `CARGO_TARGET_DIR` was never set:** this
task compiled nothing, so it created no build tree on any filesystem.
The worktree itself —
`<scratchpad>/board-two-rows/wt`, with `<scratchpad>` as spelled in
*Gates* — holds only the checkout and is the orchestrator's to remove
with `git worktree remove` once the batch has landed. Scratchpad files
left for the record beside it: `row235.txt`, `row236.txt`,
`row237.txt`, `row238.txt`, `board.new`, `board2.new`, `cur235.txt`,
`extra.txt`, `dup-fixture.md` (the doctored board that proved the
duplicate-id detector fires), `check-docs.log`, `check-orch.log`,
`check-docs-final.log`, `check-orch-final.log`, `handoff-head.md`,
`handoff-tail.md`, `handoff2.new`.
