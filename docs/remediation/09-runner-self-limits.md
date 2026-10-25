# 09 — The runner holds its own limits

**Date:** 2026-10-25
**Base:** `origin/main` at `f507ab6`.
**Branch:** `runner-holds-its-own-limits`.
**Scope:** C5. Invariant 2's first half — *a runner never destabilizes its
host* — had no defence on the runner. This piece gives it one, says how a
refusal is spoken without touching the protocol, and closes the `outputs/`
leak, which is the same question asked about disk.
**Scope, as it now stands.** C5 shipped in two halves. The first gave the
runner a ceiling of its own and a way to refuse past it; the second, on
Alexa's decision **D-C of 2026-10-25**, has the runner **announce** that
ceiling so the hub stops placing work the runner is certain to hand back.

**What this is not:** no version moved. `PROTOCOL_VERSION` stays 2 and
`RECORD_VERSION` stays 1, no frame variant was added and **no
`JobEventKind` variant was added** — the wait a capped runner causes is said
with the `waiting` line that already existed. The one protocol surface that
changed is one `#[serde(default)]` field on `ToHub::Hello`, which is
additive under the repository's own rule (`docs/decisions.md:7293`,
`herd-hub/src/job.rs:44-51`).

**§0.5 corrects this document.** Its first version, written before D-C,
said "nothing on the hub was changed". That is no longer true: §7 is the
hub half.

**Evidence standard.** Every claim carries **F** (measured here, or read at
a named `file:line` on this branch), **H** (hypothesis, with what would
settle it) or **I** (unknown).

---

## Release notes — the three things that must ship with this version

Not bullets in a section somebody reads afterwards. These two are the whole
of what this change does to a fleet that is not ours, and `herd-release`
copies them into the release body verbatim.

### RN-1. Upgrade the **hub** before the runners.

A runner on this version announces a ceiling; a hub on this version honours
it and holds the surplus in the queue. A hub on **`v0.3.7`** — the version
in the field — cannot read the announcement and goes on sending, and the
runner now refuses.

**What a refusal costs changed on 2026-10-25, and the order still matters.**
Since §8 a refusal at a machine's own ceiling spends none of that job's two
goes: it has a budget of its own, eight, and a job that reaches it ends
naming the budget and the machines. So a runner-first fleet no longer
*loses* jobs to the order — the sentence this note carried before, that a
job refused twice ends with no verdict, is no longer true of this version.
What a runner-first fleet pays instead is **churn**: a hub that cannot read
the announcement goes on offering work the runner will hand straight back,
so every surplus job makes a round trip, an attempt record and a journal
line per offer, and a queue that would have waited quietly instead cycles
until the machine has room. On `v0.3.7` that cost was zero, because the
runner had no ceiling: it ran everything, which is what invariant 2 is
about.

Hub-first costs nothing: a hub on this version reads no `jobs_at_once` from
a `v0.3.7` runner's hello, which means *"it did not say"*, and places on it
exactly as it does today.

**Nothing in Herd detects or enforces this order.** `Welcome` carries no
echo of the field and the protocol version deliberately does not move, so a
fleet upgraded in the wrong order gets no signal beyond those refusals.
§7.6 has the detail and §8 has what they now cost.
### RN-2. A runner started **without `--host-memory` now takes one job at a time.**

`herd-runner/src/main.rs` fills in `host.memory_mib` only when
`--host-memory` is given or an enclosing cgroup limit can be read. With no
figure, `confine::jobs_at_once` returns
`JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY` = **1**, and both the runner and
the hub hold that machine to one job at a time, for ever.

That is correct and deliberate — D23: an absent figure is absent, never a
figure invented from the machine, and the only count certainly safe on a
host of unknown size is one. It is also a **fleet-wide throughput cliff
caused by a missing flag**, and before this version such a runner took
memory-silent jobs without bound.

The runner says so at startup, by name:

```
this machine does not say how much memory it lends, so how many jobs of
2 GiB fit on it cannot be derived: this runner holds at most 1 job(s) at
once until --host-memory says otherwise
```

**Whether any runner in the first consumer's fleet starts without that flag
is unknown to us and stays unknown (invariant 9, D161).** Nobody here logs
into their machines to find out. That is exactly why this is a release note
and not a footnote: the only person who can check is the one running the
fleet, and they can only check if they are told.

### RN-3. A hub rolled **back** past this version cannot read a record that ends `refused_as_full`.

A job that every machine handed back because each was already as full as the
ceiling it announced now ends `JobOutcome::RefusedAsFull` rather than `lost`
(§8.3, item 3). That is a new variant of a tagged enum, which is additive by
the type's own rule and moves neither `PROTOCOL_VERSION` nor
`RECORD_VERSION` — D166 added `SetupFailed` to the same enum two days ago on
exactly those terms.

What it is not is free in the other direction. Neither `JobOutcome` nor
`JobState` carries `#[serde(other)]`, deliberately: a state a hub does not
know must be *a read error naming it* rather than a job quietly read as
something else. So a hub **rolled back** to a build without this variant,
on a state root that holds such a job, refuses to read that job's record and
says which word it did not know. The records of jobs that ended any other
way are untouched, and this hub reads everything an older hub wrote.

An older `herdctl` following such a job ends its run with no verdict for it,
which is D124's deliberate behaviour for a frame it cannot read.

