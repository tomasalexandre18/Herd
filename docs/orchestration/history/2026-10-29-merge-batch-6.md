# merge-batch-6 — handoff 2026-10-29

- **Item:** batch 6 — land the reaper/sweep regression fix. One branch, one
  bare `gate.sh`, one `--no-ff` merge commit whose message **is** the landing
  record, one push under ORCH-001, one post-push `ci.yml` run.
- **Agent:** herd-merger
- **Branch / commit:** `batch-6-integration`, off `origin/main`
  `10ff31de597d4481b7d3cd150ee28470c48b318b`; **this handoff commit is its
  tip and the gated sha.**
- **Worktree:** `scratchpad/wt-batch-6/` under this session's scratchpad
  directory, the full path in *Processes* below.
- **Landing:** this batch. One push, one merge commit, one post-push run.
- **No tag, no release, no pull request.** The release that carries this fix
  is what unblocks the first consumer and it is a separate act on Alexa's
  word, one act at a time. Nothing here touches the published v0.4.0.

## Where the things that cannot be in this file are

This handoff is the **last change to the tree**, because the gate must run
on the tree that is pushed (ORCH-121 applied to the merger; `workflow.md`,
*What a landing is*). So four things happen after it was committed and are
in the merger's **report to the orchestrator**, not here:

1. the gate's **verbatim output** and its **skip list** — never a skip
   count (D27, D179);
2. the **merge commit sha** and the resolved **gated sha**, which is that
   commit's second parent;
3. the **post-push `ci.yml` run id**;
4. what step 10 did to the orchestrator's checkout at
   `/mnt/ssd_980/herd-project` — the shas it moved between, or which
   precondition stopped it.

## The batch, in merge order

One branch. The batch is everything ready when the merger started, and that
was one, deliberately: this fixes a regression in a release published today
and nothing rode along.

| # | branch | commit | files | decisions | conflicts |
| --- | --- | --- | --- | --- | --- |
| 1 | `reaper-sweep` | `72c333977b5d967d7940fc5f1e94d0ff5cdd4735` | 5, +895/−51 | none of its own; D211 is the merger's | none |

Its four commits, in order, on `10ff31d`: `7b3bce7` the diagnosis handoff
plus the then-`#[ignore]`d reproduction, `339d1ba` the fix itself, then
`631a83d` and `72c3339` the fix's handoff. The five files are
`herd-runner/src/reclaim.rs`, `herd-runner/src/confine.rs`,
`herd-runner/src/sweep.rs` and two new files under
`docs/orchestration/history/`. **No version number moves.**

`collect-gap` is **not a second branch of this batch.** Its tip `7b3bce7`
is an ancestor of `reaper-sweep` — `git merge-base --is-ancestor` says so —
so its handoff rides inside this one branch, and it is named here and in
the record so that a reader looking for ORCH-163, ORCH-165 and ORCH-166
finds where their evidence landed.

## Conflict log

**Nothing to log, and the measurement is what says so rather than the
absence of a complaint.** `git merge --no-ff` reported `Merge made by the
'ort' strategy`, no conflict marker and no manual resolution. The merged
diffstat is byte-for-byte the diffstat of `git diff --stat origin/main
origin/reaper-sweep`: the same five files, the same +895/−51.

**No conflict in this batch changed a specification or an invariant**,
because there was no conflict at all. Nothing was escalated.

The branch touched **none** of `docs/decisions.md`,
`docs/orchestration/board.md` or `docs/status.md`, so every docs edit in
this landing is the merger's own work, allocated once for the whole batch
in merge order.

## D numbers allocated

**D211 — The startup reaper leaves outputs/ to the sweep.** One decision,
appended at the end of `docs/decisions.md`. The diff is **95 insertions, 0
deletions**: nothing renumbered, no existing entry edited (invariant 8).

How the number was established, rather than taken on report:

- The highest on `origin/main` `10ff31d` is **D210**, measured here with
  `grep -oE '^## [0-9-]+ — D[0-9]+'` over `docs/decisions.md`.
