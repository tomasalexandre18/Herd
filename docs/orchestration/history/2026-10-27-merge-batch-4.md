# merge-batch-4 — handoff 2026-10-27

- **Item:** ORCH-107 (integrated), ORCH-119 (carried), ORCH-120 (opened),
  ORCH-111, ORCH-116 and ORCH-117 (corrected on their rows)
- **Agent:** herd-merger
- **Branch / commit:** `merge-batch-4` / `8470b05`, based on
  `origin/main` `3bcb018` (re-read in my own worktree)
- **PR:** #103, merged under ORCH-001 on green CI — see *Gates*

## Done

A batch of one branch, `orch-107` (tip `af280ef`), plus a correction
that reaches six places the branch does not touch.

### 1. The merge, and the conflict log

**No conflict, and nothing to resolve.** `orch-107` branched from
`origin/main` `3bcb018` itself — `git merge-base origin/main
origin/orch-107` *is* `3bcb018` — so the merge was a fast-forward,
taken as an explicit merge commit (`a6c5eb9`) so that the batch has one
integration point. Six files, 492 insertions, 12 deletions:
`.claude/agents/herd-merger.md`, `docs/orchestration/board.md`,
`docs/orchestration/workflow.md`, the new handoff,
`herd-hub/tests/real_herd/mod.rs`, `scripts/test.sh`.

Conflict log: **empty**. No file had two sides, no hunk was dropped,
and no `--ours`/`--theirs` was used anywhere.

### 2. The board check, both ways — the rule this very branch writes

Run on the merged board **before the integrator edited it**, which is
the only order in which it means anything:

```text
=== direction B: merged vs branch (a batch of one: expect no difference) ===
(identical)
=== direction A: each line the branch wrote, verbatim in merged ===
2 board-added-lines.txt
VERBATIM OK: | ORCH-107 | **The suite's host-contention flakiness, which ...
VERBATIM OK: | ORCH-119 | **A runner that is merely starved loses its lin...
```

A batch of one is the weakest possible case for the check and it was
still worth running: it is what proves the ORCH-107 row reached `main`
with the author's own amendment intact rather than with `main`'s older
row, which is the silent revert the rule exists to catch.

### 3. The wrong prebuild advice — six places, each handled

The claim: that running `cargo build --workspace --all-targets` first
stops the `real_herd` tests building the binaries they drive inside
their own `PATIENCE`. It is false. `real_herd::binaries` runs
`cargo build --offline -p herd-hub -p herd-runner -p herdctl`, which
resolves a **narrower** feature set — dev-dependencies unify features
across a workspace — so the two never share fingerprints: **9.70 s** of
compiling immediately after `--all-targets`, against **0.06 s** after
the identical command (ORCH-107's measurements, reproduced here as
0.10 s on a warm tree).

| place | what was done |
|---|---|
| `board.md`, ORCH-107's row | the author's own amendment was already there; the sentence boundary it lacked is added, and the integration recorded |
| `board.md`, *Next* 6 | rewritten: ORCH-107 is `done`, the advice this board gave was wrong, and the 21 failures at load 19 stay unexplained. It no longer tells an agent to say "the host" and move on |
| `board.md`, *Next* 7 | a skip count is per host **and per load**, with the measured figures |
| `history/2026-10-27-orch-092-spool-sweep.md` | dated correction after the warning that holds it |
| `history/2026-10-27-merge-batch-orch-080-092.md` | two dated corrections, one in *Risks*, one in the appendix |
| `history/2026-10-27-merge-96-orch-081-hub-stop-path.md` | dated correction after the three-runs claim |
| `history/2026-10-27-merge-97-orch-094-id-highwater.md` | dated correction: its candidate explanation is refuted |
| `history/2026-10-27-merge-batch-docs-gate-orch-batched-pr.md` | dated correction: the run is still green, the reason is withdrawn |
| `history/2026-10-27-orch-069-host-independent-assertions.md` | two dated corrections, one per place |

**No handoff was rewritten in place.** Each keeps its original wording
and gains a dated correction bullet or paragraph immediately after it,
which is the precedent ORCH-112 set (`2026-10-27-orch-record-fixes.md`,
*Done* 1: "a handoff is evidence; nothing was rewritten in place").

**`docs/status.md`** did not carry the advice; it carried its
consequence — "ORCH-107 is what is left" and a stale 836/12. It is
state again, at **90 lines exactly**: ORCH-107 `done`, the skip count
per host *and* per load, the unexplained residue named, 228 commits
past v0.3.7.

**`workflow.md` and `.claude/` carried it nowhere.** Grepped for
`all-targets`, `prebuild`, `PATIENCE` and the phrasings around them
across the whole tree; every other hit is a `clippy --all-targets`
invocation or a *Processes* list, neither of which asserts anything
about the in-test build.

