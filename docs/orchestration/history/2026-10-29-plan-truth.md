# plan-truth — handoff 2026-10-29

- **Item:** ORCH-152 (the four `plan.md` errors), ORCH-201 (`known_gaps`),
  and `coverage.md`'s stale State column
- **Agent:** herd-step
- **Branch / commit:** `plan-truth` / see the push, based on origin/main
  b37b2a4
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Three files, prose plus one shell line: `docs/coverage.md`,
`docs/plan.md`, `scripts/check-docs.sh`. Nothing else is touched.

### `docs/coverage.md` — the State column is true again, and counted

Each of the planner's three flagged rows was verified against the landed
tree before it was changed. All three had moved, and two more had moved
that the brief did not name.

- **E-35** `planned` → **`merged`**. `herd-hub/src/status.rs` (36 KB),
  `herd-hub/tests/status.rs`, `pub const STATUS_VERSION: u32 = 1`,
  `status.json` in the state root, `herd-hub status` as the text screen.
  Landed in batch 8 from `v18-status`; recorded as D215.
- **E-37** `planned` → **`part`**. `herd-hub/src/notify.rs` (44 KB) and
  `herd-hub/tests/events.rs` exist and emit four of the list's
  transitions; two of its five items have no transition in today's hub.
  Landed from `v20-events`; D216.
- **E-41** `planned` → **`merged`**. `herdctl/src/moved.rs` (39 KB),
  `herdctl/tests/if_moved.rs`, `docs/scheduling.md`. Landed from
  `v22-schedule`; D217. The cell says what `merged` means here: Herd's
  side is on `main` and nothing closeable is left, with the
  no-submission skip a declared deviation.
- **E-16** `planned` → **`part`**, which the brief did not ask for. Its
  own note already said the hub-side budget was *"built in M11
  (D152)"*, so `planned` contradicted the cell beside it.
  `--output-budget` and `output_budget_bytes` are in
  `herd-hub/src/main.rs` and `status.rs`.
- **E-49, E-50** `planned` → **`unmet by choice`**, which is the word
  D206 itself asks for: *"marked unmet by choice and not unplaced —
  `check-docs.sh` would accept either, and a reader must not have to
  guess which one it is."*
- **E-40**'s note no longer says the ages are *"surfaced in the status
  document in M13"* (future); they are surfaced now.

The header now carries the counted figure and the arithmetic, dated, and
the legend gained `unmet by choice` and a widened `part`. The *State of
play* section is re-dated and re-counted, the fifth caution carries
`LAYOUT_VERSION` and D199's triple, and the closing sentence — which
claimed *"no must is deferred or partial"* — is replaced, because two
musts are now partial and two are deferred by choice.

### `docs/plan.md` — seventeen corrections, four of them load-bearing

Each wrong claim is **quoted where a reader meets it** rather than
deleted, so it cannot be carried forward from a stale copy. A dated note
above *Milestones* says the document was corrected and that this is not
the v1.0.0 roadmap, which is still ORCH-155's.

The four that change what gets built: M14's hub-side bare repository
(D207), M12's three-term placement order against
`herd-hub/src/matcher.rs`, M15d's empty justification, and M11's pack
written under `{out}` (ORCH-153). The other thirteen are listed in the
agent's report with the wrong line beside its replacement.

### `scripts/check-docs.sh` — `known_gaps="143 154 170"`

D170 was held by PR #67, which `docs/decisions.md:9490` records as
**closed unmerged as obsolete**, so the number is spent and the gap is
permanent (D148: a number is an identifier, gaps stay). The note that
printed on every green run is gone; five comment lines say where D170
went. Nothing is renumbered.

## Not done, and why

- **`docs/decisions.md` and `docs/orchestration/board.md`** — not mine
  (only `herd-merger` writes them). D205-versus-D206 is drafted as a
  question in the agent's report, not written into the board.
- **`docs/operating.md`, `docs/remediation/13-crash-point-enumeration.md`,
  `docs/remediation/README.md`** — another agent's tonight. Untouched.
- **The v1.0.0 roadmap (ORCH-155)** — not attempted. This task makes the
  existing documents true; it does not carry the 27-step roadmap into
  the repository, which ORCH-155 says rewrites `plan.md` and needs a
  reviewer pass.
- **ORCH-153's shape is not decided**, only written down. `plan.md` now
  says the pack path is unspecified and names the two candidates with
  the argument for the second; choosing is a decision, not an edit.
- **The four Rust gates were NOT RUN locally.** This change is prose plus
  one `known_gaps` string in a shell script; no crate, test or build
  file is touched. They are covered by the batch's own
  `./scripts/gate.sh` run at integration, which is `herd-merger`'s.

