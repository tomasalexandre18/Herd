# shared-host-and-selectors — handoff 2026-10-30

- **Item:** ORCH-246 (corrected), ORCH-235 (amended), and ORCH-247,
  ORCH-248, ORCH-249, ORCH-250 (filed)
- **Agent:** herd-step
- **Branch / commit:** `shared-host-and-selectors` / the commit this file
  is committed in, based on origin/main **`3f3a07c`**
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**1. `CLAUDE.md`'s per-tree build figure is corrected — with its category
and not only its value — and the added line is paid for.** *Shared host
rules* said `~8.5 GB per tree`. Seven build trees measured on 2026-10-30
came back 12 to 14 GB, and **all seven are `debug` trees carrying test
binaries**, because every one was built by `./scripts/test.sh` or
`gate.sh`; a **`--release` build of the whole workspace at `v0.4.1` is
283 MiB** — 292,415,975 bytes, 101 crates, 52 s — which is forty times
smaller and the same repository. The line now reads *a **debug tree with
tests** is **~12–14 GB** (seven, 2026-10-30; `--release` is 283 MiB)*, so
the figure that bounds how many heavy agents fit is the headline and the
one answering a different question sits beside it. That ordering is the
point: without it, the next reader to measure a release build *corrects*
12–14 GB down to 283 MiB and halves the ceiling on a true number.

**What the line no longer says, for want of columns:** that ~8.5 GB was
low by half. The five-line bullet is full, and the sentence that had to go
is the one about the old figure rather than anything about the new ones.
It is in ORCH-250 and in this file.

All three files that carry the figure describe the same quantity the seven
reports measured — a `CARGO_TARGET_DIR` debug tree, not a worktree and not
a log directory — so the brief's escape hatch (*if the five reports are
measuring something else, change nothing*) does not apply: `CLAUDE.md:113`
said *per tree* inside a sentence about deleting `target/`,
`workflow.md:623` says *of build tree each*, and `disk-policy.md:1535`
says *per build tree*.

`CLAUDE.md` is held at **150 lines** by `scripts/check-docs.sh:21`-`:24`
and stood at exactly 150, so the one added line had to be paid for. It is
paid by re-wrapping invariant 8 from eight lines to seven — **a pure
reflow, proved and not asserted**: `git diff --word-diff=porcelain
CLAUDE.md` shows no word added or removed in that block. The file is back
at 150 lines.

**2. ORCH-246 is corrected, and the error in it is named as the
orchestrator's.** The row read the 95 idle `herdctl` processes as *an
earlier agent's residue* and treated a later census of zero as a second
data point on whether a requester has a silence bound. Both are wrong:

- They were the first consumer's **production nightly requesters**, read
  off `ps -o args` an hour later. uid **1002** (`server`) carries their
  nightly and our own test fixtures alike and `comm` is `herdctl` for
  both, so *our uid* and *nobody living started these* prove nothing.
- **Not one of them exited on its own.** 78 were killed by the
  orchestrator by explicit PID between 08:45 and 09:00; the rest by the
  consumer at 09:31 on Alexa's request — `TERM` on pgid **256928**, 415
  processes — then `herdctl cancel` on **126** open nightly jobs, 125
  queued and one running (`j0000003749`): 126 cancelled, **0 failures**.
- The row's **second reading is refuted by the same measurement**: a
  production loop's requesters are not a test harness's clients, so no
  harness failed to reap them.
- The evidence that **does** bear on the hypothesis is in the row now:
  all 78 in state **`Sl`**, alive and blocked and **not `Z`**, and 71 of
  them with **0 seconds of CPU between them**, the oldest over 5 h 30 m
  old — consistent with a blocking wait that has no timeout, and the
  opposite of an unreaped zombie.
- Batch 18's merger is credited, not blamed: it reported what it could
  see and said in the same breath that it could not tell *exited on their
  own* from *something reaped them*. It had not been told of the kills.

**3. ORCH-235 is amended with batch 18's fourth sighting**, rather than a
fifth row being filed, because that row already carries this family and a
fourth instance measures its shape. The generalisation is kept verbatim:
*"a branch's literal inventory is complete about its own base, so the
inventory cannot cover a file the other branch invents — which makes
'compile the merged tree' not a double-check but the only check."* The
instance is verified in the tree: `herd-hub/tests/pack_frames.rs:843`
holds the `ToHub::SubmitJob` literal, and `git log --diff-filter=A` says
that file was created by **`c8f1fa1`**, on `pack-assignment` — so no
inventory taken on `detached-end-event`'s base could have named it.

