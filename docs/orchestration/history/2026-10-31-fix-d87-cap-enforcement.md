# fix-d87-cap-enforcement — handoff 2026-10-31

- **Item:** ORCH-NEXT (drafted below; `herd-merger` allocates the number)
- **Agent:** herd-step
- **Branch / commit:** `fix-d87-cap-enforcement` / this commit, based on
  origin/main `d32c87a7876b91c55ca5caa8284ba5fa1357adc7`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The project's record claimed an enforcement the code does not have. Every
citation in the brief was walked against `main` at `d32c87a` **before**
anything was written; the finding holds, with three line-number
refinements noted under *Risks and findings*.

**The reading.** `herd-runner/src/cgroup.rs:660` is the only `memory.max`
write in the product. It writes `limits.memory_max` inside
`JobCgroup::create` (`cgroup.rs:644`), and that `limits` is one
`JobLimits` built once at startup from `--job-memory-max`:
`main.rs:525`-`:530` → `confine.rs:180` `job_limits()` →
`confine.rs:430` `Ceiling::detect` → `cgroup.rs:603` `open_for_job`,
which destructures the same `limits` for every job it opens. There is no
per-job branch. The kernel enforces the **runner-wide** cap, identically
for every job, never the job's declared figure — and it could not do
otherwise, because no frame carries that figure to the runner.

**Three prose sites corrected**, each as a reading of the code with the
line that decides it, never as a promise:

- `docs/coverage.md:216`, E-29's note. Was *"M4's cap is what makes the
  declaration honest (D87)"*. Now names `cgroup.rs:660`, says the cap
  bounds what a job is **permitted** while the declaration is what the
  hub reserves against and places on, and says the two are not the same
  figure. **E-29's State cell was not moved** and no other `must` state
  cell was touched.
- `docs/architecture.md:762`-`:770`, in running prose under the
  admission section — the site a table scan misses. Was *"M4's kernel
  cap is what makes the declaration the honest unit (D87)"*.
- `docs/handover.md:425`-`:435`. Was *"M4's kernel cap is what makes it
  true (D87)"*. This one is a **recommendation that survives**: the cap
  really is the only honest cost of a job that declares none. What was
  false was the warrant, so the correction keeps the recommendation and
  replaces its reason — the cap is runner-wide, which is exactly why it
  is the one figure the hub can know applies.

`docs/decisions.md` was **not** touched (append-only, D148): D87's text
stands and the correction is the `D-NEXT` entry below. `docs/status.md`
was not touched — it is at 90 lines and no pointer in it went dead.
`deploy/release-notes/1.0.0.md` was not touched (D210).

**The coverage tally, re-derived mechanically** rather than trusted:

```
$ awk -F'|' '$3 ~ /must/ {gsub(/ /,"",$6); print $6}' docs/coverage.md \
    | sort | uniq -c | sort -rn
     36 merged
      5 part
      2 withdrawn
      1 waived
$ awk -F'|' '$3 ~ /must/' docs/coverage.md | wc -l
44
$ grep -cE '^\| E-[0-9]+ ' docs/coverage.md
55
```

36 + 5 + 0 `planned` + 1 + 2 = 44 over 55 rows, before and after the
edit, exactly as the brief predicted and as ORCH-307 last recorded it.
`$ awk -F'|' '/^\| E-29 /{print $6}'` → ` merged`, unmoved.

## Not done, and why

- **The two `.rs` doc comments that carry the same false warrant.**
  `herd-hub/src/job.rs:880` and `herd-hub/src/matcher.rs:479` (quoted in
  full in the board row below). This was a prose-only task — no `.rs`,
  no script — so they are a finding and a drafted row, not an edit.
- **No per-job cap was built.** Making the kernel enforce the
  declaration is a protocol change (a `Requirements` field on
  `AssignJob`) and a product question, not a docs defect.
- **The Rust gates were not run locally.** Prose-only change; what
  covers them is the batch's own `./scripts/gate.sh` run at
  integration, which is `herd-merger`'s and not this task's.

## Gates

Run **before** this commit, on the tree with all three corrections and
the board row in place:

```
$ ./scripts/check-docs.sh
docs ok
```

The four Rust gates were **NOT RUN** locally — prose only, covered by
the landing's own `./scripts/gate.sh`.

The **last** run is `./scripts/check-orchestration.sh` on this committed
tree, made after this file was committed and before the push; its
verbatim output is in the agent's report, not in the tree it gates. Its
`gate:` self-tests are load-gated, so it was run at a quiet moment
(`/proc/loadavg` 1-minute figure under 1.0).

**Skips:** no skip. Neither script skips an assertion; no `HERD-SKIPPED`
line was produced or copied.

## Risks and findings

**Three line numbers in the brief were off; none refutes the finding.**
Reported rather than silently corrected, since a citation that does not
say what a brief claims is itself the defect this task exists to fix.

- `docs/decisions.md:1908` is D87's **header**. The false sentence is at
  `:1924`-`:1926`, verbatim as the brief quoted it.
