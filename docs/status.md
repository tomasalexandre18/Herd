# Status — 2026-10-13

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`,
the order of work in `docs/plan.md`.

## What works

Nine milestones are merged to `main`: M1 through M9. Nothing else is
written and waiting. M1-M5 are additionally verified against the real
binaries by a separate independent adversarial pass; M6-M9 by their
own milestone tests only — that distinction is the cost of dropping
the separate pass after M5, and it is deliberate.

What a requester can do today, end to end, against the real binaries:
submit and follow or detach; get the job's own exit code untouched, 124 for
a cap, 125 when Herd has no verdict, 128+n for a signal; read a complete
byte-faithful record while the job runs, and after a hub killed mid-job;
cancel it and have the whole tree die. One durable queue orders everything
by four priorities aging off the record, joins a second submission of open
work, never gives a job twice. Placement matches one capability algebra,
counts build slots, derives `-j`, admits against memory and tmpfs costed
from the records, sticks to a preferred runner while it is merely busy,
and explains every decision in one line with its figures. A lost runner
gets one more go and no more. A machine's owner switches it off with a
reason and is obeyed in under a second; a job can carry a deadline and
one named parent; a `metal-*` lease is read as a busy probe so a leased
trial waits instead of failing.

Nothing is deployed: no runner but this host's has ever connected, and
no job of the first consumer's has ever run.

`cargo fmt`, `clippy -D warnings` and `./scripts/test.sh` are green
through `nix develop`: **408 passed**, 2 skipped here, a third on CI.

## What is not done, and why

17 of 44 musts are planned, from M10 on. The consumer cannot submit one
real job until M10 (commands), M11-M12 (artefacts), M13 (status), M14
(nightly, hook) and M15 (auth) land.

1. **No authentication before M15.** The hub holds a fleet (M7), so a
   hand-crafted peer can join beside real runners under any name and
   write a false verdict. **Stay on loopback.**
2. E-45's confinement is cooperative only (D28); the runner's user is
   also in the `docker` group on the deployment host. Settled (D78):
   the runner needs no Docker at all; three checks land in M16.
3. E-30's real kernel ceiling needs a delegated cgroup; the WSL
   replay is a manual M17 check.
4. Both version numbers are free-running: `PROTOCOL_VERSION` is **6**
   (D99 added the switch acknowledgement), `RECORD_VERSION` **5**. An
   older root or peer is refused by name, no migration. Both go back
   to 1 and freeze at M16 (D73).
5. The whole-pass `dispatching` lock is measured, not fixed: 3.31 s
   over 400 queued jobs writing a waiting line each, 46.8 ms steady
   state (D92). M9's dry run costs one more such pass, unmeasured.
6. `off` does not stop serving content: no content endpoint exists
   before M11, and the gate waits behind `Switch::serves_content` (D97).
7. Two new ways a queued job can die: asking for two labels a joining
   runner declares mutually exclusive, and a deadline it missed only
   because its machine was off. Both explained, neither silent.
8. A job no *connected* runner could ever fit **waits** rather than
   ending (D88) — a fleet is not a closed world. Its only bound is
   E-09's deadline, which is why that `should` is load-bearing.

Booked, not fixed: a drain-first job reserves its host from submission,
so a machine can sit idle up to 15 min for a trial something else was
blocking anyway (D98); `SubmitJob`'s wire shape nested under `asked`, so
M9 re-pointed rather than re-proved the frame that refuses a bad priority;
a SIGKILLed runner leaves in-flight probes behind; a verdict the hub could
not write logs "is over" for ever; D63's write-door rule holds by review,
not the compiler; D64's probe sweep is untested at scale; M8's restatement
gives two lines a minute, recorded and not tuned.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no
  swap, `/dev/kvm`, Intel iGPU, docker 29.7. Rust 1.98.1 from the
  flake (D12); nothing is installed globally.
- GitHub: `tomasalexandre18` via `gh`; `main` protected. Issues #1
  and #16 are **open**: #16 holds the six gated deliverables (M18).
- The SSH key here is a deploy key for another repository; git uses
  the HTTPS remote with the `gh` credential helper.

## Next action

M10, the command declaration contract: the field set, the
`labels_from` reading run on a runner (D74), the timestamped executor
steps (D77). Alexa's own, non-blocking: take the runner's user out of
`docker` (D78), and settle **Q10, the licence**.
