# Status — 2026-10-16

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`.

## What works

**Herd is deployed, running, and the first consumer's only path**: they
have removed their fallback, so a job Herd cannot run now fails their
build on purpose and problems surface instead of hiding.

**The server's runner is declarative and survives a reboot (E-52).**
`/etc/nixos` imports Herd at tag `v0.2.0` and
`deploy/nixos-herd-runner.nix`; the generation holds the store path, so
no garbage collection can take it, and the rendered unit was diffed
against the transient one it replaced — identical flag for flag. Its
caps, read back out of the kernel by a job: `memory.max` 8 GiB in one
cgroup v2 subtree per job, `pids.max 4096`, `cpu.weight 10`, and a hard
`io.max` on `259:0` — `rbps=775MB wbps=435MB riops=180000 wiops=105000` —
**proved at startup by writing it**. `io.weight` bites because the data
root is ext4 on the NVMe. Pressure is recorded and gates nothing (E-32).

**Files come home, and large ones stream.** `j0000000030` and
`j0000000031` put their `results.json` on the hub's disk at exit 0, read
there rather than reported. At or under 32 MiB an output rides the
verdict, above it streams; a requester cannot tell which carriage it
got, and that is a test (D148, D150).

**Protocol version 2, and the whole fleet is on it.** v0.3.0 is released,
the hub runs `herd-hub:0.3.0` and the runner 0.3.0, switched together in
one window and proved with a job through the consumer's own `herdctl`.
The bump bought the file frames, M11's registry frames, and the catch-all
arm that ends D128 — an unknown frame used not to decode at all, so a
runner looped redialling into it: 1 dial with the arm, **178** without.
`RECORD_VERSION` stays 1, and a job may now return **1 GiB**.

**Proven by the consumer through Herd**: `rig` (1 799 s), `rig firefox`
(2 660 s), `test d3d` (2 006 s), `test web-firefox` at zero pixels off,
four `check` runs — so long jobs, KVM behind a busy probe and the
exclusive `rig-cpu` label all behave as designed. The runner offers
**one** build slot and **two** kvm, Alexa's choice: a VM costs little
next to a build. `main` carries M1-M11, M15, M16 and Apache-2.0, and the
gates are green with 13 loud skips, every one this host's.


## What is not done, and why

1. **The reaper's janitor list is unbuilt** (D134), the last of the
   consumer's four blocking items: when a `down` step fails, Herd records
   that and does nothing else, so a failing `up` leaves workspaces behind.
2. **The hub re-reads every job record** on each 200 ms poll of a waiting
   `--outputs-to` — invisible at today's job count, about 0.9 s of hub
   time per poll at 100 000 records (D152). Next after the janitor work.
3. **M17's second machine is prepared, not joined.** Everything possible
   without the PC is done (D149); the load-bearing unknown is whether a
   user scope on WSL2 gets a delegated subtree with `memory` in it.
4. **`wss://` is configured and never dialled**; the tailnet makes it
   optional. And one shared token means any requester may ask for the
   merger level: no authentication ceiling per requester yet.
5. A test that cannot be made honestly **skips loudly** (D27). Thirteen
   do here, and that counter was itself wrong until today: it said 12.

Open and recorded: D63, D64, D92, D98, D101, D122's two reaper blind
spots, D134's janitor gap, and the consumer's items 7-9 — a flock probe,
per-capability probe budgets, a lighter `labels_from`.

## The host, as it really is

- `/mnt/ssd_980` is ext4 on `/dev/nvme0n1p1`, and the cgroup IO knobs
  live on the whole NVMe, `259:0`, not the partition; `/` and
  `/nix/store` are ZFS, which **no** cgroup IO knob reaches (D144).
- `io.cost.qos` carries a p95 5 ms target, applied live: under load every
  top-level cgroup is held to a share, pressure reaches 95-99 % for a
  minute and recovers, with no freeze of the old kind. The floor is still
  `min=1.00`, a cut to 1 % of the cost model; `cost.vrate` in
  `/sys/fs/cgroup/io.stat` is what would say whether that bites.
- A Herd job here is equivalent to the `server` account, by a uid mapping
  recorded in `/etc/nixos`, and the libvirt socket is `0666`: both are
  settled, not open.
- `systemd-run --scope -p IOWeight=20 -p CPUWeight=20` reproduces CI.

## Next action

M17: the runbook is with Alexa and stops after the verification script,
because whether a user scope on that WSL2 delegates a cgroup with
`memory` in it decides the shape of everything after it — and the same
block reports the PC's RAM, the other figure nobody here has. Then the
janitor list (D134). Waiting on Alexa: a Docker Hub account, a
repository-scoped token and two repository settings, after which a hub
deploys by `docker compose pull` instead of a hand-run `docker load`.
