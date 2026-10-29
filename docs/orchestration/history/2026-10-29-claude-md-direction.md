# claude-md-direction — handoff 2026-10-29

- **Item:** ORCH-nnn (rule change; no board row was allocated to this task)
- **Agent:** herd-step
- **Branch / commits:** `claude-md-direction` — `2bb47e4` (the rule),
  `c273dee` (this handoff, first version), `7d70e85` (the review's named
  changes), based on origin/main
  `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`
- **Review:** `herd-reviewer` returned "land with named changes"; all of
  them are applied in `7d70e85` and listed below.

## Done

`CLAUDE.md` carries Alexa's direction of 2026-10-29 instead of depending
on a conversation. **One file changed**, 64 insertions / 64 deletions,
150 lines before and after. `docs/orchestration/workflow.md` is back at
`origin/main`, byte-identical — see *The tag rule, narrowed*.

**The new section, replacing *When to ask Alexa*: *Direction, and what
reaches Alexa*** (`CLAUDE.md:115-143`, 29 lines). It says, in this order:

1. **First duty: keep bounded work in flight so the project advances**,
   above any single event — a red `main`, a release or a live defect must
   not drain the queue. **Answering Alexa is the second duty.** The order
   is written down because the order is the point.
2. **The triage has three outcomes, in their order:**
   - **Ask him** when the answer changes *what* gets done — what the
     product *is* or *promises* — and never *how*: **anything
     structural, the method or the shape of the product**, scope, step
     order, an irreversible act, **anything on the consumer's side**.
     One question with a recommendation in the board's *Waiting*; other
     work goes on. **A permission the system refuses is escalated, never
     worked around**: a peer refused it cannot delegate it.
   - **Decide alone** what is inside the mandate and **reversible**: not
     submitted beforehand, **told afterwards in the session-end report**.
   - **Report, with no question attached,** what could cost him later: a
     trap that would destroy something, a threshold being approached, a
     structural decision arriving from elsewhere. Early enough to matter.
3. **How** is never his: not a technical choice, an implementation, an
   API, a defect, the tooling.
4. **A report is aggregated and quantified**, with its columns named:
   date · milestone · musts done/total · open rows · red gates.
5. **A guidance he has given is standing**: followed, never re-raised,
   with the live ones as bullets — no compatibility to preserve (the
   consumer resets entirely at v1.0.0), GitHub is not the centre of this
   project and local git is, and no compatibility question gates a tag.

**Who decides what** moved into *Roles*: Alexa is the project's
**director** and gives the direction; the orchestrator **manages**; the
execution agents **build**. The clause that made Alexa "the word for
anything outward-facing" is gone, and so is "escalates decisions" as a
duty of the orchestrator's own.

## The tag rule, narrowed — the review's central finding

The first commit wrote "**a tag and a release no longer wait on his
word**" in `CLAUDE.md` and in `workflow.md`. That was wrong twice, and
the brief that asked for it says so itself:

- **It granted the act to nobody.** The orchestrator is refused by its
  own charter and by `orchestrator-guard.sh`; `herd-release` by its
  charter and by `release-guard.sh`; `herd-merger` **contractually by
  `docs/orchestration/permissions.md:137`** — *"push a tag with the
  version batch | refused | contractual (Alexa's word per release
  act)"*, which refuses **on the very sentence that commit deleted**.
  Followed exactly, the deleted-reservation version sends the next tag
  question back to Alexa anyway.
- **The quote does not support it.** *"pour le tag on sens fou tout vas
  etre reset"* sits inside the compatibility paragraph: it lifts a
  **compatibility** worry off a tag, not the **act**.

So the bullet now claims only what the evidence supports, and names
where the act's reservation lives:

> - **No compatibility question gates a tag**: the consumer resets at
>   v1.0.0, so a version is cut when it is useful, not when he is asked.
>   The act stays reserved: `workflow.md`, `permissions.md`,
>   `.claude/agents/herd-release.md`.

And `docs/orchestration/workflow.md` was **reverted to `origin/main`**:
its two sentences reserving a tag and a release stand as they were. The
cross-reference now points at the three files that do carry the
reservation, not at *Git and main*, which no longer states it.

