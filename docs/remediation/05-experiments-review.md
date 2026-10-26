# 05 — Adversarial review of the two experiments

**Date:** 2026-10-25.
**Base:** `origin/main` at `0a2cf73`, the same base both experiments read.
**Under review:** `04a-cost-surface.md` (`experiment-cost-surface`, `da44414`)
and `04b-integrity-and-containment.md`
(`experiment-integrity-containment`, `47efaa0`, `14ac19d`).
**Author:** `herd-reviewer`. I wrote neither experiment and neither brief.

**What this piece is.** Unlike `03a`, `03b`, `03c` and `00-baseline.md`, this
one **ran things**. Every figure below was produced on the shared host, in a
worktree of its own, under `nix develop`, with `-j 4` and `/proc/loadavg`
read before each phase. The one-minute load peaked at 10.1 during
`./scripts/test.sh` — the suite sets its own parallelism — and stayed under 5
everywhere else; the five-minute load never passed 5.

**Evidence standard**, as in the rest of the dossier: **F** measured here or
read at a named `file:line` on `0a2cf73`; **H** a hypothesis with what would
settle it; **I** unknown.

**What I added.** One file, `herd-hub/tests/review_pass_cost.rs`: six
`#[ignore]`d `measure`-style cases. No product file was touched —
`herd-hub/src`, `herd-runner/src`, `herd-proto/src` and `herdctl/src` are
byte-identical to `0a2cf73`. `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings` and `./scripts/test.sh` are green.

---

## 0. The findings, ranked

1. **04a's junction figure measures one half of a junction, and the other
   half is an order of magnitude larger.** F. A pass ends by writing a
   durable record for every job it leaves queued whose cause has changed
   (`server.rs:2619-2640` → `say_it_waits` `:3415` → `store.record_waiting`
   `store.rs:2007`, a record rewrite plus an event append, four `fsync`s).
   Measured on this host: **8.3 s and 9.2 s** for 1 000 such writes, against
   **13 ms** for the two reads at the same `N` and **195–203 ms** for the
   whole `T` = 20 000 walk. 04a's "`T` costs 25x what `N` costs" compares the
   `T` walk with the `N` *reads* only. Counting the writes, the order
   reverses by about an order of magnitude. §3.
2. **04a's recommendation 1, as written, cannot be implemented.** F. "A
   `collections_owed` that reads only the open jobs the hub already holds"
   returns **0 of 1** owed collections, measured; an owed collection is by
   construction on a **finished** job, which `unfinished_jobs` never
   contains. The repository says so itself, in a test comment 2 200 lines
   below the function: *"so an outstanding collection is never in
   `unfinished_jobs` and a scan that only looked there would wait for bytes
   nobody was ever going to be asked for"* (`herd-hub/src/store.rs:4624-4629`).
   The index variant of the same recommendation is viable and is **not**
   "small and local": it is the in-memory list the code comment at
   `store.rs:2369-2372` deliberately refused. §4.
3. **04a's recommendation 4 is refuted by measurement.** F. The amplifier is
   **not** the `state.dispatching` mutex. Four concurrent `collections_owed`
   calls, on plain threads, with no hub, no reactor and no `dispatching`
   lock anywhere, cost **4.59x** one call (868 ms against 189 ms at
   `T` = 20 000). `collections_owed` runs at `server.rs:4263`, reached from
   `server.rs:2415` — **four lines before** the lock is taken at
   `server.rs:2493`. A pass that is skipped rather than queued would not
   remove this. The amplifier is the store's own single `Mutex<Live>`, taken
   once per record read inside `JobStore::job` (`store.rs:2255-2258`). §4.3.
4. **04b's central claim is correct, and my retraction-check confirms it.**
   F. `ran_commit` as a `#[serde(default)]` field on `StepRun` needs no
   protocol break and no record break. Every precondition holds on
   `0a2cf73` and I checked each one. **One cell of 04b's own matrix is
   optimistic:** an older hub *reads* the field and then **erases it on its
   next write of that record** — measured. "A rollback works too" should
   read "a rollback reads; it does not preserve". §5.
5. **The loud-skip count is 12 on this host today, and `coverage.md` is
   right.** F, from a real `./scripts/test.sh`: 12 lines in the skip log,
   12 distinct assertions, no merging. The split is **10** cgroup-v2
   delegation and **2** the ZFS ceiling — but the two ceiling ones are not
   the two `04a`'s handover names. D151's 13 is not reproducible here and I
   cannot say why. §6.
6. **04a's "`W` = 4 is worse than `W` = 2" is an `F` the table does not
   support.** Every cell compared holds `callers = W`, so two variables move
   together. For a fixed number of redialling runners, more workers is not
   worse. §2.3.
