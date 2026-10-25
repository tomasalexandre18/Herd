# watchdog-measures-each-age — handoff 2026-10-25

- **Item:** ORCH-063 (part 2 of 3 of ORCH-060)
- **Agent:** herd-step
- **Branch / commit:** `watchdog-measures-each-age`, based on
  `runner-keys-jobs-by-attempt` `cc0d29a` (ORCH-062), itself based on
  `origin/main` `199e4df`
- **PR:** opened from the template. **Not merged** — `herd-merger` merges.

The second of the three PRs Alexa asked for on 2026-10-25, splitting
PR #82: *"2. chien de garde : plus d'entrelacement mesure / remise en
file"*. Stacked on part 1 because part 3 needs both.

## Done

**The test first, in its own commit (`4ad75e5`), red on `cc0d29a`.**
`an_attempt_the_pass_itself_placed_is_not_judged_on_the_age_before_it` in
`herd-hub/src/server.rs`. Two jobs in the air on a machine that is **still
connected and reporting**; the pass requeues the first, its own dispatch
places both again, and the second is then judged on the figure the pass read
before the walk. Under the defect the hub writes:

```text
herd-hub: j0000000002 is attempt 2 on runner the-runner: …
herd-hub: j0000000002 is lost: … nothing has been heard about that attempt
for 90.00023203s …
```

— two lines apart, about an attempt the same pass created.

Two pieces of scaffolding, both in the test module:

- **`one_watchdog_pass(state: &State)`**, one pass of
  `watch_for_a_lost_runner` extracted verbatim. A `loop` with a ticker in it
  cannot be run once. Nothing in the pass is `async` — the store blocks and
  the peers are behind a `Mutex` — so the test drives the real pass over a
  real `JobDirStore` rather than a copy of its arithmetic. The extraction is
  behaviour-preserving: `continue` at the top level became `return`, and
  `&state` became `state`.
- **a second forced window on the test store**, which is renamed
  `AStoreWithAWindowForcedOpen` because it now holds two unrelated ones (no
  test uses both). `record_state` ends another job's attempt and requeues it
  the first time any job is set back to `Queued` — which is what the hub's
  relay loop does at whatever moment a runner speaks. A hook rather than a
  second thread: the defect is about an ordering, and a test of an ordering
  has to be able to choose it.

**Then the fix.** The age is read **again, inside the loop**, immediately
before the judgement, and a job whose fresh figure is inside `lost_after` is
skipped. The list keeps the figures the pass began with and nothing is
decided on them.

**One `D-NEXT` entry** in `docs/decisions.md`, which says why this is not a
cosmetic exaggeration of the silence: `what_a_runner_says` decides whether a
health report is evidence by comparing the report's age with the age of the
clock the watchdog is about to act on, so a stale figure makes a report
composed *before* the new attempt read as newer than it, and *"its report
does not name this attempt"* becomes proof that the attempt is gone. **The
board** carries ORCH-063.

## Not done, and why

- **Nothing of C5.** No ceiling, no refusal, no new field. Part 3.
- **The relay side is untouched.** The other half of the interleaving — a
  frame ending an attempt while a pass walks — is not a defect and needs no
  change; the pass is what had to stop trusting a figure it had kept.
- **No second snapshot.** Re-reading the whole list at the top of the loop
  would be the same bug one iteration later. What matters is that the figure
  and the attempt number come from one instant.
- **`docs/status.md` not touched** — it is the merger's.
- **No merge, no tag, no release, no `main`.**

## Gates

`export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-split`;
`./scripts/bench-preflight.sh` **ok** before each heavy phase
(`load 1.09 on 12 cores`, `14 GiB available`, `343 GiB free`).

```text
$ nix develop --command cargo build --workspace
(clean, exit 0)

$ nix develop --command cargo fmt --all --check
(clean, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
(clean, exit 0)

$ nix develop --command ./scripts/test.sh
test suite: PASSED
passed=779 failed=0 ignored=20
 12 assertion(s) COULD NOT RUN on this host and were skipped.
```

779 against part 1's 778: this PR adds one test.

### The red, on `cc0d29a`, before the fix

