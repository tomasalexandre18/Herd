# method-local-gate — handoff 2026-10-28

- **Item:** T1 of the local-merge method (plan of 2026-10-28, herd-planner)
- **Agent:** herd-step
- **Branch / commit:** `method-local-gate` / `0a3994f`, based on
  origin/main `bc83717`
- **Landing:** none — an agent pushes its branch and opens no PR;
  `herd-merger` names the landing here

## Done

`scripts/gate.sh` exists: one command, nine steps numbered 0 to 8, each
naming the `ci.yml` job it stands for, every step's output appended to one
file whose last line is the verdict.

| step | id | covers the ci.yml job |
|---|---|---|
| 0 | tree, sha and host | — (replaces `actions/checkout`'s guarantee) |
| 1 | `check-docs` | docs |
| 2 | `check-orchestration` | docs |
| 3 | `nix-flake-check` | check |
| 4 | `cargo-fmt` | check |
| 5 | `cargo-clippy` | check |
| 6 | `test-suite` (`scripts/test.sh`, captured to a file) | check |
| 7 | the skips: `report-skips.sh` plus the list diff | check (annotation) |
| 8 | the verdict and the receipt | — |

`actions/cache` is the only CI step with no local equivalent, and it is an
optimisation rather than a claim.

Three rules decide the design, each answering a way a local run can lie.

1. **A count is not a signal; a list is (D27).** The gate compares the
   run's skips against a committed list and is red on any assertion
   **name** that is not on it, loud but green when an expected name does
   not appear, and silent on the number. A skip's identity is the text
   between `HERD-SKIPPED: ` and the first colon; the reason that follows
   is kept in the expected file for the reader and deliberately not
   compared, because it carries a temporary directory and a pid that
   change on every run.
2. **A load-dependent run is not a run.** Step 0 reads load1, applies the
   load-conditional list above `HERD_GATE_LOAD_NOTE` (8) and says so, and
   refuses outright above `HERD_GATE_LOAD_MAX` (24).
3. **A gate run on a tree that is not what gets pushed proves nothing.**
   Step 0 refuses a dirty tree; step 8 writes a receipt named for the sha,
   and only on GREEN; `./scripts/gate.sh --verify-receipt <sha>` exits 0
   only for a GREEN receipt whose own `sha:` field matches its file name.
   That is the one call `scripts/githooks/pre-push` needs (T5), so receipt
   semantics live in one place.

Three verdicts, and only the first authorises anything:

```
gate: GREEN sha=<40 hex> steps=0-8:ok host=<host> load1=<load>
gate: RED sha=<40 hex> failed=<step ids>
gate: REFUSED sha=<40 hex> reason=<why it did not run>
```

RED and REFUSED both also print `gate: NOT GREEN — this sha must not be
pushed to main.` and exit non-zero (1 and 3).

`scripts/expected-skips/` holds the lists, under `scripts/` and not under
`docs/` deliberately: `check-docs.sh` width-checks `docs/*.md` at 79
columns and a skip line is far longer than that. `README.md` says what
each file is; `ci.txt` carries the fourteen lines of green CI run
40564336703 for comparison.

`scripts/check-orchestration.sh` gains one section, 26 cases, **172
passed, 0 failed** against 146 before. Eleven are red paths, because a
gate only ever seen green is not known to work: a red step (clippy), a
red docs step, a red suite, a dirty tree, an unexpected skip (named, not
counted), the same skip below the load threshold still red, a suite that
never printed `test suite: PASSED`, `--bless` on a red suite, no expected
list at all, a load above the ceiling, a receipt for another sha, a green
receipt renamed onto another sha, and a red run that must leave no
receipt. Each runs the real `gate.sh` in a scratch git repository whose
`nix`, `cargo`, `test.sh`, `report-skips.sh` and both check scripts are
stubs, so the gate's own logic is under test and no toolchain is needed.
No new whole-corpus `check-docs.sh` run, so that script's 32× multiplier
is unchanged.

Whole-script timing of `check-orchestration.sh`, same host, same load
window (load1 2.4 to 3.0), each measured twice:

```
before (origin/main, 146 cases): 11.95 s real, 7.51 user, 5.85 sys
before (origin/main, 146 cases): 12.04 s real, 7.45 user, 6.00 sys
after  (this branch, 172 cases): 14.54 s real, 8.63 user, 7.62 sys
after  (this branch, 172 cases): 14.47 s real, 8.54 user, 7.62 sys
```

Plus 2.5 s, plus 21 %, far inside the `docs` job's five-minute cap.

### H3's at-rest half is ANSWERED: the list is stable, three times of three

Three full runs of the suite on the same sha and the same tree, at three
different loads, produced **the same twelve assertion names**.

| run | load1 at start | step 6 | skip names |
|---|---|---|---|
| sample 1, gate | 1.99 | FAILED, exit 101, 226 s | 12 |
| sample 1b, gate | 0.85 | FAILED, exit 101, 235 s | 12 |
| run 3, instrumented | 0.91 | FAILED, exit 101, ~250 s | 12 |

Sample 1b ran against sample 1's list and step 7 printed, verbatim:

```
  observed skip lines: 12 (the count is
  not the signal; the list is)
  expected list: .../samples/expected-from-sample1.txt
  the skip list matches the expectation exactly.
step 7: the skips — ok
```

Run 3's names `diff` empty against both. The lists:
`…/scratchpad/method-local-gate/samples/names-1.txt`, `names-1b.txt`,
`names-3.txt`. The logs: `…/samples/sample1-at-rest.log`,
`…/samples/sample1b-at-rest.log`,
`…/diag/suite-full-timestamped.txt`.

The twelve names, for the record:

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job (the named memory outcome)
an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device branch)
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces (the cgroup branch of the report)
```

The under-load half of H3 is **not** measured: see *Not done*.

### The suite is red on this host, reproducibly, three times of three

Every full-workspace run failed with the same three assertions, all of
them 30 s `PATIENCE` timeouts in `herd-hub/tests/real_herd`:

```
a_result_the_hub_cannot_write_is_kept_until_it_can
  -p herd-hub --test killed_hub   real_herd/mod.rs:774
  31.82 s / 31.88 s / 31.87 s
