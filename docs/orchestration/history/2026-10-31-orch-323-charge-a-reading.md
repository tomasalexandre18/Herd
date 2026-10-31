# orch-323-charge-a-reading — handoff 2026-10-31

- **Item:** ORCH-323
- **Agent:** herd-step
- **Branch / commit:** `orch-323-charge-a-reading` / this commit, based on
  `origin/main` `9f76b548678208592be6e58b01643a5729ab188b`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

D285's option (b), implemented: **the hub charges a figureless
requirement reading exactly what it charges every other figureless job**,
and the count bound comes off the hub's own records.

**The one sentence.** A reading reaches a runner with no `JobNeed` on it,
which is what `herdctl submit -- true` reaches it with, and the runner
cannot tell the two apart: it grants both the whole of
`--job-memory-max`. So the hub charges both `C_hub`, the charge is never
below the grant for any job, the memory sum refuses exactly the sets the
runner refuses, and the announced count is no longer needed as a second
bound over what the records say is out there.

**`herd-hub/src/matcher.rs` — the charge.**

- `charged_against_the_room` lost its `a_reading: bool` argument and its
  `None if a_reading => 0` arm. It is now told a declaration and a room
  and nothing else, so a reading is not a case in that function any more
  than it is a case on the wire. Both callers — `reserved` and
  `admission` — pass one argument fewer; `JobRecord::is_a_reading` is
  untouched and still used everywhere else.
