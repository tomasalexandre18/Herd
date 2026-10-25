# 08 — S0: what a waiting write costs without its syncs

The architecture work for C4 rests on one premise nobody had measured:
that taking the `fsync`s out of a waiting write makes the dominant post
collapse. **If an unsynchronised waiting write costs more than about 1 ms
per job, staging durability is not worth what it costs in contract, and
the recommendation changes.** This file measures that number.

It measures. It decides nothing and recommends nothing beyond saying
which way the premise fell.

## The answer, in one line

**An unsynchronised waiting write costs 70–104 µs per job**, against
**8.1–10.8 ms** for the write the hub makes today. That is **ninety to a
hundred and twenty times less**, and it is **ten times under the 1 ms
threshold at the worst of seven independent measurements**. The premise
holds and the recommendation stands.

The syscall count that has to go with that figure: **zero `fsync` and
zero `fdatasync`**, counted under `strace` with the corpus cancelled, so
the figure is the thing it claims to be.

## The one table

Same corpus, same cells and same method as
[`06-junction-breakdown.md`](06-junction-breakdown.md), so a figure here
and a figure there compare without reinterpretation. `N` is the open jobs
in the queue; `T` = 0 throughout. Release build, medians.

One real runner junction, post C and the whole window:

| `N` | path | C durable writes | C per job | junction window | C's share |
|-----|------|------------------|-----------|-----------------|-----------|
| 200 | today | **1.619 s** | 8.09 ms | 1.626 s | 99.6 % |
| 200 | unsynced | **14.04 ms** | **70.2 µs** | 19.64 ms | 71.5 % |
| 1 000 | today | **9.912 s** | 9.91 ms | 9.970 s | 99.4 % |
| 1 000 | unsynced | **94.20 ms** | **94.2 µs** | 128.96 ms | 73.0 % |

A junction at `N` = 1 000 goes from **9.97 s to 129 ms**: seventy-seven
times shorter. `06`'s figure for the same cell by the same method was
9.633 s of post C in a 9.685 s window, which this reproduces to **2.9 %**
across two campaigns, two weeks and two processes.

**Post C does not stop being the largest post.** It stops being 99.5 % of
a junction and becomes 73 % of a junction seventy-seven times shorter.
That distinction matters for whatever comes after S0 and is taken up in
the `F` list.

## Debt this leaves behind: none

**Nothing under `herd-hub/src`, `herd-runner/src`, `herd-proto/src` or
`herdctl/src` was touched.** `git diff origin/main --stat` on this branch
shows one added test file, this document and a handoff. There is no
`#[cfg(test)]` hook to remove later and no behaviour that differs between
a measured build and a shipped one.

That was possible because `record_waiting` is a **public** `JobStore`
method, so the hub's own write path is timed from outside, and because
`durable::write_file` and `durable::append_line` are public too, so the
synchronised control runs through the hub's own primitives rather than
through a lookalike.

## The method: three paths, not two

A two-path experiment — the store against a replica with the syncs
removed — cannot tell the cost of the syncs from the cost of the replica
being a replica. So there are three:

| path | what it is |
|------|------------|
| **store** | `JobStore::record_waiting`, what the hub does today |
| **replica, synced** | the same sequence rewritten in the test file, calling `durable::write_file` then `durable::append_line` — every sync kept |
| **replica, unsynced** | the same replica with the three syncs removed: write a temporary, `rename` it over `job.json`, append the event line bare |

The synced replica is the control. If it lands on the store's figure,
then what separates the two replicas is the syncs and nothing else. It
does: **8.07, 8.23, 9.07, 8.51 ms per job against the store's 8.36,
10.75, 8.80, 9.72** over the four cell/run pairs below — inside the
store's own run-to-run spread. And under `strace` the two are not merely
close but identical: 2 `fsync` + 1 `fdatasync` each, per write.

The replica is `record_waiting` line for line (`store.rs:2007`): read the
record, return `false` unless the job is queued and the cause is news,
stamp `waiting_for` and `updated_at`, write the record, append the event.
Two differences, both stated rather than discovered later:

1. **It does not take the store's `Mutex<Live>`**, which cannot be taken
   from outside the crate. For a single-threaded pass that changes
   nothing, and the synced control proves it. For the live junction it
   means the unsynced row holds the store lock for less time than the
   real thing would — a difference that works *against* the measurement
   being conservative, and is listed in `I`.
2. **It re-declares `RecordFile` and `EventLine`**, which are private in
   `store.rs`. Every pass asserts the store reads its own record back
   and that exactly one `waiting` line was appended, so a replica that
   wrote the wrong bytes would fail rather than measure.

The unsynced write keeps the atomic rename. That is deliberate and is
the whole design claim being costed: rename is atomic, so a reader still
sees the whole old file or the whole new one. What is dropped is
survival of a power cut, not visibility. See "What `rename` alone
guarantees" below.

## The host

| what | this host |
|------|-----------|
| CPU | Intel i5-10600K, 6 cores / 12 threads |
| memory | 31 GiB |
| kernel | 6.18.46 |
| state root | `$TMPDIR` = `/tmp`, on `rpool/root` |
| filesystem | ZFS, `sync=standard`, `compression=zstd` |
| device | mirror of two SATA SSDs, **no SLOG** |
| build | `--release` |

The same host as `06`, which is why the baseline reproduces. Post C
today is device-bound there; **post C unsynced is not**, which changes
what transfers to another machine and is the subject of `H`-1.

## Phase S0a — the floor, with the syncs and without

Measured first, so no figure below is reported as a measurement when it
is smaller than the floor under it. Load before the phase: `4.03 3.91
3.05`. 100 samples each.

| primitive | synced | unsynced | ratio |
|-----------|--------|----------|-------|
| atomic replace of a 1 KiB record | 2.36 ms (iqr/med 1.6 %) | 33.7 µs (8.1 %) | **x70** |
| append one event line | 1.21 ms (3.8 %) | 3.7 µs (4.6 %) | **x327** |
| **one waiting write, floor** | **3.57 ms** | **37.4 µs** | **x95** |

The synced floor agrees with `06`'s: that campaign measured
`create`+`write`+`sync_all` at 1.19 ms and `sync_dir` at 6.6 µs
separately and `append`+`sync_data` at 1.20 ms, and predicted 2.39 ms;
measured here end to end through `durable::write_file`, including the
rename over an existing file and a directory with dirty metadata under
it, the record half is 2.36 ms rather than 1.20. The total, 3.57 ms,
lands on the 3.6 ms median `06` measured for a whole `record_waiting`.

## Phase S0b — a whole pass of `N` waiting writes, three ways

`06`'s phase 1d, run once per path, on a fresh corpus per path — the
first pass over a corpus is the news and the second is not, so each path
must meet an untouched queue to be doing the same work.

**Run 1**, load `3.57 3.82 3.03`:

| `N` | path | one write, median | iqr/med | whole pass | **per job** |
|-----|------|-------------------|---------|------------|-------------|
| 200 | store | 3.65 ms | 187 % | 1.672 s | **8.36 ms** |
| 200 | replica, synced | 3.66 ms | 186 % | 1.614 s | **8.07 ms** |
| 200 | replica, unsynced | 69.6 µs | 6.6 % | 14.10 ms | **70.5 µs** |
| 1 000 | store | 3.63 ms | 190 % | 8.799 s | **8.80 ms** |
| 1 000 | replica, synced | 3.62 ms | 181 % | 9.066 s | **9.07 ms** |
| 1 000 | replica, unsynced | 70.9 µs | 6.4 % | 71.82 ms | **71.8 µs** |

**Run 2**, an independent process, load `2.50 3.21 2.93`:

| `N` | path | one write, median | iqr/med | whole pass | **per job** |
|-----|------|-------------------|---------|------------|-------------|
| 200 | store | 3.59 ms | 2.9 % | 2.150 s | **10.75 ms** |
| 200 | replica, synced | 3.61 ms | 183 % | 1.647 s | **8.23 ms** |
| 200 | replica, unsynced | 102.3 µs | 2.3 % | 20.78 ms | **103.9 µs** |
| 1 000 | store | 3.62 ms | 159 % | 9.715 s | **9.72 ms** |
| 1 000 | replica, synced | 3.61 ms | 60 % | 8.515 s | **8.51 ms** |
| 1 000 | replica, unsynced | 73.4 µs | 4.2 % | 75.76 ms | **75.8 µs** |

