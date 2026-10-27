# merge orch-077-price-the-syncs — handoff 2026-10-27

- **Item:** ORCH-077 (integration); board rows ORCH-077, ORCH-078, ORCH-069
- **Agent:** herd-merger
- **Branch / commit:** `orch-077-price-the-syncs` / `3a59c05`, based on
  origin/main `2d177eb`
- **PR:** #90

## Done

**No merge and no rebase were needed, so nothing could be lost.**
`git merge-base HEAD origin/main` returned `2d177ebf…` — exactly
`origin/main` — and `git log HEAD..origin/main` was **empty**, so the
branch was already on top of `main` and the integration is a
fast-forward. There was no conflict to resolve and therefore no conflict
log. `gh pr view 90` independently reported `MERGEABLE` /
`mergeStateStatus: CLEAN`.

**What the branch carries**, `git diff --name-status origin/main...HEAD`,
4 files and +1543 −2 as briefed:

```text
A	docs/orchestration/history/2026-10-27-orch-077-price-the-syncs.md
A	docs/remediation/11-sync-prices.md
M	docs/remediation/README.md
A	herd-hub/tests/sync_prices.rs
```

A path filter for `herd-hub/src`, `herd-proto`, `herd-runner`, `herdctl`,
`scripts/`, `docs/decisions.md`, `docs/orchestration/board.md`,
`docs/status.md` and `Cargo*` over that diff returned **nothing**, so the
branch changes no product code and no file an integrator owns.

**Decision numbers: none allocated.** `docs/decisions.md` is byte-identical
to `origin/main` on this branch (`git diff origin/main...HEAD --
docs/decisions.md` is empty), and `grep -rn 'D-NEXT'` over the whole tree
finds no placeholder in any file this branch adds or changes — every hit
is pre-existing prose on `main` describing the mechanism. The highest
number on `origin/main` is **D179**; the open PRs are **#90** (allocates
none), **#85** and **#67**. `D143`, `D154` and `D170` stay gaps (D148);
`check-docs.sh` notes D170 as held by an open PR.

**The committed tree is the formatted one** — the one thing nobody had
checked. The working tree was clean before and after
`cargo fmt --all --check`, which exited **0**, so the author's four
rustfmt diffs were committed and nothing unformatted slipped in.

**The measurement harness stays out of the gate.** In the suite's own
output, `tests/sync_prices.rs` reports `running 3 tests` with all three
`ignored, a bench; run it on purpose`, i.e. **3 ignored, 0 passed; 0
failed** — so no measurement ran inside a timed gate.

**One defect found and fixed here** (see *Risks and findings* 1): a stray
blank line between the `08` row and the new `11` row broke the GFM table
in `docs/remediation/README.md`. One blank line deleted; no word of the
author's text changed.

**Board**, `docs/orchestration/board.md`, three rows:

- **ORCH-077 → `done`**, a **new** row (the highest id on the board at
  `2d177eb` was ORCH-074), appended after ORCH-074 in *Open*, where the
  other finished remediation rows sit (ORCH-071 is filed the same way).
  Taken **verbatim** from the author's handoff.
- **ORCH-078 → `open`**, a new row, appended after it. Worded as the
  orchestrator directed — the plan was written in its author's worktree
  and never committed, which is a recorded miss and not a mystery, and
  the row tracks the author being resumed to push it as its own PR. **No
  such PR was open when I wrote the row**: `gh pr list` showed only #90,
  #85 and #67, so it is unnamed and the next integrator names it.
- **ORCH-069 → stays `open`**, with the fifth round *and* my own sixth
  round appended. Every figure already in the row is kept and its first
  question still stands first. I did **not** make that question belong to
  the admission test alone, because my round refutes the premise for that
  — see *Risks and findings* 2 and 3.

**`docs/status.md`**: the dossier line said it "holds ten pieces" and now
holds eleven, so `11` is named there. The file stays at exactly **90**
lines; the figures live in the board row, the README index and the
dossier rather than in state.

