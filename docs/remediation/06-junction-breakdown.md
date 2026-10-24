# 06 — The full cost of one pass, broken into five

What one **runner junction** costs the hub, separated into the five posts
Alexa asked for: historical reads, open-job reads, durable writes, lock
wait, and the dispatch computation. One real event, one table, with the
noise characterised before the signal and the experimental limits named.

This file measures. It decides nothing, and it recommends nothing: C1
exists so that the architecture decision has a whole event under it
instead of half of one.

## The one table

One real runner junction, release build, five rounds, medians. `N` is the
open jobs in the queue, `T` the closed job directories still under
`jobs/`. The window is the whole event, and the five posts sum to it by
construction.

| post | what it is | `N`=50, `T`=0 | `N`=1 000, `T`=0 | `N`=50, `T`=20 000 | `N`=1 000, `T`=20 000 |
|------|------------|---------------|------------------|--------------------|-----------------------|
| A | historical reads | 502 µs | 15.4 ms | 198 ms | 203 ms |
| B | open-job reads | 435 µs | 9.99 ms | 495 µs | 9.84 ms |
| C | durable writes | 189 ms | **9.633 s** | 420 ms | **11.458 s** |
| D | lock wait | 614 ns | 1.2 µs | 1.4 µs | 1.6 µs |
| E | dispatch compute | 1.10 ms | 20.9 ms | 1.35 ms | 21.7 ms |
| | **junction window** | **191 ms** | **9.685 s** | **614 ms** | **11.692 s** |

In one sentence: **durable writing is 68–99.5 % of a junction, the two
reads together are under 0.3 % once the queue is deep, the dispatch
computation is a fifth of a percent, and lock wait is zero until a second
runner dials — at which point it is the whole of the first junction's
writes.**

The rest of this file is how those figures were obtained, how far they
reproduce, and what they do not say. Two cells are left out of this table
for room and are in phase 2 below.

## Debt this leaves behind: none

**Nothing under `herd-hub/src` was touched.** `git diff origin/main --stat`
on this branch shows one added test file and this document, and no source
file. There is no `#[cfg(test)]` hook to remove later and no behaviour that
differs between a measured build and a shipped one.

That was possible because the three store-touching posts are **public**
`JobStore` methods, so a delegating store sees them from outside, and the
two remaining posts are exactly the **gaps between** those calls:

```text
server.rs:2415  ask_for_the_files_still_owed -> collections_owed()   (A)
                .............. gap ..............                    (D)
                    dispatch_queued_jobs takes `dispatching` (:2493)
server.rs:2510  dispatch_a_pass -> unfinished_jobs()                 (B)
                .............. gaps .............                    (E)
                    settle_readings, queue::order, matcher::holdings,
                    matcher::reserved, matcher::reservations,
                    matcher::place, queue::waiting_sentence_behind
server.rs:2640  say_it_waits -> record_waiting() x k                 (C)
```

The pass holds `state.dispatching` from before the first `unfinished_jobs`
until after the last `record_waiting`. So **every gap after that first
queue read is work, and the one gap before it is waiting** — which is the
whole separation, obtained without a probe inside the hub.

`A + B + C + D + E` is the junction window exactly, by construction: every
instant in the window is either inside a traced call or between two of
them.

## How to reproduce

The bench is `herd-hub/tests/junction_breakdown.rs`. Every case is
`#[ignore]`d, so `cargo test --workspace` does not run it.

```sh
nix develop --command cargo test -p herd-hub --test junction_breakdown \
  --release -- --ignored --nocapture --test-threads=1
```

One phase at a time, which is how the figures below were taken (the name
of the case is the filter):

```sh
# phase 0, the floors                              ~12 s
... --release -- --ignored --nocapture --test-threads=1 noise_floor
# phase 1, each post alone, swept in N and T       ~384 s
... --release -- --ignored --nocapture --test-threads=1 each_post_alone
# phase 1f, post D against a mutex-free control    ~40 s
... --ignored --nocapture --test-threads=1 lock_wait_under_contention
# phase 2, the five posts on a real junction
... --ignored --nocapture --test-threads=1 one_real_junction_broken_down
# phase 3, two junctions at once: post D, live
... --ignored --nocapture --test-threads=1 two_runners_at_once
```

`HERD_ROUNDS=<n>` sets how many junctions each cell of phases 2 and 3
measures; the default is 5.

**`--test-threads=1` is not a detail.** Two cases of this file running at
once would contend for the disk and for the page cache, and post D's whole
point is that contention is the measurement.

## The host, and what it costs the figures

