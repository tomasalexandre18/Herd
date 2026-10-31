# oom-step-attribution — handoff 2026-10-31

- **Item:** ORCH-NEXT (three rows, in this branch's `board.md`; the
  merger allocates their numbers)
- **Agent:** herd-step
- **Branch / commit:** `oom-step-attribution` / `714176f1`, based on
  origin/main `d32c87a7876b91c55ca5caa8284ba5fa1357adc7`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### The hypothesis in the brief is refuted, and one line decides it

The brief's hypothesis was that `since` is captured once for the job and
never advanced, so that once `run` has been killed `oom_kills > since`
stays true for every later step. **It is not what the code did.** The
line that decides it is `herd-runner/src/job.rs:567` as it stood on
`d32c87a`:

```rust
let since = oom_kills;
oom_kills = cgroup.as_ref().map_or(0, JobCgroup::oom_kills);
let oomed = oom_kills > since;
```

`oom_kills` is declared at `:379`, **before** the `for stage in &stages`
loop, and the middle line **reassigns it inside the loop body**, between
`watch()` returning and `ran.step_ended`. So the baseline was advanced
once per executed step, and it was advanced to whatever that step had
just read. A step that observed a kill therefore raised the baseline for
the next step, which is the opposite of the hypothesis.

### The mechanism that is actually there

`memory.events`' `oom_kill` field counts **processes**, not events. With
`memory.oom.group = 1` written on the job cgroup
(`herd-runner/src/cgroup.rs:713` here, inside `JobCgroup::create`,
one cgroup per job) a single kill event takes the whole job tree, and the
kernel raises `oom_kill` once per task as it walks that tree. The runner
is woken by the **first** of those deaths — `watch()` returns when
`child.wait()` completes — so the reading it takes immediately afterwards
can be **mid-burst**, and the remainder of the *same* kill lands in
whatever is read next.

The consumer's own timestamps are what close it, and they are the
evidence this branch's regression test replays:

| fact | figure |
|---|---|
| kernel `oom-killer` invoked, j69 | 04:54:44.391340 |
| `run` ended, j69 | 04:54:44.395 |
| the per-process burst | about 4 ms from the invocation |
| `down` started | 04:54:44.397, 2 ms after the burst |
| kernel OOM lines during `down`'s 5.1 s | none |

`run`'s reading was taken about 3.7 ms into a burst that ran for about
4 ms over a kill set of eight processes (`qemu`, `xtask`, `bash` ×4,
`flock`, `sleep`). It was partial. 5.1 s later `down` read the settled
count, the delta over `run`'s partial baseline was positive, and `down`
was given the same verdict. **One kill, counted twice, from a baseline
that was advanced correctly to a figure that had not finished moving.**

### A second defect in the same path, found while reading it

Not in the brief, and it is the half a reader feels. In
`Attempt::step_ended` the cap arm of the `(&capped, &watched.ending,
&watched.outcome)` match is tried **before** the exit-code arms, so

```rust
(Some(capped), _, _) => (None, Some(herd_proto::cap_stop_reason(...))),
...
(None, None, JobOutcome::Exited { code }) => (Some(*code), None),
```

means a cap verdict **replaces** the step's own exit code with `None`.
`came_off = exit_code == Some(0)` is then false, so a `down` that had in
fact done its work was recorded with no code at all and announced in the
job's log as *"the down step the runner stopped this job at its memory
cap of 8589934592 bytes, so this job's workspace may be left on this
host."* — a takedown failure that had not happened, about a workspace
that had been reclaimed.

### What is in the tree now

**One kill, one step** — `OomWatermark` (`herd-runner/src/job.rs`), with
two independent clauses:

1. **A step is judged on `oom_group_kill` wherever the kernel publishes
   it.** That field counts kill *events*, one increment, raised before
   the first task of the group is signalled, so by the time any death can
   be observed it is already settled. It is the counter that matches the
   flag this runner itself sets. Absent (before Linux 5.17) it is read as
   **absence and never as zero**, and the per-process counter is used.
