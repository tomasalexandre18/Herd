# self-collision-no-reoffer — handoff 2026-11-01

- **Item:** ORCH-NEXT (first of the four rows this branch adds to the
  board's *Open* section; the merger allocates the numbers)
- **Agent:** herd-step
- **Branch / commit:** `self-collision-no-reoffer` / `e460f6f` (the gated
  commit) plus this handoff commit, based on `main`
  `9e8fdefc0d5786d37cfdb9344494423eddfed951`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

T3 and T4 of the 2026-10-31 analysis, in one branch because T3 cannot be
correct without T4's field.

**T4 — the wire says which refusal it was.**
`herd_proto::ToHub::AttemptAbandoned` carries a second defaulted field,
`still_running_attempt: Option<u32>`, naming which attempt of this very
job the machine is still running when that is why it refused. `None` on
every other abandonment, which is what its absence has always meant.

- `PROTOCOL_VERSION` stays **3** and `RECORD_VERSION` stays **2**. The
  brief's reading is correct and I verified it rather than taking it:
  `PROTOCOL_VERSION`'s own doc comment, `herd-proto/src/lib.rs:140`-`:142`,
  lists among the changes that do **not** move it *"a new
  `#[serde(default)]` field on an existing variant, when its absence means
  exactly what its default says (the precedent is `JobMetrics`' `pressure`,
  D73)"*. The field is `#[serde(default, skip_serializing_if =
  "Option::is_none")]`, so an older hub drops the key and an older runner
  sends none. The in-tree assertion the brief named is at
  `herd-hub/tests/announced_job_ceiling.rs`
  `neither_version_number_moves`, which pins the two literals 3 and 2,
  and it is green on the gated sha. Nothing on the record changed at all:
  no new `Attempt` field, no new `JobEventKind`, no new `JobOutcome`.
- The runner fills it: `herd-runner/src/job.rs`'s refusal arm is split in
  two, and `Session::abandon_still_running_this_job` is the second of the
  two methods that build the frame. The count `at_its_ceiling` still
  travels with it, because it is still true — the machine is holding that
  many — and it is no longer the only thing the hub has.
- The wire shape is pinned as bytes in `herd-proto/tests/round_trip.rs`:
  three `to_frame` assertions (no key, the ceiling refusal's key alone,
  and both keys), one `from_frame` of an older runner's frame reading
  back as `still_running_attempt: None`, and a round trip of the
  populated frame.

**T4's prose — the design statement the measurement refuted.** The
paragraph at `herd-proto/src/lib.rs:1764`-`:1767`, *"Both are the same
fact … and both carry the same consequence"*, is replaced. The
replacement keeps the half that holds — the fact is one, and neither
refusal spends one of E-21's goes — and states what differs, which is the
**rate**, with the mechanism and the field measurement: a ceiling refusal
reports `holding == most` so the hub's count-based guard fires and is
self-limiting; this one reports `holding < most` by construction, so the
same guard is guaranteed inert exactly where the refusal is guaranteed to
repeat. This is the paragraph a `herd-reviewer` pass is for.

**T3 — the hub does not offer a job to a machine that just refused that
job.** On a refusal carrying `still_running_attempt`, the hub records the
`(job_id, runner)` pair on the connection and the matcher answers
`Admission::ShortNow` for that pair, so the job **waits** for the place
it is already in instead of being handed back at the speed of the queue.

- The pair lives on `ConnectedRunner::it_said_it_is_running_these`
  (`herd-hub/src/server.rs`), beside `it_said_it_is_holding` and with the
  same lifetime: it dies with the connection, so a redial offers the job
  again and the machine refuses again if it is still holding it.
- It is cleared by the machine's own next health report, in
  `State::record_health`, which now also returns true when a pair cleared
  so the queue is judged on that same report. **The clearing reads what
  the report *named*, not what survived being judged** — and that
  distinction is load-bearing: `mind_a_reported_job` refuses the entry
  about the written-off attempt as `Superseded`, and that refused entry
  is the only evidence the hub gets that the place is still taken. Clearing
  on the filtered list would clear on the very report that proves the
  exclusion is still needed.
- No new frame, no new clock, no third budget. The brief's two named hooks
  were both already there and both were used: `mind_a_reported_job`'s
  caller for the clearing, and `running_jobs` on the runner's side for the
  naming.

**The sentence is attributable.** `herd_runner::A_RUNNERS_OWN_SECOND_ATTEMPT`
= `"one machine runs one job once"` is a fourth exported clause, and the
refusal sentence interpolates it rather than repeating its words, so the
two cannot drift. `herd-hub/tests/announced_job_ceiling.rs`'s
`the_runners_ceiling_sentence_is_not_the_hubs` now asserts over all four
clauses and all five sentences as they are really built, through a new
`herd_runner::testing::a_refusal_for_a_second_attempt()` — the same shape
as the existing `a_refusal_for_the_memory_sum()`, so the hub-side test
reads the sentence rather than a copy of it.

**The known-bad, recorded on `9e8fdef` before anything was changed.** The
fixture was run in a throwaway worktree detached at
`9e8fdefc0d5786d37cfdb9344494423eddfed951`, with the frame an un-upgraded
v1.0.0 runner really sends — `at_its_ceiling: Some(1)` and no
discriminant — and it reproduces the consumer's record figure for figure:

    KNOWN-BAD: 8 refusal(s) in 5.292577698s, offers [2,3,4,5,6,7,8,9]
    KNOWN-BAD: state "finished" outcome {"refusals":8,
      "refusals_allowed":8,"runners":[{"holding":1,
      "name":"storm-runner"}],"type":"refused_as_full", "reason":"...
      the hub's count of what is out on storm-runner (holding 1) and
      theirs simply never agreed. Add capacity, or look at why the two
      counts disagree"}
    KNOWN-BAD: attempts on the record 9, refusals matching the clause 8

Eight refusals, attempts 2 to 9, nine attempts on the record, and the
hub's own sentence verbatim — the one the brief quoted from the
consumer's instance. The 5.29 s against their 0.51 s is this fixture's
round trip through a test harness; the count is the thing. That worktree
and its build tree were removed. The two-part oracle also fails on
`9e8fdef` at its first half, with the offer named:

    the machine said it is still running this job, so the hub must offer
    it no further attempt of it: [AssignJob { job_id:
    JobId("j0000000001"), attempt: 3, ... }]

**The test, green on the fix and needing no clock.**
`herd-hub/tests/one_machine_runs_one_job_once.rs`, one test, **0.32 s**,
no sleep, no cut socket, no `tokio::time::pause`, no `Policy` seam. Both
halves of the analysis §4.5 oracle: the job does not reach a terminal
state and in particular not the `refused_as_full` **outcome** while the
machine still names it, and exactly **one** refusal carries the clause
where the first consumer's record carries eight. Plus a matcher unit
test, `a_job_a_machine_is_already_running_is_not_offered_to_it_again`,
which asserts both halves on one seat: holding 1 of 2 is admissible, and
the one excluded job is not.

## Not done, and why

- **The brief's budget ruling — "no third budget, and no budget at all
  for this case" — is half done, deliberately.** No third budget was
  added. The existing ceiling budget is still charged for this refusal,
  which the exclusion reduces from eight charges in half a second to **at
  most one per episode**: after the refusal the pair is excluded, and when
  the report stops naming the job the place is free, so the next offer is
  taken rather than refused. Two reasons for stopping there, one of scope
  and one the tree gave me. They are in *Risks and findings* and the
  second row of *Drafted for `herd-merger`*.
- **`herd-runner/src/confine.rs`'s "three sentences about room" is now
  four and was left unedited**: `confine.rs` is outside the brief's write
  paths, and the disjointness claim those paragraphs make is still true —
  it is the count in the prose that is stale. Fourth board row.
- **No `docs/decisions.md` write**, no `docs/status.md` write, no
  `docs/coverage.md` write, no `CLAUDE.md` write: out of scope, and the
  drafted decision entry is below in full.
- **No merge, no tag, no pull request, no push to `main`.**

## Gates

All four Rust gates plus the docs checks, run **whole and bare through
the queue** on the gated commit `e460f6f`, never per step. Verbatim
verdict:

    gate: GREEN sha=e460f6f997f83113c3432671dd1ab56b4b05d568 steps=0-8:ok
    host=servernixos load1=6.13 load1-end=9.00
    skips=under-load(flood-fifty=11.09>8,flood-two-fifty=11.09>8)

Receipt, on the gated tree:

    verdict: GREEN
    sha: e460f6f997f83113c3432671dd1ab56b4b05d568
    branch: self-collision-no-reoffer
    host: servernixos
    nproc: 12
    load1-start: 6.13
    load1-end: 9.00
    started: 2026-11-01T02:40:36Z
    finished: 2026-11-01T02:49:33Z
    load-excused-skips: flood-fifty=11.09>8,flood-two-fifty=11.09>8
    skips-observed: 15
    skip-list-sha256: c9dd3040103efcedea68c62392b44e653304b933528293d088
                      6f69b29a36222f
    ignored-observed: 38
    ignore-list-sha256: f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c
                        4744d2aed7a46545
    steps: 0..8 all ok

Step 6, the suite, took 431 s. Steps 1 and 2, the docs checks, were part
of this same run and were ok.

**The skips, as a list and not a count.** 15, which is the at-rest 13 plus
the two load-gated flood benches, each of which read the load itself and
declined: `flood-fifty` and `flood-two-fifty`, both at 11.09 against
their own ceiling of 8. `skip-list-sha256`
`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f` is the
documented 15-skip value, and `ignore-list-sha256`
`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545` over
**38** is the documented at-rest value. **Both shas are unmoved: this
branch's fixture adds no `HERD-SKIPPED` line and no `#[ignore]`.** No
`HERD-SKIPPED` line was copied as a pass.

