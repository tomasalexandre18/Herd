# Status — 2026-10-13

State of the project right now. Replaced every session, never appended to.

## What works

Eight milestones are merged to `main`: M1 through M8 (substrate,
durable store, link robustness, confinement, capability model, queue
and priorities, matching and slots, admission). Nothing else is
written and waiting. M1-M5 are additionally verified against the
real binaries by a separate independent adversarial pass
(`docs/coverage.md`); M6-M8 by their own milestone tests only.

- **M1-M5** — streamed output, exit codes unchanged, SIGTERM becomes
  143; a hub killed mid-job loses no result (E-44); a lost runner
  requeues once, cancel kills the whole tree (E-06, E-21);
  byte-faithful logs, every cap exits 124 named, workspace always
  reclaimed (E-15, E-30, E-34, E-45, E-46); one capability algebra,
  a busy capability suspends a claim (E-24, E-26, E-28).
- **M6-M7** — one queue, four priorities with aging from the record
  (D67); dedup and `coalesce` (E-05, E-18, E-20, E-43); slots read
  from the record each pass, none leak (E-22); `-j` derived, rounded
  down, never zero (D83); stuck to a preferred runner while busy
  (E-19); exclusion at claim time (E-25); every placement explained
  (E-36).
- **M8** — memory/tmpfs admission costed from the records, never
  measured usage (D87); short of tmpfs waits for its machine, too
  big for any connected runner waits "ever" rather than failing
  (D88); refusals and waits carry their figures (E-04, E-29, E-31,
  E-32). Fixed a leak predating M1: `Hub::serve`'s background tasks
  held the store's `flock` after the hub was gone, a one-in-six
  flake since M8's fourth task; a `Drop` guard now aborts them with
  the owning future, proven by a regression test (D93); not proven:
  how the leaked lock reached the particular root that complained.

`cargo fmt`, `clippy -D warnings` and `cargo test --workspace` are
green through `nix develop`. CI (`check` + `docs`) and the
pull-request flow are live on `main`.

## What is not done, and why

20 of 44 musts are still planned, from M9 (owner control) onward.
Caveats, detailed in `docs/coverage.md`:

1. No authentication before M15; the hub holds a **fleet** (M7), so
   a hand-crafted peer can join beside real runners under any name.
   Stay on loopback.
2. E-45's confinement is cooperative only (D28); the runner user is
   also in the `docker` group on the deployment host. Settled (D78):
   the runner needs no Docker at all; three checks land in M16.
3. E-30's real kernel ceiling needs a delegated cgroup; the WSL
   replay is a manual M17 check.
4. Both version numbers are free-running: M8 moved them to **4**; a
   pre-M8 root is refused with no migration, frozen at M16 (D73).
5. The whole-pass `dispatching` lock is measured, not fixed: 3.31 s
   over 400 queued jobs writing a waiting line each, 46.8 ms steady
   state; D92 names what is bad about it.
6. A job queued for two mutually-exclusive labels is **ended** when
   a runner joins declaring them so — defensible, but a new way to
   die. Separately, a job no *connected* runner could ever fit
   **waits, never ends** (D88): a fleet is not a closed world, and
   E-31 says "wait, never fail". Such a job can wait for ever; the
   bound is **E-09's deadline, in M9** — E-09 (a `should`) now
   carries a second load-bearing role.
7. Two smaller unsures from M8: the restatement figure is two lines
   a minute, not twenty per ten minutes, because the 45 s clock and
   M6's 60 s countdown interleave rather than coincide — recorded,
   not tuned; and `--host-tmpfs-path`'s non-default-mount path was
   never exercised against a real tmpfs.

Booked, not yet fixed: a SIGKILLed runner leaves in-flight probes
behind; a verdict the hub could not write logs "is over" for ever;
D63's write-door rule holds by review, not the compiler; D64's
probe sweep is untested at scale.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no
  swap, `/dev/kvm`, Intel iGPU, nix with flakes, docker 29.7. Rust
  1.98.1 from the flake (D12); nothing installed globally.
- GitHub: `tomasalexandre18` via `gh`; `main` protected (pull
  request, checks green, no force push/deletion); issue #16 closed.
- SSH key here is a deploy key for another repo; git uses the HTTPS
  remote with the `gh` credential helper.

## Next action

M9 (owner control), in progress on `m9-owner-control`. Alexa's own,
non-blocking: take the runner's user out of `docker` (D78), and
settle **Q10, the licence** — blocks nothing before M16.
