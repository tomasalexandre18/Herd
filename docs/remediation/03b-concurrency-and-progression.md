# 03b — Scheduler, concurrency and progression

Audit B of mandate 2 (`MANDAT_HERD_AUDIT_ARCHITECTURE_DECISION_2026-10-24`,
§4.2). Reading only: no Rust was changed, no test was run, no load was
placed on this host.

- **Date:** 2026-10-24
- **Base read:** `origin/main` at `0a2cf73`
- **Scope:** `herd-hub/src/{server,matcher,queue,store,durable}.rs`, the
  hub's tokio tasks, the runner's job-acceptance path, the hub-side tests
  that claim progression properties.
- **Out of scope here:** identity, attribution and durability of records
  (audit A); protocol compatibility; `herdctl` ergonomics.

## 0. How to read the marks

Every claim is **F** (observed in the tree at `0a2cf73`, with a
`file:line`, a test name or a decision entry), **H** (a causal hypothesis
I am asserting, not proving) or **I** (unknown — I could not establish it
without an experiment I am not allowed to run).

Three things that are deliberately *not* said anywhere below:

- No conclusion is drawn from the size of a file. `server.rs` is 7 985
  lines and `matcher.rs` 3 901 at this commit (F); both are coherent and
  heavily commented, and the defects found below are small and local, not
  consequences of length.
- No fix is credited with having removed a defect because a PR merged.
  D169 and D171 are each examined for whether they struck the cause, a
  symptom, or one case of a family.
- No figure measured by somebody else is restated as mine. D171's
  "3.21 s / 62 ms" and "17.3 s / 0.23 s" are *their* measurements, made on
  this host on 2026-10-24, and I mark them as such.

---

## 1. The model of the loops and the critical tasks

### 1.1 Runtime and tasks

**F** — The hub is one `#[tokio::main]` process (`herd-hub/src/main.rs:213`)
with the default multi-thread runtime: worker threads = available
parallelism, nothing configured. The consumer's container showed 13 threads
(D171), consistent with that.

**F** — `Hub::serve` (`server.rs:563`) spawns exactly four long-lived tasks
and then one task per accepted connection:

| Task | Source | Wakes | What it does |
|---|---|---|---|
| `watch_for_a_lost_runner` | `server.rs:5021` | `lost_after/20`, clamped to **1 s** (`server.rs:445`, `LOST_AFTER = 60 s` at `server.rs:79`) | drops silent links; times every attempt in the air; requeues |
| `keep_trying_to_write_results` | `server.rs:4926` | 250 ms, doubling to 5 min | retries verdicts the disk refused |
| `let_the_queue_age` | `server.rs:3186` | `aging_step/4` clamped to [1 s, 60 s]; **60 s** at the default `AGING_STEP = 30 min` (`queue.rs:45`) | re-judges the queue so aging takes effect |
| `say_again_why_jobs_wait` | `server.rs:3249` | min(15 s, 60 s) cadence | restates E-04 and breathes on quiet follower links |
| per-connection `serve_peer` | `server.rs:2073`, spawned at `server.rs:581` | per frame | the whole request path |

**F** — There is **no cap on accepted connections** and no semaphore
anywhere in `herd-hub/src` (`grep -rn "Semaphore\|max_connections"` returns
nothing).

### 1.2 The locks, and the order between them

**F** — Three `std::sync::Mutex` (blocking, not `tokio::sync`):

| Lock | Declared | Held across | Documented order |
|---|---|---|---|
| `State::dispatching` | `server.rs:720` | **one whole dispatch pass**, including every disk read and every fsync it performs | taken *before* `peers`, never after (doc comment at `server.rs:707-719`) |
| `State::peers` | `server.rs:706` | short critical sections; 13 maps keyed by `JobId` | taken inside `dispatching` only |
| `JobDirStore::live` | `store.rs:142` | **a full `unfinished_jobs()`**, i.e. one file read + JSON parse per pending job (`store.rs:2312-2325`) | independent of the two above |

The stated order is respected in the code I read, and `with_peers`
(`server.rs:1111`) is the only door to `peers`. **I found no lock-order
inversion and no deadlock candidate.** The problem is not ordering, it is
*duration*: two of the three locks are held across disk I/O.

### 1.3 Where a blocking call is made from an asynchronous context

This is the heart of §4.2's first question, and the answer is: **everywhere
that matters, and by design, not by accident.**

**F** — `herd-hub` contains **no `spawn_blocking`, no `block_in_place` and
no dedicated I/O thread**. `grep -rn "spawn_blocking\|block_in_place"
herd-hub/src/` returns nothing; the only hit in the workspace is
`herd-runner/src/job.rs:1222`.

**F** — `dispatch_queued_jobs` (`server.rs:2492`) and `dispatch_a_pass`
(`server.rs:2509`) are synchronous `fn`. They are called from **eleven
production sites**, and every one of them is inside an `async fn` or a
synchronous function reached from one:

```
server.rs:3199  let_the_queue_age                 (async task)
server.rs:3591  relay_from_runner                 (async, per frame)
server.rs:3609  relay_from_runner
server.rs:3669  relay_from_runner
server.rs:3932  relay_from_runner
server.rs:4984  keep_trying_to_write_results      (async task)
server.rs:5256  give_the_job_another_go           (sync, from the watchdog)
server.rs:5302  give_the_job_another_go
server.rs:5390  requeue_for_a_lost_capability     (async)
server.rs:6105  accept_submissions                (async, per submission)
server.rs:6584  raise_job                         (sync, from accept_submissions)
```

**F** — Inside a pass, the blocking calls are:

- `store.unfinished_jobs()` at `server.rs:2510` and again at `:2521` when a
  reading settled — each one takes `JobDirStore::live` and reads one
  `job.json` per pending job (`store.rs:2312`), parsing each file's text
  **three times** (`VersionProbe`, `DurableLabels`, `RecordFile` —
  `store.rs:1141`, `:1150`, `:1160`);
- `say_it_waits` → `store.record_waiting` (`server.rs:3442`,
  `store.rs:2007`) → `write_record` + `append_event` = `write_file_with`
  (temp, `sync_all`, rename, `sync_dir` — `durable.rs:35-56`, two fsyncs)
  plus `append_line` (`durable.rs:64-71`, one fsync): **three fsyncs per
  written wait**;
- `dispatch` → `store.begin_attempt` (`store.rs:2040`), same discipline;
- `end_the_job` → `record_state`, same discipline;
- `settle_readings` (`server.rs:2709`) → `store.submit` for every minted
  reading.

**F** — A second, entirely separate family of blocking calls on the reactor:
**90 `println!`/`eprintln!` sites in `server.rs` alone**
(`grep -c "println!\|eprintln!"`), plus `journal()` at `server.rs:3972`,
which locks stderr and writes synchronously. `println!` takes a
process-wide lock and writes to a pipe; under a container log driver that
pipe can block.

**H** — This second family has the *same* absent frontier as the first and
is not mentioned in D171. In the 12:14 incident the hub emitted 20 683
`stays queued` lines in some minutes; each one was a synchronous write to
the container's stdout taken on a reactor thread, under a lock shared by
all reactor threads. D171 removed most of those lines, so the *volume* is
gone; the *mechanism* is untouched, and nothing in the tree bounds what a
hub prints per pass. I did not measure it and it is not the thing the
thread states pointed at (one thread in `D`, i.e. disk, not a pipe), so I
mark it H and not F.

### 1.4 Critical-path table

| Entry | Module | State read | State written | I/O | Locks | Observability | Evidence |
|---|---|---|---|---|---|---|---|
| `Submit` frame | `accept_submissions` → `dispatch_a_pass` | whole queue | new record, index, every judged job's `waiting_for` | O(N) reads + up to 3·k fsyncs | `dispatching`, `peers`, `live` | `herd-hub: … stays queued` | `server.rs:6105` |
| health report | `relay_from_runner` → `dispatch_queued_jobs` | whole queue | as above | as above | as above | — | `server.rs:3591` |
| verdict frame | `record_the_end` → `dispatch_queued_jobs` | whole queue | record + state + metrics | as above | as above | per-job lines | `server.rs:3932` |
| watchdog tick (1 s) | `attempts_in_the_air` | **whole queue, every second** | — | O(N) reads | `live` | — | `server.rs:2459`, `:5050` |
| aging tick | `let_the_queue_age` | whole queue twice | per-job waits | O(2N) reads | all three | — | `server.rs:3192` |
| E-04 restatement | `say_again_why_jobs_wait` | followed jobs only | — | O(followed) reads | `peers` | notes to followers | `server.rs:3292` |

---

## 2. Why a deep queue or a slow store stops progression

### 2.1 What the code says (F)

Three facts compose:

1. A dispatch pass is synchronous and occupies the worker that called it
   for its whole duration (`server.rs:2492`, §1.3).
2. A pass costs **O(queue depth)** in disk reads unconditionally, and
   **O(number of jobs whose wait is news)** in fsyncs
   (`server.rs:2510`, `:3442`).
3. A pass is started by **every event**, including every single submission
   (`server.rs:6105`), every health report and every verdict.

**F** — Therefore a burst of *S* submissions against a queue of depth *N*
performs on the order of *S·(N + S/2)* record reads, because each
submission runs a complete pass over a queue that the earlier submissions
have just made deeper. For the consumer's real case (S = 214, one runner,
nothing placeable) that is on the order of 2·10⁴ `job.json` reads and
6·10⁴ serde parses, inside one second of wall-clock submissions. This is
the quadratic read cost; it is a property of "one pass per event over the
whole queue", and **D171 did not change it** — D171 changed only what a
pass *writes*.