**The last run**, made after this file was committed and therefore not
quotable here: `./scripts/check-docs.sh`, from the worktree root, output
to the terminal and reproduced verbatim in this agent's report.

## Risks and findings

1. **The brief's budget ruling is refuted by an existing green test, and
   that is the finding I would most want read.** Had I charged nothing for
   a self-collision, `announced_job_ceiling.rs`'s
   `a_job_refused_past_the_bound_ends_and_names_it` would have become
   unreachable in one of its two regimes. That test runs with
   `ceiling_refusals_allowed: 1` and asserts `!refused.is_empty()` — at
   least one job must end `refused_as_full` — and its own comment at
   `:1645`-`:1655` records the measurement: *"4 failures in 20 on two CPUs
   under four spinners and 13 in 20 under eight — every one of them
   `holding` 1 with the second branch's sentence"*. In that regime the
   **only** refusal the fixture produces is this very branch, so with no
   charge no job would reach the bound and the assertion would fail on a
   loaded host. The brief invited exactly this correction.
2. **And the other half of it is scope.** Charging neither budget needs
   the record to distinguish a third kind of ending — a `HowItEnded`
   variant and an `Attempt` field in `herd-hub/src/job.rs` and
   `herd-hub/src/store.rs`, neither of which is in the brief's write
   paths, and a record-format addition owing a `D-NEXT` of its own.
   Recording a self-collision as a ceiling refusal *without* charging it
   would be worse than either: `JobRecord::goes_a_ceiling_refused` counts
   the record, so uncharged refusals would still inflate `ceilings_hit`
   and a later genuine ceiling refusal would find its budget short.
