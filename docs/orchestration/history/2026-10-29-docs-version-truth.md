# docs-version-truth — handoff 2026-10-29

- **Item:** none allocated. The brief names no `ORCH-nnn` and the board is
  not this agent's to edit; the orchestrator allocates one.
- **Agent:** herd-step
- **Branch / commit:** `docs-version-truth` / this commit, based on
  `origin/main` b37b2a4
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`)

## Done

**`docs/architecture.md` §3.4 rewritten: eleven constants, not six.**
Every one was verified against the tree before it was written down, with
`rg -n 'pub const [A-Z_]*VERSION|^const [A-Z_]*VERSION' --glob '*.rs'`
and again with a wider pattern that admits any visibility and any indent.
`rg` is ripgrep 15.2.0 from the flake's PATH; `grep` on this host is
ugrep 7.8.4 and was not used for the count.

| Constant | Where | Value |
|---|---|---|
| `PROTOCOL_VERSION` | `herd-proto/src/lib.rs:114` | 2 |
| `OUTPUT_FORMAT_VERSION` | `herd-proto/src/lib.rs:2350` | 1 |
| `VERDICT_FORMAT_VERSION` | `herd-proto/src/lib.rs:2380` | 1 |
| `RECORD_VERSION` | `herd-hub/src/job.rs:52` | 1 |
| `LAYOUT_VERSION` | `herd-hub/src/store.rs:60` | 1 |
| `INDEX_VERSION` | `herd-hub/src/store.rs:64` | 1 |
| `READINGS_VERSION` | `herd-hub/src/store.rs:119` | 1 |
| `ROSTER_VERSION` | `herd-hub/src/roster.rs:44` | 1 |
| `STATUS_VERSION` | `herd-hub/src/status.rs:78` | 1 |
| `EVENT_FORMAT_VERSION` | `herd-hub/src/notify.rs:73` | 1 |
| `STATE_FORMAT_VERSION` | `herdctl/src/moved.rs:47` | 1 |

The table in §3.4 now carries one row per constant with five columns —
value, where it lives and what it fronts, **what moves it**, and what a
mismatch does **and whether anything can migrate** — each written from
the constant's own doc comment and its refusal site, not from a summary.
The build version (`version_gap`, workspace 0.4.1) keeps its row and is
labelled *not one of the eleven*: it is no constant at all but
`CARGO_PKG_VERSION` through `hub_version()`, `runner_version()` and
`requester_version()`.

Three paragraphs follow the table for the three numbers that matter in
their own right — `EVENT_FORMAT_VERSION` (the only one whose reader is
the consumer's own code, `notify.rs:66-72`), `ROSTER_VERSION` (refused at
`roster.rs:135`, and an unreadable roster refuses every runner), and
`STATUS_VERSION` (whose refusal sentence contains the words *"There is no
migration"*, `status.rs:406-412`) — and a three-way classification that
is the checklist the old text could not produce.

**`docs/handover.md` made consistent.** Two rows:
- *Hub, the state root* said "five version numbers (`RECORD_VERSION`,
  `LAYOUT_VERSION`, …)" with the list elided. It now names all five by
  name, says that the hub's sixth is `STATUS_VERSION` in the *Status and
  events* row, and points at §3.4 as the whole list of eleven.
- *Protocol* cited `herd-proto/src/lib.rs:2144` for "the refusal sentence
  naming both versions". Line 2144 is a doc comment about the `ran on:`
  line. The sentence is `version_mismatch_reason` at
  `herd-proto/src/lib.rs:2485`, and the citation now says so.
- *Status and events* said "UNBUILT: M13's JSON status document and
  events (E-35, E-37)". Batch 8 landed both (D215, D216). Corrected,
  because §3.4 is now pointed at it for `STATUS_VERSION`, and it keeps
  E-37's fifth event and the use-against-budget figure marked UNBUILT.
  Consistent with `origin/plan-truth`, which marks E-35 merged and E-37
  part.

**`docs/orchestration/templates/landing.md` amended.** The template said
the record is "written twice, identically". A new subsection, *The two
copies are not identical, and must not be asked to be*, states the rule
and derives it from ORCH-121, with a table of the five fields that can
only exist in the merge message and why. The *Evidence* section points at
it. Quoted in the agent's report.

**Em dashes checked, not assumed.** Every edit was spliced with a
raw-byte perl `index`/`substr`, never `sed` or `perl -pe`. A byte-regex
double-encoding detector was proved on a fixture built by double-encoding
`docs/architecture.md` on purpose — 242 hits — and silent on a clean copy
of the same file, before its zero over the three edited files was
believed.

## Not done, and why

- `docs/coverage.md`, `docs/plan.md`, `docs/operating.md`,
  `docs/remediation/*`, `docs/decisions.md`,
  `docs/orchestration/board.md` and everything under `herd-*/`: not this
  agent's, by the brief. `origin/plan-truth` was read for consistency and
  not changed.
- No `D-NEXT` entry: the brief asks for none, and the rule it records is
  Alexa's of 2026-10-29, cited in the template rather than re-decided.
- No board row: not this agent's file.
- No `cargo`, no build, no test: the brief forbids them while five other
  agents compile on this host. `CARGO_TARGET_DIR` was never set and no
  build tree was created, so there is none to delete.

## Gates

Runs made **before** this commit, verbatim, on the tree with all three
file edits in it and this handoff not yet written:

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0
```

