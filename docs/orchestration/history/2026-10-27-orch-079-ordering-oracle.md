# orch-079-ordering-oracle — handoff 2026-10-27

- **Item:** ORCH-079
- **Agent:** herd-step
- **Branch / commit:** `orch-079-ordering-oracle` / `be8ba2b`, based on
  origin/main `cd59ef5`
- **PR:** #92

## Done

**The oracle.** `herd-hub/src/store/ordering_oracle.rs`, new, entirely
`#[cfg(test)]`. One test drives a real `JobDirStore` through **16 store
calls** over one job's whole life and asserts, after each one, that the
record's `fsync` **and** its directory `fsync` have **returned** before the
first byte of the event line is **issued** — plus, per call, the exact
sequence of record and event operations, so a reordering inside a call
cannot hide behind an earlier complete write.

**The observation point.** `herd-hub/src/durable.rs` gains a `watch`
module. Five notes are placed in the real `write_file_with` and the real
`append_line`: the temporary's `fsync` returned, the rename returned, the
directory `fsync` returned, the event line's `write_all` is about to be
issued, its `fdatasync` returned. The recording half is `#[cfg(test)]` and
is not compiled into `herd-hub`; in a product build the notes are two empty
`#[inline(always)]` functions.

**The one deliberate deviation from the board row,** which said "no product
code". Nine lines of product code were added: a zero-sized `Synced`
produced by nothing but the three functions that make the sync calls, taken
by reference by `watch::returned`. Without it the first red run **stayed
green**: deleting `sync_dir` left the note that said it had happened, so
the oracle survived exactly the change it exists to catch. With it, delete
a sync and the note for it no longer compiles. The brief allowed an
instrumented wrapper in `durable.rs` behind `#[cfg(test)]`; this is that,
plus the minimum that makes it unforgeable.

**The complete transition list** — all **13 `self.write_record(` sites** and
all **11 `self.append_event(` sites** of `store.rs`, driven as 16 calls:
`submit`, `join` (1 record, 2 events), `raise_priority_because`,
`record_waiting`, `record_reading`, `begin_attempt` (2 records, 2 events),
`move_to` via `record_state` (×3: assigned, running, terminal),
`record_step`, `append_log` past a gap (event only), `name_missing_log`
(event only), `record_metrics`, `record_outputs`, `end_attempt`,
`note_outputs_fetched`, `evict`. 13 event lines, read back off
`events.jsonl` at the end.

**Why that list is complete:** `store.rs` is the only writer of a job's
`job.json` and `events.jsonl`, through exactly `write_record` and
`append_event`; nothing else in the workspace calls `durable::append_line`.
A second test counts both call sites in the source and goes red if either
number moves — a **tripwire, not a proof**, and it says so.

**Proof it can fail**, on a scratch copy outside the branch
(`…/scratchpad/orch-079/red/`), twice: the directory `fsync` dropped (the
ORCH-083/084 hazard itself) and `record_waiting`'s two writes swapped. Both
outputs are in §5 of the write-up.

**Cross-check from outside the process:** `strace 7.2` on the compiled test
binary, the same state machine applied to the kernel's own trace with
`awk`: *event writes checked: 13, violations: 0*.

