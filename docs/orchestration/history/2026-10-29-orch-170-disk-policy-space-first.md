# orch-170-policy — handoff 2026-10-29

- **Item:** ORCH-170
- **Agent:** herd-step
- **Branch / commit:** `orch170-policy` / `b9316e4`, based on origin/main
  `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**Prose only. Three files, no code file, no `decisions.md`, no
`status.md`, no `board.md`.**

`docs/disk-policy.md` — **+950 lines, 1 522 in all.**

- **New §0, the contract**, read before §1 and pointed at from the
  intro. `§0.1` the direction in four sentences; `§0.2` the ownership
  table; `§0.3` the log; `§0.4` deduplication and the corrected figure;
  `§0.5` free space as a burst cache; `§0.6` keep only what still
  serves; `§0.7` the three visibility requirements.
- **§0.2 is the table the brief asked for**: four populations — collected
  files, the record, the log inside it, bytes in flight — each with what
  bounds it, what triggers it, what guards it and what an operator sees,
  every cell carrying a code citation or a reported row.
- **§1.2 gains R10 to R20**, the reading of 2026-10-29 as relayed: the
  filesystem, the ratio, the deciles, the record side to the kilobyte,
  Alexa's 85 % rule, the damaged job kept on our ask, and the 65 GB that
  no tool reported.
- **§1.3 rewritten**: most of what nobody could read on 2026-10-27 is now
  answered, read-only and without root. What is left is three items, and
  ORCH-103 should close.
- **§2.4 retired, not deleted.** The old finding is kept verbatim with
  the retirement after it: §2.3's identity `budget >= F x A` survives and
  is exactly what kills it, because deduplication divides `A`.
- **§2.6 amended**: `R` promoted from neighbour to prerequisite; its
  proposed default named as wrong for their disk against Alexa's own
  rule; the floor said to be a floor and not a target.
- **§2.7 amended**: its alarm becomes a requirement, and it now watches
  two quantities — `B` against what arrived, and free space against `R`.
- **§2.9 new**: what deduplication is, the four properties of the
  existing code that make it safe, and a five-row table of what it does
  **not** touch.
- **§3 demoted**: record removal is a correctness, privacy and
  walk-bounding instrument, not a space one, at 0.32 % of their root.
- **§3.1 amended**: exactly what survives of D201, item by item.
- **§3.2 amended**: the manifest's promise unchanged, plus its second job
  as the reference list; the stale `main.rs` citation corrected.
- **§3.3 inverted** with its reason, keeping the old sentence verbatim so
  the inversion is visible. New `§3.3.1` the ladder (five rungs plus the
  backstop), `§3.3.2` what happens to what can no longer serve,
  `§3.3.3` what the two mechanisms share.
- **§3.4 amended**: every measured figure survives; what it sizes is now
  the backstop, and the space path needs no walk of its own.
- **§4 gains H10 to H15** and a note narrowing H1 to H3 and confirming
  H8. **§5 gains seven out-of-scope items.** **§6 re-cut** to A to G plus
  the two records for `herd-merger`. **§7 gains §7.1**, nine proposed
  rows and seven changes to existing ones, and the order section is
  re-cut around `store.rs`.

`docs/plan.md` — M12's retention paragraph re-cut: space decides, the
span is the maximum age, the output floor doubles as the record guard,
deduplication ships on by default, the budget governs the collected files
only, and nothing is kept without a bound. **M12's `done when` is
untouched**: *"a step result past retention is gone while a 60-day merger
result reads fine"* survives verbatim and is now the backstop's test.

`docs/coverage.md` — **E-15** re-pointed (the two numbers are a maximum
age; the record side is 0.32 %; the done-when survives), **E-16** re-cut
(the floor in days survives by D202 and gains the record-guard job; the
free-space floor joins it; nothing is kept without a bound), and
**E-13** amended, which the brief did not name — see *Risks and
findings*.

**Three findings verified in the code rather than inherited**, at
`cd0f1b7`:

1. The output budget governs the collected files only, in its own
   argument documentation — `herd-hub/src/main.rs:149-151`. The sweep's
   running total is `directory_bytes(dir.join(KEPT_FILES_DIR))` and
   nothing else, `herd-hub/src/store.rs:785`.
2. Nothing on the hub bounds a log. `append_log` appends what arrives
   with no cap, `herd-hub/src/store.rs:2469-2509`; the only cap is
   `DEFAULT_LOG_CAP = 64 << 20` on the runner,
   `herd-runner/src/confine.rs:50`.
3. **New, and the brief did not have it:** that runner cap is **per
   attempt**, not per job. The hub appends a second attempt's output onto
   the first attempt's log (`herd-hub/src/store.rs:2283-2300`) and
   `ATTEMPTS_ALLOWED` is 2 (`herd-hub/src/server.rs:194`), so the
   hub-side ceiling for one job is two caps. Written into §0.3.

## Not done, and why

- **`docs/decisions.md` not written.** The brief forbids it and
  `check-docs.sh` fails on a `D-NEXT` header. The entry's text is below
  under *Decisions needed*; `herd-merger` allocates the number.
- **`docs/orchestration/board.md` not written.** The integrator owns it.
  Nine proposed rows and seven changes to existing rows are in
  `docs/disk-policy.md` §7.1, and the one row for this task is below.
- **`docs/status.md` not written.** The integrator owns it.
- **No code file touched**, and no `cargo` run: this changes no code and
  sibling tasks are building.
- **`--output-floor` not retired**, and the scratchpad proposal to
  replace it with a byte figure is recorded as dead in §6 A.
- **The delay and the archive path carry no default figure.** Deliberate:
  they are Alexa's (§0.6, §6 F).

## Gates

Runs made **before** this file was committed, on the tree of `b9316e4`:

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
EXIT=0
```

