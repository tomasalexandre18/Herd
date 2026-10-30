# coverage-recount — handoff 2026-10-30

- **Item:** ORCH-241 (filed by this task); the recount itself answers no
  existing row, and it deliberately does not touch ORCH-210
- **Agent:** herd-step
- **Branch / commit:** `coverage-recount` / `071f0f5`, based on
  origin/main `26e83cf`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The recount's result first, because it is the point of the task: the
done figure did not move. 36 of 44 musts are wholly on `main`, exactly
as the 2026-10-29 count said. The shape of the eight that are not did
move.**

`docs/coverage.md`'s tally now reads: 36 wholly on `main`; **4** with
part of their work on `main` (E-12, E-16, E-17, E-37); **2** with none
of it (E-14, E-42); 2 unmet by choice (E-49, E-50, D206).
36 + 4 + 2 + 2 = 44, so the sum still visibly closes.

**Two rows moved, both from `planned` to `part`, and both because a
sentence in their own note is refuted by the tree.**

- **E-12** — its note said *"the pack half has no frame on either
  protocol enum and no code behind one"*. On `main` at `26e83cf`:
  `herd-proto/src/pack.rs`, five pack variants on `ToHub` (`PacksHeld`,
  `RegisterPack`, `AskPack`, `PackTransferred`, `PackUnavailable`) and
  five on `FromHub` (`PackHolders`, `PackRegistered`, `PackRefused`,
  `ReadPacksAgain`, `PinPacks`); `herd-hub/src/registry.rs`
  (`PackRegistry`, keyed on `(commit, key)`);
  `herd-runner/src/packstore.rs`, `fetch.rs`, `endpoint.rs`, `mesh.rs`;
  and `herd-hub/tests/packs_announced.rs` against both binaries as
  processes. The orchestrator's suspicion about this row was correct.
- **E-17** — its note said the manifest *"is part of M11's unbuilt pack
  half"*. The manifest half is built and its atomicity is the registry's
  own first stated property: one `rename(2)` out of
  `packs/manifests.part/<unique>/`, so a killed hub leaves a pack absent
  or whole with no third outcome (`herd-hub/src/registry.rs`).

**Neither row becomes `merged`, and the reason is the same for both:
nothing carries a pack through to a job.** Measured, three independent
sightings in the product:

1. the hub sends the assignment's packs empty — `packs_in: Vec::new()`
   in `herd-hub/src/server.rs`, in the `FromHub::AssignJob` it builds;
2. the runner binds and drops both fields — `packs_in: _, packs_out: _`
   in `herd-runner/src/lib.rs`, with a comment saying the `{in}`/`{out}`
   layout is *"laid out by nobody yet"*;
3. `ToHub::RegisterPack` is constructed **nowhere** under
   `herd-runner/src`, so no pack a job leaves is ever published.

`main`'s own prose agrees in two places written by other agents:
`herd-hub/src/registry.rs` (*"What is **not** wired is the assignment
side"*) and `docs/status.md` (*"The pack half is wired at the edges, not
through … no job is assigned a pack (ORCH-234)"*).

**Also restated, because the recount made them false:** the State
paragraph's date and the *State of play* heading (2026-10-29 →
2026-10-30); the *Gated on chatOnS* paragraph, which said E-12, E-14 and
E-42 are gated *and* have nothing on `main` — E-12 is now gated **and**
partly built, which the paragraph now says are two different columns;
the *Thirteen milestones* sentence's parenthesis about M11; the
*36 of 44* paragraph's breakdown; point 2 of *Five things a reader must
not mistake for done*; and the closing *three kinds of must* sentence.

**One more falsity found and fixed in the same file, outside the musts
count:** point 5 gave `PROTOCOL_VERSION` **2**, `RECORD_VERSION` **1**
and `LAYOUT_VERSION` **1**. Read off the tree at `26e83cf` they are
**3** (`herd-proto/src/lib.rs:160`), **2** (`herd-hub/src/job.rs:72`)
and **2** (`herd-hub/src/store.rs:99`) — D199's three moves are all
spent, in M11's pack half. `docs/architecture.md` and
`docs/handover.md` already carried the new figures, so `coverage.md` was
the one file disagreeing with the tree and with its two siblings.

