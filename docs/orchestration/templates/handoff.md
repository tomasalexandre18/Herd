# Handoff template

Saved as `docs/orchestration/history/<YYYY-MM-DD>-<task>.md` on the task's
branch by the agent that did the work. One file per task; never append to
another task's file.

The gate is the **last thing run** — after this file is committed, before
the push — because a handoff is itself a docs file and a gate run made
before it was written tested a different tree. **So the output of that
last run cannot be in this file**, and the two sentences this paragraph
used to carry ("the last thing run" and "the output under *Gates* is
that run's") could not both hold. The resolution is the one
`.claude/agents/herd-merger.md` step 6 already uses, and it is the same
for every agent:

- *Gates* **names** the run that is about to happen on the committed
  tree — the command, and where its output will be: the log or receipt
  path for `./scripts/gate.sh`, the output file for
  `./scripts/check-docs.sh`.
- The **verbatim output goes to the agent's report** and to that file,
  not into the tree it has just gated. Nothing is added to the tree
  afterwards, and a second commit to carry the output would need a
  second gate run.
- Output quoted in *Gates* is from the runs made **before** this file
  was committed — the Rust gates and any earlier docs run, which is most
  of the evidence — and it says which run each block came from.
- A red last run is reported red, in the agent's report, and the branch
  is not pushed.

A drafted `docs/decisions.md` entry and a drafted `board.md` row go in
**this file, in full**: the whole text the merger will paste, headed
`D-NEXT` for a decision, since the merger allocates the number and gaps
stay (D148, invariant 8). Not a pointer, not *"see the agent's
report"*, not a summary of what the entry would say. **The reason is
that the merger reads the tree and nothing else**: the orchestrator's
conversation and an agent's hand-back report are not artefacts
`herd-merger` can read, so anything the merger must act on has to be in
a file on the branch.

Measured once, on 2026-10-29: `spool-offer-fix`'s handoff deferred its
drafted entry to *"the agent's report"*, so `herd-merger` had to
compose D221 out of that handoff's *Done* section, and said in
`history/2026-10-29-merge-batch-10.md` that the wording was not its
author's. The reconstruction turned out faithful. Nothing in the method
had made it so.

This template said nothing close before. Its one decision-shaped
section was *Decisions needed*, which is for a question somebody else
must answer, not for an entry already written; that section is
unchanged, and *Drafted for `herd-merger`* now stands beside it.

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
<verbatim output of the runs made before this commit, or "check-docs.sh:
docs ok; Rust gates NOT RUN locally (prose only), CI runs them">
<the last run, named and not quoted: the command, and the log or receipt
path its output goes to; the output itself is in the agent's report>
<skips, each one named; "no skip" if none>

## Risks and findings
<what could be wrong, what contradicts the brief, with evidence>

## Drafted for `herd-merger`
<the full `docs/decisions.md` entry this task drafts, headed `D-NEXT`,
and the full text of every `board.md` row it proposes — verbatim, never
a pointer to the agent's report and never a summary, because the merger
reads the tree and nothing else; "none" if the brief asked for neither>

## Decisions needed
<question — recommendation — fallback; or "none">

## Resume point
<the exact next command or task for whoever continues>

## Processes
<PIDs started and ended, or "none">
<own `target/` deleted once the branch was pushed, with the path; why not, if not — the reason is in workflow.md "Concurrency and host rules">
```