**What to do about it:** nothing, if the rollback window matters more than
the ending, in which case the figure to set is
`--ceiling-refusals-allowed` high enough that no job reaches the bound
during it. Otherwise it is a documented one-way step, like every other new
ending this enum has taken.

---

## 0. The six things that matter

1. **The ceiling is derived from what the machine itself declared.** F. A
   runner holds at most `(memory it lends − floor it keeps) / cap it puts on
   every job` jobs at once, never fewer than one. At the shipped 2 GiB a
   job: 12 on the 31 GiB server, 7 on the 17 GiB PC, 1 on a machine that
   will not say how big it is. §1.
2. **A refusal is `ToHub::AttemptAbandoned`, which already existed for
   exactly this.** F. Its own doc comment says *"a runner handed a job while
   it is draining abandons it before it starts"*
   (`herd-proto/src/lib.rs:1525-1536`). Nothing new on any enum, so nothing
   is caught by a `#[serde(other)]` arm and `JobEventKind` is not touched.
   §2.
3. **The job is not lost.** F. The hub requeues an abandoned attempt
   (`herd-hub/src/server.rs:3934` → `give_the_job_another_go`, `:5181`),
   bounded by `attempts_allowed`, and `herd-hub/tests/link.rs:2086-2113`
   already proves the job comes back out as attempt 2. §2.2.
4. **Measured, before and after.** F. 50 assignments from a hub that is not
   one: 50 workspaces and 50 on the health report before, **8 and 8** after,
   with 42 attempts handed back. 250 assignments: 250 and 250 before, **8
   and 8** after, 242 handed back. §4.
5. **The `outputs/` leak was still there at `f507ab6`, and is closed.** F.
   A spool left by a `SIGKILL`ed runner could never be offered by any later
   runner and nothing removed it. The startup reaper now reclaims it and
   names it with its size. §3.

6. **And the hub now honours what the runner announced.** F. The runner
   declares its ceiling in `ToHub::Hello`, the hub holds every placement
   against it, and the surplus **waits** instead of being sent and handed
   back. Measured against the real hub: eight memory-silent jobs on a runner
   that holds two spent **12 attempts on the ceiling before and 0 after**,
   and **six of the eight ended `lost` with no verdict at all** before,
   against eight of eight exiting 0 after. §7.

---

## 0.5 What this document said before, and no longer says

The version of this page committed at `aa4699a` ended its header with
*"and nothing on the hub was changed"*, and §5 named the announcement as
**the** fix, as Alexa's call, and as deliberately not done. Alexa made that
call on 2026-10-25 (D-C):

> D-C : oui. `#[serde(default)] jobs_at_once: Option<u32>` sur Hello.
> Absent = « n'a pas dit ». Aucune version ne bouge. C5 peut livrer.

So the sentence is withdrawn. **The hub was changed**, in two files and in
one place that decides anything: `herd-hub/src/matcher.rs`'s `admission`.
§5 is kept as it was written, because what it costs is still what it costs
when the hub is older than this change — that is §7.6 — and §7 is the new
half. §§1-4 and §6 are unchanged and still hold; §6 gains one paragraph.

---

## 1. The limit, and where it comes from

### 1.1 What was wrong — F

`serve_hub` spawned a task for every `AssignJob` with no condition at all,
and `take_job` refused for `draining` and for the owner's switch and for
nothing else. Audit 04b measured the consequence: 250 of 250 admitted, at
both figures, with no refusal at either (§2.2 of
`04b-integrity-and-containment.md`).

Two facts make this worse than "the hub will not do that".

* **F.** The hub's own memory admission charges a job only what it
  *declared* (`herd-hub/src/matcher.rs:583-645`, `record.memory_need_mib`).
  A job that declares no memory need costs the seat nothing, so a queue of
  such jobs is placed on one runner without any bound at all. This is not a
  hypothetical faulty hub; it is the hub as written.
