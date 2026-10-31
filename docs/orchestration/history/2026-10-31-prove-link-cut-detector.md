# prove-link-cut-detector — handoff 2026-10-31

- **Item:** ORCH-321
- **Agent:** herd-step
- **Branch / commit:** `prove-link-cut-detector` / this commit, based on
  `origin/main` `3ea839c977827bc71391a33f56881df19383a3b5`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A control, not a change.** The only committed files are this handoff
and one `board.md` row. No product source is committed, and none is
changed in the tree that was pushed.

**The detector of
`a_link_cut_while_the_runner_is_full_loses_no_job` fires.** It is
capable of going red, and the red is about the property the test is
named for.

### The fixture, and why it is the one the assertion depends on

One line, `herd-hub/src/server.rs:8509`, inside
`give_the_job_another_go`:

```rust
-    let at_its_ceiling = matches!(how, HowItEnded::TheRunnerWasAtItsCeiling { .. });
+    let at_its_ceiling = false;
```

That binding is the whole of D182's separation of budgets at the point
where a go is spent. With it forced `false`, the next three lines

```rust
    let (spent, allowed) = if at_its_ceiling {
        (ceilings_hit + 1, state.policy.ceiling_refusals_allowed)
    } else {
        (goes_taken + 1, state.policy.attempts_allowed)
    };
```

charge the runner's ceiling refusal to E-21's budget —
`ATTEMPTS_ALLOWED = 2` (`server.rs:198`) — instead of its own —
`CEILING_REFUSALS_ALLOWED = 8` (`:225`). The job has already spent one
go on the watchdog's write-off, so the refusal is its second and last,
and the job ends `lost` with no verdict. That is the pre-D182 defect
the test exists for, reproduced exactly.

### The red, verbatim

