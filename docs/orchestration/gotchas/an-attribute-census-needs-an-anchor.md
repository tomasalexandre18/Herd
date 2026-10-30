# An attribute census needs a start-of-line anchor

- **Date:** 2026-10-30, measured by `m12-plan-rows` on `main` at
  `a8cf397` while cross-checking the gate's `#[ignore]` list.
- **Observed:** `grep -rc '#\[ignore' herd-*/ herdctl/ --include='*.rs'`
  summed to **51**. The attribute lines are **38**, and
  `scripts/expected-skips/ignored.txt` holds exactly **38**
  `HERD-IGNORED:` names. The 13 extra hits are prose: 11 test files
  quote the attribute inside a doc comment while explaining why a case
  is off — a module header saying that every case is ignored, so the
  workspace suite does not run it, writes the attribute out in full, and
  a grep that does not care where on the line it sits counts it.
- **Why:** the gate's census is not a grep at all — `scripts/test.sh`
  reads cargo's own `... ignored` lines and `scripts/gate.sh` compares
  those names against the expected list. A human re-deriving the figure
  with an unanchored pattern therefore gets a number 13 too high and
  concludes the **expectation is stale**, which is the one reading that
  sends somebody to bless a list that is already correct. The overcount is
  the mirror of the usual trap: a detector proven to fire can still be
  counting the wrong thing.
- **Expected now:** anchor it — `grep -rc '^[[:space:]]*#\[ignore'` — and
  say which of the two figures a report is quoting. A count is never the
  signal anyway; `gate.sh` says so itself, *the count is not the signal,
  the list is*, and the list is compared by name.
- **Verify:** the three figures agree in a healthy tree, and the first
  does not:
  `grep -rc '^[[:space:]]*#\[ignore' herd-*/ herdctl/ --include='*.rs' |
  awk -F: '{s+=$2} END{print s}'` against
  `grep -c '^HERD-IGNORED' scripts/expected-skips/ignored.txt` — both 38
  at `a8cf397`, while the unanchored form prints 51.
- **Replaces:** nothing. It is the counting half of ORCH-202 and D27: that
  rule is about a skipped assertion never being reported as a pass, this
  is about the census that enforces it being re-derived correctly.
