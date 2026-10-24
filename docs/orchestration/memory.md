# Memory, context and resumption

What each Claude session actually loads, where the owner's lessons live,
and how a fresh session resumes. Verified on Claude Code 2.1.303 on
2026-10-24 against the official documentation and this host.

## What is loaded, by role

| Source | Orchestrator (main session) | Execution agent (`Agent` tool) |
|---|---|---|
| `CLAUDE.md` (repo root) | yes | yes, unless `omitClaudeMd` or built-in Explore/Plan |
| `.claude/rules/*.md` | yes | yes |
| `.claude/agents/herd-*.md` | the one named by `--agent`, if any | its own definition is its system prompt |
| Auto memory `<CLAUDE_CONFIG_DIR>/projects/<slug>/memory/MEMORY.md` | yes (first lines) | **no** |
| `<CLAUDE_CONFIG_DIR>/settings.json` (user) | yes, including `autoMode` | yes (permissions and hooks) |
| `.claude/settings.json` (project) | yes | yes; `permissions.deny` applies to agents too |
| `docs/orchestration/*` | only what it reads | only what the brief names |

Consequences:

- A rule that only lives in auto memory reaches no execution agent. Every
  essential rule is therefore in `CLAUDE.md` or an agent file; memory is
  a reminder for the orchestrator, never the only copy.
- A project-level `permissions.deny` on `Edit` would paralyse `herd-step`.
  Orchestrator-only restrictions are a hook that distinguishes the main
  thread from an agent (`docs/orchestration/permissions.md`).
- The orchestrator runs with `--permission-mode auto`; an agent spawned
  from it inherits that mode whatever its own `permissionMode` says.

## The two memory directories, reconciled

Old, no longer loaded (`CLAUDE_CONFIG_DIR` points elsewhere):
`/home/server/.claude/projects/-mnt-ssd-980-herd-project/memory/`,
three files dated 2026-10-15. Current, loaded:
`/mnt/ssd_980/chatons/claude/config/projects/-mnt-ssd-980-herd-project/memory/`
— four files dated 2026-10-23. Neither is deleted or edited by this work.

| Lesson | Old (10-15) | Current (10-23) | Versioned as |
|---|---|---|---|
| orchestrate, do not code | `orchestrate-do-not-code.md` | `always-delegate.md` (stricter: "every file edit") | `gotchas/orchestrator-does-not-edit.md`, `CLAUDE.md` Roles |
| cargo needs nix develop | `herd-cargo-needs-nix-develop.md` | absent | `gotchas/cargo-needs-nix-develop.md`, `CLAUDE.md` Commands |
| docs-only gates | absent | `docs-only-gates.md` | `gotchas/docs-only-gates.md`, `CLAUDE.md` Commands |
| own scratchpad, no pkill | absent | `subagent-brief-hygiene.md` | `gotchas/own-scratchpad-no-pkill.md`, `CLAUDE.md` Shared host |
| one-at-a-time limit lifted | `orchestrate-do-not-code.md` | implied by `always-delegate.md` | `CLAUDE.md` Roles; concurrency cap in `workflow.md` |

No contradiction between the two sets; the current set supersedes the old
by being stricter. The old `orchestrate-do-not-code.md` also says "Keep
for yourself: … `docs/decisions.md`, merging branches" — that part is
**replaced**: both are agent tasks now (`herd-merger`).

## Proposed `MEMORY.md` for the orchestrator (not applied: outside the repo)

The current directory is outside the repository and this remediation
does not write there. The orchestrator may apply this itself; it is a
pointer file, not a rule store.

```markdown
- Contract: `CLAUDE.md`; roles: `.claude/agents/`; procedure:
  `docs/orchestration/workflow.md`; open work and waiting decisions:
  `docs/orchestration/board.md`. Resume from the latest file in
  `docs/orchestration/history/`, not from a conversation summary.
- Lessons with dates and evidence: `docs/orchestration/gotchas/`.
  The three 2026-10-23 memory files are kept there; do not re-derive.
```

## Fresh session procedure

1. Read `CLAUDE.md` (≤ 150 lines).
2. Read `docs/orchestration/board.md`: *Waiting* first, then *Active*,
   *Blocked*, *Owed*, *Next*.
3. Read the newest file in `docs/orchestration/history/` for the task
   being resumed; it names the branch, the commit and what is not done.
4. Do not trust a `/compact` summary for state: if it disagrees with the
   board or a handoff, the files win and the discrepancy is reported.
5. Before launching an agent, check the board for a running item on the
   same files; one integration at a time.

## Configuration backup and migration check

- Back up only `<CLAUDE_CONFIG_DIR>/projects/<slug>/memory/` and
  `<CLAUDE_CONFIG_DIR>/settings.json`; never the whole config directory
  (`.credentials.json`, `history.jsonl` and transcripts are sensitive) and
  never into Git.
- After moving `CLAUDE_CONFIG_DIR`: `/status` shows the setting sources,
  `/memory` lists the memory files; both must name the new directory, and
  the orchestrator states it in its first report. The existence of the
  files is not proof they are loaded.
