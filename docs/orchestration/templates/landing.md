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

The record is written twice, in the same push:

- as the **merge commit message** (`git merge --no-ff -F <file>`), which
  is the record itself;
- inside `herd-merger`'s own handoff,
  `docs/orchestration/history/<YYYY-MM-DD>-merge-<batch>.md`, which also
  carries the **verbatim** `gate.sh` output file and the full skip list.
  `check-docs.sh` asserts that this file carries the record, filled,
  because a commit message is not in the tree a gate can read.

A reader who was not there then needs one `git show` and one file.

### The two copies are not identical, and must not be asked to be

**The merge message is a strict superset of the committed copy. Nothing
in the committed copy may contradict it.** That is the rule (Alexa,
2026-10-29), and this section exists so that no future landing re-derives
it from scratch; batch 8's merger reached the same answer unaided and
said so in its record.

It follows from ORCH-121, which makes the merger's handoff **the last
change before the gate**. The handoff is therefore inside the tree the
gate runs on, and is committed before the gate has run and before the
merge commit exists. So the committed copy cannot carry anything that
only exists afterwards:

| Only in the merge message | Why it cannot be in the committed copy |
|---|---|
| the gated sha as 40 hex | that sha is the commit that **adds the handoff**; writing it inside the handoff changes the content being hashed. Name it instead as "`<sha of the commit before>` plus this handoff commit, which is the merge's second parent": exact, self-resolving, and it satisfies `check-docs.sh`, which wants a hex string on the **Gate:** line |
| the gate's verbatim verdict line (`gate: GREEN sha=… steps=…`) | the gate had not run |
| the receipt path under `target/herd-gate/<sha>.receipt` | named for that same sha, and per worktree, never copied into the tree |
| the skip-list sha256 and the resolved `load-excused-skips` figure | computed by the run that had not happened |
| the `ci.yml` run id, when the push starts it | the push had not happened; the committed copy says "none yet -- this push starts it", and the next landing is what records it, in its own *CI run* and in its board edit |

One figure is the exception that proves the rule, and it is deferred
nowhere: **the previous landing's run**. That push has happened, so its
id and its conclusion exist before this record is written, and they go
in **both** copies in full. *CI run*, below, has the fields and the
check that reads them.

The committed copy carries everything else in full: the seven sections,
the batch and its branches, the skip **names** and the reasoning. Where a
field can only be resolved later, it says what will resolve it and where
that resolution lives — the merger's report and the merge message — and
never leaves a placeholder. A reader who has only the handoff must be
able to tell which fields are deferred and that they were deferred on
purpose.

What is still forbidden: a committed copy that **states something the
merge message contradicts**. A different verdict, a different skip count,
a different batch, a different list of branches, a gate described as
green that the message reports red. The superset is a licence to be
shorter, never a licence to differ.

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
- **CI run:** <id> — recorded, not waited on; see *CI run* below
- **Previous run:** batch <n-1> · `ci.yml` <id> · <verdict> — read back
  by id. One bullet, this exact label, no placeholder left in it: a gate
  reads this line and nothing else for that figure

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

The resolved sha, the verbatim verdict line, the receipt path and the
skip-list hash belong here **in the merge message**, and in the committed
copy are named as deferred, for the reason *The two copies are not
identical* gives above.

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

Then, in its own bullet, **the previous landing's run**, in this one
labelled field and nowhere else:

- **Previous run:** batch 33 · `ci.yml` 41228366473 · success

A landing cannot record its own run — the push that starts it comes
after the tree is frozen — so the next landing is the only place that
run can be written down, and this field is that place. Take the id from
the previous merger's report, or find it with
`gh run list --workflow ci.yml --branch main --limit 10` matched to that
landing's merge sha; then read the conclusion back **by id** with
`gh run view <id>`, never off a list line. Use GitHub's own conclusion
word (`success`, `failure`, `cancelled`, `timed_out`, `skipped`,
`neutral`, `stale`, `action_required`), and write the literal
`no ci.yml run` in the field when that push started none — a statement,
not an omission. Unlike this landing's own run, this one is **not**
deferred: it goes in the committed copy too.

What the field is, exactly, because a gate reads it:

- one top-level bullet, opening `- **Previous run:**` — that label, that
  capitalisation;
- carrying the **previous** landing's batch number, a `ci.yml` run id of
  at least `run_digits` digits (8; real ids are 11), and one conclusion
  word for that run — or the literal `no ci.yml run`;
- with no `<placeholder>` left anywhere in it;
- exactly one such bullet in the record. Two are refused rather than
  resolved: a record with two answers has not got one.

The separator is cosmetic — `·`, a comma, a dash, words. The field may
wrap over as many lines as it needs, and may carry a nested sub-bullet
under it for the figure; what ends it is a line that is not indented, or
a blank line. A fenced block is **not** the field: paste the `gh run
view` output into your handoff as evidence if you like, but the claim is
this bullet. A markdown table is not the field either.

`check-orchestration.sh` reads it, under *a landing records its
predecessor's post-push run* (ORCH-333), from the newest
`<date>-merge-<batch>.md` in `docs/orchestration/history/`, and it runs
at step 2 of `gate.sh` — before any cargo step, so a missing field costs
about a minute and not a compile. It is required of landings dated
**2026-11-01 or later** (`lr_from`); records written before the field
existed are printed as a skip with that reason and never refused. If the
previous landing is not named `-merge-batch-<n>.md` — a hotfix, a
re-gate under another word — there is no batch number to ask for, and
that too is a printed skip, not a failure.

Why a labelled field and not a sentence in the prose. The first shape of
this check asked whether some paragraph of the record mentioned the
predecessor's batch near eight digits and a verdict word, and
`herd-reviewer` measured three ways to pass it without recording
anything: a table whose predecessor row said *not read yet* (a table has
no blank line, so it is one paragraph), the word *successful* beside a
byte count, and a record merely quoting one of this check's own case
names. A control that goes green on the defect it was built for ends the
vigilance that was holding the rule, which is how batch 32 happened.
The field cannot be satisfied by accident; that, and not truth, is what
it buys (ORCH-335, 2026-10-31).

What that check cannot do: it has no network, so it never establishes
that the run exists, that its head sha is the one named, or that the
verdict is true. A mistyped digit passes; a false verdict passes. It
asks only that somebody went and looked and wrote down what they saw —
*What a landing is*, in `workflow.md`, is where the truth of the verdict
stays a human's.
```
