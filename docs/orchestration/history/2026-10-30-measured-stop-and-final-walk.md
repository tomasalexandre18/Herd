# measured-stop-and-final-walk — handoff 2026-10-30

- **Item:** ORCH-270, applied twice — two measurements whose conclusions
  lived only in a chat message. No id of its own was given to this task.
- **Agent:** herd-step
- **Branch / commit:** `measured-stop-and-final-walk` / this commit, based
  on origin/main `83bc78a`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A1 — invariant 2's five seconds is in the tree, measured, with its
scope stated in the same breath as the figure.**

`docs/joining.md`, immediately after the fenced block that already
quotes a stopping runner's own two lines, now carries one paragraph: the
figure (**0.212 s**, SIGTERM to process exit, a 24× margin), its
provenance (2026-10-30, our own queue runner, v0.4.1, one host, one
version, `kill -TERM` by PID), and what it does **not** cover — the
runner was **idle**, `0 job(s) to end`, so the measurement is of the
stop path with nothing to drain, and the draining path with work in
flight, which is the case the bound exists for, remains untimed. The
paragraph says to read 0.212 s as the floor of the stop path and not as
the answer for a runner draining a three-hour build.

The verbatim lines the measurement produced, which `docs/joining.md`
already quoted before this change and which this change did not touch:

```
herd-runner: draining (the runner's owner asked it to stop (SIGTERM)); 0 job(s) to end, no new job accepted
herd-runner: stopped
```

`docs/operating.md`'s *The stop drains* bullet gains a pointer and **no
figure**: it says the five seconds has been timed once and not on the
case that bullet describes, and names `docs/joining.md` as the one place
the figure lives. The number is written down once on purpose; two copies
of a measured figure is how a figure drifts.

**Half B — two stale pointers outside the repository, done, verified,
and not part of this branch.** Reported in full in *Risks and findings*
below, because nothing of it is a repository change.

## Not done, and why

**A2 and A3 are drafted, not landed, because the brief's premise is
refuted: the gotcha they amend is not on `main`.**

The brief says A2 "sharpens what landed in
`docs/orchestration/gotchas/bytes-written-is-the-workspace-not-the-job.md`
on `main`". That file does not exist at `83bc78a`, nor anywhere on
`main`. It exists on **`origin/bytes-written-measured`**, added by
`165879a` *"Measure what a job's bytes_written actually measures"*, with
`d9795b9` on top; `git branch -a --contains 165879a` names that branch
and no other, so it is in no integration branch either. It is an
unlanded sibling, presumably of this same batch.

Creating that path on a branch based on `main` would hand `herd-merger`
an add/add conflict over a file whose existing measurements the brief
explicitly says to keep intact — the exact way those numbers drift. So
the amendment is written out in full under *Drafted for `herd-merger`*,
anchored to a unique line of the file, to be applied **after**
`bytes-written-measured` is merged onto the integration branch. The
measurement is in the tree, in this file, which is what ORCH-270 asks;
it is the gotcha's own body that waits one merge.

**A3's placement, decided: inside the gotcha being amended, not its own
file.** Three reasons. The `du -sb` trap is not a general measuring
lesson detached from that file; it is the failure mode of that file's
own *Verify* section, which tells a reader to measure a workspace. The
reason it matters is specific to this bug — the product's walk sums
apparent size, so a measurer reaching for `du -b` confirms the bug with
the bug — and that sentence is unreadable away from the bug. And the
paragraph above it warns that a reader who fixes only the sampler fixes
half of it; splitting the lesson across two files is the same failure
one level up. The cost is accepted and named: a lesson inside another
file's body gets no row of its own in `gotchas/README.md`, so the index
line proposed below carries the words `du` and `apparent size` to keep
it findable by the person who will reach for the wrong tool.

**Nothing was written to `docs/decisions.md` or
`docs/orchestration/board.md`**, per the brief. Both drafts are below.

**`docs/plan.md` was not touched.** Its line 62 claims *"a draining busy
runner exits ≤ 5 s"* as an M9 property, which is precisely the case A1's
measurement does **not** cover. Correcting a milestone's claim is wider
than this task and is proposed as a board row instead.

