# orch-357-deploy-md-gate-exemption — handoff 2026-11-01

- **Item:** ORCH-357, plus three `ORCH-NEXT` rows this task files in
  *Owed* for what the work and the reviewer's block turned up
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

**It is prose where its five neighbours are a file, a file, two
directories and a glob, and that is the decision of this branch rather
than an accident of fitting.** The first spelling committed here was
`deploy/**.md`, which **does not match `deploy/release-notes/1.1.0.md` —
the file whose release produced ORCH-357.** `herd-reviewer` blocked on
it and was right; the measurement is *Risks and findings* 1.

**Why prose and not the corrected glob.** Both wordings fit in five
lines and both were measured (*Risks and findings* 2). The corrected
glob `` `deploy/**/*.md` (markdown only) `` was available at 74, 77, 79,
74, 75. It was not chosen, for one reason: **a glob in this document has
no engine.** Nothing mechanical reads this list — that is established in
4 — so a glob here is evaluated only by a reader's belief about a glob
dialect, which is exactly how `deploy/**.md` crossed the v1.1.0 handoff,
the board row's recommendation, Alexa's validation and one commit of
this branch without anyone expanding it. `deploy/**/*.md` would replace
it with a second construct whose **zero-directory case** a reader must
know, and that case is live and populated here: `deploy/README.md` and
`deploy/release-notice.md` are 2 of the 7 markdown files under
`deploy/`. It does match them, verified in both engines — but the next
reader must take that on faith or test it, and the failure mode is the
mirror of the one being fixed, a reader concluding `deploy/README.md`
owes four gates. *"Only the markdown under `deploy/`"* has no expansion
to get wrong at any depth. The list is already mixed notation —
`docs/` and `.claude/` are bare directory prefixes, not globs — so a
worded entry is not out of place in it.

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
`pre-push` refuses a push without that receipt, and `.github/workflows/`
`ci.yml` has no `paths` filter. **Nothing reaches `main` ungated by this
exemption**, and an earlier draft of this handoff implied otherwise.

**The board.** ORCH-357 left *Open* for the tail of *Owed*, state
`done`, owner `herd-step`. Three `ORCH-NEXT` rows sit under it, listed
in *Drafted for `herd-merger`*.

## Not done, and why

1. **`docs/orchestration/gotchas/docs-only-gates.md` is not edited**,
   although it is a second live copy of the rule and is now stale in two
   places. The brief said, of a second holder, *"report it rather than
   widening this one"*, and the gotchas were not among the paths it
   allowed. It is reported and **rowed**, with the final spelling in the
   row so the next agent cannot land a third one.
2. **`scripts/check-docs.sh` is not touched**, deliberately and on the
   orchestrator's instruction. Two things that were floated and refused:
   narrowing `CLAUDE.md`'s letter to the script's two `deploy/` globs
   does not fit in five lines, and it would couple the method's letter
   to one script's internal corpus, so a later widening of the script
   would silently change what `CLAUDE.md` says. The corpus question is
   its own row.
3. **No `docs/decisions.md` byte is written.** One entry is drafted
   below, headed `D-NEXT`; the merger allocates the number (D148,
   invariant 8). The highest decision on `main` is D295.
4. **The three copies of the list in `docs/orchestration/history/` are
   left alone** — `2026-10-26-merge-herd-own-claude-root.md:116`,
   `2026-10-27-merge-batch-docs-gate-orch-batched-pr.md:120` and
   `2026-11-01-release-v1.1.0.md:379`. History is append-only and not
   rewritable (`check-docs.sh:286`-`:296`), and each states correctly
   what the rule was on its own date. The false `nix flake check`
   warrant of 5 also stands at
   `2026-11-01-merge-batch-39.md:286` for the same reason.
5. **The batch-39 landing narrative in `board.md`'s *Next* section is
   left alone.** It says the question *"edits `CLAUDE.md` and is
   therefore Alexa's, with a recommendation in the row"*, true of that
   landing and not falsified by his having since decided.

## Gates

**This branch is prose only.** `CLAUDE.md`,
`docs/orchestration/board.md` and this file. No `.rs`, no `Cargo.toml`
byte, no `Cargo.lock` byte, no script, no workflow — the correction
after the reviewer's block changed nothing about that, and the gate
class is the same as it was on the first commit.

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

**Runs made before this file was committed**, verbatim. The first pair
is from the first commit, the second from the corrected tree:

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
The lesson is in the drafted entry, because it is the reason the final
clause carries no glob at all.

