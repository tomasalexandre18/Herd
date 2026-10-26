# merge-orch-031-brief-worktree — handoff 2026-10-26

- **Item:** ORCH-031 (integration), plus the board rows the branch was told
  not to write
- **Agent:** herd-merger
- **Branch / commit:** `orch-031-brief-worktree`, integrated from worktree
  branch `merge-87-integration` / merge of `origin/main` `08480b3` into
  `24ba5dc`
- **Worktree:** `/mnt/ssd_980/herd/claude/tmp/claude-1002/`
  `-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/`
  `scratchpad/merge-87/wt` (mine alone)
- **PR:** #87

## Done

**The comparison, before merging.** `origin/main` was `08480b3`, the merge
of #72. The merge base with the branch was `c1d5ce7`. The two sides touched
**disjoint** sets of files:

```text
$ git diff --name-only c1d5ce7..HEAD          # the branch's side
docs/orchestration/history/2026-10-26-orch-031-brief-worktree.md
docs/orchestration/templates/brief.md
scripts/check-docs.sh
scripts/check-orchestration.sh

$ git diff --name-only c1d5ce7..origin/main   # main's side
README.md
docs/decisions.md
docs/orchestration/board.md
docs/orchestration/history/2026-10-26-merge-herd-own-claude-root.md
docs/orchestration/state-root.md
ops/agents/env.sh
ops/agents/session.sh
```

**`git merge origin/main` resolved with no conflict at all**, by the `ort`
strategy: `7 files changed, 745 insertions(+), 7 deletions(-)`, all seven
from main's side. **No conflict log, because there was no conflict** — see
"Risks and findings", where the brief expected one in `board.md`.

**`docs/decisions.md`: no `D-NEXT`, no number allocated.** The branch adds
no decision; it enforces one already recorded. The file is untouched by
this integration and still holds 175 entries, max D178, no duplicate, and
the three gaps 143, 154 and 170 — verified on the merged tree.

**`docs/orchestration/board.md`, the only file this integration wrote.**
46 rows before, 49 after; every pre-existing id still present (`diff` of
the sorted id lists shows additions only), and all seven `Next` items
intact.

1. **ORCH-031 → `done`**, the row rewritten in place with what PR #87
   actually built and with the two limits its author states: the rule is
   textual, so it cannot know the directory exists nor that the agent
   worked there; and it only sees briefs that are *archived*, so a brief
   that is never written down is checked by nothing.
2. **ORCH-069 amended, not duplicated.** PR #87's author recommended a new
   row for the two red tests; the row already existed — it landed on
   `main` with #72, after his branch was cut, so he could not have known.
   His figures are sharper than what was in it and are now folded in: the
   `slots.rs:742` guard reads `total_memory_mib() >= 17 * 1024` while the
   behaviour depends on what the runner *announces*, and `a_small_lender()`
   declares a fixed `--host-memory 8GiB --host-memory-floor 1GiB`. **One
   of his figures is corrected** — see "Risks and findings" 3.
