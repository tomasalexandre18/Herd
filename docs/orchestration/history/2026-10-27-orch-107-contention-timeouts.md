# orch-107 — handoff 2026-10-27

- **Item:** ORCH-107 (and ORCH-119, opened by this task as a finding)
- **Agent:** herd-step
- **Branch / commit:** `orch-107` / see below, based on origin/main `3bcb018`
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

Two items, two commits.

### 1. ORCH-107 — the contention timeouts

**The in-test build is real, and the prebuild the board named does not
remove it.** `real_herd::binaries` and `capabilities::binaries` each run
`cargo build --offline -p herd-hub -p herd-runner -p herdctl` once per test
process, because `cargo test` does not build another package's *binary* for
this package's integration test. That command resolves a **narrower feature
set** than `cargo test --workspace` does — dev-dependencies unify features
across a workspace — so the two do not share fingerprints, and the in-test
build has real work to do on a tree that was just built. Measured on this
host, each figure from a run whose output is in the branch's scratchpad:

| state of the tree | what the in-test build then costs |
|---|---|
| fresh worktree, right after `cargo build --workspace --all-targets` | **9.70 s**, recompiling rustix, linux-raw-sys, the herd-hub binary |
| fresh worktree, read from inside a test by the new diagnostic | **8.83 s**, and the wait that failed began **84 ms** later |
| warm worktree after a source edit (`touch herd-proto/src/lib.rs`) | **1.88 s** |
| after `cargo build --offline -p herd-hub -p herd-runner -p herdctl` | **0.06 s** |

So `cargo build --workspace --all-targets`, the prebuild the row and three
handoffs recommend, leaves **9.7 s of twelve-way compiling inside the test
process** — which is why it only ever half-worked. Only the identical
command removes it, and the two feature sets coexist once both exist:
prebuild (narrow) → `cargo test --workspace` (wide) → in-test build
**0.06 s**, measured as a sequence.

**The fix is to move that build, not to move a deadline.**

- `scripts/test.sh` runs the tests' own build command first, so no test
  process compiles while timing a hub. It is **advisory**: a failed prebuild
  prints one line and carries on to `cargo test`, because the tests own this
  build and the suite's exit status and skip report are what the script
  exists to print honestly (D27, D179). Exiting there would print neither —
  `check-orchestration.sh` caught exactly that in a first draft of this
  change (4 red cases), and the draft was wrong, not the check.
- `PATIENCE` is **unchanged at 30 s**. Nothing grows with load, nothing
  became a skip. A patience that stretches to fit the machine stops catching
  a product that got slower; a skip whose count moves with the load is a
  count nobody can reason about. Evidence that this is not hypothetical is
  below: the suite already has such a skip.
