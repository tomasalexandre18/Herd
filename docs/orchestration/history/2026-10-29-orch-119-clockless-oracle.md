# orch-119-clockless-oracle — handoff 2026-10-29

- **Item:** ORCH-119
- **Agent:** herd-step
- **Branch / commit:** `orch-119-clockless-oracle` / `78d4b76` (the fix) plus
  this handoff, based on `origin/main` `b73d38c`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## Done

### The mechanism is neither of the two readings in the brief

The brief offered a late redial and a job that ran short, and asked for the
measurement. It is **a third thing**, and it is not a margin on the job's
`sleep` at all: **the test cut the link before the runner was holding the
jobs.**

The fill loop waited for `JobState::Assigned | JobState::Running` and cut as
soon as it saw [`CEILING`] of them. `Assigned` is written when the **hub**
sends `FromHub::AssignJob`; `Running` is written only on
`ToHub::JobStarted`, which the runner sends from inside the lock that counts
its ceiling (`herd-hub/src/server.rs:4136`, `herd-runner/src/lib.rs:3340` —
`AssignJob` is handled by `tokio::spawn`, and the count is read inside the
spawned task on purpose). So the loop exited on a frame the hub had *sent*,
and the next statement cut the wire under it.

Measured, one run of the instrumented copy under load (wall-clock stamps per
line, hub and runner journals interleaved):

```
1793271394.919 herd-hub: j0000000001 is attempt 1 on runner capped-runner …
1793271394.962 herd-hub: j0000000002 is attempt 1 on runner capped-runner …
1793271394.963 PROBE cut 1793271394.963
1793271394.964 herd-runner: the link to the hub ended: … Connection reset …
```

**One millisecond** between the hub's second dispatch and the cut. Whether
the runner had read that frame is then a scheduling question, and on two
busy cores the answer is often no. Each job's own process stamped its start
into a file the test read afterwards; across 20 runs on two pinned CPUs
under eight spinners:

| What the runner was holding at the redial | Runs | Refusal | Assertion |
|---|---|---|---|
| both originals (both processes started before the cut) | 9 | `AtItsCeiling`, `holding` 2 | green |
| one original (one `AssignJob` lost in the cut) | 11 | `StillRunningThisJob`, `holding` 1 | green since D195 |
| neither | 0 here | nothing refused | **red at 1156** |

The 11 runs are unambiguous: the second job's first `date` stamp was written
**4.2–4.6 s after the cut** — that is to say after the link came back, as
its *second* attempt. Its first attempt never ran, because the assignment
went down with the link. `sleep 6` never ran short in any run, and the
redial was 0.02–0.37 s after `restore()` in all twenty.

So all three regimes are one quantity — **how many of its originals the
runner is still holding when the second attempts arrive** — and the thing
that moved it on CI was not the clock but a lost assignment.

Two further facts the instrumentation settled, both of which matter to any
future fix here:

1. **The product stops the originals itself, by design.** At the redial the
   hub answers a frame about a written-off attempt with
   `FromHub::StaleAttempt` and the runner **ends** that attempt (E-21,
   `herd-runner/src/lib.rs:2051`). Holding a job open does not hold it
   against that. It does not have to: the second attempt is dispatched on the
   hello, ahead of any answer the hub gives to a frame of the runner's, and a
   job told to stop is still a job the runner is holding until its processes
   are gone. One measured redial: dispatches at +8 ms and +15 ms after the
   join, the stale answer at +16 ms, the stop signalled at +17 ms, both
   refusals after that and both reading the count as 2.
2. The originals therefore never finish in this test, before or after the
   fix. The end stamps of a `sleep 6` never appeared in 30 s of waiting in
   any run: the jobs are stopped ~3 s into their sleep, every time.

### The fix: the premise is built, not timed

`herd-hub/tests/announced_job_ceiling.rs`, two changes and their comments:

- **The jobs are held open.** `a_job_that_holds_its_place` returns a release
  file and `until [ -e … ]; do sleep 0.05; done`, and
  `let_the_held_job_finish` writes the file once the records have been read.
  This is `admission.rs`'s shape and its argument, verbatim in spirit: a
  `sleep` makes the premise a duration, and every test that does it then has
  a race between how long the sleep is and how long its own steps take.
  The job prints nothing, which is one frame fewer about the written-off
  attempt at the redial.
- **The cut waits on the runner's own word**: `JobState::Running` for
  [`CEILING`] jobs, not `Assigned | Running`. The same correction is made in
  the sibling `no_job_is_lost_to_an_outage`, whose assertion message already
  said *"only n of 2 jobs reached the runner"* — which the old condition did
  not establish.

Nothing else moved: `CEILING` is 2, `THE_OUTAGE` is 4 s,
`SLEEP_ACROSS_THE_OUTAGE` is 6 s and is now used by the sibling test alone
(its oracle — no job dead, no job without a verdict — reads the same
whichever refusal the redial produces), every assertion about the record is
as it was, and D195's assertion of the figure against the record's own
sentence is untouched and still sharp (see the forced regimes below).

**The test also got faster**: under the loaded condition below, one run of
the bound test went from about 10 s to about 5 s, and the whole file is
11.3 s.

### Red before, green after, same condition

The condition is the brief's own named one: `SLEEP_ACROSS_THE_OUTAGE` = 4 s
on an idle host, which is regime 3 — the runner holding neither original
when the second attempts arrive. Both binaries built from this branch; the
"before" one is `origin/main`'s file byte for byte with that single constant
changed, so its line numbers are CI's.

| Tree | Condition | Runs | Green | Red |
|---|---|---|---|---|
| `origin/main`'s test | idle, the constant at 4 | 10 | 0 | **10 at `:1156`** |
| this branch | idle, the same constant at 4 | 10 | **10** | 0 |

The 10 reds are the CI failure verbatim, including its figures:

```
panicked at herd-hub/tests/probe_old.rs:1156:5:
with a budget of one and an outage that produces a refusal, a job has to
reach the bound; what the 2 jobs ended as: [Object {"code": Number(0),
"type": String("exited")}, Object {"code": Number(0), "type": String("exited")}]
```

And the fix is green **because the construction holds under the load that
used to break it**, not because the regime went away. Under the exact
condition that gave the old test the formerly-red arm in 11 of 20 runs:

| Tree | Condition | Runs | Green | Regime reached |
|---|---|---|---|---|
| `origin/main`'s test | `taskset -c 0,1` + 8 spinners | 20 | 20 | ceiling 9, one-original 11 |
| `origin/main`'s test | `taskset -c 0,1` + 16 spinners | 15 | 15 | ceiling 5, one-original 10 |
| `origin/main`'s test | `taskset -c 0` + 2 spinners | 10 | 10 | ceiling 7, one-original 3 |
| **this branch** | `taskset -c 0,1` + 8 spinners | **20** | **20** | **ceiling 20, `holding` 2 in all 20** |

Regime 3 was never reached by load on this host — 0 of 45 loaded runs here,
as 0 of 40 in the previous agent's — which is why it had to be driven on
purpose. Contention moves the old test from "both held" to "one held"; CI
went one step further out on the same quantity.

### All three regimes, driven on purpose, against the fixed oracle

A scratch copy of the fixed file with two extra tests, each letting one or
both held jobs go **while the link is still down**, so that the runner is
holding exactly what the regime says. Neither is committed; the patcher is
`…/scratchpad/patch-regimes.pl`.

| Regime driven | How | Runs | Result |
|---|---|---|---|
| 1 — both originals held | the committed test | 20 under load + 10 idle | green, `holding` 2, the ceiling sentence |
| 2 — one original held | release one hold during the outage | 5 | **green**, `holding` 1 in 2 of 5 and 2 in 3 of 5 — D195's branch, exercised and checked |
| 3 — no original held | release both holds during the outage | 5 | **red, 5 of 5, at the 1156 assertion**, with CI's message |