### 4. What the branch does, verified rather than accepted

- **`PATIENCE` is still 30 s** (`real_herd/mod.rs:30` unchanged in the
  diff), nothing grows with load, and **nothing became a skip**: the
  skip set is byte-for-byte the same twelve cgroup-delegation
  assertions as on `main`. Verified against the diff and against the
  run, not taken from the handoff.
- **The author's second reason is real and I re-measured half of it.**
  The suite already has a load-gated skip — `flood-fifty` and
  `flood-two-fifty` above a one-minute load of 8 — which is why a
  second one would compound a count nobody can reason about. 12 here at
  rest, confirmed.
- **`herdctl`'s panic gained `everything_said()`**, which it lacked;
  D101 was one site short and now is not.
- **Two deadlines start after `binaries()` resolves** (`herdctl`,
  `wait_for_runner`). That is attribution, not leniency: the 30 s is
  unchanged and still starts at the real wait.
- **The prebuild in `scripts/test.sh` is advisory**, and it has to be:
  exiting there would skip the skip report, which is the one thing it
  must never do (D27, D179). `check-orchestration.sh` caught exactly
  that in the author's first draft, on four cases, and he changed the
  design rather than the fixture. Those four cases are green here.

### 5. Three rows corrected beyond the branch's reach

- **ORCH-111** gains the mechanism as a **lead and not a measurement**:
  `check-orchestration.sh:438` greps the value under test for
  `":$PATH"`, so it weighs the file against the live `$PATH` of
  whatever shell launched the gate rather than against what `env.sh`
  was handed. It would explain a one-run-in-eight failure; it does not
  yet explain the profile-derived tail with an empty user.
- **ORCH-116** quoted CI's skip count as 15. It is **14**, measured:
  run 40538722315, `main` at `3bcb018`, job `check`, `14 assertion(s)
  COULD NOT RUN` — thirteen names with
  `the_io_weight_verdict_follows_the_device_the_job_works_on` twice.
  `workflow.md` also says 15 and is left standing (see *Not done*).
- **ORCH-117** is **two anchors, not one**. See *Risks and findings* 1.

## Not done, and why

- **`workflow.md`'s refusal of the local gate is untouched**, which is
  the author's judgement and I agree with it. Two clauses in it are now
  wrong — *"the suite is red here for host reasons"* is false, and
  *"12 here against 15 on CI"* is wrong by measurement — but that
  paragraph records a refusal of Alexa's. Both are corrected in the
  **same** edit that records his answer to **ORCH-120**, and the
  ORCH-107 row says so, so the record changes once with the answer
  attached rather than twice with a gap in between.
- **ORCH-119 was not moved.** The brief offered me the judgement. Its
  author used the id the brief had reserved for his second item, which
  needed none; nothing else holds ORCH-119, no row conflicts, and his
  own handoff cites it. An id is an identifier and gaps stay (D148);
  renumbering it would cost a correct citation to buy nothing.
- **No `D` number, and D170 stays a gap.** No entry headed `D-NEXT`
  exists anywhere in the tree (`grep -rn D-NEXT`, every hit is prose
  about the mechanism or a test fixture), no open PR held a number, and
  nothing in this batch is a decision. Highest on `origin/main`: D184.
- **The 21 failures at load 19 stay unexplained** and no row upgrades
  them. Reproducing them needed three concurrent suites on the host the
  first consumer's production hub runs on; the author refused that
  trade and so do I.
- **Nothing on the consumer's side**: no tag, no release, no issue
  (invariant 9, D161). No `herd-runner` or `herd-hub` process on this
  host was signalled; the two about a day old are the consumer's own.

## Gates

All six, on the merged head, each redirected to a file and read from the
file — never piped to `head` or `tail`.

```text
$ nix develop --command cargo fmt --all --check
fmt exit=0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.42s
build exit=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.27s
clippy exit=0

$ nix develop --command ./scripts/test.sh
Building the binaries the real_herd tests drive, so that they do not:
  cargo build --offline -p herd-hub -p herd-runner -p herdctl
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
…
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
test exit=0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check-docs exit=0

$ ./scripts/check-orchestration.sh
check-orchestration: 141 passed, 0 failed
check-orchestration exit=0
```

**837 passed, 0 failed**, summed over every `test result:` line. Host
one-minute load 3.78 before the suite and 2.76 after, over 12 cores.
Nothing was run in parallel with the suite and no second compile ran at
any point.

### Skips — 12, each named, none counted as a pass (D27)

The cgroup-v2-delegation family of `herd-runner --test confinement`, in
the order the report prints them:

```text
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

A **count and not a floor** (D179): the run completed. And a count that
**moves with the load** — 14 under load, because the two flood benches
skip themselves above a one-minute load of 8 — which is why the list,
not the number, is what a reader compares (ORCH-120).

## Risks and findings

### 1. `check-docs.sh` was RED on the branch as pushed

**And its handoff reports `exit 0`.** Green at `f75b09d`, red at
`af280ef`, the commit that adds the handoff — so the gate was run before
the handoff was written and the handoff file itself was never gated.
Verbatim, and this fence is at column 1 on purpose:

```text
docs/orchestration/history/2026-10-27-orch-107-contention-timeouts.md has lines over 79 columns:
    …:194 (106 cols)  …:196 (93)  …:197 (102)  …:209 (96)  …:290 (135)
docs check failed
```

What caught it is **ORCH-117's family, in a second form**: the width
pass exempts a table row only when its leading bar sits at column 1, so
the four-run table indented inside that handoff's *Risks and findings* 1
was width-checked like prose. **A third agent, a third day, the same
anchor** — and a fourth time in this very file, whose own first draft
indented the fence above inside a numbered item and went red for it.
The row now says it is two anchors and not one.

Fixed at integration with **no figure and no word of the evidence
altered**: the four runs are the same four runs as a list, the long test
name is on its own line, the scratchpad path is in a column-1 fence. The
handoff carries a dated correction saying exactly that. **The
generalisable lesson, which is not this batch's to fix:** a gate run
before the handoff is written is a gate that did not run on what was
pushed.

### 2. The branch's "15 on CI" is wrong, and so is `workflow.md`'s.** CI
   reports **14** (run 40538722315). It strengthens the author's
   recommendation rather than weakening it: 12 at rest / 14 under load /
   14 on CI means the local figure matches CI only by coincidence of
   load, which is a worse gate than a figure that is reliably different.

### 3. ORCH-107's fix is a real fix whose effect on the reported symptom
   is unproven, and the row now says so in both directions.** The
   in-test build was real (9.70 s inside a test process timing a hub)
   and is gone. It is **not** shown to be what produced the 21 failures:
   four runs at peak loads 9.6 to 18.2, including two that still built
   inside the tests, gave zero `:649`/`:789` timeouts. I verified the
   negative reading survives into the row and into *Next* 6, because
   the easy drift here is a later reader treating ORCH-107 as the
   explanation.

### 4. I lost my own board edits once, to `git checkout -q HEAD -- .`**
   while checking whether the branch was green before its handoff
   commit. Re-applied from the scripts in my scratchpad, which is the
   only reason it cost a minute: every prose edit in this batch was
   made by a script that asserts its target matches exactly once, and
   the scripts are in
   `…/scratchpad/merge-batch-4/edit-*.pl`. Worth copying.

### 5. `git worktree add` writes into the orchestrator's `.git` It
   does not write a file in the checkout, and it is what every previous
   integration did, but it is the one write outside my worktree that
   this task performed. Noting it rather than leaving it unsaid.

## Decisions needed

1. **ORCH-120 — may this workstation's local run be the gate now?**
   Both fixes `workflow.md` named have landed (ORCH-069, ORCH-107), so
   its own condition is met. *Recommendation:* **not yet** — keep CI as
   the gate and make "green locally before a branch is handed off" the
   local requirement, because a skip count is per host **and now
   demonstrably per load** (12 / 14 / 14), so a local number judges a
   different suite and judges a different one from run to run.
   *Fallback:* adopt it, and require the merger to read the skip
   **list** and never the count. It answers a proposal of Alexa's of
   the same day, so it is in front of him in *Waiting*.
2. **The two stale clauses in `workflow.md`'s refusal go with that
   answer**, in one edit, for the reason in *Not done*. No separate
   question.

## Resume point

- **Nothing is in flight.** PR #103 is merged, `main` carries the
  batch, the branch `orch-107` stays.
- **ORCH-119**, the next piece of real work out of this: start a
  bounded CPU load, then
  `nix develop --command cargo test -p herd-hub --test announced_job_ceiling`
  alone under it, and read the hub's log for whether the runner came
  back inside E-21's 3 s. One target, bounded load, the log — not a
  suite run.
- **ORCH-120** is Alexa's and blocks nothing.

## Processes

**None started that outlive this task, and nothing killed by name.**
Every command ran in the foreground in my own worktree: six gates, the
merge, the board check, `gh`. No background shell, no burner, no
`pkill`, no `killall`. The `herd-runner` and `herd-hub` processes about
a day old on this host are the first consumer's own instance (D161) and
were not touched.

Own `target/` deleted once the branch was pushed and CI was green:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merge-batch-4/wt/target
```
