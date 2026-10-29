# spool-offer-repro — handoff 2026-10-29

- **Item:** ORCH-NEXT — the brief named no id; `herd-merger` allocates it, as
  it does a decision number. Closest existing rows: ORCH-167 (nothing removes
  a spool the hub stayed silent about) and ORCH-108 (may a runner ever remove
  an `.unclaimed` spool); this row is neither, because the hub here is **not**
  silent
- **Agent:** herd-step
- **Branch / commit:** `spool-offer-repro` / see the report, based on
  origin/main `5d422bf7f1378c9242ef9dda6db9ee97cd8f4e40`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The planner's reading was verified line by line before anything was
written, and it holds.** Each of the five claims, read at `5d422bf`:

- `herd-hub/src/store.rs:2086-2090` — `write_output_chunk`'s *already here and
  whole* arm returns `self.collection_state(id, &record)`, neither writing nor
  refusing; `:968-974` returns `Complete` when `collection_is_whole`
  (`:554-559`, bytes in `files/` at the manifest's length).
- `herd-hub/src/server.rs:4273-4290` — on `Complete` the hub sends
  `FromHub::JobFilesAck` and prints `has every file of its collection`.
- `herd-hub/src/store.rs:2737-2778` — `collection_owed` returns `None` when
  `collection_is_whole`, so `ask_for_the_files_still_owed`
  (`server.rs:4784-4806`) sends no `JobFilesHeld` for such a collection — and
  `heard` is set only by that frame (`herd-runner/src/lib.rs:1676`).
- `herd-runner/src/lib.rs:1803-1829` — the barrier condemns on
  `swept && probed && !heard`, fired by the first `HealthRecorded`
  (`:3427`); `:860` sets `probed = true` under the lock, before the disk read;
  `:3279` only spawns the pumps and `:3282` reports health on an interval
  whose first tick is immediate.
- `herd-runner/src/lib.rs:1708-1713` — `files_acknowledged` on an entry the
  barrier has already removed takes the `_ => None` arm: nothing printed,
  nothing undone.

**One new test, no fix:** `herd-hub/tests/spool_offer_answered.rs`,
`every_swept_spool_the_hub_acknowledges_is_deleted_and_none_set_aside`. Real
`herd-hub` and real `herd-runner` **binaries**, through `tests/real_herd`, so
that what each process *said* is captured and assertable. A hand-driven peer
runs the jobs and hands the hub every byte; the life that matters is the
second one, which is the real runner going through its own start-up —
`sweep::pick_up`, `settle_against_state_root`, `offer_swept_collections`,
`report_health` — in its own order. The spool directory is a fixture for the
disk only, at the name `outputs::spool_path` writes and the sweep parses,
because the ordinary path has already, correctly, deleted it.

It is **RED on `main`**, in 6 runs of 6. Verbatim output is in the report.

**The brief's premise is confirmed and one of its predictions is not. Both
are measured, and the test reports the figures whichever way it goes.**

## Not done, and why

- **No fix.** Not the barrier, not `files_acknowledged`, not `sweep.rs`, and
  nothing under `herd-hub/src/`. The diff is one new test file plus this
  handoff.
- `a_swept_spool_no_hub_asks_for_is_set_aside_and_never_offered_again` is
  untouched: it asserts the current behaviour correctly, and changing it
  belongs to the fix.
- `herd-runner/src/sweep.rs` untouched — `sweep-names-why` holds it.
- No version number, no board edit (the row is proposed in the report), no
  `docs/decisions.md` entry (the brief asked for none).

## Gates

Run **before** this commit, verbatim in the report:

- `cargo fmt --all` — applied, then clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — `Finished dev
  profile`, no warning.
- `cargo test -p herd-hub --test spool_offer_answered -- --ignored` — RED, 6
  runs of 6, 4 to 10 of 12 collections condemned each time and **zero**
  `cannot send` lines in every one.

The **last run**, on this committed tree, is the bare
`./scripts/gate.sh` — never nested in `nix develop`, which panics
`confinement.rs:2529` on `SUN_LEN`. Its log goes to
`spool-offer-repro-gate.txt` in this session's scratchpad (the full path is
in the report) and its receipt to this worktree's `target/herd-gate/`; the
verdict line and the named skip list are in the report, not here.

**Skips:** the new test is `#[ignore]`d with its reason, so cargo prints it as
`ignored` and it is **never** reported as a pass. One caveat, stated because
the brief expected otherwise: an `#[ignore]`d test does **not** appear in
`scripts/test.sh`'s `HERD_SKIP_LOG`, which collects only the `HERD-SKIPPED`
lines a test writes while *running*. A test that never runs writes nothing.
So the gate's named skip list is unchanged by this branch, and the evidence
that this assertion did not run is cargo's own `1 ignored` plus the reason
string. That is a real gap in how D27 is enforced for compile-time skips, and
it is a finding below rather than something this task changed.

