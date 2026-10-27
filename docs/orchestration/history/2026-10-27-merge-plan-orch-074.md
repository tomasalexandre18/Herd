# merge-plan-orch-074 — handoff 2026-10-27

- **Item:** ORCH-078 (named and closed here), carrying ORCH-074's plan
- **Agent:** herd-merger
- **Branch / commit:** `merge-91-plan-orch-074`, pushed to
  `plan-orch-074` / based on `origin/main` `10f57f7`, PR head `39fe513`
- **PR:** #91
- **Worktree:** the fenced path below; nothing outside it was written.

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merge-91/wt
```

## Done

**A merge was needed and it conflicted in nothing.** The branch was
based on `2d177eb`; `origin/main` had moved to `10f57f7` through PR #90.
`git merge origin/main` resolved by the `ort` strategy with **zero
conflicts**, because the two sides touch disjoint files: the branch adds
`docs/remediation/10-ordering-on-ext4.md` and its handoff, while #90
added `11-sync-prices.md`, `herd-hub/tests/sync_prices.rs`, two handoffs
and edits to `board.md`, `README.md` and `status.md`.

**Nothing was lost, proved both ways rather than asserted.** The two
files the branch carries are byte-identical to PR head `39fe513` after
the merge (`git diff --quiet 39fe513 HEAD -- <file>` on each). Every
file `main` brought is byte-identical to `origin/main`: the net diff
`origin/main..HEAD` restricted to everything *except* those two files
was **empty** before the board edits began.

**ORCH-078 is named, which the previous integrator could not do.** It
is `done`, citing PR #91. The row keeps the finding's own wording — the
plan was on no ref while two briefs were written from it — and records
the cost rather than repairing it away.

**Ids allocated, all verified free first.** The board topped out at
ORCH-078 (#90 having created 077 and 078). Checked against the board,
against the whole repository, and against the open PRs: #85 touches
ORCH-001, 057, 060-064 only; #67 touches no ORCH id. Used:

- **ORCH-075**, **ORCH-076** — new, `waiting`, in `## Waiting`.
- **ORCH-079** to **ORCH-090** — new, the ordered task list, in
  `## Owed`. **ORCH-085 is `open`, not `owed`**: step C is its own
  decision for Alexa and the author wrote it so.
- **ORCH-091** — new, `open`, narrowed (see findings 2).

Rewritten in place, not renumbered: **ORCH-047** → `done` (Alexa,
2026-10-26, *"je valide"*), **ORCH-074** → `done`, **ORCH-078** →
`done`. **ORCH-069** corrected, no figure changed (findings 1).
`## Next` items 1-3 rewritten; the old 3-7 renumbered 4-8.

**`docs/decisions.md` was not touched and no `D` number was allocated:**
the branch carries **no `D-NEXT` entry**. Highest on `main` is **D179**.
PR #85 holds three `D-NEXT` entries, so D180-D182 are its to claim.

**`docs/remediation/README.md`** gained the index row for `10` and the
numbering note was corrected (findings 2). **`docs/status.md`** stays at
**90 lines**, the limit: twelve dossier pieces, not eleven, and the
commit count refreshed from the stale `102 at fba3abb` to 118.

## Not done, and why

- **Nothing was adopted.** ORCH-075 is a question for Alexa and stays
  open; no `fsync` was touched, `07` was not adopted, `08` not reopened.
- **PR #85, PR #67, ORCH-068 untouched**, as briefed. ORCH-073 still
  waits on #85, which is `DIRTY` today.
- **No tag, no release, no issue, no branch deletion**, nothing on the
  consumer's side (D161).
- **The `09` index row** is not written: `09-runner-self-limits.md` is
  on #85's branch and gets its row when that branch lands. ORCH-091.

## Gates

Prose only — the net diff against `origin/main` is **five files, all
under `docs/`**, verified after the board edits:

```
 docs/orchestration/board.md                        |  78 +-
 .../history/2026-10-27-plan-orch-074.md            | 302 +++++++
 docs/remediation/10-ordering-on-ext4.md            | 919 +++++++++++++++++++++
 docs/remediation/README.md                         |  14 +-
 docs/status.md                                     |  18 +-
 5 files changed, 1294 insertions(+), 37 deletions(-)
```

`herd-hub/tests/sync_prices.rs` is in the worktree but **not in this
diff**: it arrived with the merge of `main`, where it already is.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

```
$ ./scripts/check-orchestration.sh
check-orchestration: 111 passed, 0 failed
```

