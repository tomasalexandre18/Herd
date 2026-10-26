# Brief template

Copy, fill every line, keep it short. English.

```text
Role:       herd-<planner|step|merger|release|reviewer>
Mission:    ORCH-nnn — <observable end state, one sentence>
Base:       origin/main at <sha>; branch <name>
Worktree:   <absolute path>   (required; yours alone; create it with
            git worktree add -b <branch> <path> origin/main)
Scratchpad: <absolute path>/<task>/   (required; yours alone)
Read, in order:
  1. CLAUDE.md
  2. docs/orchestration/board.md (item ORCH-nnn)
  3. <the two or three files the task really needs>
Hypotheses to treat as such: <what the brief assumes and has not proven>
Write only: <paths>, inside your worktree; everything else is read-only
Forbidden:  main, tags, releases, other worktrees, the orchestrator's
            checkout /mnt/ssd_980/herd-project, /etc, containers,
            services, the consumer's files, pkill -f / kill by name,
            cargo unless the change touches code
Gates:      <exact commands>, or "check-docs.sh only; Rust gates not run"
Stop when:  <done criterion> or a decision is needed; then write
            docs/orchestration/history/<date>-<task>.md from
            templates/handoff.md and report
Report:     branch, commit, diff summary, gate output verbatim, skips,
            not done, risks, decisions needed, resume point
```

`Worktree:` is required, absolute, and never the orchestrator's checkout
`/mnt/ssd_980/herd-project`. The guard exempts agents by design
(`../permissions.md` "Live verification"), so nothing but this line keeps
an agent out of that checkout: in V11 a brief that named no worktree let
its agent edit and stage `docs/status.md` there. A brief kept as evidence
under `docs/orchestration/history/` is named `<task>-brief.txt`, and
`check-docs.sh` rejects one that names no worktree. A brief whose agent
writes nothing in the repository says `read-only` on its `Base:` line
instead; "everything else is read-only" on the `Write only:` line is a
different statement and does not exempt it.
