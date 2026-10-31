# Handover — Herd is shipped, not operated

On 2026-10-23 the owner decided that Herd no longer operates the hub or
the runner; that responsibility passes to the first consumer, chatOnS OS.
His words: *"tu ne dois plus gérer le hub et le runner, cette
responsabilité tombe au client, soit chatOnS OS."*

This document is the plan for resuming under that decision. It is a plan
to act on, not an essay: every section ends in something to build, stop,
sign or decide, and every open question carries one recommendation. The
reasoning behind the boundary itself is D161 in `docs/decisions.md`.

How it was written, so a reader knows what to trust:

- the plan was written against `36ef9ab` (v0.3.3, `main`) and cites the
  code by file and line. Everything it asserts about **what is built**
  was re-read at `5b1021e` on the evening of 2026-10-23, after the gate
  release; §3.0 and the opening line of each subsection of §3 carry
  that reading, and a line number below belongs to the first one and
  may have moved since;
- everything about the server outside the repository was read on
  2026-10-23 from the host and is marked **server**: the owner's
  `/etc/nixos/flake.nix`, the live unit, the consumer's capability file,
  the running container. Two things could not be re-read by this agent's
  uid and are marked **unverified here**: the hub's state volume and the
  runner's data root. The figures for those come from the scope review
  of 2026-10-23 and are flagged as such;
- everything on the consumer's side — their templates, their tooling,
  their nightly — is **theirs** and is not asserted; the document says
  what would settle each point instead;
- `UNBUILT` marks anything planned and not in the code.

Where the code disagreed with the two analyses this plan was built from
(the scope review and the grievance response), the code wins; the
disagreements are collected in section 10.

## 1. The boundary in one paragraph

**We own what is in this repository and what a tag produces from it: the
protocol, the four crates, the deployment files, the releases and the
documentation. They own every running instance: the hub container and
its volume, the runner units and their data roots, the capability files
and the probes behind them, the tokens, the disk, the restarts, the
upgrades.** What crosses the line is always a named artefact — a file
format, a flag, a document, an exit code, a JSON schema — and never a
person logging in. A fact about their machines that we need reaches us
as a bug report carrying a job id and two version numbers, or it does
not reach us.

The one sentence that makes the boundary honest rather than a
withdrawal: **we may not hand over an instance that only its author can
tell is healthy.** The fleet this was written about was exactly that,
and section 3 is what closed the gap. It did not close it first: the
handover was announced before any of it was built, and §3.0 says where
the gate stands and what is still open.

## 2. Where the line falls, subsystem by subsystem

One row per subsystem. "We ship" is an artefact with a version. "They
operate" is a thing on their machine. "Crosses" is the contract between
the two, named so it can be pointed at in a dispute.

| Subsystem | We ship | They operate | What crosses the line |
|---|---|---|---|
| Protocol | `herd-proto`, `PROTOCOL_VERSION` **3** on `main` and 2 in every release to date, `version_gap` (E-51, invariant 4) | nothing — but every peer of theirs must speak it | the refusal sentence naming both versions (`version_mismatch_reason` in `herd-proto/src/lib.rs`); a protocol move is announced in `deploy/release-notice.md` as a fleet-wide restart |
| Hub, the binary | `herd-hub` and the image `packages.hub-image` (D5, D153), both at the workspace version | the container, its restarts, `docker compose pull`/`load`, `.env` | `deploy/docker-compose.yml`, `deploy/env.example`, the image tag `herd-hub:<version>`, the archive attached to the release and `docker load`ed (D162), the three lines the hub prints at startup (`herd-hub/src/main.rs:252-288`) |
| Hub, the state root | the layout and its **five** version numbers — `LAYOUT_VERSION`, `RECORD_VERSION`, `ROSTER_VERSION`, `INDEX_VERSION`, `READINGS_VERSION` (D8, D73) — and the rule that an unreadable version refuses to start. The hub's **sixth**, `STATUS_VERSION`, is in the *Status and events* row; `docs/architecture.md` §3.4 is the whole list of eleven and what each one refuses | the volume `herd_herd-state`, its filesystem, its backups, its size | `job.json`, `events.jsonl`, `log`, `files/` as plain files (E-15, E-53); a `herd-hub migrate` is UNBUILT and is the only way a `RECORD_VERSION` move will ever be theirs to run. **`RECORD_VERSION` and `LAYOUT_VERSION` are both 2 on `main` since M11's pack half (D199)** and 1 in every release to date, so the first move of either has now happened with no such tool: what crosses the line at v1.0.0 is a reset, not a migration |
| Hub, the output budget and sweeper | `--output-budget`, `--output-floor`, the four-rank eviction, the manifest that outlives the bytes (D152) | choosing the two figures, watching the disk, the first real eviction | the two flags, the `OutputsRemoved` event and the sentence a requester gets for an evicted collection |
| Hub, the roster and tokens | `runners.json` with salted hashes, `herdctl runner approve/remove/list` (E-48, Q8) | `/etc/herd/requester-token`, each runner's token file, who holds the requester token | the token's path in `.env`; the approve command's one-time print; the roster version |
| Runner, the binary | `herd-runner` static musl, and the flake package | the process, its user, its groups, its restarts | every `--flag`/`HERD_*` pair in `herd-runner/src/main.rs:36-327`; the startup report (`confine.rs`); the exit sentence |
| Runner, the unit and module | `deploy/herd-runner.service` and `deploy/nixos-herd-runner.nix` as **reference texts**, with their OWNER notes | the copy or the import in `/etc/nixos`, the pin, `nixos-rebuild`, every OWNER item | the module's options (the seven figures that were literals are options since D163, section 3.3); the unit's OWNER notes (`deploy/herd-runner.service:64,76,87,153`) |
| Runner, the data root | the layout (`jobs/`, `held-results/`, `outputs/`, the lock, the switch) and E-45's cooperative confinement (D28) | `/mnt/ssd_980/herd-runner`, its filesystem, its free space, **the spool it cannot free** (D158) | the D158 message naming the shallowest directory the runner does not own; the `switch` file's JSON |
| Runner, capabilities and probes | the file's strict schema (D60, D61), the three kinds, `busy_probe`, the suspension rule (D48, D49) | the file, the probe scripts, their substitution, their permissions | `--capabilities FILE`; the `herd-runner: <cap> is suspended: <reason>` stderr line (`capability.rs:860`); `herd-runner check --capabilities FILE`, and a probe that cannot be spawned refusing the file when it is loaded (D165, section 3.1) |
| Runner, the owner's switch | `herd-runner switch`, `herdctl switch`, `SwitchApplied` (E-33, D99) | the owner's shortcut, the reason she types | the switch file; the fleet listing `herdctl switch`, now carrying each runner's build and its suspended capabilities (D166); the drain-restart trap, closed by D164 (section 3.4) |
| `herdctl` | the binary, the declaration reader (D106), the exit codes (D20, D40) | their wrappers, their `commands.toml`, their agents' `HERD_TOKEN` | the exit-code table (`herdctl/src/main.rs:200-236`), the `ran on:` line with `OUTPUT_FORMAT_VERSION` (Q7, D110), the stderr `job-id:` line; the verdict document of `run|wait --verdict-to FILE`, carrying `VERDICT_FORMAT_VERSION` (D166, section 3.2) |
| Releases | the tag, three binaries, `SHA256SUMS`, the notice, the image (D132, D153) | pinning a version in `/etc/nixos` and `.env`, moving the fleet | the GitHub release page and `deploy/release-notice.md`; **shipping again since v0.3.4** — three binaries, `SHA256SUMS` and the image archive, no credentials (D162, section 3.5) |
| Status and events | `status.json` with its own `STATUS_VERSION`, published on change and never on a clock, with `herd-hub status` as the one text screen over it (E-35, D215); four of E-37's five events to one configured sink, each payload fronted by `EVENT_FORMAT_VERSION` (D216). Both landed 2026-10-29 in batch 8; E-37's fifth event and the use-against-budget figure are still UNBUILT | the renderer, the ntfy topic, the dashboard, the sink's URL or command | the two documents and their two numbers: `STATUS_VERSION` refuses a document of another version by name and its sentence says *"There is no migration"*, while `EVENT_FORMAT_VERSION` refuses nothing and is the only thing their notification handler can branch on (`docs/architecture.md` §3.4) |
| Documentation | `deploy/README.md`, `docs/joining.md`, `docs/runner-confinement.md`, `docs/architecture.md`, UNBUILT `docs/operating.md` | reading them before changing a figure; their own runbooks on top | the OWNER vocabulary: every step marked OWNER is theirs by definition |
| Diagnosis | the sentences on the record (E-04, E-36), the hub's log lines, the runner's journal lines | reading them, on their machines | a bug report: hub version, runner version, job id, the verdict, the record directory (section 6, O-6) |

