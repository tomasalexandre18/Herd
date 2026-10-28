# Status — 2026-10-28

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8). **The first
consumer operates the running system** (D161, invariant 9): the hub container,
the runner units, the tokens, the disk, the version pins. Ours is what a tag
produces.

## How a change reaches `main`

**No pull request, and no lane left that could start one** (**D189**). One
batch, one `--no-ff` merge commit whose message **is** the landing record, one
push by `herd-merger`, one post-push `ci.yml` run that is then **read**. The
condition is a green `./scripts/gate.sh` for the merge's **second parent, with
an identical tree**, plus a filled record. **`ci.yml` lost its
`pull_request:` trigger at the first local landing, 2026-10-28, and runs only
on a push to `main`** — the last transition step that was ours. Still pending,
and nothing waits on it: Alexa narrows the branch protection, a tidying step
and not a safety one. `release.yml` and `release-dev.yml` stay (D162, D168).

**The gate** (**D190**): steps 0 to 8, the middle six one per command `ci.yml`
runs. It refuses a dirty tree, ends in one verdict naming its sha, and writes
a receipt **only on GREEN**, which `pre-push` asks about by
`--verify-receipt`. It compares the skip **list**, never the count (**D187**):
twelve names at rest here, plus a two-name **delta** list above a load of 8.
A `--bless` run writes **no** receipt, and the load is read twice (**D191**).
`pre-push` is the **only** machine in front of `main` (**D188**) — nothing
server-side binds our admin credential — `core.hooksPath` is absolute now,
arming all 85 worktrees (ORCH-131), and `--no-verify` is forbidden by contract
and not by the machine.

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24 (D171); v0.3.4 onward are
published (D162) and a **`v*.*.*-dev*` tag** publishes a pre-release (D172).
**`main` is 309 commits past v0.3.7**, the workspace version still
reads `0.3.7`, and nothing merged since the tag has shipped.
`PROTOCOL_VERSION` is **2**, `RECORD_VERSION` **1**.

**Fourteen changes are in no release:** nine defects (**D175**-**D178**,
**D180**-**D182**, **ORCH-094**, **D185**), three new behaviours (**D184**,
`SIGTERM` stops the hub in five seconds; **ORCH-092**, a runner sweeps its
output spool at startup; **D194**, `herd-hub report [STATE_DIR]`, a lock-free
read of a state root printing `r_file`, the kept-byte deciles and the
record-against-disk gap) and two cost cuts (**D183 step A**, the `waiting`
event line unsynced; **ORCH-082**, the store's lock no longer spans a read).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their instance is paused**, so more than two heavy agents may run here.

- **Versions, reported 2026-10-24:** their nightly ran on **0.3.5** (D169),
  then **0.3.6** with a full pass (D171). **Whether v0.3.7 reached them is not
  recorded.**
- **Disk:** about **90 GB across 1 571 jobs** against D152's 200 GB budget,
  nothing ever evicted (2026-10-23); **125 runner spools holding 65 GB** under
  seven days old (2026-10-26). State root `/mnt/ssd_980`, ext4 on NVMe,
  **~339 GB free** (2026-10-28, ORCH-076).
- **The seven-day output floor cannot be kept at their workload at any
  budget:** ~750 GB needed against a ~451 GB ceiling (**ORCH-105**, Alexa's).
  **`r_file` is unmeasured on the volume that decides it** (**ORCH-145**).

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB by default
(`confine.rs:82`). **A skip count is per host and per load** (D27): **12**
here at rest, **14** under load, **14** on CI — the **list** decides (D187).

**The suite is green here at rest: 0 failed, 12 skipped, `test suite:
PASSED`**, every skip the same cgroup v2 delegation this host does not grant;
the two extra under load are `flood-fifty` and `flood-two-fifty`, which read
the load themselves. `check-orchestration.sh` is at **256 passed, 0 failed**.
**`main` went red once on a post-push run** (`40720983497`, `7eb9585`,
`announced_job_ceiling.rs:1179`) on content green ten minutes earlier, and was
deliberately not re-run: **ORCH-119**.

## What the software still does not do

Each is a board row, which holds its owner and next step.

1. **The reaper's janitor list is unbuilt** (D134): a failing `up` leaves
   workspaces behind and Herd records only that `down` failed (ORCH-012).
2. **A requirement reading still weighs nothing** — no memory, no tmpfs
   (`matcher.rs:421`); nothing ever deletes a job directory (ORCH-021); what a
   runner lends is a startup snapshot never revised (**ORCH-126**).
3. **M11's pack half does not exist**, with no frame on either protocol enum,
   so E-12 and E-17 are undelivered (ORCH-020).
4. **M17's second machine is prepared, not joined**, one shared token serves
   every requester, three tests are known unstable (D149; ORCH-013, -014).
