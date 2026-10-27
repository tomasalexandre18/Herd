# merge-85-c5-announced-job-ceiling — handoff 2026-10-27

- **Item:** ORCH-064 (C5 part 3), closing ORCH-060 and ORCH-073; plus
  Alexa's three answers of 2026-10-27 recorded on the board
- **Agent:** herd-merger
- **Branch / commit:** `merge-85-c5-announced-job-ceiling`, pushed to
  `c5-announced-job-ceiling` / based on `origin/main` `cd59ef5`, PR head
  before this pass `561c574`
- **PR:** #85
- **Worktree:** the fenced path below; nothing outside it was written.

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merge-85/wt
```

## Done

**A merge, not a rebase, and the reason is in the branch.** The branch
carries thirteen commits, one of them a merge of
`watchdog-measures-each-age`; `git merge origin/main` is what the three
previous C5 integrations did and it keeps that history readable. PR #85
was also still based on `watchdog-measures-each-age`, a branch already
merged into `main` through PR #84, so its base was retargeted to `main`.

**Three conflicts, all in `docs/`. Every Rust file merged by itself.**
`herd-hub/src/server.rs` and `herd-runner/src/lib.rs` were touched by
both sides and `ort` resolved them without a marker.

### 1. `docs/decisions.md` — both sides appended at the tail

- **Ours:** three new entries headed `D-NEXT`, after D178.
- **Theirs:** D174 (from PR #72) and D179 (from PR #88), after D178,
  plus a changed closing paragraph of **D178** itself — `main` had
  allocated that number, the branch still read `D-NEXT` there.
- **Kept:** everything from both sides, main's first and byte for byte.
  Verified rather than asserted: `diff` of the resolved file's first
  8 917 lines against `origin/main:docs/decisions.md` is **empty**, so
  no entry `main` holds was touched, and the three new entries differ
  from `561c574`'s only in the number allocations below.
- **Numbers:** D179 is the highest on `origin/main` `cd59ef5`; the only
  other open PR, #67, holds D170 and no other. So **D180**, **D181**,
  **D182**, in the order the entries appear, which is the order the
  branch's own handoff asked for. Appended at the end; nothing
  renumbered. D143, D154 and D170 stay gaps (D148).

### 2. `docs/remediation/README.md` — two sides, two index rows

- **Ours:** the row for `09-runner-self-limits.md`.
- **Theirs:** the rows for `10-ordering-on-ext4.md` and
  `11-sync-prices.md`, added at PR #91.
- **Kept:** all three, in number order. The note below the table said
  *"`09` is the one number absent from this table"*, which the merge
  makes false, so it is rewritten to say every number written now has a
  row and to leave the reuse question where it was — **ORCH-091**.

### 3. `docs/orchestration/board.md` — three hunks

- **Open:** ours had `ORCH-022` saying **two** known-unstable tests and
  added `ORCH-061`; theirs had `ORCH-022` saying **three** and eleven
  rows the branch never saw. **Kept main's `ORCH-022`** — it is the
  later reading of the same fact — **and all of main's rows**, and
  carried `ORCH-061` over, since that is what `ORCH-073` asked for.
- **Active:** ours had `ORCH-062`, `ORCH-063`, `ORCH-064`; theirs had
  the first two, rewritten to `done` at their own integrations. Kept
  main's two and carried `ORCH-064` over.
- **Next:** main's list already supersedes the branch's, which still
  described C5 as unmerged and PR #72 as open. Main's was taken and
  then rewritten for today (below).

**`ORCH-073` is closed by this merge and closed the way it asked.**
Neither row was written by hand and neither is duplicated: `ORCH-064`
and `ORCH-061` came over from the branch at the merge. `ORCH-061`'s
state is `owed`, so it was moved into `## Owed` rather than left in
`## Open`, and its evidence now names **D182** instead of *"the third
`D-NEXT`"*.

**Sixty-five `D-NEXT` references outside `docs/decisions.md`** were left
by the branch for the integrator (`history/2026-10-25-c5-split-whole.md`
"Resume point"). Each was read in context and placed on its meaning:

- **D180** — the runner's own ceiling, `take_job`'s refusal, the
  `outputs/` reclaim.
- **D181** — `jobs_at_once` on `Hello` and the hub's `admission`
  honouring it; every `jobs_at_once: None` stand-in in the tests.
- **D182** — `at_its_ceiling` on `AttemptAbandoned` and on the attempt
  record, `goes_a_runner_took`, the remembered count,
  `CEILING_REFUSALS_ALLOWED`, `JobOutcome::RefusedAsFull` and its 125.
