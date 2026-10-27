# orch-069-host-independent-assertions — handoff 2026-10-27

- **Item:** ORCH-069
- **Agent:** herd-step
- **Branch / commit:** `orch-069-host-independent-assertions`, two
  commits on origin/main `64cc532`: `ee6d2e3` the tests, then this
  file and the board (its own sha is the branch tip after the push)
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

**The cause, which this row had named wrongly.** Neither assertion was
failing on `MemTotal`. `herd-runner` lowers `--host-memory` three times
over: to `MemTotal` (D89, `metrics::lendable_memory_mib`), then to the
room under the tightest memory ceiling on the cgroup chain **enclosing
the runner** (D119, `room::announced_memory_mib`), and `--host-tmpfs` to
the size of the mount. On this workstation the binding clamp is the
third-party one: `chatons.slice` carries `memory.high` 18 GiB — *lower*
than its `memory.max` 22 GiB, and `room::enclosing` takes the lower of
the two — and `room_mib()` subtracts that slice's live `memory.current`,
9855 MiB when this was measured. 18432 − 9855 = 8576 MiB of room.

That one figure explains every number in this row:

- `slots.rs` asked for 17408 MiB and got 8576. Three quarters of 8576 is
  6432 MiB, which is 3 threads of 1792 — `-j3`, the figure reported.
  Earlier rounds saw `-j1`, `-j2`, `-j4` because `memory.current` moves
  with whatever else is in the slice.
- `admission.rs` asked for 8192 MiB. On the round that failed the room
  was a little **under** 8192, so the lend was clamped by about a
  hundred mebibytes and the admission read `6.9 GiB` instead of `7 GiB`.
  A clamp of 1.3 % is why that one looked like noise and the other like
  a bug, and why `memory.max` and `MemAvailable` both looked innocent:
  neither is the knob.

So the row's **original** reading — two assertions, both host-derived —
was right. The eighth reading's narrowing to `slots` alone was an
artefact: under load `admission` times out (ORCH-107) before it reaches
the figure, so the figure was simply never seen in those runs.

**And the half that mattered more.** `slots.rs:742` guarded on
`total_memory_mib() >= 17 * 1024`, which the cgroup clamp passes
straight through, so on this host the assertion ran and failed; on CI
(`MemTotal` 15983 MiB) the guard was false and it was **skipped**. By
D27 a skip is not a pass, so that assertion was proved nowhere at all.

**The fix: the host out of the expectation, not a wider guard.**

`herd-hub/tests/real_herd/mod.rs` gains
`a_lend_this_host_can_really_make(memory_mib, tmpfs_mib)`, which reads
`MemTotal`, `room::measure()` and the tmpfs mount and **asserts** —
never skips — that a declared lend survives this host untouched, naming
the cgroup, the knob and both figures when it would not. A skip here
would make CI's silence permanent, which is the thing to avoid.

`herd-hub/tests/slots.rs`,
`the_derived_j_is_recorded_and_is_what_the_job_is_given`: the runner is
told to lend **2 GiB** with 20 threads and a 64 MiB floor. Three
quarters of 2048 MiB is 1536 MiB on every machine that can build this
workspace, so the derivation's inputs are constants. Two jobs on the one
runner, because the derivation is `min` of two branches and one job
would leave the other deletable:

| per-thread figure | memory branch | thread branch | asserted |
|---|---|---|---|
| 128 MiB | 1536 / 128 = **12** | 20 | `built with -j12` |
| 64 MiB | 1536 / 64 = 24 | **20** | `built with -j20` |

Each expected figure is written out by the caller and never computed the
way the hub computes it. What the test asserts per job: the figure the
process printed, the `parallelism` on the record and the `-j N` in the
line explaining the placement are **one value**, and that value is the
constant above; plus `build_thread_mib` on the record, and
`2048 MiB and 20 threads` in the why line, so a clamped lend shows up as
a named memory figure rather than as an unexplained `-j`. The job whose
record is read is found by its own per-thread figure, not by taking the
newest directory, so two builds in one test cannot be read for each
other. **It no longer skips on any host**, and `use
herd_runner::testing::skipped` is gone from the file.

Why the floor is 64 MiB and not the default: a build declares what one
*thread* needs and the hub admits the derived `-j` **times** that figure
(`matcher.rs:1534` and its comment), so a derived build is weighed
against *lend − floor* while its share is *3/4 of lend*. At the default
floor those are the same number, and a `-j` one thread too large would
not be admitted at all — the test would hang on a job nothing ever
claims instead of failing on the figure. My first attempt sat exactly on
that tie and did hang under mutation; 64 MiB leaves 1984 MiB of
admission against a 1536 MiB share.

