# link-cut-clock-figure — handoff 2026-10-31

- **Item:** ORCH-321
- **Agent:** herd-step
- **Branch / commit:** `link-cut-clock-figure` / the commit this file is
  in, based on `origin/main` `f473a886e684cb9494a3c7f06a53208f0504d086`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**No `.rs` file was changed. The task stopped on a refuted premise**, as
the brief instructs (*"Verify all of that yourself before changing
anything; if one part does not hold, say which and stop"*) and as
`herd-step`'s own contract requires of a premise the code refutes. What
exists now is the measurement, the mechanism and a worked remedy, plus
one extended `board.md` row.

**1. The figures the brief calls undeclared are declared.** Both are
named constants with doc comments of their own, in the test's own file,
`herd-hub/tests/announced_job_ceiling.rs` lines 657–681:

- `THE_OUTAGE` = 4 s (line 657),
- `LOST_AFTER_HERE` = 3 s (line 662) — **the hub's silence window**,
- `HEALTH_EVERY_HERE` = 1 s (line 663) — **the health-report cadence**,
- `SLEEP_ACROSS_THE_OUTAGE` = 6 (line 675) — how long each job runs,
- `PATIENCE_ACROSS_AN_OUTAGE` = 120 s (line 681).

`LOST_AFTER_HERE`'s doc comment already states the ratio and why:
*"Three to one, which is `herd_proto::SILENCE_MARGIN`: anything tighter
is refused at the handshake, and rightly."* So *"the test declares
neither"* is false of both figures the brief names.

**2. The silence window is 3 s, not 6 s.** The `6.017909028s` in the
failure text is not a threshold: it is the silence the watchdog
**measured** and then compared with `lost_after`. The sentence is built
at `herd-hub/src/server.rs:8212` from `unheard_for`, which is
`State::unheard_of_for`'s elapsed figure (`server.rs:3222`). A window of
6 s exists nowhere in this test or in the hub.

**3. `load1` does not predict the failure, so no load reading can gate
this test.** The named test, run alone, three times, this session:

| run | load1 before | verdict | wall | load1 after |
|---|---|---|---|---|
| 1 | 16.07 | ok | 10.76 s | 16.66 |
| 2 | 13.73 | ok | 10.57 s | 12.83 |
| 3 | 12.83 | ok | 10.46 s | 11.79 |

All three are **above** the **9.49** at which it went red inside a full
`gate.sh` suite, and above the 6.08 at which the board row records it
green. The discriminator is therefore **not the host's one-minute load**
but the *suite's own concurrency* — roughly thirty test binaries, each
with a hub, a runner and real subprocesses, contending for the runner's
event loop. Two consequences, both load-bearing for the brief's shape 3:
a skip keyed on `load1` would decline at load 16 where the test passes
and would **not** decline at load 7 where the suite could still break
it; and `gate.sh`'s `load_excuse` (`scripts/gate.sh` 461–491) only
excuses a skip whose reason names a figure **above 8**, so a red taken
at load 7 would stay red whatever the skip line said.

**4. The figure the test really hides is a host property, and it is
nowhere in the file.** From the hub's own code, three facts:

- the silence clock on an attempt **starts when the attempt is handed
  out**: `state.attempt_was_heard_of(job_id)` immediately after
  `begin_attempt`, `server.rs:5617`, commented *"Handing it out is the
  first word about it: the clock on this attempt starts here"*;
- an attempt that has only been **assigned** is in the air:
  `attempts_in_the_air` matches `JobState::Assigned | Running |
  Interrupted`, `server.rs:4032`;
- a runner that is connected and whose last health report does not name
  **that attempt number** gives `WhatTheRunnerSays::NotAnyMore`
  (`server.rs:3253`, `names_it` requires `held.attempt == attempt`),
  which is a write-off that **spends one of E-21's two goes**.

So the test silently requires that **the runner answer a dispatch within
`lost_after`** — 3 s here. On this host alone that is milliseconds; in
the failing gate run it took **6.02 s**, the hub charged the job's second
and last go, and the job ended `lost`. The accusation the brief quotes
is then printed because the oracle is a job count and cannot tell the
two causes apart.

**5. The remedy already exists in the same file, applied to the
neighbouring test and deliberately not to this one.** `78d4b76`,
2026-10-29, *"Build the ceiling bound test's premise instead of timing
it"* (ORCH-119) added `a_job_that_holds_its_place` and converted
`a_job_refused_past_the_bound_ends_and_names_it` from `sleep
{SLEEP_ACROSS_THE_OUTAGE}` to a job the test releases on its word. Its
doc comment is the diagnosis of tonight's failure, written two days
early: *"A `sleep` cannot keep that premise: it makes the premise a
duration, and whether the duration outlasts an outage, a watchdog, a
redial and a dispatch pass is then decided by whatever else the host is
doing."* The same commit re-scoped `SLEEP_ACROSS_THE_OUTAGE` to
*"Used by `no_job_is_lost_to_an_outage` **alone**"* — i.e. it left the
timed premise standing in the test that now flakes.

**6. Provenance, corrected.** The brief dates the test's last change to
`3d6e86a`, 2026-10-25. `git log -S` on the file: the figures came in
with `be2b742` (2026-10-25), and the shared body
`no_job_is_lost_to_an_outage` was last touched by **`78d4b76`,
2026-10-29** — the ORCH-119 commit above. The file itself was last
changed by `9b36871`, 2026-10-30.

**7. The board row.** `ORCH-321` is **extended, not closed**: its *Next
action* now carries the two refutations with their figures and the
redirected remedy, and its *Evidence* cell gains this handoff, `78d4b76`
and the three `server.rs` line numbers. Verbatim under *Drafted for
`herd-merger`*.

## Not done, and why

- **The fix itself.** A premise of the brief is refuted (items 1–3
  above), and both the brief and `herd-step`'s contract say to stop
  rather than work around it. The remedy I would write is not any of the
  brief's three shapes as described — it is a *construction* change plus
  an *attribution*-based third verdict — so choosing it is the
  orchestrator's, not mine. It is written out under *Decisions needed*
  so the re-brief is one step.
- **The hollow-green control.** Not run: there is no new version to
  prove still asserts. The control is designed and named under
  *Decisions needed* so whoever takes the re-brief runs it.
- **The Herd gate.** Not submitted. No `.rs`, `Cargo.toml` or script
  changed, so this branch is **prose only**; a `gate.sh` job would have
  spent a slot of a queue another agent is re-gating on, for a tree
  whose Rust gates nothing in it can move.
- **`docs/decisions.md`.** Untouched, and no `D-NEXT` drafted: the brief
  asked for none and this task decided nothing that needs one.
- **The skip lists.** Untouched. `scripts/expected-skips/local.txt`
  still holds **13** `HERD-SKIPPED` names and `local-under-load.txt` is
  unchanged, so `gate.sh`'s `skip-list-sha256` — which it computes from
  the names a run *observes*, not from the file — cannot move on this
  branch.
- **`docs/status.md`.** Untouched, **90** lines (D276), re-counted.

## Gates

**Rust gates NOT RUN locally, and not run through Herd either** — this
branch changes two Markdown files and no code, so there is nothing for
`cargo fmt`, `cargo clippy`, `./scripts/test.sh` or `gate.sh` to see
that `main` has not already gated. Under *One landing per batch* the
batch's own `./scripts/gate.sh` at integration covers them, and that run
is `herd-merger`'s.

Run **before** this file was committed, on the tree as it then stood —
three control runs of the one named test, which is what the brief allows
as a control:

```
### run 1, load1 before: 16.07
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 10.76s
    load1 after: 16.66
### run 2, load1 before: 13.73
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 10.57s
    load1 after: 12.83
### run 3, load1 before: 12.83
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 10.46s
    load1 after: 11.79
```

`./scripts/bench-preflight.sh` was run before the build and its
**verdict line** read, never `$?` through a pipe. It **refused** once —
*"load 9.10 is at or above 9.00 on 12 cores … bench-preflight: REFUSED,
do not start the heavy phase"* — and nothing heavy started; the second
run, after the load fell, read *"bench-preflight: ok, safe to start."*
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set or exported.

**The last run, named and not quoted:** `./scripts/check-docs.sh`, run
after this file was committed and before the push, with its output going
to the agent's report and to
`<scratchpad>/check-docs-after-handoff.txt`. Nothing is added to the
tree after it.

**Skips:** no skip. This branch adds none, removes none and changes no
skip list.

## Risks and findings

1. **The brief's diagnosis is refuted in two places, and the refutation
   changes the remedy.** Items 1–3 of *Done*. The brief's shape 2
   (*"declare the window as a figure the test owns … and named"*) is
   **already in the tree**: the window is `LOST_AFTER_HERE`, named, with
   a doc comment that derives its ratio from `herd_proto::
   SILENCE_MARGIN`. Doing shape 2 again would change nothing. Shape 3
   (*a skip naming the load it read*) is **worse than useless here**:
   measurement 3 shows `load1` neither implies the failure nor is
   implied by it, so the skip would decline runs that pass and admit
   runs that fail, and `gate.sh`'s own excuse grammar would refuse it
   below load 8 anyway.
2. **Shape 1 — the one the brief prefers — is not reachable from the
   test side.** The clock the test would have to remove is the **hub's**,
   and the hub reads real time: `Policy` is an immutable field of
   `State` (`server.rs:1434`), set once at construction, with no setter;
   the watchdog ticks on `tokio::time::interval(watchdog_every())`
   (`server.rs:8071`, `watchdog_every` = `lost_after / 20` clamped to
   50 ms…1 s, `server.rs:756`); and `tokio::time::pause` is refused on
   the `multi_thread` runtime this test needs and would in any case not
   move the real `sleep` of a real subprocess. Making the hub's silence
   window test-driveable — an injected clock, or a `Policy` that can be
   changed while the hub is up — is a **product change to `herd-hub`**,
   is far outside this task's write paths, and is the only thing that
   would close the whole class. It is a planner-sized row, not a step.
   **This is the finding that matters most**, because it is also the
   answer for every other `real_herd`-shaped fixture with the same
   shape (ORCH-128).
3. **What I would write instead, and why it is neither 2 nor 3.** Two
   halves, and the first is the one that does the work:
   - **Construct the premise instead of timing it**, exactly as
     ORCH-119 did for the sibling: replace `sleep
     {SLEEP_ACROSS_THE_OUTAGE}` with `a_job_that_holds_its_place` and
     release on the test's word, and replace the `sleep(THE_OUTAGE)`
     with a **bounded poll** until the hub has written off every
     attempt — the assertion that follows the sleep today becomes the
     poll's exit condition, so nothing is lost. That collapses the
     exposed window from *"the ~2 s for which the runner stays full
     after the link returns"* to *"one dispatch round trip"*, measured
     at 8–16 ms on an idle host by the file's own doc comment.
   - **A third verdict attributed from the record, with no clock and no
     load reading in it.** The hub already marks a ceiling refusal on
     the attempt itself: `AttemptRecord::at_its_ceiling`, and
     `JobRecord::goes_a_runner_took` (`herd-hub/src/job.rs:1869`) counts
     an attempt as a spent go only when `requeue_code.is_none() &&
     at_its_ceiling.is_none()`. So for a job that came back without a
     verdict the test can read its own record and say **which** of the
     two things happened: if the attempts the record itself attributes
     to the runner number fewer than `attempts_allowed` while the hub
     has nonetheless ended the job, then the difference was charged to
     something the hub marked `at_its_ceiling` — **the defect**, and the
     existing accusation is exactly right. If instead the record
     attributes `attempts_allowed` goes to watchdog write-offs and
     **zero** to a ceiling refusal, the product did what D182 says and
     the host broke the premise — *premise did not hold*, printed as a
     `HERD-SKIPPED` with the two figures (`lost_after`, and the silence
     the hub measured, both read off the record) rather than a load
     average. That is the file's own stated oracle made operative:
     *"Attempts spent because of the runner's ceiling: zero … told apart
     by attribution, never by a total that happens to be zero."*
   The residual risk, stated: a `HERD-SKIPPED` whose condition is an
   attributed event and not a load reading **cannot satisfy**
   `gate.sh`'s `load_excuse` grammar, so the name would have to go in
   `scripts/expected-skips/local-under-load.txt` *and* `gate.sh`'s
   excuse would have to admit a reason that names the hub's measured
   silence instead of a load figure. That is a change to `gate.sh`,
   which is a rule file and takes a `herd-reviewer` pass
   (`workflow.md` step 6). **It is the reason this is a decision and not
   a step**: the honest version of the third verdict costs a gate change,
   and the version that avoids the gate change is the dishonest one.
4. **A separate product finding, reported and not acted on, as the brief
   requires.** The hub's `NotAnyMore` arm asks whether the runner's
   health report names **this attempt number**, so a runner that is
   plainly still running *the same job* under an **earlier** attempt
   number reads as *"the attempt is gone from a machine that is still
   here"*. In this test that is the machine the hub has just written the
   attempt off at, so the reading is deliberate and D182's exemption is
   the hub's answer to it. But the write-off that follows is **not**
   exempt, and it is what spends the job's last go on a host that is
   merely slow. Whether `lost_after` should bound *"a runner has not
   answered a dispatch"* with the same figure it uses for *"a runner has
   gone silent"* is a product question I did not touch and did not
   measure in deployment. **No product figure, default or cadence was
   changed.** It wants its own row.
5. **One thing I could not measure, named as unmeasured.** I did not
   reproduce the red. It needs the full suite's concurrency, which is a
   `gate.sh` run — the brief forbids re-running a failed job locally,
   and manufacturing load on a host shared with the consumer's
   production and another re-gating agent would break D161. So the
   mechanism in item 4 of *Done* is established from the failure text
   plus the hub's code, and **not** from a reproduction. A re-brief that
   wants one should get it from a `gate.sh` receipt, not from this host.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** None asked for, none owed. Highest
real `D` on `main` re-measured at this branch's base: **D284**.

**One `board.md` row, already written on this branch** — `ORCH-321` is
**extended in place**, not added and not closed, and no row was
renumbered. Highest real `ORCH` row re-measured: **ORCH-322**. The
mentions of `ORCH-NEXT` placeholders inside dated records (ORCH-138,
ORCH-243, ORCH-307, ORCH-237) were not touched. The row's *Next action*
and *Evidence* cells now read, verbatim and in full:

```
| ORCH-321 | <description, source, owner and state cells unchanged> | ... | **Do not add it to a skip list** — it would be a real assertion silently excused, which is the one thing D27 forbids. Give it what D186 gave the other three: find the figure it silently depends on and declare it. The candidates are the 6 s silence window and the health-report cadence against a host at load 9.5, both of which are the test's own and neither of which it states. Then re-measure at rest and under load, and if it cannot be made load-independent, decide whether it belongs in `local-under-load.txt` as a **named** load-conditional skip that prints the figure it read, which is the only form that is not an excuse. **Re-measured 2026-10-31 by `link-cut-clock-figure`, and two of this row's premises do not hold.** Both figures ARE declared: `LOST_AFTER_HERE` 3 s and `HEALTH_EVERY_HERE` 1 s, named constants with doc comments at `announced_job_ceiling.rs:657-681`, and the `6.017909028s` in the failure text is not a window but the silence the watchdog *measured* against that 3 s. And **load1 does not predict the failure**: the test is green **alone** three times at load1 **16.07, 13.73 and 12.83**, in 10.76 / 10.57 / 10.46 s, all well above the **9.49** it went red at inside a full suite — so a load-gated skip would key on the wrong variable, and `gate.sh`'s own `load_excuse`, which demands a figure above 8, would not excuse a red taken at load 7. The figure this test really hides is a **host property**: the hub starts an attempt's silence clock when it *hands the attempt out* (`server.rs:5617`) and `attempts_in_the_air` watches `Assigned` (`server.rs:4032`), so the test silently requires the runner to answer a dispatch inside `lost_after`; under the suite's own concurrency that answer took 6.02 s. The remedy is already in the same file, applied to the sibling test by ORCH-119 (`78d4b76`, 2026-10-29) and deliberately not to this one: hold the originals open with `a_job_that_holds_its_place` instead of `sleep 6`, and poll for the write-off instead of sleeping `THE_OUTAGE`. **Next: re-brief on that premise**, with the three-way verdict attributed from the record's own `at_its_ceiling` field rather than from a load reading | — | `herd-hub/tests/announced_job_ceiling.rs:1079`; gate receipts for `adcc4df8c7de8f0f98e46b1cf9058c2e4665315f` and `13efad21c3e42cc8041b2b597ba79a529f8a3595`; **D186**, ORCH-128; `history/2026-10-31-oom-step-attribution.md`; `history/2026-10-31-link-cut-clock-figure.md`; `78d4b76` and ORCH-119; `herd-hub/src/server.rs` 4032, 5617, 8212 |
```

Nothing else on this branch is the merger's to act on.

## Decisions needed

1. **Which remedy, given that shape 2 is already in the tree and shape 3
   would key on a variable that does not predict the failure?**
   *Recommendation:* the two halves of *Risks and findings* 3 — construct
   the premise as ORCH-119 did for the sibling, and give the test a third
   verdict attributed from `at_its_ceiling` rather than from a clock or a
   load average. One `.rs` file, `herd-hub/tests/announced_job_ceiling.rs`,
   and the hollow-green control is already designed: delete
   `&& attempt.at_its_ceiling.is_none()` from
   `JobRecord::goes_a_runner_took` (`herd-hub/src/job.rs:1875`), which is
   D182's fix and nothing else, and the test must go **red on the defect
   branch** — not skip, not pass. *Fallback:* the construction half
   alone, which needs no `gate.sh` change and no skip-list name, and
   shrinks the exposed window from seconds to one round trip without
   closing it. That is a smaller claim honestly stated, and it is what I
   would land first if the gate change is refused.
2. **Does the class get its product seam?** A hub whose silence window a
   test can drive — an injected clock, or a `Policy` that may change
   while the hub is up — is the only thing that makes this test, and
   every other fixture of ORCH-128's class, independent of what else the
   host is doing. *Recommendation:* a `herd-planner` row, not a step, and
   not inside this fix. *Fallback:* leave the class open and pay the
   construction cost per fixture, as ORCH-119 and D186 have done twice
   already.
3. **Is `lost_after` the right bound for "a runner has not answered a
   dispatch"?** *Risks and findings* 4. *Recommendation:* its own row,
   measured against the deployed 60 s / 15 s pair before anything moves.
   *Fallback:* none needed — nothing is broken in deployment by it; the
   test's compressed 3 s / 1 s pair is what exposes it.

## Resume point

Re-brief ORCH-321 on the measured premise, with *Decisions needed* 1
settled. The next command for whoever takes it, in a worktree off
`origin/main`:

```
nix develop --command cargo test --package herd-hub \
  --test announced_job_ceiling -- --exact \
  a_link_cut_while_the_runner_is_full_loses_no_job --nocapture --test-threads=1
```

with `CARGO_TARGET_DIR` under `/mnt/ssd_980/herd/build/<task>` and
`./scripts/bench-preflight.sh` read by its verdict line first. The
binary builds in minutes and the test takes 10.5 s.

## Processes

PIDs started: one backgrounded `nix develop --command cargo test --no-run`
(harness id `bf04x1dtj`, exited 0 on its own) and three foreground
`cargo test` runs of the one named test, each of which exited on its own.
**Nothing was signalled**, so no PID was killed: `pkill -f` was not used
in any form, and 3006543, 3021002, 565152 and 662711 were not touched.
Own build tree: `/mnt/ssd_980/herd/build/link-cut-clock-figure`, off `/`
as required, deleted once the branch was pushed; its size is in the
agent's report.