Three rows deserve a sentence each, because they are where the line is
easiest to get wrong.

**The capability file is theirs, and the proof is on the server.**
`/mnt/ssd_980/herd-etc/chatons-runner.json` (**server**, root-owned,
written 2026-10-23 15:08) carries `"command": ["@probe@", "qemu",
"@agents_user@"]` verbatim. Herd substitutes `{j}`, `{job}`, `{commit}`,
`{args}`, `{in}`, `{out}`, `{line}` in a declaration (D107,
`architecture.md` §8.2) and nothing in a capability file. The runner
spawns `@probe@`, gets `ENOENT`, and reads the capability as
`Availability::unknown("the probe … cannot be started: …")`
(`herd-runner/src/capability.rs:639`), which **suspends** it (D49). So
`kvm` and `gpu-venus` are suspended for as long as that file stands, and
every job needing them waits with a true sentence that nobody reads.
That is their fix to make — and section 3.1 was ours, because a runner
that lets this happen silently is not one we may hand over. Ours is
done: since D165 a runner handed that file refuses to start, naming the
file, the capability and the program, and `herd-runner check
--capabilities FILE` says the same before a restart. Theirs stands
until the templates are substituted.

**The data roots are theirs, including the part Herd cannot clean.** The
spool under the runner's `outputs/` holds trees the consumer's jobs
deposited under uid 1002 `server:users`; the runner is uid 986 and
cannot unlink inside them (D158, `docs/decisions.md:6502-6527`). The hub's
own log shows the same family turning into verdicts: three jobs ended
`stopped_by_cap` on disk because the runner *could not measure* a
directory it may not read (**server**, `docker logs herd-hub-1`,
j0000000984, j0000000989, j0000001197). The bytes are theirs to free;
the misreporting was ours and is fixed: since D166 a workspace the
runner is refused ends the job `SetupFailed`, exit 125, naming the
path, and never a disk cap at 124 (section 3.2).

**The pin is theirs.** `/etc/nixos/flake.nix:15` (**server**) reads
`herd.url = "github:tomasalexandre18/Herd/v0.3.2"`, the live unit runs
`herd-runner-0.3.2`, the hub container is `herd-hub:0.3.3`. Moving a pin
is "the deliberate act of upgrading the runner", in the owner's own
comment on that line. Under the boundary it stays deliberate and becomes
theirs, with one obligation attached (O-1). Those three figures were read
before the gate release; a supported pin today is **v0.3.4** or the tag
before it, v0.3.3 (O-1), and v0.3.2 is under both. Nothing in section 3
reaches a machine until the pin moves.

## 3. The gate: what had to be built, and where it stands

### 3.0 Where the gate stands

Re-read at `5b1021e` on the evening of 2026-10-23. Six of the seven
items below are built; the seventh is not. Four of the six are in
**v0.3.4**, the gate's own release, cut at 17:43; the other two landed
on `main` after the tag and are **in no release**, so the gate as
shipped software is smaller than the gate as merged work. Nothing in
either reaches the fleet until the pin moves off v0.3.2 (O-1).

| Item | State | What landed, and where it differs from the plan |
|---|---|---|
| 3.1 an unstartable probe refuses the file; `herd-runner check` | **met, in v0.3.4** | D165. The refusal is at *file load*, before the process is a runner, and D49's runtime rule is untouched — the distinction 3.1 asked to be written in the code is written in D165 |
| 3.2 a verdict a machine can read | **met, on `main`, in no release** | D166, merged at 18:02, after v0.3.4 was cut. As `run\|wait --verdict-to FILE` and not `--json`: a document carrying `VERDICT_FORMAT_VERSION`, `cause`, `exit_code_from`, `job_exists`, `ran_on`, the cap's figures and the prose `herdctl` printed. With it the permission lie: a workspace the runner is refused is `SetupFailed` at exit 125, never a disk cap at 124 |
| 3.3 the seven module options | **met, in v0.3.4** | D163, plus one thing this plan ruled out and the code made free: `args_override_self` on the runner, so a repeated flag takes the value stated last and `extraArgs` can override after all |
| 3.4 a persisted `drain` is read as `off` | **met, in v0.3.4** | D164, as planned, reason rewritten to say the drain happened and that the switch is now `off` |
| 3.5 a release that ships | **met, in v0.3.4** | D162, the recommendation taken: no credential preflight, the three binaries published first, the hub image attached as a `docker load` archive. v0.3.4 carries the three binaries, the archive and `SHA256SUMS`, and is the first release since v0.3.0 |
| 3.6 versions you can see | **met for the runner, not for the hub; on `main`, in no release** | D166 puts a runner's build and every suspended capability with its reason on `herdctl runner list` and on `herdctl switch`. The hub's own version is **not** on that listing: it travels on `Welcome` and `herdctl` drops it (`herdctl/src/lib.rs:1395`), so O-6's report is where a hub version still comes from |
| 3.7 a reading costed and bounded | **not met** | nothing landed. A reading is still minted with no figures (`herd-hub/src/server.rs:2724`), `memory_need_mib` still answers `Ok(None)` for a job that declares none (`herd-hub/src/job.rs:1100-1123`), the matcher still reserves `unwrap_or(0)` (`herd-hub/src/matcher.rs:441`), and no `--readings-per-runner` exists. Four processes on a two-slot runner, accounted as two |

