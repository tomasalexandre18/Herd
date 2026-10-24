# Experiment 6.1 — the cost surface of one dispatch pass

What this answers: of `N` (open jobs), `T` (closed job directories) and
`d` (latency per record access), **which term dominates the worst turn** —
the longest stretch over which the hub's reactor cannot answer anything.
The decision behind it is whether taking persistence out of the reactor is
urgent or merely correct.

Every claim below carries **F** (measured, or read with a `file:line`),
**H** (hypothesis, with what would confirm it) or **I** (unknown).

The bench is `herd-hub/tests/cost_surface.rs`, four `#[ignore]`d cases. It
adds no production code. Run it on purpose:

```sh
cargo test -p herd-hub --test cost_surface --release -- \
  --ignored --nocapture --test-threads=1
```

## The three hot paths, as the code has them

- **F** `dispatch_a_pass` (`herd-hub/src/server.rs:2509`) is a synchronous
  function. Its first act is `store.unfinished_jobs()`, and it runs a
  second one when a reading settles (`server.rs:2523`). It is reached only
  through `dispatch_queued_jobs` (`server.rs:2493`), which takes the single
  `state.dispatching` **std** mutex and holds it across the whole pass.
  `unfinished_jobs` walks the store's in-memory pending list
  (`herd-hub/src/store.rs:2312`), so it is O(`N`) and **not** a function of
  `T`.
- **F** `collections_owed` (`herd-hub/src/store.rs:2367`) lists *every*
  directory under `jobs/` through `durable::names_in` and reads one record
  per entry. Closed jobs are entries. It is O(`N + T`). It is reached from
  `ask_for_the_files_still_owed` (`server.rs:2415`), called at every runner
  junction — so at every redial — four lines before the pass at
  `server.rs:2419`, and again per outputs query at `server.rs:4384`.
- **F** The watchdog ticks every second with no condition and calls
  `attempts_in_the_air` (`server.rs:2459`, from `server.rs:5050`), which is
  `unfinished_jobs()` — O(`N`), once a second, for ever.
- **F** Each record read opens and reads `job.json` once and then runs
  `serde_json::from_str` over the same text **three times**
  (`store.rs:1142`, `:1150`, `:1160`). The repetition is CPU, not I/O: the
  bytes are read once (see the `bytes` column below).
- **F** Nothing ever removes a job directory. `sweep_outputs`
  (`store.rs:642`) removes output *bytes* and leaves the directory; the
  only `remove_dir_all` on a job path is the part directory
  (`store.rs:1981`). So `T` only grows, and it grows by one per job the
  herd has ever finished.
- **F** `sweep_outputs` is a third O(`T`) walk, and a heavier one — it calls
  `directory_bytes` on each job's `files/` subtree. Its trigger is an event
  and not a clock: only a collection landing over budget calls it
  (`store.rs:530`–`:539`). **I** — not measured here.

## The plan, and why this plan

Not the cartesian product of 4x4x4x2. The product would spend most of its
cells confirming an interaction the code rules out: `unfinished_jobs` and
`collections_owed` read disjoint things — one the in-memory pending list,
one the directory — so `N` and `T` cannot interact inside a pass. What is
needed is one sweep per axis, plus a corner at both maxima to show the two
sweeps simply add.

| case | varies | fixed | what it settles |
|---|---|---|---|
| 1 | `N` in {50, 200, 400, 1000} then `T` in {0, 1k, 5k, 20k}, plus the corner (1000, 20000) | `d` = 0 | the cost of one call, and the record and byte counts behind it |
| 2 | `N` in {50, 1000} x `T` in {0, 20000} x `W` in {2, 4} x callers in {1, `W`} | `d` = 0 | the worst turn, sampled |
| 3 | `d` in {0, 1, 5, 20} ms, callers in {1, 4} | `N` = 200, `T` = 0, `W` = 4 | that `d` is linear in records touched, and how it interacts with `W` |
| 4 | a live hub with a real runner dialling in | `N` = 50, `T` = 5000 | **how often** the hub pays each cost |

Case 4 is the one no table of durations can replace: a cost paid once at
startup and a cost paid at every redial are not the same cost.

### The oracle

**Worst turn**, sampled continuously, never divided out of a total: a probe
task on the same runtime asks for a 500 µs timer over and over and records
how late it was. A synchronous call made from an async task holds a worker
thread; while every worker is held, nothing on that runtime runs — no
timer, no frame, no health report. One 900 ms pass and nine 1 ms passes
have the same total and are not the same hub.

**F** The probe's noise floor on this host is 1.63 ms (W = 2 and W = 4
alike). Any figure at ~1.6 ms below means *no stall was observed*.

### What `d` means here, and the one place this experiment is a model

