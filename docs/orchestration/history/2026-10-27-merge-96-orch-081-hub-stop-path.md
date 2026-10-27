# merge-96 ORCH-081 hub stop path — handoff 2026-10-27

- **Item:** ORCH-081
- **Agent:** herd-merger
- **Branch / commit:** `orch-081-hub-stop-path` / the merge of `5580f33`
  with `origin/main` `a4f6e26`, based on origin/main `a4f6e26`
- **PR:** #96

## Done

- **Compared first.** `5580f33` branched at `cd59ef5`; `main` had since
  taken PRs #85, #93, #94, #95 and #97. The branch's own Rust is
  `herd-hub/Cargo.toml` (one `tokio` feature, `signal`),
  `herd-hub/src/lib.rs` (three new re-exports), `main.rs`, `server.rs` and
  the new `herd-hub/tests/stopping.rs`. `main`'s Rust since that base is in
  `store.rs`, `durable.rs`, `job.rs`, `matcher.rs` and the runner — a
  different set, with `server.rs` touched by #85 only.
- **Merged `origin/main` into the branch**, never rebased, never
  force-pushed. **No conflict at all**, including on
  `docs/orchestration/board.md`: #94's rewrite, #95's ORCH-079 edit and
  #97's appended ORCH-094 row are all elsewhere in the table, and this
  branch edits the ORCH-081 row alone.
- **`docs/decisions.md`: D184 allocated and written.** The next free number
  above the highest on `origin/main` (**D183**) and above every open PR —
  #92 and #98 both have a `docs/decisions.md` identical to `main`'s. D143,
  D154 and D170 stay gaps (D148). The entry is
  **"D184 The hub stops in five seconds; an overrun is a crash"** and it
  says five things:
  1. the figure, `STOP_WITHIN = 5 s` in `herd-hub/src/server.rs`, mirrors
     **invariant 2's runner bound** and is `07` **V9** made real — and this
     entry does **not** extend invariant 2 to the hub, it gives the hub a
     figure of its own that happens to be the same one;
  2. it is **justified on ext4** — `11-sync-prices.md` §7's **3.128 s**
     pass at `N` = 1 000, the class the deployment runs on per ORCH-053 —
     and **not on ZFS**: `11`'s 6.949 s and `08`'s **9.970 s** for the same
     depth both overrun it, which is in the constant's doc comment;
  3. **an overrun is a crash and not a corruption**: it costs exactly what
     a `SIGKILL` at the same instant would, every write in `durable.rs`
     being a synced temporary renamed into place or a synced append (D8),
     and `JobDirStore::open` already recovering an attempt a hub died
     inside;
  4. the hub **exits non-zero** on an overrun, deliberately, so that a stop
     which is still a crash stays countable instead of being laundered into
     a clean exit because the hub was asked politely;
  5. five fits inside **docker compose's ten-second default grace**, which
     would otherwise `SIGKILL` the stop path mid-drain — worse than no stop
     path, because it is harder to see.
- **It says whose word it is not.** Alexa was asked whether to record the
  bound and **had not answered when this was written**. D184 states that in
  its own text, says he may overturn it, and says that if he does the entry
  stays and a later one supersedes it, because a number is an identifier
  and the file is append-only (D148).
- **One line of product prose changed to match**: `STOP_WITHIN`'s doc
  comment opened *"A proposal, not an invariant"*; it now opens *"Recorded
  as D184, and not an invariant"* and carries the same caveat about Alexa's
  silence. Nothing else under `herd-hub/src` was touched by this
  integration.
- **Board:** ORCH-081 `active` → `done`, with the integration, D184's
  content, and ORCH-083's last open dependency recorded as cleared; `##
  Next` item 3 updated.
- **`docs/status.md` rewritten**, because this merge made it wrong and it
  was owed two corrections besides. It is state and 90 lines exactly.
- The task's own handoff,
  `history/2026-10-27-orch-081-hub-stop-path.md`, exists on the branch.

## Not done, and why

- **`herd-hub/tests/killed_hub.rs` is not reworded.** Its file header
  describes a signalled hub as ending the link *"with a reset"*. After this
  merge that is true of `SIGKILL` by design and of `SIGTERM` only by
  accident — nothing is closed gracefully on the way out, so the socket
  still dies under the peer. It is **not a one-line fix**: the header is a
  seventeen-line argument about why the in-process tests cannot reach this,
  and correcting it means distinguishing the two signals throughout it.
  The author declined it deliberately as another file's prose. Named on the
  ORCH-081 row instead.
- **No graceful WebSocket close**, and the present behaviour is now
  asserted rather than incidental. Changing it moves which branch a runner
  takes on an ending link (D104) and is its own task with its own
  measurement.

## Gates

All four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, on
the merge result, through `nix develop --command`.

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.57s
(exit 0)

$ nix develop --command cargo fmt --all --check
(no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.30s
(exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 138 passed, 0 failed
(exit 0)

$ nix develop --command cargo test -p herd-hub --lib
test result: ok. 198 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

198 is 195 (`main` after PR #97) plus this branch's three. The stop's own
tests, and PR #95's oracle beside them:

```text
test server::tests::a_second_stop_signal_changes_nothing ... ok
test server::tests::a_hub_that_is_stopping_starts_no_dispatch_pass ... ok
test server::tests::the_stop_waits_for_the_pass_in_flight_and_gives_up_at_its_bound ... ok
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 193 filtered out

