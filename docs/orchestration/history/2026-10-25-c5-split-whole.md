# c5-split-whole — handoff 2026-10-25

- **Item:** ORCH-064 (part 3 of 3 of ORCH-060)
- **Agent:** herd-step
- **Branch / commit:** `c5-announced-job-ceiling`, based on
  `watchdog-measures-each-age` (ORCH-063), itself on
  `runner-keys-jobs-by-attempt` (ORCH-062), itself on `origin/main`
  `199e4df`
- **PR:** opened from the template. **Not merged** — `herd-merger` merges.

The third of the three PRs Alexa asked for on 2026-10-25: *"3. C5 complet
(plafond, deux champs, refus gratuit, compte retenu,
CEILING_REFUSALS_ALLOWED) avec la coupure de lien"*. It carries the whole of
C5 — the work that was written on `runner-holds-its-own-limits` at `72d3242`
— **minus** the two defects that are now the PRs underneath it, **plus** the
one change of behaviour Alexa required the same day.

## Done

### What came across from `runner-holds-its-own-limits`

The four code commits of that branch, cherry-picked in their original order
with their original messages and their three handoffs, then **one
reconciliation commit** (`104b44d`) squaring them with the two PRs
underneath. No behaviour of C5 was changed by the split; the three
reconciliation points are:

1. **`take_job`'s "one machine runs one job once" refusal** reads the map
   keyed by job **and** attempt (part 1) instead of by job id. Its *reason*
   changes, because the old one no longer applies: the map cannot replace an
   entry any more, and what still forbids two attempts of one job here is
   that **a workspace belongs to a job** — `DataRoot::open_workspace` takes
   a `JobId` and clears the directory if it is already there, so a second
   attempt would wipe the working directory out from under the first one's
   process. The refusal is still a refusal about the machine's own count and
   still spends no go.
2. **`abandon` and `release`** keep part 1's keyed forms. The branch's
   versions of those two hunks did the same job with a guard on a job-keyed
   map and were dropped.
3. **The watchdog** keeps part 2's extracted pass and its re-read age, and
   gains the `HowItEnded` argument C5 adds to `give_the_job_another_go`.

Part 1's three tests about a map holding two attempts of one job now reach
that state through a test-only door, `Session::put_in_hand`, because
`take_job` refuses it: they are why the refusal is a guard and not the only
thing standing between the hub's view of a machine and the machine's own
count. The third one still goes through `take_job`, which is reachable — the
refusal is about a job this runner is *running*, and that test's first
attempt has ended.

### The change of behaviour Alexa required

> PR 3 : que devient un job au 9e refus ? Verdict lisible machine exigé, pas
> lost.

A job handed back past `CEILING_REFUSALS_ALLOWED` ends
**`JobOutcome::RefusedAsFull`**, a new variant:

```rust
RefusedAsFull {
    reason: String,
    refusals: u32,
    refusals_allowed: u32,
    runners: Vec<RunnerThatWasFull>,   // { name, holding: Option<u32> }
}
```

**The name.** *Refused as full* says what happened and who it happened to:
the machines were full, the job was refused. It is not readable as the job
being full, and the serde tag a program sees is `refused_as_full`.

**The fields, and why those.** `reason` is the sentence a person reads.
Beside it is everything a program needs to decide what to do: how many
refusals, the bound they ran into, and every machine that said it was full
with the count **it** gave. The count is per machine and not one figure for
all of them, because *"the machine configured for one job at a time refused
this four times"* is something to act on and *"four machines were full"* is
not. `holding` is `Option<u32>` for a refusal recorded by a hub that did not
keep the figure, and is never read as zero — a machine holding nothing would
have taken the job.

**Why none of the four endings it is nearest to.** `Lost` is E-21's *the
machine went silent*, and no machine went anywhere — reporting it that way
told a reader to go and look for a dead host. `Impossible` is written in its
own doc as *what no fleet can fix*, and this is fixable by adding capacity,
so it would be a lie. `Exited { code }` is forbidden by `JobOutcome`'s own
rule: Herd's failures never wear the consumer's exit code. `CouldNotStart`
is Herd trying to run a job and failing, which is not what happened.

