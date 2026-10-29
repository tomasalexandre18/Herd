# Status — 2026-10-29

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8). **The first
consumer operates the running system** (D161, invariant 9): container, units,
tokens, disk, pins. Ours is what a tag produces.

## How a change reaches `main`

**No pull request, and no lane left that could start one** (**D189**). One
batch, one `--no-ff` merge commit whose message **is** the landing record, one
push by `herd-merger`, one post-push `ci.yml` run that is then **read**, on a
green `./scripts/gate.sh` for the merge's **second parent, with an identical
tree**; no branch protection (**D203**). **The gate** (**D190**) is steps 0 to
8, one per `ci.yml` command, and the skip **list** decides, never the count
(**D187**). **The preflight reads the enclosing cgroup** and refuses below 8
GiB of room (ORCH-177). **Two fixtures stopped being the environment's**: a
long or nested `TMPDIR` no longer reds the suite (ORCH-139) and the follower's
window is the silence itself (ORCH-022's V4; its V3 is the product's).

## Releases, and what v1.0.0 will mean

**v0.4.1 is published and tagged, and `herd-merger` pushed that tag**, the
first the integrator has posted (**D212**). It carries **D211**, the startup
reaper that deleted the spools its own startup sweep exists to re-adopt, **so
the release the first consumer waited for is out and their runner can leave
0.3.6.** Its body went out **without the human half** again, the second
running: that paste is Alexa's (**D210**, ORCH-164), and **ORCH-162, which
would retire the need, still cannot land** — it merges clean, then its own
guard refuses `main`, which carries 0.4.1 and no
`deploy/release-notes/0.4.1.md`. **Nothing is released since.**

**Stable means v1.0.0** (**D204**): most features implemented, then support,
then a period constrained on features. **M11's pack half is inside it**, making
v1.0.0 **44 of 44** of the consumer's musts (**D205**); **M17 is outside it**
by choice (**D206**). **All three versions move before it** (**D199**) — 2 → 3,
1 → 2, 1 → 2 — no migration path, the last two destroying a root (ORCH-156).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their instance is paused**, so more heavy agents may run here.

- **The largest open thing in the product: 37.6 GiB kept for nothing.** The hub
  says *"has every file of its collection"* **244 times, for 75 jobs**, while
  the runner has already renamed the spool and stopped listening — **not a
  silence but an answer the runner discarded**, so it needs no decision from
  Alexa (ORCH-192). **A reproduction is on `main`**, red 6 runs of 6, ruling
  out both causes first proposed. Alexa waits for the automatic fix: those
  spools are its only real proof.
- **50 more spools their runner refuses to offer**; the refusal now names the
  first unreadable path and the errno, settling their pack-ownership hypothesis
  at their next start (ORCH-185). **The `535691 kB` on one offer line in
  seventy-five is fixed**: a measured count reads back exactly, a typed cap in
  its typed family (ORCH-186).
- **Disk, a rate and not a stock:** their volume dates from **2026-10-15**,
  never recreated; ~90 GB across 1 571 jobs at 2026-10-23 is **11 GB/day**
  active, ~2.8 GB/day as they paused, and they keep it until v1.0.0. **`r_file`
  = 5.04** against the never-recorded threshold of 4 (ORCH-199); **the floor
  stays and the hub stops promising it** (**D202**).

## Where the software is

M1-M11, M15, M16, and **half of M13**: the fleet's state is `status.json` in
the state root, published on change and never on a clock, read by `herd-hub
status` (**D215**) — a document, not the frame `plan.md` reasoned for. **E-37's
events are four of five** (**D216**), **E-41's predicate is `herdctl
if-moved`** (**D217**), **an unreadable record is quarantined instead of
refusing the whole state root** (**D218**, 176 refusals → 0, which closes
ORCH-106 and ORCH-161 — no release body owes the `report`-first caution now);
the retention preview tells record bytes from output bytes, `herd-hub report`
names the jobs behind its gap, and three public formats are golden text.

**A skip count is per host and per load** (D27): **12** at rest, **14** under
load — the **list** decides (D187), and an `#[ignore]` is in neither list
(ORCH-202). `check-orchestration.sh` is at **285 passed**; **six green
post-push runs on `main`**, `40956679574` (`b37b2a4`) the newest.

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), and the janitor's *reaped* list is unbuilt — only
   its `left-behind` half shipped (**D134**, ORCH-187).
2. **M11's pack half does not exist**, no frame on either protocol enum, so
   E-12 and E-17 are undelivered — and `outputs.rs:12-40` would collect a pack
   as record evidence, the byte path D10 removed (ORCH-020, **ORCH-153**).
3. **Five declaration fields are parsed, validated and discarded** — `needs`,
   `leaves`, `coalesce`, `dedup_key`, `schedule` (**ORCH-148**) — and
   **`plan.md` disagrees with the code in about a dozen places**, four changing
   what gets built (**ORCH-152**; M14 wrong, D207).
