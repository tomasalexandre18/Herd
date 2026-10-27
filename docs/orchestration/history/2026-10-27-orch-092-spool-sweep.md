# orch-092-spool-sweep — handoff 2026-10-27

- **Item:** ORCH-092
- **Agent:** herd-step
- **Branch / commit:** `orch-092-spool-sweep`, with `origin/main`
  `a789153` merged into it (written against `cd59ef5`; PR #85, #93 and #94
  landed while it was being written). The merge was clean; the only code it
  cost is one new field, `ToHub::Hello::jobs_at_once`, in the end-to-end
  test. Merged rather than rebased because the branch was already pushed
  and `CLAUDE.md` forbids a force-push
- **PR:** #98

## Done

At startup the runner now finds the spools under `{data root}/outputs/` and
brings them back under the **existing** offer-and-acknowledge path. Nothing
in this change deletes a collection's bytes.

- `herd-runner/src/sweep.rs` (new). `pick_up(&DataRoot)` reads `outputs/`,
  parses each entry as `{job}-{attempt}`, walks it for the regular files the
  manifest would have named (path and length only, nothing opened and
  nothing hashed), and returns one `Swept` per spool plus a line for
  everything in there that is not one. `set_aside(path, suffix)` renames a
  spool under a name that no longer parses, which is what makes a terminal
  state terminal.
- `herd-runner/src/root.rs`: `DataRoot::outputs_dir()`, beside
  `held_results_dir()`.
- `herd-runner/src/lib.rs`:
  - `Inner::swept` holds what was read until a hub has said which state root
    it serves — the gate `spool.rs` already applies to a verdict;
  - `Session::pick_up_collections(root)`, called in `run_until` beside
    `Session::pick_up`;
  - `settle_against_state_root` takes the swept collections into
    `Inner::collections` when the state roots agree, and sets each one aside
    as `.foreign-state-root` when they do not;
  - `Session::offer_swept_collections()`, called once the link is attached,
    pumps each swept collection: one chunk is both the resumption and the
    question;
  - `Session::the_hub_has_said_what_it_is_owed()`, called on
    `FromHub::HealthRecorded`, sets aside as `.unclaimed` any swept
    collection that was offered and that the hub neither asked for nor
    acknowledged;
  - `FilesOut` carries a path+length manifest (`sweep::SpooledFile`) instead
    of `herd_proto::OutputFile`, plus `swept`, `heard` and `probed`.
- `herd-hub/tests/spools_resumed.rs` (new): two protocol-level end-to-end
  tests against the real hub, over a real socket, with a runner that
  rejoins.

**No protocol change.** `herd-proto` is untouched; `PROTOCOL_VERSION` stays
2 (D73). No frame was added and no frame's meaning changed.

**`Inner::collections` is still keyed by `JobId` alone** — `lib.rs:949`
with `a789153` merged in, which carries PR #85: D177 and #85
corrected `Inner::jobs`, and left this map as it was.
Nothing here changes that, which is ORCH-065 / D177's ground and PR #85's.

### What the hub does with a re-offered collection, established by test

`herd-hub/src/store.rs:1851-1856`, `write_output_chunk`:

```rust
// Already here and whole: a chunk for a file the hub finished is a
// runner that was told so after it had already sent more, which is
// ordinary on a resumed transfer and not an error.
if file_length(&whole) == Some(entry.bytes) {
    return Ok(self.collection_state(id, &record));
}
```

A chunk for a file the hub already holds whole is **not refused, not
written twice and not read**: the store answers with the state of the
collection, so a collection the hub has in full is acknowledged again on the
spot and the spool goes through `files_acknowledged`, the one path that ever
removes one. That is asserted end to end in
`a_collection_the_hub_already_holds_is_never_asked_for_again`, which failed
loudly on the first run with the opposite expectation written in — the
assertion is still in the file as the one that decides the design.

The second fact, asserted by
`a_hub_asks_a_rejoining_runner_for_a_collection_it_is_still_owed`:
a collection the hub is owed is asked for unprompted at join
(`ask_for_the_files_still_owed`, `server.rs:2415`), before the hub reads a
frame from the connection. The same test shows the hub says **nothing** about
a collection it already holds, because `collection_owed` returns `None` for
a whole one (`store.rs:2411`).

### The rule for a spool whose job the hub no longer knows

