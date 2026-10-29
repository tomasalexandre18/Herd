# claude-md-direction — handoff 2026-10-29

- **Item:** ORCH-nnn (rule change; no board row was allocated to this task)
- **Agent:** herd-step
- **Branch / commit:** claude-md-direction / 2bb47e4, based on origin/main
  cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`
- **Review:** this is a rule change, so `herd-reviewer` reviews commit
  2bb47e4 before it lands.

## Done

`CLAUDE.md` now carries Alexa's direction of 2026-10-29 instead of
depending on a conversation. Two files changed, 63 insertions, 59
deletions.

**The new section, replacing *When to ask Alexa*: *Direction, and what
reaches Alexa*** (`CLAUDE.md:117-143`, 27 lines plus its blank). It says,
in this order:

1. **First duty: keep bounded work in flight so the project advances**,
   above any single event — a red `main`, a release or a live defect must
   not drain the queue. **Answering Alexa is the second duty.** The order
   is written because the order is the point.
2. **The triage has three outcomes, in their order**, which is the part
   the mid-task correction added and the part that was missing:
   - **Ask him** when the answer changes *what* gets done — what the
     product *is* or *promises* — and never *how*: scope, step order,
     money or hardware, an irreversible act. One question with a
     recommendation in the board's *Waiting*; other work goes on. **A
     permission the system refuses is escalated, never worked around**,
     and a peer refused it cannot delegate it either.
   - **Decide alone** what is inside the mandate and reversible or
     riskless: not submitted beforehand, **told afterwards** — stated as
     not optional, since that is what keeps him directing without being
     consulted.
   - **Report, with no question attached,** what could cost him later: a
     trap that would destroy something, a threshold being approached, a
     decision arriving from elsewhere. Early enough to matter.
3. **How** is never his: not a technical choice, an implementation, an
   API, a defect, the tooling.
4. **A report to him is aggregated and quantified** — a dated advancement
   line with **stable columns**, comparable with the next.
5. **A guidance he has given is standing**: followed, never re-raised,
   with the three live ones as bullets so nobody re-derives them — no
   compatibility to preserve (the consumer resets entirely at v1.0.0),
   GitHub is not the centre of this project and local git is, and a tag
   and a release no longer wait on his word.

**Who decides what** moved into *Roles*: Alexa is the project's
**director** and gives the direction; the orchestrator **manages**; the
execution agents **build**. The clause that made Alexa "the word for
anything outward-facing" is gone, and so is "escalates decisions" as a
duty of the orchestrator's own.

**The supersession is stated in both files**, so a reader of either gets
the new rule. `CLAUDE.md`'s *Git and main* lost the bullet reserving a tag
and a release, and the new section says it supersedes that section and
`workflow.md`'s copy. `docs/orchestration/workflow.md` says the same at
its two rule statements — the *Unchanged.* paragraph (line 366) and
*Permissions and escalation* (line 582) — each naming the new section.

## The 150-line cap: not moved, and why it could not be

`CLAUDE.md` is **150 lines against `scripts/check-docs.sh`'s limit of
150**, the same count as before, and no line exceeds 79 columns.

Moving the cap was ruled out on a fact, not a preference: the limit is
`limit=150` in `scripts/check-docs.sh`, and `scripts/` is **outside** the
prose paths of *Commands* (`CLAUDE.md`, `README.md`, `docs/`, `.claude/`,
`.github/*.md`). Editing it would have made this a code change owing all
four Rust gates, which this task is forbidden to run (no `cargo`, eight
sibling builds on the host). So the 19 net lines were paid for. Per
section, including each section's trailing blank:

| Section | Before | After | What left |
|---|---|---|---|
| Roles | 17 | 14 | "decides *how* … does not ask" (now the new section); the enumeration behind "English in everything written" |
| Vocabulary | 14 | 10 | "WebSocket", "in docker compose", "Dials out to the hub" (invariant 3), "(AI agent, merger, nightly task)" |
| Invariants | 34 | 34 | untouched, as the brief requires |
| Layout | 11 | 5 | the fence, the column alignment, the `(library)`/`(binary)` markers and the per-crate descriptions |
| Commands | 19 | 18 | "A gate that did not run is reported as not run" shortened to "never as passed" |
| Git and main | 17 | 13 | the tag/release bullet (superseded), "on the orchestrator's instruction", "nothing waits for Alexa" (now the new section) |
| Shared host rules | 15 | 15 | nothing |
| *When to ask Alexa* → *Direction…* | 9 | 28 | the section, replaced |
| Session start, session end | 7 | 6 | "and reports in three or four lines" — the new section says what a report is; the board is named "the board" here, its path still in invariant 8 |

Two rules were **dropped** to buy the three outcomes, and both are the
reviewer's to restore (each costs lines that only exist if something else
goes, or the cap moves):

- Alexa's example of the consumer's project — *"j'ai rien eu et j'ai pas
  besoin"* — which the brief asked for verbatim. The *what/how* test in
  outcome 1 now carries the rule it illustrated, and the quote is in the
  proposed decision text below, so it is recorded, not lost.
- "Do not ask about file layout, naming, crate internals, test style or
  libraries" (the old section's last line). Outcome 1's *never how* and
  outcome 2 cover it, less concretely.

## Not done, and why

- **No entry in `docs/decisions.md`.** The brief forbids it and a
  `D-NEXT` header is a `check-docs.sh` failure (`scripts/check-docs.sh`,
  the D-NEXT guard). The proposed text is under *Decisions needed*;
  `herd-merger` allocates the number.
- **`docs/status.md` and `docs/orchestration/board.md` untouched**: the
  integrator owns both.
- **`workflow.md` edited only where point 4 of the brief requires it.**
  Three further places in that file still read as if a release act were
  Alexa's; they are under *Risks and findings*, left alone.
- **The Rust gates were not run** — see *Gates*.

## Gates

Prose only. **The four Rust gates were NOT RUN**, and nothing here claims
they passed; what covers them is the batch's own `./scripts/gate.sh` at
integration, which is `herd-merger`'s run and not this agent's.

`./scripts/check-docs.sh`, run on the tree of commit 2bb47e4, verbatim:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Exit code 0. The `D170` line is a `note:`, not a finding: it is
pre-existing and about a decision number this task did not touch.

Supporting measurements, verbatim, on the same tree:

```
$ wc -l CLAUDE.md
150 CLAUDE.md
$ LC_ALL=C awk '{l=$0;c=length(l)-gsub(/[\200-\277]/,"",l); \
    if (c>79) printf "%d: %d cols\n", FNR, c}' CLAUDE.md
(no output)
```

Both byte checks were proved on a known-bad **and** a known-good fixture
before being trusted, because `grep -P` on this host (ugrep) matches no
byte pattern:

```
$ perl -ne 'print "$ARGV:$.: $_" if /\xc3\x83|\xc3\xa2\xc2\x80|\\x\{/' bad.md good.md
bad.md:1: bad double â here
bad.md:2: bad escape \x{2014} here
$ awk '...c>79...' long.md ok79.md      # 80-column and 79-column fixtures
long.md:1: 80 cols
```

The same scan over `CLAUDE.md` and `docs/orchestration/workflow.md` is
clean: the `workflow.md` edits were made with `perl -0777` on raw bytes,
never on `\x{...}` escapes.

The **last run**, on the committed tree that includes this handoff, is
`./scripts/check-docs.sh` from the worktree root; its output goes to the
agent's report, not into the tree it gates.

**Skips:** no skip. No `HERD-SKIPPED` line was produced or copied — no
test suite ran.

## Risks and findings

1. **`workflow.md` still reserves release acts in three places this task
   left alone** (point 4 of the brief bounds the edit to the rule
   statements). Each now contradicts `CLAUDE.md`:
   - line 14, the agent table: `herd-release` is "simulation only until
     Alexa orders a tag";
   - lines 75-85, step 8 *Release*: "On Alexa's order", "The tag itself is
     pushed by whoever Alexa names in that order", and **D210** of
     2026-10-29, "A release's own notes are Alexa's act too";
   - line 10, the orchestrator row, keeps "tag, release" among what it
     never does itself — that one stays **true** (it is an agent's task,
     not Alexa's word) and needs no change.
   **D210 is the sharp one**: a dated decision in `docs/decisions.md`, one
   day old, which the new rule supersedes in part. A decision is
   superseded by another decision, not by an edit to `workflow.md`, which
   is why this agent did not touch either.
2. **`.claude/agents/herd-release.md`** very probably carries the same
   "until Alexa orders a tag" wording in its own contract; not read, not
   in this task's allowed paths.
3. **Outcome 2's "told afterwards" has no channel.** The rule says a
   decision taken alone is told afterwards, and nothing in the file says
   where: the board, the session-end report, or the advancement line. The
   session-end report is the obvious home and `docs/status.md` is the
   integrator's file, so this needs one line somewhere — a separate task,
   because it is a second rule change.
4. **The report format is stated, not templated.** "A dated advancement
   line with stable columns" fixes the shape but names no columns, so the
   first such report chooses them and every later one is compared against
   that choice. A one-line template in `docs/orchestration/templates/`
   would remove the ambiguity.
5. **Compressions a reviewer may want back.** Every row of the table
   above is reversible on its own, but only by moving the cap — which
   costs the four Rust gates (see above) — or by cutting something else.
   The two most arguable: *Layout* lost its fence and its per-crate
   descriptions, and *Vocabulary*'s **hub** entry lost "in docker
   compose", an operational fact about the consumer rather than a word.
6. **Nothing was softened.** His words are quoted where they are the rule
   (*"pour le tag on sens fou tout vas etre reset"* in `workflow.md`, the
   rest in the proposed decision), and the file records the rule, not the
   apology.

## Decisions needed

**The entry this change deserves, for `herd-merger` to number** (the text
is proposed here; no placeholder header is written, since a `D-NEXT`
header fails `check-docs.sh`):

> **2026-10-29 — D&lt;n&gt; Alexa directs; the orchestrator keeps work in
> flight; the triage has three outcomes**
>
> Alexa, verbatim: *"tu est orchestrateur ton but est de filé du bouleau
> pour que le projet avance, tu est plus haut que un évènement alors il y
> a que ca qui compte"*, *"ton deuxiemme role et de me repondre"*, *"mon
> but est d'etre directeur du project, tu est manageur, je donne la
> direction … tout les choix tenique ne passe pas par moi"*, *"tout ce
> qui est vraiment structurant pour le project dois quand meme me
> remonter, mais pas tout"* — of the consumer's project he chose the
> method and of the rest *"j'ai rien eu et j'ai pas besoin"* — *"une
> guidance dois etre suivie"*, *"quand je dis pas de preservation de
> compatibilité et c'est la seul chose que tu me parle c'est chiant,
> Github c'est la meme histoire"*, *"pour le tag on sens fou tout vas
> etre reset"*, *"je veux des feuille de route chiffré et pouvoir
> comparer les avancement"*.
>
> The orchestrator's first duty is to keep bounded work in flight so the
> project advances; it stands above any single event, and a red `main`, a
> release or a live defect must not drain the queue. Answering Alexa is
> the second duty. The triage has three outcomes, in order: **ask him**
> when the answer changes what gets done — what the product is or
> promises — and never how, one question with a recommendation, and a
> permission the system refuses is escalated and never worked around;
> **decide alone** what is inside the mandate and reversible or riskless,
> not submitted beforehand and told afterwards; **report with no question
> attached** what could cost him later, early enough to matter. A
> guidance already given is standing and is not re-raised: no
> compatibility to preserve (the consumer resets entirely at v1.0.0),
> GitHub is tooling and not the centre of this project, and **a tag and a
> release no longer wait on his word**. Reports to him are aggregated and
> quantified: a dated advancement line with stable columns. This
> supersedes the reservation of a tag and a release in `CLAUDE.md` and
> `docs/orchestration/workflow.md`, and the half of **D210** that makes a
> release's own notes Alexa's act.
> Recorded in `CLAUDE.md`, *Direction, and what reaches Alexa*.

Three questions for whoever integrates, none of them for Alexa:

1. **D210's fate.** Does the entry above supersede it explicitly, as
   drafted, or does D210 stand for the release notes alone? Finding 1.
   Recommendation: supersede it in the same entry, because a reader of
   D210 next week otherwise re-raises the notes — the exact behaviour
   this change exists to stop.
2. **The two rules dropped to buy the three outcomes** (the French
   example, and "do not ask about file layout, naming, crate internals,
   test style or libraries"). Recommendation: leave them out; both are
   covered in substance and the file is at its cap.
3. **Follow-ups needing a board row**: findings 1 and 2 (the three stale
   places in `workflow.md`, plus `.claude/agents/herd-release.md`),
   finding 3 (where a decision taken alone is told afterwards) and
   finding 4 (the advancement line's columns). Recommendation: one row
   each for 1-2 and 3, and fold 4 into the first advancement report.

## Resume point

`herd-reviewer` reviews commit 2bb47e4 on branch `claude-md-direction`:
whether the three outcomes are the rule Alexa gave and in his order,
whether either dropped rule must come back (and what pays for it), and
the D210 question above. Then the branch joins the batch for
`herd-merger`, which allocates the decision number and opens the board
rows named above.

## Processes

No process started, no PID to report: no `cargo`, no build, no test, no
background job. This task ran `git`, `sed`, `awk`, `fmt`, `perl`, `wc` and
`./scripts/check-docs.sh` only.

No `target/` was created, so there is none to delete: `CARGO_TARGET_DIR`
was never set and nothing compiled. Prose only, so there was no heavy
phase and `./scripts/bench-preflight.sh` did not apply.
