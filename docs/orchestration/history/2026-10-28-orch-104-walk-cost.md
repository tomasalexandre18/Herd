# orch-104-walk-cost — handoff 2026-10-28

- **Item:** ORCH-104
- **Agent:** herd-step
- **Branch / commit:** `orch-104-walk-cost`, one commit, amended once to
  carry this file's final text; the pushed sha is in the agent's report,
  based on origin/main `09229db`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The two walks are measured at five directory counts, cold and warm, in a
release build and a debug one. No product file is touched.** The brief's
two populations — 3 334 and 33 340 — plus 1 000, 10 000 and 100 000, the
last chosen so that the table meets D152's own 100 000-directory figure
head on instead of being scaled towards it.

### What was built, and where

`herd-hub/tests/walk_cost.rs`, 858 lines, four cases, **every one
`#[ignore]`d** so the workspace suite is unchanged. It is a measurement
file in the shape `herd-hub/tests/junction_breakdown.rs` and
`herd-hub/tests/cost_surface.rs` already established, and like them it
asserts no product behaviour.

```sh
export TMPDIR=<a directory on a volume with room>   # never the root fs
nix develop --command cargo test -p herd-hub --test walk_cost \
  --release -- --ignored --nocapture --test-threads=1
```

`HERD_WALK_COST_MAX=33340` drops the 100 000 population, which is the one
that costs **327 s and ~1.3 GB** to build; a capped run finishes in a
couple of minutes and says in its own output which populations it did
**not** measure, so a short run can never be mistaken for a full one.

The four cases: `what_a_walk_costs_against_the_directory_count` (§1),
`what_dominates_a_record_walk` (§2, §2b, §3), `the_eviction_is_real`
(the cold-cache check), `the_order_a_walk_reads_in` (§3b).

**Nothing under `herd-hub/src`, `herd-runner/src`, `herdctl/src` or
`herd-proto/src` is modified, and no `Cargo.toml` either.** The two walks
are called through the public `JobStore` trait. The breakdown is a
*replica* of the store's private `read_stored`, rebuilt from public types
only — `JobRecord`, `LabelSet`, `RECORD_VERSION` — making the same three
`serde_json::from_str` calls over the same text in the same order. The
replica's total is printed beside the real walk's every time, so its
faithfulness is a figure and not a claim: **replica/real is 0.93 to 0.98
warm at every population.**

### The fixture, and that it is ours

A `tempfile::tempdir()` state root opened with `JobDirStore::open`, one
template job minted and finished through the store, then `T` synthetic
job directories each holding a stamped copy of that record — the method
`cost_surface.rs` and `junction_breakdown.rs` use, so the three campaigns
measure the same shape of disk.

Two deliberate departures from those two, both to stop the corpus from
being easier than the real one:

* **One record in ten carries a 240-entry manifest** (~35 kB of JSON
  against ~500 bytes for a plain one). The consumer's own output
  distribution is deciles 1–8 at 0.0 MB with the top tenth carrying 95 %
  of the bytes, so a corpus of identical records would make the
  per-record cost uniform *by construction* and the deciles meaningless.
  Mean record size in the fixture is 3.5 kB.
* **One directory in ten holds bytes** under `files/`, twelve files of
  1 KiB — the shape the sweep's narrow pass actually pays for. D152 built
  100 000 directories of which 1 000 held bytes; one in ten is the
  heavier case.

**No real state root was read or copied.** The consumer's runner data root
(`drwx------ herd:herd`) was not touched, nothing of the chatOnS project
was touched, and nothing was read on any machine but this workstation
(invariant 9, D161).

### Cold means something here, and it was checked

A cold figure nobody verified is a warm figure. Every record file is
`fsync`ed as it is written, and a cold pass is preceded by
`posix_fadvise(POSIX_FADV_DONTNEED)` over every one of them, which drops
their clean page-cache pages. **Phase 3 checks that this works on this
host** rather than assuming it: 500 records, each read immediately after
its own eviction and then again.

```
  one record's open+read      D1      D2      D3      D4      D5      D6      D7      D8      D9 |     max      mean  max/D5
  (microseconds per record)
  after DONTNEED           18.61   19.02   19.36   19.77   20.38   34.93   69.46   71.20   74.55 |  124.93     41.18    6.13
  immediately again         3.04    3.06    3.09    3.11    3.13    3.16    3.21    3.27    5.02 |    6.42      3.37    2.05

  median after eviction 20.4 us against warm 3.1 us: ratio 6.50
```