**`docs/runner-confinement.md` was not touched.** Its line 83 says *"the
disk budget is measured on what the job wrote, not on what it let the
runner see"*, which the sibling branch's measurement refutes — it is the
apparent size of a tree, and for an ad-hoc job it excludes the build
entirely. That sentence is inside the scope of that branch's own drafted
*Owed* row (naming the figure for what it covers), not of this task.

## Gates

`./scripts/check-docs.sh`, run on this tree **before** this handoff was
committed, over the two product-docs changes alone:

```
docs ok
```

The **Rust gates were NOT RUN** — `cargo build`, `./scripts/test.sh`,
`cargo clippy`, `cargo fmt`. This change is prose only: three markdown
files under `docs/`, no product file. Under `CLAUDE.md`'s docs-only rule
what covers them is the batch's own `./scripts/gate.sh` at integration,
which is `herd-merger`'s run and not this agent's. A gate that did not
run is reported as not run.

The **last run** is `./scripts/check-docs.sh` on the committed tree,
this handoff included, run after this file was committed and before the
branch was pushed. Its output is in this agent's report to the
orchestrator and not in this file, because the tree it gated is the tree
that was pushed.

**No skip.** `check-docs.sh` needs no toolchain and prints none; no
`HERD-SKIPPED` line was produced or copied, and the suite that produces
them was not run.

## Risks and findings

**1. The brief's A2 premise is refuted (see *Not done*).** The gotcha is
on `origin/bytes-written-measured`, not on `main`. This is the one
finding that changes what `herd-merger` has to do.

**2. The final-walk path is enforced, not only recorded — verified in
the code on `main`, not inferred.** The sibling gotcha's framing dwells
on the 200 ms sampler, and its own drafted board row offers as a
fallback *"stop letting one 200 ms sample kill a job"*. That fallback
fixes half of it. Read on `main` at `83bc78a`:

- `herd-runner/src/job.rs:570` kills everything of the job;
- `:574` takes `let final_disk = workspace.measure();`, the walk after
  nothing of the job is running;
- `:582` guards on `ran.nothing_ended_it()`, and `:590`-`:599` compare
  **that** figure against `caps.disk_budget` and set
  `ran.cap = Capped { cap: Cap::Disk, limit: caps.disk_budget }`;
- `:624` is the separate thing — `max(peak_disk, final)` — which is the
  figure the *record* carries;
- `:1521` is the sampler's own comparison, every `POLL_INTERVAL` =
  200 ms (`herd-runner/src/confine.rs:106`).

So a job that outlives no sampler tick at all is still turned from its
own verdict into `stopped_by_cap` / `disk` by the final walk. The
product documentation already says this and is not wrong:
`docs/runner-confinement.md:172`-`:176`, *"the runner checks the caps
once more after the process is gone, and the cap wins"*. It is the
gotcha's framing, and that drafted fallback, that are half the story.

**3. The 30 GiB control proves the figure, not the kill.** It ran under
a 64 GiB budget and recorded 32,212,254,720, so it was **recorded** over
30 GiB and **not capped**. That the final walk also *kills* is finding 2
— read in the code, on `main` — and not something the control observed.
The drafted amendment says so in those words. Anyone restating it should
keep the two apart.

**4. The 30 GiB control's job id is not in the record this was written
from.** The brief gave the figures (65 ms, 32,212,254,720, `stat -c %b`
= 0 blocks) without an id, and this agent did not submit the job. The
drafted amendment marks the provenance that way rather than inventing an
id; a later task can fill it from the queue's own records.

**5. Half B, in full — three acts outside the repository, in
`/mnt/ssd_980/herd/queue/log/`, none of them on this branch and none
committed.**

*The live PID was verified first, not taken from the brief.*
`ps -o pid,uid,args -p 662711` reports uid 1002, binary
`/mnt/ssd_980/herd/queue/bin/v0.4.1/herd-runner`, with
`--job-disk-budget 64GiB`, started 16:20:19, PPID 1.
`ps -p 607643` returns nothing: that process is gone.