**4. Four rows filed**, `check-docs.sh` green on each:

- **ORCH-247**, `open`, `herd-planner` — the consumer cannot ask which of
  its jobs are open, and read Herd's state root to find 126 ids. Verified
  against the tree at `3f3a07c` before filing, as the brief asked:
  `herd-hub status` does **not** already answer it, and the reason is the
  useful part. The status document carries `running` and `queued` with
  each job's id, priority and labels (`herd-hub/src/status.rs:155`-`:157`,
  `:328`, `:349`), so the information exists — but it is a file in the
  state root read from the **operator's** side, which is exactly what the
  consumer had to do. And it could not be narrowed to one requester even
  if a requester could read it: nothing on the record, on the status
  document or on `ToHub::SubmitJob` names who submitted a job, and Q8
  gives **one shared requester token** (`herd-hub/src/main.rs:341`). So
  *its own work* is not expressible in this build, which is what puts the
  legitimate form of a listing **behind** M15 rather than in front of it.
- **ORCH-248**, `owed`, `herd-step` — ORCH-234's record half, left to the
  orchestrator by batch 18's *Left out*. The five implementors were
  re-counted in the tree rather than copied from the brief: the trait at
  `herd-hub/src/job.rs:2087`, then `store.rs:2011`, `server.rs:9157`,
  `tests/cost_surface.rs:126`, `tests/unsynced_write_cost.rs:682` and
  `tests/junction_breakdown.rs:197`.
- **ORCH-249**, `owed`, the orchestrator — why the process-ownership rule
  is not in `CLAUDE.md`, with its text ready to paste and three payments
  sized. See *Not done* and *Decisions needed*.
- **ORCH-250**, `owed`, `herd-step` — the same stale ~8.5 GB figure in
  `workflow.md:623` and `disk-policy.md:1535`, neither of which this
  branch's paths allowed it to touch; `disk-policy.md:1536`-`:1537` is
  stale a second time in the same sentence, still saying *at most two
  heavy agents* after Alexa lifted that on 2026-10-27.

## Not done, and why

- **The process-ownership rule is not in `CLAUDE.md`, and this is the one
  thing the brief asked for that did not land.** The arithmetic, measured:
  `check-docs.sh` holds the file at `limit=150`, `wc -l` is 150, and the
  detector was **proved** rather than assumed — appending one line returns
  `CLAUDE.md is 151 lines, limit 150` and exit 1. A greedy re-wrap of
  every paragraph in the file at 79 columns frees exactly **one** line (in
  invariant 8, nowhere else), and that line is spent on the disk figure
  above. The tightest wording that keeps all three things the brief asked
  for is **four** lines replacing the existing one-line kill rule, so the
  rule needs **three** lines that do not exist. What leaves `CLAUDE.md` to
  make room is a ruling on what the contract says, which is not a step's
  to make — and raising the ceiling is `scripts/check-docs.sh`, outside
  this brief's paths. Filed as **ORCH-249** with the text, so nothing is
  lost, and it is also in *Decisions needed* below. **No rule was deleted
  to fit, and no gate was reddened to ship it.**
- **`docs/orchestration/workflow.md` and `docs/disk-policy.md` are not
  edited**, though both carry the figure this branch corrected. Outside
  the paths the brief allowed, so a finding and a row (**ORCH-250**), not
  an edit.
- **`docs/decisions.md` is untouched** and no `D` number is allocated. The
  drafted entry is below, headed `D-NEXT`.
- **ORCH-246's discriminator was not run** — start a hub, start one
  `herdctl wait`, kill the hub by PID, time the client. The brief says not
  to and the row now says so too: it is a product question for a step with
  a brief, not something to slip into a prose branch.
- **`board.md`'s *Next* section is not restated.** Six rows moved or
  arrived and that section names none of them; restating it is the
  merger's at landing, and this branch kept to added rows plus the two
  corrections it was sent to make.