**The eviction is real: 6.5×.** What it does *not* drop is the dentry and
inode caches, which need `/proc/sys/vm/drop_caches` and therefore root —
and this host also runs the first consumer's production, so a global cache
drop is their incident and not ours to cause. **So every "cold" figure
below is a page-cache cold and is a floor, not a ceiling: a hub restarted
onto a state root it has never read is colder still.** That is stated
rather than hidden, and it matters, because the startup pass §3.4 proposes
runs on exactly that colder cache.

## The figures

### 1. Both walks against the directory count

Release build. `collections_owed` is the full record walk — one `open` +
`read` + three parses per record — and is the profile §3.4 says a
retention pass would have. `sweep_outputs` is the narrower pass the store
already does, which under budget reads **no record at all** (`[C5]`): one
`stat` of `files.gone/` and one `opendir` of `files/` per directory.
Median of five warm runs, one cold run per cell.

| dirs | sweep warm | sweep cold | cold/warm | record walk warm | record walk cold | cold/warm | record per dir warm | record per dir cold |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 000 | 3.65 ms | 5.77 ms | 1.58 | 19.42 ms | 86.95 ms | 4.48 | 19.42 µs | 86.95 µs |
| 3 334 | 13.76 ms | 19.77 ms | 1.44 | **65.64 ms** | **291.63 ms** | 4.44 | 19.69 µs | 87.47 µs |
| 10 000 | 39.62 ms | 59.48 ms | 1.50 | 189.59 ms | 873.44 ms | 4.61 | 18.96 µs | 87.34 µs |
| 33 340 | 136.62 ms | 204.81 ms | 1.50 | **639.07 ms** | **3.290 s** | 5.15 | 19.17 µs | 98.68 µs |
| 100 000 | 619.78 ms | 1.738 s | 2.80 | 1.993 s | 15.213 s | 7.63 | 19.93 µs | 152.13 µs |

Host load, 1-minute, read by the test at each phase: **2.87–4.83** for the
four smaller populations and **8.08–10.90** for the 100 000 row, whose
fixture build spanned a burst of other agents' work. The 100 000 row is
therefore the only one taken under load, and it is the only one whose
per-directory *cold* figure is out of line (152 µs against 87–99 µs) —
consistent with D136's finding that this host has no IO latency target and
stalls under IO pressure (`/proc/pressure/io` read `full avg60=21.81`
during that window). **Its warm figure is not out of line at all**, which
is itself evidence: see *load sensitivity* below.

### 2. What dominates, instrumented and not inferred

Release build, warm, per population. `readdir` is one
`durable::names_in` of `jobs/`; the per-record rows are a replica of
`read_stored` timed in two halves.

| dirs | readdir | per-record open+read | per-record parse ×3 | per-record total | replica/real | parse share of per-record | one parse instead of three |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 000 | 258.7 µs (1.4 %) | 3.44 ms (18.3 %) | 15.04 ms (80.3 %) | 18.47 ms (**98.6 %**) | 0.96 | **81.4 %** | saves 3.87 ms (20.7 %) |
| 3 334 | 847.6 µs (1.4 %) | 11.26 ms (18.3 %) | 49.51 ms (80.4 %) | 60.77 ms (**98.6 %**) | 0.93 | **81.5 %** | saves 14.23 ms (23.1 %) |
| 10 000 | — (1.4 %) | 34.54 ms (18.7 %) | 147.79 ms (79.9 %) | 182.33 ms (**98.6 %**) | 0.95 | **81.1 %** | saves 41.52 ms (22.4 %) |
| 33 340 | 9.08 ms (1.4 %) | 120.56 ms (18.6 %) | 520.12 ms (80.0 %) | 640.68 ms (**98.6 %**) | 0.98 | **81.2 %** | saves 159.00 ms (24.5 %) |

And the same question **cold**, where the answer inverts:

| dirs | open+read cold | parse ×3 cold |
|---:|---:|---:|
| 1 000 | 64.00 ms (**76.4 %**) | 19.74 ms (23.6 %) |
| 3 334 | 160.38 ms (**74.0 %**) | 56.32 ms (26.0 %) |
| 10 000 | 452.82 ms (**71.5 %**) | 180.09 ms (28.5 %) |
| 33 340 | 1.668 s (**74.6 %**) | 568.05 ms (25.4 %) |

For scale, at 33 340: a per-record `stat` alone is **49.12 ms**, and the
sweep's whole per-directory job is **124.46 ms** — a fifth of the record
walk's 640 ms.

### 2b. The same breakdown in a debug build, which ORCH-123 asked for

ORCH-123's row says to *"measure the 55.6 µs on a release build as well
as a debug one, because three `serde_json` parses of one small document
is exactly the kind of cost a debug build exaggerates"*. It does, and
here is by how much. Debug, same fixtures, `HERD_WALK_COST_MAX=33340`.