**ORCH-210 is untouched.** Both ceilings still stand side by side with
the same numbers — D205's 44 of 44 and the 42 of 44 the arithmetic gives
with D206 — because my recount moved neither: it moved two rows between
`planned` and `part`, and both ceilings are about scope. The sentence
*"Which of the two statements moves is the scope of v1.0.0 … is not
settled in this document"* is unedited.

**Rows checked and deliberately left alone, each with what I measured:**

- **E-14** (`planned`) — no key-aware placement in
  `herd-hub/src/matcher.rs` (no pack, cache, locality or holder
  preference anywhere in it), no pack retention or budget at either end
  (ORCH-215), and no pack reaches a job. M12 has nothing on `main`.
- **E-16** (`part`) — unchanged. `PinPacks` is constructed nowhere: the
  only two occurrences outside `herd-proto` map the frame to a name for
  a log line (`herdctl/src/lib.rs:1656`, `herd-runner/src/lib.rs:4162`).
  No per-runner pack budget in `packstore.rs`. Keeper pinning and the
  per-runner budgets, which is exactly what its note says is left, are
  still what is left.
- **E-37** (`part`) — unchanged. `herd-hub/src/notify.rs` still has
  exactly four `Event` variants (`RunnerJoined`, `RunnerLost`,
  `OwnerSwitch`, `JobLost`). No detached-job-end event and no
  submitter's-asking field on a submission.
- **E-42** (`planned`) — unchanged, and this is the row I came closest
  to moving. `herdctl/src/declaration.rs` parses both `dedup_key` and
  `coalesce` from a declared command (`:267`, `:265`), but **neither
  field reaches a submission**: no occurrence of either outside
  `declaration.rs` in `herdctl/src`, no field on any protocol frame, and
  no coalescing or superseding by key in `herd-hub/src`. D75's concrete
  shape is parsed and not honoured, so none of E-42's own work is on
  `main`.
- **E-49, E-50** (`unmet by choice`) — not mine to move: that state is
  Alexa's decision (D206). I checked anyway that nothing undermines it:
  `deploy/wsl2-check.sh` and `deploy/capabilities-wsl-pc.json` are on
  `main` but date from `2ed8f63`, 2026-10-16, well before the count, and
  no Herd runner has run on WSL2.
- **The 36 `merged` rows** were not re-derived from the code one by one.
  What I checked is that nothing could have left the `merged` column: a
  row leaves it only by a regression, the four landings since the count
  are additive (`git diff --stat a15b52d~1 26e83cf` over the four
  crates: 4277 insertions, 121 deletions, no file removed), and no
  landing reverted a must. This is the recount's boundary and it is
  stated as one rather than implied.

## Not done, and why

- **`docs/status.md` not edited** — nothing in it was made false. Its
  point 2 already says the pack half is *"wired at the edges, not
  through"* with ORCH-234, and its version sentences are already right.
  So no line of its 90-line ceiling was spent.
- **`docs/plan.md` not edited** — out of this task's paths, and it
  carries the same stale *"`PROTOCOL_VERSION` 2 → 3 … in the same
  window"* framing at `:309`-`:310` and `:543`-`:545` as a *future*
  move that has in fact happened. A finding below, not an edit.
- **No `D-NEXT` entry** — the brief asked for a board row, not a
  decision, and nothing here needs a decision recorded: two State cells
  moved because the tree says so.
- **The Rust gates were NOT run** (see *Gates*).

## Gates

Prose only: the four Rust gates were **not run** locally and are not
reported as passed. What covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s.

`./scripts/check-orchestration.sh`, run on the tree at `071f0f5`
(before this handoff was committed), verbatim tail:

```
== the gate scripts' own stdin (ORCH-111)
ok    check-orchestration.sh redirects its stdin before its first capture
ok    gate.sh redirects its stdin before its first step
ok    pre-push keeps the stdin git hands it: the ref list is its input

check-orchestration: 324 passed, 0 failed
```

