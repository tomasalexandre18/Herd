# c5-review-hardening — handoff 2026-10-25

- **Item:** ORCH-060
- **Agent:** herd-step
- **Branch / commit:** `runner-holds-its-own-limits` / this commit, on top of
  `ceff7b7`, based on `origin/main` `199e4df`
- **PR:** #82, **left open and unmerged**

The seven items of the orchestrator's brief, which are the findings of the
contradictory review of PR #82 that the brief chose to act on. The review
itself is at
`scratchpad/c5-review/2026-10-25-review-c5-announced-job-ceiling.md` (not in
the repository). **The review's blocking finding — §1.2, a link outage past
`lost_after` turning N running jobs into N jobs ending `lost`, and the
`goes_a_runner_took` / D76-shaped exemption that would answer it — is with
Alexa and is untouched here.** No behaviour of the hub or the runner on that
path changed; nothing below prejudges it.

## Done

### 1. The refusal sentence is tied to the words the runner really produces

The literal `"holds at most"` was duplicated into the test *and* ambiguous:
the hub's own `ShortNow` sentence contained the same words, so
`ceiling_refusals` could not perform the attribution the documentation
claims for it.

- **`herd-runner/src/confine.rs`** gains
  `pub const A_RUNNERS_OWN_CEILING: &str = "this runner holds at most"`, and
  **both** branches of `jobs_at_once` build their sentence with it — the
  silent-host branch included, which previously carried no attribution
  clause at all (a refusal from a runner with no `--host-memory` would have
  been invisible to the oracle).
- **`herd-hub/src/matcher.rs`** gains
  `pub const A_CEILING_THE_RUNNER_ANNOUNCED: &str = "it said it holds at most"`
  and builds the `ShortNow` sentence with it. Neither clause is a substring
  of the other and neither sentence contains the other's clause.
- **`herd-hub/tests/announced_job_ceiling.rs`**:
  `THE_RUNNERS_CEILING_SENTENCE` is now `herd_runner::A_RUNNERS_OWN_CEILING`
  (imported, not copied), and the third use of the old literal at the
  `waiting`-line filter — the one matching the *hub's* sentence on purpose —
  is now `THE_HUBS_CEILING_SENTENCE` =
  `herd_hub::matcher::A_CEILING_THE_RUNNER_ANNOUNCED`.
- **`herd-hub/tests/fake_hub_flood.rs`** matches the refusal with the runner's
  constant instead of the literal.
