# merge-batch-5 — handoff 2026-10-29

- **Item:** batch 5 — record that v0.4.0 is released. One branch, one
  `gate.sh`, one `--no-ff` merge commit whose message **is** the landing
  record, one push under ORCH-001, one post-push `ci.yml` run.
- **Agent:** herd-merger
- **Branch / commit:** `batch-5-integration`, off
  `origin/main` `171c78065a6918ef4a1f1fb6f933315153fea53c`; this handoff
  commit is its tip and the **gated sha**.
- **Worktree:**
  `scratchpad/wt-batch-5/` under this session's scratchpad directory, the
  full path in *Processes* below.
- **Landing:** this batch. One push, one merge commit, one post-push run.
- **No tag, no release, nothing touching the published v0.4.0, no pull
  request.** The release was published before this landing started and
  its notes were completed by Alexa's own hand; this landing's whole job
  is to write that down.

## Where the things that cannot be in this file are

This handoff is the **last change to the tree**, because the gate must
run on the tree that is pushed (ORCH-121 applied to the merger;
`workflow.md`, *What a landing is*). So four things happen after it was
committed and are in the merger's **report to the orchestrator**, not
here:

1. the gate's **verbatim output** and its **skip list** — never a skip
   count (D27, D179);
2. the **merge commit sha** and the resolved **gated sha**, which is that
   commit's second parent;
3. the **post-push `ci.yml` run id**;
4. what step 10 did to the orchestrator's checkout at
   `/mnt/ssd_980/herd-project` — the shas it moved between, or which
   precondition stopped it.

## The batch, in merge order

One branch. The batch is everything ready when the merger started, and
that was one.

| # | branch | commit | files | decisions | conflicts |
| --- | --- | --- | --- | --- | --- |
| 1 | `tag-v040-handoff` | `28415a4` | `docs/orchestration/history/2026-10-29-tag-v0.4.0.md` (+386), new file, prose only | none — it proposes no `D-NEXT` and does not touch `docs/decisions.md` | none, and none possible |

Compared with `origin/main` before merging: the branch adds **one file
that does not exist on `main`** and changes nothing else. It touches
**none** of `docs/decisions.md`, `docs/orchestration/board.md`,
`docs/status.md` or `docs/orchestration/workflow.md` —

```
$ git diff --stat origin/main origin/tag-v040-handoff -- \
    docs/orchestration/board.md docs/status.md \
    docs/decisions.md docs/orchestration/workflow.md
(empty)
```

— which is why no conflict was possible and why the board check below
mattered more here than usual, not less.

Its handoff is present, as every branch of a batch owes:
`docs/orchestration/history/2026-10-29-tag-v0.4.0.md`.

The merge of that one branch onto `batch-5-integration` **fast-forwarded**
(`171c780..28415a4`), which is expected for a batch of one off the same
base. The `--no-ff` merge commit that is the landing is made onto `main`
in step 8 and is what gives the record a second parent.

## Conflict log

**No conflict, and none was possible.** The branch adds one new file and
modifies none, so there is no overlapping hunk anywhere in the batch. No
file was resolved, no side was chosen, no specification or invariant came
into question, and nothing was escalated.

That is the dangerous case rather than the easy one — `workflow.md`, *One
landing per batch*: "a silent revert with nothing to resolve is caught by
this check and by nothing else" — so the board check was run in both
directions and its exact output is under *Gates*.

## D numbers allocated

**One: D210.**

- **D210 — Completing a published release's notes is Alexa's own act.**

Allocation, measured rather than assumed:

- Highest on `origin/main`: **D209** (`docs/decisions.md`, 206 entries).
- No `D-NEXT` header anywhere: `grep -rn 'D-NEXT' docs/` matches only
  prose *about* the placeholder (`decisions-structure.md`, the
  remediation reports, older handoffs) and no decision header. The one
  branch of this batch does not touch `docs/decisions.md` at all, so no
  unlanded branch holds a number.
- D210 is therefore the next free number. Appended at the end; **nothing
  renumbered, no existing entry edited.** The gaps stay — `D143`, `D154`,
  `D170` — and `check-docs.sh` prints `D170` as a gap, as it did before
  (D148, invariant 8).

