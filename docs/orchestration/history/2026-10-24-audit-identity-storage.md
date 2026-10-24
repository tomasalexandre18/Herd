# audit-identity-storage — handoff 2026-10-24

- **Item:** none assigned on the board; this is mandate n° 2 (technical
  audit), §4.1 "Agent A: identity, stockage et sûreté des états". A board
  row should be opened for it and for Agent B.
- **Agent:** herd-step
- **Branch / commit:** `audit-identity-storage`, based on `origin/main`
  `0a2cf73`
- **PR:** none. `herd-step` may not open one; `herd-merger` opens and
  integrates it.

## Done

`docs/remediation/03a-identity-and-storage.md`, new, the only file this task
writes. It covers the five deliverables of §4.1: the responsibility and
persistence map (hub durable state, hub volatile state, runner state, and a
single-owner matrix), the job and attempt automaton with its commit points
and a crash-window table, the real meaning of each acknowledgement, the
identity chain from the consumer's request to the artefacts with eight named
confusion points, and the integrity risks with the code paths that carry
them. It closes with the six candidate invariants the brief assigns
(I01, I03, I04, I05, I11, I12), six experiments to authorise under gate T2,
four open questions for the owner, and an explicit list of what the document
does **not** establish.

Every claim is marked **F** (observed, with `file:line` on `0a2cf73`),
**H** (hypothesis) or **I** (unknown). No claim about a defect rests on the
size of a file; §6 argues the three structural couplings from dependencies
and shared locks instead.

## Not done, and why

- No board update. The brief restricts this task to one file, and audit B
  works from the same base at the same time; two agents editing the board
  would collide. The orchestrator should add the rows.
- No `docs/remediation/README.md` index (mandate §10 asks for one). It is
  shared between the two audits and belongs to whoever assembles them.
- `matcher.rs`, the queue, the admission arithmetic and the dispatch loop
  were deliberately left unread beyond what the state model needed: they are
  audit B's, and the mandate asks for two independent readings.
- Nothing was executed. No experiment in §8 was run; each is written as a
  request with what it would settle.

## Gates

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```
exit 0. The D170 note is pre-existing on this base and is informational, not
a failure.

**The four Rust gates were NOT RUN locally.** This change is prose only and
touches no Rust, no configuration and no migration; CI runs `check` on the
PR. A gate that did not run is reported as not run, never as passed (D27).
No skip of any kind was produced.

## Risks and findings

The five that matter, each with its reference:

1. **The commit link of the identity chain is open-loop.** No frame from a
   runner carries the revision the workspace was at; `StepRun` carries step,
   start, duration and exit code and no commit
   (`herd-proto/src/declaration.rs:250-274`). A `fetch` that exits 0 without
   reaching the declared commit produces a verdict attributed to a commit
   the tree was never at, and nothing on the record could contradict it.
   This is the reported incident's weakest joint, and D156 did not close it.
2. **One inline path turns a short collection into one that reads as whole.**
   `record_outputs` drops a manifest entry whose bytes disagree with their
   own digest (`herd-hub/src/store.rs:1710-1716`) instead of keeping the
   entry and naming the absence; `herdctl`'s shortfall count is computed off
   the manifest (`herdctl/src/main.rs:1322`) and so returns 0, and
   `code_for` then exits 0 (`herdctl/src/main.rs:591-596`). No test sends
   such a verdict.
3. **Two unbounded leaks around the streamed carriage.**
   `jobs/<id>/files.part/` is neither counted by the output budget nor
   removed by the sweeper for an abandoned collection
   (`herd-hub/src/store.rs:642-667`, `:1976-1982`), and
   `<data root>/outputs/<job>-<attempt>/` on the runner is removed only on
   acknowledgement (`herd-runner/src/lib.rs:1421-1446`) and is not in the
   startup reaper (`herd-runner/src/reclaim.rs:391-446`). The second is on
   the consumer's machine and touches invariant 2.
4. **`Peers::unwritten` is never pruned by a terminal state.** A verdict the
   hub could not write, for a job that then ends another way, stays in the
   retry list for the life of the process and prints an error per cycle
   (`herd-hub/src/server.rs:1570-1607`, `:4926-5010`).
5. **A stale comment on the ownership gate.** `server.rs:1795-1804` says
   M15's token "does not exist yet". It does: `admission_refusal`
   (`server.rs:2043-2069`) calls `Roster::admit`
   (`herd-hub/src/roster.rs:234-256`), covered by
   `herd-hub/tests/approval.rs`. The comment is on the one gate that decides
   whose verdict is believed.

What **contradicts the brief**: the brief says to write in one file and
nowhere else, while `herd-step.md` requires a handoff in
`docs/orchestration/history/`. The brief itself allows the handoff ("écris
ta passation si ton rôle l'exige"), so this file exists and no third path is
written.

What **supports** the brief's suspicion but only partly: D156 is a
cause-level fix and the best piece of work I read; D167 is also cause-level
for the fetch-on-verdict race and covers both live followers and late
`wait`s; D171's fix is correct but costs observability (the live E-04
sentence now lives only in memory, and `job.json`'s countdown decays), which
is named in the decision rather than hidden. D169 is audit B's.

## Decisions needed

Four, in §9 of the document, each with a recommendation. The one that cannot
be taken by an agent: **should the verdict carry back the revision the
workspace was at?** Recommendation: yes, one field on `JobFinished` or
`StepRun`, with the hub recording a divergence rather than refusing at
first. It is a protocol break and so is Alexa's. Fallback: record the
divergence only, with no refusal, which still makes I01 falsifiable.

## Resume point

`herd-merger` opens the PR for this branch (`herd-step` may not), fills the
Evidence block with the `check-docs.sh` output above and the statement that
the Rust gates were not run locally. Then the orchestrator opens board rows
for audit A and audit B, and whoever assembles the dossier writes
`docs/remediation/README.md` from both reports.

## Processes

None started, none ended. No build, no test, no binary, no container, no
service, nothing on the consumer's side. `scripts/check-orchestration.sh`
was not run, as the brief requires.
