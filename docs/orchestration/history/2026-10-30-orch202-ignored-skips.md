# orch202-ignored-skips — handoff 2026-10-30

- **Item:** ORCH-202
- **Agent:** herd-step
- **Branch / commit:** `orch202-ignored-skips` / `6876fae` plus this file,
  based on origin/main `1a8c1ae` (batch 9). Worked through the night of
  2026-10-29 to 2026-10-30; every figure below carries its own timestamp
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The gate's skip accounting now sees a test skipped at **compile time** as
well as one skipped at run time. Before this, `scripts/test.sh` built its
list from the `HERD-SKIPPED` lines a test writes **while running**, and a
test carrying `#[ignore]` never runs, so it wrote nothing: batch 9 added
one and the twelve-name list and its `sha256` did not move. An `#[ignore]`
was a silent exemption from D27.

**Two kinds, two lists, never merged.** Cargo prints a compile-time skip on
stdout in its own words — `ignored, <reason>`, or a bare `ignored`.
`scripts/test.sh` now pipes cargo's stdout through `tee`, parses those
lines and emits:

| marker | meaning |
|---|---|
| `HERD-SKIPPED: <name>: <why>` | unchanged. A test ran and said *this host cannot check it* |
| `HERD-IGNORED: <name>: <reason>` | the test never ran. *Nobody is checking it until someone acts* |
| `HERD-IGNORED-UNEXPLAINED: <name>` | `#[ignore]` with no reason. Refused; no expectation file can match this marker |

They are printed in **separate blocks** with separate counts, and the run
ends with an `ignored census: COMPLETE|INCOMPLETE` marker, the compile-time
counterpart of `test suite: PASSED`: a run that did not finish did not
print every binary's `ignored` lines either, so its list is a floor.

`scripts/gate.sh` step 7 now has two halves, `(a)` runtime and `(b)`
compile-time, compared **separately** and each by **name** (D187), and the
step's verdict covers both. `(b)` reads the new
`scripts/expected-skips/ignored.txt` — **one** file, not one per host,
because `#[ignore]` is a property of the tree and the same on every
machine, so `ci.txt` needs no copy of it and cannot drift from it.
`--bless` adopts both halves, and refuses the compile-time one outright
while any `#[ignore]` carries no reason.

### How each of the six conditions is met

1. **The list, never the count** (D187). `(b)` compares the set of names
   from `comm`, exactly as `(a)` does; every figure it prints says the
   count is not the signal. Three cases drive it, including one where a
   third unexpected `#[ignore]` is red while the two expected ones are
   still present.
2. **A reason is required, and it reaches the list.** The reason is carried
   on every `HERD-IGNORED` line, into `ignored.txt`, and into the gate's
   output when a name is unexpected. A bare `#[ignore]` gets its own marker
   and the gate is **red** on it — I agree it should be a refusal and made
   it one. It cannot be blessed away: `--bless` refuses to write the file
   at all while one exists, and no expectation line can match the marker.
   The reason is **not compared**, only required: comparing it would redden
   the gate on a reworded sentence, which is not a loss of coverage. That
   is a judgement, and the alternative is in *Risks* 4.
3. **The two are distinguishable.** Different markers, different blocks,
   different counts, different expectation files, different receipt fields,
   two labelled halves of step 7. Two cases prove they cannot stand in for
   one another: a name on the skip list does not excuse the same name
   ignored, nor the other way round.
4. **The expectation says which kind each entry is** — in the marker on the
   line, not in the filename. Converting one into the other therefore moves
   a line between two files **and** changes its marker: a visible change in
   both, never a wash. A case asserts the blessed file says so in its own
   header.
5. **`scripts/test.sh` still works run directly.** Its exit status is still
   cargo's own, `$PIPESTATUS[0]` through the `tee`; stderr is left where it
   was; both new blocks print to the terminal. Fifteen `test.sh` cases run
   it directly with no gate involved. Why the exit status did **not**
   become non-zero on a reasonless `#[ignore]`: the gate reads
   `test suite: PASSED` to decide whether the suite was complete, so a
   non-zero exit here would make step 6 call the *suite* incomplete and the
   skip list **unknown** — destroying the whole skip report over a question
   that is not about completeness. The refusal lives in the gate instead,
   and `test.sh` prints it loudly.