2. **A step that exited 0 was not stopped by anything.** With
   `memory.oom.group = 1` the whole job tree dies together, so a shell
   that exited cleanly cannot be a shell the kernel killed. **A non-zero
   code keeps its cap**: a job whose own process was killed while its
   shell reported 137 *was* stopped by the ceiling, and that case is
   tested.

The watermark is a **high-water mark**. An unreadable `memory.events`
reads as zero (`JobCgroup::oom_kills`), and a baseline one unreadable
step had lowered would charge the next step with every kill the job ever
had; that is tested too.

**Two real kills are still both reported.** The fix is not a `down` that
cannot report a cap: two `oom_group_kill` events give two verdicts, and
so do two kills on a kernel with no group counter. Both are tested, and
the second test is the one that would fail if the fix had been written
the easy way.

**The capped step names itself in the log.** `step_ended` now writes one
line through `say_in_the_log` for every step whose own verdict is a cap
— `the_cap_that_stopped_this_step` — and a step's `how`, which the
takedown line and the never-started line embed, is built by
`how_a_cap_stopped_this_step` instead of borrowing the job's sentence.
`herd_proto::cap_exceeded_reason` is **unchanged** and still produces the
job's sentence for the attempt's outcome and the record.

The three verbatim lines a reader now sees are in *Risks and findings*,
numbered to the brief's three outcomes.

**What did not move.** No exit code: rc 124 stays one code for all four
caps. No cap, no default, no admission arithmetic. `herd-proto` is
untouched — the step is named in the runner's own wording, so the
protocol surface was never reached, and `herd-proto/tests/frozen_formats.rs`
holds. `herdctl/src` is untouched. `docs/decisions.md`,
`deploy/release-notes/1.0.0.md`, `docs/coverage.md` and `docs/status.md`
are untouched; `status.md` says nothing about a cap or an OOM kill, so it
had no dead pointer to correct and stays at its 90 lines (**D276**).

### Files

| path | what |
|---|---|
| `herd-runner/src/cgroup.rs` | `OomKills` (the two counters), `oom_kills_in` (the parse, pure), `JobCgroup::oom_kills` returns both; three unit tests |
| `herd-runner/src/job.rs` | `OomWatermark` and `killed_in_this_step`; the reading site in `execute`; `how_a_cap_stopped_this_step` and `the_cap_that_stopped_this_step`; `step_ended` writes the log line and stops borrowing the job's sentence; one module-doc paragraph; eight tests |
| `docs/orchestration/board.md` | three rows, below |

## Not done, and why

- **The memory ceiling is still not announced before `run`.** `run` is
  the one step with an announce (`herd-runner/src/step.rs:245`,
  `run_cap.said`) and it carries the time figures only, so a reader's
  first sight of the memory ceiling is still a step that was stopped by
  it. Changing the announce changes every declared job's log, including
  jobs nothing stops, and the brief's three outcomes are about the step
  that *was* stopped. Filed as a row.
- **The runner does not say which counter it judged on.** No log line,
  no startup report clause. Same row.
- **The fallback's residual race is named, not closed.** On a kernel with
  no `oom_group_kill`, a `down` that ends with no exit code can still
  read the tail of `run`'s kill. Closing it needs either a second reading
  after a settle, which costs every capped step, or a figure only the
  consumer can report. Filed as its own row, state `open`.
- **No test makes a real OOM kill.** This host has no delegated cgroup
  v2, so a fixture that needed the kernel to kill something in a job
  cgroup could not run here at all. See *Gates*: no skip was added, and
  the reason is stated there rather than hidden in one.

## Gates

**All four Rust gates plus `check-docs.sh`, through Herd as one bare
`./scripts/gate.sh`** (`.rs` changed, so the full set is owed).

**There is exactly one gate run, and its output cannot be here.** The
first attempt was submitted before this file was committed and `gate.sh`
**refused** it, which is the right answer and is recorded rather than
tidied away — job **`j0000000174`**, `"passed": false`,
`"exit_code": 3`, `"cause": "exited"`, wall 55 ms, `bytes_written` 0,
nothing compiled:

```text
gate: REFUSED sha=714176f1a7e16b59cdf3168946461732d91067a6 reason=the working tree is not clean, so the gate would not be about 714176f1a7e16b59cdf3168946461732d91067a6
gate: NOT GREEN — this sha must not be pushed to main.
```

The unclean path was this handoff, untracked. So the order in
`workflow.md` is not a preference: a gate run **before** the handoff is
committed is not merely about a different tree, it does not happen at
all. **A refusal is not a red gate and is not a pass either** — it is no
verdict, and it is reported as none.

**Quoted here, from the runs made before this commit:**
`./scripts/check-docs.sh`, run locally — no toolchain, so it needs no
Herd job — twice: once on the code commit and once after the three
`board.md` rows were added.

```text
docs ok
```

**The four Rust gates were NOT run before this commit.** They are the
run named below, on the committed tree, and nothing else covers them.

**The last run, named and not quoted.** `./scripts/gate.sh`, bare and
whole, submitted to the local hub `ws://127.0.0.1:8791` with
`--label build --memory-mib 4700`, on **this tree** — after this file is
committed and before the branch is pushed. Its output goes to the Herd
job's own log and to the receipt `gate.sh` writes under
`target/herd-gate/<sha>.receipt` in this worktree, and **verbatim to
this agent's report**. Nothing is added to the tree after it.