* **F.** The runner caps every job at the same kernel memory ceiling
  whatever the job declared (`Confinement::memory_max`, one figure, carried
  into every job's cgroup at `herd-runner/src/confine.rs:180-189`). So N
  jobs permit N times that ceiling, with nothing bounding the product.

### 1.2 The rule — F

`herd_runner::jobs_at_once(host, memory_max)`
(`herd-runner/src/confine.rs`) returns the count and the sentence that
explains it:

```
most = max(1, (memory_mib − floor_mib) / (memory_max / MiB))
```

* The floor is the host's own when it names one, and
  `herd_proto::default_memory_floor_mib` when it does not — the same default
  the binary fills in before it dials (`herd-runner/src/main.rs:595`), so
  **the figure a runner enforces is the figure it announced**.
* A host that will not say how much memory it lends holds
  `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY`, which is **one**. D23's rule says
  an unreadable figure is not read as zero; it is not read as plenty either.
* Never zero. `Confined::prepare` already refuses a memory cap that would
  refuse every job, on the ground that invariant 2 asks for a cap and not
  for a wall; a count of zero is the same wall one level up.

The figures this gives on the two machines the first consumer actually has,
at the shipped 2 GiB per job: **12** on the 31 GiB server keeping 6 GiB, and
**7** on the 17 GiB PC keeping 3 GiB. Asserted in
`herd-runner/src/confine.rs`'s own tests.

**Why the worst case and not the likely one.** The cap is what a job is
*permitted*, not what it takes. A bound set on what jobs usually take is a
bound that is not there on the day they do not, which is the only day it
matters.

**Why the runner cannot simply mirror the hub. F.** `AssignJob`
(`herd-proto/src/lib.rs`) carries a command, a declaration and a timeout. It
carries no declared memory need, so the runner has nothing to reproduce the
hub's arithmetic with. The local bound is a backstop expressed in the one
currency the runner does control.

### 1.3 Where it is held — F

In `Session::take_job`, under the same lock that writes the job down. Two
assignments arriving together cannot both find room for the last place, and
the reason comes back from inside the lock as a `NotTaken` rather than being
read off the session afterwards — the previous code asked `session.draining()`
*after* the refusal, which with three reasons would have reported the wrong
one whenever the state moved in between.

**What is counted is jobs that are still running** (`end.is_none()`), not
jobs still on the books. A finished job whose verdict the hub has not
acknowledged holds a record and some unsent bytes; it holds no process and
no memory ceiling, and counting it would let a slow hub shut a working
machine down. Asserted.

---

## 2. How a refusal is said

### 2.1 With the frame that was already there — F

`ToHub::AttemptAbandoned { job_id, attempt, reason }`. Its doc comment, on
`main` before this work, already describes this case: *"Not an outcome: the
job did not end, this attempt did. … a runner handed a job while it is
draining abandons it before it starts. What becomes of the job — one more
attempt, or an end — is the hub's policy and never the runner's"*
(`herd-proto/src/lib.rs:1525-1536`).

So nothing was added anywhere. This matters beyond tidiness: a new variant
on `FromHub` or `ToHub` would be absorbed by their `#[serde(other)] Unknown`
arm, but **`JobEventKind` (`herd-hub/src/job.rs`) is `#[serde(tag =
"event")]` with no such arm**, and a new event kind would make
`events.jsonl` unreadable to an earlier hub. That road was not taken.

The sentence carries the arithmetic, because a hub's operator has to be able
to tell a full machine from a broken one:

```
the runner is already running 8 job(s) and never started this job: this
machine lends 4 GiB and keeps 2 GiB as its own floor, and caps every job at
256 MiB, so this runner holds at most 8 job(s) at once
```

*"this runner holds at most"* is `herd_runner::A_RUNNERS_OWN_CEILING`, and
it is in the sentence rather than beside it because this is how a reader —
and `herd-hub/tests/announced_job_ceiling.rs`'s oracle — knows the words
came from the runner and not from the hub, which has a ceiling sentence of
its own (§7.3, §7.5).

### 2.2 What becomes of the job — F

The hub requeues it. `ToHub::AttemptAbandoned` reaches
`herd-hub/src/server.rs:3934`, which calls `give_the_job_another_go`
(`:5181`): the attempt is ended on the record, the job goes back to
`Queued`, every follower is told, and a dispatch is triggered — unless
`goes_a_runner_took() + 1 >= attempts_allowed`, in which case the job ends.
`herd-hub/tests/link.rs:2086-2113` already asserts the whole of that for the
drain case, which is now literally the same path.

So: **a refused job is never lost and never silently dropped, and it costs
one of that job's goes.** That last clause is the price, and §5 is about it.

---

## 3. The `outputs/` leak

**Still true at `f507ab6`, verified. F.**

* A streamed collection is moved out of the workspace to
  `<data root>/outputs/<job>-<attempt>/` (`herd-runner/src/outputs.rs:136`)
  and removed only on `JobFilesAck` (`herd-runner/src/lib.rs`,
  `files_acknowledged`).
* The hold is **in this process's memory**: the only place a spool is ever
  registered is the end of the job that produced it (the single
  `collections.insert` in `Session::finish`). Nothing anywhere reads
  `outputs/` back.
* `reclaim_a_past_life` (`herd-runner/src/reclaim.rs`) swept orphaned
  processes, `jobs/` and the cgroups. It did **not** sweep `outputs/`.

Put together: after a `SIGKILL`, those bytes can never be offered again —
a hub asking for them is answered *"this runner holds no collection for
it"* (`herd-runner/src/lib.rs`, the `other =>` arm of `files_held`) — no
acknowledgement can ever free them, and nothing removed them. On the first
consumer's own machine that is a directory that only grows. The audit called
it the plainest breach of invariant 2 and that reading holds.

**What is done.** The startup reaper sweeps `outputs/` with the same
`reclaim_directory` it uses for workspaces, and names each one with its size:

```
reclaimed 4 KiB of output left in <root>/outputs/j0000000001-1 by a
previous life: no runner can offer a spool it did not make, so these bytes
could never have reached the hub
```

Asserted in `reclaim::tests`, by
`the_reaper_reclaims_an_output_spool_a_previous_life_left_behind`.

**What is deliberately not done, and it is the better fix. H.** The held
verdict in `held-results/` carries the whole manifest, so a restarted runner
*could* re-adopt the spool and deliver the bytes after all. That is a
feature, not a containment fix, and nobody has asked for it. What would
settle whether it is worth building: how often a runner is killed with a
streamed collection outstanding on the consumer's instance — which this
work did not look at (D161).

---

## 4. Measured, before and after

Same bench, `herd-hub/tests/fake_hub_flood.rs`: a hand-written WebSocket
server welcomes a real `herd_runner::serve_once` and hands it 50, then 250,
`AssignJob` frames as fast as the socket takes them. Jobs are
`/bin/sh -c 'sleep 20'` with `timeout_s = 25`. Loopback, temporary data
root, load guard at 8.

**One change to the bench beyond the assertions: the runner's host size is
now declared, not read.** The ceiling is derived from it, so a bench that
read the real machine would assert a different number on every host. It
announces 4 GiB lent, a 2 GiB floor and a 256 MiB per-job cap: **8 jobs**.

| | 50 before | 50 after | 250 before | 250 after |
|---|---|---|---|---|
| job workspaces (peak) | 50 | **8 (8)** | 250 | **8 (8)** |
| jobs on its health report | 50 | **8** | 250 | **8** |
| attempts handed back | 0 | **42** | 0 | **242** |
| control probe | 360 µs | 218 µs | 237 µs | 1.16 ms |
| host 1-min load before → during | 2.61 → 2.40 | 0.68 → 0.71 | 2.03 → 2.02 | 0.68 → 0.71 |

The "before" column is audit 04b §2.2's first run, not a re-run: the bench
is the same file and re-running the old behaviour would mean reverting the
fix. The "after" column is this branch, on this host, today.

The two tests no longer begin `measure_`: the behaviour is decided now, so
they assert. The load guard, which used to print a plain line because there
was no assertion to skip, now emits a `HERD-SKIPPED` (D27). **That skip is
conditional on the host's load and is not part of the suite's standing skip
reference** — on a quiet host it never appears, and the reference stayed at
12 (10 cgroup-v2 delegation, 2 ZFS ceiling) through every run of this work.

---

## 5. What this costs, and what it does not settle

**F — the hub can over-place, and each refusal spends a go.** The hub has no
per-runner job ceiling for jobs that declare no memory need (§1.1), so it
can legitimately send more than a runner now accepts.
`give_the_job_another_go` counts each abandon against `attempts_allowed`, so
a job refused repeatedly ends without a verdict instead of waiting. This is
not new behaviour invented here — a drain has always cost a go the same way
— but it is newly reachable.

**The fix, and it is Alexa's call.** The runner should **announce** its
ceiling so the hub stops over-placing: one `#[serde(default)]` field on
`Hello`, additive under the rule at `docs/decisions.md:7293`, moving neither
`PROTOCOL_VERSION` nor `RECORD_VERSION`, with absent meaning "did not say"
and never "unlimited". That is a protocol surface change and was therefore
not done here.

**F — one test harness had to stop reading the host.** `with_hub_and_runner`
(`herd-hub/tests/support/mod.rs`) built its runner from `Config::default()`,
whose host size is the real machine. With a ceiling that is a finding, not a
convenience: `no_job_is_claimed_twice_when_passes_overlap` submits twelve
jobs at once and asserts exactly one attempt each, so on this 32 GiB host it
would have had a ceiling of 12 and on a 16 GiB CI box a ceiling of 6 — the
same test asserting two different things. The harness now declares 64 GiB
lent with an 8 GiB floor. The ceiling itself is asserted where it belongs,
in the bench and in the runner's unit tests.

**I — what the consumer's hub actually does under load.** Whether their
fleet ever places more on one runner than its derived ceiling is a question
about their queue and their declarations, and nothing on their instance was
read (D161). §6 says what they should look at.

**I — the damage a full flood would really do.** Still unmeasured, and
deliberately: the jobs here are `sleep`, so what is measured is the number
admitted and not work. 250 jobs that each compile is a measurement that must
not be taken on a shared host.

---

## 6. For chatOnS

Nothing they have to change, and one thing they should read.

* **Their runners now refuse work past a ceiling they can compute
  themselves**: `(--host-memory − --host-memory-floor) / --job-memory-max`,
  at least 1. At the defaults that is 12 on a 31 GiB machine and 7 on a
  17 GiB one. If a runner of theirs announces no `--host-memory`, it will
  hold **one job at a time** — the startup line says so by name, and adding
  `--host-memory` is the whole fix. **This is RN-2 at the top of this page,
  and it ships as a release note**, because whether any of their runners
  starts without the flag is unknown to us and must stay so (D161).
* **A refused job is requeued, not failed** — but a job refused
  `attempts_allowed` times ends without a verdict. If they see jobs ending
  with no verdict and the reason *"the runner is already running N job(s)"*,
  their hub is placing more on one machine than that machine accepts, and
  the answer is a larger `--host-memory`, a smaller `--job-memory-max`, or
  more machines. Not a Herd bug: a fleet whose declared capacity is smaller
  than its queue.
* **The startup line to grep for** is the one beginning *"this machine lends
  … and caps every job at …, so this runner holds at most N job(s) at
  once"*. The clause *"this runner holds at most"* is the runner's own and
  appears in no sentence the hub writes, so it is also what tells a refusal
  the runner made apart from a job the hub is merely holding back (which
  says *"it said it holds at most N at once"* and costs the job nothing).
* **Disk**: a runner restarted after a kill now deletes any leftover
  `<data root>/outputs/<job>-<attempt>/` and says how much went. Those bytes
  were never recoverable; if anybody was harvesting that directory by hand,
  that stops working and they should say so.
* **And once both sides are on this version, that paragraph stops
  applying.** The hub now honours the ceiling a runner announces, so a
  surplus job **waits in the queue** instead of being sent and refused. A
  fleet still sees the refusal sentence when a runner is newer than its hub
  — §7.6 — and there it is a version gap, not a capacity problem.

---

## 7. The hub half (D-NEXT, on Alexa's D-C of 2026-10-25)

### 7.1 What the runner says — F

One field on `ToHub::Hello`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
jobs_at_once: Option<u32>,
```

`None` means **the peer did not say**, and nothing else: never unlimited,
never zero, never a figure derived from `host`. The register is
`health_every_ms`'s, and for the same reason — the mistake it prevents needs
both sides at once. The runner derives a ceiling and refuses past it; the
hub charges a job only what it declared and so has no bound on
memory-silent jobs at all (§1.1). Each is self-consistent; together they
spend one of every surplus job's goes on a refusal that was certain before
it was sent.

**The figure announced is the figure enforced, and that is structural, not a
convention. F.** `serve_once` computes `jobs_at_once(&config.host,
confined.caps().memory_max)` **once**, before the handshake, sends that
value in the hello and hands the same `Arc` to `serve_hub`, which is where
every assignment is held against it. There is no second derivation that
could drift. Asserted from the outside in
`herd-hub/tests/fake_hub_flood.rs`, which reads the hello and checks the
figure is the ceiling the same runner then enforces.

**Neither version moves. F.** `PROTOCOL_VERSION` is 2
(`herd-proto/src/lib.rs:113`) and `RECORD_VERSION` is 1
(`herd-hub/src/job.rs:52`), asserted by
`announced_job_ceiling::neither_version_number_moves`. The field is
`#[serde(default)]` and means, when absent, exactly what its default says,
which is the one shape the rule at `docs/decisions.md:7293` allows to leave
both alone.

### 7.2 Where the hub counts, and why there — F

In `herd-hub/src/matcher.rs`, at the top of `admission`, against
`seat.reserved.jobs`.

That is the right place for three reasons, and each of them is a defect
avoided:

* **`reserved.jobs` is already the count.** It is the number of attempts out
  on that machine that have not ended, taken off the records by `out_there`
  — the single filter the labels (`holdings`) and the memory and tmpfs
  (`reserved`) are all taken off. A second counter kept beside it would be a
  second answer to one question, and the first job counted by one and not
  the other is a machine the hub reads as having room on top of work that is
  running.
* **It is derived, never accumulated (D79).** There is no table to release,
  so a job that ends any of the eight ways gives its place back by ending,
  and a hub that restarts recomputes the count from the records instead of
  rebuilding it.
* **It is taken again *inside* the pass.** `claim` calls
  `seat.reserved.take(...)`, so two jobs offered in one pass contend for the
  last place rather than both finding it free. Asserted:
  `a_runner_at_the_ceiling_it_announced_takes_nothing_more_and_says_so`
  fills a ceiling of three and refuses the fourth in the same pass.

**Before the memory arithmetic, not after.** It is the blunter of the two,
and the sentence a waiting job reads should be the one that is actually
holding it up: a job told how many mebibytes are free on a machine that will
not take it anyway has been told the wrong thing.

**A bound beside the memory admission, never instead of it. F.** The two
answer different questions and neither subsumes the other — admission weighs
what a job declared, so a queue of jobs declaring nothing is this field's
alone to bound; and this field knows nothing about size, so a job larger
than the machine is still admission's to refuse, and still refused as
*impossible there* rather than as a wait. Asserted in
`the_ceiling_and_the_memory_admission_are_two_bounds_and_either_may_bite`,
in both directions and in the impossible case.

**It counts every job out there, readings included. F.** D159 keeps a
requirement reading out of the slot algebra and D169 caps readings
separately; neither makes a reading weightless on the machine it runs on,
and the runner's own ceiling counts every job it is running (`end.is_none()`,
§1.3). A hub counting a smaller set than the runner would read room that is
not there. Asserted in `the_ceiling_counts_readings_too_because_the_runner_s_does`.

