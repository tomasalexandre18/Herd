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

**`docs/remediation/12-crash-point-enumeration.md`** — the write-up: the three
axes, the completeness argument, the 7 × 11 verdict grid, each behaviour
separated into *what we have* and *whether it is what we want*, one proposal,
and F/H/I.

**`docs/remediation/README.md`** — the index row for `12`, the date line, the
note that `12` is the first piece based on `cd59ef5` rather than `0a2cf73`,
and `12` added to the *mesures* list with the sentence that it carries no
timing.

**`docs/orchestration/board.md`** — ORCH-080 moved `owed` → `done` with its
result. No other row touched.

### The three findings

1. **The brief's finding is verified, at `cd59ef5`.** A `job.json` the hub
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
   budget** by `directory_bytes` (`store.rs:2730-2746`, which has no
   dot-filter) — 4096 bytes measured — and the sweeper cannot take it back.

## Not done, and why

- **No change to `herd-hub/src`.** The brief says propose, do not make. §5 of
  `12` proposes quarantining a record that will not parse; it is a separate
  task and a separate decision.
- **No new board row for that proposal.** The brief says update ORCH-080's
  row and nothing else in the board. The proposal is in `12 §5` and in the
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

## Risks and findings

- **The proposal of `12 §5` is a behaviour change on a crash path and is not
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
  narrow case and would not by itself catch the wide one; `12 §4.1 H` says
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
- **A number collision is possible at rebase.** ORCH-079 may be adding a
  document at the same time. If `12` collides, renumber this one; nothing in
  `12` or in the index row depends on the number except the filename and the
  anchors.

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

Read `docs/remediation/12-crash-point-enumeration.md` §4.1 and §6 before
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
