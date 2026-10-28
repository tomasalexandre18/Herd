# orch-124-output-removed-readmit — handoff 2026-10-28

- **Item:** ORCH-124
- **Agent:** herd-step
- **Branch / commit:** orch-124-output-removed-readmit / 004be39 (code),
  handoff commit below; based on origin/main bc83717
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

**The defect is real and it was reproduced red before anything was
changed.** `write_output_chunk` in `herd-hub/src/store.rs` never looked
at `record.outputs.removed`. A whole, verified file renamed back into
`files/` after an eviction therefore left the mark in place, and from
then on those bytes could never be freed by any sweep: `sweep_now`
measures them (`directory_bytes` of `files/` is non-zero), then refuses
them because `record.outputs.removed.is_some()`, and adds them to
`could_not_free`. The budget shrinks by that collection for ever, and —
worse than the shrinking — the sweeper destroys the collections it *can*
take in their place.

The fix, per Alexa's decision of 2026-10-28:

- new private `JobDirStore::readmit_to_the_budget(id)` beside the
  sweeper in `herd-hub/src/store.rs`: takes the store lock, re-reads the
  record, clears `outputs.removed` and writes the record. It does not
  touch `updated_at`, so re-admission is not a second chance at the
  retention floor, and it does not go through `open_for_writing`, for
  the reason `note_outputs_fetched` does not.
- `write_output_chunk` calls it at the rename that puts a whole verified
  file into `files/`, **before** `held_more`, so the sweep `held_more`
  may trigger already sees an unmarked collection.
- regression test
  `store::tests::bytes_written_after_an_eviction_are_swept_again`:
  evict, rewrite, evict again. A budget of 512 bytes and one 1024-byte
  streamed collection, so the collection is taken the moment it is
  whole; the runner then sends it again; the sweeper takes it a second
  time. It asserts the bytes are gone, `could_not_free == 0`,
  `held == 0`, the record carries a fresh mark with its whole manifest,
  and that the job's own history has **two** `outputs_removed` lines.
- `herd-hub/src/store/ordering_oracle.rs`: the new record writer is
  driven as the oracle's step 17 (`Part::Record`, no event), and
  `RECORD_WRITE_SITES` moves 13 → 14. The fixture's closing assertion
  becomes `removed.is_none()` plus "the `outputs_removed` line is still
  on the history", which is the state those two steps leave.

**No new `JobEventKind` variant.** `herd-hub/tests/announced_job_ceiling.rs`
guards that enum: it is `#[serde(tag = "event")]` with no
`#[serde(other)]` arm, so a new variant makes `events.jsonl` undecodable
to a hub rolled back. Re-admission therefore writes no history line; the
`outputs_removed` line already there stays true (the bytes *were*
removed that day), and the live state is the record's mark.

`docs/decisions.md` and `docs/orchestration/board.md` were not touched.

## Not done, and why

- **`files_may_land` in `herd-hub/src/server.rs` is untouched.** It
  already refuses a chunk for an evicted collection, which is the branch
  the decision says was rejected. Changing it is widening the change,
  and the brief says to report that rather than do it. See *Risks and
  findings* 1 and 2 and *Decisions needed*.
- No end-to-end hub+runner test was added: with `files_may_land` as it
  is, a runner cannot drive this path through the link at all (finding
  1), so an e2e test would assert the refusal, not the fix.

## Gates

`nix develop --command ./scripts/test.sh` — **PASSED**, exit 0:

```
test store::tests::bytes_written_after_an_eviction_are_swept_again ... ok
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 201 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.22s
...
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
```

`nix develop --command cargo clippy --workspace --all-targets -- -D
warnings` — exit 0:

```
    Checking herd-hub v0.3.7 (.../orch-124/wt/herd-hub)
    Checking herd-runner v0.3.7 (.../orch-124/wt/herd-runner)
    Checking herdctl v0.3.7 (.../orch-124/wt/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.31s
```

`nix develop --command cargo fmt --all --check` — exit 0, no diff (one
`matches!` block it did ask for was folded before this run).

`./scripts/check-docs.sh` — exit 0, run with this handoff in the tree:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**Skips: 12 `HERD-SKIPPED` lines, all cgroup-v2 delegation on this
host**, none of them near the output budget. Copied as skips, never
counted as passes (D27). Named:

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
an_io_weight_that_cannot_bite_is_said_out_loud
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces
```

### The red run, before the fix

Same test, on the unfixed store. The hub's own line is the evidence:

```
running 1 test
herd-hub: the output sweeper removed 1 collection(s), 1 KiB, to stay inside the 512 bytes budget; 0 bytes held now. Every one of those records still names every file it had, with its size and digest.
herd-hub: 1 KiB of job output is held against a 512 bytes budget and nothing more may be removed: 1 KiB of it belongs to collections that are still arriving or whose records will not read, and a transfer in flight is never taken. The volume will stay over budget until those finish.