Regime 3 red is the right answer: the assertion is meant to catch "nothing
was refused", and the fix removes the regime rather than teaching the oracle
to tolerate it. Regime 2 green is the answer that matters for D195: the
figure is still read against the record's own sentence, and the sentence is
still checked, in the arm where `holding` is 1.

## Not done, and why

- **No product change.** The hub and the runner did exactly what D181, D182
  and E-21 say in every one of the ~90 runs. What was wrong is what the test
  waited for. One finding about the product is below, and it is the same
  finding the previous agent left: it is wording, and it is ORCH-126's
  neighbourhood, not this task's.
- **`THE_OUTAGE`, `SLEEP_ACROSS_THE_OUTAGE` and `CEILING` left alone.** The
  fallback the brief allowed — widening a timeout — was not taken and is
  not
  needed. Nothing in the bound test reads a duration for its premise any
  more.
- **No `D-NEXT`** proposed by me; the brief asked for the board row, which is
  below. The two decisions the previous handoff raised are still open and are
  untouched here.
- `docs/decisions.md`, `docs/orchestration/board.md`, `docs/status.md`,
  `.github/`, `ci.yml` untouched, as the brief requires.

## Gates

Run **before** this commit, on `78d4b76` (the fix alone), all with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-119b`:

```
nix develop --command cargo fmt --all --check      →  (no output, exit 0)
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.02s
nix develop --command cargo test -p herd-hub --test announced_job_ceiling
    test result: ok. 8 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;
    finished in 11.39s
