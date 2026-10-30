# consumer-answers-e14-retention — handoff 2026-10-30

- **Item:** ORCH-302 neighbour; no board id of its own (the brief is the
  origin, and the row this task proposes is drafted below)
- **Agent:** herd-step
- **Branch / commit:** `consumer-answers-e14-retention` / see the agent's
  report for the sha, based on origin/main `71efccf`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

1. **`docs/status.md:89`-`:90` replaced, two lines for two lines.** It
   said *"`coverage.md`'s E-12 and E-17 rows are false and are not
   recounted here (ORCH-302)"*, which `e12-e17-recount` (`ee28e15`,
   unlanded) makes false: that branch recounts them and keeps both at
   `part`. The replacement is the one drafted verbatim in that branch's
   own handoff, `history/2026-10-30-e12-e17-recount.md:519`-`:521`, and
   is taken from there unaltered. The file is still **90 lines**, its
   enforced cap, and the replacement **adds no reason** — see *Drafted*
   entry 2, the orchestrator's ruling that `status.md` carries state and
   pointers and that reasons live in `docs/decisions.md`.
2. **The consumer's two answers of 2026-10-30 are drafted as one
   decision entry**, in D248's shape, as a **consumer answer** and not
   as a Herd decision: E-14 **withdrawn, not waived**, with their reason
   beside the withdrawal; and the hub's record retention **cut from
   v1.0.0, following in v1.1**, on their two stated conditions, the
   first in its **corrected** form. Verbatim and fenced below.
3. **Every figure in that entry was read off the tree, not off the
   brief**, and each carries its instrument. Sources checked at
   `71efccf`: `docs/disk-policy.md:318` (R14), `:320` (R16), `:152`
   (the derived rate), `:88`-`:89` and `:96`; `docs/decisions.md` D248,
   D249, D254, D271. The provenance table is in *Risks and findings* 1.
4. **Nothing else was touched.** No crate file, no `docs/coverage.md`,
   no `docs/decisions.md`, no `docs/orchestration/board.md`, and
   nothing on `cut-v1.0.0`. The diff is two files, both under `docs/`.

## Not done, and why

1. **`docs/coverage.md`'s E-14 row was not moved off `planned`**, and
   the brief forbade it twice over: `e12-e17-recount` owns that file and
   is unlanded, and retiring a must is the consumer's act (D248, D249).
   **It is now unblocked** once the drafted entry lands — the row is
   **`docs/coverage.md:98`**, `| E-14 | must | warm caches ready before
   job | M12 | planned | …`, whose note ends *"gated on their
   replication (M18)"*. The board row for it is drafted below.
2. **`docs/decisions.md` and `docs/orchestration/board.md` were not
   edited.** The entry is drafted `D-NEXT`; the merger allocates, and
   gaps stay (D148, invariant 8). Highest on `main`: **D272**,
   **ORCH-302**. Two branches are unlanded ahead of this one
   (`e12-e17-recount`, and `cut-v1.0.0` which Alexa has not signed).
3. **The two false in-crate doc comments were not touched** —
   `herd-hub/src/registry.rs:19`-`:23` and
   `herd-hub/src/job.rs:1133`-`:1135`. They span two crates and owe all
   four Rust gates, and a row for them is already drafted in
   `history/2026-10-30-e12-e17-recount.md:511`. Not re-drafted here, so
   that the merger has one copy and not two.
4. **The four Rust gates were not run.** Prose only.
5. **`cut-v1.0.0`'s own drafted entry was not reconciled**, and it
   overlaps this one. See *Risks and findings* 2 — this is the finding
   the merger must read before pasting anything.

## Gates

**Prose only. The four Rust gates were NOT RUN locally** — `cargo build
--workspace`, `./scripts/test.sh` with its skips printed and never
counted as passes (D27), `clippy --workspace --all-targets -- -D
warnings` and `fmt --all --check`, each under `nix develop --command`.
What covers them is the batch's own `./scripts/gate.sh` run at
integration, which is `herd-merger`'s and not this agent's. No crate
file is touched by this branch.

**The detector was proven before its silence was trusted.** An
80-column line was appended to `docs/status.md`, `./scripts/check-docs.sh`
was run, and it named both faults — the cap and the width — then the
line was removed and the run went green. Verbatim, the red run:

