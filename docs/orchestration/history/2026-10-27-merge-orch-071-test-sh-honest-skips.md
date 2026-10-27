# merge-orch-071-test-sh-honest-skips — handoff 2026-10-27

- **Item:** ORCH-071 (integration), plus the ORCH-069 correction and the
  `docs/decisions.md` entry the branch was told to leave to the merger
- **Agent:** herd-merger
- **Branch / commit:** `orch-071-test-sh-honest-skips`, author's tip
  `b7b4dea`, integrated as `e9d0ae4` (one docs commit of mine on top) plus
  this file, based on `origin/main` `96fc10b`
- **Worktree:** `/mnt/ssd_980/herd/claude/tmp/claude-1002/`
  `-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/`
  `scratchpad/merge-88/wt` (mine alone, created detached at `b7b4dea`)
- **PR:** #88

## Done

### No merge was needed, and that is a verified fact, not an assumption

`origin/main` was `96fc10b` when this task began and `96fc10b` when it
ended; the branch was cut from it and nothing landed under it.

```text
$ git merge-base HEAD origin/main
96fc10bfb54a7cfc22f7f2be63bbdaddbe4bd4f0
$ git merge-base --is-ancestor origin/main HEAD && echo YES
YES - fast-forward possible, no merge needed
$ git log --oneline HEAD..origin/main
(empty: main has nothing the branch lacks)
$ git log --oneline origin/main..HEAD
b7b4dea Record CI's green run and that ORCH-069 is host-dependent
4746708 Name the branch tip rather than a stale sha in the handoff
d2929c8 Record PR #88 and the ORCH-071 board row in the handoff
c67b5bb Wrap the handoff's headings inside 79 columns
9dba2bf Add the ORCH-071 handoff
74a9680 Say in the marker when a run was only a part of the suite
88c96d2 Check the skip reports on red, green and narrowed runs
77004a5 Make CI's skip report honest on a red suite
d9eb041 Make scripts/test.sh honest about skips it did not collect
```

**Conflict log: empty.** No rebase, no merge commit of `main` into the
branch, no conflicted file, no resolution — so nothing of either side could
be lost by one. The branch's four files and my three are disjoint sets:

```text
author  scripts/test.sh  scripts/report-skips.sh  scripts/check-orchestration.sh
        docs/orchestration/history/2026-10-27-orch-071-test-sh-honest-skips.md
merger  docs/decisions.md  docs/orchestration/board.md  docs/status.md
        docs/orchestration/history/2026-10-27-merge-orch-071-...-skips.md
```

**How nothing was lost, independently of the diff.** The branch's own
control was re-run here against `origin/main`'s two scripts rather than
taken on trust: the two files were checked out from `96fc10b`, the suite of
checks run, and the ten expected failures appeared; the files were then
restored and `git status` came back clean. See gate 6.

### `docs/decisions.md`: D179 allocated

The next free number above **D178**, which is the highest on `origin/main`
`96fc10b` and the highest on every open PR — #67 (`status-after-036`, which
holds D170), #85 (`c5-announced-job-ceiling`) and #88 itself; none of the
three carries a `D179` or above, checked by grep over each branch's file.
`D143`, `D154` and `D170` stay gaps (D148). There was no `D-NEXT`
placeholder anywhere in the tree to replace — `grep -rn D-NEXT` over the
branch finds only prose about the mechanism — so D179 is a new entry,
appended at the end and nothing renumbered.

**D179 — Every binary runs; an incomplete run's skips are unknown.** Its
subject, in the author's words and the orchestrator's: the suite runs with
`--no-fail-fast`, and an incomplete run reports its skips as unknown. The
entry says why it belongs in this file at all — it changes what a gate's
*output means*, and **D27** is the invariant it serves: a skip log empty
because nothing ran must never read as nothing to skip. It names the
failure prevented (the green sentence printed on a red run in which
`confinement`, the binary carrying the twelve skips, never started, and the
same sentence published by `report-skips.sh` into PR #86's failing `check`
job and its summary) and the alternative rejected (leave the reasoning in
the comment block in `scripts/test.sh` and change no output — rejected
because a comment binds nobody and is not where anyone reads a verdict).

### `docs/orchestration/board.md`

- **ORCH-071 → `done`.** The row is the one the author wrote under "Board
  row for the merger", verbatim, with two changes: the state is `done`
  rather than `active`, as instructed, and the cell that said "Nothing left
  but the merge; flip to `done` then" now records the integration instead —
  no merge needed, D179 allocated, the gates run, merged under ORCH-001 on
  green CI. Leaving the old sentence beside a `done` state would have made
  the row contradict itself.
