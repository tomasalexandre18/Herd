# orch-337-merger-field — handoff 2026-10-31

- **Item:** ORCH-337
- **Agent:** herd-step
- **Branch / commit:** `orch-337-merger-field` / the commit this file is
  in, based on `origin/orch-335-labelled-previous-run` `46d0611` —
  **not `main`**: the oracle this file aligns to exists only on that
  branch, so branching off `main` would have aligned the agent file to
  a check that is not there.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One file of rules and one board row. No gate script, no template and no
`workflow.md` was touched: ORCH-335 had already made all four say the
new thing, and the only document left saying the old thing was the
agent file that tells the merger what to write.

**`.claude/agents/herd-merger.md`, step 5's *CI run* paragraph.** It
said the record must name the batch, the id and the verdict *"inside
one paragraph"*, and spelled out that a paragraph ends at a blank line,
at the next bullet or at the next heading. It now names the one
labelled field that may hold the claim,
`- **Previous run:** batch <n-1> · ci.yml <id> · <verdict>`, with the
four properties the gate checks — one top-level bullet, that label and
capitalisation, no `<placeholder>` left in it, exactly one such bullet
in the record — and the two rules about its extent: it may wrap over as
many lines as it needs and may carry a nested sub-bullet, and what ends
it is a blank line or a line that is not indented. Three sentences were
added rather than reworded, each answering one of the brief's three
points:

- **The field is the claim and a fence is only evidence.** Said in
  those words, because `templates/landing.md` *CI run* draws that line
  explicitly and the agent file must not draw a different one. With the
  consequence named: a `gh run view` paste in a fenced block does not
  satisfy the field, and neither does a paragraph of prose or a table
  row — batch 33's record put both of its owed runs in a fence, and a
  record like it now fires, deliberately.
- **The floor, and who it makes first.** Required of landings dated
  **2026-11-01 or later** (`lr_from`); batch 34's record predates it
  and is not rewritten, so the first landing on or after that date is
  the first that must carry the field — *"very likely yours"*, since
  the next landing is that landing.
- **What the check does not do.** The old sentence (*"cannot tell a
  verdict you read from one you invented"*) was true but understated:
  it is now *"no network and no toolchain: it asks that the field is
  there and well shaped, never that the verdict is true, so a mistyped
  digit passes"*, and it ends by citing where `workflow.md` says the
  comparison is a human's (*No docs gate can check the record
  itself*). The step also now says the case runs at step 2 of
  `gate.sh`, **before any cargo step**, so the cost of a missing field
  is about a minute.

**Step 6**, which never mentioned a paragraph, needed one word: *"Your
predecessor's is a different figure and it stays, from step 5"* became
*"a different figure, carried by step 5's field, and it stays"*. The
step's own subject — the handoff is the last change to the tree — is
untouched.

**Step 9**, the third mention, judged on its merits as the brief asked:
it was **not** about a different thing. *"the **next** landing is what
records it — in its own *CI run* paragraph and in its board edit"*
describes where the successor must write **this** landing's run, which
is the same field under a retired name, so leaving it would have
taught the retired form on the receiving end of the same chain. It now
reads *"in the `- **Previous run:**` field of its own *CI run* section,
and in its board edit"*. The board edit stays as practice, which is
what `workflow.md` *The next landing writes that run into its own
record* says.

After the change the word *paragraph* survives once in the file, in the
sentence that says a paragraph of prose does **not** satisfy the field.

**`docs/orchestration/board.md`:** ORCH-337 `owed` → `done`, with its
action cell replaced by what was done and its reference cell converted
from line numbers to headings and quoted phrases (**D289**, applied on
touch: the row cited `.claude/agents/herd-merger.md:57`-`:70`,
`:78`-`:86`, `:128`-`:137`, three ranges that this very change moves).
The row stays where it is, in *Open*, as ORCH-333, ORCH-335 and
ORCH-336 do with the same state. No other row touched; no new row
needed, so none was taken from ORCH-339.

**Citations into the file, checked.** `.claude/agents/herd-merger.md:`
appears in seven places in the tree. One is live and is the row above.
The other six are inside dated handoffs under
`docs/orchestration/history/` (`2026-10-27-orch-record-fixes.md`,
`2026-10-28-method-landing-record.md` twice,
`2026-10-28-merge-method.md`, `2026-10-31-orch-333-figure-and-record.md`
twice, `2026-10-31-orch-align-record-run.md`) and are **deliberately
left alone**: a dated record is evidence of what was read on its date,
and D286's precedent is that a stale figure in one is answered by a new
entry, never by an edit. Three of them (`:36`, `:47`, `:86`) point
above the hunk and still resolve; the three at `:100`-`:107` in
`2026-10-31-orch-333-figure-and-record.md` and
`2026-10-31-orch-align-record-run.md` already pointed at a step 8 that
had moved before this task started.