**F** — `watch_for_a_lost_runner` adds a steady O(N) read of the whole
queue **once per second**, for ever, whatever the queue depth, because
`attempts_in_the_air` is unconditional (`server.rs:5050` calling
`server.rs:2459`). `docs/status.md:73` and board item **ORCH-021** already
track a cousin of this ("the hub re-reads every job record on each 200 ms
poll of a waiting `--outputs-to`"); the watchdog's copy is not named there.

### 2.2 The causal chain (H, built on the F above)

A slow store multiplies (2) directly: at *f* ms per fsync and *r* ms per
record read, a pass costs roughly `N·r + 3·k·f`. With every pass pinning a
worker, and *W* workers, the hub has no worker to poll anything once *W*
passes are concurrent — which is the state D171 describes from the
consumer's thread dump (one thread in `D`, twelve waiting, 1.8–3.9 % CPU:
blocked, not saturated). Nothing then answers a health report, so the
runner's 60 s link-silence limit expires, it redials, and **the redial
starts another pass** (`server.rs:3591`) — a positive feedback loop. I did
not reproduce this; it is the mechanism D171 states and the code at
`0a2cf73` still has every link of it except the fsync volume.

**H** — The loop's gain is now much lower but not zero. The fsync term
`3·k·f` collapses when causes stop changing (D171), but the read term
`N·r` does not, and the redial path still starts a pass. On a store whose
reads are slow — a network filesystem, a saturated disk, a container on an
overlay with a cold cache — `N·r` alone can re-arm the loop. **This is the
single most important thing D171 left in place.**

### 2.3 What was measured, and by whom (not me)

- D171, on this host, 2026-10-24: one pass over 400 queued jobs took
  **3.21 s** writing a line for each and **62 ms** writing none.
- D171's test `a_deep_queue_leaves_the_runtime_a_worker_to_answer_with`
  (`server.rs:7565`): a do-nothing task waited **17.3 s** for a turn on
  v0.3.6 and **0.23 s** after the fix, with 300 jobs queued and two
  workers.
- Everything else in this section is read or reasoned, not measured.

**I** — What a pass costs at 200–400 queued jobs on a *slow* store is
unknown. No fault injection for I/O latency exists anywhere in the tree
(see §10.1).

---

## 3. Backpressure

### 3.1 Where load arrives, and what bounds it

| Arrival | Bounded? | By what | Who pays |
|---|---|---|---|
| TCP connections | **no** (F) | nothing; one task + one unbounded channel each (`server.rs:581`, `:2332`) | hub host memory |
| one frame's size | yes (F) | `FRAME_CEILING_BYTES` on message and frame (`server.rs:6737`) | the sender, by refusal |
| submissions | **no** (F) | nothing refuses for queue depth; D38 makes waiting the answer to everything | hub host disk, and every later pass |
| queue depth | **no** (F) | nothing | every pass, O(N) |
| jobs per runner | yes (F) | label slots, declared memory, declared tmpfs, computed from the records (`matcher.rs:583`, `:685`) — **hub-side only** | the queued job, as a wait |
| requirement readings per runner | yes, **since D169** (F) | `--readings-at-once`, default 2 (`matcher.rs:267`, gate at `matcher.rs:793-803`) | the queued reading, as `Later` |
| log bytes runner → hub | yes (F) | `HELD_BYTES_PER_JOB`, oldest dropped (`herd-runner/src/lib.rs:562`) | the log's completeness, said out loud |
| **frames hub → any peer** | **no** (F) | `type Outbox = UnboundedSender<FromHub>` (`server.rs:246`), fed by `write_frames` (`server.rs:6676`) | hub host memory |

### 3.2 The asymmetry worth naming

**F** — The runner bounds what it holds for the hub and says so when it
drops bytes. The hub bounds nothing it holds for a peer. A follower that
reads slowly — `herdctl wait --outputs-to` over a poor link, or a
suspended process — makes its `Outbox` grow without limit while the hub
keeps relaying log chunks (`record_chunk` → `followers_to_tell` → `send`).

**H** — This is not what broke on 24 October, and I have no incident for
it. It is the same family: *a producer with no counterparty bound*. With
`big_log.rs` proving large logs work, the exposure is real but needs a slow
reader to show, which is exactly the scenario no test constructs.

### 3.3 Who pays the limits that do exist

Every bound in Herd is paid by **the queued job**, never by the requester:
`Verdict::Later` queues, it does not refuse (E-31, D38; `matcher.rs:793-803`).
That is a deliberate and, in my reading, correct product choice. Its cost
is the one §2 describes: the queue is the shock absorber, the queue is
unbounded, and the cost of a pass is linear in the shock absorber.

---

## 4. Concurrency, fairness, priority, preemption

### 4.1 Double assignment — not found (F)

The design here is good and I want to say so plainly. Every figure the
matcher uses is **derived from the records at the start of each pass** and
from nothing else (`matcher.rs:348` `holdings`, `:471` `reserved`, D79):
there is no counter to leak, nothing to release on any of the eight ways a
job ends, and nothing to rebuild after a restart. `seats` is carried
*through* the loop (`server.rs:2593`), so a job placed earlier in a pass
contends with the next one. One pass at a time is enforced by
`dispatching`, and the submission path takes that same lock around
submission-plus-pass so the two cannot interleave (`server.rs:6105` with
the doc comment at `server.rs:2500`).

Tests: `priorities.rs: no_job_is_claimed_twice_when_passes_overlap`,
`server.rs:7163 a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim`,
`slots.rs: the_slots_are_rebuilt_from_the_records_after_a_hub_restart`.

### 4.2 Starvation

