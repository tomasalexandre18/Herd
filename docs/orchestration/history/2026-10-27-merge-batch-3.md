# merge-batch-3 — handoff 2026-10-27

- **Item:** ORCH-069, ORCH-083, ORCH-112; rows opened ORCH-113 to ORCH-118
- **Agent:** herd-merger
- **Branch / commit:** `merge-batch-3` / `56365ed`, based on origin/main
  `64cc532`
- **PR:** #102, https://github.com/tomasalexandre18/Herd/pull/102

## Done

Three branches merged onto one integration branch in the order asked,
one at a time, each with gates before the next.

| # | Branch | Tip | ORCH | Handoff |
|---|---|---|---|---|
| 1 | `orch-069-host-independent-assertions` | `6e54fca` | ORCH-069 | `history/2026-10-27-orch-069-host-independent-assertions.md` |
| 2 | `orch-083-drop-event-fdatasync` | `8c14d25` | ORCH-083 | `history/2026-10-27-orch-083-event-fdatasync.md` |
| 3 | `orch-record-fixes` | `c109ce2` | ORCH-112 | `history/2026-10-27-orch-record-fixes.md` |

All three branch from `64cc532`, so every merge was a three-way merge
against the same base.

### Conflicts

**None textually, and that is the finding rather than the relief.** The
three branches touch `docs/orchestration/board.md` and nothing else in
common; their hunks land at board lines 66, 85, 95, 109 and 202-209,
every pair more than three lines apart, so `ort` applied all of them
without asking. A batch's danger is a board conflict resolved
mechanically, and a batch's *quieter* danger is a board merge that was
never a conflict and therefore never read. Each side was checked on
meaning afterwards, by extracting the exact line each branch wrote and
requiring it verbatim in the merged file:

```text
069-row line 95: PRESENT        (ORCH-069 open -> done)
083-row line 66: PRESENT        (ORCH-083 owed -> done)
rec-ORCH112 line 85: PRESENT    (the new row, inserted)
rec-ORCH111 line 109: PRESENT   (ORCH-111 tightened to what was measured)
item6 block identical at 203-210 (069's rewrite of the start-here prose)
```

and from the other direction: the merged board differs from branch 1's
board by 3 lines, from branch 2's by 11 and from branch 3's by 10 —
each difference being exactly the other branches' own hunks. Nothing
was reverted.

**ORCH-069's second board edit is kept, deliberately.** Besides its own
row it rewrote item 6 of the `Next` prose, which asserted the suite is
red on this workstation. Its author flagged the edit and offered to
have it reverted. Keeping it was the right call: leaving item 6 would
have left a statement this branch had just falsified standing three
screens below its own row, and ORCH-107's sentences inside that item
are byte-identical across the rewrite, so nothing of the other row was
touched. Item 6 has since been rewritten again here, to carry the
figure measured on the merged tree.

### The suite, measured on the merged tree

**Green on this workstation, for the first time.** After merge 1,
`836 passed, 0 failed`, 12 skipped — the same three figures ORCH-069's
author reported, independently reproduced. After merge 2, `837 passed,
0 failed`, 12 skipped; the one extra test is named below and is the only
difference between the two runs' test lists. The skip figure is a count
and not a floor, because both runs completed (D179).

### ORCH-083 verified rather than accepted

Each of the five things the brief asked to be checked, checked:

- **`write_file_with` is byte-for-byte unchanged.** The function's text
  hashes identically at `64cc532` and at the merged head
  (`103aedfc…a73e8` both sides). The record keeps `sync_all` **and**
  `sync_dir`.
- **One event line of thirteen is unsynced.** `store.rs` holds exactly
  one `self.append_event_unsynced(` call site, at `record_waiting`, and
  ten `self.append_event(` sites; the oracle's own tripwire pins both
  counts and their sum.
- **`store.json`'s `ids_minted` is still in the decisive tier.**
  `mark_minted` goes through `write_json` -> `write_file_with`,
  untouched.
- **The 308-state enumeration is unchanged.** `308 / 176 / 44 / 4 / 99
  / 0`, the same six figures; and `crash_points.rs` hashes identically
  at both revisions once comment lines are removed, so only its prose
  moved.
