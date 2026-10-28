# Landing record template

A **landing** is one batch of branches merged onto `main` and pushed by
`herd-merger`. The landing record is that merge commit's message. It
replaces the pull-request body as the audit trail of a change to `main`
(2026-10-28): with no pull request, nothing else carries the gate verdict,
the branches of the batch and the decisions allocated.

It is attached to the thing it documents, `git log --merges main` reads it
with no network and no GitHub account, and it cannot be rewritten after
the fact — force-push and deletion on `main` stay refused server-side,
whereas a pull-request body can be edited silently. That is why the merge
commit, and not a new journal file under `docs/`, is where the evidence
lives.

The record is written twice, identically, in the same push:

- as the **merge commit message** (`git merge --no-ff -F <file>`), which
  is the record itself;
- inside `herd-merger`'s own handoff,
  `docs/orchestration/history/<YYYY-MM-DD>-merge-<batch>.md`, which also
  carries the **verbatim** `gate.sh` output file and the full skip list.
  `check-docs.sh` asserts that this file carries the record, filled,
  because a commit message is not in the tree a gate can read.

A reader who was not there then needs one `git show` and one file.

Seven sections: the six of the pull-request template this replaces,
adapted to a landing, plus *CI run*, which a pull request did not need
because GitHub ran its checks before the merge and somebody watched them.
Keep all seven, keep this order, and write "nothing" rather than drop one.
A commit message is plain text, so the record carries no fenced block; the
verbatim output lives in the handoff, under its own fences.

```text
Land batch <n>: <subject, imperative English, ≤ 72 chars>

- **Batch:** <n>, landed <YYYY-MM-DD> by herd-merger
- **Gate:** green — ./scripts/gate.sh on <sha>, this commit's second
  parent, `git diff --quiet <sha> HEAD` clean; receipt <path>
- **Skips:** <n>, each named under *Evidence* and printed as a skip
  (D27); "no skip" if none
- **CI run:** <id> — recorded, not waited on

## What changed and why

Which milestone this moves and which `E-nn` expectations come with it;
then, in two or three lines, what `main` can do now that it could not
before. A batch is not permission for one blurred summary.

## Branches in this batch

One line per branch, in the order they were merged: branch name, its
`ORCH-nnn`, the path of its handoff under `docs/orchestration/history/`.
A single branch is a batch of one.

## Evidence

The gate's verdict and the sha it was run on. That sha is this merge
commit's **second parent**, and its tree must be identical to the merge's
(`git diff --quiet <gated sha> <merge sha>`): the merge commit's message
is this record, so its sha cannot exist before the gate runs, and a
record demanding a gate on "the sha pushed" permits no landing at all
(measured 2026-10-28; `CLAUDE.md` *Git and main*, `workflow.md` *What a
landing is*, `.claude/agents/herd-merger.md` steps 7 and 8). An identical
tree is what makes the receipt bind: a gate run on another tree proves
nothing about this one. One verdict block per branch, named for it, plus
the batch's own run on the merged tree. Claiming it passed is not
evidence.

Every skip is named and shown as a skip, never counted into a pass (D27,
invariant 5). A count is not a signal, the list is: paste the
`HERD-SKIPPED:` lines, or the diff against `scripts/expected-skips/`,
and say "no skip" when there is none.

## Left out

What this landing deliberately does not do, and where it is meant to
land instead.

## Decisions allocated

Each `D-NEXT` entry this batch brought, with the number allocated to it
and its title. `herd-merger` allocates; numbers are identifiers and gaps
stay (D148, invariant 8). "Nothing" if the batch records no decision.

## Board rows

Every `ORCH-nnn` this landing moves, with its new state. Each branch's
own handoff is listed under *Branches in this batch* above.

## CI run

The run id of the `ci.yml` run this push started, from
`gh run list --workflow ci.yml --branch main --limit 1`, recorded here
**and not waited on** — that wait is what the local gate exists to stop
paying. Say which was done: recorded, or waited on by choice because the
landing was judged risky. Its verdict is read at the next session start,
and a failure becomes an `ORCH-nnn` row before new work is briefed.
```