| what | this host |
|------|-----------|
| CPU | Intel i5-10600K, 6 cores / 12 threads, 4.1 GHz |
| memory | 31 GiB |
| kernel | 6.18.46 |
| state root | `$TMPDIR` = `/tmp`, on `rpool/root` |
| filesystem | ZFS, `sync=standard`, `compression=zstd`, `recordsize=128K` |
| device | mirror of two SATA SSDs (TOSHIBA 128 GB), **no SLOG** |
| build | `--release` (`nix/workspace.nix` ships release) |

Three limits follow from that row of hardware, and they bound every figure
below:

1. **Post C is device-bound, and this device is slow at `fsync`.** A SATA
   SSD mirror with no separate log device costs ~1.2 ms per `fsync`. An
   NVMe would cost roughly a tenth of that. Post C's *shape* (how it grows
   with `N`) transfers to another host; its *absolute* figure does not.
2. **The page cache cannot be dropped without root**, so "cold" here means
   "written by this process a moment ago" — the warmest cold there is. A
   hub restarted onto an old state root is colder than anything measured
   here. Measured first-touch penalty: x1.23 at `T` = 1 000, x1.13 at
   `T` = 20 000, so on *this* host the page cache is not where the cost is.
3. **The host is shared.** `/proc/loadavg` was read before each phase and
   is quoted with it; no phase ran above 4.1. A campaign run at load 9
   would be measuring the other tenants.

## Phase 0 — the noise, before any signal

Measured first, so that no post below is reported as a figure when it is
smaller than the floor it sits on. `iqr/med` is the interquartile range
over the median: the one number that says whether a post is a figure or a
guess.

Load before the phase: `0.45 0.53 0.75`. Free on `/`: 56 GiB.

| floor | median | iqr/med | min | max | n |
|-------|--------|---------|-----|-----|---|
| `Instant::now()` pair | 14 ns | 0.0 % | 13 ns | 45 ns | 200 |
| empty `unfinished_jobs`, bare | 31 ns | 3.2 % | 30 ns | 64 ns | 200 |
| the same through the trace | 89 ns | 5.6 % | 86 ns | 10.7 µs | 200 |
| `create`+`write`+`sync_all` | 1.19 ms | 2.5 % | 1.13 ms | 4.66 ms | 100 |
| `sync_dir` (`open`+`sync_all`) | 6.6 µs | 18.3 % | 5.1 µs | 13.0 µs | 100 |
| `append`+`sync_data` | 1.20 ms | 3.0 % | 1.16 ms | 4.50 ms | 100 |
| one warm record read | 8.0 µs | 1.6 % | 7.9 µs | 33.3 µs | 200 |
| uncontended mutex round trip | 24 ns | 0.0 % | 23 ns | 32 ns | 200 |

Three things this settles:

- **The instrument costs 58 ns per store call** (89 − 31). A junction at
  `N` = 1 000 makes about a thousand of them, so the trace adds ~58 µs to a
  window measured in seconds: four orders of magnitude below the signal.
- **The `fsync` floor under one `record_waiting` is 2.39 ms** on this
  host (1.19 + 0.0066 + 1.20). Anything post C costs below that is not
  `fsync`.
- **A warm record read costs 8.0 µs**, which is the unit posts A and B are
  built from and agrees with the ~11 µs already on file.

## Phase 1 — each post alone, and its slope

Single-threaded and uncontended, so there is no lock wait in these figures
by construction. This is the **work** half of Alexa's separation; phase 1f
and phase 3 measure the **waiting** half against it.

### Post A — historical reads (`collections_owed`), against `T`

| `N` | `T` | dirs | median | iqr/med | per directory |
|-----|-----|------|--------|---------|---------------|
| 50 | 0 | 51 | 422.6 µs | 1.2 % | 8.3 µs |
| 50 | 1 000 | 1 051 | 8.98 ms | 5.4 % | 8.5 µs |
| 50 | 5 000 | 5 051 | 47.91 ms | 3.1 % | 9.5 µs |
| 50 | 20 000 | 20 051 | 186.32 ms | 0.8 % | 9.3 µs |
| 1 000 | 0 | 1 001 | 9.01 ms | 3.7 % | 9.0 µs |
| 1 000 | 20 000 | 21 001 | 210.56 ms | 2.6 % | 10.0 µs |