## Gates

Runs made **before** this file was committed, verbatim:

```
$ ./scripts/check-docs.sh
docs ok
(exit 0)
```

```
$ ./scripts/check-orchestration.sh
check-orchestration: 285 passed, 0 failed
(exit 0)
```

The **last** runs, on the tree with this handoff committed, are
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`; their
output goes to the agent's report and to
`scratchpad/orch-152-plan-truth/check-docs-final.txt` and
`scratchpad/orch-152-plan-truth/check-orch-final.log`.

Rust gates: **NOT RUN** locally (prose plus one shell line); a gate that
did not run is reported as not run, never as passed.

Skips: **no skip**. Neither docs gate has a skip mechanism and no
`HERD-SKIPPED` line was produced or copied.

## Risks and findings

1. **D205 and D206 cannot both be true, and `coverage.md` now says so
   without resolving it.** The arithmetic is in the header. The question
   is in the agent's report for the orchestrator to carry to Alexa.
2. **D215 mis-cites `docs/plan.md`.** D215's *Context* says *"`docs/plan.md`
   reasoned that E-35's machine-readable status should be a protocol
   frame a requester asks for"*, and its *Alternative rejected* is *"the
   frame the plan named"*. `plan.md`'s M13 never said frame: it said
   *"One status document, JSON with a format version (Q7) … refreshed on
   change"*, which is what was built. `grep` finds no "frame" in M13.
   The decision is right; its account of what the plan said is wrong.
   `decisions.md` is append-only and not mine, so it is left alone.
3. **D205 says the record half *"paid the `RECORD_VERSION` 2 break"*.**
   `herd-hub/src/job.rs:52` is `pub const RECORD_VERSION: u32 = 1`, and
   D199 lists `RECORD_VERSION` 1 → 2 as still owed. D205's parenthesis
   is loose; D199 and the code agree with each other. Left alone.
4. **`plan.md`'s "Gated on chatOnS" list is not the same list as
   `coverage.md`'s.** `plan.md` names E-02, E-11, E-12, E-14, E-38,
   E-41, E-42, E-53, E-54; `coverage.md`'s M18 line names E-02, E-11,
   E-12, E-14, E-41, E-42, and the rows whose notes actually carry a
   gated or manual marker are E-02, E-11, E-12, E-14, E-20, E-30, E-41,
   E-49. Three lists, three answers. Not reconciled here: which is
   authoritative is a judgement about the consumer's acceptance, not a
   typo.
5. **`herd-hub` PID 3046930 on `127.0.0.1:18461` was left alone**, as
   the brief required. No process of any other agent was signalled.

## Decisions needed

**Question for Alexa (drafted; the orchestrator carries it).** D205 puts
M11's pack half inside v1.0.0 and states that this makes it 44 of 44 of
the first consumer's musts, with no must left outside the stable
release. D206, the same day, declares E-49 and E-50 unmet by choice.
Those two are musts, so the two decisions together give a ceiling of
**42 of 44**, not 44 of 44. Which statement is the one that moves: is
v1.0.0 *42 of 44, with E-49 and E-50 named as the two it does not meet*,
or does M17 come back inside so that 44 of 44 is reachable?

**Recommendation: 42 of 44, said plainly.** It is the only one of the two
that is true tonight, it costs no engineering, and D206 already accepted
the visible half of the price — that the stability promise covers a
platform the software has never run on. Pulling M17 back in buys the
rounder number at the cost of an unstarted milestone with three known
unstable tests (D149) and a manual 30-minute walk on hardware nobody
here may touch (D161).

**Fallback if he wants 44 of 44:** M17 re-enters scope, D206 is
superseded by a new entry, and the release date moves by whatever M17
costs — which is unmeasured, and must be measured before the date is
promised rather than after.

## Resume point

For `herd-merger`: integrate branch `plan-truth` into the next batch.
For the orchestrator: put the D205/D206 question to Alexa at 09:00 and,
on his answer, brief one step to set `coverage.md`'s header figure to
match it. ORCH-153's shape (pack outside `{out}`, or a fifth exclusion)
must be decided before the first pack brief, and `plan.md` now refuses
to name a pack path until it is.

## Processes

None started, none ended. No compile, no suite, no `cargo`: the change
needs no build, so `bench-preflight.sh` was not applicable and no
`CARGO_TARGET_DIR` was created. Nothing to delete — this branch has no
`target/`. PID 3046930 (`herd-hub`, `127.0.0.1:18461`) was left
untouched.