The delegating store (`CostlyStore`, built from the shape of
`StoreThatHoldsTheClaimOpen`, `server.rs:6928`) charges `d` **per record
access**, not per trait call. That choice is load-bearing and it is the
honest one: `unfinished_jobs` is a single trait call that reads `N`
records and `collections_owed` is a single trait call that reads `N + T`.
A wrapper that slept once per call would make `d` a constant — at `d` =
20 ms, 40 ms on a junction — and would answer the question by
construction.

So the measured part of the `d` axis is **F** (the number of record
accesses, exactly, and the wall clock of the sleeps) and the extrapolation
to a real remote store is **H**: it assumes such a store costs per record
and cannot batch. What would confirm it is one real backend behind the
same trait. Note also that today's `d` is not zero — it is the 11 µs per
record measured in case 1.

## Case 1 — what one call costs

Release profile (what ships: `nix/workspace.nix` builds with
`rustPlatform.buildRustPackage`, which is release, **F**), median of five,
warm page cache, on a shared 12-core host. `reads` is read syscalls and
`bytes` is bytes read, both from `/proc/self/io`.

| N | T | dirs | `unfinished_jobs` | reads | bytes | `collections_owed` | reads | bytes |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 50 | 0 | 51 | 853 µs | 104 | 24 544 | 837 µs | 106 | 25 043 |
| 200 | 0 | 201 | 2.04 ms | 404 | 97 961 | 2.56 ms | 406 | 98 460 |
| 400 | 0 | 401 | 3.54 ms | 804 | 195 912 | 3.58 ms | 806 | 196 411 |
| 1000 | 0 | 1001 | 11.40 ms | 2 004 | 489 813 | 9.81 ms | 2 006 | 490 312 |
| 50 | 1 000 | 1 051 | 432 µs | 104 | 24 552 | 10.08 ms | 2 106 | 524 051 |
| 50 | 5 000 | 5 051 | 428 µs | 104 | 24 556 | 48.76 ms | 10 106 | 2 520 055 |
| 50 | 20 000 | 20 051 | 794 µs | 104 | 24 553 | **247.16 ms** | 40 106 | 10 005 052 |
| 1000 | 20 000 | 21 001 | 10.05 ms | 2 004 | 489 776 | **220.04 ms** | 42 006 | 10 470 275 |

The same grid in the debug profile, for anyone running the bench without
`--release` (**F**): `unfinished_jobs` 2.93 / 12.44 / 24.91 / 65.91 ms over
the `N` sweep; `collections_owed` 3.02 / 12.31 / 24.67 / 62.79 / 68.01 /
327.46 / **1 325.93** / **1 390.12** ms down the column. Debug is ~6x
release and must not be quoted as the hub's cost.

What the columns say:

- **F** Two read syscalls and ~499 bytes per record, in both calls, at every
  size. The file is read once; the three `serde_json::from_str` calls parse
  the same `String`. So **serde parses = 3 x records read**: 150 at
  (N=50, T=0), 60 153 at (N=50, T=20 000).
- **F** `unfinished_jobs` is flat in `T`: 853 µs, 432 µs, 428 µs, 794 µs as
  `T` goes 0 to 20 000 at `N` = 50. It is ~10 µs per open job.
- **F** `collections_owed` is linear in `N + T` at ~11 µs per directory,
  and at `T` = 20 000 it is **290x** the cost of `unfinished_jobs` at the
  same `N`.
- **F** The corner adds up: (1000, 20 000) costs what (50, 20 000) costs,
  within noise, because 1 000 extra directories on 20 000 is 5 %.

## Case 2 — the worst turn against `N`, `T` and `W`

Release, `d` = 0, five rounds per cell, noise floor 1.63 ms. `callers` is
how many async tasks enter the synchronous call at the same moment —
what a redial storm, a watchdog tick and a submission landing together look
like from the reactor's side. `junction` is `collections_owed` then the
pass behind the `dispatching` lock; `watchdog read` is `unfinished_jobs`
alone.