7. **One genuinely one-line O(`T`) walk that 04a did not name.**
   `server.rs:4384` answers *"are this one job's files still coming?"* with
   `collections_owed()` — a whole-volume walk — and then `.find()`s one
   entry, while `collection_owed(id)` sits right there
   (`store.rs:2389`). Per requester outputs query. F. §8.

Everything else in both documents that I checked, I confirm. §7 lists it.

---

## 1. The replay

`cargo test -p herd-hub --test cost_surface --release -- --ignored
--nocapture --test-threads=1`, one run, 225 s, on the same host 04a used,
with another project idle beside it.

**Case 1** — reproduces. Read syscalls and bytes are identical to the digit
at every size, which is the part that cannot drift; wall clocks are 0–20 %
lower than 04a's, in the direction of a quieter host.

| N | T | `unfinished_jobs` 04a → mine | `collections_owed` 04a → mine |
|---:|---:|---|---|
| 50 | 0 | 853 µs → 439 µs | 837 µs → 440 µs |
| 200 | 0 | 2.04 ms → 1.83 ms | 2.56 ms → 1.87 ms |
| 400 | 0 | 3.54 ms → 3.53 ms | 3.58 ms → 3.89 ms |
| 1000 | 0 | 11.40 ms → 8.92 ms | 9.81 ms → 8.94 ms |
| 50 | 1 000 | 432 µs → 456 µs | 10.08 ms → 9.74 ms |
| 50 | 5 000 | 428 µs → 427 µs | 48.76 ms → 48.27 ms |
| 50 | 20 000 | 794 µs → 433 µs | 247.16 ms → 203.14 ms |
| 1000 | 20 000 | 10.05 ms → 9.13 ms | 220.04 ms → 203.59 ms |

**Case 2** — reproduces, including the shape. The two decisive cells:
(`N` = 1000, `T` = 0, `W` = 4, callers = 4) 33.22 ms → **27.07 ms**;
(`N` = 50, `T` = 20 000, `W` = 4, callers = 4) 814.68 ms → **683.25 ms**.
The ratio 04a's conclusion rests on is 24.5x in 04a and **25.2x** here. The
single-caller cells are noisier on my run than on 04a's (one read 23 ms
where 04a read 1.66 ms), which does not change any conclusion.

**Case 3** — reproduces almost exactly: 8.024 s against 04a's 8.03 s at
`d` = 20 ms, callers = 4.

**Case 4** — reproduces exactly: `collections_owed` x1, `unfinished_jobs` x1,
**5 103** record accesses at one junction, 0 `collections_owed` over five
idle seconds, 6 watchdog reads. §3 is about what this count leaves out.

**Verdict on the replay: 04a's figures are honest and its bench is sound.**
Nothing below is a charge of carelessness in measuring. What I dispute is
what was measured and what was concluded from it.

---

## 2. Question 1 — "the dominant term is `T`", and the `d` convention

### 2.1 The convention does not decide the result

04a charges `d` per **record access** rather than per trait call, says so,
and says the choice is load-bearing. The orchestrator's brief says the
choice "partly decides the result". **It does not.** F, and the reason is
structural rather than arguable:

* The 815 ms / 33 ms comparison the conclusion rests on is **case 2, `d` = 0**
  (`cost_surface.rs`, case 2's header, and the replay above). At `d` = 0 the
  wrapper's sleep is skipped entirely (`charge` returns early when
  `per_access.is_zero()`), so the figures are the real `JobDirStore`'s and
  carry no convention at all.
* Under the per-call convention the surface becomes
  `c x (2N + T + 1) + 2d` instead of `(c + d) x (2N + T + 1)`. The `T` term
  is carried by `c`, which is measured, not by `d`. So `T` still beats `N`
  by the same factor, and `d` simply stops being able to compete.

So the conclusion **survives the other convention**, and survives it more
easily. What the convention does decide is 04a's *third* recommendation —
"do not size the fix off `d`", with its 20 s-per-junction figure. Under the
per-call convention that warning evaporates. The per-record choice is the
conservative one for a naive port of this store (`collections_owed` really
does one read per directory, so a remote backend really would pay `N + T`
round trips) and the wrong one for a rewritten store that can ask one
question. 04a marks exactly this as **H** and names what would settle it.
I have nothing to add except that the **H** is doing more work than the
document's emphasis suggests: recommendation 3 is entirely inside it.

### 2.2 A smaller note on the `d` axis's mechanism

**F.** `CostlyStore::charge` sleeps **before** delegating, and outside the
inner store's lock (`cost_surface.rs:103-114` and `:203-207`). So case 3's
four concurrent sleeps genuinely overlap, which is why 04a's arithmetic —
4.02 s in parallel plus 4.00 s serial = 8.02 s — lands to 0.3 %. That is a
property of the wrapper, not of the store. At `d` = 0 the same four callers
do **not** overlap (§4.3), and the document does not notice that its case 3
mechanism and its case 2 mechanism are different mechanisms.

