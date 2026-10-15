# Deploying Herd

Eight files besides this one, of which you use five — the two
reverse-proxy fragments are alternatives, the Traefik override is only
for a containerised proxy, and `release-notice.md` deploys nothing at
all: it is what ships with a release, for the person on the machine that
is joining — plus `docs/joining.md` for the person who walks a machine
in. `/etc/nixos` is the owner's and nothing here
ever writes to it: what needs it is either an instruction marked
**OWNER** in the unit file, or — since D126 — a module he may import if
he wants it, which is an offering and not an installation.

| File | What it is | Where it goes |
| --- | --- | --- |
| `docker-compose.yml` | the hub, under docker compose (D5, D8) | the operator's compose directory, say `/srv/herd/` |
| `env.example` | the variables that file needs, holding paths and no secrets | copy to `.env` beside it |
| `docker-compose.traefik.yml` | an override, only when the proxy is a container: puts the hub on the proxy's network | beside `docker-compose.yml`, passed with a second `-f` |
| `reverse-proxy/traefik-herd-hub.yml` | **this server:** a Traefik v3 dynamic config for `wss://` | the directory Traefik's file provider watches, `conf.d/` |
| `reverse-proxy/nginx-herd-hub.conf` | the same job on a host that runs nginx instead | pasted into the config he already runs |
| `herd-runner.service` | the server's runner, native and unprivileged (E-47, E-52) | `/etc/systemd/system/` |
| `nixos-herd-runner.nix` | the same runner declared: the unit plus the user, the binary's path and the devices the unit marks OWNER | imported from `/etc/nixos`, or not at all |
| `release-notice.md` | what a release's binaries are and are not: bare binary against unit, the four things a runner needs, the filesystem that decides whether IO is bounded, and the two gaps | quoted into every GitHub release body by `.github/workflows/release.yml`; nothing installs it |

**Two paths for the runner, and a machine is on exactly one.** Either the
unit file is copied into `/etc/systemd/system/` and the OWNER notes in it
are satisfied by hand, or the module is imported and declares them. The
project's server is on the **first**: tonight's `herd` user was made with
`useradd --system` and the unit was dropped into `/etc/systemd/system/`,
which works and survives a reboot but is unmanaged state outside the
NixOS generation. The module is how that becomes declarative whenever the
owner wants; its header says how to import it and what the flake already
exposes. The two must be changed together — see the header of either.

Pick exactly one proxy fragment: the one that names the proxy the host
actually runs. They are not a translation of each other and nothing is
gained by reading both.

The shape, in one picture — the project's own server, which is the only
machine this has ever been applied to:

```
  requester    wss://herd.example.com/herd
  or runner  ------------------------------>  Cloudflare  (holds the
  elsewhere                                    certificate, terminates
                                               TLS, ~100 s idle window)
                                                   |
                                                   v
                                             cloudflared  (dials out;
                                                   |        no inbound port)
                                                   |  http, docker network
                                                   v
                                             Traefik v3  (router + the
                                                   |       timeouts; holds
                                                   |       no certificate
                                                   |       on this path)
                                                   |  ws://herd-hub:8787
                                                   v
                                         herd-hub  (docker compose)
                                          image herd-hub:0.1.0
                                          state on the herd-state volume
                                            ^                 ^
         ws://127.0.0.1:8787  -------------'                  |
          herdctl, on the host                                |
                                                ws://127.0.0.1:8787
                                         herd-runner  (systemd, on the
                                                       host, NOT in a
                                                       container)
                                          /dev/kvm, /dev/dri, /dev/vfio
                                          one cgroup per job, memory.max
```

The hub listens; everything else dials out. No port is opened on a
router anywhere in that picture — cloudflared dials Cloudflare, a remote
runner dials Cloudflare, and nothing on a runner's machine listens at
all (invariant 3, D11). Herd holds no certificate at any point.

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
4. Publish it as `wss://`, if anything will dial from off this host.
   On a Traefik host that is two commands and one **OWNER** step in the
   Cloudflare dashboard — "Step 4, exactly", below. On an nginx host it
   is `reverse-proxy/nginx-herd-hub.conf` pasted in and the proxy
   reloaded. Skip it entirely while every runner and requester is on
   this machine: they reach the hub on loopback and want no proxy.
5. Make the runner's data root — `install -d -o herd -g herd -m 0700
   /mnt/ssd_980/herd-runner` — then install the unit and walk
   `docs/joining.md`. On the declarative path, importing the module does
   this step and every other **OWNER** item with it.

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

## Step 4, exactly: `wss://` through Traefik and Cloudflare

This is the half of M16 that **has never been run**. The loopback path
above was walked end to end; everything in this section is written as the
commands that would prove it, with the output each one must produce, so
that whoever has the server confirms it in minutes instead of deriving
it. Nothing below was executed, and it says where that matters.

