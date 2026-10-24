# Status — 2026-10-24

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`.

## Who operates what

**The first consumer operates the running system**, since 2026-10-23
(D161, `docs/handover.md`): the hub container and its volume, the runner
units and their data roots, the capability files, the tokens, the disk
and the pins. Ours is what a tag produces — the protocol, the four
crates, `deploy/`, the releases and these docs. Every figure about their
host below is one they reported, dated the day they said it, never ours.

## What is running, and what is published

The fleet as the consumer reported it at **11:54 on 2026-10-24**: the
hub is **`herd-hub:0.3.6`**, the server's runner is **0.3.6** through a
flake pin (their nixos-config `66d3eac`), `herdctl` is **0.3.6**, and
the binary for the **PC 4060**, their own second machine, is staged for
its next fill. They installed v0.3.6 with their own `update.sh` and
**no hand steps**: the fetch verified five assets and their sums, and
their `same_numbers` check read `RECORD_VERSION: 1` and
`PROTOCOL_VERSION: 2` out of the release body. `--readings-at-once` is
left at its default of 2.

**The fleet is effectively one runner.** The PC 4060 ran one at 0.3.3
until its owner took it back, so the server's runner is the only machine
connected — and it is not M17's prepared machine (D149), which is a
different one again. That is the shape of the fleet now, not a dip it is
coming back from.

`main` carries M1-M11, M15, M16 and decisions through D169. The fleet is
on **protocol version 2** and **record version 1**, both unmoved since
v0.3.0 (D73). The gates are green with **12** loud skips (D27), all of
cgroup delegation or ZFS `io.max`, the same set on three branches today.

**v0.3.4, v0.3.5 and v0.3.6** are published, cut on 2026-10-23 and
2026-10-24, each verified by consuming it: the path that produced
nothing for v0.3.1 through v0.3.3 (D162) has produced three in a row.
From v0.3.6 the body states `RECORD_VERSION` and `PROTOCOL_VERSION` as
bare matchable lines (D168), so an operator's updater decides alone.

## What v0.3.6 fixed, and what it cost the consumer

On 2026-10-24 the consumer submitted a **214-job nightly pass at once**
against the single runner. Every job needed a requirement reading, which
reserved nothing at all, and about **200 ran together** on a 20 GiB,
eight-thread lend. The host saturated, the kvm probe missed its 2 000 ms
answer in a loop and the capability was suspended (D49), and **201 of
the 214 jobs ended 125 with no test run**. A second occurrence the same
day reached **255 concurrent readings at host load 399**; both stayed
up. v0.3.6 bounds readings per runner (`--readings-at-once`, default 2,
queued and never refused), stops a *lost* reading from refusing the job
it was read for, and makes the watchdog ask the holding runner's last
health report before believing its own clock (D169). That closes
yesterday's open item, a reading that weighs nothing.

## What is not done, and why

1. **The reaper's janitor list is unbuilt** (D134), the last of the
   consumer's four blocking items: a failing `up` leaves workspaces
   behind and Herd records only that the `down` step failed.
2. **M11's pack half does not exist** — no registry or pack frame is on
   either protocol enum — so E-12 and E-17 are undelivered.
3. **The hub re-reads every job record** on each 200 ms poll of a
   waiting `--outputs-to` (D152), invisible at today's count.
4. **M17's second machine is prepared, not joined** (D149), a different
   machine from the PC 4060: the unknown is whether a user scope on
   WSL2 gets a delegated subtree with `memory` in it. With the 4060
   gone, that join is also what would end the single-runner fleet.
5. **One shared token**: any requester may ask for the merger level.
6. **D166 shipped only half of the fleet listing**: `herdctl runner
   list` gives each runner's version and suspended capabilities but not
   the hub's own version, so an operator comparing a fleet still reads
   one of the two numbers off a container.

**Two figures we no longer measure.** About **90 GB across 1 571 jobs**
against D152's 200 GB budget with the sweeper never having evicted, and
about **7.4 GB of the runner's spool no runner can remove** (D158), were
read off the consumer's disk on **2026-10-23**, before the handover.
Neither is re-read: M13's hub status view would replace them, and until
it exists a figure about their host comes from them, not from us.

## Next action

**The consumer's next full pass is 2026-10-25 at 03:00** — 214 jobs at
once against one runner, the first test of the cap at 2 on a 20 GiB,
eight-thread lend. The default is a recommendation, not a measurement;
their run would change it. Then D134's janitor list, then M17's join.