### 2.3 "`W` = 4 is worse than `W` = 2" is not supported by its own table

**F.** Every cell 04a compares holds `callers = W`. 814 ms at `W` = 4 is
four callers; 474 ms at `W` = 2 is two. The table has no
(`W` = 2, callers = 4) cell, so the claim that *workers* make it worse is
read off a change in *callers*. For the quantity that matters — a fixed
number of runners redialling at once — more workers cannot be worse: a
caller beyond `W` blocks either way, and the time to the first free worker
is set by how long one caller takes.

This matters because the sentence is repeated in 04a's handover as a
finding, and a reader could take it as an argument against raising the
hub's worker count.

---

## 3. Question 2 — the hot path, and what the indirect measurement cost

The orchestrator asks whether its own ban on touching `herd-hub/src` made
the measurement inconclusive. **It did not make it inconclusive; it made it
measure the wrong half, and the ban is not the reason.** The missing half is
reachable through the public `JobStore` trait, with no production code and
no private function.

### 3.1 What a pass does after the two reads

`dispatch_a_pass` (`server.rs:2509`) reads, then orders, then walks the
queue. For each job it leaves queued it calls `say_it_waits`
(`server.rs:2619-2640`), which since D171 writes **only when the cause
changed** (`server.rs:3435`) — and when it does write, it calls
`store.record_waiting` (`store.rs:2007-2023`): `write_record` (a durable
replace: temporary, `fsync`, rename, `fsync` the directory —
`durable.rs:6-7,46,206`) **plus** `append_event`.

04a's "what was not measured" names `queue::order`, `matcher::holdings`,
`matcher::reserved` and `matcher::reservations` — the arithmetic — and
concludes *"it would make `N` larger, and `N` is already 25x behind"*. The
arithmetic is indeed small. **The writes are not**, and they are not in the
list.

### 3.2 Measured

`herd-hub/tests/review_pass_cost.rs::the_write_half_of_a_pass`, release, two
runs:

| N | `unfinished_jobs` | `N` x `record_waiting` | per record |
|---:|---:|---:|---:|
| 50 | 0.60 / 1.16 ms | 0.77 s / 1.11 s | 15.4 / 22.3 ms |
| 200 | 2.24 / 2.62 ms | 2.00 s / 2.13 s | 10.0 / 10.7 ms |
| 1000 | 13.25 / 12.82 ms | **8.31 s / 9.24 s** | 8.3 / 9.2 ms |

And D171's half, for completeness: a second pass over the same 1 000 jobs
with **no** cause changed costs **11.7 ms / 12.2 ms** — the two reads and
nothing else. D171 is real and it is large.

### 3.3 What that does to the ratio

Put beside 04a's own two maxima, on the same host, release:

| term | 04a's figure | with the write half |
|---|---:|---:|
| `N` = 1 000, `T` = 0, one pass, cause changed | 33 ms | **~8.3 s** |
| `T` = 20 000, `N` = 50, four concurrent junctions | 815 ms | 815 ms |

`T` does not dominate `N`. **`N` dominates `T` by roughly an order of
magnitude**, on a pass whose causes changed — and a runner junction is
exactly such a pass, which is not a hypothesis: 04a's own case 4 log prints
**50 of 50** `stays queued` lines immediately after
`herd-hub: runner cost-surface-runner has joined`, and that line is printed
only when `record_waiting` returned `true`.

### 3.4 And it shows in case 4's count too

**F.** Case 4 reads its counters the moment `handle.runner_connected()`
turns true (`cost_surface.rs:802-813`). In the replay log, the 50
`stays queued` lines are printed **after** the `one junction:` line. So the
window closed before the pass's writes, and the 50 `record_waiting` charges
— which `CostlyStore` does count (`cost_surface.rs:157-160`) — are not in
the 5 103. The arithmetic confirms it: 5 051 + 50 + 2 = 5 103, with no room
for 50 more.

So **"99 % of a junction's disk work is `T`"** is 99 % of the *record
accesses the wrapper counted before the writes started*, in a unit that
prices an 11 µs read and a four-`fsync` record rewrite the same. In wall
clock at `N` = 50, `T` = 5 000, the `T` walk is ~48 ms and the 50 writes are
0.8–1.1 s.

### 3.5 On a live hub, end to end

`review_pass_cost.rs::one_whole_junction_timed`: a real hub, a real
`herd-runner` dialling in, `N` = 50 jobs nothing can run, timed from the dial
to the moment the last queued job carries a reason **naming the new runner**.
Three rounds per `T`:

| T | rounds | median |
|---:|---|---:|
| 0 | 665 ms, 721 ms, 414 ms | 665 ms |
| 5 000 | 1.03 s, 298 ms, 491 ms | 491 ms |
| 20 000 | 417 ms, 1.96 s, 4.02 s | 1.96 s |