The task's own handoff is present at
`docs/orchestration/history/2026-10-27-orch-077-price-the-syncs.md`.

## Not done, and why

- **No decision number allocated** — the branch proposes none and
  `docs/decisions.md` is untouched. Nothing to renumber.
- **No conflict log** — there was no conflict; the branch was already on
  `origin/main`.
- **ORCH-069 not fixed and no test edited.** The suite is red and that
  red is ORCH-069's, not this branch's. Not mine to settle.
- **I did not re-run the two failing targets on a pristine `origin/main`**
  this round. I did not need to in order to clear the branch: both test
  files are byte-identical to `origin/main` (checked explicitly), the
  branch touches no `src`, and the row already records both failing on
  `origin/main` alone. I am not claiming a fresh `main` reproduction.
- **ORCH-078's PR is not named** — none was open. See above.
- **Left alone as instructed:** #85, #67, ORCH-068, the plan's PR. No
  tag, no release, no issue, no branch deletion, nothing on the
  consumer's side.

## Gates

All run in my own worktree, `CARGO_TARGET_DIR` in my scratchpad, one
`cargo` at a time. The diff adds a test file, so all four Rust gates are
owed and all four were run.

```text
$ nix develop --command cargo fmt --all --check
(no output)
fmt exit: 0
git status --porcelain: empty before and after
```

```text
$ nix develop --command cargo build --workspace
   Compiling herd-hub v0.3.7 (…/merge-90/wt/herd-hub)
   Compiling herdctl v0.3.7 (…/merge-90/wt/herdctl)
   Compiling herd-runner v0.3.7 (…/merge-90/wt/herd-runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.21s
build exit: 0
```

```text
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Checking herd-hub v0.3.7 (…/merge-90/wt/herd-hub)
    Checking herdctl v0.3.7 (…/merge-90/wt/herdctl)
    Checking herd-runner v0.3.7 (…/merge-90/wt/herd-runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.87s
clippy exit: 0
```

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

```text
$ ./scripts/check-orchestration.sh
check-orchestration: 111 passed, 0 failed
exit 0
```

Both docs checks were re-run **after** my board, status and README edits
and are still green.

**`nix develop --command ./scripts/test.sh` — RED, exit 101.** Two
targets failed, not one:

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
```

```text
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' (1206968)
  panicked at herd-hub/tests/real_herd/mod.rs:789:13:
timed out waiting until the job is running.
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 38.93s
```

```text
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' (1221334)
  panicked at herd-hub/tests/slots.rs:744:9:
  left: "built with -j1"
 right: "built with -j7"
```

with the hub's placement line in that same failure reading
`4.6 GiB lent, 1 GiB kept as its own floor` and `-j 1 from 4729 MiB and
20 threads, 3546 MiB for this slot (3 of 4), 1792 MiB per build thread`.

The new harness did not run:

```text
     Running tests/sync_prices.rs (…/debug/deps/sync_prices-4fa466c4714d7c73)
