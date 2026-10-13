# Runner confinement (M4)

What a runner enforces against every job, how it is configured, and what it
does on a host that cannot give it the real mechanism. Reference, not a
journal: it changes when the behaviour changes.

Expectations closed here: E-30 (kernel memory ceiling), E-34 (niced, no
inbound, disk budget), E-45 (one data root), E-46 (unprivileged), and E-15's
resource fields.

## Configuration surface

Every value is a path, a number or a list of paths. There is no policy
language and no plugin, by design (invariant 1). Each flag has an environment
variable, in the style of `--hub` / `HERD_HUB_URL`.

| Flag | Environment | Default | What |
|---|---|---|---|
| `--data-root` | `HERD_DATA_ROOT` | `$XDG_STATE_HOME/herd-runner`, else `~/.local/state/herd-runner` | The one directory this runner may write in |
| `--job-memory-max` | `HERD_JOB_MEMORY_MAX` | `2GiB` | Kernel memory ceiling per job. Refused below `16MiB` |
| `--job-timeout` | `HERD_JOB_TIMEOUT` | `3600` | Time cap per job, in seconds |
| `--job-log-cap` | `HERD_JOB_LOG_CAP` | `64MiB` | Log one job may produce |
| `--job-disk-budget` | `HERD_JOB_DISK_BUDGET` | `20GiB` | Workspace one job may hold |
| `--job-nice` | `HERD_JOB_NICE` | `10` | CPU niceness floor for a job |
| `--cgroup-root` | `HERD_CGROUP_ROOT` | the runner's own cgroup | Delegated cgroup v2 directory to hang job cgroups under. Must contain this runner |
| `--privileged-helper` | `HERD_PRIVILEGED_HELPERS` | empty | Absolute path of a command allowed to need privilege; repeat the flag, or separate with `:` |

Sizes accept a plain number of bytes or a suffix: `B`, `KiB`, `MiB`, `GiB`,
`TiB`, and the decimal `kB`, `MB`, `GB`, `TB`. A value that does not parse is
refused at startup naming itself.

A cap of `0` is refused, and so is a cap too small to be a cap at all: a
memory ceiling under **16 MiB** refuses every job exactly as `0` does, since
`/bin/sh` alone needs that much address space under the `RLIMIT_AS` fallback.
`--job-memory-max 1kB` used to be accepted. Invariant 2 asks for a cap, not
for a wall, and the refusal names the floor.

A runner prints what it will enforce on its own stderr before it dials the
hub, so that the answer to "what is this machine doing to my job" never has
to be inferred from a job that behaved oddly. That report also carries, in
capitals, every fallback it has had to take and every way this deployment is
less confined than the expectations ask.

## The data root (E-45)

One directory, validated at startup: created if absent, and refused — every
refusal naming the path — if the path exists and is not a directory, if it is
a symlink to something that is not a directory, if it is not writable, or if
it is the filesystem root. The "not a directory" case is asked *before* the
directory is created: `create_dir_all` answered a regular file with "cannot
be created: File exists", which named neither the problem nor the fix and
left the `is_dir` refusal unreachable for the commonest way of getting this
wrong.

