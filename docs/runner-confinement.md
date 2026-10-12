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
| `--job-memory-max` | `HERD_JOB_MEMORY_MAX` | `2GiB` | Kernel memory ceiling per job |
| `--job-timeout` | `HERD_JOB_TIMEOUT` | `3600` | Time cap per job, in seconds |
| `--job-log-cap` | `HERD_JOB_LOG_CAP` | `64MiB` | Log one job may produce |
| `--job-disk-budget` | `HERD_JOB_DISK_BUDGET` | `20GiB` | Workspace one job may hold |
| `--job-nice` | `HERD_JOB_NICE` | `10` | CPU niceness floor for a job |
| `--cgroup-root` | `HERD_CGROUP_ROOT` | the runner's own cgroup | Delegated cgroup v2 directory to hang job cgroups under |
| `--privileged-helper` | `HERD_PRIVILEGED_HELPERS` | empty | Absolute path of a command allowed to need privilege; repeat the flag, or separate with `:` |

Sizes accept a plain number of bytes or a suffix: `B`, `KiB`, `MiB`, `GiB`,
`TiB`, and the decimal `kB`, `MB`, `GB`, `TB`. A value that does not parse is
refused at startup naming itself.

A runner prints what it will enforce on its own stderr before it dials the
hub, so that the answer to "what is this machine doing to my job" never has
to be inferred from a job that behaved oddly.

## The data root (E-45)

One directory, validated at startup: created if absent, refused if it is a
file, not writable, or the filesystem root — every refusal naming the path.
Inside it, `jobs/<job>/` holds `work` (the job's working directory), `home`
(its `HOME`) and `tmp` (its `TMPDIR`), and the whole directory is removed when
the job ends.

The job's environment is built from scratch rather than inherited: `HOME`,
`TMPDIR`, `TMP`, `TEMP` and the `XDG_*` directories all point inside the
workspace, and only `PATH`, `LANG`, `LC_ALL`, `TZ` and `TERM` are carried
through. So nothing in the runner's own environment — a token, a `TMPDIR` on
the host's disk — reaches a job.

**The limit, stated plainly.** Herd writes nowhere but the data root, and a
job is given nowhere else to write. A job that names an absolute path outside
the root is *not* stopped by M4: that needs a mount namespace, which is not
built here. The declared command contract (M10) is where a job stops choosing
its own paths.

## Unprivileged (E-46)

A runner refuses to start if its real or effective user id is 0, and says so.
It invokes no escalation tool; a test walks the crate's own sources and fails
if any module but `privilege.rs` so much as names one. Anything that ever
needs privilege must go through the configured fixed list of absolute helper
paths: a command not on the list is refused by name, and a list containing an
escalation tool is itself refused. Nothing in M4 needs a helper, so no helper
is invoked — what exists is the refusal and the shape of the list.

## Caps (E-34)

- **Workspace** per job, inside the data root, removed afterwards.
- **One session per job**, so every process it starts shares one process
  group. When the job's own process is gone the runner kills that group and
  the job's cgroup, every time: a job that leaves a background child behind
  does not outlive itself.
- **Nice** on CPU as a floor — the configured value is what the job runs at,
  unless the runner is itself niced beyond it, which an unprivileged process
  cannot undo. IO goes to the idle class in the same breath.
- **Time cap**, **log cap** and **disk budget**, each ending the job with
  `stopped_by_cap` naming the cap and its figure. `herdctl` exits 124 for all
  four, which is `timeout(1)`'s code, with the sentence on stderr.

**No inbound socket.** A runner binds nothing in M4, asserted two ways: the
real binary is inspected through `/proc` while running a job, and a test reads
the crate's sources for anything that could bind. The mesh-bound fetch
endpoint of D10 and D11 arrives in M11.

## The memory ceiling, and its fallback (E-30, Q6)

Each job gets its own cgroup v2 directory with `memory.max` set **before** any
process is in it, `memory.swap.max` at 0 and `memory.oom.group` at 1. The
job's own process writes itself into that cgroup between fork and exec, so
there is no instant in which a job process exists outside its ceiling. An
allocation past the ceiling makes the kernel kill that cgroup — the job, and
only the job: the runner sits in a sibling cgroup and so does every other job.
The runner then reports `stopped_by_cap { memory }` rather than a bare signal,
because it reads `memory.events`.

This needs the runner's base cgroup to be **delegated**. The deployment that
gives it is a systemd unit with `Delegate=yes` (D5); `--cgroup-root` points at
a delegated subtree where the runner is not a unit. The runner moves its own
processes into a `herd-runner.leaf` child of its base first, because cgroup v2
forbids a cgroup from holding processes once a controller is enabled for its
children.

**Where delegation is unavailable** — no `memory` controller, a base the
runner may not create cgroups in, a base that still holds somebody else's
processes, or no cgroup v2 at all — the runner falls back to `RLIMIT_AS` on
each job process, and says so at startup with `FALLING BACK` and the reason.
This is documented behaviour, not a silence, and it is weaker in two ways
that the fallback line names:

- it bounds each process's address space rather than the job's whole tree, so
  a job of many processes can hold more in total than the ceiling;
- an allocation past it **fails** rather than killing, so the job reports its
  own failure instead of `stopped_by_cap { memory }`.

Invariant 2 still holds either way: every job runs under an explicit memory
cap enforced by the kernel. The WSL replay of the 2026-10-12 out-of-memory
event is a manual check in M17, against the real ceiling on the owner's PC.

## What a runner measures (E-15)

Reported with the outcome and kept on the job's record: the host's one-minute
load average (recorded, never a gate — E-32), the job's peak memory, the
high-water mark of its workspace, the bytes of log it produced, and its wall
time. A figure a host does not expose is **absent** rather than zero, so that
nobody reads a missing measurement as a measured nothing. Peak memory comes
from the job's cgroup where there is one, and from sampling the job's own
process where there is not, which undercounts a process tree.

## Tests, and which of them a host can make

`cargo test --workspace` asserts all of the above against the real kernel. The
memory ceiling's *named* outcome needs cgroup v2 delegation, which a GitHub
Actions runner does not have; on such a host that one assertion is **skipped
with its reason** rather than failed or quietly passed. A skip prints a
`HERD-SKIPPED:` line, CI runs the suite with `--nocapture`, and
`scripts/report-skips.sh` turns each line into a warning on the run and into
its summary. A green check is not the same claim on every host, and that
difference is meant to be visible on the pull request.

To make every assertion on a machine whose login session is not delegated,
run the test binary inside a delegated scope:

```sh
cargo test --workspace --no-run
systemd-run --user --scope -p Delegate=yes \
  target/debug/deps/confinement-<hash> --nocapture
```