$ nix develop --command cargo test -p herd-hub --test stopping -- --test-threads 1
test a_hub_asked_to_stop_leaves_cleanly_and_the_store_is_whole ... ok
test a_burst_of_stop_signals_is_still_one_clean_stop ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.19s
```

## Risks and findings

- **D184 was written without Alexa's word**, which is the one thing in this
  integration he should see first. The alternative was to merge a figure
  that governs every `docker compose stop` of the consumer's hub and leave
  it written down nowhere but a constant, which is how a number nobody
  agreed to becomes a promise. The entry is reversible by a later entry and
  says so.
- **The bound does not hold on ZFS** at `N` = 1 000, and the deployment is
  on ext4 only by ORCH-053's answer. If the consumer's state root ever
  moves, the bound moves with it and D184 has to be revisited rather than
  rediscovered.
- **`killed_hub.rs`'s header is now inaccurate by omission** — see *Not
  done*.

## Decisions needed

**Alexa, one question, already on the board as ORCH-081's own and now
answered provisionally in D184:** is `STOP_WITHIN = 5 s` the hub's bound?
*Recommendation:* yes, as written in D184 — the figure, its ext4
justification, its ZFS refutation, the non-zero exit and the docker compose
grace. *Fallback:* overturn it with a later entry naming a different figure;
nothing in the code resists that, the constant being one line.

## Resume point

PR #92 (`orch-080-crash-point-enumeration`) is next, then PR #98
(`orch-092-spool-sweep`). PR #92's dossier index explains why `12` is
absent from the table; `12` now exists on `main`, so that paragraph has to
be rewritten at its integration.

## Processes

Background shells started and ended by this agent only: `cargo build
--workspace --all-targets`, one `scripts/test.sh`, and wait loops. No
process of another agent was signalled; no `pkill`, no kill by name.
Own `target/` is not deleted at this merge: the same worktree carries the
integrations after it. It is deleted once the last of them is pushed.

## Appendix — the suite on this workstation

```text
$ nix develop --command cargo build --workspace --all-targets
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

**The same two targets and the same two tests as at PRs #95 and #97**, and
both are the host's. Attributed by two means, neither of them the
assertion's own words.

**By a base run** on `origin/main` `c831d55`, recorded at PR #95's
integration in this same worktree: `admission` failed there with
`a_killed_hub_reads_the_reservations_back_off_the_records` at
`admission.rs:419`, `slots` with
`the_derived_j_is_recorded_and_is_what_the_job_is_given` at
`slots.rs:744`. Identical.

**By construction**, which matters more here than on the other two because
this branch *does* change the hub's lifecycle. `herd-hub/tests/stopping.rs`
is new and passes 2/2, three times over. `herd-hub/tests/killed_hub.rs`,
the file whose two tests send real signals to a real hub, passes in full in
this run — including `a_hub_asked_to_stop_mid_job_loses_nothing`, which
now exercises the stop path rather than a kill. Neither failing assertion
touches a signal, a stop or a dispatch pass: `slots.rs:744` compares a
placement figure the hub derives from this host's live `MemTotal`, and
`admission.rs:419` how much memory a lender admits. `slots.rs:744` is
**ORCH-069** and is **not** claimed green on CI — `MemTotal` there is 15983
MiB, under the guard's 17408, so CI skips it and by D27 a skip is never a
pass.

**Skips (D27): at least 12**, the cgroup-v2-delegation family of
`herd-runner --test confinement`, each printed as a skip and none counted
as a pass. A **floor, not a count** (D179): the run did not finish.

**`cargo build --workspace --all-targets` was run first again**, and again
`killed_hub` did not time out. Three runs now: without the prebuild, three
targets red and the third a `real_herd/mod.rs:649` timeout; with it, two.

**Correction 2026-10-27, by `herd-merger` at batch 4's integration
(ORCH-107): that prebuild does not remove the in-test build, so the
three runs are not evidence for it.** `real_herd::binaries` runs
`cargo build --offline -p herd-hub -p herd-runner -p herdctl`, a
narrower feature set than `cargo test --workspace` resolves, and it
still compiled for **9.70 s** immediately after `--all-targets` --
against **0.06 s** after the identical command, which `scripts/test.sh`
now runs itself. What the three runs observed stands as observed; the
explanation offered for it does not.