- **Every remote branch was scanned**, not just the landed ones: none
  carries an allocated number above D210. Three carry a `D-NEXT`
  placeholder and nothing else — `fix-false-success`,
  `identity-ran-commit` and `runner-holds-its-own-limits` — and a
  placeholder reserves no number.
- **D143, D154 and D170 stay gaps** (D148, invariant 8). `check-docs.sh`
  still prints its standing note about D170, which is the correct
  behaviour and not a failure.
- `D-NEXT` never appears in the tree: `scripts/check-docs.sh:111` treats it
  as a failure, and the entry was written with its number from the start.

D211 **partly reverses D180**, says so by number, and leaves D180's three
numbered items standing. What it reverses is the unnumbered paragraph at
D180's end that gave the startup reaper `<data root>/outputs/` — the half
D180's own *Reversibility* paragraph had already called "the one
irreversible half".

## Gates

### The docs gates, run before the handoff commit

`./scripts/check-docs.sh` — **`docs ok`, exit 0**, run after the D211
append, after each board splice and after the `status.md` rewrite. Its one
note is the standing gap:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

It went **red twice on the way and both were caught early**, which is why
it was run after every edit rather than once at the end: D211's own header
was **82 columns** (shortened, the title is now "The startup reaper leaves
outputs/ to the sweep") and `docs/status.md` came out at **91 lines** on
the first pass. Both agents on this branch went red on this script first,
on one over-long line each, and the merger did too.

`./scripts/check-orchestration.sh` — **269 passed, 0 failed**, exit 0,
**measured on this integration tip**, not taken from the brief. Unmoved
from the figure at `b73d38c`, `6036d24`, `1dd2383`, `5c2de3b` and
`8455ec8`.

Line budgets, measured and not assumed: `docs/status.md` **90/90** and
`CLAUDE.md` **150/150** — no slack either side. `CLAUDE.md` was not touched
at all. `status.md`'s new paragraph about the regression was **paid for out
of four other paragraphs**: the branch-protection sentence, the v1.0.0
version-number list, the v0.4.0 interoperability narrative and the
consumer's disk bullet each lost a line or two, and the post-push streak
sentence was rewritten from two greens to three in the same three lines.

### The board read line by line, both directions

Required by `workflow.md`, and this batch is exactly the case it was
written for: a batch with no conflict is where a silently reverted row
would be invisible.

**Direction 1 — every branch's own `board.md` line verbatim in the merged
file.** The branch's `board.md` is **byte-identical** to `origin/main`'s
(`cmp -s` on both blobs), so it contributed no line that could go missing;
running the literal comparison is what proves that rather than assuming it.
Of its **356 lines, 355 are verbatim** in the merged file. The one that is
not is **`ORCH-119`** — the row this landing extended with batch 5's
post-push run. **No second row moved.**

**Direction 2 — the merged file differs from the branch's only by this
landing's own hunks.**

```
$ diff <(git show origin/reaper-sweep:docs/orchestration/board.md) \
       docs/orchestration/board.md | grep -E '^[0-9]'
96a97
128c129
159a161,164
```

ORCH-167 added to *Owed* at line 97, ORCH-119's evidence extended at 129,
and ORCH-163 to ORCH-166 added to *Open* at 161-164. Three hunks, five new
rows, one extended row, nothing else and nothing reverted.

`docs/status.md`, against `origin/main` because the branch does not touch
it either:

```
$ diff <(git show origin/main:docs/status.md) docs/status.md | grep -E '^[0-9]'
14,20c14,19
26,33c25,30
41,51c38,51
60,66c60,66
75,77c75,77
```

The four paragraphs that paid, and the fifth that spent. No sixth block.

### Every branch of the batch left its handoff

Checked, not assumed. `reaper-sweep` carries
`docs/orchestration/history/2026-10-29-reaper-sweep.md` (313 lines) and,
from its `collect-gap` base, `2026-10-29-collect-gap.md` (430 lines). Both
are in the merged tree and both are named in the record.

### The mojibake scan, proven before its silence was trusted

`grep` on this host is **ugrep 7.8.4** and its `-P` matches no `\xNN` byte
in any locale, so `grep -rnE 'Ã|Â'` is blind to a mangled em dash. The
detector is a perl byte regex and it was **proven on a known-bad and a
known-good fixture first**:

```
perl -ne 'print "HIT $ARGV:$.\n"
  if /\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]/ || /\xc3\x82\xc2[\x80-\xbf]/'
```

**2 hits** in a fixture corrupted by hand, **0** in a clean one holding a
real `—` and `≤`. Then `docs/decisions.md`,
`docs/orchestration/board.md` and `docs/status.md` after every edit:
**0 hits in all three**, including after the two `perl -i -pe` in-place
edits this landing made. Both were byte-only substitutions with no
`\x{...}` escape in the replacement and no `-0777`, which is the shape that
double-encoded 79 occurrences at batch 5's landing.

**A second ugrep finding, measured here and worth carrying:**
`grep -rh 'skip-list-sha256' <dir> --include='*.receipt'` reported **3**
matching receipts where `find` plus a loop found **17** of 27. The
recursive-with-include form under-reported by a factor of five on this
host. Prefer `find` and an explicit file list when a count matters.

### The Rust gates, separately

**Not run separately by the merger.** The merger's own changes are prose:
`docs/decisions.md`, `docs/orchestration/board.md`, `docs/status.md` and
this file, and nothing else. The branch's Rust changes were gated on the
branch — its receipt is quoted in the record below — and are gated again by
the one `./scripts/gate.sh` run on this integration tip, which runs the
steps of CI's `check` and `docs` jobs in the same order.

### The gate, which runs after this commit

**One `./scripts/gate.sh` run, BARE, on a clean tree, and it is the last
thing before the merge.** Not inside `nix develop`: nesting `TMPDIR` panics
`herd-runner/tests/confinement.rs:2529` on `SUN_LEN` (ORCH-139).

The sha it gates is **this handoff commit** — the integration tip, which is
the **second parent** of the merge commit, not the merge commit itself,
whose message is the record and which therefore cannot exist yet. What has
to hold at the landing is a GREEN receipt for
`git rev-parse <merge sha>^2` **and** `git diff --quiet` between that sha
and the merge commit: an identical tree is what makes the receipt bind.

The receipt it will write:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-6/target/herd-gate/<gated sha>.receipt
```

with `<gated sha>` the full 40 hex of this commit — a value that cannot be
written here, because naming it would change the tree whose sha it is. The
receipt store is **per worktree** (`$HERD_GATE_RECEIPTS`, default
`target/herd-gate/`) and nothing was ever copied into it. `CARGO_TARGET_DIR`
is set **outside** the worktree so the build tree can be deleted after the
push while the small receipts stay.

**The verbatim output and the skip list go to that receipt and to the
merger's report, not into this tree.** A red step blocks the landing, is
reported red, and `main` does not move.

`./scripts/bench-preflight.sh` was run before the heavy phase and said
**ok, safe to start**: 336 GiB free on `/mnt/ssd_980`, one-minute load 0.48
on 12 cores against a 9.00 ceiling, 22 GiB available.

## Not done, and why

- **No tag, no release, no pull request.** The release that carries this fix
  is Alexa's word, one act at a time, and it is what unblocks the first
  consumer's runner upgrade.
- **ORCH-163, ORCH-165 and ORCH-166 are opened, not fixed**, and each needs
  sizing before it can be briefed. ORCH-165 is blocked on ORCH-163.
- **ORCH-167 is opened `owed` and was not put to Alexa.** An automatic end
  for a spool the hub stayed silent about is a policy decision about
  deleting a requester's evidence. It is deliberately in *Owed* and not in
  *Waiting*: five questions are already waiting and `CLAUDE.md` says one at
  a time, so the asking is the orchestrator's next step.
- **One question is still not a row, and it is flagged here rather than
  dropped:** `history/2026-10-29-collect-gap.md` *Decisions needed* 1 —
  whether `finished / exited 0` over an incomplete collection is a defect
  or a choice about `finished` — is Alexa's, the brief for this landing did
  not list it among the five rows, and the merger did not invent a sixth.
  **ORCH-163's *blocked on* cell names it**, so it cannot be lost, and the
  orchestrator owns deciding whether it becomes its own row.
- **`docs/remediation/09-runner-self-limits.md` is untouched**, on purpose.
  It is a dated record of what was known then, its *What is deliberately
  not done* paragraph is quoted in the record, and rewriting it would
  destroy the evidence that the better fix was named before it was built.
- **`herd-hub/tests/spools_resumed.rs` still drives a fake runner over a
  raw socket** and still never calls `Confined::prepare`. That gap is named
  in the record as the lesson; widening the test is not this landing's.
- **`docs/plan.md` and `docs/coverage.md` are untouched.** This batch moves
  no milestone and adds no expectation.
- **`check-orchestration.sh` was not changed**, and its figure did not
  move. The brief asked for it to be measured and for any movement to be
  explained; it did not move.

## Risks and findings

- **The disk half of D180's concern now rests on naming alone.** With the
  reaper out of `outputs/`, nothing ever removes a spool the hub stayed
  silent about: it is renamed, named with its figure at every start, and
  kept. That is correct under D19 and **unbounded in time**. It is
  **ORCH-167**, and the consumer's roughly 396 MiB is exactly that shape if
  their hub has written the job off. `docs/status.md` now says so where it
  records their 125 spools holding 65 GB.
- **`main` carries a fix that no release carries.** Until the release is
  cut, a v0.4.0 runner still deletes spools on startup, so the upgrade
  advice stands and the first consumer stays at 0.3.6. That state is in
  `status.md` and in ORCH-164's *next step*, not only here.
- **Two claims in the brief were wrong in their particulars and are
  corrected on the record**, in D211 and in ORCH-163's row: `root.rs`'s doc
  comment was added by `b250358` itself and so was not in the tree before
  v0.4.0, at `:197-203` rather than `:199-205`; and `load_milli` is not the
  no-bump precedent, because `ToHub::Health::load_milli` arrived in the same
  commit that moved `PROTOCOL_VERSION` from 3 to 4. The conclusions both
  supported still hold, by a different and better citation each time.
- **`grep` on this host under-reports.** Two separate measurements in this
  landing: `-P` matches no `\xNN` byte, and `-r --include` found 3 of 17
  receipts. Nothing in this landing's evidence rests on a `grep` silence.
- **Not verified, and it cannot be from here (D161):** that the first
  consumer's spool still holds their bytes, and that their hub has or has
  not written the job off. Every figure about their machines in this
  landing is quoted from a dated report, never measured.

## The landing record

Written now, at step 5, because it **is** the message of the `--no-ff` merge
commit of step 8. This copy and that message are the same text. The gated
sha appears in it as "`5cb3fcb` plus the merge handoff commit, which is this
commit's second parent": exact and self-resolving — `git rev-parse <merge
sha>^2` — and not writable as 40 hex here, for the reason given under
*Gates*.

Land batch 6: the startup reaper leaves outputs/ to the sweep

- **Batch:** 6, one branch, landed 2026-10-29 by herd-merger. One push, one
  --no-ff merge commit carrying this record, one post-push ci.yml run.
- **Gate:** green -- ./scripts/gate.sh on `5cb3fcb` plus the merge handoff
  commit, which is this commit's second parent and the last change to the
  tree; trees identical, `git diff --quiet` between that sha and this one
  clean. Run BARE, not inside nix develop: nesting TMPDIR panics
  confinement.rs:2529 on SUN_LEN (ORCH-139). The receipt is named for that
  sha under target/herd-gate/ in the merger's own worktree, per worktree,
  never copied in. The resolved 40-hex sha, the verbatim output and the
  receipt path are in the merger's report: each exists only after the gated
  tree was closed, so none of them can be in it.
- **Skips:** 12, the at-rest names of scripts/expected-skips/local.txt, each
  printed as a skip and none counted into a pass (D27, invariant 5). Named
  one by one under *Evidence*. The list decides, never the count (D187); the
  two load-gated flood benches of local-under-load.txt are a delta applied
  only on the refusing bench's own figure (D196) and were not applied here.
- **CI run:** none yet -- this push starts it, so its id goes in the merger's
  report and the next landing's board edit records it. This landing is where
  that duty was discharged for batch 5: run 40896526152, green on 10ff31d,
  is now in ORCH-119's evidence.

## What changed and why

This landing moves no milestone and adds no expectation. It fixes a
**regression in v0.4.0, published earlier today**: the runner's startup
reaper deleted the output spools the runner's startup sweep exists to
re-adopt. The two halves run twenty-eight lines apart on the same startup --
`Confined::prepare` at `herd-runner/src/lib.rs:2634`, then
`Session::pick_up_collections` at `:2662` -- over the same directory, so the
half that recovers a requester's evidence read a directory the other had
just emptied. v0.4.0 shipped both, which means the feature was dead on
arrival in the release that introduced it.

What `main` can do now that it could not before: a runner that was killed
with a streamed collection outstanding can be restarted without losing it.
The reaper owns processes, workspaces and cgroups and nothing else;
`outputs/` is the sweep's alone, and `herd-runner/src/reclaim.rs` holds zero
references to `SPOOL_DIR`. Removal of a spool stays on the hub's sentence
(`files_acknowledged`, D19); on silence the bytes are set aside under a name
that says so and the disk is left to a human. No version number moves, no
frame and no record shape changes.

This is also what unblocks the first consumer, and it is why the batch is
one branch and nothing rode along. Their runner is held at 0.3.6 on the
orchestrator's instruction because roughly 396 MiB of a job's output is
probably sitting in such a spool; the release that carries this fix is what
lets them upgrade without deleting it. The release is a separate act and
Alexa's word.

## Branches in this batch

One branch. The batch is everything ready when the merger started, and that
was one.

1. `reaper-sweep` at 72c333977b5d967d7940fc5f1e94d0ff5cdd4735 -- ORCH-164,
   handoff `docs/orchestration/history/2026-10-29-reaper-sweep.md`. Four
   commits on `10ff31d`, because it is based on `collect-gap` and carries
   the diagnosis with the fix: `7b3bce7` the diagnosis handoff plus the
   then-`#[ignore]`d reproduction, `339d1ba` the fix, `631a83d` and
   `72c3339` the fix's handoff. Five files, +895/-51:
   `herd-runner/src/reclaim.rs`, `confine.rs`, `sweep.rs`, and two files
   under `docs/orchestration/history/`.

`collect-gap` is **not** a second branch of this batch: its tip `7b3bce7`
is an ancestor of `reaper-sweep`, verified with `git merge-base
--is-ancestor`, so its diagnosis handoff -- ORCH-163, ORCH-165, ORCH-166,
`docs/orchestration/history/2026-10-29-collect-gap.md` -- lands inside this
one branch and is named here so a reader is not left looking for it.

## Evidence

### Branch `reaper-sweep` -- its own gate, read by the merger

The branch's receipt was read, not taken on report:

  verdict: GREEN
  sha: 72c333977b5d967d7940fc5f1e94d0ff5cdd4735
  steps: 0..8 all ok
  skips-observed: 12
  load-excused-skips: none
  skip-list-sha256:
  0fbdadcaf14aac20eb9875f41aa74b11349d7aee9df39d3dc93be5b714ba66f9

That hash is the at-rest skip set, and **17 of the 27 gate receipts this
host has kept carry it**, including the five gates run immediately before
it, by their finish times: `8455ec8`, `5c2de3b`, `1dd2383`, `061b134` and
`fd9500c`. The skip set has not moved. The branch's own lib suite reads
`test result: ok. 186 passed; 0 failed; 0 ignored`, against `collect-gap`'s
185 passed and 1 ignored: the reproduction stopped being a skip and became
a test, which is the one direction D27 allows.

Also verified on the branch's merged tree rather than believed: zero
references to `SPOOL_DIR` in `herd-runner/src/reclaim.rs`, and the
reproduction a plain `#[test]` at `herd-runner/src/sweep.rs:683-684`.

### The merge, and the conflict log

One merge, no conflict, no resolution. `git merge --no-ff` of `reaper-sweep`
onto an integration branch off `origin/main` `10ff31d` reported
`Merge made by the 'ort' strategy` and the same five files, +895/-51, that
`git diff --stat origin/main origin/reaper-sweep` reports. Nothing in the
batch touched `docs/decisions.md`, `docs/orchestration/board.md` or
`docs/status.md`, so every docs edit in this landing is the merger's own and
there was nothing to settle.

### The board read line by line, both directions

**Direction 1 -- every branch's own `board.md` line verbatim in the merged
file.** The branch's `board.md` is byte-identical to `origin/main`'s
(`cmp -s`), so it contributed no line that could go missing; the literal
comparison is what proves that rather than assuming it. Of its 356 lines,
**355 are verbatim in the merged file** and the one that is not is
`ORCH-119`, the row this landing extended. No second row moved.

**Direction 2 -- the merged file differs from the branch's only by this
landing's own hunks.** `diff` reports exactly three: `96a97` (ORCH-167
added to *Owed*), `128c129` (ORCH-119's evidence extended with batch 5's
post-push run) and `159a161,164` (ORCH-163 to ORCH-166 added to *Open*).
Nothing else, nothing reverted. `docs/status.md` against `origin/main`:
five hunks, `14,20c14,19`, `26,33c25,30`, `41,51c38,51`, `60,66c60,66` and
`75,77c75,77` -- the four paragraphs that paid for the fifth and the fifth
itself.