| N | T | W | callers | watchdog read | junction |
|---:|---:|---:|---:|---:|---:|
| 50 | 0 | 2 | 1 | 1.62 ms | 1.60 ms |
| 50 | 0 | 2 | 2 | 1.57 ms | 1.86 ms |
| 50 | 0 | 4 | 1 | 2.05 ms | 1.68 ms |
| 50 | 0 | 4 | 4 | 1.59 ms | 1.71 ms |
| 1000 | 0 | 2 | 1 | 1.63 ms | 1.64 ms |
| 1000 | 0 | 2 | 2 | 16.95 ms | 27.64 ms |
| 1000 | 0 | 4 | 1 | 1.66 ms | 1.66 ms |
| 1000 | 0 | 4 | 4 | 13.15 ms | **33.22 ms** |
| 50 | 20 000 | 2 | 1 | 1.67 ms | 6.49 ms |
| 50 | 20 000 | 2 | 2 | 1.56 ms | **474.28 ms** |
| 50 | 20 000 | 4 | 1 | 1.63 ms | 1.68 ms |
| 50 | 20 000 | 4 | 4 | 1.61 ms | **814.68 ms** |
| 1000 | 20 000 | 2 | 1 | 1.70 ms | 1.65 ms |
| 1000 | 20 000 | 2 | 2 | 11.81 ms | **480.44 ms** |
| 1000 | 20 000 | 4 | 1 | 1.64 ms | 2.64 ms |
| 1000 | 20 000 | 4 | 4 | 16.82 ms | **811.51 ms** |

Three things fall out of this table.

- **F** With `callers` = 1 there is essentially no stall, at any `N` or `T`:
  one blocked worker leaves the others free and the probe still runs. The
  reactor only dies when the number of simultaneous synchronous callers
  reaches `W`. That is reachable: `dispatch_queued_jobs` has thirteen async
  call sites, and a hub restart makes every runner redial at once.
- **F** At `callers` = `W` the stall is the time until the *first* worker
  comes free, which is longer than one pass because the `dispatching` mutex
  queues the callers. Case 3 pins the mechanism exactly.
- **F** `W` = 4 is worse than `W` = 2, not better: 814 ms against 474 ms at
  the same (N, T). More workers means more callers can pile in before the
  reactor is wedged, and each one is blocked, not scheduled.

## Case 3 — the `d` axis, and the mutex

Release, `N` = 200, `T` = 0 (so 201 directories, 200 open jobs), `W` = 4.
A small corpus on purpose: `d` is charged per record, and 20 ms against
case 1's 20 000 directories would be a seven-minute stall — a figure to
compute, not to sit through.

| d (ms) | callers | records charged | watchdog read | junction |
|---:|---:|---:|---:|---:|
| 0 | 1 | 401 | 1.56 ms | 1.61 ms |
| 0 | 4 | 1 604 | 1.64 ms | 4.07 ms |
| 1 | 1 | 401 | 1.59 ms | 1.62 ms |
| 1 | 4 | 1 604 | 201.96 ms | 405.33 ms |
| 5 | 1 | 401 | 1.79 ms | 1.64 ms |
| 5 | 4 | 1 604 | 1.00 s | 2.01 s |
| 20 | 1 | 401 | 3.16 ms | 3.44 ms |
| 20 | 4 | 1 604 | 4.00 s | 8.03 s |

- **F** Perfectly linear in `d`, and the arithmetic names the mechanism.
  At `d` = 20 ms, `callers` = 4: the four `collections_owed` walks run in
  parallel and each takes 201 x 20 ms = 4.02 s; then one caller takes the
  `dispatching` lock and the other three *block a worker thread on a std
  mutex* while it does its 200 x 20 ms = 4.00 s read. The first worker
  comes free at 8.02 s. Measured: 8.025 s.
- **F** `callers` = 1 is again flat — 3.4 ms at `d` = 20 ms. The cost is
  real but it is paid on one worker, and three are left.

## Case 4 — how often a live hub pays it

A real hub on loopback, a real `herd-runner` dialling in, `N` = 50 open
jobs needing a capability nothing offers (so the queue stays deep and
nothing is placed), `T` = 5 000. Counted by the delegating store, not
inferred (**F**):

```
one junction:       collections_owed x1, unfinished_jobs x1, records charged 5103
five idle seconds:  collections_owed x0, unfinished_jobs x6, records charged  350
queue is still 50 deep and nothing was placed
```

- **F** One runner junction = **5 103 record accesses**, of which 5 051 are
  the closed-directory walk: **99 %** of a junction's disk work is `T`, at
  `T` = 5 000 and `N` = 50. Times three serde parses each: 15 309 parses
  for one runner saying hello.
- **F** `collections_owed` is not called while the hub is idle. It is
  purely per junction — therefore per redial, which is per flaky link.
- **F** The watchdog ticks once a second, unconditionally, and reads all
  `N` records each time: 6 ticks and 300 records over five idle seconds on
  a hub with nothing to do. O(`N`) per second for ever, O(`T`) never.

## The surface

With `c` ~ 11 µs the measured cost of one record access on a local
directory with a warm cache (**F**, case 1), and `d` any extra latency a
different persistence layer adds per record:

```
one junction  = (c + d) x (2N + T + 1) + O(N) ordering
one watchdog  = (c + d) x N,  every second
worst turn    ~= k(W) x one junction,  with k(W) ~ 2 when callers reach W
worst turn    ~= noise floor,          while callers stay below W
```