3. **The path the brief asked me to look for does not exist: the pair
   cannot fail to clear.** The exclusion lives on the connection, so it
   cannot outlive it — a link that goes takes every pair with it, and
   there is no residue on disk and none in the registry. While the link
   lives, a runner that stops reporting is dropped at the liveness
   threshold. So there is no "waits for ever with no timeout covering it",
   and the outer bounds are untouched: E-21's goes still cover a runner
   that really went, and the exclusion costs at most one health cadence of
   latency when the place frees.
4. **A throughput cost, named.** A place that frees is noticed at the next
   health report and not sooner, because the report is the only clearing
   hook. That is the same cost `it_said_it_is_holding` already pays, for
   the reason `record_health`'s own comment gives, and it is bounded by
   `HEALTH_EVERY` = 15 s. Measured where it could have bitten:
   `a_link_cut_while_the_runner_is_full_loses_no_job` runs at a 1 s
   cadence inside 120 s of patience and is green.
5. **Two of the brief's write paths were short, and I treated the brief's
   list as the concern rather than the file list.** The field cannot be
   put on the frame without `herd-runner/src/lib.rs` (the `Session`
   methods that build it, and the sentence), and the attribution
   assertion cannot read a real sentence without
   `herd-runner/src/testing.rs`. Both are the same change seen from one
   crate further in; neither is a second concern. Also touched, to
   compile: `herd-hub/tests/link.rs` and `herd-proto/tests/round_trip.rs`,
   one field each.
