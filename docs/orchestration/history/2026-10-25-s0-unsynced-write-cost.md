# s0-unsynced-write-cost — handoff 2026-10-25

- **Item:** none named in the brief (step S0 of the architecture plan,
  the premise under the C4 recommendation)
- **Agent:** herd-step
- **Branch / commit:** `s0-unsynced-write-cost`, based on `origin/main`
  `f507ab6`
- **PR:** none (the brief forbids one)

## Done

**The number the step was asked for: an unsynchronised waiting write
costs 70–104 µs per job, against 8.1–10.8 ms for the write the hub makes
today — x90 to x120, and ten times under the 1 ms threshold at its worst
reading. The recommendation is supported.**

Three artefacts exist on this branch:

- `herd-hub/tests/unsynced_write_cost.rs` — a new bench derived from
  `herd-hub/tests/junction_breakdown.rs` (branch
  `measure-junction-breakdown`, `8471939`). Four `#[ignore]`d cases:
  `sync_floor`, `whole_pass_three_ways`, `syncs_per_waiting_write`
  (run under `strace`) and `one_real_junction_unsynced`.
- `docs/remediation/08-s0-unsynced-write-cost.md` — the dossier, in
  English, with `F` / `H` / `I` and "what I judge wrong in the brief".
- this handoff.

**Nothing under `herd-hub/src`, `herd-runner/src`, `herd-proto/src` or
`herdctl/src` was touched.** No `#[cfg(test)]` instrumentation was
needed: `record_waiting` is a public `JobStore` method and
`durable::write_file` / `durable::append_line` are public, so both the
measured path and its synchronised control run from outside the crate.

### Method, in three lines

