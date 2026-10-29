# measured-not-cap — handoff 2026-10-29

- **Item:** ORCH-nnn — to allocate; the highest on `main` at the time of
  writing is ORCH-176, and the proposed row is under *Board row* below
- **Agent:** herd-step
- **Branch / commit:** `measured-not-cap` / `052e4a6` (code) plus this
  handoff commit, based on origin/main `5d422bf`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The defect.** The first consumer read `535691 kB` on one runner offer
line out of seventy-five, where the other seventy-four said bytes. The
figure was right and the unit moved. `herd_proto::size::show_bytes`
(`herd-proto/src/size.rs:70`) returns the first unit a figure divides
exactly, walking `[GiB, GB, MiB, MB, KiB, kB]`; `535691000` divides by
1000 and by nothing larger, so it alone took `kB`. That function is
correct for what it is for — a cap an operator *typed* reads back in the
family they typed it in, `200GB` as `200 GB` (D148, D152) — and it is
unchanged. **What was wrong is that a measured figure was being printed
by the formatter built for a typed cap.**

**The measurement's rule, where the runner can have it.**
`herd_runner::size::measured_bytes` (`herd-runner/src/size.rs`) prints
the exact count always, with a binary approximation beside it once the
count stops being readable, and none below a kibibyte: the hub's own
shape, from `herd-hub/src/report.rs` `bytes_said` and
`herd-hub/src/retention.rs` `said_bytes`. `1136` reads
`1136 bytes (1.11 KiB)` in all three.

**The five measured sites in the runner now use it:**

| site | line prints |
| --- | --- |
| `herd-runner/src/sweep.rs:151` — the offer line, the one the consumer read | the spool's weight |
| `herd-runner/src/sweep.rs:166` — the foreign-state-root line | the spool's weight |
| `herd-runner/src/outputs.rs` `not_spooled` (was the inline `format!` at `:251`) | the walked weight |
| `herd-runner/src/outputs.rs` `over_the_cap`, the weight argument (was `:347`) | the walked weight |

**`herd-runner/src/outputs.rs:348` was in the brief's list and is NOT
changed, deliberately.** It is `show_bytes(cap)` — the figure someone
typed after `--job-output-cap`. The brief's own test settles it ("did a
human type this figure, or did we measure it?") and its own prohibition
covers it ("Any **cap** call site anywhere"), so the rule won over the
line number. `over_the_cap` now says the two figures with two
formatters, and its doc comment says why. See *Risks and findings*.

**One small refactor, for testability only.** The spool-failure sentence
was an inline `format!` inside `collect`'s `Err` arm, reachable only by
arranging the filesystem to refuse a rename — which would have pinned
the line through whatever errno that arrangement produced. It is now
`pub(crate) fn not_spooled(weight, count, error)` beside `over_the_cap`,
same text, and the `Err` arm calls it. No behaviour changed.

**Golden text, against literals and never as a round trip.** Ten tests,
all new, all in the three touched files. Each expectation is a literal
string; the figure each one pins is beside it.

```
herd-runner/src/size.rs
  the_consumers_own_figure_prints_as_bytes_and_never_as_a_round_kb
      measured_bytes(535_691_000) == "535691000 bytes (510.87 MiB)" --
      the consumer's own figure, by name.
  a_figure_that_divides_by_a_thousand_does_not_change_unit
      six figures of the defect's class, each against its literal
      string: 1000, 2000, 1000000, 535691000, 1000000000,
      200000000000.
  a_small_figure_carries_no_approximation_and_a_large_one_does
      the hub's shape at the KiB boundary: 999, 1023, 1024, 1136.
  a_typed_cap_still_reads_back_in_its_own_family
      the OTHER formatter, pinned so a later tidy cannot "unify" the
      two without a red gate: 200 GB, 64 MiB, 2 GiB.

herd-runner/src/sweep.rs
  the_offer_line_says_a_measured_weight_in_bytes_not_in_a_round_kb
      the whole offer line, literal, at 535691000.
  no_measured_weight_changes_unit_because_it_divides_by_a_thousand
      five such figures through the offer line; each asserts the exact
      count is there and the kB it used to take is not.
  the_foreign_state_root_line_says_a_measured_weight_in_bytes
      the whole line, literal but for the throwaway directory the test
      made.

herd-runner/src/outputs.rs
  the_cap_refusal_says_a_typed_cap_in_its_family_and_a_measured_weight_in_bytes
      the whole refusal, literal, with BOTH figures: "535691000 bytes
      (510.87 MiB)" for the weight and "200 GB" for the cap.
  a_weight_that_divides_by_a_thousand_keeps_its_bytes_in_the_cap_refusal
  the_unspooled_refusal_says_its_measured_weight_in_bytes
      the class at the other two sites.
```

