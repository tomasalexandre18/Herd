# real-herd-patience-timeouts — handoff 2026-10-28

- **Item:** none given in the brief (the three `real_herd` 30 s `PATIENCE`
  timeouts on this host)
- **Agent:** herd-step
- **Branch / commit:** `real-herd-patience-timeouts` / see below, based on
  `origin/main` bc83717
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

**The cause, measured.** The three tests do not wait on a lock, a port, a
state root or a lost wakeup. They wait on a job the hub is **right** not
to claim, because the runner they started announced one job slot.

`herd_runner::jobs_at_once` (`herd-runner/src/confine.rs:284`) derives the
ceiling: `most = (lent − floor) / per-job cap`, floor
`= clamp(lent/5, 1 GiB, 8 GiB)` (`herd-proto/src/placement.rs:256`),
per-job cap 2 GiB by default. `lent` is **not** `MemTotal`: D119/D79 read
the tightest `memory.max`/`memory.high` enclosing the runner's cgroup and
subtract what that cgroup already holds, once, at startup
(`herd-runner/src/room.rs`).

On this workstation the runner stands under `/chatons.slice`, whose
`memory.high` is 18 GiB. At the moment of the failing runs it already held
13.31 GiB, so the runner announced the honest remainder:

```
herd-runner: announcing 4.6 GiB of memory rather than 31.1 GiB, because
/proc/self/cgroup is enclosed by memory.high 18 GiB on /chatons.slice,
which already holds 13.3 GiB, so the room here is 4.6 GiB and not what
/proc/meminfo reports
herd-runner: this machine lends 4.6 GiB and keeps 1 GiB as its own floor,
and caps every job at 2 GiB, so this runner holds at most 1 job(s) at once
```

and the hub said, every time, exactly why the second job waited:

```
herd-hub: j0000000003 stays queued: ... it is waiting for the-real-runner:
on the-real-runner, 1 job(s) already out there and it said it holds at
most 1 at once, so it takes nothing more until one of them ends
```

All three tests need a **second** concurrent job and none of them says so:

- `a_result_the_hub_cannot_write_is_kept_until_it_can`: the first job's
  outcome cannot be written, so the hub still counts it as out there —
  correctly — and the sibling `herdctl run` that proves the link survived
  is never claimed. 30 s at `mod.rs:774`.
- `a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it` and
  `a_drain_first_reservation_survives_a_kill_nine_on_either_side`: a build
  holds the slot, and the metal job has to reach the *drain-first*
  judgement while it does. At one slot the concurrency ceiling answers
  first and `being emptied` is never written. 30 s at `mod.rs:926`.

**Proved on demand, four ways.** The arithmetic is the switch:

| knob (environment only, no code change) | slots | result |
|---|---|---|
| none (default 2 GiB cap) | 1 | red, 3 × 30 s |
| `HERD_JOB_MEMORY_MAX=512MiB` | 7 | green in 2.2–2.7 s |
| `HERD_JOB_MEMORY_MAX=1800MiB` | 2 | green in 3 s |
| `HERD_JOB_MEMORY_MAX=1900MiB` | 1 | red, 61 s for two tests |
| `HERD_HOST_MEMORY=31GiB` | 1 | red — the cgroup clamp wins, rightly |

The threshold is exact: two slots need
`room − clamp(room/5, 1 GiB, 8 GiB) ≥ 4 GiB`, i.e. **room ≥ 5 GiB**, i.e.
`/chatons.slice` holding **≤ 13 GiB** of its 18 GiB. At the failing runs
it held 13.31 GiB. An hour later, a sibling agent's 9.8 GB `target/`
having gone, it held 11.75 GiB — room 6.25 GiB, two slots — and the same
unfixed binaries would have passed. That is the whole of the
"intermittent" behaviour.