### Docs gates, run before the handoff commit

`./scripts/check-docs.sh` -- **docs ok, exit 0**, run after the D211 append,
after each board splice and after the `status.md` rewrite. Its one note is
the standing gap: `D170 is a gap in docs/decisions.md ... gaps stay (D148)`.

`./scripts/check-orchestration.sh` -- **269 passed, 0 failed**, exit 0,
measured on this integration tip and not taken on report. Unmoved from the
figure at `b73d38c`, `6036d24`, `1dd2383`, `5c2de3b` and `8455ec8`.

Line budgets, measured: `docs/status.md` **90/90** and `CLAUDE.md`
**150/150**. There was no slack, so the regression paragraph was paid for
out of four other paragraphs, and `CLAUDE.md` was not touched at all. Two
over-length lines were caught by `check-docs.sh` on the way -- D211's own
header at 82 columns and one status line at 91 lines -- which is why that
script was run after every edit and not once at the end.

### The mojibake scan, proven before it was trusted

`grep` on this host is ugrep 7.8.4 and its `-P` matches no `\xNN` byte, so
the byte detector is a perl regex, and it was proven on a known-bad and a
known-good fixture **before** its silence was trusted: 2 hits in the bad
fixture, 0 in the good one. `docs/decisions.md`,
`docs/orchestration/board.md` and `docs/status.md` are all clean after
every edit, including the two perl in-place edits this landing made.

