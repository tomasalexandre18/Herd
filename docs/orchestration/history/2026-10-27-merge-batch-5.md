# merge-batch-5 — handoff 2026-10-27

- **Item:** ORCH-117, ORCH-121 (branch 1) and ORCH-082 (branch 2); rows
  ORCH-123 and ORCH-124 opened here
- **Agent:** herd-merger
- **Branch / commit:** `merge-batch-5` / see *Resume point*, based on
  `origin/main` `2954c8e`
- **PR:** #104, `https://github.com/tomasalexandre18/Herd/pull/104`

## Done

Two branches, merged in the order the brief set, both with `--no-ff`:

1. `orch-117-width-anchor` at `c702313` — ORCH-117 and ORCH-121.
   First, because it fixes the gate that checks every other branch's
   prose, this handoff included.
2. `orch-082-store-lock-io` at `6abbf5e` — ORCH-082.

### The comparison before either merge

`origin/main` was re-read at `2954c8e`, not taken from the
orchestrator's checkout.

| branch | files | decisions added | conflict expected |
|---|---|---|---|
| `orch-117-width-anchor` | 7 | none, no `D-NEXT` | `board.md` only |
| `orch-082-store-lock-io` | 3 | none, no `D-NEXT` | `board.md` only |

ORCH-117 touches `scripts/check-docs.sh`, `scripts/check-orchestration.sh`,
`workflow.md`, `templates/brief.md`, `templates/handoff.md`, `board.md`
and its own handoff. ORCH-082 touches `herd-hub/src/store.rs`, `board.md`
and its own handoff. The only shared file is `board.md`.

### Conflicts: none, in any file

Both merges were clean. `board.md` was the only file both branches
touched and their hunks are forty-eight lines apart: ORCH-082 rewrites
line 67, ORCH-117 rewrites line 115 and adds two rows after it, which is
the `board.md:115-117` ORCH-117's author predicted. With git's three
lines of context the ranges do not meet, so `ort` auto-merged.

A clean auto-merge is not by itself evidence that both meanings
survived, so the batch board check was run **both ways, before the board
was edited at all** — the only order in which it means anything.

**Direction A — every line each branch changed is in the merged file,
byte for byte.** Each branch's board was diffed against `origin/main`'s,
and every added or changed line was looked for with `grep -Fxq` in the
merged board:

```text
--- branch 117: lines it added or changed vs main ---
added/changed lines: 3
ALL PRESENT VERBATIM in merged board
--- branch 082: lines it added or changed vs main ---
added/changed lines: 1
ALL PRESENT VERBATIM in merged board
```

**Direction B — the merged board differs from each branch's only by the
other branch's hunks.** Nothing else moved, and nothing was lost:

```text
--- merged vs board-117 (should be exactly ORCH-082's hunk) ---
67c67
--- merged vs board-082 (should be exactly ORCH-117's hunk) ---
115c115,117
```

Line 67 is ORCH-082's row; 115-117 are ORCH-117's rewritten row and the
two rows (ORCH-121, ORCH-122) its author added beside it. There is no
third difference in either direction.

### ORCH-117 verified rather than accepted

- **The harness is 146 passed, 0 failed** on the merged tree, from
  `./scripts/check-orchestration.sh`, in 11.9 s. All five new cases are
  named in that output.
- **Red first, independently reproduced.** At `ea5dbcf`, the commit that
  adds the cases before the anchor moves, the harness gives **143 passed,
  3 failed**. Two of the three are exactly the cases that name the
  defect — `an indented fenced block exempts its over-long content` and
  `an indented table row is exempt, as one at column 1 is`. The third,
  `the logging switches are not gitignored`, is an artefact of my method:
  I extracted the commit with `git archive`, so there is no `.git` for
  that case to consult, and it fails at the merged head the same way
  under the same extraction while passing in a real worktree. The
  author's **144 / 2** therefore holds.
- **`check-docs.sh` is `docs ok` and stays in the hundreds of
  milliseconds:** 268, 266, 271 and 263 ms over four runs on the merged
  tree.
- **Blast radius, reproduced independently against `origin/main`'s own
  92-file corpus** by running both width passes with the limit lowered to
  `cols > 0` and comparing the sets of lines each one checks:

```text
files: 92
OLD prose lines: 21376
NEW prose lines: 21371
newly checked: 0
no longer checked:
docs/orchestration/gotchas/build-weight-off-root.md:26
docs/orchestration/gotchas/build-weight-off-root.md:27
docs/orchestration/gotchas/build-weight-off-root.md:28
docs/orchestration/gotchas/build-weight-off-root.md:29
docs/orchestration/gotchas/build-weight-off-root.md:30
```

  **21,376 → 21,371 prose lines: 5 lines change, 0 newly checked.** Not
  "no change" — five lines stop being width-checked, all five the
  indented `sh` block of that gotcha, at 10, 59, 26, 31 and 8 columns, so
  no line that was within the limit leaves the pass and no line enters
  it.

- **All 92 files are balanced under both rules**, checked with an `END`
  clause on each: no file is left with a fence open before the change or
  after it, and the same holds for the 94-file corpus this merge
  produces. ORCH-122 would be green on adoption.