**Why it is a `D` and not a board row, argued, because the orchestrator
read it the other way and left the call to the merger.** Two things were
tangled together and they separate cleanly:

- **The proposal** — a per-release notes file that the gate checks for —
  is a design nobody has built and whose shape is arguable. It stays a
  **board row**, `ORCH-162`, `open`, to be sized and then put to Alexa.
- **The rule** — that completing a published release's notes is Alexa's
  own act, and that no brief may assign it to an agent — is not a
  proposal. **Alexa has already performed it**, in the only way the
  permission system permits, and it constrains what every future release
  brief may contain. That is the shape of a rule, like D27's about skips.
  Writing it down pre-empts nothing; it records what was done.

D210 names its own **retirement condition**, so the entry does not
outlive its reason: if ORCH-162 lands, the notes ship in the tree, the
page is complete the moment it is published, and nothing is left for the
rule to govern. It also says plainly that **neither permission wall is to
be weakened** to discharge it.

One sentence was added to `workflow.md` **step 8** pointing at D210, so
that a release brief carries the paste in the same order as the tag — a
rule nobody reads where briefs are written is not a rule. Judged a
**micro-edit** and not a rule change needing a `herd-reviewer` pass:
`workflow.md` is not in step 6's review list (`CLAUDE.md`, agents,
permissions, hooks, `check-docs.sh`, an invariant), and the sentence
records a measured fact rather than changing a procedure. Flagged here so
the orchestrator can send it to a reviewer if it disagrees.

## Gates

### The docs gates, run before the handoff commit

`./scripts/check-docs.sh` — **`docs ok`, exit 0**, run after the
`workflow.md` sentence, after D210, after each board edit and after the
`status.md` splice. Its one note is the standing gap:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — **269 passed, 0 failed**, exit 0.
**Unmoved** from the figure measured at `b73d38c`, `6036d24`, `1dd2383`
and `5c2de3b`; measured here on the integration tip rather than taken on
report, because a single docs length or width overflow turns thirteen of
its cases red at once.

Line budgets, measured and not assumed: `docs/status.md` **90/90** and
`CLAUDE.md` **150/150** — no slack either side, which is why the
`status.md` edit **replaced** rather than appended. The *Releases*
section is 15 lines where it was 14, and the post-push streak sentence is
3 where it was 4; that is where the line came from.

### The board read line by line, both directions

Required by `workflow.md`, and this batch is exactly the case it was
written for.

**Direction 1 — every branch's own `board.md` line verbatim in the merged
file.** Satisfied vacuously, and the check is what proves that rather
than assuming it: the branch has **no** `board.md` hunk at all, so it
contributed no line that could go missing. Running the literal
line-by-line comparison against the branch's copy reports exactly three
lines as not-verbatim — ORCH-156, ORCH-119 and ORCH-161 — and all three
are rows **this landing** extended. No fourth row moved.

**Direction 2 — the merged file differs from the branch's only by this
landing's own hunks.** `board.md`:

```
$ diff <(git show origin/tag-v040-handoff:docs/orchestration/board.md) \
       docs/orchestration/board.md | grep -E '^[0-9]'
96c96
128c128
158c158,159
```

ORCH-156 extended, ORCH-119's evidence extended, ORCH-161 corrected, and
ORCH-162 added as line 159. Four edits, nothing else, nothing reverted.
`docs/status.md`:

```
$ diff <(git show origin/tag-v040-handoff:docs/status.md) docs/status.md \
       | grep -E '^[0-9]'
37,50c37,51
74,77c75,77
```

The two blocks named above and no third.

### The mojibake scan, and the one it caught

The byte detector of the brief, **proven on a known-bad and a known-good
fixture before its silence was trusted** (`grep` here is ugrep 7.8.4 and
its `-P` matches no `\xNN` byte, so `grep -rnE 'Ã|Â'` is blind to a
mangled `—`):

```
perl -ne 'print "HIT $ARGV:$.\n"
  if /\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]/ || /\xc3\x82\xc2[\x80-\xbf]/'
```

