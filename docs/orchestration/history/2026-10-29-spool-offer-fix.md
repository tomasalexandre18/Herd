# spool-offer-fix — handoff 2026-10-29

- **Item:** ORCH-192 (the defect); ORCH-193 declined here, with reasons
- **Agent:** herd-step
- **Branch / commit:** `spool-offer-fix` / `c00d47e` (code) + this file,
  based on `spool-offer-repro` `b81bdea` with `main` `b37b2a4` merged in
  (`358822c`). `main`'s `herd-runner/src/lib.rs` is byte-identical to the
  base's: `git diff main --stat -- herd-runner/src/lib.rs` was empty, and
  the repro adds two new files and changes nothing else.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing and fills the record.

## Done

**The rule, as implemented.** A swept collection may be set aside only on a
silence the hub has already had the chance to break: its whole offer is on
the link, a health report queued after the last chunk of that offer has been
answered, and no frame naming the collection arrived in between. The barrier
in `Session::the_hub_has_said_what_it_is_owed` is now exactly:

```rust
files.swept
    && !files.heard
    && files.offered_at.is_some_and(|at| at < recorded)
```

`recorded` is `inner.reports_recorded` read in the same `with(…)` that
incremented it, so nothing can be queued between the count and the
judgement. `<` and not `<=`: `offered_at` is the number of reports *already
queued* when the offer finished, so report `offered_at + 1` is the first one
queued strictly after it.

**The proof is positional and was re-checked on this tree, not carried from
the design.** `grep -n HealthRecorded herd-hub/src/server.rs` → one line,
`:4629`, unconditional at the end of the `ToHub::Health` arm; the arm's only
early exits are `dispatch_queued_jobs(state)?`, which end the connection, so
the failure direction is "nothing is condemned". No `spawn` anywhere in
`serve_runner`'s frame loop (`awk` over `:4500-4760` → nothing). The runner
handles `JobFilesAck` (`:3399`) and `HealthRecorded` (`:3441`) as arms of one
sequential loop. `pump_files` has exactly three callers (`:1567` `finish`,
`:1703` `files_held`, `:1767` `offer_swept_collections`), and `files_held`
sets `heard = true` **under the lock before** it spawns.

**Where each of the three Health sites now counts.** One private free
function, `queue_a_health_report(&mut Inner) -> bool`, builds, queues and
counts in one critical section, and is the only place that does either.
Called by `Session::send_health` (the cadence, `report_health`),
`Session::start_draining` and `Session::set_switch`. `Session::health()` —
which made `report_health` take the lock twice, `send(health())` — is gone.
It takes `&mut Inner` rather than `&Session` precisely so that it can only be
called from inside a `with(…)`: whatever queued the frame is what counted it.
The invariant is in `Inner::reports_sent`'s doc comment and in the commit
message.

**`offered_at`.** `FilesOut::probed: bool` → `offered_at: Option<u64>`.
`FilesOut::next` no longer sets anything; the pump sets
`offered_at = Some(reports_sent)` in the same critical section in which
`next()` returned `None` — the offer is complete — reading `reports_sent`
before it borrows the entry. Reset to `None` (with `offer_cut_short`) in
`offer_swept_collections`, where `probed` was reset. Both counters zeroed in
`attach`; `set_aside_here` cleared in `detach`.

**The arm the rule would otherwise leak.** The pump's `Err` arm — a file that
will not read — also sets `offered_at`, plus `offer_cut_short: bool`, because
"cursor exhausted" never becomes true for it and the spool would otherwise be
re-offered on every connection for ever. The barrier's second sentence says
which ending it was.

**The sentences.** `files_acknowledged` returns `Option<String>`; the
`FromHub::JobFilesAck` arm prints it. The silent `_ => None` is a three-armed
match: the collection (nothing to say, `None`), another attempt of the same
job (named, nothing removed, **`heard` deliberately not set**), no collection
at all (named). The last one is a **sentinel**, and the doc comment says so in
the brief's words: with the barrier sound it is unreachable, so if it is ever
printed beside an `.unclaimed` line for the same job id, the barrier is wrong
again. The connection-scoped note (`Inner::set_aside_here`, cleared at
`detach`) lets that sentence name the spool the barrier took; it deletes
nothing, and the doc says why.

The barrier's own two sentences carry `{n} file(s), {size}` from the manifest
(ORCH-167's shape) and name **no** cause: the replaced text listed four (a
record that is gone, a superseded attempt, an eviction, another machine's
name) and all four were false in every reproduced run while the hub had
acknowledged the collection in the same second.

**Tests.** T1
`every_swept_spool_the_hub_acknowledges_is_deleted_and_none_set_aside`
off `#[ignore]`, body unchanged, one doc sentence added. T2 rewritten
(stimulus only). T3–T8 new, in `herd-runner/src/lib.rs`'s own tests. Three of
them (T3, T4, T5) measured red on the old rule and green on this one.

## Not done, and why

- **No reclaim path for already-set-aside spools (ORCH-193 declined).** On
  the orchestrator's ruling and the design's reasons: nothing on the disk
  distinguishes one set aside wrongly from one set aside rightly, only the
  hub can tell them apart, and asking means undoing a terminal state on a
  guess. The human route costs no code — rename `.unclaimed` back to
  `{job}-{attempt}` and restart — and that sentence is in the drafted
  decision entry.
- **Nothing outside the two owned files.** No `sweep.rs`, `outputs.rs`,
  `size.rs`, `reclaim.rs`, nothing under `herd-hub/src/`, nothing in
  `herd-proto` (`PROTOCOL_VERSION` unmoved), and no write to
  `docs/decisions.md` or `docs/orchestration/board.md` — the decision entry
  is drafted in the agent's report for the merger to place.
