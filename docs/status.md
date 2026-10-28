# Status — 2026-10-28

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8). **The first
consumer operates the running system** (D161, invariant 9): the hub container
and its volume, the runner units and their data roots, the tokens, the disk,
the version pins. Ours is what a tag produces.

## How a change reaches `main`

**No pull request** (**D189**, 2026-10-28): one batch, one `--no-ff` merge
commit carrying the landing record, one push by `herd-merger`, one post-push
`ci.yml` run. The condition is a green `./scripts/gate.sh` for the merge's
**second parent, trees identical**, reading the skip **list** and never the
count (**D187**); `pre-push` is the only machine in front of `main` (**D188**),
`enforce_admins` being false against an admin credential. **Still to do:**
Alexa narrows the branch protection, then the first local landing deletes
`ci.yml`'s `pull_request:` trigger — the one lane that can. `release.yml` and
`release-dev.yml` stay (D162, D168, D172).

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24: v0.3.6 plus a deep queue
that no longer writes a non-event per queued job per pass (D171); v0.3.6
carried the bounded reading (D169). v0.3.4 onward are published (D162); a
**`v*.*.*-dev*` tag** publishes a pre-release (D172). **`main` is 280 commits
past v0.3.7** on this batch's head, the workspace version still reads `0.3.7`,
and nothing merged since the tag has shipped. `PROTOCOL_VERSION` is **2**,
`RECORD_VERSION` **1**.

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact about their machines reaches
us as a report, and each figure below carries the date it reached us.

- **Versions, reported 2026-10-24:** their nightly ran on **0.3.5** (D169),
  then **0.3.6** with a full pass (D171). **Whether v0.3.7 reached them is not
  recorded.**
- **Disk:** about **90 GB across 1 571 jobs** against D152's 200 GB budget,
  nothing ever evicted (2026-10-23); **125 runner spools holding 65 GB**, under
  seven days old (2026-10-26). A runner **sweeps `outputs/` at startup**
  (ORCH-092), re-offers each, and sets aside — never deletes — what no hub
  claims (**ORCH-108**). **State root `/mnt/ssd_980`, ext4 on NVMe, 983 GB,
  ~341 free** (2026-10-27, ORCH-076) — shared with this workstation's builds.
- **The shipped seven-day output floor cannot be kept at their workload at any
  budget:** it needs **~750 GB** against a **~451 GB** theoretical and ~250 GB
  practical ceiling (`disk-policy.md` §2.4); whether the default stays or drops
  is **ORCH-105**, with Alexa.
- Herd is their only path: a job it cannot run fails their build; long jobs,
  KVM and exclusive `rig-cpu` worked on 2026-10-23.

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB by default
(`confine.rs:82`). **A skip count is per host and per load** (D27, ORCH-070,
ORCH-107): **12** here at rest, **14** under load, **14** on CI — compare the
**list**, not the number (**D187**); D151's 13 is unreproducible (`05`).

**The suite is green here: 839 passed, 0 failed, 12 skipped at rest**, every
skip a cgroup-delegation refusal; the same tree gave **14** at load 13.62 an
hour earlier, `flood-fifty` and `flood-two-fifty` being load-gated at a ceiling
of 8 — the split above, measured twice in a day. **D179** still reports an
incomplete run's skips as a floor; the 21 at load 19 stays **unexplained**.

**Thirteen changes are in no release:** nine defects (**D175**-**D178**,
**D180**-**D182**, **ORCH-094**, **D185**), two new behaviours (**D184**,
`SIGTERM` stops the hub in five seconds, non-zero on overrun; **ORCH-092**, a
runner sweeps its output spool at startup) and two cost cuts (**D183 step A**,
the `waiting` event line alone unsynced, 3.10 → 2.10 ms; **ORCH-082**, the
store's lock no longer spans a record read, eight readers ×8.49 → ×1.71).
Each is one dated entry in `docs/decisions.md`. `docs/remediation/` holds
fifteen pieces: the baseline, audits A and B with their review, two
experiments with theirs, and `06`-`13` (**D183**).

## What the software still does not do

Each is a board row, which holds its owner and next step.

1. **The reaper's janitor list is unbuilt** (D134): a failing `up` leaves
   workspaces behind and Herd records only that `down` failed (ORCH-012).
2. **A requirement reading still weighs nothing** — no memory, no tmpfs
   (`matcher.rs:421`). Nothing ever deletes a job directory (ORCH-021);
   **~94 % of a `T` walk is per-record CPU** (ORCH-123, a hypothesis); **what a
   runner lends is a startup snapshot** never revised as its cgroup empties,
   and nobody is told (**ORCH-126**).
3. **M11's pack half does not exist**, with no frame on either protocol enum,
   so E-12 and E-17 are undelivered (ORCH-020).
4. **M17's second machine is prepared, not joined**, **one shared token**
   serves every requester, and **three tests are known unstable** (D149;
   ORCH-013, -014, -022).
