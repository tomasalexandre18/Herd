# orch-335-labelled-previous-run — handoff 2026-10-31

- **Item:** ORCH-335, ORCH-336 (ORCH-337 and ORCH-338 opened, not done)
- **Agent:** herd-step
- **Branch / commit:** `orch-335-labelled-previous-run` / `cae7175`,
  based on `origin/batch-34-integration` `6565e8d` — **not `main`**: the
  guard this repairs exists only on that integration branch
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Two defects in `scripts/check-orchestration.sh`, in the order their
dependency requires: the load defect first, because without it a loaded
host cannot produce a green run of this script and the evidence for the
second defect needs one.

**1. The host's own load no longer decides a case (ORCH-336).**
`gate_run` built every child with `env -i` and a fixed assignment list
that omitted `HERD_GATE_LOADAVG`, so the 25 of 40 call sites that supply
no value of their own had `gate.sh` read the real `/proc/loadavg` —
and `gate.sh:279` refuses outright above `load_max`, default 24
(`gate.sh:118`). The default is now a quiet loadavg file, rewritten on
every call because the stub suite writes to `$HERD_GATE_LOADAVG` when a
case asks it to. The 15 sites that drive the load cases still override
it: `env` applies assignments left to right and `"$@"` comes last.
`HERD_GATE_LOAD_MAX` is deliberately untouched — raising a gate's own
ceiling to get past it is what `workflow.md`'s *Failure* forbids.

**2. The predecessor-run oracle is one labelled field (ORCH-335).** The
paragraph oracle could go green on the exact defect it was built for.
It now asks for one bullet:

```text
- **Previous run:** batch 33 · `ci.yml` 41228366473 · success
```

anchored at the start of a line, placeholder-refused, carrying the
predecessor's batch, a run id of `run_digits` digits and one GitHub
conclusion word — or the literal `no ci.yml run` as the explicit hatch.
Exactly one such field per record: two are refused rather than resolved.
The field may wrap and may carry a nested sub-bullet; a blank line or an
unindented line ends it.