Also landed on `main` and in no release, and nowhere in this plan:
**D167**, a job is reported finished only once its declared outputs
have reached the hub. The
consumer found it after the handover — `herdctl wait --outputs-to`
returned *successfully* while files were still arriving, one job short
195 of 657 — and it is the worst defect Herd has had. A gate about an
instance that cannot be judged from outside did not foresee a verdict
that was itself untrue; nothing in section 3 would have caught it.

**So the gate is substantially met and not cleared.** Two of its three
headings are done as code: a release that ships (released), and a
verdict a machine can read (merged, untagged). The third — an instance
that does not lie to its operator — is done but for 3.7, which is the
one part of it an operator cannot see at all and cannot bound: the hub
puts processes on their machine and weighs them at zero. One item,
costed at a day, plus a tag that carries D166 and D167.

**And the handover did not wait for the gate.** It was announced on
2026-10-23 before any of these seven landed, and the consumer accepted
it with a full inventory of the defects they were taking on
(orchestrator's report; no GitHub issue records the exchange). D161's
third rejected alternative — *hand over now and fix the gate
afterwards* — was rejected as a plan and is what happened anyway, with
that inventory standing where the gate was meant to stand. So nothing
below is a precondition that was waited for. It is the obligation the
handover created, and six of its seven items were discharged inside the
day.

What follows is the analysis that produced those seven items, kept as it
was written: the reasoning is the record of why each one exists, and the
state of each is in the table above and in the line that opens its
subsection.

The test case is the placeholder bug: an operator must be able to tell a
healthy runner from a half-dead one **without reading our source**.
When this was written they could not, and here is exactly why, in the
code:

1. the probe is read *beside* the join, not before it
   (`herd-runner/src/lib.rs:2111-2118`), so a runner with an unstartable
   probe joins the hub looking like any other;
2. the suspension is printed **once**, on change, to the runner's stderr
   (`capability.rs:860`), and then never again;
3. the hub keeps the list of suspended capabilities "for the hub's log"
   (`herd-hub/src/server.rs:1145-1150`) — a log line, not a state anyone
   can ask for;
4. `herdctl runner list` returns `ApprovedRunner { name, approved_unix_s,
   connected }` (`herd-proto/src/lib.rs:443-449`) and `herdctl switch`
   returns `RunnerSwitch { name, switch, jobs, stopping }` (`lib.rs:427-
   435`): **neither carries a version or a suspended capability**;
5. the waiting job's sentence does carry everything (D159's `of_which`,
   E-04) — on a record nobody opens unless a job is already late.

So the information exists, in four places an operator never looks, and
nowhere an operator does. Five candidates were weighed; the verdict on
each follows, then the three that gate the handover.

### 3.1 A runner that refuses what it cannot do, and a check to run first

**Built, D165, in v0.3.4.**

**Build: a startup refusal and a `check` subcommand.** Two changes in
`herd-runner`, no protocol move.

- At startup, a `busy_probe` of type `command` whose program cannot be
  spawned — `ENOENT`, `EACCES`, a directory — **refuses the startup**,
  naming the capability, the command and the error, exactly as an
  unknown key in the same file already does (`main.rs:194-197`, D60).
  The distinction from D49 is deliberate and must be written in the
  code: D49's "unknown suspends, never fails" is about a probe that
  *was* readable and stopped being — a lock file removed while the hub
  was down, a script that hangs tonight. A program that does not exist
  on the first read is not a moment about the machine; it is a
  configuration the operator has not finished, and the strict-reading
  rule is the one that applies. After startup, the same error keeps
  D49's behaviour: suspended, named, waiting.
- `herd-runner check --capabilities FILE [--data-root …]`: read the file
  strictly, run every probe once with the real timeout, print one line
  per capability — `kvm: counted(2), probe free` or
  `kvm: counted(2), probe CANNOT START: @probe@: No such file` — and
  exit 0 only if every probe answered. Dial nothing. This is the command
  their configuration generator runs before `systemctl restart`, and the
  first line of every bug report about a capability. It is cheap: it is
  `CapabilityFile::parse` plus one `sweep()` plus a printer.

Cost of building it: a day, with the tests that prove a refused probe is
refused and a hanging one is still only suspended. Cost of not building
it: the fleet as it is now — two capabilities dead for a day with four
jobs queued behind them, found by us reading their disk.

### 3.2 A verdict a machine can read

**Built, D166, on `main` and in no release yet**, as `run|wait
--verdict-to FILE` rather than `--json`: a file and not stdout, so the
prose a person reads stays exactly where it was. The `SetupFailed` half
below landed with it.

**Build: `herdctl wait|run --json`, printing the serialised `JobOutcome`
plus the runner's name, the attempt list and the outputs manifest.**
`JobOutcome` is already `Serialize` (`herd-proto/src/lib.rs:720`);
`FromHub::RanOn` already carries `runner: String`; `StoppedByCap` already
carries `cap`, `limit` and `detail`. Nothing new is computed, it is
printed. One field is added to the format: the `OUTPUT_FORMAT_VERSION`
it already has (Q7). Exit codes do not change (D20).

Four complaints close on this one change: the consumer's 9 (the memory
cap exits 124 without the word *memory*), 10a and 10c (the runner's
name only on a stderr line to regex), and the scope review's 3.2 (125
covers five cases). It is also the shape of every future bug report
(O-6): a report without the JSON is a report we cannot act on.

While in that code: a collection that fails on a **permission** error is
reported today as `StoppedByCap { Disk }` because the runner "could not
measure the job's workspace" (**server**, the three jobs above). That is
a lie wearing a cap's clothes: the job did not exceed anything. It
becomes `CouldNotStart`-class, exit 125, naming the path and the uid
that owns it — the D158 message already computes the path. This is the
Herd half of grievance 7; the other half is O-4. As built it is a new
`SetupFailed` and not `CouldNotStart`: D166's reason is that
`CouldNotStart` means Herd never got a process at all, so reporting a
workspace it was refused that way would hide the one fact the report
exists for. Exit 125 either way.

### 3.3 The seven module options

**Built, D163, in v0.3.4** — and one line more than "nothing else":
`args_override_self` on the runner's parser, which makes the repeated
flag below take its last value, so `extraArgs` overrides too. The
objection in "Not done" was to *silence*; clap's own rule is not silent.

