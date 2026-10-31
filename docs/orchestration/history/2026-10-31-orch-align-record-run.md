# orch-align-record-run — handoff 2026-10-31

- **Item:** the `ORCH-NEXT` row drafted on `orch-333-figure-and-record`
  for exactly this gap. **This branch closes it and adds no row of its
  own**, deliberately: a second unnumbered row for one gap is how a
  duplicate lands.
- **Agent:** herd-step
- **Branch / commit:** `orch-align-record-run` / this commit, based on
  `origin/main` 7309b16798b45050f17bf9dfe9d1652d4fa68636
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The three documents that send the post-push run id somewhere a check
cannot reach now also send it where the check reads: **the previous
landing's `ci.yml` run id and a verdict for it go in the landing
record, in one paragraph that also names the batch they belong to.**
What they already said about the merger's *report*, the handoff's
*Resume point* and the board edit is kept — nothing was replaced, the
record was added beside it.

The guard this serves is `scripts/check-orchestration.sh`'s section *a
landing records its predecessor's post-push run* (ORCH-333), which
**is not in this branch**: it is on the unlanded branch
`orch-333-figure-and-record`, and this branch must land in the **same
batch** as that one. Landed alone, it documents a check that does not
exist; the other landed alone traps the next merger, who would follow
`.claude/agents/herd-merger.md` and go red at step 2 of its own
`gate.sh`.

| File | What was added |
|---|---|
| `docs/orchestration/workflow.md` | two new bullets at the end of *What a landing is*: *The next landing writes that run into its own record* (what the case refuses, the one-paragraph scope and its boundaries, the wrapped form as accepted, the literal `no ci.yml run`) and *What that case does not do* (no network, no toolchain, co-occurrence only, with batch 32 as the shape refused and batch 33 as the shape accepted). The one pre-existing sentence changed is at `:344`-`:346`, *"the next landing's board edit records it"* → *"the next landing writes it into its own record, and into its board edit"*, rewrapped inside the same four lines so that no line number moved before it |
| `docs/orchestration/templates/landing.md` | the *CI run* section gains the required paragraph (the three fields, where to read the conclusion back from, GitHub's conclusion words, the literal `no ci.yml run`, and that unlike the landing's own run it is **not** deferred and goes in the committed copy); the check and its one-paragraph scope stated under it, with what it cannot do; the header bullet becomes `- **CI run:** <id> — recorded, not waited on. Batch <n-1>'s own run, …`, which is itself an accepted shape; the *two copies are not identical* table row for the `ci.yml` run id now points at the next landing's record as well as its board edit, with a short paragraph naming the predecessor's run as the figure that is deferred nowhere |
| `.claude/agents/herd-merger.md` | step 5 (write the record) gains the instruction and the reason it is a gate and not bookkeeping, including that it runs at step 2 of the merger's own `gate.sh` at step 7; step 6 now says **your own** post-push run id is in the report and not in the handoff, and that the predecessor's is a different figure that stays; step 9's title becomes *Report your own post-push run; you cannot commit it*, and it ends by pointing at step 5 for the predecessor's |

**The guard's two literal constraints are stated where a writer meets
them**, because both are easy to get wrong from the prose alone:

1. **One paragraph, three things together** — the batch, the run id, a
   verdict for it. A paragraph ends at a blank line, at the next bullet
   or at the next heading, so a claim in one bullet is not answered by
   a figure in another; and wrapping over two or three lines is
   accepted, which is how batches 29, 30 and 31 wrote it. Every one of
   the three documents now says both halves, so nobody tries to cram
   the figure onto one line and nobody splits it across bullets.
2. **What the check is not.** It has no network and no toolchain. It
   cannot ask GitHub whether a run exists, whether its head sha is the
   one named or whether it passed. A mistyped digit passes it and so
   does an invented verdict. All three documents say this in their own
   register, and `workflow.md:330`'s *"that comparison is a human's"*
   is cited rather than contradicted.

**The two accepted shapes were tested against the guard's own
detector, not inferred from its prose.** The `awk` program of
`lr_prev_run` was copied verbatim out of
`origin/orch-333-figure-and-record:scripts/check-orchestration.sh` and
run with `num=33` over four fixtures:

```text
a   QUIET (accepted)   the compact `- **CI run:** … Batch 33's own run,
                       `ci.yml` 41228366473, `success` …` bullet, wrapped
                       over two lines — the shape the template's header
                       bullet now prescribes
b   QUIET (accepted)   the separate paragraph under `## CI run`, wrapped
                       over two lines — the shape the template's body
                       now prescribes
c   FIRES: names batch 33 but no paragraph carries id AND verdict
                       the shape batch 32's record actually had
d   QUIET (accepted)   `Batch 33: `no ci.yml run` — that push started
                       none.`
