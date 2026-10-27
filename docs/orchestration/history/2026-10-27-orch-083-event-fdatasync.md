# orch-083 — handoff 2026-10-27

- **Item:** ORCH-083 — durability tier, step A: drop the event `fdatasync`
- **Agent:** herd-step
- **Branch / commit:** `orch-083-drop-event-fdatasync` / `4f16a32` (the code),
  this file in the commit after it; based on `origin/main` `64cc532` (re-read
  with `git rev-parse origin/main`, not taken from the brief)
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

**The `waiting` line of `events.jsonl` no longer waits for the disk. Nothing
else moved.** Five files, 287 insertions and 34 deletions, of which the
product change is about twenty lines; the rest is the oracle, the two
harnesses and the reasons.

### The change, in the two files D183 names

- `herd-hub/src/durable.rs` — `append_line` splits into `append_line`
  (unchanged behaviour: `write_all`, then `sync_data`) and
  `append_line_unsynced` (the same `write_all`, no sync), over one shared
  body `append_one_line` so the two cannot drift. The
  `watch::issuing(AppendIssued)` note is made on **both** paths, at the same
  instant, so the observation D103 rests on is unchanged. One new unit test
  asserts the two doors write byte-identical files.
- `herd-hub/src/store.rs` — a second appender, `append_event_unsynced`,
  beside `append_event`; both build the line through one new
  `event_line`. `record_waiting` is the one caller of the new one. The
  comment above `write_record` now says *why* the order is what licenses the
  unsynced line, rather than only that the order exists.

**Scope, and why it is narrower than the brief's wording.** The brief said
"the event line's `fdatasync` goes … that step disappears from every trace".
What was implemented is the authority's own scope: **`record_waiting`'s line
only**, twelve of the thirteen event lines still synced. D183 authorises
*"dropping the event `fdatasync` of `record_waiting`"*; board row ORCH-083
says *"`record_waiting`'s `events.jsonl` line goes through
`durable::append_bytes`"* and *"owns one line of `herd-hub/src/durable.rs`
plus `record_waiting`"*; `10 §6`'s task table says `store.rs`
**(`record_waiting` only)**. Three independent places, one scope. See
**Risks and findings** for why the wider reading is not a free generalisation
of this one — it is not, and it would have needed its own decision.

### The oracle: what changed, and why it is not a weakening

`herd-hub/src/store/ordering_oracle.rs`:

- a second expected event shape, `EVENT_UNSYNCED`, holding
  `[(AppendIssued, Events)]` where `EVENT` holds `[(AppendIssued, Events),
  (AppendSynced, Events)]`, and a `Part::EventUnsynced` that selects it;
- call 4 of 16 — `record_waiting` — expects `[Record, EventUnsynced]`.
  **The other fifteen are untouched.**

Nothing that was asserted is asserted less:

1. `check_the_rule` is **not edited**. The `Written` state machine still
   requires `Nothing → BytesSynced → Renamed → Durable`, and still refuses
   `AppendIssued` in any state but `Durable`. For all 16 calls, including
   this one, the record's `sync_all` must have returned before its `rename`
   was issued, the `rename` before the directory `fsync` was issued, and the
   directory `fsync` before the **first byte** of the event line was issued.
2. The per-call exact-sequence equality is **not loosened**; it is restated.
   `record_waiting`'s expectation went from five operations to four, and the
   four are still asserted in order, by identity, against the observed trace.
   What is gone is one step that came *after* the append was issued, because
   there is no longer a syscall there to observe. D103 is a statement about
   what has returned before the append is issued; it says nothing about
   afterwards.
3. The expectation is still **written out per call, not derived**, so moving
   a *transition* onto the unsynced door cannot pass quietly — proved red
   below.
4. The tripwire gained strength. `EVENT_APPEND_SITES` is 10,
   `UNSYNCED_EVENT_APPEND_SITES` is **1**, and a third assertion counts the
   shared prefix `self.append_event` against their sum, so a third door would
   not hide between the two exact counts. Step A's narrowness is now
   mechanical: the unsynced appender cannot acquire a second caller without
   this test naming it.
