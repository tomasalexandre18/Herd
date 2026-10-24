# 00 — Baseline: the state of Herd at `0a2cf73`

**Date:** 2026-10-24
**Base:** `origin/main` at `0a2cf73b8cc4d0361ee08a73e85f999ca41ec4b`.
**Mandate:** lot T0-A of `MANDAT_HERD_AUDIT_ARCHITECTURE_DECISION_2026-10-24`.
**Method:** reading the repository only. No build, no test, no `cargo`, no
process on this host, nothing on any consumer machine. Every figure below
was taken from the tree at `0a2cf73` or from `git` on this clone, on the
date above.

This is the reference the rest of the dossier is written against. It is
meant to be enough, on its own, for a reader in 2027 with the repository
and nobody from this project to answer: *what was this software, what did
it promise, what was known to be wrong with it, and what exactly was the
commit that was audited.*

**Marks.** **F** = observed in the tree or in `git` at `0a2cf73`, with a
path, a line or a command. **H** = hypothesis. **I** = unknown, not
established here. A figure that is only an **I** is never written as if it
were measured.

---

## 1. What the software is

Herd is distributed build and test execution, "GitHub-runners shaped",
independent of any particular project (`README.md:1-11`, F).

A **requester** asks for work on a commit — build, test, check — without
naming a machine. The **hub**, one instance, picks a **runner** whose
declared capabilities fit, and returns the exit code and the logs. Runners
dial out to the hub; nothing inbound is needed for a runner to work. Each
job runs under an explicit memory cap and time cap, and a runner's owner
can stop it in seconds.

Four words, used with no synonyms anywhere in the repository (`CLAUDE.md`,
F): **hub** (the service), **runner** (a client that offers capacity and
executes jobs), **requester** (whoever submits, through `herdctl`), **job**
(one unit of requested work with requirements, a priority, an exit code and
logs).

The first consumer is the chatOnS OS project. Their issues say "client"
where this repository says "runner" and "agent" where it says "requester";
`CLAUDE.md` instructs translation rather than adoption.

Herd is **shipped, not operated** (invariant 9, D161, F). The consumer runs
the hub container, the runner units, the tokens, the disk and the version
pins. What this project owns is what a tag produces. Every figure in this
dossier about a running system is therefore second-hand, and says so.

---

## 2. The commit, and the thing people will call it

**`0a2cf73` carries no tag.** F: `git tag --points-at 0a2cf73` is empty.

**Tag `v0.3.7` is `3da48f5`**, two merges earlier. F:
`git rev-parse "v0.3.7^{commit}"` → `3da48f50c6412815d313674d0ea90ce7eb4d1444`,
`Merge pull request #69 from tomasalexandre18/cut-v0.3.7`. The audit mandate
labels the base `v0.3.7`; that label is wrong, and `03c` says so
independently (`03c §0.2`). The base itself is right: both audits read
`0a2cf73`.

**What lies between the tag and the base, and what does not.** Seventeen
commits, two merges (PR #71 `orchestration-remediation`, then PR #70
`hotfix-dev-lane`), 68 files, +3 196 / -84 lines. F:
`git diff --stat 3da48f5 0a2cf73`.

**Not one line of product code.** F:

```
git diff --name-only 3da48f5 0a2cf73 \
  -- herd-proto herd-hub herd-runner herdctl \
     Cargo.toml Cargo.lock nix flake.nix flake.lock
```

returns nothing. Everything between the tag and the audited base is
`docs/`, `.claude/`, `.github/workflows/`, `scripts/`, `deploy/README.md`,
`CLAUDE.md` and `.gitignore`. So the Rust audited at `0a2cf73` **is**
`v0.3.7`'s Rust, byte for byte. That does not excuse the mislabel — a base
named wrong is a finding nobody can reproduce in six months — but it does
mean no finding in this dossier changes if the reader checks out the tag
instead.

The workspace version string at `0a2cf73` is `0.3.7`
(`Cargo.toml:6`, F).

### The release line

| Tag | Commit | Date |
|---|---|---|
| v0.3.0 | `a3cc264` | 2026-10-16 |
| v0.3.1 | `95cb4d0` | 2026-10-19 |
| v0.3.2 | `f01d489` | 2026-10-19 |
| v0.3.3 | `36ef9ab` | 2026-10-19 |
| v0.3.4 | `9866e9e` | 2026-10-23 |
| v0.3.5 | `21cc7a3` | 2026-10-23 |
| v0.3.6 | `d57f23a` | 2026-10-24 |
| v0.3.7 | `3da48f5` | 2026-10-24 |