6. **`skip-list-sha256` is unchanged**, and deliberately so: I did not
   touch `skip_names()`, the observed-skip grep, `local.txt`,
   `local-under-load.txt` or `ci.txt`. The figure is still over the
   `HERD-SKIPPED` names **alone**, so every figure an older receipt quoted
   still compares. The compile-time half gets a **new** field,
   `ignore-list-sha256`, and a case asserts both are present and separate.

### The fixtures, and what each proved

A detector only ever seen silent is not known to fire, so each was watched
firing before the zero was trusted.

- **Known-bad, against real cargo.** A throwaway crate in the scratchpad —
  **no `.rs` file in this repository was touched** — with three tests:
  `#[ignore = "..."]`, a bare `#[ignore]`, and one whose *reason* contains
  the text ` ... ignored, ` to trap a parser that takes the last match
  instead of the first. The parser lifted out of the shipped `test.sh` was
  run on that cargo output. All three fired, each classified correctly, and
  the name of the trap case was not shifted by its own reason. This also
  settled the one assumption the design rests on: the `ignored` lines are
  on cargo's **stdout**.
- **Known-bad, against the gate.** Thirty cases in
  `scripts/check-orchestration.sh`: an unexpected `#[ignore]` is red and
  named with its reason; a reasonless one is red **even when the
  expectation file lists it**; a missing `ignored.txt` is refused rather
  than assumed; a suite with no complete census is not read as a list;
  `--bless` refuses a reasonless one. Each was watched going red.
- **No leak.** The parser was run on cargo's `test result: ... 1 ignored;`
  summary lines alone. It produced nothing, so a summary line cannot be
  mistaken for an assertion.
- **Known-good, the tree as it stands.** The mechanism reported **39**
  `HERD-IGNORED` lines and **0** unexplained. I counted the tree
  independently — every `#[ignore]` attribute and the `fn` after it, by
  `awk` over the nine files `rg` named — and got **39**. The two sorted
  name lists are **identical**, and they are **disjoint** from the 14
  `HERD-SKIPPED` names of the same run.

**Which tool, and that it counts.** `rg` (ripgrep 15.2.0, from the flake's
PATH) for every file and attribute count; `awk` for the name extraction;
`grep` (ugrep 7.8.4) only for fixed-string line matching inside the scripts
themselves. The counts were proved two independent ways and agree: `rg -c`
per file sums to 39, and the `awk` census that reads the `fn` name after
each attribute yields 39 rows. Neither figure was taken on trust from the
other.

## The full inventory of `#[ignore]`, and which should not exist

**39 attributes, 9 files, 0 without a reason.** Ranked by how much their
existence costs. **I removed none** — that is yours to weigh.

| rank | what | how many | why it is where it is |
|---|---|---|---|
| 1 | `spool_offer_answered.rs` — `every_swept_spool_the_hub_acknowledges_is_deleted_and_none_set_aside` | 1 | **The one that should not exist.** Not slow, and it needs no host shape: it reproduces an *unfixed* defect — 37.6 GiB held on the first consumer's runner — and is RED on `main` by design. Its `#[ignore]` is a decision to ship a known defect, and before tonight it was also the thing keeping the gate quiet about it. It stops existing when ORCH-192 is fixed, not by deleting the attribute |
| 2 | `crash_points.rs` — ORCH-080's enumeration | 10 | **Real assertions, outside the gate purely for runtime.** Torn records, an interrupted mint, a stale temporary at open, an event ahead of the record: the 308-state census. These are the ten whose absence actually loses coverage, and the only question about them is cost — see the measurement below |
| 3 | `announced_job_ceiling.rs` — `a_link_cut_while_the_runner_is_full_loses_no_job_at_the_deployed_sixty_seconds` | 1 | A real assertion about the **deployed** 60 s threshold, ignored for a measured 171 s of wall clock. One test, one number, and the cheapest of the three to bring back in |
| 4 | the benches and experiments: `junction_breakdown.rs` 6, `review_pass_cost.rs` 6, `cost_surface.rs` 4, `unsynced_write_cost.rs` 4, `walk_cost.rs` 4, `sync_prices.rs` 3 | 27 | **These should exist.** They assert nothing — `"a bench; run it on purpose"`, `"an experiment, not an assertion"`. Un-ignoring them would turn the gate into a benchmark runner and buy no coverage at all |