```text
docs/status.md is 91 lines, limit 90 (it is state, not a journal)
docs/status.md has lines over 79 columns:
    docs/status.md:91 (80 cols)
docs check failed
```

and the same command on the reverted tree, before this file existed:

```text
docs ok
```

**The last runs are named, not quoted**, because they happen on the tree
that includes this file: `./scripts/check-docs.sh` and
`./scripts/check-orchestration.sh`, both from the worktree root, their
output in the agent's report and in
`<scratchpad>/wt/consumer-answers-gates.txt`. A red last run is
reported red and the branch is not pushed.

**No skip.** `./scripts/test.sh` was not run, so no `HERD-SKIPPED` line
was produced or copied; a skip is never counted as a pass (D27).

## Risks and findings

### 1. Every figure in the drafted entry, with its instrument

Each was read at `71efccf` and none was taken on the consumer's
machines by anyone in this project (invariant 9, D161).

| Figure | Source | Instrument |
|---|---|---|
| ≈ 346 MiB of records, logs and events | `disk-policy.md:320` R16 | the consumer's own read-only `du -sk`, 2026-10-29 ~16:30Z — **disk blocks** |
| 3 334 jobs | `disk-policy.md:318` R14, `:306` R2 | the consumer, 2026-10-27 and 2026-10-29 |
| ≈ 104 KiB a job | `disk-policy.md:320` R16, `:125` | derived from R16 over 3 334 |
| 0.32 % of the state root | `disk-policy.md:89`, `:96` R16 + D | derived |
| 105.57 GiB of collected files, 99.68 % | `disk-policy.md:318` R14, `:88` | `herd-hub report`, 2026-10-29 |
| 7.57 GiB/day over 14 days | `disk-policy.md:152` | derived, marked `[D, from R12]` |
| 1,443,667 bytes apparent vs 2850816 blocks, 1.97× | `decisions.md` D271 | two tools on a frozen copy of **our own queue root**, 2026-10-30 |

**One correction to the brief's wording, and it matters for invariant
9.** The brief says the four previous figures read nought *"on one real
root"*. D271 says which root: **our own queue root**, measured frozen on
the same day, **not** a consumer machine. The entry says *our own queue
root* for that reason, and the brief's own figure, 1,443,667 bytes, is
the one D271 gives for it. Nothing else in the brief needed changing:
all seven other figures reconcile exactly, units included.

### 2. `cut-v1.0.0` already drafts an entry over the same two facts

**This is the finding the merger must read first.**
`history/2026-10-30-release-v1.0.0.md:418`-`:481`, on `cut-v1.0.0`
(`f3e9df0`, unlanded, unsigned under D210), drafts
`## 2026-10-30 — D-NEXT v1.0.0 is cut; the three withdrawals and the
named test`. Its items **3** and **4** are E-14's withdrawal and the
retention cut — the same two facts this entry records.

**They are not duplicates and the difference is not cosmetic.** That
entry opens *"Choice, Alexa, 2026-10-30"* and folds all five named
exclusions into one act of Alexa's. **Two of those five are the
consumer's answers, not his**, and D249's last clause says their answers
are information to weigh and carry no authority over Herd; recording a
consumer answer as an owner's choice is the confusion D248 wrote
*"recorded as a consumer answer, not as a Herd decision"* to prevent.
This entry records the two answers; that one records the cut, the body
and the named unstable test.

**So the merger has a choice to make and must not make it by
accident:** land both, and amend the release entry's items 3 and 4 to
cite this entry rather than restate it; or land this one and drop those
two items from the release entry. **Do not paste both as written** — the
tree would then carry E-14's withdrawal twice, once attributed to Alexa
and once to the consumer. Recommended: land both with items 3 and 4
reduced to a citation, because the release entry is also about the cut
and the test, which this one says nothing about.

### 3. The scope figure moves, and the published headline is not ours

Mechanically, with E-14 withdrawn the released-from set is **three**, so
`docs/coverage.md`'s tally re-derives as **36** wholly on `main` + **5**
`part` (E-12, E-16, E-17, E-37, E-42) + **0** with none of it + **3**
released from = 44, and D248's *scope* figure 42 = 44 − 2 becomes
**41 = 44 − 3**. That is arithmetic. **What is published as a headline
is not**: the release and its notes are Alexa's (**D210**), and
`cut-v1.0.0`'s own finding 1 already records that *"42 of 44"* is a
scope figure and never a count of what is met. The drafted entry states
the arithmetic and leaves the headline his.

