# c5-no-life-spent — handoff 2026-10-25

- **Item:** ORCH-060 (and ORCH-061, opened here)
- **Agent:** herd-step
- **Branch / commit:** `runner-holds-its-own-limits` / `17014c2`, based on
  `origin/main` `199e4df`, continuing `fa2ee15`
- **PR:** #82

## Done

The blocking finding of PR #82's contradictory review (§1.2) is closed, on
Alexa's decision of 2026-10-25, and the condition of merge she attached to
it is met.

**The test first, and red before the fix.**
`herd-hub/tests/announced_job_ceiling.rs`,
`a_link_cut_while_the_runner_is_full_loses_no_job`. A real hub, a real
runner announcing a ceiling of 2 and running two jobs, and a TCP relay
between them the test can cut: the sockets are shut down under both peers
and every redial is dropped for the length of the outage, so both processes
stay alive and only the wire goes. The oracle is Alexa's and nothing else —
**no job dead, no job without a verdict**. It was run against the branch as
it stood, before a line of the fix, at the suite timings and again at the
real 60 s; both red, both for the stated reason. A second test,
`a_job_refused_past_the_bound_ends_and_names_it`, drives the new budget at a
figure of one and asserts what a job that reaches the bound is told.

**Then the fix, five parts.**

1. `at_its_ceiling: Option<u32>` on `ToHub::AttemptAbandoned`,
   `#[serde(default, skip_serializing_if = "Option::is_none")]`: how many
   jobs the runner was holding, when **its own count of what it holds** is
   why it refused. `None` on every other abandonment, which is what every
   abandonment meant before the field existed.
2. `at_its_ceiling: bool` on `herd_hub::job::Attempt`, `#[serde(default)]`
   and left off the record entirely when false.
   `JobRecord::goes_a_runner_took` filters on it beside D76's
   `requeue_code`, so such a refusal spends none of E-21's two goes.
   `end_attempt`'s fourth parameter is now one `HowItEnded` enum rather than
   an `Option<i32>`, so the three budgets cannot be confused at a call site.
3. A bound of its own: `CEILING_REFUSALS_ALLOWED` = **8**, a `Policy`
   figure. A job handed back an eighth time ends `Lost`, and the sentence
   names the budget, both figures, that none of E-21's goes were spent, and
   every machine that said it was full.
4. The hub remembers the machine's own count (`ConnectedRunner` /
   `Seat::it_said_it_is_holding`) until that runner's next health report,
   and `admission` holds the ceiling against the larger of that and
   `seat.reserved.jobs`. The health report ends the correction **and judges
   the queue at once** — without that the job waited for the restatement
   cadence, which measured as a 60 s stall.
5. A runner never holds two attempts of one job: `take_job` refuses a second
   attempt of a job it is already running (`NotTaken::StillRunningThisJob`,
   handed back as a refusal about the runner's own count, so it spends no go
   either), `abandon` drops only the entry belonging to the attempt it is
   about, and `release` drops only a job that is holding an end.

**Documents.** A third `D-NEXT` entry in `docs/decisions.md` recording the
defect, the mechanism, Alexa's decision and the bound with its reasoning.
`docs/remediation/09-runner-self-limits.md` gains §8 and its **RN-1 release
note is corrected**: the sentence "a job refused twice ends with no verdict"
was true of `fa2ee15` and is false of this version — what a runner-first
fleet now pays is churn, not lost jobs. The board carries ORCH-061, the
written criterion for the S3 brief. The one new line on this branch that
said "she" of Alexa now says "he" (two places in the second `D-NEXT` entry);
the wider harmonisation is E5's and was left alone.

## Not done, and why

- **`check-docs.sh` and `check-orchestration.sh` are red**, on exactly the
  three `D-NEXT` placeholders. Expected and not fixed: `herd-merger`
  allocates the numbers (`gotchas/a-decision-branch-cannot-be-all-green.md`).
