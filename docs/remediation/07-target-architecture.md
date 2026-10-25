# 07 — The target architecture

**Date:** 2026-10-25.
**Base:** `origin/main` at `f507ab6`, plus `06-junction-breakdown.md` read
from `measure-junction-breakdown` at `8471939`.
**Author:** `herd-planner`, workshop C4.
**Mandate:** one architectural decision covering the three linked defects —
unbounded synchronous work, repeated durable writes, store locking — compared
on durability, progression, crash recovery and migration cost.

**What this piece is.** A design comparison and a recommendation. It changes
no code, runs no experiment and starts no work. Every figure in it was
measured by `04a`, `05` or `06`; every mechanism in it was read at a
`file:line` on the base above. Where I estimate, I say so and name the
measurement that would settle it.

**Marks.** **F** — read at a named `file:line` on the base, or measured in a
piece of this dossier and attributed. **H** — a hypothesis, with what would
confirm it. **I** — could not be decided here.

---

## 0. The decision, in one page

**What the measurement settled.** A runner junction at `N` = 1 000 queued
jobs costs the hub **9.69 s**, of which **9.633 s — 99.5 % — is durable
writing** (`06` phase 2, F). The two directory reads that `04a` was built
around are together **0.26 %**. The write cost is device-bound and proved so:
debug against release moves the three CPU posts by ×3.7 to ×8.0 and moves the
write post by **×1.00** (`06`, F). The store's single `Mutex<Live>` does not
create cost; it transmits it, and the `dispatching` lock makes one junction's
writes into the next junction's wait (`06` phases 1f and 3, F).

**So the architecture question is not "where does the pass run".** Moving
9.633 s of `fsync` from a reactor thread to a blocking thread leaves 9.633 s
of `fsync`. The question is **what a single event is allowed to cost, and
which of the hub's writes have to reach the platter before the hub may
continue.**

**The recommendation, named: "the keeper".** One owned thread performs every
store mutation; the reactor wakes it by message and never waits on a disk;
the thread's unit of work is bounded in time and re-armed; the hub's writes
are split into two durability tiers, and the store's mutex guards memory and
is never held across a filesystem call.

Its load-bearing claim, and the one Alexa must assent to:

> **`waiting_for` has no crash-durability requirement.** A hub that restarts
> has an empty `Peers::waiting` and its first pass rewrites every queued
> job's reason (D171, `decisions.md:8083-8086`, F; `03b §5.2 H1`). A wait
> explanation that a power cut destroys is reconstructed within one pass of
> the next start. It must be **visible** to a reader the instant it is
> written — E-53 — and `rename` already gives that. It does not need an
> `fsync`, and today it pays three.

**What it costs.** `State::dispatching` is deleted and D100's test is rebuilt
on the keeper's single-threadedness; a submission no longer dispatches
synchronously, so `herdctl submit` can return before its job is judged; a
crash truncates the tail of `events.jsonl`'s *waiting* lines, permanently,
and that is a diagnostic loss Alexa has to accept in writing.

**What it leaves open.** Post A — the O(`T`) history walk — is paced, not
removed: it stays linear in a directory count that grows for ever
(`store.rs:1981`, F). And nothing mechanically prevents a future `O(N)` store
call from being added to a reactor path except the guard in step 6 and the
rule it enforces.

---

## 1. The three problems, restated against the code

### 1.1 Unbounded synchronous work

`dispatch_a_pass` (`server.rs:2509`) is a synchronous `fn`. `grep -rn
"spawn_blocking\|block_in_place" herd-hub/src/` returns nothing (F).
`dispatch_queued_jobs` has **thirteen** non-test call sites —
`server.rs:2419`, `:2434`, `:3199`, `:3591`, `:3609`, `:3669`, `:3932`,
`:4984`, `:5256`, `:5302`, `:5390`, `:6105`, `:6584` (F) — and the hub runs
on `#[tokio::main]` with the default multi-threaded runtime
(`main.rs:213`, F). A pass therefore occupies a reactor worker for its whole
duration. That is D171's own diagnosis (`decisions.md:8117-8125`, F).

Three unbounded units of work hang off one event, and only the first is in
`06`'s table:

| unit | where | bound today |
|---|---|---|
| `N` × `record_waiting` | `server.rs:2619-2640` → `:3438` | none |
| one `collections_owed`, O(`T`) | `server.rs:2415` → `store.rs:2367` | none |
| `end_the_job` for every `Impossible` job in the pass | `server.rs:2645` | none |

The third is worth naming because **no piece of this dossier has measured
it**: a submission of a thousand jobs needing a capability nothing offers
ends all thousand in one pass, each a synchronous durable write and a state
transition — a *decisive* write, which no tiering may defer. `06`'s `I`4 says
so explicitly: its corpus never places and never ends a job. **I.**

### 1.2 Repeated durable writes

`record_waiting` (`store.rs:2007`) does `write_record` then `append_event`.
`write_record` → `durable::write_file_with` (`durable.rs:34-56`) does
`file.sync_all()` and then `sync_dir(parent)`; `append_event` →
`durable::append_line` (`durable.rs:64-72`) does `file.sync_data()`.
**Three syncs, not four** — counted under `strace` with the corpus cancelled
(`06`, F). Measured floor 2.39 ms, measured median 3.6 ms per call, ~10 ms
per job when a whole pass of 1 000 is summed (`06` phases 0, 1d, 2, F).