## Not done, and why

- **`docs/decisions.md` is untouched, and no entry is drafted.** The
  brief forbids a second one: ORCH-335 already drafted `D-NEXT` for
  the oracle's shape (*a claim a machine checks is a labelled field,
  never prose*), and this task propagates that decision rather than
  taking one. Nothing here is a new rule.
- **`scripts/check-orchestration.sh`, `scripts/check-docs.sh`,
  `templates/landing.md` and `workflow.md` are untouched.** Out of
  scope and already consistent with the new oracle; each was read to
  take its wording from, none was edited.
- **ORCH-338 is left open.** Only the newest landing record is ever
  asked for the field, and the per-file half belongs in
  `check-docs.sh`. Not this task's.
- **Batch 34's merge record is not amended**, as ORCH-335 already
  stated: it was written under the paragraph rule and the floor exists
  so that it is not refused for it.
- **No `herd-reviewer` pass was requested for this change**, although
  `workflow.md`'s step-6 list names agents. The pass exists and covers
  it: see *Risks and findings*.

## Gates

**Prose only.** Everything changed is under `.claude/` and `docs/`,
both exempt (`CLAUDE.md`, *Gates before a commit*), so
`./scripts/check-docs.sh` is the whole of what is owed and **the four
Rust gates were NOT run locally** — not `./scripts/test.sh`, not
`cargo clippy`, not `cargo fmt --check`, not a build. What covers them
is the batch's own `./scripts/gate.sh` run at integration, which is
`herd-merger`'s and not an agent's. A gate that did not run is reported
as not run and never as passed. No `cargo`, no Herd job, no
`CARGO_TARGET_DIR`, no build tree: this task compiled nothing.

Run made **before** this file was committed, on the tree holding the
agent-file and board edits:

```text
$ ./scripts/check-docs.sh
docs ok                                            (exit 0)
```

**The last run, named and not quoted** — it runs after this file is
committed, on the tree that is pushed, so its output cannot be in this
file (`templates/handoff.md`): `./scripts/check-docs.sh` on this
branch's tip, its verbatim output in the agent's report.

`./scripts/check-orchestration.sh` was re-run as a control, not as an
owed gate: the base carries `350 passed, 0 failed, 1 skipped` and a
difference would have been a finding (ORCH-336 removed the host's load
from its verdict). Its output is in the agent's report.

**Skips:** the one the base already prints — `landing: the real tree's
newest landing record carries it`, skipped because
`2026-10-31-merge-batch-34.md` predates the field required of landings
dated 2026-11-01 or later. It is **not** a pass (D27). The suite's own
skip list is unchanged by this branch: no `#[ignore]` and no
`HERD-SKIPPED` was added, moved or reworded, and no `.rs` file is
touched.

## Risks and findings

- **The review that covers this change exists and this is the
  citation.** `workflow.md` requires a `herd-reviewer` pass for a
  change to `CLAUDE.md`, **agents**, permissions, hooks or a gate
  script, and an agent file is in that list. The adversarial pass is
  `herd-reviewer`'s of 2026-10-31 on `orch-333-figure-and-record`,
  which extracted the paragraph oracle's `awk` and measured three ways
  to pass it with nothing recorded; **it is not in this tree** — it
  lives in the orchestrator's scratchpad as
  `2026-10-31-review-orch-333-guard.md`, and the tree's own record of
  it is ORCH-335's board row and handoff, which quote its three
  measurements. Its F1 recommendation — a labelled field, not prose —
  is
  what ORCH-335 implemented and what this task propagates into the one
  document that still taught the retired form. **Nothing here was
  designed:** every sentence added comes from ORCH-335's *Drafted for
  `herd-merger`* text, from `templates/landing.md` *CI run* or from
  `workflow.md`, and where those three use a form of words it is the
  one used here. Had the propagation required a choice the review did
  not make, it would have stopped and said so.
- **One wording is mine and not ORCH-335's draft, and it is the floor
  sentence.** The draft ended with *"And: the field is required of
  landings dated 2026-11-01 or later"*, a bare fact; the brief asked
  that the agent file say plainly that the next merger is the first
  that must write the field. The file says it as *"batch 34's record
  predates it and is not rewritten, so the first landing on or after
  that date is the first that must carry the field — very likely
  yours"*. **This dates:** once several landings have carried the
  field, *"very likely yours"* is false for every merger but the first.
  It is a sentence to retire after the first landing on or after
  2026-11-01, not a rule, and it is flagged here rather than left to be
  discovered.
- **The gate cannot see this change, so nothing proves the next merger
  reads it.** `check-orchestration.sh` checks landing records, not
  agent files; no case asserts that `herd-merger.md` teaches the form
  the gate enforces, and no case can without reading prose, which is
  the very thing ORCH-335 retired. What holds this alignment is the
  same thing that broke at ORCH-337: a human noticing. The honest
  mitigation, if one is wanted, is that the next merger's brief relays
  the field's one line as well — ORCH-335's own fallback — which costs
  nothing and does not depend on this file being read.
