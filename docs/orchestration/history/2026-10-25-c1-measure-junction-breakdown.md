# c1-measure-junction-breakdown — handoff 2026-10-25

- **Item:** C1, the full measurement of one pass (approved by Alexa, D-2).
  No ORCH id exists for it on the board; the board was not touched.
- **Agent:** herd-step
- **Branch / commit:** `measure-junction-breakdown`, based on
  `origin/main` `0a2cf73`
- **PR:** none (`herd-merger` opens)

## Done

- `herd-hub/tests/junction_breakdown.rs`, a new bench. Six cases, all
  `#[ignore]`d, so `cargo test --workspace` does not run them. It
  measures one **real runner junction** — a hub on a loopback port and a
  real `herd_runner::run` dialling it — and splits it into the five posts
  Alexa named: historical reads, open-job reads, durable writes, lock
  wait, dispatch computation.
- `docs/remediation/06-junction-breakdown.md`, the report: the one table,
  the noise floors measured before any signal, each post swept in `N` and
  `T`, post D against a mutex-free control and again live with two
  runners, three reproducibility runs, a debug-against-release
  comparison, and the `F`/`H`/`I` grading.
- **Nothing under `herd-hub/src` was touched.** `git diff origin/main
  --stat` shows two added files and no source file. No `#[cfg(test)]`
  hook, no `pub(crate)` widening, no debt to remove.

The headline, at `N` = 1 000 open jobs and `T` = 0 closed directories:

| post | median | share |
|------|--------|-------|
| A historical reads | 15.39 ms | 0.16 % |
| B open-job reads | 9.99 ms | 0.10 % |
| C durable writes | **9.633 s** | **99.5 %** |
| D lock wait | 1.2 µs | 0.00001 % |
| E dispatch compute | 20.86 ms | 0.22 % |
| **junction window** | **9.685 s** | |

The method that made this measurable without touching the hub: the three
store-touching posts are public `JobStore` methods, so a delegating store
timestamps them from outside; and because `state.dispatching`
(`server.rs:2493`) is taken *after* `collections_owed` and held past the
last `record_waiting`, the one gap before the first `unfinished_jobs` is
lock wait and every later gap is computation. A + B + C + D + E is the
window exactly.

## Not done, and why

- **The second full phase-2 run was abandoned mid-way.** A host backup
  (`borgbackup create`) drove `/proc/loadavg` from 2.9 to **35.7**, above
  the guard this task works under. The run was stopped, its two completed
  cells kept and labelled, and a third run on the `T` = 0 cells was made
  once the host was quiet. Reproducibility is therefore stated from three
  runs of the `T` = 0 cells, not two of the full grid.
- **Post C below `N` = 200 is not a figure and is not reported as one.**
  189 ms, 362 ms, 753 ms on three runs of the same cell.
- **No second filesystem.** `/mnt/ssd_980` is not writable by this user,
  so post C was measured on ZFS only. `/dev/shm` would have given a
  zero-durability control and was not used for want of time; it is the
  cheapest next experiment.

## Gates

All four Rust gates plus `check-docs.sh`, run at the end, load checked
before each.

```
cargo fmt --all --check            -> clean (after one `cargo fmt --all`)
cargo clippy --workspace --all-targets -j 4 -- -D warnings
                                   -> Finished `dev` profile, exit 0
./scripts/test.sh -j 4             -> test suite: PASSED, exit 0
./scripts/check-docs.sh            -> docs ok, exit 0
   (note: D170 is a gap in docs/decisions.md; gaps stay, D148)
cargo build --workspace            -> covered by clippy --all-targets and
                                      by test.sh, both green
```

**Skips: 12, which is the reference count.** Copied as skips, never
counted as passes (D27). Ten are cgroup v2 delegation
(`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`) and two are the ZFS
ceiling
(`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`). Full
output in the campaign's raw logs, listed under "Resume point".

## Risks and findings

1. **The brief says four `fsync` per `record_waiting`; it is three.** Two
   `fsync` (the record file's `sync_all`, then the directory's) and one
   `fdatasync` (the `events.jsonl` append). Counted under `strace` with
   the corpus cancelled out, and agreeing with the measured 2.39 ms
   floor.
2. **The brief's post A constant is ~25 % pessimistic.** 8.3–10.0 µs per
   directory in release, 186 ms at `T` = 20 000, not ~11 µs and 247 ms.
