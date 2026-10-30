# The hub's disk policy

What bounds the hub's disk, what the first consumer's figures say about it,
and what M12 still owes. It closes the debt D148 wrote to itself — *"the
one measurement that would change it is their distribution of output size
per job per day"* — and it sizes M12's record retention against the same
figures.

**Five voices, kept apart on every line.** `[C]` is **code**, with a file
and a line, and it is the only voice that is checkable here. `[R]` is
**reported** by the first consumer or by Alexa's breakdown of their
figures: theirs, not ours, and **unverified by this project** — nobody
here has looked at their machines and nobody will (invariant 9, D161).
`[D]` is **derived**, and every derived number names the inputs it came
from. `[P]` is a **proposal**: mine, with its reasoning, decided by nobody
yet. `[M]` is **measured by this project on this workstation**, against a
fixture store this project built and never against anything of the
consumer's; a measured figure names the run that produced it. `[M]` was
added on 2026-10-28 by ORCH-104, which replaced two `[D]` figures in §2.5
and §3.4 with measurements; the four voices above it are unchanged.

The reader this is written for arrives from `docs/plan.md` M12 or from
`docs/coverage.md` E-15. It is a topic document beside
`docs/runner-confinement.md`, not a decision and not a journal: when the
decisions in §6 are taken, they become numbered entries in
`docs/decisions.md` and this file keeps the arithmetic behind them.

**§0 is the contract and it was re-cut on 2026-10-29** (ORCH-170), when
Alexa moved this subject from a calendar to space and turned
deduplication on by default. §1 to §7 are the arithmetic behind it and the
record of how it was reached: where a section was written against the old
direction, it says so and points back at §0. Two of its findings are
retired by that move and are marked retired where they stand rather than
deleted, because the arithmetic that produced them is the arithmetic
somebody will redo.

---

## 0. The direction of 2026-10-29: space before age

Alexa re-cut this document's subject on 2026-10-29. **Deduplication is on
by default, and what is deleted is decided by space rather than by a
calendar.** Everything below §1 was written against a calendar, and §3.3
stated the reverse of the new rule; this section is the contract, and the
sections it corrects say so where they stood.

**Read this section before §1.** The rest of the document is the arithmetic
behind it and the record of how it was reached — including, deliberately,
two findings this direction retires. A finding that is retired is marked
where it stands rather than deleted, because the arithmetic that produced it
is still the arithmetic someone will redo.

**Every figure about the first consumer's volume in this section is their
reading of 2026-10-29, relayed to us and carried forward** — rows R10 to
R20 of §1.2. It is a reading of their past, not a measurement of the
future, and nobody in this project has looked at their machines or will
(invariant 9, D161).

### 0.1 The direction, in four sentences

1. **Deduplication is where the space is.** It destroys nothing: one
   `link(2)` per arriving file, and the 5.04 ratio read on the consumer's
   own volume says four fifths of their collected bytes are second and
   later copies `[R12]`.
2. **Space decides, not a timer.** The trigger is bytes held against a
   ceiling, and free space on the filesystem behind it. A span is a guard,
   never the reason something goes.
3. **Age is demoted, not deleted.** D201's class map — merger 90 days,
   every other class 14, active by default — survives **unchanged as a
   maximum age**: the oldest a record is ever allowed to get, not when it
   is deleted. What loses its primacy is the timer, not the numbers. The
   easy mistake is to read "space before age" as "no age"; it is not.
4. **Keep only what still serves — and nothing is kept without a bound.**
   Free space is a burst cache, so bytes that serve nothing cost
   throughput and not merely room. What can no longer serve is removed
   after a stated delay, or moved to an archive path on another disk if
   the operator gives one. Setting something aside under a name that says
   so is a **transitional state**, never an end state.

### 0.2 Which mechanism owns which bytes

**One table a reader can act on.** Four populations, four owners, and
the shares are measured rather than assumed. The share column is of the
state root as the consumer read it on 2026-10-29 `[R14, R16]`.

| | Population | Share | Bounded by | Triggered by | Guarded by | What an operator sees |
|---|---|---|---|---|---|---|
| P1 | **Collected files** — every job's `files/`, whole and verified | **105.57 GiB, 99.68 %** `[R14, D]` | `--output-budget B` `[C, main.rs:146-167]` | A collection arriving, never a clock `[C, store.rs:530-549]` | `--output-floor`, then D152's four keys `[C, store.rs:686-732]` | One line per sweep, with both figures; a named line and an event when the floor is breached `[C, store.rs:797-841]` |
| P2 | **The record** — `job.json`, `events.jsonl`, `log`, `log.seq` | **≈ 346 MiB, 0.32 %** `[R16, D]` | **Nothing, today** | **Nothing, today.** Under this direction: free space on the filesystem, with the maximum age as a backstop | `--output-floor` as the record guard, plus §3.5's never-delete list | Nothing today; a tombstone's sentence once records are removable (§3.2) |
| P3 | **The log inside a record** | **173.8 MiB in all** `[R16]` | Only the runner's `--job-log-cap`, **per attempt**, default 64 MiB `[C, confine.rs:50]`; the hub appends what arrives with no cap of its own `[C, store.rs:2469-2509]` | — | — | Nothing. A hub-side cap does not exist |
| P4 | **Bytes in flight** — `files.part/` | not read | Outside `B` **by design**, because evicting a transfer in flight would destroy the thing the hub is being given `[C, main.rs:152-156]`; D152 | — | — | An orphan part file is unreachable and unbudgeted: ORCH-165 |

Two readings of that table, and only the first is comfortable.

- **Almost the whole space question is P1.** Records and logs together are
  **0.32 %** of the state root `[R14, R16, D]`. So deduplication and the
  free-space trigger are the two mechanisms that matter for space, and
  **record deletion is a correctness and a privacy instrument rather than
  a space instrument.** Nothing below should be read as saying records
  threaten this disk. They do not.
- **The mechanism gap is nonetheless real, and it is about authority
  rather than about magnitude.** `--output-budget` governs the collected
  files **only** and says so in its own argument documentation — *"The
  output files only, not the state root: a record is kilobytes and a log
  has a cap of its own"* `[C, herd-hub/src/main.rs:149-151]`. So it
  cannot see a record byte or a log byte **by definition**, however many
  there turn out to be, and only removing a record ever frees a log byte.
  The quantity that can see all four populations at once is **free space
  on the filesystem**. That is why the space trigger for records is the
  free-space guard and never the output budget, and it would be the right
  shape at a hundredth of these figures or a hundred times them.

### 0.3 The log: measured small, unbounded on the hub, and not urgent

The hub's log file is the one population nothing in Herd bounds. The only
cap is the **runner's** `--job-log-cap`, default 64 MiB
`[C, herd-runner/src/confine.rs:50]`, and it is a cap **per attempt**: the
hub appends a second attempt's output onto the first attempt's log
`[C, herd-hub/src/store.rs:2283-2300]` and `ATTEMPTS_ALLOWED` is 2
`[C, herd-hub/src/server.rs:194]`, so the hub-side ceiling for one job is
two caps rather than one. Nothing on the hub refuses a byte past that.

**And in practice it is 173.8 MiB over 3 334 jobs** `[R16]`. The
argument documentation's *"a record is kilobytes"* is vindicated by
measurement: ≈ 104 KiB a job, all four record files together `[R16]`.

> **A hub-side log cap is therefore a guard worth having and not a
> priority** `[P]`. What is true and worth one line: it is absent, and one
> chatty job can legitimately bring 64 MiB — twice that over two attempts.
> What is **not** true: that logs threaten this disk.

This paragraph replaces a derivation that was briefed as alarming and was
not a measurement: 3 334 records × the runner's 64 MiB cap ≈ **208 GiB of
possible log**. It is arithmetic on a cap, it was marked derived by the
planner who wrote it, and the measurement is about six hundred times
smaller. **It is not a risk figure and it is not repeated as one.**

### 0.4 What deduplication does and does not do, and one figure corrected

**The correction first, because it was said out loud and is wrong as
stated.** Deduplication was described on 2026-10-29 as freeing *"about
85 GB"* on the consumer's volume. **That is wrong.** The 5.04 ratio means
84.94 GiB of what their records name are second-and-later copies
`[R12, D]` — but deduplication by hard link applies to **new arrivals**, and
the bytes already on their disk are already written. **On day one it frees
zero.**

What it changes is the **rate**, and that is the figure to quote:

| Figure | Before dedup | After dedup | Voice |
|---|---|---|---|
| Named arrival, over their 14 days | **7.57 GiB/day** | — | `[D, from R12]` |
| Distinct arrival, same 14 days | — | **1.50 GiB/day** | `[D, from R12]` |
| Headroom to a 200 GB budget, at 105.57 GiB held | 80.69 GiB | 80.69 GiB | `[D]` |
| **Days of that headroom** | **10.7** | **53.8** | `[D]` |

So: **five times the headroom in days, and zero bytes recovered on the
first day.** The 84.94 GiB is a measurement of their past. It becomes real
on their current volume only through an optional, operator-run compaction
that re-reads and re-verifies what is already there — not in this
direction's scope, and named in §5 so nobody discovers it.

**Every figure in this section is the consumer's reading of 2026-10-29,
carried forward** — not a measurement of the future and not ours. The
ratio could be higher over a longer window or lower over today's mix: it
is read over 14 days that include the pre-streaming era, when a job
directory was kilobytes. A second reading is what settles it, and nobody
in this project runs either (invariant 9, D161).

### 0.5 Free space is a burst cache, not a margin to reach and then fill

**The state root is not on a dedicated disk.** It is a Docker volume
sharing one **916 GB filesystem** with the consumer's whole project, their
runner's own spool and other volumes: **64 % used, 319 GB free** on
2026-10-29 `[R11]`. The hub is one tenant among several.

**That is the same filesystem as R5 and very nearly the same reading, in
different units** `[D]`. R5 gave 983 GB capacity and 344 GB free on
2026-10-27; 983 GB is 915.5 GiB and 344 GB is 320.4 GiB, and `df`'s own
`Use%` is used over used-plus-available rather than over capacity, which
turns R5's 589-of-983 into 63 % and not 60 %. So the two readings agree,
**free space has not materially moved in those two days**, and the figures
below are in the GiB the 2026-10-29 reading is in. Two readings of one
quantity that differ only in their units are worth reconciling out loud in
a document that now carries both.

Two consequences, and the second is the one that reframes the policy.

1. **A threshold expressed as a proportion of a hub-private budget is the
   wrong shape.** The hub's budget can sit half empty while the filesystem
   fills because of something that is not Herd's. The trigger that can see
   that is **free space on the filesystem**, which is §2.6's
   `--volume-free-floor R` from `statvfs` — already recommended there, and
   now a prerequisite rather than a neighbour.
2. **Free space is what absorbs a spike of activity.** More free space is
   more room for a burst, so holding bytes that serve nothing costs
   **throughput**, not merely room. **The 15 %-free figure is a floor, not
   a target**: the policy's ideal is *keep only what still serves*, not
   *climb to the floor and sit on it*.

**The figure is the operator's and it already exists.** Alexa's standing
rule for that disk is **alert above 85 % used**, so the floor on their
filesystem is **15 % free ≈ 137 GiB** `[D, from R18]`. That is not a new
decision and it is not ours to invent. It also says §2.6's proposed
default is wrong for this disk: `max(20 GB, 5 % of the volume)` would be
≈ 46 GB there, **about a third of the operator's own figure** `[D]`.

What that leaves on their disk today, with both ceilings named `[D]`:

| Ceiling | Headroom now | At 7.57 GiB/day | At 1.50 GiB/day |
|---|---|---|---|
| The output budget, `B` = 200 GB | **80.69 GiB** | 10.7 days | **53.8 days** |
| Free space, floor 15 % of 916 GB | **181.6 GiB** | 24.0 days | **121 days** |

**So the budget binds first on their disk today, and the free-space guard
is what survives a neighbour growing** `[D]`. Both rows assume Herd is the
only thing moving on that filesystem, which this section's first sentence
says it is not: the free-space row is an upper bound on time, not a
forecast.

### 0.6 Keep only what still serves: nothing is kept without a bound