- New test **`the_runners_ceiling_sentence_is_not_the_hubs`** asserts it
  against the sentences **as they are really built** (both branches of
  `jobs_at_once`, and the hub's clause), not against the two constants: a
  wording edit that reintroduced the ambiguity would leave the constants
  looking fine.

The shared constant was available to a `herd-hub` test, so no fallback was
needed: `herd-runner` is already a dev-dependency of `herd-hub` (the file
calls `herd_runner::jobs_at_once`).

Evidence — the real refusal now carries the runner's own clause:

```text
what it said when it refused: the runner is already running 8 job(s) and
never started this job: this machine lends 4 GiB and keeps 2 GiB as its own
floor, and caps every job at 256 MiB, so this runner holds at most 8 job(s)
at once
```

and the hub's `waiting` line still carries the hub's:

```text
on capped-runner, 2 job(s) already out there and it said it holds at most 2
at once, so it takes nothing more until one of them ends
```

### 2. `peak_out_there` is an equality

`assert!(peak_out_there <= CEILING)` became
`assert_eq!(peak_out_there, CEILING)`, with the reason for the lower half in
the code. The test also now **prints** the peak, so a future failure arrives
with its figure.

**Measured 50 times**, 20 of them as a batch and 4 inside full
`./scripts/test.sh` runs: the peak was **2 every time**. It is also 2 at a
200 µs poll (6 runs), which is the experiment for the only mechanism that
could make it read 3 — the window in which a result is written before the job
is terminal. See *Risks* for the one failure I cannot account for.

### 3. The guard matches the real spelling, and asserts it scanned something

`the_hub_never_derives_a_ceiling_of_its_own` looked for
`jobs_at_once = Some(` and `jobs_at_once.unwrap_or(`; the hub builds
`Announced` with field-init shorthand, so the real spelling has a colon, and
the reviewer's `jobs_at_once: Some(999)` mutation passed the guard.

It now walks every occurrence of the field name in shipped code and refuses
any `Some(` that appears **after** a `:` or `=` — which is punctuation-blind
for a value being put *into* the field, while leaving the legitimate read
`if let Some(at_once) = seat.jobs_at_once` alone — plus `.unwrap_or(`,
`.or(`, `.map_or(`, `.unwrap_or_else(` and `.get_or_insert` directly on it.
Two more holes closed in the same test:

- `assert!(scanned > 1000)` per file, so a `#[cfg(test)]` helper moved up
  either file cannot silently shrink the region to nothing;
- `assert!(found_the_field)`, so a rename cannot leave the guard scanning for
  a word the hub no longer uses.

**Verified against the reviewer's mutation**: with
`herd-hub/src/server.rs:2212` set to `jobs_at_once: Some(999),` the guard now
fails. See *Gates* for the output.

### 4. `fake_hub_flood`'s cut-short path skips loudly instead of asserting

The settle loop broke above load 8.0, set `cut_short`, and still returned
`Some(Flood)`; the helper then asserted `abandoned == how_many - 8` on a
truncated read, so on a loaded shared host both tests went **red** where the
file's own D27 paragraph promises a skip.

The cut-short path now prints its figures, cleans up, emits
`herd_runner::testing::skipped(...)` and returns `None`. The `cut_short`
field and the helper's preamble are gone: every `Flood` that reaches the
assertions is a complete one. The file's D27 paragraph is rewritten to say
so, for both readings of the load.

This skip is **conditional on host load** and did not fire — the suite's skip
count is unchanged at 12, the standing reference.

### 5. The "paced by nothing" test asserts its ceiling

`a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing` was headed
*"a runner that said nothing is placed on exactly as it was"* while its
runner announces 28, and asserted nothing about that 28.

- Two named constants, `THE_HARNESS_CEILING` = 28 and
  `HOW_MANY_WELL_INSIDE` = 4, with the derivation written down.
- The 28 is **asserted** from `support::host()` and
  `herd_runner::DEFAULT_MEMORY_MAX` (newly re-exported for it), with the
  runner's own sentence in the failure message; and
  `const _: () = assert!(THE_HARNESS_CEILING > HOW_MANY_WELL_INSIDE)`, so the
  day the two figures meet the file stops compiling.
- The heading is rewritten to what it tests, and says plainly that **no
  end-to-end test in this suite exercises `jobs_at_once: None` and none
  can** — our runner derives a ceiling from its own machine and always
  announces one, floored at 1 — naming where the case *is* covered:
  `herd-proto`'s `round_trip` (a `main`-shaped hello decodes to `None`) and
  the matcher's unit tests, whose seats announce nothing by default,
  including
  `a_runner_that_announced_no_ceiling_is_placed_on_exactly_as_it_was`.

The test keeps its name: it is now accurate, and renaming it would cost the
PR's cross-references for nothing.

### 6. §7.5's table carries the measured spread, not one sample

`docs/remediation/09-runner-self-limits.md` §7.5, the **handoff of
2026-10-25** (`2026-10-25-c5-announced-job-ceiling.md`, with a dated
correction note), `docs/remediation/README.md`'s row and the board row now
state the five runs: **6 of 8 lost and 12 attempts spent in four runs, 4 of
8 and 9 in one; peak 6 or 7**. The source is cited as the reviewer's five
runs of the neutered-gate oracle, and the bench was **not re-run** — nothing
here needed it.

The total-attempt row is marked **I** for the fifth run: 14 was read in the
four runs that spent 12, and the fifth run's total was not read off its
records. I did not derive it, although it is derivable, because deriving it
would put an unmeasured figure in a table whose whole defect was unmeasured
figures.

### 7. The two sentences wrong against `v0.3.7`, and the release notes

- **`herd-runner/src/lib.rs`** (the `jobs_at_once` field in the hello): *"a
  hub that ignores it only costs the fleet the attempts it costs today"* is
  replaced. The comment now says that the version to measure against is
  `v0.3.7`, where this runner had **no** ceiling and such a hub cost **zero**
  refusals; that a runner-first fleet therefore pays refusals it never paid,
  and `attempts_allowed` is 2, so a job refused twice ends with no verdict;
  that the order is **hub first**; and that **nothing detects it** —
  `Welcome` carries no echo of the field.
- **§7.6** is rewritten the same way, opening *"Upgrade the hub first. This
  is a condition, not a preference"*, withdrawing its own earlier *"pays
  exactly what it paid before this change"* by name, and stating that
  hub-first costs nothing (a hub on this version reads no `jobs_at_once`
  from a `v0.3.7` runner, which means *"it did not say"*).
- **A new top-level `## Release notes` section**, placed before §0 and
  written for `herd-release` to copy verbatim:
  - **RN-1 — upgrade the hub before the runners**, with the `v0.3.7`
    comparison and the fact that nothing in Herd detects or enforces the
    order.
  - **RN-2 — a runner started without `--host-memory` now takes one job at a
    time**, with the mechanism (`herd-runner/src/main.rs` sets
    `host.memory_mib` only when the flag is given or a cgroup limit is read,
    so `confine::jobs_at_once` returns
    `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY` = 1), the startup line verbatim,
    why it is correct under D23 (an absent figure is absent, never a figure
    invented from the machine), and that it is nonetheless a fleet-wide
    throughput cliff from a missing flag. **Whether any of the consumer's
    runners starts without the flag is written as unknown and stays
    unknown** (invariant 9, D161) — which is the reason it has to be said
    loudly rather than checked.
- §6's bullets now point at RN-2 and quote the corrected startup line.
- **The PR body** (#82) is edited so that **"upgrade the hub first"** is a
  stated condition in a section of its own rather than a remark at the end
  of the rollback paragraph, with RN-2 beside it, and so that its table
  carries the spread. See *Risks* for the one thing to know about that edit.

## Not done, and why

- **The review's §1.2 blocking finding and the D76-shaped exemption.** Out of
  scope by the brief: it touches the attempt record, which is persistence,
  and Alexa has not answered. Nothing here changes behaviour on that path.
  Nothing here prejudges it either: the only wording that comes near it
  (§7.6, RN-1) describes the cost that exists *today* on an un-upgraded hub,
  which is true under either answer.
- **The review's §1.3** (`inner.jobs` keyed by `JobId` alone, so attempt 2
  of a job assigned to a runner still running attempt 1 silently replaces
  the entry). Pre-existing, reachable only by the §1.2 sequence, and a fix
  touches `take_job`/`finish`/`release` — behaviour on exactly the path the
  brief fenced off. Carried as a finding.
- **The review's §2 second bullet** (priority against a capped seat, **H** —
  "what would settle it: a matcher test with two priorities and one capped
  seat"). Not in the brief's seven, and it is new test coverage rather than a
  correction to what a reader is told.
- **The review's §6.2** (`reclaim_a_past_life` deleting every
  `<data root>/outputs/*` with no pid match, **H**, and whether
  `DataRoot::open` takes an exclusive lock). Not in the brief's seven, and
  settling it is a behaviour question about the reaper.