| | debug | release | debug ÷ release |
|---|---:|---:|---:|
| real walk, 3 334 dirs | 626.10 ms | 66.12 ms | **9.5×** |
| real walk, 33 340 dirs | 6.162 s | 661.52 ms | **9.3×** |
| per-record `read+parse`, median | 53.67 µs | 6.73 µs | **8.0×** |
| per-record `open+read`, median | 5.71 µs | 3.03 µs | 1.9× |
| per-record `parse ×3`, median | 47.88 µs | 3.69 µs | **13.0×** |
| parse share of per-record | **96.5 %** | **81.2 %** | — |
| `readdir` share of the walk | 0.2 % | 1.4 % | — |
| one parse instead of three saves | **48.1 %** | **24.5 %** | 2.0× |
| cold: `open+read` share | 39.6 % | 74.6 % | — |

**ORCH-123's 55.6 µs a record is reproduced almost exactly, and it is a
debug figure.** The row's own number came from one caller over 1 001
directories at 55.66 ms, i.e. 55.6 µs a record; the debug median here is
**53.67 µs** at 33 340 and **55.41 µs** at 3 334. That is a clean
independent corroboration of the row's measurement — and it confirms
that the figure describes a build the consumer does not run.

**The consequence for anyone sizing the work: debug doubles the apparent
value of the fix.** Collapsing three parses into one saves **48 %** of a
debug walk and **24 %** of a release one, because what debug exaggerates
most is precisely the per-parse overhead the redundancy duplicates. And
in debug the parse dominates even **cold** (60 % against 40 %), while in
release the cold walk is disk-bound (25 % against 75 %) — so a debug
build hides the whole of *Risks and findings* 3.

### 3. The deciles, because the mean describes no record

Per-record cost, warm, microseconds, at 33 340 directories:

```
  per-record distribution      D1      D2      D3      D4      D5      D6      D7      D8      D9 |     max      mean  max/D5
  (microseconds per record)
  open+read                 2.88    2.92    2.95    2.98    3.03    3.12    3.29    3.78    6.22 |   27.05      3.62    8.93
  parse x3                  3.62    3.64    3.66    3.67    3.69    3.73    3.82    4.28   28.23 |  206.10     15.60   55.79
  read+parse                6.53    6.57    6.61    6.66    6.73    6.85    7.02    8.08   31.86 |  215.35     19.22   32.00
  one parse                 2.30    2.34    2.35    2.37    2.38    2.40    2.42    2.67   28.10 |  161.37     10.83   67.80
  stat only                 1.29    1.33    1.35    1.37    1.40    1.42    1.46    1.57    1.72 |    9.31      1.47    6.67
  sweep per dir             2.21    2.22    2.24    2.26    2.27    2.30    2.35    2.54   14.05 |   31.16      3.73   13.71
```

**The distribution is skewed and the skew is the manifest.** Deciles 1–8
of `read+parse` are 6.53–8.08 µs, decile 9 is 31.9 µs, the maximum is
215 µs, and the **mean, 19.22 µs, is 2.9× the median**. Arithmetic on the
same figures: the nine tenths of records that are plain cost about
30 006 × 6.8 µs ≈ 204 ms of the 640.68 ms total, so **the top tenth
carries about two thirds of the walk** while being a tenth of the records.
That is the same shape as the consumer's byte distribution, and it is why
the deciles were asked for.

**The honest reading of "mean versus deciles" here, said precisely,
because it is not the usual one.** For a *total* the mean is the correct
multiplier — a walk's cost **is** `N` × mean, by definition — so the mean
is not the wrong statistic for sizing a pass. What it is wrong for is
describing a record, and for predicting what happens when the record-size
mix changes: a corpus of uniformly tiny records costs about **1.8×** less
per record than this one, and that single factor is most of the gap
between the derived figures and the measured ones.

Cold, the tail is far worse: at 33 340 the cold `open+read` maximum is
**20 488 µs** — one record's read stalling 20 ms — against a median of
24.46 µs, a ratio of 838×. That is this host's IO under pressure (D136)
and it is a property of the host, not of the walk.

### 3b. The order the walk reads in halves the cold cost

Phase 4, release, load 2.2–2.8. Every record read twice over, once in
the order `durable::names_in` hands back — which is **what the store
actually does** — and once in sorted id order, with the sequence
reversed afterwards so that neither order gets the benefit of having run
second.

