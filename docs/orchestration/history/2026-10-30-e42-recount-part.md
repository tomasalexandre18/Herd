# e42-recount-part — handoff 2026-10-30

- **Item:** ORCH-NEXT (no number allocated; the orchestrator's ruling of
  2026-10-30, briefed to this agent)
- **Agent:** herd-step
- **Branch / commit:** `e42-recount-part` / see the push, based on
  origin/main `df47266`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**E-42 is recounted `planned` → `part`, and the tally is re-derived from
the rows rather than patched.** Two files changed, prose only:
`docs/coverage.md` and `docs/road-to-v1.md`.

**The arithmetic, re-derived from the 44 `must` rows at `df47266` after
the move** (read off the State column of `docs/coverage.md`'s table, not
carried forward):

```
must rows: 44
  merged     36
  part        5   E-12 E-16 E-17 E-37 E-42
  planned     1   E-14
  waived      1   E-49
  withdrawn   1   E-50
SUM: 36 + 5 + 1 + 2 = 44
```

**It does not come out as the brief predicted, and the difference is one
row: E-14.** The brief said *"36 wholly + 5 part + 0 with nothing + 3 the
consumer released us from = 44"*, with *"E-50 and E-14 withdrawn"*. Both
distributions sum to 44, and the headline figures (**36 of 44** on
`main`, **42 of 44** for v1.0.0) are the same under either. What the tree
refutes is the claim that **E-14 is withdrawn**; the evidence is under
*Risks and findings*. **The file carries the row-derived distribution,
1 / 2, not the brief's 0 / 3.**

### `docs/coverage.md`

- **E-42's row**: State `planned` → `part`; the note now names the dedup
  half as built and asserted (the derived key at
  `herd-hub/src/job.rs:817`, the join of any non-terminal job at
  `herd-hub/src/store.rs:2234`, asserted in
  `herdctl/tests/declared_dedup.rs`) and the coalesce half as absent (no
  `coalesce` on `herd_proto::Asked`, no `superseded` outcome, the two
  declared fields parsed and discarded in
  `herdctl/src/declaration.rs`), with D75's *"Dedup joins; coalescing
  discards"* as the reason the live joining is not evidence against the
  row. The ruling is attributed to the orchestrator, 2026-10-30.
- **The opening count** (was `:21`-`:25`): re-derived at `df47266`,
  **36 + 5 + 1 + 2 = 44**, with the sentence it replaces quoted so a
  reader can see what moved and why. Two paragraphs added: one for the
  E-42 recount and its evidence, one saying E-14 is `planned` and that
  retiring a must is the consumer's act alone (D248, D249). The
  paragraph that said *"the count above is not a fresh recount"* is
  replaced, because it now is one; its honest replacement says that only
  E-42's cell was re-measured against the code and the other 43 carry
  their 2026-10-30 reading. ORCH-241's standing staleness note is kept.
- **The *Gated on chatOnS* column sentence** (was `:126`): *"E-14 and
  E-42 are gated and have nothing on `main`"* → E-14 alone has nothing;
  E-12 and E-42 are gated *and* partly built.
- **The *State of play* count** (was `:194`-`:198`): five partly built
  (E-42 added, with its two halves named), one with nothing (E-14).
- **The closing count** (was `:296`): *"four are partial"* → **five**,
  E-42 added.
- **The release figure** (was `:208`-`:212`): *"the newest release
  anybody can download is v0.3.0"* was two releases stale and now reads
  **v0.4.1**, sourced to `docs/status.md`. The deployed hub and runner
  versions in that same paragraph are **not** restated as live figures:
  they are `docs/handover.md`'s dated reading (pin `v0.3.2`, unit
  `herd-runner-0.3.2`, container `herd-hub:0.3.3`), what the consumer's
  machines run reaches us only as a dated report (**D161**), and
  `docs/status.md` implies a runner that has since left 0.3.6. Two
  documents disagreeing about their fleet is how the line went stale, so
  the paragraph now points at `status.md` instead of competing with it.

