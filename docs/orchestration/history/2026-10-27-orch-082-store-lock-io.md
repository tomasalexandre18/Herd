# orch-082 — handoff 2026-10-27

- **Item:** ORCH-082 — the store's lock guards memory, never I/O (`07` S2)
- **Agent:** herd-step
- **Branch / commit:** `orch-082-store-lock-io` / `9655539` (the code), this
  file and the board in the commit after it; based on `origin/main`
  `2954c8e`, re-read with `git rev-parse origin/main` and not taken from
  the brief
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Where the lock is held across I/O, at `2954c8e`

Every `self.lock()` in `herd-hub/src/store.rs`, classified. "I/O under the
guard" means a filesystem call happens while the guard is alive.

| line | method | I/O under the guard | this task |
|---|---|---|---|
| 514 | `quarantined` | no — clones a map | — |
| 577 | `held_more` | no — one addition | — |
| 612 | `evict` | **yes** — `read_record`, `rename`, `create_dir`, `write_record`, `append_event` | left |
| 717 | `sweep_now` (tail) | no — stores the measured total | — |
| 872 | `say_what_the_sweep_did` | no — a flag and a line on stderr | — |
| 1064 | `recover` | **yes** — the whole startup walk | left |
| 1236 | `open_unless_over` | **yes** — `read_record`, the writer gate | left |
| 1541 | `mint` | **yes** — `mkdir`, `write_record`, `touch`, `append_event` | left |
| 1732 | `submit` | **yes** — `read_record` before the join | left |
| 1784 | `reading_answer` | no — one map lookup | — |
| 1795 | `remember_reading` | **yes** — writes `readings/answers.json` | left |
| 2353 | `chunks_recorded` | **yes** — `log_at` reads `log.seq` and `stat`s the log | left |
| 2449 | `quarantine_reason` | **yes** — `read_stored`; guard bound to `_live` | left |
| 2457 | `job` | **yes** — `read_record`; guard bound to `_live` | **fixed** |
| 2462 | `job_log` | **yes** — reads the log; guard bound to `_live` | left |
| 2471 | `job_events` | **yes** — reads and parses `events.jsonl`; guard bound to `_live` | left |
| 2514 | `unfinished_jobs` | **yes** — `N` record reads under one hold | **fixed** |
| 2533 | `note_outputs_fetched` | **yes** — `read_record` then `write_record` | left |
| 2555, 2562 | `sweep_outputs` | no — the one-at-a-time flag | — |

Twelve of nineteen sites hold the guard across a filesystem call. Four of
those twelve hold a guard they never read — `quarantine_reason`, `job`,
`job_log`, `job_events`, each `let _live = self.lock()?;` — which is the
clearest form of the fault: the lock is there for exclusion and for nothing
else.

`collections_owed` (`:2568`) takes no lock of its own. It reaches `job`
once per directory through `collection_owed` (`:2590`), and `job` again
through `output_holdings` for the few that get that far — which is the
20 001 of the row, and the reason `06` phase 1f measures ×8.60 at eight
callers against a mutex-free control of ×1.66.

## Done

Three methods, one new private helper, one new test. `store.rs` only:
194 insertions, 29 deletions in the code commit.

1. **`JobStore::job` takes no lock.** It is `self.read_record(id)` and a
   paragraph saying why. This is the row's "`read_record` comes out from
   under the guard", and it is what takes the lock out of the `T` walk,
   because the walk reaches the lock only through this method.
2. **`collection_owed` answers from the record it has already read**, via a
   new private `holdings_of(&self, id, record)`. `JobStore::output_holdings`
   is now that helper with a record read for it, so the public behaviour is
   unchanged and `collection_owed` stops reading the same `job.json` twice.
   `collections_owed` therefore takes the store's lock **zero** times, and
   is **not** restructured into one long hold — the row forbids both.
3. **`unfinished_jobs` copies `pending` under the lock and lets it go
   before the first record read**, then filters terminal records. The
   filter is not tidying: `move_to` writes a terminal record *before* it
   removes the job from `pending`, both under one hold, so the old hold was
   what made (`pending`, records) one snapshot and every record returned
   non-terminal. Without the filter a job ending mid-walk would be handed
   to the dispatch pass, `open_for_writing` would refuse it by name, and
   `07 §2` constraint 5 says one refused record aborts the whole pass.
