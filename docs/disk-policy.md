# The hub's disk policy

What bounds the hub's disk, what the first consumer's figures say about it,
and what M12 still owes. It closes the debt D148 wrote to itself — *"the
one measurement that would change it is their distribution of output size
per job per day"* — and it sizes M12's record retention against the same
figures.

**Three voices, kept apart on every line.** `[C]` is **code**, with a file
and a line, and it is the only voice that is checkable here. `[R]` is
**reported** by the first consumer or by Alexa's breakdown of their
figures: theirs, not ours, and **unverified by this project** — nobody
here has looked at their machines and nobody will (invariant 9, D161).
`[D]` is **derived**, and every derived number names the inputs it came
from. `[P]` is a **proposal**: mine, with its reasoning, decided by nobody
yet.

The reader this is written for arrives from `docs/plan.md` M12 or from
`docs/coverage.md` E-15. It is a topic document beside
`docs/runner-confinement.md`, not a decision and not a journal: when the
decisions in §6 are taken, they become numbered entries in
`docs/decisions.md` and this file keeps the arithmetic behind them.

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
| C12 | The startup line names the budget and the floor, and promises *"A record keeps every path, size and digest for ever: only the bytes are ever removed"* | `herd-hub/src/main.rs:283-297` |
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

One figure in this table is not theirs and is ours: the orchestrator ran
`df` on this host minutes after R5 and got `/dev/nvme0n1p1`, ext4,
983 GB / 592 used / 341 free — the same filesystem, the same numbers.
That is a measurement on **this workstation**, which is where Herd is
developed; it establishes that the hub's state root is this machine's
`/mnt/ssd_980` by measurement rather than by anyone's word, and it
answers ORCH-076 yes. It is not a reading of the consumer's job
directories and nothing below treats it as one.

### 1.3 What nobody can read today, and who could

Everything in §2 and §3 turns on one quantity: **how many bytes one
nightly pass leaves on the hub**. It is one read away —

```sh
du -s --block-size=1M /path/to/herd_herd-state/_data/jobs/* | sort -n
ls -ld --time-style=long-iso /path/to/herd_herd-state/_data/jobs/*
```

— and that read needs root on the host that runs the hub. The consumer
cannot run it and correctly declined to work around the permission
(R8). **Nobody in this project runs it either**, including from this
shell and including on the paths named in R5: Herd is shipped, not
operated, and a fact about that machine reaches us as a report (invariant
9, D161). The ask goes to Alexa, who can relay it; it is ORCH-103 below.

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

### 2.4 The finding: the seven-day floor is unsatisfiable here

750 GB is needed; 451 GB is the theoretical maximum and ~250 GB the
practical one. **At the first consumer's workload, on the disk they have,
the seven-day floor cannot be kept at any budget — not by raising the
budget, not by buying back space from the neighbours.** `[D]`

This is a larger finding than "200 GB is the wrong default", and it is
the heart of this document, so here is exactly what would refute it:

> The floor becomes satisfiable if `N ≤ 451 / 7 ≈ 64 GB` in theory, or
> `N ≤ 250 / 7 ≈ 36 GB` in practice. The evidence puts `N` at 95–116 GB:
> **1.5× over the theoretical bar and 3× over the practical one.** The
> finding dies if a per-directory listing (§1.3) shows the 107 GB is
> three or more passes rather than one, or if ~540 MB is the largest
> directory rather than a typical one.

A 3× margin against two readings of one unverified breakdown is strong
enough to plan from and not strong enough to ship a default from. §6
keeps those two apart.

The other way to satisfy it is on the consumer's side and not ours: a
seven-day floor fits a 200 GB budget if a pass leaves 200 ÷ 7 = 28.6 GB,
i.e. about 134 MB a group instead of 540 MB `[D]` — a four-fold
reduction in what the nightly ships. That is their decision about their
test suite, relayed as a figure, never as an instruction (D161).

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
* **What that walk costs.** D152 measured it on this host at 100 000
  directories: 1.15–1.96 s for the sweep's walk and 0.87–1.31 s for the
  record-reading profile. Scaled linearly — *derived, not measured* — the
  walk is **40–65 ms at 3334 directories** and **0.4–0.65 s at ten times
  that** `[D]`. A hundred walks a night is 4–6.5 s today and 40–65 s at
  ten times, in bursts, against a hub whose host already has IO as its
  sore point (D136).
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