a_drain_first_reservation_survives_a_kill_nine_on_either_side
  -p herd-hub --test owner_control  real_herd/mod.rs:926
a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it
  -p herd-hub --test owner_control  real_herd/mod.rs:926
  30.43 s / 30.38 s / 30.41 s
```

**"Flake" is the wrong word.** Three runs of three, at load1 1.99, 0.85
and 0.91 on a twelve-core host. The same two targets run on their own at
load1 1.28 are green: `18 passed; 0 failed` and `20 passed; 0 failed`
(`…/rerun-failing-1.txt`). The failure appears only when the whole
workspace runs at once. Each failing test's own diagnostic already clears
the build and the load:

```
why this wait may have run out:
  * the in-test cargo build had nothing to do (78.237192ms), so the
    binaries were ready long before this wait: this is the hub, the
    runner, or the machine — not the build.
  * host load average 2.87 / 3.20 / 2.33 (1, 5, 15 min) over 12 cores,
    read as this wait ran out
```

No test and no expectation was edited. The suite is reported red.

### The cause: two hypotheses tested and both refuted

This is a negative result and it is written down so that nobody spends a
day re-deriving it.

**Hypothesis 1 — memory throttling by the enclosing slice. REFUTED.**
An agent session here runs in
`/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`, which has
no limit of its own and is enclosed by `chatons.slice`, whose
`memory.high` is 18 432 MiB. The reading was that the kernel stalls
allocation in that slice — `memory.events high` stood at 24 289 — and
that this is what blows a 30 s wait while the CPU is idle. Measured
against the failing windows, it is not: `memory.events high` and both
`memory.pressure` totals were **flat on every second of both windows**,
`hi+0 mem_full+0 mem_some+0`, with the scope at ~7.5 GiB and the parent at
~13.7 GiB against that 18 432 MiB ceiling.

**Hypothesis 2 — IO stall on the shared nvme. REFUTED.** Twelve
`real_herd` tests at once, each with a real hub that `fdatasync`s event
lines, on one device shared with the consumer's production, and the
scope's lifetime `io.pressure full` total was five times its memory one.
Measured, it is not the cause either. The worst `io.pressure full` delta
anywhere near the windows is 45 206 µs at 14:35:50 — 4.5 % of one second,
and outside both windows. Across the thirty seconds `owner_control` spent
waiting the deltas are single-digit microseconds per second:

```
14:36:49 io_full+6     mem_full+0 mem_some+0 hi+0 scope=7574MB par=13732MB
14:36:53 io_full+0     mem_full+0 mem_some+0 hi+0 scope=7581MB par=13740MB
14:37:03 io_full+0     mem_full+0 mem_some+0 hi+0 scope=7570MB par=13728MB
14:37:13 io_full+1     mem_full+0 mem_some+0 hi+0 scope=7559MB par=13717MB
14:37:17 io_full+0     mem_full+0 mem_some+0 hi+0 scope=7559MB par=13717MB
```

**The 18 270 MiB `memory.peak` of the scope is NOT evidence** and must
not be quoted as such. It is a high-water mark since the counter was last
reset; the slice's live memory is page cache, not process memory
(`chatons.slice`: `file` 12.43 GB of `current` 13.42 GB, largest live
process in our scope 0.44 GB), and clean page cache reclaims for free. A
9.8 GB `CARGO_TARGET_DIR` and today's parallel cargo builds account for
the peak.

**So nothing was contended.** CPU low, memory unthrottled, device idle,
and three tests wait thirty seconds and give up. That is a product defect
that appears only at the suite's own parallelism — not an environment
problem, and not a property of where our agents live. It is a separate
task, with this measurement in hand; it is not fixed here and nothing was
changed to hide it.

Raw evidence kept in the scratchpad:
`…/diag/suite-full-timestamped.txt` (every line stamped with epoch and
clock, ending `TESTSH-EXIT:101` at 14:38:28), `…/diag/psi-full.tsv`
(1 Hz: both memory PSI totals, the IO PSI total, the parent's
`memory.events high`, both `memory.current`), `…/diag/cgroup-full.tsv`
(1 Hz memory and counters; **its four PSI columns are wrong** — they
parse field 4, `avg300=`, instead of field 5, `total=` — ignore them),
and `…/diag/readout.sh`, which prints the windows and the per-second
deltas.

## Not done, and why

- **`scripts/expected-skips/local.txt` does not exist.** `--bless`
  requires a run whose last line is `test suite: PASSED` and refused on
  every red run, correctly (D27, D179): a run that did not finish yields
  a floor — the suite itself printed `AT LEAST 12 assertion(s)` — and a
  floor is not an expectation. The brief's *done means* clause for this
  file cannot be met until the suite is green on this host. The three
  measured lists are in the scratchpad for whoever blesses it then.
- **`scripts/expected-skips/local-under-load.txt` does not exist** for
  the same reason. Its content is known from the code rather than
  measured: `herd-hub/tests/fake_hub_flood.rs:72` sets
  `const LOAD_CEILING: f64 = 8.0;` and at `:364` the bench skips itself
  through `herd_runner::testing::skipped`, with a comment that says
  *"This skip is conditional on the host's load, so it is not part of the
  suite's standing skip reference: on a quiet host it never appears."*
  The two labels are `flood-fifty` (`:587`) and `flood-two-fifty`
  (`:598`). Writing the file from the source rather than from a
  measurement would be inventing evidence, so it is left for the run that
  measures it.
- **H3's under-load half is not measured.** The two load generators are
  built and were never started: `…/scratchpad/method-local-gate/loadgen.sh`
  with source copies `loadsrc1/` and `loadsrc2/` (7.4 MB each), each
  meant to use its own `CARGO_TARGET_DIR`. No such build directory was
  ever created.
- **No green gate run on this branch**, and none is obtainable here while
  the suite is red.
- **The mitigation was not run and nothing for it was committed**:
  lowering the suite's parallelism is a contract-level change to the
  project's own test command and the product defect is another agent's
  task.
- **D-B is owed, not done**: step 0 and the receipt record load1 but not
  the enclosing slice's `memory.high`, its live `memory.current` or the
  IO stall clock. Accepted in principle, deliberately not added here.

## Gates

Run on this branch at `0a3994f`. Verbatim, each from its own file.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 172 passed, 0 failed
(exit 0)

$ nix develop --command cargo fmt --all --check
(no output, exit 0; step 4 of the gate, ok, 1s)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
(exit 0; step 5 of the gate, ok, 7-15s)

$ nix flake check --no-build
(exit 0; step 3 of the gate, ok, 2s)

$ nix develop --command ./scripts/test.sh
test result: FAILED. 17 passed; 1 failed   (-p herd-hub --test killed_hub)
test result: FAILED. 18 passed; 2 failed   (-p herd-hub --test owner_control)
error: 2 targets failed
==============================================================
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
==============================================================
test suite: FAILED (cargo test exited 101)
(exit 101)
```