```text
$ nix develop --command cargo test -p herd-hub --lib -- --nocapture \
      an_attempt_the_pass_itself_placed

running 1 test
herd-hub: j0000000001 is queued again for attempt 1 of 2 at a runner that went away: runner the-runner is connected and the health report it sent 1.000251397s ago does not name attempt 1 of this job, and nothing has been heard about that attempt for 90.000231697s: the attempt is gone from a machine that is still here
herd-hub: j0000000001 is attempt 2 on runner the-runner: claimed by the-runner: labels none; …
herd-hub: j0000000002 is attempt 2 on runner the-runner: claimed by the-runner: labels none; …
herd-hub: j0000000002 is lost: runner the-runner is connected and the health report it sent 1.033654288s ago does not name attempt 2 of this job, and nothing has been heard about that attempt for 90.00023203s: the attempt is gone from a machine that is still here

panicked at herd-hub/src/server.rs:8172:9:
the pass placed attempt 2 of j0000000002 and then wrote it off in the same
pass, on the silence of attempt 1: Finished { outcome: Lost { reason:
"runner the-runner is connected and the health report it sent 1.033654288s
ago does not name attempt 2 of this job, and nothing has been heard about
that attempt for 90.00023203s: the attempt is gone from a machine that is
still here; this was attempt 2 of 2 at a runner that went away, so the job
is not attempted again (E-21). A go spent on a capability that had gone
under it would not have counted towards these (D76)" } }

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 177 filtered out
```

### The green, after it

```text
running 1 test
herd-hub: j0000000001 is queued again for attempt 1 of 2 at a runner that went away: …
herd-hub: j0000000001 is attempt 2 on runner the-runner: …
herd-hub: j0000000002 is attempt 2 on runner the-runner: …
test server::tests::an_attempt_the_pass_itself_placed_is_not_judged_on_the_age_before_it ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 177 filtered out
```

The `is lost` line is simply gone, and the job the pass really was about —
`j0000000001` — is still written off as it should be.

### The two expected reds

```text
$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8593:## 2026-10-25 — D-NEXT A runner's live jobs are keyed by job and attempt
8656:## 2026-10-25 — D-NEXT The watchdog measures each silence inside its own loop
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs check failed

$ ./scripts/check-orchestration.sh
FAIL  unmodified copy passes
check-orchestration: 86 passed, 1 failed
```

Two `D-NEXT` placeholders now, because this branch carries part 1's as well.
Both are the one expected failure and the only one
(`gotchas/a-decision-branch-cannot-be-all-green.md`). **Not fixed, by
instruction.**

### Skips — 12, the standing reference, unchanged

10 for cgroup-v2 delegation and 2 for the ZFS IO ceiling, printed and
counted as skips (D27). No new skip.

## Risks and findings

1. **`one_watchdog_pass` is a new seam, and a seam is a thing that can
   drift.** It is called from exactly one place and the loop around it does
   nothing but tick, so there is no second behaviour to keep in step. It was
   extracted rather than copied: `git show 4ad75e5 -- herd-hub/src/server.rs`
   shows the body moving with a dedent, one `continue` becoming `return`,
   and two borrows losing an `&`.
2. **The re-read costs one lookup per job the pass has decided to act on**,
   under the peers lock, at most once per job per tick. It is not on the
   path of a job that is behaving.
3. **The test's runner reports with `jobs: Vec::new()`**, which is a machine
   saying it holds nothing. That is deliberate and it is the dangerous case:
   a report that names nothing is read as evidence against every attempt
   whose clock looks older than the report. A test whose runner named its
   attempt would pass under the defect as well.
4. **One ordering the test does not cover**: the same pass judging a job
   whose fresh attempt was placed by the dispatch of a *later* requeue. It
   cannot happen — the walk is sequential and the dispatch runs inside the
   requeue — but it is the kind of thing the next change to this loop could
   make possible, and the fix is immune to it either way, since the figure
   is now read after whatever has happened.
5. **The test store is renamed**, so a future reader grepping
   `StoreThatHoldsTheClaimOpen` finds nothing. The one existing user
   (`a_probe_s_snapshot_cannot_land_in_the_middle_of_a_claim`) is updated
   and its behaviour is untouched: `and_requeue` is `None` there, and the
   hook only fires on `Queued`.

## Decisions needed

**None.** Alexa's decision of 2026-10-25 is the brief; this is part 2 of it.

## Resume point

`herd-merger` integrates ORCH-062 first, then this branch, allocating
**one** `D` number for it, then ORCH-064 (`c5-announced-job-ceiling`). The
three are stacked and the order is Alexa's.

## Processes

None left running. Every `cargo` and `nix` invocation was a foreground
child of this session and exited. No `pkill`, no `killall`, nothing killed
by name. `/mnt/ssd_980/herd/build/c5-split` is shared by the three parts and
is removed at the end of the last one.
