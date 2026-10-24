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
| ORCH-001 | Does a standing authorisation to merge PRs to `main` exist, and for whom? `CLAUDE.md` (2026-10-13) said `main` moves only on Alexa's word; the transcript had "oui merge et corrige" (2026-10-15) and "tu peux merge toi meme" (2026-10-16). | remediation H0, 2026-10-24; decided by Alexa 2026-10-24, relayed by the orchestrator | Alexa | done | **Decided, wider than the recommendation:** *"non il peux merge par defaut"*. `herd-merger` holds a standing authorisation to merge any PR whose CI is green and whose Evidence block is filled, `cut-v*`, `.github/`, `CLAUDE.md` and `.claude/` included. **Kept distinct, not covered by it:** pushing a tag and publishing a release stay on Alexa's word, one act at a time (v0.3.6 and v0.3.7 were each approved singly). | — | `CLAUDE.md` "Git and main"; `docs/orchestration/history/2026-10-24-orchestration-remediation.md` |
| ORCH-002 | Accept the extended wording of invariant 8 (board as canonical list; `herd-merger` the only writer of `docs/decisions.md`; `D-NEXT` placeholders) and the added invariant 9 (D161 restated)? **Correction 2026-10-24, before the question reached Alexa:** the orchestrator's own diff found invariants 3 and 6 shortened without notice (3 lost "bound to that interface only, never on a default or public one, and never behind a port forwarded on someone's box"; 6 lost "based on declared capabilities and priority"). Both are restored verbatim; `git diff 3da48f5 -- CLAUDE.md` now shows invariants 1–7 unchanged except one added sentence in 5 (D27). | remediation H1, 2026-10-24 | Alexa | waiting | Accept: 8 gains D148, the single writer and the board; 9 promotes D161 to an invariant; nothing is weakened, and the two cuts the review found are undone. | Revert `CLAUDE.md` invariants 8–9 to the 2026-10-13 text; keep the rest. | `git diff 3da48f5 -- CLAUDE.md` |
| ORCH-005 | Which account runs the nested validation sessions (V2, V3, V4, V6, V10)? | remediation H7, 2026-10-24 | Alexa | done | Done 2026-10-24: the sessions ran under this account after the orchestrator relayed the go; that relay is not in the remediation agent's surviving context, so the evidence of the answer is the orchestrator's transcript, not a file here. No other account's credentials were used. | — | `history/2026-10-24-h7-evidence/` |

## Owed — agreed, not started

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-003 | Journal of decisions: option A (one file, `herd-merger` the only writer, `D-NEXT` placeholder) is in force now; option B (one file per decision plus a generated index, the history archived byte for byte) is **decided as the direction and deferred**. A is B's prerequisite, not its alternative: separate files do not stop two branches both creating `D170`; the single allocator does. | Alexa, 2026-10-24, relayed by the orchestrator; analysis `decisions-structure.md` | herd-planner (plans the migration PR when triggered) | owed | Nothing until the trigger: `herd-merger` has allocated `D-NEXT` on at least five real decisions, at least two from branches open at the same time, with no duplicate flagged by `check-docs.sh`; or earlier on Alexa's word, or when a second integrator role is introduced | the trigger, then Alexa's word to open the migration PR | `docs/orchestration/decisions-structure.md` "Decision" |
| ORCH-010 | Restate `docs/status.md` after v0.3.7 (PR #67 describes v0.3.6 and is stale) | PR #67, 2026-10-24 | herd-step | owed | Rebase #67 or open a new branch; point at the board; ≤ 90 lines | — | `gh pr view 67` |
| ORCH-011 | Hotfix `-dev` release lane (PR #70, CI green, unmerged) | Alexa 2026-10-24 12:18, PR #70 | herd-merger | owed | `herd-merger` integrates (CI green, Evidence filled: ORCH-001 grants the merge) and allocates D172 |  — | `gh pr view 70` |
| ORCH-012 | The reaper's janitor list (D134), last of the consumer's four blocking items | D134, 2026-10-15; `status.md` 2026-10-23 | herd-planner, then herd-step | owed | Plan it when Alexa orders it; tracking only here | Alexa's order | `docs/status.md` "not done" 1 |
| ORCH-013 | M17: second machine prepared, not joined (D149) | `status.md` 2026-10-23 | Alexa, consumer | owed | Nothing from this project until the consumer joins a machine | consumer | `docs/status.md` "not done" 5 |
| ORCH-014 | One shared token: any requester may ask for the merger level (M15b) | `status.md` 2026-10-23; handover §9.4 | herd-planner | owed | Plan with M13 when ordered | Alexa's order | `docs/handover.md` §9 |
| ORCH-015 | Fleet listing shows no hub version (D166's other half) | PR #67 status draft | herd-step | owed | Size it; one small task | — | PR #67 `docs/status.md` item 6 |
| ORCH-016 | `docs/operating.md` unwritten (O-5, O-6 point at it) | handover §9.2 | herd-step | owed | Write after the next release is cut | — | `docs/handover.md` §9 |
| ORCH-031 | A brief must name the agent's own worktree: the guard exempts agents by design, so nothing but the brief keeps one out of the orchestrator's checkout. V11's agent edited and staged `docs/status.md` there. | remediation H7 (V11), 2026-10-24 | herd-step | owed | Add the worktree line to `templates/brief.md` as required, and a `check-docs.sh` rule that a brief in `history/` names a worktree path | — | `docs/orchestration/permissions.md` "Live verification" |
| ORCH-032 | Live validation V2, V3, V4, V5, V6, V10 | remediation H7, 2026-10-24 | remediation agent | done | Done 2026-10-24: all run, results in `permissions.md` "Live verification"; the fixture branches and worktrees were inspected and removed | — | `history/2026-10-24-h7-evidence/` |

## Open — seen, not agreed or sized

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-020 | M11's pack half (E-12, E-17) is cut by D161; coverage.md still lists it | D161; `status.md` | herd-planner | open | Confirm with Alexa whether the rows become `refused` | Alexa | `docs/coverage.md` |
| ORCH-021 | Hub re-reads every record per 200 ms poll (D152) | `status.md` 2026-10-23 | — | open | Only when a count makes it visible | — | `status.md` "not done" 3 |

## Active — an agent holds it

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-004 | Orchestration remediation (mandate 2026-10-24): contract, roles, memory, board, checks, validation | mandate v1.0 | remediation agent, then Alexa | active | H0–H7 done; PR open and **left unmerged**; Alexa answers ORCH-001 and ORCH-002, then `herd-merger` or Alexa merges | ORCH-001 | branch `orchestration-remediation`, `remediation-report.md` |

## Blocked — needs something outside the project

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-030 | Shared `CLAUDE_CONFIG_DIR` with chatOnS injects an operator persona and sudo allowances into the Herd orchestrator | remediation H0 | Alexa | blocked | Give Herd its own `CLAUDE_CONFIG_DIR`, or move the `autoMode` block to chatOnS's project settings | owner's host configuration | `gotchas/shared-config-dir.md` |

## Next

1. Alexa answers ORCH-002 (one question); ORCH-001 is decided.
2. `herd-merger` merges the remediation PR (#71) under ORCH-001 once CI is
   green and ORCH-002 is answered; the orchestrator then applies the
   `MEMORY.md` pointer from `memory.md`
   and starts its next session from the board.
3. ORCH-010: status restated after v0.3.7; ORCH-031: the brief rule.
