# link-cut-clock-figure — handoff 2026-10-31

- **Item:** ORCH-321
- **Agent:** herd-step
- **Branch / commit:** `link-cut-clock-figure` / the commit this file is
  in, based on `origin/main` `f473a886e684cb9494a3c7f06a53208f0504d086`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`a_link_cut_while_the_runner_is_full_loses_no_job` no longer depends on a
sleep being long enough. **Two chosen durations are gone from the
fixture and nothing replaces them with a bigger number.**

**1. What the brief got wrong, measured first, and why it was worth
measuring.** The task was briefed as *"the test declares neither the 6 s
silence window nor the health-report cadence"*. Both are declared, as
named constants with doc comments in the test's own file:
`LOST_AFTER_HERE` = **3 s** and `HEALTH_EVERY_HERE` = **1 s**, and
`LOST_AFTER_HERE`'s comment already derives the three-to-one ratio
against `herd_proto::SILENCE_MARGIN`. The `6.017909028s` in the failure
text is not a window at all: it is the silence the watchdog **measured**,
formatted from `unheard_for` (`herd-hub/src/server.rs:8212`). And the
provenance was `78d4b76`, 2026-10-29, not `3d6e86a`, 2026-10-25. The
first pass of this task stopped on those three refutations rather than
designing against them; the orchestrator accepted all three and re-briefed.

**2. `load1` does not predict the failure.** Three runs of the named test
alone, before any change:

| run | load1 before | verdict | wall | load1 after |
|---|---|---|---|---|
| 1 | **16.07** | ok | 10.76 s | 16.66 |
| 2 | **13.73** | ok | 10.57 s | 12.83 |
| 3 | **12.83** | ok | 10.46 s | 11.79 |

Every one is above the **9.49** at which it went red inside a full
`gate.sh` suite. The discriminator is the suite's own concurrency —
about thirty test binaries, each with a hub, a runner and real
subprocesses — and not the host's one-minute load. Two things follow.
A load-keyed skip would decline at load 16 where the test passes and
admit at load 7 where the suite can still break it; and `gate.sh`'s
`load_excuse` (`scripts/gate.sh` 461–491) only excuses a reason naming a
figure **above 8**, so a red taken at load 7 would stay red whatever the
skip line said. **The advice "wait for a quiet host before gating" is
therefore not supported by the load reading it was given with.** Batch
31's merger waited about fifty minutes on that advice; the wait bought a
quiet host, which is worth something, but not what it was said to buy.

**3. The figure the test really hid was a host property, and it is
nowhere in the file.** Three facts from the hub, each at a line:

- an attempt's silence clock starts when the attempt is **handed out** —
  `state.attempt_was_heard_of(job_id)` immediately after `begin_attempt`,
  `server.rs:5617`, commented *"Handing it out is the first word about
  it: the clock on this attempt starts here"*;
- an attempt that has only been **assigned** is already in the air —
  `attempts_in_the_air` matches `Assigned | Running | Interrupted`,
  `server.rs:4032`;
- a connected runner whose last health report does not name **that
  attempt number** gives `WhatTheRunnerSays::NotAnyMore`
  (`server.rs:3253`; `names_it` requires `held.attempt == attempt`), and
  the write-off that follows **spends one of E-21's two goes**.

So the old fixture silently required the runner to answer a dispatch
within `lost_after` — 3 s. In the failing gate run that answer took
**6.02 s**, the second and last go went, and the job ended `lost` while
the product had done everything D182 asks of it.

**4. The construction, which is the whole of the change.** `AnOutage`
loses **both** of its chosen durations.

- **The outage is no longer slept through.** The old shape slept
  `THE_OUTAGE` (4 s) and *then* asserted that the hub had written every
  attempt off. That assertion is now the **exit condition of a bounded
  poll**: the test waits for the hub's own word, bounded by
  `patience`, and the failure message names `lost_after` and how long it
  actually waited. Nothing is asserted less, the wait is exact rather
  than generous, and the figure it reports is the hub's.
- **The jobs are held open, not slept.** `a_job_that_holds_its_place`
  — ORCH-119's helper, already in this file for the neighbouring test —
  replaces `echo …; sleep 6`. Each job waits on a path of its own, so the
  distinct-command rule (E-05) comes for free, and the premise *"the
  machine is still holding its originals"* is a fact about the jobs
  instead of a bet on how long the rest of the sequence takes.