thread 'store::tests::bytes_written_after_an_eviction_are_swept_again' (1318645) panicked at herd-hub/src/store.rs:6086:9:
bytes written after an eviction are the sweeper's again: left marked, they would sit on the disk for ever with nothing able to free them
test store::tests::bytes_written_after_an_eviction_are_swept_again ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 200 filtered out; finished in 0.07s
```

That second line is the hub lying to its operator: the collection is
neither still arriving nor unreadable. It was evicted and refilled, and
1 KiB of a 512-byte budget is gone for good. After the fix the same test
prints the eviction line twice and nothing else.

## Risks and findings

1. **The brief's reachability story is wrong, and this is the one thing
   I would want checked before anybody concludes the consumer's 107 GB
   is explained by this.** A runner's re-sent bytes do not reach
   `write_output_chunk` today: `files_may_land`
   (`herd-hub/src/server.rs:4835`) reads the record first and refuses
   the chunk when `outputs.removed` is set, with a comment naming D152
   and the refill/sweep loop it avoids. The runner side matches it on
   purpose — `herd_runner::Session::offer_swept_collections` re-offers a
   swept spool at every join, and
   `the_hub_has_said_what_it_is_owed` exists to drop the spool on that
   silence. `collections_owed`/`collection_owed` also skip a removed
   collection, so the hub never asks for it either. So the leak is a
   store-level defect, not a live runner-driven loop.
2. **The decision as relayed contradicts `files_may_land`.** "Refusing
   the write after eviction was considered and rejected because it loses
   output without telling the requester" is exactly what that function
   does, and has done since D152. My change makes the *store* honest
   (and fixes the race in 3) but does not change the hub's answer to a
   runner, so Herd now holds both behaviours: the server refuses the
   chunk, and the store would re-admit it if one ever arrived. That is
   coherent but it is not what the decision describes end to end. If the
   intent is really "take the bytes back and sweep them again", the
   refusal in `files_may_land` has to go, and D152's loop objection has
   to be answered — my reading is that it is answerable (the second
   sweep is cheap, the runner drops its spool on the `JobFilesAck` the
   completion sends), but it is a decision, not a refactor.
3. **The path that *is* reachable today is a race, and the fix closes
   it.** `files_may_land` reads the record with no lock held, and
   `write_output_chunk` reads it again and only takes the store lock
   inside `held_more`. A sweep that evicts between that check and the
   rename leaves a refilled `files/` under a live mark — permanently
   unfreeable before this commit. It needs a chunk completing a file at
   the moment a sweep lands on the same job, so it is narrow; on a hub
   at 107 GB of 200 GB, with a sweep per collection that completes, it
   is not negligible.
4. **The other writer of output bytes does not have the blind spot.**
   `record_outputs` (`herd-hub/src/store.rs:1863`), the inline carriage,
   rebuilds `outputs` from scratch with `removed: None` and an explicit
   comment, so a re-run's verdict already re-admits the collection. Its
   `held_more` call then counts the bytes. No third writer touches
   `files/`: the only other uses of `KEPT_FILES_DIR` read, measure or
   rename it away.
5. `outputs.first_fetched_at` survives a re-admission, so a collection
   somebody fetched before the first eviction keeps "fetched" and is
   ranked for eviction before an unfetched one. I left it: that
   requester does have a copy. Flagging it because it is a judgement,
   not a consequence of the fix.
6. The `herd-hub` lib suite is 201 tests, up from 200: one test added,
   none removed, none weakened. The oracle's closing assertion changed
   from `removed.is_some()` to `removed.is_none()` **plus** an assertion
   that the `outputs_removed` history line is still there, because step
   17 now runs after step 16; nothing it checked is checked less.

## Decisions needed

**D-NEXT (proposed, text for the merger to number):**

> **Bytes that arrive for a collection the sweeper has evicted re-admit
> that collection to the output budget.** The "removed" mark on a job's
> record is the live state of its collection, not a verdict on the job:
> `write_output_chunk` clears it at the rename that puts a whole,
> verified file back into `files/`, and the sweeper may take the
> collection a second time. The alternative — refusing the write — loses
> output without telling the requester. Left marked, those bytes are
> measured by every walk, refused by `evict` and reported as
> `could_not_free` for ever: the budget shrinks by one collection per
> episode and the collections the sweeper can still take are destroyed
> in their place (ORCH-124, red test first). Re-admission writes no
> `events.jsonl` line, because `JobEventKind` may not grow a variant
> (D181), and does not move `updated_at`, so it is not a second chance
> at the retention floor.

**Question for Alexa / the orchestrator:** should `files_may_land` stop
refusing a chunk for an evicted collection, so the decision holds end to
end (finding 2)? — **Recommendation:** yes, as a separate task with its
own red test on the link path, because it changes what a runner is told
and touches the spool contract D152 reasoned about. — **Fallback:** keep
the refusal and record it as the deliberate shape: the store re-admits,
the server does not take bytes back, and the only way bytes come back is
a re-run's verdict or the race in finding 3.

## Resume point

Nothing is pending on this branch. For the merger:

```sh
git fetch origin
git log --oneline origin/main..orch-124-output-removed-readmit
```

then merge it into the batch's integration branch and allocate the
D-NEXT number above. If the follow-up in *Decisions needed* is approved,
the next task is `files_may_land` plus an end-to-end test where a runner
re-offers a swept collection and the hub takes it.

## Processes

No background process was started; no PID was killed. Every command was
a foreground `nix develop --command …` in
`.../scratchpad/orch-124/wt`. The worktree's own `target/` is left in
place until the branch is pushed; it is at

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-124/wt/target
```

and is deleted immediately after the push, with the worktree kept for
the merger.
