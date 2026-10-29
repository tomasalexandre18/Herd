# claude-md-direction — handoff 2026-10-29

- **Item:** ORCH-nnn (rule change; no board row was allocated to this task)
- **Agent:** herd-step
- **Branch / commits:** `claude-md-direction` — `2bb47e4` the rule,
  `c273dee` first handoff, `7d70e85` the review's named changes,
  `f0d147c` written against batch 7's tag answer, this handoff last.
  Based on origin/main `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`, and
  **written against `batch-7-integration`'s `CLAUDE.md` as it will stand
  after that landing** (see *The collision*).
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`
- **Review:** `herd-reviewer` returned "land with named changes"; all are
  applied, and the tag finding was then overtaken by Alexa's answer.

## Done

`CLAUDE.md` carries Alexa's direction of 2026-10-29 instead of depending
on a conversation. **One file changed** against `origin/main`; 150 lines
before and after. `docs/orchestration/workflow.md` is byte-identical to
`origin/main`.

**The new section, replacing *When to ask Alexa*: *Direction, and what
reaches Alexa*** (`CLAUDE.md:117-143`, 27 lines with its trailing blank):

1. **First duty: keep bounded work in flight so the project advances**,
   above any single event — a red `main`, a release or a live defect must
   not drain the queue. **Answering Alexa is the second duty.** The order
   is written down because the order is the point.
2. **The triage has three outcomes, in their order:**
   - **Ask him** when the answer changes *what* gets done, never *how*:
     **anything structural** — the method, the shape of what the product
     *is* or *promises* — scope, step order, **a release and its notes
     (not its tag)**, an irreversible act, **anything on the consumer's
     side**. One question with a recommendation in the board's *Waiting*;
     work goes on. **A permission the system refuses is escalated, never
     worked around**: a peer refused it cannot delegate it.
   - **Decide alone** what is inside the mandate and **reversible**: not
     submitted beforehand, **told afterwards in the session-end report**.
   - **Report, with no question attached,** what could cost him later: a
     trap that would destroy something, a threshold being approached, a
     structural decision arriving from elsewhere. Early enough to matter.
3. **How** is never his: not a technical choice, an implementation, an
   API, a defect, the tooling.
4. **A report is aggregated and quantified**, columns named: date ·
   milestone · musts done/total · open rows · red gates.
5. **A guidance he has given is standing**: followed, never re-raised —
   no compatibility to preserve, and no compatibility question gates a
   tag (**D212**); GitHub is not the centre, local git is, and a run id
   is not a report while a green run is not a decision.

**Who decides what** moved into *Roles*: Alexa is the project's
**director**; the orchestrator **manages**; the agents **build**.

## The collision with `batch-7-integration`, and its resolution

Alexa answered the actor question: **the integrator pushes the tag**, on
the landing's own two conditions, **D212**, and `batch-7-integration` is
landing that with five contract files and the board's ORCH-001 row. Two
of its hunks are in `CLAUDE.md`, in the exact region this branch
rewrites. I read its diff and wrote this branch against the **post-
landing** text, not against `cd0f1b7`:

- ***Git and main*** carries batch 7's tag bullet **byte-identical**
  (verified by `diff`): *"**Pushing a release's tag is `herd-merger`'s
  too** (Alexa, 2026-10-29, **D212**), on the landing's own two
  conditions, as a separate later act."*
- Its **push-to-`main` bullet is restored verbatim from `origin/main`**.
  My compressed five-line version sat immediately above batch 7's
  insertion and made that region conflict for no reason;
  `git merge-tree` confirms the region now auto-merges. The line it cost
  was paid by the GitHub bullet ("of this project" dropped).
- **The half of batch 7's edit that lived in the deleted section is
  absorbed**, not merged: *a release and its own notes (not its tag)* is
  a trigger in outcome 1.
- **The tag's actor is stated once**, in *Git and main*. The standing
  guidance keeps only the compatibility claim and cites **D212** — which
  is where the reviewer found the quote belongs anyway, inside the
  compatibility paragraph.

**What remains, measured, read-only** (`git merge-tree --write-tree
HEAD batch-7-integration`, merged tree `a5c0ea6`):

```
CLAUDE.md

