# experiment-cost-surface — handoff 2026-10-24

- **Item:** experiment 6.1 of the remediation file (no ORCH id given in
  the brief)
- **Agent:** herd-step
- **Branch / commit:** `experiment-cost-surface`, based on origin/main
  `0a2cf73`
- **PR:** none (herd-merger opens)

## Done

- `herd-hub/tests/cost_surface.rs` — four `#[ignore]`d measurement cases
  and a delegating `JobStore` (`CostlyStore`, built from the shape of
  `StoreThatHoldsTheClaimOpen`, `server.rs:6928`) that charges a
  configurable latency per **record** access and counts every call.
- `docs/remediation/04a-cost-surface.md` — the plan and why that plan, the
  raw figures, the surface, which term dominates, and what it implies.
- No production file touched: `herd-hub/src`, `herd-runner/src`,
  `herd-proto/src` and `herdctl/src` are byte-identical to `0a2cf73`.
- `docs/orchestration/board.md` untouched, as the brief required.

The answer: **`T` dominates.** At both axes' maxima and four concurrent
redials on a four-worker runtime, `T` = 20 000 buys an 815 ms worst turn
and `N` = 1 000 buys 33 ms — 25x, and `T` is the one that only grows.
99 % of a runner junction's record reads at `T` = 5 000 are the walk of
closed job directories (measured against a live hub).

## Not done, and why

- `d` was not measured against a real alternative backend. The `d` axis is
  a model: `CostlyStore` charges per record, and the extrapolation to a
  remote store is marked **H** in the document.
- `sweep_outputs` (`store.rs:642`), the third O(`T`) walk, and store
  startup (`store.rs:439` plus recovery), also O(`T`). Out of the brief's
  three hot paths; named in the document as **I**.
- The arithmetic of the pass after the two reads — `queue::order`,
  `matcher::holdings`, `matcher::reserved`, `matcher::reservations`, all
  O(`N`) — is not in the junction figure, so the `N` term reported is a
  floor. Reaching it needs production code or a hub driven to a real pass,
  and the conclusion does not turn on it: it would make `N` larger, and
  `N` is already 25x behind.
- The real `T` on the deployed hub: never touched (nothing in this task
  went near the server).

## Gates

```
cargo fmt --all --check                                    exit 0
./scripts/check-docs.sh                                    exit 0
  note: D170 is a gap in docs/decisions.md (held by an open PR, or lost)
  docs ok
cargo clippy --workspace --all-targets -j 4 -- -D warnings exit 0
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.06s
./scripts/test.sh                                          exit 0
  test suite: PASSED
  44 test binaries, no FAILED result
```

Clippy's coverage of the new file was not taken on trust: a deliberate
`clippy::len_zero` was inserted into `herd-hub/tests/cost_surface.rs`, the
lint fired on line 837, and the probe was removed.

**12 skips**, the host's reference figure: 10 for cgroup v2 delegation
being unavailable under this session's scope, 2 for `io.max` on ZFS
(`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
and `a_declared_ceiling_changes_what_the_inert_weight_line_says`). A green
run does not mean those were checked (D27).

> **Corrected at integration, 2026-10-25, by `herd-merger`.** This
> paragraph originally named `an_io_weight_that_cannot_bite_is_said_out_loud`
> as the second ZFS skip. It is not one: its reason begins *"cgroup v2
> delegation is unavailable"*, so it is the tenth delegation skip, and it
> lives in `herd-runner/tests/confinement.rs:1337`, not in
> `herd-runner/src/confine.rs`. The two ZFS skips are the pair now named
> above, both in `herd-runner/src/confine.rs` (`:1118`, `:1220`).
> `05-experiments-review.md` §6 reached the same conclusion by reading;
> this correction was made from a measured run on this host, 2026-10-25,
> on the tree carrying all seven C0 branches — `wc -l`, `sort -u` and the
> distinct-name count of `target/herd-skips.txt` all give **12**, and
> `grep -c 'cgroup v2 delegation is unavailable'` gives **10**. The **12**
> total and the **10 / 2** split this paragraph states are both confirmed;
> only the one name was wrong.

The first attempt at `./scripts/test.sh` died mid-compile with ENOSPC:
the host's root was at 100 %. It was not this task's doing — the peak
footprint here was ~715 MB — and the rerun on a freed disk passed.

## Risks and findings

- **The brief's `d` axis is underspecified, and the choice decides the
  answer.** "Sleeps `d` before each read and each write" at trait
  granularity makes `d` a constant: `unfinished_jobs` is one call that
  reads `N` records, `collections_owed` one call that reads `N + T`, so
  `d` = 20 ms would add 40 ms to a junction and the axis would be inert by
  construction. The bench charges per record instead and says so.
- **The brief's own hypothesis is confirmed, and its framing is the right
  one.** "If it is `T`, the answer is probably not an architecture but
  stopping walking the whole volume at every redial" — that is what the
  figures say.
- **`W` = 4 is worse than `W` = 2**, measured (814 ms against 474 ms at
  the same corpus). More workers means more synchronous callers can pile
  in before the reactor is wedged.
- **The amplifier is `state.dispatching`**, a std mutex held across the
  whole pass (`server.rs:2493`) and taken from async tasks: `k` concurrent
  callers become `k` blocked worker threads, and case 3 reproduces the
  arithmetic to within 0.3 %.
- **Nothing ever deletes a job directory** (`herd-hub/src/store.rs`: the
  only `remove_dir_all` on a job path is the part directory, `:1981`), so
  `T` is monotone in the number of jobs the herd has ever run.
- Figures are warm-cache on ZFS. A cold cache is the one direction in
  which `T` is certainly worse than reported.
- Debug is ~6x release and the hub ships release
  (`nix/workspace.nix`). Both columns are in the document so neither is
  quoted by accident.

## Decisions needed

**Question:** fix the `T` walk before, or as part of, taking persistence
out of the reactor?

**Recommendation:** before, and separately. Make `collections_owed` ask
only the jobs that can owe bytes — the open ones the hub already holds, or
a small index — instead of listing `jobs/`. It is local to `store.rs`,
breaks no protocol, and takes the measured worst turn from 815 ms to
~33 ms. The reactor refactor stays correct and becomes a milestone rather
than an incident.

**Fallback:** if the reactor refactor is already committed to, do it with
the `T` walk removed first anyway — moving a 20 s operation (at `d` = 1 ms
and `T` = 20 000) onto a blocking pool hides it rather than removing it.

## Resume point

```sh
cargo test -p herd-hub --test cost_surface --release -- \
  --ignored --nocapture --test-threads=1
```

Then, if the recommendation above is taken: a `collections_owed` that does
not list `jobs/`, with case 4 of this bench as its regression test — it
counts the record accesses one junction makes and would drop from 5 103 to
~100 at `N` = 50, `T` = 5 000.

## Processes

Only `cargo`, `cargo test` and the test binaries this task started, all in
its own worktree; each exited on its own. One `herd-runner` ran in-process
inside case 4's test binary and was aborted with it. No PID was signalled,
nothing was killed by name, and nothing outside
`scratchpad/exp-61/` was touched.