- **The oracle is green**, and the gate ORCH-084 inherits is recorded
  in *Gates* below.

### Rows opened

Six, from findings that had no row. Five are the brief's; ORCH-114 is
the sixth and is argued for below.

- **ORCH-113** (*Waiting*, Alexa) — D183's reversibility paragraph
  names a `--durable-waits` flag; it was not built. Recommendation:
  skip it and record why; the revert is one commit (`4f16a32`) and
  nothing is deployed. Written so that D183 having named it is the
  first thing the row says.
- **ORCH-114** (*Open*) — the wider reading of step A, every event line
  unsynced. Not in the brief's list of rows to open, but the brief
  asked that it "belongs in a row if it is not already", and it was
  not: an unsynced `events.jsonl` can lose many trailing lines, so a
  hub could return with `job.json` at `finished` and the last event at
  `assigned`, which is behind but more than *one* transition behind.
  The row says what ORCH-083's author recommended and the orchestrator
  agreed: ask it with step B, one answer for both.
- **ORCH-115** (*Open*) — the derived-build admission trap. A build's
  admitted memory is `-j` x per-thread while `-j` is 3/4 of the lend,
  so a fixture whose floor exceeds a quarter of its lend makes the
  derived build inadmissible and the test hangs instead of failing.
- **ORCH-116** (*Open*) — D119's clamp is proved by no test that runs
  here; the one that would is in the skip list on every run.
- **ORCH-117** (*Open*) — `check-docs.sh`'s fence rule is anchored at
  column 1. Routed to `herd-reviewer` first, per `workflow.md` step 6.
- **ORCH-118** (*Open*) — `durable.rs`'s `Renamed` note has no witness
  type; it is pinned by convention where the four sync points are
  pinned by a type.

Nothing was judged too small for a row.

### Also written

- `docs/orchestration/board.md`: the three rows now name PR #102
  instead of "pushed, no PR"; `Next` item 6 carries the measured green
  suite and points at ORCH-116; a new item 8 records that the
  durability tier has begun and that **one line of thirteen** is
  unsynced.
