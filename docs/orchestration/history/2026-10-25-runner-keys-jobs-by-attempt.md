# runner-keys-jobs-by-attempt — handoff 2026-10-25

- **Item:** ORCH-062 (part 1 of 3 of ORCH-060)
- **Agent:** herd-step
- **Branch / commit:** `runner-keys-jobs-by-attempt`, based on `origin/main`
  `199e4df`
- **PR:** opened from the template. **Not merged** — `herd-merger` merges.

The first of the three PRs Alexa asked for on 2026-10-25, splitting PR #82:
*"Trois PR dans cet ordre, chacune avec son test rouge sur la base
précédente : 1. carte des jobs indexée par (job, essai) + accusé qui ne
radie plus le vivant"*. Nothing of C5's ceiling is in it.

## Done

**The tests first, in their own commit (`ac3ec2c`), red on `199e4df`.**
Three of them in `herd-runner/src/lib.rs`, one per consequence that was
really observed:

| Test | What it asserts |
|---|---|
| `two_attempts_of_one_job_are_two_entries_in_the_machines_own_count` | A runner holding attempts 1 and 2 of one job counts two, and its health report names both under their own numbers |
| `handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running` | `abandon` of attempt 2 leaves attempt 1 running, named, and its cancel token unasked |
| `an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other` | Taking attempt 2 does not destroy attempt 1's unacknowledged verdict, and the acknowledgement frees that verdict without touching the attempt that is running |

**Then the fix.** `Inner::jobs` is `HashMap<InHand, JobInHand>` where
`InHand` is `{ job_id, attempt }`. `JobInHand::attempt` is gone — the key
carries it, and two places to read an attempt number from are two numbers
that can disagree. What followed from the key:

1. `send_chunk` and `finish` take the attempt they are about. It is threaded
   through `job.rs` — `say_in_the_log`, `reclaim_or_say`,
   `report_could_not_start`, `stream_output`, and the three `Ran` methods
   that write in the job's log — all of which already had `attempt` in
   scope, inside `execute`.
2. `abandon` removes that attempt's entry and no other.
3. `release` takes the entry it is letting go of, and the acknowledgement
   path is a function of its own, `result_acknowledged`: `JobResultAck`
   names no attempt, so the attempt it is about is the one **holding an
   end** — this function's own sentence read literally. A job with nothing
   in hand still has its spool swept, which is what frees a verdict a
   previous life of this runner wrote (D34).
4. `log_acknowledged`, `resend_log` and `drop_stale` look the entry up by
   both instead of finding it by id and then comparing the number.
5. `end_job` takes `attempt: u32` rather than `Option<u32>`: it was only
   ever called with `Some`, and the `Option` existed to support exactly the
   lookup the key now makes correct by construction.
6. `reclaim::LiveJobs` carries the attempt beside the job id, so the stop
   path's one log chunk — a workspace it could not reclaim — goes into the
   record of the attempt it belongs to.

**One `D-NEXT` entry** in `docs/decisions.md`, with the defect, the three
consequences, why the key rather than a guard, and what it does **not**
fix. **The board** carries ORCH-062 and ORCH-060 now says C5 is three PRs
in this order.

## Not done, and why

- **No guard in `take_job`.** Refusing a second attempt of a job this
  runner is already running is a decision about what a runner may accept,
  it is part of C5's refusal machinery, and it is in PR 3 by Alexa's
  ordering. This PR makes the books honest; it does not change what is
  taken.
- **A workspace still belongs to a job, not to an attempt.**
  `DataRoot::open_workspace` takes a `JobId` and clears the directory if it
  is already there, so two attempts of one job on one machine still wreck
  each other's working directory. Pre-existing, unchanged by this PR,
  written into the decision entry and into *Risks* below.
- **`Inner::collections` is still keyed by `JobId` alone.** Same shape as
  the defect fixed here, one map over. Not touched: it is not one of the
  three consequences Alexa named, and `FilesOut` carries its own attempt
  number, so what it can cost is a collection of one attempt replaced by
  another's rather than a job vanishing. Carried as a finding.
- **`docs/status.md` not touched** — it is the merger's, per the workflow.
- **No merge, no tag, no release, no `main`.**

## Gates

`export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-split`.
`./scripts/bench-preflight.sh --target-dir /mnt/ssd_980/herd/build/c5-split`
ran **ok** before each heavy phase (`343 GiB free`, `load 0.51` then `2.52`
on 12 cores, `14 GiB available`).

```text
$ nix develop --command cargo build --workspace
(clean, exit 0)

$ nix develop --command cargo fmt --all --check
(clean, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
(clean, exit 0)

$ nix develop --command ./scripts/test.sh
test suite: PASSED
passed=778 failed=0 ignored=20
 12 assertion(s) COULD NOT RUN on this host and were skipped.
```

### The red, on `199e4df`, before a line of the fix