- **`runner.pid`** read `RUNNER_PID=607643` and now reads
  `RUNNER_PID=662711`, one line, nothing else in the file.
- **`runner.log`** keeps every byte it had and gains a delimited note at
  the end: 607643 is gone and must not be acted on because the kernel
  may recycle the number; 662711 is live, with its binary, its start
  time and its disk budget; its own output is **not** appended below,
  because its stdout and stderr were opened on a session scratchpad file
  and no process's open descriptor can be redirected from outside
  without restarting it; and where to read it instead.
- **`runner-current.log`** is new in that directory: a **hard link** to
  the inode the live runner's fd 1 and fd 2 are open on. Both names are
  one inode (verified: inode 37523443, link count 2, same device 66305,
  both under the `/mnt/ssd_980` mount), so the canonical directory shows
  the live output as it is written, and the output survives the pruning
  of the scratchpad directory it was opened in.

*Why a hard link and not a move.* A move would have worked — the fd
follows the inode — but it would have had to land on `runner.log`, the
only name in that directory this task was allowed to write, and that
would have destroyed the dead runner's own startup report. That report
is not redundant: it announces **16 GiB** and **2 job slots** and a
**20 GiB** disk budget, where the live runner announces **12.1 GiB**,
**1 slot** and **64 GiB**. A move would also have removed a path another
agent may be watching. The link is additive, breaks nothing and needs no
restart. **The runner was not restarted, not signalled and not touched**;
it was alive at 09:24 elapsed after the change, with fd 1 unchanged.

*What is still wrong there, and is nobody's single act.* The live
runner's own descriptors still point into a session scratchpad. The link
makes the bytes durable; it does not make the next start correct. Only a
restart puts the canonical path on fd 1, and this task was told not to
restart. The note says so to the next reader, and the board row below
asks for it to be done at the next restart that happens for another
reason.

*Revert, exactly.* `rm /mnt/ssd_980/herd/queue/log/runner-current.log`
removes the link and nothing else — the scratchpad name keeps the inode
alive. Byte-for-byte copies of both files as they were before the change
are at
`…/scratchpad/measured-stop-and-final-walk/runner.pid.before` and
`…/runner.log.before`, under this session's scratchpad.

**6. Token files were not read.** `etc/runner-token` and
`etc/requester-token` were neither opened nor stat'd; nothing in this
task needed to speak about them.

## Drafted for `herd-merger`

### 1. The `docs/decisions.md` entry, in full

```markdown
## 2026-10-30 — D-NEXT Invariant 2's five seconds, timed once, idle

Invariant 2 publishes that *"a runner stops cleanly in ≤ 5 s when its
owner asks, and refuses new jobs while draining"*. Until 2026-10-30
that bound was asserted in `CLAUDE.md`, in `docs/plan.md`, in
`docs/joining.md` and in `docs/operating.md`, and had never been timed
against a running runner.

**Measured 2026-10-30** on our own queue runner — v0.4.1, one host,
one version, idle, `kill -TERM` by PID — **SIGTERM to process exit
was 0.212 s**, a 24× margin against the published bound. The runner's
own two lines were `draining (the runner's owner asked it to stop
(SIGTERM)); 0 job(s) to end, no new job accepted` and `stopped`.

**What this entry does not record, and says so in the same breath as
the figure.** `0 job(s) to end` is the whole scope of the
measurement: it is the stop path with nothing to drain. The case the
bound exists for — a runner asked to stop while a job is running,
which has to end that job, take its verdict and write it down — is
**untimed**, and no figure in this project covers it. 0.212 s is the
floor of the stop path. It is not the answer for a runner draining a
three-hour build, and `docs/plan.md`'s claim that *"a draining busy
runner exits ≤ 5 s"* remains an assertion.

One host and one version is also the scope. Nothing here says what
the figure is on the first consumer's machines, and under D161 a fact
about their machines reaches us as a report with a job id and version
numbers, never as a measurement we take.

**What this decides: nothing about the bound.** It moves one half of
invariant 2 from asserted to measured, names the half that is still
asserted, and puts the figure in one place —
`docs/joining.md`'s section on stopping a runner — with
`docs/operating.md` pointing at it and carrying no copy of the
number, because two copies of a measured figure is how a figure
drifts.
```

