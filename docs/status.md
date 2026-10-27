# Status — 2026-10-27

State now. Replaced every session, never appended to. **What is owed and
what Alexa has not decided lives in `docs/orchestration/board.md`**
(invariant 8); this file describes the software. Reasoning: `decisions.md`;
expectations: `coverage.md`; the audit: `remediation/`.

## Who operates what

**The first consumer operates the running system** (D161, invariant 9):
the hub container and its volume, the runner units and their data roots,
the capability files and probes, the tokens, the disk, the version pins.
Ours is what a tag produces: protocol, crates, `deploy/`, releases, docs.

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24: v0.3.6 plus a deep
queue that no longer writes a non-event per queued job per pass (D171);
v0.3.6 carried the bounded reading (D169). v0.3.4 onward are published
(D162); a **`v*.*.*-dev*` tag** publishes a pre-release in four to six
minutes, built as a real tag builds (D172).

**`main` is 137 commits past v0.3.7** counted on PR #85's head
(`origin/main` `cd59ef5` was 121), and the workspace version still reads
`0.3.7` (`Cargo.toml` line 6; read 2026-10-27). Nothing merged since the
tag has shipped, the fixes below included. `PROTOCOL_VERSION` is **2**
and `RECORD_VERSION` **1** (`herd-proto/src/lib.rs:114`, `job.rs:52`).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact about their machines
reaches us as a report. Each figure below carries the date it reached us.

- **Versions, reported 2026-10-24:** their nightly ran on **0.3.5**
  (D169); they then installed **0.3.6** and ran a full pass (D171).
  **Whether v0.3.7 reached them is not recorded here.**
- **Disk, read 2026-10-23 and not since:** about **90 GB across 1 571
  jobs** against D152's 200 GB budget, nothing ever evicted, plus **7.4
  GB of runner spool no runner can remove** (D158). **Their state root
  is `/mnt/ssd_980`, ext4 on NVMe, 983 GB with ~341 free** (reported
  2026-10-27, ORCH-076) — the filesystem this workstation also builds
  on, so it is shared.
- Herd is their only path: a job Herd cannot run fails their build. Long
  jobs, KVM and exclusive `rig-cpu` worked on 2026-10-23.

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB by
default (`herd-runner/src/confine.rs:82`). **A skip count is per host**
(D27, ORCH-070): **12** here — **10** for cgroup v2 delegation
(`herd-runner/tests/confinement.rs`), **2** for a ZFS data root with no
hard `io.max` (`confine.rs:1118`, `:1220`) — and **15** on CI; D151's 13
is not reproducible (`05-experiments-review.md` §6). **The suite is red
here on ORCH-069's two assertions and nothing else**, both comparing a
figure from this host's live `MemTotal` against constants for a 17 GiB
machine. **CI settles only one**: `slots.rs:744` is *skipped* there and
a skip is never a pass (D27); `admission.rs:419` runs green. **D179**:
an incomplete run reports its skips as unknown, never as none.

**Eight defects are fixed and in no release.** **D175** an inline file
whose bytes did not hash to its digest exited 0; **D176** a verdict could
name a commit the tree was never at; **D177** a live-job map keyed by job
id let a second attempt replace the first; **D178** the watchdog judged
an attempt it had just placed on the silence of the one before it;
**D180-D182** (C5) a runner holds its own ceiling, announces it in
`Hello`, and a refusal there spends none of its goes — one new
`JobOutcome` an older hub refuses to read; and **ORCH-094**, the hub
re-minted job ids as soon as anything removed a record. No version moves.

`docs/remediation/` holds fourteen pieces: the baseline, audits A and B,
their review, two experiments and their review, `06` (a junction in five
posts), `07` (architecture, **adopted by D183**), `08`, `09` (C5), `10`
(D103's order on ext4), `11` (each `fsync` priced), `12` (**the ordering
oracle**: both record syncs return before the event's first byte).

## What the software still does not do

Each of these is a board row; the board holds its owner and next step.

1. **The reaper's janitor list is unbuilt** (D134): a failing `up` leaves
   workspaces behind and Herd records only that `down` failed (ORCH-012).
2. **A requirement reading still weighs nothing** — no memory, no tmpfs
   (`herd-hub/src/matcher.rs:421`); D169 bounded only how many run at
   once. **Durable writing is 99.5 % of a runner junction** at `N` =
   1 000 and nothing ever deletes a job directory (`06`; ORCH-021).
3. **M11's pack half does not exist**, with no frame on either protocol
   enum, so E-12 and E-17 are undelivered (ORCH-020).
4. **The hub has no stop path** (ORCH-081). **M17's second machine is
   prepared, not joined**, **one shared token** serves every requester,
   and **three tests are known unstable** (D149; ORCH-013, -014, -022).
