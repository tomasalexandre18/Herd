# orch-071-test-sh-honest-skips — handoff 2026-10-27

- **Item:** ORCH-071
- **Agent:** herd-step
- **Branch / commit:** `orch-071-test-sh-honest-skips` / `c67b5bb`, based on
  origin/main `96fc10b`
- **PR:** #88

## Done

`scripts/test.sh` and `scripts/report-skips.sh` no longer say that nothing
was skipped on a run that did not happen.

**The shape chosen.** Three measures, because one is not enough, and none of
them makes any run less red.

1. **`--no-fail-fast` is now the script's own default.** `cargo test` stops
   launching test binaries once one has failed, which is why the skip log was
   empty: `confinement`, the binary carrying the cgroup skips, never ran. The
   suite is a gate; all of its failures at once is the useful answer, and a
   skip list collected from a run that stopped early is worth nothing. The
   flag is not added when the caller passed it — `cargo test` refuses
   `--no-fail-fast` twice (`error: the argument '--no-fail-fast' cannot be
   used multiple times`), which is checked.
2. **The exit status, now consulted.** `--no-fail-fast` does not make the log
   provably complete: a compile error, a cancelled run, a binary killed
   mid-flight each truncate it. So on a non-zero status the report says what
   it knows and no more — `SKIPS UNKNOWN on this host` for an empty log,
   `AT LEAST n` for a partial one. An empty log on a run that did not finish
   means *unknown*, not *none*.
3. **The caller's own arguments.** `./scripts/test.sh <filter>` finishes
   green having run a fraction of the suite, so its empty log is reported as
   being about the tests this run selected and nothing else. Its verdict
   marker says so too, so a capture of it cannot be read back as a whole run.

**The CI half, which is where gates are judged.** `scripts/report-skips.sh`
had the same defect and it runs with `if: always()` (`.github/workflows/
ci.yml`), so it printed "Every assertion ran on this host: nothing was
skipped" into PR #86's *failing* `check` job, in the step log and in the job
summary. Fixing `test.sh` alone would have left the false sentence in the one
place people read it. It now decides on the marker `test.sh` prints last: a
plain `test suite: PASSED` means the whole suite finished and the list is
complete; `test suite: FAILED`, the marker's narrowed form, or no marker at
all (a cancelled job) means unknown, or a floor. It still never fails the
job: the suite's own step owns that verdict.

**Nothing was weakened.** The exit code is still cargo's own; the two real
failures are still failures; no skip became a pass; the unqualified green
wording — the sentence `deploy/README.md` quotes — is byte-for-byte what it
was, and is now reachable only from a complete, green, unnarrowed run.

**Both tests shipped**, 18 cases in `scripts/check-orchestration.sh`, in the
pattern PR #87 used: a fake `cargo` on `PATH` records its arguments, writes
the skip lines it was given to `$HERD_SKIP_LOG` and exits with the status it
was given, so every combination of (the run finished, something was skipped)
is reproducible in a second with no toolchain — the red one above all. Each
case asserts what the report must say *and* what it must not.

## Not done, and why

- **`docs/orchestration/board.md` not edited**, as the brief required
  (another integration may be writing it). The row is below, verbatim.
- **No entry in `docs/decisions.md`.** The brief did not ask for one and the
  merger allocates numbers. `--no-fail-fast` as the suite's default is
  arguably worth a decision entry; a `D-NEXT` candidate is in *Decisions
  needed* for whoever wants it.
- **No unqualified green whole-suite run on this host.** The suite is red
  here for ORCH-069, so that path is proved by the fake-`cargo` cases, not by
  a real run. On a host where the suite is green it prints exactly the old
  sentence.
- **ORCH-069 untouched.** `herd-hub/tests/admission.rs:419` and
  `herd-hub/tests/slots.rs:744` are red and were left red; no test, no
  expectation and no assertion was edited.

## Gates

