# orch-125-state-root-report — handoff 2026-10-28

- **Item:** ORCH-125
- **Agent:** herd-step
- **Branch / commit:** `orch-125-state-root-report` / `c3b2d2f`, based on
  origin/main `bc83717`
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

`herd-hub report <STATE_DIR>` exists: a subcommand of the hub binary that
reads a state root and prints the three figures, then exits. `herd-hub`
with no subcommand serves exactly as before, which a test asserts.

**Where it lives, and why.** A subcommand of `herd-hub`, with the
arithmetic in a new `herd-hub/src/report.rs`.

- Not `herdctl`: everything `herdctl` does crosses the WebSocket
  protocol, so this would have meant a new message and a
  `PROTOCOL_VERSION` bump on a version frozen at 2. It is also the wrong
  machine — a requester runs anywhere, this reads one host's directory.
  Nothing was added to `herd-proto`.
- Not a flag on the serving path: all seventeen serving options are
  meaningless here, and `--report` would have to be documented as
  ignoring them.
- The hub binary is already in the consumer's image, so
  `docker compose exec hub herd-hub report` needs no new image, no new
  entrypoint and no port. The state root is a positional argument
  defaulting to `$HERD_STATE_DIR` and then to `herd-state`, so inside
  the hub's own container `herd-hub report` alone is enough.