**F** — Order is `effective priority` then `id` (`queue.rs:114-129`), with
aging raising the effective level every `AGING_STEP` (30 min). A job that
cannot be placed does **not** block the queue behind it: the loop
`continue`s (`server.rs:2593` onwards), so there is no head-of-line
blocking at placement.

**F** — Covered by `queue.rs:
the_lowest_priority_job_runs_within_the_aging_bound_on_the_real_clock`.

**H** — One starvation shape is left open by design and named in the code
(`server.rs:2616-2624`): a high-priority job sticking to a preferred busy
runner lets lower-priority jobs take lesser runners. It is bounded by the
stickiness limit, visible in the sentence, and is what E-19 asks for. Not a
defect; worth a row in the invariant table as an accepted exception.

**H** — A second shape is *not* named: aging is computed from wall-clock
`Timestamp::now()` per pass. If passes stop running — the §2.2 state — the
queue does not age, because aging is evaluated inside the thing that is
stuck. Progression failure and fairness failure are therefore the same
failure, which is an argument for treating I06 as load-bearing for I07's
fairness clause too.

### 4.3 Retry loops

**F** — Three budgets, counting three different things off the record, and
they cannot spend each other (D169's own table; `Policy::attempts_allowed`
= 2, `--requeue-budget` = 3, `Reading::lost` weighed against
`attempts_allowed`). Worst case is bounded at four readings plus two
attempts, and then one refusal naming both.

**F** — `requeue_for_a_lost_capability` (`server.rs:5332`) requeues and
immediately re-dispatches, with **no backoff**. Bounded at 3 iterations by
the budget, so it cannot spin; but each iteration is a whole pass, so a
burst of capability-loss exits costs 3·S passes.

**H** — No unbounded retry loop exists in the hub. The only loop that can
repeat without a budget is the §2.2 one — drop link, redial, start a pass —
and its budget is the operator's patience.

### 4.4 Preemption

**F** — There is none, and that is correct for this product: a running
attempt is never taken away to make room, and a job the watchdog finds
slow on a live runner is explicitly *left alone* (`server.rs:5080-5094`,
D169). The only thing resembling preemption is E-27's drain-first
reservation (`matcher.rs:993`), which holds a host *empty* rather than
stopping anything, bounded by `--drain-first-limit` (15 min).

---

## 5. Stop, drain, restart

### 5.1 The runner (F)

Handles `SIGTERM` and `SIGINT` (`herd-runner/src/lib.rs:2355-2379`),
refuses new work while draining (`take_job`, `lib.rs:1151-1155`), and is
tested at the five-second bound
(`a_busy_runner_asked_to_stop_is_gone_in_five_seconds`). Invariant 2's stop
clause is real and proven.

### 5.2 The hub (F)

**The hub has no signal handling at all.** `grep -rn
"tokio::signal\|SignalKind" herd-hub/` returns nothing. `SIGTERM` kills it
exactly as `SIGKILL` does, and the test tree says so explicitly: `killed_hub.rs:141
a_hub_asked_to_stop_mid_job_loses_nothing` passes `Signal::TERM` into the
same helper as `Signal::KILL` (`killed_hub.rs:142`).

This is defensible — safety comes from the atomic write discipline
(`durable.rs`) and from rebuilding `pending` from the directories at
startup, and it is covered by `a_hub_that_restarts_loses_no_result_and_no_queued_job`,
`a_log_survives_twelve_killed_hubs_whole`,
`a_killed_hub_reads_the_reservations_back_off_the_records`. But three
consequences follow and none of them is written down anywhere:

**H1 — a restart under a deep queue re-pays what D171 removed, once.**
`Peers::waiting` is in memory and empty after a restart (D171 says so and
calls it right). The first pass therefore writes `waiting_for` for every
queued job: at 200 queued that is 200 records × 3 fsyncs in one pass, on a
reactor thread, before the hub answers anything. D171 bounds the *aging*
burst and names it; it does not name the *restart* burst, which is the same
size and happens exactly when the hub is least able to afford it — during
recovery, with every runner redialling.

**H2 — the inherited-attempt clock starts at `State::started`**
(`server.rs:705`). After a restart, attempts the hub never heard of are
timed from zero; D169's change saves them if a connected runner's health
report names them, and loses them at 60 s if the hub has not managed to
process a health report by then. Under H1 that is not obviously true.

**H3 — there is no drain for the hub.** Nothing refuses new submissions
while the hub is going down, nothing finishes the in-flight pass, nothing
tells followers. For a service the consumer restarts to upgrade — which is
how 0.3.6 and 0.3.7 reached them — the absence is a product gap, not just
an implementation one.

**I** — Whether H1 actually delays a recovering hub past the runners'
60 s limit: unknown without the experiment in §10.2.

---

## 6. Host failure modes

