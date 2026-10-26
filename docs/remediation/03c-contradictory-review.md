# 03c — Contradictory review of audits A and B

**Date:** 2026-10-24
**Base read:** `0a2cf73`, the commit both audits read. Verified in a
separate worktree; nothing in `/mnt/ssd_980/herd-project` was touched.
**Reviewed:** `docs/remediation/03a-identity-and-storage.md` at
`685b19e` (audit A) and `docs/remediation/03b-concurrency-and-progression.md`
at `41abb27` (audit B), plus both handoffs.
**Method:** every **F** in both reports was taken back to the code at
`0a2cf73`. I am the author of neither. Read-only: no Rust, no
configuration, no migration changed; no build, no test, no experiment,
no load on this host.
**Marks:** **F** = observed at `0a2cf73` with a `file:line`. **H** =
hypothesis, with what would confirm it. **I** = unknown.

---

## 0. The three things in the brief that are wrong

Said first, because one of them is the brief's central question.

**0.1 — The "frontal contradiction" on D171 does not exist. F.**
The brief states that audit A wrote that D171 *corrects its cause* and
audit B that it is a *symptom fix*. **Audit A never says anything of the
kind about D171.** A's five mentions of D171 are at `03a:91`, `:452`,
`:560`, `:591` and `:623`, and none is a verdict on cause versus
symptom; A's handoff says only "D171's fix is correct but costs
observability". The sentence the brief is quoting is A's **§4.2(b)**,
`03a:371` — *"D156 fixed the dedup key, and the fix is at the cause"* —
which is about **D156**, the deduplication key, a different decision
about a different defect. The two audits were set against each other on
a misreading. §3 below answers the substantive question anyway, because
it is worth answering; but the premise is false and no arbitration
between the two agents is owed.

