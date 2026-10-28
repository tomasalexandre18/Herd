# orch-111-stdin-independence — handoff 2026-10-28

- **Item:** ORCH-111
- **Agent:** herd-step
- **Branch / commit:** `orch-111-stdin-independence` / `f056c25`, based on
  `origin/no-sigpipe-assertions` at `6649bd1`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`scripts/check-orchestration.sh` and `scripts/gate.sh` each carry one
`exec </dev/null` directly under their `set -uo pipefail`, with the
mechanism written above it. Neither script reads fd 0 anywhere, so the
redirection costs nothing and covers every child they start, present and
future.

The mechanism, re-measured on this host before any edit. A socket on fd 0
makes bash source `SYS_BASHRC` (`/etc/bashrc`) through its `isnetconn()`
check although the shell is not interactive. The 27 captures in
`check-orchestration.sh` run `env -i PATH=... HOME=/wrong/home bash -c
...`, and `env -i` has stripped `__ETC_PROFILE_DONE`, so `/etc/bashrc`
finds that variable empty, sources `/etc/profile` and
`/etc/set-environment`, and rebuilds `PATH` out of the child's own
`HOME=/wrong/home` — discarding the `PATH` the case supplied. The
assertions are then right and the diff is innocent:

```
./scripts/check-orchestration.sh </dev/null   -> 253 passed, 0 failed
the same script, stdin = AF_UNIX socketpair   -> 232 passed, 21 failed
```

Same tree, same minute, `6649bd1`. Only a socket arms it; `/dev/null`, a
pipe, a regular file and a closed fd 0 are all inert, which is why the row
survived — the caller chose and the script had no say. The socketpair is
in-memory: no listening socket, no port, no network, so invariant 3 is
not in play.

One refinement to the mechanism, found while verifying the fix and now in
the comment: bash looks at fd 0 **only on the `-c` path**
(`command_execution_string` in its `run_startup_files()`), so a bash
running a script *file* never arms, whatever is on its stdin. The script
itself was therefore never the shell at risk; its `bash -c` children were,
inheriting fd 0 from it. That is why the redirection has to sit in the
parent, and it is also why `gate.sh`, which runs no `bash -c` of its own,
is defence in depth rather than a second defect — said so in its comment.
Isolated, with the socket on fd 0 both times:

```
script file, no `exec </dev/null`:
  self_fd0=socket:[74839290]
  bashrc=1 PATH=/wrong/home/.local/bin:...:/etc/profiles/per-user//bin:...
the same script with `exec </dev/null`:
  self_fd0=/dev/null
  bashrc=unset PATH=<exactly the PATH the capture supplied>
```

The lever was re-checked live, in the same backgrounded shape the gate
runs were started in, so that a green run could not be green because the
lever had quietly stopped arming: `bash -c` under it reports
`fd0=socket:[74828981] bashrc=1`.

Redirecting stdin was taken over `__ETC_PROFILE_DONE=1` and over
`NOSYSBASHRC=1`: the first suppresses the symptom while the child still
executes `/etc/bashrc`, the second is a NixOS-specific knob, and only the
redirection removes the cause for every present and future capture site.
It is also the convention the product already follows — a runner gives
every job `Stdio::null()` (`herd-runner/src/job.rs`).

Three new cases in `check-orchestration.sh` (253 → 256) freeze the
invariant with no toolchain at all:

- the redirection is present in `check-orchestration.sh` **above** its
  first `env -i` capture — a guard below the first capture guards nothing;
- the same for `gate.sh`, above its first `run_step`;
- `scripts/githooks/pre-push` does **not** have it. That is the one script
  here that must keep the stdin it is handed: git feeds it the ref list on
  fd 0, and `while read -r ...` over `/dev/null` iterates zero times, so a
  guarded pre-push would fall straight through to its `exit 0` and approve
  every push to `main`, refusing nothing and saying nothing.

Each of the three has been seen red, in a throwaway copy outside the
worktree: guard deleted from `check-orchestration.sh` → 255/1; guard
deleted from `gate.sh` → 255/1; guard moved below the first capture →
255/1, with the two line numbers in the diagnostic; `exec </dev/null`
added to pre-push → 233/23, the 23rd being the new case and the other 22
being real pushes the hook stopped refusing.