**I** — this is too noisy to split into terms, and I am not going to pretend
otherwise. What it does establish, and it is enough: **a junction at `N` = 50
already costs hundreds of milliseconds to seconds of hub time on this host
before `T` is large enough to matter**, and the spread is set by durable
writes on ZFS, not by the directory walk. 04a's headline 815 ms sits inside
the run-to-run noise of a 50-job queue.

### 3.6 Was the orchestrator's ban the cause?

**No.** F. `record_waiting` is a `JobStore` trait method and is public;
measuring it needs no access to `dispatch_a_pass` and no change to
`herd-hub/src`. `CostlyStore` already wraps it and already counts it. What
was needed was to look at the write column, and to close case 4's window
after the pass rather than before it. The ban on touching `herd-hub/src` is
sound and I would keep it; it did not cause this.

What the ban *did* cost is precision, not the conclusion: case 4's window
has to be guessed at from outside, and 04a guessed it at
`runner_connected()`, which is one event too early. A single public hook —
or simply polling a counter until it stops moving — closes it.

---

## 4. Question 3 — "the urgent fix is not architectural"

### 4.1 The open-jobs variant does not work

**F, measured** (`review_pass_cost.rs::a_collection_owed_is_not_an_open_job`):
a job that has sent its manifest and whose bytes have not arrived gives

```
collections_owed -> 1 entry
unfinished_jobs  -> 0 entries
the owed job is in unfinished_jobs: false
=> a collections_owed restricted to the open jobs would return 0 of 1
```

**F, by reading.** The hub writes the verdict and the manifest and *then*
moves the job to `Finished` (`server.rs:4877-4881`), `Finished` is terminal,
and a terminal job is dropped from `live.pending` (`store.rs:1356`), which
is the only thing `unfinished_jobs` walks (`store.rs:2312-2316`). D148 is
built that way on purpose: the verdict goes first and the files follow.

**F, in the repository's own words**, `herd-hub/src/store.rs:4624-4629`:

> **A finished job**, and that is the point: the verdict goes first and the
> files follow (D148), so an outstanding collection is never in
> `unfinished_jobs` and a scan that only looked there would wait for bytes
> nobody was ever going to be asked for.

Taking the recommendation literally would silently stop D148 resumption: no
`JobFilesHeld` on a redial, ever, and a consumer's outputs would simply
never arrive after a dropped link. It would pass every test that does not
cross a reconnection.

### 4.2 The index variant works, and is not cheap in the way the word "local" suggests

The other half of 04a's sentence — "an index of the jobs that actually owe
bytes" — is sound, and it is the exact thing `collections_owed`'s own
comment refuses:

> the alternative is a list of outstanding transfers kept in memory, which
> is the thing that can disagree with the disk (`store.rs:2369-2372`).

It can be made safe (rebuild it in `JobDirStore::open`, which already scans,
`store.rs:439`), but that is a new piece of store state with a new
invariant, in the module whose whole discipline is "derive it from the
records, so there is nothing to leak and nothing to rebuild" (D79, quoted
four times in `server.rs`). It is a storage decision, and `CLAUDE.md` puts
storage decisions in front of Alexa.

**What the cheap variants actually buy.** Measured
(`review_pass_cost.rs::what_a_bare_directory_walk_costs`):

| T | `collections_owed` today | `read_dir` + one `stat` per entry | `read_dir` alone |
|---:|---:|---:|---:|
| 1 000 | 9.27 ms | 1.07 ms | 0.20 ms |
| 5 000 | 46.96 ms | 5.81 ms | 1.02 ms |
| 20 000 | **194.77 ms** | **24.90 ms** | 4.23 ms |

So a filter that keeps the scan a scan buys **8x**, not "to zero". Only the
index buys zero. 04a's "815 ms becomes ~33 ms, measured" is the index
variant's number, attached to a sentence that offers the open-jobs variant
first.

### 4.3 The `dispatching` mutex is not the amplifier

04a's fourth recommendation — a pass **skipped** when one is running rather
than queued — rests on an **F** that is wrong.

**Measured** (`review_pass_cost.rs::where_the_amplification_comes_from`):
`C` concurrent `collections_owed` calls on plain `std::thread`s, no tokio,
no hub, no `dispatching` lock in the process at all, `T` = 20 000:

| callers | slowest caller | x one call |
|---:|---:|---:|
| 1 | 188.97 ms | 1.00 |
| 2 | 410.10 ms | 2.17 |
| 4 | **868.02 ms** | **4.59** |

868 ms with no lock, against 04a's 815 ms with the whole hub. **The entire
amplification is present without the mutex 04a blames.** And it must be:
`collections_owed` is called from `ask_for_the_files_still_owed`
(`server.rs:4263`, reached at `server.rs:2415`), four lines *before*
`dispatch_queued_jobs` takes the lock at `server.rs:2493`.