It caught a real corruption of this landing's own making. A first attempt
at the `workflow.md` sentence used `perl -0777 -i -pe` with `\x{2014}` in
the replacement; perl upgraded the whole byte string to UTF-8 on output
and **double-encoded the entire file** — 0 occurrences of the mojibake
shape on `origin/main`, **79** in the worktree afterwards, and a diff of
86 insertions against 80 deletions on a one-sentence edit. It printed one
warning, `Wide character in print`, and `sed` displayed the result
correctly. Reverted with `git checkout --` and redone with the raw UTF-8
bytes `\xe2\x80\x94` in a byte-only string: 7 insertions, 1 deletion, 0
occurrences. Every file this landing touched was scanned clean afterwards.

### The Rust gates

**Not run separately.** This landing is prose only: `docs/` and nothing
else, no Rust file, no workflow file, no script. What covers the four
Rust gates is the one `./scripts/gate.sh` run below, which runs the steps
of CI's `check` and `docs` jobs in the same order.

### The gate, which runs after this commit

**One `./scripts/gate.sh` run, BARE, on a clean tree, the last thing
before the merge.** Not inside `nix develop`: nesting `TMPDIR` panics
`herd-runner/tests/confinement.rs:2529` on `SUN_LEN` (ORCH-139).

The sha it gates is **this handoff commit** — the integration tip, which
is the second parent of the merge commit, not the merge commit itself,
whose message is the record and which therefore cannot exist yet. The
receipt it will write:

```
<worktree>/target/herd-gate/<gated sha>.receipt
```

with `<worktree>` the path in *Processes* below and `<gated sha>` the full
40-hex sha of this commit — a value that cannot be written here, because
naming it would change the tree whose sha it is. The receipt store is
**per worktree** (`$HERD_GATE_RECEIPTS`, default `target/herd-gate/`) and
nothing was ever copied into it.

**The verbatim output and the skip list go to that receipt and to the
merger's report, not into this tree.** A red step blocks the landing, is
reported red, and `main` does not move.

## Not done, and why

- **No tag, no release, no `gh release edit`, nothing on the published
  v0.4.0.** Alexa ran the one body edit himself before this landing
  started; this landing re-measured the page read-only and recorded it.
- **No pull request.** Nobody opens one, the merger included; the one
  exception is dated and spent.
- **ORCH-156 not closed.** Its release half is fully discharged and
  written down. Its other half is ORCH-145's reading, which is the
  consumer's to take and is still in *Waiting*. Whether the row closes on
  the release half alone is the orchestrator's call, left rather than
  taken.
- **ORCH-155 stays `owed`, ORCH-152 stays `open`**, both verified
  unchanged after the edits. The roadmap to v1.0.0 still lives only in a
  scratchpad file; `docs/plan.md` is untouched.
- **ORCH-162 opened, not implemented.** No `deploy/release-notes/`
  directory was created and `release.yml` and `check-docs.sh` are
  unchanged.
- **Neither permission wall widened.** Nothing in `.claude/` was touched.
- **The other 29 branches on the origin** were not ready and are not in
  this batch.

## Risks and findings

1. **A literal string search said ABSENT on a marker that is present.**
   Seven of the eight release-body markers match as plain strings; the
   eighth is on the page as *What is **not** in this release*, so a
   search for `What is not in this release` reports it absent. The page
   is complete. This is the same class of error as the ugrep one above
   and it cost nothing only because the figure was cross-checked another
   way (805 lines, and the body byte-identical to the prepared file).
2. **`tail -523` is the wrong window on the live body and gives a
   different sha256.** The page is the prepared file plus one trailing
   newline, so the generated half is lines **282-804**, which hashes to
   `87cb889775e8d347064ae4e476670c72fb1f2ad6d5ea68ae3cc4d92c072a82d0` as
   expected; `tail -523` starts one line late, picks up the trailing
   blank, and hashes to something else. Anybody re-checking this should
   use the line range, not the tail.
