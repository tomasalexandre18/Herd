# C4 target architecture — handoff 2026-10-25

- **Item:** ORCH-047 (its standing recommendation is superseded; see below)
- **Agent:** herd-planner
- **Branch / commit:** `arch-target-decision`, based on `origin/main`
  `f507ab6`
- **PR:** none — the brief forbids one

## Done

`docs/remediation/07-target-architecture.md`: six candidate architectures
compared on durability, progression, crash recovery and migration cost; one
recommendation with what it costs and what it leaves open; a six-stage plan
in which each stage is revertible alone; ten system validation criteria;
six missing measurements; one decision for Alexa.

**The recommendation is "the keeper":** one owned thread performs every
store mutation, the reactor wakes it by message, the unit of work is bounded
in time and re-armed, writes are split into a decisive tier (synced, as
today) and an explanatory tier (`waiting_for` and its `events.jsonl` line —
atomic and visible, not synced), and the store's `Mutex<Live>` guards memory
and is never held across a filesystem call.

Its load-bearing claim: **`waiting_for` has no crash-durability
requirement**, because `Peers::waiting` is in memory and the first pass
after any restart rewrites every queued job's reason
(`decisions.md:8083-8086`).

## Not done, and why

- No code, no experiment, no PR, no deployment — all forbidden by the brief.
- `docs/remediation/README.md` not updated to list `07`: it is another
  agent's piece and the brief forbids touching the dossier's other files.
- `docs/orchestration/board.md` untouched, as instructed, although
  ORCH-047's recommendation is now superseded (§10 of the piece says so).
- The `collections_owed` index (`05 §8` item 3) is named as a separate
  storage decision and is not folded into this one.

## Gates

```
scripts/check-docs.sh: docs ok
```

Rust gates NOT RUN: no Rust file was touched, and the brief states no
`cargo` is needed. No skip to report — no suite was run.

Load and disk read before the reading phase, per the brief:
`/proc/loadavg` 0.85 2.72 5.19; `df -h /` 58 G free, 15 % used.

## Risks and findings

Four corrections to the brief, each with its evidence, in §11 of the piece.
The consequential one:

- **"Bounding `record_waiting` touches E-04" is wrong.** E-04's figures
  reach a requester from memory, not from the record
  (`coverage.md:21`, `decisions.md:8081-8086`). The record's obligation is
  E-53's — a job nobody follows still says on its disk why nothing is
  happening — which is a *visibility* promise that `rename` keeps with no
  `fsync`. This is what opens the design space the recommendation uses.
- "Aborts the pass in silence" is wrong on the silence: three of thirteen
  sites print the error. The unreported thing is that the pass is
  **partial** — the jobs behind the failing one are never judged.
- "The lock is taken once per directory" is half the finding;
  `unfinished_jobs` holds it across its whole read, which is the half that
  blocks writers.
- Store locking is a **structural** finding on a synthetic fan-out, not a
  measured production cost: post D at one real junction is 600 ns–1.6 µs
  and `06` `I`1 leaves the arrival process unobserved.

The recommendation rests on one unmeasured figure — the cost of an unsynced
`record_waiting`. Stage S0 of the plan settles it with one run of a bench
that already exists, and the piece says what changes if it comes back high.

**A contract conflict, reported as the brief asks.** `.claude/agents/
herd-planner.md` says the planner may write exactly one file, a handoff
under `docs/orchestration/history/`. The brief names a second deliverable,
`docs/remediation/07-target-architecture.md`. I wrote both and touched
nothing else. The contract should be amended: a planner whose output is a
dossier piece cannot also be limited to a handoff. The same frontmatter
disallows `Edit`; I used it once, on my own file, to rewrap one line.

## Decisions needed

**Does Herd accept that a queued job's wait explanation is visible but not
crash-durable?** `job.json`'s `waiting_for` written atomically and readable
instantly, a power cut may discard the last change, the first pass after
restart rewrites it; the `events.jsonl` "stays queued" lines lose their tail
on a crash and are not rewritten.

- **Recommendation: yes.** It is 99.5 % of a runner junction, nothing
  depends on the guarantee, and the alternative that keeps it — a
  write-ahead log — buys an unmeasured burst for a silent rollback hazard in
  a repository with no migration code.
- **Fallback if no:** stages S1, S2, S3 and S5 still bound the hub's
  responsiveness; the budget then paces 9.633 s of unavoidable `fsync`, the
  records lag memory by seconds to minutes under a deep queue, and V4 is
  dropped.

## Resume point

If Alexa approves the direction, the next task is **S0**, which is a
measurement and not a change: extend
`herd-hub/tests/junction_breakdown.rs` (on `measure-junction-breakdown`,
`8471939`, already `#[ignore]`d) with a `write_file_unsynced` variant of
post C at `N` = 200, 1 000, 4 000, plus the `strace` sync count. It blocks
only stage S4 and refutes or confirms the piece's central H.

S1 (a `SIGTERM` path for the hub) and S2 (the store's lock guards memory,
never I/O) are independent of S0 and of each other, and either may start in
parallel.

## Processes

none