### 7.3 What a queued job is told — F

`Admission::ShortNow`, which is `Verdict::Later`, which is
`Placement::Wait`, which the hub writes as a **`waiting` line** — a
`JobEventKind` that already existed. **No variant was added to that enum**,
and that was a stop condition on this work: it is `#[serde(tag = "event")]`
with no `#[serde(other)]` arm, so a new kind would make `events.jsonl`
unreadable to a hub rolled back, which is worse than a lost field.
`announced_job_ceiling` asserts that every line written in its run is one of
the eight kinds that already existed.

The whole sentence, copied from that test's own output on this branch
today, wrapped here and otherwise verbatim:

```
queued 1st of 1 at priority step, rising to merger in 30m; it is waiting
because it is waiting for capped-runner: on capped-runner, 2 job(s)
already out there and it said it holds at most 2 at once, so it takes
nothing more until one of them ends: 2 jobs running there hold 0 MiB of
memory and 0 MiB of tmpfs; no other eligible runner is free either. It
moves to another eligible runner only when capped-runner stops being
available, or after 3h in the queue (--stickiness-limit); it has waited
under a minute; nothing is ahead of it
```

The clause this change contributes is the middle one; the queue's standing,
the stickiness limit and *nothing is ahead of it* were already there and are
what make it readable. Three things in that clause are deliberate: it names
the machine, it carries **both** figures — what is out there and what the
machine said it holds — and it says **what ends the wait**.