**2. The two wordings, measured.** Reproduced with a greedy wrapper
using `check-docs.sh`'s own column count (codepoints, not bytes):

| wording | lines | columns |
|---|---|---|
| `deploy/**/*.md` (markdown only) | **5** | 74, 77, 79, 74, 75 |
| only the markdown under `deploy/` | **5** | 74, 78, 79, 74, 75 |
| `deploy/**/*.md` (its markdown only) | 6 | — |
| under `deploy/`, its markdown alone | 6 | — |

Both five-line forms fit. The reviewer's measurement of the first is
reproduced exactly, and so is the finding that keeping the parenthetical
*and* the corrected glob needs six lines. Greedy wrapping is
line-count-optimal, so there is no five-line arrangement of the six-line
rows. The choice between the two five-line forms is argued in *Done*.

**3. `deploy/` holds 7 markdown files and 9 others, not 5 and 11.** The
first version of this handoff, its board row and its drafted entry all
said *"five markdown, and eleven that are not"* while naming nine files
by name — two wrong figures and an internal contradiction.
`git ls-files deploy | grep -c '\.md$'` is **7**, the complement is
**9**, the total is 16. The 5 was propagated from
`check-docs.sh:88`-`:89`, which says *"all five markdown files under
`deploy/`"* and was true when it was written on 2026-10-30, before
`release-notes/1.0.0.md` and `1.1.0.md` existed. **A figure in a comment
is dated even when the comment does not say so**, and this one was
copied into three places without being recounted.

**4. Nothing mechanical holds the exempt list *today*, and that claim
needs the qualifier it did not have.** `check-docs.sh:168`-`:170`
already asserts literal strings in `CLAUDE.md` with `grep -qF` — it
loops over `docs/orchestration/board.md`,
`docs/orchestration/workflow.md` and `.claude/agents` and notes when one
is no longer named — and `check-orchestration.sh:143`-`:144` proves that
exact class red by mutation. **So the shape that could hold this list
exists, a few lines from where `CLAUDE.md` is already checked for width
and length**, and *"nothing mechanical holds it"* is a statement about
today and not about what is possible. It is rowed. The unqualified
version was this agent's and `herd-reviewer` refuted it.

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

**5. The seven-hit count, corrected.** On the committed tree, excluding
this file, `git grep -nF -e '.github/*.md'` returns **7 lines in 6
files**, not the five the first version of this handoff claimed while
enumerating six. The figure is load-bearing for 4, so here it is in
full: `CLAUDE.md:85`; `board.md:211` and `board.md:212`, the ORCH-357
row and the gotcha row, which quote the list rather than hold it;
`gotchas/docs-only-gates.md:12`, which **does** hold it and is stale;
and the three dated history files of *Not done* 4. Every hit is prose.
`git grep -nF -e 'Gates before a commit'` returns `CLAUDE.md:84` and
three history references. `git grep -nE -e 'prose.only|docs.only|`
`markdown.only'` over `scripts`, `.claude`, `.github`, `Makefile` and
`flake.nix` returns nothing. All five `.claude/agents/*.md` have
`grep -c 'README\.md'` of 0; `herd-step.md:19`-`:21` says *"Prose
only"* with no directory list.
`.claude/hooks/orchestrator-guard.sh:126` holds a path list that is not
this one — what the orchestrator may not write in the main checkout,
with `deploy/` on it already — and is unaffected.

**6. The warrant, corrected twice.** The brief's and the board row's
reason for keeping `deploy/*.nix` on the gated side was that it and
`deploy/env.example` *"are read by `nix flake check`"*, gate step 3.
**They are not:** `flake.nix:27`-`:35` builds `src` with
`lib.fileset.toSource` over a `lib.fileset.unions` of exactly
`./Cargo.toml`, `./Cargo.lock`, `./herd-proto`, `./herd-hub`,
`./herd-runner` and `./herdctl`, its own comment saying it is
*"narrowed so that editing `docs/` or this comment does not rebuild
Rust"*; `flake.nix` declares **no `checks` and no `nixosModules`
output**; and `deploy/nixos-herd-runner.nix:74` is the file itself
describing adding `nixosModules.herd-runner = ./deploy/`
`nixos-herd-runner.nix` *"to flake.nix"* as a thing not done, while
`:39` shows the consumer importing it from their own `/etc/nixos`,
which is their machine and not ours (D161).