**The fix.** `herd-hub/tests/real_herd/mod.rs` gains one constant,
`ROOM_FOR_A_SECOND_JOB = "--job-memory-max=512MiB"`, with the arithmetic
and the measurement written down, and the three tests pass it to the
runner they start — a declared fixture, the same move
`a_log_survives_twelve_killed_hubs_whole` makes with `--redial-cap 1`.
Nothing about what they assert changes. Red before (32 s, 30 s), green
after (10.7 s, 9.1 s).

Diff: 3 files, +60/−3. No product code touched.

## Not done, and why

- No change to the runner, the hub or `scripts/test.sh`. The product is
  correct here and says so in words; see *Risks and findings* for the one
  product question this turned up, which is Alexa's.
- `RUST_TEST_THREADS` was used as a diagnostic only, never committed.
- `docs/decisions.md` and `docs/orchestration/board.md` untouched, as
  briefed; proposed text under *Decisions needed*.

## Gates

```
$ nix develop --command cargo build --workspace
build exit=0
$ nix develop --command cargo fmt --all --check
fmt exit=0            (no output)
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
clippy exit=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.58s
$ nix develop --command ./scripts/test.sh
test.sh exit=0 wall=170s
55 `test result:` lines, 0 of them FAILED
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost);
gaps stay (D148)
docs ok
```