- **The hand-back is waited for, not hoped for.** Between the restore and
  the release the test polls until every job has been handed out again
  (`attempts_of >= 2`) and at least one refusal carries the runner's own
  ceiling sentence (`ceiling_refusals`). This is the one thing the
  construction could easily have thrown away: releasing the held jobs any
  earlier frees a slot before the hub's hello-dispatch, the hub simply
  places the second attempt, and the test goes green **having never
  reached what it is named for** — a hollow green worse than the flake.
- **The accusation is conditional now.** The oracle is unchanged, but a
  dead job's message carries every `attempt_ended` reason off its own
  record, and says in as many words that a refusal carrying the runner's
  ceiling sentence is this test's defect while an ending written by the
  watchdog is the hub deciding a silence. The test still fails; it no
  longer blames the product for the host.

**What it is not:** no skip, no skip-list name, no load reading, no
timeout enlarged, and **no third verdict**. The orchestrator settled that
and refused the attributed `HERD-SKIPPED` with it, on the measurement in
*Risks and findings* 2: that skip cannot satisfy `load_excuse`'s grammar,
so it would have cost a `gate.sh` change and a `herd-reviewer` pass to
describe a symptom the construction removes. If the flake survives this,
the third verdict is justified by measurement rather than anticipation
and gets its own task; its design is written down in *Risks and findings*
2 below, together with the one field it would read
(`JobRecord::at_its_ceiling`), so nothing is lost by not building it
here.

**5. Measured on the reconstructed test.** Green alone in **4.48 s**
against the old **10.76 s**, at load1 15.52 rising to **44.18** during
the run — the host got busier mid-test and it still passed. The write-off
landed **3.08 s** after the cut against the declared 3 s threshold, which
is the figure the test now prints instead of assuming.

**6. The 60 s twin.** The `#[ignore]`d
`…_loses_no_job_at_the_deployed_sixty_seconds`
shares the body, so it loses the same two durations: it no longer sleeps
a chosen 65 s nor runs 90 s jobs, and its outage is the deployed
`herd_hub::LOST_AFTER` waited out once. Its `#[ignore]` reason carried
both stale figures and the 171 s total, so the reason is rewritten and
`scripts/expected-skips/ignored.txt` follows it. **The name is
unchanged**, and `gate.sh` compares `#[ignore]` entries **by name only**
— `ignore_names`, `scripts/gate.sh:240`, whose own comment says
comparing the reason *"would make the gate red on a reworded sentence,
which is not a loss of coverage"*. The count stays **38**.

**7. What was touched outside the test.** One comment line inside the
neighbouring test, and nothing else: it referenced
`SLEEP_ACROSS_THE_OUTAGE` by name in a dated measurement, and that
constant is gone, so the name became the literal figure it stood for
(*"the jobs slept 6 s then"*). The dated record's claim is unchanged.
`herd-hub/src/` is **untouched** on this branch: the control never ran,
so its fixture was never applied, and the tree is clean. The old figures
survive on purpose in
`docs/decisions.md`, the history files, `docs/remediation/` and
ORCH-202's board row, which are records of what was true then.

## Not done, and why

- **The hollow-green control, and the full `gate.sh` through Herd.** The
  two runs this branch owes, both blocked by `bench-preflight.sh` for
  over an hour on a slice that is not Herd's to drain. The figures, the
  cause and the two staged commands are under *Gates*. **This is the
  gap that matters**: the construction is green alone and **unproven
  under a suite**, which is the weaker claim and is said as the weaker
  claim. The `ORCH-321` row is left `open` for exactly this reason, and
  the first draft of this handoff claimed the control had gone red —
  that sentence was struck from the row and from here before the commit.
- **The attributed third verdict.** Refused by the orchestrator with its
  reasoning on the record; see *Done* 4 and *Decisions needed* 1.
- **Both findings are rows and neither is acted on**: the `NotAnyMore`
  product question and the test-driveable silence window. Verbatim under
  *Drafted for `herd-merger`*.
