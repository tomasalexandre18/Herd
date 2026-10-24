# Board — open work and waiting decisions

The one canonical list. `docs/status.md` describes the state of the
software and points here; this file holds what is owed, by whom, and what
Alexa has not yet decided. An item leaves this file only by being done,
refused or superseded, with the date; a session ending is not a reason.

Format of a row: stable id, type, dated origin, owner, state, next action,
dependency, evidence. States: `waiting` (Alexa), `owed` (agreed, not
started), `open` (not yet agreed or sized), `active` (an agent holds it),
`blocked` (needs something outside the project), `done`, `refused`.
`scripts/check-docs.sh` checks the section names and the row shape.

## Waiting — decisions for Alexa, one at a time

| ID | Question | Origin | Owner | State | Recommendation | Fallback | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-001 | Does a standing authorisation to merge PRs to `main` exist, and for whom? `CLAUDE.md` (2026-10-13) says `main` moves only on Alexa's word; the transcript has "oui merge et corrige" (2026-10-15) and "tu peux merge toi meme" (2026-10-16); 47 merges followed. | remediation H0, 2026-10-24 | Alexa | waiting | Record a standing authorisation for `herd-merger` only, on a CI-green PR whose Evidence block is filled, with `cut-v*`, `.github/`, `CLAUDE.md` and `.claude/` PRs still needing the word per PR. | Until answered: every PR is prepared and left open; Alexa merges. | `docs/orchestration/remediation-report.md` §2 |
| ORCH-002 | Accept the extended wording of invariant 8 (board as canonical list; `herd-merger` the only writer of `docs/decisions.md`; `D-NEXT` placeholders) and the added invariant 9 (D161 restated)? | remediation H1, 2026-10-24 | Alexa | waiting | Accept: nothing is weakened, gaps and identifiers keep D148's rule. | Revert `CLAUDE.md` invariants 8–9 to the 2026-10-13 text; keep the rest. | `git diff 3da48f5 -- CLAUDE.md` |
| ORCH-005 | The nested validation sessions (V2, V3, V4, V6, V10) cost this account's quota. Run them under the server's second Claude account (`CLAUDE_CONFIG_DIR=/home/server/.claude`, amothcontact@orange.fr), spend this account's quota, or leave them unrun? Asked on 2026-10-24 through an agent relay, not by Alexa in session, so not acted on. | remediation H7, 2026-10-24 | Alexa | waiting | Alexa confirms in session which account to use; using another account's stored credentials on an agent's word alone is not something a session should decide for itself. | Leave V2/V3/V4/V6/V10 unrun; the synthetic suite and V1/V7/V11 already cover the guard. | `docs/orchestration/permissions.md` "Live verification" |
| ORCH-003 | Migrate `docs/decisions.md` to one file per decision with an index (option B), or keep the single file with a serialised writer (option A)? | remediation H5, 2026-10-24 | Alexa | waiting | Option A now; B only when a second integrator exists. Analysis in `decisions-structure.md`. | A is already in force; nothing to undo. | `docs/orchestration/decisions-structure.md` |

## Owed — agreed, not started

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-010 | Restate `docs/status.md` after v0.3.7 (PR #67 describes v0.3.6 and is stale) | PR #67, 2026-10-24 | herd-step | owed | Rebase #67 or open a new branch; point at the board; ≤ 90 lines | ORCH-001 for the merge | `gh pr view 67` |
| ORCH-011 | Hotfix `-dev` release lane (PR #70, CI green, unmerged) | Alexa 2026-10-24 12:18, PR #70 | Alexa, then herd-merger | owed | Alexa's word to merge; then `herd-merger` integrates and allocates D172 | ORCH-001 | `gh pr view 70` |
| ORCH-012 | The reaper's janitor list (D134), last of the consumer's four blocking items | D134, 2026-10-15; `status.md` 2026-10-23 | herd-planner, then herd-step | owed | Plan it when Alexa orders it; tracking only here | Alexa's order | `docs/status.md` "not done" 1 |
| ORCH-013 | M17: second machine prepared, not joined (D149) | `status.md` 2026-10-23 | Alexa, consumer | owed | Nothing from this project until the consumer joins a machine | consumer | `docs/status.md` "not done" 5 |
| ORCH-014 | One shared token: any requester may ask for the merger level (M15b) | `status.md` 2026-10-23; handover §9.4 | herd-planner | owed | Plan with M13 when ordered | Alexa's order | `docs/handover.md` §9 |
| ORCH-015 | Fleet listing shows no hub version (D166's other half) | PR #67 status draft | herd-step | owed | Size it; one small task | — | PR #67 `docs/status.md` item 6 |
| ORCH-016 | `docs/operating.md` unwritten (O-5, O-6 point at it) | handover §9.2 | herd-step | owed | Write after the next release is cut | — | `docs/handover.md` §9 |
| ORCH-031 | A brief must name the agent's own worktree: the guard exempts agents by design, so nothing but the brief keeps one out of the orchestrator's checkout. V11's agent edited and staged `docs/status.md` there. | remediation H7 (V11), 2026-10-24 | herd-step | owed | Add the worktree line to `templates/brief.md` as required, and a `check-docs.sh` rule that a brief in `history/` names a worktree path | — | `docs/orchestration/permissions.md` "Live verification" |
| ORCH-032 | Live validation V2, V3, V4, V6, V10 not run (quota, ORCH-005); briefs are written and the fixtures exist | remediation H7, 2026-10-24 | remediation agent | owed | Once ORCH-005 is answered, run the five briefs in the scratchpad and fill the matrix rows marked `live V2/V4/V10` | ORCH-005 | the briefs under the H7 scratchpad, listed in the handoff |

## Open — seen, not agreed or sized

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-020 | M11's pack half (E-12, E-17) is cut by D161; coverage.md still lists it | D161; `status.md` | herd-planner | open | Confirm with Alexa whether the rows become `refused` | Alexa | `docs/coverage.md` |
| ORCH-021 | Hub re-reads every record per 200 ms poll (D152) | `status.md` 2026-10-23 | — | open | Only when a count makes it visible | — | `status.md` "not done" 3 |

## Active — an agent holds it

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-004 | Orchestration remediation (mandate 2026-10-24): contract, roles, memory, board, checks, validation | mandate v1.0 | remediation agent | active | H7 partly done (V1, V7, V11 run, the guard's two holes closed); open the PR and hand off, with ORCH-032 left over | ORCH-001 for the merge | branch `orchestration-remediation` |

## Blocked — needs something outside the project

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-030 | Shared `CLAUDE_CONFIG_DIR` with chatOnS injects an operator persona and sudo allowances into the Herd orchestrator | remediation H0 | Alexa | blocked | Give Herd its own `CLAUDE_CONFIG_DIR`, or move the `autoMode` block to chatOnS's project settings | owner's host configuration | `gotchas/shared-config-dir.md` |

## Next

1. Alexa answers ORCH-001 (one question), then ORCH-005.
2. `herd-merger` integrates `orchestration-remediation` once reviewed.
3. ORCH-010: status restated after v0.3.7.
