# board-row-indent-guard — handoff 2026-11-01

- **Item:** ORCH-NEXT (three rows in `board.md` read `ORCH-NEXT` and need
  three numbers: one `done` in *Owed*, two `open` in *Open*)
- **Agent:** herd-step
- **Branch / commit:** `board-row-indent-guard` / see the agent's report
  for the sha, based on origin/main `d6c4048`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A board row must now be written in exactly one shape, and
`check-docs.sh` requires that shape rather than forbidding a bad one.**

The three row-level checks over `board.md` — the ten-field count, the
known-state check, the duplicate-id check — all anchor on a
start-of-line pipe followed by a space and `ORCH-`. Their blind set is
therefore **every line that renders as a row of that table and does not
begin with exactly those characters**. An indented row is one member of
it. Measured on this branch, each detector proven on a column-1
known-bad first by breaking a state cell to `banana`:

| appended row | before | after |
|---|---|---|
| canonical, `banana` state | red (state check) | red |
| one leading space | red (first guard) | red |
| a tab | red (first guard) | red |
| **no space after the pipe** | **green, `docs ok`** | **red** |
| **two spaces after the pipe** | **green, `docs ok`** | **red** |
| indented *and* no space | **green, `docs ok`** | **red** |
| lowercase `orch-` | **green, `docs ok`** | **red** |
| NBSP / ZWSP / BOM indent | green | green (named limit) |

One such row was on `main`: `board.md:207`, ORCH-341, against 317 at
column 1. It was substantively correct — ten fields, state `done`,
confirmed now that the checks can see it — so nothing was broken; the
hole was latent. It is de-indented in the same commit that makes the
shape mandatory, so `main` cannot go red on the first run.

### The first fix was wrong in kind, and this is the correction

My first version forbade the indented spelling alone, matching
`^[[:space:]]+\| ORCH-`. It left a leading pipe with **no** space after
it, and one with **two**, green with `banana` in the state cell — while
the comment above it asserted the shape was machine-held. Neither needs
an unusual character; both render in GFM as a row of the same table; a
pipe with no space after it is the most ordinary typo available over a
pipe table. That is precisely the class ORCH-335 names: *a control that
can go green on the defect it was built for ends the vigilance that was
holding the rule.* It replaced one latent hole with a narrower latent
hole plus a strong written claim that there was none, which is worse
than the hole it closed.

`herd-reviewer` found it by mutation and blocked the branch for it. I
reproduced it independently from the patterns before changing anything.
The guard now matches any line whose first cell is an ORCH id in any
spelling, case-insensitively, and requires it to be exactly canonical —
pipe, one space, `ORCH-<n>`, one space, pipe. `ORCH-NEXT` is canonical
too, because it is the placeholder an agent writes and the merger
numbers; a case asserts the guard does not refuse it.

### The choice, and the alternative not taken

Requiring the canonical spelling, rather than anchoring the three checks
at an optional-whitespace prefix and accepting several, is deliberate.
Accepting several makes a row several spellings of one thing, and every
check written over this table afterwards must carry the tolerance for
ever or quietly lose a row again. The indentation was a typo, not a
style — 1 against 317 — and `board.md` is a live document, not a dated
record, so de-indenting it was an ordinary edit.

**The twelve-line comment above the guard is the project's only record of
that rejected alternative.** No `docs/decisions.md` entry was written,
deliberately: the alternative belongs where whoever trips over the rule
is standing, not 9 000 lines into an append-only file. The comment says
so, because a later trim of it for brevity would lose the record — the
same device the file already uses for its cost note.

### The fixtures, and that they are live

Ten cases in `check-orchestration.sh`, beside the four column-1 cases of
the same family, which pass unchanged: one leading space, a tab, no
space after the pipe, two spaces, four red-**and-silent** cases, and two
green controls (a canonical row, and `ORCH-NEXT`).

**Proven live by mutation, not by being green.** With the guard deleted
and nothing else changed, exactly the **eight** red cases fail and the
two green controls still pass. The same mutation reopens the hole on the
real tree: a no-space row with `banana` gets `docs ok`, `rc=0`.

