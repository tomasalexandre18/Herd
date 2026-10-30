# docs-pack-figures — handoff 2026-10-30

- **Item:** the five version figures made false by M11's pack half
- **Agent:** herd-step
- **Branch / commit:** `docs-pack-figures` / see the push, based on
  `main` `351c826`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

**Ordering, and it is the one thing that can make this branch wrong:**
every figure here is true of a tree that contains `pack-variants` and
`pack-registry`. This branch must land **in the same batch as those two,
or after them**. Landed before them, it says 3, 2 and 2 while `main`
says 2, 1 and 1 — the same defect it exists to close, with the sign
flipped.

## Done

Four files, prose only. No `.rs` file, no `cargo`, no build, no test.

### What the tree says, measured rather than assumed

Read off the branches, not off the brief:

| Constant | `origin/main` | what lands | verified at |
|---|---|---|---|
| `PROTOCOL_VERSION` | 2 | **3** | `herd-proto/src/lib.rs`, `pack-variants` (`:160`) and `pack-store`/`pack-registry` (`:147`) |
| `RECORD_VERSION` | 1 | **2** | `herd-hub/src/job.rs:72`, `pack-registry` |
| `LAYOUT_VERSION` | 1 | **2** | `herd-hub/src/store.rs:99`, `pack-registry` |

`job.rs` is touched by all four pack branches but the constant sits at
line 52 on `pack-proto`, `pack-variants` and `pack-store` and at 72 only
on `pack-registry`, so **72 survives the merge**. `store.rs` is touched
by `pack-registry` alone, so **99 and every other `store.rs` line below
hold**. `herd-proto/src/lib.rs` is the exception: `pack-variants` puts
`PROTOCOL_VERSION` at 160 and `pack-store` at 147, so **no line number
for that file can be verified for the merged tree** and none is written
here — every `lib.rs` citation this branch touches now names the
constant or the function instead of a line.

`rg`-equivalent count of `const [A-Z_]*VERSION` declarations: **eleven on
`main` and eleven on each pack branch**, so §3.4's "eleven" and the
one-row-per-constant structure are both still right and were kept.

### The five, old beside new

**1. `docs/operating.md`, *When a version refusal names two figures*
(was `:555-563`, now `:559-575`).** Old: *"the release body prints the
two that matter"* above a table reading **2 / 1 / 1**. New: *"prints
**all three**"*, a table reading **3 / 2 / 2** with a fourth column
giving each constant's declaration site, and a paragraph saying those
are `main`'s numbers and no release's. *"All three"* is measured:
`pack-registry` rewrites `.github/workflows/release.yml` to read and
echo `LAYOUT_VERSION` beside the other two (`:183-219`, `:457`).

**2. `docs/operating.md` layout-refusal quotation (was `:576-578`, now
`:594-597`) — NOT TOUCHED, as instructed.** Byte-compared against
`351c826`: `cmp` says IDENTICAL. It still reads

```
the state root /var/lib/herd has layout version 1, and this hub writes
version 2
```

and it is a correct prefix of `layout_version_mismatch_reason`
(`pack-registry`, `store.rs:3345`), which formats *"the state root
{root} has layout version {found}, and this hub writes version
{LAYOUT_VERSION}. Nothing converts it: …"*. It is now literally what a
v0.4.1 root gets. The table above it is what was edited.

