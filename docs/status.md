# Status — 2026-10-27

State now. Replaced every session, never appended to. **What is owed and
what Alexa has not decided lives in `docs/orchestration/board.md`**
(invariant 8); this file describes the software. Reasoning is in
`docs/decisions.md`, expectations in `docs/coverage.md`, the audit in
`docs/remediation/`.

## Who operates what

**The first consumer operates the running system** (D161, invariant 9):
the hub container and its volume, the runner units and their data roots,
the capability files and probes, the tokens, the disk, the version pins.
Ours is what a tag produces: protocol, crates, `deploy/`, releases, docs.

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24. It is v0.3.6 plus one
change, a deep queue that no longer writes a non-event per queued job per
pass (D171); v0.3.6 is what carried the bounded reading (D169). v0.3.4
onward are all published, the credential preflight that killed
v0.3.1-v0.3.3 being gone (D162). A **`v*.*.*-dev*` tag** publishes a
pre-release in four to six minutes, built as a real tag builds (D172).

**`main` is 100 commits past v0.3.7** at this merge (`96fc10b` was 90),
and the workspace version still reads `0.3.7` (`Cargo.toml` line 6;
read 2026-10-27). Nothing merged since the tag has shipped, the four
fixes below included. `PROTOCOL_VERSION` is **2** and `RECORD_VERSION`
**1** on `main` (`herd-proto/src/lib.rs:114`, `herd-hub/src/job.rs:52`).

## What is known about the consumer's machines, and when

Nothing in this project has looked at their hosts since **2026-10-23**,
and nothing here may (D161). Each figure carries the date it reached us.

- **Versions, reported 2026-10-24:** their nightly ran against one runner
  with hub and runner both **0.3.5** (D169); they then installed
  **0.3.6** and ran a full pass on it (D171). **Nothing here records
  whether v0.3.7 was installed** — one passing clause calls their pass
  that day a pass on v0.3.7 (`remediation-report.md` §1), which is not a
  version report. Their versions today are not known here.
- **Disk, read 2026-10-23 and not re-read since:** about **90 GB across
  1 571 jobs** against D152's 200 GB budget, the sweeper having **never
  evicted anything**, plus **7.4 GB of runner spool no runner can
  remove** (D158).
- Herd is their only path: a job Herd cannot run fails their build. Long
  jobs, KVM and exclusive `rig-cpu` worked on 2026-10-23.

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB by
default (`herd-runner/src/confine.rs:82`). **A skip count is per host**
(D27, ORCH-070): **12** here — **10** for cgroup v2 delegation
(`herd-runner/tests/confinement.rs`), **2** for a ZFS data root with no
hard `io.max` (`confine.rs:1118`, `:1220`) — and **15** on CI; D151's 13
is not reproducible (`05-experiments-review.md` §6). **The suite is red
here and green on CI**: ORCH-069's two assertions are a property of this
host plus the code, so CI is the verdict. **D179**: every binary runs,
and an incomplete run reports its skips as unknown, never as none.

**Four defects are fixed and in no release** (2026-10-25). **D175**: an
inline file whose bytes did not hash to its digest exited 0. **D176**: a
verdict could name a commit the tree was never at. **D177**: a runner's
live-job map keyed by job id alone let a second attempt replace the
first. **D178**: the watchdog judged an attempt it had just placed on the
silence of the one before it. No version moves for any of them. **Not on
`main`:** PR #85 (ORCH-064, C5 part 3), which adds two additive
`herd-proto` fields — a runner's own ceiling and a free refusal (E-21).

`docs/remediation/` holds ten pieces: the baseline, audits A and B, their
contradictory review, two experiments and the review of them, `06` (a
junction in five posts), `07` (target architecture, recommended and
**not** adopted), `08` (the unsynced-write cost).

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
4. **The hub has no stop path** and invariant 2 is enforced in the hub
   only, not at the runner (`03b` H3). **M17's second machine is
   prepared, not joined**, **one shared token** serves every requester,
   and **three tests are known unstable** (D149; ORCH-013, -014, -022).