- **No `cargo`, and the four Rust gates were NOT RUN.** This is a
  prose-only change (`CLAUDE.md`, `docs/`), so `check-docs.sh` and
  `check-orchestration.sh` are the gates; the Rust gates are covered by
  the batch's own `./scripts/gate.sh` at integration, which is
  `herd-merger`'s and not this step's.
- **No measurement of my own for the disk figure.**
  `/mnt/ssd_980/herd/build/` is empty — every agent deleted its tree — and
  no `cargo` was allowed, so there was nothing left to `du`. Two of the
  seven figures are in the tree and are cited; five are from agent reports
  of 2026-10-30 that the tree does not carry. See *Risks*.

## Gates

**Run before this file was committed**, on the tree carrying the
`CLAUDE.md` edits and all six board changes, and with this handoff present
in the working tree — both gates read the working tree and not the index,
so this file was inside what they checked:

```text
./scripts/check-docs.sh
docs ok
CHECK_DOCS_EXIT=0

./scripts/check-orchestration.sh
... 324 ok lines, no failure ...
check-orchestration: 324 passed, 0 failed

real	0m40.745s
user	0m24.015s
sys	0m20.183s
CHECK_ORCH_EXIT=0
```

An earlier pair, run before the build-profile correction landed, returned
the same two verdicts: `docs ok` and `check-orchestration: 324 passed, 0
failed` in 40.690 s. `check-docs.sh` was also run after each single edit;
every run was green and none is reported as covering a later tree.

**The ceiling detector was proved, so the green is a measurement.** One
line appended to `CLAUDE.md` gives `CLAUDE.md is 151 lines, limit 150` and
`docs check failed`, exit 1; the file was then restored with `git
checkout` and `wc -l` read 150 again.

