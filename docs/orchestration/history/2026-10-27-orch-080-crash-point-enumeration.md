# orch-080-crash-point-enumeration — handoff 2026-10-27

- **Item:** ORCH-080
- **Agent:** herd-step
- **Branch / commit:** `orch-080-crash-point-enumeration` / `043d145`,
  based on `origin/main` `cd59ef5`
- **PR:** [#92](https://github.com/tomasalexandre18/Herd/pull/92), filled
  in from `.github/pull_request_template.md`. **Not merged**
- **Worktree:** this agent's own, `/mnt/ssd_980/herd/claude/tmp/claude-1002`
  then `/-mnt-ssd-980-herd-project`, the session id
  `3ddf11ac-718a-4231-bc8d-45291a39ef82`, then `/scratchpad/orch-080/wt` —
  one path, broken here only to stay inside 79 columns.
  `/mnt/ssd_980/herd-project` was read and never written.

## Done

Two new files, two edited, **nothing under `herd-hub/src`, `herd-runner/src`,
`herdctl/src` or `herd-proto/src`**. No `fsync` was dropped, added or
reordered: this branch describes today's tree.

**`herd-hub/tests/crash_points.rs`** — ten `#[ignore]`d cases. Every on-disk
state a power cut can leave in one job directory is **built by construction**:
7 shapes of `job.json` × 11 of `events.jsonl` × 4 of stale temporary =
**308 states**, each written byte for byte into a `tempfile::tempdir()` and
each opened with `JobDirStore::open_with`. No hub process is started, no
signal is sent, nothing is killed, and nothing outside the temporary directory
is written. The table is **asserted**, not only printed, so it cannot drift
from the code without going red.

**`docs/remediation/13-crash-point-enumeration.md`** — the write-up: the three
axes, the completeness argument, the 7 × 11 verdict grid, each behaviour
separated into *what we have* and *whether it is what we want*, one proposal,
and F/H/I.

**`docs/remediation/README.md`** — the index row for `13`, the date line, the
note that `13` is written against a later base than every piece before it,
and `13` added to the *mesures* list with the sentence that it carries no
timing.

**`docs/orchestration/board.md`** — ORCH-080 moved `owed` → `done` with its
result. No other row touched.

### The three findings

1. **The brief's finding is verified, at `2ca6dc7`.** A `job.json` the hub
   cannot parse makes it refuse the **whole state root**:
   `store.rs:1141-1142` (the `?` on `serde_json::from_str::<VersionProbe>`)
   → `store.rs:899` (`self.read_stored(&id)?` in `recover`) → `store.rs:436`
   (`store.recover()?` in `open_with`) → `main.rs:224`, whose `?` leaves
   `main` before a port is bound. D52's quarantine is `store.rs:1150-1157`
   and covers **labels only**. The index cannot route around it:
   `usable_index` reads every record the index omits and sets the index aside
   on an unreadable one (`store.rs:1052-1054`). 176 of the 308 states refuse,
   and a second and third job on the same root go down with the first.
2. **Step B's hazard is 4 of the 308.** `events.jsonl` one line ahead of
   `job.json`, over a *reason*. Unreachable today — `write_file_with`'s
   `sync_dir` (`durable.rs:56`) returns before `append_line` is issued.
   Acceptable for an explanatory write, **not** for a decisive one.
3. **ORCH-086 confirmed.** Nothing removes a stale temporary at
   `JobDirStore::open`; all 99 states that carry one still carry it
   afterwards. New beside it: a temporary left in `files/` by the
   inline-output write (`store.rs:1752`) is **counted against the output
   budget** by `directory_bytes` (`store.rs:2733-2749`, which has no
   dot-filter) — 4096 bytes measured — and the sweeper cannot take it back.

## Not done, and why

- **No change to `herd-hub/src`.** The brief says propose, do not make. §5 of
  `13` proposes quarantining a record that will not parse; it is a separate
  task and a separate decision.
- **No new board row for that proposal.** The brief says update ORCH-080's
  row and nothing else in the board. The proposal is in `13 §5` and in the
  PR's *To record*; whoever allocates the row should rank it **before**
  ORCH-085.
- **No `D-NEXT` entry.** Not asked for, and nothing here decides anything.
- **No real power cut, no device, no VM, no root.** By design and by brief.
- **The frequency question stays `I`.** Which of these states a real ext4 cut
  on the deployment's volume produces, and how often, needs the device and
  the mount options; invariant 9 forbids both. `10 §2`'s `I` stands.

## Gates

All five ran in the worktree, under `nix develop` for the four Rust ones.

`nix develop --command cargo build --workspace`:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.23s
```

`nix develop --command cargo fmt --all --check`:

```text
warning: Git tree '…/scratchpad/orch-080/wt' is dirty
FMT-OK
```

(no diff printed; `FMT-OK` is the shell's own echo after a zero exit)

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```text
warning: Git tree '…/scratchpad/orch-080/wt' is dirty
    Checking herd-hub v0.3.7 (…/scratchpad/orch-080/wt/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.36s
```

`./scripts/check-docs.sh`:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`nix develop --command ./scripts/test.sh` — **RED, for the known host
reason**, exit 101:

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`

==============================================================
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
==============================================================
```

The two failures are **ORCH-069's two, and no others**:

```text
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at herd-hub/tests/slots.rs:744:9:
  left: "built with -j2"
 right: "built with -j7"

failures:
    a_killed_hub_reads_the_reservations_back_off_the_records
test result: FAILED. 4 passed; 1 failed; … (-p herd-hub --test admission)
```

`admission.rs:419` asserts `"3 GiB free of its 7 GiB memory admission, 5 GiB
needed"` — a constant written for a 17 GiB machine against a figure the hub
derives from this host's live `MemTotal`. `slots.rs:744` is the other.
**Not caused by this branch**: it touches no `src`, and its only new test file
is `#[ignore]`d in full. ORCH-069's board row already records that the suite
is red on `origin/main` alone on this workstation; **I did not re-run the
suite on a pristine `cd59ef5` to re-prove it**, so that half is cited, not
measured here. By D27, `slots.rs:744` being *skipped* on CI is **not** a pass
and this handoff does not call it one.

The new harness is ignored by the suite, as intended:

```text
     Running tests/crash_points.rs (target/debug/deps/crash_points-…)
running 10 tests
test a_record_renamed_with_no_event_appended_is_permitted_and_silent ... ignored, ORCH-080's enumeration; run with --ignored
test a_temporary_in_files_is_held_output_for_ever ... ignored, ORCH-080's enumeration; run with --ignored
test a_torn_record_refuses_the_whole_state_root ... ignored, ORCH-080's enumeration; run with --ignored
test a_whole_line_that_is_not_json_refuses_the_query_and_not_the_hub ... ignored, ORCH-080's enumeration; run with --ignored
test an_event_ahead_of_the_record_is_neither_seen_nor_repaired ... ignored, ORCH-080's enumeration; run with --ignored
test an_interrupted_mint_loses_the_job_quietly ... ignored, ORCH-080's enumeration; run with --ignored
test every_crash_point_a_waiting_write_can_leave ... ignored, ORCH-080's enumeration; run with --ignored
```

and run on purpose, it is green:

```text
nix develop --command cargo test -p herd-hub --test crash_points \
  -- --ignored --nocapture --test-threads=1

308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.72s
```

**Skips.** The suite's 12 `HERD-SKIPPED` lines were printed verbatim by
`scripts/test.sh` and are copied as skips, never counted as passes (D27): 10
cgroup-v2 delegation and 2 ZFS `io.max` ceiling, the same split
`README.md §"The three marks"` records. The new harness skips nothing — its
ten cases are `#[ignore]`d by attribute, which is not a skipped assertion but
a case the suite does not run, and the handoff reports both states of it
above.

### CI, run 40485375703 on `24c2e53`

Both required jobs green:

```text
check   pass   8m24s
docs    pass   3m56s
test suite: PASSED
15 assertion(s) COULD NOT RUN on this host and were skipped.
```

**15, not 12** — the skip count is per host (ORCH-070), and three of CI's
are not this workstation's. One of the three is
`the_derived_j_is_recorded_and_is_what_the_job_is_given`, which is **red
here and skipped there**. By D27 that skip is **not** a pass, and this
handoff does not claim the suite is green on CI for `slots.rs:744`: it
claims CI skipped it. The other two jobs of the run are the gates
`herd-merger` reads.

### The merge of PR #85, and the gates run again on top of it

PR #85 landed on `main` as `2ca6dc7` while this branch was open. It touches
`herd-hub/src/store.rs`, so **every `file:line` in `13`, in the harness and
in the board row was re-read on `2ca6dc7` and corrected** before the merge
was committed. The ones that moved are all in the second half of
`store.rs` — `job_events`, `read_optional`, `directory_bytes`,
`unfinished_jobs`, `collections_owed`, `read_the_readings` and `mod tests`
— and every one was re-checked by printing the line.
`13`'s base is restated as `2ca6dc7` and says at its head why it
is not `0a2cf73` like the pieces before it.

**The enumeration is unchanged by the merge.** Same 308 states, same
verdicts, re-run on the merged tree:

```text
308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.23s
```

`build`, `fmt --check`, `clippy -D warnings` and `check-docs.sh` were all
re-run on the merged tree and are all clean.

`nix develop --command ./scripts/test.sh` on the merged tree is **redder
than before — six targets, not two**:

```text
error: 6 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test announced_job_ceiling`
    `-p herd-hub --test killed_hub`
    `-p herd-hub --test owner_control`
    `-p herd-hub --test slots`
    `-p herd-runner --test busy_probe`
 AT LEAST 14 assertion(s) COULD NOT RUN on this host and were skipped
test suite: FAILED (cargo test exited 101)
```

**This is the shared host, and the evidence says so rather than my saying
so.** Almost every new failure is a timeout at one of two lines —
`herd-hub/tests/real_herd/mod.rs:649` and `:789`, both "timed out waiting
until …" — and `/proc/loadavg` read at the end of the run was
**`13.46 14.73 10.15`** on a box several agents were building on. Two
independent checks separate it from the code: **`main`'s own CI at
`2ca6dc7` is `success`** (run 40486480230), and this branch adds no `src`
and one fully `#[ignore]`d test file. **I did not re-run the six targets on
a quiet host**, so this paragraph is an attribution with its evidence
named, not a measurement: the local suite is reported **red**, and CI on
the branch is the arbiter.

#### The six reds, measured against pristine `main` rather than attributed

The paragraph above was written as an attribution. It has since been
**measured**, on the same host, with the load down to `4.28 6.96 8.19`:
`origin/main` at `2ca6dc7` was checked out into a second worktree with no
file of this branch in it, and the two targets that carry every failure
were run there.

```text
# pristine origin/main 2ca6dc7, no file of this branch present
$ cargo test -p herd-hub --test admission
test result: FAILED. 0 passed; 5 failed; …
    a_killed_hub_reads_the_reservations_back_off_the_records
    a_refusal_carries_the_free_figure_and_the_needed_figure
    a_starved_reservation_waits_then_starts_and_leaks_nothing
    a_waiting_job_says_why_again_on_a_clock_and_does_not_flood
    the_load_is_on_the_placement_line_and_stops_nothing

$ cargo test -p herd-runner --test busy_probe
test result: FAILED. 6 passed; 2 failed; …
    a_probe_child_that_left_the_group_dies_when_the_runner_is_stopped
    a_runner_told_to_stop_takes_its_probes_children_with_it
```

**The same seven names, in the same two targets, on `main` with none of
this branch's files present** — so the red is this host's and `main`'s
together, and not this branch's. The only Rust difference between this
branch and `main` is one added test target: `git diff origin/main --stat
-- '*.rs'` is `herd-hub/tests/crash_points.rs | 1013 +`, and every case in
it is `#[ignore]`d.

Two further facts worth keeping apart. **The admission failures are
timeouts, not ORCH-069's assertion** — every one of them panics at
`real_herd/mod.rs:649` or `:789` with *"timed out waiting until …"*, so
`admission.rs:419`'s memory constant is never reached on this host and
ORCH-069 names only `slots.rs:744` of what is red here today. And
`announced_job_ceiling`, `killed_hub`, `owner_control` and `slots` were
**not re-run**: `cargo test` stops after the first failing target, so those
four are reported from the full-suite run only.

The comparison worktree was removed after the run; nothing of it remains.

## Risks and findings

- **The proposal of `13 §5` is a behaviour change on a crash path and is not
  made.** Quarantining a record that will not parse, instead of refusing the
  root, is small in lines — the `Stored::Quarantined` machinery, the list, the
  per-start message and the count at `main.rs:234` all exist — but it changes
  *"the hub does not start"* into the quieter *"the hub starts and is missing
  jobs"*. It needs its own task, its own test and its own decision, and
  `RECORD_VERSION` mismatch should probably **stay** an error.
- **The enumeration supports ORCH-084 only under the explanatory/decisive
  split the board already states.** If a future branch drops `sync_dir`
  unconditionally, the four `⚠` states become reachable for a terminal
  transition, and that is a lie about a verdict. The harness asserts the
  narrow case and would not by itself catch the wide one; `13 §4.1 H` says
  what to add.
- **A premise of the brief, narrowed, not refuted.** The brief asks to "start
  a real hub on each". 308 hub *processes* would need 308 ports and far more
  time than the finding is worth, so each state is opened with
  `JobDirStore::open_with`, which is literally what `main.rs:224` calls and
  whose error is literally what stops the binary. The verdict "the hub refuses
  to start" is therefore exact, and
  `one_torn_record_takes_every_other_job_with_it` shows the blast radius on a
  multi-job root.
- **One citation in the brief needed a small correction.** `07`'s stale-
  temporary material is at **§5 item 5**, not §5.5; §5.5 does not exist.
- **The number collision the brief warned of happened, and this branch
  gave way.** ORCH-079 (PR #95, branch `orch-079-ordering-oracle`) is
  adding `12-ordering-oracle.md`. This piece was written as `12` on the
  same day, so it **renumbered itself to `13`** rather than contest the
  number, which is what the brief asked for. `13` was checked free against
  every open branch — PRs #93, #94, #95 and #96 — before it was taken, and
  the index now says in one paragraph why `12` is absent from it. Nothing
  depends on the number except the filename, the heading and the
  cross-references, and all three moved together.

## Decisions needed

**One, and it is not urgent today.** Should a `job.json` the hub cannot parse
be **quarantined** (the job skipped and named at every start, the hub running)
rather than **refuse the whole state root**?

- **Recommendation: yes, as a task of its own, ranked before ORCH-085.** D52
  already decided the principle and its own comment states it
  (`store.rs:1129-1135`); only the implementation stops three lines short. The
  consumer runs the hub in docker compose and cannot be asked to find and
  delete a file inside a volume, and invariant 9 forbids us doing it.
- **Fallback:** leave it, and then **do not take ORCH-085 (step C)**, which is
  the step that turns this from latent into live. The board's own sequencing
  already says C waits on this; this piece says it should wait on the *fix*,
  not only on the measurement.

## Resume point

```sh
gh pr view <n>                       # CI: check + docs
# then, for whoever takes ORCH-084:
nix develop --command cargo test -p herd-hub --test crash_points \
  -- --ignored --nocapture --test-threads=1
```

Read `docs/remediation/13-crash-point-enumeration.md` §4.1 and §6 before
dropping `sync_dir`: they say exactly which four states B creates and under
what condition they are acceptable. The proposal of §5 wants a board row of
its own, allocated by whoever owns the board.

## Processes

No background process was started and none was killed. Every command above
ran in the foreground, in this agent's worktree, under `nix develop`. No
`pkill`, no `killall`, nothing by name. Nothing was written under
`/mnt/ssd_980/herd/claude/` outside this agent's scratchpad, nothing under
`/mnt/ssd_980/herd-project`, and nothing touched `/etc/nixos`, `/srv`, a
container, a service or anything of the chatOnS project.