- A figureless job of either kind is charged
  `herd_proto::charged_ceiling_mib(None, room.per_job_cap_mib)`, which is
  `C_hub = (lent − floor) ÷ jobs_at_once`, floored at
  `MINIMUM_CEILING_MIB`. On a runner that announced no count it is
  charged nothing, exactly as before: a figure nobody stated is not
  invented (D23's rule).

**`herd-hub/src/matcher.rs` — the bound.** `admission` no longer holds
the announced count against `seat.reserved.jobs`. It holds it in **two**
cases, and both are the runner's own shape since ORCH-325
(`herd-runner/src/lib.rs`, its `match room.admissible_mib`):

1. **A host that will not say how much memory it lends.** There is no sum
   on either side there — the hub charges a figureless job nothing
   because there is no cap to charge it with, and the runner announces
   `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY`, which is one. The count is
   the whole of that machine's bound on both sides. Dropping it here
   would have left nothing in the hub bounding such a machine at all,
   which is the defect D181 closed. New test:
   `a_host_that_will_not_say_how_big_it_is_is_bounded_by_its_count_alone`.
2. **A machine that has told this hub it is holding more than the records
   know of** (D182, `Seat::it_said_it_is_holding`). That figure is a
   count of jobs and nothing says what each of them was granted, so it
   cannot be added to a sum in mebibytes. Held against the ceiling the
   machine announced, so the surplus waits instead of being offered
   straight back to a full machine after an outage — the sequence D182
   measured. New test:
   `a_machine_that_said_it_is_full_is_held_to_its_count_after_an_outage`.

   In the ordinary state that figure is `None`, so on a machine that
   stated its size this bound is inert and the sum is what answers.

A runner that announces **zero** is still `NotHere` and not a wait,
unchanged: a machine with no place has no job that could end there to
free one.

**What this delivers, measured on the matcher's own fixture.** On
`build-host` — 31 GiB lent, a 6 GiB floor, 25 GiB admitted — that
announced **two** places: **22 jobs** declaring 1 GiB each are now
placed, where **two** were placed before and 23 GiB of what that machine
lends went unused. Each is held to 1126 MiB (its declaration plus the
10 % margin), far under the 12.5 GiB its count implies, and the 23rd is
refused by the sum with 828 MiB free. A **figureless** queue still fills
to exactly `at_once` jobs and no further, because
`at_once × C_hub = lent − floor`: three on a seat announcing three,
eight on a seat announcing eight, two on this file's capped runner.
That is why charging the reading is what made dropping the count safe
rather than generous.

**`PROTOCOL_VERSION` stays 3.** No frame, no field, no changed field
meaning, no `JobEventKind` variant. `READINGS_AT_ONCE` stays **2** and
no figure was invented for it: it bounds how many readings one machine
runs at once (D169), which is a statement about their kind and not about
memory. `RECORD_VERSION` and `LAYOUT_VERSION` untouched.

**The test the brief named, answered by renaming and not by deleting.**
`herd-hub/tests/packed_by_declaration.rs`'s
`a_reading_is_charged_nothing_and_the_announced_count_is_what_bounds_it`
said in as many words that *"if the count bound is ever dropped in favour
of the sum alone, this test is where it has to be answered"*. It is now
`a_reading_is_charged_the_per_job_cap_like_every_other_figureless_job`,
it says what it recorded and what it records now, and it asserts the
charge is never below the grant for the one job the hub cannot
distinguish, over 180 of the grid's triples.

**The property test and its control, both kept.**
`the_hubs_charge_is_never_below_the_runners_grant` is green and
`the_charge_this_hub_made_before_is_caught_by_that_property` still fails
on the pre-ORCH-312 charge, naming the three under-charges. The
consequence test, `no_set_the_hub_admits_is_a_set_the_runner_cannot_hold`,
**lost its `admitted < at_once` term**: that term modelled a bound the
hub no longer applies, and a walk that stopped at the count was answering
the claim only over the sets the count let through. It now walks until
the room is full, which is further on every queue of jobs charged under
the cap, and it is green. The stated limit of the property is unchanged
and unwidened: it is walked over the triples where the runner's division
did not hit its `.max(1)` floor, and that branch keeps its own test.

**Four matcher unit tests renamed, because their names said the count was
the bound:**

| was | is |
|---|---|
| `a_runner_at_the_ceiling_it_announced_takes_nothing_more_and_says_so` | `a_runner_fills_to_the_room_its_announced_count_divides_and_says_so` |
| `the_ceiling_and_the_memory_admission_are_two_bounds_and_either_may_bite` | `the_memory_sum_is_the_bound_and_a_job_under_the_cap_packs_past_the_count` |
| `the_ceiling_counts_readings_too_because_the_runner_s_does` | `a_reading_is_charged_the_per_job_cap_because_the_runner_grants_it_one` |
| `a_job_held_by_a_ceiling_waits_for_that_machine_and_then_moves` | `a_job_held_by_a_full_machine_waits_for_that_machine_and_then_moves` |

Every one of them asserts the figures of the sentence it now reads, and
the reading test also asserts that the count is still broken down —
*"of which 2 are requirement readings"* (D159).

**`herd-hub/tests/announced_job_ceiling.rs`.** Alexa's oracle is
untouched and still green: attempts spent for the runner's ceiling **0**,
every other attempt unchanged, peak attempts out on the runner exactly
`CEILING`. What changed is **which sentence the surplus reads**: the
hub's memory sentence instead of its count sentence, asserted with the
figures built from that file's own three constants rather than written
out. And the file now asserts the **negative**: no `waiting` line of that
run carries `A_CEILING_THE_RUNNER_ANNOUNCED`, because that machine said
how much memory it lends.

**Doc comments that stated the old charge**, all corrected in place:
`Seat::jobs_at_once`, `READINGS_AT_ONCE`, `charged_against_the_room`,
`A_CEILING_THE_RUNNER_ANNOUNCED` (where it is written now), `admission`'s
first bound, and `herd-hub/src/main.rs`'s `--readings-at-once` help,
which said this flag was *the only thing* that bounds how many readings
one machine is given.

**Also in scope, one word:** `herd-runner/src/cgroup.rs:2532` said *"the
twelve standing skips"* where `scripts/expected-skips/local.txt` holds
**thirteen** (13 `HERD-SKIPPED` lines, counted). Now thirteen. No
behaviour, no other stale count touched — `scripts/`, `docs/` and
`README` outside the two files below are another agent's territory.

**`docs/status.md`** said *"a reading weighs no memory (**ORCH-126**)"*,
which this change makes false. Replaced in place by *"no frame marks a
reading (**D285**)"*, which is true, is state and not a reason, and
points at the decision that refused the frame field. **Still exactly 90
lines**, nothing appended.

**`docs/orchestration/board.md`:** ORCH-323 is `done` with what was done
and what was kept; two rows added, **ORCH-331** and **ORCH-332** (both in
*Open*, full text below).

## Not done, and why

- **`docs/decisions.md` untouched**, as the brief directs: only
  `herd-merger` appends. The entry is drafted below, headed `D-NEXT`.
- **Nothing outside the hub's charge.** The runner is untouched but for
  one word of a doc comment: its half of this arithmetic landed with
  ORCH-325 and needed nothing here.
- **`docs/remediation/09-runner-self-limits.md` not corrected.** It names
  all four renamed tests. `docs/` was not this task's to write, and the
  precedent (D286) is that a dated record is answered by a new entry
  rather than edited. Reported as **ORCH-332**.
- **No measurement on the consumer's machines** (D161). The 20 GiB /
  6 GiB / 7 GiB figures in ORCH-331 are from their own report, read off
  ORCH-323's row, and nothing here was measured against their queue.

## Gates

**Before this commit**, in the order the brief set, in this agent's own
worktree under its scratchpad directory (`…/scratchpad/orch-323-wt`, the
absolute path is in the agent's report):

```text
$ git status --short
 M herd-hub/src/main.rs
 M herd-hub/src/matcher.rs
 M herd-hub/tests/announced_job_ceiling.rs
 M herd-hub/tests/packed_by_declaration.rs
 M herd-runner/src/cgroup.rs

$ nix develop --command cargo fmt --all --check
warning: Git tree '…/orch-323-wt' is dirty
(no diff; exit 0 — the warning is nix's, about the uncommitted tree)

$ export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-323
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Checking herd-proto v1.0.0 (…/herd-proto)
    Checking herd-runner v1.0.0 (…/herd-runner)
    Checking herdctl v1.0.0 (…/herdctl)
    Checking herd-hub v1.0.0 (…/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.21s
clippy exit=0

$ ./scripts/check-docs.sh
docs ok
```

`./scripts/bench-preflight.sh` was read before every heavy phase, verdict
and status from one invocation, chained with `&&`. It **REFUSED twice** —
once on load 12.23 of a 9.00 ceiling, once on 7316 MiB of slice room
against an 8192 MiB floor — and both times the chain stopped and nothing
was started. `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set.

**The last run, named and not quoted here**, because it is made on this
committed tree and nothing may be added to the tree after it:

```text
/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run \
  --label build --memory-mib 4700 -- /bin/sh -c "cd <this worktree> && \
  export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-323 && \
  ./scripts/gate.sh"
```

Its output goes to the agent's report, and `gate.sh` writes its own
receipt for the sha it ran on at `$HERD_GATE_RECEIPTS/<sha>.receipt`,
which is `<this worktree>/target/herd-gate/<sha>.receipt` here. The skip
list is compared **as a list and never as a count** (D187).

**Skips:** none introduced and none removed by this change. The suite's
standing skips are the host's: **13** names at rest, sha256
`67ae71ed344e1ed18cb6c1c75002aabcc15e77de010c0601d1b89a81c1b20489`;
**15** under load, with `flood-fifty` and `flood-two-fifty` excused by
name, sha256
`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f`.
Whichever the receipt carries, it is reported as a skip and never as a
pass (D27).

## Risks and findings

1. **The count bound could not be dropped outright, and the brief's own
   safety argument is why.** Two cases are left where the memory sum
   cannot carry it, and each is a real machine:

   - **A silent host.** `herd_runner::jobs_at_once` answers
     `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY` = 1 for a host with no
     `--host-memory`, and the hub charges a figureless job on such a seat
     **nothing** (`charged_ceiling_mib(None, None)` is 0, deliberately:
     *"the one thing this must not become is a hub that invents a cost
     for a job on a runner that announced nothing"*). With the count
     gone, the hub would offer such a machine an unbounded queue of
     figureless jobs and the runner — which bounds itself by its count
     there, `herd-runner/src/lib.rs`'s `None` arm — would refuse all but
     one. That is D181's refusal storm, reached from the other side.
   - **D182's correction.** Its §3 is explicit: *"`admission` holds the
     ceiling against the larger of `seat.reserved.jobs` and that
     figure, so the surplus **waits** instead of being offered straight
     back"*. That figure is a count and the sum is mebibytes, so the one
     cannot be expressed in the other. Dropped, every outage would end
     with the hub re-offering a full machine its own jobs.

   **This is not the outcome the brief said would invalidate the
   decision**: the hub never admits a set a runner refuses, because the
   charge is never below the grant over the same set, and the runner's
   own bound on a host that states its size is the same sum. It is the
   condition under which dropping the count is safe, and it is the
   runner's own shape, so the two sides still read alike. Both cases have
   a test, so neither can be dropped by a later edit without a red gate.

2. **A reading is no longer free of the memory admission, and on a
   two-place runner it waits behind the work.** This is the stated cost
   of option (b) and it is now a row: **ORCH-331**, to report with no
   question attached (CLAUDE.md, outcome 3). Nothing is refused and no
   attempt is spent — a reading over the room *waits* (E-31) — so what it
   costs is discovery latency on a busy one-runner fleet. The honest
   mitigations are a larger `--job-memory-max` (fewer, bigger places) or
   marking a reading on the frame, which is protocol surface Alexa would
   have to want; **exempting a reading from the charge again is not one**,
   it is the gap this task closed.

3. **`docs/status.md` cited the wrong row for the clause this change
   falsified.** *"a reading weighs no memory (**ORCH-126**)"* — ORCH-126
   is *"what a runner lends is read once at startup and never revised"*,
   about `herd-runner/src/room.rs`, and has nothing to do with readings.
   The citation is gone with the clause; nobody needs to act, but the
   next reader of that line should know it was never the right pointer.

4. **Four test names in a dated remediation record are now stale**
   (**ORCH-332**, with the author's recommendation: leave it, and let the
   row be the correction).

5. **Where the charge is still below the grant, unchanged and still
   safe.** On the `.max(1)` branch — a host that lends less after its
   floor than it caps a job at — `C_hub` is the whole room, so a job the
   hub under-charges there is a job charged the whole room, admitted
   **alone**, and the set's grant is one cap. That is the most either
   side ever permitted and it is what that machine permitted before any
   of this. `a_host_too_small_for_one_ceiling_admits_one_job_on_both_sides`
   now asserts the charge as well as the cap, and the property is still
   stated only over the triples where the division did not hit that
   floor.

6. **What was not re-derived.** A runner that announced a host size but
   **no** count still has figureless jobs charged nothing, and nothing in
   the hub bounds them — today's behaviour, untouched by this change, and
   unreachable from our own runner, which always announces a count
   floored at one. The test that holds it is
   `a_runner_that_announced_no_ceiling_is_placed_on_exactly_as_it_was`,
   green and unedited.

## Drafted for `herd-merger`

### `docs/decisions.md` — the full entry, headed `D-NEXT`

```markdown
## 2026-10-31 — D-NEXT The hub charges a reading, and the count stops
being the bound

ORCH-323, and the second half of D285. D285 ruled option **(b)** — a
figureless requirement reading is charged `C_hub`, exactly as every
other figureless job already is — and refused option (a), a frame field
marking a reading, as protocol surface that buys nothing (b) does not.
This is that ruling implemented. `PROTOCOL_VERSION` stays **3**: no
frame, no field, no changed field meaning.

**The charge.** `herd_hub::matcher::charged_against_the_room` lost its
`a_reading` argument and its `None if a_reading => 0` arm. A job with no
`JobNeed` is charged `C_hub = (lent − floor) ÷ jobs_at_once`, floored at
`MINIMUM_CEILING_MIB`, whether the hub minted it as a reading or a
requester submitted `herdctl submit -- true`. **Because the runner
cannot tell them apart**: `reading_job` gives a reading no figure (D159,
D169, deliberately — nought is not a figure), `job_ceiling` grants every
job that arrives with no `JobNeed` the whole of `--job-memory-max`, and
nothing on the frame says which jobs are readings. A hub charging one of
the two less than the other charges a figure its runner does not honour.

**The bound.** `admission` no longer holds `jobs_at_once` against
`seat.reserved.jobs`. With one charge over one set on both sides,
`Σ charged ≤ lent − floor` refuses exactly the sets
`herd_runner::take_job` refuses, so a count beside it could only bite
earlier than either side needs — `at_once × C_hub ≤ lent − floor`, so it
bit first on every job held to less than the cap. **That is the packing
D285 said this landing did not deliver**: 22 jobs declaring 1 GiB now
fit on a seat admitting 25 GiB that announced two places, where two fit
before and 23 GiB of what it lends went unused. A figureless queue still
fills to exactly `at_once` jobs, because `at_once × C_hub = lent − floor`.

**The count is kept in the two cases the sum cannot answer, and they are
the runner's own two since D285** (`herd-runner/src/lib.rs`, its `match
room.admissible_mib`):

1. **a host that will not say how much memory it lends** — no sum on
   either side, the hub charges a figureless job nothing because there is
   no cap to charge it with (D23's rule), the runner announces
   `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY` = 1, and the count is the
   whole of that machine's bound on both sides;
2. **a machine that has told the hub it is holding more than the records
   know of** (D182 §3) — a count of jobs, with nothing saying what each
   was granted, so it cannot be added to a sum in mebibytes. Held
   against the announced ceiling so the surplus waits instead of being
   offered straight back to a full machine the moment it redials.

Dropping either would be an over-placement of a kind already measured:
the first is D181's refusal storm reached from the other side, the
second is the outage D182 was written for. Each has a test of its own.

**What a waiting job reads changed with it.** On a machine that stated
its size the sentence is the memory one — *"N MiB free of its X memory
admission and this job's ceiling would be Y"* — and
`A_CEILING_THE_RUNNER_ANNOUNCED` is written only in the two cases above.
`herd-hub/tests/announced_job_ceiling.rs` asserts both directions, and
Alexa's own oracle there is unmoved: attempts spent for the runner's
ceiling **0**, every other attempt unchanged, peak attempts out on the
runner exactly the figure it announced.

**`READINGS_AT_ONCE` stays 2 and no figure was invented for it.** It
bounds how many readings one machine runs at once (D169) — their kind's
own concurrency — and what room a reading takes is the charge's to say.

**Its one cost, stated.** A reading stops being free of the memory
admission: on the first consumer's own figures (20 GiB lent, a 6 GiB
floor, 7 GiB a job, their report) `jobs_at_once` is 2 and `C_hub` is
7168 MiB, so two jobs fill that machine and a reading behind them waits
until one ends. Nothing is refused and no attempt is spent — a reading
over the room waits, and waiting is never a failure (E-31). ORCH-331
carries it, to report with no question attached.

`herd-hub/tests/packed_by_declaration.rs` is still the proof, with
`herd_runner::job_ceiling` as its oracle and its known-bad control still
red. Its last test was
`a_reading_is_charged_nothing_and_the_announced_count_is_what_bounds_it`,
which said that if the count bound were ever dropped this is where it had
to be answered; it is renamed, not deleted, and answers it. The
consequence test lost its `admitted < at_once` term, which modelled a
bound the hub no longer applies.
```

### `docs/orchestration/board.md` — the rows

ORCH-323's row is already written on this branch: state `done`, with
what was done, the two cases the count is kept in, and the measured
figure. The two new rows are already written on this branch as well, in
*Open*, and their full text is:

```text
| ORCH-331 | **A requirement reading is no longer free of the memory admission, and on a two-place runner it now waits behind the work.** Since ORCH-323 a figureless reading is charged `C_hub = (lent − floor) ÷ jobs_at_once`, because that is what its runner grants a job that arrives with no `JobNeed` — and nothing on the frame says which jobs are readings (**D285**). On the first consumer's own figures (20 GiB lent, a 6 GiB floor, 7 GiB a job, their report) `jobs_at_once` is 2 and `C_hub` is 7168 MiB, so **two** jobs fill that machine and a reading submitted behind them waits until one of them ends. Before this it ran at once, charged nothing, bounded only by `--readings-at-once`. **Nothing is lost and nothing is refused** — a reading over the room *waits*, and waiting is never a failure (E-31) — so what this costs is the latency of discovering what a job needs on a busy one-runner fleet, which is the shape of their nightly. **Not measured against their machines** (D161): nobody here knows what their queue looks like at 20:23 | `herd-step` on `orch-323-charge-a-reading`, 2026-10-31, derived from the charge it changed and from the figures in ORCH-323's own row | orchestrator (to report) | open | **Report it, with no question attached** (CLAUDE.md, outcome 3). It is the stated cost of D285's option (b), it is reversible in one line, and the figure that would move it is theirs: the time between a job's submission and its reading's `attempt_started` on a runner that is full. **Do not pre-emptively exempt a reading from the charge** — a reading the hub charges less than its runner grants is exactly the gap ORCH-323 closed, and reopening it reopens D181's refusal storm. If it bites, the honest mitigations are a larger `--job-memory-max` on that runner (fewer, bigger places) or marking a reading on the frame, which is protocol surface Alexa would have to want | ORCH-323 | `herd-hub/src/matcher.rs` `charged_against_the_room`; `herd-hub/src/server.rs` `reading_job`; **D159**, **D169**, **D285**; `docs/orchestration/history/2026-10-31-orch-323-charge-a-reading.md` |
| ORCH-332 | **A dated remediation record names four unit tests that no longer exist under those names.** `docs/remediation/09-runner-self-limits.md` (2026-10-25, C5) cites `a_runner_at_the_ceiling_it_announced_takes_nothing_more_and_says_so` (:514), `the_ceiling_and_the_memory_admission_are_two_bounds_and_either_may_bite` (:528), `the_ceiling_counts_readings_too_because_the_runner_s_does` (:536) and `a_job_held_by_a_ceiling_waits_for_that_machine_and_then_moves` (:585) as the assertions behind its claims. ORCH-323 renamed all four, because every one of those names said the announced count was the bound and the memory sum is. The claims that record makes about C5 were true the day it was written; a reader following those names today finds nothing | `herd-step` on `orch-323-charge-a-reading`, 2026-10-31, from the rename it made — reported rather than fixed, because `docs/` was not this task's to write | herd-step | open | Decide whether a dated record is corrected at all. The precedent is **D286**: a stale figure in a dated entry is answered by a *new* entry and never by an edit, and a remediation record is the same kind of artefact as a handoff. **The author's recommendation is to leave it and let this row be the correction**, since a record edited after its date is a record nobody can read as evidence. If it is to be corrected, one sentence at the head of its §7.2 naming the rename is the whole of it, and it is a prose change: `check-docs.sh` alone | nothing | `docs/remediation/09-runner-self-limits.md:514`, `:528`, `:536`, `:585`; `herd-hub/src/matcher.rs` (the four renamed tests); **D286**; ORCH-323 |
```

## Decisions needed

None. D285 settled the choice between (a) and (b); this is (b) built, and
everything in it is inside the mandate and reversible. ORCH-331 is a
report with no question attached, and ORCH-332 asks the orchestrator, not
Alexa, whether a dated record is corrected at all.

## Resume point

`herd-merger`: take this branch into the batch, allocate the `D-NEXT`
entry above and land it with the batch's one `--no-ff` merge. Nothing
else is owed on ORCH-323.

If the gate named under *Gates* came back red, the resume point is that
receipt and not this file: the branch is not pushed on a red gate, and
the three sentences a red run would most likely be about are the
figure-bearing assertions added here — `1 MiB free of its 25 GiB`,
`828 MiB free of its 25 GiB`, and `0 MiB free of its 2 GiB memory
admission` in `announced_job_ceiling.rs`.

## Processes

PIDs started: one `cargo clippy` under `nix develop`, ended on its own;
one backgrounded `until ./scripts/bench-preflight.sh` wait loop, ended on
its own; one `herdctl run` submission per queued job, each ended with its
job. Nothing of another agent's was signalled, no `pkill -f`, no kill by
name. The untouchable PIDs 3006543, 3021002, 565152 and 662711 were never
touched.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-323`, off `/`, deleted
once the branch was pushed (`rm -rf`), as gotcha
`build-weight-off-root` asks.