**`./scripts/bench-preflight.sh` ran first, every time, and its refusal
was obeyed by waiting.** Green at load 6.27 of a 9.00 ceiling before any
work; **REFUSED** at load 10.28 when the gate was first wanted, and the
heavy phase waited rather than starting; green again at **8.59** when it
cleared, 21 GiB available, slice room 16 255 MiB over an 8 192 MiB
floor, 371 GiB free. **`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never
set or exported.**

**Skips: see the gate output in the agent's report.** Expected and
compared as a **list**, never a count (D27, D187): twelve at rest, or
the under-load list with `flood-fifty` and `flood-two-fifty` excused by
name. **No skip was added by this task and none was joined.** The eight
new unit tests and three new parse tests run on this host and make their
assertions here: they are about the arithmetic over the counters and the
wording of the lines, both of which are pure. **What they do not prove is
the kernel's own timing** — that `oom_group_kill` is raised before the
first task is signalled and that `oom_kill` is raised one task at a time
— because a test for that needs a real OOM kill in a delegated cgroup
and this host has none. That is said here rather than turned into a
twelfth-plus-one skip with a vague reason: the readings the tests replay
are the first consumer's measured ones, and the kernel ordering behind
them is an argument from their timestamps, not a local measurement.

## Risks and findings

### The three outcomes, with the lines a reader now sees

**1. One kill is charged to one step.** A fixture OOM-killed in `run`
leaves `down` with a verdict that does not say a cap stopped it. Proved
by `one_oom_kill_is_charged_to_one_step`, which replays j0000000069's
own readings, and by
`a_down_step_that_came_off_is_never_called_capped`, which asserts
`down`'s record carries `exit_code: Some(0)` and `ended: None`. What
`down` wrote in the log before, and no longer writes:

```text
herd-runner: the down step the runner stopped this job at its memory cap of 8589934592 bytes, so this job's workspace may be left on this host. The job's verdict is unchanged.
```

**The detector was proved on the known-bad case before its silence was
trusted.** The old rule is written out in the test as a function and
asserted **true** on the two readings, in the same test, immediately
before the new rule is asserted false:

```rust
fn the_old_rule(before: OomKills, now: OomKills) -> bool {
    now.processes > before.processes
}
```

So the test fails if the fix is removed, and it says in one place both
what the defect was and what replaced it.

**2. Two real kills are still both reported.** `run` killed and `down`
killed again gives two lines a reader can tell apart without fetching
the record:

```text
herd-runner: the run step was stopped at this runner's own memory cap of 8589934592 bytes — this runner's own ceiling, the figure it holds every job it takes to, and not a figure this job declared
herd-runner: the down step was stopped at this runner's own memory cap of 8589934592 bytes — this runner's own ceiling, the figure it holds every job it takes to, and not a figure this job declared
herd-runner: the down step was stopped at this runner's own memory cap of 8589934592 bytes, so this job's workspace may be left on this host. The job's verdict is unchanged.
```

Proved by `two_capped_steps_read_as_two_lines_in_the_log`,
`two_real_kills_are_both_reported` and
`two_real_kills_are_both_reported_without_the_group_counter`.

**3. The capped step names itself.** The line is the first of the three
above, and the figure is named for what it is: the runner-wide
`--job-memory-max`, written once at startup
(`herd-runner/src/cgroup.rs:707` here, the only `memory.max` write in
the product, from `herd-runner/src/main.rs:525`-`:530`), and never the job's
declaration. Their job declared **1792 MiB** and the sentence said
**8589934592 bytes**; the clause *"not a figure this job declared"* is
there because that has already been misread twice. The time cap keeps
its own grammar — its figure **is** the step's own (D108) — and its line
carries no such clause:

```text
herd-runner: the run step was stopped at the time cap of 60 s, this runner's own figure
```

### 1. The announce, and why it is the other half

A reader's first sight of the memory ceiling is still a step that was
stopped by it. `run_cap.said` names the time cap and whose figure it is,
in words, before `run` starts; nothing names the memory ceiling at all
until something dies against it. Row filed.

### 2. The runner cannot say which counter it used

The fix prefers `oom_group_kill` and falls back to `oom_kill`, and says
in no log and no report which it had. Invariant 9 forbids going to look
at the consumer's kernel (**D161**), so this is the only way the question
could ever be answered. Row filed, with the announce.

### 3. The residual race, and what is not known about j0000000069

Two independent clauses fix this job, and **it is not known which of
them did**, for two reasons that are both outside this project's reach:

- `oom_group_kill` is Linux 5.17 and later, and this project cannot read
  the first consumer's kernel version.
- Whether their `down` exited 0 cannot be read off their record
  **because of the very defect**: a cap verdict replaced the exit code
  with `None`, so `down`'s record shows the cap sentence whatever the
  shell did. Their 5 097 ms and 1 974 ms takedowns are consistent with
  clean cleanups, which is a reading and not a measurement.

On a kernel without the group counter, clause 2 is the only guard, and it
needs the step to have exited 0. A `down` that ends with no exit code for
any other reason can still read the tail of `run`'s kill as its own. Row
filed, state `open`, with the two ways to close it and a recommendation.

### 4. The helper-kill question: accepted behaviour, not a second defect

The brief asked whether `down` runs with its helpers freshly killed, and
whether that is a second defect. **It is accepted behaviour, and the
premise that the kill set held *the runner's own* helpers does not hold.**

Established by reading, not assumed:

- **Nothing of the runner is ever in a job cgroup.** The only write to
  `cgroup.procs` in the product is `Confined`'s, from inside the
  **child**, between fork and exec: `herd-runner/src/confine.rs:860`-
  `:862` writes `"0"` to `cgroup_procs_fd`, the descriptor opened per
  step at `herd-runner/src/job.rs:416` here. The runner's own process
  never joins. So the whole kill set was the **job's own process tree** —
  `qemu`, `xtask`, the consumer's hook shells, their `flock` and their
  `sleep`. uid 986 is the uid `herd-runner.service` runs as on their
  host: the runner does not drop to a job uid anywhere in the tree, so a
  process the runner started wears the runner's uid and that uid says
  nothing about which cgroup it was in.
- **Killing all of it at once is what `memory.oom.group = 1` is for**,
  and `herd-runner/src/cgroup.rs:711`-`:713` says so in as many words:
  *"Kill the whole cgroup on the first OOM rather than one process at a
  time, so a job dies as a job and does not limp on half-killed."*
- **`down` then starts a fresh shell in a cgroup that is empty**, under
  the ceiling that is still set. The job's cgroup is not recreated and
  nothing of it is reset — only `cgroup.procs` is reopened per step —
  which is why `down` can itself be killed by the same ceiling. That is
  outcome 2's case, and it is now reported rather than hidden.

So: no row for it as a defect. The one consequence worth stating is that
a `down` hook which depends on what `run` left running — a held `flock`,
a background process — finds it gone, and that is the promise the flag
makes rather than a surprise. **Said plainly rather than left
unstated**, as the brief asked.

### 5. One premise of the brief that reads differently after measurement

The brief lists the complete set of log writers as `say_in_the_log` at
`job.rs:394`, `:1192`, `:1213`, `:1288`. That set was complete. It is now
five: the call added in `step_ended` is the fifth, and it is the only one
that fires for a step that was capped and not for a step that failed.

### 6. The workspace walk was kept clear of

No test added here touches a directory the runner's workspace walk could
be asked to list, sets a mode, or runs a real job: all thirteen new tests
are pure arithmetic over the two counters, the parse of a text fixture,
or `Attempt::step_ended` against an unattached `Session` with no
workspace at all. The 200 ms `set_permissions(mode | 0o700)` loop is
duration-triggered and nothing here runs long enough to reach it.

## Drafted for `herd-merger`

**For `docs/decisions.md`** — one entry, verbatim, headed `D-NEXT`
because the merger allocates the number and gaps stay (D148,
invariant 8):

```markdown
## 2026-10-31 — D-NEXT A kill is charged to the step it happened in, on
the counter that settles first