| dirs | names in the same position | cold, readdir order | cold, id order | id ÷ readdir | reversed: readdir ÷ id | warm, readdir | warm, id |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 3 334 | 3 of 3 335 | 233.63 ms | **116.95 ms** | **0.50** | 1.83 | 10.77 ms | 10.61 ms (0.99) |
| 33 340 | **0 of 33 341** | 2.654 s | **1.101 s** | **0.41** | 2.01 | 111.37 ms | 113.69 ms (1.02) |

**Reading the records in id order costs half to two fifths of what
reading them in directory order costs, on a cold cache, and nothing at
all warm.** The two orders share essentially no positions — zero of
33 341 — because `names_in` returns ext4's directory-hash order, which is
unrelated to the creation order the files were laid out in. Warm the
difference vanishes, which is the control: it confirms the effect is the
disk and not the loop.

**What that is worth, in the figure the engine cares about.** A cold
record walk is its reads plus its parses, and only the reads are affected
by order: at 33 340, 2.654 s of readdir-order reads plus 0.568 s of
parses is 3.22 s, which is the 3.29 s §1 measured through the store; the
same walk in id order is 1.101 s plus 0.568 s, about **1.67 s**. So
**an id-ordered cold pass is ~49 % cheaper than the current one** — a
larger saving than the 24 % ORCH-123's parse fix buys warm, on the pass
that actually delays a hub coming up.

This also explains a discrepancy in §2 that would otherwise sit
unexplained: the replica tracks the real walk to within 2–7 % **warm**
but comes out at 2.24 s against 3.29 s **cold**, because the replica
walks id order and the store walks directory order. The replica is
faithful; the orders are not the same.

### 4. Load sensitivity, which two readings settle

The warm figures were taken twice, at different loads, by two phases that
do not share code: phase 1 times `collections_owed` through the trait,
phase 2 times it again as its own control.

| dirs | phase 1 warm (load 4.6–4.8) | phase 2 warm (load 9.0–9.6) | apart |
|---:|---:|---:|---:|
| 3 334 | 65.64 ms | 66.12 ms | 0.7 % |
| 33 340 | 639.07 ms | 661.52 ms | 3.5 % |

**The warm walk is load-insensitive between load 3 and load 10 on this
host**, so no warm figure above needs re-running at rest. The cold figures
are the opposite and the 100 000 cold cell shows it; it is marked.

## Not done, and why

* **No product file touched, by design.** Nothing under `herd-hub/src`,
  `herd-runner/src`, `herdctl/src`, `herd-proto/src`, and no manifest.
  The measurement never needed one: the two walks are public trait
  methods and the record parse is reproducible from public types.
* **`/proc/sys/vm/drop_caches` was not used**, so dentry and inode caches
  are warm in every "cold" figure. It needs root, and the first
  consumer's production shares this host. The cold column is therefore a
  floor; the true startup cost is above it by an unmeasured amount.
* **The 100 000-directory row was taken under load 8–11**, not at rest,
  because the fixture build spanned another agent's heavy phase. It is
  marked in the table, its warm figure is corroborated by the flat
  per-directory column, and its **cold** figure — 15.2 s, the most
  dramatic number here — should be re-taken at rest before anyone quotes
  it. It is a bonus population: both populations the brief named were
  taken at load 2.9–4.8.
* **§2's breakdown has no 100 000 row at all**, and that is a decision
  rather than an omission: the second build of that fixture was killed
  (see *Processes*) after it had run for twenty minutes on a disk shared
  with the consumer's production. The parse share it would have reported
  was 81.4, 81.5, 81.1 and 81.2 % at the four populations below it, so
  the missing row would have confirmed a constant, not revealed
  anything. The figure that *is* missing and would have been worth
  having is its **cold** split.
* **§2b's debug table stops at 33 340** (`HERD_WALK_COST_MAX=33340`), for
  the same reason and with the same justification: debug at 3 334 and
  33 340 already agree to within 0.1 percentage point on every share.
* **Nothing was measured about a *concurrent* walk.** Every figure is one
  caller, single-threaded. `04a-cost-surface.md` case 2 and `06` phase 1f
  own the multi-caller question and this task did not re-open it.
* **No board row, no decision entry, no `docs/status.md` edit.** Proposed
  text is below; the board is `herd-merger`'s and `docs/status.md` is at
  its 90-line limit with no headroom (`CLAUDE.md` is at 150/150 too, so
  neither file can take a line without losing one).
* **`scripts/gate.sh` is not fixed**, although this branch's first gate
  run found a real defect in it (*Risks and findings* 6). It is method
  code, a change to it needs a `herd-reviewer` pass by `CLAUDE.md`, and
  it is outside this task's write paths. Proposed as ORCH-N4.
* **No expectation was edited to make a gate green.** No `--bless`, no
  new name on `scripts/expected-skips/local.txt` or
  `local-under-load.txt`.