Three things in these two tables beyond the headline:

1. **The unsynced write is not bimodal.** `06`'s central warning about
   post C is that one synchronised write has an `iqr/med` of 180 %,
   because ZFS groups `fsync`s into transaction groups, and that it must
   therefore be reported per pass and never per write. The unsynced
   write has an `iqr/med` of **2.3–6.6 %**: with no `fsync` there is no
   transaction group to land in, and the per-write figure becomes a
   figure. That is a second, independent sign that the syncs are what
   was being removed.
2. **D171's quiet path does not move**, and should not: 11.3–17.4 µs
   through the store, 9.2–16.6 µs through the unsynced replica. A pass
   with nothing new to say writes nothing by either path, so staging
   durability changes nothing about the case that is already free.
3. **With the syncs gone, the cost is no longer the disk.** The unsynced
   floor is 37.4 µs and the measured write is 70–104 µs. The difference
   is reading `job.json` and parsing it (the no-news path, 9–17 µs,
   is exactly that read) plus re-serialising it. A waiting write without
   `fsync` is a **JSON round trip with a rename on the end**, and that
   is what any further step would have to attack.

## Phase S0c — count the syncs, do not time them

The exit criterion of this step is a figure **and** a sync count of zero.
`06`'s method, extended to the three paths: the same case run twice with
a different number of writes, so the corpus cancels out of the
difference.

```sh
for p in store synced unsynced; do
  for k in 1 201; do
    for skip in "" HERD_SKIP_WRITES=1; do
      env HERD_PATH=$p HERD_WRITES=$k $skip \
        strace -f -c -e trace=fsync,fdatasync \
        target/release/deps/unsynced_write_cost-<hash> \
        --exact --ignored --nocapture syncs_per_waiting_write
    done
  done
done
```

| path | 201 writes | 201 jobs, no write | difference | **per write** |
|------|------------|--------------------|------------|---------------|
| store | 1 828 `fsync`, 406 `fdatasync` | 1 426, 205 | 402, 201 | **2 + 1** |
| replica, synced | 1 828, 406 | 1 426, 205 | 402, 201 | **2 + 1** |
| replica, unsynced | **1 426, 205** | 1 426, 205 | **0, 0** | **0 + 0** |

The same at `HERD_WRITES` = 1: the store and the synced replica each add
exactly 2 `fsync` and 1 `fdatasync` over the control; the unsynced
replica adds nothing at all, 26 and 5 either way.

Three readings of that table:

- **The exit criterion is met.** Zero and zero, by difference, on an
  identical corpus.
- **`06`'s count reproduces exactly.** Two `fsync` and one `fdatasync`
  per `record_waiting`, three syncs and not four, counted again on a
  different branch by a different process.
- **The synced replica is the store's own path**, to the syscall. That
  is the strongest form the control could take.

## Phase S0d — one real junction, the write swapped underneath it

`06`'s phase 2, run once per path: a hub on a loopback port, a real
`herd_runner::run` dialling it, the five posts read off the store's own
trace — and the waiting write served either by the store or by the
unsynced replica, chosen in the delegating store. Three junctions per
cell, medians. Load before the phase: `5.00 4.87 3.79`. Every cell made
exactly one `collections_owed`, exactly one `unfinished_jobs` and
exactly `N` `record_waiting`, so the window is fully accounted for.

| `N` | path | A | B | C | D | E | window |
|-----|------|---|---|---|---|---|--------|
| 200 | today | 2.82 ms | 2.76 ms | **1.619 s** | 746 ns | 8.08 ms | 1.626 s |
| 200 | unsynced | 1.83 ms | 1.74 ms | **14.04 ms** | 541 ns | 1.85 ms | **19.64 ms** |
| 1 000 | today | 13.57 ms | 9.62 ms | **9.912 s** | 762 ns | 24.44 ms | 9.970 s |
| 1 000 | unsynced | 12.81 ms | 10.31 ms | **94.20 ms** | 1.7 µs | 12.26 ms | **128.96 ms** |

The same as shares of the window:

| `N` | path | A | B | C | D | E |
|-----|------|---|---|---|---|---|
| 200 | today | 0.17 % | 0.17 % | **99.6 %** | 0.00005 % | 0.50 % |
| 200 | unsynced | 9.3 % | 8.9 % | **71.5 %** | 0.003 % | 9.4 % |
| 1 000 | today | 0.14 % | 0.10 % | **99.4 %** | 0.000008 % | 0.25 % |
| 1 000 | unsynced | 9.9 % | 8.0 % | **73.0 %** | 0.001 % | 9.5 % |

What the live junction adds to the pass-level figure:

1. **The baseline reproduces `06` to 2.9 %** at `N` = 1 000 (9.912 s
   against 9.633 s of post C; 9.970 s against 9.685 s of window) and to
   **1.0 %** at `N` = 200 (1.619 s against 1.603 s). Two campaigns, two
   branches, two processes. The baseline this experiment is measured
   against is not in doubt.
2. **The window collapses with post C**, because post C was the window:
   x83 at `N` = 200, x77 at `N` = 1 000.
3. **Posts A, B and E do not move.** 12.81 against 13.57 ms, 10.31
   against 9.62 ms, and post E 12.26 against 24.44 ms — the last
   difference is the hub's own `println!` of one `stays queued` line per
   job (`server.rs:3455`) landing on a pipe at a different rate, which
   `06` had already named as the likely bulk of live post E (`06` `H`-4).
   Nothing in the unsynced path makes a read cheaper, and nothing
   measured here says it does.
4. **Post D stays nothing**, 541 ns to 1.7 µs, as it must with one
   runner on a quiet hub.

## What `rename` alone guarantees — checked in the code, not assumed

The design argument is that an atomic replace keeps E-53's **visibility**
promise (`coverage.md:70`, "records readable during run") without
surviving a power cut, and that the precedent is already in the tree.
Taken back to the code at `f507ab6`, three of the four premises hold and
one is wrong in a way worth writing down.

**Confirmed — E-04's figures are in memory, not on the record.**
`coverage.md:21` says, in those words: "The figures a requester is told
are the last pass's, held in memory and not on the record, so a job
queued 74th of 184 is not still saying so when it is 3rd of 12." The
code agrees: `server::why_it_waits` (`server.rs:1598`) reads
`peers.waiting`, which is memory.

**Confirmed — a restarted hub has an empty table and rewrites.**
`decisions.md:8081-8083`: "A hub that restarts has an empty one, and its
first pass writes every queued job's reason once, which is right: a new
hub has told nobody anything."

**Confirmed, and stronger than claimed — the precedent is in the tree,
twice.** `durable::append_bytes` (`durable.rs:74-83`) says exactly the
bargain: "A log is a stream, not a record: ... E-53 asks only that a
reader can follow it. The bytes are visible to any reader as soon as this
returns; `sync_file` is what makes them survive the machine." The closer
precedent is the one the brief did not name: `write_fixed_in_place`
(`durable.rs:85-109`), which writes a **record-like** value — the log
chunk counter — deliberately without a sync, and spells out the trade in
full: "The value is in the page cache when this returns, which is what a
killed hub needs: the kernel keeps it. A machine that loses power can
lose it, and then the hub has no number to compare with — which it says,
rather than assuming nothing is missing." That is the same bargain
applied to a record and not to a log, and it is already shipped.

**Wrong — the record is read, and the code says so in as many words.**
The premise that nothing depends on `waiting_for` surviving does not
survive reading `server.rs:1588-1599`:

> Why a queued job is waiting, with the figures the **last pass** found
> (E-04, D171). Preferred over the record's own `waiting_for` everywhere
> a requester is told ... `None` for a job this hub has not judged since
> it started, **and then the record is the only answer there is**.

And it is used as that fallback in two places: the restatement clock
(`server.rs:3336`, `state.why_it_waits(&job_id).or(record.waiting_for)`)
and a wait attaching to a queued job (`server.rs:6323-6325`). A test
states the trade on purpose (`server.rs:7529-7538`): "the record carries
the sentence that held when the cause was news, which is what a hub that
restarts reads back".

So the honest statement is **not** "the record is never relied on". It is:

> The record's `waiting_for` is a **stale fallback of last resort**,
> already acknowledged stale by design, read only between a hub's start
> and that hub's first pass over that job. An unsynchronised write puts
> one more thing in that window: after a *power cut* specifically, the
> fallback may be one cause behind, or absent for a job that had never
> had a wait written. After a `kill -9`, nothing changes at all — the
> page cache is the kernel's and it survives the process.

That is a smaller loss than the premise assumed it was avoiding, and it
is bounded by the same window the design already calls stale. It does
not overturn the recommendation. It does mean the contract wording for
any durability staging has to describe that window rather than claim
there is none.

**One ordering invariant to check before shipping, not after.** D103, in
`coverage.md:70`: "`job.json` is the authority on where a job stands now;
`events.jsonl` is the history and may be one transition behind it,
**never ahead**." Today the order is enforced by the two syncs: the
record is on the disk before the event line is appended. With both syncs
gone, the ordering after a power cut is the filesystem's business. On
ZFS it is safe — transaction groups commit atomically and in order, so a
later write cannot be durable while an earlier one is not — and ZFS is
what this host and `06`'s host run. On a filesystem without that
property there is no such guarantee, and `events.jsonl` could come back
one transition **ahead**. This is an `H`, with what would settle it, and
it is about the server's state root and not about this bench.

## Reproducibility, and the residual noise

The noise is characterised before the signal, as `06` did it. Two
independent runs of phase S0b, and two of phase S0d's `N` = 1 000
unsynced cell, in four separate processes.

Phase S0b, run 1 against run 2, per job:

| `N` | store | replica, synced | replica, unsynced |
|-----|-------|-----------------|-------------------|
| 200 | 8.36 → 10.75 ms (**+28.6 %**) | 8.07 → 8.23 ms (+2.0 %) | 70.5 → 103.9 µs (**+47.4 %**) |
| 1 000 | 8.80 → 9.72 ms (+10.5 %) | 9.07 → 8.51 ms (−6.2 %) | 71.8 → 75.8 µs (+5.6 %) |

Phase S0d, `N` = 1 000, unsynced, two runs: post C **77.87 ms** (77.9 µs
per job, window 108.46 ms) and **94.20 ms** (94.2 µs per job, window
128.96 ms) — 21 % apart.

**The margin, stated plainly:**

- **Every independent measurement of the unsynced per-job cost lies
  between 70.2 µs and 103.9 µs**: 70.5, 71.8, 103.9, 75.8 from phase
  S0b; 70.2, 77.9, 94.2 from phase S0d. Seven figures, four processes,
  two methods.
- **The decision does not depend on which of them you take.** The worst
  is 103.9 µs, which is **9.6 times under** the 1 ms threshold. The
  spread would have to be wrong by an order of magnitude to change the
  answer, and it reproduces to 5.6 % at the deepest cell.
- **The threshold is never close.** Had the brief set it at 100 µs
  instead of 1 ms, this report would have said "too close to call on
  this host" — the spread, 70–104 µs, straddles it. At 1 ms there is
  nothing to argue about.
- **The baseline's own noise is the larger of the two.** The store path
  moves 10–29 % between runs, which is `06`'s finding about ZFS
  transaction groups showing again; the unsynced path at `N` = 1 000
  moves 5.6 %.
- **Nothing was run above load 5.7.** `/proc/loadavg` was read before
  every phase and is quoted with it. No `borgbackup` run was seen during
  this campaign; had one started, the phase would have been abandoned
  and said so, which is what `06`'s agent did.

## What is `F`, what is `H`, what is `I`

**`F` — established, with the figure.**

1. **An unsynchronised waiting write costs 70–104 µs per job**, median
   per-write 69.6–102.3 µs, against **8.07–10.75 ms per job** for the
   write the hub makes today. **x90 to x120.** (Phases S0b and S0d,
   seven independent measurements, four processes, two methods.)
2. **That is ten times under the 1 ms threshold at its worst reading.**
   The premise the C4 recommendation rests on is **supported**, and the
   margin is large enough that this host's device does not decide it.