F: `git tag --list 'v0.3.*'` with `git rev-parse` and `git log -1 --format=%cs`
on each.

**Open pull requests on the date of this document:** #72
`herd-own-claude-root`, #67 `status-after-036`. F: `gh pr list --state open`,
2026-10-24. This is a snapshot and will not stay true.

### What was running, and why it is not stated here

`docs/status.md` (dated 2026-10-23) reports the consumer running
`herd-hub:0.3.3` and a server runner at 0.3.2, about 90 GB across 1 571 jobs
on the hub volume, and about 7.4 GB of runner spool no runner can remove
(D158). Those are **their** measurements on **their** hosts, relayed under
invariant 9. Nothing in this dossier re-measured them, and the live fleet
state at the moment of the audit is **I**.

---

## 3. The four crates

`herd-proto` is a library and does no I/O. The other three are binaries
with a library beside them, which is what lets the end-to-end tests run a
real hub, a real runner and a real requester in one process. Every arrow
into `herd-proto` is a real dependency; every edge between the other three
is a **dev-dependency only**, so no binary links another
(`docs/architecture.md §2`, F).

| Crate | What it carries |
|---|---|
| `herd-proto` | every message (`ToHub`, `FromHub`), `PROTOCOL_VERSION`, the JSON framing, the capability algebra, the declaration types, the output types, host size and parallelism, byte parsing. Invariant 4: nothing is sent ad hoc. |
| `herd-hub` | the service: the listener and handshake (`server.rs`), the job directories and index (`store.rs`), the claim step (`matcher.rs`), queue order and ageing (`queue.rs`), the record and `RECORD_VERSION` (`job.rs`), the roster and salted token hashes (`roster.rs`), and `durable.rs`, the only place the hub writes to a filesystem. |
| `herd-runner` | the client: the dial and backoff (`lib.rs`), one job's whole task (`job.rs`), the executor steps (`step.rs`), confinement (`confine.rs`, `cgroup.rs`, `blockio.rs`, `privilege.rs`), the data root (`root.rs`), reaping (`reclaim.rs`), capability detection and busy probes (`capability.rs`), output collection (`outputs.rs`), the held-results spool (`spool.rs`), host metrics (`metrics.rs`), the owner's switch (`switch.rs`), lendable memory (`room.rs`). |
| `herdctl` | the requester CLI: submit, follow, wait, cancel, raise, dry run, switch, runner approve (`lib.rs`), the CLI and exit codes (`main.rs`), strict TOML reading (`declaration.rs`), commit resolution in the requester's own git (`checkout.rs`), the machine-readable verdict (`verdict.rs`). |

The full module-by-module table is `docs/architecture.md §2`; it is not
copied here, because it changes when the behaviour changes and this file
should not become a second copy of it.

### Size, measured here

| Crate | `src/*.rs` | `tests/*.rs` | `.rs` files | integration files |
|---|---:|---:|---:|---:|
| `herd-hub` | 22 130 | 15 508 | 31 | 21 |
| `herd-runner` | 17 064 | 4 570 | 22 | 4 |
| `herdctl` | 5 794 | 5 772 | 14 | 9 |
| `herd-proto` | 6 790 | 1 416 | 7 | 1 |
| **total** | **51 778** | **27 266** | **74** | **35** |

F: `wc -l` over each crate at `0a2cf73`. 651 `#[test]` / `#[tokio::test]`
attributes across the workspace (F, `grep -c`).

Two cautions a later reader will need. **`src` is not "production code":**
it includes every `#[cfg(test)]` module, and in this project those are
large. And the mandate's own table (its §1.1) gives `herd-hub` 21 757
production lines and 649 test functions; measured here the figures are
22 130 and 651. The difference is the mandate reading an earlier commit,
not a discrepancy worth chasing — but the mandate's table is not a
measurement of `0a2cf73`.

**No conclusion is drawn here from the size of a file.** `server.rs` is
7 985 lines and `matcher.rs` 3 901 (F, confirmed in `03b §0`). That is a
fact about the tree and an argument for nothing by itself; `03a §6` and
`03b` argue coupling from dependencies, states and locks instead.

---

## 4. The gates

Four Rust gates, all run through the Nix flake's dev shell — nothing is
installed globally (D12):