3. **The board carries a dated sentence that a later dated sentence
   supersedes.** ORCH-156's cell still says, of batch 4, that "no
   `v0.4.0` tag exists locally or on the origin, measured at the
   landing". That was true then. The paragraph immediately after it is
   dated at this landing and says the release half is fully discharged.
   This is the board's existing convention — append with a date, never
   rewrite — and ORCH-161's row does the same. Noted because a reader who
   stops at the first sentence gets a false answer.
4. **`workflow.md` is loose about the twelve skips' wording.** *What the
   local merge costs* says all twelve read `cgroup v2 delegation is
   unavailable`; measured in `scripts/expected-skips/local.txt`, **ten**
   do, and the other two refuse for the same missing cgroup v2 subtree in
   different words. Right about the cause, loose about the wording. Not
   fixed here: that sentence is not this batch's, and the record says so
   as a reading rather than a correction.
5. **This landing's record is prose about a release that is already
   public.** If any figure in it is wrong, the cost is a wrong record,
   not a wrong artefact. Every figure in it was re-read off the origin or
   out of the tree at this landing rather than copied from the brief, and
   the two places where the brief's own numbers needed refining are
   findings 1 and 2.

## The landing record

Written now, at step 5, because it **is** the message of the `--no-ff`
merge commit of step 8. This copy and that message are the same text.
The gated sha appears in it as "`fb75d47` plus the merge handoff commit,
which is this commit's second parent": exact and self-resolving —
`git rev-parse <merge sha>^2` — and not writable as 40 hex here for the
reason given under *Gates*.

Land batch 5: record that v0.4.0 is released

- **Batch:** 5, one branch, landed 2026-10-29 by herd-merger. One push,
  one --no-ff merge commit carrying this record, one post-push ci.yml run.
- **Gate:** green -- ./scripts/gate.sh on `fb75d47` plus the merge handoff
  commit, which is this commit's second parent and the last change to the
  tree; trees identical, `git diff --quiet` between that sha and this one
  clean. Run **BARE**, not inside nix develop: nesting TMPDIR panics
  confinement.rs:2529 on SUN_LEN (ORCH-139). The receipt is named for that
  sha under target/herd-gate/ in the merger's own worktree -- per worktree,
  never copied in. The resolved 40-hex sha, the verbatim output and the
  receipt path are in the merger's report: each comes into being only
  after the gated tree was closed, so none of them can be in it.
- **Skips:** 12, the at-rest names of scripts/expected-skips/local.txt,
  each printed as a skip and none counted into a pass (D27, invariant 5).
  Named one by one under *Evidence*. The list decides, never the count
  (D187); the two load-gated flood benches of local-under-load.txt are a
  delta over that list and are excused only on their own figure (D196).
- **CI run:** none yet -- this push starts it. A run cannot exist before
  the push that triggers it, so its id goes in the merger's report and the
  next landing's board edit records it (workflow.md, *What a landing is*).
  This landing is where that duty was discharged for batch 4: run
  40882112506 is now in ORCH-119's evidence.

## What changed and why

This landing moves no milestone and ships no code. It writes down that
**v0.4.0 is released**, which is the half of ORCH-156 a release owed, and
it records the one thing that went wrong in releasing it.

What `main` says now that it did not before: that the ordinary release
before v1.0.0 has shipped -- tag `2631286` dereferencing to `171c780`,
published 2026-10-29T13:50:10Z, `prerelease: false`, now *Latest* -- and
that the three version numbers D199 will move are still 2, 1 and 1, which
is the constraint ORCH-156 exists to hold. The consumer can take their
`report` reading (ORCH-145) on a root this build still opens.

And that **the release body went out without the half written for it.**
`release.yml` assembles a body from pieces it generates itself and never
reads the prepared human half, so the first published page carried neither
operator caution. Two permission walls then refused the repair -- the
auto-mode classifier refused herd-step, orchestrator-guard.sh refused the
orchestrator -- and Alexa ran the one `gh release edit` himself. Neither
wall is a defect and neither is weakened here. D210 records the rule that
follows; ORCH-162 is the proposal that would retire it.

## Branches in this batch