324 passed, 0 failed — the figure the brief gives, unmoved by this
branch, so no case moved and none needs naming.

`./scripts/check-docs.sh`, run on the tree at `071f0f5`, verbatim:

```
docs ok
```

**The last run**, on the tree including this handoff:
`./scripts/check-docs.sh`, its output in the agent's report, nothing
added to the tree afterwards.

**Skips:** no skip. `check-docs.sh` and `check-orchestration.sh` printed
no `HERD-SKIPPED` line. The 12 loud skips the suite reports on this host
belong to the Rust gates, which did not run.

## Risks and findings

1. **`docs/plan.md` still frames D199's three version moves as
   forthcoming.** `:309`-`:310` (*"the pack half pays its own break —
   `PROTOCOL_VERSION` 2 → 3, with `RECORD_VERSION` and `LAYOUT_VERSION`
   1 → 2 in the same window"*) and `:543`-`:545` (*"All three then move
   before v1.0.0"*) read as plans; the tree says 3, 2 and 2. Outside my
   paths. Small, and it is the same class of staleness this task was
   sent to fix, one file over.
2. **`herd-hub/src/registry.rs`'s module doc is now stale about the
   runner, by one batch.** It says *"the **runner** neither answers that
   frame nor constructs `ToHub::PacksHeld` of its own accord
   (`herd-runner/src/` was not this step's to change)"*. Batch 16's
   `runner-announces` made that false: `herd-runner/src/lib.rs:1303`
   and `:1345` send `ToHub::PacksHeld` unprompted. The hub's own half
   (*"the hub does not send `FromHub::ReadPacksAgain`"*) is still true —
   I checked: the frame is constructed nowhere outside tests. Product
   prose, outside my paths, one paragraph.
3. **A premise of the brief was off by one landing, in the direction
   that matters.** The brief says four landings reached `main` since the
   count (13, 14, 15, 16). It is **five**: batch 12 (`67bcd43`, *"M11's
   pack half and all three version numbers"*) is also dated 2026-10-30
   and is also after the 2026-10-29 count — and it is the landing that
   carried most of what refutes E-12 and E-17. The brief's own test is
   satisfied the other way too: `coverage.md` *was* touched after the
   count, by `890cb3e` and `a4b9d67`, both of which landed **in batch
   12**; neither changed a State cell or the tally (ORCH-206 work on the
   gated list), which is why the paragraph kept its 2026-10-29 date
   while the tree moved underneath it. The recount is counted against
   the tree at `26e83cf`, so the figure is unaffected; what changes is
   the size of the gap ORCH-241 describes.
4. **`part` for E-12 is a judgement with a defensible alternative, and
   it is mine, so here is the reasoning.** A reader could argue
   `planned` still, on the ground that no must of E-12 is *delivered* —
   no job gets a pack, so nothing E-12 promises happens. I chose `part`
   because this file defines `part` as *"a row some of whose work is on
   `main` and some of which is not … a row whose own list is partly
   built"*, which is about where the work is and not about what is
   delivered; and because `planned` is defined as *"unbuilt"*, which
   four thousand lines of pack code on `main` make plainly false. The
   `merged` column — the only one the headline figure counts — is
   untouched either way, so the published figure does not depend on this
   judgement. Reversible in two cells.
5. **A measured fact with a consequence I did not resolve.** Because
   `RECORD_VERSION` and `LAYOUT_VERSION` are already 2 on `main`, the
   sentence I removed from point 5 — *"the ordinary release before
   v1.0.0 therefore ships at layout 1 and record 1, which is binding"* —
   cannot be satisfied by any release cut from `main`. The new text
   states the figures and says what follows for the release is scope and
   is not settled in this document. **It is release scope, so it is
   Alexa's**, and it is a report and not a question: nothing in the
   queue waits on it.
6. **What I did not verify.** The 36 `merged` rows were not re-derived
   against the code individually — boundary stated above. Nothing was
   compiled and no test was run on this branch.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: this task drafts none and allocated no `D`
number.

**One `board.md` row, already written on this branch** at
`docs/orchestration/board.md`, in *Open — seen, not agreed or sized*,
immediately after ORCH-238. **ORCH-240 is the highest id on `main`,
verified by this task** (`grep -ohE 'ORCH-[0-9]+' docs/ | sort -t- -k2
-n | tail`, ignoring the template's ORCH-998/999), so this row takes
**ORCH-241**. It is on the branch rather than only here, so the merger
has nothing to paste; the full text is reproduced for the reason the
template gives — the merger reads the tree, and a row quoted here can
be checked against the row that landed.

```
| ORCH-241 | **`docs/coverage.md`'s musts tally is a hand count carrying its own date, and nothing makes a landing refresh it.** The one figure the owner's report format requires — musts done against total — lives in a single paragraph whose own words say when it was counted, and no gate, no landing step and no standing brief obliges anybody to recount it. **Measured by the recount that filed this row:** the paragraph read *"counted from the table below on 2026-10-29"* while **five** landings had reached `main` after it (batches 12 to 16), and two rows were stale in the same direction — E-12 and E-17 read `planned`, each with a sentence saying M11's pack half had no frame on either protocol enum and no code behind one, while the tree held `herd-proto/src/pack.rs`, the hub's registry and the runner's store, fetch and endpoint. Nothing was red and nothing noticed. The drift is silent and one-directional: a landing can make the figure stale and can never correct it | `coverage-recount`, 2026-10-30; the recount itself is the measurement, and the stale figure is what the task was sent to find | the orchestrator | open | **Three shapes, stated rather than chosen, because which one is right is a method ruling and not this step's.** (a) **A landing-time obligation on `herd-merger`** — cheapest to write, and forgettable, being one more line in a record that already carries many. (b) **A check in `check-docs.sh` that fails when the State paragraph's date is older than `main`'s last landing that touched product code** — the only shape that cannot be forgotten; it needs a definition of *touched product code* and a way to read that landing's date with no toolchain, both of which the script's existing git-free style makes a real cost rather than a line. (c) **Leave it a periodic audit**, as today, and accept that the figure is as old as the last agent who was sent to look. This step leans to **(b)**, for the one reason that it is the only shape that cannot be forgotten — **a lean and not a ruling** — and notes that (a) and (b) are not exclusive: the obligation is what a merger does, the check is what notices when nobody did | — | `docs/coverage.md` State paragraph and tally line, both restated at this row's filing; `history/2026-10-30-coverage-recount.md`; ORCH-210, whose two ceilings this row deliberately does not touch |
```

## Decisions needed

None from Alexa, and the one method question is on the board rather than
here: **which shape ORCH-241 takes** is the orchestrator's, reversible,
and the row states the three with their costs and names its own lean as
a lean.

Reported with no question attached, per `CLAUDE.md`'s third outcome:
finding 5 above — the layout and record numbers on `main` have already
left 1, so the *"ordinary release before v1.0.0 ships at layout 1 and
record 1"* constraint is no longer satisfiable from `main`.

## Resume point

Nothing in this task is half-finished. The next steps it suggests, in
the order I would take them:

1. Rule on ORCH-241's shape; if (b), a `herd-step` task against
   `scripts/check-docs.sh` plus the two source-of-truth questions the
   row names.
2. One prose task for finding 1 (`docs/plan.md`'s three version
   sentences) and finding 2 (`herd-hub/src/registry.rs`'s module doc,
   one paragraph, stale by batch 16) — both are one-file edits, and the
   second needs the Rust gates because it touches a crate.
3. The recount's own boundary: the 36 `merged` rows have never been
   re-derived against the code row by row. That is a bigger task than
   this one was and worth a brief of its own.

## Processes

PIDs started and ended: none — no compile, no binary, no background
job. `./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`
ran in the foreground and exited.

No `target/` was created: this branch builds nothing, `CARGO_TARGET_DIR`
was never set and `./scripts/bench-preflight.sh` was not needed, there
being no heavy phase. The worktree holds docs changes only and can be
removed once the branch has landed; it is at

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/coverage-recount/wt
```