- **Two are both:** `herd-runner/src/lib.rs:1242` names the ceiling and
  the one-job-once rule together (D180, D182), and
  `announced_job_ceiling.rs:110` asserts the version numbers for D181
  and D182 alike.
- **One was neither:** the map keyed by job *and* attempt is C5 part 1,
  already **D177** on `main`.

`grep -rn D-NEXT` over `herd-proto`, `herd-hub`, `herd-runner`,
`herdctl` and `docs/remediation` is now empty but for
`00-baseline.md:186`, which describes the mechanism and is not a
placeholder.

### Alexa's three answers of 2026-10-27, recorded

Verbatim: *"1) oui 2) oui 3) oui obsolete"*, to the three questions in
that order.

1. **ORCH-075 — yes**, and written up as **D183**, *The keeper is the
   target, its tier taken in thirds*. The entry states the scope
   plainly, including what it does **not** authorise: step **A** alone
   (ORCH-083, drop the event `fdatasync`, the record keeping both its
   syncs, ~31 %); **B** (ORCH-084) not adopted and behind ORCH-080's
   crash-point enumeration; **C** (ORCH-085) not adopted, deferred, and
   possibly never taken; and nothing dropped at all before ORCH-079 and
   ORCH-081 are done. Row `done`.
2. **ORCH-068 — yes**: the live launcher
   `/mnt/ssd_980/herd/claude/env.sh` is authoritative, because it is
   what actually starts sessions and the only thing that sets
   `GH_CONFIG_DIR=/home/server/.config/gh`. Row moved from `## Blocked`
   to `## Owed`, owner `herd-step`, with the work described and bounded
   to the repository. **No work was done on it here and nothing under
   `/mnt/ssd_980/herd/claude/` was written**; `.credentials.json` was
   never opened.
3. **PR #67 — obsolete**, closed unmerged (see below).
4. **ORCH-076 — answered, and by measurement rather than by Alexa's
   word.** Added mid-task on the coordinator's instruction; it changes
   nothing about the integration. The hub volume
   (`/mnt/ssd_980/docker/volumes/herd_herd-state`) and the runner data
   root (`/mnt/ssd_980/herd-runner`) are both on `/mnt/ssd_980`, **ext4
   on NVMe**, 983 GB / 589 used / 344 free, reported from the
   consumer's end on 2026-10-27 — a report, not a look (invariant 9).
   `df` on this workstation minutes later: `/dev/nvme0n1p1`, ext4,
   983 GB / 592 / 341. Same device, same figures within the interval.
   The row is `done` and moved out of `## Waiting` into `## Open`,
   because it was never Alexa's to answer in the end; the row says in
   so many words that it was settled by measurement, so a later reader
   knows a contrary measurement may reopen it. It **confirms ORCH-053
   from the other direction** — the state root is ext4, not ZFS, the
   premise `10-ordering-on-ext4.md` rests on — and ORCH-053 is **not**
   reopened. The consequence recorded in `## Next` and in
   `docs/status.md` is a constraint and not a reassurance: that
   filesystem is **shared** between their hub volume and runner data
   root, chatOnS's caches and this project's agent build trees, so how
   many heavy agents run here is now bounded by disk as well as by CPU.

**`ORCH-079` is `active`** and moved into `## Active`; ORCH-080,
ORCH-081 and ORCH-082 are untouched behind it.

**`## Next` is rewritten** around Alexa's framing, so that a next
session reads it there and not in a new document: *finish the
remediation first, then the features that were expected*; the first
consumer is updating Herd and resuming development, so the near-term
need is **support more than new features**.

**`docs/status.md`** is restated, still **90 lines**, the limit. The
commit count is re-anchored (137 past `v0.3.7` on this head, `cd59ef5`
being 121); the dossier is thirteen pieces, not twelve; `07` is no
longer "recommended and not adopted" but adopted as the direction by
D183; the defects in no release are seven, not four; the sentence that
invariant 2 was enforced in the hub only is gone, because C5 is what
changed it; and the line that read *"the suite is red here and green on
CI, so CI is the verdict"* is corrected — CI settles `admission.rs:419`
and not `slots.rs:744`, which it **skips**.

## Not done, and why

- **No decision entry for ORCH-068.** The brief asked for a dated entry
  for ORCH-075 and, for ORCH-068, for the decision to be recorded and
  the row moved. It is recorded on the row, in Alexa's terms, with the
  date. If a `D` number is wanted for it, it is the next one free.
- **The ORCH-068 work itself** — aligning `ops/agents/env.sh` and
  `state-root.md` — is `herd-step`'s, as briefed.
- **ORCH-091 is narrowed, not closed.** All three index rows exist now;
  what is left is one sentence in the numbering note saying whether `10`
  stands as written. That is the row's owner's call, not the
  integrator's, and it is not Alexa's.
