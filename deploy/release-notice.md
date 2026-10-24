# What you downloaded, and what to do with it

This is the notice that comes with a Herd release. It is for the person
deploying on a machine nobody in this repository will ever see, so it says
what you will actually hit rather than what would be nice. Every claim here
was either read out of the code it describes or measured on a running
deployment, and the ones nobody has checked are named as such.

For the full walk — one machine, one person, under thirty minutes — read
`docs/joining.md`. This notice is what that walk assumes you already know.

## The files

| File | What it is |
| --- | --- |
| `herd-runner-<version>-x86_64-unknown-linux-musl` | the client that offers a machine's capacity and runs jobs |
| `herdctl-<version>-x86_64-unknown-linux-musl` | the CLI: submit a job, follow it, approve a runner |
| `herd-hub-<version>-x86_64-unknown-linux-musl` | the service. You probably do not need this one: the hub is deployed as a container image, attached to this same release, and there is one hub. See "the hub is an image" below |
| `herd-hub-image-<version>.tar.gz` | that container image, as an archive to `docker load`. About 19 MB. See "the hub is an image" below |
| `SHA256SUMS` | so you can verify the four rather than trust a URL. Add `--ignore-missing` if you did not download them all |

**x86_64 Linux only**, and statically linked against musl: they depend on no
libc, no dynamic loader and no Nix store, and they run on any distribution —
Arch, Debian, Alpine, WSL2. That is not a claim made from reading: the
release workflow runs a real hub, a real runner and two real jobs out of
these exact binaries inside an `archlinux` container and an `alpine` one,
both of which have no Nix on them, and fails if any of it fails
(`scripts/release-smoke.sh`). There is no aarch64 build because every machine
in the herd today is x86_64; ask if yours is not.

**Upgrading from 0.2.x is a fleet-wide step, not a per-machine one.** The
protocol went from version 1 to version 2 in 0.3.0, and a version-2 hub
refuses a version-1 `herdctl` or runner **by name** — it tells you rather
than misbehaving, which is still a hard stop for anything with no fallback.
So: put the new binaries on every machine first and restart nothing; then
restart the hub and its runners together; then the rest. Submit nothing in
between, and keep the window short. Within a major version the two sides may
differ by patch — a runner says so at startup and serves the connection
anyway — but 1 and 2 are not that.

```sh
sha256sum --ignore-missing -c SHA256SUMS
install -m 0755 herd-runner-<version>-x86_64-unknown-linux-musl \
  /usr/local/bin/herd-runner
herd-runner --version
```

## The two numbers in the release body

The release body that quotes this notice prints `RECORD_VERSION` and
`PROTOCOL_VERSION`, read out of the source that release was built from, each
on a line a script can match. They answer the two questions no binary here
will tell you.

**`RECORD_VERSION` is the hub's state volume, and therefore rollback.** Every
file the hub writes under its data root carries that number, and a hub reads
only a state root whose number is **exactly** its own; anything else is
refused by name — the path, the root's number, the hub's — and because the
refusal comes out of recovery the hub does not start rather than guess. That
cuts both ways: if this number differs from the one on the release you are
running, **neither the upgrade nor the rollback across it can read your state
root, and there is no offline migration tool**. Crossing it means starting on
a fresh root, leaving every queued job and every uncollected verdict behind
in the old one. So compare the two releases' numbers before you roll an image
back; equal numbers mean a rollback is another `docker load` and nothing
more.

**`PROTOCOL_VERSION` is whether two builds can talk.** A hub refuses a runner
or a `herdctl` announcing a different protocol version **by name**, naming
both numbers, and that refusal is terminal — nothing but a new build changes
it. A patch-level difference in *build* version at the same protocol version
is only a warning, printed at startup, and the connection is served. So a
protocol move is a fleet-wide step, as "Upgrading" above describes, and never
one machine at a time.

## The hub is an image, and it is attached to this release

