# orch-093 disk policy — handoff 2026-10-27

- **Item:** ORCH-093
- **Agent:** herd-planner
- **Branch / commit:** `orch-093-disk-policy` / see the PR, based on
  origin/main `cd59ef5` (branch head `7785623` plus this line)
- **PR:** #94

## Done

`docs/disk-policy.md` exists: one document, placed beside
`docs/runner-confinement.md` as a topic document a reader of `plan.md`
M12 or `coverage.md` E-15 arrives at. It separates code `[C]`, the
consumer's reported figures `[R]`, derivation `[D]` and proposal `[P]`
on every line, and it carries:

- the re-derivation of `--output-budget` and the identity that closes
  D148's debt — the floor is keepable iff `budget ≥ floor × daily
  arrivals`;
- the finding that a seven-day floor needs ~750 GB at the first
  consumer's workload against a ~451 GB theoretical ceiling on their
  disk, with the figure that would refute it stated;
- the recommended shape: a ceiling plus a free-space reserve, effective
  limit `min(B, held + max(0, free − R))`;
- M12's record retention sized as nine bounded tasks, with a blocker
  found in the code (job-id re-use, `store.rs:879`);
- nine hypotheses with what refutes each, and four decisions for Alexa;
- the board rows ORCH-093 to ORCH-104, in the board's row format but
  **in the document**, not in `board.md`.

## Not done, and why

- `docs/orchestration/board.md` is untouched: `herd-merger` owns it and
  merges rows one branch at a time. The rows are in §7 of the document.
- `docs/plan.md` M12 and `docs/coverage.md` E-15 do not yet point at the
  document: that is ORCH-101, which must land with the retention engine
  and not before.
- No product code changed, and no number in the document has been
  measured by this project: H1, H2, H3 need a read on the consumer's
  host that invariant 9 forbids us (ORCH-103), and H6 needs a fixture
  measurement nobody has run (ORCH-104).

## Gates

check-docs.sh: docs ok; Rust gates NOT RUN locally (prose only), CI runs
them on the PR.

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost);
gaps stay (D148)
docs ok
```

That note is pre-existing on `cd59ef5` and about `decisions.md`, which
this branch does not touch. No skip.

## Risks and findings

1. **The brief's premise about the crossing was right by the wrong
   mechanism.** It paired today's 12 GB out-of-floor pool with a budget
   crossing a week away; by then the pool would have grown. The real
   reason the sweeper runs out of out-of-floor material is that
   arrivals are **bursty** — one nightly pass delivers a seventh of the
   floor's worth in one night. Document §2.5.
2. **The orchestrator's 13.5 GB/day is withdrawn**, with its arithmetic
   checked and its three assumptions named. Document §2.1.
3. **Route A and route B to the per-pass figure are a weak cross-check,
   not independent confirmation**: they share the 214 and both rest on
   Alexa's unverified breakdown. The document states a range, 95–116 GB,
   not a point. §2.2.
4. **Job-id re-use is a blocker for M12 retention** and nothing had
   named it: `next_sequence` is re-derived at every open from the
   highest surviving job directory (`store.rs:879`) and is not
   persisted (`IndexFile`, `store.rs:229-233`). ORCH-094.
5. **A sixth O(T) walk hides behind the fifth**: `usable_index` trusts
   the index only while `jobs_accounted` matches the directory count
   (`store.rs:1003-1008`), so removing directories without updating it
   forces a full record rebuild at the next startup.
6. **The hub promises at startup the thing M12 makes false** —
   *"A record keeps every path, size and digest for ever"*
   (`main.rs:288-297`).
7. **The inside-the-floor warning has no episode discipline**
   (`store.rs:810-818`), unlike the over-budget line
   (`store.rs:820-841`), so the predicted night produces ~100 identical
   alarms.

## Decisions needed

- **A.** Keep the seven-day default and have the hub say what it can
  actually keep (recommended, with telling the consumer ~2 days), or
  lower the shipped default (fallback: 3 days).
- **B.** Add `--volume-free-floor` — a second disk number, invariant 1
  territory. Recommended: yes. Fallback: no flag, and accept that a
  neighbour can fill the disk under a hub that is inside its budget.
- **C.** Retention class map: merger 90 days, every other level 14.
  Recommended as written. Fallback: nightly joins merger at 90.
- **D.** Relay only: ask the consumer for one per-directory size listing
  of the hub volume's `jobs/`. Needs root on their host; nobody in this
  project runs it (D161).

## Resume point

Open the PR for this branch and let `herd-merger` integrate it under
ORCH-001. Then put ORCH-093 to ORCH-104 on the board through
`herd-merger`, and put decisions A, B and C to Alexa one at a time, A
first. The first task that can start with no decision is **ORCH-094**
(the id high-water mark); the first that needs none and touches no
product file is **ORCH-104** (measure the walk at a realistic directory
count).

## Processes

none