Skips: 12, each a cgroup-v2-delegation refusal on this host, as at rest.
Named: `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
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
Counted as skips, never as passes (D27).

## Risks and findings

**Where the brief was wrong.** Three premises died on measurement, and
each death was cheap:

1. *"All three pass when their targets run alone."* They do not. Running
   the compiled binaries straight from `target/` — `killed_hub` then
   `owner_control`, nothing else on the host — reproduced all three, same
   lines, same durations: 31.91 s and 30.36 s. The earlier green "alone"
   runs were green because the slice held less at that minute, not
   because they ran alone.
2. *"Only when many `real_herd` tests run concurrently."* Each failing
   test reproduces **on its own, `--test-threads=1`, `--exact`**: 32 s,
   30 s, 31 s. The rung experiment the brief asked for (2 → 4 → 12
   binaries in parallel) was never needed; the first rung is one test on
   one thread. Nothing about `libtest` parallelism, cargo's binary
   ordering, `flock`, `free_port`, `TMPDIR` or a lost wakeup is involved.
3. *"A `memory.peak` of 18 270 MiB on our cgroup scope is not evidence."*
   It is the mechanism, read through the wrong lens. Not as pressure —
   the brief's refutation of pressure stands, there was none — but as the
   **input to the capacity a runner announces**. `memory.current` of the
   enclosing slice is what `room.rs` subtracts, and page cache counts in
   it. The build artefacts the brief dismissed as "just page cache" are
   precisely what shrank the announced machine from 11 slots to 1.

**Product or tests?** The failures are the **tests'**: three fixtures
depend on an undeclared host property (a runner that holds ≥ 2 jobs) and,
when the host does not have it, spend `PATIENCE` instead of saying so.
The product did everything right: it announced less than the machine on
purpose (D119, so a slice is not OOM-killed while the hub believes it was
careful), it refused to lend more when `--host-memory` asked it to, it
queued the second job, and it printed the reason in both logs. The
diagnostic was already adequate — `holds at most 1 job(s) at once` was in
the failure output all along.

**One product question, which is not a defect and not mine.** `lent` is
read once at startup and never revised (`room.rs`, deliberate, D79: a
figure that moves between judgement and claim would be the overcommit
E-32 forbids). Consequence for the consumer: a runner started while its
slice holds reclaimable page cache lends a fraction of its machine **for
its whole life**, and the only cure is a restart. On a host that runs a
hub and a runner in docker compose under a slice with `memory.high`, a
nightly that filled the cache can halve the fleet until someone restarts
it. The trade-off is written down and was chosen; what is missing is that
nobody is *told* the figure is a startup snapshot. Reported, not touched.

**Residual risk of the fix.** A 512 MiB per-job cap is enforced on this
host as an `RLIMIT_AS` fallback (no cgroup delegation). The jobs in the
three tests are `printf`, `sleep` and a `until [ -e … ]` shell loop, and
they ran green; a test later added to one of these three that needs real
memory in a job would have to raise its own cap. Said in the constant's
doc comment.

**Scope not closed.** Only the three tests that are red today are fixed.
The class is open: any `real_herd` test that comes to need two concurrent
jobs will inherit the same silent dependency. The whole suite is green at
**one** slot with this change (the 170 s run above ran at room 4.7–6 GiB),
so nothing else needs it today.

## Decisions needed

1. **Should the harness declare the per-job cap for every `real_herd`
   runner, not only these three?** — *Recommendation:* no, not now. It
   would change the announced capacity of ~30 test binaries at once, some
   of which assert admission figures (`admission.rs`,
   `announced_job_ceiling.rs`), to close a class that costs two words per
   test when it appears. *Fallback if Alexa wants the class closed:* a
   `job_cap_unless_given` helper beside `cadence_unless_given`, defaulting
   to 512 MiB, and one full-suite run to see what moves.

2. **Proposed decision entry, for `herd-merger` to number as `D-NEXT`:**

   > **D-NEXT (2026-10-28) — how many jobs a test's runner holds is a
   > fixture, not the host's to decide.** A `real_herd` test whose proof
   > needs a runner to hold a second job while the first is out there
   > declares it, with `real_herd::ROOM_FOR_A_SECOND_JOB`
   > (`--job-memory-max=512MiB`). It is not optional politeness: the
   > ceiling is `(lent − floor) / per-job cap`, and `lent` is the room
   > under the tightest `memory.max`/`memory.high` enclosing the runner
   > less what that cgroup already holds, read once at startup (D119,
   > D79). Page cache counts in that figure, so on the project's own
   > workstation — `/chatons.slice`, `memory.high` 18 GiB — the ceiling
   > fell to one job whenever the slice held more than 13 GiB, and three
   > tests that need two spent 30 s each waiting for a job the hub was
   > right not to claim. Measured 2026-10-28: red at a 1 900 MiB cap,
   > green at 1 800 MiB, same tree, same minute. A test that waits on a
   > machine's capacity must state the capacity it needs.

3. **Not asked, but worth Alexa's word later:** should a runner say, in
   its startup report, that the room it announced is a snapshot taken at
   startup and will not rise when its slice empties? One sentence in
   `room.rs`'s report, nothing else. *Recommendation:* yes, as a separate
   task; it is the one thing the consumer could be surprised by.

## Resume point

Nothing is pending on this branch. For the merger:

```sh
git log --oneline origin/main..real-herd-patience-timeouts
```

To re-prove the cause on any host at any time, no code change needed:

```sh
cd <worktree> && export CARGO_TARGET_DIR=<dir>
nix develop --command cargo test -p herd-hub --test owner_control --no-run
HERD_JOB_MEMORY_MAX=1900MiB <dir>/debug/deps/owner_control-<hash> \
  --test-threads=1 --exact \
  a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it   # red
HERD_JOB_MEMORY_MAX=1800MiB <dir>/debug/deps/owner_control-<hash> \
  --test-threads=1 --exact \
  a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it   # green
```

## Processes

Started and ended by this agent, all in its own worktree and
`CARGO_TARGET_DIR`: `nix develop` shells running `cargo`, the test
binaries `killed_hub-a8c909d89fcc51f9` and
`owner_control-1a6e7a66cfed38c5`, one `herd-runner` started by hand to
check that clap takes `--job-memory-max=512MiB` (pid reaped by its own
`timeout 20`), and the hubs, runners and `herdctl` processes those tests
spawn, each reaped by the harness's `kill_on_drop`. One background
`bash` of this agent's own (a mistyped heredoc that read stdin) was
stopped by its task id. No `pkill`, no `killall`, no kill by name, no pid
this agent did not start. Nothing outside the worktree, the scratchpad
and `CARGO_TARGET_DIR` was written.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/real-herd-patience` is deleted
after the push.
