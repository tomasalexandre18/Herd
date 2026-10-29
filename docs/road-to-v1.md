# The road to v1.0.0 — the dated, counted view

**What this file is.** One quantified view of how far the stable release
is, taken at a dated point and comparable to the point before it. It is
the third document of three and **duplicates neither of the other two**:
`docs/plan.md` holds the milestones and what each one is done-when,
`docs/coverage.md` holds the first consumer's numbered expectations and
their state row by row. This file counts across them, names the blockers
as statements that are green or are not, and says what is still Alexa's
to decide. Where a figure comes from a table, this file points at the
table rather than restating it.

**How to read a figure here.** Every one names where it came from. A
figure nobody measured is marked **not measured** or **not recounted**,
never rounded and never quietly dropped. A figure counted at a sha says
the sha, because `main` moves several times a night.

## Advancement — one row per point of measurement, newest first

| Point | Counted at | Musts on `main` | Ceiling | Blockers green | Landed since the point above | Released |
|---|---|---|---|---|---|---|
| 2026-10-30 | `5f73c18` | **36 of 44** — 2 part (E-16, E-37), 4 unbuilt (E-12, E-14, E-17, E-42), 2 unmet by choice (E-49, E-50, D206) | **42 of 44**, with 44 of 44 claimed by D205 and unreconciled — **ORCH-210** | **5 of 10**; 2 in flight, 3 not ours to close alone | batches 8, 9 and 10: **24 branches, 85 commits**, three post-push `ci.yml` runs all green (40956679574, 40963330630, 40969367475) | v0.4.1; nothing tagged since |
| 2026-10-29 | `5d422bf` | **34 of 44**, as `coverage.md` read at that sha, with no `part` or `unmet by choice` vocabulary yet | not stated; D205 and D206 were taken this day | not counted — the list did not exist until 2026-10-29 | batch 7, which cut v0.4.1 and made the tag the integrator's (D212) | v0.4.1 published and tagged |

**Provenance of the top row, figure by figure.**

- **36 of 44** — counted column by column from `docs/coverage.md` at
  `5f73c18`: 36 `merged`, 2 `part`, 4 `planned`, 2 `unmet by choice`,
  summing to the 44 `must` rows. The count is `plan-truth`'s, made
  against the tree that became batch 10 and landed with it.
- **The jump from 34 to 36 is not two musts of work.** Three rows (E-35,
  E-37, E-41) landed in **batch 8** at `b37b2a4` while the table's State
  column still said 2026-10-23, so the document was two rows behind the
  code for two landings. `plan-truth` corrected it in batch 10. **No
  recount was made at `b37b2a4`** and none is claimed: nothing in
  batches 9 or 10 moved a must row, so 36 very probably held there too,
  and *probably* is why this is marked **not recounted**.
- **42 of 44** — D205 and D206 together, as `docs/coverage.md` states
  without reconciling them. D205 puts M11's pack half inside v1.0.0 and
  calls that 44 of 44; D206 declares E-49 and E-50 unmet by choice, and
  both are musts. Both cannot be true. **ORCH-210**, in the board's
  *Waiting*.
- **24 branches, 85 commits** — `git log` over the three landing
  commits: 13 into `batch-8-integration`, 4 into `batch-9-integration`,
  7 into `batch-10-integration`, and `git rev-list --count
  5d422bf..5f73c18` = 85.
- **Three green CI runs** — read with `gh run list`, each on the right
  sha: 40956679574 on `b37b2a4`, 40963330630 on `1a8c1ae`, 40969367475
  on `5f73c18`, all `success`. **Scoped claim:** no landing has been red
  since `5d422bf`. Earlier on 2026-10-29 there were three consecutive
  reds on `main`, the last of them run 40857453150 on `b73d38c`
  (batch 2), with no green run on that sha; the first green one after
  them was 40870735400 on `6036d24`.
- **v0.4.1, nothing since** — `git tag` and `gh release list`: v0.4.0
  (13:50Z) and v0.4.1 (19:13Z) on 2026-10-29 are the last two, the
  workspace version is 0.4.1, and no tag has been pushed since. By
  Alexa's own direction an intermediate version is cut when useful, so a
  gap here is not a delay.

## The blockers — ten statements, each green or not

A blocker is a promise the product makes and does not keep, or a
statement the release itself would make falsely. Each is phrased so that
"ready" is decidable instead of felt. State read at `5f73c18`.

