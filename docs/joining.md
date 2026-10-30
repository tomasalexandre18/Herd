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
| Nothing to build with | a release is a download; Nix only if you build from source (D12) |
| A name for the machine | `server`, `wsl-pc`; the operator approves it |
| The hub's URL | `ws://127.0.0.1:8787` on the hub's own host, else ask |
| 10 GB free, and somewhere to put a data root | one job's workspace |

Check the one that is not obvious:

```sh
stat -fc %T /sys/fs/cgroup       # must print: cgroup2fs
```

**If the machine is a Windows PC**, it joins inside WSL2 (D145).
Read **If the machine is a Windows PC: the WSL2 path**, after step 7,
before you start — it is the same seven steps and seven things that
differ.

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
v=v0.4.1
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
# server: approved 2026-10-15T17:38:28Z, connected now, herd 0.4.1

herdctl run -- sh -c 'echo it works; nproc'
```

The listing names the build this machine is running, and one indented line
under it for every capability it is offering and **not** handing out, with
the reason (D166). Lines like

```
  kvm is suspended: the probe `/etc/herd/kvm.sh` exited 0, which means busy
```

are not a failure of this step — a job that needs `kvm` waits rather than
being refused — but a capability suspended because its probe is broken
waits for ever, and nothing else in the listing would say so. Read them
before you read anything else.

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

## If the machine is a Windows PC: the WSL2 path

A Windows PC joins as a runner **inside WSL2**, and that is a ruling
rather than a convenience: the runner's entire safety story is cgroup v2,
which WSL2 has and Windows does not, so there is no native Windows runner
and will not be one (D145). The seven steps above are the steps this
machine walks as well. What follows is only what differs.

**Nobody on this project has a WSL2 host.** So every line below says
which of two things it is: read out of the code in this repository, or
**yours to confirm on the machine**, with the command that settles it and
the output to expect. A sentence that merely sounds right about WSL2 is
worth nothing here, and D27's rule — an assertion nobody could make is
named, never quietly assumed — is why none is written as fact.

Before anything else, run the one script that answers most of this at
once, inside the distribution the runner will live in:

```sh
./deploy/wsl2-check.sh ws://<the hub on the tailnet>:8787
```

It reads the machine and changes nothing that outlives it — one check
starts a transient systemd scope and removes one directory, and its own
header says so — then prints a block to paste back. `UNKNOWN` lines are
questions it could not answer; they are information, not faults.

### The binary comes from a release, and it goes in her own home

Step 1 above is already the release download, so the only difference here
is **where the binary lands**. A release carries three static musl binaries
that depend on no libc, no dynamic loader and no Nix store, and the
release workflow proves it by running a hub, a runner and two real jobs
out of those exact bytes in containers that have no Nix on them (D132).
Take `herd-runner`, verify it, and keep it in her
own home, because a user unit needs no root anywhere on this path:

```sh
sha256sum -c SHA256SUMS
install -D -m 0755 herd-runner-<version>-x86_64-unknown-linux-musl \
  ~/.local/bin/herd-runner
