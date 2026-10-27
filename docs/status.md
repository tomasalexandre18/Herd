# Status — 2026-10-27

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8).

## Who operates what

**The first consumer operates the running system** (D161, invariant 9):
the hub container and its volume, the runner units and their data roots,
the tokens, the disk, the version pins. Ours is what a tag produces.

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24: v0.3.6 plus a deep
queue that no longer writes a non-event per queued job per pass (D171);
v0.3.6 carried the bounded reading (D169). v0.3.4 onward are published
(D162); a **`v*.*.*-dev*` tag** publishes a pre-release (D172).

**`main` is 219 commits past v0.3.7** counted on PR #102's head, and the
workspace version still reads `0.3.7`. Nothing merged since the tag has
shipped. `PROTOCOL_VERSION` is **2** and `RECORD_VERSION` **1**.

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact about their machines
reaches us as a report. Each figure below carries the date it reached us.

- **Versions, reported 2026-10-24:** their nightly ran on **0.3.5**
  (D169), then **0.3.6** with a full pass (D171). **Whether v0.3.7
  reached them is not recorded.**
- **Disk:** about **90 GB across 1 571 jobs** against D152's 200 GB
  budget, nothing ever evicted (read 2026-10-23). **125 runner spools
  holding 65 GB**, under seven days old (2026-10-26); a runner now
  **sweeps `outputs/` at startup** (ORCH-092) and re-offers each, and
  sets aside — never deletes — what no hub claims (**ORCH-108**).
  **Their state root is `/mnt/ssd_980`, ext4 on NVMe, 983 GB with ~341
  free** (2026-10-27, ORCH-076) — the filesystem this workstation also
  builds on, so it is shared.
- **The shipped seven-day output floor cannot be kept at their workload
  at any budget:** it needs **~750 GB** against a **~451 GB** theoretical
  and ~250 GB practical ceiling (`disk-policy.md` §2.4). Whether the
  default stays or drops is **ORCH-105**, with Alexa.
- Herd is their only path: a job it cannot run fails their build; long
  jobs, KVM and exclusive `rig-cpu` worked on 2026-10-23.

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB by
default (`confine.rs:82`). **A skip count is per host** (D27, ORCH-070):
**12** here, **14** on CI; D151's 13 is not reproducible (`05` §6).
**D179**: an incomplete run reports its skips as a floor, not a count.

**The suite is green here for the first time: 836 passed, 0 failed, 12
skipped** (PR #102). **ORCH-069 is done**: two assertions declared a lend
larger than the cgroup enclosing the runner leaves free, so it was clamped
(D119) and the hub's sentences carried it; both fixtures now declare 2 GiB
and `slots.rs` skips nowhere. **ORCH-107** is what is left: under several
heavy agents the real-hub tests **time out**, asserting nothing wrong.

**Eleven changes are in no release.** Eight defects — **D175** a file whose
bytes did not hash to its digest exited 0; **D176** a verdict could name
a commit the tree was never at; **D177** a live-job map keyed by job id
let a second attempt replace the first; **D178** the watchdog judged an
attempt on the silence of the one before it; **D180-D182** (C5) a runner
holds and announces its own ceiling, and a refusal there spends none of
its goes; **ORCH-094** the hub re-minted ids once anything removed a
record. Two new behaviours: **D184** `SIGTERM` stops the hub instead of
killing it, bounded at five seconds and non-zero on an overrun; and
**ORCH-092**, a runner sweeps its output spool at startup. One cost cut,
**D183 step A** (ORCH-083): the `waiting` event line alone is unsynced,
one of thirteen, the record still fully synced, 3.10 ms → 2.10 ms.

`docs/remediation/` holds fifteen pieces: the baseline, audits A and B,
their review, two experiments and their review, and `06`-`13`: junction,
architecture (**D183**), C5, ext4 order, prices, oracle, 308 crash points.

## What the software still does not do

Each of these is a board row; the board holds its owner and next step.

1. **The reaper's janitor list is unbuilt** (D134): a failing `up` leaves
   workspaces behind and Herd records only that `down` failed (ORCH-012).
2. **A requirement reading still weighs nothing** — no memory, no tmpfs
   (`matcher.rs:421`). **Durable writing is 99.5 % of a runner junction**
   at `N` = 1 000 and nothing ever deletes a job directory (ORCH-021).
3. **M11's pack half does not exist**, with no frame on either protocol
   enum, so E-12 and E-17 are undelivered (ORCH-020).
4. **M17's second machine is prepared, not joined**, **one shared token**
   serves every requester, and **three tests are known unstable** (D149;
   ORCH-013, -014, -022).