Three write paths, not two, so the cost of the syncs is separable from
the cost of the replica being a replica: the store's own
`record_waiting`; a replica keeping every sync (the control, which lands
on the store's figure and is identical to it under `strace`); and the
same replica with the three syncs removed, keeping the atomic rename.

### The figures

One real runner junction, release, `T` = 0, three rounds, medians:

| `N` | path | post C | per job | window |
|-----|------|--------|---------|--------|
| 200 | today | 1.619 s | 8.09 ms | 1.626 s |
| 200 | unsynced | 14.04 ms | **70.2 µs** | 19.64 ms |
| 1 000 | today | 9.912 s | 9.91 ms | 9.970 s |
| 1 000 | unsynced | 94.20 ms | **94.2 µs** | 128.96 ms |

A whole pass of `N` writes, two independent runs, per job: store 8.36 /
10.75 / 8.80 / 9.72 ms; synced replica 8.07 / 8.23 / 9.07 / 8.51 ms;
unsynced replica **70.5 / 103.9 / 71.8 / 75.8 µs**.

The baseline reproduces `06-junction-breakdown.md` to **2.9 %** at
`N` = 1 000 and **1.0 %** at `N` = 200, across two campaigns and two
processes.

### The sync count — the step's exit criterion

Counted under `strace -f -c -e trace=fsync,fdatasync`, by difference
between 201 writes and the identical corpus with no write:

| path | difference for 201 writes | per write |
|------|---------------------------|-----------|
| store | 402 `fsync`, 201 `fdatasync` | 2 + 1 |
| replica, synced | 402, 201 | 2 + 1 |
| replica, unsynced | **0, 0** | **0 + 0** |

**Zero and zero.** The criterion is met, and `06`'s count of three syncs
per `record_waiting` reproduces exactly.

### Noise, characterised before the signal

Two independent runs of the pass phase and two of the junction phase.
Seven independent measurements of the unsynced per-job cost, spanning
**70.2–103.9 µs**. The worst is 9.6 times under the threshold; the
deepest cell reproduces to 5.6 %. The baseline's own run-to-run noise
(10–29 %) is the larger of the two.

## Not done, and why

- **`T` > 0 cells.** `06` showed post C's dependence on `T` is not
  monotone and wide-spread; the decision is set by `N` and the margin is
  a factor of ten, so the closed-history cells would not move it. Named
  in the dossier.
- **Phase 3 (two junctions at once) was not rerun.** Listed as `I`-1:
  the replica cannot take the store's private `Mutex<Live>`, so a
  contention measurement through it would not be the real thing.
- **No PR, no merge, no `main`, no deployment**, per the brief.
- **`docs/orchestration/board.md` and other agents' pieces under
  `docs/remediation/` were not touched**, per the brief. The index
  `docs/remediation/README.md` was left alone too; `06` did not update
  it either.

## Gates

All four Rust gates plus `check-docs.sh`, run through
`nix develop --command`, load read before each. Load during the gate run
reached `8.33 10.51 7.60` at its peak (another workshop's build); the
gates are pass/fail and not measurements, and the measurement phases
were all run under load 5.7 or less.

```
$ nix develop --command cargo build -j 4 --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.57s

$ nix develop --command ./scripts/test.sh
test suite: PASSED
 12 assertion(s) COULD NOT RUN on this host and were skipped.

$ nix develop --command cargo clippy -j 4 --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.48s

$ nix develop --command cargo fmt --all --check
fmt: clean

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**Skips: 12, exactly the reference count** — ten for cgroup v2
delegation unavailable on this host, two for the ZFS ceiling
(`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
and `a_declared_ceiling_changes_what_the_inert_weight_line_says`). Each
is a `HERD-SKIPPED` line in the suite's output and is read as a skip,
never as a pass (D27).

## Risks and findings

1. **A premise of the brief is refuted by the code, and it is the most
   important thing here after the number.** The brief holds that nothing
   depends on `waiting_for` surviving, because E-04's figures are in
   memory (`coverage.md:21`, confirmed word for word) and a restarted
   hub rewrites every reason on its first pass (`decisions.md:8081-8083`,
   confirmed word for word). But `server.rs:1596` says of
   `why_it_waits`: "`None` for a job this hub has not judged since it
   started, **and then the record is the only answer there is**", and it
   is used as exactly that fallback at `server.rs:3336` and `:6323`,
   with a test stating the trade on purpose at `:7529`. The honest
   statement is narrower: the record's `waiting_for` is a **stale
   fallback of last resort**, read only between a hub's start and that
   hub's first pass over the job. An unsynchronised write means that
   after a *power cut* the fallback may be one cause behind, or absent.
   After a `kill -9`, nothing changes — the page cache is the kernel's.
   **This does not overturn the recommendation**; it means the contract
   wording has to describe that window rather than claim there is none.
2. **An ordering invariant to settle before shipping, not after.** D103
   (`coverage.md:70`) says `events.jsonl` may be one transition behind
   `job.json`, **never ahead**. Today the two syncs enforce it. With
   both gone, the post-crash ordering is the filesystem's: safe on ZFS
   (transaction groups commit in order), unguaranteed elsewhere. `H`-2
   in the dossier, with what would confirm it. **The filesystem of the
   server's state root should be named before a design leans on this.**
3. **Post C does not stop being the largest post.** It falls from
   99.4–99.6 % of a junction to 71–73 % of a junction seventy-seven
   times shorter. Whatever S1 is should not be written as if post C had
   gone away.
4. **What is left is not the disk.** The unsynced I/O floor is 37.4 µs
   and the measured write is 70–104 µs: the remainder is reading and
   re-serialising `job.json`. A further step aims at JSON, not at
   `fsync`.
5. **A minor hazard, widened not created.** An unsynchronised rename
   widens the window in which a `.job.json.*` temporary survives a crash
   inside a job directory; `store.rs:2928` asserts a job directory holds
   exactly five names. `H`-3.
6. **The replica does not hold `Mutex<Live>`**, which cannot be taken
   from outside the crate. Stated in the dossier, controlled for by the
   synced replica at the pass level, and left as `I`-1 for concurrency.

## Decisions needed

**One, for Alexa.** Does the contract for staged durability have to
promise that a power cut costs no `waiting_for`, or is "one stale cause
between a restart and the first pass" acceptable?

- **Recommendation:** accept it, and write the window into the E-04 row
  rather than into the code. The design already calls that fallback
  stale (`server.rs:1590-1594`), the loss is bounded by the first pass,
  and the precedent for an unsynchronised record-like value is already
  shipped in `durable::write_fixed_in_place` (`durable.rs:85-109`).
- **Fallback:** keep one sync — the record's `sync_all`, dropping the
  directory `fsync` and the event `fdatasync`. That is 2 of 3 syncs
  gone; `06`'s floors put the remaining cost near 1.19 ms per job, i.e.
  **at the threshold**, which is why it is the fallback and not the
  recommendation.

## Resume point

```sh
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/s0-unsynced
nix develop --command cargo test -p herd-hub \
  --test unsynced_write_cost --release -- \
  --ignored --nocapture --test-threads=1 whole_pass_three_ways
```

The next measurement worth making is `H`-1: the same two phases with
`TMPDIR` on the NixOS server's state root, because the unsynced figure
— unlike `06`'s — is not device-bound and should transfer, and that is
the claim a deployment would rest on.

## Processes

Every process started here was a `cargo`, `nix develop`, `strace` or
test binary in the foreground of this agent's own shell, each exited on
its own. No `pkill`, no kill by name, no PID this agent did not start.
The build tree `/mnt/ssd_980/herd/build/s0-unsynced` is off the root
disk and is removed at the end of the task.