| Mode | What the code does | Mark |
|---|---|---|
| **CPU exhaustion on the runner's host** | Nothing in Herd notices; the runner's kvm probe times out at 2 000 ms, the capability is suspended (D49), and every job needing it becomes unplaceable. This is the D169 incident's amplifier and it is **still the behaviour**: a saturated host disarms itself by suspending capabilities, which turns a slowdown into 201 refusals. | F (D49 path), H (amplification) |
| **OOM on the runner's host** | Per-job cgroup memory cap, exit 124. The *sum* of caps is bounded only by the hub's admission arithmetic (`matcher.rs:583`), and only for jobs that declare a memory need: `needs_memory == None` → `memory_mib = 0` and no memory gate at all (`matcher.rs:603-605`). | F |
| **Disk full on the hub** | `record_the_end` fails, the verdict is kept in `Peers::unwritten` and retried with exponential backoff to 5 min (`server.rs:4926`). Proven by `a_result_the_hub_cannot_write_is_kept_until_it_can`. `unwritten` is in memory, so a hub killed while the disk is full loses it — and the runner's spool re-reports. Good. | F |
| **Disk full / slow on the hub, during a pass** | Nothing. A pass that cannot write a `waiting_for` propagates the error out of `say_it_waits` and aborts the whole pass (`server.rs:3441-3443`): one unwritable job stops the queue being judged. | F |
| **I/O saturated** | §2. No detection, no degradation, no shedding. | F |
| **Runner wedged but connected** | D169's `what_the_runner_says` (`server.rs:1893`, `1971`) asks the machine before believing the clock; a report naming the attempt means busy, not gone. Unit-tested at `server.rs:7898-7975`. **But the link-silence check above it is unchanged and purely clock-based** (`server.rs:5030-5041`) — D169 says so in as many words. | F |
| **Hub unreachable** | Runner redials with its own backoff; verdicts spool; `a_spooled_verdict_reaches_the_hub_when_the_runner_comes_back`. Each redial starts a hub-side pass (§2.2). | F |
| **Messages reordered** | WebSocket over TCP gives per-connection order; across a redial there is none. `is_current` (`server.rs:4064`) and the attempt number are the guard. Reordering is audit A's ground; I found no progression-side hazard. | F / I |

---

## 7. The two incidents, re-examined

### 7.1 D169 — 214 jobs, ~200 readings, 201 × exit 125

**Cause, as the entry states it:** a requirement reading held no slot,
emptied no host and waited for no probe (D159), so nothing counted it; the
matcher put ~200 on one machine.

**What the fix does (F):** the bound lives in the claim step's seat
accounting (`matcher.rs:793-803`), read off `Holdings::readings_running()`,
derived from the records like everything else (D79). Over the cap the
verdict is `Later` — queued, never refused, never moved. Default 2, a
setting, zero refused at startup (`server.rs:426-433`). Two real defects
were fixed beside it: a lost reading now re-mints instead of refusing the
job, and the watchdog asks the runner before believing its clock.

**My verdict: the fix strikes the cause of *this* incident, and it is the
right shape** — one answer to one question, no counter to leak, nothing to
rebuild. The reading-specific part is sound.

**But it is one member of a family, and the family is not addressed (F):**

- The missing frontier is *"a kind of work that nothing counts"*, and
  readings were one instance, not the instance. At `0a2cf73` a job whose
  every label is `CapabilityKind::Shared` (`herd-proto/src/capability.rs:208`,
  `holders_allowed() → None`) and which declares **no** `memory_mib`, no
  per-thread figure and no tmpfs passes `admission` with `memory_mib = 0`
  (`matcher.rs:604`) and is bounded by nothing at all. `slots_for`
  (`matcher.rs:498`) returns 1 for such a job, but that figure is used only
  to derive `-j`, not as a gate. **I did not find a test that stands a
  fleet of such jobs against one runner.**
- **The runner has no bound of its own, and this is the structural
  finding.** `serve_hub` spawns a task for *every* `AssignJob` frame
  unconditionally (`herd-runner/src/lib.rs:2745`), and `take_job`
  (`lib.rs:1151`) refuses only while draining or switched off — never for
  count, never for load, never for free memory. Invariant 2 of `CLAUDE.md`
  — *"a runner never destabilizes its host"* — is therefore enforced
  **entirely by arithmetic in a different process**. D169 corrected that
  arithmetic; it did not add the second line of defence at the boundary
  where the host actually is. Any future hub bug, version skew, or
  mis-set `--readings-at-once` reproduces the 255-cgroup state exactly.

**What is proven, and what is not:** `herdctl/tests/bounded_readings.rs`
(two tests, against real binaries, overlap computed from the records) is a
good test of the bound. Neither it nor anything else asserts a *host-side*
invariant such as "this runner never holds more than K cgroups".

### 7.2 D171 — the hub blocked under a deep queue

**Cause, as the entry states it:** `record_waiting` judged news by
comparing the whole sentence, and the sentence embedded `queued 74th of
184`, so one arrival rewrote every standing job's record and history line,
each with fsyncs, on a reactor thread, under one lock held for the pass.

**What the fix does (F):** `say_it_waits` (`server.rs:3415`) now compares
`declared weighed as effective; because` against what it last wrote for
*that job*, holding the per-job sentence in `Peers::waiting` and pruning it
against the queue each pass (`server.rs:2677`). The countdown is
deliberately outside the key. Tested by
`a_queue_that_moves_writes_nothing_about_the_jobs_standing_still_in_it`
(`server.rs:7467`) and `a_deep_queue_leaves_the_runtime_a_worker_to_answer_with`
(`server.rs:7565`).