**The last run** is `./scripts/check-docs.sh` followed by
`./scripts/check-orchestration.sh`, on the tree with this handoff
committed, before the push. Their output goes to the agent's report and to

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/shared-host-and-selectors/work/gate-docs-final.txt
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/shared-host-and-selectors/work/gate-orch-final.txt
```

**Skips: no skip.** Neither gate prints a `HERD-SKIPPED` line and neither
has a skip list; nothing was counted as a pass that was not one (D27).
**The four Rust gates were NOT RUN locally** — prose only — and are
covered by the landing's `gate.sh`.

## Risks and findings

1. **The brief's base sha was stale and the branch was rebased, which is a
   deviation stated rather than hidden.** The brief said to branch off
   `27a1040` as *current origin/main*; `git rev-parse origin/main` said
   **`3f3a07c`** — batch 18 had landed. The branch was created at
   `27a1040` and rebased onto `3f3a07c` before any edit, for three
   reasons: ORCH-244, ORCH-245 and ORCH-246 did not exist at the old base
   and two of the tasks need them; the highest row id is 246 and not 243,
   so the ids taken here are **247 to 250**; and `CLAUDE.md` is untouched
   by batch 18, so neither edit changed meaning. The coordinator confirmed
   the same rebase mid-task.
2. **`CLAUDE.md` is full, and that is a standing trap, not an incident.**
   The next true sentence anybody measures has nowhere to go, and the
   failure mode is that it goes nowhere silently — or that somebody
   deletes a rule to fit. ORCH-249 carries it; it is the finding most
   worth reading twice.
3. **ORCH-246 carried a second falsehood beyond the one this step was sent
   to fix, and both were corrected.** *They are an earlier agent's
   residue* is false in the same way and from the same cause, and the
   harness reading it offered is refuted by the ownership measurement.
   Leaving them while correcting the sentence beside them would have left
   the row self-contradictory. **This was this step's judgement**, not an
   instruction, and it is reversible by reading the row.
4. **One reading in ORCH-246 is this step's own and is hedged as such.**
   The consumer's hub (PID 3006543) was up since 20:23 and untouched that
   morning (`history/2026-10-30-merge-batch-18.md` *Host*), and 125 of the
   126 jobs were still **queued** — so those requesters were blocked on
   jobs that had not ended, which is `herdctl wait` keeping its promise
   rather than missing a bound. The *hub went away* case the hypothesis is
   about was never the case here. Stated in the row as a third reading,
   explicitly not resolved; the discriminator stays the right experiment.
5. **Five of the seven disk measurements are not in the tree, and the
   release figure is not either.** In the tree:
   `history/2026-10-30-pack-fetch.md:566` (13 GB) and
   `history/2026-10-30-pack-registry.md:629` (12 GB), both debug. The
   other five — 12, 12, 12, 14 and 14 GB, from batches 15, 16 and 17 and
   the branches `runner-announces` and `detached-end-event` — and the
   283 MiB `--release` figure come from the orchestrator's brief, which
   says those agents reported them; their committed handoffs name their
   `CARGO_TARGET_DIR` by path and not its size. **No measurement of my
   own was possible:** `/mnt/ssd_980/herd/build/` is empty, every agent
   having deleted its tree, and no `cargo` was allowed. So the
   `CLAUDE.md` line says *seven* and cites no file, and the provenance in
   full is here and in ORCH-250. If the orchestrator wants these anchored
   in the tree, the cheapest fix is one line in each future handoff's
   *Processes*: the `du -sh` before the delete, and the profile.
6. **The category correction arrived mid-task and before the push, and it
   is the measurement that matters more than the arithmetic.** The brief
   said the figure was understated and five agents had measured 12 to
   14 GB; it did not say all five were debug trees with test binaries,
   and a `--release` workspace at `v0.4.1` is **283 MiB** — forty times
   smaller, same repository. Had the line landed saying only *~12–14 GB
   per build tree*, it would have been true and still misleading, and the
   predictable next correction would have been the wrong way round. **For
   the record the orchestrator asked for:** this is the **tenth** figure
   of its own corrected by an agent since 2026-10-29, and the first where
   the measurements were right and the **category** was wrong — which is
   why the line now names the build profile before the number, and why
   ORCH-250 obliges the two sibling files to do the same.
7. **A `perl` one-liner double-encoded `CLAUDE.md`'s existing UTF-8, and
   the width check is what caught it.** Writing an en dash as `\x{2013}`
   in a `perl -i -0777 -pe` script upgrades the whole record to character
   semantics, so every byte already in the file is re-encoded: four
   unrelated lines jumped from 78-79 to 80-82 columns. Reverted with `git
   checkout` and redone with the raw bytes `\xe2\x80\x93` under
   `LC_ALL=C`, with a mojibake probe after. **Worth a gotcha entry**: the
   board's traps list warns about pipes and widths in `board.md` and says
   nothing about this, and it is silent except for the width check.
8. **ORCH-249 and ORCH-250 were not asked for.** Both are measurements
   this step made and could not act on inside its own paths, and invariant
   8 says the board is where open work goes rather than a handoff. If
   either is unwanted, deleting a row is cheaper than recovering a
   measurement nobody wrote down.
9. **The `ps` evidence in ORCH-246 is the orchestrator's measurement, not
   re-measured here.** The processes are gone, so the 78 in `Sl`, the 71
   at 0 s of CPU and the 5 h 30 m age cannot be re-read. They are
   attributed in the row to the orchestrator's census, which is what a
   later reader needs to know in order to doubt them.

## Drafted for `herd-merger`

**The six board rows are filed on this branch, not drafted:** ORCH-246 and
ORCH-235 are edited in place, and ORCH-247 to ORCH-250 are new rows in
`docs/orchestration/board.md`. There is nothing to paste for them. Only
the decision below is drafted.

**One `docs/decisions.md` entry, in full, to paste at the end of the file
with its `---` separator. The merger allocates the number and gaps stay
(D148, invariant 8); the merger also judges whether the incident earns an
entry at all.**

```markdown
---

## 2026-10-30 — D-NEXT Ownership, not age, decides what may be signalled

