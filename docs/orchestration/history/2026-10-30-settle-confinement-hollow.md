# settle-confinement-hollow — handoff 2026-10-30

- **Item:** the hollow-green member of the restoration class that
  `settle-owed-scan-red` drafted as `ORCH-NEXT+1`; unnumbered here,
  because the merger allocates and D267 plus ORCH-286 to ORCH-290 are
  taken
- **Agent:** herd-step
- **Branch / commit:** `settle-confinement-hollow` / `b3a0592` for the
  fix, plus this file's own commit, based on
  **`origin/settle-owed-scan-red`**
  `771ef9a117675a17a3d8da6d2ccd42bf9e30415a` — **not** on `origin/main`:
  the base branch carries the `still_binds` / `hold_again` / `ATTEMPTS`
  pattern this reuses and has not landed yet, so this branch is the
  second half of that one and must land after it or with it
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One file changed: `herd-runner/tests/confinement.rs`, 305 insertions and
11 deletions. No product file is touched. No assertion is widened,
weakened or deleted; the three the test ended on are byte-identical.

### What was hollow

`a_job_cannot_hide_what_it_wrote_from_its_disk_budget` ran
`mkdir h; dd …100 MB…; chmod 300 h; sleep 6`. It wrote its bytes **in
the open** and hid them afterwards, for six seconds, inside a suite that
itself runs inside a Herd job whose runner walks the whole job base every
200 ms and hands itself back read access on any directory it cannot list
(`herd-runner/src/root.rs`'s `read_dir_restoring` → `restore_access`,
`set_permissions(mode | 0o700)`). So the cap tripped on bytes nothing had
ever hidden, and `StoppedByCap { Disk }` held either way.

**That is now measured and not argued.** With `restore_access` robbed of
its effect — `mode | 0o700` written as `mode`, in the worktree, never
committed — the test **as it was** is **green 3 runs out of 3** inside a
Herd job (`j0000000160`). A test that passes with the behaviour it is
named for removed was reporting nothing.

### What the fix is

Three changes, each one closing a different way for the green to be a
lie:

1. **The bytes go into the hiding place.** `mkdir h; chmod 300 h; dd …
   of=h/big` — a directory moded `0o300` can still be written through
   (`w` and `x`, no `r`), so the hundred megabytes exist only behind a
   directory nobody can list. There is now **no instant** at which that
   figure is reachable without somebody restoring access, which is what
   makes the assertion about the restoration rather than about the
   bytes.
2. **The job states its own premise.** It checks `[ -r h ]` with a shell
   builtin, prints `this job cannot read its own hiding place`, and
   exits 9 **writing nothing** if it can still read it. A run whose
   premise was already gone costs no disk and is void.
3. **The enclosure, which is the witness.** The harness's data root is
   put inside a directory the test owns (`TheOnlyWayIn`), so the only
   path from the host to `h` runs through it. `read_dir_restoring`
   restores **the ancestors first** — `restore_ancestors`, every
   directory up to the walking runner's own data root — and only then
   the directory it failed on. The enclosing runner's data root is above
   `$TMPDIR`, so that directory is one of those ancestors: **nothing
   outside can give `h` its access back without giving the enclosure its
   access back in the same call**. The runner under test never touches
   it, because `restore_ancestors` stops at its own root, which is
   *inside* the enclosure.

So the enclosure's mode, read after the job has ended, is a three-way
verdict and not a boolean: `0o300` the premise held, `0o700` something
outside opened it, `0o755` the job never blinded it at all. A fourth
state — moded `0o300` and listing anyway — is a process file permissions
do not apply to. A measurement taken on anything but the first is **void
and taken again from nothing**, eight times (`ATTEMPTS`), and the last
void is **red** with a named sentence (`WOULD_NOT_HOLD`) rather than
skipped, because a skip would move the compared skip list (D27, D187).

`0o755` is set by the fixture itself and never produced by
`mode | 0o700`, so the umask has no say in which of the three a reader
sees.

### Proved both ways, inside a Herd job

| the test | product as it is | `restore_access` with no effect |
| --- | --- | --- |
| as it was (`chmod 300 h; sleep 6`) | green | **green** 3/3, `j0000000160` |
| as it is now | green 3/3, `j0000000158` | **red** 3/3, `j0000000159` |

**Which assertion failed for which clause.** The **first** one, the
outcome, on all three negative runs and in the whole-file negative run:

```
assertion `left == right` failed: a job that blinds the runner must
still be held to its budget:
  left: SetupFailed { reason: "this runner was refused access to the
    job's own workspace, so it could not hold the job to its disk budget
    of 8388608 bytes at all: …/root/jobs/j0000000001/work/h cannot be
    read even after its mode was restored: Permission denied (os error
    13). The job was ended and has no verdict; whoever runs this machine
    has to make that path readable to the user the runner runs as" }
 right: StoppedByCap { cap: Disk, limit: 8388608, detail: None }
```

That is D166's refusal path where the disk cap is asserted, which is
exactly what a runner that cannot see through a `chmod` produces. The
second and third clauses — `bytes_written` present, and nothing left in
the data root — are never reached, and both would also be red: the same
`restore_access` is what `reclaim_directory` uses, and the negative runs
printed `could not be removed even after its modes were restored`.

In the whole confinement binary, the negative control took **3 tests**
red and 39 green: this one, `a_job_cannot_lock_the_runner_out_of_its_own
_data_root` and `a_workspace_a_job_locked_shut_is_still_reclaimed`.

### Alone and in the full suite

- alone, 3/3 green (`j0000000158`);
- inside the full `./scripts/test.sh`, green (`j0000000161`), the line
  `test a_job_cannot_hide_what_it_wrote_from_its_disk_budget ... ok`;
- inside the whole confinement binary at its own parallelism, **10 runs
  out of 10 green, 42 passed each run** (`j0000000162`).

### The witness is proven to fire

Not a detector trusted for its silence. Across those 10 whole-file runs
the ten judgements cost **12 attempts**: two attempts were void, both
reading `SomethingOutsideOpenedIt` — the enclosing runner's walk coming
past and being caught at it. No run needed more than **2** of its 8
attempts. One negative-alone run (`j0000000159`, run 3) voided two
attempts in a row before judging the third.

So the per-attempt void rate on this host is **2 in 12 attempts ≈ 17%**,
and eight attempts is margin of about `0.17 ** 8`, near 7e-7, not a
figure this suite will meet. The figure is of this host on 2026-10-30
under a load of 1.7 to 3.7 on 12 cores; a slower enclosing walk raises
it and the bound is what bounds it.

### What did not move

- no product change: `herd-runner/src`, `herd-hub/src`, `herdctl/src`
  are untouched, and the negative control's mutation was reverted inside
  its own job (`git status --porcelain` printed 0 lines after it);
- no `#[ignore]` added or removed: the census is **COMPLETE at 38**;
- no skip added or removed: the same **twelve** `HERD-SKIPPED` lines,
  the cgroup-delegation family, by name;
- the test keeps its name, so nothing moves in either compared list;
- `Harness` gained `rooted_at` and two fields so that a test may hand in
  its data root; the 41 other tests in the file still call
  `Harness::new` unchanged.

## Not done, and why

- **The four sub-millisecond members of the class are untouched**, as the
  brief required: `herd-runner/src/root.rs`'s
  `a_directory_a_job_made_unreadable_is_still_measured` and
  `a_workspace_a_job_locked_shut_is_still_reclaimed`,
  `herd-runner/src/outputs.rs`'s
  `a_directory_the_job_closed_is_still_collected`, and
  `herdctl/tests/exit_code.rs:756`. One test, proved. They are a small
  hole in the right direction rather than a hollow green, and the row
  below proposes what to do with them.
- **No edit to `docs/decisions.md` or `docs/orchestration/board.md`**:
  both are drafted in full below for `herd-merger`.
- **No merge, no tag, no pull request, no push to `main`.**

## Gates

Four Rust gates plus `check-docs.sh`, all through Herd on the committed
sha `b3a05929c1324e01b71cd73c83d0ca3ca39f5958`, job `j0000000161`,
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 --label
build --memory-mib 4700`, never bare, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/settle-confinement-hollow` on
the command line. `./scripts/bench-preflight.sh` was run before every
heavy phase and printed `ok, safe to start` each time;
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set or exported.

```
sha=b3a05929c1324e01b71cd73c83d0ca3ca39f5958 dirty=0
=== GATE 1/4: cargo fmt --all --check
fmt exit=0
=== GATE 2/4: cargo clippy --workspace --all-targets -- -D warnings
clippy exit=0
=== GATE 3/4: cargo build --workspace
build exit=0
=== GATE 4/4: ./scripts/test.sh (the suite plus its skips)
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 38 assertion(s) DID NOT RUN AT ALL: they carry #[ignore].
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
suite exit=0
```

`./scripts/check-docs.sh` printed `docs ok` on the tree before this file
was committed.

**The last run**, on the tree this file is committed into:
`./scripts/check-docs.sh`, run locally with no toolchain, its output in
the agent's report. The Rust gates were **not** re-run for this commit,
which adds prose only; what covers them is the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s.

