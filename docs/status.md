# Status — 2026-10-13

State of the project right now. Replaced every session, never appended to.

## Where we are

Four milestones are merged to `main`: M1 (end-to-end substrate), M2
(durable job store), M3 (link robustness) and M4 (runner confinement).
PR #9 (M5, the capability model) is open, not merged. PR #11 (an M3
follow-up: a terminal record is immutable, plus three refusals that
name their figures) is also open, not merged.

## What works

Verified against the real binaries by an independent adversarial pass,
not taken on report:

- **M1** — a job streams its output incrementally; exit codes reach
  `herdctl` unchanged; a SIGTERM becomes 143.
- **M2** — a hub killed mid-job loses no result: the job's record
  lands complete once the hub comes back (E-44).
- **M3** — a runner killed mid-job has its job attempted exactly
  twice, never a third time, across six alternating runner lives
  (E-21); cancellation kills the whole process tree, siblings
  untouched (E-06).
- **M4** — a job's output is byte-faithful through NUL bytes and
  invalid UTF-8 (E-15); every cap exits 124 and names its figure; a
  job's descendants die with it; a workspace is always reclaimed; the
  exit-code contract holds: the job's own code for a verdict, 124
  stopped by a cap, 125 when Herd has no verdict, 128+signal when
  signalled.

`cargo fmt`, `clippy -D warnings` and `cargo test --workspace` are
green through `nix develop`. CI (`check` + `docs`) and the
pull-request flow are live on `main`.

## In review, not merged

- **PR #9 — M5, the capability model.** One algebra, offers with
  origins, busy probes. Four defects found and fixed this session
  (D59-D62), but **not adversarially re-verified** — not recommended
  for merge yet.
- **PR #11 — M3 follow-up.** A terminal record is immutable (D54),
  plus three refusals that name their figures. Written, not merged.

## What is not done, and why

32 of 44 musts are still planned; M6 through M18 have not started.
Four caveats, detailed in `docs/coverage.md`:

1. No authentication before M15 — a runner's name is self-asserted,
   so Herd must not be exposed beyond loopback.
2. E-45's confinement is cooperative only (D28); the deployment host's
   runner user is additionally in the `docker` group, root-equivalent
   through the container socket.
3. E-30's real kernel ceiling needs a delegated cgroup; the WSL replay
   on real hardware is a manual M17 check.
4. Nothing is deployed, so `PROTOCOL_VERSION` and `RECORD_VERSION`
   stay at 1 by choice, freezing only at M16.

Booked, not yet fixed: a SIGKILLed runner leaves its in-flight probes
behind; a verdict the hub could not write, later overtaken by its own
watchdog, logs "is over" for ever instead of erroring — loud, never a
false write, but a stall worth a later look. **Q10, the licence**, is
the owner's only outstanding decision; it blocks nothing before M16.

**On pace.** Every milestone this session needed three passes — write,
fix, re-verify — because each failed its first adversarial
verification: a protocol that rewrote a job's output, a disk budget
bypassable in two shell commands, a log cap that reported a truncated
build as green, a job that outlived its own death, a verdict a runner
could forge onto another's job, a capability offered unprobed because
of one mistyped key. Plan on three passes, not one.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no
  swap, `/dev/kvm`, Intel iGPU at `/dev/dri/renderD128`, nix with
  flakes, docker 29.7. Rust 1.98.1 comes from the flake (D12); nothing
  is installed globally.
- GitHub: `tomasalexandre18` via `gh`. `main` protected (a pull
  request, `check` + `docs` green, no force push, no deletion). Issue
  #1 open, unanswered — replying is outward-facing, waits for Alexa.
- This machine's SSH key is a deploy key for another repo; git uses
  the HTTPS remote with the `gh` credential helper.

## Next action

Re-verify PR #9 adversarially and merge it, then start milestone M6
(queue and priorities).