4. **A test that proves it rather than describing it**, in `store.rs`:
   `every_record_read_answers_while_another_thread_holds_the_store_lock`.
   The lock is taken on the test's thread and kept; a scoped thread then
   calls `job`, `collection_owed`, `collections_owed` and `output_holdings`
   and reports back; the guard is released before anything is asserted, so
   a read that has gone back under it fails the test instead of hanging the
   suite. **Verified as a negative control:** with `let _live =
   self.lock()?;` put back in `job`, the test goes red in 10.05 s with
   `Timeout`, and the run returns rather than wedging.

## The invariants the change leans on

Named, because taking a read out from under a lock changes what a caller
may assume.

1. **`job.json` is published only by `rename`.** The only writer of
   `RECORD_FILE` is `JobDirStore::write_record` (`store.rs:1301`) →
   `write_json` → `durable::write_file` → `durable::write_file_with`, which
   writes a temporary, `sync_all`s it, renames it over the destination and
   `fsync`s the directory (`durable.rs:247-273`). A reader therefore sees
   the whole old record or the whole new one, never a torn one. That is
   `durable.rs`'s opening paragraph, it is the module's reason to exist, and
   `durable`'s own tests hold it. **Checked, not assumed:**
   `grep -n RECORD_FILE herd-hub/src/store.rs` outside the tests gives
   `:1256` (the read path) and `:1302` (the one write).
2. **The guard was per call, so no caller had cross-call consistency.**
   `job`, `quarantine_reason`, `job_log` and `job_events` each took and
   released the guard inside one call. Nothing that happens between two
   store reads is new, because the lock never spanned two.
3. **Every writer re-reads the record under the lock.** `open_unless_over`
   (`:1235`) is the one gate, and it keeps its lock-across-the-read exactly
   because that read *is* the read-modify-write. So no decision is taken on
   a record a reader handed back: a stale read can at worst cause a refusal
   the caller already has to handle.
4. **`move_to` clears `pending` after writing the terminal record**, both
   under one hold (`:1494-1512`). That is the one thing `unfinished_jobs`'s
   long hold was buying, and the terminal filter restores it.
5. **`collections_owed`'s answer is advisory by construction.** It returns
   a `Vec` its caller consumes with every lock released. For the first of
   20 000 entries the answer is already 200 ms old before the caller sees
   it, so the agreement between a record and the directory beside it that
   the per-call guard gave *inside* `collection_owed` was discarded the
   instant the walk moved on. This is the proof that the eviction race
   below is not made newly reachable by the change.

## What a concurrent writer can now interleave

- **In `job`: nothing.** One `read_to_string` of one atomically published
  file. What a writer can do is finish a write a few microseconds after
  the read began, so the record returned is that much older — already true
  the instant the old guard was dropped.
- **In `unfinished_jobs`: the records are no longer read at one instant.**
  The list can carry one job's priority or waiting reason as of the start
  of the walk and another's as of its end. That costs nothing a caller
  had: the queue is recomputed from this list at every pass, `queue::order`
  is a total order on priority and id, and a raise landing mid-walk takes
  effect at this pass or the next either way. A job that *joins* `pending`
  after the snapshot is absent — already true of every caller the moment
  the function returned. A job that *leaves* it by ending is filtered.
- **In `collection_owed`: the eviction window widens by microseconds and
  not by a class.** `evict` (`:607`) renames `files/` away and only then
  writes the record that says `removed`, all under one hold, so a reader
  outside the lock can see "files gone, record does not say removed" and
  report a collection owed that the sweeper has just taken — which is the
  fight D152's `removed` field exists to stop. It is **not newly
  reachable**: by invariant 5 above, the caller of `collections_owed` acts
  on every entry with no lock held, up to 200 ms after the entry was
  computed, so the same stale entry was already reachable through the gap
  between the walk and its consumer. What the change adds is the few
  microseconds of one record read. **It cannot be closed inside
  `collection_owed`** with or without a lock, because the answer outlives
  any hold; closing it needs the consumer of the list to re-check, which
  is not this row. Recorded in *Risks* below.