`herd-hub/tests/admission.rs`, `a_small_lender()`: 2 GiB lent, 512 MiB
floor, so **1.5 GiB of admission**; 4 GiB of tmpfs and 4 threads
unchanged. The precondition is called by all five tests in the file.
`a_killed_hub_reads_the_reservations_back_off_the_records` now holds
1 GiB across the `kill -9` and asks for another, and asserts
`512 MiB free of its 1.5 GiB memory admission, 1 GiB needed`.

`a_small_lender` also gained `--job-memory-max 512MiB`. The count of
jobs a runner holds at once is its admission over that per-job cap
(`confine::jobs_at_once`), so shrinking the lend alone took the machine
from three concurrent jobs to one and four of the five tests were
refused on the runner's own ceiling instead of on the arithmetic they
are about. 1536/512 is three, which is what 7 GiB at 2 GiB a job was.

**What each assertion still catches, proved red.** Each mutation was
made on product code in the worktree *after* the test commit, run, then
reverted with `git checkout --`; no mutation is in the branch, and both
tests were re-run green unmodified afterwards (`5 passed` and
`8 passed`). `cargo build --workspace --all-targets` before every run,
so no `real_herd` test built its binaries inside its own 30 s patience.

**Correction 2026-10-27, by `herd-merger` at batch 4's integration
(ORCH-107): the second clause is false.** `--all-targets` does not
remove the in-test build -- `real_herd::binaries` runs
`cargo build --offline -p herd-hub -p herd-runner -p herdctl`, a
narrower feature set, measured at **9.70 s** of compiling immediately
after `--all-targets` and **0.06 s** after the identical command. The
mutation results above are unaffected: they are figures from
`slots.rs` and `admission.rs`, not waits. `scripts/test.sh` now runs
the identical command itself.

1. `BUILD_SHARE_DENOMINATOR` 4 → 5 (the share fraction), caught by the
   **first** job:
   `left: "built with -j9"` / `right: "built with -j12"`, with the hub's
   own line attached: `-j 9 from 2048 MiB and 20 threads, 1228 MiB for
   this slot (3 of 5), 128 MiB per build thread`.
2. `from_memory.min(from_threads)` → `from_memory` (the thread cap
   dropped), caught by the **second** job and only by it — the first
   passed at `-j12`:
   `left: "built with -j24"` / `right: "built with -j20"`.
3. `admissible_memory_mib` subtracting half the floor, caught by
   `a_killed_hub…`: it gave `768 MiB free of its 1.7 GiB memory
   admission, 1 GiB needed` where the test wants
   `512 MiB free of its 1.5 GiB memory admission, 1 GiB needed`.
4. `out_there` no longer counting `JobState::Interrupted`, i.e. the
   inherited reservation not read back off the record — which is what
   attack 10 is *about*. Caught, but as a **timeout**, not a figure: with
   the reservation uncounted the second job fits, is placed, and the hub
   never writes the sentence the test waits for. Honest about it: that
   half of the test is a liveness assertion, not an equality one.

Mutations 1 and 2 together show why both jobs are there; neither test
can now pass with the derivation wrong on either branch, on any host.

## Not done, and why

- **`herd_proto::placement`'s own exact case for E-22's 17 GiB → `-j 7`
  is left where it is** and not reproduced end to end. It cannot be: a
  lend of 17 GiB is clamped on any host with less room, and there is no
  injection point for the announced figure that is not product code. The
  arithmetic is asserted exactly and host-free at
  `placement.rs:342-351`; what the real binaries uniquely prove — that
  the three reports of the figure are one value — is asserted here on
  figures no host can move.
- **No product code changed**, and none should have. The derivation and
  the clamp are both correct; the tests were wrong about the host.
- **ORCH-107 is untouched.** It is the other half of
  `workflow.md`'s local-gate sequence and still open.
- **`docs/decisions.md` untouched**, no `D-NEXT`: the brief asked for
  none and only `herd-merger` appends.

## Gates

`nix develop --command cargo build --workspace --all-targets`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.21s
```

`nix develop --command cargo fmt --all --check`

```
(no output; exit 0)
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.55s
(exit 0)
```

`nix develop --command ./scripts/test.sh`

```
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
test suite: PASSED
(exit 0; 836 passed, 0 failed across every binary)
```

`./scripts/check-docs.sh`

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh`

```
check-orchestration: 141 passed, 0 failed
```