running 3 tests
test a_whole_pass_of_waiting_writes ... ignored, a bench; run it on purpose
test each_sync_priced_alone ... ignored, a bench; run it on purpose
test the_eight_subsets_of_one_waiting_write ... ignored, a bench; run it on purpose
```

**Skips, with the host named (ORCH-070):** **AT LEAST 12** on **this
shared workstation**, reported as a floor and not a count because the run
was red and did not finish — D179 working as intended:

```text
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
```

Twelve `HERD-SKIPPED:` lines were emitted, ten for cgroup v2 delegation
and two for a ZFS data root with no hard `io.max`. **CI's `check` reports
15** on the same tree, and **CI's number is the one the gate is judged
on**. No skip is reported as a pass (D27).

**CI on PR #90, run 40431461428, at `3a59c05`:**

```text
check	pass	7m49s
docs	pass	2m30s
```

## Risks and findings

1. **A broken table in `docs/remediation/README.md`, found and fixed
   here.** The author's new `11` index row was separated from the table
   by a blank line, which in GFM ends the table — so the row would have
   rendered as a literal paragraph of pipes, not as a row of the dossier
   index, defeating the only purpose it has. `check-docs.sh` does not see
   this and passed either way. I deleted the one blank line
   (`1 file changed, 1 deletion(-)`, whitespace only). Flagging it
   because it is the author's content and I changed its rendering.

2. **The brief's "`admission` was green, so only one of the two failed"
   does not hold this round.** In my run **both** targets failed. So
   **"both reproduce on this host" now has a round in which both did**,
   and the first question of ORCH-069 does **not** belong to the
   admission test alone. I recorded both rounds rather than replacing the
   author's, and left the question where it was.

3. **"The declared 8 GiB was honoured in full" is wrong, in the author's
   row, in the brief, and in the row's existing fourth-round line.**
   `slots` declares **`--host-memory 17GiB`** (`herd-hub/tests/slots.rs:697`),
   not 8 GiB — the 8 GiB belongs to `admission`'s `a_small_lender()`
   fixture. So the fourth round's `8 GiB lent` and the fifth's `8.4 GiB`
   are both far **under** what `slots` declares, not a declaration
   honoured, and the `-j` shortfall is the lendable-versus-total gap the
   row already names rather than per-thread arithmetic alone. Evidence:
   the test's D27 guard is `total_memory_mib()` compared against
   `17 * 1024` through `is_some_and` (`slots.rs:742`), and
   `total_memory_mib()` reads **`MemTotal`**
   (`herd-runner/src/metrics.rs:291-304`), which is 31.1 GiB on this host
   and constant across every round — while the lent figure moved
   9.9 → 8.4 → 4.6 GiB with load. The guard therefore always passes here
   and the assertion it guards depends on something else entirely. This
   sharpens a diagnosis the row already carried; I did not invent it.
   I corrected the wording in the row and changed no figure.

4. **My run is a sixth data point, not a verdict on the branch.** It is a
   shared workstation under other load; `MemAvailable` was 22.8 GiB and
   load 1.36 when read **just after** the run, not sampled during it, and
   I say so in the row rather than implying otherwise.

5. **Nothing in this branch can have caused the red.** Both failing test
   files are byte-identical to `origin/main`
   (`git diff --quiet origin/main HEAD -- …` for `slots.rs`,
   `admission.rs` and `real_herd/mod.rs`), the branch touches no `src`,
   and CI is green on this exact tree with both targets included.

6. **ORCH-077 had no row to move**, so `done` is recorded on a row created
   at integration rather than transitioned. That is what the brief and the
   author's handoff both describe, but it means no prior row corroborates
   the item's framing.

## Decisions needed

**None for Alexa from this integration.** Nothing here changes a
specification or an invariant: no product code, no decision entry, no
protocol change.

One thing the orchestrator should pick up, not Alexa: **ORCH-069's first
question now has a concrete, testable answer available** — the guard reads
`MemTotal` while the behaviour tracks the runner's announced figure — and
both tests are now observed failing in the same round. The row's own next
action ("either the guard and the constants read the lendable figure, or
the fixture pins the runner's lending") is unchanged and still owned by
`herd-planner`.

## Resume point

PR #90 is merged and `main` carries it. The next integration is **#85**
(`CONFLICTING`, and it carries the ORCH-073 rows), which is a separate
task and was left untouched. ORCH-078's PR — the missing
`10-ordering-on-ext4.md` — was not open at this integration; whoever
integrates it names it in the ORCH-078 row.

To see this integration's state:

```sh
git -C <a worktree> fetch origin && git log --oneline -3 origin/main
```

## Processes

One `nix develop --command cargo build`, one `cargo clippy`, one
`cargo fmt --check` and one `./scripts/test.sh`, each run in the
foreground in my own worktree, one at a time, each exited on its own. No
background process, no `pkill`, no process killed by name. My
`CARGO_TARGET_DIR` was
`…/scratchpad/merge-90/work/target`; I wrote nowhere outside my worktree
and my scratchpad.
