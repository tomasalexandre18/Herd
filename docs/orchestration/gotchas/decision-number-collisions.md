# Decision number collisions

- **Date:** 2026-10-16 (D145: "Three branches appended an entry on the
  same day and each took the next free number against a `main` that then
  moved under it; this one was renumbered twice"), 2026-10-23 (memory
  `always-delegate.md`: verification "caught a decision number used twice
  on two branches").
- **Observed:** on 2026-10-24 `origin/main` holds D1–D171 with gaps D143,
  D154 (documented) and D170 (held by PR #67); PR #70 holds D172. The
  file is append-only but not in number order: D148 sits after D149,
  D161 after D167.
- **Why:** a branch allocates its number against the `main` it saw; two
  branches in flight see the same `main`.
- **Expected now:** a number is an identifier, not a position (D148), gaps
  stay and are never filled. A branch writes its entry with a provisional
  id `D-NEXT` and `herd-merger` allocates the real number at integration,
  one branch at a time. `scripts/check-docs.sh` rejects a duplicate
  number and a malformed header, and lists the gaps it does not know.
- **Verify:** `./scripts/check-docs.sh` on a copy with a duplicated header
  fails with the number named; `docs/orchestration/decisions-structure.md`
  holds the analysis and the pending migration proposal.
- **Replaces:** "take the next free number against `main`" as the writing
  rule; the identifier rule itself is unchanged.
