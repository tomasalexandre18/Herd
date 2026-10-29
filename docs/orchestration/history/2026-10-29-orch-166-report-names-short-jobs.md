# orch-166-report-names-short-jobs — handoff 2026-10-29

- **Item:** ORCH-166
- **Agent:** herd-step
- **Branch / commits:** `orch166-report`, based on `origin/main`
  `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`. Four, in order and all
  fast-forward — the branch was never force-pushed and no published
  commit was rewritten: `3a7a47b` the three lists, `7657389` this
  handoff, `96c2027` the ordering the orchestrator decided afterwards,
  and this file's update on top.
- **Gate:** **RED**, on the host and not on this change. Read *Gates*
  before anything else is done with this branch.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Read this first: the fixture already held the defect state

The job this change exists to name was **already in the committed test
fixture, and the whole-text assertion passed straight over it.**
`every_figure_of_the_report_is_this_fixture_to_the_byte` has asserted
every character of this report since ORCH-125, and its `j0000000008`
claims 2 000 600 bytes against a `files/` holding the 7 bytes of
`a-short`. The middle state. Green, every landing, for days.

It was green because the assertion checked the text the report *did*
print, and no line of that text was about this job. Two correct tests
either side of a gap nothing asserted — the arithmetic test checked the
sums, the text test checked the rendering, and "is this job named
anywhere" was in neither. That is the same shape as the defect that
shipped in v0.4.0, and it is worth more than any figure below: a
whole-output assertion proves the output is stable, not that it is
sufficient, and nothing catches a missing line except someone asking
what the output is *for*.

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
       lists either side of this one name. Largest shortfall first, so the
       names that fit are the ones that matter, and both figures are on
       each line, so the shortfall can be checked rather than trusted.
       jobs/j0000000008: claims 2000600 bytes (1.91 MiB), files/ measures 7 bytes, short by 2000593 bytes (1.91 MiB)
   ```

   bounded by the existing `NAMED_AT_MOST` (10) with the same `and N
   more` tail the unread and conflict lists already use.

   **Ordered by shortfall, descending, ties broken by job name — and that
   order is deliberately not the order of its neighbours.** Every other
   list in this report is in the walk's sorted job order. Do not "fix"
   this one back to match them. The reason is on the field in
   `report.rs` and it is this: the other lists are all-or-nothing states
   where the order carries no information, while this one is a
   *magnitude* that gets truncated at ten. In job order the ten names
   that fit would be the ten lowest ids — arbitrary with respect to the
   question — and an operator would have to re-sort by hand, which is
   the shell loop this list exists to stop. The tie-break is what keeps
   the rendered text a function of its inputs once the primary key stops
   being unique, so the whole-text assertion still holds.
   (Decided by the orchestrator, 2026-10-29, over my own argument for
   consistency with the neighbours; the counter-argument is the one
   above and it is the better one.)
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

Proof, in `herd-hub/tests/state_root_report.rs` (7 tests, all green):

- `every_figure_of_the_report_is_this_fixture_to_the_byte` — the existing
  whole-text assertion, extended. Its fixture's `j0000000008` was
  **already** in the middle state and no line named it: see the section
  at the top of this file, which is the finding of this task worth the
  most. The test now asserts the value, that
  `shortfall() == claimed - measured`, that it is 2 000 593, and
  `parts_jobs == [("j0000000006", 3)]`.
- `a_job_claiming_more_than_its_files_holds_is_named_with_both_figures` —
  a root with the middle state and both its neighbours: 1 000 claimed /
  400 held, 1 000 claimed / nothing held, 500 claimed / 500 held. Only
  the first is in the new list, the second is in `claimed_but_absent`,
  the third in neither; `claimed_bytes` 2 500, `measured_bytes` 900,
  gap 1 600, shortfall 600 — the figures check by arithmetic against the
  totals as well as inside the value.
- `the_shortfall_list_names_the_ten_largest_and_counts_the_rest` — the
  bound **and** the order, as one test. 13 jobs, job *n* claiming
  1 000 × *n* and holding one byte, so the shortfalls rise with the id.
  The count line says 13; `j0000000004..13` are named and
  `j0000000001..3` are not; `and 3 more` is printed;
  `j0000000013` is printed before `j0000000004`; the value's positions
  are asserted one by one and `windows(2)` proves the list never rises.
  **Under the old job order the three left out would have been the three
  worst, and this test would fail** — which is the point of the order.
  10 is written out in the test rather than imported, so raising
  `NAMED_AT_MOST` fails it and somebody has to say the list got longer
  on purpose.
- `jobs_equally_short_fall_back_to_job_order` — two jobs 400 short and
  one 900 short, written to disk in an order that matches neither the
  answer nor the digest order, so nothing but the tie-break can produce
  `[("j0000000005", 900), ("j0000000002", 400), ("j0000000009", 400)]`.
- `a_sound_root_prints_no_empty_list` — a root with neither: both count
  lines print at nought, neither paragraph prints, and no `    jobs/`
  line exists anywhere in the text.
- `an_unfinished_collection_is_named_and_claims_nothing_about_arrival` —
  the job is named with its 7 bytes and the text no longer contains
  `still arriving`.
- `the_report_reads_what_the_store_itself_wrote` — unchanged.

**It still removes nothing, takes no lock and writes nothing.** Each of
the five new tests takes a `listing(root)` (every path and length under
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
- **No fourth list for the mirror case** (a still-claiming record whose
  `files/` measures *more* than it names). Decided against; see *Risks*.

## Gates

**The last full gate run went RED, it is reported RED, and nothing was
edited to make it green.** `./scripts/gate.sh`, **bare** (never nested
in `nix develop` — that nests `TMPDIR` and panics
`herd-runner/tests/confinement.rs:2529` on `SUN_LEN`), on `aa974c1`,
whose tree is **identical to this branch's tip but for this one file**
(the branch was rebuilt as fast-forward commits afterwards so that
nothing published had to be force-pushed):

```
gate: RED sha=aa974c1aa2333c666cbc1a7099b264bbb678a3ee failed=6:test-suite skips=at-rest
gate: NOT GREEN — this sha must not be pushed to main.
```

Steps 0–5, 7 and 8 were ok in that run — `check-docs.sh`,
`check-orchestration.sh`, `nix flake check`, `cargo fmt`, `cargo clippy`
and the skip comparison. **`step 6` alone is red**, and `step 7` was ok
because the skip list matched the at-rest expectation exactly, so no
skip was counted as a pass and the red is the suite and nothing else.

**Six assertions failed, and every one is the enclosing cgroup**, in the
suite's own words:

- `real_herd/mod.rs:1247` — `a_lend_this_host_can_really_make`, the
  suite's own precondition guard, refusing to assert rather than
  asserting the wrong host: *"the runner below is told to lend 2048 MiB
  of memory, but /proc/self/cgroup is enclosed by memory.high 18 GiB on
  /chatons.slice, which already holds 16.7 GiB, so the room here is
  1.2 GiB … every figure asserted after that would be this cgroup's and
  not this test's"* (D119). 1.2 GiB against the 2 GiB asked for.
- `admission.rs:456` — the same cause one step downstream: the runner
  had clamped itself to the cgroup, so the figures the test reads are
  the slice's. Its own message says it: *"on lender, 211 MiB free of its
  1.2 GiB memory admission, 1 GiB needed"* — 1.2 GiB being exactly the
  room above, where the test intended 2 GiB.
- `real_herd/mod.rs:805`, `:957` and `confinement.rs:1893` — 30-second
  waits that ran out and `the real runner never joined`, on a host whose
  slice had 1.2 GiB left.

**The report's own tests passed inside that red run**, which is what
says the red is not this change:

```
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