### The Rust gates, separately

**Not run separately by the merger.** The merger's own changes are prose:
`docs/` and nothing else. The branch's Rust changes were gated on the branch
(above) and are gated again by the one `./scripts/gate.sh` run on this
integration tip, which runs the steps of CI's `check` and `docs` jobs in the
same order.

### The twelve skips, each one named

The at-rest names of `scripts/expected-skips/local.txt`, each printed by the
suite as a `HERD-SKIPPED` line, none counted into a pass (D27, invariant 5),
none of them this batch's, all of them the same missing cgroup v2 delegated
subtree:

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

### The story of the defect, because it was written down before it happened

`docs/remediation/09-runner-self-limits.md`, *What is deliberately not done,
and it is the better fix. H.*, says of re-adopting the spool: "That is a
feature, not a containment fix, and nobody has asked for it." **Two days
later somebody built it and nobody removed the arm that made it useless.**
That paragraph is ORCH-164's whole story, and it is left exactly as it
stands -- a dated record, quoted and not rewritten.

And the lesson, which belongs on the record because the tests were green:
**both halves had a passing test and neither test ran the startup
sequence.** `herd-hub/tests/spools_resumed.rs` drives a fake runner over a
raw socket and never calls `Confined::prepare`; the reaper's own test
reclaimed a spool it had placed itself. Two green tests, one dead feature.
The fix's test calls the runner's own two entry points and re-implements
neither, which the agent rebuilt deliberately after finding that its first
reproduction had copied the startup order rather than called it.