Linear in the **total** directory count, 8.3–10.0 µs each, and it does not
matter whether a directory holds an open job or a closed one: the two
sweeps add up (9.01 + 186.32 ≈ 195 against 210.56 measured, the rest being
the per-directory cost drifting up with the directory's size).

### Post B — open-job reads (`unfinished_jobs`), against `N`

| `N` | `T` | median | iqr/med | per open job |
|-----|-----|--------|---------|--------------|
| 50 | 0 | 430.4 µs | 3.0 % | 8.6 µs |
| 200 | 0 | 2.36 ms | 2.2 % | 11.8 µs |
| 1 000 | 0 | 9.78 ms | 2.5 % | 9.8 µs |
| 4 000 | 0 | 39.59 ms | 5.1 % | 9.9 µs |
| 200 | 0 | 1.76 ms | 2.3 % | — |
| 200 | 20 000 | 1.75 ms | 6.6 % | — |

Linear in `N`, ~9.9 µs each, and **flat in `T`** — the last two rows differ
by 0.6 %, well inside their own spread.

### Post C — durable writes (`record_waiting`), against `N`

The post the earlier campaigns left out, and the one that dominates.

| `N` | one write, news | iqr/med | one write, no news | whole pass of `N` news writes | per job |
|-----|-----------------|---------|--------------------|-------------------------------|---------|
| 50 | 3.64 ms | 179 % | 11.4 µs | 305.86 ms | 6.12 ms |
| 200 | 4.02 ms | 180 % | 11.3 µs | 1.871 s | 9.36 ms |
| 1 000 | 3.66 ms | 185 % | 11.4 µs | 10.156 s | 10.16 ms |

Three facts, in order of how much they matter:

1. **A junction that is news costs ~10 ms per queued job**, and a queue of
   a thousand therefore costs **10.2 s of durable writing** — against
   **19 ms** for both reads at the same depth. The write half is **530
   times** the read half at `N` = 1 000, `T` = 0. This is the number the
   earlier campaigns' `junction` column did not have.
2. **D171's quiet path costs 11.4 µs**, which is 320 times less, and its
   spread is 3.6 % rather than 180 %. A pass with nothing new to say is
   free. The cost is paid only when the *cause* changed — so what matters
   for a live hub is how often that happens, not how deep the queue is.
3. **A single write is bimodal and the aggregate is not.** `iqr/med` of
   180 % on one write is two modes (~3.6 ms and ~10 ms), which is ZFS
   grouping `fsync`s into transaction groups; summed over a pass the modes
   average out and the per-job figure rises smoothly with `N` (6.12 →
   9.36 → 10.16 ms) as sustained pressure saturates the intent log.
   **Report post C per pass, never per write.**

### Post E — the dispatch computation, against `N`

The parts of `dispatch_a_pass` that are public, measured directly:

| `N` | `queue::order` | `holdings` | `reserved` | sentences | total |
|-----|----------------|------------|------------|-----------|-------|
| 50 | 13.4 µs | 53 ns | 51 ns | 43.5 µs | 57.0 µs |
| 200 | 57.6 µs | 137 ns | 146 ns | 187.9 µs | 245.8 µs |
| 1 000 | 284.5 µs | 893 ns | 886 ns | 897.7 µs | 1.18 ms |
| 4 000 | 1.27 ms | 6.4 µs | 6.4 µs | 3.43 ms | 4.72 ms |

`O(N log N)` and small: at `N` = 1 000 the whole computation is **1.18 ms**
against 10.2 s of writing. `matcher::place` is not in this table — it needs
a `Seat`, which only a live fleet builds — so this is a **lower bound** on
post E; phase 2 measures post E whole on a real junction, and the two are
compared there.

## Phase 1f — post D, measured as waiting and not as slowness

"How long is the holder busy" and "how long is everyone else blocked" are
different questions, and only the second is a lock cost. The control is the
**same I/O without the mutex**: `durable::names_in` plus one
`read_to_string` per directory, which is what `collection_owed` reads. If
the control scales with the callers and the store does not, the difference
is the lock and nothing else.

`N` = 1 000, `T` = 0 (1 001 directories):

| callers | through the store | without the mutex | store | control |
|---------|-------------------|-------------------|-------|---------|
| 1 | 9.56 ms | 4.77 ms | x1.00 | x1.00 |
| 2 | 19.52 ms | 4.87 ms | x2.04 | x1.02 |
| 4 | 38.72 ms | 5.59 ms | x4.05 | x1.17 |
| 8 | 82.28 ms | 7.92 ms | x8.60 | x1.66 |

`N` = 50, `T` = 20 000 (20 051 directories):

| callers | through the store | without the mutex | store | control |
|---------|-------------------|-------------------|-------|---------|
| 1 | 189.55 ms | 93.06 ms | x1.00 | x1.00 |
| 2 | 473.94 ms | 116.11 ms | x2.50 | x1.25 |
| 4 | 904.64 ms | 134.67 ms | x4.77 | x1.45 |
| 8 | 1.770 s | 187.93 ms | x9.34 | x2.02 |

**The store's `Mutex<Live>` serialises the history walk completely.** `C`
callers cost `C` times one call, while the identical I/O without the mutex
stays nearly flat on twelve threads. At four callers and `T` = 20 000, a
caller spends **904.64 − 134.67 ≈ 770 ms blocked out of 905 ms, i.e. 85 %
of the call is waiting**; at `N` = 1 000, `T` = 0 it is 33 ms out of 39,
86 %. The walk itself is ~95 % parallelisable and is not allowed to be.

The reason is in the code and not in the figures: `collections_owed`
(`store.rs:2367`) takes and releases the lock **once per directory**,
because each entry goes through `collection_owed` → `JobStore::job` →
`self.lock()` (`store.rs:2255`), whereas `unfinished_jobs`
(`store.rs:2313`) takes it **once** and holds it for the whole read.

The mix a live hub actually makes — one junction walking the history while
the watchdog reads the queue (`server.rs:2459`, ticked from `:5050`):

| `N` | `T` | alone | beside the other | added |
|-----|-----|-------|------------------|-------|
| `collections_owed`, 1 000 | 0 | 9.24 ms | 18.33 ms | +9.09 ms |
| `unfinished_jobs`, 1 000 | 0 | 9.11 ms | 9.70 ms | +591 µs |
| `collections_owed`, 50 | 20 000 | 202.50 ms | 215.75 ms | +13.26 ms |
| `unfinished_jobs`, 50 | 20 000 | 444.6 µs | 442.6 µs | +0 ns |

The asymmetry is the per-record lock again: a history walk that releases
the lock between directories lets a queue read in almost at once (+591 µs),
and pays for the whole of it in return (+9.09 ms, which is one entire
`unfinished_jobs`).

## Phase 2 — the five posts, on one real runner junction

A hub on a loopback port, a real `herd_runner::run` dialling it, and the
five posts read off the store's own trace. Five junctions per cell,
medians. Every cell made **exactly one `collections_owed`, exactly one
`unfinished_jobs`, and exactly `N` `record_waiting`** — no unclassified
store call in any cell, so the window is fully accounted for.

Load before the phase: `3.05 4.72 3.38`. Free on `/`: 56 GiB. Total run:
563 s.

### The table

| `N` | `T` | A historical | B open-job | C writes | D lock wait | E compute | window |
|-----|-----|--------------|------------|----------|-------------|-----------|--------|
| 50 | 0 | 502.2 µs | 434.6 µs | **188.92 ms** | 614 ns | 1.10 ms | 190.83 ms |
| 200 | 0 | 1.95 ms | 1.90 ms | **1.603 s** | 698 ns | 3.67 ms | 1.611 s |
| 1 000 | 0 | 15.39 ms | 9.99 ms | **9.633 s** | 1.2 µs | 20.86 ms | 9.685 s |
| 50 | 5 000 | 48.76 ms | 476.4 µs | **722.77 ms** | 1.5 µs | 1.48 ms | 793.41 ms |
| 50 | 20 000 | 197.69 ms | 495.4 µs | **420.33 ms** | 1.4 µs | 1.35 ms | 613.99 ms |
| 1 000 | 20 000 | 203.33 ms | 9.84 ms | **11.458 s** | 1.6 µs | 21.71 ms | 11.692 s |

The same table as shares of the window:

| `N` | `T` | A | B | C | D | E |
|-----|-----|---|---|---|---|---|
| 50 | 0 | 0.26 % | 0.23 % | **99.0 %** | 0.0003 % | 0.58 % |
| 200 | 0 | 0.12 % | 0.12 % | **99.5 %** | 0.00004 % | 0.23 % |
| 1 000 | 0 | 0.16 % | 0.10 % | **99.5 %** | 0.00001 % | 0.22 % |
| 50 | 5 000 | 6.1 % | 0.06 % | **91.1 %** | 0.0002 % | 0.19 % |
| 50 | 20 000 | 32.2 % | 0.08 % | **68.5 %** | 0.0002 % | 0.22 % |
| 1 000 | 20 000 | 1.7 % | 0.08 % | **98.0 %** | 0.00001 % | 0.19 % |

### The spread, post by post

This is the half of the answer the earlier campaign could not give.

| `N` | `T` | A | B | C | D | E | window |
|-----|-----|---|---|---|---|---|--------|
| 50 | 0 | 3.6 % | 11.7 % | 190.3 % | 45.6 % | 16.0 % | 188.5 % |
| 200 | 0 | 0.7 % | 5.1 % | 17.4 % | 49.0 % | 6.1 % | 17.3 % |
| 1 000 | 0 | 17.9 % | 8.0 % | **5.4 %** | 10.8 % | 5.8 % | **5.2 %** |
| 50 | 5 000 | 4.6 % | 5.2 % | 16.8 % | 33.5 % | 8.3 % | 15.7 % |
| 50 | 20 000 | 10.8 % | 0.8 % | 58.4 % | 17.8 % | 24.1 % | 50.6 % |
| 1 000 | 20 000 | 5.7 % | 1.9 % | 7.2 % | 5.5 % | 6.6 % | **7.0 %** |

`iqr/med`, over five junctions. Read it this way:

- **The deep-queue cells are the quiet ones.** At `N` = 1 000 the whole
  junction reproduces to **5.2 %** (`T` = 0) and **7.0 %** (`T` = 20 000).
  The reviewer's 0.3–4.0 s range is gone, and it was method, not host (see
  "What I judge wrong in the brief", item 5).