**Who performs the act is an open permission question** going to Alexa
separately — which is what outcome 1 says it is. Board row proposed
below.

## The other named changes from the review

| Change | Why it was needed |
|---|---|
| outcome 1 names **anything structural — the method, the shape of the product** | *structurant* is the word in his instruction and appeared nowhere in the section; outcome 3 is a risk list, so a structural choice that breaks nothing reached him under no outcome at all. Echoed in outcome 3 as "a structural decision arriving from elsewhere", at no line cost |
| **"anything on the consumer's side"** replaces "money or hardware" | "money or hardware" was unevidenced; `CLAUDE.md` read alone no longer reserved their side at all, and that case has a real victim |
| GitHub bullet: "a run id is not a report, a green run is not a decision" | "never a report" gagged the one GitHub fact that must reach him, and could read as licence to act on the consumer's repository |
| **"edits a repository file"** restored at `:13-14` | without "repository" the orchestrator may never edit any file, which it must do every session to write a brief. 78 columns, measured |
| the advancement line's **columns named** | a rule whose columns are fixed by the first report is checkable only after the fact |
| outcome 2 loses **"or riskless"** and gains **"in the session-end report"** | "riskless" let an irreversible act be taken alone on self-assessed harmlessness, which outcome 1 forbids, and it is the least falsifiable predicate in the section. The channel was not missing from the project — the first commit **deleted** it ("writes the decision down through an agent"); naming the session-end report restores it |
| the freed line restores **"A gate that did not run is reported as not run, never as passed"** | as narrowed, a code change could run three Rust gates and report the fourth as passed; that sentence is the orchestrator's twin of D27 |

## The 150-line cap: not moved, and why it cannot be

`CLAUDE.md` is **150 lines against `scripts/check-docs.sh`'s limit of
150**, the same count as `origin/main`, and no line exceeds 79 columns.

Moving the cap was ruled out on a fact, not a preference: `limit=150`
lives in `scripts/check-docs.sh`, and `scripts/` is **outside** the
prose paths of *Commands*. Editing it would make this a code change
owing all four Rust gates, which this task is forbidden to run (no
`cargo`; sibling builds on the host). So every line was paid for. Per
section, including each section's trailing blank:

| Section | Before | After | What left |
|---|---|---|---|
| Roles | 17 | 14 | "decides *how* … does not ask" (now the new section); the enumeration behind "English in everything written" |
| Vocabulary | 14 | 9 | "WebSocket", "in docker compose", "Dials out to the hub" (invariant 3), "(AI agent, merger, nightly task)"; the closing note compressed to one line, keeping the mapping and "translate" |
| Invariants | 34 | 34 | untouched, as the brief requires |
| Layout | 11 | 5 | the fence, the column alignment, the `(library)`/`(binary)` markers, the per-crate descriptions |
| Commands | 19 | 18 | the "Toolchain comes from the Nix flake" sentence, folded into "`cargo` is not on PATH"; the did-not-run sentence is **back**, in full |
| Git and main | 17 | 13 | the tag/release bullet, "on the orchestrator's instruction", "nothing waits for Alexa" (now the new section) |
| Shared host rules | 15 | 14 | "build or test", "free" memory/disk, "too, at", "(workflow.md)" on the `target/` sentence |
| *When to ask Alexa* → *Direction…* | 9 | 30 | the section, replaced |
| Session start, session end | 7 | 6 | "and reports in three or four lines" — the new section says what a report is; the board is "the board" here, its path still in invariant 8 |

**Two rules stay dropped**, as the review accepted after trying to break
the case: Alexa's *"j'ai rien eu et j'ai pas besoin"* example (the
*what/how* test carries the rule it illustrated; the quote survives in
the proposed decision text) and "do not ask about file layout, naming,
crate internals, test style or libraries" (covered by outcome 1's
*never how* and outcome 2). **The compressions stand**, likewise
reviewed.

## Not done, and why

- **No entry in `docs/decisions.md`.** The brief forbids it and a
  `D-NEXT` header is a `check-docs.sh` failure. The proposed text is
  under *Decisions needed*; `herd-merger` allocates the number.