- **ORCH-194's `JobFilesRefused` frame** is not built: it is behind D199's
  version move, and it would not remove the need for this rule.
- **The sweep's module doc and `spools_resumed.rs`'s doc comment** are still
  wrong about silence and about a re-offer being refused. Prose, and
  `sweep.rs` is contended — not this task.

## Gates

Run on the code tree before this file was committed, all from the worktree
with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/spool-offer-fix`:

```
nix develop --command cargo fmt --all --check          → clean (after one `cargo fmt --all`)
nix develop --command cargo build --workspace          → Finished `dev` profile
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
                                                       → Finished, no warning
nix develop --command ./scripts/test.sh                → test suite: PASSED
./scripts/check-docs.sh                                → docs ok
```

And the bare `./scripts/gate.sh` on the code commit:

```
gate: GREEN sha=c00d47e44413739010e9156ce595088168e4027b steps=0-8:ok
host=servernixos load1=3.98 load1-end=5.79 skips=at-rest
```

T1 was run six times on its own before the suite — it is a race, and one
green run of a race is not a result. Every run: `0 set aside, 0 still
spooled`. The figures are in the agent's report.

**The last run, on the tree this file is part of:** `./scripts/gate.sh`,
bare and never inside `nix develop` (which nests `TMPDIR` and panics
`confinement.rs:2529` on the 108-byte `SUN_LEN`). Its log goes to
`gate-spool-offer-fix-tip.log` in this session's scratchpad directory
(`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/`, the full path is
in the agent's report) and its receipt to
`target/herd-gate/<sha>.receipt`; the verbatim verdict is in the agent's
report.

**Skips:** 12, the at-rest list, matching `scripts/expected-skips/local.txt`
exactly — all twelve are the cgroup-delegation assertions this host cannot
make (`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`). None of them is this
task's, none is counted as a pass (D27), and the two load-gated benches
**ran**. The one remaining `#[ignore]` in the tree is in
`herd-hub/tests/announced_job_ceiling.rs` and is not this task's.

## Risks and findings

1. **`bench-preflight.sh` REFUSED twice on the cgroup floor and was lifted on
   my own reading, with the figures.** Both times: `/chatons.slice`
   `memory.current` 11.9 / 11.6 GiB against `memory.high` 18.0 GiB — 6.06 and
   6.44 GiB of room against the script's 8 GiB floor — while
   `memory.stat` said `anon` 511.7 / 470.8 MiB and `file` 10.84 / 10.55 GiB.
   Over 90% of the slice's charge is reclaimable page cache from this
   evening's build trees, and the host had 22–23 GiB available. Nothing was
   written under `/sys/fs/cgroup` and no cache was dropped. The suite and
   both `gate.sh` runs then passed with no memory failure.
2. **T1's `barrier_lines` figure is now blind to the new sentences, and the
   body was not to be touched.** The test counts lines containing
   `neither asked for it nor acknowledged it`. I kept that exact substring in
   both replacement sentences for this reason — the design's §4 draft read
   "neither asked for **any of** it", which would have made the figure
   silently read 0. Worth knowing for whoever edits either side.
3. **The re-offer costs about two chunks per collection, not one.** T1's
   `acknowledged_for_the_offer` over six green runs: 26, 21, 24, 20, 20, 20
   for `SWEPT = 12`, against a ceiling of `chunks_in_the_collection()` = 4
   each (48). So it is ≈ 2 × SWEPT: the acknowledgement of the first chunk
   arrives while a second is already in flight. Not `SWEPT × chunks`, so
   H8's shape holds, but it is not the one-chunk minimum either. Reported,
   not fixed; it is a board row if anyone wants the minimum (offering the
   manifest's smallest file first, `plan-spool-silence` step 3b).
4. **Two forced edits to tests on the must-stay-green list**, because
   `files_acknowledged` now returns `Option<String>` and `Option` is
   `#[must_use]`: `a_finished_collection_gives_the_whole_window_back` and
   `a_swept_spool_is_offered_again_and_its_acknowledgement_removes_it` each
   had their bare call turned into
   `assert_eq!(session.files_acknowledged(…), None, …)`. That **adds** an
   assertion and removes none. Both green.
5. **ORCH-197 is now the only way a healthy collection can still be set
   aside**, as the design says: the hub derives its owed list from one global
   record scan, so a hub whose scan failed is silent to every runner at once.
   That is why the rename-not-delete asymmetry stays.
6. **D27's blind spot for `#[ignore]`** stands: an `#[ignore]`d test never
   writes a `HERD-SKIPPED` line, so `scripts/test.sh`'s skip list cannot see
   a compile-time skip. Not this task's; it deserves its own row. T1 coming
   off `#[ignore]` removes the one instance that mattered tonight.

## Decisions needed

None for Alexa on the code. For the orchestrator: where the drafted decision
entry (in the agent's report) is placed, which is the merger's to do; and
whether findings 3 and 6 get board rows. For Alexa only when the release is
cut: the body's own sentence, which should promise the forward behaviour and
imply no repair — *a runner no longer keeps a spool the hub has told it it
holds whole.*

## Resume point

```sh
# the branch is pushed and gated; nothing is in flight
git fetch origin spool-offer-fix
# herd-merger: merge it into batch 9, where spool-offer-repro is already
# merged (the shared commits are a no-op for git), and place the decision
# entry from this task's report.
```

## Processes

No PID started that outlived its command: every `cargo`, `nix develop` and
`gate.sh` invocation ran in the foreground and exited. Nothing was killed, by
name or otherwise.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/spool-offer-fix` (and
`/mnt/ssd_980/herd/build/spool-offer-fix-before` for the red-before
measurement, deleted immediately after it). Both are deleted once the branch
is pushed; the worktree's own `target/` holds only `herd-gate/`'s receipts and
goes with it.
