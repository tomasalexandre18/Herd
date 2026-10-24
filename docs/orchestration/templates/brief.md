# Brief template

Copy, fill every line, keep it short. English.

```text
Role:       herd-<planner|step|merger|release|reviewer>
Mission:    ORCH-nnn — <observable end state, one sentence>
Base:       origin/main at <sha>; branch <name>; worktree <path>
Scratchpad: <session scratchpad>/<task>/   (yours alone)
Read, in order:
  1. CLAUDE.md
  2. docs/orchestration/board.md (item ORCH-nnn)
  3. <the two or three files the task really needs>
Hypotheses to treat as such: <what the brief assumes and has not proven>
Write only: <paths>; everything else is read-only
Forbidden:  main, tags, releases, other worktrees, /etc, containers,
            services, the consumer's files, pkill -f / kill by name,
            cargo unless the change touches code
Gates:      <exact commands>, or "check-docs.sh only; Rust gates not run"
Stop when:  <done criterion> or a decision is needed; then write
            docs/orchestration/history/<date>-<task>.md from
            templates/handoff.md and report
Report:     branch, commit, diff summary, gate output verbatim, skips,
            not done, risks, decisions needed, resume point
```
