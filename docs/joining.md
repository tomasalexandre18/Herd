# Joining a machine to the herd

One machine, one person, under thirty minutes (E-49). Follow it top to
bottom; nothing here is optional and nothing here is a decision you have
to make.

Two people are involved, and the document says which is which:

- **you**, on the machine that is joining;
- an **operator**, at a terminal that can reach the hub. One command,
  step 4. Agree beforehand how you will reach him.

What you never need, so do not go looking for it:

- **no port opened on a router, no port forwarding, nothing reachable
  from the internet.** The runner dials the hub; nothing on your machine
  listens (invariant 3, D11).
- **no membership of the `docker` group** — not now, not later. The hub
  runs in docker compose on the server; your runner has nothing to do
  with Docker, and a runner whose user could write the Docker socket
  would be root on its own host (D78).
- **no root.** The runner runs as an ordinary user.

## Before you start

| What | Why |
| --- | --- |
| Linux with systemd and cgroup v2 | the memory ceiling is a cgroup |
| Nix, with flakes enabled | the toolchain comes from the flake, D12 |
| A name for the machine | `server`, `wsl-pc`; the operator approves it |
| The hub's URL | `ws://127.0.0.1:8787` on the hub's own host, else ask |
| 10 GB free, and somewhere to put a data root | one job's workspace |

Check the two that are not obvious:

```sh
stat -fc %T /sys/fs/cgroup       # must print: cgroup2fs
nix --version                    # any recent version with flakes
```

## The seven steps

| # | Step | Who | Minutes |
| --- | --- | --- | --- |
| 1 | Install `herd-runner` | you | 5-10 |
| 2 | Give it a data root and the hub's address | you | 2 |
| 3 | Probe: dial once, be refused by name | you | 2 |
| 4 | Approve it, and take the token | operator | 1 |
| 5 | Put the token where only this machine reads it | you | 2 |
| 6 | Start it, and keep it starting | you | 5 |
| 7 | Verify: one real job | you | 3 |

### 1. Install `herd-runner`

**A release is one download, and that is the ordinary way in.** The
assets are static musl binaries: nothing on the machine, no toolchain,
no clone. Take the version you want from the releases page.

```sh
v=v0.2.0
base=https://github.com/tomasalexandre18/Herd/releases/download/$v
curl -fLO $base/herd-runner-${v#v}-x86_64-unknown-linux-musl
curl -fLO $base/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS      # verify before you trust it
sudo install -m 0755 herd-runner-${v#v}-x86_64-unknown-linux-musl \
  /usr/local/bin/herd-runner
herd-runner --version
```

`SHA256SUMS` is there so a machine is not trusting a URL. Verify it; the
whole point of shipping it is that somebody checks.

On **NixOS** the binary does not belong in `/usr/local/bin`: the unit in
`deploy/herd-runner.service` expects `/run/current-system/sw/bin`, so
put the package in `environment.systemPackages`, or import
`deploy/nixos-herd-runner.nix`, which does the whole of steps 2, 5 and 6
declaratively. That is an `/etc/nixos` change and belongs to whoever owns
that file.

Building from source is still there if you want it — the toolchain is in
the flake and nowhere else (D12), so clone first and
`nix develop --command cargo build --release -p herd-runner`. You do not
need it to join.

### 2. Give it a data root and the hub's address

The data root is the **one** directory this runner may write in (E-45):
its own files, and every job's workspace, `HOME` and `TMPDIR` inside it.
A root the runner cannot use is refused at startup, naming it.

```sh
install -d -m 0700 ~/.local/state/herd-runner
```

That is also the default, so you may leave `--data-root` out. One runner
per data root (D32): a second runner pointed at the same directory is
refused.

Which filesystem it is on is not neutral. A job's IO is weighted per
cgroup only on a single block device the kernel can cost — ext4 or xfs on
one disk — and never on ZFS, which does its pool IO without attributing
it to the cgroup that asked (D120). So if this machine's IO must be
bounded at all, put the root on a plain disk; the project's server puts
it on its NVMe for exactly that reason (D126). The runner says which of
the two it got in its startup report, so you do not have to guess.

