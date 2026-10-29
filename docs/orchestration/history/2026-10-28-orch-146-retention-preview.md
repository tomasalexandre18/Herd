# orch-146-retention-preview — handoff 2026-10-28

- **Item:** ORCH-146 (a new row; its text is proposed below, the merger
  allocates nothing but the decision number)
- **Agent:** herd-step
- **Branch / commit:** `orch-146-retention-preview` / see the agent's
  report, based on origin/main `09229db`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`herd-hub retention-preview [STATE_DIR] --merger <SPAN> --step <SPAN>
--nightly <SPAN> --producer <SPAN>` exists: a lock-free subcommand of the
hub binary that answers *"what would record retention remove, and what
would it cost me?"* and **removes nothing**. It is M12's first task and
the engine is a separate one.

- `herd-hub/src/retention.rs` (new, 1339 lines). It carries the
  **selection function the engine will call** — `retention::select(Judged,
  &Spans, Timestamp) -> Selection` — so that "what the preview said would
  go" and "what the engine takes" cannot be two different sentences.
  `Judged::of(&JobRecord)` is the three facts the decision rests on and
  there is no fourth: the class, whether the job is over, and
  `updated_at`. `RetentionPreview::read(&Path, Spans, Timestamp)` walks a
  state root and answers.
- `herd-hub/src/main.rs`: the subcommand beside `Command::Report`,
  decided and printed **before** the budget is parsed and long before the
  state root is opened, for D194's reason — opening it takes the `flock`
  on `hub.lock` and sweeps. Four spans, `required = true`, **no default
  anywhere**, each with a compulsory unit (`90d`, `36h`, `90m`, `30s`, or
  a bare `0`).
- `herd-hub/tests/retention_preview.rs` (new, 8 tests) and 5 unit tests
  in `retention.rs` and 1 in `main.rs`: 14 new assertions in all.
- `herd-hub/src/lib.rs`: the module and its re-exports. **No other file
  changed. `herd-hub/src/store.rs` was read and not touched**, so there is
  no collision with ORCH-095/096/097 or ORCH-087.