- `Declaration` is at `herd-proto/src/declaration.rs:235`, not
  `lib.rs:235`; `lib.rs:36` re-exports it. Its fields are as stated:
  command + commit + args + executor.
- The last link of the startup chain is `cgroup.rs:603`
  (`open_for_job`), with `JobCgroup::create` at `:644`; the brief said
  `:607`.

Everything else verified exactly: `cgroup.rs:660` is the sole
`memory.max` write in the product (`room.rs`'s many hits are **reads**,
doc comments and tests); `FromHub::AssignJob` at `lib.rs:2375` carries
job id, attempt, command, `declaration`, `timeout_s`, `packs_in`,
`packs_out` and nothing else; `herd-hub/src/server.rs:1642` says *"the
runner's arithmetic also uses the cap it puts on every job, which no
frame carries"*; and the four hub-side uses of the declaration are where
the brief put them (`job.rs:1690`, `matcher.rs:758`-`:834`, `:1565`,
`:1585`-`:1600`).

**The strongest evidence was already in the tree, and nobody had read it
against D87.** Three of the project's own texts state the truth:

- `herd-runner/src/confine.rs:203`-`:212` — *"every job here runs under
  the same kernel memory ceiling (`Confinement::memory_max`, one figure
  for every job and not a per-job negotiation)"* and *"the cap is what a
  job is permitted, not what it takes"*. The product's own voice, and
  the source of the word *permitted* used in all three corrections.
- `docs/decisions.md:8923` — *"the runner caps every job at the same
  kernel memory ceiling whatever it declared"*.
- `docs/remediation/09-runner-self-limits.md:190`-`:196`, finding **F**
  — the same sentence with `Confinement::memory_max` cited.

So this was not an unknown fact. It was a known fact that never reached
the entry other documents cite, which is **D278** precisely.

**The unmeasured consequence, which is the part that matters.** A job
that honestly declares 1.7 GiB runs today with the runner-wide kernel
ceiling and nothing else. Nothing in Herd has ever measured whether real
jobs stay inside their declarations, because the declared figure never
reaches the only process that could compare it with what the job took.
Admission's arithmetic is sound; its inputs are unaudited. This is
adjacent to ORCH-268's `peak_memory` half but is not the same row: that
one is about a record asserting an unmeasured figure, this one is about
a figure nothing can measure from where it sits.

**What is *not* wrong, and must not be "fixed".** D87's **choice** —
admission costs the declaration, never a measurement — is right for the
reason D87 gives, and the corrections leave it standing. The kill
mechanism is also real: a job that exceeds the **runner's** cap is
OOM-killed as a group (`cgroup.rs:662`-`:666`, `memory.swap.max` 0 and
`memory.oom.group` 1) and reported `stopped_by_cap` with `cap: memory`
(`herd-runner/src/job.rs:1092`). Only the identity of the figure was
false. `matcher.rs:479`'s *"(exit 124)"* was checked and is **not** a
second error: 124 is the code every cap wears
(`herd-proto/src/lib.rs:955`).

**Search provenance.** `grep` here is ugrep 7.8.4. A line-oriented
search cannot find this claim, because the sentences wrap; the sites in
`architecture.md` and `handover.md` were found only by a
paragraph-unwrapping search. The detector was proved on the known
positive (`docs/decisions.md:1924`, exit 0) before its silence elsewhere
was trusted. Nine patterns were used; they and the full hit list are in
the agent's report. An early run returned **exit 2 with no output** from
an empty ERE alternation that ugrep rejects — a silent false negative
that would have passed for "no other sites", and the reason each pattern
was then run on its own with its exit code checked.

## Drafted for `herd-merger`

Two artefacts, both in full. **Nothing here is a pointer to the agent's
report.**

### 1. The `docs/decisions.md` entry

Append verbatim; the merger allocates the number. Highest real `D` on
`main` is **D280**, re-measured rather than taken from the brief: every
non-ancestor ref (13 local, 29 remote) was checked and the highest `D`
on any of them is **184** (`merge-queue-target-deleted`), so nothing
above 280 is in flight and no `D281`+ appears anywhere in `docs/` or
`.claude/`.