**3. `docs/architecture.md` §3.4 (was `:345-346`, now `:357-359`) plus
the frozen claim (was in §3.4's tail, now `:414-425`).** Rows: values
**2 → 3**, **1 → 2**, **1 → 2**, each stated as *"N on `main`, M in
every release to date"*; `job.rs:52 → 72`, `store.rs:60 → 99`. Frozen
claim, old: *"`RECORD_VERSION` is frozen at 1 since M16. It reached 6
while nothing was deployed; a bump now costs the operator's whole state
root … and the hub says so by not starting, never by guessing."* New:
*"**`RECORD_VERSION` is not frozen any more.** It was 1 from M16 until
M11's pack half moved it to **2** (D199) …"*, keeping the "reached 6"
history, and correcting *which* number stops a deployment — see finding
F1. Eleven rows, one per constant, unchanged in structure.

**4. `docs/handover.md:65`.** Old: *"`PROTOCOL_VERSION` 2"* and
*"(`version_mismatch_reason`, `herd-proto/src/lib.rs:2485`)"*. New:
*"`PROTOCOL_VERSION` **3** on `main` and 2 in every release to date"*,
and the citation is now `version_mismatch_reason` **in**
`herd-proto/src/lib.rs` with no line, because that line is 2485 on
`main` and 2922 on `pack-variants` and the merged value is not
determinable here.

**5. `docs/disk-policy.md:769`.** Old: *"`objects/` **must not move
`LAYOUT_VERSION`**, which is 1 `[C, store.rs:59]`"*. New: *"which is 2
`[C, store.rs:99]` since M11's pack half moved it there (D199)"*, plus
two sentences saying the figure is the layout the work starts from and
not a constant of the design. Note the citation was stale **twice over**:
`store.rs:59` was wrong against `main` too, where the constant is at
`:60`.

### ORCH-171's acceptance criterion, `docs/disk-policy.md:1462` (now `:1469`)

Old: *"`LAYOUT_VERSION` unchanged at 1"*. New, in full:

> and **this work moves `LAYOUT_VERSION` by nothing** — it must stand,
> when the work is done, at exactly the number it stood at when the work
> began, whatever that number is (2 at the time of writing,
> `[C, store.rs:99]`). Read it as the requirement and not as the figure:
> the figure is `main`'s and moves without asking ORCH-171, and a
> criterion written as *"unchanged at 1"* was already unsatisfiable once

**No decision was needed and none is asked for.** The criterion's
purpose is unambiguous from the argument it sits on — §2.9's bullet at
`:767` says hard links are transparent to every reader, so `objects/`
needs no layout move — so restating it as *the work adds no move of its
own* preserves the meaning exactly rather than guessing at it. Writing
*"unchanged at 2"* would have been the trap again: it keeps the meaning
only while 2 is the layout, and the number just proved it moves. The
rewording is self-maintaining; the figure in it is dated and cited so a
reader can tell a stale figure from the requirement.

### Other sentences found false, and fixed (all four files are mine)

- **`docs/operating.md:670-690`**, *What this means for you, concretely,
  today*. Old: *"Nothing shipped moves `RECORD_VERSION` or
  `LAYOUT_VERSION`. All three numbers are 2, 1 and 1 in v0.4.0 …"*. The
  first sentence survives in substance (no release moves them) but read
  as a claim about the tree; v0.4.1 was missing; and `main` now holds
  3/2/2 with nothing said about it. New: the shipped claim narrowed to
  *"No release you can install …"*, v0.4.1 named beside v0.4.0, and a
  second paragraph stating what `main` holds. See **D1** below for the
  consequence it names.
- **`docs/operating.md:615-620`** (new paragraph). The section said a
  record-number difference with a matching layout *"starts, and serves
  an empty queue"*. True as a conditional, but between v0.4.1 and `main`
  **both** numbers moved, so the layout refuses the root first and the
  quarantine path never runs. Said so, in one paragraph, without
  touching the conditional.
- **`docs/operating.md:885`** (*Where to go next*). Old: *"the two
  numbers in its body"*. New: *"the version numbers in its body"* —
  deliberately not *"three"*, because the row points at
  `deploy/release-notice.md`, which is not mine, is quoted in full on two
  published release pages, and still documents two.
- **`docs/architecture.md:79`** (mermaid node). *"PROTOCOL_VERSION 2"* →
  *"PROTOCOL_VERSION 3"*. **Not in the brief's list of five** — a sixth
  figure, in a diagram rather than prose.
- **`docs/architecture.md:364-365`.** `INDEX_VERSION` `store.rs:64 → 103`
  and its refusal site `store.rs:1240 → 1315`; `READINGS_VERSION`
  `store.rs:119 → 158` and `store.rs:3146-3151 → 3226-3231`. Values
  unchanged at 1; only the pointers rotted, all four verified on
  `pack-registry`.
- **`docs/architecture.md:366-367`.** `OUTPUT_FORMAT_VERSION` and
  `VERDICT_FORMAT_VERSION` cited `herd-proto/src/lib.rs:2350` and
  `:2380`; both move on every pack branch (2770/2800 on `pack-variants`,
  2396/2426 on `pack-store` and `pack-registry`) and the merged line is
  not determinable, so both now name the constant and no line. Their
  values are still 1 and *"frozen at 1 since M16"* is still true of
  `OUTPUT_FORMAT_VERSION`.
- **`docs/architecture.md:406-412`.** *"That is true today of all six
  numbers that refuse anything — no migration tool exists for any of
  them"* stays true, but was written as a hypothesis. Added: three of
  the six have now moved without that tool, so the absence of a
  migration path is the state of `main`.
- **`docs/handover.md:67`.** *"a `herd-hub migrate` is UNBUILT and is the
  only way a `RECORD_VERSION` move will ever be theirs to run"* — still
  true, and now the live case: both state-root numbers moved with no
  such tool. Said so in the cell. *"**five** version numbers"* and
  *"the whole list of eleven"* were checked and are both still right.

### What the tree refuted in the brief's list

- **The protocol figure was *not* already wrong before tonight.** The
  brief says so twice — for `operating.md:555-563` and for
  `handover.md:65`. `origin/main` and tag `v0.4.1` both carry
  `pub const PROTOCOL_VERSION: u32 = 2;` (`herd-proto/src/lib.rs:114`),
  and `history/2026-10-29-release-v0.4.1-checklist.md:27` records 2 as
  both expected and found. Both documents were **correct until
  `pack-proto` lands**; they are wrong only afterwards. The second
  reader's refinement was itself wrong on this point. Nothing changes
  about the fix — both figures had to move to 3 — but the reason is the
  landing, not pre-existing rot.
- **`handover.md:65` *did* carry a stale-on-landing pointer**, which the
  brief did not mention: `herd-proto/src/lib.rs:2485` for
  `version_mismatch_reason`. Right on `main`, 2922 on `pack-variants`.
- **The record-version quotation has the same accidental-accuracy
  property as the layout one.** `docs/operating.md:605-608` illustrates
  *"has record version 1, and this hub reads version 2"*; it is a
  correct prefix of `record_version_mismatch_reason` (`pack-registry`,
  `herd-hub/src/job.rs:102-120`) and is now literally what a v0.4.1
  record gets. **Left untouched**, for the same reason as `:594-597`.
  The brief flagged only the layout one.

## Not done, and why

- **`.github/workflows/release-dev.yml:17`** and
  **`deploy/release-notice.md:48`** — left untouched and unread beyond
  confirming the brief's description. `.github/` is `herd-merger`'s; the
  notice is quoted in full on two published release pages. Both still
  say the body carries two numbers where the landing tree reads three.
  Named, not fixed, as instructed.
- **`docs/coverage.md`, `docs/plan.md`, `docs/status.md`,
  `docs/road-to-v1.md`, `docs/decisions.md`,
  `docs/orchestration/board.md`, `scripts/*`, `.github/*`, `deploy/*`,
  `herd-*/`** — not touched. Not read for version figures either,
  except `decisions.md` read-only for D199's text.
- **`docs/handover.md:67`'s *"the rule that an unreadable version refuses
  to start"*** — left as it stands. It is loose rather than false
  (`INDEX_VERSION` rebuilds, `READINGS_VERSION` reads again,
  `RECORD_VERSION` quarantines since D218), it was not made false by
  tonight's three moves, and the cell already carries one correction
  from this task. Named here instead of widening the edit. See **F1**.
- **The four Rust gates were NOT RUN.** Prose only; no `.rs` file is in
  the diff. What covers them is the batch's own `./scripts/gate.sh` run
  at integration, which is `herd-merger`'s and not this agent's.

## Gates

Baseline, measured in the shared checkout `/mnt/ssd_980/herd-project` at
`351c826` **before** any edit:

```
docs ok
```

```
check-orchestration: 291 passed, 0 failed
```

After all edits, on this branch:

```
docs ok
```

```
check-orchestration: 291 passed, 0 failed
```

**Delta: none, in either check.** No case changed state, so there is no
case to account for.

Mojibake detector, byte-level perl on raw bytes
(`\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]` and
`\xc3\x82\xc2[\x80-\xbf]`), armed before it was trusted:

```
clean synthetic fixture (real em dash, nbsp, U+2264): 0 hits, exit 0
double-encoded em dash fixture:                       1 hit,  exit 1
double-encoded nbsp fixture:                          1 hit,  exit 1
clean copy of docs/operating.md:                      0 hits, exit 0
same file, every em dash double-encoded on purpose:  45 hits, exit 1
```

On the four edited files, before and after: **0 hits, exit 0.** No
`sed`/`perl` text-mode rewriting was used on any of them; every change
was made with the file tools.

The last run is the one about to happen on the committed tree:
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh` in
`<scratchpad>/wt-docs-pack-figures`, after this file is committed and
before the push. Its verbatim output is in the agent's report, not in
this tree.

**No skip.** `check-orchestration.sh` printed no `HERD-SKIPPED` line and
no skipped case; the Rust gates are reported above as **not run**, which
is not a pass and not a skip.

## Risks and findings

- **F0 — ordering.** Restated because it is the whole risk: this branch
  is true of a tree holding `pack-variants` and `pack-registry`. Landing
  it alone inverts the defect. If the batch is split, this branch goes
  with or after the pack branches.
- **F1 — `architecture.md` §3.4's `RECORD_VERSION` row was already
  false, by D218 and not by tonight.** It said a wrong record number
  means *"the hub **refuses to start** on that state root"*. Since D218
  (2026-10-29) the record is quarantined and the hub comes up —
  `docs/operating.md:764` says in so many words that *"this page
  promised the opposite"*, of itself, and `architecture.md` was never
  corrected to match. Verified in the landing code: `store.rs:1548-1551`
  returns `Stored::Quarantined`. **Fixed here**, because leaving *refuses
  to start* beside a number this branch just moved to 2 would mislead an
  operator about which failure they are about to get. Same defect fixed
  in the three-kinds list at `:373` (*"`RECORD_VERSION` and
  `LAYOUT_VERSION` stop the hub from starting"*). Flagged because it is
  outside the brief's stated cause.
- **F2 — the `events.jsonl` arm still errors.** `store.rs:2806-2810`
  turns a version-mismatched event line into a `StoreError` rather than
  a quarantine. The §3.4 row now names both arms and claims nothing
  about what the event arm does to startup, because that was not
  measured here. Worth a reader's attention if the record move produces
  a surprise.
- **F3 — `lib.rs` line citations cannot be made true by this branch.**
  `pack-variants` and `pack-store` disagree on where `PROTOCOL_VERSION`
  sits (160 vs 147) and both are green, so the merged line is whatever
  the integration produces. Every `herd-proto/src/lib.rs` citation this
  branch touched is now line-free. Others elsewhere in the corpus were
  not surveyed.
- **F4 — the figures carry a date and a provenance, not a promise.** The
  three tables now say *"on `main`"* and name the declaring constant.
  They will be stale again at the next move. That is the intended
  failure mode: a reader can tell a stale figure from a false claim, and
  §3.4 already tells them to re-derive.

## Drafted for `herd-merger`

The brief asked for the drafted decision and board row **in full, in
this file** (D226). Here they are in full; nothing is deferred to the
agent's report.

### Drafted `docs/decisions.md` entry

```markdown
## 2026-10-30 — D-NEXT A version figure in prose carries its constant, and
one in an acceptance criterion carries its requirement instead

**Context.** M11's pack half moved all three of D199's numbers —
`PROTOCOL_VERSION` to 3, `RECORD_VERSION` and `LAYOUT_VERSION` to 2 — and
`LAYOUT_VERSION` moved for the first time in the project's history. Six
figures in four shipped documents went false in one commit, one citation
(`docs/disk-policy.md`, `[C, store.rs:59]`) had already been stale against
`main`, and one of the six was not prose at all: ORCH-171's acceptance
criterion read *"`LAYOUT_VERSION` unchanged at 1"*, which the move made
**unsatisfiable**. A stale figure in prose is read by a person. A stale
figure in an acceptance criterion is read as an instruction, and somebody
picking ORCH-171 up would have tried to satisfy it by undoing a landed
version move.

**Decision.**

1. **Every version figure written in `docs/` names the constant it came
   from.** A line number is given only when it is verifiable for the tree
   the text lands in; where two green branches disagree on it, the
   constant or the function is named and no line is written. A figure
   with no provenance is a defect, not a style choice.
2. **A figure about a moving number says which tree it is about.** The
   form is *"N on `main`, M in every release to date"*, so that a reader
   comparing two releases is not reading `main`'s number as a release's.
3. **An acceptance criterion never states a figure it means as a
   constraint.** It states the constraint: *this work moves
   `LAYOUT_VERSION` by nothing — it must stand, when the work is done, at
   exactly the number it stood at when the work began*. A figure may
   appear beside it, dated and cited, as information. This is the rule
   D226 is to drafted decisions: the artefact an agent will act on has to
   be written so that the act is still right after the tree moves.
4. **A prose correction that is only an illustration is left alone when
   the tree makes it true.** `docs/operating.md`'s two refusal
   quotations were written as illustrations when both numbers were 1;
   they are now literally what a v0.4.1 root and a v0.4.1 record get from
   a `main` build, and both are correct prefixes of the real sentences.
   Neither was touched.

**Evidence.** `docs-pack-figures`, 2026-10-30, four files and no code;
`herd-proto/src/lib.rs`, `herd-hub/src/job.rs:72`,
`herd-hub/src/store.rs:99` on `pack-variants` and `pack-registry`;
`herd-hub/src/store.rs:3345` and `herd-hub/src/job.rs:102` for the two
quotations; D199, D218, D226, ORCH-156, ORCH-171.
```

### Drafted `board.md` rows

Two rows. Ids are the merger's to allocate; `ORCH-A` and `ORCH-B` are
placeholders for the first two free ids, in this order.

```markdown
| ORCH-A | **The release body reads three numbers and two documents say two.** `.github/workflows/release.yml` reads and echoes `LAYOUT_VERSION` beside `PROTOCOL_VERSION` and `RECORD_VERSION` since M11's pack half (`:183-219`, `:457`). Two short sentences still say two: `.github/workflows/release-dev.yml:17` (*"the same `RECORD_VERSION` and `PROTOCOL_VERSION` read out of the source"*) and `deploy/release-notice.md:48` (*"## The two numbers in the release body"*). The first is `herd-merger`'s file; the second is quoted in full on two published release pages, so changing it is a question about what those pages say, not a docs fix | herd-merger for `.github/`; Alexa for the notice | open | the pack batch landing | `docs/` |
| ORCH-B | **D199's ordering constraint and `main` no longer fit together.** D199 binds *"the ordinary release before v1.0.0"* to ship at layout 1 and record 1, for two measured reasons (the owed ORCH-145 reading needs a hub that can open their existing root; a record-2 build on a version-1 root exits cleanly with `read = 0` and looks like an answer). `main` carries record 2 and layout 2 once the pack batch lands, so that release cannot be cut from `main`'s tip. Decide where it is cut from, or whether it still happens. `docs/operating.md` now states the fact and names no resolution | Alexa | waiting | the pack batch landing; ORCH-145 | `docs/decisions.md` (D199 stands as written) |
```

## Decisions needed

1. **Where is the ordinary release before v1.0.0 cut from, now that
   `main` carries the triple?** — *Recommendation:* ask Alexa, as
   drafted row ORCH-B, since it is a question about what ships and in
   which order. D199's two reasons are still sound and still unmet: the
   ORCH-145 reading is owed and needs a hub that opens their existing
   root. *Fallback:* cut it from `v0.4.1`'s commit plus whatever
   non-version fixes cherry-pick cleanly, which keeps D199 literally
   satisfied at the cost of not shipping the batch. This branch states
   the fact in `docs/operating.md` and proposes nothing.
2. **Does `LAYOUT_VERSION` belong on the release body's
   machine-readable lines?** — already answered in the tree:
   `pack-registry` put it there. Raised only because `pack-registry`'s
   own handoff lists it as an open decision (*Decisions needed*, item 2)
   while its `release.yml` already does it. *Recommendation:* the
   merger confirms the tree is the answer and closes that item.
   *Fallback:* none needed; nothing here depends on it.

## Resume point

`herd-merger`: take `docs-pack-figures` into the pack batch, **in the
same batch as `pack-variants` and `pack-registry` or after them, never
before**. Then allocate the number for the `D-NEXT` entry above and the
two board ids, and close `pack-registry`'s *Decisions needed* item 2
against its own `release.yml`.

## Processes

- **PIDs started and ended:** none. No process was backgrounded, nothing
  was killed, no `cargo` ran. Only `git`, `sed`, `cat`, `perl` on
  scratchpad copies, and the two check scripts.
- **Own `target/`:** none to delete — no build ran, `CARGO_TARGET_DIR`
  was never set and `./scripts/bench-preflight.sh` was not needed, there
  being no heavy phase. The worktree
  `<scratchpad>/wt-docs-pack-figures` holds no build tree. The first
  consumer's hub and runner were untouched; this agent stayed outside
  the heavy-agent ceiling of two by touching no crate.
