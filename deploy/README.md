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
   it. `env.example` says how, including the mode, which is not the one
   you would guess.
3. `docker compose up -d`, then `docker compose logs hub`. The hub says
   in its first three lines whether a requester token is set, how many
   runners are approved, and where its state is.
4. Paste the proxy fragment and reload the proxy.
5. Install the runner's unit and walk `docs/joining.md`.

## The first start, exactly

This sequence was run end to end on a clean named volume, against the
image built from this tree, and it is the one that works. The two
failures underneath it are the ones a first start actually hits; both are
ownership, and both read as the hub refusing to run, which is correct of
it (invariant 2's cousin: a service that cannot do its job says so and
stops).

```sh
# 1. the image, from the flake, and into the local docker
nix build .#hub-image
docker load < result                       # Loaded image: herd-hub:0.1.0

# 2. the requester token, outside every repository (invariant 7).
#    0444 inside a 0700 directory: see env.example for why the mode is
#    not 0400.
install -d -m 0700 /etc/herd
head -c 32 /dev/urandom | base64 > /etc/herd/requester-token
chmod 0444 /etc/herd/requester-token

# 3. the operator's own .env, beside docker-compose.yml, pointing at it
cp env.example .env                        # then set the path in it

# 4. up, and read the three lines it prints
docker compose up -d
docker compose logs hub
# herd-hub: every requester must carry the token in
#   /run/secrets/herd-requester-token (--requester-token-file); this hub
#   keeps only a hash of it
# herd-hub: 0 runner(s) are approved on this state root; ...
# herd-hub 0.1.0 listening on ws://0.0.0.0:8787, state in /var/lib/herd
#   (root-f7dbe2f978721d9f308054b4be8a7843), 0 job(s) not over, ...

# 5. one runner in, and a job through it
export HERD_TOKEN=$(cat /etc/herd/requester-token)
herdctl runner approve server                 # prints that runner's token
HERD_TOKEN=<that token> HERD_RUNNER_NAME=server herd-runner &
herdctl run -- sh -c 'echo it works'          # exits 0
herdctl run -- sh -c 'exit 7'                 # exits 7
```

`herdctl` reaches the hub on `ws://127.0.0.1:8787`, its default, because
that is where the compose file publishes it.

### `state ... is not usable` — the volume is root's

```
Error: the hub's state directory /var/lib/herd/state is not usable

Caused by:
    creating /var/lib/herd/state/jobs failed: Permission denied (os error 13)
```

The hub is uid 1000 and the volume came up root-owned. It happens when
the volume is mounted on a path the image does not contain: docker seeds
a fresh named volume from the directory it mounts over, inheriting its
ownership, but *creates* a missing mount point itself, as root. The one
path the image makes and gives to the hub is **`/var/lib/herd`**, so the
mount and `HERD_STATE_DIR` are both exactly that and never a
subdirectory of it. If you see this error, the mount and the image have
drifted apart; a volume already made wrong has to be removed, not
chowned-around, because `docker compose up` will never re-seed it.

### `reading the requester token` — the secret is root's

```
Error: reading the requester token from /run/secrets/herd-requester-token

Caused by:
    Permission denied (os error 13)
```

The same thing one layer along. Docker bind-mounts a file secret with the
host's ownership and mode, and it ignores the `uid:`, `gid:` and `mode:`
a compose secret may declare — those are swarm's, and nothing warns you
here. A token file left root-owned 0400, which is how one instinctively
writes a secret, is unreadable by the non-root hub. `env.example` makes
it 0444 in a 0700 directory instead: the directory is what keeps it off
the host, the mode is only what the container sees, and the image holds
no users but `herd` and root.

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
  image's own applies, uid 1000, and both the `herd-state` volume and
  the token file must be readable by it. The hub refuses to start naming
  whichever one is not, so a mismatch is a loud first start — see "the
  first start, exactly" above for both errors verbatim.

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

## Confirming the cgroup delegation, and the two skips it closes

Two assertions of the suite skip on a host that cannot delegate a
cgroup v2 subtree (D27), and both are about the kernel memory ceiling:

```
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
  (the named memory outcome)
the_runner_says_which_memory_ceiling_it_enforces
  (the cgroup branch of the report)
```

`Delegate=yes` in the unit is what closes them, and it is checkable in
two ways. First, against the running service — the line must say the
cgroup, not the fallback:

```sh
journalctl -u herd-runner -n 40 | grep 'memory ceiling'
# herd-runner: memory ceiling: one cgroup v2 subtree per job,
#   enforced by the kernel
```

Second, against the suite, by giving it the same delegation the unit
gives the runner. Outside a delegated scope this prints the two skips;
inside one it prints `Every assertion ran on this host`:

```sh
nix develop --command ./scripts/test.sh                       # 2 skips
systemd-run --user --scope -p Delegate=yes --setenv=HERD_SKIP_LOG=/tmp/skips \
  nix develop --command ./scripts/test.sh                     # none
```

Both were run here: the `confinement` binary reports 27 passed and the
two skips outside a delegated scope, and 27 passed with an empty skip
log inside one.
