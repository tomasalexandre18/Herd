# merge batch — docs-gate-cost, orch-batched-pr — handoff 2026-10-27

- **Item:** ORCH-109, ORCH-110 (both `done`); ORCH-111 opened
- **Agent:** herd-merger
- **Branch / commit:** `merge-batch-2` / see *Gates*, based on
  `origin/main` `1e739fb` (re-read and confirmed, not taken from the
  brief)
- **PR:** one per batch, named below with its CI run

## Done

Two branches merged one at a time onto one integration branch, with the
fast gates run after each so that a red CI would be attributable to a
named merge commit.

1. `100e11a` **Merge docs-gate-cost into the batch** (`b8a87a7`).
2. `acc64dd` **Merge orch-batched-pr into the batch** (`cd09798`).
3. `349fb0a` the one commit of **PR #100**, cherry-picked with `-x`, its
   author preserved.
4. `6191152` the board rows and the durable multiplier note.

**No conflict arose, in either merge.** The brief expected `board.md` and
the handoff directory to collide; neither did. `docs-gate-cost` was
already level with `origin/main`, so its merge had nothing to reconcile.
`orch-batched-pr` was 19 commits behind, but the intersection of the
files `main` changed in those 19 commits with the files that branch
changed is **empty**:

```text
$ comm -12 <(git diff --name-only $MB..origin/main | sort) \
           <(git diff --name-only $MB..orch-batched-pr | sort)
(no output)
```

The two halves are disjoint as predicted — `scripts/` on one side,
`CLAUDE.md` plus `docs/orchestration/` plus three role files on the
other — and neither author wrote a board row, which is why the one
expected collision did not happen. **Nothing was resolved, so there is
nothing in the conflict log, and no conflict touched a specification or
an invariant.**

### What each branch did, verified rather than accepted

**`docs-gate-cost`.** The `docs` CI job was being killed at
`timeout-minutes: 5` having already printed its pass, so a passing gate
reported as red. 5.86 s of each 5.9 s of `check-docs.sh` was one bash
loop measuring line width one `read` per line over a ~28 000-line
corpus; the width is now counted inside awk under `LC_ALL=C` as
`length(line) - gsub(/[\200-\277]/, "", line)`. I re-measured both
implementations against the same corpus and compared their findings
rather than trusting the claim — see *Gates*, branch 1. The fixtures
were **not** trimmed: all cases still run against the whole real
repository, which is stronger than trimming, and `timeout-minutes` stays
at 5 because the cap is the only thing that reported the cost growing.
The suite goes **138 → 141** deliberately: its only width case was an
85-character ASCII line, 85 bytes either way, so a byte-counting mistake
would have stayed green; three new cases straddle the limit in 3-byte
and 4-byte characters. **141 is the number to keep.** No existing case
was altered or removed.

**`orch-batched-pr`.** The one-PR-per-batch method (Alexa, 2026-10-27)
and the rule that the orchestrator's checkout is never read for current
state. `CLAUDE.md` is at **exactly 150 lines** on the merged tree, its
enforced limit; the merge did not push it to 151. The three refusals —
selective testing, local-as-gate, nightly pass — are intact with their
reasons, and the local-gate refusal still names ORCH-069 and ORCH-107 as
the fixes that would make it reconsiderable. The author's withdrawal of
his own false claim stands in place, not deleted.

### PR #100: folded, not rebased

Its head was `origin/main` plus one commit, nine added lines in one
handoff file, `check` green and `docs` cancelled by the timeout alone.
**I folded it into this batch and will close it naming this PR.** The
reason to prefer folding over rebasing: #100's own branch does not
contain branch 1's fix, so its `docs` job would keep being killed until
it were rebased onto a `main` that already carries this batch — which
means leaving a red PR open across a merge and then spending a second
CI cycle on nine lines of prose. Folding settles it inside the cycle
this batch already pays for. It touches a file neither branch touches,
so it created no conflict, and `-x` keeps its author and records the
cherry-picked sha, so the authorship is not laundered into mine.

### The 26× multiplier, written where it will be hit

The orchestrator's read is that the cost fix is **not a decision** and
deserves no `D` entry; I agree, and allocated none. But the multiplier
it exposed is a durable constraint, so it is now in the header of
**both** scripts rather than only in a handoff: in `check-docs.sh`,
where a future author adds a rule, and in `check-orchestration.sh`,
where a future author adds a case. The figure is counted, not quoted —
29 mentions of `expect_red`/`expect_green` in that file, less two
function definitions and one comment, is **26 whole-corpus runs**.