---

## 3. Half two — sizing M12's record retention

Nothing in the tree deletes a record today, ever `[C13, C18]`. 3334
records is small; unbounded is unbounded, and §2 showed that every walk
in the hub is O(all jobs ever submitted) because of it. Retention is not
only about disk: it is the only thing that can ever make those walks
shrink.

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

### 3.3 Where the two mechanisms meet, and which wins

Two mechanisms over one directory tree: the output budget evicts **bytes
under pressure**, retention expires **records on time**. In one sentence:

> **Retention is about records and outranks the budget, which is about
> bytes. The budget may never remove a record; retention always removes
> the bytes in the same act.** `[P]`

They can contradict each other in exactly one configuration: a record
past its retention whose bytes are still inside the output floor. With
retention ≥ 14 days and a 7-day floor it cannot arise; with
`--output-floor 30d --record-retention 14d` it does, and then one
mechanism says keep and the other says go. **Refuse that configuration at
startup, naming both figures** `[P]` — the style `herd-hub/src/main.rs`
already uses, where a budget that does not parse stops the hub before any
byte has moved `[C11]` and *"a timing that cannot mean what it says is
refused here, before the port is bound"*.

The hazard that makes the ordering non-negotiable: a `files/` whose
`job.json` is gone is permanently `could_not_free` `[C16]`, so the
running total drifts up, never comes down, and the sweeper can never
reach the budget again. Therefore **bytes first, record last, in one
act**: the reverse order leaves exactly that orphan if the hub dies in
the middle.

Two more things they must share rather than duplicate `[P]`:

* **One writer, one guard.** The sweeper has a one-at-a-time flag; the
  retention pass takes the **same** one, not a second. D8's one-writer
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
records on this host. Scaled linearly, *derived and not measured*: **30–
45 ms at 3334 records**, **0.3–0.45 s at 33 340** `[D]`.

And there is a **sixth** behind it, which nothing has named: `usable_index`
trusts the index only while `index.jobs_accounted` equals the number of
job directories `[C15]`. A pass that removes directories without updating
that count forces a **full index rebuild — every record read — at the
next startup**. Retention must update the index in the same act, or the
trust check must stop being a raw count.

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
At 30–45 ms a day that is free, and even at 0.45 s it is half of one of
ORCH-021's 200 ms polls.

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
| H6 | D152's walk timings scale linearly from 100 000 directories down to 3334 and up to 33 340 (§2.5, §3.4) | A measurement at the real directory count. D152 itself calls its figure *"one host, one filesystem, warm cache"* | **Yes**, and nobody has done it: a fixture store is cheap to build |
| H7 | "In-floor eviction every night" assumes the nightly returns at this size with the budget at 200 GB | The nightly is off `[R7]`, so nothing will show until it returns | No, and this is why the warning of §2.7 matters more than the prediction |
| H8 | `statvfs` on the state root measures the filesystem Herd shares (§2.6) | True here: the hub volume is a directory on the same ext4 `[R5]`. A separate mount, an overlay or a quota would measure something else — and the guard would then be measuring the right thing for *that* deployment anyway | Yes, by test |
| H9 | A `herd_herd-state` docker volume maps to a plain directory under `/mnt/ssd_980/docker/volumes` | `[R5]` says so. If it were a separate device the whole of §2.6 still holds, with different numbers | No |

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

---

## 6. Decisions for Alexa

One at a time, each with a recommendation and a fallback. None of these
is the orchestrator's to take: each changes *what* is built or is
outward-facing.