- **The shallow cells are the noisy ones, and that is post C's bimodality
  showing through.** Fifty writes are not enough samples for the two ZFS
  modes to average out, so one round in five lands on a transaction-group
  stall and the median of five inherits it (190 % at `N` = 50, `T` = 0 —
  four rounds at ~187 ms and one at 2.00 s). **Post C's relative spread
  falls as `N` rises**, which is the opposite of the usual intuition and
  is exactly what a bimodal per-write distribution does when you sum it.
- **Post D's 45 % spread is 45 % of 614 ns.** A post measured in hundreds
  of nanoseconds is reported with its spread for honesty, not because the
  spread matters.

### What the table says

1. **Post C is the junction.** At every depth of queue, with or without a
   history, durable writing is 68–99.5 % of the window. Everything the
   earlier campaigns measured — the two reads — is together **under 0.3 %**
   at `N` = 1 000, `T` = 0. The decision this campaign was asked to inform
   is a decision about `record_waiting`, not about `collections_owed`.
2. **Post A matters only when the queue is shallow.** At `N` = 50,
   `T` = 20 000 it is 32 % of the window; add a thousand open jobs to the
   same history and it falls to 1.7 %, because post C grew twenty-seven
   times and post A did not move. `T` is a real cost and it is the *second*
   one.