3. **ORCH-070 added** to `Open`, as drafted by the #72 integrator and
   handed to this task: a skip count is a per-host figure. `070` was free
   (highest row on `main` was `069`; the highest on any open PR is `064`,
   on #85).
4. **ORCH-071 added** to `Open`: `./scripts/test.sh` printed "Every
   assertion ran on this host: nothing was skipped" on a **red** run in
   which most of the suite never started. Confirmed in the source on this
   tree — `scripts/test.sh` branches on `[ -z "$skips" ]` and never looks
   at `$status`, which it has already captured. Owner `herd-step`, not
   fixed here: it is a gate and wants its own task.
5. **ORCH-072 added** to `Open`: `docs/orchestration/history/README.md`
   does not state that an archived brief is named `<task>-brief.<ext>`,
   which is the convention the new `check-docs.sh` rule keys on. PR #87's
   author recorded it in `templates/brief.md` because the README was
   outside his write paths and says the README is the better home.
6. **`Next` corrected.** Item 4 listed ORCH-031 among "the owed rows that
   nothing blocks"; it is done, so it is out of that list and its residue
   ORCH-072 is named. A new item 7 states the per-host skip rule and
   points at ORCH-070 and ORCH-071.

**The task's own handoff exists** on the branch:
`docs/orchestration/history/2026-10-26-orch-031-brief-worktree.md`.

## Not done, and why

- **The rule ORCH-070 describes was not implemented**, on instruction: it
  changes how a gate is judged and wants a `herd-reviewer` pass first.
  The row is the deliverable.
- **ORCH-071's fix was not written.** `scripts/test.sh` is a gate.
- **`history/README.md` was not edited** (ORCH-072): same reason the branch
  did not — it is a separate task, now tracked.
- **No `ORCH-064` or `ORCH-061` row was written here.** They belong to
  #86's finding and to PR #85; see the #86 section of this session's work
  and "Risks and findings" 4.
- **No tag, no release, no issue, no branch deleted.** #85, #67, ORCH-068
  and ORCH-069 were left alone as work items.

## Gates

The merged tree's diff against `origin/main` includes
`scripts/check-docs.sh` and `scripts/check-orchestration.sh`, which are
outside the prose allowlist, so **all four Rust gates are owed and all
four were run.**

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0

$ ./scripts/check-orchestration.sh
check-orchestration: 93 passed, 0 failed
exit=0

$ nix develop --command cargo fmt --all --check
exit=0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.29s
exit=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.05s
exit=0
```

The 93 `check-orchestration` checks include the six this PR adds: three
that prove the brief rule can go **red** (a brief naming no worktree, one
naming the orchestrator's checkout, one whose only path sits inside a
`git worktree add` command) and three that prove it stays green (an
absolute worktree of its own, a read-only brief, and a handoff whose task
name merely contains the word "brief").

**`./scripts/test.sh` — RED, and red for reasons that are not this
branch's.** Run with `--no-fail-fast`, because plain `test.sh` stops after
the first failing binary and then misreports the skips (that is ORCH-071):

```text
$ nix develop --command ./scripts/test.sh --no-fail-fast

thread 'a_killed_hub_reads_the_reservations_back_off_the_records' (4137999) panicked at herd-hub/tests/admission.rs:419:5:
the inherited job's 4 GiB of declared memory must still be held: [...] on lender, 1.1 GiB free of its 5.1 GiB memory admission, 5 GiB needed: [...]
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.66s

thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' (4151160) panicked at herd-hub/tests/slots.rs:744:9:
  left: "built with -j3"
 right: "built with -j7"
test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.33s

error: 2 targets failed:
 12 assertion(s) COULD NOT RUN on this host and were skipped.
test suite: FAILED (cargo test exited 101)
exit=101
```

**Both failures are ORCH-069 and nothing else.** They are red on a
pristine `origin/main` with no branch content present (verified by the #72
integrator and recorded in that handoff's "Gates"), they were red 3 of 3
run alone on an idle machine for PR #87's author, and CI ran the same
`nix develop --command ./scripts/test.sh` green on this PR and on
`c1d5ce7`. Neither test, neither guard and neither skip was touched.

**Skips: 12 on this host** — this workstation,
`/mnt/ssd_980/herd-project`'s machine, inside
`/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`. The split
is **10 naming cgroup v2 delegation** (`this runner cannot create a cgroup
under /sys/fs/cgroup/chatons.slice/...`) and **2 naming the `io.max`
ceiling and its device**. **CI's `check` reports 15 on the same tree**, so
the 12 is a local figure and the gate's verdict is CI's — which is exactly
ORCH-070's point, and the reason the count is named with its host here.

## Risks and findings

1. **The brief's expected conflict did not exist.** It said to expect a
   conflict in `docs/orchestration/board.md` because "this branch will
   have written its own ORCH-031 row". It had not: the branch deliberately
   left `board.md` untouched — its handoff's "Not done, and why" says so
   in terms, *"three branches are in flight and the board is the file they
   would collide on"* — and `gh pr view 87 --json files` lists four files,
   none of them `board.md` or `decisions.md`. The merge was clean, and the
   board rows in this integration are **written by me, not resolved**.
2. **PR #87's CI was already green on the right content, but on the wrong
   base.** Run `40406116096` passed `check` and `docs` on head `24ba5dc`,
   before `08480b3` existed. `mergeStateStatus` had settled from `UNKNOWN`
   to `CLEAN` by the time this task started. CI is re-run on the merge
   commit below, so the green that authorises the merge is the one on the
   integrated tree.
3. **A figure in PR #87's handoff does not generalise, and ORCH-069 now
   says so.** Its author found `admission.rs:419` withholding "1.5 GiB
   where 1 was declared, identically in all four runs" (6.5 GiB admitted
   against a declared 8), and proposed the default floor
   (`herd_proto::default_memory_floor_mib`) as the explanation. The #72
   integrator saw 6.8 GiB and **this integration saw 5.1 GiB** — 2.9 GiB
   withheld. So the admission is **not a constant offset** from the
   declared figure: a runner told `--host-memory 8GiB` announces less, and
   a different amount each time, which tracks live memory and makes a
   wrong default floor at most part of the story. The row now carries all
   three rounds and names the question to answer first. `slots.rs` moves
   with it: 9.9 GiB, 6.1 GiB and 7204 MiB lent in three rounds, giving
   `-j4`, `-j2` and `-j3` against the required `-j7`.
4. **`ORCH-064` and `ORCH-061` already have rows, on PR #85's branch.**
   #86's handoff reports them missing from the board, which is true of
   `main`. But the `board.md` held by `origin/c5-announced-job-ceiling`
   carries a full `ORCH-064` row (C5 part 3, `active`, naming PR #85 and
   Alexa's `refused_as_full` decision of 2026-10-25) and a full
   `ORCH-061` row. Writing either row here would duplicate #85's and
   guarantee a conflict at its integration, so neither was written.
5. **`check-docs.sh` still notes D170 as a gap and that is correct**: PR
   #67 holds it, #67 is Alexa's to close, and gaps stay (D148). D174 no
   longer notes, because #72 merged.
6. **The new brief rule is inert on CI** by design: it keys on the literal
   path `/mnt/ssd_980/herd-project`, which never appears on a GitHub
   runner. That is the half of the rule that matters only on this machine,
   where the incident happened. The author states this; it is not a defect.

## Decisions needed

**None from this integration.** Standing items untouched and still
Alexa's: PR #67's fate, ORCH-068, and every tag or release.

## Resume point

PR #86 (`status-after-037`) is the next integration, in this session:
re-run its failed `check` job rather than bypassing it, and give its
`ORCH-064` finding a free id above `ORCH-072`.

## Processes

No background job, no service, no container. `cargo` ran three times in
the foreground under `nix develop`, one at a time, with
`CARGO_TARGET_DIR` inside this task's own scratchpad. No PID was killed.