**Each failure message argues against the wrong repair.** A round trip
through the formatter would stay green through exactly the change that
breaks a reader — the writer and the reader moving together — so every
assertion spells the line out, and every message says that pasting new
output over the expectation is the repair that is always wrong here.

**Nothing a human typed changed its shape.** `show_bytes` is untouched;
every cap site in `herd-runner/src/confine.rs` (lines 294, 310, 357–359,
389–391, 526–530), `herd-hub/src/main.rs:164` and
`herd-runner/src/outputs.rs:348` still calls it; and the pre-existing
test that asserts the cap refusal contains `show_bytes(cap)` is still
green unchanged:

```
herd-runner/src/outputs.rs
  over_the_cap_nothing_comes_back_and_the_refusal_carries_the_figures
```

## Not done, and why

- **The owed fold** of `herd-hub/src/report.rs:514` `bytes_said` and
  `herd-hub/src/retention.rs:801` `said_bytes` — identical bodies,
  deliberately duplicated — into one function. Out of scope by the
  brief: both files are in branches waiting to land, and a conflict
  there would cost more than the duplication does. **This change makes
  it a third copy** (`herd_runner::size::measured_bytes`), which raises
  the price of leaving it. The natural home when it is done is a second
  function in `herd-proto/src/size.rs` beside `show_bytes` — the two
  rules side by side, with the doc comment that says which figure wants
  which — and that is also the one place a reader would look for them.
- **`herd-proto/src/size.rs` unchanged**, as the brief requires:
  `show_bytes` is correct for a typed cap and is neither changed nor
  deleted.
- **The other 28 `show_bytes` call sites** across the three binaries
  were not re-triaged. The brief named the runner's measured ones and
  that is what was fixed.
- **No `docs/decisions.md` entry.** The brief did not ask for one. If
  the orchestrator wants the rule written down — *a typed figure reads
  back in its typed family, a measured figure reads back exactly* —
  that is a `D-NEXT` entry somebody should brief, and it would be the
  place to record the owed fold too.

## Gates

**Run before this commit**, on the committed code tree `a90f4de`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/measured-not-cap`:

```
$ ./scripts/bench-preflight.sh
target dir:  …/wt-measured-not-cap/target
filesystem:  /mnt/ssd_980, 323 GiB free
load (1m):   8.09 on 12 cores, ceiling 9.00
memory:      21 GiB available, floor 2
bench-preflight: ok, safe to start.
```

```
$ nix develop --command cargo fmt --all --check
(no output; exit 0)
```

```
$ nix develop --command cargo test -p herd-runner --lib -- size:: sweep::tests outputs::tests
running 28 tests
…
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 168 filtered out
```

All ten new tests are in that 28 and all ten passed; the full list is
in the agent's report.

A second `./scripts/bench-preflight.sh` at the start of the gate phase
**REFUSED** (`load 12.18 is at or above 9.00 on 12 cores`), so the heavy
phase waited rather than started — it is a gate, not advice. The gate
ran once the preflight passed.

**The last run, named and not quoted**, as this template requires — it
is made on the tree that includes this file:

- **Command:** `./scripts/gate.sh`, bare (not nested in
  `nix develop`: nesting panics `confinement.rs:2529` on `SUN_LEN`),
  run from the worktree root with
  `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/measured-not-cap`.
- **Where its output goes:** the receipt `gate.sh` step 8 writes under
  `target/herd-gate/` in the worktree, named for the gated sha, and a
  full log in this session's scratchpad as `mnc-gate.log`. The verdict
  line, the skip list and `check-orchestration`'s count are quoted
  verbatim in the agent's report, with the absolute paths spelled out
  there.

**Skips.** No skip is introduced by this change: the ten new tests are
unconditional and none prints `HERD-SKIPPED`. The suite's own skip list
is whatever `gate.sh` step 7 names against
`scripts/expected-skips/local.txt`; it is reported by name in the
agent's report and **never counted as a pass** (D27).

## Risks and findings

1. **The brief's site list named one cap as a measurement.**
   `herd-runner/src/outputs.rs:348` is `show_bytes(cap)` — the
   `--job-output-cap` figure — and it shares its `format!` with
   `:347`'s weight, which is probably how the two came to be listed
   together. It is **not** changed. This is the one place the brief's
   list and the brief's rule disagreed, and the rule won; if the
   orchestrator meant the cap to change too, that is a decision about
   whether a typed cap stops reading back in its typed family, which
   contradicts D148/D152 and is not a step's call.
2. **Two hand-written fixtures still carry the old shape.**
   `herd-proto/tests/round_trip.rs:220` and
   `herd-hub/src/store.rs:5497` each hold the literal string
   `"this job's output is not in its record: 1 GiB in 3 file(s), and
   this runner's output cap is 8 MiB"`. They are opaque payloads — a
   refusal carried through serialization and through the store — not
   assertions about `over_the_cap`, so both are still green and both
   are still valid test data. But a reader of those files would now
   infer a weight shape the runner no longer produces. Cosmetic, out of
   this brief's scope, worth a line in whatever task next touches them.