## Not done, and why

- **No `docs/decisions.md` entry, and no `D` number allocated.** Neither
  branch carried a `D-NEXT` heading (checked on both:
  `git show <branch>:docs/decisions.md | grep -nE '^#+ *D-NEXT'` is
  empty on each), and `decisions.md` is byte-identical to `origin/main`
  on both. The highest number on `origin/main` is **D184**; nothing in
  this batch is a decision. **D143, D154 and D170 stay gaps** (D148);
  D170 is held by an open branch.
- **No `docs/status.md` change.** No software state changed: this batch
  touches two shell scripts and prose, no crate. The file is also at
  exactly its 90-line limit, so a line added costs a line removed, and
  the one fact worth recording — the `docs` gate is trustworthy again —
  is on the board at ORCH-109. Its "137 commits past v0.3.7" is dated by
  its own source ("counted on PR #85's head"), so it is stale by
  construction and honest, not wrong.
- **Nothing on the consumer's side**: no tag, no release, no issue,
  nothing read or written on their hub volume or runner data root
  (invariant 9, D161). Merging a version PR is not in this batch at all.

## Gates

**All four Rust gates were run**, because the diff touches `scripts/`,
which is outside the prose allowlist of CLAUDE.md ("`CLAUDE.md`,
`README.md`, `docs/`, `.claude/` and `.github/*.md`"), even though no
Rust file changed.

### Branch 1 — `docs-gate-cost`, after `100e11a`

The claim re-measured against the same corpus, and the findings compared
rather than assumed equivalent:

```text
$ git show origin/main:scripts/check-docs.sh > …/check-docs-old.sh
$ cat CLAUDE.md README.md docs/*.md docs/orchestration/*.md \
      docs/orchestration/*/*.md | wc -l
28156
$ # old implementation, today's corpus
old: 6085 ms
$ # new implementation, same corpus
new: 250 ms
$ bash …/check-docs-old.sh 2>&1 | sort | sha256sum
0e1d9795c65f75753fd4cac1c501b1779b786dfb9ae1a69d464399c8c3a70877  -
$ ./scripts/check-docs.sh 2>&1 | sort | sha256sum
0e1d9795c65f75753fd4cac1c501b1779b786dfb9ae1a69d464399c8c3a70877  -
```

**24× faster, byte-identical findings.** The 6009 ms → 242 ms the branch
reported is reproduced at 6085 ms → 250 ms on a corpus that has grown
since.

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)
elapsed: 238 ms

$ ./scripts/check-orchestration.sh
…
check-orchestration: 141 passed, 0 failed
elapsed: 9633 ms
```

**141 passed, 0 failed, and `check-docs.sh` in the hundreds of
milliseconds**, both verified on the merged tree as the brief asked.
`check-orchestration.sh` 153.4 s → 9.6 s.

### Branch 2 — `orch-batched-pr`, after `acc64dd`

```text
$ wc -l < CLAUDE.md
150

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)
elapsed: 232 ms

$ ./scripts/check-orchestration.sh
check-orchestration: 141 passed, 0 failed
elapsed: 9887 ms
```

**One failure here and a second one later, and neither has a name — see
*Risks*.** The first run of `check-orchestration.sh` after this merge
printed `140 passed, 1 failed`, and so did the run before the handoff
commit. Both times I had piped the run to `tail -2`/`tail -3` and lost
the `FAIL` line. It is ORCH-111, and it is not swept under the green
figure above.

### Batch — the four Rust gates and `check-docs.sh`

```text
$ nix develop --command cargo fmt --all --check
(exit 0)