- **ORCH-069 corrected.** The row said the two assertions are "red on this
  workstation and red on `origin/main` alone"; it now also says they are
  **green on CI**, that ORCH-069 is therefore a property of **this host plus
  the code** and not of the code alone, and that the account relayed in two
  briefs — that they reproduce on a pristine `main` — holds **on this
  workstation only**. The "deterministic … 3 of 3" clause is replaced: the
  failure mode is not stable even here, the admission test having failed at
  `admission.rs:419` in one local run and as a timeout at
  `herd-hub/tests/real_herd/mod.rs:789` in another. Every figure already in
  the row is kept, and the question it names first is kept first: *why does
  a runner told `--host-memory 8GiB` announce less, and less each time?*
  One fourth round of figures is added, from my own gate run — see
  "Risks and findings", because it bears on that question. **No test was
  touched.**
- **Next, items 6 and 7** rewritten for the same two facts: ORCH-069 is this
  host, and ORCH-071 is done with D179. Item 7 keeps the standing rule — a
  skip count is quoted with its host, and a gate is judged on CI's number —
  and notes that ORCH-048, the over-counting in the same two scripts, is
  untouched by this change.

### `docs/status.md`

Still **90 lines**, the limit, and still state rather than journal. Three
facts changed: a skip count is now given as a per-host figure (12 here, 15
on CI, with what each of the twelve is), the suite is said to be red here
and green on CI with ORCH-069 named as a property of this host, and D179 is
named for what a gate's output now means. The commit count past v0.3.7 and
the read date are refreshed; the workspace version still reads `0.3.7` and
no version moves.

### The task's own handoff

`docs/orchestration/history/2026-10-27-orch-071-test-sh-honest-skips.md`
exists on the branch, 334 lines, and is what the board row and D179 cite.

## Not done, and why

- **No tag, no release, no issue, no branch deleted.** Not a merger's, and
  none was asked for.
- **ORCH-069 is not fixed and no test, expectation or assertion was
  edited.** The red suite is this change's fixture. Its row is corrected,
  which is a prose change.
- **PR #85, PR #67, ORCH-068 and ORCH-074's `plan-orch-074` branch were not
  touched.** #85 is the next integration and the one with a real conflict;
  #67 and ORCH-068 are Alexa's; `plan-orch-074` holds another agent's
  uncommitted files and is not mine to commit or merge.
- **ORCH-048 not folded in.** It is the over-counting of the same two
  scripts and it is its own row and its own task, as that row says.
- **Nothing on the consumer's side** (D161), nothing in the orchestrator's
  checkout, and no `.claude/` or `/etc` path written.

## Gates

Run in this worktree, on the integrated tree, with `CARGO_TARGET_DIR`
inside this task's own scratchpad. The diff touches `scripts/`, so all four
Rust gates are owed and none is skipped. **The host is this workstation**
— ZFS data root, no cgroup v2 delegation — and **every skip count below is
this host's: 12. CI's runner reports 15 on the same tree.** The two are
different hosts, not a disagreement (ORCH-070).

### 1. `nix develop --command cargo fmt --all --check`

```text
fmt EXIT=0
```

### 2. `nix develop --command cargo build --workspace`

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.41s
build EXIT=0
```

### 3. `cargo clippy --workspace --all-targets -- -D warnings`

(`nix develop --command` in front of it, as every Rust command here is.)

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.10s
clippy EXIT=0
```

### 4. `nix develop --command ./scripts/test.sh` — **RED**, for ORCH-069

Red is reported as red. The two failing targets are `main`'s own, unedited
by this branch, and the run is **longer than it used to be by design**:
`--no-fail-fast` means every binary runs, so both failing targets are named
at once instead of the first one only.

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
test EXIT=101
```

**Skips, named with their host:** **12 on this workstation**, and on this
run a *floor* and not a count because the run did not finish — 10 because
cgroup v2 delegation is unavailable in this session's scope, 2 because the
data root is on ZFS and no hard `io.max` can be carried. **CI's runner
reports 15 when green.** The script saying "AT LEAST … and not a count"
here, in place of the old unqualified sentence, is the change under test.

The two failures, verbatim and unedited:

```text
test result: FAILED. 4 passed; 1 failed; ... (-p herd-hub --test admission)
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' (9963)
  panicked at herd-hub/tests/admission.rs:419:5:
  the inherited job's 4 GiB of declared memory must still be held: ... on
  lender, 2.3 GiB free of its 6.3 GiB memory admission, 5 GiB needed ...

test result: FAILED. 7 passed; 1 failed; ... (-p herd-hub --test slots)
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' (24594)
  panicked at herd-hub/tests/slots.rs:744:9:
  left: "built with -j3", right: "built with -j7"
```

### 5. `./scripts/check-docs.sh`

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check-docs EXIT=0
```

The one note is expected and correct: D170 is held by the unmerged branch of
PR #67 and gaps stay (D148). Run after every edit of mine, including the
last.

### 6. `./scripts/check-orchestration.sh`

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

**And the control, re-run here rather than trusted.** `scripts/test.sh` and
`scripts/report-skips.sh` were checked out from `origin/main` `96fc10b` and
the same checks run against them:

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