```text
$ ./scripts/check-orchestration.sh
...
== the gate scripts' own stdin (ORCH-111)
ok    check-orchestration.sh redirects its stdin before its first capture
ok    gate.sh redirects its stdin before its first step
ok    pre-push keeps the stdin git hands it: the ref list is its input

check-orchestration: 285 passed, 0 failed
```

285 passed, 0 failed — `main`'s figure at b37b2a4, measured in this same
worktree before any edit. The one over-long line that turns thirteen
`check-orchestration` cases red at once did not happen.

The **last run**, named and not quoted, on the tree this commit closes:
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`, both
from the worktree root, their output going to the agent's report.

**The four Rust gates were NOT RUN locally.** This is a prose-and-template
change — `docs/architecture.md`, `docs/handover.md`,
`docs/orchestration/templates/landing.md` and this handoff, no file
outside `docs/` — so `CLAUDE.md` *Commands* covers them with the batch's
own `./scripts/gate.sh` run at integration, which is `herd-merger`'s.
A gate that did not run is reported as not run, never as passed.

**Skips:** no skip. Neither script prints a `HERD-SKIPPED` line, and none
was copied in.

## Risks and findings

- **No twelfth constant exists.** Searched for `*VERSION` under any
  visibility and indent, for `SCHEMA`/`FORMAT`/`REVISION`/`_V[0-9]` const
  names, for `static` version items, and for a literal `version: N` in a
  struct or JSON built outside a test. The only non-test hits of the last
  kind are `herd-hub/src/status.rs:627-628`, inside
  `#[cfg(test)] mod tests` at line 616. What a reader could mistake for a
  twelfth is the build version, which §3.4 now labels as not a constant.
- **`OUTPUT_FORMAT_VERSION` is the one of the eleven with no wire field.**
  Every other constant has a `*_version` key in a document; this one
  rides inside the parentheses of the `ran on:` line as free text
  (`ran_on_line`, D109). Anyone writing a checker that looks for the
  field will miss exactly this number.
- **`docs/handover.md` section 4 is stale about M13 and was left alone.**
  "M13 moves to first after the gate" and the row promising
  use-against-budget "when: M13" read as though nothing of M13 had
  shipped; E-35 and most of E-37 did, in batch 8. The use-against-budget
  figure is genuinely still unbuilt, so the row is not false, only dated.
  Correcting the narrative there overlaps `docs/operating.md` and
  `docs/plan.md`, both owned by other agents tonight.
- **`docs/handover.md:78` still lists `docs/operating.md` as UNBUILT.**
  Another agent owns that file tonight; if it lands, this line needs the
  word removed. Not changed here, to avoid writing a claim about somebody
  else's branch.
- **Batch 8's own landing record says the two copies "are the same
  text", and then explains four ways they differ.** The handoff
  `docs/orchestration/history/2026-10-29-merge-batch-8.md:464` reads
  "This copy and that message are the same text", immediately before the
  **Gate:** bullet that defers the resolved sha, the verbatim output and
  the receipt path to the merger's report. The substance is right and is
  what the template now codifies; only that one sentence overstates it.
  History is append-only and the file is the merger's, so it is left as
  it stands — but the template no longer asks a future merger to write
  that sentence.
- **The mojibake detector has one known false positive, already on
  `main`.** `docs/orchestration/history/2026-10-29-merge-batch-3.md:188`
  fires, because that handoff deliberately quotes the byte shapes `â` and
  `Â§` while documenting the detector itself. Not corruption. **This
  handoff is now a second such file**, for quoting the same two shapes in
  this bullet: a corpus-wide run should expect exactly these two files and
  treat a third as real.
- **`check-docs.sh` requires a hex string on the committed copy's
  `**Gate:**` line** (`scripts/check-docs.sh`, the `hex` pattern of the
  landing awk, seven hex characters). The template's new subsection says
  so explicitly, because the self-resolving form batch 8 used — a short
  sha plus "this handoff commit" — is what satisfies it; a copy that
  deferred the sha entirely would go red.

## Decisions needed

None. The rule the template now records is Alexa's of 2026-10-29, given
in the brief; the board row and the `ORCH-nnn` for this task are the
orchestrator's to allocate.

## Resume point

`git fetch && git log --oneline origin/docs-version-truth -1`, then merge
the branch into the batch's integration branch like any other. Nothing in
it needs a Rust gate of its own; the batch's `./scripts/gate.sh` covers
it. If `docs/operating.md` lands in the same batch, check
`docs/handover.md:78` for the word UNBUILT.

## Processes

No long-running process started; no PID to report. No `cargo`, no build,
no test, so no `target/` and no `CARGO_TARGET_DIR` was created, and there
is none to delete. The worktree, removable with `git worktree remove`
once the branch has landed:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-docs-version-truth
```