herd-runner --version
```

`deploy/release-notice.md` is the notice that ships with the release and
is the companion to this section: what a bare binary does and does not
have, the four things a runner needs, and the two gaps.

### systemd is not PID 1 until `/etc/wsl.conf` asks for it

WSL2 runs a real Linux kernel, but a distribution's init is Microsoft's
own unless it is told otherwise, and everything that gives this machine a
kernel-enforced ceiling hangs off systemd. The mechanism:

```ini
# /etc/wsl.conf
[boot]
systemd=true
```

and then, **from Windows** and not from inside the distribution, because
the setting is read when the distribution starts:

```
wsl --shutdown
```

Open the distribution again and check that it actually took, which is the
only part of this worth trusting:

```sh
ps -p 1 -o comm=            # must print: systemd
systemctl is-system-running # running, or degraded with a reason
```

**Yours to confirm, three things.** Whether this distribution already
carries that file — several images ship it, so the edit may be a no-op;
`cat /etc/wsl.conf`. Whether the WSL on this PC is new enough to honour
it at all; `wsl --version` from Windows, which also prints the kernel.
And whether `systemctl is-system-running` says `degraded`, which is
common and usually harmless but names the unit that failed —
`systemctl --failed` is the list, and the question is only whether it
matters to this runner.

### The memory ceiling, which is the one that matters

This is the whole reason M17 is careful. Q6 answers it with one cgroup v2
subtree carrying a ceiling, and the runner makes **one subtree per job**
under its own cgroup — which needs cgroup v2 **delegation**, the three
rights a systemd unit with `Delegate=yes` grants: create a cgroup, move a
process into it, and enable the memory controller for its children
(`herd-runner/src/cgroup.rs`).

The runner probes that at startup by doing it, and prints which of two
answers it got. These are its own strings, read out of
`herd-runner/src/confine.rs`, and the single line to look for:

```
herd-runner: memory ceiling: one cgroup v2 subtree per job, enforced by the kernel
```

That is the one you want. The other:

```
herd-runner: memory ceiling: FALLING BACK to a per-process address-space
limit, which bounds each process instead of the job's tree and makes an
allocation fail instead of killing the job. Reason: <why>
```

and, immediately under it, two more lines that come with the same
failure — the second is from this host, run bare, verbatim:

```
herd-runner: kill without a cgroup: FALLING BACK to killing the job's
process group and then sweeping /proc for processes still carrying this
job's marker. That reaches a job that gave itself a new session; it
cannot reach one that erases its own environment, which a cgroup kill
would still catch.

herd-runner: cpu.weight, cpu.max, io.weight and pids.max: NOT ENFORCED AT
ALL on this host: there is no cgroup for a job to carry them. A job here
is bounded by nice 10, the idle IO class and an address-space limit, and
nothing bounds how many processes it forks.
```

```sh
# whichever way it was started, this is how you read which one you got
systemctl --user status herd-runner | grep 'memory ceiling'
journalctl --user -u herd-runner | grep 'memory ceiling'
```

**The fallback is not good enough on this machine, and that is a
position, not a nicety.** `RLIMIT_AS` caps the address space of *each
process*, so a build that forks eight compilers holds the ceiling eight
times over; it makes an allocation *fail* rather than killing the job, so
a build that handles a failed allocation badly dies oddly instead of
coming back as exit 124; and with no cgroup there is no `pids.max`, so
nothing bounds a fork bomb. On a server one can argue about it. On
somebody's own PC — the machine she plays on, which is the whole reason
E-33 exists — a ceiling that can be exceeded eightfold is not invariant
2, and the honest thing is to fix the delegation rather than lend the
machine without it.

The `Reason:` is what says how to fix it. Two of its forms are the ones
to expect here, both read out of `cgroup.rs`:

```
cgroup v2 delegation is unavailable: this runner cannot create a cgroup
under <path> (Permission denied (os error 13)). Run it as a systemd unit
with Delegate=yes, or inside a delegated scope, and point --cgroup-root
inside that subtree.