1. `nix flake check --no-build`
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `./scripts/test.sh`

`scripts/test.sh` runs `cargo test --workspace` and then prints every
assertion the host could not make (§5). `CLAUDE.md` names gates 1-4 and
`scripts/test.sh` as the per-commit rule; no other suite is required.

Two documentation gates run beside them:

- **`scripts/check-docs.sh`** enforces invariant 8 mechanically: `CLAUDE.md`
  at most 150 lines, `docs/status.md` at most 90, prose wrapped at 79
  columns outside tables and fenced code, no duplicate decision number, no
  `D-NEXT` placeholder left behind, and more. F: `scripts/check-docs.sh`.
- **`scripts/check-orchestration.sh`** tests the controls themselves: it
  copies the repository to a scratch directory, breaks it in each of the
  ways `check-docs.sh` claims to catch, and fails unless `check-docs.sh`
  goes red on each one and green on the unmodified copy; it also exercises
  the two guard hooks. F: `scripts/check-orchestration.sh:1-40`. A control
  only ever seen green is not known to work; this is the answer to that.

CI (`.github/workflows/ci.yml`, F) is the merge gate and nothing else is.
Job `check` runs the four Rust gates on `ubuntu-latest` and then
`scripts/report-skips.sh`, which turns each skipped assertion into a
warning annotation on the run. Job `docs` runs `check-docs.sh` and
`check-orchestration.sh`. Direct pushes to `main` are blocked by branch
protection; `main` moves only on Alexa's explicit approval.

Two further scripts exist and are not merge gates: `scripts/image-smoke.sh`
and `scripts/release-smoke.sh`.

---

## 5. The loud skips

**The rule (D27, invariant 5).** An assertion that needs a host capability
this machine lacks is *skipped with its reason*, never failed and never
silently passed. A suite that is red on a host that is merely different is
a suite people learn to ignore; a suite that is green when a third of it
did not run is worse.

**The machinery (F).** `herd_runner::testing::skipped(test, why)`
(`herd-runner/src/testing.rs:25-47`) writes `HERD-SKIPPED: <test>: <why>`
to stdout, to stderr, **and** to the file named by `$HERD_SKIP_LOG`.
`cargo test` swallows a passing test's stdout, so the file is the only one
of the three it cannot hide: `scripts/test.sh` sets `HERD_SKIP_LOG`, runs
the suite, then prints the distinct lines (`sort -u`) under a banner saying
a green run does **not** mean these were checked. Each line is appended
with a single `write_all`, not `writeln!`, because two tests skipping at
the same moment used to interleave between the text and the newline and
land on one line (D151).

**Where the skips live (F).** Skip sites are in
`herd-runner/src/{confine,root,outputs}.rs`,
`herd-runner/tests/{confinement,declared_command,busy_probe}.rs` and
`herd-hub/tests/{slots,priorities,capabilities}.rs`. They are not all
reached on every host: the number that actually fires is a property of the
machine, not of the tree.

**The count is `I`, and the repository contradicts itself about it.**
`docs/status.md:49` and `docs/coverage.md:113` both say the suite reports
**12** loud skips on a host with no delegated subtree on a block device,
"every one of the twelve a cgroup-delegation or ZFS `io.max` assertion".
D151 (2026-10-16) records that a count of **12** was itself the bug — two
skips on one line — and that after the `write_all` fix "the gate run for
this branch reports **13**". The fix is in the tree at `0a2cf73`
(`testing.rs:46`), and the 12 in `coverage.md` was written on 2026-10-23
(commit `ff3a525`), a week *after* the fix. So either a skip site was
removed between the two and the count genuinely returned to 12, or a stale
figure was restated. **I**: this document was not allowed to run `cargo`,
and the live count on any host is therefore unverified here.

**The split is `I` too.** The split into "10 cgroup-v2 delegation
assertions and 2 ZFS `io.max` assertions" is stated in no document in the
tree. What the tree says is the weaker, unsplit claim quoted above. A
reader who needs the split must run `./scripts/test.sh` and read its
banner; that is a two-minute measurement and it is the right way to get it.

---

## 6. The frozen formats

| Constant | Value | Where |
|---|---:|---|
| `PROTOCOL_VERSION` | **2** | `herd-proto/src/lib.rs:113` |
| `RECORD_VERSION` | **1** | `herd-hub/src/job.rs:52` |

