# 12 — D103's ordering oracle, and the proof that it can go red

**Item:** ORCH-079. **Base:** `origin/main` at `cd59ef5`, where the oracle
was written and every run in §5 and §6 was made. `origin/main` moved to
`2ca6dc7` (PR #85) while this was being written; the branch is merged up to
it and the gates re-run there. **Branch:** `orch-079-ordering-oracle`.
**Date:** 2026-10-27.

**Which kind of piece this is: neither a constat nor a mesure.** It is a
**gate**. `06` and `08` *counted* the hub's `fsync`s; neither *ordered*
them, and this is the assertion that orders them. It is the only piece of
this dossier that changes a file under `herd-hub/src`, and what it changes
is named in §8.

**It ran on the shared workstation**, under `nix develop`, in the debug
profile, with `/proc/loadavg` recorded beside every run. Nothing here is a
timing figure, so load is reported for honesty rather than for
interpretation.

---

## 1. What it asserts, in four lines

1. For **every** store call that writes a record and appends an event, the
   record's `fsync` **and** its directory `fsync` have **returned** before
   the first byte of the event line is **issued**.
2. The assertion is made on the code that ships — the real
   `durable::write_file_with` and the real `durable::append_line`, driven
   through a real `JobDirStore` — and not on a replica of it.
3. It **can** go red, and the two runs in §5 are it going red: once with the
   directory `fsync` dropped, once with `record_waiting`'s two writes
   swapped.
4. An `strace` of the same test, from outside the process, sees the same
   order in the kernel's own words (§6): 13 event writes, 0 violations.

## 2. Why this and not another count

`10-ordering-on-ext4.md` settles that D103 needs **no new ordering
mechanism**. What holds the order on a *decisive* write is that
`write_file_with` has returned, both of its syncs with it, before
`append_line` is called at all — a syscall-return guarantee that owes
nothing to ext4, to ZFS, to `data=ordered` or to transaction groups.

A guarantee of that shape has exactly one failure mode: **someone changes
the calls.** `11-sync-prices.md` then made that failure cheap to reach by
accident. Its §5 showed the directory `fsync` is not the 6.6 µs rounding
error `06` measured but a full **991 µs** on ext4 — a third of a waiting
write, and therefore a third of the prize that steps A, B and C of `10`'s
amendment are there to collect. The same correction raises its *safety*
standing: `sync_all` makes the record's bytes durable, and only `sync_dir`
makes its **rename** durable. A decisive write stripped of its directory
sync can lose its transition while the `events.jsonl` line naming that
transition survives, which is `events.jsonl` genuinely **ahead** of
`job.json` — the one thing D103 forbids.

So this oracle stands in front of **ORCH-083 and ORCH-084**. It is not
documentation of today's order; it is the thing that goes red if tomorrow's
change moves it.

## 3. The complete list of transitions, and how completeness is known

`store.rs` is the only writer of a job's `job.json` and `events.jsonl`, and
it writes them through exactly two private methods: `write_record`, which
calls `write_json` → `durable::write_file`, and `append_event`, which calls
`durable::append_line`. Nothing else in the workspace calls `append_line`.
So enumerating the call sites of those two methods enumerates every record
write and every event append there is.

On the base there are **13 `self.write_record(` sites and 11
`self.append_event(` sites**. The oracle drives every one of them:

| # | store call | record writes | event appends | what the events are |
|---|---|---|---|---|
| 1 | `submit` — a job is minted | 1 | 1 | `transition{queued}` |
| 2 | `join` — a second identical submission joins | 1 | 2 | `priority_raised`, `joined` |
| 3 | `raise_priority_because` — an operator raises the level | 1 | 1 | `priority_raised` |
| 4 | `record_waiting` — why a queued job is not starting | 1 | 1 | `waiting` |
| 5 | `record_reading` — a reading lands on the record | 1 | 0 | — |
| 6 | `begin_attempt` — assigned, then an attempt opened | 2 | 2 | `transition{assigned}`, `attempt_started` |
| 7 | `move_to` via `record_state` — assigned to running | 1 | 1 | `transition{running}` |
| 8 | `record_step` — one executor step | 1 | 0 | — |
| 9 | `append_log` — a chunk arrives past a gap | 0 | 1 | `log_gap` |
| 10 | `name_missing_log` — chunks gone for good | 0 | 1 | `log_gap` |
| 11 | `record_metrics` — the runner's figures | 1 | 0 | — |
| 12 | `record_outputs` — a collection carried inline | 1 | 0 | — |
| 13 | `end_attempt` — the attempt is closed | 1 | 1 | `attempt_ended` |
| 14 | `move_to` via `record_state` — the terminal transition | 1 | 1 | `transition{finished}` |
| 15 | `note_outputs_fetched` — the first fetch is stamped | 1 | 0 | — |
| 16 | `evict` — the sweeper takes a collection | 1 | 1 | `outputs_removed` |

Sixteen calls, because `move_to` is reached three times (rows 6, 7, 14) and
`join` appends two lines of its own: **13 event lines from 11 appenders**,
asserted at the end of the test by reading `events.jsonl` back.

**Three of the rows carry a finding of their own.** Rows 9 and 10 append an
event and write **no record at all** — a log gap is a fact about the log,
not a transition — so the rule is vacuous there and the oracle is told so
rather than being quietly weakened for every row. Rows 5, 8, 11, 12 and 15
are the reverse: a record write with no event. The rule still binds their
three syscalls to each other.

**How completeness is kept, and what keeps it is weaker than the list
itself.** A second test counts the two call sites in `store.rs`'s own
source and fails if either number moves. It is a **tripwire, not a proof**:
it cannot tell a covered site from an uncovered one, and a writer reached
through a new helper would walk past it. What it does is make a fourteenth
`write_record` impossible to add silently, which is the decision point that
matters.

## 4. The mechanism, and what it cost the product

`strace` **does** work on this host (§6), and it is not the gate. Three
reasons: the gate has to run inside `./scripts/test.sh` on CI, where no
tracer is installed; a gate that skips is not a gate (D27 — a skip is never
a pass); and a tracer that must map file descriptors back to paths across a
threaded test harness is a second program to maintain for an assertion that
can be made directly.

So the observation point is in-process, in `herd-hub/src/durable.rs`, and
the recording half of it is `#[cfg(test)]` — not compiled into `herd-hub`
at all. Five notes are placed in the real `write_file_with` and the real
`append_line`: the temporary's `fsync` returned, the rename returned, the
directory `fsync` returned, the event line's `write_all` is about to be
issued, its `fdatasync` returned. The asymmetry is the point: D103 is about
*returns* on one side and the *issue* on the other.

**One thing had to be added to the product for the red run to mean
anything.** A note placed *beside* a sync can be left behind when the sync
is deleted, and an oracle built on such a note stays green through exactly
the change it exists to catch — which is what the first attempt at the red
run below actually did. So `durable.rs` now has a zero-sized `Synced`,
produced by nothing but the three functions that make the calls, and the
observation point takes one by reference. **Delete a sync and the note for
it no longer compiles.** That is nine lines of product code and no runtime
cost; in a build that is not `cfg(test)` the note is an empty
`#[inline(always)]` function and the whole thing is gone.

## 5. Proof the oracle can fail

Both runs are on a **scratch copy of the tree**, outside the branch and
outside git, at
`…/scratchpad/orch-079/red/`. Neither change is on the branch, in a commit
or in the PR. Both blocks below are the runs' own output with two edits and
no others: the temporary state root's path is written `<root>`, and the
panicking thread's name and pid, which are the test's own long name, are
written `'...'`. The backtrace frames are cut.

### 5.1 The directory `fsync` dropped — the ORCH-083/084 hazard itself

`write_file_with`'s `sync_dir_of(parent)?` and the note it proves are
deleted. Nothing else changes. Load `11.64 7.85 5.31` before,
`12.63 8.12 5.41` after.

```
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... FAILED

failures:

---- store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event stdout ----

thread '...' panicked at herd-hub/src/store/ordering_oracle.rs:219:17:
assertion `left == right` failed: D103 VIOLATED — submit — a job is minted: the first byte of an event line was issued while the record write of <root>/jobs/j0000000001/job.json had only reached Renamed. An event line is durable the moment append_line returns; a record whose fsync or whose directory fsync has not returned is not. events.jsonl can therefore end up ahead of job.json, which D103 forbids.
The trace of this call:
   0. TemporarySynced job.json
   1. Renamed job.json
   2. AppendIssued events.jsonl
   3. AppendSynced events.jsonl
   4. TemporarySynced state.json
   5. Renamed state.json

  left: Renamed
 right: Durable

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 178 filtered out; finished in 0.11s
```

### 5.2 One transition reordered — `record_waiting`'s two writes swapped

`durable.rs` is restored; in `store.rs`, `record_waiting` appends its event
before writing its record. Load `9.15 7.20 5.06` before, `10.34 7.48 5.16`
after.

```
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... FAILED

thread '...' panicked at herd-hub/src/store/ordering_oracle.rs:219:17:
assertion `left == right` failed: D103 VIOLATED — record_waiting — why a queued job is not starting: the first byte of an event line was issued while the record write of <root>/jobs/j0000000001/job.json had only reached Nothing. An event line is durable the moment append_line returns; a record whose fsync or whose directory fsync has not returned is not. events.jsonl can therefore end up ahead of job.json, which D103 forbids.
The trace of this call:
   0. AppendIssued events.jsonl
   1. AppendSynced events.jsonl
   2. TemporarySynced job.json
   3. Renamed job.json
   4. DirectorySynced job.json

  left: Nothing
 right: Durable

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 178 filtered out; finished in 0.27s
```

Note which transition each run names. The oracle reports the **first**
violation it meets, by the name the table in §3 gives it, with the trace of
that one call printed underneath — so a future red run says which writer
moved, not merely that something did.

## 6. Green on the unmodified tree, and the cross-check from outside

On the branch, nothing modified, load `13.16 8.38 5.52` before and after:

```
running 2 tests
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 178 filtered out; finished in 0.11s
```

The in-process observation and a `cfg(test)` build are two gaps the oracle
cannot close by itself, so they were closed once, by hand, from outside the
process: `strace 7.2` on the compiled test binary at load
`10.00 8.03 5.48`, tracing `fsync`, `fdatasync`, `rename`, `write` and
`openat`. The first transition, verbatim but for the job directory's path
shortened to `<job>`:

```
openat(AT_FDCWD, "<job>/.job.json.tmp-99558-2", O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC, 0666) = 4<<job>/.job.json.tmp-99558-2>
write(4<<job>/.job.json.tmp-99558-2>, "{\"record_version\":1,\"id\":\"j00000"..., 453) = 453
fsync(4<<job>/.job.json.tmp-99558-2>) = 0
rename("<job>/.job.json.tmp-99558-2", "<job>/job.json") = 0
openat(AT_FDCWD, "<job>", O_RDONLY|O_CLOEXEC) = 4<<job>>
fsync(4<<job>>) = 0
openat(AT_FDCWD, "<job>/events.jsonl", O_WRONLY|O_CREAT|O_APPEND|O_CLOEXEC, 0666) = 4<<job>/events.jsonl>
write(4<<job>/events.jsonl>, "{\"record_version\":1,\"at\":\"2026-1"..., 106) = 106
fdatasync(4<<job>/events.jsonl>) = 0
```

The same state machine the oracle applies in process, applied to the whole
trace with `awk`:

```
event writes checked: 13, violations: 0
```

Thirteen, the same thirteen the test counts. The kernel's record of this
run and the oracle's own agree.

**One detail the trace shows that the oracle deliberately does not
assert.** The `openat` that creates `events.jsonl` sits *between* the
directory `fsync` and the event's first byte. The oracle asserts on the
`write`, as D103's wording does, because an empty `events.jsonl` cannot be
ahead of anything; an assertion on the `open` would be stronger than the
rule and could go red on a change that breaks nothing.

## 7. What it proves, and what it does not

**Proved, for all 16 calls and all 13 event lines:**

- the record's `sync_all` returned before its `rename` was issued;
- its `rename` returned before the directory `fsync` was issued;
- the directory `fsync` returned before the **first byte** of the event line
  was issued;
- the observed sequence of record and event operations in each call is
  exactly the one written down for that call, so a reordering *inside* one
  call cannot hide behind an earlier complete write — which is what rows 2
  and 6 of §3, the two calls with more than one pair, needed.

**Not proved, and these are the residual gaps:**

1. **Nothing about the platters.** A returned `fsync` is durable by the
   syscall's contract and the oracle takes that contract as given. It is
   also precisely what `10` says D103 rests on, so the gap is the premise
   and not a hole in the argument.
2. **Nothing about a path that bypasses `crate::durable`.** None exists;
   `durable.rs`'s module comment is the standing statement of that and
   nothing here re-checks it.
3. **A `cfg(test)` build is what is watched.** A future
   `#[cfg(not(test))]` divergence inside `durable.rs` would be invisible to
   the oracle. §6's `strace` closes this **once, for this commit**, and not
   continuously.
4. **The completeness tripwire is a count, not a proof** (§3).
5. **Nothing about concurrency.** One thread drives one job. Two writers
   racing on one record are excluded by the store's lock, which this does
   not test and `03b` already treats.

**No transition was found where the order does not hold.** The brief asked
for that finding to be loud if it existed; it does not exist on `cd59ef5`.

## 8. What it changes, and what it does not

`herd-hub/src/durable.rs` gains the `Synced` witness, three private
producers of it, and a `watch` module whose recording half is `#[cfg(test)]`
— the product build keeps two empty `#[inline(always)]` functions and one
zero-sized value. `herd-hub/src/store/ordering_oracle.rs` is new and is
entirely `#[cfg(test)]`. **No behaviour of the hub changes**: no sync is
added, removed or moved, `PROTOCOL_VERSION` stays 2 and `RECORD_VERSION`
stays 1.

This piece authorizes nothing. In particular it does not authorize step A,
B or C of `10`'s amendment: it is what those steps have to stay green
against.
