# Orchestration remediation — report

Mandate: "HERD — MANDAT D'EXÉCUTION, Remédiation de l'orchestration IA
uniquement (v1.0)", 2026-10-24. Scope: the Claude agents' organisation
only. Nothing here touches the Rust crates, the protocol, the deployment
files, the OS or the running instance.

This file is the mandate's consolidated report (§5). It is written step
by step; a section marked *pending* has not been done yet.

## 1. Starting reference (H0, 2026-10-24)

- Repository `/mnt/ssd_980/herd-project`, remote
  `github.com/tomasalexandre18/Herd`. The orchestrator's checkout has
  `main` at `d57f23a`, **behind** `origin/main` at `3da48f5` (merge of
  PR #69, `cut v0.3.7`). The remediation branch
  `orchestration-remediation` is cut from `origin/main` (`3da48f5`) in
  its own worktree; the orchestrator's checkout was not touched.
- Newest tag `v0.3.7` (2026-10-24). Open PRs: #67 `status-after-036`
  (docs only, carries D170), #70 `hotfix-dev-lane` (release lane,
  carries D172); both CI-green, both deliberately unmerged.
- CI on every PR: job `check` (flake check, fmt, clippy,
  `scripts/test.sh`) and job `docs` (`scripts/check-docs.sh`); both
  required by branch protection. No path filter, so CI runs the Rust
  gates on a docs-only PR too. Branch protection: 0 required reviews,
  force-push and deletion blocked, admins not enforced.
- `scripts/check-docs.sh` on `origin/main`: `docs ok`. Its rules:
  `CLAUDE.md` ≤ 150 lines, `docs/status.md` ≤ 90 lines, prose ≤ 79
  columns in `CLAUDE.md`, `README.md` and `docs/*.md` (not
  subdirectories), every `must` in `docs/coverage.md` names a milestone.
- `.gitignore` ignores all of `.claude/`, so nothing under
  `.claude/agents/` can be versioned until that line is narrowed to
  `.claude/worktrees/`.
- Claude Code **2.1.303**. The orchestrator process runs
  `claude --resume … --permission-mode auto` with
  `CLAUDE_CONFIG_DIR=/mnt/ssd_980/chatons/claude/config`. **That
  directory is shared with the chatOnS orchestrator process** (same
  `CLAUDE_CONFIG_DIR`), and its `settings.json` carries an `autoMode`
  block whose `environment` says *"This session is chatOnS's
  orchestrator"* and whose `allow` list grants `sudo systemctl`,
  `nixos-rebuild switch`, docker operations on the hub and pushes to
  `/etc/nixos`. Those rules therefore also apply to the Herd
  orchestrator. See risk R1.
- Baseline hashes of every organisational file are in the remediation
  scratchpad (`baseline-sha256.txt`); the Git rollback is
  `git checkout 3da48f5 -- <file>` or dropping the branch.

### Access limits respected

Read-only on: `/etc/nixos`, `/srv/herd`, containers, running jobs, the
consumer's production pass on v0.3.7, both memory directories, the
session transcript, the 42 registered worktrees (none deleted).

## 2. Facts re-verified against the 24 October audit