F, both read at `0a2cf73`.

**The check is `!=`, in all three places** (F):

- `herd-hub/src/server.rs:2141` — the handshake. A peer whose
  `protocol_version` is not exactly 2 is refused, by a sentence naming both
  numbers.
- `herd-hub/src/store.rs:1143` and `:2297` — reading a record. A record
  whose `record_version` is not exactly 1 is refused.

`!=` and not `<` is the load-bearing detail: it blocks the move **up** and
the move **back** alike. A hub rolled back to an older binary refuses the
state root the newer one wrote, and a newer runner is refused by an older
hub exactly as an older runner would be. There is no migration and
`herd-hub migrate` is unbuilt (`docs/handover.md:67`, F), so a fleet
crosses a version line only by a deliberate, coordinated upgrade of every
machine in it — and cannot cross back.

**What D73 actually says.** D73 (2026-10-13) is the *policy* entry, not the
freeze of these two values. Before M16 both numbers were free-running
development counters: every format break advanced the one it touched,
nothing promised compatibility, no migration was ever written, and a stale
state root was refused by name and deleted by its operator. At M16, the
deployment milestone, both were **set back to 1** and the freeze begins
there. After that a bump is a compatibility statement.

So the pair is 2/1 and not 1/1 because the protocol has been bumped once
since the reset: D148 (2026-10-16) paid for version 2 to carry a record's
files through the hub, and deliberately bundled M11's registry frames and
the runner's unknown-frame catch-all into the same break so it would be
spent once. `docs/coverage.md:117-122` records the pair as holding since
the v0.3.0 freeze. Saying "frozen since D73" is a reasonable shorthand for
the regime; the exact statement is **D73 set the regime, D148 set the 2**.

---

## 7. The repository's invariants

Nine at `0a2cf73`, in `CLAUDE.md`, each one changeable only with Alexa's
approval (F). In brief:

1. **Stay specialized.** No workflow language, no plugin system, no cloud,
   no multi-tenant surface. A feature that smells like generic CI is
   refused.
2. **A runner never destabilizes its host.** Explicit memory cap and time
   cap on every job; a clean stop in ≤ 5 s when the owner asks; no new jobs
   while draining.
3. **Runners dial out for control; no port is ever opened on a router.** A
   private mesh interface may serve content to peers (D10, D11), bound to
   that interface only. Herd itself builds and requires no network.
4. **The protocol is the contract.** All messages in `herd-proto`, a
   version in the handshake, nothing ad hoc.
5. **Done means a passing test.** Unit tests plus one real end-to-end
   hub+runner test per milestone; a skipped assertion is printed as skipped
   and never reported as passed (D27).
6. **A requester never names a machine.** Selection is the hub's, from
   declared capabilities and priority.
7. **No secrets in the repo.**
8. **Docs discipline.** `status.md` is state and is replaced, never
   appended to; `decisions.md` is append-only with one dated entry per
   decision and numbers that are identifiers, so gaps stay (D148);
   `plan.md` holds the milestones, `coverage.md` maps the consumer's
   numbered expectations, `orchestration/board.md` is the one list of open
   work. `check-docs.sh` enforces what it can.
9. **Herd is shipped, not operated (D161).** Nobody here logs into the
   consumer's hub, runner, host or NixOS config.

Two of these are newer than the tag: invariant 9 and the board clause in
invariant 8 arrived with PR #71, between `v0.3.7` and `0a2cf73`.
`CLAUDE.md` at `3da48f5` lists **eight** invariants (F,
`git show 3da48f5:CLAUDE.md`).

---

## 8. The two known-unstable tests

Neither is a new discovery of this dossier; both are recorded, and both are
examined in `03b §9` and `03c §2.8`.

**`herd-hub` `slots::a_runner_killed_mid_job_gives_its_slot_back`** —
`herd-hub/tests/slots.rs:206` (F). A runner is killed mid-job and the test
asserts the slot comes back.

**`herd-hub/tests/follower_alive.rs`,
`a_quiet_running_job_keeps_its_follower_hearing_from_the_hub`** — the
failing assertion is the `assert!` that begins at **line 230**, whose
condition `longest < spread / 2` is on **line 231** (F). Both numbers are
in circulation for this reason: `docs/orchestration/board.md` ORCH-022 and
`03b §9` cite 230, the panic location; `03c` cites 231, the condition. They
are the same assertion.