The hub's address is a `ws://` or `wss://` URL, with the scheme spelled
out at the call site on purpose (D14). See **Picking `ws://` or
`wss://`**, at the end, before you pick one: which you can use is
decided by the certificate in front of the hub, not by preference.

### 3. Probe: dial once, be refused by name

Run the runner by hand, with the name you agreed and no token. It will
read the host, print what it found, dial the hub and be refused — which
is exactly what you want: you now know the machine is right, and you
have the sentence the operator needs.

```sh
herd-runner --hub ws://127.0.0.1:8787 --name server
```

Read the report it prints before the refusal. Four lines matter:

- `admission here is ... of memory less a ... floor` — what this machine
  lends and what it keeps for itself.
- `memory ceiling:` — if it says **FALLING BACK**, the ceiling is a
  per-process address-space limit rather than a cgroup. That is a real
  cap, but a weaker one; step 6 is what fixes it, and it is fixed by
  running under systemd rather than by hand.
- `WARNING: this runner's user is in the "docker" group` — or `wheel`,
  or `libvirtd`. Stop here and use a user that is in none of them
  (D78). A job could leave every cap behind through any one of these.
- `kvm: present`, `dri: present` — what this machine will be given work
  for.

The last line is the refusal:

```
Error: the hub refused this runner: runner server is not approved on
this hub, so it is given no job: an operator approves it with `herdctl
runner approve server`, which mints the token this runner then carries
as --token or HERD_TOKEN
```

If instead it says it cannot reach the hub at all, the hub's URL or the
network is wrong, and no amount of approving will help.

### 4. Approve it, and take the token (the operator)

The hub does not keep a list of machines waiting to be let in, so tell
the operator the name you used. One command, at the hub:

```sh
herdctl runner approve server
```

The token goes to **stdout**, alone and once; the hub keeps only a hash
of it (invariant 7). The sentence explaining it goes to stderr, so
`token=$(herdctl runner approve server)` is all a script needs.

```
herdctl: runner server is approved on this hub. Its token is printed
once and this hub keeps only a hash of it: give it to that machine as
--token or HERD_TOKEN. Approving server again mints a new token and
revokes this one
```

Approving a runner that is already approved mints a new token and
revokes the old one. That is how a leaked token is replaced, and it is
also the way out if the token is lost between here and step 5.

Send it to the machine over something private. It is a credential.

### 5. Put the token where only this machine reads it

Never on a command line: `/proc` makes a command line readable by every
user of the host. Never in the repository (invariant 7). A file the
runner's user alone can read, outside any checkout:

```sh
sudo install -d -m 0700 /etc/herd
printf 'HERD_TOKEN=%s\n' '<the token from step 4>' |
  sudo tee /etc/herd/runner-token.env > /dev/null
sudo chown herd:herd /etc/herd/runner-token.env   # the runner's user
sudo chmod 0400 /etc/herd/runner-token.env
```

The service unit reads it with `EnvironmentFile=`. Running by hand,
`set -a; . /etc/herd/runner-token.env; set +a` does the same.

### 6. Start it, and keep it starting

A runner is a long-lived process started by the machine's owner (D5),
and it must come back with the host (E-52). Under systemd it also gets
the **delegated cgroup** that makes the memory ceiling kernel-enforced
rather than the fallback step 3 warned about.

**On a machine that is a server**, use the system unit. The data root is
made first, with the owner and the mode, because the unit no longer has a
`StateDirectory=` to make it (D126):

```sh
sudo install -d -o herd -g herd -m 0700 /mnt/ssd_980/herd-runner
sudo cp deploy/herd-runner.service /etc/systemd/system/
sudoedit /etc/systemd/system/herd-runner.service   # read every OWNER note
sudo systemctl daemon-reload
sudo systemctl enable --now herd-runner
systemctl status herd-runner
```

