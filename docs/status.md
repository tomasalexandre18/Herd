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
an identical tree**, plus a filled record. **Branch protection is deliberately
not pursued** — the accepted final state, not a gap (**D203**): GitHub printed
`Bypassed rule violations` and let the push through, so what guards `main` is
the gate, `pre-push` (**D188**) and the contract. **The gate** (**D190**) is
steps 0 to 8, one per `ci.yml` command, ending in one verdict naming its sha
and a receipt **only on GREEN**. The skip **list** decides, never the count
(**D187**): twelve at rest, plus a two-name **delta** applied per skip from
the refusing bench's own figure, not the gate's host readings (**D196**).

## What v1.0.0 means, now that it is defined

**Stable means v1.0.0** (**D204**), defined 2026-10-29: most features
implemented, then support, then a period constrained on features. **M11's
pack half is inside it**, making v1.0.0 **44 of 44** of the consumer's musts
(**D205**); **M17 is outside it** by choice, E-49 and E-50 unmet (**D206**).
**All three versions move before it** (**D199**) — 2 → 3, 1 → 2, 1 → 2 — with
no migration path, so the last two destroy an existing root: ORCH-156.

## Releases, and what is in none

**v0.4.0 is published**: tag `2631286` → `171c780`, 2026-10-29 13:50:10Z,
`prerelease: false`, now *Latest*, five assets (**D162**); `release.yml`
`40885270441` green in **all 22 steps**, Arch and Alpine smokes included.
The published hub carries **`report`** and **`retention-preview`**, which
v0.3.7's has not; **interoperability was run both ways** and the three
version numbers are **unmoved** (ORCH-156): 2, 1, 1. So **the fifteen
changes that were in no release are now in one**, nine of them defects, and
retention ships **active** behind that preview (**D201**). **Its body went
out without the human half and Alexa pasted it himself** (**D210**;
ORCH-162 would make it a gate). **Their `report` reading is takeable now**
(ORCH-145).

**And v0.4.0 shipped a regression, fixed on `main` and in no release**
(**D211**): its startup reaper deleted the output spools its own startup
sweep exists to re-adopt, twenty-eight lines apart on one startup, so that
half was dead on arrival. **The first consumer's runner is held at 0.3.6**
until a release carries the fix, or the upgrade deletes the bytes they wait
for (ORCH-164). Both halves had a green test; neither ran that startup.

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their instance is paused**, so more than two heavy agents may run here.

- **v0.3.7 did reach them** — they run image `herd-hub:0.3.7` (2026-10-29,
  correcting this file's earlier "not recorded").
- **Disk, a rate and not a stock:** their hub volume was created
  **2026-10-15 22:10 and never recreated**, so the ~90 GB across 1 571 jobs
  at 2026-10-23 is about **11 GB/day** active, ~2.8 GB/day as they paused.
  Against D152's 200 GB budget **the first eviction arrives within weeks of
  their resuming**, and that order **has never run in production**
  (ORCH-154). They keep the volume until v1.0.0, then reset. Also **125
  spools holding 65 GB**, which D211 leaves to naming alone (ORCH-167).
- **The seven-day output floor stays and the hub stops promising it**
  (**D202**). **`r_file` is still unmeasured** (ORCH-145), which is why
  ORCH-156's ordering constraint matters to them.

M1-M11, M15 and M16; a job may return 1 GiB by default (`confine.rs:82`).
**A skip count is per host and per load** (D27): **12** here at rest, **14**
under load — the **list** decides (D187). The suite is green here at rest,
every skip the same cgroup v2 delegation this host does not grant.
`check-orchestration.sh` is at **269 passed**. **Three green post-push runs
on `main`**, `40896526152` (`10ff31d`) the newest, ending three reds fixed
by **D195** then **D208** (ORCH-119).

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
