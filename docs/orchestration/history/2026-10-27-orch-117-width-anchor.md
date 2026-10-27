# orch-117-width-anchor — handoff 2026-10-27

- **Item:** ORCH-117 (the anchors), ORCH-121 (the gate ordering);
  ORCH-122 opened, not done
- **Agent:** herd-step
- **Branch / commit:** `orch-117-width-anchor` / `bbd31e8` plus this
  handoff, based on origin/main `2954c8e`
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

Three commits, in the order a `herd-reviewer` pass prescribed.

1. `ea5dbcf` — **five cases, committed before the anchor moved.** The
   reviewer's top finding was that the 141-case harness could not tell
   today's anchors from loosened ones: he loosened each in turn and the
   harness stayed byte-identical to its control over six runs. Only
   outright removal of an exemption was caught, incidentally, by the
   85-character case. The five new cases sit beside the width cases in
   `scripts/check-orchestration.sh` and pin both directions — the
   exemption exists at all, it applies at any indent, and it does not
   close a fence early.

   Red-first run, on `ea5dbcf`, with the anchors untouched:

   ```text
   ok    over-long line in docs/orchestration
   ok    a 79-character line of 3-byte characters (237 bytes) is within the limit
   ok    an 80-character line of 3-byte characters is over the limit
   ok    a 79-character line of 4-byte characters (316 bytes) is within the limit
   FAIL  an indented fenced block exempts its over-long content
       docs/orchestration/workflow.md has lines over 79 columns:
           docs/orchestration/workflow.md:202 (103 cols)
       note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
       docs check failed
   FAIL  an indented table row is exempt, as one at column 1 is
       docs/orchestration/workflow.md has lines over 79 columns:
           docs/orchestration/workflow.md:201 (210 cols)
       note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
       docs check failed
   ok    a fence at column 1 exempts a 1600-character line
   ok    a table row at column 1 is exempt over 79 columns
   ok    an indented ``` inside a fence at column 1 does not close it
   
   check-orchestration: 144 passed, 2 failed
   ```

   Red on exactly the two cases that name the defect, and nothing else:
   the 85-character case and both multibyte cases behave as before.

2. `3f18dd2` — **one anchor edit**, the shape the reviewer tested:

   ```awk
   /^[[:space:]]*```/ {
     ind = match($0, /[^ \t]/) - 1
     if (!fence) { fence = 1; find = ind }
     else if (ind == find) { fence = 0 }
     next
   }
   fence { next }
   /^[[:space:]]*\|/ { next }
   ```

   A fence opens at any indent and closes **only at the indent it
   opened on**, which is what keeps case 5 green: a plain toggle at any
   indent would let a nested fence close the outer one and width-check
   the rest of the block. After it, `146 passed, 0 failed`.

3. `bbd31e8` — **subject 2, two sentences and one ordering fix.**
   `templates/handoff.md` and the `Gates:` line of `templates/brief.md`
   both say the docs gate is the last thing run, after the handoff is
   committed and before the push, and that the output quoted in the
   handoff is that run's. `workflow.md` *The loop* step 4 read
   *"pushes the branch, and ends with a handoff"* — literally the order
   that caused the incident — and now puts the gate and the push after
   the handoff, with the amplification named: under one PR per batch,
   one ungated handoff reddens the whole batch's PR. Board: ORCH-117's
   row corrected, ORCH-121 and ORCH-122 opened (`ORCH-121` was free on
   `origin/main`; the highest id there is ORCH-120).

**Blast radius, measured as the reviewer measured it.** The limit
lowered to `cols > 0` in both versions of `check-docs.sh`, run over the
unmodified corpus of `origin/main`, and the two sets of lines each
classifies as prose compared:

```text
old prose lines: 21376
new prose lines: 21371
=== only in old (no longer checked) ===
docs/orchestration/gotchas/build-weight-off-root.md:26 (10 cols)
docs/orchestration/gotchas/build-weight-off-root.md:27 (59 cols)
docs/orchestration/gotchas/build-weight-off-root.md:28 (26 cols)
docs/orchestration/gotchas/build-weight-off-root.md:29 (31 cols)
docs/orchestration/gotchas/build-weight-off-root.md:30 (8 cols)
=== only in new (newly checked) ===
```

