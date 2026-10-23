# Status — 2026-10-23

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`.

## Who operates what

**The first consumer operates the running system**, as of today: the hub
container and its volume, the runner units and their data roots, the
capability files and probes, the tokens, the disk and the version pins.
Ours is what a tag produces — the protocol, the four crates, `deploy/` as
reference texts, the releases and these docs. So nothing here sends its
reader into our source, and a figure read off a host says so.

## What is running

- The hub is **`herd-hub:0.3.3`** (`docker ps` on the server, today).
- The server's runner is **0.3.2**: `/etc/nixos/flake.nix` line 15 pins
  tag `v0.3.2` and `systemctl cat herd-runner` shows a store path named
  `herd-runner-0.3.2`. It is declarative and survives a reboot — the
  generation holds that path, so no garbage collection can take it
  (E-52's server half).
- Herd is the consumer's only path: they have removed their fallback, so
  a job Herd cannot run fails their build on purpose. Long jobs, KVM
  behind a busy probe and the exclusive `rig-cpu` label behave as
  designed on their runs — `rig firefox` at 2 660 s, `test web-firefox`
  at zero pixels off. One build slot and two kvm; files come home and
  large ones stream, no requester seeing which carriage it got (D148,
  D150).
- The hub's volume holds about **90 GB across 1 571 jobs** against D152's
  200 GB budget, and the sweeper has **never evicted anything**, so its
  first eviction will be its first real test. About **7.4 GB of the
  runner's spool cannot be removed by any runner** (D158): the consumer's
  jobs deposit trees under a foreign uid, and a runner cannot unlink
  inside a directory it does not own. Both measured on their disk today.

**Three fixes are on `main` and in no tag**, so the running runner has
none of them: its figures cannot be restated by its operator, so
`--host-tmpfs 4GiB` permanently refuses the consumer's 10 GiB jobs on a
host with 16 GiB of `/dev/shm` (D163); a persisted `drain` is obeyed at
every start, so a withdrawn machine restarts in a loop and comes back
only from a shell on it (D164); and a capability file whose probe cannot
be started is accepted, suspending that capability while it is live
(D165).

`main` carries M1-M11, M15 and M16, the fleet is on protocol version 2, a
job may return 1 GiB, and the gates are green with **12** loud skips
(D27) — every one a cgroup-delegation or ZFS `io.max` assertion no host
without a delegated subtree on a block device can make.

## Releases: three tags produced nothing

The newest release is **v0.3.0, of 2026-10-16**. `v0.3.1`, `v0.3.2` and
`v0.3.3` each failed `release.yml` in 22-26 s on a Docker Hub credential
preflight — a refusal, not a build (D162). The registry is off that path
now: the binaries publish first, the hub image ships as a release asset
built by the one derivation (`nix/hub-image.nix`), the preflight is gone,
and the workflow ran green by hand today, 4 m 1 s, with no tag. **What is
missing is a tag**: until one is cut nobody can download a binary and
D163-D165 reach no machine.

## What is not done, and why

1. **The reaper's janitor list is unbuilt** (D134), the last of the
   consumer's four blocking items: a failing `up` leaves workspaces
   behind and Herd records only that the `down` step failed.
2. **A requirement reading weighs nothing**: it declares no memory, and
   nothing bounds how many run at once on a runner (D159): two builds
   in two slots beside two readings are four processes counted as two.
3. **M11's pack half does not exist** — no registry or pack frame is on
   either protocol enum — so E-12 and E-17 are undelivered.
4. **The hub re-reads every job record** on each 200 ms poll of a
   waiting `--outputs-to`: about 0.9 s of hub time per poll at 100 000
   records (D152), invisible at today's count.
5. **M17's second machine is prepared, not joined** (D149): the
   load-bearing unknown is whether a user scope on WSL2 gets a delegated
   subtree with `memory` in it.
6. **One shared token**: any requester may ask for the merger level, and
   `wss://` is configured and never dialled; the tailnet makes it optional.

Being built now and on nobody's `main`: a machine-readable verdict, the
runner's version and suspended capabilities in the fleet listing, and a
reading costed and bounded.

## Next action

Cut a tag: the only way D163-D165 reach the running runner and the only
way an operator gets a binary, on a release path green by hand and never
yet proven on a tag. That is Alexa's word to give. Then the janitor list
(D134), then M17's join.