```text
$ nix develop --command cargo test -p herd-runner --lib -- --test-threads 1 \
      two_attempts_of_one_job handing_one_attempt_back an_acknowledgement_lets_go

running 3 tests
test tests::an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other ... FAILED
test tests::handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running ... FAILED
test tests::two_attempts_of_one_job_are_two_entries_in_the_machines_own_count ... FAILED

---- an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other ----
herd-runner: j0000000403 ended while there was no hub to tell; its result is
held until a hub acknowledges it
panicked at herd-runner/src/lib.rs:3369:9:
assertion `left == right` failed: taking attempt 2 of j0000000403 destroyed
the unacknowledged verdict of attempt 1
  left: 0
 right: 1

---- handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running ----
panicked at herd-runner/src/lib.rs:3298:9:
assertion `left == right` failed: attempt 1 of j0000000402 is still running
on this machine and its own count says 0
  left: 0
 right: 1

---- two_attempts_of_one_job_are_two_entries_in_the_machines_own_count ----
panicked at herd-runner/src/lib.rs:3252:9:
assertion `left == right` failed: this machine is running two processes and
its own count says 1: a count that is short is a host that accepts work it
has no room for (invariant 2)
  left: 1
 right: 2

test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 160 filtered out
```

### The green, after it

```text
running 3 tests
test tests::an_acknowledgement_lets_go_of_the_attempt_that_holds_the_result_and_no_other ... ok
test tests::handing_one_attempt_back_leaves_the_other_attempt_of_that_job_running ... ok
test tests::two_attempts_of_one_job_are_two_entries_in_the_machines_own_count ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 160 filtered out
```

### The two expected reds

```text
$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8593:## 2026-10-25 — D-NEXT A runner's live jobs are keyed by job and attempt
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs check failed

$ ./scripts/check-orchestration.sh
FAIL  unmodified copy passes
    docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
    8593:## 2026-10-25 — D-NEXT A runner's live jobs are keyed by job and attempt
check-orchestration: 86 passed, 1 failed
```

Both are the one expected failure and the only one — the `D-NEXT`
placeholder, which only `herd-merger` may resolve
(`gotchas/a-decision-branch-cannot-be-all-green.md`). The D170 and D174
lines are notes about gaps held by open PRs, not failures. **Not fixed, by
instruction.**

### Skips — 12, the standing reference

10 for cgroup-v2 delegation and 2 for the ZFS IO ceiling, printed as skips
and counted as skips (D27). None of them is in a file this task touched, and
no new skip was introduced.

## Risks and findings

1. **The map is honest; the workspace is not.** `open_workspace` is per job
   and clears what it finds, so two attempts of one job on one machine still
   destroy each other's working directory. This PR neither causes that nor
   fixes it — on `199e4df` the same two `execute` calls do the same thing to
   the same directory. What it changes is that the machine now *knows* both
   are there. The refusal that stops it happening at all is PR 3's
   `StillRunningThisJob`.
2. **`result_acknowledged` may let go of more than one entry.** If two
   attempts of one job were both holding an end, one acknowledgement frees
   both. That state is not reachable today — a second attempt is only
   assigned after the hub has written the first off, and a written-off
   attempt's entry is dropped by `drop_stale` — and the alternative
   (freeing an arbitrary one of them) is worse. Named here rather than left
   to be discovered.
3. **`send_chunk` for an attempt that is not in hand is now silent in one
   more case.** It was already silent for a job that is not in hand; it is
   now also silent for the right job at the wrong attempt. That is the
   correct behaviour — a chunk of an attempt this runner is not running has
   nowhere to go — and it is what `log_acknowledged` and `resend_log`
   already did by comparing the number after the lookup.
4. **Two `#[allow(clippy::too_many_arguments)]` added**, on
   `Session::finish` (8) and `job::stream_output` (8), each with the reason
   beside it. Three of the same allow already exist in `job.rs`, so this is
   the crate's own precedent rather than a new liberty.
5. **The red half is a commit of its own (`ac3ec2c`) and is red.** That is
   deliberate: it makes the red reproducible by checkout rather than only
   by this handoff. CI runs on the PR head, which is green.

## Decisions needed

**None.** Alexa's decision of 2026-10-25 is the brief; this is part 1 of it.

## Resume point

`herd-merger` takes `runner-keys-jobs-by-attempt` first, allocates **one**
`D` number, and merges on green CI under ORCH-001. Parts 2
(`watchdog-measures-each-age`, ORCH-063) and 3
(`c5-announced-job-ceiling`, ORCH-064) are stacked on this branch in that
order and must be merged after it.

## Processes

None left running. Every `cargo` and `nix` invocation was a foreground
child of this session and exited. No `pkill`, no `killall`, nothing killed
by name. The build directory `/mnt/ssd_980/herd/build/c5-split` is shared by
the three parts of this task and is removed at the end of the last one.