**Build: seven `mkOption`s, and nothing else in the module.**
`deploy/nixos-herd-runner.nix:116-118,132-135` renders `--job-log-cap
64MiB`, `--job-disk-budget 20GiB`, `--job-nice 10`, `--host-tmpfs 4GiB`,
`--host-tmpfs-path /dev/shm`, `--health-every 15`, `--drain-grace 3` as
literals. The module does have `extraArgs` (`:355-363`), but it cannot
help: clap refuses a repeated option by default and the workspace uses
`overrides_with` nowhere, so appending `--job-nice 5` fails the startup
instead of winning. An operator who now owns the figures and cannot set
them is blocked by us, and D144's own module header says the unit and
the module "must be changed together" — so the unit's comments gain the
seven defaults in words, and the module gains the seven options with
those defaults. Half a day.

Not done: making `extraArgs` override. It would silently turn a typo
into two values on one line, and a module option is what a NixOS
operator expects.

### 3.4 Drain does not loop

**Built, D164, in v0.3.4.**

The trap, verified end to end: a switch is persisted under the data root
(`herd-runner/src/switch.rs:95`); `Drain` means "this runner leaves once
it holds nothing" (`herd-runner/src/lib.rs:966-967`); the leaving half
polls `leaves_when_empty() && jobs_running() == 0` from the first second
(`lib.rs:2166-2185`), so a fresh process with a persisted `drain` and no
jobs exits at once; the unit says `Restart=always`, `RestartSec=5s`
(`nixos-herd-runner.nix:535-536`, the live unit lines 26-27, **server**);
and the hub's `set_a_switch` answers "no runner called X is connected
… nothing about it has changed" when the runner is between two lives
(`herd-hub/src/server.rs:4810-4835`) — it holds no pending switch. The
runner is up for well under a second every five, never long enough to
receive `switch on`. The consumer needed physical access to break out.

**Build: at startup, a persisted `drain` is read as `off`, with the
reason rewritten to say so** — `drained and left on <date>; now off
(<original reason>)`. The drain *happened*: the process left. What the
owner wanted from `drain` was the machine withheld, and `off` is that,
connected and able to receive `on`. One function in `switch.rs`, one
test that starts a runner over a `drain` file and asserts it connects
`off`. No protocol move, no pending-switch state at the hub.

Why not the consumer's three fixes (stay connected while draining; a
pending switch at the hub; a flag to not persist drain): the first
redefines E-33's word, the second is new durable state for a case the
first change removes, the third is a knob that lets the trap be
re-enabled. Cost of this fix: an hour. Cost of leaving it: an operator
we have just made responsible for restarts, shipped a restart loop.

### 3.5 A release that ships

**Fixed, D162, in v0.3.4**, the recommendation below taken whole: no
Docker Hub, no credentials, the image archive attached to the release,
the binaries published before it. One thing went differently from step 1
of section 9: the v0.3.3 tag was not re-run, v0.3.4 was cut instead, so
**v0.3.1, v0.3.2 and v0.3.3 still have no release** and the page shows
v0.3.0 then v0.3.4.

**Finding, not in either analysis:** `.github/workflows/release.yml` has
**failed on every tag since D153 landed** — v0.3.1, v0.3.2 and v0.3.3
all stopped at "the Docker Hub credentials this tag needs, by name"
(`gh run list --workflow=release.yml`, run of 2026-10-19 13:19 for
v0.3.3: `MISSING … DOCKERHUB_USERNAME`, `MISSING … DOCKERHUB_TOKEN`,
exit 1). D153 ordered that check before anything is built so a tag never
leaves binaries published and an image missing; the account never
existed, so since then a tag leaves **nothing** published. The newest
GitHub release is **v0.3.0**. The fleet has moved past it only because
the Nix flake input pins a git tag, not a release, and because the hub
image was built on the server and `docker load`ed — the path
`deploy/docker-compose.yml:7-12` says "is no longer the normal way".

Under the boundary this is the first thing to fix, because every other
item in this section reaches them through a release or not at all.

**Decision for the owner — recommendation: drop Docker Hub and attach
the image archive to the GitHub release.** `nix build .#hub-image`
produces a `docker-archive` tarball (111 MB loaded, **server**); the
release uploads it beside the three binaries and `SHA256SUMS` covers
four files; the operator runs `docker load < herd-hub-0.3.4.tar.gz` and
names `herd-hub:0.3.4` in `.env`. D153's principle survives untouched —
one derivation, the thing deployed is the thing tested, the archive's
own manifest asserts its single tag — and the two credentials, the
`skopeo` shell and the pull-back-by-digest step go. Cost: `docker load`
instead of `docker compose pull`, a few hundred megabytes on each release
page. The alternative, keeping Docker Hub, costs an account and a scoped
token that have been "waiting on Alexa" since 2026-10-18
(`docs/status.md:88-90`) and a registry we would be operating — which is
the thing this decision stops us doing.

Either way the workflow must never again let an *optional* publication
veto the binaries: the image step fails its own step, not the release.

### 3.6 Versions you can see

**Half built, D166, on `main` and in no release yet.** `ApprovedRunner`
gained `facts`, a
`#[serde(default)]` field carrying the runner's build and its suspended
capabilities with their reasons, printed by `herdctl runner list` and by
`herdctl switch`. The hub's own version did not join the listing: it is
on `Welcome` and `herdctl` discards it, so a hub version reaches us only
through O-6's report. That residue is small and named, not forgotten.

**Build: the build version on `ApprovedRunner` and in `herdctl runner
list`, and the hub's version on the same listing.** `Hello` already
carries `peer_version` and the hub already judges it with `version_gap`
(`server.rs:1694`); the roster listing simply does not repeat it. A
`FromHub` variant a requester sees gains a field: with `#[serde(default)]`
and "absent means unknown" this is additive under D124's own rule
(`architecture.md` §3.3). Half a day. Without it, every grievance begins
with us asking which version they run, and `docs/status.md:13` saying
`v0.2.0` while the server runs 0.3.2 is what that looks like from our
side.

### 3.7 A reading weighs what it puts on the machine

**Not built. This is the one gate item still open**, and every line of
it below was re-read at `5b1021e` and still holds.

A requirement reading is admitted against memory while declaring none,
and nothing bounds how many run on one runner. The chain, re-read:

- `reading_job` mints the reading with `.with_m9(record.deadline, None,
  None)` (`herd-hub/src/server.rs:2472`) and its doc comment says what it
  leaves out: "the figures (it builds and boots nothing)" (`:2459`). The
  same comment, ten lines earlier, says `up` and `fetch` "are what put
  the tree on a machine" (`:2449`) — and a reading runs both. The
  premise is false: a reading materialises a tree;
- with no `memory_mib`, no `build_thread_mib` and no reading of its own,
  `memory_need_mib` answers `Ok(None)` (`herd-hub/src/job.rs:1114`), and
  `reserved` takes that as `unwrap_or(0)` (`herd-hub/src/matcher.rs:441`);