```

Fixture `c` is the known-bad one: the detector fires on it, so its
silence on `a`, `b` and `d` is a result and not an absence.

## Not done, and why

- **`scripts/check-orchestration.sh` is untouched**, as the brief
  requires. The guard is on another unlanded branch and its 11 cases
  must not drift; editing it here would guarantee a conflict over the
  one file whose cases are the specification this branch documents.
- **No `board.md` row and no `docs/decisions.md` entry.** The row for
  this gap already exists, unnumbered, on `orch-333-figure-and-record`.
  See *Drafted for `herd-merger`*.
- **The four Rust gates were NOT run locally.** This is a prose-only
  change under `docs/` and `.claude/`, both exempt; what covers them is
  the batch's own `./scripts/gate.sh` run at integration, which is
  `herd-merger`'s. No `cargo`, no Herd job, no `CARGO_TARGET_DIR`, no
  build tree created.
- **`workflow.md:364`-`:367` was left exactly as it stands**, including
  the claim that the merger puts the run id *"in its handoff's *Resume
  point*"*. The brief asked for it to be kept and it is cited by line.
  It is also, on the evidence below, not achievable — see *Risks and
  findings*, finding 1 — so it was not propagated into the two other
  documents.

## Gates

`./scripts/check-docs.sh`, run in this worktree **before** this handoff
was committed, verbatim:

```text
docs ok
```

`./scripts/check-orchestration.sh` — `main`'s version, which does not
yet contain the ORCH-333 section — run in this worktree before this
handoff was committed, so that the prose edits could not turn any of
its existing cases red. Last line, verbatim:

```text
check-orchestration: 324 passed, 0 failed
```

Width, measured over the three edited files with the same column rule
`check-docs.sh:85`-`:98` applies, and **without** its fence exemption,
so stricter than the gate: **no line this branch wrote or rewrapped is
over 79 columns**, in any of the three and in this handoff. The
over-long lines that remain in them are pre-existing and width-exempt:
the role and template table rows, and `herd-merger.md`'s YAML
`description:` at `:3`.

**The last run, named and not quoted:** `./scripts/check-docs.sh` on
the tree that carries this handoff, run after this commit and before
the push; its output goes to this agent's report, because nothing may
be added to the tree a gate has run on.

**Rust gates: NOT RUN** locally — prose only, `docs/` and `.claude/`
are exempt, and the batch's `./scripts/gate.sh` at integration covers
them. A gate that did not run is reported as not run.

**Skips:** no skip. Neither docs script prints `HERD-SKIPPED`, and no
test suite was run.

## Risks and findings

1. **`workflow.md:366` asks the merger for something ORCH-121 makes
   impossible, and it is not mine to edit.** It says the merger
   *"puts its id in its **report** to the orchestrator and in its
   handoff's *Resume point*"*. But the merger's handoff is **the last
   change before the gate** (`workflow.md:336`-`:347`,
   `.claude/agents/herd-merger.md` step 6), so it is committed before
   the push that starts the run exists. Batch 33's own merge handoff
   confirms the practice rather than the sentence:
   `history/2026-10-31-merge-batch-33.md:315` reads *"**None yet — this
   push starts it.** The id comes from `gh run list …` immediately
   after the push, goes into the merger's **report**"*. The brief told
   me to keep those four lines and they are cited by line, so I kept
   them; I did **not** carry the *Resume point* into the two other
   documents, which now source the id from the previous merger's report
   or from `gh run list --workflow ci.yml --branch main --limit 10`
   matched to that landing's merge sha. **Decision for the
   orchestrator, not an edit I took:** either drop *"and in its
   handoff's *Resume point*"* from `workflow.md:366`, or reword it to
   *"and its handoff says where the id is"*. Doing it in this branch
   would have moved `:364`-`:367`, which the brief protects.
2. **`workflow.md` gained 31 lines and every one of them is after line
   374.** `:330` (*"that comparison is a human's"*) and `:364`-`:367`
   are byte-identical and at the same line numbers — verified by
   printing them after the edit. Everything below `:374` moved by
   exactly +31 (file 1033 → 1064 lines), and **one live document cites
   into that range**: `docs/orchestration/board.md:147`, the ORCH-250
   row, whose *Evidence* cell names `docs/orchestration/workflow.md:620`-
   `:641` and `:829`-`:841`. Those are now **`:651`-`:672`** and
   **`:860`-`:872`** — checked by printing old `:620`, `:641`, `:829`,
   `:841` against new `:651`, `:672`, `:860`, `:872` and finding the
   same four lines. ORCH-250's State is `done`, `board.md` is outside
   my write paths, and editing a landed row's evidence cell in a
   prose branch is a conflict magnet, so this is a finding and not an
   edit.
3. **`.claude/agents/herd-merger.md` steps moved** (file 159 → 187
   lines). Step 5 still starts at `:50`; step 6 `:56`→`:78`, step 7
   `:64`→`:87`, step 8 `:78`→`:101`, step 9 `:100`→`:123`, step 10
   `:108`→`:136`. The brief's own citation `:100`-`:107` is now
   `:123`-`:135`. No live document cites this file by line; the two
   that do are history files — `history/2026-10-27-orch-record-fixes.md`
   cites `herd-merger.md:36`, unmoved, and
   `history/2026-10-28-merge-method.md` cites `:86`, now `:109` (same
   sentence, located by grep).
4. **The guard reads the handoff, not the merge commit, and the
   documents now say so by naming the file.** `lr_prev_run` globs
   `docs/orchestration/history/*-merge-*.md`; it never sees the merge
   commit message, exactly as `workflow.md:322`-`:330` already warns.
   So the paragraph has to be in the **committed copy** of the record,
   which is why `templates/landing.md` now states that this one figure
   is deferred nowhere. A merger who put the predecessor's run only in
   the merge message would pass `check-docs.sh` and fail ORCH-333.
5. **It is co-occurrence, not attribution, and the prose says so three
   times rather than once.** A paragraph that names the predecessor's
   batch and carries any 8-digit-or-longer number plus any accepted
   verdict word passes — including this landing's **own** run id if it
   happens to share the paragraph. That is the guard's stated limit,
   not a defect introduced here; it is written down in all three
   documents so that no later reader promotes the case into a
   verification.
6. **This handoff is not a `*-merge-*.md` file**, so it is not itself
   read by the ORCH-333 case group. The run ids quoted below are
   therefore safe to carry here and do **not** satisfy the landing's
   own obligation.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry is drafted.** The brief asked for none,
and nothing here changes what the product is or promises.

**No `board.md` row is drafted, on purpose.** `ORCH-333`'s branch
`orch-333-figure-and-record` already carries a row for exactly this
gap, written as `ORCH-NEXT` because batch 33 had two branches both
claiming ORCH-331. **This branch closes that row.** So, merger: number
that one row once, and set its State to **`done`**, not `open` — the
documents it asks for are in this branch. Do not add a second row.

**The figure this landing owes, and it is a gate.** Once
`orch-333-figure-and-record` is in the batch, the landing record of
**this** batch is the newest `*-merge-*.md` in the tree the gate runs
on, so its *CI run* must carry batch 33's post-push run or
`check-orchestration.sh` fails the case *landing: the real tree's
newest landing record carries it* at **step 2 of `gate.sh`** — before
any Rust step, and the landing stops there.

The figure, as the orchestrator read it with `gh run view` on
2026-10-31, quoted as **that run's verdict at that sha and not as a
standing fact**: batch 33 → `ci.yml` run **41228366473** on `main` at
`7309b167`, conclusion **`success`**, completed 18:24:15Z. Read it back
by id before writing it; the check cannot tell a figure that was looked
up from one that was not.

One accepted shape, verified against the guard's own detector as
fixture `a` above — the record's header bullet, wrapped:

```text
- **CI run:** none yet — this push starts it. Batch 33's own run,
  `ci.yml` 41228366473 on `main` at `7309b167`, `success`, read back by
  id with `gh run view`.
```

Equivalently, as its own paragraph under `## CI run` (fixture `b`). One
of the two is enough; both is harmless. What fails is the batch-32
shape: a sentence claiming the predecessor's run is recorded somewhere
else, with no figure in the same paragraph.

## Decisions needed

**One, and it is the orchestrator's, not Alexa's** — it is a *how*.
`workflow.md:366` asks the merger to put the post-push run id in its
handoff's *Resume point*, which ORCH-121 makes impossible (finding 1).
Recommendation: a one-line follow-up branch rewording it to *"and its
handoff says where the id is"*, after this batch lands, so that
`:364`-`:367` does not move while briefs still cite it. Fallback: leave
it; the sentence is harmless in practice, since every merger so far has
done the achievable thing and said so in its handoff.

## Resume point

Nothing is left in this task. For whoever continues: this branch is
pushed and must be integrated **in the same batch** as
`orch-333-figure-and-record` — either alone traps or orphans the other.
The merger's own first command, in its worktree, after the batch is
merged:

```sh
./scripts/check-orchestration.sh 2>&1 | grep -n "landing:"
```

all 11 ORCH-333 cases green, the last of them reading the record this
batch's own handoff carries.

## Processes

No PID started, no process killed, no background job. No `cargo`, no
Herd job, no `./scripts/bench-preflight.sh` run: nothing heavy was
needed for a prose change. **No `CARGO_TARGET_DIR` was created**, so
there is no build tree to delete; the only paths written are the three
files of the brief, this handoff, and four throwaway fixtures under
this agent's own scratchpad subdirectory.