## Gates

`./scripts/check-orchestration.sh`, before any change, on `09229db`:

```
check-orchestration: 256 passed, 0 failed
```

and again after the documentation change:

```
check-orchestration: 256 passed, 0 failed
```

**256, not the 257 the brief expected**, and 256 is the right figure —
see *Risks and findings* 1.

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```
    Checking herdctl v0.3.7 (…/wt-orch-104/herdctl)
    Checking herd-hub v0.3.7 (…/wt-orch-104/herd-hub)
    Checking herd-runner v0.3.7 (…/wt-orch-104/herd-runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.65s
```

`nix develop --command cargo fmt --all --check`: exit 0, no diff.

`nix develop --command cargo build --workspace`:
`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 10.40s`.

`nix develop --command ./scripts/test.sh`, on this tree before the
commit, at load 0.7 rising to 3.2:

```
test suite: PASSED
```

Summed over every test binary: **842 passed, 0 failed, 38 ignored**, and
**12 assertions skipped**.

**The 4 cases this task adds are in the `ignored` column and not in the
`passed` one, and the suite's own output proves it rather than my saying
so:**

```
     Running tests/walk_cost.rs (…/debug/deps/walk_cost-e8a8477d1e97de1e)

running 4 tests
test the_eviction_is_real ... ignored, a bench; run it on purpose
test the_order_a_walk_reads_in ... ignored, a bench; run it on purpose
test what_a_walk_costs_against_the_directory_count ... ignored, a bench; run it on purpose
test what_dominates_a_record_walk ... ignored, a bench; run it on purpose

test result: ok. 0 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

So **842 is the base commit's own figure** and this branch adds no
passing test; the `ignored` total moved from 34 to 38. The board's last
recorded figure was *838 passed, 12 skipped* on 2026-10-27, since when
four branches have landed on `main` — so 842 is growth on `main` and not
mine.

**The last run, named and not quoted:** `./scripts/gate.sh` on the
committed tree, after this file was committed and before the push — the
full five steps plus the suite and the skip list. Its log goes to
`target/herd-gate/` in this worktree and to a log file named in the
agent's report, and **its verbatim verdict line is in that report**,
because the tree it gated is the tree that is pushed and nothing may be
added to it afterwards (ORCH-121, `templates/handoff.md`).

**That run is the third gate run on this branch, and the first two are
named here so the record is complete rather than tidy.** Run 1 came back
**RED** on `b8c887f` with `failed=7:unexpected-skip` — a load burst
inside the suite, diagnosed in *Risks and findings* 6, not a defect of
this branch and not made green by touching any expectation. Run 2 was
started on that same clean sha and then **invalidated by my own hand**:
I edited this file while it was in flight, so the tree it was measuring
stopped being the committed one, and a gate run on a tree that is not
what gets pushed proves nothing (`gate.sh`'s own rule 3). Its verdict,
whatever it was, is discarded and is not evidence for anything. The
third run is on the tree that carries this paragraph. All three logs are
named in the agent's report.

**Skips (D27): 12, the expected set for this host at rest, and not one
of them is counted as a pass.** The suite printed the banner *"12
assertion(s) COULD NOT RUN on this host and were skipped. A green run
does NOT mean these were checked."* By name:

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
an_io_weight_that_cannot_bite_is_said_out_loud
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces
```

All twelve are the cgroup v2 delegation family this host cannot provide,
all twelve are on `scripts/expected-skips/local.txt`, **none is new** and
this branch adds none. The gate compares names and never a count, which
is why the list is here and the number is only context.

**The four `#[ignore]`d cases are a third thing, named so that nobody
later reads an `ignored` count as a skip list.** Not a skip and not a
pass: a case the suite deliberately does not run, in the shape
`junction_breakdown.rs` and `cost_surface.rs` already use —
`what_a_walk_costs_against_the_directory_count`,
`what_dominates_a_record_walk`, `the_eviction_is_real`,
`the_order_a_walk_reads_in`. They are **not** on the skip list and must
not be put there: a `HERD-SKIPPED` line means *this host could not check
an assertion*, and these assert nothing.

## Risks and findings

**1. The brief's `check-orchestration.sh` figure is wrong, and the
repository already said so.** The brief says *"expect 257 passed, 0
failed"*. The real figure on `09229db` is **256**, before and after my
change. The second measurement that settles it: the string `257 passed`
appears **nowhere** in `docs/`, while
`history/2026-10-28-orch-111-stdin-independence.md` records **ten
consecutive runs** of `check-orchestration.sh`, all `256 passed, 0
failed`. So this is not a regression I caused and not a count that moved;
the brief's number was never a measured one. **Nothing in this branch
changes it.**

