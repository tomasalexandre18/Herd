# Status — 2026-10-12

State of the project right now. Replaced every session, never appended to.

## Where we are

**M1 is merged to `main` and verified by hand with the real binaries.**
Two further milestones are written and in review, neither merged yet:
**PR #4** (M2, durable job store) and **PR #5** (M4, runner confinement).

## What works

M1, verified by hand with the real binaries, not taken on report: a
job streams its output incrementally; exit codes 0, 7, 75 and 125 reach
`herdctl` unchanged; a SIGTERM becomes 143; a missing executable gives
125 with the reason on stderr and nothing on stdout; neither a process
nor a bound port is left behind. `cargo fmt`, `clippy -D warnings` and
`cargo test --workspace` are green through `nix develop`. CI (`check` +
`docs`) and the pull-request flow are live on `main`.

## In review, not merged

- **PR #4 — M2, durable job store.** Hub state as files, one directory
  per job, atomic rename, queue rebuilt at startup. M2's own test suite
  was green, but real-binary verification on 2026-10-12 found the job lost
  entirely across a hub restart: the record stuck in `running` forever,
  `herdctl wait` hanging (**E-44 failing**). D17 adds the minimum reconnect
  — the runner survives a dropped control connection and redelivers its
  held result — and that fix is in flight now. Not mergeable until it passes.
- **PR #5 — M4, runner confinement.** 75 tests green, CI green. Two
  limits named on purpose, not oversights: **D28** — E-45's confinement
  is cooperative, not a mount namespace, because a namespace needs privilege
  E-46 forbids the runner to hold; **D23/D27** — the kernel memory ceiling
  (E-30) is asserted only under a delegated cgroup, and where delegation
  is unavailable the assertion prints `HERD-SKIPPED` rather than passing
  silently. Three real bugs only the kernel revealed were found and fixed
  while building this milestone, including job cgroup names colliding across
  runners, so one job's kill reached another runner's processes.

## What is not done, and why

Nothing of the 44 musts is verified on `main`: M1 carries no must of its
own, and M2 and M4 are written but not merged. **Q10, the licence**, is
still the owner's only outstanding decision; it blocks nothing before M16.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no swap,
  `/dev/kvm`, Intel iGPU at `/dev/dri/renderD128`, nix with flakes, docker
  29.7. Rust 1.98.1 comes from the flake (D12); nothing is installed globally.
- GitHub: `tomasalexandre18` via `gh`. `main` protected (a pull request,
  `check` + `docs` green, no force push, no deletion). Issue #1 open and
  unanswered — replying is outward-facing and waits for Alexa.
- This machine's SSH key is a deploy key for another repository; git uses
  the HTTPS remote with the `gh` credential helper.

## Next action

Merge M2 once its fix passes real-binary verification, then rebase and merge
M4, then M3 on the spine (D17's minimum reconnect gets its backoff policy,
health report and requeue-once).