The four red-and-silent cases use a new
`expect_red_quiet <name> <must print> <must not print>` helper, because
`expect_red` cannot make that assertion — a message naming any defect
would satisfy it. The helper is itself proven to fail in both directions
(a case forbidding the message it does get; a case demanding one nothing
prints): `pass=1 fail=2`, the real case ok.

**One caveat recorded in the script**, so nobody reads a case as proving
more than it does: the duplicate-id case's silence is **also** explained
by `grep -o` capturing the prefix into the compared key, so a leading
space makes two different strings and `uniq -d` finds nothing. That case
therefore stays green even if the id check is re-anchored, and whoever
re-anchors it must not rely on this case to tell them they got it wrong.
`herd-reviewer` measured that; the positive half is live either way.

### A figure both scripts document, corrected twice

Both scripts asserted in nine places that `check-docs.sh` is run **38**
times over a full corpus copy. Ten new cases make it **48**. Counted
with a runtime shim, never inferred: 51 invocations, 3 of them in the
parallel-decisions section's small synthetic repo, so 48 whole-corpus,
and 49 `fresh()` copies counted at the `cp` itself.

The count alone was the wrong figure to leave behind. It now carries a
dated measured cost, per ORCH-335's labelled-field rule:

    48 whole-corpus runs; check-orchestration.sh 63 s on this host
    2026-11-01; ~120 s in CI at the 1.9x host-to-CI factor measured
    2026-10-27 (history/2026-10-27-docs-gate-cost.md); against the
    `docs` job's 300 s cap -- about 40% of it.

**My earlier "about a fifth of its budget" was a true ratio of the wrong
two numbers**: 63 s is this host, and the cap is a CI cap. For scale,
the same history file measured 25 cases at 9.63 s here and ~20 s in CI
on 2026-10-27: cases ×1.9 and wall ×6.5 in five days. The cost is cases
× corpus, so it grows faster than either, and `docs/` is now the bigger
half. 48 is safe today; the cap is the thing to watch, and that figure
is the one to re-measure before adding a whole-corpus rule.

### The `fresh()` comment's own off-by-one

One of the nine mentions names *copies*, not runs, and is one higher: 39
on the base, **49** now. It said 38, so it was off by one before this
branch as well. It now says 49 copies of which 48 run the script, with
the distinction spelled out rather than left as an apparent
inconsistency with the other eight.

## Not done, and why

- **`docs/decisions.md` untouched**, and no entry drafted. Append-only,
  the merger's alone, and the orchestrator decided against an entry: the
  rejected alternative is recorded in the script instead. Stated as a
  decision taken, not an omission.
- **The seven sibling holes are not fixed** — they are now **rowed**
  (two `open` rows), which is what makes that word true; see *Risks*.
- **`scripts/gate.sh` and `scripts/test.sh` untouched**: `wc -l` against
  the base confirms 733 and 302 unchanged.
- **Nothing done about the load-induced red** (see *Risks*): the
  reporting half is the orchestrator's own branch, and the per-test half
  was refused on the evidence. My premise about it was wrong.
- **NBSP, ZWSP and BOM indents are a named limit, not a guard.** All
  three measured green; none exists in the corpus. Refusing them means
  deciding what characters a document may contain, a larger rule than
  this one.

## Gates

`scripts/` is not prose-exempt, so all four Rust gates plus
`check-docs.sh`. No `.rs` file is touched by this branch, so the Rust
gates are a formality here — run in full regardless and reported as run.

`git status --porcelain` clean but for the four intended files.
`cargo fmt --all --check`: exit 0, no output but the flake's dirty-tree
note. `cargo clippy --workspace --all-targets -- -D warnings`: exit 0,
with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/board-row-indent-guard`,
the dir the gate reuses; the only `^warning` line in the log is that
same dirty-tree note, no lint fired.

`./scripts/check-docs.sh` on the final tree: `docs ok`, exit 0. The
shape matrix under *Done* is the detector's own evidence, red before and
after on each shape.

`./scripts/check-orchestration.sh`:

    base d6c4048:  351 passed, 0 failed   (53.1 s)
    this branch:   361 passed, 0 failed   (62.6 s)

351 + 10 = 361. The mutation run, guard deleted, fails exactly the eight
red cases and keeps the two green controls.

**The last run, named and not quoted**, on the committed tree including
this file: the whole gate, bare, through the queue, with
`CARGO_TARGET_DIR` off the root filesystem. Its receipt and verbatim
output are in the agent's report, because a handoff cannot carry the
output of the run that gates it.

**Skips:** no skip from either docs gate — neither printed a
`HERD-SKIPPED` line nor any skipped case, on the base or here. The
suite's own skip list belongs to the final gate run and is in the
agent's report with both sha256 figures; this branch touches no
`#[ignore]` and no test, so neither should move.