### 2. The amendment to `bytes-written-is-the-workspace-not-the-job.md`

**Apply only after `origin/bytes-written-measured` is merged onto the
integration branch.** That branch adds the file; this task is based on
`main`, where the file does not exist, and deliberately does not create
it. If that branch is not in this batch, this block waits for the batch
that holds it, and nothing else in this handoff depends on it.

**Where:** in
`docs/orchestration/gotchas/bytes-written-is-the-workspace-not-the-job.md`,
insert the block below immediately **before** the line that begins
`- **Replaces:**`, which is the last top-level bullet and is unique in
the file. Nothing already in that file is restated, reworded or removed.

```markdown
- **Sharpened 2026-10-30, and it doubles the lesson: the sampler is
  not required at all.** A control job doing `truncate -s 30G` in its
  own `work/` ran in **65 ms** — **shorter than one 200 ms sampler
  period** — and recorded `bytes_written` = **32,212,254,720**,
  exactly 30.000 GiB, with `stat -c %b` reporting **0 blocks** of
  real disk. (Figure relayed from the control's own run; no job id
  came with it, and a later task can fill it from the queue's
  records.)

  **So there are two paths to the cap and not one.** The walk at
  `job.rs:574`, taken once after `kill_everything_of` at `:570`, is
  compared against the budget on its own: `:582` guards on
  `ran.nothing_ended_it()` and `:590`-`:599` set
  `ran.cap = Capped { cap: Cap::Disk, limit: caps.disk_budget }` from
  **that** figure. The `max(peak, final)` at `:624` is the separate
  thing — the figure the *record* carries. The sampler's own
  comparison is `:1521`, every `POLL_INTERVAL` = 200 ms
  (`confine.rs:106`).

  A job that outlives no sampler tick at all is therefore still
  turned from its own verdict into `stopped_by_cap` / `disk` by the
  final walk. **A reader who fixes only the 200 ms sampling fixes
  half of this**, and the 30 GiB control is the proof that the half
  left behind is enough on its own: it declared 30 GiB in 65 ms and
  the figure came out whole. What that control did **not** observe is
  the kill — it ran under a 64 GiB budget, so it was recorded over
  30 GiB and never capped. The kill on the final walk is read in the
  code above, and `docs/runner-confinement.md:172`-`:176` already
  states the behaviour in prose: *"the runner checks the caps once
  more after the process is gone, and the cap wins."*

  **A measurer reaching for `du -b` confirms this bug with the bug.**
  The agent that ran the control caught itself: `du -sb` **implies**
  `--apparent-size`, so it reported `32,212,254,720` and proved
  nothing whatever about disk. The product's own walk sums apparent
  size too — `metadata.len()` per file, *Why* 2 above — so the two
  agree for the same reason and the agreement means nothing. The real
  occupancy needs `du -s --block-size=1`, which counts blocks, and
  `stat -c %b`, which reports them per file; both read **0** for that
  30 GiB file. Any check of this gotcha's claims measures apparent
  size and real size with **different tools** and prints both.
```

### 3. One correction to that branch's own drafted board row

`history/2026-10-30-bytes-written-measured.md` drafts a *Waiting* row
whose Fallback cell ends *"and stop letting one 200 ms sample kill a
job"*. That is half the fix, per the amendment above. **If both branches
land in the same batch**, replace that clause with the one below; if
they do not, leave the row alone and this block expires.

```markdown
and stop letting the walk kill a job on a declared length — on either path, the 200 ms sample and the final walk after the kill, which are two comparisons against the budget and not one
```

### 4. The `board.md` rows, in full

One for *Owed*, A1's untimed half:

```markdown
| ORCH-NEXT | **Invariant 2's five seconds is timed on the idle path only; the draining path with work in flight is untimed, and it is the case the bound exists for.** Measured 2026-10-30: SIGTERM to process exit was 0.212 s on our own queue runner, v0.4.1, one host, idle, `0 job(s) to end` — a 24× margin on the easy half. A runner asked to stop while a job is running has to end that job, take its verdict and write it down, and no figure covers that. `docs/plan.md` claims a draining busy runner exits within the bound as an M9 property; that claim is still an assertion | `measured-stop-and-final-walk`, 2026-10-30; ORCH-270 applied to the drain timing | herd-step | owed | Time the busy drain: a runner holding a job that is really running, stopped by SIGTERM, with the job's own end and its verdict inside the figure — at least one short job and one job long enough that ending it is the whole cost. Then say in `docs/joining.md` which half each figure covers. **No bound changes**; if the figure comes out over five seconds, that is a product finding and its own row | — | `docs/joining.md`, the section on stopping a runner; `docs/operating.md` *The stop drains*; `docs/plan.md` line 62; `CLAUDE.md` invariant 2; `history/2026-10-30-measured-stop-and-final-walk.md` |
```

One for *Owed*, Half B's remaining half — the queue's own log has no
durable home at the next start:

```markdown
| ORCH-NEXT | **Our own queue runner writes its stdout and stderr into a session scratchpad directory, which is pruned.** Done 2026-10-30 as far as it goes without a restart: `runner.pid` names the live PID again, the frozen `runner.log` carries a note naming the dead PID and the live one, and `runner-current.log` in the same directory is a hard link to the inode fd 1 and fd 2 are open on, so the bytes outlive the prune. What a link cannot fix is the next start: only a restart puts the canonical path on fd 1, and the task that found this was told not to restart the runner | `measured-stop-and-final-walk`, 2026-10-30, Half B; the brief's own framing of the defect | herd-step | owed | At the next restart that happens for another reason, start the runner with its output on `/mnt/ssd_980/herd/queue/log/runner.log` and drop the note and the link. **Do not restart the runner for this alone**: every agent's queue depends on it, and the link already makes the bytes durable | — | `/mnt/ssd_980/herd/queue/log/runner.pid`, `runner.log`, `runner-current.log`; `history/2026-10-30-measured-stop-and-final-walk.md` |
```

### 5. One line for `gotchas/README.md`'s index

**Apply with block 2, and only with it.** The README row belongs to the
sibling branch, which adds it; this is an addition to that row's own
one-line cell, so that A3's lesson is findable by the person reaching
for the wrong tool even though it has no file of its own:

```markdown
; and `du -sb` implies `--apparent-size`, so it confirms this bug with the bug — measure real disk with `du -s --block-size=1` and `stat -c %b`
```

## Decisions needed

**None for Alexa.** Both measurements are *how*, not *what*: nothing
here changes what the product is or promises, and invariant 2's text is
untouched — the figure is published beside it, and the half that is
still an assertion is named as one.

One decision for `herd-merger`, which is procedural and inside its
mandate: whether `origin/bytes-written-measured` is in this batch. If it
is, blocks 2, 3 and 5 above apply after it is merged. If it is not, they
wait, and blocks 1 and 4 land unaffected.

## Resume point

For `herd-merger`: merge this branch, allocate the `D-NEXT` of block 1,
add the two `board.md` rows of block 4, and — only if
`origin/bytes-written-measured` is in the same batch and merged first —
apply blocks 2, 3 and 5 to the files that branch brings.

For the next `herd-step`: the busy-drain timing of block 4's first row.
It needs a job that really runs, our own instance, and the queue's one
build slot, so it waits on nothing in this branch.

## Processes

**No process was started, signalled or killed.** The live queue runner
PID 662711 was read with `ps` and `readlink /proc/662711/fd/1` and was
not touched; it was alive after every change. PIDs 607643 (gone),
674855, 565152, 3006543 and 3021002 were never addressed.

**No `target/` to delete:** no `cargo` command was run, no
`CARGO_TARGET_DIR` was set, and `./scripts/bench-preflight.sh` was not
needed because this task had no heavy phase. The worktree is at
`…/scratchpad/measured-stop-and-final-walk/wt` under this session's
scratchpad and holds no build output.