- **The two ORCH-069 failures were not re-verified on `origin/main`
  alone** in this pass. They were verified there at PR #88's and PR
  #72's integrations (`c1d5ce7`) and the board records it; a second
  full suite on a shared host was not worth the hour.
- **No row was written for `ORCH-093`.** The coordinator named it as a
  planner's live item deriving the hub's disk policy from ORCH-076. Its
  text is not mine to invent and it is held outside this board, so
  `## Next` names it and says it has no row here. A live item with no
  row is the failure ORCH-073 was about; whoever holds it should write
  it.
- **No tag, no release, no issue**, nothing on the consumer's side
  (D161), nothing written in the orchestrator's checkout.

## Gates

The diff touches code, so all four Rust gates ran, plus both scripts.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

```
$ ./scripts/check-orchestration.sh
check-orchestration: 111 passed, 0 failed
```

```
$ nix develop --command cargo fmt --all --check
(no output, exit 0)
```

```
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 48.97s
```

```
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.34s
```

```
$ nix develop --command ./scripts/test.sh
test result: FAILED. 4 passed; 1 failed; 0 ignored  (herd-hub --test admission)
test result: FAILED. 7 passed; 1 failed; 0 ignored  (herd-hub --test slots)
test suite: FAILED (cargo test exited 101)
```

**Red, and red exactly where ORCH-069 says this host is red — nowhere
else.** The two failures, verbatim:

```
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' panicked at herd-hub/tests/admission.rs:419:5:
the inherited job's 4 GiB of declared memory must still be held: queued 2nd of 2 at priority step, ... on lender, 1.8 GiB free of its 5.8 GiB memory admission, 5 GiB needed
```

```
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at herd-hub/tests/slots.rs:744:9:
assertion `left == right` failed: the figure E-22 asks for, in the process's own words.
  left: "built with -j2"
 right: "built with -j7"
```

Both compare a figure the hub derives from this host's live `MemTotal`
against constants written for a 17 GiB machine. **Neither is this
branch's doing**, and no other binary failed: `announced_job_ceiling`,
the file C5 part 3 adds, ran **8 passed, 0 failed, 1 ignored**.

**Skips: a floor of 14, not a count**, because the suite did not finish
(D179's own rule, and the run printed it that way). Ten for cgroup v2
delegation, two for a ZFS data root with no hard `io.max`, and two bench
skips — `flood-fifty` and `flood-two-fifty` — because the host's
one-minute load was 8.85, over the bench's ceiling of 8.

**On CI and `slots.rs:744`: do not read a green `check` as that
assertion passing.** A GitHub runner reports `MemTotal` 15983 MiB,
under the guard's 17408, so the assertion is **skipped** there, and by
D27 a skip is never reported as a pass. `admission.rs:419` does run on
CI and does pass.

## Risks and findings

1. **PR #85's base was `watchdog-measures-each-age`**, not `main`. That
   branch is already merged, so the PR's own diff and its `DIRTY` flag
   were both measured against a stale base. Retargeted to `main` before
   pushing. Anyone reading the PR's earlier conversation should know the
   file list it showed was not the list against `main`.
2. **The allocation of the 65 references is a reading, and a reader may
   disagree with two of them.** `herd-proto/src/lib.rs:1632`
   (`at_its_ceiling` on `AttemptAbandoned`) is tagged *invariant 2* in
   its own doc, which points at D180, but the field exists so that a
   ceiling refusal can be told apart and charged nothing, which is D182;
   D182 is what the entry itself claims. Same reasoning for
   `herd-runner/src/lib.rs:1661`. Both are comments; neither changes
   behaviour.
3. **D182 is one-way and the entry says so.** A hub rolled back past
   this build refuses to read a record that ended `refused_as_full`,
   because `JobOutcome` carries no `#[serde(other)]` by design. The
   lever if that window matters is `--ceiling-refusals-allowed` set high
   enough that no job reaches the bound during it.
4. **`ORCH-022` still says three known-unstable tests** and this run
   showed none of them failing. It is main's row, not this branch's, and
   it was not touched.

## Decisions needed

**None from this task.** No row on the board waits on Alexa any more:
ORCH-076, the last one, was answered by measurement today.

## Resume point

`herd-step` has three rows ready and none of them needs Alexa: ORCH-068
(align `ops/agents/env.sh` on the live launcher, repository side only),
ORCH-091 (one sentence in the dossier index's numbering note), and the
queue behind ORCH-079 once that agent lands it.

## Processes

None left running. Every `cargo`, `nix` and script invocation was a
foreground or backgrounded child of this session and exited. No
`pkill`, no `killall`, nothing killed by name. One compile at a time.