`deploy/herd-runner.service` carries every figure for the project's own
server — 31 GiB, 12 threads, no swap — with the reasoning for each one
beside it in a comment. On a different machine, change the figures and
nothing else. The ones to change are `--host-memory`,
`--host-memory-floor`, `--host-threads`, `--job-memory-max` and
`MemoryMax` — and `--data-root` with the `RequiresMountsFor=` that names
the same path.

**Your jobs' `PATH` is the unit's `PATH`, so read that line before step
7.** A runner builds each job's environment from scratch instead of
passing its own on — a variable of yours must never be able to redirect a
job's writes — and carries only `PATH`, `LANG`, `LC_ALL`, `TZ` and `TERM`
into it. systemd's default `PATH` has no shell on a NixOS host, so a job
as ordinary as `sh -c 'echo hi'` fails to exec and exits 125. The unit
states the value instead: `Environment=PATH=/run/current-system/sw/bin`,
which is right on NixOS and wrong everywhere else —
`/usr/local/bin:/usr/bin:/bin` is the Debian-shaped answer. Set it to
whatever holds your host's tools, and have jobs that need more bring
their own toolchain.

**On NixOS, this is one of two paths**, and a machine is on exactly one:
the copy above, with its OWNER notes done by hand, or
`deploy/nixos-herd-runner.nix` imported from `/etc/nixos`, which declares
the same unit plus the user, the data root and the devices. The copy is
what the project's server runs today, which means its `herd` user and its
unit are state outside the NixOS generation; the module is how that stops
being true, and importing it is the owner's call. The file and the module
must be changed together — both headers say what drifting costs.

**On your own desktop or laptop**, a user service is the right shape,
because stopping it must be yours alone:

```sh
mkdir -p ~/.config/systemd/user
cp deploy/herd-runner.service ~/.config/systemd/user/
# then, in your copy: delete User=, Group=, SupplementaryGroups=,
# RequiresMountsFor= and the [Install] section's WantedBy=, and set
#   --data-root ~/.local/state/herd-runner
#   Environment=PATH=... whatever holds your host's tools
#   WantedBy=default.target
systemctl --user daemon-reload
systemctl --user enable --now herd-runner
loginctl enable-linger "$USER"     # so it survives your logging out
```

Either way, check the one line that says the ceiling is real:

```sh
journalctl -u herd-runner -n 40 | grep 'memory ceiling'
```

It must say `one cgroup v2 subtree per job`. If it still says
**FALLING BACK**, the message names what is missing: `Delegate=yes` is
in the unit, so the usual cause is that the unit was moved into a
different slice and `--cgroup-root` no longer points at its own cgroup.
Compare the two:

```sh
systemctl show -p ControlGroup herd-runner
grep -o -- '--cgroup-root [^ ]*' /etc/systemd/system/herd-runner.service
```

### 7. Verify: one real job

Ask the operator for these two, or run them yourself if the hub is
yours. The second is the only proof that counts.

```sh
herdctl runner list
# server: approved 2026-10-15T17:38:28Z, connected now

herdctl run -- sh -c 'echo it works; nproc'
```

The job's output is preceded by one line naming where it ran and why:

```
ran on: server (claimed by server: labels none; it needs no capability;
admitted 0 MiB of memory, 14 GiB free of 14 GiB after it (20 GiB lent,
6 GiB kept as its own floor); it reserves no tmpfs; load 43.56 there,
recorded and not a gate (E-32); preference 0, the only eligible runner;
herd output format 1)
it works
12
```

Read the figures in that line back against what you set in step 6. If
they are not what you meant, the unit is what to fix.

The machine has joined.

## Taking the machine back

Yours, at any time, with no hub and no operator (E-33). The position is
a file under the data root, so it outlives both the command and a
reboot:

```sh
herd-runner --data-root ~/.local/state/herd-runner \
  switch off --reason "I want my machine back"
```

