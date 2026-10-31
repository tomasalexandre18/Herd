# Status — 2026-10-31

State now, replaced every session. **What is owed and what Alexa has not
decided is in `board.md`** (invariant 8); **we ship, they operate** (D161).
**The method is `CLAUDE.md` and `workflow.md`, in full** — no pull request,
one batch, one `--no-ff` landing, a green `./scripts/gate.sh` for the merge's
**second parent on an identical tree**, every gate through Herd (**D258**).

## Releases

**v1.0.0 is landed and tagged by `herd-merger`** (**D212**, **D277**); a
release's own notes stay **Alexa's** (**D210**). **Stable means v1.0.0**
(**D204**); the notes carry **no headline figure** — his choice — and a
sourced table: the 44 `must` cells at `f7f2ea1` read **36 / 5 / 1 / 2**, E-14
`planned` there and `withdrawn` here since 2026-10-31. **Named as not met,
five**: E-49 waived and **unmeasured**, E-50 and E-14 **withdrawn**
(**D248**, **D275**), record retention cut to **v1.1**, **one known-unstable
test shipping**, breaking blocker **B8**. **All three versions have moved** —
`PROTOCOL_VERSION` **3**, `RECORD_VERSION` **2**, `LAYOUT_VERSION` **2** —
and there is **no migration path** (**D243**).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their hub and runner are up since 20:23 on 2026-10-29**, so the ceiling is
**two** heavy agents; the exchange is ours to open, unanswered (**D249**).

**From their reports:** **37.6 GiB kept for nothing**, fixed in the code and
still on their disk — **freed by the release, not the merge**, **no migration
reclaims it** (**D221**, **D242**, ORCH-205) — and **50 spools their runner
refuses to offer** (**D224**, ORCH-185, -186). **Disk is a rate:** **11
GB/day** active, ~2.8 stopped; **`r_file` = 5.04**.
**Reset from zero at v1.0.0, reconfirmed 2026-10-30:** the current root
(**3334 `job.json`, 5.9 MiB**) is set aside and is **unreadable by v1.0.0's
binaries** — refused by version, not degraded. Told to extract now. **Both
conditions it ships on are now met**: retention follows in v1.1, and
**`herd-hub report` weighs the records, events and logs** (**D271**) — apparent
size, said to be a floor, 1.97x under blocks.

## Where the software is

M1-M11, M15, M16, and **more of M13**: the fleet's state is `status.json`,
published on change and never on a clock (**D215**), at `STATUS_VERSION` **3**.
**E-37's five transitions are all in the hub and the row still reads `part`**
(**D216**, **D250**) — the last item waits on M14's nightly, one line in
`herdctl`. **E-41's predicate is `herdctl if-moved`** (**D217**); **an
unreadable record is quarantined, not fatal** (**D218**, 176 → 0, **D219**).
**E-42 is `part`** (**D272**): dedup joins any non-terminal job and is
asserted, coalesce exists nowhere (D75).

**A pack moves from one real runner to another, peer to peer, and the hub
carries no byte of it** (**D269**, **D270**) — E-12's cross-machine half. The
hub mints a scoped capability, tells the holder over the holder's own link,
answers the asker, and journals **no part of the token**; the runner advertises
the mesh endpoint it really **bound**, awaiting that bind before its first
dial, so `--mesh-listen 127.0.0.1` works in both spellings. **No `herd-proto`
move, no invariant-3 guard weakened.** Under it: **a job is laid out with its
packs under `{in}` and registers what it leaves from `{out}`** (**D267**); the
hub keys a registry on `(commit, key)`; the runner holds a content-addressed
store hard-linking a pack into a job; **a pack with no holder is one of two
named waits, never a `Refused`** (D13, **D257**). **A dry run answers for the
dispatch** (**D261**); **keepers are named only if asked** (**D266**); a fetch
says which path it took (**D223**, **D240**).

**The runner has a mesh-address rule and binds nothing by default** (**D222**);
a `--help` figure, an `EBUSY` and a controller are each read, never guessed
(**D245**, **D259**, **D265**); **skips per host** (D27): **13** runtime,
**15** loaded, **38** ignored, the thirteen needing a delegated scope with its
own `MemoryMax` (**D260**). **Three are narrower than their names:** the stop
is **0.212 s**, idle only (**D264**), `bytes_written` is a workspace's
apparent size, sampled (**D263**), and **a green suite proves less than its
name** (**D268**): a mode a fixture sets is undone within 200 ms; three tests
are latently red (ORCH-291) and the hollow green is closed (**D273**).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), *reaped* is unbuilt (**D134**), the **hub's**
   `packs/` has no retention (ORCH-215), and the runner's gc spends **the pack
   budget on one edge** (**D262**, ORCH-275); the unit names none (ORCH-252).
2. **Only the push direction of a transfer is left** (ORCH-220, plan S10): a
   holder that listens on nothing is refused **by name** and nothing pushes to
   the asker instead. **A peer's refusal carries a sentence and no code**, so a
   fetcher cannot name `NotPushedByHub` to retry it (ORCH-294) — a latency, not
   a lost pack. `JobFact` and six reason codes ride untouched (D230).
3. **V3 stays red and the reading is Alexa's** (**D228**, ORCH-022): whether a
   runner silent for `lost_after` keeps its link, against E-21's 60 s promise.
4. **The runner's half of D225 has never run against a real hub** (ORCH-222).
5. **E-16 stays `part` on three clauses** (**D280**): an empty claim set, an
   unsettable N, a spool kept under `unclaimed` (ORCH-275, ORCH-108).