A batch of one. The other 29 branches on the origin were not ready and are
not in it (see *Left out*).

1. `tag-v040-handoff` at `28415a4` -- ORCH-156 (its release half; the tag
   push itself was ordered by Alexa directly, outside any board row), and
   it also drafted what became ORCH-162. Handoff:
   `docs/orchestration/history/2026-10-29-tag-v0.4.0.md`.

## Evidence

### Branch `tag-v040-handoff` -- its own evidence, not the batch's

Two commits, one file, +386 lines, prose only: the handoff for the v0.4.0
tag push. Nothing outside `docs/`, so the four Rust gates were **not run**
on the branch and the branch says so; what covers them is this landing's
`gate.sh`. Its own last gate was `./scripts/check-docs.sh` on the
committed tree, and it reports an **earlier red run** of that script
honestly: three lines over 79 columns, a release URL and two scratchpad
paths, fixed by fencing them and not by editing any check. No
`HERD-SKIPPED` line was produced on the branch, because no suite ran
there. Its author force-pushed one earlier revision of the file with
`--force-with-lease`, on its own unlanded branch, and disclosed it
unprompted; the second commit is an ordinary fast-forward and nothing is
based on the branch.

Re-verified by the merger off the origin at this landing, not taken on
report: tag `26312865f9d3369c207fe8c2dc87cba05aed61ca` ->
`171c78065a6918ef4a1f1fb6f933315153fea53c`; release `Herd v0.4.0`,
`isDraft: false`, `isPrerelease: false`, `publishedAt`
`2026-10-29T13:50:10Z`, *Latest*; five assets at 5 501 744, 7 493 984,
6 512 232, 19 112 402 and 417 bytes; `release.yml` run `40885270441`
`completed`/`success`; `ci.yml` run `40882112506` `success` on `171c780`.
`sha256sum -c SHA256SUMS` on the downloaded hub binary: OK, exit 0.

### The repaired release body -- measured at this landing

The live page is **805 lines** and is the prepared file byte-identical
plus one trailing newline: 40 353 bytes against 40 352. The generated half
is **unchanged**, lines 282-804 of the page, `diff` empty against the
pre-edit pull and sha256
`87cb889775e8d347064ae4e476670c72fb1f2ad6d5ea68ae3cc4d92c072a82d0`. Both
operator cautions are on it.

**A detector note, because the obvious check lies and this is the second
time a literal search has been wrong on this host.** Seven of the eight
markers match as plain strings; the eighth is on the page as
*What is **not** in this release* and a literal search for
`What is not in this release` reports ABSENT. The marker is present. A
detector gets proven on a known-bad and a known-good fixture before its
silence is trusted -- which is also how the merger caught itself
double-encoding `workflow.md`: a byte-regex scan showed 0 occurrences of
the mojibake shape on `origin/main` and 79 in the worktree after one
careless perl invocation, and the edit was reverted and redone.

### The board read line by line, both directions (workflow.md)

This is the check a batch with no conflict needs most, and here it needed
it most of all: the branch **touched no board.md, status.md, decisions.md
or workflow.md at all** -- `git diff origin/main origin/tag-v040-handoff`
over those four paths is empty -- so there was no branch hunk to lose and
no conflict to resolve. The verbatim requirement is therefore satisfied
vacuously for the branch, and the check's whole value here is the other
direction: the merged `board.md` differs from the branch's copy at exactly
`96c96`, `128c128`, `158c158,159` -- ORCH-156, ORCH-119, ORCH-161 extended
and ORCH-162 added, the four edits this landing made and nothing else; and
`status.md` at exactly `37,50c37,51` and `74,77c75,77`, the two blocks it
replaced. Nothing was silently reverted.

### Docs gates, before the handoff commit

`./scripts/check-docs.sh`: `docs ok`, exit 0, run after each of the four
record edits; its one note is the standing `D170 is a gap` (D148).
`./scripts/check-orchestration.sh`: **269 passed, 0 failed**, exit 0 --
unmoved from the figure measured at `b73d38c`, `6036d24`, `1dd2383` and
`5c2de3b`. `docs/status.md` is **90/90** and `CLAUDE.md` **150/150**.

