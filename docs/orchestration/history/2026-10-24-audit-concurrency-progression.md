# audit-concurrency-progression — handoff 2026-10-24

- **Item:** audit mandate 2, §4.2 (agent B); no ORCH id was given in the
  brief, and I did not allocate one. Closest board rows: ORCH-021 (the
  hub re-reads every record), ORCH-022 (`follower_alive.rs:230` flaky).
- **Agent:** herd-step
- **Branch / commit:** `audit-concurrency-progression`, based on
  `origin/main` `0a2cf73`
- **PR:** none — `herd-step.md` forbids opening one. `herd-merger` opens
  it when the orchestrator says so.

## Done

`docs/remediation/03b-concurrency-and-progression.md`, new, the only
product file touched. Read-only audit of the hub's loops, locks, queue,
backpressure, stop/restart and host failure modes, against `0a2cf73`.
Every claim marked F / H / I with a `file:line`, a test name or a
decision entry. Covers §4.2's six deliverables, re-examines D169 and
D171 for cause vs symptom vs special case, evaluates I02/I06/I07/I08/
I09/I10 against §6, judges the two unstable tests, and specifies five
experiments that are **not** run.

## Not done, and why

- No Rust changed, no test run, no build, no load: the brief and
  mandate §0.2 forbid all of it on this shared host.
- `scripts/check-orchestration.sh` not run: the brief names a known
  defect in it that can write into the current repository.
- `docs/remediation/README.md` not created: the brief allows exactly one
  file, and the index belongs to whoever assembles both audits.
- Board not updated: outside the allowed paths.

## Gates

```
$ ./scripts/check-docs.sh
docs ok
```

The four Rust gates (`cargo fmt`, `cargo clippy`, `cargo build`,
`scripts/test.sh`) were **NOT RUN** locally. The change is prose only;
CI runs them on the PR. A gate that did not run is not a gate that
passed (D27).

No `HERD-SKIPPED` line: no suite was run to produce one.

## Risks and findings

The five ranked findings are in §12 of the report. In short: the hub's
persistence still runs on its reactor and no pass has a cost bound
(D171 says so itself); a pass is O(queue depth) in reads and one pass
runs per submission, which D171 did not touch; invariant 2 is enforced
only by hub-side arithmetic, because the runner spawns a task per
`AssignJob` with no count gate; the hub has no stop path and an unnamed
restart write-burst; an unwritable record aborts a whole pass with
nothing on any record to say so.

Contradiction with the brief, §13 of the report: **D171 does not mention
"quadratic reads"**. Its words are a per-queued-job-per-pass cost that is
linear in the queue, and what it removes is writes. The quadratic
behaviour is real but comes from elsewhere (one pass per submission over
a growing queue) and survives D171 untouched. If the phrase came from
one of the two 24 October analyses, the attribution should be corrected
there rather than in D171.

`docs/status.md:69` is stale against `0a2cf73` (says a reading is
unbounded; D169 bounded it). ORCH-010 already owns the restatement; I
changed nothing.

## Decisions needed

Whether to authorise experiment §10.1 (pass cost against injected store
latency, on an isolated machine) before the architecture options are
compared. **Recommendation: yes, and first.** It is the one measurement
that decides whether moving persistence off the reactor is urgent or
merely correct, and the delegating-store pattern it needs already exists
in the tree. **Fallback:** if no isolated machine is available, record
I06 as unproven in `05-invariants.md` and let option A/B/C be compared
without it, stating that gap in `09-independent-review.md`.

## Resume point

`herd-merger` opens the PR for branch `audit-concurrency-progression`
once audit A's branch is also ready, so the two reads land as a pair and
can be cross-checked as §0.1 requires.

## Processes

None started, none ended.