Auto-merging CLAUDE.md
CONFLICT (content): Merge conflict in CLAUDE.md
```

**`CLAUDE.md` is the only file in conflict, with exactly one hunk** (at
lines 120/145/152 of the merged file): they edit the body of the section
this branch deletes and renames. **Resolution: take this branch's body
(`HEAD`), whole.** Nothing of theirs is lost by it — both of their
additions, *a release and its own notes (not its tag)* and *anything on
the consumer's side*, are already in outcome 1, and their tag bullet is
already in *Git and main* byte-identical.

**One thing that side carries and mine does not: "issues".** See finding
3. Do not resolve that by guessing.

## The 150-line cap: not moved, and why it cannot be

`CLAUDE.md` is **150 lines against `scripts/check-docs.sh`'s limit of
150**, as is `origin/main`'s and as is batch 7's. No line exceeds 79
columns.

Moving the cap is not available to this task: `limit=150` lives in
`scripts/check-docs.sh`, and `scripts/` is **outside** the prose paths of
*Commands*, so editing it would make this a code change owing all four
Rust gates, which this task may not run. Every line was paid for. Per
section, including each section's trailing blank:

| Section | main | here | What left |
|---|---|---|---|
| Roles | 17 | 14 | "decides *how* … does not ask" (now the new section); the enumeration behind "English in everything written" |
| Vocabulary | 14 | 9 | "WebSocket", "in docker compose", "Dials out to the hub" (invariant 3), "(AI agent, merger, nightly task)"; the closing note at one line, mapping and "translate" kept |
| Invariants | 34 | 34 | untouched, as the brief requires |
| Layout | 11 | 5 | the fence, the alignment, the `(library)`/`(binary)` markers, the per-crate descriptions |
| Commands | 19 | 18 | the "Toolchain comes from the Nix flake" sentence, folded into "`cargo` is not on PATH". The did-not-run sentence is **back**, in full |
| Git and main | 17 | 16 | the *One landing per batch* bullet at three lines instead of four ("on the orchestrator's instruction", "with the record"). The push bullet and the tag bullet are **verbatim** |
| Shared host rules | 15 | 14 | "build or test", "free" memory/disk, "too, at", "(workflow.md)" on the `target/` sentence |
| *When to ask Alexa* → *Direction…* | 9 | 27 | the section, replaced |
| Session start, session end | 7 | 6 | "and reports in three or four lines" — the new section says what a report is; the board is "the board" here, its path still in invariant 8 |

**Two rules stay dropped**, as the review accepted after trying to break
the case: Alexa's *"j'ai rien eu et j'ai pas besoin"* example, and "do
not ask about file layout, naming, crate internals, test style or
libraries". **The compressions stand**, likewise reviewed.

## Not done, and why

- **No entry in `docs/decisions.md`**: the brief forbids it, and a
  `D-NEXT` header fails `check-docs.sh`. Text proposed below;
  `herd-merger` allocates the number. **D212 is batch 7's and is not
  touched here.**
- **`docs/status.md` and `docs/orchestration/board.md` untouched**: the
  integrator owns both. Rows proposed, not written.
- **`docs/orchestration/workflow.md` untouched** (reverted to
  `origin/main`): batch 7 corrects its six stale places; two edits of
  mine would have collided with that for no gain.
- **The Rust gates were not run** — see *Gates*.

## Gates

Prose only. **The four Rust gates were NOT RUN**, and nothing here
claims they passed; what covers them is the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s run.

`./scripts/check-docs.sh`, run on the tree of commit `f0d147c`, verbatim:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Exit code 0. The `D170` line is a pre-existing `note:`.

Supporting measurements, verbatim, on the same tree:

```
$ wc -l CLAUDE.md
150 CLAUDE.md
$ LC_ALL=C awk '{l=$0;c=length(l)-gsub(/[\200-\277]/,"",l); \
    if (c>79) printf "%d: %d cols\n", FNR, c}' CLAUDE.md
