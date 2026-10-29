# orch-119-announced-ceiling — handoff 2026-10-28

- **Item:** ORCH-119
- **Agent:** herd-step
- **Branch / commit:** `orch-119-announced-ceiling` / `326b714` (the fix) plus
  this handoff, based on `origin/main` `09229db`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## Done

### What line 1179 asserted, and what the `1` was

`herd-hub/tests/announced_job_ceiling.rs:1179`, in
`a_job_refused_past_the_bound_ends_and_names_it`, was:

```rust
    assert_eq!(
        end["runners"][0]["holding"], CEILING,
        "with the figure that machine itself gave — the count it was holding, which is the \
         ceiling it announced, that being why it refused: {end}"
    );
```

`CEILING` is `2` (`(4096 - 2048) / 1024`, this file's declared runner).
`end` is `record["state"]["outcome"]` of the first job whose outcome is
`refused_as_full`. `holding` reaches it from the runner, unchanged:
`herd_runner::Session::take_job` → `NotTaken` → `abandon_at_its_ceiling` →
`ToHub::AttemptAbandoned { at_its_ceiling }` → `Attempt::at_its_ceiling` →
`JobOutcome::RefusedAsFull { runners: [RunnerThatWasFull { holding }] }`. It
is a settled count read inside the runner's own lock at the instant it
refused — **not** a count read early, and not a count of what the hub
thinks.

**Two refusals set that field, and only one of them is a ceiling**
(`herd-runner/src/lib.rs`, `take_job`):

```rust
            let running = inner.jobs.values().filter(|job| job.end.is_none()).count();
            if running >= most_at_once {
                return Err(NotTaken::AtItsCeiling { running, most: most_at_once });
            }
            …
            if let Some(held) = /* another attempt of *this* job, still running */ {
                return Err(NotTaken::StillRunningThisJob { held, running, most: most_at_once });
            }
```

`StillRunningThisJob` is reachable **only below the ceiling**: the
`AtItsCeiling` arm returns first whenever `running >= most_at_once`, so in
that second arm `running < CEILING` always — `1`, at a ceiling of 2. That
`1` is the `left` of the failed assertion. The line asserted a figure that
one of the two legitimate refusals cannot produce.

### Reproduced, with a rate at each condition

Single test, release-free debug binary from this branch, 20 s per run at
worst. Spinners are `bash -c 'while :; do :; done'` pinned to the same two
CPUs as the test; `taskset -c 0,1` is the whole of the 2-core simulation
(tokio reads `available_parallelism`, which honours the affinity mask).

| Condition | Runs | Failures | Where |
|---|---|---|---|
| unconstrained, 12 cores | 15 | 0 | — |
| `taskset -c 0,1`, no load | 20 | 0 | — |
| `taskset -c 0,1` + 4 spinners | 20 | **4** (20 %) | 1179, `holding` 1 |
| `taskset -c 0,1` + 8 spinners | 20 | **13** (65 %) | 1179, `holding` 1 |
| unconstrained, `SLEEP_ACROSS_THE_OUTAGE` 5 s | 12 | 0 | — |
| unconstrained, `SLEEP_ACROSS_THE_OUTAGE` 4 s | 10 | **10** (100 %) | **1156**, nothing refused at all |

Every one of the 17 failures at 1179 was byte-identical, and the `reason`
names the branch:

> `the runner is still running attempt 1 of this job and never started this
> one: one machine runs one job once, so the place this attempt would need is
> the place the job it is a second go at is already in. It is holding 1
> job(s); this job has been handed straight back 1 time(s) of the 1 allowed
> by machines that were already as full as the ceiling they announced, …
> the hub's count of what is out on capped-runner (holding 1) and theirs
> simply never agreed.`
>
> `left: Number(1)  right: 2`

A green run says `the runner is already running 2 job(s) and never started
this job: … this runner holds at most 2 job(s) at once` and `"holding": 2`.
So the two CI reds and the greens differ by **which arm of `take_job`
answered**, and by nothing else.

### Three regimes, one clock

The two jobs sleep `SLEEP_ACROSS_THE_OUTAGE` (6 s); their second attempts
arrive about `THE_OUTAGE` (4 s) plus a redial later — measured at 4.3–4.6 s
on an idle host, so the margin is ~1.5 s. What the runner is holding at that
instant decides everything:

1. **both originals running** → `AtItsCeiling`, `holding == CEILING`. Green.
2. **exactly one running** → `StillRunningThisJob`, `holding == 1`. Red at
   1179.
3. **neither running** → the attempt is simply accepted; nothing is refused.
   Red at 1156 (`refused.is_empty()`).

The 4 s row above is regime 3 on an idle 12-core host, which brackets the
clock from the other side and shows the host is incidental: the test sits on
a timing window, not on a core count. Contention moves the system from
regime 1 into regime 2 — it widens the stagger between the two originals'
own start and finish times, so the "exactly one has finished" window grows
from tens of milliseconds to seconds. (That is the inference the five rows
support; the stagger itself was not measured directly. Regime 3 was never
reached under load, which is why 1156 has never been red on CI.)

### The fix, and its proof under the condition that was red

The figure is now read against the record's **own sentence** rather than
against `CEILING`: a field beside a reason has to agree with it (D166), and
that holds in both arms. `THE_RUNNERS_CEILING_SENTENCE` is already this
file's attribution constant (`herd_runner::A_RUNNERS_OWN_CEILING`), which
the ceiling arm carries and the other does not.

| Condition | Runs | Failures | `holding` 1 runs |
|---|---|---|---|
| `taskset -c 0,1` + 8 spinners | 20 | **0** | 13 of 20 |
| `taskset -c 0,1` + 4 spinners | 20 | **0** | 3 of 20 |
| `taskset -c 0,1`, no load | 10 | 0 | 0 |
| unconstrained | 15 | 0 | 0 |

The proof is not that the branch stopped happening: under eight spinners it
happened in 13 of 20 runs — the same 65 % that was red — and the assertion
accepted it and checked it. The oracle is stronger than it was, not weaker:
it now asserts the figure against the sentence in both arms, and the
ceiling arm is still held to `== CEILING` exactly.

## Not done, and why

- **No product change.** What makes `main` red is the test's premise, and
  the hub's behaviour in both arms is what D182 approved: not `lost`, no go
  at a runner that went away spent, the machine named, the figures in
  fields. There **is** a separate reporting defect in the product (below);
  it changes an operator-facing record and three doc comments in
  `herd-proto`/`herd-hub`, which is a decision and not mine to take.
- **`SLEEP_ACROSS_THE_OUTAGE` left at 6 s.** Widening it would buy margin
  for regime 3 (assertion 1156), which has never been red and which
  contention does not push towards. It would lengthen two tests for a clock
  bet, which is what this file's own comments argue against.
- `docs/orchestration/board.md`, `docs/decisions.md`, `.github/` untouched,
  as the brief requires. Proposed text below.

## Gates

Run **before** this commit, on `326b714` (the fix alone):

```
step 2: check-orchestration.sh (ci job: docs) — ok (43s)
step 3: nix flake check (ci job: check) — ok (4s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (16s)
step 6: test suite — ok (365s)
step 7: the skips — FAILED
gate: RED sha=326b7144cbb6ff11b109fbab27f5836b41f21d1b failed=7:unexpected-skip skips=at-rest
```

That red is step 7 and nothing else: two skips not on the expected list,

```
    + flood-fifty
    + flood-two-fifty
HERD-SKIPPED: flood-fifty: the host's one-minute load is Some(10.67), over this bench's ceiling of 8; the host is shared with a production system
HERD-SKIPPED: flood-two-fifty: the host's one-minute load is Some(10.67), over this bench's ceiling of 8; the host is shared with a production system
```

— the two load-conditional flood benches, which read **10.67** mid-flood
while the gate's own two readings were 2.48 at the start and 5.93 after the
suite, so neither allowed the load-conditional list. That is gate.sh's own
design note 2 (two readings are not enough on a shared host), met by the
suite's own 365 s of 12-core work, and it is reachable on any tree: this
change touches one test file in `herd-hub/tests/` and cannot move a flood
bench's reading of `/proc/loadavg`. Also:

```
./scripts/check-docs.sh   →  docs ok
./scripts/check-orchestration.sh  →  check-orchestration: 256 passed, 0 failed
```

`check-orchestration` is **256 passed, 0 failed** before and after. The
brief expected 257: 256 is what `09229db` gives, measured in the
orchestrator's own checkout as well as in this worktree, so the expectation
was off by one before this branch existed and this change does not move it.

The **last run**, on the tree with this file committed, its output going to
the log named here and its receipt under `target/herd-gate/` of this
worktree:

```
./scripts/gate.sh --out /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch119/gate-final.log
```

Its verdict line is in the agent's report, verbatim, and is not in this
tree.

**Skips (D27):** the 12 at-rest skips of `scripts/expected-skips/local.txt`
— the cgroup-delegation family and the io-ceiling family — plus
`flood-fifty` and `flood-two-fifty` above, 14 lines in all. Every one is
printed as skipped; **none is counted as a pass**, and the count is not the
signal (the gate is red on the two names, not on the number). The suite's
`AT LEAST 14 … this list is a floor and not a count` line is D179 on an
unfinished run and is not a skip count.

## Risks and findings

1. **A product reporting defect, for ORCH-126's neighbourhood.** A
   `StillRunningThisJob` refusal is recorded as `JobOutcome::RefusedAsFull`
   and the hub's own clause then says the job was handed back *"by machines
   that were already as full as the ceiling they announced"* and advises
   *"Add capacity"*. The machine was holding **1 of 2**. The record
   contradicts itself in one sentence — `It is holding 1 job(s)` beside *as
   full as the ceiling they announced* — and the advice is wrong: the thing
   to act on is that the hub sent a second attempt of a job the machine was
   still running, which is exactly ORCH-126's family (the hub's view of a
   machine and the machine's own disagree, and nobody is told which).
   Nothing is lost by it: the ending is right, the budget charged is right.
   It is wording and attribution, and an operator would see it.
2. **Three doc comments state a claim the code does not keep.** All three
   say the figure *"is the ceiling it announced, that being why it
   refused"*: `herd_hub::job::HowItEnded::TheRunnerWasAtItsCeiling::holding`,
   `herd_hub::job::Attempt::at_its_ceiling`, and
   `herd_proto::RunnerThatWasFull::holding`. False in the
   `StillRunningThisJob` arm, which sets the same field with a figure that
   is by construction below the ceiling. The assertion at 1179 was that
   claim written as a test; the claim is what should change, not the test
   back again.
3. **The file's own oracle cannot see that arm.** `ceiling_refusals()` (line
   ~165) filters on `THE_RUNNERS_CEILING_SENTENCE`
   (`herd_runner::A_RUNNERS_OWN_CEILING`, `"this runner holds at most"`),
   which `StillRunningThisJob`'s sentence does not contain. So a refusal the
   hub charges to the ceiling budget is invisible to the attribution this
   file performs. No test is red for it today — the other tests in the file
   provoke the ceiling arm — but "attempts spent for the runner's ceiling =
   0" is being proved over a subset of the refusals that spend that budget.