So of 39, **twelve** are assertions nobody makes (1 + 10 + 1) and
**twenty-seven** are measurements that assert nothing. That split is the
finding, not the total: a count of 39 reads far worse than the truth, and a
count of 12 would read far better than the truth about `crash_points.rs`.

## Not done, and why

- **The `crash_points.rs` enumeration's runtime: NOT MEASURED.**
  `./scripts/bench-preflight.sh` refused it twice, and the second refusal
  was on **load**, not on the cgroup floor:

```
load (1m):   10.73 on 12 cores, ceiling 9.00
bench-preflight: load 10.73 is at or above 9.00 on 12 cores: two heavy
agents are already running, or the consumer's production is busy. Wait, or
run a read-only task instead
bench-preflight: REFUSED, do not start the heavy phase.
```

  The brief gives latitude over the **cgroup floor** and I used it, with
  the figures, once. It gives none over the load ceiling, and a timing
  figure taken at load 10.73 on a 12-core host shared with the consumer's
  production would not be the number the decision needs anyway. So the
  number that would decide whether those ten could ever be in the gate is
  **owed**. The command, on a quiet host:
  `cargo test -p herd-hub --test crash_points -- --ignored`, under
  `nix develop --command`.
- **No `#[ignore]` was removed, and no `.rs` file was touched** — as the
  brief directs. The ranking is above; the decision is the owner's.
- **`report-skips.sh` and `.github/workflows/ci.yml` were not changed**:
  outside my write paths. *Risks* 3 says what that leaves asymmetric.
- **`docs/decisions.md` and `docs/orchestration/board.md` were not
  touched.** A `D-NEXT` draft is below.

## Gates

All four Rust gates plus both docs checks were run on the committed tree,
in the worktree `/mnt/ssd_980/herd-project/.claude/worktrees/orch202`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch202`.

```
$ nix develop --command cargo fmt --all --check
fmt rc=0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.37s
build rc=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.26s
clippy rc=0

$ nix develop --command ./scripts/test.sh
ignored census: COMPLETE (39 ignored, 0 without a reason)
test suite: PASSED
suite rc=0
  -- cargo's own totals, summed over its 64 `test result:` lines:
     passed=942 failed=0 ignored=39

$ ./scripts/check-orchestration.sh
check-orchestration: 315 passed, 0 failed
  -- main measures 285; the +30 are this task's, 15 on test.sh and 15 on
     gate.sh, and each was watched going red before it went green

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

**The last run, named and not quoted** (workflow.md: the docs gate is the
last thing run, after this file is committed, before the push, so its
output cannot be in the tree it gated): bare `./scripts/gate.sh` on this
branch's tip, from this worktree and never from the shared checkout, its
log at `target/herd-gate/gate-<sha>.log` and its receipt at
`target/herd-gate/<sha>.receipt`. The verbatim output is in the agent's
report.