**Then the replacement warrant was itself a true claim carrying a false
qualifier**, and `herd-reviewer` caught that too. It said
`check-docs.sh` is *"the only gate that reads any file under
`deploy/`"*. **Two prose gates read that markdown:**
`check-docs.sh:97`-`:99` directly, and `check-orchestration.sh`, whose
`fresh()` at `:99`-`:103` copies the real `deploy/release-notes` into
every one of its corpora — 49 copies, 48 of which go on to run
`check-docs.sh` over them. Narrowed to what is true and sufficient:
**no Rust gate reads any file under `deploy/`, and two prose gates read
its markdown.** That is the warrant in the clause, the row and the
drafted entry.

**7. Not a finding of this branch, reported and not acted on:**
`CLAUDE.md`'s *Commands* fence lists three `nix develop` commands and
`check-docs.sh`, while the prose beside it says *"all four Rust
gates"*. The four, from `scripts/gate.sh:301`-`:308`, are `nix flake
check` (step 3), `cargo fmt` (4), `cargo clippy` (5) and the test suite
(6). **`nix flake check` is the one the fence does not show.** Nothing
is false, but the block a reader copies has three of the four. Not
fixed: it costs a line `CLAUDE.md` does not have, and it is outside what
this brief allowed. Not rowed either, pending the orchestrator's word.

**8. There is no `deploy/hub.nix`.** The brief used it as the file a
reader should be able to place; the `.nix` under `deploy/` is
`deploy/nixos-herd-runner.nix`.

## Drafted for `herd-merger`

**One `docs/decisions.md` entry.** The ruling and its reasoning first,
since the orchestrator asked this agent to decide and justify it.

**It earns an entry.** The precedent cuts both ways — ORCH-070 went into
`decisions.md` because prose was its only holder, and the board-row
indent guard did not because the guard and its comment were the record.
The question is whether a second prose record adds anything when the
holder is already `CLAUDE.md`, which every agent reads at start-up. It
does, for three reasons:

1. **`CLAUDE.md` holds the rule and cannot hold the warrant.** The
   boundary is the load-bearing half, and the only reason anyone accepts
   it over *"add `deploy/`"* is 6 above — a warrant that was **false in
   three documents and then over-qualified in a fourth** before it came
   out right. Six words of clause cannot carry that, and the 150-line
   cap means they never will.
2. **Nothing mechanical holds it today, and something could.**
   `check-docs.sh:168`-`:170` is the shape, proven red as a class by
   `check-orchestration.sh:143`-`:144`. Until that row is built, the
   boundary's only guards are prose sentences — one of which, the
   gotcha, is already stale. This is the qualified form of the claim; 4
   has the unqualified one and why it was wrong.
3. **The notation is the residual risk, and it is not guarded.** The
   clause is deliberately prose where its neighbours are globs, and a
   later editor will be tempted to regularise it — *"only the markdown
   under `deploy/`"* back into `` `deploy/**.md` ``. That is not a
   hypothesis: it is what this branch did, with Alexa's validation
   behind it, and the result silently excluded the release notes while
   reading as though it included them. **An append-only record is what
   makes that tidy-up recognisable as a regression rather than a
   cleanup.** This replaces an argument in the first draft of this
   handoff which claimed the boundary rode on a droppable parenthetical;
   `herd-reviewer` refuted it — with the glob still in the clause the
   `.md` carried the boundary, and widening needed a visible semantic
   edit — and it is struck rather than reworded, because a false
   argument cannot be corrected once appended (D148).

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
file under `deploy/`, and two prose gates read its markdown** —
`check-docs.sh` directly, and `check-orchestration.sh`, whose `fresh()`
copies the real `deploy/release-notes` into each of its corpora and
runs `check-docs.sh` over 48 of them. **What no Rust gate reads is not
something an exemption may assume on its behalf**: exempting a file
because a gate covers it is a measured claim, exempting one because no
gate would notice is the opposite, and that is why the boundary stops
at markdown.

**It is written in words, not a glob, and that is part of the
decision.** The first spelling was `` `deploy/**.md` ``, and it was
carried from the v1.1.0 handoff into the board's recommendation, into
this validation, and into one commit. **It does not match
`deploy/release-notes/1.1.0.md`** — the file whose release produced
the row. `**` is recursive only as a whole path component, so
`deploy/**.md` degrades to `deploy/*.md`; `git ls-files` with
`:(glob)` and `bash` with `globstar` both return `deploy/README.md`
and `deploy/release-notice.md` alone. **A glob in a method document
has no engine.** Nothing mechanical reads this list, so a glob here is
evaluated only by a reader's belief about a dialect — which is how a
wrong one crossed four documents unexpanded. `deploy/**/*.md` is
correct and was measured as fitting, and was still not chosen: its
zero-directory case is live, two of the seven markdown files sit
directly under `deploy/`, and a reader must know that `**/` matches no
directory at all before they can place them. Words have no expansion
to get wrong.

**Why this is recorded when `CLAUDE.md` already says it.** The rule's
holder is prose every agent reads, so the rule needs no second record;
the **warrant** and the **notation** do. `check-docs.sh` caps
`CLAUDE.md` at 150 lines and it is at 150; the gates paragraph fills
five lines, one of them at exactly 79 columns. The paragraph above
cannot live there. Nothing mechanical holds the list **today** —
searched across `scripts/`, `.claude/hooks/`, `.github/workflows/`,
`flake.nix` and `Cargo.toml` with a detector first proven on a planted
copy — though `check-docs.sh:168` already asserts literal `CLAUDE.md`
strings with `grep -qF` and is the shape that could, which is rowed
separately. So the boundary's only guards are prose, and the standing
risk is a later editor regularising the words back into a glob and
reintroducing exactly the defect this entry closes. `CLAUDE.md` is
rewritten; this file is appended to. That is why it is recorded here.

D27, D148, D161, D279, ORCH-162, ORCH-357.
```