Why `gate.sh` too, when step 2 now guards itself: it guards every other
child the gate starts, and `pre-push` calls `gate.sh --verify-receipt`
from inside that `while read` loop, where a child that consumed any of the
ref list would silently drop a ref from the hook's checks.

## Not done, and why

- **No behavioural case for the redirection.** Arming it needs a process
  that can put a socket on fd 0. `python3` is absent on this host, `perl`
  is at `/run/current-system/sw/bin/perl` (system, not the dev shell) and
  `socat` is dev-shell only, while `check-orchestration.sh` runs with no
  toolchain. A case skipped on every host where this script actually runs
  — the CI runner's `/etc/bashrc` has a different shape, so even with perl
  present the negative control would not arm and the case would skip there
  too — is not a control. It would also be the first skip in a script
  whose contract is `N passed, 0 failed`, which CI reads. The three
  textual cases give the red-on-removal guard everywhere instead, and the
  measurement is in this handoff. Reported as a judgement, not a pass.
- **`scripts/check-docs.sh` left alone.** No `env -i`, no `bash -c`
  capture, and nothing in it reads fd 0. Measured inert under the lever:
  identical output and exit 0 with stdin on `/dev/null` and on a
  socketpair. It is also a child of `check-orchestration.sh`, which now
  hands it `/dev/null` 32 times a run.
- **The brief's third scope item could not be done: the comment it names
  does not exist.** See *Risks and findings*.
- Nothing written to `main`, `docs/decisions.md`,
  `docs/orchestration/board.md`, `.github/` or branch protection. Board
  and decision text proposed in the agent's report.

## Gates