**The write-up:** `docs/remediation/12-ordering-oracle.md`, with its row
added to `docs/remediation/README.md` (index table, "Neither" bullet, and
the index's date line).

**No transition was found where the order does not hold** on `cd59ef5`.

## Not done, and why

- **No `strace`-based gate.** `strace` works on this host and is used once
  as a cross-check, but it is not installed on the CI runner, and a gate
  that skips is not a gate (D27). Reasoned in §4 of the write-up.
- **Nothing for ORCH-080, ORCH-083 or ORCH-084.** Out of scope; this is the
  guard they have to stay green against.
- **No decision entry.** Nothing here decides anything: no behaviour of the
  hub changes, `PROTOCOL_VERSION` stays 2, `RECORD_VERSION` stays 1.
- **Only the ORCH-079 board row is touched**, as the brief required.

## Gates

`cargo fmt --all --check` — clean, no output.

`cargo clippy --workspace --all-targets -- -D warnings`:

```
    Checking herd-hub v0.3.7 (…/orch-079/wt/herd-hub)
    Checking herdctl v0.3.7 (…/orch-079/wt/herdctl)
    Checking herd-runner v0.3.7 (…/orch-079/wt/herd-runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.60s
```

`cargo build --workspace`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.63s
```

`./scripts/test.sh` — **RED on this workstation, for host reasons**, load
`2.83 5.10 5.30` before and `5.80 5.58 5.50` after:

```
test the_load_is_on_the_placement_line_and_stops_nothing ... FAILED
test a_killed_hub_reads_the_reservations_back_off_the_records ... FAILED
test the_derived_j_is_recorded_and_is_what_the_job_is_given ... FAILED
error: 2 targets failed:
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
test suite: FAILED (cargo test exited 101)
```

Separated:

- `herd-hub/tests/slots.rs:744`
  (`the_derived_j_is_recorded_and_is_what_the_job_is_given`) is **ORCH-069**,
  known, a figure derived from this host's live `MemTotal` against constants
  written for a 17 GiB machine. Not mine, and **not "green on CI"**: on a
  GitHub runner that assertion is *skipped*, and a skip is not a pass (D27).
- The two `killed_hub` failures are **deadline timeouts in end-to-end tests
  that spawn real hub and runner processes**, while another agent was
  integrating PR #85 on the same host. `cargo test -p herd-hub --test
  approval --test killed_hub` at the same commit, run alone: *ok. 5 passed*
  and *ok. 18 passed*. Two earlier full runs of the suite failed on
  different members of this same family (`approval.rs:432`,
  `real_herd/mod.rs:649` and `:789`), which is the signature of load and not
  of a change.
- Everything else was green, the two oracle tests included.

The oracle itself, run alone at load `13.16 8.38 5.52`:

```
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 178 filtered out; finished in 0.11s
```

`./scripts/check-docs.sh`:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**Skips: 12 on this workstation**, reported by `test.sh` as "AT LEAST 12"
because the suite did not finish — which is the honest answer and not a
count. 10 cgroup-v2 delegation, 2 the ZFS/IO ceiling, the same 12 the
dossier records for this host. CI reports 15. A skip is never a pass (D27).

## Risks and findings

1. **The observation point had to be made unforgeable, and this is the
   finding.** The first attempt at the red run deleted `sync_dir` and the
   oracle **stayed green**, because the note that said the sync had
   happened sat beside it and survived. An oracle of that shape is worse
   than none. The fix is the `Synced` witness; the lesson for ORCH-083 and
   ORCH-084 is that an assertion about a syscall has to be tied to the
   syscall's own return value, not placed next to it.
2. **The completeness tripwire is a count.** It cannot tell a covered call
   site from an uncovered one, and a writer reached through a new helper
   would walk past it. It makes a fourteenth `write_record` impossible to
   add *silently*, which is the decision point; it does not make the list
   self-proving.
3. **The oracle watches a `cfg(test)` build.** A future
   `#[cfg(not(test))]` divergence inside `durable.rs` would be invisible to
   it. The `strace` cross-check closes this **once, for `be8ba2b`**, not
   continuously.
4. **Nothing about concurrency.** One thread, one job. Two writers racing
   on one record are excluded by the store's lock, which this does not
   test; `03b` already treats it.
5. **The suite is red on this host and I am not able to show it green.**
   Two of the three failures are load, one is ORCH-069. CI on the PR is
   where a clean run has to come from, and `slots.rs:744` will be *skipped*
   there rather than passed.
6. **The board row for ORCH-079 said "no product code"** and that is no
   longer true. Nine lines, named above and in §8 of the write-up. Nothing
   the hub does changes.

## Decisions needed

None.

## Resume point

`herd-merger` integrates PR #92 once CI is green (ORCH-001). After that,
ORCH-080 (the crash-point enumeration) and then ORCH-083 (step A, the event
`fdatasync`) — both of which must keep the oracle's
`every_transition_fsyncs_the_record_and_its_directory_before_appending…`
green, and ORCH-084 is the one it was built for.

## Processes

Every process started here was a foreground `nix develop --command cargo …`
or `strace` under this agent's own shell, in this agent's worktree and
scratchpad; all exited before the next command. No background process, no
`pkill`, nothing killed by name, nothing outside
`…/scratchpad/orch-079/`.