**What it prints, per class:** the span as this invocation was given it,
how many records of the class there are, how many of them are not over
(the reaper's), how many were judged, how many are past the span with
their bytes split into output bytes and record bytes, the **oldest and
the newest** of them, each one named with its weight and its end time,
how many are inside the span with their bytes, and which one expires
next. Plus the totals, the census of what could not be read, and **the
clock reading every figure was judged against**. One output, no `--json`
(D194's *"one output is one thing to keep honest"*).

**No span is in the binary.** D194's refusal to print a threshold is the
rule inherited here, and it is why four spans are required arguments
rather than two with a class map: the 14/90 figures live in
`docs/plan.md`, `docs/coverage.md` and `docs/disk-policy.md`, Alexa has
the class map as an open question, and four required spans is what lets
this tool be run against **any** answer to it.

### The four settlements, each with its test and its verdict

1. **`updated_at` is a stable end time, not a last-touch clock** —
   **SETTLED AS READ; the premise holds.**
   `updated_at_is_an_end_time_and_not_a_last_touch_clock` drives the real
   `JobDirStore`: a job ends, a requester fetches its outputs
   (`job_outputs` + `note_outputs_fetched`), a runner resends a file the
   hub already holds whole (`write_output_chunk`), and the output budget
   evicts its bytes — and `updated_at` is byte-identical after all four.
   The manifest outlived the bytes (`outputs.removed` set,
   `outputs.files` non-empty) and `Judged::of` reads the same figure. So
   a span measured from it is a span and not an LRU, and D152's sign — an
   unfetched collection kept **longer** — is not inverted.
2. **A forward clock step must refuse, not empty the root** — **SETTLED,
   AND THE PLANNER'S PROPOSED RULE IS REFUTED.** See *Risks and
   findings* 1. Two refusals are implemented instead of one, and both are
   threshold-free.
3. **A span of `0` means never expire, printed in those words** —
   **SETTLED AS ASKED.** `Spans::never_expires`, `Selection::NeverExpires`,
   and the output says `0: these records NEVER EXPIRE` and, in the spans
   block, why it is **not** `--output-floor 0`'s reading: the floor's zero
   loses bytes a budget would have taken anyway, and this one would lose
   the records. Asserted by
   `a_span_of_zero_never_expires_and_the_preview_says_so_in_those_words`
   and by `a_span_of_zero_means_never_and_not_now`, which keeps a
   century-old record.
4. **The wrong population is never selected** — **SETTLED AS ASKED.**
   `a_job_that_is_not_over_and_a_quarantined_record_are_never_expired`
   runs the fixture at a span of **one second per class** — as aggressive
   as a span can be — and asserts that the running record (the reaper's,
   ORCH-012 and D134), the quarantined record (D52), the record from
   another layout and the directory with no record are none of them in any
   class's expired list, and that exactly the seven judgeable records are.
   The same thing is asserted at `select` itself, so it cannot be true of
   the preview and false of the engine.

### Evidence that it disturbs nothing

- `every_figure_of_the_preview_is_this_fixture_to_the_byte` holds the
  `flock(2)` on `hub.lock` — the state a live hub leaves that file in —
  for the whole of the read, and asserts the state root's listing
  byte-identical afterwards. Every `job.json` in that fixture is padded to
  exactly **512 bytes** with trailing whitespace, so every byte figure the
  preview prints is one a reader can check by arithmetic rather than one
  the test copied from a run. The whole rendered text is then asserted.
- `the_preview_reads_what_the_store_itself_wrote` previews a root the
  `JobDirStore` still has **open**, so its `flock` is held and its index
  is still being written, and derives the expected byte figure from an
  independent walk of the directory.
- By hand, against a hub that was really serving: see *Gates*.

## Not done, and why

- **The retention engine.** Out of scope by the brief; this is the task
  that comes before it. There is no deletion path in the new module, no
  `--apply`, and no flag that a later change could invert.
- **`docs/disk-policy.md` `[C18]`** — *"Nothing anywhere reads a
  retention class, a retention span, or expires a record"* — is now false
  in its first two thirds and still true in the third. It is a dated
  planner's claim table and amending someone else's recorded evidence is
  not mine; see *Risks and findings* 3.
- **`docs/status.md`** does not mention the new subcommand. It is *state*,
  replaced every session, and is the orchestrator's.
- **No board row, no `docs/decisions.md` entry, no PR, no merge, no tag.**
  The proposed texts are below.

## Gates

Four Rust gates plus `check-docs.sh`, all run before this file was
committed, on the same tree but for this file:

```
$ nix develop --command cargo fmt --all --check
(no output; clean)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.48s
(no warning, no error)

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 28.76s

$ nix develop --command ./scripts/test.sh
     Running tests/retention_preview.rs
running 8 tests
test a_record_with_no_end_time_is_named_and_never_expired ... ok
test a_job_that_is_not_over_and_a_quarantined_record_are_never_expired ... ok
test a_span_of_zero_never_expires_and_the_preview_says_so_in_those_words ... ok
test a_clock_a_year_forward_refuses_rather_than_reporting_everything_expired ... ok
test a_clock_behind_the_root_refuses_and_names_the_figures ... ok
test every_figure_of_the_preview_is_this_fixture_to_the_byte ... ok
test updated_at_is_an_end_time_and_not_a_last_touch_clock ... ok
test the_preview_reads_what_the_store_itself_wrote ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

test suite: PASSED
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok

$ ./scripts/check-orchestration.sh
check-orchestration: 256 passed, 0 failed
```

`./scripts/bench-preflight.sh --target-dir /mnt/ssd_980/herd/build/orch-146`
was run before each heavy phase and answered `ok, safe to start` each
time (load 8.52 then 5.88 against a ceiling of 9.00).

**A run against a hub that was really serving**, by hand, because this
tool's one claim is that it removes nothing:

```
$ herd-hub --listen 127.0.0.1:18461 --state-dir .../live-root &   # pid 3046925
$ herd-hub retention-preview .../live-root --merger 90d --step 14d \
      --nightly 14d --producer 0
...
  in all                   3 of the 5 record(s) that can expire are past their span
    records judged         6 (a class whose span is 0 is judged and never expires)
  what that would free     875 bytes
    of which output        54 bytes
    the records            821 bytes
  Nothing above was removed, and this subcommand cannot remove it.
exit=0
STATE ROOT BYTE-IDENTICAL (content digest equal)
```

and the refusal, with its exit status, on the same serving root:

```
$ herd-hub retention-preview .../live-root --merger 1s --step 1s \
      --nightly 1s --producer 1s
REFUSED: A PASS WOULD EMPTY THIS ROOT OF RECORDS
  records that can expire    6
  of those, past their span  6 -- every one
  the newest record          jobs/j0000000004, a step, last changed 2026-10-27T00:00:00Z
  it is past its span by     1 day(s) (162734 s)
  clock read                 2026-10-28T21:12:15.265614533Z
...
Error: every one of the 6 record(s) that can expire on this root is past
its span -- ... -- so a pass would empty this root of records, and nothing
was previewed
exit=1
STATE ROOT BYTE-IDENTICAL (content digest equal)
```

and the two command-line refusals:

```
$ herd-hub retention-preview ... --step 14 ...
error: invalid value '14' for '--step <SPAN>': "14" carries no unit: write 14d, 36h, 90m or 30s
exit=2

$ herd-hub retention-preview ... --merger 90d --step 14d --nightly 14d
error: the following required arguments were not provided:
  --producer <SPAN>
exit=2
```

**The last run** is `./scripts/gate.sh` on the committed tree, with this
file in it; its log goes to `/mnt/ssd_980/herd/build/orch-146/gate.log`
and its receipt to this worktree's `target/herd-gate/`. The verdict line
is verbatim in the agent's report, not here, because this file is part of
the tree that run gates.

**Skips (D27).** 12, every one of them a `HERD-SKIPPED` line copied as a
skip and **none counted as a pass**. All 12 are the host's known
cgroup-delegation and block-device skips, unchanged by this branch:
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
`the_runner_says_which_memory_ceiling_it_enforces`. **None of the 14 new
assertions is skipped or conditional**: they need no cgroup, no network
and no load threshold.

**`RECORD_VERSION` is 1 and `PROTOCOL_VERSION` is 2, both unchanged.**
`herd-proto` is untouched, and so are `herd-hub/src/store.rs` and
`herd-hub/src/job.rs`.

## Risks and findings

1. **The brief's settlement 2 holds, but the planner's proposed rule for
   it does not, and the two are opposites.** The proposal was: *"if `now`
   is earlier than the newest record's `updated_at`, the clock is unusable
   and the pass does not run."* That catches a clock that stepped
   **backwards**. A clock stepped **forward** is ahead of every record on
   the root, so that test passes it straight through — and a fixture whose
   `now` is a year ahead would have been reported as *everything is
   expired*, which is what the brief asked to prevent. Measured, not
   argued: `a_backward_clock_can_only_make_a_record_read_young` shows
   `Timestamp::since` saturating to nought, and
   `a_clock_a_year_forward_refuses_rather_than_reporting_everything_expired`
   is the fixture read at `PINNED + 365d`.

   **A monotone forward clock step is not detectable from a state root at
   all**, and that is the finding under the finding: a year-old newest
   record is exactly what a broken clock and a hub idle for a year both
   look like, and nothing on the disk tells them apart.
   `docs/disk-policy.md` §3.6 says the idle case is real (*"a hub idle
   longer than its retention window loses every directory"*). So what is
   refused is **the consequence the two share**:

   - `Verdict::ClockBehindTheRoot` — `now` is earlier than the newest
     `updated_at` on the root, which this hub wrote with its own clock.
     The planner's rule, kept because it is sound and cheap.
   - `Verdict::WouldEmptyTheRoot` — **every** record that *can* expire is
     past its span, so a pass would leave no record behind. This is the
     forward-step guard. Its threshold is the span the operator typed and
     nothing else, so no figure is enshrined (D194).

   A refusal prints the clock, the spans and the census and **no list of
   what would go**, and exits non-zero: a refusal that still printed
   "everything is expired" would be the output it exists to prevent.

   The population is deliberately *records that can expire* and not
   *records judged*: one class pinned to `0` would otherwise be enough to
   mask a clock that had stepped forward under all three others.
   `RetentionPreview::expirable()`.

   **What this costs, said plainly:** a hub that really has been idle
   longer than its shortest span is refused too. For a preview that costs
   nothing. For the **engine** it is a decision — see *Decisions needed* 1.

2. **`updated_at` is not monotone in job id**, which this branch does not
   depend on and the engine will. The preview sorts its expired list by
   `(ended_at, id)` and never by id alone, because a long job ends after a
   later short one. `docs/disk-policy.md` §3.4 proposes the engine stop
   after K consecutive in-window records plus a full pass at startup; that
   K is the engine's and nothing here pins it.

3. **`docs/disk-policy.md` `[C18]`** — *"Nothing anywhere reads a
   retention class, a retention span, or expires a record"* — is now false
   in its first two thirds: `herd-hub/src/retention.rs` reads a class and
   a span. Nothing expires a record, which is the third. The row is dated
   evidence in a planner's claim table, so it is reported rather than
   edited; the merger or the next planner pass should amend it.

4. **The whole job directory is what `expired_bytes` measures** —
   `job.json`, `events.jsonl`, the log, `log.seq` and the inline files —
   because that is what removing a record frees. Output bytes are broken
   out beside it, because the budget governs those and may never remove
   the record (§3.3), so an operator weighing the two mechanisms needs
   them apart. `files.part/` and `files.gone/` count as output bytes.
   **A collection still arriving is therefore inside a figure the engine
   must not act on**: §3.5 forbids removing one, and the engine will need
   `collection_is_whole` as a second test beside `select`. The preview
   says nothing about it because the preview removes nothing; the engine
   must.

5. **The clock is read once, at the start of the walk**, and the figures
   are not one consistent instant — the trade `report.rs` already makes
   and prints. The preview prints it too.

6. **A record's end time is `updated_at` and nothing more precise
   exists.** If M12 ever wants the moment a job *ended* as distinct from
   the moment its record last changed, that is a new field and a
   `RECORD_VERSION` conversation, not a reading of this one. Settlement 1
   says the two are the same today.

## Decisions needed

1. **Should the engine refuse a pass that would empty the state root, or
   take an explicit instruction to do it?** — *Recommendation:* refuse by
   default and give the engine one explicit, loud option meaning *"yes, I
   have been idle that long, do it"* — not a `--force`, but a flag that
   names the count it is authorising, so a script cannot carry it by
   accident. The preview already refuses, which costs nothing. —
   *Fallback:* the engine never refuses and relies on the operator running
   the preview first, which is a discipline and not a mechanism, and
   `docs/disk-policy.md` §3.6's idle-hub case is exactly when nobody is
   watching.
2. **The class map** (Alexa's open question, `docs/disk-policy.md` §3.1,
   §6 C) is untouched by this branch and does not block it: four required
   spans work against any answer. The engine's own flag shape — two
   numbers as §3.1 recommends, or four — does need the answer.

### Proposed board row, for `herd-merger` to place

| ORCH-146 | **`herd-hub retention-preview` — what M12's record retention would remove, and what it would cost, removing nothing.** The task that must exist before the engine: once the hub acknowledges a collection the runner's spool goes through that acknowledgement, so a record is the only copy of a requester's evidence and nothing in Herd can undo its removal. A lock-free subcommand of the hub binary beside `report`, arithmetic in `herd-hub/src/retention.rs`, taking **a span per priority class as four required arguments with no default and a compulsory unit** — D194's refusal to print a threshold, applied: the 14/90 figures are a recorded policy and a shipped binary stating one is a number nobody can later change, and four spans work against any answer to Alexa's open class map. It prints, per class, how many records are past the span, their bytes split into output bytes and record bytes, the oldest and the newest, what expires next, and **the clock reading it used**; it **exposes the selection the engine will call** (`retention::select`) rather than burying it in the printer. **Four things settled by measurement:** `updated_at` is a stable end time and not a last-touch clock (so a span is not an LRU); a clock the root's own records contradict, and a pass that would empty the root, both refuse and name the figures; a span of `0` means **never**, said in those words, and not `--output-floor 0`'s reading; and a job that is not over (the reaper's, ORCH-012/D134) or a quarantined record (D52) can never be shown as expired | `docs/plan.md` M12; E-15; `docs/disk-policy.md` §3; **D194** | herd-step | done | 14 new assertions, a fixture whose every `job.json` is padded to 512 bytes so each printed figure is checkable by arithmetic, asserted figure by figure and then as whole rendered text; a run while a serving hub held the `flock` with the state root proved byte-identical afterwards, and a run by hand against a hub that was really serving. **`store.rs` untouched**, `RECORD_VERSION` 1 and `PROTOCOL_VERSION` 2 unchanged. **It has no deletion path and no flag that could be given one.** The planner's proposed forward-clock rule was **refuted** and replaced by two threshold-free refusals; see the handoff | — | `history/2026-10-28-orch-146-retention-preview.md`; `herd-hub/src/retention.rs`; `herd-hub/tests/retention_preview.rs`; **D-NEXT** |

### Proposed `docs/decisions.md` entry, for `herd-merger` to number

```markdown
## 2026-10-28 — D-NEXT A retention pass that would empty the state root refuses

**Context.** M12's record retention removes a requester's evidence on a
timer, and nothing in Herd can undo that removal: once the hub
acknowledges a collection the runner's spool goes through that
acknowledgement, so the record is the only copy. `Timestamp::since`
saturates backwards only (`herd-hub/src/job.rs:76`), so a clock stepped
**backwards** makes every record read as younger than it is — safe — and a
clock stepped **forward** makes every record read as expired. The second
is the one that destroys a root.

**What was refuted.** The proposed guard was *"refuse when `now` is
earlier than the newest record's `updated_at`"*. That catches the
backward step and lets the forward step through, because a clock that
stepped forward is ahead of every record on the root. Worse, **a monotone
forward step is not detectable from a state root at all**: a year-old
newest record is exactly what a broken clock and a hub idle for a year
both look like (`docs/disk-policy.md` §3.6 says the idle case is real),
and nothing on the disk distinguishes them.

**Choice.** Refuse the consequence the two share, with two verdicts and no
threshold of Herd's own:

1. **The clock is behind the root's own newest record.** This hub wrote
   that record with this clock, so the clock has moved backwards; an age
   measured against it saturates and every figure means nothing. The pass
   does not run.
2. **Every record that can expire is past its span**, so a pass would
   leave no record behind. The pass does not run. The only figure in this
   test is the span the operator typed — no number is enshrined (D194) —
   and the population is *records that can expire*, not *records judged*,
   because one class whose span is `0` would otherwise mask a clock that
   had stepped forward under all the others.

A refusal prints the clock reading, the spans and the census and **no list
of what would go**, and exits non-zero: a refusal that still printed
"everything is expired" would be the output it exists to prevent.

**And a span of `0` means never expire**, which is deliberately **not**
`--output-floor 0`'s reading. The two settings are not symmetric because
their failures are not: the floor's zero loses bytes a budget was going to
take anyway, and this zero would lose the records. A span left unset, a
shell variable that expanded to nothing, or a `0` typed while reaching for
`90` must keep the evidence, not destroy it. Every span is also a
**required argument with a compulsory unit**: `--step 14` read as fourteen
seconds would select every record on the root.

**Cost, said plainly.** A hub that really has been idle longer than its
shortest span is refused too. For the preview that costs nothing. For the
engine it is one explicit instruction naming the count it authorises —
never a `--force` a script can carry by accident.

**Evidence.** `herd-hub/tests/retention_preview.rs`, ORCH-146:
`a_clock_behind_the_root_refuses_and_names_the_figures`,
`a_clock_a_year_forward_refuses_rather_than_reporting_everything_expired`,
`a_span_of_zero_never_expires_and_the_preview_says_so_in_those_words`,
and `a_backward_clock_can_only_make_a_record_read_young`.
```

## Resume point

The engine, which is ORCH-098 on the board. It calls
`herd_hub::retention::select(Judged::of(&record), &spans, now)` and acts
only on `Selection::Expired`; `Judged::of` is the one reader of the three
facts, so the engine cannot judge a record differently from the preview.
Before writing it, settle *Decisions needed* 1 with Alexa, and read
*Risks and findings* 2 and 4: `updated_at` is not monotone in job id, and
`collection_is_whole` is a second test the engine needs beside `select`
(`docs/disk-policy.md` §3.5) that the preview does not, because the
preview removes nothing.

The exact next command, to see what this branch gives the engine:

```sh
nix develop --command cargo test -p herd-hub --test retention_preview
```

## Processes

- `herd-hub` pid **3046925**, started by this agent on
  `127.0.0.1:18461` against a throwaway state root in its own scratchpad,
  to prove the preview runs against a hub that is really serving and
  leaves the root byte-identical. Stopped by this agent with
  `kill -TERM 3046925`; it reported a clean stop and exited. No other
  process was started, nothing was killed by name, and nothing on the
  consumer's side was touched.
- `CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/orch-146`, outside the
  worktree and off the root filesystem (gotcha
  `build-weight-off-root`). It is removed once the branch is pushed, and
  the agent's report says so.
