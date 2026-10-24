# Orchestration remediation — handoff 2026-10-24 (final)

- **Item:** ORCH-004
- **Agent:** the remediation agent (mandate v1.0, bootstrap exception)
- **Branch / commit:** `orchestration-remediation` / see `git log
  origin/main..`, based on `origin/main` `3da48f5`
- **PR:** #71, open; mergeable by `herd-merger` under ORCH-001 (decided
  2026-10-24) once CI is green and Alexa has answered ORCH-002

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

- Nothing merged: ORCH-002 is still waiting, and this PR is the one that
  carries the invariant wording it asks about. ORCH-001 is decided
  ("non il peux merge par defaut"): `herd-merger` merges by standing
  authorisation; tags and releases stay on Alexa's word per act.
- Two invariants (3 and 6) had been shortened without notice in the
  first rewrite; the orchestrator's review caught it and both are
  restored verbatim (gotcha `invariants-are-quoted-not-reflowed`).
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

ORCH-002 (invariant 8 extended, invariant 9 added) — accept. ORCH-001 is
decided and recorded in `CLAUDE.md`, the board, `workflow.md`,
`herd-merger.md` and D173.

## Resume point

Read the board. When ORCH-002 is answered: brief `herd-merger` to
integrate PR #71 (CI green, Evidence in the body; ORCH-001 grants the
merge). No tag is involved. Then
ORCH-010. If the running orchestrator has not yet got this contract, it
still reads the 2026-10-13 `CLAUDE.md`: the first thing after the merge is
a fresh session.

## Processes

Eleven `claude -p` children, all exited; none killed by name. Test
worktrees and branches removed after inspection; none pushed.