- `docs/status.md`: the "suite is red here" paragraph replaced by the
  measured green one; D183 step A added to what is in no release;
  the commit count past v0.3.7 re-counted (219, on this PR's head);
  90 lines.

## Not done, and why

- **No `D` number allocated.** No entry headed `D-NEXT` exists on any
  of the three branches, `gh pr list` showed no open PR before #102,
  and the highest number on `origin/main` is D184. D183 already covers
  step A; ORCH-069 and ORCH-112 decide nothing. D170 stays a gap
  (D148), as `check-docs.sh` notes on every run.
- **`--durable-waits` was not built here either.** It is a question for
  Alexa (ORCH-113), not an integrator's to settle.
- **`docs/coverage.md` E-53 untouched.** `10 §6` scopes the sentence it
  needs to step **B**; step A touches no transition line.
- **`11-sync-prices.md` §6's table was not corrected.** Its "store
  `record_waiting` | 3 | 3.11 ms" row now describes a hub that no
  longer exists, but the dossier's `README.md` forbids editing an
  earlier piece. The harness says so itself and ORCH-083's handoff is
  the later statement.
- **No tag, no release, no issue, and nothing read or written on the
  consumer's side** (invariant 9, D161).

## Gates

Four Rust gates plus both scripts, on the head that was pushed. Every
command was redirected to a file; none was piped to `head` or `tail`
(ORCH-112's new rule in `workflow.md` *Failure*).

```text
$ nix develop --command cargo fmt --all --check
exit=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.21s
exit=0

$ nix develop --command cargo build --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 04s
exit=0

$ nix develop --command ./scripts/test.sh
test suite: PASSED
837 passed; 0 failed   (summed over every `test result:` line)
exit=0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0

$ ./scripts/check-orchestration.sh
check-orchestration: 141 passed, 0 failed
exit=0
```

Per branch, in the order they were merged:

```text
-- after merge 1, orch-069-host-independent-assertions --
$ nix develop --command cargo build --workspace --all-targets
exit=0
$ nix develop --command ./scripts/test.sh
836 passed; 0 failed; 12 skipped
test suite: PASSED
exit=0

-- after merge 2, orch-083-drop-event-fdatasync --
$ nix develop --command cargo build --workspace --all-targets
exit=0
$ nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 197 filtered out
exit=0
$ nix develop --command cargo test -p herd-hub --test crash_points -- --ignored --nocapture
308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)
test result: ok. 10 passed; 0 failed
exit=0

-- after merge 3, orch-record-fixes (prose only) --
$ ./scripts/check-docs.sh
docs ok
exit=0
```

The one test that exists after merge 2 and not after merge 1, which is
the whole difference between the two suite runs:

```text
test durable::tests::an_unsynced_append_writes_exactly_what_a_synced_one_writes ... ok
```

**Skips: 12, every one of them cgroup v2 delegation this host refuses,
and the count is a count and not a floor because the run completed
(D179).** Named:
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.

The ninth of those is ORCH-116: it is the only proof D119's clamp has,
and it has never run on the machine the clamp was biting.

**CI's skip count fell from 15 to 14**, and the one that left is the
assertion this batch was about: CI run 40535627242 reports
`the_derived_j_is_recorded_and_is_what_the_job_is_given ... ok` where it
used to report a `HERD-SKIPPED` line for it. That is ORCH-069's claim
confirmed from the other side — the assertion had never run anywhere,
and it now runs in both places. `docs/status.md` and the board's item 7
are corrected to 14.

## Risks and findings

1. **The board merged without a conflict, and that is worth saying out
   loud to the next integrator.** Two previous integrators warned about
   a hurried board conflict. This batch had none, which removes the
   warning's trigger and not its danger: three independent rewrites of
   one file landed with no human decision required. The check that
   catches a silent revert is the one run here — take each branch's own
   line and require it verbatim in the result — and it costs a minute.
2. **837, not 836.** The brief and ORCH-069's handoff both say 836, and
   836 is right for ORCH-069 alone; the final tree has one more test
   because ORCH-083 adds one. Both figures are above with the tree each
   belongs to. Nobody should read a later 837 as a drifting count.
3. **ORCH-107 is untouched and should not be read as cured.** The suite
   was green in both runs here, and both ran with no other heavy agent
   on the host. ORCH-107 is contention under several agents, so a green
   run on a quiet host is not evidence against it.
4. **`check-orchestration.sh`'s flaky case did not fire** this run (141
   passed, 0 failed, one run). That is ORCH-111's roughly 1-in-8, and
   one green run says nothing about it in either direction.
5. **The fence rule caught nothing here only because it was avoided.**
   Every fenced block in this handoff starts at column 1; indenting one
   inside a list item would have failed `check-docs.sh` on pasted
   evidence (ORCH-117).
6. **Other agents were compiling on this host.** `cargo` was run one at
   a time from this worktree; no process outside it was signalled, and
   no PID not started here was touched.

## Decisions needed

1. **Does step A ship without `--durable-waits`?** — *Recommendation:*
   yes, and record why: the flag has to reach `main.rs` and the hub's
   config, outside the ownership ORCH-083's row granted, and the commit
   is already the revert. D183 named the flag, so the deviation is on
   the board as **ORCH-113** rather than left in a handoff. *Fallback:*
   if the consumer must restore the sync without a rebuild, that is its
   own task and it carries step **B**'s flag too.
2. **Is the wider scope — every event line unsynced — wanted?** —
   *Recommendation:* ask it with ORCH-084 (step B), one answer for
   both; it is the same question about what D103's permitted lag means.
   Row **ORCH-114**. *Fallback:* leave it unasked; step A keeps its
   third either way.

Both are Alexa's and neither was taken here.

## Resume point

```sh
# the four rows this batch opened that nobody holds
grep -n '^| ORCH-11[45678] ' docs/orchestration/board.md
# the gate ORCH-084 has to keep green
nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle
```

## Processes

`nix develop --command cargo …` and the two shell gates, each in the
foreground in this worktree, each ended before the next began; no PID
outside this worktree was signalled and nothing was killed by name.
Own `target/` deleted after the push:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merge-batch-3/wt/target
```