The three binaries above are not the whole release. **The hub is normally
not one of them**: it is a container image, attached here as
`herd-hub-image-<version>.tar.gz` and built by the same workflow on the same
tag, out of the same Nix derivation that has ever built it
(`nix/hub-image.nix`, D153). There is no Dockerfile and no second build, so
the hub a release deploys is the hub a release tested — the workflow loads
this very archive and runs a hub, a runner and two real jobs through it
before the release exists.

It is about 19 MB, and `SHA256SUMS` above covers it along with the binaries.
Deploying it is a load and an up:

```sh
sha256sum --ignore-missing -c SHA256SUMS
docker load < herd-hub-image-<version>.tar.gz
#   Loaded image: herd-hub:<version>
docker compose up -d
```

`deploy/docker-compose.yml` takes the image from `HERD_HUB_IMAGE` in your
`.env`, and that is the tag `docker load` just printed;
`deploy/env.example` says what to put and `deploy/README.md` walks the first
start, including the two ownership failures it actually hits. x86_64 only,
for the reason the binaries are.

**There is no registry behind that tag, and so no `docker compose pull`.**
The tag is local to the machine that loaded it, and a pull fails on it,
which is correct. Upgrading is the four lines above again with the next
release's archive. Putting the image in a registry of your own is one
`skopeo copy` out of this archive — `flake.nix`'s `release` shell has the
tool and the command — and is deliberately not part of cutting a release:
Herd published a registry step inside the release path once, and the three
tags v0.3.1, v0.3.2 and v0.3.3 produced no release at all because of it
(D162). An optional publication never gets a veto over the artefacts again.

**There is no `latest` tag and there will not be one.** A hub is a single
instance that a whole fleet speaks to, and the protocol version is part of
what it speaks: 0.3.0 moved it from 1 to 2, and a version-2 hub refuses a
version-1 runner by name. A floating tag means the next `up -d` — a reboot,
a host upgrade — can land the fleet on a different protocol without anybody
choosing the moment. Naming the version is how that stays a decision, and a
loaded tag is pinned by construction: nothing can move it but another
`docker load`.

**Nothing about this release needs a credential.** Not a registry account,
not a token, not a repository secret or variable. Everything a tag publishes
is built from the flake and attached by GitHub's own token, which is why a
tag now either produces all four files or fails visibly while producing
none of them.

## A binary and a service are not the same thing

Run the binary bare and the runner works: it reads the host, dials the hub,
joins, claims jobs, executes them and returns verdicts. Nothing is broken and
nothing is pretending. What it does **not** have is most of the reason this
project puts a `systemd` unit in front of it — and the binary says so itself,
in capitals, in the report it prints at startup. These are its own words, from
a bare run:

```
memory ceiling: FALLING BACK to a per-process address-space limit, which
bounds each process instead of the job's tree and makes an allocation fail
instead of killing the job. Reason: cgroup v2 delegation is unavailable […]
Run it as a systemd unit with Delegate=yes, or inside a delegated scope, and
point --cgroup-root inside that subtree.

cpu.weight, cpu.max, io.weight and pids.max: NOT ENFORCED AT ALL on this
host: there is no cgroup for a job to carry them. A job here is bounded by
nice 10, the idle IO class and an address-space limit, and nothing bounds how
many processes it forks.
```

Read that report every time you start a runner a new way. It is the only
thing that knows what your host actually granted.