`files_may_land` (`server.rs:4314-4359`) refuses **in silence** — an
`eprintln!` on the hub and no frame back — for every branch: no record, the
record names no streamed collection, the collection was evicted (D152), the
attempt is not the one on the record, or the machine is not the one that ran
it. So the hub cannot be asked, and absence has to be read instead.

The barrier that makes absence readable, with no new frame: the hub answers
every health report with `FromHub::HealthRecorded` (`server.rs:3597`), and
the owed list goes out at join before the hub reads anything. Frames on one
link are ordered, so the first `HealthRecorded` of a connection proves the
whole owed list has already arrived. A swept collection that was **offered**
on this connection (`probed`) and that the hub has neither mentioned
(`heard`) nor acknowledged by then is one nothing will ever acknowledge.

It is then **set aside, not removed**: renamed to
`{job}-{attempt}.unclaimed`, with a line naming the job, the attempt, the
size and the four reasons it can be in that state. The asymmetry is
deliberate. An acknowledgement is a sentence from the hub and the spool goes
on it; this is silence, and a hub whose record scan fails
(`collections_owed` returning early, `server.rs:4268`) is silent in exactly
the same way about every runner it has. A sweep that deleted on silence
would empty every spool on a fleet over one hub fault.

### Adverse cases, each named and each leaving the bytes alone

| On disk | What happens |
|---|---|
| name is not `{job}-{attempt}` (no hyphen, empty job, attempt not a `u32`) | named, left exactly as it is, sweep carries on |
| entry is a regular file | named by its type, left |
| entry is a symlink | never followed, named, left |
| entry's name is not UTF-8 | named, left |
| entry unreadable (`lstat` fails) | named, left |
| a directory under the spool will not read | the walk is not whole, so the spool is **not offered** — a part of a manifest offered as the whole of one reads to the hub as a collection it already holds — named, left |
| the walk hits `MOST_ENTRIES_SWEPT` | same: not offered, named, left, and the next start looks again |
| spool holds no regular file | named, left |
| already `.unclaimed` or `.foreign-state-root` | counted in one summary line, never offered again |
| two spools of one job | the highest attempt is offered; the other is named and left, because the map holds one collection per job (ORCH-065) |

No single entry can stop the sweep, and no entry can stop the runner: every
branch returns a sentence, never an error.

### Bounded, not off the critical path

The sweep runs on the startup path and is bounded there, which is the choice
D134's argument asks for. It opens nothing, reads nothing and hashes
nothing: one `readdir` per directory and one `lstat` per entry, stopping
after `MOST_ENTRIES_SWEPT` (50 000) entries over the whole sweep. A 65 GB
sweep therefore costs the same as an empty one, and the worst case is a
fixed number of syscalls rather than a function of what a job once wrote.
Unlike the step D134 refused, it has no external dependency that can block.

## Not done, and why

- **The 65 GB is not freed by this change alone**, and cannot be known to be
  until a runner carrying it starts and prints its sweep. If the hub holds
  those collections, they are acknowledged and the spools go for free; if it
  does not, they become `.unclaimed` directories whose removal is a human's.
  A retention policy for `.unclaimed` is a decision, below, not something to
  smuggle in here.
- **`Inner::collections` keying** untouched, as the brief requires.
- **No change to `herd-proto`**, no new frame, no `herdctl` surface.
- **Nothing read of the consumer's machine** (D161): every state in the
  tests is built locally.

## Gates

`nix develop --command cargo fmt --all --check` — clean, no output.

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```text
    Checking herd-runner v0.3.7 (…/orch-092/wt/herd-runner)
    Checking herd-hub v0.3.7 (…/orch-092/wt/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.84s
```

`nix develop --command cargo build --workspace`:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.36s
```

`nix develop --command ./scripts/test.sh` — **RED**, exit 101, and red for
host reasons that are **not this branch's**:

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
    a_killed_hub_reads_the_reservations_back_off_the_records
    the_derived_j_is_recorded_and_is_what_the_job_is_given
```

The same suite on the **pristine** tree at `cd59ef5` (this branch stashed,
everything prebuilt first), on this host, in the same conditions:

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
    a_killed_hub_reads_the_reservations_back_off_the_records
    the_derived_j_is_recorded_and_is_what_the_job_is_given
