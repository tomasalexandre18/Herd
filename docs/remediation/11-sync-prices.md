# 11 — The three `fsync`s of `record_waiting`, priced separately

**This is a mesure**, in the sense `README.md` gives the word: it ran on
the shared workstation, in its own worktree, under `nix develop`, in
`--release`, with `/proc/loadavg` recorded before and after every phase.
It changed nothing and it touches no file under `herd-hub/src`. Its
figures are this host's and the limits of what they transfer are named at
the end.

**Item:** ORCH-077. **Base:** `origin/main` at `2d177eb`.

---

## 1. What this settles, in four lines

1. **One `fsync` on ext4/NVMe costs 989 µs**, not the tens-to-low-hundreds
   of microseconds that was hoped for — three to fifty times above that
   range, and only **1.2x** better than the 1.19 ms `08` derived on
   ZFS/SATA (1.3-1.7x better than this harness's own ZFS figures). The
   hypothesis this task was set to test is **refuted**.
2. **The three syncs are additive, not redundant.** Each costs about the
   same whether it is the only sync or the last of three, on both
   filesystem classes. No sync pays for another.
3. **Keeping one sync and dropping two collects 65 % of the prize, not
   "nearly all" of it** — 64.3 % on ZFS and 64.5 % on ext4, which is just
   "two syncs out of three".
4. **`08`'s 9.970 s junction at `N` = 1 000 is about 3x too large for the
   class that matters, not an order of magnitude.** The same pass on
   ext4/NVMe is **3.128 s**. It is smaller, and it is still seconds.

---

## 2. The question, and why a block figure could not answer it

`08-s0-unsynced-write-cost.md` priced the syncs as a block: all three on
against all three off, **x95** apart, and a junction at `N` = 1 000 going
from 9.97 s to 129 ms. That is the size of the prize. It is not a guide to
collecting it, because the change under discussion keeps **one** of the
three syncs and drops the other two, and a block figure cannot say what
one sync costs.

It could not be assumed either, for a specific reason: three syncs against
one journalling filesystem are not obviously three independent costs. The
first call can commit what the second would otherwise have had to write,
in which case the second is nearly free and dropping it buys nothing. So
the three had to be measured **in every combination**, not just once each.

One waiting write performs exactly three durable operations, named here by
the lines that perform them:

| code | operation | where |
|---|---|---|
| **R** | `file.sync_all()` on the record's temporary, before the rename | `durable.rs`, `write_file_with` |
| **D** | `sync_dir(parent)`, after the rename | `durable.rs`, `write_file_with` |
| **E** | `file.sync_data()` on the appended event line | `durable.rs`, `append_line` |

`record_waiting` (`herd-hub/src/store.rs:2020`) performs R and D through
`write_record`, then E through `append_event`, in that order.

## 3. The host, and the two cells

Both classes were measured on **one host, by one harness, in one
session**, which is the part that is easy to get wrong: a figure from this
machine and a figure from `08`'s campaign differ by filesystem *and* by
whatever else moved in between, while the two rows below differ only by
`$TMPDIR`.

| what | this host |
|------|-----------|
| CPU | Intel i5-10600K, 6 cores / 12 threads |
| memory | 31 GiB |
| kernel | 6.18.46 |
| build | `--release` |

| cell | `$TMPDIR` | filesystem | device |
|---|---|---|---|
| **ZFS** | a subdirectory of `/tmp`, on `rpool/root` | ZFS, `sync=standard`, `compression=zstd` | mirror of two SATA SSDs (Toshiba THNSNJ128GCST + THNSNF128GCSS), **no SLOG** |
| **ext4** | a subdirectory of the agent's scratchpad, on `/mnt/ssd_980` | ext4, `rw,relatime,stripe=32`, `has_journal` | Samsung SSD 980 1 TB, NVMe |

The ZFS cell is `08`'s and `06`'s cell, deliberately: it is what ties
these figures to theirs. The ext4 cell is the class the deployment
actually runs on — ORCH-053, answered by the owner and corroborated by the
first consumer's own evidence from the wedged hub.

Each phase prints the mount it landed on, so a pasted run says for itself
which class it describes.

## 4. Method

`herd-hub/tests/sync_prices.rs`, derived from `08`'s
`unsynced_write_cost.rs`, whose corpus, spread and statistics are reused
unchanged so a figure here and a figure there compare without
reinterpretation.

- The eight subsets of `{R, D, E}` are a **replica** of `record_waiting`,
  written in the test, over the same two files in the same order with the
  same bytes, with each sync independently skippable. The rename is never
  skipped: atomicity is the visibility guarantee and is not one of the
  three costs.
- **`RDE` is the control.** Every sync on must land on the store's own
  `record_waiting`. Where it does not, the cell is reported as failed and
  not as a figure — which happened once, see §8.
- A **fresh corpus per subset**: the first waiting write on a job is the
  news and a second with the same cause is a no-op, so every subset meets
  an untouched queue.
- Nothing under `herd-hub/src` is touched. The hub's path is timed from
  outside through the public `JobStore` trait.

## 5. Phase 1 — each sync call on its own

The write that precedes each sync is kept **outside** the timer, which
`08` could not do: it reported `write`+`sync_all` composed at 1.19 ms and
so could not separate the syscall from the write under it. 200 samples
(ZFS: 400).

**ext4 on NVMe**, load `0.77 1.39 1.42`:

| sync call | median | iqr/med |
|---|---|---|
| **R** record `fsync`, `sync_all` on a 1 KiB file | **989.1 µs** | 7.3 % |
| **D** directory `fsync`, in the real order (after R) | **991.1 µs** | 3.9 % |
| **D** directory `fsync`, with no R before it | 968.5 µs | 2.2 % |
| **E** event `fdatasync` on an appended line | **983.4 µs** | 3.3 % |

**ZFS on the SATA mirror**, load `0.73 1.35 1.41`, 400 samples:

| sync call | median | iqr/med |
|---|---|---|
| **R** record `fsync` | **1.65 ms** | 102.9 % |
| **D** directory `fsync`, after R | **1.32 ms** | 117.4 % |
| **D** directory `fsync`, with no R before it | 1.47 ms | 87.4 % |
| **E** event `fdatasync` | **1.28 ms** | 4.5 % |

Two things to read here.

**The ext4 figures are flat and tight** — three syncs within 2 % of each
other at iqr/med under 8 %. Three sync calls of the same size costing the
same is the signature of a fixed per-flush device cost, not of the work
each one has to do. The Samsung 980 is a DRAM-less consumer drive, and
roughly 1 ms to force a cache flush to NAND is an ordinary figure for one.

**The ZFS figures are bimodal and must be read per pass**, which is `06`'s
and `08`'s standing warning about this class: an `fsync` either lands in an
open transaction group or waits for the next one, so iqr/med runs past
100 % and the median of a per-call sample is not a cost. §7 is the figure
to use for ZFS.

### A correction to `06`, which `08` noticed and did not diagnose

`06` measured `sync_dir` at **6.6 µs** on ZFS and `create`+`write`+
`sync_all` at 1.19 ms, and predicted 2.39 ms for the pair. `08` then
measured the record half end to end at **2.36 ms** rather than the
predicted 1.20 ms, and recorded the gap without explaining it.

The gap is D, and the mechanism is visible in `06`'s own loop
(`herd-hub/tests/junction_breakdown.rs:579-592`). It creates a file,
`sync_all`s it, and then syncs the directory — **with no `rename` in
between**. That file sync has already committed the only change the
directory had, so the directory sync that follows finds **nothing owed
under it** and returns in microseconds.

`durable::write_file_with` does something different: it syncs the
temporary, **then renames it over the destination**, and only then syncs
the directory. The rename is a directory change made *after* the file
sync, so the real `sync_dir` always has dirty metadata to commit and
always pays a flush.

Measured in the position the code actually puts it in, D is **1.32 ms on
ZFS and 991 µs on ext4** — about two hundred times `06`'s figure. The
paired cells in §5 show it is the rename and not the ordering that costs:
a directory sync with no record sync before it is 1.47 ms on ZFS and
968 µs on ext4, within noise of the sync that follows one.

So `06`'s per-primitive `sync_dir` figure should not be used to size D,
and any arithmetic that treats the directory sync as free is wrong by
about a third of a waiting write.

## 6. Phase 2 — all eight subsets of one waiting write

60 samples per subset on ext4; 400 on ZFS, because 60 is not enough
against the transaction-group bimodality (§8).

**ext4 on NVMe**, load `0.77 1.39 1.42`:

| subset | kept | one waiting write, median | iqr/med |
|---|---|---|---|
| `---` | 0 | **51.2 µs** | 3.0 % |
| `R--` | 1 | **1.10 ms** | 3.6 % |
| `-D-` | 1 | 1.09 ms | 14.9 % |
| `--E` | 1 | 1.01 ms | 9.6 % |
| `RD-` | 2 | 2.10 ms | 6.1 % |
| `R-E` | 2 | 2.07 ms | 5.2 % |
| `-DE` | 2 | 2.00 ms | 3.9 % |
| `RDE` | 3 | **3.00 ms** | 1.2 % |
| store `record_waiting` | 3 | **3.11 ms** | 3.3 % |

**ZFS on the SATA mirror**, load `0.73 1.35 1.41`, 400 samples:

| subset | kept | one waiting write, median | iqr/med |
|---|---|---|---|
| `---` | 0 | **76.5 µs** | 5.3 % |
| `R--` | 1 | **1.46 ms** | 65.9 % |
| `-D-` | 1 | 1.56 ms | 105.4 % |
| `--E` | 1 | 1.67 ms | 90.7 % |
| `RD-` | 2 | 2.65 ms | 9.5 % |
| `R-E` | 2 | 2.75 ms | 62.3 % |
| `-DE` | 2 | 3.00 ms | 163.2 % |
| `RDE` | 3 | **3.97 ms** | 65.9 % |
| store `record_waiting` | 3 | **4.14 ms** | 172.9 % |

**The control holds in both cells**: `RDE` 3.00 ms against the store's
3.11 ms on ext4, 3.97 ms against 4.14 ms on ZFS. The replica is the same
experiment as the hub.

### The syncs are additive

Each sync's marginal price, measured two ways — added to nothing, and
added last to the other two:

| sync | ext4: alone | ext4: added last | ZFS: alone | ZFS: added last |
|---|---|---|---|---|
| **R** record `fsync` | 1.05 ms | 1.00 ms | 1.39 ms | 974 µs |
| **D** directory `fsync` | 1.04 ms | 930 µs | 1.48 ms | 1.22 ms |
| **E** event `fdatasync` | 963 µs | 907 µs | 1.59 ms | 1.32 ms |

On ext4 the two columns agree within 11 %, and the three marginals sum to
3.05 ms against a measured `RDE` of 3.00 ms. **The three syncs are three
independent costs.** The mechanism that would have made the later syncs
cheap — the first commit paying for what the second would have had to
write — is not operating at this write size on either class. On ZFS the
"added last" column is 15-30 % below the "alone" column, so there is a
little sharing there, but nothing like the collapse that would make
dropping D and E free.

This is the result that decides the shape of the recommendation, and it is
the one that could not be guessed from `08`.

## 7. Phase 3 — a whole pass, comparable with `08`'s junction

The per-write medians above are not the figure to multiply by `N`: on ZFS
the distribution's mean sits well above its median, which is why `06` and
`08` both insist on reporting the pass. This phase times the whole pass end
to end, which is what `08`'s **9.970 s at `N` = 1 000** is.

`N` = 1 000, one fresh corpus per subset:

| cell | `RDE` (today) | `R--` (keep the record sync) | `---` (no sync) |
|---|---|---|---|
| **ext4 on NVMe**, load `1.07 1.34 1.41` | **3.128 s** | **1.085 s** | 52.85 ms |
| **ZFS/SATA**, load `1.42 1.41 1.43` | **6.949 s** | **2.150 s** | 78.61 ms |

The ZFS row reproduces `08`'s S0b in order and in magnitude — `08` had
8.5-9.7 s for the synced pass and 72-76 ms unsynced, against 6.9 s and
79 ms here, the difference being load and ZFS state, not method.

## 8. What `08`'s 9.97 s becomes, and what the recommendation collects

**The junction figure.** `08`'s 9.970 s at `N` = 1 000 was measured on
ZFS/SATA. On the class the deployment runs on, the same pass is
**3.128 s** — a factor of **3.2**, not the order of magnitude that was
suspected. So:

- the plan's advice to **stop quoting 9.97 s for the deployment** is
  right, and the correction is **3x, not 10x**;
- the problem `08` identified **does not go away on ext4**. Three seconds
  of durable writing in one dispatch pass at `N` = 1 000 is still the
  dominant cost of that pass by a wide margin, and still seconds.

**The recommendation.** Keeping R and dropping D and E:

| cell | today | keep R only | no sync | prize collected |
|---|---|---|---|---|
| ext4, per write | 3.00 ms | 1.10 ms | 51.2 µs | 1.90 of 2.95 ms = **64.5 %** |
| ext4, pass at `N` = 1 000 | 3.128 s | 1.085 s | 52.85 ms | 2.043 of 3.075 s = **66.4 %** |
| ZFS, per write | 3.97 ms | 1.46 ms | 76.5 µs | 2.50 of 3.89 ms = **64.3 %** |

**Keeping one of three additive syncs collects two thirds.** It is not
"nearly the whole prize": the kept sync still costs **1.09 ms, which is
21x the 51 µs unsynced floor**, and it alone is 35 % of what the write
costs today.

That is not an argument against the increment — a third off is a real
third off, and it keeps an integrity guarantee the unsynced path does not
have. It is an argument against pricing the increment as if it were nearly
free, and against any plan whose first step is justified by the claim that
one sync is cheap on NVMe. On this host it is not: it is 989 µs.

## 9. What could not be measured, and the weaker evidence that stands in

**Nothing here proves the device honours a flush, and a drive that lies
defeats every figure above.** What can be said, and no more:

- The ext4 mount carries **no `barrier=0`**: its options are
  `rw,relatime,stripe=32` verbatim from `/proc/mounts`, and ext4's default
  is `barrier=1`, so **write barriers are enabled by configuration**. The
  filesystem has `has_journal`.
- ZFS is at `sync=standard`, so an `fsync` is honoured rather than
  discarded.
- All three devices report `write back` in
  `/sys/block/*/queue/write_cache` — volatile caches, enabled — so every
  figure above depends on a `FLUSH` being obeyed. The NVMe reports
  `fua 1`; the two SATA SSDs report `fua 0`.
- **`nvme-cli` is not installed on this host**, so the drive's volatile
  write cache feature could not be queried directly, and no tool available
  here can establish that the controller obeys a flush rather than
  acknowledging it.

The weaker evidence that the flushes are real is the **shape of the
numbers**: 989 µs for a 1 KiB `fsync` on an NVMe drive capable of writing
far more than 1 KiB in a microsecond is only explicable as a device round
trip, and the 51 µs unsynced floor on the same path shows what the same
write costs when no flush is asked for — a 19x gap in the right direction.
A drive that ignored flushes would have collapsed that gap. That is
consistent with the flushes being honoured; it does not prove it.

Also **not measured here**, and not claimed:

- **D103's ordering on ext4 after a crash** — whether `events.jsonl` can
  end up ahead of `job.json` on this filesystem. That is ORCH-074's
  question and it needs a crash test (`dm-log-writes`), not a stopwatch.
  This piece prices the syncs; it says nothing about which of them the
  ordering needs.
- **The consumer's own device.** Their state root is ext4 on NVMe, which
  is this cell's class, but it is not this cell's drive. A per-flush cost
  is a property of the drive, so these figures size the class and not
  their machine. An enterprise NVMe with power-loss protection can be ten
  times faster per flush than this one; a worse consumer drive can be
  slower.
- **Concurrency.** Every figure is single-threaded, `--test-threads=1`.
  What these syncs cost when two runners junction at once is `06`'s lock
  question and is not reopened here.
- **Any other write path.** Only `record_waiting` was priced. The hub's
  other durable writes use the same primitives and so presumably the same
  per-flush cost, but that is an inference, not a measurement.

**One cell is reported as failed rather than as a figure.** The first ZFS
round, at 60 samples per subset and under a load of `3.35` left over from
this agent's own compile, produced `R--` at 4.85 ms *above* `RDE` at
4.07 ms, and a control that missed badly — replica 4.07 ms against the
store's 10.58 ms. Sixty samples do not survive the transaction-group
bimodality. The round was discarded and re-run at 400 samples under a load
of `0.73`, where the control holds to 4 %. The ext4 figures were also
taken twice, under load `4.44` and load `0.77`, and agree to within 3 %
on every row — that class is flat enough not to care.

## 10. What this piece does not do

It authorizes nothing. It is a measurement, and per `README.md` no piece
of this dossier makes a change to the software or turns a recommendation
into a plan. It removes one unmeasured premise from a decision that is
Alexa's, and it leaves ORCH-074's crash-ordering question exactly where it
was.

## Reproducing it

```sh
# ext4 on NVMe
TMPDIR=<a dir on ext4> nix develop --command cargo test -p herd-hub \
  --test sync_prices --release -- --ignored --nocapture --test-threads=1
# ZFS, where the per-write medians need many more samples
HERD_SAMPLES=400 TMPDIR=<a dir on zfs> nix develop --command cargo test \
  -p herd-hub --test sync_prices --release -- --ignored --nocapture \
  --test-threads=1
```

`HERD_SAMPLES` overrides the sample count of whichever phase runs;
`HERD_CELLS` overrides phase 3's queue depths, as in `08`.