`./scripts/check-docs.sh` on the fix, before this file was written:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/gate.sh` on `f056c25` (the fix commit, before this file), with
fd 0 on `/dev/null`:

```
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (18s)
step 3: nix flake check (ci job: check) — ok (2s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (18s)
step 6: test suite — ok (236s)
step 7: the skips — ok
gate: GREEN sha=f056c25ab2ebd1d38581600e9924418235feaad9 steps=0-8:ok host=servernixos load1=1.04 skips=at-rest
```

Both stdin kinds, five runs each, interleaved so neither got the quieter
window, on the fix:

```
run 1  devnull  exit=0  check-orchestration: 256 passed, 0 failed
run 1  socket   exit=0  check-orchestration: 256 passed, 0 failed
run 2  devnull  exit=0  check-orchestration: 256 passed, 0 failed
run 2  socket   exit=0  check-orchestration: 256 passed, 0 failed
run 3  devnull  exit=0  check-orchestration: 256 passed, 0 failed
run 3  socket   exit=0  check-orchestration: 256 passed, 0 failed
run 4  devnull  exit=0  check-orchestration: 256 passed, 0 failed
run 4  socket   exit=0  check-orchestration: 256 passed, 0 failed
run 5  devnull  exit=0  check-orchestration: 256 passed, 0 failed
run 5  socket   exit=0  check-orchestration: 256 passed, 0 failed
```

An intermediate gate also ran GREEN on `99d638f`, under `/dev/null` and
again under the socket (`steps=0-8:ok`, 12 skips, at-rest list both
times).

The last runs, on the tree that carries this file, are not quoted here —
a gate run made before this file existed tested a different tree
(`templates/handoff.md`). They are `./scripts/gate.sh` on the branch tip
with fd 0 on `/dev/null`, and the same gate once more with fd 0 on an
`AF_UNIX` socketpair — the direct test of whether a landing can still be
derailed by its caller's fd 0. Their logs go to the agent's scratchpad,
their receipts to `target/herd-gate/`, and their verbatim verdicts to the
agent's report.

**Skips: 12**, every one of them a cgroup v2 delegation skip, and the
observed list matched `scripts/expected-skips/local.txt` exactly (step 7:
"the skip list matches the expectation exactly", at-rest list, load1 1.04
at the start and 2.88 after the suite). Each was printed as skipped and
none was counted as a pass (D27, invariant 5). The names:
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
`the_runner_says_which_memory_ceiling_it_enforces`.
`check-orchestration.sh` itself printed no skip line.

## Risks and findings

1. **The brief's third scope item rests on a comment that is not there.**
   It asked to add the payload size to a comment at
   `scripts/check-orchestration.sh:683` quoting "2 false failures in 4000
   with the pipe". No such text exists in `scripts/` on this base or on
   `origin/main`; the only occurrences of the figure in the repository are
   in two handoffs, and both already name the size —
   `2026-10-28-harden-the-gate.md:267` says "an env-shaped input of 7.9
   kB" and `2026-10-28-no-sigpipe-assertions.md:89` says "their 7883-byte
   payload". The `no-sigpipe-assertions` branch had already replaced the
   script comment with a size-indexed table (lines 34–50 of this base):
   "Payloads of 945, 1950, 3960 and 4161 bytes false-failed 0/200; 4965
   and 7980 bytes 1/200; 64 kB 14/200; 1 MB 200/200." That is the asked-for
   correction, already landed and finer-grained. Nothing was edited: a past
   handoff is an append-only record and the script already does better than
   asked.
2. **A guarded pre-push is a silent catastrophe, and nothing but the new
   case stands between the repository and it.** `exec </dev/null` in
   `pre-push` makes its ref-list loop iterate zero times, so the hook
   exits 0 having checked nothing — it approves a deletion of `main`, a
   non-fast-forward, a commit made straight on `main`, a push with no
   receipt. The mutation measured 23 failures in
   `check-orchestration.sh`, so the suite does catch it today; the new
   case is what names the reason. Anyone "tidying" these three scripts for
   consistency should read the comment above pre-push's loop first.
3. **The fix is portable but the measurement is not.** `/etc/bashrc`
   sourcing `/etc/profile` when `__ETC_PROFILE_DONE` is empty is NixOS's
   shape. On another distribution the socket on fd 0 still makes bash
   source the system bashrc, but what that file then does to `PATH` is
   that distribution's business. The fix does not depend on any of it —
   it removes the socket — and that is why it was chosen over
   `NOSYSBASHRC=1`.
4. **`bash` version.** The `isnetconn()` path is a long-standing bash
   behaviour, not a local patch, but it was verified only against the
   bash in this host's `/run/current-system`. A bash that dropped it would
   make the mechanism unreproducible and the three new cases would still
   pass, since they are textual.
5. **Inert, and checked:** `scripts/check-docs.sh`, `scripts/test.sh`,
   `scripts/report-skips.sh`, `scripts/bench-preflight.sh`,
   `scripts/image-smoke.sh`, `scripts/release-smoke.sh`,
   `.claude/hooks/orchestrator-guard.sh`, `.claude/hooks/release-guard.sh`
   and `scripts/githooks/pre-push`. None has an `env -i`. The only fd-0
   readers are `pre-push` and the two hooks, which take their input there
   by design and must not be guarded; `bench-preflight.sh`'s and
   `report-skips.sh`'s `read` builtins are redirected to `/proc/loadavg`,
   a pipe and a here-string, never fd 0.

## Decisions needed

none. Two proposals for `herd-merger` are in the agent's report: a board
row closing ORCH-111, and a `D-NEXT` entry stating that a gate script
owns its own fd 0 — this handoff does not write either file.

## Resume point

Nothing is pending on this branch. For whoever continues:
`git fetch && git log --oneline origin/orch-111-stdin-independence -1`
should show the handoff commit, and
`./scripts/check-orchestration.sh` plus the same script with a socket on
fd 0 should both print `256 passed, 0 failed`.

## Processes

- `./scripts/gate.sh` runs, each started and reaped by this agent and each
  exiting 0 on its own: PID 1833328 on `f056c25`, PID 1925655 on
  `99d638f` under `/dev/null`, PID 1992001 on `99d638f` under the socket.
  Two more follow on the branch tip once this file is committed; every PID
  is in the agent's report. Short-lived `perl` probes were waited on by
  PID in the same way. Nothing was killed, and nothing was matched by
  name.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-111`, off the root
  filesystem and outside the worktree (8.6 GB at its peak), deleted once
  the branch was pushed. The worktree's own `target/` held only the gate
  receipts (132 KB) and goes with the worktree.
