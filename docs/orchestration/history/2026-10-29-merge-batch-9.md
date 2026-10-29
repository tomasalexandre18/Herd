# merge-batch-9 — handoff 2026-10-29

- **Item:** the landing of batch 9
- **Agent:** herd-merger
- **Branch / commit:** `batch-9-integration`, off `origin/main`
  `b37b2a4` (batch 8, landed the same evening); this file is the last
  change to the tree and the commit that carries it is the sha the gate
  runs on
- **Landing:** one `--no-ff` merge commit onto `main`, one push, no tag —
  this batch cuts no version and the workspace stays at 0.4.1

## The batch, in merge order

**Five branches were ready; four landed and one was held out.** The list
as it was taken, with each branch's own handoff:

| # | Branch | At | ORCH | Handoff |
|---|---|---|---|---|
| 1 | ~~`orch162-notes`~~ | `80f2f3e` | ORCH-162 | `history/2026-10-29-orch162-notes.md` — **not landed**, see below |
| 2 | `measured-not-cap` | `d37650b` | **ORCH-186** | `history/2026-10-29-measured-not-cap.md` |
| 3 | `v1-sunlen` | `f582c56` | **ORCH-139** | `history/2026-10-29-v1-sunlen.md` |
| 4 | `v3v4-rates` | `839590e` | **ORCH-022** (V3 and V4) | `history/2026-10-29-v3v4-rates.md` |
| 5 | `spool-offer-repro` | `b81bdea` | **ORCH-192**, and the new **ORCH-202** | `history/2026-10-29-spool-offer-repro.md` |

All four handoffs are in `docs/orchestration/history/` in the merged
tree, one per branch, and each was read before its branch was merged.
Only `measured-not-cap` was pushed (`origin/measured-not-cap`); the other
four exist as local branches of this checkout, which is why the merge was
done from the local refs and why a reader needs
`git show batch-9-integration` on the origin rather than four branch
names.

**Two ORCH ids proposed by their authors were not allocated, because the
item already had a row** — an id is an identifier and a second row for
one defect splits it:

- `measured-not-cap` asked for a new id (its handoff knew `main` only as
  far as ORCH-176). **ORCH-186** is this exact defect, written at batch
  8's landing off `sweep-names-why`'s diagnosis, down to the call sites
  `sweep.rs:151`, `:166` and `outputs.rs:251`, `:347`. It is closed, not
  duplicated.
- `spool-offer-repro` asked for one too, naming ORCH-167 and ORCH-108 as
  the closest and saying it was neither. It is **ORCH-192** — "the 37.6
  GiB was not kept on a silence; it was kept on an answer the runner
  discarded" — whose own cell already said *a reproduction is being
  written*. That row now carries the reproduction.

## The branch held out, and why it is the orchestrator's