### Apply it

Two commands on the host, and one step that is the owner's.

```sh
# 1. the hub also joins the network the proxy is on, keeping its loopback
#    publish for this host's own runner and herdctl. No new port.
docker compose -f docker-compose.yml -f docker-compose.traefik.yml up -d
docker inspect --format \
  '{{range $n,$v := .NetworkSettings.Networks}}{{$n}} {{$v.Aliases}}
{{end}}' herd-hub-1
# traefik [herd-hub ...]      <- the name the fragment dials
# herd_herd [hub ...]

# 2. the router and its timeouts, into the directory Traefik watches.
#    Edit the hostname in it first. `watch: true` means this is the whole
#    of the install: no restart, no reload, nothing else on the host is
#    touched.
sudo install -m 0644 reverse-proxy/traefik-herd-hub.yml \
  /path/to/traefik/conf.d/herd-hub.yml
docker logs --since 1m traefik-traefik-1 2>&1 | grep -i 'herd\|error'
# ... Creating server URL=http://herd-hub:8787 routerName=herd-hub@file
# and no "field not found", no "cannot create service"
```

**OWNER**, in the Cloudflare dashboard: add the public hostname to the
tunnel, with the same service target as every other hostname on this
host, and nothing else enabled on it — no Access policy, no Zero Trust
application. An Access policy in front of the hub is the same failure as
forward auth in Traefik: a 302 to a login page where a WebSocket upgrade
should have been, and no runner can answer it. The zone's WebSockets
setting (Network → WebSockets) must be on; it is on by default.

### Which certificate — the hour this saves

Cloudflare holds it and terminates TLS; Traefik holds none on this path;
Herd holds none anywhere, ever. The one rule: **the certificate a Herd
client sees must be signed by a public CA.** `herd-runner` and `herdctl`
verify against the webpki roots bundled in the binary and have no
trust-this-CA flag, by decision (D115), so a Cloudflare **Origin CA**
certificate, a self-signed one, Traefik's built-in fallback certificate,
or a hostname two labels deep under Universal SSL each make every runner
and every requester fail to dial — with a TLS error, before the hub sees
a byte, so Herd cannot explain it. `reverse-proxy/traefik-herd-hub.yml`
has the long version at the top.

### 1. The upgrade reaches the hub

From a machine that is not the server:

```sh
export HERD_TOKEN=$(cat the-requester-token)
herdctl --hub wss://herd.example.com/herd runner list
# server   approved   connected   ...
```

That table can only be printed after a 101, a completed TLS handshake and
an accepted Herd handshake, so one command settles all three. The useful
negative control is the same command with no token: the hub must answer
in words — a refusal naming the token — because a *named refusal is
itself proof the upgrade worked*. What each wrong answer means:

| What you see | Where it broke |
| --- | --- |
| `herd cannot give you a verdict ...: invalid peer certificate: UnknownIssuer` (or another rustls error) | the certificate is not publicly trusted. The section above; no flag will get you past it |
| `HTTP error: 404 Not Found` | the router rule did not match — hostname or path prefix |
| `HTTP error: 302 Found` | forward auth or a Cloudflare Access policy is in front of the hub |
| `HTTP error: 403 Forbidden` | an IP-reputation middleware (crowdsec) is on the router |
| `HTTP error: 200 OK` | the upgrade was answered as plain HTTP by something that is not the hub — the exact symptom D115 recorded |
| `HTTP error: 502 Bad Gateway` | Traefik is there and the hub is not reachable from it: step 1 of "Apply it" did not happen, or the alias differs |
| a refusal from the hub, naming the token | **the whole path works.** This is the pass |

On the host, the same thing seen from the other end — a long-lived
WebSocket appears in Traefik's access log once, when it *closes*, so an
absent entry during a live link is normal:

```sh
grep '"RequestPath":"/herd"' /path/to/traefik/logs/access.log | tail -1
# ..."DownstreamStatus":101,"RouterName":"herd-hub@file"...
```

### 2. A link quiet longer than every idle window survives

The point of D117 and D118, and the one test that cannot be replaced by
reading configuration. Follow a job that prints nothing for four minutes,
from off the host:

```sh
time herdctl --hub wss://herd.example.com/herd \
  run -- sh -c 'sleep 240; echo done'
# done
# real 4m0s      and exit 0
echo $?          # 0
```

240 s is chosen to be past every figure on the path at once: Cloudflare's
~100 s idle window, Traefik's 180 s entrypoint `idleTimeout` and its 60 s
`readTimeout`. It passes because the hub sends a keep-warm frame every
15 s to the followers of a running job (D118), so the link is never
actually idle — not because any timeout was raised.

The control, which must also pass, and which tells you whether to blame
the proxy or Herd: the same command on the host, over loopback.