## Not done, and why

- **The ten other sites that hold the guard across I/O.** The row names
  `read_record` and `collections_owed`; a long diff in `store.rs` is a long
  conflict for ORCH-084 and ORCH-087, which both own this file next.
  Specifically:
  - `open_unless_over`, `mint`, `submit`, `evict`, `recover`,
    `remember_reading`, `note_outputs_fetched` are **writers**. The lock
    across the read is what makes their read-modify-write atomic; taking it
    out would lose updates. Each needs a different mechanism — a per-record
    lock, or a record cache in `Live` — and that is a design, not a
    refactor.
  - `quarantine_reason` is a pure reader of the same `job.json`, under the
    same rename proof, and is a one-line follow-up. Left out only because
    it is neither in the row nor in the `T` walk.
  - `job_log` and `job_events` are pure readers whose files are **not**
    published by `rename`: the log is appended and can be shortened by
    `log_at`'s `truncate`, and `events.jsonl` is appended one line per
    `write_all`. A lockless read of either returns a prefix, which both
    callers already tolerate — `job_events` discards an unterminated tail
    by design (`:2480-2490`) — but `job_log`'s bytes are spliced with a
    live stream in `Server::attach`, and a snapshot containing part of a
    chunk is a new failure mode. Not worth taking for a method that is not
    in the `T` walk.
  - `chunks_recorded` reads `log.seq` through `log_at`, which also *writes*
    `Live.log_chunks`: it is a writer in reader's clothing.
- **No `docs/decisions.md` entry, no `D-NEXT`** — the brief forbids both.
- **Only the ORCH-082 board row touched**, as instructed. The board's prose
  at §3 of the closing notes still reads "**ORCH-082** is the one of the
  four still open"; that sentence is now stale and is the merger's or the
  orchestrator's to correct, not an agent's.

## Gates

All four Rust gates plus both scripts, on the final tree. Every one was
redirected to a file and the file read; none was piped to `head` or `tail`.

`nix develop --command cargo fmt --all --check` — rc 0, no output beyond
the dirty-tree warning.

`nix develop --command cargo build --workspace`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.06s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```
    Checking herd-hub v0.3.7 (.../orch-082/wt/herd-hub)
    Checking herd-runner v0.3.7 (.../orch-082/wt/herd-runner)
    Checking herdctl v0.3.7 (.../orch-082/wt/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.86s
```

`nix develop --command ./scripts/test.sh`:

```
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
test.sh rc=0
```

837 passed, 0 failed, 12 skipped — the count the brief names, and a count
rather than a floor. The twelve are all the same host fact and are copied
as skips, never counted as passes (D27):

- `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
- `a_declared_ceiling_changes_what_the_inert_weight_line_says`
- `a_job_cannot_fork_past_its_own_pid_ceiling`
- `a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`
- `a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`
- `a_job_s_record_carries_the_pressure_it_ran_under`
- `an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`
- `an_io_weight_that_cannot_bite_is_said_out_loud`
- `a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`
- `no_declared_ceiling_leaves_io_max_empty`
- `no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`
- `the_runner_says_which_memory_ceiling_it_enforces`

Every one of them is "cgroup v2 delegation is unavailable" or "this host
has no cgroup v2 subtree for a job to carry an io.max in". Nothing in this
change touches the runner or confinement.

`./scripts/check-docs.sh` — rc 0:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — rc 0:

```
check-orchestration: 141 passed, 0 failed
```

**The ordering oracle, which must not be weakened** (`ORCH-079`),
`nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle`:

```
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.08s
```

Green before and after, unmodified. The tripwire's counts — ten synced
sites, one unsynced, and their sum — are untouched: nothing in this change
goes near `durable.rs` or moves a call between the two doors.

**The 308-state crash enumeration**, `#[ignore]`d:

```sh
nix develop --command cargo test -p herd-hub --test crash_points -- --ignored
```

Run on the base and on the change, the six figures identical:

| figure | before | after |
|---|---|---|
| states built | 308 | 308 |
| refuse the whole state root at `open` | 176 | 176 |
| open with the job silently gone from the queue | 44 | 44 |
| leave `events.jsonl` ahead of `job.json` | 4 | 4 |
| leave a stale temporary nothing removes | 99 | 99 |
| quarantined | 0 | 0 |

## Measurements

`06`'s own phase 1f harness, `lock_wait_under_contention` in
`herd-hub/tests/junction_breakdown.rs`, unmodified. The two test binaries
were built separately and run back to back from the saved binaries, so the
comparison is the same harness on the same host within three minutes.

Host one-minute load: **1.74 → 1.52** across the before run, **1.19 →
2.39** across the after run. The single-caller figure is the control for
load and it is the same in both runs to within 2.5 %, so the comparison
holds. The absolute numbers are **not** `06`'s and the gap is named under
*Risks*.

`N` = 1 000, `T` = 0 — 1 001 directories:

| callers | before | after | control | store ratio before → after |
|---|---|---|---|---|
| 1 | 55.66 ms | 54.29 ms | 3.2–3.4 ms | ×1.00 → ×1.00 |
| 2 | 113.91 ms | 55.42 ms | 3.2–3.4 ms | ×2.05 → ×1.02 |
| 4 | 233.70 ms | 57.34 ms | 3.2 ms | ×4.20 → ×1.06 |
| 8 | 472.33 ms | 92.71 ms | 5.58 ms | ×8.49 → **×1.71** |

`N` = 50, `T` = 20 000 — 20 051 directories:

| callers | before | after | control | store ratio before → after |
|---|---|---|---|---|
| 1 | 1.179 s | 1.178 s | 67–68 ms | ×1.00 → ×1.00 |
| 2 | 2.500 s | 1.181 s | 71 ms | ×2.12 → ×1.00 |
| 4 | 5.029 s | 1.224 s | 76 ms | ×4.27 → ×1.04 |
| 8 | 9.994 s | 1.741 s | 119 ms | ×8.48 → **×1.48** |

The real mix — one history walk beside one queue read:

| pairing | before | after |
|---|---|---|
| `collections_owed` beside a queue read, `N` = 1 000 | +57.60 ms | +203.8 µs |
| `unfinished_jobs` beside a history walk, `N` = 1 000 | +534.3 µs | +1.41 ms |
| `collections_owed` beside a queue read, `T` = 20 000 | +0 ns | +19.31 ms |
| `unfinished_jobs` beside a history walk, `T` = 20 000 | +87.0 µs | +27.8 µs |

**Reading the tables, including the parts that are not a win.**

- **V5 at `T` = 20 000 is met outright:** ×1.48 against the criterion's
  ≤ 1.5×, and the store now scales *better* than the mutex-free control
  (×1.75), because it does more CPU per directory and so parallelises
  better.
- **V5 at `N` = 1 000 is met in substance and not to the letter:** ×1.71
  against ≤ 1.5×. But the **control itself** is ×1.66 at eight callers on
  this host, so ≤ 1.5× is unreachable here by any implementation; the store
  is now within 3 % of an implementation that takes no lock at all, which
  is what the criterion was written to test. Reported as what it is rather
  than rounded down.
- **The second exit criterion — "no writer is blocked more than 1 ms by a
  reader" — is derived, not measured.** The harness measures readers
  against readers. After the change the only lock a reader takes is
  `unfinished_jobs`'s copy of `pending` into a `Vec`; the longest hold in
  the store by any reader is therefore a `BTreeSet` walk of `N` `u64`s,
  microseconds at `N` = 1 000. `collections_owed` takes none. A direct
  measurement of writer-blocking would need a harness phase 1f does not
  have.
- **The honest small number: one caller saves nothing.** 55.66 ms →
  54.29 ms at `N` = 1 000 and 1.179 s → 1.178 s at `T` = 20 000 — a
  difference inside the noise. The gain is entirely in concurrency, which
  is exactly `07 §1.3`'s own first qualification: post D at a real single
  junction is 600 ns to 1.6 µs, so the lock was never a measured cost of
  one junction. **The value of this row is prospective** — `05 §4.3`'s
  convoy, which ORCH-087's keeper would otherwise inherit.