3. **Post D is nothing, for one junction.** 614 ns to 1.6 µs in every
   cell: the `dispatching` lock is uncontended when one runner dials a
   quiet hub, as it must be. Post D is a coupling term between
   *simultaneous* junctions, measured in phase 3, and not a cost of a
   junction.
4. **Post E is small but fifteen to nineteen times the public queue work.**
   Phase 1e measures `queue::order` + `holdings` + `reserved` + the
   sentences at 57 µs (`N` = 50) and 1.18 ms (`N` = 1 000); live, post E is
   1.10 ms and 20.86 ms — about **21 µs per queued job** either way. The
   missing work is `matcher::place` and the hub's own `println!` of one
   `stays queued` line per job (`server.rs:2618`). Which of the two
   dominates is an `H`, not an `F`.

### A third method on post C: count the syncs, do not time them

`record_waiting` was straced twice with the same corpus and a different
number of writes, so the corpus cancels:

```sh
for k in 1 201; do
  for skip in "" HERD_SKIP_WRITES=1; do
    env HERD_WRITES=$k $skip strace -f -c -e trace=fsync,fdatasync \
      target/release/deps/junction_breakdown-<hash> \
      --exact --ignored --nocapture syncs_per_record_waiting
  done
done
```

| run | `fsync` | `fdatasync` |
|-----|---------|-------------|
| 201 jobs, 201 writes | 1 828 | 406 |
| 201 jobs, no write | 1 426 | 205 |
| **difference, for 201 writes** | **402** | **201** |
| **per `record_waiting`** | **2** | **1** |

**Three syncs per `record_waiting`, not four** — the record file's
`sync_all` and the job directory's `sync_all` (both from
`durable::write_file_with`), plus `events.jsonl`'s `sync_data` (from
`durable::append_line`). The same difference at `HERD_WRITES` = 1 gives
the same 2 + 1. It agrees with phase 0's floor, 1.19 + 0.0066 + 1.20 =
2.39 ms, against a measured 3.6 ms median for the whole call.

## Phase 3 — post D, live: two junctions at once

The only configuration in which post D is a cost. Two runners dial the
same hub together. `state.dispatching` (`server.rs:2493`) is held from
before the first `unfinished_jobs` until after the last `record_waiting`,
so **the second junction's gap between its own `collections_owed` and its
own `unfinished_jobs` is exactly the time it spent blocked** — read off
the trace, from outside, with nothing added to the hub.

Five rounds per cell, and the five rounds are listed, because the median
is the wrong statistic here: whether two junctions overlap at all is a
matter of arrival timing.