- **`docs/decisions.md` is untouched**, as the brief required. See *Risks*:
  one paragraph of the second `D-NEXT` entry repeats the sentence item 7
  corrects, and `herd-step` may not edit that file.
- **No third `D-NEXT` entry.** Items 6 and 7 are corrections of wording and
  of a measurement's precision; neither settles anything that was open, so
  neither is a decision.
- **No merge, no tag, no release, no `main`.** PR #82 stays open.

## Gates

`./scripts/bench-preflight.sh` before each heavy phase; the first:

```text
$ ./scripts/bench-preflight.sh --target-dir /mnt/ssd_980/herd/build/c5-harden
target dir:  /mnt/ssd_980/herd/build/c5-harden
filesystem:  /mnt/ssd_980, 340 GiB free
load (1m):   0.37 on 12 cores, ceiling 9.00
memory:      15 GiB available, floor 2
bench-preflight: ok, safe to start.
```

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 32.48s

$ nix develop --command cargo fmt --all --check
(clean)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.62s
(clean)

$ nix develop --command ./scripts/test.sh
...
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
```

796 test functions reported `ok` and 12 `HERD-SKIPPED` lines — one test more
than the 795 of `ceff7b7` — it is
`the_runners_ceiling_sentence_is_not_the_hubs` —
and **the same 12 skips**: 10 for cgroup-v2 delegation and 2 for the ZFS IO
ceiling, the standing reference (D27). **No new skip.** The flood bench's
load-conditional skips, both the before-flood one and the new mid-flood one,
did not fire on a quiet host. The suite was run **four times**, green each
time.

The file under change, and the flood bench:

```text
$ nix develop --command cargo test -p herd-hub --test announced_job_ceiling
test neither_version_number_moves ... ok
test the_figure_announced_is_the_one_the_runner_derives ... ok
test the_runners_ceiling_sentence_is_not_the_hubs ... ok
test the_hub_never_derives_a_ceiling_of_its_own ... ok
test a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing ... ok
test no_attempt_is_spent_on_a_ceiling_the_hub_was_told_about ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ nix develop --command cargo test -p herd-hub --test fake_hub_flood \
    -- --test-threads 1 --nocapture