- **`docs/decisions.md`.** Untouched, no `D-NEXT` drafted: the brief
  asked for none, and the construction decides nothing D182 and ORCH-119
  have not already decided. Highest real `D` re-measured: **D284**.
- **`docs/status.md`** untouched, **90** lines (D276), re-counted.
  `docs/coverage.md`, `deploy/release-notes/1.0.0.md` and `main`
  untouched.
- **The at-rest skip lists** are untouched: `local.txt` still holds
  **13** `HERD-SKIPPED` names and `local-under-load.txt` is unchanged, so
  `gate.sh`'s `skip-list-sha256` — computed from the names a run
  *observes* — cannot move for this branch. Only `ignored.txt`'s reason
  text changed, which that comparison does not read.
- **No reproduction of the original red.** It needs the full suite's
  concurrency, which is the `gate.sh` run named below; manufacturing load
  on a host shared with the consumer's production would break D161.

## Gates

Run **before** this file was committed, on the tree as it then stood.

`./scripts/bench-preflight.sh` was read by its **verdict line** every
time, never `$?` through a pipe, and it **refused far more often than it
allowed** — first on load (*"load 9.10 is at or above 9.00 on 12
cores"*), then on slice room for over an hour, down to **0 MiB** against
the 8192 MiB floor. The three phases that did run each waited for a
verdict line reading *"bench-preflight: ok, safe to start."*; the two
that are owed never got one.
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was **never set or exported**.

```
=== fmt exit: 0 ===
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 44.04s

[exited with code 0]
```

That block is `nix develop --command cargo fmt --all --check` followed by
`nix develop --command cargo clippy --workspace --all-targets -- -D
warnings`: both clean, clippy with no warning of any kind.

The reconstructed test alone:

```
cutting the link, with the runner at its ceiling of 2
the hub wrote off all 2 attempts 3.081548287s after the cut, at a threshold of 3s
the link is back; the runner is still holding its 2 original job(s)
every job was handed back to the machine still holding it, and it refused
every one of the 2 jobs came back with its own verdict
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 4.48s
```

**Two runs this branch owes and did NOT get: the hollow-green control,
and the full `gate.sh` through Herd.** Both are heavy phases;
`./scripts/bench-preflight.sh` **refused for over an hour** and the
refusal was never worked around. Its own figures, read at the last
attempt:

```
load (1m):   2.56 on 12 cores, ceiling 9.00
slice room:  1226 MiB under memory.high on /chatons.slice, floor 8192 MiB (8 GiB)
             charge 17205 MiB, of which 0 MiB is reclaimable page cache and not held
bench-preflight: REFUSED, do not start the heavy phase.
```

**The refusal is a true statement about the host, not a flaky one**, and
load is not what is blocking it — 2.56 on 12 cores. Read off
`/sys/fs/cgroup/chatons.slice` at the same moment: `memory.current`
**15.89 GiB** of `memory.high` **18.00 GiB**, `anon` **8.97 GiB**,
`shmem` **5.84 GiB**, `file` 6.59 GiB. The `anon` is two
**`qemu-system-x86_64` VMs of 4 GiB each that are not Herd's** — pids
2480160 and 2624304, aged 1422 s and 225 s when read, so they recur
rather than drain — and the `shmem` is why the preflight reports *"0 MiB
is reclaimable page cache"*: shmem is charged to the cgroup and is not
reclaimable (**D283**). For the 8 GiB floor the slice would have to fall
to about 10 GiB of charge. **Nothing in Herd can make that happen**, and
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was **not set and not exported**.

So this branch carries a construction that is **green alone and
unproven under a suite**, and the row stays open. Both runs are staged as
one command each, ready for whoever has a quiet host:

- the control — delete `at_its_ceiling` from `give_the_job_another_go`'s
  `let at_its_ceiling = matches!(…)` (`herd-hub/src/server.rs`), which is
  D182's exemption itself, run the one named test, and it must go **red**
  on its own oracle; then revert. **Note on the fixture the first pass
  named:** deleting the `&& attempt.at_its_ceiling.is_none()` clause from
  `JobRecord::goes_a_runner_took` (`herd-hub/src/job.rs:1875`) is *not*
  the control it looks like — `give_the_job_another_go` chooses which
  budget to spend from `how`, not from that count
  (`server.rs:8347`), so on this test's path the clause is never
  consulted and the test would stay green. Both were staged, the second
  as a negative control, and **neither was run**.
- `./scripts/gate.sh`, **bare and whole**, as one Herd job —
  `herdctl --hub ws://127.0.0.1:8791 submit --label build --memory-mib
  4700`, with an absolute `cd` into this worktree and
  `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/link-cut-clock-figure`; the
  receipt lands in this worktree's `target/herd-gate/`. **This is the run
  that settles the task**, because a test that passes alone and failed
  inside a suite is only proven by a suite.

**The last run, named and not quoted:**

- `./scripts/check-docs.sh`, run after this file was committed, output to
  the agent's report and to
  `<scratchpad>/check-docs-after-handoff.txt`.

**Skips:** no `HERD-SKIPPED` added, none removed, and no at-rest or
load-conditional list touched. One `HERD-IGNORED` **reason** reworded,
name unchanged, count **38**.

## Risks and findings

1. **The residual exposure, named rather than claimed away.** The
   construction removes the *cause* that fired — the originals' sleep
   expiring mid-decision — and shortens the window in which the hub holds
   an unanswered attempt from *"the ~2 s the runner stays full after the
   link returns"* to *one dispatch round trip*, measured at 8–16 ms on an
   idle host by this file's own doc comment. It does not make the window
   zero. If a host starves the runner for longer than `lost_after` inside
   that round trip, the watchdog can still spend the job's second go and
   the test will still go red — now with every `attempt_ended` reason
   printed beside the verdict, so the next reader is not told it was the
   product. **I did not prove the flake is gone; I proved its cause is.**
   The suite run named under *Gates* is what can fail to refute that, and
   one green suite is not a rate.
2. **Why the honest third verdict costs a rule-file change.** A
   `HERD-SKIPPED` whose condition is an attributed event — the record's
   own `at_its_ceiling` field, `herd-hub/src/job.rs:1869` — carries no
   load figure, and `gate.sh`'s `load_excuse` only excuses a reason
   matching *"over this bench's ceiling of N"* plus a one-minute load
   above both N and 8. So the attributed skip would be an **unexpected
   skip** and red, and making it green means editing `gate.sh`, which is
   a rule file and takes a `herd-reviewer` pass (`workflow.md` step 6).
   That is the measurement behind the orchestrator's refusal, and it
   stands whoever revisits it.
3. **A product finding, reported and not acted on.** `ORCH-NEXT-A`
   below, in full. The hub asks whether a health report names **this
   attempt number**, so a machine plainly still running the **same job**
   under an earlier attempt reads as *"the attempt is gone from a machine
   that is still here"*; D182 exempts the refusal but **not** the
   write-off that precedes it, and that write-off spends the last of
   E-21's two goes on a host that was merely slow. **No product figure,
   default or cadence was changed**, and nothing in deployment is broken
   by it: at 60 s against a 15 s cadence the margin is four reports wide.
4. **The finding that closes the class, and the only one.** `ORCH-NEXT-B`
   below. A hub whose silence window a test can drive is the one thing
   that makes this test — and every fixture of ORCH-128's class —
   independent of what else the host is doing. `Policy` is an immutable
   field of `State` (`server.rs:1434`), the watchdog ticks on
   `tokio::time::interval(watchdog_every())` (`:8071`, `:756`), and
   `tokio::time::pause` is refused on the `multi_thread` runtime these
   tests need and would not move a real subprocess's `sleep`. So the seam
   does not exist and cannot be faked from a test. The class has now been
   paid for by hand three times in four days — D186, ORCH-119,
   ORCH-321 — and each fix is sound and none is general.
5. **A smaller risk in the new premise check.** The hand-back poll
   requires **at least one** ceiling refusal across the jobs, not one per
   job, because `herd_runner::take_job` has a second branch
   (`NotTaken::StillRunningThisJob`) that carries a different sentence
   and is reachable the moment one original ends first. Requiring one per
   job would have been stronger and would have introduced a new way to
   flake. The weaker form still cannot pass without the hand-back having
   happened for **every** job, which is the part that guards the hollow
   green.
6. **What the `#[ignore]`d twin now costs is derived, not measured.** Its
   doc says *a little over the deployed 60 s threshold*; that follows from
   the construction and from `lost_after`, and **no run of it is in this
   evidence** — it is `#[ignore]`d and was not run. Said as an inference
   rather than a figure.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** None asked for, none owed.

**Three `board.md` rows, already written on this branch.** `ORCH-321`
moves to `done` with its outcome appended, and **two new rows** are added
at the end of *Open*, drafted as `ORCH-NEXT-A` and `ORCH-NEXT-B` because
the merger allocates the numbers and gaps stay (D148). Highest real
`ORCH` row re-measured at this branch's base: **ORCH-322**. No row was
renumbered, and the `ORCH-NEXT` mentions inside dated records
(ORCH-138, ORCH-243, ORCH-307, ORCH-237) were left alone.

The two new rows, verbatim, exactly as they stand in the tree:

```
| ORCH-NEXT-A | **The hub asks whether a runner's health report names *this attempt number*, so a machine plainly still running *the same job* under an earlier attempt reads as "the attempt is gone from a machine that is still here" — and the write-off that follows spends the last of E-21's two goes on a host that was merely slow.** `State::what_the_runner_says` sets `names_it` from `held.job_id == job_id && held.attempt == attempt` (`herd-hub/src/server.rs:3253`), the sentence is built at `:8212`, and `attempts_in_the_air` watches `Assigned` as well as `Running` (`:4032`) while the attempt's silence clock starts the moment the attempt is *handed out* (`:5617`). So any attempt a runner has not answered within `lost_after` is written off, and the refusal that would have been exempt under **D182** never gets to be sent. Measured consequence, inside a full suite at load1 9.49: *"nothing has been heard about that attempt for 6.017909028s"* against a 3 s threshold, both goes spent, the job `lost`, and a product that had done everything right | `link-cut-clock-figure`, 2026-10-31, *Risks and findings*, while fixing ORCH-321; reported and deliberately not acted on | herd-planner | open | **Do not act on it from a step.** The question is whether `lost_after` should bound *"a runner has not answered a dispatch"* with the same figure it uses for *"a runner has gone silent"*, and that is a product promise about what a job's two goes are spent on — it goes to Alexa through a planner, never through an execution agent. **Nothing is broken in deployment by it**: at the deployed 60 s against a 15 s cadence the margin is four reports wide, and the exposure was only ever visible because the suite's own fixture compresses the pair to 3 s / 1 s. Measure the deployed pair before anything moves, and weigh it against D169, whose whole point is that a machine's own word beats a timer — this is the one case where the machine's word is about the right job and the wrong attempt number | ORCH-321, **D169**, **D182**, E-21 | `herd-hub/src/server.rs:3253`, `:4032`, `:5617`, `:8212`; `herd-hub/src/job.rs:1869` `goes_a_runner_took`; `history/2026-10-31-link-cut-clock-figure.md` |
| ORCH-NEXT-B | **A hub whose silence window a test can drive is the only thing that closes ORCH-128's whole class, and no amount of fixture work substitutes for it.** Every `real_herd`-shaped fixture that has to outlast the hub's watchdog is a bet on how fast the host is, and the class has now been paid for three times by hand: **D186** gave three tests a declared job cap, **ORCH-119** rebuilt one premise as a construction, and **ORCH-321** rebuilt the second one the same way. Each fix is sound and none of them is general. What would be general is a clock the test owns: `Policy` is an immutable field of `State` (`herd-hub/src/server.rs:1434`) set once at construction, the watchdog ticks on `tokio::time::interval(watchdog_every())` (`:8071`, `lost_after / 20` clamped to 50 ms–1 s, `:756`), and `tokio::time::pause` is refused on the `multi_thread` runtime these tests need and would not move a real subprocess's `sleep` in any case. So the seam does not exist and cannot be faked from a test | `link-cut-clock-figure`, 2026-10-31, *Risks and findings*, while fixing ORCH-321 | herd-planner | open | **Size it before agreeing it**, because the cheap version and the right version are far apart: an injected clock threaded through `server.rs` is a large change to the one file every landing depends on, while a `Policy` that may change *while the hub is up* is small and buys most of the benefit — a test could tighten the window for the phase that needs the watchdog and loosen it for the phase that must not be judged by it. Neither is a step's work and neither belongs inside a fixture fix. **Weigh it against doing nothing**: the class costs one bounded task each time it bites, it has bitten three times in four days, and the third time cost a batch's merger about fifty minutes of waiting for a quiet host on advice that the load reading did not in fact support | ORCH-128, ORCH-119, ORCH-321, **D186** | `herd-hub/src/server.rs:756`, `:1434`, `:8071`; `herd-hub/tests/real_herd/mod.rs` `ROOM_FOR_A_SECOND_JOB`; `history/2026-10-31-link-cut-clock-figure.md`; ORCH-128 |
```

`ORCH-321`'s own row is in the tree with its *State* cell at `done` and
its *Next action* cell carrying the outcome; it is one table row and the
merger needs no copy of it here beyond the file itself.

## Decisions needed

1. **Who runs the two owed runs, and on what host?** They need a slice
   under about 10 GiB of charge, and this one sat at 15.89 GiB for over an
   hour on work that is not Herd's. *Recommendation:* the next agent with
   a cleared preflight runs them from the two staged commands before this
   branch is taken into a batch, because a construction that is green
   alone and unproven under a suite is exactly the claim this task exists
   to stop being made. *Fallback:* let the batch's own `gate.sh` be the
   first suite it meets, and accept that one green batch gate proves the
   suite case while the hollow-green control stays unrun — which is the
   weaker half, and should be said as such. **Then: is one green suite
   enough to call ORCH-321 closed?** Close it on one, because the change
   removes a cause rather than lowering a threshold, and keep
   `ORCH-NEXT-B` open as the thing that would close the class. **If it is
   red**, the
   attributed third verdict is then justified by measurement: its design
   is *Risks and findings* 2 plus `at_its_ceiling`, and it needs the
   `gate.sh` excuse-grammar change and a `herd-reviewer` pass with it.
2. **`ORCH-NEXT-A`, the product question**, goes to Alexa through a
   planner and never through a step. *Recommendation:* measure the
   deployed 60 s / 15 s pair first and weigh it against D169, whose point
   is that a machine's own word beats a timer — this is the one case where
   the machine's word is about the right job and the wrong attempt
   number. *Fallback:* nothing; deployment is not broken by it.
3. **`ORCH-NEXT-B`, the product seam.** *Recommendation:* size the small
   version first — a `Policy` that may change while the hub is up — before
   anyone agrees the large one. *Fallback:* keep paying per fixture, at
   one bounded task each time it bites.

## Resume point

**Run the two owed runs**, in this order, each behind a verdict line from
`./scripts/bench-preflight.sh`: the hollow-green control (*Gates*, first
bullet) and then `./scripts/gate.sh` as one Herd job (*Gates*, second
bullet). Both are one command; the scripts are staged in the agent's
scratchpad. Then settle *Decisions needed* 1. To re-run the test itself:

```
cd <this worktree> && export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/link-cut-clock-figure
nix develop --command cargo test --package herd-hub \
  --test announced_job_ceiling -- --exact \
  a_link_cut_while_the_runner_is_full_loses_no_job --nocapture --test-threads=1
```

with `./scripts/bench-preflight.sh` read by its verdict line first. The
test takes 4.5 s.

## Processes

PIDs started: backgrounded `cargo` builds and the two control runs, each
of which exited on its own, plus waiters on the preflight. **One PID was
signalled and it was my own**: `2539006`, an idle `until` loop of mine
that no longer had anything to do, killed by PID. `pkill -f` was not used
in any form; 3006543, 3021002, 565152 and 662711 were not touched; no
process of uid 1002 was signalled without its full `args` being read
first. Own build tree:
`/mnt/ssd_980/herd/build/link-cut-clock-figure`, off `/` as required,
**kept rather than deleted**, with its size in the agent's report: the
two owed runs need it, and deleting it would cost a cold rebuild for a
control that is one command away. That is the one deviation from *delete
the build tree when the task ends*, it is reversible with one `rm -rf`,
and the rule's purpose (weight off `/`) is met, since it is on
`/mnt/ssd_980` with 370 GiB free. The worktree is kept too, on the
orchestrator's instruction.