| `N` | `T` | first junction's window | the second's wait, round by round | overlapped |
|-----|-----|-------------------------|-----------------------------------|------------|
| 50 | 0 | 246.25 ms | 782 ns, 184.37 ms, 187.17 ms, 238.28 ms, 425.61 ms | 4 / 5 |
| 200 | 0 | 1.700 s | 792 ns, 900 ns, 955 ns, 1.672 s, 2.157 s | 2 / 5 |
| 1 000 | 0 | 9.403 s | 937 ns, 1.2 µs, 1.4 µs, 1.5 µs, 8.874 s | 1 / 5 |
| 200 | 20 000 | 1.998 s | 1.7 µs, 2.2 µs, 1.141 s, 1.532 s, 1.934 s | 3 / 5 |

A round either overlaps or it does not; there is no middle. When it does
not, post D is ~1 µs — the uncontended lock. When it does:

| `N` | the second's wait, when it overlapped | the first's post C (phase 2) |
|-----|---------------------------------------|------------------------------|
| 50 | 184–426 ms | 188.92 ms |
| 200 | 1.67–2.16 s | 1.603 s |
| 1 000 | 8.87 s | 9.633 s |

**Post D of a concurrent junction is post C of the junction ahead of
it.** The `dispatching` lock serialises passes; a pass is 99 % durable
writing; so a runner that dials while another runner's pass is writing
waits for that writing and for nothing else. The lock is not the cost —
it is the mechanism by which one junction's cost becomes another's.

How often two junctions *do* overlap is **not** measured here: the
overlap rate above (4/5, 2/5, 1/5, 3/5) is a property of how fast two
`herd_runner::run` tasks happen to come up in one test process, not of
any real fleet. See the `I` list.


## Reproducibility, and the residual noise

Phase 2 was run three times. Run 1 is the full grid on a quiet host. Run 2
began quiet and was abandoned part-way when a host backup
(`borgbackup create`) drove `/proc/loadavg` from 2.9 to **35.7** — above
the guard this campaign works under, so the run was stopped rather than
reported. Run 3 repeats the three `T` = 0 cells after the host went quiet
again.

Run 1 against run 3, same command, same grid, independent processes:

| `N` | A | B | C | D | E | **window** |
|-----|---|---|---|---|---|------------|
| 50 | −2.4 % | +3.4 % | **+298.6 %** | +3.1 % | +40.9 % | **+295.9 %** |
| 200 | −9.2 % | −7.9 % | +8.4 % | −18.9 % | +6.5 % | **+8.3 %** |
| 1 000 | −13.1 % | −7.4 % | **−1.7 %** | −22.4 % | −8.9 % | **−1.8 %** |

Run 1 against run 2's two completed cells (`N` = 50: C +91.5 %, window
+90.6 %; `N` = 200: C +3.1 %, window +3.0 %) says the same thing a third
time.

**The margin, stated plainly:**

- **At `N` ≥ 200 the breakdown reproduces.** The window is within 8 %
  between runs at `N` = 200 and within **2 %** at `N` = 1 000, and every
  post is within 25 % of itself. The deeper the queue, the tighter the
  figure — because post C dominates and post C is a sum of many samples.
- **At `N` = 50, post C is not a measurement.** 189 ms, 362 ms, 753 ms
  over three runs of the same cell: a factor of four. Fifty `fsync`
  triples are too few for ZFS's transaction grouping to average out, and
  the pool's own state (how much dirty data a backup left behind) sets the
  answer. **Do not quote a post-C figure below `N` = 200.**
- **Posts A, B and E reproduce everywhere**, within 13 % and usually
  within 9 %, `N` = 50's post E included once post C's stall is excluded.
- **Post D's 19–22 % is 19 % of 600 ns.** It reproduces as "nothing",
  which is the only claim made about it for a single junction.

**What is left that this campaign cannot remove:** the host is shared, and
nothing here can stop a backup starting mid-run. The defence is the guard
(`/proc/loadavg` read before every phase, abandon above 8) and the
three-run comparison above, not a quieter host.

## Debug against release: the strongest check that post C is the disk

Everything above is `--release`, which is what `nix/workspace.nix` ships.
The same junction, same grid, from the **debug** binary the workspace
suite builds (three rounds rather than five):

```sh
HERD_ONLY_T0=1 HERD_ROUNDS=3 \
  target/debug/deps/junction_breakdown-<hash> \
  --exact --ignored --nocapture one_real_junction_broken_down
```

`N` = 1 000, `T` = 0:

| post | release | debug | debug / release |
|------|---------|-------|-----------------|
| A historical reads | 15.39 ms | 123.20 ms | **x8.0** |
| B open-job reads | 9.99 ms | 61.57 ms | **x6.2** |
| C durable writes | 9.633 s | 9.633 s | **x1.00** |
| D lock wait | 1.2 µs | 3.7 µs | x3.1 |
| E dispatch compute | 20.86 ms | 77.60 ms | **x3.7** |
| **junction window** | **9.685 s** | **9.849 s** | **x1.017** |