**2. ORCH-123 is CONFIRMED, and sharpened in three ways — one of which
cuts its proposed fix to a quarter of its apparent size.**

The row claims *"about 94 % of the `T` walk is per-record CPU — not the
disk and not the lock"*, from a debug figure of 55.66 ms over 1 001
directories (55.6 µs a record), and records its cause — three
`serde_json` parses of one text in `read_stored` — as *"a hypothesis with
an arithmetic bound, not a measurement"* where *"nobody has yet
attributed one microsecond of the 55.6 to it"*.

* **The share is confirmed and is higher than 94 %.** Per-record work is
  **98.6 %** of the warm walk at every population measured; `readdir` of
  `jobs/` is **1.4 %**. The lock is not in it at all — ORCH-082 already
  took it out, and `collections_owed` takes it zero times.
* **The cause is confirmed, and in a release build, which the row asked
  for.** The three parses plus `LabelSet::new` are **81.1–81.5 %** of the
  per-record cost, so about **80 % of the whole warm walk**. The
  microseconds the row said nobody had attributed are now attributed:
  at 33 340 directories, 520.12 ms of parse against 120.56 ms of
  `open`+`read`.
* **But the fix the row sketches buys 23 %, not 80 %.** Collapsing the
  three parses into one — `serde_json::from_str::<RecordFile>` alone,
  which is the shape §3.4 proposes — costs **2.38 µs a record against
  3.69 µs for three** (medians at 33 340), saving **20.7–24.5 % of the
  walk**. The redundancy is real but it is not where the parse time is:
  `VersionProbe` and `DurableLabels` are cheap, because serde skips the
  fields they do not want. **The expensive parse is the one that cannot
  be removed** — building the whole `JobRecord`. Anyone sizing ORCH-123's
  work off "80 % of the walk is the parses" will be disappointed by a
  factor of 3.5; the honest figure is a **quarter of the walk**, and D52's
  ability to read a record whose labels are bad must survive it.
* **One release figure the row would want:** its 55.6 µs a record was
  debug. Release here is **6.5–6.7 µs** a record warm on a plain record
  and **19.2 µs** on the mean of this skewed corpus — so debug
  exaggerates by roughly the 6× `04a-cost-surface.md` already recorded.

**3. ORCH-123's claim is a *warm* claim, and cold it is false.** On a cold
page cache the attribution inverts: `open`+`read` is **71.5–76.4 %** and
the parses **23.6–28.5 %**. This is not a quibble about caches, because
the two passes M12 proposes sit on opposite sides of it: a **daily** pass
on a busy hub reads a warm cache and is CPU-bound, so ORCH-123's lever
applies to it; a **startup** pass reads a cold one and is disk-bound, so
ORCH-123's lever does almost nothing for the figure that actually delays
a hub coming up. **A fix justified by the warm number and measured
against the cold one will look like it failed.**

**4. §2.5's sweep figure was wrong in the *other* direction, and nobody
had noticed.** The bullet derived *"40–65 ms at 3334 directories"* for the
sweep's walk and *"a hundred walks a night is 4–6.5 s"*. Measured, the
sweep's walk is **13.76 ms warm / 19.77 ms cold at 3 334** — a third of
the derived figure — because the derivation scaled D152's
*record-reading* profile onto a pass that reads no records (`[C5]`). A
hundred of them a night is **1.4 s**, not 4–6.5 s. The alarm-storm
complaint in that bullet stands on its own; the cost complaint was
overstated. Both figures are now `[M]` in the document.

**5. The store walks its records in directory-hash order, and that is
not free.** In phase 2 the replica tracks the real `collections_owed` to
within 2–7 % **warm** but comes out well under it **cold** (2.24 s of
replica against 3.29 s of real walk at 33 340). The two do the same
syscalls over the same files; what differs is the order.
`durable::names_in` returns ext4's directory-hash order, effectively
random against the disk, while the replica walks id order, which is
creation order and laid out sequentially.

**Phase 4 measured it, and it is a factor of two: see §3b.** Cold, id
order costs **0.50×** at 3 334 and **0.41×** at 33 340 of what directory
order costs, confirmed by running the pair in both sequences (1.83× and
2.01× reversed) and controlled by the warm pair, where the difference is
nil (0.99× and 1.02×). Zero of 33 341 names occupy the same position in
the two orders.

**This is an argument for §3.4's id-ordered pass that §3.4 does not
make.** Id order was proposed there because it lets the pass **stop
early**; it is also, separately, about **half the cold cost per record**.
A full cold pass at 33 340 is ~3.3 s in the order the store walks today
and ~1.7 s in id order — a bigger saving than ORCH-123's parse fix buys
warm, on the pass that delays a hub coming up. It is also a saving
available to `collections_owed` itself, for nothing: that answer is
advisory and recomputed at every redial (D79), so the order it is built
in changes no behaviour. ORCH-N3 below.