4. **Assertion 1156 is on the same clock**, one regime further out: 10 of 10
   red at `SLEEP_ACROSS_THE_OUTAGE` 4 s on an idle host. Contention does not
   push towards it (0 of 40 under load), so it is not what CI hit, but the
   margin is ~1.5 s of wall clock and it is a margin, not a mechanism.
5. **The gate's red is reproducible on any tree** and is a host-measurement
   interaction, not this branch's: see *Gates*.

## Decisions needed

1. **Does a refusal for "one machine runs one job once" keep saying `as full
   as the ceiling they announced`?** — *Recommendation:* no. Give that arm
   its own clause in `end_attempt`'s `RefusedAsFull` sentence (the runner's
   own sentence already travels verbatim ahead of it and is correct), drop
   *"Add capacity"* for it, and correct the three doc comments in finding 2
   to say "the count the machine gave, which is its ceiling when the ceiling
   is why it refused". Keep `JobOutcome::RefusedAsFull`, the budget and
   `RECORD_VERSION` 1 exactly as they are — nothing in the shape changes, so
   no version moves. *Fallback:* leave the wording and record findings 1–3
   on the board under ORCH-126, which is about the same disagreement.
2. **Should `ceiling_refusals()` count both arms (finding 3)?** —
   *Recommendation:* yes, matched on a second constant taken from
   `herd_runner` the way the first one is, never a literal. *Fallback:* a
   board row; it weakens an oracle, it does not make one wrong.