**`herdctl`.** `cause` is the variant's own serde tag by construction
(D166), so the document says `"cause": "refused_as_full"` with
`"exit_code": 125` and `"exit_code_from": "herd"` — 125 because nobody ran
this job, not 124 which would say a budget stopped it, not 1 which would say
it failed. `no_verdict_reason` names the machines and the bound on stderr.

**The record.** `Attempt::at_its_ceiling` widens from `bool` to
`Option<u32>`, carrying the machine's own count so the figure survives to
the end. Additive on exactly the terms the old flag was: `#[serde(default,
skip_serializing_if)]`, absent meaning what `false` meant, and the field off
the record entirely otherwise.

**Neither version moves.** `PROTOCOL_VERSION` stays 2, `RECORD_VERSION`
stays 1, both asserted by `neither_version_number_moves`. The precedent is
two days old and in this repository: D166 added `SetupFailed` to the same
enum with neither number moving (`f2c0ea7`; both were 2 and 1 before and
after, checked).

### The 60 s test
`a_link_cut_while_the_runner_is_full_loses_no_job` with the deployed sixty
seconds (`…_at_the_deployed_sixty_seconds`),
in the suite behind `#[ignore]`, found by `cargo test -- --ignored`. It is
one body with the short one — `no_job_is_lost_to_an_outage`, taking an
`AnOutage` of five durations — and one oracle, which is Alexa's and does not
depend on the clock. The `#[ignore]` reason and the doc comment say what it
costs (171 s, measured) and why it is not in the default suite. **Run by
hand, green: 171 s.**

### Documents

- **Three `D-NEXT` entries**, not one. See *Risks* 1: they are C5's own
  three decisions, written and reviewed before the split, and the new
  verdict is folded into the third rather than made a fourth.
- `docs/remediation/09-runner-self-limits.md`: §8.2 now says the 60 s shape
  is in the suite behind `#[ignore]`; §8.3 items 2 and 3 carry the widened
  field and the new ending; a paragraph after them states the one-way cost;
  §8.4 says the job map and the watchdog are their own PRs underneath this
  one. **A new RN-3** release note: a hub rolled back past this version
  cannot read a record that ends `refused_as_full`.
- The board carries ORCH-064.

## Not done, and why

- **`RECORD_VERSION` and `PROTOCOL_VERSION` are untouched**, which the brief
  made a stop condition. Nothing forced either: the variant is additive by
  the enum's own rule and by D166's two-day-old precedent. The cost that is
  *not* additive is the rollback direction, and it is written down in the
  decision entry and in RN-3 rather than paid for with a number that is
  Alexa's.
- **No `JobEventKind` variant.** The brief's other stop condition did not
  fire: `JobEventKind` is untouched by this PR, as it was by the branch.
- **`Inner::collections` is still keyed by `JobId` alone** — part 1's
  finding, unchanged here.
- **The hub still dispatches to a runner before reading one frame from it.**
  ORCH-061, for S3. This change makes it safe, not right.
- **`docs/status.md` not touched** — it is the merger's.
- **No merge, no tag, no release, no `main`.** PR #82 and
  `runner-holds-its-own-limits` are untouched: no force-push, nothing
  rewritten. The comment naming the three successors and the close are the
  last thing this task does.

## Gates

`export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-split`;
`./scripts/bench-preflight.sh` **ok** before each heavy phase (`343 GiB
free`, `load 0.51–2.52 on 12 cores`, `14–15 GiB available`).

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.97s