- **The hub still dispatches to a runner before reading one frame from it.**
  Out of this task's scope by the brief; written down as ORCH-061 for S3.
- **The 60 s shape of the link-cut test is not in the suite.** Run by hand
  twice, red then green, as the brief asked; the suite keeps the short one,
  which drives the same watchdog through `--lost-after`.

## Gates

`export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-no-life-spent`;
`./scripts/bench-preflight.sh` ran **ok** before each heavy phase.

```
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ nix develop --command cargo fmt --all --check
(no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.11s
(exit 0)

$ nix develop --command ./scripts/test.sh
(exit 0; every "test result:" line ok, 0 failed)

$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8595:## 2026-10-25 — D-NEXT A runner holds its own ceiling
8690:## 2026-10-25 — D-NEXT A runner announces its ceiling and the hub honours it
8793:## 2026-10-25 — D-NEXT A refusal at a machine's own ceiling spends no go
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs check failed
(exit 1 — the expected failure)

$ ./scripts/check-orchestration.sh
check-orchestration: 86 passed, 1 failed
(exit 1; the one failure is "check-docs.sh goes green on the repository as
 it is", which is the same three placeholders and cannot pass on a decision
 branch)
```

**The condition of merge, side by side.**

```
$ git -C . log --oneline -1          # fa2ee15, before any of the fix
$ cargo test -p herd-hub --test announced_job_ceiling \
      a_link_cut_while_the_runner_is_full_loses_no_job -- --nocapture

herd-hub: attempt 2 of j0000000001 was abandoned: the runner is already running 2 job(s) and never started this job: this machine lends 4 GiB and keeps 2 GiB as its own floor, and caps every job at 1 GiB, so this runner holds at most 2 job(s) at once
herd-hub: j0000000001 is lost: the runner is already running 2 job(s) and never started this job: ...
herd-hub: attempt 2 of j0000000002 was abandoned: ...
herd-hub: j0000000002 is lost: ...

panicked at herd-hub/tests/announced_job_ceiling.rs:951:5:
0 job dead and 0 job without a verdict is the condition this change merges
under, and 2 of 2 came back without one: ["j0000000001: {\"reason\":\"...;
this was attempt 2 of 2 at a runner that went away, so the job is not
attempted again (E-21)...\",\"type\":\"lost\"}", "j0000000002: ..."]

test result: FAILED. 0 passed; 1 failed; finished in 4.69s
```

and at the real 60 s, still on `fa2ee15`:

```
cutting the link for 65s, with the runner at its ceiling of 2
...
0 job dead and 0 job without a verdict ... and 2 of 2 came back without one
test result: FAILED. 0 passed; 1 failed; finished in 126.06s
```

after the fix, suite timings, ten consecutive runs:

```
herd-hub: attempt 2 of j0000000001 was abandoned: the runner is already running 2 job(s) and never started this job: ...
herd-hub: j0000000001 is queued again, handed back 1 of 8 times by a machine that was already as full as the ceiling it announced; no go at a runner that went away was spent on it: ...
herd-hub: j0000000001 stays queued: ... it is waiting for capped-runner: on capped-runner, 2 job(s) already out there and it said it holds at most 2 at once, so it takes nothing more until one of them ends: ... — 2 of them by the machine's own count, which it gave when it last handed an assignment back; this hub's records knew of 0 ...
herd-hub: j0000000001 is attempt 3 on runner capped-runner: ...
every one of the 2 jobs came back with its own verdict

test result: ok. 1 passed; 0 failed; finished in 10.49s   (p1 … p10 all ok)
```

and at the real 60 s, after the fix:

```
cutting the link for 65s, with the runner at its ceiling of 2
the link is back; the runner is still running its 2 original job(s)
every one of the 2 jobs came back with its own verdict
test result: ok. 1 passed; 0 failed; finished in 140.48s
```

