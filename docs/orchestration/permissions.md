# Permissions — wanted, technically enforced, verified

Claude Code 2.1.303. "Enforced" means the tool call is refused by the
harness or a hook; "contractual" means only the text forbids it and the
review of traces catches it. Nothing here weakens a gate: CI, the PR
template's Evidence, D27, branch protection and D161 are unchanged.

## Why it is a hook and not a deny rule

`permissions.deny` in `.claude/settings.json` applies to the main session
**and to every agent it spawns**, so a deny on `Edit` would paralyse
`herd-step`. The only documented signal that separates the orchestrator
from an agent is the hook input: an agent's `PreToolUse` payload carries
`agent_id`, the main session's does not. Hence
`.claude/hooks/orchestrator-guard.sh`, registered in `.claude/settings.json`
on `Edit|Write|NotebookEdit|Bash`.

The orchestrator runs with `--permission-mode auto`; agents inherit that
mode whatever their own `permissionMode` says, so the mode is not a lever
here either.

## Matrix

| Actor | Action | Wanted | Enforced by | Verified |
|---|---|---|---|---|
| orchestrator | `Edit`/`Write`/`NotebookEdit` inside the repository | refused | `orchestrator-guard.sh`, path under the repo root and not under `.claude/worktrees/` | synthetic: `check-orchestration.sh`; live: see below |
| orchestrator | `Edit`/`Write` in the scratchpad (briefs) | allowed | same hook, path outside the repo | synthetic |
| orchestrator | `git commit`, `merge`, `rebase`, `cherry-pick`, `revert`, `am`, `apply`, `tag`, `push` | refused | same hook, regex on the command | synthetic |
| orchestrator | `gh pr merge/create/edit/close`, `gh release …`, `gh workflow run` | refused | same hook | synthetic |
| orchestrator | `sed -i`, `perl -i`, redirection or `tee` into a repository path, `cp/mv/rm/touch/mkdir/install` into its checkout | refused | same hook | synthetic |
| orchestrator | `git -c x commit`, `/usr/bin/git commit`, `sh -c '…'`, `python -c` writing a file | **not refused** | contractual only: the regex is best effort (the official documentation says Bash rules are not a security boundary) | synthetic (documented bypass) |
| orchestrator | reading, `git log/diff/status`, `gh pr view/checks`, `check-docs.sh`, `git worktree add` | allowed | hook lets it through | synthetic |
| any agent (`agent_id` present) | everything above | allowed by the hook; bounded by its role file | contractual (role text, brief, diff review by the merger) | — |
| `herd-planner`, `herd-reviewer` | `Edit`, `NotebookEdit`, `Agent` | removed | frontmatter `disallowedTools` (removes the whole tool) | live V7 |
| `herd-planner`, `herd-reviewer` | `Write` outside their one handoff file | refused | contractual | — |
| `herd-step`, `herd-merger` | `Agent` (re-delegation) | removed | frontmatter `disallowedTools` | live V2 |
| `herd-step` | `main`, PR, tag, release, other worktrees | refused | contractual; branch protection makes a push to `main` fail on GitHub's side | live V2 |
| `herd-merger` | merge without ORCH-001 or Alexa's word | refused | contractual; branch protection requires green `check` and `docs` | live V4 |
| `herd-release` | `git tag`, tag push, `gh release`, `gh workflow run`, `gh api …dispatches`, docker, systemctl, ssh, sudo | refused | frontmatter hook `release-guard.sh` | synthetic; live V10 |
| anyone | `--no-verify`, force-push, red merge | refused | branch protection (force-push and deletion blocked; `check` and `docs` required); `--no-verify` contractual | GitHub API read 2026-10-24 |
| anyone | the consumer's instance, `/etc/nixos`, containers | refused | contractual (D161); the shared `CLAUDE_CONFIG_DIR` currently *allows* sudo for chatOnS's session — ORCH-030 | — |

## Disabling and false positives

- `touch .claude/hooks/DISABLED` switches both hooks off without editing
  settings; remove the file to switch them on. The hooks fail **open**
  on unparseable input or a missing `perl`, so they can never immobilise
  a session.
- A false positive blocks only the one call and prints why; the
  orchestrator's remedy is to brief an agent, which is the intended
  behaviour. Legitimate inspection (`git log`, `grep 'git commit'`,
  `gh pr view`, `check-docs.sh`, `ls`, `cat`) is on the allowed list of
  `check-orchestration.sh`, which must stay green.
- `.claude/hooks/.log-to` containing a file path makes the guard append
  one line per call (`agent=main|<id>`, tool, path, command) to that
  file: the trace used for V11 and V12. Remove the file afterwards.

## Live verification

Filled in by step H7 (V2, V4, V7, V10, V11); see
`history/2026-10-24-orchestration-remediation.md`.
