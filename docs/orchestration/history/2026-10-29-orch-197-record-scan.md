# orch-197-record-scan — handoff 2026-10-29

- **Item:** ORCH-197 (fixed). ORCH-022 / V3 offered mid-task and **taken,
  then not shipped**: the fix its brief recommends is refuted by
  measurement on this tree. See *Risks and findings* 1 and
  *Decisions needed*.
- **Agent:** herd-step
- **Branch / commit:** `orch-197-record-scan` / `3d44312` (code) + this
  file, based on `origin/main` **`1a8c1ae`**
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### What the scan is, and what fails

One function, `ask_for_the_files_still_owed` (`herd-hub/src/server.rs`,
`:5315` on `1a8c1ae` — the brief's `:4786-4791` had moved), called once per
runner dial from `serve_runner` `:3287`, before the hub reads a frame from
that connection. Its whole answer came from `JobStore::collections_owed`,
and on its error it returned having sent nothing:

```rust
    let owed = match state.store.collections_owed() {
        Ok(owed) => owed,
        Err(error) => {
            eprintln!("herd-hub: cannot tell which collections are outstanding: {error}");
            return;
        }
    };
```

`collections_owed` (`herd-hub/src/store.rs:2859`) is **one `readdir`**, and
it has exactly one error path:

```rust
        for name in durable::names_in(&self.jobs_dir())
            .map_err(|error| failed("listing", &self.jobs_dir(), &error))?
```

`durable::names_in` is `fs::read_dir` plus each entry (`durable.rs:486`), so
the errors that reach it are the directory's: `EACCES` on a tree whose mode
or ACL changed, `ENOTDIR`, `EIO`, `ENOENT`, `ENFILE`. The per-job half
cannot fail it — `collection_owed` swallows an unreadable record with
`let Ok(Some(record)) = JobStore::job(self, id) else { return Ok(None) }`
(`store.rs:2882`) and nothing else inside it returns `Err`, so **the only
way `collections_owed` fails is the listing**. Nothing is cached: the scan
is recomputed per dial, which is why the failure is not sticky and not
per-runner — it is one directory, re-read identically for every connection.

### Why one failure became a global silence

Not sharing and not caching: the frames. The owed list is *N* unprompted
`JobFilesHeld` and nothing else, and the runner's rule reads the absence of
them against a barrier — the answer to a health report, frames on one link
being ordered (`herd-runner/src/sweep.rs:27-52`,
`Session::the_hub_has_said_what_it_is_owed`, `herd-runner/src/lib.rs:1804`).
So silence **plus** `HealthRecorded` is a complete sentence on this wire:
*"this hub holds nothing of yours."* Returning early sent that sentence. One
unlistable directory therefore said it to every runner at once, every runner
read a genuine silence, and every one of them correctly set its spools
aside. One failure, the whole fleet.

### The fix

`ask_for_the_files_still_owed` now returns `OwedAtJoin`, which the
connection carries into `relay_from_runner`:

- `Said` — the scan ran, the join list is complete, and a collection the hub
  did not name is one it is not waiting for. Byte for byte today's
  behaviour, one discriminant test added to the chunk path.
- `CannotTell { answered }` — the scan did not run. Nothing is claimed about
  any collection, and each one the runner **offers** is answered from that
  job's own record: `OwedAtJoin::answer_this_offer`, called from the
  `ToHub::JobFileChunk` arm before `files_may_land`, derives the one answer
  with `JobStore::collection_owed` — D79's "one definition, asked two ways",
  the per-job form of the same derivation — and sends the `JobFilesHeld` the
  join list would have carried, once per collection per connection.

**It does not lie.** Nothing is asserted about a collection whose record has
not been read. A collection the hub already holds in full is still not
claimed as owed, because `collection_owed` answering `None` is a fact read
off that job's record, not a guess — asserted end to end in
`a_collection_this_hub_already_holds_is_not_claimed_as_owed`. The failed
listing says nothing about any record, and the fix claims exactly nothing on
its authority.

**It does not stop serving.** A failed `readdir` is not a failed `open`: the
fixture proves both halves in the same breath. Admission, dispatch, logs,
verdicts and file chunks are untouched; the collection *lands and is
acknowledged* while the listing is broken, which is the point — bytes
recovered rather than renamed `.unclaimed`. And the answer to a health
report still goes out. That last one is not a style choice: withholding
`HealthRecorded` is the obvious "say nothing rather than lie", and the code
refutes it — `herd-runner`'s `HUB_SILENCE_LIMIT` is 60 s with the comment
"the hub answers every health report, so four unanswered reports is a dead
link" (`herd-runner/src/lib.rs:97-106`), and `HealthRecorded` is the only
frame an idle runner ever gets (`server.rs:4629`, the single site). A hub
that went quiet there would be dropped by its whole fleet in a redial loop.

**No frame, and no runner change.** `JobFilesHeld` is the frame the runner
already resumes on (`Session::files_held`, `lib.rs:1675`, the only place
`heard` is set), and it may arrive at any time. Ordering does the rest: the
offer's first chunk precedes the completed offer, which precedes any health
report queued after it, and the hub answers in order — so the runner cannot
reach its barrier without having read the answer. That holds for the old
barrier (`probed && !heard`) and for `spool-offer-fix`'s new one
(`offered_at.is_some_and(|at| at < recorded)`): both are keyed on
`HealthRecorded`. `herd-proto`, `herd-runner`, `store.rs`, `report.rs`,
`retention.rs` are untouched.

### The tests, and the red-before/green-after measurement

**The fixture is the listing itself, never a mock.** The jobs directory is
left at mode `0o111`: `fs::read_dir` fails `EACCES` while
`open("jobs/<id>/job.json")` still succeeds. Both halves are asserted in the
fixture (`AListingThatFails::over`, `::but_a_record_still_opens`), the
permissions are restored by a `Drop` guard however the test ends, and a
process permissions do not apply to is a **skip** and not a pass (D27) —
there is no way to make a real `readdir` fail for root, and a store mocked
to return an error would prove nothing about the directory this code reads.
No skip was printed on this host: the fixture really failed.

`herd-hub/tests/owed_scan_fails.rs`, three tests, real hub over a real
socket:

1. `a_hub_that_cannot_list_its_jobs_answers_each_collection_it_is_offered` —
   silence at join (honest: nothing has been read), then the offer answered
   with the right attempt and empty holdings, once and not once per chunk,
   then the collection whole and acknowledged, then the link still answering
   a health report.
2. `one_failed_listing_silences_every_runner_and_each_is_still_answered` —
   two machines, two collections, one broken directory: both silent at join
   (the fleet-wide half, measured rather than argued) and each answered
   about its own collection on its own offer.
3. `a_collection_this_hub_already_holds_is_not_claimed_as_owed` — the
   control, and the other direction of "do not lie".

**Measured.** In a throwaway worktree at `1a8c1ae` (`--detach`, own
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch197-revert`) with this test
file copied in and **no product change**: 1 and 2 are RED, twice over, 20 s
of patience each, with

```
the hub never answered the offer of a collection it is owed, although its
listing failed and that offer named the one record it had to read
(ORCH-197); it said []
```

— `it said []` being the defect verbatim: the hub said nothing at all. Test
3 passed there, so the file is not trivially red. On this branch: 3 of 3
green, three runs of three, and green again inside the workspace suite run
under host load 17 (`0.58s`).

Four unit tests in `server.rs`'s own test module, same real fixture:
`a_failed_record_scan_claims_nothing_about_any_collection` (returns
`CannotTell`, sends nothing),
`an_offer_on_a_connection_whose_scan_failed_is_answered_from_that_job_s_record`
(one frame, right attempt, empty holdings, and nothing on the second
offer), `an_offer_is_not_answered_on_a_connection_whose_scan_ran` (the
ordinary path still asks at join and the fallback adds no frame),
`a_collection_another_machine_owes_is_not_asked_of_this_one`. These cannot
be red-before: they name a type the unfixed tree does not have. Their job is
branch coverage; the end-to-end file carries the red/green.

### What must not break, confirmed

Run on this branch, each green: `spools_resumed` (2 — the two facts the
sweep's design rests on), `spool_offer_answered` (0 passed, **1 ignored**:
ORCH-192's repro, still `#[ignore]`d on `main`, not this task's to enable),
`outputs_whole` (3), `recorded_offer` (3), `link` (19), `unknown_frames`
(3), `retention_preview` (11), `state_root_report` (7), and the hub lib's
226 unit tests. D218's startup quarantine and the pre-lock `report`,
`status` and `retention-preview` paths are untouched by this diff — nothing
outside `server.rs` is changed, and inside it nothing before the lock.

## Not done, and why

- **ORCH-022 / V3 is not fixed and nothing of it is on this branch.** It was
  offered mid-task, taken, implemented, measured, and withdrawn on the
  measurement. See *Risks and findings* 1. The patch is kept out of the tree
  at

  ```
  …/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/
  orch022-ask-before-dropping.patch
  ```

  (273 lines, applies to `server.rs` at `3d44312`), with its own
  deterministic unit test in it.
- **No frame added.** None was needed. The one case the protocol genuinely
  cannot express is in *Risks and findings* 2, with its shape, and it is
  pre-existing, in `store.rs`, and covered at startup by D218.
- **No write to `docs/decisions.md` or `docs/orchestration/board.md`** — the
  entry is drafted in the agent's report for the merger to place, and the
  board rows with it.
- **Nothing outside `herd-hub/src/server.rs` and `herd-hub/tests/`.** No
  `store.rs`, no `report.rs`, no `retention.rs`, no `herd-runner`, no
  `herd-proto`; `PROTOCOL_VERSION` unmoved.

## Gates

Run in this worktree on the code commit `3d44312`, before this file was
written, all with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch197`:

```
nix develop --command cargo fmt --all --check     → clean (after one `cargo fmt --all`)
nix develop --command cargo build --workspace     → Finished `dev` profile in 14.54s
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
                                                  → Finished, no warning
./scripts/check-docs.sh                           → docs ok
```

`nix develop --command ./scripts/test.sh` was run on the same tree and
**went red on host contention, not on this diff**: 16 failures across
`approval`, `away_fleet`, `durability`, `slots`, `priorities` and
`stopping`, every one of them a 30-second wait that ran out, each with the
harness's own line — `host load average 17.23 / 10.34 / 7.75 (1, 5, 15 min)
over 12 cores, read as this wait ran out`, and 18.44 by the next target.
`bench-preflight.sh` had cleared at load 3.90 with 14823 MiB of slice room;
four other agents were on the host. None of the failures is in the region
this diff touches, and this branch's own tests passed in that same run. The
suite was re-run for the gate below.

**The last run, named and not quoted:** bare `./scripts/gate.sh` on the tree
with this file committed, never inside `nix develop`. Its log goes to
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch197-gate.log` and its
receipt to `target/herd-gate/<sha>.receipt`; the verbatim verdict is in the
agent's report.

**Skips:** the at-rest cgroup-delegation list, every one of them a host
fact and none of them this task's, plus the two load-gated benches when the
host is busy. The suite run above printed 14: the twelve delegation
assertions (`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
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
`the_runner_says_which_memory_ceiling_it_enforces`) and `flood-fifty` and
`flood-two-fifty` on a one-minute load of 16.02 against their ceiling of 8.
None is counted as a pass (D27). The three tests of
`owed_scan_fails.rs` printed **no** skip: the `0o111` fixture made the real
`readdir` fail on this host.

## Risks and findings

1. **ORCH-022's recommended fix is refuted by measurement: "ask first" is
   not a fix, it is a hundred-millisecond delay.** I implemented it exactly
   as the brief recommends — `one_watchdog_pass` split into
   `judge_the_attempts_in_the_air` then
   `drop_the_links_nothing_comes_back_on`, both doc comments that assert the
   old order rewritten, and a deterministic unit test
   (`a_late_runner_is_asked_before_its_link_is_dropped`) that is RED on the
   old order with the defect's own sentence —

   ```
   the machine said it is running attempt 1 and the pass took the job away
   from it anyway: Queued — Some("runner a-machine-that-was-descheduled
   stopped answering: nothing about attempt 1 of this job for 90.000501086s,
   so it is not coming back")
   ```

   — and green on the new one. Then I ran V3's own acceptance regime, the
   kept probe at `V3_STARVE=2200`, five times on the fixed tree: **5 of 5
   red**, unchanged. The hub's own two lines say why:

   ```
   herd-hub: dropping the connection to runner wsl-pc: nothing has come back on it for 2.043258695s
   herd-hub: j0000000002 is lost: runner wsl-pc stopped answering: nothing about attempt 2 of this job for 2.143005362s, so it is not coming back
   ```

   100 ms apart — one watchdog tick (`watchdog_every` is `lost_after/20`).
   The pass's *own drop* is what deletes the word, so asking first only
   moves the clock's decision to the next pass. And the assertion V3 makes
   is not about the attempt at all: it reads
   `it is waiting because no runner is connected to this hub`, 17 ms after
   `SIGCONT` — **the fleet is empty**, because the link was dropped. No
   change that leaves the link drop at `lost_after` can make that assertion
   green. I did not ship a change that reads like a fix and is not one.
2. **One collection whose own record will not read is still answered with
   silence, and the protocol cannot express otherwise.** `collection_owed`
   can only return `Ok` — an unreadable record becomes `Ok(None)`, "nothing
   left to wait for". The honest answer needs a frame; `JobFilesHeld`'s own
   `attempt` field has to come from the record, so it cannot be filled, and
   `held: vec![]` would be exactly the forbidden lie. The shape, if it is
   ever wanted, is ORCH-194's: `JobFilesRefused { job_id, attempt, why }`
   with a sibling that carries no attempt, e.g.
   `JobFilesUnknown { job_id, why }`, which the runner would read as "keep
   the bytes, do not condemn, do not re-offer this connection". It is
   pre-existing, it lives in `store.rs` (not mine tonight), and D218's
   startup quarantine narrows it: a record that will not read is moved aside
   before a runner dials, and `Ok(None)` is then true.
3. **The fallback moves a cursor, once per collection, on a broken hub.** A
   `JobFilesHeld` answering an offer tells the runner what the hub holds
   *now*, so a collection already in flight resends what was in flight — at
   most one link window (`FILES_IN_FLIGHT`, 8 chunks), once per collection,
   and only on a hub whose jobs directory will not list. Named in the doc
   comment. The ordinary path sends not one extra frame.
4. **`bench-preflight.sh` cleared every heavy phase and was never lifted.**
   Readings at the four phases: load 0.68 / 5.79 / 7.44 / 3.90 against a
   ceiling of 9, slice room 16665 / 8435 / 11364 / 14823 MiB against the
   8192 MiB floor. Nothing was written under `/sys/fs/cgroup`, no cache was
   dropped, and no figure was exported into any gate's environment.
5. **The suite is red on this host above load ~15**, whatever the diff. Four
   other agents were running; the figure is the harness's own, printed in
   each timeout. Anyone reading a red `test.sh` from tonight should read the
   load line first.

## Decisions needed

**One, and it is ORCH-022's, now located a level deeper than the previous
diagnosis put it.** Not "should the watchdog ask before deleting" — it
should, and that change is written and tested — but: **may a runner silent
for `lost_after` keep its link?**

- **Recommendation: give the link its own bound, derived and not typed** —
  `lost_after * 2`, kept above `lost_after / 2` so that
  `register_runner`'s displacement of a quiet incumbent still comes first —
  **together with** the ask-first reorder, which is what makes the surviving
  word usable. Then a runner descheduled for 2.2 s against 2.0 s keeps its
  link and its attempt (`StillRunningIt`, the true sentence), V3's
  assertion can hold, and a machine that is really gone loses its attempts
  at `2 × lost_after` plus one tick.
- **The cost, which is Alexa's to weigh and the reason this is not mine:**
  E-21 promises "a runner nothing has been heard from for about 60 s is
  lost, and the jobs it was running are attempted once more". Doubling the
  link's bound doubles that to about two minutes for a machine that is
  genuinely off. A reading of E-21 in which a machine silent for
  `lost_after` *is* lost makes V3's 2200 ms regime a test of something the
  product deliberately does, and then V3's assertion is what should move.
- **Fallback, which I disfavour:** drop the proactive link check entirely
  and rely on `register_runner`. It frees the name on a live process's own
  word, but it leaves the hub listing — and **dispatching onto** — a
  machine that is gone until a replacement dials, which is worse than the
  defect.

## Resume point

For `herd-merger`: `orch-197-record-scan` at `3d44312` plus this file,
nothing pending, gate verdict in the agent's report. Draft decision entry
and board text are in the agent's report.

For whoever takes ORCH-022 next: the question above needs answering before
any code. Once it is answered, start from
```
…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/
orch022-ask-before-dropping.patch
```
(the reorder, the two rewritten doc comments and the deterministic unit
test, red-before/green-after measured) and add whatever the ruling says
about the link's bound. The acceptance regime is the kept probe,
`…/scratchpad/v3v4-probe-kept.rs`, at `V3_STARVE=2200` (5 of 5 red today
**and** with the reorder alone) and `1500` (green); build it as
`herd-hub/tests/v3probe.rs` and delete it afterwards.

## Processes

Started and ended by this task: `cargo`/`rustc` under
`nix develop` (builds, clippy, `test.sh`, the targeted test runs) and the
real `herd-hub`, `herd-runner` and `herdctl` processes the end-to-end tests
and the V3 probe spawn, each reaped by its own test harness; the five probe
runs' `SIGSTOP`/`SIGCONT` went only to runner processes the probe itself
started. Nothing was killed by name and no `pkill` was run. The stray
`herd-hub` PID 3046930 that earlier tasks noted was left alone.

Two build trees, both deleted once the branch was pushed:
`/mnt/ssd_980/herd/build/orch197` and
`/mnt/ssd_980/herd/build/orch197-revert` (the throwaway at `1a8c1ae`). The
throwaway worktree
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-orch197-revert` is
removed with it; this worktree stays until the batch lands.