`0 MiB of memory` is not a bug. It is the clause that makes the defect
legible: those two jobs really did declare nothing, which is exactly why the
memory admission could never have been what stopped the third, and why this
field had to exist.

### 7.4 Can a job sit for ever behind a cap? — F

No, and three separate things stop it:

1. **A place comes back when a job ends.** Every job runs under a time cap
   (invariant 2), so a machine at its ceiling empties whether or not its
   jobs behave, and ending a job triggers a dispatch pass.
2. **`ShortNow` is bounded by the stickiness limit.** A job held by a
   ceiling sticks to that machine — correctly, since a job ending there is
   what frees the room — but only until `--stickiness-limit`, after which it
   takes any other eligible runner that is free. Asserted in
   `a_job_held_by_a_ceiling_waits_for_that_machine_and_then_moves`.
3. **A ceiling of zero is not a wait at all.** Our own runner never
   announces zero (`jobs_at_once` is floored at one, §1.2), but a peer that
   is not ours could, and `ShortNow` on such a machine would be a wait
   nothing on that machine could ever end. It is answered as `NotHere`
   instead: nothing is placed there and nothing waits for it. Asserted in
   `a_ceiling_of_zero_is_a_machine_nothing_waits_for`.

And a job's own deadline still cancels it while it waits (E-09), which is
the bound D88 leaves on a job no connected runner can take.

