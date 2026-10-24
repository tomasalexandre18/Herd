# The orchestrator does not edit

- **Date:** 2026-10-15 (memory `orchestrate-do-not-code.md`: *"tu n'as
  pas à coder toi-même, tu es orchestrateur, tu lances des sous-agents"*;
  *"tu peux mettre autant d'agent que besoin"*), reaffirmed 2026-10-23
  (memory `always-delegate.md`: *"un agent toujours tu est
  orchestrateur"*, then *"et la petit regles des sous agent ?"* after the
  orchestrator resolved two merge conflicts itself).
- **Observed:** between 12 and 24 October the orchestrator's own shell
  ran 60 `sed -i`, 92 `cat >`, 29 `git commit`, 62 `git merge`, 47
  `gh pr merge`, 12 `git tag`, 14 `gh release` (audit extract of the
  session transcript). Calling a conflict resolution "integration, not
  implementation" was a rationalisation.
- **Why:** verification is where the orchestrator earns its place. It
  caught a decision number used twice, a skip count misreported, and a
  conclusion the code refuted. Typing itself costs that second pair of
  eyes and fills its context.
- **Expected now:** every repository write, including a one-line fix, a
  conflict resolution and the decisions journal, is a `herd-step` or
  `herd-merger` task. The orchestrator reads, compares, verifies, briefs
  and reports. Parallel agents are fine when they touch different files;
  when they would collide, the second branch is based on the first.
- **Verify:** `.claude/hooks/orchestrator-guard.sh` blocks the main
  session's `Edit`/`Write` on repository paths and its `git commit`,
  `git merge`, `gh pr merge`, `git tag`, `gh release`; see
  `docs/orchestration/permissions.md` for what is enforced and what is
  only contractual.
- **Replaces:** `CLAUDE.md` 2026-10-13 "Claude works alone. A sub-agent
  only for one long, isolated task, never several in parallel, and never
  without Alexa's agreement."