5. The zero-sized `Synced` witness is untouched and still bites on both
   sides — proved by two compile failures below. In `append_one_line` the
   sync and its note live together inside the `Durably::Yes` arm, so deleting
   the sync deletes the only value that can be noted for it.

One assertion was **added**: the thirteen lines read back at the end of the
test must include the unsynced `waiting` line, with its reason. An unsynced
append is still immediately visible to a reader (the page cache is the
kernel's), which is what E-53 asks for and what step A must not have cost.

### Two harnesses, corrected so they do not lie about the code

Neither changes an assertion, so neither can move the figures it reports.

- `herd-hub/tests/sync_prices.rs` — the control row printed `kept = 3` for
  the store's own write path and compared it with the replica's `RDE`. Since
  step A the store performs `RD-`. A new `Syncs::STORE` names the subset the
  store actually runs; the row and the control line read it, and the old
  `RDE` figure is still printed, labelled as what the store did before
  ORCH-083. **All eight subsets are still timed** — the lattice is the
  measurement, and a file that can price a step before it is taken should
  keep being able to.
- `herd-hub/tests/crash_points.rs` — the "write under study" header said
  step 2 was `append_event` → `durable::append_line` with **E**
  `file.sync_data()`. It now says what the code does, plus the paragraph that
  matters: **the enumeration and its 308 outcomes are unchanged by step A**,
  because the states are written byte for byte rather than crashed into
  existence and every byte prefix of `events.jsonl` was already enumerated.
  And `an_event_ahead_of_the_record_is_neither_seen_nor_repaired` now says in
  its doc comment that ORCH-083 did **not** make its state reachable — the
  unreachability rests on `sync_dir` returning, not on the event being
  synced — so that case stays an assertion about what step **B** would cost.

### The record's two syncs, and `store.json`

- **`write_file_with` is byte-for-byte unchanged.** `sync_all_of` on the
  temporary and `sync_dir_of` on the parent both still run, both still hand
  back a `Synced`, both notes still compile only because they do.
  `git diff origin/main -- herd-hub/src/durable.rs` touches nothing between
  `pub fn write_file(` and the end of `write_file_with`.
- **ORCH-094's `ids_minted` write is untouched and still fully synced.**
  `mark_minted` (`store.rs`) goes through `write_json` →
  `durable::write_file_with`, so it is still a temporary, an `fsync`, a
  rename and a directory `fsync`. The diff of `herd-hub/src/store.rs` against
  `origin/main` shows no line of it, of `write_json`, of `STORE_FILE` or of
  `recover`'s high-water handling. The one write whose loss re-issues a job
  id stays in the decisive tier, as its author asked.

### Measured, not quoted

**The saving.** `sync_prices.rs` phase 2, ext4 on NVMe (`/mnt/ssd_980`,
`/dev/nvme0n1p1`, `rw,relatime,stripe=32` — the class the deployment runs
on, ORCH-053), 60 samples per subset, `--release`. The "before" run is the
binary compiled from `origin/main` `64cc532` before any edit; the "after" run
is this branch. The measuring code is identical in both: the only edit to
this file is the control row's label and a constant it reads.

| | store `record_waiting`, median | load (1 min) at the run |
|---|---|---|
| **before**, `origin/main` `64cc532` | **3.10 ms** | 1.65 |
| **after**, this branch | **2.10 ms** | 1.88 |

**1.00 ms off a 3.10 ms write: 32.3 %**, against the ~31 % D183 predicted
from `11 §6`. The replica's own lattice is the reference and is measured on
both runs: `RDE` 3.00 → 3.02 ms, `RD-` 2.09 → 2.00 ms, `R--` 1.02 →
1.10 ms, `---` 50.6 → 49.4 µs. `11 §6` had 3.00 and 2.10 ms for the two
subsets that matter, so the host reproduced itself and the prediction held.

The control still holds in the same way it did before: the store sits
**100 µs above its own subset's replica** in both runs — 3.10 against `RDE`
3.00 before, 2.10 against `RD-` 2.00 after. The offset did not move, which
is a second, independent sign that one sync and nothing else came out.

**The enumeration.** `crash_points.rs` on this branch: **308 states built,
176 refuse the whole state root, 44 open with the job silently gone, 4 leave
`events.jsonl` ahead of `job.json`, 99 leave a stale temporary, 0 are
quarantined** — and the 308-row table plus those five counters are
**byte-for-byte identical** to the run on `origin/main` `64cc532` before the
change. `diff` of the two outputs, with host-specific temporary paths and the
timing line removed, is empty. The expected answer held: the record's two
syncs still order the pair, so step A adds no reachable state and removes
none. The four `events.jsonl`-ahead states are still exactly `Older ×
Boundary(3) × 4 temporaries`, still what step **B** would cost, and ORCH-084
is still held behind them.

## Not done, and why

- **`--durable-waits`, default off.** D183's reversibility paragraph and the
  board row both name this flag, and it is not here. It cannot be built
  inside the ownership the same row grants — one line of `durable.rs` plus
  `record_waiting` — because a flag has to reach `main.rs`, the hub's config
  and `JobDirStore`. It is also "something else" where the brief said "and
  nothing else". Reversibility is still high and still declared: the revert
  is this commit. A question for the orchestrator is in **Decisions needed**.
- **`docs/coverage.md` E-53.** `10 §6`'s table says the tier touches D103's
  scope "**from step B on**", and E-53's row says `events.jsonl` "may be one
  transition behind it, never ahead". Step A touches no transition line, so
  the row is still true as written and was left alone. Step B is what makes
  it need a sentence.
- **No `docs/decisions.md` entry and no `D-NEXT`.** D183 already carries the
  decision; only `herd-merger` appends.
- **No dossier piece.** The evidence is in this handoff. A numbered
  `docs/remediation/` piece would be the project's usual home for the
  before/after figures; the brief did not ask for one and it is offered
  rather than assumed.
- **No PR, no merge, no tag, no release**, and nothing written to `main`.

## Gates

All four Rust gates plus both scripts ran, on this branch, in this worktree,
under `nix develop`. `cargo build --workspace --all-targets` ran **before**
`./scripts/test.sh`, as the brief requires.

See the verbatim blocks below. **Two known-red host failures are reported
red and are not mine:** `herd-hub/tests/slots.rs:744` and
`herd-hub/tests/admission.rs:419`, both ORCH-069, both failing on a figure
derived from this host's live `MemTotal`. Neither is skipped here and neither
is claimed green on CI — `slots.rs:744` is *skipped* there, and by D27 a skip
is never a pass.

### `cargo fmt --all --check`

```text
(no output, exit 0)
```

### `cargo clippy --workspace --all-targets -- -D warnings`

```text
    Checking herd-hub v0.3.7 (…/scratchpad/orch-083/wt/herd-hub)
    Checking herd-runner v0.3.7 (…/scratchpad/orch-083/wt/herd-runner)
    Checking herdctl v0.3.7 (…/scratchpad/orch-083/wt/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 29.22s
(exit 0)
```

### `cargo build --workspace --all-targets`

Run **before** `./scripts/test.sh`, as the brief requires, so the
`real_herd` targets do not build the binaries they drive inside their own
30 s patience.

```text
   Compiling herd-hub v0.3.7 (…/scratchpad/orch-083/wt/herd-hub)
   Compiling herdctl v0.3.7 (…/scratchpad/orch-083/wt/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.76s
(exit 0)
```

### `./scripts/test.sh` — **RED, on the two known host failures**

Load `1.95 3.52 2.86` before, `3.64 4.27 3.28` after. Exit 101.

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`

test suite: FAILED (cargo test exited 101)
```

**Failure 1 — `herd-hub/tests/admission.rs:419`, ORCH-069.** Not a timeout:
it fails on a figure derived from this host's live `MemTotal`.

```text
---- a_killed_hub_reads_the_reservations_back_off_the_records stdout ----

thread 'a_killed_hub_reads_the_reservations_back_off_the_records' panicked at herd-hub/tests/admission.rs:419:5:
the inherited job's 4 GiB of declared memory must still be held: queued 2nd of 2 at priority step, rising to merger in 30m; it is waiting because no connected runner can run it: lender admits at most 4.9 GiB of memory (5.9 GiB lent, 1 GiB kept as its own floor) and this job needs 5 GiB, so it can never fit there however empty it gets; 1 ahead of it: j0000000003 (step)

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.25s
```

**Failure 2 — `herd-hub/tests/slots.rs:744`, ORCH-069.** Same family, same
cause, and visibly a figure: `-j3` where the expectation is `-j7`, derived
from the `7195 MiB` and `1792 MiB per build thread` read off this host.

```text
---- the_derived_j_is_recorded_and_is_what_the_job_is_given stdout ----

thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at herd-hub/tests/slots.rs:744:9:
assertion `left == right` failed: the figure E-22 asks for, in the process's own words. …
herd-hub: j0000000002 is attempt 1 on runner wsl-pc: claimed by wsl-pc: labels build; build slot 1 of 1; -j 3 from 7195 MiB and 20 threads, 5396 MiB for this slot (3 of 4), 1792 MiB per build thread: memory allows 3, threads allow 20; admitted 5.2 GiB of memory, 380 MiB free of 5.6 GiB after it (7 GiB lent, 1.4 GiB kept as its own floor); …
  left: "built with -j3"
 right: "built with -j7"

test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.36s
```

**Neither is mine, and the reason is in the diff and not only in the
attribution.** Nothing in this change touches memory admission, build-slot
arithmetic, `-j` derivation or `MemTotal`; the whole diff is the durability
of one appended line, the oracle that watches it, and two harness headers.
Both are **ORCH-069**, both were reached independently by two integrators
today, the second on a run where nothing timed out. ORCH-107, the separate
contention family, did not appear here. **I did not re-run these two targets
against `origin/main` to re-confirm the attribution**, which is said rather
than implied.

**No claim of "green on CI" is made about `slots.rs:744`.** It is *skipped*
there, and by D27 a skip is never a pass.

Everything else was green, including the oracle inside the library suite:

```text
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test result: ok. 199 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.03s
```

### Skips — **a floor of 12, not a count** (D179), and none of them a pass

The suite did not finish, so `test.sh` printed its own warning, and this
file repeats it rather than counting:

```text
==============================================================
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
==============================================================
```

All twelve are one host limitation — no cgroup v2 delegation, and for two of
them no hard IO ceiling this host will take — and all twelve are in
`herd-runner`'s confinement tests, none in anything this change touches:

```text
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces
```

### The oracle, green on this branch unmodified

```text
running 10 tests
test durable::tests::appended_lines_arrive_whole_and_in_order ... ok
test durable::tests::an_unsynced_append_writes_exactly_what_a_synced_one_writes ... ok
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 189 filtered out; finished in 0.49s
```

Load `2.66 2.03 1.74` before, `2.53 2.01 1.73` after.

### The oracle, proved red four ways

All four on a **scratch copy of the tree, outside the branch and outside
git**, at `…/scratchpad/orch-083/red/`, with its own `CARGO_TARGET_DIR`.
Confirmed not a repository: `git rev-parse --show-toplevel` there answered
*"fatal: not a git repository"*. Nothing below is on the branch or in a
commit, and the copy was deleted once the four runs were recorded. Two edits
to the blocks and no others: the temporary state root is written `<root>`,
and the panicking thread's name, which is the test's own long name, is
written `'…'`.

**Red 1 — `write_file_with`'s `sync_dir_of(parent)` and its note deleted.**
The ORCH-084 hazard itself; the oracle still catches it after step A. Load
`2.99 2.23 1.83`.

```text
thread '…' panicked at herd-hub/src/store/ordering_oracle.rs:268:17:
assertion `left == right` failed: D103 VIOLATED — submit — a job is minted: the first byte of an event line was issued while the record write of <root>/jobs/j0000000001/job.json had only reached Renamed. …
The trace of this call:
   0. TemporarySynced store.json
   1. Renamed store.json
   2. TemporarySynced job.json
   3. Renamed job.json
   4. AppendIssued events.jsonl
   5. AppendSynced events.jsonl
   6. TemporarySynced state.json
   7. Renamed state.json

  left: Renamed
 right: Durable

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.04s
```

**Red 2 — `durable.rs` restored; `record_waiting`'s two writes swapped.** The
one call step A touched, and the oracle still names it by name. Note the
four-step trace: the unsynced shape, asserted in order, with the append first
and therefore red.

```text
thread '…' panicked at herd-hub/src/store/ordering_oracle.rs:268:17:
assertion `left == right` failed: D103 VIOLATED — record_waiting — why a queued job is not starting: the first byte of an event line was issued while the record write of <root>/jobs/j0000000001/job.json had only reached Nothing. …
The trace of this call:
   0. AppendIssued events.jsonl
   1. TemporarySynced job.json
   2. Renamed job.json
   3. DirectorySynced job.json

  left: Nothing
 right: Durable

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.06s
```

**Red 3 — a *transition* moved onto the unsynced door.** `move_to`'s
`self.append_event(` changed to `self.append_event_unsynced(`, nothing else.
**This is the hazard step A creates, and it goes red twice over.** It is the
run that answers "was the oracle weakened": what step A allowed for one line
is caught the moment it reaches a second.

```text
thread '…' panicked at herd-hub/src/store/ordering_oracle.rs:691:5:
assertion `left == right` failed: store.rs no longer has 10 event appenders. D103 binds every one of them to the record write beside it; a new one has to be driven by the ordering oracle above before this number moves.
  left: 9
 right: 10

thread '…' panicked at herd-hub/src/store/ordering_oracle.rs:353:9:
assertion `left == right` failed: begin_attempt — the job is assigned and an attempt is opened: the record and event operations of this call are not the sequence the oracle was told to expect
observed:
   0. TemporarySynced job.json
   1. Renamed job.json
   2. DirectorySynced job.json
   3. AppendIssued events.jsonl
   4. TemporarySynced job.json
   5. Renamed job.json
   6. DirectorySynced job.json
   7. AppendIssued events.jsonl
   8. AppendSynced events.jsonl

  left:  [(TemporarySynced, Record), (Renamed, Record), (DirectorySynced, Record), (AppendIssued, Events), (TemporarySynced, Record), …]
 right: [(TemporarySynced, Record), (Renamed, Record), (DirectorySynced, Record), (AppendIssued, Events), (AppendSynced, Events), …]

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.05s
```

**Red 4 — the zero-sized `Synced` witness, twice, by compile failure.** The
property the oracle's own author had to add because his first red run stayed
green: delete a sync, leave its note, and the tree stops compiling.

```text
=== 4a: sync_dir_of deleted from write_file_with, its note LEFT BEHIND ===
error[E0425]: cannot find value `name` in this scope
   --> herd-hub/src/durable.rs:271:56
    |
271 |     watch::returned(watch::Op::DirectorySynced, path, &name);
    |                                                        ^^^^ not found in this scope
error: could not compile `herd-hub` (lib) due to 1 previous error

=== 4b: sync_data_of deleted from append_one_line's synced arm, note LEFT BEHIND ===
error[E0308]: mismatched types
   --> herd-hub/src/durable.rs:341:60
    |
341 |             watch::returned(watch::Op::AppendSynced, path, &line);
    |             ---------------                                ^^^^^ expected `&Synced`, found `&&str`
    |
    = note: expected reference `&Synced`
               found reference `&&str`
error: could not compile `herd-hub` (lib) due to 1 previous error
```

**4b is the stronger of the two, and it is the one step A had to not break.**
The `line` parameter of `append_one_line` shadows the deleted binding, so the
note still *resolves* — and `Synced` is what rejects it. The witness survived
being moved inside a `match` arm, which is the only structural change step A
made to a sync site.

### `./scripts/check-docs.sh`

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

### `./scripts/check-orchestration.sh`

```text
check-orchestration: 141 passed, 0 failed
```

## Risks and findings

1. **The wider reading of step A is not a free generalisation of it, and
   should not be taken by accident.** Dropping the event `fdatasync` for
   *every* event line would still be safe against the letter of "never
   ahead" — the record's two syncs return first on every writer, which is
   `10 §1`'s closing paragraph. But D103's permitted lag is *"may be one
   transition behind the record"*, and an unsynced `events.jsonl` can lose
   **many** trailing lines to a power cut, so a hub could come back with a
   `job.json` that says `finished` and an `events.jsonl` whose last line says
   `assigned`. That is behind, not ahead, and it is still more than one
   transition behind. It is a scope question for D103 and therefore Alexa's,
   exactly as `10 §1` says of step B. Step A as taken does not raise it: a
   `waiting` line is not a transition (`10 §1` F-1), so no count of
   transitions moves.
2. **Nothing found that argues against taking step A itself.** The two
   premises were checked rather than believed. The order holds (the oracle,
   green, and red on both ways of breaking it). The enumeration did not move
   (byte-for-byte). The record's integrity guarantee is untouched. The one
   observable cost is a `waiting` reason that a cut can take off the history
   while leaving it on the record — already reachable today by two other
   routes the code names (`move_to`, `raise`), never read by recovery, and
   rewritten by the next pass (D171).
3. **One asymmetry worth naming, in step A's favour.** A `kill -9` cannot
   lose the line: the page cache belongs to the kernel, not the process. So
   the window step A widens is the *power-cut* window only, and D184 has just
   made the ordinary stop stop being a kill. The window it narrows is nobody's
   — it narrows nothing — but it does not widen the `docker compose restart`
   case at all, which is the case the consumer actually meets.
4. **`durable.rs`'s `Renamed` note is still recorded through
   `watch::issuing`, not `watch::returned`.** It is documented as "has
   returned" on `Op::Renamed` and is noted after `fs::rename` returns, so the
   recorded fact is right; what is missing is a witness type for a rename, so
   nothing stops a future author moving that note above the `rename`. Not
   mine to change and not a defect today — `Written::Renamed` is only ever
   reached through it and the sequence equality pins its position — but it is
   the one place in the observation point where the proof is a convention.
   A finding, not an edit.
5. **`11-sync-prices.md` §6's table now describes a store that no longer
   exists** — its "store `record_waiting` | 3 | 3.11 ms" row is the
   pre-ORCH-083 hub. `README.md` of the dossier forbids editing an earlier
   piece, so it was not edited; the harness it was measured with now says so
   itself, and this handoff is the later statement.
6. **Another agent was compiling on this host during the session.** `rustc`
   processes not mine appeared at 15:5x and again later. No collision — my
   worktree, branch and scratchpad are my own, and every figure in this file
   records the load it was taken at. The phase-2 measurement was deliberately
   taken at the lowest load the host offered.

## Decisions needed

1. **Does step A ship without `--durable-waits`?** — *Recommendation:* yes.
   The flag belongs to `07`'s S4, which is the whole tier; for step A alone
   it is a config path through three files to make one commit revertible,
   and the commit is already revertible. *Fallback:* if the consumer must be
   able to put the sync back without a rebuild, that is its own task with its
   own ownership, and it should carry **B**'s flag too rather than being
   built twice.
2. **Is the wider scope — every event line unsynced — wanted, and is it
   Alexa's to answer?** — *Recommendation:* ask it with step B, not before:
   it is the same kind of question (what D103's permitted lag means) and the
   same answer would settle both. *Fallback:* leave it unasked; step A
   collects its third either way.

## Resume point

```sh
# the branch, re-read rather than trusted
git -C <worktree> log --oneline -1
# the gate ORCH-084 has to stay green against
nix develop --command cargo test -p herd-hub --lib -- store::ordering_oracle
```

Next task is **not** ORCH-084: B is not adopted (D183) and sits behind
ORCH-080's enumeration, which is now green and whose §4.1 and §6 are the
answer B needs Alexa to accept. `herd-merger` batches this branch into the
open batch PR; ORCH-086 (the sweep of stale temporaries) rides with or just
after this one per `10 §6`.

## Processes

PIDs started: `cargo`, `rustc`, `nix develop` and test binaries under this
worktree, plus two `until`-loop shells that waited on `/proc/loadavg`. All
exited on their own; none was killed, no `pkill`, no `killall`, nothing by
name. No hub or runner process was started at all — `crash_points.rs` starts
none by construction, and `./scripts/test.sh`'s own `real_herd` binaries are
its children and ended with it.

Own `target/` deleted after the push:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-083/wt/target
```

The red-run scratch copy and its separate target directory
(`…/scratchpad/orch-083/red`, `…/red-target`, 1.0 GB) were removed as soon as
the four red runs were recorded.