Inside it, `jobs/<job>/` holds `work` (the job's working directory), `home`
(its `HOME`) and `tmp` (its `TMPDIR`), and `held-results/` holds a verdict
the hub never acknowledged — deliberately outside `jobs/`, so the startup
reaper cannot clear it (D34, and "A runner's own death" below).

**One runner per data root.** A runner takes an exclusive lock on
`<root>/.herd-runner.lock` and refuses to start if another runner holds it,
naming that runner's pid. The lock dies with the process, so a runner that
was killed leaves the root free. This is what gives "left behind" a meaning
for the reaper below, and it is the same choice D21 made for the hub's state
root.

**A job cannot blind the runner about its own workspace.** A job runs as the
runner's own user, so it can `chmod 000` a directory it made. Every walk the
runner does over a workspace therefore restores the access it needs first —
those directories are its own — so:

- the disk budget is measured on what the job wrote, not on what it let the
  runner see;
- a figure that still cannot be taken is **absent**, and the job ends at its
  disk cap: a budget that cannot be measured cannot be enforced, and an
  unenforced cap must never read as a job that passed. The outcome then
  carries **why** as well as the cap's figure, because the figure alone lies:
  a job that wrote four kilobytes was being told "the runner stopped this job
  at its disk cap of 21474836480 bytes", with the real reason only on the
  runner's stderr. `stopped_by_cap` has a `detail` field for exactly this,
  absent whenever the cap's own figure says it all;
- the workspace is reclaimed when the job ends, whatever its modes are, and
  if a reclaim still fails that is said in the **job's own record** — a line
  on the job's stderr the requester reads — and not only in the runner's log,
  because bytes left on a host nobody collects are exactly what invariant 2
  forbids. **This holds on both paths**: the job's own end, and the drain's
  backstop when a runner is stopped before a job's task got that far. The
  backstop used to say `ITS WORKSPACE LEFT BEHIND` on the runner's stderr and
  nowhere a requester could read.

The job's environment is built from scratch rather than inherited: `HOME`,
`TMPDIR`, `TMP`, `TEMP` and the `XDG_*` directories all point inside the
workspace, and only `PATH`, `LANG`, `LC_ALL`, `TZ` and `TERM` are carried
through. So nothing in the runner's own environment — a token, a `TMPDIR` on
the host's disk — reaches a job. One variable is Herd's own: `HERD_JOB_MARKER`
carries the job's unique tag, which is how the runner finds the job's
processes again (see the kill, below).

**The limit, stated plainly.** Herd writes nowhere but the data root, and a
job is given nowhere else to write. A job that names an absolute path outside
the root is *not* stopped by M4: that needs a mount namespace, which is not
built here. The declared command contract (M10) is where a job stops choosing
its own paths.

## Unprivileged (E-46)

A runner refuses to start if its real or effective user id is 0, and says so.
It invokes no escalation tool; a test walks the crate's own sources and fails
if any module but `privilege.rs` so much as names one.

Anything that ever needs privilege must go through the configured fixed list
of absolute helper paths. An entry is refused unless it is **an absolute
path**, an existing **regular file** — a directory passed every check and was
then printed at startup as a configured helper, which is a list with an entry
nobody can run — **not setuid and not setgid**, checked on the file's mode
rather than on its name, and **not one of the known escalation tools**:
`sudo`, `doas`, `pkexec`, `run0`, `su`, `runuser`, `setpriv`, `machinectl`,
`systemd-run`, `nsenter`, `unshare`, `chroot`, `capsh` and `sudoedit`.

That last check is made on the **resolved** path as well as the one written
down: `ln -s /usr/bin/unshare harmless` walked straight past a check that
looked only at the last component it was handed. A symlink to an escalation
tool is an escalation tool, and the refusal names what the link points at.
So a list containing an escalation tool is itself refused, and so is a tool
nobody listed that happens to be setuid-root. A command not on the list is
refused by name. Nothing in M4 needs a helper, so no helper is invoked.

### Honest limits of "unprivileged"

A uid is not the whole of E-46, and on the first host Herd runs on it is not
even the interesting part:

- a user in `docker`, `podman`, `lxd`, `incus` or `libvirt` is
  **root-equivalent**: the daemon runs as root and will mount the host's
  filesystem wherever its client asks;
- a user in `wheel`, `sudo` or `admin` is one password away from root;
- a user who can write `/run/docker.sock` (or the podman, containerd, CRI-O
  or libvirt socket) has the same power whether or not it is in the group.

The runner **warns** about each of these at startup, naming the specific
group or socket, and runs anyway. It does not refuse: the owner's own server
is in `docker` because that is where the hub runs in docker compose, and a
runner that would not start there would be useless on exactly the machines
Herd was built for. Whether to accept that is the machine owner's call; what
is not his to lose is knowing. So on that server E-46 holds for the process
and **not** for the deployment, and the startup report says so every time.

## Caps (E-34)

- **Workspace** per job, inside the data root, reclaimed afterwards.
- **One session per job**, so every process it starts shares one process
  group, plus one marker in the job's environment.
- **Nice** on CPU as a floor — the configured value is what the job runs at,
  unless the runner is itself niced beyond it, which an unprivileged process
  cannot undo. IO goes to the idle class in the same breath.
- **Time cap**, **log cap** and **disk budget**, each ending the job with
  `stopped_by_cap` naming the cap and its figure. `herdctl` exits 124 for all
  four, which is `timeout(1)`'s code, with the sentence on stderr.

**A breached cap outranks the job's own exit status.** A job whose log was cut
at the cap can still exit 0 a millisecond later, and a job that keeps writing
into the pipe the cap closed dies of `SIGPIPE`. Neither is the job's verdict:
the runner checks the caps once more after the process is gone, and the cap
wins. A truncated build reported green is the worst thing a build service can
do, so the log cap is checked last and loudest.

**The kill.** When the job's own process is gone, everything it started goes
with it — a job that leaves a background child behind does not outlive
itself. How that is done depends on what the host gives:

- **with a cgroup**, one write to `cgroup.kill` takes the whole tree,
  whatever it did to detach itself, followed by a process-group kill for good
  measure;
- **without one**, the runner kills the job's process group and then sweeps
  `/proc` for every process still carrying the job's `HERD_JOB_MARKER`. A
  process keeps its environment across `fork` and `exec`, so a double fork, a
  `nohup` and a `setsid` are all still found — and `setsid` is precisely what
  a process-group kill cannot reach.

**No inbound socket.** A runner binds nothing in M4, asserted two ways: the
real binary is inspected through `/proc` while running a job, and a test reads
the crate's sources for anything that could bind. The mesh-bound fetch
endpoint of D10 and D11 arrives in M11.

## A runner's own death

A job must not outlive the runner that was watching it, in any of the three
ways a runner can go:

- **asked to stop** (`SIGTERM`, `SIGINT`): the runner **drains**. It refuses
  new work from that instant and says so to the hub at once rather than at
  the next report; it tells every running job it is over, which makes each
  job's own task kill its process tree — cgroup kill, process-group kill,
  marker sweep — reclaim its workspace, and hand the *attempt* back to the
  hub without a verdict; it waits briefly for those to land and to be
  acknowledged; and then, as a backstop, it ends whatever did not get that
  far itself. It is gone inside invariant 2's five seconds whatever happened,
  and a job still running when the owner asked is named on the record as an
  abandoned attempt and attempted again — never reported as "killed by signal
  9", which would be a verdict nobody earned.
- **asked to stop while holding a verdict**: a job that *ran to its end* with
  no hub to tell produced a real outcome, and that outcome exists nowhere but
  this process's memory. On its way out the runner writes the `job_finished`
  frame verbatim and fsynced to `<root>/held-results/<job>.json`, naming each
  path on stderr — and **the next runner on that data root picks it up,
  offers it on every link it makes, and removes the file only once the hub
  has acknowledged it** (D34). A verdict the hub will never record — an
  unknown job, a job already finished, an attempt superseded by a requeue —
  is dropped by name rather than offered for ever, and a file that cannot be
  decoded at all is kept under a `.unreadable` name rather than deleted.
- **killed** (`SIGKILL`): nothing can be handled, so the cleanup happens in
  the next life. At startup, before it dials the hub, a runner **reaps what a
  previous life left**: every process carrying a job marker whose runner is
  gone, every workspace under its own `jobs/` (safe because it holds the
  root's lock), and every `herd-job-*` cgroup under its base whose runner pid
  is gone. Each reclaimed thing is named in the startup report.
- **crashed in a job's task**: the job's own path reclaims its workspace and
  kills its processes on every exit path.

The reaper's test for "left behind" is deliberately weak: the pid that minted
the name must be **gone**. Nothing is inferred from a command name, so a
runner started through a wrapper, under another argv0, or as a test binary is
never mistaken for a dead one. The cost is pid reuse — a leftover can then
survive one more life. A leftover that lingers is a nuisance; a live job
killed by a stranger is a lost build.

## The memory ceiling, and its fallback (E-30, Q6)

Each job gets its own cgroup v2 directory with `memory.max` set **before** any
process is in it, `memory.swap.max` at 0 and `memory.oom.group` at 1. The
job's own process writes itself into that cgroup between fork and exec, so
there is no instant in which a job process exists outside its ceiling. An
allocation past the ceiling makes the kernel kill that cgroup — the job, and
only the job: the runner sits in a sibling cgroup and so does every other job.
The runner then reports `stopped_by_cap { memory }` rather than a bare signal,
because it reads `memory.events`.

### The deployment this needs

Creating a cgroup and *putting a process into one* are two different rights.
cgroup v2 requires write access to the `cgroup.procs` of the **common
ancestor** of the process's current cgroup and its destination — so a runner
that owns a delegated directory it is standing outside of can make job
cgroups it could never move a job into. The runner tests that ancestor at
startup and refuses rather than discovering it on the first job.

What works, therefore:

- a **systemd unit with `Delegate=yes`** (D5), with no `--cgroup-root` at
  all: the runner's own cgroup is the delegated scope, it moves its own
  processes into a `herd-runner.leaf` child of it — cgroup v2 forbids a
  cgroup from holding processes once a controller is enabled for its children
  — and hangs one cgroup per job beside that leaf;
- the same shape started by hand:
  `systemd-run --user --scope -p Delegate=yes herd-runner …`;
- `--cgroup-root <dir>` where `<dir>` **contains this runner**: its own
  cgroup, or an ancestor of its own cgroup, inside a delegated subtree.

What does not work, and is refused at startup naming the exact
`cgroup.procs` the operator would have to grant: `--cgroup-root` pointing at
a subtree the runner is not inside, however thoroughly that subtree is
chowned to the runner's user. A runner that falls back leaves no leaf cgroup
behind.

Because `--cgroup-root` is an explicit request for one subtree, a runner that
cannot use the one it was given **refuses to start** and says what the
subtree lacks. It does not quietly enforce something else instead.

### Where delegation is unavailable

No `memory` controller, a base the runner may not create cgroups in, a base
whose common ancestor it may not write, a base that still holds somebody
else's processes, or no cgroup v2 at all: the runner falls back to `RLIMIT_AS`
on each job process and says so at startup with `FALLING BACK` and the
reason. This is documented behaviour, not a silence, and it is weaker in
**three** ways that the startup report names:

- it bounds each process's address space rather than the job's whole tree, so
  a job of many processes can hold more in total than the ceiling;
- an allocation past it **fails** rather than killing, so the job reports its
  own failure instead of `stopped_by_cap { memory }`;
- and — the one that matters most — there is no handle on the job's tree to
  kill, so the kill is a process-group kill plus the marker sweep described
  above. That reaches a job that gave itself a new session; it cannot reach
  one that erases its own environment, which a cgroup kill would still catch.

Invariant 2 still holds either way: every job runs under an explicit memory
cap enforced by the kernel. The WSL replay of the 2026-10-12 out-of-memory
event is a manual check in M17, against the real ceiling on the owner's PC.

## What a runner measures (E-15)

Reported with the outcome and kept on the job's record: the host's one-minute
load average (recorded, never a gate — E-32), the job's peak memory, the
high-water mark of its workspace, the bytes of log it produced, and its wall
time. Peak memory comes from the job's cgroup where there is one, and from
sampling the job's own process where there is not, which undercounts a
process tree.

A figure the runner could not take is **absent** rather than zero, so that
nobody reads a missing measurement as a measured nothing. That applies to
`bytes_written` too: a workspace the runner could not measure reports no
figure at all *and* ends the job at its disk cap, while a job that genuinely
wrote nothing reports zero.

**The workspace figure is sampled**, every 200 ms and once more after the
job's last process is gone. So the high-water mark is the highest the runner
*saw*, and a job that writes and deletes faster than one sample can hold more
than its budget for less than a sample without being stopped. The final
measurement is what catches a job that writes and exits at once, which is the
case that matters: what a job leaves is always measured, what it held for an
instant may not be. Making that exact needs filesystem quotas, which is not
M4.

## Tests, and which of them a host can make

`./scripts/test.sh` runs the workspace suite — all of the above against the
real kernel, the real binaries and a real hub — and then prints every
assertion this host could not make.

Two assertions need cgroup v2 delegation, which a GitHub Actions runner and a
plain login session do not have: the *named* memory outcome, and the cgroup
branch of the startup report. On such a host they are **skipped with their
reason** rather than failed or quietly passed. A skip prints a
`HERD-SKIPPED:` line and appends it to `$HERD_SKIP_LOG`, because `cargo test`
hides the stdout of a passing test — so `cargo test --workspace` alone would
show a green run with no hint that part of it did not run.
`scripts/test.sh` prints the log; `scripts/report-skips.sh` turns each line
into a warning on the CI run and into its summary. A green check is not the
same claim on every host, and that difference is meant to be visible from the
terminal and on the pull request.

To make every assertion on a machine whose login session is not delegated,
run the test binary inside a delegated scope:

```sh
cargo test --workspace --no-run
systemd-run --user --scope -p Delegate=yes \
  target/debug/deps/confinement-<hash> --nocapture
```

Everything else — the disk budget against a job that hides its files, the log
cap against a job that exits at once, the reclaim of a workspace a job locked
shut, the kill of a `setsid` child, the end of a job when its runner is
stopped, the reaping of what a killed runner left, and the root-equivalence
warning — is asserted on every host, delegated or not.