The quiet path D171 built costs **11.4 µs**, 320 times less (`06`, F). So the
cost is not "the queue is deep": it is **"the cause changed for many jobs at
once"**. Three events do that to the whole queue simultaneously:

- a runner joining or leaving — every queued job's cause names the fleet;
- an aging step — a queue submitted in one second ages in one second, which
  D171 names and bounds at "about 1.5 s of fsync every thirty minutes"
  (`decisions.md:8112-8116`, F);
- **a hub restart** — `Peers::waiting` is in memory and empty, so the first
  pass is news for every queued job, at the moment every runner is redialling
  (`03b §5.2 H1`, F on the mechanism, **I** on whether it outlasts the
  runners' 60 s limit).

**What E-04 actually promises, because the brief overstates it.** E-04's row
in `docs/coverage.md:21` says the restatement cadence is served from the
*stream*, and that **"the figures a requester is told are the last pass's,
held in memory and not on the record"** (F). D171 says the same
(`decisions.md:8081-8086`). The record's obligation under this contract is
narrower and comes from **E-53**: a job nobody is following still says *on
its disk* why nothing is happening to it. **E-53 is a visibility promise, and
`rename` keeps it. Nothing in E-04 or E-53 asks that a wait explanation
survive a power cut.** That is the whole of the design space this piece
opens, and it was closed only by assumption.

### 1.3 Store locking

`JobStore::job` takes the store's single `Mutex<Live>` per record read
(`store.rs:2255`); `collections_owed` (`store.rs:2367`) reaches it through
`collection_owed` once **per directory**; `unfinished_jobs`
(`store.rs:2313`) takes it **once and holds it across the whole `N`-record
read**. Two opposite granularity faults in one lock (F).

Measured against a mutex-free control doing the identical I/O: eight callers
of `collections_owed` cost **×8.60** one call, where the control costs ×1.66;
85–86 % of a caller's time is blocked (`06` phase 1f, F).

**Two honest qualifications the brief does not make.** First, post D at a
real single junction is **600 ns to 1.6 µs** — six to seven orders of
magnitude below post C (`06` phase 2, F). The lock is not a measured cost of
the junction. Second, `06`'s `I`1 states that nothing in this dossier
observes the production arrival process, so how often callers actually
contend is **I**. The lock is a **structural** finding supported by a
synthetic fan-out, and its importance is prospective: `05 §4.3` is right that
any design moving the pass off the reactor inherits this convoy, and *that*
is when eight concurrent callers stop being synthetic.

---

## 2. What every candidate must satisfy

Hard constraints, each read on the base:

1. **`PROTOCOL_VERSION` = 2, `RECORD_VERSION` = 1, checked with `!=`**
   (`store.rs:1143`, `store.rs:2297`, `server.rs:2141`, F). The check blocks
   the move up *and* the move back, and **there is no migration code anywhere
   in the tree** (F). The written additive rule — a `#[serde(default)]` field
   meaning, absent, exactly what its default says — is the only change that
   leaves the numbers alone (`job.rs:44-51`, `decisions.md:7293`, F).
2. **`durable.rs` is the one place the hub writes** (`durable.rs:1-12`, F).
   Any durability change is a change to that module and to who calls it,
   which is what makes it auditable at all.
3. **The hub has no stop path.** No `tokio::signal`, no `SignalKind`, in
   `herd-hub/src` (F; `03b §5.2`). `SIGTERM` kills it exactly as `SIGKILL`
   does, and `killed_hub.rs:141-142` passes both into the same helper.
4. **`T` grows for ever.** The only `remove_dir_all` on a job path is the
   output-parts directory (`store.rs:1981`, F).
5. **One unwritable record aborts the pass.** `say_it_waits` propagates out
   of the per-job loop (`server.rs:3438-3441`), so the jobs behind the
   failing one are never judged (F).
6. **Invariant 9 — Herd is shipped, not operated** (`CLAUDE.md`, D161).
   Nothing may require a hand on the consumer's hub. Every figure here is
   this workstation's; the deployment's device is **I** (`06` `I`2).
7. **Invariant 1 — stay specialized.** No workflow engine, no plugin
   surface. A write-ahead log and a general executor both have to answer to
   this; a hub thread does not.

---

## 3. The candidates

Six, described to the level of mechanism. A, B and D are the options already
on the table; C and E attack the write term itself, which is where 99.5 % of
the cost is; F is the one `05 §8` item 4 names and that must be evaluated and
refused on evidence rather than ignored.

### A — `spawn_blocking` at the call sites

D171's first named option. Each of the thirteen sites becomes
`tokio::task::spawn_blocking(move || dispatch_queued_jobs(&state)).await??`.
Several of them sit in synchronous functions reached from synchronous
functions (`decisions.md:8126-8129`), so those have to become `async` or
acquire a runtime handle. `State::dispatching` stays; errors come back
wrapped in a `JoinError`.

The pass still takes 9.69 s at `N` = 1 000. The reactor is free; the blocking
pool is not. Tokio's blocking pool defaults to 512 threads: a redial storm of
`C` runners puts `C` threads into the pool, all but one of them parked on
`dispatching`, each holding a stack, for up to `C` × 9.69 s. **The block is
relocated and a new unbounded resource is created.**

### B — a coalescing pass thread

D171's second named option, with the correction `05 §4.3` demands. One owned
`std::thread` runs the pass loop. The thirteen sites become
`state.wake_pass(reason)` — a non-blocking set of a dirty flag plus a
`Notify`. The thread waits, clears the flag, runs one pass, and runs again
immediately if the flag was set while it ran. `State::dispatching` is deleted:
there is one pass because there is one thread.

Coalescing is the part that matters and the part a naive "skip if busy" gets
wrong: `05 §4.3` shows that dropping a wake loses the dispatch the waking
event was about. Set-flag-and-re-run loses nothing.

Errors can no longer be returned to the caller that woke the pass. They go to
`eprintln!` and to a `last_pass` field the health surface can report. D100's
test is built on `dispatching` being held (`server.rs:6933`), so it is
rebuilt on the thread's single-threadedness.

One pass is still 9.69 s. Eight simultaneous junctions become **one or two
passes** instead of eight serialized ones.

### C — durability tiers: the explanatory write stops being synced

Attacks the term. Every store write is classified:

- **decisive** — a state transition, an attempt, a verdict, a reservation, a
  reading's answer, the index. Unchanged: temporary, `fsync`, rename, `fsync`
  the directory, `fdatasync` the event line.
- **explanatory** — `waiting_for` and its `events.jsonl` line, and nothing
  else today. Written through a new `durable::write_file_unsynced` (create
  temporary, write, rename, **no sync**) and the existing
  `durable::append_bytes` (`durable.rs:80-83`, **no sync**).

The precedent is in the module already: `append_bytes` exists because "a log
is a stream, not a record … the bytes are visible to any reader as soon as
this returns" (`durable.rs:74-79`, F). C generalises that sentence into a
rule and applies it to the one other field that satisfies it.

**Why it is sound.** `rename` is atomic and immediately visible, so a reader
of `job.json` sees the whole old file or the whole new one at every instant —
E-53 and `durable.rs`'s own discipline are kept intact. What is given up is
only survival of a power cut, and `waiting_for` is rebuilt by the first pass
after any restart because `Peers::waiting` starts empty (D171, F). The
`events.jsonl` waiting lines are **not** rebuilt: a crash truncates them, and
that is the real cost.

**What it is worth.** Post C is three syncs out of a 3.6 ms call whose
measured sync floor is 2.39 ms (`06` phase 0, F). Removing all three should
leave a file create, a write of ~500 bytes, a rename and an append.
**H: `record_waiting` falls from ~10 ms per job to 50–150 µs per job, and the
pass at `N` = 1 000 from 9.633 s to 50–150 ms.** Confirmed by running `06`'s
`each_post_alone` against a `write_file_unsynced` variant; see §7.

C changes no file format, no record version and no protocol. It leaves the
pass synchronous on the reactor.

### D — a write-ahead log, records as a projection

The hub appends every decision to one `wal.jsonl` on the state root, synced
once per group; `jobs/<id>/job.json` and `events.jsonl` become rendered
artefacts refreshed off the critical path. Recovery replays the WAL tail over
the rendered files.

One sync per group, whatever `N` is — the strongest bound on the write term
of any option here, and the only one that also bounds the *decisive* tier's
burst (§1.1's third unit).

Three costs, each structural. **(a)** E-53 and E-54 say the consumer reads
`job.json` while a job runs and re-judges it much later; a rendered file lags
the WAL by a bounded interval, which is a change to what those readers see.
**(b)** A record that will not read quarantines one job today (D52,
`store.rs:1143`); a WAL that will not replay stops the hub. One file becomes
a single point of unreadable state. **(c)** A state root written by a
WAL-carrying hub and then opened by an older one: `RECORD_VERSION` is a `!=`
check on the *records*, which would not have changed, so the old hub opens
the root happily, ignores the WAL and overwrites the rendered files — **a
silent rollback data-loss path**, the same shape `05 §5` found for
`ran_commit`, with the whole state root instead of one field. Avoiding it
needs a second version check and a migration, and **no migration code exists
anywhere in this tree** (F).

### E — the keeper: B + C + a bounded unit + a lock that guards memory

The recommendation. Four components, each separately deliverable:

1. **The store's `Mutex<Live>` guards `Live` and is never held across a
   filesystem call.** `read_record` comes out from under the guard;
   `collections_owed` stops taking and releasing it 20 001 times without
   replacing that with one long hold.
2. **The keeper** — B, exactly.
3. **Durability tiers** — C, exactly, and only after a stop path exists.
4. **A bounded unit of work.** The keeper's loop takes a time budget. One
   unit judges jobs from a round-robin cursor over the queue until the budget
   is spent, then yields and re-arms. The budget counts **everything the unit
   does**, which is the point: post A (`collections_owed`) and the
   `end_the_job` fan-out are inside it, not beside it.

The bound is what makes the property stateable without reference to `N`, `T`
or the device: **no single event causes the hub more than one budget of
uninterrupted work.** That sentence is false today for three different
reasons and true under E for all three.

Component 4 bounds **latency, not throughput**, and that limit must be said
plainly: if cause-changes arrive faster than the device retires writes, the
records fall permanently behind memory. Under C the device retires an
explanatory write in ~100 µs rather than ~10 ms (H), which is what makes the
pacing enough rather than a queue that never drains.

### F — `waiting_for` ceases to be stored, and is derived on read

`05 §8` item 4 names it. The field leaves the record; a reader asking for a
queued job's reason gets it computed from `Peers::waiting` and the queue.
Post C goes to zero and the pass at `N` = 1 000 becomes ~46 ms.

**Refused, by the repository's own words.** D171 keeps the field precisely so
that *"a job nobody is following still says on its disk why nothing is
happening to it (E-53)"* (`decisions.md:8063`, F), and E-53 is a consumer
expectation about reading the files, not about asking the hub. A derived
answer requires a live hub and an open connection; the field exists for the
case where there is neither. F is the cheapest option on the table and it
breaks the contract the field was created to keep.

---

## 4. The comparison, on Alexa's four axes

`N` = 1 000, `T` = 0, this workstation, release. The baseline row is `06`
phase 2, measured. Every other junction figure is derived from it and from
`06` phase 0's floors, and is marked **H** where it is.

| | A `spawn_blocking` | B keeper thread | C durability tiers | D write-ahead log | **E = B+C+bound+lock** | F derive, do not store |
|---|---|---|---|---|---|---|
| **junction window** | 9.69 s (unchanged, F) | 9.69 s (unchanged, F) | ~90–190 ms (H) | ~50 ms (H) | ~90–190 ms, **and bounded per unit** (H) | ~46 ms (H) |
| **8 junctions at once** | 8 passes, ~77 s, 8 pool threads | 1–2 passes, ~10–20 s | 8 passes, ~1.5 s, reactor blocked | 8 passes, ~0.4 s, reactor blocked | 1–2 units, ≤ 2 budgets | 1 pass |
| **durability: decisive writes** | unchanged | unchanged | unchanged | **changed**: durable in the WAL, visible in the record later | unchanged | unchanged |
| **durability: `waiting_for`** | unchanged | unchanged | atomic + visible, **not crash-durable**; rebuilt by the next pass | durable in the WAL | as C | **does not exist** |
| **durability: `events.jsonl` wait lines** | unchanged | unchanged | **tail lost on a crash** | durable | **tail lost on a crash** | do not exist |
| **progression: reactor free of disk** | yes | yes | no | no | yes | no |
| **progression: bounded per event** | **no** | no | no | no | **yes** | no |
| **progression: storm amplification** | worse (pool grows) | removed by coalescing | unchanged | unchanged | removed | unchanged |
| **crash recovery** | unchanged | unchanged | unchanged for every decisive fact; `waiting_for` reconstructed | **replay**; new unreadable-state class | as C | unchanged |
| **recovery burst (03b H1)** | unchanged | unchanged | ~100× cheaper (H) | ~100× cheaper (H) | **bounded** by the budget | removed |
| **needs a stop path first** | no | no | **yes** | **yes** | **yes** | no |
| **migration: protocol** | none | none | none | none | none | none |
| **migration: `RECORD_VERSION`** | none | none | none | none (records unchanged) | none | the field leaves the record |
| **migration: state root** | none | none | **none — forward and backward compatible, a tiered hub's state root is an ordinary one** | **required, and no migration code exists**; silent rollback loss | none | a rolled-back hub reads a field that is no longer written |
| **reversible** | per call site | as one commit | **by a flag, at any time** | **not after the first WAL is written** | per component | no |
| *(information only, not a criterion)* *sites touched* | *13 + their callers* | *13 + ~8 test sites + D100* | *2 call sites, 1 new `durable` fn* | *`durable.rs`, `store.rs`, recovery, a renderer, a version* | *all of the above* | *`job.rs`, `store.rs`, every reader* |

### Reading the table

**A is refused.** It satisfies the sentence "move the pass off the reactor"
and changes not one number in `06`'s table. It is the clearest instance of
the thing Alexa named: a remediation that leaves the structure able to
produce the next defect, while appearing to have addressed this one. It also
*adds* an unbounded resource — a blocking pool that grows with the storm.

**B alone is refused**, for the same reason in a milder form, with one real
gain: coalescing. Eight junctions becoming one pass is a 4–8× improvement on
the worst measured case (`06` phase 3, where the second junction waits the
whole of the first's post C, F), and it is the only option that removes the
*amplification* rather than the unit cost. But a 9.69 s pass is a 9.69 s
dispatch latency, and a hub that answers health reports while taking ten
seconds to place a job has traded one symptom for another.

**C alone is refused**, despite removing 99 % of the measured cost. It leaves
every store call synchronous on the reactor and `T` walking for ever, so the
first `O(N)` store operation added to a reactor path after it — and `05 §8`
item 1 already names one that exists, `collections_owed().find()` at
`server.rs:4384` — reproduces the defect with a new name. It is a correct fix
to the term and not an architecture.

**D is refused on durability and migration, not on size.** It is the most
ambitious option and it buys, over C, only the decisive tier's burst — which
§1.1 says is unmeasured and may be small. Against that it puts a rollback
path that silently discards state, on a project whose version checks are `!=`
and whose migration code is nonexistent, and it makes the consumer's own
reading of `job.json` eventually-consistent, which E-53 and E-54 did not
agree to. **"Sound and durable" argues against D here, not for it.** If the
unmeasured burst of §1.1 turns out to be large, the answer is to bound it
(component 4), not to re-found the storage layer.

**F is refused by D171's own sentence** and by E-53.

---

## 5. What the recommendation leaves open

Stated before the recommendation itself, because this is the column Alexa
asked for.

1. **Post A is paced, not removed.** `collections_owed` stays O(`T`), `T`
   grows for ever (`store.rs:1981`, F), and at `T` = 20 000 the walk is
   186–211 ms (`06` phase 1a, F). Inside the keeper's budget that is one
   whole unit spent on a question that could be answered from an index. E
   makes the walk harmless to the hub's responsiveness and leaves it a
   growing waste. **The index `05 §8` item 3 recommends is a separate
   storage decision for Alexa and is not folded into this one.**
2. **The bound is on latency, not on throughput.** §3 E, last paragraph.
3. **Nothing mechanically forbids the next unbounded reactor call** except
   the guard of step 6 and the review rule it encodes. A guard that counts
   durable writes made off the keeper thread catches writes; it does not
   catch a 200 ms *read* done on a reactor path, and `server.rs:4384` is
   exactly that. **H: a second guard counting store calls by thread would
   catch it; whether that can be done without threading a context through
   `JobStore` is not designed here.**
4. **The `events.jsonl` waiting tail is lost on a crash**, permanently. Not
   reconstructible, by design, because it is a history of what was said at a
   time.
5. **Stale temporaries.** `durable::temporary_beside` names them
   `.job.json.tmp-<pid>-<n>` (`durable.rs:235-242`, F) and **nothing in the
   tree removes a temporary left by a crash** (F). The unsynced tier widens
   the window in which one exists. Step 4 adds a sweep at
   `JobDirStore::open`, which already scans (`store.rs:439`).
6. **The deployment's device is unknown** (`06` `I`2, invariant 9). Every
   absolute figure here is a SATA ZFS mirror with no SLOG. On NVMe the whole
   problem is roughly an order of magnitude smaller and the *urgency*
   changes; the *shape* does not, and neither does the recommendation.

---

## 6. The recommendation

**Build E, the keeper, in the order of §7.** The argument, in four steps:

1. **The term has to go, and only C or D removes it.** 99.5 % of a junction
   is `fsync`, proved device-bound (`06`, F). A and B are therefore not
   answers to the measurement; they are answers to the diagnosis D171 wrote
   before the measurement existed.
2. **Between C and D, C is the sound one.** C changes no format, needs no
   migration, is reversible by a flag after any amount of running, and gives
   up exactly one guarantee — crash-survival of a field the hub already
   discards and rebuilds at every start. D gives a better bound on an
   unmeasured burst and takes a silent rollback hazard and a new class of
   total state unreadability in exchange, in a repository that has never
   written a migration.
3. **But the term is not the whole defect.** `05 §9` is right: fixing the
   largest term and leaving the structure is how a dossier closes on the
   smallest one. The structure is "an event causes work with no ceiling, on
   the reactor, behind one lock". C fixes neither the ceiling nor the lock,
   and `05 §4.3`'s warning holds: a pass moved off the reactor inherits the
   convoy unless the lock moves with it.
4. **So the architecture is the bound, and C is what makes the bound
   affordable.** Component 4 states a property — one event, one budget — that
   is checkable from outside and does not mention `N`, `T` or the device.
   Components 1 to 3 are what make that budget large enough to get work done
   in.

**What it costs, named.**

- `State::dispatching` is deleted. D100's guarantee — one claim at a time —
  moves from a mutex to a thread, and `server.rs:6933`'s test is rewritten
  against the new mechanism. A guarantee that moves is a guarantee that must
  be re-proved, not assumed.
- A submission no longer dispatches inside its own call. `herdctl submit`
  may return before the job is placed, and the ~8 tests that call
  `dispatch_queued_jobs` directly need a test door that runs one unit
  synchronously and returns its error.
- A crash truncates the tail of the waiting history.
- A stop path becomes mandatory rather than merely absent, because a deferred
  write that `SIGTERM` discards is a regression.

---

## 7. The plan, stage by stage

Each stage is one branch, one PR, deliverable and revertible on its own, with
an exit criterion that is a run and not a reading. The validation criteria
`V1`–`V10` are defined in §8.

**S0 — Price the unsynced write. No product code.**
Extend `herd-hub/tests/junction_breakdown.rs` (already `#[ignore]`d, already
on `measure-junction-breakdown`) with a `write_file_unsynced` variant of post
C, swept at `N` = 200, 1 000, 4 000, and with the same `strace` sync count.
*Exit:* a figure for an unsynced `record_waiting`, and the sync count at zero.
*Why first:* it is the only thing that can refute the H the whole
recommendation rests on. If the unsynced write costs 2 ms rather than 100 µs,
C is not worth its contract cost and the recommendation becomes B + a much
harder component 4. **Revertible: nothing to revert.**

**S1 — A stop path for the hub.**
`tokio::signal` on `SIGTERM` and `SIGINT`: stop accepting connections and
submissions, let the in-flight pass finish, flush, exit. Nothing else changes.
*Exit:* **V9**. *Revertible:* delete the handler. *Why before S4:* a
deferred write that an upgrade discards is worse than a synchronous one, and
the consumer upgrades by restarting the container (`00 §9.2`, F).

**S2 — The store's lock guards memory, never I/O.**
`read_record` comes out from under the guard; `collections_owed` is restruc-
tured so it neither takes the lock 20 001 times nor holds it for 200 ms.
*Exit:* **V5**, plus "no writer is blocked more than 1 ms by a reader",
both measurable with `06`'s phase 1f harness as it stands.
*Revertible:* per method. *Why before S3:* `05 §4.3` — S3 inherits the convoy
otherwise, and S2's benefit is only observable once several callers really
run at once.

**S3 — The keeper.**
The pass thread, the coalescing wake, `dispatching` deleted, errors surfaced
on a `last_pass` field, a synchronous test door.
*Exit:* **V3**, the whole existing suite green, and **V1** partially — the hub
answers throughout a storm, with dispatch latency still long.
*Revertible:* as one commit.

**S4 — Durability tiers.**
`durable::write_file_unsynced`; `record_waiting` moved to it and to
`append_bytes`; a sweep of stale temporaries at `JobDirStore::open`; a flag
(`--durable-waits`, default off) that puts the tier back on the synced path.
*Exit:* **V4** and **V8**. *Revertible:* the flag, with no state migration in
either direction. *Depends on:* S0 (the figure), S1 (the stop path).

**S5 — The bounded unit.**
A time budget on the keeper's unit, a round-robin cursor over the queue for
fairness, and `collections_owed` and the `end_the_job` fan-out brought inside
the budget.
*Exit:* **V2** and **V7**. *Revertible:* set the budget to infinity, which
restores S3's behaviour exactly.

**S6 — The guard and the decision entry.**
A counter in `durable.rs` of durable writes performed off the keeper thread,
asserted zero by an end-to-end test; the decision written in
`docs/decisions.md` with what was given up.
*Exit:* **V10**, **V1** in full, **V6**. *Revertible:* the assertion.

S0 is a measurement and blocks only S4. S1 and S2 are independent of each
other and of everything else, and either could ship first. S3 → S5 is a
chain. S6 closes.

---

## 8. System validation criteria

What tells us, **in execution and not in reading**, that this is repaired.
Each is an oracle observed from outside the hub, with real binaries, and each
names the incident or the measurement it answers.

**V1 — the D171 oracle.** `N` = 1 000 queued, `T` = 20 000, four runners
redialling every 10 s for five minutes. Throughout: every
`herdctl runner list` returns in **< 2 s**, and no runner's journal ever
carries *"the hub has said nothing for 60s"*. *Today:* `rc 124` at 10 s, 30 s
and 180 s for eight minutes, and three silence detections in three minutes
(`00 §9.2`, F). This is the one criterion that reproduces the incident.

**V2 — one event, one budget.** With `N` = 4 000 and `T` = 100 000, no single
keeper unit exceeds its budget, read off the keeper's own trace. Covers all
three unbounded units of §1.1, including the `end_the_job` fan-out that
nothing has measured.

**V3 — coalescing.** Eight runners dialling together cause **≤ 2** keeper
units. *Today:* eight passes, each waiting the whole of the one ahead
(`06` phase 3: the second junction's wait is the first's post C, F).

**V4 — the write term.** Post C at `N` = 1 000 under **300 ms**, by `06`'s own
`one_real_junction_broken_down`, which exists. *Today:* 9.633 s.

**V5 — the lock.** Eight concurrent `collections_owed` callers cost **≤ 1.5×**
one caller, by `06`'s phase 1f with its mutex-free control. *Today:* ×8.60
against a control of ×1.66.

**V6 — D169 is not undone.** `bounded_readings.rs` green, and the D169
scenario replayed: 214 jobs submitted in one second against a one-runner
fleet leaves the runner holding at most `--readings-at-once` plus its seats,
and the hub answering throughout. *Today:* the bound holds; what V6 adds is
that it still holds when the pass is a thread. 255 cgroups and host load 399
is the observed ceiling of no bound at all (`00 §9.1`, F).

**V7 — restart under a deep queue.** A hub restarted with 1 000 queued jobs
while four runners redial answers a fleet listing within 2 s and dispatches
its first job within 5 s. Covers `03b §5.2 H1`, which **nothing in the tree
tests today** and which is the same burst as D171's, at the moment the hub
can least afford it.

**V8 — a crash loses nothing decisive.** The four existing killed-hub tests
green, plus a new one: a hub killed mid-unit with 1 000 queued jobs loses no
verdict, no queued job and no state transition, and every `waiting_for` it
lost is rewritten by the first pass after restart. **This is the test that
proves the tiering, and S4 does not merge without it.**

**V9 — the hub stops in five seconds.** `SIGTERM` → clean exit ≤ 5 s, nothing
decisive lost, mirroring invariant 2's runner bound, which
`a_busy_runner_asked_to_stop_is_gone_in_five_seconds` already proves on the
other side.

**V10 — no durable I/O off the keeper.** The `durable.rs` counter of writes
made on another thread is zero after a five-minute storm. Structural, cheap,
and enforceable because `durable.rs` is already the one door
(`durable.rs:1-12`, F).

Two of these — V1 and V7 — need a host that can carry a deep queue and real
`fsync` load without a production neighbour feeling it (`00 §11`, F). They are
the campaign, not a unit test.

---

## 9. Measurements I am missing

Named rather than guessed, in the order they would change a decision.

1. **The cost of an unsynced `record_waiting`.** The H the recommendation
   rests on (§3 C). `06` phase 0 gives `create`+`write`+`sync_all` at 1.19 ms
   and `append`+`sync_data` at 1.20 ms but never separates the sync from the
   write, so the unsynced figure is an estimate from the debug/release
   evidence that the sync is essentially all of it. **S0 settles it in one
   run of an existing bench.** If it comes back above ~1 ms per job, C's
   contract cost is not worth paying and the recommendation changes.
2. **The decisive tier's burst** (§1.1, third unit). A pass that ends 1 000
   `Impossible` jobs pays 1 000 synchronous decisive writes, and `06`'s `I`4
   says no bench has ever placed or ended a job. If that burst is large, it
   raises component 4's importance and is the one argument that could revive
   D. **Settled by** a `06`-style cell whose corpus is placeable and a cell
   whose corpus is impossible.
3. **Whether the deployment's state root is a dedicated filesystem.** Only
   needed if S0 refutes the unsynced tier and group commit via `syncfs`
   becomes the fallback — `syncfs` on a shared filesystem drags the
   neighbours. Invariant 9: this is a question for the consumer's operator,
   not a measurement we may take.
4. **`T` on the deployed hub** (`05` item 10, still **I**). Sets whether post
   A is 200 ms or 2 s per junction, and therefore whether §5 item 1 is a
   waste or a problem.
5. **The deployment's device** (`06` `I`2). Changes every absolute figure by
   about an order of magnitude and changes no conclusion.
6. **How often junctions really overlap** (`06` `I`1). Decides how much V3 is
   worth in service. Needs an observation of the production arrival process,
   which invariant 9 puts outside this project.

Nothing here was run. Per the brief, no experiment was started for this piece.

---

## 10. What needs Alexa's word

One decision, with one recommendation, as `CLAUDE.md` asks.

> **Does Herd accept that a queued job's wait explanation is visible but not
> crash-durable?** Concretely: `job.json`'s `waiting_for` is written
> atomically and is readable the instant it changes, and a power cut may
> discard the last change — which the first pass after the restart rewrites
> anyway. The `events.jsonl` lines saying *"stays queued: …"* lose their tail
> on a crash and are not rewritten.
>
> **Recommendation: yes.** It is 99.5 % of a runner junction, it is the one
> guarantee in the store that nothing depends on, the hub already throws the
> fact away at every restart, and the alternative that keeps the guarantee —
> a write-ahead log — buys an unmeasured burst in exchange for a silent
> rollback hazard in a repository with no migration code.
>
> **Fallback if no:** S1, S2, S3 and S5 still stand and still bound the hub's
> responsiveness; the budget then paces 9.633 s of unavoidable `fsync` across
> many units, so a deep queue's records lag memory by seconds to minutes and
> V4 is dropped. That is a worse system, and it is a coherent one.

Three things that do **not** need a decision and are noted so they are not
re-litigated: no protocol change is proposed; no `RECORD_VERSION` bump is
proposed; nothing in this piece touches a consumer machine.

Two items that belong to other decisions and are deliberately not folded in:
the `collections_owed` index (`05 §8` item 3 — a storage decision), and the
two one-line fixes `05 §8` items 1 and 2, which `05` is right to say should
not wait for this.

**ORCH-047's standing recommendation is superseded.** It reads *"take the `T`
walk out of `collections_owed` first, before and separately from any executor
change"*, resting on `04a`'s "`T` dominates by 25×". `05 §3` and `06` phase 2
reverse that: at `N` = 1 000 the `T` walk is 0.16 % of a junction and the
writes are 99.5 %. The walk is still worth removing; it is not the thing to do
first.

---

## 11. What I judge wrong in the brief

Said plainly, because the brief asked for it.

1. **"Un enregistrement non écrivable avorte toute la passe en silence"** —
   it aborts the pass, which is `03b`'s finding and is right, but **not in
   silence**. Of the thirteen call sites, `server.rs:2434`, `:4984` and
   `:5302` print the error; the rest propagate it into a handler that reports
   it. What *is* unreported is that the pass was **partial**: the jobs behind
   the failing one are silently never judged, and nothing anywhere says a
   pass was cut short. The defect is partiality without a signal, not
   silence. F.
2. **"Le verrou est pris et relâché une fois par répertoire"** — true of
   `collections_owed` (`store.rs:2367` → `collection_owed` → `JobStore::job`
   → `store.rs:2255`) and **the opposite of true** for `unfinished_jobs`,
   which takes it once and holds it across the whole `N`-record read
   (`store.rs:2313`). Two opposite granularity faults in one lock, and the
   brief names one. The second is the one that blocks *writers*, which is
   what `05 §4.3` cares about. F.
3. **The brief ranks store locking as the third co-equal problem.** `06` is
   explicit that post D at a real single junction is 600 ns–1.6 µs and that
   `06` `I`1 leaves the production arrival process unobserved. The lock is a
   **structural** finding on a synthetic fan-out, not a measured production
   cost. That does not make it unimportant — it becomes load-bearing the
   moment the pass moves off the reactor — but it changes the *reason* S2
   comes early: "S3 would otherwise inherit a convoy", not "the lock costs us
   85 % today". F.
4. **"La borner, c'est toucher à ce qu'on leur a promis [E-04]."** This is
   the brief's most consequential error, and correcting it is what opens the
   design space. E-04's contract is served from **memory**: `coverage.md:21`
   says *"the figures a requester is told are the last pass's, held in memory
   and not on the record"*, and D171 (`decisions.md:8081-8086`) says the
   same. The record's obligation is **E-53**'s — that a job nobody is
   following still says on its disk why nothing is happening to it — which is
   a **visibility** promise that `rename` keeps without any `fsync`. Bounding
   or de-syncing `record_waiting` does not touch E-04's cadence at all. F,
   from both documents.
5. **"Les deux que D171 nomme lui-même … sont un point de départ."** Agreed,
   and worth sharpening: D171 named them in October before anything was
   measured, and `06` has since shown that **neither of them changes any
   number in the table**. They are not a weak starting point; on the
   measurement they are not answers to the question at all. Treating them as
   the baseline pair risks anchoring the decision on the axis the measurement
   eliminated.
6. **"Ne pas choisir automatiquement la solution qui demande le moins de
   code."** Honoured — no column of this comparison is code size, and the
   sites-touched row is marked as information. But the converse deserves
   stating, because it decides the recommendation: here the two options that
   demand the **most** code (D, and F) are the two that break a contract or
   create an unrecoverable state class. On a repository with `!=` version
   checks and **no migration code anywhere**, "sound and durable" argues
   against the largest change, not for it. An architecture is judged by what
   it guarantees, in both directions.
7. **One premise I checked and confirm, against my expectation.** "Thirteen
   async call sites" is exactly right: thirteen non-test calls to
   `dispatch_queued_jobs`, enumerated in §1.1, and no `spawn_blocking` or
   `block_in_place` anywhere in `herd-hub/src`. F.

---

## 12. F, H and I

**F — read at a `file:line` on the base, or measured and attributed.**

1. A junction at `N` = 1 000 is 9.69 s, of which 9.633 s (99.5 %) is durable
   writing; the two reads are 0.26 %. (`06` phase 2.)
2. Post C is device-bound: debug/release is ×1.00 for C and ×3.7–×8.0 for A,
   B and E. (`06`.)
3. `record_waiting` makes three syncs — two `fsync`, one `fdatasync`.
   (`06`, `strace`; `store.rs:2007`, `durable.rs:34-56`, `:64-72`.)
4. D171's quiet path costs 11.4 µs, 320× less. (`06` phase 1d.)
5. Thirteen non-test call sites of `dispatch_queued_jobs`; no
   `spawn_blocking` or `block_in_place` in `herd-hub/src`; `#[tokio::main]`
   at `main.rs:213`.
6. `JobStore::job` locks per record (`store.rs:2255`); `unfinished_jobs`
   holds the lock across the whole read (`store.rs:2313`); eight callers of
   `collections_owed` cost ×8.60 against a control of ×1.66. (`06` phase 1f.)
7. Post D at one real junction is 600 ns–1.6 µs; when two junctions overlap,
   the second waits the first's post C. (`06` phases 2, 3.)
8. E-04's figures reach a requester from memory, not from the record.
   (`coverage.md:21`, `decisions.md:8081-8086`.)
9. `Peers::waiting` is in memory and empty after a restart; the first pass
   rewrites every queued job's reason. (`decisions.md:8083-8086`.)
10. The hub has no signal handling. (`03b §5.2`; no `tokio::signal` in
    `herd-hub/src`.)
11. `RECORD_VERSION` and `PROTOCOL_VERSION` are checked with `!=`
    (`store.rs:1143`, `:2297`, `server.rs:2141`); the additive rule is at
    `job.rs:44-51`; no migration code exists in the tree.
12. `durable.rs` is the one writer (`durable.rs:1-12`), and `append_bytes`
    (`:80-83`) is the existing precedent for a visible-but-unsynced write.
13. Nothing removes a stale temporary (`durable.rs:235-242`); nothing removes
    a job directory (`store.rs:1981`), so `T` is monotone.
14. One unwritable record aborts the rest of the pass
    (`server.rs:3438-3441`).

**H — a hypothesis, with what would confirm it.**

1. *An unsynced `record_waiting` costs 50–150 µs, so a pass at `N` = 1 000
   falls from 9.633 s to 50–150 ms.* **Confirmed by** S0: `06`'s post-C bench
   against a `write_file_unsynced` variant, with the `strace` sync count at
   zero.
2. *The decisive tier's burst is small, because placements are bounded by
   free seats.* **Refuted or confirmed by** a `06` cell whose corpus is all
   `Impossible`: `end_the_job` for every job in one pass is unbounded and
   unmeasured.
3. *A guard counting store calls by thread would catch a reactor-path read
   like `server.rs:4384`, where a write counter would not.* **Confirmed by**
   prototyping it; whether it can be done without threading a context through
   the `JobStore` trait is not designed here.
4. *Component 4's budget is enough rather than a queue that never drains,
   because C makes the per-job cost ~100× smaller.* **Confirmed by** V2 run
   at a sustained cause-change rate, not a single burst.

**I — could not be decided here.**

1. The deployment's device, filesystem and `T`. Invariant 9 puts them with
   the consumer; every absolute figure in this piece is one workstation's.
2. How often junctions overlap in service (`06` `I`1), which sets what V3 is
   worth.
3. Whether the restart burst outlasts the runners' 60 s limit today
   (`03b §5.2`). V7 is the test; nothing has run it.
4. What `matcher::place` costs inside post E, against the hub's own
   `println!` per queued job (`06` `H`4). Small either way, and it is the one
   term component 4's budget would have to account for if post C stops
   dominating.
