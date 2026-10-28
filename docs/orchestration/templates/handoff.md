# Handoff template

Saved as `docs/orchestration/history/<YYYY-MM-DD>-<task>.md` on the task's
branch by the agent that did the work. One file per task; never append to
another task's file.

The docs gate is the **last thing run** — after this file is committed,
before the push — and the output under *Gates* is that run's: a handoff
is itself a docs file, so a gate run before it was written tested a
different tree.

`herd-merger`'s own handoff, named `<YYYY-MM-DD>-merge-<batch>.md`, carries
one more thing: the filled landing record of `templates/landing.md`, which
is the merge commit's message and the audit trail of the push to `main`.
`check-docs.sh` asserts it is there and filled.

```markdown
# <task> — handoff <YYYY-MM-DD>

- **Item:** ORCH-nnn
- **Agent:** herd-<role>
- **Branch / commit:** <name> / <sha>, based on origin/main <sha>
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done
<what exists now, as facts a reader can check>

## Not done, and why
<each item with its reason; "nothing" if none>

## Gates
<verbatim output, or "check-docs.sh: docs ok; Rust gates NOT RUN
locally (prose only), CI runs them">
<skips, each one named; "no skip" if none>

## Risks and findings
<what could be wrong, what contradicts the brief, with evidence>

## Decisions needed
<question — recommendation — fallback; or "none">

## Resume point
<the exact next command or task for whoever continues>

## Processes
<PIDs started and ended, or "none">
<own `target/` deleted once the branch was pushed, with the path; why not, if not — the reason is in workflow.md "Concurrency and host rules">
```