### `docs/road-to-v1.md`

- **The false sentence is corrected and the correction is attributed.**
  Step 3 of *What is left to v1.0.0* said of E-42: *"`coalesce` and
  `dedup_key` are parsed, validated and discarded today (ORCH-148), so
  this is wiring a field that already exists on the wire."* The second
  clause is false and **the text now says it was the orchestrator's own
  sentence, repeated from one of its briefs**. Measured at `df47266`:
  `herd_proto::Asked` carries `needs` and `leaves` and neither
  `coalesce` nor `dedup_key`; `herdctl` has no `--coalesce` flag;
  neither field is anywhere on the wire. The replacement says what is
  true — the dedup behaviour is delivered on a key the **hub derives
  itself**, while the two declared fields reach nothing — and that what
  is left needs an additive `serde(default)` field on `Asked` (D229) and
  a new job-outcome variant an older `herdctl` would meet inside a frame
  it decodes. **It says the timing is open and Alexa's.**
- **B5 is green.** It said *"`main` is still 2 / 1 / 1"*; the tree reads
  **3 / 2 / 2** — `PROTOCOL_VERSION` 3 at `herd-proto/src/lib.rs:160`,
  `RECORD_VERSION` 2 at `herd-hub/src/job.rs:72`, `LAYOUT_VERSION` 2 at
  `herd-hub/src/store.rs:99` — landed with M11's pack half from batch 12
  (D229-D235, D243).
- **B1 is green.** It said *"Alexa's — ORCH-210, D205 against D206"*;
  **D248** settled it (Alexa's ruling, the consumer asked under D249 and
  waiving both M17 musts, 42 of 44 named in the notes), so ORCH-210 is
  closed.
- **The blocker summary** *"Five green, two in flight, three that no
  amount of work here closes"* → **seven green, one in flight (B7), one
  half (B8), one nobody has started (B3)**, with B1 named as no longer
  his. **B8 was checked and is unchanged**, as the brief said.
- **The provenance of the shas is kept honest.** The blocker table's
  column header and lead-in now say *state read at `5f73c18`, except B1
  and B5, re-read at `df47266`*; the other eight **were not re-read**
  and each still carries its `5f73c18` reading. The 2026-10-30
  advancement row is **not edited** — see *Not done*.

## Not done, and why