6. **One assertion I wrote was vacuous and the known-bad run caught it.**
   `refused_as_full` is an **outcome** under a `finished` state, not a
   state, so `file_state_of(...) != "refused_as_full"` could never fail.
   It now reads the outcome's own `type`. Worth saying because the same
   mistake in a test watching only the state would read the consumer's
   dead job as a job that ended well.
7. **ORCH-330 was not needed**, and the brief was right to tell me to look
   for the door first. Third board row.
8. **Not re-derived and not re-checked:** T1 (`D291`) and T2 (`D292`) have
   already landed and this branch does not touch them. I did not measure
   the consumer's instance or anything on it (invariant 9, D161); every
   field figure here is from their report, through the brief.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full

## 2026-11-01 — D-NEXT A second attempt of one job is a rate, not
a ceiling

Two refusals a runner can make are about its own bookkeeping, and
`herd-proto` said in writing that they *"are the same fact … and both
carry the same consequence"*. The first half holds. The second is false,
and the first consumer's `j0000000655` is the measurement: on 2026-10-31,
v1.0.0, after a 71.28 s link silence at host load 172, the hub's watchdog
wrote attempt 1 off while that machine was still running it, offered the
job back to the same machine eight times between 20:28:37.160Z and
20:28:37.671Z, and ended it `refused_as_full`, exit 125, with no verdict,
for work no machine had failed at.

The mechanism, and why it is a rate. The hub's one throttle for this shape
is a count: `matcher.rs`'s `out_there >= at_once`, fed by what the machine
itself said it was holding. A refusal at the announced ceiling reports
`holding == most`, so the guard fires and nothing more is placed there
until the next health report squares the counts — self-limiting, which is
the behaviour D182 was written around. A refusal for a second attempt of
one job reports `holding < most` **by construction**, because
`herd_runner::Session::take_job` answers the ceiling first whenever
`running >= most_at_once`: that branch is reachable only below the
announced count. So the guard is guaranteed inert exactly where the
refusal is guaranteed to repeat, and `give_the_job_another_go` ends on
`dispatch_queued_jobs` with no delay anywhere on the path. Their figures
were holding 1 of 2 places. `1 >= 2` is false.

So the decision has two parts. **The wire says which refusal it was**: a
second `#[serde(default)]` field, `still_running_attempt`, on
`ToHub::AttemptAbandoned`, naming the attempt that is in the way. In a
field and not in the `reason`, which is prose a hub may not parse; and in
this field rather than inferred from the count being under the announced
ceiling, because that ceiling reached the hub on a different frame, at a
different moment, and sometimes from a different process of the same
runner. Neither version number moves: `PROTOCOL_VERSION` stays 3 and
`RECORD_VERSION` stays 2, on `PROTOCOL_VERSION`'s own rule for a
defaulted field on an existing variant whose absence means what its
default says, which is the condition D181 and D182 shipped under.

**And the hub excludes the pair, not the machine.** On such a refusal the
hub records `(job_id, runner)` and the matcher answers `ShortNow` for that
pair until the machine's next health report stops naming the job. The job
**waits** for the place it is already in, which is what the consumer
needed; every other job is placed on that machine as before; nothing about
any other machine changes. `ShortNow` and not `NotHere` because the place
is really taken by a job that is really running and will really come free,
which is the shape E-31's *short is never a failure* is about. The
clearing needs no new frame and no clock — health reports already name
every job a runner holds — and it is read off what the report **named**,
before any entry is judged: the entry about the written-off attempt is
refused as superseded, and it is also the only evidence the hub gets that
the place is still taken.