**The principle, in the form that survives.** We do not destroy a
requester's evidence immediately and we **never** destroy it silently.
That is not the same as keeping it: **keeping for the sake of keeping is
refused** (Alexa, 2026-10-29, relayed in French and rendered here in the
English this document is written in). So:

> **What can no longer serve is removed after a stated delay, saying so,
> or moved to a configurable archive path on another disk if the operator
> gives one.** Set aside under a name that says so is a **transitional
> state** until that delay expires, never an end state. `[P]`

**Two settings, named, with no default invented here** `[P]`:

| Setting | What it bounds | Whose figure |
|---|---|---|
| The **delay** before something that can no longer serve is removed | How long a transitional state may last — the window in which an operator can still change their mind | **Alexa's.** Not invented in this document |
| The **archive path** | Where bytes go instead of being removed, on a filesystem that is not this one | **Alexa's.** Unset means removal after the delay |

Both are deliberately unfilled. A shipped binary stating an unrecorded
policy is a number nobody can later change, which is D194's rule and
`herd-hub report`'s own; the consumer's orchestrator declined to invent
these two and so does this document.

**The worked example, because the distinction is only concrete in one.**
The consumer is keeping one damaged job and its interrupted spool right
now, at this project's request `[R19]`: they are the evidence of a defect
still being fixed. **They still serve.** The same bytes a week after the fix
ships serve nothing at all. **Same bytes, opposite answer** — and the rule
that tells the two apart is *does this still serve*, not *how old is it*.
That is the whole of "space before age" in one case, and it is also why
the age spans survive as a backstop: nothing in the hub can read "still
serves", so the age is what bounds the answer when nobody says.

### 0.7 What an operator must be able to see, and why it is a requirement

On 2026-10-27 the consumer found **65 GB of accumulated runner spools that
no tool reported** `[R20]`. That is the precedent, and it makes three
properties requirements rather than preferences `[P]`:

1. **Whatever is kept appears in a report with its size** — including
   anything in a transitional state and anything under the archive path.
2. **It counts toward the space calculation.** A byte that is held but not
   counted is the 65 GB again.
3. **An overflow is signalled.** Not merely recorded: said, once per
   episode, in the unit the operator thinks in (§2.7).

A hub that sets bytes aside and does not report them has reproduced the
defect this requirement was earned on.

---

## 1. The inputs

### 1.1 What the code does today

| | Fact | Where |
|---|---|---|
| C1 | `--output-budget` defaults to `200_000_000_000` bytes, decimal, a chosen figure | `herd-hub/src/job.rs:994`; argument at `herd-hub/src/main.rs:146-167` |
| C2 | `--output-floor` defaults to seven days | `herd-hub/src/job.rs:998`; argument at `herd-hub/src/main.rs:168-183` |
| C3 | The budget counts **whole, verified collections** in every job's `files/`. Part-written bytes are outside it | `herd-hub/src/job.rs:980-993`; D152 |
| C4 | The sweep is triggered by an arrival, never by a clock: `held_more` adds to a running total and sweeps when it crosses the budget | `herd-hub/src/store.rs:530-549` |
| C5 | A sweep walks every job directory; under budget it reads **no records at all** | `herd-hub/src/store.rs:642-684` |
| C6 | Over budget it reads a record per byte-holding job and sorts on four keys: outside the floor, then passed, then fetched, then oldest | `herd-hub/src/store.rs:686-732` |
| C7 | The floor is measured from `record.updated_at`, not from a file's mtime | `herd-hub/src/store.rs:712` |
| C8 | Eviction renames `files/` to `files.gone/`, writes the record and the event under the lock, unlinks with no lock held | `herd-hub/src/store.rs:562-598` |
| C9 | One line per sweep; the **inside-the-floor** warning fires on every sweep that took something in-floor | `herd-hub/src/store.rs:797-818` |
| C10 | The **over-budget-and-nothing-to-free** line is said once per episode and cleared when a sweep comes back inside | `herd-hub/src/store.rs:820-841` |
| C11 | Opening the state root **sweeps**, so a budget change takes effect at the next hub start, before a job runs | `herd-hub/src/main.rs:215-224` |
| C12 | The startup line names the budget and the floor, and promises *"A record keeps every path, size and digest for ever: only the bytes are ever removed"* | `herd-hub/src/main.rs:428` |
| C13 | **Nothing in the tree removes a job directory, ever.** Every walk is therefore O(all jobs ever submitted) | `docs/remediation/03c-contradictory-review.md:384-386` |
| C14 | `next_sequence` is re-derived at every open from the **highest surviving job directory name**, and nothing persists it | `herd-hub/src/store.rs:879`; `IndexFile` at `:229-233` |
| C15 | `usable_index` trusts the index only while `index.jobs_accounted` equals the number of job directories; otherwise it rebuilds by reading every record | `herd-hub/src/store.rs:1003-1008`, `:884-893` |
| C16 | A record that will not read is counted in `could_not_free` and never evicted: *"not a licence to delete the files it describes"* | `herd-hub/src/store.rs:692-699` |
| C17 | `Priority` has four levels — merger, step, nightly, producer — with no `Default`, and the record carries it | `herd-proto/src/lib.rs:704-713`, `:737`, `:1624` |
| C18 | Nothing anywhere reads a retention class, a retention span, or expires a record | grep for `retention` in `herd-hub/src`: six hits, all prose or the output floor |

### 1.2 What the first consumer reports

Unverified by this project. Figures as relayed.

| | Reported | Source |
|---|---|---|
| R1 | Hub volume `herd_herd-state`: **107 GB**, almost entirely `jobs/` | Alexa's breakdown |
| R2 | **3334** job directories | Alexa's breakdown |
| R3 | **~540 MB** each for the recent ones | Alexa's breakdown; the consumer **could not confirm** this (see R8) |
| R4 | **362** directories older than seven days, **12 GB** in all | Alexa's breakdown |
| R5 | `/mnt/ssd_980` is **one shared ext4 filesystem on NVMe**: 983 GB size, 589 GB used, **344 GB free**, holding both the hub volume (`/mnt/ssd_980/docker/volumes/herd_herd-state`) and the runner's data root (`/mnt/ssd_980/herd-runner`) | the consumer, 2026-10-27 |
| R6 | Job ids `j0000000001` to about `j0000003334` between 2026-10-13 and 2026-10-24 — about **280 ids a day**, and that is a **ceiling, not a rate**: it includes requirement readings, which leave no output, and the two lost or cancelled passes of 10-24 | the consumer, 2026-10-27 |
| R7 | The real shape: *"a few dozen step and merger jobs a day, plus one nightly full pass of ~214 groups, which carries most of the output volume"*. The nightly is **off right now** and chatOnS is paused, so today's rate is near zero | the consumer, 2026-10-27 |
| R8 | The per-job sizes and the runner's spool listing **cannot be read**: the runner's data root is `drwx------ herd:herd`, the read needs `sudo`, their operator permissions do not cover it, and they declined to work around it | the consumer, 2026-10-27 |
| R9 | A runner's `outputs/`: 65 GB in 125 spools, all under seven days | Alexa's breakdown; **ORCH-092's**, not this document's |
| R10 | **The reading of 2026-10-29, and it is the one §0 is built on.** Taken on their real root with `herd-hub report` and a read-only `du -sk`, by the operator, on their own machine. R10 to R19 are all of it | the consumer, 2026-10-29 |
| R11 | The state root is a Docker volume on **one 916 GB filesystem**, shared with the consumer's whole project, their runner's own spool and other volumes: **64 % used, 319 GB free**. It is **not** a dedicated disk. Same filesystem as R5, in GiB (§0.5) | the consumer, 2026-10-29 |
| R12 | Bytes the records name over files carrying a digest: **105.96 GiB**. Over distinct `sha256`: **21.02 GiB**. Ratio `r_file` = **5.04** | `herd-hub report`, 2026-10-29 |
| R13 | **110 653** files carry a digest, over **9 272** distinct digests | `herd-hub report`, 2026-10-29 |
| R14 | Every job's `files/` together: **105.57 GiB**, over **3 334** records — of which **1 377**, 41.3 %, name no file at all | `herd-hub report`, 2026-10-29 |
| R15 | Per-record output size: decile 9 is **39.82 MiB** and the largest **333** records carry **98.12 %** of the bytes | `herd-hub report`, 2026-10-29 |
| R16 | Everything under `jobs/<id>/` except `files/` and `files.part/`, by `du -sk` — disk blocks, not exact lengths: `log` **173.8 MiB**, `events.jsonl` **119.2 MiB**, `job.json` **40.1 MiB**, `log.seq` **13.0 MiB**. **≈ 346 MiB in all, ≈ 104 KiB a job** | the consumer, read-only on their real root, 2026-10-29 ~16:30Z |
| R17 | Outside `jobs/`: **176 KiB** of readings, **8 KiB** of index, 4 KiB each for the rest | the consumer, 2026-10-29 |
| R18 | Alexa's standing rule for that filesystem: **alert above 85 % used**. It is an existing rule, not a new decision, and it is the free-space floor of §0.5 | Alexa, 2026-10-29 |
| R19 | One damaged job and its interrupted spool are being kept **right now**, at this project's request, as the evidence of a defect still being fixed. §0.6's worked example | the consumer, 2026-10-29 |
| R20 | The 65 GB of R9 **were reported by no tool**: they were found by looking. That is the precedent behind §0.7's three visibility requirements | the consumer, 2026-10-27 |

One figure in this table is not theirs and is ours: the orchestrator ran
`df` on this host minutes after R5 and got `/dev/nvme0n1p1`, ext4,
983 GB / 592 used / 341 free — the same filesystem, the same numbers.
That is a measurement on **this workstation**, which is where Herd is
developed; it establishes that the hub's state root is this machine's
`/mnt/ssd_980` by measurement rather than by anyone's word, and it
answers ORCH-076 yes. It is not a reading of the consumer's job
directories and nothing below treats it as one.

### 1.3 What could not be read on 2026-10-27, and what was read on 2026-10-29

**Most of this is now answered, by the operator, read-only, on their own
machine.** What §2 and §3 turned on was one quantity — **how many bytes
one nightly pass leaves on the hub** — and the shape of the distribution
behind it. The reading of 2026-10-29 `[R10 to R20]` gives the
root-wide totals, the dedup ratio, the per-record deciles and the record
side to the kilobyte. It did **not** need root and it did not need a
`du` of every job directory: `herd-hub report` reads the records the hub
already owns, and the `du -sk` that completed it covers only the four
record files.

What is still unread, and it is now a smaller list:

* **A per-directory listing of `jobs/` with dates**, which would say
  whether the 105.57 GiB is one nightly pass or several (H1, H3). The
  deciles of R15 bear on it and do not settle it.
* **The runner's spool listing**, still `drwx------ herd:herd` and still
  needing `sudo` `[R8]`. Unchanged, and still ORCH-092's rather than
  this document's.
* **A second reading of `herd-hub report`, days apart.** Every rate in
  this document is a stock wearing a rate's clothes: one reading
  measures their history, two measure the process. This is the single
  most valuable figure still missing and it costs one command on their
  side.

**Nobody in this project runs any of them**, including from this shell
and including on the paths named in R5: Herd is shipped, not operated,
and a fact about that machine reaches us as a report (invariant 9, D161).
**ORCH-103 — the `du` of every job directory — should close**: it needed
root, they declined and were right to, and the reading of 2026-10-29
supersedes most of what it wanted. What replaces it is the second
`report` reading, which is a different and cheaper ask.

---

## 2. Half one — re-deriving the output budget

### 2.1 The rough reading was 13.5 GB a day. It is the wrong statistic.

The arithmetic behind it is sound as arithmetic. 107 GB held minus 12 GB
older than seven days is 95 GB that landed inside the window `[D, from R1
and R4]`, and 95 ÷ 7 is **13.57 GB a day** `[D]`. From today's 107 GB a
200 GB budget is then (200 − 107) ÷ 13.57 ≈ **6.9 days away** `[D]`,
which is where "about a week" came from.

Three things that reading assumes, and the inputs do not support the
first two:

1. **That the seven-day window is a steady state.** It is not: it
   contains the incident of 10-24 and two lost or cancelled passes
   `[R6]`.
2. **That a daily mean describes the arrival process.** It does not. The
   process is one burst a night carrying most of the volume, plus a few
   dozen small jobs `[R7]`. A mean is the right statistic for a mechanism
   driven by the average and the wrong one for a mechanism driven by the
   peak — and this mechanism is driven by the peak: the sweeper fires on
   a *crossing*, and a crossing is made by a burst.
3. **That the arrival continues.** It does not, today: the nightly is off
   and chatOnS is paused `[R7]`, so the volume is currently static and
   nothing is approaching anything.

So the daily rate is withdrawn. **The unit that matters is bytes per
nightly pass.** Call it `N`.

### 2.2 Bounding `N`, and the circularity to avoid claiming

Two routes, and they are *not* independent confirmations:

* **Route A.** If the volume holds about one pass, then `N ≈ 107 GB`
  `[D, from R1 + R7]`; dividing by 214 groups gives ~500 MB a group.
* **Route B.** If a recent job directory really is ~540 MB, then
  214 × 540 MB = **115.6 GB** a pass `[D, from R3 + R7]`.

They agree to within 8 %, and that is worth something, but route A
*assumes* the volume holds one pass in order to produce the per-group
size, while route B *assumes* the per-group size in order to produce the
pass. They share the 214 and they both rest on Alexa's breakdown, which
the consumer could not confirm `[R8]`. The honest statement is a range,
not a point:

> **`N` is between about 95 and 116 GB, on two readings of the same
> unverified breakdown.** `[D]`

One weak independent check does support it: 95 GB ÷ 540 MB = **176**
output-leaving jobs inside the seven-day window `[D, from R1, R3, R4]`,
which is *less than one full pass of 214* — consistent with "the volume
holds roughly one pass" and inconsistent with "the volume holds four or
five small passes". And 12 GB over 362 old directories is 33 MB each
`[D, from R4]`, far below 540 MB, which is what you would expect of
directories written before the streaming carriage shipped on 2026-10-16
(D150, D152): kilobyte records, not collections.

### 2.3 The identity that replaces the debt D148 left

D148 said the arithmetic that would derive the budget is *"a few hundred
megabytes times the jobs a day that really leave that much"*. Written as
a rule rather than a number, with `F` the floor in days and `A` the bytes
arriving per day:

> **The floor is a promise the hub can keep if and only if
> `budget ≥ F × A`.** `[P]`

That is not a tuning heuristic; it falls straight out of the sweeper's
first key. Nothing inside the floor is taken while anything outside it
remains `[C6]`, so the material the sweeper is *allowed* to take is
exactly what has aged past `F`. At steady state the hub holds `F × A`
bytes inside the floor; if the budget is smaller than that, there is
nothing outside the floor to take and every eviction is a broken promise.
Above it, the hub never touches the floor at all.

Three numbers fall out of it for the first consumer, each marked with
what it depends on:

* **The budget a seven-day floor needs here: `7 × N` = 665 to 809 GB,
  about 750 GB** `[D, from §2.2]`.
* **The largest budget this filesystem could carry:** the hub's current
  107 GB plus the 341–344 GB free is **448–451 GB** `[D, from R1, R5]` —
  and only by taking every free byte, which R5 forbids: that free space
  is shared with chatOnS's caches, workspaces and images, with the
  runner's 65 GB of spools `[R9]`, and with this project's own agent
  build trees. Leave the neighbours a realistic share and the honest
  ceiling is **nearer 250 GB** `[P]`.
* **The floor 200 GB can actually keep here:** 200 ÷ N ≈ **1.9 days**
  `[D]`. About two nightly passes.

### 2.4 The finding of 2026-10-27, and how deduplication retires it

**This was the heart of this document and it no longer holds.** It is kept
here in full, with the retirement after it, because the identity it rests
on is sound and is what kills it.

**As written on 2026-10-27.** 750 GB is needed; 451 GB is the theoretical
maximum and ~250 GB the practical one. *At the first consumer's workload,
on the disk they have, the seven-day floor cannot be kept at any
budget — not by raising the budget, not by buying back space from the
neighbours* `[D]`. What would refute it: the floor becomes satisfiable if
`N ≤ 451 / 7 ≈ 64 GB` in theory, or `N ≤ 250 / 7 ≈ 36 GB` in practice,
and the evidence put `N` at 95–116 GB — 1.5× over the theoretical bar and
3× over the practical one.

> **Retired on 2026-10-29, by deduplication rather than by a
> measurement.** `[D]` §2.3's identity `budget >= F x A` survives
> untouched and is exactly what retires the finding: **deduplication
> divides `A`**, the bytes arriving per day, by the ratio read on their
> own volume.

Two readings of it, both from R12 and both arithmetic on their figures:

* **On the rate.** Their distinct arrival is 1.50 GiB/day `[D, R12]`, so a
  seven-day floor needs `7 x 1.50 = 10.5 GiB` against a 200 GB
  (186.26 GiB) budget `[D]` — **about eighteen times the margin needed**.
* **On this section's own per-pass reading.** With `N` at 95–116 GB named,
  the distinct bytes of one pass are `N / 5.04` = **18.9 to 23.0 GB**
  `[D]`, so seven passes is **132 to 161 GB** against 200 GB: satisfiable
  with **19 % to 34 % of the budget still free** `[D]`. Thin at the upper
  end, and no longer impossible.

> **So the seven-day floor becomes keepable at the first consumer's
> workload, by deduplication rather than by a measurement or a bigger
> disk** `[D]`. That is the strongest single argument for doing
> deduplication first, and it vindicates **D202** three days after it was
> taken: the floor Alexa kept becomes a promise the hub can make.

**What would bring the finding back.** A prospective ratio materially
below 5.04 — the figure is read over 14 days that include the
pre-streaming era, when a job directory was kilobytes, so it could move
either way. That is H10, and it is settled by a **second** reading rather
than a better one (§1.3).

**One thing the reading of 2026-10-29 says about the old inputs, and it
narrows rather than settles.** R3's *"~540 MB each for the recent ones"*
is the premise of Route B, and R15 says per-record output is deciles 1 to
8 at nothing, decile 9 at 39.82 MiB, and the largest **333** records
carrying **98.12 %** of the bytes — a top-decile mean of about **311 MiB**
`[D, from R14 and R15]`. So ~540 MB describes the top decile at most and
not a typical directory, and 214 groups at that top-decile mean would be
**≈ 67 GiB a pass** `[D]` rather than 95 to 116 GB. That assumes a nightly
pass *is* the top 214 records, which nothing shows; it does **not** settle
H1 or H3, and the per-directory listing with dates is still what would.

The other way to satisfy the floor is on the consumer's side and not ours,
and it is unchanged: a seven-day floor fits a 200 GB budget if a pass
leaves 200 / 7 = 28.6 GB of **distinct** bytes `[D]`. On R12's ratio their
current pass already does. That is their decision about their test suite,
relayed as a figure, never as an instruction (D161).

### 2.5 What happens at the crossing, concretely

Taking `N ≈ 107 GB`, a 200 GB budget and the volume as it stands `[R1,
R4]`:

* **The second pass.** Held goes from ~107 GB to ~214 GB, over budget by
  14 GB. Outside the floor there is the 12 GB of pre-window material
  `[R4]`. The sweeper takes all of it and then **~2 GB from inside the
  floor** `[D]`.
* **The third pass and every one after it.** Everything held is from the
  last two nights, so the out-of-floor pool is empty and **every byte
  freed is inside the floor**, for as long as the nightly runs `[D]`.
  This is the brief's intuition, reached by a different mechanism than
  the one it named: not because today's ratio is 12-against-95, but
  because a burst delivers a seventh of the floor's worth of material in
  one night.
* **It happens ~100 times a night, not once.** The sweep is triggered by
  each completing collection `[C4]`, so once a pass is past the budget
  every remaining group of that pass triggers a **full walk of every job
  directory ever created** `[C5, C13]`. The hub spends the night deleting
  last night's evidence to make room for tonight's, one collection at a
  time, with a walk between each.
* **What that walk costs — measured, 2026-10-28 (ORCH-104).** The sweep's
  own walk is the cheap one, because under budget it reads no record at
  all `[C5]`: **13.8 ms warm and 19.8 ms cold at 3334 directories**, and
  **137 ms warm and 205 ms cold at 33 340** `[M]`. A hundred of them a
  night is **1.4 s today and 14 s at ten times**, in bursts, against a
  hub whose host already has IO as its sore point (D136). That is
  *cheaper* than the 4–6.5 s this bullet derived, and the reason is that
  the derivation scaled the wrong one of D152's two figures: the sweep
  under budget does one `stat` and one `opendir` per directory, not a
  record read. The expensive walk is the record-reading one, and §3.4
  carries it.
* **And it produces ~100 identical alarms a night.** The inside-the-floor
  warning fires on every sweep that took something in-floor `[C9]`; only
  the over-budget-nothing-to-free line has the once-per-episode
  discipline `[C10]`. An alarm that fires a hundred times a night, every
  night, is not an alarm.

**Is this the behaviour we want?** The eviction *order* is right and
nothing here argues against it: D152's four keys are the correct answer
to "which bytes go", and the budget beating the floor is the only honest
answer to "a promise about time against a fact about a disk". What is
wrong is three things around it: a default that cannot see the disk
(§2.6), a promise the hub keeps making after it has been shown to be
unkeepable (§2.4), and a warning that arrives only after evidence is
already gone (§2.7).

### 2.6 The shape of the budget: not a constant, not a fraction

**Is a default in gigabytes even the right shape?** No. Four shapes,
and why three of them lose `[P]`:

1. **An absolute constant (today).** It is blind to the disk. On a
   100 GB volume a 200 GB budget is a *disabled* sweeper: the running
   total never crosses it, no sweep ever runs, and the first symptom is a
   full filesystem. The failure is silent and total, which is the worst
   property a safety mechanism can have.
2. **A fraction of the volume's capacity, as an entitlement.** Wrong
   here for the reason R5 gives: the volume is shared. 60 % of 983 GB is
   590 GB that does not exist for Herd. A fraction of capacity is a claim
   on space nobody granted.
3. **A fraction of free space at startup.** Non-deterministic in the
   start time. A hub restarted while the disk is quiet claims space it
   will later be denied; one restarted after a neighbour grows silently
   evicts. Two hubs on the same disk with the same configuration get
   different budgets.
4. **A ceiling and a reserve, with the effective limit a function of the
   disk.** Recommended.

The recommendation, in one formula:

> **`effective_ceiling = min(B, held + max(0, free − R))`** `[P]`
>
> `B` = `--output-budget`, **the most Herd will ever hold**: an
> allocation decision an operator makes. `R` = `--volume-free-floor`
> (new), **the least Herd leaves free on the volume**, whoever consumed
> it. `free` comes from `statvfs` on the state root, re-read at each
> sweep trigger. The sweeper fires when `held > effective_ceiling`.

Why this survives a neighbour growing, which is the property R5 demands:
if chatOnS fills the disk, Herd's ceiling shrinks and Herd gives its own
bytes back rather than joining a disk-full outage; when the neighbour
shrinks, the ceiling comes back. Herd never claims a share nobody gave
it, and it never holds bytes while the filesystem dies around it. The
hub can only free what it holds, so when `free < R` and there is nothing
of Herd's left to take, it says so once per episode — the discipline
already at `herd-hub/src/store.rs:820-841`.

**The defaults** `[P]`:

* `B` defaults to `min(200 GB, 25 % of the measured volume)`. On this
  disk that is `min(200, 246) = 200 GB`: **Alexa's figure survives
  unchanged where it is defensible**, and this is a safety net rather
  than a policy reversal. On a 100 GB VPS it becomes 25 GB instead of a
  number larger than the disk, which is the pure-failure mode of shape 1.
* `R` defaults to `max(20 GB, 5 % of the volume)` — 49 GB here.
* An explicit `--output-budget` always wins over the derived default, and
  the argument should accept `auto`, `60%` and a size. That parsing is
  hub-local and must **not** go into `herd_proto::parse_bytes`: D152 put
  the unit table in `herd-proto` precisely because an operator types a
  size on one side of the link and reads it back from the other, and a
  percentage of the hub's own volume is not a thing the runner can ever
  mean.

