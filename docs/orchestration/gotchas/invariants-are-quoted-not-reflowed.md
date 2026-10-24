# Invariants are quoted, not reflowed

- **Date:** 2026-10-24 (remediation H1, caught by the orchestrator's
  review before ORCH-002 reached Alexa).
- **Observed:** the rewrite of `CLAUDE.md` for the multi-agent contract
  shortened two invariants to save lines and did not say so: invariant 3
  lost *"bound to that interface only, never on a default or public one,
  and never behind a port forwarded on someone's box"*, invariant 6 lost
  *"based on declared capabilities and priority"* — the criterion
  `matcher.rs` implements. The prohibitions held; the text that says
  **how** they hold was gone, and the question put to the owner described
  a diff that was not the one on the branch.
- **Why:** a length limit (150 lines) invites compression, and a
  compression of an invariant reads as harmless to the one doing it.
- **Expected now:** an invariant's text changes only as an explicit
  proposal to Alexa, quoted before and after. A length problem is solved
  elsewhere in the file. Any PR touching `CLAUDE.md` "Invariants" gets a
  `herd-reviewer` pass whose first step is
  `git diff origin/main -- CLAUDE.md` restricted to that section, with
  every removed word named in the review.
- **Verify:** `diff <(git show origin/main:CLAUDE.md | sed -n
  '/^## Invariants/,/^## Layout/p') <(sed -n '/^## Invariants/,/^## Layout/p'
  CLAUDE.md)` shows only additions, or the removal is on the board as a
  question.
- **Replaces:** nothing; it sharpens `workflow.md` step 6.