Every gate was redirected to a file and the file read; nothing was piped
to `head` or `tail`.

**Skips — 12, and this is a count and not a floor** (D179): the run was
complete, every binary ran, `test.sh` printed the unqualified figure and
the suite passed. All twelve are cgroup v2 delegation or `io.max` on
this host; **not one of them is a test this branch touched**, and
`slots.rs` now contributes none where it used to contribute one on every
host under 17 GiB.

- a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
- a_declared_ceiling_changes_what_the_inert_weight_line_says
- a_job_cannot_fork_past_its_own_pid_ceiling
- a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
- a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
- a_job_s_record_carries_the_pressure_it_ran_under
- an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
- an_io_weight_that_cannot_bite_is_said_out_loud
- a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
- no_declared_ceiling_leaves_io_max_empty
- no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
- the_runner_says_which_memory_ceiling_it_enforces

**The suite is green on this workstation.** Nothing else was red in the
final run — no ORCH-107 timeout appeared in it, though the host was
quiet at the time and that is luck rather than a fix.

## Risks and findings

- **The brief's figure for this host is wrong, and it matters.** It says
  "This workstation has 15983 MiB". `MemTotal` here is **31882 MiB**;
  15983 MiB is the *GitHub runner's*, quoted from CI's own skip line. On
  31882 MiB the old guard was **true**, which is exactly why the
  assertion ran and failed here while being skipped there. Any future
  brief repeating 15983 as this host's figure will mis-diagnose again.
- **The test for the clamp that caused all of this is itself skipped
  here.** `a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`
  is in the skip list above: it needs cgroup delegation to build a
  bounded cgroup, which this host refuses. So D119's mechanism is proved
  on a tree a unit test builds (`room.rs`'s own tests) but not through
  the real binaries on this machine — while being the live cause of two
  red assertions on it. Worth a row of its own; it is not ORCH-069.
- **A derived build's admission is a tight coupling worth knowing about,
  and it is not documented outside a comment.** Because the admitted
  memory is `-j` × per-thread and the share is 3/4 of the lend, any
  fixture whose floor exceeds a quarter of its lend makes a derived
  build *inadmissible* — the job queues for ever and the test hangs
  rather than failing. That is a trap for the next person writing a
  `--build-thread-mib` test. Not a defect: both rules are right on their
  own. A finding, not fixed here.
- **`a_small_lender`'s tmpfs is still host-dependent in principle**: 4
  GiB is clamped to `/dev/shm`, which is 16 GiB here and is typically
  half of RAM, so a host with under 8 GiB would clamp it. The new
  precondition now says so by name instead of letting it surface as a
  wrong figure, but the figure itself was not lowered — no assertion in
  the file depends on a margin there, and 4 GiB is below any plausible
  mount.
- **Two board edits, not one.** The ORCH-069 table row is marked `done`
  with its resolution. I also rewrote item 6 of the "where to start"
  prose, because it asserted "the suite is red on this workstation" and
  attributed half of it to ORCH-069; leaving it would have left a false
  statement about my own row. ORCH-107's sentences in that item are
  byte-identical. If `herd-merger` would rather own that paragraph, it
  is one contiguous block and trivially reverted.
- **`workflow.md`'s local-gate refusal is not mine to change.** It names
  ORCH-069 and ORCH-107 as the two fixes that would make the refusal
  reconsiderable. One is now done; the sequence is not complete, so the
  text still reads correctly and I left it alone.

## Decisions needed

none.

## Resume point

`herd-merger` integrates this branch in the current batch. Then
**ORCH-107** is the remaining half of `workflow.md`'s local-gate
sequence, and its own row already names the first experiment: one
controlled pair of runs to settle how much of the timeout flakiness is
the missing `cargo build --workspace --all-targets`.

**Correction 2026-10-27 (batch 4): ORCH-107 is `done`, and the answer
was "almost none of it".** The prebuild named here is not the one the
tests run, so it never removed the in-test build; `scripts/test.sh` now
runs the tests' own command first. Four runs at peak loads 9.6 to 18.2
gave zero `:649`/`:789` timeouts, and the 21 failures at load 19 are
**unexplained**.

## Processes

PIDs started: `nix develop` ran `cargo` and `./scripts/test.sh` in the
foreground only; every hub, runner and `herdctl` process was started and
reaped by the test harness within its own run. Nothing was left behind,
nothing was backgrounded, and no process was killed by name — no
`pkill`, no `killall`.

Own `target/` deleted once the branch was pushed:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-069/wt/target
```
