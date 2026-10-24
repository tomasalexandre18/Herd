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

Sections 6 to 10 of the mandate's report: *pending*.
