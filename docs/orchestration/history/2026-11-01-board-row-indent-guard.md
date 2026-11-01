# board-row-indent-guard — handoff 2026-11-01

- **Item:** ORCH-NEXT
- **Agent:** herd-step
- **Branch / commit:** `board-row-indent-guard` / see the agent's report
  for the sha, based on origin/main `d6c4048`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A board row that does not start at column 1 is now refused, and the one
row that did not has been de-indented in the same commit.**

The hole, re-measured on this branch before anything was changed: all
three row-level checks over `board.md` anchor at the start of a line on a
pipe followed by `ORCH-` — the ten-field count, the known-state check and
the duplicate-id check — so a row indented by even one space was read by
none of them. One such row existed, `board.md:207` (ORCH-341), against
317 at column 1. It was substantively correct: ten fields, state `done`,
which the three checks confirm now that they can see it. Nothing was
broken on `main`; the hole was latent.

### The choice, and the alternative I did not take

`check-docs.sh` **rejects** the indented shape. The alternative was to
anchor the three checks at an optional-whitespace prefix and accept both
shapes; it is fewer lines and worse in kind, because a row then has two
spellings of one thing and every check written over that table afterwards
has to carry the tolerance for ever or quietly lose a row again. The
indentation was a typo and not a style — 1 against 317 — and `board.md`
is a live document, not a dated record, so de-indenting it is an ordinary
edit. I agree with the brief's ruling and the tree gave me no reason to
contradict it.

The new guard is one grep over one small file, flat in the corpus, placed
with the three checks it completes. Its domain is exactly their blind
set: an indented line whose first cell is an ORCH id. It keeps the three
anchors unchanged, so the legal spelling stays the only one.

### The fixture, and what it proves

Six cases in `check-orchestration.sh`, beside the four column-1 cases of
the same family, which keep passing unchanged:

- a row indented by one space — red;
- a row indented by a tab — red;
- an indented row whose **state cell is broken** — red on the
  indentation and **silent about the state**;
- an indented row with **too few cells** — red on the indentation and
  silent about the field count;
- an indented **duplicate id** — red on the indentation and silent about
  the id;
- a well-formed row at column 1 — green, which is what the guard must
  leave alone.

The last three need an assertion `expect_red` cannot make, since a
message naming any defect at all would satisfy it, so they use a new
`expect_red_quiet <name> <must print> <must not print>` helper. **Those
three cases are what tell the two shapes apart**: the same broken state
cell goes red on `unknown state` at column 1 and red on the indentation,
silent as to its state, one space in. A fix whose own control cannot do
that reproduces the defect it closes.

The guard was also proven on the **live known-bad** before the row was
touched: on the real tree, still indented, `check-docs.sh` went red
naming `board.md:207`. And the new helper was itself proven to fail in
both directions — a case forbidding the message it does get, and a case
demanding a message nothing prints — both FAIL, the real case ok.

### A figure both scripts document, corrected

Both scripts say in nine places that `check-docs.sh` is run **38** times
over a full corpus copy, a cost multiplied into the `docs` job's
5-minute cap. My six cases make it 44, so leaving 38 there would have
been a true sentence carrying a false number. Counted, not inferred: a
shim that logged every invocation measured 41 runs on the base, of which
3 are in the small synthetic repo of the parallel-decisions section, so
38 whole-corpus — the comment was exactly right — and 47 on this branch,
44 whole-corpus. All nine mentions now read 44.

One of the nine, the `fresh()` comment, names *copies* rather than runs,
and that figure is one higher: 39 `fresh` call sites on the base, 45 on
this branch, because one case sets up a copy it reads without running the
script. That comment said 38, so it was off by one before my change as
well; it now says 45 copies of which 44 run `check-docs.sh`, and the
distinction is spelled out rather than left as an apparent inconsistency
with the other eight.

## Not done, and why

- **`docs/decisions.md` is untouched.** It is append-only and the
  merger's alone. The brief did not ask for an entry; one is drafted
  below anyway, marked optional, because the merger reads the tree and
  could not otherwise take it.
- **The two sibling holes are rowed, not fixed** — see *Risks and
  findings*. The brief said to fix only the ones over the table I was
  already touching, and both of the others are over files out of scope.
- **No reviewer pass.** `workflow.md` step 6, widened by ORCH-335 to
  name every gate script, requires one before integration, and both
  files I changed are gate scripts.
- **`scripts/gate.sh` and `scripts/test.sh` untouched**, as the brief
  required; `wc -l` against the base confirms 733 and 302 unchanged.

