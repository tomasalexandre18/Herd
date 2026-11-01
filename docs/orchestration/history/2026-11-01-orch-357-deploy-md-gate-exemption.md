# orch-357-deploy-md-gate-exemption — handoff 2026-11-01

- **Item:** ORCH-357, plus three `ORCH-NEXT` rows in *Owed* and two
  sentences added to rows that already existed (ORCH-310, ORCH-129)
- **Agent:** herd-step
- **Branch / commit:** `orch-357-deploy-md-gate-exemption` / see the
  agent's report for the sha, based on origin/main
  `958d711ff5600f6282b985675cdf2dd6a9bda435`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`CLAUDE.md`'s gate rule now exempts only the markdown under `deploy/`,
and nothing else there.** The *Commands* paragraph in full, as it now
reads — every word other than the list's sixth entry is the word that
was there before:

```
Gates before a commit: a change outside `CLAUDE.md`, `README.md`, `docs/`,
`.claude/`, `.github/*.md` and only the markdown under `deploy/` runs all four
Rust gates plus `check-docs.sh`. A prose-only change runs `check-docs.sh` alone
and says the Rust gates were **not run** locally, covered by the landing's
`gate.sh`. A gate that did not run is reported as not run, never as passed.
```

Five lines at 74, 78, 79, 74 and 75 columns; `CLAUDE.md` stays at 150
against the 150-line cap of `check-docs.sh:34`-`:37`.

**It is prose where its neighbours are two files, two directories and a
glob, and that is a decision of this branch rather than an accident of
fitting.** The first spelling committed here was `deploy/**.md`, which
**does not match `deploy/release-notes/1.1.0.md` — the file whose
release produced ORCH-357.** `herd-reviewer` blocked on it and was
right; the measurement is *Risks and findings* 1, and the reasoning for
prose over the corrected glob is 2.

**What the boundary is, and why it is not a coverage loss.** `deploy/`
holds **16 files: 7 markdown and 9 that are not** —
`capabilities-wsl-pc.json`, `docker-compose.traefik.yml`,
`docker-compose.yml`, `env.example`, `herd-runner.service`,
`nixos-herd-runner.nix`, `wsl2-check.sh`,
`reverse-proxy/nginx-herd-hub.conf` and
`reverse-proxy/traefik-herd-hub.yml`. The clause's one operative
sentence exempts markdown and makes **no statement about any other
extension**, so all 9 keep the obligation the previous letter gave them,
unchanged and unexamined, and no file's class is decided here.
Exempting `deploy/` wholesale would have been a real loss of coverage
dressed as the correction of a letter.

**What the exemption governs, stated narrowly.** Only whether an agent
pays the compile **locally, before its own commit**. `scripts/gate.sh`
runs steps 0 to 8 over the whole tree at the landing on the gated sha,
`pre-push` refuses a push without that receipt, and
`.github/workflows/ci.yml` has no `paths` filter. **Nothing reaches
`main` ungated by this exemption**, and an earlier draft of this handoff
implied otherwise.

**The ceiling was not raised, and that was not this agent's call to
make.** `check-docs.sh`'s `limit=150` stands. **D251** (ORCH-249,
2026-10-30) already forbids raising it, with a sharper warrant than the
one an earlier draft of this handoff offered on its own authority: *a
limit that exists to force this very discipline would be spent to buy
the lesson, and invariants have been silently compressed to fit it once
already* (ORCH-002, 2026-10-24). This branch is also a live instance of
**ORCH-129**, the row that says a change to `CLAUDE.md` is a constrained
edit and must be briefed as one; see *Risks and findings* 5.

**The board.** ORCH-357 left *Open* for the tail of *Owed*, state
`done`. Three `ORCH-NEXT` rows sit under it and two existing rows gain a
sentence each; all five are listed in *Drafted for `herd-merger`*.

## Not done, and why

1. **`docs/orchestration/gotchas/docs-only-gates.md` is not edited**,
   although it is a second live copy of the rule and is now stale in two
   places. The brief said, of a second holder, *"report it rather than
   widening this one"*, and the gotchas were not among the paths it
   allowed. It is reported and **rowed**, with the final spelling in the
   row so the next agent cannot land a third one.
2. **`scripts/check-docs.sh` is not touched**, deliberately. Two things
   floated and refused: narrowing `CLAUDE.md`'s letter to the script's
   two `deploy/` globs does not fit in five lines, and it would couple
   the method's letter to one script's internal corpus, so a later
   widening of the script would silently change what `CLAUDE.md` says.
   **And its `deploy/` comment wants no correction at all** — see
   *Risks and findings* 4, which withdraws a claim this handoff and a
   board row made about it.
3. **No `docs/decisions.md` byte is written.** One entry is drafted
   below, headed `D-NEXT`; the merger allocates the number (D148,
   invariant 8). The highest decision on `main` is D295.
4. **The three copies of the list in `docs/orchestration/history/` are
   left alone** — `2026-10-26-merge-herd-own-claude-root.md:116`,
   `2026-10-27-merge-batch-docs-gate-orch-batched-pr.md:120` and
   `2026-11-01-release-v1.1.0.md:379`. History is append-only and not
   rewritable (`check-docs.sh:286`-`:296`), and each states correctly
   what the rule was on its own date. The false `nix flake check`
   warrant of 7 also stands at `2026-11-01-merge-batch-39.md:286` for
   the same reason.
5. **No row is opened for the missing `nix flake check` line in
   `CLAUDE.md`'s *Commands* fence.** It is one sentence in **ORCH-310**'s
   next action instead: that row already holds *"a clause that must go
   in `CLAUDE.md`'s *Commands* block and does not fit"*, with the
   11-character budget measured, and the two are the same unit of work
   on the same scarce line. `herd-reviewer` and the orchestrator each
   withdrew an *"earns a row"* on it.
6. **The batch-39 landing narrative in `board.md`'s *Next* section is
   left alone.** It says the question *"edits `CLAUDE.md` and is
   therefore Alexa's, with a recommendation in the row"*, true of that
   landing and not falsified by his having since decided.

## Gates

**This branch is prose only.** `CLAUDE.md`,
`docs/orchestration/board.md` and this file. No `.rs`, no `Cargo.toml`
byte, no `Cargo.lock` byte, no script, no workflow — and neither
correction after the review pass changed that, so the gate class is the
one the first commit had.

**The four Rust gates were NOT RUN.** `nix flake check`,
`cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`nix develop --command ./scripts/test.sh` were not run on this branch.
Nothing they read moved. **What covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s and not
this agent's.** A gate that did not run is reported as not run, never as
passed.

**`./scripts/bench-preflight.sh` was not run and did not need to be:**
nothing was submitted to Herd, no compile was started, no job touched
the queue. It gates a heavy phase and this branch has none.

**Runs made before this file was committed**, verbatim, the same pair on
each of the three commits:

```
$ ./scripts/check-docs.sh
docs ok

$ ./scripts/check-orchestration.sh
check-orchestration: 361 passed, 0 failed
```

**The last run, named and not quoted** — it is made on the committed
tree, after this file, which is why its output goes to the agent's
report and not into the tree it gates: `./scripts/check-docs.sh` and
`./scripts/check-orchestration.sh` from the worktree root. Neither
writes a log file; `check-docs.sh` prints `docs ok` on success and notes
to stderr on failure, `check-orchestration.sh` ends on its
passed/failed line.

**Skips: none.** This branch adds no `HERD-SKIPPED` line, no test skip
and no `#[ignore]`, and runs no suite that could skip one.

## Risks and findings

**1. The committed spelling `deploy/**.md` did not match the file this
row exists for, and I propagated it without testing it.**
`herd-reviewer` blocked; the block is correct. `**` is a recursive
wildcard only as a **whole path component**, so `deploy/**.md` degrades
to `deploy/*.md`. Measured here in both engines before anything was
rebuilt:

```
$ git ls-files ':(glob)deploy/**.md'
deploy/README.md
deploy/release-notice.md

$ git ls-files ':(glob)deploy/**/*.md'
deploy/README.md
deploy/release-notes/0.4.0.md
deploy/release-notes/0.4.1.md
deploy/release-notes/1.0.0.md
deploy/release-notes/1.1.0.md
deploy/release-notes/README.md
deploy/release-notice.md