`off` withdraws it: nothing new is claimed, nothing is served to a peer,
and the jobs it already holds finish. `drain` is the same and then the
runner leaves once it holds nothing. `on` puts it back and takes no
reason. Honoured within ten seconds.

To stop the process altogether, `systemctl stop herd-runner` — or
`--user`, or `Ctrl-C` by hand. It drains and is gone inside five
seconds (invariant 2):

```
herd-runner: draining (the runner's owner asked it to stop (SIGTERM));
0 job(s) to end, no new job accepted
herd-runner: stopped
```

A verdict the hub has not acknowledged by then is written under the data
root and delivered on the next start (D34). Stopping loses nothing.

## When it does not work

| What you see | What it is |
| --- | --- |
| `runner <name> is not approved on this hub` | step 4 has not happened, or the name differs from the one approved |
| `the hub refused this runner` with a token complaint | the token in step 5 is not the one step 4 minted; approve again |
| `dialling ...: Connection refused` | the hub's URL or the network, not Herd. Nothing on your side listens, so nothing on your side can be the cause |
| `invalid peer certificate: UnknownIssuer`, or another TLS error | the certificate in front of the hub is not signed by a public CA. See the next section; there is no flag for it |
| `HTTP error: 302 Found` or `403 Forbidden` on a `wss://` dial | something in front of the hub wants a browser session or has banned the address. The hub's own refusals are sentences, never HTTP codes |
| `memory ceiling: FALLING BACK` | the runner is not in a delegated cgroup. Start it from systemd, not by hand |
| a job exits 125 with `sh: No such file or directory`, or a command inside a job is not found | the unit's `Environment=PATH=` is not right for this host. A job's `PATH` is the unit's, and systemd's default holds no shell on NixOS |
| `WARNING: ... is in the "docker" group` | use a user that is in no root-equivalent group (D78) |
| a data root that is refused, naming itself | the directory is not writable by the runner's user, or another runner already holds it (D32) |
| the unit will not start, and systemd names a mount rather than the runner | the data root's filesystem is not mounted. `RequiresMountsFor=` holds the service back on purpose, so it cannot build a root on a bare mount point |
| `herdctl runner list` shows it approved but not connected | the service is not running: `journalctl -u herd-runner` |

## Picking `ws://` or `wss://`, and the certificate that decides it

`wss://` works: the clients dial TLS with rustls and verify against the
webpki roots bundled in the binary (D115). The old `URL error: TLS
support not compiled in` is gone, and so is the advice that went with
it.

What has not been proven is one real dial through this project's proxy
and Cloudflare into the hub — `deploy/README.md`, "Step 4, exactly", is
the walk that proves it, and nobody has run it yet. So if you are the
first remote runner, expect to be the one who finds out, and read that
section before you start rather than after.

**The one thing that will stop you.** There is no trust-this-CA option
and no `--insecure`, on purpose (D115). If the certificate your runner
is shown is not signed by a public CA — a Cloudflare Origin CA
certificate, a private CA, a self-signed one, a proxy serving a fallback
certificate because nothing matched the hostname — the dial fails with a
TLS error and the hub never sees you. Nothing you can pass on the
command line changes that; the certificate in front of the hub has to
change.

Two ways to join that need no certificate at all, both keeping
invariant 3:

1. **On the hub's own host** — the project's server — use
   `ws://127.0.0.1:8787`, the loopback port docker compose publishes.
   Nothing crosses a network, so nothing needs TLS. This is what
   `deploy/herd-runner.service` does.
2. **On another machine**, reach the hub over the mesh the machine's
   owner already runs and dial plain `ws://` on the mesh address. The
   mesh is approved for exactly this (D11): no port on a router,
   nothing reachable from the internet. It needs the hub published on
   the mesh address instead of loopback, which is one line of
   `deploy/docker-compose.yml`, and the requester token set — without
   TLS the token is the only thing in front of the hub.

Whichever you pick, the scheme is spelled out at the call site and never
inferred (D14), so `--hub` says which of these you are doing.