### Two corrections measured at this landing

Both were in the material handed to the merger, both are now right on the
record, and neither changes the fix.

1. **`root.rs` did not say it first.** `DataRoot::outputs_dir`'s doc comment
   -- "the startup reaper clears a workspace, and a collection the hub has
   not acknowledged is the only copy of a result (D19)" -- was added by
   `b250358` **itself**, the commit that built the sweep: `git log -S` over
   `herd-runner/src/root.rs` names that one commit and no earlier one. So
   v0.4.0 carried the rule and its violation twenty-eight lines apart; the
   tree did not "already say" it before the release. The block is at
   `:197-203`, not `:199-205`.
2. **`load_milli` is the wrong protocol precedent.** The field
   `ToHub::Health::load_milli` arrived at `7ca9597`, the same commit that moved
   `PROTOCOL_VERSION` from 3 to 4. What `135510e` added without a bump was
   `JobMetrics::host_load_milli`, a record field and not a frame field. The
   real precedent for ORCH-163 is `18a1c82`, whose own doc comment states
   the rule -- an optional `#[serde(default)]` field on an existing variant
   "is the one change allowed to leave PROTOCOL_VERSION and the hub's record
   version alone (D73)" -- and before M16 both numbers are free-running
   anyway (D73), so a bump would be harmless rather than forbidden. The
   conclusion stands: ORCH-163 needs no version bump. Both corrections are
   written into ORCH-163's row and into D211.