**My verdict: the fix strikes a real and well-identified cause, and it is
a symptom fix for the incident.** It removes three orders of magnitude of
cost from the dominant term. It does not touch why a cost in a pass becomes
a hub that answers nothing, and D171 says so itself, under *"The deeper
defect, deliberately not fixed here"*. I agree with the decision to defer
it and I disagree with nothing in that section. What I add is that the
deferred item is larger than the entry suggests:

1. **The reactor is still the only executor (F).** Confirmed: no
   `spawn_blocking` anywhere in `herd-hub`. Every store write and every
   pass runs on a tokio worker.
2. **The quadratic reads are untouched (F).** D171 changed what a pass
   *writes*. A pass still reads the whole queue, twice when a reading
   settled, and one pass still runs per submission (`server.rs:6105`). The
   steady O(N)-per-second watchdog read (`server.rs:2459`, `:5050`) is
   also untouched, and `read_stored` parses each record's text three times
   (`store.rs:1141`, `:1150`, `:1160`).
3. **The feedback loop is intact (F).** A link the hub failed to answer is
   dropped and redialled, and the redial starts a pass
   (`server.rs:3591`). Lower gain, same shape.
4. **One unwritable record still aborts a whole pass (F)** —
   `say_it_waits` propagates (`server.rs:3441`), so the loop in
   `dispatch_a_pass` exits and no job behind it is judged. On a full disk
   this is a queue that stops moving for a reason that appears only in the
   hub's own stderr.

**The family D171 and D169 share.** Both are instances of *a per-unit cost
paid inside a serialized, un-insulated critical section whose unit count
nobody bounds*. D169 bounded one unit count (readings per runner). D171
reduced one per-unit cost (fsyncs per queued job per pass). Neither
established the boundary: **nothing in Herd says what a dispatch pass may
cost, and nothing executes it off the thread that must answer the
network.** Other members of the same family, all present at `0a2cf73`:

- the O(N) read per pass and per watchdog tick (§2.1);
- the restart burst (§5.2 H1);
- the aging burst D171 names and accepts (184 records in one breath every
  30 min);
- `println!` on the reactor (§1.3);
- the unbounded `Outbox` (§3.1);
- the uncounted job kinds (§7.1).

---

## 8. The candidate invariants of §6

### I06 — Progression

- **Oracle:** from outside the runtime, a task that does nothing gets a
  turn within a requester's patience, while the hub is under its worst
  realistic queue and store.
- **Evidence now (F):** `a_deep_queue_leaves_the_runtime_a_worker_to_answer_with`
  (`server.rs:7565`), 300 jobs, 2 workers, 8 passes, 2 s bound. It is a
  real oracle, measured from outside the runtime — the right design.
- **What it does not search (F):** (a) a **slow store** — it runs on
  whatever tmpfs/disk the test host gives, and no latency is injected
  anywhere in the tree; (b) a **real hub and runner** — it builds `State`
  by hand, so the redial feedback loop of §2.2 is not in it at all; (c)
  **sustained** load — the 20 probes sample about 1 s while 8 passes run,
  and may not overlap them.
- **Verdict:** I06 is **not** proven. It has one good test of one term of
  the cost, on fast storage, in a harness that excludes the loop that
  actually stopped the hub. Classify **indispensable**; its proof is §10.

### I07 — Boundedness

- **Evidence now (F):** readings bounded (D169, `bounded_readings.rs`);
  slots, memory, tmpfs bounded and record-derived; runner log buffer
  bounded; frame size bounded; all 13 `Peers` maps have a pruning site
  (`server.rs:1847-1851`, `:3285-3288`, `:1604`, `:2677`).
- **Unbounded at `0a2cf73` (F):** queue depth; connections; per-connection
  `Outbox`; concurrent jobs accepted by a runner; a job kind that declares
  nothing.
- **Fail case not searched:** a slow follower; a job whose labels are all
  `Shared` and which declares nothing.
- **Verdict:** partially held, by instance and not by rule. Classify
  **indispensable**, and state it as *"every unit of work is counted by
  something on the host that runs it"* — which is the rule D169 satisfied
  for one kind.

### I02 — Validity of transitions

- **Evidence now (F):** one pass at a time under `dispatching`; one gate
  (`open_unless_over`, `store.rs:1116`) that refuses every write to a
  job that is over; the `queued → assigned` transition as the final catch;
  `no_job_is_claimed_twice_when_passes_overlap` and
  `a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim` with the
  store double that forces the window open (`server.rs:6928`).
- **Verdict:** the best-defended of the six, on the progression side.
  Audit A owns the late-message half.

### I08 — Stop / restart

- **Evidence now (F):** runner side proven (§5.1). Hub side: no stop path
  exists; restart is proven lossless by four tests.
- **Not searched:** a restart **under a deep queue** (§5.2 H1); whether a
  recovering hub answers inside the runners' 60 s limit.
- **Verdict:** half held. The missing half is a *progression* property of
  recovery, not a durability one, and it belongs to this report.

### I09 — Host isolation

- **Evidence now (F):** per-job cgroup caps, time caps, workspace
  confinement — all real and tested on the runner. The *aggregate* is a
  hub-side computation only (§7.1).
