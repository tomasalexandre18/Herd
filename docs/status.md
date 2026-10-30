# Status — 2026-10-30

State now, replaced every session. **What is owed and what Alexa has not
decided is in `board.md`** (invariant 8); **we ship, they operate** (D161).

## How a change reaches `main`

**`CLAUDE.md` and `docs/orchestration/workflow.md` hold the method in
full**: **no pull request**, one batch, one `--no-ff` landing, a green
`./scripts/gate.sh` for the merge's **second parent, same tree**, **protection
that does not bind us**, **the skip list, never the count**, **every gate
through our own Herd** (**D189**, **D203**, **D187**, **D236**, **D258**).

## Releases, and what v1.0.0 will mean

**v0.4.1 is published, tagged by `herd-merger`** (**D212**), carrying **D211**
so their runner can leave 0.3.6; **nothing released since**. Notes live in
the tree (ORCH-162), **Alexa's** (**D210**). Next tag: v1.0.0 (ORCH-217).

**Stable means v1.0.0** (**D204**) and **it ships 42 of 44 musts** (**D248**).
**M11's pack half is inside it; M17's two musts the consumer has released us
from** — **E-50 withdrawn** (their fleet needs KVM, so native Linux, never
Windows) and **E-49 waived** with a re-enrolment measurement that is **not**
E-49, on their own condition that **both are named in the notes as not met**.
**All three versions have moved** — `PROTOCOL_VERSION` **3**,
`RECORD_VERSION` **2**, `LAYOUT_VERSION` **2** — with no migration path, the
last two destroying a root. **D199 is moot** (**D243**), **ORCH-156 caduque**.

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their hub and runner are up since 20:23 on 2026-10-29**, so the ceiling is
**two** heavy agents. **The orchestrator opens that exchange on its own**
(**D249**), by `SendMessage`, never a GitHub issue, and nothing reports back.

- **The 37.6 GiB kept for nothing is fixed in the code and still on their
  disk** (**D221**, ORCH-205): **freed by the release, not the merge**, and
  **no migration will reclaim it** (**D242**) — one `mv`, theirs to do.
- **50 more spools their runner refuses to offer**, naming the first
  unreadable path and the errno (ORCH-185, ORCH-186; **D224**). **Disk is a
  rate:** **11 GB/day** active, ~2.8 stopped; **`r_file` = 5.04** (ORCH-199).
- **Reset from zero at v1.0.0, reconfirmed 2026-10-30:** the current root
  (**3334 `job.json`, 5.9 MiB**) is set aside, and is **unreadable by
  v1.0.0's binaries** — refused by version, not degraded. Told to extract now.

## Where the software is

M1-M11, M15, M16, and **more of M13**: the fleet's state is `status.json`,
published on change and never on a clock (**D215**). **E-37's five transitions
all exist in the hub and the row still reads `part`** (**D216**, **D250**) —
a detached job's end is announced when its submitter asked, and the last item
waits on M14's nightly, one line in `herdctl`. **E-41's predicate is
`herdctl if-moved`** (**D217**); **an unreadable record is quarantined, not
fatal** (**D218**, 176 → 0; the `RECORD_VERSION` arm too, **D219**).

**M11's pack half is in the tree** (**D229**-**D235**), **the hub answers all
six pack frames** (**D247**), and **a job is now assigned its packs**
(ORCH-234): manifest, holder and `leaves`, resolved before `begin_attempt`, an
unresolvable key being **a named wait, never a `Refused`** (D13). The hub keys
a registry on `(commit, key)`; the runner holds a content-addressed store
hard-linking a pack into a job; a peer serves and fetches on the one socket
invariant 3 permits, **reporting which path** (**D223**, **D233**, **D240**,
**D241**). **A dry run answers for the dispatch too** (**D261**, E-07); **the
runner honours the hub's custody, but no hub names a keeper yet** (**D262**).

**The runner has a mesh-address rule and binds nothing by default** (**D222**);
**every `--help` figure reads back through its own constant** (**D245**); **an
`EBUSY` is read, never guessed** (**D259**) and **a controller is read from a
list, never inferred from a file** (**D265**); **skips per host** (D27): **12**
runtime, **14** loaded, **38** ignored, the twelve needing a delegated scope
with its own `MemoryMax` (**D260**). **Two published figures are narrower than
their names:** the stop is **0.212 s** and only idle (**D264**), and
`bytes_written` is a workspace's apparent size, sampled (**D263**).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), *reaped* is unbuilt (**D134**), and the **hub's**
   `packs/` has no retention (ORCH-215). The runner's gc has a caller at last
   and **the pack budget is spent** (**D262**, ORCH-264 done), on **one edge
   only** (ORCH-275); the deployed unit names none (ORCH-252, -253).
2. **The pack half reaches a job, and no pack reaches a disk.** **The runner
   lays no pack out under `{in}` and registers none from `{out}`** — it hands
   the job back with every key named (ORCH-220). `ToHub::JobFact` and six
   reason codes ride the wire untouched (D230); **`herdctl` parses `needs` and
   `leaves` then discards them** (ORCH-274). One chatOnS-gated list (**D238**).
3. **V3 stays red and the reading is Alexa's** (**D228**, ORCH-022): whether a
   runner silent for `lost_after` keeps its link, against E-21's 60 s promise.
4. **The runner's half of D225 has never run against a real hub**, though both
   halves are in the tree — the cheapest gap before v1.0.0 (ORCH-222).
