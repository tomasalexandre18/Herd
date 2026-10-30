# A drafted title is measured unfenced, not as it sits in the handoff

- **Date:** 2026-10-30, at batch 19's landing; lifted out of
  `history/2026-10-30-merge-batch-19.md` by `m12-plan-rows` so that it is
  read on demand rather than once.
- **Observed:** a `D-NEXT` entry drafted by `m12-r0-r9-preview` carried the
  title *The retention preview answers the output budget's question too,
  without being given a budget*. Inside the handoff it sat in a fenced
  `markdown` block and every gate was green. As a real header in
  `docs/decisions.md` it is `## <date> — D253 ` plus 92 columns of title —
  **113 columns**, over the 79-column rule by 34. The merger shortened it
  at allocation to *The retention preview answers a budget it is not
  given*, 75 columns, keeping what the entry turns on.
- **Why:** `scripts/check-docs.sh`'s width check skips a line inside a
  fence and a line beginning with `|`, and nothing else. **A header is not
  exempt.** So a draft is checked in the one place it cannot fail and is
  unchecked in the shape it will land in, and the agent who wrote it has
  already handed off by the time it is prose. The margin is thin on
  purpose: the longest header in `docs/decisions.md` is **exactly 79**, so
  the convention is tight against the limit and there is no slack to lose.
- **Expected now:** measure anything drafted for somebody else to paste
  **as it will land** — a `decisions.md` title, a `docs/status.md`
  sentence, a prose line of a handoff's *Drafted for* section. A board row
  and a table row stay exempt wherever they go, because the rule itself
  exempts them; a header never is.
- **Verify:** on the line as it will land, not as it sits:
  `printf '%s' "## 2026-10-30 — D999 <title>" | LC_ALL=C awk '{l=$0;
  print length(l)-gsub(/[\200-\277]/,"",l)}'` — the same count
  `check-docs.sh` makes, dashes and `≤` included. 79 or under, or shorten
  it.
- **Replaces:** nothing. It is the drafting half of
  `a-decision-branch-cannot-be-all-green.md`: that lesson is why a
  drafted entry's branch is green with one red note, this one is why green
  there says nothing about the width it lands at.
