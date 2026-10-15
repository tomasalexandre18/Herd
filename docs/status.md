# Status — 2026-10-15

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`,
the order of work in `docs/plan.md`.

## What works

Nine milestones are merged to `main`: M1 through M9. Nothing else is
written and waiting. M1-M5 are additionally verified against the real
binaries by a separate adversarial pass; M6-M9 by their own milestone
tests only — the cost of dropping that pass after M5, and now measured
twice: a two-core host under load found eight tests in M9 asserting
what they never established (D100), then the same class in six of the
other ten files (D101), and behind one of those a product defect that
would have killed every runner of a real fleet (D104, fixed by D105).

What a requester can do today, end to end, against the real binaries:
submit and follow or detach; get the job's own exit code untouched, 124 for
a cap, 125 when Herd has no verdict, 128+n for a signal; read a complete
byte-faithful record while the job runs, and after a hub killed mid-job;
cancel it and have the whole tree die. One durable queue orders everything
by four priorities aging off the record, joins a second submission of open
work, and never gives a job twice. Placement matches one capability algebra,
counts build slots, derives `-j`, admits against memory and tmpfs costed
from the records, sticks to a preferred runner while it is merely busy,
and explains every decision in one line with its figures. A lost runner
gets one more go and no more. An owner switches her machine off with a
reason and is obeyed in under a second, the position persisted before the
command returns; a job may carry a deadline and one named parent; a `metal-*`
lease is read as a busy probe, so a leased trial waits instead of failing.

Nothing is deployed: no runner but this host's has ever connected, and
no job of the first consumer's has ever run.

`fmt`, `clippy -D warnings` and `./scripts/test.sh` are green: **413
passed**, 2 skipped here, a third on CI (E-22 needs a 17 GiB host). The
suite is also run pinned to two cores under five spinners, which is
where every lying test so far was caught.

## What is not done, and why

17 of 44 musts are planned, from M10 on; the consumer cannot submit a
real job until M10 through M15 land.

1. **No authentication before M15.** The hub holds a fleet (M7), so a
   hand-crafted peer can join beside real runners under any name and
   write a false verdict. **Stay on loopback.**
2. E-45's confinement is cooperative only (D28). The runner needs no
   Docker at all (D78); three checks land in M16.
3. E-30's real kernel ceiling needs a delegated cgroup; the WSL
   replay is a manual M17 check.
4. Both numbers are free-running: `PROTOCOL_VERSION` **6** (D99 added
   the switch acknowledgement), `RECORD_VERSION` **5**. An older root
   or peer is refused by name, no migration; both go back to 1 and
   freeze at M16 (D73).
5. The whole-pass `dispatching` lock is measured, not fixed (D92), and
   M9's dry run costs one more such pass, unmeasured.
6. `off` does not stop serving content: no endpoint exists before M11,
   and the gate waits behind `Switch::serves_content` (D97).
7. A job no *connected* runner could ever fit **waits**, not ends
   (D88): a fleet is not a closed world, so its only bound is E-09's
   deadline — which is why that `should` is load-bearing. Two new ways
   a queued job can die, both explained: mutually exclusive labels a
   joining runner declares, and a deadline missed because its machine
   was off.

Booked debt that no decision records, so it lives here: a SIGKILLed
runner leaves its in-flight probes behind; a verdict the hub could not
write, overtaken by its own watchdog, logs "is over" for ever instead
of erroring; `a_job_assigned_as_the_switch_moves_…` proves D80
placement, not the hand-back its name claims; and nobody traced how
M8's leaked `flock` reached the one state root that complained. Debt
that **is** recorded and still open: D63, D64, D92, D98, D101.

## Environment facts

- Server: NixOS 6.18, i5-10600K, 12 threads, 31 GB RAM, no swap,
  `/dev/kvm`, Intel iGPU, docker 29.7. Rust 1.98.1 from the flake
  (D12). CI runs on two cores, which is the only slow host we have.
- Issues #1 and #16 are **open**; #16 holds the six gated deliverables
  (M18). The SSH key here is a deploy key for another repository, so
  git uses the HTTPS remote with the `gh` credential helper.

## Next action

M10, the command declaration contract: the field set, the
`labels_from` reading run on a runner (D74), the timestamped executor
steps (D77). Alexa's own: the runner's user out of `docker` (D78), and
**Q10, the licence**.