$ nix develop --command cargo fmt --all --check
(clean, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
(clean, exit 0)

$ nix develop --command ./scripts/test.sh
test suite: PASSED
passed=807 failed=0 ignored=21
 12 assertion(s) COULD NOT RUN on this host and were skipped.
```

807 against part 2's 779. The 21st ignored test is the 60 s one this PR
adds; the other 20 were already there.

### The red, on `301f4d7`, the branch's own pre-fix commit

The base of this PR is part 2's head, where
`herd-hub/tests/announced_job_ceiling.rs` does not exist at all, so the
honest "before" is the commit of this branch where the test exists and the
fix does not: `301f4d7`, the announced-ceiling work with the free refusal not
yet written. This PR's test file was put on it (the bound test dropped, since
it needs a `Policy` field `301f4d7` has not got) and run:

```text
$ nix develop --command cargo test -p herd-hub --test announced_job_ceiling \
      a_link_cut_while_the_runner_is_full_loses_no_job -- --nocapture

herd-hub: attempt 2 of j0000000001 was abandoned: the runner is already running 2 job(s) and never started this job: this machine lends 4 GiB and keeps 2 GiB as its own floor, and caps every job at 1 GiB, so this runner holds at most 2 job(s) at once
herd-hub: j0000000001 is lost: the runner is already running 2 job(s) and never started this job: ...
herd-hub: attempt 2 of j0000000002 was abandoned: ...
herd-hub: j0000000002 is lost: ...

panicked at herd-hub/tests/announced_job_ceiling.rs:1052:5:
0 job dead and 0 job without a verdict is the condition this change merges
under, and 2 of 2 came back without one: ["j0000000001: {\"reason\":\"...;
this was attempt 2 of 2 at a runner that went away, so the job is not
attempted again (E-21)...\",\"type\":\"lost\"}", "j0000000002: ..."]

test result: FAILED. 0 passed; 1 failed; 1 ignored; 0 measured; 6 filtered out; finished in 4.25s
```

The worktree it was run in was removed and its branch deleted; nothing of it
is pushed.

### The green, at this PR's head

```text
$ nix develop --command cargo test -p herd-hub --test announced_job_ceiling
test a_link_cut_while_the_runner_is_full_loses_no_job_at_the_deployed_sixty_seconds ... ignored, 171 s of wall clock, measured: the deployed 60 s threshold, a 65 s outage and 90 s jobs
test neither_version_number_moves ... ok
test the_figure_announced_is_the_one_the_runner_derives ... ok
test the_runners_ceiling_sentence_is_not_the_hubs ... ok
test the_hub_never_derives_a_ceiling_of_its_own ... ok
test a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing ... ok
test a_job_refused_past_the_bound_ends_and_names_it ... ok
test no_attempt_is_spent_on_a_ceiling_the_hub_was_told_about ... ok
test a_link_cut_while_the_runner_is_full_loses_no_job ... ok
test result: ok. 8 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 10.42s
```

and the 60 s one, by hand:

```text
$ nix develop --command cargo test -p herd-hub --test announced_job_ceiling \
      -- --ignored --nocapture
cutting the link for 65s, with the runner at its ceiling of 2
test ... has been running for over 60 seconds
the link is back; the runner is still running its 2 original job(s)
every one of the 2 jobs came back with its own verdict
test a_link_cut_while_the_runner_is_full_loses_no_job_at_the_deployed_sixty_seconds ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 170.56s
```

### What a job at the bound is now recorded as

```json
{
  "reason": "the runner is already running 2 job(s) and never started this job: this machine lends 4 GiB and keeps 2 GiB as its own floor, and caps every job at 1 GiB, so this runner holds at most 2 job(s) at once; this job has been handed straight back 1 time(s) of the 1 allowed by machines that were already as full as the ceiling they announced, so it is not offered again (D-NEXT). None of those refusals spent one of its 2 goes at a runner that went away (E-21): nothing failed and no machine went anywhere, the hub's count of what is out on capped-runner (holding 2) and theirs simply never agreed. Add capacity, or look at why the two counts disagree",
  "refusals": 1,
  "refusals_allowed": 1,
  "runners": [ { "holding": 2, "name": "capped-runner" } ],
  "type": "refused_as_full"
}
```

### The two expected reds

```text
$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8593:## 2026-10-25 — D-NEXT A runner's live jobs are keyed by job and attempt
8656:## 2026-10-25 — D-NEXT The watchdog measures each silence inside its own loop
8713:## 2026-10-25 — D-NEXT A runner holds its own ceiling
8808:## 2026-10-25 — D-NEXT A runner announces its ceiling and the hub honours it
8911:## 2026-10-25 — D-NEXT A refusal at a machine's own ceiling spends no go
docs check failed

$ ./scripts/check-orchestration.sh
check-orchestration: 86 passed, 1 failed
```

Five placeholders, because this branch carries parts 1 and 2 as well. Both
are the one expected failure and the only one
(`gotchas/a-decision-branch-cannot-be-all-green.md`). **Not fixed, by
instruction.**

### Skips — 12, the standing reference, unchanged

10 for cgroup-v2 delegation and 2 for the ZFS IO ceiling, printed and
counted as skips (D27). No new skip.

## Risks and findings

1. **This PR carries three `D-NEXT` entries, and Alexa asked for one per
   PR.** C5's own work was written as three decisions by the agents that did
   it, before the split existed, and each is a real separate decision: a
   runner holding its own ceiling, a runner announcing it and the hub
   honouring it, and what a refusal at that ceiling costs. Collapsing them
   would destroy argued text that has already been reviewed, so the new
   verdict is **folded into the third** rather than made a fourth. Parts 1
   and 2 each carry exactly one. If the orchestrator or Alexa would rather
   see three PRs with one entry each, that is a fourth split and not
   something to decide here.
2. **The rollback cost of the new variant is real and one-way.** A hub
   rolled back past this build, on a state root holding a job that ended
   `refused_as_full`, refuses to read that job's record and names the word
   it did not know. It is asserted in `herd-proto`'s `round_trip` test
   about a refused job, both halves, and written in the decision entry and
   in RN-3. If that window matters more than the ending, the lever is
   `--ceiling-refusals-allowed` set high enough that no job reaches the
   bound during it.
3. **The brief's `at_its_ceiling: Option<u32>` is now on two surfaces.** It
   was already on `AttemptAbandoned` (Alexa ratified that today); this PR
   widens the *record's* field from a flag to the same shape, which the brief
   did not name. Without it the end could not report a per-machine figure
   and the variant would carry a count attributed to no machine in
   particular. Additive on the same terms as the flag.
4. **`a_job_refused_past_the_bound_ends_and_names_it` now asserts more than
   it did**, including that **no** job in the run ended `lost`. That is the
   assertion the change of behaviour is for, and it is also the one that
   would catch a future edit routing a ceiling refusal back through `Lost`.
5. **The split changed no behaviour relative to `72d3242`** except the new
   ending, and I checked the three reconciliation points one by one (see
   *Done*). The one place where the *reason* for a behaviour changed rather
   than the behaviour is `take_job`'s refusal, and it is commented with the
   reason that survives.
6. **`Session::put_in_hand` is a `#[cfg(test)]` door past every refusal in
   `take_job`.** It exists for three tests and is documented with why. A
   reader should know it is there: a future test that used it to avoid a
   refusal it did not understand would be testing nothing.
7. **The 60 s test's 171 s is one measurement on this workstation.** On a
   slower CI box it would be longer; it is `#[ignore]`d precisely so that
   nothing in CI depends on the figure.
8. **The branch's own three handoffs are in this PR verbatim**, including
   their statements about PR #82 and about work that is now in parts 1 and 2.
   They are the record of what those agents did and I did not rewrite them;
   this handoff is where the split is described.

## Decisions needed

**None from this task.** Finding 1 is a question about form rather than
about the product, and the recommendation is in it.

## Resume point

`herd-merger` integrates in Alexa's order and no other:

1. `runner-keys-jobs-by-attempt` (ORCH-062, PR #83) — one `D` number;
2. `watchdog-measures-each-age` (ORCH-063, PR #84) — one `D` number;
3. `c5-announced-job-ceiling` (ORCH-064, this PR) — **three** `D` numbers,
   in the order the entries appear, and the `D-NEXT` references in Rust
   comments and in `docs/remediation/09-runner-self-limits.md` replaced with
   them.

PR #82 keeps a comment naming these three in order and is closed, reopenable
by anybody who disagrees.

## Processes

None left running. Every `cargo` and `nix` invocation was a foreground child
of this session and exited; the relay in the link-cut tests runs on threads
inside the test process and ends with it. No `pkill`, no `killall`, nothing
killed by name. The temporary worktree used for the red measurement was
removed and its branch deleted. `/mnt/ssd_980/herd/build/c5-red` is removed;
`/mnt/ssd_980/herd/build/c5-split`, shared by the three parts, is removed at
the end of this task.
