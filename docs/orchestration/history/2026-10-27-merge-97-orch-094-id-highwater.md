# merge-97 ORCH-094 id high-water mark — handoff 2026-10-27

- **Item:** ORCH-094
- **Agent:** herd-merger
- **Branch / commit:** `orch-094-id-highwater` / the merge of `12012b9`
  with `origin/main` `6c366e8`, based on origin/main `6c366e8`
- **PR:** #97

## Done

- **Compared first.** `12012b9` branched at `cd59ef5`, which is older than
  the other three in this queue: `main` had since taken PRs #85, #93, #94
  and #95, 57 files and ~10 300 lines. Of those, the Rust that mattered was
  `herd-hub/src/store.rs` — touched by #85 and by #95's one-line module
  declaration — and this branch rewrites 382 lines of the same file.
- **Merged `origin/main` into the branch**, never rebased, never
  force-pushed. **One conflict**, and only one.
  - **`docs/orchestration/board.md`**, at the end of the *Owed* table.
    **HEAD** appended the ORCH-094 row. **origin/main** appended eight rows
    — ORCH-061, ORCH-068 (PR #93), ORCH-093, ORCH-096, ORCH-099, ORCH-100,
    ORCH-101, ORCH-102 (PR #94). Neither side edited the other's text; both
    are additions at the same place. **Resolved by keeping both**, `main`'s
    block verbatim and in `main`'s order, with ORCH-094 appended after it.
    Checked afterwards: `git diff origin/main -- docs/orchestration/board.md`
    is exactly one added line, the ORCH-094 row. No row was edited, none
    dropped, and the *Owed* table is not reordered.
  - **`herd-hub/src/store.rs` merged clean**, which is a thing to verify and
    not to assume — see *Risks*.
- **The ORCH-094 row is the one that stands.** `main` deliberately carries
  none; PR #94's §7 draft was skipped at its own integration, and both the
  ORCH-093 row and `docs/disk-policy.md` §3.6 say so in as many words. The
  branch's row was brought across intact. `check-docs.sh` would have caught
  the duplicate had both landed; it passes.
- **`docs/decisions.md`: nothing to allocate.** The branch carries no
  `D-NEXT` and adds no entry. The author named two candidates in the PR's
  *To record* and did not write them; the brief asked for none, so none was
  written. The highest number on `main` is **D183**.
- **Board:** ORCH-094's next-action cell now records the integration, says
  which CI run proves it, and replaces the stale *"Note for the merger"*
  with the **two risks its author recorded**, which were in the PR body and
  on no row: `store.json` stops being near-immutable and is rewritten at
  every mint, so anyone who later writes it non-atomically breaks this; and
  if ORCH-083 to ORCH-085 tier the store's durability, **this write belongs
  in the decisive tier**, being the one write whose loss re-issues an id.
- **`docs/status.md`:** the defect count is **eight**, not seven, with
  ORCH-094 named. 90 lines.
- The task's own handoff,
  `history/2026-10-27-orch-094-id-highwater.md`, exists on the branch.

## Not done, and why

- **No decision entry for either of the author's two candidates** — *"a
  number that cannot be re-derived never lives in a derived file"* and *"an
  unreadable high-water mark stops the hub"*. Both are real and both are
  stated in `StoreFile::ids_minted`'s doc comment; neither was asked for,
  and `decisions.md` is not a place to write things nobody requested.
- **`docs/disk-policy.md` §7 is not corrected** to say the mark landed in
  `store.json`. §3.6 already carries that correction, written at PR #94's
  integration and naming this PR, so the document is self-consistent the
  moment this lands. §7's table is the planner's original proposal and the
  dossier's rule is that a later piece wins and an earlier one is not
  edited.

## Gates

All four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, on
the merge result, through `nix develop --command`.

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.34s
(exit 0)

$ nix develop --command cargo fmt --all --check
(no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.62s
(exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 138 passed, 0 failed
(exit 0)

$ nix develop --command cargo test -p herd-hub --lib
test result: ok. 195 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

195 is 189 (`main` after PR #95) plus this branch's six. The two that
matter most on this merge are not this branch's at all:

```text
$ nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 193 filtered out
```

## Risks and findings

- **The one real integration risk, and it was checked rather than
  assumed.** PR #95's ordering oracle watches *every* durable operation
  under the state root and asserts an exact per-call sequence. This branch
  adds a durable write that did not exist when the oracle was written —
  `store.json` is now rewritten at every mint, raised and `fsync`ed before
  the `mkdir` that claims the id. Had the oracle's trace been order- or
  count-sensitive to writes outside a job directory, #95 and #97 would have
  been green apart and red together, and only an integration that runs both
  would see it. It runs both, and both oracle assertions pass, as do the
  six new `store::tests`.
- **No CI run had ever started for PR #97 before this merger pushed.**
  `gh run list --branch orch-094-id-highwater` was empty at 11:40 on
  2026-10-27, so "not red" was all that could be said of it. Its green is
  this merger's run and not the author's.
- **`store.json` is no longer near-immutable**, and that is a standing
  constraint rather than a defect: the rename is atomic, so
  `real_herd/mod.rs:715` still reads a whole file beside a running hub, but
  the next person to write that file non-atomically breaks the guarantee
  without touching this code. Now on the board row.

## Decisions needed

None for this PR. Two candidates are named in its body and deliberately
unwritten; they are a question for whoever next opens `decisions.md` with a
mandate, not for this integration.

## Resume point

PR #96 (`orch-081-hub-stop-path`) is next. It needs a `D-NEXT` number
allocated for `STOP_WITHIN = 5 s`; the next free number is **D184**.

## Processes

Background shells started and ended by this agent only: one
`cargo build --workspace --all-targets`, one `scripts/test.sh`, and wait
loops. No process of another agent was signalled; no `pkill`, no kill by
name.
Own `target/` is not deleted at this merge: the same worktree carries the
integrations after it. It is deleted once the last of them is pushed.

## Appendix — the suite on this workstation

```text
$ nix develop --command cargo build --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.06s
(exit 0)

$ nix develop --command ./scripts/test.sh
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
test suite: FAILED (cargo test exited 101)
EXIT=101
```

**Two targets, one test each, and both are the host's.** Attributed by the
same two means as PR #95, not by what the failures say about themselves.

**By a base run**, recorded at PR #95's integration on `origin/main`
`c831d55` in this same worktree: `admission` failed there with the same
test, `a_killed_hub_reads_the_reservations_back_off_the_records` at
`herd-hub/tests/admission.rs:419`, and `slots` with the same test,
`the_derived_j_is_recorded_and_is_what_the_job_is_given` at
`herd-hub/tests/slots.rs:744`. Identical sites, identical tests.

**By construction**, for the second means: this branch's Rust is 382 lines
of `herd-hub/src/store.rs` and nothing else. `slots.rs:744` asserts a
placement figure the hub derives from the host's live `MemTotal`, and
`admission.rs:419` asserts how much memory a lender admits — *"lender
admits at most 4 GiB of memory (5 GiB lent, 1 GiB kept as its own floor)"*.
Neither reads a job id, neither removes a record, and no mint is involved
in either. `slots.rs:744` is **ORCH-069**, and is **not** claimed green on
CI: `MemTotal` there is 15983 MiB, under the guard's 17408, so CI skips it
and by D27 a skip is never a pass.

**Three targets failed at PR #95's integration; two here, on more code.**
The one that dropped out is `killed_hub`, which failed at #95 with the
`real_herd/mod.rs:649` timeout — ORCH-107. The only difference in how the
suite was run is that `cargo build --workspace --all-targets` was run
**first** this time, so the `real_herd` tests did not spend their patience
building the binaries they drive. That is one observation and not a proof,
but it points the same way as the trap ORCH-092's author hit, and it is a
candidate explanation for part of ORCH-107 rather than a separate item.

**Skips (D27): at least 12**, all the cgroup-v2-delegation family of
`herd-runner --test confinement`, each printed as a skip and none counted
as a pass. A **floor, not a count** (D179): the run did not finish.