At `N` = 200 the same shape: A x7.0, B x6.4, C x1.15, E x2.6, window
x1.17. At `N` = 50: A x6.6, B x7.0, E x3.4, and post C too noisy at that
depth to compare (see "Reproducibility").

**The three CPU-bound posts slow down by four to eight times and post C
does not move at all.** That is the cleanest evidence in this file that
post C is the device and not the code: an optimiser has nothing to
optimise in an `fsync`. It also means **a junction costs the same in debug
as in release** — 1.7 % apart at `N` = 1 000 — which is worth knowing
before anyone tries to explain a slow hub by its build profile.

## What is `F`, what is `H`, what is `I`

**`F` — established, with the figure.**

1. A junction at `N` = 1 000, `T` = 0 costs **9.69 s**, of which
   **9.633 s (99.5 %) is post C**, durable writes. Posts A, B, D and E
   together are **0.48 %** of it. (Phase 2, five rounds, window
   `iqr/med` 5.2 %, and the whole cell reproduces to 1.8 % in an
   independent run.)
2. **Post C is `N` x ~10 ms**, one `record_waiting` per job the pass leaves
   queued, and each of those is three `fsync`s on this host's ZFS mirror.
   (Phases 0, 1d, 2, three ways.)
3. **D171's quiet path costs 11.4 µs**, 320 times less than a write that is
   news. A pass with nothing new to say is free. (Phase 1d.)
4. **Post A is linear in the total directory count**, 8.3–10.0 µs each,
   indifferent to whether the directory holds an open job or a closed one.
   (Phase 1a.)
5. **Post B is linear in `N`** at ~9.9 µs per open job and **flat in `T`**
   (0.6 % apart between `T` = 0 and `T` = 20 000). (Phases 1b, 1c.)
6. **Post E is `O(N log N)` and small**: 1.18 ms of public queue work at
   `N` = 1 000, 3.7–5 ms measured whole on a live junction at `N` = 200.
   (Phases 1e, 2.)
7. **The store's `Mutex<Live>` serialises `collections_owed` completely**:
   `C` callers cost `C` times one call while the same I/O without the mutex
   stays flat on twelve threads. At four callers, 85–86 % of a caller's
   `collections_owed` is time blocked, not time working. (Phase 1f, with a
   mutex-free control.)
8. **With one runner and no other caller, post D is ~600 ns–1.6 µs**, i.e.
   nothing. Post D is not a property of a junction; it is a property of
   *concurrent* junctions. (Phase 2, every cell.)
9. **When two junctions do overlap, the second waits the first's post C**:
   184–426 ms behind a pass of 50, 1.67–2.16 s behind 200, 8.87 s behind
   1 000 — against the first's post C of 189 ms, 1.603 s and 9.633 s.
   `state.dispatching` does not add a cost; it passes one junction's cost
   to the next. (Phase 3.)
10. **The breakdown reproduces at `N` >= 200 and not at `N` = 50.** Two
    independent runs agree on the whole window to 1.8 % at `N` = 1 000
    and 8.3 % at `N` = 200; at `N` = 50 post C was 189 ms, 362 ms and
    753 ms on three runs of the same cell. (Three runs of phase 2.)
11. **Post C is identical in debug and in release; A, B and E are four to
    eight times slower in debug.** At `N` = 1 000 the window differs by
    1.7 % between the two profiles. An optimiser has nothing to optimise
    in an `fsync`, which is the cleanest proof in this file that post C
    is the device. (Debug section above.)
12. **One `record_waiting` makes three syncs, not four**: two `fsync` and
    one `fdatasync`, counted under `strace` with the corpus cancelled out,
    and agreeing with the 2.39 ms floor of phase 0. (Phase 0, phase 1d,
    strace.)

**`H` — a hypothesis, with what would confirm it.**

1. *Post C grows with `T` as well as with `N`.* At `N` = 50 it is 189 ms at
   `T` = 0, 723 ms at `T` = 5 000 and 420 ms at `T` = 20 000 — not
   monotone, and the spread is wide. The likely cause is ZFS: the history
   walk immediately before the writes fills the ARC with 5 000–20 000
   records and the writer then contends with eviction. **Confirmed by**
   repeating phase 2 with the warm-up walk removed, and by
   `arcstat`/`zpool iostat` sampled across the junction.
2. *The bimodality of a single `record_waiting` is ZFS transaction-group
   batching.* Two modes at ~3.6 ms and ~10 ms, `iqr/med` 180 % per write
   against 5–17 % per pass. **Confirmed by** `zfs_txg_timeout` changed on a
   scratch pool, or by the same bench on ext4, where the modes should
   collapse.