Ten of the eighteen new cases go red against `main`, reproduced
independently of the author. The two files were then restored from `HEAD`
and `git status --short` came back empty.

### 7. The green sentence, byte for byte

```text
$ git show origin/main:scripts/test.sh        | grep -n 'nothing was skipped'
37:  echo "Every assertion ran on this host: nothing was skipped."
$ git show origin/main:scripts/report-skips.sh | grep -n 'nothing was skipped'
30:  echo "Every assertion ran on this host: nothing was skipped."
$ git show HEAD:scripts/test.sh               | grep -n 'nothing was skipped'
101:    echo "Every assertion ran on this host: nothing was skipped."
$ git show HEAD:scripts/report-skips.sh       | grep -n 'nothing was skipped'
59:  echo "Every assertion ran on this host: nothing was skipped."
```

The string is identical on both sides; only its indentation and its place in
the file moved, because it is now inside the complete-and-green branch.
`deploy/README.md:542` quotes its opening and needs no change.

### CI

`check` **pass** 7m50s and `docs` **pass** 3m31s on the author's head
`b7b4dea`, run **40421949808** — green, both of ORCH-069's targets
included, which is the evidence that those two failures are this host's and
not the code's. CI is re-run on the head this handoff is part of, and the
merge waited for it; the merge happened only on a green `check` and a green
`docs`, under ORCH-001.

## Risks and findings

1. **ORCH-069's two assertions do not fail for the same reason in every
   round, and this run splits them apart.** The admission test failed here
   at `admission.rs:419` with a *fourth* distinct figure — "2.3 GiB free of
   its 6.3 GiB memory admission" — after 2.8/6.8, 2.5/6.5 and 1.1/5.1 in
   earlier rounds. But in that same run `slots.rs` printed `8 GiB lent, 1.6
   GiB kept as its own floor` and `-j 3 from 8288 MiB and 20 threads, 6216
   MiB for this slot (3 of 4), 1792 MiB per build thread`: **there the
   declared `--host-memory 8GiB` was honoured in full**, and the `-j3`
   against E-22's `-j7` came out of the per-thread arithmetic, not out of a
   lower announcement. So "the runner announces less, and less each time" is
   the admission test's story and not, in this round, the slots test's. The
   row now carries that; whoever takes ORCH-069 should not assume one cause
   for both.
2. **The host-dependency claim is now evidenced from three directions:**
   CI's `check` green on the branch (run 40421949808), the admission test
   failing at two different assertions across two local runs, and the slots
   test failing for a different reason in this run than the one its row
   describes. A local red suite here is not a verdict on a branch; CI is.
3. **A red suite is now slower, by design.** Every binary runs, so this
   host's red run takes minutes rather than stopping at the first failing
   target. The gain is that both failing targets are named at once, which is
   what made finding 1 visible at all. Anyone timing something on this host
   should know a red suite run is no longer a short one.
4. **`report-skips.sh` keys on a text marker**, not an exit status, because
   CI hands it a captured file. Rewording `test.sh`'s last line degrades it
   to "unknown" — never to a false pass — and two of the eighteen cases pin
   both spellings so the pair cannot drift silently. That is a real coupling
   between two files, written down here because no check can state it.
5. **ORCH-048 remains open on these very two scripts** (both dedupe on the
   whole line, and two of the twelve reasons embed a PID-bearing path, so
   one assertion can read as two). Nothing here fixed it and nothing here
   made it worse; a count printed as `AT LEAST n` is now explicitly a floor,
   which is the safe direction for it.
6. **Nothing in the brief was contradicted** except what the brief itself
   flagged: `origin/main` had *not* moved again, so no merge was needed, and
   the integration is a fast-forward plus my own docs commit.

## Decisions needed

None. D179 was the one allocation this integration owed and it is written.
ORCH-069 stays `open` with `herd-planner` as its owner and its question
unchanged; ORCH-048, ORCH-070 and ORCH-072 stay `open` and untouched.

## Resume point

**ORCH-077**, the timing measurement of the three `fsync`s, and the host is
quiet for it: no `cargo`, no `nix` and no background job of mine is
running. My last build finished at **2026-10-27 01:01:44 +01:00**
(`scripts/test.sh`, the last of the four Rust gates).

The next integration is **PR #85** (`c5-announced-job-ceiling`, ORCH-064),
which is based on `watchdog-measures-each-age` rather than on `main`, is
`CONFLICTING`, and carries the two board rows ORCH-073 is waiting for.
`plan-orch-074` holds another agent's uncommitted prose and is not an
integration.

## Processes

One background shell of my own, which ran the four Rust gates in sequence
and exited on its own; `cargo` ran exactly once at a time inside it, under
`nix develop`, with `CARGO_TARGET_DIR` in this task's scratchpad. Two short
foreground runs of `check-docs.sh` and `check-orchestration.sh`, no
toolchain. **No PID was killed, no `pkill`, nothing of another agent's
touched, nothing written outside this worktree and this task's scratchpad.**