--- a hub that is not one, flooding a runner: flood-two-fifty ---
assignments sent: 250 in 2.292587ms
job workspaces open on the runner: 8 (peak 8)
attempts the runner handed straight back: 242
what it said when it refused: the runner is already running 8 job(s) and
never started this job: this machine lends 4 GiB and keeps 2 GiB as its own
floor, and caps every job at 256 MiB, so this runner holds at most 8 job(s)
at once
jobs on the runner's own health report: Some(8)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**242 unchanged**, which §7.6 says it must be for ever.

The peak, printed, over 20 consecutive runs of the whole file:

```text
     20 peak attempts out on the runner at once: 2
```

**Item 3's guard, proved against the reviewer's own mutation.** With
`herd-hub/src/server.rs:2212` temporarily set to `jobs_at_once: Some(999),`
— the mutation the old guard passed:

```text
$ nix develop --command cargo test -p herd-hub --test announced_job_ceiling \
    the_hub_never_derives
test the_hub_never_derives_a_ceiling_of_its_own ... FAILED
server.rs:2212 fills in a ceiling the runner did not announce:                     jobs_at_once: Some(999),
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out
```

The mutation was reverted; `git status` is clean of it.

### The two expected reds

```text
$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8595:## 2026-10-25 — D-NEXT A runner holds its own ceiling
8690:## 2026-10-25 — D-NEXT A runner announces its ceiling and the hub honours it
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs check failed

$ ./scripts/check-orchestration.sh
== check-docs.sh goes green on the repository as it is
FAIL  unmodified copy passes
    docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
    8595:## 2026-10-25 — D-NEXT A runner holds its own ceiling
    8690:## 2026-10-25 — D-NEXT A runner announces its ceiling and the hub honours it
check-orchestration: 86 passed, 1 failed
```

**Both are the one expected failure and the only one**, exactly as the brief
predicted: `gotchas/a-decision-branch-cannot-be-all-green.md`. The
`check-orchestration.sh` failure is `check-docs.sh` run on an unmodified copy
of this tree — the same complaint, cascaded. The D170/D174 lines are notes
about gaps held by open PRs, not failures. Both go green when the merger
allocates the two numbers. **Not fixed, by instruction.**

## Risks and findings

1. **One unexplained red I could not reproduce, and did not see the figure
   of.** The first run of `announced_job_ceiling` after item 2's change
   failed at the new `assert_eq!(peak_out_there, CEILING)`. I captured the
   backtrace but the panic message was cut by the `tail` I used, so **I do
   not know whether the peak read high or low.** Since then: **50
   observations, all exactly 2** — 20 as a batch, 6 at a 200 µs poll (the
   experiment for the only "reads 3" mechanism I can name, the window in
   which an attempt's place is freed before its job is terminal, which
   `a_result_waiting_for_an_acknowledgement_does_not_hold_a_place` is
   about), 4 inside full-suite runs, and the rest singly and after forced
   recompiles. I cannot close it. The `println!` is in the test precisely so
   that a recurrence arrives with its figure; if it recurs and reads **low**,
   the honest fix is `1 <= peak <= CEILING` with the reason written down,
   which still closes the "passes at 0" hole the review found. If it reads
   **high**, it is a real over-placement window and a finding of its own. I
   judged 50 clean observations enough to ship the equality the brief asked
   for; a reader who disagrees should know it rests on that and on one
   uncaptured failure.