The amplifier is the store's own single `Mutex<Live>`: `JobStore::job` takes
it for every record (`store.rs:2255-2258`), so a `T` = 20 000 walk takes and
releases it 20 001 times and two walks interleave rather than overlap; and
`unfinished_jobs` holds it across its whole `N`-record read
(`store.rs:2312-2316`), so while it runs **no writer in the hub can make
progress** — not a log append, not a verdict.

**Consequence for the architecture question.** A refactor that moves these
calls onto a blocking pool, or a pass thread fed by a channel, inherits this
lock unchanged and inherits the convoy with it. Whatever is decided about
the reactor, the store's lock granularity is part of the same decision.

**Second consequence, on the proposal itself.** "Skip a pass when one is
running" is also not safe as stated. A caller that skips has, by definition,
seen an event the running pass read the queue before — a runner that just
joined, a job that was just submitted. Dropping it leaves that job queued
until something unrelated wakes the hub, which is the shape of the stall the
dossier is about. The correct form is *coalesce*: skip, set a flag, and make
the running pass run again if the flag is set. That is a different change,
and it should be written down as the different change it is.

### 4.4 So: does recommendation 1 hold?

Partly, and not for the reason given.

* **It holds that the whole-volume walk should stop.** 195–203 ms per
  junction, per redial, growing for ever, is a real and unbounded defect.
  F.
* **It does not hold that this removes the urgency.** The term it removes is
  not the largest one (§3), the amplification it claims to explain is
  elsewhere (§4.3), and the structure that produced it — a synchronous
  store, behind one global mutex, called from the reactor, `fsync`ing once
  per queued job — is untouched and can produce the next one. The honest
  reading of `T`'s growth is that it is **one instance of a general shape**,
  not an isolated accident: nothing bounds what a single reactor-blocking
  store call may cost, and that is precisely the audit's premise.

---

## 5. Question 4 — `ran_commit` needs no protocol break

I checked every link of 04b's chain myself. **04b is right and the
orchestrator's retraction was not premature.** F on each of the following,
on `0a2cf73`:

* `PROTOCOL_VERSION = 2` (`herd-proto/src/lib.rs:113`),
  `RECORD_VERSION = 1` (`herd-hub/src/job.rs:52`).
* The written additive rule for the wire: *"a new field on an existing
  variant may leave it alone when it is `#[serde(default)]` and means,
  absent, exactly what its default says"* (`docs/decisions.md:7293`).
* The written additive rule for the record: *"Only a field that is
  `#[serde(default)]` and means, when absent, exactly what its default says
  is additive enough to leave the number alone"* (`herd-hub/src/job.rs:49-51`).
* `deny_unknown_fields` appears **nowhere** in `herd-proto` or `herd-hub`.
  Its five occurrences are `herdctl/src/declaration.rs:204,527,540,552` and
  `herd-runner/src/capability.rs:389,405,442,461` — declaration files and
  probe files, not frames and not records. I grepped the whole workspace.
* Both frame enums carry `#[serde(other)]`
  (`herd-proto/src/lib.rs:1600`, `:2138`).
* `StepRun` already carries two `#[serde(default, skip_serializing_if)]`
  options, `exit_code` and `ended` (`herd-proto/src/declaration.rs:265-275`).
  The precedent is in the very struct the field would join.
* **One thing 04b does not say and should.** The record does not have its
  own step type: `herd-hub/src/job.rs:268` holds `Vec<StepRun>`, the
  `herd_proto` one. So the wire change and the record change are the *same*
  change and cannot be staged apart. This strengthens 04b's case — both
  additive rules are satisfied by one `#[serde(default)]` — but it is a fact
  a planner needs.

**Measured, forwards** (`review_pass_cost.rs::an_unknown_field_survives_a_read_and_not_a_write`):

```
StepRun with an unknown `ran_commit`: decoded, step = Fetch
FromHub with an unknown field: decoded as SetSwitch { switch: On, why: "a probe" }
old build read a record carrying ran_commit: ok, 1 step(s)
  the planted field is still on the disk: true
```

**Measured, backwards — and this is where 04b is optimistic:**

```
  after one write by the old build, ran_commit is still there: false
```

An older hub reads a record a newer hub wrote, keeps running, and then
**erases the field the first time it writes that record back**. Serde drops
unknown fields on deserialisation and has nothing to re-emit. The reachable
paths are ordinary: `record_state` on any job still in flight, and
`note_outputs_fetched` (`store.rs:2331-2350`), which rewrites a **finished**
job's whole record the first time a requester fetches its outputs.

04b §3.5 says *"Neither number moves, so a rollback works too — which is the
half of I12 that a bump would break"*. The first half is true and is the
important half. The sentence should be narrowed to: **a rollback reads every
record; it does not preserve the new field on the records it touches.** For
a diagnostic field recording "declared X, ran Y" that is an acceptable cost
and should be written down, not discovered.