- Instead, a wait that runs out now says **which**:
  `real_herd::why_a_wait_ran_out` prints what the in-test build cost, whether
  it compiled anything, and when it ended relative to the wait that failed —
  or that it had nothing to do, so the binaries were ready — plus the host's
  one-, five- and fifteen-minute load average over its core count. It is
  used by `herdctl` ("never came back"), `wait_for_runner` and `wait_until`,
  the three waits ORCH-107 names. `herdctl`'s failure also gained
  `everything_said()`, which it did not have (D101's rule, one site short).
- Two deadlines now start **after** `binaries()` has resolved rather than
  before (`herdctl`, `wait_for_runner`), so a first-call build is never
  charged to the product. That is attribution, not leniency.

Verbatim, from a real failure (`PATIENCE` set to 1 ms to force it):

```
why this wait may have run out:
  * the in-test cargo build compiled for 8.830649152s and finished
    83.633566ms before this wait began — every core it could take, on the
    same cores as the hub and the runner this wait is about. Run
    `scripts/test.sh`, which builds them first, before believing anything
    about the product here.
  * host load average 2.36 / 2.03 / 1.66 (1, 5, 15 min) over 12 cores,
    read as this wait ran out
```

**What it still catches.** The deadline is the same 30 s and starts at the
real wait, so a hub that answers slower than it used to still fails, and the
message's first bullet then reads "the in-test cargo build had nothing to
do (…), so the binaries were ready long before this wait: this is the hub,
the runner, or the machine — not the build." A reader can tell the three
apart without a rerun, which is the hour per agent ORCH-107 was costing.

### 2. The batch board check (its own commit)

`docs/orchestration/workflow.md`, *One PR per batch of branches*: a required
step for `herd-merger` — after the batch is merged, require each branch's
exact `board.md` line **verbatim** in the merged file, then check the other
direction, that the merged board differs from each branch's version only by
the other branches' own hunks. The reason is kept with the rule: batch 3 had
**no conflict at all**, three branches wrote more than three lines apart and
git merged every hunk without asking, so the danger was a board merge nobody
had to read, and a silent revert with nothing to resolve is caught by this
and by nothing else. `.claude/agents/herd-merger.md` step 5 gained a
two-line pointer to it, not a second copy of the reasoning.

## Not done, and why

- **The 21 failures at load 19 are not explained.** Four runs under induced
  load gave zero `:649`/`:789` timeouts (below). That event had three full
  suites running at once, which is a different and much heavier load than
  CPU burners, and reproducing it would have meant three concurrent suites
  on the host the first consumer's production hub runs on. Not attempted.
- **`capabilities.rs`'s duplicate `binaries()` was left alone.** The
  prebuild makes it a no-op too, so it needed no change; de-duplicating it
  against `real_herd` is a separate tidy-up and was out of scope.
- **`workflow.md`'s refusal of the local gate was not edited.** Its sentence
  *"the suite is red here for host reasons"* is now out of date — the suite
  is green here — but that paragraph records a decision of Alexa's, and
  rewriting a refusal is not an agent's. See *Decisions needed*.
- No `docs/decisions.md` entry and no `D-NEXT`, as the brief said.

## Gates

All six run in the worktree, each redirected to a file and read from the
file, never piped to `head` or `tail`.

```
cargo fmt --all --check                                   exit 0
cargo build --workspace                                   exit 0
cargo clippy --workspace --all-targets -- -D warnings      exit 0
./scripts/test.sh                                          exit 0
./scripts/check-docs.sh                                    exit 0
./scripts/check-orchestration.sh                           exit 0
```

`./scripts/test.sh` (at rest, after the change):

```
test suite: PASSED
837 tests passed, 0 failed
 12 assertion(s) COULD NOT RUN on this host and were skipped.
```

`./scripts/check-docs.sh`:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**Correction 2026-10-27, by `herd-merger` at batch 4's integration:
`check-docs.sh` is green at `f75b09d` and RED at `af280ef`, the commit
that adds this handoff.** Verbatim on the branch as pushed:

```
docs/orchestration/history/2026-10-27-orch-107-contention-timeouts.md has lines over 79 columns:
    …:194 (106 cols)  …:196 (93)  …:197 (102)  …:209 (96)  …:290 (135)
docs check failed
```

The gate was run before the handoff was written, so the handoff itself
was never gated — and what it caught is **ORCH-117's family**: the width
pass exempts a table row only at column 1 (`check-docs.sh`,
`/^\|/ { next }`), so the four-run table above, indented inside a
numbered item, was width-checked like prose. Three layout changes were
made at integration and **no figure and no word of the evidence was
altered**: the four-run table is the same four runs as a list, the long
test name sits on its own line, and the scratchpad path sits in a
column-1 fence. Recorded on ORCH-117's row.

**One figure in *Decisions needed* 1 is wrong:** CI's skip count is
**14**, not 15 — run 40538722315, `main` at `3bcb018`, job `check`:
`14 assertion(s) COULD NOT RUN`. It does not change the recommendation,
which is stronger with 12 / 14 / 14 than with 12 / 14 / 15: the local
figure moves with the load and still is not CI's.

`./scripts/check-orchestration.sh`:

```
check-orchestration: 141 passed, 0 failed
```

ORCH-111's flaky case, `PATH keeps what it inherited behind the four`,
**passed** on the run that counts; it had failed four cases earlier in this
task and those four were my own regression, not ORCH-111 (see below).

### Skips — 12, and not a host constant

The 12 are the same ones as at rest; each is printed by `test.sh` as a skip
and none is counted as a pass (D27). **But the count moves with the load**,
and this matters for ORCH-107's second candidate fix: under induced load the
same suite reported **14**, the two extra being

```
HERD-SKIPPED: flood-fifty: the host's one-minute load is Some(13.02), over this bench's ceiling of 8; the host is shared with a production system
HERD-SKIPPED: flood-two-fifty: the host's one-minute load is Some(13.02), over this bench's ceiling of 8; the host is shared with a production system
```

So a load-gated skip is not hypothetical — the suite already has one, and it
already makes "12 skips on this host" a figure that is only true at rest.
That is the strongest argument against adding a second one, and it is why
this task added none.

### The red gate I caused, and did not paper over

The first draft of `scripts/test.sh` exited non-zero when the prebuild
failed. `check-orchestration.sh` went red on four cases —
`test.sh: red with an empty skip log reports it unknown` and three
neighbours — because the exit skipped the skip report entirely, which is the
one thing that report must never do. Verbatim:

```
FAIL  test.sh: red with an empty skip log reports it unknown
    exit=1, expected 101
    missing from the report: SKIPS UNKNOWN on this host
```

The fixture was not touched. The prebuild was made advisory, which is also
the better design, and the check went to 141 passed, 0 failed.

## Risks and findings

1. **The prebuild explains less than the board hoped, and the reading is a
   negative one.** Four full suite runs under bounded induced load:

   Each run: its condition, the CPU burners beside the suite, the peak
   one-minute load, the result.

   - **B** — binaries warm, 6 burners, peak 1-min load 9.62, raw
     `cargo test --workspace --no-fail-fast`: green, 837 passed, 3m20.
   - **A** — same tree, 6 burners, peak 1-min load 11.27,
     `./scripts/test.sh`: green, 837 passed, 3m55.
   - **B2** — binaries absent, built **inside** the tests, 8 burners,
     peak 1-min load 17.12: green, 837 passed, 5m05.
   - **A2** — binaries absent, `test.sh` builds them first, 8 burners,
     peak 1-min load 18.15: **1 failure**, 836 passed, 5m20.

   **Zero `:649`/`:789` timeouts in all four**, including B2, which still
   built inside the tests at load 17. So on this evidence the in-test build
   is a real defect that is now fixed, but it is **not** what produced the
   21 failures, and I cannot show that anything I changed would have
   prevented them. The honest residue is: at load up to 18 the two waits
   ORCH-107 names did not fire at all, and the event at load 19–21 remains
   unreproduced. It is neither the harness's nor the product's on this
   evidence — it is a host with more concurrent suites than cores.

2. **A different failure, and it may be the product's — ORCH-119.** Run A2,
   at load 18.15, failed
   `announced_job_ceiling::a_job_refused_past_the_bound_ends_and_names_it`
   at `announced_job_ceiling.rs:1179`: `holding: 2` expected, `holding: 1`
   read. The runner's websocket was reset without a closing handshake, the
   hub said `runner capped-runner is gone; an attempt it was running is lost
   if it is not back within 3s (E-21)`, and the runner redialled ten times
   with `Connection reset by peer`. **It is not a timeout** — the assertion
   ran and was wrong — and that test does not use `real_herd`, so nothing on
   this branch can have caused it. One sample; green at rest and in the
   other three loaded runs. Whose it is is open: a test that assumes the
   link stays up, or a hub that drops a runner which is only slow. Board row
   ORCH-119.

3. **I used ORCH-119 for that finding, not for item 2.** The brief allocated
   the next free id after ORCH-118 to item 2 if it needed one; it did not,
   and the finding above would otherwise have been lost. Say so if the id
   should move.

4. **`cargo clean -p` is much blunter than it looks.** `cargo clean -p
   herd-hub -p herd-runner -p herdctl` removed 10.9 GiB and 22 177 files,
   not three binaries. It was used twice here, inside this worktree only,
   to put the tree in the fresh state; anyone repeating the experiment
   should expect a full rebuild, not an incremental one.

5. **ORCH-111, in passing and not fixed.** `check-orchestration.sh:438` is
   `printf '%s' "$path_val" | grep -qF ":$PATH"` — it compares the file
   under test against the **live `$PATH` of whatever shell launched the
   gate**, so its result depends on the launching environment and not on the
   file. That is a plausible mechanism for its one-run-in-eight failure, and
   it is a one-line change, but it is ORCH-111's row and not mine.

## Decisions needed

1. **Can the local run now be reconsidered as a gate? My answer is: not
   yet, and for a reason the branch itself produced.** ORCH-069 and
   ORCH-107 are both addressed, which was the stated condition, and the
   suite is green here twice at rest and twice under load. But
   `workflow.md`'s refusal gives a *second* reason that this task did not
   remove and in fact confirmed: **skips are per host and now demonstrably
   per load** — 12 here at rest, 14 here under load, 15 on CI. Judging a
   branch on the local number is still judging a different suite.
   *Recommendation:* reconsider it as "green locally is required before a
   branch is handed off" (which it already effectively is) and keep **CI as
   the gate**, until the flood benches' load ceiling is settled. *Fallback:*
   adopt it and accept that a merger must read the skip list, not the count.
2. **`workflow.md`'s refusal paragraph still says the suite is red here for
   host reasons, which is out of date.** *Recommendation:* `herd-merger` or
   a prose task corrects that one clause when Alexa answers 1, in the same
   edit, so the record changes once and with her answer attached.
   *Fallback:* leave it and rely on ORCH-107's board row, which now carries
   the measurement. Not edited here, because rewriting a refusal of Alexa's
   is not an agent's.

## Resume point

Nothing is in flight. For whoever continues:

- The branch is pushed and carries both commits; it needs `herd-merger`
  only, in the next batch. No PR, by the batch rule.
- To settle ORCH-119: `cd` to a worktree, start a bounded CPU load, and run
  `nix develop --command cargo test -p herd-hub --test announced_job_ceiling`
  alone under it, then read the hub's log for whether the runner came back
  inside E-21's 3 s.
- The experiment's raw output — the four loaded runs, the load traces, the
  build-cost measurements and the forced-timeout demonstrations — is in
  `.../scratchpad/orch-107/` under this session's directory. It is scratch
  and is not on the branch.

## Processes

Started and ended by this task, all killed by **PID**, never by name:

- 6 burners for run B: 1606239–1606244
- 6 burners for run A, 8 for run B2, 8 for run A2: PIDs recorded in
  `run-a.pids`, `run-b2.pids`, `run-a2.pids` in the scratchpad
- one load-sampling loop per run, same files

Every one was confirmed gone by `ps -p <pid>` after each run; the final
check listed none. `herd-runner` (PID 2213) and `herd-hub` (PID 3226), a day
old, are the first consumer's own instance and were not touched (D161).

Own `target/` removed once the branch was pushed:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-107/wt/target
```
