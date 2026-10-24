# 03a — Identity, storage and state safety (audit A)

**Date:** 2026-10-24
**Base read:** `origin/main` at `0a2cf73` (PR #70 merged).
**Scope:** mandate §4.1 — who owns which state, where it persists, what the
job/attempt automaton allows, what each acknowledgement really guarantees,
and where the identity chain from a consumer's request to an artefact can be
confused. Read-only: no binary was built, no test was run, no process on this
host was touched.
**Method:** code reading only. Every **F** below carries a `file:line` on
`0a2cf73`. **H** is a hypothesis the code suggests and no evidence here
settles. **I** is unknown. Where an incident is reported but no runtime
evidence was available to me, the claim stays **H** or **I** — the two
analyses dated 24/10 were treated as leads, never as findings.

---

## 0. Summary of the five things that matter

1. **The durability discipline is real and is one module.** Every hub write
   goes through `herd-hub/src/durable.rs`; a record is replaced by
   write-temp / fsync / rename / fsync-dir, and a job's end is on the disk
   *before* its acknowledgement. (F — §3.1, §3.2.)
2. **Herd is at-least-once execution with at-most-once recording.** Nothing
   in the code produces exactly-once, and nothing claims to. A network
   partition can have two attempts of one job running at once; only one of
   them can ever be written to the record. (F — §3.4.)
3. **The commit link of the identity chain is open-loop.** The commit is
   declared on the way out and is never confirmed on the way back: no frame
   from the runner carries the commit the tree was actually at.
   (F — §4.2.) This is the weakest joint in the chain the consumer reported
   problems with, and D156 (dedup key) did not close it.
4. **Two unbounded leaks on either side of the streamed carriage.** An
   abandoned collection leaves `jobs/<id>/files.part/` on the hub, which the
   sweeper neither counts nor removes (F — §5.3), and
   `<data root>/outputs/<job>-<attempt>/` on the runner, which no reaper
   touches (F — §5.4).
5. **One inline path can turn a short collection into one that reads as
   whole.** `record_outputs` drops a manifest entry whose bytes disagree with
   their digest instead of keeping the entry and naming the absence — the one
   place in the output design that breaks its own "the manifest outlives the
   bytes" rule, and `herdctl` then exits 0. (F — §5.2.)

---

## 1. Who owns which state, who writes it, where it persists

### 1.1 The durable state (hub)

One state root, one hub, enforced by `flock(2)` on `hub.lock`
(`herd-hub/src/store.rs:360-361`, `durable.rs:131-146`). The layout is
documented at `store.rs:1-41`:

| Path | Holds | Written by | Durable? |
|---|---|---|---|
| `store.json` | layout version + state-root identity | `JobDirStore::open_with` (`store.rs:372-406`) | yes (atomic rename) |
| `hub.lock` | which process owns the root | `durable::lock_exclusive` | kernel-held |
| `jobs/<id>/job.json` | the record: fields, state, attempts, manifest | `JobDirStore::write_record` (`store.rs:1182-1191`) | yes |
| `jobs/<id>/events.jsonl` | the history, append-only | `append_event` (`store.rs:1284-1292`) → `durable::append_line`, `sync_data` | yes, per line |
| `jobs/<id>/log` | job output, arrival-ordered | `append_log` (`store.rs:2205-2245`) → `durable::append_bytes` | **no** per chunk; fsynced once at the terminal transition (`store.rs:1349-1353`) |
| `jobs/<id>/log.seq` | chunks + bytes counted | `write_fixed_in_place` (`durable.rs:96-107`) | **no** (page cache only, by design) |
| `jobs/<id>/files/` | whole, digest-verified output files | `record_outputs` (inline) / `write_output_chunk` (streamed) | yes |
| `jobs/<id>/files.part/` | part-written streamed files | `write_output_chunk` (`store.rs:1869`) | no (append, no fsync until rename) |
| `jobs/<id>/files.gone/` | collection being evicted | `sweep_now` (`store.rs:654-670`) | n/a |
| `index/state.json` | derived: which jobs are open | `write_index` (`store.rs:1060-1067`) | yes, rebuildable |
| `readings/answers.json` | `labels_from` cache by (commit, command, args) | `remember_reading` | yes, rebuildable |
| roster file | approved runner names + salted token hashes | `herd-hub/src/roster.rs:258-266` | yes |

**F.** `store.rs:26-27` states the rule and the code keeps it: no module but
`durable` calls `std::fs` for a write. I checked `store.rs`, `roster.rs` and
`server.rs`; the only `std::fs` writes outside `durable` are
`fs::rename`/`fs::remove_file`/`remove_dir_all` *inside* `store.rs`
(`store.rs:1882`, `1893`, `1981`), which are the atomic half of the same
discipline.

**F.** `job.json` is the authority and `events.jsonl` may be one transition
behind it, never ahead: `move_to` writes the record and then appends the
event (`store.rs:1345-1347`). `docs/coverage.md:70` states this as the E-53
contract (D103), so reader and writer agree.

### 1.2 The volatile state (hub)

`Peers` (`herd-hub/src/server.rs:893-1029`) is memory only, lost on restart,
and holds thirteen maps. The ones that carry a guarantee:

| Field | Line | What it owns | What a restart costs |
|---|---|---|---|
| `live_attempts` | 949 | (attempt, runner) per job — the ownership gate | rebuilt lazily from the record (`standing_of`, `server.rs:1817-1842`) |
| `unwritten` | 936 | verdicts the hub could not write (D19) | **the only copy besides the runner's**; the runner redelivers (E-44) |
| `held_verdicts` | 1029 | verdicts held until their collection is whole (D167) | re-derived from disk on the next `wait` (`answer_a_wait`, `server.rs:6332+`) |
| `waiting` | 1011 | the live E-04 sentence per queued job (D171) | empty; the first pass writes every reason again |
| `stalled` | 923 | why a job cannot be written | re-derived when the write is retried |

**F.** `collections_owed` and `collection_owed` are deliberately *not* in
this list: both are recomputed from the disk every time
(`store.rs:2367-2425`). That is the right call and it is what makes a hub
restarted mid-transfer resume correctly.

### 1.3 The runner's state

| Path (under the data root) | Holds | Written by | Reaped by |
|---|---|---|---|
| `<root>/.hub-state-root` | which hub state these files belong to | `root.rs:217-220` | never (one line, overwritten) |
| `jobs/<id>/` | one job's workspace, `{in}`, `{out}` | `open_workspace` (`root.rs:279-311`) | job end, and `reclaim_a_past_life` at startup (`reclaim.rs:405-422`) |
| `held-results/<job>.json` | a spooled `job_finished` frame (D34) | `spool.rs` | on ack, on refusal, on foreign state root |
| `outputs/<job>-<attempt>/` | a streamed collection's bytes | `outputs.rs:137-142`, called from `job.rs:538` | **only** `files_acknowledged` (`lib.rs:1421-1446`) — see §5.4 |

**F.** The data root is also flocked, one runner per root
(`root.rs:639-652`), which is what makes "anything under `jobs/` at startup
is a leftover" true.

### 1.4 Responsibility map — single owner or shared

| Responsibility | Owner | Shared with | Evidence |
|---|---|---|---|
| Job identity (`j…`) | hub store; `mkdir` arbitrates | nobody | `store.rs:1399-1409` |
| State-root identity | hub store | announced to runners in `Welcome` | `store.rs:2541-2552`, `server.rs:2258-2266` |
| Attempt number | hub store (`begin_attempt`) | carried on every runner frame | `job.rs:1715` trait doc, `proto` frames |
| Job state transitions | hub store (`transition`) | **nobody** — one table | `store.rs:2475-2499` |
| "Is this frame about the current attempt" | hub server | reads the store when memory is cold | `server.rs:1805-1843` |
| Commit | requester (`herdctl`) | runner executes it; hub only records | `herdctl/src/declaration.rs:443-493` |
| Output manifest (path, len, sha256) | runner (`OutputFile::collected`) | hub verifies and records | `herd-proto/src/outputs.rs:213-220`, `store.rs:1710-1716` |
| Output completeness oracle | hub store (`collection_is_whole`) | asked by server and by `herdctl` indirectly | `store.rs:509-516` |
| Output budget / eviction | hub store (`sweep_now`) | nobody | `store.rs:642-700` |
| Verdict release to a requester | hub server (D167) | — | `server.rs:4670-4703` |
| Requester-side digest check | `herdctl` | — | `herdctl/src/lib.rs:1313-1333` |

**No responsibility here is genuinely shared in the sense of two writers.**
The three structural couplings are listed in §6.

---

## 2. The job and attempt automaton

### 2.1 Job states and the permitted edges

`JobState` (`herd-hub/src/job.rs:156-176`): `Queued`, `Assigned`, `Running`,
`Interrupted { reason }`, `Finished { outcome }`. Only `Finished` is
terminal (`job.rs:181-184`).

**F.** The whole transition table is one function, `transition`
(`store.rs:2475-2499`):

| From | Allowed to | Note |
|---|---|---|
| `Queued` | `Assigned`, `Running`, `Finished` | `Running` direct covers a start frame that beats the assignment write |
| `Assigned` | `Running`, `Finished`, `Queued`, `Interrupted` | `Queued` is the requeue edge (E-21) |
| `Running` | `Finished`, `Queued`, `Interrupted` | |
| `Interrupted` | `Running`, `Finished`, `Queued` | the runner coming back resolves it |
| `Finished` | `Finished` with the **same** outcome → no-op | a different outcome is refused |
| `Interrupted`→`Interrupted`, `Queued`→`Queued` | no-op | a second restart says nothing new |
| anything else | **refused by name** | |

**F.** `Finished` is enforced twice, which is the right belt-and-braces: the
table above, and `open_unless_over` (`store.rs:1116-1125`), the single gate
every writer in the store passes before it can even *read* the record it
wants to modify. The comment at `store.rs:1078-1086` records why — two
earlier writers (`record_metrics`, `append_log`) each lacked their own
guard, and a late frame overwrote a closed record.

### 2.2 Attempts

`Attempt` (`job.rs:210-269`) carries `number`, `runner`, `started_at`,
`ended_at`, `requeue_code`, `ended`, `why`, `steps`.

**F.** Attempt budgets are *derived from the attempt list on every read*, not
counted in a field (`job.rs:1323-1355`, `goes_a_runner_took`,
`requeues_spent`). D79's rule — a counter is a second copy that can disagree
— is applied consistently, and the same reasoning produces
`collections_owed` and `output_holdings` from the disk.

**F.** An attempt that produced a verdict gets `ended_at` stamped by the
terminal transition itself (`store.rs:1331-1341`). The comment there records
the defect this fixed: the happy path never stamped it, so the attempt that
produced the verdict stayed "current" for ever and a later frame naming it
passed the ownership gate on a closed record. That is a cured
identity-confusion bug, and the cure is at the right level (the transition,
not the caller).

### 2.3 Commit points and what a crash between two writes costs

Ordering inside `move_to` (`store.rs:1345-1371`):
`write_record` → `append_event` → (if terminal) `sync_file(log)`,
prune `pending`/`open_work`/`log_chunks` → `write_index`.

| Crash window | What the disk holds | What the next hub does | Verdict |
|---|---|---|---|
| after `write_record`, before `append_event` | record says `finished`, history lacks the line | nothing; the history is one line short for ever | **F** — tolerated, and declared (`docs/coverage.md:70`) |
| after the terminal record, before `write_index` | index still lists the job as pending | `recover` reads the record, sees terminal, drops it from `pending` (`store.rs:900-914`) | self-correcting |
| after `mkdir jobs/<id>`, before `job.json` | an empty job directory | named on stderr, id stays spent (`store.rs:922-928`) | self-correcting |
| after `append_bytes(log)`, before `log.seq` | the log holds uncounted bytes | the first `log_at` compares the file length with the counted length and gives the uncounted bytes back, so the resent chunk is not appended twice (`store.rs:1202-1212`) | **F** — explicitly designed for |
| after `record_outputs`, before `record_state` | manifest and inline bytes on the record, state still `running` | not acknowledged; the runner redelivers and all three writes run again, idempotently | **F** — `record_the_end`, `server.rs:4876-4886` |
| power loss (not `kill -9`) with `log.seq` in page cache | hub has no number to compare | says so rather than assuming nothing is missing (`durable.rs:96-107`, `store.rs:1222-1229`) | **F** — honest |

**F.** Interrupted recovery: `recover` moves every `Assigned`/`Running`
record to `Interrupted` with a sentence (`store.rs:955-967`). No job is left
reading `running` for ever (D17).

**I.** None of the crash windows above is exercised by a fault injector.
`herd-hub/tests/killed_hub.rs` signals the real binaries at job boundaries
(`a_hub_killed_outright_mid_job_loses_nothing:136`,
`a_log_survives_twelve_killed_hubs_whole:239`), which is strong, but the kill
lands wherever the scheduler puts it — there is no test that *forces* a stop
between `write_record` and `append_event`, or between the log append and the
`log.seq` write. The properties are argued in comments and are plausible;
they are not proven. See the experiments in §8.

### 2.4 Expiry, retry, deduplication, arbitration after reconnect

* **Expiry.** `deadline_unix_s` bounds *waiting* only and is judged on the
  hub's clock (`herd-proto/src/lib.rs:1661-1675`). A started job is never
  touched by it. **F.**
* **Retry.** Two kinds, with two budgets, kept apart by
  `Attempt::requeue_code` (`job.rs:232-247`): E-21's "a runner disappeared"
  budget, and D76's "the declared requeue code" budget.
  `give_the_job_another_go` (`server.rs:5181-5260`) refuses to act on an
  attempt that is not current,
  which is what stops two callers (the watchdog and an `AttemptAbandoned`)
  spending three goes on a job allowed two. **F.**
* **Deduplication.** `submit` is one store call that decides and mints under
  one lock (`job.rs:1748-1760`, `store.rs:1574`). The key is
  length-prefixed over (command, labels, declared commit)
  (`job.rs:621-636`), derived from the record and never stored
  (`job.rs:1138-1148`). Only an **open** job is joined. **F.**
* **Arbitration after reconnect.** Three gates, in this order: the state root
  (`server.rs:2325-2331`), the roster token (`server.rs:2050-2061`), and the
  attempt/holder pair (`standing_of`, `server.rs:1805-1843`). A runner that
  loses is told `StaleAttempt` and kills the job it was running
  (`herd-runner/src/lib.rs:1599-1641`). **F.**

---

## 3. What each acknowledgement actually guarantees

### 3.1 `JobResultAck` — the verdict

**F.** `server.rs:3903-3912`: `record_the_end` runs first and the ack is sent
only if it returned true. `record_the_end` (`server.rs:4851-4886`) writes
metrics, then outputs, then the state, inside one try, three fast tries, then
hands the verdict to a background task that never gives up (D19).

**Guarantee: at-least-once delivery, exactly-once recording, durable before
ack.** The mechanism that produces "exactly once recorded" is named and is
not a race: `open_unless_over` refuses every write to a terminal record
(`store.rs:1116-1125`), so a redelivered verdict is answered `JobIsOver`
(`server.rs:1826-1830`) and the runner drops its spool by name
(`herd-runner/src/spool.rs:93-101`). This is **deduplication by terminal
state**, not by a message id.

**F, limit.** `record_outputs` is inside the same try, so a verdict whose
files could not be written is not acknowledged. That is the correct coupling
and it is stated at `job.rs:1814-1827`.

**H.** If the hub cannot write *and* the runner is `SIGKILL`ed before it can
spool (no clean stop, so `held-results/` is never written), the verdict is
lost. The record then reads `interrupted` and the job is requeued or ends
naming why — a loss, but never a silent success. I found no path that turns
this into a wrong verdict.

### 3.2 `LogAck` — one output chunk

**F.** `append_log` (`store.rs:2205-2245`) appends with
`durable::append_bytes`, which does **not** fsync (`durable.rs:78-81`), and
updates `log.seq` in place without fsync (`durable.rs:96-107`). So a
`LogAck` means *"these bytes are visible to any reader of this file"*, not
*"these bytes survive the machine"*. The log is fsynced once, at the terminal
transition (`store.rs:1349-1353`).

**Guarantee: at-least-once on the wire, de-duplicated by sequence number,
visible-but-not-durable until the job ends.** A `seq` below the expected one
is `LogWrite::AlreadyRecorded` and dropped (`store.rs:2216-2218`); a `seq`
above it writes a named gap marker into the log and a `LogGap` event
(`store.rs:2220-2232`, marker at `store.rs:2429-2443`). This is the right
call and it is written down as such — but it must not be read as durability.

### 3.3 `JobFilesAck` / the chunk path — artefacts

**F.** A streamed file is verified before it becomes visible: length checked
against the manifest, offset checked against what the hub holds, sha256
recomputed over the part file, then `fsync` + `rename` into `files/` +
`fsync` of the directory (`store.rs:1809-1906`). A digest mismatch throws the
bytes away and names both digests (`store.rs:1878-1888`). `files/` therefore
holds only whole, verified files.

**Guarantee: at-least-once transfer, content-addressed idempotence.** A chunk
re-sent for a file already whole is a no-op (`store.rs:1838-1841`), and a
chunk at the wrong offset is refused rather than spliced
(`store.rs:1854-1859`). The runner's spool is removed only after the ack
(`herd-runner/src/lib.rs:1421-1446`).

### 3.4 What is *not* guaranteed anywhere

**Exactly-once execution does not exist in Herd and nothing claims it.** The
honest statement:

> Herd executes a job **at least once** and records **at most one** verdict
> for it.

**F, the mechanism that makes double execution reachable.** The watchdog
requeues an attempt whose runner has gone silent (`server.rs:5181-5260`); it
has no way to stop that runner, and the runner does not stop its own job when
the link dies — it holds the result (`herd-runner/src/lib.rs:750-762`,
`drop_stale` only fires when a `StaleAttempt` frame *arrives*,
`lib.rs:1599-1641`). So during a partition longer than the silence threshold,
attempt 1 and attempt 2 both run. Attempt 1's verdict is then refused as
`Superseded`. Side effects the job itself produced twice are the consumer's
problem and Herd says nothing about them.

**I.** Whether the first consumer's declared commands are idempotent under
double execution is unknown to me and is not Herd's to know. It is worth
asking them, because it is the one externally visible consequence of E-21.

---

## 4. The identity chain, end to end

### 4.1 The chain as the code builds it

```
consumer's request
  └─ herdctl resolves {commit} and {args} in its own checkout   (D106, E-01)
       declaration.rs:443-493 — fills only those two tokens
  └─ Asked { command, labels, priority, declaration{command,commit,args,executor}, … }
       proto/lib.rs:1611-1731
  └─ hub: submit → dedup key over (command, labels, commit)     (D69, D156)
       job.rs:621-636 ; mkdir arbitrates the id                  store.rs:1399-1409
  └─ job id  j0000000001  — meaningful only inside one state root
       state root identity: store.rs:2541-2552
  └─ begin_attempt → attempt number + runner name + `why` + `-j`
       job.rs:1986-1992, written on the record
  └─ AssignJob to one runner; every runner frame carries (job_id, attempt)
  └─ runner: workspace jobs/<id>/ cleared first                  root.rs:279-290
       four steps up/fetch/run/down, fixed order, no conditions   step.rs:12-26
       a failed preparation step means the job never started
  └─ verdict: JobFinished{job_id, attempt, outcome, chunks_sent, metrics,
               steps, outputs{files[path,len,sha256],refused,skipped,streamed}}
  └─ hub: is_current → record_metrics → record_outputs → record_state → ack
  └─ artefacts: files/ keyed by **job id**, paths relative to {out}
       proto/outputs.rs:145-164 — deliberately not keyed by `leaves`
```

### 4.2 Where it can be confused — the findings

**(a) The commit is never confirmed on the way back. F, and this is the
central gap.**
The commit appears in exactly three places: `Declaration::commit`
(`herd-proto/src/declaration.rs:225-230`), the text of the `fetch` and `up`
shell lines that `herdctl` filled (`declaration.rs:459-474`), and the dedup
key. **No frame from the runner carries a commit.** `StepRun`
(`declaration.rs:250-274`) carries step, start, duration, exit code and
`ended` — not the revision the tree was at. So the hub's statement "this
verdict is about commit X" rests entirely on the assumption that a `fetch`
step exiting 0 left the tree at X.

The runner-side protection that exists is real but partial: a preparation
step that fails means the job never runs (`step.rs:19-22`), and the workspace
is emptied before every attempt (`root.rs:282-290`). What is *not* protected
is a `fetch` that exits 0 without reaching the commit — a consumer script
with a fallback, a `git fetch` that succeeded against a stale mirror, a
`switch` whose failure is swallowed by `&&` ordering, a shared cache. In all
of those Herd attributes a verdict to a commit the tree was never at, and
there is nothing on the record that could contradict it afterwards.

> **Candidate invariant.** The verdict should carry back the revision the
> workspace was at when `run` started, and the hub should refuse — or at
> minimum record a divergence on `job.json` — when it is not the declared
> commit. One field on `StepRun` or on `JobFinished` closes the loop and
> makes I01 falsifiable for the first time. This is a protocol change and so
> is Alexa's, not mine.

**(b) D156 fixed the dedup key, and the fix is at the cause.** F.
`job.rs:423-440` records the defect in full: the commit moved out of the
command string into `Declaration::commit`, and the key — built from the
string — silently stopped distinguishing commits, so "four requesters asking
about four commits were handed one commit's verdict". The fix puts the commit
into the key as a counted field (`job.rs:621-636`) and derives the key from
the record so no migration was needed (`job.rs:1138-1148`). That is a cause
fix, not a symptom fix, and the reasoning is generalised in the comment
("before giving a submission a new field, ask whether it used to be part of a
string a key already reads"). **It is the strongest single piece of work I
read.**

**(c) What the dedup key still does not distinguish. F.**
The key is (command, labels, commit) and nothing else. Two submissions that
agree on those three but differ in `declaration.executor` (the four shell
lines), `timeout_s`, `memory_mib`, `tmpfs_mib`, `deadline`, `after` or
`labels_from` are **one job**, and the second requester is handed the first
one's verdict. For the first consumer this is probably benign — the executor
is a function of their declaration file at that commit — but it is an
assumption Herd does not state and cannot check. **H:** the realistic way to
hit it is a consumer changing their executor or their timeouts without
changing the commit, e.g. during their own tooling development.

**(d) `coalesce` and `dedup_key` are parsed by `herdctl` and go nowhere. F.**
`herdctl/src/declaration.rs:264-267` declares them, `:492-493` resolves their
`{commit}`, two tests assert the resolution
(`herdctl/tests/declared.rs:200-205`, `:285-286`) — and no code path puts
either field into `Asked`, `Declaration` or any frame. `docs/coverage.md:59`
says E-42 is **planned for M14**, so this is forward work and not a
regression. The risk is a *contract* one: a consumer can write
`dedup_key = "produce:{commit}"` in their declaration file today, see
`herdctl` accept it, and be joined on the hub's own rule instead. It should
be refused by name or documented as inert until M14.

**(e) An old result for a new hub is well defended. F.**
`store.rs:2541-2552` mints a random 16-byte state-root id, kept for the life
of the root. The hub refuses a runner holding another root's state
(`server.rs:2325-2331`), and the runner independently sets aside everything
it holds (`herd-runner/src/lib.rs:1847-1920`), never offering it
(`herd-runner/src/spool.rs:104-120`). Two integration tests cover it:
`killed_hub.rs:1302` and `durability.rs:540`. This closes the "`j0000000001`
of a root that is gone" case completely, as far as I can see.

**(f) A late frame about a closed job. F.** Covered twice: the terminal gate
in the store, and `standing_of`'s `JobIsOver` arm (`server.rs:1826-1830`),
with `ended_at` on the attempt stopping the ownership gate from believing it
(`store.rs:1331-1341`).

**(g) Two logs, one file. F.** A requeued job's `log` holds both attempts,
separated only by `new_attempt_marker` (`store.rs:2448-2456`). A consumer
parsing the log with no awareness of that line reads attempt 1's partial
output as part of attempt 2's. The marker exists and is named; whether the
consumer's tooling honours it is **I**.

**(h) The runner-name gate is documented as weaker than it is. F.**
`server.rs:1795-1804` says, in a doc comment on the ownership gate: *"The gate
is exactly as strong as M15's token, which does not exist yet; nothing here
should be taken for proof of who is speaking."* M15's token **does** exist on
`0a2cf73`: `admission_refusal` (`server.rs:2043-2069`) calls
`Roster::admit`, which compares a salted SHA-256 in constant time
(`roster.rs:234-256`), and it is covered by `herd-hub/tests/approval.rs`.
A stale comment on the one gate that decides whose verdict is believed is an
observability defect, not a correctness one — but it is exactly the comment a
future reader will trust.

---

## 5. Integrity risks

### 5.1 Concurrent writes

**F.** Every store write takes one process-wide `Mutex` through `lock()`
(`store.rs:1069-1073`), and the state root takes `flock` so a second hub
cannot exist. Inter-process concurrency is therefore not a hazard; the one
reader the design admits is the consumer reading records while a job runs
(E-53), and the rename discipline makes that safe — with a test for a reader
racing writers (`durable.rs:317-349`).

**H.** The cost of that single mutex is a progression concern, not an
integrity one, and it belongs to audit B. I note only that the write path is
synchronous under the lock and is called from async contexts, which is the
mechanism D171 describes.

### 5.2 A short collection that reads as whole — the one real hole

**F.** `record_outputs` (`store.rs:1694-1718`) handles two bad cases by
`continue`-ing **before** the manifest entry is pushed:

* a path that is not relative (`:1698-1707`) → `skipped` gets a sentence;
* bytes that disagree with their own length or digest (`:1710-1716`) →
  `skipped` gets a sentence.

In both cases the file is **absent from the manifest**, not present-and-empty.
Everything downstream counts off the manifest: `job_outputs` returns what the
manifest has (`store.rs:1763-1799`), and `herdctl` computes
`missing = files.iter().filter(|f| !f.is_held()).count()`
(`herdctl/src/main.rs:1322`) — which is **0**. `code_for`
(`herdctl/src/main.rs:591-596`) therefore returns the job's own code, and a
passing build with a dropped output file exits **0**.

**The two cases are not equal, and the distinction matters.** The *path*
case is deliberate and is written down: `JobStore::record_outputs`'s contract
says a path the store will not join is "dropped and named on the record, not
an error" (`job.rs:1823-1826`), and a walk on a runner cannot produce one
anyway. The *digest* case has no such sentence anywhere, and it contradicts
the design's own rule, which appears three times elsewhere: "the manifest
outlives the bytes … twelve files on the record and eight on the disk have
to read as twelve" (`store.rs:1773-1778`, `herdctl/src/lib.rs:856-859`).
Every other absence path in Herd honours that rule — `job_outputs` keeps the
entry when the file cannot be read back (`store.rs:1790-1798`),
`send_the_files` returns an `AbsentFile` with a reason
(`server.rs:4428-4445`), `write_outputs` names what it did not get
(`herdctl/src/lib.rs:870-877`). This one path does not, and it is the only
one where the hub decides the entry never existed.

**On likelihood.** Reaching it needs a manifest entry whose digest does not
match its own bytes. The runner computes both from the same buffer
(`herd-proto/src/outputs.rs:213-220`), and WebSocket frames are checksummed,
so the realistic triggers are a runner of a different build, a corrupted
spool read, or memory corruption — **H**, low frequency, high consequence,
and precisely the shape the consumer reported. The `skipped` sentence does
reach stderr (`herdctl/src/main.rs:1261-1263`), so the fact is not invisible
— but it does not reach the exit code, which is the thing a wrapper reads.

**No test covers it.** I found no test anywhere that sends a verdict whose
inline bytes disagree with the manifest. The only output-integrity
integration tests are `herd-hub/tests/outputs_whole.rs:232`, `:333`, `:486`
and `herd-hub/tests/outputs_*.rs`, all on the streamed path.

### 5.3 `files.part/` is unbounded on the hub

**F.** `clear_output_parts` (`store.rs:1976-1982`) is called only when every
file is in place. `sweep_now` (`store.rs:642-667`) measures
`directory_bytes(files/)` and cleans `files.gone/`; it never looks at
`files.part/`. The code says so deliberately — "Part-written bytes are a
transfer in flight — real on the disk, outside the budget, and never evicted"
(`store.rs:1898-1903`) — which is correct while the transfer is in flight and
wrong once it is abandoned. A runner that dies mid-collection leaves up to
one file's worth of bytes per abandoned collection, for ever, outside the
budget that exists to keep the volume from filling. Nothing names them, and
`output_holdings` will keep reporting them as held (`store.rs:1920-1926`),
so a later runner would be told to resume a transfer whose source is gone.

### 5.4 `outputs/` is unbounded on the runner

**F.** `herd-runner/src/outputs.rs:137-142` creates
`<data root>/outputs/<job>-<attempt>/` by renaming `{out}` out of the
workspace; it is removed in exactly one place, `files_acknowledged`
(`herd-runner/src/lib.rs:1421-1446`). `reclaim_a_past_life`
(`herd-runner/src/reclaim.rs:391-446`) sweeps processes, `jobs/` and
cgroups — **not** `outputs/`. `grep` finds no other reader or remover of
`SPOOL_DIR`.

So a runner that is `SIGKILL`ed, or whose hub never acknowledges a
collection, keeps hundreds of megabytes per abandoned collection on its
owner's machine for ever, and a restart does not reclaim it. This is the
clearest breach of invariant 2 ("a runner never destabilizes its host") that
I found, and unlike §5.3 it is on somebody else's machine. Note also that the
spool is *orphaned* at startup even in the recoverable case: the verdict is
spooled to `held-results/` but nothing reconnects it to the bytes in
`outputs/`, so a collection cannot in fact survive a runner restart — only
the verdict can. **H** on that last point; I did not find the reconnection
and `FilesOut` is built only in `Session::finish` (`lib.rs:1273-1278`).

### 5.5 `Peers::unwritten` is never pruned by a terminal state

**F.** `unwritten` is filled by `note_a_stall` (`server.rs:1570-1586`) and
emptied only by `stall_is_over` (`server.rs:1602-1607`), which runs on a
**successful** write (`server.rs:3642`, `:4884`, `:4967`).
`keep_trying_to_write_results` (`server.rs:4926-5010`) retries for ever by
design and, on `Err`, prints and keeps the entry.

**H, the reachable sequence.** The hub fails to write attempt 1's end (disk
full); the entry lands in `unwritten`; the runner goes silent; the watchdog
requeues (`give_the_job_another_go` does not touch `unwritten`); attempt 2
finishes and writes its verdict; the disk recovers. From then on every retry
cycle calls `record_metrics` on a terminal record, is refused by
`open_unless_over`, and prints *"j… still cannot be written"* for the life of
the process. The record is correct and no wrong verdict is produced — the
cost is one permanently stuck entry, a log line per cycle, and an operator
reading a persistent error about a job that is in fact fine. The fix is to
recognise "this verdict can never be written because the job ended another
way", say it once, and drop the entry.

### 5.6 `jobs/` grows without bound

**F.** The sweeper removes `files/` and never a job directory
(`store.rs:642-700`); nothing else removes one. `job.json`, `events.jsonl`
and `log` of every job ever submitted stay for the life of the volume. At the
consumer's observed rate (214 job groups in one pass, D171) this is a slow
but real growth, and it is also what keeps job ids monotonic
(`recover`: `next_sequence = max(dir names) + 1`, `store.rs:879`) — so
retention of job directories cannot be added naively without reopening id
reuse. Worth naming before anyone proposes a retention feature.

### 5.7 A record is not readable across a version bump

**F.** `RECORD_VERSION = 1` (`job.rs:52`) and a record of any other version is
a hard read error (`store.rs:1143-1149`); so are the layout, index, readings
and roster versions. There is **no migration code anywhere** (`grep -rl
migrat --include=*.rs` finds only comments). The consequence is written down
honestly at `job.rs:41-51`: a bump makes the operator's whole state root
unreadable in both directions and ships with a migration or an instruction.
For I12 the position today is: **compatibility is enforced by refusal, and
nothing has ever had to migrate.** That is a defensible state and an untested
one.

---

## 6. Structural couplings — argued, not measured by file size

The mandate is explicit that `server.rs` (7 985 lines on this base) and
`store.rs` (5 519) are not evidence of anything. I looked for couplings
instead, and found three worth naming. None of them is "this file is long".

1. **`server.rs` owns both the wire and the policy.** It decides admission,
   attempt ownership, requeue policy, verdict release, and the dispatch pass,
   and it holds all of that in one `Peers` mutex (`server.rs:893-1029`) that
   is also taken on the log path (`server.rs:1554-1566`). The coupling is not
   the length; it is that **a policy change and a framing change touch the
   same lock**. That is also the mechanism D171 had to reason about.
2. **The store is both the record and the artefact store.** `JobStore` has 40
   methods; roughly a third of them (`write_output_chunk`,
   `output_holdings`, `collections_owed`, `collection_owed`,
   `sweep_outputs`, `note_outputs_fetched`, `read_output_chunk`,
   `clear_output_parts`, `job_outputs`, `record_outputs`) are a content
   store with a budget and an eviction policy, bolted onto a record store
   whose discipline is "one atomic rename per record". They share the lock
   and the state root and nothing else. This is the cleanest seam in the
   codebase if anything is ever to be extracted — and §5.3's hole is exactly
   where the two disciplines meet.
3. **`herdctl` is a client *and* the declaration compiler.**
   `herdctl/src/declaration.rs` (1 266 lines) parses the consumer's file,
   resolves tokens, and decides what is a requester's field and what is the
   declaration's (`:285-300`). The hub never reads a declaration file. That
   is a good boundary — but it is why §4.2(d) can happen: a field can be
   fully supported in the compiler and absent from the protocol with nothing
   failing.

---

## 7. The candidate invariants of §6 of the mandate

Marked for the six the brief assigns me. "Oracle" is what would let someone
say true or false.

| Id | Where it stands today | Code | Test that carries it | Fail case **not** searched for |
|---|---|---|---|---|
| **I01 Identity** | **Partly held.** Job/attempt/state-root/runner are all enforced. **Commit is not:** nothing comes back carrying it (§4.2a). Dedup since D156 distinguishes commits (§4.2b) but not executors (§4.2c). | `store.rs:2541`, `server.rs:1805-1843`, `job.rs:621-636` | `killed_hub.rs:1175`, `:1302`, `durability.rs:540` | a `fetch` that exits 0 at the wrong revision; two submissions with one commit and two executors. **No oracle exists for either.** |
| **I03 Durability** | **Held for the verdict, explicitly not held for the log.** Record before ack (`server.rs:3903-3912`), fsync+rename everywhere (`durable.rs:36-57`). Log chunks are visible-not-durable until the job ends. | `durable.rs`, `store.rs:1345-1371` | `killed_hub.rs:136`, `:152`, `:239`; `durability.rs:46`, `:414` | a stop *forced* between the record write and the event append, or between the log append and `log.seq`. Tests signal at job boundaries and take what the scheduler gives. |
| **I04 Idempotence** | **Held, by terminal state and by sequence number.** Redelivered verdict → `JobIsOver` → spool dropped. Duplicate log chunk → `AlreadyRecorded`. Duplicate file chunk → no-op or offset refusal. **Execution is at-least-once** and says so (§3.4). | `store.rs:1116-1125`, `:2216`, `:1838-1859` | `killed_hub.rs:975`, `:1175`; `outputs_whole.rs:232` | a `unwritten` entry for a job that ended another way (§5.5). Double *execution* during a partition is not tested and arguably cannot be. |
| **I05 Artefact integrity** | **Held on the streamed path, broken on one inline path.** Streamed: length, offset and sha256 all checked before the file is visible. Inline: a disagreeing file is dropped from the manifest and `herdctl` exits 0 (§5.2). | `store.rs:1809-1906` vs `store.rs:1710-1716` | `outputs_whole.rs:232`, `:333`; `outputs_fetch.rs`; `herdctl/src/main.rs:1382` | **a verdict whose inline bytes contradict their own digest.** No test sends one. |
| **I11 Observability** | **Strong on the record, weakened in one place by D171.** Every placement writes its line (E-36), every wait writes its cause, `events.jsonl` is ordered history. But the live E-04 sentence now lives only in `Peers::waiting` (`server.rs:1011`), so a hub restart loses every queued job's current position, and `job.json`'s countdown decays between level changes — both named in D171 rather than hidden. | `job.rs:1387-1491`, `server.rs:1588-1600` | `herd-hub/tests/` queue/priorities suites | reconstructing a verdict's *cause* after an incident still cannot name the commit the tree was at (I01), which is the question an incident actually asks. |
| **I12 Compatibility** | **Enforced by refusal, never exercised.** Six independent version numbers (protocol 2, record 1, layout 1, index 1, readings 1, roster 1) plus a build version in the handshake (E-51). A record of an unknown version stops the hub rather than being guessed at. No migration code exists. | `job.rs:41-52`, `store.rs:385-392`, `proto/lib.rs:113` | `herd-hub/tests/versions.rs:140` | a real forward migration. Also: `herdctl`'s `coalesce`/`dedup_key` accepted and inert (§4.2d) is a compatibility surface with no refusal behind it. |

**Two things the mandate warns about, applied to my own reading.** A green
unit test is not proof of an invariant: I have counted none of them as such
above, and where an integration test does carry a property I have named it.
And no guarantee is "validated" here whose failure scenario was not searched
for — which is why I01, I03 and I05 carry explicit "not searched for"
columns rather than a verdict.

---

## 8. Experiments I would have run, and what each would settle

All of these need an isolated bench and an authorisation under mandate gate
T2. None was run; none should be run on the shared host.

| # | Experiment | Settles |
|---|---|---|
| A | Send a `JobFinished` with one inline `OutputFile` whose `sha256` is wrong, against a real hub, and read the requester's exit code. | §5.2 — whether a corrupt inline file really exits 0 as the code reads. One hub + one fake runner, seconds, no load. |
| B | `kill -9` the hub in a loop while a job transitions, with `strace`/`fault-injection` forcing the stop between `write_record` and `append_event`. | I03 — whether the one-line-behind property holds under a *forced* window, not a lucky one. |
| C | Kill a runner mid-streamed-collection, restart it, and measure `<data root>/outputs/`. | §5.4 — the size of the leak and whether any path reclaims it. Needs a few hundred MiB of scratch. |
| D | Abandon a streamed collection against a hub at its output budget, then submit until the budget would evict. | §5.3 — whether `files.part/` really escapes the budget, and by how much. |
| E | Partition a runner for longer than the silence threshold with a job running, and count executions. | §3.4 — confirms at-least-once execution as a measured fact rather than a reading. |
| F | A declared command whose `fetch` exits 0 without reaching the commit. | I01 — demonstrates that Herd today has no oracle for the commit, which is the finding the consumer's incident most needs settled. |

---

## 9. Open questions for the owner

| # | Question | Recommendation | Consequence if deferred |
|---|---|---|---|
| 1 | Should the verdict carry back the revision the workspace was at? | **Yes** — one field, and the hub records a divergence rather than refusing at first. It is a protocol change (PROTOCOL_VERSION) and so is Alexa's. | I01 stays unfalsifiable and the reported "verdict on the wrong commit" cannot be ruled out by anything in the code. |
| 2 | Should a disagreeing inline file keep its manifest entry? | **Yes**, and it is a small, local change in `record_outputs`. It makes the inline path obey the rule the rest of the design states. | A corrupt output file is reported as a clean, complete run. |
| 3 | Who reclaims `outputs/` on a runner and `files.part/` on a hub? | A startup sweep on the runner and a `files.part` arm in `sweep_now`. Both are additive. | Two unbounded disk leaks, one of them on the consumer's own machine (invariant 2). |
| 4 | Should `herdctl` refuse `coalesce`/`dedup_key` until M14? | **Refuse by name**, or document them as inert in the declaration reference. | A consumer can believe they control deduplication and be joined on another rule. |

---

## 10. What this document does not establish

* **I** — no runtime evidence. Nothing here was measured on the consumer's
  hub or on any hub. Every claim is a reading of `0a2cf73`.
* **I** — I did not read `matcher.rs`, the queue ordering, the admission
  arithmetic or the dispatch loop beyond what the state model required; they
  are audit B's and I deliberately avoided forming a view that would
  contaminate a second independent reading.
* **I** — the consumer's own tooling. Whether their wrapper reads
  `herdctl`'s exit code, honours `new_attempt_marker`, or is idempotent under
  a double execution is unknown and is the other half of several findings
  above.
* **I** — `herd-runner`'s confinement, cgroups and privilege paths were read
  only where identity or artefacts passed through them.
* **H, stated as such** — §5.4's claim that a streamed collection cannot
  survive a runner restart. I found no code reconnecting `outputs/` to a
  spooled verdict, which is evidence of absence only as strong as my search.
