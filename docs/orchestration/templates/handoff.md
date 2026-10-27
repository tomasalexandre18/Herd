# Handoff template

Saved as `docs/orchestration/history/<YYYY-MM-DD>-<task>.md` on the task's
branch by the agent that did the work. One file per task; never append to
another task's file.

```markdown
# <task> — handoff <YYYY-MM-DD>

- **Item:** ORCH-nnn
- **Agent:** herd-<role>
- **Branch / commit:** <name> / <sha>, based on origin/main <sha>
- **PR:** #<n> or none

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