### The first gate run on this branch's predecessor commit was RED

Recorded here as labelled fields, because the merger reads the tree and
nothing else, and because `gate.sh` writes **no receipt on RED** — the
only machine trace was a log inside a worktree the shared-host rule says
to delete. `templates/landing.md` forbids only what exists *after* the
gating commit, and a run that finished *before* it is in none of those
rows, so this belongs here at no cost.

    gate: RED sha=66b578f4afa49d089dc7e81c4a0440d7ab8a103d
      failed=6:test-suite
      skips=under-load(flood-fifty=93.07>8,flood-two-fifty=93.07>8)
    suite: exit 101 after 442 s; 9 failures -- 5 in `admission`,
      4 in `announced_job_ceiling`, every one a startup or timing wait
    gate.sh read load1 1.45 at step 0 and 10.36 after the suite
    harness peak, printed by the harness itself:
      host load average 131.49 / 62.43 / 25.57 (1, 5, 15 min), 12 cores
    cause: a root `borgbackup` run, observed via /proc for 735 s,
      never signalled
    resolution: one re-submission through the queue, GREEN,
      load1-start 3.74, load1-end 3.61
    no `.rs` touched, no test edited, no expectation edited,
      no local re-run for a greener answer

That sha is this branch's **previous** commit; the current tree differs
and carries its own gate run, named above.

## Risks and findings

**Seven sibling holes, measured, and now rowed.** Two `open` rows in
`board.md` carry them, which is what makes the word "rowed" true — my
previous handoff said "rowed" while the holes lived only in the notes
cell of a `done` row, and invariant 8 makes `board.md` the one list of
**open** work. A true item no scan can reach is the same shape as the
defect this branch closes. `herd-reviewer` caught that; the rows are the
fix.

- **Six defeated by one leading space.** Five over `docs/decisions.md`,
  all anchored on a column-1 `## `: `:126` malformed header, `:129` the
  `D-NEXT` placeholder left behind, `:132` duplicate number, `:141`
  highest number, `:143` the gap list. One over `docs/coverage.md:107`,
  the orphan-must rule. Markdown renders a header indented up to three
  spaces as a header, so an indented decision header reads normally.
  Today: `decisions.md` 289 column-1 headers, **0** indented;
  `coverage.md` **44** column-1 `must` rows, **0** indented. (My earlier
  "55" was the count of all `E-` rows; the check reads only the `must`
  ones, so the denominator belonged to a different set.)
- **A seventh, different in kind:** `check-docs.sh:143`'s gap list is a
  bare `echo "note: …"`, not the `note()` function, so it never sets
  `fail`. A gap two past the highest number prints a `note:` line and
  the script still exits 0 — it reads like a finding in CI output and
  holds nothing. Found by `herd-reviewer`.

On the `D-NEXT` one I accept the reviewer's correction and record it
against my own reasoning: it is the **least** urgent of the five, not
the most. `decisions.md` is written by one actor at one moment, so an
indented placeholder is a two-fault scenario, while the single fault that
has actually happened — a placeholder forgotten — is already caught at
column 1.

**What the same space does *not* defeat**, reported either way: the
board's section-order check (`:172`) fails safe, the landing-record awk
(`:255`) fails safe by construction, and the agent frontmatter checks
(`:159`-`:160`) fail safe. `docs/plan.md` has no row- or line-level
check at all beyond the width glob. All confirmed independently by the
reviewer, who nearly reported the first of them wrong when an edit
silently failed to apply — which is why each is stated with its message.