ORCH-022 (F, `board.md`) records the evidence: CI job `check` went red on
head `b40c8dc` of PR #71 — **a diff of three Markdown files** — and the
same test was green on `46f3fb1` minutes earlier. CI run 40114565027,
2026-10-24. It is open, tracked for the technical audit, and no change to
the test was made under the orchestration mandate.

Why this matters more than two red runs. Invariant 5 is the project's whole
definition of done. A test that is red on a prose commit teaches readers to
re-run CI instead of reading it, and a suite people re-run until it is
green is a suite that has stopped being evidence. These two are the only
ones known to behave that way at `0a2cf73`; whether there are others is
**I** — it needs repeated runs, which this document did not make.

---

## 9. The two real incidents

These are the reason the mandate exists. Both are recorded in
`docs/decisions.md` with their figures; everything in this section is
quoted from there (F as to what the entry says) and was measured by the
first consumer or on the host D171 was written on, not here.

### 9.1 Host exhaustion — D169, 2026-10-24

**The first consumer's nightly submitted 214 jobs in one second** at
03:10:56 — 48 `test`, 166 `test-pack`, priority nightly, commit
`923fe476c0de` — against a fleet of **one runner**, hub and runner both
0.3.5.

Every one of those jobs needed its requirements read on a machine first
(D74). A reading holds no slot, empties no host and waits for no probe
(D159), so **nothing anywhere counted them**: the matcher put some two
hundred readings on that one machine at once, on a 20 GiB, eight-thread
lend. The host saturated. The runner's own kvm probe then timed out in a
loop from 03:11 to 03:50 — "did not answer within 2000 ms and was killed" —
which suspends the capability (D49), so jobs needing kvm found no runner
that could ever run them.

**201 of the 214 ended 125 with no test run.** Six passed, five failed, one
was memory-capped, and one job waited **6 h 55** for a reading that never
came back.

At 10:09 the batch resubmitted the 203 groups that had ended 125, once
each. The same thing happened at sharper figures: **255 `herd-job` cgroups
at once and a host load of 399 at 10:25**. They stopped the batch at 10:26
and the machine was normal by 10:29.

**Nothing crashed, either time.** Neither the runner nor the hub went down;
the host was simply buried. D169 calls that the honest description and a
stronger argument for a bound than a crash would have been: what is being
protected is not a process but a machine that stays usable enough for its
probes to answer inside two seconds. **255 is the observed ceiling of "no
bound at all".**

D169's fix: readings take a place in the claim step's seat accounting, with
a bound of two per machine (`--readings-at-once`, `HERD_READINGS_AT_ONCE`);
a reading over the bound queues rather than fails; a *lost* reading is read
again instead of refusing the job; and the watchdog asks a connected runner
before believing its own clock.

### 9.2 The hub blocked — D171, 2026-10-24 12:14

An hour after the release that caused it. 0.3.6 shipped D169 at about
11:10; the consumer installed it and ran a full pass at 12:10:20 — **214
job groups, one runner**, `--readings-at-once` at its default of 2.

**D169's bound worked.** The runner held at most four `herd-job` cgroups,
load 16-21, and not one job ended 125. That is the whole of what D169 was
for, and it held.

Then the hub stopped answering. From about **12:14** `herdctl runner list`
timed out — 10 s in their collector, then three tries at 30 s and one at
180 s, every one `rc 124`. The runner's journal says the other half: *"the
hub has said nothing for 60s, so this link is dead even though its socket
is open"* at 12:14:15, 12:15:53 and 12:17:29, redialling each time, and at
12:18:29 *"j0000002485 ended while there was no hub to tell"*. By then the
runner held **nought** cgroups against a queue of **184 waiting**: the pass
had stopped moving.

**The hub container was not busy — 1.8-3.9 % of a CPU, 209 MiB, 13 PIDs. It
was blocked, not saturated.** That distinction is the single most important
sentence in this dossier's starting material, and it is the one a reader
should carry into `03b`: the failure was not a shortage of CPU and will not
be fixed by giving the hub more of it.

What was blocking it, from the thread states and the state volume captured
before the restart: of the thirteen threads, exactly **one** was always in
`D`, uninterruptible disk wait, and it moved between two different tokio
workers across samples; the rest were waiting. **In two minutes the hub
modified 775 files** — `index/state.json`, and for every queued job both
`jobs/<id>/job.json` and `jobs/<id>/events.jsonl`. One queued job,
`j0000002656`, had accumulated **94 lines** in its `events.jsonl`, each a
full *"stays queued: queued 65th of 165 …"* explanation. Their hub log
carried **20 683** `stays queued` lines since 12:10, **1 931** of them
inside one three-minute window.

