# cgroup-first-enable-race — handoff 2026-10-30

- **Item:** ORCH-NEXT (proposed row below; no existing row covered this)
- **Agent:** herd-step
- **Branch / commit:** `cgroup-first-enable-race` / `516cd45` plus this
  file, based on origin/main `b99d32d`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One file changed: `herd-runner/src/cgroup.rs`, +948 / -31.

**The defect, reproduced before it was fixed, and not the one the brief
described.** Writing `+<controller>` to a base's
`cgroup.subtree_control` is refused with `EBUSY` while *anything* sits
directly in that cgroup. The runner read that refusal as "I may not
write `cgroup.subtree_control`" and fell back to `RLIMIT_AS`: a
per-process address-space limit where a job-tree ceiling was promised,
which is invariant 2 degrading without a word.

**The measurement, one binary and one variable at a time.** The
`confinement` test binary, alone in a child of a delegated
`systemd-run --user --scope -p Delegate=yes -p MemoryMax=12G`, that
child holding nothing else and its own `cgroup.subtree_control` empty —
the shape a runner started as a systemd unit with `Delegate=yes` meets.
Same binary, same scope recipe, same build; the only thing varied is
named in each row.

| Code | `--test-threads` | runs | `EBUSY` skips |
|---|---|---|---|
| `b99d32d` | 12 | 3 | 2, 2, 3 |
| `b99d32d` | 1 | 1 | 0 |
| this branch | 12 | 3 | 0, 0, 0 |
| this branch | 1 | 1 | 0 |

The two names at 12 threads on `b99d32d` are the brief's own pair,
`a_job_cannot_fork_past_its_own_pid_ceiling` and
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`, with
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given` as the third
in one run. All of them `Device or resource busy (os error 16)` writing
`cgroup.subtree_control`. Wall time moved with the finding: 5.1 s when
those assertions skipped, 20.2 s when they ran.

**The mechanism the brief gave is refuted, and the new message is what
refuted it.** The brief said two concurrent `prepare` calls race between
reading `memory_is_enabled_for_children` and writing `+memory`. They
cannot: every `prepare` calls `enter_leaf` before it writes, and that
write migrates the whole thread group, so **this process is never
directly in the base at the moment of a `subtree_control` write**. What
was in the base was a **real `herd-runner` binary a test had spawned** —
`cgroup.procs` listed `3175023 (herd-runner)`, a descendant of the test
binary — forked out of the base before any `prepare` had left it. At one
thread it does not happen because the first `prepare` comes before the
first fork. That is the production shape the finder named: **two runners
sharing one delegated base.**

**The fix: the refusal is read, not guessed.** On `EBUSY` — and only on
`EBUSY`, by `raw_os_error()`, because `EACCES` is a permission and
`ENOENT` is a missing controller and neither is about who is standing
there — `base/cgroup.procs` is read and each pid is asked whose it is
through `/proc/<pid>/stat`, walking the parent chain:

- `Whose::ThisRunner` and `Whose::ItsChild` are this runner's to move.
  They go into `herd-runner.leaf` — `0` for itself, which needs write
  access over nobody else's processes, and a pid for one it started,
  which it may move because it started it and the base is delegated to
  it. Then one more write. **Once, never in a loop.**
- `Whose::Foreign` is a refusal naming the pid and what the process
  table calls it, with what to grant. A retry there would spin against a
  condition that will not change — which is the trap the brief named.
  Read **before** anything is moved, so a base this runner is not going
  to get is a base it leaves exactly as it found it.
- `Whose::Gone` — listed in `cgroup.procs`, absent from `/proc` — is in
  nobody's way and is not reported as a stranger.
- An unreadable `cgroup.procs` is **not** an empty one. It is a refusal
  saying this runner cannot tell whose those processes are, only that
  the kernel says there are some: the project's rule about an absence.

**The phantom in the message is gone.** It said *"or something other
than this runner still has processes directly in that cgroup"*, and in
the one case ever measured the something-other was the runner's own
child. Each arm now names its own case or says it cannot tell which.

**Seven assertions against a fake tree, because a skip is not a proof.**
This host gives our account no delegated cgroup outside a scope, so a
real-kernel test would skip here (D27). `herd-runner/src/room.rs`'s
precedent is a tree a test builds, and the one thing a temporary
directory will not do is *refuse* a write — so the two writes go through
a `Kernel` struct whose fixture writer enforces cgroup v2's own two
rules: a controller cannot be handed to the children of a cgroup that
holds processes, and writing a pid to a `cgroup.procs` moves that
process there. That makes the race deterministic rather than hoped for.
The new assertions:

| Assertion | What it holds |
|---|---|
| `an_ebusy_caused_by_this_runners_own_un_migrated_self_is_moved_and_retried` | moved, retried, exactly two writes |
| `an_ebusy_caused_by_a_process_this_runner_started_moves_it_rather_than_blaming_a_stranger` | the measured case; a grandchild, so the ppid walk answers |
| `an_ebusy_caused_by_a_foreign_process_is_refused_naming_it_and_never_retried` | one write, nothing moved, a child left where it was |
| `an_ebusy_with_nothing_left_in_the_base_is_retried_without_a_move` | a stale refusal; retried, and nothing moved |
| `an_ebusy_this_runner_cannot_explain_says_that_rather_than_guessing` | the absence said as one, and not retried |
| `a_process_of_this_runners_that_will_not_move_is_a_refusal_saying_so` | names the pid and where it would not go |
| `who_is_in_a_cgroup_is_read_out_of_the_process_table` | the four `Whose` readings, and a command with a space and a `)` in it |

**Four reverted mutants, each red in exactly the tests covering its
clause, the file restored byte-identically and checked with
`sha256sum -c` between every attempt** (`OK` four times, sha
`209e3892…`): no retry → 3 red; no move before the retry → 3 red; no
stranger check → 1 red; an unreadable `cgroup.procs` read as empty → 2
red.

**What was considered and not done: a mutex.** The brief offered
`one_reaper_at_a_time()` across the enable as one of two shapes. It was
written, and then removed, because the measurement refuted the window it
would have guarded. One window a lock would have shut survives — the
fallback moves this process **back into the base**, and another
`prepare` writing `cgroup.subtree_control` in that instant meets an
`EBUSY` about somebody else's host — and the reading above already
answers it, because the occupant is then this process itself. Redundant
as well as unmeasured, and no run has ever shown it. The reasoning is on
the module.

## Not done, and why

- **The cross-process race is not cured, only made loud.** Two
  *unrelated* runner processes sharing one delegated base — neither a
  descendant of the other — still refuse each other, now with a refusal
  naming the pid instead of a silent `RLIMIT_AS`. No lock inside one
  process reaches it. The cure is a base cgroup per runner, which is a
  decision and not this task's: it is the drafted `D-NEXT` below.
- **No test asserts the real kernel path**, because this host cannot
  give one without a scope. The scope runs are a **named local control**
  (see *Gates*), and the permanent proof is the fake tree.
- **`board.md` and `docs/decisions.md` are untouched.** Both are drafted
  in full below; `herd-merger` allocates the number and owns the row,
  and a merger is landing batch 21 beside this branch.

## Gates

**The four Rust gates ran through Herd**, as Alexa's directive requires,
not locally: `herdctl run --label build --memory-mib 6000`, job
**`j0000000038`**, on `queue-local`, exit **0**, 316.6 s wall,
290 846 317 bytes written, peak memory 3 137 536 B. The four in order,
`set -e`, ending in a sentinel:

```
ran on: queue-local (claimed by queue-local: labels build; build slot 1 of 2; admitted 5.8 GiB of memory, 6.1 GiB free of 12 GiB after it (16 GiB lent, 4 GiB kept as its own floor); it reserves no tmpfs; load 0.57 there, recorded and not a gate (E-32); preference 0, the only eligible runner; herd output format 1)
...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.63s
RUST-GATES-ALL-FOUR-GREEN
```

From that job's own log: `test suite: PASSED`, `ignored census:
COMPLETE (38 ignored, 0 without a reason)`, **12** `HERD-SKIPPED` lines
— the standing twelve, every one of them *"cgroup v2 delegation is
unavailable … Permission denied (os error 13)"* — and **zero**
occurrences of `os error 16` anywhere in 2234 lines.

The twelve, by name:

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job (the named memory outcome)
an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device branch)
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces (the cgroup branch of the report)
```

**Named local controls, and why they could not go through Herd.** The
race reproduction needs a delegated cgroup scope, which a Herd job
cannot give: our runner has no cgroup at all, which is the very reason
those twelve skip. Routing it through Herd would also have added a
variable to a measurement whose whole value is one variable at a time.
So the eight scope runs in the table above, the unit-test runs and the
four mutant runs were local. The four Rust gates were **also** run
locally earlier in the task, before the directive reached me, and all
four were green then too; the authoritative run is `j0000000038`.