- **The `·` separators in the field's example are cosmetic**, as
  `templates/landing.md` says, and the agent file reproduces the
  template's exact example so that a merger copying it gets a shape the
  gate accepts. A merger who prefers a comma or a dash is not refused.
- **Nothing in the brief was contradicted by the tree.** The three
  mentions are where the brief said they were, the third was about the
  same chain, the base's `350 passed, 0 failed, 1 skipped` is what it
  said, and D289 is in `docs/decisions.md` on this branch so the board
  may cite it.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** Deliberately: ORCH-335's handoff
already drafted the `D-NEXT` for this oracle and a second entry for the
same decision would be two numbers for one choice (D148, invariant 8).
Nothing in this task is a new rule.

**The `board.md` row**, written into `docs/orchestration/board.md` on
this branch in place of the existing ORCH-337 row, in *Open*, verbatim
and in full — nothing else in that file is touched:

```markdown
| ORCH-337 | **`.claude/agents/herd-merger.md` still tells the merger to write the predecessor's run as a paragraph, which ORCH-335 retired, so the next landing's own gate goes red at step 2 for following its instructions.** Steps 5, 6 and 9 say the record must name the batch, the id and the verdict *"inside one paragraph"* and spell out that a paragraph ends at a blank line, at the next bullet or at the next heading. After ORCH-335 the only accepted form is the `- **Previous run:**` bullet. The cost is bounded — `check-orchestration.sh` is step 2 of `gate.sh`, before any cargo step, so the trap costs about a minute and the message names the field and points at `templates/landing.md` — but it is still a trap, and an agent file is a rule change that wants its own `herd-reviewer` pass rather than a hunk of a gate branch. The replacement text is drafted verbatim in the handoff | ORCH-335's own out-of-scope finding, 2026-10-31 | herd-step | done | Nothing. **Done on `orch-337-merger-field`, 2026-10-31**, from ORCH-335's drafted text and no further design: step 5 now names the field, its shape, the wrap-and-sub-bullet rule and what ends it, and says in its own sentence that **the field is the claim and a fence is only evidence** — a `gh run view` paste in a fenced block, a paragraph of prose and a table row all fail it, which is the verdict batch 33's fenced record would now get, deliberately. It also carries the floor: required of landings dated **2026-11-01 or later**, batch 34's record predates it and is not rewritten, so the first landing on or after that date is the first that must write the field. Step 6 points its *predecessor's figure* sentence at step 5's field, and step 9's *"its own *CI run* paragraph"* is now the `- **Previous run:**` field of the next record's *CI run* section. The sentence about truth is strengthened rather than dropped: no network and no toolchain, shape and not truth, a mistyped digit passes, and *"that comparison is a human's"* is cited where `workflow.md` says it. `check-docs.sh` alone, `.claude/` and `docs/` being prose-exempt; the four Rust gates are the batch's own `gate.sh` at integration | ORCH-335 | `.claude/agents/herd-merger.md`, steps 5, 6 and 9 (the *CI run* paragraph of step 5, *"a different figure, carried by step 5's field"* in step 6, and step 9's *"the next landing is what records it"*); `docs/orchestration/templates/landing.md` *CI run*; `docs/orchestration/workflow.md` *The next landing writes that run into its own record* and *No docs gate can check the record itself*; `history/2026-10-31-orch-335-labelled-previous-run.md`, *Not done* and *Drafted for `herd-merger`*; `history/2026-10-31-orch-337-merger-field.md`; **D289** |
```

**ORCH-338 is unchanged and stays open**, and no row was taken from
ORCH-339.

## Decisions needed

None. The decision was taken by ORCH-335 and the review behind it; this
task carries it into the one document that had not got it.

## Resume point

This branch joins the batch that `orch-335-labelled-previous-run` is
in, and nothing orders them except that both must land together: the
gate's new oracle and the merger's instructions for it are one change
in two files. **The merger of that batch is the first that must write
`- **Previous run:** batch <n-1> · ci.yml <id> · <verdict>`** in its
own record, since its landing is dated on or after 2026-11-01 — and
step 5 of its own agent file now tells it how, which is the whole point
of this task.

Then: size ORCH-338, the per-file half in `check-docs.sh`.

## Processes

PIDs started and ended: none. No background command, no `pkill`, no
`killall`, no Herd job submitted, no service touched.

No `target/` of my own: this task touches no `.rs` file and compiled
nothing, so no `CARGO_TARGET_DIR` was created and there is none to
delete (`workflow.md`, *Concurrency and host rules*). The worktree is
`scratchpad/wt-orch-337` and it is the only path written outside the
branch.