## Gates

`scripts/` is not prose-exempt, so this runs all four Rust gates plus
`check-docs.sh`. `cargo` only inside the flake. No `.rs` file is touched
by this branch, so the Rust gates are a formality here — run in full
regardless, and reported as run.

`git status --porcelain` before starting the Rust gates, which gate step
0 refuses outright on a dirty tree — the three expected files and
nothing else:

     M docs/orchestration/board.md
     M scripts/check-docs.sh
     M scripts/check-orchestration.sh

`cargo fmt --all --check`, exit 0, no output but the flake's note that
the worktree is dirty:

    warning: Git tree '…/board-row-indent-guard/wt' is dirty

`cargo clippy --workspace --all-targets -- -D warnings`, same tree, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/board-row-indent-guard` — the
dir the gate reuses — exit 0, **22.8 s** wall on a cold dir:

    Checking herd-proto v1.0.0 (…/herd-proto)
    Checking herd-hub v1.0.0 (…/herd-hub)
    Checking herd-runner v1.0.0 (…/herd-runner)
    Checking herdctl v1.0.0 (…/herdctl)
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.11s

The only line in that log matching `^warning` is the dirty-tree note
above; no clippy lint fired.

`./scripts/check-docs.sh` on the tree with the guard added and the row
still indented — the known-bad control, which must be red:

    docs/orchestration/board.md has rows that do not start at column 1,
    where the row checks cannot see them (not their ten fields, not
    their state, not their id); de-indent them:
        line 207: | ORCH-341 | **Nothing derives, after the 30 s give
    docs check failed

`./scripts/check-docs.sh` after the row was de-indented: `docs ok`,
exit 0.

`./scripts/check-orchestration.sh`, the meta-gate, on the base and on
this branch:

    base d6c4048:  check-orchestration: 351 passed, 0 failed   (53.1 s)
    this branch:   check-orchestration: 357 passed, 0 failed   (59.5 s)

351 + 6 = 357, and the diff of the two runs' result lines is exactly my
six new cases plus the random temp-directory name — no other case
changed, in either direction. The +6.4 s is six more whole-corpus runs
of `check-docs.sh` against the `docs` job's 5-minute cap, which leaves
that job at about a fifth of its budget.

**The last run, named and not quoted**, on the committed tree including
this file, as the template requires: the whole gate, bare, through the
queue —

    /mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run \
      --label build --memory-mib 4700 -- /bin/sh -c "cd <worktree> && \
      export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/\
    board-row-indent-guard && ./scripts/gate.sh"

— its receipt and log path, and its verbatim output, are in the agent's
report, because a handoff cannot carry the output of the run that gates
it.

**Skips:** no skip, listed and not counted (D187, D27). Neither
`check-docs.sh` nor `check-orchestration.sh` printed a `HERD-SKIPPED`
line or any skipped case, on the base or on this branch; the one skip the
previous handoff on this file reported (a landing record predating the
**Previous run:** field) is now a pass. The suite's own skip list belongs
to the final gate run and is in the agent's report, with its
`ignore-list-sha256`; this change touches no `#[ignore]` and no test, so
it should move neither sha.

## Risks and findings

**Two sibling holes of the same family, measured and not fixed.** The
brief asked whether other checks anchored at `^` over `board.md`,
`coverage.md`, `plan.md` or `decisions.md` would be defeated by the same
leading space. Yes, six of them, over two files that are out of my scope:

- `docs/decisions.md`, five checks, all anchored on `^## `:
  `check-docs.sh:126` (malformed header), `:129` (**the `D-NEXT`
  placeholder left behind**), `:132` (duplicate number), `:141` (the
  highest number) and `:143` (the gap list). A decision header indented
  by one space escapes all five, and Markdown still renders it as a
  header at up to three spaces of indent. Today: 0 indented against 289
  at column 1. The `D-NEXT` one is the one that would cost something — it
  is what stops a placeholder reaching `main`.
- `docs/coverage.md`, one check: `check-docs.sh:107`, the orphan-must
  rule, anchored on a column-1 pipe followed by `E-`. An indented must
  row is not checked for a milestone. Today: 0 indented against 55 at
  column 1.

Both are latent, as this one was; neither is wrong on `main` today. They
are rowed rather than fixed, per the brief. If the same guard is wanted
there, it is the same shape in both places.

