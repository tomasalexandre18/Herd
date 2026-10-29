# Build weight stays off the root filesystem

- **Date:** 2026-10-24 (C6, resource guardrails).
- **Observed:** while two benches were running, `/` reached 100 %. An
  agent's `./scripts/test.sh` died on ENOSPC in the middle of a
  compilation. The orchestrator recovered 54 GiB by deleting the
  `target/` of thirty-one finished agent worktrees — sparing the one
  bench still running — and `/` went from 98 % back to 20 %.
- **Why:** agent worktrees live under `/tmp/claude-1002/…`, which is on
  `rpool/root`, 68 GiB. Each worktree carries a `target/` of 1.2 to
  5.4 GiB and thirty-two existed. `/mnt/ssd_980`, where the repository
  lives, had 341 GiB free and none of it was used. Nothing cleaned up
  after a task and nothing checked before a build. This host also runs
  the chatOnS production, so a full `/` is the consumer's incident, not
  a test failure.
- **Expected now:** two rules, both for any phase that compiles, runs
  the suite or benches.
  1. **Before:** run `./scripts/bench-preflight.sh`. It checks free
     space on the filesystem that will hold `target/`, the 1-minute
     load against the core count, host memory headroom, and — since
     2026-10-29 — the room left in the cgroup the phase will actually
     run in, read the way `herd-runner/src/room.rs` reads it. It exits
     non-zero with the sentence saying what to do. A refusal is not
     advice; a reading it could not take is printed as unavailable and
     refuses nothing.
  2. **Build output goes to Herd's own volume, one directory per
     task**, and is deleted when the task ends:

     ```sh
     export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/<task>
     # … the heavy phase …
     rm -rf "$CARGO_TARGET_DIR"
     ```

  One directory *per task*, not one shared directory: cargo takes an
  exclusive lock on its build directory, so two agents sharing one
  would serialize, and the workflow allows two heavy agents at once.
  Worse, `cargo test` releases that lock before it runs the test
  binaries, so a neighbour's relink could swap a binary out from under
  a running test. The cost is a cold cache per task and about 7 GiB
  per live task — one cold run of the four gates on 2026-10-25 left
  6.6 GiB — on a volume with 341 GiB. The scarce resource was never
  this one.
- **Verify:** `./scripts/bench-preflight.sh` refuses, naming the
  mountpoint, when `CARGO_TARGET_DIR` is unset in a worktree under
  `/tmp`; it passes once the variable points at
  `/mnt/ssd_980/herd/build/<task>`. After a task,
  `du -sh /mnt/ssd_980/herd/build/*` lists only live tasks, and
  `df -h /` stays well under 80 %.
- **Replaces:** nothing. It gives the "at most two heavy agents" rule
  of `docs/orchestration/workflow.md` something that enforces it.