**Skips: the standing twelve, unchanged, by list.**
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
`the_runner_says_which_memory_ceiling_it_enforces`. Every one is the
cgroup-delegation family or a device this host does not have. **No skip
was added, and the test fixed here was never one of them**, which was
checked before anything was written: it carries no `#[ignore]` and
appears in none of `scripts/expected-skips/`.

## Risks and findings

1. **The witness rests on one line of the *enclosing* runner's code,
   which is a deployed binary and not this branch.** `read_dir_restoring`
   calls `restore_ancestors` **before** `restore_access`. Verified in
   `git show v0.4.1:herd-runner/src/root.rs`, which is what
   `/mnt/ssd_980/herd/queue/bin/v0.4.1/herd-runner` is built from, and
   on `main`. If a later runner ever restored only the directory it
   failed on, this witness would weaken from deterministic to a race of
   roughly the depth of one descent — the enclosure would still catch
   every walk that *starts* the descent while it is blinded, but not one
   already below it. The doc comment says so where the mechanism is
   written down, so the next reader is not guessing.
2. **A product hole this test deliberately walks around, reported and
   not fixed** (no product change was mine to make). `read_dir_restoring`
   covers a directory that cannot be **listed**. Nothing covers a
   directory that lists but cannot be **searched**: `measure_tree`'s
   `entry.metadata()` returns `Err` straight out, with no restoration at
   all. So `chmod 400 d` on a directory holding files — `r`, no `x` —
   makes the walk fail where `chmod 000 d` and `chmod 300 d` are both
   restored and measured. The job gains no free pass; it ends. But it
   ends as `SetupFailed`, which tells whoever runs the machine to go fix
   a path, about a mode the **job** set. Invariant 2's "a job must not
   chmod its way out of a figure the runner enforces" is held for
   listing and not for searching. **This is a code reading of
   `root.rs:512-523` and no run was made**, on purpose: a nested test
   doing it would make the *enclosing* Herd job unmeasurable and kill the
   gate run it was part of. That hazard is worth knowing on its own.
   A row is drafted below.
3. **Up to eight job runs per test run, each up to 100 MB.** Measured:
   the ten whole-file runs needed twelve. Every workspace is reclaimed by
   the runner under test, and a void attempt writes nothing at all. The
   enclosing job's disk budget is 64 GiB.
4. **The negative control leaves a tree behind, by construction.** With
   `restore_access` ineffective, `reclaim_directory` cannot remove the
   workspace either, so each negative run left one `jobs/<id>` under the
   enclosing job's `tmp`. The enclosing runner reclaims its own base when
   its job ends, so nothing outlived `j0000000159`.
5. **This branch is based on `settle-owed-scan-red`, not on `main`.** It
   reuses that branch's pattern and its handoff's class row. If the
   merger lands them in separate batches, this one has to follow.

## Drafted for `herd-merger`

The full text to paste. The number is the merger's to allocate (D148,
invariant 8).

**One `docs/decisions.md` entry.** It answers the question
`settle-owed-scan-red`'s class row asked — *how a test proves the
runner's own restoration still happens when something else would have
done it anyway* — which is a method and so was decided here rather than
raised.

```markdown
## 2026-10-30 — D-NEXT A premise inside a Herd job needs an enclosure

A test that asserts the runner restores access it was denied cannot be
written as a window of time inside a Herd job. The runner carrying the
suite walks the whole job base every 200 ms and restores anything it
cannot list, ancestors first, so any premise built on a mode is undone
on a schedule nothing in the test controls. A six-second premise against
a 200 ms restorer is not a long premise; it is no premise at all, and
the assertion it carries holds whether or not the product does anything
(`confinement.rs::a_job_cannot_hide_what_it_wrote_from_its_disk_budget`,
green 3 runs out of 3 with `restore_access` robbed of its effect, job
j0000000160).

So the method is an **enclosure**, and three rules with it:

1. the thing being hidden is created hidden, never hidden after the
   fact: a figure that was reachable for an instant is a figure the test
   no longer owns;
2. the data root sits inside a directory the test keeps, which every
   outside restorer must open first, because `read_dir_restoring`
   restores the ancestors before the directory it failed on and the
   runner under test stops at its own root. That directory's mode, read
   after the fact, says which of three things happened: the premise
   held, something outside opened it, or the premise was never set;
3. a measurement taken on a premise that did not hold is void and taken
   again from nothing, a bounded number of times, and the last void is
   red with a named sentence. Never skipped: a skip moves a compared
   list (D27, D187).

This is the general form of what D225's `AListingThatFails` did for a
few frames of a hub, carried to a premise that has to hold across a
job's own execution. Measured void rate on this host, 2026-10-30: 2
attempts in 12 across ten whole-file runs, worst run 2 of its 8
attempts.
```