## Left out

- **No tag and no release.** The release that carries this fix is what
  unblocks the first consumer, and it is a separate act on Alexa's word, one
  act at a time. Nothing here touches the published v0.4.0.
- **No pull request.** Nobody opens one (D189), the merger included.
- **ORCH-163, ORCH-165, ORCH-166 are opened, not fixed.** The hub-side
  silence, the orphan `files.part/` and `report`'s missing middle are each a
  row with an owner and a next step, and each needs sizing first.
- **ORCH-167 is opened as `owed` and not asked.** An automatic end for a
  spool the hub stayed silent about is a policy decision about deleting a
  requester's evidence, so it is Alexa's; five questions are already in
  *Waiting* and CLAUDE.md says one at a time, so the asking is the
  orchestrator's next step and not this landing's act.
- **One question is deliberately still not a row**, and it is flagged rather
  than dropped: `history/2026-10-29-collect-gap.md` *Decisions needed* 1 --
  whether `finished / exited 0` over an incomplete collection is a defect or
  a choice about `finished` -- is Alexa's, the brief for this landing did not
  list it, and ORCH-163's *blocked on* cell names it so it cannot be lost.
- **`docs/remediation/09-runner-self-limits.md` is untouched.** It is a dated
  record of what was known then; it is quoted above, never rewritten.