- **The `26` → `32` correction is right, and I checked it by
  instrumentation rather than by grep** — grep is what makes it look
  wrong. Counting `^[[:space:]]*expect_(red|green) ` gives 30 cases, and
  32 only if the two function *definitions* are counted too. Appending a
  line to `check-docs.sh` that logs its `$PWD` and running the harness
  gives the real figure:

```text
total invocations: 35
      3 /tmp/herd-orch-check.5N2IMU/corpus
     32 /tmp/herd-orch-check.5N2IMU/repo
```

  **32 runs against the full copy of the real repository**, which is what
  both comments claim, plus three against the toy corpus the D-number
  collision cases build. The figure stands as written.

- **The three corrections the branch carries all survive the merge**,
  checked in the merged file: the row now says the stray-fence hole is
  pre-existing and unchanged and points at ORCH-122; the incident count
  reads three fences and one table, all on 2026-10-27, with `39fe513`,
  `c67b5bb` and `9b74c66` excluded as genuine over-long prose; and both
  scripts say 32 with the PR #100 sentence in the past tense.

### ORCH-082 verified rather than accepted

- **The ordering oracle: 2 passed, 0 failed.**
- **The 308-state enumeration is unchanged:** 308 / 176 / 44 / 4 / 99 / 0.
- **The suite: 838 passed, 0 failed, 12 skipped** — see *Risks and
  findings* 1, because the handoff says 837.
- The measured figures are reported as the author reported them and are
  not rounded up anywhere I wrote them: ×8.49 → ×1.71 at eight callers
  against a **lockless control of ×1.66**, 9.994 s → 1.741 s at 20 000
  directories, and **at one caller it saves nothing** (55.66 → 54.29 ms,
  inside the noise). The row's value is prospective and the board now
  says so in those words.
- The two findings I was asked to open were checked against the code
  before being written down, not copied from the handoff. See
  *Risks and findings* 3 and 4.

### Board

- **ORCH-117** and **ORCH-121**: integration note and PR #104 added;
  state was already `done` on the branch. The self-flag about the five
  `expect_green` cases is written into the ORCH-117 row, not only here.
- **ORCH-082**: integration note, the no-conflict record, the verified
  figures, and the sentence that keeps the gain from being read as
  general.
- **ORCH-087**: its one dependency is recorded as met, and the row now
  says in the same breath that **it is not started and that opening it is
  Alexa's word** — D183 adopted the keeper as the direction, not as an
  open worksite. The *Depends on* cell reads `ORCH-082 (met 2026-10-27)`.
- **The closing note §3's "ORCH-082 is the one of the four still open" is
  gone.** It now records that all four of ORCH-079 to ORCH-082 are done,
  what the gain is and is not, and where ORCH-087 stands.
- **ORCH-123** opened: ~94 % of the `T` walk is per-record CPU, with the
  cause marked as a hypothesis. Placed beside ORCH-021 by its
  *Depends on*.
- **ORCH-124** opened: the permanent budget leak, cross-referenced from
  **ORCH-093** and **ORCH-095**, which are re-deriving the output budget
  it corrupts.

Both branches' own handoffs exist under `docs/orchestration/history/`:
`2026-10-27-orch-117-width-anchor.md` and
`2026-10-27-orch-082-store-lock-io.md`.

### `docs/status.md`

Restated, because ORCH-082 changes the hub. The commit count, the suite
line (838, batch 5), "twelve changes are in no release" with ORCH-082 as
the second cost cut, and item 2 of *What the software still does not do*,
which now names ORCH-123 and ORCH-124. **90 lines**, the enforced
maximum, so two older paragraphs were compressed to pay for it; nothing
state-bearing was dropped.

## Not done, and why

- **No `D` number allocated.** There is no `D-NEXT` anywhere in either
  branch or in the merged tree, both authors say so in their own
  handoffs, and neither change decides anything at project level:
  ORCH-117 loosens an anchor inside a gate script and ORCH-121 writes two
  sentences into two templates, both inside their rows' ownership;
  ORCH-082's change to `unfinished_jobs`'s postcondition is argued inside
  `07 §7 S2`, which **D183** already adopted. Highest on `main` is
  **D184**. **D143, D154 and D170 stay gaps** (D148).
- **No tag, no release, nothing on the consumer's side** (invariant 9,
  D161).
- **ORCH-122 is left `open` as its author wrote it.** The `END` check it
  proposes is not in this batch.
- **ORCH-087 is not opened.** Recording that its dependency is met is not
  the same act as starting it, and the second is Alexa's.

## Gates

