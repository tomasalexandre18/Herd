# Deploying Herd

Four files, and `docs/joining.md` for the person who walks a machine in.
None of them is a NixOS module: `/etc/nixos` is the owner's, so anything
that needs it is written here as an instruction marked **OWNER**, never
as a file that reaches in.

| File | What it is | Where it goes |
| --- | --- | --- |
| `docker-compose.yml` | the hub, under docker compose (D5, D8) | the operator's compose directory, say `/srv/herd/` |
| `env.example` | the variables that file needs, holding paths and no secrets | copy to `.env` beside it |
| `reverse-proxy/nginx-herd-hub.conf` | a fragment for the nginx that already terminates TLS | pasted into the config he already runs |
| `herd-runner.service` | the server's runner, native and unprivileged (E-47, E-52) | `/etc/systemd/system/` |

The shape, in one picture:

```
  requester  --- wss://host/herd --->  nginx (already there, TLS)
                                         |  ws://127.0.0.1:8787
                                         v
                                  herd-hub  (docker compose)
                                   image herd-hub:0.1.0
                                   state on the herd-state volume
                                         ^
                                         |  ws://127.0.0.1:8787
                                  herd-runner  (systemd, on the host,
                                                NOT in a container)
                                   /dev/kvm, /dev/dri, /dev/vfio
                                   one cgroup per job, memory.max
```

The hub listens; a runner dials out. No port is ever opened on a router,
and nothing on a runner's machine listens at all (invariant 3, D11).

## The order to do it in

1. Build and load the image. That is the packaging half of M16
   (`flake.nix`, `nix/`); its contract with this directory is the image
   name and tag, **`herd-hub:0.1.0`**, and nothing else.
2. Make the requester token, outside any repository, and point `.env` at
   it. `env.example` says how.
3. `docker compose up -d`, then `docker compose logs hub`. The hub says
   in its first three lines whether a requester token is set, how many
   runners are approved, and where its state is.
4. Paste the proxy fragment and reload the proxy.
5. Install the runner's unit and walk `docs/joining.md`.

## Known gaps, so nobody debugs them twice

- **`wss://` cannot be dialled.** `herd-runner` and `herdctl` are built
  without a TLS feature, so a `wss://` URL fails with `URL error: TLS
  support not compiled in` before it touches the network. The proxy
  fragment is right and ready; the clients are not. The server's own
  runner is unaffected — it reaches the hub on loopback. See the last
  section of `docs/joining.md` for the two ways to join meanwhile.
- **No health endpoint.** The hub speaks WebSocket and nothing else, so
  there is no `healthcheck:` in the compose file. `herdctl runner list`
  from the host is how you ask whether the hub is answering.
- **No pending list.** The hub does not record a runner that dialled and
  was refused, so the name to approve travels out of band — the joiner
  tells the operator. `docs/joining.md` step 4 says so plainly.
- **The image's user.** The compose file does not set `user:`; the
  image's own applies, and the `herd-state` volume must be writable by
  it. The hub refuses to start naming the directory if it is not, so a
  mismatch is a loud first start.

## What stays the owner's to do

Each of these is in `/etc/nixos`, and each is marked **OWNER** in the
unit file where it bites:

- a system user `herd`, `isSystemUser`, in group `herd` — and **not** in
  the `docker` group, now or ever (D78). `id -nG herd` must not print
  `docker`. The runner itself warns at startup if it is, naming the
  group, so this is checkable from the journal as well.
- `environment.systemPackages` carrying the runner, so that
  `/run/current-system/sw/bin/herd-runner` — the path in the unit —
  exists.
- a udev rule giving `/dev/vfio/*` a group, if VFIO work is wanted. It
  is `root:root 0600` on a stock kernel, which no supplementary group
  can reach.