**What the same space does *not* defeat**, checked and reported either
way: `check-docs.sh:172`, the board's section-order check, fails safe —
an indented `## Open` drops out of the list and the order string
mismatches, so it goes red, not quiet. The landing-record awk at `:255`
fails safe the same way, reporting the section missing. The agent
frontmatter checks at `:159`-`:160` are `grep -q || note` and go red. The
79-column rule and the brief's Base line are already
whitespace-tolerant (`:86`, `:93`, ORCH-117). `docs/plan.md` has no
row-level or line-level check at all beyond the width rule, so there is
nothing there to defeat.

**A known limit of the new guard.** It is not fence-aware, so a board row
written as an indented example inside a fenced block in `board.md` would
go red. `board.md` holds no fenced block at all today and no indented
table row other than the one fixed, so nothing legitimate is in range —
and the three checks it backs up are not fence-aware either, so a
column-1 example row inside a fence is already counted by them. I treated
that as the existing property rather than widening it; the comment says
so, and the answer for such a row would be to unnest it.

**The nine `38`s were a real stale-figure risk** and are the one thing I
changed beyond the brief's literal subject. It is inside the two files
the brief names, and the alternative was to add six runs and leave nine
comments asserting the old cost.

**Nothing in the tree contradicted the brief.** Every figure it gave
reproduced exactly: 1 indented row at `board.md:207` (ORCH-341), 317 at
column 1, the three anchors at `:174`, `:177` and `:180`, the fixture
style at `:132`-`:137` with the synthetic two-name pattern, the
column-1-only fixtures, and 351 passed / 0 failed on the base. The only
correction is the one above, that the `fresh()` comment's 38 named copies
and was already off by one.

## Drafted for `herd-merger`

**The `board.md` row**, already on the branch in the *Owed* section after
ORCH-343, written as literally `ORCH-NEXT` for the merger to number. It
is in the tree, so it needs no paste; its text is the row at
`docs/orchestration/board.md:210`.

**A `docs/decisions.md` entry — optional.** The brief did not ask for
one and I have written nothing to that file. Take it or drop it; the rule
it records is enforced mechanically and documented in both scripts
either way. Written unfenced, so `check-docs.sh` width-checks it here
rather than at integration, and the header is 53 columns at `D-NEXT` and
51 at a three-digit number:

## 2026-11-01 — D-NEXT A board row starts at column 1

A row in `docs/orchestration/board.md` begins at column 1. The indented
spelling is refused by `scripts/check-docs.sh`, not tolerated.

All three row-level checks over that table anchor at the start of a line
on a pipe followed by `ORCH-`: the ten-field count, the known-state check
and the duplicate-id check. A row indented by one space was read by none
of them, and one such row was on `main` — correct in substance, so
nothing was broken, but every check written over that table afterwards
would have inherited the blind spot. Batch 36's merger proved it rather
than inferring it, by breaking that row's state cell with the leading
space intact and getting silence from all three.

Accepting both shapes, by anchoring the three at an optional-whitespace
prefix, was the cheaper fix in lines and the worse one in kind: a row
becomes two spellings of one thing and the tolerance has to be rewritten
into every future check, or a row is quietly lost again. The indentation
was a typo, not a style, at 1 against 317, and the board is a live
document rather than a dated record, so the existing row was de-indented
in the same commit that made the shape illegal.

The same leading space defeats five `^## ` checks over `docs/decisions.md`
— the `D-NEXT` placeholder check among them — and the orphan-must check
over `docs/coverage.md`. Those are recorded as open work, not fixed here.

D27, ORCH-117, ORCH-335.

## Decisions needed

None. One offer, not a question: the drafted decision entry above is
optional and the merger may drop it without anything else changing.

## Resume point

A `herd-reviewer` pass on branch `board-row-indent-guard`
(`workflow.md` step 6, widened by ORCH-335 to name every gate script;
both changed files are gate scripts). The reviewer's material is this
file's *Done* and *Risks and findings*: the design choice and the
alternative, the six fixtures and what the three red-and-silent ones
prove, the new `expect_red_quiet` helper and its own proof, the nine
corrected `38`s, and the two sibling holes left rowed. After review, the
branch is `herd-merger`'s to integrate in the batch's one landing.

## Processes

No background process started, no PID started or ended, nothing killed.

Own build tree `/mnt/ssd_980/herd/build/board-row-indent-guard`, used by
the clippy run and by the gate, deleted once the branch was pushed.
`/mnt/ssd_980/herd/build/link-cut-clock-figure` is another task's and was
left alone.