### 7.5 Measured, on the real hub — F

`herd-hub/tests/announced_job_ceiling.rs`. A real hub, a real runner
announcing a ceiling of **2** (4 GiB lent, 2 GiB floor, 1 GiB a job), and
**8 memory-silent jobs** submitted at once — memory-silent because
`herdctl::Declared::default()` declares nothing, so `admission` costs each
of them at zero and the memory accounting cannot be what bounds them. Eight
*distinct* commands: two submissions of one command join one job (E-05), and
a queue of identical sleeps is one job wearing eight requesters, which paces
itself perfectly and would prove nothing.

| | before the hub half (5 runs) | after |
|---|---|---|
| peak attempts out on the runner at once | 6 or 7 | **2** |
| attempts spent **because of the ceiling** | 12 in 4 runs, 9 in 1 | **0** |
| attempts spent for anything else | 0 | **0** |
| attempts recorded in all, for 8 jobs | 14 in 4 runs; **I** in the fifth | **8** |
| jobs that reached their own verdict | 2 of 8 in 4 runs, 4 of 8 in 1 | **8 of 8** |
| jobs that ended `lost`, with no verdict | **6 of 8 in 4 runs, 4 of 8 in 1** | **0** |

**The "before" column is a spread, not a sample. F.** It was measured by
running the same test with the gate in `admission` neutered
(`seat.jobs_at_once` read as `None`), **five times**, in the contradictory
review of PR #82 — four runs gave 6 of 8 lost and 12 attempts spent, one
gave 4 and 9, and the peak was 6 in two runs and 7 in three. An earlier
version of this table printed `7`, `12` and `6 of 8` as if each were the
figure; they are the modal figures of a timing-dependent race between the
hub's dispatch pass and the runner's refusals, and a sixth run could
contradict any single one of them. **The finding does not rest on the
spread:** in every one of the five runs a majority of perfectly runnable
jobs ended with no verdict at all, and that is what this row says.

The "after" column is not timing-dependent and was measured on this branch:
peak 2, 8 attempts for 8 jobs, every job exited 0. The one row the five runs
did not each record separately is the total attempt count, marked **I**
above: 14 was read in the four runs that spent 12, and the fifth run's total
was not read off its records.

**The last row is the damage, and it is worse than the attempt count makes
it look.** `attempts_allowed` is 2. A job refused twice has spent both its
goes on refusals that were certain before they were sent, and
`give_the_job_another_go` then ends it — so a majority of eight perfectly
runnable jobs (six in four of the five runs, four in the fifth) ended with
no verdict at all, each carrying this reason:

```
the runner is already running 2 job(s) and never started this job: this
machine lends 4 GiB and keeps 2 GiB as its own floor, and caps every job
at 1 GiB, so this runner holds at most 2 job(s) at once; this was attempt
2 of 2 at a runner that went away, so the job is not attempted again
(E-21)
```

That sentence is §5's warning happening. It is also exactly the sentence §6
tells the first consumer to grep for — and after this change it is a sign of
a version gap (§7.6), not of a fleet short of capacity.

**Attribution, not a total. F.** Alexa's oracle is *"essais dépensés pour
cause de plafond runner = 0, autres essais inchangés"*, and a count of
everything cannot say that. The two are told apart by the sentence: a
refusal for the ceiling carries the runner's own words — built by
`jobs_at_once`, carried in `AttemptAbandoned`, written verbatim into the
record's `attempt_ended` line by `end_attempt` — and a timeout, a cancel or
a lost runner carries one of the hub's. `ceiling_refusals` in that file is
the whole of the distinction, and the "attempts spent for anything else"
row is what it leaves behind.

**And the two sentences are now told apart with certainty. F.** The runner's
carries `herd_runner::A_RUNNERS_OWN_CEILING` — *"this runner holds at most
N job(s) at once"* — and the hub's own ceiling sentence, the one it writes
on a `waiting` line, carries
`herd_hub::matcher::A_CEILING_THE_RUNNER_ANNOUNCED` — *"it said it holds at
most N at once"*. Both used to contain the words *"holds at most"*, and the
test matched that as a literal of its own: it could not say which side had
spoken — which is precisely the attribution claimed here — and a wording
edit on either side would have left it matching a phrase nothing says, so
the oracle would have read green on a fleet losing jobs. The test now
imports both constants and asserts, against the sentences as they are
really built, that neither contains the other's clause
(`the_runners_ceiling_sentence_is_not_the_hubs`).

