# Status — 2026-10-13

State of the project right now. Replaced every session, never appended to.

## Where we are

Seven milestones are merged to `main`: M1 (end-to-end substrate), M2
(durable job store), M3 (link robustness), M4 (runner confinement), M5
(the capability model), M6 (queue and priorities) and M7 (matching,
slots, no fallback). Nothing else is written and waiting.

## What works

Verified against the real binaries by an independent adversarial pass,
not taken on report:

- **M1** — streamed output; exit codes reach `herdctl` unchanged; a
  SIGTERM becomes 143.
- **M2** — a hub killed mid-job loses no result; the record lands
  complete once it comes back (E-44).
- **M3** — a lost runner requeues a job exactly once, never a third
  time (E-21); cancel kills the whole process tree (E-06).
- **M4** — byte-faithful logs; every cap exits 124, named; descendants
  die with the job; workspace always reclaimed (E-15, E-30, E-34,
  E-45, E-46).
- **M5** — one capability algebra; probed, declared or denied origins;
  an externally busy capability suspends a claim rather than handing
  it out (E-24, E-26, E-28).
- **M6** — one queue for everything, four priorities with aging
  computed from the record (D67); dedup joins, `coalesce` drops a
  queued job (E-05, E-18, E-20, E-43).
- **M7** — slots read from the record each pass, none leak (E-22);
  `-j` derived from memory and threads, three quarters kept back,
  rounded down, never zero (D83); a preferred runner is stuck to
  while merely busy (E-19); exclusion enforced at claim time (E-25);
  every placement, and every impossible ending, explained (E-36).

`cargo fmt`, `clippy -D warnings` and `cargo test --workspace` are
green through `nix develop`. CI (`check` + `docs`) and the
pull-request flow are live on `main`.

## What is not done, and why

24 of 44 musts are still planned, from M8 (admission and explained
waits) onward. Caveats, detailed in `docs/coverage.md`:

1. No authentication before M15, and the hub now holds a **fleet**
   (M7): a hand-crafted peer can join beside the real runners under
   any name. Herd must stay on loopback.
2. E-45's confinement is cooperative only (D28); the deployment host's
   runner user is also in the `docker` group, root-equivalent through
   the socket — removing it is Alexa's NixOS change, landing at M16.
3. E-30's real kernel ceiling needs a delegated cgroup; the WSL replay
   is a manual M17 check.
4. Nothing is deployed, so both version numbers are free-running: M7
   moved them to **3**; a pre-M7 root is refused with no migration,
   frozen at M16 (D73).
5. The `dispatching` lock is now held for a **whole pass** over the
   whole queue, including every claim's disk writes; a pass's duration
   against a large queue is unmeasured.
6. A job queued for two mutually-exclusive labels is **ended** the
   moment a runner joins declaring them so — a new way to die, albeit
   a defensible one.
7. M7's three-quarters reserve and slot divisor reproduce E-22's
   worked figure exactly, but that is **judgement, not measurement**;
   M8 is where they meet reality.

Booked, not yet fixed: a SIGKILLed runner leaves in-flight probes
behind; a verdict the hub could not write, overtaken by its watchdog,
logs "is over" for ever instead of erroring; D63's write-door rule
holds by review, not the compiler; D64's probe sweep is untested and
unmeasured at scale. **Q10, the licence** blocks nothing before M16.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no
  swap, `/dev/kvm`, Intel iGPU, nix with flakes, docker 29.7. Rust
  1.98.1 comes from the flake (D12); nothing installed globally.
- GitHub: `tomasalexandre18` via `gh`. `main` protected (pull request,
  `check` + `docs` green, no force push, no deletion). Issue #1 holds
  the consumer's expectations; issue #16 (six deliverables) is closed
  through D74-D86, ending in their agreement on D51's probe rule.
- This machine's SSH key is a deploy key for another repo; git uses
  the HTTPS remote with the `gh` credential helper.

## Next action

M8 (admission and explained waits) is next, built against the
requirements, slots and preferences M7 already put on the record.
Alexa's own action: take the runner's user out of the `docker` group (D78).