**5 lines change, 0 newly checked**, all five the indented ```sh block
at `build-weight-off-root.md:26-30`, the widest of them 59 columns, so
no finding at the real limit changes either way. Not "no change": five
lines really do leave the width pass.

**This handoff is itself the live case.** Its red-first evidence above
sits in a fenced block indented inside a numbered item. Run
`origin/main`'s `check-docs.sh` over this tree and it says:

```text
docs/orchestration/history/2026-10-27-orch-117-width-anchor.md has lines over 79 columns:
    …:29 (81 cols)
    …:31 (81 cols)
    …:35 (95 cols)
    …:40 (95 cols)
docs check failed
```

Four lines of pasted gate output, which has to stay verbatim. With the
new anchors the same tree is `docs ok`, and the one line that really was
over the limit — a 136-column path in prose — was found and fenced.

**mawk.** A branch push does not trigger CI — `ci.yml` runs on
`pull_request` and `push` to `main` only, so there is no run to point
at, and the `docs` job is unverified on CI until the batch PR. The last
bug in exactly these lines was a gawk/mawk divergence
(`check-docs.sh:41-48`), so it was checked locally instead under
`nix shell nixpkgs#mawk`, with a shim directory putting `awk` on PATH
as mawk — **mawk 1.3.4 20240819, the same mawk Ubuntu ships as `awk`**:

```text
# before the anchor moved (origin/main's check-docs.sh in this worktree)
FAIL  an indented fenced block exempts its over-long content
FAIL  an indented table row is exempt, as one at column 1 is
check-orchestration: 144 passed, 2 failed
# after
check-orchestration: 146 passed, 0 failed
```

Byte-identical verdicts to gawk 5.4.1 on both sides, so the cases
discriminate under mawk too and `match()` and `[[:space:]]` behave as
POSIX says. The harness takes 11 s under mawk here with 146 cases,
against the `docs` job's 5-minute cap.