The defect D171 names: E-04's waiting sentence wraps a *cause*, which
changes rarely, around a *snapshot of the whole queue* — "queued 74th of
184" — which changes whenever any other job arrives, starts or ends.
`record_waiting` judged "news" by comparing the whole sentence, so one job
moving rewrote the record and appended a history line, each `sync_all`'d
with its directory, for every one of the 184 jobs about which nothing had
happened. Before D169 this could not bite, because a one-runner fleet never
stood a deep queue. **D169's bound is correct and is the one thing that
worked that night; what it changed is that the queue is now deep**, putting
a cost that had always been linear in the queue in front of a queue two
hundred long.

The rest follows with no further defect, and D171 says so in the entry: a
dispatch pass is a synchronous function called straight from async callers,
so a pass that spends seconds in fsync is a tokio worker unavailable for
seconds; a dozen callers each start one and serialize on
`State::dispatching`; with no worker left nothing is polled, including the
answer to a health report and the accept of a requester asking for a fleet
listing. Measured on the host D171 was written on: one pass over **400**
queued jobs took **3.21 s** writing a line for each and **62 ms** writing
none.

D171's fix: news is what changed about *this* job — its cause and its
weighed standing — not what changed around it; the full sentence moves to
memory (`Peers::waiting`) and is what every reader is given.

**What D171 explicitly did not do**, and the dossier should not be read as
if it had: it did not insulate the hub's persistence from its reactor. That
frontier is named in the entry and deferred, and it is what `03b`'s first
finding is about.

---

## 10. What was verified here, and what was not

**Verified (F).** The commit, its tag relationship and the absence of
product-code change between `3da48f5` and `0a2cf73`; the tag list; the four
crates, their modules and their measured sizes; the four Rust gates and the
two documentation gates as the files define them; the skip machinery and
where its call sites are; both version constants and all three `!=` checks;
the nine invariants and the two that postdate the tag; the location of both
unstable tests and ORCH-022's evidence; what D169 and D171 say.

**Not verified (I).**

- **The live skip count and its split.** No `cargo` was run. The tree
  states 12 in two places and 13 in a third, as §5 sets out.
- **Whether any other test is unstable.** Needs repeated runs.
- **Every figure about a running system** — the hub and runner versions in
  service, the 90 GB across 1 571 jobs, the 7.4 GB of unremovable spool,
  the fleet's state during the audit. Invariant 9 puts those hosts outside
  this project; they are reported, not measured here, and a request for
  anything more goes to the consumer's operator.
- **Backup and restore.** There is no backup or restore procedure owned by
  this repository. `docs/handover.md:67` assigns the hub volume, its
  filesystem and **its backups** to the consumer. No restore exercise is
  attested anywhere in the tree. Per the mandate's T0-A gate, that is a
  **blocker before any future operational change**, not before reading the
  code — and the exercise, when it happens, happens off production.
- **The incidents' causality.** §9 reports what D169 and D171 record. A
  demonstrated root-cause analysis is the work of `03b §7` and the
  experiments listed in `03b §10` and `03c §6`, none of which were run.

**Deliberately not done here.** No build, no test, no clippy, no load on
this host, no change to any Rust file, no tag, no release, no touch of
`main`. Nothing in this document authorizes a modification of the software.

---

## 11. Prerequisites for any later test campaign

Stated so that the first person who proposes an experiment does not have to
rediscover them.

- **An isolated host or a confined scope.** This machine is shared with a
  system in production and with other agents. The experiments `03b §10` and
  `03c §6` describe put a deep queue and real fsync load on a hub; none of
  them may run where a production service can feel it.
- **Resources named in advance** — CPU, RAM, disk, I/O, and a test state
  root that is not any live one — because the failure under study *is* an
  I/O wait, and an experiment that shares a spindle with anything else
  measures the neighbour.
- **A delegated cgroup v2 subtree on a block device**, if the point is to
  close the skips of §5 rather than to measure the hub. Without it the same
  assertions skip again and the campaign learns nothing new.
- **The gates stay the gates.** A campaign does not get its own looser
  definition of green, and D27 holds inside it: an assertion that could not
  be made is reported as not made.