| | Bare binary | Under a unit with `Delegate=yes` |
| --- | --- | --- |
| per-job memory ceiling | `RLIMIT_AS`, which caps the address space of **each process** and makes an allocation *fail*. A job that forks ten processes gets the ceiling ten times over, and a build that handles a failed allocation badly dies oddly instead of being killed | `memory.max` on a cgroup of the job's own: the kernel refuses the allocation and kills **that job's whole tree**, and nothing else (D23) |
| `cpu.weight`, `io.weight`, `pids.max` | not enforced at all — there is no cgroup to write them into. A job gets `nice` and the idle IO class, and may fork without bound | written into the job's cgroup, and read back out of the kernel on the project's own server: `cpu.weight 10`, `io.weight 10`, `pids.max 1024` (D129) |
| killing a job (a cap, a cancel, a drain) | the job's process group, then a sweep of `/proc` for processes still carrying the job's marker. That reaches a job that gave itself a new session; it does **not** reach one that erases its own environment | one `cgroup.kill`: every descendant, whatever it did to its session or its environment |
| back after a reboot | no. You started it; nothing starts it again | `Restart=always` and `WantedBy=multi-user.target` (E-52) |
| the token | `--token` on the command line is readable out of `/proc` by every user of the host | `EnvironmentFile=` a file mode 0400 owned by the runner's user (invariant 7) |
| a job's `PATH` | **your shell's** `PATH`, inherited by the runner and carried into the job — which is exactly why a bare run works | the **unit's** `PATH`, and systemd's default is not usable on every host. See below; this is the one row where the unit is the thing that can go wrong |
| an outer bound on all of Herd | none. The per-job ceilings are all there is | `MemoryMax=` over the whole subtree, which the runner reads at startup and admits against, announcing it instead of the host's memory when it is lower (D119) |

So: bare is fine for a probe, a one-off, or finding out whether this machine
can join at all. For a machine that is actually in the herd, write the unit.
`deploy/herd-runner.service` is a complete one with the reasoning for every
figure beside it; copy it and change the figures.

Two more things the binary does on its own, with or without a unit:

- **it refuses to start as root.** Not a warning — a refusal, before it
  dials, because a job must never be able to inherit root.
- **it warns, by name, if its user is in `docker`, `wheel`, `libvirtd` or
  `sudo`, or can write a container or VM runtime socket.** Any one of those
  is root in all but name, and a job could leave every cap behind through it.
  Read those lines. On the project's own server one of them found a
  world-writable `libvirt-sock` nobody was looking for (D129).

## The four things a runner needs

Nothing else is required.

| | Flag | Note |
| --- | --- | --- |
| the hub's URL | `--hub ws://…` | the scheme is spelled out and never inferred (D14) |
| a name | `--name wsl-pc` | agree it with the operator before step 4; it is what gets approved |
| a token | `HERD_TOKEN` | minted by the operator, below. Through the environment, never on the command line |
| a data root | `--data-root` | the **one** directory this runner may write in (E-45). It defaults to `~/.local/state/herd-runner`, which is a real default and not a placeholder — but read the filesystem section before you accept it |

A runner with no token still does something useful: it reads the host, prints
the report, dials, and is refused **by name**, with the sentence the operator
needs. That is the recommended first run.

## The token: `herdctl runner approve <name>`

On the hub, by whoever operates it:

```sh
herdctl runner approve wsl-pc
```

The token goes to **stdout, alone and once**; the hub keeps only a hash of
it. The sentence explaining it goes to stderr, so

```sh
token=$(herdctl runner approve wsl-pc)
```

is all a script needs — verified, not assumed. **Approving a name that is
already approved mints a new token and revokes the old one**, which is both
how a leaked token is replaced and how you recover when the token is lost
between the hub and the machine. A runner carrying a revoked token is refused
by name and claims nothing.

Put it where only this machine's runner can read it:

```sh
sudo install -d -m 0700 /etc/herd
printf 'HERD_TOKEN=%s\n' "$token" | sudo tee /etc/herd/runner-token.env >/dev/null
sudo chown herd:herd /etc/herd/runner-token.env
sudo chmod 0400 /etc/herd/runner-token.env
```

## Is the fleet working? `herdctl runner list`

One command, and it answers four questions: which machines are approved,
which of them are here, which build each one runs, and — the one that
matters most — which of its capabilities it is **not** handing out, with
the reason.

