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
8, one per `ci.yml` command, and the skip **list** decides (**D187**). **The
preflight no longer counts reclaimable cache against the room it reports**
(**D220**) — an override goes on that invocation, never in the gate's
environment (ORCH-209).

## Releases, and what v1.0.0 will mean

**v0.4.1 is published and tagged, and `herd-merger` pushed that tag**, the
first the integrator has posted (**D212**). It carries **D211**, the startup
reaper that deleted the spools its own startup sweep exists to re-adopt, **so
the release the consumer waited for is out and their runner can leave 0.3.6.**
**A release's notes are now a file in the tree**, read by `release.yml` and
asserted by `check-docs.sh` for the workspace version (ORCH-162), so the page
is complete when published; **publishing it and writing those notes stay
Alexa's own act** (**D210**). **Nothing is released since.**

**Stable means v1.0.0** (**D204**): most features implemented, then support,
then features constrained. **M11's pack half is inside it**, **M17 outside**
by choice (**D206**). **Whether that is 44 of 44 of the consumer's musts or 42
of 44 is Alexa's to settle** — D205 says 44, D206's two unmet musts imply 42
(ORCH-210). **All three versions move before it** (**D199**) — 2 → 3, 1 → 2,
1 → 2 — no migration path, the last two destroying a root (ORCH-156); **a
`RECORD_VERSION` move leaving `LAYOUT_VERSION` behind starts a hub on a wholly
quarantined root**, so v1.0.0 moves both or guards it (ORCH-204).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated
report. **Their instance is paused**, so more heavy agents may run here.

- **The 37.6 GiB kept for nothing is fixed in the code and still on their
  disk.** The hub had answered — *"has every file of its collection"*, **244
  times, for 75 jobs** — and the runner discarded the answer. **A swept spool
  is now judged on an answer, never on a silence the hub has not had the
  chance to break** (**D221**, narrowing D19 and D211), and the reproduction
  red 6 runs of 6 is off `#[ignore]` and asserts the fix. **The bytes are
  freed by the release, not the merge**; ~2 chunks per re-offer (ORCH-205).
- **50 more spools their runner refuses to offer**; the refusal names the first
  unreadable path and the errno (ORCH-185, ORCH-186 for the `535691 kB` line).
- **Disk, a rate and not a stock:** their volume dates from **2026-10-15**;
  ~90 GB across 1 571 jobs at 2026-10-23 is **11 GB/day** active, ~2.8 GB/day
  as they paused, kept until v1.0.0. **`r_file` = 5.04** against the
  never-recorded threshold of 4 (ORCH-199); **the floor stays** (**D202**).

## Where the software is

M1-M11, M15, M16, and **half of M13**: the fleet's state is `status.json` in
the state root, published on change and never on a clock (**D215**). **E-37's
events are four of five** (**D216**), **E-41's predicate is `herdctl
if-moved`** (**D217**), and **an unreadable record is quarantined instead of
refusing the whole state root** (**D218**, 176 refusals → 0; the
`RECORD_VERSION` arm quarantines too, **D219**, the reversal ORCH-106's
published caveat promised against). **`herd-hub report` is the laxer reader of
the two and says nothing about it** (ORCH-203).

**The runner has a mesh-address rule and still binds nothing** (**D222**):
`--mesh-listen` is refused by name for a wildcard, a default-route address or a
globally routable one, and both invariant-3 guards are rewritten — one named
module may reach for `TcpListener`, every other for nothing that binds, so a
single-NIC flat-LAN machine is outbound-only (D10's own path). **A pack is gone
from `{out}` before the record walk** (**D223**). **Skips are per host and per
load** (D27): **12** at rest, **14** under load, the **list** deciding (D187),
and an `#[ignore]` in neither (ORCH-202).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), and the janitor's *reaped* list is unbuilt
   (**D134**, ORCH-187).
2. **M11's pack half does not exist**, no frame on either protocol enum, so
   E-12 and E-17 are undelivered. Its rule is written (D222, D223); its
   `PROTOCOL_VERSION` 2 → 3 is not, and is not in this batch.
3. **Five declaration fields are parsed, validated and discarded**
   (**ORCH-148**). **`plan.md` and `coverage.md` are corrected against the
   tree** (ORCH-152), but **three "Gated on chatOnS" lists still disagree**
   (ORCH-206) and `docs/road-to-v1.md` counts the road (ORCH-155).