**And the silent runner is unchanged. F.** Every seat the matcher's own
tests build announces nothing, deliberately, so the whole existing suite is
the assertion that a runner which did not say is placed on exactly as it was
before this field existed;
`a_runner_that_announced_no_ceiling_is_placed_on_exactly_as_it_was` states
it directly, placing forty memory-silent jobs on one machine and checking
that nothing stops them.

### 7.6 The rollback case, which is the cost that stays — F

**`fake_hub_flood.rs` still shows 8 held and 242 handed back, and it should
for ever.** Its "hub" is a hand-written WebSocket server that floods a real
runner on purpose; it reads the announcement — the bench now asserts that it
arrived and that it is the figure the runner enforces — and then ignores it,
which is precisely a hub older than this change, or a hub that is not one.
The
runner protects itself regardless, which is invariant 2 and the premise of
C5's first half.

**Upgrade the hub first. This is a condition, not a preference.**

An earlier version of this section said a runner-first fleet "pays exactly
what it paid before this change". That is **false**, and the version it is
false against is the one in the field. Compared with `v0.3.7`:

* On `v0.3.7` a runner had **no ceiling at all**. A hub placing a queue of
  memory-silent jobs on one machine cost **zero** refusals — the runner
  accepted every one and ran them all. That was bad for the host and is the
  invariant 2 defect C5 closes, but the jobs produced verdicts.
* A fleet whose runners are on this version while its hub is on `v0.3.7`
  pays refusals it **never paid before**: one `AttemptAbandoned` per
  surplus assignment, on a hub that cannot read the announcement and will
  go on sending them. `attempts_allowed` is 2, so by §7.5's mechanism a job
  refused twice does not wait — it **ends with no verdict**.

So runner-first does not preserve today's cost; it converts "host
overloaded, work done twice" into "jobs lost". Hub-first costs nothing: a
hub on this version placing work on a `v0.3.7` runner reads no `jobs_at_once`
in the hello, which means *"it did not say"*, and places exactly as it does
today.

**And nothing detects the order. F.** `Welcome` carries no echo of the
field, the protocol version deliberately does not move (§7.1), and a runner
cannot tell a hub that honoured its announcement from one that never read
it. A consumer who upgrades runners first gets no signal at all until jobs
start ending `lost` with §7.5's sentence. That is why the order belongs in
the PR body and in whatever goes out with the release, and not only here.

### 7.7 What is still open

**I — whether the consumer's fleet ever hits either bound.** Their queue,
their declarations and their machines were not read (D161). What they should
look at is §6.

**H — the ceiling is derived from the per-job memory cap and nothing else.**
A machine whose real constraint is cores, disk bandwidth or file handles
announces a figure that says nothing about it. That is deliberate — the cap
is the one currency the runner certainly controls — but it means
`jobs_at_once` is a memory ceiling wearing a general name. What would settle
whether a second figure is wanted: a consumer report of a runner saturated
while well inside its announced count.

**H — the hub trusts the figure.** A runner announcing a very large ceiling
is placed on as though it had none, which is the same as today and is
therefore not a regression; a runner announcing a very small one holds its
own fleet back. Neither is checked against `host`, deliberately (§7.1), and
the hub has no reason to prefer its own arithmetic to the machine's. What
would settle it: a fleet where a misconfigured runner's announcement costs
throughput that nobody can see the cause of — which §7.3's sentence is
written to make visible.

---

## 8. What a ceiling refusal costs (D-NEXT, on Alexa's decision of 2026-10-25)

`herd-reviewer`'s contradictory review of PR #82 found one thing it would
not sign: **§1.2, a link outage that turns N running jobs into N jobs ending
`lost`.** This section is what was done about it. Alexa's words, of the same
day:

> §4 : oui, un refus de plafond ne dépense pas de vie (forme de
> `goes_a_runner_took`, additif sur l'enregistrement d'essai, approuvé).
>
> Condition de fusion ajoutée : un test hub réel qui coupe le lien 60 s avec
> un runner à son plafond, 0 job mort, 0 job sans verdict. Il doit être
> rouge sur `fa2ee15` avant le correctif. Rien ne fusionne sans lui.

### 8.1 The chain, re-read line by line on this branch — F