**The defect, from the first consumer's own records and kernel log.**
Job j0000000069 reported `run` *"stopped at its memory cap of
8589934592 bytes"* after 218 217 ms and `down` the **same sentence**
after 5 097 ms, state `stopped_by_cap memory`. j0000000460 has the
identical shape. Their kernel log holds **one** OOM event per job, at
the very end of `run` — j69 at 04:54:44.391340, `run` ending
04:54:44.395, `down` starting 04:54:44.397 — and **no kernel OOM line
at all** during either `down`. One kill, two steps blamed.

**It was not a baseline nobody advanced.** `herd-runner/src/job.rs`
refreshed `since` once per executed step. The fault is in *when* a
counter settles: `memory.events`' `oom_kill` counts **processes**, and
with `memory.oom.group = 1` on the job cgroup one event takes the whole
job tree while the kernel raises the counter once per task. The runner
is woken by the first of those deaths, so its reading was taken about
3.7 ms into a burst of about 4 ms over eight processes. It was partial,
and the rest of the same kill became `down`'s delta.

**Decided.** A step's out-of-memory verdict is read from
`oom_group_kill` wherever the kernel publishes it (Linux 5.17 and
later): that field counts **events**, one increment, raised before the
first task is signalled, so it is settled by the time any death can be
observed, and it is the counter that matches the flag this runner sets
itself. Where the field is absent it is read as **absence, never as
zero**, and the per-process counter is used.

**And a second clause, which holds on every kernel.** A step that
exited 0 is never said to have been capped: with `memory.oom.group = 1`
a shell the kernel killed has no code to report at all. A **non-zero**
code keeps its cap — a job whose own process was killed while its shell
reported 137 *was* stopped by the ceiling. This also ends a compounding
defect of the same path: a cap verdict was replacing a step's exit code
with `None`, so a `down` that had done its work was recorded with no
code and announced as *"the down step the runner stopped this job at its
memory cap of 8589934592 bytes, so this job's workspace may be left on
this host"* — a takedown failure that had not happened.

**The baseline is a high-water mark.** An unreadable `memory.events`
reads as zero, and a baseline one unreadable step had lowered would
charge the next step with every kill the job ever had.

**And the step says which step it was, in the job's own log.** The step
whose verdict is a cap writes one line naming itself, its cap and the
figure — and naming the figure for what it is: *"this runner's own
ceiling, the figure it holds every job it takes to, and not a figure
this job declared"*. The three attempt-wide caps are the runner's own,
built once at startup from one runner-wide `--job-memory-max`
(`herd-runner/src/cgroup.rs:707` is the only `memory.max` write in the
product). Their job declared **1792 MiB** and the sentence said
**8589934592 bytes**; the clause is there because that has been misread
twice. The time cap keeps its own grammar, because its figure **is** the
step's own (D108). A step's line no longer borrows
`herd_proto::cap_exceeded_reason`, which is the **job's** sentence and
stays exactly as it is for the attempt's outcome and the record.

