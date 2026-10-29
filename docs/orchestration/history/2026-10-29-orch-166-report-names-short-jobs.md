# orch-166-report-names-short-jobs — handoff 2026-10-29

- **Item:** ORCH-166
- **Agent:** herd-step
- **Branch / commit:** `orch166-report` / `3a7a47b` (code),
  based on `origin/main` `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`herd-hub report`'s figure 3 now names the jobs behind its total. It
already computed, per job, what the record claims and what `files/`
measures (`herd-hub/src/report.rs`, the one loop over `jobs/`), and named
jobs in only the two extremes: a claim against a `files/` measuring
nought, and bytes removed while `files/` still holds them. The middle —
claimed above measured, measured above nought — is the state a real
accounting defect leaves, and it was in no list at all. The first
consumer's job measured 143 MB against a record claiming 559 MB, so the
whole 396.76 MiB reached us as an anonymous total and they found the job
with a shell loop over a figure this report had already computed.

Three changes, all in figure 3's region, and **no new command-line
option**: a knob per question is how a reporting tool becomes a query
language.

1. **A third list.** `Accounting::claiming_more_than_held`, a
   `Vec<ClaimShortfall>` (`job`, `claimed`, `measured`, and
   `shortfall()` = `claimed - measured`), filled under the existing
   `still_held && !files.is_empty()` arm by the `else if job_bytes >
   measured` branch of the test that already caught `measured == 0`, so
   `measured > 0` holds by construction and no job is in two lists. It
   prints as

   ```
     claiming more than files/ holds     1 job(s)
       Bytes were claimed and some of them are there: neither extreme the
       lists either side of this one name. Both figures are on each line,
       so the shortfall can be checked rather than trusted.
       jobs/j0000000008: claims 2000600 bytes (1.91 MiB), files/ measures 7 bytes, short by 2000593 bytes (1.91 MiB)
   ```

   bounded by the existing `NAMED_AT_MOST` (10) with the same `and N
   more` tail the unread and conflict lists already use.
2. **`files.part/` named, not only counted.**
   `Accounting::parts_jobs` carries the per-job measurement the loop was
   already taking and throwing into a sum.
3. **`files.part/ still arriving` reworded** to `files.part/ unfinished
   collection`, with three lines under it saying a collection started
   and did not finish and that this report cannot tell whether anything
   is still coming. On the consumer's hub the old wording was false: the
   directory had sat untouched for six days through a reboot, and it was
   the line that would have pointed straight at the problem.

`ClaimShortfall` is re-exported from `herd-hub/src/lib.rs`. No version
number moved, `herd-hub/src/store.rs` was not touched, and the record's
contents did not change — only what the report says about them.

Proof, in `herd-hub/tests/state_root_report.rs` (6 tests, all green):

- `every_figure_of_the_report_is_this_fixture_to_the_byte` — the existing
  whole-text assertion, extended. Its fixture's `j0000000008` was
  **already** in the middle state (claims 2 000 600, `files/` holds the
  7 bytes of `a-short`); before this change no line named it. The test
  now asserts the value, that `shortfall() == claimed - measured`, that
  it is 2 000 593, and `parts_jobs == [("j0000000006", 3)]`.
- `a_job_claiming_more_than_its_files_holds_is_named_with_both_figures` —
  a root with the middle state and both its neighbours: 1 000 claimed /
  400 held, 1 000 claimed / nothing held, 500 claimed / 500 held. Only
  the first is in the new list, the second is in `claimed_but_absent`,
  the third in neither; `claimed_bytes` 2 500, `measured_bytes` 900,
  gap 1 600, shortfall 600 — the figures check by arithmetic against the
  totals as well as inside the value.
- `the_shortfall_list_names_the_first_ten_and_counts_the_rest` — 13 such
  jobs: the count line says 13, `jobs/j0000000001..10` are named,
  `j0000000011..13` are not, and `and 3 more` is printed. 10 is written
  out in the test rather than imported, so raising `NAMED_AT_MOST` fails
  it and somebody has to say the list got longer on purpose.
- `a_sound_root_prints_no_empty_list` — a root with neither: both count
  lines print at nought, neither paragraph prints, and no `    jobs/`
  line exists anywhere in the text.
- `an_unfinished_collection_is_named_and_claims_nothing_about_arrival` —
  the job is named with its 7 bytes and the text no longer contains
  `still arriving`.
- `the_report_reads_what_the_store_itself_wrote` — unchanged.

**It still removes nothing, takes no lock and writes nothing.** Each of
the four new tests takes a `listing(root)` (every path and length under
the root) before the read and asserts it is identical after; the
whole-text test does that *and* holds the `flock(2)` on `hub.lock` that a
live hub holds, for the whole run. No code was added that opens anything
for writing: the three additions read `metadata().len()` through the
existing `directory_bytes` call the loop already made.

## Not done, and why

- **No board row written.** The brief says propose it, not write it. The
  proposal is in this agent's report.
- **No `D-NEXT` entry.** The brief did not ask for one and this changes
  no decision: the report's contract (no lock, no write, no payload
  byte) is unchanged.
- **Nothing landed, no tag, no pull request, `main` untouched.**

## Gates

Full `./scripts/gate.sh`, **bare** (not nested in `nix develop`), on the
committed code tree `3a7a47b`:

```
gate: GREEN sha=3a7a47b8391394763ef36d91f0722e11b132f14f steps=0-8:ok host=servernixos load1=4.69 load1-end=20.30 skips=at-rest(host-busy:load1-end=20.30>8,no-bench-declined-for-load)
```

Steps 1 and 2, from that same run:

```
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (24s)
```

`./scripts/check-orchestration.sh` run on its own for the figure the gate
does not print: **`check-orchestration: 269 passed, 0 failed`** — the
baseline of the last five landings, unmoved.

Skips: **12 `HERD-SKIPPED` lines, the at-rest expectation matched
exactly**, each copied as a skip and none counted as a pass (D27). By
name:

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

Eleven are cgroup v2 delegation this host does not grant; the twelfth is
the same host refusing an `io.max` it cannot enforce. Both load-gated
flood benches **ran** (`ran flood-fifty`, `ran flood-two-fifty`), so the
load-conditional delta list excused nothing. `load1` rose 4.69 → 20.30
across the suite; the gate carries that as information and it decided
nothing.

The run about to happen on the tree that carries **this file** is
`./scripts/check-docs.sh` alone — this commit is prose only, so the Rust
gates are **not run** for it and are covered by the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s. Its output
goes to `check-docs-orch166.txt` below and is quoted in the agent's
report.

Receipts kept, all four under this session's scratchpad directory
`/mnt/ssd_980/herd/claude/tmp/claude-1002/` +
`-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/`:

```
gate-orch166.log                 the gate's own log
gate-orch166.stdout              the gate's stdout, verdict line included
check-docs-orch166.txt           this commit's docs run
```

and the gate's receipt, inside the worktree:

```
target/herd-gate/3a7a47b8391394763ef36d91f0722e11b132f14f.receipt
```

## Risks and findings

- **One long line.** A named shortfall carries three `bytes_said`
  figures, so at the consumer's scale the line is around 140 characters
  and wraps in an 80-column terminal. Kept on purpose: the brief asks
  for the claim, the measurement and the shortfall on one line an
  operator can act on, and exactness is the file's own rule (`bytes_said`
  prints the byte count always and the approximation beside it). Nothing
  is lost to wrapping. The alternative — bare byte counts for two of the
  three — mixes units within one line.
- **The list is in job-name order, not by shortfall.** That matches every
  other list in the file and makes the text a function of the directory
  order the walk already sorts. It means the ten jobs named are the ten
  with the lowest ids and **not** the ten worst offenders. A reader who
  needs the worst has the count and the total above. Changing it to sort
  by shortfall descending is a one-line change if the orchestrator wants
  the other trade; I did not take it, because the two neighbouring lists
  would then disagree with it.
- **The condition is `claimed > measured`, not "claimed and measured
  disagree".** A job whose `files/` measures *more* than its record
  claims, while the record still claims bytes, is in no list — the gap
  total reports it as the disk measuring more. That is the mirror case of
  `removed_but_present` and was not in the brief; it is a real remaining
  hole in figure 3's naming, and the cheapest fourth list if it is ever
  seen. Noted, not built.
- **No evidence either way about the 143 MB / 559 MB job itself.** Herd
  is shipped, not operated (D161): nothing here read the consumer's
  state root. The defect state is reproduced in a fixture, and the two
  figures in the brief are the orchestrator's provenance, not a
  measurement of mine.

## Decisions needed

none.

## Resume point

Branch `orch166-report` is pushed and green at the code commit. For
whoever continues: nothing is required of this task. The orchestrator
decides which batch takes it; `herd-merger` integrates the branch, runs
the batch's own `./scripts/gate.sh` on the integration tip, and writes
the board row proposed in the agent's report.

## Processes

PIDs started and ended: none beyond the foreground `cargo`, `nix
develop`, `./scripts/gate.sh`, `./scripts/check-orchestration.sh` and
`./scripts/bench-preflight.sh` invocations of this task, each of which
exited on its own. Nothing was killed, no `pkill -f`, no kill by name.

Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch166-report`,
outside the worktree, `rm -rf`'d once the branch was pushed. The gate's
own receipts live in `<worktree>/target/herd-gate`, which goes with the
worktree.