$ shopt -s globstar; printf '%s\n' deploy/**.md
deploy/README.md
deploy/release-notice.md
```

`deploy/release-notes/1.1.0.md` is absent from the first and present in
the second. **The spelling was not invented on this branch** — it came
from the v1.1.0 handoff, through the board row's recommendation, into
what Alexa validated — but this branch is where it was committed, and
testing a glob before writing it into the method was this agent's job.

**2. The two wordings, measured.** Reproduced with a greedy wrapper
using `check-docs.sh`'s own column count (codepoints, not bytes):

| wording | lines | columns |
|---|---|---|
| `deploy/**/*.md` (markdown only) | **5** | 74, 77, 79, 74, 75 |
| only the markdown under `deploy/` | **5** | 74, 78, 79, 74, 75 |
| `deploy/**/*.md` (its markdown only) | 6 | — |
| under `deploy/`, its markdown alone | 6 | — |

Both five-line forms fit; greedy wrapping is line-count-optimal, so
there is no five-line arrangement of the six-line rows. **Prose was
chosen over the correct glob because a glob in this document has no
engine.** Nothing mechanical reads this list (4), so a glob here is
evaluated only by a reader's belief about a dialect — which is how
`deploy/**.md` crossed four documents unexpanded. `deploy/**/*.md` is
correct, and it would still require a reader to know that `**/` matches
**zero** directories before they could place `deploy/README.md` and
`deploy/release-notice.md`, which are 2 of the 7. Words have no
expansion to get wrong at any depth. The orchestrator raised a grammar
objection to the worded form and withdrew it.

**3. `deploy/` holds 7 markdown files and 9 others, not 5 and 11.** The
first version of this handoff, its board row and its drafted entry all
said *"five markdown, and eleven that are not"* while naming nine files
by name — two wrong figures and an internal contradiction.
`git ls-files deploy | grep -c '\.md$'` is **7**, the complement is
**9**, the total is 16. The 5 was propagated from
`check-docs.sh:87`-`:88`, which says *"all five markdown files under
`deploy/`"* and was true when written on 2026-10-30, before
`release-notes/1.0.0.md` and `1.1.0.md` existed. **A figure in a comment
is dated even when the comment does not say so.** That stale figure is
now rowed, in the same comment block and the same edit.

**4. A refutation this handoff and a board row carried is withdrawn:
`check-docs.sh`'s `deploy/` comment is correctly reasoned as written.**
An earlier version of this branch said its stated reason for excluding
`deploy/*/*.md` *"does not survive"* and *"cannot be the reason"*, and
set a board row's next action to correcting it. **That was a misreading
of the comment's structure.** `:90`-`:96` states **two choices with two
reasons in matching order**: `.md`-only because
`docker-compose.yml`, `env.example`, `herd-runner.service`,
`nixos-herd-runner.nix`, `wsl2-check.sh` and the two files under
`reverse-proxy/` *"are not prose and have no business being
width-checked"*; and one-directory-at-a-time because `deploy/*/*.md`
*"would have pulled in whatever the next subdirectory turns out to
hold"*. The refutation attacked the **first** reason as though it were
offered for the **second** choice. Nothing in that comment needs
fixing, and the row now says so; if it is touched at all it is to
**add** the hazard it leaves implicit. Raised by `herd-reviewer`,
relayed to this agent as a refutation by the orchestrator, and withdrawn
by both — recorded here because it reached a board row before it was
caught.

**5. The constraint that shaped this task was discovered, not briefed —
which is ORCH-129's own content.** ORCH-129 is `open` and its next
action is *"brief it as a constrained edit, not an edit: any task that
must add to `CLAUDE.md` is told the two figures and told which bullet it
may shorten in exchange"*. This brief named neither the 150-line cap nor
the 79-column width; both were found by reading `check-docs.sh` after
the first draft of the clause had already been written and did not fit.
No harm resulted — the clause fits and the ceiling held — but the row
exists so that this is observed rather than survived, so it is recorded
in ORCH-129's own *Evidence* cell with its date. **And the refusal to
raise the cap was already settled by D251**, which an earlier draft
presented as this agent's judgment; the attribution is corrected in
*Done*, in the row and in the drafted entry.

**6. Nothing mechanical holds the exempt list *today*, and that claim
needs the qualifier it did not have.** `check-docs.sh:168`-`:170`
already asserts literal strings in `CLAUDE.md` with `grep -qF` — it
loops over `docs/orchestration/board.md`,
`docs/orchestration/workflow.md` and `.claude/agents` and notes when one
is no longer named — and `check-orchestration.sh:143`-`:144` proves that
exact class red by mutation. **So the shape that could hold this list
exists**, and *"nothing mechanical holds it"* is a statement about today
and not about what is possible. It is rowed. The unqualified version was
this agent's and `herd-reviewer` refuted it.

What the search itself established stands, and the detector was proven
on a known-bad fixture before its silence was trusted (`grep` here is
ugrep 7.8.4). Fixture: copies of `scripts/`, `.claude/hooks/`,
`.github/workflows/`, `flake.nix` and `Cargo.toml` into a scratch
directory, plus a planted `scripts/planted-bad.sh` holding
`case "$1" in CLAUDE.md|README.md|docs/*|.claude/*|.github/*.md)`.

- **D1**, `grep -rnF -e '.github/*.md'`: **fired on the planted file and
  on nothing else in that tree.**
- **D2**, `CLAUDE\.md` piped through `README\.md` piped through
  `docs/`: **fired on the planted file**, plus two real lines that are
  different lists — `check-docs.sh:97`, the 79-column prose corpus, and
  `check-orchestration.sh:325`, the file list its meta-gate copies into
  a scratch repo. Neither is a gate-exempt list.

On the committed tree, excluding this file,
`git grep -nF -e '.github/*.md'` returns **7 lines in 6 files**, not the
five an earlier draft claimed while enumerating six: `CLAUDE.md:85`;
`board.md:211` and `:212`, which quote the list rather than hold it;
`gotchas/docs-only-gates.md:12`, which **does** hold it and is stale;
and the three dated history files of *Not done* 4. Every hit is prose.
`git grep -nF -e 'Gates before a commit'` returns `CLAUDE.md:84` and
three history references. All five `.claude/agents/*.md` have
`grep -c 'README\.md'` of 0; `herd-step.md:19`-`:21` says *"Prose
only"* with no directory list.
`.claude/hooks/orchestrator-guard.sh:126` holds a path list that is not
this one — what the orchestrator may not write in the main checkout,
with `deploy/` on it already — and is unaffected.

**7. The warrant, corrected three times before it was right.** The
brief's and the board row's reason for keeping `deploy/*.nix` on the
gated side was that it and `deploy/env.example` *"are read by `nix flake
check`"*, gate step 3. **They are not:** `flake.nix:27`-`:35` builds
`src` with `lib.fileset.toSource` over a `lib.fileset.unions` of exactly
`./Cargo.toml`, `./Cargo.lock`, `./herd-proto`, `./herd-hub`,
`./herd-runner` and `./herdctl`, its own comment saying it is
*"narrowed so that editing `docs/` or this comment does not rebuild
Rust"*; `flake.nix` declares **no `checks` and no `nixosModules`
output**; and `deploy/nixos-herd-runner.nix:74` is the file itself
describing adding a `nixosModules.herd-runner` entry *"to flake.nix"* as
a thing not done, while `:39` shows the consumer importing it from their
own `/etc/nixos`, which is their machine and not ours (D161).

The replacement then claimed `check-docs.sh` was *"the only gate that
reads any file under `deploy/`"* — a true claim with a false qualifier,
since `check-orchestration.sh` reads that markdown too. **And the fix
for that was itself loose:** *"two prose gates read its markdown"* is
not true of all 7 files. `check-orchestration.sh`'s `fresh()` at
`:99`-`:103` copies `deploy/release-notes` **alone** into each of 49
corpora, 48 of which run `check-docs.sh`, so the 5 files under
`release-notes/` have two prose readers and `deploy/README.md` and
`deploy/release-notice.md` have exactly one. Stated precisely, and this
is the form in the clause's row and the entry: **no Rust gate reads any
file under `deploy/`; `check-docs.sh` reads all 7 markdown files;
`check-orchestration.sh` reads the 5 under `release-notes/`.**

**8. The operative test is `.md`, not the word markdown.** A future
`deploy/notes.markdown` would be markdown to a reader and therefore
exempt by `CLAUDE.md:85`, and `.md` to no glob and therefore read by no
prose gate. None exists today — all 7 are `.md` — and it is one of two
routes into the gap the corpus row carries, the other being a third
directory under `deploy/`. **Both are true before this change and after
it, and neither is caused by it:** the exemption changed which files owe
a local compile, not which files a prose gate reads.

**9. There is no `deploy/hub.nix`.** The brief used it as the file a
reader should be able to place; the `.nix` under `deploy/` is
`deploy/nixos-herd-runner.nix`.

## Drafted for `herd-merger`

**One `docs/decisions.md` entry.** The ruling and its reasoning first,
since the orchestrator asked this agent to decide and justify it.

**It earns an entry.** The precedent cuts both ways — ORCH-070 went into
`decisions.md` because prose was its only holder, and the board-row
indent guard did not because the guard and its comment were the record.
Three reasons, and the entry argues the third against **D251** by name,
because D251 is the standing ruling on where a rule that does not fit
`CLAUDE.md` goes and a reader who finds it would otherwise reopen the
question:

1. **`CLAUDE.md` holds the rule and cannot hold the warrant.** The
   boundary is the load-bearing half, and the only reason anyone accepts
   it over *"add `deploy/`"* is 7 above — a warrant that was **false in
   three documents, then over-qualified, then still loose** before it
   came out right. Six words of clause cannot carry that, and the
   150-line cap means they never will.
2. **Nothing mechanical holds it today, and something could.**
   `check-docs.sh:168`-`:170` is the shape, proven red as a class by
   `check-orchestration.sh:143`-`:144`. Until that row is built the
   boundary's only guards are prose sentences, one of which — the
   gotcha — is already stale.
3. **The notation is the residual risk, it is not guarded, and
   `workflow.md` is not where it belongs.** D251 routes an operational
   rule that does not fit `CLAUDE.md` to `workflow.md`. What has no home
   here is not an operational rule but a warrant, and `workflow.md`
   holds method rather than rationale: nothing an agent must *do* would
   change if the four paragraphs below were pasted into it. This
   replaces an argument in the first draft of this handoff which claimed
   the boundary rode on a droppable parenthetical; `herd-reviewer`
   refuted it — with the glob still in the clause the `.md` carried the
   boundary, and widening needed a visible semantic edit — and it is
   struck rather than reworded, because a false argument cannot be
   corrected once appended (D148).

The entry, verbatim, with `D-NEXT` replaced by the next number. The
header is 77 columns, measured unfenced, so it does not trip the
79-column rule when it lands.

```markdown
## 2026-11-01 — D-NEXT Only `deploy/`'s markdown is gate-exempt, and in words

`CLAUDE.md`'s *Commands* paragraph exempted by directory —
`CLAUDE.md`, `README.md`, `docs/`, `.claude/`, `.github/*.md` — and
`deploy/` was not on the list. By its letter a markdown-only change
under `deploy/` owed all four Rust gates locally, although no Rust
tool reads one. `deploy/release-notes/<version>.md` is prose that
ships in the tree (ORCH-162), and v1.1.0's cut hit the case exactly:
it ran `check-docs.sh` alone and reported the four as **not run, never
as passed**, which was right under the letter as it stood.

**The rule now exempts only the markdown under `deploy/` and nothing
else there** (Alexa, 2026-11-01, ORCH-357).

**What it governs is narrow.** Only whether an agent pays the compile
locally before its own commit. `scripts/gate.sh` runs steps 0 to 8
over the whole tree at the landing on the gated sha, `pre-push`
refuses a push without that receipt, and `ci.yml` has no `paths`
filter. Nothing reaches `main` ungated by this exemption.

**The boundary is the decision, not the exemption.** Exempting
`deploy/` wholesale would have been a loss of coverage dressed as the
correction of a letter. `deploy/` holds 16 files: 7 markdown, and 9
that are not — a `.nix`, a `.service`, a `.json`, two `docker-compose`
YAMLs, a `.sh`, `env.example` and two files under `reverse-proxy/`.
The clause's one operative sentence exempts markdown and makes no
statement about any other extension, so all 9 keep the obligation the
previous letter gave them and no file's class is decided here.

**The warrant.** ORCH-357's row recommended the narrow exemption on
the ground that `deploy/*.nix` and `deploy/env.example` *"are read by
`nix flake check`"*. **They are not.** `flake.nix` narrows `src` with
`lib.fileset.unions` to `Cargo.toml`, `Cargo.lock` and the four
crates, declares no `checks` and no `nixosModules` output, and leaves
`deploy/nixos-herd-runner.nix` to the consumer's own `/etc/nixos`
(D161). What holds instead, and is enough: **no Rust gate reads any
file under `deploy/`.** `check-docs.sh` reads all 7 of its markdown
files; `check-orchestration.sh` reads the 5 under `release-notes/`,
which `fresh()` copies into each of its corpora. **What no Rust gate
reads is not something an exemption may assume on its behalf**:
exempting a file because a gate covers it is a measured claim,
exempting one because no gate would notice is the opposite, and that
is why the boundary stops at markdown. The operative test is **`.md`**
— a `deploy/notes.markdown` would be markdown to a reader and `.md` to
no glob, so exempt here and read by no prose gate. None exists; the
hazard is rowed.

**It is written in words, not a glob, and that is part of the
decision.** The first spelling was `` `deploy/**.md` ``, carried from
the v1.1.0 handoff into the board's recommendation, into this
validation, and into one commit. **It does not match
`deploy/release-notes/1.1.0.md`** — the file whose release produced
the row. `**` is recursive only as a whole path component, so
`deploy/**.md` degrades to `deploy/*.md`; `git ls-files` with
`:(glob)` and `bash` with `globstar` both return `deploy/README.md`
and `deploy/release-notice.md` alone. **A glob in a method document
has no engine.** Nothing mechanical reads this list, so a glob here is
evaluated only by a reader's belief about a dialect — which is how a
wrong one crossed four documents unexpanded. `deploy/**/*.md` is
correct and was measured as fitting, and was still not chosen: two of
the seven markdown files sit directly under `deploy/`, so a reader
must know that `**/` matches no directory at all before they can place
them. Words have no expansion to get wrong.

**Why this is recorded, and why here.** The rule's holder is prose
every agent reads, so the rule needs no second record; the **warrant**
and the **notation** do. `check-docs.sh` caps `CLAUDE.md` at 150 lines
and it is at 150, with the gates paragraph at five lines and one of
them at exactly 79 columns, so the paragraphs above cannot live there.
**D251** (ORCH-249) is why the cap was not raised rather than
negotiated — a limit that exists to force this discipline would be
spent to buy the lesson — and D251 also routes a rule that does not
fit `CLAUDE.md` to `workflow.md` *Concurrency and host rules*. **This
is not that case**: what has no home is not an operational rule but a
warrant, and `workflow.md` holds method rather than rationale, so
nothing an agent must do would change if this entry were pasted into
it. Nothing mechanical holds the list **today** — searched across
`scripts/`, `.claude/hooks/`, `.github/workflows/`, `flake.nix` and
`Cargo.toml` with a detector first proven on a planted copy — though
`check-docs.sh:168` already asserts literal `CLAUDE.md` strings with
`grep -qF` and is the shape that could, which is rowed separately. So
the boundary's only guards are prose, and the standing risk is a later
editor regularising the words back into a glob whose dialect they have
not tested. `CLAUDE.md` is rewritten; this file is appended to. That
is why it is recorded here.

D27, D148, D161, D251, D279, ORCH-129, ORCH-162, ORCH-357.
```

