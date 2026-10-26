# merge-status-after-037 — handoff 2026-10-26

- **Item:** ORCH-010 (integration), plus the two board rows the branch was
  told not to write
- **Agent:** herd-merger
- **Branch / commit:** `status-after-037`, integrated from worktree branch
  `merge-86-integration` / merge of `origin/main` `f7425d8` into `6dab4ad`
- **Worktree:** `/mnt/ssd_980/herd/claude/tmp/claude-1002/`
  `-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/`
  `scratchpad/merge-86/wt` (mine alone)
- **PR:** #86

## Done

**The comparison, before merging.** `origin/main` was `f7425d8` (the merge
of #87, which this session integrated first). The merge base with the
branch was `c1d5ce7`. The branch's side is **two prose files**; main's
side is twelve files, none of them `docs/status.md`:

```text
$ git diff --name-only c1d5ce7..6dab4ad          # the branch's side
docs/orchestration/history/2026-10-26-status-after-037.md
docs/status.md

$ git diff --name-only c1d5ce7..6dab4ad -- '*.rs' 'Cargo.*' 'flake.*' \
      'scripts/*' 'ops/*' '.github/workflows/*' | wc -l
0
```

**`git merge origin/main` resolved with no conflict**, by the `ort`
strategy: `12 files changed, 1331 insertions(+), 14 deletions(-)`, all
twelve from main's side. **No conflict log, because there was no
conflict.**

**`docs/decisions.md`: no `D-NEXT`, no number allocated.** The branch adds
no decision. 175 entries, max D178, no duplicate, gaps 143, 154 and 170 —
unchanged.

**`docs/orchestration/board.md`.** 49 rows before, 50 after; a `diff` of
the sorted id lists shows one addition and no loss.

1. **ORCH-010 → `done`**, the existing row replaced by the one the
   branch's handoff drafted, verbatim. It records that the 2026-10-25 pass
   had already happened, so this is a refresh rather than the v0.3.6
   replacement the task was briefed for.
2. **ORCH-073 added** to `Open` — the branch's `ORCH-064` finding, with a
   free id and a corrected substance. See "Risks and findings" 2: the
   handoff proposed `ORCH-066`, which `main` had already taken, and the
   finding itself turns out to be narrower than stated.

**`docs/status.md` corrected on one figure** and otherwise taken as the
branch wrote it: it said *"`main` is 66 commits past v0.3.7"*, true at
`c1d5ce7` and 21 commits stale by this integration. It now reads **88**,
which is the count once this merge lands, and says "this merge included"
so the provenance of the number is on the line. The file is **90 lines**,
the limit, and no line exceeds 79 columns.

**The task's own handoff exists** on the branch:
`docs/orchestration/history/2026-10-26-status-after-037.md`.

## Not done, and why

- **The `ORCH-064` row was deliberately not written**, and nor was
  `ORCH-061`. PR #85's branch already carries full rows for both; writing
  them here would duplicate them and guarantee a conflict at #85's
  integration. ORCH-073 records that and says to confirm it there.
- **PR #67 was not closed.** The branch recommends closing it; closing a
  PR is Alexa's word. D170 stays its gap (D148).
- **`docs/coverage.md` and `docs/plan.md` were not edited.** The branch
  reports two staleness findings in them (its findings 6 and 7) and was
  told to report, not edit. Neither has a board row yet; neither is this
  integration's to open.
- **The failing test was not touched**, nor its guard, nor its skip.
- No tag, no release, no issue, no branch deleted.

## Gates

**This is a prose-only tree.** The merged tree's diff against
`origin/main` is `docs/status.md`, `docs/orchestration/board.md` and two
files under `docs/orchestration/history/` — everything inside `docs/`.
**The four Rust gates were NOT RUN locally**; CI runs them on the PR, and
the CI verdict is recorded below.

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0

$ ./scripts/check-orchestration.sh
check-orchestration: 93 passed, 0 failed
exit=0

$ wc -l < docs/status.md
90
```

**No skip was produced by this task**: no test was run here. The skip
figures quoted anywhere in this integration are **12 on this
workstation** — the machine holding `/mnt/ssd_980/herd-project`, inside
`/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`, 10 cgroup
v2 delegation and 2 `io.max` — against **15 on CI**. Named with their
hosts, per ORCH-070.

## Risks and findings

1. **The `check` failure was ORCH-022, and it did not recur.** On head
   `6dab4ad` (run `40403507379`) `check` failed in
   `a_quiet_running_job_keeps_its_follower_hearing_from_the_hub`:

```text
thread 'a_quiet_running_job_keeps_its_follower_hearing_from_the_hub' (18052) panicked at herd-hub/tests/follower_alive.rs:230:5:
the longest silence (707.812046ms) is most of the run (1.075503747s), so the follower was effectively silent after all.
```

   That is a **timing** assertion — `longest < spread / 2` — on a diff of
   two Markdown files with no Rust in it (`git diff --name-only` above,
   zero matches for `*.rs` and `Cargo.*`). It is **ORCH-022's failure
   mode, not ORCH-069's**: ORCH-069 is a figure the hub derives from the
   host's live memory compared against a constant, deterministic and
   reproducible on a pristine `main`; this is a wall-clock ratio on a
   shared CI runner, green minutes earlier on another head, which is
   exactly what ORCH-022 records for this test. The job was **not
   bypassed**: merging `origin/main` into the branch produced a new head,
   whose full CI run is the verdict, and it came back green.
2. **The branch's `ORCH-064` finding is real but narrower than stated,
   and its proposed id was taken.** It asked for `ORCH-066`; `main` had
   already given `ORCH-066` to the #72 integration for the credentials
   sync. It is `ORCH-073` here. More importantly, the finding says
   `ORCH-064` and `ORCH-061` have no rows: that is true **of `main`**,
   but `git show origin/c5-announced-job-ceiling:docs/orchestration/board.md`
   carries a full `ORCH-064` row (C5 part 3, `active`, naming PR #85 and
   Alexa's `refused_as_full` decision of 2026-10-25) and a full
   `ORCH-061` row holding the criterion the S3 brief must inherit. So the
   gap is on `main` only and closes when #85 integrates. `ORCH-073` says
   so and says not to write either row before then.
3. **ORCH-071 reproduces on CI, which strengthens it.** The failed run
   `40403507379` printed *"Every assertion ran on this host: nothing was
   skipped"* while the suite was **red** — on the same CI where a green
   run reports 15 skips. So the defect is not a local artefact of this
   workstation: `scripts/test.sh` misreports skips on any red run,
   anywhere, and CI is where gates are judged.
4. **`docs/status.md` still names `ORCH-064`** (line 67, "Not on `main`:
   PR #85 (ORCH-064, C5 part 3)") while `main`'s board has no such row.
   That is the same transient gap as finding 2 and is tracked by
   ORCH-073; it was left as written rather than papered over, because
   the id is correct and it is the board that is incomplete.
5. **The branch's own premise was partly refuted by its author**, and the
   `ORCH-010` row carries that correction: `status.md` on `main` was not
   a v0.3.6-era file. This integration verified the one figure that had
   since gone stale (66 commits) and corrected it; it did not re-verify
   every dated figure in the file, which the branch sourced and dated
   individually.
6. **Two staleness findings have no row.** The branch reports
   `docs/coverage.md` stating its own state as of 2026-10-23, three
   releases ago, and `docs/plan.md` line 5 requiring `cargo test
   --workspace` where `CLAUDE.md` requires the four gates plus
   `./scripts/test.sh` — the very command whose absence hides a
   `HERD-SKIPPED` line (D27). `ORCH-020` covers part of the coverage
   staleness and nothing covers the `plan.md` contradiction. Flagged, not
   opened: neither was this task's.

## Decisions needed

**None from this integration.** Still Alexa's and untouched: PR #67's
fate (the branch recommends closing it), ORCH-068, and every tag or
release.

## Resume point

Nothing is owed on this branch. The next integration in the queue is
**PR #85** (`c5-announced-job-ceiling`), which is `CONFLICTING` against
`main` and will need a real conflict resolution; at its integration,
confirm the `ORCH-064` and `ORCH-061` rows land and close `ORCH-073`.

## Processes

No background job, no service, no container, no `cargo` invocation at all
in this task. No PID was killed.