```

Identical: same targets, same two named tests. `slots` is ORCH-069 (a figure
derived from live `MemTotal` against constants written for a 17 GiB
machine). `admission`'s two are `real_herd` process tests that time out on
this workstation; `a_killed_hub_reads_the_reservations_back_off_the_records`
was also run alone on the pristine tree and failed there too.

**A warning for whoever re-runs this.** The `real_herd` tests build the
binaries inside the test, so running the suite without building
`--all-targets` first makes the build eat the test's patience and produces
a different, much larger red set every time (7 targets, then 3, then 2, all
on the same code). Build first, then run the suite, or the reading is noise.

The new tests, run on their own:

```text
running 2 tests
test a_hub_asks_a_rejoining_runner_for_a_collection_it_is_still_owed ... ok
test a_collection_the_hub_already_holds_is_never_asked_for_again ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
running 15 tests
test sweep::tests::a_name_is_split_at_the_last_hyphen_and_the_attempt_is_a_number ... ok
test sweep::tests::a_data_root_with_no_outputs_directory_says_nothing ... ok
test sweep::tests::a_bad_entry_is_named_and_left_and_the_sweep_carries_on ... ok
test sweep::tests::a_spool_that_will_not_read_through_is_not_offered ... ok
test sweep::tests::a_spool_a_previous_runner_left_is_picked_up_with_its_files ... ok
test sweep::tests::the_sweep_stops_at_its_bound_and_says_so ... ok
test sweep::tests::a_spool_already_set_aside_is_counted_and_never_offered_again ... ok
test tests::a_live_collection_is_never_set_aside_by_the_barrier ... ok
test sweep::tests::a_spool_holding_nothing_is_named_rather_than_offered ... ok
test tests::a_swept_spool_no_hub_asks_for_is_set_aside_and_never_offered_again ... ok
test tests::a_swept_spool_never_displaces_a_collection_in_hand ... ok
test tests::a_swept_spool_the_hub_asked_for_survives_the_barrier ... ok
test tests::a_swept_spool_is_offered_again_and_its_acknowledgement_removes_it ... ok
test tests::a_swept_spool_of_a_state_root_that_is_gone_is_never_offered ... ok
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured
```

`./scripts/check-docs.sh`:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**CI on `d63f763`:** `check` **pass** (8m35s), `docs` **pass** (4m40s),
run `40495608347`. That is the gate that decides, and it is green. It is
not a statement about `herd-hub/tests/slots.rs:744`: that assertion is
*skipped* on CI, and by D27 a skip is never reported as a pass.

**Skips (D27):** 12 `HERD-SKIPPED` assertions, the host's usual set — every
one is cgroup v2 delegation being unavailable to this user
(`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
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
`the_runner_says_which_memory_ceiling_it_enforces`). The suite's own banner
says this is a floor and not a count, because the run did not finish. None
of them is reported as a pass. The test
`a_spool_that_will_not_read_through_is_not_offered`
carries a skip of its own for a host running as root, which this one is not,
so it ran.

## Risks and findings

**Finding 1 — `files_acknowledged` is the only remover, confirmed.**
`inner.collections.remove` appears exactly once in the runner
(`herd-runner/src/lib.rs:1539`) and the only `remove_dir_all` of a spool is
the line below it (`:1547`), both inside `files_acknowledged`. The one other
`remove_dir_all` on a spool path is `outputs.rs:329`, inside `spool_into`,
and it only clears a spool of the *same* `{job}-{attempt}`, so it reaps
nothing. A spool therefore survives its process unconditionally, and before
this change nothing enumerated `outputs/` ever again.

**Finding 2 — the cancelled-job hypothesis, refined.** Traced on `cd59ef5`:
`cancel_job` (`herd-hub/src/server.rs:6667`) does **not** write an end for a
running job; it forwards `FromHub::CancelJob` and lets the runner end the
attempt. The runner then collects, spools and sends `JobFinished` with the
manifest, so the record carries a streamed collection and `collection_owed`
(`store.rs:2389`) — which looks at `streamed`, the manifest, eviction and
completeness, and **not** at whether the job was cancelled — still owes it.
So a cancel on its own does **not** orphan a spool while the runner stays
up and linked. What orphans it is a cancel (or any end) whose bytes have not
all crossed when the runner is **restarted**: the hub goes on asking at
every join, and until this change there was nobody on the runner's side
holding the collection to answer. That is the mechanism this change closes,
and it needs no concurrency argument.

