# Status — 2026-10-15

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
`pids.max 1024`, in one cgroup per job under the delegated service. A job
at exit 0, one at exit 7, and one silent for 50 s whose follower never
went quiet.

`main` carries M1-M10 and M15. The M16 branch `m16-deploy-hub` adds: the
flake's packages and the hub image, the compose deployment, the runner
unit and a NixOS module for it, `wss://` in the clients (rustls, webpki
roots), the four cgroup controls plus an optional hard `io.max`, pressure
recorded and gating nothing, a keepalive so a quiet follower is never
cut, and release binaries built by CD as **static musl** — proven by a
real hub, runner and two jobs on Arch and Alpine with no Nix anywhere.

`fmt`, `clippy -D warnings` and `./scripts/test.sh` are green on that
branch: **575 passed**, 7 loud skips, all one cause — this user cannot
delegate a cgroup.

## What is not done, and why

1. **`main` is red**, on `killed_hub.rs`, and the same code passed as a
   pull request and failed as a push. A two-core CI host decides every
   merge, so this blocks M16's own pull request. Being fixed.
2. **E-52 is unmet on the server.** `/etc/systemd/system` is read-only
   here — NixOS manages `/etc` by activation — so the running unit is
   **transient** and will not survive a reboot (D130). The module in
   `deploy/` is the only route, and it needs `/etc/nixos` plus a
   rebuild: Alexa's.
3. **Herd returns an exit code and logs, not files.** A job's `{out}` is
   reclaimed with its workspace, so the consumer's `results.json` does
   not come home. Ruled (D131) and being built.
4. **`wss://` is configured and never dialled.** The Traefik fragment is
   written for this host's real proxy and loads in Traefik, but no real
   dial through Cloudflare has happened. The tailnet makes it optional.
5. **Any user of this host can become root.**
   `/var/run/libvirt/libvirt-sock` is mode 0666 and libvirtd runs as
   root, so a job could escape every cap (D129). Not Herd's to close —
   it is the host's, and it predates us. The runner says so at startup.
6. **The host's disk freezes are not a Herd problem.**
   `io.cost.qos` reads `rpct=0 wpct=0`, so blk-iocost apportions the NVMe
   and never throttles it to a latency target — every weight is honoured
   while `io.pressure full` sits at 43 (D136). One `/etc/nixos` line
   would protect every cgroup on the machine.
7. No authentication ceiling per requester: one shared token means any
   requester may ask for the merger level.
8. A test that cannot be made honestly on a loaded host **skips loudly**
   and never passes quietly (D27). Seven do here.

Open and recorded: D63, D64, D92, D98, D101, D122's two reaper blind
spots, D134's janitor gap. The consumer's items 7, 8 and 9 — a flock
probe, per-capability probe budgets, and `labels_from` costing a whole
executor cycle — are accepted and unbuilt.

## Environment facts

- Server: NixOS 6.18, 12 threads, 31 GB RAM, **and swap** — 16 GB zvol
  plus 7.8 GB zram, the zram saturated. Rust from the flake (D12).
- **Everything is on one device.** The consumer's docker root, their
  work, this checkout and the runner's data root all sit on
  `/dev/nvme0n1p1` (ext4, `259:0`), where blk-iocost is live so a cgroup
  weight is honoured. `/` and `/tmp` are ZFS, where **no** cgroup IO knob
  reaches, ever (D120, D126).
- A low-weight scope reproduces the two-core CI host locally:
  `systemd-run --scope -p IOWeight=20 -p CPUWeight=20`.
- CI runs on two cores. Issues #1 and #16 are open.

## Next action

Fix `main`, then open M16's pull request. Then the consumer's four
blocking items: files home, `{out}` in a declared line, the reaper's
janitor list, and telling "nothing submitted" from "no verdict".
Alexa's own: the `nixos-rebuild` for E-52, the libvirtd socket, the
`io.cost.qos` latency target, and **Q10, the licence**.