3. *Post C on another filesystem is a different number of the same shape.*
   The three `fsync`s are the code path; 1.2 ms each is this SATA mirror
   with no SLOG. **Confirmed by** running phase 1d with `TMPDIR` on NVMe
   ext4 and on `tmpfs`; the shape (`N` x k) should hold with k an order of
   magnitude smaller.
4. *The live post E is dominated by the hub's own `println!`, not by
   `matcher::place`.* Live post E is 1.10 ms at `N` = 50 and 3.67 ms at
   `N` = 200, which is 19x and 15x the public queue work measured in phase
   1e — and the hub prints one `stays queued` line per job
   (`server.rs:2618`). **Confirmed by** running the same junction with
   stdout to `/dev/null` versus to a pipe, and comparing post E.

**`I` — could not be decided here.**

1. **What post D is worth on the real hub under its real concurrency.**
   Phase 3 measures a second junction blocked behind the first, and phase
   1f measures the store mutex against a control — but neither says how
   often two junctions, a watchdog tick and a submission actually coincide
   on the production hub. That is a question about the *arrival process*,
   not about the cost of one event, and nothing in this campaign observes
   a production arrival process.
2. **What any of this costs on the server the hub actually runs on.**
   Every figure here is from one host, one filesystem and one device. Post
   C in particular is device-bound. Until the same bench runs on the NixOS
   server's state root, the absolute figures are this workstation's.
3. **Whether `collections_owed` can be answered from the open jobs.** The
   review bench already showed a collection can be owed by a job that is
   *not* open, so the reduction is not sound as stated; what a correct
   cheaper form would cost is a design question, not a measurement, and it
   is not answered here.
4. **The cost of a junction on a queue that is partly placeable.** Every
   job in this corpus needs a capability nothing offers, so every pass
   writes a wait and nothing is ever dispatched. A pass that *places* jobs
   pays `begin_attempt` and `record_state` instead, which are different
   writes. Deliberate — it is what keeps the bench from spawning processes
   on a shared host — and it means the table is the **deep-queue** case and
   not the general one.

## What I judge wrong in the brief

Said plainly, because the brief asked for it.

1. **"quatre `fsync`" is three.** `record_waiting` (`store.rs:2007`) calls
   `write_record` then `append_event`. `write_record` →
   `durable::write_file_with` does `file.sync_all()` and then
   `sync_dir(parent)` — two. `append_event` → `durable::append_line` does
   `file.sync_data()` — one. Three, not four. The arithmetic matches: the
   measured floor is 1.19 + 0.0066 + 1.20 = **2.39 ms**, and a fourth
   `fsync` would put it near 3.6 ms.
2. **"~11 µs par répertoire, 247 ms à `T` = 20 000" is high.** In release
   on this host it is 8.3–10.0 µs per directory and **186 ms** at
   `T` = 20 000, `N` = 50 (211 ms at `N` = 1 000). The shape is right; the
   constant is ~25 % optimistic about how bad it is.
3. **"Les deux lectures ensemble à `N` = 1 000 : 13 ms" is low.** Measured
   solo at `T` = 0: 9.01 + 9.78 = **18.8 ms**. Live, inside the junction:
   1.95 + 1.90 ms at `N` = 200 and 9.0 + 9.8 ms at `N` = 1 000. Either way
   the conclusion is unchanged and strengthened — the read half is even
   smaller next to the write half than the brief assumed.
4. **The brief frames lock wait as one of five comparable posts. It is
   not, for a single junction.** With one runner, post D is 600 ns–1.5 µs
   in every cell — six to seven orders of magnitude below post C. Post D
   only becomes a post when two callers meet, and then it is large. Asking
   for "the five posts of one junction" and expecting five comparable
   numbers would mislead; the honest statement is **four posts and a
   coupling term**.
5. **"La mesure vivante du relecteur s'est étalée de 0,3 à 4,0 s" had two
   avoidable causes, and both were method, not host.** The reviewer timed
   from `herd_runner::run` being spawned — which includes the runner's
   confinement setup, a process-level variable with nothing to do with the
   pass — and detected settlement by calling `store.job()` every 1 ms,
   which takes the store's `Mutex<Live>` (`store.rs:2255`) *against the
   writes being measured*. Removing both gives a window with `iqr/med`
   5.2 % at `N` = 1 000. The host was never the problem.
6. **One premise the code confirms, against my expectation.** The brief
   says the `dispatching` lock is taken *after* `collections_owed`, four
   lines later. True: `ask_for_the_files_still_owed` at `server.rs:2415`,
   `dispatch_queued_jobs` at `:2419`, the lock at `:2493`. That ordering is
   what makes the five-way split observable from outside at all, so the
   brief's most useful sentence is also its most accurate one.