- **Fail case not searched:** the hub telling a runner to do more than its
  host can take. That is precisely the 24 October incident, and the test
  that was added (`bounded_readings.rs`) asserts the hub's arithmetic, not
  the host's state.
- **Verdict:** **the weakest of the six.** An invariant whose only enforcer
  is in another process is not an invariant of the host.

### I10 — No silent success

- **Evidence now (F):** `Placement::Impossible` and `Cancel` end jobs by
  name; `Verdict::Later` queues with a sentence; D27's skip rule in
  `scripts/test.sh`; refusals are never silent (`refuse`,
  `server.rs:6696`).
- **The one hole I found (F):** a pass aborted by an unwritable record
  (§7.2 item 4) leaves every job behind it unjudged, with no record
  anywhere saying the pass did not finish — only a line on the hub's
  stderr from whichever caller logged the error. A queue that stopped being
  judged is indistinguishable, from a record, from a queue nothing could be
  placed in.

---

## 9. The two unstable tests

The brief is right not to ask for a repair. Both are audit questions and
the answers differ.

### `herd-hub` `slots::a_runner_killed_mid_job_gives_its_slot_back`

**F** — The test starts a hub with `--lost-after 2.0`
(`slots.rs:210`) and its own comments say what goes wrong
(`slots.rs:266-276`): *"on a host where a healthy runner's whole process can
be descheduled for that long … the hub drops it, correctly, and it
rejoins"*. The author already identified the mechanism and added a
condition to narrow the window, leaving *"a residual window of one `herdctl`
spawn"*.

**My reading: it is both, and the product half is the interesting one.**
The test is written too tight for a shared host — two seconds is inside the
scheduling noise of this machine. But what it is tight *against* is a real
property: **the hub decides a link is dead from a wall clock evaluated on
a reactor thread that the hub's own dispatch passes can monopolise**
(`server.rs:5030`, §1.3). D169 fixed the *attempt* clock by asking the
machine; it explicitly left the *link* clock alone. So the test's flakiness
is a scaled-down rehearsal of the 12:14 incident: a healthy runner declared
gone because the hub was busy. Shortening `--lost-after` to 2 s does not
create that failure, it accelerates it. **Recommendation for the audit
file:** keep it on the register as evidence for I06, and do not "fix" it by
raising the threshold until the link clock has an answer like the attempt
clock's.

### `follower_alive.rs:230`

**F** — Line 230 is `assert!(longest < spread / 2, …)`. With
`BREATH = 200 ms` and `BREATHS_WANTED = 8` (`follower_alive.rs:59`, `:68`),
`spread` is about 1.4 s, so the assertion demands that **no gap between any
two frames exceed ~700 ms**, i.e. 3.5 cadences — while the assertion
immediately above it allows ten cadences (`let allowed = BREATH * 10`,
`follower_alive.rs:224`).

**F** — Worse, `longest` is computed over `heard`, which includes every
frame pushed *after* the gate is opened (`follower_alive.rs:158-169`): the
job finishing, its log, the verdict. That stretch is not the quiet stretch
the test is about, is not paced by any cadence, and on a loaded host
trivially exceeds 700 ms.

**My reading: this one is a badly written test**, and I would say so
without hedging. It mixes a measured property (breath cadence during
silence) with an unmeasured tail (teardown after the gate) inside one
`max`, and then compares the result against a figure derived from the
measured part alone. The product property it is meant to defend — a quiet
follower link carries a frame every 15 s — is already asserted correctly by
the `allowed = BREATH * 10` line above it. Board item **ORCH-022** already
records this as flaky and product-side; it can be answered by scoping
`longest` to the frames between the first and last breath. **That is a
finding, not an edit: nothing was changed here.**

---

## 10. Experiments I did not run, and what each would prove

None of these was run. Each needs the mandate's T2 gate: an isolated
machine, not this shared host.

### 10.1 Pass cost against store latency (the one I most want)

- **Topology:** no network. A `JobStore` wrapper that delegates to
  `JobDirStore` and sleeps a configured `d` before each write and each
  read — the pattern already exists in the tree as
  `StoreThatHoldsTheClaimOpen` (`server.rs:6928`), so this is cheap.
- **Load:** queue depths N ∈ {50, 200, 400, 1 000}; d ∈ {0, 1 ms, 5 ms,
  20 ms}; 1, 2 and 4 concurrent passes on a 2-worker runtime.
- **Oracle:** the D171 oracle — worst turn given from outside the runtime
  to a task that does nothing — sampled continuously for the whole run,
  not 20 times.
- **Proves:** the surface `worst_turn(N, d, W)`. It would say whether the
  surviving O(N) read term alone re-arms the §2.2 loop, and at what
  storage latency. **This is the single measurement that decides whether
  "move persistence off the reactor" is urgent or merely correct.**

### 10.2 Recovery under a deep queue

- **Topology:** real hub binary, real runner, isolated host.
- **Load:** 250 jobs queued against a fleet that can place none; kill the
  hub; restart it.
- **Oracle:** time from restart until the runner's first health report is
  answered, against the runner's 60 s silence limit; and the count of
  `waiting` lines written in the first pass.