Run on the mutated tree, in the worktree, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/link-cut-clock-figure`:

```text
thread 'a_link_cut_while_the_runner_is_full_loses_no_job' (4179881)
panicked at herd-hub/tests/announced_job_ceiling.rs:1374:5:
0 job dead and 0 job without a verdict is the condition this change
merges under, and 2 of 2 came back without one:
["j0000000001: {\"reason\":\"the runner is already running 2 job(s) and
never started this job: the jobs it is holding already add up to 2 GiB
of the 2 GiB it admits, and this job would be held to 1 GiB more, which
does not fit in what is left. this machine lends 4 GiB and keeps 2 GiB
as its own floor, and caps every job at 1 GiB, so this runner holds at
most 2 job(s) at once; this was attempt 2 of 2 at a runner that went
away, so the job is not attempted again (E-21). A go spent on a
capability that had gone under it would not have counted towards these
(D76), nor would one spent on a machine that was already full
(D182)\",\"type\":\"lost\"} — every attempt_ended: [\"runner
capped-runner stopped answering: nothing about attempt 1 of this job
for 3.108865981s, so it is not coming back\", \"the runner is already
running 2 job(s) and never started this job: ... so this runner holds
at most 2 job(s) at once\"]", "j0000000002: ... same, with
3.096935247s ..."]. Each of these was a runnable job on a healthy
machine. Read the endings above before blaming the product: a refusal
carrying the runner's own ceiling sentence that spent the job's last go
is this test's defect; an ending written by the hub's watchdog is the
hub deciding a silence, and a silence long enough to decide is this
host
```

(The two records are identical but for the measured silence, so the
second is elided; the backtrace frames are
`announced_job_ceiling.rs:1374` → `:1111` → `:1102`.)

```text
test a_link_cut_while_the_runner_is_full_loses_no_job ... FAILED

failures:
    a_link_cut_while_the_runner_is_full_loses_no_job

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured;
9 filtered out; finished in 3.49s
```

### Red for the named property, not a collapse

Not a compile error, not a timeout, not a panic in setup. Every
premise of the sequence printed before the assertion fired:

```text
cutting the link, with the runner at its ceiling of 2
the hub wrote off all 2 attempts 3.052280715s after the cut, at a
  threshold of 3s
the link is back; the runner is still holding its 2 original job(s)
every job was handed back to the machine still holding it, and it
  refused
```

So the cut landed on a full machine, the hub's own watchdog wrote off
every attempt, the machine was still holding the originals when the
hub handed them back, and it refused for the ceiling it announced. The
failure is the *cost* of that refusal — a job lost across a link cut —
which is what the test is named for and what its own message says to
read the endings for. Each job's two `attempt_ended` reasons tell the
two candidate causes apart exactly as the message asks: the first is
the watchdog's silence, the second is the runner's own ceiling
sentence, and it is the second that spent the last go.

### Restored, and shown restored

```text
$ git checkout -- herd-hub/src/server.rs
$ git diff --stat -- herd-hub/src herd-runner/src herd-proto/src \
    herdctl/src
(no output)
$ git status --porcelain
(no output, before the two docs files of this commit)
$ sed -n '8509p' herd-hub/src/server.rs
    let at_its_ceiling = matches!(how, HowItEnded::TheRunnerWasAtItsCeiling { .. });
```

### Green again, unmutated, same host and same target dir

```text
   Compiling herd-hub v1.0.0 (.../wt/prove-link-cut-detector/herd-hub)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.68s
     Running tests/announced_job_ceiling.rs

running 1 test
cutting the link, with the runner at its ceiling of 2
the hub wrote off all 2 attempts 3.052280715s after the cut, at a
  threshold of 3s
the link is back; the runner is still holding its 2 original job(s)
every job was handed back to the machine still holding it, and it
  refused
every one of the 2 jobs came back with its own verdict
test a_link_cut_while_the_runner_is_full_loses_no_job ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;
9 filtered out; finished in 3.63s
```

The red and the green are the same command, the same tree but for one
line, the same target dir, minutes apart on the same host. The red is
attributable to the fixture.

### Load beside every verdict

This test is a member of the wall-clock family with ORCH-244,
ORCH-321 and ORCH-324, so `load1` is part of the result:

| phase | `bench-preflight.sh` verdict | load1 | result |
|---|---|---|---|
| before the red | `ok, safe to start.` | 0.52 | — |
| the red | — | 0.39 → 0.86 | FAILED in 3.49 s |
| before the green | `ok, safe to start.` | 0.74 | — |
| the green | — | 0.70 → 0.73 | ok in 3.63 s |

Both verdicts were taken **well below 8**, so neither figure is in the
band where this family's results stop meaning anything. Nothing else
heavy was run beside them; the preflight's verdict line was read from
one invocation each time and neither
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` nor `HERD_GATE_LOAD_MAX` was ever
set.

## Not done, and why

- **The full `gate.sh` the ORCH-321 row also owed.** Not a step's to
  make under *One landing per batch*: it is the batch's own landing
  gate, `herd-merger`'s. The row is closed on the control alone and
  says so, and the construction under test landed green in batch 35
  (`3ea839c`).
- **The 60 s twin,
  `..._at_the_deployed_sixty_seconds`.** `#[ignore]`, and it proves
  nothing this control does not: both drive the same watchdog, the
  same refusal and the same oracle. Not run.
- **No second mutation was tried.** Point 4 of the brief: a fixture
  chosen because it works is not a control. The one the brief named
  reddened the test on the first run, so there was nothing to hunt.

## Gates

Prose only. The four Rust gates were **NOT RUN** locally — covered by
the landing's own `./scripts/gate.sh`, which is `herd-merger`'s at
integration and not this task's. The two `cargo test` runs quoted above
are **controls**, named as such (`CLAUDE.md`, *Commands*): a single
test by name, never the suite and never `gate.sh`.

The last run on the committed tree is `./scripts/check-docs.sh`, run
after this file was written; its verbatim output is in this agent's
report, not in the tree it gates.

No skip. No `HERD-SKIPPED` line was produced or copied.

## Risks and findings

1. **The brief's earlier candidate was not an uncovered line, only a
   line covered elsewhere.** The orchestrator's first fixture,
   deleting `&& attempt.at_its_ceiling.is_none()` from
   `herd-hub/src/job.rs:1875`, would indeed not redden this
   integration test — the orchestrator's own correction is right, and
   for the reason given: `give_the_job_another_go` picks the budget
   from `how` at `server.rs:8509`, so `goes_a_runner_took()` is never
   consulted on the ceiling path. But that clause is **not
   undetected**: the unit test
   `a_ceiling_refusal_spends_its_own_budget_and_no_other`
   (`herd-hub/src/job.rs:3027`) builds a record with two
   ceiling-refused attempts and asserts `goes_a_runner_took() == 1`,
   which deleting the clause turns into 3. So D182's three budgets are
   each held by a test; they are just held in two different places,
   and the integration test is not one of them. Nothing to act on —
   recorded so the next reader does not conclude from this control
   that `job.rs:1875` is unguarded.
2. **The red depended on `ATTEMPTS_ALLOWED` being 2 and already
   spent.** The job's first go goes to the watchdog's write-off, which
   is E-21 working, and the forced-false refusal is then the second of
   two. Were `attempts_allowed` ever raised, this same fixture would
   requeue rather than lose, and the test would go green over a hub
   with D182 deleted. That is a property of the fixture, not of the
   test: nothing in the product changes, and the control stands as the
   evidence for the code as it is at `3ea839c`.
3. **Nothing in the brief was contradicted.** The constants are where
   the brief said, as named constants with doc comments
   (`LOST_AFTER_HERE` 3 s and `HEALTH_EVERY_HERE` 1 s,
   `announced_job_ceiling.rs:855`-`856`); the test is at `:1102`, not
   `:916`, found by name as instructed, and the ORCH-321 row's own
   evidence cell still says `:1079`, which is stale by line drift
   alone and left as it is.
4. **The preserved build tree can go**, and this task did not delete
   it. See *Processes*.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** This task decided nothing: it
produced evidence for a decision already taken (D182) and a row
already written.

**One `board.md` row, already edited in place on this branch** rather
than drafted, because ORCH-321 exists and this task only closes it:
`docs/orchestration/board.md` line 322, State `open` → `done`, with
the red quoted in the *Next action* cell and three evidence references
added. It is the only non-handoff file this branch touches. No new row
is proposed, so no `ORCH-NEXT`.

## Decisions needed

None.

## Resume point

Nothing is owed on ORCH-321. For whoever integrates this branch:
`herd-merger` takes `prove-link-cut-detector` into the batch, and the
batch's own `./scripts/gate.sh` for the gated sha is the Rust evidence
this handoff does not carry.

The build tree may be removed by whoever next needs the space:
`rm -rf /mnt/ssd_980/herd/build/link-cut-clock-figure` (1.9 GiB). It
was preserved for exactly this control and the control is complete.

## Processes

No PID was started beyond the two `nix develop --command cargo test`
foreground runs, each of which exited on its own; nothing was
signalled, and no process of another agent or of the consumer was
touched.

**Own `target/` not deleted:** the task built into
`/mnt/ssd_980/herd/build/link-cut-clock-figure`, the tree the brief
named and told this task not to delete without confirming the control
was complete. The control **is** complete, so the tree can now go; it
is left in place because the brief reserved the decision, and the one
command is in *Resume point*. Nothing was written to `/`.