the cgroup <path> does not offer the memory controller (it offers: <list>),
so no kernel memory ceiling can be set there
```

The first is a unit without `Delegate=yes` — the fix is the unit below.
The second is a different animal and would be the one piece of bad news
on this path: it means the memory controller does not reach where the
runner stands, and no `Delegate=yes` recovers it. `wsl2-check.sh` answers
that before the runner is ever installed: its `delegation` line prints
the controller list a `--user --scope -p Delegate=yes` really got, and
whether `memory` is among them.

#### The user unit, with the ceiling on it

A user unit is the right shape on her own PC — stopping it must be hers
alone — and it is also what carries `Delegate=yes`. The shape, with the
figures from the next section:

```ini
# ~/.config/systemd/user/herd-runner.service
[Unit]
Description=Herd runner (the owner's PC, inside WSL2)
Documentation=https://github.com/tomasalexandre18/Herd/blob/main/docs/joining.md
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
# A job's PATH is this unit's PATH and nothing else, so state it: on a
# Debian-shaped distribution this is the value, and systemd's default for
# a user unit is not it.
Environment=PATH=/usr/local/bin:/usr/bin:/bin
EnvironmentFile=-%h/.config/herd/runner-token.env
# The point of the unit, not a detail: this is what makes the ceiling a
# cgroup instead of an address-space limit.
Delegate=yes
# A job reaching its own memory.max is a cgroup OOM kill under this unit,
# and the default policy would take the runner down with the job.
OOMPolicy=continue
# The outer bound over everything Herd does on this PC (Q6).
MemoryAccounting=yes
MemoryMax=12G
MemorySwapMax=0
# Herd yields to her when the PC is contended.
CPUWeight=20
IOWeight=20
ExecStart=%h/.local/bin/herd-runner \
    --hub ws://<the hub on the tailnet>:8787 \
    --name wsl-pc \
    --data-root %h/.local/state/herd-runner \
    --capabilities %h/.config/herd/capabilities-wsl-pc.json \
    --host-memory 11GiB \
    --host-memory-floor 3GiB \
    --host-threads <half of nproc; see below> \
    --host-tmpfs 2GiB \
    --job-memory-max 8GiB \
    --job-timeout 5400 \
    --job-nice 10 \
    --drain-grace 3
KillMode=mixed
KillSignal=SIGTERM
TimeoutStopSec=10
Restart=always
RestartSec=5s

[Install]
WantedBy=default.target
```

```sh
install -d -m 0700 ~/.local/state/herd-runner ~/.config/herd
printf 'HERD_TOKEN=%s\n' '<the token from step 4>' \
  > ~/.config/herd/runner-token.env
chmod 0400 ~/.config/herd/runner-token.env
# the capability file is four lines (below): copy it across or type it
install -m 0444 capabilities-wsl-pc.json ~/.config/herd/
systemctl --user daemon-reload
systemctl --user enable --now herd-runner
loginctl enable-linger "$USER"      # so it survives her logging out
```

**Yours to confirm, and it is the load-bearing one.** `MemoryMax=` on a
*user* unit only bites if the memory controller is delegated down to
`user@.service`, which is the same question as the delegation probe: if
`wsl2-check.sh` reports a user scope whose controllers include `memory`,
both work; if it does not, then neither `MemoryMax=12G` nor the per-job
subtree does anything here, and the fallback is a **system** unit in
`/etc/systemd/system/` with `User=`, `Group=` and the same
`Delegate=yes` — `deploy/herd-runner.service` is that file, with a
comment on every figure. The cost of the system unit is only that
`systemctl stop` then needs `sudo`; it costs E-33 nothing, because her
switch is a file she writes and never a service she stops.

Two smaller ones, both read out of the code. `--cgroup-root` is left out
on purpose: with `Delegate=yes` the runner uses its own cgroup, which is
this unit's, and an *absent* root falls back loudly while an *explicit*
root that cannot be used refuses the startup (D33) — on a machine nobody
can log into remotely, loud is better than refused. And the data root
goes on the Linux filesystem, never under `/mnt/c`: that is a Windows
drive through a translation layer, and `stat -fc %T
~/.local/state/herd-runner` is how you check which one you are on. The
runner states in its own startup report whether an `io.weight` reaches
that filesystem at all, so this is checkable rather than assumed (D120).

### WSL2's own memory limit is a different ceiling, above ours

Two ceilings, and confusing them is how this gets wrong. `.wslconfig`'s
`memory=` is how much RAM **Windows gives the WSL2 virtual machine**;
`--host-memory` is how much of whatever the VM got **Herd may lend** to
jobs inside it. Ours sits strictly under theirs, and the figures only
make sense read as a chain from outside in:

| Ceiling | Where it is set | Who it binds |
| --- | --- | --- |
| the PC's RAM | the hardware | everything, Windows included |
| the WSL2 VM's RAM | `%USERPROFILE%\.wslconfig`, `[wsl2] memory=` | every distribution, and every process in them |
| Herd's whole subtree | `MemoryMax=` on the unit | the runner plus every job it runs (Q6, ~12 GiB) |
| what Herd says it lends | `--host-memory` | what the hub is allowed to admit here |
| what stays hers regardless | `--host-memory-floor` | subtracted before anything is admitted (E-29) |
| one job | `--job-memory-max` | the kernel ceiling on a single job |

**The number to look up is the PC's RAM**, because every figure below it
is a fraction of it and the 12 GiB of Q6 assumes a 32 GiB machine. From
Windows:

```
wmic computersystem get TotalPhysicalMemory
```

or, on a Windows where `wmic` is gone:

```
powershell -c "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"
```

And inside WSL2, what the VM actually got — which is the truth whatever
`.wslconfig` says, and which `wsl2-check.sh` prints:

```sh
grep MemTotal /proc/meminfo
nproc
```

A coherent set on a 32 GiB PC, which is what the unit above is written
for:

| Figure | Value | Why |
| --- | --- | --- |
| `.wslconfig` `memory=` | `16GB` | half the PC; Windows, her games and everything outside WSL2 keep the rest |
| `MemoryMax=` | `12G` | Q6's ceiling: Herd's whole subtree, kernel-enforced, inside a 15.6 GiB VM |
| `--host-memory` | `11GiB` | just under `MemoryMax`, so the runner announces the figure you wrote |
| `--host-memory-floor` | `3GiB` | what the first consumer asks of this PC (E-29), so at most 8 GiB is ever committed |
| `--job-memory-max` | `8GiB` | one build slot, so one job, and 8 GiB is what their build needs at `-j 4` |
| `--host-tmpfs` | `2GiB` | `/dev/shm` defaults to half the VM — about 7.8 GiB — and tmpfs pages are RAM inside the 12 GiB |
| `--host-threads` | half of `nproc` | the rest stays outside Herd, for Windows and for whoever is typing |

**If the PC has 16 GiB and not 32**, this whole chain halves and Q6's
figure has to be revisited rather than squeezed: a 12 GiB ceiling on a
16 GiB machine is not lending capacity, it is taking the machine. Say
which it is before the unit is written.

Two things to know about the figures once they are in. Writing
`--host-memory` *above* `MemoryMax` is not an error and not silent: the
runner walks up from its own cgroup, takes the tightest ceiling it finds
and announces that instead, saying so in one line (D119) —
`announcing 11.4 GiB of memory rather than 20 GiB, because …`. And
`.wslconfig` is read when the VM starts, so a change to it needs
`wsl --shutdown` exactly as `/etc/wsl.conf` does.

**Yours to confirm.** What WSL2 gives the VM when `.wslconfig` says
nothing at all: the rule has changed between WSL versions and the number
is not worth guessing, so read `MemTotal` and treat it as the answer. And
whether this Windows user has a `.wslconfig` at all —
`wsl2-check.sh` looks for one under `/mnt/c/Users/*` and prints it.

### Starting it with her session: a Windows logon task

Nothing in WSL2 starts with Windows. One scheduled task, no script:

| Field | Value |
| --- | --- |
| Trigger | At log on, her account |
| Action | Start a program |
| Program | `C:\Windows\System32\wsl.exe` |
| Arguments | `-d <distro> --exec /bin/true` |
| Settings | run only when she is logged on; hidden |

The task's whole job is to **start the distribution**. It deliberately
does not start the runner: a runner launched by `wsl.exe` directly would
be a bare process with no delegated cgroup and no ceiling — the exact
failure the section above is about. So the task runs for a moment and
exits, systemd inside the distribution starts `herd-runner.service`, and
`loginctl enable-linger` is what lets the user manager run with no Linux
shell open.

A logon trigger rather than a startup one, and that is not a preference:
WSL instances are per Windows user, so the distribution has to be started
as **her**. A task set to run whether or not she is logged on runs as
another identity and starts somebody else's WSL.

**Yours to confirm, and this is the part of the whole document nobody can
verify from here.** Log in, close every window, wait two minutes, then
from PowerShell:

```
wsl.exe -d <distro> -- systemctl --user --no-pager status herd-runner
```

Expect `Active: active (running)`. Four questions that answer decides, or
that stay open after it:

- whether the WSL2 VM keeps running after the launching `wsl.exe` has
  exited — the command above is the test;
- whether `loginctl enable-linger` behaves under WSL2 as it does on a
  normal host (`loginctl show-user "$USER" -p Linger` must print
  `Linger=yes`);
- whether the VM is shut down under her while idle, asleep or hibernated.
  Newer WSL has a `[wsl2] vmIdleTimeout`; if the runner keeps vanishing,
  that is the first thing to read. A runner that is stopped is not a
  failure — it drains, loses nothing, and comes back (D34) — but a PC
  that silently leaves the herd every afternoon is worth knowing about;
- whether the network is up when the task fires. Harmless either way: the
  runner redials with backoff and gives up only after
  `--give-up-after`.

### Her switch, and the ten seconds it takes

This is the control that makes lending a PC acceptable, so it is written
for **her** and needs no operator, no hub and no network.

The switch is a **file**, because nothing on this machine ever listens
(invariant 3). It lives under the data root and is called `switch`, so by
default:

```
~/.local/state/herd-runner/switch
```

She sets it with one command, which is also exactly what a desktop
shortcut can be:

```sh
herd-runner --data-root ~/.local/state/herd-runner \
  switch off --reason "I want my PC"
```

and the command's own answer, run on this host:

```
herd-runner: the switch in ~/.local/state/herd-runner/switch is now
off (I want my PC). A runner of this data root honours it within ten
seconds, whether or not a hub is up (E-33)
```

| Position | What it does |
| --- | --- |
| `off` | nothing new is claimed and nothing is served to a peer; a job already running finishes |
| `drain` | the same, and then the runner leaves once it holds nothing |
| `on` | back in the herd. It takes no `--reason`, and is refused with a sentence if given one |

`off` and `drain` **require** `--reason`: a machine withdrawn for no
stated reason is one nobody can decide to switch back on. Ten seconds is
the promise (E-33); the file is read every second by default, so in
practice it is the next second. The position outlives the command, the
runner process and a reboot, and it is honoured with the hub down —
which is when an owner is most likely to want her machine back.

Neither position stops the runner. `off` leaves the process up and
listening to nothing, which is what lets `on` put it back. To stop it
altogether, `systemctl --user stop herd-runner`: it drains and exits
inside five seconds (invariant 2), and a verdict the hub has not
acknowledged is written under the data root and delivered on the next
start (D34).

**The shortcut** — M17's own manual check is her flipping it from the
Windows desktop within ten seconds. Two shortcuts, and they differ by
one word:

```
C:\Windows\System32\wsl.exe -d <distro> -u <her Linux user> --
  /home/<her>/.local/bin/herd-runner
  --data-root /home/<her>/.local/state/herd-runner
  switch off --reason "I want my PC"
```

```
C:\Windows\System32\wsl.exe -d <distro> -u <her Linux user> --
  /home/<her>/.local/bin/herd-runner
  --data-root /home/<her>/.local/state/herd-runner
  switch on
```

(one line each, in the shortcut's Target field; broken here to fit the
page.)

**One trap, verified on this host rather than guessed.** `switch` writes
the file wherever `--data-root` points and **creates the directory if it
is not there**, reporting success either way — so a shortcut with a
typo'd path prints a cheerful sentence and changes nothing the runner
reads. Check a new shortcut once, the first time:

```sh
cat ~/.local/state/herd-runner/switch
# {"position":"off","reason":"I want my PC"}
```

and that the path in the shortcut is character-for-character the
`--data-root` in the unit.

**Yours to confirm.** Whether a Windows shortcut's Target field carries
the quoted `--reason` through to the Linux side intact — double-click it
once and read the file above; and whether a console window flashes, which
"Run: Minimized" in the shortcut's properties is the usual answer to.

### What this machine offers: one build slot, and no `kvm`

`deploy/capabilities-wsl-pc.json`, copied to
`~/.config/herd/capabilities-wsl-pc.json` and named by `--capabilities`
in the unit above:

```json
{
  "capabilities": [
    { "name": "build", "kind": { "type": "counted", "slots": 1 } }
  ],
  "deny": ["kvm"]
}
```

**One build slot**, because this is a PC somebody uses: a second
concurrent build is the difference between a machine that is quietly
lending capacity and a machine that is unusable. **`kvm` denied**, and a
denial wins over detection — if WSL2 exposes `/dev/kvm` with nested
virtualisation on, the runner still refuses to offer it, so no job that
asks for a VM is ever sent here (E-24).

The file is read strictly: an unknown key, or a key written twice,
refuses the startup naming the key and the capability. The runner says
what it resolved in its startup report, and the capability lines on this
host read:

```
herd-runner: build: declared
herd-runner: kvm: present, denied
```

The slot *count* is not echoed there — the proof that it is one is this
file plus the `ran on:` line of the first job, which names the slot it
took: `build slot 1 of 1`. On a machine where the host has no `/dev/kvm`
to detect, the second line is absent rather than different; the denial is
in force either way.

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

**Timed once, on 2026-10-30, and only the easy half.** Until that day
the five seconds was asserted and never measured against a running
runner. On our own queue runner (v0.4.1, one host, one version),
`kill -TERM` by PID to process exit was **0.212 s** — a 24× margin.
That runner was **idle**, and its own first line above says so:
`0 job(s) to end`. So what is measured is the stop path with nothing
to drain. **The case the bound exists for is still untimed**: a runner
asked to stop while a job is running has to end that job, take its
verdict and write it down, and no figure in this project covers that
yet. Read 0.212 s as the floor of the stop path, not as the answer for
a runner draining a three-hour build.

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
| jobs needing a label wait for ever on a connected machine | `herdctl runner list` prints an indented line for every capability that machine is not handing out, with the reason (D166). A probe that cannot be read suspends its capability, and the wait is then endless |

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

## What protects a pack between runners, and what does not

**None of this is built yet, and the order is deliberate.** A job's
declared artefacts are meant to move runner to runner across your mesh,
with the hub as registry and authority and never in the byte path (D10,
D11) — and none of it exists. No pack frame is numbered on either
protocol enum, no fetch endpoint binds anything, and the word `keeper`
appears nowhere in the binaries you installed at step 1. If you go
looking in `herd-runner` for a transfer to inspect, you will find none;
M11 builds it (`docs/plan.md`). What follows is the property the design
is being built to have, written down before the first frame was numbered
(Alexa, 2026-10-29), so you can judge your mesh now instead of after the
transfers start.

**Herd gives authorisation. It does not give confidentiality.** Those
are two promises, and only the first one is ours:

| What | Who provides it | What it means |
| --- | --- | --- |
| Authorisation | Herd | only a runner the hub told to fetch this pack can fetch it |
| Confidentiality | you, through your mesh | whether the bytes are unreadable on the way |

**The right to fetch a pack is an ephemeral capability.** When a runner
needs a pack, the hub mints a random bearer token and pushes it to the
runner that holds the pack, over that holder's own control link — the
one it already authenticated with the token of step 5. The message says:
expect this token, for this pack, from this peer, for the next few
minutes. The fetching runner presents the token, and the holder serves
those bytes and nothing else. A fetch carrying no valid capability is
refused.

There is nothing in that for you to distribute and nothing to renew on a
calendar: no key, no certificate, no CA, no PKI. Nothing secret lives in
Herd's repository either (invariant 7), and the only secret on your
machine is still the approval token you placed at step 5.

**What the capability buys, which is not nothing:** a machine the hub
never authorised cannot fetch a pack, even when it can reach the serving
runner's address and knows exactly what to ask for. It holds no token,
and no one will serve it one.

**Herd adds no TLS of its own, on purpose.** TLS on the peer path does
not stop at wrapping a socket. It asks who signs, who verifies, how a
key reaches each machine, and what happens the day one expires — a whole
trust-distribution subsystem, built to answer "someone is already inside
your private network". Herd stays specialized and grows no such surface
(invariant 1). The mesh you already run answers that question better,
because you chose it, you operate it, and it is already what protects
everything else those machines say to each other.

**So, the residue, stated here rather than left for you to infer from
the code later.** If your mesh does not encrypt what crosses it, then
whoever can read the mesh can do two things:

- **read pack bytes in clear.** A pack is your build and test
  artefacts; treat them as readable by anyone who can read the mesh.
- **reuse a capability for as long as it lives.** It is a bearer token,
  so it proves nothing about who presents it. Someone who reads one off
  the wire can present it as their own until it expires — minutes, not
  hours, but not zero either.

That is the honest cost of the choice, and it leaves you one question:
**is your mesh good enough to carry your artefacts in clear?** For a
tailnet, a WireGuard network, or anything else that encrypts peer to
peer, yes. For a flat LAN, a shared VLAN, or a mesh somebody else
operates, no — and then the thing to fix is the mesh, not a Herd flag,
because there will not be one.

**None of this opens a port.** The rule is the one at the top of this
document, under **What you never need**, and a pack path does not bend
it: the fetch endpoint, once it is built, binds the single mesh
interface you name in the configuration and is refused at startup if it
would bind a default or a public one, control is still the runner
dialling out, and nothing of this ever sits behind a port forwarded on a
router (invariant 3, D11).
