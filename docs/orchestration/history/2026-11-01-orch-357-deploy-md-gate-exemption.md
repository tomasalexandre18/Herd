# orch-357-deploy-md-gate-exemption — handoff 2026-11-01

- **Item:** ORCH-357 (plus one `ORCH-NEXT` row this task files, in
  *Owed*, for the second live copy of the list it found)
- **Agent:** herd-step
- **Branch / commit:** `orch-357-deploy-md-gate-exemption` / see the
  agent's report for the sha, based on origin/main
  `958d711ff5600f6282b985675cdf2dd6a9bda435`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`CLAUDE.md`'s gate rule now exempts `deploy/**.md` and nothing else
under `deploy/`.** The *Commands* paragraph, in full, as it now reads —
every word of it other than the list entry and its four-word qualifier
is the word that was there before:

```
Gates before a commit: a change outside `CLAUDE.md`, `README.md`, `docs/`,
`.claude/`, `.github/*.md` and `deploy/**.md` (its markdown only) runs all four
Rust gates plus `check-docs.sh`. A prose-only change runs `check-docs.sh` alone
and says the Rust gates were **not run** locally, covered by the landing's
`gate.sh`. A gate that did not run is reported as not run, never as passed.
```

The diff is four lines replaced by four, and the two things added are
`` `deploy/**.md` `` in the list and `(its markdown only)` beside it.
Nothing else in the file moved.

**Why the qualifier is there and why it is four words and not twenty.**
A reader standing at the rule can place both of the files that matter
without leaving the paragraph: `deploy/release-notes/1.1.0.md` is
markdown under `deploy/`, so it matches and is exempt;
`deploy/nixos-herd-runner.nix` is not markdown, so it does not match and
owes the four. The qualifier is a **closed positive statement about
markdown**: it exempts markdown and says nothing whatever about any
other extension, so every non-markdown path under `deploy/` stays
exactly where the previous letter put it, with no file's class decided
by this change.

**That closure is the reason the change is safe, because `deploy/` holds
seven kinds of non-markdown file and not the two the recommendation
named** — see *Risks and findings* 1. Had the clause named `.nix` and
`env.example` as the counter-examples, it would have implied, by
omission, that `deploy/wsl2-check.sh` and
`deploy/capabilities-wsl-pc.json` were on the exempt side. It names
neither, so it implies neither.

**The board row is updated and moved.** ORCH-357 left the *Open* section
and sits at the tail of *Owed*, state `done`, owner `herd-step`, next
action the reviewer pass and then the batch's one landing. Its *Item*
cell states what the rule now says and enumerates the seven
non-markdown kinds that keep owing the four. Its *Evidence* cell carries
the correction described in *Risks and findings* 2 and the proof of the
search described in 3.

## Not done, and why

1. **`docs/orchestration/gotchas/docs-only-gates.md:11`-`:12` is not
   edited, although it is a second live copy of the list and now
   disagrees with `CLAUDE.md`.** This is *Risks and findings* 1's
   sibling and the one real loose end of the branch. The brief said, of
   a second holder, *"report it rather than widening this one"*, and
   `docs/orchestration/gotchas/` was not among the paths it allowed. So
   it is reported, and it is **rowed** — an `ORCH-NEXT` row in *Owed*
   carrying the exact replacement text — rather than left to be
   remembered. The disagreement is conservative: the stale copy is the
   narrower list, so a reader following it over-gates rather than
   under-gates.
2. **No `docs/decisions.md` byte is written.** One entry is drafted
   below, headed `D-NEXT`; the merger allocates the number (D148,
   invariant 8). The highest decision on `main` is D295.
3. **No script, hook or workflow is touched**, so no Rust gate is owed
   by this branch. The search that establishes that nothing mechanical
   needed touching is in *Risks and findings* 3.
4. **The three copies of the list in `docs/orchestration/history/` are
   deliberately left alone** — `2026-10-26-merge-herd-own-claude-root.md`
   `:116`, `2026-10-27-merge-batch-docs-gate-orch-batched-pr.md:120` and
   `2026-11-01-release-v1.1.0.md:379`. History is append-only and not
   rewritable (`check-docs.sh:286`-`:296`), and each states correctly
   what the rule was on its own date.
5. **The batch-39 landing narrative in `board.md`'s *Next* section is
   left alone.** It says the question *"edits `CLAUDE.md` and is
   therefore Alexa's, with a recommendation in the row"*, which is a
   true statement about that landing and is not falsified by his having
   since decided.

## Gates

**This branch is prose only.** `CLAUDE.md`, `docs/orchestration/board.md`
and this file; no `.rs`, no `Cargo.toml` byte, no `Cargo.lock` byte, no
script and no workflow. `git diff --stat` against `origin/main` names
three markdown files and nothing else.

**The four Rust gates were NOT RUN**, and that is stated rather than
implied. `nix flake check`, `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings` and
`nix develop --command ./scripts/test.sh` were not run on this branch.
Nothing they read moved. **What covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s and not
this agent's.** A gate that did not run is reported as not run, never as
passed.

**`./scripts/bench-preflight.sh` was not run and did not need to be:**
nothing was submitted to Herd, no compile was started, and no job
touched the queue. It is a precondition of a heavy phase and this branch
has none.

**The run made before this file was committed**, verbatim:

```
$ ./scripts/check-orchestration.sh
check-orchestration: 361 passed, 0 failed
```

**The last run, named and not quoted** — it is made on the committed
tree, after this file, which is why its output is in the agent's report
and not in the tree it gates: `./scripts/check-docs.sh` from the
worktree root, output to the agent's report and to the terminal, no log
file (the script prints `docs ok` on success and notes to stderr on
failure). `./scripts/check-orchestration.sh` is run again with it.

**Skips: none.** This branch adds no `HERD-SKIPPED` line, no test skip
and no `#[ignore]`, and runs no suite that could skip one. The thirteen
capability skips this host cannot make are unchanged and unread by this
branch.

## Risks and findings

**1. `deploy/` holds seven kinds of non-markdown file, not the two the
brief and the board row named — and that is the finding the brief asked
to be told rather than guessed at.** The recommendation's warrant named
`deploy/*.nix` and `deploy/env.example`. `git ls-files deploy` gives
sixteen files: five markdown, and eleven that are not —
`capabilities-wsl-pc.json`, `docker-compose.traefik.yml`,
`docker-compose.yml`, `env.example`, `herd-runner.service`,
`nixos-herd-runner.nix`, `wsl2-check.sh`, and
`reverse-proxy/nginx-herd-hub.conf` and
`reverse-proxy/traefik-herd-hub.yml`.

The brief said to stop and report rather than decide their class.
**Nothing in this branch decides any of their classes**, which is why it
was written rather than stopped, and the reasoning is put here so it can
be reversed: the clause exempts `deploy/**.md` and makes no statement
about any other path, so each of those eleven files keeps the gate
obligation the previous letter gave it, unchanged and unexamined. There
was no guess to make. Had the clause instead said *"not its `.nix` or
`env.example`"* — the wording the recommendation's own bullets invite —
it would have classified two files and silently implied a class for the
other nine, which is the outcome the brief was guarding against. **If
the orchestrator reads the stop condition more strictly than that, the
branch is unlanded, unreviewed and one revert away.**

**2. The brief's and the board row's warrant for keeping `deploy/*.nix`
on the gated side is false, and the tree refutes it.** Both said
`deploy/*.nix` and `deploy/env.example` *"are read by `nix flake
check`"*, gate step 3. They are not:

- `flake.nix:27`-`:35` builds `src` with `lib.fileset.toSource` over a
  `lib.fileset.unions` of exactly `./Cargo.toml`, `./Cargo.lock`,
  `./herd-proto`, `./herd-hub`, `./herd-runner` and `./herdctl`. Its own
  comment says the fileset is *"narrowed so that editing `docs/` or this
  comment does not rebuild Rust"*. `deploy/` is not in it.
- `flake.nix` declares **no `nixosModules` output at all**; `grep -n
  deploy flake.nix` returns one hit and it is a comment about the image
  tag.
- `deploy/nixos-herd-runner.nix:74` is the file itself describing adding
  `nixosModules.herd-runner = ./deploy/nixos-herd-runner.nix` *"to
  flake.nix"* as a thing that has not been done. `:39` shows the
  consumer importing it from their own `/etc/nixos`, which is the only
  way it is ever evaluated — and that is their machine, not ours (D161).

So `nix flake check --no-build` (`scripts/gate.sh:304`-`:305`) evaluates
nothing under `deploy/`. **The conclusion is unchanged; its warrant is
replaced.** The true one is narrower and sufficient: `check-docs.sh` is
the only gate in the project that reads any file under `deploy/`, it
reads `deploy/*.md` and `deploy/release-notes/*.md` and nothing else
(`check-docs.sh:97`-`:99`, extended to `deploy/` on 2026-10-30 and its
reasoning at `:81`-`:96`), and **what no gate reads is not something an
exemption may assume on its behalf.** Exempting a file because a gate
covers it is a measured claim; exempting one because no gate would
notice is the opposite.

The false sentence is corrected in `board.md`, which is live. It also
stands at `docs/orchestration/history/2026-11-01-merge-batch-39.md:286`,
which is a dated record and stays as written.

**3. Nothing mechanical holds a copy of the exempt list, and the
detector that says so was proven on a known-bad fixture before its
silence was trusted.** `grep` on this host is ugrep 7.8.4.

The fixture: a copy of `scripts/`, `.claude/hooks/`, `.github/workflows/`,
`flake.nix` and `Cargo.toml` into a scratch directory, plus a planted
`scripts/planted-bad.sh` holding a plausible mechanical copy —
`case "$1" in CLAUDE.md|README.md|docs/*|.claude/*|.github/*.md) exit 0`.
Two detectors, both run over that tree:

- **D1**, the list's most distinctive token: `grep -rnF -e '.github/*.md'`.
  **Fires on the planted file and on nothing else in that tree.**
- **D2**, a line naming three of the five: `CLAUDE\.md` piped through
  `README\.md` piped through `docs/`. **Fires on the planted file**, and
  on two real lines that are different lists — `check-docs.sh:97`, the
  79-column prose corpus, and `check-orchestration.sh:325`, the file
  list its meta-gate copies into a scratch repo. Neither is a gate-exempt
  list and neither disagrees with `CLAUDE.md` now.

With the fixture removed, `git grep -nF -e '.github/*.md'` over the whole
tracked tree returns **five** hits and every one is prose:
`CLAUDE.md:85`, the ORCH-357 board row,
`docs/orchestration/gotchas/docs-only-gates.md:12`, and the three
history files of *Not done* 4. `git grep -nF -e 'Gates before a commit'`
returns `CLAUDE.md:84` and three history references.
`git grep -nE -e 'prose.only|docs.only|markdown.only'` over `scripts`,
`.claude`, `.github`, `Makefile` and `flake.nix` returns nothing.
`.github/workflows/ci.yml` has **no `paths` or `paths-ignore` filter**,
so CI gates every path on every push and no path list exists there to
disagree with.

**The one holder that does disagree is prose, not a script**, and it is
the `ORCH-NEXT` row and *Not done* 1:
`docs/orchestration/gotchas/docs-only-gates.md:11`-`:12`.
`.claude/agents/*.md` carry no copy — `herd-step.md:19`-`:21` says
*"Prose only"* with no directory list, and `grep -c 'README\.md'` is 0
in all five agent files — so no agent definition needs changing either.
`.claude/hooks/orchestrator-guard.sh:126` holds a path list that is
**not** this one: it is what the orchestrator may not write in the main
checkout, `deploy/` has been on it all along, and it is unaffected.

**4. `CLAUDE.md` is now at 150 lines against the 150-line limit of
`check-docs.sh:34`-`:37`, with two lines at exactly 79 columns.** It was
at 150 before this change too, which is why the clause is four words:
the budget for the whole paragraph is about 380 columns over five lines,
it held 345, and the list entry plus qualifier spend 36 of the 35-to-40
available. **A wording that named a counter-example cost two more lines
and did not fit.** Three consequences worth stating:

- The next clause anyone adds to `CLAUDE.md` must free a line first.
  This is a real constraint on the method, not a formatting note.
- Raising `limit=150` to fit this edit was considered and **refused**:
  the guard exists so `CLAUDE.md` stays readable before every session,
  and editing a guard to make one's own change fit is the pattern the
  brief forbids. It would also have flipped this branch into owing all
  four Rust gates.
- It is the strongest argument for the drafted decision entry. The
  boundary rides on eighteen characters in a file under a size cap, and
  a future compression of that paragraph is the likeliest way the
  exemption silently widens to all of `deploy/`.

**5. One small thing the tree refutes about the brief, for completeness:
there is no `deploy/hub.nix`.** The brief used it as the file a reader
should be able to place. The `.nix` under `deploy/` is
`deploy/nixos-herd-runner.nix`, and that is the name used throughout
this handoff and the board row.

**6. Not a finding of this branch, but seen while verifying gate step 3,
and reported rather than acted on:** `CLAUDE.md`'s *Commands* fence
lists three `nix develop` commands and `check-docs.sh`, while the prose
beside it says *"all four Rust gates"*. The four, from
`scripts/gate.sh:301`-`:308`, are `nix flake check` (step 3),
`cargo fmt` (4), `cargo clippy` (5) and the test suite (6). **`nix flake
check` is the one the fence does not show.** Nothing is false — the
prose says four and there are four — but the block a reader copies from
has three of them. It is not fixed here: it costs a line `CLAUDE.md` does
not have (see 4), and it is outside what this brief allowed.

## Drafted for `herd-merger`

**One `docs/decisions.md` entry.** The ruling, and the reasoning the
brief asked for, first.

**It earns an entry.** The brief's precedent cuts both ways — ORCH-070
went into `decisions.md` because prose was its only holder, and the
board-row indent guard did not because the guard and its comment were
the record. The question here is whether a second prose record adds
anything when the holder is already `CLAUDE.md`, which every agent reads
at start-up. It does, for three reasons, and the third is the one that
decides it:

1. **`CLAUDE.md` holds the rule and cannot hold the reason.** The
   boundary is the load-bearing half, and the only reason anyone will
   accept it over *"add `deploy/`"* is the argument of *Risks and
   findings* 2 — which did not survive its first telling intact, since
   the warrant in the brief and the board row was false. Eighteen
   characters cannot carry that. The 150-line cap means they never will.
2. **Nothing mechanical holds it**, proven in 3. The exemption's only
   guards are two prose sentences, one of which is already stale.
3. **`CLAUDE.md` is rewritten; `decisions.md` is appended to.** The
   paragraph sits at both of its guards' ceilings, so the next person to
   add a clause there is under direct pressure to compress — and
   `(its markdown only)` is the cheapest thing in that paragraph to
   drop. Dropping it widens the exemption from `deploy/`'s markdown to
   all of `deploy/`, which is the coverage loss Alexa's boundary exists
   to prevent, and no gate would say a word. An append-only entry is the
   only holder in this project that survives a rewrite of the holder.

The entry, verbatim, to be appended with `D-NEXT` replaced by the next
number. The header is 73 columns, measured unfenced, so it does not trip
the 79-column rule when it lands in `decisions.md`.

```markdown
## 2026-11-01 — D-NEXT The gate rule exempts `deploy/**.md`, markdown only

`CLAUDE.md`'s *Commands* paragraph exempted by directory —
`CLAUDE.md`, `README.md`, `docs/`, `.claude/`, `.github/*.md` — and
`deploy/` was not on the list. By its letter a markdown-only change
under `deploy/` owed all four Rust gates, although no Rust tool reads
one. `deploy/release-notes/<version>.md` is prose that ships in the
tree (ORCH-162), and v1.1.0's cut hit the case exactly: it ran
`check-docs.sh` alone and reported the four as **not run, never as
passed**, which was the right answer under the letter as it stood
(`history/2026-11-01-release-v1.1.0.md`).

**The rule now exempts `deploy/**.md` and nothing else under
`deploy/`** (Alexa, 2026-11-01, ORCH-357). The list gains one entry
and the words `(its markdown only)` beside it.

**The boundary is the decision, not the exemption.** Exempting
`deploy/` wholesale would have been a loss of coverage dressed as the
correction of a letter. `deploy/` holds sixteen files: five markdown,
and eleven that are not — a `.nix`, a `.service`, a `.json`, two
`docker-compose` YAMLs, a `.sh`, `env.example` and two files under
`reverse-proxy/`. The clause is a **closed positive statement about
markdown**: it exempts markdown and makes no statement about any other
extension, so all eleven keep the obligation the previous letter gave
them, and no file's class is decided by the change.

**The warrant, corrected.** ORCH-357's row recommended the narrow
exemption on the ground that `deploy/*.nix` and `deploy/env.example`
*"are read by `nix flake check`"*. **They are not.** `flake.nix`
narrows `src` with `lib.fileset.unions` to `Cargo.toml`, `Cargo.lock`
and the four crates; it declares no `nixosModules` output; and
`deploy/nixos-herd-runner.nix` is the consumer's to import from their
own `/etc/nixos` (D161). Gate step 3 evaluates nothing under
`deploy/`. The true warrant is narrower and enough: `check-docs.sh` is
the **only** gate that reads any file under `deploy/`, and it reads
`deploy/*.md` and `deploy/release-notes/*.md` alone. **What no gate
reads is not something an exemption may assume on its behalf** —
exempting a file because a gate covers it is a measured claim;
exempting one because no gate would notice is the opposite, and it is
the whole reason the boundary stops at `.md`.

**Why this is written down when `CLAUDE.md` already says it.** The
rule's holder is prose every agent reads, so the rule needs no second
record; the **boundary** does. `CLAUDE.md` is capped at 150 lines by
`check-docs.sh` and is at 150; the gates paragraph fills five lines
with two of them at exactly 79 columns. There is no room there for the
paragraph above, and the next clause anyone adds puts the paragraph
under compression — where `(its markdown only)` is the cheapest thing
to drop, and dropping it widens the exemption to all of `deploy/` with
no gate to say so. Nothing mechanical holds the list: it was searched
for in `scripts/`, `.claude/hooks/`, `.github/workflows/`, `flake.nix`
and `Cargo.toml` with a detector first proven on a planted copy, and
the list lives in prose alone. `CLAUDE.md` is rewritten; this file is
appended to. That is why the boundary is recorded here.

D148, D161, D279, ORCH-162, ORCH-357.
```

**Two `board.md` rows, both already in the tree on this branch**, so
neither needs pasting:

1. `board.md:211`, tail of *Owed*, `| ORCH-357 | … | herd-step | done |`
   — this task. It keeps its allocated number; nothing to allocate.
2. `board.md:212`, tail of *Owed*, `| ORCH-NEXT | … | herd-step | owed |`
   — the stale second copy of the list in
   `docs/orchestration/gotchas/docs-only-gates.md:11`-`:12`. **One
   number to allocate.** Its *Next action* cell carries the exact
   replacement text, so whoever picks it up decides nothing.

Nothing else is drafted: no change to `scripts/`, no `D-NEXT` reference
left anywhere in the tree to renumber, and no `cut-v*` or tag.

## Decisions needed

**None blocking.** Two things are decided in this handoff rather than
asked, both reversible on an unlanded branch, and both are named so they
can be reversed:

1. **The decision entry is drafted** rather than skipped, on the
   orchestrator's explicit ruling that this agent decide and justify it.
   The justification is above.
2. **The branch was written rather than stopped** on finding file kinds
   under `deploy/` that the brief's two bullets did not cover, because
   the clause as written classifies none of them. *Risks and findings* 1
   states the reasoning and what reversing it would cost: nothing — the
   branch is unlanded and the reviewer pass comes next.

## Resume point

`herd-reviewer` takes the branch. The three things worth an adversarial
pass, in order: whether `(its markdown only)` really lets a reader place
`deploy/capabilities-wsl-pc.json` without leaving the paragraph; whether
*Risks and findings* 1's reading of the brief's stop condition holds;
and whether the drafted entry's third argument — compression pressure on
a file at its line cap — is a real mechanism or a story. Then
`herd-merger` integrates the branch in the batch's one landing,
allocating one number for the `ORCH-NEXT` row at `board.md:212` and one
for the drafted decision, and runs `./scripts/gate.sh` on the gated sha,
which is what covers the four Rust gates this branch did not run.

## Processes

**None.** No process was started, no job was submitted to Herd, no
compile was run and no PID was killed. `CARGO_TARGET_DIR` was never set
and **no `target/` was created**, because this branch touches no code —
so there is none to delete (`workflow.md`, *Concurrency and host rules*).
The scratch fixture of *Risks and findings* 3 is a copy under this
agent's own scratchpad subdirectory, outside the worktree and outside
the repository.