3. **The brief's "both reads: 13 ms at `N` = 1 000" is low** — 18.8 ms
   solo. The conclusion is unchanged and strengthened.
4. **Post D is not one of five comparable posts.** For a single junction
   it is 600 ns–1.6 µs, six to seven orders of magnitude below post C. It
   becomes a cost only when two junctions meet, and then it equals the
   earlier junction's post C: 8.87 s behind a pass of 1 000. The honest
   description of a junction is **four posts and a coupling term**.
5. **The reviewer's 0.3–4.0 s spread was method, not host.** Timing from
   `herd_runner::run` being spawned includes the runner's confinement
   setup; and polling `store.job()` every 1 ms to detect settlement takes
   the store's `Mutex<Live>` against the writes being measured. Removing
   both gives `iqr/med` 5.2 % at `N` = 1 000 and 1.8 % between
   independent runs.
6. **The store's `Mutex<Live>` serialises `collections_owed` completely**
   — `C` callers cost `C` times one call, while the same I/O without the
   mutex stays flat on twelve threads, so 85–86 % of a contended call is
   blocking. Product-side observation, outside this task's scope, no
   change made.
7. **Post E in a live junction is 15–19x the public queue work**, about
   21 µs per queued job. The hub prints one `stays queued` line per job
   (`server.rs:2618`); whether that or `matcher::place` dominates is an
   `H`, and it is the cheapest open question left.
8. **Every figure is from one host, one filesystem, one device**
   (ZFS mirror of two SATA SSDs, no SLOG). Post C is device-bound. The
   shape transfers; the constant does not.

## Decisions needed

**One, for `herd-merger` rather than Alexa.** `origin/main` moved while
this was being measured (C0 merged: `docs/remediation/` now holds 00,
03a, 03b, 03c, 04a, 04b, 05 and a `README.md` index). This branch is
based on `0a2cf73` as the brief required and merges cleanly — no file is
touched on both sides. But `docs/remediation/README.md` indexes the
dossier and does not list `05` or `06`. **Recommendation:** the merger
adds the `06` row (and `05`, if it is also missing) when integrating,
since the brief forbade this task from editing another author's file
under `docs/remediation/`. **Fallback:** leave it; the gap is visible
and the README already says a gap means "not written", which would then
be wrong rather than merely incomplete.

Also for the merger: `06` corrects five published figures in `04a` and
`05` (listed in `06` under "These corrections land on the dossier"). It
does not edit those files. Whether they are amended or left with `06` as
the correction of record is the merger's call.

**Nothing for Alexa.** C1 was a measurement and it is complete; the
architecture decision it was blocking now has a whole event under it.

If the orchestrator wants one more experiment before that decision, the
recommendation is the `/dev/shm` control (phase 1d with `TMPDIR` on
tmpfs): it separates `record_waiting`'s code path from its durability
cost in about two minutes, and it is the only cheap thing left that could
change how the decision reads. Fallback: decide on what is here, since
post C dominates by two orders of magnitude on any plausible device.

## Resume point

```sh
git checkout measure-junction-breakdown
nix develop --command cargo test -p herd-hub --test junction_breakdown \
  --release -- --ignored --nocapture --test-threads=1
```

Raw output of every phase, outside the repo and not committed, in this
session's scratchpad under `c1-out/` (full path in the task log):
`phase0-noise.txt`, `phase1-posts.txt`, `phase1f-lock.txt`,
`phase2-junction.txt`, `phase2-junction-b-partial.txt`,
`phase2-junction-b.txt`, `phase2-debug.txt`, `phase3-two-runners.txt`,
`syncs-strace.txt`, `gate-clippy.txt`, `gate-test.txt`,
`gate-check-docs.txt`.

## Processes

Started and ended by this task, all in its own scratchpad worktree:

- `cargo`/`rustc` builds and `cargo test` runs under `nix develop`, each
  waited on to completion.
- One `cargo test` process (the second phase-2 run) was stopped
  deliberately when host load reached 35.7; it was killed through its own
  background-task handle, by PID, never by name. No `pkill`, no
  `killall`.
- The hubs and runners each case starts are in-process tokio tasks, all
  aborted by the case that started them. No stray `herd-hub` or
  `herd-runner` process was left: checked by `ps` on the exact binary
  path after the run.
- `target/` was removed at the end of the task.