**What a hub does when it cannot measure its volume** `[P]`: `statvfs`
fails, so the free-space guard is **off** and `B` alone applies. It does
not guess a capacity, and it does not refuse to start — one guard off is
better than no hub. It says so, by name, in the startup line that already
names the budget `[C12]`, and that line must distinguish *"138 GB, a
quarter of the 553 GB volume"* from *"200 GB, the fallback, because this
hub could not measure its volume: <errno>"*. That distinction is D148's
own rule carried forward: a chosen number must not wear a derived
number's clothes, because whoever reasons from it next reasons from the
clothes.

**Amended 2026-10-29 (§0.5): the recommendation stands, two of its figures
do not, and `R` is promoted.**

* **`R` is no longer a neighbour of record removal; it is its
  prerequisite.** The space trigger for records is `R` and never `B`,
  because `B` governs the collected files only by its own documentation
  `[C, main.rs:149-151]` and cannot see a record or a log byte (§0.2).
  One operator-set number answering for three populations is the thing
  that argument refuses in writing.
* **`R`'s default is wrong for the first consumer's disk, and the figure
  is not ours.** `max(20 GB, 5 % of the volume)` is ≈ 46 GB on their
  916 GB filesystem `[D, R11]`, while Alexa's standing rule for that
  filesystem is **alert above 85 % used**, which is **15 % free ≈ 137 GiB**
  `[D, R18]` — about three times the proposed default. The rule already
  exists, so the figure is the operator's and this document does not
  invent it. What the default is *for* is a disk nobody has told Herd
  about; it must not quietly contradict an operator who has.
* **The floor is a floor and not a target.** Free space is what absorbs a
  burst, so a hub that climbs to `R` and sits there has spent the
  consumer's throughput, not just their room. Nothing in the mechanism
  makes the hub climb — the sweeper only ever removes — but the *policy*
  must say which of the two it is aiming at, and it aims at **keep only
  what still serves** (§0.6).
* **`B`'s own default is unchanged and still defensible.**
  `min(200 GB, 25 % of the measured volume)` is `min(200, 229) = 200 GB`
  on 916 GB `[D, R11]`, so Alexa's figure survives where it is
  defensible, exactly as this section said on a slightly different
  capacity.

### 2.7 Warn on the approach, not at the breach

Today there is **no signal at all while the volume climbs**. The per-sweep
line only speaks when something was removed `[C9]`; the first line an
operator ever reads arrives after evidence is already deleted. One loud
line at the breach is not enough, and the reason is specific to this
workload rather than general: here the volume goes from 107 GB to 214 GB
**in one night** `[D, §2.5]`, so a percentage watermark at 80 % of the
budget would be crossed and breached inside the same night, probably
inside the same hour. A percentage is the wrong unit.

The right alarm is in the unit the operator thinks in `[P]`:

> **Warn when `B − held` is less than what arrived in the last 24 hours**
> — "this hub cannot hold another day of what you are sending it."

It needs one rolling counter on the store's `Live` state, no IO and no
walk, beside the running total that already lives there `[C4]`. On these
figures it would have fired on the **first** night, with a week of
warning. And the inside-the-floor line should come under the same
once-per-episode flag as the over-budget line `[C9, C10]`, so a night of
evictions is one piece of news rather than a hundred.

**Amended 2026-10-29 (§0.7): this section's alarm is now a requirement
rather than a recommendation, and it gains two more.** The precedent is
R20 — 65 GB of accumulated runner spools that no tool reported, found by
looking. So, for whatever the hub holds, including anything in a
transitional state and anything under an archive path (§3.3.2):

1. **It appears in a report with its size.** `herd-hub report` is where
   that lands on the record side; the sweep line is where it lands per
   episode.
2. **It counts toward the space calculation.** A byte that is held but
   not counted is R20 again, with Herd's name on it.
3. **An overflow is signalled**, once per episode, in the unit the
   operator thinks in — which is this section's own alarm, and the reason
   it is not optional.

And the warning must now watch **two** quantities rather than one: `B`
against what arrived, as above, and **free space against `R`**, because a
neighbour filling the filesystem is invisible to the first (§0.5).

### 2.8 What an operator can do today, with no release, and the trap

Two knobs exist and they are **not symmetric**:

* **Lowering `--output-floor` is free and safe.** Nothing is ever removed
  for merely being old — that is D152's rule and its test,
  `a_hub_inside_its_budget_removes_nothing_however_old_the_bytes_are`.
  The floor only *orders* candidates `[C6]`, so lowering it deletes
  nothing by itself; it enlarges the pool the sweeper may take from
  before it has to break a promise.
* **Lowering `--output-budget` below what is held is destructive and
  immediate.** Opening the state root sweeps `[C11]`, so the eviction
  happens at the next hub start, before a single job runs. Concretely:
  at 107 GB held with 12 GB outside the floor `[R1, R4]`, an operator
  who types `--output-budget 50GB` to be safe destroys 57 GB **inside**
  the floor on that first sweep `[D]`. The sentence on each record will
  say so, which is the design working, and the bytes are still gone.

**The safe order, today** `[P]`: lower the floor first, then lower the
budget in steps, reading the per-sweep line between steps. And raise the
budget rather than lower it if the disk can carry it — on these figures
`B` has room up to the §2.3 ceiling, though not nearly enough room to
rescue the floor.

**The gap this exposes**: there is no dry run. An operator cannot ask
*what would a budget of X take from me?* It is cheap to add — the walk,
the record reads and the four-key sort all exist `[C5, C6]`; the dry run
is the same code stopping before `evict`. ORCH-097.

### 2.9 Deduplication: the denominator, and what it does not touch

§0.4 gives the honest figures. This section gives the shape, because the
figures are only defensible if the mechanism is the small one.

> **`objects/<sha256>` beside `jobs/` in the state root; every file in a
> job's `files/` is a hard link to it.** No format, no compressor, no
> chunker, no dependency, no secret, **no new record field and no new
> state of a record** — the file *is* present, merely shared. `[P]`

Four properties of the existing code are what make it safe, and all four
are read from the tree rather than assumed:

* **Herd never writes through a name in `files/`.**
  `durable::write_file_with` writes a temporary, `fsync`s it and renames
  `[C, durable.rs:247-274]`; the streamed path appends into `files.part/`
  and renames `[C, store.rs:2136-2143]`. **So a shared inode can never be
  modified under its other claimants.** The whole design rests on this
  and it already holds.
* **The digest is verified before the file becomes visible**
  `[C, store.rs:2114-2140]`, and the inline path refuses bytes that
  disagree with the manifest `[C, store.rs:1968-1976]`. One exception,
  and it is a rule rather than a caveat: the comparison is guarded by a
  non-empty digest, so **a blob with no verified digest is never shared**.
* **The reference count is the link count, not a ledger.** A blob whose
  `st_nlink` is back to 1 is garbage. Nothing is counted; it is measured —
  the same argument `collection_is_whole` already makes for itself,
  that a tally kept beside the files could disagree with them
  `[C, store.rs:554]`.
* **Hard links are transparent to every reader, including an older
  binary**, and `recover` enumerates `jobs/` and nothing else. So
  `objects/` **must not move `LAYOUT_VERSION`**, which is 2
  `[C, store.rs:99]` since M11's pack half moved it there (D199): a
  version move has no migration path and refuses to open an operator's
  existing root (ORCH-156), while a downgrade onto a shared root costs a
  **leak** of orphan blobs that the next upgraded start collects. A leak
  that self-heals beats a refusal that does not. **The figure is the
  layout this work starts from, not a constant of the design** — what
  the argument needs is that `objects/` adds no move of its own, at
  whatever number the layout then stands. That the number has just moved
  once, for the first time ever, does not buy a second move cheaply: it
  spends the one reset the consumer's volume was ever going to get
  (D199), so the next move has no budget behind it at all.

**What it does not touch**, each named so it is not discovered later:

| | Not touched | Why |
|---|---|---|
| D1 | The 105.57 GiB already on their volume | **New arrivals only.** The existing bytes are never rewritten, and §0.4 says so in the figure. Recovering them is an optional, operator-run compaction: §5 |
| D2 | What a record says | No new field, no new state, `RECORD_VERSION` unchanged. A pre-deduplication record reads identically |
| D3 | `files.part/` | Part files are neither whole nor verified, so they are never linked. ORCH-165 stays exactly the size it is |
| D4 | A filesystem without hard links | One `link()` probe at startup, a degrade to a plain write, **said once**, and never a failed transfer |
| D5 | Files carrying no digest | They can never be shared: a byte with no digest can neither be matched against another nor be called distinct from it. `herd-hub report` leaves them out of both sides of the ratio and **counts them separately** as `undigested_files` and `undigested_bytes` `[C, report.rs:249-253]`; the reading of 2026-10-29 did not relay those two figures, so their size here is unknown |

**Two consequences to state rather than let someone discover** `[P]`:

* **Two records naming identical content can be in different output
  states.** A record marked `outputs.removed` may be looking at content
  still on the disk under another job's `files/`. The hub does not serve
  it: removed means removed for that record. No fourth state is created.
* **Eviction becomes two figures where it was one** — *"unlinked 338 MiB,
  freed 12 MiB"* — and a sweeper that evicts a record at a time can walk
  its whole candidate list, destroy evidence and free nothing. **So it
  evicts by blob**, unlinking a blob from every record that claims it, and
  a shared blob is as protected as its **most protected** claimant under
  D152's four keys. The visible corollary: a green job's bytes can go with
  a red one's, and both sentences say so.

---

---

## 3. Half two — record retention, demoted to a guard

Nothing in the tree deletes a record today, ever `[C13, C18]`. 3334
records is small; unbounded is unbounded, and §2 showed that every walk
in the hub is O(all jobs ever submitted) because of it. Retention is not
only about disk: it is the only thing that can ever make those walks
shrink.

**And after the reading of 2026-10-29 it is barely about disk at all.**
The whole record side of their root is **≈ 346 MiB**, 0.32 % of the state
root `[R16, D]`. So:

> **Record removal is a correctness and a privacy instrument, and a bound
> on the walks. It is not a space instrument at these figures.** `[D]`

That is the sentence to carry out of this half. It does not make retention
less necessary — unbounded is still unbounded, 1 377 records that name no
file at all `[R14]` are never removed by any space rule because removing
them frees nothing measurable, and space pressure cannot bound a count —
but it does mean **nothing in §3 should be read as protecting this disk**.
§0.2's two populations have two owners for a reason of authority, not of
magnitude.

What this half still owes, and all three survive the re-cut: the class
map's two numbers as a **maximum age** (§3.1), the answer a requester gets
for a record that is gone (§3.2), and the never-delete list (§3.5). What
it loses is the **timer's primacy** (§3.3).

### 3.1 What a priority class is, and where 14/90 is read from

A priority class is `herd_proto::Priority` `[C17]`: four levels —
**merger**, **step**, **nightly**, **producer** — deliberately with no
`Default`, carried on the record. Four levels must map onto the two
numbers E-15 and `docs/plan.md` M12 promise, and the only place the split
is spelled is M12's *done when*: *"a step result past retention is gone
while a 60-day merger result reads fine"*.

**Recommendation** `[P]`: **merger → 90 days, every other level →
14 days**, read from two flags, `--record-retention` and
`--record-retention-merger`, mirroring the two-number shape
`--output-budget` / `--output-floor` already has. **Two numbers and not
four**: D152 refused a per-job retention class as *"the first line of a
policy language"* under invariant 1, and four knobs is that policy
language with the levels pre-filled. Two classes is what E-15 already
sanctions and no more.

One consequence to say out loud rather than let someone discover: on
these figures the class that carries nearly all the volume is
**nightly**, and it gets 14 days, while **merger** — which is small —
gets 90. That is the right way round by evidence value and the wrong way
round by bytes, and somebody will assume the big one is the protected
one.

**Amended 2026-10-29 (§0.1): the two numbers survive and their meaning
changes.** Alexa took this map as D201 — merger 90 days, every other class
14, active by default — and then moved its basis. What survives, said
precisely, because D201 is append-only and its numbers are still the only
ones we have:

* **The two numbers are unchanged.** 90 and 14. They stop being *when a
  record is deleted* and become **the oldest a record is ever allowed to
  get**: a maximum age, the backstop of §3.3's ladder.
* **"Ships active by default" survives**, and now means two things rather
  than one: the space ladder is on, and the backstop is on.
* **The three guards D201 named survive verbatim** — the preview that
  removes nothing, D197's two threshold-free refusals, and `0` meaning
  never.
* **What is demoted is the trigger.** A shipped Herd still removes an
  operator's records without being asked to, but **the timer is no longer
  the first thing that decides**.
* **D201's context figures are superseded**, not its choice. It cites
  *"~90 GB across 1 571 jobs"*; the reading of 2026-10-29 says 3 334
  records and 105.57 GiB of collected files `[R14]`. The new figures make
  a bound more necessary, not less, so nothing in the choice turns on it.
* **Four required spans with no default is still the right shape** — more
  so, because they are now a *guard*, and a guard a shipped binary
  invented would be worse than one an operator typed (D194).

The consequence this section already named survives the demotion and is
worth re-reading with it: the class carrying nearly all the volume is
**nightly** at 14 days, while **merger**, which is small, gets 90. Right
by evidence value, wrong by bytes, and somebody will assume the big one is
the protected one.

### 3.2 The manifest outlives the bytes — and now the record expires

Today the hub promises the opposite of M12 at startup, in these words
`[C12]`:

> *"A record keeps every path, size and digest for ever: only the bytes
> are ever removed."*

**M12 makes that sentence false, and changing it is part of M12 rather
than an afterthought.** So: what is the answer to a requester asking
about a job whose record is gone?

1. **"No such job."** Unacceptable. It is indistinguishable from a typo'd
   id and from a hub that lost data, which is exactly the trap D152
   names: *"the next thing after evidence disappears with no reason is
   somebody concluding Herd lost it."*
2. **Keep a trimmed `job.json` in the job directory.** The directory
   count never shrinks, so every O(T) walk keeps growing `[C13]` and the
   main cost retention exists to bound is not bounded.
3. **A tombstone outside `jobs/`.** Recommended. Append-only, sharded by
   month: id, priority, outcome, `ended_at`, `removed_at`, file count,
   total bytes. The directory goes, the walks shrink, the answer
   survives. At ~120 bytes a line, 3334 records is **0.4 MB** and
   110 000 jobs a year is **13 MB a year** `[D]` — small enough that
   tombstones need no retention rule of their own, and this document
   proposes none.

**Is that answer acceptable?** For the requester's question, yes; for the
diagnostic question, partly, and the line between them should be stated
rather than blurred. A tombstone can say:

> *job j0000000123, a nightly, failed, ended 2026-08-01; its record was
> removed 2026-08-15 under the 14-day retention for its priority. It had
> 12 files and 338 MiB.*

That is an answer, not a shrug, and it is the same shape as the sentence
an evicted collection already carries `[C8]`. What it cannot say is
*which* files — and that is not an oversight to fix, because the manifest
**is** the bulk of `job.json`: a retention that kept every path, length
and digest would be a retention that kept the record. **The per-file
detail is exactly what retention costs; the count, the size, the outcome
and the reason are what it must keep.** `[P]`

**Amended 2026-10-29: the manifest's promise is unchanged, and it acquires
a second job.** Nothing in the new direction touches what a record *says*.
*"A record keeps every path, size and digest for ever: only the bytes are
ever removed"* `[C, main.rs:428]` holds until a record is removed, which
is where that sentence has to change anyway (ORCH-101). What changes is
what a removal **frees**, not what a record promises, and D148's and
D152's rule — the manifest outlives the bytes — survives untouched.

The second job it acquires comes from deduplication (§2.9): because every
`KeptFile` carries its `sha256`, **the manifest is the reference list that
makes a shared blob collectable**. That is why sharing inodes does not
weaken the promise: it is the promise that makes sharing safe to undo.

**One line number to carry forward.** The startup sentence is at
`herd-hub/src/main.rs:428`. `[C12]`, this section and ORCH-101's row all
cited `:283-297` or `:288-297` until 2026-10-29; that citation was stale
by about 130 lines and is corrected in all three places.

### 3.3 Where the two mechanisms meet, and which wins

**This section stated the reverse of the rule and is inverted here.** As
written on 2026-10-27 it said: *"Retention is about records and
**outranks** the budget, which is about bytes. The budget may never
remove a record; retention always removes the bytes in the same act."*
The first sentence is wrong under the direction of 2026-10-29. The
second half of the second sentence is right and survives for a reason
that has nothing to do with precedence (the hazard below).

> **The budget and the free-space guard outrank. Space decides, and age
> is the guard that bounds the answer when nothing is under pressure.**
> `[P]`
>
> **Why**, in one line each: the budget is a fact about a disk and the
> span is a promise about time, and 2026-10-29 put the fact first; the
> span cannot see a byte, so it can never be the thing that answers
> *"this volume is filling"*; and `--output-budget` governs the collected
> files only, by its own documentation, so **it is not even eligible** to
> be the mechanism that removes a record `[C, main.rs:149-151]`.

### 3.3.1 The ladder

One trigger per rung. Each rung is either already shipped, already
approved by Alexa, or named as new. **No rung below is a new knob.**

| | Rung | Trigger | What it takes | Already exists? |
|---|---|---|---|---|
| 1 | Collected bytes outside the floor | `held > B` | `files/`, in D152's four keys | **Shipped** `[C, store.rs:686-732]` |
| 2 | Collected bytes **inside** the floor, said once per episode | `held > B` and rung 1 exhausted | same | **Shipped** `[C9]`, and D202 has re-affirmed the promise it breaks |
| 3 | Collected bytes, whatever the budget says | `free < R` | same | ORCH-095, Alexa's yes, unbuilt |
| 4 | **Whole records** — tombstone, bytes, directory — oldest outside the floor first | `free < R` and rung 3 exhausted | `job.json`, `events.jsonl`, `log`, `log.seq`, the directory | new |
| 5 | Nothing. Say so once per episode, with both figures | `free < R` and rung 4 exhausted | — | the discipline at `[C10]` |
| — | **The backstop**, in parallel and secondary: a record past its class's maximum age goes whether or not anything is under pressure | a clock, once a day and once at startup | as rung 4 | new; this is D201's class map, demoted (§3.1) |

**The two numbers that bound rung 4 both already exist** `[P]`:

> **`--output-floor` becomes the record guard as well as the byte floor.**
> No record is ever removed by *space pressure* while it is inside the
> floor. **The retention spans become the maximum age.** Space pressure
> may act anywhere between the two; the backstop acts at the far end.

That is the whole of "age is a guard, not the trigger", in two numbers
Alexa has already set. It also gives **ORCH-100**'s startup refusal a
sharper form rather than removing it: refuse a configuration whose
**shortest span is below the floor**, naming both figures, because then
there is no window in which space pressure may act and the two numbers
contradict each other. Same shape as before, better reason.

**Rung 4 never breaks its guard, and that is the one asymmetry with the
bytes.** Rung 2 breaches the floor loudly because the budget is a fact
about a disk, and the manifest survives — the requester is told what was
there. **A record removal destroys the manifest**, and a tombstone is a
sentence, not evidence. So a deficit that survives rung 4 is **reported,
never forced** `[P]`.

### 3.3.2 What happens to what can no longer serve

§0.6 is the rule and this is where it lands in the mechanism. A record
rung 4 or the backstop selects is not simply unlinked:

1. If an **archive path** is configured, the record moves there and the
   job directory goes. The archive is on another filesystem by the whole
   point of it, so the move is a copy and an unlink, not a `rename`, and
   it must be reported (§0.7) and counted.
2. If none is configured, the record enters a **transitional state** under
   a name that says so, and is removed when the **delay** expires. The
   transitional bytes are still Herd's: they appear in the report with
   their size, they count toward the space calculation, and an overflow
   is signalled.
3. Either way the **tombstone is written first** and survives both, so
   the requester's question has an answer whatever happened to the bytes
   (§3.2).

**Both figures — the delay and the archive path — are Alexa's and are not
invented here** (§0.6). Unset archive path means removal after the delay;
what the delay defaults to is a decision, not a derivation.

### 3.3.3 What the two mechanisms must share rather than duplicate

They can contradict each other in exactly one configuration: a record
past its maximum age whose bytes are still inside the output floor. With
a span at or above 14 days and a seven-day floor it cannot arise; with
`--output-floor 30d` and a 14-day span it does, and then one mechanism
says keep and the other says go. **Refuse that configuration at startup,
naming both figures** `[P]` — the style `herd-hub/src/main.rs` already
uses, where a budget that does not parse stops the hub before any byte
has moved `[C11]` and *"a timing that cannot mean what it says is refused
here, before the port is bound"*.

**The hazard that makes the removal order non-negotiable, and it is
untouched by the inversion.** A `files/` whose `job.json` is gone is
permanently `could_not_free` `[C16]`, so the running total drifts up,
never comes down, and the sweeper can never reach the budget again.
Therefore **bytes first, record last, in one act**: the reverse order
leaves exactly that orphan if the hub dies in the middle. This is why the
old section's second sentence survives its first one — it was never a
statement about precedence.

* **One writer, one guard.** The sweeper has a one-at-a-time flag; the
  record pass takes the **same** one, not a second. D8's one-writer
  discipline, and D152's reason for refusing a timer was precisely that a
  second writer waking on a quiet hub is the wrong shape.
* **One removal discipline.** Rename to make it invisible, write, unlink
  with no lock held, clean the leftover on the next pass — exactly what
  `files.gone` already does `[C8]`, applied to the job directory.

### 3.4 The cost: a fifth walk, and a sixth hiding behind it

`docs/remediation/03c-contradictory-review.md:360-386` counts the walks
of the whole volume: `collections_owed` on every runner join
(`server.rs:2415`), `collections_owed` inside `send_the_files` for one
requester's `AskJobFiles` and therefore on every 200 ms poll
(`server.rs:4384`, `:6245`), and `sweep_now` reached from `held_more`
`[C4, C5]` — all O(all jobs ever) because nothing deletes a directory
`[C13]`. ORCH-021 tracks the poll.

A naive retention pass is a **fifth**, and it is the expensive kind: the
sweep under budget reads no records at all `[C5]`, while retention must
read every record to learn its priority and its end time — the
`collections_owed` profile, measured by D152 at 0.87–1.31 s per 100 000
records on this host. This document first scaled that figure linearly and
printed **30–45 ms at 3334 records** and **0.3–0.45 s at 33 340** `[D]`.

**Those two figures were measured on 2026-10-28 (ORCH-104) and the warm
ones are about twice what was derived, while the cold ones are seven to
eleven times it.** The walk is linear — that part of H6 holds — and the
derivation's error is not its scaling but its cache and its corpus:
`[M]`

| directories | sweep walk warm | sweep walk cold | record walk warm | record walk cold | record walk, per dir warm |
|---:|---:|---:|---:|---:|---:|
| 1 000 | 3.65 ms | 5.77 ms | 19.4 ms | 87.0 ms | 19.4 µs |
| 3 334 | 13.8 ms | 19.8 ms | **65.6 ms** | **292 ms** | 19.7 µs |
| 10 000 | 39.6 ms | 59.5 ms | 190 ms | 873 ms | 19.0 µs |
| 33 340 | 137 ms | 205 ms | **639 ms** | **3.29 s** | 19.2 µs |
| 100 000 | 620 ms | 1.74 s | 1.99 s | 15.2 s | 19.9 µs |

Release build, `collections_owed` through the public trait, median of
five warm runs, one cold run per cell; the 100 000 row was taken at a
1-minute load of 8–11 and the rest at 2.9–4.8. **Cold** means every
record `fsync`ed and then `posix_fadvise(DONTNEED)`ed, which drops the
page cache and not the dentry cache — a hub restarted onto an old state
root is colder still, so the cold column is a floor and not a ceiling.
Method and the full tables: `herd-hub/tests/walk_cost.rs` and
`docs/orchestration/history/2026-10-28-orch-104-walk-cost.md`.