**0.2 — `0a2cf73` is not tag `v0.3.7`. F.**
`v0.3.7` is `3da48f5` (`git rev-list -n1 v0.3.7`). `0a2cf73` is
`origin/main`, two merges later (PR #71 `orchestration-remediation`,
then PR #70 `hotfix-dev-lane`). The base is right and both audits read
it correctly; the label on it is wrong. An audit base that is named
wrong in the mandate is the kind of thing that makes a finding
unreproducible six months later.

**0.3 — The brief's own three "verified" findings are stated loosely.**
I re-did them as asked (§1.1, §1.2, §1.5). All three hold. Two of the
three are *stronger* than the brief's shorthand: A-5 is not only an
observability defect, and A's §5.4 "H" is provable to **F**. One
citation in A is wrong by 39 lines (§1.2).

A fourth point, not an error but a consequence: the A/B split the brief
imposed produced one real blind spot, and it is load-bearing for B's
central argument. §4.2.

---

## 1. Audit A — every F taken back to the code

### 1.1 §4.2(a) the commit is never confirmed on the way back — **holds, F**

`StepRun` (`herd-proto/src/declaration.rs:251-272`) carries `step`,
`started_unix_ms`, `duration_ms`, `exit_code`, `ended`. No revision.
`Declaration::commit` (`:230`) is the only place a commit exists, and it
is filled by the requester. `grep -n commit herd-proto/src/*.rs` outside
`declaration.rs` returns five hits, all doc comments
(`lib.rs:1698`, `:1720`, `:1850`, `outputs.rs:22`, `placement.rs:288`).
**No frame from a runner carries a commit.** A's finding holds exactly
as written, including its scope: the gap is an *absent field*, not a
weak check, so no test could close it and no reading of the current code
can rule the failure out.

### 1.2 §5.2 the inline digest path — **holds, F; one citation wrong**

`store.rs:1711` `if let Some(why) = file.disagreement()` → `:1716`
`continue`, and the manifest push is at `:1726-1730`, **after** it. So
the entry is never created. Confirmed, and three things A did not say
make it tighter:

* `OutputFile::disagreement` returns `None` when `data` is `None`
  (`herd-proto/src/outputs.rs:260-262`), so this is the **inline path
  only** — A says so and is right.
* `store.rs:1711` is the **only** caller of `OutputFile::disagreement`
  in the hub. `OutputsOut::disagreement` (`outputs.rs:398`) exists and
  is called from nowhere outside its own tests. There is no earlier
  gate that would refuse the whole verdict, so the path is reachable.
* `record_outputs` returns `Ok`, so `record_the_end`
  (`server.rs:4877-4885`) succeeds and the verdict **is** acknowledged.

**Wrong citation.** A cites `herdctl/src/main.rs:1322` for
`missing = files.iter().filter(|f| !f.is_held()).count()`. It is at
**`herdctl/src/main.rs:1283`**; 1322 is inside `fn wanting`. The claim
is right, the pointer is not. `code_for` is at `:591` as A says.

**Narrowing.** The `missing` count at `:1283` is reached only after
`let Some(directory) = &keeping.outputs_to else { … }`, so a `herdctl
run` without `--outputs-to` never evaluates it. The conclusion is
unchanged — such a run exits on the job's own code either way — but the
finding is about *every* inline run, not only fetching ones, and A's
text implies the count is what lets it through. What actually lets it
through is that **nothing anywhere counts a manifest entry that was
never written**.

### 1.3 §5.3 `files.part/` escapes the budget — **holds, F**

`sweep_now` (`store.rs:642-667`) measures `directory_bytes(files/)`
(`KEPT_FILES_DIR`) and removes `files.gone/` (`OUTPUT_GONE_DIR`). It
never names `OUTPUT_PARTS_DIR`. The constant is used at exactly three
places (`store.rs:1835`, `:1912`, `:1977`) and `clear_output_parts`
(`:1976`) has one production caller, `server.rs:3771`, on
`ChunkLanded::Complete`. An abandoned collection is therefore never
cleared.

**Strengthened.** A marks "a later runner would be told to resume a
transfer whose source is gone" as a consequence; it is **F**:
`output_holdings` (`store.rs:1912-1925`) falls back to the part file's
length, `collections_owed` (`store.rs:2367`) is read on every runner
join (`server.rs:2415`), and the runner answers
`"the hub is waiting for the files of {job} attempt {n}, and this
runner holds no collection for it"` (`herd-runner/src/lib.rs:1397-1411`).
The hub also has a bound on the *requester's* wait
(`collection_silence`, `server.rs:4586`) but that bound releases the
verdict and does not touch the bytes.

### 1.4 §5.4 `outputs/` unbounded on the runner — **holds, F, and A's H
is provable**

`spool_path` (`herd-runner/src/outputs.rs:137-139`) under
`SPOOL_DIR = "outputs"` (`:142`), built at `herd-runner/src/job.rs:538`.
Removed only in `files_acknowledged` (`herd-runner/src/lib.rs:1421-1446`).
`reclaim_a_past_life` (`herd-runner/src/reclaim.rs:391-446`) sweeps
processes, `jobs/` and cgroups; `outputs/` appears in no reaper.

**A's §5.4 closing H — "a collection cannot survive a runner restart" —
is F.** `inner.collections` is written at exactly one site,
`lib.rs:1275`, inside the live session; after a restart it is empty, and
`files_held` (`lib.rs:1393-1412`) says so in words. The bytes are on
disk, the map that knows about them is not, and nothing reads the
directory back. So the verdict survives a runner restart (D34 spool) and
the collection does not — and the hub keeps a `files.part/` for it for
ever (§1.3). The two leaks are **one failure seen from two ends**, which
neither report says.

### 1.5 §4.2(h) the stale M15 comment — **holds, F, and understates it**

`server.rs:1793-1804` says *"Until M15 there is no token to check
(E-48) … The gate is exactly as strong as M15's token, which does not
exist yet"*. The token exists: `admission_refusal`
(`server.rs:2043-2060`) calls `store.admit_runner`
(`store.rs:1555-1557`) → `Roster::admit` (`roster.rs:234-256`), a
salted SHA-256 compared with `same_bytes`. There is **no flag that
disables it**: `Role::Runner` has no bypass arm, an unknown name is
`NotApproved`, a missing token is `NoToken`.

**Correction to A's severity.** A calls this "an observability defect,
not a correctness one". That is too kind in one direction and too harsh
in the other. The comment does not weaken the running code — so not a
correctness defect, agreed — but it understates a **shipped security
control** on the one gate that decides whose verdict is believed. A
reader who trusts it will either add a redundant control or refuse to
rely on a control that is already there. Treat it as a documentation
defect with a security blast radius, not as a note.

### 1.6 The rest of A's F claims — spot-checked

| A's claim | Verdict |
|---|---|
| §1.1 only `durable` writes (`store.rs:26-27`) | **Holds**, with one omission: `fs::rename` at `store.rs:579` is a fourth atomic-half call A did not list beside `:1882`, `:1893`, `:1981`. `roster.rs:484` is inside `mod tests` (`roster.rs:393`). Claim intact. |
| §1.2 `waiting` at `server.rs:1011` | **Holds** (`server.rs:1011`). |
| §2.1 the transition table (`store.rs:2475-2499`) | **Holds**, faithfully transcribed, including `Interrupted → Queued`. |
| §2.3 the log/`log.seq` crash window | **Holds**: `log_at` (`store.rs:1213-1232`) and the contract comment at `:1202-1212` say exactly what A says. |
| §3.1 recorded before acknowledged | **Holds**: `server.rs:3903` `if !record_the_end(…) { continue; }`, ack at `:3909`; `record_the_end` at `:4851-4886`. |
| §4.2(c) the dedup key's fields | **Holds**: `job.rs:621-636` is (command, labels, commit) and nothing else. |
| §4.2(d) `coalesce`/`dedup_key` inert | **Holds**: `herdctl/src/declaration.rs:265`, `:267`, `:492-493`, `:626-627`; **zero** occurrences in `herd-proto` or `herd-hub` (the hub's `dedup_key` at `job.rs:621` is its own, unrelated computation). `docs/coverage.md:59` does say E-42 is M14/planned. |
| §5.5 `unwritten` never pruned by a terminal state | **Holds**: `unwritten` appears at `server.rs:936`, `:1580`, `:1605`, `:4933` only; the sole removal is `stall_is_over` (`:1602-1607`), reached only on `Ok`. The Err arm (`:4991-4993`) prints and keeps. A's sequence is correctly marked **H**. |
| §5.6 `jobs/` grows without bound | **Holds**: no job-directory removal anywhere (`remove_tree`/`remove_dir_all` at `store.rs:579`, `:597`, `:669`, `:1981`, `:2718` are `files/`, `files.gone/`, `files.part/`); `next_sequence = max + 1` at `store.rs:879`. |

**Nothing in audit A is refuted.** One citation is wrong (§1.2), one
list is short by one entry (§1.6), one **H** should be an **F** (§1.4),
and one severity is mis-stated in both directions (§1.5).

---

## 2. Audit B — every F taken back to the code

### 2.1 §1.3 no insulation between persistence and the reactor — **holds, F**

`grep -rn "spawn_blocking\|block_in_place" --include=*.rs .` returns two
hits: `herd-runner/src/job.rs:1222` and `herd-hub/tests/support/mod.rs:340`.
B says "the only hit in the workspace is `herd-runner/src/job.rs:1222`";
the test helper is a second hit, which does not affect the claim —
**no production code in `herd-hub` moves anything off the runtime.**
`dispatch_queued_jobs` (`server.rs:2492`) and `dispatch_a_pass`
(`:2509`) are synchronous.

**B's call-site list is short by two, and the two it misses are the
important ones.** B names eleven. There are thirteen, and the missing
pair is `server.rs:2419` (a pass on **every runner join**, i.e. every
redial) and `server.rs:2434` (a pass on every runner departure). B's
§2.2 feedback loop is attributed to `server.rs:3591`
(`relay_from_runner`, per frame); the actual redial site is `:2419`,
which runs **before** any frame is relayed. The mechanism B describes is
real and its citation is wrong. See §4.2.

### 2.2 §1.2 locks, §3.1 bounds — **holds, F**

`grep -rn "Semaphore\|max_connections" herd-hub/src/` → nothing.
`type Outbox = UnboundedSender<FromHub>` at `server.rs:246`. 90
`println!`/`eprintln!` in `server.rs` (B says 90; `grep -c` agrees).
No lock-order inversion found by me either; I did not look as hard as B
and record that as **I** on my side.

### 2.3 §2.1 a pass is O(queue depth) and runs per event — **holds, F,
with one narrowing**

`dispatch_a_pass` → `store.unfinished_jobs()` (`server.rs:2510`, again
at `:2521`). `unfinished_jobs` (`store.rs:2312-2324`) iterates
`live.pending` and calls `read_stored`, which parses the same text three
times (`store.rs:1142`, `:1150`, `:1160`). All confirmed.

**Narrowing.** `pending` is **open** jobs (queued, assigned, running,
interrupted), not all jobs on the volume. So the O(N) term is in open
work, and A's §5.6 unbounded `jobs/` does **not** feed it. That matters
because it bounds how bad the dispatch term can get — and it is exactly
why the term I found in §4.2, which *is* O(all jobs ever), is worse.

### 2.4 §2.1 the watchdog's steady O(N) read — **holds, F**

`attempts_in_the_air` (`server.rs:2459-2470`) calls `unfinished_jobs()`
unconditionally; `watch_for_a_lost_runner` calls it at `:5050` every
`watchdog_every()` = `(lost_after / 20).clamp(50 ms, 1 s)`
(`server.rs:445`) = **1 s** at the default `LOST_AFTER = 60 s`
(`server.rs:79`).

### 2.5 §5.2 the hub has no stop path — **holds, F**

`grep -rn "tokio::signal\|SignalKind" herd-hub/` → nothing. H1, H2 and
H3 are correctly marked **H**; I can neither confirm nor refute them by
reading, and §6 is the only way.

### 2.6 §7.1 invariant 2 is enforced only in the hub — **holds, F, and
is the strongest finding in either report**

`herd-runner/src/lib.rs:2745` spawns a task per `AssignJob`
unconditionally. `take_job` (`:1151-1166`) returns `None` only for
`inner.draining` and `!inner.switch.claims()`. No count, no load, no
memory gate. `grep` for `jobs_at_once|max_jobs|concurrency` across
`matcher.rs`, `server.rs` and the runner finds only `readings_at_once`.

The uncounted-kind claim is also **F** and tighter than B states it:
`CapabilityKind::Shared.holders_allowed()` returns `None`
(`herd-proto/src/capability.rs:213-219`), and the slot gate is
`if let Some(slots) = capability.kind.holders_allowed()`
(`matcher.rs:1229-1239`) — so for `Shared`-only labels the gate is not
merely satisfied, it is **not evaluated**. With no declared memory,
`admission` sets `memory_mib = 0` (`matcher.rs:603-605`). Such a job
passes both gates by not reaching either.

### 2.7 §7.2 item 4 one unwritable record aborts a pass — **holds, F**

`say_it_waits` (`server.rs:3415`) propagates the `record_waiting` error
at `:3441-3443`, and its three callers inside the pass loop
(`server.rs:2577`, `:2618`, `:2640`) all use `?`. The pass ends; every
job behind it is unjudged; the only trace is a line on stderr.

### 2.8 §9 the two unstable tests — **holds, F on `follower_alive`;
narrowed on `slots`**

`follower_alive.rs`: `longest` is the max gap over `heard`
(`:216-220`), and `heard` is pushed after the gate opens (`:165`).
`spread` is `breathed_at[7] - breathed_at[0]` (`:200`), the breaths
alone. `assert!(longest <= BREATH * 10)` at `:225` and
`assert!(longest < spread / 2)` at `:231`. B's reading is exactly right
and I would sign it: the second assertion compares a quantity measured
over one interval against a figure derived from a strictly shorter one.

`slots.rs`: **B's diagnosis goes beyond what the code supports.** The
author's own comment (`slots.rs:266-277`) names the residual window as
*a submission landing while the fleet is momentarily empty, refused
outright by `no runner is connected`*, and points at D88 — a
**refusal-should-be-a-wait** question. B recasts it as "the hub decides
a link is dead from a wall clock evaluated on a reactor thread its own
passes can monopolise". That reading is plausible and is marked as B's
own, but the test stands two seconds of silence against **one runner and
no queue** (`slots.rs:207-213`), which is not a state in which a pass can
monopolise anything. B's framing should be **H**, and it is not the
mechanism the test's author identified. Keep the test on the register
for D88, not for I06.

### 2.9 §13 D171 says nothing about quadratic reads — **holds, F**

I read D171 in full. Its words are "a cost that had always been linear
in the queue" and "per queued job per pass". B is right, and right to
say so.

**Nothing in audit B is refuted either.** One call-site list is short by
two in a way that matters (§2.1), one O(N) is in open jobs rather than
all jobs (§2.3), and one diagnosis of a flaky test should be **H**
(§2.8).

---

## 3. D171: cause or symptom

There is no contradiction to arbitrate (§0.1). The question underneath
it is worth answering, and both agents in fact answer it the same way.

**What D171 itself says. F.** The entry has a section headed *"The
defect: a figure that moves inside a sentence that is compared"*, then
the sentence **"The rest follows without any further defect"**, then a
section headed **"The deeper defect, deliberately not fixed here"**
which states that "every dispatch pass, every store write, runs on the
runtime" and defers it with its reasons.

**What B says. F.** `03b:456-457`: *"the fix strikes a real and
well-identified cause, and it is a symptom fix for the incident"*, and
then *"I agree with the decision to defer it and I disagree with
nothing in that section"*. B holds both halves in one sentence, exactly
as D171 does. There is no disagreement between B and D171 either.

**My verdict, under the two definitions.**

* **Proximate cause of the write volume: D171 fixes it, at the cause.
  F.** The defect was that `record_waiting` judged news by comparing a
  sentence containing a figure about the queue. The fix changes the key
  to a fact about the job (`server.rs:3422-3436`). Nothing is masked,
  no counter is introduced, and the removed writes were genuinely
  non-events. The shape is the same as D156's and it deserves the same
  credit.
* **Cause of "a hub that answers nothing": D171 does not touch it. F.**
  Every link of the chain is intact at `0a2cf73`: the pass is
  synchronous (`server.rs:2509`), it is called from thirteen async
  sites, one of them the redial itself (`server.rs:2419`), it reads
  O(open jobs) with three parses each, and the hub has no executor but
  its reactor. D171 reduced one term of the cost by three orders of
  magnitude; it removed no link.

**So the useful formulation is not "cause or symptom" but: D171 reduced
the loop's gain and left its structure.** That is what both D171 and B
say, and nothing in A contradicts it.

**What it changes for the next step. H.** A loop whose structure is
intact re-arms when any remaining term grows. B names the surviving term
as `N·r` (open jobs × read latency). §4.2 adds a second, larger one that
neither audit costed. Whether either is enough to re-arm the loop on a
real store is **I**, and §6.1 is the only thing that settles it. The
decision this feeds is not "was D171 right" — it was — but "is the
deferred item in D171's last section now urgent".

---

## 4. Overlaps, and what the split left unread

### 4.1 The same defect under two names

| Audit A | Audit B | They are |
|---|---|---|
| §5.1 "one process-wide mutex … synchronous under the lock, called from async contexts" (`03a:442-453`) | §1.3 "no `spawn_blocking` … everywhere that matters, by design" | **The same finding.** A names it, marks it **H**, and hands it to B. The hand-off worked; the reports do not collide. |
| §6.1 "a policy change and a framing change touch the same lock" | §1.2 "the problem is not ordering, it is duration" | **Two readings of one fact.** A's is a maintainability argument, B's a latency one. Both true, neither refutes the other. |
| §5.3 `files.part/` abandoned on the hub | — | **One failure, two ends.** §1.4 above: A filed them as two leaks in two places. They are the hub side and the runner side of a single event — a runner that dies mid-collection. A fix to one without the other leaves the pair. |
| §5.6 `jobs/` grows without bound | §2.1 a pass is O(queue depth) | **Not the same, and the difference matters.** §2.3 above: the pass reads `pending`, so A's growth does not feed B's term. It feeds a different one — §4.2. |
| §3.4 at-least-once execution (partition → two attempts) | §4.3 "the only loop that can repeat without a budget is drop-link-redial" | **Two consequences of one partition.** Nobody owns the composite: during the §2.2 state, the hub's own slowness produces A's double execution. Neither report draws the line. |

### 4.2 The blind spot the split created

**F.** `collections_owed` (`store.rs:2367-2386`) lists **every directory
under `jobs/`** — `durable::names_in(&self.jobs_dir())` at `:2374` —
and calls `collection_owed` for each, which reads that job's record
(`:2390`), i.e. one file read and three serde parses per job **ever
submitted on the volume**. It is called from two production sites:

* `server.rs:2415`, `ask_for_the_files_still_owed`, on **every runner
  join** — four lines before the dispatch pass at `:2419`;
* `server.rs:4384`, inside `send_the_files`, to answer **one**
  requester's `AskJobFiles` for **one** job (`server.rs:6245`).

**F.** `sweep_now` (`store.rs:642-667`) does the same walk — every job
directory, plus a recursive `directory_bytes` (`store.rs:2692-2707`) per
job — and is reached from `held_more` (`store.rs:530-540`) whenever the
running total crosses the budget. `held_more` is called from
`record_outputs` (`store.rs:1748`), i.e. on every verdict carrying inline
bytes, and from `write_output_chunk` (`:1903`), i.e. on every completed
streamed file. There is a re-entrancy guard (`store.rs:2352-2358`) and
no rate limit.

**F.** Nothing ever removes a job directory (A §5.6, re-verified at
§1.6). So both walks are **O(all jobs ever submitted)** and grow
monotonically for the life of the volume.

**Why neither audit has it.** A owns storage and found the growth
(§5.6) but was told progression is B's, and explicitly did not read the
dispatch loop (`03a:667-669`). B owns progression and costed the pass,
the watchdog and the aging burst — all of which read `pending` — but
output storage is A's, so B never costed the two walks that read the
whole volume. **The split is mine to report and the orchestrator's to
own: this term sits exactly on the seam, and it is larger than the term
B identified.** The consumer's incident had 184 open jobs; `jobs/`
after a week of their traffic has thousands.

**Partially known.** `docs/status.md:73` already records one of the two
paths — *"The hub re-reads every job record on each 200 ms poll of a
waiting `--outputs-to`: about 0.9 s of hub time per poll at 100 000
records (D152)"* — and board **ORCH-021** tracks it, which B cites as a
"cousin". Neither the **redial** path (`server.rs:2415`) nor the
**sweeper** path is recorded anywhere I can find, and neither is
connected to B's feedback loop.

**H, and it is the hypothesis I would most want tested.** B's loop is
drop-link → redial → pass. The redial path at `server.rs:2415` pays
`collections_owed` **before** the pass at `:2419`. If that scan costs
more than the pass — which it does as soon as the volume holds more
closed jobs than open ones — then B's §2.2 argument understates the
loop's gain, and the surviving term is not `N·r` over the queue but
`T·r` over the whole volume, where `T` only ever grows. Confirmed by
§6.1 with a third axis; refuted if the scan is cheap enough at realistic
`T` that it never dominates.

### 4.3 What neither audit looked at, and why

* **The requester trust boundary. I.** `admission_refusal`
  (`server.rs:2061-2070`) authenticates a requester against one shared
  token, and *"a hub with no requester token authenticates no
  requester"*. Who may submit, cancel, follow or fetch another
  requester's job is in neither mandate: A took identity of *jobs*, B
  took the scheduler. **The split left the requester side of E-48
  unaudited.** This is a gap in the mandate, not in either agent.
* **Transport. I.** `herd-hub/tests/tls.rs` exists; neither §4.1 nor
  §4.2 covers TLS, certificate handling or the plain-`ws` default.
* **`herdctl`'s own behaviour under failure. I.** A read its exit codes
  where outputs passed through; nobody read its wait loop, its retry or
  its signal handling.
* **The consumer's side. I, and correctly so.** D161 forbids it. Both
  audits say so; I agree and add nothing.

---

## 5. One ranked list

Ordered by what to treat first = (consequence if it fires) × (how
reachable) ÷ (how visible it is when it fires). **Proved** means a
`file:line` settles it; **needs an experiment** means no amount of
reading will.

| # | Finding | Where | Status |
|---|---|---|---|
| 1 | **Invariant 2 is enforced only by arithmetic in another process.** The runner spawns a task per `AssignJob` with no count, load or memory gate, and the hub's arithmetic has at least one kind it does not count (`Shared`-only labels, no declared memory). | B §7.1; verified §2.6 | **Proved** (`herd-runner/src/lib.rs:2745`, `:1151-1166`; `matcher.rs:1229`, `:603-605`). The *consequence* needs §6.3. |
| 2 | **The commit link of the identity chain is open-loop.** No frame carries the revision the workspace was at; no oracle exists for "this verdict is about commit X". | A §4.2(a); verified §1.1 | **Proved as an absence.** Needs Alexa: it is a protocol field. |
| 3 | **Persistence and the scheduler share the reactor, and nothing bounds what a pass costs.** D171's own deferred item. | B §12.1; verified §2.1 | **Proved** as a structure. Whether it is *urgent* needs §6.1. |
| 4 | **Two whole-volume walks on hot paths, growing for ever.** `collections_owed` on every runner redial and every `AskJobFiles`; `sweep_now` on every verdict once at budget. O(all jobs ever). | **New — §4.2** | **Proved** (`store.rs:2374`, `:642-667`, `:530-540`; `server.rs:2415`, `:4384`). Its weight needs §6.1. |
| 5 | **An inline output file whose bytes contradict their digest is dropped from the manifest and the run exits 0.** | A §5.2; verified §1.2 | **Proved.** Settled definitively by §6.3, which costs seconds. |
| 6 | **A runner that dies mid-collection leaks at both ends, for ever.** `files.part/` on the hub (outside the budget), `outputs/` on the runner (no reaper), and the collection cannot survive a runner restart. | A §5.3+§5.4; verified §1.3, §1.4 | **Proved**, including the part A marked H. Size needs §6.4. |
| 7 | **One unwritable record aborts a whole dispatch pass, with nothing on any record to say so.** | B §12.5; verified §2.7 | **Proved.** Touches I10. |
| 8 | **The hub has no stop path**, and an unnamed restart write-burst at the worst moment. | B §5.2; verified §2.5 | H1/H2 **need §6.2**; "no signal handler" is proved. |
| 9 | **`Peers::unwritten` is never pruned by a terminal state**: a permanent error line about a job that is fine. | A §5.5; verified §1.6 | Mechanism proved; the sequence is **H**. |
| 10 | **The stale M15 comment** on the gate that decides whose verdict is believed. | A §4.2(h); verified §1.5 | **Proved.** One-line fix, no risk. |
| 11 | **`herdctl` accepts `coalesce`/`dedup_key` and drops them.** | A §4.2(d); verified §1.6 | **Proved.** Contract surface, not a bug. |
| 12 | **`follower_alive.rs:231` is a badly written assertion** (ORCH-022). | B §9; verified §2.8 | **Proved.** |
| 13 | **The dedup key does not distinguish executors or caps**; **two attempts share one log file**; **no migration code exists**; **unbounded `Outbox`**; **unbounded connections**. | A §4.2(c), (g), §5.7; B §3.1 | Proved as facts, **H** on reachability. |

Items 5, 10, 11 and 12 are small, local and independent of every
architectural question. They do not need the mandate to conclude.

---

## 6. The experiments: overlap, and the minimal sequence

A proposes six (`03a:640-648`), B five (`03b:645-701`). **No two are the
same experiment.** Four pairs share a rig:

| Share a rig | Why |
|---|---|
| B-10.1 (pass cost vs store latency) + B-10.4 (submission burst) | One in-process harness: the delegating store with injected latency, plus a read counter. |
| A-C (kill runner mid-collection) + A-D (abandon a collection at budget) | One hub at its output budget, one runner killed mid-transfer. Measures both leaks of finding 6 at once. |
| B-10.2 (recovery under a deep queue) + B-10.3 (runner overloaded) | Both need real binaries on an isolated host; different loads, same setup cost. |
| A-E (partition → double execution) + B-10.2 | Both need a real hub and runner and a controllable link. |

**One experiment should be dropped. A-F** ("a declared command whose
`fetch` exits 0 without reaching the commit") settles nothing that
§1.1 has not already settled: the field does not exist, so the oracle
cannot exist, and running it would demonstrate rather than discover.
Keep it only if Alexa wants to *see* the failure before approving a
protocol change — which is a legitimate reason, but it is a
demonstration, not an experiment.

### The minimal sequence — four runs, in this order

**6.1 — The cost surface, in process.** B-10.1 ∪ B-10.4, with **a third
axis neither audit asked for**: the number of **closed** job directories
`T`, varied independently of the open-job depth `N`. Delegating store
with injected latency `d` (the pattern exists at `server.rs:6928`), and
the D171 oracle sampled continuously. Measure `worst_turn(N, T, d, W)`,
the `job.json` read count per pass, and the read count per
`collections_owed`.
*Settles:* finding 3's urgency, finding 4's weight, B's §2.1 quadratic
claim, §4.2's H about which term dominates, and I06's missing "slow
store" axis. *Cost:* no network, no real binaries, isolated machine but
not a loaded one. **Run this first: it is the only one whose result
changes the strategy choice.**

**6.2 — Recovery under a deep queue, real binaries.** B-10.2, with two
additions: count `collections_owed` calls and their duration on the
redial path (`server.rs:2415`), and let the runner's 60 s silence limit
actually expire so the loop closes.
*Settles:* B's H1 and H2, I08's missing half, and whether §4.2's term
shows up in a real redial storm. *Cost:* an isolated host, minutes.
If 6.1 shows `T·r` dominating, this is where it either appears or does
not.

**6.3 — The two cheap, decisive ones, together.** A-A (one `JobFinished`
with a wrong inline `sha256`, against a real hub, read the exit code)
and B-10.3 (a stub hub sending 50 then 250 `AssignJob` frames at one
runner, count cgroups and host load). Unrelated findings; both are
short; both convert a proved absence into a measured consequence.
*Settles:* finding 5 outright and finding 1's real-world severity, which
is the strongest single argument for where the next boundary goes.
*Cost:* A-A is seconds; B-10.3 needs an isolated host it is allowed to
hurt.

**6.4 — The leak size.** A-C ∪ A-D on one rig.
*Settles:* finding 6's magnitude, and whether `files.part/` really
escapes the budget by the amount the code implies. *Cost:* a few hundred
MiB of scratch.

**Deferred: A-B** (a *forced* crash window between `write_record` and
`append_event`) and **B-10.5** (a slow follower against the unbounded
`Outbox`). Both are real gaps; neither changes any decision on the
table now.

All of these are **T2**. None was run. None should be run on this host.

---

## 7. Recommendation

The mandate's options A/B/C are not in the repository and I did not see
them; what follows is framed against the two architectures **D171 itself
names** in its last section — `spawn_blocking` at the call sites, or a
dedicated pass thread fed by a channel — plus the third option of
bounding the inputs instead. If the mandate's A/B/C are different,
re-read this against them. **I** on the mapping.

**Recommendation: experiment 6.1 first, before any architecture is
chosen, and ship findings 5, 10, 11 and 12 in parallel without waiting
for it.**

*Why first.* Every architectural option on the table is a way of paying
for the same thing: the cost of a pass, off the thread that must answer
the network. D171 already bought three orders of magnitude on one term
of that cost and explicitly declined to buy the structure. The open
question is not which structure — it is **whether the surviving cost is
still large enough to matter**, and §4.2 has just added a term that
neither audit costed and that grows for ever. Choosing between
`spawn_blocking` and a pass thread before measuring `worst_turn(N, T, d,
W)` is choosing between two prices for a quantity nobody has weighed.
D171's own words — "three orders of magnitude of headroom on the figure
that mattered" — are an argument for measuring before paying again.

*Cost.* 6.1 is in-process: no network, no real binaries, no container,
one isolated machine for perhaps an hour, and the delegating-store
pattern it needs already exists in the tree (`server.rs:6928`). It is
the cheapest of the nine proposed experiments and the only one whose
result reorders the list in §5.

*What it decides.* If `worst_turn` stays inside a requester's patience
across the realistic surface, the deferred item in D171 is **correct but
not urgent**, finding 1 (invariant 2 at the runner) becomes the first
thing to build, and the architecture question waits for a milestone of
its own. If it does not — and especially if the `T` axis dominates —
then the answer is not an architecture at all in the first instance: it
is to stop walking the whole volume on a redial (finding 4), which is a
local change, before paying for a new executor. **That is the outcome
I would bet on**, and it is why I would not let the architecture
question be decided first.

*What not to wait for.* Findings 5, 10, 11 and 12 are local, carry no
architectural commitment, and two of them are one line. Holding them
behind a mandate is cost with no benefit. Finding 2 (the commit field)
is Alexa's and should be put to him now, independently: it is a protocol
version bump, it has the longest lead time of anything here, and it is
the only finding that speaks to the incident the consumer actually
reported.

---

## 8. What this review does not establish

* **I** — nothing was measured. Every claim is a reading of `0a2cf73`.
  No gate was run except `scripts/check-docs.sh`.
* **I** — I verified the **F** claims of both reports and spot-checked
  their **H** claims. I did not re-derive their **H** reasoning
  independently, so an error inside a hypothesis would survive this
  review.
* **I** — I did not look for findings outside the two mandates except
  where a seam between them made one visible (§4.2). The gaps in §4.3
  are named, not filled.
* **I** — the mandate's strategy options A/B/C. §7 is framed against
  D171's own two architectures and may not map onto them.
* **H** — §4.2's claim that the whole-volume walk can dominate the
  surviving cost. It rests on `T ≫ N` after sustained use, which is a
  property of the consumer's traffic and not of this tree.
