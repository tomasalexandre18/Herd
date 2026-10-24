# C6 bench resource guardrails — handoff 2026-10-25

- **Item:** C6 (no board id allocated; `board.md` is held by another
  agent and was not touched)
- **Agent:** herd-step
- **Branch / commit:** `bench-resource-guardrails` / 09cb434, based on
  origin/main 0a2cf73
- **PR:** none (`herd-merger` opens it)

## Done

- `scripts/bench-preflight.sh`, a read-only gate to run before any
  compile, suite run or bench. It checks four things and exits 1 with
  one sentence per failure, or 0 and "safe to start":
  1. free space on the filesystem that will hold `target/`, floor
     **10 GiB**;
  2. build output must not land on a different, smaller filesystem
     than `/mnt/ssd_980/herd/build` when that directory exists;
  3. 1-minute load average, ceiling **0.75 per core** (9.00 here);
  4. `MemAvailable`, floor **2 GiB**.
  Every threshold is named, overridable by an environment variable,
  and justified in a comment next to it. `shellcheck` 0.11.0 clean,
  `set -uo pipefail`, every `cd` tested.
- `/mnt/ssd_980/herd/build/` created, as the brief allowed. Nothing
  else outside the repository was created or modified.
- `docs/orchestration/gotchas/build-weight-off-root.md` and its row in
  the gotchas `README.md` index.
- One added bullet in `.claude/agents/herd-step.md` and one added
  paragraph in `.claude/agents/herd-reviewer.md`. **No existing
  sentence in either file was reworded**; `git diff` on
  `.claude/agents/` is 12 inserted lines and 0 deleted.

## Not done, and why

- `docs/orchestration/workflow.md` still says "at most two heavy
  agents" with nothing enforcing it, and does not mention the
  preflight. The brief named only the two role files; changing
  `workflow.md` is a separate, one-line task.
- `scripts/test.sh` still derives `HERD_SKIP_LOG` from `$root/target`
  rather than `$CARGO_TARGET_DIR`, so a worktree keeps a `target/`
  holding one small text file even under the new rule. Harmless for
  disk; left alone because that path is D27 machinery.
- No periodic reaper for abandoned build directories. The rule is
  "delete it when the task ends"; an agent that dies does not.

## Gates

```
bench-preflight.sh (self, before the heavy phase): ok, safe to start
shellcheck 0.11.0 scripts/bench-preflight.sh         : rc=0, no output
nix develop --command cargo fmt --all --check        : rc=0
nix develop --command cargo clippy --workspace \
  --all-targets --jobs 4 -- -D warnings              : rc=0
nix develop --command ./scripts/test.sh --jobs 4     : rc=0
    test suite: PASSED
    12 assertion(s) COULD NOT RUN on this host and were skipped.
./scripts/check-docs.sh                              : rc=0, docs ok
./scripts/check-orchestration.sh  : 87 passed, 0 failed
```

Skips: **12**, the expected count — 10 from cgroup v2 delegation being
unavailable to this user session, 2 from ZFS ignoring every cgroup IO
knob on `rpool/root`. They are skips, not passes (D27).

Refusal paths exercised, each one observed red:

```
real low-disk filesystem (--target-dir /mnt/hdd, 3 GiB free)  -> rc=1
build output off Herd's volume (CARGO_TARGET_DIR unset, /tmp) -> rc=1
load ceiling crossed                                          -> rc=1
memory floor crossed                                          -> rc=1
all three at once, --quiet                                    -> rc=1
unknown argument / --target-dir with no value                 -> rc=2
--help                                                        -> rc=0
run from another cwd                                          -> rc=0
```

## Risks and findings

- The load gate uses the 1-minute average, which lags: an agent that
  started compiling ten seconds ago is close to invisible to it. It
  catches sustained load, not a race between two agents starting
  together. The board, not this script, is what stops that.
- The memory floor is a floor against collateral OOM damage, not a cap
  on the build. A shell script cannot bound what rustc allocates;
  only `-j` and a cgroup can, and this user session has no cgroup v2
  delegation (the same limitation the 10 skips above report).
- Check 2 is a gate only on a host where `/mnt/ssd_980/herd/build`
  exists. Elsewhere — CI, any other machine — it is silently skipped,
  by design, so the script stays runnable on the CI runner.
- `scripts/check-orchestration.sh` line 80 tests its `cd` into the
  scratch corpus, but lines 81 to 84 do not: if that `cd` ever failed,
  `printf ... > docs/decisions.md` and `git commit -qam base` would run
  in the real repository. The one chained `cd` is guarded; the four
  commands after it are not. Not fixed here — it would change a script
  whose 87-assertion count the brief uses as a reference.
- Nothing enforces the second half of the rule. A gate refuses a bad
  start; no gate notices a `target/` nobody deleted.

## Decisions needed

- **A reaper for abandoned build directories.** Recommendation: a line
  in the orchestrator's session-start routine that deletes any
  `/mnt/ssd_980/herd/build/*` older than 48 hours whose task is not on
  the board. Fallback: leave it manual and let the preflight's
  free-space check catch the accumulation, which on a 341 GiB volume
  means about forty-five abandoned builds before anyone notices.

## Resume point

```sh
cd <worktree> && git log --oneline -2
./scripts/bench-preflight.sh          # expect: refuses, no CARGO_TARGET_DIR
```
Then `herd-merger` opens the PR for `bench-resource-guardrails`.

## Processes

`cargo` and `nix develop` children of this agent's own shells, all
exited; `rm -rf /mnt/ssd_980/herd/build/c6-guardrails` is the only
cleanup left and is done. No PID was killed, no `pkill`, no `killall`.