**One correction of labelling.** 04b's second matrix in §3.5 is headed "old
`herdctl` / new record". `herdctl` never reads a record: `grep -rn
"job.json\|JobRecord" herdctl/src/` finds nothing. What an old `herdctl`
sees is a `FromHub` frame, and the tolerance there is the `#[serde(other)]`
and the absent `deny_unknown_fields` — which is the same answer, reached by
a different route. The cell is right; the artefact it names does not exist.

**On the rest of 04b.** I did not re-run its two benches. I did take every
`file:line` it cites back to the code, and they are all exact:
`store.rs:1711-1716` (the `continue`) against `:1726` (the push the
`continue` skips) and `:1717-1724` (the rule it contradicts, in bold, four
lines below); `herdctl/src/main.rs:1283` (`missing` counted off the
manifest) and `:591-596` (`code_for`); `herd-runner/src/lib.rs:1151-1165`
(`take_job` refuses for `draining` and the switch and nothing else) and
`:2745` (the unconditional `tokio::spawn`). 04b's reading of the mechanism
is correct line by line.

---

## 6. Question 5 — the loud skips, counted here

`./scripts/test.sh` on this host, this tree (`0a2cf73` plus my one
`#[ignore]`d test file), `nix develop`, full workspace suite, green.

**F. The count is 12.** Not 12 reported from 13 lines: the raw
`$HERD_SKIP_LOG` holds **12 physical lines**, `sort -u` leaves **12**, and
the 12 lines carry **12 distinct test names**. I checked all three, which is
the check neither `scripts/test.sh` nor `scripts/report-skips.sh` makes.
D151's defect is genuinely gone — `herd-runner/src/testing.rs:38-46` puts
the whole line in one `write_all` and says why.

**F. The split is 10 / 2**, by the first clause of each reason:

*Ten — "cgroup v2 delegation is unavailable: this runner cannot create a
cgroup under `/sys/fs/cgroup/user.slice/user-1002.slice/session-128.scope`":*
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.