**Skips.** Twelve `HERD-SKIPPED` lines from `./scripts/test.sh`, all
pre-existing host-capability skips on this workstation: cgroup v2 delegation
is unavailable under this session scope, and the data root is on ZFS, where
no cgroup IO knob bites. None of them is in a file this task touched. The
full list is in the suite output; they are printed as skips and counted as
skips (D27).

## Risks and findings

1. **The brief's chain was right, and incomplete.** Making the ceiling
   refusal free was necessary and not sufficient. Three further defects had
   to be closed before Alexa's oracle would hold, and each of them was found
   by the test going red for a new reason, not by reading:
   - **the hub re-offering the surplus immediately.** With the refusal free
     but the hub's count still stale, the same N jobs were placed, refused,
     requeued and placed again as fast as the hub could loop. The bound
     alone would not have saved them — it would only have changed how long
     they took to die. Part 4 of the fix is what makes the bound a backstop
     rather than the mechanism.
   - **the runner's job map is keyed by job id alone** (the review's §1.3,
     which it called pre-existing and now load-bearing). Handing back
     attempt 2 of a job dropped the *running* attempt 1 from the machine's
     own count; taking attempt 3 while attempt 1 ran replaced its entry;
     and an acknowledgement for an old attempt removed the live one. The
     last of these is how a job that was running perfectly vanished from
     its own health report and was then written off for silence. All three
     are closed; the map is still keyed by job id, which is now safe only
     because nothing can put two attempts of one job in it.
   - **the watchdog acts on an age it read before its own pass placed
     anything.** `watch_for_a_lost_runner` snapshots every open attempt's
     age, then walks the list; the walk requeues, which dispatches, which
     can give a job further down the list a brand-new attempt — judged
     against the old figure. Observed killing a 0.2 s-old attempt with a
     sentence about 60 s of silence. Fixed by reading the age again inside
     the loop.
2. **Can a job cycle without end?** No, by three separate bounds, and the
   first is the one that matters. (a) A refusal teaches the hub the
   machine's count, so the job *waits* instead of being re-offered; the
   correction ends only on that runner's next health report, which is the
   reconciliation. (b) Each refusal that does happen spends one of eight,
   and the eighth ends the job by name. (c) Every job still runs under its
   own time cap on whatever machine takes it. What the fix **can** cost is
   churn on a mis-paired fleet — a runner on this version against a hub
   that cannot read the announcement — and that is RN-1's corrected text.
3. **The bound is a judgement, not a measurement.** Eight is argued in the
   decision entry from the health cadence and from what the two older
   budgets are for; it is not derived from a run. A fleet that genuinely
   needs more has `--ceiling-refusals-allowed`.
4. **`herd-hub` grew a correction that lives on a connection.** It is lost
   when the link ends, which is correct — a new connection's records and the
   machine have not disagreed yet — but it means a runner that refuses and
   then immediately loses its link is offered the surplus again on the next
   hello. That costs one refusal per job, not a life, and it is the same
   thing ORCH-061 would remove.
5. **The 60 s run is a hand run, not a gate.** If CI's machine is slower
   than this one, the figure to watch is the suite version's 4 s outage
   against its 3 s `lost_after`; both are constants at the top of the test.

## Decisions needed

None. Alexa's decision of 2026-10-25 is recorded and implemented; nothing
on this branch now waits on a word.

## Resume point

`herd-merger` takes `runner-holds-its-own-limits`, rebases it on
`origin/main`, allocates **three** D numbers in order — the runner's ceiling,
the announced ceiling, what a ceiling refusal costs — and merges PR #82 on
green CI under ORCH-001. The PR body already carries the evidence above.

## Processes

None left running. Every `cargo` and `cargo test` was a foreground child of
this session and exited; the relay in the new test runs on threads inside
the test process and ends with it. No `pkill`, no `killall`, nothing killed
by name. `/mnt/ssd_980/herd/build/c5-no-life-spent` is removed.