./scripts/check-orchestration.sh   →  check-orchestration: 269 passed, 0 failed
./scripts/check-docs.sh            →  docs ok
```

`check-orchestration` is **269 passed, 0 failed** both before the branch
existed and after the fix — measured in this worktree at `b73d38c` before
the first edit, as the brief asked, and not taken from the brief.

The **last run**, on the tree with this file committed, is the whole gate:

```
./scripts/gate.sh --out /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/gate-final.log
```

Its verdict line is in the agent's report, verbatim, with the receipt under
this worktree's `target/herd-gate/`; it is not in this tree, because a
handoff cannot carry the output of the run that gated it.

**Skips (D27):** the at-rest family of `scripts/expected-skips/local.txt` —
the cgroup-delegation skips and the io-ceiling skips — each printed as
`HERD-SKIPPED` and **none counted as a pass**. The two load-conditional
flood benches (`flood-fifty`, `flood-two-fifty`) decline when the host's
one-minute load is over their ceiling; under D196 the verdict names them and
their reason if they do. The suite's `AT LEAST n` line is D179's floor and is
not a skip count. The exact list of the last run is in the agent's report.

## Risks and findings

1. **The residual race, named.** The oracle no longer reads a clock, but one
   ordering remains inside the redial: the second attempt must reach
   `take_job` before the stopped originals' processes are actually gone. It
   is structurally favourable — a `tokio::spawn` against a round trip plus a
   SIGTERM and a `/proc` sweep — and it was never lost in the 35
   measured runs of the fixed test, 20 of them on two pinned CPUs under
   eight spinners. It is not a duration anybody chose, and no test figure
   moves it. If it ever bites, the shape that would remove it is a
   hand-written runner against the real hub (`Puppet` in
   `herd-hub/tests/link.rs` already does this, and can
   send `AttemptAbandoned { at_its_ceiling }` directly) — which would prove
   the hub's half of D182 with no runner process at all, at the cost of no
   longer proving that a real runner produces the refusal.
2. **The fill loop now waits for more.** `Running` arrives after the runner
   has claimed the job, spawned the process and reported it; `PATIENCE` is
   20 s for two jobs, which was never approached here: from the hub's first
   assignment to the cut was 0.05–0.10 s over the 20 runs on two pinned
   CPUs under eight spinners. On a CI box running the whole suite in
   parallel it is still 20 s for two `/bin/sh` starts.
3. **The same defect may be elsewhere in the suite.** Any test that cuts a
   link, kills a hub or asserts occupancy after waiting on `JobState::
   Assigned` is making the hub's bookkeeping stand for the runner's. I did
   not survey the other 30 test files; `grep -rn "JobState::Assigned"
   herd-hub/tests/` is the survey, and it is worth a board row rather than a
   quiet assumption.
4. **The product reporting defect from the previous handoff is unchanged and
   confirmed on this branch.** A `StillRunningThisJob` refusal is recorded as
   `refused_as_full` and the hub's clause then says the machine was *"already
   as full as the ceiling they announced"* and advises *"Add capacity"* about
   a machine holding 1 of 2. I saw it in 24 of the ~45 unfixed runs. Nothing
   is lost by it; it is wording and attribution, it is ORCH-126's family, and
   it is not mine to change.
5. **Nothing in CI was re-run**, no push to `main`, no PR, no tag.

## Decisions needed

None of mine. Two are still open from
`2026-10-28-orch-119-announced-ceiling.md`
(the `as full as the ceiling they announced` wording, and whether
`ceiling_refusals()` should count both arms); finding 3 above suggests a
third, which is a board row rather than a decision.

Proposed board text for ORCH-119, for whoever holds the board:

> **ORCH-119 — `announced_job_ceiling.rs:1156` red on CI (run `40857453150`,
> `b73d38c`). Diagnosed and fixed (test); the clock is out of the oracle.**
> Not a regression of D195: 1179 no longer fires, and 1156 is the next
> assertion on the same quantity. That quantity is **how many of its
> originals the runner still holds when the second attempts arrive**, and
> what moved it was not the clock: the test cut the link one millisecond
> after the hub's second dispatch, measured, so an assignment the runner had
> not yet read went down with the link and left the machine holding one
> original or none. Fixed on `orch-119-clockless-oracle` by building the
> premise — the jobs are held open on a release file as in `admission.rs`,
> and the link is cut on `JobState::Running` (the runner's word) instead of
> `Assigned` (the hub's). Red 10/10 before and green 10/10 after at the same
> condition; green 20/20 under the load that gave the old test the
> formerly-red arm 11/20, in regime 1 in all 20. Regimes 2 and 3 driven on
> purpose against the fixed oracle: green 5/5 with `holding` 1 in 2 of them,
> and red 5/5 at 1156 with CI's own message. **Carries two findings:** E-21
> stops the originals at the redial, so no test can hold a job against it
> (the residual ordering is a spawn against a round trip, never lost in 35
> runs); and any other test that waits on `JobState::Assigned` before cutting
> a link is making the same mistake —
> `grep -rn "JobState::Assigned" herd-hub/tests/` is the survey.

## Resume point

Nothing is pending on this branch. For the merger:
`git log --oneline origin/main..orch-119-clockless-oracle` is two commits,
one touching `herd-hub/tests/announced_job_ceiling.rs` and one this file. The
batch's own `./scripts/gate.sh` at integration is the gate that counts. The
open work is finding 3's survey and the two decisions above.

## Processes

Load generators started and reaped **by PID**, never by name:
`2213234`–`2213241` (8 spinners, the unfixed test under load);
`2248544`–`2248557`, `2248559`, `2248560` (16 spinners, unfixed);
`2261408` `2261409` (2 spinners, one core, unfixed);
`2278951`–`2278958` (8 spinners, the fixed test). All 34 confirmed gone with
`kill -0`. `SIGTERM` does not reach a `while :; do :; done` loop between
commands — `kill` left every one of them alive — so each was ended with
`kill -9 <pid>`; that is the note for the next agent who copies the spinner
recipe. No `pkill`, no `killall`, nothing signalled that this task did not
start.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/orch-119b`, off the root
filesystem, deleted once the branch is pushed, together with this worktree's
own `target/` (which holds only the gate's receipt and log).