- **Proves or refutes H1 and H2 of §5.2.** If the first pass exceeds 60 s,
  a hub restart under a deep queue is itself an incident, and the aging
  burst D171 accepts has a twin that nobody accepted.

### 10.3 A runner given more than its host can take

- **Topology:** one real runner, isolated host, and a **stub hub** that
  sends `AssignJob` frames directly — no matcher involved.
- **Load:** 50, then 250 concurrent trivial jobs.
- **Oracle:** peak `herd-job` cgroup count, host load, and whether the
  runner's own probes answer within 2 000 ms.
- **Proves:** whether invariant 2 holds **as a property of the runner** or
  only as a property of the hub's arithmetic. I predict it does not, from
  `lib.rs:2745` and `lib.rs:1151`. A negative result would be the
  strongest single argument in the whole audit for where the next boundary
  goes.

### 10.4 Submission burst cost

- **Topology:** hub + one runner, isolated.
- **Load:** 214 submissions in one second — the consumer's real shape.
- **Oracle:** count `job.json` reads with `strace -c` or an instrumented
  store; compare against the predicted `S·(N + S/2)`.
- **Proves:** §2.1's quadratic claim as a measurement rather than a count
  of call sites, and bounds the benefit of coalescing passes.

### 10.5 A slow follower

- **Topology:** hub + runner + one requester that connects, follows a
  chatty job, and never reads its socket.
- **Oracle:** hub RSS over time.
- **Proves or bounds** the unbounded `Outbox` of §3.1.

---

## 11. What I could not establish (I)

1. **Whether the surviving read cost alone can stop the hub.** The entire
   §2.2 argument past the fsync term is H. Only §10.1 settles it.
2. **What a pass costs on the consumer's actual storage.** D161 forbids us
   their machines; their figures reach us as reports. We have one pair of
   figures (3.21 s / 62 ms at 400 jobs) from this host, not theirs.
3. **Whether the `println!` family contributes.** The thread dump showed
   `D` state (disk), not a pipe write, so it is probably not what blocked
   on 24 October — but "probably" is the word, and no test or figure
   separates them.
4. **Whether the uncounted job kinds of §7.1 are reachable through the
   consumer's real declarations.** I read the matcher, not their capability
   files, and D161 means I should not read their files. The code path
   exists; whether anyone can walk it is theirs to say.
5. **Whether a recovering hub answers in time** (§5.2 H2, §10.2).
6. **The real-world rate of the two flaky tests.** Three agents observed
   them on 2026-10-24; I did not run them and have no denominator.

---

## 12. Five findings, ranked by gravity × exposure × detectability

1. **The hub's persistence and its scheduler share the reactor, and
   nothing bounds what a pass costs.** F: no `spawn_blocking` in
   `herd-hub`; `dispatch_a_pass` synchronous at `server.rs:2509`, eleven
   async call sites. D171 names this and defers it; it is the frontier
   every other finding here leans on.
2. **A pass is O(queue depth) in reads and runs once per event, so a
   submission burst is quadratic.** F: `server.rs:2510`, `:2521`,
   `:6105`; `store.rs:2312`; three serde parses per record at
   `store.rs:1141/1150/1160`. Untouched by D171. Plus a steady O(N)
   whole-queue read **every second** at `server.rs:2459`/`:5050`.
3. **Invariant 2 is enforced only in the hub.** F: the runner spawns a task
   per `AssignJob` unconditionally (`herd-runner/src/lib.rs:2745`) and
   `take_job` refuses only for drain and switch (`lib.rs:1151-1165`). D169
   fixed the arithmetic; the boundary is still absent, and the same
   arithmetic has at least one other uncounted kind (`Shared`-only labels
   with no declared memory, `matcher.rs:498`, `:604`).
4. **The hub has no stop path and an un-named restart burst.** F: no
   signal handling in `herd-hub`; `killed_hub.rs:141-143` treats TERM as
   KILL. H: `Peers::waiting` empty after a restart means every queued job's
   reason is written in the first pass — the aging burst D171 bounds, at
   the worst moment.
5. **One unwritable record aborts a whole dispatch pass, silently as far
   as any record is concerned.** F: `say_it_waits` propagates at
   `server.rs:3441-3443`, ending the loop in `dispatch_a_pass`. Touches
   I10 as much as I06.

---

## 13. Deviations from the brief, stated plainly

- **"Les lectures quadratiques signalées dans D171"** — D171 does not
  mention quadratic reads. Its words are a cost "linear in the queue" paid
  "per queued job per pass", and what it removes is writes. I answered the
  underlying question anyway (§2.1, §7.2 item 2): the quadratic behaviour
  is real, it comes from one pass per submission over a growing queue, and
  it survives D171 untouched. If the orchestrator's note came from one of
  the two 24 October analyses rather than from D171, the attribution
  should be corrected there.
- **"Un seul fichier"** — `herd-step.md` also requires a handoff under
  `docs/orchestration/history/`. I wrote both, and nothing else.
- **`docs/status.md` is stale** against `0a2cf73`: item 2 at line 69 still
  says a reading "declares no memory, and nothing bounds how many run at
  once", which D169 answered. Board item ORCH-010 already owns this; I
  changed nothing.