*Two — the host cannot carry a hard IO ceiling at all, because the data root
is on ZFS:* `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
and `a_declared_ceiling_changes_what_the_inert_weight_line_says`, both in
`herd-runner/src/confine.rs` (`:1118`, `:1220`).

**The split is the one the orchestrator's briefs carried, and the two names
in 04a's handover are not both right.** That handover names
`an_io_weight_that_cannot_bite_is_said_out_loud` as one of the two ZFS
skips; its reason begins *"cgroup v2 delegation is unavailable"* and it
would still skip on a delegated host with a block device under it, for a
different reason. The second ZFS one is
`a_declared_ceiling_changes_what_the_inert_weight_line_says`. That is a
detail, and it is the kind of detail that shows the number was carried
rather than read.

**Reconciling 12 and 13.**

* `docs/status.md:47` and `docs/coverage.md:113` say **12**, and on this
  host today they are **right**. `coverage.md`'s further claim — *"every one
  of the twelve is a cgroup-delegation or ZFS `io.max` assertion"* — is also
  right. F.
* D151 (2026-10-16) says *"the gate run for this branch reports 13"*. I
  cannot reproduce it and I will not guess at it in an **F**. What I can
  rule out: **no skip site was deleted between the two.** `skipped(`
  appears 51 times at `5430731` (the D151 fix), 51 at `9d3d101`, and **52**
  at `0a2cf73`, and `git diff 5430731 0a2cf73 -- '*.rs' | grep '^-.*skipped('`
  is empty. So the tree did not lose an assertion. **I** on the cause. The
  most likely remaining explanations are a different host condition in that
  session (the reason strings name the session's own cgroup scope and the
  run's own `TMPDIR`, both of which change which assertion fires), or a
  double-counted line — see below.
* **The history, F.** The repository said **7**, then **13** on 2026-10-16
  (`5430731`), then dropped the sentence on 2026-10-19 (`9d3d101`), then
  wrote **12** fresh on 2026-10-23 (`ff3a525`, the "rewrite the status and
  the coverage state for an operator" commit). So 12 is not a survival of
  the pre-fix number; it is a figure written after 13 and, as it happens,
  the right one here.

**And a live defect of the same family, which is why this matters.** Both
counting scripts dedupe on the **whole line**, and two of the twelve reasons
embed a run-specific path containing a PID — for example
`/tmp/nix-shell.dmS1bM/herd-test-confine-io-max-2767855-11`. Two emissions
of one assertion then produce two different lines and the banner reports one
assertion too **many**. It does not bite today (each of the twelve is in
exactly one test binary; I checked), so **H** on the trigger and **F** on
the mechanism. D151 fixed an under-count; the same unit of counting can
over-count. The fix is one `sed`: dedupe on the text up to the second colon,
which is the test name.

---

## 7. What I checked and confirm

**In 04a, unchanged:** every `file:line` (`server.rs:2415`, `:2459`,
`:2492`, `:2509`, `store.rs:2312`, `:2367`, `:1142/:1150/:1160`, `:1981`,
`:642`, `:439` — all exact); that `unfinished_jobs` is flat in `T` and
`collections_owed` linear in `N + T`; that two read syscalls and ~499 bytes
go per record at every size; that the text is parsed three times; that
**nothing ever removes a job directory** (the only `remove_dir_all` on a job
path is the part directory at `:1981`; `remove_tree` at `:2717` is the
sweeper on `files/`), so `T` is monotone; that the watchdog reads all of `N`
once a second unconditionally; that `collections_owed` is never called while
the hub is idle; that debug is ~6x release and the hub ships release.

**In 04b, unchanged:** §1 end to end (the `continue` that drops the manifest
entry, the rule it contradicts four lines below, `missing` counted off the
manifest, `code_for`, and that `outputs.skipped` reaches stderr and the
record but never `Verdict::said`); §2's reading of `take_job` and the
unconditional spawn; §3.1's gap (no frame from a runner carries a revision)
and §3.3's rejection of options 4, 5 and 6 on invariant 1; the honesty of
leaving the cgroup count as **I** and of refusing to run a real-job flood on
this host.

**One judgement I share with 04b and want on the record:** its §4.1 stopgap
— putting the `skipped` sentences into `Verdict::said` — is one line, no
protocol and no record cost, and it is the only thing in either document
that gives the first consumer something machine-readable to act on before a
real fix. It should not wait for the architecture decision.

---

## 8. The ordered list

### Proved here, and ready to be decided

1. **Replace `collections_owed().find(...)` with `collection_owed(job_id)`
   at `server.rs:4384`.** One line. Removes a whole-volume walk from every
   requester outputs query. The single-job method is already there
   (`store.rs:2389`) and is already what `collections_owed` calls per entry.
   No design question, no protocol, no record.
2. **Put the `skipped` sentences into `Verdict::said`** (04b §4.1). One
   line, no protocol, no record. Turns an invisible dropped output into
   something a wrapper can read.
3. **Stop `collections_owed` reading every record.** The walk is 195–203 ms
   at `T` = 20 000, per junction, per redial, growing for ever. **Not** via
   the open jobs — that returns nothing (§4.1). Either an index rebuilt at
   `JobDirStore::open` (buys all of it, and is a storage decision for
   Alexa), or a cheap per-entry test (buys 8x, measured, and is local).
   Recommend the index, with the open-jobs variant struck from the record.
4. **Bound the write half of a pass** (§3). `N` durable record rewrites on
   the reactor, 8.3–9.2 s at `N` = 1 000 on this host, triggered by exactly
   the event a junction is. This is the largest measured term in the
   dossier and no document names it. It needs a decision, because
   `record_waiting` plus `events.jsonl` is an E-04 contract and not an
   implementation detail: batch the writes, defer them off the pass, or
   make the queued-wait reason a derived fact rather than a stored one.
5. **Fix the amplifier that is actually there** (§4.3): the store's single
   `Mutex<Live>`, taken per record in `JobStore::job` and held across the
   whole of `unfinished_jobs`. Any "persistence off the reactor" design that
   does not address it inherits the convoy.
6. **Correct 04a's recommendation 4 before anyone builds it** (§4.3): a
   skipped pass must be a *coalesced* pass, or dispatches are dropped.
7. **`ran_commit`, option 3** (04b §3.3), with one sentence added to §3.5:
   a rollback reads the field and erases it on the next write of that
   record.
8. **Dedupe the skip banner on the test name, not the line** (§6).

### Still needs a measurement

9. **The write half on the deployment's own filesystem.** Everything in §3
   is ZFS on this host's root. The hub runs in docker compose on the NixOS
   server and I did not go near it (D161). **H**: the figure is of the same
   order. What would settle it: `N` `record_waiting` calls against a state
   root on the deployed volume.
10. **The real `T` on the deployed hub.** Still **I**, as 04a leaves it. It
    sets whether item 3 is urgent or merely correct.
11. **A junction's wall clock, cleanly split.** My §3.5 is too noisy to
    apportion. Closing case 4's window after the pass instead of before it
    (poll `calls_to("record_waiting")` until it stops moving) is a ten-line
    change to an existing bench and would give the whole surface in one run.
12. **Cold page cache**, `sweep_outputs` (`store.rs:642`) and store startup
    (`store.rs:439`) — the two other O(`T`) walks. All three left **I** by
    04a, correctly, and all three now matter more, because `T` is no longer
    the term that justifies the work on its own.
13. **A round-trip test across the three crates** for the added field (04b's
    own **H**), which belongs in option 3's implementation. My §5 covers the
    hub and the record; the runner's encode path is not covered.

---

## 9. The strategy recommendation

**Alexa asked for a deep and durable remediation, not a run of local
patches. 04a's recommendation, taken as written, does not serve that goal —
and not because local fixes are wrong, but because the measurement that was
supposed to decide the question weighed one half of the pass.**

Stated plainly: 04a argues "the urgent fix is not architectural" from
"`T` costs 25x what `N` costs". With the write half counted, `N` costs
roughly 10x what `T` costs on the event where both are paid. The premise of
the argument does not hold, so the conclusion is not available — not false,
*not available*. Nothing I measured says the architecture must change
tomorrow; what I measured says the figure that was going to decide it was
the wrong figure.

**What I recommend, in order.**

* **Ship items 1 and 2 now.** Two one-line changes, both proved, neither
  touching the protocol, the record or the architecture. Holding them behind
  a decision is cost with no benefit — the same thing `03c §7` said about
  its own small findings.
* **Do not start item 3 or the reactor refactor until item 11 is run.** It
  is the cheapest thing on this list: ten lines in a bench that already
  exists, one run, perhaps twenty minutes, and it produces the one table
  that has never existed — a whole junction, split into the directory walk,
  the queue reads, the durable writes and the arithmetic, at two or three
  values of `N` and `T`. Both of the architectures D171 names, and both of
  the fixes 04a proposes, are priced off that table. Choosing now means
  choosing from the table we already know to be half of one.
* **Then take the architecture decision with items 4 and 5 inside it, not
  beside it.** The thing to decide is not "`spawn_blocking` or a pass
  thread". It is: *what bounds the synchronous work one event may cause the
  hub to do, and where does the store's lock granularity sit in that bound?*
  Framed that way, item 3 (the walk), item 4 (the writes) and item 5 (the
  lock) are three faces of one answer, and fixing any one of them alone
  leaves the structure able to grow the next. Framed as "fix the `T` walk
  first and the refactor becomes a milestone", item 3 is the only one that
  gets done, and the dossier closes on the smallest of the three terms.
* **Run `ran_commit` (item 7) on its own track, now.** It is independent of
  all of the above, it is additive, it has the longest lead time, and it
  answers the incident the consumer actually reported. 04b settled the
  question the dossier had wrong; there is nothing left to measure before
  Alexa can decide it.

**What each road costs, since the brief asks for it.**

*Local fix first (04a's road).* Cost: small, if the index variant is chosen
and the open-jobs variant is struck. Buys: 195–203 ms per junction, removed
and not coming back. Leaves open: the 8.3 s write half, the store lock, and
the general shape — nothing bounds one event's synchronous cost. Risk: the
dossier is declared answered on the smallest term, and the next incident is
the same incident with a different name.

*Persistence off the reactor first.* Cost: a milestone, a new executor, and
a reckoning with `Mutex<Live>` that nobody has scoped. Buys: the general
bound. Leaves open: nothing structural — but it is a large commitment taken
on a surface that is still, after two experiments, measured on one axis and
a half.

*What I actually recommend:* neither yet. Items 1 and 2 today; item 11 this
week; then one decision that covers 3, 4 and 5 together, priced off a
complete table. That is the shortest road to a durable answer, and it is
shorter than it looks, because the measurement it waits on is twenty
minutes of work.

---

## 10. What this review does not establish

* **I** — the deployed hub. Nothing on the consumer's instance was touched
  (D161). Every figure here is this host, ZFS on root, warm cache, release.
* **I** — why D151's run reported 13. I ruled out a deleted skip site; I did
  not reconstruct that session.
* **I** — a clean split of a live junction's wall clock (§3.5). Item 11.
* **I** — 04b's two benches were read, not re-run. Its `file:line` citations
  were all re-checked and are exact; its printed figures I take on the same
  footing the dossier takes `03a`'s and `03b`'s.
* **H** — that the write cost of §3 transfers to the deployment's volume.
* **H** — that a junction changes the cause for most queued jobs in general.
  It did for 50 of 50 in 04a's case 4 and in mine; a fleet of several runners
  where one joins may change fewer.
* **I** — I did not look for findings outside the two experiments' subjects,
  except where measuring one made another visible (§4.3, §6's last
  paragraph, item 1).

---

## 11. Reproducing

```sh
# 04a, replayed
cargo test -p herd-hub --test cost_surface --release -- \
  --ignored --nocapture --test-threads=1

# this review's six cases
cargo test -p herd-hub --test review_pass_cost --release -- \
  --ignored --nocapture --test-threads=1

# the skip count and its split
./scripts/test.sh
sort -u target/herd-skips.txt | wc -l
sort -u target/herd-skips.txt | grep -c "cgroup v2 delegation is unavailable"
```

`--test-threads=1` for the same reason 04a gives, and because the write
figures are `fsync`-bound and a second test writing beside them is a second
writer on the same pool.