(no output)
$ git diff --stat cd0f1b7 -- docs/orchestration/workflow.md
(no output — identical to origin/main)
$ diff <(git show batch-7-integration:CLAUDE.md | sed -n "/Pushing a release's tag/,+1p") \
       <(sed -n '101,102p' CLAUDE.md) && echo IDENTICAL
IDENTICAL
```

Both byte checks were proved on a known-bad **and** a known-good fixture
first, because `grep -P` on this host (ugrep) matches no byte pattern:

```
$ perl -ne 'print "$ARGV:$.: $_" if /\xc3\x83|\xc3\xa2\xc2\x80|\\x\{/' bad.md good.md
bad.md:1: bad double â here
bad.md:2: bad escape \x{2014} here
$ awk '...c>79...' long.md ok79.md      # 80-column and 79-column fixtures
long.md:1: 80 cols
```

The scan over `CLAUDE.md` is clean: every edit used `perl -0777` on raw
bytes, never `\x{...}` escapes.

The **last run**, on the committed tree that includes this handoff, is
`./scripts/check-docs.sh` from the worktree root; its output goes to the
agent's report, not into the tree it gates.

**Skips:** no skip. No `HERD-SKIPPED` line was produced or copied.

## Risks and findings

1. **The conflict must be resolved to `HEAD`'s body, not merged
   line-by-line.** A hand merge that kept any of batch 7's section body
   would reintroduce the two-way "ask / do not ask" wording this change
   replaces, and would leave two rules about tags in one file.
2. **This branch and batch 7 both claim the same 150 lines.** Each is at
   the cap on its own; the merged file is also 150, because the
   resolution takes one body and not both. Any later hand-edit to the
   merged section has no room.
3. **"Issues" is in batch 7's side of the conflict and not in mine.**
   Their text keeps "issues" among what reaches Alexa; my outcome 1 does
   not name it, and no line is free to add it. It is **not lost from the
   project**: `workflow.md` still says "No automatic GitHub issue: issues
   are Alexa's" and lists "GitHub issues" under *Permissions and
   escalation*. Either the integrator accepts `workflow.md` as its home,
   or a later task spends a line to name it in outcome 1.
   Recommendation: accept `workflow.md`, and note that an issue that
   changes scope is already caught by "anything structural".
4. **D210 stands** on its two measured permission walls, unchanged by
   Alexa's answer, which moved the tag and nothing else. What the new
   rule withdraws is only D210's **class** premise: the *When to ask
   Alexa* section it pointed at no longer exists under that name.
5. **The first duty has no threshold.** "Keep bounded work in flight"
   names no number, so nothing says when the queue is too thin — the
   failure being recorded, still unfalsifiable.
6. **The quantified roadmap does not exist yet.** The section fixes the
   columns; nobody has produced the line.
7. **A brief of mine described this change smaller than it is.** The
   first mid-task brief called it three sections; the diff rewrites
   **eight**, two of them changing rules rather than wording. The table
   above was in the first handoff and was not carried into that brief.
   Worth a habit: when a brief describes an agent's own diff back to it
   and gets it smaller than it is, the agent says so.

## Decisions needed

**The entry this change deserves, for `herd-merger` to number** (no
placeholder header: a `D-NEXT` header fails `check-docs.sh`):

> **2026-10-29 — D&lt;n&gt; Alexa directs; the orchestrator keeps work in
> flight; the triage has three outcomes**
>
> Alexa, verbatim: *"tu est orchestrateur ton but est de filé du bouleau
> pour que le projet avance, tu est plus haut que un évènement alors il
> y a que ca qui compte"*, *"ton deuxiemme role et de me repondre"*,
> *"mon but est d'etre directeur du project, tu est manageur, je donne
> la direction … tout les choix tenique ne passe pas par moi"*, *"tout
> ce qui est vraiment structurant pour le project dois quand meme me
> remonter, mais pas tout"* — of the consumer's project he chose the
> method and of the rest *"j'ai rien eu et j'ai pas besoin"* — *"une
> guidance dois etre suivie"*, *"quand je dis pas de preservation de
> compatibilité et c'est la seul chose que tu me parle c'est chiant,
> Github c'est la meme histoire"*, *"je veux des feuille de route
> chiffré et pouvoir comparer les avancement"*.
>
> The orchestrator's first duty is to keep bounded work in flight so the
> project advances; it stands above any single event, and a red `main`,
> a release or a live defect must not drain the queue. Answering Alexa
> is the second duty. The triage has three outcomes, in order: **ask
> him** when the answer changes what gets done — anything structural,
> the method or the shape of what the product is or promises, scope,
> step order, a release and its own notes, an irreversible act, anything
> on the consumer's side — and never how, one question with a
> recommendation, and a permission the system refuses is escalated and
> never worked around, by a peer too; **decide alone** what is inside
> the mandate and reversible, not submitted beforehand and told
> afterwards in the session-end report; **report with no question
> attached** what could cost him later, early enough to matter. A
> guidance already given is standing and is not re-raised: no
> compatibility to preserve (the consumer resets at v1.0.0), and no
> compatibility question gates a tag; GitHub is tooling and not the
> centre of this project — a run id is not a report, a green run is not
> a decision.  Reports to him are aggregated and quantified: one dated
> line, columns date · milestone · musts done/total · open rows · red
> gates.
>
> **Scope.** This supersedes the *When to ask Alexa* section of
> `CLAUDE.md`, which it replaces, and nothing else. **The tag's actor is
> D212's**, not this entry's: the integrator pushes it on the landing's
> own two conditions. **D210 stands**: a published release's notes are
> Alexa's own act, not because a release waits on his word but because
> two permission walls refuse the write to every agent and to the
> orchestrator, and neither is to be weakened (D210). It retires when
> ORCH-162 ships the notes in the tree. No brief re-raises it; a release
> brief carries the paste in the same order as the tag.
> Recorded in `CLAUDE.md`, *Direction, and what reaches Alexa*.

## Board rows proposed (not written: the integrator owns the board)

The two rows the previous version of this handoff proposed — the actor of
a tag, and the release residue — are **answered and done by batch 7**
(D212, five contract files, ORCH-176). What is left:

| Item | Origin | Owner | State | Next action |
|---|---|---|---|---|
| **The quantified roadmap**: produce the advancement line whose columns the new rule fixes (date · milestone · musts done/total · open rows · red gates) — the half of *"feuille de route chiffré"* nobody has delivered | Alexa, 2026-10-29 | orchestrator | open | emit it at the next session end and keep the columns |
| **The first duty has no threshold**: "keep bounded work in flight" names no number, so a thin queue cannot be called thin | this review, 2026-10-29 | herd-planner | open | propose a floor (tasks in flight, or work-days queued) and one line for `CLAUDE.md` |
| **"Issues" after the merge** (finding 3): `CLAUDE.md` read alone no longer names them; `workflow.md` does | this merge, 2026-10-29 | herd-step | open | accept `workflow.md` as the home, or spend a line in outcome 1 |

## Resume point

The branch joins the batch for `herd-merger`, which (1) resolves the one
`CLAUDE.md` hunk to this branch's body, whole, as *The collision* sets
out, (2) numbers the decision above, whose scope paragraph is already
narrowed to the triage and the duties, and (3) opens the three rows. If
batch 7 lands first, nothing here changes: this text was written against
its result.

## Processes

No process started, no PID to report: no `cargo`, no build, no test, no
background job. This task ran `git` (including the read-only
`git merge-tree`), `sed`, `awk`, `fmt`, `perl`, `wc`, `diff` and
`./scripts/check-docs.sh`. No file outside this worktree and my
scratchpad was written; `batch-7-integration` was **read** through
`git show` and `git diff` only, never checked out.

No `target/` was created, so there is none to delete: `CARGO_TARGET_DIR`
was never set and nothing compiled. Prose only, so there was no heavy
phase and `./scripts/bench-preflight.sh` did not apply.