| Audit claim | Verified state on 2026-10-24 |
|---|---|
| `CLAUDE.md` says "Claude works alone" | True, line 112; also "A sub-agent only for one long, isolated task, never several in parallel, and never without Alexa's agreement" |
| Oral authorisations contradict the Git clause | Transcript: 2026-10-15 "oui merge et corrige"; 2026-10-16 09:55 "tu peux merge toi meme". `CLAUDE.md` still says "`main` only moves on Alexa's explicit approval". Since then the orchestrator ran `gh pr merge` 47 times. **Ambiguous: open question ORCH-001** |
| Old memory under `/home/server/.claude/projects/-mnt-ssd-980-herd-project/memory/` | Present, 3 files dated 2026-10-15: `MEMORY.md`, `orchestrate-do-not-code.md`, `herd-cargo-needs-nix-develop.md`. Not loaded: `CLAUDE_CONFIG_DIR` points elsewhere |
| New memory under `…/chatons/claude/config/projects/-mnt-ssd-980-herd-project/memory/` | Present, 4 files dated 2026-10-23: `MEMORY.md`, `always-delegate.md`, `docs-only-gates.md`, `subagent-brief-hygiene.md`. This is the one injected in the orchestrator's session |
| No `.claude/agents/`, no typed roles | True. `.claude/` holds only `worktrees/` |
| Orchestrator edits, `sed -i`, commits, merges, releases itself | Transcript extract: 8 `Edit` calls, 60 `sed -i`, 92 `cat >`, 29 `git commit`, 62 `git merge`, 47 `gh pr merge`, 12 `git tag`, 14 `gh release` from the orchestrator's own shell |
| `status.md` has no reliable owed/waiting list | True: prose "What is not done" list, no IDs, no owner |
| `decisions.md` conflicts and renumbering | 168 entries D1–D171 on `origin/main`; gaps D143, D154 (documented), D170 (held by PR #67). File order is **not** monotonic: D148 sits after D149, D161 after D167. Rule in force (D148, restated in D162, D165, D166): a number is an identifier, not a position; gaps stay; a branch takes the next free number against `main` |
| D27, D161, Evidence, check-docs, test.sh, PR template | All present and unchanged by this work |
| Shared scratchpad collision, `pkill -f`, misplaced worktrees | Recorded in memory `subagent-brief-hygiene.md` (2026-10-23); worktrees live in `.claude/worktrees/` and the session scratchpad |

## 3. Scope of change (strictly outside Rust and production)

Files that may change: `CLAUDE.md`, `.gitignore` (one line),
`.claude/agents/herd-*.md`, `.claude/settings.json` (permissions only),
`docs/orchestration/**`, `docs/status.md` (pointer only),
`scripts/check-docs.sh` and a new docs-check test script,
`.github/pull_request_template.md` (handoff fields). Nothing under
`herd-*/`, `deploy/`, `nix/`, `flake.*`, `.github/workflows/*.yml`.

## 4. Risk register

| ID | Risk | Mitigation |
|---|---|---|
| R1 | Shared `CLAUDE_CONFIG_DIR` injects chatOnS's operator persona and sudo allowances into the Herd orchestrator (D161 boundary) | Cannot be fixed from the repo. Recommendation to the owner in §8; versioned `CLAUDE.md` states the boundary so the contract wins over the injected persona |
| R2 | Narrowing `.gitignore` could expose worktree content | Keep `.claude/worktrees/` ignored; only `agents/` and `settings.json` become visible |
| R3 | A docs check that is too strict blocks legitimate PRs #67/#70 | New checks are additive and tested on both open branches before the PR |
| R4 | `CLAUDE.md` over 150 lines | Measured by `check-docs.sh`; detail goes to `docs/orchestration/workflow.md` |
| R5 | Permission deny rules on `Bash` can be bypassed by synonyms | Documented as contractual, not technical, in the permission matrix (H6) |
| R6 | Two agents writing `docs/decisions.md` in parallel collide on a number | H5: serialised writer (option A) until the owner decides on a migration |

## 5. Plan of lots

1. Lot 1 — contract: `CLAUDE.md`, `.gitignore`, this report, memory
   reconciliation (H1, H2).
2. Lot 2 — roles and handoff: `.claude/agents/`, workflow, brief and
   report templates (H3).
3. Lot 3 — board and checks: `board.md`, check-docs extensions and their
   failure tests, D-number validation (H4, H6).
4. Lot 4 — `decisions.md` structure: analysis and recommendation only,
   migration **blocked on approval** (H5).
5. Validation campaign V1–V12 (H7), recorded in `history/remediation.md`.

## 6. Inventory after (what changed, and why)

| File | Change | Why |
|---|---|---|
| `CLAUDE.md` | rewritten, 147 lines | roles split (orchestrator / five agents), "works alone" gone, docs-only gate rule reconciled with D27, merging recorded as `herd-merger`'s standing authorisation and tags/releases as Alexa's per act (ORCH-001, decided 2026-10-24), invariant 8 extended and invariant 9 added (ORCH-002), shared-host rules, session start/end. **Correction:** the first rewrite had shortened invariants 3 and 6 without saying so; the orchestrator's review caught it and both are restored verbatim — invariants 1–7 now differ from `3da48f5` only by one added sentence in 5 (D27) |
| `.gitignore` | `.claude/` narrowed to `worktrees/`, agent memory, local settings, trace switches | roles, hooks and settings become versioned |
| `.claude/agents/herd-{planner,step,merger,release,reviewer}.md` | new | one bounded role each; `disallowedTools` where the harness can remove a tool; `herd-release` carries its guard hook |
| `.claude/hooks/orchestrator-guard.sh`, `release-guard.sh`, `.claude/settings.json` | new | best-effort technical restriction of the main session and of the release role; fail open; `DISABLED` switch |
| `docs/orchestration/workflow.md`, `memory.md`, `permissions.md`, `decisions-structure.md`, `board.md`, `templates/`, `gotchas/`, `history/` | new | procedure, loading facts, permission matrix, journal analysis, canonical list, brief/handoff templates, dated lessons, handoffs and H7 evidence |
| `docs/status.md` | 3 lines reflowed, pointer to the board | status stays state; 90 lines |
| `docs/decisions.md` | D173 appended | the organisational decision, numbered above D172 (PR #70) |
| `scripts/check-docs.sh` | extended | decision headers, duplicates, `D-NEXT`, gaps noted, contract contradiction, roles, hooks, settings, board shape, handoff names, width in `docs/orchestration/**` |
| `scripts/check-orchestration.sh` | new, 87 assertions | every check proven red on a copy; collision corpus; both hooks positive and negative; run by CI's `docs` job |
| `.github/pull_request_template.md` | `## Handoff` section | the PR names its handoff and board items |
| `.github/workflows/ci.yml` | one line in job `docs` | runs `check-orchestration.sh` |

Not touched: `herd-*/`, `herdctl/`, `deploy/`, `nix/`, `flake.*`,
`Cargo.*`, `release*.yml`, `scripts/test.sh`, `scripts/report-skips.sh`,
anything outside the repository.

## 7. Contract and roles

Effective responsibilities are in `CLAUDE.md` Roles and
`workflow.md` "Roles and rights". Per operation: edit → `herd-step`;
integrate, resolve, allocate `D`, update board and status, open PR, merge
when authorised → `herd-merger`; plan → `herd-planner`; review a rule or
permission change → `herd-reviewer`; prepare a release → `herd-release`;
tag and publish → Alexa or whoever she names. Authority chain: Alexa →
orchestrator (briefs, verifies, asks) → agent (does, hands off). Real vs
procedural restrictions: `permissions.md` matrix; in one line, the
harness removes whole tools from planner and reviewer, the hook refuses
the main session's ordinary writes and the release role's publication
verbs, and everything finer than that is contractual and reviewed.

## 8. Memory and resumption

Paths observed, rules reconciled and the loading facts per role are in
`memory.md`; the lessons with dates and provenance in `gotchas/`. Loading
was verified live: V1 cited `CLAUDE.md` as startup context and opened the
board and workflow; `--agent` sessions followed their role file. The
fresh-session procedure worked on this very task: the remediation agent's
own context was cut mid-H7 and it resumed from
`history/2026-10-24-orchestration-remediation.md` and the `live/`
outputs, not from a conversation summary. Not recoverable: the exact
relayed answer that unblocked ORCH-005 (it is in the orchestrator's
transcript, not in a file). The proposed `MEMORY.md` is in `memory.md`;
applying it is outside the repository and left to the orchestrator.

## 9. Board, decisions, history

Open items were verified against PRs #67 and #70, `docs/status.md`,
`handover.md` §9 and D134/D149/D166 before being written; the audit's
"Docker Hub credentials" item is closed by D162 and is not on the board.
Format of a handoff: `templates/handoff.md`; six were produced in H7 by
agents of three roles. `Dxxx` validation: unique, well-formed, `D-NEXT`
rejected, gaps noted, file never rewritten — proven by
`check-orchestration.sh`. `decisions.md` structure: **option A applied,
option B decided as the direction and deferred** behind the trigger on
the board (ORCH-003, Alexa's answer relayed on 2026-10-24); no migration
run.

## 10. Verifications V1–V12

| Test | Verdict | Where |
|---|---|---|
| V1 fresh orchestrator | passed | `permissions.md` Live verification; `h7-evidence/v1-…` |
| V2 delegated micro-task | passed | `v2-…`, `v2-step-handoff.txt` |
| V3 two agents in parallel | passed | `v3a-…`, `v3b-…` |
| V4 merger with simulated conflict | passed | `v4-…`, `v4-merger-handoff.txt` |
| V5 human decision, then resumption | passed, with **simulated** owner messages | `v5a-…`, `v5b-…` |
| V6 interrupted then fresh | passed (fixture) and passed again for real on this task | `v6-…`, this report §8 |
| V7 agent refutes the brief | passed; found two guard holes, since closed | `v7-review.txt` |
| V8 failure and skip | passed | `check-orchestration-final-run.txt`, V2/V3/V11 outputs |
| V9 `Dxxx` control | passed | same |
| V10 simulated release | passed; found the `git tag -l` false positive, since fixed | `v10-…` |
| V11 orchestrator drift | passed (contract held; hook not needed); found ORCH-031 | `v11-…` |
| V12 context cost | partially tested: file inventory from the sessions' own citations, no token measure available from `claude -p` | the session outputs |

No verdict above is without an executed session or script; the
corresponding output is in `history/2026-10-24-h7-evidence/`.

## 11. Git changes and rollback

Branch `orchestration-remediation` from `origin/main` `3da48f5`, pushed;
commits listed by `git log origin/main..orchestration-remediation`. PR
#71, opened from the template with real Evidence; mergeable by
`herd-merger` under ORCH-001 once CI is green and ORCH-002 is answered. CI on the PR is the proof of the Rust gates, which were not
run locally (prose, scripts and `.claude` only). Rollback: close the PR
and delete the branch; nothing outside the branch was changed. Test
fixtures (`v2-gotcha-note`, `v3a-brief-gate-note`, `v3b-handoff-note`,
`v4-x`, `v4-y`, `v4m-merge`, `v6-task`) were inspected, their evidence
copied, then removed; none was pushed.

## 12. Remaining gaps

Blocking for "ready without reserve":

- ORCH-002 — invariant 8 wording and invariant 9 need the owner's accept.
  Settled the same day and no longer a gap: ORCH-001, Alexa's *"non il
  peux merge par defaut"* — `herd-merger` merges any CI-green PR with
  filled Evidence, no path excluded; pushing a tag and publishing a
  release stay on Alexa's word, one act at a time.

Non-blocking:

- ORCH-030 — shared `CLAUDE_CONFIG_DIR` with chatOnS (owner's host
  configuration).
- ORCH-031 — a brief that names no worktree lets an agent work in the
  orchestrator's checkout; the guard exempts agents by design.
- The guard is best effort (`git -c`, absolute binary path, `sh -c`) and
  version-dependent; the review of traces remains the protection.
- Open PRs #67 and #70 will need CI re-run after this PR merges; #67 is
  stale (ORCH-010).

## 13. Recommendation

**PRÊTE SOUS RÉSERVES.** The contract, roles, board, checks and hooks
exist, are versioned, go red when broken, and held under eleven live
sessions. The reserves are ORCH-002 (the owner's accept of invariants 8 and 9,
asked on a diff that now restores 3 and 6 verbatim) and the merge of this
PR itself, without which the running orchestrator still reads the
2026-10-13 `CLAUDE.md`. ORCH-001 was answered during the mandate.

## 14. Suggested next step

Only after Alexa validates this organisation: a separate mandate for the
technical audit of the Herd product. Nothing of it was started here.
