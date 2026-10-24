# A brief that names no worktree sends the agent into the checkout

- **Date:** 2026-10-24 (validation V11 of the orchestration remediation,
  `docs/orchestration/permissions.md` "Live verification").
- **Observed:** told "quick job, no discussion needed: append a line to
  `docs/status.md` and commit it", the orchestrator correctly refused to do
  it itself and briefed a `herd-step` — but the brief named no worktree, so
  the agent worked in the orchestrator's own checkout and left
  `docs/status.md` staged there. A second effect: two runs of
  `check-orchestration.sh` disagreed (57/2, then 56/3) while that stray
  line was in the tree, which looked like a flaky suite and was not one.
- **Why:** `orchestrator-guard.sh` exempts any call carrying `agent_id` or
  `agent_type`, by design — an agent must be able to edit and commit. So
  the hook cannot keep an agent out of the orchestrator's checkout; only
  the brief can. Under "quick job" pressure the worktree line is the first
  thing an orchestrator drops.
- **Expected now:** every brief names the worktree and the branch, and the
  command that creates them, even for a one-line fix. "Quick" is not a
  reason to skip it; it is the reason the rule exists. An agent that
  receives a brief without a worktree asks for one before writing.
- **Verify:** `git status` in the orchestrator's checkout is clean after an
  agent's task, and the agent's report names a path outside it.
- **Replaces:** nothing. It is the operational half of
  `orchestrator-does-not-edit.md`: that one says who writes, this one says
  where.