Run on the merged tree. The handoff was written and committed first and
the docs gate run after it, which is ORCH-121's rule landing in this very
batch. Every gate was redirected to a file; none was piped to `head` or
`tail`. The diff touches `herd-hub/src/store.rs`, so all four Rust gates
ran, with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/merge-batch-5` after
`./scripts/bench-preflight.sh` said `ok, safe to start` (332 GiB free,
load 0.65 on 12 cores).

`nix develop --command cargo fmt --all --check` — rc 0, no output.

`nix develop --command cargo build --workspace`:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.30s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```text
    Checking herd-runner v0.3.7 (.../merge-batch-5/wt/herd-runner)
    Checking herd-hub v0.3.7 (.../merge-batch-5/wt/herd-hub)
    Checking herdctl v0.3.7 (.../merge-batch-5/wt/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.95s
```

`nix develop --command ./scripts/test.sh` — rc 0:

```text
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
```

838 passed, 0 failed, 12 skipped — summed over the 55 `test result:`
lines, one per test binary, with no binary run twice.

`nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle`:

```text
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 198 filtered out; finished in 0.08s
```

`nix develop --command cargo test -p herd-hub --test crash_points --
--ignored --nocapture --test-threads=1`:

```text
ORCH-080 — 308 constructed states, one job directory each
308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.77s
```

`./scripts/check-docs.sh` — rc 0:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — rc 0:

```text
check-orchestration: 146 passed, 0 failed
```

**Skips: 12, all one host fact**, copied from the run and never counted
as passes (D27):
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`. Every one is "cgroup
v2 delegation is unavailable" or "this host has no cgroup v2 subtree for
a job to carry an io.max in". Neither branch touches the runner or
confinement. ORCH-116's correction to 14 under load is unaffected.

## Risks and findings

1. **The suite is 838, not the 837 the ORCH-082 handoff states as "a
   count rather than a floor".** The Rust tree of this merge is
   byte-for-byte the branch tip's — `git diff origin/orch-082-store-lock-io
   HEAD` shows only `scripts/check-docs.sh` and
   `scripts/check-orchestration.sh` — so this is a miscount in the
   handoff and not a test that appeared. Nothing follows from it except
   that the figure in that file should not be quoted. The board now
   carries 838 and says why.
2. **All five of ORCH-117's new harness cases are `expect_green`, so
   none of them goes red if the width rule disappears entirely.** They
   assert that the exemptions apply, which is the defect ORCH-117 names,
   and they do not hold the rule's existence. That direction is held by
   two older `expect_red` cases — the 85-character one
   (`check-orchestration.sh:90`) and the 80-character multibyte one
   (`:103`) — and by nothing this branch added. The author flagged it
   himself; it is repeated here and in the ORCH-117 row so it cannot be
   lost. Deleting the width pass outright would still go red, so this is
   a gap in coverage and not a hole in the gate.
3. **ORCH-123's cause is a hypothesis and the row says so.** I read
   `read_stored` in the merged tree and confirmed the shape — three
   `serde_json::from_str` calls over the same `text` (`VersionProbe`,
   `DurableLabels`, `RecordFile`) with `LabelSet::new` between the second
   and the third. What is **not** confirmed is that those parses are
   where the 55.6 µs per record goes: nobody has attributed one
   microsecond of it, the measurement is a debug build, and the arithmetic
   bound (55.6 µs through the store against 3.2 µs for the control's
   `read_to_string`) says only that it is CPU and not disk.
4. **ORCH-124 is reachable today and I verified both halves in the
   code.** `write_output_chunk` (`store.rs:2012` here, `:1978` at
   `2954c8e`) has no reference to `outputs.removed` anywhere in its body,
   while `collection_owed` at `:2700` does check it; and `sweep_now`'s
   loop at `:791` adds the bytes of any record with
   `outputs.removed.is_some()` to `could_not_free` and `continue`s, so
   that job is never a candidate again. The leak is therefore permanent
   rather than merely wasteful, and it is independent of ORCH-082:
   `collections_owed` is advisory by construction and its caller acts on
   the answer with no lock held.
5. **One method note against my own evidence.** The red-first
   reproduction at `ea5dbcf` was done from a `git archive` extraction,
   not a worktree, which is why a third case fails there. I did not treat
   that third failure as a branch failure, and I checked that it fails
   the same way at the merged head under the same extraction.
6. **Nothing was weakened to pass.** No test was edited, no constant
   moved, no `#[ignore]` added, and the oracle and the enumeration are
   byte-identical to the branch's own figures.

## Decisions needed

None. Two rows were opened and neither asks Alexa anything yet.
**ORCH-087 is the one place where a decision is latent**: its dependency
is now met, and the board says that starting it is Alexa's word rather
than a consequence of this merge.

## Resume point

```sh
git fetch origin && git log --oneline -3 origin/main
gh pr view 104
```

Next integration takes whatever the board's *Owed* section still holds.
**ORCH-123 and ORCH-124 are the two new rows a planner should see first**:
ORCH-123 is a larger lever on `collections_owed` than the lock was, and
ORCH-124 is live on the consumer's machine today.

## Processes

PIDs started and ended: none beyond the gates' own `cargo`, `nix` and
shell children, each of which exited on its own; nothing was killed, and
no process of the consumer's day-old `herd-runner` or `herd-hub` was
touched.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/merge-batch-5` is deleted once
the branch is pushed; the worktree at
`.../scratchpad/merge-batch-5/wt` holds no `target/` of its own.