**6. `gate.sh`'s two load readings have a hole in the middle, and this
branch's first gate run fell into it. It is a method defect, not a
defect of this branch, and I did not touch the expectation to make it
go away.**

The first `./scripts/gate.sh` on `b8c887f` came back **RED** on step 7,
`failed=7:unexpected-skip`, with these two lines:

```
    + flood-fifty
    + flood-two-fifty
```

The two flood benches decline a host over a one-minute load of 8 and
they read it as they run. They read **10.12** and correctly skipped
themselves. The gate read the load **twice — 1.70 at step 0 and 3.30
after the suite — and neither is above 8**, so it applied the *at-rest*
list of 12 and called those two skips unexpected. Its own log says so:
*"the at-rest list applies on its own: load1 1.70 at the start and 3.30
after the suite, neither above 8."*

**Both are right, and that is the defect.** `gate.sh`'s design note says
it reads the load twice because *"one reading of the load does not say
what a run was"* and names the case it fixed: *"a run that begins quiet
and becomes busy."* The case it does **not** cover is the one that
happened here — **a burst that begins and ends inside the suite**. I
watched it from outside: load1 went 1.68 → 8.54 → 11.60 → 10.12 → 3.30
between 21:35 and 21:38 while another agent's heavy phase ran, and both
of the gate's sampling points sit outside the burst.

**What I did not do.** I did not run `--bless`, did not add the two
names to `scripts/expected-skips/local.txt`, and did not touch
`local-under-load.txt`. Either would make the gate green by widening the
list of assertions nobody checks, which is exactly what
`expected-skips/README.md` calls *"a decision, not a chore"*, and it
would have hidden a method defect behind my own branch's landing. The
red is reported red.

**What I did instead:** re-ran the same gate on the same clean sha with
the host quiet. Both runs' verdict lines are in the agent's report. A
re-run is not an edited expectation — `gate.sh`'s own header lists
*"flakiness in either direction"* among the things a local gate cannot
cover — but it is also not a fix, and the defect should be a row.

**The fix is not mine to write** (`scripts/gate.sh` is method code and
`CLAUDE.md` sends a change to it through `herd-reviewer`), so it is
proposed as ORCH-N4 below. The shape that would close it: the *benches*
already know the load they refused at and say it in their skip line, so
the gate can read that figure out of the skip itself instead of
sampling the host twice and guessing. A skip that carries *"the host's
one-minute load is Some(10.12), over this bench's ceiling of 8"* is
self-justifying evidence; two samples taken minutes away from it are
not.

**7. A fixture of 100 000 directories costs 327 s to build and ~1.5 GB,
and the teardown is not free either.** Anybody re-running this should
budget ~15 minutes and 2 GB, point `TMPDIR` at a volume with room — never
the root filesystem (`gotchas/build-weight-off-root.md`) — and expect the
100 000 cell to be the one the host's other work contaminates.

## Decisions needed

**None for Alexa.** Nothing here changes what is built or is
outward-facing; it replaces two derived figures with measured ones and
settles a hypothesis this project could settle itself. Three rows for the
orchestrator to place on the board, text ready to paste:

**ORCH-N1 — size ORCH-123 off the measured quarter, not the apparent
80 %.** Amend ORCH-123's row with the release attribution
(per-record 98.6 % of the warm walk, parses 81 % of per-record, one parse
instead of three buying 23 %) and with the fact that the attribution
inverts on a cold cache. The work itself is unchanged and still worth
doing; what changes is its expected size and the number it is measured
against. *Evidence:* this handoff, §2 and *Risks and findings* 2 and 3.

**ORCH-N2 — the startup full pass needs a bound of its own.** A full cold
record walk is ~100 µs a directory on this host: **3.3 s at 33 340 and
15.2 s at 100 000**, all of it before the hub serves. §3.4 proposes a
startup full pass to catch the outliers an early-stopping id-ordered
daily pass would miss, and sizes it off a warm figure. Either the startup
pass gets a cap and resumes, or it runs after the port is bound, or the
daily pass's `K` is chosen so that no outlier needs catching. *Evidence:*
§1's cold column; `disk-policy.md` §3.4.

**ORCH-N3 — every walk in the store reads in directory-hash order, and
sorting the names first halves the cold cost.** Measured: cold, id order
is **0.50×** at 3 334 directories and **0.41×** at 33 340 of what
`durable::names_in` order costs, with the pair run in both sequences and
a warm control showing no difference (§3b). The change is one `sort()`
after `names_in`. **Two call sites are safe by inspection and the third
is not mine to clear:**

