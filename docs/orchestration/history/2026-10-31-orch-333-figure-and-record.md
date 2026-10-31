# orch-333-figure-and-record — handoff 2026-10-31

- **Item:** ORCH-333 (parts 1, 2, 4), ORCH-070 (part 3)
- **Agent:** herd-step
- **Branch / commit:** `orch-333-figure-and-record` /
  `b5be18b527d97cd0d389bbb9f399e2fbcfcae43f`, plus the commit that adds
  this file; based on `origin/main`
  `7309b16798b45050f17bf9dfe9d1652d4fa68636`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Four parts on one theme: **a figure nobody maintains, and a record
nobody checks.** Nothing here touches the product's behaviour — one
`.rs` doc comment, two shell comments, one README section, one new
`check-orchestration.sh` section, three board rows.

### 1. `scripts/report-skips.sh`: the qualifier went with the figure

The sentence read *"on a runner that reports 15 skips when green"*. It
now reads *"on a runner with a skip list of its own
(`scripts/expected-skips/ci.txt`)"* — the phrasing `scripts/test.sh`
already uses since ORCH-313's sweep. The point the clause was making is
intact and needs no number: a red suite's output is not a complete list
of skips, because a test binary that never ran records no skip of its
own.

The qualifier was the false part, as briefed and as verified here: the
15 came from run **40439537929**, which printed *"AT LEAST 15
assertion(s)"* — `report-skips.sh`'s own floor wording for a run with no
`test suite: PASSED` marker (D179) — and `ci.txt`'s header pins CI's
green figure to run 40564336703.

**The file is still 125 lines**, byte-identical in length to `main`'s,
so the two dated records that cite `report-skips.sh:20` and `:38` keep
pointing where they pointed.

### 2. `herd-runner/src/cgroup.rs:2532`: the count is gone, not corrected

*"the thirteen standing skips in `scripts/expected-skips/local.txt` are
where it is made"* is now *"the standing skips named in
`scripts/expected-skips/local.txt` are where it is made"* — the first of
the preference order in the brief, remove rather than derive.

Same two lines, same two line numbers, and the file is **still 2574
lines**, so `board.md` and `history/2026-10-31-merge-batch-33.md`, which
both cite `cgroup.rs:2532`, still land on this sentence.

**Attribution, because the pattern is only useful if it is
attributable.** This count did not drift in: it was created on purpose
at batch 33, by the orchestrator's instruction to an agent, in the very
batch whose other branch was sweeping hand-maintained counts out of the
prose. The agent did as told. ORCH-313's detector read `scripts/` and
`docs/`, so no machine could have caught it. That is recorded in
ORCH-333's board row and in the drafted decision.

### 3. ORCH-070's rule is written down

New section in `scripts/expected-skips/README.md`, *A total is not
written down (ORCH-070, 2026-10-31, extending D187)*, carrying Alexa's
ruling verbatim as its own paragraph, then why it exists (the three
landings it cost), then the two halves as this directory already
practises them, then where it stops.

It is placed after the opening block and before *Two kinds of skip*,
so a reader who opens this directory looking for a number meets the rule
before any of the files. The decision entry is drafted below, headed
`D-NEXT`; **`docs/decisions.md` is untouched.**

### 4. `scripts/check-orchestration.sh`: the chain has a machine now

New last section, *a landing records its predecessor's post-push run
(ORCH-333)*, **11 cases**, appended **below every existing case** so
that no pinned line number in this file moves — `:856`'s synthetic
two-name collation hash included, and it is untouched.

What it refuses: a newest landing record that does not name the previous
landing's batch together with a run id (8+ digits) and a verdict for it
**inside one paragraph**. A paragraph ends at a blank line, at the next
bullet and at the next heading, so a claim in one bullet cannot be
answered by a figure in another.

What it cannot do, and its own text says so in as many words: this
script has no network and no toolchain, it cannot ask GitHub whether a
run exists or passed, and it never tries. It enforces *"you went and
looked and wrote down what you saw"*, not *"the verdict is true"*. A
merger who invents both figures passes it. The accepted verdicts are
GitHub's own conclusions plus `failed`, and a predecessor that started
no run is written as the literal `no ci.yml run`.

