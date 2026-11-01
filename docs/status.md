# Status — 2026-11-01

State now, replaced every session. **What is owed and what Alexa has not
decided is in `board.md`** (invariant 8); **we ship, they operate** (D161).
**The method is `CLAUDE.md` and `workflow.md`, in full**; gates on Herd (D258).

## Releases

**v1.1.0 is landed and tagged by `herd-merger`** (**D212**; Alexa validated
the publication 2026-11-01). Its notes are **his** (**D210**) and ship **in
the tree** (ORCH-162). **All three versions unmoved** — `PROTOCOL_VERSION`
**3**, `RECORD_VERSION` **2**, `LAYOUT_VERSION` **2** — so **no state root
is reset** and `operating.md`'s four steps do not apply (ORCH-356). **Every
runner first, then the hub**: a v1.0.0 runner sends no key. It ships the
2026-10-31 fixes and **names the retention promise v1.0.0's page attached to
v1.1 and this one does not keep** — chosen over holding the tag. **Stable
began at v1.0.0** (**D204**, **D277**), which moved all three with **no
migration path** (**D243**); its **five not met** stand (`coverage.md`): E-49
waived and **unmeasured**, E-50 and E-14 **withdrawn** (**D248**, **D275**),
retention **unbuilt**, **one known-unstable test shipping**, blocker **B8**.

## What is known about the consumer's machines, and when

Their facts reach us as dated reports (D161). **Their hub and runner are up
since 20:23 on 2026-10-29**, so the ceiling is **two** heavy agents; the
exchange is ours to open, unanswered (**D249**).

**From their reports:** **37.6 GiB kept for nothing**, fixed in code, still on
disk — **freed by the release, not the merge**, **no migration reclaims it**
(**D221**, **D242**, ORCH-205) — and **50 spools their runner refuses to
offer** (**D224**, ORCH-185, -186). **Disk is a rate:** **11 GB/day** active,
~2.8 stopped; **`r_file` = 5.04**. **Reset from zero at v1.0.0, reconfirmed
2026-10-30:** the current root (**3334 `job.json`, 5.9 MiB**) is set aside,
**unreadable by v1.0.0's binaries**, so extract now. **On 2026-10-31 they
moved `--job-memory-max` to 7 GiB** (2 seats, 14336 MiB usable); **D282** and
**D284** quote the 8 GiB. **Their 2026-10-31 incident ships in v1.1.0, in two
halves:** the 17 files, 42 MiB lost under `finished, exited 0` (**D291**,
**D292**) is hub-only, complete; `j0000000655`'s 8 re-offers in 0.51 s to the
machine still running it — exit 125, no verdict — is **bounded, not removed**
(**D294**): one refusal per *connection*, re-armed by a redial.

## Where the software is

M1-M11, M15, M16, and **more of M13**: the fleet's state is `status.json`,
published on change and never on a clock (**D215**), at `STATUS_VERSION` **3**.
**E-37's five transitions are all in the hub and the row still reads `part`**
(**D216**, **D250**) — the last item waits on M14's nightly, one line in
`herdctl`. **E-41's predicate is `herdctl if-moved`** (**D217**); **an
unreadable record is quarantined, not fatal** (**D218**, 176 → 0, **D219**).
**E-42 is `part`** (**D272**): dedup joins any non-terminal job, coalesce
exists nowhere (D75).

**A pack moves from one real runner to another, peer to peer, and the hub
carries no byte of it** (**D269**, **D270**) — E-12's cross-machine half. The
hub mints a scoped capability, tells the holder over its own link, answers the
asker and journals **no part of the token**; the runner advertises the mesh
endpoint it really **bound**, so `--mesh-listen 127.0.0.1` works in both
spellings. **No `herd-proto` move, no
invariant-3 guard weakened.** Under it: **a job is laid out with its packs
under `{in}` and registers what it leaves from `{out}`** (**D267**); the hub
keys a registry on `(commit, key)`; the runner hard-links a pack from a
content-addressed store into a job; **a pack with no holder is one of two
named waits, never a `Refused`** (D13, **D257**). **A dry run answers for the
dispatch** (**D261**) and **keepers are named only if asked** (**D266**).

**The runner has a mesh-address rule and binds nothing by default** (**D222**);
**skips per host** (D27): **13** runtime, **15** loaded, **38** ignored, the
thirteen needing a delegated scope with its own `MemoryMax` (**D260**).
**Three are narrower than their names:** the stop is **0.212 s**, idle only
(**D264**), `bytes_written` is a workspace's apparent size, sampled
(**D263**), and **a green suite proves less than its name** (**D268**): a mode
a fixture sets is undone within 200 ms; three tests are latently red
(ORCH-291) and the hollow green is closed (**D273**).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), no frame marks
   a reading (**D285**), *reaped* is unbuilt (**D134**), the **hub's**
   `packs/` has no retention (ORCH-215), and the runner's gc spends **the pack
   budget on one edge** (**D262**, ORCH-275); the unit names none (ORCH-252).
2. **Only the push direction of a transfer is left** (ORCH-220, plan S10): a
   holder that listens on nothing is refused **by name** and nothing pushes to
   the asker instead. **A peer's refusal carries a sentence and no code**, so a
   fetcher cannot name `NotPushedByHub` to retry it (ORCH-294); `JobFact` and
   six reason codes ride untouched (D230).
3. **V3 stays red and the reading is Alexa's** (**D228**, ORCH-022): whether a
   runner silent for `lost_after` keeps its link, against E-21's 60 s promise.
4. **The runner's half of D225 has never run against a real hub** (ORCH-222).
5. **E-16 stays `part` on three clauses** (**D280**): an empty claim set, an
   unsettable N, a spool kept under `unclaimed` (ORCH-275, ORCH-108).