The Rust gates **were run**; `./scripts/test.sh` is **red**, with the
three assertions named above, and it is reported red. `check-docs.sh` was
the last thing run, after this file was committed.

Skips: twelve, every one named above. Each is a capability skip reading
`cgroup v2 delegation is unavailable: this runner cannot create a cgroup
under /sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope
(Permission denied (os error 13))` or a consequence of it. The suite
labelled them a floor because the run did not finish; none is reported as
a pass.

## Risks and findings

1. **The method's premise is refuted on this host.** A green
   `./scripts/gate.sh` is unobtainable here today, so the new ORCH-001
   condition — a green gate on the exact sha — would authorise nothing.
   The gate itself behaved correctly throughout: it never printed a pass,
   it named the failing step, it refused to bless a red suite, and it
   wrote no receipt.
2. **The plan's `CLAUDE.md` sentence must not be copied as written.**
   Plan §2.2 says *"This workstation has cgroup v2 delegation, which is
   why it skips 12 at rest and not 14"*. False for an agent session: all
   twelve local skips say delegation is unavailable, so an agent here
   cannot check the confinement assertions behind invariant 2 any more
   than a GitHub runner can. The real difference is two CI-only entries,
   both named `the_io_weight_verdict_follows_the_device_the_job_works_on`
   (one *"no device offers bfq"*, one *"this host has no ZFS mount"*),
   which do not skip here because `/mnt/ssd_980` has blk-iocost enabled
   (`259:0 enable=1 ctrl=auto`).