```
$ herdctl runner list
server: approved 2026-10-01T09:11:12Z, connected now, herd 0.3.7
  kvm is suspended: the probe `/etc/herd/kvm.sh` exited 0, which means busy
  gpu-venus is suspended: gpu-venus has a busy probe (command /etc/herd/gpu.sh) and server has not reported a reading of it yet
wsl-pc: approved 2026-10-02T18:40:03Z, not connected
```

A suspended capability is not a refusal: jobs that need it **wait**, with
the same reason on their own record (`herdctl wait <id>` prints it). But a
capability suspended because its probe is broken waits for ever, and the
fleet looks healthy from every other angle — so this is the line to read
first when a job is not starting. A machine that is not connected has told
this hub nothing, so it carries neither a build nor a list: that is the
absence of an answer and not a clean bill of health.

`herdctl switch` says the same two things under each runner's switch
position, for whoever is already looking there.

## The data root's filesystem decides whether IO is bounded at all

This is not a detail and it cannot be fixed later by a flag. A job's IO is
weighted per cgroup only on a single block device the kernel can cost — ext4
or xfs on one disk, where `blk-iocost` or `bfq` is in play. **On ZFS no cgroup
IO knob reaches it, ever**: ZFS does its pool IO from its own threads and
attributes none of it to the cgroup that asked, so neither `io.weight` nor
`io.max` does anything at any value (D120). The project's server puts its data
root on an NVMe for exactly this reason (D126).

Check before you choose, and then check what the runner says it got:

```sh
stat -f -c %T ~/.local/state/herd-runner      # ext4, xfs, zfs, overlay…
journalctl -u herd-runner | grep 'io.weight'
```

The runner states which of the two it is in one line, so you never have to
guess. On a host where it bites:

```
io.weight 10: enforced for jobs here, because /mnt/ssd_980/herd-runner is on
/mnt/ssd_980 (device 259:0), where blk-iocost is enabled for it […], so the
kernel weights this job's IO by the number written.
```

On ZFS it says instead that no cgroup IO knob reaches a ZFS dataset, at any
value. If that is your machine, the honest position is that Herd bounds this
job's memory, CPU share, process count, wall clock, log size and disk usage,
and does not bound its IO. Decide whether that is acceptable on your host
rather than discovering it under load.

One more thing about the root: **one runner per data root.** A second runner
pointed at the same directory is refused, naming the first one's pid (D32).

## A job's `PATH` comes from the unit

A runner builds each job's environment **from scratch** rather than passing
its own on — its own environment holds the hub token, and a stray variable
must never be able to redirect a job's writes. It carries exactly five
variables through: `PATH`, `LANG`, `LC_ALL`, `TZ`, `TERM`.

So under systemd, your jobs' `PATH` is the unit's `PATH` and nothing else.
systemd's default for a system service is
`/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`, which is
right on a Debian-shaped host and nearly empty on NixOS. Forget the line and
every job fails the same way (D129, found on the first real job the project's
own server ran):

```
sh: No such file or directory (os error 2)
```

exit **125**, on every job, including `sh -c 'echo hi'`. Inside a job even
`cat` was not found. The fix is one directive, and the right value is your
host's, which is why it is yours to state and not Herd's to guess:

```ini
Environment=PATH=/run/current-system/sw/bin        # NixOS
Environment=PATH=/usr/local/bin:/usr/bin:/bin      # Debian-shaped
```

A job needing more than the host offers brings its own toolchain and extends
`PATH` itself. Three things that need no variable at all, each checked rather
than assumed: TLS trust (the bundle is where `curl` and `git` look by
default), the timezone (`/etc/localtime` is read with `TZ` unset), and `nix`
(it finds its daemon with an empty environment).

## It listens on nothing

The runner dials **out** to the hub and needs nothing inbound: no port opened
on a router, no port forwarded, nothing reachable from the internet
(invariant 3). Not an intention — measured on the running deployment:

```
$ ss -tnap | grep herd-runner
ESTAB 0 0 127.0.0.1:44818 127.0.0.1:8787 users:(("herd-runner",pid=…,fd=10))
```

One outbound connection, and **zero** listening sockets. If a firewall rule
or a router forward was ever added for Herd, it was not needed and should be
removed.

(A future milestone, M11, lets runners serve content to each other. When it
arrives a runner may listen on a **private mesh interface** — bound to that
interface only, never a default or public one, and still never behind a port
forwarded on anybody's box.)

## Two gaps you will hit

Both are real today. Neither is a bug to report.

**Herd returns an exit code and logs. It does not return files.** A job's
workspace lives under the data root — `jobs/<job>/work`, `home`, `tmp`, and
for a declared command `in` and `out` as well — and it is **reclaimed when
the job ends**. So a job that writes `results.json` into `{out}` produces a
file that does not come home; the directories exist and are empty until M11
fills them. If you need something out of a job today, print it: the log comes
back. Do not build a workflow on artefacts yet.

**`wss://` is configured and has never been dialled for real.** The clients
do TLS with rustls against the webpki roots compiled into the binary (D115),
and that path is exercised by tests — but one real dial through this
project's reverse proxy into the hub has not been done by anybody. There is
also deliberately **no `--insecure` and no trust-this-CA option**: if the
certificate in front of the hub is not signed by a public CA — a Cloudflare
Origin CA certificate, a private CA, a self-signed one, a proxy serving a
fallback certificate — the dial fails and nothing on your command line can
change that. The certificate has to change.

The two paths proven today, both of which keep invariant 3:

1. **on the hub's own host**, `ws://127.0.0.1:8787` — the loopback port the
   compose file publishes. Nothing crosses a network, so nothing needs TLS.
2. **on another machine**, plain `ws://` over a private mesh the machine's
   owner already runs (D11). It needs the hub published on the mesh address
   instead of loopback, and the requester token set — without TLS that token
   is the only thing in front of the hub.

If you are the first remote runner over `wss://`, expect to be the one who
finds out, and say so when it breaks.

## The exit codes that mean something

`herdctl` exits with the job's verdict, so a script can read it directly.

| Code | What it means |
| --- | --- |
| the job's own | worn untouched — `0`, `1`, `75`, anything the process really exited with. This is the whole point: a test suite's own code reaches the caller |
| `124` | a runner stopped the job at one of its caps: time, memory, log size or disk budget. `timeout(1)`'s code, generalised. Which cap it was is printed on stderr and carried in the record |
| `125` | **Herd has no verdict.** It could not run the job, lost the runner, refused it, could not be reached at all, or the machine it ran on could not do its own part (`setup_failed` — a workspace the runner was refused access to, say, which is a host to fix and never a build that failed). Never a failing build — a script that reads 125 as "the tests failed" is the one mistake that cannot be survived (D20). The reason is printed on stderr |
| `128 + n` | the job was killed by signal `n`, exactly as a shell reports one |

A job *can* exit 124 or 125 by itself, which is why the reason is always on
stderr as well, and why the wire carries a typed outcome rather than a
number. Read the sentence, not just the code.

### For an agent: `--verdict-to`, and no sentence to parse

`herdctl run --verdict-to <FILE>` and `herdctl wait --verdict-to <FILE>`
write the job's end as JSON. The fields that answer what an exit code
cannot:

| Field | What it says |
| --- | --- |
| `cause` | one word: `exited`, `signalled`, `stopped_by_cap`, `could_not_start`, `setup_failed`, `cancelled`, `lost`, `impossible` — or `no_verdict` when Herd never got one |
| `exit_code_from` | `job`, `signal` or `herd`. **This is what tells a job's own 124 or 125 from Herd's.** A build that chose to exit 124 is `{"exit_code": 124, "exit_code_from": "job"}`; a cap is `{"exit_code": 124, "exit_code_from": "herd"}`, with `cause` saying which |
| `job_exists` | `a_job`, `nothing` or `unknown` — the three stderr lines below, as a field |
| `ran_on` | `{"runner": …, "why": …}`: which machine ran it and why there |
| `cap` | which cap and its figures, when a cap ended the job. A non-null `detail` means the job did **not** reach `limit` |
| `passed`, `job_id`, `said`, `outcome`, `metrics` | the verdict in one boolean, the id, the sentences a person would have read, the outcome as the wire carries it, and what the runner measured |

Three things about the file:

* **Every field in the table above is always present.** One with nothing in
  it is `null`, so a reader indexes the document instead of probing it.
  (Inside `outcome` and `metrics` the wire's own habits apply: a figure
  nobody could measure is absent rather than a zero that reads as one.) The
  first field is `verdict_format_version`; check it before anything else.
* **It is written however the run ends**, no verdict included, so a stale
  file from an earlier run is never read as this run's answer.
* **It is a file, not a stream.** `stdout` belongs to the job and `stderr`
  carries the prose above. For a pipe, name one: `--verdict-to /dev/fd/3`.

```sh
herdctl run --verdict-to v.json --command build --commit "$sha"
jq -r 'if .passed then "ok" else "\(.cause) on \(.ran_on.runner // "nowhere")" end' v.json
```

### Falling back after 125: the id is the answer, not the code

125 is one code for two situations that call for **opposite** actions: a job
that exists and may still be running, and a submission that never left. Run
the first again and the work is done twice; decline to run the second and it
is never done at all. There is no spare exit code to split them with — 0‑255
belongs to the job, 124 is the caps, 126 and 127 are what a *shell* returns
for "not executable" and "not found", and 128+n is a signal — so the
discriminator is on **stderr**, which a job's own output never shares:

| Line on stderr | What it means | What to do |
| --- | --- | --- |
| `herdctl: job-id: <id>` | a job exists, and that is its id | **do not submit again.** `herdctl wait <id>` picks it back up, output, verdict and all |
| `herdctl: no-job: …` | nothing was submitted | running the work again is safe and starts no second job |
| `herdctl: job-unknown: …` | the submission left and was never acknowledged, so a job may exist whose id nobody learned | submitting the same work again **joins** that job rather than starting a second one (E-05) |

Three rules about that line, and they are the contract:

* **The id is printed the moment the hub says it**, before anything can go
  wrong — so it is already out when the link dies mid-run, when the hub goes
  away, when a cancel races the verdict, and when the wrapper itself gives up
  and kills `herdctl`.
* **The test is the presence of an id, never the wording of a sentence.** The
  prefixes above are stable; the prose after them is not.
* **None of the three** means `herdctl` died before it could say. Treat that
  as `job-unknown`.

```sh
out=$(herdctl run --command build --commit "$sha" 2>&1 >/dev/null) || {
  id=$(printf '%s\n' "$out" | sed -n 's/^herdctl: job-id: //p' | tail -1)
  if [ -n "$id" ]; then herdctl wait "$id"; else run_it_locally; fi
}
```

`herdctl --help` carries the same thing, for whoever is at a terminal.

## Taking the machine back

Yours at any time, needing neither the hub nor the operator (E-33). The
position is a file under the data root, so it outlives both the command and a
reboot:

```sh
herd-runner --data-root ~/.local/state/herd-runner \
  switch off --reason "I want my machine back"
```

`off` claims nothing new and lets what is running finish; `drain` is the same
and then the runner leaves; `on` puts it back. Honoured within ten seconds.
`systemctl stop herd-runner` drains and exits inside five seconds
(invariant 2), and a verdict the hub has not acknowledged by then is written
under the data root and delivered on the next start (D34). Stopping loses
nothing.
