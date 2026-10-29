# preflight-cgroup — handoff 2026-10-29

- **Item:** none allocated; the row is proposed below under *Decisions
  needed*. The preflight itself was ORCH-058's delivery
- **Agent:** herd-step
- **Branch / commit:** `preflight-cgroup` / `442bc40` plus this file, based
  on origin/main `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`scripts/bench-preflight.sh` has a fourth check, and it reads the cgroup it
is actually standing in rather than a slice named in the source.

- The reading: `own_cgroup` takes the `0::` line of `$SELF_CGROUP`
  (`/proc/self/cgroup`), `cgroup_enclosure` walks from the root of
  `$CGROUP_MOUNT` down to that cgroup, takes the lower of `memory.max` and
  `memory.high` at each level, keeps the tightest ceiling found anywhere up
  the chain, and subtracts that cgroup's `memory.current`.
- It follows `herd-runner/src/room.rs` step for step — same walk, same
  choice of knob, same reading of an unreadable `memory.current` (fully
  charged, so the figure errs towards less room). **Nothing under
  `herd-*/src/` was changed**; `room.rs` was read only.
- The floor is **8 GiB**, `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB`. Why 8: the
  real-binary tests need a runner that can lend 2 GiB, so 2 is where they
  cannot start at all; one agent held for 5 GiB on 2026-10-29 and still lost
  the suite, because its own cargo and rustc are charged to the same cgroup
  and grew about 4 GiB while it ran; 2 + ~4 is 6, and 8 leaves 2 GiB of
  slack for the sibling phase workflow.md allows to start alongside. It is a
  starting point from one day's measurements on one host, not a constant;
  fractions are accepted, and a value that is not a number is exit 2 rather
  than a guard that quietly clears every run.
- A refusal names all three figures. Driven over a tree a test builds:
  `bench-preflight: only 661 MiB of memory left under memory.high on
  /chatons.slice, floor is 8192 MiB: … Wait for the sibling phases charged
  to /chatons.slice to finish, or set HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB if
  this host has a different shape`.
- With room, it clears and prints what it read:
  `slice room:  17079 MiB under memory.high on /chatons.slice, floor 8192
  MiB (8 GiB)` — this host, 2026-10-29, hand-checked against
  `(19327352832 - 1402609664) / 1 GiB ≈ 16.7 GiB` before the gate.
- **With no cgroup v2 memory limit at all it clears and says the reading was
  unavailable**, and so does a host with no `0::` line and a host where
  `/proc/self/cgroup` cannot be read. It never refuses because it could not
  read.
- The three checks that were already there are untouched and each still
  refuses on its own; three cases assert that.
- A cleared run ends with a note that all four figures are of this instant
  only and nothing watches the host afterwards (the "also worth doing" of
  the brief, taken: two phases cleared at a quiet moment reached load 29
  mid-suite that day). A watchdog is not this task.
- `--help` no longer prints a literal line range of the header, which the
  new paragraphs would have truncated; it stops at `set -uo pipefail`, the
  way `gate.sh` already does, and a case asserts the last header line is
  printed.
- `scripts/check-orchestration.sh` gains **16 cases**, section
  *"bench-preflight.sh reads the room in the cgroup it is standing in"*. Four
  of them mirror `room.rs`'s own unit tests — tightest ceiling up the chain,
  `memory.high` under `memory.max`, unreadable `memory.current`, nothing
  enclosing at all — so the two readings cannot drift apart silently. They
  neutralise checks 1–3 by their own environment variables so that only the
  cgroup reading decides the verdict, and the three cases that assert those
  checks still work put their own figures back.
- `docs/orchestration/gotchas/build-weight-off-root.md` line 18's list of
  what the preflight checks now names the cgroup reading, and says that a
  reading it could not take is printed as unavailable and refuses nothing.

## Not done, and why

- **No watchdog.** The brief excluded it; the note on the way out is what
  replaces it.
- **No version number, no `gate.sh`, no `herd-*/src/`, no slice
  configuration.** Out of bounds by the brief; `/etc/nixos` and the
  consumer's production were not touched.
- **No board row and no `docs/decisions.md` entry.** The row is proposed
  below; the brief asked for neither to be written.
- **The floor is not validated against a measurement of its own.** 8 GiB is
  one agent's recommendation plus the brief's arithmetic; nobody has yet run
  a suite at exactly 8 GiB of room and watched whether it holds. The figure
  is overridable precisely because of that.

## Gates

`scripts/` is outside the prose paths, so this is a code change and owes the
full bare `./scripts/gate.sh`.

Before this commit, on the committed code (`442bc40`), the orchestration
checks that the change moves:

```
$ ./scripts/check-orchestration.sh    # baseline on cd0f1b7: 269 passed, 0 failed
== bench-preflight.sh reads the room in the cgroup it is standing in
ok    preflight: a slice with 661 MiB left refuses the run
ok    preflight: the refusal names the figure, the floor and the cgroup
ok    preflight: the same slice with 16 GiB left clears
ok    preflight: a cleared run says its figures are of this instant only
ok    preflight: the tightest ceiling up the chain is the one that binds
ok    preflight: a memory.high below the memory.max is the ceiling
ok    preflight: an unreadable memory.current is read as no room left
ok    preflight: no cgroup memory limit at all clears and says so
ok    preflight: a host with no cgroup v2 line clears and says so
ok    preflight: an unreadable /proc/self/cgroup clears, it never refuses
ok    preflight: the floor moves with HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB
ok    preflight: a floor that is not a number is a usage error, not a cleared run
ok    preflight: the load check still refuses on its own
ok    preflight: the MemAvailable check still refuses on its own
ok    preflight: the free-space check still refuses on its own
ok    preflight: --help prints the whole header, including its last line