**The stray fence, measured and left alone.** A scratch copy with one
unbalanced ``` at column 1 followed by a 120-column line:

```text
new rule, stray fence: rc=0
old rule, stray fence: rc=0
grep -c 'over 79': 0 and 0
```

Both green, when both should be red. The hole is **pre-existing and
unchanged**: the column-1 anchor was never what protected against it,
which is what ORCH-117's row claimed as the reason it was parked. That
claim is corrected on the row. Nothing here fixes the hole; it is
ORCH-122, with the reviewer's `END { if (fence) … }` proposal, and all
92 corpus files are balanced today under both rules, so it would be
green on adoption.

**The incident count, checked against the commit record.** ORCH-117's
row said *"two agents on consecutive days"* in one clause and *"a third
agent, a third day"* in another. The record shows **three fences and
one table, all four on 2026-10-27**: two fenced blocks un-indented in
`b02a58b`, one in `6676813`, and ORCH-107's six-line indented table in
`8470b05`. The three other wrap-shaped commits — `39fe513`, `c67b5bb`,
`9b74c66` — are genuine over-long prose and headings, not instances;
`9b74c66` is also dated 2026-10-25, not the 27th. The row now says
exactly that.

## Not done, and why

- **ORCH-122, the unbalanced-fence `END` check.** Opened as its own row
  on the brief's instruction, deliberately not smuggled into this
  change.
- **A tab-indented open paired with a space-indented close.** It does
  not pair, because `match($0, /[^ \t]/) - 1` counts a tab as one
  column. No such fence exists in the corpus; named as a known limit in
  `check-docs.sh`, in the harness comment and on the row, rather than
  given a case that would assert the limit as if it were wanted.
- **CI itself.** No run exists on this branch to point at (see above).
- **`docs/decisions.md`** — untouched, no `D-NEXT`, as briefed.

## Gates

All four Rust gates plus both scripts: the diff touches `scripts/`, so
the prose allowlist does not apply. Each one redirected to a file and
read from the file; nothing piped to `head` or `tail`.

```text
$ nix develop --command cargo build --workspace        → rc=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.63s
$ nix develop --command ./scripts/test.sh              → rc=0
    test result: ok. (837 passed, 0 failed, summed over the run)
     12 assertion(s) COULD NOT RUN on this host and were skipped.
     A green run does NOT mean these were checked.
    test suite: PASSED
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings → rc=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.40s
$ nix develop --command cargo fmt --all --check        → rc=0
    (no output beyond nix's "Git tree is dirty" warning)
$ ./scripts/check-docs.sh                              → rc=0
    note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
    docs ok
$ ./scripts/check-orchestration.sh                     → rc=0
    check-orchestration: 146 passed, 0 failed
```

`837 passed, 0 failed` is a count and not a floor, and matches the
figure the brief gave for this host.

**Skips: 12, each one printed as a skip (D27) and none counted as a
pass.** All twelve are the same host fact — this host has no delegated
cgroup v2 subtree, so a job cannot be given a cgroup of its own:
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
`the_runner_says_which_memory_ceiling_it_enforces`.

The two docs scripts above were run **after this handoff was committed**
and before the push, which is the rule this branch writes down; the
output quoted is that run's, and the only difference between the gated
tree and the pushed one is this paragraph's own wording.

## Risks and findings

- **The five new cases are all `expect_green`.** They assert that the
  exemptions apply, so none of them can go red if the whole width rule
  disappears — that direction is held by the 85-character case and by
  the multibyte pair, which are `expect_red`. Worth knowing before
  someone deletes the width rule and reads 146 green.
- **The run-count comments were stale and are now exact.** Both scripts
  said the harness runs `check-docs.sh` 26 times over a full corpus;
  `grep -cE '^(expect_red|expect_green)'` gives **32** after this
  change, and the cost argument those comments carry is now stated on
  that number. One sentence in `check-docs.sh` kept its historical 25
  and is now past tense, because it describes what PR #100 measured.
- **The harness is 11 s here, gawk or mawk.** Five more whole-corpus
  runs cost about 2 s locally. The 5m12s incident was the shell loop,
  not the case count, so the cap is not at risk — but the cost is still
  linear in cases × corpus, and that is why the limit above got a
  sentence instead of a case.
- **`herd-merger` will see the board row, not the width fix, as the
  conflict.** This branch rewrote ORCH-117's row and added two rows at
  `board.md:115-117`; another agent holds `herd-hub/src/store.rs`
  (ORCH-082) and does not touch these files, but any other branch in
  the batch that writes the board will conflict there.
- **`docs/status.md` untouched**, as the brief allows only `scripts/`
  and `docs/`-by-instruction; the merger writes status at integration.

## Decisions needed

None. One thing for whoever sizes ORCH-122: its `END` check would be
the first rule in `check-docs.sh` that reports a *file* rather than a
line, and the reviewer's proposal names the line the open fence started
on, which the current pass does not remember.

## Resume point

`herd-merger` takes `orch-117-width-anchor` into the next batch. The
command that reproduces the whole claim, in the worktree:

```sh
./scripts/check-docs.sh > /tmp/none.txt 2>&1 ; ./scripts/check-orchestration.sh
```

For the mawk half, prepend a shim directory whose `awk` is mawk:

```sh
mkdir -p /scratch/mawkbin
ln -sf "$(nix shell nixpkgs#mawk --command sh -c 'command -v mawk')" /scratch/mawkbin/awk
PATH=/scratch/mawkbin:$PATH ./scripts/check-orchestration.sh
```

## Processes

Started and ended by this task: one background shell running the four
Rust gates in sequence (`gates.sh`, PIDs 2924913 and 2927752, its `nix
develop` child), both exited on their own; no PID was killed, and
nothing was killed by name. The day-old `herd-runner` and `herd-hub` on
this host are the consumer's and were not touched.

Own `target/` deleted once the branch was pushed: 8.6 GB, removed by
this literal path in the same step as the push (workflow.md,
"Concurrency and host rules"), and nothing else:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-117/wt/target
```
