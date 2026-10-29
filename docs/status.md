# Status — 2026-10-29

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8). **The first
consumer operates the running system** (D161, invariant 9): the container, the
units, the tokens, the disk, the pins. Ours is what a tag produces.

## How a change reaches `main`

**No pull request, and no lane left that could start one** (**D189**). One
batch, one `--no-ff` merge commit whose message **is** the landing record, one
push by `herd-merger`, one post-push `ci.yml` run that is then **read**. The
condition is a green `./scripts/gate.sh` for the merge's **second parent, with
an identical tree**, plus a filled record. **Branch protection and
`enforce_admins` are deliberately not pursued** — the accepted final state,
not a gap (**D203**): GitHub printed `Bypassed rule violations` and let the
push through, so GitHub is storage, and what guards `main` is the gate,
`pre-push` (**D188**) and the contract. **The gate** (**D190**) is steps 0 to
8, one per `ci.yml` command, ending in one verdict naming its sha and a
receipt **only on GREEN**. The skip **list** decides, never the count
(**D187**): twelve at rest, plus a two-name **delta** applied per skip from
the refusing bench's own figure, not the gate's host readings (**D196**).

## What v1.0.0 means, now that it is defined

**Stable means v1.0.0** (**D204**), defined for the first time 2026-10-29:
most features implemented, then support, then a period constrained on
features. **M11's pack half is inside it**, making v1.0.0 **44 of 44** of the
consumer's musts (**D205**); **M17 is outside it**, E-49 and E-50 unmet by
choice, so the promise covers a platform the software has never run on
(**D206**). **All three versions move before it** — `PROTOCOL_VERSION` 2 → 3,
`RECORD_VERSION` 1 → 2, `LAYOUT_VERSION` 1 → 2 (**D199**); `JobDirStore::open`
has no migration path, so the last two destroy an existing root, which is why
**the ordinary release ships at layout 1 and record 1** (ORCH-156).

## Releases, and what is in none

**v0.3.7 is the newest** — `3da48f5`, 2026-10-24 (D171); v0.3.4 onward are
published (D162), a **`v*.*.*-dev*` tag** publishes a pre-release (D172).
**`main` is 322 commits past v0.3.7** and the workspace version still reads
`0.3.7`. `PROTOCOL_VERSION` is **2**, `RECORD_VERSION` **1**,
`LAYOUT_VERSION` **1**. **Fifteen changes are in no release:** nine defects
(**D175**-**D178**, **D180**-**D182**, **ORCH-094**, **D185**), four new
behaviours (**D184**, `SIGTERM` in five seconds; **ORCH-092**, a spool sweep;
**D194**, `herd-hub report`; **D197**, `herd-hub retention-preview`, which
removes nothing and refuses a pass that would empty the root) and two cost
cuts (**D183 step A**, **ORCH-082**). Retention will ship **active**, merger
90 days and the rest 14, behind that preview and its refusals (**D201**).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their instance is paused**, so more than two heavy agents may run here.

- **v0.3.7 did reach them** — they run image `herd-hub:0.3.7` (2026-10-29,
  correcting this file's earlier "not recorded").
- **Disk, and for the first time a rate and not a stock:** their hub volume
  was created **2026-10-15 22:10 and never recreated**, so the ~90 GB across
  1 571 jobs at 2026-10-23 is about **11 GB/day** over their active period,
  slowing to ~2.8 GB/day as they paused. Against D152's 200 GB budget **the
  first eviction arrives within weeks of their resuming**, and the four-key
  eviction order **has never run in production** (ORCH-154). They keep the
  volume until v1.0.0, then reset. Also **125 spools holding 65 GB**.
- **The seven-day output floor stays and the hub stops promising it**
  (**D202**). **`r_file` is still unmeasured** (ORCH-145), which is why
  ORCH-156's ordering constraint matters to them.

M1-M11, M15 and M16; a job may return 1 GiB by default (`confine.rs:82`).
**A skip count is per host and per load** (D27): **12** here at rest, **14**
under load — the **list** decides (D187). The suite is green here at rest,
every skip the same cgroup v2 delegation this host does not grant.
`check-orchestration.sh` is at **269 passed**. **`main` went red on three
consecutive post-push runs**: `40720983497` (`7eb9585`) and `40762764493`
(`09229db`) at `announced_job_ceiling.rs:1179`, fixed by **D195**, then
`40857453150` (`b73d38c`) at **1156**, the next assertion out on the same
quantity, fixed here by **D208** — the premise is built, not timed
(ORCH-119). Whether `main` is green is this landing's own post-push run.

## What the software still does not do

1. **The reaper's janitor list is unbuilt** (D134), nothing ever deletes a job
   directory (ORCH-012, -021), and a reading weighs no memory (**ORCH-126**).
2. **M11's pack half does not exist**, no frame on either protocol enum, so
   E-12 and E-17 are undelivered — and `herd-runner/src/outputs.rs:12-40`
   would collect a pack as record evidence, the byte path D10 removed
   (ORCH-020, **ORCH-153**).
3. **Five declaration fields are parsed, validated and discarded** — `needs`,
   `leaves`, `coalesce`, `dedup_key`, `schedule` (**ORCH-148**) — and
   **`plan.md` disagrees with the code in about a dozen places**, four of them
   changing what gets built (**ORCH-152**; M14 is wrong on the record, D207).