### The batch's own run on the merged tree

`./scripts/gate.sh`, run bare, on this commit's second parent. Steps 0 to
8, one per `ci.yml` command, ending in one verdict naming its sha. The
twelve skips it expects at rest, each named and printed as a skip, never
counted into a pass (D27, invariant 5). Each is printed by the suite as a
HERD-SKIPPED line; these are the assertion names, which is what the gate
compares:

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

Measured in that file rather than asserted: **ten** of the twelve carry
the sentence `cgroup v2 delegation is unavailable` verbatim, and the other
two refuse for the same missing cgroup v2 subtree in different words --
the io.max ceiling this host will not announce, and the inert weight line.
So *workflow.md* `What the local merge costs`, which says all twelve read
that sentence, is right about the cause and loose about the wording; said
here as a reading of the file, not fixed in it, because that file is not
this batch's.

CI skips two more that this host does not, both
`the_io_weight_verdict_follows_the_device_the_job_works_on`, because
`/mnt/ssd_980` has blk-iocost enabled -- so neither host runs the twelve,
and this one runs two assertions CI cannot (workflow.md, *What the local
merge costs*). The verbatim output and the gate's own skip diff are in the
receipt and in the merger's report; they cannot be in this tree, because
the gate ran on it after it was closed.

## Left out

- **The `gh release edit` that completed the body is not this landing's
  act and was not re-done.** Alexa ran it; nothing here touches the
  published v0.4.0, and no tag was pushed, moved or deleted.
- **ORCH-162 is not implemented**, only opened. The per-release notes file
  and the `check-docs.sh` assertion are a later landing's, after a planner
  sizes it and Alexa chooses it.
- **ORCH-155 stays `owed` and ORCH-152 stays `open`.** The roadmap to
  v1.0.0 still lives only in a scratchpad file and `docs/plan.md` still
  disagrees with the code in about a dozen places. Neither is this
  landing's to close, and `docs/plan.md` is untouched.
- **ORCH-156 is not closed.** Its release half is fully discharged and
  recorded; its other half is ORCH-145's reading, which is the consumer's
  to take and is still in *Waiting*. Whether the row closes on the release
  half alone is left to the orchestrator rather than taken here.
- **Neither permission wall is widened**, and D210 says so explicitly. An
  agent that can write to a public page is a larger change than the
  problem.
- **The 29 other branches on the origin** were not ready when this merger
  started and are not in the batch. The batch is everything ready at
  start, and that was one branch.

## Decisions allocated

**D210 -- Completing a published release's notes is Alexa's own act.**
Allocated as the next free number above the highest on `origin/main`
(D209, measured) and above every branch not yet landed: `git grep D-NEXT`
finds no `D-NEXT` header anywhere in `docs/decisions.md` on any branch,
and the branch of this batch does not touch that file at all. The gaps
stay, D170 included, and `check-docs.sh` prints it as a gap (D148,
invariant 8). Nothing was renumbered and no existing entry was edited.

Argued, because the orchestrator read it the other way and the call was
the merger's: D210 is a **rule** and not a proposal. Alexa has already
performed it, in the only way the permission system permits, and it
constrains what every future release brief may contain. The proposal is a
separate thing and stayed a board row: ORCH-162. D210 names its own
retirement condition -- if ORCH-162 lands, the notes ship in the tree and
nothing is left for the rule to govern.

One sentence was added to `workflow.md` step 8 pointing at D210, so a
release brief carries the paste in the same order as the tag. Judged a
micro-edit and not a rule change needing a reviewer: `workflow.md` is not
in step 6's review list, and the sentence records a measured fact rather
than changing a procedure. Said here so the orchestrator can send it to a
reviewer if it disagrees.

## Board rows

- **ORCH-162** -- **new**, `open`, in *Open -- seen, not agreed or sized*.
  A release's own notes are a file the gate checks for. Owner
  `herd-planner`, then `herd-step`. Put in *Open* and not *Waiting*
  deliberately: *Waiting* already holds five unanswered questions and this
  one is unsized, and one question at a time with a recommendation is the
  rule.