- **`herd-hub/tests/spools_resumed.rs` still drives a fake runner over a raw
  socket** and still never calls `Confined::prepare`. Widening it is not this
  landing's, and the branch's own test covers the startup order instead.
- **The other branches on the origin** were not ready when this merger
  started and are not in the batch.

## Decisions allocated

**D211 -- The startup reaper leaves outputs/ to the sweep.** The only
decision this batch carries, appended at the end of `docs/decisions.md`,
numbered above D210, the highest on `origin/main` `10ff31d` **and on every
branch not yet landed**: all remote branches were scanned and none carries an
allocated number above D210; three old branches hold a `D-NEXT` placeholder,
which reserves nothing. D143, D154 and D170 stay gaps (D148, invariant 8).
Nothing was renumbered and no existing entry was edited: the diff is 95
insertions and 0 deletions. D211 **partly reverses D180** -- its three
numbered items stand, the unnumbered paragraph that gave the reaper
`outputs/` does not -- and says so by number, as D180's own *Reversibility*
paragraph had already called that half "the one irreversible half".

## Board rows

- **ORCH-164** -- the fix. **done**, and the cell says why it is not closed:
  what remains is the release that carries it, and the consumer's upgrade is
  held until then.
- **ORCH-163** -- the hub never records a failed resume and re-asks for ever.
  **open**, with the protocol finding and both corrections above, so the next
  agent does not re-derive them.