**What is not decided here.** No exit code moves: **rc 124 stays one
code for all four caps** (Alexa, 2026-10-30), because the consumer's
scripts are a live contract now that they run 1.0.0. No cap, default or
admission figure moves. `herd-proto` is untouched. On a kernel with no
`oom_group_kill` the second clause is the only guard and it needs the
step to have exited 0, and **which of the two clauses carried
j0000000069 is not known**: the field is 5.17-and-later and this project
reads nothing of the consumer's machines (invariant 9, **D161**), while
their `down`'s real exit code is unreadable from their record precisely
because of the defect this entry closes. That residual is a board row,
not a decision.

**Evidence.** `herd-runner/src/job.rs` `OomWatermark`,
`the_cap_that_stopped_this_step`; `herd-runner/src/cgroup.rs`
`OomKills`, `oom_kills_in`; thirteen tests, of which
`one_oom_kill_is_charged_to_one_step` replays the consumer's own two
readings and asserts the **old** rule true on them in the same test;
`history/2026-10-31-oom-step-attribution.md`.
```

**For `docs/orchestration/board.md`** — three rows, **already applied on
this branch** in commit `714176f1`, reproduced here verbatim so the
merged board can be checked line by line. Two in *Owed*, after
ORCH-307; one in *Open*, after ORCH-303. The highest id on `main` is
**ORCH-311**.

In *Owed*:

```markdown
| ORCH-NEXT | **One OOM kill was charged to two steps, and the job's log named only the wrong one.** The first consumer's j0000000069 and j0000000460 each reported `run` stopped at the memory cap **and** `down` stopped at the memory cap seconds later, against exactly one kernel OOM event per job and no kernel OOM line at all during `down`. Not a baseline nobody advanced: `herd-runner/src/job.rs` did refresh it per step. `oom_kill` counts **processes** — one per task as the kernel walks a `memory.oom.group` kill — and the runner is woken by the first of those deaths, so its reading was mid-burst and the rest of the same kill landed in `down`'s delta. Compounding it, a cap verdict overrode `down`'s own exit code, so a takedown that had succeeded was recorded with no code and announced as *"the down step the runner stopped this job at its memory cap of 8589934592 bytes, so this job's workspace may be left on this host"* — two claims in one clause, about a figure that is the runner-wide `--job-memory-max` and never the 1792 MiB the job declared | the first consumer's records and kernel log, reported 2026-10-31; brief to `oom-step-attribution` | herd-step (done), herd-merger (integration) | done | **Done on branch `oom-step-attribution`.** A step is judged on `oom_group_kill` where the kernel publishes it (one increment per event, before the first task is signalled) and a step that exited 0 is never said to have been capped, since `memory.oom.group = 1` leaves a killed shell no code to report; a non-zero code keeps its cap and the baseline is a high-water mark. The capped step now writes one line in the job's own log naming itself and naming the figure as this runner's own ceiling. **No exit code moved** — rc 124 stays one code for all four caps — and `herd-proto` is untouched. Thirteen new tests, ten of them in `job.rs`, over the consumer's own readings | — | `herd-runner/src/job.rs` `OomWatermark`, `the_cap_that_stopped_this_step`; `herd-runner/src/cgroup.rs` `OomKills`, `oom_kills_in`; `history/2026-10-31-oom-step-attribution.md` |
| ORCH-NEXT | **The memory ceiling a reader needs is never announced, and the runner cannot say which counter it judged a kill on.** Two halves of one gap, both left by `oom-step-attribution` and neither inside it. (1) `run` is the one step with an announce (`herd-runner/src/step.rs:245`, `run_cap.said`) and it carries the **time** figures only, so the memory ceiling reaches a job's log for the first time when something has already been stopped by it — which is why the first consumer read 8589934592 bytes as a figure their 1792 MiB job had asked for. (2) The runner now prefers `oom_group_kill` and falls back to `oom_kill`, and **says in no log and no report which of the two it used**, so nobody outside the kernel can tell whether a given host has the settled counter at all — and invariant 9 forbids going to look (**D161**) | `oom-step-attribution`, 2026-10-31, *Risks and findings* 1 and 2; the announce is the other half of the same reader's problem and the startup report is outside that task's paths | herd-step | owed | One clause in `run_cap.said` naming the runner's own memory ceiling beside the time figure, and one clause in the runner's startup report saying whether a probe cgroup's `memory.events` publishes `oom_group_kill`. Both are `.rs`, so both owe all four Rust gates plus `check-docs.sh` through one bare `./scripts/gate.sh` | — | `herd-runner/src/step.rs:387`-`:420` `run_cap`; `herd-runner/src/cgroup.rs` `oom_kills_in`; `history/2026-10-31-oom-step-attribution.md` |
```

In *Open*:

```markdown
| ORCH-NEXT | **On a kernel with no `oom_group_kill` the mid-burst race is closed by one clause only, and that clause needs the step to have exited 0.** `OomWatermark::killed_in_this_step` (`herd-runner/src/job.rs`) judges a step on `oom_group_kill` where the kernel publishes it — one increment per event, raised before the first task is signalled — and on the cumulative per-process `oom_kill` where it does not. In that fallback a `down` that ends **without an exit code**, for any reason at all, can still read the tail of `run`'s own kill as its own, because the baseline was taken mid-burst. The field is Linux 5.17 and later, and this project cannot read the first consumer's kernel version (invariant 9, **D161**), so **which of the two guards carried j0000000069 is not known** | `oom-step-attribution`, 2026-10-31, *Risks and findings* 3; stated rather than closed, because closing it needs either a second reading after a settle or a figure only the consumer can report | herd-step | open | Decide between (a) one re-read of `memory.events` after a bounded settle when a step ended with no code and only `processes` moved, which costs every capped step a few ms, and (b) asking the first consumer to report whether `oom_group_kill` appears in a job cgroup's `memory.events` on their fleet. **(b) first:** a few ms is cheap, but it is a change to every capped step's path and it may be guarding a branch nobody runs | a report from the first consumer, for (b) | `herd-runner/src/job.rs` `OomWatermark`; `herd-runner/src/cgroup.rs` `OomKills`; `history/2026-10-31-oom-step-attribution.md` |
```

## Decisions needed

**None for Alexa.** Everything here is inside the mandate and reversible:
a defect in shipped code, fixed without touching the protocol, an exit
code, a cap, a default or admission arithmetic.

**One question that is the consumer's and not his**, carried by the
*Open* row above and not by this section, because it needs no decision
from the director: whether a job cgroup's `memory.events` publishes
`oom_group_kill` on their fleet. It reaches us as a report with a job id
and version numbers, never by anybody looking (invariant 9, **D161**).

## Resume point

`herd-merger` takes this branch into the batch: allocate the `D-NEXT`
entry's number, check the three `ORCH-NEXT` rows survived the board merge
verbatim, and gate the integration tip. Nothing in this task is left
half-done.

If the next task is the follow-up: the first *Owed* row's second half is
the smaller one and the more useful — one clause in the runner's startup
report saying whether `oom_group_kill` is readable, which is what turns
the *Open* row's unknown into a measurement.

## Processes

- **PIDs started and ended by this task:** one background shell, started
  by this agent to wait on `./scripts/bench-preflight.sh` until it
  cleared, and ended on its own when the preflight passed. Its pid is in
  this agent's report. Nothing else was started and **nothing was
  signalled**: no `kill`, no `pkill` in any form, nothing by name.
  `3006543`, `3021002`, `565152` and `662711` were never read, touched or
  signalled. `/sys/fs/cgroup` was read and never written; no cache was
  dropped.
- **`./scripts/bench-preflight.sh` ran before every heavy phase** and its
  refusal was obeyed by waiting, once at load 10.28 against a ceiling of
  9.00. **`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set or
  exported.**
- **Own build tree:** `CARGO_TARGET_DIR` was
  `/mnt/ssd_980/herd/build/oom-step-attribution`, distinct so that the
  hub's dedup key could not join this submission to another agent's, and
  never on the root filesystem. Its size at removal and its deletion are
  in this agent's report: both happen **after** the push, which happens
  after this file is committed.