- **ORCH-156** -- stays `owed`. Release half fully discharged, with the
  tag, the publication, the 22 green steps, the three unmoved numbers
  re-read on the landed tree and the interoperability run in both
  directions.
- **ORCH-161** -- stays `open`. Corrected: its cell claimed the caution
  was written into the v0.4.0 release body, which was true of the body
  file and false of the public page for about fifty-five minutes. It is
  true of the page now, and the row closes the day ORCH-106 lands.
- **ORCH-119** -- stays `done`. Its evidence now carries post-push run
  `40882112506` (`171c780`, `success`), the second consecutive green and
  the run batch 4 recorded for this landing to carry.
- **ORCH-155** -- unchanged, `owed`. **ORCH-152** -- unchanged, `open`.
  Both verified unchanged after the edits.

## CI run

**Recorded, not waited on** -- the wait is the nine minutes the local gate
exists to stop paying, and this landing is prose only: no Rust file, no
workflow file, no script changed, so there is nothing in it that could
make `main` red that the local gate did not already run. The id is in the
merger's report, from
`gh run list --workflow ci.yml --branch main --limit 1`, and the next
landing's board edit is what records it, because this handoff is already
in `main` and a second push for one batch is forbidden. Its verdict is
read at the next session start and a `failure` becomes an `ORCH-nnn` row
before new work is briefed.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc

## Resume point

**v0.4.0 is released and `main` now says so.** The release half of
ORCH-156 is discharged; the version triple (D199) stays unbriefed until
ORCH-145's reading is taken, which is the consumer's act on a root this
build still opens.

The next session, after this file and the board:

1. **Read the post-push `ci.yml` run.** Its id is in the merger's report,
   not here, and the **next** landing's board edit is what records it —
   a second push for one batch is forbidden. A `failure` becomes an
   `ORCH-nnn` row before any new work is briefed, and
   `gh run rerun --failed` is the first command (`workflow.md`, *When the
   post-push run on `main` is red*).
2. **ORCH-162** needs sizing by `herd-planner` and then one question to
   Alexa. It is the row that retires D210.
3. **ORCH-155** is the largest thing owed: the roadmap to v1.0.0 exists
   only in a scratchpad file and rewrites `docs/plan.md`, so it needs a
   reviewer pass and cannot be one step.

Nothing about this release is outstanding. The tag is pushed, the
workflow is green in all 22 steps, the artefacts are verified, the
published hub binary provably carries `report`, interoperability was run
in both directions, and the body is complete on the page.

## Processes

Worktree, created by this agent off `origin/main` and the only place it
wrote inside the repository:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-5/
```

Scratchpad files this landing wrote, outside the repository, named in
full for whoever audits the figures above:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/landing-batch-5.txt
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/body-at-batch5.md
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/live-generated-half.md
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/fx-good.txt
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/fx-bad.txt
```

The last two are the known-good and known-bad fixtures the mojibake
detector was proven on before its silence was trusted.

PIDs started and ended: none that outlived its command. Nothing was
killed — not by name, not `pkill -f`, not `killall`. Only read-only `gh`
and `git` calls touched anything outside the worktree, and
`/etc/nixos`, `/srv`, the containers, the services and everything of the
chatOnS project were untouched.

**`CARGO_TARGET_DIR`** is set outside the worktree for the gate run and
is deleted once the push is done; the worktree's small
`target/herd-gate/` receipts are kept, as the brief requires. The gate
run below is the only heavy phase and the only compile — one at a time,
and `./scripts/bench-preflight.sh` is run before it.

**Step 10**, the fast-forward of the orchestrator's checkout at
`/mnt/ssd_980/herd-project`, happens after the push and only if `main`
actually moved. Its two preconditions are checked explicitly, because
`--ff-only` supplies neither: `HEAD` must be `main`, and the tree must be
clean. A refusal prints why, a success prints the before and after shas,
and either way it is in the merger's report and **never** fails the
landing. If a precondition stops it, the checkout is left alone — no
`reset --hard`, no `checkout -f`, no `stash`, no commit there.