**Five `board.md` edits, all in the tree on this branch**, so none needs
pasting. **Three numbers to allocate, in document order:**

1. `board.md:211`, *Owed*, `| ORCH-357 | … | herd-step | done |` — this
   task. Already numbered.
2. `board.md:212`, *Owed*, `| ORCH-NEXT | … | herd-step | owed |` — the
   two stalenesses in `gotchas/docs-only-gates.md`: the five-entry list,
   and a *Verify* step that asks for a green `check` job on a pull
   request nobody opens. Its *Next action* carries the **final**
   spelling to copy.
3. `board.md:213`, *Owed*, `| ORCH-NEXT | … | herd-planner | owed |` —
   `check-docs.sh:168` is the shape that could hold the exempt list
   mechanically.
4. `board.md:214`, *Owed*, `| ORCH-NEXT | … | herd-planner | owed |` —
   the corpus reads less of `deploy/`'s markdown than `CLAUDE.md`
   exempts, by two routes, plus the stale *"all five markdown files"*
   figure in the same comment block.

**3 and 4 edit the same script and cross-reference each other**: both
owe all four Rust gates and a reviewer pass on `scripts/check-docs.sh`,
so whoever picks up either should carry both and pay that once. Neither
is a condition of this landing.

Two rows that already exist gain one sentence each, **no number
needed**: **ORCH-310**'s next action now carries the missing `nix flake
check` line in the *Commands* fence, since it is the same scarce line
that row already measured; **ORCH-129**'s evidence now carries this
branch as a dated instance of a constrained edit briefed as an edit.

Nothing else is drafted: no `D-NEXT` reference left anywhere in the tree
to renumber, no `cut-v*`, no tag.

## Decisions needed

**None blocking.** One call is made here rather than asked, and it is
reversible on an unlanded branch: **prose over the corrected glob** in
`CLAUDE.md`, argued in *Risks and findings* 2. It survived a reviewer
pass and a withdrawn grammar objection. The drafted entry is drafted
rather than skipped on the orchestrator's explicit ruling that this
agent decide and justify it.

## Resume point

**`herd-merger`, not review.** The reviewer's block and the
orchestrator's five acceptance changes are all addressed, and neither
`CLAUDE.md` nor any gate script changed after the review pass — the
five changes are confined to `board.md` and this file. The merger
integrates the branch in the batch's one landing, allocating **three**
`ORCH-NEXT` numbers in document order and **one** decision number, and
runs `./scripts/gate.sh` on the gated sha, which is what covers the four
Rust gates this branch did not run.

## Processes

**None.** No process started, no job submitted to Herd, no compile run,
no PID killed. `CARGO_TARGET_DIR` was never set and **no `target/` was
created**, because this branch touches no code — so there is none to
delete (`workflow.md`, *Concurrency and host rules*). The scratch
fixture of *Risks and findings* 6 was a copy under this agent's own
scratchpad subdirectory, outside the worktree and the repository, and is
removed.