### 4. The two apparent-size figures still err in opposite directions

D271 records it and the entry repeats it because the retention decision
rests on comparing two numbers taken with two instruments. Section 5
sums **apparent size**, a **floor** on occupancy, measured **1.97×**
below blocks on one small-file population; the consumer's own ≈ 346 MiB
is `du -sk`, which is **blocks**. **At their migration section 5 will
print roughly half their `du` figure with nothing having shrunk**, and
the natural reading of a root halving is that retention got less urgent.
Section 5 now prints the unit and that caveat itself; the entry says it
too. The paired hazard is `bytes_written`, which **overstates** on a
sparse file (D263, ORCH-278): same unit, same word, opposite bias.

### 5. What could still be wrong here

- **The consumer's quoted sentences reach this branch through the
  brief**, not through a file in the tree. Their E-14 reason — *"the
  requester supplies its own warm cache through its executor hook; Herd
  need not know the shape of a cache"* — and condition 2's *"so that we
  see the growth instead of discovering it"* are quoted as given; only
  the second is independently in the tree, in D271. D249's channel
  reports nothing back, so there is no receipt to check either against.
- **Condition 1 is recorded in the orchestrator's corrected form**, as
  the brief instructs, and D254 independently supports it: the floor is
  derived from `statvfs` on the whole volume at every trigger, while
  what goes is still D152's four keys read down to
  `min(--output-budget, held + free − floor)` — output collections.
  Nothing in the tree contradicts the corrected form.
- **E-14's note still reads *"gated on their replication (M18)"*** and
  E-14 is a member of `coverage.md`'s one *Gated on chatOnS* list
  (D238). A withdrawal takes it out of that list as well as out of the
  `planned` cell; the drafted board row names both.

## Drafted for `herd-merger`

Two entries and one row. **Not numbered**: the merger allocates and gaps
stay (D148, invariant 8). Highest on `main` today: **D272**,
**ORCH-302**. Read *Risks and findings* 2 before pasting entry 1.

### Entry 1 — the consumer's two answers