**`orch162-notes` merges clean and then its own guard refuses `main`.**
It was merged first, as briefed, and the two files the brief named as
likely conflicts — `scripts/check-orchestration.sh` against batch 8's
`preflight-cgroup`, and `scripts/check-docs.sh` — **auto-merged with no
conflict**, both sets of cases present (the six release-notes cases at
`check-orchestration.sh:223-238` beside the preflight cgroup cases at
`:1546-1583`; the figure's own note moved to 38 runs). Then
`./scripts/check-docs.sh` on the merged tree said, verbatim:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
deploy/release-notes/0.4.1.md does not exist: the workspace is at 0.4.1 and a release's notes are a file in the tree, not a paste into the page (ORCH-162). Write it, or put the version back.
docs check failed
```

That is the branch's own new rule working exactly as its author's
*Risks and findings* 2 predicted — "the version this guard asks for
moves under it … whoever lands the bump adds the notes file in the same
branch" — meeting a bump that landed **before** the guard existed. The
branch was written when the workspace carried 0.4.0 and it brings
`deploy/release-notes/0.4.0.md` only; `main` has carried **0.4.1** since
batch 7. **No branch anywhere holds a `0.4.1.md`**: every local and
remote ref in this checkout was scanned for that path and none has it.

**What the integrator did not do, and will not.** The obvious content for
the missing file is the v0.4.1 body at
`history/2026-10-29-release-v0.4.1-body.md`,
already in the tree and already on the public page by Alexa's own hand,
transcribed to `deploy/release-notes/0.4.1.md`. **Writing a release's own
notes is forbidden to this agent (D210)**, and choosing instead to narrow
the guard — to a version that has a tag, say, or to a floor version —
changes what the branch asserts, which is a specification and equally not
the integrator's. So the integration was **re-rolled from `origin/main`
without `orch162-notes`** and the other four landed, under the authority
the brief gives for a branch whose own content reds the gate. Five
branches held hostage by one is the worse outcome; the branch is
unchanged, unpushed and still correct.

**This is the decision to route**, recorded in ORCH-162's row and not
settled here: who writes `deploy/release-notes/0.4.1.md`, or whether the
guard should ask only for a version that has shipped. Until one of the
two, that branch cannot land, and the next version bump inherits the same
obligation.

## Conflict log

**Nothing was resolved by hand. Every one of the four merges was clean,**
and the two collisions the brief named by name did not materialise as
conflicts:

| File | Branch | Against | Outcome |
|---|---|---|---|
| `herd-runner/src/sweep.rs` | `measured-not-cap` | batch 8's landed `sweep-names-why` | **auto-merged, both changes kept** |
| `herd-runner/src/outputs.rs`, `src/size.rs` | `measured-not-cap` | — | clean, no other branch or `main` hunk |
| `herd-runner/tests/confinement.rs` | `v1-sunlen` | — | clean |
| `herd-hub/tests/follower_alive.rs` | `v3v4-rates` | — | clean |
| `herd-hub/tests/spool_offer_answered.rs` | `spool-offer-repro` | — | new file, clean |

**`sweep.rs` was verified rather than trusted**, because an `ort`
auto-merge of two changes to one file is exactly where a semantic loss
hides. Both wanted changes are present: the reason-carrying walk of
`sweep-names-why` (`Unreadable` at `:468`, `why` at `:477`,
`for_a_foreign_state_root(why)` at `:187`, the `COULD NOT read … {error}`
sentence at `:219`) **and** `measured-not-cap`'s measured weight
(`use crate::size::measured_bytes` at `:99`, used at `:178` on the offer
line and `:193` on the foreign-state-root line). Counted both ways:
**99 of 99** added lines of `measured-not-cap` and **352 of 352** added
lines of `main`-since-the-merge-base are present verbatim in the merged
file, none missing on either side.

**And the check that actually binds, since no two branches of this batch
share a file:** for every file touched by exactly one branch, the merged
tree's version must be byte-identical to that branch's. **Nine of ten
are** — the four handoffs, `outputs.rs`, `size.rs`, `confinement.rs`,
`follower_alive.rs`, `spool_offer_answered.rs` — and the tenth is
`sweep.rs`, the one file `main` moved since `5d422bf`, measured line by
line above.

No conflict touched a specification or an invariant, so nothing was
stopped for a question.

## Decisions allocated

**None.** No branch of this batch carries a `D-NEXT` entry and
`docs/decisions.md` is byte-identical on all four to their merge base, so
nothing was allocated and nothing renumbered. The highest on `origin/main`
is **D218**; the known gaps D143, D154 and D170 stay (D148, and ORCH-201
owns the stale note). Two of the four handoffs ask for a decision to be
*briefed* rather than allocated — `measured-not-cap`'s "a typed figure
reads back in its typed family, a measured figure reads back exactly",
which its author recommends writing down in whichever batch lands the
fold, and `spool-offer-repro`'s nothing-for-Alexa — and neither is this
landing's to write.

## Board rows

Ten, all read against the merged tree:

- **ORCH-186 → `done`.** The measured byte count. Closed on the code, not
  on the branch's word.
- **ORCH-139 → `done`.** The `SUN_LEN` fixture, by the first of the two
  options the row offered. Two things still owed and neither is code: the
  row's **title** names only the long `TMPDIR` when the trigger is the
  total length of the bound path, and **no gate step asserts the fix** —
  the suite runs under whatever `TMPDIR` the caller exported, so the
  assertion inside the fixture is the whole guard.
- **ORCH-022 amended, stays `open`.** V4 fixed, and at `:199` rather than
  the `:231` the row is named for; V3 is the **product's** and failed 0 of
  40; a **fourth** sighting recorded from `v1-sunlen`'s first gate run and
  belonging to no branch of this batch — `reclaim.rs:685`,
  `the orphan 4041154 was not found among []`, one sample, a lead and not
  a diagnosis.
- **ORCH-106 → `done`** and **ORCH-161 → `done`**, as the brief asked and
  as the tree agrees: `read_stored` (`herd-hub/src/store.rs:1454`) returns
  `Ok(Stored::Quarantined(…))` on all four damaged shapes, so a hub binds
  its port over a torn record. **One deviation is named and not closed
  over:** ORCH-106's own caveat asked that the `RECORD_VERSION` mismatch
  arm **stay an error**; **D218 decided the other way** and that arm
  quarantines too (`:1472`). The decision is recorded, so it is flagged
  for the next reader rather than reopened.
- **ORCH-183 unchanged, and now the remainder of that cluster:** its prose
  residue — `docs/remediation/13-crash-point-enumeration.md` §4.2 and
  `README.md:42` still state the old refusal — is all that is left owed of
  ORCH-106/161/183.
- **ORCH-192 carries the reproduction** and the two things it rules out:
  the ENOENT fingerprint is absent here too, and the pump **does** read,
  so it is a third thing — one silent exit, not two, and nothing
  swallowed. The fixture size is now part of the row, because one swept
  collection was green 3 of 3 and **a fix validated against one
  collection would validate nothing**.
- **ORCH-200 amended:** one of its two named demonstrations is paid —
  `follower_alive`'s ratio of two measured durations is gone — and a
  fifth sighting added, `show_bytes` printing a measured figure.
- **ORCH-162 amended** with the whole of *The branch held out* above.
- **ORCH-202 is new** (`open`, the first id above ORCH-201, and no branch
  anywhere holds it): **an `#[ignore]`d test is in neither skip list, so
  D27's enforcement reaches runtime skips and not compile-time ones.**
  `scripts/test.sh`'s `HERD_SKIP_LOG` collects what a test writes **while
  running**, and a test that never runs writes nothing; measured here —
  the gate's named skip list is byte-identical with and without
  `spool_offer_answered.rs`. The row carries the cheap shape
  (`cargo test -- --list` names the ignored tests without running them)
  and the two things a brief must settle: a wanted `#[ignore]` belongs in
  `scripts/expected-skips/` with its row id, and an `#[ignore]` with no
  reason string should be red on sight.

`docs/status.md` is restated, **90 of 90 lines**, including the figure
this landing moves: `check-orchestration.sh` **285 passed, 0 failed**,
measured here before the gate, and batch 8's own post-push run
**40956679574** on `b37b2a4`, read and **green** — the duty the previous
landing deferred to this one.

## The landing record

Written now, as `templates/landing.md` requires, and it is the **message
of the merge commit**. The copy below is this handoff's; the one in the
merge commit carries the gated sha, the verdict line and the skip list,
which cannot be written into the tree the gate is about to read.

```text
Land batch 9: four branches, a measured count and two fixtures

- **Batch:** 9, four branches of a batch of five, landed 2026-10-29 by
  herd-merger. One integration branch, merged one at a time; one push,
  one --no-ff merge commit carrying this record, one post-push ci.yml
  run. No tag: no version branch is in this batch and the workspace
  version does not move. The fifth branch, orch162-notes, was held out
  and the reason is under *Left out*.
- **Gate:** green -- ./scripts/gate.sh on 8b16a43 plus the merge-handoff
  commit, which is this commit's second parent and the last change to the
  tree; trees identical, git diff --quiet between that sha and this one
  clean. Run BARE, never inside nix develop: nesting TMPDIR panics
  confinement.rs on SUN_LEN (ORCH-139) -- which v1-sunlen, in this batch,
  is what makes harmless. The receipt is named for that sha under
  target/herd-gate/ in the merger's own worktree, per worktree, never
  copied in. The resolved 40-hex sha, the verbatim output and the receipt
  path are in the merger's report: each exists only after the gated tree
  was closed, so none can be in this message.
- **Skips:** the at-rest names of scripts/expected-skips/local.txt, each
  printed as a skip and none counted into a pass (D27, invariant 5),
  named one by one in the merger's handoff. The list decides, never the
  count (D187); the two load-gated flood benches of local-under-load.txt
  are a delta applied only on the refusing bench's own figure (D196). The
  run's own list and its sha256 are in the merger's report. This batch
  adds no runtime skip -- and its one #[ignore]d test is in neither list,
  which is the new row ORCH-202.
- **CI run:** none yet -- this push starts it, so its id goes in the
  merger's report and the next landing's board edit records it. This
  landing discharges that duty for batch 8: run 40956679574, green on
  b37b2a4, read.

## What changed and why

Three things `main` could not do before. **A measured byte count says
itself exactly** -- `herd_runner::size::measured_bytes`, the hub's own
shape, at the runner's five measured sites -- so the consumer's
`535691 kB` on one offer line in seventy-five is gone while every cap an
operator typed still reads back in the family they typed it in (D148,
D152). That is ORCH-186, reported off their own fleet. **Two fixtures
stopped being the environment's:** the planted AF_UNIX socket binds under
a short path of its own, so a long or nested TMPDIR no longer reds the
suite (ORCH-139), and the follower-liveness window is the silence the
gate holds shut rather than everything heard (ORCH-022's V4, fixed at
:199 and not at the :231 the register names). **And the largest open
defect in the product has a reproduction on main** -- real hub and real
runner binaries, red 6 runs of 6, #[ignore]d until the fix removes the
attribute (ORCH-192). No expectation was edited to make anything green.
No milestone completes; E-nn coverage does not move.

## Branches in this batch

1. measured-not-cap, d37650b -- ORCH-186 --
   history/2026-10-29-measured-not-cap.md
2. v1-sunlen, f582c56 -- ORCH-139 -- history/2026-10-29-v1-sunlen.md
3. v3v4-rates, 839590e -- ORCH-022 -- history/2026-10-29-v3v4-rates.md
4. spool-offer-repro, b81bdea -- ORCH-192 and the new ORCH-202 --
   history/2026-10-29-spool-offer-repro.md

## Evidence

Each branch's own output separately, in the merger's handoff
(history/2026-10-29-merge-batch-9.md) and in its report; this record
carries the batch's own run on the merged tree. The gated sha is this
commit's second parent and its tree is identical to this commit's --
the measurement workflow.md *What a landing is* prescribes.

Four of the four merges were clean and nothing was resolved by hand.
sweep.rs, the one file main had also moved, was verified rather than
trusted: 99 of 99 added lines of measured-not-cap and 352 of 352 of
main-since-the-merge-base present verbatim. Nine of ten single-branch
files are byte-identical to their branch's version; the tenth is
sweep.rs. check-docs.sh green, check-orchestration.sh 285 passed /
0 failed, status.md 90 of 90 lines.

Per branch, as their own handoffs measure it: ORCH-186, ten new tests
pinned on literal strings, the consumer's 535691000 by name, and every
cap site still show_bytes. ORCH-139, base tree RED and this branch ok at
TMPDIR 131 and at a nested 62, ok at 148, with the regrowth guard proved
by a mutation that was not committed. ORCH-022, 20 runs at rest and 20
under load a side: 2 of 20 red before and 0 of 20 after under the same
load, with the ninth breath still landing in 4 of 40 and now harmless.
ORCH-192, red 6 runs of 6 on main, 4 to 10 of 12 collections condemned
each run and zero `cannot send` lines in every one.

The gate's verbatim output and the full skip list go to the receipt and
to the merger's report, never into the tree the gate read (D27, D179).

## Left out

**orch162-notes, and the reason is measured.** It merges clean -- both
files the brief named as likely conflicts auto-merged with both sets of
cases present -- and then its own new guard refuses the merged tree:
`deploy/release-notes/0.4.1.md does not exist: the workspace is at 0.4.1
and a release's notes are a file in the tree, not a paste into the page
(ORCH-162). Write it, or put the version back.` The branch was written
when the workspace carried 0.4.0 and brings 0.4.0.md only; main has
carried 0.4.1 since batch 7, and no branch anywhere holds a 0.4.1.md.
Writing a release's own notes is D210's and forbidden to the integrator,
and narrowing the guard would change what the branch asserts, so the
integration was re-rolled without it and the decision is routed in
ORCH-162's row rather than taken. Also left out: ORCH-183's prose
residue, the fold of the three copies of the measured rule, and the fix
ORCH-192's reproduction is the acceptance criterion for.

## Decisions allocated

Nothing. No branch carries a D-NEXT entry, docs/decisions.md is
untouched at D218, and the gaps D143, D154 and D170 stay (D148).

## Board rows

ORCH-186 done. ORCH-139 done, with its title and the missing gate
assertion owed. ORCH-022 amended and open: V4 fixed, V3 is the
product's, a fourth sighting at reclaim.rs:685. ORCH-106 done and
ORCH-161 done, with D218's deviation from ORCH-106's caveat named.
ORCH-183 unchanged and now the remainder of that cluster. ORCH-192
carries the reproduction and what it rules out. ORCH-200 has one
demonstration paid and a fifth sighting. ORCH-162 records why its branch
was held out. ORCH-202 is new: an #[ignore]d test is in neither skip
list, so D27's enforcement does not reach compile-time skips.

## CI run

Recorded, not waited on: the id is in the merger's report, read at the
next session start, and a failure becomes a board row before new work is
briefed. Not judged risky -- four branches, one of them inert, no
version number moved and no source file shared with main but sweep.rs.
```

## Gates

**One `./scripts/gate.sh`, bare, on the tree that carries this file**, and
it is the last thing that runs before the merge. Never inside
`nix develop`: that nests `TMPDIR` and panics `confinement.rs` on the
108-byte `SUN_LEN` (ORCH-139) — which `v1-sunlen`, in this very batch,
should now make harmless, and whether it did is a finding reported either
way.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/batch-9`, outside the worktree
and off the root filesystem, deleted after the push. The receipt it will
write:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/batch-9/wt/target/herd-gate/<gated sha>.receipt
```

with `<gated sha>` the full 40 hex of **this commit** — a value that
cannot be written here, because naming it would change the tree whose sha
it is. The receipt store is per worktree and nothing was copied into it.

**The preflight refused, and it was lifted on a measurement.**
`./scripts/bench-preflight.sh` said, verbatim:

```
slice room:  5739 MiB under memory.high on /chatons.slice, floor 8192 MiB (8 GiB)
bench-preflight: REFUSED, do not start the heavy phase.
```

Read for myself in `/sys/fs/cgroup/chatons.slice/memory.stat`:
**`anon` 597 397 504 B = 570 MiB** and **`file` 12 091 052 032 B =
11.26 GiB**, with `kernel` 590 MiB, against `memory.current` 12 693 MiB
and `memory.high` 18 432 MiB. So of the 12.4 GiB the preflight subtracts,
only about **1.1 GiB is not reclaimable**, and the room for the suite's
own anonymous allocation is about **16.9 GiB** — twice the floor, not
5.6 GiB short of it. Host-wide `MemAvailable` 23.1 GiB, `MemFree` 6.6 GiB,
load1 3.06 on 12 cores. `anon` being a few hundred MiB is the brief's own
test for "the refusal is about page cache the kernel will reclaim", so the
heavy phase was started with `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` lowered
to **5** — still a real floor, since 5 GiB of nominal room sits on top of
11.3 GiB of reclaimable cache. **Nothing under `/sys/fs/cgroup` was
written and no cache was dropped.** The arithmetic that makes an idle host
read as full is a known defect being fixed elsewhere and was not touched
here.

**The skips, as a list and never a count** (D27, D179, D187). This batch
introduces **no runtime skip**: the ten new tests of `measured-not-cap`
are unconditional, `v1-sunlen` adds and removes none, `v3v4-rates` adds
none, and `spool-offer-repro`'s one new test is `#[ignore]`d, which puts
it in **neither** list — the new row ORCH-202. So the names the gate
compares are exactly the twelve at-rest assertion names already in
`scripts/expected-skips/local.txt`, each one the same missing cgroup v2
delegated subtree, quoted here by name with their reasons cut (the gate
compares the name between the marker and the first colon, never the
reason, which carries a path and a pid):

```
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces
```

plus, **only if load1 crosses 8 while the suite runs**, the two
load-gated flood benches of `scripts/expected-skips/local-under-load.txt`
— `flood-fifty` and `flood-two-fifty` — each excused on its own reading
against its own ceiling of 8 and never on a count (D196). Load1 was
**3.06** when the preflight was read, so the at-rest list of twelve is
what is expected; **which list the run actually produced, and its
sha256, are in the merger's report**, because a run's output cannot be in
the tree that run read.

**What is in the report and deliberately not in this file**, because it
happens after this commit exists: the gate's **verbatim** output and its
**skip list** (a list and never a count — D27, D179), the merge commit
sha, the result of `git diff --quiet <gated> <merge>`, the post-push
`ci.yml` run id, and what step 10 did to the orchestrator's checkout. A
red step blocks the landing, is reported red, and `main` does not move.

## Risks and findings

1. **`docs/orchestration/workflow.md` step 8 will contradict the tree the
   moment `orch162-notes` lands** — it says, citing D210, that
   `release.yml` "never reads the prepared human half". That branch did
   not land, so the contradiction is **not** live, but the prose edit is
   owed in the same batch that lands it. `orch162-notes`' own handoff
   carries the proposed replacement wording verbatim; it is not the
   integrator's to write.
2. **ORCH-202 means the gate cannot see this batch's own new test.**
   `spool_offer_answered.rs` is `#[ignore]`d on purpose, so it cannot red
   the suite and cannot appear in the skip list either. If it somehow
   runs, that is a finding worth reporting in full, not a retry.
3. **`v1-sunlen` is in this gate's own tree**, so this landing is the
   first evidence of its claim at suite scale rather than at
   single-test scale. If the suite reds on a path length, that is a
   finding and not a retry.
4. **Agents share one scratchpad root.** `orch162-notes`' handoff records
   two tasks overwriting each other's `handoff-draft.md` in
   `…/3ddf11ac-…/scratchpad`. This landing wrote only inside
   `scratchpad/batch-9/`, and the fix is for a brief to **name** the
   subdirectory the way it names the worktree.
5. **PID 3046930**, the stray `herd-hub` on `127.0.0.1:18461`, was left
   alone and its port never taken. No `pkill`, no `killall`, no kill by
   name; the only processes started were the preflight,
   `check-orchestration.sh`, the gate and git.

## Resume point

```sh
cd /mnt/ssd_980/herd-project && git log --oneline -1 main
git show --stat main                   # the landing record
```

Then: route ORCH-162's one question (who writes
`deploy/release-notes/0.4.1.md`, or does the guard narrow), read the
post-push run id out of the merger's report, and brief ORCH-202 and the
ORCH-192 fix that deletes `spool_offer_answered.rs`' `#[ignore]`.

## Processes

Started and ended by me, all in my own worktree
(`…/scratchpad/batch-9/wt`): `git`, one run of
`./scripts/bench-preflight.sh`, one of `./scripts/check-docs.sh` per
merge, one of `./scripts/check-orchestration.sh`, and one
`./scripts/gate.sh`. Nothing of the consumer's was read or touched
(D161); nothing under `/sys/fs/cgroup` was written; `/etc/nixos`, `/srv`
and the chatOnS project were not approached. The orchestrator's checkout
at `/mnt/ssd_980/herd-project` is touched exactly once, by step 10's
`--ff-only`, after `main` has moved, with both preconditions checked and
printed — and the outcome is in the report.