That `D170` line is a pre-existing `note:`, not a failure: it is printed
by `echo`, never by `note()`, and it is present on `cd0f1b7` unchanged.

**The Rust gates were NOT RUN locally.** This change touches only
`docs/`, so `CLAUDE.md`'s prose-only rule applies: what covers them is
the batch's own `./scripts/gate.sh` run at integration, which is
`herd-merger`'s and not this agent's. A gate that did not run is reported
as not run, never as passed.

**The last run, named and not quoted** — made on the committed tree that
includes this file, its output in this agent's report and not added to
the tree afterwards:

```
cd /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-orch170-policy
./scripts/check-docs.sh
```

**Skips: no skip.** No `HERD-SKIPPED` line was produced, because no test
suite was run.

**The 79-column trap, and how it was checked.** A perl byte-regex
checker mirroring `check-docs.sh`'s awk — fenced blocks and table rows
exempt, characters counted as bytes minus UTF-8 continuation bytes — was
**proved on four fixtures before use**: a known-good 79-column line (not
flagged), a known-bad 80-column ASCII line (flagged), a known-bad
80-column line built from em dashes, 207 bytes (flagged), and a long
table row and a long fenced line (both exempt, not flagged).
`check-docs.sh` itself was run after every edit — fifteen times — and no
edit used a `\x{...}` escape: every substitution was raw bytes in a perl
script read from a file.

## Risks and findings

1. **`docs/coverage.md` E-13 was edited and the brief named only E-15 and
   E-16.** E-13's note said *"retrofitting CAS would force a migration"*,
   which directly contradicts §2.9's `LAYOUT_VERSION` stays 1. Left
   alone it would be a live contradiction inside a file this task owns,
   so one clause was added saying the deduplication of this direction is
   not that retrofit and forces no migration. **If the integrator
   prefers it out, it is one `index()` anchor to revert** and the finding
   stands on its own.
2. **Everything in §0 rests on H10, and H10 is one unsettled
   hypothesis.** The 5.04 is read over 14 days that include the
   pre-streaming era. If the prospective ratio is materially below 5.04,
   §0.4's rate figures and §2.4's retirement both weaken. Said in §0.4,
   in §2.4 and as H10; the second reading is §6 G.
3. **A unit collision that would have read as a contradiction.** The
   2026-10-29 reading gives 916 GB / 64 % used / 319 GB free; R5 gives
   983 GB / 589 used / 344 free. They are the same filesystem: 983 GB is
   915.5 GiB, 344 GB is 320.4 GiB, and `df`'s `Use%` is used over
   used-plus-available, which turns 589-of-983 into 63 % rather than
   60 %. Reconciled in §0.5 and marked `[D]`. **The consequence worth
   noticing: free space has not materially moved in those two days.**
4. **The 208 GiB log figure is not repeated as a risk** anywhere, and
   §0.3 says explicitly why. The mechanism gap is written as a
   correctness argument about authority, not about magnitude.
5. **Two figures in §0.4's table are rounded differently from the
   planner's scratchpad.** §2.4's per-pass margin is **19 % to 34 %** of
   the budget, computed in GB throughout (`N / 5.04 x 7` against 200 GB);
   the scratchpad's "14 to 28 %" mixed GB and GiB. Mine is marked `[D]`
   and its inputs are named.
6. **§0.2's shares sum to 100 % and ignore 184 KiB** of readings and
   index outside `jobs/` `[R17]`, and `files.part/`, which the reading
   did not cover. Stated as "of the state root as the consumer read it".
7. **`herd-hub report` does count the digest-less population** as
   `undigested_files` / `undigested_bytes`
   (`herd-hub/src/report.rs:249-253`) — a scratchpad draft said it does
   not. What is true is that the reading of 2026-10-29 did not relay
   those two figures. §2.9's row D5 says so.
8. **No `D-NEXT` header was left anywhere**, verified by grep over
   `docs/`: the only hits are in existing handoffs under
   `history/`, untouched by this branch.

## Decisions needed

