# audit-contradictory-review — handoff 2026-10-24

- **Item:** mandate 2 (technical audit), §0.1 — contradictory review of
  audits A and B. No ORCH row was given and I did not open one: the
  brief reserves `docs/orchestration/board.md` to the orchestrator.
- **Agent:** herd-reviewer
- **Branch / commit:** `audit-contradictory-review`, based on
  `origin/main` `0a2cf73`, in its own worktree. The principal working
  directory was not touched.
- **PR:** none. `herd-step` and this role may not open one; the brief
  says `herd-merger` does.

## Done

`docs/remediation/03c-contradictory-review.md`, new. Every **F** in
both reports taken back to the code at `0a2cf73`; the D171 question
answered; overlaps and blind spots named; one ranked list of thirteen
findings separating what the code proves from what needs an
experiment; the nine proposed experiments reduced to a sequence of
four; one strategy recommendation.

This handoff is the second file. The brief allows a passation; my
contract (`.claude/agents/herd-reviewer.md`) names
`docs/orchestration/history/<date>-review-<subject>.md` as the one file
I may write, and the brief names `docs/remediation/03c-…` instead. I
wrote the review where the brief asks and the handoff where the
contract asks. No third path.

## Not done, and why

- No Rust, no configuration, no migration changed. No `cargo build`,
  `cargo test`, `cargo clippy`, `cargo fmt`. No experiment run, no load
  placed on this shared host.
- `scripts/check-orchestration.sh` not run: the brief forbids it.
- `docs/orchestration/board.md` not touched: the brief reserves it.
- No PR opened.

## Gates

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

The D170 note is pre-existing on this base. The four Rust gates were
**NOT RUN** locally; this change is prose only and CI runs them on the
PR. A gate that did not run is not a gate that passed (D27). No
`HERD-SKIPPED` line: no suite was run.

## Risks and findings

1. **The brief's central question rests on a misreading.** Audit A
   never says D171 corrects its cause; A says that of **D156**
   (`03a:371`). B says D171 is both a cause fix for the write volume
   and a symptom fix for the incident — which is what D171 itself
   says. No arbitration is owed. §0.1 and §3 of the review.
2. **A new finding on the seam between the two mandates.**
   `collections_owed` (`store.rs:2374`) and `sweep_now`
   (`store.rs:642-667`) each walk **every job directory ever
   submitted**, and nothing removes one. They run on every runner
   redial (`server.rs:2415`, four lines before the dispatch pass), on
   every requester `AskJobFiles` (`server.rs:4384`), and on every
   verdict once the output budget is reached (`store.rs:1748`,
   `:1903`). A owns the unbounded growth and did not read the hot
   paths; B costed the hot paths and did not read output storage.
   §4.2.
3. **Three corrections, no refutations.** Nothing in either report is
   refuted. A's `herdctl/src/main.rs:1322` should be `:1283`; A's
   §5.4 closing **H** is provable to **F**
   (`herd-runner/src/lib.rs:1275`, `:1397-1411`); B's eleven dispatch
   call sites are thirteen, and the two it misses — `server.rs:2419`
   and `:2434` — are the redial and departure sites its own feedback
   loop runs through. B's reading of `slots.rs` should be **H**: the
   author's comment names a different residual window (D88).
4. **The split left the requester trust boundary unaudited.** Who may
   submit, cancel, follow or fetch is in neither mandate. §4.3.
5. **`0a2cf73` is not tag `v0.3.7`.** `v0.3.7` is `3da48f5`, two
   merges earlier. The base both audits read is right; the label the
   brief puts on it is not.

## Decisions needed

Whether to authorise experiment **6.1** — the cost surface
`worst_turn(N, T, d, W)`, in process, with a third axis for closed job
directories — before any architecture is chosen.
**Recommendation: yes, and first.** It is the cheapest of the nine
proposed experiments, needs no network and no real binaries, and is
the only one whose result reorders the finding list.
**Fallback:** if no isolated machine is available, record I06 as
unproven, ship findings 5, 10, 11 and 12 (all local, two of them one
line), and put the commit-field question to Alexa separately — it has
the longest lead time of anything in the dossier.

## Resume point

`herd-merger` opens the PR for `audit-contradictory-review` once the
orchestrator has the three reports in hand. The orchestrator opens the
board rows for audits A, B and this review, and whoever assembles the
dossier writes `docs/remediation/README.md` from all three.

## Processes

None started, none ended. No build, no test, no binary, no container,
no service, nothing on the consumer's side.