**Figure 1, `r_file`.** Printed twice, each with the four counts it
rests on and never as a bare ratio: over every record the root holds
(bytes gone or not — what dedup would have saved over the hub's whole
history), and over the collections whose bytes the records still claim
(what it would save on today's disk). Two cases the brief's formula did
not cover, both decided towards the lower claim:

- A `KeptFile` with an empty `sha256` — a record written before the
  field existed — is counted and reported apart (count and bytes) and
  left out of **both** sides of both ratios. Putting its bytes in the
  numerator alone would inflate `r_file` by exactly the undigested
  bytes.
- A digest two records give two different lengths is an accounting
  defect; it is named with both figures and the **larger** length is
  used as its distinct size, which makes `r_file` the lower of the two
  claims it could have made.
- `0 / 0` — a root whose only outputs are empty files — prints
  "undefined: no record names a digested byte", not 1.00.

**Figure 2, deciles.** Deciles 1-9 by nearest rank (decile *k* is the
smallest size at least `ceil(k*n/10)` of the jobs are at or under: no
interpolation, so every figure printed is a size some job really has),
plus the maximum, the total, and what the top decile carries as a share.
The population is **every record read**, including the ones naming no
file — the consumer's eight nought deciles come from exactly those, and
leaving them out would describe the jobs that kept something and call it
the jobs. No mean is printed anywhere.

**Figure 3, the gap.** What the records still claim against what the
`files/` directories measure, by file length, through
`store::directory_bytes` — the sweeper's own walk, on purpose: a report
measuring by a different rule would print a gap that was partly its own
doing. Plus the two defects only that comparison shows (a record
claiming bytes whose `files/` measures nought; a record whose bytes were
swept whose `files/` still holds some, which is a sweep killed between
the rename and the unlink), and `files.part/` and `files.gone/` measured
apart, since both are real disk outside the output budget.

**The hard constraints.**

- **No lock.** It never builds a `JobDirStore`, so it takes neither the
  store's `Mutex<Live>` nor the `flock(2)` on `hub.lock`. It opens files
  read-only, one at a time, and creates nothing. The test runs it while
  the test itself holds that `flock` — the state a live hub leaves the
  file in — and asserts the root is byte-identical afterwards. It was
  also run by hand against a hub that was serving (see *Processes*).
- **Records only.** Sizes and digests come out of `job.json`; the only
  thing read off the payload side is `metadata().len()` for figure 3. No
  output byte, no log byte, no `events.jsonl`.
- **Point-in-time**, said in its own first paragraph, with what it gives
  up for taking no lock: each record is read whole or not at all
  (ORCH-082's `rename` publication), but the figures together are not
  one instant.
- A record this build cannot read — a version it does not know, a file
  that does not parse — is **counted and named** with its path, never
  read as a job that kept nothing. A job directory with no `job.json`
  yet is counted apart as the state it is.

**Tests** (`herd-hub/tests/state_root_report.rs`, plus one in
`herd-hub/src/main.rs`):

1. `every_figure_of_the_report_is_this_fixture_to_the_byte` — a
   twelve-directory fixture holding a digest shared by three jobs, a
   digest given two lengths, a zero-byte output, a file with no digest,
   a record whose bytes were swept while its `files/` still holds
   eleven bytes, a record at version 2, and a directory with no record.
   Every structured figure asserted exactly, then the **whole** of the
   rendered text, because these figures reach this project as text
   somebody pasted and a line that stopped printing a number would go
   unnoticed. Run with the `flock` held; the root is asserted unchanged.
2. `the_report_reads_what_the_store_itself_wrote` — drives the real
   `JobDirStore` writer, two jobs sharing one file's bytes, and reports
   on what it left with the store still open and its lock still held.
   Without it, test 1 would only prove the report agrees with its own
   fixture.
3. `the_command_line_is_well_formed_and_the_serving_hub_is_unchanged` —
   clap's `debug_assert`, and that `herd-hub --state-dir X` still parses
   as the serving hub with no subcommand.

Four non-test lines elsewhere: `pub mod report;` and one `pub use` in
`herd-hub/src/lib.rs`, and six names in `herd-hub/src/store.rs` widened
from private to `pub(crate)` (`JOBS_DIR`, `RECORD_FILE`,
`KEPT_FILES_DIR`, `OUTPUT_PARTS_DIR`, `OUTPUT_GONE_DIR`,
`directory_bytes`) so the report reads the layout's own names rather
than a second copy of them. No behaviour in `store.rs` changed.

## Not done, and why

- **No threshold printed.** The brief says "below 4 we build nothing and
  write the refusal down". The tool prints the ratio and its counts and
  not the 4: no decision in `docs/decisions.md` carries that figure yet,
  and a shipped binary stating an unrecorded policy is a figure nobody
  can later change. Proposed text under *Decisions needed*.
- **No `--json`.** The figures reach us as a report somebody pastes, and
  one output is one thing to keep honest. Easy to add later.
- **No doc page.** The brief allowed no edit to `docs/decisions.md` or
  `docs/orchestration/board.md`, and `docs/status.md` is this session's
  state and not mine to rewrite. The command's `--help` and its module
  docs carry the explanation. A line in the operator-facing docs is
  worth a follow-up item.
- **`docs/coverage.md` untouched**: this answers no numbered expectation
  of the first consumer.

## Gates

All five run, in the worktree, toolchain from the flake.

`nix develop --command cargo fmt --all --check` — exit 0, no output.

`nix develop --command cargo build --workspace`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.70s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`
— exit 0:

```
    Checking herd-runner v0.3.7 (.../wt/herd-runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.57s
```

`nix develop --command ./scripts/test.sh` **on this branch — RED**, exit
101, three failures, every one of them pre-existing (see *Risks*):

```
test a_result_the_hub_cannot_write_is_kept_until_it_can ... FAILED
test result: FAILED. 17 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 31.87s
test a_drain_first_reservation_survives_a_kill_nine_on_either_side ... FAILED
test a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it ... FAILED
test result: FAILED. 18 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.37s
test suite: FAILED (cargo test exited 101)
```

The three new tests passed in that same run:

```
test tests::the_command_line_is_well_formed_and_the_serving_hub_is_unchanged ... ok
test every_figure_of_the_report_is_this_fixture_to_the_byte ... ok
test the_report_reads_what_the_store_itself_wrote ... ok
```

`nix develop --command ./scripts/test.sh` **on this branch with
`origin/real-herd-patience-timeouts` merged in** — a throwaway branch
`orch-125-verify-only` in a second worktree, never pushed, deleted after
the run — exit 0:

```
test suite: PASSED
```

Skips, from that complete run: **14, which is this host at load1
8.39** — the figure `docs/status.md` already records as "14 under load"
against "12 here at rest". They are skips, not passes (D27):

```
 14 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
```

Twelve are cgroup v2 delegation refusals:
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

The other two are the two load-conditional `flood` benches, which
decline a host over `LOAD_CEILING = 8.0`
(`herd-hub/tests/fake_hub_flood.rs:72`) because this host is shared with
a production system. They are not part of the standing skip reference and
do not appear on a quiet host, so they are the whole of the difference
between this run and a run at rest:

```
HERD-SKIPPED: flood-fifty: the host's one-minute load is Some(8.39), over this bench's ceiling of 8; the host is shared with a production system
HERD-SKIPPED: flood-two-fifty: the host's one-minute load is Some(8.39), over this bench's ceiling of 8; the host is shared with a production system
```

`./scripts/check-docs.sh` — the last gate run, on the tree holding this
file, exit 0:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

It was run once more after the commit below, on the committed tree, with
the same output.

## Risks and findings

**What the brief got wrong, and what I got wrong, in order of how much
it matters.** Items 1, 3 and 4 are corrections to the brief, which the
orchestrator has taken. Item 2 is a correction to my own first report of
this task, kept here rather than deleted because the wrong version was
reported once and a handoff that quietly dropped it would leave the
record worse than the mistake did.

1. **`real-herd-patience-timeouts` is not in `origin/main`.** The brief
   says a full run "is now green at rest". It is green only on a tree
   carrying `origin/real-herd-patience-timeouts`, whose one commit
   `ee8dec0` ("Declare the job slots three real_herd tests need") is
   **not** an ancestor of `origin/main` (`bc83717`). A branch cut from
   `origin/main` today is red on exactly the three tests that commit
   touches, which are the three hunks of that commit:

   ```
   killed_hub.rs:   a_result_the_hub_cannot_write_is_kept_until_it_can
   owner_control.rs: a_drain_first_reservation_survives_a_kill_nine_on_either_side
   owner_control.rs: a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it
   ```

   I did not work around it and changed no test or expectation; I proved
   it instead, by running the suite a second time with that branch
   merged in a throwaway worktree, where it is `test suite: PASSED`.
   **Every branch the merger takes from `origin/main` before that one
   lands will be red the same way.**
2. **My run skipped 14 because it ran under load, not because 12 was
   wrong.** This is a correction to what I first reported, and the
   corrected statement is the one the project already records: a skip
   count is **per host and per load** (D27, ORCH-070, ORCH-107;
   `docs/status.md`) — 12 on this host at rest, 14 when load1 passes 8,
   14 on CI — which is exactly why the gate compares the **list** and
   never the number (ORCH-120).

   My complete green run reported 14 at load1 8.39. The reference
   complete green run on `real-herd-patience-timeouts` reported 12 at
   rest. Both are complete, both exit 0, and both are true: the two
   extra names are `flood-fifty` and `flood-two-fifty`, the
   load-conditional benches that decline a host over `LOAD_CEILING =
   8.0` (`herd-hub/tests/fake_hub_flood.rs:72`) and never appear on a
   quiet host. My own compile is what put the host over the ceiling. So
   14 is not a property of this host, and nothing about the brief's 12
   was a floor or an early exit.

   What does stay from the first version of this note, because it is a
   separate and correct point: a run that stopped early prints "AT LEAST
   n" and yields a **floor**, never an expectation (D179). That is worth
   keeping — it is simply not what separates 12 from 14 here.
3. **`cargo build -p herdctl` alone does not compile**, on `origin/main`
   and unrelated to this change: `herdctl/src/lib.rs:1216` calls
   `tokio::time::sleep` and a single-package build resolves a tokio
   without the `time` feature. `--workspace` unifies the features and is
   green, which is why no gate sees it. `scripts/test.sh`'s prebuild
   (`-p herd-hub -p herd-runner -p herdctl`) is also fine. Worth an item
   anyway: a narrow build is the first thing someone tries.
4. **The brief's `r_file` formula is incomplete** in the three ways
   under *Done*, figure 1 — undigested files, a digest with two lengths,
   and `0/0`. Each is a real state of the live records, each would have
   printed a wrong or inflated ratio, and each is now reported apart.
5. `KeptFile`'s doc comment cites D148 for "the manifest outlives the
   bytes" at `job.rs:1189`, while `OutputsRemoved` and `OutputSummary`
   cite D152 for the removal itself. Not a defect; noting it because the
   report's own text had to pick one and picked D148 for the principle
   and D152 for the mechanism.

**Risks of this change.** Three, all small.

- The walk is `O(jobs)` file reads and, for figure 3, one `read_dir` of
  each `files/` tree. On the consumer's 20 000-job root that is of the
  order of the sweeper's own startup walk, which the hub already does —
  but it is the operator's own foreground process and not the hub's, and
  nothing caps how long it takes. If it turns out slow on their volume,
  the fix is to report figure 3 behind a flag, not to cache anything.
- `report` runs inside `#[tokio::main]`, so the walk blocks that
  runtime. Harmless — the process does nothing else and exits — but it
  is a blocking call on an async runtime and a reviewer will see it.
- The rendered text is asserted whole, so any wording change to the
  report updates a long literal in the test. That is the cost of
  catching a number that stopped being printed, and it is the cost the
  brief asked for.

## Decisions needed

1. **Write the dedup threshold down, or drop it.** The brief states
   "below 4 we build nothing and write the refusal down"; no decision
   carries that figure, so the tool prints no threshold.
   *Recommendation:* a `D-NEXT` entry the merger numbers —
   *"File-level deduplication is built only if `r_file` on the first
   consumer's hub, measured by `herd-hub report`, is 4 or more. Below
   that, the saving does not pay for a content-addressed store, a
   reference count and a garbage collector in a project whose invariant
   1 refuses generic-CI surface; the refusal is recorded and the figure
   is re-measured only if the fleet's shape changes. The consumer
   measured 2.81 over one day of what their requester received; the
   hub's own volume is the population that decides."*
   *Fallback:* leave it unrecorded until the first real measurement
   comes back, and decide with the number in hand.
2. **Who runs it, and when.** The figures only exist if the consumer
   runs the command. *Recommendation:* ask Alexa to have the operator
   run `docker compose exec hub herd-hub report` once and send the
   output whole, as text — it names its own state root and its own
   moment, and no part of it is a secret (no job id, command, label or
   path outside the state root is printed; the only paths are
   `jobs/<id>` and the root the operator already knows).
   *Fallback:* nothing reaches us and figure 1 stays a guess.
3. **Land `ee8dec0` before the next batch.** Not a question about
   *what* is built, so not Alexa's: flagging it because every branch cut
   from `origin/main` until it lands reports three red tests that are
   not its own.

## Resume point

```sh
git fetch origin && git log --oneline -1 origin/orch-125-state-root-report
# expected: c3b2d2f plus the handoff commit
```

Then, for whoever continues: the branch is ready for a batch PR; it
needs `origin/real-herd-patience-timeouts` in the same batch, or ahead
of it, for CI's `check` job to be green. Nothing in this branch depends
on that commit; the suite does.

## Processes

Started and ended by me, on 127.0.0.1 and my own state root only:

- `herd-hub` PID 3044590, `--listen 127.0.0.1:38125`, state root under
  my scratchpad. Started to prove the report runs against a hub that is
  serving and holding the `flock`: it did, exit 0, twice, once with the
  root empty and once after two records had been written through
  `herdctl submit`. Stopped with `kill -TERM 3044590`, which it took
  cleanly ("the dispatch pass in flight was over after 713ns"). Verified
  gone. No `pkill`, no kill by name.
- `herdctl` PIDs: short-lived children of my own shell
  (`runner approve`, two `submit`), all exited on their own.
- `cargo` and `nix develop` under my own `CARGO_TARGET_DIR`.

Both build trees deleted once the branch was pushed:
`/mnt/ssd_980/herd/build/orch-125` and
`/mnt/ssd_980/herd/build/orch-125-verify`. The verification worktree
`.../scratchpad/orch-125-state-report/verify` and its branch
`orch-125-verify-only` were removed too; nothing of them was pushed.
