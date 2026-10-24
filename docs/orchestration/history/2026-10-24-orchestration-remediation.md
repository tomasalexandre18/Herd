# Orchestration remediation — handoff 2026-10-24 (final)

- **Item:** ORCH-004
- **Agent:** the remediation agent (mandate v1.0, bootstrap exception)
- **Branch / commit:** `orchestration-remediation` / see `git log
  origin/main..`, based on `origin/main` `3da48f5`
- **PR:** opened from this branch, left unmerged (ORCH-001)

## Done

H0–H7 as the mandate lists them; the consolidated report is
`docs/orchestration/remediation-report.md`. Eleven live sessions ran
(V1–V7, V10, V11 as `claude -p`; V8, V9 as scripts); outputs under
`2026-10-24-h7-evidence/`. Findings from them were folded back: two guard
holes closed (V7), `git tag -l` allowed for `herd-release` (V10), the
worktree rule ORCH-031 (V11), the trace switches gitignored and refused
by the suite.

A real interruption happened mid-H7: the agent's context was cut and it
resumed from the previous version of this file and the `live/` outputs.
That is V6 for real.

## Not done, and why

- Nothing merged: ORCH-001 is waiting. The PR is prepared.
- The `MEMORY.md` pointer (in `memory.md`) is not applied: outside the
  repository.
- ORCH-030 (shared `CLAUDE_CONFIG_DIR`) is the owner's host configuration.
- V12 has no token measure: `claude -p` gives none.

## Gates

`./scripts/check-docs.sh`: `docs ok` (two notes: D170 and D172 held by
open PRs). `./scripts/check-orchestration.sh`: 87 passed, 0 failed. Rust
gates **NOT RUN** locally (no code touched); CI runs them on the PR. No
skip to name, since no suite ran.

## Risks and findings

See `remediation-report.md` §12.

## Decisions needed

ORCH-001 (merge authority) — recommendation on the board; ORCH-002
(invariant wording) — accept.

## Resume point

Read the board. If ORCH-001 is answered: brief `herd-merger` to integrate
this PR (its CI must be green; the PR's Evidence is in the body). Then
ORCH-010. If the running orchestrator has not yet got this contract, it
still reads the 2026-10-13 `CLAUDE.md`: the first thing after the merge is
a fresh session.

## Processes

Eleven `claude -p` children, all exited; none killed by name. Test
worktrees and branches removed after inspection; none pushed.