3. **`local.txt` and `ci.txt` are two lists, not two orderings of one.**
   One name's text differs between the hosts: CI has
   `an_io_weight_that_cannot_bite_is_said_out_loud`, this host has
   `an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device
   branch)`. Anyone reconciling them into one file will be wrong.
4. **The `narrowed=yes` trap would give a false negative on the
   mitigation.** `scripts/test.sh:58` is
   `[ "$#" -gt 0 ] && narrowed=yes`, so **any** argument makes the run
   end in `test suite: PASSED (a part of the suite: narrowed by its
   arguments)`, which `report-skips.sh:38` and `gate.sh` step 6 both
   refuse — correctly — to read as a complete run. So
   `./scripts/test.sh -- --test-threads=4` can never be a green gate
   however green its tests are. The form that keeps the marker is
   `RUST_TEST_THREADS=4 nix develop --command ./scripts/test.sh`: zero
   arguments, `narrowed` stays `no`. Whoever measures the parallelism
   must use the environment variable.
5. **The gate measures a slice, not a host.** The in-test runner
   announced *"4.9 GiB rather than 31.1 GiB, because /proc/self/cgroup is
   enclosed by memory.high 18 GiB on /chatons.slice, which already holds
   13 GiB"*. Two agents on this workstation, in different slices or at
   different times of the consumer's memory use, do not run the same
   suite. This is D-B, and it is owed.
6. **The orchestrator's checkout is stale and it cost real time.**
   `/mnt/ssd_980/herd-project` was at `860c4f5`: its
   `check-orchestration.sh` was 185 lines against 516 on `origin/main`,
   its `check-docs.sh` 129 against 197, and its `test.sh` had neither the
   prebuild nor the `SKIPS UNKNOWN` / `AT LEAST` wording. Work read
   against it had to be redone. ORCH-031 as written.