**Three things that table says, and only the first was expected** `[M]`:

1. **Linear, flatly so.** The per-directory warm cost is 19.4, 19.7,
   19.0, 19.2 and 19.9 µs from 1 000 to 100 000 directories. Nothing
   bends over two orders of magnitude, so **H6's scaling claim is
   confirmed** and a retention pass can be sized by multiplication.
2. **The cold cache is the whole risk, and it is a factor of 4.4 to 7.6.**
   Nobody had measured a cold walk. The startup pass §3.4 proposes runs
   on exactly the cold cache this column measures: **3.3 s at 33 340**,
   not 0.45 s.
3. **The cost is concentrated, so the mean is not the record.** One
   record in ten in this fixture carries a 240-file manifest, as the
   consumer's own output distribution is deciles 1–8 at 0.0 MB with the
   top tenth carrying 95 % of the bytes. Deciles 1–8 of the per-record
   warm cost are 6.5–8.1 µs, decile 9 is 31.9 µs and the maximum is
   215 µs: the **top tenth of records carries about two thirds of the
   walk**, and the mean, 19.2 µs, is 2.9× the median. The mean is still
   the right multiplier for a total — a total *is* `N` × mean — but it
   describes no record, and a corpus of uniformly small records
   understates the walk by about 1.8×, which is most of the gap between
   the derived figures and these.

And there is a **sixth** behind it, which nothing has named: `usable_index`
trusts the index only while `index.jobs_accounted` equals the number of
job directories `[C15]`. A pass that removes directories without updating
that count forces a **full index rebuild — every record read — at the
next startup**. Retention must update the index in the same act, or the
trust check must stop being a raw count.

**And id order is also about half the cold cost per record, which is a
second reason for the proposal below and was not known when it was
written** `[M]`. Every walk in the store reads the names
`durable::names_in` hands back, which is ext4's directory-hash order:
at 33 340 directories **zero** of 33 341 names sit where sorted id order
would put them. Reading the same records in id order costs **0.50× at
3 334 and 0.41× at 33 340** on a cold cache — measured in both sequences,
1.83× and 2.01× with the orders reversed — and **nothing at all warm**
(0.99× and 1.02×), which is the control that says the effect is the disk
and not the loop. So a full cold pass at 33 340 is ~3.3 s in the order
the store walks today and **~1.7 s in id order**: a larger saving than
collapsing `read_stored`'s three parses buys warm (ORCH-123), on the
pass that delays a hub coming up.

**How to add retention without a fifth O(T) walk** `[P]`: job ids are
sequence-ordered and `sequence_of` already filters them
(`herd-hub/src/store.rs:2559`, `:650-653`), so the pass walks in id order
and **stops after K consecutive records inside the window** — O(expired +
K) instead of O(T). `updated_at` is not strictly monotone in id (a long
job ends after a later short one), so K bounds the skew and a **full pass
at startup** catches the outliers. A record lingering a day past its span
is a disk cost, never a correctness one.

**The trigger is a clock, and that applies D152's rule rather than
breaking it.** D152 refused a timer for the budget because *"the only
thing that can put a hub over its output budget is a collection
arriving"*. The rule underneath is *trigger on the thing that can change
the answer*; for retention that thing **is** the passage of time. **Once
a day, plus once at startup**, matching the sweep's startup run `[C11]`.

**Can a daily full pass be afforded? Yes, by a wide margin, and the
startup pass is the one to watch** `[M]`. At the first consumer's 3334
directories a full record walk is **65.6 ms warm and 292 ms cold**, once
a day: that is free, and the cold figure is the one that counts because
a daily pass on a hub that has been quiet reads a cache nobody has
warmed. At ten times their count — 33 340, about ten months of their
own id rate `[R6]` — it is **639 ms warm and 3.3 s cold**. A once-a-day
3.3 s unlocked read is still affordable; it is **not** affordable as
anything that runs per event, and the number to compare it against is
not 200 ms but *how long a hub may be slow at startup*. Where it stops
being affordable, said as a rule rather than a date: a full cold pass
costs about **100 µs per directory** on this host `[M]`, so it crosses
10 s at ~100 000 directories and a minute at ~600 000. **At 100 000 the
startup pass alone is 15.2 s of a hub not yet serving** — which is why
§3.4's early-stopping id-ordered pass is the right shape for the daily
one and why the startup full pass needs a bound of its own, not because
the daily cost is high.

**Amended 2026-10-29: every figure in this section survives, and what it
sizes is now the backstop.** The walk is still linear, the cold cache is
still the whole risk, the cost is still concentrated in the top decile,
and the sixth walk behind it — `usable_index`'s raw count `[C15]` — is
still a correctness requirement on any pass that removes a directory.
What changes is only the name of the thing being sized: this is **the
backstop's** daily and startup pass (§3.3.1), not the primary mechanism.

Two consequences of the demotion, and the second is a small gift:

* **The startup pass still needs its own bound** — 292 ms cold at their
  3 334 directories and **3.29 s at 33 340** `[M]`, which is a hub not yet
  serving. Unchanged, and it is ORCH-157's.
* **A space-triggered pass does not need a walk of its own.** Rung 4 fires
  when `free < R`, which is already a sweep trigger, and it selects from
  the candidates the sweep's own walk has already read. The fifth walk is
  the backstop's alone, once a day; the space path adds none.

### 3.5 What must never be deleted, under any rule

* **A queued, assigned or running job.** `record.state.is_terminal()` is
  already the sweeper's first test and `JobState::Finished` its candidate
  test `[C6]`; retention takes the same ones.
* **A record that never finished** — a hub killed mid-attempt — is
  terminal-less for ever and must never be swept away silently. It is
  counted at startup (`unfinished_jobs`, `herd-hub/src/store.rs:2350`,
  reported at `herd-hub/src/main.rs:231`) and it belongs to the reaper
  (ORCH-012, D134), not to retention. Retention must not become the
  reaper by accident.
* **A collection still arriving**, and its `parts` directory
  (`herd-hub/src/store.rs:1994`): `collection_is_whole` is already the
  test `[C6]`.
* **A quarantined record, or any record that will not parse** `[C16]`. An
  unreadable record has no readable end time, so it can never be *shown*
  to be past retention — which is the argument, not just the precedent.
* **Anything a requester is following or fetching.** This is a **seam,
  not a check**: the store knows nothing about sessions and the live
  follower set is the server's. The server should hand the store a set of
  protected ids per pass, so the store stays ignorant of sessions `[P]`.
  ORCH-102.
* **Anything that is not Herd's.** `recover` already says of a foreign
  directory *"this directory belongs to its owner, not to Herd"*
  (`herd-hub/src/store.rs:870-874`); retention keeps that promise, and
  `readings/`, `index/` and the tombstones are not job directories.
* **The job-id high-water mark.** See §3.6 — it is a blocker, not a
  caveat.

### 3.6 The blocker: job ids are reused once records are removable

`next_sequence` is re-derived at every open from the **highest surviving
job directory name**, and nothing persists it — `IndexFile` carries
`index_version`, `jobs_accounted` and `pending` only `[C14]`.

Today that is safe, because no directory is ever removed. The moment
records expire it is not:

* in the ordinary case retention takes the oldest, the newest survives,
  and the high-water mark holds;
* **but a hub idle longer than its retention window loses every
  directory**, and the next start mints `j0000000001` again — reusing
  ids that tombstones, the consumer's logs and their issue tracker still
  name. A 14-day span and a three-week pause is not a hypothetical: the
  consumer's nightly is off right now `[R7]`.

**Persist a high-water mark and never mint below it, before any record
becomes removable.** `[P]` This is the first task of M12's retention, not
part of it. ORCH-094.

**Added at this document's integration by `herd-merger`, and not
reviewed with the rest of it. The diagnosis above stands; §7's proposed
placement does not.** The mark was implemented on
`orch-094-id-highwater`, PR #97, as `ids_minted` in **`store.json`**
beside `state_root_id` — not in `IndexFile`. The reason is evidence and
not taste: `index/state.json` is *derived*, and every path in
`usable_index` that doubts it throws it away and re-reads the records.
That is right for a summary the directories can rebuild and fatal for a
number nothing on disk can reconstruct once those directories are gone.
`store.json` is the root's authoritative file, already refuses the start
when unreadable, and its neighbouring field answers the companion
question: `state_root_id` says *which* root a `j…` belongs to, and the
mark says that one root never hands the same `j…` out twice.

---

## 4. Hypotheses, and what would refute each

| | Hypothesis | What refutes or settles it | Can we? |
|---|---|---|---|
| H1 | One nightly pass leaves 95–116 GB on the hub (§2.2) | A per-directory size listing of `jobs/` with dates: it shows one pass or several. Refuted if `N ≤ 36 GB` | **No.** Needs root on the consumer's host (§1.3, R8, D161) |
| H2 | ~540 MB is a *typical* recent directory, not the largest one | The same listing, read as a distribution rather than a mean | No, same read |
| H3 | The volume currently holds about one nightly pass | The same listing plus directory dates | No, same read |
| H4 | The breakdown's "older than seven days" is directory mtime, while the hub's floor is `record.updated_at` `[C7]`. Any record rewrite — a first fetch, an eviction — bumps the directory's mtime, so `find -mtime +7` **undercounts** what the hub calls old, and the 12 GB `[R4]` is a **lower bound** on the out-of-floor pool | Comparing the two figures. Nothing prints the hub's own figure today, which is one more reason for M13's status document | Partly: the fix is ours, the comparison is theirs |
| H5 | The arrival process is one burst a night, so a daily mean misdescribes it (§2.1) | The consumer's own words `[R7]`; refuted if the pass actually spreads its output across the day | No |
| H6 | D152's walk timings scale linearly from 100 000 directories down to 3334 and up to 33 340 (§2.5, §3.4) | A measurement at the real directory count. D152 itself calls its figure *"one host, one filesystem, warm cache"* | **Settled 2026-10-28, ORCH-104, and it splits in two.** The *scaling* is **confirmed**: the warm record walk is 19.4/19.7/19.0/19.2/19.9 µs per directory at 1 000/3 334/10 000/33 340/100 000, flat over two orders of magnitude `[M]`. The *figures it was used to derive* are **refuted**: §3.4's 30–45 ms and 0.3–0.45 s are about half the measured warm cost and a seventh to an eleventh of the measured cold cost, because the derivation inherited D152's warm cache and its uniformly tiny records. §2.5's sweep figure was wrong the other way — too high, because it scaled the record-reading profile onto a walk that reads no records. Both are now `[M]` in the table at §3.4 |
| H7 | "In-floor eviction every night" assumes the nightly returns at this size with the budget at 200 GB | The nightly is off `[R7]`, so nothing will show until it returns | No, and this is why the warning of §2.7 matters more than the prediction |
| H8 | `statvfs` on the state root measures the filesystem Herd shares (§2.6) | True here: the hub volume is a directory on the same ext4 `[R5]`. A separate mount, an overlay or a quota would measure something else — and the guard would then be measuring the right thing for *that* deployment anyway | Yes, by test |
| H9 | A `herd_herd-state` docker volume maps to a plain directory under `/mnt/ssd_980/docker/volumes` | `[R5]` says so. If it were a separate device the whole of §2.6 still holds, with different numbers | No |
| H10 | `r_file` holds **prospectively** at about 5, so §0.4's rate and §2.4's retirement hold going forward | **Their figure, read forward** `[R12]`. A second `herd-hub report` reading, days apart. Their 5.04 is over 14 days that include the pre-streaming era, when a job directory was kilobytes, so the ratio over today's mix could be higher (a longer window accumulates copies) or lower. **This is the one hypothesis the whole of §0.4 and §2.4 rests on** | Yes, by one command on their side — §1.3's remaining ask |
| H11 | Nothing on the hub bounds a log: the only cap is the runner's `--job-log-cap`, default 64 MiB and **per attempt**, and the hub appends what arrives | **Read from the code at `cd0f1b7`, 2026-10-29** `[C, confine.rs:50; store.rs:2469-2509, :2283-2300; server.rs:194; main.rs:149-151]`. Refuted by a hub-side cap, and there is none. **Verified rather than inherited**, because it is what reorganises §0.2 | Settled |
| H12 | Records and logs stay a negligible share of the state root, so record removal is not a space instrument | **Measured by them**, 0.32 % on 2026-10-29 `[R16, D]`. Refuted by a workload whose logs are large: a chatty job can legitimately bring 64 MiB, twice that over two attempts, so a fleet of chatty jobs moves this. It is a statement about their workload, not about Herd | Yes, by a second reading — and it is the figure that would reopen §0.3 |
| H13 | The state root's filesystem supports hard links, so §2.9 works there | ext4 `[R5, R11]`. An overlay, a cross-device bind or a filesystem without links refuses, and the answer is a `link()` probe at startup with a plain-write fallback said once (§2.9 D4). **The hypothesis does not have to hold for the design to ship**, which is the point of the probe | Yes, by test, on any host |
| H14 | `st_nlink` is a sufficient reference count: no ledger beside the disk | **Inferred.** Refuted by a blob that must survive at `nlink == 1`, or by a sequence where the count is read non-atomically against a concurrent `link`. The second is real and is answered by taking the store's existing mutex for the `stat` plus `unlink` and for the `link`; both are O(1) metadata calls, and the failure if it is got wrong is a **missed deduplication**, never a lost byte | Yes, by test |
| H15 | Space pressure cannot bound the **record count**, because 41.3 % of records hold no collected bytes `[R14]` | It would die if those records were large in log bytes. **R16 says they are not**: ≈ 104 KiB a job over the whole population. So space pressure provably cannot bound the count here, and the backstop is the only thing that can — **strengthened, not refuted** | Settled by R16 |