```markdown
## 2026-10-31 — D-NEXT The kernel enforces the runner's cap, not the job's

**Context.** D87 chose that admission costs what a job **declared** and
never a measurement. That choice stands. Its warrant does not.
`docs/decisions.md:1924`-`:1926` reads *"The declaration is honest
because M4's cap already stops a job that exceeds it: a job over its
figure is killed, not tolerated, so the figure is a ceiling the kernel
enforces and not a guess."* That sentence is false, and it was the
warrant copied into three other files.

**What the code does.** `herd-runner/src/cgroup.rs:660` is the only
`memory.max` write in the product. It writes `limits.memory_max` inside
`JobCgroup::create` (`cgroup.rs:644`), and that `limits` is one
`JobLimits` built once at startup from `--job-memory-max`:
`main.rs:525`-`:530` → `confine.rs:180` `job_limits()` →
`confine.rs:430` `Ceiling::detect` → `cgroup.rs:603` `open_for_job`,
which hands the same `limits` to every job it opens. There is no per-job
branch. **The kernel enforces the runner-wide cap, identically for every
job, and never the job's declared figure.**

**And it could not do otherwise today, because no frame carries that
figure.** `FromHub::AssignJob` (`herd-proto/src/lib.rs:2375`) carries
job id, attempt, command, `declaration`, `timeout_s`, `packs_in` and
`packs_out`. `Declaration` (`herd-proto/src/declaration.rs:235`) is
command + commit + args + executor. `Requirements`
(`declaration.rs:516`), which holds `memory_mib`, is written onto the
record and read by the hub's own matching; it never reaches the runner,
and `requirements` appears nowhere in `herd-runner/src/`. The hub says
so itself at `herd-hub/src/server.rs:1642`: *"the runner's arithmetic
also uses the cap it puts on every job, which no frame carries"*.

**Choice. The cap bounds what a job is *permitted*; the declaration is
what the hub reserves against and places on. Two figures, and this
project says so wherever it says either.** The phrasing is the runner's
own, already in the tree at `herd-runner/src/confine.rs:203`-`:212`:
*"one figure for every job and not a per-job negotiation"*, and *"the
cap is what a job is permitted, not what it takes"*.

**What the declaration is actually for — four uses, hub-side, none of
them enforcement.** Admission arithmetic (`herd-hub/src/job.rs:1690`
`memory_need_mib`; `matcher.rs:758`-`:834` the room check, `:1565` the
reservation, `:1585`-`:1600` the *"admitted X, Y free of Z after it"*
sentence); **placement** (D88 — a need larger than the machine has in
all is "not here ever", so the figure decides whether a job waits or
sticks); the job record and its explanations (E-15, E-36); and
`status.rs`.

**The consequence, plainly.** A job that honestly declares 1.7 GiB runs
today with the runner-wide kernel ceiling and nothing else. So
**nothing in Herd has ever measured whether real jobs stay inside their
declarations**: the declared figure never reaches the only process that
could compare it with what the job took. Admission's arithmetic is
sound and its inputs are unaudited.

**What this does not change.** D87's choice, untouched, and right for
the reason D87 gives: a job admitted a second ago has allocated
nothing, so a hub costing real usage admits a second job beside it and
both then grow. D148 keeps D87's text where it is; this stands beside
it.

**Corrected in the same pass**, each as a reading of the code with the
line that decides it: `docs/coverage.md:216` (E-29's note, **State cell
unmoved**, tally re-derived at 36 / 5 / 0 / 1 / 2 = 44 over 55),
`docs/architecture.md:762`-`:770`, and `docs/handover.md:425`-`:435`,
where the recommendation survives and only its reason changes. Two
`.rs` doc comments still carry the false warrant and were outside a
prose-only task: `herd-hub/src/job.rs:880` and
`herd-hub/src/matcher.rs:479`.

**Why it went stale, and why this one cost more than a wrong sentence.**
**D278**'s shape exactly — a sourced sentence false because the thing it
cites moved. This is its clearest instance, because the sentence was the
**warrant** for a design the consumer is now asking us to change. Three
of the project's own texts already said the true thing
(`herd-runner/src/confine.rs:203`, `docs/decisions.md:8923`,
`docs/remediation/09-runner-self-limits.md:190`) and nobody had read
them against D87. A claim about enforcement is worth checking against
the line that writes the knob, and there is exactly one.
```

### 2. The `board.md` row

Already inserted on this branch at the end of *Owed* as `ORCH-NEXT`,
which escapes `check-docs.sh`'s duplicate-id check (it matches
`^\| ORCH-` for the cell and state checks but not `^\| ORCH-[0-9]+`) and
so is safe to carry until numbered. Highest real row id on `main` is
**ORCH-311**; ORCH-777, ORCH-998 and ORCH-999 are fixtures. The full
text is the row at `docs/orchestration/board.md:197` on this branch — it
needs only its number, nothing rewritten.

## Decisions needed

None from this task. One **product** question is adjacent and is **not**
this task's to ask, and is already named in the board row so it is not
lost: whether the kernel should enforce a job's declared figure rather
than the runner-wide cap. That is a protocol change (`Requirements` on
`AssignJob`), it touches what Herd *promises*, and it is Alexa's — the
brief says the consumer is already asking for it. This task only stops
the record claiming it is already true.

## Resume point

`herd-merger`: allocate the `D-NEXT` number and the `ORCH-NEXT` id when
this branch is integrated into its batch. Then, as the board row's own
next action, one `herd-step` task correcting
`herd-hub/src/job.rs:880` and `herd-hub/src/matcher.rs:479` — a `.rs`
change, so the four Rust gates run for it.

## Processes

None started. No build, no suite, no bench, no compile: the only
commands run were `git`, `grep`/`ugrep`, `awk`, `perl`, `sed`,
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`. No
`CARGO_TARGET_DIR` was created, so there is no build tree to delete; no
PID was signalled, and `pkill` was not used in any form.