3. **The unsynchronised path makes zero `fsync` and zero `fdatasync`**,
   counted under `strace` by difference on an identical corpus, against
   2 `fsync` + 1 `fdatasync` for the path the hub uses today. (Phase
   S0c. `06`'s count of three syncs reproduces exactly.)
4. **A junction at `N` = 1 000 goes from 9.97 s to 129 ms**, and at
   `N` = 200 from 1.626 s to 19.6 ms: x77 and x83. (Phase S0d, a real
   runner dialling a real hub.)
5. **Post C is still the largest post afterwards** — 71–73 % of the
   window, against 99.4–99.6 % today. It stops being the whole junction;
   it does not stop being the biggest piece of a junction that is
   seventy-seven times shorter. (Phase S0d.)
6. **What remains in an unsynchronised waiting write is not the disk.**
   The unsynced I/O floor is 37.4 µs and the measured write is 70–104
   µs; the difference is reading and re-serialising `job.json`, of which
   the read alone is the 9–17 µs the no-news path costs. Any step after
   S0 aims at JSON, not at the device. (Phases S0a, S0b.)
7. **An unsynchronised write is not bimodal**: `iqr/med` 2.3–6.6 % per
   write, against 159–190 % for the synchronised one. `06`'s rule —
   report post C per pass, never per write — stops being necessary once
   the `fsync` is gone. (Phase S0b.)
8. **D171's quiet path is unaffected**, 9–17 µs either way. A pass with
   nothing new to say was free and stays free. (Phase S0b.)
9. **The baseline reproduces `06` across campaigns**: post C at
   `N` = 1 000 is 9.912 s here against 9.633 s there (+2.9 %), and at
   `N` = 200, 1.619 s against 1.603 s (+1.0 %). (Phase S0d.)
10. **The record's `waiting_for` is read after a restart, by design and
    in the code**: `server.rs:1596` ("then the record is the only answer
    there is"), used at `server.rs:3336` and `:6323`. The premise that
    nothing depends on the written value is false; the true statement is
    narrower and is in "What `rename` alone guarantees".
11. **The in-tree precedent for an unsynchronised record is
    `write_fixed_in_place` (`durable.rs:85-109`)**, not only
    `append_bytes`: it is a record-like value written deliberately
    without a sync, with the page-cache/power-cut trade written out.

**`H` — a hypothesis, with what would confirm it.**

1. *The unsynced figure transfers to another host far better than the
   synced one does.* Post C today is device-bound (`06` proved it:
   debug/release x1.00); post C unsynced is CPU and page cache, so an
   NVMe host should show the synced figure fall by an order of magnitude
   and the unsynced figure barely move. **Confirmed by** running phases
   S0a and S0b with `TMPDIR` on the NixOS server's state root and on
   `tmpfs`.
2. *With both syncs gone, `events.jsonl` can come back one transition
   ahead of `job.json` after a power cut on a filesystem that does not
   commit in order* — which D103 (`coverage.md:70`) forbids. Not
   reachable on ZFS. **Confirmed by** naming the filesystem of the
   server's state root, and, if it is not ZFS, by a crash test (`dm-log-
   writes`, or a VM with its disk cache dropped) on the two files.
3. *An unsynchronised rename widens the window in which a `.job.json.*`
   temporary survives a crash inside a job directory.* The hazard exists
   today — a crash between `File::create` and `rename` leaves one either
   way — but the window grows. `store.rs:2928` asserts a job directory
   holds exactly five names. **Confirmed by** the same crash test, or
   refuted by showing every reader of a job directory filters by name.
4. *Live post E is the hub's `println!`.* 24.44 ms against 12.26 ms for
   the same computation in the same cell, differing only in how fast the
   junction around it ran. This is `06`'s `H`-4 seen from a second
   angle. **Confirmed by** `06`'s own proposal: the same junction with
   stdout to `/dev/null` against a pipe.

**`I` — could not be decided here.**

1. **What the replica's missing lock is worth on a live hub.** The
   unsynced replica does not hold `Mutex<Live>` across its two writes,
   because the lock is private. On a single-runner junction post D is
   under 2 µs and it cannot matter; on a hub with two junctions and a
   watchdog, a real implementation holding the lock for 70 µs instead of
   8 ms is a different contention picture from either thing measured
   here. `06`'s phase 3 is the experiment to rerun, and this campaign
   did not.
2. **Whether the contract can be written for the window named above.**
   That a power cut may cost one stale `waiting_for` in the interval
   between a hub's start and its first pass is a measurement-free
   question about what E-04 promises. It is Alexa's, not this file's.
3. **What any of this costs on the server the hub actually runs on.**
   Every figure here is one workstation, one filesystem, one device.
   Unlike `06`'s, the figure that matters here is *not* device-bound,
   which makes it more portable — but "more portable" is not "measured
   there".
4. **The cost of a pass that places jobs.** As in `06`: every job in
   this corpus needs a capability nothing offers, so every pass writes a
   wait and nothing is dispatched. A pass that *places* pays
   `begin_attempt` and `record_state`, which are different writes and
   are not staged by anything measured here.

## What I judge wrong in the brief

Said plainly, because the brief asked for it.

1. **"Le poste dominant s'effondre" is true and misleading.** Post C
   falls by a factor of a hundred, and it is **still 73 % of the
   junction** afterwards. A reader of that phrase would expect post C to
   stop mattering; what the measurement says is that it stops being
   99.5 % of a nine-second event and becomes 73 % of a 129 ms one, and
   that what is left of it is JSON rather than the disk. The difference
   matters for whatever S1 is.
2. **The premise that nothing reads the written `waiting_for` is
   false**, and it is the brief's own `E-04`/`E-53` question that
   surfaced it. `server.rs:1596` says the record "is the only answer
   there is" for a job the hub has not judged since it started, and two
   call sites use it as the fallback. The conclusion survives; the
   sentence does not. This is the most important thing in this report
   after the number itself.
3. **The threshold is well chosen but was never in play.** At 1 ms the
   answer is unambiguous by a factor of ten. At 100 µs it would have
   been undecidable on this host: the seven measurements span
   70–104 µs. Worth knowing before the next threshold is set.
4. **`durable.rs:74-83` is exact — and it is not the best citation.**
   `append_bytes` is there, to the line. The stronger precedent is
   `write_fixed_in_place` at `durable.rs:85-109`, which applies the same
   bargain to a *record-like* value and not to a log, and which the
   brief did not mention. Cite both; the second is the one that answers
   "has this trade been made in this tree before, for something that is
   not a stream".
5. **"Mesure à `N` >= 200" was right and is worth keeping.** At
   `N` = 200 the *unsynced* figure still moved 47 % between runs
   (70.5 → 103.9 µs) while `N` = 1 000 moved 5.6 %. `06`'s rule about
   shallow cells applies to the cheap path too, for a different reason:
   two hundred writes of 70 µs is 14 ms, which is short enough for a
   scheduler hiccup to be a third of it.
6. **Everything else in the brief that could be checked, checked.**
   Three syncs, two `fsync` and one `fdatasync`: exactly. 9.633 s at
   `N` = 1 000 and 189 ms at `N` = 50: the first reproduces to 2.9 %,
   the second was not attempted because `06` says not to quote it.
   `coverage.md:21`, `coverage.md:70` and `decisions.md:8081-8086` say
   what the brief says they say, word for word.

## How to reproduce

The bench is `herd-hub/tests/unsynced_write_cost.rs`, derived from
`herd-hub/tests/junction_breakdown.rs`. Every case is `#[ignore]`d, so
`cargo test --workspace` does not run it.

```sh
# phase S0a, the floors                                  ~1 s
nix develop --command cargo test -p herd-hub \
  --test unsynced_write_cost --release -- \
  --ignored --nocapture --test-threads=1 sync_floor

# phase S0b, a whole pass of N writes, three ways        ~110 s
... --ignored --nocapture --test-threads=1 whole_pass_three_ways

# phase S0d, one real junction, two paths, 3 rounds      ~210 s
HERD_ROUNDS=3 ... --ignored --nocapture --test-threads=1 \
  one_real_junction_unsynced
```

`HERD_CELLS=200,1000` sets the queue depths, `HERD_T` the closed
directories beside them, `HERD_ROUNDS` the junctions per cell of phase
S0d. Phase S0c is the `strace` loop quoted in its own section; the
`HERD_PATH` variable selects `store`, `synced` or `unsynced`.

**`--test-threads=1` is not a detail**, for `06`'s reason: two cases of
this file at once would contend for the disk and for the page cache.