**Four `board.md` rows, all in the tree on this branch**, so none needs
pasting. One keeps its number; **three need one each, in document
order**:

1. `board.md:211`, *Owed*, `| ORCH-357 | … | herd-step | done |` — this
   task. Already numbered.
2. `board.md:212`, *Owed*, `| ORCH-NEXT | … | herd-step | owed |` — the
   two stalenesses in `gotchas/docs-only-gates.md`: the five-entry list,
   and a *Verify* step that asks for a green `check` job on a pull
   request nobody opens. Its *Next action* carries the **final**
   spelling to copy, so a third one cannot be landed by accident.
3. `board.md:213`, *Owed*, `| ORCH-NEXT | … | herd-planner | owed |` —
   `check-docs.sh:168` is the shape that could hold the exempt list
   mechanically. **Edits a gate script, so it owes all four Rust gates
   and a reviewer pass of its own; not a condition of this landing.**
4. `board.md:214`, *Owed*, `| ORCH-NEXT | … | herd-planner | owed |` —
   `check-docs.sh`'s stated reason for excluding `deploy/*/*.md` does
   not survive, a better unstated one does, and the corpus now reads
   less of `deploy/`'s markdown than `CLAUDE.md` exempts. **Edits a gate
   script.**

Nothing else is drafted: no `D-NEXT` reference left anywhere in the tree
to renumber, no `cut-v*`, no tag.

## Decisions needed

**None blocking.** Two calls are made here rather than asked, both
reversible on an unlanded branch:

1. **Prose over the corrected glob** in `CLAUDE.md`, argued in *Done*
   against the alternative the orchestrator offered. If the reviewer
   prefers `` `deploy/**/*.md` (markdown only) ``, it fits at five lines
   and the swap is one line of the paragraph.
2. **The decision entry is drafted** rather than skipped, on the
   orchestrator's explicit ruling that this agent decide and justify it.

## Resume point

`herd-reviewer` sees the branch a second time. The three things worth
the pass, in order: whether *"only the markdown under `deploy/`"* is the
right call against the corrected glob, and whether its reasoning holds;
whether the drafted entry's three arguments are now all true, argument 3
especially, since it replaces one that was refuted and `decisions.md`
cannot be corrected after the fact; and whether *Risks and findings* 5's
recount is right this time. Then `herd-merger` integrates the branch in
the batch's one landing, allocating **three** `ORCH-NEXT` numbers in
document order and **one** decision number, and runs `./scripts/gate.sh`
on the gated sha, which is what covers the four Rust gates this branch
did not run.

## Processes

**None.** No process started, no job submitted to Herd, no compile run,
no PID killed. `CARGO_TARGET_DIR` was never set and **no `target/` was
created**, because this branch touches no code — so there is none to
delete (`workflow.md`, *Concurrency and host rules*). The scratch
fixture of *Risks and findings* 4 was a copy under this agent's own
scratchpad subdirectory, outside the worktree and the repository, and is
removed.