**One decision for Alexa, and it is §6 E of the document.** Is
deduplication on by default what he has directed, with its cost — a state
root whose output files are shared inodes, on every hub, from the release
that carries it? **Recommendation: yes, and record it with the threshold
it cleared** — 5.04 against the never-recorded 4, on their own volume,
read by a shipped binary. **The cost said plainly:** zero bytes freed on
day one, an older binary leaking orphan blobs until an upgraded one
collects them, and a silent degrade to today's behaviour on a filesystem
without hard links. **Fallback:** ship the linking off behind a flag for
one release, which costs a knob, a second code path and a release.

**A second, and it is §6 F:** the delay before something that can no
longer serve is removed, and the archive path. Both are figures, both
left blank on purpose. **Recommendation: ask for the delay, treat the
archive path as optional.** **Fallback:** neither, which means removal in
the same act as the selection.

**Proposed `docs/decisions.md` entry — text only, number allocated by
`herd-merger`:**

```markdown
## 2026-10-29 — D<n> Space before age: deduplication by default, the spans demoted to a maximum age

Alexa, 2026-10-29. The hub's disk policy is decided by space, not by a
calendar, and deduplication ships on by default.

**What changes.** The trigger for removing a record is free space on the
filesystem the state root sits on, never the output budget: the budget
governs the collected files only, by its own argument documentation
(`herd-hub/src/main.rs:149-151`), so it cannot see a record byte or a log
byte by definition. `--output-floor` becomes the record guard as well as
the byte floor. **D201's two numbers are unchanged — merger 90 days,
every other class 14, active by default — and become the maximum age a
record may reach rather than when it is deleted**; this entry amends D201
and edits nothing of it. Deduplication is `objects/<sha256>` beside
`jobs/` with every file in a job's `files/` a hard link to it, new
arrivals only, `LAYOUT_VERSION` unchanged at 1.

**And nothing is kept without a bound.** What can no longer serve is
removed after a stated delay, saying so, or moved to a configurable
archive path on another disk. Never immediately, never silently, and
never kept for the sake of keeping. Whatever is held — including a
transitional state and an archive path — appears in a report with its
size, counts toward the space calculation, and signals an overflow: the
precedent is the 65 GB of runner spools that no tool reported on
2026-10-27.

**The figure deduplication cleared, and the one that was wrong.** The
first consumer's reading of 2026-10-29 gives `r_file` = 5.04 against the
never-recorded threshold of 4. It does **not** free ~85 GB: 84.94 GiB are
second-and-later copies, but linking applies to new arrivals, so day one
frees zero. What changes is the rate — 7.57 to 1.50 GiB/day of distinct
bytes, and headroom to a 200 GB budget from 10.7 to 53.8 days. The free
space floor for their filesystem is their own existing rule, alert above
85 % used, which is 15 % free and about 137 GiB; the delay and the
archive path carry no default, because a shipped binary stating an
unrecorded policy is a number nobody can later change (D194).

Arithmetic, voices and the retired findings: `docs/disk-policy.md` §0.
```

**Proposed board row — written here, not on the board:**

```markdown
| ORCH-170 | Re-cut the disk policy for space before age: `docs/disk-policy.md` §0, §2.4 retired, §2.9 new, §3.3 inverted, `docs/plan.md` M12 and `docs/coverage.md` E-13/E-15/E-16 | Alexa 2026-10-29; the space-first plan | herd-step | done | Land in the batch; then decisions §6 E and §6 F to Alexa before ORCH-171 | — | `docs/disk-policy.md` §0; `history/2026-10-29-orch-170-disk-policy-space-first.md` |
```

## Resume point

1. **Push is done; the branch is `orch170-policy` at the commit this file
   is in.** Nothing else of this task remains.
2. **Put §6 E to Alexa, alone**, with 5.04 against 4 and the
   day-one-zero cost in the same breath. Then §6 F, the two figures.
3. **Have `herd-merger` write the proposed entry above** whatever the
   answer to E, because the demotion is already Alexa's direction, plus
   the second record of §6: the budget keeps its documented scope.
4. **Brief ORCH-168** (`retention.rs` and its test only) in parallel with
   anything: it touches no `store.rs` path.
5. **On E's yes, brief ORCH-171 alone on `store.rs`**, with H1 and H3 of
   the space-first analysis as its first acceptance work — the first
   because the design dies without it, the second because getting it
   wrong refuses to open an operator's existing root (ORCH-156).
6. Then ORCH-172, ORCH-095, ORCH-173, ORCH-174, ORCH-175, one worktree
   each, nothing else in `store.rs` in those batches, and **ORCH-087
   stays out of all of them** (`docs/disk-policy.md` §7, *File ownership
   and order*).

## Processes

**None.** No build, no test, no background process, no PID started and
none killed. No `cargo` invocation at all: the task changes no code and
sibling tasks were building (`CLAUDE.md`, shared host rules).

**No `target/` to delete:** none was created, and `CARGO_TARGET_DIR` was
never set, because nothing was compiled. The worktree holds no build
tree:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-orch170-policy
```
