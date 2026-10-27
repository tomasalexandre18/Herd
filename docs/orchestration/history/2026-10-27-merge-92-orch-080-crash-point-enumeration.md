# merge-92 ORCH-080 crash-point enumeration — handoff 2026-10-27

- **Item:** ORCH-080
- **Agent:** herd-merger
- **Branch / commit:** `orch-080-crash-point-enumeration` / the merge of
  `6f5b1c0` with `origin/main` `f19efe6`, based on origin/main `f19efe6`
- **PR:** the batch PR (ORCH-080 + ORCH-092); #92 is closed as superseded

## Done

- **Compared first.** `6f5b1c0` branched at `2ca6dc7`; `main` had since
  taken PRs #85, #93, #94, #95, #96 and #97 — three of them this merger's,
  in this queue, in the hour before. The branch's only Rust is **one new
  file**, `herd-hub/tests/crash_points.rs`, 1 013 lines, ten
  `#[ignore]`d tests. `git diff origin/main HEAD -- '*.rs' 'Cargo.toml'
  'Cargo.lock' '*/Cargo.toml'` is that file and nothing else: **no
  `herd-hub/src` change, no `fsync` touched, no `Cargo` file touched.**
- **Merged `origin/main` into the branch**, never rebased, never
  force-pushed. **Two conflicts, both in docs, both resolved on meaning.**

### Conflict 1 — `docs/orchestration/board.md`

Both sides had rewritten the **same adjacent pair of rows**, ORCH-080 and
ORCH-081, which is why `ort` could not separate them.

- **HEAD** carried this branch's ORCH-080 (`owed` → `done`, the 308 states,
  B's hazard at 4 of them, the 176 refusals, ORCH-086 confirmed) and
  `main`'s *old* ORCH-081 (`owed`).
- **origin/main** carried the *old* ORCH-080 (`owed`) and PR #96's new
  ORCH-081 (`done`, D184).

**Kept each side's own row**: this branch's ORCH-080 and `main`'s ORCH-081.
Taking either side whole would have silently reverted the other PR's row.
Verified afterwards that both read `done` and that no other row moved.

### Conflict 2 — `docs/remediation/README.md`, three hunks

All three come from `12` and `13` having been written as the same number on
the same day. **All three kept both pieces.**

1. **The index date line.** HEAD said it was refreshed when `13` was
   written and added a paragraph that `13` is written against a later base
   than every piece before it. `main` said it was refreshed when `12` was
   written on `cd59ef5`. Resolved to name **both** refreshes, keeping
   HEAD's later-base paragraph, which is about `13` alone and true.
2. **The table row.** One row each. Both inserted, **`12` before `13`**.
3. **The "Mesures / Neither" classification.** HEAD added `13` to *Mesures*
   with an exception — it ran on this workstation but carries no timing at
   all, only counts and verdicts. `main` moved `12` into *Neither* as a
   **gate**. Resolved to a single bullet naming `07`, `12` and `13` and
   saying in what different way each of the three carries no figure,
   keeping HEAD's exception sentence verbatim in the *Mesures* bullet.

- **The paragraph explaining why `12` was absent is rewritten, because it
  no longer reads correctly.** It said `12` is on an unmerged branch and
  gets its row when that branch lands. That branch landed an hour before
  this one. It now says which piece gave way and why, that `13` checked
  `13` free against every open branch first, that **both are in the table
  above**, and that the paragraph itself was rewritten at `13`'s
  integration — which is the integration after `12`'s.
- **ORCH-106 and ORCH-107 were not written again.** Both are already on
  `main` from PR #94's integration; the enumeration's §5 finding is
  ORCH-106's origin and the row names it.
- **`docs/decisions.md`: nothing to allocate.** The branch carries no
  `D-NEXT` and the PR says so in *To record*. The highest number is
  **D184**, written at PR #96's integration an hour before.
- **Board:** ORCH-080's row records the integration and both conflicts;
  `## Next` item 3 now has ORCH-079, ORCH-080 and ORCH-081 all `done` and
  ORCH-082 as the one of the four still open.
- **`docs/status.md`:** the remediation dossier is **fifteen** pieces, `13`
  named. 90 lines.
- The task's own handoff,
  `history/2026-10-27-orch-080-crash-point-enumeration.md`, exists on the
  branch.

## Not done, and why

- **Nothing of §5's proposal was implemented.** A torn `job.json` still
  refuses the whole state root. That is **ORCH-106**, it is Alexa's because
  it changes what a hub does at startup on his consumer's running system,
  and the board ranks it before ORCH-085.
- **ORCH-086 is confirmed, not closed.** Nothing removes a stale temporary
  at open, and one left in `files/` is counted against the output budget
  for ever — 4 096 bytes measured. Its own row.

## Gates

Four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, on the
merge result, through `nix develop --command`. The diff touches a `.rs`
file, so none was skipped.

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.73s
(exit 0)

$ nix develop --command cargo fmt --all --check
(no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.66s
(exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 138 passed, 0 failed
(exit 0)
```

The enumeration itself, re-run on the merge result — it is `#[ignore]`d, so
the suite does not run it and someone has to:

```text
$ nix develop --command cargo test -p herd-hub --test crash_points -- --ignored --nocapture --test-threads=1
ORCH-080 — 308 constructed states, one job directory each
308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.38s
(exit 0)
```

Every figure reproduces on a tree that now carries PRs #95, #96 and #97 as
well, which is worth one line: the four `events.jsonl`-ahead states are
still four with the ordering oracle in place, and 0 are quarantined still.

## Risks and findings

- **The board conflict was the one place in this queue where taking a side
  whole would have been wrong and silent.** Both sides had rewritten the
  same two adjacent rows for different reasons; `--theirs` would have
  reverted ORCH-080 to `owed`, `--ours` would have reverted PR #96's
  ORCH-081 and lost D184's row text. Neither would have failed a gate.
- **`#[ignore]`d means the suite is not a guard here.** The 308 figures
  cannot drift silently *in CI*, because the table is asserted rather than
  printed — but nothing runs it unless somebody passes `--ignored`. That is
  the right trade for a 10 s enumeration that no other test depends on, and
  it is worth knowing when ORCH-084 is taken, since 4 of the 308 are
  exactly its hazard.

## Decisions needed

None new. **ORCH-106** is on the board waiting for Alexa and is this
enumeration's finding: recommended to make a torn record quarantine the
way a rejected label set already does, with the `RECORD_VERSION` mismatch
arm probably staying an error.

## Resume point

This half is integrated as part of the ORCH-080 + ORCH-092 batch; see
`2026-10-27-merge-batch-orch-080-092.md` for the batch-level handoff.

## Processes

Background shells started and ended by this agent only: `cargo build
--workspace --all-targets`, one `scripts/test.sh`, the enumeration, and
wait loops. No process of another agent was signalled; no `pkill`, no
kill by name.
Own `target/` is not deleted at this merge: PR #98 follows in the same
worktree. It is deleted once that one is pushed.