- **`docs/status.md` and `docs/orchestration/board.md` untouched**: the
  integrator owns both. The four rows this change needs are proposed
  below, not written.
- **`docs/orchestration/workflow.md` untouched** (reverted): the rule it
  carries is the one that stands.
- **The Rust gates were not run** — see *Gates*.

## Gates

Prose only. **The four Rust gates were NOT RUN**, and nothing here
claims they passed; what covers them is the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s run and not
this agent's.

`./scripts/check-docs.sh`, run on the tree of commit `7d70e85`, verbatim:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Exit code 0. The `D170` line is a pre-existing `note:`, about a decision
number this task did not touch.

Supporting measurements, verbatim, on the same tree:

```
$ wc -l CLAUDE.md
150 CLAUDE.md
$ LC_ALL=C awk '{l=$0;c=length(l)-gsub(/[\200-\277]/,"",l); \
    if (c>79) printf "%d: %d cols\n", FNR, c}' CLAUDE.md
(no output)
$ git diff --stat cd0f1b7 -- docs/orchestration/workflow.md
(no output — identical to origin/main)
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

The same scan over `CLAUDE.md` is clean: every edit used `perl -0777` on
raw bytes, never `\x{...}` escapes.

The **last run**, on the committed tree that includes this handoff, is
`./scripts/check-docs.sh` from the worktree root; its output goes to the
agent's report, not into the tree it gates.

**Skips:** no skip. No `HERD-SKIPPED` line was produced or copied — no
test suite ran.

## Risks and findings

1. **Nobody can perform a tag today**, and the narrowed rule does not
   fix that — it only stops claiming otherwise. Three refusals stand:
   `orchestrator-guard.sh` and the orchestrator's charter, the
   `herd-release` charter plus `release-guard.sh`, and
   `permissions.md:137` for `herd-merger`. Alexa must name the actor;
   that is a permission question under outcome 1, not a technical one.
2. **The release residue spans four files**, all left as they are
   because the reservation stands: `workflow.md` (the agent table at
   line 14, step 8 *Release* at lines 75-85), `permissions.md:137`,
   `.claude/agents/herd-release.md`, and `release-guard.sh`, the hook
   that enforces it. They become stale only if Alexa moves the actor.
3. **D210 stands and must not be superseded** — the first version of
   this handoff proposed to, and that was wrong. It does not rest on his
   word: it rests on two measured permission walls, says in terms that
   neither is to be weakened, and carries its own retirement condition
   (ORCH-162 shipping the notes in the tree). What the new rule
   legitimately withdraws is only D210's **class** premise — it pointed
   at a *When to ask Alexa* section that no longer exists in that form.
4. **The first duty has no threshold.** "Keep bounded work in flight"
   names no number, so nothing says when the queue is too thin; it is
   the failure being recorded and it is still unfalsifiable.
5. **The quantified roadmap does not exist yet.** The section now fixes
   the columns, which is half of *"feuille de route chiffré"*; nobody
   has produced the line itself.
6. **Nothing was softened.** Where his words are the rule they are
   quoted (in the proposed decision), and the file records the rule, not
   the apology.
7. **A brief of mine described this change smaller than it is.** The
   mid-task brief called it three sections; the diff rewrites **eight**,
   two of them changing rules rather than wording (*Commands*' gate
   sentence, *Git and main*' tag bullet). The table above was in the
   first handoff and was not carried forward into that brief. Worth a
   habit: when a brief describes an agent's own diff back to it and gets
   it smaller than it is, the agent says so.

## Decisions needed

**The entry this change deserves, for `herd-merger` to number** (no
placeholder header is written: a `D-NEXT` header fails `check-docs.sh`):

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
> Github c'est la meme histoire"*, *"pour le tag on sens fou tout vas
> etre reset"*, *"je veux des feuille de route chiffré et pouvoir
> comparer les avancement"*.
>
> The orchestrator's first duty is to keep bounded work in flight so the
> project advances; it stands above any single event, and a red `main`,
> a release or a live defect must not drain the queue. Answering Alexa
> is the second duty. The triage has three outcomes, in order: **ask
> him** when the answer changes what gets done — anything structural,
> the method or the shape of the product, what the product is or
> promises, scope, step order, an irreversible act, anything on the
> consumer's side — and never how, one question with a recommendation,
> and a permission the system refuses is escalated and never worked
> around, by a peer too; **decide alone** what is inside the mandate and
> reversible, not submitted beforehand and told afterwards in the
> session-end report; **report with no question attached** what could
> cost him later, early enough to matter. A guidance already given is
> standing and is not re-raised: no compatibility to preserve (the
> consumer resets entirely at v1.0.0), GitHub is tooling and not the
> centre of this project — a run id is not a report, a green run is not
> a decision — and **no compatibility question gates a tag**: a version
> is cut when it is useful, not when he is asked. Reports to him are
> aggregated and quantified: one dated line, columns date · milestone ·
> musts done/total · open rows · red gates.
>
> This supersedes the reservation of a tag and a release in `CLAUDE.md`
> and `docs/orchestration/workflow.md`. **D210 stands**: a published
> release's notes are Alexa's own act, not because a release waits on
> his word but because two permission walls refuse the write to every
> agent and to the orchestrator, and neither is to be weakened (D210).
> It retires when ORCH-162 ships the notes in the tree. No brief
> re-raises it; a release brief carries the paste in the same order as
> the tag.
> Recorded in `CLAUDE.md`, *Direction, and what reaches Alexa*.

**One thing for the integrator to reconcile before numbering it.** The
supersession sentence above — the reviewer's wording, kept verbatim as
instructed — says the reservation of a tag and a release in `CLAUDE.md`
and `workflow.md` is superseded. As **landed**, that is wider than the
tree: `CLAUDE.md` supersedes only the **compatibility gate**, the act's
reservation stands, and `workflow.md` is unchanged. Either the sentence
narrows to "supersedes the compatibility gate on a tag; the act stays
reserved pending ORCH-nnn", or Alexa's answer on the actor (row 1 below)
lands first and makes the wider sentence true. Recommendation: narrow it
now and let the actor's answer carry its own entry — a decision should
not describe a tree that does not exist yet.

## Board rows proposed (not written: the integrator owns the board)

| Item | Origin | Owner | State | Next action |
|---|---|---|---|---|
| **Who performs a tag?** Three refusals stand (orchestrator charter + `orchestrator-guard.sh`; `herd-release` charter + `release-guard.sh`; `permissions.md:137` for `herd-merger`), so the act is granted to nobody. A permission question, not a technical one | this review, 2026-10-29 | Alexa | waiting | ask Alexa, one question with a recommendation: name the actor, or keep the act his |
| **The release residue across four files** — `workflow.md` (table line 14, step 8), `permissions.md:137`, `.claude/agents/herd-release.md`, `release-guard.sh` — consistent today, stale the moment the actor moves | this review, 2026-10-29 | herd-step | blocked | after the row above is answered, one bounded task over the four files |
| **The quantified roadmap**: produce the advancement line the new rule fixes the columns for (date · milestone · musts done/total · open rows · red gates), the half of *"feuille de route chiffré"* nobody has delivered | Alexa, 2026-10-29 | orchestrator | open | emit it at the next session end and keep the columns |
| **The first duty has no threshold**: "keep bounded work in flight" names no number, so a thin queue cannot be called thin | this review, 2026-10-29 | herd-planner | open | propose a floor (tasks in flight, or work-days queued) and one line for `CLAUDE.md` |

## Resume point

`herd-reviewer` re-reads `7d70e85` on `claude-md-direction` if it wants
to confirm the named changes; otherwise the branch joins the batch for
`herd-merger`, which numbers the decision (narrowing its supersession
sentence per *Decisions needed*) and opens the four rows above. The tag
actor goes to Alexa as one question, with a recommendation.

## Processes

No process started, no PID to report: no `cargo`, no build, no test, no
background job. This task ran `git`, `sed`, `awk`, `fmt`, `perl`, `wc`
and `./scripts/check-docs.sh` only.

No `target/` was created, so there is none to delete: `CARGO_TARGET_DIR`
was never set and nothing compiled. Prose only, so there was no heavy
phase and `./scripts/bench-preflight.sh` did not apply.