**Skips.** Twelve runtime skips, the at-rest list of
`scripts/expected-skips/local.txt` exactly, every one a cgroup-delegation
or IO-ceiling capability this host does not have. Both flood benches
**ran** this time (load 2.09 at the suite's start), so neither
load-conditional name appears:

```
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job (the named memory outcome)
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device branch)
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces (the cgroup branch of the report)
```

`skip-list-sha256` over those twelve sorted names is
`0fbdadcaf14aac20eb9875f41aa74b11349d7aee9df39d3dc93be5b714ba66f9` —
**unchanged**, the same figure receipts on this host have carried. Nothing
in this change touches `skip_names()`, the observed-skip grep, `local.txt`,
`local-under-load.txt` or `ci.txt`.

**And thirty-nine compile-time skips**, which no gate run before this one
reported at all: the `scripts/expected-skips/ignored.txt` list exactly, 0
without a reason. `ignore-list-sha256` over those 39 sorted names is
`fb94da45470f85c2100d4d80f8cf5e537cae27cac6c3822d1d2450587d3ce9a8`, a new
field beside the old one and not replacing it.

## Risks and findings

1. **The brief's census of `#[ignore]` was incomplete, by a factor of
   three.** It names two other populations — `crash_points.rs` ten,
   `announced_job_ceiling.rs` one — for twelve in all with the spool one.
   The tree carries **39**, in **nine** files: six bench and experiment
   files hold 27 more that the brief does not mention. Nothing in the
   design depended on the figure, but the hole was three times the size it
   was described as, and `ignored.txt` would have been wrong had I taken
   twelve on trust.
2. **The board's ORCH-202 row proposes the opposite shape to the brief's,
   and I followed the brief.** The row's "cheap shape" is to write the
   ignored names *into* `HERD_SKIP_LOG` "in the same
   `HERD-SKIPPED: <name>: <reason>` form". The brief's condition 3 forbids
   exactly that, and the brief is right: the two facts are different and
   `local.txt` is per host while `#[ignore]` is not. Whoever closes the row
   should know its own recommendation was not taken, and why.
3. **`report-skips.sh` does not annotate the compile-time half, and
   `ci.yml` does not know about it.** Both are outside my write paths, so
   this is a finding and not an edit. Consequence: on GitHub Actions the
   `#[ignore]` block is in the log but produces no `::warning` annotation
   and no line in the run summary, where every runtime skip produces both.
   The **gate** is complete — it does its own grep of the suite output and
   does not go through `report-skips.sh` — so nothing a landing depends on
   is weaker. It is the CI *presentation* that is now asymmetric.
4. **The reason is required but not compared, by choice.** So an
   `#[ignore]`'s reason can be silently reworded — including from a real
   reason to a meaningless one — without the gate noticing, as long as it
   is non-empty. Comparing it is a two-line change (`ignore_names` would
   stop cutting at the colon) and it is strictly stronger; I did not take
   it because it reddens the gate on an edited sentence, which is a cost
   the owner should weigh, not me. If the answer is "compare it", the place
   is `ignore_names()` in `gate.sh`.
5. **`#[ignore]` is counted from cargo's output, so a test filtered out is
   not in the census.** A narrowed run (`test.sh -p herd-proto`) prints
   `ignored census: INCOMPLETE` for exactly this reason, and the gate
   refuses to read an incomplete census as a list. A run that is complete
   cannot miss one: every test binary announces its own ignored tests.
6. **The parser assumes no test name contains ` ... ignored`.** Rust test
   paths carry no spaces, and a doc-test name (`src/lib.rs - Foo::bar
   (line 10)`) carries no `...`, so the first match is the marker. A
   *reason* that contains the marker is handled, and there is a case for
   it. This is the same class of coupling as ORCH-150: the gate now depends
   on libtest's output wording, which is stable but not a contract.
7. **`test.sh` now pipes cargo's stdout.** Consequence: under a direct run
   stdout is no longer a terminal, so libtest prints no colour. Under the
   gate it never was one. Nothing in the suite is known to branch on
   `isatty`, and a test that did would already have behaved this way under
   every gate run.

### Two things about D27's enforcement weaker than the brief describes

8. **`skip-list-sha256` is locale-dependent, and always has been.** The
   figure is `sort "$seen" | sha256sum`, and `sort`'s collation comes from
   the environment. `gate.sh` sets no collation locale anywhere;
   `check-docs.sh` sets `LC_ALL` in five places, deliberately, for exactly
   this class of reason. Measured on this host, over the **identical**
   twelve names:

```
en_US.UTF-8 : 0fbdadcaf14aac20eb9875f41aa74b11349d7aee9df39d3dc93be5b714ba66f9
LC_ALL=C    : 2911f1e8cdbaaaa3005e5318bda0f46016cc359b6aefec879b771c65e16a3c0e
```

   The two differ only in where `a_tighter_enclosing_...` sorts against
   `an_allocation_...` and `an_io_weight_...`: UTF-8 collation ignores the
   underscore, C does not. So **two correct gate runs on the same tree, in
   different locales, print different figures over the same list** — and a
   gate run from a stripped environment (`env -i`, as a hook or a service
   would give it) prints the C one. I found this because my own harness
   runs under `env -i` and disagreed with my hand computation. **I changed
   nothing**: one `LC_ALL=C` on each sort fixes it and *changes the
   published figure*, which is precisely the thing the brief told me not to
   do quietly. It is in *Decisions needed*. My new `ignore-list-sha256`
   inherits the same weakness, knowingly, so that the two fields stay
   comparable with each other.
9. **The `#[ignore]` list mixes two unlike things, and the count reads
   worse than the truth.** 27 of the 39 are benches and experiments that
   assert nothing — `"a bench; run it on purpose"`. They are correct
   `#[ignore]`s, but they sit in the same list as the twelve assertions
   nobody makes, so "39 assertions nobody checks" overstates the hole by
   more than three times. A third kind of marker, or a tag on the line for
   "measures, does not assert", would make the list read right. I did not
   invent one: it is a vocabulary change and the brief gave me six
   conditions, not seven.

### One thing that is not about D27, found while gating

10. **Five tests go red on this host for a reason that is page cache, not
    memory.** The first suite run failed 5 assertions, all one cause:
    `herd-hub/tests/real_herd/mod.rs:1264` asserts
    `enclosure.room_mib() >= 2048`, and `room_mib()` is
    `memory.high − memory.current`, which counts **reclaimable page cache**
    as occupied. At the moment it fired, `/chatons.slice` held 16.4 GiB —
    of which `anon` was **797 MiB** and `inactive_file` **11.9 GiB** —
    `memory.pressure` was `some avg10=0.00 full avg10=0.00`, and the host
    had **22.8 GiB** `MemAvailable`. The build the suite requires is what
    fills that cache, so the suite can red itself. The second run, with a
    warm build, passed all 942. The guard's own comment says "a failure
    here is a broken machine, and it reads like one" — on this host it also
    reads like a full page cache. Netting `inactive_file`, or reading
    `memory.pressure`, would make it measure what it means. **Not my write
    path** (`.rs`), so: a finding, with its figures.

## Decisions needed

1. **`skip-list-sha256`'s locale dependence** (finding 8). Recommendation:
   **pin it** with `LC_ALL=C` on both sorts in `gate.sh`, and record in the
   landing that the published figure for this host's twelve names moves
   from `0fbdadca…` to `2911f1e8…` once and for a stated reason. Fallback:
   leave it, and write in `expected-skips/README.md` that the figure is
   only comparable between runs in the same locale. Either way it should
   not change by accident, which is what can happen today.
2. **Whether any of the twelve unmade assertions should enter the gate**
   (the ranking above). Recommendation: fix ORCH-192 so rank 1 stops being
   ignored, and take rank 3 (`announced_job_ceiling`, a measured 171 s)
   into the gate next, since it is one test and one number. Rank 2's ten
   wait on the measurement this handoff owes. Fallback: nothing moves, and
   the gate now at least names all twelve every run.
3. **Whether the `#[ignore]` list should distinguish "measures" from
   "asserts"** (finding 9). Recommendation: yes, but as its own small task
   with a vocabulary decision, not folded into this one.

### Draft entry for `docs/decisions.md`

```markdown
## 2026-10-30 — D-NEXT A compile-time skip is a skip, and is listed apart

**Context.** D27 and invariant 5 say a skipped assertion is printed as
skipped and never reported as passed, and D187 made the list — never the
count — the thing that decides. Both were enforced only for a skip a
test writes while running. A test carrying `#[ignore]` never runs and
writes nothing: at batch 9's landing one was added on purpose and the
gate's twelve-name list and its sha256 did not move, so `#[ignore]` was a
silent exemption from the one rule this project treats as
non-negotiable. The tree carried 39 of them, in 9 files.

**Choice.** `scripts/test.sh` turns cargo's own `ignored` lines into
`HERD-IGNORED: <name>: <reason>` and ends with an `ignored census:`
marker; `scripts/gate.sh` step 7 has two halves, compared separately and
each by name, the compile-time one against
`scripts/expected-skips/ignored.txt` — one file, because `#[ignore]` is a
property of the tree and not of the host. The two kinds are never merged:
`HERD-SKIPPED` says *this host cannot check it*, `HERD-IGNORED` says
*nobody is checking it until someone acts*, and the marker on the line
carries the kind, so converting one into the other is a visible change in
two files and never a wash. An `#[ignore]` with no reason gets
`HERD-IGNORED-UNEXPLAINED`, which no expectation file can match and
`--bless` refuses to adopt: an assertion nobody makes must say why.
`skip-list-sha256` is untouched and still over the `HERD-SKIPPED` names
alone, so every figure an older receipt quoted still compares;
`ignore-list-sha256` is new beside it.

**Cost.** 30 cases in `check-orchestration.sh`, 285 to 315. `test.sh`
pipes cargo's stdout through `tee`, so a direct run loses libtest's
colour. D27, D187, invariant 5, ORCH-202.
```

## Resume point

**The branch is NOT pushed: the last gate run went RED, and it went red on
the host and not on this change.** `./scripts/gate.sh` on `d018610`,
2026-10-29T23:02:00Z, 9 m 26 s:

```
step 0..5 — ok      (check-docs 2s, check-orchestration 84s, nix flake
                     check 2s, cargo fmt 2s, cargo clippy 11s)
step 6: test suite — FAILED (exit 101, 465s)   901 passed, 41 failed
step 7: the skips, both kinds — ok             12 matched, 39 matched
gate: RED sha=d0186103c09945ecbd51081f2bcdb829b7ce96f0 failed=6:test-suite
```

All 41 failures are one cause, and it is a true statement about the host:

```
Error: --host-memory-floor is 1024 MiB and this host lends 462 MiB, so
nothing could ever be admitted here.
```

The runner measured 462 MiB of lendable memory because `/chatons.slice`
had filled to within ~460 MiB of its 18 GiB `memory.high` while the gate's
own earlier steps ran. **`bench-preflight.sh` refused this run twice and
was right both times, and I overrode its cgroup floor both times.** Its
message says exactly what happened — *"the suite grows about 4 GiB more in
the same cgroup as it runs. The run would go red on a true statement about
the host"* — and my reason for overriding it (that `anon` was only
~800 MiB against ~11 GiB of reclaimable `inactive_file`) was **wrong in a
way worth writing down**: the product's own D119 measurement reads the
same stock figure preflight reads, so whatever preflight predicts is
exactly what the runner will measure. A small `anon` does not help,
because nothing in the product looks at `anon`. The 8 GiB floor is not
over-caution; it is the suite's own headroom.

So the precondition for the retry is **preflight passing on its own
terms**, with no override:

1. Wait until `./scripts/bench-preflight.sh` is `ok` unaided — it needs
   8 GiB of room under `memory.high` on `/chatons.slice`, and load under
   9. Right now the slice holds 15.1 GiB, so there is 3.3 GiB.
2. Then, from this worktree and never from the shared checkout, with
   `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch202` (left in place and
   warm, deliberately — see *Processes*):
   `./scripts/gate.sh`
3. On `gate: GREEN`, push `orch202-ignored-skips`. Nothing else is owed:
   the four Rust gates, both docs checks and step 7 are green on this
   content, and the suite itself passed **942 passed, 0 failed, 39
   ignored** at 2026-10-30T00:05Z on `6876fae`, whose only difference from
   the gated tip is this handoff file.
4. Then `herd-merger` takes the branch into the batch, allocates the
   `D-NEXT` above and closes ORCH-202 — noting that the row's own proposed
   shape (fold the ignores into `HERD_SKIP_LOG` as `HERD-SKIPPED`) was
   **not** taken, and why (*Risks* 2).
5. Still owed, on a quiet host: the enumeration's runtime,
   `cargo test -p herd-hub --test crash_points -- --ignored` under
   `nix develop --command`.
6. Decision 1 above, before some run in a stripped environment prints
   `2911f1e8…` and somebody reads it as a changed skip list.

## Processes

- The suite (three times: twice standalone, once inside the gate), `cargo
  fmt`, `cargo build`, `cargo clippy`, `check-docs.sh`,
  `check-orchestration.sh` and `./scripts/gate.sh` — each a child of this
  agent's own shell, started and reaped by it. **No PID of any other agent
  and none of the consumer's production was signalled.** The consumer's
  `herd-hub` (3006543) and `herd-runner` (3021002) were read out of `ps`
  while judging host load and **left alone**; nothing was killed by name
  or by pattern, and nothing under `/sys/fs/cgroup` was written.
- Build tree `/mnt/ssd_980/herd/build/orch202`: **kept, on purpose.** The
  branch is not pushed, so workflow.md's "delete your own `target/` once
  your branch is pushed" has not come due, and a warm tree is what makes
  the retry above cost 9 minutes instead of a cold rebuild — which would
  itself put ~8.5 GiB of fresh page cache into the very cgroup whose
  fullness caused the red. Whoever gets the green push runs
  `rm -rf /mnt/ssd_980/herd/build/orch202`.