**The detector fires before its silence is trusted.** Nine synthetic
fixtures and one real one:

| fixture | expected | why it is there |
|---|---|---|
| id + verdict on one line | quiet | the ordinary shape |
| never names the predecessor | fires | the silent omission |
| names it, no run id | fires | **batch 32's actual sentence** |
| run id, no verdict | fires | half a record |
| id + verdict for its OWN batch, predecessor named in another paragraph | fires | the near miss |
| the figure wrapped across one bullet | quiet | how batches 29-31 wrote it |
| id + verdict in a different bullet | fires | the bound on the paragraph scope |
| explicit `no ci.yml run` | quiet | a statement, not an omission |
| one landing only | quiet | the first landing has no predecessor |
| the real records with batch 33 removed | fires, **and for the stated reason** | the known-bad that is not synthetic |
| the real tree as it stands | quiet | batch 33 recorded both owed runs |

Calibrated on the real records, which is how the design was corrected
rather than assumed: a first version required the three things on **one
line** and would have gone red on batches 29, 30 and 31, all three of
which wrote the figure correctly across two or three wrapped lines. With
the paragraph scope, the records of batches **28, 29, 30, 31 and 33
pass, and batch 32 is the only one of the six that fires** — exactly the
landing batch 33's merger found had broken the chain.

### Board

`ORCH-333` → **done**, `ORCH-070` → **done**, and one new row, written
`ORCH-NEXT` for `herd-merger` to number (it is 334 unless another branch
in this batch claims it). All three are reproduced in full under
*Drafted for `herd-merger`*. `board.md` 1603 → 1604 lines.

## Not done, and why

- **`docs/status.md` and `docs/coverage.md` are untouched**, as the brief
  put them out of scope on purpose, and the rule adopted in part 3
  exempts them by name: `status.md` is *state*, where a current total is
  what the file is for (and it is still **exactly 90 lines**, D276), and
  `coverage.md` carries headline figures by design. Read for this, not
  edited. I do not think either is wrong.
- **`docs/decisions.md` is untouched.** The entry is drafted below as
  `D-NEXT`; only `herd-merger` appends.
- **`docs/orchestration/templates/landing.md`,
  `docs/orchestration/workflow.md` and
  `.claude/agents/herd-merger.md` are untouched**, and all three say the
  predecessor's run id is recorded in the **next landing's board edit**
  rather than in its record. That is a real divergence from the guard
  this task was asked to add, it is outside the paths this brief allows,
  and it is the first risk below and the new board row's *Next action*.
- **No merge, no tag, no release, no pull request, no push to `main`.**
- **`scripts/test.sh:63`** keeps *"the same twelve names with the same
  `sha256`"*. It is ORCH-313's deliberate leftover: a dated measurement
  of batch 9's run, not a current total, and the rule's second half
  covers it — though it names its batch rather than a host and a date,
  which is the weaker form. Left as the sweep left it; mentioned here so
  nobody reads it as a site this task missed.

## Gates

`scripts/` and `herd-runner/` are not prose-exempt, so all four Rust
gates plus `check-docs.sh`. Everything below ran on the committed tree
`b5be18b527d97cd0d389bbb9f399e2fbcfcae43f` **before this handoff was
written**, except the last run, which is named and not quoted.

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets --
-D warnings`, one queue job, `--label build --memory-mib 4700`,
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-333`:

```text
    Checking herd-runner v1.0.0 (.../wt-orch-333/herd-runner)
    Checking herdctl v1.0.0 (.../wt-orch-333/herdctl)
    Checking herd-hub v1.0.0 (.../wt-orch-333/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.15s

[exited with code 0]
```

`./scripts/check-docs.sh`:

```text
docs ok
```

`./scripts/check-orchestration.sh`:

```text
== a landing records its predecessor's post-push run (ORCH-333)
ok    landing: the predecessor's run id and verdict on one line is enough
ok    landing: a record that never names its predecessor is refused
ok    landing: naming the predecessor with no run id is refused
ok    landing: a run id with no verdict for it is refused
ok    landing: a run id for its OWN run does not answer for the previous one
ok    landing: the figure wrapped across one bullet is accepted
ok    landing: a run id in a different bullet does not answer for it
ok    landing: an explicit "no ci.yml run" is a statement, not an omission
ok    landing: the first landing has no predecessor and is not refused
ok    landing: fires on the real record that broke the chain (batch 32)
ok    landing: the real tree's newest landing record carries it

check-orchestration: 335 passed, 0 failed
```

**324 → 335 passed, 0 failed, 0 skipped**, and the eleven are the new
section. No `SKIP` line in the whole run: the load-gated `gate:`
self-tests ran.

**The last run, named and not quoted**, because a handoff is itself a
docs file and a gate run made before it was written tested a different
tree: `./scripts/gate.sh` on the commit that adds this file, submitted
whole and bare through the queue
(`herdctl --hub ws://127.0.0.1:8791 run --label build --memory-mib 4700`,
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-333`). Its output lands in
`target/herd-gate/<sha>.log` and its receipt in
`target/herd-gate/<sha>.receipt` inside this worktree; the verbatim
verdict, the skip list and the two hashes are in the agent's report.

An earlier whole-gate run on the same tree **minus this handoff** was
made first, so that a red step would be found before this file existed;
its verdict is in the agent's report too, named as the earlier run.

**Skips:** whichever of the two committed lists the benches' own printed
readings justify — `scripts/expected-skips/local.txt` at rest, plus the
two names of `local-under-load.txt` when and only when each flood bench
prints a load reading over its own ceiling. Nothing in this branch adds,
removes or renames a test, a `#[ignore]` or a skip reason, so neither
list moves and `ignore-list-sha256` cannot move either
(`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545`,
38 `#[ignore]`s).

## Risks and findings

**1. The method's own documents send that run id somewhere else, and the
new guard will go red on a merger who follows them.** This is the one
thing in this branch that can cost somebody a gate run.
`docs/orchestration/workflow.md:364`-`:367`,
`.claude/agents/herd-merger.md:100`-`:107` and
`docs/orchestration/templates/landing.md` *CI run* all say the same
thing: the merger puts the post-push run id in its **report**, and *"the
board gets it at the next landing"*. None of the three says it goes in
the next landing's **record**, which is what step 2 of the gate now
asks for. Batch 33 wrote it in both places and passes; a merger who
writes it only in `board.md` is red. All three files are outside this
brief's allowed paths, so the fix is the new board row's *Next action*:
one line in `templates/landing.md` and one in `herd-merger.md` step 9.

**2. Measured, and it decides the design: pointing the check at
`board.md` instead would have passed batch 32.** The obvious
alternative — follow the method's wording and look in `board.md` —
fails on the very case the guard exists for. `board.md` at batch 32's
landing commit `9f76b54` contains no `41206929574` anywhere, so the
figure genuinely was not there; but its lines 1516-1522 are a single
paragraph that reads *"Batch 30's post-push run, recorded by id at this
landing … `ci.yml` run **41153775629** … `completed`/`success` … Batch
31's own gate job id … and **its post-push run id is in the report
alone**"*. A paragraph-scoped check for *batch 31 + a run id + a
verdict* matches it. So the landing record, not the board, is the only
one of the two that can carry a checkable claim — the brief's choice,
and now with a measurement behind it rather than a preference.

**3. A premise of the brief was refuted by the records, and the design
changed because of it.** "Together on one line" was the first
implementation. Batches 29, 30 and 31 each wrote the predecessor's run
id and verdict **correctly** and across two or three lines, because this
prose wraps at 79 columns, so that version went red on three compliant
records. The scope is now the paragraph, and the six most recent records
are the calibration: 28, 29, 30, 31 and 33 quiet, 32 fires. Had the
first version shipped, the guard would have been a false-positive
machine on the method it was written to protect.

**4. What the guard is not, restated here because it is easy to
overrate.** It is co-occurrence inside a paragraph, not attribution. A
mistyped digit passes. A paragraph naming both batches with one run id
passes. A verdict that contradicts GitHub passes. It refuses exactly the
shape batch 32 had — a claim about the predecessor with no figure
anywhere near it — and `workflow.md:330`'s *"that comparison is a
human's"* is still exactly true of the verdict itself. The case says all
of this in its own comment block, which is the brief's requirement that
nobody later mistake it for a verification.

**5. Line counts: nothing that is pointed at moved.**
`scripts/report-skips.sh` **125 → 125**,
`herd-runner/src/cgroup.rs` **2574 → 2574** with the sentence still at
`:2532`, `scripts/gate.sh` **733** and `scripts/test.sh` **302**
untouched (not edited at all), `scripts/check-orchestration.sh`
**2033 → 2224** with the 191 new lines appended **after** every existing
line, so `:73`, `:438`, `:464`, `:475`, `:683`, `:856`, `:923`,
`:1171`-`:1176`, `:1278`, `:1408`, `:1804` and the rest are where they
were. `:856`'s pinned two-name hash is not touched and not "fixed".

**6. One file's interior did shift: `scripts/expected-skips/README.md`
100 → 142 lines**, the 42 inserted above *Two kinds of skip*. No live
document points into this file by line; the only pointers are in
`history/2026-10-31-orch-313-skip-count-sweep.md:30`-`:33`, which quote
`README.md:8`-`:13`, `:81`, `:83`-`:85` and `:91` as *the text that
record changed*, which is a dated snapshot and not a live reference.
Appending at the end would have preserved those numbers and put the rule
where nobody meets it; this was the trade, and it is deliberate.

**7. Two limits of the new detector's bookkeeping**, neither reachable
today. It takes every `*-merge-*.md` under `history/`, without
`check-docs.sh`'s `landing_from=2026-10-29` floor, and orders them by
date then by `sort -V` on the batch tag. Five landing records on `main`
have no digit in their name (`merge-herd-own-claude-root`,
`merge-batch-docs-gate-orch-batched-pr`, `merge-gate-and-report`,
`merge-method`, `merge-product-fixes`); if one of those ever became the
**predecessor** of the newest, the case prints *"has no batch number in
its name"* and refuses rather than guessing. All five are dated
2026-10-26 to 2026-10-28, so this is a statement about shape and not a
live condition.

**8. Confirmed against the brief.** `check-orchestration.sh` was indeed
**324 passed / 0 failed** before this branch (335 − 11). Batch 32's
record does carry the sentence the brief quotes, at
`history/2026-10-31-merge-batch-32.md:49`. `report-skips.sh`'s 15 is
`AT LEAST 15`, printed by its own floor branch, and `ci.txt`'s header
pins the green 14 to run 40564336703. Nothing in the brief was
contradicted except the "one line" premise of point 3.

## Drafted for `herd-merger`

One decision entry and three `board.md` rows. The two edited rows are
**already in `board.md` on this branch** and are reproduced here so the
merger can resolve a conflict without opening another branch; the third
is written `ORCH-NEXT` and needs a number.

### The `docs/decisions.md` entry, in full

```markdown
## 2026-10-31 — D-NEXT A skip total is not written down; prose names
the file that holds the list

**Context.** D187 made the **gate** compare the skip list and never the
count. Prose was left out, and no gate reads prose. Three landings paid
for that in a row. Batch 31 added one name to
`scripts/expected-skips/local.txt` and silently falsified every sentence
that carried a total. ORCH-313 swept four of those sentences at batch 33
and its detector read `scripts/` and `docs/` only, so a fifth survived
in code — where, in that same batch, a correction from *twelve* to
*thirteen* was made at `herd-runner/src/cgroup.rs:2532` on the
orchestrator's own instruction, and was true for one day. And
`scripts/report-skips.sh` carried *"a runner that reports 15 skips when
green"*, whose false part was the **qualifier**: the 15 came from run
40439537929, a red run that printed "AT LEAST 15" because it had not
finished (D179), while CI's green list is `ci.txt`, blessed from run
40564336703. ORCH-070 had named this rule on 2026-10-26 and nobody had
written it down.

**Choice.** Alexa's ruling, 2026-10-31, in the stronger form the sweep
had already implemented in practice: **a skip or ignore total is not
written down. Where prose needs to point at one, it names the file that
holds the list. Where a figure is genuinely a measurement, it carries
its host, its date and its run or sha, and is labelled as that run's
measurement rather than as a current total.** It is written in
`scripts/expected-skips/README.md`, the directory a reader goes to when
they want a number, and the worked example of its second half is
`local-under-load.txt`'s own header: it keeps its **14** as one blessed
run's measurement, with host, date and sha, and says that what
`local.txt` holds at rest *"is local.txt's business and is not restated
here"*.

**What it exempts, by name.** `docs/status.md` is *state*, replaced
every session (invariant 8), so a current total there is what that file
is for; `docs/coverage.md` carries headline figures by design. Both were
read against this rule at ORCH-333 and left untouched.

**Alternative rejected.** A `check-docs.sh` rule that a skip total under
`docs/` must name its host and its date — ORCH-333's own proposal.
Nothing mechanical can tell a total from any other number in prose, and
the two files that legitimately hold one are the two the rule exempts,
so the check would have been a false-positive generator over the only
corpus it can read.

**Reversibility.** High. One section of one README; no code reads it.

**On the number.** This entry is `D-NEXT`, to be allocated at this
batch's integration above D287; numbers are identifiers and gaps stay
(D148, invariant 8).
```

### `board.md` row, new, needs a number

| ORCH-NEXT | **A landing's record of its predecessor's CI run is now checked for shape, nothing checks that it is true, and the method's own documents point that figure at `board.md` instead — where it cannot be checked at all.** `scripts/check-orchestration.sh`, *a landing records its predecessor's post-push run*, refuses a newest landing record that does not name the previous landing's batch together with a run id and a verdict for it inside one paragraph. It has no network and no toolchain, so it enforces *"you went and looked and wrote down what you saw"* and a merger who invents both figures passes it. Two gaps stay. **(a)** Nothing compares the written verdict with GitHub's: `workflow.md:330`'s *"that comparison is a human's"* is still exactly true, and only `scripts/githooks/pre-push` is positioned to do it. **(b)** `workflow.md:364`-`:367`, `.claude/agents/herd-merger.md` step 9 and `templates/landing.md` *CI run* all say the predecessor's run is recorded in **the next landing's board edit**, not in its record — so a merger following them to the letter writes the figure only in `board.md` and goes red at step 2 of its own gate. Batch 33 wrote it in both and passes | `orch-333-figure-and-record`, 2026-10-31, part 4 of ORCH-333's brief; the chain broke at batch 32 and batch 33's merger found two runs owed | herd-step | owed | One line in `templates/landing.md` *CI run* and in `herd-merger.md` step 9: the predecessor's run id **and its verdict** go in this record as well as in the board edit, in one paragraph with the batch they belong to. **Do not instead point the check at `board.md`** — measured: a board-scoped variant would have **passed** batch 32, because batch 31's board item names batch 31 in the same paragraph as batch 30's run id and `success` (`board.md` at `9f76b54`, lines 1516-1522). Then (a) is a `pre-push` question and not this row's | — | `scripts/check-orchestration.sh`, last section; `docs/orchestration/templates/landing.md` *CI run*; `docs/orchestration/workflow.md:330`, `:364`-`:367`; `.claude/agents/herd-merger.md:100`-`:107`; `history/2026-10-31-merge-batch-32.md:49`; `history/2026-10-31-orch-333-figure-and-record.md` |

### `board.md` row, already edited on this branch

| ORCH-333 | **Answered, and the rule behind it is written down.** (1) `scripts/report-skips.sh` carries no total: the sentence now reads *"on a runner with a skip list of its own (`scripts/expected-skips/ci.txt`)"*, which is the point the false qualifier was making — a red suite's output is not a complete list of skips — and it survives with no figure at all. (2) and (3) are **not edited and are not defects**: ORCH-070's rule as adopted exempts them, because `docs/status.md` is *state*, where a current total is what the file is for (D276's 90 lines untouched), and `docs/coverage.md` carries headline figures by design. A **fourth** site existed in code, where ORCH-313's detector did not read: `herd-runner/src/cgroup.rs:2532` said *"the thirteen standing skips"* and now says *"the standing skips named in `scripts/expected-skips/local.txt`"*. **That count was created by the orchestrator's own instruction at batch 33**, in the very batch that was sweeping counts out of the prose — recorded as attributable, because the pattern is only useful if it is | ORCH-313's sweep, `orch-313-skip-count-sweep`, 2026-10-31; fixed on `orch-333-figure-and-record`, 2026-10-31 | herd-step | done | Nothing. The rule is in `scripts/expected-skips/README.md` *A total is not written down*, and the decision entry for it is drafted as `D-NEXT` in the handoff for `herd-merger` to allocate | ORCH-070 | `scripts/report-skips.sh` *This step runs with `if: always()`*; `herd-runner/src/cgroup.rs:2532`; `scripts/expected-skips/README.md`; `history/2026-10-31-orch-313-skip-count-sweep.md`; `history/2026-10-31-orch-333-figure-and-record.md` |

### `board.md` row, already edited on this branch

| ORCH-070 | **Written down, 2026-10-31, in the stronger form the sweep had already implemented in practice**, and it is in `scripts/expected-skips/README.md` — the directory a reader meets when they go looking for a number: *a skip or ignore total is not written down; where prose needs to point at one it names the file that holds the list; where a figure is genuinely a measurement it carries its host, its date and its run or sha, and is labelled as that run's measurement rather than as a current total.* The finding that opened this row stands as written: the skip count is **per host** and the project quoted one host's figure as universal — 12 here and 15 on CI's `check` on 2026-10-26, both since moved. Three landings paid for the rule's absence: batch 31 falsified every sentence carrying a total, batch 33 corrected one from *twelve* to *thirteen* while sweeping counts out of the prose, and `report-skips.sh` carried a **red** run's floor as a green figure | finding of the PR #72 integration, 2026-10-26; adopted by Alexa's ruling at ORCH-333, 2026-10-31 | herd-planner, then herd-step | done | Nothing. `D-NEXT` is drafted in `history/2026-10-31-orch-333-figure-and-record.md` for `herd-merger` to allocate; it extends **D187** from the gate to the prose. Deliberately **not** a `check-docs.sh` rule: nothing can tell a total from any other number, and the two files that legitimately hold one (`status.md`, `coverage.md`) are named as exempt | — | `scripts/expected-skips/README.md` *A total is not written down*; `history/2026-10-26-merge-herd-own-claude-root.md` "Gates"; CI run 40403915407 `check`; ORCH-069; ORCH-313; ORCH-333 |

## Decisions needed

**None.** Part 3's rule is Alexa's own ruling, taken in the brief, and
the rest is inside the mandate and reversible. The one open method
question — whether the predecessor's run id lives in the landing record,
in the board edit, or in both — is *how*, not *what the product is or
promises*, so it is the new board row's and not Alexa's.

## Resume point

For `herd-merger`: allocate `D-NEXT` above D287, number the `ORCH-NEXT`
row (334 unless another branch in this batch claims it), and **write the
previous landing's `ci.yml` run id and its verdict into the landing
record, in one paragraph with the batch they belong to** — step 2 of
your own gate now refuses a record without it, and your agent file still
tells you the board edit alone is enough.

For the next `herd-step`: the new row's *Next action* — one line in
`docs/orchestration/templates/landing.md` *CI run* and one in
`.claude/agents/herd-merger.md` step 9, so the method and the guard say
the same thing.

## Processes

No long-lived process was started and nothing was killed. Two
`herdctl` clients were run in the background by this session, both
exited on their own: the `fmt` + `clippy` job and the whole-gate job,
both `--label build --memory-mib 4700` against
`ws://127.0.0.1:8791`, the local queue. Nothing of another agent was
touched; `/mnt/ssd_980/herd/build/link-cut-clock-figure` was left alone.

Build tree: `/mnt/ssd_980/herd/build/orch-333`, off `/`, this task's
own, deleted once the branch was pushed.