7. **`scripts/bench-preflight.sh` does not exist** on `origin/main`;
   `scripts/` holds `check-docs.sh`, `check-orchestration.sh`,
   `image-smoke.sh`, `release-smoke.sh`, `report-skips.sh`, `test.sh` and
   now `gate.sh`. Briefs that tell an agent to run it first cannot be
   followed.
8. Nothing was written under `/sys/fs/cgroup`, and no cgroup, slice,
   systemd unit or anything under `/etc/nixos` was touched. The
   instrumentation reads those files and only reads them.

## Decisions needed

**D-A — a reproducibly red local suite against ORCH-001.** *Question:*
three assertions fail on every full-workspace run on this host and pass
at low concurrency, with nothing contended. Do we hold the method until
that is fixed, adopt the gate and let the merger record a rerun of the
failing target, or drop the pull request and keep CI as the required
check? *Recommendation:* ship `gate.sh` now as a tool, give the product
defect its own board row, and keep CI as the merge condition until the
suite is green here — which is ORCH-120's own fallback. A gate whose red
is routine becomes "rerun until green", and that is what the new
ORCH-001 must not rest on. *Fallback:* adopt it with the rerun recorded,
and accept that risk explicitly.

**D-B — the gate should record the slice it ran in.** *Question:* may a
later task add the enclosing slice's `memory.high` and live
`memory.current`, and the scope's `io.pressure` total, to step 0 and to
the receipt? *Recommendation:* yes, in a task of its own; accepted in
principle today and deliberately not added here. *Fallback:* receipt
only.

**D-C — bounded parallelism in `scripts/test.sh`.** *Question:* should
the project's own test command cap its test threads? *Recommendation:*
measure `RUST_TEST_THREADS` first (never arguments, finding 4); it is a
contract-level change to how the suite runs, so Alexa is told before it
lands. *Fallback:* leave it and fix the product defect instead.

Proposed decision entry, for `herd-merger` to number:

> ## 2026-10-28 — D-NEXT The local gate reads the skip list, never the count
>
> `scripts/gate.sh` runs every step of `ci.yml` in order and ends in one
> verdict naming the sha it ran on; only `gate: GREEN` authorises a push
> to `main`. It compares the run's skips against a committed list for the
> host and is red on any assertion name that is not on it, silent on how
> many there are. A count cannot carry D27: this workstation reported 12
> at rest on three runs of 2026-10-28, GitHub Actions 14 on green run
> 40564336703, and a red CI run printed "AT LEAST 15" because it had not
> finished. The lists live in `scripts/expected-skips/`, not under
> `docs/`, because `check-docs.sh` width-checks `docs/*.md` at 79 columns
> and a skip line is far longer. A list is produced only by `--bless`
> from a run ending `test suite: PASSED`; a run that did not finish
> yields a floor and never an expectation.

## Resume point

The product defect first: `a_result_the_hub_cannot_write_is_kept_until_it_can`,
`a_drain_first_reservation_survives_a_kill_nine_on_either_side` and
`a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it`, with
the measurement above in hand — nothing is contended, so the 30 s
`PATIENCE` against the suite's own parallelism is where to look. Then
`./scripts/gate.sh --bless` on the first green run to produce
`scripts/expected-skips/local.txt`, and H3's under-load half with
`…/scratchpad/method-local-gate/loadgen.sh`. T2 needs findings 2 and 3 of
this handoff before it writes any `CLAUDE.md` sentence about the local
gate; T5 needs `./scripts/gate.sh --verify-receipt <sha>`.

## Processes

Started and ended by this task, each killed by pid and never by name:
`1648726` (prewarm `cargo test --workspace --no-run`), `1921320` (gate
sample 1), `2247378` (gate sample 1b), `2362798` (the instrumented
suite), `2362797` and `2379264` (the two 1 Hz cgroup samplers, killed
once the suite had exited). No other process was touched; nothing of the
consumer's was signalled.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/method-local-gate` and is
deleted now that the branch is pushed (7.8 GB at its largest). No
`method-local-gate-load*` directory was ever created. The two 7.4 MB
source copies and `loadgen.sh` stay in the scratchpad for H3's
under-load half.
