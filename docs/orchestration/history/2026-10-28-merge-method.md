# merge-method — handoff 2026-10-28

- **Item:** ORCH-120 (the method), with ORCH-134, ORCH-136, ORCH-137 and
  ORCH-138 for the four branches that carried no number
- **Agent:** herd-merger
- **Branch / commit:** `merge-method` / see *Commits*, based on
  `origin/main` `d73077f`
- **Landing:** **pull request** — this is the one dated exception
  (`workflow.md` *Not an option*, step 1): the last pull request this
  project opens, carrying the change that ends them. Its number, the CI
  run and the merge sha are in the report to the orchestrator, because
  they do not exist until after this file is committed (ORCH-121).

## The batch, in merge order

| # | branch | head | into | handoff |
|---|---|---|---|---|
| 1 | `method-local-gate` | `58dd6f3` | `58ce593` | `history/2026-10-28-method-local-gate.md` (ORCH-134) |
| 2 | `method-push-guard` | `1f9edf1` | `20d3640` | `history/2026-10-28-method-push-guard.md` (ORCH-136) |
| 3 | `method-permissions-truth` | `59e3848` | `a24797a` | `history/2026-10-28-method-permissions-truth.md` (ORCH-137) |
| 4 | `method-landing-record` | `a80ea1a` | `27888ce` | `history/2026-10-28-method-landing-record.md` (ORCH-138) |
| 5 | `method-rules` | `7b3a619` | `2e1521c` | `history/2026-10-28-method-rules.md` (ORCH-120) |

All five were cut from `bc83717`, which is an ancestor of `origin/main`
`d73077f` (`git merge-base --is-ancestor`, verified). Branches 2 and 3
are based on 1 and on 2 respectively, so merges 2 and 3 carried their
ancestors' commits already; 4 and 5 were independent.

## Conflicts

**None — zero conflicts across five `--no-ff` merges**, and that is a
measured fact and not an assumption. `git status --porcelain` was empty
after each merge. The reason is that the product batch of the same day
(`d73077f`, D185/D186) touched `docs/decisions.md`, `board.md`,
`status.md`, three handoffs and four files under `herd-hub/`, and this
batch touched none of them: `git diff --name-only origin/main HEAD --
docs/decisions.md docs/orchestration/board.md docs/status.md` was empty
before this merger wrote in those three files. The two batches are
disjoint by file.

## My own commits, in order

```
ddf70b7 Move the landing-record floor past the last pull request
82bf546 Name the merge's second parent in the landing record
a5939dd Delete the pull request template
2e49efb Record the method batch as D187, D188 and D189
93cd67c Close ORCH-120, add five board rows and restate the state
01695fb Give the batch's five tasks their board rows
dbaff97 Record the long-TMPDIR suite failure as ORCH-139
```

## Decisions allocated

Three `D-NEXT` entries, numbered in merge order above the highest on
`origin/main` (**D186**) and above every open pull request — there were
none (`gh pr list --state open` empty, 2026-10-28). **D143, D154 and
D170 stay gaps** (D148).

| D | from | title |
|---|---|---|
| **D187** | `method-local-gate` | The local gate reads the skip list, never the count |
| **D188** | `method-push-guard` | The push guard is the last machine before main |
| **D189** | `method-rules` | The pull request leaves the loop: one landing per batch |

`method-permissions-truth` and `method-landing-record` each state in
their handoff that they allocate none, and neither does.

## Board rows written

Eleven, all mine (invariant 8). None of the five branches touched
`board.md`, verified by `git diff --name-only`.

1. **ORCH-120 → `done`.** The row that could not wait: `CLAUDE.md`'s
   session start sends every fresh session to the board at step 2, where
   it read a waiting decision proposing the opposite of the contract it
   had just loaded. Answered 2026-10-28 **against the recommendation in
   the row** and wider than its fallback, with Alexa's two quotations,
   **D187**/**D188**/**D189**, and the note that the refusal's two
   factual errors are marked where they occur in `workflow.md` rather
   than rewritten. The recommendation it went past is kept verbatim.
2. **ORCH-129** — `CLAUDE.md` at both ceilings, measured on this tip:
   `wc -l` = **150** against `check-docs.sh`'s `limit=150`, widest prose
   line **79** columns at `:100`, and **12** prose lines at 78 or 79
   columns, so there is no slack to absorb a reflow.
3. **ORCH-130** — `permissions.md` and the rules land together, with the
   standing rule it leaves behind.