Run on this workstation (the ZFS/no-delegation host; CI's runner reports a
different number — 15 when green — which is ORCH-070's point).

### 1. nix develop --command ./scripts/test.sh — RED, for ORCH-069

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`

==============================================================
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
==============================================================
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device: ... (ZFS, io.max)
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says: ... (ZFS, io.max)
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job (the named memory outcome): ...
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device branch): ...
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced: ...
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max: cgroup v2 delegation is unavailable ...
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces (the cgroup branch of the report): ...
==============================================================
Run them inside a delegated cgroup scope to make them real:
  cargo test --workspace --no-run
  systemd-run --user --scope -p Delegate=yes \
    target/debug/deps/confinement-<hash> --nocapture
==============================================================
test suite: FAILED (cargo test exited 101)
EXIT=101
```

Before this branch the same red run printed, in place of all of that:
`Every assertion ran on this host: nothing was skipped.`

The two failures, red and unedited — **ORCH-069**, which reproduces on a
pristine `main`:

```text
test result: FAILED. 4 passed; 1 failed; ... (-p herd-hub --test admission)
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' panicked at
  herd-hub/tests/real_herd/mod.rs:789:13: timed out waiting until the job is running.
  [the run before this one failed the same test at herd-hub/tests/admission.rs:419:5
   instead: "the inherited job's 4 GiB of declared memory must still be held"]

test result: FAILED. 7 passed; 1 failed; ... (-p herd-hub --test slots)
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at
  herd-hub/tests/slots.rs:744:9: left: "built with -j3", right: "built with -j7"
```

### 2. nix develop --command ./scripts/test.sh <name filter> — GREEN

```text
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out

No assertion was skipped on this host by the tests this run
selected — but it was narrowed by its own arguments
(the_binary_refuses_a_dirty_tree_naming_the_file), so it
says nothing about the rest of the suite.
test suite: PASSED (a part of the suite: narrowed by its arguments)
EXIT=0
```

A green, complete, unnarrowed run still prints the old sentence verbatim;
this host cannot produce one while ORCH-069 stands, so that path is proved by
the fake-`cargo` cases in `check-orchestration.sh` ("green with an empty skip
log says nothing was skipped", "green with skips reports the real count").

### 3. scripts/report-skips.sh on both captures, as CI's `if: always()` step

```text
$ ./scripts/report-skips.sh <the red capture>
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were skipped.
 This output is not a complete run of the suite, so the list below is a floor and not a count.
rc=0

$ ./scripts/report-skips.sh <the narrowed green capture>
 SKIPS UNKNOWN on this host: this output is not a complete run of
 the suite — no plain `test suite: PASSED` from scripts/test.sh —
 and no HERD-SKIPPED line reached it either.
rc=0
```

Before this branch both printed `Every assertion ran on this host: nothing
was skipped.`, which is what PR #86's failing `check` job printed.

### 4. cargo clippy --workspace --all-targets -- -D warnings

(`nix develop --command` in front of it, as every Rust command here is.)

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.61s
clippy EXIT=0
```

### 5. cargo fmt --all --check

```text
fmt EXIT=0
```

### 6. ./scripts/check-docs.sh

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check-docs EXIT=0
```

### 7. ./scripts/check-orchestration.sh

```text
== the skip report never claims more than it saw (ORCH-071, D27)
ok    test.sh: red with an empty skip log reports it unknown
ok    test.sh: red with skips reports them as a floor
ok    test.sh: the floor lists the skips it has
ok    test.sh: green with an empty skip log says nothing was skipped
ok    test.sh: green with skips reports the real count
ok    test.sh: a finished run prints the marker report-skips.sh reads
ok    test.sh: an unfinished run prints the failed marker
ok    test.sh: a narrowed green run does not speak for the whole suite
ok    test.sh: a narrowed green run says its count may be short
ok    test.sh: a narrowed run's marker is not a plain PASSED
ok    test.sh: --no-fail-fast is passed by default
ok    test.sh: the caller's own --no-fail-fast is not doubled
ok    report-skips.sh: a red run with no skip line reports it unknown
ok    report-skips.sh: a red run with skips reports them as a floor
ok    report-skips.sh: a finished run with no skips says nothing was skipped
ok    report-skips.sh: a finished run reports the real count
ok    report-skips.sh: a narrowed run is not read as a whole one
ok    report-skips.sh: an output with no verdict marker is not a pass

check-orchestration: 111 passed, 0 failed
EXIT=0
```

### 8. The same 18 cases against the scripts as they are at 96fc10b

A check that cannot fail is not a check, so the two scripts were reverted to
`origin/main` and the new cases run against them:

```text
FAIL  test.sh: red with an empty skip log reports it unknown
FAIL  test.sh: red with skips reports them as a floor
FAIL  test.sh: a narrowed green run does not speak for the whole suite
FAIL  test.sh: a narrowed green run says its count may be short
FAIL  test.sh: a narrowed run must not print the whole-suite marker
FAIL  test.sh: --no-fail-fast missing
FAIL  report-skips.sh: a red run with no skip line reports it unknown
FAIL  report-skips.sh: a red run with skips reports them as a floor
FAIL  report-skips.sh: a narrowed run is not read as a whole one
FAIL  report-skips.sh: an output with no verdict marker is not a pass
check-orchestration: 101 passed, 10 failed
EXIT=1
```

The first of those prints, in full, the sentence this task is about:

```text
    missing from the report: SKIPS UNKNOWN on this host
    said what it must not say: nothing was skipped

    Every assertion ran on this host: nothing was skipped.
    test suite: FAILED (cargo test exited 101)
```

**Skips, named with their host:** 12 on this workstation, a floor and not a
count on that red run — 10 for the absence of cgroup v2 delegation, 2 for a
data root on ZFS, which has no block device for `io.max`. CI's runner
reports 15 when green; the two numbers are different hosts, not a
disagreement.


## Board row for the merger

`docs/orchestration/board.md` was deliberately not edited. This is the
ORCH-071 row as it should read, to replace the `open` one in *Owed*; flip
the state to `done` on merge.

| ORCH-071 | `./scripts/test.sh` printed **"Every assertion ran on this host: nothing was skipped"** on a **red** run in which most of the suite never started: `cargo test` stops after the binary that fails, so `confinement` — the binary carrying the cgroup skips — never ran and `$HERD_SKIP_LOG` stayed empty. `scripts/report-skips.sh`, which CI runs with `if: always()`, had the same defect and printed the same sentence into PR #86's failing `check` job and its summary. A skip log empty because nothing ran, reported as nothing to skip, is false reassurance in exactly the direction D27 exists to prevent | finding of PR #87, 2026-10-26 | herd-step (author), herd-merger (integration) | active | Fixed on `orch-071-test-sh-honest-skips`, PR #88: both scripts now consult whether the run was a complete one — `SKIPS UNKNOWN on this host` for an empty log, `AT LEAST n` for a partial one, the unqualified green sentence byte-for-byte unchanged and reachable only from a complete green run; `--no-fail-fast` is now the script's own default so a failing binary no longer truncates the log; a run narrowed by its own arguments no longer speaks for the whole suite. Eighteen cases in `check-orchestration.sh`, ten of which go red against 96fc10b. Nothing left but the merge; flip to `done` then | — | PR #88 Evidence: the red run now prints `AT LEAST 12` (this workstation; CI's runner reports 15 when green) with exit 101 and ORCH-069's two targets still red and unedited; `check-orchestration: 111 passed, 0 failed`; `history/2026-10-27-orch-071-test-sh-honest-skips.md` |

## Risks and findings

- **`report-skips.sh` reads a text marker**, not an exit status, because CI
  hands it a captured file and nothing else. If someone changes the wording
  of `test.sh`'s last line, `report-skips.sh` degrades to "unknown" — the
  safe direction, never to a false pass — and two cases in
  `check-orchestration.sh` pin the two spellings so the pair cannot drift
  silently.
- **`--no-fail-fast` makes a red run longer**, because every binary runs. On
  this workstation the red run takes about three minutes rather than
  stopping at the first failing target. In exchange CI's red runs now name
  every failing target at once (`error: 2 targets failed`) instead of one.
- **A narrowed run's qualifying sentence echoes the caller's arguments**, so
  a very long filter makes a long line. Harmless, but it is there.
- **The brief's premise held in full**, with one correction worth recording:
  the defect is in *both* scripts, not only `scripts/test.sh`. The brief
  named CI's symptom (PR #86) without naming `report-skips.sh` as its
  source; it is the source, and it is fixed here. Nothing else in the brief
  was refuted by the code.
- `-p <crate>` does **not** narrow a run of this script: `test.sh` passes
  `--workspace`, and `cargo test --workspace -p herd-proto` still runs the
  whole workspace. A test-name filter does narrow it. That is why the green
  evidence below uses a name filter.

## Decisions needed

- **D-NEXT candidate, optional**: *"The suite runs with `--no-fail-fast`, and
  an incomplete run reports its skips as unknown."* Recommendation: record
  it, because it changes what a gate's output means and D27 is the invariant
  it serves. Fallback: leave it as the comment block in `scripts/test.sh`,
  which states the reasoning and the evidence. The merger allocates the
  number if it is taken.

## Resume point

`herd-merger`: review and merge the PR (CI `check` will be red for ORCH-069
on `main`'s own code, unchanged by this branch — the two failing targets are
named in Evidence and reproduce on a pristine `main`), flip the ORCH-071 row
to `done` with the PR number, and consider the `D-NEXT` above.

## Processes

Three foreground/background `nix develop --command` runs of my own, each
awaited and each exited on its own: the suite twice and `cargo clippy` once,
all with `CARGO_TARGET_DIR` inside my scratchpad. No PID was killed, no
`pkill`, nothing of anyone else's was touched.