1. A runner at its ceiling N loses its link. Its jobs keep running
   (`herd-runner/src/lib.rs`, the comment at the end of `serve_hub`: *"the
   jobs still running hold senders of their own … They report on the next
   link"*), so its own count is still N.
2. After `lost_after` the hub's watchdog ends all N attempts and requeues
   each job as attempt 2 of 2. Its count for that machine is now **0**.
3. The runner redials. The hub calls `dispatch_queued_jobs` **on the hello,
   before reading one frame from it**, and nothing avoids the machine a job
   has just come off. All N go back.
4. The runner is still holding the originals, refuses every one, and each
   refusal was the job's last go. **N jobs end `lost` with no verdict.**

### 8.2 The test, which is the condition of merge — F

`herd-hub/tests/announced_job_ceiling.rs`,
`a_link_cut_while_the_runner_is_full_loses_no_job`. A real hub, a real
runner announcing [`CEILING`] = 2 and running two jobs, a TCP relay between
them that the test **cuts**: the sockets are shut down under both peers and
every redial is dropped for the length of the outage, so both processes stay
alive and only the wire goes. The oracle is Alexa's two figures and nothing
else: no job dead, no job without a verdict.

| | on `fa2ee15`, before the fix | after |
|---|---|---|
| suite timings (`--lost-after 3s`, 4 s outage) | **2 of 2 `lost`**, each *"this was attempt 2 of 2"* | 2 of 2 exited 0, ten runs in a row |
| the real 60 s (`--lost-after 60s`, 65 s outage) | **2 of 2 `lost`** | 2 of 2 exited 0 (140 s) |

The suite keeps the short one and drives the same watchdog through
`--lost-after`, which `herd-hub/src/main.rs` already exposes. **The 60 s
shape is in the suite too, behind `#[ignore]`** —
`a_link_cut_while_the_runner_is_full_loses_no_job_at_the_deployed_sixty_seconds`,
found by `cargo test -- --ignored`, about 150 s of wall clock and nearly all
of it waiting. Both tests are one body with one oracle and differ only in
their figures, which is the point: the oracle does not depend on the clock.
A test that was never red proves nothing, so both reds were taken on the
branch as it stood.

### 8.3 The change — F

1. **`at_its_ceiling: Option<u32>` on `ToHub::AttemptAbandoned`**,
   `#[serde(default, skip_serializing_if = "Option::is_none")]`: how many
   jobs the runner was holding, when **its own count of what it holds** is
   why it refused. `None` on every other abandonment, and `None` is what an
   older runner's frame decodes to — which is what every abandonment meant
   before the field existed.
2. **`at_its_ceiling: Option<u32>` on the attempt record**, left off the
   record entirely when absent. `JobRecord::goes_a_runner_took` filters on it
   beside D76's `requeue_code`, so a ceiling refusal spends none of E-21's
   goes. The figure it keeps is the machine's own count, because that is what
   the ending in 3 reports to a program.
3. **A budget of its own**: `CEILING_REFUSALS_ALLOWED` = 8, a `Policy`
   figure. A job handed back an eighth time ends — and **it does not end
   `lost`**. Alexa, 2026-10-25: *"que devient un job au 9e refus ? Verdict
   lisible machine exigé, pas lost."* It ends
   `JobOutcome::RefusedAsFull`, a new variant carrying `refusals`,
   `refusals_allowed` and `runners` — every machine that said it was full,
   each named once, with the count it gave — beside the sentence a person
   reads. `herdctl`'s verdict document reports
   `"cause": "refused_as_full"`, `"exit_code": 125`,
   `"exit_code_from": "herd"`.
   `a_job_refused_past_the_bound_ends_and_names_it` drives it at a budget of
   one and asserts every field, and that **no** job ended `lost`.
4. **The hub remembers the machine's own count** until that runner's next
   health report, and `admission` holds the ceiling against the larger of
   that and `seat.reserved.jobs`. Without this the surplus would be offered
   straight back and the refusals would cycle; with it the job **waits**,
   and the report — which names every attempt the runner holds — ends the
   correction and judges the queue at once.
5. **A runner never holds two attempts of one job.** `take_job` refuses a
   second attempt of a job it is already running, `abandon` drops only the
   entry that belongs to the attempt it is about, and `release` drops only a
   job that is holding an end. This is the review's §1.3 — pre-existing, and
   made reachable by jobs now surviving to a third attempt. Without it the
   first attempt ran untracked, the machine's ceiling was short by one for
   the rest of its life, and the live attempt vanished from the health
   report, where the watchdog then wrote it off for silence.

**Neither number moves.** `PROTOCOL_VERSION` stays 2 and `RECORD_VERSION`
stays 1, asserted by `neither_version_number_moves` and by
`the_record_version_does_not_move`; and
`a_record_crosses_the_field_in_both_directions` writes a record with the new
field, reads it with a struct that does not have it, writes that back and
reads it again — a rollback is one of those directions and an upgrade is the
other. **No `JobEventKind` variant was added.**

**One thing is one-way, and it is RN-3 below.** A new `JobOutcome` variant
is additive by the type's own rule and the precedent is two days old — D166
added `SetupFailed` with neither number moving — but neither `JobOutcome`
nor `JobState` carries `#[serde(other)]`, by design: a state this Herd does
not know is *a read error naming it* rather than a job quietly misread. So a
hub older than this one **will refuse to read a record that ends
`refused_as_full`**, and an older `herdctl` following such a job ends its
run with no verdict (D124). This hub reads everything an older one wrote;
the other direction is the cost, and
`round_trip::a_job_refused_as_full_travels_with_its_figures_and_is_unreadable_to_an_older_peer`
asserts both halves of it.

### 8.4 What is left — H

**The hub still dispatches to a runner before reading one frame from it.**
This change makes that safe — the refusal costs nothing and teaches the hub
at once — and does not make it right. The criterion is on the board as
**ORCH-061**, to be carried into the S3 brief: *the keeper's pass must not
place on a runner whose reconciliation has not arrived.*

**The watchdog acts on an age read before its own pass placed anything.**
`watch_for_a_lost_runner` takes one snapshot of every open attempt's age and
then walks it, and the walk requeues, which dispatches, which can give a job
further down the list a brand-new attempt — judged against the old figure.
**Fixed in its own PR, underneath this one** (Alexa, 2026-10-25, part 2 of
three), by reading the age again inside the loop, after the record. It is
the one tick that follows a link coming back, which is where a fleet has
least to spare. The runner's job map (part 1) is likewise its own PR, so
item 5 above reads the map keyed by job **and** attempt, and its reason is
now the one that survives that key: a workspace belongs to a job, so a
second attempt here would clear the working directory out from under the
first one's process.