**The four Rust gates were NOT RUN locally** — `cargo build`, `test.sh`,
`clippy`, `fmt`. The change touches no file outside `docs/`, so the
contract does not ask for them and no `cargo` build was started. **CI
runs them on the PR** and CI's result is the verdict.

**Skips: none run here, because no test suite was run.** The standing
figures, each with its host (ORCH-070): **12 on this workstation** (10
cgroup v2 delegation, 2 ZFS `io.max`) and **15 on CI's `check`**, on the
same tree. **CI's 15 is the number a gate is judged on.**

## Risks and findings

1. **Both ORCH-069 corrections the brief asked for were already on
   `main`** — #90's integrator appended them — **but the row still
   contradicted itself**, which is the part that was real work. The
   fourth-round line still read "so there the declared 8 GiB **was**
   honoured and the `-j` shortfall came from the per-thread arithmetic,
   not from a lower announcement", three sentences before the appended
   correction saying the opposite. A correction appended to the end of a
   row while the wrong sentence stays in the middle leaves the row
   asserting both. That line is now rewritten — `slots` declares
   `--host-memory 17GiB` (`slots.rs:697`), so 8 GiB is less than half of
   it and the shortfall combines the lendable-versus-declared gap with
   the per-thread arithmetic. **No figure was changed.** Added: the
   `MemTotal` citation (`herd-runner/src/metrics.rs:291-304`), which was
   missing, and one sentence marking this as a partial answer to the
   row's own first question.
2. **The merge made a sentence on `main` false, and that forced an edit
   the brief did not anticipate.** #90's `README.md` says "**`10` is
   unclaimed on every ref** — no file of that number exists anywhere in
   the repository". Landing `10-ordering-on-ext4.md` falsifies it. Under
   ORCH-049 the index is the integrator's, so the row for `10` and the
   corrected numbering note are written here rather than deferred.
   **ORCH-091 is therefore narrower than the author proposed:** the
   author's wording claims the index has no row for `10` **or** `11`,
   but #90 already added `11`'s row, and `10`'s is now written too. What
   remains is the `09` row when #85 lands, and whether `09` is reused
   deliberately a second time.
3. **The orchestrator's reading of PR #91's state was stale in the
   direction of caution.** `mergeStateStatus` had resolved from
   `UNKNOWN` to **`CLEAN`** / `MERGEABLE` by the time this integration
   read it. The merge of `main` was still done, because a `CLEAN`
   mergeability is not a CI result on the merged tree.
4. **The green CI the brief quoted was for `39fe513`, a head that no
   longer exists on this branch.** Run 40434036184 is not evidence for
   what is merged. CI was re-run on the pushed head and that run is the
   one this integration merged on.
5. **A disagreement between the brief and the author's handoff, resolved
   for the handoff.** The brief says ORCH-079 through ORCH-090 are all
   `owed`; the author's row for **ORCH-085** sets it `open`, owner
   "Alexa, then herd-step", because dropping the record `fsync` is its
   own decision and may never be taken. The author's state is kept.
   `check-docs.sh` does not bind a row's state to its section, so the
   row sits with its task list in `## Owed` while reading `open`.
6. **`docs/status.md` was stale on `main` in a way nothing flagged**:
   it said `main` was "102 commits past v0.3.7 at `fba3abb`" when
   `origin/main` was `10f57f7` at 112. A merge commit's hash cannot be
   known before the merge, so this line will always lag by one
   integration; it now names the integration instead of a bare hash.
7. **No crash evidence was produced or implied by this integration.**
   Step A (ORCH-083) needing none is the *author's* claim, carried
   verbatim into its row and into `## Next`; it is not something this
   integration verified.

## Decisions needed

**None from this integration.** Two rows now wait on Alexa and neither
was created by a decision of mine: **ORCH-075** (adopt the keeper, tier
in three steps — the one that matters) and **ORCH-076** (one sentence on
whether the server's state root is this host's `/mnt/ssd_980`; blocks
nothing).

## Resume point

Put **ORCH-075** to Alexa with `11`'s figures: one question, the
recommendation in the row, the fallback beside it. In parallel, brief a
`herd-step` on **ORCH-079** (the D103 ordering oracle) — it needs no
decision and is green on `main` as it stands. ORCH-080, ORCH-081 and
ORCH-082 can be briefed alongside it: different files, no shared target
directory. **The next integration is PR #85**, still `DIRTY`, carrying
the ORCH-061 and ORCH-064 rows that ORCH-073 waits for.

## Processes

None started and none left running. No `cargo` invocation, no test
binary, no `pkill`. Writes were confined to this worktree.