Proposed board text for ORCH-119, for whoever holds the board:

> **ORCH-119 — `announced_job_ceiling.rs:1179` red on CI, three sightings.
> Diagnosed and fixed (test).** Sighting 3: run `40762764493`, headSha
> `09229db`, same assertion, `holding` 1 against `CEILING` 2. Cause: the
> assertion pinned `NotTaken::AtItsCeiling`, while the outage also produces
> `NotTaken::StillRunningThisJob`, which by construction reports
> `holding < CEILING`. Reproduced on two pinned CPUs under load — 4/20 and
> 13/20 — green 35/35 unconstrained; fixed on
> `orch-119-announced-ceiling`, 0/20 red under the 13/20 condition with 13
> of those 20 taking the formerly-red arm. **Carries three findings for
> ORCH-126**: the record says "as full as the ceiling they announced" of a
> machine holding 1 of 2 and advises "Add capacity"; three doc comments
> state a claim the code does not keep; `ceiling_refusals()` cannot see the
> second arm.

Proposed `D-NEXT`, if the orchestrator wants decision 1 recorded (the
merger allocates the number):

> **D-NEXT (2026-10-28) — a refusal about a runner's own count is not always
> a runner at its ceiling.** `at_its_ceiling` carries the count a machine
> gave when it handed an assignment straight back for a count of its own.
> Two refusals do that and they are different facts: the machine is at the
> ceiling it announced, or the machine is still running another attempt of
> that very job (D182, one machine runs one job once) — which is reachable
> only *below* the ceiling. Both keep the exemption and the bounded budget
> D182 gave them, and `JobOutcome::RefusedAsFull`, `RECORD_VERSION` and
> `PROTOCOL_VERSION` are unchanged. What changes is what is said: the
> record no longer calls the second one "as full as the ceiling it
> announced", no longer advises adding capacity for it, and no test asserts
> that the figure equals the ceiling without first reading which refusal it
> was (ORCH-119; three CI reds on one assertion, reproduced at 13 failures
> in 20 on two pinned CPUs under load and 0 in 35 unconstrained).

## Resume point

Nothing is pending on this branch. For the merger:
`git log --oneline origin/main..orch-119-announced-ceiling` is two commits,
one touching `herd-hub/tests/announced_job_ceiling.rs` and one this file.
The batch's own `./scripts/gate.sh` at integration is the gate that counts;
if it is red on `flood-fifty`/`flood-two-fifty` only, that is the host's
load and not this branch (*Gates*). The open work is the two decisions
above.

## Processes

Load generators started and reaped **by PID**, never by name: `2856022`
`2856023` `2856024` `2856025` (4 spinners, red run); `2865964`–`2865971`
(8 spinners, red run); `2961513`–`2961520` (8 spinners, green run);
`2972731`–`2972734` (4 spinners, green run). All 24 confirmed gone with
`kill -0` after their loop. No other process was signalled, no `pkill`, no
`killall`.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/orch-119`, off the root
filesystem; it is deleted once the branch is pushed, with this worktree's
own `target/` (which holds only the gate receipts and logs quoted above).
