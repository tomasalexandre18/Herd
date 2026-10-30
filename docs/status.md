# Status — 2026-10-30

State now, replaced every session. **What is owed and what Alexa has not
decided lives in `docs/orchestration/board.md`** (invariant 8). **The first
consumer operates the running system** (D161, invariant 9). Ours is a tag.

## How a change reaches `main`

**No pull request, and no lane left that could start one** (**D189**). One
batch, one `--no-ff` merge commit whose message **is** the landing record, one
push by `herd-merger`, one post-push `ci.yml` run that is then **read**, on a
green `./scripts/gate.sh` for the merge's **second parent, with an identical
tree**. **Branch protection exists and does not bind us** (**D203**): `check`
and `docs` required and force-push barred, but `enforce_admins=false` against
an admin credential, so a push prints `Bypassed rule violations` and succeeds —
expected, not a defect. **GitHub is storage**; `gate.sh`, `pre-push` and the
contract guard `main`. **The skip list decides, never the count** (**D187**),
its hash now **pinned to byte order** (**D227** by **D236**), and **a
compile-time `#[ignore]` is a skip too** (**D236**): its own list, **38**
names. **A drafted decision lives in the committed handoff** (**D226**).

## Releases, and what v1.0.0 will mean

**v0.4.1 is published and tagged, and `herd-merger` pushed that tag**
(**D212**); it carries **D211**, so their runner can leave 0.3.6. **Nothing is
released since.** Notes are a file in the tree (ORCH-162); **publishing and
writing them stay Alexa's** (**D210**).

**Stable means v1.0.0** (**D204**). **M11's pack half is inside it**, **M17
outside** (**D206**); **44 of 44 musts or 42 is Alexa's** (ORCH-210). **All
three versions have now moved** — `PROTOCOL_VERSION` **3**, `RECORD_VERSION`
**2**, `LAYOUT_VERSION` **2** — no migration path, the last two destroying a
root (ORCH-156). **D199's constraint on the ordinary release before v1.0.0 is
moot** (**D243**): intermediate releases are cancelled, so the next tag is
v1.0.0. The body carries all three numbers; two sentences say two (ORCH-217).

## What is known about the consumer's machines, and when

Nobody here logs into their hosts (D161); a fact reaches us as a dated report.
**Their hub and runner are running again, up since 20:23 on 2026-10-29** — so
the ceiling here is **two** heavy agents.

- **The 37.6 GiB kept for nothing is fixed in the code and still on their
  disk.** The hub had answered, 244 times for 75 jobs, and the runner discarded
  it; **a swept spool is judged on an answer, never a silence** (**D221**), and
  is **freed by the release, not the merge** (ORCH-205). **No migration will
  reclaim it** (**D242**): nothing on disk tells a spool set aside wrongly from
  one set aside rightly, so it is one `mv` and a restart, theirs to do.
- **50 more spools their runner refuses to offer**, naming the first unreadable
  path and the errno (ORCH-185, ORCH-186; **D224**).
- **Disk, a rate not a stock:** ~90 GB/1 571 jobs to 2026-10-23, **11 GB/day**
  active; ~2.8 GB/day stopped. **`r_file` = 5.04**, no threshold (ORCH-199).
## Where the software is

M1-M11, M15, M16, and **half of M13**: the fleet's state is `status.json`,
published on change and never on a clock (**D215**). **E-37's events are four
of five** (**D216**), **E-41's predicate is `herdctl if-moved`** (**D217**),
**an unreadable record is quarantined, not fatal** (**D218**, 176 → 0; the
`RECORD_VERSION` arm too, **D219**; `herd-hub report` banners one, **D224**).

**M11's pack half is in the tree** (**D229**-**D235**) and **the hub answers
all six pack frames** (**D247**), each of which used to close the link. The
hub holds a registry under `packs/manifests/<id>/` keyed on `(commit, key)`,
refusing two ids for one key; the runner holds a content-addressed store that
hard-links a pack into a job; a peer serves and fetches objects on the one
socket invariant 3 permits, taking D10's paths in D10's order and **reporting
which one**. A pack **leaves `{out}` before the record walk** (**D223**); one
Herd cannot read is **set aside and kept** (**D233**). **Two store guards**
(**D240**, **D241**): a relative path where a collection is judged whole, and
a chunk with no sha256.

**The runner has a mesh-address rule and binds nothing by default** (**D222**).
**Every figure in all three `--help`s is read back through its own constant**
(**D245**). **Skips are per host** (D27): **12** at rest, **14** loaded (D187).

## What the software still does not do

1. **Nothing ever deletes a job directory** (ORCH-012, -021), a reading weighs
   no memory (**ORCH-126**), *reaped* is unbuilt (**D134**), and **`packs/` has
   no retention, sweep or budget at either end** (ORCH-215).
2. **The pack half is wired at the edges, not through.** `ToHub::JobFact` and
   six reason codes ride the wire untouched (D230); the **push** half is
   unbuilt (ORCH-220); **no job is assigned a pack** (ORCH-234); and **D231-5
   is a behaviour** (ORCH-230, ORCH-232, **D247**) — a runner announces what
   it holds unprompted; the hub's holdings line undercounts a chunked one
   (ORCH-239). One chatOnS-gated list (**D238**); the road is counted (D237).
4. **V3 stays red and the reading is Alexa's** (**D228**, ORCH-022): whether a
   runner silent for `lost_after` keeps its link, against E-21's 60 s promise.
5. **The runner's half of D225 has never run against a real hub**, though both
   halves are in the tree — the cheapest gap before v1.0.0 (ORCH-222).