| # | The statement | State at `5f73c18` |
|---|---|---|
| B1 | The scope the release claims is the scope it holds | **Alexa's** — ORCH-210, D205 against D206 |
| B2 | A spool the hub has acknowledged is freed | **green** — D221; the reproduction is off `#[ignore]` and asserts the fix |
| B3 | The bytes already condemned have a stated route out | **not started** — ORCH-193 is `owed`; D221 fixes the judgement going forward and re-adopts nothing already set aside |
| B4 | A release body reaches the page with its human half | **green** — a release's notes are a file in the tree, read by `release.yml` and asserted by `check-docs.sh` (ORCH-162) |
| B5 | The three version numbers move, and each refusal says what is lost | **in flight, ungated** — `main` is still 2 / 1 / 1; `PROTOCOL_VERSION` 3 is on `pack-proto` (pushed), `RECORD_VERSION` 2 and `LAYOUT_VERSION` 2 on `pack-registry` (committed, not pushed) |
| B6 | A runner lends what its host can really lend | **green** — D220; reclaimable cache is no longer charged against the room |
| B7 | Nothing the release ships says something false about the code | **in flight** — three branches landed in batch 10; `stale-comments` is in batch 11; ORCH-206's three disagreeing "Gated on chatOnS" lists are open |
| B8 | No known-unstable test ships | **half** — V4 fixed with measured rates in batch 9; V3 is open and its question is Alexa's (below) |
| B9 | The `--help` a runner prints is true | **green** — the output cap reads 1 GiB and says the flag raises it |
| B10 | The release act is permitted without waking him | **green for an ordinary release** — the tag is the integrator's (D212); a v1.0.0 body is the stability promise itself and stays Alexa's own act (D210) |

**Five green, two in flight, three that no amount of work here closes:**
B1 and B8 are Alexa's word, and B3 is owed work nobody has started.

## What is left to v1.0.0, in the order it has to happen

1. **M11's pack half — thirteen steps, two of them on `main` or pushed.**
   The one gate on everything is numbering the whole protocol surface at
   `PROTOCOL_VERSION` 3, because a frame nobody numbers in that bump is
   a frame nobody can add afterwards (D204 freezes the surface). The
   mesh-address rule and both invariant-3 guards are **on `main`**
   (D222), the protocol surface is **pushed** on `pack-proto`, and the
   pack store, the `{out}` extraction (D223) and the hub registry with
   both version bumps are **committed on local branches and not pushed**.
   What it buys: E-12 and E-17, and with them the only route from 36 to
   the ceiling. What it blocks: everything below, because the version
   bump rides with it.
2. **M12's remainder** — per-runner budgets and keeper pinning (the rest
   of E-16), key-aware placement and the GC that moves packs (E-14).
   What it buys: two musts. What it blocks: nothing below, but its
   retention delay is a parked question, so it cannot be finished
   without an answer.
3. **E-42** — the push hook's producer submit. `coalesce` and
   `dedup_key` are parsed, validated and discarded today (ORCH-148), so
   this is wiring a field that already exists on the wire.
4. **E-37's last two transitions** — a detached job's end needs the
   submitter's asking carried on `NewJob`, on `ToHub::SubmitJob` and in
   `job.json`, which is a wire *and* a record change and therefore wants
   to ride the same version bump as step 1; the nightly's verdict needs
   M14's nightly to exist first and then needs no event of its own.

Everything else that is open is in `docs/orchestration/board.md`. A row
is not on this list unless it stands between here and the tag.

## The parked questions — Alexa's, and visible here on purpose

Each is a question only he can answer, and each would otherwise sit in
one board row a reader has to go looking for.

1. **The delay before something that no longer serves is removed**, and
   **whether an archive path on another disk exists.** `docs/disk-policy.md`
   §6 F, left blank on purpose; **ORCH-199**. The delay is not optional;
   the archive path is.
2. **May Herd itself remove an output spool when the hub says it holds
   no copy**, or does it stay until a human removes it? **ORCH-108**.
3. **Is v1.0.0 42 of 44 or 44 of 44?** D205 against D206; **ORCH-210**.
4. **May a runner silent for `lost_after` keep its link?** A link bound
   of twice the threshold makes V3 holdable, and takes E-21's published
   promise — *"nothing heard for about 60 s is lost"* — to about two
   minutes for a machine genuinely off. The other reading is that the
   product does deliberately what the test complains of and the test
   should move. **This question is not on the board yet**, which is the
   same single-point-of-failure that put this file here.

## Adding the next row

A point of measurement is comparable to the one above it only if it is
taken the same way. For each new row: name the sha; recount the `must`
column of `docs/coverage.md` rather than carrying a figure forward;
re-read the blocker statements against the tree and count only the ones
whose statement is true; take the branch and commit counts from `git
log` between the two shas; read every CI conclusion with `gh` rather
than from a handoff. A figure you cannot take, mark **not measured** and
leave in place.