## Risks and findings

1. **The defect is a race, not a certainty, and the number of swept
   collections is what decides it.** The first fixture here swept **one**
   collection and was **green** three runs running: every chunk acknowledged,
   the spool deleted, the barrier arriving to find nothing left to condemn. A
   pump sets `probed` only once it holds a permit from the link's window
   (`herd_runner::FILES_IN_FLIGHT`, **8**), so a lone pump takes its permit,
   queues every chunk and is answered before `report_health` has even been
   spawned. The consumer swept **75** against that window: the pumps that wait
   take their permits back *after* `ToHub::Health` is on the queue, and each
   then sets `probed` and starts a disk read inside the window the first
   `HealthRecorded` closes. The test therefore sweeps `FILES_IN_FLIGHT + 4`,
   which is the smallest fixture with the production shape. **A fix validated
   against one collection would validate nothing.**
2. **The predicted ENOENT fingerprint is absent here too, and it is not a
   second swallowed error.** 6 runs, 4 to 10 collections condemned each, and
   not one `cannot send … No such file or directory` in the runner's output —
   the same absence the consumer's journal showed. The cause is in the barrier
   and not in the disk: `the_hub_has_said_what_it_is_owed` removes the map
   entry **inside** `self.with(...)` and renames **after**, in the loop that
   follows, while the pump's one and only read site sits behind
   `inner.collections.get_mut(&job_id)` under that same lock
   (`lib.rs:1632-1636`). There is no instant in which the entry exists and the
   directory is gone, so a pump that wakes after the barrier returns through
   its ordinary "nothing more to send" door. Of the coordinator's two
   candidates, **neither** is right: the pump *does* read — the hub
   acknowledged the re-offers 9 to 28 times per run — and nothing is swallowed.
   It is a third thing, and the fix is correspondingly narrower: **one silent
   exit to account for, not two.**
3. **`files_acknowledged`'s silence is still worth a line.** It remains a
   no-op that prints nothing when the entry is gone; that is the step at which
   37.6 GiB was lost without a word. Whatever the fix does to the barrier,
   this arm should say something.
4. **The runner's own sentence is false in every particular**, which is the
   most misleading artefact of this defect. Verbatim from a red run:
   `the collection this runner holds for j0000000007 (attempt 1) was offered to
   the hub and the hub has neither asked for it nor acknowledged it: its record
   is gone, the attempt was superseded, its collection was evicted there, or
   this machine is not the one that record names.` The record is present, the
   attempt is current, the collection was not evicted, and the machine **is**
   the one the record names — and the hub acknowledged it, repeatedly, in the
   same second. An operator reading that line would go looking in the four
   wrong places.
5. **The rule the code defeats is written in a comment directly above it**
   (`lib.rs:1786-1794`): *"A hub that already holds the collection answers the
   offer with `JobFilesAck` and the spool goes on the ordinary path; … Silence
   is the third case only."* The comment is correct; the barrier does not
   implement it.
6. **`#[ignore]` is invisible to the gate's skip list** (see *Gates*). Worth a
   row of its own: D27's enforcement covers runtime skips and not
   compile-time ones.

## Decisions needed

None for Alexa. One thing for the orchestrator to place: the **fix** task,
which this test is the acceptance criterion for. The narrow shape the evidence
now supports — do not let the barrier judge an offer the hub has not had the
chance to answer, rather than anything about the disk — plus a line where
`files_acknowledged` currently says nothing. That task owns `lib.rs` and is
the task that deletes the `#[ignore]` here and deliberately rewrites
`a_swept_spool_no_hub_asks_for_is_set_aside_and_never_offered_again`.

## Resume point

```sh
cd <the fix task's worktree>
# make the barrier wait for the answer to its own offer, then:
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/<task>
nix develop --command cargo test -p herd-hub --test spool_offer_answered -- --ignored --nocapture
# it must go green with `0 set aside, 0 still spooled` — and the `#[ignore]`
# comes off in the same diff.
```

## Processes

Started and ended by this task: the `cargo`, `clippy`, `rustfmt` and
`cargo test` processes of the runs above, each awaited to completion, plus the
`herd-hub`, `herd-runner` and `herdctl` child processes the test's own harness
starts and kills on drop (`kill_on_drop(true)`). None left running; nothing
killed by name and no `pkill`. **PID 3046930**, the stray `herd-hub` on
`127.0.0.1:18461`, was left alone and its port never taken — every hub here
binds a port `real_herd::free_port` chose.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/spool-offer-repro`, outside
the worktree and off the root filesystem; it is removed once the branch is
pushed, and the removal is stated in the report.