3. **The third copy of the measured rule.** Named under *Not done*. The
   risk is drift: three bodies that must stay identical, with only
   `size::tests::a_typed_cap_still_reads_back_in_its_own_family` and
   the hub's own tests to notice if one moves.
4. **Nothing mechanical stops the next author reaching for
   `show_bytes`** at a measured site. The doc comments on both
   functions now state the test in one question, and
   `over_the_cap`'s doc states the asymmetry, but that is prose and not
   a lint.
5. **Several assertions depend on `{:.2}` rounding of `f64`.**
   `510.87`, `976.56`, `953.67`, `1.95`, `186.26`, `1.11`, `523.14` —
   each was computed independently before the code was written and each
   matched what the suite produced. None is near a half-way case.
6. **`check-orchestration.sh` measured 269 passed / 0 failed on this
   branch, not the 285 the brief expected of an unlanded branch.** The
   brief gave 269 as `main`'s figure and said an unlanded branch takes
   it to 285; this branch measures the same 269 as `main`. The script's
   case count does not move for a branch that adds only a handoff —
   whatever takes it to 285 is something this change does not do (it
   writes no landing record, no `docs/decisions.md` entry and no board
   row). The figure is reported as measured, not as expected.

## Board row

Proposed, for `herd-merger` to place with its allocated number:

| ORCH-nnn | **A measured byte count was printed by the formatter built for a typed cap, so one runner offer line in seventy-five said `535691 kB` where the other seventy-four said bytes.** `show_bytes` returns the first unit a figure divides exactly, which is right for a cap an operator typed (D148, D152) and wrong for a figure we measured. `herd_runner::size::measured_bytes` is the measurement's rule — exact count always, binary approximation beside it, the hub's own shape — and the runner's five measured sites use it. Every cap site untouched, `herd-proto/src/size.rs` untouched. | reported by the first consumer; fixed by `herd-step` on branch `measured-not-cap`, 2026-10-29 | herd-step | done | Ten new tests pin the lines against **literal** expected strings, not round trips: `535691000` by name, and the whole 1000-dividing class at each site. **Owed:** fold `herd-hub/src/report.rs:514` `bytes_said` and `herd-hub/src/retention.rs:801` `said_bytes` — and now `herd_runner::size::measured_bytes` — into one function, once the branches holding those two files have landed; `herd-proto/src/size.rs` beside `show_bytes` is the home. | — | `history/2026-10-29-measured-not-cap.md`; `herd-runner/src/size.rs`; `herd-runner/src/sweep.rs`; `herd-runner/src/outputs.rs` |

## Decisions needed

**One, and it is not blocking.** Should the rule be written into
`docs/decisions.md` — *a figure a human typed reads back in the family
they typed it in; a figure Herd measured reads back exactly, with a
binary approximation beside it* — together with the owed fold?
**Recommendation:** yes, as a `D-NEXT` entry in whichever batch lands
the fold, so the entry and the single function arrive together.
**Fallback:** leave it in the two doc comments, where it is now, and
accept that prose in two crates is what keeps the next author from
reaching for the wrong formatter.

## Resume point

Nothing is owed on this branch. For whoever continues:

```sh
cd /mnt/ssd_980/herd-project
git log --oneline origin/main..measured-not-cap   # two commits
```

The next act is `herd-merger`'s: this branch goes into the batch, and
the batch's own `./scripts/gate.sh` at integration is the run that
authorises the landing. The owed fold is a separate task and wants the
branches holding `herd-hub/src/report.rs` and
`herd-hub/src/retention.rs` landed first.

## Processes

- One background PID of my own: a shell loop re-running
  `./scripts/bench-preflight.sh` until it passed, started and ended by
  me. No other process started.
- **PID 3046930, the stray `herd-hub` on `127.0.0.1:18461`, was left
  alone** and that port was not taken. No `pkill -f`, no `killall`, no
  kill by name.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/measured-not-cap`, outside
  the worktree and off the root filesystem. It and the worktree's own
  `target/` (which holds only `herd-gate/`'s receipt) are deleted once
  the branch is pushed; the receipt path is quoted in the agent's report
  before the deletion.