2. **`docs/decisions.md` carries the same false sentence item 7 corrects,
   and I may not edit it.** The second `D-NEXT` entry, under *"What this
   costs, in the one direction that is left"* (near
   `docs/decisions.md:8775`),
   says a runner-first fleet *"pays exactly what the entry above measured"*.
   That is the §7.6 defect verbatim, and it is about to become a permanent
   numbered decision. `herd-step` may write nothing in that file but a new
   `D-NEXT` entry, and a wording correction is not a decision, so I left it.
   **`herd-merger` should correct that paragraph when it allocates the
   number**; it is on the board row and in *Not done*. If the orchestrator
   would rather it were a decision of its own, that is its call, not mine.
3. **The two new constants are public API of two crates.**
   `herd_runner::A_RUNNERS_OWN_CEILING` and
   `herd_hub::matcher::A_CEILING_THE_RUNNER_ANNOUNCED` exist so a test can
   match words it did not copy. Both are documented with the reason, and the
   new test asserts the property rather than the spelling, so a future edit
   to either sentence is safe as long as the clause stays in it. Also newly
   re-exported from `herd-runner`: `DEFAULT_MEMORY_MAX` and
   `JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY`, both already `pub` in `confine`.
4. **Every operator-facing refusal sentence changed**, by five words: *"so it
   holds at most N job(s)"* became *"so this runner holds at most N job(s)"*.
   Anybody grepping the old phrase still matches, since `holds at most` is
   intact; anybody matching `so it holds at most` does not. §6's "line to
   grep for" is updated. This is a sentence shipped to someone else's
   machine, which is why it is named here.
5. **I did not re-run the "before" oracle**, by instruction. §7.5's spread is
   the reviewer's five runs, cited as such. If anyone wants a sixth, the
   mutation is `herd-hub/src/matcher.rs`'s `seat.jobs_at_once` read as
   `Option::<u32>::None`.
6. **The PR body edit.** I edited #82's body with `gh pr edit` because the
   brief asked for the condition to be in it and the orchestrator does not
   edit. My own contract says "no PR", which I read as *do not open one*; if
   it was meant to cover editing a body, this crossed it, and the content is
   in the repository anyway (§7.6 and the Release notes section) so nothing
   is lost by reverting the body.
7. **Review findings I judged not worth acting on**, beyond the brief's
   seven and the three behaviour items above: the review's §5.6 sweep found
   nothing, and I confirmed nothing there needed a change; `line.split("//")`
   in the guard still truncates a `//` inside a string literal, which the
   review calls harmless and which I left — it costs reach, never a false
   failure, and there is no such line in either file. §2's first bullet (the
   stickiness window now reachable by a merely busy machine, 3 h at the
   default) is already said out loud in §7.4 and the PR body; I added
   nothing.
8. **The worktree.** The brief said "work in your own worktree". The branch
   was already checked out at
   `scratchpad/c5-runner-limits`, by the finished agent that wrote
   `ceff7b7`, and git will not check one branch out twice, so I worked there
   rather than creating a second. No other agent was in it.

## Decisions needed

**None from this task.** The one open question on ORCH-060 is not mine and is
unchanged: the review's §1.2 — should a refusal for a runner's own ceiling
spend one of the job's goes, given that a link outage longer than
`lost_after` (60 s) can otherwise turn N running jobs into N jobs ending
`lost`? Recommendation in the review: the D76 shape
(`goes_a_runner_took` already ignores an attempt with a `requeue_code`).
Fallback: accept it, written into `decisions.md` beside the two `D-NEXT`
entries rather than discovered by the consumer. It touches the attempt
record, so it is Alexa's.

## Resume point

`herd-merger` takes `runner-holds-its-own-limits`: rebase on `origin/main`,
allocate **two** D numbers in order (the runner's ceiling first, the
announced cap second), replace the 29-odd `D-NEXT` references in Rust
comments, **correct the second entry's "pays exactly what the entry above
measured" paragraph** (finding 2 above), re-run the gates, and merge #82 once
CI is green — subject to Alexa's answer on §1.2 if the orchestrator decides
that answer must land first.

## Processes

No background process, no daemon, nothing killed. Every `cargo` and `nix`
invocation ran in the foreground and exited. `CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/c5-harden`, removed at the end of the task.