```markdown
## 2026-10-30 — D-NEXT The consumer withdraws E-14 and cuts v1.0.0's retention

**Context.** **D249** authorises the orchestrator to ask the first
consumer directly, and **D248** records their first two answers, on
E-49 and E-50. They answered twice more the same day, and both answers
were acted on before either was written down: `cut-v1.0.0`'s body
counts E-14 among the musts they released us from, and `docs/status.md`
and the release notes treat the hub's record retention as cut. **Neither
was in this file.** A release claiming a retirement nobody recorded is
what D249's limits exist to prevent, which is why this entry exists
before the body is signed.

Their orchestrator answered in chatOnS's name as requester, under a
delegation from Alexa, and said explicitly that Alexa's word replaces
theirs if she rules otherwise. As in D248, it is recorded as a
**consumer answer**, not as a Herd decision, and **it binds none of our
invariants**.

**(a) E-14 is WITHDRAWN, not waived, and the difference is
load-bearing.** E-14 asks that warm caches be ready before a job runs.
Their executor does it itself: a **copy-on-write mount of the nearest
cache per commit**, from their own `up` step, under their own ADR.
**The reason is written beside the withdrawal and not only beside the
tally**, because without it somebody re-adds a cache engine in six
months believing they are closing a gap. Their words: *"the requester
supplies its own warm cache through its executor hook; Herd need not
know the shape of a cache."* **A waiver is a promise deferred; a
withdrawal is a requirement retired.** E-49 is the first kind and this
is the second.

**What that moves, stated so no file is patched by guess.** The set the
consumer has released us from becomes **three** — E-49 waived, E-50 and
E-14 withdrawn. `docs/coverage.md` re-derives as **36** wholly on
`main`, **5** `part` (E-12, E-16, E-17, E-37, E-42), **0** with none of
it, **3** released from: 36 + 5 + 0 + 3 = 44. D248's **42 of 44** is a
*scope* figure — 44 less the waivers, never a count of what is met — and
it becomes **41 of 44**. That is arithmetic. **Which headline is
published stays Alexa's** (**D210**). E-14's `coverage.md` cell may move
off `planned` now that this entry exists, and it leaves the *Gated on
chatOnS* list of **D238** at the same time; neither edit is this entry.

**(b) The hub's record retention may be cut from v1.0.0 and follows in
v1.1**, named in the release notes as not met. **They stated two
conditions.**

1. **`--volume-free-floor` protects the whole filesystem — and the
   precise form is the one that must be held, not the loose one.** The
   floor **measures** the whole volume: it is the only quantity in the
   hub that sees a record byte, a log byte and a collected byte at
   once, derived from `statvfs` at every trigger (**D254**). **But the
   only thing a sweep can remove is output collections** — what goes,
   and in what order, is still D152's four keys, read down to
   `min(--output-budget, held + free − floor)`. **It detects pressure
   from any source and relieves it through one lever.** The
   orchestrator first described this loosely, corrected it, and the
   corrected form is what the consumer holds; it is written here that
   way so the loose form does not come back as the condition.
2. **`herd-hub report` keeps showing the weight of the records and the
   logs** *"so that we see the growth instead of discovering it"*.
   **This was not met when they asked and is met now**: **D271**,
   landed in batch 27, section 5, which weighs records, event files and
   logs per job directory beside section 3's collected figure. **The
   limit case that proved it was missing** is our own queue root,
   measured frozen the same day: **1,443,667 bytes** of bookkeeping
   against **0** bytes of collected output, and every one of the four
   figures `report` printed about that root read **nought**.

**Their figures, which are what the cut was decided on, each with its
instrument.** **≈ 346 MiB** of records, logs and event files over
**3 334** jobs — about **104 KiB a job**, **0.32 %** of that root — by
their own **`du -sk` of 2026-10-29, which is allocated blocks and
therefore real disk occupancy, not apparent size**; against **105.57
GiB** of collected files, **99.68 %**, by `herd-hub report` the same
day; and a derived **7.57 GiB/day** of named arrivals over their
fourteen days (`docs/disk-policy.md` R14, R16, §0.4). Nobody in this
project read their root (invariant 9, **D161**).

**The trap that sits between those two numbers, and it is in this entry
because the decision rests on comparing them.** D271's section 5 sums
**apparent size**, which is a **floor** on occupancy — measured
**1.97×** below blocks on a small-file population, 1,443,667 apparent
against 2850816 of blocks on one root. **So at their migration section
5 will read roughly half their `du` figure and nothing will have
shrunk**, and the natural reading of a root halving is that retention
got less urgent. Section 5 prints the unit and that caveat itself; it is
stated here too, because a reader of this entry compares the two
figures above and not the program's output. Compare `du` with `du`, and
never one unit against the other.

**What this does not decide.** It is not a decision of Herd's at all: it
records what the requester answered. It does not decide when v1.1's
retention engine lands, nor the bound ORCH-022 asks about, nor
`bytes_written` (ORCH-278), nor any wording in the release notes, which
is Alexa's (**D210**). It retires no must but E-14, and a future answer
of theirs retires none without an entry of its own (**D248**, **D249**).
```

### Entry 2 — the orchestrator's ruling on what `status.md` carries

```markdown
## 2026-10-30 — D-NEXT `status.md` carries state and pointers, never reasons

**Context.** `docs/status.md` is capped at **90 lines** by
`scripts/check-docs.sh` and has sat at exactly 90 for three landings.
Each of those three bought its line by **removing content**, and one of
them removed E-50's KVM reason to fit — the sentence D248 had written
down precisely so that nobody re-adds WSL2 believing they are closing a
gap. A cap that is paid for in reasons deletes the one kind of sentence
that cannot be re-derived from the code.

**Ruling, the orchestrator, 2026-10-30.** `status.md` carries **state
and pointers, not reasons**. A reason belongs in `docs/decisions.md`,
which is append-only and unbounded (invariant 8). **The cap is not
raised** — it is what keeps the file state rather than a journal — and a
replacement that needs a line spends the line of a sentence carrying a
reason, because that sentence is in the wrong file, not because it is
the least important.

**First application, the same day:** item 5 at `:89`-`:90`, which said
`coverage.md`'s E-12 and E-17 rows are *"false and not recounted here"*,
was replaced two lines for two lines with what `e12-e17-recount`
measured, and the replacement adds no reason.

**It is the orchestrator's and inside its mandate** — the shape of a
docs file is *how*, not *what* the product is or promises — so it is
decided alone and reported afterwards (`CLAUDE.md`, *Direction*). It
changes no gate: `check-docs.sh` is untouched by it.
```