- **ORCH-165** -- an orphan `files.part/` is unreachable, unbudgeted garbage.
  **open**, blocked on ORCH-163, which is the same silence from the other end.
- **ORCH-166** -- `report` names the two extremes and misses the middle.
  **open**, recommending a third list in the existing `NAMED_AT_MOST` shape
  and not a flag, because a knob per question is how a reporting tool becomes
  a query language (invariant 1).
- **ORCH-167** -- nothing ever removes a spool the hub stayed silent about.
  **owed**, blocked on Alexa's word.
- **ORCH-119** -- evidence only: batch 5's post-push run `40896526152`,
  `conclusion: success` on `10ff31d`, the third consecutive green, read here
  rather than taken from the brief. That is the duty batch 5's step 9 left to
  this landing.

## CI run

**Recorded, not waited on.** The run this push starts cannot exist before the
push, so its id is in the merger's report to the orchestrator and the next
landing's board edit is what writes it down -- a second push for this batch
is forbidden. The landing was **not** judged risky: one branch, no conflict,
a green branch gate and a green gate on the integration tip, and the batch's
only code change removes a deletion. Its verdict is read at the next session
start, and a failure becomes an `ORCH-nnn` row before new work is briefed.
The run this landing discharged that duty for is batch 5's `40896526152`,
green.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc

## Resume point

`main` carries the fix and D211; the board carries ORCH-163 to ORCH-167 and
batch 5's post-push run. Next, in this order:

1. **Read this landing's post-push `ci.yml` run.** Its id is in the
   merger's report, not here, because it cannot exist before the push and a
   second push for one batch is forbidden. A `failure` becomes an `ORCH-nnn`
   row before new work is briefed (`workflow.md`, *When the post-push run on
   `main` is red*). The **next** landing's board edit is what records it.
2. **The release that carries the fix** — the thing the first consumer is
   waiting for. It is Alexa's word, one act at a time, and `herd-release`
   prepares and never publishes. ORCH-164 stays `done` and not closed until
   then.
3. **ORCH-167 to Alexa**, one question with a recommendation, when a slot in
   *Waiting* frees. Five are already there.
4. **Size ORCH-163**, then ORCH-165 behind it and ORCH-166 beside them.
   ORCH-163's row carries the protocol answer and both corrections, so the
   agent that takes it does not re-derive them.
5. **Decide whether `collect-gap`'s *Decisions needed* 1 becomes a row** —
   the `finished`-over-an-incomplete-collection question. It is Alexa's and
   it is the one thing in that diagnosis that has no id.

Nothing else on this branch is waiting, and the branch's own worktrees are
not this merger's to remove.

## Processes

No background process was started and there is no PID to end; `pkill -f`,
`killall` and killing by name were not used, and the merger started nothing
it did not wait for. One integration at a time: one `gate.sh` run, after
this commit.

The merger's own worktree, created by it and the only tree it wrote in:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-6
```

Its scratchpad subdirectory, holding the fixtures, the spliced rows and the
record:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/b6
```

`CARGO_TARGET_DIR` is set outside the worktree for the gate's four Rust
steps and is deleted once the push is done; the small receipts under the
worktree's own `target/herd-gate/` are kept. Nothing in `/etc/nixos`,
`/srv`, any container, any service or anything of the chatOnS project was
read or written. The orchestrator's checkout at `/mnt/ssd_980/herd-project`
was read from and fetched into, and the **only** thing written to it is
step 10's `--ff-only`, after `main` has moved; what that did is in the
merger's report.
