# self-collision-no-reoffer — handoff 2026-11-01

- **Item:** ORCH-NEXT (first of the five rows this branch adds to the
  board's *Open* section; the merger allocates the numbers)
- **Agent:** herd-step
- **Branch / commit:** `self-collision-no-reoffer` / `3f37bd9` (the gated
  commit) plus this handoff commit, based on `main`
  `9e8fdefc0d5786d37cfdb9344494423eddfed951`
- **Reviewed:** `herd-reviewer`, 2026-11-01, on the earlier gated sha
  `e460f6f`. Accept-with-changes: six text changes and two corrections to
  the record, none of them behaviour. All are made on this branch and each
  is named below.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

T3 and T4 of the 2026-10-31 analysis, in one branch because T3 cannot be
correct without T4's field.

**T4 — the wire says which refusal it was.**
`herd_proto::ToHub::AttemptAbandoned` carries a second defaulted field,
`still_running_attempt: Option<u32>`, naming which attempt of this very
job the machine is still running when that is why it refused.

- `PROTOCOL_VERSION` stays **3** and `RECORD_VERSION` stays **2**, and the
  warrant is now stated correctly, which it was not in the first draft.
  The rule at `herd-proto/src/lib.rs:140`-`:142` admits a defaulted field
  *"when its absence means exactly what its default says"*. `None`
  therefore means **"this runner did not say which refusal it was"** — an
  absence of testimony. It does **not** mean "not a refusal for a second
  attempt of this job": that reading is false for an un-upgraded v1.0.0
  runner, which refuses exactly that way and sends no key, because the
  runner put both refusals in one match arm. D182's own field escaped
  cleanly because before it no abandonment carried any count at all, so
  its `None` was true of every frame ever sent; this one is not in that
  position. The decision is unchanged — a misread degrades to today's
  behaviour, nothing new breaks, and the consumer resets at v1.0.0 — but
  the ground is not the ground the first draft claimed.
- **So the upgrade order is runner first, and a hub deployed alone fixes
  nothing.** An upgraded hub beside un-upgraded runners receives no
  discriminant from any of them, excludes no pair, and offers the job back
  exactly as it does today. The reverse of D181, whose order was hub
  first. This is in the field's doc and in the drafted decision, and it
  was missing from both in the first draft.
- The in-tree assertion the brief named is green on the gated sha:
  `herd-hub/tests/announced_job_ceiling.rs` `neither_version_number_moves`
  pins the two literals 3 and 2. Nothing on the record changed at all: no
  new `Attempt` field, no new `JobEventKind`, no new `JobOutcome`.
- The runner fills it: `herd-runner/src/job.rs`'s refusal arm is split in
  two, and `Session::abandon_still_running_this_job` is the second of the
  two methods that build the frame. The count `at_its_ceiling` still
  travels, because it is still true — the machine is holding that many —
  and it is no longer the only thing the hub has.
- The wire shape is pinned as bytes in `herd-proto/tests/round_trip.rs`:
  three `to_frame` assertions (no key, the ceiling refusal's key alone,
  and both keys), one `from_frame` of an older runner's frame reading back
  as `still_running_attempt: None`, and a round trip of the populated
  frame.

**T4's prose — the design statement the measurement refuted.** The
paragraph at `herd-proto/src/lib.rs`, *"Both are the same fact … and both
carry the same consequence"*, is replaced. The replacement keeps the half
that holds — the fact is one, and neither refusal spends one of E-21's
goes — and states what differs, which is the **rate**, with the
mechanism and the field measurement. It now also says the two things a
reader could otherwise finish it without:

- **the refusal is not free.** It still carries `at_its_ceiling`, so the
  hub still charges `herd_hub::CEILING_REFUSALS_ALLOWED` — eight — and at
  the eighth the job still ends `RefusedAsFull` with no verdict. The
  budget is named rather than left as "this field's own bound", which was
  unparseable;
- **what is bounded is the rate per *connection*.** See *Risks* 1.

**T3 — the hub does not offer a job to a machine that just refused that
job.** On a refusal carrying `still_running_attempt`, the hub records the
`(job_id, runner)` pair on the connection and the matcher answers
`Admission::ShortNow` for that pair, so the job **waits** for the place it
is already in instead of being handed back at the speed of the queue.

- The pair lives on `ConnectedRunner::it_said_it_is_running_these`
  (`herd-hub/src/server.rs`), beside `it_said_it_is_holding` and with the
  same lifetime: it dies with the connection.
- It is cleared by the machine's own next health report, in
  `State::record_health`, which now also returns true when a pair cleared
  so the queue is judged on that same report. **The clearing reads what
  the report *named*, not what survived being judged** — and that
  distinction is load-bearing: `mind_a_reported_job` refuses the entry
  about the written-off attempt as `Superseded`, and that refused entry is
  the only evidence the hub gets that the place is still taken. The
  reviewer attacked this four ways and could not break it; its second
  attack is the most interesting thing it found, and it is in *Risks* 3.
- No new frame, no new clock, no third budget. Both hooks the brief named
  were already there and both were used.

**Whose clause is whose.** `herd_runner::A_RUNNERS_OWN_SECOND_ATTEMPT` =
`"one machine runs one job once"` is the runner's clause for this refusal,
interpolated into the sentence rather than repeated beside it. The first
draft also put those words verbatim into the **hub's** `ShortNow`
sentence, which broke the existing discipline in the other direction: a
line the hub writes while a job waits costs that job nothing, and the
runner's was written inside an `attempt_ended` that spent one of its
budgets, so a reader matching the words could no longer tell the two
states apart. The hub now has a clause of its own,
`herd_hub::matcher::A_SECOND_ATTEMPT_OF_ITS_OWN` =
`"already running another attempt of"`, and
`the_runners_ceiling_sentence_is_not_the_hubs` runs **five** clauses and
both of this branch's new sentences through the disjointness check. What
no gate can assert is the link across the crates — `herd-runner` is only
a `[dev-dependencies]` of `herd-hub`, so a test binary is the only place
both constants exist at once — and both const docs now say that instead of
claiming the opposite.

**The known-bad, recorded on `9e8fdef` before anything was changed.** The
fixture cannot compile at that commit — it names `still_running_attempt`,
`A_RUNNERS_OWN_SECOND_ATTEMPT` and `testing::a_refusal_for_a_second_attempt`,
none of which exist there — so what was run, in a throwaway worktree
detached at `9e8fdefc0d5786d37cfdb9344494423eddfed951`, was the
**equivalent** fixture sending the frame an un-upgraded v1.0.0 runner
really sends: `at_its_ceiling: Some(1)` and no discriminant. It
reproduces the consumer's record figure for figure:

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
and its build tree were removed. The two-part oracle also fails there at
its first half, with the offer named:

    the machine said it is still running this job, so the hub must offer
    it no further attempt of it: [AssignJob { job_id:
    JobId("j0000000001"), attempt: 3, ... }]

The fixture's own doc comment now says exactly that, rather than
claiming a result at a commit where it cannot build. **There is no
artefact in this tree that is red without the hub change**: T4's field is
a compile-time dependency of the fixture, so "T4 without T3" cannot be
run from the tree.

**The test, green on the fix and needing no clock.**
`herd-hub/tests/one_machine_runs_one_job_once.rs`, one test, **0.23 s**,
no sleep, no cut socket, no `tokio::time::pause`, no `Policy` seam. Both
halves of the analysis §4.5 oracle: the job does not reach a terminal
state and in particular not the `refused_as_full` **outcome** while the
machine still names it, and exactly **one** refusal carries the clause
where the first consumer's record carries eight. Plus a matcher unit
test, `a_job_a_machine_is_already_running_is_not_offered_to_it_again`,
which asserts both halves on one seat: holding 1 of 2 is admissible, and
the one excluded job is not, and the sentence carries the hub's clause
and not the runner's.

**Two fixture figures that did not come out now do** — both were inert
and both were wrong on their face, which is worse in a paragraph about
what the figures demonstrate. `testing::a_refusal_for_a_second_attempt`
lent 14 GiB over a 6 GiB floor at a 7 GiB cap, which is **one** place by
`(lent − floor) / per_job`, while `most: 2` and its own doc said two — so
it exhibited the opposite of the property that doc calls "not a detail of
the example", since holding 1 of 1 is *at* the ceiling where this branch
is unreachable. It now lends 20 GiB, which really is two places, with a
`debug_assert_eq!` on `ceiling.most()` so the arithmetic cannot drift
from the prose again. And the fixture's `LENT_MIB` was `14 * 1024` under
a paragraph saying 20 lent, 6 kept, 14 usable: the constant is named for
the lent figure and held the usable one. It is now `20 * 1024`.

**And `a_refusal_for_the_memory_sum` has its doc block back.** The new
function's `///` block went in with no blank line after it, so one
contiguous block documented the new function while opening with the
memory-sum summary and its host figures, and the older function was left
undocumented. Nothing in the tree catches that: there is no `missing_docs`
lint anywhere.

## Not done, and why

- **The brief's budget ruling — "no third budget, and no budget at all
  for this case" — is half done, deliberately.** No third budget was
  added. The existing ceiling budget is still charged for this refusal,
  which the exclusion reduces from eight charges in half a second to **at
  most one per connection**. It is *not* once per outage and the job can
  still die: see *Risks* 1. Two reasons for stopping there, one of scope
  and one the tree gave me, in *Risks* 1 and 2 and in board row 2.
- **`herd-runner/src/confine.rs`'s "three sentences about room" is now
  more than three and was left unedited**: `confine.rs` is outside the
  brief's write paths, and the disjointness claim those paragraphs make is
  still true — it is the count in the prose that is stale. Board row 5.
- **The four places that still assert the retired claim are not touched**,
  including the sentence a requester really reads at
  `herd-hub/src/server.rs:8789`-`:8794`, which tells an operator to *"add
  capacity"* for a refusal that is not about capacity. All four pre-date
  this branch, none is in its diff, and the sentence cannot be fixed
  without the record change board row 2 owns. Board row 3 says *read row
  2*, which is the right action rather than a new proposal.
- **No `docs/decisions.md` write**, no `docs/status.md`, no
  `docs/coverage.md`, no `CLAUDE.md`: out of scope, and the drafted
  decision entry is below in full.
- **No merge, no tag, no pull request, no push to `main`.**

## Gates

All four Rust gates plus the docs checks, run **whole and bare through
the queue** on the gated commit `3f37bd9`, never per step. Verbatim
verdict:

    gate: GREEN sha=3f37bd9712ba86fd038d658e3eeb327977abb01c steps=0-8:ok
    host=servernixos load1=7.57 load1-end=6.30
    skips=under-load(flood-fifty=10.21>8,flood-two-fifty=10.21>8)

Receipt, on the gated tree:

    verdict: GREEN
    sha: 3f37bd9712ba86fd038d658e3eeb327977abb01c
    branch: self-collision-no-reoffer
    host: servernixos
    nproc: 12
    load1-start: 7.57
    load1-end: 6.30
    started: 2026-11-01T03:26:10Z
    finished: 2026-11-01T03:34:46Z
    load-excused-skips: flood-fifty=10.21>8,flood-two-fifty=10.21>8
    skips-observed: 15
    skip-list-sha256: c9dd3040103efcedea68c62392b44e653304b933528293d088
                      6f69b29a36222f
    ignored-observed: 38
    ignore-list-sha256: f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c
                        4744d2aed7a46545
    steps: 0..8 all ok

Step 6, the suite, took 414 s and printed 1307 passing test lines. Steps
1 and 2, the docs checks, were part of this same run and were ok.

**The skips, as a list and not a count.** 15, which is the at-rest 13 plus
the two load-gated flood benches, each of which read the load itself and
declined: `flood-fifty` and `flood-two-fifty`, both at 10.21 against
their own ceiling of 8. `skip-list-sha256`
`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f` is the
documented 15-skip value, and `ignore-list-sha256`
`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545` over
**38** is the documented at-rest value. **Both shas are unmoved, on both
of this branch's gate runs: nothing here adds a `HERD-SKIPPED` line or an
`#[ignore]`.** No `HERD-SKIPPED` line was copied as a pass.

**The earlier gate.** The pre-review tree, gated commit `e460f6f`, was
also GREEN, with the same two shas and the same two load-excused benches:

    gate: GREEN sha=e460f6f997f83113c3432671dd1ab56b4b05d568 steps=0-8:ok
    host=servernixos load1=6.13 load1-end=9.00
    skips=under-load(flood-fifty=11.09>8,flood-two-fifty=11.09>8)

Its receipt reported `skips-observed: 15`, `skip-list-sha256
c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f`,
`ignored-observed: 38`, `ignore-list-sha256
f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545`, and
steps 0..8 all ok, step 6 at 431 s. It is quoted because the review was
performed against it and because the amendments after it are text only.

**The reviewer ran `./scripts/check-docs.sh` only** — `docs ok` at both
`e460f6f` and `39c6d5e` — and states that the four Rust gates were **not
run** by it. Nothing in its file should be read as saying they passed.

**The last run**, made after this file was committed and therefore not
quotable here: `./scripts/check-docs.sh`, from the worktree root, output
to the terminal and reproduced verbatim in this agent's report.

## Risks and findings

1. **"At most one per episode" was wrong, and the corrected figure still
   leaves the job killable.** The reviewer's F1: `peers.runners.insert`
   gives every redial a fresh empty `it_said_it_is_running_these`
   (`herd-hub/src/server.rs:2294`), and `dispatch_queued_jobs` runs
   **before** `relay_from_runner` (`:4070`-`:4071`), so the hub offers on
   the hello, before reading a frame. Each redial therefore costs exactly
   one more self-collision refusal and one more `ceilings_hit`, and
   **eight redials during one long attempt still end the job
   `refused_as_full`, exit 125, with no verdict** — the same death, eight
   link flaps instead of 0.51 s. The incident this branch is about was a
   71.28 s silence at host load 172, so redials are not hypothetical on
   that fleet. The branch is a strict improvement with no regression — the
   behaviour on one redial is exactly today's, once — but *per connection,
   re-armed by a redial* is the honest unit, and it is now what the
   protocol doc, the board rows and the drafted decision all say. The
   sizing plan was more careful than my first draft: its T3 evidence line
   said "per health cadence".
2. **The budget ruling's refutation is right on evidence weaker than I
   stated, and the weaker evidence is worse news.** I wrote that with
   `ceiling_refusals_allowed: 1` "this branch is the only refusal the
   fixture produces". Too strong: the test's own comment at
   `announced_job_ceiling.rs:1645`-`:1655` records *"green 35 of 35
   unconstrained and 20 of 20 pinned to two idle CPUs, then 4 failures in
   20 … and 13 in 20 under eight"*. On an **idle** host the fixture
   produces the *ceiling* refusal, which a no-charge rule still charges,
   so `!refused.is_empty()` stays reachable there; under load it is this
   branch that fires. So charging nothing would be a **CI-only,
   load-only red** — three red runs on a two-core `ubuntu-latest` is the
   shape it took before — which a local gate on this twelve-core box would
   not have caught. That makes keeping the charge a *stronger* call than I
   claimed, not a weaker one.
3. **And the other half of it is scope.** Charging neither budget needs
   the record to distinguish a third kind of ending — a `HowItEnded`
   variant and an `Attempt` field in `herd-hub/src/job.rs` and
   `herd-hub/src/store.rs`, neither in the brief's write paths, and a
   record-format addition owing a `D-NEXT`. Recording a self-collision as
   a ceiling refusal *without* charging it would be worse than either:
   `JobRecord::goes_a_ceiling_refused` (`job.rs:1944`) filters on the
   **record** — `attempt.at_its_ceiling.is_some()` — so uncharged
   refusals would still inflate `ceilings_hit` and a later genuine
   ceiling refusal would find its budget short. The reviewer confirmed
   this one as written (its N2).
4. **The pair cannot fail to clear, and the bound I named for it was the
   wrong one.** The exclusion lives on the connection, so it cannot
   outlive it; a runner that stops reporting is removed from the registry
   at `lost_after` (`server.rs:8425`-`:8429`) and the set goes with it. So
   there is no "waits for ever". But I wrote that E-21's goes are what
   bound the wait, and they are not: **a queued job has no attempt
   watchdog**. What bounds it is invariant 2's per-job time cap on the
   attempt that is actually running, plus the liveness reap. Both exist,
   so the conclusion stands on a different leg than the one I gave it.
5. **The reviewer's second attack is the best thing it found, and it
   passed.** It expected `Session::drop_stale` to remove the entry on
   `StaleAttempt` while the process lived — which would have let the hub
   offer attempt 3 beside a live attempt 1 and broken invariant 2
   *through the hub's own clearing rule*. It does not:
   `herd-runner/src/lib.rs:3263`-`:3268` leaves a still-running job in
   `inner.jobs` for the task to notice, so `running_jobs` keeps naming it
   until the place is really free. The clearing rule tracks the place and
   not a bookkeeping entry. That is luck I did not earn by design, and it
   is worth knowing it is load-bearing.
6. **A throughput cost, named.** A place that frees is noticed at the next
   health report and not sooner, because the report is the only clearing
   hook. That is the same cost `it_said_it_is_holding` already pays, for
   the reason `record_health`'s own comment gives, and it is bounded by
   `HEALTH_EVERY` = 15 s. Measured where it could have bitten:
   `a_link_cut_while_the_runner_is_full_loses_no_job` runs at a 1 s
   cadence inside 120 s of patience and is green.
7. **The exclusion is not an attack surface.**
   `the_runner_says_it_is_running` is keyed on the authenticated
   connection's name and the arm is gated by `is_current` →
   `State::standing_of(job_id, attempt, from)`, which refuses somebody
   else's attempt by name. So a buggy or hostile runner can exclude only
   jobs it was itself just assigned, only from itself, and only until its
   own next report. No cross-runner and no cross-job reach. (Reviewer's,
   checked independently of me.)
8. **Two of the brief's write paths were short, and I treated the brief's
   list as the concern rather than the file list.** The field cannot be
   put on the frame without `herd-runner/src/lib.rs` (the `Session`
   methods that build it, and the sentence), and the attribution
   assertion cannot read a real sentence without
   `herd-runner/src/testing.rs`. Both are the same change seen from one
   crate further in. Also touched, to compile: `herd-hub/tests/link.rs`
   and `herd-proto/tests/round_trip.rs`, one field each.
9. **One assertion I wrote was vacuous and the known-bad run caught it.**
   `refused_as_full` is an **outcome** under a `finished` state, not a
   state, so `file_state_of(...) != "refused_as_full"` could never fail.
   It now reads the outcome's own `type`. Worth saying because the same
   mistake in a test watching only the state would read the consumer's
   dead job as a job that ended well. The reviewer swept the whole tree
   for the same shape with a detector proven on three known-bad fixtures
   first and found **0** code hits on either tree, and one prose hit in
   `docs/decisions.md`, which is append-only.
10. **Not re-derived and not re-checked:** T1 (D291) and T2 (D292) have
    landed and this branch does not touch them. I did not measure the
    consumer's instance or anything on it (invariant 9, D161); every
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
runner.

**What `None` means, exactly, because the obvious reading is false.**
`None` is *"this runner did not say which refusal it was"* — an absence of
testimony. It is not *"not a refusal for a second attempt of this job"*:
an un-upgraded v1.0.0 runner refuses exactly that way and sends no key,
because the runner put both refusals in one match arm. D182's own field
escaped this cleanly, since before it no abandonment carried any count at
all. Both version numbers stay where they are all the same —
`PROTOCOL_VERSION` 3, `RECORD_VERSION` 2 — because the absence does mean
what the default says once the default is stated as silence, and because
what a hub does with that silence is the behaviour it had before the
field existed.

**What it does cost is named, not softened: the upgrade order is runner
first, and a hub deployed alone fixes nothing.** An upgraded hub beside
un-upgraded runners receives no discriminant, excludes no pair, and
offers the job back exactly as it does today — so until every runner that
might refuse this way is rebuilt, the storm is still reachable on that
fleet. The reverse of D181, whose order was hub first.

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

**What is bounded is the rate, and it is bounded per connection.** The
exclusion lives on the connection, so a redial arrives with an empty set
and the hub offers on the hello, before it reads a frame. Each redial
costs one more refusal and one more charge, so eight redials during one
long attempt still reach the bound and still end the job
`refused_as_full`. What is closed is the 0.51 s storm, not every road to
that outcome.

**No third budget, and the existing one is kept.** The analysis left the
choice to the brief and the brief ruled for no budget at all; the tree
refuted it twice. Charging nothing would need a third kind of ending on
the record, and — measured, not argued — it would turn
`a_job_refused_past_the_bound_ends_and_names_it` red on a loaded host
only: that test requires a job to reach the bound with
`ceiling_refusals_allowed: 1`, and its own comment records 4 failures in
20 under four spinners and 13 in 20 under eight in the regime where this
branch is the refusal that fires, while on an idle host the ceiling
refusal fires instead and would still be charged. A CI-only, load-only
red is the one a local gate does not catch. An exemption with no bound
remains the one outcome worse than a job that ends.

Deliberately not in this decision: whether a self-collision earns an
ending of its own on the record, which is a record-format addition and is
on the board; the four places in `herd-hub/src/job.rs` and
`herd-hub/src/server.rs` whose prose still asserts the merged claim,
which cannot be fixed without that addition and is on the board beside
it; and anything about the consumer's instance, which is theirs
(invariant 9, D161).

The evidence is a fixture that needs no clock. The write-off is reached by
a frame — `ToHub::AttemptAbandoned` with no count enters the same
`give_the_job_another_go(..., TheRunnerWentAway)` the watchdog calls, so
the ending, the budget charged and every record field match and only the
`ended` sentence differs — and the rate is bounded by
`FromHub::HealthRecorded`, which the hub sends after judging the queue, so
*"the hub did not offer this job again"* is a positive assertion over a
bounded frame list. The fixture cannot compile on `9e8fdef`, since the
field is a compile-time dependency of it; the equivalent fixture there,
sending the frame a v1.0.0 runner really sends, failed with 8 refusals,
offers 2 through 9, nine attempts on the record and `refused_as_full`. On
the fix it passes in 0.23 s with exactly one refusal and the job's own
verdict.

### The `board.md` rows, in full

The five rows below are **already appended** to the board's *Open* section
on this branch, at the end, each written literally as `ORCH-NEXT` for the
merger to number. They are reproduced here because the merger reads the
tree and nothing else, and because a row renumbered in one place and not
the other is the failure this section exists to prevent. `main`'s highest
real row at the time of writing is ORCH-349.

Row 1 — the work itself, state `active`, owner `herd-step, then
herd-merger`, next action integration: the reviewer pass is done and its
six changes are made.

Row 2 — the budget ruling the tree refuted, state `open`, owner
`herd-planner`, next action to decide whether a self-collision gets an
ending of its own on the record.

Row 3 — the four places that still assert the retired claim, state
`open`, owner `herd-planner`, next action **read row 2 and D182 first**,
because the sentence cannot be fixed without that row's record change.

Row 4 — ORCH-330 was not needed for this fixture, state `open`, owner
`herd-planner`, next action to re-size ORCH-330 against the two fixtures
said to need it that did not.

Row 5 — `confine.rs`'s "three sentences about room" is now more, and the
cross-crate clause link is asserted by nothing, state `open`, owner
`herd-step`.

Their verbatim text is the last five rows of the *Open* table in
`docs/orchestration/board.md` on this branch, which is the copy the
merger numbers.

## Decisions needed

None for Alexa. One technical question for `herd-planner`, and it is board
row 2 rather than a question held open here: does a refusal for a second
attempt of one job earn an ending of its own on the record — a third
`HowItEnded`, a defaulted `Attempt` field, a `D-NEXT` — or does it go on
charging the bounded budget it charges today? **Recommendation:** leave it
as it is for now. The 0.51 s storm is closed, which is what killed the
job; the remaining charge is one per connection. **Fallback:** if a fleet
is ever measured reaching eight **redials** during one attempt of one job
— which is the unit, and not eight outages — the third ending is the
answer and the shape is already drawn in the analysis's T4. Board row 3
would be fixed by the same change.

## Resume point

The reviewer pass is done and nothing in it blocks: six text changes, all
made on this branch, none behavioural. So the next step is the batch's
landing, which is `herd-merger`'s: five `ORCH-NEXT` rows to number and one
`D-NEXT` entry to paste.

    git fetch origin && git log --oneline origin/self-collision-no-reoffer

## Processes

No long-lived process was started and nothing was killed: no PID of mine
outlived its command, and no `pkill` or kill by name was used at any
point. One background shell of my own — a poll that waited for
`./scripts/bench-preflight.sh` to stop refusing, after it refused at load
11.18 — ran to its own exit and is reported in this agent's report with
its id. Every `cargo` invocation ran inside `nix develop` and one at a
time; both gates ran as queued jobs through
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791`, with the
preflight's **verdict text** read before each heavy phase and the phase
not started while that text said REFUSED.

Build trees, both off the root filesystem. The known-bad tree
`/mnt/ssd_980/herd/build/t3t4-knownbad` and its throwaway worktree
`…/scratchpad/wt-knownbad` were removed as soon as the known-bad was
recorded. This task's own tree,
`/mnt/ssd_980/herd/build/t3t4-self-collision`, is deleted once the branch
is pushed; the path is named here so that whoever reads this can check it
is gone.