**A. The seven-day floor cannot be kept at the first consumer's workload
on their disk (§2.4).** Do we (i) keep the shipped default at seven days
and make the hub say what it can actually keep, (ii) lower the shipped
default, or (iii) tell the consumer to set about two days?
**Recommendation: (i) and (iii) together** — the default stays
project-agnostic, the hub stops promising what it has been shown it
cannot keep, and the consumer gets a concrete figure for their own
configuration. **Fallback:** (ii) at three days, which is honest on more
disks and gratuitously short on small fleets.

**B. A second disk knob, `--volume-free-floor` (§2.6).** This is
invariant 1 territory: is a second number the beginning of a policy
language? **Recommendation: yes, add it.** One number bounds what Herd
*takes*, the other what Herd *leaves*; a shared volume needs both, and
R5 says this volume is shared. It is a bound, not a policy: no job can
be named in it and no job is more important than another under it.
**Fallback:** no new flag — derive the reserve from the budget alone and
accept that a neighbour can fill the disk under a hub that is inside its
budget and sweeping nothing.

**C. The retention class map: merger 90 days, every other level 14
(§3.1).** **Recommendation: as written.** **Fallback:** nightly joins
merger at 90, which on these figures is nearly the whole volume and
makes retention close to a no-op.

**D. One read, on the consumer's side, that would settle H1, H2 and H3
(§1.3).** A per-directory size listing of the hub volume's `jobs/`. It
needs root on their host; the consumer declined to work around their
permissions and was right to; **nobody in this project runs it** (D161).
This is a relay, not a decision: Alexa asks or does not.

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
| ORCH-101 | The startup line stops promising a record *"for ever"* (`main.rs:288-297`); `docs/plan.md` M12 and `docs/coverage.md` E-15 point at `docs/disk-policy.md` | `docs/disk-policy.md` §3.2 | herd-step | owed | Prose plus one `println!`; must land with ORCH-098, never before | ORCH-098 | `herd-hub/src/main.rs:288-297` |
| ORCH-102 | The server hands the retention pass a set of protected ids — jobs being followed, fetches in flight — so the store stays ignorant of sessions | `docs/disk-policy.md` §3.5 | herd-step | owed | One task at the server/store seam | ORCH-098 | `docs/disk-policy.md` §3.5 |
| ORCH-103 | Relay to Alexa: ask the consumer for one per-directory size listing of the hub volume's `jobs/`, with dates. It settles H1, H2 and H3; it needs root on their host; nobody in this project runs it (D161) | `docs/disk-policy.md` §1.3, §4 | Alexa | waiting | Ask, one question, with the two commands of §1.3 | Alexa | `docs/disk-policy.md` §1.3 |
| ORCH-104 | Measure the sweep walk at a realistic directory count (3334 and 33 340), refuting or confirming H6's linear scaling of D152's 100 000-directory figures | `docs/disk-policy.md` §4 H6 | herd-step | open | A fixture store and a timing, no product change; the only hypothesis here we can settle ourselves | — | D152's measurement block |

### File ownership and order

Four of these rows touch `herd-hub/src/store.rs`, so they cannot share a
worktree or run at once. The order below is the dependency order and also
the conflict order:

1. **ORCH-094** — `store.rs` (`recover`, `StoreFile`), `main.rs` none.
   Must land before anything can remove a record.
2. **ORCH-095** — `main.rs` (the arguments and the startup line),
   `job.rs` (`Budget`, the defaults), `store.rs` (`held_more`, the
   ceiling). Blocked on decision §6 B.
3. **ORCH-096** — `store.rs` only (`Live`, `say_what_the_sweep_did`).
   After ORCH-095, because it warns against the effective ceiling.
4. **ORCH-098** — `store.rs`, heavily, plus `job.rs` and `main.rs` for
   the two flags. The largest task here; it should not be cut smaller,
   because the pass, the guard, the index update and the removal order
   are one correctness argument.
5. **ORCH-099, ORCH-100, ORCH-101, ORCH-102** — each small, each after
   ORCH-098, and ORCH-101 must never land before it.
6. **ORCH-097** and **ORCH-104** are independent of the rest:
   ORCH-104 touches no product file at all and can run at any time,
   including first.