**A board row can no longer be shown as a literal in `board.md`.** At
column 1 the state check already read it before this guard existed; at
any other spelling the guard reads it. The escape is a backslashed
leading pipe, and the comment says so. The asymmetry is deliberate but
real: indenting *used* to be the one way to show a literal row, and now
there is none. The place this rule is documented is the script.

**Two opposite statements about an indented table row now sit 100 lines
apart in `check-docs.sh`**: the 79-column rule exempts a table row at
any indent (ORCH-117), and this guard forbids that shape in `board.md`.
Both are right — one measures width, the other fixes a row's shape — and
each now carries a cross-reference to the other, which they did not.

**`gate.sh` writes no receipt on RED.** A green run leaves a
`<sha>.receipt`; a red run leaves only its log, under `target/`, in a
worktree the shared-host rule says to delete once pushed. So a red
verdict's figures survive only if an agent copies them out by hand, as
this handoff does. Worth its own row if it recurs.

**`ORCH-NEXT` appears in `scripts/`** — once in `check-docs.sh`'s comment
saying the placeholder is canonical, and twice in
`check-orchestration.sh`'s case asserting the guard does not refuse it.
No check anywhere covers a placeholder in a `.sh` (`check-docs.sh` looks
only for `D-NEXT`, only in `decisions.md`), so this is stated
deliberately: **all three are the rule's subject, not a leftover, and
there is nothing there for the merger to replace.** Only the three
`board.md` rows need numbers.

**My premise about the load-induced red was wrong**, and the record
should carry the correction rather than the premise. I implied those
integration tests have no load gate. `gate.sh` already has
`load_max`/`load_note` and reads the host twice, and its own header
records that those two figures **decide nothing**, because a burst that
begins and ends inside the suite is missed. This run is the third
measurement of exactly that: 1.45 and 10.36 sampled against a true peak
of 131.49, so no post-suite threshold catches it without refusing
routine work. And the per-test variant contradicts
`herd-hub/tests/real_herd/mod.rs:285-295` (a deadline that stretches to
fit the machine stops catching a product that got slower) and the
two-name delta contract of `scripts/expected-skips/`. The orchestrator
has refused that half on the evidence; the reporting half is its own
branch and not this one.

## Drafted for `herd-merger`

**Three `board.md` rows, all in the tree, all reading `ORCH-NEXT`, each
needing its own number in document order:**

1. `board.md:210`, *Owed*, state `done` — this task.
2. `board.md:342`, *Open*, state `open` — the six leading-space holes.
3. `board.md:343`, *Open*, state `open` — `check-docs.sh:143`'s
   non-failing gap note.

They need no paste; they are rows in the file. Nothing else is drafted:
**no `docs/decisions.md` entry**, by the orchestrator's decision, and
nothing to replace in `scripts/`.

The red run above is in *Gates* as labelled fields so that the landing
record can quote it from the tree.

## Decisions needed

None. The two `open` rows each carry a decision for whoever picks them
up, and neither blocks this branch.

## Resume point

`herd-merger` integrates this branch in the batch's one landing,
allocating three `ORCH-NEXT` numbers in document order. The reviewer's
six acceptance conditions are all addressed: the two pipe-spacing shapes
are guarded and cased, the domain claim is replaced by what the domain
actually is, "rowed" is made true, the `44`s carry a dated cost and the
CI factor, the duplicate-id caveat is in the script, the `ORCH-NEXT`
tokens are named as deliberate, and the red run is in this file.

## Processes

No background process left running; no PID killed, no `pkill`, nothing
by name. The only PIDs started were two `herdctl` submissions, a waiter
script of my own, and the shell steps above, all exited.

Own build tree `/mnt/ssd_980/herd/build/board-row-indent-guard`, used by
clippy and by the gate, deleted once the branch was pushed.
`/mnt/ssd_980/herd/build/link-cut-clock-figure` and
`/mnt/ssd_980/herd/build/t1t2-events-detector` are other tasks' and were
left alone, as was a concurrent `gate.sh` for `wt-events-detector` whose
full args I read before concluding it was not mine.