### A `board.md` row — `coverage.md`'s E-14 cell, now unblocked

For *Owed — agreed, not started*; allocate the next free ORCH number.
It is **blocked until entry 1 lands**, and until `e12-e17-recount`
lands, because that branch owns `docs/coverage.md`.

```text
| ORCH-NEXT | **E-14's `coverage.md` cell reads `planned` and the consumer has withdrawn the row**, so the file and the release body disagree about the same must. The row is `docs/coverage.md:98` — `E-14 | must | warm caches ready before job | M12 | planned`, its note ending *"gated on their replication (M18)"* — and E-14 is also a member of the one *Gated on chatOnS* list that **D238** made authoritative. Two agents refused to move it and both were right: retiring a must is the consumer's act (**D248**, **D249**) and no entry recorded theirs. **The entry this branch drafts records it**, which is what unblocks the edit and nothing else does | `consumer-answers-e14-retention`, 2026-10-30, against `71efccf`; the withdrawal is the consumer's answer of 2026-10-30 via D249's channel | herd-step | owed | Move E-14's State cell from `planned` to `withdrawn` with the consumer's own reason beside it, as E-50's row carries its KVM reason; take E-14 out of the *Gated on chatOnS* list, since a retired row waits on nothing; and re-derive the four tallies mechanically from the 44 `must` State cells rather than patching them — **36** wholly on `main` + **5** `part` + **0** with none of it + **3** released from = 44. **The scope figure becomes 41 of 44** and is stated as a scope figure, never as a count of what is met; **which headline is published stays Alexa's** (**D210**), so the file states the arithmetic and claims no headline. Prose only: `check-docs.sh` alone, the Rust gates covered by the batch's `gate.sh` | the drafted entry 1 of `history/2026-10-30-consumer-answers-e14-retention.md`; `e12-e17-recount` (`ee28e15`), which owns `docs/coverage.md` and is unlanded | `docs/coverage.md:98`, `:24`, `:46`-`:48`, `:162`, `:226`; `docs/coverage.md` *Gated on chatOnS*; **D238**, **D248**, **D249**, **D272**; `history/2026-10-30-e12-e17-recount.md`; `history/2026-10-30-release-v1.0.0.md:302`-`:311` on `cut-v1.0.0` |
```

## Decisions needed

1. **Which of the two drafted entries carries E-14's withdrawal, and
   does the release entry cite it or restate it?** *Recommendation:*
   land both, with `cut-v1.0.0`'s items 3 and 4 reduced to a citation of
   entry 1, because a consumer answer recorded as an owner's choice is
   the confusion D248's own wording exists to prevent. *Fallback:* land
   entry 1 alone and drop items 3 and 4 from the release entry. **Either
   way, not both as written** — see *Risks and findings* 2. This is the
   merger's and the orchestrator's, not Alexa's: it is *how* a fact is
   filed, not what the product promises.
2. **Is the published headline 41 of 44, or 42 of 44 beside the table?**
   Alexa's, under **D210**, and already open as `cut-v1.0.0`'s finding
   1. Nothing here answers it and nothing here needs it answered.

## Resume point

`herd-merger`, at the batch's integration: read *Risks and findings* 2,
settle decision 1, then paste entry 1 (and entry 2) into
`docs/decisions.md` with numbers allocated, add the drafted row to
`docs/orchestration/board.md`, and run the batch's own
`./scripts/gate.sh`. The edit that row describes — `docs/coverage.md`'s
E-14 cell — is a **separate prose-only `herd-step` task** and waits on
both this branch and `e12-e17-recount` landing; it is not the merger's.

## Processes

**None started, none ended.** No `cargo`, no build, no Herd job, no
signal sent to any process — PID 662711, the queue's runner, was not
touched. `./scripts/bench-preflight.sh` was **not run** and was not
owed: there was no heavy phase, no compile and no suite.

**No `target/`, no `CARGO_TARGET_DIR`**, so nothing to delete: this
branch compiles nothing. The worktree, the only place written besides
the scratchpad's own gate output, is:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt/consumer-answers
```
