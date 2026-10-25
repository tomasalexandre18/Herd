# 09 — The runner holds its own limits

**Date:** 2026-10-25
**Base:** `origin/main` at `f507ab6`.
**Branch:** `runner-holds-its-own-limits`.
**Scope:** C5. Invariant 2's first half — *a runner never destabilizes its
host* — had no defence on the runner. This piece gives it one, says how a
refusal is spoken without touching the protocol, and closes the `outputs/`
leak, which is the same question asked about disk.
**What this is not:** no protocol change. `PROTOCOL_VERSION` stays 2,
`RECORD_VERSION` stays 1, no frame variant and no `JobEventKind` variant was
added, and nothing on the hub was changed.

**Evidence standard.** Every claim carries **F** (measured here, or read at
a named `file:line` on this branch), **H** (hypothesis, with what would
settle it) or **I** (unknown).

---

## 0. The five things that matter

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
256 MiB, so it holds at most 8 job(s) at once
```

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
  `--host-memory` is the whole fix.
* **A refused job is requeued, not failed** — but a job refused
  `attempts_allowed` times ends without a verdict. If they see jobs ending
  with no verdict and the reason *"the runner is already running N job(s)"*,
  their hub is placing more on one machine than that machine accepts, and
  the answer is a larger `--host-memory`, a smaller `--job-memory-max`, or
  more machines. Not a Herd bug: a fleet whose declared capacity is smaller
  than its queue.
* **The startup line to grep for** is the one beginning *"this machine lends
  … and caps every job at …, so it holds at most N job(s) at once"*.
* **Disk**: a runner restarted after a kill now deletes any leftover
  `<data root>/outputs/<job>-<attempt>/` and says how much went. Those bytes
  were never recoverable; if anybody was harvesting that directory by hand,
  that stops working and they should say so.