- **Two rows of the mix got slightly worse, and both are real
  parallelism rather than regression.** At `T` = 20 000 the queue read used
  to be serialised behind the entire 1.18 s walk and so cost it nothing
  measurable; now the two really run at once and compete for CPU, which is
  +19.31 ms on 1.17 s, 1.6 %. The `N` = 1 000 row's +1.41 ms against
  +534.3 µs is 2.5 % of a 56 ms call and within this host's noise at the
  load recorded.

## Risks and findings

1. **The absolute figures are 5.8× `06`'s on the store side while the
   control matches `06`.** One caller, 1 001 directories: 55.66 ms here
   against `06`'s 9.56 ms, while the mutex-free control is 3.2 ms here
   against `06`'s 4.77 ms. So the disk is not the difference. 55.66 ms /
   1 001 = **55.6 µs per record through the store** against 3.2 µs for the
   `read_to_string` the control does, which puts ~94 % of the whole `T`
   walk in per-record CPU and not in the disk or the lock. The likely cause
   is in the code: `read_stored` (`:1255`) parses the **same text three
   times** — `VersionProbe`, then `DurableLabels`, then `RecordFile` — and
   builds a `LabelSet` in between, in a debug build. I did not change it
   and did not measure it directly, so the attribution is a hypothesis with
   an arithmetic bound, not a result. **It is the next lever on
   `collections_owed` and it is larger than the lock was**, which is worth
   a row of its own beside ORCH-021.
2. **The eviction window described under *What a concurrent writer can now
   interleave* is widened by microseconds and is not closable where it
   lives.** A `collections_owed` entry can name a collection the sweeper
   took, and `write_output_chunk` (`:1978`) does **not** check
   `outputs.removed`, so a resend would refill `files/` beside a record
   that says `removed` — and `sweep_now` counts such bytes in
   `could_not_free` for ever (`:757`), so the budget would lose them
   permanently. I satisfied myself the class is already reachable without
   this change (invariant 5: the caller acts on the list with no lock held,
   up to 200 ms late) and that no hold inside `collection_owed` can close
   it, because the answer outlives the hold. **The real fix is a `removed`
   check in `write_output_chunk`** — three lines, refusing a chunk for a
   collection the sweeper has taken — and it belongs to whoever owns D152's
   sweeper, not to this row.
3. **`unfinished_jobs` no longer returns a single-instant snapshot.** The
   terminal filter restores the one postcondition the pass depends on, and
   the mixed-freshness consequence is argued above, but it is an argument
   and not a test. If a later reader of this file finds a caller that needs
   one instant, the fix is to take the lock around the walk again for that
   caller alone and leave `job` as it is — the two changes are independent.
4. **Nothing was weakened to pass.** The oracle is unmodified and green,
   the enumeration's six figures are identical, and the new test was
   checked against its own negative control.

## Decisions needed

None for this row. Two findings above are candidates for rows and neither
is mine to open: the `read_stored` triple parse (finding 1) and the
`removed` check in `write_output_chunk` (finding 2).

## Resume point

```sh
git fetch origin && git log --oneline origin/main..orch-082-store-lock-io
```

The branch is pushed and carries two commits: the code, then this file and
the board row. Nothing is owed on it. For `herd-merger`: it shares
`herd-hub/src/store.rs` with nothing else in flight that I could see, and
`docs/orchestration/board.md` only in the ORCH-082 row.

Next in `07`'s chain is **ORCH-087, the keeper** (S3), whose row names
ORCH-082 as its dependency; that dependency is now met.

## Processes

PIDs started: `nix develop --command cargo …` builds and test runs, and two
runs of the saved `junction_breakdown` binary, all in the foreground of my
own shell and all exited before the next step. None left behind, nothing
killed, no `pkill` and no kill by name. The `herd-runner` and `herd-hub`
processes about a day old were not touched: they are the consumer's own
instance.

Own `target/` deleted once the branch was pushed:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-082/wt/target
```