4. **ORCH-131** — `core.hooksPath` is relative on this host where the
   contract says absolute. **Open, not fixed here.**
5. **ORCH-132** — the landing-record floor. `done`.
6. **ORCH-133** — the merge condition stated five ways, one copy saying
   the opposite. `open`, because the generative question is whether it
   gets one source and four pointers.
7. **ORCH-134** — T1, `scripts/gate.sh`. `done`.
8. **ORCH-135** — **the gate cannot go green on this host: no
   `scripts/expected-skips/local.txt`.** `owed`.
9. **ORCH-136** — T5, `scripts/githooks/pre-push`. `done`.
10. **ORCH-137** — T3b, `permissions.md` and the two `-c` spellings.
    `done`.
11. **ORCH-138** — T4, the landing record. `done`.
12. **ORCH-139** — a long `TMPDIR` makes the suite red. `open`.

Already present on `origin/main` and **not duplicated**: ORCH-111's
three sample sets of 2026-10-28 including the discriminating experiment,
and **ORCH-128**, the fixed-test-defect class behind D186. Both were
checked before writing anything.

`Next` item 7 lost its stale clause (*"which is why the local run is
still not the gate"*) and item **9** is new: the three transition steps,
which two are not done, and who owns them.

`docs/status.md` has a new section, *How a change reaches `main`*. The
file was already **at** its 90-line cap, so the unreleased-changes
paragraph is compressed to its decision numbers and one clause each;
every D number and every figure it carried is still there or still in
`decisions.md`, and the file is 90 lines again.

## Gates

**Two full runs, and the first one was red for a reason I caused.** Both
are reported, because a gate that was red once is reported red once
(D27's spirit: a run that did not happen as claimed is not a run).

### Gate 1 of 5 — `cargo build --workspace`, on `93cd67c`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.45s

real	0m23.208s
EXIT=0
```

### Run A, on `93cd67c` — RED, and the cause was my environment

```
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (18s)
step 3: nix flake check (ci job: check) — ok (1s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (13s)
step 6: test suite — FAILED (exit 101, 218s)
step 6: the suite did not print 'test suite: PASSED'; this is not a
        complete run and cannot be reported as one (D27, D179).
step 7: the skips this host could not make (ci job: check, annotation)
gate: REFUSED sha=93cd67c8479328ecc1d5842fb8890103ea7fcc58 reason=no expected skip list at …/scripts/expected-skips/local.txt; produce one with --bless after a green suite
gate: NOT GREEN — this sha must not be pushed to main.
GATE_EXIT=3
```

The one failure:

```
thread 'no_descriptor_of_the_runner_or_its_job_reaches_a_runtime_socket' (3567279) panicked at herd-runner/tests/confinement.rs:2529:58:
the fixture binds a socket: Error { kind: InvalidInput, message: "path must be shorter than SUN_LEN" }
test result: FAILED. 38 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.10s
test suite: FAILED (cargo test exited 101)
```

I had exported `TMPDIR` to this task's scratchpad — **131 characters** —
and the fixture binds a Unix socket under it, where `SUN_LEN` is 108.
Not a regression of this batch, which touches no Rust file at all
(`git diff --stat origin/main HEAD` is 21 files, none under `herd-*/`),
and not a product defect either. It is **ORCH-139**, and it is written
down because under the new method it turns a landing red for a reason
that has nothing to do with the landing.

### Run B, on `01695fb` — the suite green, the gate still refused

`TMPDIR=/mnt/ssd_980/herd/claude/tmp` (28 characters), the session's own,
`ops/agents/env.sh:45`. Verbatim:

```
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (0s)
step 2: check-orchestration.sh (ci job: docs) — ok (18s)
step 3: nix flake check (ci job: check) — ok (1s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (8s)
step 6: test suite — ok (189s)
step 7: the skips this host could not make (ci job: check, annotation)
gate: REFUSED sha=01695fb6c650738d1b35e27e405baa28817b7d83 reason=no expected skip list at …/scripts/expected-skips/local.txt; produce one with --bless after a green suite
gate: NOT GREEN — this sha must not be pushed to main.
GATE_EXIT=3
```

**The verdict of the tool this batch adopts, on the very change that
adopts it: `REFUSED`, exit 3 — not red, refused.** Every step it can run
passed; it cannot reach a verdict because
`scripts/expected-skips/local.txt` does not exist and the batch ships
`ci.txt` only. That is **ORCH-135** and it is a sixth precondition of
transition step 3 that `workflow.md` does not list.

### The four Rust gates and the docs gate, named

| gate | where it ran | result |
|---|---|---|
| `cargo build --workspace` | separately, `93cd67c` | **ok**, 23 s |
| `cargo fmt --all --check` | gate step 4, run B | **ok**, 2 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | gate step 5, run B | **ok**, 8 s |
| `./scripts/test.sh` | gate step 6, run B | **ok**, 189 s |
| `./scripts/check-docs.sh` | gate step 1, run B, and again on the tip | **ok** |

Beyond the five: `nix flake check --no-build` **ok**, and
`./scripts/check-orchestration.sh` **243 passed, 0 failed** — the number
the brief expected, which it did **not** read on arrival: see *Risks*.
The intermittent case at `:464` (ORCH-111) did **not** fire in either
run.

### The suite, by figure and by list

```
test suite: PASSED
55 `test result:` lines, 839 passed, 0 failed, 34 ignored
0 occurrences of FAILED in the whole output
12 assertion(s) COULD NOT RUN on this host and were skipped.
A green run does NOT mean these were checked.
```

**12 skips at rest**, load1 **0.46** at the start, below `gate.sh`'s own
load-note threshold of 8, so the two load-gated `flood` benches ran.
Every one of the twelve is the same cgroup v2 delegation refusal under
`/sys/fs/cgroup/chatons.slice/…` — none of them a failure, each printed
as a skip (D27):

```
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
```

### Which tree each run saw, said plainly (ORCH-121)

Run B gated `01695fb`. Two commits came after it and both are
**docs-only**: `dbaff97` (the ORCH-139 row) and this handoff. The Rust
gates cannot be affected by either, and the two docs checks were re-run
on the final tip — `check-docs.sh` **ok**, `check-orchestration.sh`
**243 passed, 0 failed**; their output is in the report. This is the cost
ORCH-121 names and it is paid here rather than hidden: a handoff is a
docs file, so no single run can be both after it and about it. **CI on
the pull request is this landing's actual condition** and it runs
everything on the final tip.
## Risks and findings

Five defects of **one family** — a rule written in N places and fixed in
N−1 — which is the family all three adversarial passes on `method-rules`
found. Three were still live when the batch arrived here.

1. **`check-docs.sh`'s landing rule was red on `main` the moment it
   merged** (ORCH-132, fixed here). `landing_from=2026-10-28` is the day
   the rule lands, and `2026-10-28-merge-product-fixes.md` — written
   under the pull-request method, already on `main`, and not rewritable
   — matched its glob. Measured on the integration tip before the fix:
   `check-docs.sh` exit 1, and **13 of `check-orchestration.sh`'s 243
   cases red**, every one of them a case that runs `check-docs.sh` over
   the real corpus. The floor is now `2026-10-29`. The branch's own
   author had written the move down as finding 3 and nobody made it,
   because the batch was composed on the same date.
2. **`templates/landing.md` demanded a gate on "the exact sha pushed"**
   (ORCH-133, fixed here), in its `**Gate:**` field and in its
   *Evidence* section. That is the exact wording `method-rules`' own
   decision text says permits **no landing at all**, and it was fixed in
   five other places by the N1 review. `landing.md` was cut from
   `bc83717` in parallel and kept it. Nothing mechanical caught it and
   nothing would have.
3. **`core.hooksPath` on this host is relative where the contract says
   absolute** (ORCH-131, **not** fixed here, deliberately).
   `workflow.md` says "**The path is absolute, and that is not a style
   choice**" and speaks of the relative form in the past tense;
   `git config --get core.hooksPath` returns `scripts/githooks`. That is
   pass 3's P4 answered in prose and not on disk. Not fixed because it
   is one setting for all 85 worktrees of other live agents, because
   `workflow.md` itself calls it "one deliberate act, not something an
   agent sets for itself in passing", and because an absolute path set
   **before** the orchestrator's checkout is advanced points at a
   directory that does not exist — which the same bullet calls strictly
   worse than leaving it unset. The row carries the command and the
   order.
4. **`.github/pull_request_template.md` was deleted by no branch of the
   batch** (fixed here). Transition step 1 says this pull request
   carries that deletion "and nothing else of `.github/`"; five
   branches, none of them did it. `ci.yml`'s `pull_request:` trigger is
   untouched (step 3), and `release.yml` and `release-dev.yml` are
   untouched.
5. **The local gate cannot go green on this host at all** (ORCH-135,
   not fixed here). See *The gate* above: `scripts/expected-skips/`
   ships `ci.txt` and no `local.txt`, so a plain run is **REFUSED** at
   step 7 and exits 3. `workflow.md`'s transition step 3 names five
   preconditions and not this one, and it is the one that blocks the
   first local landing. It is also what `herd-step.md` now promises
   prose-only tasks: "what covers them is the batch's own
   `./scripts/gate.sh` run at integration".

### Where the brief was wrong

- **`method-rules` touches four agent files, not three:**
  `herd-merger.md`, `herd-planner.md`, `herd-release.md`,
  `herd-step.md`.
- **Three branches propose a `D-NEXT`, not two.** `method-local-gate`
  proposes one too ("The local gate reads the skip list, never the
  count"), which is D187. The brief named only `method-rules` and
  `method-push-guard`.
- **`.github/pull_request_template.md` is not deleted "in this PR" by
  the batch** — it was deleted by this merger, because no branch had.
- **`check-orchestration.sh` did not read 243 passed on arrival.** It
  read **230 passed, 13 failed**, all from finding 1. The brief's "243
  passed, 0 failed; anything lower means a merge dropped cases" would
  have read as a dropped merge; no case was dropped, the total was 243
  throughout.
- **The pass-3 review's blocker P1 is satisfied by `main`, not by the
  batch.** `real-herd-patience-timeouts` (`ee8dec0`) is an ancestor of
  `origin/main`, so the fifth precondition holds; the review was
  written when it was not.
- **The orchestrator's checkout `/mnt/ssd_980/herd-project` is at
  `860c4f5`**, two merges behind `origin/main` `d73077f`. Not touched:
  advancing it is step 10 of the *new* `herd-merger.md`, which is not in
  force until this lands.

### Not rewritten, and named instead

`docs/orchestration/remediation-report.md` names
`.github/pull_request_template.md` twice (`:75`, `:114`). It is the
dated consolidated report of the 2026-10-24 remediation mandate and
those lines record what that mandate changed, which stays true; it is
not a live instruction and was left alone.

## The pass-3 review's minimum list, item by item

Read before anything was merged, from
`…/scratchpad/review-method-rules/2026-10-28-review-method-rules-pass3.md`.
`method-rules` answered it in `7b3a619`, which the review did not see.
Verified on the integration tip:

1. **P1 — the fifth precondition.** **Landed.** `workflow.md` step 3 now
   says "**Five things must be true before it**" and the fifth is *the
   test fix is on `main`*, written as a state of `main` and in bold that
   it is "not only about the first landing". And the state holds:
   `git merge-base --is-ancestor origin/real-herd-patience-timeouts
   origin/main` → **yes**, `ee8dec0` is on `main`, which it was not when
   the review was written.
2. **P2 — the false lesson.** **Landed.** The handoff now says a task
   branch push gets the contract and nothing else, that `pre-push` would
   not and must not have refused it, and that reading the output
   afterwards is what caught it.
3. **P3 — one copy of the fast-forward block.** **Landed.** The commands
   are only in `herd-merger.md` step 10, with the four `echo`s;
   `workflow.md` keeps the reasoning and says where the commands live,
   naming the two-copies defect as the reason.
4. **P4 — the absolute hooks path.** **Landed in the text, and the text
   is wrong about the host.** `workflow.md` says the path is absolute;
   `git config --get core.hooksPath` says `scripts/githooks`. **ORCH-131.**
5. **P5 — `CLAUDE.md`'s clause.** **Landed.** `:107-110` carries both
   *once* and *on the orchestrator's instruction*.
6. **P6 — a sha behind the green suite.** **Landed.** The sentence
   carries `ee8dec0`, `test.sh exit=0 wall=170s`, 55 `test result:`
   lines and the handoff that names the twelve skips. Measured again
   here: 839 passed, 0 failed, 12 skipped.
7. **The batch composition.** As the review asked, minus
   `real-herd-patience-timeouts`, which is on `main` already.
8. **N1 re-verified post-merge:** "the merge's second parent" is in
   `CLAUDE.md:113`, `workflow.md:13`, `:62`, `:230`,
   `herd-merger.md:86`, `pre-push:26`, `:187-197` — and now in
   `landing.md:60`, which was the one place that said the opposite
   (ORCH-133).
9. **N6's verification duty discharged:** `grep ORCH-120 board.md` on
   `origin/main` showed `waiting` before this landing; the row moves in
   the **same push** as `CLAUDE.md`, so no window exists in either
   direction.

## Not done, and why

- **Transition step 2** — narrowing the branch protection. Alexa's,
  outward-facing, a separate task.
- **Transition step 3** — deleting `ci.yml`'s `pull_request:` trigger.
  It cannot be done in a pull request: that request would produce no
  `check` and no `docs` and could never satisfy the contexts the trigger
  creates. `release.yml` and `release-dev.yml` untouched.
- **`core.hooksPath` not set to the absolute path** — ORCH-131, with the
  reason and the ordering in the row.
- **`scripts/expected-skips/local.txt` not blessed** — ORCH-135. A
  `--bless` run would have manufactured the expectation it then checks,
  which is not the verdict the brief asked for, and committing it would
  have cost a third full suite run for a receipt this landing does not
  use.
- **`orch-125-state-root-report` (`dad7366`) not touched**, as
  instructed; it waits for the next batch.
- **The orchestrator's checkout not advanced.** It is at `860c4f5`, two
  merges behind `origin/main`. Step 10 of the new `herd-merger.md` is
  what advances it and it is not in force until this lands.
- **`docs/orchestration/remediation-report.md` not rewritten** — see
  *Risks*.

## Decisions needed

1. **Is `enforce_admins` turned on?** Carried over from
   `method-rules`: `main`'s history is protected by contract and by a
   client-side hook that `--no-verify` bypasses, against a credential
   the branch protection does not bind. *Recommendation:* ask Alexa — it
   is outward-facing and the cheapest mechanism left. *Fallback:* the
   contract plus `pre-push`, which is what the text claims and nothing
   more.
2. **`core.hooksPath`: who runs the one command, and when** (ORCH-131).
   *Recommendation:* one agent, after this lands and after the
   orchestrator's checkout is advanced, with the absolute path.
   *Fallback:* change the sentence instead and keep the relative value,
   which arms worktrees built from this landing onward and nothing else.
3. **`local.txt`: bless, or state that a landing waits for a quiet
   host** (ORCH-135). *Recommendation:* bless at rest, commit, then one
   plain run to prove GREEN, and add the precondition to step 3.
   *Fallback:* `HERD_GATE_EXPECTED` pointed at `ci.txt`, which judges a
   different suite and is the thing D187 exists to forbid.

## Resume point

```sh
# 1. the two steps this landing does not do
#    step 2 is Alexa's; step 3 is the first local landing
# 2. before any landing can be green (ORCH-135), on a quiet host:
./scripts/gate.sh --bless          # needs test suite: PASSED
git add scripts/expected-skips/local.txt && git commit
./scripts/gate.sh                  # must print gate: GREEN
# 3. then ORCH-131, after /mnt/ssd_980/herd-project has been advanced:
test -x /mnt/ssd_980/herd-project/scripts/githooks/pre-push &&
  git -C /mnt/ssd_980/herd-project config core.hooksPath \
    /mnt/ssd_980/herd-project/scripts/githooks
```

## Processes

PIDs started and ended, each killed by nothing — all three were
foreground commands that exited on their own: the `cargo build`
(`01-build.log`), and the two `gate.sh` runs (`02-gate.log`,
`03-gate.log`). Nothing was signalled, no `pkill`, no process of another
agent or of the consumer touched. One compile at a time throughout: the
build finished before the first gate started, and run A finished before
run B started.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/merge-method`, outside
the worktree, and is deleted at this handoff. `target/herd-gate/` inside
the worktree holds the two gate logs, the two suite outputs and the skip
lists; it is under `.gitignore`'s `target/` and goes with the worktree.
The gate logs are kept at
`…/scratchpad/merge-method/gates/` for the report.

## Where the record for this landing is

`templates/landing.md` describes the message of a `--no-ff` merge commit
on `main`. **This landing is not that** — it is the one dated pull
request of transition step 1 — so the record for it is the **pull
request body**, written from the six sections of
`.github/pull_request_template.md` as that template stood before this
branch deleted it, with the batch table, the decisions, the board rows
and the gate output above repeated there. The pull request number, the
CI run id and the merge sha go in the report to the orchestrator: none of
the three exists until after this file is committed, which is ORCH-121
applied to the merger itself.

`check-docs.sh` does **not** demand the record of this file, and that is
deliberate rather than an escape: the floor is `2026-10-29` (ORCH-132),
so the first handoff the rule will ever check is the first *local*
landing's — which is the right first test for it, since that is the
first handoff that will have a merge commit message to be a copy of.