On the morning of 2026-10-30 the orchestrator killed **78** processes
under our own account, believing them to be a Herd agent's leak. They
were the first consumer's **production nightly requesters** — `herdctl
wait --outputs-to /mnt/ssd_980/chatons/agents/nightly/herd-j<id>
--verdict-to <path>` — and the kill was `SIGTERM` by explicit PID,
between 08:45 and 09:00. The rule in force was kept to the letter:
nothing was killed by name, no `pkill -f`, no `killall`. **The rule was
not sufficient.**

**What the incident measured.** Our account is `server`, **uid 1002**,
and the consumer's nightly runs under it, so our own uid is no proof of
ownership. Their hub runs as uid **1000** and their runner as uid
**986**, so a check that excludes those two known PIDs proves nothing
about a third. And `comm` is `herdctl` for our test fixtures and for
their production alike, so only the **full command line** separates
them. The filter actually used was **age** plus those two PIDs: careful
on the wrong axis, twice. One `ps -o args` would have shown the
`chatons/` path, and it was run an hour late.

**Decided.** Ownership is established from a process's full `args`, and
said out loud, before anything is signalled. uid, `comm`, age, PID and
name are **not** provenance. **And a report that declares its own lack
of provenance is not provenance either:** the agent that flagged these
wrote *"worth someone with provenance cleaning up; I had none"*, and the
orchestrator passed it on as fact. **The error is the orchestrator's**
and is recorded as such: no agent is at fault, and the one that reported
it said exactly what it did not know.

**What it cost beyond the processes.** The consumer stopped the rest of
its own loop at 09:31, on Alexa's request — `TERM` on pgid **256928**,
415 processes — and cancelled **126** open nightly jobs, 125 queued and
one running, with **0 failures**; the runner confirmed *"ending
j0000003749 because a requester cancelled this job"*. Cancellation
worked exactly as promised, and what was missing was a way to ask which
jobs were open at all (ORCH-247). The reading the kills destroyed is
recorded in **ORCH-246**: not one of those processes exited on its own,
so a later census of zero is no evidence about a requester's silence
bound, and the evidence that does bear on it is that they were alive,
blocked and burning no CPU.

**Where the rule lives.** `CLAUDE.md` stands at exactly its 150-line
ceiling (`scripts/check-docs.sh`), a full re-wrap of the file frees one
line, and that line was spent correcting the per-tree disk figure in the
same branch. The rule's text — four lines wrapped at 79 columns — and
the three ways to pay for them are in **ORCH-249**, owner the
orchestrator. Until that ruling, this entry is where the rule is
written down.
```

## Decisions needed

1. **What pays for the three lines the ownership rule needs in
   `CLAUDE.md`?** — *Recommendation:* raise the ceiling at
   `scripts/check-docs.sh:23`, once and with the reason recorded, then
   compress three lines of `CLAUDE.md` if the ceiling is to stay: the rule
   binds every agent on every task and belongs in the file they all read,
   and a ceiling is what should move when the contract genuinely grows.
   *Fallback:* put the rule in `docs/orchestration/workflow.md`'s
   *Concurrency and host rules*, which has no ceiling and which
   `CLAUDE.md:108` already points at for this section's exceptions — one
   hop further from the file every agent reads first. The three payments
   are sized in **ORCH-249**; this is a method ruling and inside the
   orchestrator's mandate, not Alexa's.
2. **Does ORCH-247 go to Alexa's *Waiting*, or stay a planner's row?** —
   *Recommendation:* file it as it stands, `open` and `herd-planner`,
   until the shape is sized, and take it to Alexa only as the one question
   *does `herdctl` grow a way to ask about a requester's own jobs before
   v1.0.0* — because a new requester-facing surface is what the product
   **promises**, which is his, while which flags it carries is not.
   *Fallback:* leave it a planner's row for v1.0.0 and tell the consumer
   to read `herd-hub status`, which is what it did.

## Resume point

The branch is pushed and lands nothing. For whoever continues:

```sh
git fetch origin && git log --oneline origin/shared-host-and-selectors -6
```

Then, in order of what is cheapest to lose: rule on **ORCH-249** so the
ownership rule can be written where it binds; brief a step for
**ORCH-250** (prose only, two files, both already located by line); and
size **ORCH-247** with M15 in view, since *my own jobs* needs an identity
this build does not have. **ORCH-248** is ready to brief as it stands and
owes all four Rust gates, being a crate change.

## Processes

- **No background process was started.** Every command was a foreground
  child that had exited before the next step: `git`, `perl`, `awk`, `sed`,
  `grep`, `wc`, `du`, and the two gate scripts. Nothing was signalled,
  nothing was killed, no `pkill`, no `killall`, no kill by name — and
  nothing of the consumer's was inspected beyond `ls` on
  `/mnt/ssd_980/herd/build/`, which was empty.
- **No `target/` and no `CARGO_TARGET_DIR`.** No `cargo` ran, so none was
  created and there is none to delete; `./scripts/bench-preflight.sh` was
  not run because there was no heavy phase to gate.