**The last run, named and not quoted here:** `./scripts/check-docs.sh`
on the tree with this file committed, through Herd
(`herdctl run --label build --memory-mib 1000 --verdict-to
<scratchpad>/runs/verdict-check-docs.json`). Its verbatim output is in
this agent's report, not in the tree it has just gated.

**Skips:** the twelve named above, and nothing else. **No `HERD-SKIPPED`
line of this task's own**, and no `HERD-SKIPPED` was counted as a pass.

## Risks and findings

1. **A controller's availability is inferred from a file's existence,
   one layer down, and it is pre-existing.**
   `Ceiling::prove_io_max` chooses between two refusals with
   `if control.exists()` on `<probe>/io.max`: present means "the kernel
   refused this value", absent means "no blk-throttle, **or** the `io`
   controller is not handed down". That is exactly the reading the
   coordinator warned about — `app.slice` holds `io.pressure` while
   `io` is absent from its `cgroup.controllers`, so a file proves
   nothing about a controller. The honest reading is
   `<probe>/cgroup.controllers`, or the base's `cgroup.subtree_control`.
   **Not changed here**: it is a different defect in a different
   function and this task is bounded. Worth a row.
2. **`enter_leaf` still moves the process before anything has checked
   who else is in the base.** It is harmless — the enable now reads the
   base afterwards — but the order means a runner relocates itself
   before it knows whether it will get the subtree. The fallback undoes
   it.