check-orchestration: 285 passed, 0 failed
```

269 → **285 passed, 0 failed**: sixteen new cases, one per way the reading
can be wrong in a way nobody would see from a terminal — this host has a
slice, so a run here exercises exactly one of those shapes and never the
others.

The last run, named and not quoted here: **bare `./scripts/gate.sh`** on the
committed tree, this file included, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/preflight-cgroup`. Its log and its
receipt land in
`/mnt/ssd_980/herd/build/preflight-cgroup/herd-gate/`
(`HERD_GATE_RECEIPTS`), and the verdict line is in the agent's report.
The host's cgroup room was hand-checked first: **17 079 MiB under
`memory.high` on `/chatons.slice`**, load 0.55, so the suite had the room
this change exists to require.

Skips: whatever `scripts/expected-skips/local.txt` holds for this host —
twelve names, every one a cgroup v2 delegation refusal. The names, not a
count, are in the agent's report with the gate's verdict. No skip is counted
as a pass (D27).

## Risks and findings

- **The floor is a judgement, and 8 GiB of a slice's 18 is a lot to hold
  for.** On this host at rest the slice has ~17 GiB free, so the floor is
  invisible; with one sibling build it will start refusing, which is the
  point, but it will also refuse phases that would have passed. The
  environment variable is the escape hatch and the refusal names it.
- **The reading is a snapshot, which is ORCH-126's finding one layer up.**
  `room.rs` reads the room once at startup and never revises it; this script
  reads it once before a phase and never revises it either. Page cache
  counts in `memory.current`, so a slice that merely holds a warm cache
  reads as full. The note on the way out says the figures are of an instant;
  it does not make them any less of one.
- **Two readings of one figure now exist in two languages.** They are kept
  honest by four cases that mirror `room.rs`'s own tests, not by a shared
  implementation, which is the cheapest thing available to a script that
  must run with no toolchain. A change to `room.rs`'s walk will not break
  this script, and nothing tells its author to come here.
- **The orchestration cases run the real `scripts/bench-preflight.sh` out of
  `$root`**, not a scratch copy, because they vary only the environment. The
  script reads and writes nothing, so nothing in the repository is touched;
  but a future case that needed to edit the script would have to copy it the
  way the gate section does.
- Nothing in the brief was refuted by the code. `room.rs` reads exactly as
  the brief described it, and the hand reading of `/chatons.slice`
  reproduced the figures the brief quoted.

## Decisions needed

- **Proposed board row** (`herd-merger` allocates the number), *Open* →
  *Done* on landing: *"**The preflight cleared runs that could not pass: it
  read `/proc/loadavg` and `MemAvailable`, neither of which decides whether
  the suite can pass.** Five agents lost a twelve-core suite run each on
  2026-10-29 and three branches sat red; the suite runs inside
  `/chatons.slice` (`memory.high` 18 GiB) and the real-binary tests need a
  runner that can lend 2 GiB, so a slice with 661 MiB left is a red run on a
  true statement about the host. Measured that day: cleared at loads 8.33,
  2.27 and 4.15 and went red; slice room seen at 3, 326, 661, 772, 1281 and
  6591 MiB; 1.3 GiB of room at load 1.28, so the two readings are
  independent. **Fixed on `preflight-cgroup`:** a fourth check reads the
  enclosing cgroup the way `herd-runner/src/room.rs` reads it, refuses below
  8 GiB of room naming the figure, the floor and the cgroup, clears and says
  *unavailable* where nothing encloses the process, and leaves the three
  older checks alone. 16 cases, `check-orchestration` 269 → 285. Owner:
  herd-step (author), herd-merger (integration). Evidence:
  `history/2026-10-29-preflight-cgroup.md`; `scripts/bench-preflight.sh`;
  `gotchas/build-weight-off-root.md`; related ORCH-058, ORCH-126,
  ORCH-116."*
- **Worth a decision number?** The floor of 8 GiB and the rule *an
  unreadable cgroup clears and says so* are the two judgements here. My
  recommendation is **no new decision**: both follow D27's standing rule
  that an unmeasured thing is reported as unmeasured, and the floor is a
  threshold in a script, not a promise to the consumer. Fallback, if
  `herd-merger` disagrees: one `D-NEXT` entry naming the floor and its
  arithmetic, which is already written out in the script's own comment.

## Resume point

`git -C /mnt/ssd_980/herd-project fetch` and read `preflight-cgroup`; it is
ready for a batch. Whoever continues should, in order: (1) see the gate
verdict in this task's report, (2) write the proposed row into
`docs/orchestration/board.md` under the merger's allocated number, (3) decide
whether the 8 GiB floor holds after the first phase it refuses — the figure
to watch is the `slice room:` line of a run that then went red anyway.

## Processes

- PIDs started: none that outlived a command. Every run above was a
  foreground `bash`/`./scripts/*.sh` that exited on its own; no daemon, no
  `herd-hub`, no `herd-runner` was started. PID 3046930, the stray
  `herd-hub` on `127.0.0.1:18461`, was left alone and never signalled.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/preflight-cgroup`, off the root
  filesystem; deleted once the branch was pushed, except for the gate
  receipts under it, which are the evidence of this task's gate run.
