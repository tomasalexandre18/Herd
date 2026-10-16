# Status — 2026-10-16

State now. Replaced every session, never appended to. Per-expectation
detail is in `docs/coverage.md`, the reasoning in `docs/decisions.md`,
the order of work in `docs/plan.md`.

## What works

**Herd is deployed and running on the server.** Hub in docker compose,
runner native on the host under a dedicated `herd` user that is not in
`docker`. The hub listens on loopback *and* on the tailnet address, bound
to those two interfaces and nothing else (invariant 3, D11). The protocol
is frozen at **version 1** (D73), so this is the shape a second machine
joins.

Proven on that deployment, read back by a job out of the kernel rather
than asserted: `memory.max 8589934592`, `cpu.weight 10`, `io.weight 10`,
in one cgroup per job under the delegated service. A job at exit 0, one
at exit 7, and one silent for 50 s whose follower never went quiet.

**A job's files come home, proved live and not only by the suite.**
The first consumer's j0000000030 and j0000000031 ended at exit 0 and
their `results.json` — 50 and 616 bytes — are on the hub's disk under
`jobs/<id>/files/`, read there rather than reported (D131, D137).

**The caps the `server` runner carries, approved by Alexa (D144):** 3 h
per job, `pids.max 4096`, and a hard `io.max` on device `259:0` —
`rbps=775MB wbps=435MB riops=180000 wiops=105000`, a quarter of the
kernel's cost model, proved at startup by writing it. Every job's PATH is
`/run/wrappers/bin:/run/current-system/sw/bin`. It declares the first
consumer's capabilities from `/mnt/ssd_980/herd-etc/chatons-runner.json`:
`build` (2 slots), `kvm` and `gpu-venus` (1 slot each, both behind a busy
probe), `reference` (`shared`), `rig-cpu` (`exclusive`), and `igpu-vfio`,
`nightly` and `cache-home` denied.

`main` carries M1-M10, M15, M16 and Q10's Apache-2.0 licence. `fmt`,
`clippy -D warnings` and `./scripts/test.sh` are green: **623 passed**,
13 loud skips, every one this host's — no cgroup delegation under the
test scope, ZFS on `/`, no root-equivalent group.

## What is not done, and why

1. **E-52 is unmet on the server.** `/etc/systemd/system` is read-only
   here — NixOS manages `/etc` by activation — so the running unit is
   **transient**: a reboot loses it, caps and capabilities with it
   (D130). `deploy/nixos-herd-runner.nix` can now state every figure
   that unit carries — `jobTimeout`, `jobPidsMax`, `jobIoMax` and the
   wrappers directory on `jobPath` (D144) — so the rebuild is a no-op
   rather than a weakening. It needs `/etc/nixos` plus a
   `nixos-rebuild`: Alexa's.
2. **`wss://` is configured and never dialled.** The Traefik fragment is
   written for this host's real proxy and loads in Traefik, but no real
   dial through Cloudflare has happened. The tailnet makes it optional.
3. No authentication ceiling per requester: one shared token means any
   requester may ask for the merger level.
4. A test that cannot be made honestly on a loaded host **skips loudly**
   and never passes quietly (D27). Thirteen do here.

Open and recorded: D63, D64, D92, D98, D101, D122's two reaper blind
spots, D134's janitor gap. The consumer's items 7, 8 and 9 — a flock
probe, per-capability probe budgets, and `labels_from` costing a whole
executor cycle — are accepted and unbuilt.

## Environment facts

- Server: NixOS 6.18, 12 threads, 31 GB RAM, **and swap** — 16 GB zvol
  plus 7.8 GB zram, the zram saturated. Rust from the flake (D12).
- **Everything is on one device.** The consumer's docker root, their
  work, this checkout and the runner's data root all sit on
  `/dev/nvme0n1p1` (ext4, mounted at `/mnt/ssd_980`), where blk-iocost is
  live so a cgroup weight is honoured. A cgroup's IO knobs name the whole
  NVMe, `259:0`, not the partition — `259:1` has no request queue of its
  own. `/`, `/tmp` and `/nix/store` are ZFS, where **no** cgroup IO knob
  reaches, ever (D120, D126, D144).
- A low-weight scope reproduces the two-core CI host locally:
  `systemd-run --scope -p IOWeight=20 -p CPUWeight=20`.
- CI runs on two cores. Issues #1 and #16 are open.

## Next action

The reaper's janitor list (D134) is the first consumer's last blocking
item, and their first failing `up` walked straight into it: when a
`down` step fails too, nothing here sweeps after it. Then the second
machine (M17) — the hub is already on the tailnet, so that is a runner
on the Windows PC's WSL2 and the Arch box, not a `wss://` dial (D145).

Behind those: per-token priority ceilings, a `flock` probe type, and
`docs/joining.md` step 1, which still says "build it in the dev shell"
where a release is now one download.