```sh
herdctl --hub ws://127.0.0.1:8787 run -- sh -c 'sleep 240; echo done'
```

**If it fails**, the failure is unmistakable and so is its cause. You get

```
herdctl: herd cannot give you a verdict for this job: the hub closed the
connection while <job id> was still running
```

and **exit 125 on a job that succeeded** — the wrong answer D117 names, and
the reason a merger must never see it. `herdctl wait <job id>` afterwards
prints the real verdict, which is how you confirm the job itself was fine.
Then **the number of seconds it survived names the component**:

| Died at about | Who closed it |
| --- | --- |
| 60 s | the entrypoint's `respondingTimeouts.readTimeout` does reach an upgraded connection on this version after all — **OWNER**: raise it, or set it to 0, in Traefik's static `traefik.yml` |
| 100 s | Cloudflare's idle window, and Herd's keep-warm frames are not arriving: either the hub predates D118 or something between is swallowing them |
| 180 s | the entrypoint's `idleTimeout` — **OWNER**, same file |
| exactly `--job-timeout` | not this at all; the job hit its own cap |

Both **OWNER** rows are static per-entrypoint configuration shared with
every other service on that host, which is why Herd's fragment cannot set
them and why this walk reports the figure rather than pre-emptively
changing someone else's entrypoint.

### 3. An idle runner's link survives too

A runner reports health every 15 s, so its link is never idle either —
but it is the connection that stays up for weeks, so it is worth ten
minutes:

```sh
# on the joining machine, dialling wss:// per docs/joining.md
herd-runner --hub wss://herd.example.com/herd --name wsl-pc ...
# then, ten minutes later, with no job submitted, from anywhere:
herdctl runner list
# wsl-pc   approved   connected
journalctl -u herd-runner | grep -c 'dialling'   # still 1
```

A second `dialling` line means the link was closed and the runner
redialled — which it survives by design (a `Close` spends no failure
counter), so this is the check that distinguishes "working" from "working
because it reconnects every ninety seconds".

## Known gaps, so nobody debugs them twice

- **A job's `PATH` is the unit's `PATH`** — fixed, and written down
  because it cost one real job an exit 125. The runner builds each job's
  environment from scratch and carries only `PATH`, `LANG`, `LC_ALL`,
  `TZ` and `TERM` into it, and systemd's default `PATH` holds no shell
  on a NixOS host, so `herdctl run -- sh -c '…'` failed with `sh: No
  such file or directory`. The unit now states
  `Environment=PATH=/run/current-system/sw/bin`; the value is
  host-specific, the module mirrors it as
  `services.herd-runner.jobPath`, and the comment beside it says which
  other variables were checked and deliberately not added.
- **The `wss://` path is configured and unproven.** The clients can dial
  it — rustls with the bundled webpki roots, as of D115, so the old
  `URL error: TLS support not compiled in` is gone — and the Traefik
  fragment is written for this server's actual proxy. What has never
  happened is one real dial through Cloudflare into the hub. "Step 4,
  exactly", above, is that walk written out; until someone runs it, read
  every claim in it as reasoning from the hub's behaviour and from this
  host's configuration, not as a measurement.
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

This is the **imperative** path's list — the one the server is on. Each
is marked **OWNER** in the unit file where it bites, and
`nixos-herd-runner.nix` declares every one of them instead:

- a system user `herd`, `isSystemUser`, in group `herd` — and **not** in
  the `docker` group, now or ever (D78). `id -nG herd` must not print
  `docker`. The runner itself warns at startup if it is, naming the
  group, so this is checkable from the journal as well. The module
  asserts it at evaluation for every declarative route into that group,
  but Nix cannot make the membership impossible and no assertion sees a
  `usermod -aG` run by hand, so `id -nG herd` stays the answer.
- the data root, `/mnt/ssd_980/herd-runner`, owned by that user and mode
  0700, made before the first start: `install -d -o herd -g herd -m 0700
  /mnt/ssd_980/herd-runner`. It is on the NVMe because an `io.weight`
  reaches ext4 and never ZFS (D120, D126), and it is the operator's to
  create because the unit no longer has a `StateDirectory=` to do it. A
  root the runner cannot write is a refusal naming the path; a root left
  at the default 0755 is not, and leaves every job's workspace readable
  by every user of the host. The module uses `systemd.tmpfiles` and
  re-asserts owner and mode at every boot.
- `environment.systemPackages` carrying the runner, so that
  `/run/current-system/sw/bin/herd-runner` — the path in the unit —
  exists. The module does not need it: its `ExecStart` is the package's
  own store path, which cannot be missing.
- a udev rule giving `/dev/vfio/*` a group, if VFIO work is wanted. It
  is `root:root 0600` on a stock kernel, which no supplementary group
  can reach. The module writes that rule behind
  `services.herd-runner.vfio = true`.

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