**Amended 2026-10-29.** H1, H2 and H3 are **narrowed and not settled**:
R15's deciles say ~540 MB describes the top decile at most, and 214
groups at the top-decile mean would be ≈ 67 GiB a pass rather than 95 to
116 GB `[D, §2.4]` — but that assumes a nightly pass *is* the top 214
records, which nothing shows. The per-directory listing with dates is
still what would settle all three, and it is still theirs to run or not
(§1.3). H8 is **confirmed** by R11: the state root is a Docker volume on
one shared filesystem, so `statvfs` on it measures what the free-space
guard needs to see. H9's own id is taken by the docker-volume question,
so the ratio's hypothesis below is **H10** and not, as a scratchpad draft
had it, a second H9.

---

## 5. What is outside this mandate

* **The runner's 65 GB in 125 spools `[R9]` is ORCH-092's.** It changes
  exactly one number here, and it changes it importantly: those spools
  are on the **same filesystem** as the hub's volume `[R5]`, so they are
  one of the neighbours the free-space guard of §2.6 exists to survive.
  Herd therefore has **two budgets over one disk and nothing reconciles
  them** — the hub's `--output-budget` and the runner's own. Naming that
  is this document's job; solving it is not.
* **ORCH-021's fix** (a derived index so `collections_owed` stops
  re-reading history per poll) is not mine. Retention must not make it
  worse, and §3.4 shows it can make it better.
* **The reaper's janitor list** (ORCH-012, D134) owns the non-terminal
  records of §3.5.
* **M13's status document** is where use-against-budget belongs — the
  one thing M12 still owed on the output side before this document, per
  `docs/plan.md` M12 and D152.
* **Anything on the consumer's machines.** Invariant 9, D161.
* **`docs/remediation/`** is a closed dossier about something else and
  nothing here belongs in it.

**Added 2026-10-29, each named so it is not discovered later:**

* **The compaction pass.** The only way the 84.94 GiB already on their
  volume is ever recovered (§0.4). It re-reads and re-verifies ~105 GiB
  offline and it is an operator-run act on their machine (D161). Its own
  row, its own decision, after deduplication has shipped and been seen to
  work.
* **The archive path's own disk** (§0.6). Its capacity, its filesystem and
  its own bound are the operator's. Herd writes there if told to, reports
  what it put there, and makes no promise about the volume underneath. A
  second `--volume-free-floor` for a second filesystem is **not**
  proposed here and would need its own argument.
* **Pack deduplication.** A pack is addressed by its content and
  deduplicated by construction; the 5.04 of R12 is job evidence. The two
  are not folded together and §2.9 does not touch packs.
* **The hub as a pack cache** — deferred, Alexa's, not reopened here.
* **ORCH-108** — whether a runner may ever delete an unclaimed spool —
  is still Alexa's, and it is load-bearing for `R`'s startup sentence:
  part of Herd's reserve is spent protecting the disk from Herd.
* **ORCH-165** (an orphan `files.part/` is unreachable, unbudgeted
  garbage) and **ORCH-166** (`report` names the extremes and misses the
  middle) are neighbours. §2.9 must not grow either of them.
* **A hub-side log cap** (§0.3). A guard worth having, sized by nobody,
  and explicitly **not urgent** at R16's figures.


---

## 6. Decisions for Alexa

One at a time, each with a recommendation and a fallback. None of these
is the orchestrator's to take: each changes *what* is built or is
outward-facing. **Re-cut 2026-10-29**: A, C and D are answered or
retired and are kept with their answers; E and F are the open ones.

**A. The seven-day floor. Answered by D202, and the question that
prompted it is retired.** This document asked on 2026-10-27 whether to
keep the shipped seven-day default when it could not be kept at the
consumer's workload. Alexa kept the floor (D202, 2026-10-27), and
§2.4 now retires the premise: with deduplication the floor is keepable
there, with eighteen times the margin on the rate reading `[D]`. **What
survives of the question** is the honest-sentence half: a hub on a disk
where the floor *cannot* be kept must say so rather than keep promising
it. That is a build task, not a decision. **No part of this retires
`--output-floor`**, and the 2026-10-28 scratchpad proposal to replace it
with a byte figure is dead on arrival.

**B. A second disk knob, `--volume-free-floor` (§2.6). Recommendation:
yes, and it is now a prerequisite rather than a neighbour.** One number
bounds what Herd *takes*, the other what Herd *leaves*; the state root
shares a filesystem with the consumer's whole project `[R11]`, so a
shared volume needs both. It is a bound, not a policy: no job can be
named in it and no job is more important than another under it. **And
its figure for that disk already exists** — Alexa's standing rule is
alert above 85 % used `[R18]`, so `R` there is 15 % free ≈ 137 GiB
`[D]`, about three times what §2.6 proposed as a default. **Fallback:**
no new flag, and accept that a neighbour can fill the disk under a hub
that is inside its budget and sweeping nothing. That fallback is worse
after 2026-10-29 than before it, because the free-space guard is also
the only trigger that can ever see a record byte (§0.2).

**C. The retention class map: merger 90 days, every other level 14
(§3.1). Taken as D201, and demoted on 2026-10-29.** The two numbers are
unchanged and now mean **maximum age** rather than deletion time. Nothing
is asked here again; §3.1 records exactly what survives.

**D. One read on the consumer's side (§1.3). Superseded.** The reading of
2026-10-29 answers most of what it wanted, read-only and without root.
**ORCH-103 should close.** What replaces it is a cheaper and more
valuable ask, and it is **E** below.

**E. Deduplication on by default (§0.4, §2.9), with the threshold it
cleared.** `herd-hub report` deliberately prints no threshold (D194), and
the shape proposed and never recorded was *build file-level
deduplication only if `r_file >= 4`*. Their reading says **5.04**
`[R12]`. **Recommendation: yes, and record it with the threshold it
cleared**, so the figure stops being a guess: 5.04 against 4, on their
own volume, read by a shipped binary. **The cost, said plainly:** the
saving on their existing 105.57 GiB is **zero on day one** and arrives as
a rate (§0.4); an older binary meeting a shared root leaks orphan blobs
until an upgraded one collects them; a filesystem without hard links
degrades silently to today's behaviour, said once at startup. **Fallback:**
ship the linking off behind a flag for one release and turn it on at the
next — which costs a knob, a second code path and a release, and buys a
reversibility that removing `objects/` after a plain-write fallback
already gives.

**F. The two figures of §0.6, and they are the only numbers this document
deliberately leaves blank.** The **delay** before something that can no
longer serve is removed, and the **archive path**. Both are Alexa's: a
shipped binary stating an unrecorded policy is a number nobody can later
change (D194). **Recommendation: ask for the delay and treat the archive
path as optional** — unset means removal after the delay, which is the
behaviour that needs no second disk. **Fallback:** no delay and no
archive, which means removal in the same act as the selection, and the
principle of §0.6 — never immediately, never silently — then holds only
in its second half.

**G. One relay, and it is one ask (§1.3).** A **second** `herd-hub
report` run, days after the first, pasted whole, and one
`herd-hub retention-preview` run with the spans of D201. Two stocks days
apart are a rate; one stock is a history, and every rate in this document
is a stock wearing a rate's clothes. It settles **H10**, on which §0.4
and §2.4 both rest. Nobody in this project runs either (invariant 9,
D161). This is a relay, not a decision: Alexa asks or does not.

**Two records for `herd-merger` to write, and they are not questions.**
The direction is already Alexa's; what is missing is the entry. (i) **The
spans are demoted** — D201's two numbers become a maximum age plus a
guard, with `--output-floor` as the record guard, and the trigger becomes
space. An entry that amends D201 without editing it; `docs/decisions.md`
is append-only. (ii) **The budget keeps its documented scope** — `B`
governs the collected files, `R` governs the filesystem, and record
removal is `R`'s because a log is outside `B` by design.

---

## 7. The work, as board rows