Not one failure is in `report.rs`, `state_root_report.rs` or anything
either one touches, and the same suite went **GREEN on `3a7a47b`**
earlier the same hour — this change minus the ordering — in 287s.

**A run before it went red the same way**, on `d187317` (same tree, this
file's earlier text), with 48 failures of the same two shapes, the slice
then at 1.8 GiB free and `load1` 19.31 after the suite; `step 6` took
681s. Reported red too; nothing was edited after it either. Its output
and skip list are the `d187317ab768` files named below.

**Why this was not run a third time.** Two runs, two reds, one cause
that this branch cannot reach: `/chatons.slice` is shared with the
sibling tasks and it refilled to 16.3–16.7 GiB *during* each suite, from
5 GiB free at the start of the second one. That is a collision with
other agents on a shared host, which is a thing to write down and hand
off rather than work around, and a third 12-core suite run while
siblings build would cost them what it cost me. The batch's own
`./scripts/gate.sh` at integration is what decides this change, and it
is `herd-merger`'s.

**Finding for the host rules, not for this change:
`bench-preflight.sh` cleared both runs and does not measure what the
suite needs.** It reads `/proc/loadavg` and `MemAvailable` — 8.40 then
4.15, and over 20 GiB available, all fine — and neither is the figure
that failed. The binding constraint is the *enclosing cgroup's*
headroom, `memory.high - memory.current` on `/chatons.slice`, which
`herd_runner::room::measure()` reads and the preflight does not. I added
that reading to my own waiting loop and held for 5 GiB of it, and the
suite still exhausted it mid-run, so the fix is a floor **with margin
for the suite's own growth**, not just a reading. It is a change to
`scripts/bench-preflight.sh`, outside this task's paths: written down
here rather than made.

Everything below ran **before** this file was committed, and each block
says which run it came from.

**An earlier full gate, GREEN, on code commit `3a7a47b`** — this change
without the ordering decided afterwards, so it is evidence about the
three lists and not about the sort:

```
gate: GREEN sha=3a7a47b8391394763ef36d91f0722e11b132f14f steps=0-8:ok host=servernixos load1=4.69 load1-end=20.30 skips=at-rest(host-busy:load1-end=20.30>8,no-bench-declined-for-load)
```

Steps 1 and 2 of that run:

```
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (24s)
```

**`./scripts/check-orchestration.sh` run on its own**, for the figure the
gate does not print: **`check-orchestration: 269 passed, 0 failed`** —
the baseline of the last five landings, unmoved.

**The three Rust gates on the ordering itself**, run on the content of
code commit `d7bbd2c` before it was committed: `cargo fmt --all --check`
clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
and the report's own suite —

```
running 7 tests
test a_sound_root_prints_no_empty_list ... ok
test an_unfinished_collection_is_named_and_claims_nothing_about_arrival ... ok
test a_job_claiming_more_than_its_files_holds_is_named_with_both_figures ... ok
test jobs_equally_short_fall_back_to_job_order ... ok
test every_figure_of_the_report_is_this_fixture_to_the_byte ... ok
test the_shortfall_list_names_the_ten_largest_and_counts_the_rest ... ok
test the_report_reads_what_the_store_itself_wrote ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

A fourth run, `./scripts/gate.sh` on `d7bbd2c` with this file still
untracked, **refused and is reported refused** — it is not a red gate and
not a pass either:

```
gate: REFUSED sha=d7bbd2c369bf4ff3c1083de78241286efdedb079 reason=the working tree is not clean, so the gate would not be about d7bbd2c369bf4ff3c1083de78241286efdedb079
```

It was right to: a gate on a tree that is not what gets pushed proves
nothing, which is why this file is committed first and the gate is last.

Skips: **12 `HERD-SKIPPED` lines in every run of the three, the at-rest
expectation matched exactly each time**, each copied as a skip and none
counted as a pass (D27). The list did not move between the GREEN run and
the two red ones. By name:

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
flood benches **ran** in every run (`ran flood-fifty`,
`ran flood-two-fifty`), so the load-conditional delta list excused
nothing anywhere.

This commit is **prose only**, and `./scripts/check-docs.sh` was run on
its content before it was committed: `docs ok`. The four Rust gates are
**not run again for it** — `fmt`, `clippy`, `nix flake check` and the
suite were all exercised on the code tree it sits on top of, three of
them green and the suite red for the reason above, and a prose line
cannot move any of them. What covers them is the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s and not
mine.

Receipts kept, under this session's scratchpad directory
`/mnt/ssd_980/herd/claude/tmp/claude-1002/` +
`-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/`:

```
gate-orch166.log         the GREEN gate on 3a7a47b (no ordering yet)
gate-orch166.stdout      that run's stdout, verdict line included
gate-orch166-v2.stdout   the REFUSED run, dirty tree, no verdict earned
gate-orch166-v3.log      the first RED run, on d187317
gate-orch166-v3.stdout   its stdout and verdict line
gate-orch166-v4.log      the second RED run, on aa974c1
gate-orch166-v4.stdout   its stdout and verdict line
check-docs-orch166.txt   the docs run
```

Each red run's full suite output, every failure with its own diagnosis,
and its skip list are in the worktree beside the receipts:

```
target/herd-gate/test-output-d187317ab768.txt   first red run
target/herd-gate/skips-d187317ab768.txt         its 12 skips
target/herd-gate/test-output-aa974c1aa233.txt   second red run
target/herd-gate/skips-aa974c1aa233.txt         its 12 skips
```

The gates' own receipts live under `target/herd-gate/` too, one file
named for each sha that earned one — so `3a7a47b….receipt` exists and
**neither red sha has one**. A refused run writes none and a red run
writes none, which is the point of them, and it is why
`--verify-receipt` cannot be talked into believing any of this was
green.

## Risks and findings

- **A whole-output assertion is not a sufficiency assertion.** The
  section at the top of this file. The strongest finding here and the
  only one that generalises past this report.
- **This list's order disagrees with its neighbours, on purpose.**
  Described under *Done* and on the field in `report.rs`. Written down in
  both places because the obvious tidy-up — make it match the other
  lists — silently un-fixes ORCH-166 for any root with more than ten
  such jobs, and the code would still compile and the tests would still
  be green except for the one that exists to catch exactly this
  (`the_shortfall_list_names_the_ten_largest_and_counts_the_rest`).
- **One long line.** A named shortfall carries three `bytes_said`
  figures, so at the consumer's scale the line is around 140 characters
  and wraps in an 80-column terminal. Kept on purpose, confirmed by the
  orchestrator: the claim, the measurement and the shortfall belong on
  one line an operator can act on, and exactness is the file's own rule
  (`bytes_said` prints the byte count always and the approximation beside
  it). Nothing is lost to wrapping. The alternative — bare byte counts
  for two of the three — mixes units within one line.
- **The condition is `claimed > measured`, not "claimed and measured
  disagree".** A job whose `files/` measures *more* than its record
  claims, while the record still claims bytes, is in no list — the gap
  total reports it as the disk measuring more. That is the mirror case of
  `removed_but_present` and was not in the brief. **Deliberately not
  built** (orchestrator, 2026-10-29): nobody has observed that state, and
  a list for a state nobody has seen is a line of output that would have
  to be kept true for ever. This note is its resting place; it is a cheap
  fourth list on the day one is seen.
- **`bench-preflight.sh` has a blind spot**, found by a red gate it had
  just cleared: see the last block of *Gates*. It is the one finding
  here that is about the host rules rather than about this change, and
  the fix is outside this task's paths.
- **No evidence either way about the 143 MB / 559 MB job itself.** Herd
  is shipped, not operated (D161): nothing here read the consumer's
  state root. The defect state is reproduced in a fixture, and the two
  figures in the brief are the orchestrator's provenance, not a
  measurement of mine.

## Decisions needed

**One, and it is about the host, not this change.** The suite cannot
pass on this machine while `/chatons.slice` is being filled by sibling
tasks: `a_lend_this_host_can_really_make` needs 2 GiB of cgroup room and
the slice went to 1.2 GiB free during both runs. Two gate runs were
spent finding that out.

- *Question:* who gets a quiet window for the gate, and does
  `bench-preflight.sh` grow a cgroup-headroom floor so the next agent
  does not spend two suite runs discovering this?
- *Recommendation:* add the reading to `bench-preflight.sh` with a floor
  of about 8 GiB — 2 GiB for the test plus margin for the suite's own
  growth, which ate 4 GiB in the second run — as a task of its own, and
  let `herd-merger`'s integration gate run when the siblings are done.
- *Fallback:* raise `memory.high` on the slice, or run the suite outside
  it, both of which the assertion itself suggests and neither of which
  is mine to do (D161, and `/etc/nixos` is out of bounds).

## Resume point

Branch `orch166-report` is pushed, four commits, fast-forward. **Its
gate is RED on the host and the next person must not read that as
green.** What to do with it, in order:

1. Re-run `./scripts/gate.sh` bare in the worktree when the siblings
   have stopped, checking **cgroup headroom** and not just
   `bench-preflight.sh`. Want 8 GiB or more of it, not 5:

   ```sh
   cd /sys/fs/cgroup/chatons.slice
   echo $(( ($(cat memory.high) - $(cat memory.current)) / 1073741824 ))
   ```
2. If it is green, the branch is ready for a batch; `herd-merger`
   integrates it and runs the batch's own gate on the integration tip.
3. If it is red again with the same two shapes
   (`real_herd/mod.rs:1247`, and 30-second waits), the host is the
   finding and the decision above is the one to take.

Nothing in this branch is waiting on an edit.

## Processes

PIDs started and ended: none beyond the foreground `cargo`, `nix
develop`, `./scripts/gate.sh`, `./scripts/check-orchestration.sh`,
`./scripts/check-docs.sh` and `./scripts/bench-preflight.sh` invocations
of this task, each of which exited on its own, plus four backgrounded
waiting shells (`until ./scripts/bench-preflight.sh; do sleep 30; done`,
the last of them also reading the slice's headroom) that held the heavy
phases back until the host allowed them, and one `while`-loop watcher
that was stopped by id once it proved too chatty. Nothing was killed by
name, no `pkill -f`, no `killall`.

`./scripts/bench-preflight.sh` **refused three times** — `load 24.13`,
`11.61`, `20.92`, each at or above the 9.00 ceiling on 12 cores — and
every compile and every gate run waited for it rather than being forced
past it. The longest wait was 52 polls, about 26 minutes.

Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch166-report`,
outside the worktree, 8.9 GB, `rm -rf`'d at the end of the task. The
gates' own receipts and suite outputs live in
`<worktree>/target/herd-gate`, which goes with the worktree and is what
the red runs' evidence is in — **so that directory must survive until
the gate is re-run**.

The worktree
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-orch166-report/`
is **left in place** on the orchestrator's instruction, and goes when
they say so.