1. **The stated-at sha of `docs/road-to-v1.md` is not brought to
   today, and no new advancement row is added.** The brief allowed this
   (*"if that is more than one task's worth, do B5 and B1 and say which
   you left"*). Re-stamping the table at `df47266` would claim a
   re-reading of ten blocker statements I made for two; the file's own
   rule is *"a figure you cannot take, mark **not measured** and leave
   in place"*. **Left: B2, B3, B4, B6, B7, B9, B10 at `5f73c18`.** Adding
   the next advancement row is its own task and the file documents how
   (*Adding the next row*): name the sha, recount `coverage.md`'s must
   column, re-read all ten statements, take branch and commit counts
   from `git log`, read CI with `gh`.
2. **The 2026-10-30 advancement row (`:22`) is untouched**, although its
   cells read *"4 unbuilt (E-12, E-14, E-17, E-42)"* and *"44 of 44
   claimed by D205 and unreconciled — ORCH-210"*. It is a **dated point
   of measurement at `5f73c18`**, and the file's procedure is to add a
   row, never to amend one. Instead a bullet was added to *Provenance of
   the top row* saying ORCH-210 was settled later the same day by D248,
   that B1 carries the settled reading, and that `unmet by choice` is
   D206's vocabulary which `coverage.md` no longer uses.
3. **`docs/orchestration/board.md` is not edited** (brief). `:249` and
   `:576` both still count E-42 as having nothing. Corrections drafted
   below.
4. **`docs/decisions.md` is not edited** (brief). The entry is drafted
   below, headed `D-NEXT`.
5. **`cut-v1.0.0` (`f3e9df0`) is not touched** (brief). See the line
   addressed to the orchestrator under *Risks and findings*.
6. **No `gated` marker was added to E-42's row**, although the row would
   now carry one naturally. `coverage.md`'s *Gated on chatOnS* entry for
   E-42 states on purpose that *"the row carries no `gated` marker …
   the marker is missing, the fact is not"* (ORCH-206, D238), and adding
   the marker would make that sentence false. Whether an expectation is
   gated is a judgement about the consumer's acceptance, which D238 says
   is not an agent's edit. The row now ends by pointing at that section.
7. **The four Rust gates were NOT RUN.** Prose only.

## Gates

**Prose only, so the four Rust gates were NOT RUN locally** — not
`cargo build`, not `./scripts/test.sh`, not `cargo clippy`, not
`cargo fmt --check`. What covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s and not
this agent's. **A gate that did not run is reported as not run, never as
passed.**

Run **before** this file was committed, on the two edited docs:

```
$ ./scripts/check-docs.sh
docs ok
```

**The last run, named and not quoted:** `./scripts/check-docs.sh` from
the worktree root on the committed tree, after this file was committed
and before the push. Its output goes verbatim into this agent's report
and to this file in the agent's scratchpad:

```
<scratchpad>/e42-check-docs-final.txt
# <scratchpad> = /mnt/ssd_980/herd/claude/tmp/claude-1002/
#   -mnt-ssd-980-herd-project/
#   3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad
```

**No skip.** `check-docs.sh` runs no test and emits no `HERD-SKIPPED`
line; the suite was not run, so its 12 loud cgroup/ZFS skips were
neither observed nor are they claimed.

## Risks and findings

1. **The brief's premise that E-14 is withdrawn is refuted by the tree,
   and the file does not say it.** Evidence, all at `df47266`:
   **D248** (`docs/decisions.md:12757`) withdraws **E-50** alone — *"E-50
   is WITHDRAWN, not waived"*, on the consumer's own words about their
   fleet — and **waives E-49**; it names no other row. **E-14's State
   cell reads `planned`** and its note reads *"gated on their
   replication (M18)"*. The last agent to measure it wrote *"**E-14**
   (`planned`) — no key-aware placement in `herd-hub/src/matcher.rs` …
   no pack reaches a job. M12 has nothing on `main`"*
   (`history/2026-10-30-coverage-recount.md`). No decision anywhere
   retires it. **So the tally is 1 with nothing / 2 released, not 0 / 3**,
   and `coverage.md` now says so, with a paragraph explaining that
   retiring a must is the consumer's act (D248, D249) so that the same
   reading is not made again. **If the orchestrator believes E-14 should
   be withdrawn, that needs a consumer answer of D249's shape, and it is
   not an edit any agent may make.** The brief's own instruction was to
   re-derive from the rows and report a difference; this is the
   difference.
2. **The orchestrator's headline figures are unaffected.** 36 of 44 on
   `main` and 42 of 44 for v1.0.0 hold under both distributions. What
   would have been wrong is only the breakdown of the eight, and only in
   the brief's version.
3. **One nuance in the brief's D229 reading, kept as a hedge.** D229's
   named unrecoverable forms are *a new `ToHub` variant* and *a
   `FromHub` variant a requester sees*. A new `JobOutcome` variant is
   neither literally: it travels **inside** an existing frame. An older
   `herdctl` meeting an unknown outcome tag fails the same way, so the
   hazard is the same, and both edited files say *"not literally one of
   D229's two named unrecoverable forms, but the same hazard"* rather
   than asserting D229 covers it. **This does not change the conclusion
   that the coalesce half is Alexa's to scope.**
4. **`docs/plan.md` already agreed with the correction and was left
   alone.** `plan.md:493`: *"What is left of this milestone is E-42's
   coalesce path — `coalesce` is parsed by `herdctl`'s declaration today
   and nothing in the hub or the protocol supersedes a queued job on
   it."* That is the same reading this recount writes into
   `coverage.md`, so plan.md needed no edit. Corroborated independently
   at `docs/remediation/03c-contradictory-review.md:172`, which measured
   the two declared fields inert with line numbers.
5. **`docs/architecture.md:1407` is now stale about two rows and is
   outside this task's paths.** Its drawing-scope table says of
   *"M14's scheduled jobs and push hook (E-41, E-42)"*: *"not drawn —
   nothing of them exists to draw"*. **E-41 is `merged`** and E-42's
   dedup half is built. A finding, not an edit.
6. **The false clause existed in exactly one place, not several.**
   `grep` for *"exists on the wire"* and *"wiring a field"* across the
   tree hits only `road-to-v1.md`. The sentence that was in five live
   places is the other one — *that E-42 has nothing built* — and all
   five were in the two files this branch edits (E-42's State cell,
   `coverage.md` `:22`-`:24`, `:126`, `:194`, `:296`) plus
   `road-to-v1.md`'s step 3. The two board sites are drafted below.
7. **A line addressed to the orchestrator, in writing where you will
   see it:** **`cut-v1.0.0` (`f3e9df0`) carries a release body whose
   scope table reads *"Nothing of it in this release | 1 | E-42"*, and
   that table is now wrong. It needs the same recount before the tag.**
   The branch is unlanded and unsigned and **was not touched**. E-42
   belongs in whatever row that table uses for a partly-built must, with
   the coalesce half named as what is absent; and the body must still
   name E-49 and E-50 as not met, which is the consumer's own condition
   for the waiver (D248).
8. **Nothing in this branch was measured by running a binary.** Every
   code claim is read off the source at `df47266` at the file and line
   given. The one claim that rests on another agent's run is that
   `herdctl/tests/declared_dedup.rs` passes; it is on `main` and the
   batch's `gate.sh` is what asserts that.

## Drafted for `herd-merger`

**The `docs/decisions.md` entry, in full. Paste as written; allocate the
next free number in place of `D-NEXT` and leave the gaps (D148).**

```markdown
## 2026-10-30 — D-NEXT E-42 is `part`: its dedup half is built, its coalesce half is not

**Context.** `docs/coverage.md` counted E-42 — *a push hook submits the
producer* — among the musts with **nothing** on `main`, in five places, and
`docs/road-to-v1.md` said of it that the work left *"is wiring a field that
already exists on the wire"*. An execution agent measuring E-42 on
`e42-producer-submit` (`c47a72a`) found the row is **two rules** and reported
that one of them is live; it correctly declined to move a must between columns
itself.

**Choice, the orchestrator, 2026-10-30, with its evidence.** E-42's State cell
is **`part`**, the same shape E-12, E-16, E-17 and E-37 already carry, and the
coalesce half is named as what is absent.

- **Dedup — joining — is built and asserted.** The hub derives the key itself
  over the command, the label set and the declared commit
  (`herd-hub/src/job.rs:817`, length-prefixed fields, the commit a counted
  third since **D156**), and `JobDirStore::submit` looks it up in `open_work`
  under the store's one lock and joins **any non-terminal job, running
  included** (`herd-hub/src/store.rs:2234`). It is asserted against the real
  binaries in `herdctl/tests/declared_dedup.rs`, whose own doc comment reads
  *"A push hook firing twice is the case this exists for"*. The **detached**
  path — `herdctl submit`, which a hook uses and which no test covered — is
  added on `e42-producer-submit` in `herdctl/tests/detached_dedup.rs`, with a
  named control that was watched failing and then deleted.
- **Coalesce — discarding — exists nowhere.** No `coalesce` field on
  `herd_proto::Asked`, no `superseded` job outcome, no `--coalesce` flag; the
  declared `coalesce` and `dedup_key` are parsed and validated in
  `herdctl/src/declaration.rs` and read nowhere else, so neither declared
  field reaches the wire and the delivered behaviour rides a key the hub
  derives. **D75** keeps the two apart — *"Dedup joins; coalescing discards.
  They are two behaviours, not one"* — and considers dedup first, so joining a
  **running** job is correct for dedup and forbidden for coalesce, and the
  live joining is **not** evidence against E-42.

**The tally, re-derived from the 44 `must` rows and not patched in place:**
**36** wholly on `main`, **5** `part` (E-12, E-16, E-17, E-37, E-42), **1**
with none of it (**E-14**), **2** the consumer has released us from (E-49
waived, E-50 withdrawn, **D248**). 36 + 5 + 1 + 2 = 44. The headline figures
do not move: 36 of 44 on `main`, 42 of 44 for v1.0.0.

**E-14 is `planned`, not withdrawn, and this is recorded because the mistake
was made in the brief that ordered this recount.** D248 withdraws **E-50**
alone and waives E-49; it retires no other row. E-14's cell reads `planned`,
its note reads *"gated on their replication (M18)"*, and M12 has nothing on
`main`. **Retiring a must is the consumer's act and nobody else's** (D248,
D249): no recount may move a row into `withdrawn` without one.

**A false sentence of the orchestrator's is corrected, and the correction says
whose it was.** `docs/road-to-v1.md` said *"`coalesce` and `dedup_key` are
parsed, validated and discarded today (ORCH-148), so this is wiring a field
that already exists on the wire."* The first clause is true. **The second is
false**: `herd_proto::Asked` carries `needs` and `leaves`, added for M11, and
neither `coalesce` nor `dedup_key`; `herdctl` has no `--coalesce` flag;
neither field is anywhere on the wire. It is recorded as the orchestrator's
own because the sentence was repeated from one of its briefs.

**The clause that the second submission *"says so"* is met by a detached
submit returning the existing id, and no sentence is owed.** Measured: the hub
sends `JobAccepted` and *then* a `JobNote` carrying the join sentence, but
`submit` returns on `JobAccepted` and closes the socket, so the note reaches a
connection that is gone. The documented wrapper is `id=$(herdctl submit …)`
then `herdctl wait "$id"` — **the id is the answer.** **The silence is
deliberately not asserted in a test**: asserting a behaviour nobody chose
would freeze it, which is why `e42-producer-submit` did not.

**What this does NOT decide.** **Whether the coalesce half is built before
v1.0.0 is open and Alexa's.** It needs `herd-proto`: a `serde(default)` field
on `Asked`, which is additive and which **D229** leaves `PROTOCOL_VERSION`
alone for, **and** a new job-outcome variant that travels inside a frame a
requester decodes — not literally one of D229's two named unrecoverable forms
(a new `ToHub` variant, a `FromHub` variant a requester sees) but the same
hazard for an older `herdctl`. **D75** fixed the shape, `superseded { by }` at
exit 125, and not the timing.
```

**Two `board.md` rows, in full, for the *Owed* table's eight columns
(`| ID | Item | Origin | Owner | State | Next action | Depends on |
Evidence |`). No literal pipe in any cell.**

```markdown
| ORCH-NEXT | **E-42 recounted `planned` → `part`, and two board cells still count it as having nothing.** The row is two rules: dedup — joining — is on `main` and asserted (`herd-hub/src/job.rs:817`, `herd-hub/src/store.rs:2234`, `herdctl/tests/declared_dedup.rs`), coalesce — discarding — exists nowhere (D75). Done on `e42-recount-part`: `docs/coverage.md`'s E-42 row and its four tallies, and `docs/road-to-v1.md`'s step 3, B1 and B5. Re-derived from the rows, **36 + 5 + 1 + 2 = 44**, headline figures unmoved. **Two cells of this board were left to the merger because an agent does not edit the board:** `board.md:249` (ORCH-206's done row) ends *"E-12, E-14 and E-42 are gated with nothing on `main`"*, and `board.md:576` (batch 17's recount, a dated record) reads *"**4** part (E-12, E-16, E-17, E-37), **2** none (E-14, E-42) … 36 + 4 + 2 + 2 = 44"*. Both are **historical records of what a past branch found**, so amend each with a dated clause rather than rewriting it: at `:249` append **"Recounted 2026-10-30: E-42 is `part`, its dedup half built and asserted; E-14 alone is gated with nothing on `main`"**, and at `:576` append **"Superseded 2026-10-30 on `e42-recount-part`: E-42 moves to `part`, so the tally is 36 + 5 + 1 + 2 = 44; the 4 / 2 above is batch 17's reading and is kept as what that branch measured"** | the orchestrator's ruling, 2026-10-30, after `e42-producer-submit` (`c47a72a`) measured the row | herd-step, then herd-merger | done, board cells owed | `herd-merger` amends `board.md:249` and `:576` with the two dated clauses above when it lands this branch | the D-NEXT entry this handoff drafts | `history/2026-10-30-e42-recount-part.md`; `docs/coverage.md` E-42 row; `docs/road-to-v1.md` step 3, B1, B5 |
| ORCH-NEXT | **`cut-v1.0.0`'s release body needs the E-42 recount before the tag.** The branch `cut-v1.0.0` (`f3e9df0`) carries a scope table reading *"Nothing of it in this release, 1, E-42"*, and E-42 is now `part`: its dedup half is built and asserted, its coalesce half absent. The table is wrong as it stands. The body must also still name **E-49 and E-50 as not met**, which is the consumer's own condition for the waiver (**D248**). The branch is unlanded and unsigned and **was not touched** by `e42-recount-part` | `e42-recount-part`, 2026-10-30, flagged on the orchestrator's instruction | the orchestrator, then whoever cuts v1.0.0 | owed, blocks the tag | recount E-42 in the scope table of `cut-v1.0.0`'s release body before the tag is pushed; the notes' wording stays Alexa's (D210) | the D-NEXT entry this handoff drafts; **D212**, **D210** | `history/2026-10-30-e42-recount-part.md`; branch `cut-v1.0.0` at `f3e9df0` |
```

## Decisions needed

1. **Does the orchestrator stand by `E-14 withdrawn`?** —
   *Recommendation:* no. The tree refutes it and the file now says
   `planned`; if E-14 is to be retired it needs a consumer answer of
   D249's shape, asked and recorded like D248's, and until then no
   document may count it among the rows the consumer released us from.
   *Fallback:* if a consumer answer already exists outside the tree,
   brief an agent with it and the two counts become 0 / 3 in one edit.
2. **Is the coalesce half of E-42 inside v1.0.0?** — **open, Alexa's,
   and deliberately left open in both edited files.** Not a question
   this agent asks; recorded so it is not mistaken for decided. It needs
   an additive `serde(default)` field on `Asked` and a new job-outcome
   variant an older `herdctl` meets inside a frame it decodes.

## Resume point

`git fetch && git log --oneline origin/e42-recount-part -1`, then land
this branch in the next batch. On landing, `herd-merger`: paste the
`D-NEXT` entry above into `docs/decisions.md` with the next free number,
add the two `board.md` rows, and apply the two dated clauses to
`board.md:249` and `:576`. The two follow-on tasks, each its own brief:
**(a)** add a new advancement row to `docs/road-to-v1.md` at a named
sha, re-reading all ten blocker statements, per that file's *Adding the
next row*; **(b)** recount E-42 in `cut-v1.0.0`'s release body before
the tag.

## Processes

**None.** No process was started, none was signalled, and **PID 662711,
the queue's runner, was not touched**. No build, no suite, no bench: a
prose-only task needs no toolchain, so `./scripts/bench-preflight.sh`
was not run and no `CARGO_TARGET_DIR` was created. **No `target/` to
delete** — nothing was compiled on this branch.