Floored at `lr_from=2026-11-01`, the precedent being `check-docs.sh`'s
`landing_from`, which has been moved once for the same reason: batch
34's record was written under the paragraph rule and is not rewritable
once landed. Below the floor, and when the previous landing is not named
`-merge-batch-<n>.md` (23 of 58 records are not), the case prints a
**skip with its reason** (D27's shape) and never a failure.

**3. The known-bad fixture is pinned and frozen.** It used to copy every
`*-merge-*.md` and delete batch 33's, assuming the newest remaining was
batch 32's — false the moment a batch-34 record existed, which is why
the case read `nothing at all`. It is now pinned to the batch-31 and
batch-32 records, with an assertion that the fixture holds **exactly
two** files (this script is `set -uo pipefail` without `-e`, so a stale
glob would silently give a one-file fixture and the same uninformative
message), plus the offending sentence frozen verbatim in the script and
one case asserting it is still what the real record says. That
faithfulness case **fired on its first run**, on a paraphrase of mine:
the real sentence wraps mid-phrase, so the comparison now normalises
whitespace the way the detector itself joins a wrapped field.

**4. One width for the run id.** `check-docs.sh` now declares
`run_digits=8` and builds its own regex from it; the guard reads that
line out of that file rather than keeping a second copy, and a case
asserts the read worked. The two gates asserted 6 digits and 8 for the
same figure. Measured before raising it: all 34 landings on or after
`landing_from` satisfy the `**CI run:**` line through `none` and none
through a number, so 6 → 8 refuses nothing written down today.

**5. The rule's own list.** `workflow.md` step 6 required a
`herd-reviewer` pass for `CLAUDE.md`, agents, permissions, hooks,
`check-docs.sh` or an invariant — and not `check-orchestration.sh`, a
gate that decides whether a landing may happen. It now names **every**
gate script, `scripts/*.sh` and `scripts/githooks/*`. The review that
found the omission is the one that covers this line.

`templates/landing.md` and `workflow.md`'s *The next landing writes that
run into its own record* describe the field, its exact shape, the floor
and the two skips, in the words the code uses.

## Not done, and why

- **`.claude/agents/herd-merger.md` still describes the retired
  paragraph form** (steps 5, 6 and 9). Out of this task's scope, and an
  agent file is a rule change that wants its own `herd-reviewer` pass
  rather than a hunk of a gate branch. **This is the one finding that
  needs acting on before the next landing**: a merger following those
  steps writes a paragraph and reddens its own gate.
  `check-orchestration.sh` is step 2 of `gate.sh`, before any cargo
  step, so the trap costs about a minute and the message names the field
  and points at `templates/landing.md` — but it is still a trap.
  **ORCH-337**, with the replacement text under *Drafted* below.
- **Batch 34's merge record is not amended.** The brief forbids it and a
  merge record belongs to `herd-merger` at the re-gate. Its evidence
  block still shows `335 passed, 0 failed` with eleven `landing:` cases
  ok, which was true of a tree that was never pushed.
- **`docs/decisions.md` is untouched.** Append-only, `herd-merger`'s
  alone; the drafted entry is below, headed `D-NEXT`.
- **Only the newest landing record is ever checked.** Unchanged by this
  task and now written down as **ORCH-338**: a record that lands without
  the field is never asked again. The per-file half belongs in
  `check-docs.sh` beside the four fields it already checks.
- **The real-tree case is a printed SKIP until the next landing.** By
  construction of the floor. The first landing dated 2026-11-01 or later
  turns it live, and that landing is the one that must write the field.

## Gates

`scripts/` is not prose-exempt, so all four Rust gates plus
`check-docs.sh` are owed, and the whole `./scripts/gate.sh` is the run
that carries them.

Runs made **before** this file was committed, on `cae7175`'s tree:

```text
$ ./scripts/check-docs.sh
docs ok                                            (exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 350 passed, 0 failed, 1 skipped
a skipped case is NOT a passed one (D27): see the SKIP lines above
                                                   (exit 0, load1 2.49)
SKIP  landing: the real tree's newest landing record carries it:
  2026-10-31-merge-batch-34.md predates the **Previous run:** field,
  required of landings dated 2026-11-01 or later
```

The same script, same totals, at **load1 54.61** on the loaded host
earlier the same hour — which is the measurement that closes ORCH-336:
`350 passed, 0 failed, 1 skipped` at a load 2.3× the ceiling that
produced 45 reds before the fix.

**The last run, named and not quoted** — it runs after this file is
committed, on the tree that is pushed, so its output cannot be in this
file (`templates/handoff.md`): `./scripts/gate.sh` submitted whole
through the queue on this branch's tip; its log and the receipt under
`target/herd-gate/<sha>.receipt` are named in the agent's report, with
the verbatim verdict line.

**Skips:** one, named above — `landing: the real tree's newest landing
record carries it`, skipped because the newest landing record predates
the field. It is not a pass (D27). The suite's own skip list is
unchanged by this branch: no `#[ignore]` and no `HERD-SKIPPED` was
added, moved or reworded, and no `.rs` file is touched.

## Risks and findings

- **ORCH-337 is the risk, and it is a trap with a bounded cost.** See
  *Not done*.
- **Three of the review's eleven existing shapes changed verdict, by
  design, and the change is a strict strengthening.** Measured old
  oracle vs new on the nine synthetic shapes: `01-good` QUIET → FIRES,
  `06-wrapped` QUIET → FIRES, `08-noran` QUIET → FIRES, and
  `02-silent`, `03-claimed`, `04-noverdict`, `05-ownrun`,
  `07-otherbullet` FIRES → FIRES, `09-first` QUIET → QUIET. **No shape
  the paragraph rule refused is accepted now.** The three that moved all
  moved for one reason: the claim may no longer live in prose or in the
  `**CI run:**` bullet. Each survives as a case in its field form, and
  `01-good`'s old content is kept as its own case — *the run in a
  paragraph, with no field, is no longer enough* — so the loosening that
  did not happen is itself asserted.
- **One verbatim string in the known-bad message is retired, and this is
  the sentence owed for it.** The review's anti-weakening criterion was
  that the pinned fixture still emit `names batch 31` **and**
  `no paragraph`. It still emits `names batch 31`, verbatim. `no
  paragraph` is gone because the oracle no longer has paragraphs: the
  message is now `no "- **Previous run:**" field names batch 31, whose
  post-push ci.yml run this is the only record that can carry
  (templates/landing.md, *CI run*)`. The criterion the new message meets
  instead is the stronger one: the fires-set is a strict superset,
  case by case, measured above.
- **`adv/f3` was decided deliberately, not by accident.** A record whose
  only figure is inside a ```text fence now FIRES. Batch 33's real
  record wrote both owed runs that way, and the review was right that
  under a fence-skipping variant it would have gone red. Under a
  labelled field the fenced form is simply not the claim, and
  `templates/landing.md` now says so in writing: the fence is evidence
  for the handoff, the bullet is the record's claim.
- **Three of the brief's figures are off, and none of them changes a
  decision.** (a) `gate_run` sites supplying their own
  `HERD_GATE_LOADAVG`: **15 of 40**, not five of twenty. (b) Landing
  records not named `-merge-batch-<n>.md`: **23 of 58**, not 22
  (`2026-10-29-merge-landing-2.md` and
  `2026-10-30-merge-batch-26-regate.md` are both in the list). (c) The
  pushed-tree figure for batch 34 is **334 passed / 1 failed**, not
  "335 passed, 1 failed": 334 + 1 and 335 + 0 are the same population of
  335, and the one red was the fixture defect this task fixes.
- **`workflow.md` was edited beyond the brief's "step 6's list only".**
  The file is in scope; the sub-scope was not met, deliberately. *The
  next landing writes that run into its own record* and *What that case
  does not do* described the paragraph form in detail and would have
  been left stating a rule the code refutes, which is the class of
  defect `fix-four-false-sentences` exists for. Reported rather than
  done quietly.
- **A mishap, reported because the evidence depends on it.** Mid-task I
  ran `git checkout -- .` and discarded four files' uncommitted edits.
  All of it was recovered — the two spliced files from their scratchpad
  copies, byte-identical (`cmp`), the other three re-applied from their
  exact texts — and the restored tree's diffstat matched the measured
  one line for line (4 / 73 / 54 / 18 / 437, 459 insertions, 127
  deletions) before anything was committed. The gates quoted above were
  then re-run on the committed tree, not reused from before the mishap.

## Drafted for `herd-merger`

**The `docs/decisions.md` entry, in full:**

```markdown
## 2026-10-31 — D-NEXT A claim a machine checks is a labelled field,
never prose

**Context.** ORCH-333 gave the landing record a machine-checked claim:
the newest record must carry its predecessor's `ci.yml` run, because a
landing cannot record its own and the chain had already broken once at
batch 32. The check asked whether some **paragraph** of the record
mentioned the predecessor's batch near eight digits and a verdict word.
`herd-reviewer` extracted that `awk` and measured three ways to pass it
with nothing recorded (2026-10-31): a record laid out as a **table**
whose predecessor row said *not read yet* — a table has no blank line
and no bullet, so the whole table is one paragraph; the word
*successful* beside a byte count, `success` having no word boundary and
"any eight digits" matching `22649270272 bytes`; and a record merely
**quoting one of the check's own case names**, which is the likely next
record and not a contrived one. The document class is the problem: a
landing record's whole job is to discuss runs and verdicts, so
co-occurrence in prose is ordinary there.

**Choice.** A claim a machine checks is a **labelled field, never
prose** — one line, anchored, with a label of its own, refused while it
holds a placeholder, and shape-checked. It is the construction
`check-docs.sh` already used for **Batch:**, **Gate:**, **Skips:** and
**CI run:**, and the predecessor's run now joins them as
`- **Previous run:** batch <n-1> · ci.yml <id> · <verdict>`. A rule
whose check is a search of the prose is not held; it is narrated. The
generalisation is the half worth keeping: **before adding a check that
reads a document, name the field it reads.** A new field needs a floor
date, so that records written before it existed are not refused for not
using it — `check-docs.sh`'s `landing_from` is the precedent and has
been moved once already.

**Why not retire the section instead.** The rule is sound: batch 32
really did break the chain. Retiring the check would leave it in prose
with nothing holding it, which is the state that produced batch 32. And
reverting is not cheap — `templates/landing.md`, `workflow.md`,
`.claude/agents/herd-merger.md` and the board all document the check,
and D288 cites the predecessor-run case, so under D148 a correction
there would be its own entry. Fix forward, once.

**What this does not claim.** Nothing about truth. The gate has no
network: it cannot ask GitHub whether a run exists or whether the
verdict is honest, and a mistyped digit still passes. What the field
buys over the paragraph is that the claim cannot be assembled out of
two unrelated sentences and cannot be made by accident. The verdict's
truth stays a human's (`workflow.md`, *What a landing is*).

**Measured.** Old oracle vs new, on the reviewer's own fixtures and the
nine synthetic shapes: every shape the paragraph rule refused is still
refused; three it accepted now fire; the false RED on a nested
sub-bullet is gone; the pinned batch-31/32 fixture still fires naming
batch 31. `check-orchestration.sh`: 350 passed, 0 failed, 1 skipped —
the skip being the real-tree case, which the floor defers to the first
landing dated 2026-11-01 or later.
```

**The `board.md` rows** are already written into
`docs/orchestration/board.md` on this branch, in *Open*, as
**ORCH-335**, **ORCH-336**, **ORCH-337** and **ORCH-338** — four rows
added after ORCH-334, nothing else in that file touched. ORCH-337 and
ORCH-338 are the two that stay open.

**The text ORCH-337 needs**, so that it does not have to be re-derived:
in `.claude/agents/herd-merger.md`, every sentence that says the
predecessor's run goes in the record *"inside one paragraph"*, and the
sentences explaining that a paragraph ends at a blank line, at the next
bullet or at the next heading, become: *"in the one labelled field that
may carry it, `- **Previous run:** batch <n-1> · ci.yml <id> ·
<verdict>` — one bullet, that label, no placeholder left in it; the
field may wrap and may carry a nested sub-bullet, and a blank line or an
unindented line ends it. `templates/landing.md`, *CI run*, has the exact
shape. It runs at step 2 of the `gate.sh` you run in step 7, before any
cargo step, so a missing field turns your own landing red for about a
minute."* And: the field is required of landings dated 2026-11-01 or
later.

## Decisions needed

**One, and it is a sequencing question, not a technical one.** ORCH-337
(the merger's own instructions still describe the retired paragraph
form) must be closed before the next landing, or that landing's merger
must be told the field in its brief. — **Recommendation:** brief
ORCH-337 as its own small task now; it is one agent file and the
replacement text is drafted above. — **Fallback:** put the field's one
line in the next merger's brief and let ORCH-337 land later; the cost of
getting it wrong is one red gate step at about a minute, not a compile.

## Resume point

`herd-merger` takes this branch into batch 34's integration and
re-gates. The landing it then writes is the **first** that must carry
`- **Previous run:**` — batch 33's run id and verdict, in that bullet —
because this branch's floor is 2026-11-01 and that landing will be dated
on or after it. Until then the real-tree case prints its skip.

Then: brief ORCH-337 (`.claude/agents/herd-merger.md`), and size
ORCH-338 (the per-file half, in `check-docs.sh`).

## Processes

PIDs started and ended: one, `2904845` — a `bash` of my own left waiting
on stdin by a stray `cat` in a command of mine; found by its full `args`
and killed by PID, never by name. No `pkill`, no `killall`. No Herd job
submitted yet at the time this file was committed; the gate submission is
in the agent's report.

No `target/` of my own: this task touches no `.rs` file and compiled
nothing, so no `CARGO_TARGET_DIR` was created and there is none to
delete. The queued `gate.sh` run uses its own build tree under
`/mnt/ssd_980/herd/build/`, named in the agent's report and deleted
there.