Ids after ORCH-093. **Written here, not in `docs/orchestration/board.md`:
`herd-merger` owns that file and merges rows in one branch at a time.**
If one of these ids collides at rebase, renumber from the first free id
and keep the order.

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-093 | The hub's disk policy: re-derive `--output-budget` from the consumer's figures and size M12's record retention | mandate 2026-10-27; D148's open debt | herd-planner | done | Merge this document; brief ORCH-094 and ORCH-095 from it | — | `docs/disk-policy.md` |
| ORCH-094 | **Blocker.** Persist the job-id high-water mark: `next_sequence` is re-derived from the highest surviving directory, so a hub idle past its retention window re-mints `j0000000001` | `docs/disk-policy.md` §3.6; `store.rs:879`, `:229-233` | herd-step | done | **Done 2026-10-27 on PR #97, and not where this row proposed it.** The mark is `ids_minted` in `store.json` beside `state_root_id`, honoured at `recover`, raised and `fsync`ed before the `mkdir` that claims the id, with a test that removes every directory and asserts no id is re-used. `IndexFile` was the wrong home: `usable_index` discards it on any doubt. Amended at integration by `herd-merger`; the reason is at the end of §3.6. **This row is not brought onto the board — PR #97 carries the board's ORCH-094 row** | — | `herd-hub/src/store.rs` (`StoreFile::ids_minted`, `mark_minted`, `recover`, `mint`); PR #97 |
| ORCH-095 | The budget becomes a function of the volume: `B` defaults to `min(200 GB, 25 % of capacity)`, new `--volume-free-floor R`, effective ceiling `min(B, held + max(0, free − R))` from `statvfs` at each trigger, and a startup line that distinguishes a derived figure from the fallback | `docs/disk-policy.md` §2.6; D148's debt | herd-step | waiting | Alexa decides §6 B first; then one task over `main.rs`, `job.rs` and `held_more` | Decision §6 B | `docs/disk-policy.md` §2.3, §2.6 |
| ORCH-096 | Warn on the approach, in days and not in percent: warn when `B − held` is less than what arrived in the last 24 h, from one rolling counter on `Live`; and bring the inside-the-floor line under the once-per-episode flag the over-budget line already has | `docs/disk-policy.md` §2.5, §2.7; `store.rs:797-841` | herd-step | owed | One task over `store.rs` after ORCH-095 lands | ORCH-095 | `herd-hub/src/store.rs:810-841` |
| ORCH-097 | `--output-budget` dry run: what would this budget take, with nothing removed. The walk, the record reads and the four-key sort already exist; the dry run stops before `evict` | `docs/disk-policy.md` §2.8 | herd-step | open | Size it; decide whether it is a hub flag or a `herdctl` call | ORCH-095 | `herd-hub/src/store.rs:642-732` |
| ORCH-098 | M12 record retention, the engine: two classes (merger 90 / all others 14, configurable), a daily clock plus a startup pass, id-ordered walk stopping after K consecutive in-window records, the sweeper's one-at-a-time guard shared, `index.jobs_accounted` updated in the same act, bytes removed before the record | `docs/plan.md` M12; E-15; `docs/disk-policy.md` §3 | herd-step | waiting | Alexa decides §6 C; then one task, and it is the largest here | ORCH-094, decision §6 C | `docs/disk-policy.md` §3.1, §3.3, §3.4 |
| ORCH-099 | M12 tombstones: append-only monthly shards carrying id, priority, outcome, `ended_at`, `removed_at`, file count and total bytes; `herdctl` answers for a removed record with that sentence instead of "no such job" | `docs/disk-policy.md` §3.2 | herd-step | owed | One task over the store and `herdctl`, after the engine | ORCH-098 | `docs/disk-policy.md` §3.2 |
| ORCH-100 | Refuse at startup a configuration where `--output-floor` exceeds the shortest record retention, naming both figures | `docs/disk-policy.md` §3.3 | herd-step | owed | Small task in `main.rs`, with the other startup refusals | ORCH-098 | `herd-hub/src/main.rs:215-224` |
| ORCH-101 | The startup line stops promising a record *"for ever"* (`main.rs:428`); `docs/plan.md` M12 and `docs/coverage.md` E-15 point at `docs/disk-policy.md` | `docs/disk-policy.md` §3.2 | herd-step | owed | Prose plus one `println!`; must land with ORCH-098, never before | ORCH-098 | `herd-hub/src/main.rs:428` |
| ORCH-102 | The server hands the retention pass a set of protected ids — jobs being followed, fetches in flight — so the store stays ignorant of sessions | `docs/disk-policy.md` §3.5 | herd-step | owed | One task at the server/store seam | ORCH-098 | `docs/disk-policy.md` §3.5 |
| ORCH-103 | Relay to Alexa: ask the consumer for one per-directory size listing of the hub volume's `jobs/`, with dates. It settles H1, H2 and H3; it needs root on their host; nobody in this project runs it (D161) | `docs/disk-policy.md` §1.3, §4 | Alexa | waiting | Ask, one question, with the two commands of §1.3 | Alexa | `docs/disk-policy.md` §1.3 |
| ORCH-104 | Measure the sweep walk at a realistic directory count (3334 and 33 340), refuting or confirming H6's linear scaling of D152's 100 000-directory figures | `docs/disk-policy.md` §4 H6 | herd-step | done | **Done 2026-10-28 on `orch-104-walk-cost`. Linearity confirmed, the figures refuted, and no product file touched.** Five populations — 1 000, 3 334, 10 000, 33 340, 100 000 — cold and warm, release and debug, in `herd-hub/tests/walk_cost.rs` (all cases `#[ignore]`d). The warm record walk is flat at ~19 µs a directory over two orders of magnitude; §3.4's derived 30–45 ms / 0.3–0.45 s become **65.6 ms / 639 ms warm** and **292 ms / 3.29 s cold**; §2.5's sweep figure was too high, not too low. **ORCH-123 confirmed and sharpened:** per-record work is **98.6 %** of the warm walk, not 94 %, and the three `serde_json` parses are **81 %** of the per-record cost in a **release** build, not only in debug — but collapsing them into one parse buys only **23 %** of the walk, so the row's lever is real and about a quarter the size its framing suggests. Cold, the attribution inverts: the disk is 72–76 % and the parses 24–28 %. **A fourth finding nobody was looking for:** every walk in the store reads in ext4 directory-hash order, and sorting the names into id order first costs **0.41–0.50× cold** and nothing warm, so a full cold pass at 33 340 is ~1.7 s instead of ~3.3 s. Three new rows proposed in the handoff | — | `herd-hub/tests/walk_cost.rs`; §3.4's table; `history/2026-10-28-orch-104-walk-cost.md` |

### 7.1 Re-cut 2026-10-29: the rows this direction adds and changes

**Written here, not in `docs/orchestration/board.md`: `herd-merger` owns
that file and merges rows in one branch at a time.** Ids after ORCH-167,
the board's highest at `cd0f1b7`; if one collides at rebase, renumber
from the first free id and keep the order.

| ID | Item | Owner | State | Depends on | Must not touch |
|---|---|---|---|---|---|
| ORCH-168 | **The figure the hub can report itself.** `retention-preview` splits record bytes from output bytes on **both** sides of every span and prints one root-wide total of each. `Seen::record_bytes()` already exists `[C, retention.rs:277-283]`. R16 now gives the figure; this makes the hub give it, which is §0.7's first requirement | herd-step | open | — | `report.rs` (ORCH-166), `store.rs`, any writer |
| ORCH-169 | **The relay of §6 G**: a second `herd-hub report` reading days after the first, plus one `retention-preview` run. It settles H10 | Alexa | waiting | ORCH-168, so the preview they run carries the split | — |
| ORCH-170 | **This document.** §0 written, §2.4 retired, §3.3 inverted, `docs/plan.md` M12 and `docs/coverage.md` E-15/E-16 re-pointed | herd-step | done | — | `decisions.md`, `board.md`, code |
| ORCH-171 | **Deduplication by hard link** (§2.9): `objects/<sha256>`, both write paths linking, a `link()` probe with a plain-write fallback said once, blob collection by `st_nlink` inside the removal act, a bounded `objects/` sweep at startup, the eviction sentence carrying **unlinked** and **freed**, and **this work moves `LAYOUT_VERSION` by nothing** — it must stand, when the work is done, at exactly the number it stood at when the work began, whatever that number is (2 at the time of writing, `[C, store.rs:99]`). Read it as the requirement and not as the figure: the figure is `main`'s and moves without asking ORCH-171, and a criterion written as *"unchanged at 1"* was already unsatisfiable once | herd-step | waiting | Decision §6 E; ORCH-170 for the words | `herd-proto`, `herdctl`, `server.rs`, `job.rs` |
| ORCH-172 | **The budget starts meaning disk bytes**: `directory_bytes`/`held` count a shared blob once, the sweeper measures progress in bytes actually freed, **eviction by blob** with a blob as protected as its most protected claimant, and **no candidate destroyed that frees nothing** | herd-step | owed | ORCH-171 | `herd-proto`, the record's fields |
| ORCH-095 | *(existing, Alexa's yes)* The free-space guard. **Promoted**: it is now the prerequisite of record removal, not a neighbour, and its figure for their disk is R18's 15 % | herd-step | waiting | Decision §6 B; ORCH-172 | retention, records |
| ORCH-173 | **Record removal under space pressure** — rung 4 (§3.3.1). One act, tombstone then bytes then directory, taking the sweeper's existing one-at-a-time guard, updating `index.jobs_accounted` in the same act, reusing `retention::select` for the guard test and D197's refusals, and taking a protected-id set from its first line of code. Restates ORCH-098's space half and folds ORCH-102 | herd-step | owed | ORCH-095 | `herd-proto`, `matcher.rs`, `queue.rs`, `herdctl` |
| ORCH-174 | **The backstop, demoted** (§3.1, §3.3.1): once a day and once at startup, id-ordered, stopping after K consecutive in-window records, calling ORCH-173's removal act, with the startup pass carrying its own bound. Restates ORCH-098's calendar half and **consumes ORCH-157** | herd-step | owed | ORCH-173 | `herd-proto`, the budget |
| ORCH-175 | **Nothing is kept without a bound** (§0.6, §3.3.2, §0.7): the transitional state and its delay, the optional archive path, and the three visibility requirements — reported with its size, counted toward the space calculation, overflow signalled | herd-step | waiting | Decision §6 F; ORCH-173 | the tombstone's format (ORCH-099) |

**Changes to rows that already exist**, and none of them is written here:

* **ORCH-103** (the `du` of every job directory) **should close**:
  superseded by the reading of 2026-10-29 (§1.3).
* **ORCH-145** (will the operator run `report`) **should close**:
  answered — they did, and R12 to R17 are the answer.
* **ORCH-097** (the budget dry run) survives **re-pointed**: it is a
  second question for `retention-preview` rather than a second
  subcommand. One tool, one walk, two questions, no new CLI surface.
* **ORCH-098** splits into ORCH-173 (space) and ORCH-174 (backstop), with
  ORCH-102 folded into ORCH-173 — it cannot be a follow-up, because a
  pass that cannot be told what is in flight removes a directory under a
  working fetch.
* **ORCH-099, ORCH-100, ORCH-101** all survive. ORCH-100's refusal gets
  the sharper form of §3.3.1; ORCH-101's citation is corrected to
  `main.rs:428` (§3.2).
* **ORCH-157** (the startup pass needs its own bound) survives and is
  **consumed by ORCH-174**.
* **ORCH-158** (sort `names_in` into id order, 0.41 to 0.50x cold `[M]`)
  is independent of all of it, is one `sort()`, and halves the cold cost
  of exactly the startup pass ORCH-174 has to bound. It should ride in
  the same batch as ORCH-174 or earlier.

### File ownership and order

**Re-cut 2026-10-29.** `herd-hub/src/store.rs` is the critical path:
ORCH-171, ORCH-172, ORCH-095, ORCH-173, ORCH-174 and ORCH-175 all rewrite
it, **no two may share a worktree or run at once**, and the order below is
both the dependency order and the conflict order. ORCH-087 ("the keeper")
rewrites the same file and must not start beside any of them.

```
any time   ORCH-170  docs, no code file -- the contract, and it comes first
any time   ORCH-168  retention.rs and its test only; then ORCH-169 relays
           -- store.rs, strictly serial, one worktree each --
wave 1     ORCH-171  objects/ + links + collection + probe
wave 2     ORCH-172  the budget means disk bytes; eviction by blob
wave 3     ORCH-095  statvfs, R, the effective ceiling
wave 4     ORCH-173  rung 4: records under pressure (+ the protected set)
wave 5     ORCH-174  the backstop (daily + startup, id-ordered, bounded)
wave 6     ORCH-175  the transitional state and the archive path
wave 7     ORCH-099 (server.rs, herdctl)  ||  ORCH-100 + ORCH-101 (main.rs)
```

**The order of 2026-10-27, kept because the four rows in it are done or
still open on their own terms:**

1. **ORCH-094** — `store.rs` (`recover`, `StoreFile`). Must land before
   anything can remove a record. **Done**, PR #97.
2. **ORCH-095** — `main.rs` (the arguments and the startup line),
   `job.rs` (`Budget`, the defaults), `store.rs` (`held_more`, the
   ceiling). Blocked on decision §6 B, and now in wave 3 above.
3. **ORCH-096** — `store.rs` only (`Live`, `say_what_the_sweep_did`).
   After ORCH-095, because it warns against the effective ceiling, and
   it now watches two quantities rather than one (§2.7).
4. **ORCH-097** and **ORCH-104** are independent of the rest:
   ORCH-104 touches no product file at all and is **done**; ORCH-097 is
   re-pointed into `retention-preview` (§7.1).

**Host bound, corrected 2026-10-30 (ORCH-250):** a heavy agent's build
tree on `/mnt/ssd_980` — the consumer's production disk too — is a
**`debug` tree carrying test binaries**, and such a tree is **~12–14 GB**,
seven measured on 2026-10-30. Not the ~8.5 GB this paragraph gave until
then, which was low by a factor of 1.4 to 1.6 and named no profile. The
profile comes before the number because a `--release` build of the whole
workspace at `v0.4.1` is **283 MiB** — 292,415,975 bytes, one measurement,
2026-10-30 — forty times smaller and the same repository; a heavy agent
builds `debug` with tests, so the `debug` figure is the one that bounds
how many fit. **At most two heavy agents**, a ceiling Alexa lifted on
2026-10-27 for as long as the first consumer is stopped and which returns
the day it resumes (`CLAUDE.md` *Shared host rules*,
`docs/orchestration/workflow.md` *Concurrency and host rules*). And each
`target/` deleted once its branch is pushed, with its `du -sh` and its
profile recorded in the handoff first.
