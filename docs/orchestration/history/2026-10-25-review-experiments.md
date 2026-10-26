# review-experiments — handoff 2026-10-25

- **Item:** adversarial review of `04a` and `04b` (no ORCH id in the brief)
- **Agent:** herd-reviewer
- **Branch / commit:** `review-experiments`, based on `origin/main` `0a2cf73`
- **PR:** none, and none asked for

## Done

- `docs/remediation/05-experiments-review.md` — the review. English, the
  dossier's F/H/I standard, a ranked list separating what is proved from
  what still needs a measurement, and a strategy recommendation.
- `herd-hub/tests/review_pass_cost.rs` — six `#[ignore]`d `measure`-style
  cases, added so that nothing in the review is an assertion made from
  reading alone. They assert no product behaviour.
- No production file touched: `herd-hub/src`, `herd-runner/src`,
  `herd-proto/src` and `herdctl/src` are byte-identical to `0a2cf73`.
- `docs/orchestration/board.md` and the other authors' files under
  `docs/remediation/` were not touched.

## The five things the brief asked, in one line each

1. **The `d` convention does not decide 04a's result.** The 815/33 ms
   comparison is at `d` = 0. The conclusion survives the per-call
   convention; only recommendation 3 depends on the choice.
2. **The hot path's write half is missing, and the ban on `herd-hub/src`
   is not why.** `record_waiting` is a public trait method. 1 000 of them
   cost 8.3–9.2 s against 13 ms of reads. The ratio reverses.
3. **Recommendation 1 as worded cannot be built** (owed collections are on
   finished jobs; measured 0 of 1), and **recommendation 4 is refuted**
   (4.59x amplification with no `dispatching` lock in the process).
4. **04b is right about `ran_commit`; the retraction was not premature.**
   One narrowing: a rolled-back hub reads the field and erases it on its
   next write of that record — measured.
5. **12 skips on this host, 10 cgroup-delegation and 2 ZFS ceiling**, from
   a real run, 12 lines and 12 distinct names. `coverage.md` is right;
   D151's 13 is not reproducible and no skip site was deleted since.

## Not done, and why

- 04b's two benches were read, not re-run; its `file:line` citations were
  all re-checked and are exact.
- No clean split of a live junction's wall clock: three rounds at each `T`
  spread 0.3–4.0 s on this host. Named as item 11 of the review's list,
  and it is ten lines in an existing bench.
- Nothing on the deployment, nothing on the consumer's instance (D161).

## Gates

```
cargo fmt --all --check                                    exit 0
cargo clippy --workspace --all-targets -j 4 -- -D warnings exit 0
./scripts/test.sh                                          exit 0
  test suite: PASSED
  12 assertion(s) COULD NOT RUN on this host and were skipped
./scripts/check-docs.sh                                    exit 0
  note: D170 is a gap in docs/decisions.md (held by an open PR, or lost)
```

## Load and disk

`df -h /` and `/proc/loadavg` read before every heavy phase. Disk went
from 15 % to 18 % of root and the worktree's `target/` is removed. The
one-minute load peaked at **10.1** during `./scripts/test.sh`, which sets
its own parallelism; every phase I controlled ran `-j 4` and stayed under
5. The five-minute load never passed 5.

## Processes

Only `cargo`, `cargo test` and the test binaries this task started, all in
its own worktree under
`scratchpad/review-exp/`. Each exited on its own. One `herd-runner` and
one hub ran in-process inside the bench binaries and were aborted with
them. No PID was signalled, nothing was killed by name, `pkill` and
`killall` were not used.