At the deployment's own figures, release, `W` = 4, four simultaneous
redials:

| term | at its maximum here | worst turn it buys |
|---|---|---|
| `N` = 1000, `T` = 0 | 1 000 open jobs — an implausibly deep queue | **33 ms** |
| `T` = 20 000, `N` = 50 | 20 000 finished jobs — a few weeks of herd | **815 ms** |
| `d` = 1 ms, `N` = 200, `T` = 0 | a store that costs 1 ms a record | **405 ms** |

## Which term dominates

**`T`.** (**F**, cases 1, 2 and 4.)

- At both axes' maxima, `T` costs **25x** what `N` costs: 815 ms against
  33 ms. And the comparison flatters `N`, because `N` = 1 000 is a queue
  nobody has seen while `T` = 20 000 is arithmetic — one directory per job
  ever run, and nothing deletes one (**F**, `store.rs`: no `remove_dir_all`
  on a job directory).
- `N` is bounded by what requesters submit and drains as jobs run. `T` is
  monotone and unbounded. **H**: on a hub doing a hundred jobs a day, `T`
  passes 20 000 in about seven months and 100 000 in three years, at which
  point one junction is ~1.2 s of blocked reactor on its own and four
  concurrent redials are ~4 s. What would confirm it: the real `T` on the
  deployed hub, which is **I** here — this experiment never touched it.
- `d` dominates only if the persistence layer changes. It is the term an
  architecture decision controls; `T` is the term the current architecture
  grows on its own.

## What this implies for the architecture choice

The audit's premise is that persistence belongs outside the reactor. The
measurements agree that the reactor is blocked, and disagree about the
urgency and about which fix comes first.

1. **The urgent fix is not architectural.** **F**: 99 % of a junction's disk
   work at `T` = 5 000 is a walk of closed job directories to find
   collections that, by construction, finished jobs do not owe. Stop
   walking the whole volume at every redial — an index of the jobs that
   actually owe bytes, or a `collections_owed` that reads only the open
   jobs the hub already holds — and the `T` term goes to zero without one
   line of the reactor moving. Cost: small and local, in `store.rs`.
   Benefit: 815 ms of worst turn becomes ~33 ms, measured.
2. **Taking persistence out of the reactor is correct, and second.** **F**:
   with `callers` = 1 the worst turn is at the noise floor at every point
   measured. Synchronous store calls on worker threads hurt only when
   enough of them land at once, and after fix 1 the remaining per-call cost
   is 10 ms at `N` = 1 000. That is a real defect — it is unbounded in
   principle and the `dispatching` mutex makes concurrent callers queue on a
   worker thread rather than on a task — but it is not what makes this hub
   stall today.
3. **Do not size the fix off `d`.** **H**: if persistence ever moves behind
   something with per-record latency, `d` x `T` is catastrophic (1 ms x
   20 000 = 20 s a junction) — which is another reason to fix the `T` walk
   first. A refactor that moves `collections_owed` onto a blocking pool
   unchanged would hide a 20 s operation instead of removing it.
4. **The `dispatching` mutex is the amplifier, and it is cheap to note.**
   **F** (case 3, exactly): a std mutex held across the pass turns `k`
   concurrent callers into `k` blocked worker threads. Whatever is decided
   about persistence, a pass that can be skipped when one is already running
   — rather than queued — removes the amplification.

## What was not measured

- **I** Cold page cache. Everything here is a warm ZFS ARC on the host's
  own root. A hub that has just started reads `T` records cold, and that is
  the one case where `T` is certainly worse than measured.
- **I** The real `T` on the deployed hub.
- **I** `sweep_outputs` (`store.rs:642`), the third O(`T`) walk, which also
  stats each job's `files/` subtree. Its trigger is bounded (over budget
  only), so it was left out; it is heavier per directory than
  `collections_owed`.
- **I** Store startup: `JobDirStore::open` sweeps (`store.rs:439`) and
  recovery scans the directory, both O(`T`). Not the reactor, but it is
  the hub's time to first answer.
- **I** `queue::order`, `matcher::holdings`, `matcher::reserved` and
  `matcher::reservations` — the O(`N`) arithmetic of the pass after the
  reads. Case 2's `junction` column charges only the two reads, so the `N`
  term above is a floor, not the whole of it.
- **H** Whether a real remote store costs per record rather than per
  batch, which is what the `d` axis assumes.

## Reproducing

```sh
cargo test -p herd-hub --test cost_surface --release -- \
  --ignored --nocapture --test-threads=1
```

`--test-threads=1` is required: case 1 reads `/proc/self/io`, which is
process-wide and only honest while the process does one thing. The cases
are `#[ignore]`d so the workspace suite stays the suite.