**Finding 3 — the written-off attempt, and the keying (not mine).**
`inner.collections.insert` at `lib.rs:1385` is keyed by `JobId` alone, so a
second attempt of one job on one runner **replaces** the first attempt's
entry and the replaced `FilesOut`'s spool directory becomes unreferenced for
ever, with no line said about it. Independently, the hub's
`files_may_land` refuses a chunk whose attempt is not the one on the record
(`server.rs:4347`) and `files_held` on the runner logs a mismatch and does
nothing (`lib.rs:1504-1520`), so a written-off attempt's spool can never be
delivered. Both are ORCH-065 / D177's family and PR #85's ground; nothing
here touches them. The sweep's behaviour over them is conservative: the
later attempt is offered and the older is named and left.

**Finding 4 — a state-root recreate was unguarded for collections.** The
consumer's v0.3.7 recreate is the third mechanism. `settle_against_state_root`
already refuses to offer a spooled **verdict** produced for a state root
that is gone, because the new hub mints the same job ids for other jobs.
Nothing did the same for a **collection**, and the hazard is worse than a
refusal: `files_may_land` checks the attempt number and the runner name, so
where those happen to line up, one job's output could be written onto
another job's record. Swept collections now go through the same gate and are
kept as `.foreign-state-root`.

**Risk — the barrier depends on frame order on one link.** The owed list is
queued before the hub reads anything and `HealthRecorded` is queued when it
answers the report; both go through one outbox onto one socket. If that ever
stopped being one ordered channel, the terminal rule would fire early. It
would still only ever *rename*, never delete, which is why it renames.

**Risk — the first offer costs one chunk per swept collection.** Up to
1 MiB per spool, once per connection, even when the hub already holds the
bytes (the store ignores the data in that arm). For 125 spools that is at
most 125 MiB, once. A zero-length probe chunk would avoid it and was left
out: it is a second code path through `FilesOut::next` for a saving that is
small against what the recovery is worth.

**Risk — `.unclaimed` does not free disk by itself.** It stops the growth
and makes the figure visible in the runner's own log, which is the honest
half. See the decision below.

## Decisions needed

**One.** What becomes of a spool nobody claims?

- **Question:** after a sweep has set a spool aside as `.unclaimed`, may the
  runner ever remove it by itself — on an age (say, older than the hub's own
  output retention), on a disk budget, or on an owner's flag — or does it
  stay until a human removes it?
- **Recommendation:** leave it to the human for now, and revisit with one
  real sweep's log in hand. The runner cannot distinguish "the hub has these
  bytes" from "the hub will never take them" without a frame it may not add
  (D73), and a result nobody will ever record is still a result (D19). One
  start of the consumer's runner will say how much of the 65 GB is
  recoverable and how much is unclaimable, and that number should be known
  before a deleting rule is written.
- **Fallback:** if the consumer needs the disk before then, a one-line
  owner-facing flag (`--drop-unclaimed-spools-after <duration>`), off by
  default, removing only directories already named `.unclaimed` by a sweep
  that completed.
- **Where it belongs:** beside `docs/disk-policy.md` (ORCH-093, merged as
  PR #94), which names the same disk from the hub's side and says in §5
  that the runner's 65 GB is this item's. That document also makes the
  sharper point: the spools sit on the **same filesystem** as the hub's
  volume, so Herd has two budgets over one disk and nothing reconciles
  them. An `.unclaimed` retention rule should be written as part of that
  reconciliation, not on its own.

## Resume point

```sh
gh pr view 98          # the PR for this branch; do not merge, herd-merger does
```

Then: this branch is independent of PR #85 by construction but touches
`herd-runner/src/lib.rs` in the same region, so expect a textual conflict
around `FilesOut` and `Inner`; the keying is untouched on this side, so the
resolution is PR #85's version of the map plus this branch's fields.

## Processes

Started and ended by this agent: `cargo`, `rustc` and the test binaries
under `nix develop`, all in this worktree, all exited on their own. One
backgrounded shell (a `cat` waiting on stdin by mistake) was stopped by its
own task id. No `pkill`, no kill by name, nothing outside this worktree.
`target/` removed after the push.