**Two `board.md` rows.** The first **supersedes** the class row
`settle-owed-scan-red`'s handoff drafted as `ORCH-NEXT+1`: if that row
has already been given a number, replace its text with this one's; if it
has not, this is the row to file instead of it.

```markdown
| ORCH-NEXT | **Four tests in the restoration class still set a mode and read it back in the same breath, and the hollow one of the five is fixed.** `confinement.rs::a_job_cannot_hide_what_it_wrote_from_its_disk_budget` was green with `restore_access` robbed of its effect (3 runs out of 3, job j0000000160) and is now red with it and green without it (j0000000159 and j0000000158, 3 out of 3 each), by the enclosure method of D-NEXT. The other four have sub-millisecond windows rather than six seconds, so they are a small hole in the right direction and not a lie: `herd-runner/src/root.rs::a_directory_a_job_made_unreadable_is_still_measured`, `herd-runner/src/root.rs::a_workspace_a_job_locked_shut_is_still_reclaimed`, `herd-runner/src/outputs.rs::a_directory_the_job_closed_is_still_collected`, and `herdctl/tests/exit_code.rs:756`. Each is one unit test away from the same enclosure, and measuring the window before rewriting it is the cheap half. | `settle-confinement-hollow`, 2026-10-30 | herd-step | open | Put the four on the enclosure of D-NEXT, or measure each window and record the figure that says it needs none | ORCH-NEXT | `history/2026-10-30-settle-confinement-hollow.md` |
| ORCH-NEXT+1 | **A job can still chmod its way out of being measured, by taking search and leaving read.** `read_dir_restoring` restores a directory that cannot be listed; nothing restores one that lists and cannot be searched, because `measure_tree`'s `entry.metadata()` returns `Err` with no restoration at all (`herd-runner/src/root.rs:512-523`). So `chmod 400 d` on a directory holding files defeats the walk where `chmod 000 d` and `chmod 300 d` are both measured. The job still ends, so it buys no free pass, but it ends `SetupFailed` under D166 — the sentence that tells whoever runs the machine to fix a path, about a mode the job set. Invariant 2 is held for listing and not for searching. A code reading, no run: a nested test doing it would make its own enclosing Herd job unmeasurable and kill the gate run. | `settle-confinement-hollow`, 2026-10-30 | — | open | Decide whether the walk restores search as well as read, or whether a job that takes search is a job whose own verdict says so rather than the operator's | ORCH-NEXT+1 | `history/2026-10-30-settle-confinement-hollow.md` |
```

## Decisions needed

None. The method question the class row carried is a *how*, so it was
decided inside the mandate and is written down as the D-NEXT entry
above. The second drafted row carries a product question — whether the
walk should restore search as well as read — and that one is a
behaviour question for whoever owns the row, not for this task.

## Resume point

`git fetch origin settle-confinement-hollow` and read
`herd-runner/tests/confinement.rs`'s `TheOnlyWayIn`. Then either:

- land it, after or with `settle-owed-scan-red`, which it is based on;
  or
- take the next member of the class, which is the cheapest one:

```sh
nix develop --command cargo test -p herd-runner --lib root:: -- --nocapture
```

for the two in `root.rs`, measuring each window before rewriting it.

## Processes

- **Five Herd jobs**, each with its own marker on the command line so
  none was coalesced into another's output: `j0000000158` (the build and
  3 positive runs alone), `j0000000159` (the negative control: 3 runs
  alone and the whole file), `j0000000160` (the old test against the
  same mutation, 3 runs), `j0000000161` (the four gates and the full
  suite), `j0000000162` (ten whole-file runs and the void rate). All
  through `/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791
  --label build --memory-mib 4700`, never bare.
- **One `bash` of my own, PID 3464297, killed by its PID** after a stray
  `cat` in my own command line blocked on stdin. Its `args` were read
  first and were mine. It had changed nothing.
- Nothing else was signalled. PID 662711 (the queue's runner), 565152
  (our hub), 3006543 and 3021002 (the consumer's) were read and never
  touched. No `pkill`, no `killall`, nothing killed by name.
- Build tree: `/mnt/ssd_980/herd/build/settle-confinement-hollow`,
  **debug profile with tests**, `du -sh` **17G** at the end of the gates
  and the figure at deletion is in the agent's report — that one is the
  final one. It read 13G after the first build, and quoting that as the
  final size is the mistake this sentence exists to avoid. Deleted once
  the branch was pushed. **Every log was written outside it first**, in
  the agent's scratchpad under `hollow-evidence/`.