**No third budget, and the existing one is kept.** The analysis left the
choice to the brief and the brief ruled for no budget at all; the tree
refuted it. With the pair excluded, the ceiling budget is charged at most
once per episode rather than eight times in half a second, so it is no
longer what kills the job. Charging nothing would need a third kind of
ending on the record, and it would make
`a_job_refused_past_the_bound_ends_and_names_it` unreachable in one of its
two regimes: that test requires a job to reach the bound with
`ceiling_refusals_allowed: 1`, and its own comment records 13 failures in
20 under eight spinners in the regime where this branch is the refusal
that fires. An exemption with no bound remains the one outcome worse than
a job that ends, and this field's own bound is still that bound.

Deliberately not in this decision: whether a self-collision earns an
ending of its own on the record, which is a record-format addition and is
on the board; and anything about the consumer's instance, which is theirs
(invariant 9, D161).

The evidence is a fixture that needs no clock. The write-off is reached by
a frame — `ToHub::AttemptAbandoned` with no count enters the same
`give_the_job_another_go(..., TheRunnerWentAway)` the watchdog calls — and
the rate is bounded by `FromHub::HealthRecorded`, which the hub sends
after judging the queue, so *"the hub did not offer this job again"* is a
positive assertion over a bounded frame list. On `9e8fdef` it fails with 8
refusals, offers 2 through 9, nine attempts on the record and
`refused_as_full`; on the fix it passes in 0.32 s with exactly one
refusal and the job's own verdict.

### The `board.md` rows, in full

The four rows below are **already appended** to the board's *Open* section
on this branch, at the end, each written literally as `ORCH-NEXT` for the
merger to number. They are reproduced here because the merger reads the
tree and nothing else, and because a row renumbered in one place and not
the other is the failure this section exists to prevent. `main`'s highest
real row at the time of writing is ORCH-349.

Row 1 — the work itself, state `active`, owner `herd-step, then
herd-reviewer, then herd-merger`, next action a reviewer pass on this
branch because the replaced paragraph is a written design statement.

Row 2 — the budget ruling the tree refuted, state `open`, owner
`herd-planner`, next action to decide whether a self-collision gets an
ending of its own on the record.

Row 3 — ORCH-330 was not needed for this fixture, state `open`, owner
`herd-planner`, next action to re-size ORCH-330 against the two fixtures
said to need it that did not.

Row 4 — `confine.rs`'s "three sentences about room" is now four, state
`open`, owner `herd-step`, next action three words at whatever landing
next touches that file.

Their verbatim text is the last four rows of the *Open* table in
`docs/orchestration/board.md` on this branch, which is the copy the
merger numbers.

## Decisions needed

None for Alexa. One technical question for `herd-planner`, and it is board
row 2 rather than a question held open here: does a refusal for a second
attempt of one job earn an ending of its own on the record — a third
`HowItEnded`, a defaulted `Attempt` field, a `D-NEXT` — or does it go on
charging the bounded budget it charges today? **Recommendation:** leave it
as it is for now. The storm is what killed the job and the storm is
closed; the remaining charge is one per episode, and the alternative buys
a record-format addition the consumer's parser would see. **Fallback:** if
a fleet is ever measured reaching eight episodes on one job, the third
ending is the answer and the shape is already drawn in the analysis's T4.

## Resume point

Send a `herd-reviewer` at this branch, scoped to the replaced design
statement at `herd-proto/src/lib.rs` (`at_its_ceiling`'s doc) and to the
new field's doc beside it. Then the batch's landing is `herd-merger`'s,
with the four `ORCH-NEXT` rows to number and the `D-NEXT` entry above to
paste.

    git fetch origin && git log --oneline origin/self-collision-no-reoffer

## Processes

No long-lived process was started and nothing was killed: no PID of mine
outlived its command, and no `pkill` or kill by name was used at any
point. Every `cargo` invocation ran inside `nix develop` and one at a
time; the whole gate ran as a queued job through
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791`, with
`./scripts/bench-preflight.sh` read by its verdict text before each heavy
phase and answering *"ok, safe to start"* each time.

Build trees, both off the root filesystem. The known-bad tree
`/mnt/ssd_980/herd/build/t3t4-knownbad` and its throwaway worktree
`…/scratchpad/wt-knownbad` were removed as soon as the known-bad was
recorded. This task's own tree,
`/mnt/ssd_980/herd/build/t3t4-self-collision`, is deleted once the branch
is pushed; the path is named here so that whoever reads this can check it
is gone.
