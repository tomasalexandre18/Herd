# Brief template

Copy, fill every line, keep it short. English.

```text
Role:       herd-<planner|step|merger|release|reviewer>
Mission:    ORCH-nnn — <observable end state, one sentence>
Base:       origin/main at <sha>; branch <name>
Worktree:   <absolute path>   (required; yours alone; create it with
            git worktree add -b <branch> <path> origin/main)
Scratchpad: <absolute path>/<task>/   (required; yours alone)
Read, in order (in your worktree, never the orchestrator's checkout):
  1. CLAUDE.md
  2. docs/orchestration/board.md (item ORCH-nnn)
  3. <the two or three files the task really needs>
Hypotheses to treat as such: <what the brief assumes and has not proven>
Write only: <paths>, inside your worktree; everything else is read-only
Forbidden:  main, tags, releases, other worktrees, the orchestrator's
            checkout /mnt/ssd_980/herd-project, /etc, containers,
            services, the consumer's files, pkill -f / kill by name,
            cargo unless the change touches code
Gates:      <exact commands>, or "check-docs.sh only; Rust gates not run";
            each one redirected to a file, never piped to head or tail
            (why: workflow.md, "Failure"); the docs gate runs last, after
            the handoff is committed and before the push, and the output
            quoted in the handoff is that run's
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

That checkout is also **stale by construction**, and reading it is the
second half of the same trap. The guard blocks every git write verb
there, `git fetch` included, so it cannot be brought up to date and never
will be. On 2026-10-27 it stood 150 commits behind `origin/main`, and its
`board.md` held 18 rows ending at ORCH-032 while `origin/main` carried 85
up to ORCH-107 — an agent that read it reported a board row as missing
that had been on `main` for days, and the orchestrator reports two
earlier briefs written from a stale `main`. So: read `origin/main` for
anything you will act on or assert, with `git show origin/main:<path>` or
a `git fetch` in your own worktree. That checkout stays readable only
for what does not change, such as a file's history.