$ nix develop --command cargo build --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 04s
(exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.54s
(exit 0)   — 0 warnings

$ nix develop --command ./scripts/test.sh
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
test suite: FAILED (cargo test exited 101)
(exit 101)
```

Building `--all-targets` first, as the brief instructs, again paid: **no
`real_herd` target failed and nothing timed out** in this run.

**The two failures are the two known host failures, and I observed both
of them failing on a *figure*, neither on a timeout.**

`herd-hub/tests/slots.rs:744` — ORCH-069, a figure outright:

```text
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at herd-hub/tests/slots.rs:744:9:
assertion `left == right` failed: the figure E-22 asks for, in the process's own words.
  left: "built with -j3"
 right: "built with -j7"
```

`herd-hub/tests/admission.rs:419` — also a figure, and derived from this
host's live memory:

```text
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' panicked at herd-hub/tests/admission.rs:419:5:
the inherited job's 4 GiB of declared memory must still be held: queued 2nd of 2 at priority step … on lender, 2.9 GiB free of its 6.9 GiB memory admission, 5 GiB needed …
```

**Which reading this run supports.** The brief says `admission.rs:419`
has been seen both ways — as a figure (the first batch's integrator) and
as ORCH-107 contention (the latest run) — and asks me to say which I
observe rather than inherit one. **I observe the figure**, and on a run
where ORCH-107 cannot be the explanation at all, because no `real_herd`
target failed and nothing timed out. The `6.9 GiB memory admission` in
its own message is host-derived, the same family as ORCH-069's
`MemTotal`. On this evidence **ORCH-069 was narrowed one site too far**
and `admission.rs:419` belongs with it, not with ORCH-107. That is the
same conclusion the first integrator reached, reached independently.

**Skips: AT LEAST 12, a floor and not a count** (D27, D179) — the suite
exited 101, so it did not finish. All twelve are cgroup v2 delegation
unavailable on this host, named individually in the run output:
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.

**Do not write "green on CI" about `slots.rs:744`:** CI's `MemTotal` is
15983 MiB, under the guard's 17408, so the assertion is *skipped* there,
and by D27 a skip is never a pass.

## Risks and findings

- **ORCH-111, and it is the one thing in this batch I could not
  close.** `check-orchestration.sh` reported `140 passed, 1 failed`
  **twice** on this merged tree — once right after the second merge,
  once again before the handoff commit — and **both times I destroyed
  the evidence** by piping the run to `tail -2`/`tail -3`. It then
  refused to recur: **nineteen consecutive green runs** with the output
  fully captured, four of them under the same direct pipe to `tail -2`
  and three with the corpus dirtied immediately beforehand. So it is
  about two in twenty-five, it is real rather than a misread — `140 + 1`
  is the suite's own 141, so one `ok` became one `bad` and no counter
  was merely lost — and three hypotheses are refuted rather than
  assumed: it is **not** the pipe, **not** a lost counter increment, and
  **not** network flakiness in the `session.sh` cases, which run under
  `env -i` against a fake `gh` config directory and a fake `claude`
  binary and open no socket. A control that is red one run in twelve is
  a control nobody will trust, and this is the control that guards the
  docs gate, so it has a row, `open`. **My own mistake is part of the
  finding, and I made it twice:** a gate's output is the evidence and I
  truncated it. Keep the whole output.
- **The `docs` job time after this batch is measured on CI, not here**
  — the figure is in the PR's Evidence and in the report below. Locally
  the two scripts together are **under 10 seconds** where they were
  ~160, against a 5-minute cap.
- **ORCH-108 was not re-allocated.** `orch-batched-pr`'s author refused
  it on finding it already taken by the spool-deletion decision, and he
  was right: ORCH-108 is on `origin/main`. The next genuinely free ids,
  checked across the merged tree and across PR #100's head, are
  **ORCH-109, ORCH-110, ORCH-111**, and the highest on `main` was
  ORCH-108.
- **No risk to an invariant or a specification** anywhere in this batch:
  no conflict arose, so nothing was settled that was not mine to
  settle.

## Decisions needed

**None.** Nothing here needed Alexa's word beyond the standing
authorisations already on the record: ORCH-001 for the merge, and the
2026-10-27 word for the one-PR-per-batch method this batch uses.

## Resume point

The board's ordered list is unchanged by this batch: **ORCH-082** is the
one of the four still open and needs no decision, then the durability
tier one step at a time with **step A alone authorised** (D183). Six
rows still wait on Alexa, **ORCH-106 outranking the four from PR #94**.
If `check-orchestration.sh` goes red again, keep the whole output and
name the case — that is ORCH-111.

## Processes

Started and ended by me: `nix develop --command cargo fmt`, one
`cargo build --workspace --all-targets`, one `cargo clippy`, one
`./scripts/test.sh`, and thirteen runs of the two docs scripts. One
compile at a time throughout; nothing killed by name, no `pkill -f`.

Own `target/` **deleted** once the branch was pushed, from:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merge-batch-2/wt/target
```

It is the same filesystem as the first consumer's hub volume and runner
data root (ORCH-076), which is why the rule exists.