* `collections_owed` (`store.rs:2715`) — the answer is a `Vec` its
  caller consumes with no lock held, advisory by construction and
  recomputed at every redial (D79), so the order it was built in is not
  observable.
* `sweep_now` (`store.rs:766`) — it overwrites a running total it is
  explicitly allowed to let drift (D152), and the four eviction keys are
  applied by a `sort_by` *after* the walk. That `sort_by` is stable, so
  walk order survives only as the tie-break between two records equal on
  all four keys **including `ended_at` to the nanosecond** — which is why
  this is "safe by inspection" and not "provably irrelevant".
* `recover` — **not cleared here.** It builds the pending queue, and
  whether queue order depends on the order records were read is a
  question for whoever takes the row, not an assumption to inherit from
  this one.

A full cold pass at 33 340 goes from ~3.3 s to ~1.7 s. *Evidence:* §3b;
*Risks and findings* 5. Related to ORCH-021, which owns that call's
shape, and to ORCH-098, which inherits the cost.

**ORCH-N4 — `gate.sh` is red on a load burst that starts and ends inside
the suite.** It samples load1 at step 0 and after the suite and allows
the load-conditional skip list when either is above 8; a burst wholly
inside the suite is invisible to both, so the two flood benches skip
themselves on a load of 10.12 and the gate calls those skips unexpected.
Measured on this branch's first gate run: `gate: RED
sha=b8c887f74b806bd616c4e9c3ea5b69b4234029a4 failed=7:unexpected-skip
skips=at-rest`, with the gate's own two readings at 1.70 and 3.30.
**This reddens any batch it happens to, for a host property and not a
defect** — which is the failure mode ORCH-120's *one landing per batch*
makes expensive, since one spurious red blocks every branch in the
batch. Recommended shape: read the refused load **out of the skip line
the bench already prints** rather than sampling the host, so the
evidence and the allowance are the same measurement. Needs
`herd-reviewer` (`CLAUDE.md`: a change to `check-docs.sh`, hooks or the
gate gets a review pass). *Evidence:* *Risks and findings* 6; the two
gate logs named in the agent's report.

## Resume point

The next task on this thread is **ORCH-098**, M12's retention engine, and
it now has measured figures instead of derived ones: size the daily pass
against **65.6 ms warm / 292 ms cold at the consumer's 3 334** and
**639 ms warm / 3.29 s cold at ten times that**, and treat the **startup**
pass as the expensive one. Before writing it, read `disk-policy.md` §3.4's
new table and *Risks and findings* 3 here, because the daily pass and the
startup pass are bound by different things.

To reproduce or extend the measurement:

```sh
cd <this worktree>
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-104
export TMPDIR=<a directory on a volume with room>
./scripts/bench-preflight.sh --target-dir "$CARGO_TARGET_DIR"
nix develop --command cargo test -p herd-hub --test walk_cost \
  --release -- --ignored --nocapture --test-threads=1
```

## Processes

Started and ended, all children of this agent's own shell: four
`cargo test` measurement runs (release phases 1–3, debug phase 2,
release phase 4), `cargo build`, `cargo clippy` and `cargo fmt`
invocations, one bare `./scripts/test.sh`, three `./scripts/gate.sh`
runs, and a few background shells polling `/proc/loadavg` and an output
file.

**One process was killed, by PID, deliberately, and it was mine:**
`kill 2982691`, the release measurement's test binary, while it was
rebuilding the 100 000-directory fixture for a second time under
another agent's heavy phase. It had already produced every figure of §1
and four of the five populations of §2; the fifth would have cost
another twenty minutes of a shared disk for one confirming row. **SIGTERM
by PID, never `pkill`, never `killall`, never by name.** Rust runs no
destructor on a signal, so its `tempfile::TempDir` was left behind —
**1.3 GB, found and removed by hand** at
`…/scratchpad/fixtures/nix-shell.OMgqxo/.tmp1R9d6q`, verified to be a
fixture state root (`hub.lock`, `index`, `jobs`, `readings`,
`store.json`) before it was removed. Nothing else was killed.

Nothing of the consumer's was read or touched: not their hub volume, not
their runner data root, not `/etc/nixos`, `/srv`, a container or a
service.

Own build tree: `/mnt/ssd_980/herd/build/orch-104`, **deleted once the
branch was pushed** — see the agent's report for the confirmation, since
the deletion happens after this file is committed. Fixtures were written
under this agent's own scratchpad `fixtures/` directory, outside the
worktree, and removed with them.
