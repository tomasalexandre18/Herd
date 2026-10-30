# Status — 2026-10-30

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
8, one per `ci.yml` command, and the skip **list** decides (**D187**) — its
hash is **pinned to byte order** (**D227**), decided, not yet landed. **The
preflight no longer counts reclaimable cache** (**D220**); an override goes on
that invocation, never in the gate's environment (ORCH-209). **A drafted
decision lives in the committed handoff, never in a report** (**D226**).

## Releases, and what v1.0.0 will mean

**v0.4.1 is published and tagged, and `herd-merger` pushed that tag**, the
first the integrator has posted (**D212**); it carries **D211**, the startup
reaper that deleted the spools its own sweep exists to re-adopt, **so their
runner can leave 0.3.6. Nothing is released since.** **A release's notes are a
file in the tree**, read by `release.yml` and asserted for the workspace
version (ORCH-162); **publishing and writing them stay Alexa's** (**D210**).

**Stable means v1.0.0** (**D204**): most features implemented, then support,
then constrained. **M11's pack half is inside it**, **M17 outside** (**D206**);
**44 of 44 musts or 42 is Alexa's** (D205 against D206, ORCH-210). **All three
versions move first** (**D199**), no migration path, the last two destroying a
root (ORCH-156); a `RECORD_VERSION` move alone wholly quarantines (ORCH-204).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated
report. **Their instance is paused**, so more heavy agents may run here.

- **The 37.6 GiB kept for nothing is fixed in the code and still on their
  disk.** The hub had answered, 244 times for 75 jobs, and the runner
  discarded it; **a swept spool is now judged on an answer, never on a
  silence** (**D221**). **Freed by the release, not the merge** (ORCH-205).
- **50 more spools their runner refuses to offer**; the refusal names the first
  unreadable path and the errno (ORCH-185; ORCH-186's `535691 kB` line, whose
  rule now lives in one place, **D224**).
- **Disk, a rate and not a stock:** their volume dates from **2026-10-15**;
  ~90 GB across 1 571 jobs at 2026-10-23 is **11 GB/day** active, ~2.8 GB/day
  as they paused, kept until v1.0.0. **`r_file` = 5.04** against the
  never-recorded threshold of 4 (ORCH-199); **floor stays** (**D202**).

## Where the software is

M1-M11, M15, M16, and **half of M13**: the fleet's state is `status.json`,
published on change and never on a clock (**D215**). **E-37's events are four
of five** (**D216**), **E-41's predicate is `herdctl if-moved`** (**D217**),
**an unreadable record is quarantined, not fatal** (**D218**, 176 → 0; the
`RECORD_VERSION` arm too, **D219**), and **`herd-hub report`, the laxer reader
of the two** (ORCH-203), **now banners a report not one record parsed for**,
where it printed three sections of arithmetic over nothing (**D224**).

**No single failure condemns a healthy collection any more** (**D225**): a hub
whose record listing fails claims **nothing** and answers each collection the
runner **offers** from that job's own record, instead of telling every runner
at once that it holds nothing of theirs. With D221, both known ways a wanted
spool was set aside are closed.

**The runner has a mesh-address rule and still binds nothing** (**D222**):
`--mesh-listen` is refused for a wildcard, a default-route or a globally
routable address, one named module may reach for `TcpListener` and every other
for nothing, so a single-NIC flat-LAN machine is outbound-only. **A pack is
gone from `{out}` before the record walk** (**D223**). **Skips are per host**
(D27): **12** at rest, **14** loaded, the **list** deciding (D187; ORCH-202).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), *reaped* is unbuilt (**D134**, ORCH-187).
2. **M11's pack half does not exist**, no frame on either protocol enum, so
   E-12 and E-17 are undelivered. Its rule is written (D222, D223); its
   `PROTOCOL_VERSION` 2 → 3 is not, and is not in this batch.
3. **Five declaration fields are parsed, validated and discarded**
   (**ORCH-148**). **`plan.md` and `coverage.md` are corrected** (ORCH-152),
   but **three "Gated on chatOnS" lists disagree** (ORCH-206) and the v1.0.0
   roadmap is owed (ORCH-155).
4. **V3 stays red and the reading is Alexa's** (**D228**, ORCH-022): the
   watchdog drops a runner's link before asking what it holds, the obvious
   fix moved none of V3's five runs, and the question is whether a runner
   silent for `lost_after` keeps its link — against E-21's 60 s promise.