- `Occupant::Reading` is consulted to skip the counted slot and to word
  the sentence (`matcher.rs:875`, `of_which`). No per-runner bound on
  readings exists anywhere in the hub.

So D159 ("holds no counted slot") and D119 ("still admitted against the
room") are both true and together admit nothing: four processes on a
two-slot runner, accounted as two, each able to grow to the runner's
`--job-memory-max` — 8 GiB on a host lending 20 GiB (**server**, the
live unit). The consumer reported seeing exactly this on 2026-10-22 and
quoted the hub's own sentence, "build has 2 slot(s) and 2 are taken, and
2 requirement readings are running there besides, holding no slot
(D159)" — reported to this plan by the orchestrator, not re-read here.

Under the boundary the operator owns the host's memory, its absence of
swap and its survival, and our accounting under-reports what we put on
their machine. That is a gate item, not a refinement.

**Build: a job with no figure is costed at something, and readings are
bounded.** Two changes, one decision inside them.

- Every job the hub cannot cost — a reading, and the ad-hoc job the
  review's 3.7 describes — is reserved at a figure instead of zero.
  Recommendation: the runner's own `--job-memory-max`, carried on
  `Hello` as a `#[serde(default)]` field of a runner-only frame (no
  protocol move under `architecture.md` §3.3's rule), because the cap a
  job runs under is the only honest cost of a job that declares none.
  What makes that so is that the cap is runner-wide:
  `herd-runner/src/cgroup.rs:660` writes the same `--job-memory-max`
  into every job's cgroup, so it is the one memory figure the hub can
  know applies to a job it cannot cost. It is **not** D87's honesty
  claim, which reads the cap as enforcing the job's own declaration; no
  frame carries that declaration to the runner. Absent the field — an
  older runner — the reservation is the hub's `--reading-memory` figure,
  printed at startup. The figure is the owner's to set.
- `--readings-per-runner`, default **1**: a second reading waits, with
  the sentence saying for what. A reading is a `--labels` line;
  serialising them costs seconds and bounds what the hub cannot
  otherwise promise.

Cost: a day, with a test that admits one reading against a full room
and refuses a second by figure. The scope review called the D119/D159
asymmetry a documentation problem needing "a sentence, not code"; the
source says it needs code, and the review's author has withdrawn that
reading. It also strengthens the review's wave-3 case for moving
readings into the requester's checkout: a mechanism that is slot-free,
weight-free and unbounded is one the hub can promise nothing about. That
move stays a proposal to the consumer (O-9), never a unilateral removal.

### The three that gate the handover

Of the seven above, three are the gate; the other four ride with them.

1. **A release that ships** (3.5) — without it there is nothing to hand
   over but a git URL and a build instruction.
2. **An instance that does not lie to its operator** (3.1 with 3.3, 3.4,
   3.6 and 3.7) — refuses what it cannot do, checkable before start,
   every figure settable, no restart trap, version visible, and nothing
   on the machine that the hub weighs at zero.
3. **A verdict and a report a machine can read** (3.2) — the one
   artefact that lets a bug cross the boundary with its evidence.

Where the three stand on the evening of 2026-10-23: 1 is done and
released; 3 is done and merged, in no tag yet; 2 is done but for 3.7.
So two of three are cleared as code, one of three has reached a release,
and the gate is not cleared. 3.0 has it item by item.

Not in the gate, and said so: M13's status document (section 4) — it is
what *replaces* our reading of their disk and is the first thing after
the gate, but a handover does not wait for it; the owner's switch works,
`herdctl switch` lists the fleet, and the records are plain files they
can read today (E-53).

## 4. What replaces the access we are giving up

Every decisive number in the two analyses came from reading their disk:
678 of 678 files delivered on six jobs, 90 GB against 200, 7.4 GB of
spool, the `@probe@` string in a root-owned file, the `waiting_for`
sentence of j0000001566. This agent already could not repeat two of
those reads (**unverified here**: the volume and the data root are
unreadable to uid 1002). That is the boundary working as decided, and
it is a real loss. Four things replace it, in order of arrival:

| Access lost | What replaces it | When |
|---|---|---|
| reading `job.json` and `waiting_for` on their volume | `herdctl wait --verdict-to FILE` (3.2) in the bug report, and the record directory tarred (O-6) | merged, **needs a tag** |
| `journalctl -u herd-runner` on their host | `herd-runner check` output (3.1) in the report; the startup refusal making the common case impossible | **in v0.3.4** |
| `du` on `files/` and the sweeper's log line | the hub's startup line already prints the budget; M13's status document carries use-against-budget, evictions so far, and the floor (D152 "what M12 still owes") | M13 |
| counting suspended capabilities across the fleet | `herdctl runner list` showing version and suspended capabilities with reasons; M13's document carrying the same | listing merged, **needs a tag**; M13 (document) |
| our own end-to-end runs against the deployed pair | **nothing replaces this fully.** The suite's real-binary tests and `scripts/release-smoke.sh` stay; the deployed pair stops being our test bed. Section 8, R6 | — |

**M13 moves to first after the gate**, and its shape changes slightly
under the boundary: the status document is **the operator's view**, so
the fields are what an operator asks — per runner: name, version, switch,
jobs, suspended capabilities with reasons, health age; per hub: version,
jobs not over, output use against budget, floor, evictions so far,
roster size. E-35's list already names most of this. Events (E-37) are
pushed to a URL *they* configure; we ship the payload schema and the
list, never a destination.

## 5. What we stop doing, explicitly

Habits, each with the thing that replaces it. "Replaced by" is what we
do instead; "theirs" is what they do instead.

| We stop | Replaced by |
|---|---|
| logging into the server to look at the hub or the runner | **nothing.** A question about a running instance is a question to them, answered with O-6's report |
| reading their hub volume or runner data root to prove a point | the report's record directory and `--json` verdict; M13's document |
| `docker compose up` / `docker load` on their box | the release page; `deploy/README.md` is theirs to follow |
| `nixos-rebuild` or editing their pin | O-1: they move the pin; the release notice says what moving it means |
| diagnosing their jobs — the pack short of 188 objects, the 32-minute `check2` | we answer about Herd's verdict and Herd's sentence; the job's own content is theirs. We will still say when a grievance is misattributed, from the report's evidence |
| writing or fixing their capability file and probe scripts | `herd-runner check` (3.1) and the file's schema in `docs/operating.md` |
| choosing their figures — 1 or 2 build slots, 200 GB, 7 days, 20 GiB | the flags and the seven options; the notice's section on what each figure guards |
| freeing their disk (the spool, the first eviction) | O-3 and O-4; the hub says when it evicts and the runner names the directory it cannot remove |
| keeping `docs/status.md` as a picture of *their* deployment | `docs/status.md` describes the **software**: what is merged, what is released, what is unbuilt. Its "The host, as it really is" section goes; the facts in it move to `docs/operating.md` as *an example deployment, read on 2026-10-23* |
| counting their fleet in our plan (the server, the PC, "more machines later") | the plan names capabilities we support, not machines we run. M17 becomes a supported-platform milestone, not a joining (section 7) |
| being "waiting on Alexa for a Docker Hub account" | the decision in 3.5, taken the same day: no registry, the archive on the release (D162) |

What we do **not** stop: owning every defect in the four crates, the
unit, the module and the compose file, however it was found; answering a
report that carries O-6's evidence; cutting a release when a fix lands;
saying no to features that smell like generic CI (invariant 1).

## 6. What they must accept

Written as obligations they can agree to or refuse, one by one. A
refusal is an answer, and it tells us which sections above to keep
building.

- **O-1 — Run a supported version.** A supported version is the newest
  tag or the one before it. A report against anything older gets one
  answer: upgrade and retry. We in turn promise that a tag always ships
  (3.5, kept since D162 — v0.3.4 is the first release since v0.3.0) and
  that a protocol move is announced in the release notice as a
  fleet-wide restart (`deploy/env.example:21-26`). The newest tag is
  **v0.3.4** and the server's pin is v0.3.2, two tags under it, so the
  fleet is already below what this obligation asks — and the next tag
  will put it three under, because D166 and D167 are merged and
  untagged.
- **O-2 — Own the pin and the restarts.** `/etc/nixos`, `.env`, the
  units, `Restart=`, the reboot. We never touch them; `deploy/` is a
  reference they copy or import (D130, the module's header).
- **O-3 — Own the disk.** The hub's volume and its two figures
  (`--output-budget`, `--output-floor`); the runner's data root and its
  budget. The first real eviction will happen on their disk under their
  figures — they are told now that **none has ever happened** (scope
  review, 2026-10-23: 90 GB of 200, zero evictions; **unverified
  here**). Recommendation to pass on: lower the budget deliberately for
  one afternoon and watch the first eviction while it is cheap.
- **O-4 — Deposit what the runner owns.** A tree under `{out}` the
  runner's uid cannot unlink stays on their disk for ever (D158); Herd
  will not pretend otherwise and, after 3.2, will name the path instead
  of calling it a cap. They either chown what they deposit or free the
  spool themselves.
- **O-5 — Substitute their own templates, and check before start.** The
  capability file is theirs; `@probe@` and `@agents_user@` are not Herd
  tokens. `herd-runner check` (3.1) runs in their generator before the
  restart, and a runner that refuses to start is theirs to read.
- **O-6 — Report with evidence.** A report carries: the hub version and
  the runner version (3.6), the job id, `herdctl wait <id> --verdict-to
  FILE` (3.2), and the job directory `jobs/<id>/` tarred — `job.json`,
  `events.jsonl`, `log`, never `files/`. For a capability: `herd-runner
  check` output. For a runner: `journalctl -u herd-runner --since` the
  incident. Anything less is answered with this list.
- **O-7 — Keep the token private, and tell us when it leaks.** One
  requester token is one `merger` level for everyone (status.md "not
  done" 4). Until the admin role exists (section 7, M15b), the token is
  the whole gate.
- **O-8 — Read `deploy/release-notice.md` before moving a pin.** It is
  quoted into every release for exactly this.
- **O-9 — Tell us which mechanisms they use.** `excludes`, `drain_first`,
  `labels_from`, the requeue code 75: a mechanism nobody uses is one we
  will propose to remove (section 7). Their answer decides.

What we do **not** ask: that they read our source, that they fork the
module, that they accept a feature freeze, or that they stop filing
grievances. Misattributed grievances are the normal case and are
answered from the report.

## 7. The milestone plan, revised

`docs/plan.md` M1-M18 was written for a project that ran its own fleet.
The table says what happens to each under the boundary; the text after
it reconciles with the scope review's four waves.

| Milestone | Was | Becomes | Why |
|---|---|---|---|
| M1-M9 | merged | **survive unchanged** | the scheduler spine is software; nothing about it names a machine we run |
| M10 | merged | survives | the declaration contract is the consumer's side of the line already (D106) |
| M11 record half | merged | survives | streaming is measured as the norm — 275 of 1571 jobs over 32 MiB (review, **unverified here**) |
| M11 pack half, M12 peer-to-peer and keepers | planned, UNBUILT | **cut, pending a figure** | a distributed cache for a fleet we no longer run, with no measurement defending it; `{in}` stays made and empty. Cost of cutting: a second machine builds and tests on itself; cost of keeping: the only unbuilt milestone with mesh listeners, custody and liveness in it |
| M12 record retention (14/90 days) | planned | survives, **theirs to configure** | a retention knob is a flag; the figures are O-3's |
| M13 | planned, after M16 | **first after the gate**, reshaped as the operator's view (section 4) | it is what replaces our reading of their disk |
| M14 scheduler half | planned | **cut** | a nightly is `systemd-timer` plus `herdctl submit --priority nightly` on their side; Q13's one predicate becomes a documented pattern, zero hub state |
| M14 hook half | planned | survives as documentation | `plan.md:359` already says the hook is theirs (E-42) |
| M14 `coalesce` (D75) | rides with M14 | **separate decision**, recommendation: keep as a small hub feature only if O-9 says they submit `produce` from a hook | it is real queue state; nobody has asked for it live |
| M15 | merged | survives | auth and version refusal are the boundary's enforcement |
| **M15b** (new) | — | **the admin role**: a separate token for `SetSwitch`, `ApproveRunner`, `RemoveRunner` (review 2.1; `server.rs:5108-5119` dispatches them on the requester token) | once they operate, a requester that can remove a runner from *their* fleet is a defect in *our* software |
| M15d self-update | planned, UNBUILT | **cut** | a third deployment path into machines we do not deploy; D66 was the owner's operating requirement for a fleet he operated. Cost of cutting: a non-Nix host is updated by its operator; cost of keeping: a remote code execution channel to design, test and defend |
| M16 | merged | survives as **the deployment reference** | compose, unit, module, notice — the artefacts of section 2; the server stops being "the" deployment and becomes the one that was walked |
| M17 | planned | **becomes "WSL2 is a supported platform"**: `wsl2-check.sh` with its automount and interop checks made failing (review 2.2), the user-unit recipe, E-30's ceiling measured once on real hardware. **The joining itself is theirs** (E-49's thirty minutes become a claim they time, not one we walk) | the PC is the owner's, and the owner is now on the operating side of this line |
| M18 | planned | survives, **inverted**: the gated acceptances are closed by *them* walking `docs/coverage.md` against the release, with us answering | acceptance was always paced by their side (plan.md M18) |
| Requirement readings (D74, D159) | merged, in service | survive, **costed and bounded** in the gate (3.7) — **the one gate item still open**; moving them into the requester's checkout is **proposed to the consumer**, never removed alone | `labels_from` is their mechanism in service; what the hub can promise about it today is nothing |
| **M19** (new) | — | **the handover itself**: the gate of section 3 (built but for 3.7 — D162-D165 in v0.3.4, D166 and D167 merged and untagged), `docs/operating.md` (unwritten), O-1..O-9 agreed or refused (unanswered), `docs/status.md` rewritten as software state (done, 2026-10-23) | this document's own completion criterion; the handover was announced before any of it, so M19 is now what it owes rather than what it waits for |

**Reconciling with the scope review's four waves.** The review ordered:
wave 0 cut the unbuilt (M15d, M14's scheduler, M11/M12 peer-to-peer);
wave 1 security (admin role, WSL2 checks, `PR_SET_DUMPABLE`); wave 2 the
correctness defects by yield (join ignoring `after`, cancel killing the
joined, the serialised verdict, uncounted memory, the two docs, log
truncation); wave 3 what needs a decision or a counterpart (readings,
`excludes`, `drain_first`, fairness); wave 4 operations (M13, retention,
`migrate`, the record re-read).

This plan **follows wave 0 entirely** — the boundary makes each cut
easier, not harder. It **disagrees on what comes next**: the review puts
security first and operations last; this plan puts the gate (section 3)
first, pulls the serialised verdict out of wave 2 and M13 out of wave 4
into it, and lets security follow. The reason is the decision itself:
nothing in wave 1 is worse after a handover than before it, while every
item in the gate is — a trap or a blind spot we operated around becomes
one we shipped. Within wave 1, `PR_SET_DUMPABLE` is two lines and goes
in with the gate release; the admin role is M15b and comes right after
M13; the WSL2 checks ride M17's new meaning. Wave 2's remaining defects
and wave 3 keep the review's order, after M15b. Wave 4's `herd-hub
migrate` becomes more important than the review rates it: it is the only
tool by which a `RECORD_VERSION` move can ever be *theirs* to run.

## 8. The risks of the boundary itself

| # | Risk | What answers it |
|---|---|---|
| R1 | **A fleet nobody watches.** Two capabilities had been dead for a day and the only alarm was us reading their disk. After the handover there is no us | answered: 3.1 makes the common case a refused startup (D165, in v0.3.4), and `herdctl runner list` names the build and every suspension (D166, merged, untagged); M13 makes the rest visible; O-5 puts the check in their generator. Two honest residues: a probe that *becomes* unstartable after start is still only a line on a record, and none of it watches anything until the pin moves off v0.3.2 |
| R2 | **A bug report with no reproduction.** "Herd lost my pack" with no job id, against a hub we cannot see | O-6 and 3.2: a report is a verdict JSON and a record directory, or it is returned with the list. The record is plain files (E-53) precisely so this works |
| R3 | **Version skew that makes every grievance unanswerable.** Already real: the server runs 0.3.2, the grievances were filed against 0.3.0 and one of them (dedup by commit) was fixed in 0.3.1; D167 was reported against runner 0.3.2 with hub 0.3.3 | O-1 (newest two tags), 3.5 (a tag that ships, kept: v0.3.4), 3.6 (the runner's version on the listing, the hub's only in the report). The skew is wider after the gate, not narrower: v0.3.4 holds four fixes the running fleet does not have, and `main` holds two more that no tag carries |
| R4 | **Loss of measurement access.** Every decisive number came from their disk; this agent could already not re-read two of them | section 4: the report, the listing, M13. Residue: we will no longer *discover* a problem; we will be told. That is the decision |
| R5 | **Drift between our reference texts and their copies.** They fork the unit, lower a figure in one place and not the other, and the module header's warning (`nixos-herd-runner.nix:21-28`) comes true | the seven options (3.3) remove the main reason to fork; `docs/operating.md` names the unit as the reference; O-6's report includes `systemctl cat herd-runner` |
| R6 | **Losing the real test bed.** Every milestone's e2e ran against the real binaries and, since M16, against the deployed pair; the consumer's jobs found D158, D139, D128 | the suite's real-binary tests, `release-smoke.sh` and `image-smoke.sh` stay in CI. What is lost is *their* jobs on *our* watch — replaced by M18 inverted: they walk coverage against a release and report. Accept the loss; name it |
| R7 | **The two-sided bug.** The spool (D158) is a permission on their side and a misreport on ours; the readings cap (grievance 6) is a flag on their side and a refuse-not-queue on ours. Each side can wait for the other | every two-sided item in this document names both halves and which section or obligation owns each. The rule: we ship our half without waiting for theirs |
| R8 | **The boundary is organisational, not personal.** The owner of Herd also owns the consumer and the server; the ssh session is one keystroke away and will stay so | the rule in section 5 is for *this project's* work: a fact about a running instance enters this repository only through a report or a release. When the owner reads his own server as the consumer's operator, he writes down what he read as the consumer would |
| R9 | **The gate becomes a wall.** Seven items before "honest" can grow into six months | it did not: six of the seven were built on 2026-10-23, against a budget of five to six days, and four of them shipped as **v0.3.4** the same afternoon. The residue is 3.7 (one day) and a tag for D166 and D167 — a wall only if it waits, which is why 3.0 says the gate is not cleared rather than calling it done |

## 9. The order, and what is already behind it

Steps 1 and 2 are done, on 2026-10-23, and are left here as written with
what actually happened under each. The rest stands.

1. **Done — the owner decided D161**, and took the one question inside
   it the recommended way: no Docker Hub, the image archive on the
   release (D162). `release.yml` publishes the three binaries first and
   attaches the archive, with no credential preflight. It went
   differently in one respect: the v0.3.3 tag was not re-run. **v0.3.4**
   was cut instead and is the first release since v0.3.0, so v0.3.1,
   v0.3.2 and v0.3.3 remain tags with no release, and the fix the
   consumer was missing (D159, the slot-free reading) reaches them in
   v0.3.4.
2. **Done but for one item, and only half of it released** — the gate
   was built in a day rather than five: 3.5 (D162), 3.3 (D163), 3.4
   (D164) and 3.1 (D165) went out in **v0.3.4** at 17:43; 3.2 and 3.6's
   runner half (D166) and D167 merged after it, so they are on `main`
   and **in no release**. The next tag is what finishes this step.
   **Not built at all: 3.7 and `PR_SET_DUMPABLE`** — a reading is still
   costed at zero and unbounded, and `PR_SET_DUMPABLE` is still nowhere
   in the workspace. Still unwritten with them:
   `docs/operating.md` — what the hub prints, what
   the runner prints, the ten lines of `docs/joining.md` "When it does
   not work" extended, O-6's report recipe, the figures and what each
   guards, the example deployment read on 2026-10-23 and marked as such.
   That document is now the next thing after 3.7, because O-5 and O-6
   both point at it.
3. **The message to the consumer** — the owner's to send, drafted by us
   from the grievance response: the probe is not substituted and `kvm`
   and `gpu-venus` are dead until it is; the dedup-by-commit fix is in
   0.3.1 and they run 0.3.2; the pack is short in `{out}` before Herd
   sees it (678 of 678 delivered, 674 of 862 written — review,
   **unverified here**); and O-1..O-9 as a list to agree or refuse.
4. **M13**, reshaped (section 4). Then **M15b**, the admin role.
5. **`docs/status.md` rewritten** as software state, `docs/plan.md`
   rewritten to this table, `docs/coverage.md`'s state of play brought
   from 2026-10-13 to the present (PR #54's finding 2). The architecture
   document's stale spots from its own section 12 fixed on the way.
6. **Wave 2 and wave 3 of the review** in its order; **M17 as a
   platform**; **M18 inverted** when they are ready.

Not started unprompted: anything on the consumer's side, any GitHub
issue, any message. Those are the owner's (CLAUDE.md, "When to ask
Alexa").

## 10. Where the code and the two analyses disagree

Checked on purpose, reported as findings. The two analyses are the scope
review and the grievance response of 2026-10-23. Each finding is left as
it was written and closed, or not, at `5b1021e` on the evening of the
same day.

1. **The release has not shipped since v0.3.0.** Neither document says
   so; both treat the Docker Hub step as "written and never run". It is
   worse: the credential check *refuses the whole run*, so v0.3.1,
   v0.3.2 and v0.3.3 have no binaries on GitHub. Section 3.5.
   **Closed:** D162 took the preflight out and v0.3.4 shipped three
   binaries, `SHA256SUMS` and the image archive. Those three tags are
   still releases nobody ever got.
2. **`extraArgs` exists** (`nixos-herd-runner.nix:355`). The grievance
   response says the seven flags have "no module option", which is
   true, but a reader could think `extraArgs` is the way out. It is
   not: clap refuses the repeated flag. Section 3.3.
   **Overtaken:** D163 added the seven options *and* set
   `args_override_self`, so a repeated flag now takes the value stated
   last and `extraArgs` is a way out after all. The finding is why the
   options exist, not an argument against them.
3. **The first probe sweep is beside the join, not before it.** The
   runner's own comment at `capability.rs:757-759` ("the first sweep
   happens before the runner joins a hub") is stale; `lib.rs:2111-2118`
   explains the change. Consequence for the placeholder bug: the runner
   joins looking healthy and announces the suspension on the same link
   a moment later. Neither analysis draws this. **Half closed:** the
   stale comment is gone — `capability.rs:883` now says the sweep
   happens beside the join and not in front of it — and D165 makes the
   unstartable-probe case a refusal before there is a runner at all. The
   ordering itself is unchanged, so a probe that *becomes* unstartable
   still announces itself a moment after the join.
4. **`herdctl` has eight top-level commands, not ten**: `run`, `submit`,
   `would-run`, `switch`, `runner`, `wait`, `cancel`, `raise`, and
   `runner` holds `approve`, `remove`, `list` (`herdctl/src/main.rs:254-
   470`). The response lists `approve` and `remove` as top-level. No
   consequence. (This plan first wrote nine over the same eight names;
   re-counted at `5b1021e`, where the list is unchanged — D166 added
   flags, not commands. `herd-runner` is the binary that gained one,
   `check`.)
5. **The fleet listing carries no version and no suspension**
   (`herd-proto/src/lib.rs:427-449`). The response proposes the
   serialised verdict as the one fix for grievances 9, 10a and 10c; it
   is, but it does not fix *this*, which is where an operator would
   first look. Section 3.6. **Closed for the runner, on `main`:** D166
   put `ApprovedRunner::facts` on the listing — the build, and each
   suspended capability with its reason. The hub's own version is still
   not there, and no release carries any of it yet.
6. **The hub's log already shows D158 producing false caps**: three
   jobs ended `stopped_by_cap` on disk because a directory could not be
   measured (**server**, `docker logs herd-hub-1`). The response
   suspects grievance 7's 124 "very probably" comes from this; the log
   makes it certain for those three. **Closed on our half, on `main`:**
   D166 ends such a job `SetupFailed` at exit 125, naming the path. No
   release carries it yet, and D158's deposit is still theirs to free
   (O-4).
7. **D160 is taken, and so are 162 to 167.** `docs/decisions.md` ended
   at D159 when this was written; PR #54 appended D160 the same morning
   and six more entries landed the same day. So D161's number is in
   place and its *position* is not: this entry is appended after D167
   rather than slid into the 161 slot, because the file is append-only
   and an insertion into the middle of it is not something a reader of
   the diff can check. That is D148's rule — a decision number cited in
   code is an identifier, not a position — and D167 ruled the same way
   from the other side while this branch was open, counting 161 with 143
   and 154 as a gap "all three under D148's rule". After this merge the
   gaps left are D143's and D154's.
8. **The review's "no non-Nix host exists"** for M15d is true today and
   would stop being true the day the PC joined by release binary. Under
   the boundary it does not matter: the PC's operator updates it.
   Recorded so the cut is not defended with an argument that expires.
9. **`docs/status.md` is stale in three places**, all confirmed on the
   server: `v0.2.0` (line 13) against a pin of v0.3.2; `herd-hub:0.3.1`
   and runner 0.3.0 (line 32) against 0.3.3 and 0.3.2; and "M11's
   registry frames" (PR #54's finding 1). It is the orchestrator's to
   replace, in step 5 above. **Done**, on 2026-10-23: it opens on who
   operates what and carries the three figures. One line of it was
   overtaken by the evening — it says the newest release is v0.3.0 and
   that what is missing is a tag, which v0.3.4 answered a few hours
   later. That is the file's own to carry, not this document's.
10. **A reading weighs zero and is unbounded** (section 3.7). The scope
    review filed the D119/D159 asymmetry under "a sentence, not code",
    and the grievance response repeated it. The source — `server.rs:2459`
    against `:2449`, `job.rs:1114`, `matcher.rs:441` — says it is a
    defect in the accounting. Raised by the orchestrator after the
    review, verified here, and the one place where this plan reverses
    one of the two documents rather than refining it. **Stands**, and is
    the one gate item still open: re-read at `5b1021e`, where the
    mint is at `server.rs:2724`, `memory_need_mib` at `job.rs:1100-1123`
    and the reservation at `matcher.rs:441`, all unchanged in substance.

Everything else both documents assert about the code was re-read and
holds: `@probe@` verbatim in the live file, the seven literal flags on
the live unit, the drain semantics, the sweeper measuring only `files/`,
`version_gap` at the handshake, `JobOutcome` serialisable, no pack frame
on either enum, no `PR_SET_DUMPABLE` anywhere, `SetSwitch` dispatched on
the requester role. Of those, two have moved since: the seven literal
flags are options (D163) and `JobOutcome` is now serialised for a
requester by name (D166). `PR_SET_DUMPABLE` is still absent, the pack
frame is still on neither enum, and `SetSwitch` still rides the
requester token.