3. **Moving a descendant is a new right the runner exercises.** The
   argument it rests on: a job's process is never directly in the base,
   because a job is written into its own cgroup between fork and exec
   (`JobCgroup::procs_file`, `confine.rs`'s `cgroup_procs_fd`), so the
   only descendants this can move are helpers or a second runner. If
   that ever stops holding, a job could be moved under the runner's own
   leaf and lose its ceiling. The clause is on the code, and this is the
   thing to re-read if the fork path changes.
4. **The brief's "two concurrent `prepare` calls" is in no artefact of
   mine**, and should not survive in anyone's notes: it is refuted
   above. The coordinator's own retraction of the confounded "10 of 12"
   is honoured — no count of mine is load-bearing; the evidence is a
   reason (`os error 16`) and a pair of names inside one binary.
5. **One `herd-runner.leaf` exists on this host and it is the
   consumer's**, under
   `/sys/fs/cgroup/system.slice/herd-runner.service/`.
   Untouched, before and after. Every scope and cgroup this task made is
   gone; `systemctl --user list-units --type=scope --all` lists the same
   four units as at the start, and a `find` over `/sys/fs/cgroup` shows
   no `run-p*.scope`, no `suite` and no `herd-*probe*`.

## Drafted for `herd-merger`

The full `docs/decisions.md` entry, to be appended with the number the
merger allocates (the last on `main` is D257):

```markdown
## 2026-10-30 — D-NEXT An EBUSY is read, never guessed

Enabling a controller for a cgroup's children is refused with `EBUSY`
while anything at all sits directly in that cgroup. Until today
`herd-runner` read that refusal as "I may not write
`cgroup.subtree_control`" and fell back to `RLIMIT_AS` — a per-process
address-space limit where a job-tree ceiling was promised — and the
sentence it printed blamed "something other than this runner".

Measured in a delegated scope, one binary and one variable: at twelve
test threads the confinement suite skipped two assertions on `EBUSY (os
error 16)` in two runs of three and three in the third; at one thread
none. The something-other was the runner's **own child**: a real
`herd-runner` binary a test had spawned, forked out of the base before
any `prepare` left it, `cgroup.procs` listing `3175023 (herd-runner)`.
The first explanation — two concurrent `prepare` calls racing between
the read and the write — is refuted: every `prepare` moves out before it
writes, so this process is never directly in the base at a write.

**The rule.** An `EBUSY` from `cgroup.subtree_control` is read, not
guessed. `cgroup.procs` says who is standing in the base and `/proc`
says whose each of them is. This process and the processes it started
are this runner's to move, and go into its leaf; a process that is
nobody's of this runner's is a refusal naming its pid, because a retry
there would spin against a condition that will not change; a
`cgroup.procs` that will not read is a refusal this runner cannot
explain, said as that rather than guessed either way. Then one retry,
never a loop. Only `EBUSY` takes this path: `EACCES` is a permission and
`ENOENT` is a missing controller, and neither is about who is standing
there.

**No lock was added, and that is part of the decision.** A mutex across
the enable was written and removed: it would have guarded the window the
measurement refuted, and the one window it would really have shut — the
fallback moving this process back into the base under another
`prepare` — is already answered by the reading, because the occupant is
then this process itself.

**What is not cured: two unrelated runners sharing one delegated base.**
Neither being a descendant of the other, they refuse each other, and no
lock inside one process reaches it. It is loud now rather than silent —
a refusal naming the pid instead of a quiet `RLIMIT_AS` — and the cure
is one base cgroup per runner, which `--cgroup-root` makes easy to get
wrong. That is a separate row.

**The general shape, and it is D27's.** A refusal whose cause has three
possible readings must be measured down to one, or must say that it
could not be. A message that names only one of the three sends a reader
after a third party that may not exist.

Reversible: the reading is one function and one struct in
`herd-runner/src/cgroup.rs`, held by seven assertions against a fake
tree and four reverted mutants.
```

The `board.md` row, for **Open — seen, not agreed or sized**, with the
next free `ORCH` id (260 is the highest on `main`):

```markdown
| ORCH-NEXT | **Two unrelated runners sharing one delegated base refuse each other's `cgroup.subtree_control`, and nothing stops an operator pointing two `--cgroup-root`s at one cgroup.** Measured 2026-10-30 in a delegated scope: a second `herd-runner` standing directly in the base makes the first one's enable `EBUSY`, and before `D-NEXT` that was a silent fall back to `RLIMIT_AS`. It is loud now — a refusal naming the pid — but not cured, and `JobCgroup::create`'s own doc already contemplates two runners on one base. Separately, `Ceiling::prove_io_max` infers whether the `io` controller is handed down from `<probe>/io.max` **existing**, and a file proves nothing about a controller: `app.slice` on this host holds `io.pressure` while `io` is absent from its `cgroup.controllers` | `cgroup-first-enable-race` handoff, 2026-10-30, findings 1 and the *Not done* section | herd-planner | open | Decide whether the runner **refuses at startup** when its base already holds a foreign process, rather than falling back; and replace `prove_io_max`'s `control.exists()` reading with `<probe>/cgroup.controllers`. Both are small; the first is a product question because it turns a working runner into one that will not start | nothing | `herd-runner/src/cgroup.rs` (`enable_for_children_through`, `Occupancy`, `Ceiling::prove_io_max`); `docs/orchestration/history/2026-10-30-cgroup-first-enable-race.md`; **D-NEXT** |
```

## Decisions needed

**Should a runner whose base already holds a process it does not own
refuse to start, instead of falling back to `RLIMIT_AS`?** —
*Recommendation:* refuse, by the same reasoning `--cgroup-root` is
already refused on (`confine.rs`): an operator who pointed a runner at a
subtree asked for that subtree, and a quiet per-process limit answers a
precise request with something weaker. — *Fallback:* leave it as it is
now, which is loud and names the pid, and let the operator read it. This
is Alexa's, not an agent's: it turns a machine that runs jobs today into
one that will not start.

## Resume point

Nothing is owed on this branch. The next command for whoever continues
is `herd-merger`'s: include `cgroup-first-enable-race` at `516cd45` plus
this file in the next batch's integration, allocate the `D-NEXT` number
and add the board row above. To re-measure the race, the recipe is in
this handoff's table: build
`cargo test -p herd-runner --test confinement --no-run`, then run that
binary under `systemd-run --user --scope -p Delegate=yes -p
MemoryMax=12G` through a three-line script that puts it alone in a child
of the scope, hands that child the scope's controllers and `exec`s it —
pre-warming `herd-runner.leaf` hides the defect, and leaving a wrapper
in the scope makes everything skip.

## Processes

PIDs started and ended: three `cargo`/`nix develop` builds, eight
`systemd-run --user --scope` test runs and two `herdctl` followers, all
foreground children of this agent's own shell, all exited; one
backgrounded `herdctl` follower was superseded and its job
(`j0000000037`) cancelled by id with `herdctl cancel`. Nothing was
signalled by name, nothing with `pkill`, and nothing of the consumer's
was touched: their hub `3006543`, their runner `3021002` and our
instance's `565152` / `607643` were never addressed.

Own build tree: `/mnt/ssd_980/herd/build/cgroup-race`, `dev`
(debug) profile, measured with `du -sh` before deletion — the figure is
in this agent's report — and deleted once the branch was pushed. The
worktree
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/cgroup-race` carries
no `target/` of its own: every build went to `CARGO_TARGET_DIR` off the
root filesystem, as `build-weight-off-root` requires.
